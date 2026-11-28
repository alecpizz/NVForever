//! JohnnyGuitar NVSE, batch 6 (#1007): actor values by code, actors' states and
//! temporary effects, perks, karma, cells (visited, expired, lighting template), the moon,
//! radios and talking activators, references' prompts and primitives, effect shaders,
//! mines, and the per-reference calls CallPerRef and CallPerMobileObject [documented: each
//! page in the wiki export; docs/spec-notes/ext-jg6.md].

use crate::functions::{Args, Ctx};
use crate::game::{Game, RunCtx};
use fnv_script::vm::{ArgValue, ArrayKind, Value, XKey, XVal};
use fnv_sim::FormId;
use std::collections::{HashMap, HashSet};

/// The session's changes of this batch.
#[derive(Debug, Default)]
pub struct Jg6State {
    /// An actor value's base before the first SetActorValue on it, for UnsetAV.
    pub set_av_before: HashMap<(FormId, u16), f64>,
    /// SetLightingTemplateCell's cells, by lighting template.
    pub template_cells: HashMap<FormId, FormId>,
    /// SetRefActivationPromptOverride's prompts, by reference (read by
    /// [`activation_prompt`]).
    pub prompts: HashMap<FormId, String>,
    /// References whose primitive RemovePrimitive took away.
    pub removed_primitives: HashSet<FormId>,
    /// Every live mine not going off, the player's included (the engine's mines,
    /// `nvf-play` mines.rs); GetMineArmed's set leaves the player's out.
    pub armed_mines_all: HashSet<FormId>,
    /// Last exterior worldspace the player entered, retained indoors so `GetMoonPhase`
    /// keeps using that worldspace's active climate (`Game::player_entered`).
    pub last_exterior_world: Option<FormId>,
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

fn text(args: &[XVal], k: usize) -> Option<String> {
    match args.get(k)? {
        XVal::Str(s) => Some(s.clone()),
        _ => None,
    }
}

fn yes(b: bool) -> XVal {
    XVal::Num(f64::from(u8::from(b)))
}

fn list(g: &mut Game, items: Vec<XVal>) -> XVal {
    let entries = items.into_iter().enumerate().map(|(i, v)| (XKey::Num(i as f64), v)).collect();
    XVal::Array(g.ext.new_array(ArrayKind::List, entries))
}

fn base(g: &Game, f: FormId) -> FormId {
    crate::functions::base_of(g, f).unwrap_or(f)
}

/// An actor value given as a code or a name.
fn av(g: &Game, args: &[XVal], k: usize) -> Option<u16> {
    match args.get(k)? {
        XVal::Num(c) if *c >= 0.0 && c.fract() == 0.0 => u16::try_from(*c as i64).ok(),
        XVal::Str(n) => crate::functions::av_code(g, n).ok(),
        _ => None,
    }
}

/// A call context for one of the game's own functions on `on`.
fn vanilla_ctx(on: Option<FormId>) -> Ctx {
    Ctx { unit: "nvse_jg6".into(), opcode: 0, on, self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false }
}

/// Run the game's own actor value function `f` on `on` with the code and the value, as the
/// non-Alt function would be called.
fn vanilla_av(g: &mut Game, f: fn(&mut Game, &Ctx, &Args) -> Result<Value, String>, on: Option<FormId>, code: u16, v: f64) -> Result<(), String> {
    let a = [ArgValue::Enum(code), ArgValue::Value(Value::Num(v))];
    f(g, &vanilla_ctx(on), &Args(&a)).map(|_| ())
}

/// The base of the actor value before `SetActorValue` changes it, kept the first time for
/// UnsetAV (called by `functions::actor::set_actor_value_watched`).
pub(crate) fn note_set_av(g: &mut Game, ctx: &Ctx, a: &Args) {
    let (Ok(r), Ok(code)) = (ctx.subject(), a.code(0)) else { return };
    if g.other.jg6.set_av_before.contains_key(&(r, code)) {
        return;
    }
    let one = [ArgValue::Enum(code)];
    if let Ok(Value::Num(b)) = crate::functions::actor::get_base_actor_value(g, &vanilla_ctx(Some(r)), &Args(&one)) {
        g.other.jg6.set_av_before.insert((r, code), b);
    }
}

/// A reference's activation prompt: SetRefActivationPromptOverride's, else its own XATO.
pub fn activation_prompt(g: &Game, r: FormId) -> Option<String> {
    g.other.jg6.prompts.get(&r).cloned().or_else(|| g.world.placed.get(r).and_then(|p| p.prompt.clone()))
}

/// The temporary effects GetTempEffects lists for `a`, in its order (indices into
/// `g.effects`).
fn temp_effects(g: &Game, a: FormId) -> Vec<usize> {
    g.effects.iter().enumerate().filter(|(_, e)| e.target == a && !e.permanent && e.duration > 0.0).map(|(i, _)| i).collect()
}

/// The JohnnyGuitar batch-6 functions; `None` for any other name.
pub(crate) fn call(g: &mut Game, ctx: &RunCtx, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let zero = XVal::Num(0.0);
    let v = match name {
        // ---- actor values by code ----
        // `code value`: the game's own DamageAV, ForceAV, ModAV, RestoreAV and SetAV with
        // a numeric actor value code [documented: the *ActorValueAlt pages, each "the same
        // as" its function with a code instead of a stat name]. Their handlers see the
        // change as the plain function's do.
        "damageactorvaluealt" | "damageavalt" | "forceactorvaluealt" | "forceavalt" | "modactorvaluealt" | "modavalt" | "restoreactorvaluealt" | "restoreavalt" | "setactorvaluealt" | "setavalt" => {
            use crate::functions::actor as act;
            let f: fn(&mut Game, &Ctx, &Args) -> Result<Value, String> = match name {
                "damageactorvaluealt" | "damageavalt" => act::damage_actor_value_watched,
                "forceactorvaluealt" | "forceavalt" => act::force_actor_value_watched,
                "modactorvaluealt" | "modavalt" => act::mod_actor_value_watched,
                "restoreactorvaluealt" | "restoreavalt" => act::restore_actor_value_watched,
                _ => act::set_actor_value_watched,
            };
            let (Some(code), Some(XVal::Num(v))) = (av(g, args, 0).filter(|&c| (c as usize) < act::AV_NAMES.len()), args.get(1)) else { return Some(Ok(zero)) };
            if let Err(e) = vanilla_av(g, f, subject, code, *v) {
                return Some(Err(e));
            }
            zero
        }
        // `av`: does nothing [documented: UnforceAV, "currently broken and does
        // nothing"].
        "unforceav" => zero,
        // `av`: the base goes back to what it was before the first SetAV on it
        // [documented: UnsetAV]; the modifiers and the damage stay, and nothing happens
        // without a SetAV [hypothesis: "its previous state" read as the base SetAV
        // changed].
        "unsetav" => {
            let (Some(r), Some(code)) = (subject, av(g, args, 0)) else { return Some(Ok(zero)) };
            if let Some(before) = g.other.jg6.set_av_before.remove(&(r, code)) {
                let one = [ArgValue::Enum(code)];
                if let Ok(Value::Num(now)) = crate::functions::actor::get_base_actor_value(g, &vanilla_ctx(Some(r)), &Args(&one)) {
                    let _ = crate::functions::actor::shift_base(g, r, code, before - now);
                }
            }
            zero
        }
        // `av [filter]`: the modifier the actor's active effects hold, 1 the permanent
        // one, 2 the temporary one, 3 or none both [documented: GetActorValueModifierAlt,
        // "gained from Effects"]. ModAV's change is not in it [observed 2026-11-08: jg6.av.mod,
        // ModActorValueAlt Luck 1 on the player read 0 with every filter]. Another filter
        // 0 [observed: filter 4 read 0].
        "getactorvaluemodifieralt" | "getavmodalt" => {
            let a = subject.unwrap_or(FormId::PLAYER_REF);
            let Some(code) = av(g, args, 0) else { return Some(Ok(zero)) };
            let (mut perm, mut temp) = (0.0, 0.0);
            for e in g.effects.iter().filter(|e| e.target == a && !e.inactive) {
                let Some(c) = e.change.as_ref() else { continue };
                if c.code != code || c.kind != crate::aid::ChangeKind::Modifier {
                    continue;
                }
                if c.permanent_pool {
                    perm += c.applied;
                } else {
                    temp += c.applied;
                }
            }
            XVal::Num(match args.get(1) {
                Some(XVal::Num(t)) if *t == 1.0 => perm,
                Some(XVal::Num(t)) if *t == 2.0 => temp,
                Some(XVal::Num(t)) if *t != 3.0 => 0.0,
                _ => perm + temp,
            })
        }
        // `value`: RewardKarma without its corner message and sound [documented:
        // RewardKarmaAlt]: the karma changes, the note the engine shows is dropped.
        "rewardkarmaalt" => {
            let n = g.crimes.notes.len();
            g.change_karma(num(args, 0, 0.0), "a script (RewardKarmaAlt)");
            g.crimes.notes.truncate(n);
            zero
        }
        // ---- actors ----
        // 0 alive, 1 dying, 2 dead, 3 unconscious, 5 restrained, 6 unconscious essential
        // [documented: GetLifeState]. An essential actor knocked down at no health reads
        // 6, any other knock-down or SetUnconscious 3 [hypothesis]; NVForever's actors die
        // at once, so 1 is never read [hypothesis: no dying timer, fDyingTimer unread].
        "getlifestate" => {
            let Some(a) = subject else { return Some(Ok(zero)) };
            let e = g.extra.get(&a);
            XVal::Num(if g.is_dead(a) {
                2.0
            } else if e.and_then(|e| e.knocked_out.as_ref()).is_some_and(|k| matches!(k.cause, crate::unconscious::Cause::Essential)) {
                6.0
            } else if e.is_some_and(|e| e.unconscious || e.knocked_out.is_some()) {
                3.0
            } else if e.is_some_and(|e| e.restrained) {
                5.0
            } else {
                0.0
            })
        }
        // In a conversation with the player [documented: IsInDialogueWithPlayer]: the
        // conversation's speaker.
        "isindialoguewithplayer" => yes(subject.is_some_and(|r| g.dialogue.as_ref().is_some_and(|c| c.actor == r))),
        // The furniture reference the actor uses [documented: GetCurrentFurnitureRef]; 0
        // for none [hypothesis].
        "getcurrentfurnitureref" => XVal::Ref(subject.and_then(|a| g.furniture_use(a)).map_or(0, |u| u.reference.0)),
        // An effect on the actor lowering its Health [documented: HasHealthDamageEffect]:
        // a value change of Health with a negative amount, active [hypothesis: what counts
        // as health-damaging].
        "hashealthdamageeffect" => {
            let Some(a) = subject else { return Some(Ok(zero)) };
            yes(g.effects.iter().any(|e| e.target == a && !e.inactive && e.change.as_ref().is_some_and(|c| c.code == crate::actor_values::HEALTH && c.amount < 0.0)))
        }
        // The player's ingestibles with effects running: [item, seconds left, duration]
        // each, the effect with the longest duration standing for the item [documented:
        // GetTempIngestibleEffects]; in the order of their first effect [hypothesis].
        "gettempingestibleeffects" => {
            let mut rows: Vec<(FormId, f64, f64)> = Vec::new();
            for i in temp_effects(g, FormId::PLAYER_REF) {
                let e = &g.effects[i];
                if g.data.sig(e.source) != Some(*b"ALCH") {
                    continue;
                }
                let left = (e.duration - e.elapsed).max(0.0);
                match rows.iter_mut().find(|r| r.0 == e.source) {
                    Some(r) if e.duration > r.2 => (r.1, r.2) = (left, e.duration),
                    Some(_) => {}
                    None => rows.push((e.source, left, e.duration)),
                }
            }
            let inner: Vec<XVal> = rows.into_iter().map(|(s, l, d)| list(g, vec![XVal::Ref(s.0), XVal::Num(l), XVal::Num(d)])).collect();
            list(g, inner)
        }
        // `index modifier`: the Nth entry of GetTempEffects' list gets `modifier` more
        // seconds left: its elapsed time goes down by it [documented:
        // ModNthTempEffectTimeLeft, "internally … modifies time elapsed"]; 1 when there is
        // such an entry.
        "modnthtempeffecttimeleft" => {
            let Some(a) = subject else { return Some(Ok(zero)) };
            let k = num(args, 0, -1.0);
            let list = temp_effects(g, a);
            match (k >= 0.0 && k.fract() == 0.0).then(|| list.get(k as usize)).flatten() {
                Some(&i) => {
                    g.effects[i].elapsed -= num(args, 1, 0.0);
                    XVal::Num(1.0)
                }
                None => zero,
            }
        }
        // The perks the level-up menu would offer the player now [documented:
        // GetAvailablePerks]: the perks page's bright ones (`levelup::PerkSheet`), in its
        // order.
        "getavailableperks" => {
            let level = g.player_level() as i32;
            let sheet = crate::levelup::PerkSheet::of_player(g, level);
            let perks: Vec<XVal> = sheet.perks.iter().zip(&sheet.available).filter(|(_, &a)| a).map(|(p, _)| XVal::Ref(p.0)).collect();
            list(g, perks)
        }
        // The hotkey (1-8) the inventory reference's item is on, 0 for none [documented:
        // GetHotkeySlot] (the hotkeys of `nvse_other`).
        "gethotkeyslot" => {
            let item = subject.or(form(args, 0)).map(|r| base(g, r));
            XVal::Num(item.and_then(|i| g.other.hotkeys.iter().position(|h| *h == Some(i))).map_or(0.0, |k| (k + 1) as f64))
        }
        // SendTrespassAlarm on the owner of the calling reference, any reference: the
        // owner's people who see the player turn on it [documented:
        // SendTrespassAlarmAlt]; nothing for a reference no one else owns.
        "sendtrespassalarmalt" => {
            if let Some(r) = subject {
                g.trespass_alarm_for(r);
            }
            zero
        }
        // ---- cells ----
        // `cell`: whether the player's loaded area has ever held it [documented:
        // IsCellVisited; hypothesis: every cell loaded around the player counts, kept in
        // `FormChanges::visited_cells`].
        "iscellvisited" => yes(form(args, 0).is_some_and(|c| g.forms.visited_cells.contains(&c))),
        // `cell`: -1 not visited or the player is in it, 1 expired (ResetInterior, or left
        // for iHoursToRespawnCell game hours and not in a zone that never resets), 0
        // otherwise [documented: IsCellExpired; the timing: `crate::respawn`].
        "iscellexpired" => match form(args, 0).and_then(|c| g.cell_expired(c)) {
            None => XVal::Num(-1.0),
            Some(e) => yes(e),
        },
        // `template [cell]`: the cell SetLightingTemplateCell gave the lighting template,
        // 0 before [documented: Get/SetLightingTemplateCell; hypothesis: a lighting
        // template record names no cell (LGTM holds DATA and DALC only), so the link is
        // the session's]; no cell clears it [hypothesis]. A form that is not an LGTM or not
        // a CELL changes nothing.
        "getlightingtemplatecell" => XVal::Ref(form(args, 0).and_then(|t| g.other.jg6.template_cells.get(&t)).map_or(0, |c| c.0)),
        "setlightingtemplatecell" => {
            let t = form(args, 0).filter(|&t| g.data.sig(t) == Some(*b"LGTM"));
            let is_cell = |c: FormId| g.data.sig(c) == Some(*b"CELL") || g.data.cells.contains_key(&c);
            match (t, form(args, 1)) {
                (Some(t), Some(c)) if is_cell(c) => {
                    g.other.jg6.template_cells.insert(t, c);
                }
                (Some(t), None) => {
                    g.other.jg6.template_cells.remove(&t);
                }
                _ => {}
            }
            zero
        }
        // 0 full … 4 new … 7 waxing gibbous [documented: GetMoonPhase]: the days passed
        // over the climate's phase length (TNAM, low six bits), as the sky draws it
        // (`fnv_world::sky::moon_phase`); indoors in `GSDocMitchellHouse` (`0x00103DF9`),
        // `GetMoonPhase` returns `1` alongside `GameDaysPassed = 5.5011` and
        // `GetCurrentClimate = 0000015F` (`DefaultClimate`, phase length 3 ->
        // `floor(5.5011 / 3.0) % 8 = 1`) [observed: 2026-11-24, `GSDocMitchellHouse`
        // (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528
        // with the 10 official base masters, read-only `PrintToConsole` probes
        // `w4c_b3_03_jg6_actor_world_misc` (`tools/capture/nvse/catalogs/wave4c_extenders.json`,
        // #3678 / PR #3679): `furnP=00000000,furnD=001059B0,hdP=0,moon=1,ta=00000000,tresp=0`,
        // and `w4h_07_moon_phase_days_climate` (`tools/capture/nvse/catalogs/wave4h_reconcile.json`,
        // #3803): `PROBE|w4h_07_moon_phase_days_climate|moon=1|days=5.5011|clim=0000015F|date=2281-9-18|hour=12.026`;
        // no DLL or source consulted]. NVForever resolves via `moon_climate` (current exterior
        // climate, else `ForceClimate`, `SetCurrentClimate`, last exterior worldspace
        // climate, or `DefaultClimate` `0x0000015F` / `NVDefaultClimate`; phase 0 when no
        // climate is loaded) [design].
        "getmoonphase" => {
            let climate = moon_climate(g).map(|c| g.unclone(c));
            let len = match climate.and_then(|c| crate::nvse::record_value(g, c, "TNAM", "moons_phase_length")) {
                Some(XVal::Num(x)) => (x as u64 & 0x3F) as u8,
                _ => 0,
            };
            let days = g.world.calendar().map_or(0.0, |k| k.days_passed);
            XVal::Num(fnv_world::sky::moon_phase(days, len) as f64)
        }
        // Seconds since the last frame [documented: GameGetSecondsPassed]: the frame step.
        "gamegetsecondspassed" | "ggetsecpass" => XVal::Num(g.cfg.frame_seconds),
        // `interiors`: no array, for the interiors and the exteriors alike [observed
        // 2026-11-08: jg6.cell.buffered, ar_Size -1 for both with the player outdoors at
        // the airport, JohnnyGuitar 5.28]. The page calls it deprecated for JIP's
        // GetBufferedCells [documented: GetBufferedCellsAlt].
        "getbufferedcellsalt" => XVal::Array(0),
        // ---- radios and talking activators ----
        // The placed references of every radio station (a TACT with the Radio Station
        // header flag, docs/spec-notes/radio.md §2) [documented: GetAllGameRadios], by
        // FormID.
        "getallgameradios" => {
            let mut refs: Vec<FormId> = g
                .world
                .placed
                .refs
                .iter()
                .filter(|(_, p)| g.data.station_flags.get(&p.base).is_some_and(|f| f & crate::radio::RADIO_STATION != 0))
                .map(|(&r, _)| r)
                .collect();
            refs.sort();
            let out = refs.into_iter().map(|r| XVal::Ref(r.0)).collect();
            list(g, out)
        }
        // `activator`: the actor SetTalkingActivatorActor gave it [documented:
        // GetTalkingActivatorActor; `crate::talking_activators`]; a base form reads its
        // first placed reference with one, by FormID [hypothesis]. 0 for none.
        "gettalkingactivatoractor" => {
            let Some(t) = form(args, 0).or(subject) else { return Some(Ok(XVal::Ref(0))) };
            let on_ref = |r: FormId| g.extra.get(&r).and_then(|e| e.talking_actor);
            let actor = if g.world.placed.get(t).is_some() {
                on_ref(t)
            } else {
                let mut refs: Vec<FormId> = g.world.placed.refs.iter().filter(|(_, p)| p.base == t).map(|(&r, _)| r).collect();
                refs.sort();
                refs.into_iter().find_map(on_ref)
            };
            XVal::Ref(actor.map_or(0, |a| a.0))
        }
        // ---- references ----
        // The reference's activation prompt override (XATO), "" for none [documented:
        // Get/SetRefActivationPromptOverride]; the setter keeps it for the session.
        "getrefactivationpromptoverride" => XVal::Str(subject.and_then(|r| activation_prompt(g, r)).unwrap_or_default()),
        "setrefactivationpromptoverride" => {
            if let (Some(r), Some(t)) = (subject, text(args, 0)) {
                g.other.jg6.prompts.insert(r, t);
            }
            zero
        }
        // 1 box, 2 sphere [documented: GetPrimitiveType]; 0 for none, or one RemovePrimitive
        // took away [hypothesis]. The reference table keeps box and sphere primitives
        // only, so type 3 reads 0 here.
        "getprimitivetype" => {
            let Some(r) = subject.filter(|r| !g.other.jg6.removed_primitives.contains(r)) else { return Some(Ok(zero)) };
            XVal::Num(g.world.placed.get(r).and_then(|p| p.primitive.as_ref()).map_or(0.0, |p| if p.sphere { 2.0 } else { 1.0 }))
        }
        // 1 when the reference had a primitive [documented: RemovePrimitive].
        "removeprimitive" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            let had = g.world.placed.get(r).is_some_and(|p| p.primitive.is_some()) && !g.other.jg6.removed_primitives.contains(&r);
            if had {
                g.other.jg6.removed_primitives.insert(r);
            }
            yes(had)
        }
        // The effect shaders scripts play on the reference [documented:
        // GetPlayingEffectShaders], in the order they started.
        "getplayingeffectshaders" => {
            let Some(r) = subject else { return Some(Ok(zero)) };
            let out = g.script_looks.shaders.iter().filter(|p| p.on == r).map(|p| XVal::Ref(p.shader.0)).collect();
            list(g, out)
        }
        // A live mine not going off, the player's too [documented: GetMineArmedEx, "works
        // as expected on player-placed mines"].
        "getminearmedex" => yes(subject.is_some_and(|r| g.other.jg6.armed_mines_all.contains(&r) || g.gaps.armed_mines.contains(&r))),
        // ---- calls per reference ----
        // `function [type] [distance] [angle] [cell]`: the function runs once per
        // reference of the loaded cells (or of `cell`), the reference its argument, without
        // the calling reference, disabled ones and, with a distance or an angle, those
        // farther or outside the heading [documented: CallPerRef, CallPerRefEx: an array
        // of types]. Order: the reference walk's [hypothesis].
        "callperref" | "callperrefex" => {
            let Some(func) = args.first().cloned() else { return Some(Ok(zero)) };
            let types: Vec<i64> = match args.get(1) {
                Some(XVal::Array(id)) if name == "callperrefex" => numbers(g, *id),
                Some(XVal::Num(x)) if name == "callperref" && *x != 0.0 => vec![*x as i64],
                _ => vec![0],
            };
            let (dist, angle) = (num(args, 2, 0.0), num(args, 3, 0.0));
            let cell = form(args, 4);
            // A cell that is not loaded gives no calls; the calls run before CallPerRef
            // returns [observed: jg6 rerun, Doc's house (not loaded) 0 calls against
            // GetRefsInCell 21; the player's cell 0xDEB16, type 0, 13 calls against 14
            // references, counted in the same script and unchanged a frame later].
            if cell.is_some_and(|c| !g.loaded_cells.contains(&c)) {
                return Some(Ok(zero));
            }
            let me = subject.unwrap_or(FormId::PLAYER_REF);
            let mut refs: Vec<FormId> = Vec::new();
            for t in types {
                for r in g.ref_walk_list(cell, t, if cell.is_some() { 0 } else { 2 }, false) {
                    if !refs.contains(&r) {
                        refs.push(r);
                    }
                }
            }
            let refs: Vec<FormId> = refs.into_iter().filter(|&r| r != me && g.world.is_enabled(r).unwrap_or(false) && within(g, me, r, dist, angle)).collect();
            return Some(run_each(g, ctx, &func, refs));
        }
        // `function level [type]`: once per actor of the processing level, 0 the actors
        // of the loaded cells, 3 the others, 1 and 2 none (as GetActorsByProcessingLevel,
        // `nvse_gaps`) [documented: CallPerMobileObject, CallPerMobileObjectEx: an array of
        // types], without the calling reference and disabled ones. The original's level 0
        // also takes some references that are not actors [observed: jg6 rerun, 20 calls =
        // the 16 of GetActorsByProcessingLevel 0 and 4 other references, not identified];
        // they are not called here [not built].
        "callpermobileobject" | "callpermobileobjectex" => {
            let Some(func) = args.first().cloned() else { return Some(Ok(zero)) };
            let types: Vec<i64> = match args.get(2) {
                Some(XVal::Array(id)) if name == "callpermobileobjectex" => numbers(g, *id),
                Some(XVal::Num(x)) if name == "callpermobileobject" && *x != 0.0 => vec![*x as i64],
                _ => Vec::new(),
            };
            let high = g.package_actors();
            let mut actors: Vec<FormId> = match num(args, 1, -1.0) as i64 {
                0 => high.into_iter().collect(),
                3 => g.world.placed.refs.iter().filter(|(r, p)| matches!(&p.sig, b"ACHR" | b"ACRE") && !high.contains(r)).map(|(&r, _)| r).collect(),
                _ => Vec::new(),
            };
            actors.sort();
            let me = subject.unwrap_or(FormId::PLAYER_REF);
            let actors: Vec<FormId> = actors
                .into_iter()
                .filter(|&r| r != me && g.world.is_enabled(r).unwrap_or(false))
                .filter(|&r| types.is_empty() || types.iter().any(|&t| g.ref_of_type(r, t)))
                .collect();
            return Some(run_each(g, ctx, &func, actors));
        }
        _ => return None,
    };
    Some(Ok(v))
}

/// Whether `r` is within `dist` of `me` and within `angle` degrees of its heading (0: no
/// check) [documented: CallPerRef, GetDistance3D and GetHeadingAngle].
fn within(g: &Game, me: FormId, r: FormId, dist: f64, angle: f64) -> bool {
    if dist <= 0.0 && angle <= 0.0 {
        return true;
    }
    let (Some(a), Some(b)) = (g.world.peek(me), g.world.peek(r)) else { return false };
    let d = [b.pos[0] - a.pos[0], b.pos[1] - a.pos[1], b.pos[2] - a.pos[2]];
    if dist > 0.0 && f64::from((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()) > dist {
        return false;
    }
    if angle > 0.0 {
        let mut h = f64::from(d[0]).atan2(f64::from(d[1])).to_degrees() - f64::from(a.rot[2]).to_degrees();
        h = (h + 180.0).rem_euclid(360.0) - 180.0;
        if h.abs() > angle {
            return false;
        }
    }
    true
}

/// Run `func` once per reference, the reference its one argument; an error stops the
/// calls and the calling script [hypothesis].
fn run_each(g: &mut Game, ctx: &RunCtx, func: &XVal, refs: Vec<FormId>) -> Result<XVal, String> {
    for r in refs {
        g.run_function(func, None, vec![XVal::Ref(r.0)], None, *ctx)?;
    }
    Ok(XVal::Num(0.0))
}

/// The whole numbers of an array, in its order.
fn numbers(g: &Game, id: u32) -> Vec<i64> {
    g.ext.array(id).map(|a| a.entries.iter().filter_map(|(_, v)| if let XVal::Num(x) = v { Some(*x as i64) } else { None }).collect()).unwrap_or_default()
}

/// Active climate for `GetMoonPhase` indoors and outdoors: current exterior climate, else
/// `ForceClimate`, `SetCurrentClimate`, the last entered exterior worldspace's effective
/// climate, or `DefaultClimate` (`0x0000015F` / EDID `"DefaultClimate"`) /
/// `"NVDefaultClimate"` when loaded [design; observed indoors:
/// `w4c_b3_03_jg6_actor_world_misc`].
fn moon_climate(g: &Game) -> Option<FormId> {
    g.current_climate()
        .or(g.other.jip12.forced_climate)
        .or_else(|| g.other.jip12.set_climate.map(|(c, _)| c))
        .or_else(|| g.other.jg6.last_exterior_world.and_then(|w| g.worldspace_climate(w)))
        .or_else(|| {
            [Some(FormId(0x0000_015F)), g.data.by_edid("DefaultClimate"), g.data.by_edid("NVDefaultClimate")]
                .into_iter()
                .flatten()
                .find(|&c| g.data.sig(g.unclone(c)) == Some(*b"CLMT"))
        })
}
