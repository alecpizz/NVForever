//! Magic effects as state: spells added to actors, items cast or consumed, the
//! effects active on each actor, and their effect scripts (ScriptEffectStart /
//! Update / Finish). The rules: [`Game::apply_magic_item`] and docs/spec-notes/
//! scripts.md §7.6. What an effect does to actor values: `crate::aid`
//! (docs/spec-notes/aid-items.md); shaders and sounds are Layer 6.

use super::{none, yes, Args, Ctx};
use crate::game::{Game, InstKey};
use fnv_script::vm::Value;
use fnv_sim::FormId;

fn magic_item(g: &Game, a: &Args) -> Result<fnv_sim::FormId, String> {
    let s = a.form(0)?;
    if g.data.effect_items.contains_key(&g.unclone(s)) {
        Ok(s)
    } else {
        Err(format!("{} is not a spell, enchantment or ingestible", g.data.name(s)))
    }
}

/// `actor.AddSpell spell`: the spell's effects apply at once and last until removed
/// [documented: catalogue summary of AddSpell; hypothesis: every effect of an added
/// spell lasts, as the shipped uses are abilities and addictions]. Adding a spell the
/// actor already has does nothing [hypothesis].
pub fn add_spell(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let s = magic_item(g, a)?;
    g.sync_actor_effects(r);
    if let Some(x) = g.extra.get_mut(&r) {
        x.dispelled.retain(|&d| d != s);
    }
    if r == FormId::PLAYER_REF && g.story_counts(ctx) && !g.extra.get(&r).is_some_and(|x| x.spells.contains(&s)) {
        g.coop_story.record_spell(s, true);
        tracing::info!("co-op story: {} gave the player spell {}", ctx.unit, g.data.name(s));
    }
    g.add_spell(r, s);
    // An ability added to an actor out of the loaded area waits for it, and a listed one
    // a Dispel took off comes back (`crate::abilities`) [hypothesis].
    g.sync_actor_effects(r);
    none()
}

/// `actor.RemoveSpell spell`: the spell is removed with its active effects (their
/// finish blocks run) [hypothesis], an authored ability too, which is not given again
/// [observed: capture abilities-1, Victor's SecuritronDT gone (DT 20 to 0) and his
/// RadImmunity kept; AddSpell put it back]. A worn item's enchantment is no spell of
/// the actor and stays while the item is worn [hypothesis; aid-items.md §13].
///
/// Called with no reference in a script that has none (a quest stage result), it acts on
/// the player: the doctors' addiction cure (VDoctors / Doctors stage 20) cleared the
/// player's Jet withdrawal [observed: o5_03, 2026-10-31, observer check 42; issue
/// #1110]. The same in a quest script [hypothesis: the player is the default reference,
/// script-language.md §1.2].
pub fn remove_spell(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject_or_player();
    let s = a.form(0)?;
    g.sync_actor_effects(r);
    if r == FormId::PLAYER_REF && g.story_counts(ctx) && g.extra.get(&r).is_some_and(|x| x.spells.contains(&s)) {
        g.coop_story.record_spell(s, false);
        tracing::info!("co-op story: {} took spell {} from the player", ctx.unit, g.data.name(s));
    }
    if let Some(e) = g.extra.get_mut(&r) {
        e.spells.retain(|&x| x != s);
        e.dispelled.retain(|&x| x != s);
    }
    g.dispel(r, ctx.inst, |e| e.source == s && e.worn.is_none());
    none()
}

/// `ref.CastImmediateOnSelf spell`: the spell's effects apply to the caller at once,
/// with their durations [documented: catalogue summary; hypothesis: no casting
/// animation or projectile is involved], and its area effects to the actors around it
/// (`crate::area_effects`).
pub fn cast_immediate_on_self(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let s = magic_item(g, a)?;
    g.apply_magic_item(r, s, Some(r), false);
    g.spread_area(r, s, Some(r), false);
    none()
}

/// `actor.Dispel spell`: the effects of that spell end [documented: catalogue summary];
/// see [`Game::dispel`] for a script dispelling its own effect. An ability's end too,
/// for as long as the actor stays loaded, and a worn item's enchantment's until the item
/// is equipped again [observed: capture abilities-1, Victor's SecuritronDT (DT 20 to 0)
/// and the gambler hat's enchantment (Perception 6 to 5)]; an addiction's do not
/// [hypothesis].
pub fn dispel(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let s = a.form(0)?;
    g.sync_actor_effects(r);
    let ability = g.effects.iter().any(|e| e.target == r && e.source == s && g.is_ability_effect(e));
    let worn: Vec<_> = g.effects.iter().filter(|e| e.target == r && e.source == s).filter_map(|e| e.worn).collect();
    if ability || !worn.is_empty() {
        let mut recorded_dispel = false;
        let x = g.extra.entry(r).or_default();
        if ability && !x.dispelled.contains(&s) {
            x.dispelled.push(s);
            recorded_dispel = true;
        }
        for i in worn {
            if !x.worn_dispelled.contains(&i) {
                x.worn_dispelled.push(i);
            }
        }
        if recorded_dispel {
            g.note_dispelled_ability(r);
        }
    }
    let drop_ids: Vec<u32> = g
        .effects
        .iter()
        .filter(|e| e.target == r && e.source == s && (!e.permanent || e.worn.is_some() || g.is_ability_effect(e)))
        .map(|e| e.id)
        .collect();
    g.dispel(r, ctx.inst, |e| drop_ids.contains(&e.id));
    none()
}

/// Every effect on the actor ends except those of added spells (abilities, authored or
/// not) and of worn enchantments [observed: capture abilities-1, Victor's RadImmunity
/// and the player's added PerkToughness stayed; hypothesis for worn enchantments].
pub fn dispel_all_spells(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    g.dispel(r, ctx.inst, |e| !e.permanent);
    none()
}

/// 1 while an effect of the given magic item is on the actor [documented: catalogue
/// summary; GECK wiki IsSpellTarget: an actor effect (abilities included), an object
/// effect or an ingestible], its authored abilities and worn enchantments included
/// (`crate::abilities`, only while the actor is loaded), an effect whose conditions fail
/// too [observed: capture abilities-1, the Four Eyes glasses worn without the trait].
pub fn is_spell_target(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let s = a.form(0)?;
    g.sync_actor_effects(r);
    yes(g.effects.iter().any(|e| e.target == r && e.source == s))
}

/// 1 while an effect with the given base effect is on the reference [documented:
/// catalogue summary], one whose conditions fail included [observed: capture
/// abilities-1, the Four Eyes glasses worn without the trait: HasMagicEffect
/// IncreasePerception 1, Perception unchanged].
pub fn has_magic_effect(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let m = a.form(0)?;
    g.sync_actor_effects(r);
    yes(g.effects.iter().any(|e| e.target == r && e.effect == m))
}

/// Seconds since the last effect update inside ScriptEffectUpdate, 0 elsewhere
/// [documented: GECK wiki ScriptEffectElapsedSeconds].
pub fn script_effect_elapsed_seconds(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    Ok(Value::Num(if matches!(ctx.inst, Some(InstKey::Effect { .. })) { g.effect_elapsed } else { 0.0 }))
}

/// `actor.GetSpellUsageNum ingestible`: the uses of the ingestible while its usage
/// monitor is active, times the monitor's magnitude [documented: GECK wiki
/// GetSpellUsageNum; aid-items.md §5.1].
pub fn get_spell_usage_num(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    Ok(Value::Num(g.spell_usage(r, a.form(0)?)))
}
