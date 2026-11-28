//! The numbers the rules use (docs/design/combat.md §1 "Tunable in one place").
//!
//! - [`Rules`]: the game settings the documented formulas name, read from the masters.
//!   A setting the masters lack falls back to the default the GECK wiki page gives
//!   [documented: the page named on each field; absent from the masters = engine
//!   default is a hypothesis, docs/spec-notes/combat-data.md §3].
//! - [`CombatTuning`]: NVForever's own values for what no record or setting covers
//!   [design]. Changing one is a tuning change, not a code change.

use serde::Serialize;

use crate::data::CombatForms;

/// Game settings used by the damage, critical, spread, knockdown and knockback rules.
#[derive(Debug, Clone, Serialize)]
pub struct Rules {
    // Weapon Damage Formula
    pub damage_weapon_mult: f32,
    /// The weapon damage penalty below 75 % condition: 0.67, or JIP's
    /// SetConditionDamagePenalty (`fnv_game::Game::condition_damage_penalty`)
    /// [documented: GECK wiki SetConditionDamagePenalty].
    pub condition_damage_penalty: f32,
    /// NVForever's tweaks, set by the engine each frame and left out of the dumps
    /// (docs/design/tweaks.md): the condition at and above which a weapon deals its full
    /// damage (0.75; 1 with Remove Weapon Damage Buffer), whether an automatic weapon's
    /// critical chance is left undivided by its fire rate, and a factor on critical
    /// damage.
    #[serde(skip)]
    pub condition_damage_threshold: f32,
    #[serde(skip)]
    pub crit_ignores_fire_rate: bool,
    #[serde(skip)]
    pub crit_damage_scale: f32,
    pub damage_skill_base: f32,
    pub damage_skill_mult: f32,
    pub melee_damage_strength_mult: f32,
    pub melee_damage_strength_offset: f32,
    pub unarmed_damage_base: f32,
    pub unarmed_damage_mult: f32,
    pub damage_arm_condition_base: f32,
    pub damage_arm_condition_mult: f32,
    pub sneak_mult_gun: f32,
    pub sneak_mult_melee: f32,
    // Critical Hit Chance Formula
    pub crit_luck_base: f32,
    pub crit_luck_mult: f32,
    pub vats_crit_bonus: f32,
    // Armor
    pub max_armor_rating: f32,
    /// Melee reach base (× actor scale × weapon reach), from the attacker's body to its
    /// target's. The wiki gives no default and the masters lack the setting: 107 units is
    /// fitted to the original's NPCs, whose swings began about 45 + 107 × Reach units
    /// centre to centre (knife 95 … lead pipe 180) [observed: AI observer campaign
    /// o14_reach, 2026-11-01, #1076; that the 45 is the two bodies' radii, and so not
    /// part of the setting, is a hypothesis; docs/spec-notes/combat-rules.md §10].
    pub combat_distance: f32,
    /// V.A.T.S. melee reach: the swing's reach times `fVATSMeleeReachMult` (default 2:
    /// "doubling the reach a melee weapon can actually swing"), at most
    /// `fVATSMeleeMaxDistance` (default 300) [documented: GECK wiki "VATS Settings";
    /// neither setting is in FalloutNV.esm, so the defaults apply, observed; the page
    /// began as a Fallout 3 page, so that New Vegas keeps the defaults is a hypothesis]
    /// (`crate::damage::vats_melee_reach`, issue #2656).
    pub vats_melee_reach_mult: f32,
    pub vats_melee_max_distance: f32,
    // Gun Spread Formula (New Vegas)
    pub spread_unaimed: f32,
    pub spread_walking: f32,
    pub spread_running: f32,
    pub spread_standing: f32,
    pub spread_crippled_arm_1h: f32,
    pub spread_crippled_arm_2h: f32,
    pub spread_crippled_arms_1h: f32,
    pub spread_crippled_arms_2h: f32,
    pub wobble_to_skill: f32,
    pub min_gun_spread: f32,
    pub strength_req_penalty: f32,
    pub skill_req_penalty: f32,
    pub npc_max_gun_wobble_angle: f32,
    // Explosion Knockdown Formula
    pub knockdown_damage_mult: f32,
    pub knockdown_damage_base: f32,
    pub knockdown_agility_mult: f32,
    pub knockdown_agility_base: f32,
    pub knockdown_chance_max: f32,
    pub knockdown_current_health_threshold: f32,
    pub knockdown_base_health_threshold: f32,
    // Melee Knockback Formula
    pub knockback_damage_mult: f32,
    pub knockback_damage_base: f32,
    pub knockback_agility_mult: f32,
    pub knockback_agility_base: f32,
    pub knockback_force_max: f32,
    pub knockback_time: f32,
    // Gun Shell Settings
    pub shell_eject_speed: f32,
    pub shell_lifetime: f32,
    // Item condition (docs/spec-notes/combat-rules.md §17)
    /// Health points a weapon loses per attack.
    pub damage_to_weapon_value: f32,
    /// The share of a hit's damage the armour struck loses as health points.
    pub damage_to_armor_percentage: f32,
    // Power attacks (crate::melee): the damage multiplier of a power attack standing,
    // moving forward, back and to a side [hypothesis: the setting names].
    pub power_attack_stand: f32,
    pub power_attack_forward: f32,
    pub power_attack_back: f32,
    pub power_attack_side: f32,
    // Movement
    /// The share of their speed an actor loses running with a two-handed weapon drawn
    /// (`fMove2HBigPenalty`, 0.1 in FalloutNV.esm [observed]) [hypothesis: the setting's
    /// role, from its name; design: applied to every two-handed weapon, the owner's
    /// playtest list 5 item 61].
    pub move_2h_penalty: f32,
    /// The attack AP cost of each kind of weapon without its own (`fActionPointsAttack…`;
    /// VATS mode, docs/design/vats-mode.md §7.1).
    pub ap_attack: crate::vats::ApAttack,
    /// `fActionPointsRestoreRate`: spent AP comes back at this share of the maximum a
    /// second [hypothesis, vats-mode.md §7.2].
    pub ap_restore_rate: f32,
    /// The VATS settings of VATS mode's hit chance (docs/design/vats-mode.md §17.1).
    pub vats_hit: crate::vats::HitTuning,
    /// The VATS settings of the classic style (docs/design/vats-classic.md §3, §7).
    pub vats_classic: crate::vats::ClassicTuning,
    /// Settings not found in the load order (their defaults were used).
    pub defaulted: Vec<&'static str>,
}

/// (setting, documented default). Defaults from the GECK wiki pages: Weapon Damage
/// Formula, Critical Hit Chance Formula, Gun Spread Formula (New Vegas), Explosion
/// Knockdown Formula, Melee Knockback Formula, Gun Shell Settings, Armor (owner exports
/// 2026-10-09). `fAVDUnarmedDamage*` have no default on the page; the masters set them.
/// `fDamageToWeaponValue`, `fDamageToArmorPercentage` and the `fDamagePowerAttack…Bonus`
/// settings have no page: their defaults are FalloutNV.esm's values [observed].
const SETTINGS: &[(&str, f32)] = &[
    ("fDamageWeaponMult", 1.0),
    ("fDamageSkillBase", 0.5),
    ("fDamageSkillMult", 0.5),
    ("fAVDMeleeDamageStrengthMult", 0.5),
    ("fAVDMeleeDamageStrengthOffset", 0.0),
    ("fAVDUnarmedDamageBase", 0.5),
    ("fAVDUnarmedDamageMult", 0.05),
    ("fDamageArmConditionBase", 0.2),
    ("fDamageArmConditionMult", 0.8),
    ("fCombatDamageBonusSneakingMult", 2.0),
    ("fCombatDamageBonusMeleeSneakingMult", 5.0),
    ("fAVDCritLuckBase", 0.0),
    ("fAVDCritLuckMult", 1.0),
    ("fVATSCriticalChanceBonus", 15.0),
    ("fMaxArmorRating", 85.0),
    ("fCombatDistance", 107.0),
    ("fVATSMeleeReachMult", 2.0),
    ("fVATSMeleeMaxDistance", 300.0),
    ("fUnaimedSpreadPenalty", 0.2),
    ("fWalkingSpreadPenalty", 0.1),
    ("fRunningSpreadPenalty", 0.2),
    ("fStandingSpreadPenalty", 0.1),
    ("fCrippledArm1HSpreadPenalty", 0.2),
    ("fCrippledArm2HSpreadPenalty", 0.4),
    ("fCrippledArms1HSpreadPenalty", 0.4),
    ("fCrippledArms2HSpreadPenalty", 0.6),
    ("fWobbleToSkillConversion", 0.5),
    ("fMinGunSpreadValue", 0.01),
    ("fWeapStrengthReqPenalty", 0.025),
    ("fWeapSkillReqPenalty", 0.01),
    ("fNPCMaxGunWobbleAngle", 15.0),
    ("fKnockdownDamageMult", 0.3),
    ("fKnockdownDamageBase", 0.0),
    ("fKnockdownAgilMult", 1.0),
    ("fKnockdownAgilBase", 0.0),
    ("fKnockdownChance", 0.25),
    ("fKnockdownCurrentHealthThreshold", 50.0),
    ("fKnockdownBaseHealthThreshold", 75.0),
    ("fKnockbackDamageMult", 10.0),
    ("fKnockbackDamageBase", 50.0),
    ("fKnockbackAgilMult", -0.008),
    ("fKnockbackAgilBase", 1.0),
    ("fKnockbackForceMax", 512.0),
    ("fKnockbackTime", 1.0),
    ("fGunShellEjectSpeed", 160.0),
    ("fGunShellLifetime", 5.0),
    ("fDamageToWeaponValue", 0.2),
    ("fDamageToArmorPercentage", 0.35),
    ("fDamagePowerAttackStandBonus", 3.0),
    ("fDamagePowerAttackForwardBonus", 2.0),
    ("fDamagePowerAttackBackBonus", 2.0),
    ("fDamagePowerAttackSideBonus", 2.0),
    ("fMove2HBigPenalty", 0.1),
    // VATS mode's AP (docs/design/vats-mode.md §7): no wiki defaults; FalloutNV.esm's
    // values [observed].
    ("fActionPointsAttackPistol", 20.0),
    ("fActionPointsAttackRifle", 25.0),
    ("fActionPointsAttackHandle", 20.0),
    ("fActionPointsAttackLauncher", 40.0),
    ("fActionPointsAttackOneHandMelee", 15.0),
    ("fActionPointsAttackTwoHandMelee", 30.0),
    ("fActionPointsAttackUnarmed", 22.0),
    ("fActionPointsAttackGrenade", 35.0),
    ("fActionPointsAttackMine", 10.0),
    ("fActionPointsRestoreRate", 0.06),
    // VATS mode's hit chance (docs/design/vats-mode.md §17.1): the GECK wiki's defaults
    // (VATS Settings); FalloutNV.esm sets fVATSDistanceFactor 2 and fVATSSpreadMult 1.1
    // [observed] and none of the others.
    ("fVATSMaxChance", 95.0),
    ("fVATSHitChanceMult", 1.0),
    ("fVATSSkillFactor", 1.0),
    ("fVATSDistanceFactor", 2.0),
    ("fVATSSpreadMult", 0.5),
    ("iVATSConcentratedFireBonus", 5.0),
    // The classic VATS style (docs/design/vats-classic.md §3, §7) [documented:
    // fallout.wiki, "Fallout 3 V.A.T.S." settings table and "Fallout: New Vegas V.A.T.S."].
    ("fVATSMaxEngageDistance", 5000.0),
    ("fVATSPlayerDamageMult", 0.75),
];

impl Rules {
    /// The documented defaults only (no load order).
    pub fn defaults() -> Rules {
        Rules::with(|_| None)
    }

    /// The load order's settings, defaults where it has none.
    pub fn from_forms(c: &CombatForms) -> Rules {
        Rules::with(|name| c.setting(name))
    }

    fn with(get: impl Fn(&str) -> Option<f32>) -> Rules {
        let mut defaulted = Vec::new();
        let v: Vec<f32> = SETTINGS
            .iter()
            .map(|&(name, d)| {
                get(name).unwrap_or_else(|| {
                    defaulted.push(name);
                    d
                })
            })
            .collect();
        let mut it = v.into_iter();
        let mut n = || it.next().expect("one value per setting");
        Rules {
            damage_weapon_mult: n(),
            damage_skill_base: n(),
            damage_skill_mult: n(),
            melee_damage_strength_mult: n(),
            melee_damage_strength_offset: n(),
            unarmed_damage_base: n(),
            unarmed_damage_mult: n(),
            damage_arm_condition_base: n(),
            damage_arm_condition_mult: n(),
            sneak_mult_gun: n(),
            sneak_mult_melee: n(),
            crit_luck_base: n(),
            crit_luck_mult: n(),
            vats_crit_bonus: n(),
            max_armor_rating: n(),
            combat_distance: n(),
            vats_melee_reach_mult: n(),
            vats_melee_max_distance: n(),
            spread_unaimed: n(),
            spread_walking: n(),
            spread_running: n(),
            spread_standing: n(),
            spread_crippled_arm_1h: n(),
            spread_crippled_arm_2h: n(),
            spread_crippled_arms_1h: n(),
            spread_crippled_arms_2h: n(),
            wobble_to_skill: n(),
            min_gun_spread: n(),
            strength_req_penalty: n(),
            skill_req_penalty: n(),
            npc_max_gun_wobble_angle: n(),
            knockdown_damage_mult: n(),
            knockdown_damage_base: n(),
            knockdown_agility_mult: n(),
            knockdown_agility_base: n(),
            knockdown_chance_max: n(),
            knockdown_current_health_threshold: n(),
            knockdown_base_health_threshold: n(),
            knockback_damage_mult: n(),
            knockback_damage_base: n(),
            knockback_agility_mult: n(),
            knockback_agility_base: n(),
            knockback_force_max: n(),
            knockback_time: n(),
            shell_eject_speed: n(),
            shell_lifetime: n(),
            damage_to_weapon_value: n(),
            damage_to_armor_percentage: n(),
            power_attack_stand: n(),
            power_attack_forward: n(),
            power_attack_back: n(),
            power_attack_side: n(),
            move_2h_penalty: n(),
            ap_attack: crate::vats::ApAttack {
                pistol: n(),
                rifle: n(),
                handle: n(),
                launcher: n(),
                one_hand_melee: n(),
                two_hand_melee: n(),
                unarmed: n(),
                grenade: n(),
                mine: n(),
            },
            ap_restore_rate: n(),
            vats_hit: crate::vats::HitTuning {
                max_chance: n(),
                hit_chance_mult: n(),
                skill_factor: n(),
                distance_factor: n(),
                spread_mult: n(),
                concentrated_fire_bonus: n(),
            },
            vats_classic: crate::vats::ClassicTuning { max_engage: n(), player_damage_mult: n() },
            condition_damage_penalty: crate::damage::CONDITION_DAMAGE_PENALTY,
            condition_damage_threshold: crate::damage::CONDITION_DAMAGE_THRESHOLD,
            crit_ignores_fire_rate: false,
            crit_damage_scale: 1.0,
            defaulted,
        }
    }
}

/// NVForever's own combat numbers [design]. Each says what it shapes.
#[derive(Debug, Clone, Serialize)]
pub struct CombatTuning {
    /// Armour never takes more than this share of a hit away (DT floor, §5.3).
    pub dt_floor: f32,
    /// A headshot on a target that is unaware of the attacker is always a critical.
    pub headshot_unaware_crit: bool,
    /// Spread added per shot (spread units): this, plus [`Self::bloom_per_min_spread`]
    /// times the weapon's Min Spread, times [`Self::bloom_auto_mult`] for automatics.
    pub bloom_per_shot: f32,
    pub bloom_per_min_spread: f32,
    pub bloom_auto_mult: f32,
    /// Bloom (spread units) recovered per second once the weapon settles.
    pub bloom_recovery: f32,
    /// Bloom cap (spread units).
    pub bloom_max: f32,
    /// Seconds of rest, beyond the weapon's own time between shots, before bloom starts
    /// to shrink and a new burst's recoil pattern starts; closer shots build on each other.
    pub settle_delay: f32,
    /// Actor-spread penalty while airborne (spread units, like the documented penalties).
    pub jump_spread: f32,
    /// View kick up per shot in degrees: base plus per point of damage per shot, times
    /// [`Self::recoil_auto_mult`] for automatics.
    pub recoil_base_deg: f32,
    pub recoil_per_damage_deg: f32,
    pub recoil_auto_mult: f32,
    /// Each kick up is this share bigger or smaller at random.
    pub recoil_jitter: f32,
    /// The sideways kick as a share of the kick up, at most.
    pub recoil_side_share: f32,
    /// The chance per shot that the climb's lean swaps sides.
    pub recoil_lean_flip: f32,
    /// Melee damage window after the `Hit` key, seconds.
    pub melee_window: f32,
    /// Rays across a melee swing, and the swing's half-angle in degrees.
    pub melee_rays: u32,
    pub melee_arc_deg: f32,
    /// Explosion damage at the edge of the radius as a share of the centre's.
    pub explosion_edge: f32,
    /// NVForever's Explosion Radius Buff tweak, set by the engine and left out of the
    /// dumps (docs/design/tweaks.md): the inner share of the radius that takes full
    /// damage (0: none) and the edge's share with it on.
    #[serde(skip)]
    pub explosion_inner: f32,
    #[serde(skip)]
    pub explosion_inner_edge: f32,
    /// The share of its speed a lobber keeps at a bounce, before its PROJ's Bouncy Mult.
    pub default_bounce: f32,
    /// A thrown grenade's contacts (combat-rules.md §5): the share of the speed into a
    /// surface it keeps, out of it (the game setting `fGrenadeRestitution`, 0.01 in
    /// FalloutNV.esm [observed]; its meaning [hypothesis: from its name]); the share of the
    /// speed along the surface it keeps when it first lands (0.6); and the speed its roll
    /// loses every second, game units a second squared, until it rests (140) [observed:
    /// fitted to the original's frag grenade, nine throws of o32_throw 2026-11-25/26, the
    /// share kept 0.33-1.0 by throw, 0.61 the median, rolling friction 117-418 by throw;
    /// with 0.6 and 140 the distances at 1.0, 1.5, 2.0 s and the fuse come within about
    /// 50 units for holds of 5-120 loops; issue #1785, combat-rules.md §5].
    pub grenade_restitution: f32,
    pub grenade_roll_keep: f32,
    pub grenade_roll_friction: f32,
    /// Reload when the trigger is pulled on an empty magazine and ammo is carried.
    pub auto_reload: bool,
    /// Reload time when no reload clip length is known, seconds.
    pub fallback_reload: f32,
    /// Melee (step C6): holding Use this long starts a power attack (`crate::melee`); a
    /// power attack with no clip to time it takes this much longer than a swing.
    pub power_hold: f32,
    pub power_time: f32,
    /// Share of a melee hit a block stops.
    pub block: f32,
    /// Seconds the player cannot block after a power attack broke its block
    /// ([`crate::damage::against_block`]).
    pub guard_break: f32,
    /// Seconds after an actor's hit reaction ends before a hit on a crippled body part
    /// starts another, so that fast hits cannot hold it in the reaction
    /// ([`crate::damage::staggers`]).
    pub stagger_cooldown: f32,
    /// Crippled limbs (docs/spec-notes/crippled-limbs.md): actor-spread penalty with the
    /// head crippled (spread units, like the documented crippled-arm ones), the game's
    /// "lower accuracy" for the head having no number.
    pub spread_crippled_head: f32,
    /// The player's flinch from a staggering hit (a torso crippled, or the hit that
    /// cripples): the view is thrown off by up to this many degrees, and no attack
    /// starts for this many seconds (one under way is broken off).
    pub flinch_kick_deg: f32,
    pub flinch_time: f32,
    /// The view's sway while aiming with one crippled arm and with both, degrees at its
    /// widest, and its period in seconds (the player's first-person weapon sways; a
    /// scope's zoom makes the same angle look larger).
    pub arm_sway_deg: f32,
    pub arms_sway_deg: f32,
    pub arm_sway_period: f32,
    /// Weapon jamming (the owner's playtest list 5 item 9; [`crate::condition::jam_chance`]):
    /// below this condition (a share of full) a shot may jam the weapon.
    pub jam_condition: f32,
    /// The chance a shot jams at zero condition; it grows from 0 at
    /// [`Self::jam_condition`] to this, along [`Self::jam_curve`]. Kept low: the owner
    /// tunes it after play sessions.
    pub jam_chance_max: f32,
    /// The power the shortfall below [`Self::jam_condition`] is raised to (1: linear;
    /// above 1: rare until the weapon is well worn).
    pub jam_curve: f32,
    /// How long a jam keeps the weapon from firing when no jam clip is known, seconds.
    pub jam_fallback: f32,
}

impl Default for CombatTuning {
    fn default() -> Self {
        CombatTuning {
            dt_floor: 0.2,
            headshot_unaware_crit: true,
            bloom_per_shot: 0.08,
            bloom_per_min_spread: 0.04,
            bloom_auto_mult: 0.5,
            bloom_recovery: 2.0,
            bloom_max: 1.0,
            settle_delay: 0.3,
            jump_spread: 0.3,
            recoil_base_deg: 0.6,
            recoil_per_damage_deg: 0.04,
            recoil_auto_mult: 0.5,
            recoil_jitter: 0.25,
            recoil_side_share: 0.4,
            recoil_lean_flip: 0.3,
            melee_window: 0.1,
            melee_rays: 5,
            melee_arc_deg: 35.0,
            explosion_edge: 0.25,
            explosion_inner: 0.0,
            explosion_inner_edge: 0.2,
            default_bounce: 0.3,
            grenade_restitution: 0.01,
            grenade_roll_keep: 0.6,
            grenade_roll_friction: 140.0,
            auto_reload: true,
            fallback_reload: 2.0,
            power_hold: 0.4,
            power_time: 1.3,
            block: 0.6,
            guard_break: 1.0,
            stagger_cooldown: 3.0,
            spread_crippled_head: 0.2,
            flinch_kick_deg: 4.0,
            flinch_time: 0.6,
            arm_sway_deg: 1.5,
            arms_sway_deg: 3.0,
            arm_sway_period: 2.5,
            jam_condition: 0.75,
            jam_chance_max: 0.04,
            jam_curve: 1.5,
            jam_fallback: 1.5,
        }
    }
}
