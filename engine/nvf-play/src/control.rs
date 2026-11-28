//! The player's controls beyond walking and looking, on the game's bindings
//! ([`crate::bindings`]; docs/design/player-body.md §1.2):
//!
//! - Crouch/Sneak (Left Ctrl) toggles sneaking.
//! - Ready Item (R): with the weapon away, draws it; with it out, a tap reloads and a hold
//!   of [`HOLSTER_HOLD`] puts it away [hypothesis: the game's tap/hold split]. Reloading
//!   is `crate::combat`'s.
//! - Lower Weapon (H, NVForever's own) lowers the drawn weapon or raises it again
//!   ([`PlayerControl::lowered`]) [design: the owner 2026-10-26 asked for it on Ready
//!   Item tapped twice; 2026-10-27 moved to its own key, since waiting for a second tap
//!   held every reload back].
//! - Use (left mouse) attacks while held ([`PlayerControl::trigger`], `crate::combat`);
//!   with the weapon away it draws it first [hypothesis].
//! - Ammo Swap (hotkey 2) asks `crate::combat` for the next carried ammo type; the
//!   hotkeys 1 to 8 are the weapon wheel's (`crate::weapon_wheel`, which sets
//!   [`PlayerControl::ammo_swap`] on a tap of 2).
//! - Block (right mouse, Left Alt), held: guns aim down the sights, fists and melee
//!   weapons block.
//! - Always Run (Caps Lock) toggles running as the default (on by default:
//!   `bAlwaysRunByDefault=1`); Run (Left Shift) held does the other; Auto Move (Q) walks
//!   forward until Forward or Back is pressed (player.rs).
//! - Sprint (no key until the player binds one; Run keeps Left Shift), NVForever's own, only with its NVF Features switch
//!   on: read here into [`PlayerControl::sprint_held`] for the feature (`crate::stance`, the
//!   contract).
//! - Toggle POV (F, middle mouse): a tap switches first and third person; held while the
//!   mouse moves, it circles the camera around the player without turning them
//!   ([`Orbit`]; from first person the view goes to third person meanwhile), and when it
//!   is let go the camera swings back behind and the view stays third person [hypothesis:
//!   the game's hold-POV camera; design: the owner 2026-10-19, replacing playtest list 5
//!   item 23's return to first person].
//!   The mouse wheel zooms the third-person camera.
//! - Detached camera (the settings page's Gameplay switch, off by default): in third
//!   person with the weapon away the mouse turns the camera around the player, not the
//!   player, and the player turns to run the way they are pushed, seen from the camera
//!   ([`detach_camera`]) [design: the owner 2026-10-27, "360 movement in third person
//!   with your weapon holstered"; not in the original]. Drawing the weapon turns the
//!   player to where the camera looks.
//! - VATS (V) is VATS mode's (`crate::vats_mode`, docs/design/vats-mode.md), which also
//!   takes Use, Block and Activate while it is on.
//! - Grab (Z) held picks up what is under the crosshair and carries it (`crate::grab`,
//!   docs/spec-notes/grab.md).
//! - Quick Save (F5) and Quick Load (F9) are `crate::save`'s. A control bound but not built
//!   would go in [`NOT_BUILT`], which says so on screen when it is pressed; none is now.
//! - While the weapon wheel holds the input ([`PlayerControl::wheel`]) Use and Block are
//!   not the game's.
//! - Scripts turn groups of controls off and on (`DisablePlayerControls`,
//!   `EnablePlayerControls`): [`ControlLocks`], applied to every frame's requests by
//!   [`enforce_locks`].
//!
//! This module holds the requests; what they look like is the player body's job
//! (nvforever `player_body.rs`), which also writes the eye it sees from into
//! [`crate::player::Player::body_eye`]. The camera ([`place_camera`]) sits at that eye in
//! first person, and behind the shoulder in third person, pulled in where collision is in
//! the way.

use crate::activate::HudMessage;
use crate::bindings::{Bindings, Control, ControlInput};
use crate::phys::PhysState;
use crate::player::{MoveIntent, Player};
use crate::sim::SimState;
use crate::{space, PlayState, PlayerCamera};
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use fnv_phys::Filter;

/// First- or third-person view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pov {
    #[default]
    First,
    Third,
}

/// The camera circling the player while Toggle POV is held: angles added to the view's
/// heading and pitch for the camera only.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Orbit {
    pub yaw: f32,
    pub pitch: f32,
    /// Toggle POV is down.
    pub held: bool,
    /// The mouse moved while it was down (so letting go does not switch the view).
    pub moved: bool,
    /// The camera is detached ([`detach_camera`]): `yaw` is where it looks off the
    /// player's heading, and it does not swing back.
    pub free: bool,
}

impl Orbit {
    /// Whether the camera is off its place behind the player (circling, or swinging back).
    pub fn active(&self) -> bool {
        (self.held && self.moved) || self.yaw.abs() + self.pitch.abs() > 1e-3
    }

    /// Toggle POV let go after circling: the camera swings back behind the player, and a
    /// view that was first person stays third person (the owner 2026-10-19, replacing
    /// playtest list 5 item 23's return to first person) [design: the original's hold-POV
    /// camera is not captured].
    pub fn released(self) -> Orbit {
        Orbit { held: false, ..self }
    }
}

/// The camera in front of the player looking back at them, while the character creator
/// is open (`crate::creator`) [design, docs/spec-notes/character-creator.md §5].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Portrait {
    /// Turned about the player from straight in front of them, radians.
    pub yaw: f32,
    /// From the point looked at, game units.
    pub distance: f32,
    /// The point looked at, this far under the eye, game units.
    pub drop: f32,
    /// Looking down (positive) or up, radians.
    pub pitch: f32,
    /// The camera stepped this far to its left, game units (to its right when negative,
    /// so the player stands left of the picture's middle, clear of the creator's panel).
    pub shift: f32,
}

impl Portrait {
    /// Where the camera stands and the heading and pitch it looks with, for a player
    /// whose eye is at `eye` and who faces `heading`.
    pub fn spot(&self, eye: [f32; 3], heading: f32) -> ([f32; 3], f32, f32) {
        let target = [eye[0], eye[1], eye[2] - self.drop];
        let out = space::look_dir(heading + self.yaw, self.pitch);
        let look = heading + self.yaw + std::f32::consts::PI;
        // The camera's right, level: (cos h, −sin h) for the heading it looks with.
        let (s, c) = look.sin_cos();
        let at = [target[0] + out[0] * self.distance - c * self.shift, target[1] + out[1] * self.distance + s * self.shift, target[2] + out[2] * self.distance];
        (at, look, -self.pitch)
    }
}

/// What the player asks of their character besides moving.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct PlayerControl {
    /// Crouched (sneaking).
    pub sneak: bool,
    /// Wants the weapon out (fists without one).
    pub weapon_out: bool,
    /// Block held: aiming down the sights, or blocking.
    pub aiming: bool,
    /// Running is the default (Always Run).
    pub always_run: bool,
    /// Walking forward on its own (Auto Move).
    pub auto_move: bool,
    pub pov: Pov,
    /// Third-person camera distance behind the pivot, game units.
    pub distance: f32,
    pub orbit: Orbit,
    /// How long Ready Item has been held with the weapon out (a hold holsters).
    pub ready_held: Option<f32>,
    /// A Ready Item tap waiting [`DOUBLE_TAP`] for a second one (the player's Double-Tap
    /// Reload Changes Ammo tweak): seconds since it.
    pub reload_wait: Option<f32>,
    /// Use held with the weapon out (the trigger; `crate::combat` reads it).
    pub trigger: bool,
    /// A reload was asked for (Ready Item tapped with the weapon out); `crate::combat`
    /// clears it.
    pub reload: bool,
    /// The player lowered the weapon by hand (Lower Weapon): it stays down until Lower
    /// Weapon again, a reload, aiming, the trigger or putting it away (the first-person pose
    /// is `nvforever`'s `weapon_lower.rs`).
    pub lowered: bool,
    /// Ammo Swap pressed; `crate::combat` clears it.
    pub ammo_swap: bool,
    /// The weapon wheel has the mouse and the attack keys (`crate::weapon_wheel`).
    pub wheel: bool,
    /// The Sprint control is held with Sprint on (`crate::stance`; the Sprint feature reads
    /// it). Struck by the movement lock.
    pub sprint_held: bool,
    /// The third-person camera's line through the crosshair, set by [`place_camera`]
    /// each frame; `None` in first person and while the camera circles. Activation and
    /// the player's shots aim along it ([`Crosshair::ray`]).
    pub crosshair: Option<Crosshair>,
    /// The character creator's camera, while it is open.
    pub portrait: Option<Portrait>,
    /// A kill camera has the view (`crate::kill_cam`): it counts as a third-person view,
    /// so the body is drawn and the first-person arms are not.
    pub cinematic: bool,
    /// A minigame's camera has the view (the slot machine's, nvforever `slots_view.rs`):
    /// counted as a third-person view, as [`Self::cinematic`] is.
    pub scene_cam: bool,
    /// A conversation's close-up from third person (`crate::dialogue_zoom`): how far the
    /// camera has travelled from its place behind the player to the first-person eye, 0 to 1.
    /// [`Self::pov`] stays the player's own.
    pub talk_camera: f32,
}

impl Default for PlayerControl {
    fn default() -> Self {
        PlayerControl {
            sneak: false,
            weapon_out: false,
            aiming: false,
            always_run: crate::bindings::ALWAYS_RUN_BY_DEFAULT,
            auto_move: false,
            pov: Pov::First,
            distance: DEFAULT_DISTANCE,
            orbit: Orbit::default(),
            ready_held: None,
            reload_wait: None,
            trigger: false,
            reload: false,
            lowered: false,
            ammo_swap: false,
            wheel: false,
            sprint_held: false,
            crosshair: None,
            portrait: None,
            cinematic: false,
            scene_cam: false,
            talk_camera: 0.0,
        }
    }
}

impl PlayerControl {
    /// Cancel holds owned by the previous input device without performing their tap
    /// actions. Keep intentional toggles and the camera's angles [design, #2200].
    pub(crate) fn cancel_input_holds(&mut self) {
        self.orbit.held = false;
        self.orbit.moved = false;
        self.ready_held = None;
        self.reload_wait = None;
        self.aiming = false;
        self.trigger = false;
        self.sprint_held = false;
    }

    /// Whether the player's own view is from outside (third person, or circling), whatever a
    /// conversation's close-up does with the camera.
    pub fn own_third_person(&self) -> bool {
        self.pov == Pov::Third || self.orbit.active()
    }

    /// Whether the camera shows the player from outside: the player's own view, unless a
    /// conversation's close-up has come so close that the body goes
    /// ([`TALK_CAMERA_CLOSE`]), or a camera that does (the creator's, a kill camera).
    pub fn third_person_view(&self) -> bool {
        self.portrait.is_some() || self.cinematic || self.scene_cam || (self.own_third_person() && self.talk_camera < TALK_CAMERA_CLOSE)
    }

    /// The heading the view looks along for a player facing `heading`: turned by the
    /// orbit in third person (circling, or the detached camera), else the player's own.
    /// The compass follows it, so it shows where the camera looks [owner 2026-10-27].
    pub fn view_heading(&self, heading: f32) -> f32 {
        if self.third_person_view() && self.portrait.is_none() {
            (heading + self.orbit.yaw).rem_euclid(std::f32::consts::TAU)
        } else {
            heading
        }
    }
}

/// The third-person camera's view line: the crosshair sits in the middle of the view, so
/// what is under it lies along this line, not along the eye's (the camera is off to the
/// side, over the shoulder: playtest list 4, items 11 and 12).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crosshair {
    /// The camera, game space.
    pub camera: [f32; 3],
    /// The view direction (unit), game space.
    pub dir: [f32; 3],
    /// The pivot the camera was placed from, and where the camera's line passes it:
    /// right of it and up, level with it along the view (the shoulder offset, less what
    /// collision pulled in).
    pub pivot: [f32; 3],
    pub shoulder: [f32; 2],
}

impl Crosshair {
    /// The line of a camera placed at `camera`, looking along `heading` and `pitch`.
    pub fn placed(camera: [f32; 3], heading: f32, pitch: f32, pivot: [f32; 3]) -> Crosshair {
        let dir = space::look_dir(heading, pitch);
        let (s, c) = heading.sin_cos();
        let rel = [camera[0] - pivot[0], camera[1] - pivot[1], camera[2] - pivot[2]];
        let side = rel[0] * c - rel[1] * s;
        // Along the view to where the line is level with the pivot horizontally.
        let ahead = rel[0] * s + rel[1] * c;
        let level = dir[0] * s + dir[1] * c;
        let up = if level > 1e-3 { rel[2] - dir[2] * ahead / level } else { rel[2] };
        Crosshair { camera, dir, pivot, shoulder: [side, up] }
    }

    /// The ray under the crosshair, `(start, dir)`: from the point on the camera's line
    /// level with the eye (so nothing between the camera and the player is picked, and
    /// reaches stay measured from about the player), along the view.
    pub fn ray(this: Option<&Crosshair>, eye: [f32; 3], look: [f32; 3]) -> ([f32; 3], [f32; 3]) {
        let Some(c) = this else { return (eye, look) };
        let rel = [eye[0] - c.camera[0], eye[1] - c.camera[1], eye[2] - c.camera[2]];
        let t = (rel[0] * c.dir[0] + rel[1] * c.dir[1] + rel[2] * c.dir[2]).max(0.0);
        ([c.camera[0] + c.dir[0] * t, c.camera[1] + c.dir[1] * t, c.camera[2] + c.dir[2] * t], c.dir)
    }

    /// The direction from `eye` to the point under the crosshair, given where the
    /// crosshair ray first hits (`hit`, `None`: nothing within `reach`): what the
    /// player's shot follows, so it lands under the crosshair as in first person. The view
    /// direction itself when that point is not ahead of the eye.
    pub fn aim_from(eye: [f32; 3], start: [f32; 3], dir: [f32; 3], reach: f32, hit: Option<[f32; 3]>) -> [f32; 3] {
        let p = hit.unwrap_or([start[0] + dir[0] * reach, start[1] + dir[1] * reach, start[2] + dir[2] * reach]);
        let to = [p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]];
        let len = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt();
        // Under a hand's breadth ahead, or behind: keep the view's direction.
        if len < 1.0 || (to[0] * dir[0] + to[1] * dir[1] + to[2] * dir[2]) < 0.5 * len {
            return dir;
        }
        [to[0] / len, to[1] / len, to[2] / len]
    }

    /// The heading and pitch that put `point` under the crosshair: from `eye` in first
    /// person (`None`), else turned so the camera's line, offset by the shoulder from the
    /// pivot, passes through it (collision pulling the camera in is left out).
    pub fn aim_at(this: Option<&Crosshair>, eye: [f32; 3], point: [f32; 3]) -> (f32, f32) {
        let (from, side, up) = match this {
            Some(c) => (c.pivot, c.shoulder[0], c.shoulder[1]),
            None => (eye, 0.0, 0.0),
        };
        let (dx, dy, dz) = (point[0] - from[0], point[1] - from[1], point[2] - from[2]);
        let r = dx.hypot(dy).max(1e-3);
        let side = side.clamp(-0.99 * r, 0.99 * r);
        let heading = dx.atan2(dy) - (side / r).asin();
        let along = (r * r - side * side).sqrt();
        (heading, (dz - up).atan2(along))
    }
}

/// The control groups scripts have turned off with `DisablePlayerControls` and not yet
/// back on with `EnablePlayerControls` (docs/spec-notes/controls.md §5): the script
/// runtime's seven flags, copied here after each frame's scripts (`script::sync_locks`).
/// What each stops [documented: GECK wiki DisablePlayerControls, "Flag Definitions"]:
///
/// - `movement`: moving (and jumping, Auto Move [hypothesis]), activating world objects,
///   and the crosshair (`activate.rs`) and health (`combat.rs`) on the HUD.
/// - `pipboy`: opening the Pip-Boy (`menu.rs`).
/// - `fighting`: bringing up a weapon; a weapon that is out is put away.
/// - `pov`: third person; a third-person view goes back to first person.
/// - `looking`: turning the view (`player::mouse_look`).
/// - `rollover`: the prompt on world objects (`activate.rs`).
/// - `sneaking`: sneaking; a sneaking player stands up.
///
/// The keys are still read; [`enforce_locks`] strikes what is locked from the frame's
/// requests, so test aids that press controls are held like the keys.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ControlLocks {
    pub movement: bool,
    pub pipboy: bool,
    pub fighting: bool,
    pub pov: bool,
    pub looking: bool,
    pub rollover: bool,
    pub sneaking: bool,
}

/// The groups' places in the functions' parameter lists (and in [`LockStats::held`]).
pub mod group {
    pub const MOVEMENT: usize = 0;
    pub const PIPBOY: usize = 1;
    pub const FIGHTING: usize = 2;
    pub const POV: usize = 3;
    pub const LOOKING: usize = 4;
    pub const ROLLOVER: usize = 5;
    pub const SNEAKING: usize = 6;
    pub const NAMES: [&str; 7] = ["movement", "Pip-Boy", "fighting", "POV", "looking", "rollover text", "sneaking"];
}

impl ControlLocks {
    /// Every group locked.
    pub const ALL: ControlLocks = ControlLocks { movement: true, pipboy: true, fighting: true, pov: true, looking: true, rollover: true, sneaking: true };

    /// From the runtime's flags, in the functions' parameter order: movement, Pip-Boy,
    /// fighting, POV, looking, rollover text, sneaking.
    pub fn from_flags(f: [bool; 7]) -> ControlLocks {
        let [movement, pipboy, fighting, pov, looking, rollover, sneaking] = f;
        ControlLocks { movement, pipboy, fighting, pov, looking, rollover, sneaking }
    }

    pub fn flags(&self) -> [bool; 7] {
        [self.movement, self.pipboy, self.fighting, self.pov, self.looking, self.rollover, self.sneaking]
    }

    pub fn any(&self) -> bool {
        *self != ControlLocks::default()
    }

    /// The locked groups by name, in parameter order.
    pub fn names(&self) -> Vec<&'static str> {
        self.flags().into_iter().zip(group::NAMES).filter(|(on, _)| *on).map(|(_, n)| n).collect()
    }

    /// Strike from the frame's requests what is locked; returns the groups (by
    /// [`group`] index) that had something to strike. Nothing comes back by itself when
    /// a group is enabled again: the weapon stays away, the view in first person and the
    /// player standing [hypothesis: not captured].
    pub fn apply(&self, intent: &mut MoveIntent, control: &mut PlayerControl) -> [bool; 7] {
        let mut struck = [false; 7];
        if self.movement {
            let before = (*intent, *control);
            (intent.forward, intent.right, intent.jump) = (0.0, 0.0, false);
            control.auto_move = false;
            control.sprint_held = false;
            struck[group::MOVEMENT] = (*intent, *control) != before;
        }
        if self.fighting {
            let before = *control;
            // Blocking and aiming go with the weapon [hypothesis].
            control.weapon_out = false;
            control.ready_held = None;
            (control.trigger, control.aiming, control.reload, control.ammo_swap) = (false, false, false, false);
            struck[group::FIGHTING] = *control != before;
        }
        if self.pov {
            let before = *control;
            // The circling camera is a view from outside too.
            control.pov = Pov::First;
            control.orbit = Orbit::default();
            struck[group::POV] = *control != before;
        }
        if self.sneaking {
            struck[group::SNEAKING] = control.sneak || intent.sneak;
            control.sneak = false;
            intent.sneak = false;
        }
        struck
    }
}

/// What the locks did, for the log line at exit.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LockStats {
    /// Times scripts changed which groups are off.
    pub changes: u32,
    /// Per group ([`group`] index): frames in which something the player asked for was
    /// struck (movement), or times a request was turned down (the others).
    pub held: [u32; 7],
}

impl LockStats {
    /// "movement 120, looking 2": the groups that held something back.
    pub fn summary(&self) -> String {
        let parts: Vec<String> = self.held.iter().zip(group::NAMES).filter(|(n, _)| **n > 0).map(|(n, name)| format!("{name} {n}")).collect();
        if parts.is_empty() {
            "nothing".to_string()
        } else {
            parts.join(", ")
        }
    }
}

/// After the frame's input (keys, mouse, test aids), before anything acts on it: what
/// scripts have locked is struck from it ([`ControlLocks::apply`]). A locked movement
/// control does not get the player up from furniture either [hypothesis].
pub fn enforce_locks(
    locks: Res<ControlLocks>,
    mut stats: ResMut<LockStats>,
    mut intent: ResMut<MoveIntent>,
    mut control: ResMut<PlayerControl>,
    mut seat: ResMut<crate::furniture::PlayerSeat>,
) {
    if !locks.any() {
        return;
    }
    let (mut i, mut c) = (*intent, *control);
    let mut struck = locks.apply(&mut i, &mut c);
    intent.set_if_neq(i);
    control.set_if_neq(c);
    if locks.movement && seat.leave {
        seat.leave = false;
        struck[group::MOVEMENT] = true;
    }
    if struck != [false; 7] {
        for (n, s) in stats.held.iter_mut().zip(struck) {
            *n += u32::from(s);
        }
    }
}

/// At exit, when scripts ever changed the player's controls: how often, what is off now
/// and what was held back.
/// Tell the scripts the camera's view (IsPC1stPerson) and whether the player sneaks
/// (IsSneaking) every frame; movement is told by detection (`nvforever` `detection.rs`).
/// First JIP's ToggleFirstPerson and TogglePlayerSneaking take effect
/// (`fnv_game::nvse_gaps`), and the scripted draws and holsters (`fnv_game::nvse_hud`).
pub fn tell_scripts(mut control: ResMut<PlayerControl>, mut script: Option<ResMut<crate::script::ScriptState>>, mut held_sneak: Option<ResMut<HeldSneakInput>>) {
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    let g = &mut rt.game;
    if let Some(first) = g.gaps.view_request.take() {
        control.pov = if first { Pov::First } else { Pov::Third };
    }
    if let Some(sneak) = g.gaps.sneak_request.take() {
        control.sneak = sneak;
        if let Some(held) = held_sneak.as_deref_mut() { held.record_base(sneak); }
    }
    // SetWeaponOut and HolsterWeapon on the player (`fnv_game::nvse_hud`); an NPC's draw
    // follows the rule the engine draws by, which SetWeaponOut 1 holds.
    for (who, out) in std::mem::take(&mut g.hud.weapon_asks) {
        if who == fnv_sim::FormId::PLAYER_REF {
            control.weapon_out = out;
        }
    }
    if let Some(run) = g.gaps.always_run_request.take() {
        control.always_run = run;
    }
    g.gaps.always_run = control.always_run;
    // The player's own view: a conversation's close-up does not change what scripts see.
    g.set_first_person(!PlayerControl { talk_camera: 0.0, ..*control }.third_person_view());
    let mut m = g.motion(fnv_sim::FormId::PLAYER_REF);
    if m.sneaking != control.sneak {
        m.sneaking = control.sneak;
        g.set_motion(fnv_sim::FormId::PLAYER_REF, m);
    }
}

pub fn report_locks(mut exit: MessageReader<AppExit>, locks: Res<ControlLocks>, stats: Res<LockStats>) {
    if exit.read().next().is_none() || stats.changes == 0 {
        return;
    }
    let off = locks.names();
    info!(
        "controls summary: scripts changed the player's controls {} times; disabled at exit: {}; held back: {}",
        stats.changes,
        if off.is_empty() { "none".to_string() } else { off.join(", ") },
        stats.summary()
    );
}

/// A conversation's close-up from third person: from this much of the camera's travel to
/// the eye the view counts as first person, so the body goes while the camera is still
/// behind the head, not through it [design].
pub const TALK_CAMERA_CLOSE: f32 = 0.85;
/// Under the player's Double-Tap Reload Changes Ammo tweak, a second Ready Item tap within
/// this many seconds swaps the ammo instead of reloading, seconds [design].
pub const DOUBLE_TAP: f32 = 0.3;
/// Holding Ready Item this long with the weapon out holsters it, seconds [hypothesis].
pub const HOLSTER_HOLD: f32 = 0.5;
/// The circling camera swings back behind the player with this time constant, seconds
/// [engineering choice].
pub const ORBIT_RETURN: f32 = 0.15;
/// Third-person distance range and default, game units [engineering choice].
pub const MIN_DISTANCE: f32 = 60.0;
pub const MAX_DISTANCE: f32 = 450.0;
pub const DEFAULT_DISTANCE: f32 = 170.0;
/// Third-person camera offset from the pivot, to the right and up (game units):
/// FalloutNV.esm's `fOverShoulderPosX` (40) and `fOverShoulderPosZ` (−10) [observed:
/// GMST values; hypothesis: their meaning, from the names], used when the plugins lack
/// them.
pub const DEFAULT_SHOULDER: [f32; 2] = [40.0, -10.0];
/// How close the third-person camera may come to what is behind it, game units
/// [engineering choice].
const WALL_MARGIN: f32 = 8.0;

/// Speed while sneaking as a fraction of walking or running: `fMoveSneakMult`, 0.57 in
/// FalloutNV.esm (used when the plugins lack it) [observed: the player sneaking forward
/// at a run moved 197.7 units/s = 344 × 0.575, physics.md §6.3; hypothesis: the same
/// factor walking, backwards and sideways].
pub const DEFAULT_SNEAK_MULT: f32 = 0.57;

/// Game settings the controls use.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct ControlSettings {
    pub sneak_mult: f32,
    /// Right and up offset of the third-person camera.
    pub shoulder: [f32; 2],
}

impl ControlSettings {
    pub fn from_settings(sneak: Option<f32>, x: Option<f32>, z: Option<f32>) -> ControlSettings {
        let ok = |v: Option<f32>| v.filter(|v| v.is_finite());
        ControlSettings {
            sneak_mult: ok(sneak).filter(|v| *v > 0.0).unwrap_or(DEFAULT_SNEAK_MULT),
            shoulder: [ok(x).unwrap_or(DEFAULT_SHOULDER[0]), ok(z).unwrap_or(DEFAULT_SHOULDER[1])],
        }
    }
}

/// The body that shows the player, written by the body each frame (nvforever), read by
/// the camera.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct BodyView {
    /// Third-person pivot (game space): above the body, following crouching.
    pub pivot: Option<[f32; 3]>,
}

impl BodyView {
    /// The drawn body's eye (the pivot is 5 above it, at the feet), for the character
    /// creator's camera.
    pub fn portrait_eye(&self) -> Option<[f32; 3]> {
        self.pivot.map(|p| [p[0], p[1], p[2] - 5.0])
    }
}

pub fn init_settings(mut commands: Commands, sim: Res<SimState>) {
    let f = |n: &str| sim.forms.setting_f32(n);
    let s = ControlSettings::from_settings(f("fMoveSneakMult"), f("fOverShoulderPosX"), f("fOverShoulderPosZ"));
    info!("player controls: the game's bindings; sneak speed × {:.2}, third-person shoulder offset {:?}", s.sneak_mult, s.shoulder);
    commands.insert_resource(s);
}

/// Optional momentary touch/controller sneak sources. Native toggle input stays
/// independent underneath it; no resource means the ordinary behavior is unchanged.
#[derive(Resource, Default, Debug)]
pub struct HeldSneakInput {
    pub active: bool,
    pub(crate) controller_active: bool,
    base: Option<bool>,
    applied: bool,
}
impl HeldSneakInput {
    fn base(&self, visible: bool) -> bool {
        // A changed value from a script/lock becomes the new base, rather than being
        // overwritten with the remembered state from before the touch hold.
        self.base.filter(|_| visible == self.applied).unwrap_or(visible)
    }
    pub(crate) fn effective(&self, visible: bool, permitted: bool) -> bool {
        self.base(visible) || (self.active && permitted) || self.controller_active
    }
    fn record_base(&mut self, value: bool) {
        // Explicit script requests matter even if they equal the visible held state.
        if self.base.is_some() { self.base = Some(value); self.applied = value; }
    }
    fn apply(&mut self, base: bool, permitted: bool) -> bool {
        let held = (self.active && permitted) || self.controller_active;
        self.base = held.then_some(base);
        self.applied = base || held;
        self.applied
    }
}

/// The controls that are bound but not built yet.
const NOT_BUILT: [Control; 0] = [];

type ControlContext<'w, 's> = (
    ResMut<'w, PlayerControl>, Option<ResMut<'w, HeldSneakInput>>,
    Option<Res<'w, crate::pad::TouchDeviceSwitch>>, Option<Res<'w, crate::controller_aim::Options>>,
    Option<Res<'w, crate::controller_aim::Gestures>>, Local<'s, bool>,
);

/// The real clock and the tweaks, for the holster hold (`crate::tweaks`).
type HoldClock<'w> = (Option<Res<'w, Time<Real>>>, Option<Res<'w, crate::tweaks::TweakSettings>>);

/// Sprint's input (`crate::stance`, the contract): [`PlayerControl::sprint_held`] while the
/// Sprint control is held with Sprint on (on the key the player bound it to: none by default).
fn read_stance(input: &ControlInput, stance: Option<&crate::inertia::MovementInertia>, c: &mut PlayerControl) {
    c.sprint_held = stance.is_some_and(crate::inertia::MovementInertia::sprint_on) && input.pressed(Control::Sprint);
}

/// Toggles, holds, the third-person zoom; see the module notes.
#[allow(clippy::too_many_arguments)]
pub fn read_controls(
    input: ControlInput,
    time: Res<Time>,
    (real, tweaks): HoldClock,
    scroll: Res<AccumulatedMouseScroll>,
    state: Res<State<PlayState>>,
    (mut control, mut held_sneak, touch_switch, controller_options, controller_gestures, mut native_sneak_hold): ControlContext<'_, '_>,
    hud: Option<ResMut<HudMessage>>,
    switches: Option<Res<crate::script_switches::ScriptSwitches>>,
    stance: Option<Res<crate::inertia::MovementInertia>>,
) {
    if let Some(held) = held_sneak.as_deref_mut() {
        control.sneak = held.base(control.sneak);
    }
    if *state.get() != PlayState::Playing {
        if (cfg!(target_os = "android") || held_sneak.is_some()) && std::mem::take(&mut *native_sneak_hold) {
            control.sneak = false;
        }
        if let Some(held) = held_sneak.as_deref_mut() {
            held.controller_active = false;
            control.sneak = held.apply(control.sneak, false);
        }
        control.aiming = false;
        control.trigger = false;
        control.ready_held = None;
        control.reload_wait = None;
        control.orbit.held = false;
        control.sprint_held = false;
        return;
    }
    read_stance(&input, stance.as_deref(), &mut control);
    let mut message: Option<String> = None;
    let c = &mut *control;
    let was_out = c.weapon_out;
    let android_sneak_hold = cfg!(target_os = "android") || held_sneak.is_some();
    let controller_hold = android_sneak_hold && input.controller() && controller_options.as_deref().is_some_and(|o|o.hold_sneak);
    let sneak_pressed = if controller_hold { crate::bindings::keys_with_pad() && input.physical_pressed(Control::CrouchSneak) } else { input.pressed(Control::CrouchSneak) };
    let sneak_start = if controller_hold { crate::bindings::keys_with_pad() && input.physical_just_pressed(Control::CrouchSneak) } else { input.just_pressed(Control::CrouchSneak) };
    let sneak_end = if controller_hold { crate::bindings::keys_with_pad() && input.physical_just_released(Control::CrouchSneak) } else { input.just_released(Control::CrouchSneak) };
    let native_hold_mode = tweaks.as_ref().is_some_and(|t| t.hold_to_sneak);
    if *native_sneak_hold && (!native_hold_mode || !sneak_pressed) {
        c.sneak = false;
        *native_sneak_hold = false;
    }
    // Native hold affects only the restored base; virtual/controller holds apply below.
    if native_hold_mode {
        if sneak_start { *native_sneak_hold = true; }
        if sneak_pressed != c.sneak && (sneak_start || sneak_end) {
            c.sneak = sneak_pressed;
            info!("Crouch/Sneak held: sneaking {} asked", if c.sneak { "on" } else { "off" });
        }
    } else if sneak_start {
        c.sneak = !c.sneak;
        info!("Crouch/Sneak pressed: sneaking {} asked", if c.sneak { "on" } else { "off" });
    }
    if let Some(held) = held_sneak.as_deref_mut() {
        c.sneak = held.apply(c.sneak, input.virtual_action_allowed(Control::CrouchSneak)
            && touch_switch.as_deref().is_none_or(crate::pad::TouchDeviceSwitch::allows_actions));
    }
    if input.just_pressed(Control::AlwaysRun) {
        c.always_run = !c.always_run;
    }
    if input.just_pressed(Control::AutoMove) {
        c.auto_move = !c.auto_move;
    }
    // Ready Item: draw at once; with the weapon out, tap to reload, hold to holster.
    // Not while Quick Loot has the key (its full menu, `crate::quick_loot`).
    if input.just_pressed(Control::ReadyItem) && !crate::quick_loot::holds_input() {
        if c.weapon_out {
            c.ready_held = Some(0.0);
        } else if tweaks.as_ref().is_some_and(|t| t.ready_does_not_draw) {
            // The player's tweak: Ready Item does not draw [design: docs/design/tweaks.md].
            info!("Ready Item pressed: not drawn (tweak)");
        } else {
            c.weapon_out = true;
            info!("Ready Item pressed: the weapon drawn");
        }
    }
    if let Some(t) = c.ready_held.as_mut() {
        // The player's tweak times the hold on the real clock, not the slowed one [design: docs/design/tweaks.md].
        *t += match real.as_ref().filter(|_| tweaks.as_ref().is_some_and(|t| t.holster_real_time)) {
            Some(r) => r.delta_secs(),
            None => time.delta_secs(),
        };
        if *t >= HOLSTER_HOLD {
            c.weapon_out = false;
            c.ready_held = None;
            info!("Ready Item held: the weapon holstered");
        } else if input.just_released(Control::ReadyItem) || !input.pressed(Control::ReadyItem) {
            c.ready_held = None;
            c.lowered = false;
            // The player's tweak: a second tap within [`DOUBLE_TAP`] swaps the ammo, so a
            // tap waits for it before reloading [design: docs/design/tweaks.md].
            if tweaks.as_ref().is_some_and(|t| t.double_tap_ammo) {
                if c.reload_wait.take().is_some() {
                    c.ammo_swap = true;
                    info!("Ready Item tapped twice: ammo swap (tweak)");
                } else {
                    c.reload_wait = Some(0.0);
                }
            } else {
                c.reload = true;
            }
        }
    }
    if let Some(t) = c.reload_wait.as_mut() {
        *t += time.delta_secs();
        if *t >= DOUBLE_TAP && c.ready_held.is_none() {
            c.reload_wait = None;
            c.reload = true;
        }
    }
    // The Tweaks tab's Holster Weapon key [design: docs/design/tweaks.md].
    if input.just_pressed(Control::HolsterWeapon) && c.weapon_out {
        c.weapon_out = false;
        c.ready_held = None;
        info!("Holster Weapon pressed: the weapon holstered");
    }
    if input.just_pressed(Control::LowerWeapon) && c.weapon_out {
        c.lowered = !c.lowered;
        info!("Lower Weapon pressed: the weapon {}", if c.lowered { "lowered" } else { "raised" });
    }
    // Use draws the weapon when it is away; held with it out, it is the trigger. The
    // press that draws does not fire.
    if c.wheel {
        c.trigger = false;
    } else if input.just_pressed(Control::Use) && !c.weapon_out {
        c.weapon_out = true;
        c.trigger = false;
    } else {
        c.trigger = c.weapon_out && input.pressed(Control::Use) && (c.trigger || input.just_pressed(Control::Use));
    }
    for ctl in NOT_BUILT {
        if input.just_pressed(ctl) {
            message = Some(format!("{}: not available yet", Bindings::name(ctl)));
        }
    }
    c.aiming = if input.controller() {
        // A genuine mouse/key hold remains native even when pad aim is a toggle.
        controller_gestures.as_deref().map_or_else(||input.pad_pressed(Control::Block),|g|g.aim_requested)
            || (crate::bindings::keys_with_pad() && input.physical_pressed(Control::Block))
    } else {input.pressed(Control::Block)} && !c.wheel;
    // The player's tweak: aiming draws the weapon [design: docs/design/tweaks.md].
    if c.aiming && !c.weapon_out && tweaks.as_ref().is_some_and(|t| t.aiming_draws_weapon) {
        c.weapon_out = true;
        info!("Block/aim held: the weapon drawn (tweak)");
    }
    if c.lowered && (c.aiming || c.trigger || !c.weapon_out) {
        c.lowered = false;
    }
    // Toggle POV: a tap switches the view; held with the mouse moving, the camera circles
    // (player::mouse_look turns `orbit` instead of the view).
    if input.just_pressed(Control::TogglePov) {
        c.orbit.held = true;
        c.orbit.moved = false;
    }
    if c.orbit.held && input.just_released(Control::TogglePov) {
        c.orbit.held = false;
        if !c.orbit.moved {
            c.pov = match c.pov {
                Pov::First => Pov::Third,
                Pov::Third => Pov::First,
            };
        } else {
            c.orbit = c.orbit.released();
            // The player's tweak puts a first-person view back [design: docs/design/tweaks.md].
            c.pov = crate::vanity::end_view(c.pov == Pov::First, tweaks.as_ref().is_some_and(|t| t.vanity_returns_first));
        }
    }
    // Not while a script turned the vanity wheel off (ToggleVanityWheel,
    // `crate::script_switches`).
    let wheel = switches.is_none_or(|s| s.vanity_wheel);
    // The player's tweak: the wheel moves the camera only with Toggle View held [design: docs/design/tweaks.md].
    let wheel = wheel && !(tweaks.as_ref().is_some_and(|t| t.camera_wheel_needs_pov) && !input.pressed(Control::TogglePov));
    if wheel && c.third_person_view() && scroll.delta.y != 0.0 && !input.suppressed() && !crate::quick_loot::holds_input() {
        c.distance = zoom(c.distance, scroll.delta.y);
    }
    // The player's tweak: drawing goes to first person, holstering to third [design: docs/design/tweaks.md].
    if c.weapon_out != was_out && tweaks.as_ref().is_some_and(|t| t.pov_follows_weapon) {
        c.pov = if c.weapon_out { Pov::First } else { Pov::Third };
        info!("view: {:?} with the weapon {} (tweak)", c.pov, if c.weapon_out { "drawn" } else { "holstered" });
    }
    if let Some(m) = message {
        info!("{m}");
        if let Some(mut h) = hud {
            h.show(m);
        }
    }
}

/// Third-person distance after a wheel motion (+ towards the player).
pub fn zoom(distance: f32, wheel: f32) -> f32 {
    (distance * 0.9f32.powf(wheel)).clamp(MIN_DISTANCE, MAX_DISTANCE)
}

/// Where the third-person camera wants to be: from the pivot, offset right and up, then
/// back along the view by `distance` (game space).
pub fn third_person_spot(pivot: [f32; 3], heading: f32, pitch: f32, shoulder: [f32; 2], distance: f32) -> ([f32; 3], [f32; 3]) {
    let (s, c) = heading.sin_cos();
    let right = [c, -s, 0.0];
    let base = [pivot[0] + right[0] * shoulder[0], pivot[1] + right[1] * shoulder[0], pivot[2] + shoulder[1]];
    let d = space::look_dir(heading, pitch);
    (base, [base[0] - d[0] * distance, base[1] - d[1] * distance, base[2] - d[2] * distance])
}

/// The shoulder offset while circling: it fades to the centre as the camera turns away
/// from behind, so the camera keeps looking at the player [engineering choice].
pub fn orbit_shoulder(shoulder: [f32; 2], yaw: f32) -> [f32; 2] {
    let k = 1.0 - (yaw.abs() / 0.5).min(1.0);
    [shoulder[0] * k, shoulder[1]]
}

/// Whether the third-person camera comes away from the player's back with the weapon
/// away (the settings page's Gameplay switch; module notes).
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DetachedCamera(pub bool);

/// How briskly a detached-camera player turns to the way they are pushed: the turn is a
/// critically damped spring of this natural frequency, radians a second, so it eases out
/// (with [`FREE_TURN_SPEED_UP`], in too): an eighth of a turn settles in about a third of
/// a second, a quarter turn in 0.45 s, a half turn in 0.6 s [engineering choice: the owner 2026-10-27, a constant-rate turn "looks very
/// slightly unnatural"].
pub const FREE_TURN_SPRING: f32 = 14.0;
/// The fastest that turn goes, radians a second [engineering choice: 540°/s].
pub const FREE_TURN_MAX: f32 = 3.0 * std::f32::consts::PI;
/// How fast the turn may speed up, radians a second squared: to its top speed in about a
/// sixth of a second; slowing down is the spring's alone [engineering choice].
pub const FREE_TURN_SPEED_UP: f32 = 60.0;

/// An angle in (−π, π].
pub fn wrap_angle(a: f32) -> f32 {
    std::f32::consts::PI - (std::f32::consts::PI - a).rem_euclid(std::f32::consts::TAU)
}

/// One frame (`dt` seconds) of detached-camera movement: from the player's `heading`, the
/// camera's `yaw` off it, the pushed `forward`/`right` and the turn's `spin` (radians a
/// second, carried from frame to frame), the new heading (sprung towards the pushed way
/// as the camera sees it: [`FREE_TURN_SPRING`]), the yaw that keeps the camera where it
/// was, how hard the player is pushed straight ahead, and the new spin. Not pushed,
/// nothing turns and the spin is lost.
pub fn steer(heading: f32, yaw: f32, forward: f32, right: f32, spin: f32, dt: f32) -> (f32, f32, f32, f32) {
    let push = forward.hypot(right).min(1.0);
    if push == 0.0 {
        return (heading, yaw, 0.0, 0.0);
    }
    let camera = heading + yaw;
    let off = wrap_angle(camera + right.atan2(forward) - heading);
    // A half turn's way round is the one already turning (no dithering at ±π).
    let off = if off.abs() > 0.95 * std::f32::consts::PI && off * spin < 0.0 { off + std::f32::consts::TAU.copysign(spin) } else { off };
    let w = FREE_TURN_SPRING;
    let dt = dt.min(0.05);
    let sprung = spin + (w * w * off - 2.0 * w * spin) * dt;
    // Speeding up (the same way, or from turning the other way) at most so fast.
    let sprung = if sprung.abs() > spin.abs() || sprung * spin < 0.0 { sprung.clamp(spin - FREE_TURN_SPEED_UP * dt, spin + FREE_TURN_SPEED_UP * dt) } else { sprung };
    let spin = sprung.clamp(-FREE_TURN_MAX, FREE_TURN_MAX);
    // Never past the way pushed.
    let turn = if off >= 0.0 { (spin * dt).clamp(0.0, off) } else { (spin * dt).clamp(off, 0.0) };
    let h = (heading + turn).rem_euclid(std::f32::consts::TAU);
    (h, wrap_angle(camera - h), push, spin)
}

/// The detached camera (module notes), after the frame's input and the locks: on in third
/// person with the weapon away, the player's own view not taken by anything else; the
/// player turns to the way they move and walks straight ahead. Going off (the weapon
/// drawn, first person, the switch) turns the player to where the camera looked.
#[allow(clippy::too_many_arguments)]
pub fn detach_camera(
    time: Res<Time>,
    state: Res<State<PlayState>>,
    switch: Res<DetachedCamera>,
    seat: Res<crate::furniture::PlayerSeat>,
    driven: Option<Res<crate::player::ViewDriven>>,
    mut control: ResMut<PlayerControl>,
    mut player: ResMut<Player>,
    mut intent: ResMut<MoveIntent>,
    mut spin: Local<f32>,
) {
    if *state.get() != PlayState::Playing {
        return;
    }
    let c = *control;
    let free = switch.0 && c.pov == Pov::Third && !c.weapon_out && c.portrait.is_none() && !c.cinematic && seat.hold.is_none() && !driven.is_some_and(|d| d.0);
    if free != c.orbit.free {
        let o = &mut control.orbit;
        o.free = free;
        if !free {
            // The player faces where the camera looked, and the camera stays behind them.
            player.heading = (player.heading + o.yaw).rem_euclid(std::f32::consts::TAU);
            player.pitch = (player.pitch + o.pitch).clamp(-crate::player::MAX_PITCH, crate::player::MAX_PITCH);
            (o.yaw, o.pitch) = (0.0, 0.0);
        }
        info!("third-person camera: {}", if free { "detached, the player turns to the way they move" } else { "behind the player again" });
    }
    if !free {
        *spin = 0.0;
        return;
    }
    let (heading, yaw, push, s) = steer(player.heading, control.orbit.yaw, intent.forward, intent.right, *spin, time.delta_secs());
    *spin = s;
    if push > 0.0 {
        player.heading = heading;
        control.orbit.yaw = yaw;
        (intent.forward, intent.right) = (push, 0.0);
    }
}

/// The camera: at the eye in first person; behind the shoulder in third person (turned by
/// the orbit while circling), in front of whatever collision lies between the pivot and
/// that spot.
#[allow(clippy::too_many_arguments)]
pub fn place_camera(
    time: Res<Time>,
    player: Option<Res<Player>>,
    mut control: ResMut<PlayerControl>,
    settings: Option<Res<ControlSettings>>,
    body: Res<BodyView>,
    mut phys: ResMut<PhysState>,
    mut cams: Query<&mut Transform, With<PlayerCamera>>,
    mut told: Local<Option<bool>>,
    switches: Option<Res<crate::script_switches::ScriptSwitches>>,
) {
    // Say which line the crosshair follows when it changes (smoke tests read it).
    let third = control.crosshair.is_some();
    if *told != Some(third) {
        *told = Some(third);
        match control.crosshair {
            Some(c) => info!("crosshair: the third-person camera's line (shoulder {:?})", c.shoulder),
            None => info!("crosshair: the eye's view"),
        }
    }
    if !control.orbit.held && !control.orbit.free {
        // Swing back behind the player.
        let k = (-time.delta_secs() / ORBIT_RETURN).exp();
        let o = &mut control.orbit;
        (o.yaw, o.pitch) = (o.yaw * k, o.pitch * k);
        if o.yaw.abs() + o.pitch.abs() < 1e-3 {
            (o.yaw, o.pitch) = (0.0, 0.0);
        }
    }
    let (Some(p), Ok(mut t)) = (player, cams.single_mut()) else { return };
    let eye = p.eye();
    control.crosshair = None;
    if let Some(portrait) = control.portrait {
        // No collision: the creator's room is the player's own space [design]. Aimed from
        // the drawn body's eye ([`BodyView::portrait_eye`]): the first-person eye can lag
        // where an idle put it (the opening's wake-up) while the body already stands.
        let (at, heading, pitch) = portrait.spot(body.portrait_eye().unwrap_or(eye), p.heading);
        t.translation = space::to_bevy(at);
        t.rotation = space::look_rotation(heading, pitch);
        return;
    }
    let third_view = control.third_person_view();
    // On its way to or from a conversation's close-up the camera is between the two places.
    let close = control.talk_camera;
    if !third_view && (close <= 0.0 || close >= 1.0) {
        t.rotation = p.body_view.map_or_else(|| space::look_rotation(p.heading, p.pitch), space::view_rotation);
        t.translation = space::to_bevy(eye);
        return;
    }
    let o = control.orbit;
    let heading = control.view_heading(p.heading);
    let pitch = (p.pitch + o.pitch).clamp(-crate::player::MAX_PITCH, crate::player::MAX_PITCH);
    t.rotation = space::look_rotation(heading, pitch);
    let shoulder = settings.map_or(DEFAULT_SHOULDER, |s| s.shoulder);
    // Detached, the camera keeps its shoulder wherever it looks.
    let shoulder = if o.free { shoulder } else { orbit_shoulder(shoulder, o.yaw) };
    let pivot = body.pivot.unwrap_or(eye);
    let (base, want) = third_person_spot(pivot, heading, pitch, shoulder, control.distance);
    // The camera is not pushed in by actors' hit volumes (combat C3).
    let filter = Filter { exclude_layers: fnv_phys::layers::character_ignores().union(fnv_phys::layers::actor_volumes()), ..Default::default() };
    // From the pivot sideways to the shoulder, then back along the view.
    let mut from = pivot;
    let mut to = base;
    let mut out = base;
    for leg in 0..2 {
        let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        // ToggleCameraCollision 0: the camera goes through walls (`crate::script_switches`).
        let collides = switches.as_deref().is_none_or(|s| s.camera_collision);
        if len > 1e-3 && collides {
            let reach = match phys.ray(from, d, len + WALL_MARGIN, filter.clone()) {
                Some(h) => (h.distance - WALL_MARGIN).max(0.0).min(len),
                None => len,
            };
            out = [from[0] + d[0] / len * reach, from[1] + d[1] / len * reach, from[2] + d[2] / len * reach];
            if reach < len {
                break;
            }
        }
        if leg == 0 {
            (from, to) = (out, want);
        }
    }
    for (o, e) in out.iter_mut().zip(eye) {
        *o += (e - *o) * close;
    }
    t.translation = space::to_bevy(out);
    // While circling, the camera does not look where the player does: aim from the eye.
    // Detached, what the player uses is what the camera looks at.
    if third_view && (!control.orbit.active() || control.orbit.free) {
        control.crosshair = Some(Crosshair::placed(out, heading, pitch, pivot));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    #[test]
    fn the_view_heading_follows_the_detached_camera() {
        // First person: where the player faces.
        let mut c = PlayerControl { pov: Pov::First, ..Default::default() };
        assert_eq!(c.view_heading(1.0), 1.0);
        // Third person with the camera turned a quarter round off the player: the camera's.
        c.pov = Pov::Third;
        c.orbit = Orbit { free: true, yaw: FRAC_PI_2, ..Default::default() };
        assert!((c.view_heading(1.0) - (1.0 + FRAC_PI_2)).abs() < 1e-6);
        // Wrapped into a turn.
        c.orbit.yaw = -2.0;
        assert!((c.view_heading(1.0) - (std::f32::consts::TAU - 1.0)).abs() < 1e-5);
    }

    /// The distance from `p` to the line through `a` along unit `d`.
    fn off_line(p: [f32; 3], a: [f32; 3], d: [f32; 3]) -> f32 {
        let r = [p[0] - a[0], p[1] - a[1], p[2] - a[2]];
        let t = r[0] * d[0] + r[1] * d[1] + r[2] * d[2];
        let q = [r[0] - d[0] * t, r[1] - d[1] * t, r[2] - d[2] * t];
        (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt()
    }

    #[test]
    fn the_portrait_camera_stands_in_front_and_looks_back() {
        let p = Portrait { yaw: 0.0, distance: 50.0, drop: 10.0, pitch: 0.0, shift: 0.0 };
        // Facing +y (heading 0): the camera is 50 in front, level with the point under the eye.
        let (at, heading, pitch) = p.spot([0.0, 0.0, 120.0], 0.0);
        assert!(at[0].abs() < 1e-4 && (at[1] - 50.0).abs() < 1e-4 && (at[2] - 110.0).abs() < 1e-4, "{at:?}");
        let back = space::look_dir(heading, pitch);
        assert!((back[1] + 1.0).abs() < 1e-5);
        // Turned a quarter: beside the player, still looking at them.
        let (at, heading, _) = Portrait { yaw: FRAC_PI_2, ..p }.spot([0.0, 0.0, 120.0], 0.0);
        assert!((at[0] - 50.0).abs() < 1e-3 && at[1].abs() < 1e-3);
        assert!((space::look_dir(heading, 0.0)[0] + 1.0).abs() < 1e-5);
        // Stepped left: looking back along −y from +y, the camera's left is +x.
        let (at, ..) = Portrait { shift: 10.0, ..p }.spot([0.0, 0.0, 120.0], 0.0);
        assert!((at[0] - 10.0).abs() < 1e-3 && (at[1] - 50.0).abs() < 1e-3, "{at:?}");
    }

    #[test]
    fn the_crosshair_ray_is_the_cameras_line_level_with_the_eye() {
        let (pivot, shoulder) = ([0.0, 0.0, 100.0], [40.0, -10.0]);
        let (_, cam) = third_person_spot(pivot, 0.0, 0.0, shoulder, 170.0);
        let cross = Crosshair { camera: cam, dir: space::look_dir(0.0, 0.0), pivot, shoulder };
        let eye = [0.0, 5.0, 100.0];
        let (start, dir) = Crosshair::ray(Some(&cross), eye, [0.0, 1.0, 0.0]);
        // Right of the eye by the shoulder, level with it along the view.
        assert!((start[0] - 40.0).abs() < 1e-3 && (start[1] - 5.0).abs() < 1e-3 && (start[2] - 90.0).abs() < 1e-3, "{start:?}");
        assert_eq!(dir, cross.dir);
        // First person: the eye's own ray.
        assert_eq!(Crosshair::ray(None, eye, [0.0, 1.0, 0.0]), (eye, [0.0, 1.0, 0.0]));
    }

    #[test]
    fn a_shot_aims_from_the_eye_at_the_point_under_the_crosshair() {
        let (eye, start, dir) = ([0.0, 0.0, 100.0], [40.0, 0.0, 90.0], [0.0, 1.0, 0.0]);
        let hit = [40.0, 500.0, 90.0];
        let aim = Crosshair::aim_from(eye, start, dir, 1000.0, Some(hit));
        // It crosses the crosshair's line at the hit, not 40 units to its left.
        assert!(off_line(hit, eye, aim) < 1e-3, "{aim:?}");
        assert!(aim[0] > 0.0);
        // Nothing hit: towards the end of the reach.
        let far = Crosshair::aim_from(eye, start, dir, 1000.0, None);
        assert!(off_line([40.0, 1000.0, 90.0], eye, far) < 1e-2);
        // A hit beside the eye (not ahead of it): the view's direction.
        assert_eq!(Crosshair::aim_from(eye, start, dir, 1000.0, Some([40.0, 0.5, 90.0])), dir);
    }

    #[test]
    fn a_placed_camera_gives_its_lines_shoulder_offset() {
        let pivot = [10.0, -20.0, 120.0];
        for (h, p, sh) in [(0.3, -0.2, [40.0, -10.0]), (2.0, 0.5, [12.0, -10.0])] {
            let (_, cam) = third_person_spot(pivot, h, p, sh, 150.0);
            let c = Crosshair::placed(cam, h, p, pivot);
            assert!((c.shoulder[0] - sh[0]).abs() < 1e-3 && (c.shoulder[1] - sh[1]).abs() < 1e-3, "{:?}", c.shoulder);
        }
    }

    #[test]
    fn aiming_puts_the_point_on_the_cameras_line() {
        let (pivot, shoulder) = ([10.0, -20.0, 120.0], [40.0, -10.0]);
        for point in [[300.0, 400.0, 80.0], [-200.0, 150.0, 200.0], [60.0, -20.0, 120.0]] {
            let probe = Crosshair { camera: pivot, dir: [0.0, 1.0, 0.0], pivot, shoulder };
            let (h, p) = Crosshair::aim_at(Some(&probe), pivot, point);
            let (base, _) = third_person_spot(pivot, h, p, shoulder, 170.0);
            assert!(off_line(point, base, space::look_dir(h, p)) < 1e-2, "{point:?}");
        }
        // First person: straight at it from the eye.
        let (h, p) = Crosshair::aim_at(None, [0.0, 0.0, 0.0], [100.0, 0.0, 100.0]);
        assert!((h - FRAC_PI_2).abs() < 1e-5 && (p - std::f32::consts::FRAC_PI_4).abs() < 1e-5);
    }

    #[test]
    fn a_detached_player_turns_to_the_way_pushed_and_the_camera_stays() {
        use std::f32::consts::{FRAC_PI_4, PI};
        // At 60 frames a second: the frames' headings, from facing a little off the
        // camera's way (north: heading 0.3 + yaw −0.3) with forward/right pushed a second.
        let run = |forward: f32, right: f32| {
            let (mut h, mut yaw, mut spin) = (0.3, -0.3, 0.0);
            (0..60)
                .map(|_| {
                    let push;
                    (h, yaw, push, spin) = steer(h, yaw, forward, right, spin, 1.0 / 60.0);
                    // The camera stays put; pushed straight ahead, never faster on a diagonal.
                    assert!(wrap_angle(h + yaw).abs() < 1e-4 && push == 1.0, "{h} {yaw} {push}");
                    h
                })
                .collect::<Vec<f32>>()
        };
        // Back: about a half turn to face south, easing in (the first frame at no more than
        // half the top speed), at most 540°/s, settled within 0.6 s, one way round.
        let back = run(-1.0, 0.0);
        assert!(wrap_angle(back[0] - 0.3).abs() <= 0.5 * FREE_TURN_MAX / 60.0, "{}", back[0]);
        assert!(back.windows(2).all(|w| wrap_angle(w[1] - w[0]).abs() <= FREE_TURN_MAX / 60.0 + 1e-5));
        assert!(back[36..].iter().all(|h| wrap_angle(h - PI).abs() < 3f32.to_radians()), "{:?}", &back[25..35]);
        assert!(back.windows(2).all(|w| wrap_angle(w[1] - w[0]) >= -1e-5), "one way round");
        // Forward and right: north-east, settled within 0.4 s, never past it.
        let diag = run(1.0, 1.0);
        assert!(diag[24..].iter().all(|h| (h - FRAC_PI_4).abs() < 2f32.to_radians()), "{:?}", &diag[15..25]);
        assert!(diag.iter().all(|h| *h <= FRAC_PI_4 + 1e-4));
        // Not pushed: nothing turns and the spin goes.
        assert_eq!(steer(1.0, 2.0, 0.0, 0.0, 3.0, 0.1), (1.0, 2.0, 0.0, 0.0));
        assert!((wrap_angle(3.0 * PI / 2.0) + FRAC_PI_2).abs() < 1e-5 && (wrap_angle(PI) - PI).abs() < 1e-5);
    }

    #[test]
    fn a_conversations_close_up_leaves_the_players_own_view_alone() {
        let mut c = PlayerControl { pov: Pov::Third, ..Default::default() };
        assert!(c.third_person_view() && c.own_third_person());
        // On the way the body is still drawn; close to the eye it goes.
        c.talk_camera = TALK_CAMERA_CLOSE - 0.01;
        assert!(c.third_person_view());
        c.talk_camera = TALK_CAMERA_CLOSE;
        assert!(!c.third_person_view() && c.own_third_person() && c.pov == Pov::Third);
        // The view looks along the heading then.
        c.orbit.yaw = 0.5;
        assert_eq!(c.view_heading(1.0), 1.0);
        // A camera that shows the player from outside still does.
        c.cinematic = true;
        assert!(c.third_person_view());
    }

    #[test]
    fn third_person_sits_behind_and_right() {
        // Facing north, level: right is east, behind is south.
        let (base, cam) = third_person_spot([0.0, 0.0, 100.0], 0.0, 0.0, [40.0, -10.0], 170.0);
        assert!((base[0] - 40.0).abs() < 1e-4 && base[1].abs() < 1e-4 && (base[2] - 90.0).abs() < 1e-4);
        assert!((cam[0] - 40.0).abs() < 1e-4 && (cam[1] + 170.0).abs() < 1e-4 && (cam[2] - 90.0).abs() < 1e-4);
        // Facing east and looking down: behind is west and up.
        let (_, cam) = third_person_spot([0.0, 0.0, 0.0], FRAC_PI_2, -0.5, [0.0, 0.0], 100.0);
        assert!(cam[0] < -50.0 && cam[2] > 40.0, "{cam:?}");
        // Circling half way round: in front of the player, centred.
        assert_eq!(orbit_shoulder([40.0, -10.0], std::f32::consts::PI), [0.0, -10.0]);
        assert_eq!(orbit_shoulder([40.0, -10.0], 0.0), [40.0, -10.0]);
    }

    #[test]
    fn zoom_steps_and_clamps() {
        assert!(zoom(170.0, 1.0) < 170.0 && zoom(170.0, -1.0) > 170.0);
        assert_eq!(zoom(170.0, 100.0), MIN_DISTANCE);
        assert_eq!(zoom(170.0, -100.0), MAX_DISTANCE);
    }

    #[test]
    fn settings_fall_back_to_the_masters() {
        let s = ControlSettings::from_settings(None, Some(f32::NAN), Some(-12.0));
        assert_eq!(s, ControlSettings { sneak_mult: DEFAULT_SNEAK_MULT, shoulder: [40.0, -12.0] });
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<Bindings>()
            .init_resource::<AccumulatedMouseScroll>()
            .init_resource::<PlayerControl>()
            .init_resource::<Time>()
            .insert_state(PlayState::Playing)
            .add_systems(Update, read_controls);
        app
    }

    /// Press and release `key` over two frames (clearing the frame's edges as the input
    /// plugin would).
    fn tap(app: &mut App, key: KeyCode) {
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
        app.update();
        let mut k = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        k.clear();
        k.release(key);
        app.update();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
    }

    fn control(app: &App) -> PlayerControl {
        *app.world().resource::<PlayerControl>()
    }

    #[test]
    fn momentary_sneak_keeps_the_independent_native_toggle_base() {
        let mut app=app();app.init_resource::<HeldSneakInput>();
        app.world_mut().resource_mut::<HeldSneakInput>().active=true;app.update();assert!(control(&app).sneak);
        tap(&mut app,KeyCode::ControlLeft);
        app.world_mut().resource_mut::<HeldSneakInput>().active=false;app.update();
        assert!(control(&app).sneak,"keyboard toggle on survives releasing touch hold");
        tap(&mut app,KeyCode::ControlLeft);assert!(!control(&app).sneak);
        app.world_mut().resource_mut::<HeldSneakInput>().active=true;app.update();
        tap(&mut app,KeyCode::ControlLeft);tap(&mut app,KeyCode::ControlLeft);
        assert!(control(&app).sneak,"touch still holds sneak over a toggled-off base");
        app.world_mut().resource_mut::<HeldSneakInput>().active=false;app.update();assert!(!control(&app).sneak);
    }
    #[test]
    fn explicit_script_sneak_requests_survive_even_when_the_held_value_is_the_same() {
        for requested in [false,true] {
            let mut app=app();app.init_resource::<HeldSneakInput>();
            app.world_mut().resource_mut::<HeldSneakInput>().active=true;app.update();assert!(control(&app).sneak);
            // tell_scripts records JIP's explicit state request, including true->true.
            app.world_mut().resource_mut::<PlayerControl>().sneak=requested;
            app.world_mut().resource_mut::<HeldSneakInput>().record_base(requested);
            app.update();assert!(control(&app).sneak,"held input still owns the visible momentary state");
            app.world_mut().resource_mut::<HeldSneakInput>().active=false;app.update();
            assert_eq!(control(&app).sneak,requested);
        }
    }
    #[test]
    fn momentary_sneak_respects_capture_disabled_bindings_controller_and_menu() {
        for case in 0..4 {
            let mut app=app();app.init_resource::<HeldSneakInput>();
            app.world_mut().resource_mut::<HeldSneakInput>().active=true;
            match case {
                0=>{app.insert_resource(crate::bindings::KeyboardCapture(true));},
                1=>{app.insert_resource(crate::script_input::ScriptBlocked {keys:vec![KeyCode::ControlLeft],mouse:vec![]});},
                2=>app.world_mut().resource_mut::<Bindings>().device=crate::bindings::InputDevice::Controller,
                _=>{app.insert_state(PlayState::Menu);},
            }
            app.update();assert!(!control(&app).sneak,"restriction {case} cannot be bypassed by platform input");
        }
    }
    #[test]
    fn momentary_sneak_uses_the_same_primary_binding_as_touch_translation() {
        let mut app=app();app.init_resource::<HeldSneakInput>();
        app.world_mut().resource_mut::<Bindings>().set(Control::CrouchSneak,crate::bindings::Binding {key:Some(KeyCode::ControlLeft),mouse:Some(MouseButton::Right)});
        app.insert_resource(crate::script_input::ScriptBlocked {keys:vec![KeyCode::ControlLeft],mouse:vec![]});
        app.world_mut().resource_mut::<HeldSneakInput>().active=true;app.update();
        assert!(!control(&app).sneak,"touch cannot bypass its disabled primary through a secondary binding");
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);app.update();
        assert!(control(&app).sneak,"the independent physical secondary binding keeps native behavior");
    }
    #[test]
    fn momentary_sneak_does_not_restore_a_base_that_a_lock_changed() {
        let mut app=app();app.init_resource::<HeldSneakInput>();
        app.world_mut().resource_mut::<PlayerControl>().sneak=true;
        app.world_mut().resource_mut::<HeldSneakInput>().active=true;app.update();assert!(control(&app).sneak);
        app.world_mut().resource_mut::<PlayerControl>().sneak=false;
        app.world_mut().resource_mut::<HeldSneakInput>().active=false;app.update();assert!(!control(&app).sneak);
    }
    #[cfg(not(target_os = "android"))]
    #[test]
    fn desktop_hold_to_sneak_is_not_dropped_on_menu_entry() {
        let mut app = app();
        app.insert_resource(crate::tweaks::TweakSettings { hold_to_sneak: true, ..default() });
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ControlLeft);
        app.update();
        assert!(control(&app).sneak, "hold to sneak starts sneaking while held");
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
        app.insert_state(PlayState::Menu);
        app.update();
        assert!(control(&app).sneak, "desktop does not clear hold-to-sneak on entering a menu");
    }
    #[test]
    fn game_keys_toggle() {
        let mut app = app();
        tap(&mut app, KeyCode::ControlLeft);
        assert!(control(&app).sneak);
        tap(&mut app, KeyCode::CapsLock);
        assert!(!control(&app).always_run);
        tap(&mut app, KeyCode::KeyQ);
        assert!(control(&app).auto_move);
        // Ready Item draws; a tap with the weapon out does not put it away.
        tap(&mut app, KeyCode::KeyR);
        assert!(control(&app).weapon_out);
        tap(&mut app, KeyCode::KeyR);
        assert!(control(&app).weapon_out && control(&app).ready_held.is_none());
        // Toggle POV tapped without moving the mouse switches the view.
        tap(&mut app, KeyCode::KeyF);
        assert_eq!(control(&app).pov, Pov::Third);
    }

    /// Sprint's input (`crate::stance`): no key by default, so Shift (Run's) holds none; a key
    /// the player binds holds it while Sprint is on, nothing with it off; the movement lock
    /// strikes it.
    #[test]
    fn sprint_input_follows_its_switch_and_key() {
        let mut app = app();
        app.init_resource::<crate::inertia::MovementInertia>();
        let step = |app: &mut App, secs: f32| {
            app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(secs));
            app.update();
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
        };
        fn keys(app: &mut App) -> Mut<'_, ButtonInput<KeyCode>> {
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>()
        }
        // Unbound by default (#3000): Shift, Run's key, holds no sprint, on or off.
        keys(&mut app).press(KeyCode::ShiftLeft);
        for _ in 0..4 {
            step(&mut app, 0.4);
        }
        assert!(!control(&app).sprint_held, "off and unbound: nothing");
        app.world_mut().resource_mut::<crate::inertia::MovementInertia>().sprint = true;
        step(&mut app, 0.1);
        assert!(!control(&app).sprint_held, "on but unbound: Shift is Run's, no sprint");
        keys(&mut app).release_all();
        step(&mut app, 0.1);
        // A key the player binds holds it while Sprint is on, and Run keeps Shift.
        app.world_mut().resource_mut::<Bindings>().rebind(Control::Sprint, crate::bindings::Input::Key(KeyCode::KeyG));
        assert_eq!(app.world().resource::<Bindings>().get(Control::Run).key, Some(KeyCode::ShiftLeft));
        keys(&mut app).press(KeyCode::KeyG);
        step(&mut app, 0.1);
        assert!(control(&app).sprint_held, "the bound key sprints");
        keys(&mut app).release(KeyCode::KeyG);
        step(&mut app, 0.1);
        assert!(!control(&app).sprint_held);
        // Off again: the bound key holds nothing.
        app.world_mut().resource_mut::<crate::inertia::MovementInertia>().sprint = false;
        keys(&mut app).press(KeyCode::KeyG);
        step(&mut app, 0.1);
        assert!(!control(&app).sprint_held, "off: nothing");
        keys(&mut app).release_all();
        step(&mut app, 0.1);
        // The sneaking lock leaves it; the movement lock strikes it.
        let mut i = MoveIntent::default();
        let mut c = PlayerControl { sprint_held: true, ..control(&app) };
        ControlLocks { sneaking: true, ..Default::default() }.apply(&mut i, &mut c);
        assert!(c.sprint_held);
        ControlLocks { movement: true, ..Default::default() }.apply(&mut i, &mut c);
        assert!(!c.sprint_held);
    }

    #[test]
    fn orbit_from_first_person_stays_third() {
        let mut app = app();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Middle);
        app.update();
        assert!(control(&app).orbit.held);
        // mouse_look's part: the camera circled half round.
        let mut c = app.world_mut().resource_mut::<PlayerControl>();
        (c.orbit.moved, c.orbit.yaw, c.orbit.pitch) = (true, 3.0, 0.3);
        let mut m = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        m.clear();
        m.release(MouseButton::Middle);
        app.update();
        // Let go in first person: the view stays third person, the camera swinging back.
        let c = control(&app);
        assert!(!c.orbit.held && c.pov == Pov::Third && c.orbit.active() && c.third_person_view());
    }

    #[test]
    fn orbit_released_swings_back() {
        let o = Orbit { yaw: 1.0, pitch: 0.2, held: true, moved: true, free: false };
        assert_eq!(o.released(), Orbit { held: false, ..o });
        assert!(o.released().active());
    }

    #[test]
    fn locks_follow_the_parameter_order() {
        let l = ControlLocks::from_flags([true, false, true, false, false, false, true]);
        assert_eq!(l, ControlLocks { movement: true, fighting: true, sneaking: true, ..Default::default() });
        assert_eq!(l.names(), ["movement", "fighting", "sneaking"]);
        assert!(l.any() && !ControlLocks::default().any());
        assert_eq!(ControlLocks::from_flags([true; 7]), ControlLocks::ALL);
        assert_eq!(ControlLocks::ALL.names().len(), 7);
    }

    /// Everything a player can ask for at once.
    fn busy() -> (MoveIntent, PlayerControl) {
        let intent = MoveIntent { forward: 1.0, right: -1.0, walk: true, sneak: true, jump: true, ..default() };
        let control = PlayerControl {
            sneak: true,
            weapon_out: true,
            aiming: true,
            auto_move: true,
            pov: Pov::Third,
            orbit: Orbit { yaw: 1.0, pitch: 0.2, held: true, moved: true, free: false },
            ready_held: Some(0.1),
            trigger: true,
            reload: true,
            ammo_swap: true,
            ..Default::default()
        };
        (intent, control)
    }

    #[test]
    fn each_lock_strikes_its_own_group() {
        let (i0, c0) = busy();
        // Nothing locked: nothing changes.
        let (mut i, mut c) = (i0, c0);
        assert_eq!(ControlLocks::default().apply(&mut i, &mut c), [false; 7]);
        assert_eq!((i, c), (i0, c0));
        // Movement: no moving, jumping or Auto Move; the rest is the player's.
        let (mut i, mut c) = (i0, c0);
        let struck = ControlLocks { movement: true, ..Default::default() }.apply(&mut i, &mut c);
        assert_eq!(struck, [true, false, false, false, false, false, false]);
        // Asked again with nothing left to strike: nothing is counted.
        assert_eq!(ControlLocks { movement: true, ..Default::default() }.apply(&mut i, &mut c), [false; 7]);
        assert_eq!(i, MoveIntent { forward: 0.0, right: 0.0, jump: false, ..i0 });
        assert_eq!(c, PlayerControl { auto_move: false, ..c0 });
        // Fighting: the weapon goes away and nothing fires.
        let (mut i, mut c) = (i0, c0);
        ControlLocks { fighting: true, ..Default::default() }.apply(&mut i, &mut c);
        assert_eq!(i, i0);
        assert_eq!(c, PlayerControl { weapon_out: false, aiming: false, ready_held: None, trigger: false, reload: false, ammo_swap: false, ..c0 });
        // POV: back to first person, the camera behind the eyes.
        let (mut i, mut c) = (i0, c0);
        ControlLocks { pov: true, ..Default::default() }.apply(&mut i, &mut c);
        assert_eq!(i, i0);
        assert_eq!(c, PlayerControl { pov: Pov::First, orbit: Orbit::default(), ..c0 });
        assert!(!c.third_person_view());
        // Sneaking: the player stands up.
        let (mut i, mut c) = (i0, c0);
        ControlLocks { sneaking: true, ..Default::default() }.apply(&mut i, &mut c);
        assert_eq!(i, MoveIntent { sneak: false, ..i0 });
        assert_eq!(c, PlayerControl { sneak: false, ..c0 });
        // Pip-Boy, looking and rollover text are held where they are read (menu.rs,
        // player.rs, activate.rs).
        let (mut i, mut c) = (i0, c0);
        assert_eq!(ControlLocks { pipboy: true, looking: true, rollover: true, ..Default::default() }.apply(&mut i, &mut c), [false; 7]);
        assert_eq!((i, c), (i0, c0));
        // Everything at once.
        let (mut i, mut c) = (i0, c0);
        assert_eq!(ControlLocks::ALL.apply(&mut i, &mut c), [true, false, true, true, false, false, true]);
    }

    fn locked_app(locks: ControlLocks) -> App {
        let mut app = app();
        app.init_resource::<MoveIntent>()
            .init_resource::<crate::furniture::PlayerSeat>()
            .init_resource::<LockStats>()
            .insert_resource(locks)
            .add_systems(Update, enforce_locks.after(read_controls));
        app
    }

    #[test]
    fn locked_keys_do_nothing() {
        // DisablePlayerControls with no parameters: movement, Pip-Boy, fighting, POV.
        let mut app = locked_app(ControlLocks::from_flags([true, true, true, true, false, false, false]));
        tap(&mut app, KeyCode::KeyR);
        tap(&mut app, KeyCode::KeyF);
        tap(&mut app, KeyCode::KeyQ);
        let c = control(&app);
        assert!(!c.weapon_out && c.pov == Pov::First && !c.auto_move);
        // Sneaking is still the player's.
        tap(&mut app, KeyCode::ControlLeft);
        assert!(control(&app).sneak);
        // The sneaking flag stands the player up, and Ctrl does nothing.
        app.world_mut().resource_mut::<ControlLocks>().sneaking = true;
        app.update();
        assert!(!control(&app).sneak);
        tap(&mut app, KeyCode::ControlLeft);
        assert!(!control(&app).sneak);
        // Enabled again: the keys work, and nothing came back by itself.
        *app.world_mut().resource_mut::<ControlLocks>() = ControlLocks::default();
        app.update();
        let c = control(&app);
        assert!(!c.sneak && !c.weapon_out && c.pov == Pov::First);
        tap(&mut app, KeyCode::KeyR);
        tap(&mut app, KeyCode::ControlLeft);
        assert!(control(&app).weapon_out && control(&app).sneak);
    }

    #[test]
    fn a_lock_puts_the_weapon_away_and_the_view_inside() {
        let mut app = locked_app(ControlLocks::default());
        tap(&mut app, KeyCode::KeyR);
        tap(&mut app, KeyCode::KeyF);
        assert!(control(&app).weapon_out && control(&app).pov == Pov::Third);
        *app.world_mut().resource_mut::<ControlLocks>() = ControlLocks { fighting: true, pov: true, ..Default::default() };
        app.update();
        assert!(!control(&app).weapon_out && control(&app).pov == Pov::First);
    }

    #[test]
    fn locked_movement_keeps_the_player_seated() {
        let mut app = locked_app(ControlLocks { movement: true, ..Default::default() });
        // A movement key was pressed on furniture and a test aid holds Forward.
        app.world_mut().resource_mut::<crate::furniture::PlayerSeat>().leave = true;
        *app.world_mut().resource_mut::<MoveIntent>() = MoveIntent { forward: 1.0, jump: true, ..Default::default() };
        app.update();
        assert!(!app.world().resource::<crate::furniture::PlayerSeat>().leave);
        assert_eq!(*app.world().resource::<MoveIntent>(), MoveIntent::default());
        let stats = *app.world().resource::<LockStats>();
        assert_eq!(stats.held, [1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(stats.summary(), "movement 1");
        assert_eq!(LockStats::default().summary(), "nothing");
    }
}
