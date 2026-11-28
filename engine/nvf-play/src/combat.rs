//! The player's attacks with ranged weapons (docs/design/combat.md step C4), on the
//! headless rules of `fnv-combat` (docs/spec-notes/combat-rules.md):
//!
//! - **Weapon and ammo.** The equipped weapon (simulation) and its loaded ammo: the
//!   first of the weapon's ammo list the player carries (`CombatForms::ammo_for`). The
//!   magazine is part of the carried count, as rounds in the inventory; a shot removes
//!   `ammo_use` rounds from the inventory. A weapon with no ammo of its own but a
//!   projectile (grenades, thrown weapons) spends itself. A newly equipped weapon starts
//!   loaded with what is carried [design].
//! - **The cycle** (`fnv_combat::attack::Cycle`): Use is the trigger, a Ready Item tap
//!   reloads, Ammo Swap moves down the ammo list and reloads [design]. It runs only once
//!   the player body says the weapon is out ([`WeaponAnim::ready`]); the body plays the
//!   attack and reload clips when [`WeaponAnim::attacks`] / [`WeaponAnim::reloads`] count
//!   up and gives their timing back.
//! - **Shots** leave the eye along the view, inside the spread cone (the documented New
//!   Vegas spread plus bloom, which builds while shots follow each other and shrinks once
//!   the weapon rests); recoil climbs the view up in a random pattern and stays there
//!   for the player to pull down (`fnv_combat::spread::Kick`) [design]. They are
//!   drawn from the muzzle of the held weapon ([`WeaponAnim::muzzle`], the model's
//!   `ProjectileNode`), closing on the view ray by the point aimed at; in third person a
//!   shot with something between the muzzle and that point leaves the muzzle instead
//!   (`fnv_combat::shot::leaving`; docs/design/combat.md §5.1).
//! - **Projectiles** fly through the physics world (`fnv_phys` rays; actor hit volumes,
//!   combat C3).
//! - **Melee and unarmed** (step C6, `fnv_combat::melee`): a tap of Use swings, and taps
//!   during a swing chain the next at the clip's `a:` key (right, left, right… by the
//!   keys); holding it [`CombatTuning::power_hold`] makes a power attack whose clip
//!   follows the movement keys (standing, forward, back, left, right) and whose damage is
//!   that direction's `fDamagePowerAttack…Bonus`. The body plays [`WeaponAnim::swing`]
//!   and times it ([`WeaponAnim::timed`]); the swing strikes once at the clip's `Hit` key
//!   [documented: NiTextKeyExtraData] with a fan of rays from the eye out to the reach
//!   (`damage::melee_reach`, which reaches from the body: plus the player's capsule
//!   radius from the eye [hypothesis, as the NPCs' 45 + 107 × Reach is read,
//!   combat-rules.md §10]), and at any actor whose centre is within that reach across
//!   the ground, low or high (`projectile::swing_reaching`: a gecko at the player's feet). With no weapon equipped the player fights with the
//!   Fists (WEAP `Fists`) [hypothesis: the game's unarmed weapon form]. Knockback force
//!   is computed (documented formula) and logged; living actors are not pushed until they
//!   move physically. Every shot and blow that lands on an actor also sends an
//!   [`ActorPush`], which shoves a dead actor's ragdoll [design].
//! - **Hits** (step C5): the damage of `fnv_combat::damage` against the target's worn
//!   armour (`CombatForms::worn_armor` over its inventory) goes to the script runtime's
//!   `Game::hit` (Health's damage part, OnHit/OnHitWith, death and OnDeath, kill XP), so
//!   scripts see what combat did. Explosions hit with location −1. Each hit sends an
//!   [`ActorHit`] (nvforever plays a hit reaction, when the hit cripples a body part or
//!   lands on a crippled one, or the death pose). Without the script runtime
//!   (`NVF_PLAY_SCRIPTS=0`) hits are only logged.
//! - **Limb hits** (step C8): an actor whose hit volume has body parts
//!   (`crate::hitvol::ActorParts`) is hit on a part of its BPTD: the part's damage
//!   multiplier, its location for `GetHitLocation`, and limb damage (damage before the
//!   part multiplier times the weapon's Limb Dmg Mult) on its condition (`Game::hit`),
//!   which cripples the limb at 0. Player shots and blows select that part from the
//!   single capsule's hit point (`player_body_part`); blows keep their existing Health
//!   multiplier [hypothesis: melee is open, combat-rules.md §12]. Explosions hit no part
//!   (location −1) [design]. Crippled limbs widen the
//!   spread (the documented crippled-arm terms, the head's), weaken melee
//!   (`Attacker::arm_condition`), slow movement by the game settings, make the torso
//!   flinch from any hit ([`player_flinch`]), sway the aim ([`arm_sway`]) and disarm
//!   NPCs ([`disarm`]) (docs/spec-notes/crippled-limbs.md); the HUD lists the player's.
//! - **Explosives and destructibles** (step C7): grenades and other thrown weapons are
//!   lobbed projectiles; mines arm and wait for an actor; C-4 waits for
//!   `DetonatePlacedExplosives`, which the detonator's OnFire script calls (weapons' OnFire
//!   blocks run on each shot). Explosions hurt actors (armour, knockdown by the EXPL flags
//!   and the documented formula, logged), damage destructible objects in their radius and
//!   push loose objects. Bullets damage destructibles they hit. Destruction stages
//!   (`Game::damage_object`, as `DamageObject`) disable, destroy, burn down by their
//!   self-damage and set off their explosions, which can set off others.
//! - **Shots on objects** (combat-rules.md §14): a shot that lands on a placed object
//!   that is not an actor runs the object's OnHit and OnHitWith blocks
//!   (`Game::hit_object`: the tutorial's bottles count the player's shots there) and
//!   strikes its loose body with the projectile's impact force ([`OBJECT_PUSH`]), so
//!   bottles and cans are knocked away [design]. The scripts are told when the player's
//!   weapon is out (`IsWeaponOut`): [`WeaponAnim::ready`].
//! - **What is drawn of a shot** (docs/spec-notes/combat-rules.md §15): each shot sends
//!   a [`ShotFired`] (the renderer shows the PROJ's muzzle flash) and every projectile
//!   in flight is listed in [`ShotVisuals`] (the renderer draws the PROJ's model there).
//!   A hitscan bullet, whose hit is found at once, is drawn flying from the muzzle to
//!   where it lands at the PROJ's speed; a beam is drawn from the muzzle to its end for
//!   the PROJ's fade duration [design].
//! - **Sounds and impacts** (docs/spec-notes/combat-rules.md §16): each shot plays the
//!   weapon's Shoot 2D sound (else its Shoot 3D), an empty trigger its No Ammo sound, and
//!   drawing and putting it away its Equip and Unequip sounds (`crate::sfx`). A shot
//!   that lands sends an [`ImpactFx`] with the IPCT its weapon's impact data set names
//!   for the surface's material, and plays that impact's sounds there; an explosion
//!   sends an [`ExplosionFx`] and plays the EXPL's sounds.
//! - **Item condition** (docs/spec-notes/combat-rules.md §17, `fnv_combat::condition`):
//!   each shot, and each swing that lands, wears the player's weapon; a hit wears the
//!   armour the target wears where it lands (head or body; an explosion both); a hit on
//!   the weapon an actor holds ([`crate::hitvol::WEAPON_PART`]) takes its damage from
//!   that weapon's condition and none from the actor. The player's weapon at zero is
//!   unequipped and stays in the inventory, broken; another actor's falls to the ground
//!   ([`WeaponBroken`]). A worn weapon hits for less and worn armour protects less.
//! - **Other actors' attacks** (combat AI, docs/design/combat-ai.md): nvforever's
//!   `npc_combat` decides when an actor fires or strikes and sends [`ActorFire`] or
//!   [`ActorStrike`]; [`actor_attacks`] turns them into projectiles and hits through the
//!   same rules as the player's. Every projectile knows who fired it, with which weapon
//!   and ammunition ([`By`]), so its hit is that actor's. Shots fired by other actors
//!   can hit the player's character [design: the player has no hit volume of its own;
//!   a hit counts as the torso]. Actors fire without spending ammunition [design: until
//!   looting balance is looked at].
//! - **The player's death**: at 0 health the player dies (`Game::hit`); every control
//!   is held from then on (`ScriptState::locks`) and the screen says so; the death
//!   camera (`nvforever` `death_cam.rs`) drops the body, slows the clock and blurs the
//!   picture; `save::death_recovery` asks the existing loader for the newest eligible save. God mode ([`god_mode`]:
//!   `--god`, the settings page's Developer switch, test aid `NVF_PLAY_TEST_GOD=1`): the
//!   player takes no damage, so a long unattended run is not ended by a fight.
//! - **Feedback** [design]: no hit marker at the crosshair (owner 2026-10-27), the last
//!   target's health for a few seconds, the player's
//!   crippled limbs, and a line for kills with their XP. Health, condition and ammo are
//!   on the HUD proper (`crate::hud`, [`Combat::hud_weapon`]). Developer markers, off
//!   unless [`HitMarks`] says so: a cross at each impact and a line from the muzzle
//!   along each hitscan shot.

use fnv_util::words::tr;
use bevy::prelude::*;
use fnv_combat::attack::{ClipTiming, Cycle, Event, Input, Phase, Timing};
use fnv_combat::condition::{self, Wear, Zone};
use fnv_combat::damage::{self, Attacker, AttackKind, HitInput, HitPart, NoPerks, Target};
use fnv_combat::melee::{Melee, MeleeEvent, MeleeInput, SwingTiming};
use fnv_combat::data::AmmoEffect;
use fnv_combat::projectile::{blast, ActorId, CombatWorld, Motion, ProjEvent, Projectile, WorldHit};
use fnv_combat::rng::SplitMix;
use fnv_combat::shot::Seen;
use fnv_combat::spread::{self, Kick, Stance};
use fnv_combat::vec::{self, V3};
use fnv_combat::{shot, CombatForms, CombatTuning, Rules};
use fnv_phys::{layers, Filter, Key, Owner, Queries};
use fnv_sim::FormId;
use std::collections::{HashMap, HashSet};

use crate::activate::HudMessage;
use crate::sfx::{LoopSound, PlaySound, ScaledSound, Sfx};
use crate::hitvol::{ActorParts, WEAPON_PART};
use fnv_combat::data::{BodyPart, WeaponStats};
use fnv_game::combat::LimbHit;
use crate::control::PlayerControl;
use crate::script::{with_world, ScriptState};
use fnv_game::combat::{CombatRequest, DestructionChange, HitInfo, HitResult};
use crate::phys::PhysState;
use crate::player::{MoveIntent, Player, MAX_PITCH};
use crate::sim::SimState;
use crate::{space, PlaySet, PlayState, PlayWorld};

pub(crate) mod hit_effects;
pub mod held_grenade_probe;
mod projectile_launch;
mod projectile_launch_probe;
#[cfg(test)]
mod projectile_launch_tests;
#[cfg(test)]
mod critical_damage_hit_tests;
#[cfg(test)]
mod world_blood_tests;
#[cfg(test)]
mod weapon_sound_bridge_tests;
#[cfg(test)]
mod bullet_snap_tests;
#[cfg(test)]
mod showoff_combat_tests;

/// The shooter's id for projectiles (the player has no hit volume).
const PLAYER: ActorId = u64::MAX;

/// What the player body and the combat systems tell each other.
#[derive(Resource, Default, Debug, Clone)]
pub struct WeaponAnim {
    /// Set by the body: the weapon is drawn and in hand.
    pub ready: bool,
    /// Set by combat: the weapon's attack and reload animation names (`AttackRight`,
    /// `ReloadL`), for the body's clip choice.
    pub attack: Option<String>,
    pub reload: Option<String>,
    /// Set by the body: the attack and reload clips' timing in the rig in view.
    pub timing: ClipTiming,
    /// Counted up by combat when an attack or a reload starts; the body restarts the
    /// clip when it sees a new count.
    pub attacks: u32,
    pub reloads: u32,
    /// Set by combat: how fast the reload clips play, a multiple of their own speed
    /// (Agility and the Reload Speed perks, `fnv_combat::attack::reload_rate`); the cycle
    /// takes the same share of time. 0 before combat has set it: read as 1.
    pub reload_rate: f32,
    /// Set by combat: how fast the attack clip plays, a multiple of its own speed: the
    /// weapon's Animation and Animation Attack multipliers and the Modify Attack Speed
    /// perks; the shot cycle takes the same share of time. 0 before combat has set it.
    pub attack_rate: f32,
    /// Set by combat: how fast the draw and holster clips play (the Equip Speed perks:
    /// Quick Draw); the draw and holster take the same share of time. 0: read as 1.
    pub equip_rate: f32,
    /// Counted up by combat when a round-by-round reload starts on a round: the body
    /// plays the reload clip from its start (a reload count plays the opening clip).
    pub reload_rounds: u32,
    /// Counted up by combat when a weapon starts to spin up: the body plays the attack
    /// clip from its start, and the first shot follows at the clip's `Fire` key.
    pub spin_ups: u32,
    /// Counted up by combat when a pull starts a shot that leaves later, at the attack
    /// clip's `Hit` key (a thrown weapon's release, a gun's shot after its wind-up): the
    /// body plays the attack clip from its start then, and the shot that follows does not
    /// start it again (the owner's playtest list 7 item 22: the clip used to start at the
    /// shot, so dynamite waited, left, and only then was lit and thrown).
    pub wind_ups: u32,
    /// Set by combat: the player holds a thrown weapon at its release under the Hold and
    /// Release Throwables tweak; the body holds the attack clip at its `Hit` key
    /// [design: docs/design/tweaks.md].
    pub throw_held: bool,
    /// Set by combat: the weapon's jam animation (`JamA` for reload animation
    /// `ReloadA`: the game's `1hpjama.kf` beside `1hpreloada.kf` [observed: file lists]),
    /// and counted up when a shot jams the weapon; the body plays the jam clip.
    pub jam: Option<String>,
    pub jams: u32,
    /// Set by combat: the clip stem of the melee swing playing (`attackleft_b`,
    /// `attackforwardpower`; `fnv_combat::melee`), for the body's clip choice; `None`:
    /// the weapon's attack animation.
    pub swing: Option<String>,
    /// Set by the body: the attack stem whose clip [`Self::timing`] was read from.
    pub timed: Option<String>,
    /// Set by the body: the weapon is being drawn, or put away (GetAnimAction 0 and 1).
    pub drawing: bool,
    pub holstering: bool,
    /// Set by the body each frame: where the muzzle of the weapon in hand is, in the rig
    /// in view (game space): the weapon model's `ProjectileNode` [hypothesis: that the
    /// node marks where projectiles and the muzzle flash start, from its name and its
    /// place at the end of the barrel; docs/spec-notes/animation.md §9.9]. `None`
    /// without a body, a weapon in hand or such a node: shots are drawn from the eye.
    pub muzzle: Option<[f32; 3]>,
    /// Set by the body each frame: where the weapon's `ShellCasingNode` is in the rig
    /// in view and the way a casing leaves it (unit), game space.
    pub casing: Option<([f32; 3], [f32; 3])>,
    /// Set by the body: the weapon's attack clip has an `Eject` key, so its casings
    /// leave when the clip reaches it (the action worked), not at the shot.
    pub ejects_on_key: bool,
    /// Counted up by the body when the attack clip of the rig in view passes its
    /// `Eject` key; combat throws a casing from [`Self::casing`] then.
    pub ejects: u32,
}

/// The player fired a shot this frame: the PROJ it fires. The renderer shows the
/// PROJ's muzzle flash at the weapon's muzzle (nvforever `shot_fx`).
#[derive(Message, Clone, Copy, Debug)]
pub struct ShotFired {
    pub projectile: FormId,
    /// The weapon that fired it (nvforever `detection` hears its sound level).
    pub weapon: FormId,
    /// The mod flags of the unit that fired it, as they were at the shot (`Extra::mods`;
    /// slots 1-3 as `fnv_combat::data::mod_flag`): detection hears the modded weapon
    /// (`WeaponStats::modded`), a Silence mod's Silent level (weapon-mods.md §2, §7).
    pub mods: u8,
    /// Who fired it.
    pub shooter: FormId,
    /// Where it left, for a shot of an actor other than the player (the player's
    /// flash is at the held weapon's muzzle).
    pub at: Option<V3>,
    /// The way it left (unit, game space), with `at`: an actor's flash without a muzzle
    /// marker is turned along it (nvforever `shot_fx`).
    pub dir: Option<V3>,
}

/// An actor other than the player fires its weapon once (combat AI, nvforever
/// `npc_combat`): aimed from its eyes (`origin`) along `aim` (a unit vector), inside a
/// cone of `cone` degrees (its spread), drawn from its weapon's `muzzle` when it has one
/// (as the player's first-person shots, `shot::leaving` without cover: the shot hits
/// what the eyes see, so an actor fires over low cover its barrel is behind) [design].
#[derive(Message, Clone, Copy, Debug)]
pub struct ActorFire {
    pub shooter: FormId,
    pub weapon: FormId,
    pub origin: V3,
    pub aim: V3,
    pub cone: f32,
    pub muzzle: Option<V3>,
    /// The shot does no damage (a weapon use's Do No Damage, nvforever `use_weapon`):
    /// it flies, strikes and is seen and heard as any other.
    pub harmless: bool,
    /// The actor an Always Hit weapon use fires at (nvforever `use_weapon`): the shot
    /// passes every other actor's body and strikes this one, or the world [design: issue
    /// #2738, Heck Gunderson's bodyguard stood in the Securitron's line and took
    /// the six Do No Damage shots whose OnHit is to kill Heck; hypothesis: the original's
    /// "always hit" strikes the target named whoever stands between, not captured].
    pub sure: Option<FormId>,
}

/// An actor other than the player strikes `target` within its reach (combat AI): with
/// its melee weapon, else unarmed (the Fists), or, a creature, with its own attack
/// damage (CREA DATA Damage).
#[derive(Message, Clone, Copy, Debug)]
pub struct ActorStrike {
    pub attacker: FormId,
    pub target: FormId,
    pub weapon: Option<FormId>,
    pub creature_damage: Option<f32>,
    /// Where the blow lands and which way it goes (game space).
    pub point: V3,
    pub dir: V3,
    /// A power attack's attack group ("AttackForwardPower"), for an unarmed attack effect
    /// that names it and, a person's, its damage multiplier; `None`: an ordinary blow.
    pub anim: Option<&'static str>,
}

/// A melee or unarmed blow a blocker stopped part of (docs/design/combat-ai.md §14.3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockedBlow {
    pub attacker: FormId,
    pub blocker: FormId,
    /// Damage before and after the block.
    pub damage: f32,
    pub dealt: f32,
}

/// Who fired a projectile: the actor, its weapon and the ammunition it was loaded with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct By {
    pub actor: FormId,
    pub weapon: FormId,
    pub ammo: Option<FormId>,
    /// It does no damage ([`ActorFire::harmless`]).
    pub harmless: bool,
    /// The only actor it can strike ([`ActorFire::sure`]).
    pub sure: Option<FormId>,
    /// A marked shot of VATS mode (`crate::vats_mode`): its hit counts as a VATS attack
    /// (the critical chance bonus, docs/design/vats-mode.md §9.2).
    pub vats: bool,
    /// The player was sneaking as it left: its hit may be a sneak attack
    /// ([`sneak_attack`]).
    pub sneaking: bool,
}

/// Debris a destroyed object throws (a destruction stage's DEBR and count) from `at`
/// (docs/spec-notes/destruction-stages.md §4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObjectDebris {
    pub reference: FormId,
    pub at: V3,
    pub debris: FormId,
    pub count: i32,
}

/// Who set an explosion off: the actor whose shot (or thrown or placed explosive) it came
/// from and that weapon. Its blast hits are that actor's hits with that weapon, as a
/// bullet's are (OnHit's attacker, kill credit and XP, combat-rules.md §11); `None` for
/// one nobody set off this frame (a destruction stage burning down) [design].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Cause {
    actor: FormId,
    weapon: FormId,
}

impl Cause {
    fn of(by: &By) -> Cause {
        Cause { actor: by.actor, weapon: by.weapon }
    }
}

/// The hit an explosion's blast makes on an actor: its cause's, with no place on the body
/// (GetHitLocation −1) and no critical.
/// A weapon hit's object effects with an area reach the actors around the one hit
/// (`hit_effects::spread`); a blast's do not.
fn spread_hit_areas(script: Option<&mut ScriptState>, sim: &mut SimState, stats: &mut CombatStats, target: FormId, items: &[FormId], attacker: Option<FormId>) {
    if items.is_empty() {
        return;
    }
    let Some(rt) = script.and_then(|s| s.rt.as_mut()) else { return };
    with_world(rt, sim, |rt| hit_effects::spread(&mut rt.game, &mut stats.hit_effects, target, items, attacker));
}

/// The WeaponIneffective and ArmorIneffective lines of a hit (`fnv_game::combat_lines`):
/// what DT made of it.
/// Whether a hit got past the target's DT, told before the hit is dealt so its OnHit
/// block can ask (GetArmourPenetrated, `fnv_game::nvse_jip11`).
fn note_armour(script: Option<&mut ScriptState>, target: FormId, d: &damage::Damage) {
    if let Some(rt) = script.and_then(|s| s.rt.as_mut()) {
        rt.game.note_armour_hit(target, f64::from(d.raw), f64::from(d.dt));
    }
}

pub(crate) fn apply_critical_tweaks(rules: &mut Rules, tweaks: &fnv_game::tweaks::Tweaks) {
    rules.crit_ignores_fire_rate = tweaks.crit_ignores_fire_rate;
    rules.crit_damage_scale = tweaks.crit_damage_scale as f32;
}

/// The weapon damage penalty below 75 % condition: SetConditionDamagePenalty's, else
/// 0.67 (`fnv_game::Game::condition_damage_penalty`).
fn sync_condition_penalty(c: &mut Combat, script: Option<&ScriptState>) {
    if let Some(rt) = script.and_then(|s| s.rt.as_ref()) {
        let t = &rt.game.tweaks;
        // The player's tweaks on the damage rules [design: docs/design/tweaks.md].
        c.rules.condition_damage_penalty = if t.no_condition_damage_penalty { 0.0 } else { rt.game.condition_damage_penalty() };
        c.rules.condition_damage_threshold = if t.no_condition_damage_buffer { 1.0 } else { fnv_combat::damage::CONDITION_DAMAGE_THRESHOLD };
        apply_critical_tweaks(&mut c.rules, t);
        // The Explosion Radius Buff: the inner 30% at full damage, 20% at the edge (the
        // tweak's own numbers as its description gives them).
        c.tuning.explosion_inner = if t.explosion_radius_buff { 0.3 } else { 0.0 };
        c.tuning.explosion_inner_edge = 0.2;
    }
}

/// ext-last-core.md: the same critical-damage override that scripts read must
/// reach every direct hit, including NPC weapons and the player's modified copy.
fn script_weapon_critical_damage(game: Option<&fnv_game::Game<'_>>, weapon: FormId, stats: &mut WeaponStats) {
    if let Some(damage) = game.and_then(|game| game.weapon_critical_damage_override(weapon)) {
        stats.crit_damage = damage;
    }
}

fn armour_lines(script: Option<&mut ScriptState>, attacker: Option<FormId>, target: FormId, d: &damage::Damage, floor: f32) {
    if let Some(rt) = script.and_then(|s| s.rt.as_mut()) {
        rt.game.armour_lines(attacker, target, f64::from(d.raw), f64::from(d.dt), f64::from(d.dealt), f64::from(floor));
    }
}

/// How far before and past an NPC blow's landing point its body part is looked for, game
/// units [design].
const BLOW_PROBE: f32 = 24.0;

fn blast_hit_info(cause: Option<Cause>, explosion_ref: Option<FormId>, base_damage: f64) -> HitInfo {
    HitInfo {
        attacker: cause.map(|c| c.actor),
        weapon: cause.map(|c| c.weapon),
        projectile: explosion_ref,
        location: -1,
        critical: false,
        explosion: true,
        base_weapon_damage: base_damage,
        ..HitInfo::default()
    }
}

/// One projectile to draw this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShotVisual {
    /// The same number for as long as the projectile is drawn.
    pub id: u64,
    /// Its PROJ.
    pub projectile: FormId,
    /// Who fired it.
    pub owner: FormId,
    /// Where it is drawn (game space) and the way it points (unit).
    pub pos: V3,
    pub dir: V3,
    /// A beam: its length from `pos` along `dir`.
    pub beam: Option<f32>,
    /// How far it has flown since it left.
    pub travelled: f32,
    /// How strongly it is drawn, 1 to 0: a beam fades out over its PROJ's Fade Duration
    /// (docs/spec-notes/combat-rules.md §15); everything else is 1.
    pub strength: f32,
}

/// The player's projectiles to draw this frame (nvforever `shot_fx` draws their models).
#[derive(Resource, Default)]
pub struct ShotVisuals {
    pub flying: Vec<ShotVisual>,
    /// Timed projectile crossings and blast disturbances at the water surface this frame.
    pub water_entries: Vec<ShotWaterEntry>,
}

#[derive(Clone, Copy, Debug)]
pub struct ShotWaterEntry {
    pub point: V3,
    pub direction: V3,
    /// The placed water reference, or zero for the continuous cell water.
    pub receiver: u32,
    pub kind: WaterImpact,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WaterImpact {
    Bullet,
    HeavyEntry,
    Blast(SurfaceBlast),
}

/// The source explosion's resolved EXPL radius, signed depth (positive below the
/// surface), and the splash reach used when the surface qualified. All are game units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceBlast {
    pub radius: f32,
    pub depth: f32,
    pub reach: f32,
}

fn water_entry_kind(motion: Motion, explosive: bool) -> Option<WaterImpact> {
    match motion {
        Motion::Missile if !explosive => Some(WaterImpact::Bullet),
        Motion::Missile | Motion::Lobber if explosive => Some(WaterImpact::HeavyEntry),
        _ => None,
    }
}

/// Instant traces can detonate on their hit immediately while the authored water
/// splash still waits for its travel time. Omit just the extra delayed entry ring
/// in that case. Inspect this projectile's step so simultaneous shots stay independent.
fn timed_water_impact(kind: Option<WaterImpact>, delay: f32, detonated_now: bool) -> Option<WaterImpact> {
    kind.filter(|kind| !(*kind == WaterImpact::HeavyEntry && delay > 0.0 && detonated_now))
}

/// A shot threw out a shell casing: the weapon, where the casing starts and its
/// velocity (game space, units per second). The renderer draws the WEAP's casing model.
#[derive(Message, Clone, Copy, Debug)]
pub struct CasingFx {
    pub weapon: FormId,
    pub point: V3,
    pub velocity: V3,
    /// Seconds the casing stays (`fGunShellLifetime`).
    pub life: f32,
}

/// A shot landed: the IPCT of the weapon's impact data set for what it hit, where, the
/// surface's normal and the way the projectile was flying (game space). The renderer
/// shows the impact's effect (nvforever `shot_fx`).
#[derive(Message, Clone, Copy, Debug)]
pub struct ImpactFx {
    pub impact: FormId,
    /// Who fired the projectile (or struck, or set off the explosion); `FormId(0)`: no one.
    pub shooter: FormId,
    pub point: V3,
    pub normal: V3,
    pub dir: V3,
    /// The reference of the actor it struck, if it struck one (its decal goes on the
    /// actor's body, nvforever `decals`).
    pub actor: Option<u32>,
    /// That actor is flagged No Blood Spray / No Blood Decal: the impact's model, or its
    /// decal, is not shown ([`show_impact`], docs/spec-notes/actor-flags.md §2).
    pub no_spray: bool,
    pub no_decal: bool,
    /// Decal only: the blood a shot on an actor leaves on a wall or the floor behind or
    /// below it ([`world_blood`], docs/spec-notes/impact-decals.md §4a). It shows no
    /// model, plays no sound and is heard by no one (its `shooter` is no one).
    pub splatter: bool,
}

/// An explosion went off: its EXPL and where.
#[derive(Message, Clone, Copy, Debug)]
pub struct ExplosionFx {
    pub explosion: FormId,
    pub point: V3,
    /// Who set it off, when known (nvforever `detection` raises its event as theirs).
    pub cause: Option<FormId>,
    /// The normal of the ground under the blast, when there is ground near enough
    /// (`nvforever::shot_fx` aligns the model with it unless the EXPL always uses the
    /// world's orientation).
    pub normal: Option<V3>,
    /// How it is drawn.
    pub look: ExplosionLook,
    /// A body part's own blast as it is blown apart or severed (its BPTD's explodable or
    /// severable explosion, nvforever `dismember`), not a weapon's or a mine's: it shakes
    /// the view only with the Gore Shake setting on (`crate::shake`).
    pub gore: bool,
}

/// How an explosion is drawn (`nvforever::shot_fx`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExplosionLook {
    /// Its EXPL's model and light.
    Own,
    /// Nothing of its own: it went off in or by water, which shows instead.
    Hidden,
    /// The water's surface thrown up ([`WATER_SURFACE_EXPLOSION`]), upright.
    WaterSurface,
}

/// The model of an explosion throwing up water: a column and drops over a spreading
/// ring of foam [observed: an original-game recording of 40mm grenades going off in Lake
/// Mead, 2026-10-25, shows a tall white column over a flat white disc and no fireball;
/// the model's meshes are named WideMesh, TallMesh, BaseMesh and SurfaceFun, with a Drops
/// emitter and the text key "sound: FXExplosionImpactWater"]. No record names it; that
/// this is the model the game drew is a hypothesis from that match.
pub const WATER_SURFACE_EXPLOSION: &str = r"Effects\WaterSurfaceExplosion01.NIF";

/// Every projectile is drawn, not only the PROJ's tracer chance of them [design: the
/// owner asked for bullets that are not invisible, 2026-10-10; the game draws a
/// projectile at its tracer chance, documented: GECK wiki Projectile].
pub const DRAW_EVERY_PROJECTILE: bool = true;

/// An actor was hit (and maybe died) by combat.
#[derive(Message, Clone, Copy, Debug)]
pub struct ActorHit {
    pub form_id: u32,
    pub died: bool,
    pub knocked_out: bool,
    /// Where it was hit (`GetHitLocation`'s numbering; −1 none, 0 torso).
    pub location: i32,
    /// The hit crippled a limb.
    pub crippled: bool,
    /// The body part hit is crippled (by this hit or an earlier one).
    pub part_crippled: bool,
    /// Its torso is crippled: any hit may stagger it (`damage::staggers`).
    pub torso_crippled: bool,
    /// Who hit it.
    pub by: Option<FormId>,
    /// The share of its maximum health the hit took (0 to 1; `crate::rumble`).
    pub share: f32,
    /// A power attack broke through its block (`damage::against_block`): it staggers.
    pub guard_broken: bool,
    /// The weapon of the hit, when known (Meltdown, perk-entry-points.md §9).
    pub weapon: Option<FormId>,
}

/// A hit that forces its target's hit reaction whatever it crippled: Khan Trick
/// [documented: GECK wiki GetForceHitReaction, "set by Khan Trick (AttackCustom4Power and
/// AttackCustom5Power)"; perk-entry-points.md §11.4]. The body plays the reaction for
/// `location` (nvforever `deaths`), without the stagger rule's wait.
#[derive(Message, Debug, Clone, Copy)]
pub struct ForcedHitReaction {
    pub form_id: u32,
    /// Where it was hit (`GetHitLocation`'s numbering).
    pub location: i32,
    /// The move that forced it, for the log.
    pub by: &'static str,
}

/// An actor's weapon left its hand: it reached zero condition and fell (§17), or a hit
/// crippled the actor's arm ([`disarm`]). The renderer takes it out of the hand
/// (nvforever `weapons`); `dropped` is the reference lying on the ground, drawn like any
/// dropped item.
#[derive(Message, Clone, Copy, Debug)]
pub struct WeaponBroken {
    pub form_id: u32,
    pub weapon: FormId,
    pub dropped: Option<FormId>,
}

/// A hit shoves an actor's body at a point (game space): the ragdoll of a dead actor is
/// pushed by it (nvforever `ragdoll.rs`). `dv` is the change of velocity of the body
/// part struck, game units per second [design].
#[derive(Message, Clone, Copy, Debug)]
pub struct ActorPush {
    pub form_id: u32,
    pub point: [f32; 3],
    pub dv: [f32; 3],
}

/// A melee blow's knockback on the living actor it struck (docs/spec-notes/
/// combat-rules.md, "Knockback"): the Melee Knockback Formula's force read as the speed it
/// pushes the actor at, units per second, falling evenly to rest over fKnockbackTime
/// (`secs`), so it travels `speed × secs / 2` [design]; `dir` is the way, along the
/// ground (game x, y, a unit vector). Whoever moves the actor pushes it through its
/// character, so walls and actors stop it: `crate::player::Lunge::knock` for the player,
/// nvforever's residents for the others.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct Knockback {
    pub form_id: u32,
    pub dir: [f32; 2],
    pub speed: f32,
    pub secs: f32,
    /// Who struck the blow (a reference's FormID); 0: no blow (`SetVel`).
    pub attacker: u32,
    /// The blow's number in this run, from 1 in the order they land; 0: no blow. Ties a
    /// knockback's log lines to the blow.
    pub blow: u32,
    /// The blow was a power attack.
    pub power: bool,
}

impl Knockback {
    /// A knockback on `form_id` by a blow going along `dir` (game space; only its ground
    /// part counts), with the formula's `force`; `None` for a blow straight down or up
    /// or no force.
    pub fn of(form_id: u32, dir: V3, force: f32, secs: f32) -> Option<Knockback> {
        let len = dir[0].hypot(dir[1]);
        (len > 1e-3 && force > 0.0 && secs > 0.0).then(|| Knockback { form_id, dir: [dir[0] / len, dir[1] / len], speed: force, secs, attacker: 0, blow: 0, power: false })
    }

    /// This knockback comes of blow number `blow` by `attacker`.
    pub fn by(self, attacker: FormId, blow: u32, power: bool) -> Knockback {
        Knockback { attacker: attacker.0, blow, power, ..self }
    }

    /// How far it pushes in all, game units.
    pub fn distance(&self) -> f32 {
        self.speed * self.secs / 2.0
    }

    /// The speed `left` seconds before its end (falling evenly to 0).
    pub fn speed_at(&self, left: f32) -> f32 {
        if self.secs <= 0.0 {
            return 0.0;
        }
        self.speed * (left / self.secs).clamp(0.0, 1.0)
    }

    /// How far it pushes over a step of `dt` seconds from `left` seconds before its end
    /// (the exact share of the falling speed's ground).
    pub fn step(&self, left: f32, dt: f32) -> f32 {
        let to = (left - dt).max(0.0);
        (self.speed_at(left) + self.speed_at(to)) / 2.0 * (left.max(0.0) - to)
    }
}

/// How fast an explosion's knockdown throws the body away from the blast, game units per
/// second [design: the EXPL force is not converted].
pub const BLAST_KNOCKDOWN_SPEED: f32 = 300.0;

/// How fast a shot and a melee blow shove the body part they strike, game units per
/// second, and the share of a melee shove that goes upwards [design].
pub const SHOT_PUSH: f32 = 140.0;
pub const MELEE_PUSH: f32 = 220.0;
pub const POWER_PUSH: f32 = 1.6;

/// A shot's push on a loose object [design]: the projectile's Impact Force (PROJ DATA)
/// read as an impulse in the stored collision's units, mass × metres per second
/// [hypothesis: the field's unit is not documented; the masters' bullets carry 3–6, which
/// on a bottle of mass 1 is 3–6 m/s], times this.
pub const OBJECT_PUSH: f32 = 1.0;
/// The most speed one shot gives an object, game units per second [design].
pub const OBJECT_PUSH_MAX_SPEED: f32 = 700.0;

/// Within this distance of its target a shooter fires from the hip, beyond it down its
/// sights: 20 m at 70 units per metre [owner's playtest list 2026-10-11; the units per
/// metre: observed, gravity fits 9.81 m/s² at 70 units/m, physics.md §6.3]. The NPCs
/// (docs/design/combat-ai.md §10.6) and the player's VATS mode shots in third person
/// (docs/design/vats-mode.md §11.5) follow it.
pub const HIP_FIRE_WITHIN: f32 = 1400.0;
/// Once aiming down the sights, a shooter keeps them up until its target is this much
/// nearer than [`HIP_FIRE_WITHIN`], so it does not switch at the edge [design].
pub const SIGHTS_HYSTERESIS: f32 = 50.0;

/// Whether a shooter fires down its sights at a target `distance` away: only a gun with
/// sights (`sight_fov` above 0), beyond [`HIP_FIRE_WITHIN`]; `was`: it aimed down them
/// last frame (they stay up to [`SIGHTS_HYSTERESIS`] nearer).
pub fn aims_down_sights(was: bool, distance: f32, sight_fov: f32) -> bool {
    sight_fov > 0.0 && distance > if was { HIP_FIRE_WITHIN - SIGHTS_HYSTERESIS } else { HIP_FIRE_WITHIN }
}

/// The straight-line distance between two points, units.
pub fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The player's limb conditions (codes 25–31, 100 healthy, 0 or less crippled), read
/// from the script runtime each frame (step C8).
#[derive(Resource, Debug, Clone, Copy)]
pub struct PlayerLimbs {
    pub condition: [f32; 7],
    /// The share of normal movement speed the legs allow (`fnv_game::Game::leg_speed_mult`).
    pub leg_speed: f32,
}

impl Default for PlayerLimbs {
    fn default() -> Self {
        PlayerLimbs { condition: [100.0; 7], leg_speed: 1.0 }
    }
}

/// The limbs by condition code 25–31, for messages.
const LIMB_NAMES: [&str; 7] = ["head", "torso", "left arm", "right arm", "left leg", "right leg", "brain"];

impl PlayerLimbs {
    pub fn crippled(&self, code: u16) -> bool {
        code.checked_sub(25).and_then(|i| self.condition.get(i as usize)).is_some_and(|c| *c <= 0.0)
    }

    /// Left and right arm crippled.
    pub fn arms(&self) -> (bool, bool) {
        (self.crippled(27), self.crippled(28))
    }

    /// The share of normal movement speed the legs allow: the game settings for one and
    /// two crippled legs, 0.85 and 0.75 in FalloutNV.esm (docs/spec-notes/crippled-limbs.md
    /// §4), the rule of every actor.
    pub fn speed_mult(&self) -> f32 {
        self.leg_speed
    }

    /// Both legs crippled: no directional power attacks outside VATS mode
    /// (crippled-limbs.md §4).
    pub fn both_legs(&self) -> bool {
        self.crippled(29) && self.crippled(30)
    }

    /// A leg is crippled: the body limps (`locomotion\hurt\` clips) [design: either leg,
    /// owner's request 2026-10-16].
    pub fn limping(&self) -> bool {
        self.crippled(29) || self.crippled(30)
    }

    /// The crippled limbs, named.
    pub fn crippled_names(&self) -> Vec<&'static str> {
        (25u16..=31).filter(|&c| self.crippled(c)).map(|c| LIMB_NAMES[(c - 25) as usize]).collect()
    }
}

/// The body part an actor was hit on (step C8): its BPTD entry, when its volume has parts.
fn body_part(c: &Combat, parts: &ActorParts, actor: ActorId, part: Option<u8>) -> Option<BodyPart> {
    let data = parts.data_of(actor as u32)?;
    c.forms.body_parts.get(&data)?.parts.get(part? as usize).cloned()
}

/// The limb damage of hit `d` on part `bp` with weapon `w` (`Damage::limb_damage`: without
/// the part's damage multiplier): none when the part has no limb condition or the
/// weapon's Limb Dmg Mult is 0 [documented: WEAP field; hypothesis: 0 means no limb damage].
fn limb_hit(bp: Option<&BodyPart>, d: &damage::Damage, w: &WeaponStats) -> Option<LimbHit> {
    let bp = bp?;
    let condition = fnv_game::combat::limb_condition(bp.actor_value.as_deref()?)?;
    let damage = d.limb_damage(w.limb_dmg_mult) as f64;
    (damage > 0.0).then_some(LimbHit { condition, health_percent: bp.health_percent as f64, damage, points: false })
}

/// The human skeleton, whose body part data the player's limbs follow [design:
/// `CombatForms::body_parts_for`, docs/spec-notes/combat-rules.md §12].
const HUMAN_SKELETON: &str = "characters\\_male\\skeleton.nif";

// [design] Human skeleton proportions for the player's single capsule (combat-rules.md
// §12). How the original chooses the player's hit part remains [hypothesis].
const PLAYER_HEAD_HEIGHT: f32 = 0.86;
const PLAYER_LEG_HEIGHT: f32 = 0.50;
const PLAYER_ARM_RADIUS: f32 = 0.60;

/// The condition of the part struck on the player's capsule [hypothesis: the bands
/// above, not measured in the original]. Heading is clockwise from north, as `Player`.
fn player_hit_condition(point: V3, feet: V3, height: f32, radius: f32, heading: f32) -> u16 {
    use fnv_game::combat::{HEAD, LEFT_ARM, LEFT_MOBILITY, RIGHT_ARM, RIGHT_MOBILITY, TORSO};
    let height = height.max(f32::EPSILON);
    let z = (point[2] - feet[2]).clamp(0.0, height) / height;
    let side = (point[0] - feet[0]) * heading.cos() - (point[1] - feet[1]) * heading.sin();
    if z > PLAYER_HEAD_HEIGHT {
        HEAD
    } else if z < PLAYER_LEG_HEIGHT {
        if side < 0.0 { LEFT_MOBILITY } else { RIGHT_MOBILITY }
    } else if side.abs() > radius.max(0.0) * PLAYER_ARM_RADIUS {
        if side < 0.0 { LEFT_ARM } else { RIGHT_ARM }
    } else {
        TORSO
    }
}

/// Resolve the capsule's hit to the player's limb data by condition, never by BPTD
/// index [design: the same human data as `blast_limb_hits`, in either view].
fn player_body_part(c: &Combat, phys: &PhysState, player: Option<&Player>, point: V3) -> Option<BodyPart> {
    let ch = phys.player.as_ref()?;
    let condition = player_hit_condition(point, ch.feet, ch.params.height, ch.params.radius, player?.heading);
    let data = c.forms.body_parts_for(FormId::PLAYER_BASE, HUMAN_SKELETON)?;
    c.forms.body_parts.get(&data)?.parts.iter().find(|bp| bp.actor_value.as_deref().and_then(fnv_game::combat::limb_condition) == Some(condition)).cloned()
}

/// An explosion's limb damage on `target` (docs/spec-notes/crippled-limbs.md §5): the
/// shares of `fnv_game::combat::blast_limb_shares` (the torso 1/30 of the blast's damage
/// after armour, the legs 0.24; nothing on the head, arms or brain) of the limb conditions
/// its body part data names, times the weapon's Limb Dmg Mult (1 without a weapon)
/// [observed: the shares; hypothesis: the multiplier, 1 on the measured mine and grenade].
/// The player's body part data is the human skeleton's when the body is not posed.
fn blast_limb_hits(c: &Combat, parts: &ActorParts, target: FormId, dealt: f32, weapon: Option<&WeaponStats>) -> Vec<LimbHit> {
    let mult = weapon.map_or(1.0, |w| w.limb_dmg_mult.max(0.0));
    let damage = f64::from(dealt.max(0.0) * mult);
    let data = parts.data_of(target.0).or_else(|| (target == FormId::PLAYER_REF).then(|| c.forms.body_parts_for(FormId::PLAYER_BASE, HUMAN_SKELETON)).flatten());
    let Some(bpd) = data.and_then(|d| c.forms.body_parts.get(&d)).filter(|_| damage > 0.0) else { return Vec::new() };
    let conditions: Vec<u16> = bpd.parts.iter().filter_map(|bp| bp.actor_value.as_deref().and_then(fnv_game::combat::limb_condition)).collect();
    fnv_game::combat::blast_limb_shares(&conditions, damage)
}

/// The Splash Damage Torso Gibbing tweak's chance on a Bloody Mess blast kill, and the
/// torso's body part location (ours, the part type's index) [design: docs/design/tweaks.md].
const SPLASH_TORSO_CHANCE: f32 = 0.25;
const TORSO_LOCATION: i32 = 0;

/// The player's Fatal Crits Always Gib tweak: a killing critical that is no sneak attack
/// takes the struck part off for sure, exploded where it can be, else severed [design: docs/design/tweaks.md].
fn crit_gib(c: &Combat, parts: &ActorParts, script: Option<&ScriptState>, critical: bool, check: &mut crate::dismember::Check) {
    if !critical || check.corpse || !script.and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.fatal_crits_gib) {
        return;
    }
    let Some(data) = parts.data_of(check.target.0).and_then(|d| c.forms.body_parts.get(&d)) else { return };
    let Some(location) = data.parts.get(check.part).map(|p| p.location()) else { return };
    if let Some((part, fate)) = fnv_combat::dismember::scripted(data, location) {
        check.part = part;
        check.scripted = Some(fate);
    }
}

/// A hit that may dismember (`crate::dismember`): one that killed the actor, or landed on
/// its corpse, on a body part. The player is not dismembered.
pub(crate) fn dismember_check(target: FormId, part: Option<u8>, w: &WeaponStats, r: &HitResult, by: Option<FormId>, point: V3, dir: V3) -> Option<crate::dismember::Check> {
    let part = part.filter(|&p| p != WEAPON_PART && p != fnv_phys::gore::GORE_PART && p != fnv_phys::actors::WHOLE)?;
    if target == FormId::PLAYER_REF || !(r.died || r.corpse) {
        return None;
    }
    Some(crate::dismember::Check {
        target,
        part: part as usize,
        on_hit: fnv_combat::dismember::OnHit::from_label(w.on_hit.as_deref()),
        crippled: r.limb.is_some_and(|(_, c)| c <= 0.0),
        corpse: r.corpse,
        overkill: r.overkill as f32,
        max_health: r.max_health as f32,
        by,
        point,
        dir,
        scripted: None,
    })
}

/// The body part of `actor` a line from `from` towards `to` meets first, and where
/// (other actors left out): the part a blast or a blow lands on, for dismemberment
/// (dismemberment.md §4) [design: as a shot's part is the one its ray meets].
pub(crate) fn facing_part(world: &dyn CombatWorld, actor: ActorId, from: V3, to: V3) -> Option<(u8, V3)> {
    // Corpses too: one lying between does not take the part (#2346).
    let others: Vec<ActorId> = world.blast_targets_near(to, 512.0).into_iter().map(|(a, _)| a).filter(|&a| a != actor).collect();
    let hit = world.ray(from, to, &others)?;
    (hit.actor == Some(actor)).then_some(())?;
    Some((hit.part?, hit.point))
}

/// What a blast at `at` (radius `radius`) that caught `actor` (the reference `target`)
/// does towards dismembering it (`fly`): the part facing the blast, and the check made
/// there with the blast's source weapon `w` for hit `r` (dismemberment.md §4.2). Without a
/// source weapon, neither. A corpse is found among the blast's targets by its ragdoll (#2346).
#[allow(clippy::too_many_arguments)]
fn blast_gore(world: &dyn CombatWorld, target: FormId, actor: ActorId, at: V3, radius: f32, w: Option<&WeaponStats>, r: &HitResult, by: Option<FormId>, push: V3) -> (Option<(u8, V3)>, Option<crate::dismember::Check>) {
    let Some(w) = w else { return (None, None) };
    let center = world.blast_targets_near(at, radius.max(1.0)).into_iter().find(|(a, _)| *a == actor).map(|(_, p)| p);
    let facing = center.and_then(|p| facing_part(world, actor, at, p));
    let check = facing.and_then(|(part, point)| dismember_check(target, Some(part), w, r, by, point, push));
    (facing, check)
}

/// Whether the script runtime has the actor dead.
pub fn is_dead(script: Option<&ScriptState>, form_id: u32) -> bool {
    script.and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.is_dead(FormId(form_id)))
}

/// What the HUD shows about hits [design].
#[derive(Resource, Default, Debug, Clone)]
pub struct HitFeedback {
    /// The last actor hit: name, health, maximum, seconds since.
    pub target: Option<(String, f32, f32, f32)>,
    /// Hits the player took since [`player_flinch`] last looked.
    pub player_hits: Vec<PlayerHit>,
    /// Actors whose arm a hit crippled since [`disarm`] last looked.
    pub disarms: Vec<FormId>,
    /// Health the player lost to hits since the screen's blood last looked, each with
    /// the player's maximum (`crate::screen_splatter`'s hurt splats).
    pub player_hurt: Vec<(f32, f32)>,
}

/// A hit the player took, for the flinch rule (`damage::staggers`).
#[derive(Debug, Clone, Copy)]
pub struct PlayerHit {
    pub crippled: bool,
    pub part_crippled: bool,
    pub torso_crippled: bool,
}

/// Height above a destructible's position its stage explosions go off [design].
pub(crate) const DESTRUCTION_BLAST_HEIGHT: f32 = 40.0;

/// How one actor's weapon wore from its own attacks (`CombatStats::npc_weapon_wear`).
#[derive(Default, Debug, Clone, Copy)]
pub struct NpcWeaponWear {
    pub attacks: u32,
    /// Health points lost; the condition before the first and after the last.
    pub points: f32,
    pub first: f32,
    pub last: f32,
}

/// A co-op guest's hit on an actor, for the host, whose world it is, to apply
/// ([`CombatStats::relay`], [`RemoteHit`]).
#[derive(Debug, Clone, Copy)]
pub struct RelayedHit {
    pub target: FormId,
    pub dealt: f32,
    pub weapon: Option<FormId>,
    pub location: i32,
    pub critical: bool,
    pub limb: Option<fnv_game::combat::LimbHit>,
}

/// [`RelayedHit::location`] of a shot on a placed object that is not an actor (a hit
/// location is -1 or a body part's index): its hit blocks run on the host.
pub const OBJECT_LOCATION: i32 = -1000;

/// A hit from another game (a co-op guest's, [`RelayedHit`]) to apply here as `by`'s.
#[derive(Message, Debug, Clone, Copy)]
pub struct RemoteHit {
    pub by: FormId,
    pub hit: RelayedHit,
}

/// Counts for the exit report.
#[derive(Resource, Default, Debug, Clone)]
pub struct CombatStats {
    /// A co-op guest's: the host runs the world, so hits on actors other than the player
    /// are not applied here. The player's own are kept here for the host; others (none
    /// should land, the guest's NPCs not fighting) are dropped. `None` in any other game.
    pub relay: Option<Vec<RelayedHit>>,
    /// A co-op host's: hits on the other players' actors (`fnv_game::Game::coop_actors`),
    /// each with who dealt it, for their own games to take; they do nothing here.
    pub coop_hits: Vec<(Option<FormId>, RelayedHit)>,
    /// The player's shots that rolled for their ammo's spent item (a case, hull or
    /// drained cell), and the items given, by item.
    pub spent_rolls: u32,
    pub spent_given: std::collections::BTreeMap<u32, u32>,
    /// The magic items hits applied and which are still active (`hit_effects`).
    pub hit_effects: hit_effects::HitEffects,
    /// Hits between actors other than the player, by target: (name, the damage of each
    /// hit, its maximum health), for the summary that sets a fight against the AI
    /// observer's (docs/spec-notes/npc-behaviour-compare.md §9).
    pub npc_hits: std::collections::BTreeMap<u32, (String, Vec<f32>, f32)>,
    /// Wear of other actors' weapons from their own shots and blows (§17), by actor and
    /// weapon, for the exit summary.
    pub npc_weapon_wear: std::collections::BTreeMap<(u32, u32), NpcWeaponWear>,
    pub shots: u32,
    pub projectiles: u32,
    pub actor_hits: u32,
    pub world_hits: u32,
    /// Shots on objects whose OnHit / OnHitWith blocks ran, and objects knocked away.
    pub object_scripts: u32,
    /// The player's shots on objects whose OnHit / OnHitWith blocks ran, by object
    /// reference: the quest pilot's sign that a shot landed (the laser detonator on Lonesome
    /// Road's warheads).
    pub player_object_hits: std::collections::BTreeMap<u32, u32>,
    pub objects_pushed: u32,
    pub explosions: u32,
    pub damage: f32,
    pub reloads: u32,
    pub dry_fires: u32,
    /// Shots that jammed the weapon (item 9).
    pub jams: u32,
    /// Melee blows that showed an impact (item 24).
    pub melee_impacts: u32,
    /// Shots that splashed going into water, and explosions that threw up water.
    pub water_splashes: u32,
    pub water_blasts: u32,
    pub kills: u32,
    pub xp: f64,
    pub swings: u32,
    pub power_attacks: u32,
    /// Melee blows a blocker stopped part of (docs/design/combat-ai.md §14.3).
    pub blows_blocked: u32,
    /// Blocks power attacks broke through (combat-rules.md §12).
    pub blocks_broken: u32,
    /// Combat-tab lines said by anyone (Hit, Attack, PowerAttack, Death …), and tries by
    /// actors that have no line of the topic (`fnv_game::combat_lines`).
    pub combat_lines: u32,
    pub combat_lines_none: u32,
    /// Swings that went on from the last one's `a:` key (combos).
    pub chained_swings: u32,
    pub melee_hits: u32,
    pub knockdowns: u32,
    /// Shots drawn from the weapon's muzzle, and shots that left it because something
    /// stood in front of the barrel (third person).
    pub muzzle_shots: u32,
    pub covered_shots: u32,
    /// Item condition (§17): health points the player's weapons lost, hits on weapons
    /// actors hold, weapons broken, health points armour lost, armour worn to zero.
    pub weapon_wear: f32,
    pub weapon_hits: u32,
    pub weapons_broken: u32,
    pub armor_wear: f32,
    pub armor_broken: u32,
    /// Actors disarmed by a crippled arm ([`disarm`]).
    pub disarmed: u32,
    /// Weapons dropped by actors that died holding them ([`death_drops`]).
    pub death_drops: u32,
    /// Other actors' shots and blows (combat AI), their hits on the player and the
    /// damage those did, and whether the player died.
    pub actor_shots: u32,
    pub actor_strikes: u32,
    pub player_hits: u32,
    pub player_damage: f32,
    pub player_died: bool,
    /// Hits of harmless shots ([`ActorFire::harmless`]): no damage dealt.
    pub harmless_hits: u32,
    /// Of those, the hits whose target's OnHit / OnHitWith blocks ran.
    pub harmless_scripts: u32,
    /// Blood on the world behind shot actors ([`world_blood`]): the hits that rolled for
    /// it, those that won the roll, and of those the ones that met a surface (a splatter
    /// sent to the decals).
    pub blood_rolls: u32,
    pub blood_won: u32,
    pub blood_sent: u32,
    /// The same by actor struck (its reference): hits rolled, splatters sent.
    pub blood_by: std::collections::BTreeMap<u32, (u32, u32)>,
}

/// A projectile in flight with where it left from, where it is drawn and whether it
/// draws a tracer.
/// A projectile in flight VATS mode may mark ([`Combat::vats_projectiles`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VatsProjectile {
    /// Its number among the drawn projectiles, the same for its whole flight.
    pub id: u64,
    pub at: V3,
    /// The player's own.
    pub by_player: bool,
    /// It has an explosion.
    pub explosive: bool,
    /// A thrown mine still in the air.
    pub mine: bool,
}

/// A grenade live now ([`Combat::live_grenades`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiveGrenade {
    /// Its number among the drawn projectiles, the same for its whole flight.
    pub id: u64,
    pub at: V3,
    /// Its explosion and that explosion's radius, units.
    pub explosion: FormId,
    pub radius: f32,
    pub thrower: FormId,
    /// Seconds since it left.
    pub age: f32,
}

/// VATS mode's explosive projectiles this frame (`crate::vats_mode` holds its slow time
/// for them, docs/design/vats-mode.md §9): how many fly toward the point they were sent
/// at, how many fly off from it (further from it than they have been by more than
/// [`VATS_FLY_OFF`]), and how many have gone off since the game began.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VatsBlast {
    pub toward: u32,
    pub off: u32,
    pub went_off: u32,
}

/// An explosive sent by VATS mode is flying off once it is this much further from the
/// point it was sent at than the nearest it has been, units [design: the fork owner
/// 2026-11-10, the mode is not to wait for "a projectile flying away from the target ...
/// until the projectile hits an invisible wall"; about a grenade's blast radius, so one
/// that bounces and rolls beside its mark still counts as coming].
pub const VATS_FLY_OFF: f32 = 384.0;

struct Flying {
    p: Projectile,
    /// The point VATS mode sent it at, for an explosive of the player's ([`VatsBlast`]),
    /// the nearest it has been to it, and whether it was told that it flies off.
    vats_aim: Option<V3>,
    nearest: f32,
    told_off: bool,
    /// Its number among the drawn projectiles ([`ShotVisual::id`]) and its PROJ.
    id: u64,
    proj: FormId,
    /// Its model is drawn.
    shown: bool,
    /// The way it last flew.
    dir: V3,
    /// It is drawn from the weapon's muzzle (not from the eye for want of one).
    from_muzzle: bool,
    origin: V3,
    seen: Seen,
    tracer: bool,
    projectiles_per_shot: u32,
    /// The projectile's Impact Force (PROJ DATA), for [`OBJECT_PUSH`].
    force: f32,
    by: By,
    /// The temporary reference scripts see it as (`fnv_game::nvse_proj`), and whether one
    /// was asked for; the speed multiplier last applied.
    rref: Option<FormId>,
    registered: bool,
    speed_mult: f32,
    /// Where its flight ended without a hit (`ProjEvent::Expired`).
    ended_at: Option<V3>,
    /// The gas clouds it has passed through, each hit once (`crate::gas_traps`, #2687).
    gassed: Vec<FormId>,
    /// The Projectile Zone primitives (`XTRI 23`) it has crossed, each hit once (#3662).
    proj_zones_hit: Vec<FormId>,
}

/// A hit waiting for its delivery time (hitscan: distance ÷ speed).
struct Pending {
    due: f32,
    actor: Option<ActorId>,
    /// The body part hit (step C8).
    part: Option<u8>,
    /// The placed reference hit, for destructible objects.
    object: Option<u64>,
    point: V3,
    /// The way the projectile was flying.
    dir: V3,
    /// The surface's normal and collision material, for the impact effect.
    normal: V3,
    material: Option<u32>,
    projectiles: u32,
    force: f32,
    by: By,
    /// The shot's reference for scripts, kept until the hit lands; when it was made
    /// (`time`) and its speed multiplier as last applied.
    rref: Option<FormId>,
    since: f32,
    speed_mult: f32,
}

/// What is drawn of a projectile whose hit was found at once: a bullet flying from the
/// muzzle to where it landed, or a beam between the two [design].
struct Ghost {
    id: u64,
    proj: FormId,
    /// Its original script reference, for explicit deletion while the visual remains.
    rref: Option<FormId>,
    /// Who fired it.
    owner: FormId,
    from: V3,
    to: V3,
    /// Units per second (a bullet); a beam stays for `left` seconds instead.
    speed: f32,
    travelled: f32,
    beam: bool,
    left: f32,
    /// The whole of `left` when made (the PROJ's Fade Duration).
    fade: f32,
    /// Made this frame: drawn at its start before it moves.
    fresh: bool,
    /// A beam from the muzzle: its start stays on the muzzle while it shows.
    follow: bool,
    /// Weapon base damage and BulletSnap eligibility/player-impact flag for pass-by snaps.
    damage: f32,
    snap_eligible: bool,
    hit_player: bool,
}

/// How strongly a shot is drawn with `left` of its `fade` seconds to go: a beam fades
/// from full to nothing over its PROJ's Fade Duration, in a straight line [hypothesis:
/// what Fade Duration means, from its name and the beam staying that long; the line is
/// [design]]; anything else is drawn in full.
pub fn beam_strength(beam: bool, left: f32, fade: f32) -> f32 {
    if beam && fade > 0.0 {
        (left / fade).clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// What a [`Mark`] shows.
#[derive(Clone, Copy, PartialEq)]
enum MarkKind {
    Tracer,
    Impact,
    /// A shot left the muzzle at `to`.
    Flash,
}

/// Whether the developer markers are drawn: a cross at each impact, a line along each
/// hitscan shot (settings page `hit-markers`, `--hit-markers`) [design]. Off by default:
/// the shots' own impact effects stand for them in play. The test aid
/// `NVF_PLAY_TEST_TRACERS` draws them whatever this says.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct HitMarks {
    pub show: bool,
}

/// A short-lived marker [design].
struct Mark {
    from: V3,
    to: V3,
    age: f32,
    life: f32,
    kind: MarkKind,
    /// A line from the muzzle: its start stays on the muzzle while it shows.
    follow: bool,
}

impl Mark {
    fn tracer(from: V3, to: V3, life: f32) -> Mark {
        Mark { from, to, age: 0.0, life, kind: MarkKind::Tracer, follow: false }
    }

    fn at(point: V3, life: f32, kind: MarkKind) -> Mark {
        Mark { from: point, to: point, age: 0.0, life, kind, follow: false }
    }
}

/// How long a looping shoot sound goes on after the last shot, seconds [design].
const LOOP_SOUND_HOLD: f32 = 0.25;
/// The size of the test aid's marker at the muzzle, units.
const FLASH_SIZE: f32 = 2.5;

/// Whether a projectile's hit is found at once, on its first step (hitscan, beams), as
/// `Projectile::step` decides it.
fn instant(p: &Projectile) -> bool {
    (p.hitscan || p.motion == Motion::Beam) && p.motion != Motion::Lobber
}
/// The reach of the view ray that finds the aim point when the projectile has no range
/// of its own, units [design].
const AIM_REACH: f32 = 100_000.0;

/// The player's combat state.
#[derive(Resource)]
pub struct Combat {
    pub forms: CombatForms,
    pub rules: Rules,
    pub tuning: CombatTuning,
    /// The weapon in use: the equipped one, else the Fists.
    weapon: Option<FormId>,
    /// The equipped weapon as the simulation has it (None: unarmed).
    equipped: Option<Option<FormId>>,
    /// The equipped unit's mod flags (`Extra::mods`): its figures are the modded ones
    /// (docs/spec-notes/weapon-mods.md §2).
    mods: u8,
    /// The weapon and mod sound the player's shots last played, while it is a silenced
    /// one: its log line is written once (weapon-mods.md §7).
    silenced_logged: Option<(FormId, FormId)>,
    /// The Fists form (WEAP `Fists`), the unarmed weapon.
    fists: Option<FormId>,
    ammo: Option<FormId>,
    /// The weapon spends itself (no ammo, a projectile).
    self_ammo: bool,
    /// The weapon is melee or unarmed (no cycle; [`Swing`]s).
    melee: bool,
    /// The melee swings (step C6).
    swings: Melee,
    /// Blows that have knocked someone back so far ([`Knockback::blow`]).
    blows: u32,
    /// Explosions to set off this frame (impacts, detonations, destruction stages), with
    /// who set each off, an optional impact normal for blast visibility, and an optional
    /// script caller source actor (for `PlaceAtMe` blasts without a weapon `Cause`).
    boom: Vec<QueuedBlast>,
    /// The explosions Meltdown gives off, for the No Self Meltdown Damage tweak [design: docs/design/tweaks.md].
    meltdowns: HashSet<FormId>,
    /// Debris of destroyed objects for the engine to throw (nvforever `dismember.rs`).
    pub debris: Vec<ObjectDebris>,
    cycle: Option<Cycle>,
    kick: Kick,
    flying: Vec<Flying>,
    /// Actors whose next shot is a live grenade dropped as they died (the player's NPCs
    /// Drop Live Grenades tweak, `death_drops`): it leaves at [`LIVE_DROP_SPEED`] of its
    /// speed.
    dropped_live: Vec<FormId>,
    /// Thrown mines that came to rest this frame, for `crate::mines` to place.
    settled: Vec<SettledMine>,
    /// Thrown weapons that landed this frame, for `crate::thrown` to put down.
    landed: Vec<LandedThrow>,
    /// Shots and blasts with an object effect that landed on dry ground this frame, for
    /// `crate::grass_fire` to set the grass alight where the effect burns.
    fire_landed: Vec<crate::grass_fire::Landed>,
    /// Shots that went down into water, shown when due ([`Splash`]).
    splashes: Vec<Splash>,
    ghosts: Vec<Ghost>,
    /// The weapon's looping shoot sound while it plays, and the seconds it has left
    /// unless another shot comes.
    looping: Option<(FormId, f32)>,
    /// The last [`WeaponAnim::ejects`] seen, and when (`time`) the last shot was fired.
    ejects_seen: u32,
    last_shot: f32,
    /// The point VATS mode's throw goes at, while its clip winds up to the `Hit` key.
    vats_throw: Option<[f32; 3]>,
    /// The next [`ShotVisual::id`].
    next_visual: u64,
    pending: Vec<Pending>,
    /// Shots scripts asked for (`FireWeapon`), fired with the actors' shots next frame.
    script_shots: Vec<ActorFire>,
    /// People blocking now and the way each faces (horizontal, game space): the engine's
    /// combat AI says each frame (docs/design/combat-ai.md §14.3).
    pub guarding: HashMap<FormId, [f32; 2]>,
    /// Blows blocked since the combat AI last took them.
    pub blocked_blows: Vec<BlockedBlow>,
    /// Until when (`time`) the player cannot block: a power attack broke its block.
    guard_down_until: f32,
    marks: Vec<Mark>,
    rng: SplitMix,
    /// The melee swings' own rolls (variants), so they leave the hit rolls as they were.
    swing_rng: SplitMix,
    time: f32,
    /// Test aid `NVF_PLAY_TEST_NO_SPREAD=1`: shots leave exactly along the view (no
    /// spread, no bloom, no recoil), so an unattended check hits what it aims at.
    no_spread: bool,
    /// A save's weapon and magazine, applied when that weapon is next taken in hand
    /// (`crate::save`).
    load_magazine: Option<(u32, u32)>,
    /// A save's weapon and the type of round in its magazine, with `load_magazine`.
    load_ammo: Option<(u32, FormId)>,
    /// Player's last chosen ammo type per weapon base form for the Keep Chosen Ammo
    /// tweak (#3305) [design: per base form, player only]. Saved beside `magazine_ammo`.
    chosen_ammo: std::collections::BTreeMap<FormId, FormId>,
    /// Test aid `NVF_PLAY_TEST_TRACERS=<seconds>`: every projectile draws a line, and
    /// the lines and a marker at the muzzle stay that long, so one screenshot of an
    /// unattended check shows where the shots were drawn from. The muzzle flashes and
    /// the projectile models are not held: they are drawn as in play.
    tracer_hold: Option<f32>,
    /// The spread cone's half-angle in degrees a shot would leave in now, while a weapon
    /// with a firing cycle is drawn and ready; for the crosshair (`crate::reticle`).
    pub crosshair_cone: Option<f32>,
    /// The player's spread (PSp, spread units, after "Calculate Gun Spread" perks) at the
    /// same time: a scope's sway (docs/spec-notes/weapon-scopes.md §5).
    pub player_spread: Option<f32>,
    /// Seconds the player has held a thrown weapon's trigger (GetGrenadeHoldTime,
    /// `fnv_game::nvse_jg9`): from the press while it is held, kept after it until the
    /// next press; and whether it is held now.
    pub grenade_hold: f32,
    grenade_held: bool,
    /// When (`time`) the player's block last stopped a blow: Scribe Counter's window
    /// (perk-entry-points.md §11).
    player_blocked_at: Option<f32>,
    /// The special move the swing playing is (chosen as it starts), for its strike.
    special_swing: Option<SpecialMove>,
    /// The cone (half-angle, degrees) VATS mode's hit chance reads for the gun in hand
    /// (docs/design/vats-mode.md §17.2): the spread with the sights up, no bloom, before
    /// `fVATSSpreadMult`; kept from the last frame a gun was ready, `None` without one.
    pub vats_cone: Option<f32>,
    /// VATS mode's explosives in flight and gone off ([`VatsBlast`]).
    pub vats_blast: VatsBlast,
    /// The weapon and factor of the last Modify Throwing Velocity line (one a change).
    throw_logged: Option<(FormId, u32)>,
    /// The spread perks last logged, with their weapon ([`spread_perks`]).
    spread_perks_logged: Option<(FormId, SpreadPerks)>,
    /// The player's flinch (crippled-limbs.md §3): until when no attack starts, when the
    /// last one ended, and its own rolls (the view's kick).
    flinch_until: f32,
    flinch_end: Option<f32>,
    flinch_rng: SplitMix,
    /// The recoil's rolls, its own so other rolls keep their order.
    kick_rng: SplitMix,
    /// The bloom has just shrunk away; logged with the crosshair cone.
    kick_just_settled: bool,
    /// The view's kick still to come (pitch, heading; radians): with soft recoil a
    /// shot's kick comes over a moment instead of in one frame ([`ease_kick`]).
    kick_left: [f32; 2],
    /// The share of the kicks the view settles back down by (pitch, heading; radians), and
    /// how far it has, on springs ([`ease_back`]).
    kick_back_goal: [f32; 2],
    kick_back: [crate::ease::Spring; 2],
    /// The spent-ammo items' own rolls (docs/spec-notes/spent-ammo-items.md), so they
    /// leave the shots' rolls as they were.
    spent_rng: SplitMix,
    /// The blood splatters' own rolls ([`world_blood`]), likewise.
    blood_rng: SplitMix,
    /// The splatter chance was logged ([`blood_chance`]).
    blood_chance_logged: bool,
    /// A reload ended and no shot has been fired since (No Jam After Reload).
    pub fresh_reload: bool,
    /// Named NVForever option `localized-dt-dr` (off by default; #3843), synced live from
    /// [`LocalizedDtDr`].
    pub localized_dt_dr: bool,
}

impl Combat {
    /// [design, #2652] Run normal combat initialization over authored plugin tables,
    /// without the game/render plugins, for downstream engine tests.
    #[cfg(feature = "test-support")]
    pub fn over_plugins(play: crate::PlayData) -> Self {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = bevy::prelude::World::new();
        world.insert_resource(crate::PlayWorld(play));
        world.run_system_once(init_combat).unwrap();
        world.remove_resource::<Combat>().unwrap()
    }

    /// Build a [`Combat`] resource from [`CombatForms`] with default runtime state.
    /// Shared by [`init_combat`] and unit tests so adding a field to `Combat` updates one
    /// production constructor.
    pub(crate) fn from_forms(forms: CombatForms) -> Self {
        let rules = Rules::from_forms(&forms);
        // A thrown grenade's restitution (combat-rules.md §5).
        let grenade_restitution = forms.setting("fGrenadeRestitution").unwrap_or(CombatTuning::default().grenade_restitution);
        let fists = forms.by_edid("Fists");
        Combat {
            forms,
            rules,
            tuning: CombatTuning {
                grenade_restitution,
                ..CombatTuning::default()
            },
            weapon: None,
            equipped: None,
            mods: 0,
            silenced_logged: None,
            fists,
            ammo: None,
            self_ammo: false,
            melee: false,
            swings: Melee::default(),
            blows: 0,
            boom: Vec::new(),
            meltdowns: HashSet::new(),
            debris: Vec::new(),
            cycle: None,
            kick: Kick::default(),
            flying: Vec::new(),
            dropped_live: Vec::new(),
            settled: Vec::new(),
            landed: Vec::new(),
            fire_landed: Vec::new(),
            splashes: Vec::new(),
            ghosts: Vec::new(),
            looping: None,
            ejects_seen: 0,
            vats_throw: None,
            last_shot: 0.0,
            next_visual: 0,
            pending: Vec::new(),
            script_shots: Vec::new(),
            guarding: HashMap::new(),
            blocked_blows: Vec::new(),
            fresh_reload: false,
            guard_down_until: 0.0,
            marks: Vec::new(),
            rng: SplitMix(1),
            swing_rng: SplitMix(2),
            time: 0.0,
            no_spread: false,
            tracer_hold: None,
            load_magazine: None,
            load_ammo: None,
            chosen_ammo: std::collections::BTreeMap::new(),
            crosshair_cone: None,
            player_spread: None,
            grenade_hold: 0.0,
            grenade_held: false,
            vats_cone: None,
            vats_blast: VatsBlast::default(),
            player_blocked_at: None,
            special_swing: None,
            throw_logged: None,
            spread_perks_logged: None,
            flinch_until: 0.0,
            flinch_end: None,
            flinch_rng: SplitMix(3),
            kick_rng: SplitMix(5),
            kick_just_settled: false,
            kick_left: [0.0; 2],
            kick_back_goal: [0.0; 2],
            kick_back: [crate::ease::Spring::default(); 2],
            spent_rng: SplitMix(4),
            blood_rng: SplitMix(6),
            blood_chance_logged: false,
            localized_dt_dr: false,
        }
    }

    /// Whether the player's flinch (a staggering hit, [`player_flinch`]) still plays: the
    /// player does not walk meanwhile (`crate::player::flinch_holds_walk`).
    pub fn player_flinching(&self) -> bool {
        self.time < self.flinch_until
    }

    /// Whether the player's weapon is reloading (a reload, a jam's clearing): it ends a sprint
    /// (`crate::sprint`).
    pub fn player_reloading(&self) -> bool {
        self.cycle.as_ref().is_some_and(Cycle::reloading)
    }
}

impl Combat {
    /// Seconds since the player's last shot.
    pub fn since_last_shot(&self) -> f32 {
        self.time - self.last_shot
    }

    /// Projectiles in flight now (the stress tests' samples).
    pub fn in_flight(&self) -> usize {
        self.flying.len()
    }

    /// The grenades live now (docs/design/combat-ai.md §17.7): lobbed projectiles with an
    /// explosion, in flight or at rest, not mines.
    pub fn live_grenades(&self) -> Vec<LiveGrenade> {
        self.flying
            .iter()
            .filter(|f| f.p.alive && !f.p.mine && f.p.motion == Motion::Lobber)
            .filter_map(|f| {
                let explosion = f.p.explosion?;
                let radius = self.forms.explosions.get(&explosion)?.radius;
                (radius > 0.0).then_some(LiveGrenade { id: f.id, at: f.p.pos, explosion, radius, thrower: f.by.actor, age: f.p.age })
            })
            .collect()
    }

    /// The projectiles in flight VATS mode may mark under the player's Target Projectiles
    /// in V.A.T.S. tweak (`crate::vats_mode`): missiles and lobbed ones, not hitscan
    /// shots, beams or flames [design: docs/design/tweaks.md].
    pub fn vats_projectiles(&self) -> Vec<VatsProjectile> {
        self.flying
            .iter()
            .filter(|f| f.p.alive && !f.p.hitscan && matches!(f.p.motion, Motion::Missile | Motion::Lobber))
            .map(|f| VatsProjectile { id: f.id, at: f.p.pos, by_player: f.by.actor == FormId::PLAYER_REF, explosive: f.p.explosion.is_some(), mine: f.p.mine })
            .collect()
    }

    /// Where the projectile `id` is now, while it flies.
    pub fn projectile_at(&self, id: u64) -> Option<V3> {
        self.flying.iter().find(|f| f.id == id && f.p.alive).map(|f| f.p.pos)
    }

    /// A marked VATS shot by `by` (actor, weapon) landed on the projectile `id`: it ends
    /// where it is, its explosion going off there, the blast `by`'s hits [design:
    /// docs/design/tweaks.md, Target Projectiles in V.A.T.S.]. Whether it had an
    /// explosion; `None` when it no longer flies.
    pub fn shoot_down(&mut self, id: u64, by: (FormId, FormId)) -> Option<bool> {
        let f = self.flying.iter_mut().find(|f| f.id == id && f.p.alive)?;
        f.p.alive = false;
        let (explosion, at) = (f.p.explosion, f.p.pos);
        if let Some(x) = explosion {
            self.boom.push(QueuedBlast { id: x, at, cause: Some(Cause { actor: by.0, weapon: by.1 }), normal: None, source: None });
        }
        Some(explosion.is_some())
    }

    /// The thrown mines that came to rest since the last call (`crate::mines`).
    pub(crate) fn take_settled(&mut self) -> Vec<SettledMine> {
        std::mem::take(&mut self.settled)
    }

    /// The thrown weapons that landed since the last call (`crate::thrown`).
    pub(crate) fn take_landed(&mut self) -> Vec<LandedThrow> {
        std::mem::take(&mut self.landed)
    }

    /// The shots and blasts with an object effect that landed on dry ground since the
    /// last call (`crate::grass_fire`).
    pub(crate) fn take_fire_landed(&mut self) -> Vec<crate::grass_fire::Landed> {
        std::mem::take(&mut self.fire_landed)
    }

    /// A mine goes off (`crate::mines`): its explosion at `at`, its blast the hits of
    /// `by` (the actor who put it down and that weapon), when someone did.
    pub(crate) fn mine_explodes(&mut self, explosion: FormId, at: V3, by: Option<(FormId, FormId)>) {
        self.boom.push(QueuedBlast { id: explosion, at, cause: by.map(|(actor, weapon)| Cause { actor, weapon }), normal: None, source: None });
    }
}

/// An explosion queued for detonation this frame.
#[derive(Clone, Copy, Debug)]
struct QueuedBlast {
    id: FormId,
    at: V3,
    cause: Option<Cause>,
    normal: Option<V3>,
    source: Option<FormId>,
}

/// A thrown weapon whose projectile ended (`crate::thrown`): the weapon it was, its PROJ,
/// where it ended, the surface's normal there (none when it ran out of range in the
/// air), the way it flew, the actor it struck, and who threw it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LandedThrow {
    pub weapon: FormId,
    pub proj: FormId,
    pub point: V3,
    pub normal: Option<V3>,
    pub dir: V3,
    pub struck: Option<FormId>,
    pub by: FormId,
}

/// A shot went down into water ([`ProjEvent::Splash`]): the water impact of its weapon's
/// impact data set shows on the surface when it gets there (a shot found at once:
/// distance ÷ speed after it was fired, as its hit).
#[derive(Clone, Copy, Debug)]
struct Splash {
    due: f32,
    point: V3,
    dir: V3,
    weapon: FormId,
    shooter: FormId,
    water_impact: Option<WaterImpact>,
}

/// A thrown mine that came to rest ([`ProjEvent::Settled`]): its PROJ, where, the way it
/// last flew, and who put it down.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SettledMine {
    pub proj: FormId,
    pub point: V3,
    pub dir: V3,
    pub actor: FormId,
}

impl Combat {
    /// The weapon the player fights with now: the equipped one, else the Fists.
    pub fn player_weapon(&self) -> Option<FormId> {
        self.weapon
    }

    /// The current weapon uses the gameplay melee/Fists blocking route.
    /// Presentation reads this capability rather than guessing weapon IDs or names.
    pub fn player_weapon_blocks(&self) -> bool {
        self.melee && self.weapon.is_some_and(|id| self.forms.weapons.contains_key(&id))
    }

    /// The ammunition the player's weapon loads now (`None`: none, or melee).
    pub fn player_ammo(&self) -> Option<FormId> {
        self.ammo
    }

    /// The figures of the player's weapon `id`: the equipped one's with its mods on
    /// (`WeaponStats::modded`), any other's as the record has them.
    /// The next blow's number ([`Knockback::blow`]).
    fn next_blow(&mut self) -> u32 {
        self.blows += 1;
        self.blows
    }

    /// For the melee knockback test aid: whether the player's weapon is melee or the fists,
    /// and the swing playing, if any: whether it is a power attack.
    pub(crate) fn melee_swing(&self) -> (bool, Option<bool>) {
        (self.melee, self.swings.swing.as_ref().map(|s| s.power.is_some()))
    }

    pub fn player_stats(&self, id: FormId) -> Option<WeaponStats> {
        let w = self.forms.weapons.get(&id)?;
        Some(if self.equipped.flatten() == Some(id) && self.mods != 0 { w.modded(self.mods) } else { w.clone() })
    }

    /// The Fists (WEAP `Fists`), the unarmed weapon.
    pub fn fists_id(&self) -> Option<FormId> {
        self.fists
    }

    /// The weapon in hand and the rounds in its magazine, for a save.
    pub fn magazine_for_save(&self) -> Option<(u32, u32)> {
        let cy = self.cycle.as_ref().filter(|c| !c.no_ammo)?;
        Some((self.weapon?.0, cy.magazine))
    }

    /// The type of round in the magazine of the weapon in hand, for a save (#1574).
    pub fn magazine_ammo_for_save(&self) -> Option<u32> {
        self.cycle.as_ref().filter(|c| !c.no_ammo)?;
        self.ammo.map(|a| a.0)
    }

    /// A save's magazine and the type of round in it: the rounds go back in when that
    /// weapon is in hand. A weapon already in hand with another type is taken in hand
    /// again (`equip_sync`), so the saved type and its projectile are put back together.
    pub fn restore_magazine(&mut self, m: (u32, u32), ammo: Option<u32>) {
        self.load_magazine = Some(m);
        self.load_ammo = ammo.map(|a| (m.0, FormId(a)));
        if let (Some(w), Some(cy)) = (self.weapon, self.cycle.as_mut()) {
            if w.0 == m.0 {
                if ammo.is_some_and(|a| self.ammo != Some(FormId(a))) {
                    self.equipped = None;
                    return;
                }
                cy.magazine = m.1.min(cy.clip_size.max(m.1));
                self.load_magazine = None;
                self.load_ammo = None;
            }
        }
    }

    /// Remember `ammo` as the player's chosen ammo type for weapon base form `weapon`
    /// under the Keep Chosen Ammo tweak (#3305) [design: per base form, player only].
    pub fn remember_chosen_ammo(&mut self, weapon: FormId, ammo: FormId) {
        self.chosen_ammo.insert(weapon, ammo);
    }

    /// The player's remembered ammo type for weapon base form `weapon`, if any (#3305).
    pub fn chosen_ammo_of(&self, weapon: FormId) -> Option<FormId> {
        self.chosen_ammo.get(&weapon).copied()
    }

    /// The player's remembered per-weapon ammo choices, for a save (#3305).
    pub fn chosen_ammo_for_save(&self) -> std::collections::BTreeMap<u32, u32> {
        self.chosen_ammo.iter().map(|(w, a)| (w.0, a.0)).collect()
    }

    /// Restore the save's per-weapon remembered ammo choices (#3305). Replaces any prior
    /// choices so loading an older save with none clears stale picks.
    pub fn restore_chosen_ammo(&mut self, map: &std::collections::BTreeMap<u32, u32>) {
        self.chosen_ammo = map.iter().map(|(&w, &a)| (FormId(w), FormId(a))).collect();
    }

    /// The ammo type that equipping weapon `id` would load right now: when `keep_chosen`
    /// is on (#3305) and the remembered type is still one of the weapon's ammo choices and
    /// carried, that type wins; otherwise falls back to `CombatForms::ammo_for`.
    pub fn preferred_ammo_for(&self, id: FormId, keep_chosen: bool, carried: impl Fn(FormId) -> bool) -> Option<FormId> {
        if keep_chosen {
            if let (Some(&remembered), Some(w)) = (self.chosen_ammo.get(&id), self.forms.weapons.get(&id)) {
                if self.forms.ammo_choices(w).contains(&remembered) && carried(remembered) {
                    return Some(remembered);
                }
            }
        }
        self.forms.ammo_for(id, carried)
    }

    /// Pick the player's ammo on equip for weapon `id`: `load_ammo` (a save's
    /// weapon-in-hand ammo, #1574) takes precedence when it matches `id`, is one of the
    /// weapon's ammo choices and some is carried; otherwise when `keep_chosen` is on
    /// (#3305), the remembered type for `id` wins under the same check (retaining the
    /// remembered entry when temporarily out of stock so it returns once carried again);
    /// otherwise falls back to `CombatForms::ammo_for`.
    pub fn pick_equip_ammo(&mut self, id: FormId, w: &WeaponStats, keep_chosen: bool, carried: impl Fn(FormId) -> bool) -> Option<FormId> {
        let choices = self.forms.ammo_choices(w);
        let mut ammo = self.forms.ammo_for(id, &carried);
        if keep_chosen {
            if let Some(&remembered) = self.chosen_ammo.get(&id) {
                if choices.contains(&remembered) && carried(remembered) {
                    ammo = Some(remembered);
                }
            }
        }
        // A save's type of round for this weapon (`crate::save`; #1574), while it is one of
        // the weapon's types and some is carried.
        if let Some((sw, a)) = self.load_ammo.take() {
            if sw == id.0 && choices.contains(&a) && carried(a) {
                ammo = Some(a);
            } else if sw != id.0 {
                self.load_ammo = Some((sw, a));
            }
        }
        ammo
    }

    /// Advance the player's loaded ammo on weapon `id` to the next carried choice,
    /// remembering the chosen type for `id` when `keep_chosen` is on (#3305).
    pub fn swap_ammo_choice(&mut self, id: FormId, keep_chosen: bool, carried: impl Fn(FormId) -> bool) -> Option<FormId> {
        let cur = self.ammo?;
        let next = self.forms.next_ammo(id, cur, carried).filter(|&n| n != cur)?;
        self.ammo = Some(next);
        if keep_chosen {
            self.remember_chosen_ammo(id, next);
        }
        Some(next)
    }

    /// The weapon in use and its Sight FOV (degrees), when it is a gun with sights (Sight
    /// FOV above 0; melee weapons block instead) (`crate::aim_zoom`).
    /// Weapon `id`'s VATS special attack, when its record has one (WEAP `VATS`).
    pub fn vats_special(&self, id: FormId) -> Option<fnv_combat::vats::SpecialAttack> {
        self.forms.weapons.get(&id)?.vats_special
    }

    pub fn sight_fov(&self) -> Option<(FormId, f32)> {
        let w = self.weapon.filter(|_| !self.melee)?;
        let fov = self.player_stats(w)?.sight_fov;
        (fov > 0.0).then_some((w, fov))
    }

    /// The gun in use has its scope (weapon-scopes.md §2: Has Scope, with its scope mod
    /// when the scope comes from one).
    pub fn player_weapon_scoped(&self) -> bool {
        self.sight_fov().and_then(|(w, _)| self.player_stats(w)).is_some_and(|s| s.scope_model().is_some())
    }

    /// The PROJ the weapon in use fires with the ammunition loaded, if it fires one.
    pub fn projectile(&self) -> Option<FormId> {
        self.forms.chain(self.weapon?, self.ammo)?.projectile
    }

    /// The weapon in hand as VATS mode needs it (docs/design/vats-mode.md §3, §7, §16), or
    /// why it cannot enter: a gun with a firing cycle that fires a projectile, not
    /// reloading or jammed, a round loaded; a melee or unarmed weapon (the Fists too); a
    /// grenade or thrown weapon (not a mine), one carried.
    pub fn vats_weapon(&self, sim: &SimState, gravity: f32) -> Result<VatsWeapon, &'static str> {
        let no_gun = tr("VATS mode needs a weapon to aim.");
        use fnv_combat::vats::{shot_ap_cost, throw_reach, AttackStyle};
        let id = self.weapon.ok_or(no_gun)?;
        let w = &self.player_stats(id).ok_or(no_gun)?;
        let cost = shot_ap_cost(w, &self.rules.ap_attack);
        let edid = w.edid.clone().unwrap_or_else(|| id.to_string());
        if self.melee {
            return Ok(VatsWeapon {
                id,
                edid,
                cost,
                style: AttackStyle::Melee,
                rounds_per_shot: 1,
                rounds: u32::MAX / 2,
                reach: crate::vats_mode::MELEE_MARK_REACH,
                // From the eye, as the player's swing reaches (`melee`): the reach is
                // from the body, the capsule's radius further.
                melee_reach: damage::melee_reach(&self.rules, w, 1.0) + fnv_phys::CharacterParams::default().radius,
                classic_reach: Some(damage::vats_melee_reach(&self.rules, w, 1.0) + fnv_phys::CharacterParams::default().radius),
            });
        }
        // A gun needs to be equipped: the Fists are melee.
        if self.equipped.flatten() != Some(id) || self.player_weapon_is_mine() {
            return Err(no_gun);
        }
        let proj = self.forms.chain(id, self.ammo).and_then(|ch| ch.projectile).ok_or(no_gun)?;
        if self.self_ammo {
            let carried = player_count(sim, Some(id));
            if carried == 0 {
                return Err(tr("Out of ammo."));
            }
            let p = self.forms.projectiles.get(&proj);
            let range = p.map_or(0.0, |p| if p.range > 0.0 { p.range } else { crate::vats_mode::REACH });
            let lob = p.map_or(f32::INFINITY, |p| throw_reach(p.speed, p.fall() * gravity));
            return Ok(VatsWeapon { id, edid, cost, style: AttackStyle::Thrown, rounds_per_shot: 1, rounds: carried, reach: range.min(lob), melee_reach: 0.0, classic_reach: None });
        }
        if AttackKind::of(w) != AttackKind::Gun {
            return Err(no_gun);
        }
        let cy = self.cycle.as_ref().ok_or(no_gun)?;
        if cy.reloading() || cy.jammed() {
            return Err(tr("Not while reloading."));
        }
        let style = AttackStyle::of(w, false, false);
        // God mode keeps the magazine full (the shots take nothing).
        // A weapon of Ammo Use 0 spends none (#3023): as one with no ammo.
        let free = cy.no_ammo || cy.ammo_use == 0;
        let rounds = if free {
            u32::MAX / 2
        } else if god_mode() && cy.regen.is_none() {
            cy.clip_size
        } else {
            cy.magazine
        };
        if !free && rounds < cy.ammo_use {
            return Err(tr("Out of ammo."));
        }
        let range = self.forms.projectiles.get(&proj).map_or(0.0, |p| p.range);
        Ok(VatsWeapon {
            id,
            edid,
            cost,
            style,
            // A burst's rounds are one mark's (§16.1).
            rounds_per_shot: if free { 1 } else { cy.ammo_use * style.shots_per_mark() },
            rounds,
            reach: if range > 0.0 { range } else { crate::vats_mode::REACH },
            melee_reach: 0.0,
            classic_reach: None,
        })
    }

    /// GetAnimAction's code for what the player's weapon does now
    /// (`fnv_game::anim_state`): drawing (0) and putting away (1) as the body says, a
    /// gun's shot by its firing cycle (2 from the pull to the shot and while an automatic
    /// fires on with the trigger held, 3 the frame after, 4 until the cycle is ready
    /// again), a reload 9, a melee swing 2 until it strikes and 4 after, blocking 7. A
    /// weapon that spends itself (grenades, thrown weapons, mines) reads 5 until it leaves
    /// the hand, 6 the frame after and 4 while the next is drawn (`anim_state::
    /// throw_action`). A round-by-round reload reads 17 while it opens the weapon, 9 for
    /// its rounds and its close [hypothesis: `AnimAction::ReloadOpen`]. A forced jam reads
    /// 9 (`AnimAction::Jam`, with `IsAnimPlayingEx 4 == 0`) [observed: Fallout: New Vegas
    /// 1.4.0.525, xNVSE 6.4.9, ShowOffNVSE Plugin 184, 2026-11-23, GSDocMitchellHouse
    /// (0x00103DF9), 10 official base masters, probe `w1_3632_onfireweapon_and_forcejam`
    /// in #3679 (`Player.ForceWeaponJamAnim` on drawn `Weap10mmPistol` `0x0000434F` ->
    /// `GetAnimAction == 9`, `IsAnimPlayingEx 4 == 0`)]; generalizing `Phase::Jammed ->
    /// AnimAction::Jam` (`9` instead of `13`) to natural vanilla weapon-condition jams is
    /// [hypothesis].
    /// The player's weapon is thrown or laid by hand (SUP's GetGrenadeTimeHeld,
    /// `crate::sup_screen`): its animation type is one of
    /// `fnv_sim::forms::THROWN_ANIM_TYPES`.
    pub fn player_weapon_is_throwable(&self) -> bool {
        self.weapon.and_then(|w| self.forms.weapons.get(&w)).and_then(|w| w.anim_type.as_deref()).is_some_and(|t| fnv_sim::forms::THROWN_ANIM_TYPES.contains(&t))
    }

    /// The player's weapon is a mine: its WEAP animation type is "Land Mine (1 Hand)" or
    /// "Mine Drop (1 Hand)" (IsPlayerActionActive 4 while one is laid).
    pub fn player_weapon_is_mine(&self) -> bool {
        self.weapon.and_then(|w| self.forms.weapons.get(&w)).and_then(|w| w.anim_type.as_deref()).is_some_and(|t| t.starts_with("Land Mine") || t.starts_with("Mine Drop"))
    }

    pub fn player_anim_action(&self, anim: &WeaponAnim, trigger: bool, aiming: bool, dt: f32) -> Option<fnv_game::anim_state::AnimAction> {
        use fnv_game::anim_state::{swing_action, AnimAction};
        if anim.drawing {
            return Some(AnimAction::Equip);
        }
        if anim.holstering {
            return Some(AnimAction::Unequip);
        }
        if !anim.ready {
            return None;
        }
        if self.melee {
            if let Some(s) = &self.swings.swing {
                return swing_action(s.t, s.timing.hit, s.timing.len);
            }
            return aiming.then_some(AnimAction::Block);
        }
        let cy = self.cycle.as_ref()?;
        match cy.phase {
            Phase::SpinUp { .. } | Phase::WindUp { .. } if self.self_ammo => Some(AnimAction::ThrowWindUp),
            Phase::SpinUp { .. } | Phase::WindUp { .. } => Some(AnimAction::Attack),
            Phase::Cooldown { left } => cooldown_action(&cy.timing, left, self.self_ammo, cy.automatic && trigger, dt),
            Phase::ReloadOpen { .. } => Some(AnimAction::ReloadOpen),
            Phase::Reloading { .. } | Phase::ReloadRound { .. } | Phase::ReloadClose { .. } => Some(AnimAction::Reload),
            Phase::Jammed { .. } => Some(AnimAction::Jam),
            Phase::Ready => None,
        }
    }

    /// The player aims down a gun's sights now: a gun with sights in hand and Block held
    /// (IsPlayerActionActive 9, GetPCUsingIronSights), not while a reload or a jam holds
    /// the sights down ([`sights_held_down`]).
    pub fn player_sights_up(&self, anim: &WeaponAnim, aiming: bool) -> bool {
        sights_up(anim.ready, aiming, self.player_sights_held_down(), self.sight_fov().is_some())
    }

    /// The gun's firing cycle holds the sights down now: it reloads or clears a jam
    /// ([`sights_held_down`]). The aim zoom and the sight pose ease out meanwhile
    /// (`crate::aim_zoom`) and come back once the cycle is through, if Block is still held.
    pub fn player_sights_held_down(&self) -> bool {
        !self.melee && self.cycle.as_ref().is_some_and(|c| sights_held_down(&c.phase))
    }

    /// The player's melee swing is playing (VATS mode waits for it, vats-mode.md §16.2).
    pub fn player_swinging(&self) -> bool {
        self.melee && self.swings.swing.is_some()
    }

    /// The player's skill with the weapon in hand (VATS mode's hit chance, vats-mode.md
    /// §17.2): its WEAP skill's actor value.
    pub fn player_weapon_skill(&self, script: Option<&mut ScriptState>, sim: &mut SimState) -> f32 {
        let Some(w) = self.weapon.and_then(|id| self.player_stats(id)) else { return Attacker::default().skill };
        attacker(script, sim, &self.rules, FormId::PLAYER_REF, &w, false).skill
    }

    /// The BPTD part an actor's hit volume part is (VATS mode's marks).
    pub fn vats_part(&self, parts: &ActorParts, actor: u32, part: Option<u8>) -> Option<BodyPart> {
        body_part(self, parts, actor as ActorId, part)
    }
}

/// The sights are up ([`Combat::player_sights_up`]): the weapon is ready, Block is held
/// with it out, no reload or jam holds them down, and the weapon in use has sights (a gun;
/// a melee weapon has none, so blocking with one is not aiming).
pub fn sights_up(ready: bool, aiming: bool, held_down: bool, has_sights: bool) -> bool {
    ready && aiming && !held_down && has_sights
}

/// A firing cycle in this phase holds the sights down: a reload (every step of a
/// round-by-round one too) or a jam plays its clip with the gun off the eye, so the player
/// comes out of the sights (and a scope) as it starts (the owner's report, #1642)
/// [design: the clip's timing is unchanged; the sights ease out as it begins].
pub fn sights_held_down(phase: &Phase) -> bool {
    matches!(phase, Phase::Reloading { .. } | Phase::ReloadOpen { .. } | Phase::ReloadRound { .. } | Phase::ReloadClose { .. } | Phase::Jammed { .. })
}

/// The weapon in hand for VATS mode ([`Combat::vats_weapon`]).
#[derive(Debug, Clone)]
pub struct VatsWeapon {
    pub id: FormId,
    pub edid: String,
    /// AP a mark (`fnv_combat::vats::shot_ap_cost`): a shot, a burst, a swing, a throw.
    pub cost: f32,
    /// How a mark is spent (vats-mode.md §16).
    pub style: fnv_combat::vats::AttackStyle,
    /// Rounds one mark takes (a burst's all).
    pub rounds_per_shot: u32,
    /// Rounds in the magazine now, or the thrown weapons carried (the marks' limit).
    pub rounds: u32,
    /// How far the crosshair's ray looks for a target: the projectile's range, else
    /// `vats_mode::REACH`; a thrown weapon's no farther than it throws; a melee
    /// weapon's `vats_mode::MELEE_MARK_REACH`.
    pub reach: f32,
    /// A melee weapon's own reach from the eye (`damage::melee_reach` plus the player's
    /// capsule radius), units; 0 otherwise.
    pub melee_reach: f32,
    /// A melee or unarmed weapon's target reach from the eye in the Classic style: the
    /// original's V.A.T.S. melee reach (`damage::vats_melee_reach`) plus the player's
    /// capsule radius, as `melee_reach` counts it (issue #2656). `None` for the others.
    pub classic_reach: Option<f32>,
}

/// Named NVForever option `localized-dt-dr` (off by default; #3843): when enabled, head
/// hits apply only worn head-armour DT/DR and body hits apply only worn body-armour DT/DR,
/// while explosions still use full worn armour and innate actor DT/DR still applies
/// everywhere [documented: JIP LN NVSE v57.30 / Settings INI v56.24 `jip_nvse.ini`
/// `bLocalizedDTDR`; design: `docs/spec-notes/localized-dt-dr.md`].
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LocalizedDtDr(pub bool);

/// Keep [`Combat::localized_dt_dr`] aligned with the live [`LocalizedDtDr`] setting so
/// toggling it from the pause menu takes effect immediately.
pub(crate) fn sync_localized_dt_dr(setting: Option<Res<LocalizedDtDr>>, mut combat: ResMut<Combat>) {
    let want = setting.as_deref().is_some_and(|s| s.0);
    if combat.localized_dt_dr != want {
        combat.localized_dt_dr = want;
    }
}

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var_os("NVF_PLAY_TEST_GOD").is_some_and(|v| v != "0") {
            set_god_mode(true);
        }
        app.init_resource::<LocalizedDtDr>();
        crate::settings::add_setting(
            app,
            crate::settings::Setting {
                key: "reload-slows",
                group: crate::pause::GROUP_MENUS,
                label: "Slower while reloading",
                note: "On: you and the NPCs move at 75% of your speed while a reload runs. Off: reloading does not slow anyone, as in the original.",
                switches: "",
                get: |_| reload_slows(),
                set: |_, on| set_reload_slows(on),
                slider: None,
            },
        );
        crate::settings::add_setting(
            app,
            crate::settings::Setting {
                key: "localized-dt-dr",
                group: crate::pause::GROUP_MENUS,
                label: "Localized armor DT/DR",
                note: "On: head hits use only worn head armor DT/DR and body hits only worn body armor; explosions use all worn armor and innate DT/DR still applies. Off: full worn armor everywhere.",
                switches: "",
                get: |w| w.resource::<LocalizedDtDr>().0,
                set: |w, on| w.resource_mut::<LocalizedDtDr>().0 = on,
                slider: None,
            },
        );
        projectile_launch_probe::install(app);
        app.init_resource::<WeaponAnim>()
            .init_resource::<CombatStats>()
            .init_resource::<held_grenade_probe::Probe>()
            .init_resource::<ShotVisuals>()
            .add_message::<ShotFired>()
            .add_message::<ImpactFx>()
            .add_message::<CasingFx>()
            .add_message::<ExplosionFx>()
            .init_resource::<HitFeedback>()
            .init_resource::<HitMarks>()
            .init_resource::<PlayerLimbs>()
            .add_message::<ActorHit>()
            .add_message::<RemoteHit>()
            .add_message::<ForcedHitReaction>()
            .add_message::<ActorPush>()
            .add_message::<Knockback>()
            .add_message::<WeaponBroken>()
            .add_message::<ActorFire>()
            .add_message::<ActorStrike>()
            .init_resource::<crate::dismember::Dismemberment>()
            .add_message::<crate::dismember::Dismembered>()
            .add_systems(Startup, init_combat)
            .add_systems(Update, sync_localized_dt_dr.before(PlaySet::Activation).run_if(resource_exists::<Combat>))
            .add_systems(
                Update,
                (track_limbs, (perk_speed, stat_speed), player_flinch, arm_sway, weapon_state, equip_sync, run_pace, reload_pace, weapon_sounds, fire, melee, (actor_attacks, sync_player_anim_events).chain(), fly, crate::dismember::resolve, (crate::mines::tick, crate::grass_fire::tick).chain(), crate::thrown::land, disarm, death_drops, meltdown, log_combat_lines).chain().in_set(PlaySet::Activation).run_if(in_state(PlayState::Playing)).run_if(resource_exists::<Combat>),
            )
            // Another player's hits land while this player talks, too: the world stands
            // still behind a conversation, and a guest's hit that came then would be
            // dropped, so nobody could kill an NPC the host was talking to. Its death ends
            // the conversation (`fnv_game::Game::update_dialogue`). Without co-op no
            // [`RemoteHit`] is ever written, so this does nothing [design].
            .add_systems(Update, remote_hits.in_set(PlaySet::Activation).after(fly).before(crate::dismember::resolve).run_if(remote_hits_land).run_if(resource_exists::<Combat>))
            .add_systems(Update, (draw_marks, watch_hit_effects, leave_space_shots).run_if(resource_exists::<Combat>))
            .add_systems(Last, report);
    }
}

/// The player runs slower with a two-handed weapon drawn, by `fMove2HBigPenalty`
/// (`crate::player::RunPace`; the owner's playtest list 5 item 61) [design: every
/// two-handed weapon; hypothesis: the setting's role, from its name].
fn run_pace(c: Res<Combat>, control: Res<PlayerControl>, pace: Option<ResMut<crate::player::RunPace>>, tweaks: Option<Res<crate::tweaks::TweakSettings>>) {
    let Some(mut pace) = pace else { return };
    // The player's tweak: no slower run with a weapon drawn [design: docs/design/tweaks.md].
    let free = tweaks.is_some_and(|t| t.no_weapon_move_penalty);
    let two = !free && control.weapon_out && c.weapon.and_then(|w| c.forms.weapons.get(&w)).is_some_and(WeaponStats::two_handed);
    let want = if two { (1.0 - c.rules.move_2h_penalty).clamp(0.1, 1.0) } else { 1.0 };
    if pace.0 != want {
        pace.0 = want;
        info!("movement: running at {:.0}% of the speed ({})", want * 100.0, if two { "a two-handed weapon drawn" } else { "no two-handed weapon drawn" });
    }
}

/// The share of their speed the player and NPCs keep while they reload, with the
/// settings page's `reload-slows` on [design: owner 2026-10-27, "slow the player and npc's
/// down to about 75% move speed" during a reload; not the original's rule, which does not
/// slow a reload]. NPCs: `nvforever` limping.rs.
pub const RELOAD_PACE: f32 = 0.75;

/// The `reload-slows` switch, for the process (NPCs are slowed by another crate); on by
/// default [design: owner 2026-10-27].
static RELOAD_SLOWS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Whether a reload slows the one reloading.
pub fn reload_slows() -> bool {
    RELOAD_SLOWS.load(std::sync::atomic::Ordering::Relaxed)
}

/// The player's Equip Broken Items and Keep Broken Items Equipped tweaks: (either on, the
/// latter on), set by `crate::tweaks` each frame [design: docs/design/tweaks.md].
static BROKEN_KEPT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Set the broken-weapon tweaks: `equip` (Equip Broken Items), `keep` (Keep Broken Items
/// Equipped).
pub fn set_broken_kept(equip: bool, keep: bool) {
    BROKEN_KEPT.store(u8::from(equip || keep) | (u8::from(keep) << 1), std::sync::atomic::Ordering::Relaxed);
}

/// A broken weapon may stay in the player's hand (either tweak).
fn broken_kept() -> bool {
    BROKEN_KEPT.load(std::sync::atomic::Ordering::Relaxed) & 1 != 0
}

/// A player's weapon that breaks stays in the hand, without the message (Keep Broken Items
/// Equipped).
fn keep_on_break() -> bool {
    BROKEN_KEPT.load(std::sync::atomic::Ordering::Relaxed) & 2 != 0
}

/// The player's weapon in hand is broken (kept there by a tweak): it neither fires nor
/// swings until repaired [design: docs/design/tweaks.md].
fn broken_in_hand(sim: &SimState, id: FormId) -> bool {
    broken_kept() && item_condition(sim, FormId::PLAYER_REF, id) <= 0.0
}

/// The player's Unequip Broken Armor tweak, set by `crate::tweaks` each frame [design: docs/design/tweaks.md].
static UNEQUIP_BROKEN_ARMOR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The player's Weapon Volume Scaling tweak, set by `crate::tweaks` each frame: the
/// loudness of the player's shot sounds and of others' (as f32 bits; 1 = as recorded)
/// [design: docs/design/tweaks.md].
static WEAPON_VOLUME: [std::sync::atomic::AtomicU32; 2] = [std::sync::atomic::AtomicU32::new(0x3f80_0000), std::sync::atomic::AtomicU32::new(0x3f80_0000)];

/// Set the Weapon Volume Scaling tweak's loudness of the player's shots and of others'.
pub fn set_weapon_volume(player: f32, others: f32) {
    WEAPON_VOLUME[0].store(player.to_bits(), std::sync::atomic::Ordering::Relaxed);
    WEAPON_VOLUME[1].store(others.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// A shot's sound, the player's (`player`) or another's: as recorded, or under the
/// Weapon Volume Scaling tweak scaled.
fn shot_sound(sounds: &mut MessageWriter<PlaySound>, scaled: &mut MessageWriter<ScaledSound>, sound: FormId, at: Option<[f32; 3]>, player: bool) {
    match shot_gain(player) {
        None => {
            sounds.write(PlaySound { sound, at });
        }
        Some(gain) => {
            scaled.write(ScaledSound { sound, at, gain });
        }
    }
}

/// The Weapon Volume Scaling tweak's gain on a shot's sound, the player's or another's;
/// `None` at 1 (played as recorded).
fn shot_gain(player: bool) -> Option<f32> {
    let gain = f32::from_bits(WEAPON_VOLUME[usize::from(!player)].load(std::sync::atomic::Ordering::Relaxed));
    ((gain - 1.0).abs() >= 1e-4).then_some(gain)
}

/// Switch the Unequip Broken Armor tweak on or off.
pub fn set_unequip_broken_armor(on: bool) {
    UNEQUIP_BROKEN_ARMOR.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Switch the reload's slowing on or off.
pub fn set_reload_slows(on: bool) {
    RELOAD_SLOWS.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The share of its speed an actor keeps: [`RELOAD_PACE`] while it reloads with the
/// setting on, all of it otherwise.
pub fn reload_share(reloading: bool) -> f32 {
    if reloading && reload_slows() {
        RELOAD_PACE
    } else {
        1.0
    }
}

/// The player moves slower while a reload runs ([`RELOAD_PACE`],
/// `crate::player::ReloadPace`).
fn reload_pace(c: Res<Combat>, pace: Option<ResMut<crate::player::ReloadPace>>) {
    let Some(mut pace) = pace else { return };
    let want = reload_share(c.cycle.as_ref().is_some_and(Cycle::reloading));
    if pace.0 != want {
        pace.0 = want;
        info!("movement: moving at {:.0}% of the speed ({})", want * 100.0, if want < 1.0 { "reloading" } else { "not slowed by a reload" });
    }
}

/// Scribe Counter's window: a blow begun this long (combat seconds) after the player's
/// block stopped one is the counter [design: "quick counter-attack after Block", the
/// fandom wiki; the original's window is not documented].
pub const COUNTER_WINDOW: f32 = 1.0;

/// An unarmed special move the player's perks give a power attack (perk-entry-points.md
/// §11).
#[derive(Debug, Clone, Copy)]
struct SpecialMove {
    name: &'static str,
    /// Its clip's stem: the weapon's prefix and this are the clip file
    /// (`h2hattackcustom1power.kf`), perk-entry-points.md §11.3.
    stem: &'static str,
    /// The DOBJ's effect for the direction, put on with the blow.
    spell: Option<FormId>,
    /// Legion Assault knocks the target back [the fandom wiki, "Knock back opponents";
    /// the DOBJ names no effect for it].
    knock_back: bool,
    /// Khan Trick forces the target's hit reaction ([`ForcedHitReaction`]).
    forces_reaction: bool,
    /// Legion Assault's clip lunges forward: the player moves with it, by the clip's own
    /// travel (`crate::player::Lunge`).
    lunges: bool,
}


/// The special move a power attack in `dir` is, when the player has its perk.
fn special_move(script: Option<&mut ScriptState>, sim: &mut SimState, dir: fnv_combat::melee::PowerDir) -> Option<SpecialMove> {
    use fnv_combat::melee::PowerDir;
    use fnv_game::data::DefaultObject as D;
    use fnv_game::perks as p;
    // The clips [observed: `h2hattackcustom1power.kf` … in both rigs' folders of Fallout -
    // Meshes.bsa; custom1's Bip01 goes 332 units forward over its 1.6 s, a lunge;
    // documented: GECK wiki GetForceHitReaction, "Khan Trick (AttackCustom4Power and
    // AttackCustom5Power)"; hypothesis: custom2 for the back by elimination, 4 left and 5
    // right in the entry points' order].
    let (point, slot, name, stem) = match dir {
        PowerDir::Forward => (p::HAS_UNARMED_FORWARD_POWER_ATTACK, D::UnarmedForwardPa, "Legion Assault", "attackcustom1power"),
        PowerDir::Back => (p::HAS_UNARMED_BACK_POWER_ATTACK, D::UnarmedBackwardPa, "Ranger Takedown", "attackcustom2power"),
        PowerDir::Left => (p::HAS_UNARMED_LEFT_POWER_ATTACK, D::UnarmedLeftPa, "Khan Trick", "attackcustom4power"),
        PowerDir::Right => (p::HAS_UNARMED_RIGHT_POWER_ATTACK, D::UnarmedRightPa, "Khan Trick", "attackcustom5power"),
        PowerDir::Stand => return None,
    };
    let rt = script.and_then(|s| s.rt.as_mut())?;
    let (has, spell) = with_world(rt, sim, |rt| (rt.game.player_has_entry(point), rt.game.data.default_object(slot)));
    let side = matches!(dir, PowerDir::Left | PowerDir::Right);
    has.then_some(SpecialMove { name, stem, spell, knock_back: dir == PowerDir::Forward, forces_reaction: side, lunges: dir == PowerDir::Forward })
}

/// Scribe Counter: its clip `h2hcounter.kf` [observed: both rigs' folders; hypothesis:
/// the move by the name], the DOBJ's counter effect (none in FalloutNV.esm).
fn scribe_counter(script: Option<&mut ScriptState>, sim: &mut SimState) -> Option<SpecialMove> {
    let rt = script.and_then(|s| s.rt.as_mut())?;
    let (has, spell) = with_world(rt, sim, |rt| (rt.game.player_has_entry(fnv_game::perks::HAS_UNARMED_COUNTER_ATTACK), rt.game.data.default_object(fnv_game::data::DefaultObject::UnarmedCounterPa)));
    has.then_some(SpecialMove { name: "Scribe Counter", stem: "counter", spell, knock_back: false, forces_reaction: false, lunges: false })
}

/// Knock `target` down away along `dir`, as a blast's knockdown does; logged with `why`.
fn knock_down(script: Option<&mut ScriptState>, by: FormId, target: FormId, dir: V3, why: &str) -> bool {
    let shove = [dir[0] * BLAST_KNOCKDOWN_SPEED, dir[1] * BLAST_KNOCKDOWN_SPEED, BLAST_KNOCKDOWN_SPEED * fnv_game::unconscious::PUSH_LIFT];
    let down = script.and_then(|s| s.rt.as_mut()).is_some_and(|rt| rt.game.knock_down(target, Some(by), shove));
    if down {
        info!("combat: {why} knocks {target} down");
    } else {
        info!("combat: {why}: {target} cannot be knocked down (No Knockdowns, or seated or talking)");
    }
    down
}

/// The Knockdown Chance perks of `attacker`'s hit with `weapon` on `target` (Super Slam
/// 0.15 / 0.30, And Stay Back 0.1, Elijah's Last Words 0.25; perk-entry-points.md §11):
/// from 0, rolled once a hit.
fn perk_knockdown(c: &mut Combat, mut script: Option<&mut ScriptState>, sim: &mut SimState, attacker: FormId, weapon: FormId, target: FormId, dir: V3) {
    let (chance, by) = owner_weapon_perk(script.as_deref_mut(), sim, attacker, fnv_game::perks::KNOCKDOWN_CHANCE, weapon, 0.0);
    if chance <= 0.0 || by.is_empty() {
        return;
    }
    let roll = c.rng.next_f32();
    if roll >= chance {
        info!("combat: Knockdown Chance {chance:.2} ({}): rolled {roll:.2}, {target} stays up", by.join(", "));
        return;
    }
    knock_down(script, attacker, target, dir, &format!("Knockdown Chance {chance:.2} ({}, rolled {roll:.2})", by.join(", ")));
}

/// Meltdown (Has Meltdown, perk-entry-points.md §9): a foe the player (or a teammate with
/// the perk) kills with an Energy Weapons weapon gives off the DOBJ's Meltdown Explosion
/// (`MinePlasmaExplosion` in FalloutNV.esm) at its body, the killer's blast with the same
/// weapon, so a kill by it does the same again: "this can cause a chain reaction" [observed:
/// the perk's DESC]. A body gives one off once.
#[allow(clippy::too_many_arguments)]
fn meltdown(mut c: ResMut<Combat>, mut hits: MessageReader<ActorHit>, mut script: Option<ResMut<ScriptState>>, mut sim: ResMut<SimState>, phys: Res<PhysState>, mut done: Local<HashSet<u32>>) {
    let kills: Vec<(u32, FormId, FormId)> = hits.read().filter(|h| h.died).filter_map(|h| Some((h.form_id, h.by?, h.weapon?))).collect();
    for (victim, by, weapon) in kills {
        let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
        if done.contains(&victim) || (by != FormId::PLAYER_REF && !rt.game.is_teammate(by)) {
            continue;
        }
        let energy = rt.game.data.weapon_skills.get(&weapon).copied() == Some(fnv_game::actor_values::ENERGY_WEAPONS as i64);
        let (has, explosion) = with_world(rt, &mut sim, |rt| {
            let g = &mut rt.game;
            // The player's tweak: a teammate's energy kill uses the player's Meltdown [design: docs/design/tweaks.md].
            let who = if g.tweaks.teammates_meltdown && g.is_teammate(by) { FormId::PLAYER_REF } else { by };
            (energy && g.entry_point(who, fnv_game::perks::HAS_MELTDOWN, 0.0).0 != 0.0, g.data.default_object(fnv_game::data::DefaultObject::MeltdownExplosion))
        });
        if !has {
            continue;
        }
        let Some(explosion) = explosion else {
            warn!("combat: Meltdown: the load order's DOBJ names no Meltdown Explosion");
            continue;
        };
        let Some(at) = phys.world.actor_aim_point(victim).or_else(|| sim.world.peek(FormId(victim)).map(|r| [r.pos[0], r.pos[1], r.pos[2] + 60.0])) else { continue };
        done.insert(victim);
        info!("combat: Meltdown: {} gives off {} ({by}'s kill with {})", FormId(victim), c.forms.explosions.get(&explosion).and_then(|x| x.edid.as_deref()).unwrap_or("?"), c.forms.weapons.get(&weapon).and_then(|w| w.edid.as_deref()).unwrap_or("?"));
        c.meltdowns.insert(explosion);
        c.mine_explodes(explosion, at, Some((by, weapon)));
    }
}

/// The player's Modify Run Speed perks (Travel Light, Tunnel Runner, Implant M-5 …,
/// perk-entry-points.md §8): their factor on the player's movement, walking and running
/// alike, their conditions (armour, sneaking) asked every frame (`crate::player::PerkSpeed`).
fn perk_speed(c: Res<Combat>, mut anim: ResMut<WeaponAnim>, mut script: Option<ResMut<ScriptState>>, mut sim: ResMut<SimState>, speed: Option<ResMut<crate::player::PerkSpeed>>, inertia: Option<ResMut<crate::inertia::MovementInertia>>) {
    // The player's Agility and Strength for the draw's and the aim's pace (the original's
    // Agility rate, Weapon Handling's classes: `crate::inertia::MovementInertia::equip_rate`).
    if let Some(mut inertia) = inertia {
        let mut read = |code| actor_av(script.as_deref_mut(), &mut sim, FormId::PLAYER_REF, code).filter(|v| v.is_finite()).unwrap_or(5.0);
        let (agility, strength) = (read(fnv_game::actor_values::AGILITY), read(fnv_game::actor_values::STRENGTH));
        if inertia.agility != agility || inertia.strength != strength {
            inertia.agility = agility;
            inertia.strength = strength;
        }
    }
    // The Equip Speed perks (Quick Draw) on the weapon in hand: the draw and holster
    // clips play that much faster (`WeaponAnim::equip_rate`, perk-entry-points.md §10).
    if let Some(weapon) = c.weapon {
        let (eq, by) = player_weapon_perk(script.as_deref_mut(), &mut sim, fnv_game::perks::EQUIP_SPEED, weapon, 1.0);
        let eq = if eq.is_finite() && eq > 0.0 { eq } else { 1.0 };
        if (eq - anim.equip_rate).abs() > 1e-4 && (anim.equip_rate > 0.0 || !by.is_empty()) {
            info!("combat: draws and holsters at x{eq:.2}{}", if by.is_empty() { String::new() } else { format!(" ({})", by.join(", ")) });
        }
        anim.equip_rate = eq;
    }
    let (Some(mut speed), Some(rt)) = (speed, script.as_deref_mut().and_then(|s| s.rt.as_mut())) else { return };
    let (want, by) = with_world(rt, &mut sim, |rt| {
        let g = &mut rt.game;
        let (v, applied) = g.entry_point(FormId::PLAYER_REF, fnv_game::perks::MODIFY_RUN_SPEED, 1.0);
        (v as f32, applied.iter().map(|a| g.data.name(a.perk)).collect::<Vec<_>>())
    });
    let want = want.clamp(0.1, 5.0);
    if (speed.0 - want).abs() > 1e-4 {
        speed.0 = want;
        if by.is_empty() {
            info!("movement: no Modify Run Speed perks apply: the player moves at 100%");
        } else {
            info!("movement: the Modify Run Speed perks make the player move at {:.0}% ({})", want * 100.0, by.join(", "));
        }
    }
}

/// The player's Speed Mult, body armour and size (`crate::player::StatSpeed`): asked every
/// frame, so SetAV / ModAV / ForceAV, effects, changes of armour and SetScale count at
/// once. A smaller player takes smaller steps: the speed goes with a script's scale, as a
/// creature's pace goes with its scale (`creature_pace`) [hypothesis: the clips' root
/// motion shrinks with the body; owner's TTW run 2026-11-02, the baby at
/// `player.SetScale 0.4` moved at full speed].
fn stat_speed(mut script: Option<ResMut<ScriptState>>, mut sim: ResMut<SimState>, speed: Option<ResMut<crate::player::StatSpeed>>, mut player: Option<ResMut<crate::player::Player>>) {
    let (Some(mut speed), Some(rt)) = (speed, script.as_deref_mut().and_then(|s| s.rt.as_mut())) else { return };
    let (mult, armor) = with_world(rt, &mut sim, |rt| (rt.game.move_speed_mult(FormId::PLAYER_REF) as f32, rt.game.armor_speed_mult(FormId::PLAYER_REF)));
    let scale = sim.script_scale(FormId::PLAYER_REF.0).unwrap_or(1.0).max(crate::sim::MIN_DRAWN_SCALE);
    if let Some(p) = player.as_deref_mut() {
        p.eye_height = crate::player::EYE_HEIGHT * scale;
    }
    let want = mult * scale;
    // The player's tweak: Agility scales the jump, 1 + mult x (Agility - 1) [design: docs/design/tweaks.md].
    let k = rt.game.tweaks.agility_jump_mult;
    let jump = if k > 0.0 {
        let agility = with_world(rt, &mut sim, |rt| fnv_game::functions::actor_value_capped(&mut rt.game, FormId::PLAYER_REF, fnv_game::actor_values::AGILITY)) as f32;
        (1.0 + k * (agility - 1.0)).max(0.1)
    } else {
        1.0
    };
    // The player's tweak: the body armour's class sinks the swimmer, units a second
    // (ours) [design: docs/design/tweaks.md].
    let sink = if rt.game.tweaks.armor_sinks {
        match with_world(rt, &mut sim, |rt| fnv_game::combat::worn_armor_class(&rt.game.world, FormId::PLAYER_REF)) {
            fnv_sim::forms::ArmorClass::Heavy => 60.0,
            fnv_sim::forms::ArmorClass::Medium => 25.0,
            fnv_sim::forms::ArmorClass::Light => 0.0,
        }
    } else {
        0.0
    };
    if speed.2 != sink {
        speed.2 = sink;
        info!("movement: the player's armour sinks them at {sink:.0} units/s in water (tweak)");
    }
    if (speed.1 - jump).abs() > 1e-4 {
        speed.1 = jump;
        info!("movement: the player jumps {:.0}% as high (Agility tweak)", jump * 100.0);
    }
    if (speed.0 - want).abs() > 1e-4 {
        speed.0 = want;
        let size = if scale == 1.0 { String::new() } else { format!(", scale {scale:.2}") };
        info!("movement: Speed Mult and body armour make the player move at {:.0}% (armour {:.0}%{size})", want * 100.0, armor * 100.0);
    }
}

/// Read the player's limb conditions from the script runtime (step C8).
fn track_limbs(mut limbs: ResMut<PlayerLimbs>, mut script: Option<ResMut<ScriptState>>, mut sim: ResMut<SimState>) {
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    let (now, legs) = with_world(rt, &mut sim, |rt| (rt.game.limb_conditions(FormId::PLAYER_REF), rt.game.leg_speed_mult(FormId::PLAYER_REF)));
    let now = now.map(|v| v as f32);
    if legs as f32 != limbs.leg_speed {
        limbs.leg_speed = legs as f32;
        info!("movement: the player's legs allow {:.0}% of the speed", legs * 100.0);
    }
    if now != limbs.condition {
        let before = *limbs;
        limbs.condition = now;
        for c in 25u16..=31 {
            if limbs.crippled(c) != before.crippled(c) {
                info!("combat: player's {} {}", LIMB_NAMES[(c - 25) as usize], if limbs.crippled(c) { "crippled" } else { "healed" });
            }
        }
    }
}

/// The player flinches from a staggering hit (docs/spec-notes/crippled-limbs.md §3): the
/// hit that cripples a limb, a later hit on a crippled limb, or any hit while the torso
/// is crippled, by the rule NPCs stagger by (`damage::staggers`, with its cooldown after
/// the last flinch ended) [observed: `HelpCrippling`, "Chest: Higher chance to be
/// staggered", "These effects apply to you as well as to your enemies"; documented:
/// Fallout wiki crippling text, owner chat 2026-10-18, for the player a crippled torso
/// "throws off aim and interrupts attacks"; design: how]. The view is thrown off by up to
/// `flinch_kick_deg`, a swing under way is broken off, and for `flinch_time` no attack
/// starts (the trigger counts as released).
fn player_flinch(mut c: ResMut<Combat>, mut feedback: ResMut<HitFeedback>, mut player: ResMut<Player>, mut control: ResMut<PlayerControl>, mut anim: ResMut<WeaponAnim>, tweaks: Option<Res<crate::tweaks::TweakSettings>>) {
    // The player's tweak: the player never staggers [design: docs/design/tweaks.md].
    let no_stagger = tweaks.is_some_and(|t| t.no_player_stagger);
    let c = &mut *c;
    let now = c.time;
    for h in std::mem::take(&mut feedback.player_hits) {
        let since = c.flinch_end.map(|end| now - end);
        if no_stagger || now < c.flinch_until || !damage::staggers(&c.tuning, h.crippled, h.part_crippled, h.torso_crippled, since) {
            continue;
        }
        let (yaw, pitch) = ((c.flinch_rng.next_f32() * 2.0 - 1.0) * c.tuning.flinch_kick_deg, (c.flinch_rng.next_f32() * 2.0 - 1.0) * c.tuning.flinch_kick_deg * 0.5);
        player.heading += yaw.to_radians();
        player.pitch = (player.pitch + pitch.to_radians()).clamp(-MAX_PITCH, MAX_PITCH);
        c.flinch_until = now + c.tuning.flinch_time;
        c.flinch_end = Some(c.flinch_until);
        let broke = c.swings.swing.is_some();
        c.swings.reset();
        anim.swing = None;
        let why = if h.crippled {
            "crippled now"
        } else if h.part_crippled {
            "crippled part hit"
        } else {
            "torso crippled"
        };
        info!("combat: the player flinches for {:.1} s ({why}): view thrown off by {yaw:.1}°, {pitch:.1}°{}", c.tuning.flinch_time, if broke { ", the swing broken off" } else { "" });
    }
    if now < c.flinch_until {
        control.trigger = false;
    }
}

/// An actor other than the player drops the weapon in its hand when a hit cripples one of
/// its arms (docs/spec-notes/crippled-limbs.md §3) [observed: FalloutNV.esm MESG
/// `HelpCrippling`, "Arms: … Enemies will be disarmed at the moment their arm is
/// crippled"; hypothesis: either arm, every actor but the player, as the text names
/// enemies]. The weapon falls as a broken one does (`WeaponBroken`): a reference in front
/// of it, out of its inventory. A weapon with the "Can't Drop" flag stays in the hand
/// [documented: GECK wiki Weapon, "the weapon cannot be shot out from the hands of
/// actors"].
///
/// The player is disarmed too under the player's NPCs Can Disarm the Player tweak: the
/// weapon in hand falls in front of them as an actor's does [design: docs/design/tweaks.md].
#[allow(clippy::too_many_arguments)]
fn disarm(
    mut feedback: ResMut<HitFeedback>,
    mut sim: ResMut<SimState>,
    mut stats: ResMut<CombatStats>,
    parts: Res<ActorParts>,
    (c, script): (Res<Combat>, Option<Res<ScriptState>>),
    mut hud: Option<ResMut<HudMessage>>,
    mut broken: MessageWriter<WeaponBroken>,
) {
    let player_too = script.as_deref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.npcs_disarm_player);
    for actor in std::mem::take(&mut feedback.disarms) {
        let item = if actor == FormId::PLAYER_REF {
            if !player_too {
                continue;
            }
            // Not the Fists, nor a body weapon.
            c.weapon.filter(|w| sim.world.inventory_mut(actor).is_ok_and(|i| i.count(*w) > 0))
        } else {
            parts.by_ref.get(&actor.0).and_then(|p| p.weapon)
        };
        let Some(item) = item else { continue };
        if sim.forms.item(item).is_some_and(|it| it.cant_drop) {
            info!("combat: {actor}'s arm crippled: its {} ({item}) can't be dropped and stays in its hand", item_name(&sim, item));
            continue;
        }
        if sim.world.inventory_mut(actor).is_ok_and(|i| i.count(item) == 0) {
            let _ = sim.world.add_item(actor, item, 1);
        }
        let dropped = match sim.world.drop_equipped(actor, item) {
            Ok(refs) if !refs.is_empty() => Ok(refs),
            _ => sim.world.drop_item(actor, item, 1),
        };
        match dropped {
            Ok(refs) => {
                stats.disarmed += 1;
                let dropped = refs.first().copied();
                if actor == FormId::PLAYER_REF {
                    if let Some(h) = hud.as_deref_mut() {
                        h.show(format!("{} knocked from your hand", item_name(&sim, item)));
                    }
                }
                info!("combat: {actor}'s arm crippled: it drops its {} ({item}){}", item_name(&sim, item), dropped.map_or(String::new(), |r| format!(": reference {r}")));
                broken.write(WeaponBroken { form_id: actor.0, weapon: item, dropped });
            }
            Err(e) => warn!("combat: {actor}'s arm crippled: its {} could not be dropped: {e}", item_name(&sim, item)),
        }
    }
}

/// The share of a throw's speed a grenade dropped live by its dying holder leaves at, and
/// the height over the feet it leaves from, units [design].
const LIVE_DROP_SPEED: f32 = 0.05;
const LIVE_DROP_HEIGHT: f32 = 60.0;

/// The share of its PROJ speed a grenade the player throws leaves with: half, and the
/// other half by how much of the wind-up (attack pressed to the clip's `Hit` key,
/// `wind_up` s) attack was held, all of it when attack is still held as the throw leaves
/// (`holding`) [observed: the original's frag grenade, held 5, 10, 30, 60 and 120 game
/// loops, flew at 0.52, 0.60, 0.78, 0.99 and 0.98 of its 1,200 units/s, a tap by
/// `FireWeapon` at about 0.48; Heave Ho! multiplies on top, observer runs o32_throw
/// 2026-11-25/26, issue #1785, combat-rules.md §5 "Throw speed"; hypothesis: that the
/// share runs over our clip's wind-up (0.87 s for the frag) and stops at its `Hit` key,
/// which fits those figures as well as a rise over 1 s, and that only grenades do it].
fn throw_hold_share(held: f32, wind_up: f32, holding: bool) -> f32 {
    if holding || wind_up <= 0.0 {
        return 1.0;
    }
    0.5 + 0.5 * (held.max(0.0) / wind_up).min(1.0)
}

/// Whether `weapon` is a grenade: of the animation type "Grenade Throw (1 Hand)"
/// [documented: GECK wiki Weapons, Animation Type] and its projectile explodes (as
/// nvforever's `npc_grenades::carried`).
fn is_grenade(forms: &fnv_combat::data::CombatForms, weapon: FormId) -> bool {
    forms.weapons.get(&weapon).is_some_and(|w| w.anim_type.as_deref() == Some("Grenade Throw (1 Hand)"))
        && forms.chain(weapon, None).and_then(|ch| ch.projectile).and_then(|p| forms.projectiles.get(&p)).and_then(|p| p.explodes()).and_then(|e| forms.explosions.get(&e)).is_some_and(|e| e.radius > 0.0)
}

/// The default of `iDeathDropWeaponChance`, percent [observed: FalloutNV.esm, 100].
const DEATH_DROP_CHANCE: f32 = 100.0;

/// Actors seen alive with a weapon in the hand ([`death_drops`]).
#[derive(Default)]
struct DeathDrops {
    armed: HashMap<u32, FormId>,
    rng: Option<SplitMix>,
}

/// An actor other than the player that dies with a weapon in its hand drops it, with the
/// chance `iDeathDropWeaponChance` (percent; 100 in the masters) [hypothesis: the
/// setting's meaning, from its name; the original's corpses lie with their gun beside
/// them]. The weapon falls as a disarmed one does (`WeaponBroken`): a reference in front
/// of the body, out of its inventory. A weapon flagged "Can't Drop" stays with the body
/// [documented: GECK wiki Weapon, "the weapon cannot be shot out from the hands of
/// actors"; hypothesis: death too]. The dropped weapon stays lootable from the corpse:
/// its loot menu lists the reference beside the body's own items and taking it there
/// picks the one reference up (`RefState::dropped_by`; #1032) [hypothesis: the
/// original's corpses list the weapon lying beside them; owner's report]. Only deaths seen in play count: an actor dead when it
/// is drawn was never seen holding a weapon, so a corpse laid out by the game keeps its
/// own (`Game::starts_dead`). Another co-op player's actor drops nothing: its weapon is
/// that player's, in its own game, and it lies only while it is down (a dropped copy
/// could be picked up here) [design].
///
/// Under the player's NPCs Drop Live Grenades tweak, one that dies holding a grenade (a
/// thrown weapon that explodes, as `npc_grenades` throws) lets it fall live at its feet
/// instead: one taken from its inventory leaves its hand at [`LIVE_DROP_SPEED`] of a
/// throw's speed, downwards, and goes off as a thrown one [design: docs/design/tweaks.md].
#[allow(clippy::too_many_arguments)]
fn death_drops(
    mut st: Local<DeathDrops>,
    mut sim: ResMut<SimState>,
    mut stats: ResMut<CombatStats>,
    parts: Res<ActorParts>,
    script: Option<Res<ScriptState>>,
    (mut c, mut fire): (ResMut<Combat>, MessageWriter<ActorFire>),
    mut broken: MessageWriter<WeaponBroken>,
) {
    let Some(rt) = script.as_deref().and_then(|s| s.rt.as_ref()) else { return };
    for (&id, p) in &parts.by_ref {
        if id == FormId::PLAYER_REF.0 || rt.game.is_dead(FormId(id)) || rt.game.coop_actors.contains_key(&FormId(id)) {
            continue;
        }
        // The player's tweak: a weapon put away is dropped too [design: docs/design/tweaks.md].
        let holstered = || {
            rt.game.tweaks.npcs_drop_holstered
                .then(|| sim.world.equipment(FormId(id)).unwrap_or_default().into_iter().find(|&i| sim.forms.item(i).is_some_and(|it| it.kind == fnv_sim::forms::ItemKind::Weapon)))
                .flatten()
        };
        match p.weapon.or_else(holstered) {
            Some(item) => st.armed.insert(id, item),
            None => st.armed.remove(&id),
        };
    }
    let died: Vec<(FormId, FormId)> = st.armed.iter().filter(|(id, _)| rt.game.is_dead(FormId(**id))).map(|(id, item)| (FormId(*id), *item)).collect();
    for (actor, _) in &died {
        st.armed.remove(&actor.0);
    }
    if died.is_empty() {
        return;
    }
    let chance = sim.forms.setting_f32("iDeathDropWeaponChance").unwrap_or(DEATH_DROP_CHANCE);
    let live = rt.game.tweaks.npcs_drop_live_grenades;
    for (actor, item) in died {
        if live && is_grenade(&c.forms, item) && sim.world.remove_item(actor, item, 1).is_ok() {
            let at = sim.world.peek(actor).map_or([0.0; 3], |s| s.pos);
            let origin = [at[0], at[1], at[2] + LIVE_DROP_HEIGHT];
            fire.write(ActorFire { shooter: actor, weapon: item, origin, aim: [0.0, 0.0, -1.0], cone: 0.0, muzzle: None, harmless: false, sure: None });
            c.dropped_live.push(actor);
            stats.death_drops += 1;
            info!("combat: {actor} died holding its {} ({item}): it falls live (tweak)", item_name(&sim, item));
            continue;
        }
        let roll = st.rng.get_or_insert(SplitMix(5)).next_f32() * 100.0;
        if roll >= chance {
            info!("combat: {actor} died holding its {} ({item}): kept (roll {roll:.0} against iDeathDropWeaponChance {chance:.0})", item_name(&sim, item));
            continue;
        }
        if sim.forms.item(item).is_some_and(|it| it.cant_drop) {
            info!("combat: {actor} died holding its {} ({item}): it can't be dropped and stays with the body", item_name(&sim, item));
            continue;
        }
        if sim.world.inventory_mut(actor).is_ok_and(|i| i.count(item) == 0) {
            continue;
        }
        let dropped = match sim.world.drop_equipped(actor, item) {
            Ok(refs) if !refs.is_empty() => Ok(refs),
            _ => sim.world.drop_item(actor, item, 1),
        };
        match dropped {
            Ok(refs) => {
                stats.death_drops += 1;
                // The corpse's loot menu lists it, and taking it there picks this
                // reference up (`World::dropped_beside`, #1032).
                for r in &refs {
                    if let Ok(s) = sim.world.get(*r) {
                        s.dropped_by = Some(actor);
                    }
                }
                let dropped = refs.first().copied();
                info!("combat: {actor} died: it drops its {} ({item}){} (iDeathDropWeaponChance {chance:.0})", item_name(&sim, item), dropped.map_or(String::new(), |r| format!(": reference {r}")));
                broken.write(WeaponBroken { form_id: actor.0, weapon: item, dropped });
            }
            Err(e) => warn!("combat: {actor} died: its {} could not be dropped: {e}", item_name(&sim, item)),
        }
    }
}

/// The view sways while the player aims with a crippled arm (crippled-limbs.md §3): a
/// slow figure of eight of `arm_sway_deg` (one arm) or `arms_sway_deg` (both) at its
/// widest, which the shots follow as they leave along the view [documented: Fallout wiki
/// crippling text, owner chat 2026-10-18, "the first-person weapon sways, which shows
/// most through scopes"; design: the shape and size]. The view comes back when the arms
/// heal or the player stops aiming.
#[allow(clippy::type_complexity)]
fn arm_sway(time: Res<Time>, c: Res<Combat>, limbs: Res<PlayerLimbs>, control: Res<PlayerControl>, anim: Res<WeaponAnim>, mut player: ResMut<Player>, mut sway: Local<(f32, [f32; 2], bool)>) {
    let amp = match limbs.arms() {
        (true, true) => c.tuning.arms_sway_deg,
        (false, false) => 0.0,
        _ => c.tuning.arm_sway_deg,
    };
    let on = amp > 0.0 && control.aiming && anim.ready;
    let (t, last, was) = &mut *sway;
    if on != *was {
        info!("combat: the view {} (crippled arms)", if on { "sways while aiming" } else { "stops swaying" });
        *was = on;
    }
    let want = if on {
        *t += time.delta_secs();
        let a = *t / c.tuning.arm_sway_period.max(0.1) * std::f32::consts::TAU;
        [amp * a.sin(), 0.5 * amp * (2.0 * a).sin()]
    } else {
        *t = 0.0;
        [0.0, 0.0]
    };
    if want == *last {
        return;
    }
    player.heading += (want[0] - last[0]).to_radians();
    player.pitch = (player.pitch + (want[1] - last[1]).to_radians()).clamp(-MAX_PITCH, MAX_PITCH);
    *last = want;
}

/// Tell the script runtime whether the player's weapon is out (`IsWeaponOut`): drawn and
/// in hand, as the body reports it [documented: the value changes when the weapon is
/// attached to the weapon node, catalogue notes of IsWeaponOut].
fn weapon_state(anim: Res<WeaponAnim>, mut script: Option<ResMut<ScriptState>>) {
    if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
        rt.game.set_weapon_out(FormId::PLAYER_REF, anim.ready);
    }
}

/// The locomotion group name for a player movement direction and pace
/// (`fnv_actor::locomotion::Move::group`, `fnv_game::anims`).
pub fn movement_group(dir: fnv_game::anim_state::MoveDir, fast: bool) -> &'static str {
    use fnv_game::anim_state::MoveDir;
    match (dir, fast) {
        (MoveDir::None, _) => "Idle",
        (MoveDir::Forward, false) => "Forward",
        (MoveDir::Backward, false) => "Backward",
        (MoveDir::Left, false) => "Left",
        (MoveDir::Right, false) => "Right",
        (MoveDir::Forward, true) => "FastForward",
        (MoveDir::Backward, true) => "FastBackward",
        (MoveDir::Left, true) => "FastLeft",
        (MoveDir::Right, true) => "FastRight",
    }
}

/// Bridge the player's combat/weapon animation action and locomotion group into
/// `fnv_game::Game::set_anim_state`, firing JIP `SetOnAnimActionEventHandler`
/// (`animaction`) and `SetOnPlayGroupEventHandler` (`playgroup`) on state transitions
/// while preserving any renderer-driven non-weapon action (such as `PlayIdle` or jump
/// landing) when combat owns no action (`had_weapon_action`).
#[allow(clippy::too_many_arguments)]
pub fn bridge_player_anim_state(
    game: &mut fnv_game::game::Game<'_>,
    combat: &Combat,
    anim: &WeaponAnim,
    trigger: bool,
    aiming: bool,
    intent: Option<&MoveIntent>,
    dt: f32,
    had_weapon_action: &mut bool,
) {
    use fnv_game::anim_state::{AnimAction, AnimState, MoveDir};
    let weapon_action = game
        .has_pending_force_jam(FormId::PLAYER_REF)
        .then_some(AnimAction::Jam)
        .or_else(|| combat.player_anim_action(anim, trigger, aiming, dt));
    let prev = game.anim_state(FormId::PLAYER_REF).cloned().unwrap_or_default();
    let action = match weapon_action {
        Some(a) => {
            *had_weapon_action = true;
            Some(a)
        }
        None => {
            if *had_weapon_action {
                *had_weapon_action = false;
                None
            } else {
                prev.action
            }
        }
    };
    let (moving, group) = match intent {
        Some(i) => {
            let dir = MoveDir::of(i.right, i.forward, 0.01);
            (dir, Some(movement_group(dir, !i.walk)))
        }
        None => (prev.moving, prev.group),
    };
    let iron_sights = combat.player_sights_up(anim, aiming);
    let st = AnimState {
        action,
        moving,
        group,
        iron_sights,
        throwing: prev.throwing,
        mine: prev.mine,
        forced_hit: prev.forced_hit,
        clip: prev.clip,
        at: 0.0,
    };
    game.set_anim_state(FormId::PLAYER_REF, st);
}

/// Bridge live player combat/weapon animation and movement group transitions from
/// `CombatPlugin` into `ScriptState` inside `with_world` so `SetOnAnimActionEventHandler`
/// and `SetOnPlayGroupEventHandler` handlers fire with live world access.
#[allow(clippy::too_many_arguments)]
fn sync_player_anim_events(
    time: Res<Time>,
    combat: Res<Combat>,
    anim: Res<WeaponAnim>,
    control: Option<Res<PlayerControl>>,
    intent: Option<Res<MoveIntent>>,
    mut sim: ResMut<SimState>,
    mut script: Option<ResMut<ScriptState>>,
    mut had_weapon_action: Local<bool>,
) {
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    let dt = time.delta_secs().min(0.1);
    let trigger = control.as_deref().is_some_and(|c| c.trigger);
    let aiming = control.as_deref().is_some_and(|c| c.aiming);
    with_world(rt, &mut sim, |rt| {
        bridge_player_anim_state(&mut rt.game, &combat, &anim, trigger, aiming, intent.as_deref(), dt, &mut had_weapon_action);
    });
}

/// Production ECS entry point exposed for authored headless integration fixtures.
#[doc(hidden)]
pub fn init_combat(mut commands: Commands, d: Res<PlayWorld>) {
    let t = std::time::Instant::now();
    let tracer_hold = std::env::var("NVF_PLAY_TEST_TRACERS").ok().and_then(|v| v.trim().parse().ok()).filter(|s: &f32| *s > 0.0);
    let forms = CombatForms::build(d.lo, d.schema);
    info!("combat: {} weapons, {} ammo, {} projectiles, {} explosions read in {:.0} ms", forms.weapons.len(), forms.ammo.len(), forms.projectiles.len(), forms.explosions.len(), t.elapsed().as_secs_f64() * 1000.0);
    let mut combat = Combat::from_forms(forms);
    combat.no_spread = std::env::var_os("NVF_PLAY_TEST_NO_SPREAD").is_some_and(|v| v != "0");
    combat.tracer_hold = tracer_hold;
    commands.insert_resource(combat);
}

/// The player's spread perks for the weapon `id`: Modify Weapon Strength Req (points off
/// the requirement, from 0: Weapon Handling 2) and Modify Aiming Move Speed (the moving
/// penalty's divisor, from 1: Run 'n Gun × 2) [documented: GECK wiki Gun Spread Formula
/// (New Vegas), Perk entry points; perk-entry-points.md §12]. Logged when they change.
fn spread_perks(logged: &mut Option<(FormId, SpreadPerks)>, script: Option<&mut ScriptState>, sim: &mut SimState, id: FormId) -> SpreadPerks {
    let Some(rt) = script.and_then(|s| s.rt.as_mut()) else { return SpreadPerks::default() };
    let (cut, moving, by) = with_world(rt, sim, |rt| {
        let g = &mut rt.game;
        let (cut, a) = g.entry_point_with(FormId::PLAYER_REF, fnv_game::perks::MODIFY_WEAPON_STRENGTH_REQ, &[Some(id)], 0.0);
        let (moving, b) = g.entry_point_with(FormId::PLAYER_REF, fnv_game::perks::MODIFY_AIMING_MOVE_SPEED, &[Some(id)], 1.0);
        let by: Vec<String> = a.iter().chain(&b).map(|x| g.data.name(x.perk)).collect();
        (cut as f32, moving as f32, by)
    });
    let p = SpreadPerks { strength_req_cut: cut, aiming_move_divisor: moving.max(1.0) };
    if *logged != Some((id, p)) {
        *logged = Some((id, p));
        if !by.is_empty() {
            info!("combat: spread perks for {id}: Strength requirement -{cut:.0}, moving penalty / {:.2} ({})", p.aiming_move_divisor, by.join(", "));
        }
    }
    p
}

/// What [`spread_perks`] gives the player's stance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SpreadPerks {
    strength_req_cut: f32,
    aiming_move_divisor: f32,
}

impl Default for SpreadPerks {
    fn default() -> Self {
        SpreadPerks { strength_req_cut: 0.0, aiming_move_divisor: 1.0 }
    }
}

/// The player's stance for the spread: aiming, moving, running, sneaking, in the air,
/// the weapon's skill and Strength, crippled arms, the spread perks.
fn player_stance(control: &PlayerControl, phys: &PhysState, limbs: &PlayerLimbs, skill: f32, strength: f32, perks: SpreadPerks) -> Stance {
    Stance {
        aiming: control.aiming,
        moving: phys.player.as_ref().is_some_and(|ch| ch.velocity[0].hypot(ch.velocity[1]) > 1.0),
        running: control.always_run,
        sneaking: control.sneak,
        airborne: phys.player.as_ref().is_some_and(|ch| !ch.grounded && !ch.swimming),
        skill,
        strength,
        aiming_move_divisor: perks.aiming_move_divisor,
        strength_req_cut: perks.strength_req_cut,
        left_arm_crippled: limbs.arms().0,
        right_arm_crippled: limbs.arms().1,
        head_crippled: limbs.crippled(fnv_game::combat::HEAD),
    }
}

/// How many of `item` the player carries.
pub(crate) fn player_count(sim: &SimState, item: Option<FormId>) -> u32 {
    let inv = sim.world.peek(FormId::PLAYER_REF).and_then(|s| s.inventory.as_ref());
    item.and_then(|i| inv.map(|inv| inv.count(i))).unwrap_or(0).max(0) as u32
}

/// The rounds left in the magazine with `carried` of its ammo in the inventory, the
/// magazine's own among them: rounds dropped, sold or taken by a script are gone from the
/// gun too. A weapon that uses no ammo item, a regenerating one and god mode's full
/// magazine have no rounds in the inventory to follow.
/// ShowOff's shot switches as the scripts left them: the tracer override (`None` the
/// chance rolls) and whether hitscan shots are drawn (`fnv_game::nvse_showoff5`).
fn shot_switches(script: Option<&ScriptState>) -> (Option<bool>, bool) {
    script.and_then(|s| s.rt.as_ref()).map_or((None, false), |rt| (rt.game.showoff.b5.tracer_override, rt.game.showoff.b5.force_draw_hitscan))
}

fn magazine_in_hand(magazine: u32, carried: u32, no_ammo: bool, regenerates: bool, god: bool) -> u32 {
    if no_ammo || regenerates || god {
        magazine
    } else {
        magazine.min(carried)
    }
}

/// The HUD counter's pair: rounds in the weapon, rounds carried besides. The rounds of
/// a regenerating weapon (the Recharger Rifle) are its own charge and not taken from
/// the inventory, so the charge shows whole and nothing is taken off what is carried
/// (#2170) [hypothesis: the original's two figures for such a weapon, observer check 105].
fn hud_ammo(magazine: u32, carried: u32, regenerates: bool) -> (u32, u32) {
    if regenerates {
        (magazine, carried)
    } else {
        (magazine.min(carried), carried.saturating_sub(magazine))
    }
}

/// Follow the equipped weapon: a new weapon gets a new cycle, loaded from what is carried.
/// Production ECS entry point exposed for authored headless integration fixtures.
#[doc(hidden)]
pub fn equip_sync(mut c: ResMut<Combat>, mut sim: ResMut<SimState>, mut anim: ResMut<WeaponAnim>, tweaks: Option<Res<crate::tweaks::TweakSettings>>) {
    let keep_chosen = tweaks.as_deref().is_some_and(|t| t.keep_chosen_ammo);
    equip_sync_step(&mut c, &mut sim, &mut anim, keep_chosen);
}

fn equip_sync_step(c: &mut Combat, sim: &mut SimState, anim: &mut WeaponAnim, keep_chosen: bool) {
    let equipped = sim.world.peek(FormId::PLAYER_REF).and_then(|s| s.inventory.as_ref()).map(|i| i.equipped()).unwrap_or_default();
    let mut equipped = equipped.into_iter().find(|id| c.forms.weapons.contains_key(id));
    // A weapon at zero condition does not stay in the hand, however it got there (a
    // script's SetWeaponHealthPerc) (§17); under the player's Equip Broken Items or Keep
    // Broken Items Equipped tweak it does, and does not fire (`broken_in_hand`).
    if let Some(id) = equipped.filter(|&id| !broken_kept() && item_condition(sim, FormId::PLAYER_REF, id) <= 0.0) {
        let _ = sim.world.unequip(FormId::PLAYER_REF, id);
        info!("combat: the player's {} ({id}) is broken: unequipped", item_name(sim, id));
        equipped = None;
    }
    let mods = equipped.and_then(|id| sim.world.peek(FormId::PLAYER_REF).and_then(|s| s.inventory.as_ref()).map(|i| i.equipped_mods(id))).unwrap_or(0);
    if c.equipped == Some(equipped) && c.mods == mods {
        return;
    }
    // A mod put on the weapon in hand (`crate::pipboy::item_mod`): its rounds stay in.
    let kept = (c.equipped == Some(equipped)).then(|| c.cycle.as_ref().map(|cy| cy.magazine)).flatten();
    c.equipped = Some(equipped);
    c.mods = mods;
    let weapon = equipped.or(c.fists);
    c.weapon = weapon;
    c.cycle = None;
    c.ammo = None;
    c.melee = false;
    c.swings.reset();
    anim.swing = None;
    let Some(id) = weapon else {
        anim.attack = None;
        anim.reload = None;
        return;
    };
    let w = c.player_stats(id).expect("weapon present");
    let ammo = c.pick_equip_ammo(id, &w, keep_chosen, |a| player_count(sim, Some(a)) > 0);
    let chain = c.forms.chain_with(id, &w, ammo);
    let melee = AttackKind::of(&w) != AttackKind::Gun;
    c.melee = melee && chain.projectile.is_none();
    c.self_ammo = !melee && chain.ammo_choices.is_empty() && chain.projectile.is_some();
    c.ammo = chain.ammo;
    let carried = player_count(sim, c.ammo);
    let timing = Timing::of(&w, &anim.timing, &c.tuning);
    // Ranged weapons without a projectile (the detonator, binoculars) use no ammo.
    let no_ammo = melee || c.self_ammo || (chain.projectile.is_none() && chain.ammo_choices.is_empty());
    c.cycle = Some(Cycle::new(&w, timing, no_ammo, carried));
    if let (Some(n), Some(cy)) = (kept, c.cycle.as_mut()) {
        cy.magazine = n.min(cy.clip_size).min(carried);
    }
    // A save's magazine for this weapon (`crate::save`), no more than is carried.
    let mut restored_mag = false;
    if let (Some((sw, n)), Some(cy)) = (c.load_magazine, c.cycle.as_mut()) {
        if sw == id.0 {
            cy.magazine = n.min(carried);
            c.load_magazine = None;
            restored_mag = true;
        }
    }
    // An unequipped weapon's saved loaded round count (`fnv_sim::inventory::Extra::magazine`):
    // restored once on equip when no `load_magazine` or mod-keep override took precedence, then
    // cleared so vanilla unequip/re-equip behavior stays unchanged.
    if !restored_mag && kept.is_none() && !no_ammo {
        if let (Some(n), Some(cy)) = (
            sim.world.peek(FormId::PLAYER_REF).and_then(|s| s.inventory.as_ref()).and_then(|i| i.equipped_magazine(id)),
            c.cycle.as_mut(),
        ) {
            cy.magazine = n.min(carried);
            if let Ok(inv) = sim.world.inventory_mut(FormId::PLAYER_REF) {
                inv.map_equipped(id, |e| fnv_sim::inventory::Extra { magazine: None, ..e });
            }
        }
    }
    anim.attack = w.attack_anim.clone();
    anim.reload = w.reload_anim.clone();
    anim.jam = w.reload_anim.as_deref().and_then(jam_anim);
    info!(
        "combat: weapon {} ({id}), ammo {}, magazine {}/{}, {} projectile(s) per shot",
        w.edid.as_deref().unwrap_or("?"),
        c.ammo.and_then(|a| c.forms.ammo.get(&a)).and_then(|a| a.edid.clone()).unwrap_or_else(|| if c.self_ammo { "itself".into() } else { "none".into() }),
        c.cycle.as_ref().map_or(0, |cy| cy.magazine),
        w.clip_size,
        chain.projectiles_per_shot
    );
    if c.mods != 0 {
        info!("combat: weapon {} mods {} on: damage {}, clip {}, min spread {:.2}, sight FOV {:.0}", w.edid.as_deref().unwrap_or("?"), c.mods, w.base_damage, w.clip_size, w.min_spread, w.sight_fov);
    }
}

/// The actor base `actor`'s Model/Animation data comes from: its impact material and a
/// creature's impact data set are read there, not from the placed base (a leveled list,
/// or an actor whose template supplies them; docs/spec-notes/actors.md §1.3). The placed
/// base without the game.
fn model_base(script: Option<&ScriptState>, sim: &SimState, actor: u32) -> Option<FormId> {
    let b = sim.base_of(actor)?;
    let r = FormId(actor);
    Some(script.and_then(|s| s.rt.as_ref()).map_or(b, |rt| { let (seed, at) = sim.pick(&rt.game, r); rt.game.runtime_template_source(b, seed, at, |d| d.template_model).map_or(b, |(id, _)| id) }))
}

/// An actor's Impact Material Type (NPC_ / CREA `NAM4`) from its Model/Animation source
/// ([`model_base`]), organic without one [design].
fn actor_material(s: &Sfx, script: Option<&ScriptState>, sim: &SimState, actor: u32) -> &'static str {
    model_base(script, sim, actor).and_then(|b| s.forms.actor_material(b)).unwrap_or("organic")
}

/// The impact a weapon's impact data set names for what a hit struck, and the material
/// it was chosen by: an actor's is its Impact Material Type ([`actor_material`]);
/// anything else's its collision material (docs/spec-notes/audio.md §11). Shots and
/// melee blows alike (item 24).
fn impact_for(s: &Sfx, script: Option<&ScriptState>, sim: &SimState, weapon: FormId, actor: Option<ActorId>, material: Option<u32>) -> (Option<FormId>, &'static str) {
    let material = match actor {
        Some(a) => actor_material(s, script, sim, actor_ref(a)),
        None => fnv_combat::fx::impact_material(material),
    };
    let set = s.forms.weapons.get(&weapon).and_then(|w| w.impact_dataset);
    (set.and_then(|set| s.forms.impact_id(set, material)), material)
}

/// Show an impact (the renderer's model and debris, `ImpactFx`) and play its sounds there.
fn show_impact(s: &Sfx, script: Option<&ScriptState>, sim: &SimState, mut fx: ImpactFx, sounds: &mut MessageWriter<PlaySound>, impacts: &mut MessageWriter<ImpactFx>) {
    if let Some(a) = fx.actor {
        let (spray, decal) = blood_flags(script, sim, a);
        (fx.no_spray, fx.no_decal) = (fx.no_spray || spray, fx.no_decal || decal);
    }
    impacts.write(fx);
    if let Some(i) = s.forms.impacts.get(&fx.impact) {
        for sound in i.sound_1.into_iter().chain(i.sound_2) {
            sounds.write(PlaySound { sound, at: Some(fx.point) });
        }
    }
}

/// Whether actor `a` is flagged No Blood Spray and No Blood Decal (its Model/Animation
/// source's flags, docs/spec-notes/actor-flags.md §2); neither without a runtime.
fn blood_flags(script: Option<&ScriptState>, sim: &SimState, a: u32) -> (bool, bool) {
    // The base from the simulation: the game's world is not attached outside its turn.
    let (Some(rt), Some(b)) = (script.and_then(|s| s.rt.as_ref()), sim.base_of(a)) else { return (false, false) };
    let (seed, at) = sim.pick(&rt.game, FormId(a));
    rt.game.runtime_template_source(b, seed, at, |d| d.template_model).map_or((false, false), |(_, d)| (d.no_blood_spray, d.no_blood_decal))
}

/// How far from where a shot met an actor its blood can land on the world, units
/// [documented: GECK wiki ITR:OnWoundSpray, "a valid surface within 512 units of the
/// impact point in the projectile direction"].
pub const WORLD_BLOOD_REACH: f32 = 512.0;

/// The splatter chance without the `fCombatEnvironmentBloodChance` game setting (the
/// masters do not set it): its default [documented: GECK wiki
/// FCombatEnvironmentBloodChance, 0.7500].
pub const WORLD_BLOOD_CHANCE: f32 = 0.75;

/// The chance (0..=1) that a shot on an actor leaves blood on the world: the game setting
/// `fCombatEnvironmentBloodChance`, a fraction [documented: GECK wiki
/// FCombatEnvironmentBloodChance and ITR:OnWoundSpray], held to 0..=1;
/// [`WORLD_BLOOD_CHANCE`] without it.
pub fn blood_chance(setting: Option<f32>) -> f32 {
    setting.filter(|v| v.is_finite()).unwrap_or(WORLD_BLOOD_CHANCE).clamp(0.0, 1.0)
}

/// The reference a shot's actor id stands for: the player's for [`PLAYER`].
fn actor_ref(a: ActorId) -> u32 {
    if a == PLAYER {
        FormId::PLAYER_REF.0
    } else {
        a as u32
    }
}

/// The impact a shot's blood leaves on the world ([`world_blood`]): the struck body part's
/// blood spurt / decal impact data set (BPTD; the severable one, else the explodable
/// [hypothesis: observer check 263]) [documented: GECK wiki Body Part; ITR:OnWoundSpray,
/// "the body-part IPCT resolved for the hit limb"], its impact for the actor's Impact
/// Material Type. Not the weapon's impact data set, whose organic slot may hold another
/// effect (a flamer's scorch). None for a hit on the held weapon or a gore piece, a
/// material not organic (organic, organic bug, organic glow) [design], or a part without
/// such a set.
pub fn splatter_impact(forms: &fnv_combat::fx::FxForms, part: Option<u8>, body: Option<&BodyPart>, material: &str) -> Option<FormId> {
    if matches!(part, Some(crate::hitvol::WEAPON_PART | fnv_phys::gore::GORE_PART)) || !material.starts_with("organic") {
        return None;
    }
    let body = body?;
    forms.impact_id(body.severable_impact_dataset.or(body.explodable_impact_dataset)?, material)
}

/// The blood a shot on an actor leaves on the world (docs/spec-notes/impact-decals.md
/// §4a): `hit` is the impact sent for the actor (its No Blood Decal flag and the armour
/// tweak already in `no_decal`), `blood` the splatter's impact ([`splatter_impact`]),
/// `roll` in [0, 1) against `chance`. The first surface `surface` finds (from, unit
/// direction, reach → point and normal) along the shot, else straight down from the hit,
/// within [`WORLD_BLOOD_REACH`], gets `blood` as a decal only ([`ImpactFx::splatter`]);
/// none without a surface. At most one a hit
/// [documented: GECK wiki ITR:OnWoundSpray, "the engine breaks out of the raycast loop
/// on the first valid surface"]; the downward line is ours [hypothesis: observer check
/// 263, the wiki names only the projectile's direction].
pub fn world_blood(hit: &ImpactFx, blood: FormId, roll: f32, chance: f32, mut surface: impl FnMut(V3, V3, f32) -> Option<(V3, V3)>) -> Option<ImpactFx> {
    if hit.actor.is_none() || hit.no_decal || hit.splatter || roll >= chance {
        return None;
    }
    let along = vec::normalize(hit.dir);
    let lines = [along, [0.0, 0.0, -1.0]];
    let (point, normal, dir) = lines.into_iter().filter(|d| vec::len(*d) > 0.5).find_map(|d| surface(hit.point, d, WORLD_BLOOD_REACH).map(|(p, n)| (p, n, d)))?;
    Some(ImpactFx { impact: blood, shooter: FormId(0), point, normal, dir, actor: None, no_spray: true, no_decal: false, splatter: true })
}

/// Whether a projectile of `damage` (the weapon's per projectile, before armour) does not
/// get through a DT of `dt`: the player's Armor Prevents Blood Decals tweak leaves no blood
/// decal then [design: docs/design/tweaks.md].
pub fn blood_stopped(damage: f32, dt: f32) -> bool {
    dt > 0.0 && damage <= dt
}

/// The jam animation beside a reload animation: `ReloadA` → `JamA`, `ReloadX` → `JamX`
/// [observed: `1hpjama.kf`…`2hrjamz.kf` beside the reload clips in the meshes archive;
/// the letters' pairing is a hypothesis from the names].
pub fn jam_anim(reload: &str) -> Option<String> {
    let rest = reload.strip_prefix("Reload").or_else(|| reload.strip_prefix("reload"))?;
    (!rest.is_empty()).then(|| format!("Jam{rest}"))
}

/// Rays (shots, swings, blast lines) that passed a ghost's hit volume ([`ghosts`]), for
/// the exit summary.
static GHOST_PASSES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The physics world as combat sees it: rays that skip the player's capsule and the
/// layers nothing solid is on, and actor hit volumes.
struct PhysView<'a> {
    q: Queries<'a>,
    player: Option<Key>,
    /// The characters of the actors another system walks (`PhysState::npcs`).
    npcs: Vec<Key>,
    actors: Vec<(ActorId, V3, f32)>,
    /// Published scaled BBX widths used by actor blocking [design, #2535].
    actor_block_radii: &'a HashMap<u32, f32>,
    /// The corpses whose ragdoll took over from their hit volume (`crate::hitvol`), with
    /// their trunk's centre and the radius of their bodies' bounds: only blasts reach them
    /// ([`CombatWorld::blast_targets_near`], #2346).
    corpses: Vec<(ActorId, V3, f32)>,
    /// Each actor's body part capsule centres, widest first; a corpse's ragdoll bodies'
    /// centres, heaviest first.
    aims: HashMap<ActorId, Vec<V3>>,
    /// The ghosts (SetGhost 1) other than the player: rays pass their hit volumes
    /// ([`ghosts`]).
    ghosts: Vec<ActorId>,
    /// The layers [`CombatWorld::ray`] passes: a shot's (`layers::shot_ignores`), or a
    /// melee swing's (`layers::melee_ignores`, [`PhysView::for_melee`]).
    passes: layers::LayerSet,
    /// While set, the only actor a ray can strike ([`ActorFire::sure`]): every other
    /// actor's hit volume is passed. Set round the step of such a shot.
    only: std::cell::Cell<Option<ActorId>>,
}

impl PhysView<'_> {
    /// The same view for a melee swing's rays: they stop on the Transparent layer, which
    /// only projectiles pass (`layers::melee_ignores`; #2340).
    fn for_melee(self) -> Self {
        PhysView { passes: layers::melee_ignores(), ..self }
    }
}

impl CombatWorld for PhysView<'_> {
    fn ray(&self, from: V3, to: V3, ignore: &[ActorId]) -> Option<WorldHit> {
        let d = vec::sub(to, from);
        let len = vec::len(d);
        if len < 1e-4 {
            return None;
        }
        // Shots pass what the character passes (triggers, zones, water, …), invisible
        // walls and the plain Transparent layer (`layers::shot_ignores`; #2340): they
        // splash going into water (`water_surface`, `ProjEvent::Splash`) and fly on. A
        // melee view's rays stop on Transparent (`PhysView::for_melee`).
        let mut f = Filter { exclude_layers: self.passes, ..Default::default() };
        // Another actor's shot can hit the player's character; the player's own and
        // the rest (blasts, picks) pass it, as before.
        let other_shoots = ignore.iter().any(|&a| a != PLAYER);
        if !other_shoots {
            f.exclude.extend(self.player);
        }
        // The shooter's own hit volume and character.
        f.exclude.extend(ignore.iter().filter(|&&a| a != PLAYER).flat_map(|&a| [Key::Actor(a as u32), Key::Character(a as u32)]));
        // An actor's character (the capsule that walks it) is not its body: its hit
        // volumes are. A shot or a swing that met the character first struck it as a
        // wall, dust and no damage, and one past a flying or low creature's capsule
        // missed its body (the owner's playtest list 5 items 16 and 25) [design].
        f.exclude.extend(self.npcs.iter().copied());
        // A ghost's hit volume is passed; counted when it would have been struck.
        if !self.ghosts.is_empty() {
            let struck = self.q.ray(from, d, len, &f).map(|h| h.owner.key());
            if struck.is_some_and(|k| self.ghosts.iter().any(|&a| k == Key::Actor(a as u32))) {
                GHOST_PASSES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        f.exclude.extend(self.ghosts.iter().map(|&a| Key::Actor(a as u32)));
        // A sure shot passes every actor but the one it is for.
        if let Some(only) = self.only.get() {
            f.exclude.extend(self.actors.iter().filter(|a| a.0 != only).map(|a| Key::Actor(a.0 as u32)));
            if only != PLAYER {
                f.exclude.extend(self.player);
            }
        }
        let h = self.q.ray(from, d, len, &f)?;
        let player_hit = other_shoots && self.player == Some(h.owner.key());
        Some(WorldHit {
            point: h.point,
            normal: h.normal,
            distance: h.distance,
            actor: match h.owner {
                Owner::Actor { form_id, .. } => Some(form_id as ActorId),
                _ if player_hit => Some(PLAYER),
                _ => None,
            },
            part: match h.owner {
                Owner::Actor { part, .. } if part != fnv_phys::actors::WHOLE => Some(part),
                _ => None,
            },
            small_transparent: matches!(h.owner, Owner::Ref { layer, .. } if layer == layers::TRANSPARENT_SMALL || layer == layers::TRANSPARENT_SMALL_ANIM),
            material: h.material,
            object: match h.owner {
                Owner::Ref { form_id, .. } => Some(form_id as u64),
                _ => None,
            },
        })
    }

    fn cover_ray(&self, from: V3, to: V3) -> Option<WorldHit> {
        let d = vec::sub(to, from);
        let len = vec::len(d);
        if len < 1e-4 {
            return None;
        }
        let h = self.q.ray(from, d, len, &world_only())?;
        Some(WorldHit {
            point: h.point,
            normal: h.normal,
            distance: h.distance,
            actor: None,
            part: None,
            small_transparent: matches!(h.owner, Owner::Ref { layer, .. } if layer == layers::TRANSPARENT_SMALL || layer == layers::TRANSPARENT_SMALL_ANIM),
            material: h.material,
            object: match h.owner {
                Owner::Ref { form_id, .. } => Some(form_id as u64),
                _ => None,
            },
        })
    }

    fn actors_near(&self, point: V3, radius: f32) -> Vec<(ActorId, V3)> {
        // A blast passes a ghost too: it "cannot be harmed by a weapon or spell/magic
        // effect in any way" [documented: GECK wiki SetGhost notes] (no knock-down either).
        let near = |&&(id, c, r): &&(ActorId, V3, f32)| vec::len(vec::sub(c, point)) <= radius + r && !self.ghosts.contains(&id);
        self.actors.iter().filter(near).map(|&(id, c, _)| (id, c)).collect()
    }

    fn melee_targets_near(&self, point: V3, radius: f32) -> Vec<(ActorId, V3, f32)> {
        use fnv_combat::projectile::SWING_BODY_MARGIN;
        self.actors.iter().filter_map(|&(id, c, r)| {
            // Match blocking's BBX source, preserving the ordinary ground margin
            // and the candidate search's existing hit-volume allowance [design, #2535].
            let margin = crate::blocking::walking_radius(SWING_BODY_MARGIN, self.actor_block_radii.get(&(id as u32)).copied());
            (vec::len(vec::sub(c, point)) <= radius + margin + r && !self.ghosts.contains(&id)).then_some((id, c, margin))
        }).collect()
    }

    fn blast_targets_near(&self, point: V3, radius: f32) -> Vec<(ActorId, V3)> {
        let near = |&&(id, c, r): &&(ActorId, V3, f32)| vec::len(vec::sub(c, point)) <= radius + r && !self.ghosts.contains(&id);
        self.actors.iter().chain(&self.corpses).filter(near).map(|&(id, c, _)| (id, c)).collect()
    }

    fn aim_points(&self, actor: ActorId) -> Vec<V3> {
        self.aims.get(&actor).cloned().unwrap_or_default()
    }

    fn water_surface(&self, point: V3) -> Option<f32> {
        self.q.water_surface_for(point)
    }
}

/// The actors shots and blows pass through: ghosts (SetGhost 1), "Projectiles pass
/// through them" [documented: GECK wiki SetGhost notes]; a blow passes as a shot does
/// [hypothesis: the note names projectiles; a ghost "cannot be harmed by a weapon" in any
/// way, so a blow that stopped on it would only be wasted]. A ghosted player is still
/// struck, as "the player can" be damaged [documented: same notes; hypothesis: that its
/// body still stops projectiles]. Creatures whose base is flagged Is Ghost count as
/// ghosts too (docs/spec-notes/actor-flags.md §3).
fn ghosts(script: Option<&ScriptState>, phys: &PhysState, sim: Option<&SimState>) -> Vec<ActorId> {
    let (Some(rt), Some(sim)) = (script.and_then(|s| s.rt.as_ref()), sim) else { return Vec::new() };
    let set: Vec<ActorId> = rt.game.extra.iter().filter(|(r, e)| e.ghost && **r != FormId::PLAYER_REF).map(|(r, _)| r.0 as ActorId).collect();
    let flagged = phys.world.actor_volumes().filter(|&id| id != FormId::PLAYER_REF.0 && !set.contains(&(id as ActorId)) && sim.world.peek(FormId(id)).is_some_and(|s| rt.game.base_is_ghost(s.base, FormId(id))));
    let flagged: Vec<ActorId> = flagged.map(|id| id as ActorId).collect();
    set.into_iter().chain(flagged).collect()
}

/// The world only: no body, living or dead, shields another from a blast, nor anything
/// small or see-through (clutter, small debris, dropped weapons, fences, thin trees);
/// walls, rocks, the ground and wrecks do (the owner, 2026-10-27). Blood splatters land on
/// the same surfaces ([`world_surface`]).
fn world_only() -> Filter {
    let passes = layers::LayerSet::from_layers(&[
        layers::TRANSPARENT,
        layers::TRANSPARENT_SMALL,
        layers::TRANSPARENT_SMALL_ANIM,
        layers::INVISIBLE_WALL,
        layers::CLUTTER,
        layers::WEAPON,
        layers::PROJECTILE,
        layers::TREES,
        layers::BIPED,
        layers::DEAD_BIP,
    ]);
    Filter { exclude_layers: layers::character_ignores().union(passes), no_actors: true, ..Default::default() }
}

/// The first wall, rock, floor or wreck ([`world_only`]) on the line from `from` along
/// `dir` within `reach` units: its point and normal (game space).
fn world_surface(phys: &mut PhysState, from: V3, dir: V3, reach: f32) -> Option<(V3, V3)> {
    let h = phys.world.queries().ray(from, dir, reach, &world_only())?;
    Some((h.point, h.normal))
}

fn view<'p>(phys: &'p mut PhysState, script: Option<&ScriptState>, sim: Option<&SimState>) -> PhysView<'p> {
    let ghosts = ghosts(script, phys, sim);
    let mut actors: Vec<(ActorId, V3, f32)> =
        phys.world.actor_volumes().filter_map(|id| phys.world.actor_volume(id).map(|v| (id as ActorId, v.center(), v.radius))).collect();
    // The player has no hit volume (its capsule is its body), so it was never among the
    // actors a blast reaches: mines, grenades and an NPC's incinerator did it no harm
    // (the owner's playtest list 7 items 19 and 29). Its capsule's middle stands in for
    // it; swings and proximity fuses skip their own attacker, so only others' reach it.
    if let Some(ch) = phys.player.as_ref() {
        let h = ch.params.height.max(0.0);
        actors.push((PLAYER, [ch.feet[0], ch.feet[1], ch.feet[2] + h / 2.0], ch.params.radius));
    }
    // A corpse's hit volume is dropped once its ragdoll is built (`crate::hitvol`): blasts
    // reach it by its ragdoll's bodies instead, or a mine or grenade beside a settled corpse
    // never touched it (#2346) [design]. One entry per actor: a volume not yet dropped this
    // frame stands for it. Placed objects' ragdolls (traps) and the player are left out; gore
    // pieces are not ragdolls.
    let corpses: Vec<(ActorId, V3, f32)> = phys
        .world
        .ragdolls()
        .filter(|&id| id != FormId::PLAYER_REF.0 && phys.world.actor_volume(id).is_none())
        .filter_map(|id| phys.world.ragdoll_bounds(id).map(|(c, r)| (id as ActorId, c, r)))
        .collect();
    let player = phys.player.as_ref().map(|c| c.key());
    let npcs = phys.npcs.values().map(|b| b.ch.key()).collect();
    let mut aims: HashMap<ActorId, Vec<V3>> = actors.iter().map(|&(id, ..)| (id, phys.world.actor_part_centers(id as u32))).filter(|(_, v)| !v.is_empty()).collect();
    aims.extend(corpses.iter().map(|&(id, ..)| (id, phys.world.ragdoll_body_centers(id as u32))).filter(|(_, v)| !v.is_empty()));
    phys.world.update_queries();
    PhysView { q: phys.world.queries(), player, npcs, actors, actor_block_radii: &phys.actor_block_radii, corpses, aims, ghosts, passes: layers::shot_ignores(), only: std::cell::Cell::new(None) }
}

/// An actor's current value as the formulas read it (within its range: S.P.E.C.I.A.L.
/// 1–10, skills 0–100), from the script runtime's actor values (the record, the
/// formulas and whatever scripts changed: docs/spec-notes/actor-values.md); `None`
/// without a runtime.
pub(crate) fn actor_av(script: Option<&mut ScriptState>, sim: &mut SimState, actor: FormId, code: u16) -> Option<f32> {
    let rt = script.and_then(|s| s.rt.as_mut())?;
    Some(with_world(rt, sim, |rt| fnv_game::functions::actor_value_capped(&mut rt.game, actor, code)) as f32)
}

/// An actor's attacker values with weapon `w`: the weapon's skill, Strength, Luck and
/// Unarmed, what the Melee Damage, Unarmed Damage and Critical Chance actor values hold
/// above their formulas (scripts' changes), and its arms' condition for the weapon
/// (`damage::arm_condition`: the player's and every other actor's). Without a script
/// runtime, the defaults.
fn attacker(mut script: Option<&mut ScriptState>, sim: &mut SimState, rules: &Rules, p: FormId, w: &WeaponStats, aiming_vats: bool) -> Attacker {
    use fnv_game::actor_values as avs;
    let d = Attacker::default();
    let skill = w.skill.as_deref();
    let mut av = |code: u16, or: f32| actor_av(script.as_deref_mut(), sim, p, code).unwrap_or(or);
    let (left, right) = (av(fnv_game::combat::LEFT_ARM, 100.0) <= 0.0, av(fnv_game::combat::RIGHT_ARM, 100.0) <= 0.0);
    let (strength, luck, unarmed_skill) = (av(avs::STRENGTH, d.strength), av(avs::LUCK, d.luck), av(avs::UNARMED, d.unarmed_skill));
    let skill = skill.and_then(fnv_game::functions::av_by_name).map_or(d.skill, |code| av(code, d.skill));
    let melee = damage::melee_damage_av(rules, strength);
    let unarmed = damage::unarmed_damage_av(rules, unarmed_skill);
    let crit = rules.crit_luck_base + rules.crit_luck_mult * luck;
    Attacker {
        skill,
        strength,
        luck,
        unarmed_skill,
        melee_damage_bonus: av(avs::MELEE_DAMAGE, melee) - melee,
        unarmed_damage_bonus: av(avs::UNARMED_DAMAGE, unarmed) - unarmed,
        crit_chance_bonus: av(avs::CRITICAL_CHANCE, crit) - crit,
        arm_condition: damage::arm_condition(w, left, right),
        in_vats: aiming_vats,
        nerve: companion_nerve(script, sim, p),
        ..d
    }
}

/// Companion Nerve of `actor` (`Game::companion_nerve`): for one of the player's
/// teammates the share its damage and armour go up by, from the player's Charisma; 0
/// for anyone else or without a script runtime.
fn companion_nerve(script: Option<&mut ScriptState>, sim: &mut SimState, actor: FormId) -> f32 {
    let Some(rt) = script.and_then(|s| s.rt.as_mut()) else { return 0.0 };
    with_world(rt, sim, |rt| rt.game.companion_nerve(actor)) as f32
}

/// Whether a hit by `attacker` on `target` is a sneak attack (the sneak multiplier and
/// always a critical, docs/spec-notes/combat-rules.md §4): the player's, made sneaking
/// (`sneaking`: as a swing lands, as a shot left), on a target that does not have the
/// player in its Detected band now; a pair detection has not measured counts as unseen,
/// as does every target without a script runtime.
fn sneak_attack(script: Option<&ScriptState>, attacker: FormId, sneaking: bool, target: FormId) -> bool {
    use fnv_game::detection::Band;
    // The player's tweak: an NPC the player has not detected sneak-attacks the player
    // [design: docs/design/tweaks.md].
    if attacker != FormId::PLAYER_REF {
        return target == FormId::PLAYER_REF
            && script.and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.npcs_sneak_crit_player && rt.game.detection.get(FormId::PLAYER_REF, attacker).is_some_and(|d| d.band != Band::Detected));
    }
    // The player's tweak takes the sneak critical away [design: docs/design/tweaks.md].
    // The player's tweak: unseen is enough, standing or not [design: docs/design/tweaks.md].
    let standing = script.and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.standing_sneak_attacks);
    attacker == FormId::PLAYER_REF
        && (sneaking || standing)
        && !script.and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.no_sneak_criticals)
        && script.and_then(|s| s.rt.as_ref()).is_none_or(|rt| rt.game.detection.get(target, FormId::PLAYER_REF).is_none_or(|d| d.band != Band::Detected))
}

/// The hit log's note on a critical: a sneak attack's says so.
fn crit_note(a: &Attacker, d: &damage::Damage) -> &'static str {
    match (d.critical, a.sneaking) {
        (false, _) => "",
        (true, false) => ", critical",
        (true, true) => ", sneak attack critical",
    }
}

/// A perk entry's resolved function as combat applies it.
fn perk_op(op: fnv_game::perks::Op) -> damage::PerkOp {
    use fnv_game::perks::Op;
    match op {
        Op::Set(x) => damage::PerkOp::Set(x as f32),
        Op::Add(x) => damage::PerkOp::Add(x as f32),
        Op::Mul(x) => damage::PerkOp::Mul(x as f32),
        Op::Abs => damage::PerkOp::Abs,
        Op::NegAbs => damage::PerkOp::NegAbs,
    }
}

/// `owner`'s perk entries of the entry points `points`, each with the argument tabs
/// `tabs` and `other` as the owner's target (`fnv_game::Game::entry_ops`).
fn entry_ops(g: &mut fnv_game::Game, owner: FormId, points: &[u8], tabs: &[Option<FormId>], other: Option<FormId>) -> damage::EntryOps {
    let mut out = damage::EntryOps::default();
    for &point in points {
        for (perk, op) in g.entry_ops(owner, point, tabs, other) {
            let name = g.data.name(perk);
            out.0.push((point, perk_op(op), name));
        }
    }
    out
}

/// The perk entries of one hit (docs/spec-notes/perk-entry-points.md §2): the
/// attacker's weapon damage, critical chance and damage and the DT its hit ignores, with
/// the tabs Weapon and Target; the target's change to the attacker's critical chance
/// (Weapon, then the attacker [hypothesis: the second tab]) and its own DT (Attacker,
/// Attacker's weapon) [documented: GECK wiki GetPerkModifier, the entry functions'
/// arguments]. Only the player and its teammates have perks (a teammate the teammate
/// perks, `fnv_game::Game::held_perks`); without a script runtime, none.
fn hit_perks(script: Option<&mut ScriptState>, sim: &mut SimState, attacker: FormId, weapon: FormId, target: Option<FormId>) -> (damage::EntryOps, damage::EntryOps) {
    use fnv_game::perks as p;
    let Some(rt) = script.and_then(|s| s.rt.as_mut()) else { return Default::default() };
    let holds = |r: FormId| r == FormId::PLAYER_REF || rt.game.is_teammate(r);
    if !holds(attacker) && !target.is_some_and(holds) {
        return Default::default();
    }
    with_world(rt, sim, |rt| {
        let g = &mut rt.game;
        let mine = [p::CALCULATE_WEAPON_DAMAGE, p::CALCULATE_MY_CRITICAL_HIT_CHANCE, p::CALCULATE_MY_CRITICAL_HIT_DAMAGE, p::MODIFY_DAMAGE_THRESHOLD_ATTACKER];
        let a = entry_ops(g, attacker, &mine, &[Some(weapon), target], target);
        let t = match target {
            Some(t) => {
                let mut ops = entry_ops(g, t, &[p::MODIFY_ENEMY_CRITICAL_HIT_CHANCE], &[Some(weapon), Some(attacker)], Some(attacker));
                ops.0.extend(entry_ops(g, t, &[p::MODIFY_DAMAGE_THRESHOLD_DEFENDER], &[Some(attacker), Some(weapon)], Some(attacker)).0);
                ops
            }
            None => damage::EntryOps::default(),
        };
        (a, t)
    })
}

/// The perk entries of one blast on `target` (docs/spec-notes/perk-entry-points.md §2): of
/// [`hit_perks`], those a blast can use, the cause's weapon damage and the DT its hit
/// ignores, and the target's own DT. A blast with no cause has no attacker and no weapon
/// on those tabs, and nobody's stands in for them: what has no condition on them still
/// applies (Toughness), what has one does not (Hit the Deck) [design; hypothesis: that the
/// original does the same for a placed mine].
fn blast_entry_ops(g: &mut fnv_game::Game, cause: Option<Cause>, target: FormId) -> (damage::EntryOps, damage::EntryOps) {
    use fnv_game::perks as p;
    let a = cause.map_or_else(damage::EntryOps::default, |k| {
        entry_ops(g, k.actor, &[p::CALCULATE_WEAPON_DAMAGE, p::MODIFY_DAMAGE_THRESHOLD_ATTACKER], &[Some(k.weapon), Some(target)], Some(target))
    });
    let by = cause.map(|k| k.actor);
    let t = entry_ops(g, target, &[p::MODIFY_DAMAGE_THRESHOLD_DEFENDER], &[by, cause.map(|k| k.weapon)], by);
    (a, t)
}

/// [`blast_entry_ops`] in the played world; without a script runtime, none.
fn blast_perks(script: Option<&mut ScriptState>, sim: &mut SimState, cause: Option<Cause>, target: FormId) -> (damage::EntryOps, damage::EntryOps) {
    let Some(rt) = script.and_then(|s| s.rt.as_mut()) else { return Default::default() };
    with_world(rt, sim, |rt| blast_entry_ops(&mut rt.game, cause, target))
}

/// The player's value of entry point `point` with weapon `weapon` (the one argument tab
/// of spread, AP cost and the weapon's other figures) [documented: GECK wiki
/// GetPerkModifier], and the perks that changed it.
pub(crate) fn player_weapon_perk(script: Option<&mut ScriptState>, sim: &mut SimState, point: u8, weapon: FormId, v: f32) -> (f32, Vec<String>) {
    owner_weapon_perk(script, sim, FormId::PLAYER_REF, point, weapon, v)
}

/// [`player_weapon_perk`] for any owner: the player, or a teammate with the teammate
/// perks (`fnv_game::Game::held_perks`); others hold none.
pub(crate) fn owner_weapon_perk(script: Option<&mut ScriptState>, sim: &mut SimState, owner: FormId, point: u8, weapon: FormId, v: f32) -> (f32, Vec<String>) {
    use damage::PerkHooks;
    let Some(rt) = script.and_then(|s| s.rt.as_mut()) else { return (v, Vec::new()) };
    if owner != FormId::PLAYER_REF && !rt.game.is_teammate(owner) {
        return (v, Vec::new());
    }
    let ops = with_world(rt, sim, |rt| entry_ops(&mut rt.game, owner, &[point], &[Some(weapon)], None));
    (ops.entry(point, v), ops.names().into_iter().map(str::to_string).collect())
}

/// The item a spent round leaves the player (a case, a hull, a drained cell): the
/// ammo's DAT2 Consumed Ammo, given at its Consumed Percentage on every shot that spent
/// ammo [documented: GECK wiki Ammo, "Ammo Consumed (Percentage)": created in the
/// user's inventory "every time the weapon is fired"], the chance changed by the
/// player's Modify Chance for Ammo Item perks with the Weapon tab (Hand Loader and
/// Vigilant Recycler x 2) [documented: GECK wiki GetPerkModifier, entry 57's tab;
/// observed: the two perks' entries]. Not tied to Hardcore mode [hypothesis: the page
/// names no condition]. Its own rolls (`spent_rng`). docs/spec-notes/spent-ammo-items.md.
fn spent_ammo_item(rng: &mut SplitMix, stats: &mut CombatStats, script: Option<&mut ScriptState>, sim: &mut SimState, forms: &CombatForms, weapon: FormId, ammo: FormId) {
    let Some(a) = forms.ammo.get(&ammo) else { return };
    let Some((item, pct)) = a.consumed.filter(|(i, p)| i.0 != 0 && *p > 0.0) else { return };
    // The player's tweak: no spent items at all [design: docs/design/tweaks.md].
    if script.as_deref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.no_ammo_casings) {
        return;
    }
    let (pct, by) = player_weapon_perk(script, sim, fnv_game::perks::MODIFY_CHANCE_FOR_AMMO_ITEM, weapon, pct);
    stats.spent_rolls += 1;
    let roll = rng.next_f32() * 100.0;
    if roll >= pct {
        return;
    }
    if sim.world.add_item(FormId::PLAYER_REF, item, 1).is_err() {
        warn!("combat: the spent item {item} of {} is not an item", a.edid.as_deref().unwrap_or("?"));
        return;
    }
    *stats.spent_given.entry(item.0).or_default() += 1;
    info!(
        "combat: spent ammo item {item} from {} given to the player ({pct:.0} % chance{})",
        a.edid.as_deref().unwrap_or("?"),
        if by.is_empty() { String::new() } else { format!(" after {}", by.join(", ")) }
    );
}

/// Logs what the perks of a hit changed, when they changed anything.
fn log_hit_perks(attacker: FormId, target: Option<FormId>, a: &damage::EntryOps, t: &damage::EntryOps, d: &damage::Damage) {
    if a.is_empty() && t.is_empty() {
        return;
    }
    let names = |o: &damage::EntryOps| o.names().join(", ");
    info!(
        "combat: perks on {attacker}'s hit{}: attacker [{}], target [{}]: weapon damage {:.2} -> {:.2}, crit chance {:.1} %{}, crit damage {:.1}, DT {:.1}, dealt {:.2}",
        target.map_or(String::new(), |t| format!(" on {t}")),
        names(a),
        names(t),
        d.base,
        d.modified,
        d.crit_chance,
        if d.critical { " (critical)" } else { "" },
        d.crit_bonus,
        d.dt,
        d.dealt
    );
}

/// Logs what the perks of a blast changed, when they changed anything.
fn log_blast_perks(cause: Option<Cause>, target: FormId, a: &damage::EntryOps, t: &damage::EntryOps, d: &damage::BlastHit) {
    if a.is_empty() && t.is_empty() {
        return;
    }
    info!(
        "combat: perks on {}'s blast on {target}: attacker [{}], target [{}]: damage {:.2} -> {:.2}, DT {:.1}, dealt {:.2}",
        cause.map_or("no one".to_string(), |k| k.actor.to_string()),
        a.names().join(", "),
        t.names().join(", "),
        d.base,
        d.modified,
        d.dt,
        d.dealt
    );
}

/// The weapon's Equip sound when the player starts to draw it and its Unequip sound
/// when they start to put it away [hypothesis: when the game plays them]; its Idle sound
/// (`UNAM`) going round while it is out (docs/spec-notes/audio.md §15.2).
fn weapon_sounds(
    c: Res<Combat>,
    control: Res<PlayerControl>,
    sfx: Option<Res<Sfx>>,
    (mut sounds, mut loops): (MessageWriter<PlaySound>, MessageWriter<LoopSound>),
    mut was_out: Local<bool>,
    mut idle: Local<Option<FormId>>,
) {
    let out = control.weapon_out;
    let fx = c.weapon.zip(sfx.as_deref()).and_then(|(w, s)| s.forms.weapons.get(&w));
    if out != *was_out {
        *was_out = out;
        if let Some(sound) = fx.and_then(|f| if out { f.equip } else { f.unequip }) {
            sounds.write(PlaySound { sound, at: None });
        }
    }
    let wanted = fx.filter(|_| out).and_then(|f| f.idle);
    if *idle != wanted {
        if let Some(old) = *idle {
            loops.write(LoopSound { sound: old, on: false });
        }
        if let Some(sound) = wanted {
            debug!("combat: the player's weapon idles: {}", sound_edid(sfx.as_deref(), sound));
            loops.write(LoopSound { sound, on: true });
        }
        *idle = wanted;
    }
}

/// Turn the view by this frame's share of a kick `left` (pitch, heading; radians) and give
/// what is still to come and how far the view turned (less than asked at the pitch limit).
/// With `seconds` (soft recoil) the share follows an exponential that has 95 % of the
/// kick in by then: fastest at once, slower and slower to the end, never past it; a frame
/// of no time shows none. Without, all of it at once.
fn ease_kick(player: &mut Player, left: [f32; 2], seconds: Option<f32>, dt: f32) -> ([f32; 2], [f32; 2]) {
    if left == [0.0; 2] {
        return (left, [0.0; 2]);
    }
    let share = match seconds {
        Some(s) if s > 0.0 => 1.0 - (-3.0 * dt.max(0.0) / s).exp(),
        _ => 1.0,
    };
    let rest = left.map(|v| v * (1.0 - share));
    // The last thousandth of a degree comes at once.
    let (share, rest) = if rest[0].abs() + rest[1].abs() < 2e-5 { (1.0, [0.0; 2]) } else { (share, rest) };
    let before = player.pitch;
    player.pitch = (player.pitch + left[0] * share).clamp(-MAX_PITCH, MAX_PITCH);
    player.heading += left[1] * share;
    (rest, [player.pitch - before, left[1] * share])
}

/// With soft recoil, a share of the view's turn by a kick (`turned`, pitch and heading) to
/// settle back down by ([`ease_back`]): of the turn the view made, so a kick lost at the
/// pitch limit does not bring the view down after it (review, 2026-11-15).
fn kick_back(goal: &mut [f32; 2], turned: [f32; 2], soft: bool) {
    if soft {
        goal[0] += crate::inertia::KICK_BACK_SHARE * turned[0];
        goal[1] += crate::inertia::KICK_BACK_SHARE * turned[1];
    }
}

/// Settle the view back down by the kicks' returning share `goal` (pitch, heading; radians)
/// on critically damped springs over `seconds` (95 %): it starts gently while the kick
/// still rises, and ends slowly. Without `seconds` (soft recoil off) nothing returns
/// and what was still to return is dropped.
fn ease_back(player: &mut Player, goal: &mut [f32; 2], back: &mut [crate::ease::Spring; 2], seconds: Option<f32>, dt: f32) {
    let Some(s) = seconds else {
        *goal = [0.0; 2];
        *back = [crate::ease::Spring::default(); 2];
        return;
    };
    if *goal == [0.0; 2] {
        return;
    }
    let omega = crate::ease::omega_for(s);
    for k in 0..2 {
        let before = back[k].value;
        back[k].step(goal[k], omega, dt);
        let d = back[k].value - before;
        if k == 0 {
            player.pitch = (player.pitch - d).clamp(-MAX_PITCH, MAX_PITCH);
        } else {
            player.heading -= d;
        }
    }
    // Settled: what has returned is spent.
    if (0..2).all(|k| (goal[k] - back[k].value).abs() < 2e-5 && back[k].rate.abs() < 1e-4) {
        for k in 0..2 {
            let rest = goal[k] - back[k].value;
            if k == 0 {
                player.pitch = (player.pitch - rest).clamp(-MAX_PITCH, MAX_PITCH);
            } else {
                player.heading -= rest;
            }
        }
        *goal = [0.0; 2];
        *back = [crate::ease::Spring::default(); 2];
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn fire(
    time: Res<Time>,
    mut c: ResMut<Combat>,
    mut sim: ResMut<SimState>,
    mut phys: ResMut<PhysState>,
    mut player: ResMut<Player>,
    mut control: ResMut<PlayerControl>,
    mut anim: ResMut<WeaponAnim>,
    mut stats: ResMut<CombatStats>,
    mut script: Option<ResMut<ScriptState>>,
    limbs: Res<PlayerLimbs>,
    mut fired: MessageWriter<ShotFired>,
    sfx: Option<Res<Sfx>>,
    mut sounds: MessageWriter<PlaySound>,
    mut casings: MessageWriter<CasingFx>,
    mut loops: MessageWriter<LoopSound>,
    (mut hud, mut vats, lens_sway, inertia, mut scaled, tweaks): (Option<ResMut<HudMessage>>, Option<ResMut<crate::vats_mode::VatsMode>>, Option<Res<crate::aim_zoom::LensSway>>, Option<Res<crate::inertia::MovementInertia>>, MessageWriter<ScaledSound>, Option<Res<crate::tweaks::TweakSettings>>),
) {
    let keep_chosen = tweaks.as_deref().is_some_and(|t| t.keep_chosen_ammo);
    let dt = time.delta_secs();
    let c = &mut *c;
    c.time += dt;
    // The view's kick of the last shots, with soft recoil, comes in over a moment.
    let soft = inertia.as_deref().is_some_and(|i| i.soft_recoil());
    let kick_seconds = soft.then_some(crate::inertia::KICK_EASE_SECONDS);
    let pending = std::mem::take(&mut c.kick_left);
    let turned;
    (c.kick_left, turned) = ease_kick(&mut player, pending, kick_seconds, dt);
    kick_back(&mut c.kick_back_goal, turned, soft);
    let back_seconds = soft.then_some(crate::inertia::KICK_BACK_SECONDS);
    ease_back(&mut player, &mut c.kick_back_goal, &mut c.kick_back, back_seconds, dt);
    // Where the gun points: the view, turned by a 3D scope's sway (the view itself sways
    // otherwise).
    let gun_dir = lens_sway.as_deref().map_or_else(|| player.look_dir(), |s| s.look_dir(player.heading, player.pitch));
    // The attack clip passed its `Eject` key since the last frame (the casing is thrown
    // below, with the weapon's effects at hand).
    let eject_now = anim.ejects != c.ejects_seen;
    c.ejects_seen = anim.ejects;
    // VATS mode's marked shot due now: the point it goes straight at (vats-mode.md §9.1).
    let mut vats_shot = vats.as_deref_mut().and_then(|v| v.take_shot());
    // The projectiles VATS mode's marked shots landed on end now (the player's tweak).
    for (id, weapon) in vats.as_deref_mut().map(|v| v.take_shot_down()).unwrap_or_default() {
        match c.shoot_down(id, (FormId::PLAYER_REF, weapon)) {
            Some(blast) => info!("combat: VATS shot downs projectile {id}{}", if blast { ", its explosion goes off" } else { "" }),
            None => info!("combat: VATS shot at projectile {id}: it flies no more"),
        }
    }
    // A throw still winding up for the mode when the mode has ended (a jump, a menu) is
    // the mode's no longer: it leaves as any throw does, where the player looks, and not
    // at the point kept for it as a VATS shot [the owner's review of #2102].
    if c.vats_throw.is_some() && !vats.as_deref().is_some_and(|v| v.playing_back()) {
        c.vats_throw = None;
        info!("combat: the VATS mode throw is the mode's no longer (the mode ended while it wound up)");
    }
    // The looping shoot sound stops a moment after the last shot.
    if let Some((sound, left)) = c.looping.as_mut() {
        *left -= dt;
        if *left <= 0.0 {
            loops.write(LoopSound { sound: *sound, on: false });
            c.looping = None;
        }
    }
    // Bloom shrinks whatever happens, once the weapon has rested for the settle delay.
    // The recoil's climb stays: pulling the view back down is the player's (owner,
    // 2026-10-24).
    let was_settled = c.kick.settled();
    c.kick.recover(&c.tuning, dt);
    // Logged with this frame's crosshair cone, below.
    c.kick_just_settled |= !was_settled && c.kick.settled();
    let reload = std::mem::take(&mut control.reload);
    let swap = std::mem::take(&mut control.ammo_swap);
    // The player's tweak: reloading a full magazine swaps the ammo [design:
    // docs/design/tweaks.md].
    let full_swap = reload && script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.full_reload_swaps_ammo);
    // And reloading with none of the ammo in use left swaps to another carried type [design: docs/design/tweaks.md].
    let empty_swap = reload && script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.empty_reload_swaps_ammo);
    c.crosshair_cone = None;
    c.player_spread = None;
    // Scripts' EjectCasing requests (`fnv_game::nvse_jg10`) are taken every frame, so
    // those made with no gun out (or for other actors) do not pile up or fire later.
    let (asked, allow_attack, force_jam) = script
        .as_deref_mut()
        .and_then(|s| s.rt.as_mut())
        .map(|rt| {
            (
                rt.game.take_casing_requests(),
                rt.game.take_allow_attack(),
                rt.game.take_force_jam_requests().contains(&FormId::PLAYER_REF),
            )
        })
        .unwrap_or_default();
    // kNVSE `AllowAttack`: clear the player's current attack/cooldown/flinch lock so
    // the next attack may start at once [documented: GECK wiki AllowAttack; design].
    if allow_attack.contains(&FormId::PLAYER_REF) {
        if let Some(cycle) = c.cycle.as_mut() {
            cycle.phase = Phase::Ready;
        }
        c.swings.reset();
        anim.swing = None;
        c.flinch_until = 0.0;
    }
    if c.melee {
        c.vats_cone = None;
        return;
    }
    let Some(w) = c.weapon.and_then(|id| c.player_stats(id)) else {
        c.vats_cone = None;
        return;
    };
    let (Some(id), Some(cycle)) = (c.weapon, c.cycle.as_mut()) else { return };
    let god = god_mode();
    // Before the weapon's readiness: a holstered gun, the HUD and VATS mode see the same count.
    cycle.magazine = magazine_in_hand(cycle.magazine, player_count(&sim, c.ammo), cycle.no_ammo, cycle.regen.is_some(), god);
    if !anim.ready {
        if vats_shot.is_some() {
            info!("combat: the VATS mode shot could not leave (the weapon is not out)");
            if let Some(v) = vats.as_deref_mut() {
                v.throw_left();
            }
        }
        // The player's tweak: Ammo Swap works with the weapon put away; it reloads once
        // drawn [design: docs/design/tweaks.md].
        let holstered_swap = script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.swap_ammo_holstered);
        if swap && holstered_swap {
            if let Some(cur) = c.ammo {
                let next = c.forms.next_ammo(id, cur, |a| player_count(&sim, Some(a)) > 0);
                if let Some(n) = next.filter(|&n| n != cur) {
                    c.ammo = Some(n);
                    if keep_chosen {
                        c.chosen_ammo.insert(id, n);
                    }
                    if !god {
                        cycle.swap_ammo();
                    }
                    info!("combat: ammo swapped to {} with the weapon away (tweak)", c.forms.ammo.get(&n).and_then(|a| a.edid.clone()).unwrap_or_default());
                }
            }
        }
        return;
    }
    // The reload at the player's rate: Agility, then the Reload Speed perks (Rapid
    // Reload); the body plays the reload clips at the same rate (`WeaponAnim::reload_rate`).
    let agility = actor_av(script.as_deref_mut(), &mut sim, FormId::PLAYER_REF, fnv_game::actor_values::AGILITY).unwrap_or(5.0);
    let (rate, _) = player_weapon_perk(script.as_deref_mut(), &mut sim, fnv_game::perks::RELOAD_SPEED, id, fnv_combat::attack::reload_rate(agility));
    let rate = if rate.is_finite() && rate > 0.0 { rate } else { 1.0 };
    if (rate - anim.reload_rate).abs() > 1e-4 {
        info!("combat: reloads at x{rate:.2} (Agility {agility:.0})");
    }
    anim.reload_rate = rate;
    // The attack at the player's rate: the Modify Attack Speed perks (Fast Shot, Loose
    // Cannon, Trigger Discipline …, perk-entry-points.md §10); the body plays the attack
    // clip at the weapon's speed times that (`WeaponAnim::attack_rate`).
    let (atk, by) = player_weapon_perk(script.as_deref_mut(), &mut sim, fnv_game::perks::MODIFY_ATTACK_SPEED, id, 1.0);
    let atk = if atk.is_finite() && atk > 0.0 { atk } else { 1.0 };
    let clip_rate = w.attack_mult.max(0.01) * w.anim_mult.max(0.01) * atk;
    if (clip_rate - anim.attack_rate).abs() > 1e-4 {
        if by.is_empty() {
            info!("combat: attacks at x{clip_rate:.2} ({})", w.edid.as_deref().unwrap_or("?"));
        } else {
            info!("combat: attacks at x{clip_rate:.2} ({}; perks x{atk:.2}: {})", w.edid.as_deref().unwrap_or("?"), by.join(", "));
        }
    }
    anim.attack_rate = clip_rate;
    cycle.timing = Timing::of(&w, &anim.timing, &c.tuning).with_reload_rate(rate).with_attack_rate(atk);
    // The player's tweak: automatic weapons fire without spinning up [design: docs/design/tweaks.md].
    if cycle.automatic && script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.no_spin_up) {
        cycle.timing.spin_up = 0.0;
    }
    // A VATS mode shot leaves now, whatever the weapon's cooldown or wind-up (the owner's
    // "faster than the weapon's normal rate"), unless it is reloading, jammed or empty.
    // A throw is the exception: its clip starts now and it leaves the hand at the clip's
    // `Hit` key, as a throw outside the mode does, at the point kept till then
    // (`vats_throw`); the mode waits for it (`VatsMode::throw_left`). [observed: the fork
    // owner's playtest 2026-11-10: let go at once, the throw was seen twice, the clip from
    // its start in the mode's slow time and the rest of it after the mode; design.]
    if let Some(point) = vats_shot {
        if cycle.reloading() || cycle.jammed() || (!cycle.no_ammo && cycle.magazine < cycle.ammo_use) {
            info!("combat: the VATS mode shot could not leave (reloading, jammed or empty)");
            if let Some(v) = vats.as_deref_mut() {
                v.throw_left();
            }
        } else if c.self_ammo && cycle.timing.fire_delay > 0.0 {
            cycle.phase = fnv_combat::attack::Phase::WindUp { left: cycle.timing.fire_delay };
            anim.wind_ups += 1;
            c.vats_throw = Some(point);
            vats_shot = None;
            info!("combat: {} winds up for the VATS mode throw, it leaves at the clip's Hit key in {:.2} s", w.edid.as_deref().unwrap_or("?"), cycle.timing.fire_delay);
        } else {
            cycle.phase = fnv_combat::attack::Phase::WindUp { left: 0.0 };
        }
    }
    // Ammo Swap: the next carried type down the list, then a reload [design].
    let swap = swap || (full_swap && cycle.magazine >= cycle.clip_size && cycle.clip_size > 0) || (empty_swap && c.ammo.is_some() && player_count(&sim, c.ammo) == 0);
    if swap {
        if let Some(cur) = c.ammo {
            let next = c.forms.next_ammo(id, cur, |a| player_count(&sim, Some(a)) > 0);
            if let Some(n) = next.filter(|&n| n != cur) {
                c.ammo = Some(n);
                if keep_chosen {
                    c.chosen_ammo.insert(id, n);
                }
                // God mode's magazine stays full: its shot goes on and nothing reloads.
                if !god {
                    cycle.swap_ammo();
                }
                info!("combat: ammo swapped to {}", c.forms.ammo.get(&n).and_then(|a| a.edid.clone()).unwrap_or_default());
            }
        }
    }
    // God mode: no ammo is used and nothing needs reloading (the owner's playtest list 5
    // item 49) [design]: the magazine stays full and shots take nothing from the
    // inventory.
    if god && !cycle.no_ammo && cycle.regen.is_none() && !c.self_ammo {
        cycle.magazine = cycle.clip_size;
    }
    let total = if c.self_ammo { player_count(&sim, Some(id)) } else { player_count(&sim, c.ammo) };
    let carried = total.saturating_sub(cycle.magazine);
    let was_winding = matches!(cycle.phase, fnv_combat::attack::Phase::WindUp { .. });
    cycle.regen_frozen = script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.showoff.ammo_regen_frozen);
    // The player's tweaks on the firing cycle [design: docs/design/tweaks.md].
    if let Some(t) = script.as_ref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game.tweaks) {
        cycle.reload_in_recovery = t.reload_while_firing;
        cycle.semi_auto_queue = t.semi_auto_queue;
        cycle.reload_presses = t.per_bullet_reload.then_some((t.reload_rounds_per_press.max(1), t.reload_press_queue));
    }
    // The player's tweak: no reload until the magazine is empty [design: docs/design/tweaks.md].
    let empty_only = script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.reload_only_when_empty);
    let reload = reload && !(empty_only && cycle.magazine > 0);
    if c.self_ammo && control.trigger {
        if !c.grenade_held {
            (c.grenade_hold, c.grenade_held) = (0.0, true);
        }
        c.grenade_hold += dt;
    } else {
        c.grenade_held = false;
    }
    // The weapon's cycle goes at the player's own pace while a VATS camera shot plays (its
    // Player time multiplier), as the body's clips do: a throw leaves at its clip's key.
    let own_dt = dt * vats.as_deref().map_or(1.0, |v| v.player_rate());
    // The player's Hold and Release Throwables tweak: a throw wound up to its release waits
    // there while the trigger is held, and leaves when it is let go [design:
    // docs/design/tweaks.md]. Not VATS mode's throws.
    let hold_throw = c.self_ammo && control.trigger && c.vats_throw.is_none() && script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.hold_release_throws);
    let held_left = match cycle.phase {
        fnv_combat::attack::Phase::WindUp { left } if hold_throw && left <= own_dt => Some(left),
        _ => None,
    };
    if held_left.is_some() != anim.throw_held {
        info!("combat: {} {} (tweak)", w.edid.as_deref().unwrap_or("?"), if held_left.is_some() { "held at its release" } else { "let go" });
    }
    anim.throw_held = held_left.is_some();
    // Scripted ForceWeaponJamAnim / ForceJamAnim interrupts the current reload or attack
    // phase immediately before stepping the cycle [documented: GECK wiki ForceWeaponJamAnim].
    if force_jam && apply_forced_player_jam(cycle, &mut anim, &mut stats, &w, &c.tuning, script.as_deref()) {
        if c.vats_throw.take().is_some() {
            if let Some(v) = vats.as_deref_mut() {
                v.throw_left();
            }
        }
        vats_shot = None;
    }
    let own_dt = held_left.map_or(own_dt, |left| (left - 1e-4).max(0.0));
    let events = cycle.step(&c.tuning, Input { trigger: control.trigger && !broken_in_hand(&sim, id), reload: reload && !god, dt: own_dt, carried });
    // VATS mode's throw that wound up leaves now, at its point; one that no longer winds
    // up (the weapon went) is forgotten. Either way the mode goes on.
    let fired_now = events.iter().any(|e| matches!(e, Event::Fired { .. }));
    let throw_out = c.vats_throw.is_some() && (fired_now || !matches!(cycle.phase, fnv_combat::attack::Phase::WindUp { .. }));
    if throw_out || (fired_now && vats_shot.is_some()) {
        if let Some(v) = vats.as_deref_mut() {
            v.throw_left();
        }
    }
    let vats_shot = if throw_out { c.vats_throw.take().filter(|_| fired_now) } else { vats_shot };
    // A pull whose shot waits for the clip's `Hit` key: the clip starts now (`wind_ups`).
    if !was_winding && cycle.timing.fire_delay > 0.0 && matches!(cycle.phase, fnv_combat::attack::Phase::WindUp { .. }) {
        anim.wind_ups += 1;
        let what = if c.self_ammo { "throw" } else { "shot" };
        if c.self_ammo {
            info!("combat: {} winds up, the {what} leaves at the clip's Hit key in {:.2} s", w.edid.as_deref().unwrap_or("?"), cycle.timing.fire_delay);
        } else {
            debug!("combat: {} winds up, the {what} leaves at the clip's Hit key in {:.2} s", w.edid.as_deref().unwrap_or("?"), cycle.timing.fire_delay);
        }
    }
    let chain = c.forms.chain_with(id, &w, c.ammo);
    // The cone a shot would leave in now, for the crosshair (bloom included).
    {
        let effects: Vec<&AmmoEffect> = chain.ammo_effects.iter().filter_map(|e| c.forms.ammo_effects.get(e)).collect();
        let shooter = attacker(script.as_deref_mut(), &mut sim, &c.rules, FormId::PLAYER_REF, &w, false);
        let sp = spread_perks(&mut c.spread_perks_logged, script.as_deref_mut(), &mut sim, id);
        let stance = player_stance(&control, &phys, &limbs, shooter.skill, shooter.strength, sp);
        let actor_spread = spread::actor_spread(&c.rules, &c.tuning, &w, &stance) * wobble(script.as_deref(), FormId::PLAYER_REF);
        let (actor_spread, _) = player_weapon_perk(script.as_deref_mut(), &mut sim, fnv_game::perks::CALCULATE_GUN_SPREAD, id, actor_spread);
        let cone = spread::cone_deg(actor_spread.max(0.0), c.kick.bloom, &w, &effects);
        c.crosshair_cone = Some(if c.no_spread { 0.0 } else { cone });
        if std::mem::take(&mut c.kick_just_settled) {
            info!(
                "combat: kick settled {:.2} s after the last shot, crosshair cone {cone:.2} deg; the burst climbed the view {:.2} deg up, {:.2} deg sideways",
                c.kick.since_shot, c.kick.climb_deg, c.kick.climb_yaw_deg
            );
        }
        c.player_spread = Some(if c.no_spread { 0.0 } else { actor_spread.max(0.0) });
        // VATS mode's hit chance: its own actor spread, no bloom (vats-mode.md §17.2).
        let vats_spread = spread::vats_actor_spread(&c.rules, &c.tuning, &w, &stance);
        let (vats_spread, _) = player_weapon_perk(script.as_deref_mut(), &mut sim, fnv_game::perks::CALCULATE_GUN_SPREAD, id, vats_spread);
        c.vats_cone = Some(if c.no_spread { 0.0 } else { spread::cone_deg(vats_spread.max(0.0), 0.0, &w, &effects) });
    }
    let weapon_fx = sfx.as_deref().and_then(|s| s.forms.weapons.get(&id));
    if eject_now && anim.ejects_on_key {
        throw_casing(&mut casings, &anim, weapon_fx, id, &c.rules);
        info!("combat: {} ejects its casing at the clip's Eject key, {:.2} s after the shot", w.edid.as_deref().unwrap_or("?"), c.time - c.last_shot);
    }
    // Scripts' EjectCasing on the player (`fnv_game::nvse_jg10`): a casing from the
    // weapon's ShellCasingNode now. A node or model of the script's own and other
    // actors' casings are not drawn yet (docs/spec-notes/ext-jg10.md §3).
    if asked.iter().any(|r| r.actor == FormId::PLAYER_REF) {
        throw_casing(&mut casings, &anim, weapon_fx, id, &c.rules);
        info!("combat: a script's EjectCasing throws {}'s casing", w.edid.as_deref().unwrap_or("?"));
    }
    for e in events {
        match e {
            Event::SpinUp => {
                anim.spin_ups += 1;
                debug!("combat: {} spins up, first shot in {:.2} s", w.edid.as_deref().unwrap_or("?"), cycle.timing.spin_up);
            }
            Event::Fired { rounds } => {
                anim.attacks += 1;
                c.last_shot = c.time;
                stats.shots += 1;
                if rounds > 0 && !god {
                    if let Some(a) = c.ammo {
                        let _ = sim.world.remove_item(FormId::PLAYER_REF, a, rounds as i32);
                        spent_ammo_item(&mut c.spent_rng, &mut stats, script.as_deref_mut(), &mut sim, &c.forms, id, a);
                    }
                }
                if c.self_ammo && !god {
                    let _ = sim.world.remove_item(FormId::PLAYER_REF, id, 1);
                }
                if c.self_ammo {
                    info!("combat: {} leaves the hand {:.2} s into the attack clip", w.edid.as_deref().unwrap_or("?"), cycle.timing.fire_delay);
                }
                // The weapon's OnFire blocks (the detonator's DetonatePlacedExplosives),
                // and the player's Attack or FireExplosive line (a thrown weapon).
                if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
                    if with_world(rt, &mut sim, |rt| rt.game.weapon_fired(FormId::PLAYER_REF, id)) {
                        info!("combat: OnFire of {} ran", w.edid.as_deref().unwrap_or("?"));
                    }
                    with_world(rt, &mut sim, |rt| rt.game.attack_line(FormId::PLAYER_REF, None, false, c.self_ammo));
                }
                let launch = projectile_launch::resolve(&c.forms, &chain, script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game));
                let chain = launch.chain;
                let projectile = launch.projectile.as_ref();
                let Some(proj) = chain.projectile else {
                    debug!("combat: {} fires no projectile", w.edid.as_deref().unwrap_or("?"));
                    continue;
                };
                // The equipped unit's mods; none for a weapon that is not the equipped
                // one (the Fists) [design].
                let mods = if c.equipped.flatten() == Some(id) { c.mods } else { 0 };
                fired.write(ShotFired { projectile: proj, weapon: id, mods, shooter: FormId::PLAYER_REF, at: None, dir: None });
                // A casing for each shot of a weapon with a casing model, from its
                // `ShellCasingNode` at the game's eject speed (`fGunShellEjectSpeed`):
                // at the shot, unless the attack clip has an `Eject` key (a bolt, lever
                // or pump action), where it leaves when the clip reaches the key, below
                // [hypothesis: at the shot for the clips without the key].
                if !anim.ejects_on_key {
                    throw_casing(&mut casings, &anim, weapon_fx, id, &c.rules);
                }
                // A weapon with a looping shoot sound (the minigun has no other) plays
                // it for as long as shots keep coming [hypothesis: when the game does].
                if let Some(sound) = weapon_fx.and_then(|f| f.shoot_loop) {
                    if c.looping.is_none_or(|(s, _)| s != sound) {
                        if let Some((old, _)) = c.looping {
                            loops.write(LoopSound { sound: old, on: false });
                        }
                        loops.write(LoopSound { sound, on: true });
                    }
                    c.looping = Some((sound, LOOP_SOUND_HOLD));
                }
                // The player's own shot: the 2D sound where the weapon has one, the mod
                // set's with a Silence or Suppressor mod on (weapon-mods.md §7).
                let silenced = w.sound_mod_on(mods);
                let shoot = weapon_fx.map(|f| f.shoot_sounds(silenced)).unwrap_or_default();
                if let Some(sound) = shoot.own {
                    if shoot.silenced && c.silenced_logged != Some((id, sound)) {
                        info!("combat: {} shoots its silenced sound {}", w.edid.as_deref().unwrap_or("?"), sound_edid(sfx.as_deref(), sound));
                    }
                    c.silenced_logged = shoot.silenced.then_some((id, sound));
                    shot_sound(&mut sounds, &mut scaled, sound, None, true);
                }
                let effects: Vec<&AmmoEffect> = chain.ammo_effects.iter().filter_map(|e| c.forms.ammo_effects.get(e)).collect();
                let shooter = attacker(script.as_deref_mut(), &mut sim, &c.rules, FormId::PLAYER_REF, &w, false);
                let sp = spread_perks(&mut c.spread_perks_logged, script.as_deref_mut(), &mut sim, id);
                let stance = player_stance(&control, &phys, &limbs, shooter.skill, shooter.strength, sp);
                // The Calculate Gun Spread perks change the actor spread, not the weapon's
                // minimum (docs/spec-notes/perk-entry-points.md §2).
                let formula = spread::actor_spread(&c.rules, &c.tuning, &w, &stance) * wobble(script.as_deref(), FormId::PLAYER_REF);
                let (actor_spread, by) = player_weapon_perk(script.as_deref_mut(), &mut sim, fnv_game::perks::CALCULATE_GUN_SPREAD, id, formula);
                let actor_spread = actor_spread.max(0.0);
                if !by.is_empty() {
                    debug!("combat: perks on the player's spread: {formula:.2} -> {actor_spread:.2} ({})", by.join(", "));
                }
                // A VATS mode shot has no spread but its crippled limbs' (crippled-limbs.md §3).
                let cone = if c.no_spread {
                    0.0
                } else if vats_shot.is_some() {
                    spread::half_angle(spread::deviation_deg(spread::crippled_spread(&c.rules, &c.tuning, &w, &stance), 0.0))
                } else {
                    spread::cone_deg(actor_spread, c.kick.bloom, &w, &effects)
                };
                let eye = player.eye();
                let gravity = phys.world.params.gravity;
                let (tracer_chance, force, range) =
                    projectile.map_or((0.0, 0.0, 0.0), |p| (p.tracer_chance, p.impact_force, p.range));
                // The aim is the view ray from the eye; the shot is drawn from the
                // muzzle, and in third person leaves it when the barrel is behind cover
                // (docs/design/combat.md §5.1).
                let reach = if range > 0.0 { range } else { AIM_REACH };
                // In third person the view ray is the camera's, through the crosshair over
                // the shoulder: the shot aims from the eye at what lies under it, as the
                // eye's view ray does in first person (playtest list 4, item 12).
                // Heave Ho!'s Modify Throwing Velocity: "run when ANY projectile is
                // created … the velocity of ANY projectile spawned by the perk owner
                // (filtered by conditions)" [documented: GECK wiki Entry Point]
                // (perk-entry-points.md §8).
                let (throw_mult, throw_by) = player_weapon_perk(script.as_deref_mut(), &mut sim, fnv_game::perks::MODIFY_THROWING_VELOCITY, id, 1.0);
                let throw_mult = throw_mult.max(0.01);
                // A thrown grenade leaves faster the longer attack was held during its
                // wind-up (`throw_hold_share`); not VATS mode's throws, which aim at a
                // mark at the full speed.
                let hold = (c.self_ammo && vats_shot.is_none() && is_grenade(&c.forms, id))
                    .then(|| throw_hold_share(c.grenade_hold, cycle.timing.fire_delay, control.trigger));
                let leaving = if let Some(point) = vats_shot.filter(|_| c.self_ammo) {
                    // VATS mode's throw: from the eye on the low arc that lands on the mark
                    // (vats-mode.md §16.3), at 45° when it is out of reach; at the speed the
                    // throw will leave with.
                    let (speed, g) = projectile.map_or((0.0, 0.0), |p| (p.speed * throw_mult, p.fall() * gravity));
                    let aim = fnv_combat::vats::lob_dir(eye, point, speed, g).unwrap_or_else(|| fnv_combat::vats::lob_far(eye, point));
                    shot::Leaving { origin: eye, aim, aim_point: point, aim_distance: vec::len(vec::sub(point, eye)), seen: Seen::ON_PATH, covered: false }
                } else {
                    let world = view(&mut phys, script.as_deref(), Some(&sim));
                    let aim = match control.crosshair.as_ref() {
                        // VATS mode: from the eye straight at the mark (100 % accuracy).
                        _ if vats_shot.is_some() => {
                            let d = vec::sub(vats_shot.unwrap_or(eye), eye);
                            if vec::len(d) > 1e-3 {
                                vec::normalize(d)
                            } else {
                                gun_dir
                            }
                        }
                        Some(cross) => {
                            let (start, dir) = crate::control::Crosshair::ray(Some(cross), eye, gun_dir);
                            let end = vec::add(start, vec::scale(dir, reach));
                            let hit = world.ray(start, end, &[PLAYER]).map(|h| h.point);
                            crate::control::Crosshair::aim_from(eye, start, dir, reach, hit)
                        }
                        None => gun_dir,
                    };
                    let leaving = shot::leaving(&world, eye, aim, anim.muzzle, reach, Some(PLAYER), control.third_person_view());
                    // An arcing explosive round (a grenade launcher's) leaves the muzzle,
                    // along the aim: the original's 40 mm round started 110 / 125 / 138
                    // units above the feet and 55 ahead at aim 0 / 15 / 30 degrees up,
                    // where our weapon's muzzle is (110 / 126 / 142, 64 ahead), not at the
                    // eye; from the eye it fell 5 % short at 15 degrees [observed: o26_gl,
                    // combat-rules.md §5, issue #1471]. Not when the barrel is in a wall.
                    let arcing = vats_shot.is_none()
                        && chain.explosion.is_some()
                        && projectile.is_some_and(|p| p.gravity > 0.0 && !p.has("Hitscan") && p.kind.as_deref() == Some("Missile"));
                    match anim.muzzle {
                        Some(m) if arcing && !leaving.covered && world.ray(eye, m, &[PLAYER]).is_none() => shot::Leaving { origin: m, seen: Seen::ON_PATH, ..leaving },
                        _ => leaving,
                    }
                };
                let rng = &mut c.rng;
                let mut rand = || rng.next_f32();
                let mut shots = projectile.map_or_else(Vec::new, |p| shot::fire_projectile(p, &chain, leaving.origin, leaving.aim, cone, Some(PLAYER), gravity, &c.tuning, &mut rand));
                // A mod's Increase Projectile Speed (weapon-data-fields.md §5).
                if let Some(m) = w.projectile_speed_mult {
                    shots.iter_mut().for_each(|p| p.speed_up(m));
                }
                if let Some(share) = hold {
                    shots.iter_mut().for_each(|p| p.speed_up(share));
                    info!(
                        "combat: {} thrown after a hold of {:.2} s of its {:.2} s wind-up: x {share:.2} of its speed",
                        w.edid.as_deref().unwrap_or("?"),
                        c.grenade_hold,
                        cycle.timing.fire_delay
                    );
                }
                if (throw_mult - 1.0).abs() > 1e-4 {
                    shots.iter_mut().for_each(|p| p.speed_up(throw_mult));
                    if c.throw_logged != Some((id, throw_mult.to_bits())) {
                        info!("combat: perks change {}'s projectile speed x {throw_mult:.2} ({})", w.edid.as_deref().unwrap_or("?"), throw_by.join(", "));
                        c.throw_logged = Some((id, throw_mult.to_bits()));
                    }
                }
                stats.projectiles += shots.len() as u32;
                // ShowOff's SetProjectileTracerChanceOverride and SetForceDrawHitscanProjectiles.
                let (tracer_over, draw_hitscan) = shot_switches(script.as_deref());
                for p in shots {
                    let roll = c.rng.next_f32() < tracer_chance;
                    let rolled = tracer_over.map_or(roll, |o| o && tracer_chance > 0.0) || c.tracer_hold.is_some();
                    let shown = rolled || DRAW_EVERY_PROJECTILE || (draw_hitscan && p.hitscan);
                    // A line along the shot: for what is too fast to follow (found at
                    // once), and at the tracer chance for the rest [design].
                    let tracer = rolled || (shown && instant(&p));
                    let dir = vec::normalize(p.vel);
                    c.next_visual += 1;
                    let from_muzzle = anim.muzzle.is_some() && leaving.seen.at(leaving.origin, 0.0) != eye;
                    let by = By { actor: FormId::PLAYER_REF, weapon: id, ammo: c.ammo, harmless: false, sure: None, vats: vats_shot.is_some(), sneaking: control.sneak };
                    let vats_aim = vats_shot.filter(|_| p.explosion.is_some() && !p.mine);
                    c.flying.push(Flying { p, vats_aim, nearest: f32::INFINITY, told_off: false, id: c.next_visual, proj, shown, dir, from_muzzle, origin: leaving.origin, seen: leaving.seen, tracer, projectiles_per_shot: chain.projectiles_per_shot, force, by, rref: None, registered: false, speed_mult: 1.0, ended_at: None, gassed: Vec::new(), proj_zones_hit: Vec::new() });
                }
                let seen_from = leaving.seen.at(leaving.origin, 0.0);
                // The flash itself is the renderer's (`ShotFired`); the test aid marks
                // the point shots are drawn from.
                if let (Some(hold), true) = (c.tracer_hold, anim.muzzle.is_some()) {
                    c.marks.push(Mark::at(seen_from, hold, MarkKind::Flash));
                }
                stats.muzzle_shots += (seen_from != eye) as u32;
                stats.covered_shots += leaving.covered as u32;
                let p3 = |v: V3| format!("({:.1}, {:.1}, {:.1})", v[0], v[1], v[2]);
                debug!(
                    "combat: shot aimed from the eye {} at {} ({:.0} units), muzzle {}, drawn from {}{}",
                    p3(eye),
                    p3(leaving.aim_point),
                    leaving.aim_distance,
                    anim.muzzle.map_or("none (the eye)".to_string(), p3),
                    p3(seen_from),
                    if leaving.covered { "; cover in front of the barrel: it leaves the muzzle" } else { "" }
                );
                // VATS mode's shots kick nothing and grow no bloom [design, vats-mode.md §11.1],
                // nor do the NO_SPREAD test aid's, so each lands where it was aimed, nor a
                // throw (a grenade has no recoil).
                if vats_shot.is_none() && !c.no_spread && !c.self_ammo {
                    let rng = &mut c.kick_rng;
                    let kick = c.kick.shot(&c.tuning, &w, cycle.timing.interval, &mut || rng.next_f32());
                    // With soft recoil its first share shows this frame, the rest over the
                    // next ones; otherwise all at once, as the original.
                    let left = c.kick_left;
                    let (rest, turned) = ease_kick(&mut player, [kick.pitch_deg.to_radians(), kick.yaw_deg.to_radians()], kick_seconds, dt);
                    c.kick_left = [rest[0] + left[0], rest[1] + left[1]];
                    // A share of the turn the view then settles back down by (soft recoil).
                    kick_back(&mut c.kick_back_goal, turned, soft);
                    info!(
                        "combat: kick: shot {} in a row, bloom {:.2}, crosshair cone now {:.2} deg, view up {:.2} deg, sideways {:.2} deg",
                        c.kick.burst,
                        c.kick.bloom,
                        spread::cone_deg(actor_spread, c.kick.bloom, &w, &effects),
                        kick.pitch_deg,
                        kick.yaw_deg
                    );
                }
                debug!("combat: shot, cone {cone:.2}°, magazine {}", cycle.magazine);
                // The shot wears the weapon (§17); a thrown weapon is spent instead.
                if !c.self_ammo {
                    // Worn below the jamming mark, the shot may jam it (item 9): the jam
                    // clip plays and nothing fires until it ends. Weapons that use ammo
                    // only; a thrown one spends itself [design].
                    // The first shot after a reload of a weapon flagged No Jam After Reload
                    // does not jam [documented: the flag's name; hypothesis: the first shot;
                    // docs/spec-notes/item-fields.md §1].
                    let fresh = std::mem::take(&mut c.fresh_reload) && w.has_flag("No Jam After Reload");
                    // The player's tweak: no jams in god mode [design: docs/design/tweaks.md].
                    let no_jam = god && script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.god_mode_no_jams);
                    let jams = !no_jam && !fresh && vats_shot.is_none() && c.ammo.is_some() && c.rng.next_f32() < condition::jam_chance(&c.tuning, item_condition(&sim, FormId::PLAYER_REF, id));
                    let points = condition::weapon_wear(&c.rules, &w, &effects);
                    if wear_weapon(&c.forms, &mut sim, &mut stats, hud.as_deref_mut(), None, script.as_deref_mut(), FormId::PLAYER_REF, id, points, None).is_some_and(|w| w.broke) {
                        break;
                    }
                    if jams {
                        let at_reload_pace = script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.jam_at_reload_pace);
                        let secs = jam_duration(anim.timing.jam_len, w.jam_time, c.tuning.jam_fallback, anim.reload_rate, at_reload_pace);
                        cycle.jam(secs);
                        anim.jams += 1;
                        stats.jams += 1;
                        info!(
                            "combat: {} jammed at {}% condition (clip {}, {secs:.2} s)",
                            w.edid.as_deref().unwrap_or("?"),
                            condition::percent(item_condition(&sim, FormId::PLAYER_REF, id)),
                            anim.jam.as_deref().unwrap_or("none")
                        );
                        break;
                    }
                }
            }
            Event::ReloadStarted => {
                anim.reloads += 1;
                match cycle.timing.rounds {
                    Some(r) => info!("combat: reloading round by round (opens {:.2} s, {:.2} s a round, closes {:.2} s)", r.open, r.round, r.close),
                    None => debug!("combat: reloading"),
                }
            }
            Event::ReloadRound => anim.reload_rounds += 1,
            Event::RoundLoaded => info!("combat: a round in, magazine {}", cycle.magazine),
            Event::ReloadClosing { stopped } => {
                info!("combat: closing{}", if stopped { " (the trigger was pulled)" } else { "" });
            }
            Event::Reloaded { taken } => {
                stats.reloads += 1;
                c.fresh_reload = true;
                info!("combat: reloaded {taken}, magazine {}", cycle.magazine);
            }
            Event::DryFire => {
                if let Some(sound) = weapon_fx.and_then(|f| f.no_ammo) {
                    sounds.write(PlaySound { sound, at: None });
                }
                if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
                    with_world(rt, &mut sim, |rt| rt.game.combat_line(FormId::PLAYER_REF, None, "NoAmmo"));
                }
                stats.dry_fires += 1;
                info!("combat: click (no ammo)");
            }
            Event::Regenerated => {}
            Event::Cleared => info!("combat: the jam is cleared"),
        }
    }
    // Also apply ForceWeaponJamAnim / ForceJamAnim if a handler invoked during Event::Fired
    // (e.g. OnFire, SetOnFireWeaponEventHandler, ShowOff:OnFireWeapon) requested a jam.
    let fired_force_jam = script
        .as_deref_mut()
        .and_then(|s| s.rt.as_mut())
        .is_some_and(|rt| rt.game.take_force_jam_requests().contains(&FormId::PLAYER_REF));
    if fired_force_jam && !broken_in_hand(&sim, id) {
        apply_forced_player_jam(cycle, &mut anim, &mut stats, &w, &c.tuning, script.as_deref());
    }
}

/// Duration of a weapon jam: the loaded jam clip's length, else the weapon's `DNAM/jam_time`,
/// else `fallback`, scaled by `reload_rate` when `jam_at_reload_pace` is enabled
/// [design: docs/design/tweaks.md].
fn jam_duration(jam_len: Option<f32>, jam_time: f32, fallback: f32, reload_rate: f32, jam_at_reload_pace: bool) -> f32 {
    let secs = jam_len.or(Some(jam_time).filter(|&t| t > 0.0)).unwrap_or(fallback);
    if jam_at_reload_pace && reload_rate > 0.0 { secs / reload_rate } else { secs }
}

/// Apply a scripted `ForceWeaponJamAnim` / `ForceJamAnim` request to the player's weapon
/// cycle and `WeaponAnim`, interrupting any active reload or attack phase [documented:
/// GECK wiki ForceWeaponJamAnim]. Does not restart an already-active jam [design].
fn apply_forced_player_jam(
    cycle: &mut Cycle,
    anim: &mut WeaponAnim,
    stats: &mut CombatStats,
    w: &WeaponStats,
    tuning: &CombatTuning,
    script: Option<&ScriptState>,
) -> bool {
    if cycle.no_ammo || cycle.jammed() || anim.jam.is_none() || (anim.timing.jam_len.is_none() && w.jam_time <= 0.0) {
        return false;
    }
    let at_reload_pace = script.and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.jam_at_reload_pace);
    let secs = jam_duration(anim.timing.jam_len, w.jam_time, tuning.jam_fallback, anim.reload_rate, at_reload_pace);
    cycle.jam(secs);
    anim.jams += 1;
    stats.jams += 1;
    info!(
        "combat: {} forced jam by script (clip {}, {secs:.2} s)",
        w.edid.as_deref().unwrap_or("?"),
        anim.jam.as_deref().unwrap_or("none")
    );
    true
}

/// The target's DT and DR from the armour it wears (its carried ARMO by the wearing rule).
/// The simulation creates an actor's inventory on first touch (`World::inventory_mut`).
/// Each piece protects by its condition (§17).
pub(crate) fn target_armor(c: &Combat, script: Option<&mut ScriptState>, sim: &mut SimState, target: FormId) -> (f32, f32) {
    target_armor_for_zone(c, script, sim, target, None)
}

/// [`target_armor`] for a specific body hit `zone` when the optional `localized-dt-dr`
/// setting (`Combat::localized_dt_dr`, #3843) is enabled: head hits use worn head-armour
/// DT/DR only and body hits use worn body-armour DT/DR only [documented: JIP LN NVSE v57.30
/// / Settings INI v56.24 `jip_nvse.ini` `bLocalizedDTDR`; no DLL or source consulted];
/// explosions apply full worn armour (`Zone::All` / `None`) and innate actor DT/DR (AV 76 /
/// 18) still applies everywhere [observed: `tools/capture/nvse/probes/localized_dt_dr_probe.json`,
/// `w4e_c02_jip_localized_dtdr_explosions_innate`, 2026-11-24]. When `localized_dt_dr` is off
/// (default) or `zone` is `None`, sums all worn armour identically to vanilla.
pub(crate) fn target_armor_for_zone(c: &Combat, mut script: Option<&mut ScriptState>, sim: &mut SimState, target: FormId, zone: Option<Zone>) -> (f32, f32) {
    // The player's tweak: how fast worn-down armour loses its DT and DR [design: docs/design/tweaks.md].
    let penalty = script.as_deref().and_then(|s| s.rt.as_ref()).map_or(1.0, |rt| rt.game.tweaks.armor_condition_penalty as f32);
    let localized = zone.filter(|_| c.localized_dt_dr);
    let worn = worn_armor(&c.forms, sim, target);
    let armor_for = |pen: f32| match localized {
        Some(z) => c.forms.armor_values_worn_in_zone_with(&worn, pen, z),
        None => c.forms.armor_values_worn_with(&worn, pen),
    };
    let (dt, dr) = armor_for(penalty);
    // The player's tweaks: an NPC's armour keeps its whole DR or DT however worn [design: docs/design/tweaks.md].
    let (whole_dr, whole_dt) = script.as_deref().and_then(|s| s.rt.as_ref()).map_or((false, false), |rt| (rt.game.tweaks.npc_dr_ignores_condition, rt.game.tweaks.npc_dt_ignores_condition));
    let (dt, dr) = if target != FormId::PLAYER_REF && (whole_dr || whole_dt) {
        let (dt_whole, dr_whole) = armor_for(0.0);
        (if whole_dt { dt_whole } else { dt }, if whole_dr { dr_whole } else { dr })
    } else {
        (dt, dr)
    };
    // The actor's own Damage Threshold (76) and Damage Resistance (18) actor values:
    // nothing until a script changes them (abilities are not applied yet).
    let own_dt = actor_av(script.as_deref_mut(), sim, target, 76).unwrap_or(0.0);
    let own_dr = actor_av(script, sim, target, 18).unwrap_or(0.0);
    (dt + own_dt, dr + own_dr)
}

/// `target`'s value, in percent, of the resistance weapon `w`'s damage type names
/// (`damage::resistance_of`, docs/spec-notes/weapon-data-fields.md §3); 0 for a weapon
/// whose damage DR takes, or without a script runtime. Logged when above 0.
fn target_resist(script: Option<&mut ScriptState>, sim: &mut SimState, target: FormId, w: &WeaponStats) -> f32 {
    let Some(name) = damage::resistance_of(w) else { return 0.0 };
    let Some(code) = fnv_game::functions::av_by_name(name) else { return 0.0 };
    let v = actor_av(script, sim, target, code).unwrap_or(0.0);
    if v > 0.0 {
        info!("combat: {target} takes {} damage less {v:.0} % ({name}) as well as DR", w.edid.as_deref().unwrap_or("?"));
    }
    v
}

/// The armour `target` wears, each piece with its condition: what the player has
/// equipped; of another actor, what the rules dress it in (`World::equipment`: its
/// equipped items once outfitted, as the engine draws it), else its carried ARMO by the
/// wearing rule.
fn worn_armor(forms: &CombatForms, sim: &mut SimState, target: FormId) -> Vec<(FormId, f32)> {
    let dressed = if target == FormId::PLAYER_REF { None } else { sim.world.equipment(target) };
    let Ok(inv) = sim.world.inventory_mut(target) else { return Vec::new() };
    let items: Vec<FormId> = match dressed {
        Some(d) => d,
        None if target == FormId::PLAYER_REF => inv.equipped(),
        None => inv.iter().filter(|(_, n)| *n > 0).map(|(id, _)| id).collect(),
    };
    forms.worn_armor(items).into_iter().map(|id| (id, inv.condition_in_use(id).unwrap_or(1.0))).collect()
}

/// What one hit of the player's `weapon` on `target`'s `part` would take off its Health,
/// with no critical: the rules a landed shot goes through (`damage::hit`: the weapon's
/// figures with its mods and its condition, the player's skill and perks, the part's
/// damage multiplier, the target's DT, DR or resistance and its perks), with `power` as a
/// power or special attack's multiplier. For VATS' target screen (`crate::vats_mode`, the
/// enemy bar's damage in sight); a shot of several projectiles counts whole, as if all
/// of them hit. A weapon whose shot goes off (a launcher, a grenade) counts its explosion
/// too, as at the blast's middle, through the same armour (`damage::blast_hit`) [observed:
/// the fork owner's playtest 2026-11-10, a launcher showed no damage in sight: its own hit
/// is next to nothing, the blast is the damage]. `None` for a weapon without stats.
#[allow(clippy::too_many_arguments)]
pub fn vats_hit_estimate(c: &Combat, parts: &ActorParts, mut script: Option<&mut ScriptState>, sim: &mut SimState, weapon: FormId, target: u32, part: Option<u8>, power: f32) -> Option<f32> {
    let w = c.player_stats(weapon)?;
    let shooter = FormId::PLAYER_REF;
    let a = attacker(script.as_deref_mut(), sim, &c.rules, shooter, &w, true);
    let bp = c.vats_part(parts, target, part);
    let h = HitInput { weapon: &w, projectiles: 1, condition: item_condition(sim, shooter, weapon), ammo_effects: &[], part: bp.as_ref().map_or_else(HitPart::default, BodyPart::hit_part), power };
    let t = FormId(target);
    let (ap, tp) = hit_perks(script.as_deref_mut(), sim, shooter, weapon, Some(t));
    let (hit_dt, hit_dr) = target_armor_for_zone(c, script.as_deref_mut(), sim, t, Some(Zone::of(bp.as_ref())));
    let resist = target_resist(script.as_deref_mut(), sim, t, &w);
    // A roll of 1 is never a critical.
    let hit = damage::hit(&c.rules, &c.tuning, &h, &a, &Target { dt: hit_dt, dr: hit_dr, resist, ..Target::default() }, &ap, &tp, 1.0).dealt;
    let blast = c.forms.chain_with(weapon, &w, c.ammo).explosion.and_then(|e| c.forms.explosions.get(&e)).map_or(0.0, |x| {
        let (dt, dr) = target_armor(c, script.as_deref_mut(), sim, t);
        let raw = damage::explosion_damage(&c.tuning, x.damage, x.radius, 0.0);
        let (ap, tp) = blast_perks(script, sim, Some(Cause { actor: shooter, weapon }), t);
        damage::blast_hit(&c.rules, &c.tuning, raw, dt, dr, &ap, &tp).dealt
    });
    Some(hit + blast)
}

/// The condition of the unit of `item` that `actor` uses; 1 when it carries none (the
/// Fists).
fn item_condition(sim: &SimState, actor: FormId, item: FormId) -> f32 {
    sim.world.peek(actor).and_then(|s| s.inventory.as_ref()).and_then(|i| i.condition_in_use(item)).unwrap_or(1.0)
}

fn item_name(sim: &SimState, item: FormId) -> String {
    sim.name(item).map_or_else(|| item.to_string(), str::to_string)
}

/// In god mode (`tgm`) the player's weapon and armour lose no condition: firing, a swing,
/// a hit taken or a hit on the weapon held [design: owner decision 2026-11-15, issue
/// #2743]. Another actor's wear as ever.
pub fn keeps_condition(actor: FormId, god: bool) -> bool {
    god && actor == FormId::PLAYER_REF
}

/// A hit of `damage` (before armour) wears the armour `target` wears in `zone` (§17):
/// the player's only. Another actor's armour keeps its condition [observed: Cook-Cook's
/// armour stayed at health 85 and his DT at 13 through 60 hits, and the troopers' and
/// Fiends' through theirs, AI observer campaign fight_stats_round 2026-10-19,
/// npc-behaviour-compare.md §9].
fn wear_armor(c: &Combat, sim: &mut SimState, stats: &mut CombatStats, target: FormId, zone: Zone, damage: f32) {
    if target != FormId::PLAYER_REF || keeps_condition(target, god_mode()) {
        return;
    }
    let worn: Vec<FormId> = worn_armor(&c.forms, sim, target).into_iter().map(|w| w.0).collect();
    let struck = c.forms.armor_struck(&worn, zone);
    if struck.is_empty() || damage <= 0.0 {
        return;
    }
    let points = condition::armor_wear(&c.rules, damage) / struck.len() as f32;
    for item in struck {
        let health = c.forms.item_health(item);
        let Some(w) = sim.world.inventory_mut(target).ok().and_then(|inv| condition::wear_item(inv, item, points, health)) else { continue };
        stats.armor_wear += (w.before - w.after) * health as f32;
        info!("combat: {target}'s {} ({zone:?} hit) loses {points:.1}: condition {:.1}% -> {:.1}%", item_name(sim, item), w.before * 100.0, w.after * 100.0);
        if w.broke {
            stats.armor_broken += 1;
            info!("combat: {target}'s {} ({item}) is worn to nothing", item_name(sim, item));
            // The player's tweak: a broken piece comes off [design: docs/design/tweaks.md].
            if UNEQUIP_BROKEN_ARMOR.load(std::sync::atomic::Ordering::Relaxed) {
                match sim.world.unequip(target, item) {
                    Ok(_) => info!("combat: {target}'s broken {} is taken off (tweak)", item_name(sim, item)),
                    Err(e) => info!("combat: {target}'s broken {} stays on: {e}", item_name(sim, item)),
                }
            }
        }
    }
}

/// Take `points` health points from the weapon `item` that `actor` uses (§17). At zero
/// it breaks: the player's is unequipped and stays in the inventory, where it cannot be
/// equipped until it is repaired; another actor's falls to the ground (at `at`, when
/// given) and [`WeaponBroken`] tells the renderer, unless it has the "Can't Drop" flag:
/// then it is unequipped and stays in the inventory as the player's does [documented:
/// GECK wiki Weapon, a Can't Drop weapon "can still be disabled and rendered inoperable";
/// hypothesis: that it is unequipped rather than kept in the hand]. `None` when the weapon does not wear
/// (not carried, or its record has no health).
#[allow(clippy::too_many_arguments)]
pub fn wear_weapon(
    forms: &CombatForms,
    sim: &mut SimState,
    stats: &mut CombatStats,
    hud: Option<&mut HudMessage>,
    broken: Option<&mut MessageWriter<WeaponBroken>>,
    mut script: Option<&mut ScriptState>,
    actor: FormId,
    item: FormId,
    points: f32,
    at: Option<V3>,
) -> Option<Wear> {
    if keeps_condition(actor, god_mode()) {
        return None;
    }
    // The player's tweak: the companions' weapons do not wear [design: docs/design/tweaks.md].
    if actor != FormId::PLAYER_REF && script.as_deref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.no_companion_item_damage && rt.game.is_teammate(actor)) {
        return None;
    }
    // The player's Modify Item Damage perks (Regular Maintenance × 0.5, Built to
    // Destroy × 1.15) (docs/spec-notes/perk-entry-points.md §2).
    let points = if actor == FormId::PLAYER_REF {
        player_weapon_perk(script.as_deref_mut(), sim, fnv_game::perks::MODIFY_ITEM_DAMAGE, item, points).0.max(0.0)
    } else {
        points
    };
    // A creature's body weapon ("Embedded Weapon") is part of it and does not wear
    // [design: docs/design/combat-ai.md §19; what harms it is its limb, the record's
    // Embedded Weapon Actor Value].
    if forms.weapons.get(&item).is_some_and(|w| w.embedded) {
        return None;
    }
    let health = forms.item_health(item);
    let w = condition::wear_item(sim.world.inventory_mut(actor).ok()?, item, points, health)?;
    if actor == FormId::PLAYER_REF {
        stats.weapon_wear += (w.before - w.after) * health as f32;
    }
    if !w.broke {
        return Some(w);
    }
    stats.weapons_broken += 1;
    // Its owner says WeaponBroke (docs/spec-notes/combat-dialogue.md).
    if let Some(rt) = script.and_then(|s| s.rt.as_mut()) {
        with_world(rt, sim, |rt| rt.game.combat_line(actor, None, "WeaponBroke"));
    }
    let name = item_name(sim, item);
    if actor != FormId::PLAYER_REF && sim.forms.item(item).is_some_and(|it| it.cant_drop) {
        let _ = sim.world.unequip(actor, item);
        info!("combat: {actor}'s {name} ({item}) broke: it can't be dropped, so it is unequipped and kept");
        if let Some(b) = broken {
            b.write(WeaponBroken { form_id: actor.0, weapon: item, dropped: None });
        }
        return Some(w);
    }
    if actor == FormId::PLAYER_REF && keep_on_break() {
        info!("combat: the player's {name} ({item}) broke: kept in the hand (Keep Broken Items Equipped)");
        return Some(w);
    }
    if actor == FormId::PLAYER_REF {
        let _ = sim.world.unequip(actor, item);
        if let Some(h) = hud {
            h.show(format!("{name} is broken"));
        }
        info!("combat: the player's {name} ({item}) broke: unequipped, it stays in the inventory until repaired");
        return Some(w);
    }
    let dropped = match sim.world.drop_equipped(actor, item) {
        Ok(refs) => refs.first().copied(),
        Err(e) => {
            warn!("combat: {actor}'s broken {name} could not be dropped: {e}");
            None
        }
    };
    if let (Some(r), Some(at)) = (dropped, at) {
        if let Ok(s) = sim.world.get(r) {
            s.pos = at;
        }
    }
    if let Some(h) = hud {
        h.show(format!("{name} broken"));
    }
    info!("combat: {actor}'s {name} ({item}) broke and fell{}", dropped.map_or(String::new(), |r| format!(": reference {r}")));
    if let Some(b) = broken {
        b.write(WeaponBroken { form_id: actor.0, weapon: item, dropped });
    }
    Some(w)
}

/// An actor other than the player wore its weapon by its own attack (§17): `points`
/// come off the unit it holds, counted for the exit summary. One broken by it falls
/// where it is (`at`) as one hit to nothing, and its owner says WeaponBroke. A weapon
/// the simulation's inventory lacks (the renderer's choice, `strike_weapon`) does not
/// wear [design].
///
/// Only a weapon whose record sets its own Damage to Weapon Mult wears from an NPC's
/// attacks [observed: observer captures o14_reach and o15_round, 2026-11-01 (#1334): a
/// lead pipe, sledgehammer, 10mm Pistol and Hunting Rifle stayed at their exact health
/// through 10-42 attacks each, against god-mode and mortal players; Cook-Cook's
/// Incinerator, which has the override, wore. That the override is what decides is
/// [hypothesis], the one rule both fit].
#[allow(clippy::too_many_arguments)]
fn wear_actor_weapon(
    forms: &CombatForms,
    sim: &mut SimState,
    stats: &mut CombatStats,
    broken: &mut MessageWriter<WeaponBroken>,
    script: Option<&mut ScriptState>,
    actor: FormId,
    item: FormId,
    points: f32,
    at: V3,
) {
    if actor == FormId::PLAYER_REF || points <= 0.0 || !npc_weapon_wears(forms, item) {
        return;
    }
    let Some(w) = wear_weapon(forms, sim, stats, None, Some(broken), script, actor, item, points, Some(at)) else { return };
    let e = stats.npc_weapon_wear.entry((actor.0, item.0)).or_insert(NpcWeaponWear { first: w.before, ..NpcWeaponWear::default() });
    e.attacks += 1;
    e.points += (w.before - w.after) * forms.item_health(item) as f32;
    e.last = w.after;
}

/// Whether an NPC's own attacks wear `weapon`: only one whose record overrides Damage to
/// Weapon Mult ([`wear_actor_weapon`]).
pub fn npc_weapon_wears(forms: &CombatForms, weapon: FormId) -> bool {
    forms.weapons.get(&weapon).is_some_and(|w| w.damage_to_weapon_mult.is_some())
}

/// A hit landed on the weapon `target` holds (§17): the hit's damage comes off the
/// weapon's condition; the actor is not hurt [design: the owner's rule, 2026-10-11].
/// The weapon is the one the renderer put in the hand (`PosedParts::weapon`); when the
/// simulation's inventory, rolled on its own, lacks it, one is added first [design].
#[allow(clippy::too_many_arguments)]
fn strike_weapon(
    forms: &CombatForms,
    sim: &mut SimState,
    stats: &mut CombatStats,
    hud: Option<&mut HudMessage>,
    broken: &mut MessageWriter<WeaponBroken>,
    script: Option<&mut ScriptState>,
    parts: &ActorParts,
    target: FormId,
    points: f32,
    at: V3,
) {
    let Some(item) = parts.by_ref.get(&target.0).and_then(|p| p.weapon) else { return };
    // The player's tweak: shots do not wear the weapon in hand [design: docs/design/tweaks.md].
    if script.as_deref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.weapons_unhurt_by_hits) {
        return;
    }
    if sim.world.inventory_mut(target).is_ok_and(|i| i.count(item) == 0) {
        let _ = sim.world.add_item(target, item, 1);
    }
    stats.weapon_hits += 1;
    match wear_weapon(forms, sim, stats, hud, Some(broken), script, target, item, points, Some(at)) {
        Some(w) => info!("combat: hit {target}'s {} for {points:.1}: condition {:.1}% -> {:.1}%", item_name(sim, item), w.before * 100.0, w.after * 100.0),
        None => info!("combat: hit {target}'s {} ({item}): it does not wear", item_name(sim, item)),
    }
}

/// The magic items a hit by `attacker` with `weapon` applies (`hit_effects`): the
/// weapon's object effect; for a blow without a weapon or with the Fists, the attacker's
/// unarmed attack effect.
/// Read inside the simulation's world: an attacker a script created (PlaceAtMe) is only
/// there. `anim`: a power attack's attack group ([`ActorStrike::anim`]).
#[allow(clippy::too_many_arguments)]
fn hit_items(c: &Combat, script: Option<&mut ScriptState>, sim: &mut SimState, stats: &mut CombatStats, attacker: FormId, weapon: Option<FormId>, anim: Option<&str>) -> Vec<FormId> {
    let Some(rt) = script.and_then(|s| s.rt.as_mut()) else { return Vec::new() };
    let unarmed = weapon.is_none() || weapon == c.fists;
    with_world(rt, sim, |rt| {
        let mut items = hit_effects::hit_items(&c.forms, &rt.game, Some(attacker), weapon, unarmed, anim, &mut stats.hit_effects);
        // The poison on the player's weapon (`fnv_game::weapon_poison`).
        if let Some(p) = weapon.filter(|_| attacker == FormId::PLAYER_REF).and_then(|w| rt.game.take_poison_hit(w)) {
            info!("combat: the player's hit carries {}", rt.game.data.name(p));
            items.push(p);
        }
        items
    })
}

/// A weapon's critical effect (WEAP CRDT: the actor effect and its On Death flag), for
/// [`apply_hit`] (`fnv_game::ash_piles`, docs/spec-notes/ash-piles.md §1).
pub(crate) fn crit_effect(c: &Combat, weapon: Option<FormId>) -> Option<(FormId, bool)> {
    let w = c.forms.weapons.get(&weapon?)?;
    Some((w.crit_effect?, w.crit_on_death))
}

/// When [`remote_hits`] runs: in play, and behind this player's own conversation.
fn remote_hits_land(state: Res<State<PlayState>>) -> bool {
    matches!(state.get(), PlayState::Playing | PlayState::Dialogue)
}

/// Hits from another game ([`RemoteHit`]: a co-op guest's on this host's actors), applied
/// as this game's own are. Only co-op writes a [`RemoteHit`].
fn remote_hits(
    mut remote: MessageReader<RemoteHit>,
    mut sim: ResMut<SimState>,
    mut stats: ResMut<CombatStats>,
    mut script: Option<ResMut<ScriptState>>,
    mut feedback: ResMut<HitFeedback>,
    mut messages: MessageWriter<ActorHit>,
    (mut combat, mut phys): (Option<ResMut<Combat>>, Option<ResMut<PhysState>>),
) {
    for RemoteHit { by, hit } in remote.read().copied() {
        // A shot on a placed object (not an actor): its hit blocks run, as `by`'s, and it
        // takes the damage, as a shot of this game's own does (the projectile branch).
        if hit.location == OBJECT_LOCATION && !sim.is_actor(hit.target.0) {
            let info = HitInfo { attacker: Some(by), weapon: hit.weapon, location: -1, ..HitInfo::default() };
            let r = hit.target;
            let ran = script.as_deref_mut().and_then(|s| s.rt.as_mut()).is_some_and(|rt| with_world(rt, &mut sim, |rt| rt.game.hit_object(r, info)));
            if let Some(c) = combat.as_deref_mut() {
                damage_destructible(c, &mut sim, script.as_deref_mut(), r, hit.dealt, None);
            }
            // Knocked here too: the other players' copies of a loose object follow this
            // world's (a guest's physics does not write back what the host moves), so
            // without it a bottle the guest shot stayed put on every screen
            // [observed: buckallobill, in game, NVForever co-op].
            let pushed = match (phys.as_deref_mut(), combat.as_deref()) {
                (Some(p), Some(c)) => remote_push(p, c, &sim, by, r, hit.weapon),
                _ => false,
            };
            stats.object_scripts += ran as u32;
            stats.objects_pushed += pushed as u32;
            info!("combat: {by}'s shot (another game's) hits {}{}{}", r, if ran { ", its hit blocks ran" } else { "" }, if pushed { ", knocked away" } else { "" });
            continue;
        }
        let info = HitInfo { attacker: Some(by), weapon: hit.weapon, location: hit.location, critical: hit.critical, limb: hit.limb, ..HitInfo::default() };
        let mut dealt = hit.dealt;
        // A hit on this player in the host's world, dealt against an actor that wears
        // nothing there: this player's armour takes its part here, as an explosion's
        // damage meets it (`fly`).
        if let (FormId::PLAYER_REF, Some(c)) = (hit.target, combat.as_deref()) {
            let (dt, dr) = target_armor(c, script.as_deref_mut(), &mut sim, FormId::PLAYER_REF);
            dealt = (dealt - dt).max(dealt * c.tuning.dt_floor) * (1.0 - dr.clamp(0.0, c.rules.max_armor_rating) / 100.0);
        }
        apply_hit(script.as_deref_mut(), &mut sim, hit.target, dealt, info, &mut stats, &mut feedback, None, &mut messages, &[], false, None);
    }
}

/// The height above a shooter's feet a remote shot is taken to leave from, for its push
/// ([`remote_push`]): about a standing actor's eyes [design].
const REMOTE_SHOT_EYE: f32 = 110.0;

/// A remote shot's push on a loose object (another game's hit, [`remote_hits`]): the
/// weapon's projectile's impact force, from the shooter's eyes towards the object's
/// centre, as this game's own shot on it pushes. The hit's own point and direction are
/// not in the message, so they are taken from where the shooter stands here [design].
/// Returns whether it moved.
fn remote_push(phys: &mut PhysState, c: &Combat, sim: &SimState, by: FormId, r: FormId, weapon: Option<FormId>) -> bool {
    let force = weapon.and_then(|w| c.forms.chain(w, None)).and_then(|ch| ch.projectile).and_then(|p| c.forms.projectiles.get(&p)).map_or(0.0, |p| p.impact_force);
    if force <= 0.0 {
        return false;
    }
    let Some(at) = phys.world.ref_center(r.0) else { return false };
    let Some(feet) = phys.world.actor_volume(by.0).map(|v| v.feet).or_else(|| sim.world.peek(by).map(|s| s.pos)) else { return false };
    let dir = vec::normalize(vec::sub(at, [feet[0], feet[1], feet[2] + REMOTE_SHOT_EYE]));
    let impulse = vec::scale(dir, force * OBJECT_PUSH * phys.world.params.units_per_metre);
    phys.world.push_ref(r.0, at, impulse, OBJECT_PUSH_MAX_SPEED).is_some()
}

/// Where a shot or a blow struck `target`, for the scripts (JohnnyGuitar's
/// Get3DDistanceFromHitToNiNode, `fnv_game::nvse_jg_nif`).
fn note_hit_point(script: Option<&mut ScriptState>, target: FormId, at: [f32; 3]) {
    if let Some(rt) = script.and_then(|s| s.rt.as_mut()) {
        rt.game.note_hit_point(target, at);
    }
}

/// Apply `dealt` health damage through the script runtime; report and remember it. A
/// critical hit then gives the weapon's critical effect (`crit`: the actor effect and
/// its On Death flag; `fnv_game::Game::apply_crit_effect`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_hit(
    script: Option<&mut ScriptState>,
    sim: &mut SimState,
    target: FormId,
    dealt: f32,
    info: HitInfo,
    stats: &mut CombatStats,
    feedback: &mut HitFeedback,
    hud: Option<&mut HudMessage>,
    messages: &mut MessageWriter<ActorHit>,
    effects: &[FormId],
    guard_broken: bool,
    crit: Option<(FormId, bool)>,
) -> Option<HitResult> {
    let (r, hit) = hit_actor(script, sim, target, dealt, info, stats, feedback, hud, effects, guard_broken, crit, &[])?;
    if let Some(h) = hit {
        messages.write(h);
    }
    Some(r)
}

/// [`apply_hit`] with an explosion's limb damage (`blast`, [`blast_limb_hits`]; empty for
/// any other hit), which goes on after the health damage and counts as the hit's own: a
/// limb it cripples disarms, shows on the HUD and makes the hit a crippling one for the
/// stagger rule (crippled-limbs.md §5). The [`ActorHit`] to send is returned, for the
/// caller to send once it has decided a knockdown (none for the player, whose
/// [`PlayerHit`] is in `feedback`).
#[allow(clippy::too_many_arguments)]
fn hit_actor(
    script: Option<&mut ScriptState>,
    sim: &mut SimState,
    target: FormId,
    dealt: f32,
    info: HitInfo,
    stats: &mut CombatStats,
    feedback: &mut HitFeedback,
    mut hud: Option<&mut HudMessage>,
    effects: &[FormId],
    guard_broken: bool,
    crit: Option<(FormId, bool)>,
    blast: &[LimbHit],
) -> Option<(HitResult, Option<ActorHit>)> {
    let mine = info.attacker == Some(FormId::PLAYER_REF);
    // A co-op guest's ([`CombatStats::relay`]): the player's hits on others go to the
    // host, whose world takes them; nothing is applied here.
    if target != FormId::PLAYER_REF {
        if let Some(out) = stats.relay.as_mut() {
            if mine {
                out.push(RelayedHit { target, dealt, weapon: info.weapon, location: info.location, critical: info.critical, limb: info.limb });
            }
            return None;
        }
    }
    let rt = script.and_then(|s| s.rt.as_mut())?;
    if target == FormId::PLAYER_REF {
        // While the classic VATS style plays its queued attacks the player takes a share
        // of a hit, `fVATSPlayerDamageMult` (`crate::vats_mode`, vats-classic.md §7).
        let dealt = match rt.game.vats.player_damage_mult {
            Some(m) => {
                let less = fnv_combat::vats::playback_damage(dealt, &fnv_combat::vats::ClassicTuning { player_damage_mult: m, ..Default::default() });
                info!("combat: vats playback: the player takes {less:.1} of {dealt:.1} (fVATSPlayerDamageMult {m:.2})");
                less
            }
            None => dealt,
        };
        return hit_player(rt, sim, dealt, info, stats, hud, effects, feedback, blast).map(|r| (r, None));
    }
    // Another co-op player's: their game takes it, with their own armour (`remote_hits`).
    if rt.game.is_coop_actor(target) {
        stats.coop_hits.push((info.attacker, RelayedHit { target, dealt, weapon: info.weapon, location: info.location, critical: info.critical, limb: info.limb }));
        return Some((HitResult { dealt: dealt as f64, immune: true, ..HitResult::default() }, None));
    }
    // The hit's effects go on before its damage (`hit_effects`).
    let span = info_span!("combat::hit_effects").entered();
    with_world(rt, sim, |rt| hit_effects::apply(&mut rt.game, &mut stats.hit_effects, target, effects, info.attacker));
    drop(span);
    let span = info_span!("combat::game_hit").entered();
    let mut r = with_world(rt, sim, |rt| rt.game.hit(target, dealt as f64, info));
    drop(span);
    let blasted = blast_limbs(rt, sim, target, info, blast, &r);
    r.crippled |= !blasted.is_empty();
    let name = sim.ref_name(Some(&rt.game), target.0).map_or_else(|| target.to_string(), str::to_string);
    if let Some((spell, on_death)) = crit.filter(|_| info.critical && !r.immune) {
        let _span = info_span!("combat::crit_effect").entered();
        let done = with_world(rt, sim, |rt| rt.game.apply_crit_effect(target, spell, on_death, r.died, info.attacker));
        if let fnv_game::ash_piles::CritEffect::Applied(ids) = done {
            let spell_name = rt.game.data.name(spell);
            info!("combat: critical effect {spell_name} ({spell}) on {name} {target}{}: {} effect(s)", if on_death { " (on death)" } else { "" }, ids.len());
        }
    }
    // A critical by the player says so in the corner with the default Vault Boy corner
    // icon (`corner_messages::DEFAULT_ICON`, `glow_message_vaultboy_neutral.dds`).
    // `FalloutNV.esm`'s `sCriticalStrikeIcon` (`glow_Critical_Chance.dds`, a 256x256
    // Pip-Boy stat icon not in `InterfaceShared.tai`) is documented unused [documented:
    // GECK wiki Unused Game Settings]; whether the original shows the neutral face or
    // another `glow_message_vaultboy_*` face on criticals / sneak criticals is not yet
    // observed [hypothesis]. The words are ours (the original's are not in the master
    // data [design]); the player's Critical Hit Messages Name the Limb tweak adds the
    // part struck [design: docs/design/tweaks.md].
    if mine && info.critical && !r.immune {
        let limb = r.limb.map(|(code, _)| limb_name(code)).filter(|_| rt.game.tweaks.crit_message_limb);
        rt.game.emit(critical_hit_note(&name, limb));
    }
    if mine {
        stats.damage += r.dealt as f32;
    } else if info.attacker.is_some() && !r.immune {
        let e = stats.npc_hits.entry(target.0).or_insert_with(|| (name.clone(), Vec::new(), r.max_health as f32));
        e.1.push(r.dealt as f32);
    }
    if r.died {
        if mine {
            // For a kill camera outside VATS mode (`crate::kill_cam`, the player's tweak).
            if let Ok(mut k) = PLAYER_KILLS.lock() {
                k.push((target, info.weapon.unwrap_or(FormId(0)), r.limb.map(|l| l.0)));
            }
            stats.kills += 1;
            stats.xp += r.xp;
            if let Some(h) = hud.as_deref_mut() {
                h.show(if r.xp > 0.0 { format!("{name} killed (+{} XP)", r.xp) } else { format!("{name} killed") });
            }
        }
        let by = info.attacker.filter(|a| *a != FormId::PLAYER_REF).map_or(String::new(), |a| format!(" by {a}"));
        info!("combat: {name} {target} killed{by}{}", if r.xp > 0.0 { format!(", +{} XP", r.xp) } else { String::new() });
    } else if r.knocked_out {
        info!("combat: {name} {target} knocked out (essential)");
    }
    // The limbs this hit crippled: the one it struck, or those a blast did.
    let crippled: Vec<u16> = match r.limb {
        Some((code, _)) if r.crippled => vec![code],
        Some((code, condition)) => {
            debug!("combat: {name} {target}: {} condition {condition:.0}", limb_name(code));
            Vec::new()
        }
        None => blasted,
    };
    // The player's tweak keeps teammates armed [design: docs/design/tweaks.md].
    let spared = (rt.game.tweaks.no_disarm_companions && rt.game.is_teammate(target)) || rt.game.tweaks.crippling_never_disarms;
    // The player's own blast does not disarm the player (the NPCs Can Disarm the Player
    // tweak, `disarm`) [design].
    let own = target == FormId::PLAYER_REF && mine;
    if !spared && !own && !r.died && !r.knocked_out && crippled.iter().any(|c| matches!(*c, fnv_game::combat::LEFT_ARM | fnv_game::combat::RIGHT_ARM)) {
        feedback.disarms.push(target);
    }
    for &code in &crippled {
        let limb = limb_name(code);
        info!("combat: {name} {target}: {limb} crippled");
        if let Some(h) = hud.as_deref_mut().filter(|_| !r.died && mine) {
            h.show(format!("{name}: {limb} crippled"));
        }
    }
    let mut hit = None;
    if !r.immune {
        let torso_crippled = with_world(rt, sim, |rt| rt.game.is_crippled(target, fnv_game::combat::TORSO));
        if mine {
            feedback.target = Some((name, r.health as f32, r.max_health as f32, 0.0));
        }
        hit = Some(ActorHit { form_id: target.0, died: r.died, knocked_out: r.knocked_out, location: info.location, crippled: r.crippled, part_crippled: r.limb.is_some_and(|(_, c)| c <= 0.0), torso_crippled, by: info.attacker, share: if r.max_health > 0.0 { (r.dealt / r.max_health) as f32 } else { 0.0 }, guard_broken, weapon: info.weapon });
    }
    Some((r, hit))
}

/// A limb's name by its condition code (25–31).
/// The player's kills this frame: the actor, the weapon and the condition of the part
/// struck (`crate::kill_cam` takes them for its camera outside VATS mode).
pub(crate) static PLAYER_KILLS: std::sync::Mutex<Vec<(FormId, FormId, Option<u16>)>> = std::sync::Mutex::new(Vec::new());

/// How long the critical-hit corner message shows, seconds [design].
const CRIT_NOTE_SECONDS: f32 = 2.0;

/// Build the corner notification event for a player critical hit: uses the default
/// corner-message icon (`icon: None` -> `corner_messages::DEFAULT_ICON`) rather than the
/// documented-unused `sCriticalStrikeIcon` GMST [documented: GECK wiki Unused Game
/// Settings; hypothesis: neutral face pending an unmodded original-game capture].
fn critical_hit_note(name: &str, limb: Option<&str>) -> fnv_game::trace::EventKind {
    let text = match limb {
        Some(l) => format!("Critical hit on {name}'s {l}"),
        None => format!("Critical hit on {name}"),
    };
    fnv_game::trace::EventKind::NoteShown { text, icon: None, seconds: CRIT_NOTE_SECONDS }
}

fn limb_name(code: u16) -> &'static str {
    code.checked_sub(25).and_then(|i| LIMB_NAMES.get(i as usize)).copied().unwrap_or("limb")
}

/// An explosion's limb damage after its health damage `r` ([`hit_actor`]): none when the
/// hit killed, knocked out or did not touch the actor. Logs the damage; returns the limbs
/// it crippled.
fn blast_limbs(rt: &mut crate::script::Runtime, sim: &mut SimState, target: FormId, info: HitInfo, blast: &[LimbHit], r: &HitResult) -> Vec<u16> {
    if blast.is_empty() || r.immune || r.died || r.knocked_out || r.dealt <= 0.0 {
        return Vec::new();
    }
    let hurt = with_world(rt, sim, |rt| rt.game.blast_limbs(target, info.attacker, blast));
    let crippled: Vec<u16> = hurt.iter().filter(|h| h.2).map(|h| h.0).collect();
    if !hurt.is_empty() {
        let names: Vec<&str> = crippled.iter().map(|&c| limb_name(c)).collect();
        let each: Vec<String> = blast.iter().map(|l| format!("{} {:.2}", limb_name(l.condition), l.damage)).collect();
        info!("combat: blast on {target}: limb damage {}{}", each.join(", "), if names.is_empty() { String::new() } else { format!(", crippled {}", names.join(", ")) });
    }
    crippled
}

/// God mode: the player takes no damage from hits, so cannot be killed by them
/// (settings page `god-mode`, `--god`, test aid `NVF_PLAY_TEST_GOD=1`) [design]. One
/// switch for the process: hits reach the player from several systems.
static GOD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether god mode is on.
pub fn god_mode() -> bool {
    GOD.load(std::sync::atomic::Ordering::Relaxed)
}

/// Switch god mode on or off.
pub fn set_god_mode(on: bool) {
    GOD.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Another actor hits the player (module notes): the player's health takes the damage
/// through `Game::hit`, and at 0 the player dies.
#[allow(clippy::too_many_arguments)]
fn hit_player(rt: &mut crate::script::Runtime, sim: &mut SimState, dealt: f32, info: HitInfo, stats: &mut CombatStats, hud: Option<&mut HudMessage>, effects: &[FormId], feedback: &mut HitFeedback, blast: &[LimbHit]) -> Option<HitResult> {
    let by = info.attacker.map_or("?".to_string(), |a| sim.ref_name(Some(&rt.game), a.0).map_or_else(|| a.to_string(), str::to_string));
    stats.player_hits += 1;
    if god_mode() {
        info!("combat: {by} hits the player for {dealt:.1} (god mode: no damage taken)");
        // Still a hit: a crippled torso flinches (`player_flinch`).
        let torso_crippled = with_world(rt, sim, |rt| rt.game.is_crippled(FormId::PLAYER_REF, fnv_game::combat::TORSO));
        feedback.player_hits.push(PlayerHit { crippled: false, part_crippled: false, torso_crippled });
        let health = with_world(rt, sim, |rt| fnv_game::functions::actor_value(&mut rt.game, FormId::PLAYER_REF, fnv_game::actor_values::HEALTH).unwrap_or(0.0));
        return Some(HitResult { health, immune: true, ..HitResult::default() });
    }
    with_world(rt, sim, |rt| hit_effects::apply(&mut rt.game, &mut stats.hit_effects, FormId::PLAYER_REF, effects, info.attacker));
    let limb_before = info.limb.and_then(|l| with_world(rt, sim, |rt| fnv_game::functions::actor_value(&mut rt.game, FormId::PLAYER_REF, l.condition).ok()));
    let mut r = with_world(rt, sim, |rt| rt.game.hit(FormId::PLAYER_REF, dealt as f64, info));
    if let Some((before, (code, condition))) = limb_before.zip(r.limb).filter(|(before, (_, after))| before > after) {
        info!("combat: the player's {} takes {:.1}, condition {condition:.1}", limb_name(code), before - condition);
    }
    r.crippled |= !blast_limbs(rt, sim, FormId::PLAYER_REF, info, blast, &r).is_empty();
    stats.player_damage += r.dealt as f32;
    info!("combat: {by} hits the player for {:.1}, health {:.0}/{:.0}", r.dealt, r.health, r.max_health);
    if !r.immune && r.dealt > 0.0 {
        feedback.player_hurt.push((r.dealt as f32, r.max_health as f32));
    }
    if !r.died {
        let torso_crippled = with_world(rt, sim, |rt| rt.game.is_crippled(FormId::PLAYER_REF, fnv_game::combat::TORSO));
        feedback.player_hits.push(PlayerHit { crippled: r.crippled, part_crippled: r.limb.is_some_and(|(_, c)| c <= 0.0), torso_crippled });
    }
    if r.died {
        stats.player_died = true;
        info!("combat: the player died, killed by {by}");
        if let Some(h) = hud {
            h.show(tr("You are dead.").to_string());
        }
    }
    Some(r)
}

/// Melee and unarmed attacks (see the module notes).
#[allow(clippy::too_many_arguments)]
fn melee(
    time: Res<Time>,
    mut c: ResMut<Combat>,
    mut sim: ResMut<SimState>,
    mut phys: ResMut<PhysState>,
    (player, intent): (Res<Player>, Res<MoveIntent>),
    control: Res<PlayerControl>,
    mut anim: ResMut<WeaponAnim>,
    mut stats: ResMut<CombatStats>,
    mut script: Option<ResMut<ScriptState>>,
    mut feedback: ResMut<HitFeedback>,
    mut hud: Option<ResMut<HudMessage>>,
    (mut messages, mut pushes, mut broken, mut forced): (MessageWriter<ActorHit>, MessageWriter<ActorPush>, MessageWriter<WeaponBroken>, MessageWriter<ForcedHitReaction>),
    (parts, limbs): (Res<ActorParts>, Res<PlayerLimbs>),
    (sfx, mut sounds, mut impacts): (Option<Res<Sfx>>, MessageWriter<PlaySound>, MessageWriter<ImpactFx>),
    (mut vats, mut dm, mut knocks): (Option<ResMut<crate::vats_mode::VatsMode>>, ResMut<crate::dismember::Dismemberment>, MessageWriter<Knockback>),
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
) {
    sync_condition_penalty(&mut c, script.as_deref());
    let dt = time.delta_secs();
    let c = &mut *c;
    // VATS mode's swing due now: one tap of Use (vats-mode.md §16.2).
    let vats_tap = vats.as_deref_mut().is_some_and(|v| v.take_swing());
    let Some(id) = c.weapon.filter(|_| c.melee) else { return };
    if !anim.ready {
        c.swings.reset();
        anim.swing = None;
        return;
    }
    let mut w = c.player_stats(id).expect("weapon present");
    script_weapon_critical_damage(script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game), id, &mut w);
    // The clip the body timed for the swing playing; a new swing runs on the weapon's
    // own timing until the body has read its clip (the next frame).
    // The Modify Attack Speed perks (Slayer, Melee Hacker …) speed the swing as the
    // weapon's own multipliers do; the body plays the swing clip at the same rate.
    let (atk, by) = player_weapon_perk(script.as_deref_mut(), &mut sim, fnv_game::perks::MODIFY_ATTACK_SPEED, id, 1.0);
    let atk = if atk.is_finite() && atk > 0.0 { atk } else { 1.0 };
    let speed = w.attack_mult.max(0.01) * w.anim_mult.max(0.01) * atk;
    if (speed - anim.attack_rate).abs() > 1e-4 {
        if by.is_empty() {
            info!("combat: swings at x{speed:.2} ({})", w.edid.as_deref().unwrap_or("?"));
        } else {
            info!("combat: swings at x{speed:.2} ({}; perks x{atk:.2}: {})", w.edid.as_deref().unwrap_or("?"), by.join(", "));
        }
    }
    anim.attack_rate = speed;
    if let Some(stem) = anim.timed.as_deref() {
        let t = &anim.timing;
        c.swings.set_timing(stem, SwingTiming::of(t.attack_len, t.hit_time, t.next_attack, speed, 1.0));
    }
    let fallback = Timing::of(&w, &ClipTiming::default(), &c.tuning).interval;
    let first = fnv_actor::player_clips::attack_stem(anim.attack.as_deref());
    // Both legs crippled: no directional power attacks outside VATS mode (crippled-limbs.md
    // §4): the swing takes the standing one.
    let (forward, right) = if limbs.both_legs() { (0.0, 0.0) } else { (intent.forward, intent.right) };
    // The weapon's power attack override plays for the standing power attack.
    // Holding Block suppresses starting a melee/unarmed swing unless Power Attack From a
    // Block is enabled (`instant_power`, docs/design/tweaks.md; #3753).
    let blocking = control.aiming && !vats_tap;
    let instant_power = blocking && tweaks.as_ref().is_some_and(|t| t.power_attack_if_blocking);
    let input = MeleeInput { trigger: (control.trigger || vats_tap) && !broken_in_hand(&sim, id), dt, forward, right, first: &first, rolls: [c.swing_rng.next_f32(), c.swing_rng.next_f32()], power_override: w.power_attack_override.as_deref(), blocking, instant_power };
    let mut strike = None;
    for e in c.swings.tick(input, &c.tuning, SwingTiming::of(None, None, None, speed, fallback)) {
        match e {
            MeleeEvent::Started { stem, power, chained } => {
                // The unarmed special moves (perk-entry-points.md §11) are chosen as the
                // swing starts, so it plays the move's own clip: a power attack in a
                // direction whose move the player has; Scribe Counter, a plain blow begun
                // within COUNTER_WINDOW of the player's block, a standing power attack.
                let (mut stem, mut power) = (stem, power);
                let unarmed = Some(id) == c.fists || script.as_deref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.data.weapon_skills.get(&id).copied() == Some(fnv_game::actor_values::UNARMED as i64));
                c.special_swing = None;
                if unarmed {
                    let countering = power.is_none() && c.player_blocked_at.is_some_and(|t| c.time - t <= COUNTER_WINDOW);
                    let special = match power {
                        Some(d) => special_move(script.as_deref_mut(), &mut sim, d),
                        None if countering => scribe_counter(script.as_deref_mut(), &mut sim),
                        None => None,
                    };
                    if let Some(m) = special {
                        if power.is_none() {
                            power = Some(fnv_combat::melee::PowerDir::Stand);
                            c.player_blocked_at = None;
                        }
                        info!("combat: {} ({:?} power attack, unarmed): clip {}", m.name, power.unwrap_or(fnv_combat::melee::PowerDir::Stand), m.stem);
                        if m.lunges {
                            // Its clip carries the player (`h2hattackcustom1power.kf`
                            // moves `Bip01` 332 units forward over its 1.6 s,
                            // perk-entry-points.md §11.4): the body's attack clips move the
                            // player by their root's travel, at the swing's speed
                            // (`crate::player::Lunge`, actors.md §8.13).
                            info!("combat: {}: the player lunges with its clip", m.name);
                        }
                        stem = m.stem.to_string();
                        if let Some(s) = c.swings.swing.as_mut() {
                            s.stem = stem.clone();
                            s.power = power;
                        }
                        c.special_swing = Some(m);
                    }
                }
                if power.is_some() {
                    stats.power_attacks += 1;
                    // The player's tweak: a power attack costs Action Points [design: docs/design/tweaks.md].
                    let cost = tweaks.as_ref().map_or(0.0, |t| t.power_attack_ap);
                    if cost > 0.0 && !god_mode() {
                        if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
                            crate::script::with_world(rt, &mut sim, |rt| fnv_game::functions::add_actor_value_damage(&mut rt.game, FormId::PLAYER_REF, fnv_game::actor_values::ACTION_POINTS, f64::from(cost)));
                        }
                    }
                    // Without a clip to time it, a power attack takes longer [design].
                    if let Some(s) = c.swings.swing.as_mut() {
                        s.timing = SwingTiming::of(None, None, None, speed, fallback * c.tuning.power_time);
                    }
                }
                stats.swings += 1;
                stats.chained_swings += u32::from(chained);
                // The player's PowerAttack or Attack line (docs/spec-notes/combat-dialogue.md).
                if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
                    with_world(rt, &mut sim, |rt| rt.game.attack_line(FormId::PLAYER_REF, None, power.is_some(), false));
                }
                info!("combat: {} swing {stem}{}", w.edid.as_deref().unwrap_or("?"), if chained { " (chained)" } else { "" });
                anim.swing = Some(stem);
                anim.attacks += 1;
            }
            MeleeEvent::Strike { power } => strike = Some(power),
            MeleeEvent::Ended => {}
        }
    }
    if let Some(power) = strike {
        // The special move chosen as the swing started (its clip is playing).
        let special = c.special_swing.take();
        let power_mult = power.map_or(1.0, |d| d.damage(&c.rules));
        // The reach is from the body; the rays start at the eye, the capsule's middle.
        // The player's tweak scales the reach [design: docs/design/tweaks.md].
        let reach_mult = script.as_ref().and_then(|s| s.rt.as_ref()).map_or(1.0, |rt| rt.game.tweaks.melee_reach_mult);
        let reach = damage::melee_reach(&c.rules, &w, 1.0) * reach_mult + phys.player.as_ref().map_or(0.0, |p| p.params.radius);
        let eye = player.eye();
        // A blow that meets no one but strikes the world within reach (a wall, the
        // ground ahead) shows its impact there.
        let mut struck_world: Option<WorldHit> = None;
        // VATS mode's swing lands on its mark when the mark is still close and the first
        // thing on the line from the eye to it (100 % accuracy, vats-mode.md §16.2).
        let vats_mark = vats.as_deref().and_then(|v| v.melee_mark());
        // The blow is a queued special attack of the classic style: its damage multiplier
        // and the effect it puts on the target (docs/design/vats-classic.md §9).
        let vats_special = vats.as_deref().and_then(|v| v.swing_special());
        if let Some(s) = vats_special {
            info!("combat: vats special attack: the blow's damage times {:.2}{}", s.dam_mult, s.effect.map_or_else(String::new, |e| format!(", effect {e}")));
        }
        let power_mult = power_mult * vats_special.map_or(1.0, |s| s.dam_mult);
        let hits = {
            // A blow's rays (VATS mode's straight one, the fan, the reach across the ground
            // and the look ahead for an impact) stop on the Transparent layer (#2340).
            let world = view(&mut phys, script.as_deref(), Some(&sim)).for_melee();
            let straight = vats_mark.and_then(|(target, point, close, lands)| {
                // The hit roll missed (vats-mode.md §17.4): the blow lands on no one.
                if !lands {
                    info!("combat: vats mode blow on {target:08X}: missed (the hit roll)");
                    return Some(Vec::new());
                }
                let flat = ((point[0] - eye[0]).powi(2) + (point[1] - eye[1]).powi(2)).sqrt();
                let d = vec::sub(point, eye);
                let end = vec::add(eye, vec::scale(vec::normalize(d), vec::len(d) + VATS_BLOW_PAST));
                let hit = (flat <= close).then(|| world.ray(eye, end, &[PLAYER])).flatten().filter(|h| h.actor == Some(target as ActorId));
                info!(
                    "combat: vats mode blow on {target:08X} ({}): {:.0} units across, {}",
                    hit.as_ref().and_then(|h| h.part).map_or("whole".to_string(), |p| format!("part {p}")),
                    flat,
                    if hit.is_some() { "struck" } else { "not in reach or not first on the line: an ordinary swing" }
                );
                hit.map(|h| vec![(target as ActorId, h)])
            });
            if let Some(hits) = straight {
                hits
            } else {
                // Low and high: a short creature at the player's feet is struck within the
                // reach across the ground (`swing_reaching`).
                let hits = fnv_combat::projectile::swing_reaching(&world, &c.tuning, eye, player.feet[2], player.look_dir(), reach, PLAYER);
                let fan = fnv_combat::projectile::swing(&world, &c.tuning, eye, player.look_dir(), [0.0, 0.0, 1.0], reach, PLAYER).len();
                if hits.len() > fan {
                    info!("combat: swing reaches {} actors (reach {reach:.0}), {} of them out of the fan of rays at eye height", hits.len(), hits.len() - fan);
                }
                if hits.is_empty() {
                    let ahead = world.ray(eye, vec::add(eye, vec::scale(player.look_dir(), 400.0)), &[]);
                    struck_world = ahead.clone().filter(|h| h.actor.is_none() && h.distance <= reach);
                    info!(
                        "combat: swing misses (reach {reach:.0}); straight ahead: {}",
                        ahead.map_or("nothing within 400".to_string(), |h| format!("{} at {:.0}", h.actor.map_or("the world".to_string(), |a| format!("actor {a:08X}")), h.distance))
                    );
                }
                hits
            }
        };
        // A blow that lands wears the weapon, once however many it strikes (§17); the
        // blow itself is dealt at the condition the weapon had.
        let weapon_condition = item_condition(&sim, FormId::PLAYER_REF, id);
        // The blow's impacts, as a shot's (the owner's playtest list 5 item 24): blood on
        // the actors struck, dust or sparks on the world, with their sounds.
        if let Some(s) = sfx.as_deref() {
            let dir = player.look_dir();
            let landed = hits.iter().map(|(a, h)| (Some(*a), h)).chain(struck_world.iter().map(|h| (None, h)));
            for (actor, h) in landed {
                if h.part == Some(WEAPON_PART) {
                    continue;
                }
                let (impact, material) = impact_for(s, script.as_deref(), &sim, id, actor, h.material);
                match impact {
                    Some(impact) => {
                        stats.melee_impacts += 1;
                        debug!("combat: melee impact {impact} ({material}) at ({:.0}, {:.0}, {:.0})", h.point[0], h.point[1], h.point[2]);
                        show_impact(s, script.as_deref(), &sim, ImpactFx { impact, shooter: FormId::PLAYER_REF, point: h.point, normal: h.normal, dir, actor: actor.map(|a| a as u32), no_spray: false, no_decal: false, splatter: false }, &mut sounds, &mut impacts);
                    }
                    None => debug!("combat: {} has no impact for {material}", w.edid.as_deref().unwrap_or("?")),
                }
            }
        }
        // The player's tweak: blows that land only on the dead do not wear the weapon [design: docs/design/tweaks.md].
        let only_dead = script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.no_wear_on_dead && hits.iter().all(|h| rt.game.is_dead(FormId(h.0 as u32))));
        if !hits.is_empty() && !only_dead {
            wear_weapon(&c.forms, &mut sim, &mut stats, hud.as_deref_mut(), None, script.as_deref_mut(), FormId::PLAYER_REF, id, condition::weapon_wear(&c.rules, &w, &[]), None);
        }
        for (actor, hit) in hits {
            let target = FormId(actor as u32);
            stats.melee_hits += 1;
            let mut a = attacker(script.as_deref_mut(), &mut sim, &c.rules, FormId::PLAYER_REF, &w, vats_mark.is_some());
            a.sneaking = sneak_attack(script.as_deref(), FormId::PLAYER_REF, control.sneak, target);
            let bp = body_part(c, &parts, actor, hit.part);
            let (dt_v, dr) = target_armor_for_zone(c, script.as_deref_mut(), &mut sim, target, Some(Zone::of(bp.as_ref())));
            let part = bp.as_ref().map_or_else(HitPart::default, BodyPart::hit_part);
            // The player's tweak: sneak criticals on the head only [design: docs/design/tweaks.md].
            if !part.head && script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.sneak_criticals_head_only) {
                a.sneaking = false;
            }
            let h = HitInput { weapon: &w, projectiles: 1, condition: weapon_condition, ammo_effects: &[], part, power: power_mult };
            let roll = c.rng.next_f32();
            let (ap, tp) = hit_perks(script.as_deref_mut(), &mut sim, FormId::PLAYER_REF, id, Some(target));
            if hit.part == Some(WEAPON_PART) {
                let d = damage::hit(&c.rules, &c.tuning, &h, &a, &Target::default(), &ap, &NoPerks, roll);
                strike_weapon(&c.forms, &mut sim, &mut stats, hud.as_deref_mut(), &mut broken, script.as_deref_mut(), &parts, target, d.raw, hit.point);
                continue;
            }
            let resist = target_resist(script.as_deref_mut(), &mut sim, target, &w);
            let nerve = companion_nerve(script.as_deref_mut(), &mut sim, target);
            let mut d = damage::hit(&c.rules, &c.tuning, &h, &a, &Target { dt: dt_v, dr, resist, nerve, ..Target::default() }, &ap, &tp, roll);
            log_hit_perks(FormId::PLAYER_REF, Some(target), &ap, &tp, &d);
            // A person blocking it from the front stops part of it (combat-ai.md §14.3); a
            // power attack breaks through and staggers it (combat-rules.md §12).
            let mut guard_broken = false;
            let mut blocked = false;
            if c.guarding.get(&target).is_some_and(|f| damage::from_front(*f, player.look_dir())) {
                let before = d.dealt;
                (d.dealt, guard_broken) = damage::against_block(&c.tuning, before, power.is_some());
                if guard_broken {
                    stats.blocks_broken += 1;
                    info!("combat: the player's power attack breaks {target}'s block ({before:.1}, all dealt)");
                } else {
                    blocked = true;
                    stats.blows_blocked += 1;
                    info!("combat: {target} blocks the player's blow: {:.1} of {before:.1} stopped", before - d.dealt);
                    c.blocked_blows.push(BlockedBlow { attacker: FormId::PLAYER_REF, blocker: target, damage: before, dealt: d.dealt });
                    if let Some(b) = block_sound(c, &sim, sfx.as_deref(), target) {
                        sounds.write(b);
                    }
                }
            }
            let agility = actor_av(script.as_deref_mut(), &mut sim, target, fnv_game::actor_values::AGILITY).unwrap_or(5.0);
            let push = damage::knockback_force(&c.rules, d.dealt, agility);
            let location = bp.as_ref().map_or(0, BodyPart::location);
            let limb = limb_hit(bp.as_ref(), &d, &w);
            let info = HitInfo {
                attacker: Some(FormId::PLAYER_REF),
                weapon: Some(id),
                location,
                critical: d.critical,
                blocking: blocked,
                sneak_attack: a.sneaking,
                base_weapon_damage: f64::from(d.base + d.crit_bonus),
                hit_pos: Some(hit.point),
                // [design] Player melee hits record the player's unit look direction for
                // `GetHit3DData 1` (unprobed for melee swings in the original; jip-hit-info.md §6).
                hit_dir: fnv_game::combat::unit_hit_dir(player.look_dir()),
                limb,
                ..HitInfo::default()
            };
            pushes.write(ActorPush { form_id: target.0, point: hit.point, dv: vec::scale(player.look_dir(), MELEE_PUSH * if power.is_some() { POWER_PUSH } else { 1.0 }) });
            let mut items = hit_items(c, script.as_deref_mut(), &mut sim, &mut stats, FormId::PLAYER_REF, Some(id), None);
            // A special move's effect goes on with the blow (Ranger Takedown's knockdown
            // spell, Khan Trick's head crippling: the DOBJ's).
            if let Some(spell) = special.and_then(|m| m.spell) {
                items.push(spell);
            }
            items.extend(vats_special.and_then(|s| s.effect));
            note_hit_point(script.as_deref_mut(), target, hit.point);
            note_armour(script.as_deref_mut(), target, &d);
            let r = apply_hit(script.as_deref_mut(), &mut sim, target, d.dealt, info, &mut stats, &mut feedback, hud.as_deref_mut(), &mut messages, &items, guard_broken, crit_effect(c, Some(id)));
            armour_lines(script.as_deref_mut(), Some(FormId::PLAYER_REF), target, &d, c.tuning.dt_floor);
            spread_hit_areas(script.as_deref_mut(), &mut sim, &mut stats, target, &items, info.attacker);
            if r.as_ref().is_some_and(|r| !r.died && !r.immune && !r.knocked_out) {
                if let Some(m) = special.filter(|m| m.forces_reaction) {
                    info!("combat: {}: {target} is forced into its hit reaction (location {location})", m.name);
                    forced.write(ForcedHitReaction { form_id: target.0, location, by: m.name });
                }
            }
            // A living actor struck is pushed back by the knockback (module notes).
            if r.as_ref().is_some_and(|r| !r.died && !r.immune && !r.knocked_out) {
                if let Some(k) = Knockback::of(target.0, player.look_dir(), push, c.rules.knockback_time) {
                    knocks.write(k.by(FormId::PLAYER_REF, c.next_blow(), power.is_some()));
                }
            }
            if r.as_ref().is_some_and(|r| !r.died && !r.immune) {
                // Legion Assault knocks back (no DOBJ spell); the Knockdown Chance perks
                // (Super Slam, Elijah's Last Words) may knock the target down.
                let away = player.look_dir();
                if special.is_some_and(|m| m.knock_back) {
                    knock_down(script.as_deref_mut(), FormId::PLAYER_REF, target, away, "Legion Assault");
                } else {
                    perk_knockdown(c, script.as_deref_mut(), &mut sim, FormId::PLAYER_REF, id, target, away);
                }
            }
            if let Some(mut check) = r.as_ref().and_then(|r| dismember_check(target, hit.part, &w, r, Some(FormId::PLAYER_REF), hit.point, player.look_dir())) {
                crit_gib(c, &parts, script.as_deref(), d.critical && !a.sneaking, &mut check);
                dm.checks.push(check);
            }
            if r.as_ref().is_none_or(|r| !r.immune) {
                wear_armor(c, &mut sim, &mut stats, target, Zone::of(bp.as_ref()), d.raw);
            }
            match r {
                Some(r) => info!(
                    "combat: {}{} hits {target} on {} for {:.1} (DT {:.0}{}), health {:.0}/{:.0}, knockback {push:.0}",
                    w.edid.as_deref().unwrap_or("?"),
                    if power.is_some() { " power attack" } else { "" },
                    bp.as_ref().and_then(|b| b.name.as_deref()).unwrap_or("the body"),
                    r.dealt,
                    d.dt,
                    crit_note(&a, &d),
                    r.health,
                    r.max_health
                ),
                None => info!("combat: melee hit {target} for {:.1} (no script runtime: not applied)", d.dealt),
            }
        }
    }
}

/// A VATS mode blow's line goes this far past its mark's point, units [design].
const VATS_BLOW_PAST: f32 = 32.0;

/// Explosions set off per frame at most (chains of destructibles) [design].
const MAX_EXPLOSIONS_PER_FRAME: u32 = 64;
/// How far above and below an explosion the ground is looked for to show the impact of
/// its data set there, game units [design].
const EXPLOSION_GROUND_UP: f32 = 32.0;
const EXPLOSION_GROUND_REACH: f32 = 160.0;
/// The game's own water explosion: `DefaultWaterExplosion` ("Water Explosion",
/// `Effects\UnderWaterExplosion.NIF`, the sound `CWaterExplosionSplash`, no damage)
/// [observed: FalloutNV.esm 000001F5]. Its sound plays where an explosion throws up
/// water; its model (bubbles and a glowing shock wave) is not the surface's
/// ([`WATER_SURFACE_EXPLOSION`]).
const WATER_EXPLOSION: FormId = FormId(0x0000_01F5);
/// How far from the surface, over or under it, an explosion throws up water, units:
/// `fExplosionSplashRadius` (250 in FalloutNV.esm) [hypothesis from its name; the
/// recording's first grenade went off 186 units under the surface and threw it up].
const WATER_BLAST_REACH: f32 = 250.0;

/// The surface an explosion at `at` throws water up from: the water it is in or over,
/// when it is within `reach` of the surface and the ground under it (its height, if
/// found) is not above the surface.
fn water_thrown_up(at: V3, surface: Option<f32>, ground: Option<f32>, reach: f32) -> Option<f32> {
    surface.filter(|&s| (at[2] - s).abs() <= reach && ground.is_none_or(|g| g < s))
}

/// The block sound of a blocked blow: the blocker's weapon's `NAM6`, at the blocker (at
/// the listener for the player) [hypothesis: whose weapon; docs/spec-notes/audio.md §15.2].
fn block_sound(c: &Combat, sim: &SimState, sfx: Option<&Sfx>, blocker: FormId) -> Option<PlaySound> {
    let sfx = sfx?;
    let weapon = if blocker == FormId::PLAYER_REF {
        c.weapon?
    } else {
        let inventory = sim.world.peek(blocker)?.inventory.as_ref()?;
        inventory.equipped().into_iter().find(|w| sfx.forms.weapons.contains_key(w))?
    };
    let sound = sfx.forms.weapons.get(&weapon)?.block?;
    debug!("combat: {blocker} blocks: {}", sound_edid(Some(sfx), sound));
    Some(PlaySound { sound, at: if blocker == FormId::PLAYER_REF { None } else { Some(ref_pos(sim, blocker)?) } })
}

/// Where a reference stands (its simulation state, else its placement).
fn ref_pos(sim: &SimState, r: FormId) -> Option<V3> {
    sim.world.peek(r).map(|s| s.pos).or_else(|| sim.refs.get(r).map(|p| p.pos))
}

/// Queue what a destruction change sets off and report the rest (step C7).
fn queue_destruction(c: &mut Combat, sim: &SimState, r: FormId, ch: &DestructionChange, cause: Option<Cause>) {
    if ch.after == ch.before {
        return;
    }
    let at = ref_pos(sim, r).map(|p| [p[0], p[1], p[2] + DESTRUCTION_BLAST_HEIGHT]);
    info!(
        "combat: {r} destruction stage {} -> {}{}{}{}",
        ch.before,
        ch.after,
        if ch.destroyed { ", destroyed" } else { "" },
        if ch.disabled { ", disabled" } else { "" },
        if ch.explosions.is_empty() { String::new() } else { format!(", {} explosion(s)", ch.explosions.len()) }
    );
    if let Some(at) = at {
        c.boom.extend(ch.explosions.iter().map(|&id| QueuedBlast { id, at, cause, normal: None, source: None }));
        // Its stages' debris, thrown by the engine (nvforever `dismember.rs`).
        c.debris.extend(ch.debris.iter().map(|&(d, n)| ObjectDebris { reference: r, at, debris: d, count: n }));
    }
}

/// Object damage to a destructible reference through the script runtime; `cause` set off
/// the stages' explosions.
fn damage_destructible(c: &mut Combat, sim: &mut SimState, script: Option<&mut ScriptState>, r: FormId, amount: f32, cause: Option<Cause>) {
    let Some(rt) = script.and_then(|s| s.rt.as_mut()) else { return };
    let ch = with_world(rt, sim, |rt| rt.game.damage_object(r, amount as f64));
    if let Some(ch) = ch {
        queue_destruction(c, sim, r, &ch, cause);
    }
}

/// A gas cloud `r` reached by `attacker`'s `weapon` (`crate::gas_traps`, #2687): its
/// OnHit and OnHitWith blocks, then OnMagicEffectHit for the weapon's object effect, as
/// for a shot on any object; with `spark` (a gun fired inside it, a blast), a hit with
/// `GasTrapDummy` too when none of those ran a block. Its scripts damage it, and its
/// destruction stages burn it and set off its explosion (`Game::destruction_changes`).
#[allow(clippy::too_many_arguments)]
fn gas_hit(c: &Combat, sim: &mut SimState, script: Option<&mut ScriptState>, r: FormId, attacker: Option<FormId>, weapon: Option<FormId>, spark: bool, what: &str) {
    let Some(rt) = script.and_then(|s| s.rt.as_mut()) else { return };
    let attacker = attacker.filter(|a| *a != FormId(0));
    let weapon = weapon.filter(|w| *w != FormId(0));
    let items: Vec<FormId> = weapon.and_then(|w| c.forms.weapons.get(&w)).and_then(|w| w.enchantment).into_iter().collect();
    let (ran, dummy) = with_world(rt, sim, |rt| {
        let info = HitInfo { attacker, weapon, location: -1, ..HitInfo::default() };
        let ran = rt.game.hit_object(r, info) | rt.game.magic_effect_hits(r, &items, attacker);
        if ran || !spark {
            return (ran, false);
        }
        let dummy = HitInfo { weapon: Some(crate::gas_traps::GAS_TRAP_DUMMY), ..info };
        (rt.game.hit_object(r, dummy), true)
    });
    info!(
        "combat: {what} reaches gas cloud {r}{}{}",
        if dummy { ", hit with GasTrapDummy" } else { "" },
        if ran { ": its hit blocks ran" } else { ": no hit block ran" }
    );
}

/// A scripted Projectile Zone primitive (`XTRI 23`) crossed by a shot (#3662): runs its
/// `OnHit` / `OnHitWith` blocks via `Game::hit_object` without stopping the shot.
/// Re-checks `Game::present` in case an earlier crossed zone on the same shot disabled
/// this one (e.g. `MS01TriggerJunkyard1Ref` disabling both triggers in `OnHit`).
fn projectile_zone_hit(sim: &mut SimState, script: Option<&mut ScriptState>, r: FormId, by: By, projectile: Option<FormId>) {
    let Some(rt) = script.and_then(|s| s.rt.as_mut()) else { return };
    let attacker = (by.actor != FormId(0)).then_some(by.actor);
    let weapon = (by.weapon != FormId(0)).then_some(by.weapon);
    let ran = with_world(rt, sim, |rt| {
        if !rt.game.present(r) {
            return false;
        }
        let info = HitInfo { attacker, weapon, projectile, location: -1, ..HitInfo::default() };
        rt.game.hit_object(r, info)
    });
    if ran {
        info!("combat: shot crosses Projectile Zone {r}: its hit blocks ran");
    }
}

/// Destructible references whose collision overlaps a sphere, with their distance.
fn destructibles_near(phys: &mut PhysState, sim: &SimState, script: Option<&ScriptState>, at: V3, radius: f32) -> Vec<(FormId, f32)> {
    let Some(rt) = script.and_then(|s| s.rt.as_ref()) else { return Vec::new() };
    phys.world.update_queries();
    let shape = fnv_collide::Shape::Sphere { radius };
    let pose = fnv_collide::Iso { translation: at, ..fnv_collide::Iso::IDENTITY };
    let owners = phys.world.queries().overlap(&shape, &pose, &Filter::default()).unwrap_or_default();
    let mut out: Vec<(FormId, f32)> = Vec::new();
    for o in owners {
        let Owner::Ref { form_id, .. } = o else { continue };
        let r = FormId(form_id);
        if out.iter().any(|x| x.0 == r) || !rt.game.is_destructible(r) {
            continue;
        }
        let d = ref_pos(sim, r).map_or(radius, |p| vec::len(vec::sub(p, at)).min(radius));
        out.push((r, d));
    }
    out
}

/// What a shot sends out: sounds, impact effects, explosions, broken weapons, and what
/// each impact picked (for `crate::world_sounds`).
/// What [`actor_attacks`] sends: shots, their sounds (scaled ones apart), hits and pushes.
type ActorAttackOut<'w> = (MessageWriter<'w, ShotFired>, MessageWriter<'w, PlaySound>, MessageWriter<'w, ActorHit>, MessageWriter<'w, ActorPush>, MessageWriter<'w, ScaledSound>);
type ShotEffects<'w> = (MessageWriter<'w, PlaySound>, MessageWriter<'w, ImpactFx>, MessageWriter<'w, ExplosionFx>, MessageWriter<'w, WeaponBroken>, MessageWriter<'w, crate::world_sounds::ImpactPicked>, MessageWriter<'w, crate::RefChanged>);

/// Fly the projectiles, deliver hits when due, set off explosions (and what they set
/// off: destruction stages, detonations).
/// What says whether the player blocks: its controls, its weapon's state, where it looks.
type Guard<'w> = (Option<Res<'w, PlayerControl>>, Option<Res<'w, WeaponAnim>>, Option<Res<'w, Player>>);

/// Other actors' attacks (module notes): their shots go into flight as the player's do,
/// their blows are dealt at once.
#[allow(clippy::too_many_arguments)]
fn actor_attacks(
    mut c: ResMut<Combat>,
    mut sim: ResMut<SimState>,
    mut phys: ResMut<PhysState>,
    mut stats: ResMut<CombatStats>,
    mut script: Option<ResMut<ScriptState>>,
    mut feedback: ResMut<HitFeedback>,
    mut hud: Option<ResMut<HudMessage>>,
    (mut shots, mut blows): (MessageReader<ActorFire>, MessageReader<ActorStrike>),
    (mut fired, mut sounds, mut messages, mut pushes, mut scaled): ActorAttackOut,
    sfx: Option<Res<Sfx>>,
    (mut impacts, mut broken, mut knocks): (MessageWriter<ImpactFx>, MessageWriter<WeaponBroken>, MessageWriter<Knockback>),
    (control, anim, player): Guard,
    mut dm: ResMut<crate::dismember::Dismemberment>,
    mut grenade_probe: ResMut<held_grenade_probe::Probe>,
    mut launch_probe: Option<ResMut<projectile_launch_probe::Probe>>,
    time: Res<Time>,
) {
    sync_condition_penalty(&mut c, script.as_deref());
    let c = &mut *c;
    let gravity = phys.world.params.gravity;
    // The player blocks with Block held and fists or a melee weapon drawn (combat.md §5.4);
    // NPC blows from its front are then blocked as an NPC's are (combat-ai.md §14.3).
    let player_guard = (c.melee && control.as_deref().is_some_and(|k| k.aiming) && anim.as_deref().is_some_and(|a| a.ready)).then(|| player.as_deref().map(|p| p.look_dir())).flatten().map(|d| [d[0], d[1]]);
    let script_shots = std::mem::take(&mut c.script_shots);
    for (s, scripted) in shots.read().map(|s| (s, false)).chain(script_shots.iter().map(|s| (s, true))) {
        if let Some(probe) = launch_probe.as_deref_mut() { probe.request(s, scripted); }
        let probe_request = grenade_probe.request(s, &sim.world);
        // The shooter's own body is never hit: the player's is `PLAYER`, not its FormID (a
        // player's FireWeapon hit the player at the muzzle, 2026-10-27).
        let own = if s.shooter == FormId::PLAYER_REF { PLAYER } else { s.shooter.0 as ActorId };
        let carried = |a: FormId| sim.world.peek(s.shooter).and_then(|st| st.inventory.as_ref()).is_some_and(|i| i.count(a) > 0);
        let ammo = c.forms.ammo_for(s.weapon, carried);
        let Some(chain) = c.forms.chain(s.weapon, ammo) else { grenade_probe.finish_request(probe_request, 0); continue };
        let launch = projectile_launch::resolve(&c.forms, &chain, script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game));
        let chain = launch.chain;
        let projectile = launch.projectile.as_ref();
        let Some(proj) = chain.projectile else { grenade_probe.finish_request(probe_request, 0); continue };
        stats.actor_shots += 1;
        let range = projectile.map_or(0.0, |p| p.range);
        let reach = if range > 0.0 { range } else { AIM_REACH };
        let leaving = {
            let world = view(&mut phys, script.as_deref(), Some(&sim));
            shot::leaving(&world, s.origin, s.aim, s.muzzle, reach, Some(own), false)
        };
        let seen_from = leaving.seen.at(leaving.origin, 0.0);
        // The mods of the unit in use (the equipped one, else the next to equip), as
        // they are at the shot: its sound and its loudness for detection.
        let mods = sim.world.peek(s.shooter).and_then(|st| st.inventory.as_ref()).map_or(0, |i| i.mods_in_use(s.weapon));
        fired.write(ShotFired { projectile: proj, weapon: s.weapon, mods, shooter: s.shooter, at: Some(seen_from), dir: Some(leaving.aim) });
        // The player's tweak: an NPC's spent round leaves it its case, as the player's does
        // (no perks) [design: docs/design/tweaks.md].
        let npc_casings = s.shooter != FormId::PLAYER_REF && !scripted && script.as_deref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.npcs_earn_casings);
        if let Some(spent) = ammo.and_then(|a| c.forms.ammo.get(&a)).and_then(|a| a.consumed).filter(|(i, p)| npc_casings && i.0 != 0 && *p > 0.0) {
            if c.spent_rng.next_f32() * 100.0 < spent.1 && sim.world.add_item(s.shooter, spent.0, 1).is_ok() {
                debug!("combat: {} keeps the spent item {} (tweak)", s.shooter, spent.0);
            }
        }
        // Heard where it is fired: the weapon's 3D shoot sound, its distant one beyond
        // the 3D sound's reach, the mod set's with a Silence or Suppressor mod on the
        // unit in use (weapon-mods.md §7).
        if let Some(fx) = sfx.as_deref().and_then(|x| x.forms.weapons.get(&s.weapon)) {
            let silenced = mods != 0 && c.forms.weapons.get(&s.weapon).is_some_and(|w| w.sound_mod_on(mods));
            let shoot = fx.shoot_sounds(silenced);
            let near_max = shoot.near.and_then(|n| sfx.as_deref().and_then(|x| x.forms.sounds.get(&n))).map_or(0.0, |n| n.max_distance);
            let distance = player.as_deref().map_or(0.0, |p| vec::len(vec::sub(p.eye(), seen_from)));
            let (sound, far) = shoot.heard_at(distance, near_max);
            if let Some(sound) = sound {
                debug!("combat: npc {} shot sound {} ({}, {})", s.shooter, sound_edid(sfx.as_deref(), sound), if shoot.silenced { "silenced" } else { "plain" }, if far { "distant" } else { "3D" });
                shot_sound(&mut sounds, &mut scaled, sound, Some(seen_from), false);
            }
        }
        let (tracer_chance, force) = projectile.map_or((0.0, 0.0), |p| (p.tracer_chance, p.impact_force));
        let by = By { actor: s.shooter, weapon: s.weapon, ammo, harmless: s.harmless, sure: s.sure, vats: false, sneaking: false };
        let rng = &mut c.rng;
        let mut rand = || rng.next_f32();
        let mut ps = projectile.map_or_else(Vec::new, |p| shot::fire_projectile(p, &chain, leaving.origin, leaving.aim, s.cone, Some(own), gravity, &c.tuning, &mut rand));
        // The mods on the unit it uses: Increase Projectile Speed (weapon-data-fields.md §5).
        let mods = sim.world.peek(s.shooter).and_then(|st| st.inventory.as_ref()).map_or(0, |i| i.mods_in_use(s.weapon));
        if let Some(m) = c.forms.weapons.get(&s.weapon).filter(|_| mods != 0).and_then(|w| w.modded(mods).projectile_speed_mult) {
            ps.iter_mut().for_each(|p| p.speed_up(m));
        }
        // A grenade dropped live by its dying holder falls rather than flies (`death_drops`).
        if let Some(i) = c.dropped_live.iter().position(|a| *a == s.shooter) {
            c.dropped_live.swap_remove(i);
            ps.iter_mut().for_each(|p| p.speed_up(LIVE_DROP_SPEED));
        }
        let probe_created = ps.len();
        let (tracer_over, draw_hitscan) = shot_switches(script.as_deref());
        for p in ps {
            let roll = c.rng.next_f32() < tracer_chance;
            let rolled = tracer_over.map_or(roll, |o| o && tracer_chance > 0.0) || c.tracer_hold.is_some();
            let shown = rolled || DRAW_EVERY_PROJECTILE || (draw_hitscan && p.hitscan);
            let tracer = rolled || (shown && instant(&p));
            let dir = vec::normalize(p.vel);
            c.next_visual += 1;
            c.flying.push(Flying { p, vats_aim: None, nearest: f32::INFINITY, told_off: false, id: c.next_visual, proj, shown, dir, from_muzzle: s.muzzle.is_some(), origin: leaving.origin, seen: leaving.seen, tracer, projectiles_per_shot: chain.projectiles_per_shot, force, by, rref: None, registered: false, speed_mult: 1.0, ended_at: None, gassed: Vec::new(), proj_zones_hit: Vec::new() });
            if let Some(flying) = c.flying.last() {
                grenade_probe.accepted(probe_request, s, &flying.p, flying.proj, flying.id);
                if let Some(probe) = launch_probe.as_deref_mut() { probe.inserted(s, scripted, flying, &sim.world); }
            }
        }
        grenade_probe.finish_request(probe_request, probe_created);
        let wname = c.forms.weapons.get(&s.weapon).and_then(|w| w.edid.as_deref()).unwrap_or("?");
        if scripted {
            // A script's FireWeapon (a rigged shotgun, a turret): worth a line.
            info!("combat: FireWeapon: {} fires {wname} from ({:.0}, {:.0}, {:.0})", s.shooter, leaving.origin[0], leaving.origin[1], leaving.origin[2]);
        } else {
            debug!("combat: {} fires {wname} (cone {:.1}°)", s.shooter, s.cone);
        }
        // The shot wears the shooter's weapon as the player's (§17); a scripted
        // FireWeapon does not, it is the reference's, not a carried gun's [design].
        if !scripted {
            if let Some(w) = c.forms.weapons.get(&s.weapon) {
                let effects: Vec<&AmmoEffect> = chain.ammo_effects.iter().filter_map(|e| c.forms.ammo_effects.get(e)).collect();
                let points = condition::weapon_wear(&c.rules, w, &effects);
                wear_actor_weapon(&c.forms, &mut sim, &mut stats, &mut broken, script.as_deref_mut(), s.shooter, s.weapon, points, seen_from);
            }
        }
    }
    grenade_probe.receiver_done(time.elapsed_secs());
    for b in blows.read() {
        // The weapon: its own, else the Fists; a creature's own attack damage in the Fists'
        // place [design: the Fists' other figures stand in for a creature's].
        let Some(id) = b.weapon.or(c.fists) else { continue };
        // [design] NPCs carrying an already-modded weapon (e.g. player-modded) apply its
        // weapon-mod stats (`WeaponStats::modded`) even when `npc-weapon-mods` is off;
        // unmodded NPC weapons (`mods == 0`) stay identical to base weapon stats.
        let mods = if b.creature_damage.is_none() { sim.world.peek(b.attacker).and_then(|st| st.inventory.as_ref()).map_or(0, |i| i.mods_in_use(id)) } else { 0 };
        let Some(mut w) = c.forms.weapons.get(&id).map(|w| if mods != 0 { w.modded(mods) } else { w.clone() }) else { continue };
        script_weapon_critical_damage(script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game), id, &mut w);
        if let Some(d) = b.creature_damage {
            w.base_damage = d.round() as i32;
        }
        stats.actor_strikes += 1;
        let mut a = attacker(script.as_deref_mut(), &mut sim, &c.rules, b.attacker, &w, false);
        a.sneaking = sneak_attack(script.as_deref(), b.attacker, false, b.target);
        // A person's power attack: × its direction's fDamagePowerAttack…Bonus, as the
        // player's (combat-rules.md §10.1); a creature's deals its usual damage.
        let power = b.anim.filter(|_| b.creature_damage.is_none()).and_then(fnv_combat::melee::PowerDir::of_group).map_or(1.0, |d| d.damage(&c.rules));
        // The player's tweak: NPCs' weapons hit as if whole [design: docs/design/tweaks.md].
        let whole = b.attacker != FormId::PLAYER_REF && script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.npc_weapons_whole);
        // The part of the player struck; the player's tweak gives its damage multiplier
        // to the blow, as the player's own blows have it [design: docs/design/tweaks.md].
        let bp = (b.target == FormId::PLAYER_REF).then(|| player_body_part(c, &phys, player.as_deref(), b.point)).flatten();
        let located = script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.npc_melee_part_damage);
        let part = bp.as_ref().filter(|_| located).map_or_else(HitPart::default, BodyPart::hit_part);
        let h = HitInput { weapon: &w, projectiles: 1, condition: if whole { 1.0 } else { item_condition(&sim, b.attacker, id) }, ammo_effects: &[], part, power };
        if power != 1.0 {
            info!("combat: {}'s power attack {} on {}: damage x{power:.1}", b.attacker, b.anim.unwrap_or("?"), b.target);
        }
        let (dt_v, dr) = target_armor_for_zone(c, script.as_deref_mut(), &mut sim, b.target, Some(Zone::of(bp.as_ref())));
        let roll = c.rng.next_f32();
        let (ap, tp) = hit_perks(script.as_deref_mut(), &mut sim, b.attacker, id, Some(b.target));
        let resist = target_resist(script.as_deref_mut(), &mut sim, b.target, &w);
        let nerve = companion_nerve(script.as_deref_mut(), &mut sim, b.target);
        let mut d = damage::hit(&c.rules, &c.tuning, &h, &a, &Target { dt: dt_v, dr, resist, nerve, ..Target::default() }, &ap, &tp, roll);
        log_hit_perks(b.attacker, Some(b.target), &ap, &tp, &d);
        // A blocker facing the blow stops part of it (docs/design/combat-ai.md §14.3); a
        // power attack (a creature's, `ActorStrike::anim`) breaks through (combat-rules.md
        // §12): an NPC staggers, the player cannot block for a while.
        let player_guard = player_guard.filter(|_| c.time >= c.guard_down_until);
        let guard = if b.target == FormId::PLAYER_REF { player_guard } else { c.guarding.get(&b.target).copied() };
        let mut guard_broken = false;
        let mut blocked = false;
        if guard.is_some_and(|f| damage::from_front(f, b.dir)) {
            let before = d.dealt;
            (d.dealt, guard_broken) = damage::against_block(&c.tuning, before, b.anim.is_some());
            if guard_broken {
                stats.blocks_broken += 1;
                info!("combat: {}'s power attack breaks {}'s block ({before:.1}, all dealt)", b.attacker, b.target);
                if b.target == FormId::PLAYER_REF {
                    c.guard_down_until = c.time + c.tuning.guard_break;
                }
            } else {
                blocked = true;
                stats.blows_blocked += 1;
                info!("combat: {} blocks {}'s blow: {:.1} of {before:.1} stopped", b.target, b.attacker, before - d.dealt);
                if b.target == FormId::PLAYER_REF {
                    c.player_blocked_at = Some(c.time);
                }
                c.blocked_blows.push(BlockedBlow { attacker: b.attacker, blocker: b.target, damage: before, dealt: d.dealt });
                if let Some(sound) = block_sound(c, &sim, sfx.as_deref(), b.target) {
                    sounds.write(sound);
                }
            }
        }
        // [hypothesis] Melee's part multipliers are unmeasured (combat-rules.md §12):
        // keep Health on HitPart::default(), but give player blows a location and limb.
        // A creature uses this path's Fists Limb Dmg Mult in the absence of its own weapon.
        let info = HitInfo {
            attacker: Some(b.attacker),
            weapon: b.weapon,
            location: bp.as_ref().map_or(0, BodyPart::location),
            critical: d.critical,
            blocking: blocked,
            sneak_attack: a.sneaking,
            base_weapon_damage: f64::from(d.base + d.crit_bonus),
            hit_pos: Some(b.point),
            hit_dir: fnv_game::combat::unit_hit_dir(b.dir),
            limb: limb_hit(bp.as_ref(), &d, &w),
            ..HitInfo::default()
        };
        if b.target != FormId::PLAYER_REF {
            pushes.write(ActorPush { form_id: b.target.0, point: b.point, dv: vec::scale(b.dir, MELEE_PUSH) });
        }
        if let Some(sound) = sfx.as_deref().and_then(|x| b.weapon.and_then(|w| x.forms.weapons.get(&w))).and_then(|f| f.shoot_3d.or(f.shoot_2d)) {
            sounds.write(PlaySound { sound, at: Some(b.point) });
        }
        // Its impact on the one struck, as the player's blows (item 24); a creature's own
        // attack by its CREA CNAM impact data set (docs/spec-notes/actor-fields.md §2).
        if let Some(s) = sfx.as_deref() {
            // Both from the Model/Animation source [hypothesis: CREA CNAM is in that group
            // as NAM4 is; docs/spec-notes/actors.md §1.3].
            let own = b.creature_damage.and(model_base(script.as_deref(), &sim, b.attacker.0)).and_then(|base| s.forms.creature_impact_set(base));
            let picked = match own {
                Some(set) => {
                    let material = actor_material(s, script.as_deref(), &sim, b.target.0);
                    (s.forms.impact_id(set, material), material)
                }
                None => impact_for(s, script.as_deref(), &sim, id, Some(b.target.0 as ActorId), None),
            };
            if let (Some(impact), _) = picked {
                stats.melee_impacts += 1;
                show_impact(s, script.as_deref(), &sim, ImpactFx { impact, shooter: b.attacker, point: b.point, normal: vec::scale(b.dir, -1.0), dir: b.dir, actor: Some(b.target.0), no_spray: false, no_decal: false, splatter: false }, &mut sounds, &mut impacts);
            }
        }
        // A blow that lands wears the weapon once, as the player's (§17); the Fists and
        // a creature's own attack are no item and do not wear.
        if let (Some(weapon), true) = (b.weapon, b.creature_damage.is_none()) {
            let points = condition::weapon_wear(&c.rules, &w, &[]);
            wear_actor_weapon(&c.forms, &mut sim, &mut stats, &mut broken, script.as_deref_mut(), b.attacker, weapon, points, b.point);
        }
        let items = hit_items(c, script.as_deref_mut(), &mut sim, &mut stats, b.attacker, b.weapon, b.anim);
        note_hit_point(script.as_deref_mut(), b.target, b.point);
        note_armour(script.as_deref_mut(), b.target, &d);
        let r = apply_hit(script.as_deref_mut(), &mut sim, b.target, d.dealt, info, &mut stats, &mut feedback, hud.as_deref_mut(), &mut messages, &items, guard_broken, crit_effect(c, b.weapon));
        armour_lines(script.as_deref_mut(), Some(b.attacker), b.target, &d, c.tuning.dt_floor);
        // A killing blow (or one on a corpse) may dismember the part it lands on, by the
        // weapon's On Hit, as the player's blows (dismemberment.md §4, §7); the part is the
        // one a short line along the blow meets at its landing point.
        if r.as_ref().is_some_and(|r| r.died || r.corpse) && b.target != FormId::PLAYER_REF {
            let from = vec::sub(b.point, vec::scale(b.dir, BLOW_PROBE));
            let to = vec::add(b.point, vec::scale(b.dir, BLOW_PROBE));
            let facing = facing_part(&view(&mut phys, script.as_deref(), Some(&sim)), b.target.0 as ActorId, from, to);
            if let Some(check) = r.as_ref().zip(facing).and_then(|(rr, (part, point))| dismember_check(b.target, Some(part), &w, rr, Some(b.attacker), point, b.dir)) {
                dm.checks.push(check);
            }
        }
        spread_hit_areas(script.as_deref_mut(), &mut sim, &mut stats, b.target, &items, info.attacker);
        if r.as_ref().is_none_or(|r| !r.immune) {
            wear_armor(c, &mut sim, &mut stats, b.target, Zone::Body, d.raw);
        }
        // A living actor struck (the player too) is pushed back by the knockback; not the
        // player in god mode (`tgm`), which captures and smoke tests keep in place.
        if r.as_ref().is_some_and(|r| !r.died && !r.immune && !r.knocked_out) {
            let agility = actor_av(script.as_deref_mut(), &mut sim, b.target, fnv_game::actor_values::AGILITY).unwrap_or(5.0);
            let force = damage::knockback_force(&c.rules, d.dealt, agility);
            if let Some(k) = Knockback::of(b.target.0, b.dir, force, c.rules.knockback_time) {
                knocks.write(k.by(b.attacker, c.next_blow(), b.anim.is_some()));
            }
        }
        if b.target != FormId::PLAYER_REF {
            if let Some(r) = r {
                info!("combat: {} strikes {} with {} for {:.1}, health {:.0}/{:.0}", b.attacker, b.target, w.edid.as_deref().unwrap_or("?"), r.dealt, r.health, r.max_health);
            }
        }
    }
}

/// SetNoGunWobble (`fnv_game::nvse_jip14`): 0 for an actor whose gun does not wobble
/// (no actor spread [hypothesis: the wobble the page names is the actor's spread, the
/// aim's own unsteadiness]), else 1.
pub fn wobble(script: Option<&ScriptState>, actor: FormId) -> f32 {
    if script.and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.gun_wobble_off(actor)) {
        0.0
    } else {
        1.0
    }
}

/// The shot `FireWeapon` asks of `from` (`fnv_game::functions` fighting): from the
/// reference's centre straight ahead along its rotation, with no spread [documented:
/// GECK wiki FireWeapon, "from the center of the reference … straight out"]. The centre
/// is the reference's origin, an actor's raised to its middle (half of 128 units)
/// [hypothesis]; ahead is its heading (z, clockwise from north) tilted by its x angle,
/// a positive x looking down [hypothesis: the game turns both clockwise]. The model's
/// projectile node is not used yet; FireWeaponEx's node (`fnv_game::nvse_jip14`) gives
/// the start when its place is known.
fn script_shot(sim: &SimState, from: FormId, weapon: FormId, muzzle: Option<[f32; 3]>) -> Option<ActorFire> {
    // A reference nothing has moved yet (a rigged shotgun) stands where it was placed.
    let (pos, rot) = match sim.world.peek(from) {
        Some(s) => (s.pos, s.rot),
        None => sim.world.placed.get(from).map(|p| (p.pos, p.rot))?,
    };
    let actor = from == FormId::PLAYER_REF || sim.is_actor(from.0);
    let [x, _, z] = rot;
    let aim = [z.sin() * x.cos(), z.cos() * x.cos(), -x.sin()];
    let origin = muzzle.unwrap_or([pos[0], pos[1], pos[2] + if actor { 64.0 } else { 0.0 }]);
    Some(ActorFire { shooter: from, weapon, origin, aim, cone: 0.0, muzzle: None, harmless: false, sure: None })
}

/// A door travel leaves the shots of the space left behind: what is in flight, the hits
/// on their way and the blasts not yet gone off stand at coordinates of that space, and
/// would go on in the new one at the same numbers (#1986). They are dropped, their script
/// references with them [design]. A charge or mine at rest is a reference of its cell and
/// stays there. Not gated on play, as the loading screen is up when the travel goes out.
fn leave_space_shots(mut travel: MessageReader<crate::Travel>, mut c: ResMut<Combat>, mut sim: ResMut<SimState>, mut script: Option<ResMut<ScriptState>>, mut visuals: ResMut<ShotVisuals>, mut snap: Option<ResMut<crate::bullet_snap::BulletSnap>>) {
    if travel.read().count() == 0 {
        return;
    }
    if let Some(s) = snap.as_deref_mut() {
        s.clear_tracking();
    }
    c.splashes.clear();
    visuals.water_entries.clear();
    let n = c.flying.len() + c.pending.len() + c.boom.len();
    if n == 0 {
        return;
    }
    let gone: Vec<FormId> = c.flying.iter().filter_map(|f| f.rref).chain(c.pending.iter().filter_map(|p| p.rref)).collect();
    c.flying.clear();
    c.pending.clear();
    c.boom.clear();
    c.ghosts.clear();
    visuals.flying.clear();
    if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
        with_world(rt, &mut sim, |rt| {
            for r in gone {
                rt.game.projectile_gone(r);
            }
        });
    }
    info!("combat: a door travel drops {n} shot(s) and blast(s) of the space left");
}

/// The charges `actor` put down that wait for their detonator (#1986): the references
/// made where a Detonates projectile (C-4) came to rest ([`SettledMine`], placed by
/// `crate::mines`), owned by `actor`, not taken or gone off, in the loaded cells (one in a
/// cell that is not loaded stays where it is [design]). With where each lies, its
/// explosion and the weapon it came from (its PROJ's default weapon).
fn placed_charges(forms: &CombatForms, sim: &SimState, phys: &PhysState, actor: FormId) -> Vec<(FormId, V3, FormId, FormId)> {
    sim.world
        .created()
        .filter(|(id, r)| !r.deleted() && r.extra.owner == Some(actor) && phys.created.contains_key(&id.0) && sim.present(id.0))
        .filter_map(|(id, r)| {
            let p = forms.projectiles.get(&r.base).filter(|p| p.has("Detonates"))?;
            Some((id, r.pos, p.explodes()?, p.default_weapon.unwrap_or(FormId(0))))
        })
        .collect()
}

/// An explicitly removed shot cannot later hit or detonate. Disable alone remains
/// reversible. A naturally completed shot may still have a fading Ghost, so only
/// MarkForDelete cancels a Ghost without a live flight or pending hit.
/// Cancellation at the next consumer is an engineering policy, not native timing.
fn retire_deleted_shots(c: &mut Combat, game: &mut fnv_game::game::Game<'_>) {
    let mut gone: std::collections::HashSet<FormId> = c.flying.iter().filter_map(|f| f.rref)
        .chain(c.pending.iter().filter_map(|p| p.rref))
        .filter(|&r| game.world.peek(r).is_some_and(|s| s.shot && s.deleted()))
        .collect();
    gone.extend(c.ghosts.iter().filter_map(|g| g.rref).filter(|r| {
        game.extra.get(r).is_some_and(|e| e.marked_for_delete)
            && game.world.peek(*r).is_some_and(|s| s.shot && s.deleted())
    }));
    if gone.is_empty() {
        return;
    }
    c.flying.retain(|f| f.rref.is_none_or(|r| !gone.contains(&r)));
    c.pending.retain(|p| p.rref.is_none_or(|r| !gone.contains(&r)));
    c.ghosts.retain(|g| g.rref.is_none_or(|r| !gone.contains(&r)));
    for r in gone {
        game.retire_deleted_projectile(r);
    }
}

/// Apply explicit source edits before the next physical publication/outcome.
/// Position never comes back from ordinary engine world writes, avoiding a
/// flight -> World -> flight feedback loop. Attribution does not change the
/// physical shooter used for collision/remote-detonation admission.
fn sync_live_projectile_edits(c: &mut Combat, game: &mut fnv_game::game::Game<'_>) {
    let positions = std::mem::take(&mut game.gaps.projs.position_sets);
    for f in &mut c.flying {
        let Some(r) = f.rref else { continue };
        if let Some(source) = game.gaps.projs.shots.get(&r).map(|s| s.source) {
            f.by.actor = source;
        }
        if let Some(at) = positions.get(&r).filter(|at| at.iter().all(|v| v.is_finite())) {
            f.p.pos = *at;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn fly(
    time: Res<Time>,
    mut c: ResMut<Combat>,
    mut sim: ResMut<SimState>,
    mut phys: ResMut<PhysState>,
    mut stats: ResMut<CombatStats>,
    mut script: Option<ResMut<ScriptState>>,
    mut feedback: ResMut<HitFeedback>,
    mut hud: Option<ResMut<HudMessage>>,
    mut messages: MessageWriter<ActorHit>,
    mut pushes: MessageWriter<ActorPush>,
    (parts, player): (Res<ActorParts>, Option<Res<Player>>),
    mut visuals: ResMut<ShotVisuals>,
    anim: Res<WeaponAnim>,
    sfx: Option<Res<Sfx>>,
    mut effects: ShotEffects,
    (mut dm, mut snap): (ResMut<crate::dismember::Dismemberment>, Option<ResMut<crate::bullet_snap::BulletSnap>>),
) {
    sync_condition_penalty(&mut c, script.as_deref());
    let dt = time.delta_secs();
    let c = &mut *c;
    if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
        with_world(rt, &mut sim, |rt| {
            retire_deleted_shots(c, &mut rt.game);
            sync_live_projectile_edits(c, &mut rt.game);
        });
    }
    // What scripts asked (DetonatePlacedExplosives, a placed explosion, DamageObject's
    // stage changes) and what burns down (self-damage).
    if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
        let (requests, burning) = with_world(rt, &mut sim, |rt| {
            let mut changes = std::mem::take(&mut rt.game.destruction_changes);
            changes.extend(rt.game.destruction_tick(dt as f64));
            (std::mem::take(&mut rt.game.combat_requests), changes)
        });
        for req in requests {
            let actor = match req {
                CombatRequest::Detonate(actor) => actor,
                CombatRequest::Fire { from, weapon, muzzle } => {
                    match script_shot(&sim, from, weapon, muzzle) {
                        Some(shot) => c.script_shots.push(shot),
                        None => debug!("combat: FireWeapon on {from}: no such reference in the world"),
                    }
                    continue;
                }
                CombatRequest::Explode { explosion, at, source } => {
                    c.boom.push(QueuedBlast { id: explosion, at, cause: None, normal: None, source });
                    continue;
                }
                CombatRequest::Dismember { actor, location, by, explode } => {
                    match crate::dismember::scripted_check(c, &parts, &sim, actor, location, by, explode) {
                        Some(check) => dm.checks.push(check),
                        None => info!("dismember: KillActor on {actor}: no part at location {location} can go (or its body is not drawn)"),
                    }
                    continue;
                }
            };
            let shooter = if actor == FormId::PLAYER_REF { Some(PLAYER) } else { Some(actor.0 as ActorId) };
            let mut n = 0;
            for f in c.flying.iter_mut().filter(|f| f.p.shooter == shooter) {
                if let Some(ProjEvent::Explode { explosion, point, normal }) = f.p.detonate() {
                    c.boom.push(QueuedBlast { id: explosion, at: point, cause: Some(Cause::of(&f.by)), normal, source: None });
                    n += 1;
                }
            }
            c.flying.retain(|f| f.p.alive);
            // Charges at rest are references ([`placed_charges`], #1986).
            for (id, pos, explosion, weapon) in placed_charges(&c.forms, &sim, &phys, actor) {
                if let Err(e) = sim.world.set_deleted(id, true) {
                    warn!("combat: charge {id} could not be removed: {e}");
                    continue;
                }
                effects.5.write(crate::RefChanged::new(id.0));
                c.boom.push(QueuedBlast { id: explosion, at: pos, cause: Some(Cause { actor, weapon }), normal: None, source: None });
                n += 1;
            }
            info!("combat: {actor} detonates placed explosives: {n}");
        }
        for (r, ch) in burning {
            queue_destruction(c, &sim, r, &ch, None);
        }
    }
    // Each shot is a temporary reference of its PROJ for scripts (`fnv_game::nvse_proj`):
    // made the frame it is fired, moved as it flies; a speed multiplier a script set
    // speeds the flight or the hit's arrival [observed: proj1].
    if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
        let forms = &c.forms;
        let (flying, pending, now) = (&mut c.flying, &mut c.pending, c.time);
        with_world(rt, &mut sim, |rt| {
            for f in flying.iter_mut() {
                if !f.registered {
                    f.registered = true;
                    let damage = forms.weapons.get(&f.by.weapon).map_or(0.0, |w| w.base_damage as f32);
                    f.rref = rt.game.projectile_fired(f.proj, f.by.actor, f.by.weapon, damage, f.p.pos);
                    debug!("combat: shot reference {:?} for {} (speed {:.0}, hitscan {})", f.rref, f.proj, f.p.speed, f.p.hitscan);
                }
                let Some(r) = f.rref else { continue };
                if let Some(m) = rt.game.gaps.projs.shots.get(&r).map(|s| s.speed_mult).filter(|m| *m > 0.0 && *m != f.speed_mult) {
                    let k = m / f.speed_mult;
                    f.p.vel = vec::scale(f.p.vel, k);
                    f.p.speed *= k;
                    f.speed_mult = m;
                }
                rt.game.projectile_moved(r, f.p.pos, f.p.age, f.p.travelled);
                // GetDetonationTimer (`fnv_game::nvse_jip14`): a timed explosive's time
                // left; a mine or a proximity charge not set off, FLOAT_MAX [documented];
                // none for the rest.
                let fuse = match (f.p.explosion.is_some(), f.p.alt_trigger) {
                    (true, true) if f.p.mine || f.p.proximity > 0.0 => Some(f32::MAX),
                    (true, true) if f.p.timer > 0.0 => Some((f.p.timer - f.p.age).max(0.0)),
                    _ => None,
                };
                rt.game.set_fuse(r, fuse);
            }
            for (r, secs) in rt.game.take_fuse_sets() {
                if let Some(f) = flying.iter_mut().find(|f| f.rref == Some(r)) {
                    f.p.timer = f.p.age + secs;
                    f.p.proximity = 0.0;
                    info!("combat: a script sets {r}'s fuse to {secs:.2} s");
                }
            }
            for p in pending.iter_mut() {
                let Some(r) = p.rref else { continue };
                if let Some(m) = rt.game.gaps.projs.shots.get(&r).map(|s| s.speed_mult).filter(|m| *m > 0.0 && *m != p.speed_mult) {
                    p.due = now + (p.due - now).max(0.0) * p.speed_mult / m;
                    p.speed_mult = m;
                }
                let travelled = rt.game.gaps.projs.shots.get(&r).map_or(0.0, |s| s.travelled);
                rt.game.projectile_moved(r, p.point, now - p.since, travelled);
            }
        });
    }
    (c.vats_blast.toward, c.vats_blast.off) = (0, 0);
    if !visuals.water_entries.is_empty() {
        visuals.water_entries.clear();
    }
    if c.flying.is_empty() && c.pending.is_empty() && c.boom.is_empty() && c.ghosts.is_empty() && c.splashes.is_empty() {
        if !visuals.flying.is_empty() {
            visuals.flying.clear();
        }
        return;
    }
    // Shots whose flight ended this frame without a hit to deliver: their references go.
    let mut over: Vec<(FormId, f32)> = Vec::new();
    // The gas clouds (#2687) and scripted Projectile Zone primitives (XTRI 23, #3662)
    // shots can cross this frame.
    // Read with the world in the runtime (`with_world`): outside it, nothing is present.
    let (gas, proj_zones) = script
        .as_deref_mut()
        .and_then(|s| s.rt.as_mut())
        .map(|rt| with_world(rt, &mut sim, |rt| (crate::gas_traps::volumes(rt), crate::script_trigger::projectile_zones(rt))))
        .unwrap_or_default();
    let mut gas_hits: Vec<(FormId, By, bool)> = Vec::new();
    let mut proj_zone_hits: Vec<(FormId, By, Option<FormId>)> = Vec::new();
    {
        let world = view(&mut phys, script.as_deref(), Some(&sim));
        let hold = c.tracer_hold;
        for f in &mut c.flying {
            let before = f.p.pos;
            // Where it is drawn: from the muzzle, closing on its path (`Seen`).
            let seen_before = f.seen.at(before, f.p.travelled);
            // Found at once: what is drawn flies (or, a beam, stands) from there to
            // where it ended.
            let at_once = instant(&f.p) && f.p.age == 0.0;
            let fade = c.forms.projectiles.get(&f.proj).map_or(0.0, |p| p.fade_duration);
            let (id, proj, speed, beam, owner) = (f.id, f.proj, f.p.speed, f.p.motion == Motion::Beam, f.by.actor);
            // What stays on show after the shot (a beam, the line along the shot) keeps
            // its start on the muzzle: the attack clip moves the gun in the frames after
            // the shot [observed: the laser rifle's muzzle is 10 units higher one frame
            // after it fires], and a beam left where the muzzle was hangs below the barrel.
            // Only the player's own: the muzzle followed is the player's weapon's, so an
            // enemy's shot was drawn from the player's gun (the owner's playtest list 5
            // item 20).
            // The player's tweak leaves beams where they were fired [design: docs/design/tweaks.md].
            let detached = script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.detached_beams);
            let follow = at_once && f.from_muzzle && f.by.actor == FormId::PLAYER_REF && !detached;
            // A thrown weapon lands where its flight ends (`crate::thrown`): the weapon
            // thrown, else the PROJ's Default Weapon Source.
            let lands = c.forms.projectiles.get(&f.proj).filter(|p| p.lands_as_weapon()).and_then(|p| {
                let thrown = c.forms.weapons.get(&f.by.weapon).is_some_and(|w| w.projectile == Some(f.proj));
                if thrown { Some(f.by.weapon) } else { p.default_weapon }
            });
            let rref = f.rref;
            let snap_active = snap.as_deref().is_some_and(crate::bullet_snap::BulletSnap::active);
            let snap_damage = c.forms.weapons.get(&f.by.weapon).map_or(0.0, |w| w.base_damage as f32);
            let snap_supersonic = c.forms.projectiles.get(&f.proj).is_some_and(|p| p.has("Supersonic"));
            let snap_eligible = f.p.motion == Motion::Missile && !f.p.mine && !f.p.detonates && (f.p.explosion.is_none() || snap_supersonic);
            let ghost = move |to: V3, hit_player: bool| Ghost { id, proj, rref, owner, from: seen_before, to, speed, travelled: 0.0, beam, left: fade, fade, fresh: true, follow: follow && beam, damage: snap_damage, snap_eligible, hit_player };
            let line = move |to: V3, life: f32| Mark { follow, ..Mark::tracer(seen_before, to, life) };
            let touched_before = f.p.first_contact.is_some();
            let first = f.p.age == 0.0;
            world.only.set(f.by.sure.map(|t| if t == FormId::PLAYER_REF { PLAYER } else { t.0 as ActorId }));
            let events = f.p.step(&world, dt);
            world.only.set(None);
            if snap_active && !at_once {
                if let Some(s) = snap.as_deref_mut() {
                    let to = events
                        .iter()
                        .find_map(|e| match e {
                            ProjEvent::Hit { point, .. } | ProjEvent::Explode { point, .. } | ProjEvent::Expired { point } => Some(*point),
                            _ => None,
                        })
                        .unwrap_or(f.p.pos);
                    let hit_player = events.iter().any(|e| matches!(e, ProjEvent::Hit { actor: Some(a), .. } if *a == PLAYER));
                    let is_player_owned = f.by.actor == FormId::PLAYER_REF;
                    if hit_player {
                        s.push(crate::bullet_snap::SnapInput::PlayerImpact {
                            id: f.id,
                            at: to,
                            speed: f.p.speed,
                            damage: snap_damage,
                            is_player_owned,
                            eligible: snap_eligible,
                        });
                    } else {
                        s.push(crate::bullet_snap::SnapInput::Segment {
                            id: f.id,
                            from: before,
                            to,
                            dir: f.dir,
                            speed: f.p.speed,
                            damage: snap_damage,
                            is_player_owned,
                            eligible: snap_eligible,
                        });
                    }
                }
            }
            // Its way this frame, from the muzzle on the first, to where it stopped.
            let shot_segment = || {
                let to = events
                    .iter()
                    .find_map(|e| match e {
                        ProjEvent::Hit { point, .. } | ProjEvent::Explode { point, .. } | ProjEvent::Expired { point } => Some(*point),
                        _ => None,
                    })
                    .unwrap_or(f.p.pos);
                let from = if first { f.origin } else { before };
                (from, to)
            };
            if !gas.is_empty() && !f.by.harmless {
                let (from, to) = shot_segment();
                let inside = if first { crate::gas_traps::around(&gas, f.origin) } else { Vec::new() };
                for r in crate::gas_traps::crossed(&gas, from, to) {
                    if !f.gassed.contains(&r) {
                        f.gassed.push(r);
                        gas_hits.push((r, f.by, inside.contains(&r)));
                    }
                }
            }
            // Scripted Projectile Zone primitives (XTRI 23) crossed by this shot segment
            // (#3662): each projectile triggers each volume at most once without stopping
            // the shot.
            if !proj_zones.is_empty() {
                let (from, to) = shot_segment();
                for r in crate::script_trigger::crossed_projectile_zones(&proj_zones, from, to) {
                    if !f.proj_zones_hit.contains(&r) {
                        f.proj_zones_hit.push(r);
                        proj_zone_hits.push((r, f.by, f.rref));
                    }
                }
            }
            // VATS mode's explosive ([`VatsBlast`]): gone off, coming, or flying off.
            if let Some(aim) = f.vats_aim {
                if events.iter().any(|e| matches!(e, ProjEvent::Explode { .. })) {
                    c.vats_blast.went_off += 1;
                } else if f.p.alive {
                    let far = vec::len(vec::sub(f.p.pos, aim));
                    f.nearest = f.nearest.min(far);
                    if far > f.nearest + VATS_FLY_OFF {
                        c.vats_blast.off += 1;
                        if !std::mem::replace(&mut f.told_off, true) {
                            info!("combat: the VATS mode explosive flies off, {far:.0} units from its mark and {:.0} at the nearest", f.nearest);
                        }
                    } else {
                        c.vats_blast.toward += 1;
                    }
                }
            }
            // An explosive's arc, for comparing with the original's (combat-rules.md §5,
            // issue #1471): where it left and how, and where it first touched something.
            if let (false, Some((point, age)), Some((from, v)), Some(_)) = (touched_before, f.p.first_contact, f.p.launch, f.p.explosion) {
                let edid = c.forms.projectiles.get(&f.proj).and_then(|p| p.edid.as_deref()).unwrap_or("?");
                let across = |a: V3, b: V3| (a[0] - b[0]).hypot(a[1] - b[1]);
                info!(
                    "combat: projectile {edid} left ({:.0}, {:.0}, {:.0}) at {:.0} units/s, {:.1} deg up; first touched ({:.0}, {:.0}, {:.0}) after {:.2} s, {:.0} units across the ground and {:.0} below where it left",
                    from[0],
                    from[1],
                    from[2],
                    vec::len(v),
                    v[2].atan2(v[0].hypot(v[1])).to_degrees(),
                    point[0],
                    point[1],
                    point[2],
                    age,
                    across(point, from),
                    from[2] - point[2]
                );
            }
            if let (Some((from, _)), Some(ProjEvent::Explode { point, .. })) = (f.p.launch, events.iter().find(|e| matches!(e, ProjEvent::Explode { .. }))) {
                let edid = c.forms.projectiles.get(&f.proj).and_then(|p| p.edid.as_deref()).unwrap_or("?");
                info!(
                    "combat: projectile {edid} went off after {:.2} s at ({:.0}, {:.0}, {:.0}), {:.0} units across the ground from where it left{}",
                    f.p.age,
                    point[0],
                    point[1],
                    point[2],
                    (point[0] - from[0]).hypot(point[1] - from[1]),
                    if f.p.rolling { ", after rolling" } else { "" }
                );
            }
            let detonated_now = at_once && f.p.explosion.is_some() && !f.by.harmless && events.iter().any(|event| matches!(event, ProjEvent::Explode { .. }));
            for e in events {
                match e {
                    ProjEvent::Hit { actor, part, object, point, delay, normal, material } => {
                        let from = if f.p.hitscan { f.origin } else { before };
                        let d = vec::sub(point, from);
                        let dir = if vec::len(d) > 1e-3 { vec::scale(d, 1.0 / vec::len(d)) } else { [0.0; 3] };
                        let since = c.time - f.p.age;
                        c.pending.push(Pending { due: c.time + delay, actor, part, object, point, dir, normal, material, projectiles: f.projectiles_per_shot, force: f.force, by: f.by, rref: f.rref.take(), since, speed_mult: f.speed_mult });
                        if let Some(weapon) = lands {
                            let struck = actor.map(|a| if a == PLAYER { FormId::PLAYER_REF } else { FormId(a as u32) });
                            c.landed.push(LandedThrow { weapon, proj: f.proj, point, normal: Some(normal), dir: f.dir, struck, by: f.by.actor });
                        }
                        if f.tracer {
                            debug!(
                                "combat: tracer from ({:.1}, {:.1}, {:.1}) to the hit at ({:.1}, {:.1}, {:.1})",
                                seen_before[0], seen_before[1], seen_before[2], point[0], point[1], point[2]
                            );
                            c.marks.push(line(point, hold.unwrap_or(0.08)));
                        }
                        if at_once && f.shown {
                            c.ghosts.push(ghost(point, actor == Some(PLAYER)));
                        }
                    }
                    // A harmless shot sets nothing off [design].
                    ProjEvent::Explode { explosion, point, normal } if !f.by.harmless => {
                        c.boom.push(QueuedBlast { id: explosion, at: point, cause: Some(Cause::of(&f.by)), normal, source: None });
                    }
                    ProjEvent::Explode { .. } => {}
                    ProjEvent::Settled { point, .. } => c.settled.push(SettledMine { proj: f.proj, point, dir: f.dir, actor: f.by.actor }),
                    ProjEvent::Splash { point, dir } => {
                        let delay = if at_once && !beam && speed > 0.0 { vec::len(vec::sub(point, f.origin)) / speed } else { 0.0 };
                        let water_impact = timed_water_impact(water_entry_kind(f.p.motion, f.p.explosion.is_some()), delay, detonated_now);
                        c.splashes.push(Splash { due: c.time + delay, point, dir, weapon: f.by.weapon, shooter: f.by.actor, water_impact });
                    }
                    // A tracer that meets nothing is drawn to where it ends.
                    ProjEvent::Expired { point } => {
                        f.ended_at = Some(point);
                        if let Some(weapon) = lands {
                            c.landed.push(LandedThrow { weapon, proj: f.proj, point, normal: None, dir: f.dir, struck: None, by: f.by.actor });
                        }
                        if f.tracer {
                            c.marks.push(line(point, hold.unwrap_or(0.08)));
                        }
                        if at_once && f.shown {
                            c.ghosts.push(ghost(point, false));
                        }
                    }
                }
            }
            if f.tracer && f.p.alive {
                c.marks.push(Mark::tracer(seen_before, f.seen.at(f.p.pos, f.p.travelled), hold.unwrap_or(0.05)));
            }
            if vec::len(f.p.vel) > 1.0 {
                f.dir = vec::normalize(f.p.vel);
            }
        }
        // A shot found at once (hitscan) flies on for scripts as long as the bullet
        // would take to cover its way [observed: proj1, the 10mm shot aimed up still
        // there the frame after; hypothesis: the time at its speed].
        over.extend(c.flying.iter().filter(|f| !f.p.alive).filter_map(|f| {
            let left = match (f.p.hitscan && f.p.speed > 0.0, f.ended_at) {
                (true, Some(end)) => (vec::len(vec::sub(end, f.origin)) / f.p.speed - f.p.age).max(0.0),
                _ => 0.0,
            };
            f.rref.map(|r| (r, left))
        }));
        c.flying.retain(|f| f.p.alive);
    }
    for (r, by, inside) in gas_hits {
        let what = if inside { "a shot fired inside it" } else { "a shot through it" };
        gas_hit(c, &mut sim, script.as_deref_mut(), r, Some(by.actor), Some(by.weapon), inside, what);
    }
    for (r, by, rref) in proj_zone_hits {
        projectile_zone_hit(&mut sim, script.as_deref_mut(), r, by, rref);
    }
    if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()).filter(|_| !over.is_empty()) {
        with_world(rt, &mut sim, |rt| {
            for (r, left) in over {
                debug!("combat: shot reference {r} ends, {left:.3} s left in flight");
                if left > 0.0 {
                    rt.game.projectile_flies_on(r, left);
                } else {
                    rt.game.projectile_gone(r);
                }
            }
        });
    }
    // What is drawn this frame: the projectiles in flight where they are seen, and
    // those found at once on their way.
    let snap_active = snap.as_deref().is_some_and(crate::bullet_snap::BulletSnap::active);
    c.ghosts.retain_mut(|g| {
        if let (true, Some(m)) = (g.follow, anim.muzzle) {
            g.from = m;
        }
        if std::mem::take(&mut g.fresh) {
            return true;
        }
        if g.beam {
            g.left -= dt;
            g.left > 0.0
        } else {
            let prev = g.travelled;
            let len = vec::len(vec::sub(g.to, g.from));
            g.travelled += g.speed.max(1.0) * dt;
            let finished = g.travelled >= len;
            if snap_active {
                if let Some(s) = snap.as_deref_mut() {
                    let d = vec::sub(g.to, g.from);
                    let dir = if len > 1e-3 { vec::scale(d, 1.0 / len) } else { [0.0, 1.0, 0.0] };
                    let is_player_owned = g.owner == FormId::PLAYER_REF;
                    if g.hit_player {
                        if finished {
                            s.push(crate::bullet_snap::SnapInput::PlayerImpact {
                                id: g.id,
                                at: g.to,
                                speed: g.speed,
                                damage: g.damage,
                                is_player_owned,
                                eligible: g.snap_eligible,
                            });
                        }
                    } else {
                        let seg_from = vec::add(g.from, vec::scale(dir, prev));
                        let seg_to = vec::add(g.from, vec::scale(dir, g.travelled.min(len)));
                        s.push(crate::bullet_snap::SnapInput::Segment {
                            id: g.id,
                            from: seg_from,
                            to: seg_to,
                            dir,
                            speed: g.speed,
                            damage: g.damage,
                            is_player_owned,
                            eligible: g.snap_eligible,
                        });
                    }
                }
            }
            !finished
        }
    });
    if let Some(s) = snap.as_deref_mut().filter(|s| s.active()) {
        s.retain_active(|id| c.flying.iter().any(|f| f.id == id) || c.ghosts.iter().any(|g| g.id == id));
    }
    visuals.flying.clear();
    visuals.flying.extend(c.flying.iter().filter(|f| f.shown).map(|f| ShotVisual { id: f.id, projectile: f.proj, owner: f.by.actor, pos: f.seen.at(f.p.pos, f.p.travelled), dir: f.dir, beam: None, travelled: f.p.travelled, strength: 1.0 }));
    visuals.flying.extend(c.ghosts.iter().map(|g| {
        let d = vec::sub(g.to, g.from);
        let len = vec::len(d);
        let dir = if len > 1e-3 { vec::scale(d, 1.0 / len) } else { [0.0, 1.0, 0.0] };
        ShotVisual { id: g.id, projectile: g.proj, owner: g.owner, pos: vec::add(g.from, vec::scale(dir, g.travelled)), dir, beam: g.beam.then_some(len), travelled: g.travelled, strength: beam_strength(g.beam, g.left, g.fade) }
    }));
    // Record due crossings before this frame's detonations: a lobber can cross
    // the surface and strike the bed on one step. Keep the authored IPCT and its
    // sounds in their existing delivery order below.
    let (splashes, keep): (Vec<Splash>, Vec<Splash>) = std::mem::take(&mut c.splashes).into_iter().partition(|w| w.due <= c.time);
    c.splashes = keep;
    for w in &splashes {
        let Some(kind) = w.water_impact else { continue };
        // A hitscan crossing can be due after the water it was aimed at streamed out.
        if let Some((surface, receiver)) = phys.world.water_surface_with_receiver(w.point)
            .filter(|&(surface, _)| (surface - w.point[2]).abs() < 1.0)
        {
            let point = [w.point[0], w.point[1], surface];
            visuals.water_entries.push(ShotWaterEntry { point, direction: w.dir, receiver, kind });
        }
    }
    // Explosions, with those they set off, a bounded number per frame.
    let mut n = 0;
    while let Some(blast_req) = c.boom.pop() {
        let QueuedBlast { id, at, cause, normal, source } = blast_req;
        n += 1;
        if n > MAX_EXPLOSIONS_PER_FRAME {
            warn!("combat: more than {MAX_EXPLOSIONS_PER_FRAME} explosions in one frame; {} left for the next", c.boom.len() + 1);
            c.boom.push(blast_req);
            break;
        }
        stats.explosions += 1;
        let Some(mut x) = c.forms.explosions.get(&id).cloned() else { continue };
        let ground = phys.ground_below(at, EXPLOSION_GROUND_UP, EXPLOSION_GROUND_REACH);
        // Splash Damage's Adjust Explosion Radius, for the cause's blasts (the player's
        // and its teammates'; perk-entry-points.md §8).
        if let Some(cause) = cause {
            let (r, by) = owner_weapon_perk(script.as_deref_mut(), &mut sim, cause.actor, fnv_game::perks::ADJUST_EXPLOSION_RADIUS, cause.weapon, x.radius);
            if !by.is_empty() && (r - x.radius).abs() > 1e-3 {
                info!("combat: perks change {}'s radius {:.0} -> {:.0} ({})", x.edid.as_deref().unwrap_or("?"), x.radius, r.max(0.0), by.join(", "));
                x.radius = r.max(0.0);
            }
        }
        // Water near enough to throw up, when no ground lies between the blast and it:
        // the water shows instead of the blast's own model (no fireball in the
        // original, `WATER_SURFACE_EXPLOSION`); its damage and push are as on land.
        let reach = c.forms.setting("fExplosionSplashRadius").filter(|r| *r > 0.0).unwrap_or(WATER_BLAST_REACH);
        let water = phys.world.water_surface_with_receiver(at).filter(|&(surface, _)| {
            water_thrown_up(at, Some(surface), ground.as_ref().map(|g| g.point[2]), reach).is_some()
        });
        let look = if water.is_some() { ExplosionLook::Hidden } else { ExplosionLook::Own };
        effects.2.write(ExplosionFx { explosion: id, point: at, cause: cause.map(|c| c.actor), normal: ground.as_ref().map(|g| g.normal), look, gore: false });
        if let Some((surface, receiver)) = water {
            let point = [at[0], at[1], surface];
            let blast = SurfaceBlast { radius: x.radius, depth: surface - at[2], reach };
            visuals.water_entries.push(ShotWaterEntry { point, direction: [0.0; 3], receiver, kind: WaterImpact::Blast(blast) });
            stats.water_blasts += 1;
            info!("combat: explosion {} throws up water at ({:.0}, {:.0}, {:.0}), {:.0} units from the surface", x.edid.as_deref().unwrap_or("?"), point[0], point[1], point[2], (at[2] - surface).abs());
            // No one's: it is not heard as a second blast (detection hears the blast's).
            effects.2.write(ExplosionFx { explosion: WATER_EXPLOSION, point, cause: None, normal: None, look: ExplosionLook::WaterSurface, gore: false });
            if let Some(splash) = sfx.as_deref().and_then(|s| s.forms.explosions.get(&WATER_EXPLOSION)) {
                for sound in splash.sound_1.into_iter().chain(splash.sound_2) {
                    effects.0.write(PlaySound { sound, at: Some(point) });
                }
            }
        }
        // A blast whose object effect burns sets the grass under it alight
        // (`crate::grass_fire`); not one that threw up water.
        if let (Some(item), Some(g), None) = (x.enchantment, ground.as_ref(), water) {
            c.fire_landed.push(crate::grass_fire::Landed { point: g.point, item, by: cause.map(|k| k.actor) });
        }
        if let Some(e) = sfx.as_deref().and_then(|s| s.forms.explosions.get(&id)) {
            for sound in e.sound_1.into_iter().chain(e.sound_2) {
                effects.0.write(PlaySound { sound, at: Some(at) });
            }
            // The impact of the EXPL's data set for the ground under it, when the ground
            // is near: the dirt it throws up (`ImpactExplosionDirt.nif`) [hypothesis:
            // that the game shows the EXPL's IPDS on what lies under the blast, as a
            // shot's on what it hits; the reach is a design choice].
            // Not on a bed under the water it threw up.
            let impact = e.impact_dataset.zip(ground.as_ref().filter(|_| water.is_none())).and_then(|(set, g)| sfx.as_deref()?.forms.impact_id(set, fnv_combat::fx::impact_material(g.material)));
            if let (Some(impact), Some(g)) = (impact, ground.filter(|_| water.is_none())) {
                // The dirt is the cause's (no one's without one): detection hears it as theirs.
                effects.1.write(ImpactFx { impact, shooter: cause.map_or(FormId(0), |c| c.actor), point: g.point, normal: g.normal, dir: [0.0, 0.0, -1.0], actor: None, no_spray: false, no_decal: false, splatter: false });
                if let Some(i) = sfx.as_deref().and_then(|s| s.forms.impacts.get(&impact)) {
                    for sound in i.sound_1.into_iter().chain(i.sound_2) {
                        effects.0.write(PlaySound { sound, at: Some(g.point) });
                    }
                }
            }
        }
        // The blast's live reference for the scripts (ShowOff's GetExplosionRefRadius and
        // GetExplosionRefSource, and JIP's GetHitProjectile), gone after the frame
        // (`fnv_game::nvse_showoff5`).
        let source_actor = cause.map(|c| c.actor).or(source);
        let expl_ref = script
            .as_deref_mut()
            .and_then(|s| s.rt.as_mut())
            .and_then(|rt| crate::script::with_world(rt, &mut sim, |rt| rt.game.explosion_placed(id, source_actor, at, x.radius)));
        let hits = {
            let world = view(&mut phys, script.as_deref(), Some(&sim));
            blast(&world, &c.tuning, &x, at, normal)
        };
        let always = x.flags.iter().any(|f| f == "Knock Down - Always");
        let by_formula = x.flags.iter().any(|f| f == "Knock Down - By Formula");
        let player_blast = cause.is_some_and(|c| c.actor == FormId::PLAYER_REF);
        for b in hits {
            let target = if b.actor == PLAYER { FormId::PLAYER_REF } else { FormId(b.actor as u32) };
            // The player's tweak: the player's own blast spares the player (and optionally non-hostiles) [design: docs/design/tweaks.md].
            if player_blast && script.as_deref_mut().and_then(|s| s.rt.as_mut()).is_some_and(|rt| crate::script::with_world(rt, &mut sim, |rt| rt.game.own_explosion_spares(target))) {
                continue;
            }
            // The player's tweak: a Meltdown blast spares the player and the companions [design: docs/design/tweaks.md].
            if c.meltdowns.contains(&id) && script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.no_self_meltdown && (target == FormId::PLAYER_REF || rt.game.is_teammate(target))) {
                continue;
            }
            let (dt_v, dr) = target_armor(c, script.as_deref_mut(), &mut sim, target);
            // ShowOff:OnExplosionHit's handlers may change the blast's damage first.
            let raw = script.as_deref_mut().and_then(|s| s.rt.as_mut()).map_or(b.damage, |rt| {
                rt.game.explosion_hit_with_context(expl_ref, target, source_actor, f64::from(b.damage)) as f32
            });
            let (ap, tp) = blast_perks(script.as_deref_mut(), &mut sim, cause, target);
            let d = damage::blast_hit(&c.rules, &c.tuning, raw, dt_v, dr, &ap, &tp);
            log_blast_perks(cause, target, &ap, &tp, &d);
            let dealt = d.dealt;
            // Wave 4D w4d_13_explosion_falloff_outdoor [observed]: GetHitBaseWeaponDamage
            // includes explosion distance falloff (before difficulty scaling).
            let info = blast_hit_info(cause, expl_ref, f64::from(raw));
            // The blast's object effect on those it catches (hit-effects.md §1).
            let items: Vec<FormId> = x.enchantment.into_iter().collect();
            // Its limb damage, on the torso and legs (crippled-limbs.md §5): from the damage
            // after armour; `Game::blast_limbs` applies the difficulty, as `hit` does to Health.
            let limbs = blast_limb_hits(c, &parts, target, dealt, cause.and_then(|k| c.forms.weapons.get(&k.weapon)));
            let (r, hit) = hit_actor(script.as_deref_mut(), &mut sim, target, dealt, info, &mut stats, &mut feedback, hud.as_deref_mut(), &items, false, None, &limbs).unzip();
            let mut hit = hit.flatten();
            if r.as_ref().is_none_or(|r| !r.immune) {
                wear_armor(c, &mut sim, &mut stats, target, Zone::All, b.damage);
            }
            // A blast that kills (or hits a corpse) may dismember the part facing it, by
            // its weapon's On Hit (dismemberment.md §4, §7): most grenades and every mine are
            // No Dismember/Explode; launchers, the Fat Man and dynamite are not. A blast with
            // no source weapon (a placed explosion, a mine without an actor owner) rolls none.
            if let Some(rr) = r.as_ref().filter(|r| r.died || r.corpse) {
                let w = cause.and_then(|k| c.forms.weapons.get(&k.weapon)).cloned();
                let (facing, check) = match w.as_ref() {
                    Some(_) => blast_gore(&view(&mut phys, script.as_deref(), Some(&sim)), target, b.actor, at, x.radius, w.as_ref(), rr, info.attacker, b.push),
                    None => (None, None),
                };
                // A corpse caught by a blast says whether it reached the roll (#2346).
                if rr.corpse {
                    let why = match (&w, facing, &check) {
                        (_, Some((part, _)), Some(_)) => format!("part {part} checked for dismemberment"),
                        (None, ..) => "no dismemberment roll (no source weapon)".to_string(),
                        (_, None, _) => "no dismemberment roll (no body part faces the blast)".to_string(),
                        (_, Some((part, _)), None) => format!("no dismemberment roll (part {part} cannot go)"),
                    };
                    info!("combat: blast {} on corpse {target}: {why}", x.edid.as_deref().unwrap_or("?"));
                }
                if let Some(check) = check {
                    dm.checks.push(check);
                }
            }
            // The player's tweak: a blast kill by someone with Bloody Mess may blow the torso
            // apart [design: docs/design/tweaks.md].
            let splash_gib = r.as_ref().is_some_and(|r| r.died) && target != FormId::PLAYER_REF && script.as_deref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.splash_torso_gib);
            if let Some(by) = info.attacker.filter(|_| splash_gib) {
                let bloody = script.as_deref_mut().and_then(|s| s.rt.as_mut()).is_some_and(|rt| with_world(rt, &mut sim, |rt| rt.game.has_bloody_mess(by)));
                if bloody && c.rng.next_f32() < SPLASH_TORSO_CHANCE {
                    if let Some(check) = crate::dismember::scripted_check(c, &parts, &sim, target, TORSO_LOCATION, Some(by), Some(true)) {
                        info!("combat: blast kill on {target}: the torso blows apart (Bloody Mess, tweak)");
                        dm.checks.push(check);
                    }
                }
            }
            if let Some(r) = r {
                // Knockdown [documented: the EXPL flags; Explosion Knockdown Formula].
                // The player's tweak avoids it by Strength instead of Agility [design:
                // docs/design/tweaks.md].
                let avoid_by = if script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.knockdown_by_strength) { fnv_game::actor_values::STRENGTH } else { fnv_game::actor_values::AGILITY };
                let agility = actor_av(script.as_deref_mut(), &mut sim, target, avoid_by).unwrap_or(5.0);
                let chance = if r.died || r.immune {
                    0.0
                } else if always {
                    1.0
                } else if by_formula {
                    damage::knockdown_chance(&c.rules, r.dealt as f32, agility, (r.health + r.dealt) as f32, r.max_health as f32)
                } else {
                    0.0
                };
                let rolled = chance > 0.0 && c.rng.next_f32() < chance;
                // The rules lay it down (unconscious.md §5); refused for actors flagged
                // No Knockdowns and for the player seated or talking.
                let shove = [b.push[0] * BLAST_KNOCKDOWN_SPEED, b.push[1] * BLAST_KNOCKDOWN_SPEED, b.push[2].max(0.0) * BLAST_KNOCKDOWN_SPEED + BLAST_KNOCKDOWN_SPEED * fnv_game::unconscious::PUSH_LIFT];
                let down = rolled && script.as_deref_mut().and_then(|s| s.rt.as_mut()).is_some_and(|rt| rt.game.knock_down(target, info.attacker, shove));
                if down {
                    stats.knockdowns += 1;
                    // Laid down, it does not also stagger from the limbs the blast crippled.
                    if let Some(h) = hit.as_mut() {
                        h.crippled = false;
                    }
                    if target == FormId::PLAYER_REF {
                        if let Some(h) = feedback.player_hits.last_mut() {
                            h.crippled = false;
                        }
                    }
                }
                info!(
                    "combat: blast {} on {target} at {:.0}: {:.1} health damage, health {:.0}/{:.0}{}",
                    x.edid.as_deref().unwrap_or("?"),
                    b.distance,
                    r.dealt,
                    r.health,
                    r.max_health,
                    if down { ", knocked down" } else if rolled { ", knockdown refused (No Knockdowns, or the player seated or talking)" } else { "" }
                );
            }
            if let Some(h) = hit {
                messages.write(h);
            }
        }
        // Not one gone from the world this frame: a mine that went off is disabled
        // before its explosion, so its blast does not set off its destruction stage too.
        for (r, d) in destructibles_near(&mut phys, &sim, script.as_deref(), at, x.radius).into_iter().filter(|(r, _)| sim.present(r.0)).collect::<Vec<_>>() {
            let dmg = damage::explosion_damage(&c.tuning, x.damage, x.radius, d);
            damage_destructible(c, &mut sim, script.as_deref_mut(), r, dmg, cause);
        }
        // The gas clouds it reaches go off (`crate::gas_traps`, #2687): a gas cloud's own
        // explosion sets off the clouds beside it this way.
        let gas = script.as_deref_mut().and_then(|s| s.rt.as_mut()).map(|rt| with_world(rt, &mut sim, |rt| crate::gas_traps::in_blast(&crate::gas_traps::volumes(rt), at, x.radius))).unwrap_or_default();
        for r in gas {
            gas_hit(c, &mut sim, script.as_deref_mut(), r, cause.map(|c| c.actor), cause.map(|c| c.weapon), true, "a blast");
        }
        // The player's tweak: things that can be taken stay where they lie [design: docs/design/tweaks.md].
        let keep_items = script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.blasts_spare_items);
        let pushed = if keep_items {
            let takeable = |form: u32| sim.base_of(form).is_some_and(|b| sim.forms.item(b).is_some());
            phys.world.push_dynamic_but(at, x.radius, x.force, &takeable)
        } else {
            phys.world.push_dynamic(at, x.radius, x.force)
        };
        c.marks.push(Mark::at(at, 0.6, MarkKind::Impact));
        info!(
            "combat: explosion {} at ({:.0}, {:.0}, {:.0}), {pushed} loose object(s) pushed{}",
            x.edid.as_deref().unwrap_or("?"),
            at[0],
            at[1],
            at[2],
            if x.placed_object.is_some() { " (placed object not built yet)" } else { "" }
        );
        // The radiation it leaves (docs/spec-notes/radiation.md §2.1).
        if x.radiation_level > 0.0 {
            let zone = script.as_deref_mut().and_then(|s| s.rt.as_mut()).and_then(|rt| with_world(rt, &mut sim, |rt| rt.game.add_radiation_zone(id, at, x.radiation_level, x.radiation_radius, x.radius, x.radiation_dissipation_time)));
            if let Some(z) = zone {
                info!(
                    "radiation: explosion {} leaves a radiation zone (#{}): {:.1} rads/s within {:.0} units, fading over {:.0} s",
                    x.edid.as_deref().unwrap_or("?"),
                    z.serial,
                    z.level,
                    z.radius,
                    z.duration
                );
            }
        }
    }
    // Shots that went down into water: the water impact of the weapon's impact data set
    // on the surface, facing up (no decal on water).
    if let Some(s) = sfx.as_deref() {
        for w in splashes {
            let impact = c.forms.weapons.get(&w.weapon).and_then(|x| x.impact_dataset).and_then(|set| s.forms.impact_id(set, "water"));
            if let Some(impact) = impact {
                stats.water_splashes += 1;
                debug!("combat: splash {impact} at ({:.0}, {:.0}, {:.0})", w.point[0], w.point[1], w.point[2]);
                show_impact(s, script.as_deref(), &sim, ImpactFx { impact, shooter: w.shooter, point: w.point, normal: [0.0, 0.0, 1.0], dir: w.dir, actor: None, no_spray: false, no_decal: true, splatter: false }, &mut effects.0, &mut effects.1);
            }
        }
    }
    let now = c.time;
    let due: Vec<Pending> = {
        let (d, keep): (Vec<Pending>, Vec<Pending>) = std::mem::take(&mut c.pending).into_iter().partition(|p| p.due <= now);
        c.pending = keep;
        d
    };
    for mut p in due {
        // What scripts set on the shot (`fnv_game::nvse_proj`): its base damage and its
        // source, who the hit counts as [observed: proj1, set values held to the hit].
        let struck = match (p.actor, p.object) {
            (Some(a), _) if a == PLAYER => FormId::PLAYER_REF,
            (Some(a), _) => FormId(a as u32),
            (None, Some(o)) => FormId(o as u32),
            (None, None) => FormId(0),
        };
        let shot = p.rref.and_then(|r| script.as_deref_mut().and_then(|s| s.rt.as_mut()).and_then(|rt| with_world(rt, &mut sim, |rt| rt.game.projectile_impact(r, struck, p.material))));
        if let Some(s) = &shot {
            p.by.actor = s.source;
        }
        let id = p.by.weapon;
        // Modded weapons hit with their mods' figures (weapon-mods.md §2).
        // [design] NPCs carrying an already-modded weapon (e.g. player-modded) apply its
        // weapon-mod stats (`WeaponStats::modded`) even when `npc-weapon-mods` is off;
        // unmodded NPC weapons (`mods == 0`) stay identical to base weapon stats.
        let w = if p.by.actor == FormId::PLAYER_REF {
            c.player_stats(id)
        } else {
            let mods = sim.world.peek(p.by.actor).and_then(|st| st.inventory.as_ref()).map_or(0, |i| i.mods_in_use(id));
            c.forms.weapons.get(&id).map(|w| if mods != 0 { w.modded(mods) } else { w.clone() })
        };
        let Some(mut w) = w else { continue };
        script_weapon_critical_damage(script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game), id, &mut w);
        if let Some(s) = &shot {
            w.base_damage = s.damage.round() as i32;
        }
        let chain = c.forms.chain(id, p.by.ammo);
        let effect_ids = chain.map(|ch| ch.ammo_effects).unwrap_or_default();
        let shooter = p.by.actor;
        c.marks.push(Mark::at(p.point, 0.3, MarkKind::Impact));
        // A shot that lands on the ground (not on an actor) with a weapon whose object
        // effect burns sets the grass there alight (`crate::grass_fire`).
        if let Some(item) = w.enchantment.filter(|_| p.actor.is_none() && !p.by.harmless) {
            c.fire_landed.push(crate::grass_fire::Landed { point: p.point, item, by: (shooter != FormId(0)).then_some(shooter) });
        }
        let bp = p.actor.and_then(|a| body_part(c, &parts, a, p.part)).or_else(|| {
            (p.actor == Some(PLAYER)).then(|| player_body_part(c, &phys, player.as_deref(), p.point)).flatten()
        });
        // The reference struck: the player's, not the shots' sentinel for it.
        let struck = p.actor.map(actor_ref);
        // The impact the weapon's data set names for what was hit: an actor's is its
        // base's Impact Material Type (NPC_ / CREA `NAM4`), organic without one
        // [design]; anything else's its collision material (docs/spec-notes/audio.md §11).
        // The player's tweak: a shot whose damage does not get through the actor's DT
        // leaves no blood on it [design: docs/design/tweaks.md].
        let wanted = script.as_deref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.armor_blocks_blood);
        let stopped = wanted && struck.is_some_and(|r| blood_stopped(w.base_damage as f32, target_armor_for_zone(c, script.as_deref_mut(), &mut sim, FormId(r), Some(Zone::of(bp.as_ref()))).0));
        // The player's tweak: a shot on the weapon in an actor's hand strikes metal, sparks
        // and no blood [design: docs/design/tweaks.md].
        let on_weapon = p.part == Some(crate::hitvol::WEAPON_PART) && script.as_deref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.weapon_impacts);
        if let Some(s) = sfx.as_deref() {
            let (impact, material) = if on_weapon {
                let set = s.forms.weapons.get(&id).and_then(|w| w.impact_dataset);
                (set.and_then(|set| s.forms.impact_id(set, "metal")), "metal")
            } else {
                impact_for(s, script.as_deref(), &sim, id, p.actor, p.material)
            };
            if let Some(impact) = impact {
                show_impact(s, script.as_deref(), &sim, ImpactFx { impact, shooter, point: p.point, normal: p.normal, dir: p.dir, actor: struck, no_spray: on_weapon, no_decal: stopped || on_weapon, splatter: false }, &mut effects.0, &mut effects.1);
            }
            // Blood on the wall or floor behind or below an organic actor ([`world_blood`]),
            // whatever the weapon's own impact: the struck part's blood impact (a hit on
            // the whole volume of an actor with published parts reads the first part
            // [design]); none for a harmless shot, nor where the actor's own blood decal
            // is suppressed (No Blood Decal, the armour tweak).
            let no_decal = stopped || struck.is_some_and(|r| blood_flags(script.as_deref(), &sim, r).1);
            let body = bp.clone().or_else(|| p.actor.and_then(|a| body_part(c, &parts, a, Some(0))));
            if let Some(blood) = splatter_impact(&s.forms, p.part, body.as_ref(), material).filter(|_| p.actor.is_some() && !p.by.harmless && !no_decal) {
                let setting = sim.world.setting_f32("fCombatEnvironmentBloodChance");
                let chance = blood_chance(setting);
                if !c.blood_chance_logged {
                    c.blood_chance_logged = true;
                    info!("combat: blood on the world behind shot actors: chance {chance:.2} (fCombatEnvironmentBloodChance {}), within {WORLD_BLOOD_REACH:.0} units along the shot, else below", setting.map_or("not set".to_string(), |v| format!("{v}")));
                }
                let roll = c.blood_rng.next_f32();
                stats.blood_rolls += 1;
                stats.blood_won += u32::from(roll < chance);

                let hit = ImpactFx { impact: blood, shooter, point: p.point, normal: p.normal, dir: p.dir, actor: struck, no_spray: false, no_decal, splatter: false };
                let fx = world_blood(&hit, blood, roll, chance, |from, d, reach| world_surface(&mut phys, from, d, reach));
                let by = stats.blood_by.entry(struck.unwrap_or(0)).or_default();
                *by = (by.0 + 1, by.1 + u32::from(fx.is_some()));
                if let Some(fx) = fx {
                    stats.blood_sent += 1;
                    effects.1.write(fx);
                }
            }
            let hit = p.actor.map(|a| a as u32).or(p.object.map(|o| o as u32));
            effects.4.write(crate::world_sounds::log_impact(s, hit, p.actor.is_some(), p.material, material, impact));
        }
        // A harmless shot is seen and heard where it lands, and the OnHit and OnHitWith
        // blocks of what it hit run (`Game::harmless_hit`); it deals no damage and does
        // nothing more (#2433: G.I. Blues' thugs fall in their OnHit block for Orris's
        // Do No Damage shots).
        if p.by.harmless {
            stats.harmless_hits += 1;
            let hit = p.actor.map(|a| if a == PLAYER { FormId::PLAYER_REF } else { FormId(a as u32) }).or(p.object.map(|o| FormId(o as u32)));
            // (A co-op guest's world is the host's: its hit blocks run there.)
            if let Some(target) = hit.filter(|_| stats.relay.is_none()) {
                let info = HitInfo { attacker: Some(shooter), weapon: Some(id), projectile: p.rref, location: -1, ..HitInfo::default() };
                let ran = script.as_deref_mut().and_then(|s| s.rt.as_mut()).is_some_and(|rt| with_world(rt, &mut sim, |rt| rt.game.harmless_hit(target, info)));
                stats.harmless_scripts += u32::from(ran);
                if ran {
                    info!("combat: a harmless shot by {shooter} hits {target}: its hit blocks ran, no damage");
                } else {
                    info!("combat: a harmless shot by {shooter} hits {target} (part {:?}): no hit block of it ran", p.part);
                }
            } else {
                info!("combat: a harmless shot by {shooter} hits no reference at ({:.0}, {:.0}, {:.0})", p.point[0], p.point[1], p.point[2]);
            }
            continue;
        }
        let ammo_effects: Vec<&AmmoEffect> = effect_ids.iter().filter_map(|e| c.forms.ammo_effects.get(e)).collect();
        let mut a = attacker(script.as_deref_mut(), &mut sim, &c.rules, shooter, &w, p.by.vats);
        // The player's tweak: sneak criticals with melee only [design: docs/design/tweaks.md].
        let melee_only = script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.sneak_criticals_melee_only);
        a.sneaking = !melee_only && p.actor.is_some_and(|t| sneak_attack(script.as_deref(), shooter, p.by.sneaking, FormId(t as u32)));
        let part = bp.as_ref().map_or_else(HitPart::default, BodyPart::hit_part);
        // The player's tweak: sneak criticals on the head only [design: docs/design/tweaks.md].
        if !part.head && script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.sneak_criticals_head_only) {
            a.sneaking = false;
        }
        let whole = shooter != FormId::PLAYER_REF && script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.npc_weapons_whole);
        let h = HitInput { weapon: &w, projectiles: p.projectiles, condition: if whole { 1.0 } else { item_condition(&sim, shooter, id) }, ammo_effects: &ammo_effects, part, power: 1.0 };
        let roll = c.rng.next_f32();
        let Some(actor) = p.actor else {
            stats.world_hits += 1;
            debug!("combat: shot lands at ({:.0}, {:.0}, {:.0}) on {}", p.point[0], p.point[1], p.point[2], p.object.map_or("no object".to_string(), |o| format!("{:08X}", o as u32)));
            if let Some(obj) = p.object {
                let r = FormId(obj as u32);
                // A co-op guest's shot on a placed object goes to the host, whose world
                // runs the object's hit blocks and takes its damage as this player's
                // ([`remote_hits`]; Sunny's bottles count for the host's quest
                // [observed: buckallobill, in game, NVForever co-op]); what that changes
                // comes back as the host's world does. The object is still knocked here,
                // where it is seen.
                if shooter == FormId::PLAYER_REF && !r.is_dynamic() {
                    if let Some(out) = stats.relay.as_mut() {
                        let (ap, _) = hit_perks(script.as_deref_mut(), &mut sim, shooter, id, None);
                        let d = damage::hit(&c.rules, &c.tuning, &h, &a, &Target::default(), &ap, &NoPerks, roll);
                        out.push(RelayedHit { target: r, dealt: d.dealt, weapon: Some(id), location: OBJECT_LOCATION, critical: false, limb: None });
                        let impulse = vec::scale(p.dir, p.force * OBJECT_PUSH * phys.world.params.units_per_metre);
                        phys.world.push_ref(obj as u32, p.point, impulse, OBJECT_PUSH_MAX_SPEED);
                        continue;
                    }
                }
                // The object's OnHit and OnHitWith blocks, before any damage
                // [documented: GECK wiki Event Handling] (combat-rules.md §14).
                let info = HitInfo { attacker: Some(shooter), weapon: Some(id), projectile: p.rref, location: -1, ..HitInfo::default() };
                let ran = script.as_deref_mut().and_then(|s| s.rt.as_mut()).is_some_and(|rt| with_world(rt, &mut sim, |rt| rt.game.hit_object(r, info)));
                // The weapon's object effect reaches the object: its OnMagicEffectHit
                // blocks (the Vault 22 traps catch the flamer's burn this way).
                let items: Vec<FormId> = c.forms.weapons.get(&id).and_then(|w| w.enchantment).into_iter().collect();
                let ran = ran | script.as_deref_mut().and_then(|s| s.rt.as_mut()).is_some_and(|rt| with_world(rt, &mut sim, |rt| rt.game.magic_effect_hits(r, &items, Some(shooter))));
                // A destructible object takes the hit's damage (no armour) (step C7).
                let (ap, _) = hit_perks(script.as_deref_mut(), &mut sim, shooter, id, None);
                let d = damage::hit(&c.rules, &c.tuning, &h, &a, &Target::default(), &ap, &NoPerks, roll);
                damage_destructible(c, &mut sim, script.as_deref_mut(), r, d.dealt, Some(Cause::of(&p.by)));
                // A loose object is knocked away.
                let impulse = vec::scale(p.dir, p.force * OBJECT_PUSH * phys.world.params.units_per_metre);
                let pushed = phys.world.push_ref(obj as u32, p.point, impulse, OBJECT_PUSH_MAX_SPEED);
                stats.object_scripts += ran as u32;
                if ran && shooter == FormId::PLAYER_REF {
                    *stats.player_object_hits.entry(r.0).or_default() += 1;
                }
                stats.objects_pushed += pushed.is_some() as u32;
                if ran || pushed.is_some() {
                    info!(
                        "combat: shot hits {} {r}{}{}",
                        sim.base_of(obj as u32).and_then(|b| sim.name(b).map(str::to_string)).unwrap_or_else(|| "object".into()),
                        if ran { ", its hit blocks ran" } else { "" },
                        pushed.map_or(String::new(), |v| format!(", knocked away at {v:.0} units/s"))
                    );
                }
            }
            continue;
        };
        stats.actor_hits += 1;
        let target = if actor == PLAYER { FormId::PLAYER_REF } else { FormId(actor as u32) };
        // A shot on the weapon the actor holds wears the weapon, not the actor (§17).
        let (ap, tp) = hit_perks(script.as_deref_mut(), &mut sim, shooter, id, Some(target));
        if p.part == Some(WEAPON_PART) {
            let d = damage::hit(&c.rules, &c.tuning, &h, &a, &Target::default(), &ap, &NoPerks, roll);
            strike_weapon(&c.forms, &mut sim, &mut stats, hud.as_deref_mut(), &mut effects.3, script.as_deref_mut(), &parts, target, d.raw, p.point);
            continue;
        }
        // A piece of gore (an exploded part's bits, dismemberment.md §6) is seen hit, no more.
        if p.part == Some(fnv_phys::gore::GORE_PART) {
            continue;
        }
        let (dt_v, dr) = target_armor_for_zone(c, script.as_deref_mut(), &mut sim, target, Some(Zone::of(bp.as_ref())));
        let resist = target_resist(script.as_deref_mut(), &mut sim, target, &w);
        let nerve = companion_nerve(script.as_deref_mut(), &mut sim, target);
        let d = damage::hit(&c.rules, &c.tuning, &h, &a, &Target { dt: dt_v, dr, resist, nerve, ..Target::default() }, &ap, &tp, roll);
        log_hit_perks(shooter, Some(target), &ap, &tp, &d);
        // The ammo's Fatigue Mod (AMEF type 5: the bean bag's 250) takes that much fatigue,
        // once a hit [documented: GECK wiki GetAmmoEffectTraitNumeric effect types;
        // hypothesis: from 0, once a hit; logic-fields.md §3]. Computed before `HitInfo` so
        // `SetOnHitEventHandler` reads it via `GetHitFatigueDamage`.
        let fatigue = damage::apply_effects(0.0, &ammo_effects, "Fatigue Mod").max(0.0);
        // [design] Missing body part data keeps the whole-capsule fallback (location 0).
        let location = bp.as_ref().map_or(0, BodyPart::location);
        let limb = limb_hit(bp.as_ref(), &d, &w);
        let info = HitInfo {
            attacker: Some(shooter),
            weapon: Some(id),
            projectile: p.rref,
            location,
            critical: d.critical,
            sneak_attack: a.sneaking,
            base_weapon_damage: f64::from(d.base + d.crit_bonus),
            fatigue_damage: f64::from(fatigue),
            hit_pos: Some(p.point),
            hit_dir: fnv_game::combat::unit_hit_dir(p.dir),
            limb,
            ..HitInfo::default()
        };
        if target != FormId::PLAYER_REF {
            pushes.write(ActorPush { form_id: target.0, point: p.point, dv: vec::scale(p.dir, SHOT_PUSH) });
        }
        let items = hit_items(c, script.as_deref_mut(), &mut sim, &mut stats, shooter, Some(id), None);
        note_hit_point(script.as_deref_mut(), target, p.point);
        note_armour(script.as_deref_mut(), target, &d);
        let r = apply_hit(script.as_deref_mut(), &mut sim, target, d.dealt, info, &mut stats, &mut feedback, hud.as_deref_mut(), &mut messages, &items, false, crit_effect(c, Some(id)));
        armour_lines(script.as_deref_mut(), Some(shooter), target, &d, c.tuning.dt_floor);
        spread_hit_areas(script.as_deref_mut(), &mut sim, &mut stats, target, &items, info.attacker);
        if fatigue > 0.0 && r.as_ref().is_some_and(|r| !r.immune) {
            if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
                with_world(rt, &mut sim, |rt| rt.game.damage_fatigue(target, f64::from(fatigue)));
                info!("combat: {target} loses {fatigue:.0} fatigue to the ammo");
            }
        }
        // The Knockdown Chance perks on the shooter's hit (And Stay Back, per pellet).
        if r.as_ref().is_some_and(|r| !r.died && !r.immune) && target != FormId::PLAYER_REF {
            perk_knockdown(c, script.as_deref_mut(), &mut sim, shooter, id, target, p.dir);
        }
        if let Some(mut check) = r.as_ref().and_then(|r| dismember_check(target, p.part, &w, r, Some(shooter), p.point, p.dir)) {
            crit_gib(c, &parts, script.as_deref(), d.critical && !a.sneaking, &mut check);
            dm.checks.push(check);
        }
        if r.as_ref().is_none_or(|r| !r.immune) {
            wear_armor(c, &mut sim, &mut stats, target, Zone::of(bp.as_ref()), d.raw);
        }
        match r {
            Some(r) => info!(
                "combat: hit {target} on {} for {:.1} (DT {:.0}{}), health {:.0}/{:.0}{}",
                bp.as_ref().and_then(|b| b.name.as_deref()).unwrap_or("the body"),
                r.dealt,
                d.dt,
                crit_note(&a, &d),
                r.health,
                r.max_health,
                if r.immune { ", takes no damage" } else { "" }
            ),
            None => {
                stats.damage += d.dealt;
                info!("combat: hit {target} for {:.1} (no script runtime: not applied)", d.dealt);
            }
        }
    }
}

fn draw_marks(time: Res<Time>, mut c: ResMut<Combat>, anim: Res<WeaponAnim>, show: Res<HitMarks>, mut gizmos: Gizmos) {
    let dt = time.delta_secs();
    // The test aid keeps its lines where they were drawn, for the record of a check.
    let follow = c.tracer_hold.is_none();
    let draw = show.show || c.tracer_hold.is_some();
    for m in &mut c.marks {
        m.age += dt;
        if let (true, true, Some(muzzle)) = (follow, m.follow, anim.muzzle) {
            m.from = muzzle;
        }
        if !draw {
            continue;
        }
        let fade = (1.0 - m.age / m.life).clamp(0.0, 1.0);
        match m.kind {
            MarkKind::Tracer => {
                gizmos.line(space::to_bevy(m.from), space::to_bevy(m.to), Color::srgba(1.0, 0.9, 0.6, fade));
            }
            MarkKind::Impact | MarkKind::Flash => {
                let p = space::to_bevy(m.to);
                let (s, col) = if m.kind == MarkKind::Flash { (FLASH_SIZE, Color::srgba(1.0, 0.95, 0.7, fade)) } else { (4.0, Color::srgba(1.0, 0.8, 0.3, fade)) };
                gizmos.line(p - Vec3::X * s, p + Vec3::X * s, col);
                gizmos.line(p - Vec3::Y * s, p + Vec3::Y * s, col);
                gizmos.line(p - Vec3::Z * s, p + Vec3::Z * s, col);
            }
        }
    }
    c.marks.retain(|m| m.age < m.life);
}

/// What the HUD shows of the weapon the player has equipped.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HudWeapon {
    /// The equipped weapon's condition, 0 to 1 (§17); `None` with bare hands.
    pub condition: Option<f32>,
    /// Rounds in the magazine and rounds carried besides, for a weapon that uses ammo;
    /// a weapon that spends itself (thrown) has its count first and 0 besides.
    pub ammo: Option<(u32, u32)>,
    /// The ammo type's abbreviation (AMMO `QNAM`: "HP", "JHP", "Slg"), shown left of the
    /// counter; standard rounds have none (hud.md §4).
    pub abbrev: Option<String>,
}

impl Combat {
    /// The condition of the player's armour on the body, 0 to 1 (the lowest of the pieces a
    /// body hit wears), for the Armor Condition tweak's row; `None` with none worn there.
    pub fn hud_armor(&self, sim: &mut SimState) -> Option<f32> {
        let worn: Vec<FormId> = worn_armor(&self.forms, sim, FormId::PLAYER_REF).into_iter().map(|w| w.0).collect();
        self.forms.armor_struck(&worn, fnv_combat::condition::Zone::Body).into_iter().map(|id| item_condition(sim, FormId::PLAYER_REF, id)).min_by(f32::total_cmp)
    }

    /// The equipped weapon as the HUD shows it (docs/spec-notes/hud.md §5).
    pub fn hud_weapon(&self, sim: &SimState) -> HudWeapon {
        let condition = self.equipped.flatten().map(|id| item_condition(sim, FormId::PLAYER_REF, id));
        let ammo = match (self.equipped.flatten(), self.cycle.as_ref()) {
            (Some(_), Some(cy)) if !cy.no_ammo => {
                Some(hud_ammo(cy.magazine, player_count(sim, self.ammo), cy.regen.is_some()))
            }
            (Some(id), Some(_)) if self.self_ammo => Some((player_count(sim, Some(id)), 0)),
            _ => None,
        };
        let abbrev = match self.cycle.as_ref() {
            Some(cy) if ammo.is_some() && !cy.no_ammo => {
                self.ammo.and_then(|a| self.forms.ammo.get(&a)).and_then(|a| a.abbrev.clone()).filter(|a| !a.trim().is_empty())
            }
            _ => None,
        };
        HudWeapon { condition, ammo, abbrev }
    }
}

/// The Combat-tab lines said this frame (`fnv_game::combat_lines`), one log line each:
/// `combat line: <who> says <topic> line <info>`; tries without a line at debug level.
fn log_combat_lines(mut script: Option<ResMut<ScriptState>>, mut stats: ResMut<CombatStats>) {
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    let g = &mut rt.game;
    if g.combat_lines.said.is_empty() {
        return;
    }
    for (topic, who, line) in std::mem::take(&mut g.combat_lines.said) {
        match line {
            Some(l) => {
                stats.combat_lines += 1;
                info!("combat line: {} says {topic} line {:08X}", g.data.name(who), l.0);
            }
            None => {
                stats.combat_lines_none += 1;
                debug!("combat line: {} has no {topic} line", g.data.name(who));
            }
        }
    }
}

fn report(mut exit: MessageReader<AppExit>, stats: Res<CombatStats>) {
    if exit.read().last().is_some() {
        let s = &*stats;
        info!(
            "combat: {} shots ({} drawn from the muzzle, {} from behind cover), {} projectiles, {} actor hits, {} world hits ({} objects' hit blocks, {} objects knocked away), {} explosions ({} knockdowns), {} swings ({} chained, {} power, {} hits), {:.0} health damage, {} kills (+{} XP), {} reloads, {} dry fires, {} jams, {} melee impacts, {} water splashes, {} water blasts",
            s.shots, s.muzzle_shots, s.covered_shots, s.projectiles, s.actor_hits, s.world_hits, s.object_scripts, s.objects_pushed, s.explosions, s.knockdowns, s.swings, s.chained_swings, s.power_attacks, s.melee_hits, s.damage, s.kills, s.xp, s.reloads, s.dry_fires, s.jams, s.melee_impacts, s.water_splashes, s.water_blasts
        );
        if s.spent_rolls > 0 {
            let given: Vec<String> = s.spent_given.iter().map(|(i, n)| format!("{i:08X} x {n}")).collect();
            info!("combat: spent ammo items: {} shots rolled, {} given [{}]", s.spent_rolls, s.spent_given.values().sum::<u32>(), given.join(", "));
        }
        info!("combat lines: {} said, {} tries without a line", s.combat_lines, s.combat_lines_none);
        info!("combat blocks: {} blows blocked, {} blocks broken by power attacks", s.blows_blocked, s.blocks_broken);
        info!(
            "condition summary: the player's weapons lost {:.1} health, {} hits on held weapons, {} weapons broken, armour lost {:.1} health, {} pieces worn to nothing",
            s.weapon_wear, s.weapon_hits, s.weapons_broken, s.armor_wear, s.armor_broken
        );
        info!("{}", hit_effects::summary(&s.hit_effects));
        let passes = GHOST_PASSES.load(std::sync::atomic::Ordering::Relaxed);
        if passes > 0 {
            info!("combat summary: {passes} rays passed through ghosts (SetGhost or Is Ghost)");
        }
        if s.death_drops > 0 {
            info!("combat summary: {} weapons dropped by actors that died holding them", s.death_drops);
        }
        if s.blood_rolls > 0 {
            info!("combat summary: blood on the world: {} shots on actors rolled, {} won the roll, {} met a wall or floor (splatters sent)", s.blood_rolls, s.blood_won, s.blood_sent);
            for (actor, (rolls, sent)) in &s.blood_by {
                info!("combat summary: blood on the world behind {actor:08X}: {sent} splatters from {rolls} rolled hits");
            }
        }
        if s.disarmed > 0 {
            info!("crippled limbs summary: {} actors disarmed by a crippled arm", s.disarmed);
        }
        for (id, (name, hits, max)) in &s.npc_hits {
            let mut d = hits.clone();
            d.sort_by(f32::total_cmp);
            info!(
                "npc hits summary: {name} {id:08X} took {} hits from actors for {:.0} damage (median {:.1}, most {:.1}), maximum health {max:.0}",
                d.len(),
                d.iter().sum::<f32>(),
                d[d.len() / 2],
                d[d.len() - 1]
            );
        }
        for ((actor, weapon), w) in &s.npc_weapon_wear {
            info!(
                "npc weapon wear summary: {actor:08X}'s {weapon:08X} lost {:.2} health in {} attacks ({:.3} an attack), condition {:.1}% -> {:.1}%",
                w.points,
                w.attacks,
                w.points / w.attacks.max(1) as f32,
                w.first * 100.0,
                w.last * 100.0
            );
        }
    }
}

/// Hit effects that ended this frame are logged (`hit_effects`).
fn watch_hit_effects(script: Option<Res<ScriptState>>, mut stats: ResMut<CombatStats>) {
    let Some(rt) = script.as_deref().and_then(|s| s.rt.as_ref()) else { return };
    hit_effects::watch(&rt.game, &mut stats.hit_effects);
    // A death by an effect's drain (a burn, a poison) or a script rather than by a hit.
    if !stats.player_died && rt.game.is_dead(FormId::PLAYER_REF) {
        stats.player_died = true;
        info!("combat: the player died (no hit: an effect's drain or a script)");
    }
}

/// GetAnimAction during a cycle's cooldown, `left` seconds before it is ready again (the
/// cycle already stepped by this frame's `dt`): the clip's time is the time since the
/// trigger at the frame's start, `interval - left - dt`, no earlier than the shot (the
/// cooldown starts at the shot, `fire_delay` in), over a clip of `interval`. A thrown weapon reads as a throw
/// (6 until the clip's `Attach`), the others as a shot.
pub fn cooldown_action(timing: &fnv_combat::attack::Timing, left: f32, thrown: bool, repeating: bool, dt: f32) -> Option<fnv_game::anim_state::AnimAction> {
    use fnv_game::anim_state::{shot_action, throw_action};
    let (fire, interval) = (timing.fire_delay, timing.interval);
    let t = (interval - left - dt).max(fire);
    if thrown {
        throw_action(t, fire, timing.attach, interval, dt)
    } else {
        shot_action(t, fire, interval, dt, repeating)
    }
}

/// The cycle's clip timing from the attack clip (length, `Hit` or `Fire` key, and
/// whether a `Loop` key follows that key), the reload clip (length and `Hit` key) and the
/// length of the reload's opening clip, as the player body reads them; `next_attack`: the
/// attack clip's `a:` key, its time and text (`fnv_actor::player_clips::ClipKeys`);
/// `attach`: the attack clip's `Attach` key (a thrown weapon's).
/// A casing of weapon `id` from its `ShellCasingNode` (the place and way the body
/// gives) at the game's eject speed (`fGunShellEjectSpeed`), for a weapon with a casing
/// model.
fn throw_casing(casings: &mut MessageWriter<CasingFx>, anim: &WeaponAnim, fx: Option<&fnv_combat::fx::WeaponFx>, id: FormId, rules: &Rules) {
    if let (Some((point, dir)), true) = (anim.casing, fx.is_some_and(|f| f.shell_casing_model.is_some())) {
        casings.write(CasingFx { weapon: id, point, velocity: vec::scale(dir, rules.shell_eject_speed), life: rules.shell_lifetime });
    }
}

pub fn clip_timing(attack: Option<(f32, Option<f32>, bool)>, reload: Option<(f32, Option<f32>)>, reload_open: Option<f32>, next_attack: Option<(f32, &str)>, attach: Option<f32>) -> ClipTiming {
    ClipTiming {
        attach_time: attach,
        next_attack: next_attack.and_then(|(t, v)| fnv_combat::melee::NextAttack::parse(v).map(|n| (t, n))),
        attack_len: attack.map(|a| a.0).filter(|l| *l > 0.0),
        hit_time: attack.and_then(|a| a.1),
        reload_len: reload.map(|r| r.0).filter(|l| *l > 0.0),
        reload_hit: reload.and_then(|r| r.1),
        reload_open_len: reload_open,
        spin_up: attack.filter(|a| a.2).and_then(|a| a.1),
        jam_len: None,
    }
}

/// A SOUN's editor ID for a log line (its form ID without one).
fn sound_edid(sfx: Option<&Sfx>, sound: FormId) -> String {
    sfx.and_then(|x| x.forms.sounds.get(&sound)).and_then(|s| s.edid.clone()).unwrap_or_else(|| sound.to_string())
}

#[cfg(test)]
mod tests {
    /// The original's frag throws by game loops held (issue #1785): 5 / 10 / 30 / 60 loops
    /// at 0.52 / 0.60 / 0.78 / 0.99 of the PROJ speed; ours over the frag clip's 0.867 s
    /// wind-up comes within 0.03 of each.
    #[test]
    fn a_grenade_flies_faster_the_longer_attack_is_held() {
        let wind_up = 0.867;
        for (loops, seen) in [(5.0, 0.52), (10.0, 0.60), (30.0, 0.78)] {
            let share = super::throw_hold_share(loops / 60.0, wind_up, false);
            assert!((share - seen).abs() < 0.03, "{loops} loops: {share} against {seen}");
        }
        // Held to the throw (60 or 120 loops): the full speed; a tap: half.
        assert_eq!(super::throw_hold_share(0.867, wind_up, true), 1.0);
        assert_eq!(super::throw_hold_share(2.0, wind_up, false), 1.0);
        assert_eq!(super::throw_hold_share(0.0, wind_up, false), 0.5);
        // A throw with no wind-up keeps its speed.
        assert_eq!(super::throw_hold_share(0.1, 0.0, false), 1.0);
    }

    #[test]
    fn armour_that_stops_a_shot_stops_its_blood() {
        assert!(super::blood_stopped(10.0, 12.0));
        assert!(super::blood_stopped(12.0, 12.0));
        assert!(!super::blood_stopped(13.0, 12.0));
        assert!(!super::blood_stopped(0.0, 0.0));
    }

    /// A world of planes: each (a point on it, its unit normal); the line from `from`
    /// along unit `d` meets the nearest one it faces within `reach`.
    fn planes(planes: &[([f32; 3], [f32; 3])], from: [f32; 3], d: [f32; 3], reach: f32) -> Option<([f32; 3], [f32; 3])> {
        use fnv_combat::vec;
        planes
            .iter()
            .filter_map(|&(p, n)| {
                let k = vec::dot(d, n);
                (k < -1e-6).then(|| vec::dot(vec::sub(p, from), n) / k).filter(|t| (0.0..=reach).contains(t)).map(|t| (t, vec::add(from, vec::scale(d, t)), n))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, p, n)| (p, n))
    }

    #[test]
    fn a_shot_actor_bleeds_onto_the_wall_behind_or_the_floor_below() {
        use super::{blood_chance, world_blood, ImpactFx, WORLD_BLOOD_CHANCE};
        use fnv_sim::FormId;
        let hit = ImpactFx { impact: FormId(0x77), shooter: FormId(0x14), point: [0.0, 0.0, 50.0], normal: [-1.0, 0.0, 0.0], dir: [2.0, 0.0, 0.0], actor: Some(0x1234), no_spray: false, no_decal: false, splatter: false };
        let floor = ([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let wall = |x: f32| ([x, 0.0, 0.0], [-1.0, 0.0, 0.0]);
        // A wall within reach along the shot: the splatter goes there, facing back.
        let near = [floor, wall(100.0)];
        let fx = world_blood(&hit, FormId(0x78), 0.1, 0.5, |f, d, r| planes(&near, f, d, r)).expect("a splatter on the wall");
        assert_eq!((fx.point, fx.normal, fx.dir), ([100.0, 0.0, 50.0], [-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
        // The blood impact given, not the hit's own.
        assert_eq!(fx.impact, FormId(0x78));
        // A decal only, on the world, heard by no one.
        assert!(fx.splatter && fx.no_spray && !fx.no_decal);
        assert_eq!((fx.actor, fx.shooter), (None, FormId(0)));
        // The wall past reach: the floor below the hit.
        let far = [floor, wall(600.0)];
        let fx = world_blood(&hit, FormId(0x78), 0.1, 0.5, |f, d, r| planes(&far, f, d, r)).expect("a splatter on the floor");
        assert_eq!((fx.point, fx.normal), ([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]));
        // Nothing within reach either way: none.
        assert!(world_blood(&hit, FormId(0x78), 0.1, 0.5, |f, d, r| planes(&[wall(600.0)], f, d, r)).is_none());
        // No Blood Decal (or the armour tweak), the roll lost, a hit on no actor, or a
        // splatter itself: none, and no line is cast.
        let mut cast = 0;
        for h in [ImpactFx { no_decal: true, ..hit }, ImpactFx { actor: None, ..hit }, ImpactFx { splatter: true, ..hit }] {
            assert!(world_blood(&h, FormId(0x78), 0.1, 0.5, |f, d, r| {
                cast += 1;
                planes(&near, f, d, r)
            })
            .is_none());
        }
        assert!(world_blood(&hit, FormId(0x78), 0.5, 0.5, |f, d, r| {
            cast += 1;
            planes(&near, f, d, r)
        })
        .is_none());
        assert_eq!(cast, 0);
        // The game setting, a fraction held to 0..=1; the wiki's 0.75 without it.
        assert_eq!(blood_chance(Some(0.25)), 0.25);
        assert_eq!(blood_chance(Some(0.0)), 0.0);
        assert_eq!(blood_chance(Some(1.0)), 1.0);
        assert_eq!(blood_chance(Some(1.01)), 1.0);
        assert_eq!(blood_chance(Some(50.0)), 1.0);
        assert_eq!(blood_chance(Some(-0.5)), 0.0);
        assert_eq!(blood_chance(Some(f32::NAN)), WORLD_BLOOD_CHANCE);
        assert_eq!(blood_chance(None), 0.75);
        assert_eq!(WORLD_BLOOD_CHANCE, 0.75);
        // A chance of 1 wins every roll, 0 none.
        assert!(world_blood(&hit, FormId(0x78), 0.999, blood_chance(Some(1.0)), |f, d, r| planes(&near, f, d, r)).is_some());
        assert!(world_blood(&hit, FormId(0x78), 0.0, blood_chance(Some(0.0)), |f, d, r| planes(&near, f, d, r)).is_none());
    }

    #[test]
    fn the_splatter_is_the_struck_parts_blood_not_the_weapons_organic_slot() {
        use super::splatter_impact;
        use fnv_combat::data::BodyPart;
        use fnv_combat::fx::{FxForms, IMPACT_MATERIALS};
        use fnv_sim::FormId;
        let organic = IMPACT_MATERIALS.iter().position(|m| *m == "organic").unwrap();
        let (weapon_set, blood_set, scorch, blood) = (FormId(0x10), FormId(0x11), FormId(0x20), FormId(0x21));
        let mut forms = FxForms::default();
        // A flamer's impact data set: a scorch in the organic slot.
        let mut slots = [None; 12];
        slots[organic] = Some(scorch);
        forms.impact_sets.insert(weapon_set, slots);
        let mut slots = [None; 12];
        slots[organic] = Some(blood);
        forms.impact_sets.insert(blood_set, slots);
        let torso = BodyPart { severable_impact_dataset: Some(blood_set), ..Default::default() };
        let explodable_only = BodyPart { explodable_impact_dataset: Some(blood_set), ..Default::default() };
        let bare = BodyPart::default();
        assert_eq!(splatter_impact(&forms, Some(0), Some(&torso), "organic"), Some(blood));
        assert_eq!(splatter_impact(&forms, None, Some(&explodable_only), "organic"), Some(blood));
        // A part with no blood impact data set: none, never the weapon's scorch.
        assert_eq!(splatter_impact(&forms, Some(0), Some(&bare), "organic"), None);
        assert_eq!(splatter_impact(&forms, Some(0), None, "organic"), None);
        // The held weapon and gore pieces, whatever the tweaks: none.
        assert_eq!(splatter_impact(&forms, Some(crate::hitvol::WEAPON_PART), Some(&torso), "organic"), None);
        assert_eq!(splatter_impact(&forms, Some(fnv_phys::gore::GORE_PART), Some(&torso), "organic"), None);
        // Not organic (a robot's metal): none.
        assert_eq!(splatter_impact(&forms, Some(0), Some(&torso), "metal"), None);
    }

    #[test]
    fn a_shot_on_the_player_reads_the_players_reference() {
        assert_eq!(super::actor_ref(super::PLAYER), fnv_sim::FormId::PLAYER_REF.0);
        assert_eq!(super::actor_ref(0xA46), 0xA46);
    }

    use super::*;
    use crate::phys::NpcBody;

    fn presentation_fixture() -> Combat {
        let mut app=App::new();
        app.add_plugins(MinimalPlugins).insert_resource(PlayWorld(crate::PlayData {
            lo:Box::leak(Box::new(fnv_esm::LoadOrder::new(&[]).unwrap())),schema:Box::leak(Box::default()),index:Box::leak(Box::default()),
            world:1,interior:None,eye:[0.0;3],heading_deg:0.0,pitch_deg:0.0,auto_walk:0.0,start_at_cell_spot:false,packages:false,
        })).add_systems(Startup,init_combat);
        app.update();app.world_mut().remove_resource::<Combat>().unwrap()
    }

    #[test]
    fn touch_aim_presentation_uses_current_melee_fists_and_real_scoped_capability() {
        use crate::touch_action_presentation::{aim_role,AimRole};
        let mut c=presentation_fixture();let melee=FormId(1);let gun=FormId(2);let fists=FormId(3);
        c.forms.weapons.insert(melee,WeaponStats::default());c.forms.weapons.insert(fists,WeaponStats::default());c.fists=Some(fists);
        c.forms.weapons.insert(gun,WeaponStats {sight_fov:35.0,scope_nif:Some("fixture-scope.nif".into()),flags:vec!["Has Scope".into()],..default()});
        let mut control=PlayerControl {weapon_out:true,..default()};let mut anim=WeaponAnim {ready:true,..default()};
        c.weapon=Some(melee);c.equipped=Some(Some(melee));c.melee=true;
        assert_eq!(aim_role(Some(&c),Some(&control),Some(&anim),true),AimRole::Block);
        c.weapon=Some(fists);c.equipped=Some(None);
        assert_eq!(aim_role(Some(&c),Some(&control),Some(&anim),true),AimRole::Block,"runtime fists fallback, not an ID guess");
        c.weapon=Some(gun);c.equipped=Some(Some(gun));c.melee=false;
        for aiming in [false,true] {control.aiming=aiming;assert_eq!(aim_role(Some(&c),Some(&control),Some(&anim),true),AimRole::Scope,"scope capability is independent of a held aim key");}
        c.forms.weapons.get_mut(&gun).unwrap().scope_nif=None;
        assert_eq!(aim_role(Some(&c),Some(&control),Some(&anim),true),AimRole::Aim,"a flag without a real scope model is not scoped");
        c.weapon=Some(melee);c.melee=true;
        anim.ready=false;assert_eq!(aim_role(Some(&c),Some(&control),Some(&anim),true),AimRole::Aim);
        anim.ready=true;control.weapon_out=false;assert_eq!(aim_role(Some(&c),Some(&control),Some(&anim),true),AimRole::Aim);
        control.weapon_out=true;assert_eq!(aim_role(Some(&c),Some(&control),Some(&anim),false),AimRole::Aim,"modal context is generic");
        c.weapon=None;assert_eq!(aim_role(Some(&c),Some(&control),Some(&anim),true),AimRole::Aim);
    }

    #[test]
    fn touch_scope_caption_follows_the_equipped_units_actual_scope_mod() {
        use crate::touch_action_presentation::{aim_role,AimRole};
        let mut c=presentation_fixture();let gun=FormId(5);
        c.forms.weapons.insert(gun,WeaponStats {sight_fov:55.0,scope_nif:Some("fixture-scope.nif".into()),flags:vec!["Has Scope".into(),"Scope from Mod".into()],
            mods:vec![fnv_combat::data::WeaponMod {slot:1,item:None,effect:Some("Increase Zoom".into()),value_a:10.0,value_b:0.0}],..default()});
        c.weapon=Some(gun);c.equipped=Some(Some(gun));c.melee=false;
        let control=PlayerControl {weapon_out:true,..default()};let anim=WeaponAnim {ready:true,..default()};
        assert_eq!(aim_role(Some(&c),Some(&control),Some(&anim),true),AimRole::Aim);
        c.mods=fnv_combat::data::mod_flag(1);
        assert_eq!(aim_role(Some(&c),Some(&control),Some(&anim),true),AimRole::Scope);
        c.mods=0;assert_eq!(aim_role(Some(&c),Some(&control),Some(&anim),true),AimRole::Aim);
    }

    #[test]
    fn published_touch_role_clears_modal_state_without_mutating_the_input_owner() {
        use crate::touch_action_presentation::{AimPresentation,AimRole,TouchActionPresentationPlugin};
        let mut c=presentation_fixture();let weapon=FormId(8);c.weapon=Some(weapon);c.melee=true;c.forms.weapons.insert(weapon,WeaponStats::default());
        let mut app=App::new();app.add_plugins((MinimalPlugins,bevy::state::app::StatesPlugin,TouchActionPresentationPlugin))
            .insert_state(PlayState::Playing).insert_resource(c).insert_resource(PlayerControl {weapon_out:true,aiming:true,..default()})
            .insert_resource(WeaponAnim {ready:true,..default()}).insert_resource(crate::pause::Paused(false));
        app.update();assert_eq!(app.world().resource::<AimPresentation>().0,AimRole::Block);
        app.world_mut().resource_mut::<crate::pause::Paused>().0=true;app.update();assert_eq!(app.world().resource::<AimPresentation>().0,AimRole::Aim);
        assert!(app.world().resource::<PlayerControl>().aiming,"presentation never cancels somebody else's held action");
        app.world_mut().resource_mut::<crate::pause::Paused>().0=false;app.insert_state(PlayState::Menu);app.update();assert_eq!(app.world().resource::<AimPresentation>().0,AimRole::Aim);
        app.insert_state(PlayState::Playing);app.update();assert_eq!(app.world().resource::<AimPresentation>().0,AimRole::Block);
        app.world_mut().resource_mut::<WeaponAnim>().ready=false;app.update();assert_eq!(app.world().resource::<AimPresentation>().0,AimRole::Aim);
    }

    /// #2743: god mode keeps the player's weapon and armour from wearing, nobody else's.
    #[test]
    fn god_mode_keeps_the_players_condition_only() {
        assert!(keeps_condition(FormId::PLAYER_REF, true));
        assert!(!keeps_condition(FormId::PLAYER_REF, false));
        assert!(!keeps_condition(FormId(0x000F5704), true));
    }

    #[test]
    fn shot_sounds_are_scaled_only_off_one() {
        set_weapon_volume(1.0, 0.5);
        assert_eq!((shot_gain(true), shot_gain(false)), (None, Some(0.5)));
        set_weapon_volume(1.0, 1.0);
        assert_eq!(shot_gain(false), None);
    }

    #[test]
    fn a_soft_kick_returns_a_share_of_the_turn_the_view_made() {
        let mut player = Player { heading: 0.0, pitch: 0.0, eye_height: 0.0, feet: [0.0; 3], body_eye: None, body_view: None, placed: None };
        let kick = [0.02, 0.01];
        // Free to rise: it comes in over a few frames, all of it, and 40 % of it returns.
        let (mut left, mut goal, mut turned_sum) = (kick, [0.0; 2], [0.0; 2]);
        for _ in 0..120 {
            let turned;
            (left, turned) = ease_kick(&mut player, left, Some(crate::inertia::KICK_EASE_SECONDS), 1.0 / 60.0);
            kick_back(&mut goal, turned, true);
            turned_sum = [turned_sum[0] + turned[0], turned_sum[1] + turned[1]];
        }
        assert!(left == [0.0; 2] && (turned_sum[0] - kick[0]).abs() < 1e-6 && (player.pitch - kick[0]).abs() < 1e-6);
        assert!((goal[0] - crate::inertia::KICK_BACK_SHARE * kick[0]).abs() < 1e-6);
        // At the pitch limit the view cannot rise: nothing of the pitch returns after it.
        let mut player = Player { pitch: MAX_PITCH, ..player };
        let mut goal = [0.0; 2];
        let (_, turned) = ease_kick(&mut player, kick, None, 1.0 / 60.0);
        kick_back(&mut goal, turned, true);
        assert_eq!((player.pitch, goal[0]), (MAX_PITCH, 0.0));
        assert!((goal[1] - crate::inertia::KICK_BACK_SHARE * kick[1]).abs() < 1e-6, "the sideways part still turned");
        // Soft recoil off: nothing returns.
        let mut goal = [0.0; 2];
        kick_back(&mut goal, [0.02, 0.01], false);
        assert_eq!(goal, [0.0; 2]);
    }

    #[test]
    fn melee_hold_refuses_switch_without_starting_a_swing() {
        use crate::{bindings::{Bindings, Control, InputDevice}, pad::device::tests::{button, control_button, playing_app}};
        let (mut app, pad) = playing_app(InputDevice::KeyboardMouse);
        let forms = Box::leak(Box::new(fnv_sim::FormStore::default()));
        let refs = Box::leak(Box::new(fnv_sim::RefTable::default()));
        app.insert_resource(SimState { forms, refs, world: fnv_sim::World::new(forms, refs, fnv_sim::SimConfig::default()) })
            .insert_resource(PlayWorld(crate::PlayData {
                lo: Box::leak(Box::new(fnv_esm::LoadOrder::new(&[]).unwrap())), schema: Box::leak(Box::default()), index: Box::leak(Box::default()),
                world: 1, interior: None, eye: [0.0; 3], heading_deg: 0.0, pitch_deg: 0.0, auto_walk: 0.0, start_at_cell_spot: false, packages: false,
            }))
            .insert_resource(Player { heading: 0.0, pitch: 0.0, eye_height: 0.0, feet: [0.0; 3], body_eye: None, body_view: None, placed: None })
            .init_resource::<PhysState>().init_resource::<MoveIntent>().init_resource::<WeaponAnim>()
            .init_resource::<CombatStats>().init_resource::<HitFeedback>().init_resource::<ActorParts>()
            .init_resource::<PlayerLimbs>().init_resource::<crate::dismember::Dismemberment>()
            .add_message::<ActorHit>().add_message::<ActorPush>().add_message::<WeaponBroken>().add_message::<ForcedHitReaction>()
            .add_message::<PlaySound>().add_message::<ImpactFx>().add_message::<Knockback>()
            .add_systems(Startup, init_combat).add_systems(Update, melee.after(crate::control::read_controls));
        app.update();
        let weapon = FormId(1);
        {
            let mut c = app.world_mut().resource_mut::<Combat>();
            c.forms.weapons.insert(weapon, WeaponStats::default());
            c.weapon = Some(weapon);
            c.melee = true;
        }
        app.world_mut().resource_mut::<WeaponAnim>().ready = true;
        app.world_mut().resource_mut::<PlayerControl>().weapon_out = true;
        control_button(&mut app, pad, InputDevice::KeyboardMouse, Control::Use, true);
        app.update();
        assert!(app.world().resource::<PlayerControl>().trigger);
        assert!(app.world().resource::<Combat>().swings.swing.is_none());
        button(&mut app, pad, true);
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
        assert!(app.world().resource::<PlayerControl>().trigger);
        assert_eq!(app.world().resource::<CombatStats>().swings, 0);
        // The physical release commits the original tap on its original device;
        // only a subsequent fresh pad press switches [design, #2200].
        control_button(&mut app, pad, InputDevice::KeyboardMouse, Control::Use, false);
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
        assert_eq!(app.world().resource::<CombatStats>().swings, 1);
        button(&mut app, pad, false);
        app.update();
        button(&mut app, pad, true);
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller);
        assert_eq!(app.world().resource::<CombatStats>().swings, 1);
    }

    #[test]
    fn player_capsule_hit_bands_and_clamps() {
        use fnv_game::combat::{HEAD, LEFT_ARM, LEFT_MOBILITY, RIGHT_ARM, RIGHT_MOBILITY, TORSO};
        let part = |x: f32, z: f32| player_hit_condition([10.0 + x, 20.0, 30.0 + z], [10.0, 20.0, 30.0], 100.0, 10.0, 0.0);
        for (x, z, expected) in [
            (0.0, 87.0, HEAD), (-10.0, 1000.0, HEAD),
            (-1.0, 49.0, LEFT_MOBILITY), (1.0, 49.0, RIGHT_MOBILITY),
            (-1.0, -100.0, LEFT_MOBILITY), (1.0, -100.0, RIGHT_MOBILITY),
            (-7.0, 65.0, LEFT_ARM), (7.0, 65.0, RIGHT_ARM),
            (-6.0, 65.0, TORSO), (6.0, 65.0, TORSO), (0.0, 65.0, TORSO),
            (0.0, 50.0, TORSO), (0.0, 86.0, TORSO), (7.0, 86.0, RIGHT_ARM),
            (0.0, 0.0, RIGHT_MOBILITY),
        ] {
            assert_eq!(part(x, z), expected, "x {x}, z {z}");
        }
    }

    #[test]
    fn player_capsule_sides_follow_heading() {
        use fnv_game::combat::{LEFT_ARM, LEFT_MOBILITY, RIGHT_ARM, RIGHT_MOBILITY};
        for (heading, right) in [(std::f32::consts::FRAC_PI_2, [0.0, -8.0]), (std::f32::consts::PI, [-8.0, 0.0])] {
            for (side, arm, leg) in [(1.0, RIGHT_ARM, RIGHT_MOBILITY), (-1.0, LEFT_ARM, LEFT_MOBILITY)] {
                for (z, expected) in [(65.0, arm), (25.0, leg)] {
                    assert_eq!(player_hit_condition([right[0] * side, right[1] * side, z], [0.0; 3], 100.0, 10.0, heading), expected);
                }
            }
        }
    }

    #[test]
    fn player_shot_head_multiplier_and_blocked_blow_limb_damage() {
        let bp = BodyPart { damage_mult: 2.0, health_percent: 20, actor_value: Some("Perception Condition".into()), part_type: Some("Head 1".into()), ..Default::default() };
        let w = WeaponStats { base_damage: 16, limb_dmg_mult: 1.0, ..Default::default() };
        assert_eq!(bp.hit_part().damage_mult, 2.0);
        assert_eq!(bp.location(), 1);
        let hit = |part| damage::hit(&Rules::defaults(), &CombatTuning::default(), &HitInput { weapon: &w, projectiles: 1, condition: 1.0, ammo_effects: &[], part, power: 1.0 }, &Attacker::default(), &Target::default(), &NoPerks, &NoPerks, 1.0);
        let mut d = hit(bp.hit_part());
        let body = hit(HitPart::default());
        assert!(body.dealt > 0.0);
        assert_eq!(d.dealt, body.dealt * 2.0);
        assert_eq!(limb_hit(Some(&bp), &d, &w).unwrap().damage, f64::from(body.dealt));
        // [design: actor_attacks] Blows keep the default part and use damage left after blocking.
        d.part_mult = HitPart::default().damage_mult;
        d.dealt = damage::against_block(&CombatTuning::default(), 16.0, false).0;
        assert_eq!(limb_hit(Some(&bp), &d, &w).map(|l| l.damage), (d.dealt > 0.0).then_some(f64::from(d.dealt)));
        d.dealt = 0.0;
        assert!(limb_hit(Some(&bp), &d, &w).is_none());
        assert!(limb_hit(None, &d, &w).is_none());
    }

    #[test]
    fn npc_weapons_wear_only_with_their_own_damage_to_weapon_mult() {
        let mut forms = CombatForms::default();
        forms.weapons.insert(FormId(1), fnv_combat::data::WeaponStats::default());
        forms.weapons.insert(FormId(2), fnv_combat::data::WeaponStats { damage_to_weapon_mult: Some(0.005), ..Default::default() });
        assert!(!npc_weapon_wears(&forms, FormId(1)), "the masters' setting: an NPC's weapon does not wear (#1334)");
        assert!(npc_weapon_wears(&forms, FormId(2)), "the record's override: it wears, as Cook-Cook's Incinerator");
        assert!(!npc_weapon_wears(&forms, FormId(3)), "no record");
    }

    #[test]
    fn the_magazine_never_holds_more_than_is_carried() {
        assert_eq!(magazine_in_hand(30, 0, false, false, false), 0, "every round removed");
        assert_eq!(magazine_in_hand(30, 12, false, false, false), 12);
        // Fewer loaded than carried stay so: VATS mode counts the rounds loaded, and
        // a reload (whole or round by round) is what tops the magazine up.
        assert_eq!(magazine_in_hand(2, 20, false, false, false), 2);
        // No rounds in the inventory to follow: no ammo item, regenerating, god mode.
        assert_eq!(magazine_in_hand(1, 0, true, false, false), 1);
        assert_eq!(magazine_in_hand(5, 0, false, true, false), 5);
        assert_eq!(magazine_in_hand(30, 0, false, false, true), 30);
    }

    #[test]
    fn the_counter_shows_a_regenerating_weapons_charge() {
        // Rounds from the inventory: those loaded, and the rest of what is carried.
        assert_eq!(hud_ammo(6, 48, false), (6, 42));
        assert_eq!(hud_ammo(6, 4, false), (4, 0));
        assert_eq!(hud_ammo(6, 0, false), (0, 0));
        // The Recharger Rifle carries no rounds: its charge is the count (#2170).
        assert_eq!(hud_ammo(7, 0, true), (7, 0));
        assert_eq!(hud_ammo(0, 0, true), (0, 0));
        assert_eq!(hud_ammo(7, 20, true), (7, 20));
    }

    #[test]
    fn sights_are_up_only_with_a_ready_gun_and_no_reload() {
        assert!(sights_up(true, true, false, true));
        // Reloading (or clearing a jam) holds them down though Block is held.
        assert!(!sights_up(true, true, true, true));
        // Blocking with a melee weapon: no sights.
        assert!(!sights_up(true, true, false, false));
        // Block not held, or the weapon not ready yet.
        assert!(!sights_up(true, false, false, true));
        assert!(!sights_up(false, true, false, true));
    }

    #[test]
    fn a_reload_or_a_jam_holds_the_sights_down() {
        for p in [
            Phase::Reloading { left: 1.0 },
            Phase::ReloadOpen { left: 0.3, stop: false },
            Phase::ReloadRound { left: 0.5, stop: false, taken: 1 },
            Phase::ReloadClose { left: 0.4, taken: 2 },
            Phase::Jammed { left: 1.2 },
        ] {
            assert!(sights_held_down(&p), "{p:?}");
        }
        // Firing and waiting keep them up.
        for p in [Phase::Ready, Phase::SpinUp { left: 0.2 }, Phase::WindUp { left: 0.1 }, Phase::Cooldown { left: 0.3 }] {
            assert!(!sights_held_down(&p), "{p:?}");
        }
    }

    #[test]
    fn a_knockback_pushes_half_its_force_over_its_time() {
        // A Sledgehammer blow of 14 on Agility 5: force 114 (combat-rules.md, Knockback).
        let r = fnv_combat::tuning::Rules::defaults();
        let force = damage::knockback_force(&r, 14.0, 5.0);
        assert!((force - 114.0).abs() < 0.5, "{force}");
        let k = Knockback::of(7, [0.0, 2.0, -1.0], force, 1.0).unwrap();
        assert_eq!(k.dir, [0.0, 1.0]);
        let dt = 1.0 / 60.0;
        let (mut left, mut far) = (k.secs, 0.0);
        while left > 0.0 {
            far += k.step(left, dt);
            left -= dt;
        }
        assert!((far - k.distance()).abs() < 1e-2 && (far - 57.0).abs() < 0.5, "{far}");
        // Straight down, or with no force: no push.
        assert!(Knockback::of(7, [0.0, 0.0, -1.0], force, 1.0).is_none() && Knockback::of(7, [1.0, 0.0, 0.0], 0.0, 1.0).is_none());
    }

    /// The laser rifle's beam (BeamLaserProjectile, Fade Duration 0.14 s) is drawn in
    /// full when made, at half strength half way through and not at all at the end; a
    /// bullet, or a beam whose PROJ gives no fade, in full.
    /// A stand-in world: a ray hits actor 7's part 3 at (1, 2, 3) unless 7 is ignored,
    /// when it hits actor 9 (no part); actors 7 and 9 stand near.
    struct Two;
    impl CombatWorld for Two {
        fn ray(&self, _from: V3, _to: V3, ignore: &[ActorId]) -> Option<fnv_combat::projectile::WorldHit> {
            let (actor, part) = if ignore.contains(&7) { (9, None) } else { (7, Some(3)) };
            Some(fnv_combat::projectile::WorldHit { point: [1.0, 2.0, 3.0], normal: [0.0, 0.0, 1.0], distance: 1.0, actor: Some(actor), part, object: None, small_transparent: false, material: None })
        }
        fn actors_near(&self, _point: V3, _radius: f32) -> Vec<(ActorId, V3)> {
            vec![(7, [0.0; 3]), (9, [0.0; 3])]
        }
    }

    #[test]
    fn a_blast_or_blow_lands_on_the_part_its_line_meets() {
        // The other actor is left out of the line: actor 7's part 3.
        assert_eq!(facing_part(&Two, 7, [0.0; 3], [1.0, 0.0, 0.0]), Some((3, [1.0, 2.0, 3.0])));
        // Actor 9 has no part volumes: nothing to dismember.
        assert_eq!(facing_part(&Two, 9, [0.0; 3], [1.0, 0.0, 0.0]), None);
    }

    #[test]
    fn a_beam_fades_over_its_fade_duration() {
        assert_eq!(beam_strength(true, 0.14, 0.14), 1.0);
        assert!((beam_strength(true, 0.07, 0.14) - 0.5).abs() < 1e-6);
        assert_eq!(beam_strength(true, -0.01, 0.14), 0.0);
        assert_eq!(beam_strength(false, 0.0, 0.14), 1.0);
        assert_eq!(beam_strength(true, 0.0, 0.0), 1.0);
    }

    /// A blast is its cause's hit: an NPC's Incinerator burst on a trooper is that NPC's
    /// with that weapon, not the player's (every blast used to be the player's, with the
    /// player's weapon: XP, hit marker and OnHit attacker for NPC-against-NPC blasts, and
    /// none of them in the `npc hits summary`). One nobody set off has no attacker.
    /// Only the player's hits made sneaking are sneak attacks; with no detection measured
    /// the target counts as unseen.
    #[test]
    fn sneak_attacks_are_the_sneaking_players() {
        let target = FormId(0x00104F0A);
        assert!(sneak_attack(None, FormId::PLAYER_REF, true, target));
        assert!(!sneak_attack(None, FormId::PLAYER_REF, false, target));
        assert!(!sneak_attack(None, FormId(0x000F5704), true, target));
    }

    #[test]
    fn a_blast_hit_is_its_causes_not_the_players() {
        let by = By { actor: FormId(0x000F5704), weapon: FormId(0x000906DA), ammo: None, harmless: false, sure: None, vats: false, sneaking: false };
        let expl = FormId(0xFF00_0042);
        let info = blast_hit_info(Some(Cause::of(&by)), Some(expl), 125.0);
        assert_eq!(
            (info.attacker, info.weapon, info.projectile, info.location, info.critical, info.explosion, info.base_weapon_damage),
            (Some(FormId(0x000F5704)), Some(FormId(0x000906DA)), Some(expl), -1, false, true, 125.0)
        );
        let none = blast_hit_info(None, None, 100.0);
        assert_eq!(
            (none.attacker, none.weapon, none.projectile, none.explosion, none.base_weapon_damage),
            (None, None, None, true, 100.0)
        );
    }

    /// A grenade's cooldown, played from the shot (the clip's `Release`, 0.867 s) as the
    /// cycle runs it: 6 until `Attach` (1.533 s), then 4 to the clip's end (1.7 s). The
    /// clip's time used to start at twice the fire delay, past `Attach` (smoke
    /// anim-state-throw: 5 then 4, no 6).
    #[test]
    fn a_throws_cooldown_reads_6_until_attach_then_4() {
        use fnv_game::anim_state::AnimAction;
        let timing = fnv_combat::attack::Timing { interval: 1.7, fire_delay: 0.867, reload: 2.0, spin_up: 0.0, rounds: None, attach: Some(1.533) };
        let dt = 1.0 / 60.0;
        let mut left = timing.interval - timing.fire_delay;
        let mut codes = Vec::new();
        while left > 0.0 {
            left -= dt;
            codes.push(cooldown_action(&timing, left.max(0.0), true, false, dt).map(AnimAction::code));
        }
        let sixes = codes.iter().filter(|c| **c == Some(6)).count();
        let fours = codes.iter().filter(|c| **c == Some(4)).count();
        assert_eq!(codes[0], Some(6), "6 from the release: {codes:?}");
        assert!((38..=41).contains(&sixes), "{codes:?}");
        assert!((8..=11).contains(&fours), "{codes:?}");
        // A pistol's shot: 3 the frame after, then 4 (no 2 in the cooldown).
        let gun = fnv_combat::attack::Timing { interval: 0.5, fire_delay: 0.033, attach: None, ..timing };
        let first = cooldown_action(&gun, gun.interval - gun.fire_delay - dt, false, false, dt);
        assert_eq!(first, Some(AnimAction::FollowThrough));
        assert_eq!(cooldown_action(&gun, 0.2, false, false, dt), Some(AnimAction::AttackReturn));
    }

    /// An NPC walked by a character controller: its character (a man-sized capsule from
    /// the ground) and, inside it, a smaller body hovering (a bloatfly's hit volume).
    fn npc_world() -> PhysState {
        let mut phys = PhysState::default();
        let id = 0x1234;
        let ch = phys.world.add_character(id, fnv_phys::CharacterParams::default(), [0.0, 0.0, 0.0]);
        let standing_height = ch.params.height;
        phys.npcs.insert(id, NpcBody { prev_feet: ch.feet, ch, velocity: [0.0; 2], jump: false, sink: 0.0, lift: Default::default(), standing_height, sneak_crouch: false });
        phys.world.set_actor_volume(id, fnv_phys::HitVolume { feet: [0.0, 0.0, 30.0], radius: 8.0, height: 30.0, dead: false });
        phys
    }

    #[test]
    fn player_knife_reaches_a_low_widened_creature_at_its_blocking_stop() {
        use crate::blocking::{clip_pushing_nobody, walking_radius, Block};
        use fnv_combat::projectile::{swing, swing_reaching};
        // Synthetic low body and knife [design, #2535]. Blocking and combat read
        // the same published widths, including one beyond the old coarse query.
        let id = 0x2535;
        let eye = [0.0, 0.0, 112.0];
        let reach = 53.5 + 22.0;
        let tuning = CombatTuning::default();
        for width in [88.2, 400.0] {
            for walking in [false, true] {
                let mut phys = PhysState::default();
                phys.actor_block_radii.insert(id, width);
                let radius = walking_radius(22.0, phys.actor_block_radii.get(&id).copied());
                let target = [width + 100.0, 0.0, 0.0];
                let block = Block::character(id, target, radius, 30.0, false);
                let velocity = clip_pushing_nobody(PLAYER as u32, [200.0, 0.0], [0.0; 3], 22.0, 128.0, 1.0, &mut [block]);
                let feet = [target[0] - velocity[0], 0.0, 0.0];
                assert!((feet[0] - (width + 22.0)).abs() < 1e-4);
                phys.world.set_actor_volume(id, fnv_phys::HitVolume { feet, radius: 15.0, height: 30.0, dead: false });
                if walking {
                    let ch = phys.world.add_character(id, fnv_phys::CharacterParams::default(), feet);
                    let standing_height = ch.params.height;
                    phys.npcs.insert(id, NpcBody { prev_feet: feet, ch, velocity: [0.0; 2], jump: false, sink: 0.0, lift: Default::default(), standing_height, sneak_crouch: false });
                }
                for forward in [[1.0, 0.0, 0.0], vec::normalize([1.0, 0.0, -1.0])] {
                    let mut world = view(&mut phys, None, None).for_melee();
                    assert!(swing(&world, &tuning, eye, forward, [0.0, 0.0, 1.0], reach, PLAYER).is_empty());
                    let hits = swing_reaching(&world, &tuning, eye, 0.0, forward, reach, PLAYER);
                    assert_eq!(hits.iter().map(|h| h.0).collect::<Vec<_>>(), vec![id as ActorId], "width={width}, walking={walking}");
                    world.ghosts.push(id as ActorId);
                    assert!(swing_reaching(&world, &tuning, eye, 0.0, forward, reach, PLAYER).is_empty());
                }
                // Widening retains a finite ground-reach boundary [design, #2535].
                let outside = [reach + width + 0.01, 0.0, 0.0];
                phys.world.set_actor_volume(id, fnv_phys::HitVolume { feet: outside, radius: 15.0, height: 30.0, dead: false });
                assert!(swing_reaching(&view(&mut phys, None, None).for_melee(), &tuning, eye, 0.0, [1.0, 0.0, 0.0], reach, PLAYER).is_empty());
                phys.world.set_actor_volume(id, fnv_phys::HitVolume { feet, radius: 15.0, height: 30.0, dead: false });
                // A wall still shields the widened target [design, #2535].
                use fnv_collide::{CollisionObject, Iso, Motion, Shape};
                let wall = CollisionObject {
                    node: None, node_block: 0, node_transform: Iso::IDENTITY, body_transform: Iso::IDENTITY,
                    motion: Motion::Static, layer: layers::STATIC, mass: 0.0, friction: 0.5, restitution: 0.0,
                    shape: Shape::Box { half_extents: [5.0, 100.0, 100.0] }.into(), shell: 0.0, material: None, wind: false,
                };
                phys.world.add_ref(&fnv_phys::RefPose {
                    form_id: id + 1, position: [30.0, 0.0, 80.0],
                    rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], scale: 1.0,
                }, &[wall]);
                let world = view(&mut phys, None, None).for_melee();
                assert!(swing_reaching(&world, &tuning, eye, 0.0, [1.0, 0.0, 0.0], reach, PLAYER).iter().all(|h| h.0 != id as ActorId));
            }
        }
    }

    #[test]
    fn player_ground_reach_keeps_ordinary_targets_at_the_old_boundary() {
        use fnv_combat::projectile::swing_reaching;
        // Synthetic body, boundary and invalid/small BBX cases [design, #2535].
        let id = 0x2535;
        let reach = 53.5 + 22.0;
        for bbx in [None, Some(20.0), Some(f32::NAN)] {
            for (distance, expected) in [(reach + 24.0 - 0.01, true), (reach + 24.0 + 0.01, false)] {
                let mut phys = PhysState::default();
                if let Some(bbx) = bbx {
                    phys.actor_block_radii.insert(id, bbx);
                }
                phys.world.set_actor_volume(id, fnv_phys::HitVolume { feet: [distance, 0.0, 0.0], radius: 15.0, height: 30.0, dead: false });
                let world = view(&mut phys, None, None).for_melee();
                let hits = swing_reaching(&world, &CombatTuning::default(), [0.0, 0.0, 112.0], 0.0, [1.0, 0.0, 0.0], reach, PLAYER);
                assert_eq!(hits.iter().any(|h| h.0 == id as ActorId), expected);
            }
        }
    }

    #[test]
    fn shots_pass_an_npcs_character_and_strike_its_body() {
        // The playtest list 5 item 16: the character was struck as a wall (dust, no damage).
        let mut phys = npc_world();
        let world = view(&mut phys, None, None);
        let h = world.ray([-300.0, 0.0, 45.0], [300.0, 0.0, 45.0], &[PLAYER]).expect("the body is struck");
        assert_eq!(h.actor, Some(0x1234));
        assert!(h.point[0] > -10.0, "struck at the body, not the character's side: {:?}", h.point);
        // Below the body, through the character's legs: nothing.
        assert!(world.ray([-300.0, 0.0, 10.0], [300.0, 0.0, 10.0], &[PLAYER]).is_none());
    }

    /// A corpse lying along x at rest (its ragdoll held, so no ground is needed): a trunk,
    /// body part 0, centred at (100, 0, 10), and a head, part 1, at (140, 0, 10); with
    /// `stale`, the hit volume its ragdoll has not yet taken over from this frame.
    fn corpse_world(id: u32, stale: bool) -> PhysState {
        use fnv_collide::ragdoll::{Ragdoll, RagdollBody};
        use fnv_collide::{CollisionObject, Iso, Motion, Shape};
        let body = |shape: Shape, mass: f32| {
            let object = CollisionObject {
                node: Some("Bip01".into()),
                node_block: 0,
                node_transform: Iso::IDENTITY,
                body_transform: Iso::IDENTITY,
                motion: Motion::Keyframed,
                layer: fnv_phys::layers::BIPED,
                mass,
                friction: 0.3,
                restitution: 0.8,
                shape: shape.into(),
                shell: 0.0,
                material: None,
                wind: false,
            };
            RagdollBody { object, linear_damping: 0.1, angular_damping: 0.05, stored: Iso::IDENTITY, unit: 7.0, inertia: None }
        };
        let ragdoll = std::sync::Arc::new(Ragdoll { bodies: vec![body(Shape::Box { half_extents: [30.0, 10.0, 8.0] }, 20.0), body(Shape::Sphere { radius: 8.0 }, 4.0)], joints: vec![], problems: vec![] });
        let at = |p: [f32; 3]| Iso { translation: p, rotation: Iso::IDENTITY.rotation };
        let nodes = [at([100.0, 0.0, 10.0]), at([140.0, 0.0, 10.0])];
        let mut phys = PhysState::default();
        phys.world.add_ragdoll(&fnv_phys::RagdollStart { form_id: id, ragdoll: &ragdoll, scale: 1.0, nodes: &nodes, velocity: [0.0; 3], parts: &[0, 1], asleep: true });
        // A trap's jointed bodies beside it: a placed object's, not a corpse.
        let trap = [at([100.0, 100.0, 10.0]), at([140.0, 100.0, 10.0])];
        phys.world.add_object_ragdoll(&fnv_phys::RagdollStart { form_id: id + 1, ragdoll: &ragdoll, scale: 1.0, nodes: &trap, velocity: [0.0; 3], parts: &[], asleep: true });
        if stale {
            phys.world.set_actor_volume(id, fnv_phys::HitVolume { feet: [100.0, 0.0, 2.0], radius: 20.0, height: 16.0, dead: true });
        }
        phys
    }

    #[test]
    fn a_blast_reaches_a_settled_corpse_once_and_checks_the_part_facing_it() {
        // #2346: the corpse's hit volume was gone once its ragdoll was built, and blasts
        // asked only the volumes, so a mine or grenade beside it never reached the gore check.
        use fnv_combat::dismember::{decide, Blow, Fate, OnHit, Settings};
        let id = 0x2346u32;
        let e = fnv_combat::data::ExplosionStats { damage: 100.0, radius: 200.0, force: 300.0, ..Default::default() };
        let center = [100.0, -60.0, 10.0];
        for stale in [false, true] {
            let mut phys = corpse_world(id, stale);
            let world = view(&mut phys, None, None);
            let hits = blast(&world, &CombatTuning::default(), &e, center, None);
            assert_eq!(hits.iter().filter(|h| h.actor == id as ActorId).count(), 1, "caught once (stale volume: {stale}): {hits:?}");
            assert!(hits.iter().all(|h| h.actor != (id + 1) as ActorId), "a trap's bodies are not a corpse: {hits:?}");
            // Proximity fuses and swings still go by the living: a corpse sets off no mine.
            // (A volume not yet dropped is still the living's.)
            assert_eq!(world.actors_near(center, 200.0).iter().any(|(a, _)| *a == id as ActorId), stale);
        }
        let mut phys = corpse_world(id, false);
        let world = view(&mut phys, None, None);
        let (_, trunk) = world.blast_targets_near(center, 200.0).into_iter().find(|(a, _)| *a == id as ActorId).expect("the corpse is near");
        assert!(vec::len(vec::sub(trunk, [100.0, 0.0, 10.0])) < 1.0, "its trunk's centre: {trunk:?}");
        assert!(world.aim_points(id as ActorId).len() == 2, "its bodies are its aim points");
        // Its body part data: part 0 the torso (explodable only), part 1 the head.
        let part = |kind: &str, flags: &[&str], health: u32, av: &str| BodyPart { part_type: Some(kind.into()), flags: flags.iter().map(|f| f.to_string()).collect(), health_percent: health, explodable_chance_percent: 30, actor_value: Some(av.into()), ..BodyPart::default() };
        let bptd = fnv_combat::data::BodyPartData { parts: vec![part("Torso", &["Explodable"], 60, "Endurance Condition"), part("Head 1", &["Severable", "Explodable"], 20, "Perception Condition")], edid: None, model: None, ragdoll: None };
        let corpse = HitResult { corpse: true, immune: true, overkill: 60.0, max_health: 100.0, ..HitResult::default() };
        let s = Settings::default();
        // From each blast's hit on the corpse, as `fly` does: the part facing it, the check
        // made there, then the rolls (fixed) on that part of its data, as `dismember::resolve`
        // makes them.
        let gore = |at: V3, w: &WeaponStats| {
            let hits = blast(&world, &CombatTuning::default(), &e, at, None);
            let b = hits.iter().find(|h| h.actor == id as ActorId).expect("the blast catches the corpse");
            let (facing, check) = blast_gore(&world, FormId(id), b.actor, at, e.radius, Some(w), &corpse, None, b.push);
            let check = check.expect("a corpse's facing part is checked");
            assert_eq!(facing.map(|f| f.0 as usize), Some(check.part));
            assert!(check.corpse && check.target == FormId(id));
            check
        };
        let roll = |check: &crate::dismember::Check, bloody_mess: bool, r: f32| {
            let blow = Blow { on_hit: check.on_hit, corpse: check.corpse, crippled: check.crippled, overkill: check.overkill, max_health: check.max_health, bloody_mess, sole_part: bptd.parts.len() == 1 };
            decide(&bptd.parts[check.part], &blow, &s, r)
        };
        // Beside its trunk: the torso faces it, and goes only with Bloody Mess (80 % on a
        // corpse, dismemberment.md §4.2).
        let normal = WeaponStats::default();
        let torso = gore(center, &normal);
        assert_eq!((torso.part, torso.on_hit), (0, OnHit::Normal));
        assert_eq!(roll(&torso, false, 0.0), None);
        assert_eq!(roll(&torso, true, 0.0), Some(Fate::Explode));
        assert_eq!(roll(&torso, true, 0.9), None);
        // Past its head: the head faces it; a severable part goes 75 % of the time, 30 % of
        // that severed. A No Dismember/Explode weapon (every mine) takes nothing without
        // Bloody Mess and explodes at half the odds with it: the gate left as it is.
        let head = gore([200.0, 0.0, 10.0], &normal);
        assert_eq!(head.part, 1);
        assert_eq!(roll(&head, false, 0.0), Some(Fate::Explode));
        assert_eq!(roll(&head, false, 0.6), Some(Fate::Sever));
        assert_eq!(roll(&head, false, 0.9), None);
        let mine = WeaponStats { on_hit: Some("No Dismember/Explode".into()), ..WeaponStats::default() };
        let mined = gore([200.0, 0.0, 10.0], &mine);
        assert_eq!(mined.on_hit, OnHit::Neither);
        assert_eq!(roll(&mined, false, 0.0), None);
        assert_eq!(roll(&mined, true, 0.4), Some(Fate::Explode));
        assert_eq!(roll(&mined, true, 0.6), None);
        // Without a source weapon (a placed explosion, a mine with no actor owner): no check.
        assert_eq!(blast_gore(&world, FormId(id), id as ActorId, center, e.radius, None, &corpse, None, [0.0; 3]).1.map(|c| c.part), None);
        // A bullet still strikes the corpse on its part (the control).
        let shot = world.ray([140.0, -300.0, 10.0], [140.0, 300.0, 10.0], &[PLAYER]).expect("the corpse's head is struck");
        assert_eq!((shot.actor, shot.part), (Some(id as ActorId), Some(1)));
    }

    #[test]
    fn jam_animations_beside_reload_animations() {
        assert_eq!(jam_anim("ReloadA").as_deref(), Some("JamA"));
        assert_eq!(jam_anim("ReloadZ").as_deref(), Some("JamZ"));
        assert_eq!(jam_anim("Reload"), None);
        assert_eq!(jam_anim("AttackLeft"), None);
    }

    #[test]
    fn explosive_projectile_entry_is_distinct_from_detonation() {
        assert_eq!(water_entry_kind(Motion::Missile, false), Some(WaterImpact::Bullet));
        assert_eq!(water_entry_kind(Motion::Missile, true), Some(WaterImpact::HeavyEntry));
        assert_eq!(water_entry_kind(Motion::Lobber, true), Some(WaterImpact::HeavyEntry));
        assert_eq!(water_entry_kind(Motion::Beam, true), None);
        assert_eq!(water_entry_kind(Motion::Flame, false), None);
    }

    #[test]
    fn delayed_explosive_trace_cannot_send_a_heavy_entry_after_its_detonation() {
        let heavy = water_entry_kind(Motion::Missile, true);
        assert_eq!(timed_water_impact(heavy, 0.15, true), None);
        assert_eq!(timed_water_impact(heavy, 0.15, false), heavy, "another projectile flying through water without detonating");
        assert_eq!(timed_water_impact(heavy, 0.0, true), heavy, "a crossing due now precedes the same frame's blast");
        assert_eq!(timed_water_impact(Some(WaterImpact::Bullet), 0.15, false), Some(WaterImpact::Bullet));
    }

    #[test]
    fn explosions_throw_up_water_near_its_surface() {
        // Just over the surface, on it, and deep under it.
        let r = WATER_BLAST_REACH;
        // Over the surface, on it, and on the bed 186 under it (the recording's grenade).
        assert_eq!(water_thrown_up([0.0, 0.0, 140.0], Some(100.0), Some(-200.0), r), Some(100.0));
        assert_eq!(water_thrown_up([0.0, 0.0, 100.0], Some(100.0), None, r), Some(100.0));
        assert_eq!(water_thrown_up([0.0, 0.0, -86.0], Some(100.0), Some(-90.0), r), Some(100.0));
        // Too high over it, too deep, no water, or on a bank above the surface.
        assert_eq!(water_thrown_up([0.0, 0.0, 100.0 + r + 1.0], Some(100.0), None, r), None);
        assert_eq!(water_thrown_up([0.0, 0.0, 100.0 - r - 1.0], Some(100.0), None, r), None);
        assert_eq!(water_thrown_up([0.0, 0.0, 120.0], None, Some(0.0), r), None);
        assert_eq!(water_thrown_up([0.0, 0.0, 130.0], Some(100.0), Some(110.0), r), None);
    }

    /// A game over a plugin with a launcher and a rifle, an explosives list, and the
    /// perks of a blast as the masters' entries have them: the damage perk's condition on
    /// the Weapon tab, Hit the Deck's on the attacker's weapon's, Toughness none.
    mod blast_perks {
        use super::*;
        use fnv_esm::raw::{write_subrecords, Subrecord};
        use fnv_esm::{LoadOrder, Plugin, Schema};
        use fnv_game::perks as p;
        use fnv_game::{Game, GameConfig, GameData};
        use fnv_sim::{FormStore, RefTable, SimConfig, World};

        const CELL: u32 = 0x200;
        const NPC: u32 = 0x400;
        const NPC_REF: u32 = 0x500;
        const LAUNCHER: u32 = 0x600;
        const RIFLE: u32 = 0x601;
        const EXPLOSIVES: u32 = 0x650;
        const DEMOLITION: u32 = 0x700;
        const DECK: u32 = 0x701;
        const TOUGHNESS: u32 = 0x702;
        const IS_IN_LIST: u16 = 372;

        fn sub(sig: &[u8; 4], data: Vec<u8>) -> Subrecord {
            Subrecord { sig: *sig, data, extended: false }
        }

        fn z(s: &str) -> Vec<u8> {
            [s.as_bytes(), &[0]].concat()
        }

        fn record(sig: &[u8; 4], flags: u32, fid: u32, subs: &[Subrecord]) -> Vec<u8> {
            let payload = write_subrecords(subs);
            [&sig[..], &(payload.len() as u32).to_le_bytes(), &flags.to_le_bytes(), &fid.to_le_bytes(), &[0; 4], &15u16.to_le_bytes(), &[0; 2], &payload].concat()
        }

        fn group(label: [u8; 4], gtype: i32, body: &[u8]) -> Vec<u8> {
            [&b"GRUP"[..], &((24 + body.len()) as u32).to_le_bytes(), &label, &gtype.to_le_bytes(), &[0; 8], body].concat()
        }

        /// IsInList Explosives == 1.
        fn in_explosives() -> Subrecord {
            let d = [&[0u8, 0, 0, 0][..], &1f32.to_le_bytes(), &IS_IN_LIST.to_le_bytes(), &[0, 0], &EXPLOSIVES.to_le_bytes(), &[0; 12]].concat();
            sub(b"CTDA", d)
        }

        /// A perk of one entry: entry point, function, number, the tab of the condition.
        fn perk(fid: u32, edid: &str, point: u8, function: u8, value: f32, tab: Option<u8>) -> Vec<u8> {
            let mut s = vec![sub(b"EDID", z(edid)), sub(b"FULL", z(edid)), sub(b"DATA", vec![0, 1, 1, 1, 0]), sub(b"PRKE", vec![2, 0, 0]), sub(b"DATA", vec![point, function, 1])];
            if let Some(tab) = tab {
                s.push(sub(b"PRKC", vec![tab]));
                s.push(in_explosives());
            }
            s.push(sub(b"EPFT", vec![1]));
            s.push(sub(b"EPFD", value.to_le_bytes().to_vec()));
            s.push(sub(b"PRKF", vec![]));
            record(b"PERK", 0, fid, &s)
        }

        fn plugin_bytes() -> Vec<u8> {
            let top = |sig: &[u8; 4], body: &[u8]| group(*sig, 0, body);
            let perks = [
                perk(DEMOLITION, "DemolitionExpert", p::CALCULATE_WEAPON_DAMAGE, p::MULTIPLY_VALUE, 1.2, Some(1)),
                perk(DECK, "HitTheDeck", p::MODIFY_DAMAGE_THRESHOLD_DEFENDER, p::ADD_VALUE, 25.0, Some(2)),
                perk(TOUGHNESS, "Toughness", p::MODIFY_DAMAGE_THRESHOLD_DEFENDER, p::ADD_VALUE, 6.0, None),
            ]
            .concat();
            let npcs = [record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player"))]), record(b"NPC_", 0, NPC, &[sub(b"EDID", z("Grenadier"))])].concat();
            let weapons = [record(b"WEAP", 0, LAUNCHER, &[sub(b"EDID", z("Launcher"))]), record(b"WEAP", 0, RIFLE, &[sub(b"EDID", z("Rifle"))])].concat();
            let list = record(b"FLST", 0, EXPLOSIVES, &[sub(b"EDID", z("Explosives")), sub(b"LNAM", LAUNCHER.to_le_bytes().to_vec())]);
            let cell = record(b"CELL", 0, CELL, &[sub(b"EDID", z("TestCell"))]);
            let achr = record(b"ACHR", 0, NPC_REF, &[sub(b"NAME", NPC.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])]);
            let children = group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &achr));
            let hedr = [1.34f32.to_le_bytes(), 0i32.to_le_bytes(), 0x800u32.to_le_bytes()].concat();
            [
                record(b"TES4", 1, 0, &[sub(b"HEDR", hedr)]),
                top(b"PERK", &perks),
                top(b"NPC_", &npcs),
                top(b"WEAP", &weapons),
                top(b"FLST", &list),
                top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &[cell, children].concat()))),
            ]
            .concat()
        }

        struct Fixture {
            data: GameData,
            forms: FormStore,
            refs: RefTable,
        }

        fn fixture() -> Fixture {
            let plugins = vec![Plugin::parse(&plugin_bytes(), "BlastPerks.esm".into()).unwrap()];
            let lo = LoadOrder::new(&plugins).unwrap();
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
            let schema = Schema::load_dir(root.join("fnv")).unwrap();
            let functions = root.join("script/functions.json");
            let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&functions).unwrap()).unwrap();
            let (forms, refs) = (FormStore::build(&lo, &schema), RefTable::build(&lo, &schema));
            let data = GameData::build(&lo, &schema, fnv_script::FunctionTable::load(&functions).unwrap(), &json, &refs);
            Fixture { data, forms, refs }
        }

        fn game<'a>(f: &'a Fixture, perks: &[u32]) -> Game<'a> {
            let mut g = Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig { strict: true, ..GameConfig::default() });
            g.new_game();
            g.player_entered(Some(FormId(CELL)));
            for &perk in perks {
                g.player.perks.insert(FormId(perk), 1);
            }
            g
        }

        /// A blast of 100 on `target`, in no armour, with the perks its cause and target hold.
        fn blast_on(g: &mut Game<'_>, cause: Option<Cause>, target: FormId) -> damage::BlastHit {
            let (a, t) = blast_entry_ops(g, cause, target);
            damage::blast_hit(&Rules::defaults(), &CombatTuning::default(), 100.0, 0.0, 0.0, &a, &t)
        }

        fn close(a: f32, b: f32) -> bool {
            (a - b).abs() < 1e-3
        }

        #[test]
        fn the_causes_weapon_decides_the_conditioned_entries() {
            let f = fixture();
            let mut g = game(&f, &[DEMOLITION, DECK]);
            let (player, npc) = (FormId::PLAYER_REF, FormId(NPC_REF));
            let mine = |weapon: u32| Some(Cause { actor: player, weapon: FormId(weapon) });
            // The player's grenade on an NPC: the damage perk alone.
            assert!(close(blast_on(&mut g, mine(LAUNCHER), npc).dealt, 120.0));
            // On the player too: 100 x 1.2 - 25, each entry once.
            assert!(close(blast_on(&mut g, mine(LAUNCHER), player).dealt, 95.0));
            // A blast with another weapon behind it (a Meltdown's) answers neither condition.
            assert!(close(blast_on(&mut g, mine(RIFLE), player).dealt, 100.0));
            // An NPC's grenade: Hit the Deck runs on its weapon; the damage perk is the player's.
            assert!(close(blast_on(&mut g, Some(Cause { actor: npc, weapon: FormId(LAUNCHER) }), player).dealt, 75.0));
            // Nobody on either side holds perks.
            let (a, t) = blast_entry_ops(&mut g, Some(Cause { actor: npc, weapon: FormId(LAUNCHER) }), npc);
            assert!(a.is_empty() && t.is_empty());
        }

        /// A placed mine nobody put down: Toughness applies, Hit the Deck has no weapon to
        /// look at, and the player's weapon does not stand in for one.
        #[test]
        fn a_blast_with_no_cause_has_only_the_unconditioned_defence() {
            let f = fixture();
            let mut g = game(&f, &[DEMOLITION, DECK, TOUGHNESS]);
            let (a, t) = blast_entry_ops(&mut g, None, FormId::PLAYER_REF);
            assert!(a.is_empty(), "no attacker's perks");
            assert_eq!(t.names(), ["Toughness"]);
            assert!(close(blast_on(&mut g, None, FormId::PLAYER_REF).dealt, 94.0));
        }

        #[test]
        fn source_critical_damage_changes_the_existing_hit_calculation() {
            let f = fixture();
            let mut g = game(&f, &[]);
            let source = "Begin Function {}\nSetWeaponCritDamage 37 Rifle\nEnd";
            g.run_query(source, Default::default()).unwrap();
            for animation in ["Rifle - Automatic (2 Hand)", "One Hand Sword"] {
                let mut weapon = WeaponStats { base_damage: 20, crit_damage: 11, crit_mult: 1.0, anim_type: Some(animation.into()), ..Default::default() };
                let hit = |weapon: &WeaponStats| damage::hit(&Rules::defaults(), &CombatTuning::default(),
                    &HitInput { weapon, projectiles: 1, condition: 1.0, ammo_effects: &[], part: HitPart::default(), power: 1.0 },
                    &Attacker { sneaking: true, ..Default::default() }, &Target::default(), &NoPerks, &NoPerks, 0.0);
                let baseline = hit(&weapon);
                assert_eq!(baseline.crit_bonus, 11.0);
                script_weapon_critical_damage(Some(&g), FormId(RIFLE), &mut weapon);
                let corrected = hit(&weapon);
                assert_eq!(corrected.crit_bonus, 37.0);
                assert!(close(corrected.dealt - baseline.dealt, 26.0));
                let before = weapon.clone();
                script_weapon_critical_damage(Some(&g), FormId(LAUNCHER), &mut weapon);
                assert_eq!(weapon.crit_damage, before.crit_damage, "another weapon has no override");
            }
        }

        /// A grenade that strikes an actor deals its weapon's hit and its explosion's blast;
        /// the damage perk multiplies each of them once.
        #[test]
        fn a_direct_hit_and_its_blast_are_each_modified_once() {
            let f = fixture();
            let mut g = game(&f, &[DEMOLITION]);
            let (player, npc) = (FormId::PLAYER_REF, FormId(NPC_REF));
            let (launcher, target) = (FormId(LAUNCHER), Some(npc));
            let w = WeaponStats { base_damage: 40, anim_type: Some("Grenade Launcher".into()), ..WeaponStats::default() };
            let h = HitInput { weapon: &w, projectiles: 1, condition: 1.0, ammo_effects: &[], part: HitPart::default(), power: 1.0 };
            let ap = entry_ops(&mut g, player, &[p::CALCULATE_WEAPON_DAMAGE], &[Some(launcher), target], target);
            let direct = damage::hit(&Rules::defaults(), &CombatTuning::default(), &h, &Attacker::default(), &Target::default(), &ap, &NoPerks, 1.0);
            let blasted = blast_on(&mut g, Some(Cause { actor: player, weapon: launcher }), npc);
            assert!(direct.base > 0.0 && close(direct.modified, direct.base * 1.2), "{direct:?}");
            assert!(close(blasted.base, 100.0) && close(blasted.modified, 120.0), "{blasted:?}");
        }

        /// `bridge_player_anim_state` drives `fnv_game::Game::set_anim_state` from live
        /// `Combat` + `WeaponAnim` + `MoveIntent`, firing JIP `SetOnAnimActionEventHandler`
        /// (including literal action 0 Equip) and `SetOnPlayGroupEventHandler` (including
        /// literal group 0 Idle) on transitions without clobbering renderer-driven idles.
        #[test]
        fn player_anim_bridge_fires_jip_animaction_and_playgroup_events() {
            use fnv_combat::attack::{Cycle, Phase, Timing};
            use fnv_game::anim_state::{AnimAction, AnimState};
            use fnv_game::game::RunCtx;

            let f = fixture();
            let mut g = game(&f, &[]);
            let mut xml = String::from("<menu name=\"HUDMainMenu\"><rect name=\"Probe\">");
            for key in ["a0", "a1", "a2", "a9", "a13", "g0", "g3", "g7"] {
                xml.push_str(&format!("<_{key}>0</_{key}>"));
            }
            xml.push_str("</rect></menu>");
            let ui_src = fnv_ui::MemSource::default().with("owned.xml", &xml);
            g.ui_tree.load_menu(&ui_src, "owned.xml").unwrap();
            g.ui_tree.update();
            let q = |g: &mut Game<'_>, src: &str| {
                g.run_query(src, RunCtx::default()).unwrap_or_else(|e| panic!("{src}: {e}"))
            };
            let ui = |g: &Game<'_>, key: &str| g.ui_tree.get_float(&format!("HUDMainMenu/Probe/_{key}")).unwrap();

            // Register Equip (0), Attack (2), Reload (9), ForceScriptAnim (13), Unequip (1),
            // and PlayGroup Idle (0) + Forward (3) + FastForward (7) on PlayerREF.
            q(
                &mut g,
                "ref fnEq\nBegin Function { }\n\
                 let fnEq := (Begin Function {int a}\n\
                   SetUIFloat \"HUDMainMenu/Probe/_a0\" ((GetUIFloat \"HUDMainMenu/Probe/_a0\") + 1)\n\
                 End)\n\
                 SetOnAnimActionEventHandler fnEq 1 PlayerREF 0\n\
                 PlayerREF.AuxVarSetRef \"*fn_eq\" fnEq\nEnd\n",
            );
            q(&mut g, "Begin Function { }\nSetOnAnimActionEventHandler (Begin Function {int a}\nSetUIFloat \"HUDMainMenu/Probe/_a2\" ((GetUIFloat \"HUDMainMenu/Probe/_a2\") + 1)\nEnd) 1 PlayerREF 2\nEnd\n");
            q(&mut g, "Begin Function { }\nSetOnAnimActionEventHandler (Begin Function {int a}\nSetUIFloat \"HUDMainMenu/Probe/_a9\" ((GetUIFloat \"HUDMainMenu/Probe/_a9\") + 1)\nEnd) 1 PlayerREF 9\nEnd\n");
            q(&mut g, "Begin Function { }\nSetOnAnimActionEventHandler (Begin Function {int a}\nSetUIFloat \"HUDMainMenu/Probe/_a13\" ((GetUIFloat \"HUDMainMenu/Probe/_a13\") + 1)\nEnd) 1 PlayerREF 13\nEnd\n");
            q(&mut g, "Begin Function { }\nSetOnAnimActionEventHandler (Begin Function {int a}\nSetUIFloat \"HUDMainMenu/Probe/_a1\" ((GetUIFloat \"HUDMainMenu/Probe/_a1\") + 1)\nEnd) 1 PlayerREF 1\nEnd\n");
            q(&mut g, "Begin Function { }\nSetOnPlayGroupEventHandler (Begin Function {int gr}\nSetUIFloat \"HUDMainMenu/Probe/_g0\" ((GetUIFloat \"HUDMainMenu/Probe/_g0\") + 1)\nEnd) 1 PlayerREF Idle\nEnd\n");
            q(&mut g, "Begin Function { }\nSetOnPlayGroupEventHandler (Begin Function {int gr}\nSetUIFloat \"HUDMainMenu/Probe/_g3\" ((GetUIFloat \"HUDMainMenu/Probe/_g3\") + 1)\nEnd) 1 PlayerREF Forward\nEnd\n");
            q(&mut g, "Begin Function { }\nSetOnPlayGroupEventHandler (Begin Function {int gr}\nSetUIFloat \"HUDMainMenu/Probe/_g7\" ((GetUIFloat \"HUDMainMenu/Probe/_g7\") + 1)\nEnd) 1 PlayerREF FastForward\nEnd\n");

            let timing = Timing { interval: 0.4, fire_delay: 0.05, reload: 1.5, spin_up: 0.0, rounds: None, attach: None };
            let mut cycle = Cycle::new(&WeaponStats::default(), timing, false, 10);
            let mut combat = Combat {
                forms: CombatForms::default(),
                rules: Rules::defaults(),
                tuning: CombatTuning::default(),
                blood_rng: SplitMix(6),
                blood_chance_logged: false,
                weapon: Some(FormId(RIFLE)),
                equipped: Some(Some(FormId(RIFLE))),
                mods: 0,
                silenced_logged: None,
                fists: None,
                ammo: None,
                self_ammo: false,
                melee: false,
                swings: Melee::default(),
                blows: 0,
                boom: Vec::new(),
                meltdowns: HashSet::new(),
                debris: Vec::new(),
                cycle: Some(cycle.clone()),
                kick: Kick::default(),
                flying: Vec::new(),
                dropped_live: Vec::new(),
                settled: Vec::new(),
                landed: Vec::new(),
                fire_landed: Vec::new(),
                splashes: Vec::new(),
                ghosts: Vec::new(),
                looping: None,
                ejects_seen: 0,
                vats_throw: None,
                last_shot: 0.0,
                next_visual: 0,
                pending: Vec::new(),
                script_shots: Vec::new(),
                guarding: HashMap::new(),
                blocked_blows: Vec::new(),
                fresh_reload: false,
                guard_down_until: 0.0,
                marks: Vec::new(),
                rng: SplitMix(1),
                swing_rng: SplitMix(2),
                time: 0.0,
                no_spread: false,
                tracer_hold: None,
                load_magazine: None,
                load_ammo: None,
                crosshair_cone: None,
                player_spread: None,
                grenade_hold: 0.0,
                grenade_held: false,
                vats_cone: None,
                vats_blast: VatsBlast::default(),
                player_blocked_at: None,
                special_swing: None,
                throw_logged: None,
                spread_perks_logged: None,
                flinch_until: 0.0,
                flinch_end: None,
                flinch_rng: SplitMix(3),
                kick_rng: SplitMix(5),
                kick_just_settled: false,
                kick_left: [0.0; 2],
                kick_back_goal: [0.0; 2],
                kick_back: [crate::ease::Spring::default(); 2],
                spent_rng: SplitMix(4),
                chosen_ammo: std::collections::BTreeMap::new(),
                localized_dt_dr: false,
            };
            let dt = 1.0 / 60.0;
            let mut had_weapon = false;
            let idle_intent = MoveIntent { forward: 0.0, right: 0.0, walk: false, ..Default::default() };

            // 1. Drawing weapon -> Equip (0) + Idle (0); steady frame does not re-fire.
            let mut anim = WeaponAnim { drawing: true, ready: false, ..Default::default() };
            bridge_player_anim_state(&mut g, &combat, &anim, false, false, Some(&idle_intent), dt, &mut had_weapon);
            bridge_player_anim_state(&mut g, &combat, &anim, false, false, Some(&idle_intent), dt, &mut had_weapon);
            assert_eq!((ui(&g, "a0"), ui(&g, "g0")), (1.0, 1.0));

            // 2. Ready + WindUp -> Attack (2); walk forward -> Forward (3).
            anim.drawing = false;
            anim.ready = true;
            cycle.phase = Phase::WindUp { left: 0.03 };
            combat.cycle = Some(cycle.clone());
            let walk_fwd = MoveIntent { forward: 1.0, right: 0.0, walk: true, ..Default::default() };
            bridge_player_anim_state(&mut g, &combat, &anim, true, false, Some(&walk_fwd), dt, &mut had_weapon);
            assert_eq!((ui(&g, "a2"), ui(&g, "a0"), ui(&g, "g3")), (1.0, 1.0, 1.0));

            // 3. Reloading -> Reload (9); run forward -> FastForward (7).
            cycle.phase = Phase::Reloading { left: 1.0 };
            combat.cycle = Some(cycle.clone());
            let run_fwd = MoveIntent { forward: 1.0, right: 0.0, walk: false, ..Default::default() };
            bridge_player_anim_state(&mut g, &combat, &anim, false, false, Some(&run_fwd), dt, &mut had_weapon);
            assert_eq!((ui(&g, "a9"), ui(&g, "g7")), (1.0, 1.0));

            // 4. Jammed -> Jam (code 9, IsAnimPlayingEx 4 == 0), then Ready clears weapon action cleanly.
            cycle.phase = Phase::Jammed { left: 0.5 };
            combat.cycle = Some(cycle.clone());
            bridge_player_anim_state(&mut g, &combat, &anim, false, false, Some(&idle_intent), dt, &mut had_weapon);
            assert_eq!((ui(&g, "a9"), ui(&g, "a13"), ui(&g, "g0")), (2.0, 0.0, 2.0));
            assert_eq!(q(&mut g, "Begin Function { }\nSetFunctionValue (PlayerREF.IsAnimPlayingEx 4)\nEnd\n"), fnv_script::vm::XVal::Num(0.0));
            cycle.phase = Phase::Ready;
            combat.cycle = Some(cycle.clone());
            bridge_player_anim_state(&mut g, &combat, &anim, false, false, Some(&idle_intent), dt, &mut had_weapon);
            assert_eq!(g.anim_state(FormId::PLAYER_REF).and_then(|s| s.action), None);

            // 5. Renderer-driven non-weapon action (WaitForSpecialIdle 12) is preserved when
            //    combat owns no action.
            g.set_anim_state(FormId::PLAYER_REF, AnimState { action: Some(AnimAction::WaitForSpecialIdle), group: Some("Idle"), ..Default::default() });
            bridge_player_anim_state(&mut g, &combat, &anim, false, false, Some(&idle_intent), dt, &mut had_weapon);
            assert_eq!(g.anim_state(FormId::PLAYER_REF).and_then(|s| s.action), Some(AnimAction::WaitForSpecialIdle));

            // 6. Holstering -> Unequip (1); removing Equip handler suppresses future Equip (0).
            anim.ready = false;
            anim.holstering = true;
            bridge_player_anim_state(&mut g, &combat, &anim, false, false, Some(&idle_intent), dt, &mut had_weapon);
            assert_eq!(ui(&g, "a1"), 1.0);
            q(&mut g, "ref fnEq\nBegin Function { }\nlet fnEq := PlayerREF.AuxVarGetRef \"*fn_eq\"\nSetOnAnimActionEventHandler fnEq 0 PlayerREF 0\nEnd\n");
            anim.holstering = false;
            anim.drawing = true;
            bridge_player_anim_state(&mut g, &combat, &anim, false, false, Some(&idle_intent), dt, &mut had_weapon);
            assert_eq!(ui(&g, "a0"), 1.0, "removed Equip handler must not fire");
        }
    }

    /// Keep Chosen Ammo tweak (#3305): per-weapon remembered ammo across switching,
    /// unequipping, dropping/picking up, out-of-ammo fallback + return, save/load, and
    /// unchanged behavior when off.
    mod keep_chosen_ammo {
        use super::*;
        use fnv_combat::data::{AmmoStats, ProjectileStats};
        use fnv_sim::forms::{BaseData, BaseForm, Item, ItemKind};

        const PISTOL: FormId = FormId(0x0000_9001);
        const KNIFE: FormId = FormId(0x0000_9002);
        const AMMO_LIST: FormId = FormId(0x0000_9003);
        const AMMO_PLAIN: FormId = FormId(0x0000_9010);
        const AMMO_JHP: FormId = FormId(0x0000_9011);
        const AMMO_AP: FormId = FormId(0x0000_9012);
        const PROJ: FormId = FormId(0x0000_9020);

        fn item(id: FormId, sig: [u8; 4], kind: ItemKind) -> BaseForm {
            BaseForm {
                id,
                sig,
                edid: Some(format!("Edid{:X}", id.0)),
                name: None,
                script: None,
                prompt: None,
                voice_filter: false,
                data: BaseData::Item(Item {
                    kind,
                    value: Some(10),
                    money: None,
                    weight: Some(1.0),
                    slots: None,
                    projectile: None,
                    health: Some(100),
                    non_playable: false,
                    cant_drop: false,
                    armor_class: Default::default(),
                    power_armor: false,
                    armor_dr: None,
                    armor_dt: None,
                    weapon_damage: Some(20.0),
                    weight_mods: [0.0; 3],
                    drawable: kind == ItemKind::Weapon,
                    not_for_npcs: false,
                    thrown: false,
                    quest_item: false,
                    icon: None,
                    placed_count: None,
                    teaches: None,
                    cant_be_taken: false,
                    modulates_voice: false,
                    weapon_traits: None,
                }),
            }
        }

        fn make_sim_and_combat() -> (SimState, Combat) {
            let mut sim = crate::sim::test_world::sim(
                vec![
                    item(PISTOL, *b"WEAP", ItemKind::Weapon),
                    item(KNIFE, *b"WEAP", ItemKind::Weapon),
                    item(AMMO_PLAIN, *b"AMMO", ItemKind::Ammo),
                    item(AMMO_JHP, *b"AMMO", ItemKind::Ammo),
                    item(AMMO_AP, *b"AMMO", ItemKind::Ammo),
                ],
                Vec::new(),
            );
            sim.world.add_item(FormId::PLAYER_REF, PISTOL, 1).unwrap();
            sim.world.add_item(FormId::PLAYER_REF, KNIFE, 1).unwrap();
            sim.world.add_item(FormId::PLAYER_REF, AMMO_PLAIN, 20).unwrap();
            sim.world.add_item(FormId::PLAYER_REF, AMMO_JHP, 15).unwrap();
            sim.world.add_item(FormId::PLAYER_REF, AMMO_AP, 10).unwrap();

            let mut forms = CombatForms::default();
            forms.weapons.insert(
                PISTOL,
                WeaponStats {
                    edid: Some("Weap10mmPistol".into()),
                    clip_size: 12,
                    ammo_use: 1,
                    base_damage: 22,
                    anim_type: Some("One Hand Pistol".into()),
                    ammo: Some(AMMO_LIST),
                    projectile: Some(PROJ),
                    ..Default::default()
                },
            );
            forms.weapons.insert(
                KNIFE,
                WeaponStats {
                    edid: Some("WeapKnife".into()),
                    base_damage: 10,
                    anim_type: Some("One Hand Melee".into()),
                    ..Default::default()
                },
            );
            forms.form_lists.insert(AMMO_LIST, vec![AMMO_PLAIN, AMMO_JHP, AMMO_AP]);
            for (id, name) in [(AMMO_PLAIN, "Ammo10mm"), (AMMO_JHP, "Ammo10mmJHP"), (AMMO_AP, "Ammo10mmAP")] {
                forms.ammo.insert(id, AmmoStats { edid: Some(name.into()), ..Default::default() });
            }
            forms.projectiles.insert(PROJ, ProjectileStats { speed: 1000.0, ..Default::default() });

            let combat = Combat {
                forms,
                rules: Rules::defaults(),
                tuning: CombatTuning::default(),
                blood_rng: SplitMix(6),
                blood_chance_logged: false,
                weapon: None,
                equipped: None,
                mods: 0,
                silenced_logged: None,
                fists: None,
                ammo: None,
                self_ammo: false,
                melee: false,
                swings: Melee::default(),
                blows: 0,
                boom: Vec::new(),
                meltdowns: HashSet::new(),
                debris: Vec::new(),
                cycle: None,
                kick: Kick::default(),
                flying: Vec::new(),
                dropped_live: Vec::new(),
                settled: Vec::new(),
                landed: Vec::new(),
                fire_landed: Vec::new(),
                splashes: Vec::new(),
                ghosts: Vec::new(),
                looping: None,
                ejects_seen: 0,
                vats_throw: None,
                last_shot: 0.0,
                next_visual: 0,
                pending: Vec::new(),
                script_shots: Vec::new(),
                guarding: HashMap::new(),
                blocked_blows: Vec::new(),
                fresh_reload: false,
                guard_down_until: 0.0,
                marks: Vec::new(),
                rng: SplitMix(1),
                swing_rng: SplitMix(2),
                time: 0.0,
                no_spread: false,
                tracer_hold: None,
                load_magazine: None,
                load_ammo: None,
                chosen_ammo: std::collections::BTreeMap::new(),
                crosshair_cone: None,
                player_spread: None,
                grenade_hold: 0.0,
                grenade_held: false,
                vats_cone: None,
                vats_blast: VatsBlast::default(),
                player_blocked_at: None,
                special_swing: None,
                throw_logged: None,
                spread_perks_logged: None,
                flinch_until: 0.0,
                flinch_end: None,
                flinch_rng: SplitMix(3),
                kick_rng: SplitMix(5),
                kick_just_settled: false,
                kick_left: [0.0; 2],
                kick_back_goal: [0.0; 2],
                kick_back: [crate::ease::Spring::default(); 2],
                spent_rng: SplitMix(4),
                localized_dt_dr: false,
            };
            (sim, combat)
        }

        #[test]
        fn keep_chosen_ammo_on_survives_switch_unequip_drop_out_of_stock_and_save_load() {
            let (mut sim, mut combat) = make_sim_and_combat();
            let mut anim = WeaponAnim::default();

            // 1. Initial equip picks default first carried ammo (Plain), then Ammo Swap picks JHP.
            sim.world.equip(FormId::PLAYER_REF, PISTOL).unwrap();
            equip_sync_step(&mut combat, &mut sim, &mut anim, true);
            assert_eq!(combat.player_ammo(), Some(AMMO_PLAIN));
            let swapped = combat.swap_ammo_choice(PISTOL, true, |a| player_count(&sim, Some(a)) > 0);
            assert_eq!(swapped, Some(AMMO_JHP));
            assert_eq!(combat.player_ammo(), Some(AMMO_JHP));
            assert_eq!(combat.chosen_ammo_of(PISTOL), Some(AMMO_JHP));

            // 2. Switching to another weapon and back keeps JHP (and weapon wheel preview sees JHP).
            sim.world.equip(FormId::PLAYER_REF, KNIFE).unwrap();
            equip_sync_step(&mut combat, &mut sim, &mut anim, true);
            assert_eq!(combat.player_weapon(), Some(KNIFE));
            assert_eq!(combat.preferred_ammo_for(PISTOL, true, |a| player_count(&sim, Some(a)) > 0), Some(AMMO_JHP));
            assert_eq!(combat.preferred_ammo_for(PISTOL, false, |a| player_count(&sim, Some(a)) > 0), Some(AMMO_PLAIN));
            sim.world.equip(FormId::PLAYER_REF, PISTOL).unwrap();
            equip_sync_step(&mut combat, &mut sim, &mut anim, true);
            assert_eq!(combat.player_ammo(), Some(AMMO_JHP));

            // 3. Unequipping and re-equipping keeps JHP.
            sim.world.unequip(FormId::PLAYER_REF, PISTOL).unwrap();
            equip_sync_step(&mut combat, &mut sim, &mut anim, true);
            assert_eq!(combat.player_weapon(), None);
            sim.world.equip(FormId::PLAYER_REF, PISTOL).unwrap();
            equip_sync_step(&mut combat, &mut sim, &mut anim, true);
            assert_eq!(combat.player_ammo(), Some(AMMO_JHP));

            // 4. Dropping (removing) and picking up again keeps JHP.
            sim.world.remove_item(FormId::PLAYER_REF, PISTOL, 1).unwrap();
            equip_sync_step(&mut combat, &mut sim, &mut anim, true);
            assert_eq!(combat.player_weapon(), None);
            sim.world.add_item(FormId::PLAYER_REF, PISTOL, 1).unwrap();
            sim.world.equip(FormId::PLAYER_REF, PISTOL).unwrap();
            equip_sync_step(&mut combat, &mut sim, &mut anim, true);
            assert_eq!(combat.player_ammo(), Some(AMMO_JHP));

            // 5. Out of JHP -> falls back to ammo_for (Plain), keeps JHP remembered, and returns
            //    to JHP once carried again.
            sim.world.remove_item(FormId::PLAYER_REF, AMMO_JHP, 15).unwrap();
            sim.world.equip(FormId::PLAYER_REF, KNIFE).unwrap();
            equip_sync_step(&mut combat, &mut sim, &mut anim, true);
            sim.world.equip(FormId::PLAYER_REF, PISTOL).unwrap();
            equip_sync_step(&mut combat, &mut sim, &mut anim, true);
            assert_eq!(combat.player_ammo(), Some(AMMO_PLAIN));
            assert_eq!(combat.chosen_ammo_of(PISTOL), Some(AMMO_JHP));

            sim.world.add_item(FormId::PLAYER_REF, AMMO_JHP, 5).unwrap();
            sim.world.equip(FormId::PLAYER_REF, KNIFE).unwrap();
            equip_sync_step(&mut combat, &mut sim, &mut anim, true);
            sim.world.equip(FormId::PLAYER_REF, PISTOL).unwrap();
            equip_sync_step(&mut combat, &mut sim, &mut anim, true);
            assert_eq!(combat.player_ammo(), Some(AMMO_JHP));

            // 6. Save/load round-trip via EngineSnapshot restores remembered ammo even without
            //    load_ammo; load_ammo (#1574) still wins on the initial weapon-in-hand load;
            //    and an older save with empty chosen_ammo clears stale choices.
            let saved_map = combat.chosen_ammo_for_save();
            assert_eq!(saved_map.get(&PISTOL.0), Some(&AMMO_JHP.0));
            let snap = crate::save::EngineSnapshot {
                format: crate::save::ENGINE_FORMAT.into(),
                chosen_ammo: saved_map,
                ..Default::default()
            };
            let decoded: crate::save::EngineSnapshot = serde_json::from_str(&serde_json::to_string(&snap).unwrap()).unwrap();
            let (mut sim2, mut combat2) = make_sim_and_combat();
            combat2.restore_chosen_ammo(&decoded.chosen_ammo);
            sim2.world.equip(FormId::PLAYER_REF, PISTOL).unwrap();
            equip_sync_step(&mut combat2, &mut sim2, &mut anim, true);
            assert_eq!(combat2.player_ammo(), Some(AMMO_JHP));

            // load_ammo (#1574) precedence on load.
            combat2.restore_magazine((PISTOL.0, 6), Some(AMMO_AP.0));
            equip_sync_step(&mut combat2, &mut sim2, &mut anim, true);
            assert_eq!(combat2.player_ammo(), Some(AMMO_AP));

            // Legacy save with empty chosen_ammo clears remembered picks.
            combat2.restore_chosen_ammo(&std::collections::BTreeMap::new());
            assert_eq!(combat2.chosen_ammo_of(PISTOL), None);
        }

        #[test]
        fn keep_chosen_ammo_off_leaves_equip_and_swap_behavior_unchanged() {
            let (mut sim, mut combat) = make_sim_and_combat();
            let mut anim = WeaponAnim::default();

            sim.world.equip(FormId::PLAYER_REF, PISTOL).unwrap();
            equip_sync_step(&mut combat, &mut sim, &mut anim, false);
            assert_eq!(combat.player_ammo(), Some(AMMO_PLAIN));

            // Swapping with toggle off changes the live ammo but remembers nothing.
            let swapped = combat.swap_ammo_choice(PISTOL, false, |a| player_count(&sim, Some(a)) > 0);
            assert_eq!(swapped, Some(AMMO_JHP));
            assert_eq!(combat.player_ammo(), Some(AMMO_JHP));
            assert_eq!(combat.chosen_ammo_of(PISTOL), None);
            assert!(combat.chosen_ammo_for_save().is_empty());

            // Switching away and back resets to ammo_for (Plain).
            sim.world.equip(FormId::PLAYER_REF, KNIFE).unwrap();
            equip_sync_step(&mut combat, &mut sim, &mut anim, false);
            sim.world.equip(FormId::PLAYER_REF, PISTOL).unwrap();
            equip_sync_step(&mut combat, &mut sim, &mut anim, false);
            assert_eq!(combat.player_ammo(), Some(AMMO_PLAIN));

            // Even if a loaded save had remembered ammo, toggle off ignores it on equip.
            combat.remember_chosen_ammo(PISTOL, AMMO_JHP);
            sim.world.equip(FormId::PLAYER_REF, KNIFE).unwrap();
            equip_sync_step(&mut combat, &mut sim, &mut anim, false);
            sim.world.equip(FormId::PLAYER_REF, PISTOL).unwrap();
            equip_sync_step(&mut combat, &mut sim, &mut anim, false);
            assert_eq!(combat.player_ammo(), Some(AMMO_PLAIN));
        }

        /// #3433: critical-hit corner notifications must use the default corner-message
        /// icon (`icon: None` -> `corner_messages::DEFAULT_ICON`) instead of the unused
        /// `sCriticalStrikeIcon` GMST (`glow_Critical_Chance.dds`).
        #[test]
        fn critical_hit_note_uses_default_corner_icon() {
            assert_eq!(
                critical_hit_note("Giant Soldier Ant", None),
                fnv_game::trace::EventKind::NoteShown {
                    text: "Critical hit on Giant Soldier Ant".into(),
                    icon: None,
                    seconds: CRIT_NOTE_SECONDS,
                }
            );
            assert_eq!(
                critical_hit_note("Easy Pete", Some("torso")),
                fnv_game::trace::EventKind::NoteShown {
                    text: "Critical hit on Easy Pete's torso".into(),
                    icon: None,
                    seconds: CRIT_NOTE_SECONDS,
                }
            );
        }

        #[test]
        fn allow_attack_clears_player_cooldown_and_flinch_without_affecting_vanilla() {
            let (mut sim, mut combat) = make_sim_and_combat();
            let mut anim = WeaponAnim::default();
            sim.world.equip(FormId::PLAYER_REF, PISTOL).unwrap();
            equip_sync_step(&mut combat, &mut sim, &mut anim, false);

            // Put the player in attack cooldown and flinch lock.
            combat.cycle.as_mut().unwrap().phase = Phase::Cooldown { left: 1.2 };
            combat.flinch_until = 5.0;
            anim.swing = Some("attackleft".into());

            let mut app = App::new();
            let script = ScriptState::over_plugins(&[]);
            // Vanilla frame: AllowAttack not called -> cooldown & flinch remain active.
            app.insert_resource(Time::<()>::default())
                .insert_resource(combat)
                .insert_resource(sim)
                .insert_resource(PhysState::default())
                .insert_resource(Player { heading: 0.0, pitch: 0.0, eye_height: 0.0, feet: [0.0; 3], body_eye: None, body_view: None, placed: None })
                .insert_resource(PlayerControl::default())
                .insert_resource(anim)
                .insert_resource(CombatStats::default())
                .insert_resource(script)
                .insert_resource(PlayerLimbs::default())
                .add_message::<ShotFired>()
                .add_message::<PlaySound>()
                .add_message::<CasingFx>()
                .add_message::<LoopSound>()
                .add_message::<ScaledSound>()
                .add_systems(Update, fire);
            app.update();
            assert!(matches!(app.world().resource::<Combat>().cycle.as_ref().unwrap().phase, Phase::Cooldown { .. }), "vanilla cooldown untouched");
            assert_eq!(app.world().resource::<Combat>().flinch_until, 5.0);

            // Script calls PlayerREF.AllowAttack -> next frame clears cooldown, swing and flinch.
            app.world_mut()
                .resource_mut::<ScriptState>()
                .rt
                .as_mut()
                .unwrap()
                .game
                .run_query("Begin Function {}\nPlayerREF.AllowAttack\nEnd", Default::default())
                .unwrap();
            app.update();
            assert_eq!(app.world().resource::<Combat>().cycle.as_ref().unwrap().phase, Phase::Ready);
            assert_eq!(app.world().resource::<Combat>().flinch_until, 0.0);
            assert_eq!(app.world().resource::<WeaponAnim>().swing, None);
            assert!(app.world_mut().resource_mut::<ScriptState>().rt.as_mut().unwrap().game.take_allow_attack().is_empty());
        }

        #[test]
        fn saved_extra_magazine_restores_once_on_equip_and_leaves_vanilla_unchanged() {
            let (mut sim, mut combat) = make_sim_and_combat();
            let mut anim = WeaponAnim::default();

            // Replace plain PISTOL with a unit carrying Extra::magazine = Some(4) (clip_size = 12).
            sim.world.remove_item(FormId::PLAYER_REF, PISTOL, 1).unwrap();
            sim.world
                .add_item_extra(
                    FormId::PLAYER_REF,
                    PISTOL,
                    1,
                    fnv_sim::inventory::Extra { magazine: Some(4), ..Default::default() },
                )
                .unwrap();

            sim.world.equip(FormId::PLAYER_REF, PISTOL).unwrap();
            equip_sync_step(&mut combat, &mut sim, &mut anim, true);
            assert_eq!(combat.magazine_for_save(), Some((PISTOL.0, 4)));
            // Cleared on equip so re-equipping later behaves as vanilla full-clip.
            assert_eq!(
                sim.world.peek(FormId::PLAYER_REF).and_then(|s| s.inventory.as_ref()).and_then(|i| i.equipped_magazine(PISTOL)),
                None
            );
            sim.world.equip(FormId::PLAYER_REF, KNIFE).unwrap();
            equip_sync_step(&mut combat, &mut sim, &mut anim, true);
            sim.world.equip(FormId::PLAYER_REF, PISTOL).unwrap();
            equip_sync_step(&mut combat, &mut sim, &mut anim, true);
            assert_eq!(combat.magazine_for_save(), Some((PISTOL.0, 12)));
        }
    }

    mod localized_dt_dr_tests {
        use super::*;
        use fnv_combat::data::ArmorStats;
        use fnv_sim::forms::{BaseData, BaseForm, Item, ItemKind};

        fn make_combat_with(forms: CombatForms) -> Combat {
            Combat {
                forms,
                rules: Rules::defaults(),
                tuning: CombatTuning::default(),
                blood_rng: SplitMix(6),
                blood_chance_logged: false,
                weapon: None,
                equipped: None,
                mods: 0,
                silenced_logged: None,
                fists: None,
                ammo: None,
                self_ammo: false,
                melee: false,
                swings: Melee::default(),
                blows: 0,
                boom: Vec::new(),
                meltdowns: HashSet::new(),
                debris: Vec::new(),
                cycle: None,
                kick: Kick::default(),
                flying: Vec::new(),
                dropped_live: Vec::new(),
                settled: Vec::new(),
                landed: Vec::new(),
                fire_landed: Vec::new(),
                splashes: Vec::new(),
                ghosts: Vec::new(),
                looping: None,
                ejects_seen: 0,
                vats_throw: None,
                last_shot: 0.0,
                next_visual: 0,
                pending: Vec::new(),
                script_shots: Vec::new(),
                guarding: HashMap::new(),
                blocked_blows: Vec::new(),
                fresh_reload: false,
                guard_down_until: 0.0,
                marks: Vec::new(),
                rng: SplitMix(1),
                swing_rng: SplitMix(2),
                time: 0.0,
                no_spread: false,
                tracer_hold: None,
                load_magazine: None,
                load_ammo: None,
                chosen_ammo: std::collections::BTreeMap::new(),
                crosshair_cone: None,
                player_spread: None,
                grenade_hold: 0.0,
                grenade_held: false,
                vats_cone: None,
                vats_blast: VatsBlast::default(),
                player_blocked_at: None,
                special_swing: None,
                throw_logged: None,
                spread_perks_logged: None,
                flinch_until: 0.0,
                flinch_end: None,
                flinch_rng: SplitMix(3),
                kick_rng: SplitMix(5),
                kick_just_settled: false,
                kick_left: [0.0; 2],
                kick_back_goal: [0.0; 2],
                kick_back: [crate::ease::Spring::default(); 2],
                spent_rng: SplitMix(4),
                localized_dt_dr: false,
            }
        }

        #[test]
        fn localized_dt_dr_defaults_off_separates_zones_preserves_explosions_and_innate_dt_dr() {
            const HELMET: FormId = FormId(0x9001);
            const ARMOR: FormId = FormId(0x9002);
            let make_armor = |id, slots, dt, dr| BaseForm {
                id,
                sig: *b"ARMO",
                edid: None,
                name: None,
                script: None,
                prompt: None,
                voice_filter: false,
                data: BaseData::Item(Item {
                    kind: ItemKind::Armor,
                    value: Some(50),
                    money: None,
                    weight: Some(5.0),
                    slots: Some(slots),
                    projectile: None,
                    health: Some(100),
                    non_playable: false,
                    cant_drop: false,
                    armor_class: Default::default(),
                    power_armor: false,
                    armor_dr: Some(dr),
                    armor_dt: Some(dt),
                    weapon_damage: None,
                    weight_mods: [0.0; 3],
                    drawable: false,
                    not_for_npcs: false,
                    thrown: false,
                    quest_item: false,
                    icon: None,
                    placed_count: None,
                    teaches: None,
                    cant_be_taken: false,
                    modulates_voice: false,
                    weapon_traits: None,
                }),
            };
            let mut sim = crate::sim::test_world::sim(
                vec![
                    make_armor(HELMET, ArmorStats::HEAD_SLOTS, 4.0, 5.0),
                    make_armor(ARMOR, ArmorStats::UPPER_BODY, 14.0, 15.0),
                ],
                Vec::new(),
            );
            sim.world.add_item(FormId::PLAYER_REF, HELMET, 1).unwrap();
            sim.world.add_item(FormId::PLAYER_REF, ARMOR, 1).unwrap();
            sim.world.equip(FormId::PLAYER_REF, HELMET).unwrap();
            sim.world.equip(FormId::PLAYER_REF, ARMOR).unwrap();

            let mut cf = CombatForms::default();
            cf.armor.insert(HELMET, ArmorStats { health: 100, dt: 4.0, dr_raw: 500, slots: ArmorStats::HEAD_SLOTS, ..Default::default() });
            cf.armor.insert(ARMOR, ArmorStats { health: 100, dt: 14.0, dr_raw: 1500, slots: ArmorStats::UPPER_BODY, ..Default::default() });
            let mut combat = make_combat_with(cf);
            let mut script = crate::script::ScriptState::over_no_plugins();

            // 1. Default off (vanilla unchanged): every zone sums full worn armor (DT=18, DR=20).
            assert!(!combat.localized_dt_dr, "must default to off");
            assert_eq!(target_armor_for_zone(&combat, Some(&mut script), &mut sim, FormId::PLAYER_REF, Some(Zone::Head)), (18.0, 20.0));
            assert_eq!(target_armor_for_zone(&combat, Some(&mut script), &mut sim, FormId::PLAYER_REF, Some(Zone::Body)), (18.0, 20.0));
            assert_eq!(target_armor(&combat, Some(&mut script), &mut sim, FormId::PLAYER_REF), (18.0, 20.0));

            // 2. Enabled: Head uses helmet only, Body uses chest only, explosions (All / target_armor) use full sum.
            combat.localized_dt_dr = true;
            assert_eq!(target_armor_for_zone(&combat, Some(&mut script), &mut sim, FormId::PLAYER_REF, Some(Zone::Head)), (4.0, 5.0));
            assert_eq!(target_armor_for_zone(&combat, Some(&mut script), &mut sim, FormId::PLAYER_REF, Some(Zone::Body)), (14.0, 15.0));
            assert_eq!(target_armor_for_zone(&combat, Some(&mut script), &mut sim, FormId::PLAYER_REF, Some(Zone::All)), (18.0, 20.0));
            assert_eq!(target_armor(&combat, Some(&mut script), &mut sim, FormId::PLAYER_REF), (18.0, 20.0));

            // 3. Innate actor DT (AV 76) and DR (AV 18) still apply to every zone.
            script
                .rt
                .as_mut()
                .unwrap()
                .game
                .run_query("Begin Function {}\nPlayerREF.ModAV DamageThreshold 3\nPlayerREF.ModAV DamageResist 10\nEnd", Default::default())
                .unwrap();
            assert_eq!(target_armor_for_zone(&combat, Some(&mut script), &mut sim, FormId::PLAYER_REF, Some(Zone::Head)), (7.0, 15.0));
            assert_eq!(target_armor_for_zone(&combat, Some(&mut script), &mut sim, FormId::PLAYER_REF, Some(Zone::Body)), (17.0, 25.0));
            assert_eq!(target_armor(&combat, Some(&mut script), &mut sim, FormId::PLAYER_REF), (21.0, 30.0));
        }

        #[test]
        fn localized_dt_dr_setting_defaults_off_syncs_and_round_trips_ini() {
            use bevy::app::PostStartup;
            use bevy::ecs::system::RunSystemOnce;
            use bevy::MinimalPlugins;

            let dir = std::env::temp_dir().join(format!("nvf-localized-dt-dr-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("NVForever.ini");

            let make_app = |ini_path: &std::path::Path| {
                let mut app = App::new();
                app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
                    .init_state::<PlayState>()
                    .insert_resource(crate::settings::SettingsFile(Some(ini_path.to_path_buf())))
                    .init_resource::<crate::settings::GivenSwitches>()
                    .add_plugins(CombatPlugin)
                    .add_systems(PostStartup, crate::settings::apply_stored);
                app.world_mut().run_schedule(PostStartup);
                app
            };

            // 1. Fresh start with no INI: defaults to off, within note budget.
            let mut app = make_app(&path);
            assert!(!app.world().resource::<LocalizedDtDr>().0, "must default to off");
            let idx = app.world().resource::<crate::settings::Settings>().index("localized-dt-dr").expect("setting registered");
            let row = &app.world().resource::<crate::settings::Settings>().list()[idx];
            assert_eq!(crate::settings::note_over_budget(row.note), None, "note must fit within 200-char budget");

            // Live sync propagates to Combat when flipped on and writes NVForever.ini.
            app.insert_resource(make_combat_with(CombatForms::default()));
            assert_eq!(crate::settings::flip(app.world_mut(), idx), Some(true));
            app.world_mut().run_system_once(sync_localized_dt_dr).unwrap();
            assert!(app.world().resource::<LocalizedDtDr>().0);
            assert!(app.world().resource::<Combat>().localized_dt_dr);
            let ini = std::fs::read_to_string(&path).unwrap();
            assert!(ini.contains("localized-dt-dr=1"), "persisted to NVForever.ini: {ini}");

            // 2. Next start restores ON from NVForever.ini.
            let mut reloaded = make_app(&path);
            assert!(reloaded.world().resource::<LocalizedDtDr>().0, "restored from NVForever.ini");

            // 3. Flipping back off writes 0 and restores off on next start.
            let idx2 = reloaded.world().resource::<crate::settings::Settings>().index("localized-dt-dr").unwrap();
            assert_eq!(crate::settings::flip(reloaded.world_mut(), idx2), Some(false));
            let ini_off = std::fs::read_to_string(&path).unwrap();
            assert!(ini_off.contains("localized-dt-dr=0"), "off persisted: {ini_off}");
            let reloaded_off = make_app(&path);
            assert!(!reloaded_off.world().resource::<LocalizedDtDr>().0, "off restored from NVForever.ini");

            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// Issue #3753: holding Block with bare fists suppresses taps and holds through the
    /// real player input path (`control::read_controls` -> `combat::melee`) when Power
    /// Attack From a Block is off, resumes normally when Block ends, and preserves both
    /// the opt-in Power Attack From a Block tweak and Scribe Counter.
    mod unarmed_block_attack_tests {
        use super::*;
        use crate::bindings::Bindings;
        use crate::control::read_controls;
        use crate::tweaks::TweakSettings;
        use bevy::input::mouse::AccumulatedMouseScroll;
        use fnv_combat::fx::{FxForms, Impact, Sound, WeaponFx};
        use fnv_esm::raw::{write_subrecords, Subrecord};
        use fnv_game::anim_state::AnimAction;
        use fnv_game::perks::{HAS_UNARMED_COUNTER_ATTACK, SET_VALUE};
        use fnv_game::trace::EventKind;
        use fnv_sim::{SimConfig, World};

        const FISTS: FormId = FormId(0x0000_01F4);
        const CELL_ID: FormId = FormId(0x0000_9100);
        const TARGET_BASE: FormId = FormId(0x0000_9101);
        const TARGET_REF: FormId = FormId(0x0000_9102);
        const SCRIBE_PERK: FormId = FormId(0x0000_9103);
        const ATTACK_TOPIC: FormId = FormId(0x0000_9104);
        const ATTACK_INFO: FormId = FormId(0x0000_9105);
        const HIT_SOUND: FormId = FormId(0x0000_9106);
        const IMPACT_SET: FormId = FormId(0x0000_9110);
        const IMPACT_ID: FormId = FormId(0x0000_9111);

        fn sub(sig: &[u8; 4], data: impl Into<Vec<u8>>) -> Subrecord {
            Subrecord { sig: *sig, data: data.into(), extended: false }
        }

        fn rec(sig: &[u8; 4], id: u32, flags: u32, subs: &[Subrecord]) -> Vec<u8> {
            let body = write_subrecords(subs);
            [&sig[..], &(body.len() as u32).to_le_bytes(), &flags.to_le_bytes(), &id.to_le_bytes(), &[0; 4], &15u16.to_le_bytes(), &[0; 2], &body].concat()
        }

        fn grup(label: [u8; 4], gtype: i32, body: &[u8]) -> Vec<u8> {
            [&b"GRUP"[..], &((24 + body.len()) as u32).to_le_bytes(), &label, &gtype.to_le_bytes(), &[0; 8], body].concat()
        }

        fn build_plugin(with_scribe_counter: bool) -> fnv_esm::Plugin {
            let hedr = [1.34f32.to_le_bytes(), 0u32.to_le_bytes(), 0xA000u32.to_le_bytes()].concat();
            let mut bytes = rec(b"TES4", 0, 1, &[sub(b"HEDR", hedr)]);

            // Bare Fists weapon record.
            let fists = rec(b"WEAP", FISTS.0, 0, &[sub(b"EDID", *b"Fists\0"), sub(b"FULL", *b"Fists\0")]);
            bytes.extend(grup(*b"WEAP", 0, &fists));

            // Target NPC with 100 Health (ACBS base health = 100 at offset 12..14? SetAV in script).
            let npc = rec(b"NPC_", TARGET_BASE.0, 0, &[sub(b"EDID", *b"TestTarget\0"), sub(b"FULL", *b"Test Target\0"), sub(b"ACBS", [0u8; 24])]);
            bytes.extend(grup(*b"NPC_", 0, &npc));

            // Interior cell + persistent ACHR for TARGET_REF at (0, 75, 0).
            let cell_rec = rec(b"CELL", CELL_ID.0, 0, &[sub(b"EDID", *b"TestCell\0"), sub(b"DATA", [1u8])]);
            let mut pos = Vec::with_capacity(24);
            for v in [0.0f32, 75.0, 0.0, 0.0, 0.0, 0.0] {
                pos.extend(v.to_le_bytes());
            }
            let achr = rec(b"ACHR", TARGET_REF.0, 0x0400, &[sub(b"NAME", TARGET_BASE.0.to_le_bytes()), sub(b"DATA", pos)]);
            let cell_children = grup(CELL_ID.0.to_le_bytes(), 6, &grup(CELL_ID.0.to_le_bytes(), 8, &achr));
            let cell_block = grup(0u32.to_le_bytes(), 2, &grup(0u32.to_le_bytes(), 3, &[cell_rec, cell_children].concat()));
            bytes.extend(grup(*b"CELL", 0, &cell_block));

            // Attack combat dialogue topic + INFO so attack_line emits LineStarted on swings.
            let dial = rec(b"DIAL", ATTACK_TOPIC.0, 0, &[sub(b"EDID", *b"Attack\0"), sub(b"DATA", [2u8, 0])]);
            let mut trdt = vec![0u8; 24];
            trdt[12] = 1;
            let info = rec(b"INFO", ATTACK_INFO.0, 0, &[sub(b"DATA", [2u8, 0, 0]), sub(b"TRDT", trdt), sub(b"NAM1", *b"Hah!\0")]);
            let dial_body = [dial, grup(ATTACK_TOPIC.0.to_le_bytes(), 7, &info)].concat();
            bytes.extend(grup(*b"DIAL", 0, &dial_body));

            // Optional Scribe Counter perk (entry point 64 = HAS_UNARMED_COUNTER_ATTACK, SetValue 1.0).
            if with_scribe_counter {
                let perk = rec(
                    b"PERK",
                    SCRIBE_PERK.0,
                    0,
                    &[
                        sub(b"EDID", *b"ScribeCounterTest\0"),
                        sub(b"FULL", *b"Scribe Counter\0"),
                        sub(b"DATA", [0u8, 1, 1, 1, 0]),
                        sub(b"PRKE", [2u8, 0, 0]),
                        sub(b"DATA", [HAS_UNARMED_COUNTER_ATTACK, SET_VALUE, 0]),
                        sub(b"EPFT", [1u8]),
                        sub(b"EPFD", 1.0f32.to_le_bytes()),
                        sub(b"PRKF", []),
                    ],
                );
                bytes.extend(grup(*b"PERK", 0, &perk));
            }
            fnv_esm::Plugin::parse(&bytes, "UnarmedBlockTest.esm".into()).unwrap()
        }

        fn make_app(tweak_on: bool, with_scribe_counter: bool) -> App {
            let plugin = build_plugin(with_scribe_counter);
            let mut script = ScriptState::over_plugins(&[plugin]);
            let (forms, refs) = {
                let g = &script.rt.as_ref().unwrap().game;
                (g.world.forms, g.world.placed)
            };
            let mut sim = SimState { forms, refs, world: World::new(forms, refs, SimConfig::default()) };
            {
                let rt = script.rt.as_mut().unwrap();
                with_world(rt, &mut sim, |rt| {
                    rt.game.world.get(FormId::PLAYER_REF).unwrap().cell = Some(CELL_ID);
                    rt.game.player_entered(Some(CELL_ID));
                    rt.game
                        .run_query(
                            &format!("Begin Function {{}}\nref t\nlet t := StringToRef \"{:08X}\"\nt.SetAV Health 100\nEnd", TARGET_REF.0),
                            fnv_game::game::RunCtx::default(),
                        )
                        .unwrap();
                    if with_scribe_counter {
                        rt.game.player.perks.insert(SCRIBE_PERK, 1);
                    }
                });
            }

            let mut cforms = CombatForms::default();
            cforms.weapons.insert(
                FISTS,
                WeaponStats {
                    edid: Some("Fists".into()),
                    base_damage: 10,
                    reach: 1.0,
                    anim_type: Some("Hand to Hand Melee".into()),
                    ..Default::default()
                },
            );
            let combat = Combat {
                forms: cforms,
                rules: Rules::defaults(),
                tuning: CombatTuning::default(),
                blood_rng: SplitMix(6),
                blood_chance_logged: false,
                weapon: Some(FISTS),
                equipped: Some(None),
                mods: 0,
                silenced_logged: None,
                fists: Some(FISTS),
                ammo: None,
                self_ammo: false,
                melee: true,
                swings: Melee::default(),
                blows: 0,
                boom: Vec::new(),
                meltdowns: HashSet::new(),
                debris: Vec::new(),
                cycle: None,
                kick: Kick::default(),
                flying: Vec::new(),
                dropped_live: Vec::new(),
                settled: Vec::new(),
                landed: Vec::new(),
                fire_landed: Vec::new(),
                splashes: Vec::new(),
                ghosts: Vec::new(),
                looping: None,
                ejects_seen: 0,
                vats_throw: None,
                last_shot: 0.0,
                next_visual: 0,
                pending: Vec::new(),
                script_shots: Vec::new(),
                guarding: HashMap::new(),
                blocked_blows: Vec::new(),
                fresh_reload: false,
                guard_down_until: 0.0,
                marks: Vec::new(),
                rng: SplitMix(1),
                swing_rng: SplitMix(2),
                time: 1.0,
                no_spread: false,
                tracer_hold: None,
                load_magazine: None,
                load_ammo: None,
                chosen_ammo: std::collections::BTreeMap::new(),
                crosshair_cone: None,
                player_spread: None,
                grenade_hold: 0.0,
                grenade_held: false,
                vats_cone: None,
                vats_blast: VatsBlast::default(),
                player_blocked_at: None,
                special_swing: None,
                throw_logged: None,
                spread_perks_logged: None,
                flinch_until: 0.0,
                flinch_end: None,
                flinch_rng: SplitMix(3),
                kick_rng: SplitMix(5),
                kick_just_settled: false,
                kick_left: [0.0; 2],
                kick_back_goal: [0.0; 2],
                kick_back: [crate::ease::Spring::default(); 2],
                spent_rng: SplitMix(4),
                localized_dt_dr: false,
            };

            let mut fx_forms = FxForms::default();
            fx_forms.weapons.insert(FISTS, WeaponFx { impact_dataset: Some(IMPACT_SET), ..WeaponFx::default() });
            fx_forms.impact_sets.insert(IMPACT_SET, [Some(IMPACT_ID); 12]);
            fx_forms.impacts.insert(IMPACT_ID, Impact { sound_1: Some(HIT_SOUND), ..Impact::default() });
            fx_forms.sounds.insert(HIT_SOUND, Sound::default());

            let mut phys = PhysState::default();
            phys.world.set_actor_volume(TARGET_REF.0, crate::hitvol::volume_for([-15.0, 55.0, 0.0], [15.0, 95.0, 100.0], false));
            phys.world.update_queries();

            let control = PlayerControl { weapon_out: true, ..Default::default() };
            let anim = WeaponAnim { ready: true, ..Default::default() };
            let tweaks = TweakSettings { power_attack_if_blocking: tweak_on, ..Default::default() };

            let mut app = App::new();
            app.add_plugins(bevy::state::app::StatesPlugin)
                .insert_state(PlayState::Playing)
                .init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<ButtonInput<MouseButton>>()
                .init_resource::<Bindings>()
                .init_resource::<AccumulatedMouseScroll>()
                .init_resource::<Time>()
                .insert_resource(combat)
                .insert_resource(sim)
                .insert_resource(phys)
                .insert_resource(Player { heading: 0.0, pitch: 0.0, eye_height: 64.0, feet: [0.0; 3], body_eye: None, body_view: None, placed: None })
                .insert_resource(MoveIntent::default())
                .insert_resource(control)
                .insert_resource(anim)
                .insert_resource(CombatStats::default())
                .insert_resource(script)
                .insert_resource(HitFeedback::default())
                .insert_resource(ActorParts::default())
                .insert_resource(PlayerLimbs::default())
                .insert_resource(Sfx::new(fx_forms))
                .insert_resource(crate::dismember::Dismemberment::default())
                .insert_resource(tweaks)
                .add_message::<ActorHit>()
                .add_message::<ActorPush>()
                .add_message::<WeaponBroken>()
                .add_message::<ForcedHitReaction>()
                .add_message::<PlaySound>()
                .add_message::<ImpactFx>()
                .add_message::<Knockback>()
                .add_systems(Update, (read_controls, melee).chain());
            app
        }

        fn step(app: &mut App, dt_secs: f64) -> (Vec<ActorHit>, Vec<PlaySound>, Vec<ImpactFx>) {
            app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f64(dt_secs));
            app.world_mut().resource_mut::<Combat>().time += dt_secs as f32;
            app.update();
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
            let hits = app.world_mut().resource_mut::<Messages<ActorHit>>().drain().collect();
            let sounds = app.world_mut().resource_mut::<Messages<PlaySound>>().drain().collect();
            let impacts = app.world_mut().resource_mut::<Messages<ImpactFx>>().drain().collect();
            (hits, sounds, impacts)
        }

        fn attack_lines(app: &App) -> usize {
            app.world()
                .resource::<ScriptState>()
                .rt
                .as_ref()
                .unwrap()
                .game
                .events
                .iter()
                .filter(|e| matches!(e.kind, EventKind::LineStarted { speaker, info, .. } if speaker == FormId::PLAYER_REF && info == ATTACK_INFO))
                .count()
        }

        #[test]
        fn unarmed_block_suppresses_taps_and_holds_then_allows_attacks_after_release() {
            let mut app = make_app(false, false);

            // Hold Block (right mouse button) until the guard pose is active.
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
            let (h, s, i) = step(&mut app, 0.05);
            assert!(h.is_empty() && s.is_empty() && i.is_empty());
            assert!(app.world().resource::<PlayerControl>().aiming);
            assert_eq!(
                app.world().resource::<Combat>().player_anim_action(app.world().resource::<WeaponAnim>(), false, true, 0.05),
                Some(AnimAction::Block)
            );

            // Tap Attack while holding Block -> no swing, no sound, no hit, no attack line.
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            let (h1, s1, i1) = step(&mut app, 0.05);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
            let (h2, s2, i2) = step(&mut app, 0.05);
            let (h3, s3, i3) = step(&mut app, 0.5);
            assert!(h1.is_empty() && h2.is_empty() && h3.is_empty());
            assert!(s1.is_empty() && s2.is_empty() && s3.is_empty());
            assert!(i1.is_empty() && i2.is_empty() && i3.is_empty());
            assert_eq!(app.world().resource::<CombatStats>().swings, 0);
            assert_eq!(app.world().resource::<WeaponAnim>().attacks, 0);
            assert_eq!(app.world().resource::<WeaponAnim>().swing, None);
            assert_eq!(attack_lines(&app), 0);

            // Hold Attack past power_hold while still holding Block -> still no swing, sound, or hit.
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            for _ in 0..20 {
                let (h, s, i) = step(&mut app, 0.05);
                assert!(h.is_empty() && s.is_empty() && i.is_empty());
            }
            // Drop Block while still holding Attack -> suppressed hold must not fire a phantom attack.
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Right);
            let (h4, s4, i4) = step(&mut app, 0.05);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
            let (h5, s5, i5) = step(&mut app, 0.5);
            assert!(h4.is_empty() && h5.is_empty() && s4.is_empty() && s5.is_empty() && i4.is_empty() && i5.is_empty());
            assert_eq!(app.world().resource::<CombatStats>().swings, 0);
            assert_eq!(app.world().resource::<CombatStats>().power_attacks, 0);
            assert_eq!(app.world().resource::<WeaponAnim>().attacks, 0);
            assert_eq!(attack_lines(&app), 0);

            // Now that Block has ended, a tap starts a normal unarmed swing that lands a hit and sound.
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            step(&mut app, 0.05);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
            let mut total_hits = 0usize;
            let mut total_sounds = 0usize;
            for _ in 0..20 {
                let (h, s, _) = step(&mut app, 0.05);
                total_hits += h.len();
                total_sounds += s.len();
            }
            assert_eq!(app.world().resource::<CombatStats>().swings, 1);
            assert_eq!(app.world().resource::<CombatStats>().power_attacks, 0);
            assert_eq!(app.world().resource::<WeaponAnim>().attacks, 1);
            assert!(app.world().resource::<WeaponAnim>().swing.as_deref().is_some_and(|s| s.starts_with("attackright")));
            assert_eq!(total_hits, 1, "post-block swing must land on target");
            assert!(total_sounds >= 1, "post-block hit must play impact sound");
            assert_eq!(attack_lines(&app), 1, "post-block swing must trigger attack line");
        }

        #[test]
        fn power_attack_from_block_tweak_still_starts_power_attack_while_blocking() {
            let mut app = make_app(true, false);

            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
            step(&mut app, 0.05);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            step(&mut app, 0.05);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);

            assert_eq!(app.world().resource::<CombatStats>().swings, 1);
            assert_eq!(app.world().resource::<CombatStats>().power_attacks, 1);
            assert_eq!(app.world().resource::<WeaponAnim>().swing.as_deref(), Some("attackpower"));
        }

        #[test]
        fn scribe_counter_fires_after_block_ends_and_is_not_consumed_while_blocking() {
            let mut app = make_app(false, true);

            // Hold Block and simulate having just blocked an enemy strike.
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
            step(&mut app, 0.05);
            let blocked_at = app.world().resource::<Combat>().time;
            app.world_mut().resource_mut::<Combat>().player_blocked_at = Some(blocked_at);

            // Tapping Attack while Block is still held does not swing or consume the counter window.
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            step(&mut app, 0.05);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
            step(&mut app, 0.05);
            assert_eq!(app.world().resource::<CombatStats>().swings, 0);
            assert_eq!(app.world().resource::<Combat>().player_blocked_at, Some(blocked_at));

            // Releasing Block and tapping Attack within COUNTER_WINDOW triggers Scribe Counter.
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Right);
            step(&mut app, 0.05);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            step(&mut app, 0.05);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
            step(&mut app, 0.05);

            assert_eq!(app.world().resource::<CombatStats>().swings, 1);
            assert_eq!(app.world().resource::<CombatStats>().power_attacks, 1);
            assert_eq!(app.world().resource::<WeaponAnim>().swing.as_deref(), Some("counter"));
            assert_eq!(app.world().resource::<Combat>().player_blocked_at, None);
        }
    }
}
