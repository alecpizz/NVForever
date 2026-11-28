//! Combat's consequences in the game world (docs/design/combat.md §5.3 step 5, step C5):
//! health, hits, death and kill XP, through the same actor values, flags and script
//! events the scripts see (`functions::actor`). The damage itself is `fnv-combat`'s;
//! this module applies it.
//!
//! - **Level** [documented: GECK wiki Stats Tab pages]: ACBS level, or with "PC Level
//!   Mult" the player's level × the multiplier, kept within the calculated minimum and
//!   maximum (a maximum of 0 is none).
//! - **Maximum health** (the base of Health, code 16; `crate::actor_values` has the
//!   other values), from permanent Endurance:
//!   - player: `fAVDHealthEnduranceOffset` + Endurance × `fAVDHealthEnduranceMult` +
//!     (level − 1) × `fAVDHealthLevelMult` + its NPC_ Base Health;
//!   - NPC: Base Health + (Endurance − 1) × `fAVDNPCHealthEnduranceMult`, plus (level
//!     − 1) × `fAVDNPCHealthLevelMult` when its stats are auto-calculated ("Auto-calc
//!     stats"); a hand-set NPC's level adds nothing;
//!   - creature: its DATA Health.
//!
//!   [documented: the settings' names and values are in the masters, combat-data.md §3,
//!   and the Stats Tab pages say NPC health derives from Endurance and level plus Base
//!   Health; observed: the NPC form fits all fifteen NPCs the AI observer saw at full
//!   Health in the original (auto-calculated: the NCR troopers 50, Fiends 65 to 105,
//!   Joe Cobb, Sunny Smiles, Ringo, settlers, the Strip crowd, Violet 170; hand-set:
//!   Cook-Cook 215 at level 14, Doc Mitchell, Trudy, the Geckoman, Ramos),
//!   npc-behaviour-compare.md §9; hypothesis: the player's form, to be settled by
//!   capture.] A template with the "Stats" flag supplies the stats when it is an NPC_ or
//!   CREA.
//! - **A hit** lowers Health's damage part (as `DamageActorValue`), remembers the hit
//!   (location, critical, attacker, weapon) and runs the target's OnHit (the attacker as
//!   subject) and OnHitWith (the weapon) blocks [documented: block names, catalogue;
//!   hypothesis: their order].
//! - **Limb damage** (step C8): a hit on a body part lowers that part's condition actor
//!   value (25–31, starting at 100) by the limb damage as a share of the part's pool, the
//!   part's health percentage of the actor's maximum health: `loss = limb damage ÷
//!   (max health × percent ÷ 100) × 100`. The limb is crippled when its condition
//!   reaches 0 [documented: GECK wiki Body Part Data, crippled after that percentage of
//!   total health; hypothesis: the count-down form]. Conditions stop at 0.
//! - **No damage** for actors of a race with the Child flag [documented: GECK wiki Race]
//!   or creatures flagged Invulnerable [documented: xEdit flag name].
//! - **Essential** actors (base flag, `SetEssential` or `SetActorRefEssential`) are not
//!   killed: at zero health they are knocked out and keep 1 health, and get up later
//!   (`crate::unconscious`, docs/spec-notes/unconscious.md) [documented: the flag and
//!   KillActor's notes; design: what stays undone].
//! - **Death** at zero health: as `KillActor` (dead flag, killer, GetDeadCount, OnDeath).
//! - **Kill XP** for the player [documented: GECK wiki Experience Settings]: the target's
//!   level picks its difficulty (the first `iXPLevelKill<NPC|Creature><Class>` at or above
//!   it, very hard beyond); the reward is that class's `iXPRewardKill<NPC|Opponent>
//!   <Class>`; none when others did more than `100 − iXPDeathRewardHealthThreshold` % of
//!   the damage.

use crate::data::MagicEffectDef;
use crate::functions::{actor, base_of};
use crate::game::{event, Game};
use fnv_script::vm::Value;
use fnv_sim::FormId;
use serde::Serialize;

pub use crate::actor_values::{HEALTH, LIMB_CONDITIONS};

/// What scripts ask of the engine's combat.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum CombatRequest {
    /// Set off the placed explosives of this actor that detonate
    /// (`DetonatePlacedExplosives`).
    Detonate(FormId),
    /// `from` fires one shot of `weapon` from its centre, straight ahead (`FireWeapon`).
    Fire { from: FormId, weapon: FormId, muzzle: Option<[f32; 3]> },
    /// Dismember the part at `location` (GetHitLocation's numbering) of a dead actor:
    /// KillActor's limb argument. The engine knows the actor's body parts
    /// (docs/spec-notes/dismemberment.md §5).
    Dismember { actor: FormId, location: i32, by: Option<FormId>, explode: Option<bool> },
    /// Set off an explosion placed by a script (PlaceAtMe of an EXPL) where it was placed,
    /// carrying the script caller as `source` for explosion-ref queries and
    /// `ShowOff:OnExplosionHit` without fabricating a weapon `Cause`.
    Explode { explosion: FormId, at: [f32; 3], source: Option<FormId> },
}

/// What damage did to a destructible reference (step C7).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct DestructionChange {
    pub before: i32,
    pub after: i32,
    /// Explosions of the stages reached, in order.
    pub explosions: Vec<FormId>,
    pub destroyed: bool,
    pub disabled: bool,
    /// The last stage's replacement model and model damage stage, when it has them.
    pub model: Option<String>,
    pub damage_stage: Option<u8>,
    /// The debris (DEBR) of the stages reached and how many pieces of it.
    pub debris: Vec<(FormId, i32)>,
}

pub(crate) fn destruction_damage_threshold(health: f64, percent: u8) -> f64 {
    health * (100.0 - f64::from(percent)) / 100.0
}

pub(crate) fn destruction_threshold_reached(damage: f64, health: f64, percent: u8) -> bool {
    // [design] Eight f64 epsilons scaled by health cover rounding (including old saves'
    // health * (1 - percent / 100)) without swallowing real damage intervals; see
    // docs/spec-notes/combat-rules.md, object destruction.
    damage >= destruction_damage_threshold(health, percent) - 8.0 * f64::EPSILON * health
}

/// What a hit did.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct HitResult {
    /// Health damage actually taken.
    pub dealt: f64,
    pub health: f64,
    pub max_health: f64,
    pub died: bool,
    /// An essential actor was knocked out by this hit instead of dying
    /// (`crate::unconscious`).
    pub knocked_out: bool,
    /// The target takes no damage (child race, invulnerable, already dead).
    pub immune: bool,
    /// XP the player gained for the kill.
    pub xp: f64,
    /// The hit limb's condition after the hit, with its actor value code (step C8).
    pub limb: Option<(u16, f64)>,
    /// That hit crippled the limb (its condition reached 0 now).
    pub crippled: bool,
    /// Damage beyond the health the target had left: on the hit that killed it, and the
    /// whole damage on a corpse (docs/spec-notes/dismemberment.md §4).
    pub overkill: f64,
    /// The target was dead before the hit.
    pub corpse: bool,
}

/// A hit on a body part (step C8).
#[derive(Debug, Clone, Copy, Default)]
pub struct LimbHit {
    /// The part's condition actor value code (25–31).
    pub condition: u16,
    /// The part's share of the actor's maximum health, percent (BPTD health percent).
    pub health_percent: f64,
    /// Limb damage: the hit's damage times the weapon's Limb Dmg Mult.
    pub damage: f64,
    /// `damage` is condition points taken straight off the part's condition, not a share
    /// of its pool, and no limb damage multiplier applies (a fall's leg,
    /// docs/spec-notes/physics.md §6.12).
    pub points: bool,
}

/// A hit's circumstances.
#[derive(Debug, Clone, Copy, Default)]
pub struct HitInfo {
    pub attacker: Option<FormId>,
    pub weapon: Option<FormId>,
    /// Live projectile or explosion reference responsible for the hit (`GetHitProjectile`).
    pub projectile: Option<FormId>,
    /// GetHitLocation's index (−1 none, 0 torso, …).
    pub location: i32,
    pub critical: bool,
    /// Target blocked the melee/unarmed blow (`GetHitExtendedFlag 0`).
    pub blocking: bool,
    /// Sneak attack (`GetHitExtendedFlag 7`).
    pub sneak_attack: bool,
    /// Explosion hit (`GetHitExtendedFlag 8`; `GetHit3DData` reports all zeros).
    pub explosion: bool,
    /// Weapon base damage after attacker skill and weapon condition, plus critical hit
    /// damage on critical hits, or distance-attenuated `EXPL` damage (before difficulty
    /// scaling) on explosion hits (`GetHitBaseWeaponDamage`, jip-hit-info.md §1, §6)
    /// [observed: 2026-11-23 & 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`) &
    /// Goodsprings (`0x000DAEBB`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN
    /// 57.30 (10 official base masters), #3678 / PR #3679 probes
    /// `w1_3606_hit_3d_dir_and_base_damage` (10mm non-crit `22`, crit `44`,
    /// `SetCritDam 200` crit `222`, `GrenadeFragExplosion` at d=0 `125`) and
    /// `w4d_13_explosion_falloff_outdoor` (`tools/capture/nvse/catalogs/wave4d_extenders.json`,
    /// #3679): `MineFragExplosion` base 100 at d=0 `bd=100.00, hd=150.00`, at d=160.7
    /// `bd=82.03, hd=123.04`; no DLL or source consulted].
    pub base_weapon_damage: f64,
    /// Fatigue damage dealt by the hit (`GetHitFatigueDamage`, jip-hit-info.md §1).
    pub fatigue_damage: f64,
    /// World-space 3D impact point (`GetHit3DData 0` and elements 0..3 of `GetHit3DData 2`).
    pub hit_pos: Option<[f32; 3]>,
    /// Unit 3D incoming hit travel direction vector `[nx, ny, nz]` (`||v|| == 1.0`,
    /// `GetHit3DData 1` and elements 3..6 of `GetHit3DData 2`) [observed: 2026-11-23,
    /// GSDocMitchellHouse (0x00103DF9), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 +
    /// JIP LN 57.30 (10 official base masters), #3678 / PR #3679 probe
    /// `w1_3606_hit_3d_dir_and_base_damage`: South->North `+Y` and East->West `-X` 10mm shots].
    pub hit_dir: Option<[f32; 3]>,
    /// The body part's limb damage, when the hit is on a part with a condition.
    pub limb: Option<LimbHit>,
}

fn finite_vec3(v: [f32; 3]) -> Option<[f32; 3]> {
    v.iter().all(|c| c.is_finite()).then_some(v)
}

/// Normalize a game-space incoming hit travel direction (`+X` east, `+Y` north, `+Z` up)
/// to a unit 3D vector `[nx, ny, nz]` (`||v|| == 1.0`) for `GetHit3DData 1` and elements
/// `3..6` of `GetHit3DData 2` [observed: 2026-11-23, GSDocMitchellHouse (0x00103DF9),
/// Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 (10 official base masters),
/// #3678 / PR #3679 probe `w1_3606_hit_3d_dir_and_base_damage`: South->North `+Y` and
/// East->West `-X` 10mm shots on restrained Doc Mitchell]. Returns `None` for non-finite
/// or zero-length vectors.
pub fn unit_hit_dir(dir: [f32; 3]) -> Option<[f32; 3]> {
    let dir = finite_vec3(dir)?;
    let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    if !len.is_finite() || len <= 1e-6 {
        return None;
    }
    finite_vec3([dir[0] / len, dir[1] / len, dir[2] / len])
}

/// The limb-condition actor value code of a BPTD part's actor value name.
pub fn limb_condition(name: &str) -> Option<u16> {
    actor::av_by_name(name).filter(|c| LIMB_CONDITIONS.contains(c))
}

/// Perception Condition (the head), Endurance Condition (the torso), Left and Right
/// Attack Condition (the arms) and Left and Right Mobility Condition (the legs)
/// [documented: GECK wiki Actor Value list, codes 25–30].
pub const HEAD: u16 = 25;
pub const TORSO: u16 = 26;
pub const LEFT_ARM: u16 = 27;
pub const RIGHT_ARM: u16 = 28;
pub const LEFT_MOBILITY: u16 = 29;
pub const RIGHT_MOBILITY: u16 = 30;

/// The share of a blast's Health damage the torso loses, in condition points
/// (docs/spec-notes/crippled-limbs.md §5) [observed 2026-11-06, o25_limbs: 1/30 for five
/// frag mine and grenade blasts].
pub const BLAST_TORSO_SHARE: f64 = 1.0 / 30.0;
/// The share of a blast's Health damage the legs lose together, in condition points
/// [observed: 0.24 for the same five blasts].
pub const BLAST_LEGS_SHARE: f64 = 0.24;

/// An explosion's limb damage (docs/spec-notes/crippled-limbs.md §5) on an actor whose
/// body part data names the limb conditions `conditions`, for `damage` (the blast's
/// damage to Health): condition points, as a fall's leg ([`LimbHit::points`]). The torso
/// takes [`BLAST_TORSO_SHARE`] of it and the legs [`BLAST_LEGS_SHARE`] together, the left
/// leg two thirds of that and the right one third (all of it on the one leg when the data
/// has one); the head, arms and brain take nothing [observed: the shares and the
/// untouched parts, frag mines and a grenade at a human's feet; hypothesis: the
/// left/right split, which the original varied between all-left and 2:1, and that a
/// blast at any height or on any body does the same]. The points come straight off the
/// condition, not as a share of a pool, at any maximum Health and with no player limb
/// multiplier [hypothesis: measured only on one NPC at 5000 Health; a share of the
/// limb's pool fits the same numbers, observer check 74].
pub fn blast_limb_shares(conditions: &[u16], damage: f64) -> Vec<LimbHit> {
    if damage <= 0.0 {
        return Vec::new();
    }
    let has = |c: u16| conditions.contains(&c);
    let legs = match (has(LEFT_MOBILITY), has(RIGHT_MOBILITY)) {
        (true, true) => vec![(LEFT_MOBILITY, BLAST_LEGS_SHARE * 2.0 / 3.0), (RIGHT_MOBILITY, BLAST_LEGS_SHARE / 3.0)],
        (true, false) => vec![(LEFT_MOBILITY, BLAST_LEGS_SHARE)],
        (false, true) => vec![(RIGHT_MOBILITY, BLAST_LEGS_SHARE)],
        (false, false) => Vec::new(),
    };
    has(TORSO)
        .then_some((TORSO, BLAST_TORSO_SHARE))
        .into_iter()
        .chain(legs)
        .map(|(condition, share)| LimbHit { condition, health_percent: 0.0, damage: damage * share, points: true })
        .collect()
}

/// `fCombatPlayerLimbDamageMult` when the load order lacks it [observed: FalloutNV.esm].
pub const PLAYER_LIMB_DAMAGE_MULT: f64 = 0.5;
/// `fMoveOneCrippledLegSpeedMult` and `fMoveTwoCrippledLegsSpeedMult` when the load
/// order lacks them [observed: FalloutNV.esm].
pub const ONE_LEG_SPEED_MULT: f64 = 0.85;
pub const TWO_LEGS_SPEED_MULT: f64 = 0.75;

/// The share of its speed an actor keeps with `crippled` legs crippled (0, 1 or 2).
pub fn leg_speed(g: &Game, crippled: usize) -> f64 {
    match crippled {
        0 => 1.0,
        1 => setting(g, "fMoveOneCrippledLegSpeedMult", ONE_LEG_SPEED_MULT),
        _ => setting(g, "fMoveTwoCrippledLegsSpeedMult", TWO_LEGS_SPEED_MULT),
    }
    .clamp(0.0, 1.0)
}

/// `fMoveMediumArmorPenalty` and `fMoveHeavyArmorPenalty` when the load order lacks
/// them [observed: 0.075 and 0.15 in FalloutNV.esm].
pub const MEDIUM_ARMOR_MOVE_PENALTY: f64 = 0.075;
pub const HEAVY_ARMOR_MOVE_PENALTY: f64 = 0.15;

/// The share of its speed an actor keeps in body armour of class `class`: 1 in light
/// armour (or none), 1 − `fMoveMediumArmorPenalty` in medium, 1 − `fMoveHeavyArmorPenalty`
/// in heavy [observed: the AI observer in the original game, NPCs walking holstered
/// (docs/spec-notes/actors.md §8.10): light armour 85.5 units/s (median of the actors'
/// medians), medium 77–79 (85.3 × 0.925 = 78.9), power armour 68–72 (85.3 × 0.85 =
/// 72.5); running, a medium-armoured initiate 316 (344 × 0.925 = 318)].
pub fn armor_speed(g: &Game, class: fnv_sim::forms::ArmorClass) -> f64 {
    use fnv_sim::forms::ArmorClass;
    match class {
        ArmorClass::Light => 1.0,
        ArmorClass::Medium => 1.0 - setting(g, "fMoveMediumArmorPenalty", MEDIUM_ARMOR_MOVE_PENALTY),
        ArmorClass::Heavy => 1.0 - setting(g, "fMoveHeavyArmorPenalty", HEAVY_ARMOR_MOVE_PENALTY),
    }
    .clamp(0.1, 1.0)
}

/// The class of the body armour (upper-body slot) actor `id` wears by the rules
/// (`World::equipment`: its equipped items, before its first outfitting the ones that will
/// be, as the engine draws it); light without any.
pub fn worn_armor_class(world: &fnv_sim::World, id: FormId) -> fnv_sim::forms::ArmorClass {
    use fnv_sim::forms::{ArmorClass, ItemKind};
    world
        .equipment(id)
        .unwrap_or_default()
        .into_iter()
        .find(|&i| {
            world.resolve_item(i).is_some_and(|it| it.kind == ItemKind::Armor)
                && world.effective_slots(i).is_some_and(|s| s & crate::detection::UPPER_BODY != 0)
        })
        .and_then(|i| world.effective_armor_class(i))
        .unwrap_or(ArmorClass::Light)
}

/// Settings with their documented defaults (GECK wiki Experience Settings).
pub(crate) fn setting(g: &Game, name: &str, default: f64) -> f64 {
    g.world.setting_f32(name).map_or(default, |v| v as f64)
}

impl Game<'_> {
    /// The player's level.
    pub fn player_level(&self) -> f64 {
        self.player.level.unwrap_or(self.world.config().player_level as i32) as f64
    }

    /// An actor's level (see the module notes); SetLevel's value wins.
    pub fn actor_level(&self, r: FormId) -> f64 {
        if r == FormId::PLAYER_REF {
            return self.player_level();
        }
        if let Some(l) = self.extra.get(&r).and_then(|e| e.level) {
            return l as f64;
        }
        let Some((id, d)) = self.stats_source(r) else { return 1.0 };
        if self.acbs_flag(id, d, crate::actor_values::acbs::PC_LEVEL_MULT, |d| d.pc_level_mult) {
            let l = (self.player_level() * d.level_raw as f64 / 1000.0).round().max(d.calc_min as f64);
            if d.calc_max > 0 {
                l.min(d.calc_max as f64)
            } else {
                l
            }
            .max(1.0)
        } else {
            (d.level_raw as f64).max(1.0)
        }
    }

    /// Maximum health before any script changed it (see the module notes).
    pub fn max_health(&mut self, r: FormId) -> f64 {
        let level = self.actor_level(r);
        let Some((id, d)) = self.stats_source(r) else { return 100.0 };
        // The permanent value, as before the other actor values were read
        // [hypothesis: whether damage to Endurance lowers the maximum].
        let end = actor::actor_value_base(self, r, crate::actor_values::ENDURANCE);
        if r == FormId::PLAYER_REF {
            return setting(self, "fAVDHealthEnduranceOffset", 0.0)
                + end * setting(self, "fAVDHealthEnduranceMult", 20.0)
                + (level - 1.0).max(0.0) * setting(self, "fAVDHealthLevelMult", 5.0)
                + d.base_health as f64;
        }
        if &d.sig == b"CREA" {
            return (d.base_health as f64).max(1.0);
        }
        let autocalc = self.acbs_flag(id, d, crate::actor_values::acbs::AUTOCALC, |d| d.autocalc);
        let by_level = if autocalc { (level - 1.0).max(0.0) * setting(self, "fAVDNPCHealthLevelMult", 5.0) } else { 0.0 };
        (d.base_health as f64 + (end - 1.0).max(0.0) * setting(self, "fAVDNPCHealthEnduranceMult", 5.0) + by_level).max(1.0)
    }

    pub fn is_dead(&self, r: FormId) -> bool {
        self.extra.get(&r).is_some_and(|e| e.dead)
    }

    /// Whether the placed actor `r` starts the game as a corpse: the actor that supplies
    /// its stats (its base, or the template while the template flag "Stats" is set) has
    /// a Base Health of 0 (NPC_), or a Health of 0 (CREA) [documented for NPCs: GECK wiki
    /// Stats Tab - NPC and Category:NPC, "If Base Health is set to 0 the actor will spawn
    /// as a corpse"; hypothesis for creatures, whose DATA Health stands where an NPC's
    /// Base Health does; docs/spec-notes/dead-on-spawn.md].
    pub fn starts_dead(&self, r: FormId) -> bool {
        let placed = self.world.placed.get(r);
        placed.is_some_and(|p| matches!(&p.sig, b"ACHR" | b"ACRE")) && self.stats_def(r).is_some_and(|d| d.zero_health)
    }

    /// Whether `r`'s base has "No Low Level Processing": its AI is not updated outside
    /// the loaded area [documented: GECK wiki Category:NPC].
    pub fn no_low_level_processing(&self, r: FormId) -> bool {
        self.base_data_source(r).is_some_and(|(id, d)| self.acbs_flag(id, d, crate::actor_values::acbs::NO_LOW_LEVEL, |d| d.no_low_level))
    }

    /// Lay out the corpses of a new game: every placed actor that starts dead
    /// ([`Game::starts_dead`]) is dead from the start, without a killer, an OnDeath or a
    /// count in GetDeadCount [hypothesis: nobody killed it]. Returns how many.
    pub fn lay_out_corpses(&mut self) -> usize {
        let placed = self.world.placed;
        let dead: Vec<FormId> = placed.refs.keys().copied().filter(|&r| self.starts_dead(r)).collect();
        for &r in &dead {
            self.extra.entry(r).or_default().dead = true;
            // Its death item, read when its inventory is first made (`crate::death_items`).
            self.owe_death_item(r);
        }
        dead.len()
    }

    fn is_essential(&self, r: FormId) -> bool {
        actor::essential(self, r)
    }

    /// Whether the actor takes no damage: its race has the Child flag, the creature is
    /// Invulnerable, or a script made it a ghost (SetGhost: "cannot be harmed by a weapon
    /// or spell/magic effect in any way" [documented: GECK wiki SetGhost notes]). A
    /// ghosted player is still hurt: "Though NPCs and creatures can not be damaged in this
    /// state, the player can" [documented: GECK wiki SetGhost notes].
    pub fn is_immune(&self, r: FormId) -> bool {
        let invulnerable = self
            .base_data_source(r)
            .is_some_and(|(id, d)| &d.sig == b"CREA" && self.acbs_flag(id, d, crate::actor_values::acbs::INVULNERABLE, |d| d.invulnerable));
        let ghost = r != FormId::PLAYER_REF && self.is_ghost(r);
        invulnerable || ghost || self.actor_race(r).is_some_and(|race| self.data.child_races.contains(&race))
    }

    /// A creature's own attack: its damage (CREA DATA Damage) and its reach (RNAM Attack
    /// Reach) [documented: xEdit field names], from the actor that supplies its stats
    /// (template flag Stats) [hypothesis for the reach: which template group RNAM belongs
    /// to is not checked]. `None` for an NPC.
    pub fn creature_attack(&self, r: FormId) -> Option<(f64, Option<f64>)> {
        // A base form reads its own (a script's base form argument).
        let (id, d) = self.template_source(base_of(self, r).unwrap_or(r), r, |d| d.template_stats)?;
        if &d.sig != b"CREA" {
            return None;
        }
        // SetCreatureDamage's change first (`crate::nvse_jip6`).
        let damage = match self.field_overrides.get(&(id, "DATA/damage".to_string())) {
            Some(fnv_script::vm::XVal::Num(x)) => *x,
            _ => d.attack_damage.unwrap_or(0) as f64,
        };
        Some((damage, d.attack_reach.map(f64::from)))
    }

    /// The actor dies, of no hit: dead flag, killer, GetDeadCount, OnDeath (KillActor's
    /// effect). False when it was already dead.
    pub fn kill(&mut self, r: FormId, killer: Option<FormId>) -> bool {
        self.die(r, killer, None)
    }

    /// [`Game::kill`], with the hit that killed if one did (`RefExtra::killing_hit`, set
    /// before anything of the death runs). The player's own death is no kill to count, by
    /// anyone's hand [design: nobody has looked at what the original counts].
    fn die(&mut self, r: FormId, killer: Option<FormId>, fatal: Option<crate::game::LastHit>) -> bool {
        if self.is_dead(r) {
            return false;
        }
        // A co-op guest's kill is the player's, as a companion's is: GetKiller, IsKiller
        // player, OnDeath's killer, kill counts and karma all see the player, so the host's
        // quests move on exactly as for its own kill [design].
        let raw_killer = killer;
        let killer = killer.map(|k| self.as_player(k));
        // Karma and murder are judged while the victim still lives (`crate::crime`). A
        // guest's karma, kill count and kill challenges are its own character's, queued
        // for it rather than given to the host's player (`crate::coop_kills`; coop-shared-progress.md §4.2) [design].
        if killer == Some(FormId::PLAYER_REF) && r != FormId::PLAYER_REF {
            match raw_killer.filter(|&k| self.is_coop_actor(k)) {
                Some(guest) => {
                    let from = self.crimes.notes.len();
                    let karma = self.player_kills_karma(r, false);
                    let infamy = self.crimes.notes[from.min(self.crimes.notes.len())..]
                        .iter()
                        .filter_map(|n| match n {
                            crate::crime::Note::Infamy { reputation, before, after } if after != before => Some((*reputation, after - before)),
                            _ => None,
                        })
                        .collect();
                    self.coop_guest_killed(r, guest, karma, infamy);
                }
                None => {
                    self.player_kills(r);
                    self.count_kill(r);
                    self.challenge_kill(r);
                }
            }
        }
        // A body already lying knocked out is a ragdoll already [hypothesis].
        let lying = self.knockout(r).is_some_and(|k| k.lying());
        let day = self.game_days();
        let e = self.extra.entry(r).or_default();
        e.dead = true;
        e.died_day = day;
        e.killer = killer;
        e.killing_hit = fatal;
        // A dead actor immediately stops detecting on the same tick
        // [observed: w4g_05_isactordetected_and_setclass_invalid].
        self.detection.forget_detector(r);
        // JIP's ragdoll handlers: any actor ragdolled on death [documented:
        // SetOnRagdollEventHandler].
        if !lying {
            self.jip_event("ragdoll", Some(r), Vec::new());
        }
        if let Some(b) = base_of(self, r) {
            *self.player.dead_count.entry(b).or_default() += 1;
        }
        // The body gets its death item before OnDeath runs [hypothesis: the order]
        // (`crate::death_items`).
        self.give_death_items(r, killer);
        // OnDeath waits for the script that killed to finish (`Game::on_death`).
        if self.in_script() {
            self.defer_death(r, killer);
        } else {
            self.on_death(r, killer);
        }
        // Its effects end, but those whose base effect has "No Death Dispel" (a flamer's
        // burning) and abilities (docs/spec-notes/hit-effects.md §4).
        let keep: Vec<u32> = self
            .effects
            .iter()
            .filter(|e| e.target == r && (e.permanent || self.magic_effect_def(e.effect).is_some_and(|m| m.has(MagicEffectDef::NO_DEATH_DISPEL))))
            .map(|e| e.id)
            .collect();
        self.dispel(r, None, |e| !keep.contains(&e.id));
        self.death_line(r, killer);
        true
    }

    /// The OnDeath of `r`: the extender's handlers, then the block. A death inside a
    /// script run (KillActor) runs it once the outermost script has finished, not in the
    /// middle of the killing script [hypothesis: One for My Baby's victim scripts call
    /// `KillActor` and only then set Boone's `bGotRevenge`, while their OnDeath clears
    /// the accused (`nAccusedIndex`) when `bGotRevenge` is still 0; run at once, every
    /// revenge kill would clear it and Boone's "I found the bill of sale" line, which
    /// needs it, would never show].
    pub(crate) fn on_death(&mut self, r: FormId, killer: Option<FormId>) {
        // The extender's OnDeath handlers, before the block [hypothesis on the order;
        // docs/spec-notes/ext-events.md].
        let killer_val = fnv_script::vm::XVal::Ref(killer.map_or(0, |k| k.0));
        self.dispatch_event("OnDeath", vec![fnv_script::vm::XVal::Ref(r.0), killer_val], Some(r));
        self.jg_event(crate::nvse_jip3::JgEvent::Dying, Some(r));
        self.ref_event(r, event::ON_DEATH, killer.map(|k| Value::Ref(k.0)), killer);
        // "Teammate state is cleared on death of the actor" [documented: GECK wiki
        // SetPlayerTeammate]; after OnDeath, which may still ask [hypothesis: the order].
        if let Some(e) = self.extra.get_mut(&r) {
            e.teammate = false;
        }
    }

    /// The share (0–1) of a harmful effect's magnitude the target resists: its value of
    /// the base effect's resistance, as a percentage [documented: GECK wiki Stats Tab -
    /// Creatures, Poison Resist; hypothesis: the same for every resistance, Fire
    /// Resistance among them] (docs/spec-notes/hit-effects.md §3).
    pub fn resisted(&mut self, target: FormId, m: Option<&MagicEffectDef>) -> f64 {
        let Some(m) = m.filter(|m| m.has(MagicEffectDef::HOSTILE) || m.has(MagicEffectDef::DETRIMENTAL)) else { return 0.0 };
        let Some(code) = m.resistance else { return 0.0 };
        (actor::actor_value(self, target, code).unwrap_or(0.0) / 100.0).clamp(0.0, 1.0)
    }

    /// The magic item a hit applies with its effects: a weapon's object effect, a
    /// blast's, an actor's unarmed attack effect (docs/spec-notes/hit-effects.md §1). A
    /// dead or unharmable target takes none. The target's resistances cut the harmful
    /// magnitudes, a fully resisted effect does not start, and a hit with the same item
    /// while its effect lasts starts that effect's time again (§2, §3). The hit's damage
    /// comes next, and it decides a death by a Script effect's damage
    /// ([`Game::health_lowered`]).
    pub fn apply_hit_effect(&mut self, target: FormId, item: FormId, caster: Option<FormId>) -> Vec<crate::aid::Applied> {
        self.hits_running.push(target);
        let applied = self.start_hit_effect(target, item, caster);
        self.hits_running.pop();
        applied
    }

    /// [`Game::apply_hit_effect`] with no hit to come: a Script effect's damage kills at once.
    pub(crate) fn start_hit_effect(&mut self, target: FormId, item: FormId, caster: Option<FormId>) -> Vec<crate::aid::Applied> {
        if self.is_dead(target) || self.is_immune(target) {
            return Vec::new();
        }
        let mods = crate::aid::Mods { hit: true, ..crate::aid::Mods::default() };
        let applied = self.apply_magic_item_with(target, item, caster, false, &mods);
        // OnFire, Poisoned (`crate::combat_lines`).
        self.effect_started_lines(target, caster, &applied);
        applied
    }

    /// An actor's unarmed attack effect and the attack animation it needs (`EITM`,
    /// `EAMT`), from the actor base that supplies its actor effects (template flag "Actor
    /// Effect List") [hypothesis: the group, data.rs `template_spells`].
    pub fn unarmed_effect(&self, r: FormId) -> Option<(FormId, Option<&str>)> {
        let base = base_of(self, r)?;
        let (src, d) = self.template_source(base, r, |d| d.template_spells)?;
        // SetActorUnarmedEffect's first (`crate::nvse_jip11`).
        Some((self.unarmed_override(src).or(d.unarmed_effect)?, d.unarmed_anim.as_deref()))
    }

    /// Whether a kill by `killer` earns the player experience: the player's, and a hired
    /// companion's (a teammate's), the same experience as the player's own kill [owner,
    /// 2026-10-27: "companion kills should net you experience if they are hired"]. Their
    /// damage counts together towards the share kill XP asks for ([`Game::kill_xp`])
    /// [design].
    pub fn kill_earns_xp(&mut self, killer: FormId) -> bool {
        // The player's tweak: a teammate's kill is not the player's [design: docs/design/tweaks.md].
        if killer == FormId::PLAYER_REF || (!self.tweaks.no_companion_kill_xp && self.is_teammate(killer)) {
            return true;
        }
        // The player's tweak: a frenzied actor's kill is the player's; Aggression 3 is
        // Frenzied [documented: GECK wiki Aggression; design: docs/design/tweaks.md].
        self.tweaks.frenzy_kill_xp && actor::actor_value(self, killer, crate::actor_values::AGGRESSION).is_ok_and(|v| v >= 3.0)
    }

    /// Health damage an effect does (a burn, a poison): as a hit's, kept at 1 for an
    /// essential actor (who falls unconscious) and killing the others at 0, the effect's
    /// caster the killer (docs/spec-notes/hit-effects.md §4).
    pub(crate) fn effect_health_damage(&mut self, target: FormId, amount: f64, caster: Option<FormId>) {
        if self.is_dead(target) || self.is_immune(target) {
            return;
        }
        let before = actor::actor_value(self, target, HEALTH).unwrap_or(0.0);
        let essential = self.is_essential(target);
        let floor = if essential { 1.0 } else { 0.0 };
        // Scaled by the difficulty as a hit is (`crate::difficulty`).
        let amount = amount * self.damage_mult(caster, target);
        let dealt = amount.max(0.0).min((before - floor).max(0.0));
        actor::add_damage(self, target, HEALTH, dealt);
        let players_side = caster.is_some_and(|c| self.kill_earns_xp(c) || self.is_coop_actor(c));
        if let Some(c) = caster {
            self.note_player_hit(target, c, dealt);
        }
        {
            let e = self.extra.entry(target).or_default();
            if players_side {
                e.player_damage += dealt;
            }
            e.total_damage += dealt;
        }
        if before - dealt <= floor {
            if essential {
                self.knock_out(target, crate::unconscious::Cause::Essential, caster);
            } else if self.kill(target, caster) {
                self.kill_credit(target, caster);
            }
        }
    }

    /// A script lowered `r`'s Health (DamageActorValue): at 0 or less it dies, an essential
    /// actor is knocked out [hypothesis: unconscious.md §2.2, observer-checks.md check 89].
    /// Nobody is credited, so no XP, karma or challenge. A god-mode player stands [design], and so does an actor whose record gives
    /// no Health, as its Health is only `max_health`'s stand-in of 1. During a hit on `r`
    /// the hit decides, so that a kill finished by an OnHit script or by a Script effect the
    /// hit brings is still the attacker's (the boxing gloves' `DamageAV Health 10`,
    /// unconscious.md §1).
    pub(crate) fn health_lowered(&mut self, r: FormId) {
        if self.is_dead(r) || self.is_immune(r) || self.hits_running.contains(&r) {
            return;
        }
        let player = r == FormId::PLAYER_REF;
        // Only actors whose record gives a Health have one to run out of.
        let has_health = self.stats_def(r).is_some_and(|d| d.base_health != 0 || d.zero_health);
        // ToggleImmortalMode 1 and 2 keep the player alive too (`crate::nvse_jip9`).
        if (player && (self.toggles.god_mode || self.immortal_mode() > 0)) || (!player && !has_health) {
            return;
        }
        if !actor::actor_value(self, r, HEALTH).is_ok_and(|v| v <= 0.0) {
            return;
        }
        if self.is_essential(r) && !player {
            self.knock_out(r, crate::unconscious::Cause::Essential, None);
        } else {
            self.kill(r, None);
        }
    }

    /// Apply `damage` health damage to `target` (see the module notes).
    pub fn hit(&mut self, target: FormId, damage: f64, mut info: HitInfo) -> HitResult {
        // The difficulty scales the player's hits and the hits on the player, limb damage
        // too (`crate::difficulty`, docs/spec-notes/difficulty.md §3).
        let mult = self.damage_mult(info.attacker, target);
        let base = damage;
        let damage = damage * mult;
        if let Some(l) = info.limb.as_mut() {
            l.damage *= mult;
        }
        let max_health = actor::actor_value_base(self, target, HEALTH);
        if self.is_dead(target) || self.is_immune(target) {
            let health = actor::actor_value(self, target, HEALTH).unwrap_or(0.0);
            let corpse = self.is_dead(target);
            return HitResult { health, max_health, immune: true, corpse, overkill: if corpse { damage.max(0.0) } else { 0.0 }, ..HitResult::default() };
        }
        // Every hit the difficulty changes, so a run shows what each level does to real hits
        // (weapons, blows, blasts, falls), not only to the test aid's. Debug, under its own
        // target: a run asks for it with RUST_LOG=info,fnv_game::difficulty=debug.
        if mult != 1.0 && base > 0.0 {
            let by = info.attacker.map_or_else(|| "nobody".to_string(), |a| a.to_string());
            tracing::debug!(target: "fnv_game::difficulty", "difficulty: a hit of {base:.1} on {target} by {by} is {damage:.1} (x{mult:.2})");
        }
        let before = actor::actor_value(self, target, HEALTH).unwrap_or(max_health);
        let essential = self.is_essential(target);
        // ToggleImmortalMode [documented]: 1 keeps the player's Health at 1 or more, as an
        // essential actor's; 2 takes no Health or limb damage at all (`crate::nvse_jip9`).
        let immortal = if target == FormId::PLAYER_REF { self.immortal_mode() } else { 0 };
        if immortal == 2 {
            if let Some(l) = info.limb.as_mut() {
                l.damage = 0.0;
            }
        }
        let damage = if immortal == 2 { 0.0 } else { damage };
        let floor = if essential || immortal == 1 { 1.0 } else { 0.0 };
        let dealt = damage.max(0.0).min((before - floor).max(0.0));
        // JIP's hit record, then its hit handlers, before the damage [documented:
        // SetOnHitEventHandler; observed: jip-hits, Health in the handler is the value
        // before the hit] (jip-hit-info.md).
        let fatal = before - dealt <= floor;
        self.hits_running.push(target);
        let flags = u16::from(info.blocking)
            | (u16::from(info.critical) << 1)
            | (u16::from(fatal) << 2)
            | (u16::from(info.sneak_attack) << 7)
            | (u16::from(info.explosion) << 8);
        let limb_damage = info.limb.map_or(0.0, |l| l.damage);
        let fatigue_damage = if info.fatigue_damage.is_finite() { info.fatigue_damage.max(0.0) } else { 0.0 };
        let base_weapon_damage = if info.base_weapon_damage.is_finite() { info.base_weapon_damage.max(0.0) } else { 0.0 };
        let (hit_pos, hit_dir) = if info.explosion {
            (Some([0.0; 3]), Some([0.0; 3]))
        } else {
            (info.hit_pos.and_then(finite_vec3), info.hit_dir.and_then(unit_hit_dir))
        };
        let rec = crate::jip_events::JipHit {
            attacker: info.attacker,
            weapon: info.weapon,
            projectile: info.projectile,
            health_damage: dealt,
            limb_damage,
            fatigue_damage,
            base_weapon_damage,
            location: info.location,
            flags,
            hit_pos,
            hit_dir,
        };
        self.jip.hits.insert(target, rec);
        self.jip_event("hit", Some(target), Vec::new());
        actor::add_damage(self, target, HEALTH, dealt);
        let players_side = info.attacker.is_some_and(|a| self.kill_earns_xp(a) || self.is_coop_actor(a));
        if let Some(a) = info.attacker {
            self.note_player_hit(target, a, dealt);
        }
        let this_hit = crate::game::LastHit { attacker: info.attacker, weapon: info.weapon, location: info.location, critical: info.critical };
        {
            let e = self.extra.entry(target).or_default();
            e.last_hit = Some(this_hit);
            if players_side {
                e.player_damage += dealt;
            }
            e.total_damage += dealt;
        }
        if let Some(a) = info.attacker {
            self.extra.entry(a).or_default().last_target = Some(target);
        }
        if info.critical {
            // [documented: SetOnCriticalHitEventHandler] this = struck actor;
            // arguments: source, weapon. Fired after `last_hit` is recorded so
            // `GetHitLocation` reads this hit as in the wiki example.
            let source = fnv_script::vm::XVal::Ref(info.attacker.map_or(0, |a| a.0));
            let weapon = fnv_script::vm::XVal::Ref(info.weapon.map_or(0, |w| w.0));
            self.jip_event("criticalhit", Some(target), vec![source, weapon]);
        }
        if dealt > 0.0 {
            // [documented: SetOnHealthDamageEventHandler] the amount negative, the source.
            let source = fnv_script::vm::XVal::Ref(info.attacker.map_or(0, |a| a.0));
            self.jip_event("healthdamage", Some(target), vec![fnv_script::vm::XVal::Num(-dealt), source]);
        }
        self.ref_event(target, event::ON_HIT, info.attacker.map(|a| Value::Ref(a.0)), info.attacker);
        if let Some(w) = info.weapon {
            self.ref_event(target, event::ON_HIT_WITH, Some(Value::Ref(w.0)), info.attacker);
        }
        // The weapon's own OnHit block (the boxing gloves' fatigue damage,
        // docs/spec-notes/unconscious.md §2.9) [hypothesis: after the target's blocks].
        if let (Some(a), Some(w)) = (info.attacker, info.weapon) {
            self.item_event(a, w, event::ON_HIT, Some(Value::Ref(target.0)));
        }
        let health = actor::actor_value(self, target, HEALTH).unwrap_or(before - dealt);
        self.hits_running.pop();
        let mut out = HitResult { dealt, health, max_health, ..HitResult::default() };
        if let Some(l) = info.limb {
            let (condition, crippled) = self.hit_limb(target, l, max_health);
            out.limb = Some((l.condition, condition));
            out.crippled = crippled;
        }
        // The player's hit counts towards the hit and damage challenges (`crate::challenges`).
        if info.attacker == Some(FormId::PLAYER_REF) {
            let part = crate::challenges::body_part(info.limb.map(|l| l.condition), info.location);
            self.challenge_hit(target, info.weapon, part, dealt, info.critical, out.crippled);
        } else if let Some(guest) = info.attacker.filter(|&a| self.is_coop_actor(a)) {
            // A guest's hit counts towards its own character's challenges (`crate::coop_kills`).
            let part = crate::challenges::body_part(info.limb.map(|l| l.condition), info.location);
            self.coop_kills.hits_out.push(crate::coop_kills::HitCredit { player: guest, target, weapon: info.weapon, part, dealt, critical: info.critical, crippled: out.crippled });
        }
        // Immortal mode 1 holds the player at 1 HP: no death, no knockout.
        if health <= floor && !self.is_dead(target) && immortal == 0 {
            if essential {
                // Said its KOed line when knocked out now (`Game::knock_out`).
                out.knocked_out = self.knock_out(target, crate::unconscious::Cause::Essential, info.attacker);
            } else {
                // Also when an OnHit script or the hit's Script effect took the last of the
                // Health ([`Game::health_lowered`]): this hit decides, so it is the killing one.
                out.died = self.die(target, info.attacker, Some(this_hit));
                if out.died {
                    out.overkill = (damage - dealt).max(0.0);
                }
                if out.died {
                    out.xp = self.kill_credit(target, info.attacker);
                }
                // The player's tweak: a kill gives Action Points back [design: docs/design/tweaks.md].
                if out.died && info.attacker == Some(FormId::PLAYER_REF) && self.tweaks.kill_reward_ap > 0.0 {
                    crate::functions::restore_actor_value_damage(self, FormId::PLAYER_REF, crate::actor_values::ACTION_POINTS, self.tweaks.kill_reward_ap);
                }
            }
        }
        // The player's palm strike in VATS may paralyse (`crate::magic_archetypes`); its
        // roll is a hash of the time and the target, so no other roll moves [design].
        if info.attacker == Some(FormId::PLAYER_REF) && !out.died && self.vats.mode == crate::vats::MODE_PLAYBACK {
            let roll = crate::magic_archetypes::hash_roll(self.time, target);
            self.paralyzing_palm(target, info.weapon, roll);
            out.health = actor::actor_value(self, target, HEALTH).unwrap_or(out.health);
        }
        // What the target says (`crate::combat_lines`): Death was said by `kill`, KOed by
        // `knock_out`; nothing more from one lying knocked out.
        if !out.died && self.knockout(target).is_none() {
            self.hit_lines(target, info.attacker, before, health, max_health, out.crippled);
        }
        out
    }

    /// A hit on a reference that is not an actor (a bottle, a target, a switch): its
    /// OnHit blocks run with the attacker and its OnHitWith blocks with the weapon, as
    /// for an actor; nothing else changes (object damage is [`Game::damage_object`]).
    /// Returns whether a block ran.
    ///
    /// [documented: GECK wiki Event Handling, the target of OnHit and OnHitWith is "any
    /// reference"; observed: the tutorial's bottles (MISC `VCG02Bottle`, script
    /// `VCG02TargetSCRIPT`) count the player's shots in an OnHitWith block without a
    /// parameter. Hypothesis: OnHit's subject is the attacking actor here too — that page
    /// gives a projectile reference for a ranged hit on a non-actor, and we have no
    /// projectile references.]
    ///
    /// A co-op guest's shot (its actor the attacker) is the player's, as a companion's
    /// kill is: the blocks see the player as the attacker, and the player's weapon
    /// questions answer for the guest's shot ([`Game::shot_for_player`]), so a placed
    /// object a script counts the player's hits on counts a guest's too [design].
    pub fn hit_object(&mut self, target: FormId, info: HitInfo) -> bool {
        let guest = info.attacker.filter(|&a| self.is_coop_actor(a));
        let attacker = info.attacker.map(|a| self.as_player(a));
        let was = self.shot_for_player;
        if guest.is_some() {
            self.shot_for_player = info.weapon.or(Some(FormId(0)));
        }
        let mut ran = self.ref_event(target, event::ON_HIT, attacker.map(|a| Value::Ref(a.0)), attacker);
        if let Some(w) = info.weapon {
            ran |= self.ref_event(target, event::ON_HIT_WITH, Some(Value::Ref(w.0)), attacker);
        }
        self.shot_for_player = was;
        ran
    }

    /// A hit that deals no damage: a Use Weapon package's or UseWeapon's Do No Damage
    /// ("prevent all damage", "for scripted protracted fake combat" [documented: GECK
    /// wiki UseWeapon, docs/spec-notes/use-weapon.md §1]). The target's OnHit and
    /// OnHitWith blocks run with the attacker and the weapon, as for any hit; nothing else
    /// changes: no Health, limb or fatigue damage, no hit lines, no last hit, no JIP hit
    /// event. Returns whether a block ran. [observed: G.I. Blues' thugs fall only in their
    /// OnHit block for Orris (SCPT 0x00118A77), whose Use Weapon packages are Always Hit and
    /// Do No Damage, so the scene goes on only if such a hit runs it (#2433); hypothesis:
    /// that is what the original does, observer check 134.]
    pub fn harmless_hit(&mut self, target: FormId, info: HitInfo) -> bool {
        self.hit_object(target, info)
    }

    /// Tell the rules which weapon the animating side draws for `actor`
    /// (`RefExtra::held_weapon`).
    pub fn set_held_weapon(&mut self, actor: FormId, weapon: Option<FormId>) {
        if self.extra.get(&actor).and_then(|e| e.held_weapon) != weapon {
            self.extra.entry(actor).or_default().held_weapon = weapon;
        }
    }

    /// Tell the scripts whether an actor has its weapon out (IsWeaponOut). The animating
    /// side tells it every frame for the actors it draws; for the others the rule decides
    /// ([`Game::wants_weapon_out`]). If transitioning from holstered (`false`) to drawn
    /// (`true`) without an earlier draw-start dispatch (`showoff_begin_unholster`),
    /// dispatches `ShowOff:OnWeaponUnholster` PRE-update so `IsWeaponOut` reads `0`
    /// inside the callback [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 +
    /// ShowOffNVSE Plugin 184, 2026-11-23, GSDocMitchellHouse (0x00103DF9), 10 official
    /// base masters, probe `w1_3656_objectives_unholster_imod` (`Player.SetWeaponOut 0` /
    /// `Player.SetWeaponOut 1` with `Weap10mmPistol` `0x0000434F` and unarmed) in #3679].
    pub fn set_weapon_out(&mut self, actor: FormId, out: bool) {
        self.senses.weapon_told.insert(actor, self.time);
        let was = self.extra.get(&actor).is_some_and(|e| e.weapon_out);
        if was != out {
            let already_fired = self.showoff.unholster_pending.remove(&actor).is_some();
            if out && !already_fired {
                self.showoff_on_weapon_unholster(actor);
            }
            self.extra.entry(actor).or_default().weapon_out = out;
        } else if !out {
            // Do NOT clear active draw-start deduplication (`None`) while a draw is still
            // in progress (`weapon_state` / `idles.rs` call `set_weapon_out(actor, false)`
            // every frame until draw completes). Only clear a stale `Some(ended)` latch
            // left by an `AnimAction::Equip` that ended on an earlier frame without
            // completing and with no scripted draw pending.
            let draw_due = self.hud.weapon_due.get(&actor).is_some_and(|(o, _, _)| *o);
            if !draw_due
                && self
                    .showoff
                    .unholster_pending
                    .get(&actor)
                    .is_some_and(|ended| ended.is_some_and(|f| f < self.frame))
            {
                self.showoff.unholster_pending.remove(&actor);
            }
        }
    }

    /// Whether a living actor has its weapon out now, as IsWeaponOut reads it
    /// (`functions::actor::is_weapon_out`): what the animating side told in the last second
    /// ([`Game::set_weapon_out`]; the player's always), else the rule
    /// ([`Game::wants_weapon_out`]).
    pub fn weapon_is_out(&self, actor: FormId) -> bool {
        let told = self.senses.weapon_told.get(&actor).is_some_and(|&t| (self.time - t).abs() < 1.0);
        if actor == FormId::PLAYER_REF || told {
            return self.extra.get(&actor).is_some_and(|e| e.weapon_out);
        }
        self.wants_weapon_out(actor)
    }

    /// Whether an actor wants its weapon out: alerted (SetAlert, or fighting), its
    /// package's Weapon Drawn flag, or a scripted weapon use (UseWeapon); never under its
    /// package's Weapons Unequipped flag while it does not fight, nor while it flees a
    /// fight [owner 2026-10-17: "they holster their weapon"; docs/design/combat-ai.md
    /// §12.2.1]. The rule the engine
    /// draws and holsters NPCs' weapons by (`nvforever` alert.rs), kept here so that
    /// IsWeaponOut answers for actors the engine does not draw [engineering choice; the
    /// flags' meanings: docs/spec-notes/packages.md].
    pub fn wants_weapon_out(&self, actor: FormId) -> bool {
        use crate::package::flags;
        let fighting = self.awareness.state(actor).in_combat();
        let unequipped = !fighting && self.package_has(actor, flags::WEAPONS_UNEQUIPPED);
        let fleeing = fighting && self.awareness.tactics.flights.contains_key(&actor);
        // Pretend In Combat: "the actor to enter combat when the package is begun" [documented:
        // Combat Package Flags]: its weapon is out as in a fight [our choice].
        let drawn = self.package_has(actor, flags::WEAPON_DRAWN) || self.package_has(actor, flags::PRETEND_IN_COMBAT);
        // SetWeaponOut 1 holds it drawn (`crate::nvse_hud`).
        let held = self.hud.forced_out.contains(&actor);
        // A child fighting or alerted keeps its weapon away: it never fights (#2438).
        let child = self.never_fights(actor) && (fighting || self.draws_weapon(actor));
        !unequipped && !fleeing && !child && (self.draws_weapon(actor) || drawn || held || self.active_weapon_use(actor).is_some())
    }

    /// A hit's limb damage on one limb ([`Game::damage_limb`]) with what crippling it
    /// tells: the hit record's crippled flag, JIP's crippled-limb handlers and the
    /// player's Limbs Crippled statistic.
    fn hit_limb(&mut self, target: FormId, l: LimbHit, max_health: f64) -> (f64, bool) {
        // The player's tweak scales limb damage in power armour [design: docs/design/tweaks.md].
        let scale = self.tweaks.power_armor_limb_damage;
        let l = if scale != 1.0 && self.wears_power_armor(target) { LimbHit { damage: l.damage * scale, ..l } } else { l };
        let (condition, crippled) = self.damage_limb(target, l, max_health);
        if crippled {
            if let Some(h) = self.jip.hits.get_mut(&target) {
                h.flags |= 1 << 5;
            }
            // [documented: SetOnCrippledLimbEventHandler] 0 head … 6 brain.
            let limb = f64::from(l.condition - crate::actor_values::LIMB_CONDITIONS.start());
            self.jip_event("crippledlimb", Some(target), vec![fnv_script::vm::XVal::Num(limb)]);
            if target == FormId::PLAYER_REF {
                self.count_stat(crate::misc_stats::HAVE_LIMBS_CRIPPLED, 1.0);
            }
        }
        (condition, crippled)
    }

    /// An explosion's limb damage on the actor it caught (docs/spec-notes/crippled-limbs.md
    /// §5): every limb in `limbs` ([`blast_limb_shares`]) at once, after its health damage
    /// ([`Game::hit`], whose hit record this marks crippled), scaled by the difficulty as
    /// a hit's, so the shares hold of the Health it lost. Nothing on a
    /// dead or immune actor. Returns each limb's condition code, its condition after and
    /// whether this blast crippled it.
    pub fn blast_limbs(&mut self, target: FormId, attacker: Option<FormId>, limbs: &[LimbHit]) -> Vec<(u16, f64, bool)> {
        if self.is_dead(target) || self.is_immune(target) {
            return Vec::new();
        }
        let mult = self.damage_mult(attacker, target);
        let max_health = actor::actor_value_base(self, target, HEALTH);
        limbs
            .iter()
            .map(|&l| {
                let (after, crippled) = self.hit_limb(target, LimbHit { damage: l.damage * mult, ..l }, max_health);
                (l.condition, after, crippled)
            })
            .collect()
    }

    /// Apply an explosion's live object effect (`EITM`, respecting `SetExplosionTraitForm`
    /// trait 5 and `TempCloneForm` snapshots via [`Game::explosion_traits`]) to `target`
    /// as a hit effect. Dead or immune targets and explosions without an enchantment
    /// apply nothing. Returns the active-effect ids started on `target`.
    pub fn apply_explosion_effect(&mut self, explosion: FormId, target: FormId, caster: Option<FormId>) -> Vec<u32> {
        if self.is_dead(target) || self.is_immune(target) {
            return Vec::new();
        }
        let Some(ench) = self.explosion_traits(explosion).and_then(|t| t.enchantment) else {
            return Vec::new();
        };
        let mods = crate::aid::Mods { hit: true, ..crate::aid::Mods::default() };
        self.apply_magic_item_with(target, ench, caster, false, &mods).into_iter().filter(|a| !a.extended).map(|a| a.id).collect()
    }

    /// Lower a limb's condition by a hit (see the module notes): its condition after, and
    /// whether this hit crippled it.
    pub fn damage_limb(&mut self, target: FormId, l: LimbHit, max_health: f64) -> (f64, bool) {
        let before = actor::actor_value(self, target, l.condition).unwrap_or(100.0);
        let loss = if l.points {
            l.damage.max(0.0)
        } else {
            let pool = max_health * l.health_percent / 100.0;
            let damage = self.limb_damage(target, l.damage);
            if pool <= 0.0 {
                0.0
            } else {
                damage / pool * 100.0
            }
        };
        if loss <= 0.0 || before <= 0.0 {
            return (before, false);
        }
        let loss = loss.min(before);
        actor::add_damage(self, target, l.condition, loss);
        let after = before - loss;
        (after, after <= 0.0)
    }

    /// The limb damage a hit does to `target`'s limb (docs/spec-notes/crippled-limbs.md
    /// §3): the player's is times `fCombatPlayerLimbDamageMult` [observed: 0.5 in
    /// FalloutNV.esm; hypothesis: that it scales the limb damage the player takes, from
    /// its name], then the target's "Adjust Limb Damage" perks (Small Frame ×1.25, Tough
    /// Guy ×0.8) [observed].
    pub fn limb_damage(&mut self, target: FormId, damage: f64) -> f64 {
        let player = target == FormId::PLAYER_REF;
        let d = if player { damage * setting(self, "fCombatPlayerLimbDamageMult", PLAYER_LIMB_DAMAGE_MULT) } else { damage };
        self.entry_point(target, crate::perks::ADJUST_LIMB_DAMAGE, d).0.max(0.0)
    }

    /// The share of its speed an actor keeps on its legs (docs/spec-notes/crippled-limbs.md
    /// §4): 1 with both whole, `fMoveOneCrippledLegSpeedMult` with one crippled,
    /// `fMoveTwoCrippledLegsSpeedMult` with both [observed: 0.85 and 0.75 in
    /// FalloutNV.esm; hypothesis: the settings' role, from their names]. For the player
    /// and every other actor alike; a creature's legs are the parts that name the two
    /// mobility conditions (the hind legs of four-legged ones).
    pub fn leg_speed_mult(&mut self, r: FormId) -> f64 {
        let crippled = [LEFT_MOBILITY, RIGHT_MOBILITY].into_iter().filter(|&c| self.is_crippled(r, c)).count();
        leg_speed(self, crippled)
    }

    /// The share of its speed an actor keeps in the body armour it wears ([`armor_speed`],
    /// [`worn_armor_class`]).
    pub fn armor_speed_mult(&mut self, r: FormId) -> f64 {
        armor_speed(self, worn_armor_class(&self.world, r))
    }

    /// The share of the standard speeds an actor moves at for its stats: its current
    /// Speed Mult (percent: SetAV's base, ModAV's and effects' modifiers, damage) over 100
    /// [documented: GECK wiki Stats Tab - NPC, "The Player Character would have a Speed of
    /// 100%"; observed for people: SetAV SpeedMult 150 ran 1.5 times as fast,
    /// actors.md §8.10], times its body armour's share ([`Game::armor_speed_mult`]).
    /// The player's walk and run take it every frame (issues #1364, #1742) [observed on
    /// the player: capture o25_speed, the armour shares and Speed Mult 150 = 1.5 times the
    /// run]. A new Speed Mult counts at once, by design: the original waited for an
    /// equipment change, which the owner ruled a bug (actors.md §8.10, The player).
    pub fn move_speed_mult(&mut self, r: FormId) -> f64 {
        let mult = actor::actor_value(self, r, crate::actor_values::SPEED_MULTIPLIER).unwrap_or(100.0);
        // The player's tweak: Agility above or below 5 speeds or slows everyone [design: docs/design/tweaks.md].
        let per = self.tweaks.agility_speed_per_point;
        let agility = if per != 0.0 { (1.0 + per * (actor::actor_value(self, r, crate::actor_values::AGILITY).unwrap_or(5.0) - 5.0)).max(0.1) } else { 1.0 };
        (mult / 100.0).clamp(0.0, 10.0) * self.armor_speed_mult(r) * agility
    }

    /// An actor's limb conditions, codes 25–31 in order.
    pub fn limb_conditions(&mut self, r: FormId) -> [f64; 7] {
        let mut out = [100.0; 7];
        for (i, code) in LIMB_CONDITIONS.enumerate() {
            out[i] = actor::actor_value(self, r, code).unwrap_or(100.0);
        }
        out
    }

    /// Whether the actor's limb (condition actor value code) is crippled.
    pub fn is_crippled(&mut self, r: FormId, condition: u16) -> bool {
        actor::actor_value(self, r, condition).is_ok_and(|v| v <= 0.0)
    }

    /// Whether the actor has Bloody Mess: its `Bloody Mess` actor value is above 0 (the
    /// perk's ability raises it [hypothesis], docs/spec-notes/dismemberment.md §3).
    pub fn has_bloody_mess(&mut self, r: FormId) -> bool {
        actor::av_by_name("Bloody Mess").is_some_and(|code| actor::actor_value(self, r, code).is_ok_and(|v| v > 0.0))
    }

    /// Whether the body part at `location` (GetHitLocation's numbering) of the actor is gone.
    pub fn limb_gone(&self, r: FormId, location: i32) -> bool {
        self.extra.get(&r).is_some_and(|e| e.limbs_gone.contains_key(&location))
    }

    /// The actor's gone body parts: location, and whether it exploded.
    pub fn limbs_gone(&self, r: FormId) -> Vec<(i32, bool)> {
        self.extra.get(&r).map_or_else(Vec::new, |e| e.limbs_gone.iter().map(|(&l, &x)| (l, x)).collect())
    }

    /// Dismember the actor's parts at these locations (true: exploded, false: severed),
    /// `by` the attacker. Parts already gone stay as they went. The player's hits count
    /// toward Dismembered Limbs, one per part (docs/spec-notes/dismemberment.md §5).
    /// Returns the parts newly gone.
    pub fn dismember(&mut self, r: FormId, parts: &[(i32, bool)], by: Option<FormId>) -> Vec<(i32, bool)> {
        let e = self.extra.entry(r).or_default();
        let new: Vec<(i32, bool)> = parts.iter().copied().filter(|(l, _)| *l >= 0 && !e.limbs_gone.contains_key(l)).collect();
        for &(l, exploded) in &new {
            e.limbs_gone.insert(l, exploded);
        }
        if by == Some(FormId::PLAYER_REF) && !new.is_empty() {
            self.count_stat(crate::misc_stats::DISMEMBERED_LIMBS, new.len() as f64);
        }
        // JohnnyGuitar's limb handlers: (actor, limb) (`crate::nvse_jg8`).
        for &(l, _) in &new {
            let (a, limb) = (fnv_script::vm::XVal::Ref(r.0), fnv_script::vm::XVal::Num(f64::from(l)));
            self.jg8_fire(crate::nvse_jg8::Ev::LimbGone, &[a.clone(), limb.clone()], vec![a, limb]);
        }
        new
    }

    /// Whether the reference's base has destruction data.
    pub fn is_destructible(&self, r: FormId) -> bool {
        base_of(self, r).is_some_and(|b| self.destruction(b).is_some())
    }

    /// The model damage stage the reference is drawn at: the DSTD damage stage of the
    /// destruction stage it has reached, the number its model's `BSDamageStage` nodes are
    /// shown for; 0 undamaged, back to 0 after ClearDestruction, and always 0 without
    /// destruction data [documented: xEdit wbDEST field; hypothesis: the meaning,
    /// docs/spec-notes/destruction-stages.md].
    /// The replacement model a destructible reference is drawn with now: that of the
    /// highest destruction stage reached that names one (DSTD's DMDL, "Replacement model"
    /// [documented: xEdit wbDEST]; the Motorcycle01's stage 4 `Vehicles\Motorcycle01Hulk.NIF`)
    /// [hypothesis: it stays through later stages that name none]; `None` undamaged, after
    /// ClearDestruction, or without one (docs/spec-notes/destruction-stages.md §4).
    pub fn destruction_model(&self, r: FormId) -> Option<&str> {
        let stage = crate::functions::objects_destruction_stage(self, r);
        if stage < 0 {
            return None;
        }
        let def = base_of(self, r).and_then(|b| self.destruction(b))?;
        def.stages.iter().filter(|s| i32::from(s.index) <= stage && s.model.is_some()).max_by_key(|s| s.index).and_then(|s| s.model.as_deref())
    }

    /// The references that have taken object damage (destruction stages).
    pub fn damaged_refs(&self) -> impl Iterator<Item = FormId> + '_ {
        self.extra.iter().filter(|(_, e)| e.damage.is_some()).map(|(r, _)| *r)
    }

    pub fn model_damage_stage(&self, r: FormId) -> u8 {
        let stage = crate::functions::objects_destruction_stage(self, r);
        let def = base_of(self, r).and_then(|b| self.destruction(b));
        def.and_then(|d| d.stages.iter().find(|s| s.index as i32 == stage)).map_or(0, |s| s.damage_stage)
    }

    /// Object damage on a destructible reference (as `DamageObject`, step C7)
    /// [documented: GECK wiki Destructible Object Stage]:
    /// - damage adds up to the object's health; each stage applies once its health
    ///   percentage is reached;
    /// - a stage with Cap Damage stops one hit from going past it;
    /// - Destroy marks the object destroyed (it takes no more damage but its own
    ///   self-damage), Disable disables it;
    /// - the stage's self-damage per second keeps damaging it ([`Game::destruction_tick`]);
    /// - each stage reached runs OnDestructionStageChange [hypothesis: once per change]
    ///   and its explosion is returned for the engine to set off.
    ///
    /// `None` when the reference has no destruction data or nothing changed hands.
    pub fn damage_object(&mut self, r: FormId, amount: f64) -> Option<DestructionChange> {
        let def = base_of(self, r).and_then(|b| self.destruction(b)).cloned()?;
        let e = self.extra.get(&r);
        if e.is_some_and(|e| e.destroyed) && e.and_then(|e| e.burning).is_none() {
            return None;
        }
        let before = crate::functions::objects_destruction_stage(self, r);
        let health = def.health.max(0) as f64;
        let had = e.and_then(|e| e.damage).unwrap_or(0.0);
        let mut total = had + amount.max(0.0);
        if health > 0.0 {
            total = total.min(health);
            // Cap Damage: a stage newly reached stops the hit at its threshold.
            let mut by_pct: Vec<&crate::data::DestructionStage> = def.stages.iter().collect();
            by_pct.sort_by_key(|s| std::cmp::Reverse(s.health_percent));
            for s in by_pct {
                if s.flags & crate::data::DestructionStage::CAP_DAMAGE != 0
                    && destruction_threshold_reached(total, health, s.health_percent)
                    && !destruction_threshold_reached(had, health, s.health_percent)
                {
                    total = total.min(destruction_damage_threshold(health, s.health_percent));
                    break;
                }
            }
        }
        self.extra.entry(r).or_default().damage = Some(total);
        let after = crate::functions::objects_destruction_stage(self, r);
        let mut out = DestructionChange { before, after, ..DestructionChange::default() };
        if after == before {
            return Some(out);
        }
        for s in def.stages.iter().filter(|s| s.index as i32 > before && s.index as i32 <= after) {
            if s.flags & crate::data::DestructionStage::DESTROY != 0 {
                self.extra.entry(r).or_default().destroyed = true;
                out.destroyed = true;
            }
            if s.flags & crate::data::DestructionStage::DISABLE != 0 {
                crate::functions::objects_disable(self, r);
                out.disabled = true;
            }
            out.explosions.extend(s.explosion);
            if let Some(d) = s.debris.filter(|_| s.debris_count > 0) {
                out.debris.push((d, s.debris_count));
            }
            if s.model.is_some() {
                out.model = s.model.clone();
            }
            if s.damage_stage > 0 {
                out.damage_stage = Some(s.damage_stage);
            }
            let dps = s.self_damage_per_second;
            self.extra.entry(r).or_default().burning = (dps > 0).then_some(dps as f64);
        }
        if health > 0.0 && total >= health {
            self.extra.entry(r).or_default().burning = None;
        }
        // A block with a parameter (`Begin OnDestructionStageChange 3`, the Citadel's
        // vertibird wrecks for Liberty Prime's march, SCPT 0x0A096CC8 in TTW, #3112)
        // runs for the stage reached, counted from 1: the wreck's last of three stages
        // (index 2) is its stage 3 [hypothesis: fit to the data, the only such block in a
        // TTW load order; in both Fallout3.esm's four-stage and TTW's three-stage DEST of
        // MQ11vertibirdPartyBus, 3 is index 2, whose DSTD damage stage is also 3]. One
        // without a parameter runs for any stage. NVSE handlers keep their arguments.
        self.ext_event(event::ON_DESTRUCTION_STAGE_CHANGE, r, None);
        self.ref_blocks(r, event::ON_DESTRUCTION_STAGE_CHANGE, Some(Value::Num(f64::from(after + 1))), None);
        Some(out)
    }

    /// The destructible references burning down toward a stage that explodes (a car on
    /// fire), each with that stage's explosion: the nearest later stage with one. The
    /// player's Exploding Destructible Indicator tweak shows them on the grenade
    /// indicator [design: docs/design/tweaks.md].
    pub fn burning_explosives(&self) -> Vec<(FormId, FormId)> {
        let mut out = Vec::new();
        for (r, _) in self.extra.iter().filter(|(_, e)| e.burning.is_some()) {
            let Some(def) = base_of(self, *r).and_then(|b| self.destruction(b)) else { continue };
            let stage = crate::functions::objects_destruction_stage(self, *r);
            let next = def.stages.iter().filter(|s| i32::from(s.index) > stage).filter_map(|s| s.explosion.map(|x| (s.index, x))).min_by_key(|(i, _)| *i);
            if let Some((_, x)) = next {
                out.push((*r, x));
            }
        }
        out
    }

    /// Advance burning objects (a stage's self-damage per second) by `dt` seconds; the
    /// changes that reached new stages, with their references.
    pub fn destruction_tick(&mut self, dt: f64) -> Vec<(FormId, DestructionChange)> {
        let burning: Vec<(FormId, f64)> = self.extra.iter().filter_map(|(r, e)| e.burning.map(|d| (*r, d))).collect();
        let mut out = Vec::new();
        for (r, dps) in burning {
            if let Some(c) = self.damage_object(r, dps * dt) {
                if c.after != c.before {
                    out.push((r, c));
                }
            }
        }
        out
    }

    /// Run the OnFire blocks of the weapon `actor` fired (its object script)
    /// [documented: block name; observed: the detonator's script, `DetonatorOnFire`, is
    /// an OnFire block], fire JIP's `SetOnFireWeaponEventHandler` event
    /// [documented: SetOnFireWeaponEventHandler], and fire ShowOff's
    /// `ShowOff:OnFireWeapon` event (`nvse_showoff.rs`). Returns whether an OnFire block ran.
    pub fn weapon_fired(&mut self, actor: FormId, weapon: FormId) -> bool {
        let ran = self.item_event(actor, weapon, event::ON_FIRE, None);
        self.fire_weapon_jip_event(actor, weapon, false);
        self.showoff_on_fire_weapon(actor, weapon, false);
        ran
    }

    /// The XP the player earns for killing `target` (see the module notes): none for the
    /// player's own death, by the player's or a companion's hand [design].
    pub fn kill_xp(&self, target: FormId) -> f64 {
        if target == FormId::PLAYER_REF {
            return 0.0;
        }
        let e = self.extra.get(&target);
        let (by_player, total) = e.map_or((0.0, 0.0), |e| (e.player_damage, e.total_damage));
        if total > 0.0 && by_player / total * 100.0 < setting(self, "iXPDeathRewardHealthThreshold", 40.0) {
            return 0.0;
        }
        let creature = base_of(self, target).and_then(|b| self.data.actors.get(&b)).is_some_and(|d| &d.sig == b"CREA");
        let level = self.actor_level(target);
        let (who, reward) = if creature { ("Creature", "Opponent") } else { ("NPC", "NPC") };
        let classes: [(&str, f64, f64); 5] = if creature {
            [("VeryEasy", 1.0, 1.0), ("Easy", 3.0, 5.0), ("Average", 6.0, 10.0), ("Hard", 9.0, 25.0), ("VeryHard", 12.0, 50.0)]
        } else {
            [("VeryEasy", 0.0, 0.0), ("Easy", 1.0, 10.0), ("Average", 7.0, 20.0), ("Hard", 10.0, 30.0), ("VeryHard", 13.0, 50.0)]
        };
        let class = classes.iter().find(|(c, lvl, _)| level <= setting(self, &format!("iXPLevelKill{who}{c}"), *lvl)).unwrap_or(&classes[4]);
        // × the difficulty's `fDiffMultXP<level>` (1 at every level in FalloutNV.esm;
        // `crate::difficulty`).
        let xp = setting(self, &format!("iXPRewardKill{reward}{}", class.0), class.2) * self.kill_xp_mult();
        // The player's tweak weighs the levels between them [design: docs/design/tweaks.md].
        let per = self.tweaks.kill_xp_per_level;
        if per == 0.0 {
            return xp;
        }
        xp * (1.0 + per * (level - self.player_level())).max(0.0)
    }
}
