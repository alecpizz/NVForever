//! Unique quest rewards in co-op (docs/design/coop-shared-progress.md §4.1): the items, perks and spells that only
//! one quest gives, computed from the loaded data so DLC and mods are covered.
//!
//! **The rule** [design: coop-shared-progress.md §4.1]. A form is unique to quest Q when:
//! - an item: Q's own scripts give it (`AddItem`, `AddItemHealthPercent`), and nothing
//!   else in the load order provides it: no leveled item list, no placed reference, no
//!   container's or actor's inventory (vendors included), no recipe output, and no script
//!   that is not Q's (another quest's, an object's, a package's, a terminal's, a perk
//!   entry's) gives it (`AddItem`, `AddItemHealthPercent`, `PlaceAtMe`,
//!   `AddItemToLeveledList`);
//! - a perk: `AddPerk` of it only in Q's scripts, and it is not playable (a playable perk
//!   or trait is the level-up menu's to give as well) [design];
//! - a spell (actor effect): `AddSpell` of it only in Q's scripts.
//!
//! Q's own scripts are its stage result scripts, its quest script (the SCPT its SCRI
//! names, when no other record uses that script) and the result scripts of the dialogue
//! lines (INFO) that belong to Q. Only the winning records' units count.
//!
//! **What the rule cannot see** [design, conservative: a form left out goes to every
//! player, as before P5]:
//! - a call whose form is a `ref` variable (`player.AddItem rReward 1`): the form is not
//!   known before the script runs, so it neither makes nor unmakes a unique;
//! - a form list given (`AddItem SomeFormList`): its members count as given by something
//!   other than Q, so none of them is unique;
//! - extended expressions (`Eval`, `Let`, inline expressions of the script extenders);
//! - forms a script adds to a leveled list or a container at run time by other means
//!   (`RemoveAllItems` into a container, `AddFormToFormList`), challenge rewards.
//!
//! **Companion perks** ([`Uniques::companion`]), the permanent ones a companion's
//! personal quest awards, are the host's alone: a guest never gets one, claimed quest or
//! not [design: only the host]. The perks a companion lends while it
//! follows are not among them (`crate::coop_perks` lends the host's to guests).
//!
//! [`compute`] is pure and cheap (one walk over the compiled units and the base forms);
//! [`Game::coop_uniques`] runs it once per game on first use.

use crate::data::GameData;
use crate::game::Game;
use fnv_script::ir::{Arg, Expr, Stmt, Token};
use fnv_script::unit::RefSlot;
use fnv_script::{ScriptUnit, UnitOwner};
use fnv_sim::forms::BaseData;
use fnv_sim::{FormId, FormStore, RefTable};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// What a unique is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Item,
    Perk,
    Spell,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Item => "item",
            Kind::Perk => "perk",
            Kind::Spell => "spell",
        }
    }
}

/// The uniques of a load order: each form with its kind and the quest it is unique to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Uniques {
    pub by_form: BTreeMap<FormId, (Kind, FormId)>,
    /// Companion perks: the host's alone, never a guest's [design: only the host,
    /// for companion perks as unique quest rewards]: the permanent unique
    /// perks that a companion's own dialogue line gives or that a companion's dialogue quest gives (a quest whose
    /// lines with a speaker are all companions'). A companion is an actor some script
    /// makes or unmakes the player's teammate (`<ref>.SetPlayerTeammate`, or the call in
    /// the actor's own script or in a line it speaks); a line's speaker is its GetIsID
    /// condition's actor, else its ANAM [design]. Not the perks a companion lends while
    /// it follows (`crate::coop_perks::COMPANION_PERKS`, lent to every
    /// player while any player's companion follows; they are no unique reward at all). On the masters: Cass's, Raul's, Veronica's (with Elijah's two),
    /// Christine's, Dean's and Dog's [observed: `fnvtool coop uniques` on the masters].
    pub companion: BTreeSet<FormId>,
}

impl Uniques {
    /// The quest `form` is unique to, if it is a unique.
    pub fn quest_of(&self, form: FormId) -> Option<FormId> {
        self.by_form.get(&form).map(|(_, q)| *q)
    }
    pub fn len(&self) -> usize {
        self.by_form.len()
    }
    /// Whether `perk` is a companion perk (the host's alone).
    pub fn is_companion_perk(&self, perk: FormId) -> bool {
        self.companion.contains(&perk)
    }
    pub fn is_empty(&self) -> bool {
        self.by_form.is_empty()
    }
}

/// Who a script unit belongs to, for the rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Owner {
    Quest(FormId),
    Other,
}

/// The giving calls the rule reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gives {
    /// AddItem, AddItemHealthPercent.
    Item,
    /// PlaceAtMe, AddItemToLeveledList: a source of an item, not a quest's reward.
    ItemSource,
    Perk,
    Spell,
    /// RemoveItem: with caps (Caps001), the unit is a sale or an exchange.
    Takes,
    /// SetPlayerTeammate: the actor it runs on is a companion.
    Teammate,
}

/// Caps001 (main's `caravan::CAPS`).
use crate::caravan::CAPS;

fn gives(data: &GameData, opcode: u16) -> Option<Gives> {
    let name = &data.table.get(opcode)?.name;
    let is = |n: &str| name.eq_ignore_ascii_case(n);
    if is("AddItem") || is("AddItemHealthPercent") {
        Some(Gives::Item)
    } else if is("PlaceAtMe") || is("AddItemToLeveledList") {
        Some(Gives::ItemSource)
    } else if is("AddPerk") {
        Some(Gives::Perk)
    } else if is("AddSpell") {
        Some(Gives::Spell)
    } else if is("RemoveItem") {
        Some(Gives::Takes)
    } else if is("SetPlayerTeammate") {
        Some(Gives::Teammate)
    } else {
        None
    }
}

/// The forms a unit's giving calls name: (what, form).
fn unit_gives(data: &GameData, unit: &ScriptUnit, program: &fnv_script::Program, out: &mut Vec<(Gives, FormId)>) {
    fn call(data: &GameData, unit: &ScriptUnit, c: &fnv_script::ir::Call, out: &mut Vec<(Gives, FormId)>) {
        let Some(what) = gives(data, c.opcode) else { return };
        // SetPlayerTeammate: the actor it runs on (its reference), or 0 for the script's
        // own actor. `SetPlayerTeammate 0` counts too: what lets an actor go was one (Dead
        // Money hires Dean through a variable and lets him go by his reference) [observed:
        // DeadMoney.esm SCPT 0x01006E92].
        if what == Gives::Teammate {
            let on = c.on.and_then(|r| match unit.slot(r) {
                Some(RefSlot::Form(f)) => Some(f),
                _ => None,
            });
            if c.on.is_none() || on.is_some() {
                out.push((what, FormId(on.unwrap_or(0))));
            }
            return;
        }
        // AddItemToLeveledList names the list first, then the item: every form named
        // counts as a source. The others: the first argument.
        let args: &[Arg] = if what == Gives::ItemSource { &c.args } else { c.args.get(..1).unwrap_or(&[]) };
        for a in args {
            if let Arg::Ref(r) = a {
                if let Some(RefSlot::Form(f)) = unit.slot(*r) {
                    out.push((what, FormId(f)));
                }
            }
        }
    }
    fn expr(data: &GameData, unit: &ScriptUnit, e: &Expr, out: &mut Vec<(Gives, FormId)>) {
        for t in &e.0 {
            if let Token::Call { call: c } = t {
                call(data, unit, c, out);
            }
        }
    }
    fn stmts(data: &GameData, unit: &ScriptUnit, body: &[Stmt], out: &mut Vec<(Gives, FormId)>) {
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

/// The owner of each unit that counts (the winning records' units; module notes).
fn owners(data: &GameData) -> Vec<Option<Owner>> {
    // SCPT → the records that run it (SCRI).
    let mut users: HashMap<FormId, Vec<FormId>> = HashMap::new();
    for (form, script) in &data.form_script {
        users.entry(*script).or_default().push(*form);
    }
    let stage_units: HashSet<usize> = data.quests.values().flat_map(|q| q.stages.values().flatten().filter_map(|e| e.unit)).collect();
    data.units
        .iter()
        .enumerate()
        .map(|(i, c)| match &c.unit.owner {
            UnitOwner::Script { form } => {
                let f = FormId(*form);
                if data.scripts.get(&f) != Some(&i) {
                    return None;
                }
                // A quest's script, when that quest alone runs it.
                Some(match users.get(&f).map(Vec::as_slice) {
                    Some([q]) if data.quests.contains_key(q) => Owner::Quest(*q),
                    _ => Owner::Other,
                })
            }
            UnitOwner::QuestStage { form, .. } => stage_units.contains(&i).then_some(Owner::Quest(FormId(*form))),
            UnitOwner::Info { form, end } => {
                let d = data.infos.get(&FormId(*form))?;
                if (if *end { d.end } else { d.begin }) != Some(i) {
                    return None;
                }
                Some(d.quest.map_or(Owner::Other, Owner::Quest))
            }
            _ => Some(Owner::Other),
        })
        .collect()
}

/// The uniques of the load order `data`, `forms` and `refs` were built from (module
/// notes). Pure: the same data gives the same list.
pub fn compute(data: &GameData, forms: &FormStore, refs: &RefTable) -> Uniques {
    // Per form: the quests whose scripts give it, and whether anything else provides it.
    let mut given: HashMap<(Kind, FormId), HashSet<FormId>> = HashMap::new();
    let mut elsewhere: HashSet<(Kind, FormId)> = HashSet::new();
    let owners = owners(data);
    // Companions: the actors (base forms) scripts make teammates; and each perk's lines.
    let mut teammates: HashSet<FormId> = HashSet::new();
    let mut perk_lines: Vec<(FormId, FormId)> = Vec::new();
    let mut users: HashMap<FormId, Vec<FormId>> = HashMap::new();
    for (form, script) in &data.form_script {
        users.entry(*script).or_default().push(*form);
    }
    let mut calls = Vec::new();
    for (c, owner) in data.units.iter().zip(&owners) {
        let Some(owner) = owner else { continue };
        calls.clear();
        unit_gives(data, &c.unit, &c.program, &mut calls);
        // The rule [design]: a unique comes from that one quest.
        // What a line sells for caps (the clinic's implants, the cashier's chips) is bought,
        // and what a background quest's own script hands out (running from the game's
        // start, with no name, stages or objectives: the preorder packs) comes with the
        // game, not from a quest [design].
        let sold = calls.iter().any(|&(w, f)| w == Gives::Takes && f == CAPS);
        // An item a unit takes from the player and gives back is kept stocked, an
        // ability's charge (Old World Blues' Implant GRX: NVDLC03GenericPerkQUEST's script
        // refills it [observed: OldWorldBlues.esm SCPT 0x03009D4C]; the implant comes
        // from the Sink's Auto-Doc, each player's own [design]), not
        // a quest's reward [design].
        for &(w, f) in &calls {
            if w == Gives::Takes && f != CAPS && calls.contains(&(Gives::Item, f)) {
                elsewhere.insert((Kind::Item, f));
            }
        }
        let setup = match (owner, &c.unit.owner) {
            (Owner::Quest(q), UnitOwner::Script { .. }) => data.quests.get(q).is_some_and(|d| d.flags & crate::quest::START_GAME_ENABLED != 0 && d.name.as_deref().is_none_or(|n| n.trim().is_empty()) && d.stages.is_empty() && d.objectives.is_empty()),
            _ => false,
        };
        for &(what, f) in &calls {
            if what == Gives::Teammate {
                if f.0 != 0 {
                    teammates.insert(refs.refs.get(&f).map_or(f, |r| r.base));
                } else if let UnitOwner::Script { form } = c.unit.owner {
                    teammates.extend(users.get(&FormId(form)).into_iter().flatten().copied());
                } else if let UnitOwner::Info { form, .. } = c.unit.owner {
                    // A line's result script runs on its speaker.
                    teammates.extend(data.infos.get(&FormId(form)).and_then(|i| crate::coverage::line_actor(data, i)));
                }
                continue;
            }
            if let (Gives::Perk, UnitOwner::Info { form, .. }) = (what, &c.unit.owner) {
                perk_lines.push((f, FormId(*form)));
            }
            if what == Gives::Takes {
                continue;
            }
            // A form list given: its members, from something other than Q (module notes).
            if let Some(list) = data.form_lists.get(&f) {
                if matches!(what, Gives::Item | Gives::ItemSource) {
                    elsewhere.extend(list.iter().map(|m| (Kind::Item, *m)));
                }
                continue;
            }
            let kind = match what {
                Gives::Item | Gives::ItemSource => Kind::Item,
                Gives::Perk => Kind::Perk,
                Gives::Spell => Kind::Spell,
                Gives::Takes | Gives::Teammate => continue,
            };
            if sold || setup {
                elsewhere.insert((kind, f));
                continue;
            }
            match (owner, what) {
                (Owner::Quest(q), Gives::Item | Gives::Perk | Gives::Spell) => {
                    given.entry((kind, f)).or_default().insert(*q);
                }
                // A quest's own PlaceAtMe of it is neither a reward nor another source.
                (Owner::Quest(_), Gives::ItemSource | Gives::Takes | Gives::Teammate) => {}
                (Owner::Other, _) => {
                    elsewhere.insert((kind, f));
                }
            }
        }
    }
    // The data's own sources of items.
    for b in forms.bases.values() {
        let entries = match &b.data {
            BaseData::Container(c) => c.entries.iter().map(|e| e.item).collect::<Vec<_>>(),
            BaseData::Actor(a) => a.entries.iter().map(|e| e.item).collect(),
            BaseData::LeveledItem(l) => l.entries.iter().map(|e| e.item).collect(),
            _ => continue,
        };
        elsewhere.extend(entries.into_iter().map(|f| (Kind::Item, f)));
    }
    elsewhere.extend(refs.refs.values().map(|r| (Kind::Item, r.base)));
    elsewhere.extend(data.recipes.values().flat_map(|r| r.outputs.iter().map(|(f, _)| (Kind::Item, *f))));
    // A playable perk or trait: the level-up menu gives it too.
    elsewhere.extend(data.perks.iter().filter(|(_, p)| p.playable).map(|(f, _)| (Kind::Perk, *f)));
    let mut out = Uniques::default();
    for ((kind, f), quests) in given {
        if quests.len() != 1 || elsewhere.contains(&(kind, f)) {
            continue;
        }
        // An item must be an inventory item (not a leveled list, an actor, a note's
        // stand-in): what the base forms know as items.
        if kind == Kind::Item && forms.item(f).is_none() {
            continue;
        }
        let q = quests.into_iter().next().expect("one quest");
        out.by_form.insert(f, (kind, q));
    }
    // A companion's dialogue quest: every line of it with a speaker is a companion's.
    let mut quest_lines: HashMap<FormId, (usize, usize)> = HashMap::new();
    for i in data.infos.values() {
        let (Some(q), Some(a)) = (i.quest, crate::coverage::line_actor(data, i)) else { continue };
        let e = quest_lines.entry(q).or_default();
        e.0 += 1;
        e.1 += usize::from(teammates.contains(&a));
    }
    for (&perk, &(kind, q)) in &out.by_form {
        if kind == Kind::Perk && quest_lines.get(&q).is_some_and(|&(n, c)| n > 0 && c == n) {
            out.companion.insert(perk);
        }
    }
    for (perk, line) in perk_lines {
        let by_companion = data.infos.get(&line).and_then(|i| crate::coverage::line_actor(data, i)).is_some_and(|a| teammates.contains(&a));
        if by_companion && out.by_form.get(&perk).is_some_and(|(k, _)| *k == Kind::Perk) {
            out.companion.insert(perk);
        }
    }
    // The perks a companion lends while it follows are no quest's reward: every player
    // has them while any player's companion follows (`crate::coop_perks::COMPANION_PERKS`).
    //
    for e in crate::coop_perks::COMPANION_PERKS {
        if let Some(p) = data.by_edid(e) {
            out.companion.remove(&p);
            out.by_form.remove(&p);
        }
    }
    out
}

impl Game<'_> {
    /// This load order's uniques ([`compute`]), computed on first use and kept.
    pub fn coop_uniques(&self) -> &Uniques {
        self.coop_uniques_cache.get_or_init(|| {
            let t0 = std::time::Instant::now();
            let u = compute(self.data, self.world.forms, self.world.placed);
            tracing::info!("co-op: {} unique quest rewards in this load order ({:.0} ms)", u.len(), t0.elapsed().as_secs_f64() * 1e3);
            u
        })
    }

    /// Whether this character keeps (or may take) `form` as a quest reward: anything
    /// that is not a unique, and a unique of a quest it claimed a stage of (coop-shared-progress.md §4.1).
    pub fn coop_unique_allowed(&self, form: FormId) -> bool {
        if self.coop_uniques().is_companion_perk(form) {
            return false;
        }
        // Keys always come (the host's world's doors need them; the undo on leaving takes a
        // quest's key back out of a non-claimer's own save) [design].
        self.data.sig(form).as_ref() == Some(b"KEYM") || self.coop_uniques().quest_of(form).is_none_or(|q| claimed_any(&self.player.claims, q))
    }

    /// The host's story to take off this guest's character in its own save
    /// (`crate::coop_story`): all of it, except the uniques of quests it claimed a stage
    /// of, which it keeps (coop-shared-progress.md §4.1).
    pub fn coop_story_to_undo(&self) -> crate::coop_story::CoopStory {
        let uniques = self.coop_uniques();
        let claims = &self.player.claims;
        self.coop_story.without(|f| !uniques.is_companion_perk(f) && uniques.quest_of(f).is_some_and(|q| claimed_any(claims, q)))
    }
}

/// Whether `ledger` has a stage of `q`, waiting or replayed.
pub fn claimed_any(ledger: &crate::coop_joint::ClaimLedger, q: FormId) -> bool {
    ledger.stages.get(&q).is_some_and(|s| !s.is_empty()) || ledger.done.get(&q).is_some_and(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coop_joint::{ClaimLedger, ClaimedStage};

    #[test]
    fn a_stage_waiting_or_replayed_is_a_claim_of_its_quest() {
        let (q, other) = (FormId(0x900), FormId(0x901));
        let mut l = ClaimLedger::default();
        assert!(!claimed_any(&l, q));
        l.stages.insert(q, Vec::new());
        assert!(!claimed_any(&l, q), "an empty entry is no claim");
        l.stages.insert(q, vec![ClaimedStage { stage: 10, line: None }]);
        assert!(claimed_any(&l, q) && !claimed_any(&l, other));
        // Replayed in its own world: still claimed.
        let mut l = ClaimLedger::default();
        l.done.entry(q).or_default().insert(10);
        assert!(claimed_any(&l, q));
    }
}
