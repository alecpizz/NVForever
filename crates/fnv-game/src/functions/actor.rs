//! Actor functions: actor values, death, factions, voice, sex, race, flags.
//!
//! Actor value codes are the indices of `ActorValue` in `schemas/fnv/common.json`
//! [documented: xEdit; scripts.md §2.4]. What a value starts at (its record field or
//! formula) is `crate::actor_values`; docs/spec-notes/actor-values.md has the whole
//! model.
//!
//! A value is its base plus three pools [documented: GECK wiki ActorValue]:
//!
//! - the **base**: from the record or formula, or the number `SetActorValue` put in its
//!   place (`GetBaseActorValue` reads it);
//! - the **permanent modifier**: `ModActorValue` adds to it and `ForceActorValue` moves
//!   it so that the current value is the one asked for (`GetPermanentActorValue` reads
//!   base + this);
//! - the **damage**: `DamageActorValue` adds to it, `RestoreActorValue` takes from it and
//!   never past zero [documented: both pages; observed: Contreras' supplies restoring
//!   100 of a head condition damaged by 40 left it at 100, `L5.quest.effect_start`,
//!   layer5-captures.md §9.3 b]; combat's health and limb damage goes here;
//! - the **temporary modifier**: the Value Modifier effects with the Recover flag active
//!   on the actor (aid items, spells; `crate::aid`, docs/spec-notes/aid-items.md §3).
//!   Worn enchantments do not feed it yet (the Pip-Boy adds them itself).
//!
//! The permanent value (base + permanent modifier) of S.P.E.C.I.A.L. lives in the Layer
//! 4 world, where the carry weight rule and the menus read and write it; the override
//! and the permanent modifier of everything else, and of the attributes of actors the
//! world holds none for (creatures), live here.

use super::{base_of, none, yes, Args, Ctx};
use crate::game::Game;
use fnv_script::vm::Value;
use fnv_sim::forms::SPECIAL;
use fnv_sim::FormId;

fn special(code: u16) -> Option<&'static str> {
    (5..=11).contains(&code).then(|| SPECIAL[(code - 5) as usize])
}

/// The names of the actor value codes 0–76 ([`fnv_script::table::AV_NAMES`]).
pub use fnv_script::table::AV_NAMES;

fn squash(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace() && *c != '_').flat_map(char::to_lowercase).collect()
}

/// An actor value code from its xEdit name (`Perception Condition`), without regard to
/// case and spaces.
pub fn av_by_name(name: &str) -> Option<u16> {
    let want = squash(name);
    AV_NAMES.iter().position(|n| squash(n) == want).map(|i| i as u16)
}

use fnv_script::table::SCRIPT_AV_NAMES;

/// An actor value code from its name: the script name (the decoder's, `encodings.json`,
/// e.g. `NightEye`, `PerceptionCondition`, else [`SCRIPT_AV_NAMES`], e.g. `PoisonResist`)
/// or the xEdit name, either without regard to case and spaces (`Nighteye`, `Night
/// Eye`), or a number.
pub fn av_code(g: &Game, name: &str) -> Result<u16, String> {
    if let Ok(n) = name.parse::<u16>() {
        return Ok(n);
    }
    let want = squash(name);
    if let Some(m) = g.data.table.enums.get("actor_value") {
        if let Some((&c, _)) = m.iter().find(|(_, n)| squash(n) == want) {
            return Ok(c);
        }
    }
    if let Some((c, _)) = SCRIPT_AV_NAMES.iter().find(|(_, n)| squash(n) == want) {
        return Ok(*c);
    }
    AV_NAMES.iter().position(|n| squash(n) == want).map(|i| i as u16).ok_or_else(|| format!("no actor value named {name:?}"))
}

/// The permanent modifier of an actor value (ModActorValue, ForceActorValue).
fn modifier(g: &Game, actor: FormId, code: u16) -> f64 {
    g.extra.get(&actor).and_then(|e| e.av_permanent.get(&code)).copied().unwrap_or(0.0)
}

/// The world's run-time value of an attribute: its permanent value, when it holds one.
fn world_special(g: &Game, actor: FormId, code: u16) -> Option<f64> {
    g.world.peek(actor)?.actor_values.get(special(code)?).map(|v| *v as f64)
}

/// The base value: SetActorValue's number, else the record's or the formula's.
fn base(g: &mut Game, actor: FormId, code: u16) -> Result<f64, String> {
    if let Some(v) = world_special(g, actor, code) {
        return Ok(v - modifier(g, actor, code));
    }
    match g.extra.get(&actor).and_then(|e| e.actor_values.get(&code)).copied() {
        Some(v) => Ok(v),
        None => g.base_actor_value(actor, code),
    }
}

/// The permanent value of an actor value (GetPermanentActorValue): base plus permanent
/// modifier.
pub(crate) fn permanent_value(g: &mut Game, actor: FormId, code: u16) -> Result<f64, String> {
    permanent(g, actor, code)
}

/// The permanent value: the base plus the permanent modifier.
fn permanent(g: &mut Game, actor: FormId, code: u16) -> Result<f64, String> {
    Ok(base(g, actor, code)? + modifier(g, actor, code))
}

/// The permanent value of an actor value, 0 when it cannot be read.
pub(crate) fn actor_value_base(g: &mut Game, actor: FormId, code: u16) -> f64 {
    permanent(g, actor, code).unwrap_or(0.0)
}

/// Add `d` to an actor value's damage part (as DamageActorValue).
pub fn add_damage(g: &mut Game, actor: FormId, code: u16, d: f64) {
    let total = damage(g, actor, code) + d.max(0.0);
    set_damage(g, actor, code, total);
    if code == crate::actor_values::FATIGUE && d > 0.0 {
        g.fatigue_changed(actor);
    }
}

/// The damage an actor value has taken (0 or more).
pub fn damage(g: &Game, actor: FormId, code: u16) -> f64 {
    g.extra.get(&actor).and_then(|e| e.av_damage.get(&code)).copied().unwrap_or(0.0)
}

/// A change of one of `actor`'s values, with JohnnyGuitar's actor value change
/// handlers told of it: every change of one of the player's values, whatever made it
/// (a script, an aid item's effect, a hit), and never another actor's [documented:
/// SetOnActorValueChangeEventHandler, "limited to Player's actor value changes"; the
/// derived skills, which do not dispatch it, are worked out on reading here and never
/// stored]. A change a handler itself makes is not told again [our choice: no loop].
fn told<R>(g: &mut Game, actor: FormId, code: u16, change: impl FnOnce(&mut Game) -> R) -> R {
    if actor != FormId::PLAYER_REF || g.gaps.av_handlers.is_empty() || g.gaps.av_telling {
        return change(g);
    }
    let before = actor_value(g, actor, code).ok();
    let out = change(g);
    if let (Some(old), Ok(new)) = (before, actor_value(g, actor, code)) {
        if new != old {
            g.av_event(actor, code, old, new);
        }
    }
    out
}

fn set_damage(g: &mut Game, actor: FormId, code: u16, d: f64) {
    told(g, actor, code, |g| set_damage_untold(g, actor, code, d));
}

/// God mode (`tgm`) keeps a change off the player: its Action Points are never spent and
/// its rads never rise, whatever asks (a VATS attack, a tweak's cost, the world's
/// radiation, a script's DamageActorValue or ModActorValue) [design: owner decision
/// 2026-11-15, issue #2743; an aid effect's damage is kept off in `Game::change_value`,
/// and the needs' own rise goes on, as observed there].
pub(crate) fn god_spares(g: &Game, actor: FormId, code: u16) -> bool {
    actor == FormId::PLAYER_REF && g.toggles.god_mode && matches!(code, crate::actor_values::ACTION_POINTS | crate::aid::RAD_LEVEL)
}

/// The limb conditions god mode keeps whole: Perception (head), Endurance (torso), Left
/// and Right Attack, Left and Right Mobility Condition (codes 25–30, `AV_NAMES`); Brain
/// Condition (31) is left as it is: the issue names the six body parts [design, #2792].
const GOD_KEPT_LIMBS: std::ops::RangeInclusive<u16> = 25..=30;

/// God mode (`tgm`) keeps a script from hurting the player: a DamageActorValue on its
/// Health or a limb's condition does nothing, nor does a ModActorValue or ForceActorValue
/// that would lower them (the bear traps' scripts run `DamageActorValue Health 45` and
/// a leg's condition 50 on their victim, docs/spec-notes/traps.md §3). Restoring them, and
/// SetActorValue, go on as for Action Points and rads [design, #2792: the maintainer's
/// request on #2743, 2026-11-16, "no trap may damage the player in god mode"]. Hits and
/// effects are kept off before they get here (`nvf-play` `combat::hit_player`,
/// `Game::change_value`), so this is the script functions' check only: `add_modifier` is
/// also how an effect's modifier is held and released, and a release must still lower.
pub(crate) fn god_keeps_whole(g: &Game, actor: FormId, code: u16) -> bool {
    actor == FormId::PLAYER_REF && g.toggles.god_mode && (code == crate::actor_values::HEALTH || GOD_KEPT_LIMBS.contains(&code))
}

fn set_damage_untold(g: &mut Game, actor: FormId, code: u16, d: f64) {
    // God mode: the player's Action Points are not spent (their damage never grows).
    if code == crate::actor_values::ACTION_POINTS && god_spares(g, actor, code) && d > damage(g, actor, code) {
        return;
    }
    let e = g.extra.entry(actor).or_default();
    if d > 0.0 {
        e.av_damage.insert(code, d);
    } else {
        e.av_damage.remove(&code);
    }
    sync_carry(g, actor, code);
}

/// The world's carry capacity (the menus' weight line, `fnv_sim::World::encumbrance`)
/// follows the actor's current Carry Weight: its formula reads the current Strength,
/// temporary modifiers included (Buffout's +2), and Carry Weight's own modifiers add to
/// it [documented: GECK wiki GetCalculatedMaxCarryWeight, "CarryWeight … Actor Values";
/// hypothesis: the current, not the permanent, Strength; aid-items.md §12]. Called when
/// Strength or Carry Weight changes, and every frame for the player (its Get Max Carry
/// Weight perks: Hoarder + 25 [documented: GECK wiki Entry Point, "run when the actor's max
/// carry weight is calculated"], `Game::sync_perk_weights`).
pub(crate) fn sync_carry(g: &mut Game, actor: FormId, code: u16) {
    use crate::actor_values::{CARRY_WEIGHT, STRENGTH};
    if code != STRENGTH && code != CARRY_WEIGHT {
        return;
    }
    let Ok(formula) = g.world.formula_capacity(actor) else { return };
    let Ok(capacity) = actor_value(g, actor, CARRY_WEIGHT) else { return };
    let capacity = if actor == FormId::PLAYER_REF { g.entry_point(actor, crate::perks::GET_MAX_CARRY_WEIGHT, capacity).0 } else { capacity };
    g.world.set_carry_bonus(actor, capacity - formula as f64);
}

/// The temporary modifier of an actor value (active effects).
pub fn temporary(g: &Game, actor: FormId, code: u16) -> f64 {
    g.extra.get(&actor).and_then(|e| e.av_temporary.get(&code)).copied().unwrap_or(0.0)
}

/// Add `d` to the temporary modifier.
pub(crate) fn add_temporary(g: &mut Game, actor: FormId, code: u16, d: f64) {
    told(g, actor, code, |g| add_temporary_untold(g, actor, code, d));
}

fn add_temporary_untold(g: &mut Game, actor: FormId, code: u16, d: f64) {
    let e = g.extra.entry(actor).or_default();
    let m = e.av_temporary.get(&code).copied().unwrap_or(0.0) + d;
    if m.abs() > 1e-9 {
        e.av_temporary.insert(code, m);
    } else {
        e.av_temporary.remove(&code);
    }
    sync_carry(g, actor, code);
}

/// Take up to `amount` (0 or more) off an actor value's damage, as RestoreActorValue.
pub fn restore(g: &mut Game, actor: FormId, code: u16, amount: f64) {
    let d = damage(g, actor, code) - amount.max(0.0);
    set_damage(g, actor, code, d);
}

/// The current value: the permanent value and the temporary modifier, minus the damage.
/// The player's XP is the experience the game keeps [observed: data3, GetActorValue XP
/// reads RewardXPExact's 5 at once].
pub fn actor_value(g: &mut Game, actor: FormId, code: u16) -> Result<f64, String> {
    if actor == FormId::PLAYER_REF && code == crate::actor_values::XP {
        return Ok(g.player.xp);
    }
    Ok(permanent(g, actor, code)? + temporary(g, actor, code) - damage(g, actor, code))
}

/// The current value within the range the game calculates with (S.P.E.C.I.A.L. 1–10,
/// skills 0–100), 0 when it cannot be read [documented: GECK wiki
/// GetThresholdedActorValue].
pub fn actor_value_capped(g: &mut Game, actor: FormId, code: u16) -> f64 {
    crate::actor_values::capped(code, actor_value(g, actor, code).unwrap_or(0.0))
}

/// The value's parts: base, permanent modifier, damage (as GetActorValueInfo lists them).
pub fn actor_value_parts(g: &mut Game, actor: FormId, code: u16) -> Result<[f64; 3], String> {
    Ok([base(g, actor, code)?, modifier(g, actor, code), damage(g, actor, code)])
}

/// Put `v` in the base's place (as SetActorValue). An attribute's permanent value goes
/// to the world when the world holds attributes for the actor.
fn set_base(g: &mut Game, actor: FormId, code: u16, v: f64) -> Result<(), String> {
    told(g, actor, code, |g| set_base_untold(g, actor, code, v))
}

fn set_base_untold(g: &mut Game, actor: FormId, code: u16, v: f64) -> Result<(), String> {
    if let Some(name) = special(code) {
        // Something that has no attributes takes none.
        base(g, actor, code)?;
        if g.world.set_actor_value(actor, name, (v + modifier(g, actor, code)) as f32).is_ok() {
            sync_carry(g, actor, code);
            return Ok(());
        }
    }
    g.extra.entry(actor).or_default().actor_values.insert(code, v);
    sync_carry(g, actor, code);
    Ok(())
}

/// Add `d` to the permanent modifier (as ModActorValue).
pub(crate) fn add_modifier(g: &mut Game, actor: FormId, code: u16, d: f64) -> Result<(), String> {
    told(g, actor, code, |g| add_modifier_untold(g, actor, code, d))
}

fn add_modifier_untold(g: &mut Game, actor: FormId, code: u16, d: f64) -> Result<(), String> {
    // God mode: the player's rads do not rise ([`god_spares`]).
    if code == crate::aid::RAD_LEVEL && d > 0.0 && god_spares(g, actor, code) {
        return Ok(());
    }
    let before = permanent(g, actor, code)?;
    let e = g.extra.entry(actor).or_default();
    let m = e.av_permanent.get(&code).copied().unwrap_or(0.0) + d;
    if m != 0.0 {
        e.av_permanent.insert(code, m);
    } else {
        e.av_permanent.remove(&code);
    }
    if let Some(name) = special(code) {
        // The world's number is the permanent value; without one the base stays the
        // record's and the modifier is added on reading.
        let _ = g.world.set_actor_value(actor, name, (before + d) as f32);
    }
    sync_carry(g, actor, code);
    Ok(())
}

/// Move the base by `d`, leaving the modifier and the damage alone.
pub(crate) fn shift_base(g: &mut Game, actor: FormId, code: u16, d: f64) -> Result<(), String> {
    // God mode: the player's rads do not rise ([`god_spares`]): the world's radiation,
    // irradiated water, food and drink and a script's DamageActorValue all come here.
    if code == crate::aid::RAD_LEVEL && d > 0.0 && god_spares(g, actor, code) {
        return Ok(());
    }
    let v = base(g, actor, code)? + d;
    set_base(g, actor, code, v)
}

/// Make the current value exactly `v` with no modifier and no damage (the state a
/// capture save reports: the runner's `given` of kind `actor_value`).
pub fn set_current(g: &mut Game, actor: FormId, code: u16, v: f64) -> Result<(), String> {
    set_damage(g, actor, code, 0.0);
    if let Some(e) = g.extra.get_mut(&actor) {
        e.av_permanent.remove(&code);
    }
    set_base(g, actor, code, v)
}

pub fn get_actor_value(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    // Authored abilities and worn enchantments are in the modifiers (`crate::abilities`).
    g.sync_actor_effects(r);
    Ok(Value::Num(actor_value(g, r, a.code(0)?)?))
}

/// `GetBaseActorValue`: the base value, without the modifiers and the damage
/// [documented: GECK wiki ActorValue, GetBaseActorValue].
pub fn get_base_actor_value(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    Ok(Value::Num(base(g, r, a.code(0)?)?))
}

/// `GetPermanentActorValue`: the base value plus the permanent modifier [documented:
/// GECK wiki ActorValue, "the combined base value + the permanent modifier"].
pub fn get_permanent_actor_value(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    g.sync_actor_effects(r);
    Ok(Value::Num(permanent(g, r, a.code(0)?)?))
}

/// `SetActorValue`: the base value becomes the given number; the modifiers and the
/// damage stay [documented: GECK wiki ActorValue, "this base value can be overridden by
/// the SetActorValue command"].
pub fn set_actor_value(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let code = a.code(0)?;
    if crate::actor_values::SKILLS.contains(&code) && crate::revision::keeps(ctx, r, || format!("the skill {} (points spent stay)", AV_NAMES[code as usize])) {
        return none();
    }
    if crate::actor_values::SPECIALS.contains(&code) && crate::revision::keeps(ctx, r, || format!("the attribute {} (points are moved on the attributes page)", AV_NAMES[code as usize])) {
        return none();
    }
    // The player's skill keeps its part from the attributes: SetActorValue sets the rest
    // [observed: gaps6, Guns 17 (all from the attributes at level 1, untagged) reads 67
    // after SetActorValue Guns 50, and 117 after 100, GetBaseActorValue the same;
    // o26_setav 2026-11-06, nvfdoc: `player.setav repair 35` from the console and from a
    // script read 52 with Repair 17 from the attributes, issue #1575].
    let v = a.num(1)?;
    // The tag bonus stays too, the points spent go [observed: o26_setav, nvfLanius at
    // level 50, 27 from the attributes: Repair 127 read 62 after SetAV 35 and Science 127
    // read 27 after SetAV 0, Guns 142 read 92 after SetAV 50, 15 above; that the 15 is
    // Guns' tag bonus (fAVDTagSkillBonus) is a hypothesis, the save's tags were not read].
    let v = if r == FormId::PLAYER_REF && crate::actor_values::SKILLS.contains(&code) { v + g.derived_skill(r, code) + g.tag_bonus(code) } else { v };
    set_base(g, r, code, v)?;
    none()
}

/// The actor value functions with JohnnyGuitar's NPC actor value change handlers told
/// of a change: (actor, code, old value, new value). The player's handlers are told
/// where the value is written ([`told`]) [observed: data3, ModAV Luck +1 gave (11, 10,
/// 11), -1 (11, 11, 10)].
fn watched(g: &mut Game, ctx: &Ctx, a: &Args, f: fn(&mut Game, &Ctx, &Args) -> Result<Value, String>) -> Result<Value, String> {
    let target = ctx.subject().ok().zip(a.code(0).ok()).filter(|_| g.jg8_listening(crate::nvse_jg8::Ev::NpcActorValue));
    let before = target.and_then(|(r, c)| actor_value(g, r, c).ok());
    let out = f(g, ctx, a)?;
    if let (Some((r, c)), Some(old)) = (target, before) {
        if let Ok(new) = actor_value(g, r, c) {
            if new != old {
                g.jg8_actor_value(r, c, old, new);
            }
        }
    }
    Ok(out)
}

pub fn set_actor_value_watched(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    // UnsetAV's base to go back to (`crate::nvse_jg6`).
    crate::nvse_jg6::note_set_av(g, ctx, a);
    watched(g, ctx, a, set_actor_value)
}
pub fn mod_actor_value_watched(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    watched(g, ctx, a, mod_actor_value)
}
pub fn force_actor_value_watched(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    watched(g, ctx, a, force_actor_value)
}
pub fn damage_actor_value_watched(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    watched(g, ctx, a, damage_actor_value)
}
pub fn restore_actor_value_watched(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    watched(g, ctx, a, restore_actor_value)
}

/// `ForceActorValue`: the current value becomes the given number, by changing the
/// permanent modifier by the difference [documented: GECK wiki ForceActorValue, the
/// equivalent ModActorValue; ActorValue, "will alter the permanent modifier"].
pub fn force_actor_value(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let code = a.code(0)?;
    let diff = a.num(1)? - actor_value(g, r, code)?;
    // God mode: not lowering the player's Health or a limb ([`god_keeps_whole`]).
    if diff < 0.0 && god_keeps_whole(g, r, code) {
        return none();
    }
    add_modifier(g, r, code, diff)?;
    none()
}

/// `ModActorValue`: the permanent modifier changes by the amount [documented: GECK wiki
/// ActorValue, the permanent pool].
pub fn mod_actor_value(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let (code, d) = (a.code(0)?, a.num(1)?);
    // God mode: not lowering the player's Health or a limb ([`god_keeps_whole`]).
    if d < 0.0 && god_keeps_whole(g, r, code) {
        return none();
    }
    add_modifier(g, r, code, d)?;
    none()
}

/// `DamageActorValue`: the damage grows by the amount [documented: catalogue summary; a
/// negative amount heals at most the damage taken, hypothesis].
///
/// Rads and Hardcore's needs grow the other way: damage raises them, restoring lowers
/// them, never below 0 [hypothesis: as their aid effects, aid-items.md §3; the Cannibal
/// perk's script restores Hunger 50 with the health it gives].
/// Take `amount` from `actor`'s Fatigue, as DamageAV Fatigue does: at 0 it is knocked
/// out (an ammo effect's Fatigue Mod, logic-fields.md §3; [`Game::damage_fatigue`]).
pub(crate) fn damage_fatigue(g: &mut Game, actor: FormId, amount: f64) {
    if amount <= 0.0 {
        return;
    }
    let code = crate::actor_values::FATIGUE;
    let d = damage(g, actor, code) + amount;
    set_damage(g, actor, code, d);
    g.fatigue_changed(actor);
}

pub fn damage_actor_value(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let code = a.code(0)?;
    if crate::aid::grows(code) {
        shift_growing(g, r, code, a.num(1)?)?;
        return none();
    }
    let amount = a.num(1)?;
    // God mode (`god_keeps_whole`) and JIP `ToggleImmortalMode 2` (for Health) keep
    // scripted `DamageActorValue` from hurting the player [observed: Fallout: New Vegas
    // 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, probe jip_45_immortal_mode_damageav_bypass:
    // ToggleImmortalMode 2 kept Health at 200.0 across Player.DamageAV Health 10].
    if amount > 0.0
        && (god_keeps_whole(g, r, code)
            || (r == FormId::PLAYER_REF && g.immortal_mode() == 2 && code == crate::actor_values::HEALTH))
    {
        return none();
    }
    let limb_before = if crate::actor_values::LIMB_CONDITIONS.contains(&code) { Some(actor_value(g, r, code)?) } else { None };
    let d = damage(g, r, code) + amount;
    set_damage(g, r, code, d);
    // JIP's health-damage handlers see DamageAV too, with no source [documented:
    // SetOnHealthDamageEventHandler; observed: ev.amount -1, ev.source.type].
    if code == crate::actor_values::HEALTH && amount > 0.0 {
        g.jip_event("healthdamage", Some(r), vec![fnv_script::vm::XVal::Num(-amount), fnv_script::vm::XVal::Ref(0)]);
        // Out of health: dead, or knocked out (docs/spec-notes/unconscious.md §2.2).
        g.health_lowered(r);
    }
    // Out of fatigue: knocked out (docs/spec-notes/unconscious.md §2.3).
    if code == crate::actor_values::FATIGUE {
        g.fatigue_changed(r);
    }
    // A limb's condition brought to 0 cripples it, and JIP's crippled-limb handlers are
    // told as for a hit's [observed: jip_events2, `DamageAV LeftAttackCondition 200` on
    // a placed NPC fired its handler with limb 2; docs/spec-notes/jip-events.md §5].
    if let Some(before) = limb_before {
        if before > 0.0 && actor_value(g, r, code)? <= 0.0 {
            let limb = code - crate::actor_values::LIMB_CONDITIONS.start();
            g.jip_event("crippledlimb", Some(r), vec![fnv_script::vm::XVal::Num(f64::from(limb))]);
        }
    }
    none()
}

/// `RestoreActorValue`: the damage shrinks by the amount, never below zero, so the value
/// never rises past its permanent part [documented: catalogue notes; observed:
/// `L5.quest.effect_start`].
pub fn restore_actor_value(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let code = a.code(0)?;
    if crate::aid::grows(code) {
        shift_growing(g, r, code, -a.num(1)?.max(0.0))?;
        return none();
    }
    let d = damage(g, r, code) - a.num(1)?.max(0.0);
    set_damage(g, r, code, d);
    none()
}

/// Move a value that grows (rads, needs) by `by`, never below 0.
pub(crate) fn shift_growing(g: &mut Game, r: FormId, code: u16, by: f64) -> Result<(), String> {
    let now = actor_value(g, r, code)?;
    let to = (now + by).max(0.0);
    shift_base(g, r, code, to - now)
}

pub fn get_dead(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    yes(g.extra.get(&r).is_some_and(|e| e.dead))
}

/// `actor.KillActor [killer] [limb] [cause]`: the actor dies at once; its OnDeath blocks
/// run when the calling script has finished [hypothesis: `Game::on_death`], and the kill counts for GetDeadCount of its base (`Game::kill`,
/// shared with combat deaths). The cause is what GetCauseofDeath then reads [documented:
/// GECK wiki KillActor, "See GetCauseofDeath for death values"]. A limb given (≥ 0) is
/// dismembered, on a dead actor too ("KillActor can be called on dead actors to dismember
/// limbs" [documented: GECK wiki KillActor notes]); the engine, which knows the actor's
/// body parts, does it (docs/spec-notes/dismemberment.md §5).
pub fn kill_actor(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    // An essential actor is knocked out "for a few seconds" instead [documented: GECK
    // wiki KillActor notes] (docs/spec-notes/unconscious.md §2.2).
    if essential(g, r) && r != FormId::PLAYER_REF {
        g.knock_out(r, crate::unconscious::Cause::Essential, a.opt_form(0));
        return none();
    }
    if g.kill(r, a.opt_form(0)) {
        if let Ok(cause) = a.int(2) {
            g.extra.entry(r).or_default().death_cause = Some(cause);
        }
    }
    if let Ok(location) = a.int(1) {
        if location >= 0 && g.is_dead(r) && r != FormId::PLAYER_REF {
            g.combat_requests.push(crate::combat::CombatRequest::Dismember { actor: r, location, by: a.opt_form(0), explode: None });
        }
    }
    none()
}

/// `actor.DetonatePlacedExplosives`: the actor's placed explosives that detonate (C-4)
/// go off [documented: GECK wiki Projectile, "Detonates"]. The projectiles are the
/// engine's: the request waits in `Game::combat_requests`.
pub fn detonate_placed_explosives(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.on.or(ctx.self_ref).unwrap_or(FormId::PLAYER_REF);
    g.combat_requests.push(crate::combat::CombatRequest::Detonate(r));
    none()
}

/// The body location of the actor's last hit, −1 for none [documented: catalogue notes;
/// explosions give −1].
pub fn get_hit_location(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    Ok(Value::Num(g.extra.get(&r).and_then(|e| e.last_hit).map_or(-1, |h| h.location) as f64))
}

/// 1 while the actor has a weapon out [documented: catalogue summary and notes of
/// IsWeaponOut: the value changes when the weapon is attached to the weapon node]. The
/// state is the animating side's ([`Game::set_weapon_out`]); an actor nobody reported
/// has its weapon away.
///
/// An actor the engine has not told about in the last second (not drawn: unloaded, far,
/// or just gone) reads the rule the engine draws by ([`Game::wants_weapon_out`]), as does
/// a dead one: 0 [hypothesis: a corpse holds nothing out]. [observed: Sunny Smiles at
/// rest read 0 drawn and undrawn, capture `fa_batch2_loaded_npcs`.]
pub fn is_weapon_out(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    if g.is_dead(r) {
        return yes(false);
    }
    // "IsWeaponOut will always return 0 for Creatures without a weapon" [documented:
    // GECK wiki IsWeaponOut notes]; a creature's attacks alert it without drawing anything.
    if base_of(g, r).and_then(|b| g.data.sig(b)).as_ref() == Some(b"CREA") && super::items::equipped_weapon(g, r).is_none() {
        return yes(false);
    }
    // A co-op guest's shot, run as the player's: its weapon was out.
    if r == FormId::PLAYER_REF && g.shot_for_player.is_some() {
        return yes(true);
    }
    yes(g.weapon_is_out(r))
}

/// GetWeaponAnimType's value for a WEAP DNAM animation type (the record's numbering).
///
/// [documented: GECK wiki GetWeaponAnimType — the function tells 11 animation types
/// apart and has one catch-all value, several of the 13 (Fallout 3) or 14 (New Vegas)
/// record types sharing a value. The page's table itself is a template we have no copy
/// of.] The numbering here is a [hypothesis] fitted to the shipped scripts
/// (docs/spec-notes/combat-rules.md §14):
/// - the record's types in order starting at 1, the two pistol types sharing 4 and the
///   ballistic and energy rifles sharing 5, which leaves the 11 values 1–11;
/// - Thrown (New Vegas's 14th type) and anything else: the catch-all, 0.
///
/// [observed: `GetWeaponAnimType == 4` is tested together with `GetEquipped
/// NVWeapMS22Camera`, whose record type is 3, Pistol - Ballistic (1 Hand); the weapon
/// alarm scripts test `IsWeaponOut == 1 && GetWeaponAnimType != 1` (raised fists are no
/// alarm) and the tutorial's bottles `> 3 && < 9`, which is then every gun: pistols 4,
/// rifles 5, automatic rifles 6, Handle 7, Launcher 8.] Which types share a value besides
/// the pistols is the unchecked part.
pub fn weapon_anim_code(record_type: i64) -> i64 {
    match record_type {
        0..=2 => record_type + 1,
        3 | 4 => 4,
        5 | 7 => 5,
        6 => 6,
        8..=12 => record_type - 1,
        _ => 0,
    }
}

/// `actor.GetWeaponAnimType`: the animation type of the equipped weapon as
/// [`weapon_anim_code`] numbers it; an actor with no weapon equipped fights with its
/// hands, Hand to Hand, 1 [hypothesis: scripts treat `<= 1` as unarmed and `!= 1` as
/// armed].
pub fn get_weapon_anim_type(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    Ok(Value::Num(g.weapon_anim_type(r) as f64))
}

impl Game<'_> {
    /// What `GetWeaponAnimType` answers for `actor` ([`get_weapon_anim_type`]): the
    /// equipped weapon's type, else that of the weapon the animating side draws for it
    /// (`Game::set_held_weapon`: NPCs' weapons are not equipped in the inventories here).
    pub fn weapon_anim_type(&mut self, actor: FormId) -> i64 {
        let held = self.extra.get(&actor).and_then(|e| e.held_weapon);
        // A co-op guest's shot, run as the player's: its weapon.
        if let Some(w) = self.shot_for_player.filter(|w| actor == FormId::PLAYER_REF && w.0 != 0) {
            return self.data.weapon_anim_types.get(&w).copied().map_or(1, weapon_anim_code);
        }
        let record_type = super::items::equipped_weapon(self, actor).or(held).and_then(|w| self.data.weapon_anim_types.get(&w).copied());
        record_type.map_or(1, weapon_anim_code)
    }
}

/// `actor.ResurrectActor [anim]`: the actor comes back "from the dead or unconscious
/// state" [documented: GECK wiki ResurrectActor], generated again from its record
/// (`crate::resurrect`, docs/spec-notes/resurrect.md). `anim` 1 has it stand up, 0 (or
/// none) just standing [documented: the wiki's parameter notes].
pub fn resurrect_actor(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let animate = a.int(0).is_ok_and(|v| v != 0);
    g.resurrect(r, animate);
    none()
}

pub fn get_dead_count(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let b = a.form(0)?;
    Ok(Value::Num(g.dead_count(b) as f64))
}

/// The factions and ranks the records give an actor: its base's `SNAM` list, or its
/// template's with the template flag "Factions" (docs/spec-notes/factions.md §2).
pub(crate) fn base_factions<'d>(g: &'d Game, r: FormId) -> &'d [(FormId, i8)] {
    base_of(g, r).and_then(|b| g.template_source(b, r, |d| d.template_factions)).map_or(&[], |(_, d)| &d.factions)
}

/// Faction rank of an actor: a run-time change, else the records' ([`base_factions`]);
/// −1 when not a member.
pub fn faction_rank(g: &Game, r: FormId, f: FormId) -> i32 {
    if let Some(&rank) = g.extra.get(&r).and_then(|e| e.factions.get(&f)) {
        return rank as i32;
    }
    // SetBaseFactionRank's (`crate::nvse_jip11`).
    if let Some(rank) = factions_source(g, r).and_then(|s| g.base_rank_override(s, f)) {
        return rank.max(-1) as i32;
    }
    base_factions(g, r).iter().find(|(x, _)| *x == f).map(|(_, rank)| *rank as i32).unwrap_or(-1)
}

pub fn get_in_faction(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    yes(faction_rank(g, r, a.form(0)?) >= 0)
}

pub fn get_faction_rank(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    Ok(Value::Num(faction_rank(g, r, a.form(0)?) as f64))
}

/// `AddToFaction` / `SetFactionRank`: the actor reference joins at the rank. −1 takes it
/// out [documented: GECK wiki SetFactionRank example]; any lower rank is read back as −1
/// too [hypothesis: below −1 is no rank either].
pub fn add_to_faction(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let f = a.form(0)?;
    let rank = (a.num_or(1, 0.0) as i8).max(-1);
    g.extra.entry(r).or_default().factions.insert(f, rank);
    forget_factions(g, r);
    none()
}

pub fn remove_from_faction(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    g.extra.entry(r).or_default().factions.insert(a.form(0)?, -1);
    forget_factions(g, r);
    none()
}

/// A script changed `r`'s factions: the kept lists of them go ([`FactionKeep`]).
fn forget_factions(g: &Game, r: FormId) {
    for keep in keeps(g) {
        if let Ok(mut k) = keep.factions.write() {
            k.remove(&r);
        }
    }
    forget_relations(g);
}

/// The voice type can be given directly or as a form list of voice types.
pub fn get_is_voice_type(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let v = a.form(0)?;
    let Some(voice) = g.voice_type_of(r) else { return yes(false) };
    yes(voice == v || super::refs::list_members(g, v).contains(&voice))
}

/// The actor's sex now, SexChange's included (`crate::appearance`).
fn is_female(g: &Game, r: FormId) -> bool {
    g.actor_female(r)
}

/// Sex codes: 0 male, 1 female (scripts.md §2.4).
pub fn get_is_sex(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    yes(is_female(g, r) == (a.code(0)? == 1))
}

pub fn get_pc_is_sex(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    yes(is_female(g, FormId::PLAYER_REF) == (a.code(0)? == 1))
}

/// `SexChange [sex] [resetAppearance]`: without a calling reference it changes the
/// player; without a sex it flips [documented: catalogue summary]; any actor
/// (`crate::appearance`).
pub fn sex_change(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject_or_player();
    let female = a.code(0).ok().map(|c| c == 1);
    let reset = a.int(1).is_ok_and(|v| v != 0);
    g.sex_change(r, female, reset);
    none()
}

/// `AgeRace steps`: the actor's race moves along its age links (`crate::appearance`).
pub fn age_race(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject_or_player();
    let steps = a.int(0)?;
    g.age_race(r, steps).ok_or_else(|| format!("{} has no race", g.data.name(r)))?;
    none()
}

pub fn get_is_race(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    // The character creator's choice and AgeRace stand for the record's.
    yes(g.actor_race(r) == Some(a.form(0)?))
}

pub fn get_is_class(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    yes(g.traits_class(r) == Some(a.form(0)?))
}

macro_rules! flag {
    ($set:ident, $get:ident, $field:ident) => {
        pub fn $set(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
            let r = ctx.subject()?;
            g.extra.entry(r).or_default().$field = a.num_or(0, 1.0) != 0.0;
            none()
        }
        #[allow(dead_code)]
        pub fn $get(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
            let r = ctx.subject()?;
            yes(g.extra.get(&r).is_some_and(|e| e.$field))
        }
    };
}

/// `SetGhost flag`: projectiles pass, no damage, enemies ignore it (`Game::is_ghost`).
/// SetGhost 0 also clears a base Is Ghost flag (actor-flags.md §3).
pub fn set_ghost(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let on = a.num_or(0, 1.0) != 0.0;
    let e = g.extra.entry(r).or_default();
    (e.ghost, e.ghost_off) = (on, !on);
    none()
}

/// `GetPlantedExplosive`: "Returns whether a live explosive has been planted on an
/// actor" [documented: GECK wiki GetPlantedExplosive]; true once
/// `fProjectileInventoryGrenadeFreakoutTime` has passed [documented: GECK wiki Projectile
/// notes] (`crate::pickpocket`).
pub fn get_planted_explosive(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    yes(g.has_planted_explosive(r))
}

/// `GetIsGhost`: SetGhost or the base's Is Ghost flag.
pub fn get_ghost(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    yes(g.is_ghost(r))
}

flag!(set_restrained, get_restrained, restrained);
flag!(set_unconscious_flag, get_unconscious, unconscious);

/// `SetUnconscious flag`. On a dead actor it brings it back first, with no get-up
/// animation [documented: GECK wiki SetUnconscious notes, "Using this on dead actors will
/// cause them to be resurrected, but will not cause them to play a "get up" animation"];
/// that it then lies unconscious under a flag of 1 is [hypothesis]. Unconscious, it
/// detects no one from this call on (`Game::forget_detected_by`).
pub fn set_unconscious(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    if g.is_dead(r) {
        g.resurrect(r, false);
    }
    let done = set_unconscious_flag(g, ctx, a)?;
    if g.extra.get(&r).is_some_and(|e| e.unconscious) {
        g.forget_detected_by(r);
    }
    Ok(done)
}
flag!(set_player_teammate, get_player_teammate, teammate);

/// 1 for an actor whose race has the Child flag (RACE DATA) [documented: catalogue
/// summary of IsChild].
pub fn is_child(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    yes(g.actor_race(r).is_some_and(|race| g.data.child_races.contains(&race)))
}

/// 1 for a creature of the given type (CREA DATA type) [documented: catalogue summary];
/// 0 for NPCs.
pub fn get_is_creature_type(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let t = a.code(0)? as i64;
    // The type sits in CREA DATA with the stats [hypothesis: it follows the "Stats" flag].
    yes(g.stats_def(r).and_then(|d| d.creature_type) == Some(t))
}

/// Whether `r` is spared death: knocked out at zero health instead of killed (combat,
/// harmful effects, DamageActorValue, KillActor, KillAllActors, planted explosives).
/// Outside Hardcore the player's teammate is, flag or not, unless JIP's
/// ToggleTeammateKillable made it killable; in Hardcore a teammate is not, flag or not
/// (docs/spec-notes/unconscious.md §2.2a). Everyone else: [`essential_flagged`].
pub fn essential(g: &Game, r: FormId) -> bool {
    if g.is_teammate(r) {
        // "Essential flagged teammates are not essential in Hardcore (New Vegas)"
        // [documented: GECK wiki SetPlayerTeammate notes]: a companion killed in
        // Hardcore dies, flag or not.
        if teammates_unprotected(g) {
            return false;
        }
        // No hired companion carries the Essential flag and no script sets it, yet
        // their OnDeath blocks exist for Hardcore: outside it the engine keeps a
        // teammate alive [hypothesis: unconscious.md §2.2a, observer-checks.md check
        // 165]. ToggleTeammateKillable "Sets the specified teammate referance as
        // killable" [documented: GECK wiki, JIP].
        if !g.jip_flags.contains(&(r, "killable")) {
            return true;
        }
    }
    essential_flagged(g, r)
}

/// Whether the Hardcore rule that teammates are not essential holds: in Hardcore, or,
/// with the player's Hardcore Tweaks on, as their Essential Companions switch says
/// [design: docs/design/tweaks.md].
fn teammates_unprotected(g: &Game) -> bool {
    if g.tweaks.hardcore_tweaks { !g.tweaks.hardcore_essential_companions } else { g.hardcore() }
}

/// Essential by flag, what IsEssential reads ("Returns 1 if the calling actor is flagged
/// as essential" [documented: GECK wiki IsEssential]): the reference's own flag
/// (SetActorRefEssential) or its base's (ACBS, changed by SetEssential). The reference
/// flag cannot clear the base's [documented: catalogue notes of SetActorRefEssential].
/// A teammate in Hardcore reads 0 (the SetPlayerTeammate note, [`essential`]).
pub fn essential_flagged(g: &Game, r: FormId) -> bool {
    if teammates_unprotected(g) && g.is_teammate(r) {
        return false;
    }
    let base = base_of(g, r);
    let base_flag = base
        .and_then(|b| g.forms.essential.get(&b).copied())
        .unwrap_or_else(|| g.base_data_source(r).is_some_and(|(id, d)| g.acbs_flag(id, d, crate::actor_values::acbs::ESSENTIAL, |d| d.essential)));
    base_flag || g.extra.get(&r).and_then(|e| e.essential).unwrap_or(false)
}

/// `SetEssential base flag`: the base form's flag, for every reference [documented:
/// catalogue summary].
pub fn set_essential(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let b = a.form(0)?;
    let b = base_of(g, b).filter(|_| g.world.placed.get(b).is_some()).unwrap_or(b);
    g.forms.essential.insert(b, a.num_or(1, 1.0) != 0.0);
    none()
}

pub fn set_actor_ref_essential(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    g.extra.entry(r).or_default().essential = Some(a.num_or(0, 1.0) != 0.0);
    none()
}

pub fn is_essential(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    yes(essential_flagged(g, r))
}

/// `actor.SetActorFullName message`: the base's name becomes the message's title, for
/// every reference [documented: catalogue notes; GECK wiki SetActorFullName: "changes
/// the name of the base object, so all instances of that NPC/creature will have a new
/// name", and it works on talking activators too]. Shown through
/// [`Game::script_name`]; saved with the rest of `Game::forms`.
pub fn set_actor_full_name(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let m = a.form(0)?;
    let b = base_of(g, r).unwrap_or(r);
    g.forms.saved_names.remove(&b);
    g.forms.names.insert(b, m);
    none()
}

impl<'a> Game<'a> {
    /// The name `SetActorFullName` or a converted `.fos` save gave a base form
    /// (`FormChanges::saved_names`, else the title (FULL) of the message in
    /// `FormChanges::names`) [documented: GECK wiki SetActorFullName summary].
    /// `None` when no saved rename exists.
    pub fn script_name(&self, base: FormId) -> Option<&str> {
        let data: &'a crate::data::GameData = self.data;
        self.forms
            .saved_names
            .get(&base)
            .map(String::as_str)
            .or_else(|| self.forms.names.get(&base).and_then(|m| data.messages.get(m)).and_then(|d| d.title.as_deref()))
    }
}

/// 1 when the given actor killed the caller (KillActor's killer) [documented:
/// catalogue summary].
pub fn is_killer(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let k = a.form(0)?;
    yes(g.extra.get(&r).and_then(|e| e.killer) == Some(k))
}

/// The factions an actor is in now: the records' ([`base_factions`]) unless removed, and
/// those added at run time.
pub fn factions_of(g: &Game, r: FormId) -> Vec<FormId> {
    // Kept while asked again and again ([`FactionKeep`]).
    let kept = keep(g).map(|k| &k.factions);
    if let Some(f) = kept.and_then(|k| k.read().ok()?.get(&r).cloned()) {
        return f;
    }
    let out = factions_now(g, r);
    if let Some(mut k) = kept.and_then(|k| k.write().ok()) {
        k.insert(r, out.clone());
    }
    out
}

/// Each actor's factions ([`factions_of`]) and how it stands towards others by them
/// ([`faction_relation`]), kept while something asks them again and again: a detection
/// round (`Game::begin_detection_round`) and a frame of the combat AI
/// (`Game::begin_faction_frame`). They change only by script (AddToFaction, SetEnemy and
/// the like) and crimes (SetPCEnemyOfFaction's rule), which forget them
/// ([`forget_factions`], [`forget_relations`]).
///
/// Read under a shared lock: a detection round works out its pairs' hostility on several
/// threads at once (`Game::round_hostility`, #1799), and almost every ask is a read.
#[derive(Debug, Default)]
pub struct FactionKeep {
    factions: std::sync::RwLock<std::collections::HashMap<FormId, Vec<FormId>>>,
    relations: std::sync::RwLock<std::collections::HashMap<(FormId, FormId), i64>>,
}

/// The keep in use: the detection round's while one runs, else the combat AI frame's.
fn keep<'a>(g: &'a Game) -> Option<&'a FactionKeep> {
    g.detection_round.as_ref().map(|round| &round.kept).or(g.faction_frame.as_ref())
}

/// Every keep there is, for forgetting.
fn keeps<'a>(g: &'a Game) -> impl Iterator<Item = &'a FactionKeep> {
    g.detection_round.as_ref().map(|round| &round.kept).into_iter().chain(g.faction_frame.as_ref())
}

/// The actor base that supplies `r`'s factions (its template chain's "Factions" source).
fn factions_source(g: &Game, r: FormId) -> Option<FormId> {
    if g.other.jip11.base_ranks.is_empty() {
        return None;
    }
    base_of(g, r).map(|b| g.data.base_template_source(b, |d| d.template_factions))
}

/// SetBaseFactionRank changed a base: every kept faction list goes.
pub(crate) fn forget_all_factions(g: &Game) {
    for keep in keeps(g) {
        if let Ok(mut k) = keep.factions.write() {
            k.clear();
        }
    }
    forget_relations(g);
}

fn factions_now(g: &Game, r: FormId) -> Vec<FormId> {
    let mut out: Vec<FormId> = base_factions(g, r).iter().map(|f| f.0).collect();
    if let Some(s) = factions_source(g, r) {
        out.extend(g.base_rank_overrides(s).into_iter().map(|(f, _)| f));
    }
    if let Some(e) = g.extra.get(&r) {
        out.extend(e.factions.keys().copied());
    }
    out.sort();
    out.dedup();
    out.retain(|&f| faction_rank(g, r, f) >= 0);
    out
}

/// The group combat reaction of faction `a` towards `b`: a run-time SetEnemy/SetAlly,
/// else FACT XNAM, else neutral (0) [documented: catalogue notes of
/// GetFactionCombatReaction: 0 neutral for unlisted pairs, 1 enemy, 2 ally, 3 friend].
pub fn reaction(g: &Game, a: FormId, b: FormId) -> i64 {
    // SetAllyTemp's and SetEnemyTemp's first (`nvse_showoff3`).
    if let Some(&v) = g.showoff.temp_reactions.get(&(a, b)) {
        return v;
    }
    if let Some(&v) = g.forms.faction_reactions.get(&a).and_then(|m| m.get(&b)) {
        return v;
    }
    g.data.faction_reactions.get(&a).and_then(|l| l.iter().find(|x| x.0 == b)).map(|x| x.1).unwrap_or(0)
}

pub fn get_faction_combat_reaction(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    Ok(Value::Num(reaction(g, a.form(0)?, a.form(1)?) as f64))
}

/// How actor `r` stands towards actor `o` by their factions (0 neutral, 1 enemy, 2 ally,
/// 3 friend) [documented: GECK wiki GetFactionRelation]. One direction: the reactions
/// of `r`'s factions towards `o`'s, which need not be those of `o`'s towards `r`'s
/// [documented: GECK wiki SetEnemy, each faction has its own reaction list;
/// docs/spec-notes/factions.md §4]. How several factions combine: a friend or an ally
/// first, then an enemy, as allies and friends "cannot attack each other under any
/// circumstances" [documented: GECK wiki GetFactionCombatReaction], so one shared
/// friendly pair keeps the peace (Vulpes Inculta, in VCaesarsLegionFaction and
/// ArmorNCRFactionNVEnemy, among the Legion, #2560); friend before ally is a
/// [hypothesis]. The player counts as an enemy of an actor in a faction the player is
/// a crime enemy of, before anything else.
pub fn faction_relation(g: &Game, r: FormId, o: FormId) -> i64 {
    // Kept while asked again and again ([`FactionKeep`]).
    let kept = keep(g).map(|k| &k.relations);
    if let Some(v) = kept.and_then(|k| k.read().ok()?.get(&(r, o)).copied()) {
        return v;
    }
    let v = factions_relation(g, &factions_of(g, r), o);
    if let Some(mut k) = kept.and_then(|k| k.write().ok()) {
        k.insert((r, o), v);
    }
    v
}

/// Factions, faction reactions or the player's faction enemies changed: the kept faction
/// relations go ([`FactionKeep`]).
pub(crate) fn forget_relations(g: &Game) {
    for keep in keeps(g) {
        if let Ok(mut k) = keep.relations.write() {
            k.clear();
        }
    }
}

/// How the factions `mine` together stand towards actor `o`, combined as
/// [`faction_relation`] does (0 neutral, 1 enemy, 2 ally, 3 friend). A media location
/// controller's conditional faction asks this of the player (docs/spec-notes/audio.md §13).
pub fn factions_relation(g: &Game, mine: &[FormId], o: FormId) -> i64 {
    factions_between(g, mine, o, &factions_of(g, o))
}

/// Whether the player is an enemy of an actor in the factions `mine` by a faction enemy
/// flag (SetPCEnemyOfFaction, a witnessed crime): one of `mine` is flagged, and none of
/// the others is the player's ally or friend (`theirs`: the player's factions). A
/// faction disguise makes such a faction the player's friend: the NCR armour's OnEquip
/// makes PlayerFaction and ArmorNCRFactionNV friends, every NCR soldier is in
/// ArmorNCRFactionNV too, and the disguise pulse takes sniffers near the player out of
/// it, by its notes so that they may turn hostile to a player they think little of
/// [observed: FalloutNV.esm's disguise scripts; hypothesis: the friendship outranks the
/// flag, docs/spec-notes/disguises.md §3]. The flagged faction's own friendship does
/// not count: a crime among the player's friends stays a crime.
pub fn player_enemy_of_factions(g: &Game, mine: &[FormId], theirs: &[FormId]) -> bool {
    let enemy_of = &g.player.enemy_of;
    if enemy_of.is_empty() || !mine.iter().any(|f| enemy_of.contains(f)) {
        return false;
    }
    !mine.iter().filter(|f| !enemy_of.contains(f)).any(|&x| theirs.iter().any(|&y| matches!(reaction(g, x, y), 2 | 3)))
}

/// [`factions_relation`] with `o`'s factions given (`theirs`): reads no kept factions, so
/// it takes no lock (a detection round's pairs on several threads, #1799).
pub fn factions_between(g: &Game, mine: &[FormId], o: FormId, theirs: &[FormId]) -> i64 {
    if o == FormId::PLAYER_REF && player_enemy_of_factions(g, mine, theirs) {
        return 1;
    }
    let all: Vec<i64> = mine.iter().flat_map(|&x| theirs.iter().map(move |&y| (x, y))).map(|(x, y)| reaction(g, x, y)).collect();
    if all.contains(&3) {
        3
    } else if all.contains(&2) {
        2
    } else if all.contains(&1) {
        1
    } else {
        0
    }
}

/// Whether a mine owned by `owner` spares `actor`: its owner itself; the player's
/// teammates for a mine of the player's; for a faction's mine, a member of that faction
/// or of a faction it counts as an ally or a friend [owner 2026-10-19: "anyone who is not
/// part of or ally to the faction that the mine owner is will activate it"; documented:
/// GECK wiki Traps, a faction's members do not set off its mines; hypothesis: which way
/// the reaction is read, the owning faction's towards the actor's]. A mine without an
/// owner spares nobody.
pub fn mine_spares(g: &Game, owner: Option<FormId>, actor: FormId) -> bool {
    let Some(owner) = owner else { return false };
    if owner == actor {
        return true;
    }
    if owner == FormId::PLAYER_REF {
        return g.is_teammate(actor);
    }
    let theirs = factions_of(g, actor);
    theirs.contains(&owner) || theirs.iter().any(|&f| matches!(reaction(g, owner, f), 2 | 3))
}

/// `actor.GetFactionRelation other` ([`faction_relation`]).
pub fn get_faction_relation(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    Ok(Value::Num(faction_relation(g, r, a.form(0)?) as f64))
}

/// `SetEnemy f1 f2 [flag12 flag21]`: each direction becomes enemy (flag 0, the default)
/// or neutral (1) [documented: catalogue notes].
pub fn set_enemy(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let (f1, f2) = (a.form(0)?, a.form(1)?);
    g.showoff.temp_reactions.remove(&(f1, f2));
    g.showoff.temp_reactions.remove(&(f2, f1));
    let v = |x: f64| if x != 0.0 { 0 } else { 1 };
    let (r12, r21) = (v(a.num_or(2, 0.0)), v(a.num_or(3, 0.0)));
    g.forms.faction_reactions.entry(f1).or_default().insert(f2, r12);
    g.forms.faction_reactions.entry(f2).or_default().insert(f1, r21);
    forget_relations(g);
    none()
}

/// `SetAlly f1 f2 [flag12 flag21]`: each direction becomes ally (flag 0, the default)
/// or friend (1) [documented: catalogue notes].
pub fn set_ally(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let (f1, f2) = (a.form(0)?, a.form(1)?);
    g.showoff.temp_reactions.remove(&(f1, f2));
    g.showoff.temp_reactions.remove(&(f2, f1));
    let v = |x: f64| if x != 0.0 { 3 } else { 2 };
    let (r12, r21) = (v(a.num_or(2, 0.0)), v(a.num_or(3, 0.0)));
    g.forms.faction_reactions.entry(f1).or_default().insert(f2, r12);
    g.forms.faction_reactions.entry(f2).or_default().insert(f1, r21);
    forget_relations(g);
    none()
}

/// The actor leaves every faction it is in [documented: catalogue summary].
pub fn remove_from_all_factions(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    for f in factions_of(g, r) {
        g.extra.entry(r).or_default().factions.insert(f, -1);
    }
    forget_factions(g, r);
    none()
}

/// The caller's rank minus the target's in a faction [documented: catalogue summary];
/// a non-member's rank counts as −1 [hypothesis].
pub fn get_faction_rank_difference(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let f = a.form(0)?;
    let t = a.form(1)?;
    Ok(Value::Num((faction_rank(g, r, f) - faction_rank(g, t, f)) as f64))
}

/// `SetPCEnemyOfFaction faction flag` [documented: catalogue summary]. A flag a script
/// sets stays until a script clears it; one a crime set ends with its crimes
/// (`Game::expire_crimes`) [design, #2467].
pub fn set_pc_enemy_of_faction(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let f = a.form(0)?;
    g.script_enemy_of(f, a.num_or(1, 1.0) != 0.0);
    none()
}

pub fn get_pc_enemy_of_faction(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    g.expire_crimes();
    yes(g.player.enemy_of.contains(&a.form(0)?))
}

/// Resolve a faction parameter passed as a FormID (CTDA / bytecode / ref expression) or
/// editorID string (source calls where `functions.json` types `FactionID` as `string`),
/// requiring a `FACT` record [design].
fn faction_arg(g: &Game, a: &Args, i: usize) -> Result<FormId, String> {
    let id = if let Some(f) = a.peek_form(i) {
        f
    } else if let Some(edid) = a.peek_str(i) {
        g.data.by_edid(edid).ok_or_else(|| format!("unknown faction {edid:?}"))?
    } else {
        return Err(format!("parameter {} is not a faction", i + 1));
    };
    if g.data.sig(id).as_ref() != Some(b"FACT") {
        return Err(format!("parameter {} ({id}) is not a faction", i + 1));
    }
    Ok(id)
}

/// `GetPCExpelled faction`: 1 when the player has been expelled from `faction`
/// [documented: GECK wiki GetPCExpelled; `schemas/script/functions.json`, 0x10C1 / CTDA 193].
pub fn get_pc_expelled(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    yes(g.player.expelled_from.contains(&faction_arg(g, a, 0)?))
}

/// `SetPCExpelled faction flag`: expels the player from `faction` and sets the expelled
/// flag when `flag != 0`, or clears the flag when `flag == 0` without re-adding membership
/// [documented: GECK wiki SetPCExpelled; hypothesis: interior faction-owned cell eviction
/// is not performed in core rules].
pub(crate) fn set_pc_expelled(g: &mut Game, a: &Args) -> Result<(), String> {
    let f = faction_arg(g, a, 0)?;
    let expel = a.int(1)? != 0;
    if expel {
        g.player.expelled_from.insert(f);
        if faction_rank(g, FormId::PLAYER_REF, f) >= 0 {
            g.extra.entry(FormId::PLAYER_REF).or_default().factions.insert(f, -1);
            forget_factions(g, FormId::PLAYER_REF);
        }
    } else {
        g.player.expelled_from.remove(&f);
    }
    Ok(())
}

pub fn clear_faction_player_enemy_flag(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    g.clear_enemy_of(a.form(0)?);
    forget_relations(g);
    none()
}

/// 1 when the player is a crime enemy of any faction the caller is in [documented:
/// catalogue summary].
pub fn get_actor_faction_player_enemy(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    g.expire_crimes();
    yes(factions_of(g, r).iter().any(|f| g.player.enemy_of.contains(f)))
}

flag!(ignore_crime, get_ignore_crime, ignore_crime);
flag!(set_ignore_friendly_hits, get_ignore_friendly_hits, ignore_friendly_hits);

/// `actor.SetLevel level [levelToPC min max]`: the player's level, or an actor's
/// (with the level-to-player flag the level is an offset from the player's, kept within
/// min and max when given) [documented: catalogue summary and notes]. On the player
/// only the number changes, not the stats [documented: catalogue notes, "alters the
/// level number but no stats"]. On another actor only the level changes too: its
/// inventory is not rolled again [observed: capture ac1, 2026-10-18: three Stimpaks
/// given to Easy Pete were still carried after SetLevel 7, and his item count stayed 8
/// after a second SetLevel 7; the catalogue's note that it "regenerates the inventory"
/// did not show]. With the flag, the offset and bounds as the original: player level 1,
/// `SetLevel 2 1 5 20` gives 5 [observed: ac1, Joe Cobb].
pub fn set_level(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject_or_player();
    let mut level = a.int(0)?;
    if r == FormId::PLAYER_REF {
        let now = g.player_level();
        if crate::revision::keeps(ctx, r, || format!("the level ({now})")) {
            return none();
        }
        g.player.level = Some(level);
        g.sync_player_level();
        return none();
    }
    if a.num_or(1, 0.0) != 0.0 {
        level += g.player.level.unwrap_or(g.world.config().player_level as i32);
        if let Ok(min) = a.int(2) {
            level = level.max(min);
        }
        if let Some(max) = a.int(3).ok().filter(|&m| m > 0) {
            level = level.min(max);
        }
    }
    g.extra.entry(r).or_default().level = Some(level);
    none()
}

/// How many actors are the player's teammates now [documented: catalogue summary].
pub fn get_player_teammate_count(g: &mut Game, _ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let n = g.extra.iter().filter(|(_, e)| e.teammate && !e.dead).count();
    Ok(Value::Num(n as f64))
}

/// `ResetHealth`: the actor's health is restored and its crippled limbs cured: the
/// damage of Health and of the limb conditions goes [documented: GECK wiki ResetHealth].
pub fn reset_health(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    if let Some(e) = g.extra.get_mut(&r) {
        e.av_damage.remove(&crate::combat::HEALTH);
        for code in crate::combat::LIMB_CONDITIONS {
            e.av_damage.remove(&code);
        }
    }
    none()
}

/// Recorded on the actor and made its current package (the package runs in Layer 6).
pub fn add_script_package(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let p = a.form(0)?;
    // An actor holds one script package: a second call replaces the first
    // [documented: GECK wiki AddScriptPackage notes, actor-behaviour.md §1.4; observed:
    // od_08_script_packages, owner-decision-captures.md item 8].
    let e = g.extra.entry(r).or_default();
    // The package added again while it is still the current one and its goal was met
    // begins afresh: its goal is forgotten and the AI takes it up anew
    // (`RefExtra::ai_reset`), as for a package it did not have [hypothesis: the GECK
    // wiki's "takes precedence over all other packages until it is done" read for each
    // call; the Lucky 38 elevator's OnActivate adds its Dialogue package
    // Lucky38VictorElevatorTalkToPlayer on every talk and waits for the conversation
    // (delaycounter 1 until MenuMode 1009), so a second talk after Stay Here never
    // opened one and the floor box never came back, issue #2424]. A call while it
    // still runs changes nothing, so a script adding it every frame does not restart it.
    if e.current_package == Some(p) && e.package_goal_met {
        e.package_goal_met = false;
        e.ai_reset = true;
    }
    e.script_package = Some(p);
    if g.runs_packages() {
        g.set_package(r, Some(p));
    } else {
        g.extra.entry(r).or_default().current_package = Some(p);
    }
    none()
}

/// The script package is dropped and the actor chooses from its own list again: with
/// one slot, a package added before the last AddScriptPackage does not come back
/// [observed: od_08_script_packages, owner-decision-captures.md item 8].
pub fn remove_script_package(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    if let Some(e) = g.extra.get_mut(&r) {
        e.script_package = None;
        if !g.runs_packages() {
            g.extra.entry(r).or_default().current_package = None;
        } else {
            g.evaluate_package(r);
        }
    }
    none()
}

/// `EvaluatePackage`: see [`Game::evaluate_package`].
pub fn evaluate_package(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    g.evaluate_package(r);
    none()
}

/// 1 when the actor's current package is the given one [documented: catalogue summary].
pub fn get_is_current_package(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let p = a.form(0)?;
    yes(g.current_package(r) == Some(p))
}

/// `GetActorValue Health` over `GetBaseActorValue Health`, 1.0 when the base is 0, with
/// no upper bound (effects raising Health give more than 1) [documented: GECK wiki
/// GetHealthPercentage notes, the code the function is equivalent to]; a dead actor is at
/// 0 [hypothesis: KillActor leaves its health as it was here].
pub fn get_health_percentage(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    if g.extra.get(&r).is_some_and(|e| e.dead) {
        return Ok(Value::Num(0.0));
    }
    let base = base(g, r, crate::combat::HEALTH)?;
    let now = actor_value(g, r, crate::combat::HEALTH)?;
    Ok(Value::Num(if base != 0.0 { (now / base).max(0.0) } else { 1.0 }))
}

/// `actor.GetFatiguePercentage`: the actor's current Fatigue divided by its base
/// [documented: GECK wiki GetFatiguePercentage]; 1 when the base is 0, as
/// [`get_health_percentage`]. Not held at 0: fatigue below 0 knocks an actor out
/// (`crate::unconscious`), and the ratio then reads below 0 [hypothesis].
pub fn get_fatigue_percentage(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let base = base(g, r, crate::actor_values::FATIGUE)?;
    let now = actor_value(g, r, crate::actor_values::FATIGUE)?;
    Ok(Value::Num(if base != 0.0 { now / base } else { 1.0 }))
}

/// `actor.GetTimeDead`: hours since the actor died; 0 when it is not dead, is disabled,
/// or was disabled after dying [documented: GECK wiki GetTimeDead]. By GameDaysPassed
/// at its death (`RefExtra::died_day`); 0 for a corpse laid out at the start of the game
/// [hypothesis: nobody killed it, so no time was kept].
pub fn get_time_dead(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let Some(day) = g.extra.get(&r).filter(|e| e.dead).and_then(|e| e.died_day) else { return Ok(Value::Num(0.0)) };
    if !g.world.get(r).is_ok_and(|s| s.enabled()) {
        return Ok(Value::Num(0.0));
    }
    let now = g.game_days().unwrap_or(day);
    Ok(Value::Num(((now - day) * 24.0).max(0.0)))
}

/// `GetLastHitCritical`: 1 when the last hit the actor took was a critical [documented:
/// GECK wiki GetLastHitCritical, "The information is available in the ON HIT script
/// block"]; the last hit is kept after the block too (`RefExtra::last_hit`) [hypothesis].
pub fn get_last_hit_critical(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    yes(g.extra.get(&r).and_then(|e| e.last_hit).is_some_and(|h| h.critical))
}

/// `actor.SameFaction other`: 1 when both are members of at least one faction in common
/// [documented: GECK wiki SameFaction]; members as GetInFaction counts them (rank 0 or
/// more, [`factions_of`]).
pub fn same_faction(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let Some(other) = a.opt_form(0) else { return yes(false) };
    let mine = factions_of(g, r);
    yes(factions_of(g, other).iter().any(|f| mine.contains(f)))
}

/// `actor.SameSex other`: 1 when the calling actor is the same sex as `other`
/// [documented: GECK wiki SameSex]; each actor's sex now, SexChange's included.
pub fn same_sex(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let Some(other) = a.opt_form(0) else { return yes(false) };
    yes(is_female(g, r) == is_female(g, other))
}

/// `actor.SameSexAsPC`: 1 when the calling actor is the same sex as the player
/// [documented: GECK wiki SameSexAsPC, "Returns 1 if the calling actor is the same sex as
/// the player"]; as [`same_sex`] with the player as the other actor (#3787).
pub fn same_sex_as_pc(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    yes(is_female(g, r) == is_female(g, FormId::PLAYER_REF))
}

/// The disposition HasFriendDisposition counts as a friend's. The GECK wiki names a
/// "friend disposition game setting" without naming it, and no game setting in the
/// shipped master is one (`fFightFriendDispBase`, -25, is a fight base) [observed:
/// FalloutNV.esm GMSTs with "Friend" or "Disp" in their names]; 70 is NVForever's
/// [hypothesis: observer check 292 measures it].
pub const FRIEND_DISPOSITION: f64 = 70.0;

/// `actor.HasFriendDisposition`: 1 when the actor's disposition towards the player is at
/// or above the friend level [documented: GECK wiki HasFriendDisposition, "equal or
/// greater to the friend disposition game setting"; hypothesis: towards the player, as
/// FriendDispositionBoost's "friend level with the player"], [`FRIEND_DISPOSITION`].
pub fn has_friend_disposition(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    yes(disposition(g, r, FormId::PLAYER_REF) >= FRIEND_DISPOSITION)
}

/// `ref.GetIsObjectType type`: 1 when the calling object, a reference's base or the
/// form itself, is of form type `type` [documented: GECK wiki GetIsObjectType, "Returns
/// 1 if the object is the specified form type"], by the form type codes
/// (`fnv_script::vm::form_type_code`).
pub fn get_is_object_type(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let code = a.code(0)?;
    let form = base_of(g, r).unwrap_or(r);
    yes(g.world.forms.sig(form).and_then(|s| fnv_script::vm::form_type_code(&s)) == Some(code))
}

/// IsFacingUp (bipeds) and IsLeftUp (quadrupeds), the idle manager's checks of a knocked
/// down actor's side [documented: GECK wiki]: 1 for a standing actor [observed, function
/// probes `fp1_queries` 2026-10-17: both read 1 on Ringo standing in the gas station];
/// for a knocked-out one the side the engine saw its body come to rest on, 1 until then
/// (docs/spec-notes/unconscious.md §2.8) [hypothesis: IsLeftUp reads the same side].
pub fn knocked_side_up(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    yes(g.knockout(r).and_then(|k| k.facing_up).unwrap_or(true))
}

/// `GetOwnerLastTarget`: "the most recent target of whoever owns the calling reference"
/// [documented: catalogue summary]: the actor the calling item's holder (an item
/// script's instance), or the calling actor, last hit (`RefExtra::last_target`), else
/// its combat target [hypothesis: the hit target is the target meant; the weapon scripts
/// that call it run in OnHit, docs/spec-notes/unconscious.md §1].
pub fn get_owner_last_target(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let owner = match ctx.inst {
        Some(crate::game::InstKey::Item { holder, .. }) => Some(holder),
        _ => ctx.on.or(ctx.self_ref),
    };
    let Some(owner) = owner else { return Ok(Value::NONE) };
    let target = g.extra.get(&owner).and_then(|e| e.last_target).or_else(|| g.awareness.get(owner).and_then(|a| a.combat_target()).map(|t| t.0));
    Ok(target.map_or(Value::NONE, |t| Value::Ref(t.0)))
}

/// `actor.GetDisposition target`: the actor's record's Disposition Base (ACBS) plus
/// `fDispTargetCharMult` × (the target's Charisma − 5), kept within 0 to 100; a value
/// SetDisposition gave for that target instead [observed, function probes 2026-10-17
/// (`fp1_queries`, `fp5_actions`): every Goodsprings resident (base 35) read 35 towards a
/// player of Charisma 5, 85 at Charisma 10 and 0 at Charisma 1; Ringo towards Sunny Smiles
/// (Charisma 4) 25; Karma ±1000 changed nothing; after `SetDisposition player 20` it read
/// 20. The GMST is 10 in FalloutNV.esm and the slope observed is 10: that the GMST is the
/// factor, the anchor 5 and the upper bound 100 are hypothesis]. The wiki's "all
/// modifiers" (factions, crimes, the actor's own Charisma via `fDispActorCharMult`) are
/// not applied: the actor's Charisma (3 to 6 among those probed) made no difference.
pub fn get_disposition(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let actor = ctx.subject()?;
    let target = a.form(0)?;
    Ok(Value::Num(disposition(g, actor, target)))
}

/// `actor`'s disposition towards `target`, as GetDisposition reads it.
fn disposition(g: &mut Game, actor: FormId, target: FormId) -> f64 {
    if let Some(v) = g.extra.get(&actor).and_then(|x| x.dispositions.get(&target)) {
        return *v;
    }
    let base = g.base_data_def(actor).map_or(0, |d| d.disposition_base) as f64;
    let cha = crate::actor_values::capped(crate::actor_values::CHARISMA, actor_value(g, target, crate::actor_values::CHARISMA).unwrap_or(5.0));
    let mult = crate::combat::setting(g, "fDispTargetCharMult", 10.0);
    (base + mult * (cha - 5.0)).clamp(0.0, 100.0).round()
}

/// `actor.SetDisposition target value`: GetDisposition towards `target` reads `value`
/// from now on [documented: GECK wiki SetDisposition; observed: 20 after
/// `SetDisposition player 20`]. Kept in the saved state.
pub fn set_disposition(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let actor = ctx.subject()?;
    let target = a.form(0)?;
    let v = a.num(1)?;
    g.extra.entry(actor).or_default().dispositions.insert(target, v);
    none()
}

// ---- Cannibal and Mister Sandman (`crate::activate_choices`) ------------------------

/// `victim.StartCannibal eater`: the calling actor is eaten by `eater` (the player when
/// not given) [documented: catalogue summary].
pub fn start_cannibal(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let victim = ctx.subject()?;
    let eater = a.opt_form(0).unwrap_or(FormId::PLAYER_REF);
    g.start_cannibal(victim, eater);
    none()
}

/// `victim.StartMisterSandman killer`: the sleeping calling actor is killed by `killer`
/// at the act's end [observed: Mister Sandman's entry script `StartMisterSandman Player`,
/// capture ac5c]. A victim not asleep is refused (the original crashed,
/// `crate::activate_choices`).
pub fn start_mister_sandman(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let victim = ctx.subject()?;
    let killer = a.opt_form(0).unwrap_or(FormId::PLAYER_REF);
    if !g.start_mister_sandman(victim, killer) {
        return Err(format!("{} is not asleep: StartMisterSandman refused", g.data.name(victim)));
    }
    none()
}

/// 1 while the calling actor is in the cannibalism animation [documented: catalogue
/// summary].
pub fn get_cannibal(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    yes(g.in_perk_act(r, crate::activate_choices::Act::Cannibal))
}

/// 1 while the calling actor carries out a Mister Sandman kill [documented: catalogue
/// summary].
pub fn get_sandman(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    yes(g.in_perk_act(r, crate::activate_choices::Act::Sandman))
}

/// 1 when the player has eaten the calling actor [documented: catalogue summary]; it
/// stays so (saved with the reference) [hypothesis: the wiki leaves it untested].
pub fn has_been_eaten(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    yes(g.extra.get(&r).is_some_and(|e| e.eaten))
}
