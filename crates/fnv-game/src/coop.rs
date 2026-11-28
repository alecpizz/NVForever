//! The other players of a co-op game (docs/design/coop-save.md): what a save keeps of
//! players 2–4. Player 1 is the game's own player (`FormId::PLAYER_REF`, [`PlayerState`],
//! its `RefExtra`); each other player has a [`CoopPlayer`] holding the same kinds of state
//! for that character alone. The quests, the world and everything else the game saves
//! stay shared and are saved once, as before.
//!
//! This is the save format's groundwork: nothing plays these characters yet. The game
//! keeps them ([`Game::coop`]) so that a save loaded and saved again keeps them unchanged.

use crate::data::{Compiled, GameData, InfoDef};
use crate::game::{ActiveEffect, Game, PlayerState, RefExtra};
use fnv_script::ir::{Arg, Expr, Op, RefIndex, Stmt, Token, VarSlot};
use fnv_script::unit::RefSlot;
use fnv_sim::world::RefState;
use fnv_sim::FormId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Players in a co-op game, the game's own player included.
pub const MAX_PLAYERS: u8 = 4;
/// The first slot a [`CoopPlayer`] takes (slot 1 is the game's own player).
pub const FIRST_COOP_SLOT: u8 = 2;

/// ShowBarterMenu and ShowCaravanMenu (`crate::functions`).
const SHOW_BARTER_MENU: u16 = 0x1173;
const SHOW_CARAVAN_MENU: u16 = 0x125A;
/// AddItem and RemoveItem (`crate::functions`).
const ADD_ITEM: u16 = 0x1002;
const REMOVE_ITEM: u16 = 0x1052;

/// Whether a script changes chips for something else: it both adds and removes items, and
/// one of those items is a casino chip (CHIP). That is a casino cashier's exchange, in
/// either direction [observed: FalloutNV.esm, the casino cashiers' lines that buy chips
/// for money and cash them in; the lines that only hand out free chips, a casino's comps,
/// add without removing].
fn exchanges_chips(data: &GameData, unit: &Compiled) -> bool {
    let (mut adds, mut removes, mut chip) = (false, false, false);
    unit.program.walk(&mut |s| {
        let Stmt::Call { call } = s else { return };
        if call.opcode != ADD_ITEM && call.opcode != REMOVE_ITEM {
            return;
        }
        adds |= call.opcode == ADD_ITEM;
        removes |= call.opcode == REMOVE_ITEM;
        let item = match call.args.first() {
            Some(Arg::Ref(r)) => unit.unit.slot(*r),
            _ => None,
        };
        if let Some(RefSlot::Form(f)) = item {
            chip |= data.sig(FormId(f)) == Some(*b"CHIP");
        }
    });
    adds && removes && chip
}

/// A line that does nothing but end the conversation: flagged Goodbye, offering no
/// topics, and with no script that calls anything (a quest's farewell that sets a stage
/// or gives an item stays the host's).
fn only_takes_leave(data: &GameData, d: &InfoDef) -> bool {
    let calls_nothing = |u: crate::data::UnitId| {
        let mut n = BTreeMap::new();
        crate::intro::calls(&data.unit(u).program, &mut n);
        n.is_empty()
    };
    d.has(crate::data::info_flag::GOODBYE) && d.choices.is_empty() && d.begin.into_iter().chain(d.end).all(calls_nothing)
}

/// [`Game::guest_lines`]: the lines that trade, deal Caravan or exchange a casino's
/// chips, then, until nothing more is added, the lines whose choices (TCLT) offer a topic
/// with such a line.
fn guest_lines(data: &GameData) -> BTreeSet<FormId> {
    let calls = |u: crate::data::UnitId| {
        let mut n = BTreeMap::new();
        crate::intro::calls(&data.unit(u).program, &mut n);
        [SHOW_BARTER_MENU, SHOW_CARAVAN_MENU].iter().any(|o| n.contains_key(o)) || exchanges_chips(data, data.unit(u))
    };
    let mut out: BTreeSet<FormId> = data.infos.iter().filter(|(_, d)| d.begin.into_iter().chain(d.end).any(calls)).map(|(i, _)| *i).collect();
    loop {
        let topics: BTreeSet<FormId> = out.iter().filter_map(|i| data.infos.get(i)?.topic).collect();
        let more: Vec<FormId> = data.infos.iter().filter(|(i, d)| !out.contains(i) && d.choices.iter().any(|t| topics.contains(t))).map(|(i, _)| *i).collect();
        if more.is_empty() {
            return out;
        }
        out.extend(more);
    }
}

// ---- random encounters every player sets off ---------------------------------------
//
// In co-op a random encounter is set off by any player, not only the host: in its
// scripts the player is any player [design]:
// - `player.GetDistance X` and `GetDistance player`: the nearest player's distance;
// - `player.GetInCell X`: whether any player is in that cell;
// - `StartCombat player`: the fight is with the player nearest the attacker;
// - `OnTriggerEnter player`, `OnHit player`, `OnDeath player`: any player's.
//
// The encounter still runs once, in the host's world (a guest's game runs no world
// scripts), so each fires once per world. `player.HasPerk` and `player.GetItemCount` stay
// the host's.
//
// Which scripts are encounters is read from the load order's scripts themselves
// ([`find_coop_encounters`]), so a mod's encounters built the same way count too:
// - **A core:** a script, on a placed reference or a quest, that names no story (no
//   stage, objective or quest-state function, no quest but its own) and has one of
//   - a GameMode block that stores GameDaysPassed in a variable and tests it against
//     that variable plus some days, and enables a placed actor or asks two or more
//     whether they are dead (an encounter that comes back days after it was cleared);
//   - a GameMode block that asks the player's distance and then enables a placed actor
//     or starts a fight (an ambush as the player comes near);
//   - an `OnTriggerEnter player` block that enables two or more placed actors, each of
//     them disabled from the start (an ambush brought in, not people already there);
//   and none of the references it enables, gives a package, sets fighting or asks dead
//   is named by the story: a quest's script or stage result (but an encounter quest's
//   own) or a dialogue line's result script (a scene with people the story talks to).
// - **Its parts:** a script on a placed reference, naming no story but the cores'
//   quests, that sets a variable on a core's reference or quest, starts a core's quest,
//   or runs on a reference a core enables, gives a package, sets fighting or asks dead
//   (the encounter's triggers and creatures).
//
// [hypothesis, fit to the data: the official masters' respawning encounters, ambushes
// and their triggers are built this way; compared against the hand-picked list it
// replaces, outside the source]

/// Enable, GetDead, AddScriptPackage, StartCombat, GetDistance and StartQuest
/// (`crate::functions`).
const ENABLE: u16 = 0x1021;
const GET_DEAD: u16 = 0x102E;
const ADD_SCRIPT_PACKAGE: u16 = 0x1097;
const START_COMBAT: u16 = 0x1016;
const GET_DISTANCE: u16 = 0x1001;
const START_QUEST: u16 = 0x1036;
/// The functions that read or move a quest's story: SetStage, GetStage, GetStageDone,
/// SetObjectiveDisplayed, SetObjectiveCompleted, CompleteQuest, GetQuestRunning and
/// GetQuestCompleted (`crate::functions`).
const STORY_CALLS: [u16; 8] = [0x1039, 0x103A, 0x103B, 0x11A3, 0x11A2, 0x1071, 0x1038, 0x1222];

/// What one script does, for [`find_coop_encounters`].
#[derive(Debug, Default)]
struct EncounterFacts {
    /// It calls one of [`STORY_CALLS`].
    story: bool,
    /// The quests it names.
    quests: BTreeSet<FormId>,
    /// The references and quests whose variables it sets.
    sets_on: BTreeSet<FormId>,
    /// The quests it starts.
    starts: BTreeSet<FormId>,
    /// The references it enables, gives a package, sets fighting or asks dead.
    controls: BTreeSet<FormId>,
    /// One of its blocks makes it a core.
    core: bool,
}

/// One block's calls and its use of GameDaysPassed.
#[derive(Default)]
struct BlockScan {
    /// (opcode, the reference it runs on, its first reference argument).
    calls: Vec<(u16, Option<FormId>, Option<FormId>)>,
    days_tested: bool,
    days_stored: bool,
    sets_on: Vec<FormId>,
}

fn encounter_facts(g: &Game, unit: &Compiled, days: Option<FormId>) -> EncounterFacts {
    let data = g.data;
    let form = |r: RefIndex| match unit.unit.slot(r) {
        Some(RefSlot::Form(f)) => Some(FormId(f)),
        _ => None,
    };
    let is_days = |t: &Token| matches!(t, Token::Var { var } if var.on.is_none() && matches!(var.var, VarSlot::Global { global } if days.is_some() && form(global) == days));
    let scan_expr = |e: &Expr, b: &mut BlockScan| {
        for t in &e.0 {
            if let Token::Call { call } = t {
                let arg = match call.args.first() {
                    Some(Arg::Ref(r)) => form(*r),
                    _ => None,
                };
                b.calls.push((call.opcode, call.on.and_then(form), arg));
            }
        }
        e.0.iter().any(&is_days)
    };
    fn rec(s: &[Stmt], b: &mut BlockScan, scan: &dyn Fn(&Expr, &mut BlockScan) -> bool, form: &dyn Fn(RefIndex) -> Option<FormId>) {
        for st in s {
            match st {
                Stmt::Set { target, expr } => {
                    let days = scan(expr, b);
                    match target.on {
                        Some(r) => b.sets_on.extend(form(r)),
                        None => b.days_stored |= days && matches!(target.var, VarSlot::Local { .. }),
                    }
                }
                Stmt::If { arms, otherwise } => {
                    for a in arms {
                        let days = scan(&a.cond, b);
                        let has = |op: Op| a.cond.0.iter().any(|t| matches!(t, Token::Op { op: o } if *o == op));
                        b.days_tested |= days && has(Op::Add) && has(Op::Ge);
                        rec(&a.body, b, scan, form);
                    }
                    if let Some(o) = otherwise {
                        rec(o, b, scan, form);
                    }
                }
                Stmt::Call { call } => {
                    let arg = match call.args.first() {
                        Some(Arg::Ref(r)) => form(*r),
                        _ => None,
                    };
                    b.calls.push((call.opcode, call.on.and_then(form), arg));
                }
                Stmt::Unmatched { cond: Some(c), .. } => {
                    scan(c, b);
                }
                Stmt::While { body, .. } | Stmt::ForEach { body, .. } => rec(body, b, scan, form),
                _ => {}
            }
        }
    }
    let actor = |r: &FormId| g.world.placed.get(*r).is_some_and(|p| &p.sig == b"ACHR" || &p.sig == b"ACRE");
    let player = Some(FormId::PLAYER_REF);
    let mut f = EncounterFacts { quests: unit.unit.refs.iter().filter_map(|s| if let RefSlot::Form(x) = s { Some(FormId(*x)) } else { None }).filter(|q| data.quests.contains_key(q)).collect(), ..Default::default() };
    let tops = std::iter::once((None, None, &unit.program.top)).chain(unit.program.blocks.iter().map(|b| (Some(b.event), b.params.as_ref(), &b.body)));
    for (event, params, body) in tops {
        let mut b = BlockScan::default();
        rec(body, &mut b, &scan_expr, &form);
        let on_of = |op: u16| b.calls.iter().filter(move |c| c.0 == op).filter_map(|c| c.1);
        let enabled: BTreeSet<FormId> = on_of(ENABLE).filter(|r| actor(r)).collect();
        let dead: BTreeSet<FormId> = on_of(GET_DEAD).filter(|r| actor(r)).collect();
        f.story |= b.calls.iter().any(|c| STORY_CALLS.contains(&c.0));
        f.sets_on.extend(b.sets_on.iter().copied());
        f.starts.extend(b.calls.iter().filter(|c| c.0 == START_QUEST).filter_map(|c| c.2));
        for op in [ENABLE, ADD_SCRIPT_PACKAGE, START_COMBAT, GET_DEAD] {
            f.controls.extend(on_of(op));
        }
        let respawn = event == Some(crate::game::event::GAME_MODE) && b.days_tested && b.days_stored && (!enabled.is_empty() || dead.len() >= 2);
        let near = b.calls.iter().any(|c| c.0 == GET_DISTANCE && (c.1 == player || c.2 == player));
        let ambush = event == Some(crate::game::event::GAME_MODE) && near && (!enabled.is_empty() || b.calls.iter().any(|c| c.0 == START_COMBAT && c.1.is_some()));
        let by_player = matches!(params.and_then(|p| p.first()), Some(Arg::Ref(r)) if form(*r) == player);
        // Every actor it brings in starts disabled: a trigger that enables actors already
        // standing there sets off a scene, not an ambush.
        let starts_off = enabled.iter().all(|r| g.world.placed.get(*r).is_some_and(|p| p.initially_disabled()));
        let trigger = event == Some(crate::game::event::ON_TRIGGER_ENTER) && by_player && enabled.len() >= 2 && starts_off;
        f.core |= respawn || ambush || trigger;
    }
    f
}

/// The forms the story's scripts name: every quest's script and stage results but those
/// of the quests in `except` (the encounters' own), and every dialogue line's result
/// scripts.
fn story_named(g: &Game, except: &BTreeSet<FormId>) -> BTreeSet<FormId> {
    let data = g.data;
    let forms = |c: &Compiled| -> Vec<FormId> { c.unit.refs.iter().filter_map(|s| if let RefSlot::Form(x) = s { Some(FormId(*x)) } else { None }).collect() };
    let mut out = BTreeSet::new();
    for def in data.quests.iter().filter(|(q, _)| !except.contains(*q)).map(|(_, d)| d) {
        if let Some(c) = def.script.and_then(|s| data.script(s)) {
            out.extend(forms(c));
        }
        for e in def.stages.values().flatten() {
            if let Some(u) = e.unit {
                out.extend(forms(data.unit(u)));
            }
        }
    }
    for info in data.infos.values() {
        for u in [info.begin, info.end].into_iter().flatten() {
            out.extend(forms(data.unit(u)));
        }
    }
    out
}

/// The scripts of the random encounters every player sets off (the module notes above):
/// the cores on placed references and their parts.
/// How near the host a guest must be for a script's move of the host to take it along
/// [design: about the distance the HUD's compass shows].
pub const STORY_NEAR: f32 = 2000.0;

fn find_coop_encounters(g: &Game) -> BTreeSet<FormId> {
    let data = g.data;
    let days = data.by_edid("GameDaysPassed");
    // Script → what holds it: placed references and quests.
    let mut holders: BTreeMap<FormId, Vec<FormId>> = BTreeMap::new();
    let mut on_refs: BTreeSet<FormId> = BTreeSet::new();
    for &r in data.scripted_by_cell.values().chain(data.template_scripted_by_cell.values()).flatten() {
        if let Some(s) = g.ref_script(r) {
            holders.entry(s).or_default().push(r);
            on_refs.insert(s);
        }
    }
    for (q, def) in &data.quests {
        if let Some(s) = def.script {
            holders.entry(s).or_default().push(*q);
        }
    }
    let facts: BTreeMap<FormId, EncounterFacts> = holders.keys().filter_map(|&s| Some((s, encounter_facts(g, data.script(s)?, days)))).collect();
    let own_quests = |s: &FormId| -> BTreeSet<FormId> { holders.get(s).into_iter().flatten().copied().filter(|h| data.quests.contains_key(h)).collect() };
    let candidates: BTreeSet<FormId> = facts.iter().filter(|(s, f)| f.core && !f.story && f.quests.is_subset(&own_quests(s))).map(|(s, _)| *s).collect();
    let candidate_quests: BTreeSet<FormId> = candidates.iter().flat_map(own_quests).collect();
    let named = story_named(g, &candidate_quests);
    let cores: BTreeSet<FormId> = candidates.into_iter().filter(|s| facts.get(s).is_some_and(|f| f.controls.is_disjoint(&named))).collect();
    let core_quests: BTreeSet<FormId> = cores.iter().flat_map(own_quests).collect();
    let core_holders: BTreeSet<FormId> = cores.iter().filter_map(|s| holders.get(s)).flatten().copied().collect();
    let controlled: BTreeSet<FormId> = cores.iter().filter_map(|s| facts.get(s)).flat_map(|f| f.controls.iter().copied()).collect();
    let mut out: BTreeSet<FormId> = cores.intersection(&on_refs).copied().collect();
    for (s, f) in &facts {
        if cores.contains(s) || !on_refs.contains(s) || f.story || !f.quests.is_subset(&core_quests) {
            continue;
        }
        let controls_it = holders.get(s).is_some_and(|h| h.iter().any(|r| controlled.contains(r)));
        if !f.sets_on.is_disjoint(&core_holders) || !f.starts.is_disjoint(&core_holders) || controls_it {
            out.insert(*s);
        }
    }
    out
}

/// What a co-op guest's quest list shows: the host's active quest ([`Game::coop_host_quest`]).
#[derive(Debug, Clone, PartialEq)]
pub struct HostQuest {
    pub quest: FormId,
    /// Its objectives shown or done, by index: whether each is done.
    pub objectives: BTreeMap<i32, bool>,
    pub completed: bool,
    pub failed: bool,
}

/// Stages done a [`HostQuestState`] carries at most: the highest ones.
pub const HOST_STAGES_DONE: usize = 32;

/// The host's story as a co-op guest's world reads it ([`Game::coop_host_story`]).
///
/// A guest leaves the story to the host: its own game's quests stay where its save had
/// them (it never talks to the quest NPCs), so a door or activator whose script asks
/// the story found the guest's own quest instead of the host's: a gate whose OnActivate
/// opens only once a quest variable is set by talking to a guard, which the host did and
/// the guest never does, ran the script's other branch on the guest (the guard's
/// conversation, which no script starts with a guest), and the gate did nothing
/// [design].
///
/// - **What it holds.** Each quest the host's game has a state for (its stage, the
///   highest [`HOST_STAGES_DONE`] stages done, running, ended), and every variable of the
///   quests' scripts, as the host's game has them now ([`Game::story_view`]). nvforever's
///   `coop` sends them; a guest's game keeps the newest of each here.
/// - **Who reads it.** A script on a placed reference (a door, an activator, a trigger:
///   [`InstKey::Ref`]): `Quest.var`, GetStage, GetStageDone, GetQuestRunning and
///   GetQuestCompleted give the host's when it sent that quest [design]. The world's
///   references are the host's world's; a quest's own script (a mod's settings, the
///   guest's own story), a result script and a condition read the guest's own game, so
///   a guest's mod settings kept in quest variables stay its own [design].
/// - Nothing is written into the guest's quests: its own game, and its saves, keep them
///   [design: `save::KeepSolo`]. Not saved.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HostStory {
    pub quests: BTreeMap<FormId, HostQuestState>,
    /// Quest script variables: (quest, variable index) to value.
    pub vars: BTreeMap<(FormId, u16), fnv_script::vm::Value>,
}

/// One quest of the [`HostStory`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HostQuestState {
    /// GetStage.
    pub stage: i32,
    pub running: bool,
    /// Completed or failed (GetQuestCompleted).
    pub ended: bool,
    /// The highest stages done, at most [`HOST_STAGES_DONE`], in order.
    pub done: Vec<i32>,
    /// Failed (a Fail Quest stage, FailQuest); `ended` too.
    pub failed: bool,
    /// Its objectives shown or done, by index: whether each is done. A guest's quest list
    /// and objectives are the host's from these.
    pub objectives: BTreeMap<i32, bool>,
    /// When the host's quest list first showed it (the host's Pip-Boy's running count,
    /// larger is newer; 0: not yet), so a guest's list is in the host's order.
    pub seen: u64,
}

impl HostQuestState {
    /// GetStageDone: from the stages carried; a stage below the lowest carried, when some
    /// were left out, counts as done if it is not above the stage reached [hypothesis:
    /// close enough for a gate's check; the cap only bites on quests of more than
    /// [`HOST_STAGES_DONE`] stages].
    pub fn stage_done(&self, stage: i32) -> bool {
        if self.done.contains(&stage) {
            return true;
        }
        self.done.len() >= HOST_STAGES_DONE && self.done.first().is_some_and(|&low| stage < low)
    }
}

/// One of players 2–4 as saved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoopPlayer {
    /// The player's number, [`FIRST_COOP_SLOT`]..=[`MAX_PLAYERS`].
    pub slot: u8,
    /// The character's actor as the simulation keeps the player's: base form, cell,
    /// position and rotation, inventory (equipped items and their condition), the
    /// S.P.E.C.I.A.L. the world holds, scale, equip locks.
    pub actor: RefState,
    /// Heading and view pitch, degrees (the player's are in the save header's place).
    #[serde(default)]
    pub heading_deg: f32,
    #[serde(default)]
    pub pitch_deg: f32,
    /// Health and every other actor value (base, permanent and temporary modifiers,
    /// damage), dead, unconscious, factions, level: what `Game::extra` holds for the
    /// player's reference.
    #[serde(default, skip_serializing_if = "is_default_extra")]
    pub extra: RefExtra,
    /// Name, look and progression ([`Character`]).
    pub character: Character,
    /// Magic effects active on the character (aid items, abilities, worn enchantments,
    /// radiation and need stages' effects). `target` is written as the game's own player
    /// was when the effect was taken; it names this character, not that player.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<ActiveEffect>,
}

fn is_default_extra(e: &RefExtra) -> bool {
    *e == RefExtra::default()
}

/// What [`PlayerState`] holds that belongs to one character and not to the shared game:
/// who the character is and how it has grown. The fields have [`PlayerState`]'s names
/// and meanings. Left shared (in [`PlayerState`] only): map markers and fast travel,
/// dialogue topics, reputation and crimes, factions the player is an enemy of,
/// challenges, achievements, casino winnings and bans, the controls scripts disabled,
/// hardcore, the character-generation flag, Pip-Boy display switches.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Character {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub female: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub look: Option<crate::chargen::Look>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub race: Option<FormId>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub level: Option<i32>,
    #[serde(default)]
    pub xp: f64,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub perks: BTreeMap<FormId, u8>,
    #[serde(skip_serializing_if = "BTreeSet::is_empty", default)]
    pub tag_skills: BTreeSet<u16>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub tag_slots: BTreeMap<u8, u16>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub skill_points: BTreeMap<u16, f64>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub need_stages: BTreeMap<u16, FormId>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub misc_stats: BTreeMap<u16, f64>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub dead_count: BTreeMap<FormId, u32>,
    /// Notes and holotapes the character carries in its Pip-Boy.
    #[serde(skip_serializing_if = "BTreeSet::is_empty", default)]
    pub notes: BTreeSet<FormId>,
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub power_armor: bool,
    #[serde(skip_serializing_if = "crate::caravan::CaravanState::is_empty", default)]
    pub caravan: crate::caravan::CaravanState,
    /// The character's stable ID and its claimed quest stages (`crate::coop_joint`):
    /// they travel with the character into its own save.
    #[serde(skip_serializing_if = "crate::game::is_zero_u64", default)]
    pub character_id: u64,
    #[serde(skip_serializing_if = "crate::coop_joint::ClaimLedger::is_empty", default)]
    pub claims: crate::coop_joint::ClaimLedger,
    /// Its quest progression sync and the questlines it sits out
    /// (`crate::coop_questlines::QuestSync`): they go with it to every host.
    #[serde(skip_serializing_if = "crate::coop_questlines::QuestSync::is_empty", default)]
    pub quest_sync: crate::coop_questlines::QuestSync,
}

impl Character {
    /// The character part of the game's own player.
    pub fn of(p: &PlayerState) -> Character {
        Character {
            name: p.name.clone(),
            female: p.female,
            look: p.look.clone(),
            race: p.race,
            level: p.level,
            xp: p.xp,
            perks: p.perks.clone(),
            tag_skills: p.tag_skills.clone(),
            tag_slots: p.tag_slots.clone(),
            skill_points: p.skill_points.clone(),
            need_stages: p.need_stages.clone(),
            misc_stats: p.misc_stats.clone(),
            dead_count: p.dead_count.clone(),
            notes: p.notes.clone(),
            power_armor: p.power_armor,
            caravan: p.caravan.clone(),
            character_id: p.character_id,
            claims: p.claims.clone(),
            quest_sync: p.quest_sync.clone(),
        }
    }
}

impl Character {
    /// Put this character on the game's own player (the inverse of [`Character::of`]):
    /// the fields that are the character's; the rest of `p` (reputation, notes read,
    /// achievements and the like) stays as it is.
    pub fn put_on(&self, p: &mut PlayerState) {
        p.name = self.name.clone();
        p.female = self.female;
        p.look = self.look.clone();
        p.race = self.race;
        p.level = self.level;
        p.xp = self.xp;
        p.perks = self.perks.clone();
        p.tag_skills = self.tag_skills.clone();
        p.tag_slots = self.tag_slots.clone();
        p.skill_points = self.skill_points.clone();
        p.need_stages = self.need_stages.clone();
        p.misc_stats = self.misc_stats.clone();
        p.dead_count = self.dead_count.clone();
        p.notes = self.notes.clone();
        p.power_armor = self.power_armor;
        p.caravan = self.caravan.clone();
        p.character_id = self.character_id;
        p.claims = self.claims.clone();
        p.quest_sync = self.quest_sync.clone();
    }
}

impl crate::game::GameSnapshot {
    /// A co-op guest's save: this snapshot is the guest's own game as it was when it
    /// joined another player's world, and `now` is the game as it is in that world. The
    /// result is this game with the player's character as `now` has it (the actor's
    /// inventory, equipped items and S.P.E.C.I.A.L., health and every other actor value,
    /// level, XP, perks, skills, needs, statistics, notes, Caravan cards, the effects on
    /// the player, play time), and everything else as this one has it: the player's cell,
    /// position and rotation, the world, quests, globals and time. So a guest comes back
    /// to its own game where it left it, with what it gained.
    pub fn with_character_of(mut self, now: &crate::game::GameSnapshot) -> crate::game::GameSnapshot {
        let p = FormId::PLAYER_REF;
        if let Some(mut actor) = now.sim.refs.get(&p).cloned() {
            if let Some(old) = self.sim.refs.get(&p) {
                actor.cell = old.cell;
                actor.pos = old.pos;
                actor.rot = old.rot;
            }
            self.sim.refs.insert(p, actor);
        }
        match now.extra.get(&p) {
            Some(e) => {
                self.extra.insert(p, e.clone());
            }
            None => {
                self.extra.remove(&p);
            }
        }
        Character::of(&now.player).put_on(&mut self.player);
        // The checkpoints' records go with the inventory they were taken from:
        // a guest saved inside the Sierra Madre has its items back at the way out.
        self.coop_checked = now.coop_checked.clone();
        self.coop_checkpoint_gear = now.coop_checkpoint_gear.clone();
        // The player's effects as they are now, numbered after this game's own.
        self.effects.retain(|e| e.target != p);
        let mut next = self.next_effect.max(now.next_effect);
        for e in now.effects.iter().filter(|e| e.target == p) {
            let mut e = e.clone();
            e.id = next;
            next += 1;
            self.effects.push(e);
        }
        self.next_effect = next;
        self.time = self.time.max(now.time);
        self.sim.next_dynamic = self.sim.next_dynamic.max(now.sim.next_dynamic);
        self
    }
}

impl CoopPlayer {
    /// Player `slot` as a copy of the game's own player now (its actor, values, effects
    /// and character); `None` before the player's actor exists in the world.
    pub fn copy_of_player(g: &Game, slot: u8) -> Option<CoopPlayer> {
        let actor = g.world.peek(FormId::PLAYER_REF)?.clone();
        Some(CoopPlayer {
            slot,
            actor,
            heading_deg: 0.0,
            pitch_deg: 0.0,
            extra: g.extra.get(&FormId::PLAYER_REF).cloned().unwrap_or_default(),
            character: Character::of(&g.player),
            effects: g.effects.iter().filter(|e| e.target == FormId::PLAYER_REF).cloned().collect(),
        })
    }
}

/// Why a list of co-op players cannot be kept.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CoopError {
    #[error("co-op player slot {0}: slots are {FIRST_COOP_SLOT} to {MAX_PLAYERS}")]
    BadSlot(u8),
    #[error("co-op player slot {0} given twice")]
    Twice(u8),
}

/// Check a list of co-op players and put it in slot order.
pub fn checked(mut players: Vec<CoopPlayer>) -> Result<Vec<CoopPlayer>, CoopError> {
    players.sort_by_key(|p| p.slot);
    for (i, p) in players.iter().enumerate() {
        if !(FIRST_COOP_SLOT..=MAX_PLAYERS).contains(&p.slot) {
            return Err(CoopError::BadSlot(p.slot));
        }
        if i > 0 && players[i - 1].slot == p.slot {
            return Err(CoopError::Twice(p.slot));
        }
    }
    Ok(players)
}

impl Game<'_> {
    /// On a guest: the host's quest view is `new` now (nvforever `coop`). The quest it
    /// showed before keeps its objectives, for that quest's page once it ended.
    pub fn coop_set_host_quest(&mut self, new: Option<HostQuest>) {
        if self.coop_host_quest == new {
            return;
        }
        if let Some(old) = self.coop_host_quest.take() {
            self.coop_host_objectives.insert(old.quest, old.objectives);
        }
        self.coop_host_quest = new;
    }

    /// On a guest: the host's `quest` ended, failed or completed. The guest's
    /// quest list shows it as done, as the host's own does ([`HostQuest`]). Whether this
    /// is news: a quest failed once is not told again; a completed quest that fails after
    /// is (a Fail Quest stage also reads as completed, `Game::set_stage`).
    pub fn coop_host_quest_ended(&mut self, quest: FormId, failed: bool) -> bool {
        match self.coop_host_ended.iter().position(|(q, _)| *q == quest) {
            Some(i) if self.coop_host_ended[i].1 || !failed => false,
            Some(i) => {
                self.coop_host_ended[i].1 = true;
                true
            }
            None => {
                self.coop_host_ended.push((quest, failed));
                true
            }
        }
    }

    /// The host's quest view and ended quests are forgotten: this game is no guest now.
    pub fn coop_forget_host_quests(&mut self) {
        self.coop_host_quest = None;
        self.coop_host_ended.clear();
        self.coop_host_objectives.clear();
    }

    /// The host's: its story now, for the guests ([`HostStory`]): each quest with a state
    /// (running, ended or with a stage done) and every variable of the quests' scripts.
    pub fn story_view(&self) -> HostStory {
        let quests = self
            .quests
            .iter()
            .filter(|(_, s)| s.running || s.completed || s.failed || !s.stages_done.is_empty())
            .map(|(q, s)| {
                let skip = s.stages_done.len().saturating_sub(HOST_STAGES_DONE);
                let objectives = s.objectives.iter().filter(|(_, o)| o.displayed || o.completed).map(|(i, o)| (*i, o.completed)).collect();
                (*q, HostQuestState { stage: s.stage(), running: s.running, ended: s.completed || s.failed, done: s.stages_done.iter().skip(skip).copied().collect(), failed: s.failed, objectives, seen: 0 })
            })
            .collect();
        let vars = self
            .instances
            .iter()
            .filter_map(|(k, i)| match k {
                crate::game::InstKey::Quest { quest } => Some((*quest, i)),
                _ => None,
            })
            .flat_map(|(q, i)| i.vars.iter().map(move |(ix, v)| ((q, *ix), *v)))
            .collect();
        HostStory { quests, vars }
    }

    /// A guest's: the host's story for a script running as `inst`, when it reads the
    /// host's ([`HostStory`]: a guest's game, a placed reference's script).
    pub fn host_story(&self, inst: Option<crate::game::InstKey>) -> Option<&HostStory> {
        (self.coop_guest && matches!(inst, Some(crate::game::InstKey::Ref { .. }))).then_some(&self.coop_host_story)
    }

    /// Player `slot` (2–4), when the game has one.
    pub fn coop_player(&self, slot: u8) -> Option<&CoopPlayer> {
        self.coop.iter().find(|p| p.slot == slot)
    }

    /// Put in (or replace) the player of `p.slot`.
    pub fn set_coop_player(&mut self, p: CoopPlayer) -> Result<(), CoopError> {
        let mut all: Vec<CoopPlayer> = self.coop.iter().filter(|q| q.slot != p.slot).cloned().collect();
        all.push(p);
        self.coop = checked(all)?;
        Ok(())
    }

    /// Take player `slot` out of the game.
    pub fn remove_coop_player(&mut self, slot: u8) -> Option<CoopPlayer> {
        let i = self.coop.iter().position(|p| p.slot == slot)?;
        Some(self.coop.remove(i))
    }

    /// The lines a co-op guest may choose ([`Game::coop_guest`]), found once: those whose
    /// result scripts open the barter menu, deal a game of Caravan or exchange a casino's
    /// chips (a cashier, `exchanges_chips`), and those whose choices lead to one [design:
    /// a guest trades, plays and buys or cashes in chips, and leaves the quests to the
    /// host].
    pub fn guest_lines(&self) -> &BTreeSet<FormId> {
        self.guest_lines.get_or_init(|| guest_lines(self.data))
    }

    /// A replication guest's own script started (`started`) or stopped quest `q`: a story
    /// quest it started runs its script here until it stops ([`Game::guest_own_quests`]).
    /// True when the set changed; nothing off a replication guest.
    pub fn guest_own_quest(&mut self, q: FormId, started: bool) -> bool {
        if !self.world_logic_off {
            return false;
        }
        if started {
            self.guest_own_quests.insert(q)
        } else {
            self.guest_own_quests.remove(&q)
        }
    }

    /// A guest: `actor`, dead in this game and alive in the host's world, stands up again
    /// here (nvforever `coop`), and its death is taken back while in the host's world
    /// ([`Game::coop_deaths_taken_back`]), so GetDeadCount reads as it does in the world
    /// being played: a guest whose own save had killed a quest's targets would otherwise
    /// see the host's quest objectives on them as done, as objectives conditioned on
    /// GetDeadCount of a base being 0 are [observed: FalloutNV.esm QUST conditions;
    /// design]. The count itself stays, for the guest's own game. A script's
    /// ResurrectActor keeps the count [hypothesis].
    pub fn coop_revive(&mut self, actor: FormId) {
        if let Some(b) = crate::functions::base_of(self, actor) {
            let counted = self.player.dead_count.get(&b).copied().unwrap_or(0);
            let back = self.coop_deaths_taken_back.entry(b).or_default();
            if *back < counted {
                *back += 1;
            }
        }
        self.resurrect(actor, false);
    }

    /// The deaths of `base` GetDeadCount reads: the player's count less those a co-op
    /// guest took back ([`Game::coop_revive`]).
    pub fn dead_count(&self, base: FormId) -> u32 {
        let counted = self.player.dead_count.get(&base).copied().unwrap_or(0);
        counted.saturating_sub(self.coop_deaths_taken_back.get(&base).copied().unwrap_or(0))
    }

    /// Whether a guest's conversation with `actor` would offer anything but Goodbye.
    pub fn guest_can_talk(&mut self, actor: FormId) -> bool {
        let Some(greeting) = self.data.greeting else { return false };
        let goodbye = self.data.goodbye;
        self.guest_opening(actor, greeting).iter().any(|c| Some(c.topic) != goodbye)
    }

    /// What a guest's conversation with `actor` opens on: the choices of the line of
    /// `topic` (the greeting) the actor would say, the first it can say (no random draw),
    /// whose words the guest does not hear; with no such line, or one without choices, the
    /// choices `topic` offers by itself (the top-level topics). That is what a player
    /// alone gets ([`Game::build_choices`]: a line's own choices replace the top-level
    /// list): many vendors offer their trade only in their greeting's choices, and no
    /// top-level topic does [observed: FalloutNV.esm, Freeside vendors' greetings].
    /// Never both: a cashier whose greeting lists its own exchange and who also answers the
    /// casino cashiers' top-level exchange topics would offer each exchange twice
    /// [design].
    pub(crate) fn guest_opening(&mut self, actor: FormId, topic: FormId) -> Vec<crate::dialogue::Choice> {
        let data = self.data;
        let said = data.lines_tried(topic).iter().copied().find(|&i| self.can_say(actor, Some(FormId::PLAYER_REF), i));
        let Some(def) = said.and_then(|i| data.infos.get(&i)).filter(|d| !d.choices.is_empty()) else {
            return self.build_choices(actor, topic, &InfoDef::default());
        };
        let mut out = self.build_choices(actor, topic, def);
        // Goodbye last.
        let goodbye = data.goodbye;
        out.sort_by_key(|c| Some(c.topic) == goodbye);
        out
    }

    /// A guest's choices: only [`Game::guest_lines`], and Goodbye: the game's GOODBYE
    /// topic, or a line of the speaker's own that only takes leave ([`only_takes_leave`]),
    /// as a cashier of a casino outside the masters ends its talk with a topic of its own
    /// [design]. The game's GOODBYE is added only where the speaker offers no way out of
    /// its own.
    pub(crate) fn guest_choices(&mut self, actor: FormId, mut out: Vec<crate::dialogue::Choice>) -> Vec<crate::dialogue::Choice> {
        let data = self.data;
        let goodbye = data.goodbye;
        let lines = self.guest_lines();
        let leaves = |c: &crate::dialogue::Choice| Some(c.topic) == goodbye || data.infos.get(&c.info).is_some_and(|d| only_takes_leave(data, d));
        out.retain(|c| lines.contains(&c.info) || leaves(c));
        if let Some(bye) = goodbye.filter(|_| !out.iter().any(leaves)) {
            out.extend(self.offer_topic(actor, bye));
        }
        out
    }

    /// Whether `r` is another player's actor in a co-op game being played
    /// ([`Game::coop_actors`]).
    pub fn is_coop_actor(&self, r: FormId) -> bool {
        self.coop_actors.contains_key(&r)
    }

    /// `r`, or the player for another player's actor: whom the world's actors take it
    /// for in friendship and enmity [design: the players are one side].
    pub fn as_player(&self, r: FormId) -> FormId {
        if self.is_coop_actor(r) {
            FormId::PLAYER_REF
        } else {
            r
        }
    }

    /// Whether `event` on `r`'s own script is left to the host's world in a co-op
    /// guest's game: `r` is an NPC or creature placed in the world (ACHR, ACRE), not the
    /// player's teammate, and the block is not OnActivate (the guest's own use of it).
    /// The host's world runs its NPCs (a guest's actors' AI is off and they follow the
    /// host's, nvforever `coop`); their scripts' GameMode, OnDeath, OnHit and the rest
    /// run there, on the host's story. Run here as well they would act on the guest's own
    /// story: an NPC's GameMode that fails a quest when the NPC is dead before some stage
    /// would fail it in a guest's game that has not reached that stage when the host's
    /// fight kills the NPC [design].
    pub fn host_runs_script_of(&self, r: FormId, event: u16) -> bool {
        self.coop_guest
            && event != crate::game::event::ON_ACTIVATE
            && r != FormId::PLAYER_REF
            && self.world.placed.get(r).is_some_and(|p| &p.sig == b"ACHR" || &p.sig == b"ACRE")
            && !self.is_teammate(r)
    }

    /// The load order's random-encounter scripts (the notes before
    /// [`find_coop_encounters`]), found once.
    pub fn coop_encounters(&self) -> &BTreeSet<FormId> {
        self.coop_encounters.get_or_init(|| find_coop_encounters(self))
    }

    /// `r`'s script is one of the random encounters every player sets off
    /// ([`Game::coop_encounters`]), with other players in the game.
    pub fn coop_encounter(&self, r: FormId) -> bool {
        !self.coop_actors.is_empty() && self.ref_script(r).is_some_and(|s| self.coop_encounters().contains(&s))
    }

    /// Where `r` is: its cell's space (worldspace or interior) and position.
    pub(crate) fn space_pos(&self, r: FormId) -> Option<(FormId, [f32; 3])> {
        let s = self.world.peek(r)?;
        let c = s.cell?;
        Some((self.world.placed.world_of(c).unwrap_or(c), s.pos))
    }

    /// The player nearest `t`, the host's own or another player's actor, and its
    /// distance (`None`: `t` is nowhere, or no player is in its space).
    pub fn nearest_player(&self, t: FormId) -> Option<(FormId, f32)> {
        let (space, at) = self.space_pos(t)?;
        std::iter::once(FormId::PLAYER_REF)
            .chain(self.coop_actors.keys().copied())
            .filter_map(|p| self.space_pos(p).filter(|(s, _)| *s == space).map(|(_, q)| (p, (0..3).map(|i| (q[i] - at[i]).powi(2)).sum::<f32>().sqrt())))
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// The distance from `t` to the nearest player ([`Game::nearest_player`]).
    pub fn nearest_player_distance(&self, t: FormId) -> Option<f32> {
        self.nearest_player(t).map(|(_, d)| d)
    }

    /// Whether `is_in` holds for any player, the host's own or another player's actor.
    pub fn any_player(&self, mut is_in: impl FnMut(&Self, FormId) -> bool) -> bool {
        std::iter::once(FormId::PLAYER_REF).chain(self.coop_actors.keys().copied()).collect::<Vec<_>>().into_iter().any(|p| is_in(self, p))
    }

    /// What scripts put on the player that it still wears ([`Game::script_worn`]), with
    /// whether locked on: a co-op host's story gear, for the guests to wear too.
    pub fn story_gear(&self) -> Vec<(FormId, bool)> {
        let worn = |item: FormId| self.world.peek(FormId::PLAYER_REF).and_then(|s| s.inventory.as_ref()).is_some_and(|i| i.is_equipped(item));
        self.script_worn.iter().filter(|(item, _)| worn(**item)).map(|(item, locked)| (*item, *locked)).collect()
    }

    /// A co-op guest wears what the host's story put on the host's player (`host`, the
    /// host's [`Game::story_gear`]), and takes off what it no longer wears: a DLC start
    /// dresses every player alike (Dead Money's collar and jumpsuit,
    /// QUST 0x01000FBF stage 5; Old World Blues' patient gown, QUST 0x03002FCB stage 10)
    /// [observed: the ESM's script records; design]. An item the
    /// player lacks is added (as AddItem gives it, 99.9% health); a locked one is locked
    /// on here too. Taken off, an added item is taken away again, so nothing the host's
    /// story gave stays with the guest ([`Game::coop_story_gear`]). An item the guest
    /// already wears is its own and is left as it is, on and off ([`Game::coop_story_kept`]).
    /// `host` empty takes everything off (the guest left). Returns the items put on and
    /// taken off.
    pub fn follow_story_gear(&mut self, host: &[(FormId, bool)]) -> (Vec<FormId>, Vec<FormId>) {
        let me = FormId::PLAYER_REF;
        // An add-on checkpoint's gear only while this player's items are in it.
        let host: Vec<(FormId, bool)> = host.iter().copied().filter(|&(item, _)| self.coop_checkpoint_of(item) != CheckpointGear::Out).collect();
        let host = &host[..];
        let mut on = Vec::new();
        let wears = |g: &Self, item: FormId| g.world.peek(me).and_then(|s| s.inventory.as_ref()).is_some_and(|i| i.is_equipped(item));
        for &(item, locked) in host {
            if self.coop_story_gear.contains_key(&item) || self.coop_story_kept.contains(&item) {
                continue;
            }
            // Worn already (the guest's own Pip-Boy): its own, nothing to put on and
            // nothing to take off later.
            if wears(self, item) {
                self.coop_story_kept.insert(item);
                continue;
            }
            let had = self.world.item_count(me, item).unwrap_or(0) > 0;
            if !had && self.world.add_item_at_condition(me, item, 1, Some(0.999)).is_err() {
                continue;
            }
            let _ = self.world.lock_equip(me, item, None);
            if self.world.equip(me, item).is_ok() {
                if locked {
                    let _ = self.world.lock_equip(me, item, Some(fnv_sim::world::EquipLock::NoUnequip));
                }
                on.push(item);
            }
            self.coop_story_gear.insert(item, !had);
        }
        // The guest's own copy of an add-on checkpoint's gear the host's story took off
        // inside it goes too: NVDLC01MQ03 stage 100 takes the player's collar, whoever's
        // it is (design: guest B kept its own collar from its save).
        let dropped: Vec<FormId> = self.coop_story_kept.iter().copied().filter(|i| !host.iter().any(|(h, _)| h == i)).collect();
        for item in dropped {
            self.coop_story_kept.remove(&item);
            if matches!(self.coop_checkpoint_of(item), CheckpointGear::In(_)) {
                let _ = self.world.lock_equip(me, item, None);
                let _ = self.world.unequip(me, item);
                let _ = self.world.remove_item(me, item, 1);
                tracing::info!("co-op: {} taken from this player: the host's story took its own off inside the checkpoint", self.data.name(item));
            }
        }
        let gone: Vec<FormId> = self.coop_story_gear.keys().copied().filter(|i| !host.iter().any(|(h, _)| h == i)).collect();
        for &item in &gone {
            let added = self.coop_story_gear.remove(&item).unwrap_or(false);
            // An add-on checkpoint's gear the host's story took off inside it (Dead
            // Money's collar, NVDLC01MQ03 stage 100) goes from this player too.
            let mut checkpoint = false;
            for held in self.coop_checkpoint_gear.values_mut() {
                if let Some(i) = held.iter().position(|(h, _)| *h == item) {
                    held.remove(i);
                    checkpoint = true;
                }
            }
            let _ = self.world.lock_equip(me, item, None);
            let _ = self.world.unequip(me, item);
            if added || checkpoint {
                let _ = self.world.remove_item(me, item, 1);
            }
        }
        if !on.is_empty() || !gone.is_empty() {
            self.sync_actor_effects(me);
        }
        (on, gone)
    }

    /// Whether `actor` is a story NPC: one with a line of a story quest not yet completed
    /// or failed, or a line whose scripts set a stage of (or start) one ([`story_quest`]).
    /// Its speakers are found once, by the line's fixed speaker or its own GetIsID
    /// condition, as the coverage report finds them.
    pub fn story_npc(&mut self, actor: FormId) -> bool {
        let data = self.data;
        let base = crate::functions::base_of(self, actor);
        let speakers = self.story_speakers.get_or_insert_with(|| story_speakers(data));
        let quests: Vec<FormId> = [Some(actor), base].into_iter().flatten().filter_map(|k| speakers.get(&k)).flatten().copied().collect();
        quests.into_iter().any(|q| !self.quests.get(&q).is_some_and(|s| s.completed || s.failed))
    }

    /// The other players not within [`STORY_NEAR`] units of `actor` in its space.
    pub fn players_far_from(&self, actor: FormId) -> Vec<FormId> {
        let Some((space, at)) = self.space_pos(actor) else { return Vec::new() };
        self.coop_actors
            .keys()
            .copied()
            .filter(|p| !self.space_pos(*p).is_some_and(|(s, q)| s == space && (0..3).map(|i| (q[i] - at[i]).powi(2)).sum::<f32>() <= STORY_NEAR * STORY_NEAR))
            .collect()
    }

    /// A co-op host's talk with a story NPC waits until every other player is near it
    /// [design: the host could not talk to important NPCs when
    /// guests were too far; story NPCs, within sight]: true while one is not, with the
    /// wait put in [`Game::coop_story_wait`] for the host's screen and the far players'
    /// prompts. Asked when the host's player starts a conversation and when an NPC's
    /// Dialogue package would force-greet it (`fnv_ai`); a script's StartConversation is
    /// not held (its scene goes on). No gate for a guest's game or a game alone; never a
    /// choice for the host [design: the host never has to choose to
    /// wait].
    pub fn story_gate(&mut self, actor: FormId) -> bool {
        self.story_hold(actor, false)
    }

    /// [`Game::story_gate`] for an NPC's own force greet or Say To line (`fnv_ai`): held
    /// the same way, but the hold is shown on the screens only for its first
    /// [`NPC_WAIT_SHOWN`] seconds. The NPC's package asks again every update for as long
    /// as a guest is away, so a banner would stay up through the host's normal play
    /// [design: "WAITING FOR PLAYERS..." on the Strip while the host played,
    /// "it reads as a stuck co-op message"; a Strip Securitron's greet held for 16 s
    /// while the guest stayed in Freeside].
    pub fn story_gate_npc(&mut self, actor: FormId) -> bool {
        self.story_hold(actor, true)
    }

    fn story_hold(&mut self, actor: FormId, by_npc: bool) -> bool {
        if self.coop_guest || self.coop_actors.is_empty() {
            self.coop_story_wait = None;
            return false;
        }
        let (actor, _) = self.talker(actor);
        let far = self.players_far_from(actor);
        if far.is_empty() || !self.story_npc(actor) {
            if self.coop_story_wait.as_ref().is_some_and(|w| w.actor == actor) {
                self.coop_story_wait = None;
            }
            return false;
        }
        // The same hold asked again keeps its start; the host's own try shows it again.
        let since = match &self.coop_story_wait {
            Some(w) if w.actor == actor && self.time - w.at <= NPC_WAIT_SHOWN && (by_npc || !w.by_npc) => w.since,
            _ => self.time,
        };
        let by_npc = by_npc && self.coop_story_wait.as_ref().is_none_or(|w| w.actor != actor || w.by_npc || self.time - w.at > NPC_WAIT_SHOWN);
        self.coop_story_wait = Some(StoryWait { actor, far, at: self.time, since, by_npc });
        true
    }
}

/// A co-op host's talk held for the other players ([`Game::story_gate`]).
#[derive(Debug, Clone, PartialEq)]
pub struct StoryWait {
    /// The story NPC.
    pub actor: FormId,
    /// The other players' actors that are not near it.
    pub far: Vec<FormId>,
    /// When the talk was last asked for ([`Game::time`]).
    pub at: f64,
    /// When this hold began ([`Game::time`]): asked again without a break, it keeps it.
    pub since: f64,
    /// Asked by the NPC's own greet, not the host's try to talk
    /// ([`Game::story_gate_npc`]).
    pub by_npc: bool,
}

/// How long (game seconds) a hold an NPC's greet asked for shows on the screens
/// ([`Game::story_gate_npc`]) [design: long enough to read the banner once].
pub const NPC_WAIT_SHOWN: f64 = 6.0;

impl StoryWait {
    /// Whether the screens show this hold at `now` ([`NPC_WAIT_SHOWN`]): always for the
    /// host's own try to talk, only at first for an NPC's greet.
    pub fn shown(&self, now: f64) -> bool {
        !self.by_npc || now - self.since < NPC_WAIT_SHOWN
    }
}

/// A story quest: the main quest's (by its Editor ID prefixes in FalloutNV.esm, as the
/// coverage report ranks them) or a DLC's main quest (NVDLC01MQ… to NVDLC04MQ…)
/// [observed: the Editor IDs of the four story add-ons' main quests].
pub(crate) fn story_quest(data: &GameData, q: FormId) -> bool {
    let Some(e) = data.edid_of.get(&q) else { return false };
    let e = e.to_ascii_lowercase();
    let plugin = data.load_order.get((q.0 >> 24) as usize).map(|p| p.to_ascii_lowercase()).unwrap_or_default();
    (plugin == "falloutnv.esm" && crate::coverage::MAIN_PREFIXES.iter().any(|p| e.starts_with(p))) || dlc_main_quest(&e)
}

/// NVDLC0nMQ…: an add-on's main quest.
fn dlc_main_quest(edid: &str) -> bool {
    edid.len() > 9 && edid.starts_with("nvdlc0") && edid.as_bytes()[6].is_ascii_digit() && &edid[7..9] == "mq"
}

/// The quests a compiled script names: the quests in its reference list (SetStage,
/// StartQuest, a quest variable set or read). A unit without compiled data falls back on
/// its source, the Editor ID after a SetStage or StartQuest [observed: Honest Hearts' Jed
/// greeting, line 0x0100F195, starts NVDLC02MQ01 by its reference list; the source scan
/// alone left Jed out [design: a found bug].
fn unit_quests(data: &GameData, unit: &fnv_script::unit::ScriptUnit) -> Vec<FormId> {
    if unit.has_scda {
        return unit
            .refs
            .iter()
            .copied()
            .filter_map(|r| match r {
                fnv_script::unit::RefSlot::Form(f) if data.is_quest(FormId(f)) => Some(FormId(f)),
                _ => None,
            })
            .collect();
    }
    unit.source.as_deref().map(|src| quests_named(data, src)).unwrap_or_default()
}

/// The quests a script's source sets a stage of or starts (SetStage, StartQuest), by the
/// Editor ID after the call.
fn quests_named(data: &GameData, source: &str) -> Vec<FormId> {
    let low = source.to_ascii_lowercase();
    let mut out = Vec::new();
    for call in ["setstage", "startquest"] {
        for (i, _) in low.match_indices(call) {
            let rest = low[i + call.len()..].trim_start();
            let name: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            if let Some(q) = data.by_edid(&name) {
                out.push(q);
            }
        }
    }
    out
}

/// Every story quest's speakers ([`Game::story_npc`]).
fn story_speakers(data: &GameData) -> std::collections::HashMap<FormId, Vec<FormId>> {
    let mut out: std::collections::HashMap<FormId, Vec<FormId>> = std::collections::HashMap::new();
    for info in data.infos.values() {
        let mut quests: Vec<FormId> = info.quest.into_iter().filter(|q| story_quest(data, *q)).collect();
        for u in [info.begin, info.end].into_iter().flatten() {
            quests.extend(unit_quests(data, &data.unit(u).unit).into_iter().filter(|q| story_quest(data, *q)));
        }
        if quests.is_empty() {
            continue;
        }
        let Some(speaker) = info.speaker.or_else(|| crate::coverage::line_actor(data, info)) else { continue };
        let list = out.entry(speaker).or_default();
        for q in quests {
            if !list.contains(&q) {
                list.push(q);
            }
        }
    }
    out
}

/// A co-op guest at the host's checkpoint (observed: buckallobill, in game, FNV 1.4.0.525; `Game::coop_checked`):
/// a Strip casino's weapon check took the host's player's weapons into a container, all
/// but those in an exception list (the guard's dialogue result, `RemoveAllTypedItems`
/// with type 40), and the door that lets the player out gives them back (the container's
/// `RemoveAllItems player`) [observed: FalloutNV.esm INFO 0x0012793B / 0x0012793F and
/// the Gomorrah door script SCPT 0x00127959, script records]. The Fort's gate takes
/// them by several such calls (types 40 and 47, each with its own list) and gives them
/// back by the container's `RemoveAllTypedItems player` [observed: SCPT 0x00164763]
///. A guest's own game runs
/// neither, so it hands over and takes back its own as the host's player does.
impl Game<'_> {
    /// Hand over this player's items of form type `code` (-1: all), but those in
    /// `except` (a form list), to checkpoint `container`; what it took, counted.
    pub fn coop_check_in(&mut self, container: FormId, code: i32, except: Option<FormId>) -> i32 {
        if code == 0 {
            return 0;
        }
        let except = except.map(|l| crate::functions::refs::list_members(self, l)).unwrap_or_default();
        // In the checkpoint even with nothing to hand over (`Game::coop_checkpoint_of`).
        self.coop_checked.entry(container).or_default();
        let forms = self.world.forms;
        let stacks: Vec<(FormId, i32)> = self.world.peek(FormId::PLAYER_REF).and_then(|s| s.inventory.as_ref()).map(|i| i.iter().collect()).unwrap_or_default();
        let mut took = 0;
        for (item, n) in stacks {
            let Some(it) = forms.item(item) else { continue };
            if it.non_playable_biped() || except.contains(&item) || self.world.is_quest_item(item) {
                continue;
            }
            if code != -1 && forms.sig(item).as_ref().and_then(crate::functions::items::form_type_code) != Some(code) {
                continue;
            }
            let Ok(inv) = self.world.inventory_mut(FormId::PLAYER_REF) else { continue };
            let groups = fnv_sim::inventory::unequipped(inv.take(item, n));
            let held = self.coop_checked.entry(container).or_default();
            for (extra, c) in groups {
                took += c;
                held.push((item, c, extra));
            }
        }
        took
    }

    /// Give this player back what checkpoint `container` holds of its; how many. The
    /// checkpoint's gear it was handed meanwhile goes first ([`Game::coop_checkpoint_of`]).
    pub fn coop_check_out(&mut self, container: FormId) -> i32 {
        let me = FormId::PLAYER_REF;
        // What the checkpoint's story handed this player and the host's story did not take
        // back is its own now, as the original leaves it with the host.
        let gear = self.coop_checkpoint_gear.remove(&container).unwrap_or_default();
        for (item, _) in gear {
            if self.coop_story_gear.remove(&item).is_some() {
                let _ = self.world.lock_equip(me, item, None);
            }
            if self.world.item_count(me, item).unwrap_or(0) > 0 {
                tracing::info!("co-op: {} is this player's now: the checkpoint {container} gives its items back", self.data.name(item));
            }
        }
        let Some(held) = self.coop_checked.remove(&container) else { return 0 };
        let mut gave = 0;
        for (item, c, extra) in held {
            if self.world.add_item_extra(FormId::PLAYER_REF, item, c, extra).is_ok() {
                gave += c;
            }
        }
        gave
    }
}

/// Where an add-on checkpoint's gear may go [design: guests
/// were seen in Dead Money's gear].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointGear {
    /// Not an add-on checkpoint's: it goes anywhere.
    Free,
    /// The checkpoint of this container holds this player's items now: it goes, and is
    /// taken away again when the checkpoint gives them back.
    In(FormId),
    /// An add-on checkpoint's, and this player's items are not in it: it does not go.
    Out,
}

/// The containers the load order's scripts put the player's whole inventory in: a
/// `player.RemoveAllItems <container>` call (Dead Money's bunker, SCPT 0x0100C59E into
/// NVDLC01PlayerEquipmentContainerRef 0x0100EFCC; FalloutNV.esm SCPT 0x0015E57A and INFO
/// 0x000E2849 into 0x000E3669) [observed: the ESM's script records].
pub fn strip_containers_of(data: &GameData) -> BTreeSet<FormId> {
    use fnv_script::ir::{Arg, Stmt};
    use fnv_script::unit::RefSlot;
    let remove_all = data.table.by_name("RemoveAllItems");
    let mut out = BTreeSet::new();
    for c in &data.units {
        c.program.walk(&mut |s| {
            let Stmt::Call { call } = s else { return };
            if Some(call.opcode) != remove_all || !matches!(call.on.and_then(|r| c.unit.slot(r)), Some(RefSlot::Form(f)) if f == FormId::PLAYER_REF.0) {
                return;
            }
            if let Some(Arg::Ref(r)) = call.args.first() {
                if let Some(RefSlot::Form(f)) = c.unit.slot(*r) {
                    out.insert(FormId(f));
                }
            }
        });
    }
    out
}

/// An add-on whose story takes the player's whole inventory into a container (Dead
/// Money's Sierra Madre) owns the gear its story hands out there: its quests' hand-outs
/// and the story gear of its own forms (the collar, the jumpsuit, the holorifle and its
/// cells, QUST 0x01000FBF stage 5) [design]. In the original that gear is the
/// player's from the bunker on, where the player's own items are taken (SCPT
/// 0x0100C59E), to the way out, where NVDLC01MQ03 stage 100 takes the collar and the
/// container gives the player's items back (SCPT 0x01013FB4) [observed: the ESM's script
/// records]; nothing else on the way out takes the jumpsuit, the holorifle, the cells
/// or the Sierra Madre chips (0x01011236; only dialogue lines that exchange chips take
/// them) [observed: the same]. So a co-op guest gets that gear only while that
/// checkpoint holds its own items (`Game::coop_check_in`), loses what the
/// host's story takes off inside (the collar), and keeps the rest as its own when the
/// checkpoint gives its items back, as the host does [design: the
/// original wins]. A guest that joins later, or a host that set the stage outside (a
/// console `SetStage NVDLC01MQ01 5` in Goodsprings), hands it to no guest.
/// The base game's own such container (0x000E3669) makes no gear of its quests.
impl Game<'_> {
    /// Where `form` (a hand-out's quest, or an item of the host's story gear) may go on
    /// this player ([`CheckpointGear`]).
    pub fn coop_checkpoint_of(&mut self, form: FormId) -> CheckpointGear {
        let data = self.data;
        let plugin = form.0 >> 24;
        if plugin == 0 || form.0 >= 0xFF00_0000 {
            return CheckpointGear::Free;
        }
        let strip = self.strip_containers.get_or_insert_with(|| strip_containers_of(data));
        if !strip.iter().any(|c| c.0 >> 24 == plugin) {
            return CheckpointGear::Free;
        }
        match self.coop_checked.keys().find(|c| c.0 >> 24 == plugin && strip.contains(c)) {
            Some(c) => CheckpointGear::In(*c),
            None => CheckpointGear::Out,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_hosts_stages_done_are_read_from_what_came() {
        use super::{HostQuestState, HOST_STAGES_DONE};
        let few = HostQuestState { stage: 20, running: true, ended: false, done: vec![10, 20], ..Default::default() };
        assert!(few.stage_done(10) && few.stage_done(20));
        assert!(!few.stage_done(5) && !few.stage_done(30));
        // A quest of more stages than came: the ones below the lowest carried were done.
        let many = HostQuestState { stage: 400, running: true, ended: false, done: (0..HOST_STAGES_DONE as i32).map(|i| 100 + 10 * i).collect(), ..Default::default() };
        assert!(many.stage_done(50));
        assert!(many.stage_done(110));
        assert!(!many.stage_done(105));
    }
}
