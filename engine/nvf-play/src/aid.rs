//! Aid items the player uses from the inventory (docs/spec-notes/aid-items.md §7): the
//! menu takes the item and plays its sound (`crate::menu`, `crate::ui_sounds`); here the
//! script runtime uses it (`fnv_game::Game::ingest`: effects with potency, usage
//! monitor, addiction) and the log tells what it did to the player's actor values.
//!
//! Log lines: `aid: the player used <item> (potency p): <changes>; <value> a → b, …`
//! when an item is used (with `; perks: …` when perk entry points changed the use, and
//! Carry Weight among the values when Strength is), `aid: <item>'s effects on the
//! player ended: <value> v, …` when the last effect of an item ends, `aid: the player is
//! addicted to <item> (<spell>)`, `aid: <spell> cured`, `needs: <value> <level>: <stage
//! effect>` (or `: no stage`) when the player's stage of rads or of a Hardcore need
//! changes, `wait: hour <n> passed for the player (<seconds> s of effects): …` for each
//! hour waited or slept (aid-items.md §6, §11), and at exit
//! `aid summary: …`.

use fnv_util::words::{tr, trf};
use crate::script::{with_world, Runtime, ScriptState};
use crate::sim::SimState;
use bevy::prelude::*;
use fnv_game::aid::{ChangeKind, Ingested, ValueChange};
use fnv_game::functions::{actor_value, AV_NAMES};
use fnv_game::trace::EventKind;
use fnv_sim::FormId;
use std::collections::{BTreeMap, BTreeSet};

const P: FormId = FormId::PLAYER_REF;
const STRENGTH: u16 = 5;
const CARRY_WEIGHT: u16 = 13;
/// Perception Condition (the head) to Right Mobility Condition (the right leg).
const LIMBS: std::ops::RangeInclusive<u16> = 25..=30;

/// A limb by its condition code, for the log.
pub fn limb_name(code: u16) -> &'static str {
    match code {
        25 => "head",
        26 => "torso",
        27 => "left arm",
        28 => "right arm",
        29 => "left leg",
        30 => "right leg",
        _ => "brain",
    }
}

/// What the player's aid items did this run.
#[derive(Debug, Default, Clone)]
pub struct AidStats {
    /// Items used, by name.
    pub used: BTreeMap<String, u32>,
    /// Effects the uses applied: none held inactive at the use, none merely extended.
    pub effects_started: u32,
    /// Those of [`AidStats::effects_started`] that have ended.
    pub effects_ended: u32,
    /// The ids behind `effects_started` that have not ended yet, so the two numbers count
    /// the same effects: every effect's removal raises `EffectFinished`, a held one's too
    /// (#1494).
    started_ids: BTreeSet<u32>,
    pub addicted: Vec<String>,
    pub cured: u32,
    /// Changes of the player's stage of rads or of a need.
    pub stages: u32,
    /// Hours waited or slept that passed for the player's effects and needs.
    pub hours: u32,
    /// Actor values the items touched (for the summary).
    pub codes: BTreeSet<u16>,
    /// Items with effects on the player that last (modifiers, by the second), and the
    /// values they change: logged when the last of them ends.
    pub lasting: BTreeMap<FormId, BTreeSet<u16>>,
}

impl AidStats {
    /// A use applied these effects (`Ingested::effects`).
    fn count_started(&mut self, ids: &[u32]) {
        self.effects_started += ids.len() as u32;
        self.started_ids.extend(ids);
    }

    /// An effect is gone: it counts as ended only if it was counted as started. Whether
    /// it was suspended since does not matter, it took hold.
    fn count_finished(&mut self, id: u32) {
        if self.started_ids.remove(&id) {
            self.effects_ended += 1;
        }
    }
}

fn av_name(code: u16) -> &'static str {
    AV_NAMES.get(code as usize).copied().unwrap_or("?")
}

fn value(rt: &mut Runtime, code: u16) -> f64 {
    actor_value(&mut rt.game, P, code).unwrap_or(0.0)
}

fn change_text(c: &ValueChange, duration: f64) -> String {
    let how = match c.kind {
        ChangeKind::Modifier => format!("while it lasts ({duration:.0} s)"),
        ChangeKind::Once => "at once".to_string(),
        ChangeKind::PerSecond => format!("a second for {duration:.0} s"),
    };
    if c.parts && fnv_game::aid::LIMB_CONDITIONS.contains(&c.code) {
        if let Some(l) = c.limb {
            return format!("{} {:+.1} {how}", av_name(l), c.amount.max(100.0));
        }
    }
    let limbs = if c.parts { ", limbs too" } else { "" };
    format!("{} {:+.1} {how}{limbs}", av_name(c.code), c.amount)
}

/// The player used the poison `item` (already taken from the inventory by the menu): it
/// goes on the weapon in hand, or asks first, or there is no weapon for it; a corner
/// message says which, and the item goes back unless it was put on
/// (`fnv_game::weapon_poison`). Call with the world swapped in.
pub fn use_poison(rt: &mut Runtime, item: FormId) {
    use fnv_game::weapon_poison::PoisonUse;
    let name = |rt: &Runtime, f: FormId| rt.game.data.name(f);
    let used = rt.game.use_poison(item);
    let text = match used {
        PoisonUse::Applied { weapon } => trf("{weapon} poisoned with {item}", &[("weapon", &name(rt, weapon)), ("item", &name(rt, item))]),
        // [design: our own words]
        PoisonUse::Asks { weapon } => trf("Use {item} again to poison your {weapon}", &[("item", &name(rt, item)), ("weapon", &name(rt, weapon))]),
        // The game's own words [observed: FalloutNV.esm sPoisonNoWeaponMessage], ours without them.
        PoisonUse::NoWeapon => match rt.game.world.setting_value("sPoisonNoWeaponMessage") {
            Some(fnv_sim::settings::Setting::Str(s)) if !s.trim().is_empty() => s.clone(),
            _ => tr("Equip a weapon that can be poisoned first.").into(),
        },
    };
    if !matches!(used, PoisonUse::Applied { .. }) {
        if let Err(e) = rt.game.world.add_item(P, item, 1) {
            warn!("aid: could not give {} back: {e}", name(rt, item));
        }
    }
    info!("aid: poison {}: {text}", name(rt, item));
    rt.game.emit(fnv_game::trace::EventKind::NoteShown { text, icon: None, seconds: 2.0 });
}

/// The player uses `item` (already taken from the inventory by the menu), on one limb
/// (its condition code) when the Pip-Boy's CND page aims it (`Game::ingest_on`). Call
/// with the world swapped in (`with_world`).
pub fn use_item(rt: &mut Runtime, item: FormId, limb: Option<u16>) -> Ingested {
    let data = rt.game.data;
    // The values the item's effects may touch, before.
    let changes: Vec<ValueChange> = data
        .effect_items
        .get(&item)
        .into_iter()
        .flatten()
        .filter_map(|e| data.magic_effects.get(&e.effect).and_then(|m| fnv_game::aid::value_change(m, e, 1.0, false)))
        .collect();
    let mut codes: BTreeSet<u16> = changes.iter().map(|c| c.code).collect();
    // The six limbs the Pip-Boy shows, when an effect restores them too.
    if changes.iter().any(|c| c.parts) {
        codes.extend(LIMBS);
    }
    // Strength carries the carry weight with it (aid-items.md §12).
    if codes.contains(&STRENGTH) {
        codes.insert(CARRY_WEIGHT);
    }
    let before: Vec<(u16, f64)> = codes.iter().map(|&c| (c, value(rt, c))).collect();
    let capacity = |rt: &mut Runtime| rt.game.world.encumbrance(P).ok().map(|e| e.capacity);
    let capacity_before = capacity(rt);
    let r = rt.game.ingest_on(P, item, limb);
    let changes: Vec<String> = r
        .applied
        .iter()
        .filter_map(|a| {
            let c = a.change?;
            Some(if a.extended { format!("{} effect lengthened to {:.0} s", av_name(c.code), a.duration) } else { change_text(&c, a.duration) })
        })
        .collect();
    let values: Vec<String> = before
        .iter()
        .map(|&(c, b)| {
            let a = value(rt, c);
            format!("{} {} → {}", av_name(c), whole(b), whole(a))
        })
        .collect();
    let name = data.name(item);
    let on = limb.map_or(String::new(), |l| format!(" on the {}", limb_name(l)));
    let mut extra = String::new();
    if let Some(u) = r.usage {
        extra.push_str(&format!("; usage {u:.0}"));
    }
    if let Some((chance, hit)) = r.addiction {
        extra.push_str(&format!("; addiction chance {chance:.2}{}", if hit { ", addicted" } else { "" }));
    }
    if !r.perks.is_empty() {
        let perks: Vec<String> = r.perks.iter().map(|(p, point)| format!("{} (entry point {point})", data.name(*p))).collect();
        extra.push_str(&format!("; perks: {}", perks.join(", ")));
    }
    // What the menus' weight line reads (`fnv_sim::World::encumbrance`).
    if let (true, Some(a), Some(b)) = (codes.contains(&CARRY_WEIGHT), capacity_before, capacity(rt)) {
        extra.push_str(&format!("; carry capacity {} → {}", whole(a), whole(b)));
    }
    info!(
        "aid: the player used {name}{on} (potency {:.2}): {}{extra}; {}",
        r.potency,
        if changes.is_empty() { "no value changes".to_string() } else { changes.join(", ") },
        if values.is_empty() { "no values".to_string() } else { values.join(", ") }
    );
    let s = &mut rt.aid;
    for c in r.applied.iter().filter_map(|a| a.change).filter(|c| c.kind != ChangeKind::Once) {
        let lasting = s.lasting.entry(item).or_default();
        lasting.insert(c.code);
        if c.code == STRENGTH {
            lasting.insert(CARRY_WEIGHT);
        }
    }
    *s.used.entry(name).or_default() += 1;
    s.count_started(&r.effects);
    s.codes.extend(codes);
    r
}

/// The player reads `item` (already taken from the inventory by the menu) when it is a
/// skill book (`fnv_game::books`, docs/spec-notes/skill-books.md); false for any other
/// item. Logs `book: the player read <book>: <Skill> a → b (+p[, worth w]); perks: …;
/// Books Read n`.
pub fn read_book(rt: &mut Runtime, item: FormId) -> bool {
    let Some(r) = rt.game.read_book(P, item) else { return false };
    let data = rt.game.data;
    let mut extra = String::new();
    if r.worth != r.points {
        extra.push_str(&format!(", worth {}", whole(r.worth)));
    }
    if !r.perks.is_empty() {
        let perks: Vec<String> = r.perks.iter().map(|p| format!("{} (entry point {})", data.name(*p), fnv_game::perks::ADJUST_BOOK_SKILL_POINTS)).collect();
        extra.push_str(&format!("; perks: {}", perks.join(", ")));
    }
    let skill = fnv_game::chargen::skill_name(&rt.game, r.skill);
    let read = rt.game.misc_stat(fnv_game::misc_stats::BOOKS_READ);
    info!("book: the player read {}: {skill} {} → {} (+{}){extra}; Books Read {}", data.name(item), whole(r.before), whole(r.after), whole(r.points), whole(read));
    *rt.aid.used.entry(data.name(item)).or_default() += 1;
    true
}

fn whole(v: f64) -> String {
    if (v - v.round()).abs() < 0.05 {
        format!("{:.0}", v)
    } else {
        format!("{v:.1}")
    }
}

/// After the frame's tick (world swapped in): effects of aid items on the player that
/// ended, addictions gained and cured, from the events since `from`.
pub fn after_tick(rt: &mut Runtime, from: usize) {
    let data = rt.game.data;
    let mut ended: Vec<FormId> = Vec::new();
    let mut lines = Vec::new();
    for e in rt.game.events.get(from..).unwrap_or(&[]) {
        match &e.kind {
            EventKind::EffectFinished { id, target, source } if *target == P && data.ingestibles.contains_key(source) => {
                rt.aid.count_finished(*id);
                if !ended.contains(source) {
                    ended.push(*source);
                }
            }
            // A poison's end on the player, run out or cured (Antivenom's script dispels
            // them, docs/spec-notes/magic-archetypes.md §1).
            EventKind::EffectFinished { target, source, .. } if *target == P && data.spell_types.get(source) == Some(&fnv_game::magic_archetypes::SPELL_POISON) => {
                // Once for a spell whose several effects end together.
                let line = format!("magic: {} on the player ended", data.name(*source));
                if !lines.contains(&line) {
                    lines.push(line);
                }
            }
            EventKind::Addicted { actor, item, spell } if *actor == P => {
                lines.push(format!("aid: the player is addicted to {} ({})", data.name(*item), data.name(*spell)));
                rt.aid.addicted.push(data.name(*spell));
            }
            EventKind::AddictionCured { actor, spell } if *actor == P => {
                lines.push(format!("aid: {} cured", data.name(*spell)));
                rt.aid.cured += 1;
            }
            EventKind::NeedStage { actor, value, level, spell, .. } if *actor == P => {
                let stage = spell.map_or_else(|| "no stage".to_string(), |s| data.name(s));
                lines.push(format!("needs: {} {}: {stage}", av_name(*value), whole(*level)));
                rt.aid.stages += 1;
            }
            _ => {}
        }
    }
    for l in lines {
        info!("{l}");
    }
    for item in ended {
        // Only once no effect of the item is left on the player.
        if rt.game.effects.iter().any(|e| e.target == P && e.source == item && !e.inactive && e.effect != fnv_game::aid::USAGE_MONITOR) {
            continue;
        }
        let Some(codes) = rt.aid.lasting.remove(&item) else { continue };
        let values: Vec<String> = codes.iter().map(|&c| format!("{} {}", av_name(c), whole(value(rt, c)))).collect();
        info!("aid: {}'s effects on the player ended: {}", data.name(item), values.join(", "));
    }
}

/// An hour of a wait or a sleep has passed on the clock (world swapped in): it passes for
/// the player's effects and needs too (`fnv_game::Game::pass_hours`), and the log says
/// what changed.
pub fn hour_passed(rt: &mut Runtime, sleeping: bool, hour: u32) {
    let from = rt.game.events.len();
    // The player's tweak: a sleep heals only from its set hour on [design: docs/design/tweaks.md].
    let min = rt.game.tweaks.sleep_heal_min_hours;
    let heal = min == 0 || hour >= min;
    if sleeping && !heal {
        info!("sleep: hour {hour} of {min} before healing (tweak)");
    }
    let p = rt.game.pass_hours_healing(1.0, sleeping, heal);
    rt.aid.hours += 1;
    let needs: Vec<String> = p.needs.iter().map(|&(c, a, b)| format!("{} {} → {}", av_name(c), whole(a), whole(b))).collect();
    if !p.limbs_healed.is_empty() {
        let names: Vec<&str> = p.limbs_healed.iter().map(|&c| av_name(c)).collect();
        info!("sleep: crippled limbs healed: {}", names.join(", "));
    }
    if p.health_restored > 0.0 {
        info!("sleep: Health restored by {}", whole(p.health_restored));
    }
    info!(
        "wait: hour {hour} passed for the player ({:.0} s of effects): {} effect(s) ended{}",
        p.seconds,
        p.effects_ended,
        if needs.is_empty() { String::new() } else { format!("; {}", needs.join(", ")) }
    );
    after_tick(rt, from);
}

/// At exit, when the player used an aid item: `aid summary: …`.
pub fn report(mut exit: MessageReader<AppExit>, mut script: ResMut<ScriptState>, mut sim: ResMut<SimState>) {
    if exit.read().next().is_none() {
        return;
    }
    let Some(rt) = script.rt.as_mut() else { return };
    if rt.aid.used.is_empty() && rt.aid.stages == 0 && rt.aid.hours == 0 {
        return;
    }
    let used: Vec<String> = rt.aid.used.iter().map(|(n, c)| format!("{n} {c}")).collect();
    let total: u32 = rt.aid.used.values().sum();
    let mut codes: Vec<u16> = rt.aid.codes.iter().copied().collect();
    let hardcore = with_world(rt, &mut sim, |rt| rt.game.hardcore());
    if hardcore {
        for (c, _, _) in fnv_game::needs::NEEDS {
            if !codes.contains(&c) {
                codes.push(c);
            }
        }
    }
    let values = with_world(rt, &mut sim, |rt| codes.iter().map(|&c| format!("{} {}", av_name(c), whole(value(rt, c)))).collect::<Vec<_>>());
    let addictions: Vec<String> = rt.game.addictions(P).iter().map(|s| rt.game.data.name(*s)).collect();
    let s = &rt.aid;
    info!(
        "aid summary: {total} items used ({}), {} effects started, {} ended; addictions: {}; {} cured; {} stage changes; {} hours waited for effects{}; the player at exit: {}",
        used.join(", "),
        s.effects_started,
        s.effects_ended,
        if addictions.is_empty() { "none".to_string() } else { addictions.join(", ") },
        s.cured,
        s.stages,
        s.hours,
        if hardcore { " (Hardcore)" } else { "" },
        values.join(", ")
    );
}

#[cfg(test)]
mod tests {
    use super::AidStats;

    fn counts(s: &AidStats) -> (u32, u32) {
        (s.effects_started, s.effects_ended)
    }

    #[test]
    fn an_instant_effect_ends_as_counted() {
        let mut s = AidStats::default();
        s.count_started(&[7]);
        s.count_finished(7);
        assert_eq!(counts(&s), (1, 1));
    }

    /// An effect held inactive at the use is not in `Ingested::effects`, but its removal
    /// still raises `EffectFinished`.
    #[test]
    fn a_held_effect_that_expires_is_not_counted() {
        let mut s = AidStats::default();
        s.count_started(&[7]);
        s.count_finished(8);
        s.count_finished(9);
        assert_eq!(counts(&s), (1, 0));
    }

    /// A second dose extends the running effect and lists no new id: one start, one end.
    #[test]
    fn a_repeated_dose_is_one_start_and_one_end() {
        let mut s = AidStats::default();
        s.count_started(&[7]);
        s.count_started(&[]);
        assert_eq!(counts(&s), (1, 0));
        s.count_finished(7);
        assert_eq!(counts(&s), (1, 1));
    }

    #[test]
    fn an_effect_ends_only_once() {
        let mut s = AidStats::default();
        s.count_started(&[7]);
        s.count_finished(7);
        s.count_finished(7);
        assert_eq!(counts(&s), (1, 1));
    }
}
