//! VATS mode (docs/design/vats-mode.md): NVForever's own VATS, a Dead Eye-style slow-time
//! mark and fire, not the original's [design: combat is not a parity target].
//!
//! - **Enter** (§3): V (`Control::Vats`) with a gun in hand, not reloading or jammed, a
//!   round loaded and the AP for a shot. With the weapon away V draws it first and enters
//!   once it is out.
//! - **Marking** (§6, §7): the clock runs at [`MARK_SCALE`] (eased in over the swipe,
//!   [`time_scale_at`]); left click (Use) marks the actor under the crosshair, on the body
//!   part the crosshair's ray strikes; each mark reserves the shot's AP
//!   (`fnv_combat::vats::MarkQueue`: no more marks than the AP left and the rounds loaded
//!   allow). A tap of right click (Block, let go within [`UNMARK_TAP`] real seconds)
//!   takes the last mark back, or with none ends the mode; held, it aims down the sights
//!   (§11.5); V again ends it; E (Activate) fires.
//! - **Firing** (§9): the clock runs at [`FIRE_SCALE`] (half speed); every
//!   [`SHOT_INTERVAL`] of game time the next mark in
//!   order: the view snaps to it, the hit is rolled on the mark's chance (§17) and
//!   `crate::combat` fires one shot straight at it, or wide of it on a miss
//!   ([`VatsMode::take_shot`]: no spread, no recoil, no jam, whatever the weapon's
//!   cooldown), and its AP is spent. [`AFTER_LAST`] after the last shot the mode is off
//!   and the clock is back at 1. In first person the player's own aim stays (a scope
//!   stays up); in third person a gun with sights is raised for a mark farther than 20 m
//!   and fired from the hip nearer, as the NPCs do (`crate::combat::aims_down_sights`).
//! - **The look** (§5): the swipe, the amber grade and its fading out are four numbers
//!   ([`VatsMode::look`]) the renderer's image-space pass draws (nvforever
//!   `imagespace.rs`); the marks' "x"s are drawn here ([`draw_marks`]).
//! - **Hit chance** (§17): each mark's chance to hit (`fnv_combat::vats::raw_chance`,
//!   `final_chance`: the part's BPTD To Hit Chance, the weapon's skill, the spread at the
//!   mark's distance, the player's to-hit perks, Concentrated Fire, the 95 % cap) is worked
//!   out again every frame and drawn as "NN%" beside its "x" ([`draw_labels`]).
//!   Before marking, the actor under or nearest the crosshair shows every body part's
//!   chance on the part, the same work as a mark's ([`spot_on`]), the part under the
//!   crosshair bracketed and brighter ([`draw_previews`], §17.5).
//! - **Helpers and AP perks** (§18): with Mysterious Stranger or Miss Fortune, a mark's
//!   attack may bring the helper beside the player, who fires one shot of their own
//!   weapon at the mark (an `ActorFire`) and fades away; Friendly Help doubles the odds.
//!   Grim Reaper's Sprint gives AP back after a sequence that killed; Nerves of Steel
//!   speeds the AP's regeneration.
//! - **Sounds** (§10): the base game's `UIVATS*` SOUN records, found by Editor ID.
//! - **AP** (§7.2): reserved while marking (the HUD's bar shows what is left after the
//!   reservations, [`VatsMode::reserved`]), spent per shot; outside the mode spent AP comes
//!   back at `fActionPointsRestoreRate` × the maximum a second [hypothesis].
//! - **Scripts** (§12): `fnv_game::Game::set_vats` each frame (GetVATSMode, GetVATSValue).
//! - **Beyond single shots** (§16): an automatic weapon fires a burst a mark
//!   (`fnv_combat::vats::Volley::bursts`); a melee or unarmed weapon marks within 5 m and,
//!   firing, puts the player beside each mark and swings once (`fnv_combat::vats::MeleeRun`,
//!   [`stand_beside`]; the swing is combat's own, [`VatsMode::take_swing`], landing on the mark,
//!   [`VatsMode::melee_mark`]); a grenade or thrown weapon is thrown on the low arc that
//!   lands on each mark (`crate::combat::fire`, `fnv_combat::vats::lob_dir`).
//! - Any menu, a conversation (a forced greeting too), death, a script disabling the
//!   fighting controls or the weapon leaving the hand ends the mode (§9.3, §11.6), and so
//!   does any action but marking, aiming and accepting ([`LEAVING`]: E with nothing
//!   marked activates what is under the crosshair, Wait, Jump, Sneak, the hotkeys…); a
//!   weapon that does not come out within [`DRAW_LIMIT`] gives up entering (§11.8).
//!   Nothing of it is saved (§11.7).
//!
//! Log lines (smoke tests read them): `vats mode: on (…)`, `vats mode: refused: …`,
//! `vats mode: marked …`, `vats mode: mark refused: …`, `vats mode: unmarked …`,
//! `vats mode: firing …`, `vats mode: shot k of n …` (`burst`, `throw`, `swing` for the
//! other kinds, and `closing on …` before a swing), `vats mode: preview …`, `vats mode: off (…)`, and at exit
//! `vats mode summary: …`.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use fnv_combat::rng::SplitMix;
use fnv_combat::vats::{
    final_chance, miss_point, raw_chance, restored_damage, rolled_hit, throw_miss_point, AttackStyle, SpecialAttack, HitInputs, HitTuning, MarkQueue, MeleeAction, MeleeLimits, MeleeRun, Volley, VolleyEvent, HELD_WEAPON_TO_HIT,
    SIZE_HELD_WEAPON, SIZE_TORSO, WHOLE_TO_HIT,
};
use fnv_game::vats::{pick_helper, Helper, HitPerks};
use fnv_game::actor_values::ACTION_POINTS;
use fnv_phys::{layers, Filter, Owner};
use fnv_sim::FormId;
use std::collections::HashMap;

use crate::activate::HudMessage;
use crate::bindings::{Control, ControlInput};
use crate::combat::{ActorFire, Combat, WeaponAnim};
use crate::control::{ControlLocks, Crosshair, PlayerControl};
use crate::hitvol::{ActorParts, WEAPON_PART};
use crate::interface::{bc_supported, face as font_face, plain_font, set_line, Face};
use bevy::image::CompressedImageFormatSupport;
use nvf_assets::GameData;
use crate::phys::PhysState;
use crate::player::{MoveIntent, Player};
use crate::script::{with_world, ScriptState};
use crate::sfx::{LoopSound, PlaySound, Sfx};
use crate::sim::SimState;
use crate::vats_classic::{self as classic, Candidate, VatsSlowdown, VatsStyle};
use crate::{space, PlaySet, PlayState, PlayerCamera};

pub mod touch;

/// The clock's speed while marking [design: the owner's "very, very slow"].
pub const MARK_SCALE: f32 = 0.05;
/// The swipe across the screen on entering, and the easing of the clock with it, real
/// seconds [design].
pub const ENTER_SWIPE: f32 = 0.35;
/// The wash over the whole screen after the swipe fades over this long, real seconds
/// [design, after the references' flash].
pub const ENTER_WASH: f32 = 0.6;
/// The wash's strength when the swipe has crossed [design].
pub const WASH_START: f32 = 0.5;
/// The band riding the swipe's edge [design].
pub const BAND_STRENGTH: f32 = 0.85;
/// The grade fades out over this long on leaving, real seconds [design].
pub const LEAVE_FADE: f32 = 0.3;
/// The clock's speed while the marked shots are fired [design: owner 2026-10-18, "the
/// firing is too fast": half speed instead of normal].
pub const FIRE_SCALE: f32 = 0.5;
/// Between two marked shots, game seconds: 0.4 s of real time at [`FIRE_SCALE`]
/// [design: owner 2026-10-18; was 0.12 at full speed].
pub const SHOT_INTERVAL: f32 = 0.2;
/// From the last shot to the end of the mode, game seconds (0.6 s real at
/// [`FIRE_SCALE`]) [design].
pub const AFTER_LAST: f32 = 0.3;
/// A right click let go within this many real seconds of its press, while marking, is a
/// tap: it takes the last mark back; held longer it aims down the sights (§11.5)
/// [design: owner 2026-10-23, "let the player aim or not whilst vats slow time is
/// running"; the aim waits out the tap so a tap does not twitch the zoom].
pub const UNMARK_TAP: f32 = 0.25;
/// V with the weapon away waits this long, real seconds, for the weapon to come out; then
/// entering is given up (§11.8) [design: a draw is well under a second].
pub const DRAW_LIMIT: f32 = 3.0;
/// The controls whose press ends the mode (§11.8): every action but marking (Use),
/// aiming or unmarking (Block), V and accepting the marks (Activate while marking with a
/// mark made, [`leaving_press`]); moving, looking and the camera go on [design: owner
/// 2026-10-25, "any action besides aiming and marking or accepting marks" kicks the
/// player out, so they are never stuck in the mode].
pub const LEAVING: [Control; 17] = [
    Control::Activate,
    Control::ReadyItem,
    Control::CrouchSneak,
    Control::Jump,
    Control::MenuMode,
    Control::Rest,
    Control::Hotkey1,
    Control::AmmoSwap,
    Control::Hotkey3,
    Control::Hotkey4,
    Control::Hotkey5,
    Control::Hotkey6,
    Control::Hotkey7,
    Control::Hotkey8,
    Control::QuickSave,
    Control::QuickLoad,
    Control::Grab,
];

/// The control pressed this frame that ends the mode (§11.8), if any: one of [`LEAVING`],
/// except Activate while marking with `marks` made, which fires them.
pub fn leaving_press(phase: Phase, marks: usize, pressed: impl Fn(Control) -> bool) -> Option<Control> {
    if phase == Phase::Off {
        return None;
    }
    let accepts = phase == Phase::Marking && marks > 0;
    LEAVING.into_iter().find(|&c| pressed(c) && !(c == Control::Activate && accepts))
}

/// How far the crosshair's ray looks for a target when the projectile has no range of
/// its own, units [design].
pub const REACH: f32 = 8192.0;
/// How far from the eye a melee weapon's marks may be in the Dead Eye style, units
/// (§16.2) [design: owner 2026-10-20, "within 5 meters or so"; 5 m at 70 units a metre].
/// The Classic style has the original's reach instead (`Combat::vats_weapon`'s
/// `classic_reach`, issue #2656); Dead Eye keeps this one, the maintainer's own choice
/// for that style.
pub const MELEE_MARK_REACH: f32 = 350.0;
/// A melee mark is close enough to swing at within the weapon's reach plus this across
/// the ground, units: the two bodies' radii (§16.2) [design].
pub const CLOSE_SLACK: f32 = 64.0;
/// The melee sequence's limits, game seconds (§16.2) [design]: the longest run at a mark,
/// the longest a swing is waited for.
pub const APPROACH_LIMIT: f32 = 2.5;
pub const SWING_LIMIT: f32 = 2.0;
/// Where a teleport looks for the ground: from this far above the spot to this far
/// below, units [design].
const TELEPORT_UP: f32 = 150.0;
const TELEPORT_DOWN: f32 = 600.0;

/// How far from a melee mark's point the player is put, across the ground: half the
/// weapon's reach plus half [`CLOSE_SLACK`] (§16.2) [design].
pub fn stand_off(reach: f32) -> f32 {
    reach * 0.5 + CLOSE_SLACK * 0.5
}

/// The feet `off` units from `mark` across the ground, on the side of `feet` (on the
/// player's line to it), at the mark's height (the ground is looked for from there).
pub fn stand_beside(feet: [f32; 3], mark: [f32; 3], off: f32) -> [f32; 3] {
    let (dx, dy) = (feet[0] - mark[0], feet[1] - mark[1]);
    let len = (dx * dx + dy * dy).sqrt();
    let (ux, uy) = if len > 1e-3 { (dx / len, dy / len) } else { (1.0, 0.0) };
    [mark[0] + ux * off, mark[1] + uy * off, mark[2]]
}

/// Between two throws, game seconds (§16.3) [design].
pub const THROW_INTERVAL: f32 = 0.35;
/// The mode stays, its time slowed, while an explosive it sent is on its way to its mark
/// (`crate::combat::VatsBlast`) and until this many real seconds after it went off: the
/// blast's own show, then what the owner asked for after it [design: the fork owner
/// 2026-11-10, "vats slomo should last until and 2 seconds after the explosion ends unless
/// the projectile [is] flying away from the target"; an explosion's length is the
/// renderer's, about a second of its effect at the firing's half speed].
pub const BLAST_SHOWN: f64 = 1.0;
pub const AFTER_BLAST: f64 = 2.0;
/// After a launched explosive (not a thrown one) has gone off the mode stays this many
/// real seconds in all [design: the fork owner 2026-11-10, "shorten the vats camera after
/// luncher explosion to 1s after hit"].
pub const AFTER_LAUNCHED_BLAST: f64 = 1.0;
/// An explosive on its way is waited for this many real seconds at most [design: a mine
/// or a dud that never goes off must not hold the mode].
pub const BLAST_PATIENCE: f64 = 12.0;
/// A throw leaves the hand at its clip's `Hit` key (`crate::combat`), and the firing waits
/// for that, this many real seconds at most [design: longer than any throw's clip at
/// [`FIRE_SCALE`]].
pub const THROW_PATIENCE: f64 = 6.0;

/// A helper fires this long (game seconds) after appearing, and leaves this long after
/// appearing (§18.2) [design].
pub const HELPER_DRAW: f32 = 0.3;
pub const HELPER_STAY: f32 = 1.2;
/// A helper's eye above its feet, units: where its shot leaves from [design: a person's].
pub const HELPER_EYE: f32 = 110.0;
/// A mark's "x": its size as a share of the screen width, and its colour: the interface's
/// amber (`crate::interface::AMBER`) lifted [`MARK_LIFT`] of the way to white, so it
/// stands out on the amber grade [design: the owner's wishes, 2026-10-24, to match the HUD
/// and then to be brighter; it was a dark red before; owner 2026-10-25: smaller and thinner
/// than 0.019 with 22 % bars].
pub const MARK_SIZE: f32 = 0.013;
/// Each bar's thickness, a share of the "x"'s size [design].
const MARK_BAR: f32 = 10.0;
/// How far the marks' amber is lifted towards white (0 = the HUD's amber) [design].
const MARK_LIFT: f32 = 0.25;
const MARK_COLOUR: Color = Color::srgb(lift(crate::interface::AMBER[0]), lift(crate::interface::AMBER[1]), lift(crate::interface::AMBER[2]));

/// One channel of the interface's amber lifted [`MARK_LIFT`] of the way to white.
const fn lift(c: f32) -> f32 {
    c + (1.0 - c) * MARK_LIFT
}
/// The most marks drawn at once (more are kept but not drawn) [engineering choice].
const MAX_DRAWN: usize = 32;

/// VATS mode's state (§2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Off,
    /// V was pressed with the weapon away: it is being drawn, and the mode starts once it
    /// is out.
    Drawing,
    Marking,
    Firing,
}

/// What was pressed this frame, for the mode.
#[derive(Debug, Clone, Copy, Default)]
struct Keys {
    toggle: bool,
    mark: bool,
    unmark: bool,
    accept: bool,
    /// The classic style's: the target before or after (-1, 1), the part before or after.
    target_step: i32,
    part_step: i32,
    /// The classic style's: queue the weapon's special attack on the selected part.
    special: bool,
    /// The classic style's: the part whose box the pointer came onto this frame (its
    /// place in the preview), from the target screen (`crate::tiles::vats_menu`), or the
    /// part the controller's left stick was pushed towards.
    hover_part: Option<usize>,
    /// The classic style's: the pointer moved this frame; its line into the world (from,
    /// direction, game space), and the part whose point is nearest it on the screen.
    pointer_moved: bool,
    ray: Option<([f32; 3], [f32; 3])>,
    near_part: Option<usize>,
}

/// Where a mark is and what it is on.
#[derive(Debug, Clone, Default)]
struct Spot {
    /// Its point now (game space): the part's capsule centre, followed while the actor
    /// moves; the last one seen once it has no volume.
    point: [f32; 3],
    name: String,
    part: String,
    /// The part's condition actor value (GetVATSValue Targeted Part).
    condition: Option<u16>,
    /// The part's base chance to hit, percent (§17.2).
    base: f32,
    /// The part's size against a torso's, for the range term (§17.2).
    size: f32,
    /// The player's to-hit perks for this mark, read when it was made (§17.3).
    perks: HitPerks,
    /// Marks made before it on the same actor and part (Concentrated Fire).
    earlier: u32,
    /// What the attack would take off the target's Health if it lands, read when it was
    /// queued (`crate::combat::vats_hit_estimate`): the enemy bar shows it.
    damage: f32,
    /// Its chance to hit now, percent (§17.2): drawn beside its "x", rolled on firing.
    chance: f32,
    /// Its last roll while firing (GetVATSValue Is Success).
    landed: Option<bool>,
    /// Queued as the weapon's special attack (the classic style, vats-classic.md §9).
    special: bool,
    /// A projectile in flight (the player's Target Projectiles in V.A.T.S. tweak): its
    /// number in `crate::combat`; the mark's target is then `FormId(0)`.
    projectile: Option<u64>,
    /// That projectile no longer flies: the mark is dropped as a dead actor's.
    lost: bool,
    /// How much of the part is in sight (0 to 1), under the player's V.A.T.S. Throwables
    /// Use Visibility tweak for a thrown weapon that is no grenade: the chance is times it.
    seen: Option<f32>,
}

/// The weapon VATS mode was entered with.
#[derive(Debug, Clone)]
struct InHand {
    id: FormId,
    reach: f32,
    style: AttackStyle,
    /// A melee weapon's own reach, units.
    melee_reach: f32,
    /// Its WEAP Sight FOV (0: no sights).
    sight_fov: f32,
    /// The player's skill with it, and the VATS settings of the hit chance (§17).
    skill: f32,
    hit: HitTuning,
    /// A melee weapon's special attack the player's skill allows, and what it costs
    /// after the Action Point Cost perks (vats-classic.md §9).
    special: Option<(SpecialAttack, f32)>,
}

/// A helper in the world (§18.2).
#[derive(Debug, Clone)]
struct ActiveHelper {
    who: Helper,
    reference: FormId,
    weapon: FormId,
    /// Where they stand (feet), and the mark they came for.
    feet: [f32; 3],
    target: FormId,
    part: Option<u8>,
    point: [f32; 3],
    /// Game seconds since they appeared, and whether they have fired.
    age: f32,
    fired: bool,
}

/// How the marks are being spent (§9, §16).
#[derive(Debug, Clone)]
enum Run {
    /// Shots, bursts or throws at fixed intervals.
    Volley(Volley),
    /// Close in and swing at each mark.
    Melee(MeleeRun),
}

impl Run {
    fn all_fired(&self) -> bool {
        match self {
            Run::Volley(v) => v.all_fired(),
            Run::Melee(m) => m.all_fired(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct VatsStats {
    pub entered: u32,
    pub marks: u32,
    pub shots: u32,
    pub ap_spent: f32,
    pub refused: u32,
    /// Rolls that hit and missed (§17.4).
    pub hits: u32,
    pub misses: u32,
    /// Helpers that came, and the AP Grim Reaper's Sprint gave back (§18).
    pub helpers: u32,
    pub ap_rewarded: f32,
}

#[derive(Resource, Default)]
pub struct VatsMode {
    pub phase: Phase,
    queue: MarkQueue,
    spots: Vec<Spot>,
    run: Option<Run>,
    weapon: Option<InHand>,
    /// Marks whose attack has started (their AP spent).
    started: usize,
    /// A melee sequence: the swing due this frame (`crate::combat::melee` taps Use), the
    /// mark being swung at, and the mark last teleported to.
    swing_now: bool,
    striking: Option<usize>,
    closing: Option<usize>,
    /// Real seconds (`Time<Real>`) when marking began, and when the look began to go.
    entered_at: Option<f64>,
    left_at: Option<f64>,
    now: f64,
    keys: Keys,
    /// The point the shot `crate::combat` fires this frame aims at.
    shot: Option<[f32; 3]>,
    /// Projectiles a marked shot this frame landed on, with the weapon
    /// (`crate::combat::Combat::shoot_down`).
    downed: Vec<(u64, FormId)>,
    /// A throw handed to `crate::combat` that has not left the hand yet (its clip winds
    /// up to the `Hit` key): since when (`now`). The firing waits for it.
    throw_held: Option<f64>,
    /// The explosions of `crate::combat::VatsBlast` seen, the real second (`now`) until
    /// which the last one holds the mode, since when an explosive on its way does, and
    /// how many were on their way the frame before.
    blast_seen: u32,
    blast_until: Option<f64>,
    blast_since: Option<f64>,
    blast_flying: u32,
    /// The real second (`now`) the last attack left to play was dropped because its
    /// target had died: the mode ends [`AFTER_KILL`] after it.
    kill_end: Option<f64>,
    /// How much faster than the world's clock the player's own attack runs now: the
    /// playing camera shot's Player time multiplier (`crate::kill_cam`), else 1 (0 before
    /// the first frame).
    player_rate: f32,
    /// The mark shot last, for the scripts (GetVATSValue).
    shooting: Option<usize>,
    /// This stay's shots and AP spent.
    fired: u32,
    spent: f32,
    /// Real seconds when Block went down while marking (a tap unmarks, §11.5).
    block_at: Option<f64>,
    /// Real seconds when the weapon began to be drawn for the mode ([`DRAW_LIMIT`]).
    drawing_at: Option<f64>,
    /// The mark the third-person sights rule looks at while firing (the one being fired
    /// or the next), and whether that rule has the sights up (`None`: not decided yet).
    aim_mark: usize,
    sights_up: Option<bool>,
    /// Whether the player aimed down the sights just before the mode began (kept while
    /// it is off), for the V.A.T.S. Hip Fire tweak.
    aimed_before: bool,
    /// The mode drew the weapon itself, and, once it ended with nothing fired, the weapon
    /// is to go away again (the Entering V.A.T.S. Doesn't Unholster tweak).
    drew_weapon: bool,
    put_away: bool,
    /// The view before the First Person V.A.T.S. tweak took the firing into first
    /// person, given back when the mode ends.
    pov_before: Option<crate::control::Pov>,
    /// The hit rolls' generator (seeded, so a run repeats) and this stay's hits and misses.
    rng: Option<SplitMix>,
    hits: u32,
    misses: u32,
    /// The roll of the melee swing being made (§17.4).
    blow_lands: bool,
    /// The helper in the world, if one came (§18.2).
    helper: Option<ActiveHelper>,
    /// The player's kills when the marks began to be fired, and the same once the mode
    /// ended after firing, for Grim Reaper's Sprint (§18.3).
    kills_at_fire: Option<f64>,
    reward_due: Option<f64>,
    /// The AP regeneration factor last applied (Nerves of Steel, §18.4), for the log.
    regen_mult: Option<f64>,
    /// SOUN FormIDs by Editor ID, found on first use.
    sounds: Option<HashMap<String, FormId>>,
    /// Every body part's chance on the actor under or nearest the crosshair, before it is
    /// marked (§17.5), and the parts list it is built from (kept for its room).
    preview: Preview,
    preview_list: Vec<Option<u8>>,
    /// The style this stay was entered in, and the classic style's state
    /// (docs/design/vats-classic.md).
    style: VatsStyle,
    classic: ClassicState,
    /// The classic style's attack cameras (§8 there): the request `crate::kill_cam` reads
    /// with its serial, the stage being waited for (its serial, since when in real
    /// seconds), the mark whose Shoot stage has been asked, and whether they are given up
    /// for this stay.
    cam_ask: Option<(u64, CameraAsk)>,
    cam_serial: u64,
    cam_wait: Option<(u64, CamStage, f64)>,
    cam_led: Option<usize>,
    cam_off: bool,
    /// The attack its Shoot camera leads has been let go.
    cam_released: bool,
    /// A controller works the controls (`crate::bindings::ControlInput::controller`): the
    /// classic screen has no pointer then and shows the controller's buttons.
    pad: bool,
    /// Which way the controller's right stick was pushed last frame (-1, 0, 1), for a push
    /// to change the target once.
    stick_was: i32,
    /// The pointer is on a part of the selected target (its box or its body).
    pointed: bool,
    pub stats: VatsStats,
}

/// When one of a classic attack's cameras plays: before the attack leaves, or after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CamStage {
    Shoot,
    Hit,
}

/// A camera the classic style asks `crate::kill_cam` for: for the queue's attack `mark`,
/// as the scripts see its shot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraAsk {
    pub mark: usize,
    pub stage: CamStage,
    pub shot: fnv_game::vats::VatsShot,
}

/// The mode ends this many real seconds after the last attack left to play was dropped
/// because its target had died [design: the fork owner 2026-11-10, "when the target dies,
/// vats should automatically end after a 1s delay - currently the player character keeps
/// shooting the dead body if the NPC got killed at 2/5 shot"; 2 s since "extend the vats
/// kill/slowmo camera by 1 second", the same day].
pub const AFTER_KILL: f64 = 2.0;

/// The attack cameras are given up for a stay when `crate::kill_cam` has not taken a
/// request after this many real seconds (it does not run without the script runtime).
const CAM_PATIENCE: f64 = 1.0;
/// An attack leaves this many real seconds into its Shoot camera [design: the shot is
/// "activated ... when the command starts" (GECK wiki CameraShot, Camera Action), so the
/// attack belongs in it; the moment before is for the eye to find the attacker].
pub const SHOOT_LEAD: f64 = 0.25;

/// The classic style's state for a stay (docs/design/vats-classic.md §3, §4).
#[derive(Debug, Clone, Default)]
struct ClassicState {
    /// The clock's speed while targets are chosen (`classic::clock_scale` of the setting).
    scale: f32,
    /// The targets, left to right, and the one the view is on.
    targets: Vec<Candidate>,
    selected: Option<u32>,
    /// The selected target's part the next attack is queued on: its place in the preview.
    part: usize,
    /// The part was chosen for this target (by the player or as its best).
    part_set: Option<u32>,
    /// The line the player looked along on entering, and the view then (heading, pitch).
    entry_dir: [f32; 3],
    entry_view: Option<(f32, f32)>,
    /// Where the player stood then (feet, game space): the view is given back only there.
    entry_feet: [f32; 3],
    /// The view to go back to: set on leaving without firing, taken by the next frame.
    restore: Option<(f32, f32)>,
    /// Real seconds when the targets were last looked for.
    scanned_at: f64,
}

/// One body part on the classic style's target screen (vats-classic.md §4, §11).
#[derive(Debug, Clone, PartialEq)]
pub struct ClassicPart {
    /// Its place among the selected target's parts (the mode's own order).
    pub index: usize,
    /// The part's name as the data spells it; empty for a target without parts.
    pub name: String,
    /// The chance to hit it now, percent.
    pub chance: f32,
    /// Its condition, 0 (crippled) to 1; `None` for a part without one.
    pub condition: Option<f32>,
    /// Its condition actor value (25 the head, 26 the torso, 27 to 30 the limbs).
    pub value: Option<u16>,
    /// The part the next attack is queued on.
    pub selected: bool,
    /// Where it is now, game space.
    pub point: [f32; 3],
}

/// What the classic style's target screen shows (`crate::tiles::vats_menu` writes it into
/// the game's menu file; the engine-drawn screen reads the mode itself).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClassicView {
    /// Dead Eye's: the parts' boxes alone, without the classic style's arrows, buttons,
    /// queue and pointer ([`VatsMode::target_screen`]).
    pub bare: bool,
    pub parts: Vec<ClassicPart>,
    /// The queued attacks in their order, each by its words ([`queued_text`]).
    pub queue: Vec<String>,
    /// The weapon's special attack, when the player can use it ([`special_text`]).
    pub special: Option<String>,
}

/// A part's name on the target screen: as the body part data spells it, "Weapon" for the
/// weapon in the target's hand, none for a target without parts.
pub fn screen_name(part: &str) -> String {
    match part {
        "whole" => String::new(),
        "its weapon" => "Weapon".to_string(),
        _ => part.to_string(),
    }
}

/// [design, #2659] Join the data's target and part names with a colon; a target without
/// parts keeps its name alone. Preserve the existing special-attack suffix.
pub fn queued_text(target: &str, part: &str, special: bool) -> String {
    let what = if part.is_empty() || part == "whole" { target.to_string() } else { format!("{target}: {part}") };
    if special {
        format!("{what} (special)")
    } else {
        what
    }
}

/// Left without firing, the view is back where it was once it is within this of it,
/// radians (about a quarter of a degree) [design].
const RESTORE_WITHIN: f32 = 0.004;

/// How far from where the targets were chosen the player may be for the view still to
/// be turned back, units: the player stands still while choosing, so further away is a
/// door, a load or a script's move [design: two paces].
const RESTORE_REACH: f32 = 128.0;

/// Whether the classic style's view is still turned back to where it was (`ClassicState::
/// restore`): not behind a loading screen, and with the player still where the targets
/// were chosen [the owner's review of #2102: after a quick load or a door the view
/// turned toward the old heading for about a second, the mouse and the movement held].
fn restore_stands(loading: bool, feet: [f32; 3], entered: [f32; 3]) -> bool {
    let far = (0..3).map(|i| (feet[i] - entered[i]).powi(2)).sum::<f32>().sqrt();
    !loading && far <= RESTORE_REACH
}

/// The classic style's targets are looked for again this often, real seconds [design:
/// with the world stopped nothing changes; slowed, four times a second is as fresh as
/// `PREVIEW_REBUILD`].
const CLASSIC_SCAN: f64 = 0.25;
/// A target must be within this angle of the line the player looked along on entering,
/// degrees [design: about what the view shows; the original lists the targets in view].
pub const CLASSIC_FRONT: f32 = 55.0;

impl VatsMode {
    /// Marking or firing.
    pub fn active(&self) -> bool {
        matches!(self.phase, Phase::Marking | Phase::Firing)
    }

    /// The mode has Use, Block and Activate for itself (§11.5).
    pub fn holds_input(&self) -> bool {
        self.phase != Phase::Off
    }

    /// The classic style is choosing targets or playing the queued attacks: the player
    /// neither moves nor turns the view, which is the mode's (vats-classic.md §5).
    pub fn holds_view(&self) -> bool {
        self.style == VatsStyle::Classic && (self.active() || self.classic.restore.is_some())
    }

    /// The queued attacks play (either style): the corner HUD is hidden meanwhile
    /// [design: the fork owner 2026-11-10, "when the vats animation plays ... no hud
    /// element should be visible", and for Dead Eye "port all the fixes we put in vats
    /// into deadeye also"] (`crate::hud`, `crate::reticle`, `crate::corner_messages`,
    /// `crate::quest_messages`). [design, #2659] Classic keeps the enemy bar at bottom
    /// centre; Dead Eye's own marks stay ([`VatsMode::drawn`]).
    pub fn playing_back(&self) -> bool {
        self.phase == Phase::Firing
    }

    /// The classic target screen shows the pointer: with the keyboard and mouse, not with
    /// a controller.
    pub fn pointer_wanted(&self) -> bool {
        self.selected().is_some() && !self.pad
    }

    /// A controller works the classic target screen: its buttons are the hints shown.
    pub fn pad(&self) -> bool {
        self.pad
    }

    /// The pointer is on a part of the selected target: the screen's Select button shows.
    pub fn pointed(&self) -> bool {
        self.pointed && self.pointer_wanted()
    }

    /// The style of the stay (the last one, when the mode is off).
    pub fn style(&self) -> VatsStyle {
        self.style
    }

    /// The classic style's header while targets are chosen (vats-classic.md §4): the
    /// selected target's name and its health as a share, once its preview is built.
    pub fn classic_header(&self) -> Option<(String, f32)> {
        let actor = self.selected()?;
        let name = self.preview.parts.first().filter(|_| self.preview.actor == Some(actor))?.spot.name.as_str();
        let share = self.preview.health.map_or(1.0, |(now, max)| if max > 0.0 { (now / max).clamp(0.0, 1.0) } else { 0.0 });
        Some((name.to_string(), share))
    }

    /// The part lit on a target's body (nvforever `vats_highlight`): the classic style's
    /// selected target and the part the next attack would be queued on (`None`: a target
    /// without parts), once its parts are worked out; in Dead Eye, while marking, the
    /// previewed actor's part under the crosshair [design: the fork owner 2026-11-10,
    /// "port all the fixes we put in vats into deadeye also"].
    pub fn highlighted(&self) -> Option<(u32, Option<u8>)> {
        if self.style == VatsStyle::DeadEye {
            let actor = self.preview.actor.filter(|_| self.phase == Phase::Marking)?;
            return self.preview.parts.iter().find(|p| p.under).map(|p| (actor, p.part));
        }
        let actor = self.selected()?;
        let part = self.preview.parts.get(self.classic.part).filter(|_| self.preview.actor == Some(actor))?;
        Some((actor, part.part))
    }

    /// What the classic style's target screen shows now, while targets are chosen and the
    /// selected target's parts are worked out.
    pub fn classic_view(&self) -> Option<ClassicView> {
        let actor = self.selected()?;
        if self.preview.actor != Some(actor) {
            return None;
        }
        let parts = self
            .preview
            .parts
            .iter()
            .enumerate()
            .filter(|(_, p)| p.shown)
            .map(|(index, p)| ClassicPart {
                index,
                name: screen_name(&p.spot.part),
                chance: p.spot.chance,
                condition: p.condition,
                value: p.spot.condition,
                selected: p.under,
                point: p.spot.point,
            })
            .collect();
        let queue = self.spots.iter().map(|s| queued_text(&s.name, &s.part, s.special)).collect();
        let special = self.weapon.as_ref().and_then(|w| w.special).map(|(s, cost)| special_text(s.dam_mult, cost));
        Some(ClassicView { bare: false, parts, queue, special })
    }

    /// [design, #2659] The classic selected target, then the playback attack's target
    /// (the selected target until the first attack starts); Dead Eye keeps its preview.
    pub fn bar_target(&self) -> Option<u32> {
        if self.style == VatsStyle::Classic && self.phase == Phase::Firing {
            return self.shooting.and_then(|i| self.queue.marks().get(i)).map(|m| m.target.0).or(self.classic.selected);
        }
        self.selected().or_else(|| self.preview.actor.filter(|_| self.style == VatsStyle::DeadEye && self.phase == Phase::Marking))
    }

    /// [design, #2659] Classic queue reservations affect only the shown loaded count.
    /// Combat still consumes the actual rounds when firing; playback uses that count.
    pub fn displayed_ammo(&self, ammo: Option<(u32, u32)>) -> Option<(u32, u32)> {
        let reserved = if self.style == VatsStyle::Classic && self.phase == Phase::Marking { self.queue.rounds_needed() } else { 0 };
        ammo.map(|(loaded, spare)| (loaded.saturating_sub(reserved), spare))
    }

    /// The damage in sight on the enemy bar's actor ([`VatsMode::bar_target`]): what the
    /// attacks queued on it would take off its Health if every one lands, and one more on
    /// the selected part (the part under the crosshair in Dead Eye) [design: the fork
    /// owner 2026-11-10, "the healthbar should show how much damage will confirmed hit do
    /// if it hits - multiple shots should add"].
    pub fn expected_damage(&self) -> Option<(u32, f32)> {
        let actor = self.bar_target()?;
        let queued: f32 = self.queue.marks().iter().zip(&self.spots).skip(self.started).filter(|(m, _)| m.target.0 == actor).map(|(_, s)| s.damage).sum();
        let next = self.preview.parts.iter().find(|p| self.preview.actor == Some(actor) && p.under).map_or(0.0, |p| p.damage);
        Some((actor, queued + next))
    }

    /// The target screen has something to show (`crate::tiles::vats_menu` keeps the game's
    /// menu file built meanwhile): the classic style with a target selected, or Dead Eye
    /// while marking, whose parts' boxes are the same element [design: the fork owner
    /// 2026-11-10, "make the body part health HUD element the same in Dead Eye and
    /// vanilla vats"].
    pub fn screen_up(&self) -> bool {
        self.selected().is_some() || (self.style == VatsStyle::DeadEye && self.phase == Phase::Marking)
    }

    /// What the target screen shows now: the classic style's ([`VatsMode::classic_view`]),
    /// or for Dead Eye the previewed actor's parts alone, the one under the crosshair the
    /// selected one.
    pub fn target_screen(&self) -> Option<ClassicView> {
        if self.style == VatsStyle::Classic {
            return self.classic_view();
        }
        if self.phase != Phase::Marking {
            return None;
        }
        let parts = self
            .preview
            .parts
            .iter()
            .enumerate()
            .filter(|(_, p)| p.shown)
            .map(|(index, p)| ClassicPart {
                index,
                name: screen_name(&p.spot.part),
                chance: p.spot.chance,
                condition: p.condition,
                value: p.spot.condition,
                selected: p.under,
                point: p.spot.point,
            })
            .collect();
        Some(ClassicView { bare: true, parts, queue: Vec::new(), special: None })
    }

    /// The special attack of the blow being struck, when it was queued as one
    /// (`crate::combat::melee` multiplies the damage and adds the effect).
    pub fn swing_special(&self) -> Option<SpecialAttack> {
        let i = self.striking.filter(|_| self.phase == Phase::Firing)?;
        self.spots.get(i).filter(|s| s.special).and(self.weapon.as_ref()?.special).map(|(s, _)| s)
    }

    /// The attack camera asked for now, with its serial (`crate::kill_cam` plays each
    /// serial once).
    pub fn camera_ask(&self) -> Option<(u64, CameraAsk)> {
        self.cam_ask.filter(|_| self.phase == Phase::Firing)
    }

    /// The attack a Shoot camera shows has left whole: let go, no round of its burst still
    /// to come, no throw still in the hand. `crate::kill_cam` holds the camera till then.
    pub fn attack_out(&self) -> bool {
        self.phase != Phase::Firing || (self.cam_released && self.throw_held.is_none() && !matches!(&self.run, Some(Run::Volley(vo)) if vo.mid_burst()))
    }

    /// How much faster than the world's clock the player's own attack runs now (the
    /// playing camera shot's Player time multiplier): the marks' pace here and the weapon's
    /// cycle in `crate::combat` go by it, as the player's body does
    /// (`crate::script::PlayerTimeScale`).
    pub fn player_rate(&self) -> f32 {
        if self.player_rate > 0.0 {
            self.player_rate
        } else {
            1.0
        }
    }

    /// Mark `i`'s shot as the scripts see it; before its roll it counts as a hit.
    fn shot_of(&self, i: usize, player: &Player) -> Option<fnv_game::vats::VatsShot> {
        let (m, s) = (self.queue.marks().get(i)?, self.spots.get(i)?);
        Some(fnv_game::vats::VatsShot {
            weapon: self.weapon.as_ref()?.id,
            target: m.target,
            distance: f64::from(crate::combat::distance(player.eye(), s.point)),
            condition: s.condition,
            success: s.landed.unwrap_or(true),
            height: f64::from(s.point[2] - player.feet[2]),
        })
    }

    /// The classic style's selected target (reference), while targets are chosen.
    pub fn selected(&self) -> Option<u32> {
        self.classic.selected.filter(|_| self.style == VatsStyle::Classic && self.phase == Phase::Marking)
    }

    /// The actors marked now (references).
    pub fn marked(&self) -> Vec<u32> {
        self.queue.marks().iter().map(|m| m.target.0).collect()
    }

    /// The shot `crate::combat` is to fire now: the point it goes straight at.
    pub fn take_shot(&mut self) -> Option<[f32; 3]> {
        self.shot.take()
    }

    /// `crate::combat`: the projectiles this frame's marked shots landed on, to end.
    pub fn take_shot_down(&mut self) -> Vec<(u64, FormId)> {
        std::mem::take(&mut self.downed)
    }

    /// `crate::combat`: the throw it was handed has left the hand, or cannot.
    pub fn throw_left(&mut self) {
        self.throw_held = None;
    }

    /// Whether the mode's end waits for an explosive it sent ([`BLAST_SHOWN`],
    /// [`AFTER_BLAST`], [`BLAST_PATIENCE`]): one on its way to its mark, or one gone off a
    /// moment ago. One flying off from its mark is not waited for.
    fn blast_holds(&mut self, blast: crate::combat::VatsBlast) -> bool {
        if blast.went_off != self.blast_seen {
            self.blast_seen = blast.went_off;
            let thrown = self.weapon.as_ref().is_some_and(|w| w.style == AttackStyle::Thrown);
            let stays = if thrown { BLAST_SHOWN + AFTER_BLAST } else { AFTER_LAUNCHED_BLAST };
            self.blast_until = Some(self.now + stays);
            self.blast_since = None;
            info!("vats mode: the explosive went off; the mode stays {stays:.0} s more");
        }
        // The patience is for the explosive last sent, not for a long queue's first: it
        // starts over when none is on its way, when another leaves and when one goes off
        // (the owner's review of #2102: with a queue of grenades the mode ended while
        // the last one still flew).
        if blast.toward == 0 || blast.toward > self.blast_flying {
            self.blast_since = None;
        }
        self.blast_flying = blast.toward;
        if blast.toward > 0 {
            let since = *self.blast_since.get_or_insert(self.now);
            if self.now - since <= BLAST_PATIENCE {
                return true;
            }
            if self.blast_until.is_none() {
                info!("vats mode: the explosive has not gone off in {BLAST_PATIENCE:.0} s; the mode does not wait for it");
                self.blast_until = Some(self.now);
            }
        }
        self.blast_until.is_some_and(|until| self.now < until)
    }

    /// Whether the firing waits for a throw still in the hand, no longer than
    /// [`THROW_PATIENCE`].
    fn throw_holds(&mut self) -> bool {
        match self.throw_held {
            Some(since) if self.now - since > THROW_PATIENCE => {
                info!("vats mode: the throw did not leave the hand in {THROW_PATIENCE:.0} s; the firing goes on");
                self.throw_held = None;
                false
            }
            held => held.is_some(),
        }
    }

    /// A melee mark's swing is due now: `crate::combat::melee` taps Use once (§16.2).
    pub fn take_swing(&mut self) -> bool {
        std::mem::take(&mut self.swing_now)
    }

    /// The melee mark being swung at: its actor, its point now, how close across the
    /// ground it must be for the blow to land on it (the weapon's reach plus
    /// [`CLOSE_SLACK`]) (§16.2), and whether the hit roll lets it land (§17.4).
    pub fn melee_mark(&self) -> Option<(u32, [f32; 3], f32, bool)> {
        if self.phase != Phase::Firing {
            return None;
        }
        let w = self.weapon.as_ref().filter(|w| w.style == AttackStyle::Melee)?;
        let i = self.striking?;
        Some((self.queue.marks().get(i)?.target.0, self.spots.get(i)?.point, w.melee_reach + CLOSE_SLACK, self.blow_lands))
    }

    /// Where the view's depth of field is focused (image-modifiers.md §8.4): the mark
    /// being shot while firing, the last marked while marking; none without a mark.
    pub fn focus(&self) -> Option<[f32; 3]> {
        match self.phase {
            Phase::Firing => self.shooting.and_then(|i| self.spots.get(i)).map(|s| s.point),
            Phase::Marking => self.spots.last().map(|s| s.point),
            _ => None,
        }
    }

    /// The AP the marks not yet started hold.
    pub fn reserved(&self) -> f32 {
        if !self.active() {
            return 0.0;
        }
        self.queue.marks().iter().skip(self.started).map(|m| m.cost).sum()
    }

    /// The clock's speed the mode asks for (`script::time_multiplier` multiplies it in).
    pub fn time_scale(&self) -> f32 {
        match (self.phase, self.entered_at) {
            // The classic style stops (or slows) the world at once (vats-classic.md §2).
            (Phase::Marking, _) if self.style == VatsStyle::Classic => self.classic.scale,
            (Phase::Marking, Some(t)) => time_scale_at((self.now - t) as f32),
            (Phase::Firing, _) => FIRE_SCALE,
            _ => 1.0,
        }
    }

    /// The clock's speed the mode's sounds follow (`audio_out::slowed_sound` of it): the
    /// clock's own, but while targets are chosen no slower than firing's [design, #3784: at
    /// marking's near stop every voice and shot played at the 0.4 floor for as long as the
    /// player took, 17 s in a player's report, and read as broken sound; at
    /// [`FIRE_SCALE`] they stay drawn out and lower, the owner's "like the base game",
    /// and can still be made out].
    pub fn sound_scale(&self) -> f32 {
        let clock = self.time_scale();
        if self.phase == Phase::Marking {
            clock.max(FIRE_SCALE)
        } else {
            clock
        }
    }

    /// The look for the image-space pass (§5): the swipe's edge (a share of the screen
    /// width; graded to its left), the band on the edge, the wash over the screen, the
    /// grade's strength.
    pub fn look(&self) -> [f32; 4] {
        let since = |t: Option<f64>| t.map(|t| (self.now - t) as f32);
        // The classic style has no swipe and no wash: its grade comes in evenly over the
        // whole picture while the view narrows onto the target [design: the fork owner
        // 2026-11-10, "remove the screen's scanning and flash effect when enabling vats -
        // instead the color overlay should smoothly come in as the camera zooms"].
        // Only while targets are chosen: the queued attacks play in the picture's own
        // light.
        if self.style == VatsStyle::Classic {
            return classic_look_at(self.phase == Phase::Marking, since(self.entered_at), since(self.left_at));
        }
        look_at(self.active(), since(self.entered_at), since(self.left_at))
    }

    /// The marks to draw (§8, §17.5): one "x" and one chance for each actor and part with
    /// marks not yet fired, in the order first marked. Marks on the same part share a point,
    /// so drawing each would stack them; the chance shown is the newest mark's, the one
    /// Concentrated Fire has raised the most.
    fn drawn(&self) -> Vec<([f32; 3], f32)> {
        let skip = if self.phase == Phase::Firing { self.started } else { 0 };
        // The classic style shows what is queued on each part's own label while targets
        // are chosen (`draw_previews`).
        // Nor while its attacks play: nothing is drawn over the picture then.
        if self.style == VatsStyle::Classic {
            return Vec::new();
        }
        let mut seen: Vec<(u32, Option<u8>)> = Vec::new();
        let mut out: Vec<([f32; 3], f32)> = Vec::new();
        for (m, s) in self.queue.marks().iter().zip(&self.spots).skip(skip).take(if self.active() { usize::MAX } else { 0 }) {
            let key = (m.target.0, m.part);
            match seen.iter().position(|k| *k == key) {
                Some(i) => out[i].1 = s.chance,
                None => {
                    seen.push(key);
                    out.push((s.point, s.chance));
                }
            }
        }
        out
    }

    /// Roll a hit at `chance` percent (§17.4); the test aid `NVF_PLAY_TEST_VATS_ROLL`
    /// (`hit` or `miss`) decides instead. The roll, 0–100, and whether it hit.
    fn roll(&mut self, chance: f32) -> (f32, bool) {
        let u = self.rng.get_or_insert(SplitMix(0x7A75_4D0D)).next_f32();
        let hit = forced_roll().unwrap_or_else(|| rolled_hit(chance, u));
        if hit {
            self.hits += 1;
            self.stats.hits += 1;
        } else {
            self.misses += 1;
            self.stats.misses += 1;
        }
        (u * 100.0, hit)
    }

    /// Two numbers in [0, 1) for where a miss goes.
    fn two(&mut self) -> (f32, f32) {
        let r = self.rng.get_or_insert(SplitMix(0x7A75_4D0D));
        (r.next_f32(), r.next_f32())
    }

    fn sound(&mut self, sfx: Option<&Sfx>, edid: &str) -> Option<FormId> {
        let sfx = sfx?;
        let by = self.sounds.get_or_insert_with(|| sfx.forms.sounds.iter().filter_map(|(&id, s)| s.edid.clone().map(|e| (e, id))).collect());
        let found = by.get(edid).copied();
        if found.is_none() {
            debug!("vats mode: no sound {edid}");
        }
        found
    }
}

/// The test aid `NVF_PLAY_TEST_VATS_ROLL`: `hit` or `miss` makes every roll come out so
/// (vats-mode.md §13), for checks about something other than the chance.
fn forced_roll() -> Option<bool> {
    static FORCED: std::sync::OnceLock<Option<bool>> = std::sync::OnceLock::new();
    *FORCED.get_or_init(|| match std::env::var("NVF_PLAY_TEST_VATS_ROLL").ok()?.trim() {
        "hit" => Some(true),
        "miss" => Some(false),
        other => {
            warn!("vats mode: NVF_PLAY_TEST_VATS_ROLL={other}: not hit or miss, rolls are rolled");
            None
        }
    })
}

/// A mark's chance to hit now, percent (§17.2): `cone` is the gun's cone in VATS
/// (`Combat::vats_cone`); a melee weapon has none.
fn chance_of(spot: &Spot, w: &InHand, eye: [f32; 3], cone: Option<f32>) -> f32 {
    let distance = crate::combat::distance(eye, spot.point);
    let cone = if w.style == AttackStyle::Melee { None } else { Some(cone.unwrap_or(0.0)) };
    let raw = raw_chance(&HitInputs { base: spot.base, size: spot.size, skill: w.skill, distance, cone_deg: cone }, &w.hit);
    final_chance(raw, |c| spot.perks.apply(c as f64) as f32, spot.perks.concentrated_fire.then_some(spot.earlier), &w.hit) * spot.seen.unwrap_or(1.0)
}

/// Whether the player's V.A.T.S. Throwables Use Visibility tweak scales this mode's
/// chances: on, with a thrown weapon that lands as itself (a knife, a spear: no grenade)
/// [design: docs/design/tweaks.md].
fn throw_sees(v: &VatsMode, combat: &Combat, script: Option<&ScriptState>) -> bool {
    let on = script.and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.vats_throw_visibility);
    on && v.weapon.as_ref().is_some_and(|w| {
        w.style == AttackStyle::Thrown && combat.forms.chain(w.id, None).and_then(|c| c.projectile).and_then(|p| combat.forms.projectiles.get(&p)).is_some_and(|p| p.lands_as_weapon())
    })
}

/// How much of a part at `point` the eye sees: the share of seven points about it (the
/// middle and [`SEEN_SPREAD`] units off it each way) a line reaches [design].
fn part_seen(phys: &mut PhysState, eye: [f32; 3], point: [f32; 3]) -> f32 {
    let filter = shot_filter(phys);
    let s = SEEN_SPREAD;
    let offsets = [[0.0, 0.0, 0.0], [s, 0.0, 0.0], [-s, 0.0, 0.0], [0.0, s, 0.0], [0.0, -s, 0.0], [0.0, 0.0, s], [0.0, 0.0, -s]];
    let seen = offsets.iter().filter(|o| in_sight(phys, eye, [point[0] + o[0], point[1] + o[1], point[2] + o[2]], &filter)).count();
    seen as f32 / offsets.len() as f32
}

/// How far from the middle [`part_seen`] tries [design].
const SEEN_SPREAD: f32 = 8.0;

/// Every mark's chance again (the marks and the player move).
fn rechance(v: &mut VatsMode, combat: &Combat, eye: [f32; 3]) {
    let Some(w) = v.weapon.as_ref() else { return };
    let cone = combat.vats_cone;
    for s in &mut v.spots {
        s.chance = chance_of(s, w, eye, cone);
    }
}

fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// The clock's speed `since` real seconds after entering: from 1 down to [`MARK_SCALE`]
/// over the swipe.
pub fn time_scale_at(since: f32) -> f32 {
    1.0 - (1.0 - MARK_SCALE) * smoothstep(since / ENTER_SWIPE)
}

/// The look ([`VatsMode::look`]) while the mode is on (`on`), `entered` real seconds after
/// entering, or `left` seconds after it went off.
pub fn look_at(on: bool, entered: Option<f32>, left: Option<f32>) -> [f32; 4] {
    match (on, entered, left) {
        (true, Some(t), _) if t < ENTER_SWIPE => {
            // The edge crosses from the left edge until the band behind it has left too.
            let edge = t / ENTER_SWIPE * (1.0 + BAND_WIDTH);
            [edge, BAND_STRENGTH, 0.0, 1.0]
        }
        (true, Some(t), _) => [2.0, 0.0, WASH_START * (1.0 - (t - ENTER_SWIPE) / ENTER_WASH).max(0.0), 1.0],
        (true, None, _) => [2.0, 0.0, 0.0, 1.0],
        (false, _, Some(t)) if t < LEAVE_FADE => [2.0, 0.0, 0.0, 1.0 - t / LEAVE_FADE],
        _ => [0.0; 4],
    }
}

/// The classic style's look ([`VatsMode::look`]): no swipe, band or wash; the grade's
/// strength eased in over the zoom's time (`crate::dialogue_zoom::VATS_IN_SECONDS`) and
/// faded out as Dead Eye's is.
pub fn classic_look_at(on: bool, entered: Option<f32>, left: Option<f32>) -> [f32; 4] {
    match (on, entered, left) {
        (true, Some(t), _) => [2.0, 0.0, 0.0, smoothstep(t / crate::dialogue_zoom::VATS_IN_SECONDS)],
        (true, None, _) => [2.0, 0.0, 0.0, 1.0],
        (false, _, Some(t)) if t < LEAVE_FADE => [2.0, 0.0, 0.0, 1.0 - t / LEAVE_FADE],
        _ => [0.0; 4],
    }
}

/// The band's width behind the edge, a share of the screen width (the shader's
/// `VATS_BAND`) [design].
pub const BAND_WIDTH: f32 = 0.22;

/// The player's Action Points: (current, maximum, damage); `None` without the script
/// runtime. The maximum counts the temporary modifiers, as the HUD's bar does.
fn player_ap(script: Option<&mut ScriptState>, sim: &mut SimState) -> Option<(f32, f32, f32)> {
    let rt = script.and_then(|s| s.rt.as_mut())?;
    with_world(rt, sim, |rt| {
        let [base, modifier, damage] = fnv_game::functions::actor_value_parts(&mut rt.game, FormId::PLAYER_REF, ACTION_POINTS).ok()?;
        let max = base + modifier + fnv_game::functions::actor_value_temporary(&rt.game, FormId::PLAYER_REF, ACTION_POINTS);
        Some(((max - damage) as f32, max as f32, damage as f32))
    })
}

/// The Action Points the player has to mark with: unlimited in god mode, where an attack
/// takes none (`fnv_game::functions::actor::god_spares`) [design: owner decision
/// 2026-11-15, issue #2743]; unlimited too without the script runtime.
fn ap_to_mark(script: Option<&mut ScriptState>, sim: &mut SimState) -> f32 {
    if crate::combat::god_mode() {
        return f32::INFINITY;
    }
    player_ap(script, sim).map_or(f32::INFINITY, |a| a.0)
}

/// The sound side of [`update`].
#[derive(SystemParam)]
struct Sounds<'w> {
    sfx: Option<Res<'w, Sfx>>,
    play: MessageWriter<'w, PlaySound>,
    loops: MessageWriter<'w, LoopSound>,
    /// A helper's shot (§18.2).
    fire: MessageWriter<'w, ActorFire>,
}

impl Sounds<'_> {
    fn play(&mut self, vats: &mut VatsMode, edid: &str) {
        if let Some(sound) = vats.sound(self.sfx.as_deref(), edid) {
            self.play.write(PlaySound { sound, at: None });
        }
    }

    fn looping(&mut self, vats: &mut VatsMode, on: bool) {
        if let Some(sound) = vats.sound(self.sfx.as_deref(), "UIVATSLoop") {
            self.loops.write(LoopSound { sound, on });
        }
    }
}

/// Shift is held (for the Shift Ignores Friendly V.A.T.S. tweak, read by [`barred`])
/// [design: docs/design/tweaks.md].
static SHIFT_HELD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The keys the mode takes (after the frame's input, test aids and script locks), and
/// what it holds back from the rest of the game (§11.5): the trigger, reloading and Ammo
/// Swap while it is on. Aiming down the sights is the player's while marking (a tap of
/// Block unmarks instead) and, in first person, while firing; in third person the
/// marked shots raise a gun's sights by the mark's distance. V with the weapon away
/// draws it.
#[allow(clippy::too_many_arguments)]
fn read_keys(
    input: ControlInput,
    real: Res<Time<Real>>,
    state: Res<State<PlayState>>,
    locks: Res<ControlLocks>,
    mut vats: ResMut<VatsMode>,
    mut control: ResMut<PlayerControl>,
    (player, mut intent): (Res<Player>, ResMut<MoveIntent>),
    mut sounds: Sounds,
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
    mut tiles: Option<ResMut<crate::tiles::vats_menu::VatsTiles>>,
    menu_key: Option<ResMut<crate::flashlight::MenuKeyHold>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
) {
    let playing = *state.get() == PlayState::Playing && !input.suppressed();
    SHIFT_HELD.store(input.keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]), std::sync::atomic::Ordering::Relaxed);
    if vats.phase == Phase::Off {
        vats.aimed_before = control.aiming;
    }
    // The player's tweak: a weapon the mode drew goes away again when nothing was fired
    // [design: docs/design/tweaks.md].
    if std::mem::take(&mut vats.put_away) && tweaks.as_ref().is_some_and(|t| t.vats_keeps_holstered) && vats.phase == Phase::Off {
        control.weapon_out = false;
        info!("vats mode: the weapon it drew is put away again, nothing was fired (tweak)");
    }
    // The player's tweak: the firing in first person, the view given back after [design: docs/design/tweaks.md].
    let first_person = tweaks.as_ref().is_some_and(|t| t.first_person_vats);
    if first_person && vats.phase == Phase::Firing && vats.pov_before.is_none() {
        vats.pov_before = Some(control.pov);
        control.pov = crate::control::Pov::First;
    }
    if vats.phase == Phase::Off {
        if let Some(p) = vats.pov_before.take() {
            control.pov = p;
        }
    }
    // The player's tweak: entered not aiming, every shot from the hip [design: docs/design/tweaks.md].
    let hip_fire = tweaks.as_ref().is_some_and(|t| t.vats_hip_fire) && !vats.aimed_before;
    let pressed = |c: Control| playing && input.just_pressed(c);
    let now = real.elapsed_secs_f64();
    vats.keys = Keys { toggle: pressed(Control::Vats), mark: pressed(Control::Use), unmark: false, accept: pressed(Control::Activate), target_step: 0, part_step: 0, special: false, hover_part: None, pointer_moved: false, ray: None, near_part: None };
    vats.pad = input.controller();
    // The controller's Return (B) on the classic screen, and a click on the screen's
    // Return button, read below.
    let mut pad_return = false;
    let mut clicked_return = false;
    // The classic style (vats-classic.md §4, §5): the movement controls choose, left and
    // right the target, forward and back the part; the player stands still.
    if vats.holds_view() {
        if vats.phase == Phase::Marking && vats.pad {
            // A controller: the menu file's own buttons [documented: vats_menu.xml,
            // `_xbox_button`: A accepts, B returns, Y is the special attack, the right
            // trigger selects, the left stick is Body Part and the right stick Target],
            // whatever the game's controls are bound to.
            let button = |b: GamepadButton| playing && input.pads.iter().any(|g| g.just_pressed(b));
            vats.keys.accept = button(GamepadButton::South);
            vats.keys.mark = button(GamepadButton::RightTrigger2);
            vats.keys.special = button(GamepadButton::North);
            pad_return = button(GamepadButton::East);
            // The right stick, pushed left or right: the target before or after, once a push.
            // Retain physical history; filter only the action so a switching hold
            // cannot become a delayed target step [design, #2200; pad::update_stick_buttons].
            let direction = |x: f32| if x >= crate::pad::STICK_PRESS { 1 } else if x <= -crate::pad::STICK_PRESS { -1 } else { 0 };
            let way = direction(if playing { input.pads.physical_stick(false).x } else { 0.0 });
            if playing && way != vats.stick_was {
                vats.keys.target_step = direction(crate::pad::stick(&input.pads, false).x);
            }
            vats.stick_was = way;
            // The left stick, pushed any way: the part that lies that way on the screen
            // from the selected one [design: the file names the stick, not the rule]; where
            // none does, the part before (up, left) or after (down, right).
            use crate::pad::StickDir;
            let pushed = input.stick_buttons().as_ref().filter(|_| playing).and_then(|s| StickDir::ALL.into_iter().find(|d| s.just_pressed(*d)));
            if let Some(d) = pushed {
                let (dir, step) = match d {
                    StickDir::Up => ((0.0, -1.0), -1),
                    StickDir::Down => ((0.0, 1.0), 1),
                    StickDir::Left => ((-1.0, 0.0), -1),
                    StickDir::Right => ((1.0, 0.0), 1),
                };
                match tiles.as_deref().filter(|t| t.drawn).and_then(|t| t.toward(dir)) {
                    Some(i) => vats.keys.hover_part = Some(i),
                    None => vats.keys.part_step = step,
                }
            }
        } else if vats.phase == Phase::Marking {
            vats.keys.target_step = i32::from(pressed(Control::SlideRight)) - i32::from(pressed(Control::SlideLeft));
            vats.keys.part_step = i32::from(pressed(Control::Back)) - i32::from(pressed(Control::Forward));
            // The control R is by default queues the weapon's special attack [documented:
            // vats_menu.xml binds the key R to its special attack button; design: the
            // control and not the key, so a rebound layout keeps one key for it;
            // vats-classic.md §9].
            vats.keys.special = pressed(Control::ReadyItem);
            // The pointer on the target screen (vats-classic.md §11): coming onto a part
            // selects it, and a click on one of the file's arrows or buttons does what
            // that stands for instead of queueing an attack (`vats_menu::click`).
            if let Some(t) = tiles.as_deref_mut().filter(|t| t.drawn) {
                // The test aid's pointer, which may click as it arrives.
                if t.aid_fire(now as f32) {
                    vats.keys.mark = true;
                }
                vats.keys.pointer_moved = t.hover_moved;
                vats.keys.hover_part = t.hover.filter(|_| t.hover_moved);
                vats.keys.near_part = t.near.filter(|_| t.hover_moved);
                vats.keys.ray = t.ray.filter(|_| t.hover_moved);
                if vats.keys.mark {
                    use crate::tiles::vats_menu::Click;
                    // Where the pointer is at the click, against the boxes as last drawn:
                    // the hover (`t.arrow`) is of the frame before, and a pointer that
                    // came onto an arrow and clicked at once queued an attack (#2657).
                    let pointer = t.aid_pointer.or_else(|| windows.single().ok().and_then(|w| w.cursor_position()).map(|p| (p.x, p.y)));
                    let (arrow, button) = t.clicked(pointer);
                    let what = crate::tiles::vats_menu::click(arrow, button, !vats.queue.is_empty());
                    info!("vats mode: classic: a click on the target screen: {what:?} (the hover of the frame before: arrow {:?})", t.arrow);
                    vats.keys.mark = what == Click::Queue;
                    match what {
                        Click::Target(step) => vats.keys.target_step = step,
                        Click::Accept => vats.keys.accept = true,
                        Click::Return => clicked_return = true,
                        Click::Special => vats.keys.special = true,
                        Click::Queue | Click::Nothing => {}
                    }
                }
            }
        }
        let mut i = *intent;
        (i.forward, i.right, i.jump) = (0.0, 0.0, false);
        intent.set_if_neq(i);
    }
    // Any other action ends the mode (§11.8), here before activation, so an E with
    // nothing marked reaches what is under the crosshair.
    // (While the classic style chooses targets, Ready / Reload is its special attack's.)
    let classic_marking = vats.phase == Phase::Marking && vats.style == VatsStyle::Classic;
    // With a controller none of the game's controls leaves the classic screen: its
    // buttons are the screen's own, and B returns.
    let pad_screen = classic_marking && vats.pad;
    // A Menu Mode press made in the mode is the mode's own: it leaves (below), or on the
    // controller's classic screen, where B is both Menu Mode and the screen's Return, it
    // returns. Used up, so its release opens no Pip-Boy and its hold switches no
    // flashlight (`crate::flashlight`) [design, #2311: B, a controller's back button, left
    // the mode and opened the Pip-Boy on its release; Tab the same].
    if vats.phase != Phase::Off && pressed(Control::MenuMode) {
        if let Some(mut hold) = menu_key {
            hold.consume();
        }
    }
    if let Some(c) = leaving_press(vats.phase, vats.queue.len(), |c| pressed(c) && !pad_screen && !(classic_marking && c == Control::ReadyItem)) {
        vats.now = now;
        vats.keys = Keys::default();
        end(&mut vats, &format!("{} pressed", crate::bindings::label(c)), &mut sounds);
    }
    // Block while marking: a tap unmarks, a hold aims (the aim waits out the tap). The
    // classic style has no aiming: the press takes the last attack back at once.
    if vats.phase == Phase::Marking && vats.style == VatsStyle::Classic {
        vats.keys.unmark = if vats.pad { pad_return } else { pressed(Control::Block) || clicked_return };
        vats.block_at = None;
        control.aiming = false;
    } else if vats.phase == Phase::Marking {
        if pressed(Control::Block) {
            vats.block_at = Some(now);
        }
        let held_for = vats.block_at.map(|t| (now - t) as f32);
        let tap = held_for.is_some_and(|h| h < UNMARK_TAP);
        if tap && playing && input.just_released(Control::Block) {
            vats.keys.unmark = true;
        }
        if !(playing && input.pressed(Control::Block)) {
            vats.block_at = None;
        }
        if tap {
            control.aiming = false;
        }
    } else {
        vats.block_at = None;
    }
    if vats.phase == Phase::Off && vats.keys.toggle && !control.weapon_out && !locks.fighting {
        control.weapon_out = true;
        vats.drew_weapon = true;
        vats.phase = Phase::Drawing;
        vats.drawing_at = Some(now);
        vats.keys.toggle = false;
        info!("vats mode: drawing the weapon first");
    }
    if vats.holds_input() {
        control.trigger = false;
        control.reload = false;
        control.ammo_swap = false;
    }
    match vats.phase {
        Phase::Drawing => control.aiming = false,
        Phase::Firing => {
            let gun = vats.weapon.as_ref().filter(|w| matches!(w.style, AttackStyle::Single | AttackStyle::Burst { .. }));
            match gun.map(|w| w.sight_fov) {
                Some(_) if hip_fire => {
                    if vats.sights_up != Some(false) {
                        vats.sights_up = Some(false);
                        info!("vats mode: fires from the hip (entered not aiming, tweak)");
                    }
                    control.aiming = false;
                }
                // The body's clips show it: a gun's sights up for a far mark, from the hip
                // for a near one, as the NPCs fire (`crate::combat::aims_down_sights`).
                Some(fov) if control.third_person_view() => {
                    let away = vats.spots.get(vats.aim_mark).map(|s| crate::combat::distance(player.eye(), s.point));
                    let up = away.is_some_and(|d| crate::combat::aims_down_sights(vats.sights_up == Some(true), d, fov));
                    if vats.sights_up != Some(up) {
                        vats.sights_up = Some(up);
                        let how = if up { "aims down the sights (beyond 20 m)" } else { "fires from the hip (within 20 m)" };
                        info!("vats mode: third person, mark {} {:.0} units away: {how}", vats.aim_mark + 1, away.unwrap_or(0.0));
                    }
                    control.aiming = up;
                }
                // First person: the player's own aim (a scope stays usable).
                Some(_) => {}
                None => control.aiming = false,
            }
        }
        _ => {}
    }
}

/// What [`update`] reads for the classic style: the two settings and the cameras' state.
#[derive(SystemParam)]
struct ClassicIn<'w> {
    style: Res<'w, VatsStyle>,
    slowdown: Res<'w, VatsSlowdown>,
    kill_cam: Option<Res<'w, crate::kill_cam::KillCam>>,
}

/// The combat state `update` reads: the projectiles and shots, the weapon's clip, the
/// actors' parts and the mines.
type CombatIn<'w> = (Option<Res<'w, Combat>>, Res<'w, WeaponAnim>, Res<'w, ActorParts>, Option<Res<'w, crate::mines::Mines>>);

/// The mode's work for the frame; before `crate::combat::fire`, which fires the shot asked
/// for ([`VatsMode::take_shot`]).
#[allow(clippy::too_many_arguments)]
fn update(
    (real, time): (Res<Time<Real>>, Res<Time>),
    (state, settings): (Res<State<PlayState>>, ClassicIn),
    mut vats: ResMut<VatsMode>,
    (combat, anim, parts, mines): CombatIn,
    mut sim: ResMut<SimState>,
    mut script: Option<ResMut<ScriptState>>,
    mut phys: ResMut<PhysState>,
    mut player: ResMut<Player>,
    (control, stance): (Res<PlayerControl>, Option<Res<crate::stance::StanceMotion>>),
    locks: Res<ControlLocks>,
    mut hud: ResMut<HudMessage>,
    mut sounds: Sounds,
    mut touch: touch::Consumer,
) {
    let ClassicIn { style, slowdown, kill_cam } = settings;
    let v = &mut *vats;
    v.now = real.elapsed_secs_f64();
    v.player_rate = kill_cam.as_deref().and_then(|k| k.player_scale()).unwrap_or(1.0);
    v.shot = None;
    let mut keys = std::mem::take(&mut v.keys);
    let playing = *state.get() == PlayState::Playing;
    let dead = script.as_deref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.is_dead(FormId::PLAYER_REF));
    // A conversation (a forced greeting too) or a script's menu, caught before the state
    // follows it (§11.8).
    let held = script.as_deref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.in_menu());
    let Some(combat) = combat else { return };
    // Left without firing, the classic style gives the view back (vats-classic.md §5).
    // It turns back as it turned onto the target, in real time, while the field of view
    // widens again (`crate::dialogue_zoom`), instead of jumping there [design: the fork
    // owner's playtest, 2026-11-10]; the mouse has the view again once it is there.
    // Not behind a loading screen nor after one (a load, a door): the view there is the
    // game's, and the mouse and the movement are not held for a turn back to a heading
    // of another place ([`restore_stands`]).
    if v.classic.restore.is_some() && !restore_stands(*state.get() == PlayState::Loading, player.feet, v.classic.entry_feet) {
        info!("vats mode: the view is not turned back (a load or a move meanwhile)");
        v.classic.restore = None;
    }
    if let Some((heading, pitch)) = v.classic.restore {
        let dt = real.delta_secs();
        player.heading = classic::ease_angle(player.heading, heading, classic::TURN_RATE, dt);
        player.pitch = classic::ease_angle(player.pitch, pitch, classic::TURN_RATE, dt);
        let off = |a: f32, b: f32| (a - b + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        if v.phase != Phase::Off || (off(player.heading, heading).abs() < RESTORE_WITHIN && off(player.pitch, pitch).abs() < RESTORE_WITHIN) {
            if v.phase == Phase::Off {
                (player.heading, player.pitch) = (heading, pitch);
            }
            v.classic.restore = None;
        }
    }
    // What ends the mode whatever it is doing (§9.3, §11.6, §11.8).
    let stop = if !playing {
        Some("a menu opened")
    } else if dead {
        Some("the player died")
    } else if held {
        Some("a conversation or a script's menu began")
    } else if v.phase == Phase::Drawing && !control.weapon_out {
        Some("the weapon was put away")
    } else if v.phase == Phase::Drawing && v.drawing_at.is_some_and(|t| v.now - t > f64::from(DRAW_LIMIT)) {
        Some("the weapon did not come out")
    } else if locks.fighting {
        Some("a script disabled fighting")
    } else if v.active()
        && !v.run.as_ref().is_some_and(Run::all_fired)
        && v.weapon.as_ref().map(|w| w.id) != combat.player_weapon().filter(|_| control.weapon_out)
    {
        // (The last throw spends the last one carried: then the weapon goes, §16.3.)
        Some("the weapon left the hand")
    } else {
        None
    };
    match (v.phase, stop) {
        (Phase::Off, _) => {
            // Not while sprinting: a sprint's AP do not come back during it (`crate::sprint`).
            let sprinting = stance.as_deref().is_some_and(|s| s.sprinting);
            if let Some(m) = regenerate(script.as_deref_mut(), &mut sim, combat.rules.ap_restore_rate, if sprinting { 0.0 } else { time.delta_secs() }) {
                if v.regen_mult != Some(m) && (v.regen_mult.is_some() || m != 1.0) {
                    info!("vats mode: AP regenerates at {m:.2} x (the Action Point Regen perks)");
                }
                v.regen_mult = Some(m);
            }
            if keys.toggle {
                (v.style, v.classic.scale) = (*style, classic::clock_scale(slowdown.0));
                try_enter(v, &combat, script.as_deref_mut(), &mut sim, phys.world.params.gravity, playing && !dead && !held && !locks.fighting, &mut hud, &mut sounds);
                classic_enter(v, &combat, script.as_deref_mut(), &mut sim, &mut phys, &player, &mut sounds);
            }
        }
        (_, Some(why)) => end(v, why, &mut sounds),
        (Phase::Drawing, None) => {
            if keys.toggle {
                v.phase = Phase::Off;
                info!("vats mode: entering cancelled");
            } else if anim.ready && control.weapon_out {
                v.phase = Phase::Off;
                (v.style, v.classic.scale) = (*style, classic::clock_scale(slowdown.0));
                try_enter(v, &combat, script.as_deref_mut(), &mut sim, phys.world.params.gravity, true, &mut hud, &mut sounds);
                classic_enter(v, &combat, script.as_deref_mut(), &mut sim, &mut phys, &player, &mut sounds);
            }
        }
        (Phase::Marking, None) => {
            follow(v, &phys, &combat);
            rechance(v, &combat, player.eye());
            // A finger request is consumed here, not by a separate effect writer.
            // Preview and mark_actor retain the same actor/part/AP legality rules.
            let direct = touch.take(v, &mut phys, &keys);
            let direct_part = direct.selection.filter(|s| sim.present(s.actor) && barred(v, script.as_deref_mut(), &mut sim, s.actor).is_ok());
            if direct.selection.is_some() && direct_part.is_none() { touch.refuse(); }
            if let Some(s) = direct_part {
                if v.style == VatsStyle::Classic && v.classic.selected != Some(s.actor) {
                    v.classic.selected = Some(s.actor);
                    v.classic.part_set = None;
                }
                (keys.hover_part, keys.near_part, keys.ray) = (None, None, None);
            }
            if direct.fire { keys.accept = true; }
            let chosen = if v.style == VatsStyle::Classic {
                classic_frame(v, &keys, &combat, script.as_deref_mut(), &mut sim, &mut phys, &mut player, &control, real.delta_secs(), !direct.hold_camera || direct_part.is_none(), &mut sounds)
            } else {
                // A finger selects a real ray-hit actor for this preview only.
                // Mark and Fire has no Classic target list or camera follow.
                direct_part.map(|s| (s.actor, 0))
            };
            // (The classic style has ended the mode when no target is left.)
            let on = v.phase == Phase::Marking;
            if on {
                preview(v, &combat, &parts, script.as_deref_mut(), &mut sim, &mut phys, &player, &control, chosen);
            }
            let direct_part = direct_part.filter(|s| (v.style == VatsStyle::DeadEye || v.selected() == Some(s.actor)) && v.preview.actor == Some(s.actor));
            let direct_part = direct_part.and_then(|s| {
                let i = v.preview.parts.iter().position(|p| p.part == s.part)?;
                if v.style == VatsStyle::Classic {
                    v.classic.part = i;
                    v.classic.part_set = Some(s.actor);
                }
                for (n, p) in v.preview.parts.iter_mut().enumerate() { p.under = n == i; }
                Some(s)
            });
            if direct.selection.is_some() && direct_part.is_none() { touch.refuse(); }
            if !on {
            } else if let Some(s) = direct_part.filter(|s| s.queue) {
                // Selection never accepts. In enhanced mode only touch Fire requests
                // accept, through the existing branch below on a later contact.
                let point = point_of(&phys, s.actor, s.part).unwrap_or(player.eye());
                let before = v.queue.len();
                mark_actor(v, &combat, &parts, script.as_deref_mut(), &mut sim, &mut phys, &player, &mut hud, &mut sounds, s.actor, s.part, point, false);
                touch.queued(v.queue.len() > before);
            } else if keys.toggle {
                end(v, "V pressed again", &mut sounds);
            } else if keys.unmark {
                match v.queue.pop() {
                    Some(m) => {
                        let s = v.spots.pop();
                        info!("vats mode: unmarked {} ({}), {} marks left", m.target, s.map_or_else(String::new, |s| s.part), v.queue.len());
                        sounds.play(v, "UIVATSMove");
                    }
                    None => end(v, "right click with no marks", &mut sounds),
                }
            } else if keys.accept {
                // (With nothing marked, E has already ended the mode in `read_keys`.)
                if !v.queue.is_empty() {
                    v.phase = Phase::Firing;
                    (v.aim_mark, v.sights_up) = (0, None);
                    (v.blast_seen, v.blast_until, v.blast_since, v.blast_flying) = (combat.vats_blast.went_off, None, None, 0);
                    v.kill_end = None;
                    let n = v.queue.len();
                    let style = v.weapon.as_ref().map_or(AttackStyle::Single, |w| w.style);
                    let (run, what) = match style {
                        AttackStyle::Single => (Run::Volley(Volley::new(n, SHOT_INTERVAL, AFTER_LAST)), "shots"),
                        AttackStyle::Burst { shots, interval } => (Run::Volley(Volley::bursts(n, shots, SHOT_INTERVAL, interval, AFTER_LAST)), "bursts"),
                        AttackStyle::Thrown => (Run::Volley(Volley::new(n, THROW_INTERVAL, AFTER_LAST)), "throws"),
                        AttackStyle::Melee => (Run::Melee(MeleeRun::new(n, MeleeLimits { approach: APPROACH_LIMIT, swing: SWING_LIMIT, after: AFTER_LAST })), "swings"),
                    };
                    v.run = Some(run);
                    v.kills_at_fire = script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| rt.game.player_kill_count());
                    sounds.looping(v, false);
                    sounds.play(v, "UIVATSReady");
                    info!("vats mode: firing {n} marked {what}");
                }
            } else if keys.special && v.style == VatsStyle::Classic && v.weapon.as_ref().is_none_or(|w| w.special.is_none()) {
                info!("vats mode: classic: this weapon has no special attack the player can use");
                sounds.play(v, "UIVATSInsufficientAP");
            } else if (keys.mark || keys.special) && v.style == VatsStyle::Classic {
                // The selected target's selected part (vats-classic.md §4), with the
                // weapon's attack or its special attack (§9).
                let on = v.classic.selected.zip(v.preview.parts.get(v.classic.part).map(|p| (p.part, p.spot.point)));
                match on {
                    Some((actor, (part, point))) => mark_actor(v, &combat, &parts, script.as_deref_mut(), &mut sim, &mut phys, &player, &mut hud, &mut sounds, actor, part, point, keys.special),
                    None => info!("vats mode: nothing selected to queue an attack on"),
                }
            } else if keys.mark {
                mark(v, &combat, &parts, mines.as_deref(), script.as_deref_mut(), &mut sim, &mut phys, &player, &control, &mut hud, &mut sounds);
            }
        }
        (Phase::Firing, None) => {
            follow(v, &phys, &combat);
            rechance(v, &combat, player.eye());
            let dt = time.delta_secs();
            // An attack whose target has died is not played ([`AFTER_KILL`]).
            drop_dead_marks(v, script.as_deref(), &parts);
            // A throw is in the hand until its clip's `Hit` key: the next waits for it.
            let held = v.throw_holds();
            // An explosive the marks sent holds the mode's end, not the next mark.
            let blast = v.blast_holds(combat.vats_blast) && matches!(&v.run, Some(Run::Volley(vo)) if vo.all_fired());
            // The classic style's cameras hold the next attack while they play (§8 there).
            let wait = held || classic_cameras(v, kill_cam.as_deref(), &player) || blast;
            // The marks' pace is the player's own (a camera shot's Player time multiplier).
            let pace = dt * v.player_rate();
            if let Some(since) = v.kill_end {
                // Nothing is left to play: the cameras go on, the mode ends in a moment.
                if v.now - since >= AFTER_KILL {
                    end(v, "the target died", &mut sounds);
                }
            } else {
                match v.run.as_mut() {
                    Some(Run::Volley(vo)) => match (!wait).then_some(vo).and_then(|vo| vo.step(pace)) {
                        // The player's tweak: the rest of a burst is held once its target is
                        // dead [design: docs/design/tweaks.md].
                        Some(VolleyEvent::Shot { mark, round })
                            if round > 0
                                && script.as_deref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.vats_burst_stops_on_kill)
                                && v.queue.marks().get(mark).is_some_and(|m| crate::combat::is_dead(script.as_deref(), m.target.0)) =>
                        {
                            debug!("vats mode: round {} of mark {} held, the target is dead (tweak)", round + 1, mark + 1);
                        }
                        Some(VolleyEvent::Shot { mark, round }) => {
                            shoot(v, mark, round, script.as_deref_mut(), &mut sim, &mut phys, &mut player, &control, &mut sounds);
                        }
                        Some(VolleyEvent::Done) => end(v, "all shots fired", &mut sounds),
                        None => {}
                    },
                    Some(Run::Melee(_)) => melee_step(v, dt, &combat, script.as_deref_mut(), &mut sim, &mut phys, &mut player, &control, &mut sounds),
                    None => end(v, "nothing to fire", &mut sounds),
                }
            }
        }
    }
    tick_helper(v, time.delta_secs(), script.as_deref_mut(), &mut sim, &phys, &mut sounds);
    if let Some(before) = v.reward_due.take() {
        kill_reward(v, before, script.as_deref_mut(), &mut sim);
    }
    tell_scripts(v, script.as_deref_mut(), &player, combat.rules.vats_classic.player_damage_mult);
}

/// Spent AP comes back outside the mode (§7.2), faster with the Action Point Regen perks
/// (Nerves of Steel, §18.4). The factor used, when any AP was spent.
fn regenerate(script: Option<&mut ScriptState>, sim: &mut SimState, rate: f32, dt: f32) -> Option<f64> {
    let rt = script.and_then(|s| s.rt.as_mut())?;
    if dt <= 0.0 {
        return None;
    }
    with_world(rt, sim, |rt| {
        let g = &mut rt.game;
        let [base, modifier, damage] = fnv_game::functions::actor_value_parts(g, FormId::PLAYER_REF, ACTION_POINTS).ok()?;
        if damage <= 0.0 {
            return None;
        }
        // The player's tweak slows the return while over-encumbered [design: docs/design/tweaks.md].
        let over = g.world.encumbrance(FormId::PLAYER_REF).is_ok_and(|e| e.over);
        let mult = g.ap_regen_mult() * if over { g.tweaks.encumbered_ap_regen } else { 1.0 };
        let max = base + modifier + fnv_game::functions::actor_value_temporary(g, FormId::PLAYER_REF, ACTION_POINTS);
        let left = restored_damage(damage as f32, max as f32, rate * mult as f32, dt) as f64;
        fnv_game::functions::restore_actor_value_damage(g, FormId::PLAYER_REF, ACTION_POINTS, damage - left);
        Some(mult)
    })
}

/// V: start marking if the weapon and the AP allow (§3).
#[allow(clippy::too_many_arguments)]
fn try_enter(v: &mut VatsMode, combat: &Combat, mut script: Option<&mut ScriptState>, sim: &mut SimState, gravity: f32, allowed: bool, hud: &mut HudMessage, sounds: &mut Sounds) {
    let ap = player_ap(script.as_deref_mut(), sim);
    let refused = |v: &mut VatsMode, why: &str, hud: &mut HudMessage, sounds: &mut Sounds| {
        info!("vats mode: refused: {why}");
        hud.show(why.to_string());
        v.stats.refused += 1;
        sounds.play(v, "UIVATSEnterFail");
    };
    if !allowed {
        return refused(v, "Fighting is not possible now.", hud, sounds);
    }
    let mut w = match combat.vats_weapon(sim, gravity) {
        Ok(w) => w,
        Err(why) => return refused(v, why, hud, sounds),
    };
    // The Classic style lists melee and unarmed targets within the original's reach
    // (`VatsWeapon::classic_reach`, #2656); Dead Eye keeps `MELEE_MARK_REACH`.
    if let (VatsStyle::Classic, Some(reach)) = (v.style, w.classic_reach) {
        info!("vats mode: classic melee reach {reach:.0} units from the eye (swing {:.0}, in place of {:.0})", w.melee_reach, w.reach);
        w.reach = reach;
    }
    let base_cost = w.cost;
    // The Action Point Cost perks (docs/spec-notes/perk-entry-points.md §2).
    let (cost, by) = crate::combat::player_weapon_perk(script.as_deref_mut(), sim, fnv_game::perks::ACTION_POINT_COST, w.id, w.cost);
    if !by.is_empty() {
        info!("vats mode: perks change the AP cost {:.1} -> {:.1} ({})", w.cost, cost.max(0.0), by.join(", "));
        w.cost = cost.max(0.0);
    }
    let god = crate::combat::god_mode();
    let ap_now = if god { f32::INFINITY } else { ap.map_or(f32::INFINITY, |a| a.0) };
    if god {
        info!("vats mode: god mode: Action Points are unlimited, an attack takes none ({:.0} now)", ap.map_or(0.0, |a| a.0));
    }
    v.queue = MarkQueue::new(w.cost, w.rounds_per_shot, w.rounds);
    // Not even one mark: no AP for it, or (a burst) not a whole burst loaded.
    if let Err(r) = v.queue.check(ap_now) {
        return refused(v, r.message(), hud, sounds);
    }
    // The player's tweak: entering costs Action Points, and is refused without them [design: docs/design/tweaks.md].
    let entry = script.as_ref().and_then(|s| s.rt.as_ref()).map_or(0.0, |rt| rt.game.tweaks.vats_entry_ap);
    if entry > 0.0 && !crate::combat::god_mode() {
        if ap_now < entry as f32 {
            return refused(v, "Not enough Action Points to enter V.A.T.S.", hud, sounds);
        }
        if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
            crate::script::with_world(rt, sim, |rt| fnv_game::functions::add_actor_value_damage(&mut rt.game, FormId::PLAYER_REF, fnv_game::actor_values::ACTION_POINTS, entry));
            info!("vats mode: entering cost {entry:.0} AP (tweak)");
        }
    }
    v.phase = Phase::Marking;
    v.spots.clear();
    v.preview = Preview::default();
    v.run = None;
    v.fired = 0;
    v.started = 0;
    v.spent = 0.0;
    v.entered_at = Some(v.now);
    v.left_at = None;
    v.stats.entered += 1;
    let ap_text = ap.map_or_else(|| "unlimited".to_string(), |a| format!("{:.0}", a.0));
    let most = v.queue.most(ap_now);
    // A single shot's line is as it was; the other kinds name themselves (§16).
    match (w.style, w.style.label()) {
        (_, None) => info!("vats mode: on ({}, {:.0} AP a shot, {} rounds loaded, {ap_text} AP; at most {most} marks)", w.edid, w.cost, w.rounds),
        (AttackStyle::Melee, Some(kind)) => info!("vats mode: on ({}, {kind}, {:.0} AP a mark, reach {:.0}, {ap_text} AP; at most {most} marks)", w.edid, w.cost, w.melee_reach),
        (AttackStyle::Thrown, Some(kind)) => info!("vats mode: on ({}, {kind}, {:.0} AP a mark, {} carried, {ap_text} AP; at most {most} marks)", w.edid, w.cost, w.rounds),
        (_, Some(kind)) => info!("vats mode: on ({}, {kind}, {:.0} AP a mark, {} rounds loaded, {ap_text} AP; at most {most} marks)", w.edid, w.cost, w.rounds),
    }
    let sight_fov = combat.sight_fov().filter(|&(id, _)| id == w.id).map_or(0.0, |(_, f)| f);
    let skill = combat.player_weapon_skill(script, sim);
    // A melee weapon's special attack, in the classic style and with the skill for it.
    let special = combat.vats_special(w.id).filter(|_| w.style == AttackStyle::Melee && v.style == VatsStyle::Classic);
    let allowed = special.filter(|s| s.allowed(skill)).map(|s| (s, s.cost(base_cost, w.cost)));
    match (special, allowed) {
        (Some(_), Some((s, cost))) => info!("vats mode: {} has a special attack: damage x{:.2}, {cost:.0} AP (skill {skill:.0} of {:.0} needed)", w.edid, s.dam_mult, s.skill),
        (Some(s), None) => info!("vats mode: {} has a special attack the player cannot use yet: skill {skill:.0} of {:.0} needed", w.edid, s.skill),
        _ => {}
    }
    v.weapon = Some(InHand { id: w.id, reach: w.reach, style: w.style, melee_reach: w.melee_reach, sight_fov, skill, hit: combat.rules.vats_hit, special: allowed });
    (v.hits, v.misses) = (0, 0);
    // The classic style is entered only with a target in sight: `classic_enter` plays the
    // sound then, or the refusal's alone.
    if v.style != VatsStyle::Classic {
        sounds.play(v, "UIVATSEnter");
    }
    sounds.looping(v, true);
}

/// Leave the mode (§2): marks dropped, reservations freed, the look fading out.
fn end(v: &mut VatsMode, why: &str, sounds: &mut Sounds) {
    let was = v.phase;
    v.phase = Phase::Off;
    v.put_away = std::mem::take(&mut v.drew_weapon) && v.fired == 0;
    if was == Phase::Firing {
        v.reward_due = v.kills_at_fire.take();
    }
    v.run = None;
    v.shot = None;
    v.kill_end = None;
    v.throw_held = None;
    v.shooting = None;
    v.swing_now = false;
    v.striking = None;
    v.closing = None;
    if matches!(was, Phase::Marking | Phase::Firing) {
        if was == Phase::Marking {
            sounds.looping(v, false);
        }
        sounds.play(v, "UIVATSExit");
        v.left_at = Some(v.now);
        info!("vats mode: off ({} shots fired, {:.0} AP spent; {why})", v.fired, v.spent);
        if v.hits + v.misses > 0 {
            info!("vats mode: rolls: {} hit, {} missed", v.hits, v.misses);
        }
    } else if was == Phase::Drawing {
        info!("vats mode: entering cancelled ({why})");
    }
    v.entered_at = None;
    v.queue = MarkQueue::default();
    v.spots.clear();
    v.preview = Preview::default();
    v.blow_lands = false;
    (v.cam_ask, v.cam_wait, v.cam_led, v.cam_off, v.cam_released) = (None, None, None, false, false);
    // Left without firing, the classic style's view goes back to where it was.
    let restore = v.classic.entry_view.filter(|_| was == Phase::Marking && v.style == VatsStyle::Classic);
    v.classic = ClassicState { scale: v.classic.scale, restore, entry_feet: v.classic.entry_feet, ..ClassicState::default() };
}

/// The words for a weapon's special attack on the target screen [design: NVForever's own;
/// the moves' names are not in the data files].
pub fn special_text(dam_mult: f32, cost: f32) -> String {
    format!("special attack: damage x{dam_mult:.1}, {cost:.0} AP")
}

/// The classic style's attack cameras, one frame (vats-classic.md §8). Whether the
/// firing waits this frame.
///
/// For each mark: its Shoot camera is asked of `crate::kill_cam`; [`SHOOT_LEAD`] into it
/// the attack is let go, and plays in it (a burst's rounds, a throw's wind-up:
/// `kill_cam` holds the camera until [`VatsMode::attack_out`] and a moment more); when
/// that camera is done, but for the last mark, its Hit camera is asked for
/// ([`classic_hit_camera`]) and waited for; then the next mark. [documented: GECK wiki
/// CameraShot, Camera Action: "when the camera should be activated, whether it be when
/// the command starts, when the projectile starts to fly, or when the target takes the
/// hit"; observed: the fork owner's playtest 2026-11-10, with the attack let go only
/// after its Shoot camera the camera showed the player standing and the shot was seen
/// from the eye.]
fn classic_cameras(v: &mut VatsMode, cam: Option<&crate::kill_cam::KillCam>, player: &Player) -> bool {
    let Some(cam) = cam.filter(|_| v.style == VatsStyle::Classic && !v.cam_off) else { return false };
    let Some(Run::Volley(vo)) = v.run.as_ref() else { return false };
    let (next, mid) = (vo.next_mark(), vo.mid_burst());
    if let Some((serial, stage, since)) = v.cam_wait {
        let pending = cam.handled() < serial;
        if pending && v.now - since > CAM_PATIENCE {
            info!("vats mode: classic: no camera answered in {CAM_PATIENCE:.0} s; the attacks play without cameras");
            (v.cam_off, v.cam_wait, v.cam_ask) = (true, None, None);
            return false;
        }
        if pending {
            return true;
        }
        match stage {
            CamStage::Shoot if !v.cam_released => {
                if cam.attack_playing() && v.now - since < SHOOT_LEAD {
                    return true;
                }
                v.cam_released = true;
                if let Some(Run::Volley(vo)) = v.run.as_mut() {
                    vo.release();
                }
                return false;
            }
            CamStage::Shoot => {
                // The burst's later rounds go on in the camera.
                if mid {
                    return false;
                }
                if cam.attack_playing() {
                    return true;
                }
                (v.cam_wait, v.cam_ask) = (None, None);
                if let Some(mark) = v.cam_led {
                    classic_hit_camera(v, mark, player);
                    if v.cam_wait.is_some() {
                        return true;
                    }
                }
            }
            CamStage::Hit => {
                if cam.attack_playing() {
                    return true;
                }
                (v.cam_wait, v.cam_ask) = (None, None);
            }
        }
    }
    let Some(k) = next.filter(|k| v.cam_led != Some(*k)) else { return false };
    v.cam_led = Some(k);
    v.cam_released = false;
    let Some(shot) = v.shot_of(k, player) else {
        v.cam_released = true;
        return false;
    };
    v.cam_serial += 1;
    v.cam_ask = Some((v.cam_serial, CameraAsk { mark: k, stage: CamStage::Shoot, shot }));
    v.cam_wait = Some((v.cam_serial, CamStage::Shoot, v.now));
    true
}

/// Mark `i`'s attack has left and its Shoot camera is done: its Hit camera is asked for
/// (vats-classic.md §8), but not the last mark's, whose kill is the kill camera's.
fn classic_hit_camera(v: &mut VatsMode, i: usize, player: &Player) {
    if v.style != VatsStyle::Classic || v.cam_off || i + 1 >= v.queue.len() {
        return;
    }
    let Some(shot) = v.shot_of(i, player) else { return };
    v.cam_serial += 1;
    v.cam_ask = Some((v.cam_serial, CameraAsk { mark: i, stage: CamStage::Hit, shot }));
    v.cam_wait = Some((v.cam_serial, CamStage::Hit, v.now));
}

/// The attacks still to play whose target has died are dropped (their AP was only
/// reserved), and a burst under way on it is cut short; with none left to play the mode
/// ends [`AFTER_KILL`] later (`kill_end`) [design: the fork owner 2026-11-10; until then
/// the queue played on at the body]. Attacks queued on other targets play on. Under
/// the player's V.A.T.S. Drops Shots at a Lost Weapon tweak, so are the attacks marked on
/// an actor's weapon once it holds none (disarmed, its weapon shot from its hand)
/// [design: docs/design/tweaks.md].
fn drop_dead_marks(v: &mut VatsMode, script: Option<&ScriptState>, parts: &ActorParts) {
    if v.kill_end.is_some() {
        return;
    }
    let lost_weapon = script.and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.vats_drops_disarmed_shots);
    let disarmed = |m: &fnv_combat::vats::Mark| lost_weapon && m.part == Some(WEAPON_PART) && parts.by_ref.get(&m.target.0).is_some_and(|p| p.weapon.is_none());
    let lost = |k: usize| v.spots.get(k).is_some_and(|s| s.lost);
    let gone: Vec<bool> = v.queue.marks().iter().enumerate().map(|(k, m)| lost(k) || (m.target.0 != 0 && crate::combat::is_dead(script, m.target.0)) || disarmed(m)).collect();
    if !gone.contains(&true) {
        return;
    }
    let is_gone = |k: usize| gone.get(k).copied().unwrap_or(false);
    let mut dropped: Vec<usize> = Vec::new();
    let mut cut = false;
    let none_left = match v.run.as_mut() {
        Some(Run::Volley(vo)) => {
            if vo.fired >= 1 && is_gone(vo.fired - 1) {
                cut = vo.cut_burst();
            }
            while let Some(k) = vo.next_mark().filter(|k| is_gone(*k)) {
                vo.skip();
                dropped.push(k);
            }
            vo.all_fired()
        }
        Some(Run::Melee(run)) => {
            while let Some(k) = run.approaching().filter(|k| is_gone(*k)) {
                run.skip();
                dropped.push(k);
            }
            run.all_fired()
        }
        None => false,
    };
    if dropped.is_empty() && !cut {
        return;
    }
    // A dropped attack's camera, asked for but not yet led into, is not waited for.
    if v.cam_led.is_some_and(|k| dropped.contains(&k)) {
        (v.cam_led, v.cam_wait, v.cam_ask, v.cam_released) = (None, None, None, false);
    }
    let who = dropped.first().and_then(|k| v.spots.get(*k).zip(v.queue.marks().get(*k))).map_or_else(|| "its target".to_string(), |(s, m)| format!("{} {}", s.name, m.target));
    let why = match dropped.first().and_then(|k| v.queue.marks().get(*k).zip(v.spots.get(*k))) {
        Some((_, s)) if s.lost => "flies no more",
        Some((m, _)) if !crate::combat::is_dead(script, m.target.0) => "holds no weapon",
        _ => "is dead",
    };
    info!(
        "vats mode: {who} {why}: {} queued attacks dropped{}{}",
        dropped.len(),
        if cut { ", the burst cut short" } else { "" },
        if none_left { format!("; none left to play, the mode ends in {AFTER_KILL:.0} s") } else { String::new() }
    );
    if none_left {
        v.kill_end = Some(v.now);
    }
}

/// The classic style's targets now (vats-classic.md §3): every living actor with a hit
/// volume within the weapon's reach and `fVATSMaxEngageDistance`, within
/// [`CLASSIC_FRONT`] of the line the player looked along on entering, in sight and not
/// barred ([`barred`]: dead, cloaked without Enhanced Sensors, No VATS Melee), hostile or
/// not; left to right. With them, the place of the one nearest that line, and the actors
/// passed over by the rule that left each out (the refusal's log line).
///
/// In sight is any part of it ([`actor_in_sight`]): in a fight the enemies stand behind
/// cover, and one whose chest is behind a rock while its head and gun show is a target
/// [design: the fork owner's tester, 2026-11-10, V did nothing in a fight until one
/// enemy was down; hypothesis: that enemy's aim point was hidden, the one rule here that
/// a fight changes].
fn classic_targets(v: &VatsMode, combat: &Combat, mut script: Option<&mut ScriptState>, sim: &mut SimState, phys: &mut PhysState, eye: [f32; 3]) -> (Vec<Candidate>, Option<usize>, classic::Missed) {
    let reach = v.weapon.as_ref().map_or(REACH, |w| w.reach).min(combat.rules.vats_classic.max_engage.max(1.0));
    let dir = v.classic.entry_dir;
    let front = CLASSIC_FRONT.to_radians();
    let filter = shot_filter(phys);
    let actors: Vec<u32> = phys.world.actor_volumes().filter(|&a| a != FormId::PLAYER_REF.0).collect();
    let mut out = Vec::new();
    let mut missed = classic::Missed { reach, ..Default::default() };
    // A melee or unarmed weapon reaches from body to body, as its swing does
    // (`fnv_combat::damage::melee_reach`): the target's radius comes off the distance, the
    // player's is in the reach (`VatsWeapon::classic_reach`, #2656).
    let to_body = v.weapon.as_ref().is_some_and(|w| w.style == AttackStyle::Melee);
    for actor in actors {
        let Some(p) = phys.world.actor_aim_point(actor) else { continue };
        let to = [p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]];
        let centre = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt();
        if centre < 1.0 {
            continue;
        }
        let body = if to_body { phys.world.actor_volume(actor).map_or(0.0, |b| b.radius) } else { 0.0 };
        let len = (centre - body).max(0.0);
        if len > reach {
            missed.nearest_far = if missed.far == 0 { len } else { missed.nearest_far.min(len) };
            missed.far += 1;
            continue;
        }
        let (bearing, off) = classic::bearing_of(dir, to);
        if off > front {
            missed.aside += 1;
        } else if barred(v, script.as_deref_mut(), sim, actor).is_err() {
            missed.barred += 1;
        } else if !actor_in_sight(phys, eye, actor, p, &filter) {
            missed.hidden += 1;
        } else {
            out.push(Candidate { actor, bearing, off });
        }
    }
    let first = classic::order(&mut out);
    (out, first, missed)
}

/// Whether any of `actor` shows from `eye`: its aim point (`aim`), else the middle of one
/// of its body part capsules, the weapon in its hand among them.
fn actor_in_sight(phys: &mut PhysState, eye: [f32; 3], actor: u32, aim: [f32; 3], filter: &Filter) -> bool {
    in_sight(phys, eye, aim, filter) || phys.world.actor_part_centers(actor).into_iter().any(|p| in_sight(phys, eye, p, filter))
}

/// After [`try_enter`] let the mode start in the classic style: its targets are looked
/// for, the nearest the crosshair selected; with none the mode does not start
/// (vats-classic.md §3) [design: owner 2026-11-09, V with nothing to target only plays
/// the refusal sound].
fn classic_enter(v: &mut VatsMode, combat: &Combat, script: Option<&mut ScriptState>, sim: &mut SimState, phys: &mut PhysState, player: &Player, sounds: &mut Sounds) {
    if v.phase != Phase::Marking || v.style != VatsStyle::Classic {
        return;
    }
    v.classic = ClassicState { scale: v.classic.scale, entry_dir: player.look_dir(), entry_view: Some((player.heading, player.pitch)), entry_feet: player.feet, scanned_at: v.now, ..ClassicState::default() };
    let (targets, first, missed) = classic_targets(v, combat, script, sim, phys, player.eye());
    let Some(first) = first else {
        // Not a stay: nothing of it is counted or shown.
        v.phase = Phase::Off;
        v.entered_at = None;
        v.queue = MarkQueue::default();
        v.weapon = None;
        v.classic.entry_view = None;
        v.stats.entered = v.stats.entered.saturating_sub(1);
        v.stats.refused += 1;
        sounds.looping(v, false);
        sounds.play(v, "UIVATSEnterFail");
        info!("vats mode: refused: no target in sight (classic): {}", missed.text(CLASSIC_FRONT));
        return;
    };
    v.classic.selected = Some(targets[first].actor);
    sounds.play(v, "UIVATSEnter");
    info!(
        "vats mode: classic: {} targets, {:08X} selected; the clock at {:.2} while targets are chosen",
        targets.len(),
        targets[first].actor,
        v.classic.scale
    );
    v.classic.targets = targets;
}

/// The classic style's frame while targets are chosen (vats-classic.md §3 to §5): the
/// targets are looked for again every [`CLASSIC_SCAN`], queued attacks on a target that
/// died or left sight are dropped (their AP was only reserved, so it is back), the
/// movement controls move the selection, and the view turns onto the selected target in
/// real time. The actor and part place for [`preview`]; `None` once no target is left
/// (the mode has ended).
#[allow(clippy::too_many_arguments)]
fn classic_frame(
    v: &mut VatsMode,
    keys: &Keys,
    combat: &Combat,
    script: Option<&mut ScriptState>,
    sim: &mut SimState,
    phys: &mut PhysState,
    player: &mut Player,
    control: &PlayerControl,
    real_dt: f32,
    follow_target: bool,
    sounds: &mut Sounds,
) -> Option<(u32, usize)> {
    let eye = player.eye();
    let auto_head = script.as_deref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.vats_auto_head);
    if v.now - v.classic.scanned_at >= CLASSIC_SCAN {
        v.classic.scanned_at = v.now;
        let (targets, first, missed) = classic_targets(v, combat, script, sim, phys, eye);
        // Queued attacks whose target is no longer one.
        let mut i = v.queue.len();
        while i > 0 {
            i -= 1;
            let m = v.queue.marks()[i];
            if !targets.iter().any(|c| c.actor == m.target.0) {
                v.queue.remove(i);
                let s = v.spots.remove(i);
                info!("vats mode: classic: the attack queued on {} {} ({}) is dropped: no longer a target; {:.0} AP reserved", s.name, m.target, s.part, v.queue.reserved());
            }
        }
        // The selection stays on its actor; when that is gone it goes to the nearest in
        // the order.
        if !v.classic.selected.is_some_and(|a| targets.iter().any(|c| c.actor == a)) {
            let was = v.classic.selected;
            v.classic.selected = first.map(|i| targets[i].actor);
            if let (Some(was), Some(now)) = (was, v.classic.selected) {
                info!("vats mode: classic: {was:08X} is no longer a target; {now:08X} selected");
            }
        }
        v.classic.targets = targets;
        if v.classic.selected.is_none() {
            info!("vats mode: classic: no target left: {}", missed.text(CLASSIC_FRONT));
            end(v, "no target left", sounds);
            return None;
        }
    }
    let n = v.classic.targets.len();
    let mut at = v.classic.selected.and_then(|a| v.classic.targets.iter().position(|c| c.actor == a))?;
    if keys.target_step != 0 && n > 1 {
        at = classic::cycle(at, n, keys.target_step);
        v.classic.selected = Some(v.classic.targets[at].actor);
        info!("vats mode: classic: target {} of {n}, {:08X}", at + 1, v.classic.targets[at].actor);
        sounds.play(v, "UIVATSSelectTarget");
    }
    let actor = v.classic.targets[at].actor;
    // The part: the target's best chance until the player moves it (the preview is a
    // frame behind a new target, so it is set once the preview is this target's).
    let parts = if v.preview.actor == Some(actor) { v.preview.parts.len() } else { 0 };
    if parts > 0 {
        if v.classic.part_set != Some(actor) {
            v.classic.part_set = Some(actor);
            let best = (0..parts).max_by(|&a, &b| v.preview.parts[a].spot.chance.total_cmp(&v.preview.parts[b].spot.chance)).unwrap_or(0);
            // The player's V.A.T.S. Starts on the Head tweak: the head (the part whose
            // condition is Perception Condition) when the target has one, for each target
            // when it is first selected [design: docs/design/tweaks.md].
            let head = auto_head.then(|| head_part(&v.preview.parts)).flatten();
            v.classic.part = head.unwrap_or(best);
        }
        if keys.part_step != 0 && parts > 1 {
            v.classic.part = classic::cycle(v.classic.part, parts, keys.part_step);
            let p = &v.preview.parts[v.classic.part];
            info!("vats mode: classic: part {} ({})", p.spot.part, chance_text(p.spot.chance));
            sounds.play(v, "UIVATSMove");
        }
        // The pointer moved: the part whose box it is in, else the part of the target's
        // body its line into the world meets first (so a part in front of another is the
        // one taken, and anywhere on a large body counts), else the part whose point is
        // nearest it on the screen [design: the original picks on the body, observed on
        // the owner's screenshot with the pointer on Sunny Smiles's torso].
        let on_body = keys.ray.and_then(|(from, dir)| match phys.ray(from, dir, REACH, shot_filter(phys))?.owner {
            Owner::Actor { form_id, part, .. } if form_id == actor && part != fnv_phys::actors::WHOLE => v.preview.parts.iter().position(|p| p.part == Some(part)),
            _ => None,
        });
        let picked = keys.hover_part.or(on_body).or(keys.near_part);
        if keys.pointer_moved {
            v.pointed = picked.is_some();
        }
        if let Some(i) = picked.filter(|&i| i < parts && i != v.classic.part) {
            v.classic.part = i;
            let p = &v.preview.parts[i];
            info!("vats mode: classic: part {} ({}), {}", p.spot.part, chance_text(p.spot.chance), if keys.pointer_moved { "under the pointer" } else { "by the left stick" });
            sounds.play(v, "UIVATSMove");
        }
        v.classic.part = v.classic.part.min(parts - 1);
    }
    // Keep a validated body contact under its finger until the terminal edge.
    // Ordinary selection resumes target follow (vats-classic.md §5).
    if let Some(p) = phys.world.actor_aim_point(actor).filter(|_| follow_target) {
        let (heading, pitch) = Crosshair::aim_at(control.crosshair.as_ref(), eye, p);
        player.heading = classic::ease_angle(player.heading, heading, classic::TURN_RATE, real_dt);
        player.pitch = classic::ease_angle(player.pitch, pitch, classic::TURN_RATE, real_dt);
    }
    Some((actor, v.classic.part))
}

/// The point to aim at on an actor: its part's capsule centre, the whole capsule's aim
/// point without parts.
fn point_of(phys: &PhysState, actor: u32, part: Option<u8>) -> Option<[f32; 3]> {
    match part {
        Some(p) => phys.world.actor_part_center(actor, p).or_else(|| phys.world.actor_aim_point(actor)),
        None => phys.world.actor_aim_point(actor),
    }
}

/// The marks follow their actors, and their projectiles while they fly.
fn follow(v: &mut VatsMode, phys: &PhysState, combat: &Combat) {
    let marks: Vec<_> = v.queue.marks().to_vec();
    for (m, s) in marks.iter().zip(v.spots.iter_mut()) {
        if let Some(id) = s.projectile {
            match combat.projectile_at(id) {
                Some(p) => s.point = p,
                None => s.lost = true,
            }
            continue;
        }
        if let Some(p) = point_of(phys, m.target.0, m.part) {
            s.point = p;
        }
    }
}

/// Why an actor cannot be marked (§6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bar {
    Dead,
    /// Cloaked, and the player has no Enhanced Sensors.
    Cloaked,
    /// No VATS Melee, with a melee weapon in hand.
    NoVatsMelee,
    /// A companion, under the player's Ignore Companions tweak.
    Companion,
    /// Never a VATS target (`fnv_game::Game::vats_untargetable`), with the reason.
    NoTarget(&'static str),
}

/// Whether `actor` may be marked (§6): `Ok(true)` when it is cloaked and Enhanced Sensors
/// lets it be marked all the same.
fn barred(v: &VatsMode, mut script: Option<&mut ScriptState>, sim: &mut SimState, actor: u32) -> Result<bool, Bar> {
    if crate::combat::is_dead(script.as_deref(), actor) {
        return Err(Bar::Dead);
    }
    // The player's tweak: companions are not targets [design: docs/design/tweaks.md].
    if script.as_deref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.vats_ignores_companions && rt.game.is_teammate(FormId(actor))) {
        return Err(Bar::Companion);
    }
    // The player's tweak: with Shift held, no one who would not fight the player [design: docs/design/tweaks.md].
    if SHIFT_HELD.load(std::sync::atomic::Ordering::Relaxed) {
        if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()).filter(|rt| rt.game.tweaks.vats_shift_ignores_friends) {
            if !crate::script::with_world(rt, sim, |rt| rt.game.would_attack(FormId(actor), FormId::PLAYER_REF)) {
                return Err(Bar::Companion);
            }
        }
    }
    // SetVATSTargetable 0 (`fnv_game::nvse_jip14`): no target of the classic style either.
    if script.as_deref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| !rt.game.actor_vats_targetable(FormId(actor))) {
        return Err(Bar::NoTarget("SetVATSTargetable 0"));
    }
    // A cloaked actor (Invisibility or Chameleon at least 1: a Stealth Boy, a Nightkin's
    // cloak) is not marked unless the player has Enhanced Sensors, whose entry point 69 Has
    // Improved Detection gives "VATS targeting of cloaked enemies" [documented: GECK wiki
    // Perk entry points; hypothesis: the actor values are what "cloaked" reads]
    // (perk-entry-points.md §13).
    let mut cloaked = false;
    if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
        let (c, sensors, never) = crate::script::with_world(rt, sim, |rt| {
            let g = &mut rt.game;
            let av = |g: &mut fnv_game::Game, v: u16| fnv_game::functions::actor_value_capped(g, FormId(actor), v);
            let cloaked = av(g, fnv_game::detection::INVISIBILITY) >= 1.0 || av(g, fnv_game::detection::CHAMELEON) >= 1.0;
            (cloaked, cloaked && g.player_has_entry(fnv_game::perks::HAS_IMPROVED_DETECTION), g.vats_untargetable(FormId(actor)))
        });
        if let Some(why) = never {
            return Err(Bar::NoTarget(why));
        }
        if c && !sensors {
            return Err(Bar::Cloaked);
        }
        cloaked = c;
    }
    // "The player cannot go into VATS with this NPC if he has a melee weapon equipped"
    // [documented: GECK wiki Category:NPC, No VATS Melee] (actor-flags.md §1).
    let melee = v.weapon.as_ref().is_some_and(|w| w.style == AttackStyle::Melee);
    // The base from the simulation: the game's world is not attached outside its turn.
    let flagged = melee
        && sim.base_of(actor).is_some_and(|b| script.as_deref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| { let (seed, at) = sim.pick(&rt.game, FormId(actor)); rt.game.runtime_template_source(b, seed, at, |_| false).is_some_and(|(_, d)| d.no_vats_melee) }));
    if flagged {
        return Err(Bar::NoVatsMelee);
    }
    Ok(cloaked)
}

/// What a mark on `actor`'s `part` at `point` is (§17.2, §17.3): the names, the part's
/// base chance and size, the to-hit perks seeing it as the shot, and its chance now; with
/// the perks' names for the log. A mark and the preview of a part (§17.5) both come from
/// here, so the preview's figure is the one the mark then shows.
#[allow(clippy::too_many_arguments)]
fn spot_on(v: &VatsMode, combat: &Combat, parts: &ActorParts, script: Option<&mut ScriptState>, sim: &mut SimState, player: &Player, actor: u32, part: Option<u8>, point: [f32; 3], earlier: u32) -> (Spot, Vec<String>) {
    let bp = combat.vats_part(parts, actor, part);
    let part_name = match (part, bp.as_ref().and_then(|b| b.name.clone())) {
        (Some(crate::hitvol::WEAPON_PART), _) => "its weapon".to_string(),
        (_, Some(n)) => n,
        (Some(p), None) => format!("part {p}"),
        (None, None) => "whole".to_string(),
    };
    let condition = bp.as_ref().and_then(|b| b.actor_value.as_deref()).and_then(fnv_game::combat::limb_condition);
    // The name the health bar shows (`crate::hud_target`): the template chain's, through the
    // leveled list to the entry picked for this actor, and never an Editor ID (#3445).
    let name = sim.ref_shown_name(script.as_ref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game), actor).unwrap_or_default().to_string();
    // The part's base chance (§17.2): its BPTD To Hit Chance; an actor without
    // parts, or the weapon it holds, our own.
    let (base, size) = match (part, bp.as_ref()) {
        (Some(WEAPON_PART), _) => (HELD_WEAPON_TO_HIT, SIZE_HELD_WEAPON),
        (_, Some(b)) => (b.to_hit_chance as f32, fnv_combat::vats::part_size(b.actor_value.as_deref())),
        (_, None) => (WHOLE_TO_HIT, SIZE_TORSO),
    };
    // The to-hit perks see this mark as the shot (§17.3).
    let shot = v.weapon.as_ref().map(|w| fnv_game::vats::VatsShot {
        weapon: w.id,
        target: FormId(actor),
        distance: crate::combat::distance(player.eye(), point) as f64,
        condition,
        success: false,
        height: (point[2] - player.feet[2]) as f64,
    });
    let (perks, names) = match (shot, script.and_then(|s| s.rt.as_mut())) {
        (Some(shot), Some(rt)) => with_world(rt, sim, |rt| {
            let perks = rt.game.vats_hit_perks(shot);
            let names: Vec<String> = perks.ops.iter().map(|(p, _)| rt.game.data.name(*p)).collect();
            (perks, names)
        }),
        _ => (HitPerks::default(), Vec::new()),
    };
    let mut spot = Spot { point, name, part: part_name, condition, base, size, perks, earlier, chance: 0.0, landed: None, special: false, damage: 0.0, projectile: None, lost: false, seen: None };
    if let Some(w) = v.weapon.as_ref() {
        spot.chance = chance_of(&spot, w, player.eye(), combat.vats_cone);
    }
    (spot, names)
}

/// One body part's chance before it is marked (§17.5).
#[derive(Debug, Clone, Default)]
struct PreviewPart {
    /// The part (`None`: the whole actor, one capsule).
    part: Option<u8>,
    /// What a mark on it would be: its point now and its chance now.
    spot: Spot,
    /// Drawn this frame: in sight and not marked already.
    shown: bool,
    /// The part the crosshair is on: a click marks it.
    under: bool,
    /// The classic style: the attacks queued on it (vats-classic.md §4).
    queued: u32,
    /// The classic style: the part's condition now, 0 (crippled) to 1; `None` for a part
    /// without a condition actor value.
    condition: Option<f32>,
    /// What one attack on it would take off the target's Health if it lands (a burst's
    /// rounds together), read as often as the parts are.
    damage: f32,
}

/// The head among a target's parts: the one whose condition actor value is Perception
/// Condition (`fnv_game::combat::HEAD`).
fn head_part(parts: &[PreviewPart]) -> Option<usize> {
    parts.iter().position(|p| p.spot.condition == Some(fnv_game::combat::HEAD))
}

/// The actor whose parts' chances are shown before marking (§17.5).
#[derive(Debug, Clone, Default)]
struct Preview {
    actor: Option<u32>,
    parts: Vec<PreviewPart>,
    /// Real seconds when the parts (and their perks) were last worked out.
    built_at: f64,
    /// The classic style: the actor's health now and at most.
    health: Option<(f32, f32)>,
}

/// The preview's actor, when the crosshair is not on one, is the nearest valid actor
/// within this angle of the crosshair's line, degrees [design].
pub const PREVIEW_CONE: f32 = 10.0;
/// The preview's parts, their names and their perks are worked out again this often,
/// real seconds (their points and chances every frame) [design: the perks' conditions
/// may read the distance; four times a second keeps them as fresh as the eye can tell].
const PREVIEW_REBUILD: f64 = 0.25;
/// The most preview labels drawn at once [engineering choice: a skeleton's parts and its
/// weapon].
const MAX_PREVIEW: usize = 24;

/// The shots' filter: through what the character passes and the actors' walking
/// capsules (their hit volumes are their bodies).
fn shot_filter(phys: &PhysState) -> Filter {
    let mut filter = Filter { exclude_layers: layers::shot_ignores(), ..Default::default() };
    filter.exclude.extend(phys.npcs.values().map(|b| b.ch.key()));
    filter
}

/// The actor to preview (§17.5) and, when the crosshair is on it, the part it is on: the
/// one under the crosshair, else the nearest living actor with a hit volume within
/// [`PREVIEW_CONE`] of the crosshair's line, in reach and in sight.
fn preview_target(phys: &mut PhysState, script: Option<&ScriptState>, from: [f32; 3], dir: [f32; 3], reach: f32, filter: &Filter) -> Option<(u32, Option<Option<u8>>)> {
    if let Some(h) = phys.ray(from, dir, reach, filter.clone()) {
        if let Owner::Actor { form_id, part, .. } = h.owner {
            if !crate::combat::is_dead(script, form_id) {
                return Some((form_id, Some((part != fnv_phys::actors::WHOLE).then_some(part))));
            }
        }
    }
    let cos_cone = PREVIEW_CONE.to_radians().cos();
    let mut best: Option<(f32, u32, [f32; 3])> = None;
    for actor in phys.world.actor_volumes() {
        if actor == FormId::PLAYER_REF.0 {
            continue;
        }
        let Some(p) = phys.world.actor_aim_point(actor) else { continue };
        let d = [p[0] - from[0], p[1] - from[1], p[2] - from[2]];
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if !(1.0..=reach).contains(&len) {
            continue;
        }
        let cos = (d[0] * dir[0] + d[1] * dir[1] + d[2] * dir[2]) / len;
        if cos < cos_cone || best.is_some_and(|b| cos <= b.0) || crate::combat::is_dead(script, actor) {
            continue;
        }
        best = Some((cos, actor, p));
    }
    let (_, actor, p) = best?;
    in_sight(phys, from, p, filter).then_some((actor, None))
}

/// Whether the line from `from` to `to` meets no world before it: an actor's volume in
/// the way does not hide it [design].
fn in_sight(phys: &mut PhysState, from: [f32; 3], to: [f32; 3], filter: &Filter) -> bool {
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if len < 1.0 {
        return true;
    }
    let dir = [d[0] / len, d[1] / len, d[2] / len];
    match phys.ray(from, dir, len, filter.clone()) {
        None => true,
        Some(h) => matches!(h.owner, Owner::Actor { .. }) || h.distance >= len - 2.0,
    }
}

/// The parts an actor's preview shows: each body part its hit volume has a capsule for
/// (the weapon it holds too), in the body part data's order; the whole actor with one
/// capsule.
fn parts_of(phys: &PhysState, parts: &ActorParts, actor: u32, out: &mut Vec<Option<u8>>) {
    out.clear();
    if phys.world.actor_part_count(actor) > 0 {
        if let Some(posed) = parts.by_ref.get(&actor) {
            for s in &posed.segments {
                if !out.contains(&Some(s.part)) && phys.world.actor_part_center(actor, s.part).is_some() {
                    out.push(Some(s.part));
                }
            }
        }
    }
    if out.is_empty() {
        out.push(None);
    }
}

/// While marking, every body part's chance on the previewed actor (§17.5): the actor and
/// its parts' spots are worked out again when the actor changes and every
/// [`PREVIEW_REBUILD`]; their points, chances and sight every frame.
#[allow(clippy::too_many_arguments)]
fn preview(v: &mut VatsMode, combat: &Combat, parts: &ActorParts, mut script: Option<&mut ScriptState>, sim: &mut SimState, phys: &mut PhysState, player: &Player, control: &PlayerControl, chosen: Option<(u32, usize)>) {
    let (from, dir) = Crosshair::ray(control.crosshair.as_ref(), player.eye(), player.look_dir());
    let reach = v.weapon.as_ref().map_or(REACH, |w| w.reach);
    let filter = shot_filter(phys);
    // An explicit target comes from Classic selection or a validated body touch.
    // The touch consumer resolves the actual part after this preview is built;
    // without either owner, Mark and Fire follows the ordinary crosshair.
    let target = match chosen {
        Some((actor, _)) => Some((actor, None)),
        None => preview_target(phys, script.as_deref(), from, dir, reach, &filter),
    };
    let actor = target.map(|t| t.0);
    let under = target.and_then(|t| t.1);
    let mut pv = std::mem::take(&mut v.preview);
    if actor != pv.actor || v.now - pv.built_at >= PREVIEW_REBUILD {
        let changed = actor != pv.actor;
        pv.actor = actor;
        pv.built_at = v.now;
        pv.parts.clear();
        if let Some(actor) = actor.filter(|&a| barred(v, script.as_deref_mut(), sim, a).is_ok()) {
            let mut list = std::mem::take(&mut v.preview_list);
            parts_of(phys, parts, actor, &mut list);
            let sees = throw_sees(v, combat, script.as_deref());
            for &part in list.iter().take(MAX_PREVIEW) {
                let Some(point) = point_of(phys, actor, part) else { continue };
                let (mut spot, _) = spot_on(v, combat, parts, script.as_deref_mut(), sim, player, actor, part, point, 0);
                if let (true, Some(w)) = (sees, v.weapon.as_ref()) {
                    spot.seen = Some(part_seen(phys, player.eye(), point));
                    spot.chance = chance_of(&spot, w, player.eye(), combat.vats_cone);
                }
                pv.parts.push(PreviewPart { part, spot, shown: false, under: false, queued: 0, condition: None, damage: 0.0 });
            }
            v.preview_list = list;
            // What an attack on each part would do (the enemy bar's damage in sight).
            if let Some(w) = v.weapon.as_ref() {
                let rounds = match w.style {
                    AttackStyle::Burst { shots, .. } => shots as f32,
                    _ => 1.0,
                };
                for p in &mut pv.parts {
                    p.damage = crate::combat::vats_hit_estimate(combat, parts, script.as_deref_mut(), sim, w.id, actor, p.part, 1.0).unwrap_or(0.0) * rounds;
                }
            }
            // The target screen shows each part's condition, in both styles, and the
            // classic style the target's health (vats-classic.md §4), read as often as the
            // parts are.
            pv.health = None;
            if let Some(rt) = script.and_then(|s| s.rt.as_mut()) {
                let (limbs, health) = with_world(rt, sim, |rt| (rt.game.limb_conditions(FormId(actor)), rt.game.health_of(FormId(actor))));
                for p in &mut pv.parts {
                    p.condition = p.spot.condition.and_then(|c| limbs.get(usize::from(c).wrapping_sub(usize::from(*fnv_game::combat::LIMB_CONDITIONS.start()))).map(|v| (*v as f32 / 100.0).clamp(0.0, 1.0)));
                }
                pv.health = Some((health.0 as f32, health.1 as f32));
            }
            if changed {
                let name = pv.parts.first().map_or("?", |p| p.spot.name.as_str());
                let figures: Vec<String> = pv.parts.iter().map(|p| format!("{} {}", p.spot.part, chance_text(p.spot.chance))).collect();
                info!("vats mode: preview {name} {actor:08X}: {}", figures.join(", "));
            }
        }
    }
    if let (Some(actor), Some(w)) = (pv.actor, v.weapon.as_ref()) {
        let eye = player.eye();
        let one = pv.parts.len() == 1;
        for (i, p) in pv.parts.iter_mut().enumerate() {
            if let Some(at) = point_of(phys, actor, p.part) {
                p.spot.point = at;
            }
            let queued = v.queue.marks().iter().filter(|m| m.target.0 == actor && m.part == p.part).count() as u32;
            if let Some((_, place)) = chosen {
                // The classic style: the selected part is the bracketed one, a part with
                // attacks queued keeps its label and counts them, and Concentrated Fire's
                // bonus for the next attack shows before it is queued.
                p.spot.earlier = queued;
                p.spot.chance = chance_of(&p.spot, w, eye, combat.vats_cone);
                p.under = i == place;
                p.queued = queued;
                p.shown = true;
                continue;
            }
            p.spot.chance = chance_of(&p.spot, w, eye, combat.vats_cone);
            // With one capsule the whole actor is under the crosshair whenever it is.
            p.under = under.is_some_and(|u| one || u == p.part);
            // A part marked already shows its mark's label only.
            p.shown = queued == 0 && in_sight(phys, from, p.spot.point, &filter);
        }
    }
    v.preview = pv;
}

/// Left click: mark the actor (and body part) under the crosshair (§6, §7.2).
#[allow(clippy::too_many_arguments)]
fn mark(
    v: &mut VatsMode,
    combat: &Combat,
    parts: &ActorParts,
    mines: Option<&crate::mines::Mines>,
    mut script: Option<&mut ScriptState>,
    sim: &mut SimState,
    phys: &mut PhysState,
    player: &Player,
    control: &PlayerControl,
    hud: &mut HudMessage,
    sounds: &mut Sounds,
) {
    let (from, dir) = Crosshair::ray(control.crosshair.as_ref(), player.eye(), player.look_dir());
    let reach = v.weapon.as_ref().map_or(REACH, |w| w.reach);
    // Through what the character passes and the actors' walking capsules (their hit
    // volumes are their bodies), as the shots go.
    let mut filter = Filter { exclude_layers: layers::shot_ignores(), ..Default::default() };
    filter.exclude.extend(phys.npcs.values().map(|b| b.ch.key()));
    let hit = phys.ray(from, dir, reach, filter);
    // A projectile in flight, under the player's tweaks, nearer than what the ray met
    // [design: docs/design/tweaks.md].
    let rules = script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| ProjectileRules::of(&rt.game.tweaks)).unwrap_or_default();
    if rules.on {
        let until = hit.as_ref().map_or(reach, |h| crate::combat::distance(from, h.point) + PROJECTILE_PICK);
        if let Some(p) = projectile_under(&combat.vats_projectiles(), &rules, from, dir, until) {
            mark_projectile(v, p, script, sim, player, hud, sounds);
            return;
        }
    }
    // A placed object VATS may target (§20): a destructible flagged VATS Targetable, or
    // one a script's SetVatsTarget turned on.
    if let Some((object, point)) = hit.as_ref().and_then(|h| match h.owner {
        Owner::Ref { form_id, .. } => Some((form_id, h.point)),
        _ => None,
    }) {
        let targetable = script.as_deref_mut().and_then(|s| s.rt.as_mut()).is_some_and(|rt| with_world(rt, sim, |rt| rt.game.vats_targetable(FormId(object))));
        // The player's tweak: a live mine lying in the world [design: docs/design/tweaks.md].
        let targetable = targetable || (rules.mines && mines.is_some_and(|m| m.is_live(FormId(object))));
        if targetable {
            mark_object(v, object, point, script, sim, player, hud, sounds);
        } else {
            info!("vats mode: nothing to mark under the crosshair ({object:08X} is not a VATS target)");
        }
        return;
    }
    let Some((actor, part)) = hit.as_ref().and_then(|h| match h.owner {
        Owner::Actor { form_id, part, .. } => Some((form_id, (part != fnv_phys::actors::WHOLE).then_some(part))),
        _ => None,
    }) else {
        info!("vats mode: nothing to mark under the crosshair");
        return;
    };
    let fallback = hit.map_or(from, |h| h.point);
    mark_actor(v, combat, parts, script, sim, phys, player, hud, sounds, actor, part, fallback, false);
}

/// Mark `actor` on `part` (§6, §7.2): the crosshair's click, and the classic style's
/// queued attack on the selected part (vats-classic.md §4). `fallback` is the mark's
/// point when the part has no volume to give one.
#[allow(clippy::too_many_arguments)]
fn mark_actor(
    v: &mut VatsMode,
    combat: &Combat,
    parts: &ActorParts,
    mut script: Option<&mut ScriptState>,
    sim: &mut SimState,
    phys: &mut PhysState,
    player: &Player,
    hud: &mut HudMessage,
    sounds: &mut Sounds,
    actor: u32,
    part: Option<u8>,
    fallback: [f32; 3],
    special: bool,
) {
    // SetVATSTargetable 0 (`fnv_game::nvse_jip14`).
    if script.as_deref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| !rt.game.actor_vats_targetable(FormId(actor))) {
        info!("vats mode: {actor:08X} is not a VATS target (SetVATSTargetable 0): not marked");
        return;
    }
    match barred(v, script.as_deref_mut(), sim, actor) {
        Err(Bar::Dead) => {
            info!("vats mode: {actor:08X} is dead: not marked");
            return;
        }
        Err(Bar::Cloaked) => {
            v.stats.refused += 1;
            info!("vats mode: {actor:08X} is cloaked: not marked (Enhanced Sensors would let the player mark it)");
            return;
        }
        Err(Bar::NoTarget(why)) => {
            v.stats.refused += 1;
            info!("vats mode: {actor:08X} is {why}: not marked");
            return;
        }
        Err(Bar::NoVatsMelee) => {
            v.stats.refused += 1;
            info!("vats mode: {actor:08X} is flagged No VATS Melee: not marked with a melee weapon");
            return;
        }
        Err(Bar::Companion) => {
            v.stats.refused += 1;
            info!("vats mode: {actor:08X} is a companion: not marked (tweak)");
            return;
        }
        Ok(true) => info!("vats mode: {actor:08X} is cloaked: marked through Enhanced Sensors"),
        Ok(false) => {}
    }
    let ap_now = ap_to_mark(script.as_deref_mut(), sim);
    let earlier = v.queue.marks().iter().filter(|m| m.target.0 == actor && m.part == part).count() as u32;
    // The classic style's special attack costs its own AP (vats-classic.md §9).
    let special = v.weapon.as_ref().and_then(|w| w.special).filter(|_| special && v.style == VatsStyle::Classic);
    let cost = special.map_or(v.queue.cost, |(_, cost)| cost);
    match v.queue.push_costing(FormId(actor), part, ap_now, cost) {
        Ok(k) => {
            let point = point_of(phys, actor, part).unwrap_or(fallback);
            v.stats.marks += 1;
            let sees = throw_sees(v, combat, script.as_deref());
            let (mut spot, names) = spot_on(v, combat, parts, script, sim, player, actor, part, point, earlier);
            // The player's tweak: a thrown weapon's chance by how much of the part shows.
            if let (true, Some(w)) = (sees, v.weapon.as_ref()) {
                spot.seen = Some(part_seen(phys, player.eye(), point));
                spot.chance = chance_of(&spot, w, player.eye(), combat.vats_cone);
                info!("vats mode: the part is {:.0}% in sight (tweak)", spot.seen.unwrap_or(1.0) * 100.0);
            }
            spot.special = special.is_some();
            // What it would do: the figure its part shows now, times a special attack's.
            spot.damage = v.preview.parts.iter().find(|p| v.preview.actor == Some(actor) && p.part == part).map_or(0.0, |p| p.damage) * special.map_or(1.0, |(s, _)| s.dam_mult);
            if let Some((s, cost)) = special {
                info!("vats mode: classic: mark {k} is a special attack: damage x{:.2}, {cost:.0} AP", s.dam_mult);
            }
            let perk_text = if names.is_empty() { String::new() } else { format!(", perks {}", names.join(", ")) };
            let cf = if spot.perks.concentrated_fire && earlier > 0 { format!(", concentrated fire +{earlier}") } else { String::new() };
            // What the preview showed for this part (§17.5): the same figure.
            let previewed = match v.preview.parts.iter().find(|p| earlier == 0 && v.preview.actor == Some(actor) && p.part == part) {
                Some(p) => format!("; previewed {}", chance_text(p.spot.chance)),
                None => String::new(),
            };
            info!(
                "vats mode: marked {} {actor:08X} ({}), mark {k}; {:.0} AP reserved, {} AP left; {:.0}% to hit (base {:.0}{perk_text}{cf}); {:.0} units, skill {:.0}, cone {}{previewed}",
                spot.name,
                spot.part,
                v.queue.reserved(),
                if ap_now.is_finite() { format!("{:.0}", ap_now - v.queue.reserved()) } else { "unlimited".into() },
                spot.chance,
                spot.base,
                crate::combat::distance(player.eye(), spot.point),
                v.weapon.as_ref().map_or(0.0, |w| w.skill),
                combat.vats_cone.map_or("none".to_string(), |c| format!("{c:.2} deg")),
            );
            v.spots.push(spot);
            sounds.play(v, if part.is_some() { "UIVATSSelectTargetPart" } else { "UIVATSSelectTarget" });
        }
        Err(r) => {
            v.stats.refused += 1;
            info!("vats mode: mark refused: {} ({} marks, {:.0} AP reserved)", r.message(), v.queue.len(), v.queue.reserved());
            hud.show(r.message().to_string());
            sounds.play(v, "UIVATSInsufficientAP");
        }
    }
}

/// Mark the placed object `object` at `point` (§20): the whole object, at
/// [`WHOLE_TO_HIT`]'s base chance; its point stays where it was marked.
#[allow(clippy::too_many_arguments)]
fn mark_object(v: &mut VatsMode, object: u32, point: [f32; 3], mut script: Option<&mut ScriptState>, sim: &mut SimState, player: &Player, hud: &mut HudMessage, sounds: &mut Sounds) {
    let ap_now = ap_to_mark(script.as_deref_mut(), sim);
    let earlier = v.queue.marks().iter().filter(|m| m.target.0 == object).count() as u32;
    match v.queue.push(FormId(object), None, ap_now) {
        Ok(k) => {
            let name = sim.base_of(object).and_then(|b| sim.name(b).map(str::to_string)).unwrap_or_else(|| "?".into());
            let shot = v.weapon.as_ref().map(|w| fnv_game::vats::VatsShot {
                weapon: w.id,
                target: FormId(object),
                distance: crate::combat::distance(player.eye(), point) as f64,
                condition: None,
                success: false,
                height: (point[2] - player.feet[2]) as f64,
            });
            let perks = match (shot, script.and_then(|s| s.rt.as_mut())) {
                (Some(shot), Some(rt)) => with_world(rt, sim, |rt| rt.game.vats_hit_perks(shot)),
                _ => HitPerks::default(),
            };
            let mut spot = Spot { point, name, part: "object".into(), condition: None, base: WHOLE_TO_HIT, size: SIZE_TORSO, perks, earlier, chance: 0.0, landed: None, special: false, damage: 0.0, projectile: None, lost: false, seen: None };
            if let Some(w) = v.weapon.as_ref() {
                spot.chance = chance_of(&spot, w, player.eye(), None);
            }
            v.stats.marks += 1;
            info!("vats mode: marked object {} {object:08X}, mark {k}; {:.0} AP reserved; {:.0}% to hit; {:.0} units", spot.name, v.queue.reserved(), spot.chance, crate::combat::distance(player.eye(), point));
            v.spots.push(spot);
            sounds.play(v, "UIVATSSelectTarget");
        }
        Err(r) => {
            v.stats.refused += 1;
            info!("vats mode: mark refused: {} ({} marks, {:.0} AP reserved)", r.message(), v.queue.len(), v.queue.reserved());
            hud.show(r.message().to_string());
            sounds.play(v, "UIVATSInsufficientAP");
        }
    }
}

/// How near the crosshair's line a projectile must pass to be marked: this many units,
/// or [`PROJECTILE_CONE`] radians off it far away [design].
const PROJECTILE_PICK: f32 = 24.0;
const PROJECTILE_CONE: f32 = 0.03;

/// Which projectiles may be marked: the player's tweaks [design: docs/design/tweaks.md].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ProjectileRules {
    /// Target Projectiles in V.A.T.S.
    pub on: bool,
    /// Its Mines option: thrown mines in the air, and live mines lying in the world.
    pub mines: bool,
    /// V.A.T.S. Targets Your Own Projectiles.
    pub own: bool,
    /// V.A.T.S. Ignores Non-Explosive Projectiles.
    pub explosive_only: bool,
}

impl ProjectileRules {
    pub fn of(t: &fnv_game::tweaks::Tweaks) -> ProjectileRules {
        ProjectileRules { on: t.vats_target_projectiles, mines: t.vats_target_projectiles && t.vats_target_mines, own: t.vats_target_own_projectiles, explosive_only: t.vats_ignore_non_explosive }
    }

    fn allows(&self, p: &crate::combat::VatsProjectile) -> bool {
        (self.own || !p.by_player) && (self.mines || !p.mine) && (p.explosive || !self.explosive_only)
    }
}

/// The projectile the crosshair's line (`from`, `dir`, as far as `until`) passes nearest,
/// within [`PROJECTILE_PICK`] units or [`PROJECTILE_CONE`] of it, that `rules` allow.
pub fn projectile_under(all: &[crate::combat::VatsProjectile], rules: &ProjectileRules, from: [f32; 3], dir: [f32; 3], until: f32) -> Option<crate::combat::VatsProjectile> {
    let mut best: Option<(f32, crate::combat::VatsProjectile)> = None;
    for p in all.iter().filter(|p| rules.allows(p)) {
        let d = [p.at[0] - from[0], p.at[1] - from[1], p.at[2] - from[2]];
        let t = d[0] * dir[0] + d[1] * dir[1] + d[2] * dir[2];
        if t <= 0.0 || t > until {
            continue;
        }
        let off = ((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]) - t * t).max(0.0).sqrt();
        if off > PROJECTILE_PICK.max(t * PROJECTILE_CONE) {
            continue;
        }
        let angle = off / t;
        if best.is_none_or(|(a, _)| angle < a) {
            best = Some((angle, *p));
        }
    }
    best.map(|(_, p)| p)
}

/// Mark the projectile `p` in flight (the player's tweak): the whole of it, its chance by
/// the distance and the player's skill only, as small as a held weapon [design:
/// docs/design/tweaks.md; the readme's rule, "only the distance + player skill"].
fn mark_projectile(v: &mut VatsMode, p: crate::combat::VatsProjectile, script: Option<&mut ScriptState>, sim: &mut SimState, player: &Player, hud: &mut HudMessage, sounds: &mut Sounds) {
    let ap_now = ap_to_mark(script, sim);
    let earlier = v.spots.iter().filter(|s| s.projectile == Some(p.id)).count() as u32;
    match v.queue.push(FormId(0), None, ap_now) {
        Ok(k) => {
            let name = if p.mine { "mine" } else if p.explosive { "explosive" } else { "projectile" }.to_string();
            let mut spot = Spot { point: p.at, name, part: "projectile".into(), base: WHOLE_TO_HIT, size: SIZE_HELD_WEAPON, earlier, projectile: Some(p.id), ..Default::default() };
            if let Some(w) = v.weapon.as_ref() {
                spot.chance = chance_of(&spot, w, player.eye(), None);
            }
            v.stats.marks += 1;
            info!("vats mode: marked projectile {} ({}), mark {k}; {:.0} AP reserved; {:.0}% to hit; {:.0} units", p.id, spot.name, v.queue.reserved(), spot.chance, crate::combat::distance(player.eye(), p.at));
            v.spots.push(spot);
            sounds.play(v, "UIVATSSelectTarget");
        }
        Err(r) => {
            v.stats.refused += 1;
            info!("vats mode: mark refused: {} ({} marks, {:.0} AP reserved)", r.message(), v.queue.len(), v.queue.reserved());
            hud.show(r.message().to_string());
            sounds.play(v, "UIVATSInsufficientAP");
        }
    }
}

/// How often a missed shot's point is drawn before it is taken as it is ([`shoot`]).
const MISS_DRAWS: usize = 8;

/// Fire round `round` of mark `i` (§9.1, §16.1, §16.3): the view snaps to it,
/// `crate::combat` fires at it this frame (a throw on its arc); the mark's first round
/// spends its AP.
#[allow(clippy::too_many_arguments)]
fn shoot(v: &mut VatsMode, i: usize, round: u32, mut script: Option<&mut ScriptState>, sim: &mut SimState, phys: &mut PhysState, player: &mut Player, control: &PlayerControl, sounds: &mut Sounds) {
    let Some((point, chance)) = v.spots.get(i).map(|s| (s.point, s.chance)) else { return };
    face(player, control, point);
    // Every round is rolled on the mark's chance now (§17.4); a miss goes wide of it.
    let (rolled, hit) = v.roll(chance);
    let thrown = v.weapon.as_ref().is_some_and(|w| w.style == AttackStyle::Thrown);
    let aim = if hit {
        point
    } else if thrown {
        let (u1, u2) = v.two();
        throw_miss_point(point, u1, u2)
    } else {
        // A miss misses: the wide point is drawn again while the line through it still
        // meets the marked actor, at most [`MISS_DRAWS`] times. Wide of a dog's torso
        // is still its legs [observed: smoke `vats-mode-miss`, a forced miss on the
        // torso that hit "Right Legs" for 14.9].
        let (eye, target) = (player.eye(), v.queue.marks().get(i).map(|m| m.target.0));
        let filter = shot_filter(phys);
        let mut aim = point;
        for _ in 0..MISS_DRAWS {
            let (u1, u2) = v.two();
            aim = miss_point(eye, point, u1, u2);
            let d = [aim[0] - eye[0], aim[1] - eye[1], aim[2] - eye[2]];
            let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-3);
            let met = phys.ray(eye, [d[0] / len, d[1] / len, d[2] / len], REACH, filter.clone());
            if !met.is_some_and(|h| matches!(h.owner, Owner::Actor { form_id, .. } if Some(form_id) == target)) {
                break;
            }
        }
        aim
    };
    if let Some(s) = v.spots.get_mut(i) {
        s.landed = Some(hit);
    }
    // A projectile marked (the player's tweak) ends where it is when the shot lands
    // [design: docs/design/tweaks.md].
    if let (true, Some(id), Some(w)) = (hit, v.spots.get(i).and_then(|s| s.projectile), v.weapon.as_ref().map(|w| w.id)) {
        v.downed.push((id, w));
    }
    let outcome = format!("{chance:.0}% to hit: {} (rolled {rolled:.0})", if hit { "hit" } else { "missed" });
    v.shot = Some(aim);
    v.throw_held = thrown.then_some(v.now);
    v.shooting = Some(i);
    v.fired += 1;
    v.stats.shots += 1;
    // The sights rule looks at this mark until its burst is out, then at the next.
    let rounds = match v.weapon.as_ref().map(|w| w.style) {
        Some(AttackStyle::Burst { shots, .. }) => shots,
        _ => 1,
    };
    v.aim_mark = if round + 1 < rounds { i } else { (i + 1).min(v.spots.len().saturating_sub(1)) };
    if round > 0 {
        debug!("vats mode: round {} of mark {}: {outcome}", round + 1, i + 1);
        return;
    }
    let cost = start_mark(v, i, script.as_deref_mut(), sim);
    call_helper(v, i, script, sim, phys, player, sounds);
    let (Some(m), Some(s)) = (v.queue.marks().get(i), v.spots.get(i)) else { return };
    let n = v.queue.len();
    match v.weapon.as_ref().map(|w| w.style) {
        Some(AttackStyle::Burst { shots, .. }) => info!("vats mode: burst {} of {n} at {} {} ({}), {shots} shots, {cost:.0} AP spent; round 1 {outcome}", i + 1, s.name, m.target, s.part),
        Some(AttackStyle::Thrown) => info!("vats mode: throw {} of {n} at {} {} ({}), {cost:.0} AP spent; {outcome}", i + 1, s.name, m.target, s.part),
        _ => info!("vats mode: shot {} of {n} at {} {} ({}), {cost:.0} AP spent; {outcome}", i + 1, s.name, m.target, s.part),
    }
}

/// The view snaps to `point`.
fn face(player: &mut Player, control: &PlayerControl, point: [f32; 3]) {
    let eye = player.eye();
    (player.heading, player.pitch) = Crosshair::aim_at(control.crosshair.as_ref(), eye, point);
}

/// Mark `i`'s attack starts: its AP is spent (§7.2). The AP spent.
fn start_mark(v: &mut VatsMode, i: usize, script: Option<&mut ScriptState>, sim: &mut SimState) -> f32 {
    let Some(m) = v.queue.marks().get(i).copied() else { return 0.0 };
    v.started = v.started.max(i + 1);
    let Some(rt) = script.and_then(|sc| sc.rt.as_mut()) else { return 0.0 };
    let left = with_world(rt, sim, |rt| {
        fnv_game::functions::add_actor_value_damage(&mut rt.game, FormId::PLAYER_REF, ACTION_POINTS, m.cost as f64);
        fnv_game::functions::actor_value(&mut rt.game, FormId::PLAYER_REF, ACTION_POINTS).unwrap_or(0.0)
    });
    // God mode: the game takes none (`ap_to_mark`); the cost is still counted as the
    // attack's in the lines and the summary.
    if crate::combat::god_mode() {
        info!("vats mode: god mode: attack {} takes none of the player's Action Points: {left:.0} left", i + 1);
    }
    v.spent += m.cost;
    v.stats.ap_spent += m.cost;
    m.cost
}

/// A melee sequence's frame (§16.2): the player is put beside the mark, swings at it, and
/// the swing is waited for.
#[allow(clippy::too_many_arguments)]
fn melee_step(v: &mut VatsMode, dt: f32, combat: &Combat, mut script: Option<&mut ScriptState>, sim: &mut SimState, phys: &mut PhysState, player: &mut Player, control: &PlayerControl, sounds: &mut Sounds) {
    let Some(Run::Melee(run)) = v.run.as_mut() else { return };
    let i = run.current.min(run.marks.saturating_sub(1));
    let eye = player.eye();
    let across = v.spots.get(i).map_or(f32::INFINITY, |s| ((s.point[0] - eye[0]).powi(2) + (s.point[1] - eye[1]).powi(2)).sqrt());
    let close = across <= v.weapon.as_ref().map_or(0.0, |w| w.melee_reach) + CLOSE_SLACK;
    let action = run.step(dt, close, combat.player_swinging());
    match action {
        Some(MeleeAction::Approach(i)) => {
            let Some(s) = v.spots.get(i) else { return };
            let p = s.point;
            // Once a mark: the player is put on the ground between where they stand and
            // the mark, `stand_off` from it across the ground (owner 2026-10-20: "a
            // teleport to target and attack").
            if v.closing != Some(i) {
                v.closing = Some(i);
                let reach = v.weapon.as_ref().map_or(0.0, |w| w.melee_reach);
                let feet = stand_beside(player.feet, p, stand_off(reach));
                phys.place_player(feet, TELEPORT_UP, TELEPORT_DOWN);
                player.feet = phys.prev_feet;
                info!("vats mode: teleported to {} {} ({}), {across:.0} units away, now {:.0}", s.name, v.queue.marks()[i].target, s.part, stand_off(reach));
            }
            face(player, control, p);
        }
        Some(MeleeAction::Swing(i)) => {
            let Some((p, chance)) = v.spots.get(i).map(|s| (s.point, s.chance)) else { return };
            face(player, control, p);
            let (rolled, hit) = v.roll(chance);
            v.blow_lands = hit;
            if let Some(s) = v.spots.get_mut(i) {
                s.landed = Some(hit);
            }
            v.swing_now = true;
            v.striking = Some(i);
            v.shooting = Some(i);
            v.fired += 1;
            v.stats.shots += 1;
            let cost = start_mark(v, i, script.as_deref_mut(), sim);
            call_helper(v, i, script, sim, phys, player, sounds);
            let (m, s) = (v.queue.marks()[i], &v.spots[i]);
            info!(
                "vats mode: swing {} of {} at {} {} ({}), {across:.0} units across, {cost:.0} AP spent; {chance:.0}% to hit: {} (rolled {rolled:.0})",
                i + 1,
                v.queue.len(),
                s.name,
                m.target,
                s.part,
                if hit { "hit" } else { "missed" }
            );
        }
        // The view stays on the mark being swung at.
        Some(MeleeAction::Wait(i)) => {
            if let Some(p) = v.spots.get(i).map(|s| s.point).filter(|_| v.striking == Some(i)) {
                face(player, control, p);
            }
        }
        Some(MeleeAction::Done) => end(v, "all shots fired", sounds),
        None => {}
    }
}

impl VatsMode {
    /// The last mark's shot, once every mark's attack has started (a kill camera watches
    /// its actor, `crate::kill_cam`, §19): as the scripts see a shot, with the weapon VATS
    /// mode was entered with, and its point now.
    pub fn final_shot(&self, player: &Player) -> Option<(fnv_game::vats::VatsShot, [f32; 3])> {
        let eye = player.eye();
        let n = self.queue.len();
        if self.phase != Phase::Firing || n == 0 || self.started < n {
            return None;
        }
        let (m, s) = (self.queue.marks().get(n - 1)?, self.spots.get(n - 1)?);
        let d = ((s.point[0] - eye[0]).powi(2) + (s.point[1] - eye[1]).powi(2) + (s.point[2] - eye[2]).powi(2)).sqrt();
        let shot = fnv_game::vats::VatsShot { weapon: self.weapon.as_ref()?.id, target: m.target, distance: d as f64, condition: s.condition, success: s.landed.unwrap_or(false), height: (s.point[2] - player.feet[2]) as f64 };
        Some((shot, s.point))
    }
}

/// The test aid `NVF_PLAY_TEST_VATS_HELPER`: `stranger` or `fortune` brings that helper
/// for every mark whose odds for them are above 0 (the player has the perk), `none` none
/// (vats-mode.md §13).
fn forced_helper() -> Option<Option<Helper>> {
    static FORCED: std::sync::OnceLock<Option<Option<Helper>>> = std::sync::OnceLock::new();
    *FORCED.get_or_init(|| match std::env::var("NVF_PLAY_TEST_VATS_HELPER").ok()?.trim() {
        "stranger" => Some(Some(Helper::Stranger)),
        "fortune" => Some(Some(Helper::Fortune)),
        "none" => Some(None),
        other => {
            warn!("vats mode: NVF_PLAY_TEST_VATS_HELPER={other}: not stranger, fortune or none; helpers are rolled");
            None
        }
    })
}

/// Where a helper stands for a mark at `point` (§18.2): `away` from the player's feet across
/// the line to the mark, on its right, else its left, else behind the player; the first
/// spot from whose eye nothing but the mark's actor stands in the way. `None` when all are
/// blocked.
fn helper_spot(phys: &mut PhysState, feet: [f32; 3], point: [f32; 3], target: u32, away: f32) -> Option<[f32; 3]> {
    let (dx, dy) = (point[0] - feet[0], point[1] - feet[1]);
    let flat = (dx * dx + dy * dy).sqrt().max(1e-3);
    let (fx, fy) = (dx / flat, dy / flat);
    let spots = [[feet[0] + fy * away, feet[1] - fx * away, feet[2]], [feet[0] - fy * away, feet[1] + fx * away, feet[2]], [feet[0] - fx * away, feet[1] - fy * away, feet[2]]];
    spots.into_iter().find(|&at| {
        let eye = [at[0], at[1], at[2] + HELPER_EYE];
        let to = [point[0] - eye[0], point[1] - eye[1], point[2] - eye[2]];
        let len = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt();
        if len < 1.0 {
            return false;
        }
        let dir = [to[0] / len, to[1] / len, to[2] / len];
        let mut filter = Filter { exclude_layers: layers::shot_ignores(), ..Default::default() };
        filter.exclude.extend(phys.npcs.values().map(|b| b.ch.key()));
        match phys.ray(eye, dir, len + 30.0, filter) {
            None => true,
            Some(h) => matches!(h.owner, Owner::Actor { form_id, .. } if form_id == target) || h.distance >= len - 30.0,
        }
    })
}

/// Mark `i`'s attack started: with Mysterious Stranger or Miss Fortune, one of them may come
/// for it (§18.1, §18.2). One helper at a time.
#[allow(clippy::too_many_arguments)]
fn call_helper(v: &mut VatsMode, i: usize, script: Option<&mut ScriptState>, sim: &mut SimState, phys: &mut PhysState, player: &Player, sounds: &mut Sounds) {
    if v.helper.is_some() {
        return;
    }
    let Some(rt) = script.and_then(|s| s.rt.as_mut()) else { return };
    let (Some(m), Some(point)) = (v.queue.marks().get(i).copied(), v.spots.get(i).map(|s| s.point)) else { return };
    let odds = with_world(rt, sim, |rt| rt.game.vats_helper_odds(m.target));
    if odds.stranger + odds.fortune <= 0.0 {
        return;
    }
    let u = v.two().0 as f64;
    let who = match forced_helper() {
        Some(Some(Helper::Stranger)) => (odds.stranger > 0.0).then_some(Helper::Stranger),
        Some(Some(Helper::Fortune)) => (odds.fortune > 0.0).then_some(Helper::Fortune),
        Some(None) => None,
        None => pick_helper(odds, u),
    };
    let Some(who) = who else {
        debug!("vats mode: no helper for mark {} ({:.0}% the Stranger, {:.0}% Miss Fortune, rolled {:.0})", i + 1, odds.stranger * 100.0, odds.fortune * 100.0, u * 100.0);
        return;
    };
    let Some(feet) = helper_spot(phys, player.feet, point, m.target.0, odds.distance as f32) else {
        info!("vats mode: {} would come for mark {}, but nowhere beside the player sees it", who.label(), i + 1);
        return;
    };
    let heading = (point[0] - feet[0]).atan2(point[1] - feet[1]);
    let (reference, weapon) = with_world(rt, sim, |rt| {
        let g = &mut rt.game;
        (g.summon_vats_helper(who, feet, heading), g.data.by_edid(who.weapon_edid()))
    });
    let (Some(reference), Some(weapon)) = (reference, weapon) else {
        info!("vats mode: {} cannot come: the load order has no {} or {}", who.label(), who.edid(), who.weapon_edid());
        return;
    };
    info!(
        "vats mode: {} comes for mark {} ({}), {:.0} units from the player ({:.0}% odds, rolled {:.0})",
        who.label(),
        i + 1,
        m.target,
        odds.distance,
        if who == Helper::Stranger { odds.stranger } else { odds.fortune } * 100.0,
        u * 100.0
    );
    v.stats.helpers += 1;
    // The Stranger's theme [observed: FalloutNV.esm SOUN MUSMysteriousStrangerA01, A02;
    // hypothesis: played as he comes, one of the two].
    if who == Helper::Stranger {
        sounds.play(v, if u < odds.stranger / 2.0 { "MUSMysteriousStrangerA01" } else { "MUSMysteriousStrangerA02" });
    }
    v.helper = Some(ActiveHelper { who, reference, weapon, feet, target: m.target, part: m.part, point, age: 0.0, fired: false });
}

/// The helper's frame (§18.2): their shot at the mark after [`HELPER_DRAW`], their leaving
/// after [`HELPER_STAY`]. Runs whether the mode is on or not.
fn tick_helper(v: &mut VatsMode, dt: f32, script: Option<&mut ScriptState>, sim: &mut SimState, phys: &PhysState, sounds: &mut Sounds) {
    let Some(h) = v.helper.as_mut() else { return };
    h.age += dt.max(0.0);
    if !h.fired && h.age >= HELPER_DRAW {
        h.fired = true;
        let point = point_of(phys, h.target.0, h.part).unwrap_or(h.point);
        let origin = [h.feet[0], h.feet[1], h.feet[2] + HELPER_EYE];
        let to = [point[0] - origin[0], point[1] - origin[1], point[2] - origin[2]];
        let len = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt().max(1e-3);
        let aim = [to[0] / len, to[1] / len, to[2] / len];
        sounds.fire.write(ActorFire { shooter: h.reference, weapon: h.weapon, origin, aim, cone: 0.0, muzzle: None, harmless: false, sure: None });
        info!("vats mode: {} fires at {} from {len:.0} units", h.who.label(), h.target);
    }
    if h.age >= HELPER_STAY {
        let (r, who) = (h.reference, h.who);
        v.helper = None;
        if let Some(rt) = script.and_then(|s| s.rt.as_mut()) {
            with_world(rt, sim, |rt| rt.game.dismiss_vats_helper(r));
        }
        info!("vats mode: {} leaves", who.label());
    }
}

/// After a sequence that killed (the player's kills rose since it began to be fired), the
/// Player Kill AP Reward perks give AP back (Grim Reaper's Sprint, §18.3).
fn kill_reward(v: &mut VatsMode, before: f64, script: Option<&mut ScriptState>, sim: &mut SimState) {
    let Some(rt) = script.and_then(|s| s.rt.as_mut()) else { return };
    let given = with_world(rt, sim, |rt| {
        let g = &mut rt.game;
        let kills = g.player_kill_count() - before;
        if kills < 1.0 {
            return None;
        }
        let reward = g.vats_kill_ap_reward();
        if reward <= 0.0 {
            return None;
        }
        let [_, _, damage] = fnv_game::functions::actor_value_parts(g, FormId::PLAYER_REF, ACTION_POINTS).ok()?;
        let back = reward.min(damage.max(0.0));
        fnv_game::functions::restore_actor_value_damage(g, FormId::PLAYER_REF, ACTION_POINTS, back);
        Some((kills, reward, back))
    });
    if let Some((kills, reward, back)) = given {
        v.stats.ap_rewarded += back as f32;
        info!("vats mode: {kills:.0} killed: the Player Kill AP Reward perks give {reward:.0} AP back ({back:.0} restored)");
    }
}

/// GetVATSMode and GetVATSValue's view of the mode (§12).
fn tell_scripts(v: &VatsMode, script: Option<&mut ScriptState>, player: &Player, damage_mult: f32) {
    let Some(rt) = script.and_then(|s| s.rt.as_mut()) else { return };
    use fnv_game::vats::{VatsShot, MODE_OFF, MODE_PLAYBACK, MODE_TARGET_SELECT};
    let mode = match v.phase {
        Phase::Marking => MODE_TARGET_SELECT,
        Phase::Firing => MODE_PLAYBACK,
        _ => MODE_OFF,
    };
    let shot = v.shooting.filter(|_| mode == MODE_PLAYBACK).and_then(|i| {
        let (m, s) = (v.queue.marks().get(i)?, v.spots.get(i)?);
        let eye = player.eye();
        let d = ((s.point[0] - eye[0]).powi(2) + (s.point[1] - eye[1]).powi(2) + (s.point[2] - eye[2]).powi(2)).sqrt();
        Some(VatsShot { weapon: v.weapon.as_ref()?.id, target: m.target, distance: d as f64, condition: s.condition, success: s.landed.unwrap_or(false), height: (s.point[2] - player.feet[2]) as f64 })
    });
    if rt.game.vats.mode != mode || rt.game.vats.shot != shot {
        rt.game.set_vats(mode, shot);
    }
    // The classic style's playback: the player takes a share of a hit (vats-classic.md §7).
    let share = (v.style == VatsStyle::Classic && v.phase == Phase::Firing).then_some(damage_mult);
    if rt.game.vats.player_damage_mult != share {
        match share {
            Some(m) => info!("vats mode: classic: while the attacks play the player takes {:.0}% of a hit (fVATSPlayerDamageMult)", m * 100.0),
            None => debug!("vats mode: classic: the player takes whole hits again"),
        }
        rt.game.vats.player_damage_mult = share;
    }
}

/// A mark's "x" on the screen: two crossed bars.
#[derive(Component)]
struct MarkCross(usize);

fn spawn_marks(mut commands: Commands) {
    for i in 0..MAX_PREVIEW {
        commands.spawn((Node { position_type: PositionType::Absolute, ..default() }, Visibility::Hidden, GlobalZIndex(5), PreviewLabel { i, shown: String::new(), width: 0.0 }));
    }
    // The classic style's bars: one a part's label, each a dark ground and its filled share.
    for i in 0..MAX_PREVIEW {
        for fill in [false, true] {
            let colour = if fill { LABEL_COLOUR } else { BAR_GROUND };
            commands.spawn((Node { position_type: PositionType::Absolute, ..default() }, BackgroundColor(colour), Visibility::Hidden, GlobalZIndex(if fill { 5 } else { 4 }), PreviewBar { i, fill }));
        }
    }
    commands.spawn((Node { position_type: PositionType::Absolute, ..default() }, Visibility::Hidden, GlobalZIndex(5), ClassicHeader { shown: String::new(), width: 0.0 }));
    for i in 0..MAX_DRAWN {
        commands.spawn((Node { position_type: PositionType::Absolute, ..default() }, Visibility::Hidden, GlobalZIndex(5), MarkLabel { i, shown: String::new() }));
    }
    for i in 0..MAX_DRAWN {
        commands
            .spawn((Node { position_type: PositionType::Absolute, ..default() }, Visibility::Hidden, GlobalZIndex(5), MarkCross(i)))
            .with_children(|p| {
                for deg in [45.0f32, -45.0] {
                    p.spawn((
                        Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(MARK_BAR), top: Val::Percent((100.0 - MARK_BAR) / 2.0), ..default() },
                        BackgroundColor(MARK_COLOUR),
                        UiTransform::from_rotation(Rot2::degrees(deg)),
                    ));
                }
            });
    }
}

/// Put each mark's "x" where its point is on the screen (§8).
fn draw_marks(
    vats: Res<VatsMode>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<PlayerCamera>>,
    mut crosses: Query<(&MarkCross, &mut Node, &mut Visibility)>,
) {
    let width = windows.iter().next().map_or(1280.0, |w| w.width());
    // At the HUD's scale (`crate::interface::ui_scale`, #1631), as the labels are.
    let size = (MARK_SIZE * width * crate::interface::ui_scale()).max(6.0);
    let camera = cameras.single().ok();
    let spots: Vec<[f32; 3]> = vats.drawn().into_iter().map(|(p, _)| p).collect();
    for (c, mut node, mut vis) in &mut crosses {
        let at = spots.get(c.0).zip(camera).and_then(|(p, (cam, t))| cam.world_to_viewport(t, space::to_bevy(*p)).ok());
        let want = if at.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        if let Some(at) = at {
            node.left = Val::Px(at.x - size / 2.0);
            node.top = Val::Px(at.y - size / 2.0);
            node.width = Val::Px(size);
            node.height = Val::Px(size);
        }
    }
}

/// A mark's chance to hit, "NN%", beside its "x" (§17.5); `shown` is the text its glyphs
/// now spell.
#[derive(Component)]
struct MarkLabel {
    i: usize,
    shown: String,
}

/// The labels' font: the HUD's (`crate::hud`), and where the line stands in it.
const LABEL_FONT: &str = "textures\\fonts\\baked-in_monofonto_large.fnt";
/// The label's baseline below its top, interface units (960 to the view's height).
const LABEL_BASE: f32 = 30.0;
/// Its height, interface units, for centring it on the "x".
const LABEL_HEIGHT: f32 = 36.0;
/// The marks' brightened amber [design: the owner's wishes, 2026-10-24; it was a creamy
/// white before].
const LABEL_COLOUR: Color = MARK_COLOUR;

/// The text of a chance: whole percent.
pub fn chance_text(chance: f32) -> String {
    format!("{:.0}%", chance.clamp(0.0, 100.0))
}

/// Put each mark's chance beside its "x" (§17.5): to its right, centred on it.
#[allow(clippy::too_many_arguments)]
fn draw_labels(
    mut commands: Commands,
    vats: Res<VatsMode>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<PlayerCamera>>,
    data: Option<Res<GameData>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    mut face_: Local<Option<Face>>,
    mut labels: Query<(Entity, &mut MarkLabel, &mut Node, &mut Visibility)>,
) {
    // The HUD's lengths at the UI scale (`crate::interface::hud_units`, #1631): the
    // letters are set in them, and the places here follow.
    let _hud = crate::interface::hud_units();
    let scale = crate::interface::ui_scale();
    let (width, height) = windows.iter().next().map_or((1280.0, 720.0), |w| (w.width(), w.height()));
    let size = (MARK_SIZE * width * scale).max(6.0);
    let tall = LABEL_HEIGHT / 960.0 * height * scale;
    let camera = cameras.single().ok();
    let spots = vats.drawn();
    for (e, mut label, mut node, mut vis) in &mut labels {
        let spot = spots.get(label.i);
        let at = spot.zip(camera).and_then(|((p, _), (cam, t))| cam.world_to_viewport(t, space::to_bevy(*p)).ok());
        let want = if at.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        let (Some(at), Some((_, chance))) = (at, spot) else { continue };
        node.left = Val::Px(at.x + size * 0.75);
        node.top = Val::Px(at.y - tall / 2.0);
        let text = chance_text(*chance);
        if label.shown != text {
            let f = face_.get_or_insert_with(|| font_face(LABEL_FONT, plain_font(36.0, 15.0), data.as_deref(), &mut images, bc_supported(formats.as_deref())));
            commands.entity(e).despawn_related::<Children>().with_children(|p| {
                if let Some(line) = f.font.lines(&text, None).into_iter().next() {
                    set_line(p, f, &line, 0.0, LABEL_BASE, LABEL_COLOUR);
                }
            });
            label.shown = text;
        }
    }
}

/// A bar of the classic style's screen (vats-classic.md §4): under part label `i` the
/// part's condition; the ground or the filled share. (The target's health is the HUD's
/// enemy bar, `crate::hud`.)
#[derive(Component)]
struct PreviewBar {
    i: usize,
    fill: bool,
}

/// The bars' ground [design: dark, so the amber share reads on any picture].
const BAR_GROUND: Color = Color::srgba(0.0, 0.0, 0.0, 0.55);
/// A bar's height and the least width of a part's bar, interface units (960 to the view's
/// height) [design].
const BAR_HEIGHT: f32 = 5.0;
const BAR_MIN_WIDTH: f32 = 60.0;
/// The header: its top from the view's top, and its bar's width, interface units [design:
/// the original names the target and shows its health at the top of the view].
const HEADER_TOP: f32 = 150.0;

/// The classic style's header: the selected target's name; `shown` and `width` as a
/// [`PreviewLabel`]'s.
#[derive(Component)]
struct ClassicHeader {
    shown: String,
    width: f32,
}

/// A previewed part's chance (§17.5): `shown` is the text its glyphs now spell, `width`
/// that text's width in interface units.
#[derive(Component)]
struct PreviewLabel {
    i: usize,
    shown: String,
    width: f32,
}

/// The part under the crosshair's preview: the amber lifted this far towards white,
/// brighter than the marks' [design].
const PREVIEW_UNDER_LIFT: f32 = 0.75;
const PREVIEW_UNDER_COLOUR: Color = Color::srgb(
    crate::interface::AMBER[0] + (1.0 - crate::interface::AMBER[0]) * PREVIEW_UNDER_LIFT,
    crate::interface::AMBER[1] + (1.0 - crate::interface::AMBER[1]) * PREVIEW_UNDER_LIFT,
    crate::interface::AMBER[2] + (1.0 - crate::interface::AMBER[2]) * PREVIEW_UNDER_LIFT,
);

impl VatsMode {
    /// The preview labels to draw (§17.5), while marking only: each shown part's point,
    /// its text and whether the crosshair is on it.
    fn previewed(&self) -> impl Iterator<Item = (&PreviewPart, [f32; 3])> {
        let on = self.phase == Phase::Marking;
        self.preview.parts.iter().filter(move |p| on && p.shown).map(|p| (p, p.spot.point))
    }
}

/// A previewed part's label (§17.5): its name as the body part data spells it and its
/// chance, "Head 41%"; the whole actor's chance alone; the part under the crosshair in
/// brackets, "[Head 41%]" [design: NVForever's own wording; the name was in small letters
/// until the fork owner's playtest of 2026-11-10, "left legs" for "Left Legs"].
pub fn preview_text(out: &mut String, part: &str, chance: f32, under: bool) {
    use std::fmt::Write;
    out.clear();
    if under {
        out.push('[');
    }
    match part {
        "whole" => {}
        "its weapon" => out.push_str("Weapon "),
        _ => {
            out.push_str(part);
            out.push(' ');
        }
    }
    let _ = write!(out, "{:.0}%", chance.clamp(0.0, 100.0));
    if under {
        out.push(']');
    }
}

/// Put each previewed part's chance on its point (§17.5): centred on it, a label that
/// would overlap one placed before it moved down a line, so a far actor's labels stay
/// apart.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw_previews(
    mut commands: Commands,
    vats: Res<VatsMode>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<PlayerCamera>>,
    data: Option<Res<GameData>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    mut face_: Local<Option<Face>>,
    mut text: Local<String>,
    mut placed: Local<Vec<[f32; 4]>>,
    mut labels: Query<(Entity, &mut PreviewLabel, &mut Node, &mut Visibility)>,
    mut bars: Query<(&PreviewBar, &mut Node, &mut Visibility), (Without<PreviewLabel>, Without<ClassicHeader>)>,
    mut header: Query<(Entity, &mut ClassicHeader, &mut Node, &mut Visibility), (Without<PreviewLabel>, Without<PreviewBar>)>,
    tiles: Option<Res<crate::tiles::vats_menu::VatsTiles>>,
) {
    // The parts' labels and bars are the game's menu file's while it draws them
    // (`crate::tiles::vats_menu`, vats-classic.md §11); the header stays this drawing's.
    let from_file = tiles.is_some_and(|t| t.drawn);
    // Each bar's place and share this frame: left, top, width, share.
    let mut bar_at: [Option<[f32; 4]>; MAX_PREVIEW] = [None; MAX_PREVIEW];
    // The HUD's lengths at the UI scale (`crate::interface::hud_units`, #1631): the
    // letters are set in them, and an interface unit here is as many pixels.
    let _hud = crate::interface::hud_units();
    let (width, height) = windows.iter().next().map_or((1280.0, 720.0), |w| (w.width(), w.height()));
    let px = height / 960.0 * crate::interface::ui_scale();
    let tall = LABEL_HEIGHT * px;
    let camera = cameras.single().ok();
    placed.clear();
    let mut shown = vats.previewed().filter(|_| !from_file);
    // The labels in their order (the query's is not theirs).
    let mut order: [Option<Entity>; MAX_PREVIEW] = [None; MAX_PREVIEW];
    for (e, label, _, _) in &labels {
        if let Some(slot) = order.get_mut(label.i) {
            *slot = Some(e);
        }
    }
    for e in order.into_iter().flatten() {
        let Ok((e, mut label, mut node, mut vis)) = labels.get_mut(e) else { continue };
        let next = shown.next();
        let at = next.zip(camera).and_then(|((_, p), (cam, t))| cam.world_to_viewport(t, space::to_bevy(p)).ok());
        let want = if at.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        let (Some(at), Some((p, _))) = (at, next) else { continue };
        preview_text(&mut text, &p.spot.part, p.spot.chance, p.under);
        if p.queued > 0 {
            use std::fmt::Write;
            let _ = write!(text, " x{}", p.queued);
        }
        if label.shown != *text {
            let f = face_.get_or_insert_with(|| font_face(LABEL_FONT, plain_font(36.0, 15.0), data.as_deref(), &mut images, bc_supported(formats.as_deref())));
            let colour = if p.under { PREVIEW_UNDER_COLOUR } else { LABEL_COLOUR };
            let line = f.font.lines(&text, None).into_iter().next();
            label.width = line.as_ref().map_or(0.0, |l| l.width);
            commands.entity(e).despawn_related::<Children>().with_children(|c| {
                if let Some(line) = &line {
                    set_line(c, f, line, 0.0, LABEL_BASE, colour);
                }
            });
            label.shown.clone_from(&text);
        }
        let w = label.width * px;
        let (x, mut y) = (at.x - w / 2.0, at.y - tall / 2.0);
        while placed.iter().any(|r| x < r[0] + r[2] && r[0] < x + w && y < r[1] + r[3] && r[1] < y + tall) {
            y += tall;
        }
        placed.push([x, y, w, tall]);
        node.left = Val::Px(x);
        node.top = Val::Px(y);
        if let Some(share) = p.condition {
            let bw = w.max(BAR_MIN_WIDTH * px);
            bar_at[label.i] = Some([at.x - bw / 2.0, y + tall * 0.92, bw, share]);
        }
    }
    drop(shown);
    // [design, #2659] The target name and health belong to the HUD's bottom enemy bar
    // (`crate::hud`, `crate::hud_target`). The fallback header keeps the existing
    // special-attack hint near the top when the file's buttons are unavailable.
    let head = vats.classic_view().and_then(|v| v.special).filter(|_| !from_file).map(|special| format!("[{}] {special}", crate::bindings::label(Control::ReadyItem)));
    for (e, mut h, mut node, mut vis) in &mut header {
        let want = if head.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        let Some(ref name) = head else { continue };
        if h.shown != *name {
            let f = face_.get_or_insert_with(|| font_face(LABEL_FONT, plain_font(36.0, 15.0), data.as_deref(), &mut images, bc_supported(formats.as_deref())));
            let line = f.font.lines(name, None).into_iter().next();
            h.width = line.as_ref().map_or(0.0, |l| l.width);
            commands.entity(e).despawn_related::<Children>().with_children(|c| {
                if let Some(line) = &line {
                    set_line(c, f, line, 0.0, LABEL_BASE, LABEL_COLOUR);
                }
            });
            h.shown.clone_from(name);
        }
        let top = HEADER_TOP * px;
        node.left = Val::Px(width / 2.0 - h.width * px / 2.0);
        node.top = Val::Px(top);
    }
    for (bar, mut node, mut vis) in &mut bars {
        let at = bar_at.get(bar.i).copied().flatten();
        let want = if at.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        let Some([x, y, w, share]) = at else { continue };
        let tall = (BAR_HEIGHT * px).max(2.0);
        node.left = Val::Px(x);
        node.top = Val::Px(y);
        node.width = Val::Px(if bar.fill { w * share.clamp(0.0, 1.0) } else { w });
        node.height = Val::Px(tall);
    }
}

fn report(mut exit: MessageReader<AppExit>, vats: Res<VatsMode>) {
    if exit.read().next().is_none() {
        return;
    }
    let s = vats.stats;
    if s.entered == 0 && s.refused == 0 {
        return;
    }
    info!(
        "vats mode summary: entered {} times, {} marks, {} shots, {:.0} AP spent, {} refused, {} hit, {} missed, {} helpers, {:.0} AP rewarded",
        s.entered, s.marks, s.shots, s.ap_spent, s.refused, s.hits, s.misses, s.helpers, s.ap_rewarded
    );
}

/// VATS mode's frame (`update`), for systems that read what it decided (`crate::kill_cam`).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct VatsUpdate;

pub struct VatsModePlugin;

impl Plugin for VatsModePlugin {
    fn build(&self, app: &mut App) {
        classic::register(app);
        touch::install(app);
        app.init_resource::<VatsMode>()
            .add_systems(Startup, spawn_marks)
            .add_systems(Update, read_keys.after(PlaySet::Input).after(crate::control::enforce_locks).after(crate::player::encumbered_walks).before(PlaySet::Physics))
            .add_systems(Update, update.in_set(PlaySet::Activation).in_set(VatsUpdate).before(crate::combat::fire))
            .add_systems(Update, (draw_marks, draw_labels, draw_previews.after(crate::tiles::vats_menu::write_system)).after(PlaySet::Activation))
            .add_systems(Last, report);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_projectile_near_the_crosshair_line_is_marked_by_the_tweaks() {
        use crate::combat::VatsProjectile;
        let p = |id, at, by_player, explosive, mine| VatsProjectile { id, at, by_player, explosive, mine };
        let all = [p(1, [500.0, 10.0, 0.0], false, true, false), p(2, [400.0, 60.0, 0.0], false, true, false), p(3, [300.0, 2.0, 0.0], true, true, false), p(4, [200.0, 1.0, 0.0], false, false, false), p(5, [100.0, 0.0, 0.0], false, true, true)];
        let on = ProjectileRules { on: true, ..Default::default() };
        let pick = |r: &ProjectileRules, until| projectile_under(&all, r, [0.0; 3], [1.0, 0.0, 0.0], until).map(|p| p.id);
        // Nearest the line by angle; not the player's own, not a mine.
        assert_eq!(pick(&on, 1000.0), Some(4));
        assert_eq!(pick(&ProjectileRules { explosive_only: true, ..on }, 1000.0), Some(1));
        assert_eq!(pick(&ProjectileRules { own: true, explosive_only: true, ..on }, 1000.0), Some(3));
        assert_eq!(pick(&ProjectileRules { mines: true, ..on }, 1000.0), Some(5));
        // Not beyond what the ray met; 60 off at 400 is too far off.
        assert_eq!(pick(&ProjectileRules { explosive_only: true, ..on }, 450.0), None);
    }

    use super::*;

    #[test]
    fn the_head_is_the_part_with_perception_condition() {
        let part = |c: Option<u16>| PreviewPart { spot: Spot { condition: c, ..Default::default() }, ..Default::default() };
        let parts = [part(Some(26)), part(None), part(Some(fnv_game::combat::HEAD)), part(Some(27))];
        assert_eq!(head_part(&parts), Some(2));
        assert_eq!(head_part(&parts[..2]), None);
    }

    #[test]
    fn switching_with_the_right_stick_does_not_delay_a_vats_target_step() {
        use bevy::input::gamepad::{Gamepad, GamepadAxis};
        use crate::{bindings::{Bindings, InputDevice}, pad::device::tests::playing_app};
        for x in [-0.8, 0.8] {
            let (mut app, pad) = playing_app(InputDevice::KeyboardMouse);
            app.insert_resource(VatsMode { phase: Phase::Marking, style: VatsStyle::Classic, ..default() })
                .init_resource::<ControlLocks>().init_resource::<MoveIntent>()
                .insert_resource(Player { heading: 0.0, pitch: 0.0, eye_height: 0.0, feet: [0.0; 3], body_eye: None, body_view: None, placed: None })
                .add_message::<PlaySound>().add_message::<LoopSound>().add_message::<ActorFire>()
                .add_systems(Update, read_keys.after(crate::control::read_controls));
            app.update();
            app.world_mut().get_mut::<Gamepad>(pad).unwrap().analog_mut().set(GamepadAxis::RightStickX, x);
            app.update();
            assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller);
            assert_eq!(app.world().resource::<VatsMode>().keys.target_step, 0, "the switching edge is filtered");
            for _ in 0..2 {
                app.update();
                assert_eq!(app.world().resource::<VatsMode>().keys.target_step, 0, "the continued physical hold is not a new push");
            }
            app.world_mut().get_mut::<Gamepad>(pad).unwrap().analog_mut().set(GamepadAxis::RightStickX, 0.0);
            app.update();
            assert_eq!(app.world().resource::<VatsMode>().keys.target_step, 0);
            app.world_mut().get_mut::<Gamepad>(pad).unwrap().analog_mut().set(GamepadAxis::RightStickX, x);
            app.update();
            assert_eq!(app.world().resource::<VatsMode>().keys.target_step, if x > 0.0 { 1 } else { -1 });
            app.update();
            assert_eq!(app.world().resource::<VatsMode>().keys.target_step, 0);
        }
    }

    #[test]
    fn block_release_and_pad_press_in_one_frame_cannot_unmark_across_a_switch() {
        use crate::{bindings::{Bindings, InputDevice}, pad::device::tests::{button, control_button, playing_app}};
        let (mut app, pad) = playing_app(InputDevice::KeyboardMouse);
        app.insert_resource(VatsMode { phase: Phase::Marking, style: VatsStyle::DeadEye, ..default() })
            .init_resource::<ControlLocks>().init_resource::<MoveIntent>()
            .insert_resource(Player { heading: 0.0, pitch: 0.0, eye_height: 0.0, feet: [0.0; 3], body_eye: None, body_view: None, placed: None })
            .add_message::<PlaySound>().add_message::<LoopSound>().add_message::<ActorFire>()
            .add_systems(Update, read_keys.after(crate::control::read_controls));
        control_button(&mut app, pad, InputDevice::KeyboardMouse, Control::Block, true);
        app.update();
        assert!(app.world().resource::<VatsMode>().block_at.is_some());
        button(&mut app, pad, true);
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
        assert!(!app.world().resource::<VatsMode>().keys.unmark);
        button(&mut app, pad, false);
        app.update();
        control_button(&mut app, pad, InputDevice::KeyboardMouse, Control::Block, false);
        button(&mut app, pad, true);
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
        assert!(app.world().resource::<VatsMode>().keys.unmark, "the original device's release remains available");
        assert!(app.world().resource::<VatsMode>().block_at.is_none());
        button(&mut app, pad, false);
        app.update();
        button(&mut app, pad, true);
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller);
        assert!(!app.world().resource::<VatsMode>().keys.unmark);
    }

    /// `read_keys` after the Menu Mode key's hold (`crate::flashlight::menu_key`), as the
    /// game runs them, with the mode at `vats` (50 ms frames).
    fn menu_key_app(device: crate::bindings::InputDevice, vats: VatsMode) -> (App, Entity) {
        use crate::flashlight::{menu_key, Flashlight, MenuKeyHold};
        let (mut app, pad) = crate::pad::device::tests::playing_app(device);
        app.insert_resource(vats)
            .init_resource::<ControlLocks>().init_resource::<MoveIntent>()
            .init_resource::<Flashlight>().init_resource::<MenuKeyHold>()
            .insert_resource(Player { heading: 0.0, pitch: 0.0, eye_height: 0.0, feet: [0.0; 3], body_eye: None, body_view: None, placed: None })
            .add_message::<PlaySound>().add_message::<LoopSound>().add_message::<ActorFire>().add_message::<crate::menu::OpenInventory>()
            .add_systems(Update, (menu_key, read_keys).chain().after(crate::control::read_controls));
        app.update();
        (app, pad)
    }

    /// Menu Mode down for `frames` frames, then let go: the Pip-Boy opens asked for
    /// meanwhile, and whether the first frame asked the mode to unmark.
    fn menu_mode_press(app: &mut App, pad: Entity, device: crate::bindings::InputDevice, frames: usize) -> (usize, bool) {
        use crate::pad::device::tests::control_button;
        let opened = |app: &App| app.world().resource::<Messages<crate::menu::OpenInventory>>().iter_current_update_messages().count();
        let mut opens = 0;
        let mut unmark = false;
        control_button(app, pad, device, Control::MenuMode, true);
        for i in 0..frames {
            app.update();
            opens += opened(app);
            unmark |= i == 0 && app.world().resource::<VatsMode>().keys.unmark;
        }
        control_button(app, pad, device, Control::MenuMode, false);
        app.update();
        (opens + opened(app), unmark)
    }

    #[test]
    fn menu_mode_leaving_the_mode_opens_no_pip_boy_and_switches_no_light() {
        use crate::{bindings::InputDevice, flashlight::{Flashlight, HOLD_SECONDS}};
        // B on a controller, Tab on the keyboard (#2311): a tap, then a hold past the
        // light's time. Dead Eye leaves on either; the classic style on the keyboard (B on
        // the controller's classic screen is the screen's Return, the next test).
        let held = (HOLD_SECONDS / 0.05) as usize * 2;
        for (style, device) in [(VatsStyle::DeadEye, InputDevice::Controller), (VatsStyle::DeadEye, InputDevice::KeyboardMouse), (VatsStyle::Classic, InputDevice::KeyboardMouse)] {
            for frames in [1, held] {
                let (mut app, pad) = menu_key_app(device, VatsMode { phase: Phase::Marking, style, ..default() });
                let (opens, _) = menu_mode_press(&mut app, pad, device, frames);
                let case = format!("{style:?}, {device:?}, {frames} frames");
                assert_eq!(app.world().resource::<VatsMode>().phase, Phase::Off, "{case}: the press leaves");
                assert_eq!(opens, 0, "{case}: no Pip-Boy on the release");
                assert_eq!(app.world().resource::<Flashlight>().toggles, 0, "{case}: no light");
                // Out of the mode the key is the Pip-Boy's again: a tap opens it, a hold
                // switches the light.
                assert_eq!(menu_mode_press(&mut app, pad, device, 1).0, 1, "{case}: a second press opens it");
                assert_eq!(menu_mode_press(&mut app, pad, device, held).0, 0, "{case}");
                assert_eq!(app.world().resource::<Flashlight>().toggles, 1, "{case}: a hold out of the mode switches the light");
            }
        }
    }

    #[test]
    fn b_returning_on_the_controllers_classic_screen_opens_no_pip_boy() {
        use crate::{bindings::InputDevice, flashlight::{Flashlight, HOLD_SECONDS}};
        // B is both Menu Mode and the classic screen's Return: the press is the screen's
        // (it unmarks, or with nothing marked leaves, in `update`), not the Pip-Boy's.
        let device = InputDevice::Controller;
        for frames in [1, (HOLD_SECONDS / 0.05) as usize * 2] {
            let (mut app, pad) = menu_key_app(device, VatsMode { phase: Phase::Marking, style: VatsStyle::Classic, ..default() });
            let (opens, unmark) = menu_mode_press(&mut app, pad, device, frames);
            assert!(unmark, "{frames} frames: B is the screen's Return");
            assert_eq!(app.world().resource::<VatsMode>().phase, Phase::Marking, "{frames} frames: no game control leaves the screen");
            assert_eq!(opens, 0, "{frames} frames: no Pip-Boy on the release");
            assert_eq!(app.world().resource::<Flashlight>().toggles, 0, "{frames} frames: no light");
        }
    }

    #[test]
    fn the_view_is_not_turned_back_after_a_load_or_a_door() {
        let at = [100.0, 200.0, 30.0];
        assert!(restore_stands(false, at, at));
        assert!(restore_stands(false, [140.0, 200.0, 30.0], at), "a step aside is still there");
        assert!(!restore_stands(true, at, at), "behind a loading screen");
        assert!(!restore_stands(false, [100.0, 200.0 + 2.0 * RESTORE_REACH, 30.0], at), "elsewhere");
    }

    #[test]
    fn the_wait_for_explosives_counts_from_the_last_one_sent() {
        use crate::combat::VatsBlast;
        let flying = |toward| VatsBlast { toward, off: 0, went_off: 0 };
        let mut v = VatsMode { style: VatsStyle::DeadEye, ..Default::default() };
        // The first of a long queue flies.
        assert!(v.blast_holds(flying(1)));
        // Eleven seconds on another leaves: the patience starts over.
        v.now = 11.0;
        assert!(v.blast_holds(flying(2)));
        v.now = 20.0;
        assert!(v.blast_holds(flying(2)), "9 s after the last one left");
        // None on its way: the next one's patience is its own.
        v.now = 21.0;
        assert!(!v.blast_holds(flying(0)));
        v.now = 40.0;
        assert!(v.blast_holds(flying(1)));
        // One that never goes off is not waited for beyond the patience.
        v.now = 40.0 + BLAST_PATIENCE + 1.0;
        assert!(!v.blast_holds(flying(1)));
    }

    #[test]
    fn any_action_but_marking_aiming_and_accepting_leaves() {
        let only = |k: Control| move |c: Control| c == k;
        // Activate accepts with a mark made, and leaves (to activate) with none or while firing.
        assert_eq!(leaving_press(Phase::Marking, 2, only(Control::Activate)), None);
        assert_eq!(leaving_press(Phase::Marking, 0, only(Control::Activate)), Some(Control::Activate));
        assert_eq!(leaving_press(Phase::Firing, 2, only(Control::Activate)), Some(Control::Activate));
        assert_eq!(leaving_press(Phase::Drawing, 0, only(Control::Activate)), Some(Control::Activate));
        for c in [Control::Rest, Control::Jump, Control::CrouchSneak, Control::Hotkey3, Control::QuickLoad, Control::Grab] {
            assert_eq!(leaving_press(Phase::Marking, 1, only(c)), Some(c), "{c:?}");
        }
        // The mode's own controls, moving and the camera stay.
        for c in [Control::Use, Control::Block, Control::Vats, Control::Forward, Control::Run, Control::TogglePov] {
            assert_eq!(leaving_press(Phase::Marking, 1, only(c)), None, "{c:?}");
        }
        assert_eq!(leaving_press(Phase::Off, 0, only(Control::Rest)), None);
    }

    #[test]
    fn a_chance_reads_as_whole_percent() {
        assert_eq!(chance_text(62.4), "62%");
        assert_eq!(chance_text(94.6), "95%");
        assert_eq!(chance_text(0.0), "0%");
        assert_eq!(chance_text(-3.0), "0%");
    }

    #[test]
    fn a_marks_chance_follows_range_and_perks() {
        let w = |style, skill| InHand { id: FormId(1), reach: REACH, style, melee_reach: 75.0, sight_fov: 0.0, skill, hit: HitTuning { spread_mult: 1.1, ..HitTuning::default() }, special: None };
        let torso = Spot { point: [500.0, 0.0, 0.0], base: 60.0, size: SIZE_TORSO, ..Default::default() };
        let gun = w(AttackStyle::Single, 50.0);
        let near = chance_of(&torso, &gun, [0.0; 3], Some(2.0));
        let far = chance_of(&Spot { point: [3000.0, 0.0, 0.0], ..torso.clone() }, &gun, [0.0; 3], Some(2.0));
        assert!(near > far && far > 0.0, "{near} {far}");
        // Melee ignores the range: the player is put beside the mark.
        let blade = w(AttackStyle::Melee, 50.0);
        assert_eq!(chance_of(&torso, &blade, [0.0; 3], None), chance_of(&Spot { point: [300.0, 0.0, 0.0], ..torso.clone() }, &blade, [0.0; 3], None));
        // A melee mark follows the skill and is not pinned at the cap.
        let head = Spot { base: 30.0, ..torso.clone() };
        let (weak, strong) = (chance_of(&head, &w(AttackStyle::Melee, 0.0), [0.0; 3], None), chance_of(&head, &blade, [0.0; 3], None));
        assert!(weak < strong && strong < 95.0, "{weak} {strong}");
        // Close by: capped at fVATSMaxChance.
        assert_eq!(chance_of(&torso, &gun, [490.0, 0.0, 0.0], Some(2.0)), 95.0);
        // Concentrated Fire's bonus counts only with the perk.
        let again = Spot { earlier: 2, ..torso.clone() };
        assert_eq!(chance_of(&again, &gun, [0.0; 3], Some(2.0)), near);
        let cf = Spot { perks: HitPerks { ops: Vec::new(), concentrated_fire: true }, ..again };
        assert!((chance_of(&cf, &gun, [0.0; 3], Some(2.0)) - (near + 10.0)).abs() < 1e-3);
    }

    #[test]
    fn a_preview_figure_is_the_marks_chance() {
        let gun = InHand { id: FormId(1), reach: REACH, style: AttackStyle::Single, melee_reach: 0.0, sight_fov: 0.0, skill: 45.0, hit: HitTuning::default(), special: None };
        let head = Spot { point: [900.0, 0.0, 60.0], part: "Head".into(), base: 30.0, size: 0.4, ..Default::default() };
        let mut v = VatsMode { phase: Phase::Marking, queue: MarkQueue::new(20.0, 1, 100), weapon: Some(gun.clone()), style: VatsStyle::DeadEye, ..Default::default() };
        let previewed = chance_of(&head, &gun, [0.0; 3], Some(1.5));
        v.preview = Preview { health: None, actor: Some(7), parts: vec![PreviewPart { part: Some(1), spot: Spot { chance: previewed, ..head.clone() }, shown: true, under: true, queued: 0, condition: None, damage: 0.0 }], built_at: 0.0 };
        // Marked, the part's chance is worked out again the marks' way: the same figure.
        v.queue.push(FormId(7), Some(1), f32::INFINITY).unwrap();
        v.spots.push(head);
        let combat_cone = Some(1.5);
        for s in &mut v.spots {
            s.chance = chance_of(s, v.weapon.as_ref().unwrap(), [0.0; 3], combat_cone);
        }
        assert_eq!(chance_text(v.spots[0].chance), chance_text(previewed));
        assert_eq!(v.previewed().count(), 1);
        v.phase = Phase::Firing;
        assert_eq!(v.previewed().count(), 0, "nothing is previewed while firing");
        v.phase = Phase::Marking;
        v.preview.parts[0].shown = false;
        assert_eq!(v.previewed().count(), 0, "a hidden or marked part is not drawn");
    }

    #[test]
    fn the_classic_grade_fades_in_without_a_swipe_or_a_wash() {
        // No edge crossing the picture (2 is past its right side), no band, no wash.
        assert_eq!(classic_look_at(true, Some(0.0), None), [2.0, 0.0, 0.0, 0.0]);
        let half = classic_look_at(true, Some(crate::dialogue_zoom::VATS_IN_SECONDS / 2.0), None);
        assert_eq!((half[0], half[1], half[2]), (2.0, 0.0, 0.0));
        assert!((half[3] - 0.5).abs() < 1e-6);
        assert_eq!(classic_look_at(true, Some(5.0), None), [2.0, 0.0, 0.0, 1.0]);
        assert_eq!(classic_look_at(false, None, Some(LEAVE_FADE / 2.0))[3], 0.5);
        assert_eq!(classic_look_at(false, None, Some(LEAVE_FADE)), [0.0; 4]);
        // Dead Eye keeps its swipe.
        assert!(look_at(true, Some(0.1), None)[1] > 0.0);
    }

    #[test]
    fn a_preview_label_names_the_part() {
        let mut t = String::new();
        preview_text(&mut t, "Head", 41.4, false);
        assert_eq!(t, "Head 41%");
        preview_text(&mut t, "Left Arm", 7.6, true);
        assert_eq!(t, "[Left Arm 8%]");
        preview_text(&mut t, "whole", 62.0, false);
        assert_eq!(t, "62%");
        preview_text(&mut t, "its weapon", 120.0, false);
        assert_eq!(t, "Weapon 100%");
    }

    #[test]
    fn the_clock_eases_down_over_the_swipe() {
        assert_eq!(time_scale_at(0.0), 1.0);
        let mid = time_scale_at(ENTER_SWIPE / 2.0);
        assert!(mid < 1.0 && mid > MARK_SCALE, "{mid}");
        assert!((time_scale_at(ENTER_SWIPE) - MARK_SCALE).abs() < 1e-6);
        assert!((time_scale_at(10.0) - MARK_SCALE).abs() < 1e-6);
    }

    #[test]
    fn the_swipe_crosses_then_the_wash_fades_and_the_grade_stays() {
        let start = look_at(true, Some(0.0), None);
        assert_eq!(start, [0.0, BAND_STRENGTH, 0.0, 1.0]);
        let half = look_at(true, Some(ENTER_SWIPE / 2.0), None);
        assert!(half[0] > 0.5 && half[0] < 0.7, "{half:?}");
        let after = look_at(true, Some(ENTER_SWIPE), None);
        assert_eq!(after, [2.0, 0.0, WASH_START, 1.0]);
        assert_eq!(look_at(true, Some(ENTER_SWIPE + ENTER_WASH + 1.0), None), [2.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn leaving_fades_the_grade_out() {
        let l = look_at(false, None, Some(LEAVE_FADE / 2.0));
        assert!((l[3] - 0.5).abs() < 1e-6, "{l:?}");
        assert_eq!(look_at(false, None, Some(LEAVE_FADE)), [0.0; 4]);
        assert_eq!(look_at(false, None, None), [0.0; 4]);
    }

    #[test]
    fn marking_nearly_stops_the_clock_and_firing_halves_it() {
        let mut v = VatsMode { phase: Phase::Marking, entered_at: Some(0.0), now: 5.0, style: VatsStyle::DeadEye, ..Default::default() };
        assert!((v.time_scale() - MARK_SCALE).abs() < 1e-6);
        v.phase = Phase::Firing;
        assert_eq!(v.time_scale(), FIRE_SCALE);
        assert_eq!(FIRE_SCALE, 0.5, "owner 2026-10-18: half speed");
        // The shots come SHOT_INTERVAL / FIRE_SCALE apart in real time.
        assert!((SHOT_INTERVAL / FIRE_SCALE - 0.4).abs() < 1e-6);
        v.phase = Phase::Drawing;
        assert_eq!(v.time_scale(), 1.0);
        v.phase = Phase::Off;
        assert_eq!(v.time_scale(), 1.0);
    }

    /// #3784: sounds slow while targets are chosen only as far as while firing.
    #[test]
    fn marking_slows_sound_no_more_than_firing() {
        let mut v = VatsMode { phase: Phase::Marking, entered_at: Some(0.0), now: 5.0, style: VatsStyle::DeadEye, ..Default::default() };
        assert_eq!(v.sound_scale(), FIRE_SCALE);
        // Early in the swipe the clock is still faster: the sound follows it.
        v.now = 0.0;
        assert_eq!(v.sound_scale(), 1.0);
        // The classic style's full stop too.
        v.style = VatsStyle::Classic;
        v.classic.scale = 0.0;
        assert_eq!(v.sound_scale(), FIRE_SCALE);
        v.phase = Phase::Firing;
        assert_eq!(v.sound_scale(), FIRE_SCALE);
        v.phase = Phase::Off;
        assert_eq!(v.sound_scale(), 1.0);
    }

    #[test]
    fn the_classic_style_stops_the_clock_at_once_and_holds_the_view() {
        // The default slowdown, a full stop, from the first frame: no ease.
        let mut v = VatsMode { phase: Phase::Marking, entered_at: Some(0.0), now: 0.0, style: VatsStyle::Classic, ..Default::default() };
        v.classic.scale = classic::clock_scale(VatsSlowdown::default().0);
        assert_eq!(v.time_scale(), 0.0);
        assert!(v.holds_view());
        // Half speed, the slider's other end.
        v.classic.scale = classic::clock_scale(0.5);
        assert_eq!(v.time_scale(), 0.5);
        // The attacks play as Dead Eye's, the view still held.
        v.phase = Phase::Firing;
        assert_eq!(v.time_scale(), FIRE_SCALE);
        assert!(v.holds_view());
        v.phase = Phase::Off;
        assert_eq!(v.time_scale(), 1.0);
        assert!(!v.holds_view());
        // Dead Eye never holds the view.
        let d = VatsMode { phase: Phase::Marking, style: VatsStyle::DeadEye, ..Default::default() };
        assert!(!d.holds_view());
    }

    #[test]
    fn a_queued_special_attack_holds_its_own_ap_and_marks_its_blow() {
        let bat = SpecialAttack::from_fields(None, 50.0, 2.0, 35.0).unwrap();
        let hand = InHand { id: FormId(1), reach: MELEE_MARK_REACH, style: AttackStyle::Melee, melee_reach: 75.0, sight_fov: 0.0, skill: 60.0, hit: HitTuning::default(), special: Some((bat, 35.0)) };
        let mut v = VatsMode { phase: Phase::Marking, queue: MarkQueue::new(25.0, 1, u32::MAX), style: VatsStyle::Classic, weapon: Some(hand), ..Default::default() };
        v.queue.push(FormId(7), None, 80.0).unwrap();
        v.spots.push(Spot::default());
        v.queue.push_costing(FormId(7), None, 80.0, 35.0).unwrap();
        v.spots.push(Spot { special: true, ..Default::default() });
        assert_eq!(v.reserved(), 60.0, "an ordinary attack and a special one");
        assert_eq!(v.swing_special(), None, "nothing is struck while targets are chosen");
        v.phase = Phase::Firing;
        v.started = 1;
        assert_eq!(v.reserved(), 35.0);
        v.striking = Some(0);
        assert_eq!(v.swing_special(), None, "the first blow is the weapon's own attack");
        v.striking = Some(1);
        assert_eq!(v.swing_special(), Some(bat));
        assert_eq!(special_text(2.0, 35.0), "special attack: damage x2.0, 35 AP");
    }

    #[test]
    fn the_classic_style_draws_no_marks_while_targets_are_chosen() {
        let mut v = VatsMode { phase: Phase::Marking, queue: MarkQueue::new(20.0, 1, 100), style: VatsStyle::Classic, ..Default::default() };
        v.queue.push(FormId(7), Some(1), f32::INFINITY).unwrap();
        v.spots.push(Spot { point: [7.0, 1.0, 0.0], chance: 63.0, ..Default::default() });
        assert!(v.drawn().is_empty(), "the part's own label counts the queued attacks");
        v.phase = Phase::Firing;
        assert!(v.drawn().is_empty(), "nor while they play: nothing of the HUD is drawn then");
        assert!(v.playing_back());
        v.style = VatsStyle::DeadEye;
        assert_eq!(v.drawn(), vec![([7.0, 1.0, 0.0], 63.0)], "Dead Eye's marks show while its shots are fired");
        assert!(v.playing_back(), "and the rest of the HUD is not drawn there either");
    }

    #[test]
    fn the_reservation_shrinks_as_shots_are_fired() {
        let mut v = VatsMode { phase: Phase::Marking, queue: MarkQueue::new(26.0, 1, 6), style: VatsStyle::DeadEye, ..Default::default() };
        v.queue.push(FormId(1), None, 92.0).unwrap();
        v.queue.push(FormId(2), Some(1), 92.0).unwrap();
        assert_eq!(v.reserved(), 52.0);
        v.phase = Phase::Firing;
        v.started = 1;
        assert_eq!(v.reserved(), 26.0);
        v.phase = Phase::Off;
        assert_eq!(v.reserved(), 0.0);
    }

    #[test]
    fn queued_rows_keep_the_target_and_data_part_names() {
        assert_eq!(queued_text("Practice robot", "Left probe", false), "Practice robot: Left probe");
        assert_eq!(queued_text("Practice robot", "Left probe", true), "Practice robot: Left probe (special)");
        assert_eq!(queued_text("Practice robot", "", false), "Practice robot");
        assert_eq!(queued_text("Practice robot", "whole", true), "Practice robot (special)");
    }

    #[test]
    fn classic_displayed_ammo_reserves_rounds_without_consuming_them() {
        for rounds_per_attack in [1, 3] {
            let mut v = VatsMode { phase: Phase::Marking, style: VatsStyle::Classic, queue: MarkQueue::new(20.0, rounds_per_attack, 6), ..Default::default() };
            let actual = Some((6, 18));
            assert_eq!(v.displayed_ammo(actual), actual);
            v.queue.push(FormId(1), Some(1), 100.0).unwrap();
            assert_eq!(v.displayed_ammo(actual), Some((6 - rounds_per_attack, 18)));
            v.queue.push(FormId(2), Some(2), 100.0).unwrap();
            assert_eq!(v.displayed_ammo(actual), Some((6 - 2 * rounds_per_attack, 18)));
            assert_eq!(v.queue.rounds, 6);
            assert_eq!(v.displayed_ammo(None), None);
            assert_eq!(v.displayed_ammo(Some((0, 18))), Some((0, 18)));
            v.style = VatsStyle::DeadEye;
            assert_eq!(v.displayed_ammo(actual), actual);
            v.style = VatsStyle::Classic;
            v.phase = Phase::Firing;
            assert_eq!(v.displayed_ammo(Some((5, 18))), Some((5, 18)));
            v.phase = Phase::Off;
            assert_eq!(v.displayed_ammo(actual), actual);
            v.phase = Phase::Marking;
            v.queue.pop();
            assert_eq!(v.displayed_ammo(actual), Some((6 - rounds_per_attack, 18)));
            while v.queue.pop().is_some() {}
            assert_eq!(v.displayed_ammo(actual), actual);
        }
    }

    #[test]
    fn classic_enemy_bar_follows_the_playback_attack() {
        let mut v = VatsMode { phase: Phase::Marking, style: VatsStyle::Classic, queue: MarkQueue::new(20.0, 1, 6), ..Default::default() };
        v.classic.selected = Some(1);
        v.queue.push(FormId(2), Some(1), 100.0).unwrap();
        v.queue.push(FormId(3), Some(2), 100.0).unwrap();
        assert_eq!(v.bar_target(), Some(1));
        v.phase = Phase::Firing;
        assert_eq!(v.bar_target(), Some(1));
        v.shooting = Some(0);
        assert_eq!(v.bar_target(), Some(2));
        v.shooting = Some(1);
        assert_eq!(v.bar_target(), Some(3));
        v.phase = Phase::Off;
        assert_eq!(v.bar_target(), None);
        v.phase = Phase::Firing;
        v.style = VatsStyle::DeadEye;
        assert_eq!(v.bar_target(), None);
    }

    #[test]
    fn a_melee_teleport_lands_beside_the_mark_on_the_players_side() {
        // A machete: reach 150 × 0.5 = 75; put 69.5 units from the mark, within reach + slack.
        let off = stand_off(75.0);
        assert!((off - 69.5).abs() < 1e-4);
        assert!(off <= 75.0 + CLOSE_SLACK);
        let feet = stand_beside([300.0, 0.0, 10.0], [0.0, 0.0, 40.0], off);
        assert_eq!(feet, [69.5, 0.0, 40.0]);
        // Standing on the mark: put to one side all the same.
        let feet = stand_beside([5.0, 5.0, 0.0], [5.0, 5.0, 0.0], 50.0);
        assert!((((feet[0] - 5.0).powi(2) + (feet[1] - 5.0).powi(2)).sqrt() - 50.0).abs() < 1e-4);
        // The owner's 5 m at 70 units a metre.
        assert_eq!(MELEE_MARK_REACH, 350.0);
    }

    #[test]
    fn third_person_shots_use_the_sights_beyond_20_m_only() {
        use crate::combat::aims_down_sights;
        // 20 m = 1,400 units; once up, the sights stay up to 1,350.
        assert!(!aims_down_sights(false, 1400.0, 55.0) && aims_down_sights(false, 1401.0, 55.0));
        assert!(aims_down_sights(true, 1360.0, 55.0) && !aims_down_sights(true, 1350.0, 55.0));
        // A gun without sights (Sight FOV 0) never raises them.
        assert!(!aims_down_sights(false, 5000.0, 0.0));
        assert_eq!(crate::combat::distance([0.0, 0.0, 0.0], [3.0, 4.0, 12.0]), 13.0);
    }

    #[test]
    fn marks_on_one_part_draw_once_with_the_newest_chance() {
        let mut v = VatsMode { phase: Phase::Marking, queue: MarkQueue::new(20.0, 1, 100), style: VatsStyle::DeadEye, ..Default::default() };
        for (target, part, chance) in [(7, Some(1), 63.0), (7, Some(1), 68.0), (9, None, 40.0), (7, Some(1), 73.0), (7, Some(2), 30.0)] {
            v.queue.push(FormId(target), part, f32::INFINITY).unwrap();
            v.spots.push(Spot { point: [target as f32, part.unwrap_or(0) as f32, 0.0], chance, ..Default::default() });
        }
        assert_eq!(v.drawn(), vec![([7.0, 1.0, 0.0], 73.0), ([9.0, 0.0, 0.0], 40.0), ([7.0, 2.0, 0.0], 30.0)]);
        v.phase = Phase::Firing;
        v.started = 3;
        assert_eq!(v.drawn(), vec![([7.0, 1.0, 0.0], 73.0), ([7.0, 2.0, 0.0], 30.0)], "the fired marks go");
    }

    #[test]
    fn the_melee_mark_is_the_one_being_swung_at_while_firing() {
        let mut v = VatsMode { phase: Phase::Firing, queue: MarkQueue::new(20.0, 1, 100), style: VatsStyle::DeadEye, ..Default::default() };
        v.queue.push(FormId(7), None, 92.0).unwrap();
        v.spots.push(Spot { point: [1.0, 2.0, 3.0], name: "dog".into(), part: "whole".into(), ..Default::default() });
        v.weapon = Some(InHand { id: FormId(1), reach: MELEE_MARK_REACH, style: AttackStyle::Melee, melee_reach: 75.0, sight_fov: 0.0, skill: 50.0, hit: HitTuning::default(), special: None });
        assert_eq!(v.melee_mark(), None, "no swing yet");
        v.striking = Some(0);
        assert_eq!(v.melee_mark(), Some((7, [1.0, 2.0, 3.0], 75.0 + CLOSE_SLACK, false)), "the roll has not let it land");
        v.blow_lands = true;
        assert_eq!(v.melee_mark(), Some((7, [1.0, 2.0, 3.0], 75.0 + CLOSE_SLACK, true)));
        v.swing_now = true;
        assert!(v.take_swing() && !v.take_swing(), "one tap");
        v.weapon.as_mut().unwrap().style = AttackStyle::Thrown;
        assert_eq!(v.melee_mark(), None, "a throw is no blow");
    }
    #[test]
    fn touch_gesture_binding_pulse_uses_native_vats_draw_admission_and_all_locks() {
        use bevy::input::{keyboard::{Key, KeyboardInput}, ButtonState};
        use crate::bindings::Bindings;
        for barrier in 0..4 {
            let (mut app, _) = menu_key_app(crate::bindings::InputDevice::KeyboardMouse, VatsMode::default());
            app.world_mut().resource_mut::<Bindings>().rebind(Control::Vats, crate::bindings::Input::Key(KeyCode::F9));
            if barrier == 1 { app.insert_resource(crate::script_input::ScriptBlocked { keys: vec![KeyCode::F9], ..default() }); }
            if barrier == 2 { app.world_mut().resource_mut::<ControlLocks>().fighting = true; }
            if barrier == 3 { app.init_resource::<crate::pad::PlatformInputGate>(); app.world_mut().resource_mut::<crate::pad::PlatformInputGate>().set_blocked(true); }
            app.world_mut().write_message(KeyboardInput { key_code: KeyCode::F9, logical_key: Key::Character("VATS gesture fixture".into()),
                state: ButtonState::Pressed, repeat: false, text: None, window: Entity::PLACEHOLDER });
            app.update();
            assert_eq!(app.world().resource::<VatsMode>().phase, if barrier == 0 { Phase::Drawing } else { Phase::Off });
            assert_eq!(app.world().resource::<PlayerControl>().weapon_out, barrier == 0);
            assert_eq!(app.world().resource::<VatsMode>().stats.shots, 0, "gesture does not invent a shot/AP action");
        }
    }

}
