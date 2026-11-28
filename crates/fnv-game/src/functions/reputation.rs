//! Reputations (fame and infamy per `REPU`), docs/spec-notes/scripts.md §7.6.
//!
//! The player holds infamy (axis 0) and fame (axis 1) per reputation
//! ([`crate::game::PlayerState::reputation`]) [documented: GECK wiki GetReputation;
//! observed: the second argument is 1 for fame and 0 for infamy in all six reputation
//! functions, layer5-captures.md §9.3 a]. A reputation's `DATA` value is its scale
//! [documented: GECK wiki GetReputationPct; schema `gameplay.json` REPU; observed: the
//! limits and levels below are fractions of it, §9.3 a].

use super::{none, Args, Ctx};
use crate::game::Game;
use crate::trace::EventKind;
use fnv_script::vm::Value;
use fnv_sim::FormId;
use std::collections::{BTreeMap, BTreeSet};

/// Points per deed size 1–5 for AddReputation / RemoveReputation: the game settings
/// fReputationBumpVeryMinor … fReputationBumpVeryMajor when a plugin defines them,
/// else their defaults 1, 2, 4, 7, 12 [documented: GECK wiki AddReputation; observed:
/// sizes 1–5 added and removed 1, 2, 4, 7, 12 points on scales 10 and 70 alike, and the
/// game read the five settings as those values; no master defines them,
/// layer5-captures.md §9.3 a].
const BUMPS: [(&str, f64); 5] = [
    ("fReputationBumpVeryMinor", 1.0),
    ("fReputationBumpMinor", 2.0),
    ("fReputationBumpAverage", 4.0),
    ("fReputationBumpMajor", 7.0),
    ("fReputationBumpVeryMajor", 12.0),
];

/// Fractions of the scale at which fame or infamy reaches levels 1, 2 and 3; a value
/// on a boundary already has the higher level [observed: one axis swept in steps of
/// scale/1000 on five reputations (scales 10–70) changed level exactly at 0.15, 0.5 and
/// 1.0 × the scale, and fame 1.5 of 10 was level 1, layer5-captures.md §9.3 a].
pub const LEVEL_FRACTIONS: [f64; 3] = [0.15, 0.5, 1.0];

/// The axis argument: 0 infamy, 1 fame; `None` for any other.
///
/// FalloutNV.esm calls these functions with axes 2 and 3 too: SetReputation axis 2 in the
/// reputation-clearing lines of Crocker's messenger and Vulpes's greeting, AddReputation
/// axis 3 in Severus's tag trades and axis 2 in one of Boulder City Showdown's endings,
/// GetReputation axis 2 and GetReputationThreshold axis 3 in conditions (VMS36,
/// vFreeformTheStreet02, VDialogueBenny) [observed: the decompiled scripts and INFO
/// conditions]. A reading function reads 0 for such an axis, without a call error
/// [observed: GetReputation axes 2/3 and GetReputationThreshold axis 3 read 0, captures
/// o5_00/o5_01, observer check 30]; SetReputation fails and stops the script
/// ([`write_axis`]). AddReputation, RemoveReputation and AddReputationExact change
/// nothing and go on [hypothesis, not captured: observer check 62]. Stopping there too
/// would skip the lines after them: Boulder City Showdown's stage 100 entry 1 (VMS20)
/// completes the quest after its axis-2 AddReputation, and Severus's tag trades count
/// the tags after theirs [observed: the decompiled stage and INFO scripts].
pub fn axis(a: &Args, i: usize) -> Result<Option<usize>, String> {
    Ok(match a.int(i)? {
        0 => Some(0),
        1 => Some(1),
        _ => None,
    })
}

/// The axis argument of the four functions that change a reputation (SetReputation,
/// AddReputation, RemoveReputation, AddReputationExact). Any axis but 0 or 1 fails the
/// call and stops the script for this run, as a failed call does: each with axis 2 or 3
/// changed nothing and the line after it did not run, while axes 0 and 1 changed the
/// value and the script went on [observed: captures o5_00 and o5_01, 2026-10-31, and
/// o11_00, 2026-11-01, RepNVNCR; issue #1127; observer check 62].
pub fn write_axis(g: &mut Game, a: &Args, i: usize) -> Result<usize, String> {
    match axis(a, i)? {
        Some(x) => Ok(x),
        None => {
            g.request_return();
            Err(format!("reputation axis {} is not 0 (infamy) or 1 (fame)", a.int(i)?))
        }
    }
}

fn reputation(g: &Game, a: &Args) -> Result<FormId, String> {
    let r = a.form(0)?;
    if g.data.reputations.contains_key(&r) || g.data.sig(r).as_ref() == Some(b"REPU") {
        Ok(r)
    } else {
        Err(format!("{} is not a reputation", g.data.name(r)))
    }
}

/// The scale of a reputation (its `DATA`); 0 when the record has none.
pub fn scale(g: &Game, r: FormId) -> f64 {
    g.data.reputations.get(&r).copied().unwrap_or(0.0) as f64
}

/// Infamy and fame.
pub fn values(g: &Game, r: FormId) -> [f64; 2] {
    g.player.reputation.get(&r).copied().unwrap_or([0.0; 2])
}

/// Infamy and fame the player really has: [`values`] plus what a disguise hid.
///
/// FalloutNV.esm's faction armour scripts keep the player's reputations in quest
/// variables and SetReputation every one to 0 in OnEquip, then in OnUnequip set each to
/// the kept value plus what was earned meanwhile [observed: the decompiled scripts
/// SCPT 00163116, 00134B6F–00134B71, 0012B03C, 001264EB and quest 001411A8's
/// `bFactionArmorEquipped`]. Those changes are a disguise, not the player's standing:
/// [`hide`] keeps them out of this value, so the reputation boxes follow it.
pub fn real(g: &Game, r: FormId) -> [f64; 2] {
    let [i, f] = values(g, r);
    let [hi, hf] = g.player.reputation_hidden.get(&r).copied().unwrap_or([0.0; 2]);
    [i + hi, f + hf]
}

/// After the player's OnEquip or OnUnequip block ran: whatever it changed in the
/// reputations, from `before`, moves into the hidden part, so [`real`] stays as it was
/// [hypothesis: an equip block changes reputations only for a disguise; the only ones in
/// FalloutNV.esm are the faction armour's].
pub fn hide(g: &mut Game, before: &BTreeMap<FormId, [f64; 2]>) {
    let keys: BTreeSet<FormId> = before.keys().chain(g.player.reputation.keys()).copied().collect();
    for r in keys {
        let (b, a) = (before.get(&r).copied().unwrap_or([0.0; 2]), values(g, r));
        if b == a {
            continue;
        }
        let h = g.player.reputation_hidden.entry(r).or_default();
        for axis in 0..2 {
            h[axis] += b[axis] - a[axis];
            // The disguise's end gives back what it took: no hidden part is left.
            if h[axis].abs() < 1e-9 {
                h[axis] = 0.0;
            }
        }
        if *h == [0.0; 2] {
            g.player.reputation_hidden.remove(&r);
        }
    }
}

/// Store one axis as given (SetReputation keeps any value, negative or above the scale
/// [observed: −3, 2.5 and twice the scale read back unchanged, §9.3 a]) and trace it.
pub fn store(g: &mut Game, r: FormId, axis: usize, value: f64) {
    g.player.reputation.entry(r).or_default()[axis] = value;
    g.emit(EventKind::ReputationChanged { reputation: r, axis, value });
}

/// Add `delta` points to one axis: a gain stops at the scale, and also brings a value
/// set above the scale down to it; a loss stops at 0 and leaves a value above the scale
/// above it [observed: 9 + 12 → 10 and 20 + 1 → 10 on scale 10, 69 + 12 → 70 and
/// 140 + 1 → 70 on scale 70, 1 − 12 → 0, 20 − 1 → 19 on scale 10, §9.3 a]. A
/// reputation without a scale is not limited above [hypothesis; every REPU has one].
/// A loss from below 0 ends at 0 [hypothesis, not captured].
pub fn change(g: &mut Game, r: FormId, axis: usize, delta: f64) {
    let s = scale(g, r);
    let v = values(g, r)[axis] + delta;
    let v = if delta >= 0.0 {
        if s > 0.0 {
            v.min(s)
        } else {
            v
        }
    } else {
        v.max(0.0)
    };
    store(g, r, axis, v);
}

/// The points of a deed size; `None` for a size other than 1–5, which changes nothing
/// [observed: sizes 0, 6 and −2, §9.3 a].
fn bump(g: &Game, size: i32) -> Option<f64> {
    let (name, default) = *BUMPS.get(usize::try_from(size.checked_sub(1)?).ok()?)?;
    Some(g.world.setting_f32(name).map(|v| v as f64).unwrap_or(default))
}

/// `AddReputation rep axis size`: fame or infamy grows by the deed size's points, up to
/// the scale [documented: GECK wiki AddReputation; observed: [`change`]].
pub fn add_reputation(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = reputation(g, a)?;
    let axis = write_axis(g, a, 1)?;
    if let Some(p) = bump(g, a.int(2)?) {
        change(g, r, axis, p);
    }
    none()
}

/// `RemoveReputation rep axis size`: the same points taken away, down to 0
/// [documented: GECK wiki RemoveReputation; observed: [`change`]].
pub fn remove_reputation(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = reputation(g, a)?;
    let axis = write_axis(g, a, 1)?;
    if let Some(p) = bump(g, a.int(2)?) {
        change(g, r, axis, -p);
    }
    none()
}

/// `AddReputationExact rep axis amount` [documented: GECK wiki AddReputationExact],
/// limited as AddReputation (a gain) or RemoveReputation (a loss) [hypothesis: not
/// captured].
pub fn add_reputation_exact(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = reputation(g, a)?;
    let axis = write_axis(g, a, 1)?;
    let p = a.num(2)?;
    change(g, r, axis, p);
    none()
}

/// Fractional loss through the same writer as deed-based removal
/// (docs/spec-notes/player-source-commands.md).
pub fn remove_reputation_exact(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = reputation(g, a)?;
    let axis = write_axis(g, a, 1)?;
    let amount = a.num(2)?;
    change(g, r, axis, -amount);
    none()
}

/// Fame or infamy over the scale, not capped: negative or above 1 when the value is
/// [documented: GECK wiki GetReputationPct; observed: −0.3 and 2, §9.3 a]. 0 for a
/// reputation without a scale [hypothesis].
pub fn get_reputation_pct(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = reputation(g, a)?;
    let Some(axis) = axis(a, 1)? else { return Ok(Value::ZERO) };
    let s = scale(g, r);
    let v = values(g, r)[axis];
    Ok(Value::Num(if s > 0.0 { v / s } else { 0.0 }))
}

/// The level (0–3) of fame or infamy: how many of [`LEVEL_FRACTIONS`] it reaches.
/// The comparison allows for rounding in `value / scale` (a value set to exactly
/// 0.15 × scale is on the boundary).
pub fn level(value: f64, scale: f64) -> usize {
    if scale <= 0.0 {
        return 0;
    }
    LEVEL_FRACTIONS.iter().filter(|&&f| value / scale + 1e-9 >= f).count()
}

/// The tier on each axis (mixed, good, bad) for fame level `f` and infamy level `i`
/// [observed: the 4 × 4 grid of layer5-captures.md §9.3 a, the same on five
/// reputations with scales 10–70]:
///
/// | fame \ infamy | 0 | 1 | 2 | 3 |
/// |---|---|---|---|---|
/// | 0 | 1,1,1 | 0,0,4 | 0,0,5 | 0,0,6 |
/// | 1 | 0,4,0 | 3,0,0 | 0,0,2 | 0,0,3 |
/// | 2 | 0,5,0 | 0,2,0 | 4,0,0 | 2,0,0 |
/// | 3 | 0,6,0 | 0,3,0 | 2,0,0 | 5,0,0 |
pub fn tiers(f: usize, i: usize) -> [i32; 3] {
    const GRID: [[[i32; 3]; 4]; 4] = [
        [[1, 1, 1], [0, 0, 4], [0, 0, 5], [0, 0, 6]],
        [[0, 4, 0], [3, 0, 0], [0, 0, 2], [0, 0, 3]],
        [[0, 5, 0], [0, 2, 0], [4, 0, 0], [2, 0, 0]],
        [[0, 6, 0], [0, 3, 0], [2, 0, 0], [5, 0, 0]],
    ];
    GRID[f.min(3)][i.min(3)]
}

/// `GetReputationThreshold rep axis` (0 mixed, 1 good, 2 bad): see [`tiers`].
pub fn get_reputation_threshold(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = reputation(g, a)?;
    let axis = a.int(1)?;
    // Axis 3 (VDialogueBenny's greetings) reads 0, as an unknown axis does in the other
    // reputation functions [hypothesis: see [`axis`]].
    if !(0..=2).contains(&axis) {
        return Ok(Value::ZERO);
    }
    let s = scale(g, r);
    let [infamy, fame] = values(g, r);
    let t = tiers(level(fame, s), level(infamy, s));
    Ok(Value::Num(t[axis as usize] as f64))
}

// ---- crimes against a reputation's people (owner's playtest list 5, item 13) ----------

/// Infamy for a crime someone saw, against every reputation the victim counts towards:
/// 2 for a theft and 30 for a kill (`Kind::Murder`: any kill of its people, self-defence
/// too) [documented: Fallout wiki "Reputation (Fallout: New
/// Vegas)", "Negative reputation for all towns and factions": being caught stealing, −2
/// each; killing people affiliated with a faction or town openly, −30 each (owner's copy
/// in C:nv-privateallout fandom); hypothesis: "caught" and "openly" are the crime
/// being seen, and the points are infamy added with [`change`], so up to the scale].
/// Other crimes change no reputation [hypothesis: the page names only these two].
pub const THEFT_INFAMY: f64 = 2.0;
pub const MURDER_INFAMY: f64 = 30.0;

/// The reputations `who` counts towards: a faction's own (FACT WMI1), those of every
/// faction a reference is in now, or those of the factions an actor base's record gives
/// [documented: FACT "Reputation", xEdit wbDefinitionsFNV.pas; hypothesis: an actor counts
/// towards each of its factions' reputations once].
pub fn reputations_of(g: &Game, who: FormId) -> Vec<FormId> {
    if let Some(&r) = g.data.faction_reputations.get(&who) {
        return vec![r];
    }
    // An NPC owner (XOWN) is a base form: no reference, so no run-time faction changes and
    // no leveled pick. A placed reference of it would be an arbitrary choice, and it may
    // have none or several [design].
    let factions: Vec<FormId> = if g.data.actors.contains_key(&who) {
        let source = g.data.base_template_source(who, |d| d.template_factions);
        g.data.actors.get(&source).map_or(Vec::new(), |d| d.factions.iter().filter(|(_, rank)| *rank >= 0).map(|(f, _)| *f).collect())
    } else {
        super::factions_of(g, who)
    };
    let mut out: Vec<FormId> = factions.into_iter().filter_map(|f| g.data.faction_reputations.get(&f).copied()).collect();
    out.sort();
    out.dedup();
    out
}

/// Give the infamy of a crime (see [`THEFT_INFAMY`]); returns each reputation changed
/// with its infamy before and after.
pub fn crime_infamy(g: &mut Game, kind: crate::crime::Kind, victim: FormId, seen: bool) -> Vec<(FormId, f64, f64)> {
    let points = match kind {
        crate::crime::Kind::Theft => THEFT_INFAMY,
        crate::crime::Kind::Murder => MURDER_INFAMY,
        _ => return Vec::new(),
    };
    if !seen {
        return Vec::new();
    }
    let mut out = Vec::new();
    for r in reputations_of(g, victim) {
        let before = values(g, r)[0];
        change(g, r, 0, points);
        out.push((r, before, values(g, r)[0]));
    }
    out
}
