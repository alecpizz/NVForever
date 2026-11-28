//! Lutana NVSE (LU, merged into JIP LN), batch 1 (#1009): challenges, casinos, recipes,
//! actor AI data, perks, cells and water, encounter zone flags, activators, terminals and
//! locks, form flags and playability, weapon fields, load screen types, and a few misc
//! getters [documented: each page in the wiki export; docs/spec-notes/ext-lutana.md].
//! Setters change the record for the session, read by the getters; where a game system
//! reads the same value through one accessor (aggro radius, services, casino max
//! winnings, encounter zone flags) it sees the change too.

use crate::casino::CasinoDef;
use crate::challenges::ChallengeDef;
use crate::crafting::RecipeDef;
use crate::game::{Game, RunCtx};
use crate::nvse::{field_number, record_value, F32, U32, U8};
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;
use std::collections::HashMap;

/// What this batch's setters changed, for the session.
#[derive(Debug, Default)]
pub struct LutanaState {
    /// Batch 2's state: map markers, radio data, hotkey units (`nvse_lu2`).
    pub lu2: crate::nvse_lu2::State,
    /// Whole copies of the records a setter changed, read by the getters (and by
    /// [`challenge`], [`casino`], [`recipe`]).
    pub challenges: HashMap<FormId, ChallengeDef>,
    pub casinos: HashMap<FormId, CasinoDef>,
    pub recipes: HashMap<FormId, RecipeDef>,
    /// SetServiceFlag's flags and LNSetAggroRadius's radius, by the actor record changed
    /// (`Game::services`, `Game::aggro_radius`).
    pub services: HashMap<FormId, u32>,
    pub aggro_radius: HashMap<FormId, u32>,
    /// SetRadius on a reference (its XRDS), read by `GetRadius` and
    /// `Game::radiation_markers_in`, and carried across same-run loads in `Game::Session`.
    pub ref_radius: HashMap<FormId, f32>,
    /// SetDefaultMessageTime's seconds.
    pub message_time: Option<f64>,
    /// SetRecipeRequiredSkillLevel's level and the level SetRecipeRequiredSkill keeps when
    /// it takes the skill away: the level stands apart from the skill [observed: lutana1
    /// rcpe.setskill.neg, rcpe.setlevel.noskill].
    pub recipe_levels: HashMap<FormId, u32>,
}

/// A challenge as it stands now: a script's changes, else the record's (CHAL).
pub fn challenge<'a>(g: &'a Game, c: FormId) -> Option<&'a ChallengeDef> {
    g.other.lutana.challenges.get(&c).or_else(|| g.data.challenge_defs.get(&c))
}

/// A casino as it stands now: a script's changes, else the record's (CSNO).
pub fn casino<'a>(g: &'a Game, c: FormId) -> Option<&'a CasinoDef> {
    g.other.lutana.casinos.get(&c).or_else(|| g.data.casinos.get(&c))
}

/// A recipe as it stands now: a script's changes, else the record's (RCPE).
pub fn recipe<'a>(g: &'a Game, r: FormId) -> Option<&'a RecipeDef> {
    g.other.lutana.recipes.get(&r).or_else(|| g.data.recipes.get(&r))
}

/// The default display time of the corner messages: SetDefaultMessageTime's, else 2
/// seconds [documented: GetMessageDisplayTime, "defaults to 2.0"].
pub fn message_time(g: &Game) -> f64 {
    g.other.lutana.message_time.unwrap_or(2.0)
}

/// The record copied into `map` on the first change, then changed there.
fn copy_on_write<'a, T: Clone>(map: &'a mut HashMap<FormId, T>, from: Option<&T>, f: FormId) -> Option<&'a mut T> {
    match map.entry(f) {
        std::collections::hash_map::Entry::Occupied(e) => Some(e.into_mut()),
        std::collections::hash_map::Entry::Vacant(e) => Some(e.insert(from?.clone())),
    }
}

fn challenge_mut<'a>(g: &'a mut Game, c: FormId) -> Option<&'a mut ChallengeDef> {
    let from = g.data.challenge_defs.get(&c);
    copy_on_write(&mut g.other.lutana.challenges, from, c)
}

fn casino_mut<'a>(g: &'a mut Game, c: FormId) -> Option<&'a mut CasinoDef> {
    let from = g.data.casinos.get(&c);
    copy_on_write(&mut g.other.lutana.casinos, from, c)
}

/// A recipe's required skill level: a script's, else the record's skill's (0 without one).
fn recipe_level(g: &Game, r: FormId) -> u32 {
    g.other.lutana.recipe_levels.get(&r).copied().or_else(|| recipe(g, r).and_then(|d| d.skill).map(|s| s.1)).unwrap_or(0)
}

fn recipe_mut<'a>(g: &'a mut Game, r: FormId) -> Option<&'a mut RecipeDef> {
    let from = g.data.recipes.get(&r);
    copy_on_write(&mut g.other.lutana.recipes, from, r)
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
    match args.get(k) {
        Some(XVal::Num(x)) => Some(*x),
        _ => None,
    }
}

/// A whole number as a script stores it in a 32-bit record field: truncated and kept as
/// given, a negative number too (read back with [`signed`]) [observed: lutana1
/// chal.setthreshold.neg, csno.setmaxwinnings.neg, csno.setnumdecks.0].
fn int_bits(v: f64) -> u32 {
    v.trunc().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32 as u32
}

/// A 32-bit record field read as the game reads it back, signed.
fn signed(x: u32) -> f64 {
    f64::from(x as i32)
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

fn reff(f: Option<FormId>) -> XVal {
    XVal::Ref(f.map_or(0, |f| f.0))
}

fn record_ref(g: &Game, f: FormId, sub: &str) -> Option<FormId> {
    match record_value(g, f, sub, "")? {
        XVal::Ref(0) => None,
        XVal::Ref(r) => Some(FormId(r)),
        _ => None,
    }
}

fn record_num(g: &Game, f: FormId, sub: &str, path: &str) -> Option<f64> {
    match record_value(g, f, sub, path)? {
        XVal::Num(x) => Some(x),
        _ => None,
    }
}

fn list(g: &mut Game, items: impl Iterator<Item = XVal>) -> XVal {
    let entries = items.enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

fn string_map(g: &mut Game, items: Vec<(&str, f64)>) -> XVal {
    let entries = items.into_iter().map(|(k, v)| (XKey::Str(k.to_string()), XVal::Num(v))).collect();
    XVal::Array(g.ext.new_array(ArrayKind::StringMap, entries))
}

fn base(g: &Game, f: FormId) -> FormId {
    crate::functions::base_of(g, f).unwrap_or(f)
}

/// The actor record `f` names (a reference's base): SetServiceFlag and LNSetAggroRadius
/// change this record's AI data, not its template's, which a "Use AI Data" NPC reads
/// until then [observed: lutana1 actor.setservice.tender2 (the NPC reads the change, its
/// template does not), actor.service.tender2.4 (the template's flags read before)].
fn ai_record(g: &Game, f: FormId) -> Option<FormId> {
    if g.world.placed.get(f).is_some() || g.world.peek(f).is_some() {
        return crate::functions::base_of(g, f);
    }
    g.data.actors.contains_key(&f).then_some(f)
}

/// The services as they stand for the actor record `rec`: SetServiceFlag's, else the AI
/// data it uses (its own, or its template's with "Use AI Data").
fn services_of(g: &Game, rec: FormId) -> u32 {
    g.other.lutana.services.get(&rec).copied().unwrap_or_else(|| ai_source(g, rec).and_then(|s| g.data.actors.get(&s)).map_or(0, |d| d.services))
}

fn aggro_of(g: &Game, rec: FormId) -> u32 {
    g.other.lutana.aggro_radius.get(&rec).copied().unwrap_or_else(|| ai_source(g, rec).and_then(|s| g.data.actors.get(&s)).map_or(0, |d| d.aggro_radius))
}

/// The actor record whose AI data `f` (an actor reference or base) uses, following its
/// template while "Use AI Data" is set [documented: xEdit template flags; as
/// `Game::services` and `Game::aggro_radius` read it].
fn ai_source(g: &Game, f: FormId) -> Option<FormId> {
    if g.world.placed.get(f).is_some() || g.world.peek(f).is_some() {
        let b = crate::functions::base_of(g, f)?;
        return g.template_source(b, f, |d| d.template_ai).map(|(id, _)| id);
    }
    g.data.actors.contains_key(&f).then(|| g.data.base_template_source(f, |d| d.template_ai))
}

/// The actor record whose traits `f` (an actor reference or base) uses, following its
/// template while "Use Traits" is set, as [`ai_source`] does for the AI data; the combat
/// style is on the Traits tab [documented: GECK wiki Traits Tab - NPC, Template Data].
fn traits_source(g: &Game, f: FormId) -> Option<FormId> {
    if g.world.placed.get(f).is_some() || g.world.peek(f).is_some() {
        let b = crate::functions::base_of(g, f)?;
        return g.template_source(b, f, |d| d.template_traits).map(|(id, _)| id);
    }
    g.data.actors.contains_key(&f).then(|| g.data.base_template_source(f, |d| d.template_traits))
}

/// The cell `args[k]` names, else the player's [documented: GetCellWaterForm, "If no
/// cell is specified, the player's current cell is used"].
fn cell_arg(g: &Game, args: &[XVal], k: usize) -> Option<FormId> {
    form(args, k).or_else(|| g.cell_of(FormId::PLAYER_REF))
}

/// A cell's water form: its XCWT, else for an exterior cell its worldspace's NAM2
/// [documented: GetCellWaterForm, "the parent worldspace's water form for exterior
/// cells"].
fn cell_water(g: &Game, cell: FormId) -> Option<FormId> {
    record_ref(g, cell, "XCWT").or_else(|| g.world.placed.cell_world.get(&cell).and_then(|&w| record_ref(g, w, "NAM2")))
}

/// The reference the lock functions act on: the argument, else the calling reference.
fn lockable(args: &[XVal], k: usize, subject: Option<FormId>) -> Option<FormId> {
    form(args, k).or(subject)
}

fn is_terminal(g: &Game, r: FormId) -> bool {
    g.data.sig(base(g, r)) == Some(*b"TERM")
}

/// The functions of this file; `None` for any other name.
pub(crate) fn call(g: &mut Game, _ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let v = match name {
        // ---- challenges (CHAL) [documented: each Get/SetChallenge… page] ----
        "getchallengeform1" | "getchallengeform2" | "getchallengeinterval" | "getchallengethreshold" | "getchallengetype" | "getchallengevalue1"
        | "getchallengevalue2" | "getchallengevalue3" | "getchallengeflags" => {
            let Some(d) = form(args, 0).and_then(|c| challenge(g, c)) else {
                return Some(Ok(if name.starts_with("getchallengeform") { XVal::Ref(0) } else { zero }));
            };
            match name {
                "getchallengeform1" => reff(d.form1),
                "getchallengeform2" => reff(d.form2),
                "getchallengeinterval" => XVal::Num(signed(d.interval)),
                "getchallengethreshold" => XVal::Num(signed(d.threshold)),
                // The DATA type (0 KillInFormList … 13 Scripted) [documented:
                // GetChallengeType's table].
                "getchallengetype" => XVal::Num(d.kind as f64),
                "getchallengevalue1" => XVal::Num(signed(d.value1)),
                "getchallengevalue2" => XVal::Num(signed(d.value2)),
                "getchallengevalue3" => XVal::Num(signed(d.value3)),
                _ => XVal::Num(signed(d.flags)),
            }
        }
        "setchallengeform1" | "setchallengeform2" => {
            let f = form(args, 1);
            if let Some(d) = form(args, 0).and_then(|c| challenge_mut(g, c)) {
                if name == "setchallengeform1" {
                    d.form1 = f;
                } else {
                    d.form2 = f;
                }
            }
            zero
        }
        // The numbers as given, a negative one too [observed: lutana1
        // chal.setthreshold.neg reads -5]; NaN refused.
        "setchallengeinterval" | "setchallengethreshold" | "setchallengevalue1" | "setchallengevalue2" | "setchallengevalue3" | "setchallengeflags" => {
            let v = num(args, 1).filter(|v| v.is_finite()).map(int_bits);
            if let (Some(v), Some(d)) = (v, form(args, 0).and_then(|c| challenge_mut(g, c))) {
                match name {
                    "setchallengeinterval" => d.interval = v,
                    "setchallengethreshold" => d.threshold = v,
                    "setchallengevalue1" => d.value1 = v,
                    "setchallengevalue2" => d.value2 = v,
                    "setchallengevalue3" => d.value3 = v,
                    _ => d.flags = v,
                }
            }
            zero
        }
        // ---- casinos (CSNO) [documented: each Get/SetCasino… page] ----
        "getcasinobj17stand" | "getcasinobjpayout" | "getcasinomaxwinnings" | "getcasinonumdecks" | "getcasinoshufflepercent" | "getcasinoreelstops" => {
            let Some(c) = form(args, 0).and_then(|c| casino(g, c)) else { return Some(Ok(zero)) };
            XVal::Num(match name {
                "getcasinobj17stand" => f64::from(u8::from(c.stay_soft17)),
                "getcasinobjpayout" => crate::nvse::record_float(f64::from(c.payout)),
                "getcasinomaxwinnings" => signed(c.max_winnings),
                "getcasinonumdecks" => signed(c.decks),
                // A share, 1.0 all of the shoe [documented: "1.0 is 100%"].
                "getcasinoshufflepercent" => crate::nvse::record_float(f64::from(c.shuffle_share)),
                // `casino symbol`: symbols 1-7, 7 the W [documented]; another symbol -1
                // [observed: lutana1 csno.reel.0, csno.reel.8].
                _ => match num(args, 1).map(|s| s as i64) {
                    Some(s @ 1..=7) => signed(c.reel_stops[(s - 1) as usize]),
                    _ => -1.0,
                },
            })
        }
        "setcasinobj17stand" | "setcasinobjpayout" | "setcasinomaxwinnings" | "setcasinonumdecks" | "setcasinoshufflepercent" => {
            let v = num(args, 1).filter(|v| v.is_finite());
            if let (Some(v), Some(c)) = (v, form(args, 0).and_then(|c| casino_mut(g, c))) {
                match name {
                    "setcasinobj17stand" => c.stay_soft17 = v != 0.0,
                    "setcasinobjpayout" => c.payout = v.clamp(F32.0, F32.1) as f32,
                    // As given, -1 and 0 decks too [observed: lutana1
                    // csno.setmaxwinnings.neg, csno.setnumdecks.0].
                    "setcasinomaxwinnings" => c.max_winnings = int_bits(v),
                    "setcasinonumdecks" => c.decks = int_bits(v),
                    // As given, outside 0-1 too [observed: lutana1
                    // csno.setshufflepercent.over, csno.setshufflepercent.neg].
                    _ => c.shuffle_share = v.clamp(F32.0, F32.1) as f32,
                }
            }
            zero
        }
        // `casino symbol stops`: the count as given; the page's "must equal 14" is the
        // record's rule, not checked here (see docs/spec-notes/ext-lutana.md §2).
        "setcasinoreelstops" => {
            let (s, v) = (num(args, 1).map(|s| s as i64), num(args, 2).filter(|v| v.is_finite()).map(int_bits));
            if let (Some(s @ 1..=7), Some(v)) = (s, v) {
                if let Some(c) = form(args, 0).and_then(|c| casino_mut(g, c)) {
                    c.reel_stops[(s - 1) as usize] = v;
                }
            }
            zero
        }
        // ---- recipes (RCPE) [documented: each page] ----
        // A recipe's lists act on the first entry of the item: the count read, set,
        // removed or replaced is that entry's; added entries go first [observed: lutana1
        // rcpe.addinput, rcpe.addinput.order, rcpe.addinput.existing, rcpe.replaceinput,
        // rcpe.setincount.unlisted: an item added twice is listed twice, the later first].
        "getrecipeinputcount" | "getrecipeoutputcount" => {
            let (Some(r), Some(item)) = (form(args, 0).and_then(|r| recipe(g, r)), form(args, 1)) else { return Some(Ok(zero)) };
            let entries = if name == "getrecipeinputcount" { &r.ingredients } else { &r.outputs };
            XVal::Num(entries.iter().find(|e| e.0 == item).map_or(0.0, |e| f64::from(e.1)))
        }
        // The actor value code; a recipe with no skill -1 [documented: the record's DATA].
        "getreciperequiredskill" => XVal::Num(form(args, 0).and_then(|r| recipe(g, r)).and_then(|r| r.skill).map_or(-1.0, |s| f64::from(s.0))),
        "getreciperequiredskilllevel" => XVal::Num(form(args, 0).map_or(0.0, |r| f64::from(recipe_level(g, r)))),
        // `recipe code`: a code below 0 takes the skill away and keeps the level [observed:
        // lutana1 rcpe.setskill.neg]; a recipe without one gains it at its level.
        "setreciperequiredskill" => {
            let code = num(args, 1).filter(|c| c.is_finite()).map(f64::trunc);
            if let (Some(code), Some(r)) = (code, form(args, 0)) {
                let level = recipe_level(g, r);
                if let Some(d) = recipe_mut(g, r) {
                    d.skill = (code >= 0.0).then(|| (code.min(f64::from(u16::MAX)) as u16, level));
                    g.other.lutana.recipe_levels.insert(r, level);
                }
            }
            zero
        }
        // `recipe level`: a recipe without a skill keeps the level too [observed: lutana1
        // rcpe.setlevel.noskill].
        "setreciperequiredskilllevel" => {
            let level = num(args, 1).and_then(|v| field_number(v.trunc(), U32.0, U32.1));
            if let (Some(level), Some(r)) = (level, form(args, 0)) {
                if let Some(d) = recipe_mut(g, r) {
                    if let Some(s) = d.skill.as_mut() {
                        s.1 = level as u32;
                    }
                    g.other.lutana.recipe_levels.insert(r, level as u32);
                }
            }
            zero
        }
        // `recipe item count`: a new entry first, also for an item already listed, a count
        // of 0 or a negative count too [observed: lutana1 rcpe.addinput, rcpe.addinput.zero,
        // rcpe.addinput.existing; Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30,
        // probe jip_02_recipe_add_neg_count: count -1 adds the entry (1 -> 2)].
        "addrecipeinputform" | "addrecipeoutputform" => {
            let (item, count) = (form(args, 1), num(args, 2).filter(|v| v.is_finite()).map(|v| v.trunc().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32));
            if let (Some(item), Some(count)) = (item, count) {
                if let Some(r) = form(args, 0).and_then(|r| recipe_mut(g, r)) {
                    let entries = if name == "addrecipeinputform" { &mut r.ingredients } else { &mut r.outputs };
                    entries.insert(0, (item, count));
                }
            }
            zero
        }
        // `recipe item`: the item's first entry removed [observed: lutana1 rcpe.addinput].
        "removerecipeinputform" | "removerecipeoutputform" => {
            if let (Some(item), Some(r)) = (form(args, 1), form(args, 0).and_then(|r| recipe_mut(g, r))) {
                let entries = if name == "removerecipeinputform" { &mut r.ingredients } else { &mut r.outputs };
                if let Some(i) = entries.iter().position(|e| e.0 == item) {
                    entries.remove(i);
                }
            }
            zero
        }
        // `recipe old new`: the first entry's item, its count kept [documented: the
        // examples; observed: lutana1 rcpe.replaceinput].
        "replacerecipeinputform" | "replacerecipeoutputform" => {
            if let (Some(old), Some(new), Some(r)) = (form(args, 1), form(args, 2), form(args, 0).and_then(|r| recipe_mut(g, r))) {
                let entries = if name == "replacerecipeinputform" { &mut r.ingredients } else { &mut r.outputs };
                if let Some(e) = entries.iter_mut().find(|e| e.0 == old) {
                    e.0 = new;
                }
            }
            zero
        }
        // `recipe item count`: the first entry's count, 0 or negative stored verbatim;
        // an item not listed is not added [observed: lutana1 rcpe.setincount.0,
        // rcpe.setoutcount.0; Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN
        // 57.30, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base
        // masters, `jip_03_recipe_set_neg_count` (-5 stored verbatim: 1 -> -5) and
        // `w4d_15_recipe_unlisted_item` (`tools/capture/nvse/catalogs/wave4d_extenders.json`,
        // #3679): `in(n:1->1,c:0->0)|out(n:1->1,c:0->0)`; no DLL or source consulted].
        "setrecipeinputcount" | "setrecipeoutputcount" => {
            let (item, count) = (form(args, 1), num(args, 2).filter(|v| v.is_finite()).map(|v| v.trunc().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32));
            if let (Some(item), Some(count)) = (item, count) {
                if let Some(r) = form(args, 0).and_then(|r| recipe_mut(g, r)) {
                    let entries = if name == "setrecipeinputcount" { &mut r.ingredients } else { &mut r.outputs };
                    if let Some(e) = entries.iter_mut().find(|e| e.0 == item) {
                        e.1 = count;
                    }
                }
            }
            zero
        }
        // `item [category]`: the recipes that take the item, those of the category (or
        // subcategory) only when one is given [documented: GetFormRecipes]: the later
        // plugin's first, each plugin's in FormID order [observed: lutana1
        // rcpe.formrecipes.*]. GetFormRecipeOutputs their outputs, each once [documented:
        // GetFormRecipeOutputs], in that order (see docs/spec-notes/ext-lutana.md §3).
        "getformrecipes" | "getformrecipeoutputs" => {
            let (Some(item), cat) = (form(args, 0), form(args, 1)) else { return Some(Ok(list(g, std::iter::empty()))) };
            let mut ids: Vec<FormId> = g.data.recipes.keys().copied().collect();
            ids.sort_by_key(|f| (std::cmp::Reverse(f.0 >> 24), f.0));
            let using: Vec<&RecipeDef> = ids.iter().filter_map(|&id| recipe(g, id)).filter(|r| r.ingredients.iter().any(|e| e.0 == item) && cat.is_none_or(|c| r.listed_under(c))).collect();
            let out: Vec<FormId> = if name == "getformrecipes" {
                using.iter().map(|r| r.form).collect()
            } else {
                let mut seen = std::collections::HashSet::new();
                using.iter().flat_map(|r| r.outputs.iter().map(|e| e.0)).filter(|o| seen.insert(*o)).collect()
            };
            list(g, out.into_iter().map(|f| XVal::Ref(f.0)))
        }
        // ---- actors ----
        // `actor.ActorHasEffect effect`: an active effect of that base effect on the actor
        // [documented: ActorHasEffect], as HasMagicEffect reads it.
        "actorhaseffect" => {
            let (Some(r), Some(m)) = (subject, form(args, 0)) else { return Some(Ok(zero)) };
            g.sync_actor_effects(r);
            yes(g.effects.iter().any(|e| e.target == r && e.effect == m))
        }
        // `npc flag`: the AI data's Buys/Sells and Services flags (1 Weapons … 131072
        // Repair) [documented: GetServiceFlag's table; AIDT services, xEdit]: the NPC
        // record's, a "Use AI Data" template's until SetServiceFlag changes the NPC's
        // [observed: lutana1 actor.service.tender2.4, actor.setservice.tender2]
        // ([`ai_record`]).
        "getserviceflag" => {
            let Some(rec) = form(args, 0).and_then(|f| ai_record(g, f)) else { return Some(Ok(zero)) };
            let flags = services_of(g, rec);
            let bit = num(args, 1).map_or(0, |b| b as i64 as u32);
            yes(bit != 0 && flags & bit == bit)
        }
        // `npc flag on`: changes the NPC record's flags, which the vendor's services read
        // unless "Autocalc Service" takes the class's (`Game::services`).
        "setserviceflag" => {
            let (Some(rec), Some(bit)) = (form(args, 0).and_then(|f| ai_record(g, f)), num(args, 1).map(|b| b as i64 as u32)) else { return Some(Ok(zero)) };
            let now = services_of(g, rec);
            let new = if num(args, 2).unwrap_or(0.0) != 0.0 { now | bit } else { now & !bit };
            g.other.lutana.services.insert(rec, new);
            zero
        }
        // `[actorBase]`: the AI data's aggro radius [documented: LNGetAggroRadius], the
        // calling reference's when no base is given; round-trips on the NPC record
        // [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30,
        // 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters,
        // `w4c_b6_02_service_flag_aggro` (`tools/capture/nvse/catalogs/wave4c_extenders.json`,
        // #3679): `sf(1->1)|aggro(0->555)`; no DLL or source consulted; hypothesis:
        // template isolation matches lutana1 actor.setservice.tender2].
        "lngetaggroradius" => {
            let Some(rec) = form(args, 0).or(subject).and_then(|f| ai_record(g, f)) else { return Some(Ok(zero)) };
            XVal::Num(f64::from(aggro_of(g, rec)))
        }
        // `radius [actorBase]`: read by the aggro checks too (`Game::aggro_radius`),
        // which still need the record's "Aggro Radius Behavior" on.
        "lnsetaggroradius" => {
            let r = num(args, 0).and_then(|v| field_number(v.trunc(), U32.0, U32.1));
            if let (Some(r), Some(rec)) = (r, form(args, 1).or(subject).and_then(|f| ai_record(g, f))) {
                g.other.lutana.aggro_radius.insert(rec, r as u32);
            }
            zero
        }
        // `[actorBase]`: the combat style the actor fights with (`Game::combat_style_id`:
        // the package's, SetCombatStyle's, the ZNAM of the actor supplying its traits)
        // [documented: GetCombatStyle, "the current Combat Style"]; for a base form the
        // ZNAM of its Traits source (#2138); none DefaultCombatstyle [observed: lutana1
        // actor.combatstyle.doc, actor.combatstyle.ref.docref, actor.combatstyle.player].
        "getcombatstyle" => {
            let own = if let Some(b) = form(args, 0) {
                traits_source(g, b).and_then(|s| g.data.actors.get(&s)).and_then(|d| d.combat_style)
            } else {
                subject.and_then(|r| g.combat_style_id(r))
            };
            reff(own.or_else(|| g.data.by_edid("DefaultCombatstyle")))
        }
        // ---- perks ----
        // [documented: IsTrait] PERK DATA Trait, `Game::perk_def` first (`SetPerkFlag`).
        "istrait" => yes(form(args, 0).and_then(|p| g.perk_def(p)).is_some_and(|d| d.is_trait)),
        // `perk index`: an ability entry's spell, a quest stage entry's quest [documented:
        // "Ability for type 5, Quest for type 6"]; an entry point 0 (its leveled list is
        // not kept decoded; see docs/spec-notes/ext-lutana.md §5), a negative index or past the entries 0.
        "getnthperkentryform" => {
            let e = form(args, 0).and_then(|p| num(args, 1).and_then(|i| crate::nvse_lu3::nth_entry_of(g, p, i)));
            reff(match e.map(|e| &e.effect) {
                Some(crate::data::PerkEffect::Ability(s)) => Some(*s),
                Some(crate::data::PerkEffect::QuestStage { quest, .. }) => Some(*quest),
                _ => None,
            })
        }
        // ---- worldspaces, cells, water ----
        // [documented: GetWorldspaceParentWorldspace] WRLD WNAM.
        "getworldspaceparentworldspace" | "getworldparentworld" => reff(form(args, 0).and_then(|w| record_ref(g, w, "WNAM"))),
        "getcellwaterform" => reff(cell_arg(g, args, 0).and_then(|c| cell_water(g, c))),
        // `[cell]`: CELL XCLW (SetCellWaterHeight's first). An unloaded exterior cell
        // returns 0; a loaded exterior cell without XCLW returns `f32::MAX` (3.40282e+38,
        // NOT the worldspace DNAM default water height); an interior cell returns its
        // XCLW (e.g. -2147483648 when marked "no height") or 0 [observed: lutana1
        // cell.waterheight.startcell, cell.waterheight.doccell; Fallout: New Vegas
        // 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24, Goodsprings (`0x000DAEBB`),
        // 10 official base masters, `w4d_21_cell_water_height_ext`
        // (`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3679):
        // `in=-2.14748e+09|gs(000DAEBB)=3.40282e+38,cur=3.40282e+38|golfUnl=0`;
        // no DLL or source consulted].
        "getcellwaterheight" => {
            let Some(c) = cell_arg(g, args, 0) else { return Some(Ok(zero)) };
            if g.world.placed.cell_world.contains_key(&c) {
                let loaded = g.buffered_cells().contains(&c) || g.cell_of(FormId::PLAYER_REF) == Some(c);
                if !loaded {
                    return Some(Ok(zero));
                }
                return Some(Ok(XVal::Num(record_num(g, c, "XCLW", "").unwrap_or(f64::from(f32::MAX)))));
            }
            XVal::Num(record_num(g, c, "XCLW", "").unwrap_or(0.0))
        }
        // `height [cell]`: for the session, read by GetCellWaterHeight; the water drawn
        // keeps the record's height (ext-lutana.md, follow-ups).
        "setcellwaterheight" => {
            let h = num(args, 0).and_then(|h| field_number(h, F32.0, F32.1));
            if let (Some(h), Some(c)) = (h, cell_arg(g, args, 1)) {
                g.field_overrides.insert((c, "XCLW/".into()), XVal::Num(h));
            }
            zero
        }
        // `[cell]`: a string map of "x" and "y", the exterior cell's grid (XCLC)
        // [documented: GetCellCoords]; a cell without a grid (an interior) 0 and 0
        // [observed: lutana1 cell.coords.doccell].
        "getcellcoords" => {
            let c = cell_arg(g, args, 0);
            let xy = c.map(|c| (record_num(g, c, "XCLC", "x").unwrap_or(0.0), record_num(g, c, "XCLC", "y").unwrap_or(0.0)));
            string_map(g, xy.map_or_else(Vec::new, |(x, y)| vec![("x", x), ("y", y)]))
        }
        // `water`: the water type's actor effect (WATR XNAM) [documented:
        // GetWaterFormEffect]; with no water, the player's cell's (see docs/spec-notes/ext-lutana.md §5).
        "getwaterformeffect" => {
            let w = form(args, 0).or_else(|| g.cell_of(FormId::PLAYER_REF).and_then(|c| cell_water(g, c)));
            // SetWaterFormEffect's change first (`crate::nvse_jip8`).
            reff(w.and_then(|w| g.water_effect(w)))
        }
        // `ref.GetZone`: the encounter zone of the reference's parent cell
        // [documented: GetZone], SetCellEncounterZone's and SetWorldspaceEncounterZone's
        // changes first (`nvse_jg5::cell_zone`).
        "getzone" => {
            let c = subject.and_then(|r| g.cell_of(r));
            reff(c.and_then(|c| crate::nvse_jg5::cell_zone(g, c)))
        }
        // `zone flag`: 1 Never Resets, 2 Match PC Below Minimum Level [documented: the
        // pages' table]; 1 when any bit of `flag` is set [observed: lutana1
        // zone.flag.vault11zone.3].
        "getzoneflag" => {
            let (Some(d), Some(bit)) = (form(args, 0).and_then(|z| g.zone(z)), num(args, 1).map(|b| b as i64 as u8)) else { return Some(Ok(zero)) };
            let flags = u8::from(d.never_resets) | u8::from(d.match_pc_below_min) << 1;
            yes(flags & bit != 0)
        }
        // `zone flag on`: read wherever the zone's flags are read through `Game::zone`.
        "setzoneflag" => {
            if let (Some(z), Some(bit)) = (form(args, 0).filter(|z| g.world.forms.zones.contains_key(z)), num(args, 1).map(|b| b as i64 as u8)) {
                let on = num(args, 2).unwrap_or(0.0) != 0.0;
                if bit & 1 != 0 {
                    g.other.zone_changes.entry(z).or_default().never_resets = Some(on);
                }
                if bit & 2 != 0 {
                    g.world.set_zone_match_level(z, on);
                }
            }
            zero
        }
        // `list`: LVLG, the global giving the chance none [documented:
        // GetChanceNoneGlobal], SetChanceNoneGlobal's first (`nvse_leveled`).
        "getchancenoneglobal" => reff(form(args, 0).and_then(|l| g.lev_list(l)).and_then(|l| l.global)),
        // ---- activators (ACTI) [documented: each page; the fields: xEdit ACTI] ----
        "getactivatorradiostation" => reff(form(args, 0).or(subject).map(|f| base(g, f)).and_then(|a| record_ref(g, a, "RNAM"))),
        // `form [activator]`: RNAM, VNAM, SNAM for the session, read by their getters
        // (GetActivatorSoundActivate/Looping, `nvse_plugins`).
        "setactivatorradiostation" | "setactivatorsoundlooping" => {
            let sub = if name == "setactivatorradiostation" { "RNAM/" } else { "SNAM/" };
            let a = form(args, 1).or(subject).map(|f| base(g, f)).filter(|&a| g.data.sig(a) == Some(*b"ACTI"));
            if let Some(a) = a {
                g.field_overrides.insert((a, sub.into()), XVal::Ref(form(args, 0).map_or(0, |f| f.0)));
            }
            zero
        }
        "setactivatorsoundactivate" => {
            let a = form(args, 1).or(subject).map(|f| base(g, f)).filter(|&a| g.data.sig(g.unclone(a)) == Some(*b"ACTI"));
            let s = form(args, 0).filter(|&f| g.data.sig(g.unclone(f)) == Some(*b"SOUN"));
            if let (Some(a), Some(s)) = (a, s) {
                g.field_overrides.insert((a, "VNAM/".into()), XVal::Ref(s.0));
            }
            zero
        }
        // ---- terminals and locks ----
        // `[terminal]`: the base's DNAM difficulty, the number GetLockLevel gives a
        // terminal (0 Very Easy … 4 Very Hard, 5 Requires Key) [documented:
        // GetTerminalLock, "regardless of locked state"; `terminal::lock_level`]; a
        // terminal the record marks Unlocked -1 [observed: lutana1 term.lock.manny].
        "getterminallock" => {
            let t = form(args, 0).or(subject).map(|f| base(g, f));
            let unlocked = crate::data::terminal_flag::UNLOCKED;
            XVal::Num(t.and_then(|t| g.data.terminals.get(&t)).map_or(0.0, |d| if d.has(unlocked) { -1.0 } else { f64::from(d.difficulty) }))
        }
        // `[terminal]`: TERM PNAM [documented: GetPasswordNote], SetPasswordNote's first
        // (`crate::terminal::password_note`, shared with `terminal::has_password` and
        // `PlayerHasKey` and carried across same-run loads via `Game::Session`).
        "getpasswordnote" => reff(form(args, 0).or(subject).map(|f| base(g, f)).and_then(|t| crate::terminal::password_note(g, t))),
        "setpasswordnote" => {
            if let Some(t) = form(args, 1).or(subject).map(|f| base(g, f)) {
                crate::terminal::set_password_note(g, t, form(args, 0));
            }
            zero
        }
        // `[ref]`: 0 not broken, 1 broken (a forced lock, a terminal locked out), 2
        // broken again on Infiltrator's or Computer Whiz's try, 3 an inaccessible door
        // [documented: GetLockedOut's table; `lockpick::Locks`, `terminal::Terminals`].
        "getlockedout" => {
            let Some(r) = lockable(args, 0, subject) else { return Some(Ok(zero)) };
            let (broken, again) = if is_terminal(g, r) {
                (g.terminals.access.get(&r) == Some(&crate::terminal::Access::LockedOut), g.terminals.retried.contains(&r))
            } else {
                let r = g.world.lock_source(r);
                (g.locks.broken.contains(&r), g.locks.retried.contains(&r))
            };
            let door = g.data.sig(base(g, r)) == Some(*b"DOOR");
            XVal::Num(if again {
                2.0
            } else if broken {
                1.0
            } else if door && crate::nvse_other::record_flags_of(g, r) & fnv_sim::refs::INACCESSIBLE != 0 {
                3.0
            } else {
                0.0
            })
        }
        // `state [ref]`: 0 mends the lock, 1 breaks it, 2 breaks it with the perk's try
        // spent [documented: SetLockedOut's table]; another state changes nothing
        // [hypothesis].
        "setlockedout" => {
            let (Some(state @ 0..=2), Some(r)) = (num(args, 0).map(|s| s as i64), lockable(args, 1, subject)) else { return Some(Ok(zero)) };
            if is_terminal(g, r) {
                let t = &mut g.terminals;
                match state {
                    0 => {
                        if t.access.get(&r) == Some(&crate::terminal::Access::LockedOut) {
                            t.access.remove(&r);
                        }
                        t.retried.remove(&r);
                    }
                    _ => {
                        t.access.insert(r, crate::terminal::Access::LockedOut);
                        if state == 2 {
                            t.retried.insert(r);
                        } else {
                            t.retried.remove(&r);
                        }
                    }
                }
            } else {
                let r = g.world.lock_source(r);
                let l = &mut g.locks;
                if state == 0 {
                    l.broken.remove(&r);
                } else {
                    l.broken.insert(r);
                }
                if state == 2 {
                    l.retried.insert(r);
                } else {
                    l.retried.remove(&r);
                }
            }
            zero
        }
        // `ref.PlayerHasKey`: a door's or container's lock key carried, or a placed
        // terminal reference's password note filed/carried [documented: PlayerHasKey;
        // observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-23/24,
        // `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters,
        // `b3_lut_06_player_has_key_terminal_note` (`tools/capture/nvse/catalogs/wave4b_extenders.json`, #3679):
        // `PROBE|b3_lut_06_player_has_key_terminal_note|note=00133107,before=0,afterAddNote=0`
        // (base `TERM` form returns `0`) and `w4d_25_lsct_rgb_nan_and_password_key`
        // (`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3679 / #3803):
        // `PROBE|w4d_25_lsct_rgb_nan_and_password_key|lsct(0=255.0,180.0,53.0|nan=-nan(ind),123.5,45.0)|door(0->1)|termRef(0->1)`
        // (placed `TERM` reference returns `1` once its password note is held); no DLL
        // or source consulted]; a load door with no lock of its own reads the other side's
        // [hypothesis: as GetLockLevel].
        "playerhaskey" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            if (g.world.placed.get(r).is_some() || g.world.peek(r).is_some()) && is_terminal(g, r) {
                return Some(Ok(yes(crate::terminal::has_password(g, base(g, r)))));
            }
            let key = g.world.lock_of(r).and_then(|l| l.key);
            yes(key.is_some_and(|k| g.world.item_count(FormId::PLAYER_REF, k).is_ok_and(|n| n > 0)))
        }
        // `[ref]`: XAPD "Parent Activate Only" [documented: IsParentActivateOnly; the
        // flag: `PlacedRef::parent_activate_only`].
        "isparentactivateonly" => yes(form(args, 0).or(subject).and_then(|r| g.world.placed.get(r)).is_some_and(|p| p.parent_activate_only)),
        // ---- form flags and playability ----
        // `form flag`: 1 when any bit of `flag` is among the form's record flags
        // [documented: GetFormFlag's table; observed: `w6d2_32_lutana_form_flag_service_and_casino`],
        // as GetFlagsLow/High read them.
        "getformflag" => {
            let (Some(f), Some(bit)) = (form(args, 0), num(args, 1).map(|b| b as i64 as u32)) else { return Some(Ok(zero)) };
            yes(bit != 0 && crate::nvse_other::record_flags_of(g, f) & bit != 0)
        }
        // `form flag on`: as SetFlagsLow/High store them, read by the same getters.
        "setformflag" => {
            if let (Some(f), Some(bit)) = (form(args, 0), num(args, 1).map(|b| b as i64 as u32)) {
                let now = crate::nvse_other::record_flags_of(g, f);
                let new = if num(args, 2).unwrap_or(0.0) != 0.0 { now | bit } else { now & !bit };
                g.other.flags.insert(f, new);
            }
            zero
        }
        // `[form]`: weapons, armour and ammo as IsPlayable; races (DATA flags bit 0),
        // hair and eyes (DATA bit 0) and perks (DATA Playable) [documented:
        // LNIsPlayable's form types; the fields: xEdit]; the calling reference's base
        // when no form is given.
        "lnisplayable" => {
            let Some(f) = form(args, 0).or(subject).map(|f| base(g, f)) else { return Some(Ok(zero)) };
            yes(playable(g, f).unwrap_or(false))
        }
        "lnsetisplayable" => {
            let on = num(args, 0).unwrap_or(0.0) != 0.0;
            if let Some(f) = form(args, 1).or(subject).map(|f| base(g, f)) {
                set_playable(g, f, on);
            }
            zero
        }
        // ---- weapons (WEAP) [documented: each page; the fields: xEdit WEAP] ----
        // `weapon value`: CRDT flags, DNAM kill impulse and impulse distance, VATS effect,
        // for the session, read through the record getters (`nvse::field`).
        "setweaponcritflags" | "setweaponkillimpulse" | "setweaponimpulsedistance" => {
            let w = form(args, 0).filter(|&w| g.data.sig(w) == Some(*b"WEAP"));
            let (key, range) = match name {
                "setweaponcritflags" => ("CRDT/flags", U8),
                "setweaponkillimpulse" => ("DNAM/kill_impulse", F32),
                _ => ("DNAM/impulse_dist", F32),
            };
            // The crit flags byte keeps the low 8 bits [observed: lutana1
            // weap.setcritflags.300 reads 44].
            let v = num(args, 1).filter(|v| v.is_finite()).map(|v| if range == U8 { f64::from(v.trunc() as i64 as u8) } else { v }).and_then(|v| field_number(v, range.0, range.1));
            if let (Some(w), Some(v)) = (w, v) {
                g.field_overrides.insert((w, key.into()), XVal::Num(v));
            }
            zero
        }
        "setweaponvatseffect" => {
            if let Some(w) = form(args, 0).filter(|&w| g.data.sig(w) == Some(*b"WEAP")) {
                g.field_overrides.insert((w, "VATS/effect".into()), XVal::Ref(form(args, 1).map_or(0, |f| f.0)));
            }
            zero
        }
        // ---- load screens (LSCR) ----
        // `screen`: WMI1, the load screen type [documented: GetLoadScreenType;
        // SetLoadScreenType, for the session].
        "getloadscreentype" => reff(form(args, 0).and_then(|s| record_ref(g, s, "WMI1"))),
        "setloadscreentype" => {
            if let (Some(s), Some(t)) = (form(args, 0).filter(|&s| g.data.sig(s) == Some(*b"LSCR")), form(args, 1).filter(|&t| g.data.sig(t) == Some(*b"LSCT"))) {
                g.field_overrides.insert((s, "WMI1/".into()), XVal::Ref(t.0));
            }
            zero
        }
        // ---- misc ----
        // [documented: GetPCCanUsePowerArmor] SetPCCanUsePowerArmor's state.
        "getpccanusepowerarmor" => yes(g.player.power_armor),
        // [documented: GetMessageDisplayTime, alias GetMDT; SetDefaultMessageTime, alias
        // SetDMT]: seconds, 0 and below too [observed: lutana1 misc.setdmt]. The installed
        // JIP LN registers neither GetMessageDisplayTime nor GetMDT, which read as text in
        // a script [observed: lutana1 misc.messagedisplaytime, misc.mdt; fnv-script
        // encodings.json `not_commands`]; kept for a caller by name.
        "getmessagedisplaytime" | "getmdt" => XVal::Num(message_time(g)),
        "setdefaultmessagetime" | "setdmt" => {
            if let Some(t) = num(args, 0).filter(|t| t.is_finite()) {
                g.other.lutana.message_time = Some(t);
            }
            zero
        }
        // `note`: AddNote without the corner message [documented: AddNoteNS].
        "addnotens" => {
            if let Some(n) = form(args, 0) {
                if g.player.notes.insert(n) {
                    g.jip_event("noteadded", None, vec![XVal::Ref(n.0)]);
                }
            }
            zero
        }
        // The computer's local time as "HH-mm-SS" [documented: GetTimeStamp].
        "gettimestamp" => XVal::Str(jiff::Timestamp::now().to_zoned_datetime(crate::local_time::system_zone()).strftime("%H-%M-%S").to_string()),
        // `list`: 1 when the reference under the crosshair is in the list, 2 when its base
        // is, else 0 [documented: CrosshairRefInList].
        "crosshairrefinlist" => {
            let (Some(r), Some(l)) = (g.senses.crosshair_ref, form(args, 0)) else { return Some(Ok(zero)) };
            let members = crate::functions::refs::list_members(g, l);
            XVal::Num(if members.contains(&r) {
                1.0
            } else if members.contains(&base(g, r)) {
                2.0
            } else {
                0.0
            })
        }
        // `[light]`: a light base's radius (LIGH DATA); else the calling reference's XRDS
        // (SetRadius's first), else 0 — does NOT fall back to a placed Light ref's base
        // LIGH radius [documented: GetRadius, "a reference (any type) or Light BaseForm";
        // observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, probe
        // jip_06_radius_ref_fallback_base: base LIGH 200.000, placed ref without XRDS
        // 0.000].
        "getradius" => {
            if let Some(l) = form(args, 0) {
                XVal::Num(record_num(g, l, "DATA", "radius").unwrap_or(0.0))
            } else if let Some(r) = subject {
                XVal::Num(g.ref_radius(r).map_or(0.0, |x| crate::nvse::record_float(f64::from(x))))
            } else {
                zero
            }
        }
        // `radius [light]`: the light base's radius (an integer field, so whole), else the
        // calling reference's own [documented: SetRadius]; a negative radius as given
        // [observed: lutana1 radius.set.base.frac, radius.set.base.neg, radius.set.ref].
        "setradius" => {
            let Some(v) = num(args, 0).filter(|v| v.is_finite()) else { return Some(Ok(zero)) };
            if let Some(l) = form(args, 1).filter(|&l| g.data.sig(l) == Some(*b"LIGH")) {
                g.field_overrides.insert((l, "DATA/radius".into()), XVal::Num(signed(int_bits(v))));
            } else if let Some(r) = subject.filter(|_| form(args, 1).is_none()) {
                g.other.lutana.ref_radius.insert(r, v.clamp(F32.0, F32.1) as f32);
            }
            zero
        }
        _ => return None,
    };
    Some(Ok(v))
}

/// Whether a form is playable, for the form types LNIsPlayable names; `None` for another
/// type.
fn playable(g: &Game, f: FormId) -> Option<bool> {
    Some(match &g.data.sig(g.unclone(f))? {
        b"WEAP" | b"ARMO" | b"AMMO" => g.is_item_playable(f),
        b"RACE" => record_num(g, f, "DATA", "flags").is_some_and(|x| x as u64 & 1 != 0),
        b"HAIR" | b"EYES" => record_num(g, f, "DATA", "").is_some_and(|x| x as u64 & 1 != 0),
        b"PERK" => g.perk_def(f).is_some_and(|p| p.playable),
        _ => return None,
    })
}

/// LNSetIsPlayable: each type where its getter reads it.
fn set_playable(g: &mut Game, f: FormId, on: bool) {
    let bit = |x: Option<f64>| {
        let now = x.map_or(0, |x| x as u64);
        (if on { now | 1 } else { now & !1 }) as f64
    };
    match g.data.sig(g.unclone(f)).as_ref() {
        Some(b"WEAP") => {
            let now = record_num(g, f, "DNAM", "flags1").map_or(0, |x| x as u64);
            let new = if on { now & !(1 << 7) } else { now | (1 << 7) };
            g.field_overrides.insert((f, "DNAM/flags1".into()), XVal::Num(new as f64));
            g.playable_overrides.remove(&f);
            g.world.mutate_item_override(f, |ov| {
                ov.non_playable = Some(!on);
            });
        }
        Some(b"ARMO" | b"AMMO") => {
            g.set_item_playable(f, on);
        }
        Some(b"RACE") => {
            let v = bit(record_num(g, f, "DATA", "flags"));
            g.field_overrides.insert((f, "DATA/flags".into()), XVal::Num(v));
        }
        Some(b"HAIR" | b"EYES") => {
            let v = bit(record_num(g, f, "DATA", ""));
            g.field_overrides.insert((f, "DATA/".into()), XVal::Num(v));
        }
        Some(b"PERK") => {
            if let Some(p) = g.perk_def_mut(f) {
                p.playable = on;
            }
        }
        _ => {}
    }
}
