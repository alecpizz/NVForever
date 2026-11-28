//! What the engine sees of actors and tells the scripts: movement, sneaking, line of
//! sight, the camera, and who always shows subtitles (docs/design/script-stubs.md §3).
//!
//! The engine measures; this module only keeps the latest word ([`Senses`]) and answers
//! the script functions from it. An actor the engine has not told about (not drawn, too
//! far from the player, a headless run) reads as standing still, not sneaking and not
//! seen [engineering choice: the same "not measured is no" rule as GetDetected].

use super::{none, yes, Args, Ctx};
use crate::game::Game;
use fnv_script::vm::Value;
use fnv_sim::FormId;
use std::collections::{BTreeMap, BTreeSet};

/// One actor's movement as the engine last measured it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Motion {
    /// It moves (above the engine's walking threshold).
    pub moving: bool,
    /// It moves at its run speed (the player: not holding Walk).
    pub running: bool,
    /// It is sneaking (crouched).
    pub sneaking: bool,
    /// It is turning: 0 not, 1 left, 2 right (IsTurning's codes).
    pub turning: u8,
    /// Its speed mode is run, whether or not it moves (the player: Walk not toggled).
    pub run_mode: bool,
    /// Its character swims (the physics' state: `fnv_phys` character `swimming`).
    pub swimming: bool,
    /// Its character is airborne (physics: not grounded and not swimming; `IsInAir`).
    pub in_air: bool,
}

/// What the engine told the scripts, and the switches scripts set for it.
#[derive(Debug, Clone, PartialEq)]
pub struct Senses {
    pub motion: BTreeMap<FormId, Motion>,
    /// The camera shows the player from the eyes (first person). The game starts so
    /// [observed: the PC version opens in first person; `nvf_play` `Pov::First`].
    pub first_person: bool,
    /// Actors whose lines always show their subtitle, heard or not
    /// (AlwaysShowActorSubtitles 1).
    pub always_subtitles: BTreeSet<FormId>,
    /// Look: the reference each actor's head is held on, until StopLook. Not part of the
    /// saved state yet.
    pub looks: BTreeMap<FormId, Look>,
    /// When ([`Game::time`]) the animating side last told whether each actor has its
    /// weapon out (`Game::set_weapon_out`): it tells every frame while it draws the actor.
    pub weapon_told: BTreeMap<FormId, f64>,
    /// The open state of every object the engine animates open and closed (doors,
    /// animated containers), as GetOpenState numbers it: 1 open, 2 opening, 3 closed,
    /// 4 closing. Told by the engine's door animation (`nvf-play` doors.rs).
    pub open_states: BTreeMap<FormId, u8>,
    /// What each drawn actor's body does (GetAnimAction, IsMoving's way, its movement
    /// group), told every frame (`Game::set_anim_state`, `crate::anim_state`).
    pub anim: BTreeMap<FormId, crate::anim_state::AnimState>,
    /// The crosshair's ray as the engine last cast it (PlaceAtReticle); `None` before the
    /// engine told it (a headless run).
    pub reticle: Option<Reticle>,
    /// The reference the player would activate (GetCrosshairRef) and the first one the
    /// crosshair's ray meets at any range (GetCrosshairRefEx), as the engine tells them.
    pub crosshair_ref: Option<FormId>,
    pub crosshair_far: Option<FormId>,
    /// The reference the player holds with the grab (`nvf-play` grab.rs), told by the
    /// engine ([`Game::set_player_grab`]); `None` while the player holds nothing. Not
    /// part of the saved state: a grab ends with the frame the game is saved in.
    pub grabbed: Option<FormId>,
    /// The player's exposure to radiation, measured each frame (`crate::radiation`); the
    /// water the player is in is told by the engine.
    pub radiation: crate::radiation::Exposure,
    /// The radiation zones explosions left (`crate::radiation`; not saved [design]).
    pub rad_zones: Vec<crate::radiation::RadZone>,
    pub rad_serial: u32,
    /// What each actor last activated and when ([`Game::time`]), for IsWaterObject.
    pub activating: BTreeMap<FormId, (FormId, f64)>,
    /// The dead actors the player's camera saw at the engine's last measurement, each
    /// with when ([`Game::time`]) it was told ([`Game::set_camera_sights`]): what
    /// player.GetLineOfSight reads of a body, which the detection pairs leave out.
    pub camera_sees: BTreeMap<FormId, f64>,
}

/// The ray under the crosshair, game space: where it starts (about the player's eye,
/// `nvf_play::control::Crosshair::ray`), its unit direction, the distance to the first
/// thing it hits within [`RETICLE_REACH`] (`None`: nothing), and the body part location
/// (`BodyLocation` enum, 0..=14) when that hit is on an actor with resolved body parts
/// (`GetReticleTargetLimb`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reticle {
    pub start: [f32; 3],
    pub dir: [f32; 3],
    pub hit: Option<f32>,
    pub limb: Option<i32>,
}

/// How far the engine casts the crosshair's ray for PlaceAtReticle: the largest
/// maxDistance the function takes [documented: GECK wiki PlaceAtReticle, "1-50000"].
pub const RETICLE_REACH: f32 = 50000.0;

impl Reticle {
    /// The point the crosshair hits, when it hits within `max` units.
    pub fn point(&self, max: f32) -> Option<[f32; 3]> {
        let d = self.hit.filter(|&d| d <= max)?;
        Some([self.start[0] + self.dir[0] * d, self.start[1] + self.dir[1] * d, self.start[2] + self.dir[2] * d])
    }
}

impl Default for Senses {
    fn default() -> Senses {
        Senses { motion: BTreeMap::new(), first_person: true, always_subtitles: BTreeSet::new(), looks: BTreeMap::new(), weapon_told: BTreeMap::new(), open_states: BTreeMap::new(), anim: BTreeMap::new(), reticle: None, crosshair_ref: None, crosshair_far: None, grabbed: None, radiation: Default::default(), rad_zones: Vec::new(), rad_serial: 0, activating: BTreeMap::new(), camera_sees: BTreeMap::new() }
    }
}

/// A script's Look on one actor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    pub target: FormId,
    /// The whole body turns to the target, not only the head (Look's second parameter).
    pub rotate_body: bool,
}

impl Game<'_> {
    /// The engine tells how an actor moves (the player included).
    pub fn set_motion(&mut self, actor: FormId, m: Motion) {
        if self.senses.motion.get(&actor) != Some(&m) {
            self.senses.motion.insert(actor, m);
        }
    }

    /// The actor left the engine's view (unloaded, disabled, dead): forget its motion.
    pub fn forget_motion(&mut self, actor: FormId) {
        self.senses.motion.remove(&actor);
    }

    /// Dispatch the extender's `OnPlayerGrab` / `OnPlayerRelease` event (plain and
    /// colon-qualified `:formType`) for `r` [documented: GECK wiki OnPlayerGrab,
    /// OnPlayerRelease; hypothesis: colon-qualified `:formType` dispatch and calling
    /// reference `Some(r)`, matching `OnCrosshairOn`]. Free when no handler is registered.
    fn dispatch_player_grab_event(&mut self, event: &str, lower: &str, prefix: &str, r: FormId) {
        if !self.handlers.iter().any(|h| h.event == lower || h.event.starts_with(prefix)) {
            return;
        }
        // Snapshot the canonical form type before callbacks can mutate the reference.
        let ty = crate::functions::base_of(self, r)
            .and_then(|base| self.data.sig(base))
            .and_then(|sig| fnv_script::vm::form_type_code(&sig));
        self.dispatch_event(event, vec![fnv_script::vm::XVal::Ref(r.0)], Some(r));
        if let Some(ty) = ty {
            self.dispatch_event(&format!("{event}:{ty}"), vec![fnv_script::vm::XVal::Ref(r.0)], Some(r));
        }
    }

    /// The engine tells what the player holds with the grab (docs/spec-notes/grab.md §4).
    /// A change updates `senses.grabbed`, runs the OnRelease blocks and `OnPlayerRelease`
    /// handlers for the reference let go, then the OnGrab blocks and `OnPlayerGrab`
    /// handlers for the one taken [documented: GECK wiki block-type list (OnGrab,
    /// OnRelease) and the extender's OnPlayerGrab / OnPlayerRelease pages ("when the
    /// player picks up (Control Code 27) / releases (drops the grabbed ref)"); observed:
    /// codes 0x21 and 0x22, VCG01BrahminSkullSCRIPT sets CGTutorial stages 22 and 24 from
    /// them, script-functions.md §3.3]. Handler-vs-block order, colon-qualified form-type
    /// delivery, and the order of a swap within one frame are [hypothesis]; one grab key
    /// never swaps.
    /// Returns which reference script blocks ran: (OnRelease, OnGrab).
    pub fn set_player_grab(&mut self, now: Option<FormId>) -> (bool, bool) {
        let before = self.senses.grabbed;
        if before == now {
            return (false, false);
        }
        self.senses.grabbed = now;
        let player = Some(FormId::PLAYER_REF);
        let released = before.is_some_and(|r| {
            let block = self.ref_event(r, crate::game::event::ON_RELEASE, None, player);
            self.dispatch_player_grab_event("OnPlayerRelease", "onplayerrelease", "onplayerrelease:", r);
            block
        });
        let grabbed = now.is_some_and(|r| {
            let block = self.ref_event(r, crate::game::event::ON_GRAB, None, player);
            self.dispatch_player_grab_event("OnPlayerGrab", "onplayergrab", "onplayergrab:", r);
            block
        });
        (released, grabbed)
    }

    /// The engine tells whether the camera is in first person. Fires
    /// `ShowOff:OnPOVChange` only on actual perspective transitions (`1` = first person,
    /// `0` = third person), with calling reference `Player`; on 1P->3P `IsPC1stPerson`
    /// still reads `1` inside the callback and `0` afterward, while on 3P->1P it reads
    /// `1` inside and after [documented: GECK wiki ShowOff:OnPOVChange; observed
    /// 2026-11-24: `w6t2p2_03_showoff_on_pov_change_toggle`].
    pub fn set_first_person(&mut self, on: bool) {
        if self.senses.first_person == on {
            return;
        }
        if on {
            self.senses.first_person = true;
            self.showoff_on_pov_change(true);
        } else {
            self.showoff_on_pov_change(false);
            self.senses.first_person = false;
        }
    }

    pub fn motion(&self, actor: FormId) -> Motion {
        self.senses.motion.get(&actor).copied().unwrap_or_default()
    }

    /// Whether the actor's lines show their subtitle out of hearing too.
    pub fn always_subtitles(&self, actor: FormId) -> bool {
        self.senses.always_subtitles.contains(&actor)
    }

    /// The reference a script holds the actor's head on (Look), if any.
    pub fn script_look(&self, actor: FormId) -> Option<Look> {
        self.senses.looks.get(&actor).copied()
    }

    /// The actors a script's Look turns whole (`rotateBody` 1), each with its target, in
    /// reference order: the body faces the target until StopLook (`fnv_ai` turns those
    /// it runs, the engine's `script_look.rs` the others).
    pub fn body_looks(&self) -> Vec<(FormId, FormId)> {
        self.senses.looks.iter().filter(|(_, l)| l.rotate_body).map(|(a, l)| (*a, l.target)).collect()
    }
}

/// `actor.IsSneaking`: 1 while the actor is sneaking (crouched) [documented: GECK wiki
/// IsSneaking]. The engine tells it ([`Game::set_motion`]): the player's sneak control,
/// and for other actors [`Game::sneaks`] (SetForceSneak, a package's Always Sneak).
/// An actor the engine does not measure (not drawn, far) sneaks by the same rule
/// ([`Game::sneaks`]).
pub fn is_sneaking(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject_or_player();
    yes(g.is_sneaking(r))
}

/// `actor.IsMoving`: 0 not moving, 1 forward, 2 backward, 3 left, 4 right: the
/// direction the actor is "requesting to move" in [documented: GECK wiki IsMoving]. The
/// engine tells the way each frame (`crate::anim_state`, [`Game::anim_state`]): the
/// player's movement keys, an NPC's walk in its own axes. An actor with no fresh
/// snapshot that the engine still measures moving reads 1 [hypothesis, as before the
/// way was told]; 0 at rest [observed: probes].
pub fn is_moving(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject_or_player();
    let code = match g.anim_state(r) {
        Some(s) => s.moving.code(),
        None => u8::from(g.motion(r).moving),
    };
    Ok(Value::Num(code as f64))
}

/// `actor.IsRunning`: 1 when the actor's speed mode is run rather than walk, "even if
/// they are standing still" [documented: GECK wiki IsRunning notes; observed: the player
/// standing still read 1 and Sunny Smiles standing still 0, capture `fa_batch1_queries`].
/// The player's mode is the Walk toggle (`Motion::run_mode`); another actor's is read
/// from the engine's measure, running while it moves at its run speed [hypothesis for
/// NPCs: their mode while standing is not told].
pub fn is_running(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject_or_player();
    let m = g.motion(r);
    yes(m.running || m.run_mode)
}

/// How long after an activation the object counts as "being activated", game seconds
/// [design: about the length of an activation's idle].
pub const ACTIVATING_SECONDS: f64 = 2.0;

impl Game<'_> {
    /// `actor` activates `target` now (for IsWaterObject).
    pub fn note_activating(&mut self, actor: FormId, target: FormId) {
        let now = self.time;
        self.senses.activating.insert(actor, (target, now));
    }

    /// What `actor` is activating: the reference it activated within
    /// [`ACTIVATING_SECONDS`].
    pub fn activating(&self, actor: FormId) -> Option<FormId> {
        self.senses.activating.get(&actor).filter(|(_, at)| self.time - at <= ACTIVATING_SECONDS).map(|(t, _)| *t)
    }
}

/// `actor.IsWaterObject`: 1 if the object the actor is activating is placeable water
/// [documented: GECK wiki IsWaterObject, "Returns 1 if the object currently being
/// activated is Placeable Water. Used in vanilla game for drinking animation"]. Placeable
/// water is taken as an activator with a Water Type (ACTI WNAM: the sinks, toilets and
/// pumps the player drinks from, [`Game::drink_water`]) [hypothesis]; "being activated"
/// as activated within [`ACTIVATING_SECONDS`] [design]. Its one use is the condition of
/// the idle `ActWaterSource` [observed: FalloutNV.esm].
pub fn is_water_object(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    Ok(Value::bool(g.activating(r).and_then(|t| g.drink_water(t)).is_some()))
}

/// `actor.IsSwimming`: 1 while the calling actor swims [documented: GECK wiki
/// IsSwimming]. The engine tells it with the rest of the movement ([`Game::set_motion`],
/// `nvforever` detection.rs): the swimming state of the actor's physics character, which
/// starts and stops as physics.md §6.6 describes. On the player it reads 0 whatever the
/// player does [documented: GECK wiki IsSwimming notes, "will always return 0 if called
/// on the player"; the extender's IsPlayerSwimming exists for that case]. An actor the
/// engine does not measure (not drawn, far) reads 0 [engineering choice: the same "not
/// measured is no" rule as IsMoving]. The shipped uses are idle conditions
/// (`IsSwimming 0` beside the flee idles, `crate::idles`) [observed: catalogue use
/// counts, 7 condition uses].
pub fn is_swimming(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject_or_player();
    yes(r != FormId::PLAYER_REF && g.motion(r).swimming)
}

/// How long the engine's word that the camera sees a body holds, game seconds: as
/// long as a detection pair is kept (`nvforever` detection.rs forgets pairs older than
/// 1 s) [engineering choice].
pub const CAMERA_SIGHT_SECONDS: f64 = 1.0;

impl Game<'_> {
    /// The engine tells which dead actors the player's camera sees at this measurement
    /// (in its view, nothing solid between): the others are not seen.
    pub fn set_camera_sights(&mut self, seen: impl IntoIterator<Item = FormId>) {
        let now = self.time;
        self.senses.camera_sees.clear();
        self.senses.camera_sees.extend(seen.into_iter().map(|r| (r, now)));
    }

    /// Whether the player's camera saw `target` (a body) at the engine's last
    /// measurement, within [`CAMERA_SIGHT_SECONDS`], and is still there: in the world,
    /// enabled and in a loaded cell. A body disabled (or moved out) since the measurement
    /// is not seen, even within the same frame or while a menu holds the clock
    /// [documented: GECK wiki GetLineOfSight, "Will not return true when checking if a
    /// disabled reference is in view"].
    pub fn camera_sees(&self, target: FormId) -> bool {
        self.senses.camera_sees.get(&target).is_some_and(|&t| self.time - t <= CAMERA_SIGHT_SECONDS)
            && self.present(target)
            && self.cell_of(target).is_some_and(|c| self.loaded_cells.contains(&c))
    }
}

/// `actor.GetLineOfSight target`: 1 when the calling actor can see the target: nothing
/// solid between them and within ±95° of where it looks [documented: GECK wiki
/// GetLineOfSight]. Read from the detection the engine last measured for the pair
/// (`crate::detection`, `Detection::seen`: line of sight and field of view): every two
/// living actors of the loaded area within detection reach of each other
/// (`crate::sight`). On the player it is the camera that sees, from where the camera is
/// and along its view [documented: same page, "it returns true if the *camera* can see
/// the object"; observed: player.GetLineOfSight Ringo 1, function probes fp1,
/// 2026-10-17]. A pair not measured reads 0 [engineering choice].
///
/// A dead actor is in no detection pair; the engine measures the drawn bodies from the
/// player's camera alone, the same view and wall check aimed at the body where it lies
/// (its ragdoll's trunk), and tells which it sees ([`Game::set_camera_sights`]): the
/// player reads those [documented: the page lets the player test any object, an actor
/// dead or alive among them; Fallout3.esm's MS04Script (TTW "Those!") advances on
/// `Player.GetLOS FredWilksRef == 1` with Fred Wilks lying dead, #3232]. On a dead
/// target that is the only answer: a pair measured while it lived (kept up to a second
/// after the death) is not read, so a body the player turned from reads 0 at the next
/// measurement. Another actor's GetLOS on a body reads 0 [hypothesis: the page says only
/// that its target must be an actor]. A target that is not an actor is not measured: 0 [documented: on a
/// non-player actor GetLOS returns false for anything but an actor; on the player the
/// original also tests other objects, which NVForever does not yet].
pub fn get_line_of_sight(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    let target = a.form(0)?;
    if g.is_dead(target) {
        return yes(r == FormId::PLAYER_REF && g.camera_sees(target));
    }
    yes(g.detection.get(r, target).is_some_and(|d| d.seen))
}

/// `IsPlayerGrabbedRef ref`: 1 while the player holds that reference with the grab
/// [documented: GECK wiki IsPlayerGrabbedRef, "returns true if the player is currently
/// grabbing the selected reference"]; the engine tells what is held
/// ([`Game::set_player_grab`]). 0 at rest [observed: function probes, script-function-
/// coverage.md]. The shipped uses ask it of corpses carried into a trigger
/// (VMS18CarlyleStClairREF, ProperBurialCorpseref) [observed: their scripts].
pub fn is_player_grabbed_ref(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = a.form(0)?;
    yes(g.senses.grabbed == Some(r))
}

/// `IsPC1stPerson`: 1 while the camera is in first person [documented: GECK wiki
/// IsPC1stPerson]; the engine tells it ([`Game::set_first_person`]).
pub fn is_pc_1st_person(g: &mut Game, _ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    yes(g.senses.first_person)
}

/// `actor.AlwaysShowActorSubtitles toggle`: 1 shows the actor's subtitles always, 0
/// back to normal [documented: catalogue summary]. "Always" is taken to mean also out
/// of hearing, where a line otherwise shows none (`nvf_play::voice::audible`)
/// [hypothesis: the original's "normal" also hides subtitles by distance].
pub fn always_show_actor_subtitles(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    if a.num_or(0, 0.0) != 0.0 {
        g.senses.always_subtitles.insert(r);
    } else {
        g.senses.always_subtitles.remove(&r);
    }
    none()
}

/// `actor.IsTurning`: 0 not turning, 1 turning left, 2 turning right [documented: GECK
/// wiki IsTurning]. The engine tells it with the rest of the movement
/// ([`Game::set_motion`]; `nvforever` `detection.rs` measures the heading's change).
pub fn is_turning(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject_or_player();
    Ok(Value::Num(g.motion(r).turning.min(2) as f64))
}

/// `IsPlayerMovingIntoNewSpace`: 1 while the player is moving into a new space, through a
/// load door or by fast travel [documented: GECK wiki IsPlayerMovingIntoNewSpace]: while
/// the engine holds the game under a loading screen ([`Game::loading`], set by
/// `nvf_play::script` from the travel's frozen frames). Scripts do not run then, so a
/// script reads 0 in its own blocks, as the page's note warns [documented: "may only
/// work within a MenuMode, OnLoad or similar block"]; package conditions evaluated during
/// the load see 1.
pub fn is_player_moving_into_new_space(g: &mut Game, _ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    yes(g.loading)
}

/// `actor.SetForceSneak flag`: forced sneaking on (non-zero) or off [documented: GECK
/// wiki SetForceSneak]. The engine crouches the actor and walks it at the sneaking pace
/// while it holds ([`Game::sneaks`]).
pub fn set_force_sneak(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    g.extra.entry(r).or_default().force_sneak = a.num_or(0, 0.0) != 0.0;
    none()
}

impl Game<'_> {
    /// Whether `actor`'s current package (as last chosen) has the PKDT general flag `flag`
    /// (`crate::package::flags`).
    pub fn package_has(&self, actor: FormId, flag: u32) -> bool {
        self.extra.get(&actor).and_then(|e| e.current_package).and_then(|p| self.package_def(&p)).is_some_and(|d| d.has(flag))
    }

    /// Whether `actor`'s current package allows a Fallout behaviour (a
    /// `crate::package::fallout` bit): the bits count only on a package with "Skip
    /// Fallout Behavior" set (docs/spec-notes/packages.md §3); with no package, yes.
    pub fn package_allows(&self, actor: FormId, behaviour: u16) -> bool {
        let def = self.extra.get(&actor).and_then(|e| e.current_package).and_then(|p| self.package_def(&p));
        def.is_none_or(|d| !d.has(crate::package::flags::SKIP_FALLOUT_BEHAVIOR) || d.fallout_flags & behaviour != 0)
    }

    /// Whether the actor sneaks now, as IsSneaking reads it: what the engine measured this
    /// frame ([`Game::set_motion`]), else the rule of [`Game::sneaks`].
    pub fn is_sneaking(&self, actor: FormId) -> bool {
        self.senses.motion.get(&actor).map_or_else(|| self.sneaks(actor), |m| m.sneaking)
    }

    /// Whether the HUD's compass gives the actor a tick (docs/spec-notes/hud.md §11): alive,
    /// present and not sneaking [observed: the owner, 2026-10-23, "npc's sneaking should
    /// not show up on the compass"]. Range and bearing are the engine's to check.
    pub fn compass_shows(&self, actor: FormId) -> bool {
        !self.is_dead(actor) && self.present(actor) && !self.is_sneaking(actor)
    }

    /// Whether an actor other than the player sneaks: SetForceSneak holds, or its current
    /// package has "Always Sneak" ("Actor always sneaks during this package"
    /// [documented: GECK wiki Packages, Flags tab]). In a combat state an actor stands up
    /// to fight unless its package goes on during combat ("Continue During Combat")
    /// [hypothesis]. A teammate following the player sneaks while the player does
    /// [documented: GECK wiki SetPlayerTeammate].
    pub fn sneaks(&self, actor: FormId) -> bool {
        if actor == FormId::PLAYER_REF || self.extra.get(&actor).is_some_and(|e| e.dead) {
            return false;
        }
        // ITR's ForceCrouch and DisableCrouching over every other rule [documented:
        // ForceCrouch; DisableCrouching, "regardless of what the combat AI, follower
        // behaviour, or movement packages request"] (`crate::nvse_itr1`).
        if let Some(c) = self.itr_crouch(actor) {
            return c;
        }
        if self.extra.get(&actor).is_some_and(|e| e.force_sneak) {
            return true;
        }
        // Crouched to reload in a weapon use (`crate::use_weapon`).
        if self.weapon_uses.get(&actor).is_some_and(|w| w.crouched) {
            return true;
        }
        use crate::package::flags;
        let fighting = self.awareness.state(actor) == crate::awareness::State::Combat;
        // Crouched to fire by the combat AI (docs/design/combat-ai.md §14.2).
        if fighting && self.awareness.crouching.contains(&actor) {
            return true;
        }
        // A person hiding at its flee spot, in any combat state (§12.2.1).
        if self.at_flee_spot(actor) && self.world.placed.get(actor).is_some_and(|p| &p.sig == b"ACHR") {
            return true;
        }
        // A teammate following the player imitates the player's sneak state
        // (`crate::companion_wheel`); in a fight it stands up to fight [hypothesis, as for
        // the packages' Always Sneak].
        if !fighting && self.sneaks_with_player(actor) {
            return true;
        }
        self.package_has(actor, flags::ALWAYS_SNEAK) && (!fighting || self.package_has(actor, flags::CONTINUE_DURING_COMBAT))
    }
}

/// IsInDangerousWater: 1 when the player is in a water with "Causes Damage"
/// (`crate::radiation`); 0 on anyone else [documented: GECK wiki IsInDangerousWater;
/// observed: 0 in the Mojave's water, whose types lack the flag (physics.md); hypothesis:
/// only the player's water is known].
pub fn is_in_dangerous_water(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject_or_player();
    yes(r == FormId::PLAYER_REF && g.in_dangerous_water())
}

/// GetRadiationLevel: the radiation level of the player's surroundings, rads a second
/// after Rad Resistance (`crate::radiation`, `Exposure::resisted`); 0 on any other actor
/// [documented: GECK wiki GetRadiationLevel and GetRadiationLevelAlt; observed: the
/// original's 9.013784 in the test site's shack is the marker's level there times
/// (1 − 8 ÷ 100) for Rad Resistance 8, issue #894].
pub fn get_radiation_level(g: &mut Game, ctx: &Ctx, _a: &Args) -> Result<Value, String> {
    let r = ctx.subject()?;
    Ok(Value::Num(if r == FormId::PLAYER_REF { g.radiation().resisted } else { 0.0 }))
}

#[cfg(test)]
mod player_grab_event_tests {
    use crate::common::*;
    use crate::game::{event, Game, GameConfig, RunCtx};
    use crate::GameData;
    use fnv_esm::{LoadOrder, Plugin, Schema};
    use fnv_script::vm::XVal;
    use fnv_script::FunctionTable;
    use fnv_sim::{FormId, FormStore, RefTable, SimConfig, World};

    const CELL_ID: u32 = 0x200;
    const MISC_BASE: u32 = 0x800;
    const ACTI_BASE: u32 = 0x801;
    const SCRIPT_ID: u32 = 0x802;
    const GRAB_LIST: u32 = 0x803;
    const MISC_REF: FormId = FormId(0x300);
    const ACTI_REF: FormId = FormId(0x301);

    const G_BLOCK_GRAB: u32 = 0xA00;
    const G_BLOCK_REL: u32 = 0xA01;
    const G_PLAIN_GRAB: u32 = 0xA02;
    const G_PLAIN_REL: u32 = 0xA03;
    const G_TYPE_MISC_GRAB: u32 = 0xA04;
    const G_TYPE_ACTI_GRAB: u32 = 0xA05;
    const G_TYPE_WEAP_GRAB: u32 = 0xA06;
    const G_TYPE_MISC_REL: u32 = 0xA07;
    const G_FIRST_BASE: u32 = 0xA08;
    const G_FIRST_LIST: u32 = 0xA09;
    const G_ALT_FILTER: u32 = 0xA0A;
    const G_ORDER: u32 = 0xA0B;
    const G_GRAB_SEEN: u32 = 0xA0C;
    const G_REL_SEEN: u32 = 0xA0D;
    const G_NAME_OK: u32 = 0xA0E;

    const UDF_GRAB: u32 = 0xB00;
    const UDF_REL: u32 = 0xB01;

    struct Fixture {
        data: GameData,
        forms: FormStore,
        refs: RefTable,
    }

    fn glob(id: u32, edid: &str) -> Vec<u8> {
        record(b"GLOB", 0, id, &[sub(b"EDID", z(edid)), sub(b"FNAM", vec![b'f']), sub(b"FLTV", 0f32.to_le_bytes().to_vec())])
    }

    fn udf(id: u32, edid: &str, source: &str) -> Vec<u8> {
        let mut fields = vec![sub(b"EDID", z(edid))];
        fields.extend(unit(0, &script(&[(0, None, Vec::new())]), &[], &[]));
        fields.push(sub(b"SCTX", source.as_bytes().to_vec()));
        record(b"SCPT", 0, id, &fields)
    }

    fn placed(id: FormId, edid: &str, base: u32) -> Vec<u8> {
        let data = [0f32; 6].iter().flat_map(|v| v.to_le_bytes()).collect();
        record(b"REFR", 0, id.0, &[sub(b"EDID", z(edid)), sub(b"NAME", base.to_le_bytes().to_vec()), sub(b"DATA", data)])
    }

    fn fixture() -> Fixture {
        let globs = [
            glob(G_BLOCK_GRAB, "BlockGrab"),
            glob(G_BLOCK_REL, "BlockRel"),
            glob(G_PLAIN_GRAB, "PlainGrab"),
            glob(G_PLAIN_REL, "PlainRel"),
            glob(G_TYPE_MISC_GRAB, "TypeMiscGrab"),
            glob(G_TYPE_ACTI_GRAB, "TypeActiGrab"),
            glob(G_TYPE_WEAP_GRAB, "TypeWeapGrab"),
            glob(G_TYPE_MISC_REL, "TypeMiscRel"),
            glob(G_FIRST_BASE, "FirstBase"),
            glob(G_FIRST_LIST, "FirstList"),
            glob(G_ALT_FILTER, "AltFilter"),
            glob(G_ORDER, "OrderLog"),
            glob(G_GRAB_SEEN, "GrabSeen"),
            glob(G_REL_SEEN, "RelSeen"),
            glob(G_NAME_OK, "NameOk"),
        ]
        .concat();
        // Object script on ACTI_BASE with real OnGrab and OnRelease blocks (string_var admits SCTX).
        let mut acti_scpt_fields = vec![sub(b"EDID", z("GrabActiScript"))];
        acti_scpt_fields.extend(unit(
            0,
            &script(&[(event::ON_GRAB, None, Vec::new()), (event::ON_RELEASE, None, Vec::new())]),
            &[],
            &[],
        ));
        acti_scpt_fields.push(sub(
            b"SCTX",
            b"scn GrabActiScript\nstring_var sMarker\nBegin OnGrab\n  set BlockGrab to BlockGrab + 1\n  set OrderLog to OrderLog * 10 + 1\nEnd\nBegin OnRelease\n  set BlockRel to BlockRel + 1\n  set OrderLog to OrderLog * 10 + 2\nEnd\n".to_vec(),
        ));
        let scpts = [
            record(b"SCPT", 0, SCRIPT_ID, &acti_scpt_fields),
            udf(
                UDF_GRAB,
                "GrabEventUdf",
                "scn GrabEventUdf\nref rItem\nstring_var sMarker\nBegin Function {rItem}\n  set PlainGrab to PlainGrab + 1\n  set OrderLog to OrderLog * 10 + 3\n  if eval (IsPlayerGrabbedRef rItem) && (GetPlayerGrabbedRef == rItem)\n    set GrabSeen to GrabSeen + 1\n  endif\n  if eval (GetCurrentEventName) == \"OnPlayerGrab\"\n    set NameOk to NameOk + 1\n  endif\nEnd\n",
            ),
            udf(
                UDF_REL,
                "RelEventUdf",
                "scn RelEventUdf\nref rItem\nstring_var sMarker\nBegin Function {rItem}\n  set PlainRel to PlainRel + 1\n  set OrderLog to OrderLog * 10 + 4\n  if eval !(IsPlayerGrabbedRef rItem) && (GetPlayerGrabbedRef != rItem)\n    set RelSeen to RelSeen + 1\n  endif\n  if eval (GetCurrentEventName) == \"OnPlayerRelease\"\n    set NameOk to NameOk + 10\n  endif\nEnd\n",
            ),
        ]
        .concat();
        let misc = record(b"MISC", 0, MISC_BASE, &[sub(b"EDID", z("GrabMiscBase"))]);
        let acti = record(b"ACTI", 0, ACTI_BASE, &[sub(b"EDID", z("GrabActiBase")), sub(b"SCRI", SCRIPT_ID.to_le_bytes().to_vec())]);
        let flst = record(b"FLST", 0, GRAB_LIST, &[sub(b"EDID", z("GrabFilterList")), sub(b"LNAM", ACTI_BASE.to_le_bytes().to_vec())]);
        let npc = record(b"NPC_", 0, FormId::PLAYER_BASE.0, &[sub(b"EDID", z("Player"))]);
        let refs = [
            record(b"ACHR", 0, FormId::PLAYER_REF.0, &[sub(b"EDID", z("PlayerREF")), sub(b"NAME", FormId::PLAYER_BASE.0.to_le_bytes().to_vec())]),
            placed(MISC_REF, "GrabMiscRef", MISC_BASE),
            placed(ACTI_REF, "GrabActiRef", ACTI_BASE),
        ]
        .concat();
        let cell = [
            record(b"CELL", 0, CELL_ID, &[sub(b"EDID", z("GrabCell"))]),
            group(CELL_ID.to_le_bytes(), 6, &group(CELL_ID.to_le_bytes(), 9, &refs)),
        ]
        .concat();
        let bytes = [
            tes4(),
            top(b"GLOB", &globs),
            top(b"SCPT", &scpts),
            top(b"MISC", &misc),
            top(b"ACTI", &acti),
            top(b"FLST", &flst),
            top(b"NPC_", &npc),
            top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cell))),
        ]
        .concat();
        let plugins = [Plugin::parse(&bytes, "PlayerGrabEvents.esm".into()).unwrap()];
        let lo = LoadOrder::new(&plugins).unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
        let schema = Schema::load_dir(root.join("fnv")).unwrap();
        let path = root.join("script/functions.json");
        let catalogue = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let forms = FormStore::build(&lo, &schema);
        let refs = RefTable::build(&lo, &schema);
        let data = GameData::build(&lo, &schema, FunctionTable::load(&path).unwrap(), &catalogue, &refs);
        assert_eq!(data.source_units.len(), 3, "all three synthetic scripts must be admitted from SCTX: {:?}", data.source_units);
        assert!(data.source_units.iter().all(|u| u.error.is_none()), "source compile errors: {:?}", data.source_units);
        Fixture { data, forms, refs }
    }

    fn game(f: &Fixture) -> Game<'_> {
        Game::new(&f.data, World::new(&f.forms, &f.refs, SimConfig::default()), GameConfig { strict: true, ..Default::default() })
    }

    fn q(g: &mut Game, body: &str) -> XVal {
        g.run_query(&format!("Begin Function {{}}\n{body}\nEnd\n"), RunCtx { self_ref: Some(FormId::PLAYER_REF), ..Default::default() })
            .unwrap_or_else(|e| panic!("{body}: {e}"))
    }

    fn glob_val(g: &Game, id: u32) -> f32 {
        g.world.global(FormId(id)).unwrap()
    }

    fn aux_ref(g: &mut Game, key: &str, idx: u32) -> FormId {
        match q(g, &format!("SetFunctionValue (PlayerREF.AuxVarGetRef \"{key}\" {idx})")) {
            XVal::Ref(r) => FormId(r),
            v => panic!("AuxVarGetRef {key}[{idx}]: {v:?}"),
        }
    }

    #[test]
    fn on_player_grab_and_release_dispatch_plain_typed_filtered_symmetric_and_removed() {
        let f = fixture();
        let mut g = game(&f);

        // Vanilla / no-handler control: unscripted ref produces (false, false) and no events.
        assert_eq!(g.set_player_grab(Some(MISC_REF)), (false, false));
        assert_eq!(g.set_player_grab(None), (false, false));

        // Register plain UDFs, typed handlers (MISC=31, ACTI=21, WEAP=40), base/list filters, and SetEventHandlerAlt.
        q(
            &mut g,
            "SetEventHandler \"OnPlayerGrab\" GrabEventUdf \"priority\"::2\n\
             SetEventHandler \"OnPlayerRelease\" RelEventUdf \"priority\"::2\n\
             SetEventHandler \"OnPlayerGrab\" (Begin Function {ref r}\n  PlayerREF.AuxVarSetRef \"*grabbed\" r -1\nEnd) \"priority\"::1\n\
             SetEventHandler \"OnPlayerRelease\" (Begin Function {ref r}\n  PlayerREF.AuxVarSetRef \"*released\" r -1\nEnd) \"priority\"::1\n\
             SetEventHandler \"OnPlayerGrab:31\" (Begin Function {ref r}\n  set TypeMiscGrab to TypeMiscGrab + 1\nEnd)\n\
             SetEventHandler \"OnPlayerGrab:21\" (Begin Function {ref r}\n  set TypeActiGrab to TypeActiGrab + 1\nEnd)\n\
             SetEventHandler \"OnPlayerGrab:40\" (Begin Function {ref r}\n  set TypeWeapGrab to TypeWeapGrab + 1\nEnd)\n\
             SetEventHandler \"OnPlayerRelease:31\" (Begin Function {ref r}\n  set TypeMiscRel to TypeMiscRel + 1\nEnd)\n\
             SetEventHandler \"OnPlayerGrab\" (Begin Function {ref r}\n  set FirstBase to FirstBase + 1\nEnd) \"first\"::GrabMiscBase\n\
             SetEventHandler \"OnPlayerGrab\" (Begin Function {ref r}\n  set FirstList to FirstList + 1\nEnd) \"first\"::GrabFilterList\n\
             SetEventHandlerAlt \"OnPlayerGrab\" (Begin Function {ref r}\n  set AltFilter to AltFilter + 1\nEnd) 0::GrabActiRef 1::GrabActiBase",
        );

        // Grab MISC_REF: fires plain, :31, first::GrabMiscBase; repeating does not double-fire.
        assert_eq!(g.set_player_grab(Some(MISC_REF)), (false, false));
        assert_eq!(g.set_player_grab(Some(MISC_REF)), (false, false));
        assert_eq!(glob_val(&g, G_PLAIN_GRAB), 1.0);
        assert_eq!(glob_val(&g, G_TYPE_MISC_GRAB), 1.0);
        assert_eq!(glob_val(&g, G_TYPE_ACTI_GRAB), 0.0);
        assert_eq!(glob_val(&g, G_TYPE_WEAP_GRAB), 0.0);
        assert_eq!(glob_val(&g, G_FIRST_BASE), 1.0);
        assert_eq!(glob_val(&g, G_FIRST_LIST), 0.0);
        assert_eq!(glob_val(&g, G_ALT_FILTER), 0.0);
        assert_eq!(glob_val(&g, G_GRAB_SEEN), 1.0);
        assert_eq!(glob_val(&g, G_NAME_OK), 1.0);
        assert_eq!(aux_ref(&mut g, "*grabbed", 0), MISC_REF);

        // Direct swap MISC_REF -> ACTI_REF: releases MISC_REF first, then runs ACTI_REF's OnGrab block and handlers.
        assert_eq!(g.set_player_grab(Some(ACTI_REF)), (false, true));
        assert_eq!(glob_val(&g, G_PLAIN_REL), 1.0);
        assert_eq!(glob_val(&g, G_TYPE_MISC_REL), 1.0);
        assert_eq!(glob_val(&g, G_REL_SEEN), 1.0);
        assert_eq!(aux_ref(&mut g, "*released", 0), MISC_REF);
        assert_eq!(glob_val(&g, G_BLOCK_GRAB), 1.0);
        assert_eq!(glob_val(&g, G_PLAIN_GRAB), 2.0);
        assert_eq!(glob_val(&g, G_TYPE_ACTI_GRAB), 1.0);
        assert_eq!(glob_val(&g, G_FIRST_LIST), 1.0);
        assert_eq!(glob_val(&g, G_ALT_FILTER), 1.0);
        assert_eq!(glob_val(&g, G_GRAB_SEEN), 2.0);
        assert_eq!(glob_val(&g, G_NAME_OK), 12.0);
        assert_eq!(aux_ref(&mut g, "*grabbed", 1), ACTI_REF);

        // Release ACTI_REF: runs OnRelease block then OnPlayerRelease; repeating None does not double-fire.
        assert_eq!(g.set_player_grab(None), (true, false));
        assert_eq!(g.set_player_grab(None), (false, false));
        assert_eq!(glob_val(&g, G_BLOCK_REL), 1.0);
        assert_eq!(glob_val(&g, G_PLAIN_REL), 2.0);
        assert_eq!(glob_val(&g, G_REL_SEEN), 2.0);
        assert_eq!(glob_val(&g, G_NAME_OK), 22.0);
        assert_eq!(aux_ref(&mut g, "*released", 1), ACTI_REF);
        // Order: 3 (MISC grab) -> 4 (MISC release) -> 1 (ACTI OnGrab block) -> 3 (ACTI grab) -> 2 (ACTI OnRelease block) -> 4 (ACTI release).
        assert_eq!(glob_val(&g, G_ORDER), 341324.0);

        // RemoveEventHandler stops GrabEventUdf while typed/filter handlers still run.
        assert_eq!(q(&mut g, "SetFunctionValue (RemoveEventHandler \"OnPlayerGrab\" GrabEventUdf)"), XVal::Num(1.0));
        assert_eq!(g.set_player_grab(Some(MISC_REF)), (false, false));
        assert_eq!(glob_val(&g, G_PLAIN_GRAB), 2.0, "removed handler must not fire");
        assert_eq!(glob_val(&g, G_TYPE_MISC_GRAB), 2.0, "remaining typed handler still fires");
        assert_eq!(g.set_player_grab(None), (false, false));
        assert_eq!(glob_val(&g, G_PLAIN_REL), 3.0);
        assert!(
            g.events.iter().all(|e| !matches!(e.kind, crate::trace::EventKind::ScriptError { .. })),
            "unexpected script errors: {:?}",
            g.events
        );
    }
}
