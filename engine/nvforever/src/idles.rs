//! Idle animations played (docs/spec-notes/idles.md): what the game's rules ask for
//! (`fnv_game::idles`: a script's `PlayIdle`, a package's begin, end and change idles,
//! the idles on a dialogue response) and the idle collections of packages and idle
//! markers, on the actors drawn and on the player.
//!
//! - **Requests.** Each frame the game's `idle_requests` are taken and started: the
//!   idle's `.kf` is loaded, and how often it loops is rolled from its record
//!   (`IdleDef::loops`). An actor that is not drawn plays nothing.
//! - **Collections.** An actor standing where its package has it, with nothing else to
//!   do (`Residents::standing`), plays the idles of its package's collection, or of the
//!   idle marker it stands at (which override the package's [documented]), picked by
//!   `fnv_game::idles::IdlePicker`; so does the player under a script package. The pick
//!   is asked of the game like any other idle, so it is traced. A collection idle ends
//!   when its actor walks off.
//! - **Playing.** `fnv_actor::idle_play::IdlePlay` keeps each idle's time, loops and
//!   fades. On an NPC the clip is laid over what it plays otherwise (its locomotion, or
//!   its furniture clip) by track priority, on two more blend layers ([`ActorIdle`],
//!   read by `locomotion::drive`): the idle, and the one before it while the new one
//!   fades in. On the player ([`PlayerIdles`], read by `player_body.rs`) an idle whose
//!   clip drives the first-person camera node plays on the first-person rig and turns
//!   the view; any other plays on the third-person body.
//! - **Ends.** A one-shot idle ends at the end of its clip. An idle that loops for ever
//!   is replaced by the next idle asked for, and leaves its loop when the package (or
//!   marker) that asked for it is no longer the actor's.
//! - The rules are told what each actor plays (`Game::set_idle_playing`), which is what
//!   `IsIdlePlaying` answers.
//! - The slot also plays the animation groups scripts ask of actors (`PlayGroup`,
//!   `actor_groups.rs`); those are not idles for `IsIdlePlaying`.
//!
//! - **Drawn-weapon fidgets** (idles.md §11). An NPC standing still with its weapon out
//!   (a rifle loitering, lifting it one-handed to rest the arm, shifting its weight; an
//!   automatic rifle scanning), and the player standing still in third person with a
//!   weapon out and not aiming or firing, is checked every [`FIDGET_CHECK_SECONDS`]
//!   for a fidget from the IDLE tree (`Game::fidget`); the tree's own random
//!   percentages decide whether one plays. A fidget ends when its actor moves, puts the
//!   weapon away, or (the player) aims, fires, reloads or goes to first person.
//! - **Fidgets with the weapon away** (idles.md §11.4, #2026). An NPC standing still with
//!   its weapon away (not drawing it), not alerted and under no package flagged No Idle
//!   Anims is checked as often for a fidget from the tree's branches that want the weapon
//!   away (`FidgetKind::WeaponAway`): the general fidgets, smoking and the Pip-Boy look,
//!   and the drinking idles while it eats; the tree's own factions and random
//!   percentages decide who plays which, and how often, rolled on the tree's own
//!   generator (`Game::fidget`), not the scripts'. Not the player [design]. It ends
//!   when its actor moves, draws its weapon, is alerted, dies, is drawn into a
//!   conversation, or something with idles of its own takes over (furniture, a package
//!   with a collection or No Idle Anims, an idle marker's collection); one waiting for
//!   its clip is dropped then. The run summary names each NPC looked at and what it
//!   played (`holstered fidgets: …`). Test aid `NVF_PLAY_TEST_HOLSTERED_ROLL`.
//! - **Fidgets on furniture** (furniture.md §8). An actor on a chair, a bench, a bed or
//!   other furniture (`GetSitting` or `GetSleeping` 3: not getting on or off) is
//!   checked the same way for a fidget from the tree's furniture branch (a seated man
//!   relaxing, a sleeper stirring, a dancer, a wall leaner); so is the player on
//!   furniture in third person. It plays over the furniture's loop and ends (fades
//!   out) when its actor starts getting off.
//!
//! - **Talking gestures** (idles.md §12). An NPC in a conversation (with the player
//!   in the dialogue screen, or with another actor) is given a gesture from the IDLE
//!   tree's dialogue groups (`Game::talk_gesture`) when it starts saying a response,
//!   when it stops (it listens), and whenever the gesture it plays ends while it is
//!   still in the same part; the tree chooses talking or listening, the mood, sitting
//!   or standing. A response's own idle (INFO `SNAM`/`LNAM`) or any other idle asked
//!   for comes first: no gesture is picked while one plays. A gesture waits while the
//!   actor walks, turns on the spot, or gets on or off furniture, and ends when the
//!   conversation does, when the actor walks, or when its furniture posture changes
//!   (getting off a seat or sitting down).
//!
//! - **Cowering** (idles.md §13). An NPC fleeing from a fight that stands still (as
//!   when its flight begins, or at its flee spot) is checked every
//!   [`COWER_CHECK_SECONDS`] for the idle the tree plays while fleeing
//!   (`Game::flee_idle`: `CowerIdles`, `3rdP_Cowering.kf`, by its own conditions
//!   IsMoving 0 and GetCurrentAIProcedure 16), before its fidgets. Started, it is not
//!   stopped when its actor runs: its arms and head (priority 33) are laid over the
//!   unarmed run (30), whose legs win over its own (23), so it runs with its hands over
//!   its head; standing, over the crouched set (20) the rules give a person at its flee
//!   spot, the whole cower shows [observed: the clips' priorities; owner 2026-10-17,
//!   "they'll cower whilst running but they only crouch when stopped"]. It ends when its
//!   actor no longer flees; after its loops (2 to 12 of 12 s) it is picked again when
//!   the actor next stands still. Flee Not Combat packages (procedure 34) are not
//!   carried out (`fnv_ai::goal`), so only combat flights cower.
//!
//! - **Panic** (pickpocket.md §5). An NPC with a live explosive planted in its pocket
//!   (`GetPlantedExplosive` 1: from `fProjectileInventoryGrenadeFreakoutTime` to the
//!   blast) is given the idle tree's panic idle once (`Game::panic_idle`: the roots whose
//!   conditions name GetPlantedExplosive, `PanicGrenadePlant.kf` or its power armour
//!   form); one whose folder has none (every creature) plays nothing. Either way it
//!   stands where it is until the blast (`residents.rs`).
//!
//! - **PickIdle**. A script's `PickIdle` asks for the tree's pick for its actor
//!   (`Game::idle_picks`); the actor's folder walked as for its fidgets, with no branch
//!   kept out (`Game::pick_idle`), and the pick played as a script's idle.
//!
//! - **Activating** (idles.md §16, #2349). The player's activation asks the tree's
//!   activation root (`Game::activate_idles`, `Game::activate_idle`: `ActivateIdles`,
//!   whose conditions choose): drinking from placeable water plays `1stPersonActWater`
//!   on the first-person rig in first person, `3rdPersonActWater` on the body in third.
//!   It holds nothing: unlike a scripted idle that drives the camera node, the player
//!   keeps mouse look, movement and the HUD [design, #2349].
//!
//! - **Creatures' combat idles** (idles.md §14). A creature standing still with its
//!   stance drawn (`Game::draws_weapon`: fighting, alerted, or warning the player) is
//!   checked every [`FIDGET_CHECK_SECONDS`] for an idle from the tree's branch that
//!   names GetIsAlerted (the gecko's hiss and side hops, the mirelurk's and sentry bot's
//!   warnings); the tree's own conditions decide: the gecko's want alerted, combat or
//!   flee, so a gecko hisses standing in a fight (procedure 13, Lost too) and not while
//!   it warns (GetIsAlerted 0, procedure 44 then, as observed in the original). The
//!   warnings that want GetIsAlerted alone play only for an actor SetAlert alerted: a
//!   fight does not set GetIsAlerted (#2121, `Game::is_alerted`). It ends when the creature moves
//!   or is calm again; an attack stops it as it stops any idle (`attack_anims.rs`).
//!   People keep their drawn-weapon fidgets [design].
//!
//! - **Calm creatures' idles** (#2051). A creature standing still and calm (not drawing
//!   its weapon, not on furniture, not talking or cowering) is checked as often for an
//!   idle from its tree, the tree's own conditions deciding (`FidgetKind::Calm`): the
//!   protectron's CommonIdle (IsInCombat 0) gives IdleSearch, IdleLookLeft and
//!   IdleLookRight by their random rolls [observed: HELIOS One's stasis protectrons,
//!   #1977 probe 2]. Its Idle-section leaves are left to the standing looks. It ends
//!   when the creature moves, is alerted or dies.
//!
//! - **Stimpaks** (`npc_stimpaks.rs`). An NPC's stimpak idle plays on its upper body
//!   only, and over the fight's aim and attacks: it walks and runs while it injects.
//!
//! Not done: the rest of the idle manager (idles chosen from the IDLE tree by their
//! conditions: other fidgets; how often the original asks its tree, #2026), the sounds
//! and other text keys of idle clips,
//! and package idles of actors that are not residents. The props idles hold (ANIO) are
//! drawn by `anim_objects.rs`.

use crate::limbs::ActorRig;
use crate::locomotion::Locomotion;
use crate::residents::Residents;
use crate::weapons::{HeldWeapon, WeaponHold};
use bevy::prelude::*;
use fnv_actor::idle::meshes_path;
use fnv_actor::idle_play::{IdleKeys, IdlePlay};
use fnv_actor::weapon::Stage;
use fnv_game::game::Game;
use fnv_game::idles::{FidgetKind, IdlePicker, IdlePov, IdleRequest, IdleSource, Loops};
use fnv_game::nvse_knvse::SequenceInfo;
use fnv_game::package::{flags, PackageType};
use fnv_sim::FormId;
use nvf_assets::actor::{bind_clip, ActorCtx, ActorParts};
use nvf_assets::anim_play::{BlendPlayer, BoundClip, Clip, Layer, ScriptLayer};
use nvf_assets::model::ModelCtx;
use nvf_play::control::PlayerControl;
use nvf_play::player::Player;
use nvf_play::script::ScriptState;
use nvf_play::RefVisual;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// The blend layers of an actor's idles: the one playing, then the one before it.
pub const IDLE_LAYER: usize = 4;

/// The first-person skeleton's camera node [observed: animation.md §9.5]. A clip with a
/// track for it is a first-person idle (idles.md §4).
pub const CAMERA_NODE: &str = "Camera1st";

/// After a collection's idle could not be started (its actor is not drawn, its clip does
/// not load), seconds until the collection is tried again [engineering choice].
const RETRY_SECONDS: f32 = 5.0;

/// How long an idle asked for waits for its clip to be read on a worker thread before it
/// is read in the frame it starts [engineering choice, #1429: a KF read and parse on the
/// main thread cost 8.4 ms in one frame at the Gomorrah floor; a few frames' delay to an
/// idle's start is not seen, a frame that long is].
const CLIP_WAIT: std::time::Duration = std::time::Duration::from_millis(500);

/// Seconds between two looks at the idle tree for an actor's drawn-weapon fidget
/// [design: how often the original evaluates its tree is not known; with the rifle
/// branch's three 5% leaves a standing rifleman fidgets about every 14 s]. The fidgets
/// with the weapon away use it too [design, #2026: not calibrated; the general fidgets'
/// node passes 1 check in 100, about one general fidget in 200 s here, where the
/// original's AI observer saw three to six fidgets begun per NPC in 360 s (#2026);
/// observer check 116, docs/owner-review.md].
pub const FIDGET_CHECK_SECONDS: f32 = 2.0;

/// Seconds between two looks at the idle tree for a fleeing actor's cower idle: short,
/// so it cowers soon after it stops and again soon after the idle's loops end
/// [engineering choice].
pub const COWER_CHECK_SECONDS: f32 = 0.5;

/// Seconds between two looks at the idle tree for a creature's standing look
/// (`IdleSource::Standing`): a changed state shows within a second [engineering choice].
pub const STANDING_CHECK_SECONDS: f32 = 1.0;

/// After the tree gave an actor in a conversation no gesture, seconds until it is asked
/// again in the same part (a new response asks at once) [engineering choice].
pub const GESTURE_RETRY_SECONDS: f32 = 1.0;

/// The most NPCs the run summary names one by one with their fidgets with the weapon
/// away [engineering choice: a smoke run looks at a few; a long run crosses many cells].
const HOLSTERED_LINES: usize = 200;

/// The player moved when their feet went further than this in a frame, game units
/// [engineering choice: above the collision's settling].
const PLAYER_STILL: f32 = 0.5;

/// The animation folder of people (NPC_ and the player) below `meshes\`
/// [observed: animation.md §3.1].
const PEOPLE_FOLDER: &str = "characters\\_male";

/// One idle playing, with the clip as its rig takes it.
pub struct Playing<C> {
    /// The IDLE record (0 for a group).
    pub idle: u32,
    /// The animation group a script asked for with `PlayGroup` (`actor_groups.rs`)
    /// instead of an idle.
    pub group: Option<u16>,
    pub source: IdleSource,
    /// What asked for a package's or a marker's idle: the actor's package at the time,
    /// or the marker's base.
    owner: Option<FormId>,
    pub clip: C,
    pub play: IdlePlay,
    /// Tells this idle from the others the slot played.
    pub serial: u32,
    /// Started in the step under way.
    fresh: bool,
    /// Holds its actor in place until it ends ([`holds_in_place`]).
    pub holds: bool,
    /// Whether the actor was seated on furniture (`on_furniture`, state 3) when this
    /// idle started, so seated gestures and dialogue idles fade out if it starts getting
    /// off furniture (#3407).
    pub seated: bool,
    /// The normalized source file, when this is an IDLE loaded from a file.
    file: Option<String>,
}

/// The idles of one rig: the one playing and, while that fades in, the one before it.
pub struct IdleSlot<C> {
    pub cur: Option<Playing<C>>,
    pub prev: Option<Playing<C>>,
    serial: u32,
}

impl<C> Default for IdleSlot<C> {
    fn default() -> Self {
        IdleSlot { cur: None, prev: None, serial: 0 }
    }
}

impl<C> IdleSlot<C> {
    /// Play a clip that is no IDLE record (a slot machine's seat) as an idle `source`
    /// asked for; returns its serial.
    pub fn start_clip(&mut self, source: IdleSource, clip: C, play: IdlePlay) -> u32 {
        self.start(0, source, None, clip, play);
        self.serial
    }

    /// Play an idle from its start. What played until now is held under it while it
    /// fades in; an idle started in the same step is replaced outright (a package's
    /// change idle followed by the next package's begin idle).
    ///
    /// The idle that shows now asked for again by a script or a dialogue line is not
    /// started over: it plays on, or holds its last pose again (`IdlePlay::again`);
    /// returns false then. (Collections, fidgets and gestures that pick the same idle
    /// again play it again.) That holds too for an ended one that a fidget or a
    /// collection's idle, started as it ended, now fades in over: it comes back and the
    /// newcomer goes [design: #1355, the opening's mirror hand-over, where Doc's chair
    /// fidget started in the step the scripted idle ended and the next line's speaker
    /// idle, asked for a frame later, handed the mirror over a second time].
    fn start(&mut self, idle: u32, source: IdleSource, owner: Option<FormId>, clip: C, play: IdlePlay) -> bool {
        self.start_file(idle, source, owner, clip, play, None)
    }

    fn start_file(&mut self, idle: u32, source: IdleSource, owner: Option<FormId>, clip: C, play: IdlePlay, file: Option<String>) -> bool {
        let asked = matches!(source, IdleSource::Script | IdleSource::Speaker | IdleSource::Listener);
        let same = |c: &Playing<C>| idle != 0 && c.idle == idle && c.file == file && c.group.is_none() && !c.play.gone();
        let picked = |c: &Playing<C>| !matches!(c.source, IdleSource::Script | IdleSource::Speaker | IdleSource::Listener);
        if asked && self.cur.as_ref().is_some_and(picked) && self.prev.as_ref().is_some_and(same) {
            self.cur = self.prev.take();
        }
        if let Some(c) = self.cur.as_mut().filter(|c| asked && same(c)) {
            c.play.again();
            (c.source, c.owner) = (source, owner);
            return false;
        }
        if let Some(old) = self.cur.take().filter(|o| !o.fresh && !o.play.gone()) {
            self.prev = Some(old);
        }
        self.serial = self.serial.wrapping_add(1);
        self.cur = Some(Playing { idle, group: None, source, owner, clip, play, serial: self.serial, fresh: true, holds: false, seated: false, file });
        true
    }

    /// Play a script's animation group as an idle is played (`actor_groups.rs`); returns
    /// its serial.
    pub fn start_group(&mut self, group: u16, clip: C, play: IdlePlay) -> u32 {
        self.start(0, IdleSource::Script, None, clip, play);
        if let Some(c) = self.cur.as_mut() {
            c.group = Some(group);
        }
        self.serial
    }

    /// Advance by `dt` seconds.
    fn advance(&mut self, dt: f32) {
        if let Some(c) = self.cur.as_mut() {
            c.play.advance(dt);
        }
        if self.cur.as_ref().is_some_and(|c| c.play.gone()) {
            self.cur = None;
        }
        // The one before goes on as it was (a loop keeps looping, an ended one holds its
        // last pose) until the new one covers it.
        if let Some(p) = self.prev.as_mut().filter(|p| !p.play.ended()) {
            p.play.advance(dt);
        }
        if self.cur.as_ref().is_none_or(|c| c.play.ended() || c.play.weight() >= 1.0) {
            self.prev = None;
        }
    }

    /// The idle playing (not one that has ended and fades out).
    pub fn playing(&self) -> Option<&Playing<C>> {
        self.cur.as_ref().filter(|c| !c.play.ended())
    }

    pub fn is_empty(&self) -> bool {
        self.cur.is_none() && self.prev.is_none()
    }

    /// What shows, with its weight: the idle playing (fading in from its start, out
    /// from its end), and the one before it with the rest while the new one fades in.
    pub fn shown(&self) -> impl Iterator<Item = (&Playing<C>, f32)> {
        let w = self.cur.as_ref().map_or(0.0, |c| c.play.weight());
        self.cur.iter().map(move |c| (c, w)).chain(self.prev.iter().map(move |p| (p, 1.0 - w)))
    }

    /// Whether the idles cover the rig's other clips: one plays in full, or fades in
    /// over the one before it. (While the first one fades in, the other clips stay
    /// under it.)
    pub fn covers(&self) -> bool {
        self.playing().is_some_and(|c| self.prev.is_some() || c.play.weight() >= 1.0)
    }

    /// A package's or marker's idle that loops for ever leaves its loop once `owner`
    /// (the actor's package now, or the marker it stands at) is not what asked for it.
    fn check_owner(&mut self, owner_now: impl Fn(IdleSource) -> Option<FormId>) {
        // A new step: what plays was not started in it.
        if let Some(c) = self.cur.as_mut() {
            c.fresh = false;
        }
        if let Some(c) = self.cur.as_mut().filter(|c| c.source.of_package() && c.play.endless()) {
            if owner_now(c.source) != c.owner {
                c.play.leave_loop();
            }
        }
    }
}

/// The idles an actor plays (on its root, beside its [`Locomotion`]).
#[derive(Component, Default)]
pub struct ActorIdle(pub IdleSlot<Arc<BoundClip>>);

impl ActorIdle {
    /// Whether a stimpak's upper-body idle shows (`npc_stimpaks.rs`): it is laid over the
    /// fight's aim and attacks too.
    pub fn over_pose(&self) -> bool {
        self.0.shown().any(|(p, w)| w > 0.0 && p.source == IdleSource::Stimpak)
    }

    /// The idles as blend layers ([`IDLE_LAYER`] and the next).
    pub fn layers(&self, player: &mut BlendPlayer) {
        for (k, (p, w)) in self.0.shown().enumerate() {
            let len = p.play.keys.length.max(1e-3);
            player.layers[IDLE_LAYER + k] = (w > 0.0).then(|| Layer { clip: p.clip.clone(), phase: (p.play.time() / len).min(if p.clip.clip.cycle_type == 2 { 1.0 } else { 0.999 }), weight: w });
        }
    }
}

/// The NPCs whose legs an idle poses ([`holds_in_place`]) and who stand while it shows,
/// shared with `residents.rs` (idles.md §15).
#[derive(Resource, Default)]
pub struct IdleHolds {
    /// Written here: those whose idle shows (playing or fading out). Their walk waits.
    pub held: HashSet<u32>,
    /// Written by `residents.rs`: those of them whose AI has somewhere to go. Their idle
    /// fades out at once (a looping one too), and they walk once it has gone
    /// [design: owner 2026-10-31].
    pub breaking: HashSet<u32>,
    /// Written here: those of `held` whose idle carries the body away from the reference
    /// ([`carries_body`]).
    pub carried_off: HashSet<u32>,
}

/// How far (units, across the ground) an idle's `Bip01 NonAccum` must take the body from
/// the reference for [`carries_body`] [engineering choice: a standing idle's sway stays
/// within a few units; Dad's walk in the birth scene reaches 144].
const CARRY_DISTANCE: f32 = 32.0;

/// Whether `clip` moves `Bip01 NonAccum` (where the body hangs from the reference) more
/// than [`CARRY_DISTANCE`] across the ground at any of a few times through it: a scene
/// that walks its actor about while the reference stays (Fallout 3's birth scene: Dad's
/// sections keep him 96, 108 to 128 units forward and aside of his marker [observed:
/// `cg00dadsection00.kf` .. `02.kf`, fnvtool nif decode]).
pub fn carries_body(clip: &Clip) -> bool {
    let Some((_, t)) = clip.tracks.iter().find(|(n, _)| n.eq_ignore_ascii_case("Bip01 NonAccum")) else { return false };
    (0..=8).any(|k| t.translation.sample(clip.start + clip.length() * k as f32 / 8.0).is_some_and(|v| v[0].hypot(v[1]) > CARRY_DISTANCE))
}

/// Whether a track drives a leg: the people's `Bip01 L Thigh`, `Bip01 R Calf`,
/// `Bip01 L Foot`, and creatures' bones named the same way or `...Leg...`
/// [observed: the bone names of `_male\skeleton.nif` and the idle clips].
fn is_leg(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["thigh", "calf", "foot", "leg"].iter().any(|k| n.contains(k))
}

/// The highest priority of a clip's leg tracks.
fn leg_priority(clip: &Clip) -> Option<i32> {
    clip.tracks.iter().enumerate().filter(|(_, (n, _))| is_leg(n)).map(|(k, _)| clip.priorities.get(k).copied().unwrap_or(0)).max()
}

/// Whether an idle poses its actor's legs, so the actor stands while it shows and breaks
/// out of it (fading it out) to walk (idles.md §15) [design: the original's rule is not
/// documented, observer check 57; breaking out: owner 2026-10-31]:
///
/// - asked for by a script (`PlayIdle`, `PickIdle`), a package's begin, end or change, a
///   dialogue line, or a pick-up (collections, fidgets, gestures, cowering and stimpaks
///   end, or play on, when their actor moves: not held);
/// - not of the Upper Body section (`IDLE` DATA, [`fnv_game::data::IDLE_SECTION_UPPER_BODY`]);
/// - its legs win over the walk: one of its leg tracks has a higher priority than the
///   actor's walking clip's legs (`CheeringBothArmsUp.kf` 50 over `mtforward.kf` 30),
///   so walking on would show its legs standing while the body slides. A cower's legs
///   (23) lose to the run's (30): it runs with its arms over its head (§13).
pub fn holds_in_place(source: IdleSource, section: u8, idle: &Clip, walk: Option<&Clip>) -> bool {
    let asked = matches!(
        source,
        IdleSource::Script | IdleSource::Picked | IdleSource::PackageBegin | IdleSource::PackageEnd | IdleSource::PackageChange | IdleSource::Speaker | IdleSource::Listener | IdleSource::PickUp
    );
    asked
        && section != fnv_game::data::IDLE_SECTION_UPPER_BODY
        && match (leg_priority(idle), walk.and_then(leg_priority)) {
            (Some(i), Some(w)) => i > w,
            _ => false,
        }
}

/// The clip an actor walks on: its stance's forward walk, else its run.
fn walk_clip(lo: &Locomotion) -> Option<&Clip> {
    use fnv_actor::locomotion::Move;
    lo.set.clips.get(&Move::Forward).or_else(|| lo.set.clips.get(&Move::FastForward)).map(|c| &*c.bound.clip)
}

/// The idles the player plays: on the first-person rig (clips that drive its camera
/// node) and on the third-person body (all others).
#[derive(Resource, Default)]
pub struct PlayerIdles {
    pub first: IdleSlot<Arc<Clip>>,
    pub third: IdleSlot<Arc<Clip>>,
    /// The player when an idle began to turn the view ([`Held`]).
    pub held: Option<Held>,
    /// A menu that is played at a place in the world is open (the slot machine's,
    /// `slots_seat.rs`): the third-person body's idles go on under it, where a menu
    /// otherwise holds them [design: the body is seen at the machine; slots-170-B
    /// camera-win-warm, the cheer held on its first frame].
    pub under_menu: bool,
}

/// Movement camera overlays animate the view without owning position; other
/// sections retain the scene hold (docs/spec-notes/movement-camera-holds.md).
fn camera_idle_holds(section: u8) -> bool {
    section != 1 // IDLE Animation Group Section: Movement (fnv_actor::idle::SECTIONS).
}

impl PlayerIdles {
    pub fn holds_camera(&self) -> bool {
        self.first.playing().is_some_and(|p| p.holds)
            || self.first.shown().any(|(p, weight)| weight > 0.0 && p.holds)
    }
}

/// The player as an idle that turns the view found them. While such idles play, the
/// first-person rig stands at `feet` with that heading, the character is held there
/// (`nvf_play::furniture::PlayerSeat`), and only the view's own turning since then is
/// added to what the animation gives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Held {
    /// Where the game last put the player if they have not moved by themselves since
    /// (`Player::placed`), else where they stand [observed: the opening's start marker
    /// is in Doc Mitchell's bed, and the collision pushes the character about 10 units
    /// off it before the wake-up starts; idles.md §4].
    pub feet: [f32; 3],
    /// Where the character stood when the idle began, clear of what it collides with:
    /// it stands there again when the idles let it go [design: let go at `feet`, the
    /// opening's character was pushed up onto the bed frame; owner report 2026-10-16].
    pub free: [f32; 3],
    pub heading: f32,
    pub pitch: f32,
}

impl Held {
    /// How far (units) the game's placement may lie from `feet` and still be the same.
    const SAME_PLACE: f32 = 1.0;

    /// Where the player is held now that the game has put them somewhere else while the
    /// idles held them, `None` when it has not. A script's `MoveTo` lands a frame after
    /// the idle it starts with: Tale of Two Wastelands' opening starts the birth idle in
    /// its start room and moves the player into the birth cell in the same script run,
    /// and the view stayed at the start room's spot in the birth cell, an empty screen
    /// [observed: owner's TTW run 2026-11-02, the idle at (2036, 1946), the travel to
    /// (-5276, -7148)].
    pub fn moved(&self, placed: Option<[f32; 3]>, heading: f32, pitch: f32) -> Option<Held> {
        let p = placed?;
        let far = (0..3).any(|i| (p[i] - self.feet[i]).abs() > Self::SAME_PLACE);
        far.then_some(Held { feet: p, free: p, heading, pitch })
    }
}

/// An actor's place in an idle collection.
struct Collection {
    /// A marker's (true) or a package's, and its form.
    key: (bool, u32),
    picker: IdlePicker,
    /// The idle picked and the seconds still to wait before it.
    next: Option<(FormId, f32)>,
    /// Seconds until the collection is tried again after a failed start.
    retry: f32,
}

/// Counts for the run summary.
#[derive(Debug, Default, Clone, Copy)]
pub struct Stats {
    pub asked: u32,
    pub script: u32,
    pub package: u32,
    pub collection: u32,
    pub dialogue: u32,
    pub started: u32,
    pub player_started: u32,
    pub not_drawn: u32,
    pub no_clip: u32,
    /// Of those, idles whose clip is another creature's ([`foreign_creature_clip`]).
    pub foreign_clips: u32,
    /// Drawn-weapon fidgets started on actors and on the player.
    pub actor_fidgets: u32,
    pub player_fidgets: u32,
    /// Fidgets started on NPCs standing with their weapon away (#2026).
    pub holstered_fidgets: u32,
    /// Creatures' combat idles started (alerted, standing).
    pub combat_idles: u32,
    pub calm_idles: u32,
    /// Fidgets on furniture started on actors and on the player.
    pub actor_furniture_fidgets: u32,
    pub player_furniture_fidgets: u32,
    /// Talking gestures started while the actor talked and while it listened; walks of
    /// the tree that gave none.
    pub talk_gestures: u32,
    pub listen_gestures: u32,
    pub no_gesture: u32,
    /// Cower idles started on fleeing actors.
    pub cowers: u32,
    /// Panic idles started on actors with a live explosive in the pocket, and such
    /// actors whose folder has none.
    pub panics: u32,
    pub no_panic: u32,
    /// Idles that waited for their clip to be read on a worker thread, the longest wait
    /// (milliseconds), and those whose clip was read in the frame they started after
    /// [`CLIP_WAIT`].
    pub clip_waits: u32,
    pub longest_clip_wait_ms: f64,
    pub clip_read_late: u32,
}

/// An actor's part in a conversation as the gestures last saw it: talking or listening,
/// the line and its response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TalkKey {
    talking: bool,
    info: Option<FormId>,
    response: usize,
}

/// The gestures of one actor in a conversation.
struct Gesturing {
    key: TalkKey,
    /// Seconds until the tree is asked again after it gave nothing.
    retry: f32,
}

/// A small deterministic generator (SplitMix64) for loop counts and random picks: the
/// script runtime's random numbers are not touched, and runs repeat.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u32) -> u32 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        (((z ^ (z >> 31)) >> 32) as u32) % n.max(1)
    }
}

#[derive(Resource)]
pub struct Idles {
    collections: HashMap<u32, Collection>,
    /// Seconds until each actor's next look for a fidget, and for its cower idle.
    fidget_check: HashMap<u32, f32>,
    cower_check: HashMap<u32, f32>,
    /// The actors with a live explosive whose panic was looked for (`GetPlantedExplosive`).
    panicked: HashSet<u32>,
    /// Seconds until each creature's next look for its standing look.
    standing_check: HashMap<u32, f32>,
    /// When each actor last started each fidget or gesture (seconds of `clock`), for
    /// the replay delay.
    fidget_last: HashMap<(u32, u32), f32>,
    /// The actors in a conversation.
    gesturing: HashMap<u32, Gesturing>,
    /// The NPCs looked at for a fidget with the weapon away: their name for the run
    /// summary (their base's Editor ID and their own ID) and the fidgets started on each.
    holstered: HashMap<u32, (String, HashMap<String, u32>)>,
    /// Test aid `NVF_PLAY_TEST_HOLSTERED_ROLL=N` (0 to 99): every `GetRandomPercent` in
    /// the walks for fidgets with the weapon away reads N, so a smoke run can make the
    /// tree's random branches pass and see their clips play (#2026).
    holstered_roll: Option<u32>,
    clock: f32,
    /// The player's feet the frame before.
    player_feet: Option<[f32; 3]>,
    /// What each actor was last reported to the rules as playing.
    told: HashMap<u32, u32>,
    /// Idles asked for whose clip is being read on a worker thread, and since when.
    waiting: Vec<(IdleRequest, std::time::Instant)>,
    rng: Rng,
    pub stats: Stats,
}

impl Default for Idles {
    fn default() -> Self {
        Idles {
            collections: HashMap::new(),
            fidget_check: HashMap::new(),
            cower_check: HashMap::new(),
            panicked: HashSet::new(),
            standing_check: HashMap::new(),
            fidget_last: HashMap::new(),
            gesturing: HashMap::new(),
            holstered: HashMap::new(),
            holstered_roll: holstered_roll_aid(),
            clock: 0.0,
            player_feet: None,
            told: HashMap::new(),
            waiting: Vec::new(), rng: Rng(0x1D1E_5EED), stats: Stats::default() }
    }
}

/// A sequence a script played on an NPC that [`play_npc_sequences`] started: the actor,
/// the path and the idle's serial in its slot (0: its clip could not be played).
#[derive(Debug, Clone)]
pub struct NpcSequence {
    actor: u32,
    path: String,
    serial: u32,
}

/// What [`play_npc_sequences`] reads and writes of a drawn NPC.
type NpcSeqActor = (Entity, &'static RefVisual, &'static ActorRig, Option<&'static mut ActorIdle>, Option<&'static mut BlendPlayer>);

fn sequence_info(clip: &Clip) -> SequenceInfo {
    SequenceInfo {
        name: clip.name.clone(),
        accum_root: clip.accum_root.clone().unwrap_or_default(),
        frequency: f64::from(clip.frequency),
        begin: f64::from(clip.start),
        end: f64::from(clip.stop),
        cycle: clip.cycle_type,
        text_keys: clip.text_keys.iter().map(|(t, s)| (f64::from(*t), s.clone())).collect(),
    }
}

/// kNVSE's `PlayAnimationPath` and `ActivateAnim` on the drawn NPCs
/// (docs/design/knvse-anim.md §5):
/// - `PlayAnimationPath` starts on the actor's idle slot, bound to its skeleton; once if
///   the clip says so (cycle type Clamp), else looping until `DeactivateAnim` takes it
///   back or another idle replaces it.
/// - `ActivateAnim` (and the blend helpers `CrossFadeAnims`, `BlendToAnim`,
///   `BlendToAnimFromPose`) layers onto `BlendPlayer::script_layers` with explicit
///   sequence priority, weight (`SetAnimWeight`), ease-in/ease-out, clock/seek
///   (`SetAnimCurrentTime`), and additive reference poses (`SetAnimAdditive`) [design].
#[allow(clippy::too_many_arguments)]
pub fn play_npc_sequences(
    mut commands: Commands,
    time: Res<Time>,
    mut script: Option<ResMut<ScriptState>>,
    mut parts: ResMut<ActorParts>,
    mut models: ModelCtx,
    mut actors: Query<NpcSeqActor>,
    mut started: Local<Vec<NpcSequence>>,
    mut layer_eases: Local<HashMap<(u32, u32), f32>>,
) {
    let Some(game) = script.as_deref_mut().and_then(|s| s.rt.as_mut()).map(|rt| &mut rt.game) else { return };
    let dt = if game.menu.is_some() { 0.0 } else { time.delta_secs().min(0.1) };
    let me = FormId::PLAYER_REF;
    let same = |a: &str, b: &str| a.replace('/', "\\").eq_ignore_ascii_case(&b.replace('/', "\\"));
    let wanted: Vec<(u32, String)> = game
        .anim_paths
        .played
        .iter()
        .filter(|(a, _)| **a != me)
        .flat_map(|(a, ps)| ps.iter().map(move |p| (a.0, p.clone())))
        // ActivateAnim's requests (they stay until taken back, `auto_finish` false) are
        // layered sequences driven on `BlendPlayer::script_layers` below, not idles.
        .filter(|(a, p)| !game.anim_paths.sequences.get(&(FormId(*a), false, p.replace('/', "\\").to_ascii_lowercase())).is_some_and(|s| !s.auto_finish))
        .collect();
    let mut layered: Vec<(FormId, fnv_game::nvse_anim::ScriptSequence)> = game
        .anim_paths
        .sequences
        .iter()
        .filter(|((a, _, _), s)| *a != me && !s.auto_finish)
        .map(|((a, _, _), s)| (*a, s.clone()))
        .collect();
    layered.sort_by_key(|(_, s)| s.serial);
    if wanted.is_empty() && started.is_empty() && layered.is_empty() && layer_eases.is_empty() {
        return;
    }
    let drawn: HashMap<u32, Entity> = actors.iter().map(|(e, v, ..)| (v.0, e)).collect();
    // Taken back: its idle stops; out of view: forgotten, so it starts again when drawn.
    started.retain(|s| {
        let kept = wanted.iter().any(|(a, p)| *a == s.actor && same(p, &s.path));
        let Some(&e) = drawn.get(&s.actor) else { return false };
        if kept {
            return true;
        }
        if let Ok((_, _, _, Some(mut slot), _)) = actors.get_mut(e) {
            if let Some(c) = slot.0.cur.as_mut().filter(|c| c.serial == s.serial && s.serial != 0) {
                c.play.stop();
            }
        }
        info!("anim sequence: {:08X}'s {} stops", s.actor, s.path);
        false
    });
    let mut ctx = ActorCtx { cache: &mut parts, models: &mut models };
    for (actor, p) in &wanted {
        if started.iter().any(|s| s.actor == *actor && same(&s.path, p)) {
            continue;
        }
        let Some(&e) = drawn.get(actor) else { continue };
        let path = meshes_path(p);
        if !ctx.clip_ready(&path) {
            continue;
        }
        let Ok((e, _, rig, slot, _)) = actors.get_mut(e) else { continue };
        let bound = ctx.clip(&path).map(|clip| (bind_clip(&rig.skel, clip.clone()), clip));
        let Some((bound, clip)) = bound.filter(|(b, _)| !b.tracks.is_empty()) else {
            warn!("anim sequence: {actor:08X}'s {p} does not load or drives no node of its skeleton");
            started.push(NpcSequence { actor: *actor, path: p.clone(), serial: 0 });
            continue;
        };
        game.set_sequence_info(p, sequence_info(&clip));
        let keys = IdleKeys::read(clip.start, clip.stop, &clip.text_keys);
        let once = clip.cycle_type == 2;
        let play = IdlePlay::new(keys, once.then_some(1));
        let bound = Arc::new(bound);
        let serial = match slot {
            Some(mut s) => {
                s.0.start(0, IdleSource::Script, None, bound, play);
                s.0.serial
            }
            None => {
                let mut s = ActorIdle::default();
                s.0.start(0, IdleSource::Script, None, bound, play);
                let serial = s.0.serial;
                commands.entity(e).insert(s);
                serial
            }
        };
        info!("anim sequence: {actor:08X} plays {p} ({})", if once { "once" } else { "looping" });
        started.push(NpcSequence { actor: *actor, path: p.clone(), serial });
    }
    // Clear previous frame's layered NPC script sequences before rebuilding active ones.
    for (_, _, _, _, player) in &mut actors {
        if let Some(mut p) = player {
            if !p.script_layers.is_empty() {
                p.script_layers.clear();
            }
        }
    }
    let active_keys: HashSet<(u32, u32)> = layered.iter().map(|(a, s)| (a.0, s.serial)).collect();
    for (actor, request) in layered {
        let Some(&e) = drawn.get(&actor.0) else { continue };
        let path = meshes_path(&request.path);
        if !ctx.clip_ready(&path) {
            continue;
        }
        let Some(clip) = ctx.clip(&path) else {
            warn!("anim sequence: {:08X}'s {:?} could not load; not rendered", actor.0, request.path);
            let norm = request.path.replace('/', "\\").to_ascii_lowercase();
            game.anim_paths.sequences.remove(&(actor, false, norm));
            if let Some(paths) = game.anim_paths.played.get_mut(&actor) {
                paths.retain(|p| !same(p, &request.path));
            }
            continue;
        };
        game.set_sequence_info(&request.path, sequence_info(&clip));
        let norm_key = request.path.replace('/', "\\").to_ascii_lowercase();
        let additive_spec = game
            .knvse
            .additive
            .get(&(actor, norm_key))
            .filter(|(first, _, _)| !*first)
            .map(|(_, p, t)| (p.clone(), *t as f32));
        let ref_clip = match &additive_spec {
            Some((ref_pose, _)) if !ref_pose.is_empty() => {
                let ref_path = meshes_path(ref_pose);
                if !ctx.clip_ready(&ref_path) {
                    continue;
                }
                ctx.clip(&ref_path)
            }
            _ => None,
        };
        let Ok((_, _, rig, _, Some(mut player))) = actors.get_mut(e) else { continue };
        let bound = bind_clip(&rig.skel, clip.clone());
        if bound.tracks.is_empty() {
            continue;
        }
        let clock = game.script_sequence_time_for(actor, &request.path, false);
        let phase = (clock * f64::from(clip.frequency)) as f32;
        if !phase.is_finite() || !request.weight.is_finite() {
            continue;
        }
        let keys = IdleKeys::read(clip.start, clip.stop, &clip.text_keys);
        let fade_in = request.ease_in.unwrap_or(keys.blend_in);
        let ease = layer_eases.entry((actor.0, request.serial)).or_insert_with(|| if fade_in <= 0.0 { 1.0 } else { 0.0 });
        if request.stopped.is_some() {
            let down = if request.ease_out <= 0.0 { 1.0 } else { dt / request.ease_out };
            *ease = (*ease - down).max(0.0);
        } else {
            let up = if fade_in <= 0.0 { 1.0 } else { dt / fade_in };
            *ease = (*ease + up).min(1.0);
        }
        let eff = (request.weight.clamp(0.0, 1.0) * *ease).clamp(0.0, 1.0);
        if eff <= 0.0 {
            continue;
        }
        let additive = additive_spec.map(|(_, ref_time)| (ref_clip.map(|rc| Arc::new(bind_clip(&rig.skel, rc))), ref_time));
        player.script_layers.push(ScriptLayer {
            clip: Arc::new(bound),
            time: phase,
            weight: eff,
            priority: request.priority,
            additive,
        });
    }
    layer_eases.retain(|k, _| active_keys.contains(k));
}

/// The clip of an idle and its playback, as its record, active kNVSE overrides, and text
/// keys say; a creature's standing look loops until replaced (`IdleSource::Standing`).
fn load(ctx: &mut ActorCtx, game: &Game, actor: FormId, pov: Option<IdlePov>, idle: FormId, rng: &mut Rng, endless: bool) -> Result<(Arc<Clip>, IdlePlay, String), String> {
    let def = game.idle_def(idle).ok_or("not an idle animation")?;
    let path = clip_path_for(game, actor, pov, idle).ok_or("it names no file")?;
    let clip = ctx.clip(&path).ok_or_else(|| format!("{path} does not load"))?;
    let keys = IdleKeys::read(clip.start, clip.stop, &clip.text_keys);
    let passes = match def.loops(|n| rng.below(n)) {
        _ if endless => None,
        Loops::Forever => None,
        Loops::Times(n) => Some(n),
    };
    let play = effect_play(&clip, keys, passes);
    Ok((clip, play, path))
}

/// [design, #2685] CLAMP effect tracks keep their final values until replacement.
const HOLD_CLAMP_EFFECT_IDLES: bool = true;

fn effect_play(clip: &Clip, keys: IdleKeys, passes: Option<u32>) -> IdlePlay {
    let play = IdlePlay::new(keys, passes);
    if HOLD_CLAMP_EFFECT_IDLES && clip.cycle_type == 2 && (!clip.materials.is_empty() || !clip.emitters.is_empty()) {
        // [design, #2685] CLAMP overrides a record's repeat count for effect idles.
        IdlePlay::new(keys, Some(1)).clamped()
    } else {
        play
    }
}

/// The KF file an idle plays, as its record names it.
fn clip_path(game: &Game, idle: FormId) -> Option<String> {
    let model = game.idle_animation_path(idle)?;
    (!model.trim().is_empty()).then(||meshes_path(model))
}

/// The KF file `actor` plays for `idle`, applying any active kNVSE replacement whose
/// file name matches the idle's file name (stack top wins, case-insensitive) [hypothesis:
/// as `Rig::clip`, a kNVSE idle replacement shares the overridden file's name]. Vanilla
/// actors with no active overrides keep the exact record path.
fn clip_path_for(game: &Game, actor: FormId, pov: Option<IdlePov>, idle: FormId) -> Option<String> {
    let vanilla = clip_path(game, idle)?;
    if game.anim_paths.actor.is_empty() && game.anim_paths.weapon.is_empty() && game.anim_folders.active.is_empty() {
        return Some(vanilla);
    }
    let first = actor == FormId::PLAYER_REF
        && match pov {
            Some(IdlePov::First) => true,
            Some(IdlePov::Third) => false,
            None => game.senses.first_person,
        };
    let overrides = game.anim_overrides(actor, first);
    if overrides.is_empty() {
        return Some(vanilla);
    }
    let file_name = vanilla.replace('/', "\\");
    let file_name = file_name.rsplit('\\').next().unwrap_or(&file_name);
    overrides
        .iter()
        .rev()
        .find(|p| p.replace('/', "\\").rsplit('\\').next().is_some_and(|n| n.eq_ignore_ascii_case(file_name)))
        .map(|p| meshes_path(p))
        .or(Some(vanilla))
}

/// Strip a leading `meshes\` or `meshes/` prefix and normalize slashes for `set_sequence_info`.
fn strip_meshes_prefix(path: &str) -> String {
    let norm = path.replace('/', "\\");
    match norm.get(..7) {
        Some(head) if head.eq_ignore_ascii_case("meshes\\") => norm[7..].to_string(),
        _ => norm,
    }
}

/// Whether the clip of `idle` is loaded (or known not to load), so starting it reads no
/// file; when not, its read is started on a worker thread. An idle naming no file is
/// ready ([`load`] says why it does not play).
fn clip_ready(ctx: &mut ActorCtx, game: &Game, actor: FormId, pov: Option<IdlePov>, idle: FormId) -> bool {
    clip_path_for(game, actor, pov, idle).is_none_or(|p| ctx.clip_ready(&p))
}

/// Whether an idle asked for waits for its clip: not ready, and asked for less than
/// [`CLIP_WAIT`] ago.
fn waits_for_clip(ready: bool, waited: std::time::Duration) -> bool {
    !ready && waited < CLIP_WAIT
}

fn describe(game: &Game, idle: FormId, play: &IdlePlay) -> String {
    let k = play.keys;
    let loops = if play.endless() { ", looping until replaced" } else { "" };
    let range = k.loop_range.map_or(String::new(), |(a, b)| format!(", loop {a:.2}-{b:.2} s"));
    format!("{} ({:.2} s{range}{loops}, fades {:.2} / {:.2} s)", game.data.name(idle), k.length, k.blend_in, k.blend_out)
}

/// What [`drive`] reads and writes of a drawn actor.
type Drawn = (Entity, &'static RefVisual, &'static ActorRig, Option<&'static mut ActorIdle>, Option<&'static Locomotion>, Option<&'static WeaponHold>);

/// Take the idles asked for, run the collections, start and advance what plays.
#[allow(clippy::too_many_arguments)]
pub fn drive(
    mut commands: Commands,
    time: Res<Time>,
    mut st: ResMut<Idles>,
    mut player_idles: ResMut<PlayerIdles>,
    mut script: Option<ResMut<ScriptState>>,
    mut residents: Option<ResMut<Residents>>,
    mut sim: Option<ResMut<nvf_play::sim::SimState>>,
    player: Option<Res<Player>>,
    control: Option<Res<PlayerControl>>,
    mut parts: ResMut<ActorParts>,
    mut models: ModelCtx,
    mut actors: Query<Drawn>,
    held: Query<&HeldWeapon>,
    mut holds: ResMut<IdleHolds>,
) {
    let dt = time.delta_secs().min(0.1);
    let st = &mut *st;
    let pi = &mut *player_idles;
    let standing: HashMap<u32, Option<FormId>> = residents.as_deref().map(|r| r.standing().map(|s| (s.actor, s.marker)).collect()).unwrap_or_default();
    // The game state: the game being played, else the residents' own.
    let (game, played) = match script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
        Some(rt) => (Some(&mut rt.game), true),
        None => (residents.as_deref_mut().and_then(|r| r.own_game()), false),
    };
    let Some(game) = game else { return };
    // A script's menu (a message box, the character menus) holds the game, as the AI's
    // walks are held (`residents.rs`): the idles may start but stand still until it
    // closes, so a scene goes on where it was [observed: owner's TTW run 2026-11-02, the
    // stage that opens the trait menu starts the birth scene's last section, which
    // played out behind the menu; hypothesis: the original's menu mode stops it].
    // ... except the player's body at a menu played in the world (`PlayerIdles::under_menu`).
    let body_dt = if pi.under_menu { dt } else if game.menu.is_some() { 0.0 } else { dt };
    let dt = if game.menu.is_some() { 0.0 } else { dt };
    // Between frames the game being played keeps its world in the simulation
    // (`nvf_play::script::with_world`); conditions are evaluated with it put back.
    let mut with_world = |game: &mut Game<'static>, f: &mut dyn FnMut(&mut Game<'static>)| match sim.as_deref_mut().filter(|_| played) {
        Some(sim) => {
            std::mem::swap(&mut game.world, &mut sim.world);
            f(game);
            std::mem::swap(&mut game.world, &mut sim.world);
        }
        None => f(game),
    };
    let player_ref = FormId::PLAYER_REF.0;
    let package_of = |game: &Game, actor: u32| game.extra.get(&FormId(actor)).and_then(|e| e.current_package);
    // Seated by a Find or Use Item At package: its idles play on the seat ("Find: Idles
    // play when 'activating' the target of the find"; Use Item At "at its location"
    // [documented: Category:Packages, actor-behaviour.md §1.5]), laid over the sitting
    // clip as a seated speaker's gestures are. The G.O.A.T. students (CG03, TTW) write
    // their tests so (`LooseSitTakingTest*`); before, only standing actors played a
    // package's idles and they fidgeted in their chairs.
    let seated_idlers: HashSet<u32> = actors
        .iter()
        .map(|(_, v, ..)| v.0)
        .filter(|a| on_furniture(game, *a) && package_of(game, *a).and_then(|p| game.package_def(&p)).is_some_and(|d| matches!(d.ty, PackageType::Find | PackageType::UseItemAt)))
        .collect();

    // Idles that belong to a package or a marker that is no longer the actor's.
    let owner_now = |game: &Game, actor: u32, source: IdleSource| match source {
        IdleSource::MarkerIdles => standing.get(&actor).copied().flatten(),
        _ => package_of(game, actor),
    };
    for slot in [&mut pi.first, &mut pi.third] {
        slot.check_owner(|s| owner_now(game, player_ref, s));
    }
    let mut busy: HashMap<u32, bool> = HashMap::new();
    // The rules are told which NPCs hold their weapon in the hand (`IsWeaponOut`, as for
    // the player: once the weapon is on the hand [documented: catalogue notes]), and
    // which weapon that is (`GetWeaponAnimType` for NPCs with no weapon equipped in the
    // rules' inventories; the drawn one is the equipped one otherwise, outfits.rs).
    for h in &held {
        game.set_held_weapon(FormId(h.reference), Some(h.weapon));
    }
    for (_, v, _, _, _, hold) in &actors {
        if let Some(h) = hold {
            // GetAnimAction's drawing and putting away: `anim_state.rs`.
            game.set_weapon_out(FormId(v.0), h.anim.stage == Stage::Drawn);
        }
    }
    // Who is in a conversation, and their part in it (talking gestures).
    let talk_parts = game.talk_parts();
    // Scripts' StopIdle and ForceStopIdle (`fnv_game::nvse_plugins`).
    let stops = std::mem::take(&mut game.gaps.idle_stops);
    let pov_stops = std::mem::take(&mut game.gaps.idle_stop_pov);
    // The actors whose fidgets with the weapon away end now (`away_fidget_ends`): the
    // one playing, and one still waiting for its clip.
    let away_off: HashSet<u32> = actors
        .iter()
        .filter(|(_, v, _, _, lo, hold)| {
            let moving = lo.is_some_and(|l| l.velocity != [0.0; 2]);
            let talking = talk_parts.iter().any(|p| p.actor.0 == v.0);
            holstered_off(game, v.0, moving, talking, standing.get(&v.0).copied().flatten(), *hold)
        })
        .map(|(_, v, ..)| v.0)
        .collect();
    drop_holstered_waits(&mut st.waiting, &away_off);
    // Drop queued talking gestures whose actor started moving, left the conversation,
    // or entered a furniture transition while the clip was loading (#3407).
    let stale_gestures: HashSet<u32> = actors
        .iter()
        .filter(|(_, v, _, _, lo, _)| {
            let moving = lo.is_some_and(|l| l.velocity != [0.0; 2]);
            let talking = talk_parts.iter().any(|p| p.actor.0 == v.0);
            moving || !talking || in_furniture_transition(game, v.0)
        })
        .map(|(_, v, ..)| v.0)
        .collect();
    drop_stale_gesture_waits(&mut st.waiting, &stale_gestures);
    for (_, v, _, idle, lo, _) in &mut actors {
        let Some(mut idle) = idle else { continue };
        idle.0.check_owner(|s| owner_now(game, v.0, s));
        if stops.contains(&FormId(v.0)) || pov_stops.iter().any(|(a, _)| a.0 == v.0) {
            if let Some(c) = idle.0.cur.as_mut() {
                c.play.stop();
            }
        }
        // A collection's idle ends when its actor walks off or is drawn into a
        // conversation, so it can turn to the other and gesture (idles.md §12) [design].
        let walking = lo.is_some_and(|l| l.velocity != [0.0; 2]) || (!standing.contains_key(&v.0) && !seated_idlers.contains(&v.0));
        let talking = talk_parts.iter().any(|p| p.actor == FormId(v.0));
        if let Some(c) = idle.0.cur.as_mut().filter(|c| (walking || talking) && matches!(c.source, IdleSource::PackageIdles | IdleSource::MarkerIdles)) {
            c.play.stop();
        }
        // A fidget ends when its actor moves or puts the weapon away [design].
        let moving = lo.is_some_and(|l| l.velocity != [0.0; 2]);
        if let Some(c) = idle.0.cur.as_mut().filter(|c| c.source == IdleSource::Fidget && (moving || !weapon_out(game, v.0))) {
            c.play.stop();
        }
        // A fidget with the weapon away ends as `away_fidget_ends` says.
        end_holstered(&mut idle.0, away_off.contains(&v.0));
        // A creature's combat idle ends when it moves or is calm again [design].
        if let Some(c) = idle.0.cur.as_mut().filter(|c| c.source == IdleSource::CombatIdle && (moving || !game.draws_weapon(FormId(v.0)))) {
            c.play.stop();
        }
        // A calm creature's idle ends when it moves, is alerted or dies [design].
        if let Some(c) = idle.0.cur.as_mut().filter(|c| c.source == IdleSource::CalmIdle && (moving || game.draws_weapon(FormId(v.0)) || game.is_dead(FormId(v.0)))) {
            c.play.stop();
        }
        // A fidget on furniture ends when its actor starts getting off: it fades out over
        // the start of the getting-off clip, which starts where the body sits
        // (furniture.md §4) [design].
        if let Some(c) = idle.0.cur.as_mut().filter(|c| c.source == IdleSource::FurnitureFidget && !on_furniture(game, v.0)) {
            c.play.stop();
        }
        // So does one under a seated Find or Use Item At package with idles of its own:
        // they take its place (a fidget that loops for ever would hold them off; Susie
        // Mack and Christine Kendall in the G.O.A.T. sat down before their package began).
        let own_idles = seated_idlers.contains(&v.0) && game.package_idles(FormId(v.0)).is_some();
        if let Some(c) = idle.0.cur.as_mut().filter(|c| c.source == IdleSource::FurnitureFidget && own_idles) {
            c.play.stop();
        }
        // A cower goes on while its actor runs and ends when it no longer flees
        // (idles.md §13.2).
        if let Some(c) = idle.0.cur.as_mut().filter(|c| c.source == IdleSource::Cower && !fleeing(game, v.0)) {
            c.play.stop();
        }
        // A creature's standing look ends when it moves, fights or dies [design].
        if let Some(c) = idle.0.cur.as_mut().filter(|c| c.source == IdleSource::Standing && (moving || game.draws_weapon(FormId(v.0)) || game.is_dead(FormId(v.0)))) {
            c.play.stop();
        }
        // A gesture ends when its actor walks, is no longer in a conversation, or
        // changes furniture posture (e.g., starts getting off a chair while a seated
        // talk clip plays, or sits down during a standing gesture) [design, #3407].
        // A seated response idle (Speaker/Listener) likewise fades out when its actor
        // starts getting off furniture so its seated pose does not persist once the
        // exit clip ends [design, #3407].
        let seated_now = on_furniture(game, v.0);
        if let Some(c) = idle.0.cur.as_mut().filter(|c| {
            (c.source == IdleSource::Gesture && gesture_ends(moving, talking, c.seated, seated_now))
                || seated_dialogue_idle_ends(c.source, c.seated, seated_now)
        }) {
            c.play.stop();
        }
        // An idle that poses its actor's legs ends when the actor has somewhere to go
        // (`residents.rs`), or goes into combat (alerted, fighting or searching): it
        // breaks out of it [design: owner 2026-10-31, idles.md §15].
        if let Some(c) = idle.0.cur.as_mut().filter(|c| c.holds && !c.play.ended()) {
            let why = if holds.breaking.contains(&v.0) {
                Some("to walk")
            } else if game.awareness.state(FormId(v.0)).in_combat() {
                Some("for combat")
            } else {
                None
            };
            if let Some(why) = why {
                info!("idle hold: {} ({:08X}) breaks out of its idle {why}", game.data.name(FormId(v.0)), v.0);
                c.play.stop();
            }
        }
        busy.insert(v.0, idle.0.playing().is_some());
    }
    // The player stands still with a weapon ready in third person: may fidget [design,
    // owner 2026-10-16]; a fidget playing ends otherwise.
    let feet = player.as_deref().map(|p| p.feet);
    let moved = match (feet, st.player_feet) {
        (Some(a), Some(b)) => (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt() > PLAYER_STILL,
        _ => true,
    };
    st.player_feet = feet;
    let player_still = !moved
        && control.as_deref().is_some_and(|c| c.weapon_out && !c.aiming && !c.trigger && !c.reload && c.third_person_view())
        && weapon_out(game, player_ref)
        && package_of(game, player_ref).is_none();
    if let Some(c) = pi.third.cur.as_mut().filter(|c| c.source == IdleSource::Fidget && !player_still) {
        c.play.stop();
    }
    // On furniture in third person: may fidget there (the tree's player nodes want the
    // third person, and a fidget in first person would sway the view) [design].
    let player_seated = on_furniture(game, player_ref) && control.as_deref().is_some_and(|c| c.third_person_view()) && package_of(game, player_ref).is_none();
    if let Some(c) = pi.third.cur.as_mut().filter(|c| c.source == IdleSource::FurnitureFidget && !player_seated) {
        c.play.stop();
    }
    if stops.contains(&FormId(player_ref)) {
        for slot in [&mut pi.first.cur, &mut pi.third.cur].into_iter().flatten() {
            slot.play.stop();
        }
    }
    for (actor, pov) in &pov_stops {
        if actor.0 == player_ref {
            let slot = match pov {
                IdlePov::First => &mut pi.first.cur,
                IdlePov::Third => &mut pi.third.cur,
            };
            if let Some(c) = slot {
                c.play.stop();
            }
        }
    }
    busy.insert(player_ref, pi.first.playing().is_some() || pi.third.playing().is_some());
    // An idle asked for this frame (a package's begin idle) comes before its collection.
    for r in &game.idle_requests {
        busy.insert(r.actor.0, true);
    }
    // So is one waiting for its clip; a script's StopIdle drops it.
    st.waiting.retain(|(r, _)| !stops.contains(&r.actor));
    for (r, _) in &st.waiting {
        busy.insert(r.actor.0, true);
    }

    // Collections: the standing residents', the seated ones of Find and Use Item At packages
    // (`seated_idlers`), and the player's under a script package.
    let candidates = standing
        .iter()
        .map(|(a, m)| (*a, *m))
        .chain(seated_idlers.iter().filter(|a| !standing.contains_key(a)).map(|a| (*a, None)))
        .chain(package_of(game, player_ref).map(|_| (player_ref, None)));
    let mut live: Vec<u32> = Vec::new();
    let mut ask: Vec<(u32, FormId, IdleSource)> = Vec::new();
    for (actor, marker) in candidates {
        // A script's SetIdleMarkerAnimation(s) first (`fnv_game::nvse_jg4::idle_marker`),
        // copied out so the game can be changed below; the record's is borrowed.
        let edited = |m: FormId| game.other.idle_markers.get(&m).map(|c| Cow::Owned(c.clone()));
        let of_marker = marker
            .and_then(|m| edited(m).or_else(|| game.data.idle_markers.get(&m).map(Cow::Borrowed)).filter(|c| !c.idles.is_empty()).map(|c| ((true, m.0), c)));
        let quiet = package_of(game, actor).and_then(|p| game.package_def(&p)).is_some_and(|d| d.has(flags::NO_IDLE_ANIMS));
        let Some((key, collection)) = of_marker.or_else(|| game.package_idles(FormId(actor)).map(|(p, c)| ((false, p.0), Cow::Borrowed(c)))).filter(|_| !quiet) else { continue };
        live.push(actor);
        let c = st.collections.entry(actor).or_insert_with(|| Collection { key, picker: IdlePicker::default(), next: None, retry: 0.0 });
        if c.key != key {
            *c = Collection { key, picker: IdlePicker::default(), next: None, retry: 0.0 };
        }
        c.retry = (c.retry - dt).max(0.0);
        // In a conversation the collection waits; its gestures come from the tree.
        if busy.get(&actor).copied().unwrap_or(false) || c.retry > 0.0 || talk_parts.iter().any(|p| p.actor == FormId(actor)) {
            continue;
        }
        if c.next.is_none() {
            // The next idle of the collection whose own conditions the actor meets; when
            // none of them does, the collection is tried again later.
            let rng = &mut st.rng;
            with_world(game, &mut |game| {
                for _ in 0..collection.idles.len().max(1) {
                    c.next = c.picker.next(&collection, |n| rng.below(n as u32) as usize).filter(|(idle, _)| game.idle_allowed(FormId(actor), *idle));
                    if c.next.is_some() {
                        break;
                    }
                }
            });
            match c.next {
                None => c.retry = RETRY_SECONDS.max(collection.timer),
                // Its clip is read while the actor waits to play it.
                Some((idle, _)) => {
                    clip_ready(&mut ActorCtx { cache: &mut parts, models: &mut models }, game, FormId(actor), None, idle);
                }
            }
        }
        let Some((idle, wait)) = c.next.as_mut() else { continue };
        *wait -= dt;
        if *wait <= 0.0 {
            ask.push((actor, *idle, if key.0 { IdleSource::MarkerIdles } else { IdleSource::PackageIdles }));
            c.next = None;
        }
    }
    st.collections.retain(|a, _| live.contains(a));
    for (actor, idle, source) in ask {
        game.ask_idle(FormId(actor), idle, source);
    }

    st.clock += dt;
    // Panic: a live explosive in the pocket (pickpocket.md §5); once per explosive,
    // before anything else is picked.
    let mut panics: Vec<(u32, FormId)> = Vec::new();
    let mut live_now: HashSet<u32> = HashSet::new();
    if !game.pickpocket.planted.is_empty() {
        for (_, v, rig, ..) in &actors {
            if game.is_dead(FormId(v.0)) || !game.has_planted_explosive(FormId(v.0)) {
                continue;
            }
            live_now.insert(v.0);
            if !st.panicked.insert(v.0) {
                continue;
            }
            let mut pick = None;
            if let Some(folder) = skeleton_folder(&rig.skeleton) {
                with_world(game, &mut |game| pick = game.panic_idle(FormId(v.0), &folder, &|_| false));
            }
            match pick {
                Some(idle) => {
                    panics.push((v.0, idle));
                    busy.insert(v.0, true);
                }
                None => {
                    st.stats.no_panic += 1;
                    info!("idle: {} ({:08X}) has a live explosive and no panic idle for its kind: it stands", game.data.name(FormId(v.0)), v.0);
                }
            }
        }
    }
    st.panicked.retain(|a| live_now.contains(a));
    for (actor, idle) in panics {
        st.stats.panics += 1;
        game.ask_idle(FormId(actor), idle, IdleSource::Panic);
    }
    // Cowering: fleeing NPCs standing still (idles.md §13); before their fidgets.
    let cowering: Vec<(u32, String)> = actors
        .iter()
        .filter(|(_, v, ..)| !live.contains(&v.0) && fleeing(game, v.0) && !game.is_dead(FormId(v.0)) && game.furniture_use(FormId(v.0)).is_none())
        .filter(|(_, _, _, _, lo, _)| lo.is_some_and(|l| l.velocity == [0.0; 2]))
        .filter_map(|(_, v, rig, ..)| Some((v.0, skeleton_folder(&rig.skeleton)?)))
        .collect();
    st.cower_check.retain(|a, _| cowering.iter().any(|(c, _)| c == a));
    let mut cowers: Vec<(u32, FormId)> = Vec::new();
    for (actor, folder) in &cowering {
        let check = st.cower_check.entry(*actor).or_insert(0.0);
        *check -= dt;
        if *check > 0.0 || busy.get(actor).copied().unwrap_or(false) {
            continue;
        }
        *check = COWER_CHECK_SECONDS;
        let mut pick = None;
        with_world(game, &mut |game| pick = game.flee_idle(FormId(*actor), folder, &|_| false));
        if let Some(idle) = pick {
            cowers.push((*actor, idle));
            busy.insert(*actor, true);
        }
    }
    for (actor, idle) in cowers {
        st.stats.cowers += 1;
        game.ask_idle(FormId(actor), idle, IdleSource::Cower);
    }

    // Fidgets: NPCs on furniture, or standing still with a weapon out or away; the
    // player on furniture or with a weapon out.
    let mut still: Vec<(u32, String, FidgetKind)> = actors
        .iter()
        // An actor in a conversation has its talking gestures instead (idles.md §12).
        .filter(|(_, v, ..)| !live.contains(&v.0) && !game.is_dead(FormId(v.0)) && !talk_parts.iter().any(|p| p.actor.0 == v.0) && !cowering.iter().any(|(c, _)| *c == v.0))
        .filter_map(|(_, v, rig, _, lo, hold)| {
            let folder = skeleton_folder(&rig.skeleton)?;
            let still = lo.is_some_and(|l| l.velocity == [0.0; 2]);
            let kind = if game.furniture_use(FormId(v.0)).is_some() {
                on_furniture(game, v.0).then_some(FidgetKind::OnFurniture)?
            } else if folder.split('\\').any(|p| p == "creatures") {
                // Alerted: the combat idles; calm: the tree's ordinary idles (#2051).
                still.then_some(if game.draws_weapon(FormId(v.0)) { FidgetKind::Alerted } else { FidgetKind::Calm })?
            } else if still && weapon_out(game, v.0) {
                FidgetKind::WeaponOut
            } else {
                // The weapon away, not alerted, and nothing with idles of its own (a
                // package's collection or No Idle Anims: `Game::fidget_held_off`; a
                // marker's collection keeps its actor in `live`) (#2026).
                (still && holstered(game, v.0, hold) && !game.draws_weapon(FormId(v.0)) && !game.fidget_held_off(FormId(v.0))).then_some(FidgetKind::WeaponAway)?
            };
            Some((v.0, folder, kind))
        })
        .collect();
    if player_seated {
        still.push((player_ref, PEOPLE_FOLDER.to_string(), FidgetKind::OnFurniture));
    } else if player_still {
        still.push((player_ref, PEOPLE_FOLDER.to_string(), FidgetKind::WeaponOut));
    }
    let mut fidgets: Vec<(u32, FormId, FidgetKind)> = Vec::new();
    let mut stop_gesture: Vec<u32> = Vec::new();
    let still_set: HashSet<u32> = still.iter().map(|(s, ..)| *s).collect();
    st.fidget_check.retain(|a, _| still_set.contains(a));
    for (actor, folder, kind) in still {
        // The first look: one period after the actor stood still; for the weapon-away
        // fidgets spread over the second half of it by the actor's ID (`first_check`).
        let check = st.fidget_check.entry(actor).or_insert_with(|| first_check(actor, kind));
        *check -= dt;
        if *check > 0.0 || busy.get(&actor).copied().unwrap_or(false) {
            continue;
        }
        *check = FIDGET_CHECK_SECONDS;
        let (clock, last, data) = (st.clock, &st.fidget_last, game.data);
        // An idle is held back for its replay delay after it started [hypothesis: the
        // DATA replay delay is in seconds]. A calm creature's Idle-section leaves are its
        // standing looks (below).
        let held_back = |idle: FormId| {
            if kind == FidgetKind::Calm && data.idles.get(&idle).is_some_and(|d| d.section == 0) {
                return true;
            }
            let delay = data.idles.get(&idle).map_or(0.0, |d| d.replay_delay.max(0) as f32);
            delay > 0.0 && last.get(&(actor, idle.0)).is_some_and(|t| clock - t < delay)
        };
        let mut pick = None;
        let mut label = None;
        let away = kind == FidgetKind::WeaponAway;
        let new_away = away && !st.holstered.contains_key(&actor);
        let roll = st.holstered_roll.filter(|_| away);
        with_world(game, &mut |game| {
            // Test aid NVF_PLAY_TEST_HOLSTERED_ROLL: the walk's GetRandomPercent reads it.
            game.fixed_random_percent = roll;
            pick = game.fidget(FormId(actor), &folder, kind, &held_back);
            game.fixed_random_percent = None;
            // The run summary names each actor looked at by its base (a created one has
            // no Editor ID of its own).
            if new_away {
                label = Some(game.world.peek(FormId(actor)).map_or_else(|| game.data.name(FormId(actor)), |s| format!("{} {}", game.data.name(s.base), FormId(actor))));
            }
        });
        if let Some(label) = label {
            st.holstered.insert(actor, (label, HashMap::new()));
        }
        if let Some(idle) = pick {
            fidgets.push((actor, idle, kind));
        }
    }
    for (actor, idle, kind) in fidgets {
        st.fidget_last.insert((actor, idle.0), st.clock);
        game.ask_idle(FormId(actor), idle, kind.source());
    }

    // Standing looks: creatures standing still and calm play the tree's Idle-section leaf
    // their state calls for in place of their base idle (Mr. House's screen, #1542). The
    // look is asked again now and then: a changed state changes it, none ends it.
    let calm: Vec<(u32, String, Option<u32>)> = actors
        .iter()
        .filter(|(_, v, ..)| !live.contains(&v.0) && !game.is_dead(FormId(v.0)) && !game.draws_weapon(FormId(v.0)) && game.furniture_use(FormId(v.0)).is_none())
        .filter(|(_, v, ..)| !talk_parts.iter().any(|p| p.actor.0 == v.0) && !cowering.iter().any(|(c, _)| *c == v.0))
        .filter(|(_, _, _, _, lo, _)| lo.is_none_or(|l| l.velocity == [0.0; 2]))
        .filter_map(|(_, v, rig, slot, ..)| {
            let folder = skeleton_folder(&rig.skeleton).filter(|f| f.split('\\').any(|p| p == "creatures"))?;
            Some((v.0, folder, slot.and_then(|s| s.0.playing()).filter(|p| p.source == IdleSource::Standing).map(|p| p.idle)))
        })
        .collect();
    st.standing_check.retain(|a, _| calm.iter().any(|(c, ..)| c == a));
    let mut looks: Vec<(u32, FormId)> = Vec::new();
    let mut end_looks: Vec<u32> = Vec::new();
    for (actor, folder, look) in &calm {
        let check = st.standing_check.entry(*actor).or_insert(0.0);
        *check -= dt;
        // Another idle plays (not its own look): the look waits.
        if *check > 0.0 || (busy.get(actor).copied().unwrap_or(false) && look.is_none()) {
            continue;
        }
        *check = STANDING_CHECK_SECONDS;
        let mut pick = None;
        with_world(game, &mut |game| pick = game.standing_idle(FormId(*actor), folder));
        match (pick, look) {
            (Some(p), Some(l)) if p.0 == *l => {}
            (Some(p), _) => looks.push((*actor, p)),
            (None, Some(_)) => end_looks.push(*actor),
            (None, None) => {}
        }
    }
    for actor in end_looks {
        if let Some(c) = actors.iter_mut().find(|(_, v, ..)| v.0 == actor).and_then(|(_, _, _, s, ..)| s).and_then(|s| s.into_inner().0.cur.as_mut()) {
            c.play.stop();
        }
    }
    for (actor, idle) in looks {
        game.ask_idle(FormId(actor), idle, IdleSource::Standing);
    }

    // Talking gestures: NPCs drawn in a conversation (idles.md §12).
    // Those waiting for their clip were asked for too (#1429).
    let asked_now: Vec<u32> = game.idle_requests.iter().chain(st.waiting.iter().map(|(r, _)| r)).map(|r| r.actor.0).collect();
    st.gesturing.retain(|a, _| talk_parts.iter().any(|p| p.actor.0 == *a));
    let mut gestures: Vec<(u32, FormId, Vec<FormId>, TalkKey)> = Vec::new();
    for part in &talk_parts {
        let actor = part.actor.0;
        let Some((_, _, rig, slot, lo, _)) = actors.iter().find(|(_, v, ..)| v.0 == actor) else { continue };
        let Some(folder) = skeleton_folder(&rig.skeleton) else { continue };
        if game.is_dead(part.actor) {
            continue;
        }
        let key = TalkKey { talking: part.talking, info: part.info, response: part.response };
        // The first frame of a part counts as a change for the wait's log line.
        let fresh = !st.gesturing.contains_key(&actor);
        let g = st.gesturing.entry(actor).or_insert(Gesturing { key, retry: 0.0 });
        let changed = g.key != key;
        if changed {
            g.key = key;
            g.retry = 0.0;
        }
        g.retry = (g.retry - dt).max(0.0);
        let playing = slot.and_then(|s| s.0.playing());
        // Another idle (the response's own, a script's) plays, or was asked for this
        // frame: it comes first. A gesture goes on until the part changes.
        // A standing look gives way to the gestures (House's talking face).
        let other = playing.is_some_and(|p| !matches!(p.source, IdleSource::Gesture | IdleSource::Standing)) || asked_now.contains(&actor);
        let gesture_on = playing.is_some_and(|p| p.source == IdleSource::Gesture);
        // Walking, turning on the spot to face the other fast enough for the turn
        // clips (they step the feet; the walker's heading noise is far below it), or
        // getting onto/off furniture (idles.md §12.2) [design, #3407].
        let furniture_transition = in_furniture_transition(game, actor);
        let busy_body = lo.is_some_and(|l| l.velocity != [0.0; 2] || l.turn.abs() > fnv_actor::locomotion::TURN_RATE) || furniture_transition;
        if other || busy_body || (gesture_on && !changed) || (g.retry > 0.0 && !changed) {
            if (changed || fresh) && (other || busy_body) {
                let why = if other {
                    "another idle plays"
                } else if furniture_transition {
                    "it uses furniture"
                } else if lo.is_some_and(|l| l.velocity != [0.0; 2]) {
                    "it walks"
                } else {
                    "it turns on the spot"
                };
                info!("talk gesture: {} waits: {why}", game.data.name(part.actor));
            }
            continue;
        }
        let (clock, last, data) = (st.clock, &st.fidget_last, game.data);
        let held_back = |idle: FormId| {
            let delay = data.idles.get(&idle).map_or(0.0, |d| d.replay_delay.max(0) as f32);
            delay > 0.0 && last.get(&(actor, idle.0)).is_some_and(|t| clock - t < delay)
        };
        let mut pick = None;
        let partner = part.partner;
        with_world(game, &mut |game| pick = game.talk_gesture(FormId(actor), partner, &folder, &held_back));
        match pick {
            Some((idle, way)) => gestures.push((actor, idle, way, key)),
            None => {
                st.stats.no_gesture += 1;
                g.retry = GESTURE_RETRY_SECONDS;
                // The part changed and the tree has nothing for it: the old gesture ends.
                if changed && gesture_on {
                    stop_gesture.push(actor);
                }
            }
        }
    }
    for (actor, idle, way, key) in gestures {
        st.fidget_last.insert((actor, idle.0), st.clock);
        let data = game.data;
        let way: Vec<String> = way.iter().map(|n| data.name(*n)).collect();
        let line = key.info.map_or(String::new(), |i| format!(", line {} response {}", data.name(i), key.response + 1));
        info!("talk gesture: {} {} ({}{line}): {} by {}", data.name(FormId(actor)), if key.talking { "talks" } else { "listens" }, data.name(talk_parts.iter().find(|p| p.actor.0 == actor).map_or(FormId(0), |p| p.partner)), data.name(idle), way.join(" > "));
        if key.talking {
            st.stats.talk_gestures += 1;
        } else {
            st.stats.listen_gestures += 1;
        }
        game.ask_idle(FormId(actor), idle, IdleSource::Gesture);
    }

    for actor in stop_gesture {
        if let Some(mut s) = actors.iter_mut().find(|(_, v, ..)| v.0 == actor).and_then(|(_, _, _, s, _, _)| s) {
            if let Some(c) = s.0.cur.as_mut().filter(|c| c.source == IdleSource::Gesture) {
                c.play.stop();
            }
        }
    }

    // PickIdle: the tree's pick for each actor a script asked, by the actor's own folder
    // (`Game::pick_idle`); nothing when none fits [documented: GECK wiki PickIdle].
    for actor in std::mem::take(&mut game.idle_picks) {
        let folder = if actor.0 == player_ref { Some(PEOPLE_FOLDER.to_string()) } else { actors.iter().find(|(_, v, ..)| v.0 == actor.0).and_then(|(_, _, rig, ..)| skeleton_folder(&rig.skeleton)) };
        let name = game.data.name(actor);
        let Some(folder) = folder else {
            debug!("pick idle: {name} is not drawn: plays nothing");
            continue;
        };
        let (clock, last, data) = (st.clock, &st.fidget_last, game.data);
        let held_back = |idle: FormId| {
            let delay = data.idles.get(&idle).map_or(0.0, |d| d.replay_delay.max(0) as f32);
            delay > 0.0 && last.get(&(actor.0, idle.0)).is_some_and(|t| clock - t < delay)
        };
        let mut pick = None;
        with_world(game, &mut |game| pick = game.pick_idle(actor, &folder, &held_back));
        match pick {
            Some(idle) => {
                info!("pick idle: {name} picks {}", game.data.name(idle));
                st.fidget_last.insert((actor.0, idle.0), st.clock);
                game.ask_idle(actor, idle, IdleSource::Picked);
            }
            None => info!("pick idle: {name} finds no idle whose conditions it meets"),
        }
    }

    // Activating: the player's activation asks the idle tree's activation root, whose
    // conditions choose (`Game::activate_idle`: drinking from placeable water; idles.md
    // §16, #2349). Played as any idle asked for: a clip that drives the first-person
    // camera node on the first-person rig, any other on the third-person body.
    for actor in std::mem::take(&mut game.activate_idles) {
        let folder = if actor.0 == player_ref { Some(PEOPLE_FOLDER.to_string()) } else { actors.iter().find(|(_, v, ..)| v.0 == actor.0).and_then(|(_, _, rig, ..)| skeleton_folder(&rig.skeleton)) };
        let Some(folder) = folder else { continue };
        let what = game.activating(actor).map_or_else(|| "nothing".to_string(), |t| t.to_string());
        let (clock, last, data) = (st.clock, &st.fidget_last, game.data);
        let held_back = |idle: FormId| {
            let delay = data.idles.get(&idle).map_or(0.0, |d| d.replay_delay.max(0) as f32);
            delay > 0.0 && last.get(&(actor.0, idle.0)).is_some_and(|t| clock - t < delay)
        };
        let mut pick = None;
        with_world(game, &mut |game| pick = game.activate_idle(actor, &folder, &held_back));
        let name = game.data.name(actor);
        match pick {
            Some((idle, way)) => {
                let way: Vec<String> = way.iter().map(|i| game.data.name(*i)).collect();
                info!("activate idle: {name} activating {what}: {} by {}", game.data.name(idle), way.join(" > "));
                st.fidget_last.insert((actor.0, idle.0), st.clock);
                game.ask_idle(actor, idle, IdleSource::Activate);
            }
            None => debug!("activate idle: {name} activating {what}: the tree has no idle for it"),
        }
    }

    // Start what was asked for: those that waited for their clip first, in the order
    // asked. An actor's later asks wait behind one that waits, so its idles start in
    // the order asked, as they did in one frame.
    let fresh = std::mem::take(&mut game.idle_requests);
    let mut requests: Vec<(IdleRequest, Option<std::time::Instant>)> = std::mem::take(&mut st.waiting).into_iter().map(|(r, t)| (r, Some(t))).collect();
    requests.extend(fresh.into_iter().map(|r| (r, None)));
    if !requests.is_empty() {
        let mut ctx = ActorCtx { cache: &mut parts, models: &mut models };
        let drawn: HashMap<u32, Entity> = actors.iter().map(|(e, v, ..)| (v.0, e)).collect();
        let mut held_up: HashSet<u32> = HashSet::new();
        for (request, since) in requests {
            let IdleRequest { actor, idle, source, pov } = request;
            // Counted once, when asked for (not again after waiting for its clip).
            if since.is_none() {
                if !matches!(source, IdleSource::Fidget | IdleSource::HolsteredFidget | IdleSource::FurnitureFidget | IdleSource::CombatIdle | IdleSource::CalmIdle | IdleSource::Gesture | IdleSource::Cower | IdleSource::Stimpak) {
                    st.stats.asked += 1;
                }
                match source {
                    IdleSource::Script | IdleSource::Picked => st.stats.script += 1,
                    IdleSource::PackageBegin | IdleSource::PackageEnd | IdleSource::PackageChange => st.stats.package += 1,
                    IdleSource::PackageIdles | IdleSource::MarkerIdles => st.stats.collection += 1,
                    IdleSource::Speaker | IdleSource::Listener => st.stats.dialogue += 1,
                    IdleSource::Fidget | IdleSource::HolsteredFidget | IdleSource::FurnitureFidget | IdleSource::CombatIdle | IdleSource::CalmIdle | IdleSource::Gesture | IdleSource::Cower | IdleSource::Stimpak | IdleSource::PickUp | IdleSource::Standing | IdleSource::Panic | IdleSource::Activate => {}
                }
            }
            let name = game.data.name(actor);
            let failed = |st: &mut Idles, why: &str| {
                if let Some(c) = st.collections.get_mut(&actor.0).filter(|_| matches!(source, IdleSource::PackageIdles | IdleSource::MarkerIdles)) {
                    c.retry = RETRY_SECONDS;
                }
                debug!("idle: {name} does not play {} ({source:?}): {why}", game.data.name(idle));
            };
            if actor.0 != player_ref && !drawn.contains_key(&actor.0) {
                st.stats.not_drawn += 1;
                failed(st, "the actor is not drawn");
                continue;
            }
            // Its clip read on a worker thread rather than in this frame (#1429).
            let ready = clip_ready(&mut ctx, game, actor, pov, idle);
            let waited = since.map_or(std::time::Duration::ZERO, |t| t.elapsed());
            if held_up.contains(&actor.0) || waits_for_clip(ready, waited) {
                if since.is_none() && !ready {
                    st.stats.clip_waits += 1;
                }
                held_up.insert(actor.0);
                st.waiting.push((request, since.unwrap_or_else(std::time::Instant::now)));
                continue;
            }
            if since.is_some() {
                st.stats.longest_clip_wait_ms = st.stats.longest_clip_wait_ms.max(waited.as_secs_f64() * 1000.0);
            }
            if !ready {
                st.stats.clip_read_late += 1;
                debug!("idle: {name}: the clip of {} was not read in {} ms: read now", game.data.name(idle), CLIP_WAIT.as_millis());
            }
            let (clip, play, resolved_path) = match load(&mut ctx, game, actor, pov, idle, &mut st.rng, source == IdleSource::Standing) {
                Ok(l) => l,
                Err(why) => {
                    st.stats.no_clip += 1;
                    warn!("idle: {name} cannot play {} ({source:?}): {why}", game.data.name(idle));
                    failed(st, &why);
                    continue;
                }
            };
            let rel_path = strip_meshes_prefix(&resolved_path);
            game.set_sequence_info(&rel_path, SequenceInfo {
                name: clip.name.clone(),
                accum_root: clip.accum_root.clone().unwrap_or_default(),
                frequency: f64::from(clip.frequency),
                begin: f64::from(clip.start),
                end: f64::from(clip.stop),
                cycle: clip.cycle_type,
                text_keys: clip.text_keys.iter().map(|(t, s)| (f64::from(*t), s.clone())).collect(),
            });
            let owner = owner_now(game, actor.0, source).filter(|_| source.of_package());
            let file = Some(resolved_path);
            let what = describe(game, idle, &play);
            if actor.0 == player_ref {
                let drives_camera = clip.tracks.iter().any(|(n, _)| n == CAMERA_NODE);
                let first = match pov {
                    Some(IdlePov::First) => true,
                    Some(IdlePov::Third) => false,
                    None => drives_camera,
                };
                // An activation's idle (the drink) holds nothing: mouse look, movement
                // and the HUD stay with the player while the camera node moves the view
                // (`Camera1st` hangs under `Bip01 Looking`, so the player's pitch still
                // applies) [design, #2349: whether the original holds the player is
                // observer check 131; documented: `_1stperson\skeleton.nif`]. A first-person
                // arm-only idle (`ForcePlayIdle idle 2`) without `Camera1st` likewise keeps
                // look and movement free [design].
                let holds = first && drives_camera && source != IdleSource::Activate && camera_idle_holds(game.idle_def(idle).map_or(0, |d| d.section));
                if holds && pi.held.is_none() {
                    pi.held = player.as_deref().map(|p| Held { feet: p.placed.unwrap_or(p.feet), free: p.feet, heading: p.heading, pitch: p.pitch });
                }
                let started = if first { &mut pi.first } else { &mut pi.third }.start_file(idle.0, source, owner, clip, play, file);
                if first {
                    if let Some(current) = pi.first.cur.as_mut() {
                        current.holds = holds;
                    }
                }
                match source {
                    IdleSource::Fidget => st.stats.player_fidgets += 1,
                    IdleSource::FurnitureFidget => st.stats.player_furniture_fidgets += 1,
                    _ => st.stats.player_started += 1,
                }
                let facing = player.as_deref().map_or(String::new(), |p| {
                    // A first-person idle plays where the player is held.
                    let at = pi.held.filter(|_| first).map_or(p.feet, |h| h.feet);
                    format!(", at ({:.1}, {:.1}, {:.1}) facing {:.0} degrees, pitch {:.0}", at[0], at[1], at[2], p.heading.to_degrees(), p.pitch.to_degrees())
                });
                let again = if started { "" } else { " again: not started over" };
                info!("idle: the player plays {what}{again} on the {} rig ({source:?}){facing}", if first { "first-person" } else { "third-person" });
                continue;
            }
            let Ok((e, _, rig, slot, lo, _)) = actors.get_mut(drawn[&actor.0]) else { continue };
            // Another creature's clip is not played (`foreign_creature_clip`, #2747).
            if let Some(theirs) = file.as_deref().and_then(|f| foreign_creature_clip(&rig.skeleton, f)) {
                st.stats.no_clip += 1;
                st.stats.foreign_clips += 1;
                info!("idle: {name} does not play {} ({source:?}): its clip is made for the skeleton in {theirs}, not this actor's", game.data.name(idle));
                failed(st, "its clip is another creature's");
                continue;
            }
            let section = game.idle_def(idle).map_or(0, |d| d.section);
            let holds = holds_in_place(source, section, &clip, lo.and_then(walk_clip));
            let bound = bind_clip(&rig.skel, clip);
            // A stimpak plays on the upper body only (`npc_stimpaks.rs`).
            let bound = if source == IdleSource::Stimpak { crate::npc_stimpaks::upper_body(bound, &rig.skel.names) } else { bound };
            if bound.tracks.is_empty() {
                st.stats.no_clip += 1;
                failed(st, "its clip drives no node of the actor's skeleton");
                continue;
            }
            let bound = Arc::new(bound);
            let seated = on_furniture(game, actor.0);
            let started = match slot {
                Some(mut s) => {
                    let started = s.0.start_file(idle.0, source, owner, bound, play, file);
                    if let Some(c) = s.0.cur.as_mut() {
                        c.holds = holds;
                        if started {
                            c.seated = seated;
                        }
                    }
                    started
                }
                None => {
                    let mut s = ActorIdle::default();
                    s.0.start_file(idle.0, source, owner, bound, play, file);
                    if let Some(c) = s.0.cur.as_mut() {
                        c.holds = holds;
                        c.seated = seated;
                    }
                    commands.entity(e).insert(s);
                    true
                }
            };
            match source {
                IdleSource::Fidget => st.stats.actor_fidgets += 1,
                IdleSource::HolsteredFidget => {
                    st.stats.holstered_fidgets += 1;
                    if let Some((_, by_idle)) = st.holstered.get_mut(&actor.0) {
                        *by_idle.entry(game.data.name(idle)).or_default() += 1;
                    }
                }
                IdleSource::FurnitureFidget => st.stats.actor_furniture_fidgets += 1,
                IdleSource::CombatIdle => st.stats.combat_idles += 1,
                IdleSource::CalmIdle => st.stats.calm_idles += 1,
                IdleSource::Gesture | IdleSource::Cower | IdleSource::Stimpak => {}
                _ => st.stats.started += 1,
            }
            match source {
                IdleSource::PackageIdles | IdleSource::MarkerIdles | IdleSource::Gesture => debug!("idle: {name} plays {what} ({source:?})"),
                _ if !started => info!("idle: {name} plays {what} again: not started over ({source:?})"),
                _ => info!("idle: {name} plays {what} ({source:?}){}", if holds { ", its legs held" } else { "" }),
            }
        }
    }

    // Advance, and tell the rules what plays.
    let mut now: HashMap<u32, u32> = HashMap::new();
    let mut holding: HashSet<u32> = HashSet::new();
    let mut carried_off: HashSet<u32> = HashSet::new();
    for (e, v, _, idle, _, _) in &mut actors {
        let Some(mut idle) = idle else { continue };
        idle.0.advance(dt);
        // Held while it shows, its fade-out too (one gone is no longer `cur`).
        if let Some(p) = idle.0.cur.as_ref().filter(|p| p.holds) {
            holding.insert(v.0);
            if carries_body(&p.clip.clip) {
                carried_off.insert(v.0);
            }
        }
        match idle.0.playing() {
            // A script's animation group is not an idle.
            Some(p) if p.group.is_some() => {}
            Some(p) => {
                now.insert(v.0, p.idle);
            }
            None if idle.0.is_empty() => {
                commands.entity(e).remove::<ActorIdle>();
            }
            None => {}
        }
    }
    pi.first.advance(dt);
    pi.third.advance(body_dt);
    if let Some(p) = pi.first.playing().or(pi.third.playing()) {
        now.insert(player_ref, p.idle);
    }
    for (actor, idle) in &now {
        if st.told.get(actor) != Some(idle) {
            game.set_idle_playing(FormId(*actor), Some(FormId(*idle)));
        }
    }
    for actor in st.told.keys().filter(|a| !now.contains_key(a)) {
        game.set_idle_playing(FormId(*actor), None);
    }
    st.told = now;
    for a in holding.difference(&holds.held) {
        info!("idle hold: {} ({a:08X}) stands while its idle shows", game.data.name(FormId(*a)));
    }
    for a in holds.held.difference(&holding) {
        info!("idle hold: {} ({a:08X}) may walk again: its idle has gone", game.data.name(FormId(*a)));
    }
    holds.held = holding;
    holds.carried_off = carried_off;
}

/// Whether `actor` flees from a fight (`GetCurrentAIProcedure` 16).
fn fleeing(game: &Game, actor: u32) -> bool {
    let a = FormId(actor);
    game.awareness.state(a).in_combat() && game.awareness.tactics.flights.contains_key(&a)
}

/// Whether `actor` is on furniture, neither getting on nor off (`GetSitting` or
/// `GetSleeping` 3).
fn on_furniture(game: &Game, actor: u32) -> bool {
    game.furniture_use(FormId(actor)).is_some_and(|u| u.state == 3)
}

/// Whether `actor` is getting onto or off furniture (`GetSitting` / `GetSleeping`
/// state other than 3, such as 2 GettingOn or 4 GettingOff) [design, #3407].
fn in_furniture_transition(game: &Game, actor: u32) -> bool {
    game.furniture_use(FormId(actor)).is_some_and(|u| u.state != 3)
}

/// Whether a talking gesture ends: its actor walks, is no longer in a conversation, or
/// changed furniture posture relative to when the gesture started (e.g., started getting
/// off a chair while a seated talk clip plays, or sat down during a standing gesture)
/// [design, #3407].
fn gesture_ends(moving: bool, talking: bool, gesture_seated: bool, now_seated: bool) -> bool {
    moving || !talking || gesture_seated != now_seated
}

/// Whether a dialogue response's own Speaker/Listener idle ends because it started while
/// seated on furniture (`state == 3`) and the actor has started getting off or stood up
/// [design, #3407].
fn seated_dialogue_idle_ends(source: IdleSource, idle_seated: bool, now_seated: bool) -> bool {
    idle_seated && !now_seated && matches!(source, IdleSource::Speaker | IdleSource::Listener)
}

/// Talking gestures still waiting for their clip, of actors in `stale` (moving, no
/// longer talking, or getting on/off furniture), are dropped so they do not start during
/// or after a furniture transition [design, #3407].
fn drop_stale_gesture_waits(waiting: &mut Vec<(IdleRequest, std::time::Instant)>, stale: &HashSet<u32>) {
    waiting.retain(|(r, _)| !(r.source == IdleSource::Gesture && stale.contains(&r.actor.0)));
}

/// Whether a fidget with the weapon away ends: its actor moves, its weapon is no longer
/// away (drawn, or coming out), it is alerted, it is dead, it is drawn into a
/// conversation (so it can turn to the other and gesture, as a collection's idle does),
/// or something with idles of its own takes it over (`taken`: furniture, a package with
/// an idle collection or flagged No Idle Anims, `Game::fidget_held_off`; an idle marker
/// with a collection) [design, #2026]. The drawn-weapon fidgets' rule (the weapon away
/// ends them) is not this one.
fn away_fidget_ends(moving: bool, holstered: bool, alerted: bool, dead: bool, talking: bool, taken: bool) -> bool {
    moving || !holstered || alerted || dead || talking || taken
}

/// The fidget with the weapon away playing on `slot` fades out when `ends`; nothing
/// else is touched. Returns whether one was stopped.
fn end_holstered<C>(slot: &mut IdleSlot<C>, ends: bool) -> bool {
    match slot.cur.as_mut().filter(|c| ends && c.source == IdleSource::HolsteredFidget && !c.play.ended()) {
        Some(c) => {
            c.play.stop();
            true
        }
        None => false,
    }
}

/// Fidgets with the weapon away still waiting for their clip, of the actors in `off`,
/// are dropped: what took over is not held up behind them.
fn drop_holstered_waits(waiting: &mut Vec<(IdleRequest, std::time::Instant)>, off: &HashSet<u32>) {
    waiting.retain(|(r, _)| !(r.source == IdleSource::HolsteredFidget && off.contains(&r.actor.0)));
}

/// Whether the idle marker `marker` has idles to play: a script's
/// SetIdleMarkerAnimation(s), else the record's.
fn marker_has_idles(game: &Game, marker: FormId) -> bool {
    match game.other.idle_markers.get(&marker) {
        Some(c) => !c.idles.is_empty(),
        None => game.data.idle_markers.get(&marker).is_some_and(|c| !c.idles.is_empty()),
    }
}

/// Seconds to an actor's first look for a fidget of `kind` once it stands still: one
/// [`FIDGET_CHECK_SECONDS`], as before #2026, for every kind but the weapon-away
/// fidgets. Those, asked of every NPC standing holstered (a town's people stop
/// together when the player arrives), come within the second half of the period, spread
/// by the actor's ID (the same each run), so they do not all walk the tree in one frame
/// and a brief stop does not get a look much sooner than a full period [engineering
/// choice, #2026].
fn first_check(actor: u32, kind: FidgetKind) -> f32 {
    if kind != FidgetKind::WeaponAway {
        return FIDGET_CHECK_SECONDS;
    }
    let spread = (actor.wrapping_mul(0x9E37_79B1) >> 22) as f32 / 1024.0;
    FIDGET_CHECK_SECONDS * (0.5 + 0.5 * spread)
}

/// Whether the fidget with the weapon away of `actor` (playing, or waiting for its clip)
/// ends now, as `drive` judges it each frame (`away_fidget_ends`): `moving` and
/// `talking` as the animating layer sees them, `marker` the idle marker it stands at,
/// `hold` its weapon hand.
fn holstered_off(game: &Game, actor: u32, moving: bool, talking: bool, marker: Option<FormId>, hold: Option<&WeaponHold>) -> bool {
    let a = FormId(actor);
    let taken = game.fidget_held_off(a) || marker.is_some_and(|m| marker_has_idles(game, m));
    away_fidget_ends(moving, holstered(game, actor, hold), game.draws_weapon(a), game.is_dead(a), talking, taken)
}

/// `NVF_PLAY_TEST_HOLSTERED_ROLL` ([`Idles`]), logged when set.
fn holstered_roll_aid() -> Option<u32> {
    let roll = std::env::var("NVF_PLAY_TEST_HOLSTERED_ROLL").ok().and_then(|v| v.trim().parse::<u32>().ok()).map(|v| v.min(99));
    if let Some(r) = roll {
        info!("test aid: GetRandomPercent reads {r} in the walks for fidgets with the weapon away (NVF_PLAY_TEST_HOLSTERED_ROLL)");
    }
    roll
}

/// Whether `actor` stands with its weapon away: the rules do not hold it out
/// (`IsWeaponOut` 0) and its hand is not drawing it, holding it or putting it away
/// (no weapon at all counts as away).
fn holstered(game: &Game, actor: u32, hold: Option<&WeaponHold>) -> bool {
    !weapon_out(game, actor) && hold.is_none_or(|h| !h.drawn && h.anim.stage == Stage::Holstered)
}

/// Whether the rules hold `actor`'s weapon out (`IsWeaponOut`).
fn weapon_out(game: &Game, actor: u32) -> bool {
    game.extra.get(&FormId(actor)).is_some_and(|e| e.weapon_out)
}

/// The creature folder (`creatures\<name>`) a clip lives in, when it is not the folder
/// of `skeleton`, the creature skeleton that would play it (#2747).
///
/// An idle names its clip by path, and a dialogue response can name any idle for its
/// speaker. Mr. House's lines about Ranger Grant (INFO 0x001418A1) give his penthouse
/// screen (`creatures\nvpenthouemaincomputer`) the idles `MrHouseTalkA`/`MrHouseTalkB`,
/// whose clips are the pod's (`creatures\nvmrhouse\idleanims\specialidle_talka.kf`)
/// [observed: FalloutNV.esm, `fnvtool nif decode`]. The pod's clip keys `Bip01 Yaw` at a
/// quarter turn about z and `Bip01 Pelvis` at a quarter turn on two axes, the pod
/// skeleton's own rest pose for those bones; the screen's skeleton has bones of the same
/// names resting at no turn, so played there the screen swung 90° and tipped. A
/// creature's clips are made for the skeleton of their folder, so a creature plays only
/// its own folder's clips [design; the owner: the monitors never move]. People
/// (`characters\`) and clips outside `creatures\` are not judged here.
fn foreign_creature_clip(skeleton: &str, clip: &str) -> Option<String> {
    let creature = |p: &str| -> Option<String> {
        let p = p.to_ascii_lowercase().replace('/', "\\");
        let p = p.strip_prefix("meshes\\").unwrap_or(&p).to_string();
        let mut parts = p.split('\\');
        (parts.next()? == "creatures").then(|| parts.next().map(|n| format!("creatures\\{n}"))).flatten()
    };
    let own = creature(&skeleton_folder(skeleton)?)?;
    let theirs = creature(clip)?;
    (own != theirs).then_some(theirs)
}

/// The animation folder of a skeleton below `meshes\`
/// (`meshes\characters\_male\skeleton.nif` → `characters\_male`).
fn skeleton_folder(skeleton: &str) -> Option<String> {
    let s = skeleton.to_ascii_lowercase().replace('/', "\\");
    let s = s.strip_prefix("meshes\\").unwrap_or(&s);
    Some(s.rsplit_once('\\')?.0.to_string())
}

/// The idle line of the run summary.
pub fn report(mut exit: MessageReader<AppExit>, st: Res<Idles>) {
    if exit.read().next().is_none() {
        return;
    }
    let s = st.stats;
    info!(
        "idle summary: {} idles asked for ({} by scripts, {} by packages' begin, end and change, {} from collections, {} by dialogue lines); {} started on actors, {} on the player; {} for actors not drawn, {} without a usable clip ({} of them another creature's)",
        s.asked, s.script, s.package, s.collection, s.dialogue, s.started, s.player_started, s.not_drawn, s.no_clip, s.foreign_clips
    );
    info!("fidget summary: {} drawn-weapon fidgets started on actors, {} on the player", s.actor_fidgets, s.player_fidgets);
    info!("holstered fidget summary: {} fidgets started on NPCs standing with their weapon away; {} NPCs looked at", s.holstered_fidgets, st.holstered.len());
    // One line per NPC looked at, so a run can tell who fidgeted and who did not.
    let mut each: Vec<(&String, &HashMap<String, u32>)> = st.holstered.values().map(|(l, b)| (l, b)).collect();
    each.sort_by(|a, b| a.0.cmp(b.0));
    if each.len() > HOLSTERED_LINES {
        info!("holstered fidgets: {} NPCs looked at; the first {HOLSTERED_LINES} by name follow", each.len());
    }
    for (label, by_idle) in each.into_iter().take(HOLSTERED_LINES) {
        let mut picks: Vec<(&String, &u32)> = by_idle.iter().collect();
        picks.sort();
        let list: Vec<String> = picks.iter().map(|(i, n)| format!("{i} {n}")).collect();
        info!("holstered fidgets: {label}: {} started{}", by_idle.values().sum::<u32>(), if list.is_empty() { String::new() } else { format!(" ({})", list.join(", ")) });
    }
    info!("creature combat idle summary: {} combat idles started on alerted creatures", s.combat_idles);
    info!("creature calm idle summary: {} idles from the tree started on calm creatures standing still", s.calm_idles);
    info!(
        "idle clip summary: {} idles waited for their clip to be read on a worker thread (longest {:.0} ms); {} read in the frame they started after {} ms",
        s.clip_waits,
        s.longest_clip_wait_ms,
        s.clip_read_late,
        CLIP_WAIT.as_millis()
    );
    info!("furniture fidget summary: {} fidgets on furniture started on actors, {} on the player", s.actor_furniture_fidgets, s.player_furniture_fidgets);
    info!("talk gesture summary: {} gestures picked while talking, {} while listening; {} walks of the tree gave none", s.talk_gestures, s.listen_gestures, s.no_gesture);
    info!("cower summary: {} cower idles started on fleeing actors", s.cowers);
    if s.panics + s.no_panic > 0 {
        info!("panic summary: {} panic idles started on actors with a live explosive, {} with none for their kind", s.panics, s.no_panic);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_creature_plays_only_its_own_folders_clips() {
        let screen = r"meshes\creatures\NVPenthoueMainComputer\Skeleton.nif";
        // The pod's talk clip on the penthouse screen (#2747).
        assert_eq!(foreign_creature_clip(screen, r"meshes\creatures\NVMrHouse\IdleAnims\SpecialIdle_TalkA.kf").as_deref(), Some(r"creatures\nvmrhouse"));
        // The screen's own clips, in a subfolder or not, in any case and either slash.
        assert_eq!(foreign_creature_clip(screen, r"meshes\creatures\nvpenthouemaincomputer\idleanims\specialidle_housetalk01.kf"), None);
        assert_eq!(foreign_creature_clip(screen, "meshes/Creatures/NVPenthoueMainComputer/talking.kf"), None);
        // People and clips outside creatures\ are not judged.
        assert_eq!(foreign_creature_clip(r"meshes\characters\_male\skeleton.nif", r"meshes\creatures\nvmrhouse\idleanims\specialidle_talka.kf"), None);
        assert_eq!(foreign_creature_clip(screen, r"meshes\characters\_male\idleanims\talk.kf"), None);
    }

    #[test]
    fn renderer_idle_path_reads_the_script_setter_override() {
        use fnv_script::vm::XVal;
        let empty = crate::outfits::tests::empty_world_data();
        let forms = crate::outfits::tests::actor_forms(vec![]);
        let refs = fnv_sim::RefTable::default();
        let functions = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
        let table = fnv_script::FunctionTable::load(&functions).unwrap();
        let json = serde_json::from_str(&std::fs::read_to_string(functions).unwrap()).unwrap();
        let mut data = fnv_game::GameData::build(empty.lo,empty.schema,table,&json,&refs);
        let idle = FormId(0x7A00);
        data.idles.insert(idle,fnv_game::data::IdleDef {model:Some("Owned/Loop.kf".into()),..Default::default()});
        let mut game = Game::new(&data,fnv_sim::World::new(&forms,&refs,Default::default()),Default::default());
        assert_eq!(clip_path(&game,idle).as_deref(),Some("meshes\\owned\\loop.kf"));
        fnv_game::nvse::call(&mut game,&Default::default(),"setidleanimpath",None,&[XVal::Ref(idle.0),XVal::Str("Owned/Stop.kf".into())]).unwrap().unwrap();
        assert_eq!(clip_path(&game,idle).as_deref(),Some("meshes\\owned\\stop.kf"));
        fnv_game::nvse::call(&mut game,&Default::default(),"setidleanimpath",None,&[XVal::Ref(idle.0),XVal::Str("Owned/Loop.kf".into())]).unwrap().unwrap();
        assert_eq!(clip_path(&game,idle).as_deref(),Some("meshes\\owned\\loop.kf"));
    }

    #[test]
    fn renderer_idle_path_and_definition_admit_temp_cloned_idles() {
        use fnv_script::vm::XVal;
        let empty = crate::outfits::tests::empty_world_data();
        let forms = crate::outfits::tests::actor_forms(vec![]);
        let refs = fnv_sim::RefTable::default();
        let functions = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
        let table = fnv_script::FunctionTable::load(&functions).unwrap();
        let json = serde_json::from_str(&std::fs::read_to_string(functions).unwrap()).unwrap();
        let mut data = fnv_game::GameData::build(empty.lo, empty.schema, table, &json, &refs);
        let orig = FormId(0x7A01);
        data.idles.insert(
            orig,
            fnv_game::data::IdleDef {
                model: Some("Owned/Orig.kf".into()),
                section: 4,
                no_attacking: true,
                ..Default::default()
            },
        );
        let mut game = Game::new(&data, fnv_sim::World::new(&forms, &refs, Default::default()), Default::default());
        let XVal::Ref(clone_raw) = fnv_game::nvse::call(&mut game, &Default::default(), "tempcloneform", None, &[XVal::Ref(orig.0), XVal::Num(1.0)]).unwrap().unwrap() else {
            panic!("expected clone FormId");
        };
        let clone = FormId(clone_raw);
        assert_eq!(game.idle_def(clone).map(|d| (d.section, d.no_attacking)), Some((4, true)));
        assert_eq!(clip_path(&game, clone).as_deref(), Some("meshes\\owned\\orig.kf"));

        // Overriding the clone's path does not alter the original's resolved clip path.
        fnv_game::nvse::call(&mut game, &Default::default(), "setidleanimpath", None, &[XVal::Ref(clone.0), XVal::Str("Owned/Clone.kf".into())]).unwrap().unwrap();
        assert_eq!(clip_path(&game, clone).as_deref(), Some("meshes\\owned\\clone.kf"));
        assert_eq!(clip_path(&game, orig).as_deref(), Some("meshes\\owned\\orig.kf"));

        // Non-IDLE clone has no definition and no clip path.
        let non_idle = FormId(0x7A02);
        let XVal::Ref(bad_raw) = fnv_game::nvse::call(&mut game, &Default::default(), "tempcloneform", None, &[XVal::Ref(non_idle.0), XVal::Num(1.0)]).unwrap().unwrap() else {
            panic!("expected non-idle clone FormId");
        };
        let bad_clone = FormId(bad_raw);
        assert!(game.idle_def(bad_clone).is_none());
        assert_eq!(clip_path(&game, bad_clone), None);
    }

    #[test]
    fn a_fidget_with_the_weapon_away_plays_on_while_its_actor_stands_holstered() {
        // Standing still, the weapon away, calm, alive, not talking, nothing taking over:
        // it plays on (the drawn-weapon fidgets' rule would end it, the weapon being away).
        assert!(!away_fidget_ends(false, true, false, false, false, false));
        // Each of these ends it.
        assert!(away_fidget_ends(true, true, false, false, false, false), "it walks");
        assert!(away_fidget_ends(false, false, false, false, false, false), "it draws its weapon");
        assert!(away_fidget_ends(false, true, true, false, false, false), "it is alerted");
        assert!(away_fidget_ends(false, true, false, true, false, false), "it dies");
        assert!(away_fidget_ends(false, true, false, false, true, false), "it is drawn into a conversation");
        assert!(away_fidget_ends(false, true, false, false, false, true), "furniture or a package with idles of its own takes over");
        // Its own source: the drawn-weapon fidgets' and the stimpaks' rules know it apart.
        assert_eq!(FidgetKind::WeaponAway.source(), IdleSource::HolsteredFidget);
        assert_ne!(FidgetKind::WeaponAway.source(), FidgetKind::WeaponOut.source());
    }

    #[test]
    fn an_idle_waits_for_its_clip_for_a_bounded_time() {
        use std::time::Duration;
        assert!(waits_for_clip(false, Duration::ZERO));
        assert!(waits_for_clip(false, CLIP_WAIT - Duration::from_millis(1)));
        // Past the bound it is read in the frame it starts; a clip in is never waited for.
        assert!(!waits_for_clip(false, CLIP_WAIT));
        assert!(!waits_for_clip(true, Duration::ZERO));
    }

    fn play(length: f32, blend_in: f32, passes: Option<u32>) -> IdlePlay {
        IdlePlay::new(IdleKeys { length, loop_range: None, blend_in, blend_out: 0.25 }, passes)
    }

    fn weights(s: &IdleSlot<u8>) -> Vec<(u8, f32)> {
        s.shown().map(|(p, w)| (p.clip, w)).collect()
    }

    /// What `drive` does when a package with an idle collection (or furniture, or No
    /// Idle Anims) takes over a smoking NPC: its fidget fades out, one waiting for its
    /// clip is dropped, so the actor is no longer busy and the collection's idle starts
    /// at once over the fading fidget. Other idles and other actors are not touched.
    #[test]
    fn a_holstered_fidget_gives_way_when_something_takes_over() {
        let mut s: IdleSlot<u8> = IdleSlot::default();
        assert!(s.start(5, IdleSource::HolsteredFidget, None, 5, play(8.4, 0.3, Some(4))));
        s.advance(1.0);
        s.check_owner(|_| None);
        // Nothing takes over: it plays on, and the actor is busy.
        assert!(!end_holstered(&mut s, false));
        assert!(s.playing().is_some());
        // The package changes: it ends; the actor is free (`busy` reads `playing`).
        assert!(end_holstered(&mut s, true));
        assert!(s.playing().is_none(), "the collection no longer waits behind it");
        assert!(!end_holstered(&mut s, true), "stopped once");
        assert!(s.start(9, IdleSource::PackageIdles, Some(FormId(0xB02)), 9, play(3.0, 0.2, Some(1))));
        assert_eq!(s.playing().map(|p| (p.idle, p.source)), Some((9, IdleSource::PackageIdles)));
        // Another source on the slot is left alone.
        let mut t: IdleSlot<u8> = IdleSlot::default();
        t.start(1, IdleSource::Fidget, None, 1, play(2.0, 0.0, Some(1)));
        assert!(!end_holstered(&mut t, true));
        assert!(t.playing().is_some());
        // Waiting for their clips: the taken-over actor's fidget goes, its script idle and
        // another actor's fidget stay.
        let now = std::time::Instant::now();
        let req = |actor: u32, idle: u32, source: IdleSource| (IdleRequest { actor: FormId(actor), idle: FormId(idle), source, pov: None }, now);
        let mut waiting = vec![req(7, 5, IdleSource::HolsteredFidget), req(7, 6, IdleSource::Script), req(8, 5, IdleSource::HolsteredFidget)];
        drop_holstered_waits(&mut waiting, &HashSet::from([7]));
        let left: Vec<(u32, IdleSource)> = waiting.iter().map(|(r, _)| (r.actor.0, r.source)).collect();
        assert_eq!(left, [(7, IdleSource::Script), (8, IdleSource::HolsteredFidget)]);
    }

    #[test]
    fn the_first_fidget_looks_are_spread_over_one_period() {
        let away = FidgetKind::WeaponAway;
        let firsts: Vec<f32> = (0..64u32).map(|i| first_check(0xFF00_0000 + i, away)).collect();
        assert!(firsts.iter().all(|t| *t >= FIDGET_CHECK_SECONDS / 2.0 && *t <= FIDGET_CHECK_SECONDS));
        let (lo, hi) = firsts.iter().fold((f32::MAX, 0.0f32), |(lo, hi), t| (lo.min(*t), hi.max(*t)));
        assert!(hi - lo > FIDGET_CHECK_SECONDS / 4.0, "not all in one frame: {lo}..{hi}");
        assert_eq!(first_check(0x0012_CB7B, away), first_check(0x0012_CB7B, away), "the same each run");
        // The other kinds keep one full period, as before #2026 (the player too).
        for kind in [FidgetKind::WeaponOut, FidgetKind::OnFurniture, FidgetKind::Alerted, FidgetKind::Calm] {
            assert_eq!(first_check(0x14, kind), FIDGET_CHECK_SECONDS);
            assert_eq!(first_check(0xFF00_0003, kind), FIDGET_CHECK_SECONDS);
        }
    }

    /// `drive`'s judgement of a holstered NPC's fidget (`holstered_off`), on a game
    /// built from a plugin written for the test, through a package and a furniture
    /// change, with a fidget playing and one waiting for its clip: each change takes
    /// both away (`end_holstered`, `drop_holstered_waits`), as `drive` does with them.
    mod holstered_take_over {
        use super::*;
        use fnv_esm::raw::{write_subrecords, Subrecord};
        use fnv_esm::{LoadOrder, Plugin, Schema};
        use fnv_game::functions::Ctx;
        use fnv_game::game::{FurnitureUse, GameConfig};
        use fnv_game::GameData;
        use fnv_script::vm::{ArgValue, Value};
        use fnv_script::FunctionTable;
        use fnv_sim::{FormStore, RefTable, SimConfig, World};

        const IDLE: u32 = 0xA01;
        const P_PLAIN: u32 = 0xB01;
        const P_COLLECTION: u32 = 0xB02;
        const P_QUIET: u32 = 0xB03;
        const NPC: u32 = 0xC01;
        const ACTOR: u32 = 0xD01;
        const CELL: u32 = 0xE01;

        fn sub(sig: &[u8; 4], data: Vec<u8>) -> Subrecord {
            Subrecord { sig: *sig, data, extended: false }
        }

        fn record(sig: &[u8; 4], fid: u32, subs: &[Subrecord]) -> Vec<u8> {
            let payload = write_subrecords(subs);
            [&sig[..], &(payload.len() as u32).to_le_bytes(), &0u32.to_le_bytes(), &fid.to_le_bytes(), &[0; 4], &15u16.to_le_bytes(), &[0; 2], &payload].concat()
        }

        fn group(label: [u8; 4], gtype: i32, body: &[u8]) -> Vec<u8> {
            [&b"GRUP"[..], &((24 + body.len()) as u32).to_le_bytes(), &label, &gtype.to_le_bytes(), &[0; 8], body].concat()
        }

        /// A Travel package (PKDT type 6) with PKDT flags `flags`, and an idle collection
        /// of one idle when `collection`.
        fn pack(fid: u32, edid: &str, flags: u32, collection: bool) -> Vec<u8> {
            let mut subs = vec![
                sub(b"EDID", [edid.as_bytes(), &[0]].concat()),
                sub(b"PKDT", [&flags.to_le_bytes()[..], &[6, 0], &[0; 6]].concat()),
                sub(b"PLDT", [0i32.to_le_bytes(), 0u32.to_le_bytes(), 0i32.to_le_bytes()].concat()),
                sub(b"PSDT", [&[0xFF, 0xFF, 0, 0xFF][..], &0u32.to_le_bytes()].concat()),
            ];
            if collection {
                subs.extend([sub(b"IDLF", vec![1]), sub(b"IDLC", vec![1]), sub(b"IDLT", 0f32.to_le_bytes().to_vec()), sub(b"IDLA", IDLE.to_le_bytes().to_vec())]);
            }
            record(b"PACK", fid, &subs)
        }

        fn plugin_bytes() -> Vec<u8> {
            let hedr = [1.34f32.to_le_bytes(), 0i32.to_le_bytes(), 0x900u32.to_le_bytes()].concat();
            let tes4 = {
                let payload = write_subrecords(&[sub(b"HEDR", hedr)]);
                [&b"TES4"[..], &(payload.len() as u32).to_le_bytes(), &1u32.to_le_bytes(), &[0; 4], &[0; 4], &15u16.to_le_bytes(), &[0; 2], &payload].concat()
            };
            let idle = record(
                b"IDLE",
                IDLE,
                &[sub(b"EDID", b"TestCollectionIdle\0".to_vec()), sub(b"MODL", b"Characters\\_Male\\IdleAnims\\Test.kf\0".to_vec()), sub(b"ANAM", vec![0; 8]), sub(b"DATA", vec![7, 0, 0, 0, 0, 0, 0, 0])],
            );
            let packs = [pack(P_PLAIN, "TestPlain", 0, false), pack(P_COLLECTION, "TestCollection", 0, true), pack(P_QUIET, "TestQuiet", fnv_game::package::flags::NO_IDLE_ANIMS, false)].concat();
            let npcs = [record(b"NPC_", 0x7, &[sub(b"EDID", b"Player\0".to_vec())]), record(b"NPC_", NPC, &[sub(b"EDID", b"TestSmoker\0".to_vec())])].concat();
            let actor = record(b"ACHR", ACTOR, &[sub(b"NAME", NPC.to_le_bytes().to_vec()), sub(b"DATA", vec![0; 24])]);
            let cell = [record(b"CELL", CELL, &[sub(b"EDID", b"TestCell\0".to_vec())]), group(CELL.to_le_bytes(), 6, &group(CELL.to_le_bytes(), 9, &actor))].concat();
            [tes4, group(*b"NPC_", 0, &npcs), group(*b"IDLE", 0, &idle), group(*b"PACK", 0, &packs), group(*b"CELL", 0, &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cell)))].concat()
        }

        fn game() -> Game<'static> {
            let plugins: &'static [Plugin] = Box::leak(vec![Plugin::parse(&plugin_bytes(), "Holstered.esm".into()).unwrap()].into_boxed_slice());
            let lo: &'static LoadOrder<'static> = Box::leak(Box::new(LoadOrder::new(plugins).unwrap()));
            let schema: &'static Schema = Box::leak(Box::new(Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap()));
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
            let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let forms: &'static FormStore = Box::leak(Box::new(FormStore::build(lo, schema)));
            let refs: &'static RefTable = Box::leak(Box::new(RefTable::build(lo, schema)));
            let data: &'static GameData = Box::leak(Box::new(GameData::build(lo, schema, FunctionTable::load(&path).unwrap(), &json, refs)));
            Game::new(data, World::new(forms, refs, SimConfig::default()), GameConfig { strict: true, ..GameConfig::default() })
        }

        fn add_script_package(g: &mut Game, package: u32) {
            let ctx = Ctx { unit: "test".into(), opcode: 0x1097, on: Some(FormId(ACTOR)), self_ref: None, action_ref: None, quest: None, inst: None, in_condition: false };
            g.call_function(&ctx, &[ArgValue::Value(Value::Ref(package))]);
            assert_eq!(g.extra[&FormId(ACTOR)].current_package, Some(FormId(package)));
        }

        /// A fidget playing on the actor's slot and one of the actor's waiting for its
        /// clip; then `drive`'s step: judge, end, drop. Returns whether they went.
        fn step(g: &Game) -> (bool, bool) {
            let mut slot: IdleSlot<u8> = IdleSlot::default();
            slot.start(5, IdleSource::HolsteredFidget, None, 5, play(8.4, 0.3, Some(4)));
            let mut waiting = vec![(IdleRequest { actor: FormId(ACTOR), idle: FormId(6), source: IdleSource::HolsteredFidget, pov: None }, std::time::Instant::now())];
            let off: HashSet<u32> = holstered_off(g, ACTOR, false, false, None, None).then_some(ACTOR).into_iter().collect();
            drop_holstered_waits(&mut waiting, &off);
            end_holstered(&mut slot, off.contains(&ACTOR));
            (slot.playing().is_none(), waiting.is_empty())
        }

        #[test]
        fn a_package_or_furniture_with_idles_of_its_own_ends_a_holstered_fidget() {
            let mut g = game();
            assert_eq!(step(&g), (false, false), "standing holstered, no package: both stay");
            add_script_package(&mut g, P_PLAIN);
            assert_eq!(step(&g), (false, false), "a package without idles of its own");
            add_script_package(&mut g, P_COLLECTION);
            assert_eq!(step(&g), (true, true), "a package with an idle collection takes over");
            add_script_package(&mut g, P_QUIET);
            assert_eq!(step(&g), (true, true), "a package flagged No Idle Anims");
            add_script_package(&mut g, P_PLAIN);
            assert_eq!(step(&g), (false, false));
            g.set_furniture_use(FormId(ACTOR), Some(FurnitureUse { reference: FormId(CELL), marker_id: 1, state: 2 }));
            assert_eq!(step(&g), (true, true), "getting on furniture");
            g.set_furniture_use(FormId(ACTOR), None);
            g.set_weapon_out(FormId(ACTOR), true);
            assert_eq!(step(&g), (true, true), "the weapon drawn");
        }
    }

    #[test]
    fn a_new_idle_fades_in_over_the_one_before() {
        let mut s: IdleSlot<u8> = IdleSlot::default();
        s.start(1, IdleSource::Script, None, 1, play(2.0, 0.0, Some(1)));
        s.advance(0.5);
        assert_eq!(weights(&s), [(1, 1.0)]);
        assert!(s.covers());
        s.check_owner(|_| None);
        s.start(2, IdleSource::Script, None, 2, play(4.0, 1.0, None));
        s.advance(0.25);
        // Together they cover the rig in full.
        assert_eq!(weights(&s), [(2, 0.25), (1, 0.75)]);
        assert!(s.covers() && s.playing().unwrap().idle == 2);
        s.advance(0.75);
        assert_eq!(weights(&s), [(2, 1.0)]);
    }

    #[test]
    fn an_idle_started_in_the_same_step_is_replaced_outright() {
        let mut s: IdleSlot<u8> = IdleSlot::default();
        s.start(1, IdleSource::PackageIdles, None, 1, play(2.0, 0.0, None));
        s.advance(1.0);
        s.check_owner(|_| None);
        // A change idle, then the next package's begin idle, in one frame.
        s.start(2, IdleSource::PackageChange, None, 2, play(2.0, 0.5, None));
        s.start(3, IdleSource::PackageBegin, None, 3, play(2.0, 0.5, Some(1)));
        s.advance(0.25);
        assert_eq!(weights(&s), [(3, 0.5), (1, 0.5)]);
    }

    #[test]
    fn a_one_shot_idle_ends_fades_and_goes() {
        let mut s: IdleSlot<u8> = IdleSlot::default();
        s.start(1, IdleSource::Speaker, None, 1, play(1.0, 0.5, Some(1)));
        s.advance(0.25);
        // Fading in alone: the rig's other clips stay under it.
        assert!(!s.covers() && s.playing().is_some());
        s.advance(0.75);
        assert!(s.playing().is_none() && !s.is_empty());
        s.advance(0.125);
        assert_eq!(weights(&s), [(1, 0.5)]);
        s.advance(0.25);
        assert!(s.is_empty());
    }

    #[test]
    fn a_package_loop_is_left_when_the_package_is_no_longer_the_actors() {
        let (p, q) = (Some(FormId(1)), Some(FormId(2)));
        let mut s: IdleSlot<u8> = IdleSlot::default();
        s.start(1, IdleSource::PackageBegin, p, 1, play(1.0, 0.0, None));
        s.advance(0.5);
        s.check_owner(|_| p);
        assert!(s.playing().unwrap().play.endless());
        s.check_owner(|_| q);
        assert!(!s.playing().unwrap().play.endless());
        s.advance(0.75);
        assert!(s.playing().is_none());
        // A script's idle is nobody's package's.
        let mut s: IdleSlot<u8> = IdleSlot::default();
        s.start(1, IdleSource::Script, None, 1, play(1.0, 0.0, None));
        s.check_owner(|_| q);
        assert!(s.playing().unwrap().play.endless());
    }

    #[test]
    fn replacing_an_idle_file_restarts_but_repeating_that_file_does_not() {
        let mut slot: IdleSlot<u8> = IdleSlot::default();
        assert!(slot.start_file(7,IdleSource::Script,None,1,play(3.0,0.0,None),Some("owned\\loop.kf".into())));
        slot.advance(0.5);
        assert!(!slot.start_file(7,IdleSource::Script,None,2,play(3.0,0.0,None),Some("owned\\loop.kf".into())));
        assert_eq!(slot.playing().unwrap().clip,1);
        assert_eq!(slot.playing().unwrap().play.time(),0.5);
        assert!(slot.start_file(7,IdleSource::Script,None,3,play(0.75,0.0,Some(1)),Some("owned\\stop.kf".into())));
        assert_eq!(slot.playing().unwrap().clip,3);
        assert_eq!(slot.playing().unwrap().play.time(),0.0);
        slot.advance(0.8);
        assert!(slot.playing().is_none());
    }

    #[test]
    fn asked_again_by_a_line_an_idle_is_not_started_over() {
        // The opening: stage 30's PlayIdle, then 3 s later the next line's speaker idle,
        // the same, while the first holds its last pose and fades out.
        let mut s: IdleSlot<u8> = IdleSlot::default();
        assert!(s.start(7, IdleSource::Script, None, 1, play(3.0, 0.0, Some(1))));
        s.advance(3.03);
        s.check_owner(|_| None);
        assert!(!s.start(7, IdleSource::Speaker, None, 2, play(3.0, 0.0, Some(1))));
        assert_eq!(weights(&s), [(1, 1.0)]);
        assert_eq!(s.playing().unwrap().play.time(), 3.0);
        // A fidget started as the scripted idle ended (Doc's chair fidget, #1355): the
        // line's speaker idle brings the held one back, not a second hand-over.
        let mut s: IdleSlot<u8> = IdleSlot::default();
        assert!(s.start(7, IdleSource::Script, None, 1, play(3.0, 0.0, Some(1))));
        s.advance(3.01);
        s.check_owner(|_| None);
        assert!(s.start(9, IdleSource::FurnitureFidget, None, 3, play(16.0, 0.67, Some(1))));
        s.advance(0.02);
        assert!(!s.start(7, IdleSource::Speaker, None, 2, play(3.0, 0.0, Some(1))));
        assert_eq!(weights(&s), [(1, 1.0)]);
        assert_eq!(s.playing().unwrap().play.time(), 3.0);
        // Another asked idle over the fidget is a new one, as before.
        assert!(s.start(8, IdleSource::Speaker, None, 4, play(1.0, 0.0, Some(1))));
        // A collection picking the same idle again plays it again.
        let mut s: IdleSlot<u8> = IdleSlot::default();
        s.start(7, IdleSource::PackageIdles, None, 1, play(3.0, 0.0, Some(1)));
        s.advance(3.03);
        s.check_owner(|_| None);
        assert!(s.start(7, IdleSource::PackageIdles, None, 2, play(3.0, 0.0, Some(1))));
    }

    #[test]
    fn clamp_effect_idle_layers_keep_the_exact_last_sample() {
        use fnv_nif::anim::{Channel, Sequence, ValueTrack};
        use nvf_assets::anim_play::{MaterialTarget, MaterialTrack};
        let seq = Sequence { block: 0, name: Some("OwnedEffect".into()), start_time: 0.0, stop_time: 2.0,
            frequency: 1.0, cycle_type: 2, accum_root_name: None, text_keys: Vec::new(), tracks: Vec::new() };
        let mut clip = Clip::from_sequence(&seq);
        clip.materials.push(MaterialTrack { shape: "OwnedGlow".into(), target: MaterialTarget::SelfIllum,
            values: Arc::new(ValueTrack::Point3(Channel::Static(Some([0.2; 3])))), cycle: None, priority: 1 });
        let keys = IdleKeys::read(0.0, 2.0, &[]);
        let mut play = effect_play(&clip, keys, Some(3));
        play.advance(100.0);
        assert_eq!((play.time(), play.weight(), play.gone()), (2.0, 1.0, false));
        let mut slot = ActorIdle::default();
        slot.0.start(1, IdleSource::Script, None, Arc::new(BoundClip::bind(Arc::new(clip.clone()), |_| None)), play);
        let mut player = BlendPlayer::new(Vec::new(), Arc::new(Vec::new()));
        slot.layers(&mut player);
        assert_eq!(player.layers[IDLE_LAYER].as_ref().unwrap().phase, 1.0);
        clip.cycle_type = 0;
        let mut ordinary = effect_play(&clip, keys, Some(1));
        ordinary.advance(3.0);
        ordinary.advance(1.0);
        assert!(ordinary.gone());
        clip.cycle_type = 2;
        clip.materials.clear();
        let mut ordinary = effect_play(&clip, keys, Some(1));
        ordinary.advance(3.0);
        ordinary.advance(1.0);
        assert!(ordinary.gone());
    }

    #[test]
    fn rolls_stay_below_the_bound() {
        let mut r = Rng(7);
        assert!((0..200).all(|_| r.below(5) < 5));
        assert_eq!(r.below(0), 0);
    }

    #[test]
    fn a_scene_idle_carries_the_body_off_its_reference() {
        use fnv_nif::anim::{Channel, TransformTrack};
        let clip = |at: [f32; 3]| Clip {
            name: String::new(), start: 0.0, stop: 9.97, frequency: 1.0, cycle_type: 0,
            tracks: vec![("Bip01 NonAccum".into(), Arc::new(TransformTrack { translation: Channel::Static(Some(at)), rotation: Channel::Static(None), euler: None, scale: Channel::Static(None), per_cycle: None }))],
            priorities: vec![13], skipped: 0, accum_root: Some("Bip01".into()), text_keys: Vec::new(), visibility: Vec::new(), emitters: Vec::new(), materials: Vec::new(), free: Vec::new(), free_visibility: Vec::new(), lights: Vec::new(),
        };
        // Dad in the birth scene, against a body standing over its reference.
        assert!(carries_body(&clip([96.0, 110.6, 66.3])));
        assert!(!carries_body(&clip([0.66, 0.0, 79.1])));
    }

    #[test]
    fn held_follows_the_games_move() {
        // TTW's opening: the birth idle begins in the start room, the move lands after.
        let h = Held { feet: [2036.0, 1946.0, 7608.7], free: [2036.0, 1946.0, 7608.7], heading: 3.1, pitch: -0.2 };
        let to = [-5276.0, -7148.0, 7543.2];
        assert_eq!(h.moved(Some(to), 1.0, 0.1), Some(Held { feet: to, free: to, heading: 1.0, pitch: 0.1 }));
        // Where it was held (the opening's bed marker), or no placement: stays held there.
        assert_eq!(h.moved(Some([2036.4, 1946.0, 7608.7]), 1.0, 0.1), None);
        assert_eq!(h.moved(None, 1.0, 0.1), None);
    }

    #[test]
    fn a_movement_camera_overlay_does_not_hold_the_player() {
        // Owned camera-track fixture: a camera track alone is not a position lock.
        use fnv_nif::anim::{Channel, TransformTrack};
        let clip = Arc::new(Clip {
            name: "OwnedMovement".into(), start: 0.0, stop: 0.7, frequency: 1.0, cycle_type: 0,
            tracks: vec![(CAMERA_NODE.into(), Arc::new(TransformTrack {
                translation: Channel::Static(Some([0.0, 0.0, 70.0])),
                rotation: Channel::Static(None), euler: None, scale: Channel::Static(None), per_cycle: None,
            }))], priorities: vec![50], skipped: 0, accum_root: None,
            text_keys: Vec::new(), visibility: Vec::new(), emitters: Vec::new(), materials: Vec::new(),
            free: Vec::new(), free_visibility: Vec::new(), lights: Vec::new(),
        });
        let mut p = PlayerIdles::default();
        p.first.start(1, IdleSource::Script, None, clip.clone(), play(0.7, 0.0, None));
        p.first.cur.as_mut().unwrap().holds = camera_idle_holds(1);
        p.first.advance(2.0);
        assert!(p.first.playing().is_some(), "the movement overlay keeps looping");
        assert!(!p.holds_camera(), "walking must remain under physics control");
        // A scene using the same track still holds position; repeated requests do
        // not turn its hold off, and fading it out eventually releases it.
        p.first.start(2, IdleSource::Script, None, clip.clone(), play(1.0, 0.3, None));
        p.first.cur.as_mut().unwrap().holds = camera_idle_holds(7);
        assert!(p.holds_camera(), "a new cinematic holds even at zero fade-in weight");
        p.first.advance(0.1);
        assert!(p.holds_camera());
        p.first.start(2, IdleSource::Script, None, clip.clone(), play(1.0, 0.0, None));
        assert!(p.holds_camera());
        p.first.advance(0.1);
        p.first.check_owner(|_| None); // The next frame clears the request's fresh flag.
        p.first.start(3, IdleSource::Script, None, clip, play(0.7, 0.3, None));
        p.first.cur.as_mut().unwrap().holds = camera_idle_holds(1);
        assert!(p.holds_camera(), "the outgoing scene still contributes during its fade");
        p.first.advance(0.5);
        assert!(!p.holds_camera());
    }

    #[test]
    fn only_the_movement_section_releases_a_camera_position_hold() {
        assert!(!camera_idle_holds(1));
        for section in [0, 2, 3, 4, 5, 6, 7, 20, 21, 63, 255] {
            assert!(camera_idle_holds(section), "section {section} keeps its existing hold");
        }
    }

    #[test]
    fn third_person_idles_do_not_acquire_a_camera_hold() {
        let mut p = PlayerIdles::default();
        assert!(!p.holds_camera());
        let clip = Arc::new(Clip {
            name: "OwnedThirdPerson".into(), start: 0.0, stop: 1.0, frequency: 1.0, cycle_type: 0,
            tracks: Vec::new(), priorities: Vec::new(), skipped: 0, accum_root: None,
            text_keys: Vec::new(), visibility: Vec::new(), emitters: Vec::new(), materials: Vec::new(),
            free: Vec::new(), free_visibility: Vec::new(), lights: Vec::new(),
        });
        p.third.start(1, IdleSource::Script, None, clip, play(1.0, 0.0, None));
        p.third.cur.as_mut().unwrap().holds = true;
        assert!(p.third.playing().is_some());
        assert!(!p.holds_camera());
    }

    #[test]
    fn clip_path_for_applies_knvse_override_by_filename_and_normalizes_forward_slashes() {
        let empty = crate::outfits::tests::empty_world_data();
        let forms = crate::outfits::tests::actor_forms(vec![]);
        let refs = fnv_sim::RefTable::default();
        let functions = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
        let table = fnv_script::FunctionTable::load(&functions).unwrap();
        let json = serde_json::from_str(&std::fs::read_to_string(functions).unwrap()).unwrap();
        let mut data = fnv_game::GameData::build(empty.lo, empty.schema, table, &json, &refs);
        let idle = FormId(0x9001);
        data.idles.insert(
            idle,
            fnv_game::data::IdleDef {
                model: Some("Characters\\_Male\\IdleAnims\\SpecialIdle_Test.kf".into()),
                ..Default::default()
            },
        );
        let mut game = fnv_game::Game::new(&data, fnv_sim::World::new(&forms, &refs, Default::default()), Default::default());
        let npc = FormId(0x2000);

        // Vanilla (no active overrides): exact record path.
        assert_eq!(clip_path_for(&game, npc, None, idle).as_deref(), Some("meshes\\characters\\_male\\idleanims\\specialidle_test.kf"));

        // Active AnimGroupOverride matching the idle's filename: replacement wins.
        game.anim_folders.active.insert((npc, false), vec!["AnimGroupOverride\\ModA\\_male\\SpecialIdle_Test.kf".into()]);
        assert_eq!(clip_path_for(&game, npc, None, idle).as_deref(), Some("meshes\\animgroupoverride\\moda\\_male\\specialidle_test.kf"));

        // Player first-person POV override stays isolated from third-person POV, and forward-slash
        // "meshes/..." paths normalize without double-prefixing.
        game.anim_folders.active.insert((FormId::PLAYER_REF, true), vec!["meshes/AnimGroupOverride/ModA/_1stPerson/SpecialIdle_Test.kf".into()]);
        assert_eq!(clip_path_for(&game, FormId::PLAYER_REF, Some(IdlePov::First), idle).as_deref(), Some("meshes\\animgroupoverride\\moda\\_1stperson\\specialidle_test.kf"));
        assert_eq!(clip_path_for(&game, FormId::PLAYER_REF, Some(IdlePov::Third), idle).as_deref(), Some("meshes\\characters\\_male\\idleanims\\specialidle_test.kf"));
        assert_eq!(strip_meshes_prefix("meshes/AnimGroupOverride/ModA/SpecialIdle_Test.kf"), "AnimGroupOverride\\ModA\\SpecialIdle_Test.kf");
        assert_eq!(strip_meshes_prefix("Meshes\\AnimGroupOverride\\ModA\\SpecialIdle_Test.kf"), "AnimGroupOverride\\ModA\\SpecialIdle_Test.kf");
    }

    #[test]
    fn talking_gesture_ends_on_furniture_posture_change_and_clears_before_chair_exit_finishes() {
        // Seated gesture keeps playing while still seated, stationary, and talking.
        assert!(!gesture_ends(false, true, true, true));
        // Standing gesture keeps playing while standing still and talking.
        assert!(!gesture_ends(false, true, false, false));
        // Seated gesture ends as soon as the actor starts getting off (now_seated == false),
        // even while velocity == 0 and the dialogue line is still playing (#3407).
        assert!(gesture_ends(false, true, true, false), "seated gesture must stop on chair exit");
        // Standing gesture ends if the actor sits down.
        assert!(gesture_ends(false, true, false, true), "standing gesture must stop on sitting");
        // Existing stop triggers remain unchanged: moving or leaving conversation.
        assert!(gesture_ends(true, true, false, false), "moving ends gesture");
        assert!(gesture_ends(false, false, true, true), "leaving conversation ends gesture");

        // Slot lifecycle: a 6.97 s seated talk gesture stopped when a 1.53 s chair exit starts
        // fades out over its 0.25 s blend-out and is completely gone before the actor stands up.
        let mut slot: IdleSlot<u8> = IdleSlot::default();
        assert!(slot.start(42, IdleSource::Gesture, None, 42, play(6.97, 0.25, Some(1))));
        slot.cur.as_mut().unwrap().seated = true;
        slot.advance(1.2);
        assert!(slot.playing().is_some());
        if let Some(c) = slot.cur.as_mut().filter(|c| c.source == IdleSource::Gesture && gesture_ends(false, true, c.seated, false)) {
            c.play.stop();
        }
        assert!(slot.playing().is_none(), "stopped gesture immediately yields busy status");
        slot.advance(0.30);
        assert!(slot.is_empty(), "seated gesture fades out well before a 1.53 s exit clip ends");
    }

    #[test]
    fn seated_dialogue_response_idles_stop_on_furniture_exit_while_other_idles_stay() {
        // Seated Speaker and Listener idles stop when the actor starts getting off furniture.
        assert!(seated_dialogue_idle_ends(IdleSource::Speaker, true, false));
        assert!(seated_dialogue_idle_ends(IdleSource::Listener, true, false));
        // While still seated, they continue normally.
        assert!(!seated_dialogue_idle_ends(IdleSource::Speaker, true, true));
        assert!(!seated_dialogue_idle_ends(IdleSource::Listener, true, true));
        // Standing dialogue response idles are unaffected.
        assert!(!seated_dialogue_idle_ends(IdleSource::Speaker, false, false));
        assert!(!seated_dialogue_idle_ends(IdleSource::Listener, false, true));
        // Scripted / package idles are not forcibly ended by this rule.
        assert!(!seated_dialogue_idle_ends(IdleSource::Script, true, false));
        assert!(!seated_dialogue_idle_ends(IdleSource::PackageBegin, true, false));
    }

    #[test]
    fn furniture_transition_holds_gestures_and_drops_stale_waiting_gestures() {
        use fnv_game::game::FurnitureUse;
        let empty = crate::outfits::tests::empty_world_data();
        let forms = crate::outfits::tests::actor_forms(vec![]);
        let refs = fnv_sim::RefTable::default();
        let functions = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/script/functions.json");
        let table = fnv_script::FunctionTable::load(&functions).unwrap();
        let json = serde_json::from_str(&std::fs::read_to_string(functions).unwrap()).unwrap();
        let data = fnv_game::GameData::build(empty.lo, empty.schema, table, &json, &refs);
        let mut game = Game::new(&data, fnv_sim::World::new(&forms, &refs, Default::default()), Default::default());
        let actor = 0x2001u32;

        // Standing (no furniture): neither on furniture nor transitioning.
        assert!(!on_furniture(&game, actor));
        assert!(!in_furniture_transition(&game, actor));

        // Getting on furniture (state 2): transitioning -> gestures wait.
        game.set_furniture_use(FormId(actor), Some(FurnitureUse { reference: FormId(0x3001), marker_id: 14, state: 2 }));
        assert!(!on_furniture(&game, actor));
        assert!(in_furniture_transition(&game, actor));

        // Seated on furniture (state 3): seated and not transitioning -> chair gestures allowed.
        game.set_furniture_use(FormId(actor), Some(FurnitureUse { reference: FormId(0x3001), marker_id: 14, state: 3 }));
        assert!(on_furniture(&game, actor));
        assert!(!in_furniture_transition(&game, actor));

        // Getting off furniture (state 4): transitioning -> seated gestures end and no new gesture starts.
        game.set_furniture_use(FormId(actor), Some(FurnitureUse { reference: FormId(0x3001), marker_id: 14, state: 4 }));
        assert!(!on_furniture(&game, actor));
        assert!(in_furniture_transition(&game, actor));

        // Queued gesture requests for stale actors are dropped while Script requests and other actors stay.
        let now = std::time::Instant::now();
        let req = |a: u32, idle: u32, source: IdleSource| (IdleRequest { actor: FormId(a), idle: FormId(idle), source, pov: None }, now);
        let mut waiting = vec![
            req(actor, 10, IdleSource::Gesture),
            req(actor, 11, IdleSource::Script),
            req(actor + 1, 12, IdleSource::Gesture),
        ];
        drop_stale_gesture_waits(&mut waiting, &HashSet::from([actor]));
        let left: Vec<(u32, IdleSource)> = waiting.iter().map(|(r, _)| (r.actor.0, r.source)).collect();
        assert_eq!(left, [(actor, IdleSource::Script), (actor + 1, IdleSource::Gesture)]);
    }
}
