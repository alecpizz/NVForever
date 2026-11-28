//! ITR NVSE, batch 2 (#1011): the dialogue menu's entries and rules, speech and
//! gestures, fake hits and combat, the path and navmesh questions, ground and terrain,
//! doors and references by distance, perk eligibility, recipes, menus and the rest
//! [documented: each page in the wiki export; docs/spec-notes/ext-itr.md §7-12]. What the
//! probe set `itr` showed in the game (itr-nvse.dll installed) is marked [observed: itr
//! probes]; what a page leaves open and no probe checked is marked [hypothesis]. What the
//! engine draws or simulates (gestures, ragdolls, impacts, the barter filter) is kept for
//! it to take; nothing in the engine takes it yet (follow-up), so each queue is bounded.

use crate::combat::{HitInfo, LimbHit};
use crate::game::{Game, RunCtx};
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// A gesture an actor plays (Gesture), for the engine's head bone.
#[derive(Debug, Clone, PartialEq)]
pub struct Gesture {
    /// "nod", "shake" or "tilt".
    pub kind: String,
    /// Peak angle in degrees, total seconds, seconds a cycle.
    pub amplitude: f64,
    pub duration: f64,
    pub speed: f64,
    /// `Game::time` when it started.
    pub started: f64,
}

/// A ragdoll asked for (Ragdoll, RagdollLimb), for the engine's physics.
#[derive(Debug, Clone, PartialEq)]
pub struct RagdollAsk {
    pub actor: FormId,
    /// The limb, -1 the whole body.
    pub limb: i64,
    pub push: [f64; 3],
    pub spin: f64,
    pub duration: f64,
    pub flags: i64,
}

/// ITR's batch-2 session state (cleared by a load).
#[derive(Debug, Clone, Default)]
pub struct Itr2State {
    /// AddDialogueTopicEntry's entries (prompt, id) for the dialogue menu now shown.
    pub topic_entries: Vec<(String, u32)>,
    /// SetDialogueTopicEntryAlpha's alphas by entry index, for the menu now shown.
    pub entry_alpha: BTreeMap<usize, i64>,
    /// SetDialogueTopicHidden's and SetDialogueTopicOrder's rules, by topic or info.
    pub hidden: Vec<FormId>,
    pub order: Vec<(FormId, i64)>,
    /// The dialogue menu's entries are being built (ITR:OnDialogueMenuBuild runs).
    pub building: bool,
    /// ForceCombatTarget's targets by actor.
    pub forced_targets: BTreeMap<FormId, FormId>,
    pub gestures: BTreeMap<FormId, Gesture>,
    /// ForceReload's actors, FakeImpact's (reference, weapon, material) and the ragdolls,
    /// for the engine to take.
    pub reloads: Vec<FormId>,
    pub impacts: Vec<(FormId, FormId, Option<u8>)>,
    pub ragdolls: Vec<RagdollAsk>,
    /// SetRaceAlt's races by reference (not saved [documented: "Not save baked"]).
    pub races: BTreeMap<FormId, FormId>,
    /// The barter menu's sale filter: the form list and whether it is a whitelist.
    pub barter_filter: Option<(FormId, bool)>,
    /// StartNewGame was called: the engine goes to a new game.
    pub start_new_game: bool,
    /// ToggleAllPrimitives: primitives and markers shown.
    pub primitives_shown: bool,
}

/// AddDialogueTopicEntry's limits [documented: "between 0 and 16777215", "Up to 16"].
const MAX_ENTRY_ID: i64 = 0xFF_FFFF;
const MAX_ENTRIES: usize = 16;
/// SetDialogueTopicHidden's and SetDialogueTopicOrder's limit [documented: "Up to 32"].
const MAX_RULES: usize = 32;
/// Gesture's actors at once [documented: "Up to 32 actors"].
const MAX_GESTURES: usize = 32;
/// The most asks kept in each engine queue (ForceReload, FakeImpact, Ragdoll); the
/// oldest goes first [decision: no consumer takes them yet, so a script calling every
/// frame must not grow them for the whole session].
const MAX_QUEUED: usize = 64;

/// `x` at the back of an engine queue of at most [`MAX_QUEUED`].
fn queue<T>(q: &mut Vec<T>, x: T) {
    if q.len() >= MAX_QUEUED {
        q.remove(0);
    }
    q.push(x);
}
/// The cover functions' largest radius [documented: "no greater than 8192"] and
/// GetCoverPointsInRadius's most points [documented: "limited to 128 points"].
const MAX_COVER_RADIUS: f64 = 8192.0;
const MAX_COVER_POINTS: usize = 128;
/// How high above a reference the ground casts start [hypothesis: the page's "offset by
/// an internal start value" is not given].
const CAST_START: f32 = 64.0;
const CAST_DEPTH: f32 = 100_000.0;
/// The height above a cover point the threat's sight line is cast to [hypothesis: a
/// crouching body's chest].
const COVER_CHEST: f32 = 48.0;

/// The perk condition functions ITR reads itself [documented: GetPerkEligibility,
/// "Supported parsed condition functions"], by condition index (opcode − 0x1000)
/// [observed: schemas/script/functions.json].
const GET_ACTOR_VALUE: u16 = 14;
const GET_BASE_ACTOR_VALUE: u16 = 277;
const GET_PERMANENT_ACTOR_VALUE: u16 = 495;
const GET_LEVEL: u16 = 80;
const HAS_PERK: u16 = 449;
const GET_ITEM_COUNT: u16 = 47;
const GET_IS_ID: u16 = 72;
const IS_IN_LIST: u16 = 372;
const IS_WEAPON_IN_LIST: u16 = 399;
/// GetWeaponAnimType and IsWeaponSkillType: filters on a weapon's own fields, which
/// GetPerksForForm checks against the form [observed: itr probes, Gunslinger (anim
/// type 4) and Fast Shot (Guns) are given for the 10mm pistol].
const GET_WEAPON_ANIM_TYPE: u16 = 108;
const IS_WEAPON_SKILL_TYPE: u16 = 109;

/// GetPerksForForm's "item, weapon, armour, and combat-oriented entry points": the
/// combat entry points NVForever reads (`crate::perks`), with their names.
const COMBAT_POINTS: [(u8, &str); 19] = [
    (0, "Calculate Weapon Damage"),
    (1, "Calculate My Critical Hit Chance"),
    (2, "Calculate My Critical Hit Damage"),
    (6, "Adjust Limb Damage"),
    (8, "Calculate To Hit Chance"),
    (34, "Calculate Gun Spread"),
    (36, "Modify Enemy Critical Hit Chance"),
    (37, "Reload Speed"),
    (38, "Equip Speed"),
    (40, "Action Point Cost"),
    (43, "Modify Attack Speed"),
    (52, "Knockdown Chance"),
    (53, "Modify Weapon Strength Req"),
    (54, "Modify Aiming Move Speed"),
    (56, "Modify Damage Threshold (defender)"),
    (57, "Modify Chance for Ammo Item"),
    (58, "Modify Damage Threshold (attacker)"),
    (59, "Modify Throwing Velocity"),
    (68, "Modify Item Damage"),
];

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

fn dist(a: [f32; 3], b: [f32; 3]) -> f64 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    f64::from((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt())
}

fn list(g: &mut Game, items: Vec<XVal>) -> XVal {
    let es = items.into_iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, es))
}

fn point(g: &mut Game, p: [f32; 3], more: &[f64]) -> XVal {
    let mut xs: Vec<XVal> = p.iter().map(|&c| XVal::Num(f64::from(c))).collect();
    xs.extend(more.iter().map(|&x| XVal::Num(x)));
    list(g, xs)
}

/// A string map from (key, value) pairs.
fn smap(g: &mut Game, pairs: Vec<(&str, XVal)>) -> XVal {
    let es = pairs.into_iter().map(|(k, v)| (XKey::Str(k.to_string()), v)).collect();
    XVal::Array(g.ext.new_array(ArrayKind::StringMap, es))
}

/// The value of a condition's form parameter, 0 for none.
fn param_form(c: &crate::cond::Condition, k: usize) -> Option<FormId> {
    match c.params.get(k)?.value()? {
        fnv_script::vm::Value::Ref(0) => None,
        fnv_script::vm::Value::Ref(r) => Some(FormId(r)),
        _ => None,
    }
}

fn param_num(c: &crate::cond::Condition, k: usize) -> Option<f64> {
    match c.params.get(k)? {
        fnv_script::vm::ArgValue::Enum(code) => Some(f64::from(*code)),
        v => v.value().map(|v| v.num()),
    }
}

fn cmp_text(c: crate::cond::Cmp) -> &'static str {
    use crate::cond::Cmp;
    match c {
        Cmp::Eq => "==",
        Cmp::Ne => "!=",
        Cmp::Gt => ">",
        Cmp::Ge => ">=",
        Cmp::Lt => "<",
        Cmp::Le => "<=",
    }
}

/// GetPerkEligibility's delta array [documented: its "Delta Array" section].
#[derive(Debug, Default)]
struct Delta {
    given: bool,
    level: Option<f64>,
    av: HashMap<u16, f64>,
    av_delta: HashMap<u16, f64>,
    items: HashMap<FormId, f64>,
    perks_added: BTreeSet<FormId>,
    perks_removed: BTreeSet<FormId>,
}

/// One requirement checked: what the blocker and condition maps report.
#[derive(Debug, Clone)]
struct Req {
    kind: &'static str,
    function: String,
    cmp: &'static str,
    current: f64,
    required: f64,
    arg_form: Option<FormId>,
    arg_value: Option<f64>,
    text: String,
}

/// An entry point's rank, point, function and conditions by tab.
type EntryFilters = (u8, u8, u8, Vec<(i8, crate::cond::Condition)>);

/// A perk's eligibility for the player.
#[derive(Debug, Clone)]
struct Eligibility {
    owned: u8,
    max: u8,
    status: &'static str,
    /// A data gate (trait, hidden, not playable, not a level-up perk) holds it back.
    gated: bool,
    blockers: Vec<Req>,
    indeterminate: Vec<Req>,
}

impl Game<'_> {
    /// SetRaceAlt's race for a reference (`Game::actor_race` reads it first).
    pub(crate) fn itr_race(&self, r: FormId) -> Option<FormId> {
        self.other.itr2.races.get(&r).copied()
    }

    /// ForceCombatTarget's target while it is alive (`Game::combat_target_of`).
    pub(crate) fn itr_forced_target(&self, actor: FormId) -> Option<FormId> {
        self.other.itr2.forced_targets.get(&actor).copied().filter(|&t| !self.is_dead(t))
    }

    /// The dialogue menu's entries as ITR's rules leave them (`Game::build_choices`): the
    /// hidden topics and lines dropped, the ordered ones first by their order (ties and
    /// the rest keeping theirs) [documented: SetDialogueTopicHidden; SetDialogueTopicOrder];
    /// the custom entries and alphas of the last list dropped, then ITR:OnDialogueMenuBuild
    /// runs with the speaker, where scripts add them again [documented: AddDialogueTopicEntry;
    /// hypothesis: the event's parameter].
    pub(crate) fn itr_menu(&mut self, actor: FormId, mut out: Vec<crate::dialogue::Choice>) -> Vec<crate::dialogue::Choice> {
        let s = &self.other.itr2;
        if !s.hidden.is_empty() {
            out.retain(|c| !s.hidden.contains(&c.topic) && !s.hidden.contains(&c.info));
        }
        if !s.order.is_empty() {
            let key = |c: &crate::dialogue::Choice| s.order.iter().find(|(f, _)| *f == c.info).or_else(|| s.order.iter().find(|(f, _)| *f == c.topic)).map(|(_, o)| *o);
            out.sort_by_key(|c| (key(c).is_none(), key(c).unwrap_or(0)));
        }
        self.other.itr2.topic_entries.clear();
        self.other.itr2.entry_alpha.clear();
        self.other.itr2.building = true;
        self.dispatch_event("ITR:OnDialogueMenuBuild", vec![XVal::Ref(actor.0)], None);
        self.other.itr2.building = false;
        out
    }

    /// The engine tells that the player chose a custom entry: ITR:OnDialogueTopicSelected
    /// with the speaker and the entry's id [documented: AddDialogueTopicEntry].
    pub fn itr_topic_selected(&mut self, id: u32) {
        let speaker = self.dialogue.as_ref().map_or(0, |d| d.actor.0);
        self.dispatch_event("ITR:OnDialogueTopicSelected", vec![XVal::Ref(speaker), XVal::Num(f64::from(id))], None);
    }

    fn itr_pos(&self, r: FormId) -> Option<[f32; 3]> {
        if r == FormId::PLAYER_REF {
            return self.world.peek(r).map(|s| s.pos);
        }
        crate::awareness::position(self, r)
    }

    /// The reference's ground below it: the first thing a cast down meets, else the
    /// terrain [documented: GetDistanceToGround].
    fn ground_z(&self, at: [f32; 3], terrain_only: bool) -> Option<f32> {
        let from = [at[0], at[1], at[2] + CAST_START];
        let to = [at[0], at[1], at[2] - CAST_DEPTH];
        let world = if terrain_only { None } else { crate::ray::cast(from, to, crate::ray::CastKind::World) };
        world.or_else(|| crate::ray::cast(from, to, crate::ray::CastKind::Terrain)).map(|d| from[2] - d)
    }

    // ---- perks ----

    fn delta_of(&self, v: Option<&XVal>) -> Delta {
        let Some(XVal::Array(id)) = v else { return Delta::default() };
        let mut d = Delta { given: true, ..Delta::default() };
        let Some(a) = self.ext.array(*id) else { return d };
        let key_form = |k: &XKey| match k {
            XKey::Num(x) => Some(FormId(*x as u32)),
            XKey::Str(s) => {
                let t = s.trim();
                t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).map_or_else(|| t.parse::<u32>().ok(), |h| u32::from_str_radix(h, 16).ok()).map(FormId)
            }
        };
        let entries = |v: &XVal| -> Vec<(XKey, XVal)> {
            match v {
                XVal::Array(i) => self.ext.array(*i).map(|a| a.entries.clone()).unwrap_or_default(),
                _ => Vec::new(),
            }
        };
        let as_num = |v: &XVal| match v {
            XVal::Num(x) => Some(*x),
            _ => None,
        };
        let as_form = |v: &XVal| match v {
            XVal::Ref(r) if *r != 0 => Some(FormId(*r)),
            XVal::Num(x) if *x >= 1.0 => Some(FormId(*x as u32)),
            _ => None,
        };
        for (k, v) in &a.entries {
            let XKey::Str(k) = k else { continue };
            match k.to_ascii_lowercase().as_str() {
                "level" => d.level = as_num(v),
                "av" | "avdelta" => {
                    let into = if k.eq_ignore_ascii_case("av") { &mut d.av } else { &mut d.av_delta };
                    for (code, x) in entries(v) {
                        if let (XKey::Num(c), Some(x)) = (code, as_num(&x)) {
                            into.insert(c as u16, x);
                        }
                    }
                }
                "itemsadded" | "itemsremoved" => {
                    let sign = if k.eq_ignore_ascii_case("itemsadded") { 1.0 } else { -1.0 };
                    for (item, n) in entries(v) {
                        if let (Some(f), Some(n)) = (key_form(&item), as_num(&n)) {
                            *d.items.entry(f).or_default() += sign * n;
                        }
                    }
                }
                "perksadded" | "perksremoved" => {
                    let into = if k.eq_ignore_ascii_case("perksadded") { &mut d.perks_added } else { &mut d.perks_removed };
                    into.extend(entries(v).iter().filter_map(|(_, p)| as_form(p)));
                }
                _ => {}
            }
        }
        d
    }

    fn perk_rank(&self, p: FormId, d: &Delta) -> u8 {
        if d.perks_removed.contains(&p) {
            return 0;
        }
        let r = self.player.perks.get(&p).copied().unwrap_or(0);
        if d.perks_added.contains(&p) {
            r.max(1)
        } else {
            r
        }
    }

    fn player_av(&mut self, code: u16, d: &Delta, which: u16) -> f64 {
        if let Some(&x) = d.av.get(&code) {
            return x;
        }
        let me = FormId::PLAYER_REF;
        let v = match which {
            GET_BASE_ACTOR_VALUE => crate::functions::actor_value_parts(self, me, code).map(|p| p[0]).unwrap_or(0.0),
            GET_PERMANENT_ACTOR_VALUE => crate::functions::actor_value_parts(self, me, code).map(|p| p[0] + p[1]).unwrap_or(0.0),
            _ => crate::functions::actor_value(self, me, code).unwrap_or(0.0),
        };
        v + d.av_delta.get(&code).copied().unwrap_or(0.0)
    }

    /// The name a requirement's text gives its subject.
    fn req_name(&self, function: u16, c: &crate::cond::Condition) -> String {
        match function {
            GET_LEVEL => "Level".into(),
            HAS_PERK | GET_ITEM_COUNT => param_form(c, 0).map(|f| self.data.name(f)).unwrap_or_default(),
            _ => {
                let code = param_num(c, 0).unwrap_or(-1.0);
                u16::try_from(code as i64).ok().and_then(|code| self.data.actor_value_info(code).map(|i| i.name.clone())).filter(|n| !n.is_empty()).unwrap_or_else(|| format!("AV {code}"))
            }
        }
    }

    /// A requirement on the player: `Some` with its pass, `None` when it cannot be
    /// evaluated (an unsupported function with a delta, or one not run on the player).
    fn eval_req(&mut self, c: &crate::cond::Condition, d: &Delta) -> (Option<bool>, Req) {
        let required = match c.rhs {
            crate::cond::Operand::Value(x) => x,
            crate::cond::Operand::Global(Some(g)) => self.world.global(FormId(g)).map(|v| v as f64).unwrap_or(0.0),
            crate::cond::Operand::Global(None) => 0.0,
        };
        let fname = self.data.table.get(c.opcode()).map(|f| f.name.clone()).unwrap_or_else(|| format!("0x{:04X}", c.opcode()));
        let on_player = c.run_on == crate::cond::RunOn::Subject;
        let (kind, current) = match c.function {
            GET_ACTOR_VALUE | GET_BASE_ACTOR_VALUE | GET_PERMANENT_ACTOR_VALUE if on_player => {
                let code = param_num(c, 0).unwrap_or(-1.0) as i64;
                ("ActorValue", u16::try_from(code).ok().map(|code| self.player_av(code, d, c.function)))
            }
            GET_LEVEL if on_player => ("Level", Some(d.level.unwrap_or_else(|| self.player_level()))),
            HAS_PERK if on_player => ("HasPerk", Some(param_form(c, 0).map_or(0.0, |p| f64::from(u8::from(self.perk_rank(p, d) > 0))))),
            GET_ITEM_COUNT if on_player => {
                let have = param_form(c, 0).map_or(0.0, |i| f64::from(self.world.item_count(FormId::PLAYER_REF, i).unwrap_or(0)) + d.items.get(&i).copied().unwrap_or(0.0));
                ("ItemCount", Some(have.max(0.0)))
            }
            // Without a delta the game's own check answers [documented: "Without a delta
            // array, top-level requirements are checked against the engine's own perk
            // requirement evaluation"].
            _ if !d.given => ("Opaque", Some(self.condition_value(c, Some(FormId::PLAYER_REF), "perk requirement"))),
            _ => ("Opaque", None),
        };
        let name = self.req_name(c.function, c);
        let cmp = cmp_text(c.cmp);
        let req = Req {
            kind,
            function: fname,
            cmp,
            current: current.unwrap_or(0.0),
            required,
            arg_form: param_form(c, 0),
            arg_value: if matches!(c.function, GET_ACTOR_VALUE | GET_BASE_ACTOR_VALUE | GET_PERMANENT_ACTOR_VALUE) { param_num(c, 0) } else { None },
            text: format!("{name} {cmp} {required}, have {}", current.unwrap_or(0.0)),
        };
        (current.map(|v| c.cmp.test(v, required)), req)
    }

    /// GetPerkEligibility's answer for one perk (module notes, ext-itr.md §11).
    fn eligibility(&mut self, p: FormId, d: &Delta, flags: i64) -> Option<Eligibility> {
        let def = self.perk_def(p)?;
        let (is_trait, hidden, playable, min_level, max) = (def.is_trait, def.hidden, def.playable, def.min_level, def.ranks.max(1));
        let conds = def.conditions.clone().unwrap_or_default();
        let owned = self.perk_rank(p, d);
        let level = d.level.unwrap_or_else(|| self.player_level());
        let meta = |kind: &'static str, text: &str| Req { kind, function: "PerkData".into(), cmp: "", current: 0.0, required: 0.0, arg_form: Some(p), arg_value: None, text: text.into() };
        let mut blockers = Vec::new();
        let mut scripted = false;
        let owned_max = owned >= max && flags & 8 == 0;
        if owned_max {
            blockers.push(meta("OwnedMaxRank", "Owned at its highest rank"));
        }
        for (on, bit, kind, why) in [(is_trait, 4, "Trait", "A trait"), (hidden, 1, "Hidden", "Hidden"), (!playable, 2, "NonPlayable", "Not playable"), (min_level == 0, 16, "NonLevelUp", "Not offered at level up")] {
            if on && flags & bit == 0 {
                blockers.push(meta(kind, why));
                scripted = true;
            }
        }
        if min_level > 0 && level < f64::from(min_level) {
            blockers.push(Req { kind: "Level", function: "GetLevel".into(), cmp: ">=", current: level, required: f64::from(min_level), arg_form: None, arg_value: None, text: format!("Level >= {min_level}, have {level}") });
        }
        let mut results = Vec::new();
        let mut failed = Vec::new();
        let mut indeterminate = Vec::new();
        for c in &conds {
            let (pass, req) = self.eval_req(c, d);
            match pass {
                Some(ok) => {
                    if !ok {
                        failed.push(req);
                    }
                    results.push((ok, c.or));
                }
                None => {
                    indeterminate.push(req);
                    results.push((true, c.or));
                }
            }
        }
        if !crate::cond::combine(&results) {
            blockers.extend(failed);
        }
        let status = if owned_max {
            "owned_max"
        // A data gate is a blocker like any other: a trait reads "blocked", not
        // "scripted" [observed: itr probes, WildWasteland]. "scripted" (a page's
        // name) is not given until a probe shows when the plugin gives it.
        } else if !blockers.is_empty() {
            "blocked"
        } else if !indeterminate.is_empty() {
            "unknown"
        } else {
            "takeable"
        };
        Some(Eligibility { owned, max, status, gated: scripted, blockers, indeterminate })
    }

    fn req_map(&mut self, r: &Req) -> XVal {
        let missing = match r.cmp {
            ">=" => (r.required - r.current).max(0.0),
            ">" => (r.required - r.current + 1.0).max(0.0),
            _ => 0.0,
        };
        smap(
            self,
            vec![
                ("kind", XVal::Str(r.kind.into())),
                ("function", XVal::Str(r.function.clone())),
                ("comparison", XVal::Str(r.cmp.into())),
                ("currentValue", XVal::Num(r.current)),
                ("requiredValue", XVal::Num(r.required)),
                ("missingValue", XVal::Num(missing)),
                ("argForm", XVal::Ref(r.arg_form.map_or(0, |f| f.0))),
                ("argValue", XVal::Num(r.arg_value.unwrap_or(0.0))),
                ("text", XVal::Str(r.text.clone())),
            ],
        )
    }

    fn reqs(&mut self, rs: &[Req]) -> XVal {
        let items: Vec<XVal> = rs.iter().map(|r| self.req_map(r)).collect();
        list(self, items)
    }

    fn eligibility_map(&mut self, p: FormId, e: &Eligibility) -> XVal {
        let name = self.data.perks.get(&p).map(|d| d.name.clone()).unwrap_or_default();
        let (blockers, indeterminate) = (self.reqs(&e.blockers), self.reqs(&e.indeterminate));
        smap(
            self,
            vec![
                ("perk", XVal::Ref(p.0)),
                ("name", XVal::Str(name)),
                ("ownedRank", XVal::Num(f64::from(e.owned))),
                ("nextRank", XVal::Num(f64::from((e.owned + 1).min(e.max)))),
                ("maxRank", XVal::Num(f64::from(e.max))),
                ("eligible", yes(e.status == "takeable")),
                ("status", XVal::Str(e.status.into())),
                ("blockers", blockers),
                ("indeterminate", indeterminate),
            ],
        )
    }

    /// Every perk, by least level then name (the level-up menu's order).
    fn perks_in_order(&self) -> Vec<FormId> {
        let mut ps: Vec<(u8, String, FormId)> =
            self.data.perks.keys().filter_map(|&id| self.perk_def(id).map(|d| (d.min_level, d.name.to_lowercase(), id))).collect();
        ps.sort();
        ps.into_iter().map(|(_, _, id)| id).collect()
    }

    /// GetPerkConditions' entries for one perk.
    fn perk_conditions(&mut self, p: FormId) -> Vec<XVal> {
        let Some(def) = self.perk_def(p) else { return Vec::new() };
        let (min_level, is_trait, hidden, playable) = (def.min_level, def.is_trait, def.hidden, def.playable);
        let conds = def.conditions.clone().unwrap_or_default();
        let entries: Vec<EntryFilters> = def
            .entries
            .iter()
            .filter_map(|e| match &e.effect {
                crate::data::PerkEffect::EntryPoint(ep) => Some((e.rank, ep.point, ep.function, ep.conditions.clone().unwrap_or_default())),
                _ => None,
            })
            .collect();
        let mut out = Vec::new();
        let gate = |g: &mut Game, kind: &str, function: &str, cmp: &str, value: f64| {
            smap(
                g,
                vec![
                    ("source", XVal::Str("PerkDataRequirement".into())),
                    ("perk", XVal::Ref(p.0)),
                    ("rank", XVal::Num(0.0)),
                    ("entryPoint", XVal::Num(-1.0)),
                    ("entryFunction", XVal::Num(-1.0)),
                    ("tabIndex", XVal::Num(-1.0)),
                    ("rawFunctionId", XVal::Num(if function == "GetLevel" { f64::from(GET_LEVEL) } else { -1.0 })),
                    ("commandOpcode", XVal::Num(if function == "GetLevel" { f64::from(0x1000 + GET_LEVEL) } else { -1.0 })),
                    ("function", XVal::Str(function.into())),
                    ("runOn", XVal::Num(0.0)),
                    ("comparison", XVal::Str(cmp.into())),
                    ("comparisonValue", XVal::Num(value)),
                    ("isOr", XVal::Num(0.0)),
                    ("usesGlobalComparison", XVal::Num(0.0)),
                    ("kind", XVal::Str(kind.into())),
                    ("argForm", XVal::Ref(0)),
                    ("argValue", XVal::Num(0.0)),
                ],
            )
        };
        // [documented: GetPerkConditions, "Perk Data Gates"].
        out.push(if min_level > 0 { gate(self, "Level", "GetLevel", ">=", f64::from(min_level)) } else { gate(self, "NonLevelUp", "PerkData", "", 0.0) });
        for (on, kind) in [(is_trait, "Trait"), (hidden, "Hidden"), (!playable, "NonPlayable")] {
            if on {
                out.push(gate(self, kind, "PerkData", "", 0.0));
            }
        }
        let d = Delta { given: true, ..Delta::default() };
        let mut rows: Vec<(&str, u8, i64, i64, i64, crate::cond::Condition)> = conds.into_iter().map(|c| ("TopLevelRequirement", 0, -1, -1, -1, c)).collect();
        for (rank, point, function, cs) in entries {
            rows.extend(cs.into_iter().map(|(tab, c)| ("EntryPointFilter", rank, i64::from(point), i64::from(function), i64::from(tab), c)));
        }
        for (source, rank, point, function, tab, c) in rows {
            let (_, req) = self.eval_req(&c, &d);
            let (value, global) = match c.rhs {
                crate::cond::Operand::Value(x) => (x, false),
                crate::cond::Operand::Global(Some(g)) => (self.world.global(FormId(g)).map(|v| v as f64).unwrap_or(0.0), true),
                crate::cond::Operand::Global(None) => (0.0, true),
            };
            // xEdit's run-on order: Subject, Target, Reference, Combat Target, Linked
            // Reference [documented: xEdit CTDA "Run On"].
            let run_on = match c.run_on {
                crate::cond::RunOn::Subject => 0.0,
                crate::cond::RunOn::Target => 1.0,
                crate::cond::RunOn::Reference(_) => 2.0,
                crate::cond::RunOn::CombatTarget => 3.0,
                crate::cond::RunOn::LinkedReference => 4.0,
                crate::cond::RunOn::Other(n) => f64::from(n),
            };
            let m = smap(
                self,
                vec![
                    ("source", XVal::Str(source.into())),
                    ("perk", XVal::Ref(p.0)),
                    ("rank", XVal::Num(f64::from(rank))),
                    ("entryPoint", XVal::Num(point as f64)),
                    ("entryFunction", XVal::Num(function as f64)),
                    ("tabIndex", XVal::Num(tab as f64)),
                    ("rawFunctionId", XVal::Num(f64::from(c.function))),
                    ("commandOpcode", XVal::Num(f64::from(c.opcode()))),
                    ("function", XVal::Str(req.function.clone())),
                    ("runOn", XVal::Num(run_on)),
                    ("comparison", XVal::Str(req.cmp.into())),
                    ("comparisonValue", XVal::Num(value)),
                    ("isOr", yes(c.or)),
                    ("usesGlobalComparison", yes(global)),
                    ("kind", XVal::Str(req.kind.into())),
                    ("argForm", XVal::Ref(req.arg_form.map_or(0, |f| f.0))),
                    ("argValue", XVal::Num(req.arg_value.unwrap_or(0.0))),
                ],
            );
            out.push(m);
        }
        out
    }

    /// Whether a one-tab filter's direct form checks name `subject`: `Some(true/false)`,
    /// `None` when the tab has none (GetIsID, IsInList, IsWeaponInList) [documented:
    /// GetPerksForForm].
    fn filter_names(&self, conds: &[&crate::cond::Condition], subject: FormId) -> Option<bool> {
        let mut results = Vec::new();
        for c in conds {
            let flag = |b: bool| f64::from(u8::from(b));
            let weapon = (self.data.sig(subject) == Some(*b"WEAP")).then_some(subject);
            let value = match c.function {
                GET_IS_ID => flag(param_form(c, 0) == Some(subject)),
                IS_IN_LIST | IS_WEAPON_IN_LIST => flag(param_form(c, 0).is_some_and(|l| crate::functions::list_members(self, l).contains(&subject))),
                GET_WEAPON_ANIM_TYPE => match weapon.and_then(|w| self.data.weapon_anim_types.get(&w)) {
                    Some(&t) => crate::functions::actor::weapon_anim_code(t) as f64,
                    None => return None,
                },
                IS_WEAPON_SKILL_TYPE => match (weapon, param_num(c, 0)) {
                    (Some(w), Some(skill)) => flag(self.data.weapon_skills.get(&w).is_some_and(|&s| s as f64 == skill)),
                    _ => return None,
                },
                // A tab with any other condition does not name the form [observed: itr
                // probes, Robotics Expert, whose weapon tab mixes GetIsID with actor
                // tests, is not given for the pistol].
                _ => return None,
            };
            let rhs = match c.rhs {
                crate::cond::Operand::Value(x) => x,
                _ => 1.0,
            };
            results.push((c.cmp.test(value, rhs), c.or));
        }
        (!results.is_empty()).then(|| crate::cond::combine(&results))
    }
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, _ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let base = |g: &Game, f: FormId| crate::functions::base_of(g, f).unwrap_or(f);
    let one = XVal::Num(1.0);
    let zero = XVal::Num(0.0);
    let actor = subject.filter(|&a| g.itr_is_actor(a));
    let v = match name {
        // ---- the dialogue menu ----
        // `prompt id`: while the menu is open (or being built), id 0-16777215, at most 16
        // [documented: AddDialogueTopicEntry]. The engine's menu shows them (follow-up).
        "adddialoguetopicentry" => {
            let id = num(args, 1, -1.0) as i64;
            let s = &g.other.itr2;
            if (g.dialogue.is_none() && !s.building) || !(0..=MAX_ENTRY_ID).contains(&id) || s.topic_entries.len() >= MAX_ENTRIES {
                return Some(Ok(zero));
            }
            g.other.itr2.topic_entries.push((text(args, 0), id as u32));
            one
        }
        // `index alpha`: an entry of the list now shown [documented].
        "setdialoguetopicentryalpha" => {
            let i = num(args, 0, -1.0) as i64;
            let n = g.dialogue.as_ref().map_or(0, |d| d.choices.len());
            match usize::try_from(i).ok().filter(|&i| i < n) {
                Some(i) => {
                    g.other.itr2.entry_alpha.insert(i, num(args, 1, 255.0) as i64);
                    one
                }
                None => zero,
            }
        }
        // `topicOrInfo flag`: hide (1) or drop the rule (0); at most 32 [documented].
        "setdialoguetopichidden" => {
            // Any form is taken, not only topics and lines [observed: itr probes, a
            // reference returns 1 and counts in ClearDialogueTopicOverrides].
            let Some(f) = form(args, 0) else { return Some(Ok(zero)) };
            let h = &mut g.other.itr2.hidden;
            if num(args, 1, 0.0) != 0.0 {
                if !h.contains(&f) {
                    if h.len() >= MAX_RULES {
                        return Some(Ok(zero));
                    }
                    h.push(f);
                }
            } else {
                h.retain(|&x| x != f);
            }
            one
        }
        "setdialoguetopicorder" => {
            let Some(f) = form(args, 0) else { return Some(Ok(zero)) };
            let o = num(args, 1, 0.0) as i64;
            let rules = &mut g.other.itr2.order;
            match rules.iter().position(|(x, _)| *x == f) {
                Some(i) => rules[i].1 = o,
                None if rules.len() < MAX_RULES => rules.push((f, o)),
                None => return Some(Ok(zero)),
            }
            one
        }
        // The rules removed [documented: ClearDialogueTopicOverrides].
        "cleardialoguetopicoverrides" => {
            let s = &mut g.other.itr2;
            let n = s.hidden.len() + s.order.len();
            s.hidden.clear();
            s.order.clear();
            XVal::Num(n as f64)
        }
        // The lines of the entries shown; no array (0) with the menu closed [documented].
        "getdisplayeddialogueinfos" => match g.dialogue.as_ref().map(|d| d.choices.iter().map(|c| XVal::Ref(c.info.0)).collect::<Vec<_>>()) {
            Some(infos) => list(g, infos),
            None => zero,
        },
        // ---- speech and gestures ----
        // `topic [target]`: the topic's first line, whatever its conditions; 1 when said
        // [documented: ForceSay]; an actor that is not present says nothing [documented:
        // "A call on an actor in an unloaded cell does nothing"].
        "forcesay" => {
            let (Some(a), Some(topic)) = (actor, form(args, 0).filter(|&t| g.data.sig(t) == Some(*b"DIAL"))) else { return Some(Ok(zero)) };
            let Some(&info) = g.data.topics.get(&topic).and_then(|l| l.first()) else { return Some(Ok(zero)) };
            if a != FormId::PLAYER_REF && !g.present(a) {
                return Some(Ok(zero));
            }
            let target = form(args, 1);
            g.say_line(a, target, topic, info, if target.is_some() { crate::game::LineKind::SayTo } else { crate::game::LineKind::Say });
            yes(g.speech.iter().any(|s| s.speaker == a && s.info == info))
        }
        // `topicOrInfo`: the line the actor says now is of the topic, or is the line
        // [documented: IsSaying].
        "issaying" => {
            let (Some(a), Some(x)) = (actor, form(args, 0)) else { return Some(Ok(zero)) };
            let speaking = g.speech.iter().any(|s| s.speaker == a && (s.topic == x || s.info == x));
            let in_menu = g.dialogue.as_ref().is_some_and(|d| d.actor == a && d.line.as_ref().is_some_and(|l| l.topic == x || l.info == x));
            yes(speaking || in_menu)
        }
        // `type amplitude duration [speed]`: nod, shake or tilt (any case), the speed 0.4
        // when 0 or not given; one at a time per actor, 32 actors [documented: Gesture].
        "gesture" => {
            let Some(a) = actor else { return Some(Ok(zero)) };
            let kind = text(args, 0).to_ascii_lowercase();
            if !matches!(kind.as_str(), "nod" | "shake" | "tilt") {
                return Some(Ok(zero));
            }
            let now = g.time;
            let gs = &mut g.other.itr2.gestures;
            gs.retain(|_, x| now - x.started < x.duration);
            if !gs.contains_key(&a) && gs.len() >= MAX_GESTURES {
                return Some(Ok(zero));
            }
            // A value that is not a finite number is refused [decision: an endless
            // gesture would hold its slot until a load].
            let (amplitude, duration, speed) = (num(args, 1, 0.0), num(args, 2, 0.0), num(args, 3, 0.0));
            if ![amplitude, duration, speed].iter().all(|v| v.is_finite()) {
                return Some(Ok(zero));
            }
            let speed = Some(speed).filter(|&s| s > 0.0).unwrap_or(0.4);
            gs.insert(a, Gesture { kind, amplitude, duration: duration.max(0.0), speed, started: now });
            one
        }
        // ---- hits ----
        // FakeHit `[attacker] [damage] [weapon] [location] [flags] [skipOnHit]`, FakeHitEx
        // with fatigue and limb damage after the damage [documented]: the damage, else the
        // weapon's, else 1; the hit as combat deals it (OnHit and OnHitWith with the
        // attacker), or the bare damage with skipOnHit. FakeHitEx's limb damage on
        // locations 1-6 [documented: "Location 0 maps to torso (no limb damage)"], as
        // condition points (see docs/spec-notes/ext-itr.md §8).
        "fakehit" | "fakehitex" => {
            let Some(target) = actor else { return Some(Ok(zero)) };
            let ex = name == "fakehitex";
            let (w_at, loc_at) = if ex { (4, 5) } else { (2, 3) };
            let attacker = form(args, 0);
            let weapon = form(args, w_at).map(|w| base(g, w)).filter(|&w| g.data.sig(w) == Some(*b"WEAP"));
            // A damage that is not a finite number counts as none given [decision].
            let mut damage = num(args, 1, -1.0);
            if damage < 0.0 || !damage.is_finite() {
                damage = weapon.and_then(|w| crate::nvse::record_value(g, w, "DATA", "base_damage")).and_then(|v| v.num().ok()).unwrap_or(1.0);
            }
            let loc = num(args, loc_at, 0.0) as i64;
            let skip = num(args, if ex { 7 } else { 5 }, 0.0) != 0.0;
            // ITR's locations (0 torso … 6 brain) [documented: FakeHit's table] as
            // GetHitLocation's index and the limb's condition actor value.
            let (location, condition) = match loc {
                1 => (1, crate::combat::HEAD),
                2 => (3, crate::combat::LEFT_ARM),
                3 => (5, crate::combat::RIGHT_ARM),
                4 => (7, crate::combat::LEFT_MOBILITY),
                5 => (10, crate::combat::RIGHT_MOBILITY),
                6 => (13, 31),
                _ => (0, crate::combat::TORSO),
            };
            let finite = |v: f64| if v.is_finite() { v } else { 0.0 };
            let limb_damage = if ex { finite(num(args, 3, 0.0)).max(0.0) } else { 0.0 };
            let limb = (ex && (1..=6).contains(&loc) && limb_damage > 0.0).then_some(LimbHit { condition, health_percent: 0.0, damage: limb_damage, points: true });
            let fatigue = if ex { finite(num(args, 2, 0.0)) } else { 0.0 };
            if skip {
                g.effect_health_damage(target, damage, attacker);
            } else {
                g.hit(target, damage, HitInfo { attacker, weapon, location, critical: false, fatigue_damage: fatigue.max(0.0), limb, ..HitInfo::default() });
            }
            if fatigue > 0.0 {
                crate::functions::add_actor_value_damage(g, target, crate::actor_values::FATIGUE, fatigue);
            }
            one
        }
        // `weapon [material 0-31]` on a reference: the weapon's impact effect there, for
        // the engine; 1 for a weapon [documented: FakeImpact; hypothesis: the return].
        "fakeimpact" => {
            let (Some(r), Some(w)) = (subject, form(args, 0).map(|w| base(g, w)).filter(|&w| g.data.sig(w) == Some(*b"WEAP"))) else { return Some(Ok(zero)) };
            let m = args.get(1).and_then(|v| v.num().ok()).and_then(|m| u8::try_from(m as i64).ok()).filter(|m| *m <= 31);
            queue(&mut g.other.itr2.impacts, (r, w, m));
            one
        }
        // `[target]`: the actor fights the target (combat started when not in it), whom
        // GetCombatTarget then names until the target dies; no target clears it
        // [documented: ForceCombatTarget].
        "forcecombattarget" => {
            let Some(a) = actor.filter(|&a| !g.is_dead(a)) else { return Some(Ok(zero)) };
            match form(args, 0) {
                None => {
                    g.other.itr2.forced_targets.remove(&a);
                    one
                }
                Some(t) if g.itr_is_actor(t) && !g.is_dead(t) && t != a => {
                    g.other.itr2.forced_targets.insert(a, t);
                    g.start_combat(a, t);
                    one
                }
                Some(_) => zero,
            }
        }
        // `target`: [x, y, z] of where the actor last knew the target; 0 with no data
        // [documented: the four pages]. NVForever's awareness keeps one place per target,
        // where it was last measured, so the four read it (see docs/spec-notes/ext-itr.md §8).
        "gettargetdetectedlocation" | "gettargetinitiallocation" | "gettargetlastfullyvisiblelocation" | "gettargetlastseenlocation" => {
            let at = actor.zip(form(args, 0)).and_then(|(a, t)| g.awareness.actors.get(&a).and_then(|x| x.targets.get(&t)).map(|x| x.at));
            match at {
                Some(p) => point(g, p, &[]),
                None => zero,
            }
        }
        // The reload animation for the engine, on an actor with a weapon in hand
        // [documented: ForceReload]; 0 without one (see docs/spec-notes/ext-itr.md §8).
        "forcereload" => {
            let Some(a) = actor else { return Some(Ok(zero)) };
            if crate::functions::items::equipped_weapon(g, a).is_none() {
                return Some(Ok(zero));
            }
            queue(&mut g.other.itr2.reloads, a);
            one
        }
        // `count`: that many rounds of the weapon's ammo into the inventory, the
        // player's magazine topped up by at most the count, never past the clip and never
        // below what it holds [documented: RefillAmmo; hypothesis: the rounds already
        // loaded count, 0 without the engine's magazine]; refused for a
        // dead actor, no weapon, or one that regenerates its ammo [documented: RefillAmmo].
        "refillammo" => {
            let Some(a) = actor.filter(|&a| !g.is_dead(a)) else { return Some(Ok(zero)) };
            let Some(w) = crate::functions::items::equipped_weapon(g, a) else { return Some(Ok(zero)) };
            let field = |g: &Game, s: &str, p: &str| crate::nvse::record_value(g, w, s, p).and_then(|v| v.num().ok());
            let ammo = match crate::nvse::record_value(g, w, "NAM0", "") {
                Some(XVal::Ref(r)) if r != 0 => FormId(r),
                _ => return Some(Ok(zero)),
            };
            if field(g, "DNAM", "regen_rate").is_some_and(|r| r > 0.0) {
                return Some(Ok(zero));
            }
            let n = num(args, 0, 0.0).max(0.0) as i32;
            if n == 0 || g.world.add_item(a, ammo, n).is_err() {
                return Some(Ok(zero));
            }
            if a == FormId::PLAYER_REF {
                let clip = field(g, "DATA", "clip_size").unwrap_or(0.0).max(0.0) as u32;
                let loaded = g.player_magazine.unwrap_or(0);
                let m = clip.min(loaded.saturating_add(n as u32)).max(loaded);
                g.other.set_magazine = Some(m);
                g.player_magazine = Some(m);
            }
            one
        }
        // `item`: the actor uses one of its aid items with every effect [documented:
        // UseAidItem]; 0 without it or for another kind of item.
        "useaiditem" => {
            let (Some(a), Some(item)) = (actor, form(args, 0).map(|i| base(g, i)).filter(|&i| g.data.sig(i) == Some(*b"ALCH"))) else { return Some(Ok(zero)) };
            yes(g.consume(a, item).is_ok())
        }
        // `[flags]`: ResurrectActor in place, bit 0 the base's inventory again
        // [documented: ResurrectActorEx; hypothesis: not the animated stand-up].
        "resurrectactorex" => {
            let Some(a) = actor else { return Some(Ok(zero)) };
            g.resurrect(a, false);
            if num(args, 0, 0.0) as i64 & 1 != 0 {
                let _ = g.world.reset_inventory(a);
                g.clear_npc_weapon_mods_roll(a);
                g.maybe_apply_npc_weapon_mods(a);
            }
            one
        }
        // Every dead actor in the loaded cells around the player (the high process);
        // how many [documented: ResurrectAll].
        "resurrectall" => {
            let cells = g.cell_of(FormId::PLAYER_REF).map(|c| g.scan_cells(c, 2)).unwrap_or_default();
            let dead: Vec<FormId> = cells.into_iter().flat_map(|c| g.refs_in_cell(c, false)).filter(|&r| g.itr_is_actor(r) && g.is_dead(r)).collect();
            for &a in &dead {
                g.resurrect(a, false);
            }
            XVal::Num(dead.len() as f64)
        }
        // Ragdoll `[limb] [x y z] [spin] [duration] [flags]`, RagdollLimb `limb [x y z]`
        // (the catalogue's arguments; Ragdoll's page is the ragdoll form's): for the
        // engine's physics; 1 on an actor (see docs/spec-notes/ext-itr.md §8).
        "ragdoll" | "ragdolllimb" => {
            let Some(a) = actor else { return Some(Ok(zero)) };
            let limb = num(args, 0, -1.0) as i64;
            let ask = RagdollAsk { actor: a, limb, push: [num(args, 1, 0.0), num(args, 2, 0.0), num(args, 3, 0.0)], spin: num(args, 4, 0.0), duration: num(args, 5, 0.0), flags: num(args, 6, 0.0) as i64 };
            queue(&mut g.other.itr2.ragdolls, ask);
            one
        }
        // [documented: IsRigidBodyAtRest]: an actor has no rigid body of its own (a
        // character controller), 0 [observed: itr probes, the player reads 0]; the
        // engine's bodies are not reported to the game, so another reference reads at
        // rest (see docs/spec-notes/ext-itr.md §8; follow-up).
        "isrigidbodyatrest" => yes(subject.is_some() && actor.is_none()),
        // `x y z level` by the calling actor; `x y z level radius [flags] [topic]` by no
        // one: a detection event there, the level in place of the action sound
        // [documented: CreateDetectionSoundAt; CreateAnonymousDetectionSoundAt;
        // hypothesis: the level's scale is CreateDetectionEvent's; the radius, flags and
        // topic are not read]. 1 queued.
        "createdetectionsoundat" | "createanonymousdetectionsoundat" => {
            let anon = name == "createanonymousdetectionsoundat";
            let owner = if anon { FormId(0) } else { match actor { Some(a) => a, None => return Some(Ok(zero)) } };
            if anon && num(args, 4, 0.0) <= 0.0 {
                return Some(Ok(zero));
            }
            let at = [num(args, 0, 0.0) as f32, num(args, 1, 0.0) as f32, num(args, 2, 0.0) as f32];
            g.queue_detection_event(crate::awareness::DetectionEvent { owner, at, level: num(args, 3, 0.0).max(0.0), kind: crate::awareness::EventType::Generic });
            one
        }
        // ---- paths and navmeshes (`crate::nav_ask`) ----
        "canpathtoref" | "getpathdistancetoref" | "getpathnodecount" | "getpathtoref" | "getnthpathnode" | "getpathlength" => {
            let (from, to) = if name == "getpathlength" {
                // `target [source]`, the source an actor [documented: GetPathLength].
                (form(args, 1).or(subject).filter(|&s| g.itr_is_actor(s)), form(args, 0))
            } else {
                (subject, form(args, 0))
            };
            let path = from.zip(to).and_then(|(a, b)| g.itr_pos(a).zip(g.itr_pos(b))).and_then(|(a, b)| crate::nav_ask::path(a, b));
            let length = |p: &[[f32; 3]]| p.windows(2).map(|w| dist(w[0], w[1])).sum::<f64>();
            match (name, path) {
                ("canpathtoref", p) => yes(p.is_some()),
                // -1 on failure [documented: GetPathDistanceToRef; GetPathLength].
                ("getpathdistancetoref" | "getpathlength", Some(p)) => XVal::Num(length(&p)),
                ("getpathdistancetoref" | "getpathlength", None) => XVal::Num(-1.0),
                ("getpathnodecount", p) => XVal::Num(p.map_or(0.0, |p| p.len() as f64)),
                ("getpathtoref", p) => {
                    let pts: Vec<XVal> = p.unwrap_or_default().into_iter().map(|q| point(g, q, &[])).collect();
                    list(g, pts)
                }
                (_, p) => match usize::try_from(num(args, 1, -1.0) as i64).ok().and_then(|i| p.and_then(|p| p.get(i).copied())) {
                    Some(q) => point(g, q, &[]),
                    None => zero,
                },
            }
        }
        // `x y z [anchor]`, the player when no anchor [documented: IsPointOnNavmesh].
        "ispointonnavmesh" => {
            let at = [num(args, 0, 0.0) as f32, num(args, 1, 0.0) as f32, num(args, 2, 0.0) as f32];
            let near = form(args, 3).unwrap_or(FormId::PLAYER_REF);
            yes(crate::nav_ask::on_navmesh(at, Some(near.0)))
        }
        // `x y z radius`: [x, y, z, flags] each, at most 128, unsorted; 0 for a radius
        // outside (0, 8192] [documented: GetCoverPointsInRadius].
        "getcoverpointsinradius" => {
            let r = num(args, 3, 0.0);
            if r <= 0.0 || r > MAX_COVER_RADIUS {
                return Some(Ok(zero));
            }
            let at = [num(args, 0, 0.0) as f32, num(args, 1, 0.0) as f32, num(args, 2, 0.0) as f32];
            let mut pts = crate::nav_ask::cover(at, r as f32);
            pts.truncate(MAX_COVER_POINTS);
            let items: Vec<XVal> = pts.into_iter().map(|(p, f)| point(g, p, &[f64::from(f)])).collect();
            list(g, items)
        }
        // `threat(x y z) search(x y z) radius [max]`: the cover points near the search
        // point that a sight line from the threat cannot reach, nearest first, [x, y, z,
        // flags, distance] each; max 8 by default, 1-32; 0 for a bad radius [documented:
        // GetBestCoverFromThreat]. Blocked: a cast from the threat to the point's chest
        // height meets something first (see docs/spec-notes/ext-itr.md §9).
        "getbestcoverfromthreat" => {
            let r = num(args, 6, 0.0);
            if r <= 0.0 || r > MAX_COVER_RADIUS {
                return Some(Ok(zero));
            }
            let threat = [num(args, 0, 0.0) as f32, num(args, 1, 0.0) as f32, num(args, 2, 0.0) as f32];
            let search = [num(args, 3, 0.0) as f32, num(args, 4, 0.0) as f32, num(args, 5, 0.0) as f32];
            let max = (num(args, 7, 8.0) as i64).clamp(1, 32) as usize;
            let mut found: Vec<([f32; 3], u32, f64)> = crate::nav_ask::cover(search, r as f32)
                .into_iter()
                .filter(|&(p, _)| {
                    let chest = [p[0], p[1], p[2] + COVER_CHEST];
                    let reach = dist(threat, chest) as f32;
                    crate::ray::cast(threat, chest, crate::ray::CastKind::World).is_some_and(|d| d < reach - 1.0)
                })
                .map(|(p, f)| (p, f, dist(p, search)))
                .collect();
            found.sort_by(|a, b| a.2.total_cmp(&b.2));
            found.truncate(max);
            let items: Vec<XVal> = found.into_iter().map(|(p, f, d)| point(g, p, &[f64::from(f), d])).collect();
            list(g, items)
        }
        // ---- ground and terrain (`crate::ray`) ----
        // posZ − groundZ, signed [documented: GetDistanceToGround; GetDistanceToTerrain,
        // 0 indoors or with no terrain].
        "getdistancetoground" | "getdistancetoterrain" => {
            let terrain = name == "getdistancetoterrain";
            let Some(at) = subject.and_then(|r| g.itr_pos(r)) else { return Some(Ok(zero)) };
            if terrain && g.cell_of(subject.unwrap_or(FormId(0))).is_some_and(|c| g.world.placed.world_of(c).is_none()) {
                return Some(Ok(zero));
            }
            XVal::Num(g.ground_z(at, terrain).map_or(0.0, |z| f64::from(at[2] - z)))
        }
        // `[distance]`: onto the ground (or terrain), or down by the distance but not
        // through it; 0 when none was found [documented: MoveToGround; MoveToTerrain].
        "movetoground" | "movetoterrain" => {
            let terrain = name == "movetoterrain";
            let Some(r) = subject else { return Some(Ok(zero)) };
            let Some(at) = g.itr_pos(r) else { return Some(Ok(zero)) };
            let Some(ground) = g.ground_z(at, terrain) else { return Some(Ok(zero)) };
            let d = num(args, 0, 0.0) as f32;
            let z = if d > 0.0 { (at[2] - d).max(ground) } else { ground };
            match crate::functions::console::call_words(g, Some(r), "SetPos", &["Z".to_string(), format!("{z}")], "MoveToGround") {
                Ok(_) => one,
                Err(_) => zero,
            }
        }
        // ---- doors and references ----
        // `[ref]`: the reference itself outdoors, else the exterior side of a load door
        // out, found through the interiors the doors lead to; disabled, deleted and
        // non-teleport doors left out; 0 when none [documented: GetRefExteriorDoor;
        // hypothesis: "the exterior-side load door" is the door out's other side].
        "getrefexteriordoor" => {
            let Some(r) = form(args, 0).or(subject) else { return Some(Ok(zero)) };
            let Some(start) = g.cell_of(r) else { return Some(Ok(zero)) };
            if g.world.placed.world_of(start).is_some() {
                return Some(Ok(XVal::Ref(r.0)));
            }
            let out = g.door_route(start, |g, c| g.world.placed.world_of(c).is_some()).and_then(|w| w.last().copied());
            XVal::Ref(out.and_then(|d| g.door_teleport(d)).map_or(0, |t| t.0 .0))
        }
        // `[ref] [refresh]`: the first load door on the shortest way from the player's
        // cell to the reference's; 0 in the same cell or with no way [documented:
        // GetRefNextTeleportDoor; hypothesis: the fewest doors stands for the shortest].
        "getrefnextteleportdoor" => {
            let Some(r) = form(args, 0).or(subject) else { return Some(Ok(zero)) };
            let (Some(here), Some(there)) = (g.cell_of(FormId::PLAYER_REF), g.cell_of(r)) else { return Some(Ok(zero)) };
            let same_world = |g: &Game, a: FormId, b: FormId| a == b || g.world.placed.world_of(a).is_some_and(|w| g.world.placed.world_of(b) == Some(w));
            if same_world(g, here, there) {
                return Some(Ok(zero));
            }
            XVal::Ref(g.door_route(here, |g, c| same_world(g, c, there)).and_then(|w| w.first().copied()).map_or(0, |d| d.0))
        }
        // `maxDistance [type] [depth] [taken] [heading] [count] [base]`: the references
        // that close to the player, nearest first [documented: GetRefsSortedByDistance;
        // the types 0 any, 200 actors, 201 items, else a form type; depth -1 five cells,
        // exteriors only].
        "getrefssortedbydistance" => {
            let max = num(args, 0, 0.0);
            let Some(me) = g.itr_pos(FormId::PLAYER_REF) else { return Some(Ok(zero)) };
            if max <= 0.0 {
                return Some(Ok(zero));
            }
            let code = num(args, 1, 0.0) as i64;
            let depth = match num(args, 2, -1.0) as i64 {
                d if d < 0 => 5,
                d => d,
            };
            let taken = num(args, 3, 0.0) != 0.0;
            let heading = num(args, 4, 0.0);
            let count = num(args, 5, 0.0) as i64;
            let want = form(args, 6);
            let facing = g.world.peek(FormId::PLAYER_REF).map_or(0.0, |s| f64::from(s.rot[2]));
            let mut found: Vec<(f64, FormId)> = g
                .ref_walk_list(None, code, depth, taken)
                .into_iter()
                .filter(|&r| want.is_none_or(|b| base(g, r) == b))
                .filter_map(|r| g.itr_pos(r).map(|p| (dist(me, p), r, p)))
                .filter(|&(d, _, _)| d <= max)
                .filter(|&(_, _, p)| {
                    // The angle between where the player faces (Z rotation, clockwise from
                    // north) and the reference, within `heading` degrees (see docs/spec-notes/ext-itr.md §9).
                    heading <= 0.0 || {
                        let bearing = f64::from(p[0] - me[0]).atan2(f64::from(p[1] - me[1]));
                        let off = (bearing - facing).rem_euclid(std::f64::consts::TAU);
                        off.min(std::f64::consts::TAU - off).to_degrees() <= heading
                    }
                })
                .map(|(d, r, _)| (d, r))
                .collect();
            found.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            if count > 0 {
                found.truncate(count as usize);
            }
            let items: Vec<XVal> = found.into_iter().map(|(_, r)| XVal::Ref(r.0)).collect();
            list(g, items)
        }
        // ---- perks ----
        // `perk [delta] [flags]` [documented: GetPerkEligibility]; 0 for no perk.
        "getperkeligibility" | "getperkblockers" => {
            let Some(p) = form(args, 0).filter(|p| g.data.perks.contains_key(p)) else { return Some(Ok(zero)) };
            let d = g.delta_of(args.get(1));
            let Some(e) = g.eligibility(p, &d, num(args, 2, 0.0) as i64) else { return Some(Ok(zero)) };
            if name == "getperkblockers" {
                g.reqs(&e.blockers)
            } else {
                g.eligibility_map(p, &e)
            }
        }
        // `[delta] [flags]`: the eligibility maps of the perks that can be taken
        // [documented: GetEligiblePerks], by least level then name (see docs/spec-notes/ext-itr.md §10).
        "geteligibleperks" => {
            let d = g.delta_of(args.first());
            let flags = num(args, 1, 0.0) as i64;
            let mut items = Vec::new();
            for p in g.perks_in_order() {
                if let Some(e) = g.eligibility(p, &d, flags).filter(|e| e.status == "takeable") {
                    items.push(g.eligibility_map(p, &e));
                }
            }
            list(g, items)
        }
        // `perk [flags]`: the perk data gates, the top-level requirements and the entry
        // points' filters [documented: GetPerkConditions; flags change nothing].
        "getperkconditions" => {
            let Some(p) = form(args, 0).filter(|p| g.data.perks.contains_key(p)) else { return Some(Ok(zero)) };
            let items = g.perk_conditions(p);
            list(g, items)
        }
        // `subject [delta] [flags]`: the perks with an entry point that can act on the
        // form [documented: GetPerksForForm].
        "getperksforform" => {
            let Some(subj) = form(args, 0).map(|f| base(g, f)) else { return Some(Ok(zero)) };
            let d = g.delta_of(args.get(1));
            let flags = num(args, 2, 0.0) as i64;
            let mut items = Vec::new();
            // In FormID order [observed: itr probes, the pistol's six perks].
            let mut perks: Vec<FormId> = g.data.perks.keys().copied().collect();
            perks.sort();
            for p in perks {
                let Some(def) = g.perk_def(p) else { continue };
                let mut matches = Vec::new();
                for e in &def.entries {
                    let crate::data::PerkEffect::EntryPoint(ep) = &e.effect else { continue };
                    // Any entry point whose filter names the form [observed: itr probes,
                    // Hand Loader]; the combat ones keep their names.
                    let known = COMBAT_POINTS.iter().find(|(c, _)| *c == ep.point).map(|(_, n)| *n);
                    let conds = ep.conditions.clone().unwrap_or_default();
                    let tabs: BTreeSet<i8> = conds.iter().map(|(t, _)| *t).collect();
                    let (kind, contextual, unknown) = match tabs.len() {
                        // No filter at all: not listed [observed: itr probes, Toughness and
                        // Quick Draw are not given for the pistol; the page counts them].
                        0 => continue,
                        1 => match g.filter_names(&conds.iter().map(|(_, c)| c).collect::<Vec<_>>(), subj) {
                            Some(true) => ("filter", false, false),
                            Some(false) => continue,
                            None if flags & 64 != 0 && flags & 128 != 0 => ("unknown", false, true),
                            None => continue,
                        },
                        // Several tabs: listed when one names the form, without flag 32
                        // [observed: itr probes, The Professional reads "future"]; flag 32
                        // marks it contextual [documented: GetPerksForForm].
                        _ => {
                            let named = tabs.iter().any(|t| g.filter_names(&conds.iter().filter(|(x, _)| x == t).map(|(_, c)| c).collect::<Vec<_>>(), subj) == Some(true));
                            if !named {
                                continue;
                            }
                            if flags & 32 != 0 { ("contextual", true, false) } else { ("filter", false, false) }
                        }
                    };
                    matches.push((e.rank, ep.point, known.unwrap_or(""), ep.function, tabs.len(), kind, contextual, unknown));
                }
                if matches.is_empty() {
                    continue;
                }
                let Some(el) = g.eligibility(p, &d, flags | 8) else { continue };
                // Without the flags, the perks the level-up menu would not offer are left
                // out unless owned [documented: "Owned relevant perks are returned"].
                // Traits are listed [observed: itr probes, Fast Shot and Trigger
                // Discipline read "blocked"]; the other gates leave a perk out.
                if el.owned == 0 && el.gated && !g.perk_def(p).is_some_and(|d| d.is_trait) {
                    continue;
                }
                let all_contextual = matches.iter().all(|m| m.6);
                let any_unknown = matches.iter().any(|m| m.7);
                let status = if el.owned > 0 {
                    "owned_active"
                } else if all_contextual {
                    "contextual"
                } else if any_unknown {
                    "unknown"
                } else if el.status == "takeable" {
                    "takeable"
                // Only what the player raises holds it back, the level or actor values
                // (see docs/spec-notes/ext-itr.md §10).
                } else if !el.blockers.is_empty() && el.blockers.iter().all(|b| matches!(b.kind, "Level" | "ActorValue")) {
                    "future"
                } else if el.status == "unknown" {
                    "unknown"
                } else {
                    "blocked"
                };
                let mut ms = Vec::new();
                for (rank, point_code, pname, function, ntabs, kind, contextual, unknown) in matches {
                    ms.push(smap(
                        g,
                        vec![
                            ("rank", XVal::Num(f64::from(rank))),
                            ("entryPoint", XVal::Num(f64::from(point_code))),
                            ("entryPointName", XVal::Str(pname.into())),
                            ("entryFunction", XVal::Num(f64::from(function))),
                            ("conditionTabs", XVal::Num(ntabs as f64)),
                            ("match", XVal::Str(kind.into())),
                            ("contextual", yes(contextual)),
                            ("unknown", yes(unknown)),
                        ],
                    ));
                }
                let matched = list(g, ms);
                let pname = g.data.perks.get(&p).map(|d| d.name.clone()).unwrap_or_default();
                let (blockers, indeterminate) = (g.reqs(&el.blockers), g.reqs(&el.indeterminate));
                items.push(smap(
                    g,
                    vec![
                        ("perk", XVal::Ref(p.0)),
                        ("name", XVal::Str(pname)),
                        ("subject", XVal::Ref(subj.0)),
                        ("ownedRank", XVal::Num(f64::from(el.owned))),
                        ("nextRank", XVal::Num(f64::from((el.owned + 1).min(el.max)))),
                        ("maxRank", XVal::Num(f64::from(el.max))),
                        ("relevant", one.clone()),
                        ("eligible", yes(el.status == "takeable")),
                        ("owned", yes(el.owned > 0)),
                        ("contextual", yes(all_contextual)),
                        ("unknown", yes(any_unknown)),
                        ("status", XVal::Str(status.into())),
                        ("blockers", blockers),
                        ("indeterminate", indeterminate),
                        ("matchedEntryPoints", matched),
                    ],
                ));
            }
            list(g, items)
        }
        // ---- recipes, containers, references ----
        // `[category]`: the recipes whose conditions pass and whose skill and ingredients
        // the player has; a category (RCCT) filters, another form does not [documented:
        // GetAvailableRecipes].
        "getavailablerecipes" => {
            let cat = form(args, 0).filter(|c| g.data.recipe_categories.contains_key(c));
            let mut ids: Vec<FormId> = g.data.recipes.iter().filter(|(_, r)| cat.is_none_or(|c| r.listed_under(c))).map(|(&id, _)| id).collect();
            ids.sort();
            let mut out = Vec::new();
            for id in ids {
                let r = g.data.recipes[&id].clone();
                if !g.conditions_pass(&r.conditions, Some(FormId::PLAYER_REF), None, "GetAvailableRecipes") {
                    continue;
                }
                let skills: HashMap<u16, f64> = r.skill.iter().map(|&(av, _)| (av, crate::functions::actor_value(g, FormId::PLAYER_REF, av).unwrap_or(0.0))).collect();
                let have: HashMap<FormId, i64> = r.ingredients.iter().map(|&(i, _)| (i, i64::from(g.world.item_count(FormId::PLAYER_REF, i).unwrap_or(0)))).collect();
                if crate::crafting::missing(&r, 1, |av| skills.get(&av).copied().unwrap_or(0.0), |i| have.get(&i).copied().unwrap_or(0)).is_empty() {
                    out.push(XVal::Ref(id.0));
                }
            }
            list(g, out)
        }
        // On a container: how many kinds of item it holds, read without changing it
        // [documented: GetVisibleContainerInventoryCount]; NVForever rolls a container's
        // leveled lists when it fills it, so what it holds is what it shows (see docs/spec-notes/ext-itr.md §11).
        "getvisiblecontainerinventorycount" => {
            let Some(c) = subject else { return Some(Ok(zero)) };
            let kinds: BTreeSet<FormId> = g.world.inventory_mut(c).map(|inv| inv.iter().filter(|(_, n)| *n > 0).map(|(i, _)| i).collect()).unwrap_or_default();
            XVal::Num(kinds.len() as f64)
        }
        // `[count]` on a reference: that many copies of its base placed at it (PlaceAtMe),
        // the last one returned (see docs/spec-notes/ext-itr.md §11).
        "duplicate" => {
            let Some(r) = subject else { return Some(Ok(XVal::Ref(0))) };
            let b = base(g, r);
            if b == r {
                return Some(Ok(XVal::Ref(0)));
            }
            let n = num(args, 0, 1.0).max(1.0) as i64;
            match crate::functions::console::call_words(g, Some(r), "PlaceAtMe", &[format!("{:08X}", b.0), n.to_string()], "Duplicate") {
                Ok(c) => match c.value {
                    fnv_script::vm::Value::Ref(x) => XVal::Ref(x),
                    _ => XVal::Ref(0),
                },
                Err(_) => XVal::Ref(0),
            }
        }
        // `race` on an NPC's reference: that reference alone takes the race, its look
        // rebuilt; not saved [documented: SetRaceAlt].
        "setracealt" => {
            let (Some(a), Some(race)) = (actor, form(args, 0).filter(|&r| g.data.sig(r) == Some(*b"RACE"))) else { return Some(Ok(zero)) };
            if a == FormId::PLAYER_REF || g.data.sig(base(g, a)) != Some(*b"NPC_") {
                return Some(Ok(zero));
            }
            g.other.itr2.races.insert(a, race);
            g.appearance_changes.push(crate::appearance::AppearanceChange { actor: a, reset: false });
            one
        }
        // ---- menus ----
        // `list [flag]` on a merchant: the barter menu, the list's items barred from (or
        // alone allowed for) sale, for the engine's barter menu [documented: the pages;
        // hypothesis: the flag is ShowBarterMenu's discount].
        "showbartermenublacklist" | "showbartermenuwhitelist" => {
            let (Some(m), Some(l)) = (actor, form(args, 0).filter(|&l| g.data.sig(l) == Some(*b"FLST"))) else { return Some(Ok(zero)) };
            g.other.itr2.barter_filter = Some((l, name == "showbartermenuwhitelist"));
            g.barter_requests.push(crate::game::BarterRequest { merchant: m, discount: num(args, 1, 0.0) as i32 });
            one
        }
        // [documented: StartNewGame]: the engine goes to a new game (follow-up).
        "startnewgame" => {
            g.other.itr2.start_new_game = true;
            one
        }
        // `cell`: CenterOnCell [documented: CenterOnCellAlt; the main menu's new session
        // is the engine's]; 0 when the cell is not found (see docs/spec-notes/ext-itr.md §11).
        "centeroncellalt" | "coca" => yes(crate::functions::console::call_words(g, None, "CenterOnCell", &[text(args, 0)], "CenterOnCellAlt").is_ok()),
        // [documented: ToggleAllPrimitives]: the switch, for the engine's markers
        // (follow-up); returns 0 as the console toggles do (see docs/spec-notes/ext-itr.md §11).
        "toggleallprimitives" | "tap" => {
            g.other.itr2.primitives_shown = !g.other.itr2.primitives_shown;
            zero
        }
        _ => return None,
    };
    Some(Ok(v))
}

impl Game<'_> {
    /// The doors of the fewest-doors way from `start` to a cell `arrived` accepts,
    /// through the load doors (enabled, not deleted, teleporting) of each place: an
    /// interior cell, or a whole worldspace (one walks between its cells) [observed:
    /// itr probes, GetRefNextTeleportDoor from GSGasStation to Doc Mitchell's house
    /// gives the gas station's own door, the way crossing Goodsprings outdoors].
    fn door_route(&self, start: FormId, arrived: impl Fn(&Game, FormId) -> bool) -> Option<Vec<FormId>> {
        let place = |g: &Game, c: FormId| g.world.placed.world_of(c).unwrap_or(c);
        let mut seen = BTreeSet::from([place(self, start)]);
        let mut queue = std::collections::VecDeque::from([(start, Vec::<FormId>::new())]);
        while let Some((cell, way)) = queue.pop_front() {
            for d in self.place_doors(cell) {
                if self.data.sig(crate::functions::base_of(self, d).unwrap_or(d)) != Some(*b"DOOR") || self.world.peek(d).is_some_and(|s| !s.enabled()) {
                    continue;
                }
                let Some((to, _, _)) = self.door_teleport(d) else { continue };
                let Some(next) = self.cell_of(to) else { continue };
                let mut way = way.clone();
                way.push(d);
                if arrived(self, next) {
                    return Some(way);
                }
                if seen.insert(place(self, next)) {
                    queue.push_back((next, way));
                }
            }
        }
        None
    }

    /// The references that may be load doors in `cell`'s place: the cell's own inside,
    /// every cell of the worldspace outside (the placed doors with a teleport, in
    /// FormID order).
    fn place_doors(&self, cell: FormId) -> Vec<FormId> {
        let placed = &self.world.placed;
        let Some(w) = placed.world_of(cell) else { return self.refs_in_cell(cell, false) };
        let mut out: Vec<FormId> = placed
            .cell_world
            .iter()
            .filter(|(_, &cw)| cw == w)
            .flat_map(|(c, _)| placed.by_cell.get(c).into_iter().flatten().copied())
            .filter(|&d| placed.get(d).is_some_and(|p| p.teleport.is_some()) || self.other.doors.contains_key(&d))
            .collect();
        out.sort();
        out.dedup();
        out
    }
}
