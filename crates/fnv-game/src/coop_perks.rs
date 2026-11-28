//! Co-op perks [design]: two perks that
//! only mean something with another player, and the items a revive uses.
//!
//! - **The perks** are NVForever's own records, put into the game's data when it is built
//!   ([`add`]), with FormIDs no plugin uses ([`HEALING_HANDS`], [`QUACK_SURGEON`]): there
//!   is no NVForever patch plugin yet to carry them [design]. Each is one rank, playable,
//!   level [`LEVEL`], and requires Medicine 30 and Charisma 5 (their design's requirements)
//!   written as the masters write a requirement (GetPermanentActorValue ≥ n on the
//!   subject) [observed: Rapid Reload's two conditions, Guns 30 and Agility 5, at level
//!   2: the one perk of the masters with a 30 skill and a 5 S.P.E.C.I.A.L., so the same
//!   level here]. Their names and descriptions are NVForever's own words (LEGAL.md:
//!   nothing transcribed), shown in the chosen language through `fnv_util::words::tr`
//!   (a line in every `lang/*.toml`). Their pictures are the base game's own perk
//!   pictures (owner, 2026-11-28): Healing Hands shows Better Healing's, Quack Surgeon
//!   Living Anatomy's ([`HEALING_HANDS_ICON`], [`QUACK_SURGEON_ICON`]).
//! - **To revisit** (owner, 2026-11-28): these two perks may need to become ESM data, a
//!   PERK record each in an NVForever plugin, instead of records added in code here. Left
//!   as they are for now; to be revisited at a later date.
//! - **Only in co-op**: out of a session they are not offered at all, so a single player
//!   game's level-up list is the original's (`crate::levelup::PerkSheet::of_player`,
//!   [`Game::coop_session`]) [design].
//! - **Healing Hands**: a player revived by one who has it is also cured of all
//!   radiation, as RadAway's full course does (Rad level to 0) (nvforever `coop.rs`).
//! - **Quack Surgeon**: a revive may use an alcoholic drink in place of a Stimpak
//!   ([`revive_item`]): a drink first, the least worth of those carried, a Stimpak only
//!   when there is none.
//! - **The others' companion perks** ([`FromOthers`]): each player is the others'
//!   companion. Its Companion Nerve comes from the others' Charisma (the highest of the
//!   others in the session, wherever they are: one leader's, as a companion has one)
//!   [design], 5 % per point as
//!   the player's companions get it (`Game::companion_nerve`); and while another player
//!   with Ferocious Loyalty is below half health, this one gets that perk's Damage
//!   Resistance [observed: FalloutNV.esm's FerociousLoyaltyQuest gives each hired
//!   companion the actor effect FerociousLoyaltyEffect, Damage Resistance +50, while the
//!   player's health is below 0.5, and takes it back at 0.5 and over]. Two others with it
//!   give it once [hypothesis: the same actor effect added twice counts once]. A
//!   player's own perks never act on itself this way, and it all ends with the session
//!   (nvf-play `coop_companions`). Spray and Pray and Animal Friend name no teammates in
//!   their data (they act on factions and creatures) [observed]; Lonesome Road's own
//!   perk counts teammates (GetPlayerTeammateCount), and the other players are not
//!   counted there [design: left as it is].
//! - **What is alcohol**: an ingestible of equip type Alcohol (ETYP 13) [documented:
//!   xEdit wbEquipTypeEnum] with an effect whose Editor ID is `ChemRedINAlcohol`
//!   [observed: every drink of FalloutNV.esm and the DLCs has it (Beer, Whiskey, Vodka,
//!   Wine, Scotch, Dixon's Whiskey, Absinthe, Moonshine, Wasteland Tequila…); the
//!   scorpion poisons and Datura Antivenom are also of equip type 13 and have not].

use crate::cond::{Cmp, Condition, Operand, RunOn};
use crate::data::{GameData, PerkDef};
use crate::Game;
use fnv_script::vm::ArgValue;
use fnv_sim::FormId;

/// Healing Hands and Quack Surgeon (NVForever's records, in no plugin's FormID range:
/// 0xFE is the light plugins' index, and these sit below the co-op puppets' 0xFE00C000)
/// [design].
pub const HEALING_HANDS: FormId = FormId(0xFE00_B001);
pub const QUACK_SURGEON: FormId = FormId(0xFE00_B002);
/// Both perks' level (see the module notes).
pub const LEVEL: u8 = 2;
/// The Stimpak (main's `unconscious::STIMPAK`).
pub use crate::unconscious::STIMPAK;
/// GetPermanentActorValue's condition index [observed: the masters' requirements, 495].
const GET_PERMANENT_ACTOR_VALUE: u16 = 0x1EF;
/// The pictures: the base game's own perk pictures from its archives, so nothing new is
/// shipped (owner, 2026-11-28) [observed: FalloutNV.esm's PERK records BetterHealing
/// 0015C60B and LivingAnatomy 00146099 name these two icons].
pub const HEALING_HANDS_ICON: &str = r"interface\icons\pipboyimages\perks\perk_better_healing.dds";
pub const QUACK_SURGEON_ICON: &str = r"interface\icons\pipboyimages\perks\perk_living_anatomy.dds";

fn at_least(code: u16, n: f64) -> Condition {
    Condition { cmp: Cmp::Ge, or: false, run_on: RunOn::Subject, function: GET_PERMANENT_ACTOR_VALUE, params: vec![ArgValue::Enum(code)], rhs: Operand::Value(n) }
}

/// A co-op perk; `name` and `description` come already in the chosen language (`tr`). The
/// perks are built with the game's data, after the start has chosen the language.
fn perk(name: &str, description: &str, icon: &str) -> PerkDef {
    use crate::actor_values::{CHARISMA, MEDICINE};
    PerkDef {
        name: name.into(),
        description: description.into(),
        icon: Some(icon.into()),
        is_trait: false,
        min_level: LEVEL,
        ranks: 1,
        playable: true,
        hidden: false,
        conditions: Some(vec![at_least(MEDICINE, 30.0), at_least(CHARISMA, 5.0)]),
        entries: Vec::new(),
    }
}

/// Put the co-op perks into `data` (once, as it is built).
pub fn add(data: &mut GameData) {
    use fnv_util::words::tr;
    data.perks.insert(
        HEALING_HANDS,
        perk(
            tr("Healing Hands"),
            tr("Co-op only. Your touch does more than patch a friend up: anyone you revive gets back on their feet with every rad flushed out of their system."),
            HEALING_HANDS_ICON,
        ),
    );
    data.perks.insert(
        QUACK_SURGEON,
        perk(
            tr("Quack Surgeon"),
            tr("Co-op only. Who needs a Stimpak? You can revive a downed friend with a bottle of liquor instead. A Stimpak is used only when you have nothing to drink."),
            QUACK_SURGEON_ICON,
        ),
    );
    for (id, edid) in [(HEALING_HANDS, "NVFCoopHealingHands"), (QUACK_SURGEON, "NVFCoopQuackSurgeon")] {
        data.edid_of.insert(id, edid.into());
        data.edids.insert(edid.to_ascii_lowercase(), id);
    }
}

/// Whether the player has `perk` (a co-op perk).
pub fn has(g: &Game, perk: FormId) -> bool {
    g.player.perks.get(&perk).is_some_and(|&r| r > 0)
}

/// Healing Hands on the revived player: its rads go to 0 (as a growing value moves,
/// `RestoreActorValue RadiationRads`). The rads it had.
pub fn cure_rads(g: &mut Game) -> f64 {
    use crate::aid::RAD_LEVEL;
    let rads = crate::functions::actor::actor_value(g, FormId::PLAYER_REF, RAD_LEVEL).unwrap_or(0.0);
    if rads > 0.0 {
        let _ = crate::functions::actor::shift_growing(g, FormId::PLAYER_REF, RAD_LEVEL, -rads);
    }
    rads
}

/// Whether `perk` is one of the co-op perks.
pub fn is_coop_perk(perk: FormId) -> bool {
    perk == HEALING_HANDS || perk == QUACK_SURGEON
}

/// Whether `item` is an alcoholic drink (see the module notes).
pub fn is_alcohol(data: &GameData, item: FormId) -> bool {
    const ALCOHOL: i64 = 13;
    data.ingestibles.get(&item).is_some_and(|i| i.equip_type == ALCOHOL)
        && data.effect_items.get(&item).is_some_and(|effects| effects.iter().any(|e| data.edid_of.get(&e.effect).is_some_and(|n| n.eq_ignore_ascii_case("ChemRedINAlcohol"))))
}

/// What the player revives with: with Quack Surgeon the least worth drink carried, else
/// a Stimpak if carried; `None` with neither. `carried` is the player's inventory (item,
/// count).
pub fn revive_item(g: &Game, carried: impl IntoIterator<Item = (FormId, i32)>) -> Option<FormId> {
    let quack = has(g, QUACK_SURGEON);
    let mut stimpak = false;
    let mut drink: Option<(i32, FormId)> = None;
    for (item, n) in carried {
        if n <= 0 {
            continue;
        }
        if item == STIMPAK {
            stimpak = true;
        } else if quack && is_alcohol(g.data, item) {
            let worth = g.data.ingestibles.get(&item).map_or(0, |i| i.value);
            if drink.is_none_or(|(w, f)| (worth, item.0) < (w, f.0)) {
                drink = Some((worth, item));
            }
        }
    }
    drink.map(|(_, f)| f).or(stimpak.then_some(STIMPAK))
}

/// What the other players' companion perks give this player (see the module notes).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FromOthers {
    /// Companion Nerve: the share this player's damage and armour go up by.
    pub nerve: f64,
    /// Damage Resistance added (Ferocious Loyalty).
    pub dr: f64,
}

/// What this player tells the others: its Charisma (1 to 10) and whether its Ferocious
/// Loyalty acts now (it has the perk and is below half health).
pub fn my_side(g: &mut Game) -> (u8, bool) {
    use crate::actor_values::{CHARISMA, HEALTH};
    let charisma = crate::functions::actor_value_capped(g, FormId::PLAYER_REF, CHARISMA).clamp(0.0, 10.0) as u8;
    let loyal = g.data.by_edid("FerociousLoyalty").is_some_and(|p| g.player.perks.get(&p).is_some_and(|&r| r > 0));
    let low = loyal && {
        let base = crate::functions::actor::actor_value_parts(g, FormId::PLAYER_REF, HEALTH).map_or(0.0, |p| p[0] + p[1]);
        let now = crate::functions::actor::actor_value(g, FormId::PLAYER_REF, HEALTH).unwrap_or(0.0);
        base > 0.0 && now / base < 0.5
    };
    (charisma, low)
}

/// Ferocious Loyalty's Damage Resistance: its actor effect's magnitude [observed:
/// FerociousLoyaltyEffect, IncreaseDamageResistance 50], 50 without it.
pub fn loyalty_dr(data: &GameData) -> f64 {
    data.by_edid("FerociousLoyaltyEffect").and_then(|s| data.effect_items.get(&s)).and_then(|e| e.first()).map_or(50.0, |e| f64::from(e.magnitude))
}

/// What the others here give: from each one's Charisma and Ferocious Loyalty.
pub fn from_others(others: impl IntoIterator<Item = (u8, bool)>, loyalty_dr: f64) -> FromOthers {
    let mut out = FromOthers::default();
    for (charisma, loyal) in others {
        out.nerve = out.nerve.max(crate::followers::COMPANION_NERVE_PER_CHARISMA * f64::from(charisma.min(10)));
        if loyal {
            out.dr = loyalty_dr;
        }
    }
    out
}

/// The perks the player's companions give the player [observed: FalloutNV.esm
/// VNPCFollowersQuestSCRIPT (SCPT 0x001209D1) and P04CompanionFireQuestSCRIPT
/// (0x0017B777) take each back from the player when its companion leaves: Boone
/// Spotting, Arcade BetterHealing, Cass WhiskeyRose, Lily StealthGirl, Veronica
/// ScribeAssistant, Raul RegularMaintenance and FullMaintenance, Rex SearchAndMark,
/// ED-E EnhancedSensors; LonesomeRoad.esm NVDLC04EDEHireTriggerSCRIPT (0x0100CD49)
/// gives NVDLC04EnhancedSensors and NVDLC04EDENoMines with its ED-E. Dead Money's,
/// Honest Hearts' and Old World Blues' companions give the player none].
pub const COMPANION_PERKS: [&str; 11] = [
    "Spotting",
    "BetterHealing",
    "WhiskeyRose",
    "StealthGirl",
    "ScribeAssistant",
    "RegularMaintenance",
    "FullMaintenance",
    "SearchAndMark",
    "EnhancedSensors",
    "NVDLC04EnhancedSensors",
    "NVDLC04EDENoMines",
];

/// The companion perks the player has now (every player's go to every other).
pub fn companion_perks(g: &Game) -> Vec<FormId> {
    COMPANION_PERKS.iter().filter_map(|e| g.data.by_edid(e)).filter(|p| has(g, *p)).collect()
}

/// What this player tells the others: the follow perks its own companions give
/// it now, not the ones it has from another player's companion (`mirrored`), so a perk
/// never comes back to the player whose companion gave it.
pub fn own_companion_perks(g: &Game, mirrored: &[FormId]) -> Vec<FormId> {
    companion_perks(g).into_iter().filter(|p| !mirrored.contains(p)).collect()
}

/// The follow perks this player gets from the others [design: while a companion is in the party, all players get the perk]: every perk any other
/// player's companion gives now, whoever hired it, once each.
pub fn party_perks<'a>(others: impl IntoIterator<Item = &'a [FormId]>) -> Vec<FormId> {
    let mut want: Vec<FormId> = Vec::new();
    for p in others.into_iter().flatten() {
        if !want.contains(p) {
            want.push(*p);
        }
    }
    want
}

/// Bring the perks this player has from the others' companions (`mirrored`) to `want`:
/// give the new ones ([`mirror_perk`]) and take back each one no other player's companion
/// gives any more ([`unmirror_perk`]). A perk the player had of its own is not touched.
/// Returns the perks given and the perks taken back.
pub fn sync_party_perks(g: &mut Game, mirrored: &mut Vec<FormId>, want: &[FormId]) -> (Vec<FormId>, Vec<FormId>) {
    let mut given = Vec::new();
    for &p in want {
        if !mirrored.contains(&p) && g.data.perks.contains_key(&p) && mirror_perk(g, p) {
            mirrored.push(p);
            given.push(p);
        }
    }
    let taken: Vec<FormId> = mirrored.iter().copied().filter(|p| !want.contains(p)).collect();
    for &p in &taken {
        unmirror_perk(g, p);
    }
    mirrored.retain(|p| !taken.contains(p));
    (given, taken)
}

/// Give the player `perk` as another player's companion gives it, recorded as
/// the session's story so the player's own save never keeps it (`crate::coop_story`). False when
/// the player had it already (then it is not this game's to take back).
pub fn mirror_perk(g: &mut Game, perk: FormId) -> bool {
    if has(g, perk) {
        return false;
    }
    g.coop_story.record_perk(perk, 0);
    crate::perks::gain_rank(g, perk);
    g.perk_event(perk, true, 0, 1);
    g.sync_perk_abilities_of(FormId::PLAYER_REF);
    true
}

/// Take back a perk [`mirror_perk`] gave.
pub fn unmirror_perk(g: &mut Game, perk: FormId) {
    if g.player.perks.remove(&perk).is_some() {
        g.perk_event(perk, false, 0, 0);
        g.sync_perk_abilities_of(FormId::PLAYER_REF);
    }
    if g.coop_story.perks.get(&perk) == Some(&0) {
        g.coop_story.perks.remove(&perk);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_others_lead_as_one_companion_leader() {
        assert_eq!(from_others([], 50.0), FromOthers::default());
        let f = from_others([(5, false), (8, false)], 50.0);
        assert!((f.nerve - 0.4).abs() < 1e-9 && f.dr == 0.0);
        // Two others with Ferocious Loyalty below half health: once.
        assert_eq!(from_others([(6, true), (6, true)], 50.0).dr, 50.0);
    }

    #[test]
    fn the_co_op_perks_need_medicine_and_charisma() {
        let p = perk("x", "y", "z");
        assert_eq!((p.min_level, p.ranks, p.playable), (LEVEL, 1, true));
        let c = p.conditions.unwrap();
        assert_eq!(c.len(), 2);
        assert!(c.iter().all(|c| c.function == GET_PERMANENT_ACTOR_VALUE && c.cmp == Cmp::Ge));
        assert!(is_coop_perk(HEALING_HANDS) && is_coop_perk(QUACK_SURGEON) && !is_coop_perk(STIMPAK));
    }
}
