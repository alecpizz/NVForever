//! How actors look while they fight (docs/design/combat-ai.md §9): NPCs aim at their
//! target and play an attack clip with every shot and blow; creatures play a bite or
//! swipe with every blow. The decisions are `npc_combat.rs`'s ([`PlayAttack`]); this
//! module finds the clips, and `locomotion::drive` lays them over what the actor plays
//! otherwise ([`CombatPose::layers`]).
//!
//! - **Aiming.** An NPC in a fight with its weapon drawn bends to its target's height:
//!   its stance's level aim clip (`2hraim.kf`, played by `weapons.rs`) mixed with the
//!   up or down form (`2hraimup.kf`, `2hraimdown.kf`) by the pitch from its eyes to the
//!   target's chest, as the player's body does (`fnv_actor::player_clips::aim_weights`,
//!   docs/spec-notes/animation.md §9.6: the forms are the poses at ±90°) [hypothesis, as
//!   there].
//! - **Shots and blows of NPCs.** The weapon's attack clip, chosen as the player's is
//!   (`player_clips::attack_stem` of the WEAP attack animation, then `attack_clips`:
//!   `2hrattack3.kf`, `1hpattackright.kf`, `h2hattackright_a.kf` …), in the skeleton's
//!   folder, with its own up or down form at the aim's weight (§9.6: "an attack plays
//!   with its own up and down forms"). Fists are `h2h`.
//! - **Creatures' blows.** The creature's own `h2hattack….kf` clips in its skeleton's
//!   folder (the dog's `h2hattackleft.kf` and `h2hattackright.kf`, the radscorpion's
//!   `h2hattackleft_mid.kf` …) [observed: the base meshes archive, 2026-10-11], power and
//!   forward attacks left out, taken in turn [design]; a blow that goes on a combo takes
//!   the clip its last clip's `a:` key named first, where the creature has one. A power
//!   attack `npc_combat` asks for ([`Power`]) plays its own clip: the radscorpion's sting
//!   is `h2hattackpower.kf` and `h2hattackforwardpower.kf` [observed: the base meshes
//!   archive, 2026-10-16]. Which power attacks a creature has clips for, and when each
//!   lands, are told to `npc_combat` ([`AttackTimes::powers`]).
//! - **People's power attacks** (docs/spec-notes/combat-rules.md §10.1): for a person
//!   fighting with fists or a melee weapon, its rig's `{prefix}attackpower.kf`,
//!   `…forwardpower`, `…backpower`, `…leftpower` and `…rightpower` clips (`h2h`, `1hm`,
//!   `2hm`) are looked for once per weapon, with their `Hit` key and length
//!   ([`AttackTimes::people_powers`], logged "has N power attacks"); one asked for plays
//!   its own clip, crouched first (`sneak1hmattackpower.kf`).
//! - **Melee combos** (`fnv_combat::melee`, as the player's swings): a blow carries a
//!   [`SwingPick`], the attack group the last swing's `a:` text key named (`a:L` on
//!   `AttackRight` clips, `a:R` on `AttackLeft` ones [observed, `fnv_combat::melee`]) or
//!   none for a run's first swing, which plays the weapon's own attack group; each swing
//!   takes its `_a` or `_b` form by a roll. The started clip's `Hit` key, `a:` key and
//!   length go back to `npc_combat` ([`AttackTimes::swings`]), which decides at the `a:`
//!   key whether to go on (docs/design/combat-ai.md §15).
//! - **When a blow lands**: at the clip's `Hit` text key [documented:
//!   NiTextKeyExtraData; observed: 931 `hit` keys in the clips, animation.md §3.2]; the
//!   key's time is told to `npc_combat` ([`AttackTimes`]), which delays the blow's damage
//!   by it. Shots leave at the start of their clip [design].
//! - **Automatic fire with a looping clip** (`2hhattackspin.kf` of the minigun, flamer and
//!   gatling laser: `Fire` and `Loop` keys, animation.md §9.8): the first shot starts the
//!   clip at its `Fire` key (NPCs do not spin up first [design]), the shots that follow
//!   while it runs do not start it again, and it goes round from `Fire` to `Loop` while
//!   they come no more than [`LOOP_HOLD`] apart, then plays out from where it is, as the
//!   player's body does. Started again at every shot, it jerked the arms and shoulders
//!   back to its start many times a second [observed in NVForever: owner's playtest
//!   2026-10-16, Cook-Cook's flamer].
//! - **Reloads of NPCs** (`npc_combat` asks with [`Move::Reload`] and
//!   [`Move::ReloadRound`]): the weapon's reload clip as the player's body plays it
//!   (`player_clips::reload_clips` of the WEAP reload animation: `1hpreloada.kf`,
//!   crouched first), on the attack layers over the level aim without up or down forms;
//!   a weapon that loads round by round (`1hpreloadxstart.kf` beside `1hpreloadx.kf`:
//!   the .357 revolver, lever-actions) plays its opening clip, then the reload clip from
//!   its start for each round, without a fade in, running on past its `Hit` key to close
//!   (docs/spec-notes/combat-rules.md §4). The clips' lengths and `Hit` key in the
//!   actor's own rig go to `npc_combat` ([`AttackTimes::reloads`]), whose gun cycle
//!   reloads for as long. Their `Sound:` keys play as any clip's (`anim_sounds`).
//! - **Shot timing of NPCs** (#2739): a semi-automatic gun's standing attack clip in the
//!   actor's own rig (`player_clips::attack_clips` of the WEAP attack animation: the
//!   sentry bot's `2hlattackleft.kf`) gives its length and `a:` key with the reload
//!   clips ([`AttackTimes::reloads`]); `npc_combat` times the next shot by them as the
//!   player's (docs/spec-notes/combat-rules.md §4).
//! - While an attack clip plays, the actor's body block keeps the place it had when the
//!   attack began (`nvf_play::blocking::ActorBlocks::attacking`): a dog's lunge shoved
//!   the player back 40 units each bite before [design; owner 2026-10-11].
//! - **Attacks that travel** (docs/spec-notes/actors.md §8.13): a lunge's clip (a forward
//!   power attack) whose accumulation root (`Bip01`) travels (the Deathclaw's `h2hattackforwardpower.kf`
//!   goes 572 units forward and ends 523 forward) carries its actor
//!   (`BoundClip::carried`): the pose keeps the root where its track starts, and the
//!   root's travel of each frame ([`CombatPose::travel`]), turned by the actor's heading
//!   and scaled with it, goes to whoever moves the actor ([`AttackTravel`]; `residents.rs`
//!   moves it with its character, so walls and actors stop it, and it stays where the clip
//!   ends). Before, the clip's travel less its average was left in the pose: the model ran
//!   ahead of its collision and slid back at the end [observed in NVForever: owner
//!   2026-10-30, #1069]. Other blows, shots, reloads and block reactions do not carry
//!   [design: carried, the dogs' standing power attack (187 units) left them biting from
//!   37 units, against the original's median 133 (smoke `combat-dogs`)]. A biter (dog,
//!   coyote) never carries: it bites on the move with its head-only clip and its walk
//!   carries it [design: the walk wins, review of #1209].
//! - **Shots take priority over movement** [design: owner 2026-10-19, "shooting should
//!   always take priority"]: a shot's clip does not fade out at its end but is held at its
//!   last frame at full weight for [`SHOT_HOLD`] while the fight goes on, then fades out,
//!   and a shot that comes while one is shown starts without a fade in. The attack clips'
//!   arms and spine (priority 55) are above the armed run's (`2hafastforward.kf` 36) and
//!   the aim's (35) [observed: kf_pose_compare, 2026-10-19], so the gun stays where the
//!   shot puts it while the legs run. Before, the 9mm SMG's `2haattackloop.kf` (0.17 s)
//!   faded in and out with every shot and never reached full weight, and between shots
//!   the run's arms beat the aim's: a Powder Ganger running and firing showed the plain
//!   armed run [observed in NVForever: owner's playtest 2026-10-19].
//! - An attack clip is laid on blend layers 4 and 5 (the idles' layers: an idle stops for
//!   the attack) over the aim, by track priority (attack clips 55, aim 50,
//!   animation.md §9.6); the aim's up or down form takes layer 3 when the weapon's draw
//!   cycle is not fading there [design].
//! - **Crouched, blocking** (docs/design/combat-ai.md §14): a crouched NPC's (crouched to
//!   fire, or sneaking: `sneaking.rs`, docs/spec-notes/actors.md §9.4) aim, its up
//!   and down forms, its sights and its attack clips are the crouched ones
//!   (`sneak2hraim.kf`, `sneak2hraimis.kf`, `sneak2hrattack3.kf` …), the standing ones
//!   where a stance has none; a blocking NPC holds `{prefix}blockidle.kf` (fists:
//!   `mtblockidle.kf`) in place of the level aim, without its up or down form; a blow it
//!   blocks plays `{prefix}blockhit.kf`, and the attacker whose blow was blocked plays
//!   `{prefix}recoil.kf` [observed: the base meshes archive has them for `1hm`, `2hm`
//!   and the unarmed `mt`/`h2h`].
//! - **kNVSE replacements** (#1461, docs/spec-notes/knvse-anim-folders.md §4): an actor
//!   whose AnimGroupOverride folders or SetActorAnimationPath give it third-person files
//!   (`anim_paths::ActorAnimFiles`) plays the attack, reload, block and aim-form clips
//!   above from the file of the same name where one loads (the later file of two), and
//!   the folder's own clip otherwise; a replaced attack's up and down forms are the
//!   replacement's where it has them [hypothesis: as the idles', `idles::clip_path_for`,
//!   a replacement shares the replaced file's name]. A weapon's timing clips are looked
//!   up once per weapon drawn, so a replacement that comes later is used for its clips
//!   but not its timing [design].

use crate::actors::ActorWorld;
use crate::limbs::ActorRig;
use crate::locomotion::Dying;
use crate::npc_combat::Fighters;
use crate::weapons::WeaponHold;
use bevy::prelude::*;
use fnv_actor::idle_play::DEFAULT_FADE;
use fnv_actor::player_clips::{aim_variant, aim_weights, attack_clips, attack_stem, block_clips, reload_clips, reload_open_clips, Aim, ClipKeys, Stance};
use fnv_actor::weapon::{AnimType, Stage};
use fnv_game::anim_state::{shot_action, swing_action, AnimAction};
use fnv_combat::melee::{swing_stem, NextAttack, PowerDir};
use fnv_sim::FormId;
use nvf_assets::actor::{bind_clip, ActorCtx, ActorParts, PreparedSkeleton};
use nvf_assets::anim_play::{BlendPlayer, BoundClip, Layer};
use nvf_assets::model::ModelCtx;
use nvf_play::combat::Combat;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// The layer an attack clip plays on, and its up or down form on the next.
pub const ATTACK_LAYER: usize = 4;
/// The layer of the aim's up or down form.
pub const AIM_VARIANT_LAYER: usize = 3;
/// Seconds an attack clip fades in and out [design].
const FADE_IN: f32 = 0.06;
const FADE_OUT: f32 = 0.15;
/// The up or down form's weight is capped here (the level clip keeps a share) [design].
const MAX_VARIANT: f32 = 0.95;
/// A blow lands this long into its clip when the clip has no `Hit` key, seconds [design].
pub const DEFAULT_HIT: f32 = 0.3;
/// A looping attack clip goes round its repeated part while the shots come no further
/// apart than this, seconds [engineering choice, as `player_body`'s].
const LOOP_HOLD: f32 = 0.25;
/// A shot's clip is held at its last frame, at full weight, this long after it ends,
/// seconds [design: owner 2026-10-19, "shooting should always take priority"].
const SHOT_HOLD: f32 = 1.0;

/// An actor starts an attack (from `npc_combat`): a shot or a blow, with the weapon it
/// uses (`None`: fists, or a creature's own attack).
#[derive(Message, Clone, Copy, Debug)]
pub struct PlayAttack {
    pub actor: u32,
    pub weapon: Option<FormId>,
    pub melee: bool,
    /// A shot aimed down the sights: the iron-sights attack clip (`2hrattack3is.kf`).
    pub sights: bool,
    /// An attack, or a block's reactions (docs/design/combat-ai.md §14.3).
    pub kind: Move,
    /// A creature's power attack (module notes).
    pub power: Option<Power>,
    /// A melee blow's place in its combo (module notes); `None` for shots and reactions.
    pub swing: Option<SwingPick>,
}

/// A power attack: a creature's clip in the skeleton's folder (`h2h…`), a person's by its
/// weapon's stance prefix, and the attack group an unarmed attack effect may name (EAMT
/// "AttackPower", "AttackForwardPower") [observed: the radscorpion's clips, the `_male`
/// power clips; documented: the groups, schema gameplay.json 92 and 93]. People have all
/// five (docs/spec-notes/combat-rules.md §10.1); creatures' are looked for in the first two.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Power {
    Normal,
    Forward,
    Back,
    Left,
    Right,
}

impl Power {
    /// The ones a creature's clips are looked for.
    pub const CREATURE: [Power; 2] = [Power::Normal, Power::Forward];
    /// All, in the order of a combat style's shares (normal, forward, back, left, right).
    pub const ALL: [Power; 5] = [Power::Normal, Power::Forward, Power::Back, Power::Left, Power::Right];

    /// Its place in a combat style's shares (`CombatStyleDef::power_attack_dirs`).
    pub fn index(self) -> usize {
        Power::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }

    /// The player's direction of the same power attack (its clip stem, damage bonus).
    pub fn dir(self) -> PowerDir {
        PowerDir::ALL[self.index()]
    }

    /// A creature's clip.
    pub fn clip(self) -> String {
        format!("h2h{}.kf", self.dir().stem())
    }

    pub fn group(self) -> &'static str {
        self.dir().group()
    }
}

/// A person's power attack clip found in its rig for the weapon drawn: when it lands
/// (its `Hit` key) and how long it plays, seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PowerClip {
    pub power: Power,
    pub hit: f32,
    pub len: f32,
}

/// Which swing a melee blow plays (module notes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SwingPick {
    /// The attack group the last swing's `a:` key named; `None`: the weapon's own (the
    /// first swing of a run).
    pub next: Option<NextAttack>,
    /// Rolls in 0..1: the `_a` or `_b` form, and the group of a blank `a:` key.
    pub rolls: [f32; 2],
}

/// The clip a melee blow started: seconds into it of its `Hit` key and its `a:` key
/// (with what it names), and its length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwingClip {
    pub hit: f32,
    pub chain: Option<(f32, NextAttack)>,
    pub len: f32,
}

impl SwingClip {
    /// A **creature's** seconds until its next melee decision: the interrupt key, or the
    /// clip's end as well as the base wait without a key [documented: GECK
    /// NiTextKeyExtraData; observed: capture run15b_b2, 2026-11-15, radscorpions]. Creatures
    /// only (the maintainer's decision of 2026-11-18 on #2971): a person keeps the base
    /// timing ([`SwingClip::person_ready_in`]) until people's pace is measured in the
    /// original. The clip's key and length are divided by the blow's attack speed
    /// (`elapsed` and `base_left` are real seconds, the base already divided), in the
    /// manner of the guns' `a:` key (`fnv_combat::attack::Timing::of`, #2739, which also
    /// takes the Animation Mult; melee does not) [documented: GECK wiki Weapons page,
    /// Attack Multiplier "speeds up or slows down just the attack animation"; design,
    /// #2535 review]. The combo window ([`SwingClip::next_at`]) and the swing's end
    /// ([`SwingClip::end_at`]) use the same clock; the `Hit` key stays at the clip's own
    /// time [design].
    pub(crate) fn ready_in(&self, elapsed: f32, base_left: f32, speed: f32) -> f32 {
        let speed = Self::rate(speed);
        match self.chain {
            Some((at, _)) => (at / speed - elapsed).max(0.0),
            None => base_left.max((self.len / speed - elapsed).max(0.0)),
        }
    }

    /// A **person's** seconds until its next melee decision, once its clip is known: the
    /// base wait left (`STRIKE_SECONDS` over the attack speed), brought forward to the
    /// clip's `a:` key when that comes sooner; the key at the clip's own time, never later
    /// than the base, and a clip without a key changes nothing [design; owner request
    /// 2026-10-16; the maintainer's decision of 2026-11-18 on #2971: people keep this until
    /// their pace is measured in the original]. Applied once, when the clip arrives.
    pub(crate) fn person_ready_in(&self, elapsed: f32, base_left: f32) -> f32 {
        self.chain.map_or(base_left, |(at, _)| base_left.min((at - elapsed).max(0.0)))
    }

    /// The clip's end, real seconds into the swing, at attack speed `speed` (a person's
    /// swing passes 1: its clip's own times).
    pub(crate) fn end_at(&self, speed: f32) -> f32 {
        self.len / Self::rate(speed)
    }

    /// What a blow begun `elapsed` real seconds into this swing goes on with at attack
    /// speed `speed`: the group its `a:` key names, between that key and the clip's end,
    /// both divided by the speed (as [`SwingClip::ready_in`]).
    pub(crate) fn next_at(&self, elapsed: f32, speed: f32) -> Option<NextAttack> {
        let speed = Self::rate(speed);
        self.chain.filter(|(at, _)| elapsed >= at / speed && elapsed <= self.len / speed).map(|(_, n)| n)
    }

    /// A speed not above 0 counts as 1 [design].
    fn rate(speed: f32) -> f32 {
        if speed > 0.0 { speed } else { 1.0 }
    }
}

/// What [`PlayAttack`] plays.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Move {
    #[default]
    Attack,
    /// A blow met its block (`1hmblockhit.kf`).
    BlockHit,
    /// Its blow was blocked (`1hmrecoil.kf`).
    Recoil,
    /// A reload: the weapon's reload clip (`1hpreloada.kf`), or the opening clip of one
    /// that loads round by round (`1hpreloadxstart.kf`).
    Reload,
    /// Round by round: the next round, the reload clip again from its start, without a
    /// fade in (it follows the opening clip or the last round).
    ReloadRound,
}

impl Move {
    fn reloads(self) -> bool {
        matches!(self, Move::Reload | Move::ReloadRound)
    }
}

/// When each actor's next blow lands into its attack clip (its `Hit` key), seconds:
/// `npc_combat` delays the blow's damage by it.
#[derive(Resource, Default)]
pub struct AttackTimes {
    pub hit: HashMap<u32, f32>,
    /// The power attacks each fighting creature has clips for, with when each lands
    /// (its clip's `Hit` key), seconds.
    pub powers: HashMap<u32, Vec<(Power, f32)>>,
    /// The power attacks each fighting person has clips for with the melee weapon (or
    /// fists, `None`) drawn on it (docs/spec-notes/combat-rules.md §10.1).
    pub people_powers: HashMap<u32, (Option<FormId>, Vec<PowerClip>)>,
    /// The clip each actor's last melee blow started, powers included (used for a
    /// creature's; a person's power clip is not), until `npc_combat` takes it [design,
    /// #2535].
    pub swings: HashMap<u32, SwingClip>,
    /// Each armed actor's reload clips in its own rig, for the weapon drawn on it: their
    /// length and `Hit` key and the opening clip's length (`fnv_combat::attack::
    /// ClipTiming`), so its gun's cycle reloads for as long as the clip plays; for a
    /// semi-automatic gun also its standing attack clip's length and `a:` key, which time
    /// its shots (#2739).
    pub reloads: HashMap<u32, (FormId, fnv_combat::attack::ClipTiming)>,
}

/// One attack clip playing.
#[derive(Debug, Clone)]
pub struct Attack {
    pub clip: Arc<BoundClip>,
    pub up: Option<Arc<BoundClip>>,
    pub down: Option<Arc<BoundClip>>,
    pub t: f32,
    pub len: f32,
    /// The repeated part, from the `Fire` key to the `Loop` key (seconds into the clip),
    /// for a clip that has one.
    pub repeat: Option<(f32, f32)>,
    /// Seconds since the last shot or blow.
    pub since: f32,
    /// It fades in (not a reload's next round, which follows a clip already playing).
    pub fade_in: bool,
    /// A reload clip: the actor's body block moves with it (an attack's holds still).
    pub reload: bool,
    /// What it plays, and whether it is a blow (melee, a creature's) rather than a shot.
    pub kind: Move,
    /// A reload's opening clip (`…reloadxstart.kf`) of a weapon that loads round by round.
    pub opening: bool,
    pub melee: bool,
    /// Seconds into the clip of its `Hit` key (a blow's strike); shots leave at the start.
    pub hit: f32,
    /// A biter's blow on its head and neck only ([`head_only`]), played when the blow
    /// begins while the creature moves.
    pub on_the_move: Option<Arc<BoundClip>>,
    /// Whether this blow shows head and neck only: decided in its first frame (whether
    /// the creature moved then) and kept for the whole blow, so the pose does not snap
    /// when the creature stops or sets off mid-blow.
    pub head_blow: Option<bool>,
}

/// Whether a creature's bone is of its head or neck (the jaw, tongue, ears, muzzle and
/// eyelids below the head) [observed: the dog skeleton's names in `h2hattackleft.kf`:
/// `Bip01 Neck`…`Neck2`, `Bip01 Head`, `Head Muzzle`, `Head1 Jaw`, ` Bip01 Head11
/// TungeControler`, `Bip01 Ponytail…` (the ears and tongue), `Head2 Eyeleds`].
fn head_or_neck(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["head", "neck", "ponytail", "jaw", "tunge", "tongue"].iter().any(|k| n.contains(k))
}

/// `bound` without the tracks of a creature's body and legs: what of a blow shows over
/// its run while it moves (actors.md §8.10, "The slide") [design]. `None` when the
/// clip keys none of the head or all of the bones are head ones.
pub fn head_only(bound: &BoundClip, names: &[Option<String>]) -> Option<BoundClip> {
    let head = |i: usize| names.get(i).and_then(|n| n.as_deref()).is_some_and(head_or_neck);
    let tracks: Vec<_> = bound.tracks.iter().filter(|(i, _, _)| head(*i)).cloned().collect();
    (!tracks.is_empty() && tracks.len() < bound.tracks.len()).then(|| BoundClip { clip: bound.clip.clone(), tracks, root: None, root_track: None, carried: None, hold_xy: None, unbound: Default::default() })
}

impl Attack {
    /// GetAnimAction's code for it (`fnv_game::anim_state`), `dt` being the step the clip
    /// was advanced by this frame: a reload 9, its opening clip 17 when the weapon loads
    /// round by round (`AnimAction::ReloadOpen`, hypothesis), a blocked blow's recoil 8, a block's
    /// reaction 7, a blow 2 until it strikes and 4 after, a shot 2 on its first frame
    /// (and while an automatic weapon's loop goes round), 3 the frame after and 4 for
    /// the rest of the clip.
    pub fn action(&self, dt: f32) -> Option<AnimAction> {
        match self.kind {
            Move::Reload if self.opening => Some(AnimAction::ReloadOpen),
            Move::Reload | Move::ReloadRound => Some(AnimAction::Reload),
            Move::Recoil => Some(AnimAction::Stagger),
            Move::BlockHit => Some(AnimAction::Block),
            Move::Attack if self.melee => swing_action(self.t, self.hit, self.len),
            Move::Attack => {
                let from = self.repeat.map_or(0.0, |r| r.0);
                let repeating = self.repeat.is_some_and(|(_, to)| self.since <= LOOP_HOLD && self.t < to);
                // The clip was advanced before this is read: its time at the frame's start.
                shot_action((self.t - from - dt).max(0.0), 0.0, self.len - from, dt, repeating)
            }
        }
    }
}

/// A clip's repeated part: from its `Fire` key to its `Loop` key, seconds into it.
fn repeat_of(c: &BoundClip) -> Option<(f32, f32)> {
    let k = ClipKeys::parse(&c.clip.text_keys);
    k.hit.zip(k.loop_end).filter(|(from, to)| from < to).map(|(from, to)| (from - c.clip.start, to - c.clip.start))
}

/// `t` brought back into `from..to` once it passes `to`.
fn loop_time(t: f32, from: f32, to: f32) -> f32 {
    if t < to || from >= to {
        return t;
    }
    from + (t - to).rem_euclid(to - from)
}

/// A shot's clip held at its last frame: the clip, its up and down forms, and the
/// seconds since it ended.
pub type HeldShot = (Arc<BoundClip>, Option<Arc<BoundClip>>, Option<Arc<BoundClip>>, f32);

/// On a fighting actor's root: its aim's up and down forms, the pitch to its target and
/// the attack playing (read by `locomotion::drive`).
#[derive(Component, Debug, Clone, Default)]
pub struct CombatPose {
    pub aim_up: Option<Arc<BoundClip>>,
    pub aim_down: Option<Arc<BoundClip>>,
    /// The iron-sights aim (`2hraimis.kf`) and its up and down forms, for a weapon that
    /// has them; played in place of the level aim while [`CombatPose::sights`].
    pub aim_is: Option<Arc<BoundClip>>,
    pub aim_is_up: Option<Arc<BoundClip>>,
    pub aim_is_down: Option<Arc<BoundClip>>,
    /// Aiming down the sights now (`npc_combat`).
    pub sights: bool,
    /// The crouched aim and its forms, the crouched sights and theirs, and the block
    /// idle (docs/design/combat-ai.md §14), loaded with the aim's forms.
    pub sneak_aim: Option<Arc<BoundClip>>,
    pub sneak_aim_up: Option<Arc<BoundClip>>,
    pub sneak_aim_down: Option<Arc<BoundClip>>,
    pub sneak_aim_is: Option<Arc<BoundClip>>,
    pub sneak_aim_is_up: Option<Arc<BoundClip>>,
    pub sneak_aim_is_down: Option<Arc<BoundClip>>,
    pub block_idle: Option<Arc<BoundClip>>,
    /// Crouched to fire, blocking, now (`npc_combat`).
    pub crouched: bool,
    pub blocking: bool,
    /// Radians, negative looks down.
    pub pitch: f32,
    /// In a fight now (the aim follows the pitch); an attack still playing keeps the
    /// pose after the fight.
    pub fighting: bool,
    pub attack: Option<Attack>,
    /// [design, #2512] The advanced attack/reload visibility sample, including the exact
    /// end on its last frame; consumed by `weapons::reparent` after locomotion.
    pub weapon_visibility: Option<(Arc<BoundClip>, f32)>,
    /// The actor moves this frame (`locomotion::drive` sets it before [`CombatPose::layers`]):
    /// a creature's blow shows on its head and neck only.
    pub moving: bool,
    /// A shot's clip that has played out, held at its last frame (module notes): with
    /// its up and down forms and the seconds since it ended.
    pub held: Option<HeldShot>,
    /// The clip the aim layer showed last frame, and the one it is fading from with the
    /// seconds since the change and the fade's length ([`CombatPose::fade_level`]).
    pub level_last: Option<Arc<BoundClip>>,
    pub level_from: Option<(Arc<BoundClip>, f32, f32)>,
    /// How far the attack's clip has carried the actor since [`carry`] last took it (x,
    /// y in the actor's axes, unscaled; module notes, "Attacks that travel").
    pub travel: Vec2,
    /// The lunge reached where its walk would stop ([`carry`]): the rest of this attack's
    /// travel, its later backward travel too, is not applied.
    pub lunge_stopped: bool,
    /// The forward travel [`carry`] gave last frame (game units), which the fight's
    /// distance to the target ([`Fighters::lunge_room`]) does not count yet.
    pub lunge_last: f32,
}

/// The weight to give the up or down form, played beside its level clip at weight 1,
/// so that it has `share` of the blend (the blend divides by the weights' sum).
fn variant_weight(share: f32) -> f32 {
    let s = share.clamp(0.0, MAX_VARIANT);
    s / (1.0 - s)
}

impl CombatPose {
    /// Cross-fade the aim layer (layer 2) when its clip changes in a fight: the block
    /// idle raised or lowered, the crouched aim taken or left, the sights raised. The old
    /// clip fades out on [`AIM_VARIANT_LAYER`] over the new clip's blend time (its
    /// `Blend: N` key, else 0.2 s), as the player's body fades its clips
    /// (`player_body.rs`, `ClipInfo::fade`). The clip was swapped in one frame before
    /// [observed in NVForever: owner's playtest 2026-10-17, NPC blocking looked
    /// unnaturally fast next to the player's]. The aim's up or down form is left out
    /// while the fade runs [design: it shares that layer]. Returns whether a fade runs.
    /// Out of a fight nothing fades and nothing is remembered: the weapon's own draw
    /// cycle has the layer.
    fn fade_level(&mut self, player: &mut BlendPlayer, dt: f32) -> bool {
        let shown = player.layers[2].as_ref().map(|l| l.clip.clone());
        let changed = match (&self.level_last, &shown) {
            (Some(a), Some(b)) => !Arc::ptr_eq(a, b),
            _ => false,
        };
        if changed {
            let to = shown.as_ref().map_or(DEFAULT_FADE, |c| ClipKeys::parse(&c.clip.text_keys).blend_seconds(DEFAULT_FADE));
            self.level_from = self.level_last.take().filter(|_| to > 0.0).map(|c| (c, 0.0, to));
        }
        self.level_last = shown;
        let Some((from, t, len)) = self.level_from.as_mut() else { return false };
        *t += dt;
        if *t >= *len {
            self.level_from = None;
            return false;
        }
        let Some(l) = player.layers[2].as_mut() else {
            self.level_from = None;
            return false;
        };
        let x = (*t / *len).clamp(0.0, 1.0);
        let w = x * x * (3.0 - 2.0 * x);
        let base = l.weight;
        l.weight = base * w;
        player.layers[AIM_VARIANT_LAYER] = Some(Layer { clip: from.clone(), phase: l.phase, weight: base * (1.0 - w) });
        true
    }
    /// Lay the aim's up or down form and the attack over the actor's layers;
    /// `drawn`: its weapon is drawn (the aim clip plays). Advances the attack by `dt`.
    pub fn layers(&mut self, player: &mut BlendPlayer, drawn: bool, dt: f32) {
        self.weapon_visibility = None;
        let [_, (_, up), (_, down)] = aim_weights(self.pitch);
        let (variant, share) = if up > 0.0 { (Aim::Up, up) } else { (Aim::Down, down) };
        let w = variant_weight(share);
        // Down the sights: the iron-sights aim in place of the level aim clip (layer 2,
        // `weapons.rs`) while the draw cycle is not fading there; crouched, the crouched
        // forms; blocking, the block idle without up or down forms.
        let can = self.fighting && drawn && player.layers[AIM_VARIANT_LAYER].is_none();
        let crouched = self.crouched && !self.blocking;
        let sights = can && self.sights && !self.blocking && (self.aim_is.is_some() || (crouched && self.sneak_aim_is.is_some()));
        let level = if !can {
            None
        } else if self.blocking {
            self.block_idle.clone()
        } else {
            match (crouched, sights) {
                (true, true) => self.sneak_aim_is.clone().or(self.aim_is.clone()),
                (true, false) => self.sneak_aim.clone(),
                (false, true) => self.aim_is.clone(),
                (false, false) => None,
            }
        };
        if let (Some(l), Some(clip)) = (player.layers[2].as_mut(), level) {
            l.clip = clip;
        }
        let fading = if can {
            self.fade_level(player, dt)
        } else {
            (self.level_last, self.level_from) = (None, None);
            false
        };
        if can && !self.blocking && !fading && w > 1e-3 {
            let pick = |sneak: &Option<Arc<BoundClip>>, stand: &Option<Arc<BoundClip>>| if crouched { sneak.clone().or(stand.clone()) } else { stand.clone() };
            let clip = match (variant, sights) {
                (Aim::Up, true) => pick(&self.sneak_aim_is_up, &self.aim_is_up).or(pick(&self.sneak_aim_up, &self.aim_up)),
                (Aim::Up, false) => pick(&self.sneak_aim_up, &self.aim_up),
                (_, true) => pick(&self.sneak_aim_is_down, &self.aim_is_down).or(pick(&self.sneak_aim_down, &self.aim_down)),
                (_, false) => pick(&self.sneak_aim_down, &self.aim_down),
            };
            player.layers[AIM_VARIANT_LAYER] = clip.map(|clip| Layer { clip, phase: 0.0, weight: w });
        }
        let form_of = |up: &Option<Arc<BoundClip>>, down: &Option<Arc<BoundClip>>| if variant == Aim::Up { up.clone() } else { down.clone() };
        let Some(a) = self.attack.as_mut() else {
            // A shot played out: its last frame over the movement while the fight goes
            // on, then faded out (module notes).
            let Some((clip, up, down, since)) = self.held.as_mut() else { return };
            *since += dt;
            let fade = ((SHOT_HOLD + FADE_OUT - *since) / FADE_OUT).min(1.0);
            if fade <= 0.0 || !can {
                self.held = None;
                return;
            }
            player.layers[ATTACK_LAYER] = Some(Layer { clip: clip.clone(), phase: 0.999, weight: fade });
            player.layers[ATTACK_LAYER + 1] = form_of(up, down).filter(|_| w > 1e-3).map(|clip| Layer { clip, phase: 0.999, weight: w * fade });
            return;
        };
        let before = a.t;
        a.t += dt;
        a.since += dt;
        // Its root's travel is the actor's (a carried clip's, `BoundClip::carried`).
        self.travel += a.clip.travel(before, a.t);
        if let Some((from, to)) = a.repeat.filter(|_| a.since <= LOOP_HOLD) {
            a.t = loop_time(a.t, from, to);
        }
        if a.kind == Move::Attack || a.kind.reloads() {
            self.weapon_visibility = Some((a.clip.clone(), a.clip.clip.start + (a.t / a.len.max(1e-3)).clamp(0.0, 1.0) * a.clip.clip.length()));
        }
        let shot = a.kind == Move::Attack && !a.melee;
        if a.t >= a.len {
            self.held = shot.then(|| (a.clip.clone(), a.up.clone(), a.down.clone(), 0.0));
            self.attack = None;
            return self.layers_held(player, w, variant);
        }
        let fade_in = if a.fade_in { ((a.t - a.repeat.map_or(0.0, |r| r.0)).max(0.0) / FADE_IN).min(1.0) } else { 1.0 };
        // A shot does not fade out at its end: it is held there (module notes).
        let fade = fade_in * if shot { 1.0 } else { ((a.len - a.t) / FADE_OUT).min(1.0) };
        let phase = (a.t / a.len.max(1e-3)).min(0.999);
        let moving = self.moving;
        let head = *a.head_blow.get_or_insert(a.on_the_move.is_some() && moving);
        let clip = a.on_the_move.as_ref().filter(|_| head).unwrap_or(&a.clip);
        player.layers[ATTACK_LAYER] = Some(Layer { clip: clip.clone(), phase, weight: fade });
        player.layers[ATTACK_LAYER + 1] = form_of(&a.up, &a.down).filter(|_| w > 1e-3).map(|clip| Layer { clip, phase, weight: w * fade });
    }

    /// The held shot's last frame at full weight, in the frame its clip ended.
    fn layers_held(&mut self, player: &mut BlendPlayer, w: f32, variant: Aim) {
        let Some((clip, up, down, _)) = self.held.as_ref() else { return };
        player.layers[ATTACK_LAYER] = Some(Layer { clip: clip.clone(), phase: 0.999, weight: 1.0 });
        let form = if variant == Aim::Up { up.clone() } else { down.clone() };
        player.layers[ATTACK_LAYER + 1] = form.filter(|_| w > 1e-3).map(|clip| Layer { clip, phase: 0.999, weight: w });
    }
}

pub struct AttackAnimsPlugin;

impl Plugin for AttackAnimsPlugin {
    fn build(&self, app: &mut App) {
        // After the systems that ask for attacks, so a shot's clip starts in the frame it
        // is fired: the summaries at exit then agree (a shot fired in the last frame had
        // no clip yet).
        app.add_message::<PlayAttack>()
            .init_resource::<AttackTimes>()
            .init_resource::<AttackClips>()
            .init_resource::<AttackTravel>()
            .add_systems(Update, (start.after(crate::npc_combat::fight).after(crate::use_weapon::shoot), carry.after(crate::locomotion::drive)))
            .add_systems(PostUpdate, grip_check.after(TransformSystems::Propagate))
            .add_systems(Last, report);
    }
}

/// Bound clips by (skeleton, clip path); `None`: not there. Creature attack clips by
/// skeleton folder, and whose turn is next.
#[derive(Resource, Default)]
struct AttackClips {
    bound: HashMap<(String, String), Option<Arc<BoundClip>>>,
    /// The blows' clips that carry their actor, by the same key ([`AttackClips::carried`]).
    carried: HashMap<(String, String), Arc<BoundClip>>,
    creature: HashMap<String, Vec<String>>,
    /// A body weapon's attack clips by (skeleton folder, stance prefix) ([`body_attack_names`]).
    body: HashMap<(String, String), Vec<String>>,
    turn: HashMap<u32, usize>,
    /// Each actor's kNVSE replacement files (`anim_paths::ActorAnimFiles`): its clips are
    /// taken from them where one has the clip's file name (module notes).
    files: HashMap<u32, Vec<String>>,
    stats: Stats,
}

#[derive(Default)]
struct Stats {
    started: usize,
    shots: usize,
    blows: usize,
    /// Shots that went on with a looping clip already playing.
    kept: usize,
    missing: BTreeMap<String, usize>,
    /// Clips started, by file name (the summary).
    clips: BTreeMap<String, usize>,
    /// Clip paths logged once when first played: by path, not file name, as creatures
    /// share file names (a ghoul's and a deathclaw's `h2hattackforwardpower.kf`) and the
    /// one whose clip happened to start first in a frame hid the other's line.
    logged: std::collections::HashSet<String>,
    /// Blows that went on a combo from the last one's `a:` key.
    chained: usize,    /// Reload clips started (an opening clip and each round count once).
    reloads: usize,
    /// Shots that took over from a shot still shown, without a fade in (module notes).
    followed: usize,
    /// Blows whose clip carries its actor (module notes).
    carried: usize,
}

impl AttackClips {
    /// `clip` (bound from `path` for `skeleton`) carrying its actor
    /// ([`BoundClip::carried`]), kept per clip.
    fn carried(&mut self, skeleton: &str, path: &str, clip: &Arc<BoundClip>) -> Arc<BoundClip> {
        let key = (skeleton.to_string(), fnv_vfs::normalize(path));
        self.carried
            .entry(key)
            .or_insert_with(|| Arc::new(BoundClip { clip: clip.clip.clone(), tracks: clip.tracks.clone(), root: clip.root, root_track: clip.root_track, carried: None, hold_xy: None, unbound: clip.unbound.clone() }.carried()))
            .clone()
    }

    fn bind(&mut self, ctx: &mut ActorCtx, actor: u32, skeleton: &str, skel: &PreparedSkeleton, path: &str) -> Option<Arc<BoundClip>> {
        self.resolve(ctx, actor, skeleton, skel, path).map(|(_, c)| c)
    }

    /// The clip at `path` for `actor` and the path it was read from: a kNVSE replacement
    /// of the same file name where the actor has one that loads (module notes), else the
    /// clip itself.
    fn resolve(&mut self, ctx: &mut ActorCtx, actor: u32, skeleton: &str, skel: &PreparedSkeleton, path: &str) -> Option<(String, Arc<BoundClip>)> {
        if let Some(file) = self.files.get(&actor).and_then(|f| crate::anim_paths::replaced(f, path)) {
            let key = (skeleton.to_string(), fnv_vfs::normalize(&file));
            if let Some(c) = self.bound.entry(key).or_insert_with(|| ctx.clip(&file).map(|c| Arc::new(bind_clip(skel, c)))).clone() {
                return Some((file, c));
            }
        }
        let key = (skeleton.to_string(), fnv_vfs::normalize(path));
        // A creature's clips may be in its `specialanims` folder (Mister Handy and Gutsy,
        // protectrons: `fnv_actor::anims::special_anims_path`).
        self.bound
            .entry(key)
            .or_insert_with(|| ctx.clip(path).or_else(|| fnv_actor::anims::special_anims_path(path).and_then(|p| ctx.clip(&p))).map(|c| Arc::new(bind_clip(skel, c))))
            .clone()
            .map(|c| (path.to_string(), c))
    }

    /// The first of `paths` (file names in `folder`) that loads, and the path it was read
    /// from.
    fn first(&mut self, ctx: &mut ActorCtx, actor: u32, skeleton: &str, skel: &PreparedSkeleton, folder: &str, names: &[String]) -> Option<(String, Arc<BoundClip>)> {
        names.iter().find_map(|n| self.resolve(ctx, actor, skeleton, skel, &format!("{folder}\\{n}")))
    }
}

/// The folder of a skeleton path (`characters\_male\skeleton.nif` → `meshes\characters\_male`).
fn folder_of(skeleton: &str) -> String {
    let s = fnv_vfs::normalize(&fnv_actor::resolve::mesh_path(skeleton));
    s.rfind('\\').map_or(String::new(), |i| s[..i].to_string())
}

/// A semi-automatic gun's attack clip as its shots are timed by (#2739): its length
/// (`None` if empty) and its `a:` key's time into the clip with the group it names, from
/// the clip's length, start and text keys (`AttackTimes::reloads`).
pub fn attack_keys(len: f32, start: f32, text_keys: &[(f32, String)]) -> (Option<f32>, Option<(f32, NextAttack)>) {
    let keys = ClipKeys::parse(text_keys);
    let next = keys.next_attack.and_then(|(t, v)| NextAttack::parse(&v).map(|n| ((t - start).clamp(0.0, len.max(0.0)), n)));
    (Some(len).filter(|l| *l > 0.0), next)
}

impl AttackTimes {
    /// The timing of the gun `weapon` (`ws`) drawn on actor `id` from its clips in its rig,
    /// once [`attack_anims`](self) has looked them up for that weapon: the reload clips,
    /// and for a semi-automatic weapon its attack clip as the player's
    /// (`fnv_combat::attack::npc_clip_timing`), at the teammate attack rate `rate` gives
    /// (asked only once the clips are known). The one handoff `npc_combat`'s fights and
    /// `use_weapon`'s shooters both take (#2739).
    pub fn gun_timing(&self, id: u32, weapon: FormId, ws: &fnv_combat::data::WeaponStats, tuning: &fnv_combat::tuning::CombatTuning, rate: impl FnOnce() -> f32) -> Option<(fnv_combat::attack::ClipTiming, fnv_combat::attack::Timing)> {
        let (_, clips) = self.reloads.get(&id).filter(|r| r.0 == weapon)?;
        let timed = fnv_combat::attack::npc_clip_timing(clips, ws.automatic);
        Some((timed, fnv_combat::attack::Timing::of(ws, &timed, tuning).with_attack_rate(rate())))
    }
}

/// The `Hit` text key's time into a clip, if it has one.
fn hit_time(c: &BoundClip) -> Option<f32> {
    c.clip.text_keys.iter().find(|(_, k)| k.trim().eq_ignore_ascii_case("hit")).map(|(t, _)| (t - c.clip.start).max(0.0))
}

type Rigs<'w> = (Entity, &'w ActorRig, Option<&'w WeaponHold>, Option<&'w mut CombatPose>, Has<Dying>, Option<&'w crate::locomotion::Locomotion>);

/// Start the attacks asked for, and keep each fighting actor's pose (module notes).
#[allow(clippy::too_many_arguments)]
fn start(
    mut commands: Commands,
    mut asked: MessageReader<PlayAttack>,
    fighters: Res<Fighters>,
    combat: Option<Res<Combat>>,
    world: Option<Res<ActorWorld>>,
    mut clips: ResMut<AttackClips>,
    mut times: ResMut<AttackTimes>,
    mut parts: ResMut<ActorParts>,
    mut models: ModelCtx,
    mut rigs: Query<Rigs>,
    blocks: Option<ResMut<nvf_play::blocking::ActorBlocks>>,
    files: Option<Res<crate::anim_paths::ActorAnimFiles>>,
) {
    if let Some(f) = files.filter(|f| f.is_changed()) {
        clips.files.clone_from(&f.0);
    }
    // Who is in an attack now: their body blocks hold still (module notes).
    if let Some(mut b) = blocks {
        let now: std::collections::HashSet<u32> = rigs.iter().filter(|r| r.3.is_some_and(|p| p.attack.as_ref().is_some_and(|a| !a.reload))).map(|r| r.1.reference).collect();
        if b.attacking != now {
            b.attacking = now;
        }
    }
    let asked: Vec<PlayAttack> = asked.read().copied().collect();
    let mut ctx = ActorCtx { cache: &mut parts, models: &mut models };
    let clips = &mut *clips;
    for (e, rig, hold, pose, dying, lo) in &mut rigs {
        let id = rig.reference;
        let pitch = fighters.pitch(id);
        if dying {
            if pose.is_some() {
                commands.entity(e).remove::<CombatPose>();
            }
            continue;
        }
        let mine: Vec<&PlayAttack> = asked.iter().filter(|a| a.actor == id).collect();
        let Some(pitch) = pitch.or(pose.as_ref().filter(|p| p.attack.is_some()).map(|p| p.pitch)) else {
            if pose.is_some() {
                commands.entity(e).remove::<CombatPose>();
            }
            continue;
        };
        let folder = folder_of(&rig.skeleton);
        let skel = rig.skel.clone();
        let weapon_prefix = |w: Option<FormId>| -> &'static str {
            w.and_then(|w| world.as_deref().and_then(|wd| wd.actors.weapons.get(&w))).and_then(|w| w.anim_type.as_deref()).and_then(AnimType::from_name).map_or("h2h", AnimType::prefix)
        };
        let mut new_pose = pose.as_ref().map(|p| (*p).clone()).unwrap_or_default();
        new_pose.pitch = pitch;
        new_pose.fighting = fighters.pitch(id).is_some();
        new_pose.sights = fighters.sights(id);
        // Crouched to fire, or sneaking with its weapon out (`sneaking.rs`): the body is
        // crouched either way, so the aim and attacks are the crouched ones.
        new_pose.crouched = fighters.crouched(id) || lo.is_some_and(|l| l.sneaking && !l.swimming);
        new_pose.blocking = fighters.blocking(id);
        // The aim's up and down forms, once, for an actor with a weapon drawn on it.
        if hold.is_some() && new_pose.aim_up.is_none() && new_pose.aim_down.is_none() {
            let w = fighters.weapon(id);
            let level = format!("{folder}\\{}aim.kf", weapon_prefix(w));
            new_pose.aim_up = clips.bind(&mut ctx, id, &rig.skeleton, &skel, &aim_variant(&level, Aim::Up));
            new_pose.aim_down = clips.bind(&mut ctx, id, &rig.skeleton, &skel, &aim_variant(&level, Aim::Down));
            // The iron-sights aim, where the stance has one (rifles, pistols, automatics).
            let is = format!("{folder}\\{}aimis.kf", weapon_prefix(w));
            new_pose.aim_is = clips.bind(&mut ctx, id, &rig.skeleton, &skel, &is);
            if new_pose.aim_is.is_some() {
                new_pose.aim_is_up = clips.bind(&mut ctx, id, &rig.skeleton, &skel, &aim_variant(&is, Aim::Up));
                new_pose.aim_is_down = clips.bind(&mut ctx, id, &rig.skeleton, &skel, &aim_variant(&is, Aim::Down));
            }
            // Crouched (`sneak2hraim.kf` …) and the block idle (docs/design/combat-ai.md §14).
            let prefix = weapon_prefix(w);
            let sneak = format!("{folder}\\sneak{prefix}aim.kf");
            new_pose.sneak_aim = clips.bind(&mut ctx, id, &rig.skeleton, &skel, &sneak);
            new_pose.sneak_aim_up = clips.bind(&mut ctx, id, &rig.skeleton, &skel, &aim_variant(&sneak, Aim::Up));
            new_pose.sneak_aim_down = clips.bind(&mut ctx, id, &rig.skeleton, &skel, &aim_variant(&sneak, Aim::Down));
            let sneak_is = format!("{folder}\\sneak{prefix}aimis.kf");
            new_pose.sneak_aim_is = clips.bind(&mut ctx, id, &rig.skeleton, &skel, &sneak_is);
            if new_pose.sneak_aim_is.is_some() {
                new_pose.sneak_aim_is_up = clips.bind(&mut ctx, id, &rig.skeleton, &skel, &aim_variant(&sneak_is, Aim::Up));
                new_pose.sneak_aim_is_down = clips.bind(&mut ctx, id, &rig.skeleton, &skel, &aim_variant(&sneak_is, Aim::Down));
            }
            if fnv_actor::player_clips::blocks(prefix) {
                new_pose.block_idle = clips.first(&mut ctx, id, &rig.skeleton, &skel, &folder, &block_clips(Stance::Stand, prefix)).map(|(_, c)| c);
            }
        }
        // The reload clips of the weapon drawn on it, once per weapon (module notes).
        let drawn_gun = fighters.weapon(id).filter(|_| hold.is_some());
        if let Some(w) = drawn_gun.filter(|w| times.reloads.get(&id).is_none_or(|r| r.0 != *w)) {
            let reload = combat.as_deref().and_then(|c| c.forms.weapons.get(&w)).and_then(|ws| ws.reload_anim.clone());
            let prefix = weapon_prefix(Some(w));
            let mut timing = fnv_combat::attack::ClipTiming::default();
            if let Some(r) = reload {
                if let Some((_, c)) = clips.first(&mut ctx, id, &rig.skeleton, &skel, &folder, &reload_clips(Stance::Stand, prefix, &r)) {
                    timing.reload_len = Some(c.clip.length()).filter(|l| *l > 0.0);
                    timing.reload_hit = hit_time(&c);
                }
                timing.reload_open_len = clips.first(&mut ctx, id, &rig.skeleton, &skel, &folder, &reload_open_clips(Stance::Stand, prefix, &r)).map(|(_, c)| c.clip.length());
            }
            // A semi-automatic gun's standing attack clip: its length and `a:` key time
            // the next shot as the player's (`fnv_combat::attack::Timing::of`, #2739).
            let semi = combat.as_deref().and_then(|c| c.forms.weapons.get(&w)).filter(|ws| !ws.automatic);
            if let Some(ws) = semi {
                let stem = attack_stem(ws.attack_anim.as_deref());
                if let Some((_, c)) = clips.first(&mut ctx, id, &rig.skeleton, &skel, &folder, &attack_clips(Stance::Stand, prefix, &stem, false)) {
                    (timing.attack_len, timing.next_attack) = attack_keys(c.clip.length(), c.clip.start, &c.clip.text_keys);
                }
            }
            times.reloads.insert(id, (w, timing));
        }
        // A creature's power attacks, once (module notes).
        let creature_rig = hold.is_none() && rig.skeleton.to_ascii_lowercase().contains("creatures");
        if creature_rig && !times.powers.contains_key(&id) {
            let found: Vec<(Power, f32)> = Power::CREATURE
                .into_iter()
                .filter_map(|p| clips.bind(&mut ctx, id, &rig.skeleton, &skel, &format!("{folder}\\{}", p.clip())).map(|c| (p, hit_time(&c).unwrap_or(DEFAULT_HIT).min(c.clip.length().max(0.05)))))
                .collect();
            times.powers.insert(id, found);
        }
        // A person's power attacks with the melee weapon (or fists) drawn on it, once per
        // weapon (docs/spec-notes/combat-rules.md §10.1).
        let person_rig = !rig.skeleton.to_ascii_lowercase().contains("creatures");
        if person_rig && new_pose.fighting {
            let w = fighters.weapon(id);
            if times.people_powers.get(&id).is_none_or(|(had, _)| *had != w) {
                let prefix = weapon_prefix(w);
                // The weapon's power attack override stands in for the standing one where
                // the rig has its clip (`fnv_combat::melee::MeleeInput::power_override`).
                let over = w.and_then(|w| combat.as_deref()?.forms.weapons.get(&w)?.power_attack_override.clone());
                let found: Vec<PowerClip> = if matches!(prefix, "h2h" | "1hm" | "2hm") {
                    Power::ALL
                        .into_iter()
                        .filter_map(|p| {
                            let custom = over.as_deref().filter(|_| p == Power::Normal).and_then(|o| clips.bind(&mut ctx, id, &rig.skeleton, &skel, &format!("{folder}\\{prefix}{o}.kf")));
                            let c = custom.or_else(|| clips.bind(&mut ctx, id, &rig.skeleton, &skel, &format!("{folder}\\{prefix}{}.kf", p.dir().stem())))?;
                            let len = c.clip.length().max(0.05);
                            Some(PowerClip { power: p, hit: hit_time(&c).unwrap_or(DEFAULT_HIT).min(len), len })
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                if !found.is_empty() {
                    info!(
                        "attack anims: {id:08X} has {} power attacks for {prefix}: {}",
                        found.len(),
                        found.iter().map(|p| format!("{} (hit {:.2} s of {:.2})", p.power.group(), p.hit, p.len)).collect::<Vec<_>>().join(", ")
                    );
                }
                times.people_powers.insert(id, (w, found));
            }
        }
        for a in mine {
            let creature = hold.is_none() && a.weapon.is_none() && rig.skeleton.to_ascii_lowercase().contains("creatures");
            let found = if creature {
                // The creature's own attack clips, in turn.
                let list = clips
                    .creature
                    .entry(folder.clone())
                    .or_insert_with(|| {
                        let pre = format!("{folder}\\h2hattack");
                        ctx.models
                            .data
                            .vfs
                            .paths(|p| p.starts_with(&pre) && p.ends_with(".kf") && !p[folder.len() + 1..].contains('\\') && !p.contains("power") && !p.contains("forward"))
                            .into_iter()
                            .map(|p| p[folder.len() + 1..].to_string())
                            .collect()
                    })
                    .clone();
                // A power attack plays its own clip; the others go in turn.
                let k = clips.turn.entry(id).or_insert(0);
                let n = list.len();
                let mut names: Vec<String> = a.power.map(Power::clip).into_iter().collect();
                names.extend((0..n).map(|i| list[(*k + i) % n.max(1)].clone()));
                if a.power.is_none() {
                    *k = (*k + 1) % n.max(1);
                    if let Some(p) = a.swing.filter(|_| a.kind == Move::Attack) {
                        put_chained_first(&mut names, p);
                    }
                }
                clips.first(&mut ctx, id, &rig.skeleton, &skel, &folder, &names)
            } else {
                let prefix = weapon_prefix(a.weapon);
                let stance = if new_pose.crouched { Stance::Sneak } else { Stance::Stand };
                let names = match a.kind {
                    Move::Attack => {
                        let attack = a.weapon.and_then(|w| combat.as_deref().and_then(|c| c.forms.weapons.get(&w))).and_then(|w| w.attack_anim.clone());
                        let stem = attack_stem(attack.as_deref());
                        match (a.power, a.swing) {
                            // A person's power attack: its own clip, crouched first.
                            (Some(p), _) => {
                                let over = a.weapon.and_then(|w| combat.as_deref()?.forms.weapons.get(&w)?.power_attack_override.clone()).filter(|_| p == Power::Normal);
                                attack_clips(stance, prefix, over.as_deref().unwrap_or(p.dir().stem()), false)
                            }
                            // A swing: the group named (or the weapon's own), its `_a` or `_b` form.
                            (None, Some(p)) => attack_clips(stance, prefix, &swing_stem(&p.next.map_or(stem, |n| n.stem(p.rolls[1])), p.rolls[0]), false),
                            (None, None) => attack_clips(stance, prefix, &stem, a.sights),
                        }
                    }
                    Move::BlockHit => reaction_clips(stance, prefix, "blockhit"),
                    Move::Recoil => reaction_clips(stance, prefix, "recoil"),
                    Move::Reload | Move::ReloadRound => {
                        let reload = a.weapon.and_then(|w| combat.as_deref().and_then(|c| c.forms.weapons.get(&w))).and_then(|w| w.reload_anim.clone());
                        let opens = a.kind == Move::Reload && a.weapon.and_then(|w| times.reloads.get(&id).filter(|r| r.0 == w)).is_some_and(|r| r.1.reload_open_len.is_some());
                        reload.map_or(Vec::new(), |r| if opens { reload_open_clips(stance, prefix, &r).into_iter().chain(reload_clips(stance, prefix, &r)).collect() } else { reload_clips(stance, prefix, &r) })
                    }
                };
                let body = hold.is_some_and(|h| h.body) && a.kind == Move::Attack;
                clips.first(&mut ctx, id, &rig.skeleton, &skel, &folder, &names).or_else(|| {
                    // A creature's body weapon without the people's clip names (Mister
                    // Handy's buzzsaw: `1hmattackright_chop.kf`, `…_stab`, `…_uppercut`):
                    // its stance's attack clips in its folder, in turn (module notes).
                    if !body {
                        return None;
                    }
                    let list = clips.body.entry((folder.clone(), prefix.to_string())).or_insert_with(|| body_attack_names(&ctx.models.data.vfs.paths(|p| p.starts_with(&folder)), &folder, prefix)).clone();
                    let k = clips.turn.entry(id).or_insert(0);
                    let n = list.len().max(1);
                    let names: Vec<String> = (0..list.len()).map(|i| list[(*k + i) % n].clone()).collect();
                    *k = (*k + 1) % n;
                    clips.first(&mut ctx, id, &rig.skeleton, &skel, &folder, &names)
                })
            };
            let Some((path, clip)) = found else {
                *clips.stats.missing.entry(format!("{folder} {}", if creature { "creature attack".to_string() } else { weapon_prefix(a.weapon).to_string() })).or_default() += 1;
                continue;
            };
            // A looping clip still running goes on through this shot (module notes).
            if let Some(cur) = new_pose.attack.as_mut().filter(|c| c.repeat.is_some() && Arc::ptr_eq(&c.clip, &clip) && c.t < c.len) {
                cur.since = 0.0;
                clips.stats.kept += 1;
                continue;
            }
            // A reload has no up or down forms: it plays over the level aim.
            let reloads = a.kind.reloads();
            // A replacement's own up and down forms where it has them, else the folder's.
            let mut form = |aim: Aim| {
                let variant = aim_variant(&path, aim);
                let own = variant.rsplit('\\').next().map(|n| format!("{folder}\\{n}")).filter(|p| *p != variant);
                clips.bind(&mut ctx, id, &rig.skeleton, &skel, &variant).or_else(|| clips.bind(&mut ctx, id, &rig.skeleton, &skel, own.as_deref()?))
            };
            let up = if reloads { None } else { form(Aim::Up) };
            let down = if reloads { None } else { form(Aim::Down) };
            let len = clip.clip.length().max(0.05);
            if a.melee {
                times.hit.insert(id, hit_time(&clip).unwrap_or(DEFAULT_HIT).min(len));
            }
            // A creature's powers need the selected clip's timing too [design, #2535]:
            // its next melee decision uses the interrupt key or end (`NpcSwing::ready_in`).
            // A person's power clip is left unused there (the base timing, #2971).
            if a.melee && a.kind == Move::Attack {
                let keys = ClipKeys::parse(&clip.clip.text_keys);
                let chain = keys.next_attack.and_then(|(t, v)| NextAttack::parse(&v).map(|n| ((t - clip.clip.start).clamp(0.0, len), n)));
                times.swings.insert(id, SwingClip { hit: hit_time(&clip).unwrap_or(DEFAULT_HIT).min(len), chain, len });
                if a.swing.is_some_and(|p| p.next.is_some()) {
                    clips.stats.chained += 1;
                }
            }
            let st = &mut clips.stats;
            if reloads {
                st.reloads += 1;
            } else if a.melee {
                st.started += 1;
                st.blows += 1;
            } else {
                st.started += 1;
                st.shots += 1;
            }
            let name = path.rsplit('\\').next().unwrap_or(&path).to_string();
            // The opening clips are named `<stem>reloadxstart.kf` [observed: base archives].
            let opening = a.kind == Move::Reload && name.to_ascii_lowercase().ends_with("start.kf");
            if st.logged.insert(path.to_ascii_lowercase()) {
                info!("attack anims: {id:08X} plays {path} ({len:.2} s{}){}", hit_time(&clip).map_or(String::new(), |h| format!(", hit at {h:.2} s")), if new_pose.crouched { ", crouched" } else { "" });
            }
            *st.clips.entry(name).or_default() += 1;
            let repeat = repeat_of(&clip).filter(|_| !a.melee && !reloads);
            let t = repeat.map_or(0.0, |(from, _)| from);
            let hit = if a.melee { hit_time(&clip).unwrap_or(DEFAULT_HIT).min(len) } else { 0.0 };
            // A shot that follows a shot still shown at full weight takes over without a
            // fade in, so the arms stay on the shots (module notes).
            let shot = a.kind == Move::Attack && !a.melee;
            let follows = shot && (new_pose.held.take().is_some() || new_pose.attack.as_ref().is_some_and(|c| c.kind == Move::Attack && !c.melee));
            new_pose.held = None;
            clips.stats.followed += usize::from(follows);
            let fade_in = a.kind != Move::ReloadRound && !follows;
            // Biters only: the creatures with an `h2h` movement set (dogs and coyotes; the
            // cazador's and mirelurk king's blows are stings and claws, which the rule
            // leaves whole, as every other creature's) [design].
            let biter = creature && lo.is_some_and(|l| l.alert.is_some()) && !rig.skeleton.to_ascii_lowercase().contains("cazador") && !rig.skeleton.to_ascii_lowercase().contains("mirelurk");
            // A lunge's clip (a forward power attack) whose root travels carries its
            // actor (module notes); a biter bites on the move instead, its walk carrying
            // it [design: the walk wins for dogs and coyotes].
            let clip = if !biter && a.melee && a.kind == Move::Attack && a.power == Some(Power::Forward) { clips.carried(&rig.skeleton, &path, &clip) } else { clip };
            if let Some(far) = clip.carried.map(|_| clip.travel(0.0, len).length()) {
                clips.stats.carried += 1;
                if clips.stats.logged.insert(format!("carries {}", path.to_ascii_lowercase())) {
                    info!("attack anims: {path} carries its actor {far:.0} units (its root's travel)");
                }
            }
            let on_the_move = if biter && a.melee { head_only(&clip, &rig.skel.names).map(Arc::new) } else { None };
            new_pose.attack = Some(Attack { clip, up, down, t, len, repeat, since: 0.0, fade_in, reload: reloads, kind: a.kind, opening, melee: a.melee, hit, on_the_move, head_blow: None });
            new_pose.lunge_stopped = false;
            new_pose.lunge_last = 0.0;
        }
        match pose {
            Some(mut p) => *p = new_pose,
            None => {
                commands.entity(e).insert(new_pose);
            }
        }
    }
}

/// A block's reaction clips (`kind` `blockhit` or `recoil`) for weapon `prefix`: crouched
/// first when crouched, then the stance's, then the unarmed ones (`mtblockhit.kf`,
/// `h2hrecoil.kf`) [observed: the base meshes archive lists `1hmblockhit`, `2hmblockhit`,
/// `mtblockhit`, `sneak1hmblockhit`, `sneak2hmblockhit`, `1hmrecoil`, `2hmrecoil`,
/// `h2hrecoil`].
pub fn reaction_clips(stance: Stance, prefix: &str, kind: &str) -> Vec<String> {
    let mut out = Vec::new();
    let own = if prefix == "h2h" && kind == "blockhit" { "mt" } else { prefix };
    if stance == Stance::Sneak {
        out.push(format!("sneak{own}{kind}.kf"));
    }
    out.push(format!("{own}{kind}.kf"));
    for other in ["mt", "h2h"] {
        let name = format!("{other}{kind}.kf");
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// The attack clips of stance `prefix` among `paths` (archive paths) in a creature's
/// skeleton `folder` or its `specialanims` folder, as file names: `<prefix>attack….kf`
/// without the up and down forms (`2hlattackspinup.kf`), the iron-sights forms and the
/// power attacks [design: as the creatures' own attacks, module notes]. Mister Handy's
/// `1hm`: `1hmattackleft_chop.kf`, `1hmattackright_chop.kf`, `1hmattackright_stab.kf`,
/// `1hmattackright_uppercut.kf` [observed: `Fallout - Meshes.bsa`, 2026-10-24].
pub fn body_attack_names(paths: &[String], folder: &str, prefix: &str) -> Vec<String> {
    let stem = format!("{prefix}attack");
    let mut out: Vec<String> = paths
        .iter()
        .filter_map(|p| {
            let rest = p.strip_prefix(folder)?.strip_prefix('\\')?;
            let name = rest.strip_prefix("specialanims\\").unwrap_or(rest);
            (!name.contains('\\') && name.starts_with(&stem) && name.ends_with(".kf")).then_some(name)
        })
        .filter(|n| !n.ends_with("up.kf") && !n.ends_with("down.kf") && !n.ends_with("is.kf") && !n.contains("power"))
        .map(str::to_string)
        .collect();
    out.sort();
    out.dedup();
    out
}

/// A creature's attack clips (file names, in turn) with those of the group a combo goes
/// on with first (`h2hattackleft.kf`, `h2hattackleft_mid.kf` for [`NextAttack::Left`]).
fn put_chained_first(names: &mut [String], p: SwingPick) {
    let Some(n) = p.next else { return };
    let want = format!("h2h{}", n.stem(p.rolls[1]));
    names.sort_by_key(|c| !c.strip_prefix(&want).is_some_and(|rest| rest.starts_with(['.', '_'])));
}

/// Whether a weapon hold has its weapon drawn (the aim clip playing).
/// How far their attack clips carry the actors (module notes, "Attacks that travel"):
/// filled by [`carry`] each frame, taken by whoever moves the actors (`residents.rs`).
#[derive(Resource, Default)]
pub struct AttackTravel {
    /// Game-space travel (x, y, game units) since it was last taken, by reference; an
    /// actor whose attack clip carries it is listed every frame it plays, also with
    /// none, so its other movement waits for the clip.
    pub moves: HashMap<u32, [f32; 2]>,
    /// The travel given in all (clip and scale), game units.
    pub units: f32,
}

impl AttackTravel {
    pub fn take(&mut self) -> HashMap<u32, [f32; 2]> {
        std::mem::take(&mut self.moves)
    }
}

/// Each fighter's root travel of this frame ([`CombatPose::travel`], in its axes) turned
/// to game space by its heading and scaled with it, for [`AttackTravel`]. The actor's
/// root is in game space: x east, y north, z up (`locomotion::heading_of_rotation`).
fn carry(mut travel: ResMut<AttackTravel>, fighters: Res<Fighters>, mut poses: Query<(&ActorRig, &mut CombatPose, &Transform)>) {
    for (rig, mut pose, tr) in &mut poses {
        let asked = std::mem::take(&mut pose.travel);
        let k = tr.scale.x.max(1e-3);
        let room = fighters.lunge_room(rig.reference);
        let (was, last) = (pose.lunge_stopped, pose.lunge_last);
        let p = &mut *pose;
        let d = clamp_lunge(asked, room, k, &mut p.lunge_stopped, &mut p.lunge_last);
        if pose.lunge_stopped && !was {
            info!(
                "attack anims: {:08X}'s lunge stopped at its target's body ({:.0} units of room, {last:.0} carried the frame before, {:.1} asked)",
                rig.reference,
                room.unwrap_or(0.0),
                asked.y * k
            );
        }
        let carrying = pose.attack.as_ref().is_some_and(|a| a.clip.carried.is_some());
        if !carrying && d == Vec2::ZERO {
            continue;
        }
        let (s, c) = crate::locomotion::heading_of_rotation(tr.rotation).sin_cos();
        // Forward is (sin, cos) of the heading, right is (cos, -sin).
        let w = [(d.x * c + d.y * s) * k, (-d.x * s + d.y * c) * k];
        travel.units += w[0].hypot(w[1]);
        let m = travel.moves.entry(rig.reference).or_default();
        m[0] += w[0];
        m[1] += w[1];
    }
}

/// This frame's root travel `d` of a carried clip (the actor's axes, unscaled) as much as
/// may carry the actor, its scale `k`: a lunge carries its actor no nearer its target
/// than `room` (`Fighters::lunge_room`: where their bodies touch) less the forward travel
/// given the frame before (`last`, which the fight's distance does not count yet). Once
/// it gets there it is `stopped`, and the rest of that attack carries nothing, the clip's
/// later travel back included (the Deathclaw's lunge goes 572 forward, then back to 523)
/// [design]. Without a room (no target) the clip's travel carries in full.
fn clamp_lunge(mut d: Vec2, room: Option<f32>, k: f32, stopped: &mut bool, last: &mut f32) -> Vec2 {
    if *stopped {
        d = Vec2::ZERO;
    } else if let Some(room) = room.filter(|_| d.y > 0.0) {
        let left = (room - *last).max(0.0) / k;
        if d.y >= left {
            d.y = left;
            *stopped = true;
        }
    }
    *last = d.y.max(0.0) * k;
    d
}

/// How often [`grip_check`] logs an actor while it shoots (seconds).
const GRIP_LOG_EVERY: f32 = 0.5;

/// While a shot's clip plays with the weapon in the hand, every [`GRIP_LOG_EVERY`]
/// seconds: how far the right hand is from the `Weapon` node, and the left hand from
/// where the stance's equip clip puts it on the weapon (`weapons::Stance::grip`), as
/// shown (the propagated transforms). A flamer held below the hands (#1309) or turned in
/// the hand (#322) moves the left hand off its grip. Reloads move the left hand to the
/// ammunition and are left out.
fn grip_check(time: Res<Time>, mut last: Local<HashMap<u32, f32>>, rigs: Query<(&ActorRig, &WeaponHold, &CombatPose)>, nodes: Query<&GlobalTransform>) {
    let now = time.elapsed_secs();
    for (rig, hold, pose) in &rigs {
        let Some(a) = pose.attack.as_ref().filter(|a| !a.reload && !a.melee && a.t > 0.1) else { continue };
        let st = &hold.stance;
        let crouched = pose.crouched || hold.crouched;
        let (Some(grip), Some(&right), Some(&left)) = (st.grip_for(crouched), st.bones.get(&st.drawn), rig.skel.by_name.get(crate::weapons::LEFT_HAND)) else { continue };
        if !hold.in_hand() || last.get(&rig.reference).is_some_and(|&t| now - t < GRIP_LOG_EVERY) {
            continue;
        }
        let at = |i: usize| rig.nodes.get(i).and_then(|&e| nodes.get(e).ok()).map(|g| g.affine());
        let (Some(w), Some(r), Some(l)) = (at(st.weapon_node), at(right), at(left)) else { continue };
        // In the skeleton's units: the actor's scale divided out.
        let scale = w.matrix3.x_axis.length().max(1e-6);
        let r_off = w.translation.distance(r.translation) / scale;
        let l_off = w.inverse().transform_point3a(l.translation).distance(grip.into());
        last.insert(rig.reference, now);
        info!("weapon grip: {:08X} {} at {:.2} s{}: right hand {r_off:.1} units from the Weapon node, left hand {l_off:.1} units from its grip", rig.reference, a.clip.clip.name, a.t, if crouched { ", crouched" } else { "" });
    }
}

pub fn drawn(hold: Option<&WeaponHold>) -> bool {
    hold.is_some_and(|h| h.anim.stage == Stage::Drawn)
}

fn report(mut exit: MessageReader<AppExit>, clips: Res<AttackClips>, travel: Res<AttackTravel>) {
    if exit.read().next().is_none() {
        return;
    }
    let s = &clips.stats;
    let list = |m: &BTreeMap<String, usize>| if m.is_empty() { "none".to_string() } else { m.iter().map(|(k, n)| format!("{k} {n}")).collect::<Vec<_>>().join(", ") };
    info!(
        "attack anims summary: {} attack clips started ({} shots, {} blows, {} blows went on a combo, {} reload clips), {} shots went on with a looping clip, {} shots followed a shot still shown: {}; without a clip: {}",
        s.started,
        s.shots,
        s.blows,
        s.chained,
        s.reloads,
        s.kept,
        s.followed,
        list(&s.clips),
        list(&s.missing)
    );
    info!("attack anims: {} blows carried their actor with their clip's travel, {:.0} units of travel in all", s.carried, travel.units);
}

#[cfg(test)]
mod tests {
    /// #2739: the production handoff from an attack clip's keys, as the lookup stores them
    /// in `AttackTimes::reloads`, to the timing an NPC's (or a Use Weapon shooter's) gun
    /// cycle runs on (`AttackTimes::gun_timing`). The keys are the test's own, laid out as
    /// the sentry bot's `2hlattackleft.kf` (`a:R` at 0.967 s, end 2.0 s) and a pistol clip.
    #[test]
    fn stored_attack_clips_time_an_npcs_semi_automatic_gun() {
        use super::{attack_keys, AttackTimes};
        use fnv_combat::attack::ClipTiming;
        use fnv_combat::data::WeaponStats;
        use fnv_combat::tuning::CombatTuning;
        use fnv_sim::FormId;
        let t = CombatTuning::default();
        let keys = |list: &[(f32, &str)]| list.iter().map(|(t, k)| (*t, k.to_string())).collect::<Vec<_>>();
        let stored = |len: f32, k: &[(f32, &str)]| {
            let (attack_len, next_attack) = attack_keys(len, 0.0, &keys(k));
            ClipTiming { attack_len, next_attack, ..ClipTiming::default() }
        };
        let (me, launcher_id, pistol_id, minigun_id) = (7u32, FormId(0x0005_7E8F), FormId(0x10), FormId(0x11));
        let mut times = AttackTimes::default();
        // The launcher (an embedded semi-automatic weapon): its a: key, not 1 / 1.5789.
        let launcher = WeaponStats { embedded: true, clip_size: 1, attack_shots_per_sec: 1.578_947_4, attack_mult: 1.0, anim_mult: 1.0, ..WeaponStats::default() };
        times.reloads.insert(me, (launcher_id, stored(2.0, &[(0.0, "start"), (0.033, "BlendIn:1"), (0.067, "Hit"), (0.967, "a:R"), (2.0, "end")])));
        let (_, timing) = times.gun_timing(me, launcher_id, &launcher, &t, || 1.0).expect("stored");
        assert!((timing.interval - 0.967).abs() < 1e-5 && timing.fire_delay == 0.0, "{timing:?}");
        // Stored for another weapon: none yet (the fight keeps 1 / Attack Shots/Sec).
        assert!(times.gun_timing(me, pistol_id, &launcher, &t, || 1.0).is_none());
        // A person's semi-automatic pistol: a: at 0.4 s ÷ speed 1.25 = 0.32 s; without an
        // a: key, its clip's end (0.5 ÷ 1.25 = 0.4 s); at a teammate rate of 2, half.
        let pistol = WeaponStats { clip_size: 13, attack_shots_per_sec: 3.125, attack_mult: 1.25, anim_mult: 1.0, ..WeaponStats::default() };
        times.reloads.insert(me, (pistol_id, stored(0.5, &[(0.0, "start"), (0.4, "a:R"), (0.5, "end")])));
        let (_, timing) = times.gun_timing(me, pistol_id, &pistol, &t, || 1.0).expect("stored");
        assert!((timing.interval - 0.32).abs() < 1e-5, "{timing:?}");
        let (_, fast) = times.gun_timing(me, pistol_id, &pistol, &t, || 2.0).expect("stored");
        assert!((fast.interval - 0.16).abs() < 1e-5, "{fast:?}");
        times.reloads.insert(me, (pistol_id, stored(0.5, &[(0.0, "start"), (0.5, "end")])));
        let (_, timing) = times.gun_timing(me, pistol_id, &pistol, &t, || 1.0).expect("stored");
        assert!((timing.interval - 0.4).abs() < 1e-5, "{timing:?}");
        // An automatic weapon: its Fire Rate, whatever is stored.
        let minigun = WeaponStats { automatic: true, fire_rate: 15.0, clip_size: 255, attack_mult: 1.0, anim_mult: 1.0, ..WeaponStats::default() };
        times.reloads.insert(me, (minigun_id, stored(2.0, &[(0.967, "a:R")])));
        let (timed, timing) = times.gun_timing(me, minigun_id, &minigun, &t, || 1.0).expect("stored");
        assert!(timed.attack_len.is_none() && (timing.interval - 1.0 / 15.0).abs() < 1e-5, "{timing:?}");
    }

    /// A lunge with room carries the actor by its clip's whole root travel, frame by
    /// frame; one that reaches its target's body stops there for the rest of the attack,
    /// its travel back included (`clamp_lunge`, actors.md §8.13).
    #[test]
    fn a_lunge_with_room_carries_its_clips_travel_and_stops_at_the_body() {
        use super::clamp_lunge;
        use bevy::math::Vec2;
        // The Deathclaw's clip, in frames: 572 forward, then 49 back (scale 1).
        let frames: Vec<Vec2> = (0..22).map(|_| Vec2::new(0.0, 26.0)).chain((0..7).map(|_| Vec2::new(0.0, -7.0))).collect();
        let run = |room: f32| {
            let (mut stopped, mut last, mut moved) = (false, 0.0f32, 0.0f32);
            // The fight's distance is a frame old: it has not counted last frame's carry.
            let (mut actual, mut seen) = (room, room);
            for &d in &frames {
                let d = clamp_lunge(d, Some(seen), 1.0, &mut stopped, &mut last);
                seen = actual;
                actual -= d.y;
                moved += d.y;
            }
            (moved, stopped)
        };
        // Room for all of it: the clip's own travel, 572 out and 523 at its end.
        let (moved, stopped) = run(2000.0);
        assert!((moved - 523.0).abs() < 1e-3 && !stopped, "{moved}");
        // 300 units to the body: carried to it and no further, and not back.
        let (moved, stopped) = run(300.0);
        assert!((moved - 300.0).abs() < 1e-3 && stopped, "{moved}");
        // Already touching: nothing.
        assert_eq!(run(0.0), (0.0, true));
        // No target: in full.
        let (mut s, mut l) = (false, 0.0);
        assert_eq!(clamp_lunge(Vec2::new(3.0, 40.0), None, 2.0, &mut s, &mut l), Vec2::new(3.0, 40.0));
        assert_eq!(l, 80.0);
    }

    use super::*;

    #[test]
    fn the_variant_weight_gives_its_share_beside_the_level_clip() {
        for share in [0.0f32, 0.25, 0.5, 0.9] {
            let w = variant_weight(share);
            assert!((w / (1.0 + w) - share).abs() < 1e-5, "{share}");
        }
        assert!(variant_weight(1.0).is_finite());
    }

    #[test]
    fn a_looping_clip_goes_round_from_fire_to_loop() {
        // The install's 2hhattackspin.kf: `Fire` at 0.2 s, `Loop` at 0.867 s.
        assert_eq!(loop_time(0.5, 0.2, 0.867), 0.5);
        assert!((loop_time(0.9, 0.2, 0.867) - 0.233).abs() < 1e-5);
        assert_eq!(loop_time(0.9, 0.9, 0.2), 0.9);
    }

    #[test]
    fn block_reactions_fall_back_to_the_unarmed_clips() {
        assert_eq!(reaction_clips(Stance::Stand, "1hm", "blockhit"), ["1hmblockhit.kf", "mtblockhit.kf", "h2hblockhit.kf"]);
        assert_eq!(reaction_clips(Stance::Stand, "h2h", "blockhit")[0], "mtblockhit.kf");
        assert_eq!(reaction_clips(Stance::Sneak, "2hm", "recoil")[..2], ["sneak2hmrecoil.kf", "2hmrecoil.kf"]);
        assert_eq!(reaction_clips(Stance::Stand, "h2h", "recoil")[0], "h2hrecoil.kf");
    }

    #[test]
    fn a_creature_combo_takes_the_named_group_first() {
        let mut names: Vec<String> = ["h2hattackright.kf", "h2hattackleft_mid.kf", "h2hattackleft2.kf"].map(String::from).into();
        put_chained_first(&mut names, SwingPick { next: Some(NextAttack::Left), rolls: [0.0; 2] });
        assert_eq!(names, ["h2hattackleft_mid.kf", "h2hattackright.kf", "h2hattackleft2.kf"]);
        put_chained_first(&mut names, SwingPick { next: None, rolls: [0.0; 2] });
        assert_eq!(names[0], "h2hattackleft_mid.kf");
    }

    #[test]
    fn a_shot_is_held_at_full_weight_between_shots() {
        // The 9mm SMG's `2haattackloop.kf` is 0.17 s long.
        let mut clip = nvf_assets::anim_play::Clip::from_controllers(Vec::new(), Vec::new(), Vec::new());
        clip.stop = 0.167;
        let clip = Arc::new(BoundClip::bind(Arc::new(clip), |_| None));
        let shot = Attack { clip: clip.clone(), up: None, down: None, t: 0.0, len: 0.167, repeat: None, since: 0.0, fade_in: true, reload: false, kind: Move::Attack, opening: false, melee: false, hit: 0.0, on_the_move: None, head_blow: None };
        let mut player = BlendPlayer::new(Vec::new(), Arc::new(Vec::new()));
        let mut pose = CombatPose { fighting: true, attack: Some(shot), ..Default::default() };
        let weight = |p: &BlendPlayer| p.layers[ATTACK_LAYER].as_ref().map_or(0.0, |l| l.weight);
        // Faded in, the clip does not fade out at its end.
        for _ in 0..16 {
            pose.layers(&mut player, true, 0.01);
        }
        assert_eq!(weight(&player), 1.0);
        // Ended, it is held at its last frame at full weight.
        for _ in 0..10 {
            pose.layers(&mut player, true, 0.01);
        }
        assert!(pose.attack.is_none() && pose.held.is_some());
        assert_eq!((weight(&player), player.layers[ATTACK_LAYER].as_ref().unwrap().phase), (1.0, 0.999));
        // Then fades out once no shot follows, and goes.
        player.layers = Default::default();
        pose.held.as_mut().unwrap().3 = 0.0;
        pose.layers(&mut player, true, SHOT_HOLD + FADE_OUT * 0.5);
        assert!((weight(&player) - 0.5).abs() < 0.1, "{}", weight(&player));
        pose.layers(&mut player, true, FADE_OUT);
        assert!(pose.held.is_none());
        // Out of the fight it goes at once.
        pose.held = Some((clip.clone(), None, None, 0.0));
        pose.fighting = false;
        player.layers = Default::default();
        pose.layers(&mut player, true, 0.01);
        assert!(pose.held.is_none() && player.layers[ATTACK_LAYER].is_none());
    }

    #[test]
    fn npc_weapon_visibility_samples_attack_reload_end_and_loop_time() {
        // [design, #2512] Synthetic clips with an end-only key and a nonzero start.
        let mut c = nvf_assets::anim_play::Clip::from_controllers(Vec::new(), Vec::new(), Vec::new());
        c.start = 3.0;
        c.stop = 4.0;
        c.visibility.push(("##NifRound".into(), 0, fnv_nif::anim::BoolTrack { value: None, keys: vec![(3.0, false), (4.0, true)] }));
        let clip = Arc::new(BoundClip::bind(Arc::new(c), |_| None));
        for kind in [Move::Attack, Move::Reload, Move::ReloadRound, Move::BlockHit, Move::Recoil] {
            let attack = Attack { clip: clip.clone(), up: None, down: None, t: 0.0, len: 1.0, repeat: None, since: 0.0, fade_in: true, reload: kind.reloads(), kind, opening: false, melee: false, hit: 0.0, on_the_move: None, head_blow: None };
            let mut pose = CombatPose { attack: Some(attack.clone()), ..Default::default() };
            let mut player = BlendPlayer::new(Vec::new(), Arc::new(Vec::new()));
            pose.layers(&mut player, true, 1.5);
            assert!(pose.attack.is_none());
            if matches!(kind, Move::Attack | Move::Reload | Move::ReloadRound) {
                let (sampled, time) = pose.weapon_visibility.as_ref().unwrap();
                assert!(Arc::ptr_eq(sampled, &clip));
                assert_eq!(*time, 4.0);
                assert_eq!(sampled.clip.visibility[0].2.at(*time), Some(true));
            } else {
                assert!(pose.weapon_visibility.is_none(), "reactions do not key the weapon");
            }
            pose.layers(&mut player, true, 0.1);
            assert!(pose.weapon_visibility.is_none(), "a held shot or aim does not replay visibility");
            if kind == Move::Attack {
                pose.attack = Some(Attack { repeat: Some((0.2, 0.6)), t: 0.55, ..attack });
                pose.layers(&mut player, true, 0.1);
                assert!((pose.weapon_visibility.as_ref().unwrap().1 - 3.25).abs() < 1e-5, "sample the wrapped loop time");
            }
        }
    }

    #[test]
    fn a_body_weapons_attacks_are_its_stances_attack_clips() {
        let folder = r"meshes\creatures\mistergutsy";
        let paths: Vec<String> = [
            r"meshes\creatures\mistergutsy\specialanims\1hmattackright_chop.kf",
            r"meshes\creatures\mistergutsy\specialanims\1hmattackright_uppercut.kf",
            r"meshes\creatures\mistergutsy\specialanims\1hmaim.kf",
            r"meshes\creatures\mistergutsy\specialanims\2hlattackspin.kf",
            r"meshes\creatures\mistergutsy\specialanims\2hlattackspinup.kf",
            r"meshes\creatures\mistergutsy\idleanims\1hmattackleft.kf",
        ]
        .map(String::from)
        .into();
        assert_eq!(body_attack_names(&paths, folder, "1hm"), ["1hmattackright_chop.kf", "1hmattackright_uppercut.kf"]);
        assert_eq!(body_attack_names(&paths, folder, "2hl"), ["2hlattackspin.kf"]);
    }

    #[test]
    fn a_skeletons_folder() {
        assert_eq!(folder_of(r"Creatures\Dog\Skeleton.NIF"), r"meshes\creatures\dog");
        assert_eq!(folder_of(r"meshes\characters\_male\skeleton.nif"), r"meshes\characters\_male");
    }
}
