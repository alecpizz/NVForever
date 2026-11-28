//! Combat AI, step CA1 (docs/design/combat-ai.md): actors in Combat
//! (`fnv_game::awareness`) attack their target with the same rules as the player
//! (`nvf_play::combat`).
//!
//! Each drawn, living actor in Combat with a target:
//! - **What it fights with**: the weapon drawn on it (`HeldWeapon`); a gun (a weapon
//!   that fires a projectile) is a ranged attack, any other weapon a melee one; without
//!   a weapon a creature attacks with its own damage and reach (CREA DATA Damage, RNAM
//!   Attack Reach), an NPC with its fists [design].
//! - **How near it has to be** ([`engage`]), told to the AI that moves actors
//!   (`Awarenesses::engage`, `fnv_ai::alert` closes in): a gun's useful range, a melee
//!   weapon's or creature's reach [design].
//! - **A gun's reach** ([`gun_reach`], docs/design/combat-ai.md §16): its max range
//!   (WEAP DNAM) times its style's Ranged Weapon Max Range Mult, never beyond its
//!   projectile's range (PROJ: a flamer's flame goes 640 units). It does not fire at a
//!   target beyond that ("Actors outside this range will not fire their weapon until
//!   they are within distance of this value to the target" [documented: GECK wiki
//!   Weapons]), nor with an explosive projectile at one within the explosion's radius
//!   [design]; it leaves cover to close in on a target out of reach [design], and stops
//!   short of it at a distance by its weapon's reach (a shotgun or flamer close, a
//!   sniper rifle far, a launcher clear of its own blast).
//! - **When it fires**: only while it detects and sees its target (the latest
//!   detection of the pair: above 0, in sight), after a reaction time from the moment it
//!   first saw it in this fight, with its weapon drawn, and a gun only facing it (within
//!   [`FIRE_FACING`] of straight ahead; owner's playtest list 5 item 33). A gun keeps its own cycle
//!   (`fnv_combat::attack::Cycle`: fire rate, magazine, reload) as the player's does;
//!   a semi-automatic gun's shots are spaced by its attack clip in its own rig once known
//!   (`attack_anims`, the player's rule: `fnv_combat::attack::npc_clip_timing`, #2739),
//!   by 1 ÷ Attack Shots/Sec until then;
//!   an automatic weapon's trigger is held for a burst and let go for a pause
//!   (`fnv_combat::burst`: the V.A.T.S. burst's rounds, a 0.5–1.5 s pause scaled by the
//!   style's semi-automatic delay multipliers; embedded weapons too, issue #1448), and
//!   pulled once a shot for the others [design]. As the trigger is pulled, a gun fires only with a clear line from its
//!   eyes and muzzle to the target's chest, else its head (`combat_tactics`: a wall in
//!   the way), whatever detection says; held, it steps aside or closes in [design].
//!   Test aid `NVF_PLAY_TEST_SEE_THROUGH_WALLS=1`: fighters count their targets as seen
//!   whatever detection says, standing in for detection that says "seen" while a wall
//!   stops the shot (smoke `combat-wall-hold`).
//! - **Reloading**: its gun's magazine (WEAP Clip Size) empties as it fires, and it
//!   reloads as soon as it is empty, whether it can fire then or not, playing its
//!   weapon's reload clips (`attack_anims`, [`Move::Reload`]) for as long as they run in
//!   its rig ([`AttackTimes::reloads`]; the tuning's fallback time until they are
//!   known). A weapon that loads round by round (the .357 revolver, lever-actions;
//!   docs/spec-notes/combat-rules.md §4) loads every round: it pulls no trigger while it
//!   reloads, which would close the weapon after one. The same gun keeps its magazine
//!   on a new target. Rounds come from nowhere: an NPC uses no ammunition from its
//!   inventory [design: owner 2026-10-17, "they dont use ammo"]. Reloading before the
//!   magazine is empty (in cover, out of sight) is not built. A creature's embedded gun
//!   has a magazine, and reloads so, once its rig is known to have the weapon's reload
//!   clip; without one (the sentry bot's folder has none) it fires on without a magazine
//!   (`fnv_combat::body_weapon::has_magazine` [design, #2739]; docs/spec-notes/
//!   combat-rules.md §4.1).
//! - **Where it aims**: from its eyes at the target's chest, the shot drawn from its
//!   weapon's muzzle (the model's `ProjectileNode`, `weapons::Muzzle`) closing onto that
//!   line, as the player's first-person shots (`ActorFire`; owner 2026-10-11: "the shots
//!   were coming from Violet's face, not the gun"). A creature's embedded weapon (a
//!   sentry bot's minigun, an eyebot's laser) shoots from the node of the creature's
//!   skeleton its WEAP `NNAM` names ("Node that the weapon is attached to" [documented:
//!   GECK wiki Weapons, Embedded Weapon]; `ProjectileNode_RGatling`,
//!   `ProjectileNode_HeadLaser`); a node only in a body part's model is not found
//!   [design: the skeleton's nodes are the ones spawned]. Without either, from the eyes. A
//!   creature with no weapon drawn in its hands fights with such an embedded gun when its
//!   skeleton has the gun's node (before 2026-10-24 embedded guns were never fired: the
//!   sentry bot only struck) [design: which of several; the first equipped or carried].
//!   Inside its spread: the documented actor
//!   spread for its weapon skill and its gait ([`shot_gait`]: walking or running as it
//!   fires widens it, #2125), plus an aim that settles over
//!   [`SETTLE_SECONDS`] of firing at the same target [design].
//! - **Aiming down the sights** (owner's playtest list 2026-10-11: "only if the target
//!   is at a distance; anything less than 20 metres they fire from the hip"): a gun
//!   with sights (WEAP Sight FOV above 0; 0 "essentially disables iron sights"
//!   [documented: GECK wiki Weapons]) is aimed down its sights while the actor fires at
//!   a target more than [`HIP_FIRE_WITHIN`] away (20 m at 70 units per metre,
//!   docs/spec-notes/physics.md §6.3), from the hip nearer ([`aims_down_sights`]).
//!   Aimed, the spread loses the unaimed penalty (fUnaimedSpreadPenalty) [documented:
//!   GECK wiki Gun Spread Formula (New Vegas)], the actor plays its weapon's iron-sights
//!   aim and attack clips (`2hraimis.kf`, `2hrattack3is.kf`, `attack_anims.rs`), and it
//!   walks instead of running (`Awarenesses::aiming`, `fnv_ai::alert`). The record's
//!   Sight Usage ("the percentage of fCombatIronSightsDistance" beyond which an actor
//!   uses its sights [documented: GECK wiki Weapons]) is not used: the owner's 20 m
//!   rule stands in for it, and fCombatIronSightsDistance is neither in FalloutNV.esm
//!   nor on the wiki.
//! - **How it looks** (`attack_anims.rs`): each shot and blow asks for its attack clip
//!   ([`PlayAttack`]); a blow's damage is dealt at its clip's `Hit` key
//!   ([`AttackTimes`]), and lands if the target is still within reach then [design].
//!   Each fighter's pitch to its target ([`Fighters::pitch`]) bends its aim.
//! - **Melee**: it closes in until its body is [`MELEE_STOP_GAP`] from its target's
//!   (the bodies as `nvf_play::blocking` has them: a long creature reaches out in front
//!   of its feet) and its reach keeps it off ([`melee_spacing`]), and strikes while the
//!   gap is within [`MELEE_HIT_GAP`] or the target within its reach and it sees the
//!   target, a blow every [`STRIKE_SECONDS`] (divided by the weapon's attack speed)
//!   [design; owner 2026-10-11: a dog gets into range and bites, it does not push the
//!   player around]. Without bodies to measure, its reach from its feet counts. A
//!   creature keeps its own reach and body from its target's centre instead
//!   ([`creature_spacing`], [`creature_reach`]).
//! - **Creatures' power attacks** (a radscorpion's sting): a creature's blow is a power
//!   attack by its combat style's Power Attack % Chance [documented: GECK wiki Combat
//!   Style], or [`FALLBACK_POWER_CHANCE`] where the style gives none [design: owner
//!   2026-10-16, random power attacks where the rule is not found]; which one by the
//!   style's normal and forward shares, evenly where both are 0 [design], among those it
//!   has clips for ([`AttackTimes::powers`]). It lands at its own clip's `Hit` key, deals
//!   the creature's usual damage [design: no power damage rule is sourced] and carries
//!   its attack group to the hit, for an unarmed attack effect that names it
//!   (`nvf_play::combat::hit_effects::applies`). Rolled on their own sequence.
//! - **People's power attacks** (docs/spec-notes/combat-rules.md §10.1): a person's
//!   blow with a melee weapon or its fists is a power attack by its style's Power Attack
//!   % Chance plus the recoil/stagger and unconscious bonuses [documented: GECK wiki
//!   Combat Style]; which one by the style's five shares ("Choose Attack using % Chance")
//!   or by how it moves against its facing ([`local_motion`], [`POWER_MOVE_SPEED`]),
//!   standing with both legs crippled, among the clips its rig has for its weapon
//!   (`AttackTimes::people_powers`): a direction without one is an ordinary blow
//!   ([`pick_people_power`]). It lands at its clip's `Hit` key, its next decision waits
//!   for the clip's end, the blow carries its attack group (the direction's damage bonus
//!   in `nvf_play::combat`, breaking blocks) and the actor tries its PowerAttack line.
//!   Rolled on their own sequence.
//! - **Melee combos** (docs/design/combat-ai.md §15): a blow's clip with an `a:` text
//!   key (`a:L` on the `AttackRight` clips, `a:R` on the `AttackLeft` ones; `fnv_combat::
//!   melee`) times the next melee decision; a blow decided between the key and the
//!   clip's end goes on the combo with the group the key names (right, left, right…,
//!   fists alternating hands), as the player's swings do, cutting the clip short. A blow
//!   decided later, after a hold, a block or a recoil starts again from the weapon's own
//!   attack group. **A creature's** next decision waits for its clip: at the `a:` key,
//!   and without one for both the clip's end and the base [`STRIKE_SECONDS`] pace
//!   [observed: capture run15b_b2, 2026-11-15, radscorpions; #2535]. **A person's** keeps
//!   the base pace (over its attack speed), brought forward to an earlier `a:` key at
//!   the clip's own time, and a power attack's waits for its clip's length [design; owner
//!   request 2026-10-16]: the maintainer's decision of 2026-11-18 on #2971 keeps people
//!   on this until their pace is measured in the original (observer check 244). A
//!   creature's key, combo window and clip end are divided by its blow's attack speed (1
//!   today), in the manner of the guns' `a:` key (#2739) [design, #2535 review].
//!
//! - **Its combat style** (CSTY, `fnv_game::combat_style`; docs/design/combat-ai.md
//!   §10.4) [documented: GECK wiki Combat Style]: Weapon Restrictions "Melee Only" has
//!   it fight a gun-holder with its fists; the Ranged Weapon Max Range Mult scales the
//!   weapon's max range in the distance it fights from; a semi-automatic gun waits a
//!   random time between the weapon's Semi-Auto Fire Delay Min and Max, times the
//!   style's multipliers, between shots ("semi-auto weapon's fire rate when used by
//!   actors" [documented: GECK wiki Weapons]); in melee each time it may strike it makes
//!   the melee decision (attack chance with the hand-to-hand bonus and the attack skill
//!   modifier, under attack or not), and holds for the Hold Timer when it does not attack.
//! - **Confidence and fleeing** (`fnv_game::combat_group`, docs/design/combat-ai.md
//!   §12.2): each fighter's strength (health, damage per second) goes to the game every
//!   frame; once hurt, every random 0 to fCombatThreatRatioUpdateTime seconds it makes a
//!   flee check (its group's threat ratio, and its personal one when its style says so,
//!   against its confidence and this fight's modifier) and flees when it fails
//!   (`fnv_ai::alert` runs it to a door, cover or away). It does not attack while it
//!   flees. At its flee spot, hurt or with a target it sees within
//!   fCombatFleeBoostConfidenceTargetRadius, its confidence goes up a level and it checks
//!   again: it fights again or flees on; after fCombatFleeWaitTime there it checks again
//!   without a boost [hypothesis: what the wait time is for].
//!   A child (its race's Child flag, `Game::never_fights`) flees at once, unhurt, and
//!   never turns to fight again [owner decision 2026-11-13, #2438].
//! - **Hiding at the flee spot** (docs/design/combat-ai.md §12.2.1): a person that has
//!   reached its flee spot is crouched there by the rules (`Game::sneaks`,
//!   `Game::at_flee_spot`: the crouched set, and the original's cower idle over it,
//!   `idles.rs`) while it waits.
//!   Seen by its enemy there for [`HIDE_SEEN_SECONDS`], it runs on to another spot, at
//!   most [`HIDE_MOVES`] times a flight [design: owner 2026-10-17, "fleeing enemies run
//!   and hide, crouching down when they stop, to hide from their enemies"].
//! - **Tactics** (`combat_tactics`): cover by the style's Take Cover Chance and timers,
//!   flank places for a group's gunmen, no shot while a friend stands in the line of
//!   fire, and a step back from a target nearer than [`tactics::BACK_OFF_WITHIN`]
//!   [design]; a short-reach gun's step back ends short of where it stops closing in
//!   ([`tactics::back_off_band`], #2351).
//! - **Moving while it fights**: it stays facing its target and strafes or backs away
//!   (nvforever `residents`, `Core::aims`), so it fires on the move.
//!
//! - **Attack lines**: each shot or blow it begins tries its Attack line, said at most
//!   once per its attack timer (`fnv_game::combat_lines`, docs/spec-notes/combat-dialogue.md).
//! - **Crouching, blocking, closing in** (people only; docs/design/combat-ai.md §14, the
//!   rules in `combat_stance`): a gunman that sees a far target now and then crouches to
//!   fire (`Awarenesses::crouching`), hides crouched behind low cover, shifts sideways
//!   after a while and stands up for a close-combat enemy coming in or to move; a melee
//!   fighter against a close-combat opponent makes the melee decision block first, also
//!   in reaction to the opponent's blow, and the attacker of a blocked blow recoils;
//!   against a gunman that sees it from afar it bounds from cover to cover, or charges
//!   when there is none, or stays by a gunman of its group until the target comes near.
//!
//! - **The combat style's other fields** (docs/design/combat-ai.md §17): a melee
//!   fighter's hold also makes the maneuver decision, a dodge left, right, back or forward
//!   or an idle (`combat_style::maneuver`); a rushing attack rolled as it closes in makes
//!   its first decision an attack; no pursuit beyond its Combat Radius from where its
//!   fight began or it was last hit; no new target outside its Max Targeting FOV; an
//!   explosive shot held while it would catch itself, a comrade or a bystander (the
//!   Ignore Damaging flags); a run out of a live grenade's blast unless Ignore Threats.
//!
//! Whom it fights is the detection states' (its combat target). Not built: throwing
//! grenades, ammunition use (module notes: reloading), weapon choice among several.

use crate::attack_anims::{AttackTimes, Move, PlayAttack, Power, PowerClip, SwingPick, DEFAULT_HIT};
use crate::combat_stance::{self as stance, Temperament};
use crate::combat_tactics::{self as tactics, Battle, Phase, Tactic};
use crate::nav::NavState;
use fnv_game::combat_group::{Post, PostKind, Strength};
use fnv_nav::Space;
use nvf_play::Location;
use crate::idles::ActorIdle;
use crate::limbs::ActorRig;
use crate::locomotion::{Dying, Flinch, Locomotion};
use crate::ragdoll::Corpse;
use crate::body_weapons::{BodyMelee, BodyWeapon};
use crate::weapons::{HeldWeapon, Muzzle, WeaponHold};
use crate::world::bevy_to_game;
use bevy::prelude::*;
use fnv_actor::weapon::Stage;
use fnv_combat::attack::{ClipTiming, Cycle, Event, Input, Phase as GunPhase, Timing};
use fnv_combat::damage::AttackKind;
use fnv_combat::data::WeaponStats;
use fnv_combat::spread::{self, Stance};
use fnv_combat::vec::{self, V3};
use fnv_game::awareness::State;
use fnv_game::combat_style::{melee_attack_chance, melee_block_chance, Opponent};
use fnv_game::detection::Band;
use fnv_sim::FormId;
use nvf_play::blocking::{ActorBlocks, Block};
use nvf_play::combat::{ActorFire, ActorStrike, BlockedBlow, Combat, CombatStats};
use nvf_play::control::PlayerControl;
use nvf_play::phys::PhysState;
use nvf_play::player::{Player, EYE_HEIGHT};
use nvf_play::script::{with_world, ScriptState};
use nvf_play::sim::SimState;
use std::collections::{BTreeMap, HashMap};

/// Test aid `NVF_PLAY_TEST_SEE_THROUGH_WALLS=1` (module notes).
fn sees_through_walls() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("NVF_PLAY_TEST_SEE_THROUGH_WALLS").is_ok_and(|v| v == "1"))
}

/// Seconds between first seeing the target in a fight and the first attack [design].
pub const REACTION_SECONDS: f32 = 0.6;
/// Extra spread when an actor starts firing at a target, degrees, gone after
/// [`SETTLE_SECONDS`] of firing at it [design: the first shots of a burst miss more].
pub const SETTLE_DEGREES: f32 = 4.0;
pub const SETTLE_SECONDS: f32 = 3.0;
/// Seconds between two blows at attack speed 1 [design].
pub const STRIKE_SECONDS: f32 = 1.2;
/// A gun's engage distance: its max range (WEAP DNAM) times this, within these bounds;
/// without a max range the upper bound [design].
pub const GUN_RANGE_SHARE: f32 = 0.8;
pub const GUN_ENGAGE_MIN: f32 = 400.0;
pub const GUN_ENGAGE_MAX: f32 = 2500.0;
/// A gun never stops farther than this share of its reach ([`GunReach`]), so the
/// lower bound above and the blast's clearance do not put it out of reach; and it sets
/// off again once its target is beyond this share, while it can still fire, so it walks
/// in aiming instead of standing until the target is out of reach [design].
pub const GUN_REACH_EDGE: f32 = 0.9;
/// A gun with an explosive projectile stands at least this much beyond the explosion's
/// radius from its target, and steps back from a target nearer than that, game units
/// [design; docs/design/combat-ai.md §16].
pub const BLAST_MARGIN: f32 = 64.0;
/// A flame weapon stops at this share of its reach, and nearer than [`GUN_ENGAGE_MIN`]
/// when that is less. In the original a trooper with a flamer (reach 640) closed to
/// 164–320 units of the player [observed 2026-10-18, AI observer campaign gun_reach, one
/// run; the share is fitted to its far end, docs/design/combat-ai.md §16].
pub const FLAME_RANGE_SHARE: f32 = 0.5;
/// An area weapon: one whose explosion's radius is at least this share of its reach (the
/// missile launcher's 1,000 of 1,536; not the Gauss rifle's 40 of 2,000). It stops just
/// clear of its blast instead of at [`GUN_RANGE_SHARE`] of its reach: in the original a
/// missile trooper fought from 450–550 units, inside its blast [observed 2026-10-18, AI
/// observer campaign gun_reach, one run]; the owner wants them clear of it [design].
pub const AREA_BLAST_SHARE: f32 = 0.25;
/// Melee: within this share of the reach the actor stops closing in [design].
pub const MELEE_ENGAGE_SHARE: f32 = 0.8;
/// Melee: the gap between the two bodies it closes to, and the gap within which its
/// blows land and beyond which it closes in again, game units [design].
pub const MELEE_STOP_GAP: f32 = 6.0;
pub const MELEE_HIT_GAP: f32 = 24.0;
/// Melee with bodies to measure: an attacker also keeps its weapon's reach between the
/// two bodies: its blows begin and land within its reach plus the two bodies' radii
/// (centre to centre), and it stops closing [`MELEE_STOP_SHORT`] inside that, never
/// nearer than the gaps above. In the original NCR troopers began their swings,
/// centre to centre, a median 95 units from the player with a knife (Reach 0.5), 120
/// with a sledgehammer (0.75), 150 a super sledge (1.0), 175 a bumper sword (1.2) and
/// 180 a lead pipe (1.25): about 45 + 107 × Reach [observed 2026-11-01, AI observer
/// campaign o14_reach, #1076]; with `fCombatDistance` 107 and two people's radii (22 +
/// 22) this gives 97.5, 124, 151, 172 and 178 [hypothesis: that the 45 is the bodies'
/// radii; docs/spec-notes/combat-rules.md §10]. Before, with half the radii and a
/// reach of 150 × Reach, a lead pipe began at 209.
/// Without bodies to measure, it stops at this share of its reach [design].
pub const MELEE_SPACING_SHARE: f32 = 0.9;
/// How far inside its strike distance a melee attacker stops closing, game units: the
/// original's knife and machete troopers began their swings a median 96 and 95 units
/// from their target and came no nearer than 92 and 91 during them [observed
/// 2026-10-17, AI observer campaign melee_spacing, docs/spec-notes/npc-behaviour-compare.md
/// §3]. With a share of the reach (0.9, before #1334) Violet's dogs began at 115 against
/// the original's 133, with this at 133-134 [observed: smoke combat-dogs, 2026-11-06].
pub const MELEE_STOP_SHORT: f32 = 6.0;
/// Two people's radii summed as `nvf_play::blocking` has them (22 each), for a melee
/// fighter's stop while the bodies are not known yet [design: a plan made then held:
/// Violet, teleported beside the player, stopped at 106 units with a lead pipe].
pub const PEOPLE_RADII: f32 = 44.0;
/// A creature without a reach of its own reaches this far, game units [design].
pub const CREATURE_REACH: f32 = 96.0;
/// A creature's blows reach this much beyond its own Attack Reach (CREA RNAM), before
/// its scale, centre to centre: its spacing is (RNAM + this) × scale ([`creature_spacing`]),
/// for a skeleton without a measured row of its own ([`creature_body_reach`]).
/// In the original the creatures began their swings a median 133 units from their
/// target (Violet's dogs, RNAM 48 × 1.2), 109 (coyotes, 48 × 1.0), 163 (giant mantises,
/// 100 × 1.0) and 84 (a giant rat, 25 × 1.1, 2 swings) [observed 2026-10-17, AI observer
/// campaigns melee_spacing and battles]; (RNAM + 59) × scale gives 128, 107, 159 and 92
/// [hypothesis: fitted to those rows, docs/spec-notes/npc-behaviour-compare.md §3; it
/// stands for the creature's body in front of its centre, which the records do not give
/// (the dogs' and coyotes' Object Bounds are 0), so a creature whose body is far longer
/// or shorter than these is not covered]. Ours, by the weapon rule, began them at 54
/// (the dogs).
pub const CREATURE_BODY_REACH: f32 = 59.0;

/// How far a creature's blows reach beyond its Attack Reach (RNAM), before its scale,
/// centre to centre, by its skeleton's folder: each fitted to the original's median
/// swing start against the standing god-mode player on the airport runway (start ÷
/// scale − RNAM, the creatures' leveled-list forms as `fnvtool esm decode` reads them)
/// [observed 2026-11-01, AI observer campaigns o14_reach and o12_reach, #1076;
/// docs/spec-notes/npc-behaviour-compare.md §3]; any other skeleton gets
/// [`CREATURE_BODY_REACH`] [hypothesis: that the reach beyond RNAM belongs to the
/// skeleton and grows with the creature's scale, as RNAM's does].
pub fn creature_body_reach(skeleton: Option<&str>) -> f32 {
    let Some(path) = skeleton else { return CREATURE_BODY_REACH };
    let path = path.to_ascii_lowercase().replace('/', "\\");
    let folder = path.rsplit('\\').nth(1).unwrap_or("");
    match folder {
        // Gecko (VCrTier2GeckoMed, RNAM 35 × 0.8): 81.
        "nvgecko" => 66.0,
        // Young cazador (VCrTier3CazadorSmall, 100 × 0.5): 80.
        "nvcazadores" => 60.0,
        // Young night stalker (VCrTier2NightStalkerSmall, 48 × 0.75): 102.
        "nightstalker" => 88.0,
        // Radscorpion (VCrTier2RadscorpionMed, 64 × 0.6): 97-129, the middle 113.
        "radscorpion" => 124.0,
        // Feral ghoul (VCrTier1FeralGhoul1A, 96 × 1.0): 135-140.
        "ghoul" => 42.0,
        // Bighorner bull (VCrTier3BigHornerLarge, 50 × 1.7): 232.
        "nvbighorner" => 86.0,
        // Coyotes (48 × 1.0) 109 (campaign melee_spacing; o12_reach 84-117) and Violet's
        // dogs (48 × 1.2) 133.
        "dog" => 62.0,
        _ => CREATURE_BODY_REACH,
    }
}

/// A creature's reach with its body in, centre to centre ([`creature_spacing`]): (its
/// Attack Reach + [`creature_body_reach`]) × its scale, or [`CREATURE_REACH`] × scale
/// without an Attack Reach.
pub fn creature_reach(rnam: Option<f64>, skeleton: Option<&str>, scale: f32) -> f32 {
    rnam.filter(|r| *r > 0.0).map_or(CREATURE_REACH, |r| r as f32 + creature_body_reach(skeleton)) * scale
}
/// A fleeing person hiding at its flee spot that its enemy has seen this long runs on
/// to another spot, at most [`HIDE_MOVES`] times a flight (§12.2.1) [design].
pub const HIDE_SEEN_SECONDS: f32 = 4.0;
pub const HIDE_MOVES: u8 = 2;
/// The 20 m sights rule (hip fire within, sights beyond) is shared with the player's VATS
/// mode shots in third person: `nvf_play::combat`.
pub use nvf_play::combat::{aims_down_sights, HIP_FIRE_WITHIN};
/// Without a muzzle node, shots leave this far in front of the eyes, game units [design].
pub(crate) const MUZZLE_AHEAD: f32 = 24.0;
/// The chest as a fraction of the eye height (as `detection.rs` aims its sight).
pub(crate) const CHEST: f32 = 0.75;
/// A gunman fires only while its target is within this angle of straight ahead,
/// radians (3°) [design: owner's playtest list 5 item 33, "NPCs must face their target
/// before shooting"; the original's Fiends fired only once turned within about 1.5° of
/// their target, 0.2 s after a 180° turn ended (observed 2026-10-25, observer campaign
/// fiend_facing); 30° before let ours fire while turning].
pub const FIRE_FACING: f32 = 3.0 * std::f32::consts::PI / 180.0;

/// How far off its heading a gunman may have its target and fire (radians): its weapon's
/// Aim Arc (WEAP DNAM, degrees) when above 0, else [`FIRE_FACING`]
/// (docs/spec-notes/weapon-data-fields.md §4) [hypothesis: 0 is no arc of its own].
pub fn fire_facing(aim_arc: f32) -> f32 {
    if aim_arc > 0.0 {
        aim_arc.to_radians()
    } else {
        FIRE_FACING
    }
}

/// How far (radians, 0 to π) the target at `to` lies off the heading of the actor whose
/// root is at `global`, standing at `feet` (game space).
pub(crate) fn facing_off(global: &GlobalTransform, feet: V3, to: V3) -> f32 {
    let ahead = bevy_to_game(global.transform_point(Vec3::Y)) - bevy_to_game(global.translation());
    let want = Vec2::new(to[0] - feet[0], to[1] - feet[1]);
    let have = ahead.truncate();
    if want.length() < 1.0 || have.length() < 1e-6 {
        return 0.0;
    }
    have.angle_to(want).abs()
}

/// The way an actor whose root is `global` faces, horizontal, game space.
fn heading(global: &GlobalTransform) -> [f32; 2] {
    let ahead = bevy_to_game(global.transform_point(Vec3::Y)) - bevy_to_game(global.translation());
    [ahead.x, ahead.y]
}

fn flat(a: V3, b: V3) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

pub struct NpcCombatPlugin;

impl Plugin for NpcCombatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Fighters>()
            .add_systems(Update, fight.after(nvf_play::PlaySet::Scripts).run_if(resource_exists::<Combat>))
            .add_systems(Last, report);
    }
}

/// How an actor attacks.
#[derive(Debug, Clone, PartialEq)]
enum Arm {
    Gun(FormId),
    Melee(Option<FormId>),
    /// A creature's own attack: damage and reach.
    Creature(f32, f32),
}

/// Ranged tactics do not imply firearm execution. A held grenade owns no magazine,
/// reload or queued gun shot, including while its dedicated controller waits [design].
enum RangedAttack {
    Firearm { cycle: Cycle, reload_timed: bool },
    Grenade,
}

impl RangedAttack {
    /// Keep a firearm's cycle on a new target, but never carry it into grenade
    /// execution (a queued WindUp can fire even with the trigger released).
    fn for_weapon(ws: &WeaponStats, kept: Option<Self>, make_cycle: impl FnOnce() -> Cycle) -> Self {
        if ws.anim_type.as_deref() == Some(crate::npc_grenades::GRENADE_ANIM) {
            Self::Grenade
        } else if let Some(gun @ Self::Firearm { .. }) = kept {
            gun
        } else {
            Self::Firearm { cycle: make_cycle(), reload_timed: false }
        }
    }

    fn reloading(&self) -> bool {
        matches!(self, Self::Firearm { cycle, .. } if cycle.reloading())
    }

    fn firearm_mut(&mut self) -> Option<(&mut Cycle, &mut bool)> {
        match self {
            Self::Firearm { cycle, reload_timed } => Some((cycle, reload_timed)),
            Self::Grenade => None,
        }
    }
}

/// One actor in a fight.
struct Fighter {
    target: FormId,
    arm: Arm,
    /// Seconds since it first saw its target in this fight; seconds it has fired at it.
    seen_for: f32,
    firing_for: f32,
    ranged: Option<RangedAttack>,
    /// Seconds to its next blow.
    strike_in: f32,
    /// The trigger was pulled last frame (a semi-automatic weapon is let go between
    /// shots).
    pulled: bool,
    /// Blows on their way: seconds until each lands, and the blow.
    blows: Vec<(f32, ActorStrike)>,
    /// Aiming down its sights now; how it last fired while ready (down the sights or
    /// from the hip), for the log: it may lower its sights while not ready (going to
    /// cover) and come back ready near enough to fire from the hip.
    sights: bool,
    ready_sights: Option<bool>,
    /// Seconds before a semi-automatic gun may be fired again (its combat style's
    /// firing delay).
    pull_in: f32,
    /// An automatic gun's burst under way and the pause after it (`fnv_combat::burst`).
    burst: fnv_combat::burst::Burst,
    /// This fight's confidence modifier; seconds to the next flee check.
    confidence_mod: f64,
    flee_check_in: f32,
    /// Cover, flank, line of fire (`combat_tactics`).
    tactic: Tactic,
    /// The start (`Flinch::at`, as bits) of the last hit reaction counted as holding it
    /// back.
    stagger_seen: Option<u32>,
    /// The serial of the last "No attacking" idle counted as holding it back.
    idle_hold_seen: Option<u32>,
    /// Its temperaments (docs/design/combat-ai.md §14.1), kept across targets.
    temper: Temperament,
    /// Crouched to fire since (clock); seconds stood still; its feet last frame; seconds
    /// to its next stance decision; seconds since it last saw its target (§14.2).
    crouched_since: Option<f32>,
    still_for: f32,
    last_feet: Option<V3>,
    stance_in: f32,
    unseen_for: f32,
    /// Blocking until (clock), raised against a blow (§14.3).
    block: Option<(f32, bool)>,
    /// A reaction to its opponent's blow: due at (clock), and when the blow lands.
    react: Option<(f32, f32)>,
    /// Its opponent's blows seen begun; its own blows begun.
    seen_opp_blows: u32,
    /// Kept while the shooter had its actor ([`hold_for_use`]): the opponent's blows begun
    /// meanwhile went unseen, so on its first frame back [`Fighter::seen_opp_blows`] is
    /// brought up to date without a reaction to blows already over (#2145 review).
    opp_blows_unseen: bool,
    blows_begun: u32,
    /// It cannot strike until then (clock): its blow was blocked.
    recoil_until: f32,
    /// Closing in on a gunman (§14.4).
    approach: Approach,
    /// Hiding crouched at its flee spot (§12.2.1); seconds seen there by its enemy; the
    /// times it ran on, seen, since it began to flee.
    hiding: bool,
    hide_seen: f32,
    hide_moves: u8,
    /// Its last melee blow's swing, for a combo (module notes).
    swing: Option<NpcSwing>,
    /// Its feet last frame and its horizontal velocity (units/s, game space), for a power
    /// attack chosen by movement (docs/spec-notes/combat-rules.md §10.1).
    walk_feet: Option<V3>,
    velocity: [f32; 2],
    /// Why its gun held fire by its reach last frame (§16), for the log.
    range_held: Option<RangeHold>,
    /// It has fired at its target in this fight (the first shot's distance is logged).
    opened_fire: bool,
    /// The maneuver decision's idle or dodge runs until then (clock); the dodge's post,
    /// and whether it was handed to the AI (§17.1).
    maneuver_until: f32,
    dodge: Option<Post>,
    /// A rush rolled for this approach: `Some(true)` rushing (§17.2).
    rush: Option<bool>,
    /// A creature's attack on the run rolled for this approach ([`lunge_band`]): rolled
    /// once as its target comes within the band, again after it has left it or
    /// [`RUN_ATTACK_AGAIN`] after the roll (`lunge_at`, clock).
    lunge_rolled: bool,
    lunge_at: f32,
    /// Where its target's feet were last frame and how the target moves on the ground,
    /// units/s, for how fast it gains on it ([`closing_speed`]).
    target_feet: Option<(FormId, V3)>,
    target_velocity: [f32; 2],
    /// Where its fight began or it was last hit, and its health then; beyond its Combat
    /// Radius now (§17.4).
    anchor: V3,
    anchor_health: f64,
    leashed: bool,
    /// The live grenade it runs from, and where to (`None` once the AI found no way there)
    /// (§17.7).
    evading: Option<(u64, Option<Post>)>,
    /// Whom its explosive shot would catch, last frame (§17.6).
    blast_held: Option<u32>,
    /// A dodge or a grenade run was handed to the AI last frame.
    urgent_posted: Option<PostKind>,
}

/// A fighter's last melee blow (module notes).
#[derive(Debug, Clone, Copy, Default)]
struct NpcSwing {
    /// Seconds since it began.
    t: f32,
    /// Its clip's timing, once `attack_anims` started it.
    clip: Option<crate::attack_anims::SwingClip>,
    /// Blows in this combo so far, this one included.
    run: u32,
    /// Powers publish timing but do not continue combos [design: existing combo rule].
    power: bool,
    /// The blow's attack speed (Attack Mult × the teammate rate; 1 for a creature): a
    /// creature's clip key and end are divided by it (`attack_anims::SwingClip::ready_in`,
    /// #2535 review); a person's clip keeps its own times.
    speed: f32,
    /// A creature's blow (`Arm::Creature`): the next decision waits for its clip's `a:`
    /// key or end. A person's keeps the base timing (module notes; the maintainer's
    /// decision of 2026-11-18 on #2971).
    creature: bool,
}

impl NpcSwing {
    /// The next decision's wait once the clip is known: a creature's waits for the clip
    /// (`SwingClip::ready_in`, shared with scripted melee); a person's is the base wait,
    /// brought forward by an earlier `a:` key (`SwingClip::person_ready_in`).
    fn ready_in(&self, base_left: f32) -> f32 {
        match self.clip {
            Some(c) if self.creature => c.ready_in(self.t, base_left, self.speed),
            Some(c) => c.person_ready_in(self.t, base_left),
            None => base_left,
        }
    }

    /// The clock its clip's times are divided by: the attack speed for a creature, 1 (the
    /// clip's own times) for a person.
    fn rate(&self) -> f32 {
        if self.creature { self.speed } else { 1.0 }
    }

    /// Whether the clip published for this blow is taken: always for a creature; for a
    /// person not a power attack's (its wait is its clip's length, set at the blow, and it
    /// lands at its power clip's `Hit` key).
    fn takes_clip(&self) -> bool {
        self.creature || !self.power
    }

    /// What a blow begun now goes on with: the group its `a:` key named, between that key
    /// and the clip's end, both at the blow's rate (`SwingClip::next_at`).
    fn next(&self) -> Option<fnv_combat::melee::NextAttack> {
        if self.power {
            return None;
        }
        self.clip?.next_at(self.t, self.rate())
    }

    /// Seconds into the swing after which it is over: its clip's end at its rate, or
    /// twice the base pace before its clip is known.
    fn ends_at(&self) -> f32 {
        self.clip.map_or(STRIKE_SECONDS * 2.0, |c| c.end_at(self.rate()))
    }
}

/// A melee fighter closing in (docs/design/combat-ai.md §14.4).
#[derive(Debug, Clone, Default)]
struct Approach {
    guard: Option<Guarding>,
    /// A guard ended in this fight (it does not guard again).
    guarded: bool,
    bound: Option<Bound>,
    /// Seconds to its next look for a bound.
    bound_in: f32,
    /// Charging in the open (logged once per run).
    charging: bool,
    /// The post it was given last frame.
    posted: Option<PostKind>,
}

#[derive(Debug, Clone, Copy)]
struct Guarding {
    friend: u32,
    since: f32,
    patience: f32,
    turn: f32,
    at: V3,
}

#[derive(Debug, Clone, Copy)]
struct Bound {
    at: V3,
    arrived: Option<f32>,
    pause: f32,
}

impl Approach {
    /// The post it was given last frame is gone: the AI dropped it (no way there, or its
    /// walk there ended away from it, §12.1). That ends what it was for.
    fn post_dropped(&mut self) {
        match self.posted {
            Some(PostKind::Guard) => {
                self.guard = None;
                self.guarded = true;
            }
            Some(_) => {
                self.bound = None;
                self.bound_in = stance::BOUND_RETRY;
            }
            None => {}
        }
    }
}

/// A fighter's pose this frame, for `attack_anims`.
#[derive(Debug, Clone, Copy, Default)]
struct Pose {
    /// Its pitch to its target (radians, negative looks down).
    pitch: f32,
    weapon: Option<FormId>,
    sights: bool,
    crouched: bool,
    blocking: bool,
    /// The flat distance from its feet to its target's, units.
    distance: f32,
    /// A melee fighter's: where its walk stops short of its target (feet to feet,
    /// units).
    stop: Option<f32>,
    /// A melee fighter's: feet to feet when its body touches its target's (units); a
    /// lunge carries it no nearer ([`Fighters::lunge_room`]).
    contact: Option<f32>,
}

#[derive(Resource, Default)]
pub struct Fighters {
    fighters: HashMap<u32, Fighter>,
    /// When each gunman may next throw a grenade, and at whom that wait was rolled
    /// (`npc_grenades`): kept over a change of fighter, rolled again for a new target.
    grenade_at: HashMap<u32, (FormId, f32)>,
    /// The grenade rolls' own sequence, as `stance_rng`.
    grenade_rng: u64,
    /// Each fighter's pose this frame.
    poses: HashMap<u32, Pose>,
    /// Co-op puppets' poses (coop.rs): kept until changed, not cleared each frame.
    puppets: HashMap<u32, Pose>,
    /// People locked on their target this frame: they see it and are not running from
    /// it; where its feet are drawn (`residents` turns them to it and holds them on it,
    /// [`Fighters::lock_on`]); keyed by the person, with its target.
    locks: HashMap<u32, (u32, V3)>,
    /// Last frame's [`Fighters::locks`]: where each locked person was drawn facing.
    drawn_locks: HashMap<u32, (u32, V3)>,
    stats: Stats,
    rng: u64,
    /// The crouch, block and approach rolls' own sequence (§14), so fights without them
    /// roll as before.
    stance_rng: u64,
    /// The creatures' power attack rolls' own sequence (module notes).
    power_rng: u64,
    /// People's power attack rolls' own sequence, so fights without them roll as before.
    people_power_rng: u64,
    /// The combo swings' variant rolls' own sequence, as `stance_rng`.
    swing_rng: u64,
    /// The maneuver decision's and rushes' own sequence, as `stance_rng` (§17).
    maneuver_rng: u64,
    /// The power rolls of creatures' lunges on the run (`lunge_band`), a sequence of their own
    /// so the other rolls fall as before.
    lunge_rng: u64,
    /// (fighter, target) pairs held back by the Max Targeting FOV, logged once (§17.5).
    fov_waits: std::collections::HashSet<(u32, u32)>,
    /// The player's swings seen so far (`CombatStats::swings`), and when the last began.
    player_swings: u32,
    player_swing_at: f32,
}

/// A co-op puppet's aim (coop.rs), drawn as a fighter's ([`Fighters::set_puppet`]).
#[derive(Debug, Clone, Copy, Default)]
pub struct PuppetPose {
    pub pitch: f32,
    pub weapon: Option<FormId>,
    pub sights: bool,
    pub crouched: bool,
}

impl Fighters {
    /// A co-op puppet aims (and may attack) as a fighter does, or (None) stands down.
    pub fn set_puppet(&mut self, actor: u32, pose: Option<PuppetPose>) {
        match pose {
            Some(p) => {
                self.puppets.insert(actor, Pose { pitch: p.pitch, weapon: p.weapon, sights: p.sights, crouched: p.crouched, ..Pose::default() });
            }
            None => {
                self.puppets.remove(&actor);
            }
        }
    }

    fn pose(&self, actor: u32) -> Option<&Pose> {
        self.poses.get(&actor).or_else(|| self.puppets.get(&actor))
    }

    /// The pitch from a fighting actor's eyes to its target's chest, while it fights.
    pub fn pitch(&self, actor: u32) -> Option<f32> {
        self.pose(actor).map(|p| p.pitch)
    }

    /// The weapon drawn on a fighting actor.
    pub fn weapon(&self, actor: u32) -> Option<FormId> {
        self.pose(actor).and_then(|p| p.weapon)
    }

    /// Whether a fighting actor aims down its weapon's sights.
    pub fn sights(&self, actor: u32) -> bool {
        self.pose(actor).is_some_and(|p| p.sights)
    }

    /// Whether a fighting actor is crouched to fire (docs/design/combat-ai.md §14.2).
    pub fn crouched(&self, actor: u32) -> bool {
        self.pose(actor).is_some_and(|p| p.crouched)
    }

    /// Whether a fighting actor blocks (§14.3).
    pub fn blocking(&self, actor: u32) -> bool {
        self.pose(actor).is_some_and(|p| p.blocking)
    }

    /// The flat distance from a fighting actor's feet to its target's, units.
    pub fn distance(&self, actor: u32) -> Option<f32> {
        self.poses.get(&actor).map(|p| p.distance)
    }

    /// How much nearer a melee fighter may come to its target before its body touches
    /// the target's (where its walk stops when the bodies are not known), units (0 when
    /// there already): a lunge's clip carries it no further (`attack_anims::carry`,
    /// docs/spec-notes/actors.md §8.13) [design].
    pub fn lunge_room(&self, actor: u32) -> Option<f32> {
        self.poses.get(&actor).and_then(|p| Some((p.distance - p.contact.or(p.stop)?).max(0.0)))
    }

    /// Whether a fighting actor reloads (it moves slower then, limping.rs).
    pub fn reloading(&self, actor: u32) -> bool {
        self.fighters.get(&actor).and_then(|f| f.ranged.as_ref()).is_some_and(RangedAttack::reloading)
    }

    /// Whether a fighting actor reloads or has a blow on its way (it does not start using
    /// a stimpak then, `npc_stimpaks.rs`).
    pub fn busy(&self, actor: u32) -> bool {
        self.fighters.get(&actor).is_some_and(|f| !f.blows.is_empty() || f.ranged.as_ref().is_some_and(RangedAttack::reloading))
    }
}

#[derive(Default)]
struct Stats {
    fighters: BTreeMap<u32, String>,
    shots: usize,
    strikes: usize,
    reloads: usize,
    /// Reloads that loaded round by round.
    round_reloads: usize,
    /// Blows whose target had moved out of reach by the time they landed.
    missed: usize,
    /// Shots from a weapon's muzzle, and from the eyes (no muzzle node).
    from_muzzle: usize,
    from_eyes: usize,
    /// Shots aimed down the sights and from the hip; times an actor raised its sights.
    sights_shots: usize,
    hip_shots: usize,
    sights_raised: usize,
    /// Melee decisions to hold instead of attacking; flee checks made and flights begun.
    holds: usize,
    /// Blows that went on a combo, and the longest combo.
    chained: usize,
    longest_combo: u32,
    /// Feet-to-feet distances (2D, centre to centre) to the target when each melee blow
    /// began, and the closest a melee fighter came to its target: to compare with the AI
    /// observer's melee spacing in the original (docs/spec-notes/npc-behaviour-compare.md).
    swing_distances: Vec<f32>,
    melee_closest: Option<f32>,
    flee_checks: usize,
    flees: usize,
    /// Hiding at flee spots (§12.2.1): times a fleeing person crouched to hide, and
    /// times it ran on, seen.
    hides: usize,
    hides_seen: usize,
    /// Hit reactions (crippled staggers) that held a fighter back, and blows being
    /// swung that they cut short.
    staggers: usize,
    blows_cut: usize,
    /// Idles flagged "No attacking" that held a fighter back.
    idle_holds: usize,
    /// Frames in which a fighter that could have attacked waited to face its target.
    unfaced: usize,
    /// How far off its target a fighter faced (degrees, its drawn body): a gunman at each
    /// shot, and a person the AI runs locked on its target in each frame after its
    /// reaction time. The original holds within about
    /// 1.5° in every phase of the attack [observed 2026-10-25, observer campaign
    /// fiend_facing].
    shot_offs: Vec<f32>,
    seen_offs: Vec<f32>,
    /// Crouching (§14.2): times crouched to fire, behind low cover; stood up, for a
    /// close-combat enemy; shifts.
    crouches: usize,
    low_crouches: usize,
    stands: usize,
    stands_close: usize,
    shifts: usize,
    /// Blocking (§14.3): blocks raised by decision and in reaction; blows blocked by
    /// people; recoils.
    blocks: usize,
    reactive_blocks: usize,
    blocked: usize,
    recoils: usize,
    /// Closing in (§14.4): bounds, charges in the open, guards begun.
    bounds: usize,
    charges: usize,
    guards: usize,
    /// Power attacks by attack group, creatures' and people's.
    powers: BTreeMap<&'static str, usize>,
    /// People's power attacks rolled for but without a clip for the direction chosen
    /// (an ordinary blow instead).
    powers_unplayable: usize,
    /// Guns' reach (§16): times a fighter held fire at a target beyond its reach and
    /// within its blast; times it left cover to close in; the farthest shot as a share
    /// of the gun's reach, and shots beyond it.
    held_far: usize,
    held_blast: usize,
    left_cover_for_reach: usize,
    farthest_share: f32,
    shots_beyond: usize,
    /// The combat style's other fields (§17): dodges left, right, forward, back; idles;
    /// rushes; pursuits stopped at the combat radius; targets left outside the FOV;
    /// explosive shots held for a comrade or a bystander; runs from grenades.
    dodges: [usize; 4],
    idles: usize,
    rushes: usize,
    /// Creatures' attacks begun on the run, short of their reach, and the forward power
    /// attacks (lunges) among them ([`lunge_band`]).
    run_attacks: usize,
    lunges: usize,
    leashes: usize,
    fov_waits: usize,
    blast_holds: usize,
    evades: usize,
    /// Grenades thrown, and throws held for a comrade or a bystander (`npc_grenades`).
    grenades: usize,
    grenade_holds: usize,
}

/// The chance (percent) of a creature's blow being a power attack where its combat style
/// gives none (module notes).
pub const FALLBACK_POWER_CHANCE: f32 = 25.0;

/// Whether a creature's blow is a power attack, and which with its hit time (module
/// notes): `have`, the power attacks it has clips for; `u` and `v`, rolls in [0, 1).
/// Also says whether the chance was the fallback.
/// The Modify Attack Speed perks a teammate of the player holds for `weapon` (Old
/// Vaquero, Causeless Rebel: `Game::held_perks`; perk-entry-points.md §12): a multiple of
/// its attack speed, 1 for anyone else. Its gun's shot cycle and its blows' timing take it.
pub(crate) fn teammate_attack_rate(g: &mut fnv_game::game::Game, me: FormId, weapon: Option<FormId>) -> f32 {
    if !g.is_teammate(me) {
        return 1.0;
    }
    let (v, applied) = g.entry_point_with(me, fnv_game::perks::MODIFY_ATTACK_SPEED, &[weapon], 1.0);
    let v = (v as f32).clamp(0.1, 10.0);
    if !applied.is_empty() {
        let by: Vec<String> = applied.iter().map(|a| g.data.name(a.perk)).collect();
        debug!("npc combat: {} {me} attacks at x{v:.2} ({})", g.data.name(me), by.join(", "));
    }
    v
}

fn pick_power(style: Option<&fnv_game::data::CombatStyleDef>, have: &[(Power, f32)], fatigue: f32, u: f32, v: f32) -> (Option<(Power, f32)>, bool) {
    let own = style.filter(|s| s.power_attack_chance > 0.0).map(|s| creature_power_chance(s, fatigue));
    if have.is_empty() || u * 100.0 >= own.unwrap_or(FALLBACK_POWER_CHANCE) {
        return (None, own.is_none());
    }
    let share = |p: Power| style.map_or(0.0, |s| s.power_attack_dirs[p.index()].max(0.0));
    let total: f32 = have.iter().map(|h| share(h.0)).sum();
    if total <= 0.0 {
        return (Some(have[((v * have.len() as f32) as usize).min(have.len() - 1)]), own.is_none());
    }
    let mut x = v * total;
    for &h in have {
        x -= share(h.0);
        if x < 0.0 {
            return (Some(h), own.is_none());
        }
    }
    (have.last().copied(), own.is_none())
}

/// A creature's power attack chance: its style's, plus the Power Attack Fatigue Modifier
/// at `fatigue` (docs/design/combat-ai.md §17.3).
fn creature_power_chance(s: &fnv_game::data::CombatStyleDef, fatigue: f32) -> f32 {
    (s.power_attack_chance + fnv_game::combat_style::power_attack_fatigue(s, fatigue)).clamp(0.0, 100.0)
}

/// The speed (units/s) above which a person moves for a power attack chosen by movement;
/// slower, it stands (docs/spec-notes/combat-rules.md §10.1) [design].
pub const POWER_MOVE_SPEED: f32 = 20.0;

/// A horizontal velocity against the way an actor faces (`ahead`, horizontal): (forward,
/// right) of its direction, unit length, or zeros when slower than [`POWER_MOVE_SPEED`].
fn local_motion(velocity: [f32; 2], ahead: [f32; 2]) -> (f32, f32) {
    let speed = velocity[0].hypot(velocity[1]);
    let len = ahead[0].hypot(ahead[1]);
    if speed < POWER_MOVE_SPEED || len < 1e-6 {
        return (0.0, 0.0);
    }
    let (v, a) = ([velocity[0] / speed, velocity[1] / speed], [ahead[0] / len, ahead[1] / len]);
    // Right of ahead in game space (x east, y north, z up): (a.y, −a.x).
    (v[0] * a[0] + v[1] * a[1], v[0] * a[1] - v[1] * a[0])
}

/// What a person's melee decision made of a power attack (docs/spec-notes/combat-rules.md
/// §10.1).
#[derive(Debug, Clone, Copy, PartialEq)]
struct PeoplePick {
    /// The power attack it plays, if any.
    played: Option<PowerClip>,
    /// One it chose but has no clip for (an ordinary blow instead).
    unplayable: Option<Power>,
    /// The chance it rolled against, and how the direction was chosen, for the log.
    chance: f32,
    how: &'static str,
}

/// Whether a person's blow is a power attack, and which (docs/spec-notes/combat-rules.md
/// §10.1): by its style's chance and bonuses (`opp`), the direction by the style's shares
/// ("Choose Attack using % Chance") or by `moving` ((forward, right), [`local_motion`]),
/// standing with both legs crippled; among `have`, its rig's clips. `rolls` in [0, 1):
/// whether, and which by chance. Without a style, the creatures' fallback chance with even
/// shares.
fn pick_people_power(style: Option<&fnv_game::data::CombatStyleDef>, opp: Opponent, fatigue: f32, have: &[PowerClip], moving: (f32, f32), both_lame: bool, rolls: [f32; 2]) -> PeoplePick {
    let (chance, by_chance, shares) = match style {
        Some(s) => (fnv_game::combat_style::power_attack_chance(s, opp, fatigue), s.power_by_chance, s.power_attack_dirs),
        None => (FALLBACK_POWER_CHANCE, true, [20.0; 5]),
    };
    let how = match (style.is_some(), by_chance) {
        (false, _) => "fallback: no combat style",
        (true, true) => "by chance",
        (true, false) => "by movement",
    };
    let none = PeoplePick { played: None, unplayable: None, chance, how };
    if have.is_empty() || rolls[0] * 100.0 >= chance {
        return none;
    }
    let want = if both_lame {
        Power::Normal
    } else if by_chance {
        Power::ALL[fnv_game::combat_style::power_dir_by_chance(shares, rolls[1])]
    } else {
        let dir = fnv_combat::melee::PowerDir::of(moving.0, moving.1);
        Power::ALL.into_iter().find(|p| p.dir() == dir).unwrap_or(Power::Normal)
    };
    match have.iter().find(|p| p.power == want) {
        Some(p) => PeoplePick { played: Some(*p), ..none },
        None => PeoplePick { unplayable: Some(want), ..none },
    }
}

/// The power attacks a creature can make: with a crippled leg, none forward, the leap
/// (a deathclaw's) [observed: FalloutNV.esm MESG `HelpCrippling`, "Legs: … Enemies cannot
/// perform leaping or charging attacks"; documented: Fallout wiki crippling text, owner
/// chat 2026-10-18, "leap attacks suffer (deathclaws, for example)"; hypothesis: the
/// forward power attack is the leap].
fn no_leaps(have: &[(Power, f32)], lame: bool) -> Vec<(Power, f32)> {
    have.iter().copied().filter(|(p, _)| !(lame && *p == Power::Forward)).collect()
}

/// A creature closing in at least this fast (units/s, towards its target) may begin a
/// blow before it is within reach ([`lunge_band`]) [design].
pub const RUN_ATTACK_SPEED: f32 = 100.0;
/// A blow on the run begins at most this far beyond the strike distance, units [design: a
/// leap of about 3 m (at 300 a Goodsprings dog lunged from 419 units); the measured closing speed is also capped at the creature's own
/// running speed, as a frame's step after a MoveTo or a physics snap is no run].
pub const RUN_ATTACK_MAX: f32 = 200.0;
/// A creature still short of its target this long after its attack on the run was rolled
/// may begin another, seconds [design: owner's playtest 2026-11-11, #2163: rolled once an
/// approach, a creature chasing a player who backed away within [`RUN_ATTACK_MAX`] of
/// its reach never began a second].
pub const RUN_ATTACK_AGAIN: f32 = 1.0;
/// A target's measured speed above this is a move by a script or a physics snap, not
/// its pace, units/s ([`closing_speed`]) [engineering choice].
pub const TARGET_SPEED_MAX: f32 = 2000.0;

/// How fast an attacker gains on its target along the line to it (`to`, attacker to
/// target), units/s: its own speed that way, at most `cap` (its running speed, as a
/// frame's step after a MoveTo is no run), less the target's speed away from it. A
/// target backing away from a faster creature is gained on at the difference, so the
/// blow begun on the run ([`lunge_band`]) lands [design, #2163: with the attacker's own
/// speed alone a cazador began its sting 100 units too far from a player backing away].
pub fn closing_speed(own: [f32; 2], target: [f32; 2], to: [f32; 2], cap: f32) -> f32 {
    let len = to[0].hypot(to[1]).max(1e-3);
    let along = |v: [f32; 2]| (v[0] * to[0] + v[1] * to[1]) / len;
    along(own).min(cap) - along(target)
}

/// How far beyond its strike distance a creature closing in at `closing` units/s may begin
/// a blow whose clip reaches its `Hit` key after `hit` seconds: the ground it covers until
/// then (owner's playtest list 7 item 36: feral ghouls and radscorpions waited until they
/// stood next to the player, then swung after a pause; they should lunge and attack on the
/// move) [design: the blow still lands only if the target is within reach at the `Hit`
/// key, as every blow]. 0 below [`RUN_ATTACK_SPEED`].
pub fn lunge_band(closing: f32, hit: f32) -> f32 {
    if closing < RUN_ATTACK_SPEED {
        0.0
    } else {
        (closing * hit.max(0.0)).min(RUN_ATTACK_MAX)
    }
}

/// The alert walking speed of an actor the AI does not run, units/s (`fnv_ai::AiConfig`'s
/// default).
pub const DEFAULT_WALK: f32 = fnv_ai::NPC_WALK;
/// A dodge is no shorter than this, units; shorter, the actor idles (§17.1) [design].
pub const DODGE_MIN: f32 = 32.0;
/// How far beyond a live grenade's blast a fighter runs to, units (§17.7) [design].
pub const EVADE_MARGIN: f32 = 64.0;
/// A fighter does not run from its own grenade this soon after it left (§17.7) [design].
pub const OWN_GRENADE_GRACE: f32 = 0.5;

/// Where a dodge `dist` units long takes a fighter at `feet` fighting a target at `tfeet`
/// (§17.1): left, right, back or forward of the line to it; forward no nearer than
/// `stop` (its melee stop distance, centre to centre). `None` when shorter than
/// [`DODGE_MIN`].
pub fn dodge_point(feet: V3, tfeet: V3, dir: fnv_game::combat_style::DodgeDir, dist: f32, stop: f32) -> Option<V3> {
    use fnv_game::combat_style::DodgeDir;
    let (dx, dy) = (tfeet[0] - feet[0], tfeet[1] - feet[1]);
    let away = dx.hypot(dy);
    if away < 1e-3 {
        return None;
    }
    let ahead = [dx / away, dy / away];
    // Right of ahead in game space (x east, y north): (a.y, −a.x).
    let (way, dist) = match dir {
        DodgeDir::Forward => (ahead, dist.min(away - stop)),
        DodgeDir::Back => ([-ahead[0], -ahead[1]], dist),
        DodgeDir::Right => ([ahead[1], -ahead[0]], dist),
        DodgeDir::Left => ([-ahead[1], ahead[0]], dist),
    };
    (dist >= DODGE_MIN).then(|| [feet[0] + way[0] * dist, feet[1] + way[1] * dist, feet[2]])
}

/// Where a fighter at `feet` runs to from a grenade at `at` with blast `radius` (§17.7):
/// [`EVADE_MARGIN`] beyond the radius, straight away from it (east when on top of it).
pub fn evade_point(feet: V3, at: V3, radius: f32) -> V3 {
    let (dx, dy) = (feet[0] - at[0], feet[1] - at[1]);
    let d = dx.hypot(dy);
    let way = if d < 1e-3 { [1.0, 0.0] } else { [dx / d, dy / d] };
    let reach = radius + EVADE_MARGIN;
    [at[0] + way[0] * reach, at[1] + way[1] * reach, feet[2]]
}

/// A number in [0, 1) from the module's own sequence `rng` [design].
/// The spread of an NPC's grenade throw, degrees [design].
pub const GRENADE_CONE: f32 = 2.0;

pub(crate) fn next_unit(rng: &mut u64) -> f32 {
    *rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    (*rng >> 40) as f32 / (1u64 << 24) as f32
}

/// Damage per second of an attack, for the threat ratio [design: the documentation does
/// not say how the original rates it]: a gun's damage per shot (all its projectiles)
/// times its shots per second (its fire rate for automatic weapons, else its attack
/// shots per second); a blow's damage over [`STRIKE_SECONDS`] at the weapon's attack
/// speed.
fn dps(c: &Combat, arm: &Arm) -> f32 {
    let w = |id: Option<FormId>| id.or(c.fists_id()).and_then(|id| c.forms.weapons.get(&id));
    match arm {
        Arm::Gun(id) => w(Some(*id)).map_or(0.0, |ws| {
            let per_second = if ws.automatic && ws.fire_rate > 0.0 { ws.fire_rate } else if ws.attack_shots_per_sec > 0.0 { ws.attack_shots_per_sec } else { 1.0 };
            ws.base_damage.max(0) as f32 * per_second
        }),
        Arm::Melee(id) => w(*id).map_or(0.0, |ws| ws.base_damage.max(0) as f32 * ws.attack_mult.max(0.25) / STRIKE_SECONDS),
        Arm::Creature(d, _) => *d / STRIKE_SECONDS,
    }
}

/// The player's attack, for the threat ratio of those fighting the player: the weapon in
/// use, a gun or a blow.
fn player_arm(c: &Combat) -> Arm {
    match c.player_weapon() {
        Some(w) if c.forms.weapons.get(&w).is_some_and(|ws| is_gun(c, ws, w)) => Arm::Gun(w),
        w => Arm::Melee(w),
    }
}

/// How near an actor fighting with a melee reach or a gun's max range wants to be, and
/// how far its target may be before it closes in again (module notes): a gun stops at
/// [`GUN_RANGE_SHARE`] of its range and goes again beyond its range; melee stops at
/// [`MELEE_ENGAGE_SHARE`] of its reach and goes again beyond its reach [design].
/// A melee attacker's (stop, strike) distances, centre to centre, from how far apart the
/// two bodies' blocks keep their centres (`bodies`: the distance less the gap, shorter
/// than `radii` when a torso leans or a long body reaches ahead), the two bodies' radii
/// summed (`radii`), and its reach ([`MELEE_STOP_SHORT`]): the reach counts beyond
/// the radii, the gaps beyond the blocks.
pub fn melee_spacing(bodies: f32, radii: f32, reach: f32) -> (f32, f32) {
    let strike = reach + radii;
    ((bodies + MELEE_STOP_GAP).max(strike - MELEE_STOP_SHORT), (bodies + MELEE_HIT_GAP).max(strike))
}

/// A creature's (stop, strike) distances, centre to centre, from its reach with its body
/// already in ([`creature_reach`]): it stops [`MELEE_STOP_SHORT`] inside it and
/// strikes within it, never nearer than the bodies' gaps when the two bodies (`bodies`,
/// their radii summed as in [`melee_spacing`]) are known.
pub fn creature_spacing(bodies: Option<f32>, reach: f32) -> (f32, f32) {
    let b = bodies.unwrap_or(0.0);
    ((b + MELEE_STOP_GAP).max(reach - MELEE_STOP_SHORT), (b + MELEE_HIT_GAP).max(reach))
}

/// Combat's snapshot of walking blocks, absent from the drawn-block list [design:
/// nvf_play::player, #2535]. An ordinary walking character supplies the other body
/// of a widened pair, even after the creature stops; ordinary pairs retain their
/// existing drawn-block lookup [design, #2535].
pub(crate) struct CombatBodies<'a> {
    walking: HashMap<u32, (Block, bool)>,
    widened: std::collections::HashSet<u32>,
    drawn: &'a [Block],
    player_radius: Option<f32>,
}

impl<'a> CombatBodies<'a> {
    pub(crate) fn new(phys: Option<&PhysState>, blocks: Option<&'a ActorBlocks>) -> Self {
        let walking = phys.map(|p| p.npcs.iter().map(|(&id, n)| {
            let radius = nvf_play::blocking::walking_radius(n.ch.params.radius, p.actor_block_radii.get(&id).copied());
            (id, (Block::character(id, n.ch.feet, radius, n.ch.params.height, false), radius > n.ch.params.radius))
        }).collect()).unwrap_or_default();
        // Eligibility outlives the walking character: standing creatures keep their
        // BBX block in the drawn list [design: hitvol::sync_volumes, #2535].
        let widened = phys.map(|p| p.actor_block_radii.iter().filter_map(|(&id, &bbx)| {
            let capsule = p.npcs.get(&id).map_or_else(
                || nvf_play::phys::npc_params(p.actor_heights.get(&id).copied()).radius,
                |n| n.ch.params.radius,
            );
            (nvf_play::blocking::walking_radius(capsule, Some(bbx)) > capsule).then_some(id)
        }).collect()).unwrap_or_default();
        Self {
            walking,
            widened,
            drawn: blocks.map_or(&[], |b| b.list.as_slice()),
            player_radius: phys.and_then(|p| p.player.as_ref()).map(|c| c.params.radius),
        }
    }

    fn body(&self, id: u32, other: u32) -> Option<Block> {
        self.walking.get(&id).filter(|(_, widened)| *widened).map(|(b, _)| *b)
            .or_else(|| self.drawn.iter().find(|b| b.actor == id).copied())
            .or_else(|| {
                if !self.widened.contains(&other) { return None; }
                self.walking.get(&id).map(|(b, _)| *b)
            })
    }

    fn pair(&self, me: u32, target: u32) -> Option<(Block, f32)> {
        let mine = self.body(me, target)?;
        let radius = if target == FormId::PLAYER_REF.0 { self.player_radius? } else { self.body(target, me)?.radius };
        Some((mine, radius))
    }

    pub(crate) fn gap(&self, me: u32, target: u32, tfeet: V3) -> Option<f32> {
        let (mine, radius) = self.pair(me, target)?;
        Some(mine.room([tfeet[0], tfeet[1]], radius))
    }

    /// [`CombatBodies::gap`] for an actor whose feet are at `feet`: none when its block
    /// is not where it stands (further from its own feet than [`STALE_BLOCK`] beyond the
    /// block's radius). The drawn blocks follow the drawn pose and can be far behind an
    /// actor on the move: a gap measured from such a block read 1,161 units for a young
    /// gecko whose feet were 51 units from the player, and up to 3,355 for a feral ghoul
    /// [observed: runs of main 069ef9bde on the Searchlight Airport runway, 2026-11-21,
    /// #3256]. Without a gap the spacing falls back to the reach alone, as it does for
    /// bodies that are not known.
    pub(crate) fn gap_at(&self, me: u32, target: u32, feet: V3, tfeet: V3) -> Option<f32> {
        let (mine, radius) = self.pair(me, target)?;
        if mine.room([feet[0], feet[1]], 0.0) > STALE_BLOCK {
            return None;
        }
        Some(mine.room([tfeet[0], tfeet[1]], radius))
    }

    pub(crate) fn radii(&self, me: u32, target: u32) -> Option<f32> {
        let (mine, radius) = self.pair(me, target)?;
        Some(mine.radius + radius)
    }
}

/// A block further than this from its own actor's feet (beyond its radius, units) is not
/// where the actor is ([`CombatBodies::gap_at`]) [design, #3256: a torso that leans or a
/// long body reaches ahead by less; the stale blocks seen were hundreds of units off].
pub const STALE_BLOCK: f32 = 48.0;

/// The strike-range decision shared by melee attacks and their pending blows
/// [design: fight, melee_spacing, creature_spacing].
fn in_melee_reach(arm: &Arm, away: f32, gap: Option<f32>, radii: Option<f32>, reach: f32) -> bool {
    match (arm, gap) {
        (Arm::Creature(..), gap) => away <= creature_spacing(gap.map(|gap| away - gap), reach).1,
        (_, gap) => gap.map_or(away <= reach, |gap| away <= melee_spacing(away - gap, radii.unwrap_or(away - gap), reach).1),
    }
}

/// What a gun reaches and how near it is safe to fire (docs/design/combat-ai.md §16).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GunReach {
    /// The farthest it fires, eyes to the target's chest; `None`: no limit in its data.
    pub reach: Option<f32>,
    /// Its projectile's explosion radius; 0 without one.
    pub blast: f32,
    /// Its projectile is a flame (PROJ type Flame).
    pub flame: bool,
    /// Its projectile's own range (PROJ Range); `None`: no limit in its data. An ordinary
    /// firearm fires at any target within it (owner's playtest list 7 item 40,
    /// [`GunReach::limited`]).
    pub projectile: Option<f32>,
}

/// Why a gun holds fire at a target (docs/design/combat-ai.md §16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeHold {
    /// Beyond its reach.
    Far,
    /// Within its own explosion's radius.
    Blast,
}

/// A gun's reach from its WEAP max range times its style's Ranged Weapon Max Range Mult
/// (`range_mult`) [documented: GECK wiki Weapons, Combat Style], never beyond its
/// projectile's range ("Maximum distance the projectile will fire" [documented: GECK
/// wiki Projectile]); 0 for a range means it has none. `blast`: its projectile's
/// explosion radius, 0 without one.
pub fn gun_reach(max_range: f32, range_mult: f32, projectile_range: f32, blast: f32) -> GunReach {
    let weapon = (max_range > 0.0).then_some(max_range * range_mult.max(0.0));
    let projectile = (projectile_range > 0.0).then_some(projectile_range);
    let reach = match (weapon, projectile) {
        (Some(w), Some(p)) => Some(w.min(p)),
        (w, p) => w.or(p),
    };
    GunReach { reach, blast: blast.max(0.0), flame: false, projectile }
}

impl GunReach {
    /// The distance it keeps from its target for its own explosion: the radius plus
    /// [`BLAST_MARGIN`]; 0 without one.
    pub fn clearance(&self) -> f32 {
        if self.blast > 0.0 {
            self.blast + BLAST_MARGIN
        } else {
            0.0
        }
    }

    /// Whether its reach also bars its shots: a flame or a projectile that explodes (the
    /// flamer, missiles, grenades and other explosives). An ordinary firearm keeps its
    /// reach only as the distance it fights from ([`GunReach::engage`]) and fires out to
    /// its projectile's range [design: owner's playtest list 7 item 40, "remove the
    /// minimum firing distance from all projectile based weapons except the flamer,
    /// missiles and explosives"; before, a pistol (Max Range 768) held fire beyond 768
    /// units and ran at its target without a shot].
    pub fn limited(&self) -> bool {
        self.flame || self.blast > 0.0
    }

    /// The farthest it fires, eyes to chest: its reach for a [`GunReach::limited`] gun,
    /// else its projectile's range; `None`: no limit.
    pub fn fire_limit(&self) -> Option<f32> {
        if self.limited() {
            self.reach
        } else {
            self.projectile
        }
    }

    /// Whether it holds fire at a target `distance` away (eyes to chest), and why.
    pub fn holds(&self, distance: f32) -> Option<RangeHold> {
        if self.fire_limit().is_some_and(|r| distance > r) {
            Some(RangeHold::Far)
        } else if distance < self.blast {
            Some(RangeHold::Blast)
        } else {
            None
        }
    }

    /// Where it stops closing in and beyond what it closes in again (`Awarenesses::engage`):
    /// [`GUN_RANGE_SHARE`] of its reach ([`FLAME_RANGE_SHARE`] for a flame), within
    /// [`GUN_ENGAGE_MIN`] (not for a flame) and [`GUN_ENGAGE_MAX`], clear of its blast
    /// (an area weapon, [`AREA_BLAST_SHARE`]: just clear of it), never beyond
    /// [`GUN_REACH_EDGE`] of its reach; it closes in again beyond the stop over the
    /// share, never beyond [`GUN_REACH_EDGE`] of its reach (nor nearer than the stop).
    pub fn engage(&self) -> (f32, f32) {
        let Some(r) = self.reach else { return (GUN_ENGAGE_MAX, GUN_ENGAGE_MAX / GUN_RANGE_SHARE) };
        let edge = r * GUN_REACH_EDGE;
        let (share, floor) = if self.flame { (FLAME_RANGE_SHARE, 0.0) } else { (GUN_RANGE_SHARE, GUN_ENGAGE_MIN.min(edge)) };
        let clear = self.clearance().min(edge);
        let stop = if self.blast >= r * AREA_BLAST_SHARE { clear.max(floor) } else { (r * share).max(floor).min(GUN_ENGAGE_MAX).max(clear) };
        (stop, (stop / share).min(GUN_ENGAGE_MAX / GUN_RANGE_SHARE).min(edge).max(stop))
    }
}

/// A gun's [`GunReach`] from its data: the projectile its default ammo fires (as
/// [`is_gun`] reads it) and that projectile's explosion.
pub(crate) fn gun_reach_of(c: &Combat, id: FormId, ws: &WeaponStats, range_mult: f32) -> GunReach {
    let chain = c.forms.chain(id, None);
    let projectile = chain.as_ref().and_then(|ch| ch.projectile).and_then(|p| c.forms.projectiles.get(&p));
    let blast = chain.as_ref().and_then(|ch| ch.explosion).and_then(|e| c.forms.explosions.get(&e)).map_or(0.0, |e| e.radius);
    let flame = projectile.is_some_and(|p| p.kind.as_deref() == Some("Flame"));
    GunReach { flame, ..gun_reach(ws.max_range, range_mult, projectile.map_or(0.0, |p| p.range), blast) }
}

/// An attacker's (stop, close in again beyond) distances: a gun's by its [`GunReach`],
/// else by its melee reach (`arm_reach`, a creature's fallback without one).
pub fn engage(arm_reach: Option<f32>, gun: Option<GunReach>) -> (f32, f32) {
    match (gun, arm_reach) {
        (Some(g), _) => g.engage(),
        (None, reach) => {
            let reach = reach.unwrap_or(CREATURE_REACH);
            (reach * MELEE_ENGAGE_SHARE, reach)
        }
    }
}

/// How an NPC moves as it fires, for its shot's spread: [`Stance::moving`] and
/// [`Stance::running`] (docs/design/combat-ai.md, aiming down the sights).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ShotGait {
    pub moving: bool,
    pub running: bool,
}

/// The shooter's most recent gait from its locomotion [design]: its velocity (the actor's
/// axes and own scale, units per second, `Locomotion::velocity`), its clips' pace
/// (`Locomotion::pace`) and the move its clips play (`Locomotion::blend`). Moving, in
/// any direction, while the velocity at the clips' pace is at least the speed a walk
/// clip starts at (`fnv_actor::locomotion::STAND_SPEED`, as `choose` judges it); running
/// while the move playing is a run clip, the walk/run choice its own clips make (so a
/// small actor running slowly in world units still runs, and a fast creature at its
/// walk still walks; no fixed speed cut-off). A velocity of nought is standing, even
/// while the last run clip still plays; no locomotion is standing too. The locomotion is
/// read as `fight` finds it, before `locomotion::drive` chooses this frame's clip and
/// unordered against the walker's velocity update: up to a frame old at a start, a stop
/// or a change between walk and run.
pub(crate) fn shot_gait(locomotion: Option<([f32; 2], f32, fnv_actor::locomotion::Move)>) -> ShotGait {
    use fnv_actor::locomotion::{Move as Clip, STAND_SPEED};
    let Some((v, pace, mv)) = locomotion else { return ShotGait::default() };
    let pace = if pace > 0.0 { pace } else { 1.0 };
    let moving = v[0].hypot(v[1]) / pace >= STAND_SPEED;
    let running = moving && matches!(mv, Clip::FastForward | Clip::FastBackward | Clip::FastLeft | Clip::FastRight);
    ShotGait { moving, running }
}

/// An NPC shot's stance for the actor spread: the player's formula (`fnv_combat::spread`)
/// with its sights, crouch (no standing term, §14.2), gait ([`shot_gait`]), weapon skill,
/// Strength and crippled left arm, right arm and head; never in the air, no spread perks
/// [design: NPCs keep the player's formula].
pub(crate) fn shot_stance(aiming: bool, crouched: bool, gait: ShotGait, skill: f32, strength: f32, [left_arm_crippled, right_arm_crippled, head_crippled]: [bool; 3]) -> Stance {
    Stance { aiming, moving: gait.moving, running: gait.running, sneaking: crouched, airborne: false, skill, strength, aiming_move_divisor: 1.0, strength_req_cut: 0.0, left_arm_crippled, right_arm_crippled, head_crippled }
}

/// An NPC shot's actor spread for its stance ([`shot_stance`]), spread units; none under
/// SetNoGunWobble (`fnv_game::nvse_jip14`, `wobble_off`).
pub(crate) fn shot_spread(r: &fnv_combat::tuning::Rules, t: &fnv_combat::tuning::CombatTuning, ws: &WeaponStats, st: &Stance, wobble_off: bool) -> f32 {
    if wobble_off {
        0.0
    } else {
        spread::actor_spread(r, t, ws, st)
    }
}

/// The extra spread of an actor that has fired at its target for `firing` seconds.
pub fn settling(firing: f32) -> f32 {
    SETTLE_DEGREES * (1.0 - firing / SETTLE_SECONDS).clamp(0.0, 1.0)
}

fn reach_of(c: &Combat, arm: &Arm, scale: f32) -> f32 {
    match arm {
        Arm::Gun(_) => 0.0,
        Arm::Melee(w) => w.or(c.fists_id()).and_then(|w| c.forms.weapons.get(&w)).map_or(CREATURE_REACH, |w| fnv_combat::damage::melee_reach(&c.rules, w, scale)),
        Arm::Creature(_, reach) => *reach,
    }
}

pub(crate) fn is_gun(c: &Combat, w: &WeaponStats, id: FormId) -> bool {
    AttackKind::of(w) == AttackKind::Gun && c.forms.chain(id, None).is_some_and(|ch| ch.projectile.is_some())
}

/// The navmesh, the player's place, the battle's counts, and who the AI moves.
type Where<'w> = (Option<Res<'w, NavState>>, Option<Res<'w, Location>>, ResMut<'w, Battle>, Option<Res<'w, crate::residents::Residents>>);

type Fought<'w> = (&'w ActorRig, &'w GlobalTransform, &'w InheritedVisibility, Option<&'w WeaponHold>, Has<Dying>, Has<Corpse>, Option<&'w Flinch>, Option<&'w ActorIdle>, Option<&'w Locomotion>);

/// The share of its running speed a fighter keeps (`Awarenesses::pace`, read by the AI
/// that moves it): a two-handed weapon drawn costs `fMove2HBigPenalty` of it (item 61).
/// A creature's own running speed (its run clip's pace) is the AI's
/// (`fnv_ai::Ai::speeds`, set by `creature_pace.rs`); this was where creatures were held
/// below the people's running speed (the owner's playtest list 5 item 25).
fn pace(two_handed_drawn: bool, penalty: f32) -> f32 {
    if two_handed_drawn {
        (1.0 - penalty).clamp(0.1, 1.0)
    } else {
        1.0
    }
}

/// The embedded gun (WEAP Embedded Weapon, a projectile) among `me`'s items whose NNAM
/// node is in its skeleton (`names`): the one it shoots with.
fn embedded_gun(c: &Combat, world: &fnv_sim::World, me: FormId, names: &[Option<String>]) -> Option<FormId> {
    let usable = |id: &FormId| c.forms.weapons.get(id).is_some_and(|ws| ws.embedded && is_gun(c, ws, *id) && ws.embedded_node.as_deref().is_some_and(|n| embedded_node_index(names, n).is_some()));
    let equipped = world.equipment(me).unwrap_or_default();
    let carried: Vec<FormId> = world.peek(me).and_then(|s| s.inventory.as_ref()).map(|inv| inv.iter().map(|(id, _)| id).collect()).unwrap_or_default();
    equipped.into_iter().chain(carried).find(usable)
}

/// The index of the skeleton node named `node` (an embedded weapon's NNAM), matched
/// without regard to case.
pub fn embedded_node_index(names: &[Option<String>], node: &str) -> Option<usize> {
    names.iter().position(|n| n.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(node)))
}

/// Whether a weapon use keeps `fight` off an actor (`use_weapon`): a script's use for
/// as long as it lasts (it comes first); a melee use whose last blow is still to land
/// (`landing`, `use_weapon::Shooters::landing`), which the shooter goes on with; and a
/// use still to be carried out (`active`, `Game::active_weapon_use`). Out of a combat
/// state (`in_combat`, `State::in_combat`) that is any such use, as before #2120. In one
/// it is a package's only when the package continues during combat (#2120), and then
/// only while the shooter acts on it this frame (`use_weapon::acts_on`: in place, its
/// target having a drawn or placed aim point, `target_available`): a missing target
/// position or an actor still on its way leaves ordinary attacks in charge (#2145,
/// #2652). So the shooter and the ordinary attacks do not take turns on one actor;
/// once the use is spent and
/// landed, or let go, the fight takes the actor back. `shoot` and `fight` are not
/// ordered, so in the frame a use begins or ends one of them may still act on the state
/// the other has just left [design].
fn weapon_use_holds_fight(any: Option<&fnv_game::use_weapon::WeaponUse>, active: Option<&fnv_game::use_weapon::WeaponUse>, in_combat: bool, target_available: bool, landing: bool) -> bool {
    any.is_some_and(|w| w.source == fnv_game::use_weapon::Source::Script) || landing || active.is_some_and(|w| !in_combat || crate::use_weapon::acts_on(w, target_available))
}

/// Whether an actor a weapon use holds the fight for keeps its fight's state (its
/// `Fighter`, engage distance, aim, pace and post, which `fight` drops for an actor it no
/// longer has): a package's use still to be carried out (`active`) while the actor is in
/// Combat (`fighting`, the state `fight` acts in), which is one whose package continues
/// during combat. It hands the actor back and forth as the shooter can act on it or not
/// (`use_weapon::acts_on`), so a target flickering among the drawn people does not start
/// its fight again from nothing (its sight time, magazine) each time (#2145 review)
/// [design]. A script's use, a package's last melee blow landing (no longer active), and
/// Alert and Lost drop it, as before.
fn keeps_fight(active: Option<&fnv_game::use_weapon::WeaponUse>, fighting: bool) -> bool {
    fighting && active.is_some_and(|w| matches!(w.source, fnv_game::use_weapon::Source::Package(_)))
}

/// What `fight` does this frame with an actor and its weapon use, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UseHold {
    /// No use in the way: it fights (when in Combat).
    Free,
    /// It has a use still to be carried out that the shooter cannot act on: it fights,
    /// and the use is marked taken over (`Game::weapon_use_taken`).
    Taken,
    /// A use holds the fight and its fight's state is kept ([`keeps_fight`]), nothing of
    /// its own attack left under way ([`hold_for_use`]).
    Kept,
    /// Nothing to fight this frame (a use holds it and its state goes, or it is not in
    /// Combat): its fight's state is dropped, as before.
    Out,
}

/// [`UseHold`] from [`weapon_use_holds_fight`] and [`keeps_fight`] (#2145 review).
fn use_hold(any: Option<&fnv_game::use_weapon::WeaponUse>, active: Option<&fnv_game::use_weapon::WeaponUse>, state: State, target_available: bool, landing: bool) -> UseHold {
    if weapon_use_holds_fight(any, active, state.in_combat(), target_available, landing) {
        if keeps_fight(active, state == State::Combat) {
            UseHold::Kept
        } else {
            UseHold::Out
        }
    } else if state != State::Combat {
        UseHold::Out
    } else if active.is_some() {
        UseHold::Taken
    } else {
        UseHold::Free
    }
}

/// A kept fighter while the shooter has its actor ([`UseHold::Kept`]): nothing of its
/// own attack is left under way, since `fight` does not step it meanwhile and the
/// shooter's attacks are the ones that count. Blows begun and not landed are let go, a
/// gun's shot or reload under way is settled ([`settle_for_use`]), its melee guard is
/// lowered and a reaction to an opponent's blow dropped (other fighters read `block` as
/// it guarding, and an overdue `react` would raise a guard against an old blow on its
/// first frame back; the opponent's blows begun meanwhile are marked unseen,
/// `Fighter::opp_blows_unseen`, so none of them raises one either), and its combat crouch ends (the caller takes it out of
/// `Awareness::crouching`; the stance rolls run again once the fight has the actor back).
/// Its sight time, magazine, post and the rest stay (#2145 review) [design].
fn hold_for_use(x: &mut Fighter) {
    x.blows.clear();
    x.swing = None;
    x.block = None;
    x.react = None;
    x.opp_blows_unseen = true;
    x.crouched_since = None;
    settle_for_use(x.ranged.as_mut());
}

/// A gun's cycle handed to the shooter: a pull under way (spin-up, wind-up) is let go and
/// so is a reload, back to Ready. It is settled rather than stepped with the trigger off:
/// a step would still fire a committed wind-up (`fnv_combat::attack`), and nothing plays
/// the fight's reload clip or sound while the shooter has the actor. Settled, nothing
/// reports it reloading or busy (`Fighters::reloading`, `Fighters::busy`) while the
/// shooter fires. A reload let go leaves the magazine as it was (rounds already put in
/// by a round-by-round reload stay), and the fight reloads at once on its first frame
/// back if that holds less than one shot; a gun partly loaded, or full with only its
/// close still to play, fires on without that clip [design].
fn settle_for_use(ranged: Option<&mut RangedAttack>) {
    if let Some((cycle, _)) = ranged.and_then(RangedAttack::firearm_mut) {
        if cycle.reloading() || matches!(cycle.phase, GunPhase::SpinUp { .. } | GunPhase::WindUp { .. }) {
            cycle.phase = GunPhase::Ready;
        }
    }
}

/// One frame of every fight (module notes).
#[allow(clippy::too_many_arguments)]
pub(crate) fn fight(
    time: Res<Time>,
    mut fighters: ResMut<Fighters>,
    mut combat: ResMut<Combat>,
    mut script: Option<ResMut<ScriptState>>,
    mut sim: ResMut<SimState>,
    player: Option<Res<Player>>,
    (blocks, mut phys): (Option<Res<ActorBlocks>>, Option<ResMut<PhysState>>),
    (nav, location, mut battle, residents): Where,
    actors: Query<Fought>,
    (held, body, body_melee): (Query<&HeldWeapon>, Query<&BodyWeapon>, Query<&ActorRig, With<BodyMelee>>),
    (muzzles, nodes): (Query<(&Muzzle, &GlobalTransform)>, Query<&GlobalTransform>),
    (mut fire, mut strike, mut anims): (MessageWriter<ActorFire>, MessageWriter<ActorStrike>, MessageWriter<PlayAttack>),
    (mut times, shooters): (ResMut<AttackTimes>, Option<Res<crate::use_weapon::Shooters>>),
    (cstats, control, mut grenade_probe): (Option<Res<CombatStats>>, Option<Res<PlayerControl>>, ResMut<nvf_play::combat::held_grenade_probe::Probe>),
) {
    let dt = time.delta_secs();
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    if rt.game.menu.is_some() || rt.game.dialogue.is_some() {
        return;
    }
    // Blows blocked since last frame (docs/design/combat-ai.md §14.3).
    let blocked_blows: Vec<BlockedBlow> = std::mem::take(&mut combat.blocked_blows);
    // Who is crouched to fire: their eyes are lower (§14.2).
    let crouched_now: std::collections::HashSet<u32> = rt.game.awareness.crouching.iter().map(|a| a.0).collect();
    let mut guarding: HashMap<FormId, [f32; 2]> = HashMap::new();
    let c = &*combat;
    // The weapon drawn on each actor: its model's, or a creature's body weapon in use
    // (`body_weapons.rs`).
    let weapons: HashMap<u32, FormId> = held.iter().map(|h| (h.reference, h.weapon)).chain(body.iter().map(|b| (b.reference, b.weapon))).collect();
    // Creatures with a melee body arm: they strike a target too near rather than step
    // back from it (`body_weapons.rs`).
    let strikes_near: std::collections::HashSet<u32> = body_melee.iter().map(|r| r.reference).collect();
    let muzzle_at: HashMap<u32, V3> = muzzles.iter().map(|(m, g)| (m.reference, bevy_to_game(g.translation()).to_array())).collect();
    // A creature's embedded weapon shoots from the node its WEAP names (NNAM), where its
    // skeleton has it (module notes).
    let rigs: HashMap<u32, &ActorRig> = actors.iter().map(|(rig, ..)| (rig.reference, rig)).collect();
    let embedded_at = |id: u32, ws: &WeaponStats| -> Option<V3> {
        let node = ws.embedded_node.as_deref().filter(|_| ws.embedded)?;
        let rig = rigs.get(&id)?;
        let i = embedded_node_index(&rig.skel.names, node)?;
        nodes.get(*rig.nodes.get(i)?).ok().map(|g| bevy_to_game(g.translation()).to_array())
    };
    // Where everyone is: feet and chest (game space), and size.
    let mut places: HashMap<u32, (V3, V3, f32)> = HashMap::new();
    for (rig, g, vis, _, dying, corpse, ..) in &actors {
        if vis.get() && !dying && !corpse {
            let feet = bevy_to_game(g.translation()).to_array();
            let scale = g.affine().transform_vector3(Vec3::Z).length().max(0.1);
            let eye = EYE_HEIGHT * scale * if crouched_now.contains(&rig.reference) { stance::CROUCH_EYE } else { 1.0 };
            places.insert(rig.reference, (feet, crate::detection::eyes_of(phys.as_deref(), rig.reference, feet, eye), scale));
        }
    }
    if let Some(p) = player.as_deref() {
        places.insert(FormId::PLAYER_REF.0, (p.feet, p.eye(), 1.0));
    }
    // The gap between an actor's body and its target's, when both are known.
    let bodies = CombatBodies::new(phys.as_deref(), blocks.as_deref());
    let gap = |me: u32, target: u32, feet: V3, tfeet: V3| bodies.gap_at(me, target, feet, tfeet);
    // The two bodies' radii summed, which a melee reach is counted beyond (`melee_spacing`).
    let radii = |me: u32, target: u32| bodies.radii(me, target);
    let f = &mut *fighters;
    f.poses.clear();
    f.drawn_locks = std::mem::take(&mut f.locks);
    if f.stance_rng == 0 {
        f.stance_rng = 0x2545_F491_4F6C_DD1D;
    }
    if f.power_rng == 0 {
        f.power_rng = 0xD1B5_4A32_D192_ED03;
    }
    if f.people_power_rng == 0 {
        f.people_power_rng = 0x2545_F491_4F6C_DD1D;
    }
    if f.swing_rng == 0 {
        f.swing_rng = 0x9E37_79B9_7F4A_7C15;
    }
    if f.lunge_rng == 0 {
        f.lunge_rng = 0x94D0_49BB_1331_11EB;
    }
    if f.maneuver_rng == 0 {
        f.maneuver_rng = 0xBF58_476D_1CE4_E5B9;
    }
    if f.grenade_rng == 0 {
        f.grenade_rng = 0x6A09_E667_F3BC_C909;
    }
    let gravity = phys.as_deref().map_or(686.0, |p| p.world.params.gravity);
    // Live grenades, to get away from (§17.7).
    let grenades = c.live_grenades();
    let clock = time.elapsed_secs();
    let space = location.as_deref().map(|l| l.interior.map_or(Space::World(l.world), Space::Interior));
    let nav = nav.as_deref().map(|n| &n.graph);
    let mut still: Vec<u32> = Vec::new();
    // The player's swings (its blows, for those fighting it in close combat).
    let swings = cstats.as_deref().map_or(0, |s| s.swings);
    if swings > f.player_swings {
        f.player_swing_at = clock;
    }
    f.player_swings = swings;
    let player_close = matches!(player_arm(c), Arm::Melee(_));
    let player_blocking = player_close && control.as_deref().is_some_and(|k| k.aiming);
    with_world(rt, &mut sim, |rt| {
        // Each actor's factions, read by every comrade and hostility check below, are kept
        // for the frame (`Game::begin_faction_frame`). Begun on the played world: it reads
        // the clock there for crimes that expired (#2467).
        rt.game.begin_faction_frame();
        let g = &mut rt.game;
        // Blocked blows: the attacker recoils, the blocker plays its block hit (§14.3).
        for b in &blocked_blows {
            if let Some(x) = f.fighters.get_mut(&b.attacker.0) {
                x.recoil_until = clock + stance::RECOIL_SECONDS;
                x.blows.clear();
                x.swing = None;
                f.stats.recoils += 1;
                info!("npc combat: {} recoils: its blow was blocked by {}", tactics::who(g, b.attacker), tactics::who(g, b.blocker));
                anims.write(PlayAttack { actor: b.attacker.0, weapon: weapons.get(&b.attacker.0).copied(), melee: true, sights: false, kind: Move::Recoil, power: None, swing: None });
            }
            if f.fighters.contains_key(&b.blocker.0) {
                f.stats.blocked += 1;
                anims.write(PlayAttack { actor: b.blocker.0, weapon: weapons.get(&b.blocker.0).copied(), melee: true, sights: false, kind: Move::BlockHit, power: None, swing: None });
            }
        }
        // No use is taken over until the fight says so again this frame.
        g.weapon_uses_untaken();
        for (rig, global, _, hold, _, _, flinch, idle, loco) in &actors {
            let id = rig.reference;
            let me = FormId(id);
            let Some(&(feet, eyes, scale)) = places.get(&id) else { continue };
            // ToggleActorsAI or SetActorsAI 0: it does not fight.
            if g.ai_off(me) {
                continue;
            }
            // A weapon use being carried out fires instead (`use_weapon`), while the
            // shooter acts on it (`weapon_use_holds_fight`).
            let landing = g.weapon_use(me).is_some_and(|w| shooters.as_deref().is_some_and(|s| s.landing(id, w.seq)));
            let target_available = g.weapon_use(me).is_some_and(|w| places.contains_key(&w.target.0) || crate::use_weapon::target_place(g, me, w.target).is_some());
            match use_hold(g.weapon_use(me), g.active_weapon_use(me), g.awareness.state(me), target_available, landing) {
                // The hand-over goes back and forth as the use's target comes and goes
                // among the available targets: its fight is kept as it is meanwhile.
                UseHold::Kept => {
                    if let Some(x) = f.fighters.get_mut(&id) {
                        hold_for_use(x);
                        g.awareness.crouching.remove(&me);
                        still.push(id);
                    }
                    continue;
                }
                UseHold::Out => continue,
                // A use the shooter cannot act on now: the fight has the actor, and it
                // turns to its enemy, not the use's target (`WeaponUse::taken`;
                // `fnv_ai::carry`, `residents::use_aim`).
                UseHold::Taken => g.weapon_use_taken(me),
                UseHold::Free => {}
            }
            let Some(aw) = g.awareness.get(me).filter(|a| a.state == State::Combat) else { continue };
            // The target it detects; just after losing sight of it (still in Combat, not
            // yet Lost), the one detected last, so it keeps its distance to close in
            // (`fnv_ai::alert`) instead of standing where it cannot see [design: the
            // fiends at Cook-Cook's camp stood still behind a rise and went Lost and back
            // every few seconds].
            let latest = || aw.targets.iter().filter(|(_, t)| t.detected.is_some()).max_by(|x, y| x.1.detected.unwrap_or(0.0).total_cmp(&y.1.detected.unwrap_or(0.0))).map(|(id, t)| (*id, t));
            let Some((target, _)) = aw.combat_target().or_else(latest) else { continue };
            let Some(&(tfeet, teyes, _)) = places.get(&target.0) else { continue };
            still.push(id);
            // How fast it runs (`pace`).
            // Not for a creature's body weapon: it carries nothing in its hands (§19).
            let two = crate::attack_anims::drawn(hold) && weapons.get(&id).and_then(|w| c.forms.weapons.get(w)).is_some_and(|ws| ws.two_handed() && !ws.embedded);
            let p = pace(two, c.rules.move_2h_penalty);
            if g.awareness.pace_of(me) != p {
                info!(
                    "npc combat: {} {me} runs at {:.0}% of the running speed ({})",
                    g.data.name(me),
                    p * 100.0,
                    if two { "a two-handed weapon drawn" } else { "its own pace" }
                );
            }
            if p < 1.0 {
                g.awareness.pace.insert(me, p);
            } else {
                g.awareness.pace.remove(&me);
            }
            // What it fights with; its combat style may keep it to melee.
            let style_view = g.combat_style(me);
            let style = style_view.as_deref();
            // Its Max Targeting FOV (§17.5): a new target outside it is not taken up.
            let new_target = f.fighters.get(&id).is_none_or(|x| x.target != target);
            if new_target && style.is_some_and(|s| fnv_game::combat_style::outside_targeting_fov(s, facing_off(global, feet, tfeet))) {
                if f.fov_waits.insert((id, target.0)) {
                    f.stats.fov_waits += 1;
                    info!("npc combat: {} does not take up {}: outside its targeting field of view ({:.0}°)", tactics::who(g, me), tactics::who(g, target), style.map_or(0.0, |s| s.max_targeting_fov));
                }
                continue;
            }
            let melee_only = style.is_some_and(|s| s.weapon_restrictions == 1);
            // Ranged Only: "will always use ranged weapons as their primary weapon"
            // [documented: GECK wiki Combat Style] (a creature's body arms: `body_weapons`).
            let ranged_only = style.is_some_and(|s| s.weapon_restrictions == 2);
            // A creature with nothing in its hands fights with an embedded gun whose node its
            // skeleton has (WEAP NNAM; module notes).
            let held = weapons.get(&id).copied().or_else(|| rigs.get(&id).and_then(|rig| embedded_gun(c, &g.world, me, &rig.skel.names)));
            let arm = arm_of(c, g, held, me, scale, melee_only, rigs.get(&id).map(|r| r.skeleton.as_str()));
            // Its strength, and its target's, for the threat ratios.
            let (health, full) = g.health_now_and_full(me);
            // How spent it is, for its dodges and power attacks (§17.1, §17.3).
            let fatigue = g.fatigue_share(me);
            g.awareness.tactics.strengths.insert(me, Strength { health, dps: dps(c, &arm) as f64 });
            // What its target fights with: close combat or a gun (§14).
            let tarm = if target == FormId::PLAYER_REF { player_arm(c) } else { f.fighters.get(&target.0).map(|x| x.arm.clone()).unwrap_or_else(|| arm_of(c, g, weapons.get(&target.0).copied(), target, 1.0, false, rigs.get(&target.0).map(|r| r.skeleton.as_str()))) };
            let target_close = !matches!(tarm, Arm::Gun(_));
            // People only crouch, block and bound (owner 2026-10-16: "only affect human
            // enemies not creatures").
            let person = g.world.placed.get(me).is_some_and(|p| &p.sig == b"ACHR");
            if !f.fighters.contains_key(&target.0) || target == FormId::PLAYER_REF {
                let (th, _) = g.health_now_and_full(target);
                g.awareness.tactics.strengths.insert(target, Strength { health: th, dps: dps(c, &tarm) as f64 });
            }
            battle.side_of(g, id);
            battle.side_of(g, target.0);
            battle.fought(clock);
            // The gunmen of its group on this target (flank places), it among them.
            let mates: Vec<u32> = {
                let mut v: Vec<u32> = f
                    .fighters
                    .iter()
                    .filter(|(k, x)| **k != id && x.target == target && matches!(x.arm, Arm::Gun(_)) && g.comrades(me, FormId(**k)) && !g.awareness.tactics.flights.contains_key(&FormId(**k)))
                    .map(|(k, _)| *k)
                    .collect();
                v.push(id);
                v.sort();
                v
            };
            // Its opponent: blows begun, in a blow, blocking, recoiling, and when its blow on
            // the way lands (seconds).
            let (opp_blows, opp_attacking, opp_blocking, opp_recoiling, opp_hit_in) = if target == FormId::PLAYER_REF {
                (swings, clock - f.player_swing_at < stance::PLAYER_SWING_SECONDS, player_blocking, false, stance::PLAYER_SWING_HIT)
            } else {
                f.fighters.get(&target.0).map_or((0, false, false, false, DEFAULT_HIT), |t| {
                    (t.blows_begun, !t.blows.is_empty(), t.block.is_some(), t.recoil_until > clock, t.blows.iter().map(|b| b.0).fold(f32::MAX, f32::min).min(1.0))
                })
            };
            // Gunmen of its group with it in a fight, where they are (a melee person may
            // guard one, §14.4).
            let gun_friends: Vec<(u32, V3)> = if person && matches!(arm, Arm::Melee(_)) {
                f.fighters
                    .iter()
                    .filter(|(k, x)| **k != id && matches!(x.arm, Arm::Gun(_)) && g.comrades(me, FormId(**k)) && !g.awareness.tactics.flights.contains_key(&FormId(**k)))
                    .filter_map(|(k, _)| places.get(k).map(|p| (*k, p.0)))
                    .collect()
            } else {
                Vec::new()
            };
            let fresh = f.fighters.get(&id).is_none_or(|x| x.target != target || x.arm != arm);
            if fresh {
                // The same firearm on a new target keeps its magazine; a grenade
                // has only its own controller, whose deadlines live in grenade_at.
                let kept = f.fighters.get_mut(&id).filter(|x| x.arm == arm).and_then(|x| x.ranged.take());
                let ranged = match &arm {
                    Arm::Gun(w) => c.forms.weapons.get(w).map(|ws| {
                        RangedAttack::for_weapon(ws, kept, || {
                            let rate = teammate_attack_rate(g, me, Some(*w));
                            if rate != 1.0 {
                                info!("npc combat: {} {me} fires its {} at x{rate:.2} (the teammate perks)", g.data.name(me), g.data.name(*w));
                            }
                            let timing = Timing::of(ws, &ClipTiming::default(), &c.tuning).with_attack_rate(rate);
                            // A body weapon has no magazine until its creature's reload
                            // clip for it is known (below; `body_weapon::has_magazine`, #2739).
                            Cycle::new(ws, timing, !fnv_combat::body_weapon::has_magazine(ws.embedded, None), ws.clip_size.max(1))
                        })
                    }),
                    _ => None,
                };
                let name = g.data.name(me);
                f.stats.fighters.insert(id, name.clone());
                info!("npc combat: {name} {me} fights {} with {}", if target == FormId::PLAYER_REF { "the player".to_string() } else { g.data.name(target) }, describe(c, &arm));
                // A new target keeps the fighter's tactics (its cover roll, its flight) and
                // its temperaments; a new fight draws them (§14.1).
                let (tactic, temper, approach, anchor) = f.fighters.remove(&id).map(|x| (x.tactic, Some(x.temper), Approach { charging: x.approach.charging, guarded: x.approach.guarded, ..Default::default() }, Some((x.anchor, x.anchor_health)))).unwrap_or_default();
                // Where the fight began (§17.4); a new target keeps it.
                let (anchor, anchor_health) = anchor.unwrap_or((feet, health));
                let temper = temper.unwrap_or_else(|| Temperament { crouch: next_unit(&mut f.stance_rng), cover: next_unit(&mut f.stance_rng), guard: next_unit(&mut f.stance_rng) });
                let stance_in = stance::between(stance::STANCE_DECIDE, next_unit(&mut f.stance_rng));
                f.fighters.insert(
                    id,
                    Fighter {
                        target,
                        arm: arm.clone(),
                        seen_for: 0.0,
                        firing_for: 0.0,
                        ranged,
                        strike_in: 0.0,
                        pulled: false,
                        blows: Vec::new(),
                        sights: false,
                        ready_sights: None,
                        pull_in: 0.0,
                        burst: Default::default(),
                        confidence_mod: 0.0,
                        flee_check_in: 0.0,
                        tactic,
                        stagger_seen: None,
                        idle_hold_seen: None,
                        temper,
                        crouched_since: None,
                        still_for: 0.0,
                        last_feet: None,
                        stance_in,
                        unseen_for: 0.0,
                        block: None,
                        react: None,
                        seen_opp_blows: opp_blows,
                        opp_blows_unseen: false,
                        blows_begun: 0,
                        recoil_until: 0.0,
                        approach,
                        swing: None,
                        walk_feet: None,
                        velocity: [0.0, 0.0],
                        hiding: false,
                        hide_seen: 0.0,
                        hide_moves: 0,
                        range_held: None,
                        opened_fire: false,
                        maneuver_until: 0.0,
                        dodge: None,
                        rush: None,
                        lunge_rolled: false,
                        lunge_at: 0.0,
                        target_feet: None,
                        target_velocity: [0.0, 0.0],
                        anchor,
                        anchor_health,
                        leashed: false,
                        evading: None,
                        blast_held: None,
                        urgent_posted: None,
                    },
                );
                // This fight's confidence modifier, not for the foolhardy; the first
                // flee check within the update time [documented: GECK wiki Confidence].
                let threshold = g.confidence_threshold(me);
                let (lo, hi) = g.confidence_modifier_range();
                let update = g.threat_ratio_update_time() as f32;
                let (u1, u2) = (next_unit(&mut f.rng), next_unit(&mut f.rng));
                let x = f.fighters.get_mut(&id).expect("inserted");
                x.confidence_mod = if threshold > 0.0 { lo + (hi - lo) * u1 as f64 } else { 0.0 };
                x.flee_check_in = update * u2;
            }
            // Its opponent is in the middle of a blow (the melee decision).
            let under_attack = opp_attacking;
            // Enemy gunmen firing at it, with their chests: it takes cover from them first
            // (combat_tactics, §12.6.1).
            let shooters: Vec<(u32, V3)> = f
                .fighters
                .iter()
                .filter(|(k, x)| **k != id && x.target == me && x.opened_fire && matches!(x.arm, Arm::Gun(_)))
                .filter_map(|(k, _)| places.get(k).map(|p| (*k, [p.0[0], p.0[1], p.0[2] + (p.1[2] - p.0[2]) * CHEST])))
                .collect();
            let fighter = f.fighters.get_mut(&id).expect("inserted");
            let temper = fighter.temper;
            // A dodge or a run from a grenade the AI found no way to ends (§17.1, §17.7).
            if let Some(k) = fighter.urgent_posted {
                if g.awareness.tactics.posts.get(&me).map(|p| p.kind) != Some(k) {
                    fighter.dodge = None;
                    if let Some(e) = fighter.evading.as_mut() {
                        e.1 = None;
                    }
                    fighter.urgent_posted = None;
                }
            }
            // Confidence: once hurt, now and then, would it rather flee? At its flee spot,
            // attacked or cornered, does it stand and fight? (module notes)
            let away = (feet[0] - tfeet[0]).hypot(feet[1] - tfeet[1]);
            let sees_now = g.detection.get(me, target).is_some_and(|d| d.band == Band::Detected && d.seen);
            fighter.flee_check_in -= dt;
            let gs = g.group_settings();
            let target_name = tactics::who(g, target);
            let me_name = tactics::who(g, me);
            match g.awareness.tactics.flights.get(&me).copied() {
                Some(fl) => {
                    if let Some(arrived) = fl.arrived {
                        let at_spot = *fighter.tactic.fled_health.get_or_insert(health);
                        let hurt = health < at_spot - 0.5;
                        let cornered = sees_now && away <= gs.flee_boost_radius;
                        let waited = g.time - arrived >= gs.flee_wait;
                        if hurt || cornered || waited {
                            if hurt || cornered {
                                g.boost_confidence(me);
                                battle.boosts += 1;
                            }
                            fighter.tactic.fled_health = Some(health);
                            let check = g.flee_check(me, fighter.confidence_mod);
                            let why = if hurt { "hurt at its flee spot" } else if cornered { "cornered" } else { "waited at its flee spot" };
                            if !check.flees {
                                g.end_flight(me);
                                battle.back_to_fight += 1;
                                info!("npc combat: {me_name} turns to fight {target_name} again ({why}; confidence rating now {})", check.rating);
                            } else if hurt || cornered {
                                g.start_flight(me, target, tfeet);
                                info!("npc combat: {me_name} flees on from {target_name} ({why})");
                            } else if let Some(fl) = g.awareness.tactics.flights.get_mut(&me) {
                                fl.arrived = Some(g.time);
                            }
                        }
                    }
                }
                // A coward or a child checks at once, hurt or not (`Game::always_flees`,
                // `Game::never_fights`, #2438).
                None if fighter.flee_check_in <= 0.0 || g.always_flees(me) || g.never_fights(me) => {
                    fighter.flee_check_in = g.threat_ratio_update_time() as f32 * next_unit(&mut f.rng);
                    if health < full || g.always_flees(me) || g.never_fights(me) {
                        f.stats.flee_checks += 1;
                        let check = g.flee_check(me, fighter.confidence_mod);
                        if check.flees {
                            g.start_flight(me, target, tfeet);
                            fighter.tactic.fled_health = None;
                            fighter.tactic.cover = None;
                            fighter.hide_moves = 0;
                            f.stats.flees += 1;
                            let side = battle.side_of(g, id);
                            battle.sides[side].flights += 1;
                            // A coward the ratios alone would not send off says so.
                            let by_ratio = check.personal || check.group_ratio.is_some_and(|r| fnv_game::combat_style::flees(r, check.threshold, fighter.confidence_mod));
                            if check.rating <= 0 && !by_ratio {
                                info!("npc combat: {me_name} flees from {target_name}: its confidence is Cowardly (health {health:.0} of {full:.0})");
                            } else {
                                let (kind, ratio) = if check.personal { ("personal survival", check.personal_ratio) } else { ("group", check.group_ratio) };
                                info!(
                                    "npc combat: {me_name} flees from {target_name}: {kind} threat ratio {:.3} below its confidence {:.4} (rating {}, modifier {:+.2}; health {health:.0} of {full:.0})",
                                    ratio.unwrap_or(0.0),
                                    check.threshold,
                                    check.rating,
                                    fighter.confidence_mod
                                );
                            }
                        }
                    }
                }
                None => {}
            }
            let flight = g.awareness.tactics.flights.get(&me).copied();
            // On its way to a better weapon (`fnv_ai::rearm`, combat-ai.md §22) it holds its
            // fire and takes no post, as a fleeing actor does.
            let fleeing = flight.is_some() || g.rearms.fetches.contains_key(&me);
            if let Some(fl) = flight {
                // How it fled, counted once: by its spot, or cornered.
                let key = (id, fl.since.to_bits());
                if !battle.counted_flights.contains(&key) {
                    match (fl.spot, fl.arrived) {
                        (Some((_, how)), _) => {
                            let k = match how {
                                fnv_game::combat_group::FleeBy::Door { .. } => 0,
                                fnv_game::combat_group::FleeBy::Cover => 1,
                                fnv_game::combat_group::FleeBy::Away => 2,
                            };
                            battle.flights[k] += 1;
                            battle.counted_flights.insert(key);
                        }
                        (None, Some(_)) => {
                            battle.cornered += 1;
                            battle.counted_flights.insert(key);
                        }
                        _ => {}
                    }
                }
            }
            // How near it wants to be.
            // The style's Ranged Weapon Max Range Mult scales the weapon's range, not that of
            // a weapon flagged Range - Fixed (docs/spec-notes/weapon-data-fields.md §2).
            let fixed = matches!(&arm, Arm::Gun(w) if c.forms.weapons.get(w).is_some_and(|ws| ws.has_flag("Range - Fixed")));
            let range_mult = style.filter(|_| !fixed).map_or(1.0, |s| if s.range_mult.1 > 0.0 { s.range_mult.1 } else { 1.0 });
            let gun = match &arm {
                Arm::Gun(w) => c.forms.weapons.get(w).map(|ws| gun_reach_of(c, *w, ws, range_mult)),
                _ => None,
            };
            // Its Min Range times the style's Ranged Weapon Min Range Mult, for a short-reach
            // gun's step back (`combat_tactics::back_off_band`, #2351).
            let min_mult = style.filter(|_| !fixed).map_or(1.0, |s| if s.range_mult.0 > 0.0 { s.range_mult.0 } else { 1.0 });
            let min_range = match &arm {
                Arm::Gun(w) => c.forms.weapons.get(w).map_or(0.0, |ws| ws.min_range.max(0.0) * min_mult),
                _ => 0.0,
            };
            let reach = reach_of(c, &arm, scale);
            let gap = gap(id, target.0, feet, tfeet);
            let radii = radii(id, target.0);
            let want = match (&arm, gap) {
                // Melee with bodies to measure: from where its feet are, as far as the
                // bodies' gap allows.
                // Bodies not known yet (the frame after a teleport, say): two people's.
                // (Given its strike distance to stop at, with no band to hold in, a
                // person stopped and closed in again every frame as its body's radius
                // moved with its swing: combat-rules.md §10.)
                (Arm::Melee(_), gap) => {
                    let bodies = gap.map_or(PEOPLE_RADII, |gap| away - gap);
                    melee_spacing(bodies, radii.unwrap_or(bodies), reach)
                }
                (Arm::Creature(..), gap) => creature_spacing(gap.map(|gap| away - gap), reach),
                _ => engage((!matches!(arm, Arm::Gun(_))).then_some(reach), gun),
            };
            g.awareness.engage.insert(me, want);
            // Its Combat Radius (§17.4): a hit moves where it measures from; it pursues a
            // target beyond the radius only until it has come that far itself, then holds.
            if health < fighter.anchor_health - 0.5 {
                fighter.anchor = feet;
            }
            fighter.anchor_health = health;
            let from_anchor = flat(fighter.anchor, tfeet);
            let own_from_anchor = flat(fighter.anchor, feet);
            let leashed = !fleeing
                && style.is_some_and(|s| fnv_game::combat_style::beyond_combat_radius(s, from_anchor) && own_from_anchor >= s.combat_radius);
            if leashed != fighter.leashed {
                fighter.leashed = leashed;
                if leashed {
                    f.stats.leashes += 1;
                    info!("npc combat: {me_name} stops pursuing {target_name}: {from_anchor:.0} units from where its fight began or it was last hit, {own_from_anchor:.0} come (combat radius {:.0})", style.map_or(0.0, |s| s.combat_radius));
                } else {
                    info!("npc combat: {me_name} pursues {target_name} again: {from_anchor:.0} units from where its fight began or it was last hit, {own_from_anchor:.0} come");
                }
            }
            if leashed {
                g.awareness.engage.remove(&me);
            }
            // Can it see its target? (Test aid: it does, whatever detection says.)
            let sees = sees_through_walls() || g.detection.get(me, target).is_some_and(|d| d.band == Band::Detected && d.seen);
            fighter.seen_for = if sees { fighter.seen_for + dt } else { 0.0 };
            // A wall that stopped its shot (`combat_tactics`): forgotten for a target it
            // no longer sees or no longer fights; with no step aside that clears it, it
            // closes in as on a target it does not see.
            if !sees || fighter.tactic.wall.is_some_and(|w| w.target != target.0) {
                fighter.tactic.wall = None;
            }
            if !leashed && fighter.tactic.wall.is_some_and(|w| w.step.is_none()) {
                let near = away * fnv_ai::alert::BLIND_CLOSE_SHARE;
                g.awareness.engage.insert(me, (near, near));
            }
            let tchest = [tfeet[0], tfeet[1], tfeet[2] + (teyes[2] - tfeet[2]) * CHEST];
            // Its gun's reach (§16): no shot at a target beyond it, nor within its own blast.
            let shot_distance = vec::len(vec::sub(tchest, eyes));
            let ignores_self = style.is_some_and(|s| s.ignore_damaging_self);
            let range_hold = gun.and_then(|r| r.holds(shot_distance)).filter(|h| !(ignores_self && *h == RangeHold::Blast));
            if range_hold != fighter.range_held {
                match (range_hold, gun) {
                    (Some(RangeHold::Far), Some(r)) => {
                        f.stats.held_far += 1;
                        info!("npc combat: {me_name} holds fire: {target_name} is beyond its weapon's reach ({shot_distance:.0} units, reach {:.0})", r.fire_limit().unwrap_or(0.0));
                    }
                    (Some(RangeHold::Blast), Some(r)) => {
                        f.stats.held_blast += 1;
                        info!("npc combat: {me_name} holds fire: {target_name} is within its weapon's blast ({shot_distance:.0} units, radius {:.0})", r.blast);
                    }
                    (None, Some(r)) => {
                        info!("npc combat: {me_name} has {target_name} in reach ({shot_distance:.0} units, reach {:.0})", r.fire_limit().unwrap_or(f32::INFINITY));
                    }
                    _ => {}
                }
                fighter.range_held = range_hold;
            }
            // Tactics of a gunman (`combat_tactics`): cover, flank, line of fire.
            let mut hold_fire = fleeing || range_hold.is_some();
            // Its blast would catch a comrade or a bystander (§17.6).
            let blast = gun.map_or(0.0, |r| r.blast);
            let caught = if blast > 0.0 {
                let spares = style.is_none_or(|s| g.spares_spectators(me, s));
                let ignores_group = style.is_some_and(|s| s.ignore_damaging_group);
                places
                    .iter()
                    .filter(|(k, _)| **k != id && **k != target.0)
                    .filter(|(_, p)| vec::len(vec::sub([p.0[0], p.0[1], p.0[2] + (p.1[2] - p.0[2]) * CHEST], tchest)) < blast)
                    .map(|(k, _)| FormId(*k))
                    .find(|k| !g.would_attack(me, *k) && if g.comrades(me, *k) { !ignores_group } else { spares })
            } else {
                None
            };
            if caught.map(|k| k.0) != fighter.blast_held {
                fighter.blast_held = caught.map(|k| k.0);
                if let Some(k) = caught {
                    f.stats.blast_holds += 1;
                    info!("npc combat: {me_name} holds fire: {} would be caught in its blast at {target_name} (radius {blast:.0})", tactics::who(g, k));
                }
            }
            hold_fire |= caught.is_some();
            if matches!(arm, Arm::Gun(_)) && !fleeing {
                let name = &me_name;
                let t = &mut fighter.tactic;
                let mut want_post: Option<Post> = None;
                if !g.awareness.tactics.posts.contains_key(&me) {
                    t.post_dropped(clock);
                }
                let barred = |t: &Tactic, k: PostKind| t.no_post.is_some_and(|(n, until)| n == k && clock < until);
                // Cover: rolled every 10 s by the style's Take Cover Chance.
                t.cover_roll_in -= dt;
                if t.cover_roll_in <= 0.0 {
                    t.cover_roll_in = tactics::COVER_EVERY;
                    let chance = style.map_or(0.0, |s| s.take_cover_chance);
                    t.use_cover = chance > 0.0 && next_unit(&mut f.rng) * 100.0 < chance;
                    // Failing that, a person's cover temperament gives it a second chance
                    // (§14.2), from the stance rolls' own sequence.
                    if !t.use_cover && person {
                        t.use_cover = next_unit(&mut f.stance_rng) * 100.0 < stance::COVER_BIAS * temper.cover;
                    }
                    if t.use_cover {
                        info!("npc combat: {name} will use cover for the next {:.0} s (Take Cover Chance {chance:.0})", tactics::COVER_EVERY);
                    }
                    if !t.use_cover && t.cover.take().is_some() {
                        info!("npc combat: {name} leaves its cover (no cover for the next {:.0} s)", tactics::COVER_EVERY);
                    }
                }
                // Cover taken from a gunman that no longer fires at it follows its target.
                if let Some(cv) = t.cover.as_mut() {
                    if cv.from != target.0 && !shooters.iter().any(|(k, _)| *k == cv.from) {
                        info!("npc combat: {name} keeps its cover against {target_name}: {}, whom it hid from, no longer fires at it", tactics::who(g, FormId(cv.from)));
                        cv.from = target.0;
                    }
                }
                if let Some(cv) = t.cover {
                    let from_chest = shooters.iter().find(|(k, _)| *k == cv.from).map_or(tchest, |(_, p)| *p);
                    if !cv.edge.faces(from_chest) || (cv.edge.mid[0] - tfeet[0]).hypot(cv.edge.mid[1] - tfeet[1]) > want.1 {
                        t.cover = None;
                        info!("npc combat: {name} gives up its cover: {} is no longer behind it", if cv.from == target.0 { target_name.clone() } else { tactics::who(g, FormId(cv.from)) });
                    } else if range_hold == Some(RangeHold::Far) && !matches!(cv.phase, Phase::Going) {
                        // Out of reach from its cover: it leaves it to close in (§16).
                        t.cover = None;
                        t.bad_cover.push(cv.behind);
                        f.stats.left_cover_for_reach += 1;
                        info!("npc combat: {name} leaves its cover to close in on {target_name}: beyond its weapon's reach ({shot_distance:.0} units)");
                    }
                }
                if t.use_cover && t.cover.is_none() {
                    t.cover_search_in -= dt;
                    if t.cover_search_in <= 0.0 {
                        t.cover_search_in = tactics::COVER_RETRY;
                        let taken: Vec<[f32; 3]> = g.awareness.tactics.posts.iter().filter(|(k, _)| **k != me).map(|(_, p)| p.at).chain(t.bad_cover.iter().copied()).collect();
                        if let (Some(nav), Some(space)) = (nav, space) {
                            let radius = style.map_or(0.0, |s| s.cover_search_radius);
                            let radius = if radius > 0.0 { radius } else { tactics::COVER_SEARCH_DEFAULT }.min(tactics::COVER_MAX_RUN);
                            // Standing eyes: a crouched fighter looks from where it would stand.
                            let eye = (eyes[2] - feet[2]) / if crouched_now.contains(&id) { stance::CROUCH_EYE } else { 1.0 };
                            // Its peek point, past the edge, within its reach too (§16).
                            let farthest = (want.1 - tactics::PEEK_STEP).max(want.0);
                            // From the gunmen firing at it, nearest first, then from its target.
                            let found = tactics::threats(&shooters, tactics::Threat { actor: target.0, chest: tchest }, feet)
                                .into_iter()
                                .find_map(|from| tactics::find_cover(nav, phys.as_deref_mut(), space, feet, from, tchest, eye, radius, farthest, &taken, person));
                            if let Some(cv) = found {
                                battle.cover_taken += 1;
                                if cv.from != target.0 {
                                    battle.cover_from_shooter += 1;
                                }
                                info!(
                                    "npc combat: {name} takes {}cover {:.0} units away, {:.0} units from {target_name} (a peek {:.0} units from it){}; room round its cover spot {:.0}, its peek {:.0}",
                                    if cv.low { "low " } else { "" },
                                    (cv.behind[0] - feet[0]).hypot(cv.behind[1] - feet[1]),
                                    (cv.behind[0] - tfeet[0]).hypot(cv.behind[1] - tfeet[1]),
                                    (cv.peek[0] - cv.behind[0]).hypot(cv.peek[1] - cv.behind[1]),
                                    if cv.from == target.0 { String::new() } else { format!(", hidden from {}, who fires at it", tactics::who(g, FormId(cv.from))) },
                                    cv.room[0],
                                    cv.room[1]
                                );
                                t.cover = Some(cv);
                            }
                        }
                    }
                }
                let timer = |rng: &mut u64, (lo, hi): (f32, f32)| clock + (lo + (hi - lo).max(0.0) * next_unit(rng)).max(0.5);
                if let Some(cv) = t.cover.as_mut() {
                    let near = |p: [f32; 3]| (p[0] - feet[0]).hypot(p[1] - feet[1]) < tactics::AT_POST;
                    let (wait, wait_fire, fire_t) = style.map_or(((2.0, 4.0), (1.0, 2.0), (2.0, 4.0)), |s| (s.wait_timer, s.wait_to_fire_timer, s.fire_timer));
                    match cv.phase {
                        Phase::Going => {
                            if near(cv.behind) {
                                cv.phase = Phase::Hiding { until: timer(&mut f.rng, wait) };
                                log_room(phys.as_deref_mut(), name, "reaches", feet, cv.behind);
                            }
                        }
                        Phase::Hiding { until } => {
                            hold_fire = true;
                            if clock >= until {
                                cv.phase = Phase::WaitToFire { until: timer(&mut f.rng, wait_fire) };
                                battle.peeks += 1;
                                log_room(phys.as_deref_mut(), name, "hid", feet, cv.behind);
                            }
                        }
                        Phase::WaitToFire { until } => {
                            if sees_now && near(cv.peek) {
                                cv.phase = Phase::Firing { until: timer(&mut f.rng, fire_t) };
                            } else if clock >= until {
                                cv.phase = Phase::Hiding { until: timer(&mut f.rng, wait) };
                            }
                        }
                        Phase::Firing { until } => {
                            if clock >= until {
                                cv.phase = Phase::Hiding { until: timer(&mut f.rng, wait) };
                            }
                        }
                    }
                    let (at, kind) = tactics::cover_post(cv);
                    if !barred(t, kind) {
                        want_post = Some(Post { at, kind, run: true });
                    }
                }
                // A step back from a target too near, before any cover or flank: it is on top of it.
                {
                    // A launcher keeps clear of its own blast (§16); a short-reach gun ends
                    // its step back short of where it stops closing in (#2351).
                    let (within, until) = tactics::back_off_band(gun.map_or(0.0, |r| r.clearance()), gun.map(|_| want.0), min_range);
                    let current = g.awareness.tactics.posts.get(&me).copied().filter(|p| p.kind == PostKind::BackOff);
                    let near = away < if current.is_some() { until } else { within };
                    // A creature with a melee arm strikes instead, unless its style is
                    // Ranged Only (`body_weapons`: it takes up no melee arm then).
                    let strikes = strikes_near.contains(&id) && !ranged_only;
                    if near && !barred(t, PostKind::BackOff) && !strikes {
                        // On to the step it is taking; reached, another one.
                        let at = match current.filter(|p| (p.at[0] - feet[0]).hypot(p.at[1] - feet[1]) >= tactics::AT_POST) {
                            Some(p) => p.at,
                            None => {
                                if current.is_none() {
                                    battle.back_offs += 1;
                                    info!("npc combat: {name} steps back from {target_name}, {away:.0} units away (within {within:.0})");
                                }
                                tactics::back_off(feet, tfeet, tactics::BACK_OFF_STEP)
                            }
                        };
                        want_post = Some(Post { at, kind: PostKind::BackOff, run: false });
                    }
                }
                // The line of fire: a comrade (or the player, when not its target) in the way.
                let blocker = places
                    .iter()
                    .filter(|(k, _)| **k != id && **k != target.0)
                    .map(|(k, p)| (*k, [p.0[0], p.0[1], p.0[2] + (p.1[2] - p.0[2]) * CHEST]))
                    .find(|(k, p)| tactics::in_line_of_fire(eyes, tchest, *p, tactics::LINE_OF_FIRE_WIDTH) && (*k == FormId::PLAYER_REF.0 || g.comrades(me, FormId(*k))) && !g.would_attack(me, FormId(*k)));
                match blocker {
                    Some((k, p)) => {
                        hold_fire = true;
                        if t.blocked_by != Some(k) {
                            t.blocked_by = Some(k);
                            battle.held_fire += 1;
                            info!("npc combat: {name} holds fire: {} stands in its line of fire to {target_name}", tactics::who(g, FormId(k)));
                        }
                        // Out of reach it has no shot to clear: it closes in (§16).
                        if want_post.is_none() && range_hold != Some(RangeHold::Far) && !barred(t, PostKind::Sidestep) {
                            let at = tactics::sidestep(feet, tfeet, p, tactics::SIDESTEP);
                            if g.awareness.tactics.posts.get(&me).is_none_or(|q| q.kind != PostKind::Sidestep) {
                                battle.sidesteps += 1;
                            }
                            want_post = Some(Post { at, kind: PostKind::Sidestep, run: false });
                        }
                    }
                    None => t.blocked_by = None,
                }
                // A wall stops its shot (`combat_tactics` module notes): out at a peek
                // point, that cover is no good; else it steps aside to a clear line, and
                // once there (or with no way there) it closes in.
                if let Some(mut w) = t.wall {
                    if let Some(cv) = t.cover.filter(|cv| matches!(cv.phase, Phase::WaitToFire { .. } | Phase::Firing { .. })) {
                        t.cover = None;
                        t.bad_cover.push(cv.behind);
                        if want_post.is_some_and(|p| p.kind == PostKind::Peek) {
                            want_post = None;
                        }
                        info!("npc combat: {name} gives up its cover: a wall stops its shot at {target_name} from the peek point");
                    }
                    match w.step {
                        Some(at) if flat(at, feet) < tactics::AT_POST || barred(t, PostKind::Sidestep) => w.step = None,
                        Some(at) if want_post.is_none() => want_post = Some(Post { at, kind: PostKind::Sidestep, run: false }),
                        _ => {}
                    }
                    t.wall = Some(w);
                }
                // A shift sideways it decided on (§14.2), until it is there.
                if let Some(at) = t.shift {
                    if flat(at, feet) < tactics::AT_POST || barred(t, PostKind::Shift) || t.cover.is_some() {
                        t.shift = None;
                    } else if want_post.is_none() {
                        want_post = Some(Post { at, kind: PostKind::Shift, run: true });
                    }
                }
                // Flank places for a group's gunmen not in cover.
                // Out of reach it closes in straight, not round to a flank (§16).
                if want_post.is_none() && mates.len() >= 2 && range_hold != Some(RangeHold::Far) && !barred(t, PostKind::Flank) {
                    t.flank_in -= dt;
                    let current = g.awareness.tactics.posts.get(&me).copied().filter(|p| p.kind == PostKind::Flank);
                    // Crouched to fire, it keeps the place it has (§14.2).
                    if crouched_now.contains(&id) {
                        want_post = current;
                    } else if t.flank_in <= 0.0 {
                        t.flank_in = tactics::FLANK_EVERY;
                        let index = mates.iter().position(|m| *m == id).unwrap_or(0);
                        let n = mates.iter().filter_map(|m| places.get(m)).count().max(1) as f32;
                        let centre = mates.iter().filter_map(|m| places.get(m)).fold([0.0f32; 3], |a, p| [a[0] + p.0[0] / n, a[1] + p.0[1] / n, a[2] + p.0[2] / n]);
                        let radius = away.min(want.0);
                        let at = tactics::flank_point(tfeet, centre, radius, tactics::flank_turn(index));
                        if (at[0] - feet[0]).hypot(at[1] - feet[1]) >= tactics::FLANK_MIN_MOVE {
                            if current.is_none_or(|c| (c.at[0] - at[0]).hypot(c.at[1] - at[1]) >= tactics::FLANK_MIN_MOVE) {
                                battle.flanks += 1;
                                info!("npc combat: {name} takes a flank place, {:.0}° round {target_name} from its group ({} gunmen)", tactics::flank_turn(index), mates.len());
                            }
                            want_post = Some(Post { at, kind: PostKind::Flank, run: true });
                        } else if let Some(c) = current {
                            want_post = Some(c);
                        }
                    } else {
                        want_post = current;
                    }
                }
                t.last_post = want_post.map(|p| p.kind);
                match want_post {
                    Some(p) => {
                        g.awareness.tactics.posts.insert(me, p);
                    }
                    None => {
                        g.awareness.tactics.posts.remove(&me);
                    }
                }
            } else if !fleeing && person && matches!(arm, Arm::Melee(_)) {
                // Closing in on a gunman: guard, bound or charge (§14.4).
                let ap = &mut fighter.approach;
                if !g.awareness.tactics.posts.contains_key(&me) {
                    ap.post_dropped();
                }
                let mut want_post: Option<Post> = None;
                if let Some(gd) = ap.guard.as_mut() {
                    let friend = gun_friends.iter().find(|(k, _)| *k == gd.friend).map(|x| x.1);
                    let check = stance::GuardCheck { me: feet, friend, target: tfeet, close_threat: target_close, guarded_for: clock - gd.since, patience: gd.patience };
                    match (stance::guard_release(&check), friend) {
                        (None, Some(fp)) => {
                            let at = stance::guard_point(fp, tfeet, gd.turn);
                            if flat(at, gd.at) > stance::GUARD_MOVE {
                                gd.at = at;
                            }
                            want_post = Some(Post { at: gd.at, kind: PostKind::Guard, run: true });
                        }
                        (why, _) => {
                            info!("npc combat: {me_name} leaves {} to go for {target_name}: {}", tactics::who(g, FormId(gd.friend)), why.unwrap_or("the gunman it guards is no longer fighting"));
                            ap.guard = None;
                            ap.guarded = true;
                        }
                    }
                } else if !ap.guarded && !target_close {
                    let pick = gun_friends.iter().filter(|(_, fp)| stance::may_guard(&temper, feet, *fp, tfeet)).min_by(|a, b| flat(a.1, feet).total_cmp(&flat(b.1, feet))).copied();
                    if let Some((friend, fp)) = pick {
                        let turn = (next_unit(&mut f.stance_rng) * 2.0 - 1.0) * stance::GUARD_TURN;
                        let patience = stance::between(stance::GUARD_PATIENCE, next_unit(&mut f.stance_rng));
                        let at = stance::guard_point(fp, tfeet, turn);
                        ap.guard = Some(Guarding { friend, since: clock, patience, turn, at });
                        f.stats.guards += 1;
                        info!("npc combat: {me_name} guards {} against {target_name} ({away:.0} units away; for up to {patience:.0} s)", tactics::who(g, FormId(friend)));
                        want_post = Some(Post { at, kind: PostKind::Guard, run: true });
                    }
                }
                if want_post.is_none() {
                    if target_close || away <= stance::CHARGE_WITHIN {
                        ap.bound = None;
                        ap.charging = false;
                    } else if let Some(b) = ap.bound.as_mut() {
                        if b.arrived.is_none() && flat(b.at, feet) < tactics::AT_POST {
                            b.arrived = Some(clock);
                        }
                        match b.arrived {
                            Some(t0) if clock >= t0 + b.pause => {
                                ap.bound = None;
                                ap.bound_in = 0.0;
                            }
                            _ => want_post = Some(Post { at: b.at, kind: PostKind::Advance, run: true }),
                        }
                    } else {
                        ap.bound_in -= dt;
                        if ap.bound_in <= 0.0 {
                            ap.bound_in = stance::BOUND_RETRY;
                            let chest = (eyes[2] - feet[2]) * CHEST;
                            let exposed = crate::detection::clear_sight(phys.as_deref_mut(), teyes, [feet[0], feet[1], feet[2] + chest]);
                            if exposed {
                                let taken: Vec<V3> = g.awareness.tactics.posts.iter().filter(|(k, _)| **k != me).map(|(_, p)| p.at).collect();
                                let pick = usize::from(next_unit(&mut f.stance_rng) < 0.5);
                                let found = match (nav, space) {
                                    (Some(nav), Some(space)) => tactics::find_bound(nav, phys.as_deref_mut(), space, feet, tfeet, teyes, chest, &taken, pick),
                                    _ => None,
                                };
                                match found {
                                    Some((at, gain)) => {
                                        let pause = stance::between(stance::BOUND_PAUSE, next_unit(&mut f.stance_rng));
                                        ap.bound = Some(Bound { at, arrived: None, pause });
                                        ap.charging = false;
                                        f.stats.bounds += 1;
                                        info!("npc combat: {me_name} bounds to cover {:.0} units away, {gain:.0} units nearer {target_name} ({away:.0} units away)", flat(at, feet));
                                        want_post = Some(Post { at, kind: PostKind::Advance, run: true });
                                    }
                                    None if !ap.charging => {
                                        ap.charging = true;
                                        f.stats.charges += 1;
                                        info!("npc combat: {me_name} charges {target_name} across open ground: no cover on the way ({away:.0} units away)");
                                    }
                                    None => {}
                                }
                            }
                        }
                    }
                }
                ap.posted = want_post.map(|p| p.kind);
                match want_post {
                    Some(p) => {
                        g.awareness.tactics.posts.insert(me, p);
                    }
                    None => {
                        g.awareness.tactics.posts.remove(&me);
                    }
                }
            } else if fleeing || !matches!(arm, Arm::Gun(_)) {
                g.awareness.tactics.posts.remove(&me);
            }
            // Beyond its combat radius it takes no post that pursues (§17.4).
            if leashed && g.awareness.tactics.posts.get(&me).is_some_and(|p| matches!(p.kind, PostKind::Flank | PostKind::Advance | PostKind::Guard)) {
                g.awareness.tactics.posts.remove(&me);
            }
            // A run from a live grenade (§17.7), unless its style ignores threats.
            let grenade = (!fleeing && style.is_none_or(|s| !s.ignore_threats))
                .then(|| {
                    grenades
                        .iter()
                        .filter(|gr| !(gr.thrower == me && gr.age < OWN_GRENADE_GRACE))
                        .map(|gr| (gr, vec::len(vec::sub(gr.at, feet))))
                        .filter(|(gr, d)| *d < gr.radius + EVADE_MARGIN || fighter.evading.is_some_and(|e| e.0 == gr.id))
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                })
                .flatten();
            match grenade {
                Some((gr, _)) if fighter.evading.is_some_and(|e| e.0 == gr.id) => {}
                Some((gr, d)) => {
                    let at = evade_point(feet, gr.at, gr.radius);
                    fighter.evading = Some((gr.id, Some(Post { at, kind: PostKind::Evade, run: true })));
                    f.stats.evades += 1;
                    let said = g.combat_line(me, None, "AvoidThreat").is_some();
                    info!("npc combat: {me_name} runs from a grenade {d:.0} units away (blast radius {:.0}){}", gr.radius, if said { ", saying its AvoidThreat line" } else { "" });
                }
                None => fighter.evading = None,
            }
            // A dodge ends with its timer (§17.1).
            if fighter.dodge.is_some() && (clock >= fighter.maneuver_until || fleeing || flinch.is_some_and(|fl| clock < fl.at + fl.clip.clip.length())) {
                fighter.dodge = None;
            }
            let urgent = fighter.evading.and_then(|e| e.1).or(fighter.dodge);
            match urgent {
                Some(p) => {
                    g.awareness.tactics.posts.insert(me, p);
                    if matches!(arm, Arm::Gun(_)) {
                        fighter.tactic.last_post = Some(p.kind);
                    } else {
                        fighter.approach.posted = Some(p.kind);
                    }
                    fighter.urgent_posted = Some(p.kind);
                }
                None => fighter.urgent_posted = None,
            }
            // A hit reaction (the stagger of a crippled limb, `deaths.rs`) playing: no attack
            // until it has ended, and a blow being swung is cut short (owner's playtest
            // list 4 item 21) [design].
            let staggering = flinch.filter(|fl| clock < fl.at + fl.clip.clip.length());
            if let Some(fl) = staggering {
                if fighter.stagger_seen != Some(fl.at.to_bits()) {
                    fighter.stagger_seen = Some(fl.at.to_bits());
                    let cut = std::mem::take(&mut fighter.blows).len();
                    f.stats.staggers += 1;
                    f.stats.blows_cut += cut;
                    info!("npc combat: {me_name} holds its attacks while it staggers ({:.1} s){}", fl.at + fl.clip.clip.length() - clock, if cut > 0 { format!("; {cut} blow(s) cut short") } else { String::new() });
                }
            }
            // An idle flagged "No attacking" playing (a creature's growl, hiss, dodge or
            // hop; idles.md §14): no attack until it has ended [documented: GECK wiki Idle
            // Animations Window, "No Attacking"]. An attack would end the idle
            // (`attack_anims.rs`), so it is held instead.
            let idle_holds = idle.and_then(|i| i.0.cur.as_ref()).filter(|p| g.idle_def(FormId(p.idle)).is_some_and(|d| d.no_attacking));
            if let Some(p) = idle_holds {
                if fighter.idle_hold_seen != Some(p.serial) {
                    fighter.idle_hold_seen = Some(p.serial);
                    f.stats.idle_holds += 1;
                    info!("npc combat: {me_name} holds its attacks while its idle {:08X} plays (No attacking)", p.idle);
                }
            }
            // It attacks only facing its target (owner's playtest list 5 item 33: NPCs fired
            // while looking 90° away); the AI turns it (`fnv_ai::alert`).
            let off = facing_off(global, feet, tfeet);
            // Who the AI does not turn (no `--npc-packages`) fires as it stands; blows and
            // bites at close quarters are not held back (the item is about shooting).
            let arc = match &arm {
                Arm::Gun(w) => fire_facing(c.forms.weapons.get(w).map_or(0.0, |ws| ws.aim_arc)),
                _ => FIRE_FACING,
            };
            let facing = off <= arc || !matches!(arm, Arm::Gun(_)) || residents.as_deref().is_none_or(|r| !r.runs(id));
            // Just in combat, a fighter stands a moment on its heading before it turns
            // (`fnv_ai::alert::reacting`, #1352): not locked on its target meanwhile, and
            // the time it is given to turn to it counts from the end of that moment.
            // People only: a creature does not stand (`fnv_ai::alert::combat_began`).
            let reacting = fnv_ai::alert::reacting(g, me);
            let turning_for = fnv_ai::alert::combat_began(g, me).map_or(f64::INFINITY, |t| g.time - t - fnv_ai::alert::COMBAT_REACTION_SECONDS);
            let settled = fighter.seen_for >= REACTION_SECONDS && turning_for >= f64::from(REACTION_SECONDS);
            if !facing && sees && settled {
                f.stats.unfaced += 1;
            }
            // Locked on its target (owner 2026-10-25: "if they have a target in sight they
            // should be locked on them"; "she still breaks focus to take cover"): a person
            // that sees its target faces it whatever it does, strafing on its way to cover
            // or a post and fleeing to cover as well [design]. The original's gunmen held
            // within about 1.5° of their target through every phase of the attack
            // [observed 2026-10-25, observer campaign fiend_facing]. Not while it runs from
            // its target away or by a door.
            let running_off = flight.is_some_and(|fl| !matches!(fl.spot, Some((_, fnv_game::combat_group::FleeBy::Cover))));
            // Who the AI does not run stands as it was put (and fires as it stands, above).
            if person && sees && !running_off && !reacting && residents.as_deref().is_some_and(|r| r.runs(id)) {
                f.locks.insert(id, (target.0, tfeet));
                // Measured against where it was drawn facing (last frame's lock on the same
                // target, `residents`): this frame's place of a target in contact range
                // moves several degrees in a frame (a melee Fiend 21 units from a trooper
                // read 13° off for a 5-unit step) though the body is drawn facing it exactly.
                // A new target is measured against its own place: the turn to it counts.
                let off = match f.drawn_locks.get(&id) {
                    Some((t, at)) if *t == target.0 => facing_off(global, feet, *at),
                    _ => off,
                };
                if settled {
                    f.stats.seen_offs.push(off.to_degrees());
                    if off.to_degrees() > 10.0 && f.stats.seen_offs.iter().filter(|o| **o > 10.0).count() <= 40 {
                        info!(
                            "npc combat facing: {me_name} {:.1}° off {target_name} at {away:.0} units, seen {:.2} s{}{}{}{}{}",
                            off.to_degrees(),
                            fighter.seen_for,
                            if staggering.is_some() { ", staggering" } else { "" },
                            if idle_holds.is_some() { ", in a no-attacking idle" } else { "" },
                            if flight.is_some() { ", fleeing to cover" } else { "" },
                            if urgent.is_some() { ", dodging or running from a grenade" } else { "" },
                            fighter.tactic.cover.as_ref().map_or(String::new(), |cv| format!(", cover {:?}", cv.phase)),
                        );
                    }
                }
            }
            // Crouching to fire (§14.2): a person with a gun, by the rules of
            // `combat_stance`, deciding now and then.
            // Hiding at its flee spot (§12.2.1): a person is crouched there by the rules
            // (`Game::sneaks`); seen by its enemy a while, it runs on to another spot.
            let hiding = person && g.at_flee_spot(me);
            if hiding != fighter.hiding {
                fighter.hiding = hiding;
                fighter.hide_seen = 0.0;
                if hiding {
                    f.stats.hides += 1;
                    info!("npc combat: {me_name} hides crouched from {target_name} at its flee spot ({away:.0} units away, {})", if sees_now { "in its sight" } else { "out of its sight" });
                }
            }
            if hiding {
                fighter.hide_seen = if sees_now { fighter.hide_seen + dt } else { 0.0 };
                if fighter.hide_seen >= HIDE_SEEN_SECONDS && fighter.hide_moves < HIDE_MOVES {
                    fighter.hide_moves += 1;
                    fighter.hide_seen = 0.0;
                    f.stats.hides_seen += 1;
                    g.start_flight(me, target, tfeet);
                    info!("npc combat: {me_name} flees on from {target_name} (seen where it hides, {away:.0} units away)");
                }
            }
            let mut crouched = g.awareness.crouching.contains(&me);
            let was = crouched;
            let mut stand_why: Option<&str> = None;
            if fighter.hiding {
                // Crouched while it hides, whatever it holds (`Game::sneaks`).
            } else if person && matches!(arm, Arm::Gun(_)) {
                let moved = fighter.last_feet.map_or(0.0, |l| flat(l, feet));
                fighter.last_feet = Some(feet);
                let settling = fighter.still_for < stance::CROUCH_SETTLE;
                fighter.still_for = if dt > 0.0 && moved / dt > stance::STILL_SPEED { 0.0 } else { fighter.still_for + dt };
                // Just settled somewhere (a post reached, a stop): it decides soon.
                if settling && fighter.still_for >= stance::CROUCH_SETTLE {
                    fighter.stance_in = fighter.stance_in.min(stance::between(stance::SETTLED_DECIDE, next_unit(&mut f.stance_rng)));
                }
                fighter.unseen_for = if sees { 0.0 } else { fighter.unseen_for + dt };
                let cover = fighter.tactic.cover;
                let post_pending = g.awareness.tactics.posts.get(&me).is_some_and(|p| flat(p.at, feet) > tactics::AT_POST);
                let low_hide = cover.is_some_and(|cv| cv.low && matches!(cv.phase, Phase::Hiding { .. }));
                let low_up = cover.is_some_and(|cv| cv.low && !matches!(cv.phase, Phase::Hiding { .. }));
                let s = stance::Situation { distance: away, unseen_for: fighter.unseen_for, close_threat: target_close, still_for: fighter.still_for, busy: fleeing || staggering.is_some() || post_pending };
                if low_hide && !post_pending && !fleeing {
                    if !crouched {
                        crouched = true;
                        f.stats.low_crouches += 1;
                    }
                } else if let Some(why) = stance::stand_reason(&s).or(low_up.then_some("to fire over its cover")) {
                    crouched = false;
                    stand_why = Some(why);
                } else {
                    fighter.stance_in -= dt;
                    if fighter.stance_in <= 0.0 {
                        fighter.stance_in = stance::between(stance::STANCE_DECIDE, next_unit(&mut f.stance_rng));
                        let u = next_unit(&mut f.stance_rng);
                        match fighter.crouched_since {
                            Some(since) if crouched => {
                                if u < stance::shift_chance(clock - since) {
                                    let step = stance::between(stance::SHIFT, next_unit(&mut f.stance_rng));
                                    let left = next_unit(&mut f.stance_rng) < 0.5;
                                    fighter.tactic.shift = Some(stance::shift_point(feet, tfeet, step, left));
                                    f.stats.shifts += 1;
                                    crouched = false;
                                    stand_why = Some("to shift");
                                    info!("npc combat: {me_name} shifts {step:.0} units to its {} after {:.1} s crouched", if left { "left" } else { "right" }, clock - since);
                                }
                            }
                            _ => {
                                if stance::may_crouch(&s) && u < stance::crouch_chance(&temper, away) {
                                    crouched = true;
                                }
                            }
                        }
                    }
                }
            } else {
                crouched = false;
            }
            if crouched != was {
                if crouched {
                    fighter.crouched_since = Some(clock);
                    f.stats.crouches += 1;
                    g.awareness.crouching.insert(me);
                    let low = fighter.tactic.cover.is_some_and(|cv| cv.low);
                    info!("npc combat: {me_name} crouches {} {target_name} ({away:.0} units away)", if low { "behind low cover from" } else { "to fire at" });
                } else {
                    fighter.crouched_since = None;
                    f.stats.stands += 1;
                    let why = stand_why.unwrap_or("it no longer fires");
                    if why == "a close-combat enemy is coming in" {
                        f.stats.stands_close += 1;
                    }
                    g.awareness.crouching.remove(&me);
                    info!("npc combat: {me_name} stands up ({why}; {target_name} {away:.0} units away)");
                }
            }
            // Blocking (§14.3): a melee person against a close-combat opponent; its
            // block ends; its opponent's blow begun may raise one after a reaction time.
            let can_block = person && target_close && !fleeing && staggering.is_none() && matches!(arm, Arm::Melee(_)) && fnv_actor::player_clips::blocks(melee_prefix(c, &arm));
            if !can_block {
                fighter.block = None;
                fighter.react = None;
            }
            if fighter.block.is_some_and(|(until, _)| clock >= until) {
                fighter.block = None;
            }
            // Back from the shooter (`Fighter::opp_blows_unseen`): blows begun meanwhile
            // are counted as seen, with no reaction to them.
            if std::mem::take(&mut fighter.opp_blows_unseen) {
                fighter.seen_opp_blows = fighter.seen_opp_blows.max(opp_blows);
            }
            if opp_blows > fighter.seen_opp_blows {
                fighter.seen_opp_blows = opp_blows;
                if can_block && fighter.blows.is_empty() && fighter.block.is_none() && fighter.react.is_none() && sees && off <= stance::BLOCK_REACT_FACING {
                    fighter.react = Some((clock + stance::between(stance::BLOCK_REACT, next_unit(&mut f.stance_rng)), clock + opp_hit_in));
                }
            }
            if let Some((due, lands)) = fighter.react.filter(|(due, _)| clock >= *due) {
                fighter.react = None;
                let chance = style.map_or(0.0, |s| melee_block_chance(s, true));
                if fighter.block.is_none() && next_unit(&mut f.stance_rng) * 100.0 < chance {
                    let until = (lands.max(clock) + stance::between(stance::BLOCK_AFTER, next_unit(&mut f.stance_rng))).max(clock + stance::BLOCK_MIN);
                    fighter.block = Some((until, true));
                    f.stats.blocks += 1;
                    f.stats.reactive_blocks += 1;
                    info!("npc combat: {me_name} blocks {target_name}'s blow (reacting, {:.2} s after it began; {chance:.0} %)", due - (lands - opp_hit_in));
                }
            }
            if fighter.block.is_some() {
                guarding.insert(me, heading(global));
            }
            // It holds its fire while it uses a stimpak (`npc_stimpaks.rs`), and a creature
            // flagged No Combat in Water while it swims ("will not perform attacks in
            // water" [documented: GECK wiki Animation Tab]).
            let in_water_hold = g.no_combat_in_water(me) && g.motion(me).swimming;
            let ready = sees
                && !in_water_hold
                && facing
                && !hold_fire
                && staggering.is_none()
                && !g.using_stimpak(me)
                && idle_holds.is_none()
                && fighter.block.is_none()
                && clock >= fighter.recoil_until
                && fighter.seen_for >= REACTION_SECONDS
                && hold.is_none_or(|h| h.anim.stage == Stage::Drawn);
            let pitch = (tchest[2] - eyes[2]).atan2((tchest[0] - eyes[0]).hypot(tchest[1] - eyes[1]));
            // Down the sights while it fires at a far target, from the hip near.
            let sight_fov = match &arm {
                Arm::Gun(w) => c.forms.weapons.get(w).map_or(0.0, |ws| ws.sight_fov),
                _ => 0.0,
            };
            let sights = ready && aims_down_sights(fighter.sights, away, sight_fov);
            if sights && !fighter.sights {
                f.stats.sights_raised += 1;
            }
            // The first time a gunman is ready in a fight is logged too (a new fight after
            // a search may begin from the hip); melee fighters and creatures have no aim.
            if Some(sights) != fighter.ready_sights && ready && matches!(arm, Arm::Gun(_)) {
                fighter.ready_sights = Some(sights);
                let name = g.data.name(me);
                if sights {
                    info!("npc combat: {name} {me} aims down the sights, its target {away:.0} units away (beyond {HIP_FIRE_WITHIN:.0}): walks while aiming");
                } else if sight_fov <= 0.0 {
                    info!("npc combat: {name} {me} fires from the hip, its target {away:.0} units away (its weapon has no sights)");
                } else {
                    info!("npc combat: {name} {me} fires from the hip, its target {away:.0} units away (within {HIP_FIRE_WITHIN:.0})");
                }
            }
            fighter.sights = sights;
            if sights {
                g.awareness.aiming.insert(me);
            } else {
                g.awareness.aiming.remove(&me);
            }
            f.poses.insert(id, Pose { pitch, weapon: weapons.get(&id).copied(), sights, crouched, blocking: fighter.block.is_some(), distance: away, stop: None, contact: None });
            if grenade_probe.actor() == Some(me) {
                grenade_probe.observe(g, &c.forms, nvf_play::combat::held_grenade_probe::Frame {
                    clock, dt, actor: me, target, selected: g.held_weapon(me).map(|(w, _)| w), held: weapons.get(&id).copied(),
                    collision_ready: phys.as_deref().is_some_and(|p| {
                        let here = match (space, p.space) {
                            (Some(Space::World(w)), nvf_play::phys::cells::Space::Exterior) => w == p.worldspace,
                            (Some(Space::Interior(c)), nvf_play::phys::cells::Space::Interior(i)) => c == i,
                            _ => false,
                        };
                        here && p.player.is_some() && p.ready_at(feet) && p.ready_at(tfeet)
                    }),
                    drawn: hold.is_some_and(|h| h.anim.stage == Stage::Drawn),
                    ready, person, fleeing, eyes, feet, target_feet: tfeet, gravity,
                    due: f.grenade_at.get(&id).filter(|(t, _)| *t == target).map(|(_, at)| *at),
                });
            }
            match &arm {
                Arm::Gun(_) if matches!(fighter.ranged, Some(RangedAttack::Grenade)) => {
                    use crate::npc_grenades::{self, Decision, ThrowInput};
                    // This branch owns the whole update: a denied or waiting throw
                    // cannot fall through to a firearm cycle or its pending shots.
                    let previous = f.grenade_at.get(&id).copied();
                    let mut deadline = previous;
                    let input = ThrowInput { target, clock, ready: ready && !fleeing && person, origin: eyes, target_at: tfeet, gravity };
                    let decision = npc_grenades::plan(
                        &mut deadline,
                        input,
                        g,
                        // Not a live unit planted on it (`Extra::live`, #1640).
                        |g| g.world.peek(me).and_then(|s| s.inventory.as_ref()).and_then(|inv| npc_grenades::carried(&c.forms, inv.free_iter())),
                        |g, gr| {
                            let spares = style.is_none_or(|s| g.spares_spectators(me, s));
                            let ignores_group = style.is_some_and(|s| s.ignore_damaging_group);
                            places
                                .iter()
                                .filter(|(k, _)| **k != id && **k != target.0)
                                .filter(|(_, p)| vec::len(vec::sub(p.0, tfeet)) < gr.blast)
                                .map(|(k, _)| FormId(*k))
                                .find(|k| !g.would_attack(me, *k) && if g.comrades(me, *k) { !ignores_group } else { spares })
                        },
                        || next_unit(&mut f.grenade_rng),
                    );
                    if let Some(deadline) = deadline.filter(|next| Some(*next) != previous) {
                        f.grenade_at.insert(id, deadline);
                    }
                    let away_feet = vec::len(vec::sub(tfeet, eyes));
                    match decision {
                        Decision::Waiting => {}
                        Decision::Empty => debug!("npc combat: {me_name} has no grenade to throw"),
                        Decision::OutOfBand(gr) => {
                            debug!("npc combat: {me_name} keeps its {}: {target_name} is {away_feet:.0} units away (throws between {:.0} and {:.0})", g.data.name(gr.weapon), npc_grenades::MIN_THROW, npc_grenades::reach(&gr, gravity));
                        }
                        Decision::Held { grenade: gr, by } => {
                            f.stats.grenade_holds += 1;
                            info!("npc combat: {me_name} keeps its grenade: {} would be caught in its blast at {target_name} (radius {:.0})", tactics::who(g, by), gr.blast);
                        }
                        Decision::Throw(throw) => {
                            if let Some(throw) = npc_grenades::commit(throw, |weapon| g.world.remove_item(me, weapon, 1)) {
                                let gr = throw.grenade;
                                fire.write(ActorFire { shooter: me, weapon: gr.weapon, origin: throw.origin, aim: throw.aim, cone: GRENADE_CONE, muzzle: None, harmless: false, sure: None });
                                anims.write(PlayAttack { actor: id, weapon: Some(gr.weapon), melee: false, sights: false, kind: Move::Attack, power: None, swing: None });
                                let left = g.world.peek(me).and_then(|s| s.inventory.as_ref()).map_or(0, |i| i.free_count(gr.weapon));
                                f.stats.grenades += 1;
                                info!("npc combat: {me_name} throws {} at {target_name} ({away_feet:.0} units, blast {:.0}); {left} left", g.data.name(gr.weapon), gr.blast);
                                g.attack_line(me, Some(target), false, true);
                            } else {
                                debug!("npc combat: {me_name} keeps its grenade: its selected item could not be spent");
                            }
                        }
                    }
                    // A burst under way ends with the switch to a grenade: the gun's
                    // next pull starts a fresh one, its pause still running.
                    fighter.burst.let_go();
                    fighter.burst.tick(dt);
                }
                Arm::Gun(w) => {
                    let Some(ws) = c.forms.weapons.get(w) else { continue };
                    let Some((cycle, reload_timed)) = fighter.ranged.as_mut().and_then(RangedAttack::firearm_mut) else { continue };
                    // Its reload takes as long as its reload clips in its rig, once known
                    // (between reloads, so one under way runs to its end); a semi-automatic
                    // gun's shots follow its attack clip there as the player's do (#2739:
                    // `fnv_combat::attack::npc_clip_timing`).
                    if !*reload_timed && matches!(cycle.phase, GunPhase::Ready | GunPhase::Cooldown { .. }) {
                        if let Some((timed, timing)) = times.gun_timing(id, *w, ws, &c.tuning, || teammate_attack_rate(g, me, Some(*w))) {
                            cycle.timing = timing;
                            *reload_timed = true;
                            // A body gun gets its magazine with its reload clip: it reloads
                            // as the people's guns do; without the clip it fires on without
                            // one (`body_weapon::has_magazine`, #2739).
                            let magazine = fnv_combat::body_weapon::has_magazine(ws.embedded, timed.reload_len);
                            if ws.embedded {
                                let pace = match timed.attack_len {
                                    Some(len) => format!(
                                        "fires every {:.2} s (its attack clip {len:.2} s{})",
                                        cycle.timing.interval,
                                        timed.next_attack.map_or(String::new(), |(t, _)| format!(", the next shot from its a: key at {t:.2} s"))
                                    ),
                                    None if ws.automatic => format!("fires every {:.2} s (its Fire Rate)", cycle.timing.interval),
                                    None => format!("fires every {:.2} s (1 / Attack Shots/Sec: no attack clip in its rig)", cycle.timing.interval),
                                };
                                let mag = match timed.reload_len.filter(|_| magazine) {
                                    Some(len) => format!("has a magazine of {} and reloads in {len:.2} s", cycle.clip_size),
                                    None => "has no reload clip in its rig: no magazine".to_string(),
                                };
                                info!("npc combat: {me_name}'s body gun {} {pace}; {mag}", ws.edid.as_deref().unwrap_or("?"));
                            }
                            cycle.no_ammo = !magazine;
                        }
                    }
                    let reloading = cycle.reloading();
                    fighter.firing_for = if ready && !reloading { fighter.firing_for + dt } else { 0.0 };
                    // Automatic weapons hold the trigger for a burst, then let it go for a
                    // pause (`fnv_combat::burst`); others pull it again each time the cycle
                    // is ready and the style's firing delay has passed. Not while it
                    // reloads: a pull would stop a round-by-round reload after a round.
                    fighter.pull_in = (fighter.pull_in - dt).max(0.0);
                    fighter.burst.tick(dt);
                    let pull = if ws.automatic { fighter.burst.may_fire() } else { !fighter.pulled && fighter.pull_in <= 0.0 };
                    let mut trigger = ready && !reloading && pull;
                    // A wall in the way holds the shot (`combat_tactics` module notes),
                    // looked at as the trigger is pulled, not every frame; the shot goes
                    // at the point the line is clear to.
                    let mut aim_at = tchest;
                    if trigger {
                        let t = &mut fighter.tactic;
                        let line = t.shot_line.filter(|(k, until, _)| *k == target.0 && clock < *until).map(|l| l.2);
                        if let Some(p) = line {
                            aim_at = p;
                        } else if t.wall.is_some_and(|w| clock < w.recheck_at) {
                            trigger = false;
                        } else {
                            match tactics::shot_clear(phys.as_deref_mut(), eyes, muzzle_at.get(&id).copied().or_else(|| embedded_at(id, ws)), tchest, teyes) {
                                Some(p) => {
                                    aim_at = p;
                                    t.shot_line = Some((target.0, clock + tactics::LINE_CLEAR_FOR, p));
                                    if let Some(w) = t.wall.take() {
                                        battle.wall_clears += 1;
                                        info!("npc combat: {me_name} has a clear shot at {target_name} again after {:.1} s (at its {})", clock - w.since, if p == tchest { "chest" } else { "head" });
                                    }
                                }
                                None => {
                                    trigger = false;
                                    t.shot_line = None;
                                    match t.wall {
                                        Some(w) => t.wall = Some(tactics::Wall { recheck_at: clock + tactics::WALL_RECHECK, ..w }),
                                        None => {
                                            // Standing eyes: a crouched fighter steps aside standing.
                                            let eye = (eyes[2] - feet[2]) / if crouched { stance::CROUCH_EYE } else { 1.0 };
                                            let step = tactics::wall_step(phys.as_deref_mut(), feet, eye, tfeet, tchest, id % 2 == 0);
                                            battle.wall_holds += 1;
                                            if step.is_some() {
                                                battle.wall_steps += 1;
                                            }
                                            info!(
                                                "npc combat: {me_name} holds fire: a wall stands between it and {target_name} ({shot_distance:.0} units); {}",
                                                match step {
                                                    Some(at) => format!("steps aside {:.0} units to a clear line", flat(at, feet)),
                                                    None => "no clear line a step aside: closes in".to_string(),
                                                }
                                            );
                                            t.wall = Some(tactics::Wall { target: target.0, since: clock, recheck_at: clock + tactics::WALL_RECHECK, step });
                                        }
                                    }
                                }
                            }
                        }
                        if !trigger {
                            fighter.firing_for = 0.0;
                        }
                    }
                    fighter.pulled = trigger;
                    if !trigger {
                        // A burst cut short: the next pull starts a fresh one.
                        fighter.burst.let_go();
                    }
                    // An empty magazine is reloaded at once, whether it can fire or not;
                    // it carries what it loads (module notes).
                    let empty = cycle.magazine < cycle.ammo_use && cycle.regen.is_none();
                    let rounds = cycle.timing.rounds;
                    let events = cycle.step(&c.tuning, Input { trigger, reload: empty && !reloading, dt, carried: u32::MAX / 2 });
                    let (magazine, reload_secs) = ((cycle.magazine, cycle.clip_size), cycle.timing.reload);
                    for e in events {
                        match e {
                            Event::Fired { .. } => {
                                let mult = style.map_or((1.0, 1.0), |s| s.semi_auto_delay_mult);
                                if !ws.automatic {
                                    let (lo, hi) = (ws.semi_auto_fire_delay[0] * mult.0, ws.semi_auto_fire_delay[1] * mult.1);
                                    fighter.pull_in = (lo + (hi - lo).max(0.0) * next_unit(&mut f.rng)).max(0.0);
                                } else if let Some(end) = fighter.burst.fired(ws.fire_rate, mult, next_unit(&mut f.rng)) {
                                    info!("npc combat: {me_name} fires a burst of {} rounds at {target_name}, then lets go for {:.2} s", end.rounds, end.pause);
                                }
                                // Aimed from the eyes, drawn from the muzzle when the weapon
                                // has one (module notes).
                                let embedded = if muzzle_at.contains_key(&id) { None } else { embedded_at(id, ws) };
                                let muzzle = muzzle_at.get(&id).copied().or(embedded);
                                let to = vec::sub(aim_at, eyes);
                                let len = vec::len(to).max(1.0);
                                let aim = vec::scale(to, 1.0 / len);
                                let origin = vec::add(eyes, vec::scale(aim, MUZZLE_AHEAD));
                                if !fighter.opened_fire {
                                    fighter.opened_fire = true;
                                    let node = embedded.and(ws.embedded_node.as_deref()).map_or(String::new(), |n| format!(", shooting from its embedded weapon's node {n}"));
                                    info!("npc combat: {me_name} opens fire on {target_name} from {shot_distance:.0} units (reach {:.0}){node}", gun.and_then(|r| r.reach).unwrap_or(f32::INFINITY));
                                }
                                let first = f.stats.from_muzzle + f.stats.from_eyes == 0 || fighter.firing_for <= dt;
                                match muzzle {
                                    Some(_) => f.stats.from_muzzle += 1,
                                    None => f.stats.from_eyes += 1,
                                }
                                if first {
                                    match muzzle {
                                        Some(m) => {
                                            let d = vec::sub(m, eyes);
                                            info!("npc combat: {me} shots are drawn from its weapon's muzzle ({:.0} units from its eyes, {:.0} below them)", vec::len(d), -d[2]);
                                        }
                                        None => info!("npc combat: {me} shots are drawn from its eyes (its weapon has no muzzle node)"),
                                    }
                                }
                                let skill = ws.skill.as_deref().and_then(fnv_game::functions::av_by_name).map_or(50.0, |code| fnv_game::functions::actor_value_capped(g, me, code)) as f32;
                                let strength = fnv_game::functions::actor_value_capped(g, me, fnv_game::actor_values::STRENGTH) as f32;
                                if fighter.sights {
                                    f.stats.sights_shots += 1;
                                } else {
                                    f.stats.hip_shots += 1;
                                }
                                // Crouched: no standing term in the spread (§14.2). A crippled
                                // arm (Left/Right Attack Condition) or head widens it as the
                                // player's [design: the same settings, actors.md §8.12,
                                // crippled-limbs.md §3].
                                use fnv_game::combat::{HEAD, LEFT_ARM, RIGHT_ARM};
                                let (left_arm_crippled, right_arm_crippled, head_crippled) = (g.is_crippled(me, LEFT_ARM), g.is_crippled(me, RIGHT_ARM), g.is_crippled(me, HEAD));
                                // Walking or running widens it as the player's (#2125): its most
                                // recent gait, up to a frame old at a start, stop or walk/run change.
                                let gait = shot_gait(loco.map(|l| (l.velocity, l.pace, l.blend.cur.mv)));
                                let st = shot_stance(fighter.sights, crouched, gait, skill, strength, [left_arm_crippled, right_arm_crippled, head_crippled]);
                                let actor_spread = shot_spread(&c.rules, &c.tuning, ws, &st, g.gun_wobble_off(me));
                                g.other.jg9.spreads.insert(me, actor_spread);
                                let cone = spread::cone_deg(actor_spread, 0.0, ws, &[]) + settling(fighter.firing_for);
                                fire.write(ActorFire { shooter: me, weapon: *w, origin, aim, cone, muzzle, harmless: false, sure: None });
                                anims.write(PlayAttack { actor: id, weapon: Some(*w), melee: false, sights: fighter.sights, kind: Move::Attack, power: None, swing: None });
                                f.stats.shots += 1;
                                f.stats.shot_offs.push(off.to_degrees());
                                if let Some(r) = gun.and_then(|r| r.reach).filter(|r| *r > 0.0) {
                                    let share = shot_distance / r;
                                    f.stats.farthest_share = f.stats.farthest_share.max(share);
                                    if share > 1.0 {
                                        f.stats.shots_beyond += 1;
                                    }
                                }
                                // Its Attack line, on its own timer (fnv_game::combat_lines).
                                g.attack_line(me, Some(target), false, false);
                            }
                            Event::ReloadStarted => {
                                f.stats.reloads += 1;
                                if rounds.is_some() {
                                    f.stats.round_reloads += 1;
                                }
                                anims.write(PlayAttack { actor: id, weapon: Some(*w), melee: false, sights: false, kind: Move::Reload, power: None, swing: None });
                                let how = match rounds {
                                    Some(r) => format!("round by round (opens {:.2} s, {:.2} s a round, closes {:.2} s)", r.open, r.round, r.close),
                                    None => format!("in {:.2} s{}", reload_secs, if *reload_timed { "" } else { ", the fallback time" }),
                                };
                                info!("npc combat: {} {me} reloads {} {how}", g.data.name(me), ws.edid.as_deref().unwrap_or("?"));
                            }
                            // The opening clip played: each round the reload clip from its start.
                            Event::ReloadRound => {
                                anims.write(PlayAttack { actor: id, weapon: Some(*w), melee: false, sights: false, kind: Move::ReloadRound, power: None, swing: None });
                            }
                            Event::Reloaded { taken } => info!("npc combat: {} {me} reloaded {taken}, magazine {}/{}", g.data.name(me), magazine.0, magazine.1),
                            _ => {}
                        }
                    }
                }
                Arm::Melee(_) | Arm::Creature(..) => {
                    // How it moves, for a power attack by movement (combat-rules.md §10.1).
                    if let (Some(l), true) = (fighter.walk_feet, dt > 0.0) {
                        fighter.velocity = [(feet[0] - l[0]) / dt, (feet[1] - l[1]) / dt];
                    }
                    fighter.walk_feet = Some(feet);
                    // How its target moves: this frame's step evened with the last
                    // (a frame without a physics step reads as standing still).
                    fighter.target_velocity = match fighter.target_feet {
                        Some((t, l)) if t == target && dt > 0.0 => {
                            let now = [(tfeet[0] - l[0]) / dt, (tfeet[1] - l[1]) / dt];
                            if now[0].hypot(now[1]) > TARGET_SPEED_MAX {
                                [0.0, 0.0]
                            } else {
                                [(now[0] + fighter.target_velocity[0]) * 0.5, (now[1] + fighter.target_velocity[1]) * 0.5]
                            }
                        }
                        Some((t, _)) if t == target => fighter.target_velocity,
                        _ => [0.0, 0.0],
                    };
                    fighter.target_feet = Some((target, tfeet));
                    // Where its walk stops short of the target: a lunge carries it no nearer.
                    let stop = match (&arm, gap) {
                        (Arm::Creature(..), gap) => creature_spacing(gap.map(|gap| away - gap), reach).0,
                        (_, Some(gap)) => melee_spacing(away - gap, radii.unwrap_or(away - gap), reach).0,
                        (_, None) => reach * MELEE_SPACING_SHARE,
                    };
                    if let Some(p) = f.poses.get_mut(&id) {
                        p.stop = Some(stop);
                        p.contact = gap.map(|gap| away - gap);
                    }
                    f.stats.melee_closest = Some(f.stats.melee_closest.map_or(away, |c| c.min(away)));
                    fighter.strike_in = (fighter.strike_in - dt).max(0.0);
                    let near = in_melee_reach(&arm, away, gap, radii, reach);
                    // The last blow's clip, once started: its blow lands at its own `Hit`
                    // key, and the next decision comes at its `a:` key (module notes).
                    if let Some(sw) = fighter.swing.as_mut() {
                        sw.t += dt;
                        if let Some(clip) = times.swings.remove(&id).filter(|_| sw.clip.is_none() && sw.takes_clip()) {
                            sw.clip = Some(clip);
                            // (Before this frame's countdown below.)
                            if let Some(b) = fighter.blows.last_mut() {
                                b.0 = (clip.hit - sw.t + dt).max(0.0);
                            }
                            fighter.strike_in = sw.ready_in(fighter.strike_in);
                        }
                        if sw.t > sw.ends_at() {
                            fighter.swing = None;
                        }
                    }
                    // Blows whose clip has reached its `Hit` key land, if the target is
                    // still within reach.
                    for b in &mut fighter.blows {
                        b.0 -= dt;
                    }
                    let (due, wait): (Vec<_>, Vec<_>) = std::mem::take(&mut fighter.blows).into_iter().partition(|b| b.0 <= 0.0);
                    fighter.blows = wait;
                    for (_, b) in due {
                        if near {
                            strike.write(ActorStrike { point: tchest, ..b });
                        } else {
                            f.stats.missed += 1;
                        }
                    }
                    // The melee decision: block (against close combat, §14.3), attack, or
                    // hold for the style's hold timer [documented: the order].
                    // A rushing attack (§17.2): rolled once as its target comes within the
                    // rush distance; beyond it, rolled again next time.
                    let bodies = gap.map_or(0.0, |gp| away - gp);
                    if let Some((rd, chance)) = style.and_then(|s| fnv_game::combat_style::rush_distance(s, reach).map(|d| (d, s.rushing_attack_chance))) {
                        if away > rd + bodies {
                            fighter.rush = None;
                        } else if fighter.rush.is_none() && !near && !fleeing {
                            let rushes = next_unit(&mut f.maneuver_rng) * 100.0 < chance;
                            fighter.rush = Some(rushes);
                            if rushes {
                                f.stats.rushes += 1;
                                info!("npc combat: {me_name} rushes {target_name} from {away:.0} units ({chance:.0} %)");
                            }
                        }
                    }
                    let rushing = fighter.rush == Some(true);
                    // A creature closing in fast may begin its blow on the run, short of its
                    // reach, by the time its clip takes to reach the `Hit` key: a forward
                    // power attack (a lunge) where it has the clip, else an ordinary blow
                    // (`lunge_band`). Rolled once a band, by its style's chances.
                    let lame = g.is_crippled(me, fnv_game::combat::LEFT_MOBILITY) || g.is_crippled(me, fnv_game::combat::RIGHT_MOBILITY);
                    // (the lunges it may make, whether only a lunge goes)
                    let mut early: Option<(Vec<(Power, f32)>, bool)> = None;
                    if let Arm::Creature(..) = &arm {
                        let strike_at = creature_spacing(gap.map(|gap| away - gap), reach).1;
                        let to = [tfeet[0] - feet[0], tfeet[1] - feet[1]];
                        let run = residents.as_deref().map_or(f32::INFINITY, |r| r.alert_speeds(me).run * g.awareness.pace_of(me));
                        let closing = closing_speed(fighter.velocity, fighter.target_velocity, to, run * 1.1);
                        let hit_normal = times.hit.get(&id).copied().unwrap_or(DEFAULT_HIT);
                        let forward = times.powers.get(&id).and_then(|have| have.iter().copied().find(|p| p.0 == Power::Forward)).filter(|_| !lame);
                        let band_normal = lunge_band(closing, hit_normal);
                        let band_forward = forward.map_or(0.0, |p| lunge_band(closing, p.1));
                        let band = band_normal.max(band_forward);
                        // Rolled again only once the target is beyond the widest band (a
                        // frame without a physics step reads as standing still), or it
                        // has been short of its target for a while since (a target that
                        // keeps backing away within the band).
                        if !near && (away > strike_at + RUN_ATTACK_MAX || clock - fighter.lunge_at >= RUN_ATTACK_AGAIN) {
                            fighter.lunge_rolled = false;
                        }
                        let in_band = !near && band > 0.0 && away <= strike_at + band;
                        if in_band && !fighter.lunge_rolled && ready && !fleeing && fighter.strike_in <= 0.0 && (feet[2] - tfeet[2]).abs() < 128.0 {
                            fighter.lunge_rolled = true;
                            fighter.lunge_at = clock;
                            let lunge = forward.filter(|_| away <= strike_at + band_forward).into_iter().collect::<Vec<_>>();
                            let ordinary_ok = away <= strike_at + band_normal;
                            // No attack-chance roll on the run: a creature running in
                            // means to strike (owner: creatures were not aggressive
                            // enough); only a lunge rolls, by the power attack chance.
                            let wants = true;
                            // Beyond the ordinary blow's band only a lunge goes: its power
                            // attack chance is rolled now.
                            let lunge_only = !ordinary_ok;
                            let power_ok = !lunge_only || {
                                let chance = style.filter(|s| s.power_attack_chance > 0.0).map_or(FALLBACK_POWER_CHANCE, |s| creature_power_chance(s, fatigue));
                                next_unit(&mut f.lunge_rng) * 100.0 < chance
                            };
                            if wants && power_ok && (ordinary_ok || !lunge.is_empty()) {
                                f.stats.run_attacks += 1;
                                info!(
                                    "npc combat: {me_name} attacks {target_name} on the run from {away:.0} units ({}closing at {closing:.0} units/s; strikes within {strike_at:.0})",
                                    if lunge_only { "a lunge, " } else { "" }
                                );
                                early = Some((lunge, lunge_only));
                            }
                        }
                    }
                    let decide = ready && (near || early.is_some()) && (feet[2] - tfeet[2]).abs() < 128.0 && fighter.strike_in <= 0.0;
                    let mut blocks = false;
                    if decide && can_block && !rushing {
                        let chance = style.map_or(0.0, |s| melee_block_chance(s, under_attack));
                        if next_unit(&mut f.stance_rng) * 100.0 < chance {
                            let held = stance::between(style.map_or((0.5, 1.5), |s| s.hold_timer), next_unit(&mut f.stance_rng)).max(stance::BLOCK_MIN);
                            fighter.block = Some((clock + held, false));
                            fighter.strike_in = held;
                            fighter.swing = None;
                            blocks = true;
                            f.stats.blocks += 1;
                            info!("npc combat: {me_name} blocks {target_name}'s blow (decided, {held:.1} s; {chance:.0} %)");
                            guarding.insert(me, heading(global));
                        }
                    }
                    let opponent = Opponent { attacking: under_attack, recoiling: opp_recoiling, blocking: opp_blocking, unconscious: g.is_out_cold(target) };
                    let attack = decide
                        && !blocks
                        && (early.is_some() || rushing || style.is_none_or(|s| {
                            let (skill_name, h2h) = match &arm {
                                Arm::Melee(Some(w)) => (c.forms.weapons.get(w).and_then(|ws| ws.skill.clone()).unwrap_or_else(|| "Melee Weapons".into()), false),
                                Arm::Melee(None) => ("Unarmed".to_string(), true),
                                _ => ("Melee Weapons".to_string(), true),
                            };
                            let skill = fnv_game::functions::av_by_name(&skill_name).map_or(50.0, |code| fnv_game::functions::actor_value_capped(g, me, code)) as f32;
                            next_unit(&mut f.rng) * 100.0 < melee_attack_chance(s, skill, h2h, opponent)
                        }));
                    if decide && rushing {
                        fighter.rush = Some(false);
                    }
                    if decide && !blocks && !attack {
                        let (lo, hi) = style.map_or((0.5, 1.5), |s| s.hold_timer);
                        fighter.strike_in = (lo + (hi - lo).max(0.0) * next_unit(&mut f.rng)).max(0.1);
                        f.stats.holds += 1;
                        fighter.swing = None;
                        // The maneuver decision (§17.1): dodge or idle, unless the last one
                        // still runs.
                        if let (Some(s), true) = (style, clock >= fighter.maneuver_until && !fleeing) {
                            let walk = residents.as_deref().map_or(DEFAULT_WALK, |r| r.alert_speeds(me).walk) * g.awareness.pace_of(me);
                            let rolls = [(); 5].map(|_| next_unit(&mut f.maneuver_rng));
                            let chance = fnv_game::combat_style::dodge_chance(s, fatigue, walk, under_attack);
                            match fnv_game::combat_style::maneuver(s, fatigue, walk, under_attack, !fighter.blows.is_empty(), rolls) {
                                fnv_game::combat_style::Maneuver::Dodge(dir, secs) => match dodge_point(feet, tfeet, dir, walk * secs, want.0) {
                                    Some(at) => {
                                        fighter.maneuver_until = clock + secs;
                                        fighter.dodge = Some(Post { at, kind: PostKind::Dodge, run: false });
                                        f.stats.dodges[dir as usize] += 1;
                                        info!("npc combat: {me_name} dodges {} from {target_name} ({secs:.2} s, {:.0} units; {chance:.0} %)", dir.name(), flat(at, feet));
                                    }
                                    None => {
                                        fighter.maneuver_until = clock + stance::between(s.idle_timer, rolls[4]);
                                        f.stats.idles += 1;
                                    }
                                },
                                fnv_game::combat_style::Maneuver::Idle(secs) => {
                                    fighter.maneuver_until = clock + secs;
                                    f.stats.idles += 1;
                                }
                            }
                        }
                    }
                    if attack {
                        let speed = match &arm {
                            Arm::Melee(w) => {
                                let id = w.or(c.fists_id());
                                let rate = teammate_attack_rate(g, me, id);
                                if rate != 1.0 {
                                    info!("npc combat: {} {me} strikes at x{rate:.2} (the teammate perks)", g.data.name(me));
                                }
                                id.and_then(|w| c.forms.weapons.get(&w)).map_or(1.0, |w| w.attack_mult.max(0.25)) * rate
                            }
                            _ => 1.0,
                        };
                        fighter.strike_in = STRIKE_SECONDS / speed;
                        let dir = vec::normalize(vec::sub(tchest, eyes));
                        let (weapon, creature_damage) = match &arm {
                            Arm::Melee(w) => (*w, None),
                            Arm::Creature(d, _) => (None, Some(*d)),
                            Arm::Gun(_) => unreachable!(),
                        };
                        // A creature's power attack (module notes).
                        // With a crippled leg it does not leap: no forward power attack
                        // (crippled-limbs.md §4).
                        let lame = g.is_crippled(me, fnv_game::combat::LEFT_MOBILITY) || g.is_crippled(me, fnv_game::combat::RIGHT_MOBILITY);
                        let power = match (&arm, times.powers.get(&id)) {
                            (Arm::Creature(..), Some(have)) if !have.is_empty() => {
                                let (u, v) = (next_unit(&mut f.power_rng), next_unit(&mut f.power_rng));
                                // On the run: a lunge or nothing of the power attacks; a lunge
                                // alone was rolled already.
                                let (p, fallback) = match &early {
                                    Some((lunge, true)) => (lunge.first().copied(), false),
                                    Some((lunge, false)) => pick_power(style, lunge, fatigue, u, v),
                                    None => pick_power(style, &no_leaps(have, lame), fatigue, u, v),
                                };
                                if p.is_some_and(|p| p.0 == Power::Forward) && early.is_some() {
                                    f.stats.lunges += 1;
                                }
                                if let Some((p, _)) = p {
                                    *f.stats.powers.entry(p.group()).or_default() += 1;
                                    let chance = style.map(|s| creature_power_chance(s, fatigue)).filter(|_| !fallback).unwrap_or(FALLBACK_POWER_CHANCE);
                                    info!("npc combat: {me_name} power attack {} at {target_name} ({chance:.0} %, {})", p.group(), if fallback { "fallback: its combat style gives none" } else { "from its combat style" });
                                }
                                p
                            }
                            _ => None,
                        };
                        // A person's power attack (docs/spec-notes/combat-rules.md §10.1).
                        let people = match (&arm, times.people_powers.get(&id)) {
                            (Arm::Melee(w), Some((had, have))) if had == w => {
                                let rolls = [next_unit(&mut f.people_power_rng), next_unit(&mut f.people_power_rng)];
                                let moving = local_motion(fighter.velocity, heading(global));
                                let both_lame = g.is_crippled(me, fnv_game::combat::LEFT_MOBILITY) && g.is_crippled(me, fnv_game::combat::RIGHT_MOBILITY);
                                let pick = pick_people_power(style, opponent, fatigue, have, moving, both_lame, rolls);
                                if let Some(p) = pick.played {
                                    *f.stats.powers.entry(p.power.group()).or_default() += 1;
                                    info!("npc combat: {me_name} power attack {} at {target_name} ({:.0} %, {})", p.power.group(), pick.chance, pick.how);
                                } else if let Some(want) = pick.unplayable {
                                    f.stats.powers_unplayable += 1;
                                    info!("npc combat: {me_name} would power attack {} at {target_name} ({}), but its rig has no clip for it: an ordinary blow", want.group(), pick.how);
                                }
                                pick.played
                            }
                            _ => None,
                        };
                        // A power attack's clip plays out before the next decision (it
                        // is not cut by a combo) [design, as the player's].
                        if let Some(p) = people {
                            fighter.strike_in = fighter.strike_in.max(p.len);
                        }
                        let power = power.or(people.map(|p| (p.power, p.hit)));
                        // The blow lands at its clip's `Hit` key (the last clip's, as the
                        // next is chosen after this; a power attack's own) [design].
                        // Until its clip's own `Hit` key is known (above).
                        let delay = power.map(|p| p.1).or(times.hit.get(&id).copied()).unwrap_or(DEFAULT_HIT);
                        let anim = power.map(|p| p.0.group());
                        fighter.blows.push((delay, ActorStrike { attacker: me, target, weapon, creature_damage, point: tchest, dir, anim }));
                        // Going on a combo, or the first swing of one (module notes); a power attack
                        // plays its own clip and starts a new run [design].
                        let next = fighter.swing.filter(|_| power.is_none()).and_then(|s| s.next());
                        let run = next.map_or(1, |_| fighter.swing.map_or(0, |s| s.run) + 1);
                        if let Some(n) = next {
                            f.stats.chained += 1;
                            info!("npc combat: {me_name} goes on its combo with {n:?} (blow {run})");
                        }
                        f.stats.longest_combo = f.stats.longest_combo.max(run);
                        times.swings.remove(&id);
                        let creature = matches!(arm, Arm::Creature(..));
                        fighter.swing = Some(NpcSwing { t: 0.0, clip: None, run, power: power.is_some(), speed, creature });
                        let swing = SwingPick { next, rolls: [next_unit(&mut f.swing_rng), next_unit(&mut f.swing_rng)] };
                        anims.write(PlayAttack { actor: id, weapon, melee: true, sights: false, kind: Move::Attack, power: power.map(|p| p.0), swing: power.is_none().then_some(swing) });
                        f.stats.strikes += 1;
                        f.stats.swing_distances.push(away);
                        info!("npc combat: {me_name} swings at {target_name} from {away:.0} units (feet to feet, {})",
                              gap.map_or_else(|| format!("reach {reach:.0}"), |x| format!("bodies' gap {x:.0}, reach {reach:.0}")));
                        fighter.blows_begun += 1;
                        g.attack_line(me, Some(target), people.is_some(), false);
                    }
                }
            }
        }
        if let Some(actor) = grenade_probe.actor() {
            let deadline = f.grenade_at.get(&actor.0).copied();
            let fighting = g.awareness.get(actor).is_some_and(|a| a.state == State::Combat);
            grenade_probe.finish_frame(g, clock, deadline, fighting);
        }
        // Actors no longer fighting: no engage distance, no fighter, no post.
        let gone: Vec<u32> = f.fighters.keys().copied().filter(|id| !still.contains(id)).collect();
        for id in gone {
            f.fighters.remove(&id);
            g.awareness.engage.remove(&FormId(id));
            g.awareness.aiming.remove(&FormId(id));
            g.awareness.pace.remove(&FormId(id));
            g.awareness.tactics.posts.remove(&FormId(id));
            g.awareness.crouching.remove(&FormId(id));
        }
    });
    rt.game.end_faction_frame();
    // Who blocks now, for the blows dealt this frame (`nvf_play::combat`).
    combat.guarding = guarding;
}

/// The stance prefix of a melee arm (`1hm`, `2hm`; fists `h2h`), for whether it blocks.
fn melee_prefix(c: &Combat, arm: &Arm) -> &'static str {
    match arm {
        Arm::Melee(Some(w)) => c.forms.weapons.get(w).and_then(|ws| ws.anim_type.as_deref()).and_then(fnv_actor::weapon::AnimType::from_name).map_or("h2h", fnv_actor::weapon::AnimType::prefix),
        Arm::Melee(None) => "h2h",
        _ => "",
    }
}

/// What an actor fights with: the weapon drawn on it (a gun, unless its style keeps it to
/// melee), else a creature's own attack, else its fists.
fn arm_of(c: &Combat, g: &fnv_game::game::Game, weapon: Option<FormId>, me: FormId, scale: f32, melee_only: bool, skeleton: Option<&str>) -> Arm {
    match weapon {
        Some(w) if melee_only && c.forms.weapons.get(&w).is_some_and(|ws| is_gun(c, ws, w)) => Arm::Melee(None),
        Some(w) if c.forms.weapons.get(&w).is_some_and(|ws| is_gun(c, ws, w)) => Arm::Gun(w),
        Some(w) => Arm::Melee(Some(w)),
        None => match g.creature_attack(me) {
            Some((damage, reach)) => Arm::Creature(damage as f32, creature_reach(reach, skeleton, scale)),
            None => Arm::Melee(None),
        },
    }
}

fn describe(c: &Combat, arm: &Arm) -> String {
    let name = |w: &FormId| c.forms.weapons.get(w).and_then(|w| w.edid.clone()).unwrap_or_else(|| w.to_string());
    match arm {
        Arm::Gun(w) => format!("{} (gun)", name(w)),
        Arm::Melee(Some(w)) => format!("{} (melee)", name(w)),
        Arm::Melee(None) => "its fists".into(),
        Arm::Creature(d, r) => format!("its own attack ({d:.0} damage, reach {r:.0})"),
    }
}

impl Fighters {
    /// Whom the person `reference` is locked on this frame, and where that target's feet
    /// are drawn, while it sees that target and does not run from it.
    pub fn lock_on(&self, reference: u32) -> Option<(u32, V3)> {
        self.locks.get(&reference).copied()
    }
}

fn report(mut exit: MessageReader<AppExit>, fighters: Res<Fighters>, stats: Option<Res<CombatStats>>) {
    if exit.read().next().is_none() {
        return;
    }
    let s = &fighters.stats;
    let (hits, damage, died) = stats.map_or((0, 0.0, false), |c| (c.player_hits, c.player_damage, c.player_died));
    info!(
        "npc combat summary: {} actors fought ({}); {} shots ({} from a muzzle, {} from the eyes; {} down the sights, {} from the hip, sights raised {} times), {} blows ({} went on a combo, the longest {}; {} missed: out of reach when they landed; {} melee decisions to hold), {} reloads ({} round by round); {} flee checks, {} fled ({} hid at their flee spot, {} ran on when seen there); the player was hit {hits} times for {damage:.1}{}; {} staggers held a fighter's attacks ({} blows cut short); {} frames a fighter waited to face its target",
        s.fighters.len(),
        s.fighters.values().cloned().collect::<Vec<_>>().join(", "),
        s.shots,
        s.from_muzzle,
        s.from_eyes,
        s.sights_shots,
        s.hip_shots,
        s.sights_raised,
        s.strikes,
        s.chained,
        s.longest_combo,
        s.missed,
        s.holds,
        s.reloads,
        s.round_reloads,
        s.flee_checks,
        s.flees,
        s.hides,
        s.hides_seen,
        if died { " and died" } else { "" },
        s.staggers,
        s.blows_cut,
        s.unfaced,
    );
    let spread = |v: &[f32]| {
        let mut v = v.to_vec();
        v.sort_by(f32::total_cmp);
        match v.len() {
            0 => "none".to_string(),
            n => format!("median {:.1}°, 90th {:.1}°, max {:.1}°, over 10° in {}", v[n / 2], v[n * 9 / 10], v[n - 1], v.iter().filter(|o| **o > 10.0).count()),
        }
    };
    info!("npc combat facing: {} shots, off their target {}; {} frames a person was locked on its target, its body off it {}", s.shot_offs.len(), spread(&s.shot_offs), s.seen_offs.len(), spread(&s.seen_offs));
    if s.idle_holds > 0 {
        info!("npc combat: {} idles flagged No attacking held a fighter's attacks", s.idle_holds);
    }
    let mut d = s.swing_distances.clone();
    d.sort_by(f32::total_cmp);
    if !d.is_empty() {
        info!(
            "npc melee spacing: {} blows began at {:.0} to {:.0} units from the target (median {:.0}, feet to feet); the closest a melee fighter came: {:.0}",
            d.len(),
            d[0],
            d[d.len() - 1],
            d[d.len() / 2],
            s.melee_closest.unwrap_or(0.0),
        );
    }
    info!(
        "npc stance summary: crouched {} times ({} behind low cover), stood up {} times ({} for a close-combat enemy), {} shifts; {} blocks ({} in reaction), {} blows blocked by fighters, {} recoils; {} bounds to cover, {} charges in the open, {} guards",
        s.crouches,
        s.low_crouches,
        s.stands,
        s.stands_close,
        s.shifts,
        s.blocks,
        s.reactive_blocks,
        s.blocked,
        s.recoils,
        s.bounds,
        s.charges,
        s.guards,
    );
    info!(
        "npc gun reach summary: held fire {} times at a target beyond reach, {} times within a blast; left cover {} times to close in; the farthest shot {:.0} % of its gun's reach, {} shots beyond it",
        s.held_far,
        s.held_blast,
        s.left_cover_for_reach,
        s.farthest_share * 100.0,
        s.shots_beyond,
    );
    info!(
        "npc combat style summary: {} dodges ({} left, {} right, {} forward, {} back), {} idles; {} rushes; {} creature attacks begun on the run ({} lunges); {} pursuits stopped at the combat radius; {} targets outside the targeting field of view; {} explosive shots held for a comrade or a bystander; {} runs from grenades; {} grenades thrown, {} kept for a comrade or a bystander",
        s.dodges.iter().sum::<usize>(),
        s.dodges[0],
        s.dodges[1],
        s.dodges[2],
        s.dodges[3],
        s.idles,
        s.rushes,
        s.run_attacks,
        s.lunges,
        s.leashes,
        s.fov_waits,
        s.blast_holds,
        s.evades,
        s.grenades,
        s.grenade_holds,
    );
    if !s.powers.is_empty() || s.powers_unplayable > 0 {
        info!(
            "npc power attacks summary: {}; {} chosen without a clip (ordinary blows)",
            s.powers.iter().map(|(g, n)| format!("{g} {n}")).collect::<Vec<_>>().join(", "),
            s.powers_unplayable
        );
    }
}

/// Where a fighter in cover stands, `what` it did there (reaches its cover, hid behind
/// it): the room its body has (`combat_tactics::room`), for the log; under the capsule's
/// radius it is inside something (docs/design/combat-ai.md §12.6.2).
fn log_room(phys: Option<&mut PhysState>, name: &str, what: &str, feet: [f32; 3], spot: [f32; 3]) {
    let Some(p) = phys else { return };
    let r = tactics::room(p, feet);
    info!(
        "npc combat: {name} in cover at ({:.0}, {:.0}, {:.0}) ({what}), {:.0} units from its cover spot: room {:.0} ({})",
        feet[0],
        feet[1],
        feet[2],
        (spot[0] - feet[0]).hypot(spot[1] - feet[1]),
        r.nearest,
        if r.nearest < tactics::BODY_RADIUS { "inside geometry" } else { "clear" }
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranged_cycle(ws: &WeaponStats) -> Cycle {
        Cycle::new(ws, Timing { interval: 0.1, fire_delay: 0.2, reload: 1.0, spin_up: 0.0, rounds: None, attach: None }, false, 2)
    }

    /// #2120: the shooter (`use_weapon`) acts on what `Game::active_weapon_use` gives and
    /// on a melee use's last blow still to land; each keeps the ordinary attacks off, and a
    /// script's use comes first as before. In a combat state a package's use keeps them off
    /// only while the shooter acts on it (`use_weapon::acts_on`, #2145 review).
    #[test]
    fn a_weapon_use_being_carried_out_keeps_the_ordinary_attacks_off() {
        use fnv_game::use_weapon::{Source, WeaponUse};
        let script = WeaponUse::of_script(None, None, FormId(0x7), 3, true, true, false, false, None);
        let package = WeaponUse { source: Source::Package(FormId(0x9)), volleys: Some(2), shots_per_volley: [1, 1], ..script.clone() };
        let there = WeaponUse { in_place: true, ..package.clone() };
        // No weapon use: the actor fights.
        assert!(!weapon_use_holds_fight(None, None, true, false, false));
        // A script's use, walking to its place or firing, its target standing or not.
        for (in_combat, standing) in [(false, false), (true, false), (true, true)] {
            assert!(weapon_use_holds_fight(Some(&script), Some(&script), in_combat, standing, false));
            assert!(weapon_use_holds_fight(Some(&script), None, in_combat, standing, false), "a script's use comes first whatever its state");
        }
        // A package's use held in combat (no Continue During Combat): no package fire, the
        // actor fights.
        assert!(!weapon_use_holds_fight(Some(&package), None, true, true, false));
        // Out of a combat state a package's use still to be carried out keeps the attacks
        // off, as before #2120, wherever it is and whatever its target.
        assert!(weapon_use_holds_fight(Some(&package), Some(&package), false, false, false));
        assert!(weapon_use_holds_fight(Some(&there), Some(&there), false, false, false));
        // Its package continues during combat: the attacks are off only while the shooter
        // acts on it, in place with a drawn or placed aim point [design, #2652].
        assert!(weapon_use_holds_fight(Some(&there), Some(&there), true, true, false));
        // Not yet at its stand (on its way, or never got there): the actor fights.
        assert!(!weapon_use_holds_fight(Some(&package), Some(&package), true, true, false), "on its way to its stand");
        // [design, #2652] At its stand without any drawn or placed aim point,
        // nothing for the shooter: ordinary attacks take over.
        assert!(!weapon_use_holds_fight(Some(&there), Some(&there), true, false, false), "no drawn or placed target position");
        // Spent (kept, done, until the package changes) or let go: the actor fights again,
        // but not while the last blow of a melee use is still to land.
        let spent = WeaponUse { done: true, ..there.clone() };
        assert!(weapon_use_holds_fight(Some(&spent), None, true, true, true), "the shooter still lands its last blow");
        assert!(!weapon_use_holds_fight(Some(&spent), None, true, true, false));
    }

    /// #2145 review: a Continue During Combat package's use in Combat whose target comes and
    /// goes among the drawn people hands the actor between the shooter and the fight frame
    /// after frame without its fight ever being dropped (`UseHold::Out`); a script's use
    /// drops it as before, and outside Combat nothing is kept.
    #[test]
    fn a_package_use_handed_back_and_forth_keeps_the_actors_fight() {
        use fnv_game::use_weapon::{Source, WeaponUse};
        let script = WeaponUse::of_script(None, None, FormId(0x7), 3, true, true, false, false, None);
        let there = WeaponUse { source: Source::Package(FormId(0x9)), in_place: true, volleys: None, ..script.clone() };
        let mut drops = 0;
        for frame in 0..40 {
            let standing = frame % 3 != 0;
            let hold = use_hold(Some(&there), Some(&there), State::Combat, standing, false);
            assert_eq!(hold, if standing { UseHold::Kept } else { UseHold::Taken }, "frame {frame}");
            drops += usize::from(hold == UseHold::Out);
        }
        assert_eq!(drops, 0);
        // A script's use holds the fight and drops its state, as before.
        assert_eq!(use_hold(Some(&script), Some(&script), State::Combat, true, false), UseHold::Out);
        // No use, or a held one (no flag: not active in combat): the actor fights.
        assert_eq!(use_hold(None, None, State::Combat, false, false), UseHold::Free);
        assert_eq!(use_hold(Some(&there), None, State::Combat, true, false), UseHold::Free);
        // Not in Combat: nothing kept, nothing taken.
        assert_eq!(use_hold(Some(&there), Some(&there), State::Alert, true, false), UseHold::Out);
        assert_eq!(use_hold(Some(&there), Some(&there), State::Alert, false, false), UseHold::Out);
        assert_eq!(use_hold(Some(&there), Some(&there), State::Normal, false, false), UseHold::Out);
        // A package's last melee blow landing (no longer active) holds the fight and drops
        // its state, as before #2120, with the flag or without.
        let spent = WeaponUse { done: true, ..there };
        assert_eq!(use_hold(Some(&spent), None, State::Combat, true, true), UseHold::Out);
    }

    /// #2145 review: a kept fighter's gun handed to the shooter (`settle_for_use`, from
    /// `hold_for_use`) is neither reloading (so not busy) nor about to fire: a reload or
    /// a committed wind-up under way is let go; a cooldown, or a gun at rest, stays.
    #[test]
    fn a_gun_handed_to_the_shooter_is_left_neither_reloading_nor_about_to_fire() {
        let gun = WeaponStats { clip_size: 2, ..Default::default() };
        let input = Input { trigger: false, dt: 0.2, carried: 20, ..Default::default() };
        let fired = |c: &mut Cycle| (0..10).any(|_| c.step(&Default::default(), input).iter().any(|e| matches!(e, Event::Fired { .. })));
        for phase in [GunPhase::Reloading { left: 0.5 }, GunPhase::ReloadOpen { left: 0.5, stop: false }, GunPhase::ReloadRound { left: 0.5, stop: false, taken: 1 }, GunPhase::ReloadClose { left: 0.5, taken: 1 }] {
            let mut cycle = ranged_cycle(&gun);
            (cycle.magazine, cycle.phase) = (0, phase);
            let mut attack = RangedAttack::Firearm { cycle, reload_timed: true };
            assert!(attack.reloading(), "{phase:?}");
            settle_for_use(Some(&mut attack));
            assert!(!attack.reloading(), "{phase:?}: not reloading, so not busy");
            let (cycle, _) = attack.firearm_mut().unwrap();
            assert_eq!(cycle.magazine, 0, "the magazine as it was");
            assert!(!fired(cycle), "{phase:?}: no shot without a pull");
        }
        for phase in [GunPhase::WindUp { left: 0.1 }, GunPhase::SpinUp { left: 0.1 }] {
            let mut cycle = ranged_cycle(&gun);
            cycle.phase = phase;
            if matches!(phase, GunPhase::WindUp { .. }) {
                // Left as it was, the wind-up fires with the trigger off.
                assert!(fired(&mut cycle.clone()), "the shot already in flight");
            }
            let mut attack = RangedAttack::Firearm { cycle, reload_timed: true };
            settle_for_use(Some(&mut attack));
            let (cycle, _) = attack.firearm_mut().unwrap();
            assert!(matches!(cycle.phase, GunPhase::Ready), "{phase:?}");
            assert!(!fired(cycle), "{phase:?}: no shot after the hand-over without a pull");
        }
        // A cooldown is no shot or reload under way: kept.
        let mut cycle = ranged_cycle(&gun);
        cycle.phase = GunPhase::Cooldown { left: 0.3 };
        let mut attack = RangedAttack::Firearm { cycle, reload_timed: true };
        settle_for_use(Some(&mut attack));
        assert!(matches!(attack.firearm_mut().unwrap().0.phase, GunPhase::Cooldown { .. }));
        // A held grenade or no ranged attack: nothing to settle.
        settle_for_use(Some(&mut RangedAttack::Grenade));
        settle_for_use(None);
    }

    #[test]
    fn held_grenade_execution_discards_even_a_committed_firearm_windup() {
        let gun = WeaponStats { clip_size: 2, ..Default::default() };
        let mut cycle = ranged_cycle(&gun);
        cycle.phase = GunPhase::WindUp { left: 0.1 };
        // Merely denying the trigger would still emit the shot already in flight.
        let mut control = cycle.clone();
        let input = Input { trigger: false, dt: 0.2, carried: 20, ..Default::default() };
        assert!(control.step(&Default::default(), input).iter().any(|e| matches!(e, Event::Fired { .. })));
        let grenade = WeaponStats { anim_type: Some(crate::npc_grenades::GRENADE_ANIM.into()), ..Default::default() };
        let mut execution = RangedAttack::for_weapon(&grenade, Some(RangedAttack::Firearm { cycle, reload_timed: true }), || panic!("a grenade must not construct a gun cycle"));
        for _ in 0..100 {
            let events = execution.firearm_mut().map(|(c, _)| c.step(&Default::default(), input)).unwrap_or_default();
            assert!(events.is_empty(), "no wind-up, refire or reload can survive grenade dispatch");
            assert!(!execution.reloading());
        }
    }

    #[test]
    fn held_grenade_execution_has_no_firearm_for_initial_or_refire_updates() {
        let grenade = WeaponStats { anim_type: Some(crate::npc_grenades::GRENADE_ANIM.into()), automatic: true, clip_size: 2, ..Default::default() };
        let mut execution = RangedAttack::for_weapon(&grenade, None, || panic!("no gun cycle for a held grenade"));
        for trigger in [false, true, true, false, true] {
            let events = execution.firearm_mut().map(|(c, _)| c.step(&Default::default(), Input { trigger, reload: true, dt: 30.0, carried: 100 })).unwrap_or_default();
            assert!(events.is_empty());
        }
        // Returning to a firearm starts a real gun cycle, with no stale grenade state.
        let gun = WeaponStats { clip_size: 2, ..Default::default() };
        execution = RangedAttack::for_weapon(&gun, Some(execution), || ranged_cycle(&gun));
        let (cycle, _) = execution.firearm_mut().unwrap();
        assert!(cycle.step(&Default::default(), Input { trigger: true, dt: 0.3, carried: 20, ..Default::default() }).iter().any(|e| matches!(e, Event::Fired { .. })));
    }

    #[test]
    fn firearm_execution_retains_its_cycle_on_target_changes_and_other_throw_types_are_unchanged() {
        let gun = WeaponStats { clip_size: 2, ..Default::default() };
        let mut cycle = ranged_cycle(&gun);
        cycle.magazine = 1;
        cycle.phase = GunPhase::Reloading { left: 0.5 };
        let mut execution = RangedAttack::for_weapon(&gun, Some(RangedAttack::Firearm { cycle, reload_timed: true }), || panic!("same firearm keeps its cycle"));
        assert!(execution.reloading());
        let (cycle, timed) = execution.firearm_mut().unwrap();
        assert_eq!(cycle.magazine, 1);
        assert!(*timed);
        assert!(cycle.step(&Default::default(), Input { dt: 1.0, carried: 20, ..Default::default() }).iter().any(|e| matches!(e, Event::Reloaded { .. })));
        // #1498 deliberately does not redefine mines, other throws or launchers.
        for name in ["Land Mine (1 Hand)", "Mine Drop (1 Hand)", "Thrown (1 Hand)", "Launcher (2 Hand)"] {
            let ws = WeaponStats { anim_type: Some(name.into()), clip_size: 2, ..Default::default() };
            assert!(RangedAttack::for_weapon(&ws, None, || ranged_cycle(&ws)).firearm_mut().is_some());
        }
    }

    #[test]
    fn a_dropped_post_ends_the_guard_or_the_bound_it_was_for() {
        let guarding = Guarding { friend: 1, since: 0.0, patience: 1.0, turn: 0.0, at: [0.0; 3] };
        let bound = Bound { at: [0.0; 3], arrived: None, pause: 0.0 };
        let mut ap = Approach { posted: Some(PostKind::Guard), guard: Some(guarding), bound: Some(bound), ..Default::default() };
        ap.post_dropped();
        assert!(ap.guard.is_none() && ap.guarded && ap.bound.is_some());
        let mut ap = Approach { posted: Some(PostKind::Advance), guard: Some(guarding), bound: Some(bound), ..Default::default() };
        ap.post_dropped();
        assert!(ap.bound.is_none() && ap.bound_in == stance::BOUND_RETRY && ap.guard.is_some() && !ap.guarded);
        // It was given none: nothing ends.
        let mut ap = Approach { guard: Some(guarding), bound: Some(bound), ..Default::default() };
        ap.post_dropped();
        assert!(ap.guard.is_some() && ap.bound.is_some() && !ap.guarded && ap.bound_in == 0.0);
    }

    #[test]
    fn embedded_weapon_node_by_name() {
        // The sentry bot's skeleton names its gun node so (meshes\creatures\sentrybot).
        let names = vec![Some("Bip01".to_string()), None, Some("ProjectileNode_RGatling".to_string())];
        assert_eq!(embedded_node_index(&names, "ProjectileNode_RGatling"), Some(2));
        assert_eq!(embedded_node_index(&names, "projectilenode_rgatling"), Some(2));
        assert_eq!(embedded_node_index(&names, "ProjectileNode_HeadLaser"), None);
    }

    #[test]
    fn melee_keeps_its_reach_from_the_target() {
        // Two people (radii 22 + 22), fCombatDistance 107: the original's troopers began
        // their swings about 45 + 107 x Reach away (o14_reach): knife (0.5) 95,
        // sledgehammer (0.75) 120, super sledge (1.0) 150, bumper sword (1.2) 175, lead
        // pipe (1.25) 180.
        for (reach, original) in [(0.5, 95.0), (0.75, 120.0), (1.0, 150.0), (1.2, 175.0), (1.25, 180.0)] {
            let (stop, strike) = melee_spacing(44.0, 44.0, 107.0 * reach);
            // A torso leaning forward (its block 20 units ahead) changes nothing.
            assert_eq!(melee_spacing(24.0, 44.0, 107.0 * reach), (stop, strike));
            assert!(stop < strike && (strike - original).abs() <= 5.0, "{reach} {strike} {original}");
        }
        // A short reach still keeps the bodies' gaps.
        assert_eq!(melee_spacing(44.0, 44.0, 0.0), (44.0 + MELEE_STOP_GAP, 44.0 + MELEE_HIT_GAP));
        // A long target's body ahead of its centre keeps the gaps from it.
        assert_eq!(melee_spacing(150.0, 44.0, 53.5), (150.0 + MELEE_STOP_GAP, 150.0 + MELEE_HIT_GAP));
    }

    #[test]
    fn combat_uses_ordinary_walking_blocks_only_in_widened_pairs() {
        use nvf_play::phys::{NpcBody, PhysState};
        let mut phys = PhysState::default();
        let id = 123;
        let feet = [10.0, 20.0, 30.0];
        let ch = phys.world.add_character(id, fnv_phys::CharacterParams::default(), feet);
        let standing_height = ch.params.height;
        phys.npcs.insert(id, NpcBody { ch, prev_feet: feet, velocity: [0.0; 2], jump: false, sink: 0.0, lift: Default::default(), standing_height, sneak_crouch: false });
        assert!(CombatBodies::new(Some(&phys), None).body(id, 456).is_none());
        phys.actor_block_radii.insert(id, 10.0);
        assert!(CombatBodies::new(Some(&phys), None).body(id, 456).is_none());
        phys.actor_block_radii.insert(id, 70.0);
        let body = CombatBodies::new(Some(&phys), None).body(id, 456).unwrap();
        assert_eq!((body.a, body.b, body.radius), ([10.0, 20.0], [10.0, 20.0], 70.0));
        assert_eq!(phys.npcs[&id].ch.params.radius, 22.0);
        phys.npcs.clear();
        assert!(CombatBodies::new(Some(&phys), None).body(id, 456).is_none());
    }

    /// #3256: a drawn block that is not where its actor stands gives no gap (it read
    /// 1,161 units for a gecko 51 units from the player); one at the actor's feet does.
    #[test]
    fn a_block_left_behind_gives_no_gap() {
        use nvf_play::phys::PhysState;
        let mut phys = PhysState::default();
        let player_feet = [0.0, 0.0, 0.0];
        phys.player = Some(phys.world.add_character(FormId::PLAYER_REF.0, fnv_phys::CharacterParams::default(), player_feet));
        let player_radius = phys.player.as_ref().unwrap().params.radius;
        let me = 0xFF00_0000;
        // Its block where it stood 1,100 units back; its feet now 51 units from the player.
        let mut blocks = ActorBlocks { list: vec![Block::character(me, [1151.0, 0.0, 0.0], 30.0, 60.0, false)], ..Default::default() };
        let feet = [51.0, 0.0, 0.0];
        let bodies = CombatBodies::new(Some(&phys), Some(&blocks));
        assert_eq!(bodies.gap(me, FormId::PLAYER_REF.0, player_feet), Some(1151.0 - 30.0 - player_radius), "the old measure");
        assert_eq!(bodies.gap_at(me, FormId::PLAYER_REF.0, feet, player_feet), None);
        // The block where the actor is (a body that reaches a little ahead of its feet).
        blocks.list = vec![Block::character(me, [45.0, 0.0, 0.0], 30.0, 60.0, false)];
        let bodies = CombatBodies::new(Some(&phys), Some(&blocks));
        assert_eq!(bodies.gap_at(me, FormId::PLAYER_REF.0, feet, player_feet), Some(45.0 - 30.0 - player_radius));
        // Just within and just beyond the limit, from the block's edge.
        blocks.list = vec![Block::character(me, [51.0 + 30.0 + STALE_BLOCK - 1.0, 0.0, 0.0], 30.0, 60.0, false)];
        assert!(CombatBodies::new(Some(&phys), Some(&blocks)).gap_at(me, FormId::PLAYER_REF.0, feet, player_feet).is_some());
        blocks.list = vec![Block::character(me, [51.0 + 30.0 + STALE_BLOCK + 1.0, 0.0, 0.0], 30.0, 60.0, false)];
        assert!(CombatBodies::new(Some(&phys), Some(&blocks)).gap_at(me, FormId::PLAYER_REF.0, feet, player_feet).is_none());
    }

    #[test]
    fn walking_humanoid_can_strike_a_widened_creature_before_and_after_it_stops() {
        use nvf_play::phys::{NpcBody, PhysState};
        // Synthetic walking pair and knife-like weapon [design, #2535]. BBX width,
        // RNAM and scales supplied in #2535 [observed: FalloutNV.esm and Fallout -
        // Meshes.bsa, fnvtool esm decode / nif decode, main session 2026-11-15].
        let (person, creature) = (123, 456);
        let weapon = WeaponStats { reach: 0.5, ..Default::default() };
        let rules = fnv_combat::tuning::Rules { combat_distance: 107.0, ..fnv_combat::tuning::Rules::defaults() };
        let knife_reach = fnv_combat::damage::melee_reach(&rules, &weapon, 1.0);
        assert_eq!(knife_reach, 53.5);
        for scale in [0.6, 1.0] {
            let mut phys = PhysState::default();
            let width = 70.56 * scale;
            let combined = 22.0 + width;
            let creature_reach = creature_reach(Some(64.0), Some(r"Creatures\Radscorpion\Skeleton.nif"), scale);
            for (id, feet) in [(person, [0.0; 3]), (creature, [combined + 0.01, 0.0, 0.0])] {
                let ch = phys.world.add_character(id, fnv_phys::CharacterParams::default(), feet);
                let standing_height = ch.params.height;
                phys.npcs.insert(id, NpcBody { ch, prev_feet: feet, velocity: [25.0, 0.0], jump: false, sink: 0.0, lift: Default::default(), standing_height, sneak_crouch: false });
            }
            phys.actor_block_radii.insert(creature, width);
            let height = phys.npcs[&creature].ch.params.height;
            phys.actor_heights.insert(creature, height);
            let mut blocks = ActorBlocks::default();
            // Walkers are absent from the drawn list; stopped creatures retain a
            // BBX circle there [design: blocking.rs, hitvol::sync_volumes, #2535].
            for standing in [false, true, false] {
                if standing {
                    phys.remove_npc(creature);
                    assert!(!phys.npcs.contains_key(&creature));
                } else if !phys.npcs.contains_key(&creature) {
                    let feet = [combined + 0.01, 0.0, 0.0];
                    let ch = phys.world.add_character(creature, fnv_phys::CharacterParams::default(), feet);
                    let standing_height = ch.params.height;
                    phys.npcs.insert(creature, NpcBody { ch, prev_feet: feet, velocity: [25.0, 0.0], jump: false, sink: 0.0, lift: Default::default(), standing_height, sneak_crouch: false });
                    blocks.list.clear();
                }
                for offset in [0.0, 15.0] {
                    for (distance, knife_near) in [(combined + 0.01, true), (combined + knife_reach - 0.01, true), (combined + knife_reach + 0.01, false)] {
                        let person_feet = [offset, 0.0, 0.0];
                        let creature_feet = [offset + distance, 0.0, 0.0];
                        phys.npcs.get_mut(&person).unwrap().ch.feet = person_feet;
                        if standing {
                            blocks.list = vec![Block::character(creature, creature_feet, width, height, false)];
                        } else {
                            phys.npcs.get_mut(&creature).unwrap().ch.feet = creature_feet;
                        }
                        let bodies = CombatBodies::new(Some(&phys), Some(&blocks));
                        for (me, target, feet, tfeet, arm, reach, near) in [
                            (person, creature, person_feet, creature_feet, Arm::Melee(None), knife_reach, knife_near),
                            (creature, person, creature_feet, person_feet, Arm::Creature(1.0, creature_reach), creature_reach, distance <= creature_reach.max(combined + MELEE_HIT_GAP)),
                        ] {
                            let away = flat(feet, tfeet);
                            let gap = bodies.gap(me, target, tfeet);
                            let radii = bodies.radii(me, target);
                            assert!((radii.unwrap() - combined).abs() < 1e-4, "standing={standing}");
                            assert!((gap.unwrap() - (away - combined)).abs() < 1e-4);
                            assert_eq!(in_melee_reach(&arm, away, gap, radii, reach), near, "standing={standing}");
                        }
                    }
                }
                // Missing or smaller BBX must not enable the ordinary walker
                // fallback, even with a drawn target [design, #2535].
                for bbx in [None, Some(10.0)] {
                    phys.actor_block_radii.clear();
                    phys.actor_block_radii.extend(bbx.map(|r| (creature, r)));
                    let bodies = CombatBodies::new(Some(&phys), Some(&blocks));
                    assert!(bodies.radii(person, creature).is_none());
                    assert!(bodies.radii(creature, person).is_none());
                }
                phys.actor_block_radii.insert(creature, width);
                if standing {
                    // Eligibility alone must never invent a missing target block
                    // [design: CombatBodies::pair].
                    blocks.list.clear();
                    assert!(CombatBodies::new(Some(&phys), Some(&blocks)).radii(person, creature).is_none());
                }
            }
            // No widened participant: keep ordinary pairs' existing lookup
            // [design, #2535].
            phys.actor_block_radii.clear();
            let bodies = CombatBodies::new(Some(&phys), Some(&blocks));
            assert!(bodies.radii(person, creature).is_none());
            assert!(bodies.gap(person, creature, phys.npcs[&creature].ch.feet).is_none());
        }
    }

    #[test]
    fn radscorpion_reach_stays_beyond_bbx_and_player_blocks() {
        // Record and skeleton facts supplied in #2535 [observed: FalloutNV.esm and
        // Fallout - Meshes.bsa, fnvtool esm decode / nif decode, 2026-11-15].
        for scale in [0.6, 1.0] {
            let bodies = nvf_play::blocking::walking_radius(22.0, Some(70.56 * scale)) + 22.0;
            let reach = creature_reach(Some(64.0), Some(r"Creatures\Radscorpion\Skeleton.nif"), scale);
            let (stop, strike) = creature_spacing(Some(bodies), reach);
            assert!((strike - 188.0 * scale).abs() < 1e-4);
            assert!(stop > bodies && strike > stop);
        }
    }

    #[test]
    fn creatures_keep_their_reach_and_body_from_the_target() {
        // The original's median swing starts (o14_reach, o12_reach, melee_spacing), by
        // skeleton, RNAM and scale.
        let rows = [
            ("creatures/nvgecko/skeleton.nif", 35.0, 0.8, 81.0),
            ("creatures/nvcazadores/skeleton.nif", 100.0, 0.5, 80.0),
            ("creatures/nightstalker/skeleton.nif", 48.0, 0.75, 102.0),
            (r"Creatures\Radscorpion\Skeleton.nif", 64.0, 0.6, 113.0),
            ("creatures/ghoul/skeleton.nif", 96.0, 1.0, 137.5),
            ("creatures/nvbighorner/skeleton.nif", 50.0, 1.7, 232.0),
            ("creatures/dog/skeleton.nif", 48.0, 1.2, 133.0),
            ("creatures/dog/skeleton.nif", 48.0, 1.0, 109.0),
            // No row of its own: the giant mantis (100, 1.0) 163.
            ("creatures/nvmantis/skeleton.nif", 100.0, 1.0, 163.0),
        ];
        for (skel, rnam, scale, original) in rows {
            let (stop, strike) = creature_spacing(None, creature_reach(Some(rnam), Some(skel), scale));
            assert!(stop < strike && (strike - original).abs() < 0.05 * original, "{skel} {strike} {original}");
        }
        assert_eq!(creature_reach(None, Some("creatures/dog/skeleton.nif"), 2.0), CREATURE_REACH * 2.0);
        assert_eq!(creature_body_reach(None), CREATURE_BODY_REACH);
        // Big bodies still keep their gaps.
        assert_eq!(creature_spacing(Some(150.0), 100.0), (150.0 + MELEE_STOP_GAP, 150.0 + MELEE_HIT_GAP));
    }

    #[test]
    fn a_creatures_power_attack() {
        let both = [(Power::Normal, 0.4), (Power::Forward, 0.6)];
        // No style: the fallback chance, then evenly.
        assert_eq!(pick_power(None, &both, 0.0, 0.1, 0.2), (Some((Power::Normal, 0.4)), true));
        assert_eq!(pick_power(None, &both, 0.0, 0.1, 0.7), (Some((Power::Forward, 0.6)), true));
        assert_eq!(pick_power(None, &both, 0.0, FALLBACK_POWER_CHANCE / 100.0, 0.7).0, None);
        assert_eq!(pick_power(None, &[], 0.0, 0.0, 0.0).0, None);
        // The style's chance and shares.
        let s = fnv_game::data::CombatStyleDef { power_attack_chance: 50.0, power_attack_dirs: [0.0, 100.0, 0.0, 0.0, 0.0], ..Default::default() };
        assert_eq!(pick_power(Some(&s), &both, 0.0, 0.45, 0.1), (Some((Power::Forward, 0.6)), false));
        assert_eq!(pick_power(Some(&s), &both, 0.0, 0.55, 0.1).0, None);
        assert_eq!(pick_power(Some(&s), &both[..1], 0.0, 0.1, 0.9).0, Some((Power::Normal, 0.4)));
    }

    /// The `_male` 1hm power clips' lengths [observed: combat-rules.md §10.1]; hit times
    /// made up for the test.
    fn one_hand_powers() -> Vec<PowerClip> {
        [(Power::Normal, 1.20), (Power::Forward, 1.60), (Power::Back, 1.27), (Power::Left, 1.37), (Power::Right, 1.23)].into_iter().map(|(power, len)| PowerClip { power, hit: 0.5, len }).collect()
    }

    #[test]
    fn a_persons_power_attack_by_its_styles_chance() {
        // DefaultCombatstyle: 25 %, +5 recoiling, 20 each way, by chance [observed].
        let s = fnv_game::data::CombatStyleDef { power_attack_chance: 25.0, recoil_stagger_power_bonus: 5.0, power_attack_dirs: [20.0; 5], power_by_chance: true, ..Default::default() };
        let have = one_hand_powers();
        let calm = Opponent::default();
        let pick = |opp, rolls| pick_people_power(Some(&s), opp, 0.0, &have, (0.0, 0.0), false, rolls);
        assert_eq!(pick(calm, [0.24, 0.1]).played.map(|p| p.power), Some(Power::Normal));
        assert_eq!(pick(calm, [0.24, 0.5]).played.map(|p| p.power), Some(Power::Back));
        assert_eq!(pick(calm, [0.24, 0.95]).played.map(|p| p.power), Some(Power::Right));
        assert_eq!(pick(calm, [0.24, 0.1]).how, "by chance");
        // 27 is past 25 but within 25 + 5 while the opponent recoils.
        assert_eq!(pick(calm, [0.27, 0.1]).played, None);
        assert_eq!(pick(Opponent { recoiling: true, ..calm }, [0.27, 0.1]).played.map(|p| p.power), Some(Power::Normal));
        // A direction without a clip: an ordinary blow, said so.
        let some: Vec<PowerClip> = have.iter().copied().filter(|p| p.power != Power::Back).collect();
        let p = pick_people_power(Some(&s), calm, 0.0, &some, (0.0, 0.0), false, [0.1, 0.5]);
        assert_eq!((p.played, p.unplayable), (None, Some(Power::Back)));
        // No clips at all (a gun's stance): nothing.
        assert_eq!(pick_people_power(Some(&s), calm, 0.0, &[], (0.0, 0.0), false, [0.0, 0.0]), PeoplePick { played: None, unplayable: None, chance: 25.0, how: "by chance" });
        // Both legs crippled: the standing one whatever the shares say.
        assert_eq!(pick_people_power(Some(&s), calm, 0.0, &have, (0.0, 0.0), true, [0.1, 0.95]).played.map(|p| p.power), Some(Power::Normal));
        // Its clip's timing goes with it.
        assert_eq!(pick(calm, [0.0, 0.3]).played, Some(PowerClip { power: Power::Forward, hit: 0.5, len: 1.60 }));
        // A style of 0 %: never.
        let never = fnv_game::data::CombatStyleDef { power_attack_chance: 0.0, ..s.clone() };
        assert_eq!(pick_people_power(Some(&never), calm, 0.0, &have, (0.0, 0.0), false, [0.0, 0.0]).played, None);
        // No style: the fallback chance, even shares.
        assert_eq!(pick_people_power(None, calm, 0.0, &have, (0.0, 0.0), false, [0.2, 0.5]).played.map(|p| p.power), Some(Power::Back));
        assert_eq!(pick_people_power(None, calm, 0.0, &have, (0.0, 0.0), false, [FALLBACK_POWER_CHANCE / 100.0, 0.5]).played, None);
    }

    #[test]
    fn a_persons_power_attack_by_how_it_moves() {
        // Legate Lanius's style: 45 %, by movement [observed].
        let s = fnv_game::data::CombatStyleDef { power_attack_chance: 45.0, power_attack_dirs: [10.0, 60.0, 10.0, 10.0, 10.0], power_by_chance: false, ..Default::default() };
        let have = one_hand_powers();
        let facing_north = [0.0, 1.0];
        let by = |velocity| pick_people_power(Some(&s), Opponent::default(), 0.0, &have, local_motion(velocity, facing_north), false, [0.1, 0.0]).played.map(|p| p.power);
        assert_eq!(by([0.0, 0.0]), Some(Power::Normal));
        assert_eq!(by([0.0, 10.0]), Some(Power::Normal), "slower than the threshold stands");
        assert_eq!(by([0.0, 200.0]), Some(Power::Forward));
        assert_eq!(by([0.0, -150.0]), Some(Power::Back));
        assert_eq!(by([150.0, 20.0]), Some(Power::Right), "east of a north-facing actor is its right");
        assert_eq!(by([-150.0, 0.0]), Some(Power::Left));
        // Forward wins a tie.
        assert_eq!(by([100.0, 100.0]), Some(Power::Forward));
        assert_eq!(pick_people_power(Some(&s), Opponent::default(), 0.0, &have, (0.0, 0.0), false, [0.1, 0.0]).how, "by movement");
        // Facing east, moving north is moving left.
        assert_eq!(local_motion([0.0, 100.0], [1.0, 0.0]), (0.0, -1.0));
    }

    #[test]
    fn a_crippled_leg_stops_the_leap() {
        let both = [(Power::Normal, 0.4), (Power::Forward, 0.6)];
        assert_eq!(no_leaps(&both, false), both.to_vec());
        assert_eq!(no_leaps(&both, true), vec![(Power::Normal, 0.4)]);
        // A creature whose only power attack is the leap makes a plain blow.
        assert_eq!(pick_power(None, &no_leaps(&both[1..], true), 0.0, 0.0, 0.0).0, None);
    }

    #[test]
    fn melee_cadence_waits_for_late_keys_and_keyless_ends() {
        use crate::attack_anims::SwingClip;
        use fnv_combat::melee::NextAttack;
        // Synthetic ordinary, power and keyless clips [design, #2535], published at their
        // own speed as `attack_anims` publishes them; the swing carries the attack speed
        // (1 for a creature in `fight`; other speeds exercise the clip's clock).
        for (key, len, earliest) in [(Some(1.3), 1.5, 1.3), (Some(1.55), 1.7, 1.55), (None, 1.3, 1.3), (None, 0.8, STRIKE_SECONDS), (Some(0.6), 1.5, 0.6)] {
            for speed in [0.5, 1.0, 2.0] {
                let clip = SwingClip { hit: 0.4, chain: key.map(|at| (at, NextAttack::Left)), len };
                // Clip timing can arrive after a frame or several [design].
                for elapsed in [0.02, 0.15, 0.35] {
                    let base_left = (STRIKE_SECONDS / speed - elapsed).max(0.0);
                    // A creature waits for the clip's key or end.
                    let sw = NpcSwing { t: elapsed, clip: Some(clip), run: 1, power: false, speed, creature: true };
                    let left = sw.ready_in(base_left);
                    assert!((left - (earliest / speed - elapsed).max(0.0)).abs() < 1e-5, "{key:?}, {len}, x{speed}, t={elapsed}: {left}");
                    // A person keeps the base, brought forward only by an earlier key at
                    // the clip's own time (#2971).
                    let person = NpcSwing { creature: false, ..sw };
                    let main = key.map_or(base_left, |at| base_left.min((at - elapsed).max(0.0)));
                    assert_eq!(person.ready_in(base_left), main, "person {key:?}, {len}, x{speed}, t={elapsed}");
                }
            }
        }
        assert_eq!(NpcSwing::default().ready_in(0.7), 0.7);
        let ended = NpcSwing { t: 2.0, clip: Some(SwingClip { hit: 0.4, chain: None, len: 1.5 }), run: 1, power: true, speed: 1.0, creature: true };
        assert_eq!(ended.ready_in(0.0), 0.0);
    }

    /// #2535 review: a creature striking at x2 is not held to its clip's unscaled `a:` key:
    /// the key and the clip's end are divided by the speed, in the manner of the guns' `a:`
    /// key (#2739). A person keeps the base timing and its clip's own times at every speed
    /// (the maintainer's decision of 2026-11-18 on #2971).
    #[test]
    fn melee_cadence_divides_a_creature_clip_by_the_attack_speed_not_a_persons() {
        use crate::attack_anims::SwingClip;
        use fnv_combat::melee::NextAttack;
        // Synthetic clip, published unscaled: Hit 0.30, a:L 0.63, end 1.43 [design].
        let keyed = SwingClip { hit: 0.30, chain: Some((0.63, NextAttack::Left)), len: 1.43 };
        let keyless = SwingClip { chain: None, ..keyed };
        let base = STRIKE_SECONDS / 2.0;
        let swing = |clip, speed, t: f32, creature| NpcSwing { t, clip: Some(clip), run: 1, power: false, speed, creature };
        let at = |clip, speed, t: f32, creature| swing(clip, speed, t, creature).ready_in((STRIKE_SECONDS / speed - t).max(0.0));
        // A creature at x2: the key comes at 0.315 s, not at 0.63 s (the 1x key).
        assert!((at(keyed, 2.0, 0.05, true) - (0.315 - 0.05)).abs() < 1e-5, "{}", at(keyed, 2.0, 0.05, true));
        assert!(at(keyed, 2.0, 0.05, true) < base, "no slower than the speed-scaled base");
        // Keyless at x2: the base (0.6 s) or the clip's end at x2 (0.715 s), the later.
        assert!((at(keyless, 2.0, 0.05, true) - (0.715 - 0.05)).abs() < 1e-5);
        // At x1 the clip's own times.
        assert!((at(keyed, 1.0, 0.05, true) - (0.63 - 0.05)).abs() < 1e-5);
        assert!((at(keyless, 1.0, 0.05, true) - (1.43 - 0.05)).abs() < 1e-5);
        // A person at x2: the base (0.6 s) comes before the 1x key (0.63 s), keyed or not.
        assert_eq!(at(keyed, 2.0, 0.05, false), base - 0.05);
        assert_eq!(at(keyless, 2.0, 0.05, false), base - 0.05);
        // At x1 the key brings the base (1.2 s) forward; keyless keeps it, not the clip's end.
        assert_eq!(at(keyed, 1.0, 0.05, false), 0.63_f32 - 0.05);
        assert_eq!(at(keyless, 1.0, 0.05, false), STRIKE_SECONDS - 0.05);
        // #2535 review r2: a creature's combo window and swing end follow the same speed, so
        // a decision made at the key goes on the combo at every speed, and the window
        // closes at the clip's end ÷ speed.
        for speed in [2.0_f32, 0.8, 1.5, 1.0] {
            let next = |t| swing(keyed, speed, t, true).next();
            // (The frame that finds the wait spent is at or just past it.)
            let decided = 0.05 + at(keyed, speed, 0.05, true) + 1e-4;
            assert_eq!(next(decided), Some(NextAttack::Left), "x{speed}: decided at {decided}");
            assert_eq!(next(0.63 / speed - 0.01), None, "x{speed}: before the key");
            assert_eq!(next(1.43 / speed - 0.01), Some(NextAttack::Left), "x{speed}: before the end");
            assert_eq!(next(1.43 / speed + 0.01), None, "x{speed}: after the end");
            assert!((swing(keyed, speed, 0.0, true).ends_at() - 1.43 / speed).abs() < 1e-6);
            // A person's window and end are the clip's own times at any speed.
            let person = |t| swing(keyed, speed, t, false).next();
            assert_eq!(person(0.62), None, "x{speed}");
            assert_eq!(person(0.63), Some(NextAttack::Left), "x{speed}");
            assert_eq!(person(1.43), Some(NextAttack::Left), "x{speed}");
            assert_eq!(person(1.44), None, "x{speed}");
            assert_eq!(swing(keyed, speed, 0.0, false).ends_at(), 1.43);
        }
        // A power attack never goes on a combo, at any speed.
        assert_eq!(NpcSwing { t: 0.4, clip: Some(keyed), run: 1, power: true, speed: 2.0, creature: true }.next(), None);
    }

    /// The maintainer's decision of 2026-11-18 on #2971: a person's next melee decision
    /// keeps the 1.2 s base however late its clip's `a:` key, and a person's power attack
    /// does not take its published clip (its wait is the clip's length, set at the blow).
    #[test]
    fn a_persons_late_key_keeps_the_base_pace() {
        use crate::attack_anims::SwingClip;
        use fnv_combat::melee::NextAttack;
        // Synthetic late-key clip: a:L at 1.3 s of 1.5 [design].
        let late = SwingClip { hit: 0.4, chain: Some((1.3, NextAttack::Left)), len: 1.5 };
        let mut sw = NpcSwing { t: 0.0, clip: None, run: 1, power: false, speed: 1.0, creature: false };
        let mut strike_in = STRIKE_SECONDS;
        // As `fight` does: count down, then apply the clip once it arrives.
        let dt = 0.05;
        strike_in = (strike_in - dt).max(0.0);
        sw.t += dt;
        assert!(sw.takes_clip());
        sw.clip = Some(late);
        strike_in = sw.ready_in(strike_in);
        assert_eq!(strike_in, STRIKE_SECONDS - dt, "the base, not the key at 1.3 s");
        // The decision falls at 1.2 s, before the key: a new run, not the combo.
        sw.t = STRIKE_SECONDS;
        assert_eq!(sw.next(), None);
        // A creature with the same clip waits for its key.
        let creature = NpcSwing { t: dt, creature: true, ..sw };
        assert!((creature.ready_in(STRIKE_SECONDS - dt) - (1.3 - dt)).abs() < 1e-6);
        // A person's power attack leaves its clip unused; a creature's takes it.
        assert!(!NpcSwing { power: true, ..sw }.takes_clip());
        assert!(NpcSwing { power: true, creature: true, ..sw }.takes_clip());
        assert_eq!(NpcSwing { power: true, clip: None, ..sw }.ends_at(), STRIKE_SECONDS * 2.0);
    }

    #[test]
    fn a_combo_goes_on_between_the_a_key_and_the_clips_end() {
        use fnv_combat::melee::NextAttack;
        // `h2hattackright_a.kf`: Hit 0.30, a:L 0.63, end 1.43 [observed, fnv_combat::melee].
        let clip = crate::attack_anims::SwingClip { hit: 0.30, chain: Some((0.63, NextAttack::Left)), len: 1.43 };
        // People and creatures alike.
        for creature in [false, true] {
            let at = |t| NpcSwing { t, clip: Some(clip), run: 1, power: false, speed: 1.0, creature }.next();
            assert_eq!(at(0.5), None);
            assert_eq!(at(0.63), Some(NextAttack::Left));
            assert_eq!(at(1.4), Some(NextAttack::Left));
            assert_eq!(at(1.5), None);
            // Before its clip started, or a clip without an `a:` key: a new run.
            assert_eq!(NpcSwing { t: 0.7, clip: None, run: 1, power: false, speed: 1.0, creature }.next(), None);
            assert_eq!(NpcSwing { t: 0.7, clip: Some(crate::attack_anims::SwingClip { chain: None, ..clip }), run: 1, power: false, speed: 1.0, creature }.next(), None);
        }
        let power = NpcSwing { t: 0.7, clip: Some(clip), run: 1, power: true, speed: 1.0, creature: true };
        assert_eq!(power.next(), None);
        assert_eq!(power.ready_in(0.5), 0.0);
    }

    #[test]
    fn two_handed_weapons_slow() {
        assert_eq!(pace(false, 0.1), 1.0);
        // A rifle drawn costs fMove2HBigPenalty.
        assert!((pace(true, 0.1) - 0.9).abs() < 1e-6);
    }

    #[test]
    fn guns_engage_within_their_range_and_melee_within_reach() {
        let gun = |max: f32| Some(gun_reach(max, 1.0, 10_000.0, 0.0));
        // It sets off again at the edge of its reach, while it can still fire.
        assert_eq!(engage(None, gun(1000.0)), (800.0, 900.0));
        // A short gun stops at the lower bound when that is within its reach …
        let (stop, resume) = engage(None, gun(480.0));
        assert!((stop - GUN_ENGAGE_MIN).abs() < 1e-4 && (resume - 432.0).abs() < 1e-3);
        // … and never beyond the edge of its reach.
        let (stop, resume) = engage(None, gun(100.0));
        assert!((stop - 90.0).abs() < 1e-4 && (resume - 90.0).abs() < 1e-4);
        assert_eq!(engage(None, gun(10_000.0)), (GUN_ENGAGE_MAX, GUN_ENGAGE_MAX / GUN_RANGE_SHARE));
        assert_eq!(engage(None, Some(gun_reach(0.0, 1.0, 0.0, 0.0))).0, GUN_ENGAGE_MAX);
        let (stop, resume) = engage(Some(100.0), None);
        assert!((stop - 80.0).abs() < 1e-4 && (resume - 100.0).abs() < 1e-4);
    }

    #[test]
    fn a_gun_reaches_its_max_range_and_its_projectile_range() {
        // The flamer: max range 650, its flame goes 640 (FalloutNV.esm WeapFlamer,
        // FlameProjectile [observed]).
        let flamer = GunReach { flame: true, ..gun_reach(650.0, 1.0, 640.0, 0.0) };
        assert_eq!(flamer.reach, Some(640.0));
        assert_eq!(flamer.holds(641.0), Some(RangeHold::Far));
        assert_eq!(flamer.holds(640.0), None);
        // A flame closes in to half its reach (the original's trooper: 164–320).
        assert_eq!(flamer.engage(), (320.0, 576.0));
        // The style's Ranged Weapon Max Range Mult scales the weapon's range.
        assert_eq!(gun_reach(1000.0, 1.5, 10_000.0, 0.0).reach, Some(1500.0));
        // No range in the data: no limit; a projectile range alone limits.
        assert_eq!(gun_reach(0.0, 1.0, 0.0, 0.0).holds(1.0e6), None);
        assert_eq!(gun_reach(0.0, 1.0, 640.0, 0.0).reach, Some(640.0));
    }

    #[test]
    fn firearms_fire_beyond_their_reach_flames_and_explosives_do_not() {
        // The 10mm pistol: Max Range 768, its bullet's range 10,000 (FalloutNV.esm
        // Weap10mmPistol, 10mmBulletProjectile [observed]): it fights from 614 units but
        // fires at a target 2,000 away (playtest list 7 item 40).
        let pistol = gun_reach(768.0, 1.0, 10_000.0, 0.0);
        let (stop, resume) = pistol.engage();
        assert!((stop - 614.4).abs() < 0.01 && (resume - 691.2).abs() < 0.01);
        assert!(!pistol.limited());
        assert_eq!(pistol.holds(2000.0), None);
        assert_eq!(pistol.holds(10_001.0), Some(RangeHold::Far));
        // A missile launcher keeps its reach for its shots (blast 1,000).
        let missile = gun_reach(1536.0, 1.0, 10_000.0, 1000.0);
        assert!(missile.limited());
        assert_eq!(missile.holds(1600.0), Some(RangeHold::Far));
        assert_eq!(missile.holds(900.0), Some(RangeHold::Blast));
        assert_eq!(missile.holds(1200.0), None);
    }

    #[test]
    fn creatures_attack_on_the_run_within_the_ground_they_cover() {
        // Standing or walking slowly: no blow before it is within reach.
        assert_eq!(lunge_band(50.0, 0.5), 0.0);
        // A ghoul running in at 400 units/s with a hit 0.4 s into its clip begins 160
        // units out; never more than the cap.
        assert!((lunge_band(400.0, 0.4) - 160.0).abs() < 1e-3);
        assert_eq!(lunge_band(2000.0, 1.0), RUN_ATTACK_MAX);
        // Gaining on a target: its own speed towards it less the target's away from it.
        let east = [100.0, 0.0];
        assert_eq!(closing_speed([955.0, 0.0], [0.0, 0.0], east, 1050.0), 955.0);
        assert_eq!(closing_speed([955.0, 0.0], [272.8, 0.0], east, 1050.0), 955.0 - 272.8);
        // A target coming at it is gained on faster; one moving across, as if it stood.
        assert_eq!(closing_speed([300.0, 0.0], [-100.0, 0.0], east, 330.0), 400.0);
        assert_eq!(closing_speed([300.0, 0.0], [0.0, 250.0], east, 330.0), 300.0);
        // Its own speed is capped (a MoveTo's step), the target's is not added to the cap.
        assert_eq!(closing_speed([5000.0, 0.0], [200.0, 0.0], east, 1050.0), 850.0);
        // A person as fast as its target gains nothing: no blow begins short of its reach.
        assert_eq!(lunge_band(closing_speed([272.8, 0.0], [272.8, 0.0], east, 300.0), 0.4), 0.0);
    }

    #[test]
    fn shotguns_close_in_rifles_hang_back() {
        // Max ranges of FalloutNV.esm [observed]: hunting shotgun 600, sniper rifle 3548.
        let shotgun = gun_reach(600.0, 1.0, 10_000.0, 0.0).engage();
        let sniper = gun_reach(3548.0, 1.0, 53_000.0, 0.0).engage();
        assert_eq!(shotgun, (480.0, 540.0));
        assert_eq!(sniper, (GUN_ENGAGE_MAX, GUN_ENGAGE_MAX / GUN_RANGE_SHARE));
        assert!(sniper.1 <= 3548.0);
    }

    #[test]
    fn an_aim_arc_sets_the_firing_angle() {
        assert_eq!(fire_facing(0.0), FIRE_FACING);
        assert!((fire_facing(90.0) - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
    }

    #[test]
    fn dodges_and_runs_from_grenades() {
        use fnv_game::combat_style::DodgeDir;
        // Facing north to a target 200 units away: right is east, left west, back south.
        let (feet, tfeet) = ([0.0, 0.0, 10.0], [0.0, 200.0, 10.0]);
        let near = |a: Option<V3>, b: V3| a.is_some_and(|a| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-3));
        assert!(near(dodge_point(feet, tfeet, DodgeDir::Right, 60.0, 100.0), [60.0, 0.0, 10.0]));
        assert!(near(dodge_point(feet, tfeet, DodgeDir::Left, 60.0, 100.0), [-60.0, 0.0, 10.0]));
        assert!(near(dodge_point(feet, tfeet, DodgeDir::Back, 60.0, 100.0), [0.0, -60.0, 10.0]));
        // Forward stops at the stop distance: 200 − 150 = 50; nearer than 32, no dodge.
        assert!(near(dodge_point(feet, tfeet, DodgeDir::Forward, 90.0, 150.0), [0.0, 50.0, 10.0]));
        assert_eq!(dodge_point(feet, tfeet, DodgeDir::Forward, 90.0, 180.0), None);
        assert_eq!(dodge_point(feet, tfeet, DodgeDir::Back, 20.0, 100.0), None);
        // From a grenade 100 units south-west, radius 256: 320 units away from it.
        let g = [-60.0, -80.0, 0.0];
        let at = evade_point(feet, g, 256.0);
        assert!(((at[0] - g[0]).hypot(at[1] - g[1]) - 320.0).abs() < 1e-3 && at[0] > 0.0 && at[1] > 0.0 && at[2] == 10.0);
        // On top of it: east.
        assert!(near(Some(evade_point(feet, [0.0, 0.0, 0.0], 100.0)), [164.0, 0.0, 10.0]));
    }

    #[test]
    fn launchers_keep_clear_of_their_blast() {
        // Missile launcher: max range 1536, explosion radius 1000; Fat Man 2000 and 1700
        // (FalloutNV.esm [observed]).
        let missile = gun_reach(1536.0, 1.0, 10_000.0, 1000.0);
        assert_eq!(missile.holds(900.0), Some(RangeHold::Blast));
        assert_eq!(missile.holds(1100.0), None);
        // An area weapon stops just clear of its blast, not at 80 % of its reach.
        let (stop, resume) = missile.engage();
        assert_eq!(stop, 1000.0 + BLAST_MARGIN);
        assert!(resume <= 1536.0 * GUN_REACH_EDGE && resume > stop);
        // A small blast (the Gauss rifle's 40 of 2,000) changes nothing.
        assert_eq!(gun_reach(2000.0, 1.0, 60_000.0, 40.0).engage(), (1600.0, 1800.0));
        let fatman = gun_reach(2000.0, 1.0, 10_000.0, 1700.0);
        let (stop, resume) = fatman.engage();
        assert_eq!(stop, 1700.0 + BLAST_MARGIN);
        assert_eq!(resume, 1800.0);
        // A blast wider than the reach: it stops at the edge of its reach.
        assert_eq!(gun_reach(1000.0, 1.0, 10_000.0, 1500.0).engage().0, 900.0);
    }

    /// #2351: a gun's step back (`combat_tactics::back_off_band`) ends no farther than where
    /// it stops closing in (`GunReach::engage`), so it does not go back and forth between
    /// the two; short-reach guns step back from nearer, by their Min Range.
    #[test]
    fn short_reach_guns_step_back_short_of_their_stop() {
        let band = |r: &GunReach, min_range: f32| tactics::back_off_band(r.clearance(), Some(r.engage().0), min_range);
        // The bloatfly's dart gun: Max Range 250, Min Range 100, BloatflyDart range 10,000,
        // no explosion; CSBloatfly's range mults 1.0 (FalloutNV.esm Bloatfly1HP 0x00046BDD,
        // 0x0004741A, 0x0004713F [documented: fnvtool esm decode]). It stops at 225 and
        // steps back only from a target within its Min Range, to 164.
        let dart = gun_reach(250.0, 1.0, 10_000.0, 0.0);
        assert!((dart.engage().0 - 225.0).abs() < 1e-3);
        let (within, until) = band(&dart, 100.0);
        assert!((within - 100.0).abs() < 1e-3 && (until - 164.0).abs() < 1e-3);
        // Without a Min Range: the stop less the slack, on to the stop.
        let (within, until) = band(&dart, 0.0);
        assert!((within - (225.0 - tactics::BACK_OFF_SLACK)).abs() < 1e-3 && (until - 225.0).abs() < 1e-3);
        // A Min Range beyond the room left is held to it.
        let (within, until) = band(&dart, 500.0);
        assert!((within - (225.0 - tactics::BACK_OFF_SLACK)).abs() < 1e-3 && (until - 225.0).abs() < 1e-3);
        // Guns whose stop leaves room keep the 256 / 320 band whatever their Min Range: the
        // 10mm pistol (Max Range 768) and a rifle with Min Range 768 (§12.6).
        let pistol = gun_reach(768.0, 1.0, 10_000.0, 0.0);
        let keep = (tactics::BACK_OFF_WITHIN, tactics::BACK_OFF_WITHIN + tactics::BACK_OFF_SLACK);
        assert_eq!(band(&pistol, 0.0), keep);
        assert_eq!(band(&gun_reach(3000.0, 1.0, 10_000.0, 0.0), 768.0), keep);
        // No reach in its data: the band, whatever its stop.
        assert_eq!(tactics::back_off_band(0.0, None, 100.0), keep);
        // Every reach, Min Range and blast: it steps back from nearer than it goes on to,
        // never past its stop, and never nearer than its blast's clearance.
        for reach in (20..4000).step_by(10) {
            for min_range in [0.0, 50.0, 100.0, 300.0, 768.0] {
                for blast in [0.0, 40.0, 200.0, 1000.0] {
                    let g = gun_reach(reach as f32, 1.0, 10_000.0, blast);
                    let (stop, _) = g.engage();
                    let (within, until) = band(&g, min_range);
                    assert!(within <= until, "reach {reach} min {min_range} blast {blast}: {within} > {until}");
                    assert!(within >= g.clearance().min(stop) - 1e-3, "reach {reach} blast {blast}: within {within}");
                    if g.clearance() <= stop {
                        assert!(until <= stop + 1e-3, "reach {reach} min {min_range} blast {blast}: until {until} beyond stop {stop}");
                    }
                }
            }
        }
    }

    #[test]
    fn sights_beyond_twenty_metres_hip_within() {
        assert!(!aims_down_sights(false, 1300.0, 55.0));
        assert!(aims_down_sights(false, 1500.0, 55.0));
        // Up, they stay up a little nearer; down, they come up only beyond 20 m.
        assert!(aims_down_sights(true, 1380.0, 55.0));
        assert!(!aims_down_sights(false, 1380.0, 55.0));
        assert!(!aims_down_sights(true, 1300.0, 55.0));
        // A weapon without sights never.
        assert!(!aims_down_sights(false, 3000.0, 0.0));
    }

    #[test]
    fn the_aim_settles() {
        assert_eq!(settling(0.0), SETTLE_DEGREES);
        assert!((settling(SETTLE_SECONDS / 2.0) - SETTLE_DEGREES / 2.0).abs() < 1e-5);
        assert_eq!(settling(10.0), 0.0);
    }

    /// The actor spread of an NPC shot moving so (#2125), the weapon, skill, Strength,
    /// sights and limbs held.
    fn moving_spread(locomotion: Option<([f32; 2], f32, fnv_actor::locomotion::Move)>, aiming: bool, limbs: [bool; 3], wobble_off: bool) -> f32 {
        let (r, t) = (fnv_combat::tuning::Rules::defaults(), fnv_combat::tuning::CombatTuning::default());
        let ws = WeaponStats::default();
        let st = shot_stance(aiming, false, shot_gait(locomotion), 50.0, 5.0, limbs);
        shot_spread(&r, &t, &ws, &st, wobble_off)
    }

    #[test]
    fn an_npc_shot_spread_takes_its_most_recent_walk_or_run() {
        use fnv_actor::locomotion::Move as Clip;
        let r = fnv_combat::tuning::Rules::defaults();
        // The skill's share of the sum (skill 50).
        let share = 1.0 - r.wobble_to_skill * 0.5;
        let (walk, run) = (share * r.spread_walking, share * r.spread_running);
        assert!(walk > 0.0 && run > walk);
        for aiming in [false, true] {
            for limbs in [[false; 3], [true, false, true]] {
                let spread = |lo| moving_spread(lo, aiming, limbs, false);
                let standing = spread(Some(([0.0; 2], 1.0, Clip::Idle)));
                let near = |a: f32, b: f32| (a - b).abs() < 1e-5;
                // No locomotion, a creep below a step, or a velocity of nought while the
                // run clip still plays: standing.
                assert!(near(spread(None), standing));
                assert!(near(spread(Some(([0.0, 3.0], 1.0, Clip::Forward))), standing));
                assert!(near(spread(Some(([0.0, 8.0], 2.0, Clip::Forward))), standing), "a step judged at the clips' pace");
                assert!(near(spread(Some(([0.0; 2], 1.0, Clip::FastForward))), standing), "no velocity, the run clip still playing");
                // Walking, forward, backward and sideways.
                for (v, mv) in [([0.0, 120.0], Clip::Forward), ([0.0, -110.0], Clip::Backward), ([150.0, 0.0], Clip::Right), ([-90.0, 40.0], Clip::Left)] {
                    assert!(near(spread(Some((v, 1.0, mv))) - standing, walk), "walking {v:?} {mv:?}");
                }
                // Running, forward, backward and sideways.
                for (v, mv) in [([0.0, 380.0], Clip::FastForward), ([0.0, -300.0], Clip::FastBackward), ([360.0, 0.0], Clip::FastRight), ([-340.0, -20.0], Clip::FastLeft)] {
                    assert!(near(spread(Some((v, 1.0, mv))) - standing, run), "running {v:?} {mv:?}");
                }
                // A slow runner (a creature whose clips play at 0.4, slower in world units
                // than any fixed running cut-off) runs; a fast creature at its walk walks.
                assert!(near(spread(Some(([0.0, 100.0], 0.4, Clip::FastForward))) - standing, run));
                assert!(near(spread(Some(([0.0, 600.0], 2.5, Clip::Forward))) - standing, walk));
                // The run clip playing while it is still is no run; a walk clip at speed
                // (a set with no run clip) is a walk.
                assert!(near(spread(Some(([0.0, 500.0], 1.0, Clip::Idle))) - standing, walk));
                assert!(near(spread(Some(([0.0, 2.0], 1.0, Clip::FastForward))), standing));
                // SetNoGunWobble: no actor spread whatever the gait.
                assert_eq!(moving_spread(Some(([0.0, 380.0], 1.0, Clip::FastForward)), aiming, limbs, true), 0.0);
            }
        }
    }

    #[test]
    fn an_npc_shot_stance_keeps_its_sights_crouch_and_limbs() {
        let gait = shot_gait(Some(([0.0, 120.0], 1.0, fnv_actor::locomotion::Move::Forward)));
        assert_eq!(gait, ShotGait { moving: true, running: false });
        let st = shot_stance(true, true, gait, 62.0, 7.0, [true, false, true]);
        assert!(st.aiming && st.sneaking && st.moving && !st.running && !st.airborne);
        assert!(st.left_arm_crippled && !st.right_arm_crippled && st.head_crippled);
        assert_eq!((st.skill, st.strength, st.aiming_move_divisor, st.strength_req_cut), (62.0, 7.0, 1.0, 0.0));
        assert_eq!(shot_gait(None), ShotGait::default());
    }
}
