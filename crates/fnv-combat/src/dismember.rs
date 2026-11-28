//! Dismemberment: whether a hit severs or explodes the body part it lands on
//! (docs/spec-notes/dismemberment.md §4). Pure rules: the caller supplies the roll.

use crate::data::{BodyPart, BodyPartData, CombatForms};
use serde::Serialize;

/// A weapon's On Hit choice (WEAP `DNAM`) [documented: GECK wiki Weapons].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub enum OnHit {
    /// "Normal formula behavior": the weapon rolls to sever and to explode.
    #[default]
    Normal,
    DismemberOnly,
    ExplodeOnly,
    Neither,
}

impl OnHit {
    /// From the schema's label; an unknown or missing value is Normal.
    pub fn from_label(s: Option<&str>) -> OnHit {
        match s {
            Some("Dismember Only") => OnHit::DismemberOnly,
            Some("Explode Only") => OnHit::ExplodeOnly,
            Some("No Dismember/Explode") => OnHit::Neither,
            _ => OnHit::Normal,
        }
    }

    pub fn may_explode(self) -> bool {
        matches!(self, OnHit::Normal | OnHit::ExplodeOnly)
    }

    pub fn may_sever(self) -> bool {
        matches!(self, OnHit::Normal | OnHit::DismemberOnly)
    }
}

/// What happens to a part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Fate {
    Sever,
    Explode,
}

/// The odds of the rule, measured in the original (docs/spec-notes/dismemberment.md §4,
/// capture campaign `dismember_odds`, 2026-10-25) [observed], and the one game setting it
/// reads. The masters' iCombat* and iMess* settings (§3) do not map onto the measured odds
/// in a way the data can tell apart, so they are not read.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Settings {
    /// Percent: a killing hit takes a severable part (head, arm, leg), without Bloody Mess.
    pub dismember_chance: f32,
    /// Percent of the parts a Normal-formula weapon takes that are severed (the rest explode).
    pub sever_share: f32,
    /// Percent, with Bloody Mess: a severable part goes.
    pub mess_part_chance: f32,
    /// Percent, with Bloody Mess: the torso (explodable only) explodes when the hit crippled
    /// it, and when it did not.
    pub mess_torso_crippled: f32,
    pub mess_torso_intact: f32,
    /// With Bloody Mess, a No Dismember/Explode weapon's odds are this share of the others'.
    pub mess_neither_scale: f32,
    /// Seconds from the hit to the exploded part's skin going.
    pub explode_delay: f32,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { dismember_chance: 75.0, sever_share: 30.0, mess_part_chance: 100.0, mess_torso_crippled: 80.0, mess_torso_intact: 20.0, mess_neither_scale: 0.5, explode_delay: 0.03 }
    }
}

impl Settings {
    pub fn from_forms(forms: &CombatForms) -> Settings {
        let d = Settings::default();
        Settings { explode_delay: forms.setting("fExplodeLimbRemovalDelay").unwrap_or(d.explode_delay), ..d }
    }
}

/// The hit, as the rule sees it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Blow {
    pub on_hit: OnHit,
    /// The target was already dead (a corpse).
    pub corpse: bool,
    /// The part hit is crippled (its condition at or below 0 after the hit).
    pub crippled: bool,
    /// Damage beyond the health the target had left (the whole damage on a corpse). Only
    /// logged: it does not change the odds [observed, §4].
    pub overkill: f32,
    /// The target's maximum health.
    pub max_health: f32,
    /// The attacker has Bloody Mess.
    pub bloody_mess: bool,
    /// The actor's BPTD defines only this single body part (e.g. `NVraven02`,
    /// `NVTumbleweed`).
    pub sole_part: bool,
}

/// The part's share of the target's maximum health, at least 1.
pub fn limb_health(part: &BodyPart, max_health: f32) -> f32 {
    (max_health * part.health_percent as f32 / 100.0).max(1.0)
}

/// The chances (percent) that the blow explodes the part and that it severs it; one roll
/// decides, so they add up to the chance the part goes (§4).
pub fn chances(part: &BodyPart, b: &Blow, s: &Settings) -> (f32, f32) {
    let (severable, explodable) = (part.severable(), part.explodable());
    if !severable && !explodable {
        return (0.0, 0.0);
    }
    // Narrowed to a single-part, non-severable Torso flagged Explodable + Absolute
    // (BPND bit 6, xEdit "To Hit Chance - Absolute") — matching crows (`NVraven02` in
    // #3435) and also covering `NVTumbleweed` (`0x163CF3`, not shown in #3435
    // [hypothesis]) — using its explosion chance directly without requiring Bloody Mess
    // while still honoring the weapon's On Hit gate below [hypothesis: #3435;
    // docs/spec-notes/dismemberment.md §4.2]. With Bloody Mess, this branch uses
    // `max(explodable_chance_percent, torso_odds)` [hypothesis / design] so Bloody Mess
    // never lowers the chance below `explodable_chance_percent`. Multi-part creatures and
    // non-torso Absolute parts stay on the measured §4.1 odds until observed in the
    // original.
    let sole_absolute_torso = !severable && explodable && b.sole_part && part.location() == 0 && part.explodable_absolute();
    let mut gone = if b.bloody_mess {
        if severable {
            s.mess_part_chance
        } else if sole_absolute_torso {
            (part.explodable_chance_percent as f32).max(if b.crippled || b.corpse { s.mess_torso_crippled } else { s.mess_torso_intact })
        } else if b.crippled || b.corpse {
            s.mess_torso_crippled
        } else {
            s.mess_torso_intact
        }
    } else if severable {
        s.dismember_chance
    } else if sole_absolute_torso {
        part.explodable_chance_percent as f32
    } else {
        0.0 // the torso goes only with Bloody Mess
    };
    if b.on_hit == OnHit::Neither {
        gone *= if b.bloody_mess { s.mess_neither_scale } else { 0.0 };
    }
    // Which way it goes: the weapon's On Hit where the part allows a choice. Bloody Mess
    // explodes what the weapon could not take otherwise (No Dismember/Explode, the torso).
    // Percent of the parts taken that are severed.
    let sever = match b.on_hit {
        _ if !severable => 0.0,
        _ if !explodable => 100.0,
        OnHit::Normal => s.sever_share,
        OnHit::DismemberOnly => 100.0,
        OnHit::ExplodeOnly | OnHit::Neither => 0.0,
    };
    if !b.bloody_mess && ((sever == 0.0 && !b.on_hit.may_explode()) || (sever == 100.0 && !b.on_hit.may_sever())) {
        return (0.0, 0.0);
    }
    let gone = gone.clamp(0.0, 100.0);
    (gone * (100.0 - sever) / 100.0, gone * sever / 100.0)
}

/// What the blow does to the part, given a roll uniform in [0, 1).
pub fn decide(part: &BodyPart, b: &Blow, s: &Settings, roll: f32) -> Option<Fate> {
    let (explode, sever) = chances(part, b, s);
    let r = roll * 100.0;
    if r < explode {
        Some(Fate::Explode)
    } else if r < explode + sever {
        Some(Fate::Sever)
    } else {
        None
    }
}

/// The parts that go with part `index` (it and every severable or explodable part sharing
/// its actor value), each with its own fate: an exploded part takes the others with it, and
/// only the Explodable ones among them explode [documented: GECK wiki KillActor notes].
pub fn parts_gone(data: &BodyPartData, index: usize, fate: Fate) -> Vec<(usize, Fate)> {
    let Some(hit) = data.parts.get(index) else { return Vec::new() };
    let mut out = vec![(index, fate)];
    for (i, p) in data.parts.iter().enumerate() {
        if i == index || !(p.severable() || p.explodable()) {
            continue;
        }
        if p.actor_value.is_some() && p.actor_value == hit.actor_value {
            out.push((i, if fate == Fate::Explode && p.explodable() { Fate::Explode } else { Fate::Sever }));
        }
    }
    out
}

/// The part KillActor's dismember argument names: the first severable or explodable part at
/// that location, and its fate (explode if Explodable, else sever) [hypothesis, §5].
pub fn scripted(data: &BodyPartData, location: i32) -> Option<(usize, Fate)> {
    let (i, p) = data.parts.iter().enumerate().find(|(_, p)| p.location() == location && (p.severable() || p.explodable()))?;
    Some((i, if p.explodable() { Fate::Explode } else { Fate::Sever }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(kind: &str, flags: &[&str], health: u32, explode: u32, av: &str) -> BodyPart {
        BodyPart {
            part_type: Some(kind.into()),
            flags: flags.iter().map(|s| s.to_string()).collect(),
            health_percent: health,
            explodable_chance_percent: explode,
            actor_value: Some(av.into()),
            ..BodyPart::default()
        }
    }

    fn head() -> BodyPart {
        part("Head 1", &["Severable", "Explodable"], 20, 30, "Perception Condition")
    }

    fn blow(on_hit: OnHit, overkill: f32) -> Blow {
        Blow { on_hit, crippled: true, overkill, max_health: 100.0, ..Blow::default() }
    }

    fn torso() -> BodyPart {
        part("Torso", &["Explodable"], 60, 50, "Endurance Condition")
    }

    #[test]
    fn on_hit_labels() {
        assert_eq!(OnHit::from_label(Some("Explode Only")), OnHit::ExplodeOnly);
        assert_eq!(OnHit::from_label(Some("No Dismember/Explode")), OnHit::Neither);
        assert_eq!(OnHit::from_label(None), OnHit::Normal);
    }

    #[test]
    fn a_kill_takes_the_head_three_times_in_four_whatever_the_overkill() {
        let s = Settings::default();
        // Normal formula: 30 % of the parts taken are severed.
        assert_eq!(chances(&head(), &blow(OnHit::Normal, 5.0), &s), (52.5, 22.5));
        assert_eq!(chances(&head(), &blow(OnHit::Normal, 500.0), &s), (52.5, 22.5));
        let mut intact = blow(OnHit::Normal, 5.0);
        intact.crippled = false;
        assert_eq!(chances(&head(), &intact, &s), (52.5, 22.5));
    }

    #[test]
    fn weapons_choose_what_they_may_do() {
        let s = Settings::default();
        assert_eq!(chances(&head(), &blow(OnHit::ExplodeOnly, 50.0), &s), (75.0, 0.0));
        assert_eq!(chances(&head(), &blow(OnHit::DismemberOnly, 50.0), &s), (0.0, 75.0));
        assert_eq!(chances(&head(), &blow(OnHit::Neither, 50.0), &s), (0.0, 0.0));
    }

    #[test]
    fn the_torso_goes_only_with_bloody_mess() {
        let s = Settings::default();
        assert_eq!(chances(&torso(), &blow(OnHit::Normal, 500.0), &s), (0.0, 0.0));
        let mut b = blow(OnHit::Normal, 50.0);
        b.bloody_mess = true;
        assert_eq!(chances(&torso(), &b, &s), (80.0, 0.0));
        b.crippled = false;
        assert_eq!(chances(&torso(), &b, &s), (20.0, 0.0));
    }

    #[test]
    fn bloody_mess_takes_every_head_and_overrides_no_dismember() {
        let s = Settings::default();
        let mut b = blow(OnHit::ExplodeOnly, 50.0);
        b.bloody_mess = true;
        assert_eq!(chances(&head(), &b, &s), (100.0, 0.0));
        b.on_hit = OnHit::Normal;
        assert_eq!(chances(&head(), &b, &s), (70.0, 30.0));
        b.on_hit = OnHit::Neither;
        assert_eq!(chances(&head(), &b, &s), (50.0, 0.0));
        assert_eq!(chances(&torso(), &b, &s), (40.0, 0.0));
    }

    #[test]
    fn one_roll_decides() {
        let s = Settings::default();
        let b = Blow { on_hit: OnHit::Normal, corpse: true, overkill: 10.0, max_health: 100.0, ..Blow::default() };
        assert_eq!(decide(&head(), &b, &s, 0.52), Some(Fate::Explode));
        assert_eq!(decide(&head(), &b, &s, 0.53), Some(Fate::Sever));
        assert_eq!(decide(&head(), &b, &s, 0.75), None);
    }

    #[test]
    fn shared_actor_value_parts_go_together() {
        let brain = part("Brain", &["Severable"], 20, 0, "Perception Condition");
        let data = BodyPartData { edid: None, model: None, parts: vec![head(), part("Torso", &["Explodable"], 60, 50, "Endurance Condition"), brain], ragdoll: None };
        assert_eq!(parts_gone(&data, 0, Fate::Explode), vec![(0, Fate::Explode), (2, Fate::Sever)]);
        assert_eq!(scripted(&data, 1), Some((0, Fate::Explode)));
        assert_eq!(scripted(&data, 5), None);
    }

    /// #3435: a single-part non-severable torso flagged `Explodable` + `To Hit Chance -
    /// Absolute` at 100 % (matching `NVraven02`) explodes on Normal and Explode Only kills
    /// without Bloody Mess, while still honoring `No Dismember/Explode` and `Dismember Only`.
    #[test]
    fn sole_explodable_absolute_torso_explodes_without_bloody_mess_respecting_on_hit() {
        let s = Settings::default();
        let crow_torso = part("Torso", &["Explodable", "To Hit Chance - Absolute"], 5, 100, "Perception Condition");
        assert!(crow_torso.explodable() && crow_torso.explodable_absolute() && !crow_torso.severable());
        for on_hit in [OnHit::Normal, OnHit::ExplodeOnly] {
            for crippled in [false, true] {
                let b = Blow { on_hit, crippled, overkill: 12.0, max_health: 6.0, bloody_mess: false, corpse: false, sole_part: true };
                assert_eq!(chances(&crow_torso, &b, &s), (100.0, 0.0), "{on_hit:?} crippled={crippled}");
                assert_eq!(decide(&crow_torso, &b, &s, 0.0), Some(Fate::Explode));
                assert_eq!(decide(&crow_torso, &b, &s, 0.999), Some(Fate::Explode));
            }
        }
        // Weapon On Hit restrictions stay enforced without Bloody Mess.
        for on_hit in [OnHit::Neither, OnHit::DismemberOnly] {
            let b = Blow { on_hit, crippled: true, overkill: 12.0, max_health: 6.0, bloody_mess: false, corpse: false, sole_part: true };
            assert_eq!(chances(&crow_torso, &b, &s), (0.0, 0.0), "{on_hit:?}");
            assert_eq!(decide(&crow_torso, &b, &s, 0.0), None);
        }
        // With Bloody Mess, No Dismember/Explode uses half odds, Dismember Only explodes,
        // and odds take max(explodable_chance_percent, torso_odds) (see production `chances` above).
        let mut bm_neither = Blow { on_hit: OnHit::Neither, crippled: true, overkill: 12.0, max_health: 6.0, bloody_mess: true, corpse: false, sole_part: true };
        assert_eq!(chances(&crow_torso, &bm_neither, &s), (50.0, 0.0));
        bm_neither.on_hit = OnHit::DismemberOnly;
        assert_eq!(chances(&crow_torso, &bm_neither, &s), (100.0, 0.0));
        let half_abs = part("Torso", &["Explodable", "To Hit Chance - Absolute"], 5, 50, "Perception Condition");
        let bm_crippled = Blow { on_hit: OnHit::Normal, crippled: true, overkill: 12.0, max_health: 6.0, bloody_mess: true, corpse: false, sole_part: true };
        assert_eq!(chances(&half_abs, &bm_crippled, &s), (80.0, 0.0));
        let bm_intact = Blow { crippled: false, ..bm_crippled };
        assert_eq!(chances(&half_abs, &bm_intact, &s), (50.0, 0.0));
    }

    /// Controls keeping multi-part creatures, single-part non-torsos, and ordinary torsos
    /// on the measured §4.1 odds:
    /// - multi-part `Explodable + Absolute` parts (Securitrons, ants, eyebots, turrets, etc.)
    ///   keep their regular odds;
    /// - a single-part `Head 1` (e.g. Mr. House) stays gated without Bloody Mess;
    /// - `Absolute` without `Explodable` and ordinary torsos without `Absolute` stay 0 %.
    #[test]
    fn non_crow_absolute_parts_stay_on_measured_odds() {
        let s = Settings::default();
        // Multi-part severable Explodable + Absolute (Securitron head, Ant antenna, Ghost People):
        // unchanged measured odds and still blocked by No Dismember/Explode without Bloody Mess.
        let abs_head = part("Head 1", &["Severable", "Explodable", "To Hit Chance - Absolute"], 25, 100, "Perception Condition");
        assert_eq!(chances(&abs_head, &blow(OnHit::Normal, 10.0), &s), (52.5, 22.5));
        assert_eq!(chances(&abs_head, &blow(OnHit::Neither, 10.0), &s), (0.0, 0.0));
        assert_eq!(chances(&abs_head, &blow(OnHit::DismemberOnly, 10.0), &s), (0.0, 75.0));

        // Multi-part non-severable Torso with Explodable + Absolute (Eyebot Armor, Turret Hull):
        // `sole_part = false` keeps the existing Bloody Mess torso gate.
        let multi_torso = part("Torso", &["Explodable", "To Hit Chance - Absolute"], 50, 100, "Endurance Condition");
        assert_eq!(chances(&multi_torso, &blow(OnHit::Normal, 50.0), &s), (0.0, 0.0));

        // Single-part non-severable Head 1 with Explodable + Absolute (Mr. House): not a Torso.
        let sole_head = part("Head 1", &["Explodable", "To Hit Chance - Absolute"], 5, 100, "Perception Condition");
        let sole_blow = Blow { sole_part: true, ..blow(OnHit::Normal, 10.0) };
        assert_eq!(chances(&sole_head, &sole_blow, &s), (0.0, 0.0));

        // Absolute without Explodable on a sole torso: never explodes.
        let not_explodable = part("Torso", &["To Hit Chance - Absolute"], 100, 100, "Endurance Condition");
        assert_eq!(chances(&not_explodable, &sole_blow, &s), (0.0, 0.0));

        // Ordinary sole torso without Absolute: still 0 % without Bloody Mess.
        assert_eq!(chances(&torso(), &sole_blow, &s), (0.0, 0.0));
    }
}
