//! Part P (docs/design/layer4-play.md §4): the player — input, character movement, the
//! first-person camera.
//!
//! Each frame: keys and mouse become a [`MoveIntent`] and the look angles (Input set);
//! the character is placed on the ground once its cell is loaded, then moved in fixed
//! steps through the `Stepper` (Physics set); the simulation's player reference follows
//! the feet; a player body, when one is drawn, poses itself in [`BodySet`] and gives the
//! eye; the camera (`control::place_camera`) sits at the eye, interpolated between the
//! last two steps, or behind the player in third person.
//! Nothing moves in `PlayState::Menu` (physics time stops). Movement values are
//! [hypothesis] (docs/spec-notes/physics.md §6).
//!
//! Safety nets ([`safe`]): an interior start or door destination with nothing under it
//! moves to the nearest door arrival into the cell; a player who falls out of an
//! interior's geometry goes back to where they last stood. Test aid
//! `NVF_PLAY_TEST_NEAR=<reference FormID, hex>`: start on a standing spot from which that
//! reference can be activated, looking at it.

pub mod safe;

#[cfg(test)]
#[path = "player_control_event_tests.rs"]
mod control_event_tests;

use crate::bindings::{Bindings, Control, ControlInput};
use crate::control::PlayerControl;
use crate::phys::cells::Space;
use crate::phys::PhysState;
use crate::sim::SimState;
use crate::{space, PlaySet, PlayState, PlayWorld, Travel};
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::tasks::{ComputeTaskPool, ParallelSliceMut};
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use fnv_phys::{CharacterInput, CharacterStep, Owner, Stroke};
use fnv_sim::FormId;
use fnv_world::cell_of;
use std::collections::BTreeSet;

/// Eye height above the feet, game units [hypothesis: placeholder for a standing human at
/// about 70 units per metre; not captured].
pub const EYE_HEIGHT: f32 = 112.0;
/// Test aid `NVF_PLAY_TEST_NEAR`: how far the first-person body's eye stands above
/// [`EYE_HEIGHT`] [observed: 121 above the feet, GSGasStation 2026-10-12], and the reach
/// kept to spare when choosing the spot [engineering choice].
const NEAR_EYE_ABOVE: f32 = 9.0;
const NEAR_REACH_SPARE: f32 = 5.0;

/// FalloutNV.esm's `fMoveBaseSpeed` and `fMoveRunMult` [observed: GMST records, physics.md
/// §6], used when the loaded plugins lack them.
pub const DEFAULT_MOVE_BASE_SPEED: f32 = 77.0;
pub const DEFAULT_MOVE_RUN_MULT: f32 = 4.0;

/// Mouse look, radians per mouse count: the game's `fMouseSensitivity` (bindings.rs).
pub const LOOK_RADIANS_PER_COUNT: f32 = crate::bindings::MOUSE_SENSITIVITY;
/// Steepest look up or down [engineering choice].
pub const MAX_PITCH: f32 = 89.0 * std::f32::consts::PI / 180.0;

/// Set while an animation drives the first-person view (the opening's wake-up, an idle
/// with a `Camera1st` track; nvforever `idles.rs`, docs/spec-notes/idles.md §4): mouse
/// look, the look test aid and the movement keys are ignored, so the clip alone places
/// the camera [design: the owner's playtest of 2026-10-11, "locked in place so the idle
/// kf has control of camera position"].
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewDriven(pub bool);

/// The console's ToggleCollision (`tcl`, `crate::console`): the character passes through
/// everything, falls no more and flies where the player looks, by the swimming rule
/// (forward and back along the view, sideways level) at the walking and running speeds [hypothesis: as the
/// original's `tcl` plays; its speed not measured]. Off again, it falls from where it is.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoClip(pub bool);

/// The player's pose in game space.
#[derive(Resource, Debug, Clone, Copy)]
pub struct Player {
    /// Compass heading, radians (0 north, π/2 east).
    pub heading: f32,
    /// Pitch, radians (negative looks down).
    pub pitch: f32,
    pub eye_height: f32,
    /// Feet position, game space (interpolated between fixed steps: where the camera
    /// shows the player; `PhysState::player` holds the stepped position).
    pub feet: [f32; 3],
    /// The eye of the player's body this frame (game space), when a body is drawn
    /// (nvforever's player body writes it in [`BodySet`]): its head, crouching and
    /// swaying with the animation.
    pub body_eye: Option<[f32; 3]>,
    /// The rotation of the first-person view this frame (game space, of a node looking
    /// along its +Y with +Z up), when the body's animation turns the view (an idle that
    /// drives the camera node, a body lying on a bed); the heading and pitch give it
    /// otherwise.
    pub body_view: Option<Quat>,
    /// Where the game last put the player (the start of a game, a door, a script's
    /// move), until the player moves by themselves. The collision may have pushed the
    /// character away from it since (a start marker in a bed): a scripted scene plays
    /// where the game put the player, not where the collision left them.
    pub placed: Option<[f32; 3]>,
}

impl Player {
    /// Eye position, game space: the body's when one is drawn, else `eye_height` above
    /// the feet.
    pub fn eye(&self) -> [f32; 3] {
        self.body_eye.unwrap_or([self.feet[0], self.feet[1], self.feet[2] + self.eye_height])
    }

    /// Unit view direction, game space.
    pub fn look_dir(&self) -> [f32; 3] {
        space::look_dir(self.heading, self.pitch)
    }
}

/// Movement speeds, game units per second.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct MoveSpeeds {
    pub walk: f32,
    pub run: f32,
}

impl MoveSpeeds {
    /// The player's speeds measured in game with no one at the controls: running (Always
    /// Run on, forward held) 344 units/s, walking (the Run key held as well) 86.8 units/s,
    /// at full speed from the first loop [observed: capture mv_trial, 2026-10-08, a level-1
    /// character in Doc Mitchell's save, Speed Mult 100; physics.md §6.3]. They are 11–13%
    /// above `fMoveBaseSpeed` (77) and `× fMoveRunMult` (308); the game's formula is not
    /// known [hypothesis: animation, armour, weapon and encumbrance may all enter it].
    pub const OBSERVED: MoveSpeeds = MoveSpeeds { walk: 86.8, run: 344.0 };

    /// Walking at `fMoveBaseSpeed`, running at `fMoveBaseSpeed × fMoveRunMult`
    /// [hypothesis: meanings guessed from the setting names; the game's formula (Speed
    /// Mult, encumbrance, sneaking, weapon drawn) is not known — physics.md §6]. Missing
    /// settings take FalloutNV.esm's values.
    pub fn from_settings(base: Option<f32>, run_mult: Option<f32>) -> MoveSpeeds {
        let base = base.filter(|v| v.is_finite() && *v > 0.0).unwrap_or(DEFAULT_MOVE_BASE_SPEED);
        let mult = run_mult.filter(|v| v.is_finite() && *v > 0.0).unwrap_or(DEFAULT_MOVE_RUN_MULT);
        MoveSpeeds { walk: base, run: base * mult }
    }
}

/// What the player asks for this frame.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct MoveIntent {
    /// +1 forward (W, or Auto Move), −1 back (S).
    pub forward: f32,
    /// +1 right (D), −1 left (A).
    pub right: f32,
    /// Walk instead of run: the Run key (Shift) held with Always Run on, or not held with
    /// it off [observed: Always Run is on by default and holding Run walks, physics.md
    /// §6.3; that Run runs with Always Run off is hypothesis].
    pub walk: bool,
    /// Sneaking (toggled by Ctrl).
    pub sneak: bool,
    /// Jump (Space) on the next fixed step; kept until a step uses it.
    pub jump: bool,
}

/// The player's toggles: Always Run (Caps Lock), sneaking (Ctrl), Auto Move (Q).
/// Kept across menus [hypothesis].
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct Controls {
    /// [observed: `bAlwaysRunByDefault=1` in the original's default INI and the owner's]
    pub always_run: bool,
    pub sneaking: bool,
    pub auto_move: bool,
}

impl Default for Controls {
    fn default() -> Self {
        Controls { always_run: true, sneaking: false, auto_move: false }
    }
}

/// The keys of the original's control map for the controls NVForever has, by physical
/// key (Bevy's `KeyCode` names the key's position, as DirectInput scan codes do)
/// [observed: the owner's `FalloutPrefs.ini` [Controls], read-only: Forward 0x11 (W),
/// Back 0x1F (S), Slide Left 0x1E (A), Slide Right 0x20 (D), Activate 0x12 (E),
/// Crouch/Sneak 0x1D (left Ctrl), Run 0x2A (left Shift), Always Run 0x3A (Caps Lock),
/// Auto Move 0x10 (Q), Jump 0x39 (Space), Menu Mode 0x0F (Tab); the byte is the
/// DirectInput scan code (DIK_*); that these are the shipped defaults and not the
/// owner's own choice is hypothesis]. Activate (E) is `crate::activate`'s, Menu Mode
/// (Tab) `crate::menu`'s.
pub mod keys {
    use bevy::prelude::KeyCode;
    pub const FORWARD: KeyCode = KeyCode::KeyW;
    pub const BACK: KeyCode = KeyCode::KeyS;
    pub const LEFT: KeyCode = KeyCode::KeyA;
    pub const RIGHT: KeyCode = KeyCode::KeyD;
    pub const JUMP: KeyCode = KeyCode::Space;
    /// Also the right Shift (unbound in the original) [engineering choice].
    pub const RUN: [KeyCode; 2] = [KeyCode::ShiftLeft, KeyCode::ShiftRight];
    pub const ALWAYS_RUN: KeyCode = KeyCode::CapsLock;
    pub const SNEAK: KeyCode = KeyCode::ControlLeft;
    pub const AUTO_MOVE: KeyCode = KeyCode::KeyQ;
}

/// This frame's intent from the keys held (`pressed`) and pressed this frame (`just`),
/// updating the toggles: Caps Lock flips Always Run, Ctrl flips sneaking [observed: the
/// Crouch/Sneak control toggles, `IsSneaking` 1 after one tap and 0 after the next,
/// capture mv_values], Q flips Auto Move, which walks forward until Forward or Back is
/// pressed [hypothesis]. `jump` is only ever set here (a step clears it).
pub fn read_keys(c: &mut Controls, intent: &mut MoveIntent, pressed: impl Fn(KeyCode) -> bool, just: impl Fn(KeyCode) -> bool) {
    if just(keys::ALWAYS_RUN) {
        c.always_run = !c.always_run;
        info!("always run {}", if c.always_run { "on" } else { "off" });
    }
    if just(keys::SNEAK) {
        c.sneaking = !c.sneaking;
        info!("sneaking {}", if c.sneaking { "on" } else { "off" });
    }
    if just(keys::AUTO_MOVE) {
        c.auto_move = !c.auto_move;
    } else if c.auto_move && (just(keys::FORWARD) || just(keys::BACK)) {
        c.auto_move = false;
    }
    let axis = |plus: KeyCode, minus: KeyCode| pressed(plus) as i32 as f32 - pressed(minus) as i32 as f32;
    intent.forward = if c.auto_move { 1.0 } else { axis(keys::FORWARD, keys::BACK) };
    intent.right = axis(keys::RIGHT, keys::LEFT);
    intent.walk = keys::RUN.into_iter().any(&pressed) == c.always_run;
    intent.sneak = c.sneaking;
    if just(keys::JUMP) {
        intent.jump = true;
    }
}

/// Jump is held now (the Jump Swims Upwards tweak reads it in the step) [design: docs/design/tweaks.md].
static JUMP_HELD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Speed backwards as a fraction of the forward speed [observed: 272.8 against 344 units/s
/// running, capture mv_values, physics.md §6.3]. Sideways and diagonal speeds were not
/// measured (the strafe runs hit a wall); they take the forward speed [hypothesis].
pub const BACK_SPEED_FRACTION: f32 = 272.8 / 344.0;

/// Speed while sneaking as a fraction of the speed otherwise [observed: 197.7 against 344
/// units/s running forward, capture mv_values, physics.md §6.3; `fMoveSneakMult` is 0.57;
/// the same fraction walking is hypothesis].
pub const SNEAK_SPEED_FRACTION: f32 = 197.7 / 344.0;

/// Horizontal velocity (game x, y) for an intent at a compass heading: diagonals not
/// faster; straight backwards at [`BACK_SPEED_FRACTION`] of the speed, scaling with the
/// backward share of a diagonal [hypothesis for diagonals]; sneaking at
/// [`SNEAK_SPEED_FRACTION`].
pub fn velocity(intent: &MoveIntent, heading: f32, speeds: MoveSpeeds) -> [f32; 2] {
    let (s, c) = heading.sin_cos();
    // Forward is (sin h, cos h); right is forward turned 90° clockwise, (cos h, −sin h).
    let v = [s * intent.forward + c * intent.right, c * intent.forward - s * intent.right];
    let len = v[0].hypot(v[1]);
    if len < 1e-6 {
        return [0.0, 0.0];
    }
    let mut speed = if intent.walk { speeds.walk } else { speeds.run };
    if intent.forward < 0.0 {
        let back_share = -intent.forward / intent.forward.hypot(intent.right);
        speed *= 1.0 - back_share * (1.0 - BACK_SPEED_FRACTION);
    }
    if intent.sneak {
        speed *= SNEAK_SPEED_FRACTION;
    }
    [v[0] / len * speed, v[1] / len * speed]
}

/// Swimming speeds, units/s, before the stroke cycle (`fnv_phys::swim`): run (Always Run)
/// 339, walk (Shift) 86, the same forwards, backwards and sideways [observed: sw_trial at
/// Lake Mead; physics.md §6.6].
pub const SWIM_SPEEDS: MoveSpeeds = MoveSpeeds { walk: fnv_phys::swim::SWIM_WALK_SPEED, run: fnv_phys::swim::SWIM_RUN_SPEED };

/// Velocity (game x, y, z) while swimming: forward and back along the view, pitch
/// included (the game swims along the view [observed]), sideways level; backwards at the
/// full speed and diagonals not faster [observed for straight and forward-right runs;
/// other diagonals hypothesis].
pub fn swim_velocity(intent: &MoveIntent, heading: f32, pitch: f32, speeds: MoveSpeeds) -> [f32; 3] {
    let (s, c) = heading.sin_cos();
    let (ps, pc) = pitch.sin_cos();
    let f = intent.forward;
    let r = intent.right;
    let v = [s * pc * f + c * r, c * pc * f - s * r, ps * f];
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len < 1e-6 {
        return [0.0, 0.0, 0.0];
    }
    let speed = if intent.walk { speeds.walk } else { speeds.run };
    [v[0] / len * speed, v[1] / len * speed, v[2] / len * speed]
}

/// New look angles after a mouse motion (counts; +x right, +y down).
pub fn look(heading: f32, pitch: f32, delta: Vec2) -> (f32, f32) {
    look_with(heading, pitch, delta, LOOK_RADIANS_PER_COUNT, false)
}

/// [`look`] at a sensitivity (radians per count), with the vertical axis inverted or not
/// (the game's `bInvertYValues`).
pub fn look_with(heading: f32, pitch: f32, delta: Vec2, sensitivity: f32, invert_y: bool) -> (f32, f32) {
    let dy = if invert_y { -delta.y } else { delta.y };
    let h = (heading + delta.x * sensitivity).rem_euclid(std::f32::consts::TAU);
    let p = (pitch - dy * sensitivity).clamp(-MAX_PITCH, MAX_PITCH);
    (h, p)
}

/// Where `a` is `t` of the way to `b`.
fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Once-a-second status (debug log).
#[derive(Resource, Default)]
struct Status {
    elapsed: f32,
    steps: u32,
    grounded_steps: u32,
    swimming_steps: u32,
    moved: f32,
    commanded: f32,
    touched: BTreeSet<u32>,
    touched_terrain: bool,
    last: Option<CharacterStep>,
    waiting_frames: u32,
}

impl Status {
    fn record(&mut self, s: &CharacterStep, from: [f32; 3], vel: [f32; 2], dt: f32) {
        self.steps += 1;
        self.grounded_steps += s.grounded as u32;
        self.swimming_steps += s.swimming as u32;
        self.moved += (s.feet[0] - from[0]).hypot(s.feet[1] - from[1]);
        self.commanded += vel[0].hypot(vel[1]) * dt;
        for o in &s.touched {
            match o {
                Owner::Ref { form_id, .. } => {
                    self.touched.insert(*form_id);
                }
                Owner::Terrain { .. } => self.touched_terrain = true,
                Owner::Character { .. } | Owner::Actor { .. } => {}
            }
        }
        self.last = Some(s.clone());
    }
}

/// Within `PlaySet::Physics`, after the character has moved and before the camera is
/// placed: where a player body poses itself and writes [`Player::body_eye`].
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BodySet;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MoveIntent>()
            .add_message::<crate::rumble::Landed>()
            .init_resource::<Controls>()
            .init_resource::<Status>()
            .init_resource::<Net>()
            .init_resource::<PlayerControl>()
            .init_resource::<crate::control::ControlLocks>()
            .init_resource::<crate::control::LockStats>()
            .init_resource::<Bindings>()
            .init_resource::<crate::control::BodyView>()
            .init_resource::<ViewDriven>()
            .init_resource::<NoClip>()
            .insert_resource(NearAid::from_env())
            .add_systems(Startup, (init_player, crate::control::init_settings))
            .add_systems(Update, (grab_cursor, read_input, crate::control::read_controls, mouse_look).chain().in_set(PlaySet::Input))
            .add_plugins(crate::vanity::VanityPlugin)
            .init_resource::<RunPace>()
            .init_resource::<PerkSpeed>()
            .init_resource::<StatSpeed>()
            .init_resource::<ReloadPace>()
            .init_resource::<Lunge>()
            .add_systems(Update, take_knockbacks.after(PlaySet::Input).before(PlaySet::Physics))
            .init_resource::<crate::control::DetachedCamera>()
            .add_systems(
                Update,
                (crate::control::enforce_locks, aiming_walks, encumbered_walks, flinch_holds_walk, dialogue_lead_walks, crate::control::detach_camera).chain().after(PlaySet::Input).before(PlaySet::Physics),
            )
            .add_systems(Update, crate::control::tell_scripts.after(PlaySet::Input).before(PlaySet::Scripts))
            .add_systems(Last, crate::control::report_locks)
            .configure_sets(Update, BodySet.in_set(PlaySet::Physics).after(log_status).before(crate::control::place_camera))
            .add_systems(
                Update,
                (place_initial, interior_net, crate::phys::piles_pass_their_corpses, step, crate::phys::settle_created, sync_sim, log_status)
                    .chain()
                    .in_set(PlaySet::Physics)
                    .after(crate::phys::PhysSystems),
            )
            .add_systems(Update, crate::control::place_camera.in_set(PlaySet::Physics).after(log_status));
        app.init_resource::<crate::inertia::MovementInertia>().add_systems(Update, (crate::inertia::follow_weapon, crate::inertia::log_switches));
        // Sprint's shared state and the bindings kept on its switch.
        app.add_plugins(crate::stance::StancePlugin);
        // Sprint (`crate::sprint`): between the frame's walk rules and the physics steps.
        app.add_plugins(crate::sprint::SprintPlugin);
        // Modern Feel's switches (NVF Features › Modern Feel; the fork's experiment).
        app.add_plugins(crate::modern_feel::ModernFeelPlugin);
        // The Advanced Movement section's switches kept here (NVF Features; Weapon Inertia and
        // Camera Motion are nvforever's `settings.rs`): the Global Toggle, then Gameplay's and
        // Animation Smoothing [design: the owner, 2026-11-15, the menu's names and order].
        type Inertia = crate::inertia::MovementInertia;
        let switch = |key, label, note, switches, get: fn(&World) -> bool, set: fn(&mut World, bool)| crate::settings::Setting { key, group: crate::pause::GROUP_MENUS, label, note, switches, get, set, slider: None };
        for s in [
            switch(
                "advanced-movement",
                "Global Toggle",
                "On: the switches below shape how your character, view and weapon move. Off: none of them acts, as in the original; each keeps its own choice.",
                "--no-advanced-movement",
                |w| w.resource::<Inertia>().advanced,
                |w, on| w.resource_mut::<Inertia>().advanced = on,
            ),
            switch(
                "movement-inertia",
                "Movement Inertia",
                "On: you speed up, glide to a stop and brake before turning; crouching eases and landings dip and slow you by the fall. Off: as in the original.",
                "--movement-inertia --no-movement-inertia",
                |w| w.resource::<Inertia>().on,
                |w, on| w.resource_mut::<Inertia>().on = on,
            ),
            switch(
                "weapon-handling",
                "Weapon Handling",
                "On: drawing, putting away and aiming a weapon take a moment longer, and longer still for heavier weapons. Off: the original's timing.",
                "--weapon-handling --no-weapon-handling",
                |w| w.resource::<Inertia>().weapon_handling,
                |w, on| w.resource_mut::<Inertia>().weapon_handling = on,
            ),
            switch(
                "advanced-recoil",
                "Advanced Recoil",
                "On: a shot's kick eases the view up and part of it settles back down, and the view nudges back. Off: the whole kick stays, as in the original.",
                "--no-advanced-recoil",
                |w| w.resource::<Inertia>().recoil,
                |w, on| w.resource_mut::<Inertia>().recoil = on,
            ),
            // Sprint, off by default; its control (`crate::bindings`) is greyed while it is
            // off, its shared state is `crate::stance`'s.
            switch(
                "sprint",
                "Sprint",
                SPRINT_NOTE,
                "--sprint --no-sprint",
                |w| w.resource::<Inertia>().sprint,
                |w, on| w.resource_mut::<Inertia>().sprint = on,
            ),
            switch(
                "animation-smoothing",
                "Animation Smoothing",
                "On: in first person drawing, aiming, attacks, reloads and the Pip-Boy ease in and settle softly, taking no longer. Off: the original's clips.",
                "--animation-smoothing --no-animation-smoothing",
                |w| w.resource::<Inertia>().smooth,
                |w, on| w.resource_mut::<Inertia>().smooth = on,
            ),
        ] {
            crate::settings::add_setting(app, s);
        }
        // Under the Global Toggle: how heavy all of it is [design: the owner, 2026-11-15,
        // "add it as Movement Weight under the global toggle ... a dropdown with 25-100%"].
        crate::settings::add_choice_list(
            app,
            crate::settings::Setting {
                key: "movement-weight",
                group: crate::pause::GROUP_MENUS,
                label: "Movement Weight",
                note: "How heavy it all feels: 100% as made; lower makes speeding up, stopping and crouching quicker and cuts the extra time drawing and aiming take.",
                switches: "",
                get: |_| false,
                set: |_, _| {},
                slider: None,
            },
            crate::settings::ChoiceList {
                names: |_| crate::inertia::MOVEMENT_WEIGHTS.iter().map(|(_, n)| n.to_string()).collect(),
                chosen: |w| crate::inertia::movement_weight_name(w.resource::<Inertia>().weight()).to_string(),
                pick: movement_weight_pick,
                save: |w| crate::inertia::movement_weight_name(w.resource::<Inertia>().weight()).to_string(),
                load: movement_weight_pick,
                ask_keep: false,
            },
        );
        crate::settings::add_setting(
            app,
            crate::settings::Setting {
                key: "detached-camera",
                group: crate::pause::GROUP_MENUS,
                label: "Detached third-person camera",
                note: "On: in third person with your weapon away the mouse turns the camera around you and you run whichever way you push. Off: the camera stays behind you.",
                switches: "",
                get: |w| w.resource::<crate::control::DetachedCamera>().0,
                set: |w, on| w.resource_mut::<crate::control::DetachedCamera>().0 = on,
                slider: None,
            },
        );
    }
}

/// The Sprint row's note (NVF Features › Advanced Movement › Gameplay).
pub const SPRINT_NOTE: &str = "On: hold the Sprint key (set it in Controls) to run faster, spending AP every second; stopping low on AP leaves you out of breath. Off: as in the original.";

/// The Movement Weight dropdown's choice, by its name or its number.
fn movement_weight_pick(w: &mut World, name: &str) -> bool {
    match crate::inertia::movement_weight_by_name(name) {
        Some(weight) => {
            w.resource_mut::<crate::inertia::MovementInertia>().weight = weight;
            true
        }
        None => false,
    }
}

fn init_player(mut commands: Commands, d: Res<PlayWorld>, sim: Res<SimState>, mut near: ResMut<NearAid>) {
    let [x, y, z] = d.eye;
    let mut feet = [x, y, z - EYE_HEIGHT];
    if let Some(id) = near.target {
        // Where the reference stands, so physics streams its cells; only in the space played.
        let here = sim.refs.get(FormId(id)).filter(|r| match d.interior {
            Some(cell) => r.cell == Some(FormId(cell)),
            None => r.cell.is_some_and(|c| sim.refs.cell_world.get(&c) == Some(&FormId(d.world))),
        });
        match here {
            Some(r) => {
                (near.pos, near.start) = (Some(r.pos), feet);
                feet = r.pos;
            }
            None => {
                warn!("NVF_PLAY_TEST_NEAR: {id:08X} is not a reference of the cell or worldspace played (use its --cell)");
                near.target = None;
            }
        }
    }
    let mut heading = d.heading_deg.to_radians();
    // An interior start with no position asked for: where `coc` puts the player (physics.md
    // §6.7).
    if let Some(spot) = d.interior.filter(|_| d.start_at_cell_spot && near.target.is_none()).and_then(|c| safe::cell_spot(sim.refs, c)) {
        info!("interior start at the cell's own spot ({:?}): ({:.1}, {:.1}, {:.1}), heading {:.0}°", spot.source, spot.feet[0], spot.feet[1], spot.feet[2], spot.heading.to_degrees());
        (feet, heading) = (spot.feet, spot.heading);
    }
    commands.insert_resource(Player { heading, pitch: d.pitch_deg.to_radians(), eye_height: EYE_HEIGHT, feet, body_eye: None, body_view: None, placed: None });
    let settings = MoveSpeeds::from_settings(sim.forms.setting_f32("fMoveBaseSpeed"), sim.forms.setting_f32("fMoveRunMult"));
    let speeds = MoveSpeeds::OBSERVED;
    info!(
        "player speeds: walk {:.1}, run {:.0} units/s (measured in game; fMoveBaseSpeed and fMoveRunMult give {:.0} and {:.0})",
        speeds.walk, speeds.run, settings.walk, settings.run
    );
    commands.insert_resource(speeds);
}

/// The cursor is grabbed and hidden while playing in a focused window, released in menus
/// and while a script's message box is on screen (its buttons are clickable). Not with
/// `--auto-walk` (a test aid: the window must not hold the desktop's mouse).
///
/// The pointer shows only where it is used: menus (the console, the Pip-Boy, the
/// inventory), message boxes and a conversation's choices. Not while the intro film plays
/// (`crate::intro`) nor while an idle drives the view (the opening's wake-up,
/// [`ViewDriven`]) unless a menu or box is up [design: the owner's playtest of 2026-10-16].
/// The character creator (`crate::creator`) always has it, whatever state the game is in
/// when the opening opens it [owner 2026-10-17: "we should have mouse pointer in this
/// menu"].
///
/// With Input Device = Controller the pointer sleeps until the mouse is used
/// (`crate::pad::pointer`): hidden wherever it would show [design, #2310].
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn grab_cursor(
    state: Res<State<PlayState>>,
    d: Res<PlayWorld>,
    unattended: Option<Res<crate::test_aid::Unattended>>,
    hud: Option<Res<crate::script_hud::ScriptHud>>,
    look: Option<Res<crate::dialogue::FreeLook>>,
    over_lock: Option<Res<crate::lockpick::PointerOverLock>>,
    free_cam: Option<Res<crate::free_cam::FreeCam>>,
    (intro, view, creator, panel, talk, tweaks, vats): (
        Option<Res<crate::intro::Intro>>,
        Option<Res<ViewDriven>>,
        Option<Res<crate::creator::CreatorMenu>>,
        Option<Res<crate::message_menu::MessagePanel>>,
        Option<Res<crate::dialogue_menu::DialogueMenu>>,
        Option<Res<crate::tweaks::TweakSettings>>,
        Option<Res<crate::vats_mode::VatsMode>>,
    ),
    mut windows: Query<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    mut wanted_before: Local<Option<(bool, bool)>>,
    switches: Option<Res<crate::script_switches::ScriptSwitches>>,
    pad_pointer: Option<Res<crate::pad::pointer::PadPointer>>,
) {
    let Ok((window, mut cursor)) = windows.single_mut() else { return };
    // A script's HUD cursor (ToggleHUDCursor, `crate::script_switches`): the pointer shows
    // over the world, free.
    let hud_cursor = switches.is_some_and(|s| s.hud_cursor) && *state.get() == PlayState::Playing;
    let driven = d.auto_walk > 0.0 || unattended.is_some_and(|u| u.0);
    let boxed = hud.is_some_and(|h| h.front().is_some());
    // Also while an actor speaks in a conversation (free look, `crate::dialogue`), and
    // kept through a loading screen: the player is back in the world after it.
    // And while the console's free camera flies, also when it holds the world.
    let creating = creator.is_some_and(|c| c.is_open());
    let playing = !creating && (matches!(*state.get(), PlayState::Playing | PlayState::Loading) || look.is_some_and(|l| l.0) || free_cam.is_some_and(|f| f.active));
    let film = intro.as_ref().is_some_and(|i| i.is_playing());
    let scene = view.is_some_and(|v| v.0) && *state.get() != PlayState::Menu && !boxed && !creating;
    // The classic VATS style's target screen has the pointer: its parts and arrows are
    // chosen with it [observed: the owner's screenshots of the original's VATS, 2026-11-10].
    let targeting = vats.is_some_and(|v| v.pointer_wanted());
    let sleeping = crate::pad::pointer::asleep(pad_pointer.as_deref());
    let wanted = !sleeping && (creating || targeting || ((!playing || boxed) && !film && !scene));
    // Logged when it changes, and when a film starts or ends over a pointer already hidden.
    if *wanted_before != Some((wanted, film)) {
        let why = if sleeping {
            " (the controller has the controls)"
        } else if creating {
            " (the character creator)"
        } else if film && intro.is_some_and(|i| i.playing_script_film()) {
            " (a film)"
        } else if film {
            " (the intro film)"
        } else if scene {
            " (an idle drives the view)"
        } else {
            ""
        };
        info!("pointer: {}{why}", if wanted { "shown" } else { "hidden" });
        *wanted_before = Some((wanted, film));
    }
    let grab = playing && !driven && !boxed && window.focused && !hud_cursor && !targeting;
    // Windows cannot lock the cursor in place; confining it still gives raw motion.
    let mode = match (grab, cfg!(target_os = "windows")) {
        (false, _) => CursorGrabMode::None,
        (true, true) => CursorGrabMode::Confined,
        (true, false) => CursorGrabMode::Locked,
    };
    // Out of focus or under a test aid the mouse is the desktop's: its pointer stays. The
    // lockpicking menu hides the pointer over the lock (`crate::lockpick`).
    // The player's tweak hides it over a message box, worked with the keys then [design: docs/design/tweaks.md].
    // So does its other tweak over a conversation's choices.
    let message_box = tweaks.as_ref().is_some_and(|t| t.no_pointer_in_message_box) && panel.is_some_and(|p| p.view.is_some()) && window.focused && !driven;
    let message_box = message_box || (tweaks.as_ref().is_some_and(|t| t.no_pointer_in_dialogue) && talk.is_some_and(|t| t.view.is_some()) && window.focused && !driven);
    let visible = !grab && (wanted || driven || !window.focused || hud_cursor) && !over_lock.is_some_and(|o| o.0) && !message_box;
    if cursor.grab_mode != mode || cursor.visible != visible {
        cursor.grab_mode = mode;
        cursor.visible = visible;
    }
}

/// The movement controls: Forward, Back, Slide Left and Right; running unless Run is held
/// while Always Run is on (and the other way round); Auto Move walks forward until
/// Forward or Back is pressed [hypothesis]; Jump.
fn read_input(
    input: ControlInput,
    state: Res<State<PlayState>>,
    mut control: ResMut<PlayerControl>,
    mut intent: ResMut<MoveIntent>,
    mut seat: ResMut<crate::furniture::PlayerSeat>,
    driven: Option<Res<ViewDriven>>,
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
) {
    if *state.get() != PlayState::Playing {
        *intent = MoveIntent::default();
        return;
    }
    if driven.is_some_and(|d| d.0) {
        // An animation has the view: the player stays where it plays it.
        control.auto_move = false;
        *intent = MoveIntent::default();
        return;
    }
    if seat.hold.is_some() {
        // Held by furniture: no movement; a movement control asks to get up.
        let moving = [Control::Forward, Control::Back, Control::SlideLeft, Control::SlideRight, Control::Jump];
        if moving.into_iter().any(|c| input.just_pressed(c)) {
            seat.leave = true;
        }
        control.auto_move = false;
        *intent = MoveIntent::default();
        return;
    }
    let axis = |plus: Control, minus: Control| input.pressed(plus) as i32 as f32 - input.pressed(minus) as i32 as f32;
    if control.auto_move && (input.just_pressed(Control::Forward) || input.just_pressed(Control::Back)) {
        control.auto_move = false;
    }
    intent.forward = if control.auto_move { 1.0 } else { axis(Control::Forward, Control::Back) };
    intent.right = axis(Control::SlideRight, Control::SlideLeft);
    intent.walk = control.always_run == input.pressed(Control::Run);
    // A controller's left stick moves the way it is pushed; pushed a little the player
    // walks (with Always Run on) [design, `crate::pad`].
    if let Some(v) = input.move_stick().filter(|v| *v != Vec2::ZERO) {
        if !control.auto_move {
            intent.forward = v.y;
        }
        intent.right = v.x;
        if control.always_run && v.length() < crate::pad::STICK_WALK_BELOW {
            intent.walk = true;
        }
    }
    intent.sneak = control.sneak;
    // The player's tweak: holding Jump jumps again on landing [design: docs/design/tweaks.md].
    let repeat = tweaks.as_ref().is_some_and(|t| t.repeat_jumping);
    if input.just_pressed(Control::Jump) || (repeat && input.pressed(Control::Jump)) {
        intent.jump = true;
    }
    JUMP_HELD.store(input.pressed(Control::Jump), std::sync::atomic::Ordering::Relaxed);
}

/// Actors walked by another system whose moves are planned on the compute pool when at
/// least this many move, in chunks of [`MOVE_CHUNK`] [engineering choice: fewer are
/// cheaper on this thread].
const PARALLEL_MOVES: usize = 8;
const MOVE_CHUNK: usize = 4;

/// While the player aims (Block held with a weapon out) they move at walking speed, not
/// running, and the body walks [design: the owner's wish, playtest of 2026-10-11, "reduced
/// to walk speed while aiming"; the original's rule is not captured]. Sneaking still
/// slows the walk. NPCs that aim are for the combat AI (docs/design/combat-ai.md, open).
pub fn aiming_walks(control: Res<PlayerControl>, mut intent: ResMut<MoveIntent>, mut was: Local<bool>, tweaks: Option<Res<crate::tweaks::TweakSettings>>) {
    // The player's tweak keeps the pace while aiming or blocking [design: docs/design/tweaks.md].
    let aiming = control.aiming && control.weapon_out && !tweaks.is_some_and(|t| t.no_aiming_slowdown);
    if aiming && !intent.walk {
        intent.walk = true;
    }
    if aiming != *was {
        *was = aiming;
        info!("aiming: {}", if aiming { "the player moves at walking speed" } else { "over, running speed again" });
    }
}

/// Over-encumbered, the player can only walk [observed: with capacity 200 the player
/// walked at 85 units/s carrying 201 and more, xNVSE capture `cl_b_encumbrance_speed`,
/// docs/spec-notes/simulation.md §3; the owner's playtest list 5 item 21]. Not in god
/// mode [hypothesis: the original's tgm lifts the carry limit]. The weight is read twice
/// a second [design].
/// The player is over the carry limit now (for the Overencumbered Running tweak,
/// `crate::tweaks`) [design: docs/design/tweaks.md].
pub static OVERENCUMBERED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn encumbered_walks(time: Res<Time>, sim: Option<ResMut<crate::sim::SimState>>, mut intent: ResMut<MoveIntent>, mut st: Local<(f32, bool)>, tweaks: Option<Res<crate::tweaks::TweakSettings>>, grab: Option<Res<crate::grab::GrabState>>) {
    let Some(mut sim) = sim else { return };
    st.0 -= time.delta_secs();
    if st.0 <= 0.0 {
        st.0 = ENCUMBRANCE_CHECK;
        let mut e = sim.world.encumbrance(fnv_sim::FormId::PLAYER_REF).ok();
        // The player's tweak: an item held up by the grab weighs as if carried [design: docs/design/tweaks.md].
        let held = grab.as_ref().and_then(|g| g.held.as_ref()).filter(|_| tweaks.as_ref().is_some_and(|t| t.grabbed_weighs)).map(|h| h.on);
        if let (Some(e), Some(fnv_phys::GripOn::Ref(id))) = (e.as_mut(), held) {
            let id = fnv_sim::FormId(id);
            let base = sim.base_of(id.0);
            let weight = base.and_then(|b| sim.forms.item(b)).and_then(|i| i.weight).unwrap_or(0.0);
            let count = sim.world.peek(id).map(|s| s.count).or_else(|| base.map(|b| sim.forms.placed_count(b, sim.refs.get(id).and_then(|p| p.count)))).unwrap_or(1);
            e.weight += f64::from(weight) * f64::from(count.max(1));
            e.over |= e.weight > e.capacity;
        }
        let over = e.as_ref().is_some_and(|e| e.over) && !crate::combat::god_mode();
        if over != st.1 {
            st.1 = over;
            match (over, e) {
                (true, Some(e)) => info!("encumbrance: {:.1} / {:.0}, over the limit: the player walks", e.weight, e.capacity),
                _ => info!("encumbrance: within the limit (or god mode), running again"),
            }
        }
    }
    OVERENCUMBERED.store(st.1, std::sync::atomic::Ordering::Relaxed);
    // The player's tweak lets them run, paying Action Points (`crate::tweaks::ap_costs`).
    if st.1 && !intent.walk && !tweaks.is_some_and(|t| t.run_overencumbered) {
        intent.walk = true;
    }
}

/// While the player flinches from a staggering hit (`crate::combat`, crippled-limbs.md
/// §3) they do not walk or jump: the hit's stagger holds them where they stand, as it
/// holds NPCs and creatures (`nvforever` residents.rs) [design: owner's playtest list 7
/// item 13, "npcs/creatures and the player should be stopped from moving whilst a
/// crippled animation or stagger is currently playing"].
pub fn flinch_holds_walk(combat: Option<Res<crate::combat::Combat>>, mut intent: ResMut<MoveIntent>, mut held: Local<bool>) {
    let now = combat.is_some_and(|c| c.player_flinching());
    if now != *held {
        *held = now;
        if now {
            info!("player: held where they stand while they flinch");
        }
    }
    if now && (intent.forward != 0.0 || intent.right != 0.0 || intent.jump) {
        (intent.forward, intent.right, intent.jump) = (0.0, 0.0, false);
    }
}

/// A Dialogue package walks the player into its trigger location before its actor talks
/// ("Do Not Control Target Movement" off; `fnv_game::game::PlayerLead`, set by `fnv_ai`):
/// while the lead holds, the player walks to its point, facing that way, and the
/// movement keys are set aside [documented: GECK wiki, Dialogue Package, the target
/// "will walk towards that location when the dialogue package starts"; at walking pace
/// is our choice; issue #2453].
pub fn dialogue_lead_walks(
    script: Option<Res<crate::script::ScriptState>>,
    state: Res<State<PlayState>>,
    mut player: ResMut<Player>,
    mut intent: ResMut<MoveIntent>,
    mut was: Local<bool>,
) {
    let lead = script
        .as_ref()
        .and_then(|s| s.rt.as_ref())
        .and_then(|rt| rt.game.player_lead.filter(|l| l.fresh(rt.game.time)))
        .filter(|_| *state.get() == PlayState::Playing);
    let walking = match lead {
        Some(l) => {
            let (dx, dy) = (l.to[0] - player.feet[0], l.to[1] - player.feet[1]);
            let far = dx.hypot(dy) > LEAD_ARRIVE;
            if far {
                player.heading = dx.atan2(dy);
            }
            *intent = MoveIntent { forward: if far { 1.0 } else { 0.0 }, walk: true, sneak: intent.sneak, ..Default::default() };
            far
        }
        None => false,
    };
    if walking != *was {
        *was = walking;
        match lead.filter(|_| walking) {
            Some(l) => info!("dialogue lead: the player walks to ({:.0}, {:.0}, {:.0}) for 0x{:08X}'s Dialogue package", l.to[0], l.to[1], l.to[2], l.by.0),
            None => info!("dialogue lead: over"),
        }
    }
}

/// How near the lead's point the player stops ([`dialogue_lead_walks`]) [our choice].
const LEAD_ARRIVE: f32 = 16.0;

/// Seconds between reads of the player's carried weight ([`encumbered_walks`]).
const ENCUMBRANCE_CHECK: f32 = 0.5;

/// A share of the running speed the player keeps: below 1 with a two-handed weapon drawn
/// (`crate::combat`, the owner's playtest list 5 item 61). Walking is not slowed.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct RunPace(pub f32);

impl Default for RunPace {
    fn default() -> Self {
        RunPace(1.0)
    }
}

/// A share of the player's speed, walking and running, kept while a reload runs
/// (`crate::combat::reload_pace`); 1 otherwise or with the setting off.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct ReloadPace(pub f32);

impl Default for ReloadPace {
    fn default() -> Self {
        ReloadPace(1.0)
    }
}

/// What slows or speeds the player's steps: crippled legs, a two-handed weapon's run, the
/// Modify Run Speed perks, a reload, Speed Mult and body armour.
type Paces<'w> = (Option<Res<'w, crate::combat::PlayerLimbs>>, Option<Res<'w, RunPace>>, Option<Res<'w, PerkSpeed>>, Option<Res<'w, ReloadPace>>, Option<Res<'w, StatSpeed>>, Sprinting<'w>);

/// Sprint's share and speed (`crate::sprint`): from the gait's speed to a multiple of the run's.
type Sprinting<'w> = (Option<Res<'w, crate::stance::StanceMotion>>, Option<Res<'w, crate::sprint::SprintTuning>>);

/// Turbo's clock for the player, and where the steps' cost is measured: `--log-frame-times`'
/// counts and the physics rate's guard (`crate::phys_load::Meter`).
type Meters<'w> = (Option<Res<'w, crate::script::PlayerTimeScale>>, Option<ResMut<'w, crate::phys_load::PhysLoad>>, Option<ResMut<'w, crate::phys_rate::PhysRate>>);

/// The player's Speed Mult and body armour's factor on its movement, walking, running
/// and sneaking, as people's (`fnv_game::Game::move_speed_mult`, `crate::combat`'s
/// `stat_speed`; actors.md §8.10, issue #1364).
/// The second number is the player's Agility Scales Jump Height tweak, a factor on the
/// jump height (1 with the tweak off); the third the Armor Causes Sinking tweak's sinking
/// speed in water, units a second (0 off) [design: docs/design/tweaks.md].
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct StatSpeed(pub f32, pub f32, pub f32);

impl Default for StatSpeed {
    fn default() -> Self {
        StatSpeed(1.0, 1.0, 0.0)
    }
}

/// The player's Modify Run Speed perks' factor on its movement, walking, running and
/// sneaking (`crate::combat`'s `perk_speed`; perk-entry-points.md §8).
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct PerkSpeed(pub f32);

impl Default for PerkSpeed {
    fn default() -> Self {
        PerkSpeed(1.0)
    }
}

/// What the player's attack clip carries them through (docs/spec-notes/actors.md §8.13):
/// the body's attack clip whose `Bip01` travels (Legion Assault's
/// `h2hattackcustom1power.kf`, 332 units forward; the forward power attacks) gives the
/// player its root's travel, added every frame by the body (nvforever `player_body`).
/// `step` moves the character by the travel owed, spread over its physics steps, as well
/// as where the player walks; walls and actors stop it as they stop walking. Travel, not
/// a velocity, so a long frame after a short one moves the player the clip's own travel
/// (review of #1209).
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct Lunge {
    /// Game-space travel (x, y, units) the clip has carried the player by that the
    /// physics steps have not moved yet.
    pub owed: [f32; 2],
    /// An attack clip that carries the player plays now.
    pub carrying: bool,
    /// Where the feet were when it started, for the log at its end.
    from: Option<[f32; 3]>,
    /// A melee blow's knockback on the player and its seconds left
    /// (`crate::combat::Knockback`), with where the feet were when it began.
    knock: Option<(crate::combat::Knockback, f32, Option<[f32; 3]>)>,
}

impl Lunge {
    /// Be pushed back by `k`; a new knockback takes over from one still going [design].
    pub fn knock(&mut self, k: crate::combat::Knockback) {
        self.knock = Some((k, k.secs, None));
    }

    /// The knockback's velocity (game x, y) over a step of `dt` seconds at `feet`, using
    /// that time up; logged when it ends.
    fn knock_step(&mut self, dt: f32, feet: [f32; 3]) -> [f32; 2] {
        let Some((k, left, from)) = self.knock.as_mut() else { return [0.0; 2] };
        let from = *from.get_or_insert(feet);
        if *left <= 0.0 || dt <= 0.0 {
            info!("player: knockback done: {:.0} units of {:.0} from where it started", (feet[0] - from[0]).hypot(feet[1] - from[1]), k.distance());
            self.knock = None;
            return [0.0; 2];
        }
        let v = k.step(*left, dt) / dt;
        *left -= dt;
        [k.dir[0] * v, k.dir[1] * v]
    }
}

/// Melee blows' knockbacks on the player start here (`Lunge::knock`).
fn take_knockbacks(knocks: Option<MessageReader<crate::combat::Knockback>>, mut lunge: ResMut<Lunge>) {
    let Some(mut knocks) = knocks else { return };
    for k in knocks.read().filter(|k| k.form_id == FormId::PLAYER_REF.0) {
        info!("player: knocked back at {:.0} units/s for {:.2} s ({:.0} units)", k.speed, k.secs, k.distance());
        lunge.knock(*k);
    }
}

/// How fast the Smooth Camera tweak's look catches up with the input, per second (ours)
/// [design: docs/design/tweaks.md].
const SMOOTH_LOOK_RATE: f32 = 8.0;

/// Mouse look while the cursor is grabbed, or with a controller working the controls the
/// right stick's (`crate::pad`); while Toggle POV is held it circles the camera instead
/// (control.rs). Not while a script has locked looking: the view stays where it is
/// [documented: GECK wiki DisablePlayerControls, LookingFlag].
#[allow(clippy::too_many_arguments)]
pub(crate) fn mouse_look(
    motion: Res<AccumulatedMouseMotion>,
    (pads, time, vats): (crate::pad::ActionPads, Res<Time>, Option<Res<crate::vats_mode::VatsMode>>),
    state: Res<State<PlayState>>,
    bindings: Res<Bindings>,
    cursor: Query<&CursorOptions, With<PrimaryWindow>>,
    look: Option<Res<crate::dialogue::FreeLook>>,
    locks: Res<crate::control::ControlLocks>,
    mut control: ResMut<PlayerControl>,
    mut player: ResMut<Player>,
    mut seat: ResMut<crate::furniture::PlayerSeat>,
    driven: Option<Res<ViewDriven>>,
    free_cam: Option<Res<crate::free_cam::FreeCam>>,
    kill_cam: Option<Res<crate::kill_cam::KillCam>>,
    (assist, aim): (Option<Res<crate::aim_assist::AimAssistState>>, Option<Res<crate::aim_zoom::AimZoom>>),
    (talk, switches, mut mouse_guard): (
        Option<Res<crate::dialogue_zoom::DialogueZoom>>,
        Option<Res<crate::script_switches::ScriptSwitches>>,
        Local<DialogueMouseGuard>,
    ),
    (tweaks, mut smooth): (Option<Res<crate::tweaks::TweakSettings>>, Local<Vec2>),
) {
    let dialogue = *state.get() == PlayState::Dialogue;
    let grabbed = cursor.single().is_ok_and(|c| c.grab_mode != CursorGrabMode::None);
    let playing = *state.get() == PlayState::Playing || look.is_some_and(|l| l.0);
    let mouse_delta = mouse_guard.filter(motion.delta, grabbed && playing, dialogue);
    // A zoomed conversation locks the view: the mouse moves only the cursor, even if the
    // capture has not caught up yet (`dialogue::sync_state` frees the pointer), and the
    // right stick does not turn it either [owner decision 2026-11-13, #2254;
    // `crate::dialogue_look::view_locked`].
    if dialogue && talk.as_deref().is_some_and(crate::dialogue_look::view_locked) {
        return;
    }
    // The classic VATS style turns the view itself (`crate::vats_mode`, vats-classic.md §5).
    if vats.is_some_and(|v| v.holds_view()) {
        return;
    }
    // The console's free camera takes the mouse (`crate::free_cam`), and so does the
    // weapon wheel while it is open (`crate::weapon_wheel`); a playing kill camera holds
    // it (`crate::kill_cam`). A script's HUD cursor has the mouse too (ToggleHUDCursor,
    // `crate::script_switches`).
    if driven.is_some_and(|d| d.0) || free_cam.is_some_and(|f| f.on) || control.wheel || kill_cam.is_some_and(|k| k.active()) || switches.as_deref().is_some_and(|s| s.hud_cursor) {
        return;
    }
    // The right stick's turn this frame, as mouse counts of a thousandth of a radian
    // (`crate::bindings::LOOK_SPEED_HORIZONTAL`), pushed a little turning slowly
    // (squared) [design], slower on a nearby hostile (`crate::aim_assist`; never the
    // mouse).
    // With the player's Keyboard and Mouse With a Controller tweak, a mouse that moves
    // looks as the mouse does [design: docs/design/tweaks.md].
    let mouse_too = crate::bindings::keys_with_pad() && motion.delta != Vec2::ZERO;
    let (delta, s) = if bindings.device == crate::bindings::InputDevice::Controller && !mouse_too {
        let v = crate::pad::stick(&pads, false);
        // The player's tweak: the turn grows straight with the push past the dead zone,
        // not with its square, so a small push turns at once [design: docs/design/tweaks.md].
        let linear = tweaks.as_ref().is_some_and(|t| t.responsive_stick);
        let dz = crate::pad::dead_zone(false);
        let curve = |x: f32| if linear { x.signum() * ((x.abs() - dz) / (1.0 - dz).max(0.01)).max(0.0) } else { x * x.abs() };
        // The Controls tab's Stick Sensitivity scales the stick's look speeds.
        let dt = time.delta_secs() * assist.map_or(1.0, |a| a.look_scale) * bindings.stick_sensitivity;
        (Vec2::new(curve(v.x) * bindings.look_speed[0] * dt, -curve(v.y) * bindings.look_speed[1] * dt), STICK_LOOK_RADIANS)
    } else {
        // The axes ToggleMouseMovement left on (`crate::script_switches`).
        (switches.as_deref().map_or(mouse_delta, |s| s.look(mouse_delta)), bindings.mouse_sensitivity)
    };
    // The Controls tab's Vertical Sensitivity of the device in use (#2201).
    let delta = vertical_scaled(delta, bindings.vertical_sensitivity());
    // The player's tweak: the look eases after the mouse or stick, a cinematic camera;
    // not while aiming [design: docs/design/tweaks.md].
    let smoothing = tweaks.as_ref().is_some_and(|t| t.smooth_look) && !control.aiming;
    let delta = if smoothing {
        *smooth += delta;
        let out = *smooth * (1.0 - (-time.delta_secs() * SMOOTH_LOOK_RATE).exp());
        *smooth -= out;
        if smooth.length() < 1e-3 {
            *smooth = Vec2::ZERO;
        }
        out
    } else {
        *smooth = Vec2::ZERO;
        delta
    };
    if !grabbed || !playing || locks.looking || delta == Vec2::ZERO {
        return;
    }
    // A conversation's zoom as it eases out: the picture moves as far as it does unzoomed.
    let s = s * talk.as_deref().map_or(1.0, crate::dialogue_zoom::DialogueZoom::look_scale);
    // The Controls tab's Aim Sensitivity of the device in use, as far as the view is zoomed
    // for the aim (down the sights or through a scope; `crate::aim_zoom`): the hip look is
    // left as it is.
    let s = s * aim_scale(bindings.aim_sensitivity(), aim.as_deref().map_or(0.0, crate::aim_zoom::AimZoom::share));
    let inv = bindings.invert_y;
    // The player's tweak: left and right at their own speed [design: docs/design/tweaks.md].
    let delta = Vec2::new(delta.x * tweaks.as_ref().map_or(1.0, |t| t.horizontal_look as f32), delta.y);
    // The player's tweak turns the other way left and right [design: docs/design/tweaks.md].
    let delta = if tweaks.as_ref().is_some_and(|t| t.invert_x) { Vec2::new(-delta.x, delta.y) } else { delta };
    // The player's tweak turns slower while aiming [design: docs/design/tweaks.md].
    let delta = match tweaks.as_deref().filter(|_| control.aiming && control.weapon_out) {
        Some(t) => delta * Vec2::new(t.aim_turn_x, t.aim_turn_y),
        None => delta,
    };
    if control.orbit.held {
        // The player's tweak: a controller turns the circling camera twice as fast [design: docs/design/tweaks.md].
        let fast = bindings.device == crate::bindings::InputDevice::Controller && tweaks.as_ref().is_some_and(|t| t.faster_pad_orbit);
        let delta = if fast { delta * 2.0 } else { delta };
        let o = &mut control.orbit;
        let (yaw, pitch) = look_with(o.yaw, o.pitch, delta, s, inv);
        // Keep the yaw in (−π, π] so the swing back takes the short way.
        o.yaw = (yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        o.pitch = pitch;
        o.moved = true;
        return;
    }
    if control.orbit.free && control.talk_camera == 0.0 {
        // The detached camera (`crate::control::detach_camera`): the camera turns, the
        // player does not.
        let (yaw, pitch) = look_with(control.orbit.yaw, player.pitch, delta, s, inv);
        control.orbit.yaw = crate::control::wrap_angle(yaw);
        player.pitch = pitch;
        return;
    }
    (player.heading, player.pitch) = look_with(player.heading, player.pitch, delta, s, inv);
    if seat.hold.is_some() {
        // Held by furniture: what was asked for, for a holder with a frame of its own.
        let dy = if inv { -delta.y } else { delta.y };
        seat.look[0] += delta.x * s;
        seat.look[1] -= dy * s;
    }
}

/// Discard dialogue capture-transition motion in the current and following input frame:
/// the window backend applies capture asynchronously [design, #2254; grab_cursor and
/// Bevy bevy_winit::system::changed_cursor_options]. This is a guard, not a measured artifact.
#[derive(Default)]
pub(crate) struct DialogueMouseGuard {
    active: bool,
    settling: u8,
}

/// Ignore the transition frame and the next input frame [design, #2254].
const DIALOGUE_CURSOR_SETTLE_FRAMES: u8 = 2;

impl DialogueMouseGuard {
    pub(crate) fn filter(&mut self, delta: Vec2, active: bool, dialogue: bool) -> Vec2 {
        if dialogue && active != self.active {
            self.settling = DIALOGUE_CURSOR_SETTLE_FRAMES;
        }
        self.active = active;
        let settling = self.settling > 0;
        self.settling = self.settling.saturating_sub(1);
        if dialogue && settling { Vec2::ZERO } else { delta }
    }
}

/// `delta` (mouse counts, or the right stick's turn) with its vertical part times
/// `vertical`, the Controls tab's Vertical Sensitivity of the device in use [design:
/// #2201]; 1 leaves it exactly as it is.
pub fn vertical_scaled(delta: Vec2, vertical: f32) -> Vec2 {
    Vec2::new(delta.x, delta.y * vertical)
}

/// What the look's radians per count are multiplied by for `aim`, the Controls tab's Aim
/// Sensitivity of the device in use, with the view `zoomed` of the way to the aim's zoom
/// (`crate::aim_zoom::AimZoom::share`: 0 unzoomed, 1 down the sights or through a scope)
/// [design: owner 2026-11-15]. Exactly 1 unzoomed, and at an Aim Sensitivity of 1 (the
/// look as before); `aim` at the full zoom, in step with the zoom between.
pub fn aim_scale(aim: f32, zoomed: f32) -> f32 {
    1.0 + (aim - 1.0) * zoomed.clamp(0.0, 1.0)
}

/// Radians per unit of the right stick's turn (a look speed of 1000 turns a radian a
/// second) [hypothesis, `crate::bindings::LOOK_SPEED_HORIZONTAL`].
const STICK_LOOK_RADIANS: f32 = 0.001;

/// Put the character on the ground under the start position once the cells around it are
/// loaded ([`PhysState::ready_around`]: a save made on a walkway whose reference lies in
/// the next square loads on it, #1103): a ray down from the start eye, else (outdoors)
/// from far above (a start under the terrain). Indoors with nothing below, the start moves to the nearest door arrival into
/// the cell ([`safe`]). With the `NVF_PLAY_TEST_NEAR` aid, a standing spot next to its
/// reference replaces the start.
fn place_initial(mut phys: ResMut<PhysState>, mut player: ResMut<Player>, mut near: ResMut<NearAid>, sim: Res<SimState>) {
    if phys.player.is_some() || !phys.ready_around(player.feet) {
        return;
    }
    if let (Some(id), Some(pos)) = (near.target, near.pos.take()) {
        // The view's eye is the first-person body's head once the body is drawn, above
        // `eye_height` [observed: 121 above the feet in GSGasStation, where a spot chosen
        // with 112 at 147 units from Ringo's floor safe left it 154 away, out of reach];
        // the spot is chosen with that eye and a few units to spare.
        match safe::reach_spot(&mut phys, player.eye_height + NEAR_EYE_ABOVE, id, pos, crate::activate::pick::REACH - NEAR_REACH_SPARE) {
            Some((feet, heading, pitch)) => {
                info!(
                    "test aid NVF_PLAY_TEST_NEAR: standing at ({:.0}, {:.0}, {:.1}) facing {id:08X} at ({:.0}, {:.0}, {:.0}), heading {:.0}°, pitch {:.0}°",
                    feet[0],
                    feet[1],
                    feet[2],
                    pos[0],
                    pos[1],
                    pos[2],
                    heading.to_degrees(),
                    pitch.to_degrees()
                );
                (player.feet, player.heading, player.pitch) = (feet, heading, pitch);
            }
            None => {
                let [x, y, z] = near.start;
                warn!("test aid NVF_PLAY_TEST_NEAR: no standing spot within reach of {id:08X}; the start is kept ({x:.0}, {y:.0}, {z:.0})");
                player.feet = near.start;
                if !phys.ready_around(player.feet) {
                    return; // its cells stream in first
                }
            }
        }
    }
    let start = player.feet;
    let mut hit = phys.place_player(start, player.eye_height, 100_000.0);
    if hit.is_none() {
        if let Space::Interior(cell) = phys.space {
            if let Some((spot, feet)) = rescue(&mut phys, &sim, cell, start, "the start has nothing under it") {
                (player.feet, player.heading) = (feet, spot.heading);
                return;
            }
        } else {
            hit = phys.place_player(start, 100_000.0, 100_000.0);
        }
    }
    // A start above water deeper than the swimming depth: the character starts where it
    // was asked to and falls in (into the swimming state), as the game's player does after
    // a SetPos there [observed: physics.md §6.6], instead of standing on the bed.
    let swim_depth = phys.player.as_ref().map_or(0.0, |c| c.params.swim_depth);
    let deep = phys.world.water_surface_at(start[0], start[1]).filter(|&w| {
        let bed = hit.as_ref().map_or(f32::NEG_INFINITY, |h| h.point[2]);
        bed < w - swim_depth && start[2] > bed
    });
    if let Some(w) = deep {
        let p = &mut *phys;
        let c = p.player.as_mut().expect("placed");
        p.world.place_character(c, start);
        p.prev_feet = start;
        info!("player starts above water (surface z {w:.1}): falling in");
    }
    let feet = phys.player.as_ref().expect("placed").feet;
    player.feet = feet;
    player.placed = Some(feet);
    info!(
        "player placed: feet ({:.0}, {:.0}, {:.1}), ground {}, terrain z {}",
        feet[0],
        feet[1],
        feet[2],
        hit.map_or("none below the start".to_string(), |h| format!("z {:.1} ({:?})", h.point[2], h.owner)),
        phys.terrain_z(feet[0], feet[1]).map_or("-".to_string(), |z| format!("{z:.1}")),
    );
}

/// Test aid `NVF_PLAY_TEST_NEAR=<reference FormID, hex>`: start next to this reference.
#[derive(Resource, Default)]
struct NearAid {
    target: Option<u32>,
    /// The reference's position until the start is placed.
    pos: Option<[f32; 3]>,
    /// The start asked for (`--pos` or the cell's), kept when no spot is found.
    start: [f32; 3],
}

impl NearAid {
    fn from_env() -> NearAid {
        let Ok(v) = std::env::var("NVF_PLAY_TEST_NEAR") else { return NearAid::default() };
        match v.trim().parse::<FormId>() {
            Ok(id) => NearAid { target: Some(id.0), ..default() },
            Err(e) => {
                warn!("NVF_PLAY_TEST_NEAR: {e}");
                NearAid::default()
            }
        }
    }
}

/// The interior safety net's state.
#[derive(Resource, Default)]
struct Net {
    /// Where the character last stood on something, in the current space (cleared by
    /// `Travel`).
    last_grounded: Option<[f32; 3]>,
    /// A `Travel` arrived: check its destination once the character is there.
    check_arrival: bool,
    /// Lowest z of the interior's static collision, for (cell, cells loaded so far).
    floor: Option<(u32, usize, Option<f32>)>,
    /// This interior offered no door arrival with footing.
    hopeless: Option<u32>,
}

/// Put the player on the nearest door arrival into interior `cell` where they can stand
/// ([`safe`]), logging why and what replaced `from`. `None` (and a warning) when the cell
/// offers none; the character stays where it is.
fn rescue(phys: &mut PhysState, sim: &SimState, cell: u32, from: [f32; 3], why: &str) -> Option<(safe::Spot, [f32; 3])> {
    let spots = safe::door_arrivals(sim.refs, cell);
    // The cell's own spot first (the game put a player who fell out of the gas station
    // there [observed: pc_coc]), else the nearest door arrival with footing.
    let own = safe::cell_spot(sim.refs, cell).and_then(|s| safe::stand(phys, s.feet, None).map(|f| (s, f)));
    let Some((spot, _)) = own.or_else(|| safe::choose(&spots, from, |s| safe::stand(phys, s.feet, None))) else {
        warn!(
            "{why} at ({:.0}, {:.0}, {:.0}) in interior {cell:08X}, and none of its {} door arrivals has footing: left there",
            from[0],
            from[1],
            from[2],
            spots.len()
        );
        return None;
    };
    // The arrival rule (physics.md §6.2) settles the feet, as a door would.
    let a = phys.arrive_player(spot.feet);
    let to = match spot.source {
        safe::Source::DoorArrival { from, door } => format!("the arrival of door {from:08X} (beside {door:08X})"),
        safe::Source::Marker(m) => format!("the cell's marker {m:08X}"),
    };
    warn!(
        "{why} at ({:.0}, {:.0}, {:.0}) in interior {cell:08X}: moved to {to}, feet ({:.0}, {:.0}, {:.1}), heading {:.0}°",
        from[0],
        from[1],
        from[2],
        a.feet[0],
        a.feet[1],
        a.feet[2],
        spot.heading.to_degrees()
    );
    Some((spot, a.feet))
}

/// Interiors have no terrain to fall back onto: a character more than
/// [`safe::INTERIOR_FELL_THROUGH`] below the cell's lowest static collision goes back to
/// where it last stood, else to a door arrival; a door destination with nothing under it
/// moves to a door arrival at once.
fn interior_net(
    mut travel: MessageReader<Travel>,
    state: Res<State<PlayState>>,
    sim: Res<SimState>,
    mut net: ResMut<Net>,
    mut phys: ResMut<PhysState>,
    mut player: ResMut<Player>,
    noclip: Res<NoClip>,
) {
    if travel.read().count() > 0 {
        net.last_grounded = None;
        net.check_arrival = true;
    }
    // ToggleCollision: it may fly below the floor; where it stood is not where it flies.
    if noclip.0 {
        net.last_grounded = None;
        return;
    }
    let Space::Interior(cell) = phys.space else {
        net.check_arrival = false;
        return;
    };
    let Some((feet, grounded)) = phys.player.as_ref().map(|c| (c.feet, c.grounded)) else { return };
    if *state.get() != PlayState::Playing || !phys.ready_at(feet) {
        return;
    }
    if std::mem::take(&mut net.check_arrival) {
        let h = safe::params(&phys).height;
        if phys.ground_below(feet, h / 2.0, 100_000.0).is_none() {
            if let Some((spot, at)) = rescue(&mut phys, &sim, cell, feet, "the door's destination has nothing under it") {
                (player.feet, player.heading) = (at, spot.heading);
            }
            return;
        }
    }
    if grounded {
        net.last_grounded = Some(feet);
    }
    let key = (cell, phys.stats.cells_loaded);
    let floor = match net.floor {
        Some((c, n, z)) if (c, n) == key => z,
        _ => {
            let z = phys.world.fixed_bounds().map(|b| b.0[2]);
            net.floor = Some((key.0, key.1, z));
            z
        }
    };
    let Some(floor) = floor.filter(|&z| safe::fell_out(feet[2], z)) else { return };
    let back = net.last_grounded.and_then(|g| safe::stand(&mut phys, g, None));
    match back {
        Some(g) => {
            warn!(
                "player fell out of interior {cell:08X} at ({:.0}, {:.0}, {:.0}), lowest collision z {floor:.0}: back to where they last stood ({:.0}, {:.0}, {:.1})",
                feet[0], feet[1], feet[2], g[0], g[1], g[2]
            );
            phys.place_player(g, 1.0, 100.0);
            player.feet = phys.player.as_ref().map_or(g, |c| c.feet);
        }
        None => {
            let why = format!("player fell out of the interior (lowest collision z {floor:.0})");
            let found = if net.hopeless == Some(cell) { None } else { rescue(&mut phys, &sim, cell, feet, &why) };
            match found {
                Some((spot, at)) => (player.feet, player.heading) = (at, spot.heading),
                None => {
                    // Nowhere to stand: hold the character where the rule caught it (warned
                    // once per cell).
                    net.hopeless = Some(cell);
                    phys.place_player([feet[0], feet[1], floor - safe::INTERIOR_FELL_THROUGH], 0.0, 0.0);
                    player.feet = phys.player.as_ref().map_or(feet, |c| c.feet);
                }
            }
        }
    }
}

/// Below the terrain by this much the character has fallen through [engineering choice]:
/// it is put back on the ground (logged as a warning).
const FELL_THROUGH: f32 = 1000.0;

/// Fixed steps of the character for the frame's time; the camera's feet between the last
/// two.
#[allow(clippy::too_many_arguments)]
fn step(
    mut stroke_time: Local<f32>,
    time: Res<Time>,
    state: Res<State<PlayState>>,
    d: Res<PlayWorld>,
    speeds: Res<MoveSpeeds>,
    mut intent: ResMut<MoveIntent>,
    mut phys: ResMut<PhysState>,
    mut player: ResMut<Player>,
    mut status: ResMut<Status>,
    (limbs, pace, perk_pace, reload_pace, stat_pace, sprinting): Paces,
    (turbo, mut load, mut rate): Meters,
    (mut seat, mut landed): (ResMut<crate::furniture::PlayerSeat>, MessageWriter<crate::rumble::Landed>),
    (mut blocks, noclip, mut inertia): (Option<ResMut<crate::blocking::ActorBlocks>>, Res<NoClip>, ResMut<crate::inertia::MovementInertia>),
    (mut falls, mut fell, mut lunge): (Option<ResMut<crate::falls::Falls>>, MessageWriter<crate::falls::Fell>, ResMut<Lunge>),
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
) {
    if *state.get() != PlayState::Playing {
        // No time reaches the stepper while not playing, so nothing steps; its remainder is
        // kept, and with it the share of a step everything is drawn at (`draw_alpha`), so
        // nothing drawn between steps shifts when a menu opens or closes. (Zeroed, it drew
        // every actor, corpse and moved clutter at its pose before the last step under a
        // menu, and a frame later back where it was.)
        return;
    }
    // Measuring: the frame's time, always, for the physics rate's guard (`PhysRate`); with
    // `--log-frame-times` each step's time and the counts too, into `PhysLoad`.
    let mut meter = crate::phys_load::Meter::start(load.as_deref_mut(), rate.as_deref_mut());
    if let Some(at) = seat.hold.filter(|_| phys.player.is_some()) {
        // Held by furniture (`crate::furniture`): the world steps on, the character
        // stays where the furniture has it and collides with nothing. Movement asked
        // for by a test aid (the keys are read in `read_input`) asks to get up. A
        // knockback is dropped: not kept to push the player when it gets up.
        lunge.knock = None;
        if intent.forward != 0.0 || intent.right != 0.0 || intent.jump {
            seat.leave = true;
            intent.jump = false;
        }
        let p = &mut *phys;
        let n = p.stepper.advance(&p.world.params, time.delta_secs());
        let body = p.player.as_ref().map(|c| (at, c.params.radius, c.params.height));
        for _ in 0..n {
            let _tick = info_span!("phys::tick").entered();
            let began = meter.now();
            let listed = step_npcs_alone(&mut p.world, &mut p.npcs, &p.actor_block_radii, body, blocks.as_deref_mut(), falls.as_deref_mut(), &mut fell);
            p.world.step();
            meter.tick(began, p.npcs.len(), listed);
        }
        if let Some(c) = p.player.as_mut() {
            p.world.place_character(c, at);
            if let Some(f) = falls.as_deref_mut() {
                f.forget(c.id);
            }
            // Held up by the furniture: not in the air (the body's fall and landing).
            c.grounded = true;
            c.ground_gap = Some(0.0);
            p.prev_feet = at;
        }
        p.prev_feet = at;
        p.player_lift.reset();
        player.feet = at;
        inertia.reset();
        return;
    }
    if noclip.0 && phys.player.is_some() {
        lunge.knock = None;
        // ToggleCollision: moved where the player asks, through everything; the cells
        // ahead load as the camera reaches them.
        let p = &mut *phys;
        let dt = p.world.params.dt;
        let n = p.stepper.advance(&p.world.params, time.delta_secs());
        let v = swim_velocity(&intent, player.heading, player.pitch, *speeds);
        intent.jump = false;
        let c = p.player.as_mut().expect("placed");
        if let Some(f) = falls.as_deref_mut() {
            f.forget(c.id);
        }
        for _ in 0..n {
            let _tick = info_span!("phys::tick").entered();
            let began = meter.now();
            p.prev_feet = c.feet;
            let to = [c.feet[0] + v[0] * dt, c.feet[1] + v[1] * dt, c.feet[2] + v[2] * dt];
            p.world.place_character(c, to);
            // Not falling: no fall, landing or fall damage while it flies.
            c.grounded = true;
            c.ground_gap = Some(0.0);
            let listed = step_npcs_alone(&mut p.world, &mut p.npcs, &p.actor_block_radii, None, blocks.as_deref_mut(), falls.as_deref_mut(), &mut fell);
            p.world.step();
            meter.tick(began, p.npcs.len(), listed);
        }
        if v != [0.0; 3] {
            player.placed = None;
        }
        p.player_lift.reset();
        inertia.reset();
        let feet = c.feet;
        player.feet = lerp3(p.prev_feet, feet, p.draw_alpha());
        return;
    }
    // A frame with no player character or waiting for a cell is not a played frame for
    // `PhysLoad`: the meter is dropped without recording it.
    let Some(feet) = phys.player.as_ref().map(|c| c.feet) else {
        meter.skip();
        return;
    };
    if !phys.ready_at(feet) {
        // Walked into a cell that is not loaded yet: wait for it. The frames' time does
        // not reach the stepper; its remainder, and the drawn blend, stay as they were.
        meter.skip();
        status.waiting_frames += 1;
        return;
    }
    if let Some(t) = phys.terrain_z(feet[0], feet[1]).filter(|t| feet[2] < t - FELL_THROUGH) {
        warn!("player fell through at ({:.0}, {:.0}, {:.0}), terrain z {t:.0}: back on the ground", feet[0], feet[1], feet[2]);
        phys.place_player([feet[0], feet[1], t], 100_000.0, 100_000.0);
    }
    if intent.forward != 0.0 || intent.right != 0.0 || intent.jump {
        // Moving by themselves: the player is no longer where the game put them.
        player.placed = None;
    }
    let p = &mut *phys;
    let dt = p.world.params.dt;
    let n = p.stepper.advance(&p.world.params, time.delta_secs());
    let c = p.player.as_mut().expect("placed");
    inertia.check_game_placed(player.placed);
    inertia.check_placed(c.feet);
    // The clip's travel owed, spread over this frame's steps (none: kept for the next).
    let carry = if n > 0 { lunge.owed.map(|o| o / (n as f32 * dt)) } else { [0.0; 2] };
    if n > 0 {
        lunge.owed = [0.0; 2];
    }
    for i in 0..n {
        let _tick = info_span!("phys::tick").entered();
        let began = meter.now();
        // Swimming or not is the character's state after the last step.
        let [vx, vy, vz] = if d.auto_walk > 0.0 {
            let (s, c) = player.heading.sin_cos();
            [s * d.auto_walk, c * d.auto_walk, 0.0]
        } else if c.swimming {
            // Running strokes follow the stroke cycle from when the swimmer started to
            // move [observed]; walking stays at its start speed [hypothesis].
            let mut v = swim_velocity(&intent, player.heading, player.pitch, SWIM_SPEEDS);
            // The player's tweak: heavy armour pulls the swimmer down (`StatSpeed`'s third).
            v[2] -= stat_pace.as_deref().map_or(0.0, |p| p.2);
            // The player's tweak: Jump held swims up, at the walking stroke's speed [design: docs/design/tweaks.md].
            if tweaks.as_deref().is_some_and(|t| t.jump_swims_up) && JUMP_HELD.load(std::sync::atomic::Ordering::Relaxed) {
                v[2] = v[2].max(SWIM_SPEEDS.walk);
            }
            match Stroke::of(intent.forward, intent.right) {
                Some(stroke) => {
                    let f = if intent.walk { 1.0 } else { stroke.factor(*stroke_time + dt / 2.0) };
                    *stroke_time += dt;
                    v.map(|x| x * f)
                }
                None => {
                    *stroke_time = 0.0;
                    v
                }
            }
        } else {
            *stroke_time = 0.0;
            // Crippled legs slow the player (combat C8) [design].
            let k = limbs.as_deref().map_or(1.0, crate::combat::PlayerLimbs::speed_mult);
            // Running with a two-handed weapon drawn is slower (`RunPace`): on the run's speed
            // only (below, with the gait).
            let run_pace = pace.as_deref().map_or(1.0, |p| p.0);
            // The Modify Run Speed perks (Travel Light, Tunnel Runner, Implant M-5 …).
            let k = k * perk_pace.as_deref().map_or(1.0, |p| p.0);
            // Slower while reloading, when the setting is on (`crate::combat::reload_pace`).
            let k = k * reload_pace.as_deref().map_or(1.0, |p| p.0);
            // Speed Mult and the body armour's class, as people's (`StatSpeed`).
            let k = k * stat_pace.as_deref().map_or(1.0, |p| p.0);
            // The player's tweak: wading slows the player, the deeper the more [design: docs/design/tweaks.md].
            let wade = tweaks.as_deref().map_or(1.0, |t| t.wading_pace);
            let k = if wade < 1.0 && !c.swimming {
                let depth = p.world.water_surface_at(c.feet[0], c.feet[1]).map_or(0.0, |w| w - c.feet[2]);
                let t = (depth / c.params.swim_depth.max(1.0)).clamp(0.0, 1.0);
                k * (1.0 - t * (1.0 - wade))
            } else {
                k
            };
            // The player's movement penalty tweaks: backwards and sideways at their own
            // share of the pace, a diagonal between them by how much of each it is [design: docs/design/tweaks.md].
            let k = match tweaks.as_deref() {
                Some(t) if t.back_pace < 1.0 || t.strafe_pace < 1.0 => {
                    let (f, r) = (intent.forward.abs(), intent.right.abs());
                    let ahead = if intent.forward < 0.0 { t.back_pace } else { 1.0 };
                    if f + r > 0.0 { k * (f * ahead + r * t.strafe_pace) / (f + r) } else { k }
                }
                _ => k,
            };
            let [x, y] = velocity(&intent, player.heading, *speeds);
            let gait = if intent.walk { k } else { k * run_pace };
            let key = [x * gait, y * gait];
            // Sprint: from the gait's velocity to a multiple of the run's, all of the paces
            // above with it, by its eased share, walking or running (`crate::sprint`).
            let sprint = sprinting.0.as_deref().zip(sprinting.1.as_deref()).map_or((0.0, 1.0), |(s, t)| (s.sprint_share, t.speed));
            let key = if sprint.0 > 0.0 {
                let [rx, ry] = velocity(&MoveIntent { walk: false, ..*intent }, player.heading, *speeds);
                let run = k * run_pace;
                crate::sprint::sprint_velocity(key, [rx * run, ry * run], sprint.0, sprint.1)
            } else {
                key
            };
            // On the ground the velocity speeds up and slows down (`crate::inertia`); in
            // the air the controller's own rules take the keys' velocity.
            let [x, y] = if inertia.ground() && c.grounded { inertia.step(key, intent.walk && sprint.0 < crate::sprint::RUN_GAIT_FROM, dt) } else { key };
            // An attack's clip carries the player (`Lunge`).
            if lunge.carrying && lunge.from.is_none() {
                lunge.from = Some(c.feet);
            }
            [x + carry[0], y + carry[1], 0.0]
        };
        // A blow's knockback pushes the player back (`crate::combat::Knockback`), walking,
        // swimming or not, so it never waits to push later (review of #1219).
        let kb = lunge.knock_step(dt, c.feet);
        let (vx, vy) = (vx + kb[0], vy + kb[1]);
        // Turbo slows the world's clock, not the player: its own movement goes faster by
        // the clock's inverse; falling and jumping stay on the clock (magic-archetypes.md
        // §2) [design].
        let fast = turbo.as_deref().map_or(1.0, |t| t.0);
        let (vx, vy) = (vx * fast, vy * fast);
        let vz = if c.swimming { vz * fast } else { vz };
        p.prev_feet = c.feet;
        let view = Some([player.heading.sin(), player.heading.cos()]);
        // Actors stand in the way where they are now, and those that stand still give
        // way (`crate::blocking`). The characters (the player's, and those of the actors
        // another system walks) are blocks too, where they are at this step, and pass
        // through each other's bodies: this rule alone keeps them apart. The player's
        // block gives way when the player stands still on the ground.
        let blocks_span = info_span!("phys::blocks").entered();
        let mut listed = None;
        let mut go = [vx, vy];
        let mut walking: Vec<[f32; 2]> = p.npcs.values().map(|b| b.velocity).collect();
        let mut pushed = [0.0f32; 2];
        c.passes_characters = blocks.is_some();
        for n in p.npcs.values_mut() {
            n.ch.passes_characters = blocks.is_some();
        }
        if let Some(b) = blocks.as_deref_mut() {
            use crate::blocking::{clip, Block, Push, PLAYER};
            b.player_held = (b.player_held - dt).max(0.0);
            let standing = go == [0.0, 0.0] && c.grounded && !c.swimming && b.player_held <= dt * 0.5;
            let placed = b.list.len();
            let mut all = std::mem::take(&mut b.list);
            all.extend(p.npcs.values().map(|n| Block::character(n.ch.id, n.ch.feet, crate::blocking::walking_radius(n.ch.params.radius, p.actor_block_radii.get(&n.ch.id).copied()), n.ch.params.height, false)));
            all.push(Block::character(PLAYER, c.feet, c.params.radius, c.params.height, standing));
            listed = Some(all.len());
            // First the walking actors: what they push is their owner's to move, but for
            // the player, which their pushes move in this step.
            let mut pushes: Vec<Push> = Vec::new();
            let mut stopped = false;
            for (v, n) in walking.iter_mut().zip(p.npcs.values()) {
                let asked = *v;
                let radius = crate::blocking::walking_radius(n.ch.params.radius, p.actor_block_radii.get(&n.ch.id).copied());
                *v = if b.carried.contains(&n.ch.id) {
                    crate::blocking::clip_pushing_nobody(n.ch.id, asked, n.ch.feet, radius, n.ch.params.height, dt, &mut all)
                } else {
                    clip(n.ch.id, asked, n.ch.feet, radius, n.ch.params.height, dt, &mut all, &mut pushes)
                };
                stopped |= *v != asked;
            }
            b.steps_actor_blocked += u64::from(stopped);
            for push in pushes.iter().filter(|p| p.actor == PLAYER) {
                pushed = [pushed[0] + push.by[0], pushed[1] + push.by[1]];
            }
            pushes.retain(|p| p.actor != PLAYER);
            b.steps_pushing[1] += u64::from(!pushes.is_empty());
            b.steps_pushing[2] += u64::from(pushed != [0.0, 0.0]);
            b.pushes.extend(pushes);
            go = [go[0] + pushed[0] / dt, go[1] + pushed[1] / dt];
            let (asked, before) = (go, b.pushes.len());
            go = clip(PLAYER, go, c.feet, c.params.radius, c.params.height, dt, &mut all, &mut b.pushes);
            b.steps_blocked += u64::from(go != asked);
            b.steps_pushing[0] += u64::from(b.pushes.len() > before);
            // The placed actors' blocks, moved by what pushed them, are kept.
            all.truncate(placed);
            b.list = all;
        }
        drop(blocks_span);
        let move_span = info_span!("phys::player_move").entered();
        let was_swimming = c.swimming;
        let was_footing = crate::phys::step_lift::footing(c);
        // The player's tweaks: the steepest walkable slope, and a crippled leg's jump [design: docs/design/tweaks.md].
        if let Some(t) = tweaks.as_deref() {
            let base = fnv_phys::CharacterParams::default();
            c.params.max_slope_deg = t.walk_angle;
            let legs = limbs.as_deref().map_or(0, |l| i32::from(l.crippled(29)) + i32::from(l.crippled(30)));
            // Speed Mult (and armour) scale it too with the player's tweak [design: docs/design/tweaks.md].
            let speed = if t.game.speed_scales_jump { stat_pace.as_deref().map_or(1.0, |p| p.0) } else { 1.0 };
            let agility = stat_pace.as_deref().map_or(1.0, |p| p.1);
            c.params.jump_height = base.jump_height * t.crippled_leg_jump.powi(legs) * speed * agility;
        }
        let s = p.world.move_character(c, CharacterInput { velocity: go, jump: intent.jump && i == 0, vertical: vz, view });
        if inertia.ground() {
            // A landing lands into the inertia, not the game's landing speed profile; a
            // hard one takes speed out of it (Landing weight).
            c.since_landing = None;
            if let Some(speed) = s.landed_speed.filter(|_| inertia.landing_weight()) {
                inertia.land(speed);
            }
        }
        if !inertia.ground() || !c.grounded || c.swimming || d.auto_walk > 0.0 {
            // Off the ground, in the water or switched off: the spring takes what the
            // character did, so the ground picks up from there.
            inertia.follow(c.velocity);
        }
        // The drawn body and view climb steps and bumps smoothly (`phys::step_lift`).
        let moved = (c.feet[0] - p.prev_feet[0]).hypot(c.feet[1] - p.prev_feet[1]);
        p.player_lift.step(c.feet[2] - p.prev_feet[2], moved, was_footing && crate::phys::step_lift::footing(c), dt);
        if s.swimming != was_swimming {
            let depth = p.world.water_surface_at(s.feet[0], s.feet[1]).map_or("no water".to_string(), |w| format!("feet {:.1} below the water", w - s.feet[2]));
            info!("player: {} at ({:.0}, {:.0}, {:.1}), {depth}", if s.swimming { "starts swimming" } else { "stops swimming" }, s.feet[0], s.feet[1], s.feet[2]);
        }
        if let Some(speed) = s.landed_speed {
            landed.write(crate::rumble::Landed { speed });
        }
        // A fall's damage (`crate::falls`).
        if let Some(f) = falls.as_deref_mut() {
            f.note_player(c.id);
        }
        if let Some(height) = falls.as_deref_mut().and_then(|f| f.step(c.id, p.prev_feet[2], &s)) {
            fell.write(crate::falls::Fell { actor: FormId::PLAYER_REF, height, feet: s.feet });
        }
        // Pushed and held by something: the actors walk round the player for a while.
        let want = pushed[0].hypot(pushed[1]);
        if want > 1e-4 {
            let got = ((c.feet[0] - p.prev_feet[0]) * pushed[0] + (c.feet[1] - p.prev_feet[1]) * pushed[1]) / want;
            if let Some(b) = blocks.as_deref_mut().filter(|_| got < crate::blocking::HELD_SHARE * want) {
                b.player_held = crate::blocking::HELD_SECS;
            }
        }
        drop(move_span);
        move_npcs(&mut p.world, &mut p.npcs, walking, falls.as_deref_mut(), &mut fell, dt);
        {
            let _span = info_span!("phys::world_step").entered();
            p.world.step();
        }
        status.record(&s, p.prev_feet, [vx, vy], dt);
        // The player's controller and each walked actor's.
        meter.tick(began, 1 + p.npcs.len(), listed);
    }
    let _after = info_span!("phys::after").entered();
    if n > 0 {
        intent.jump = false;
    }
    if let Some(from) = lunge.from.filter(|_| !lunge.carrying) {
        lunge.from = None;
        info!("player: lunge done: {:.0} units from where it started", (c.feet[0] - from[0]).hypot(c.feet[1] - from[1]));
    }
    inertia.end_frame(c.feet, c.grounded && !c.swimming);
    for r in std::mem::take(&mut inertia.reports) {
        match r {
            crate::inertia::Report::Started { seconds, speed } => info!("movement inertia: up to {speed:.0} units/s in {seconds:.2}s"),
            crate::inertia::Report::Stopped { seconds, distance, speed } => info!("movement inertia: glided {distance:.1} units in {seconds:.2}s from {speed:.0} units/s"),
        }
    }
    let feet = c.feet;
    player.feet = lerp3(p.prev_feet, feet, p.draw_alpha());
    if n > 0 {
        // Where the bodies are drawn after the steps (the drawn ground under each).
        p.update_drawn_sinks();
    }
}

/// The actors another system walks, by the velocity it gave each: their moves planned at
/// once on the compute pool (`PhysWorld::plan_move`, which only reads the world), then put
/// into the world in turn, as each would have been alone.
fn move_npcs(
    world: &mut fnv_phys::PhysWorld,
    npcs: &mut std::collections::BTreeMap<u32, crate::phys::NpcBody>,
    walking: Vec<[f32; 2]>,
    mut falls: Option<&mut crate::falls::Falls>,
    fell: &mut MessageWriter<crate::falls::Fell>,
    dt: f32,
) {
    let plan_span = info_span!("phys::npc_plan").entered();
    world.update_queries_for(fnv_phys::QueryConsumer::Npc);
    let reader = &*world;
    let mut moving: Vec<(u32, &mut crate::phys::NpcBody, [f32; 2])> = npcs.iter_mut().zip(walking).map(|((&id, b), v)| (id, b, v)).collect();
    let plan = |b: &mut crate::phys::NpcBody, v: [f32; 2]| {
        b.prev_feet = b.ch.feet;
        let was_footing = crate::phys::step_lift::footing(&b.ch);
        let (s, push) = reader.plan_move(&mut b.ch, CharacterInput { velocity: v, jump: std::mem::take(&mut b.jump), ..Default::default() });
        (was_footing, s, push)
    };
    let plans: Vec<_> = if moving.len() < PARALLEL_MOVES {
        moving.iter_mut().map(|(_, b, v)| plan(b, *v)).collect()
    } else {
        moving.par_chunk_map_mut(ComputeTaskPool::get(), MOVE_CHUNK, |_, chunk| chunk.iter_mut().map(|(_, b, v)| plan(b, *v)).collect::<Vec<_>>()).into_iter().flatten().collect()
    };
    drop(plan_span);
    let _apply = info_span!("phys::npc_apply").entered();
    for ((id, b, _), (was_footing, s, push)) in moving.into_iter().zip(plans) {
        world.apply_move(&b.ch, push);
        let (f, from) = (b.ch.feet, b.prev_feet);
        b.lift.step(f[2] - from[2], (f[0] - from[0]).hypot(f[1] - from[1]), was_footing && crate::phys::step_lift::footing(&b.ch), dt);
        if let Some(height) = falls.as_deref_mut().and_then(|f| f.step(id, b.prev_feet[2], &s)) {
            fell.write(crate::falls::Fell { actor: FormId(id), height, feet: s.feet });
        }
    }
}

/// One step of the actors another system walks while the player's character is not
/// moved by [`step`]'s own rule: held by furniture, or flying with ToggleCollision. Without
/// it they stood still the whole time, and the residents' stuck rule moved them on along
/// their paths every second (owner report 2026-10-27: Doc Mitchell "teleporting" to his
/// chair with the player already on the couch). They keep apart by `crate::blocking` as in
/// a step of the player's; the player's block is where the furniture holds it, not pushed,
/// or none when the player passes through everything. Returns the length of the step's
/// block list (none without `ActorBlocks`), for `crate::phys_load`.
#[allow(clippy::too_many_arguments)]
fn step_npcs_alone(
    world: &mut fnv_phys::PhysWorld,
    npcs: &mut std::collections::BTreeMap<u32, crate::phys::NpcBody>,
    actor_block_radii: &std::collections::HashMap<u32, f32>,
    player: Option<([f32; 3], f32, f32)>,
    blocks: Option<&mut crate::blocking::ActorBlocks>,
    falls: Option<&mut crate::falls::Falls>,
    fell: &mut MessageWriter<crate::falls::Fell>,
) -> Option<usize> {
    use crate::blocking::{clip, Block, Push, PLAYER};
    let dt = world.params.dt;
    let mut listed = None;
    let mut walking: Vec<[f32; 2]> = npcs.values().map(|b| b.velocity).collect();
    for n in npcs.values_mut() {
        n.ch.passes_characters = blocks.is_some();
    }
    if let Some(b) = blocks {
        let placed = b.list.len();
        let mut all = std::mem::take(&mut b.list);
        all.extend(npcs.values().map(|n| Block::character(n.ch.id, n.ch.feet, crate::blocking::walking_radius(n.ch.params.radius, actor_block_radii.get(&n.ch.id).copied()), n.ch.params.height, false)));
        if let Some((feet, radius, height)) = player {
            all.push(Block::character(PLAYER, feet, radius, height, false));
        }
        listed = Some(all.len());
        let mut pushes: Vec<Push> = Vec::new();
        let mut stopped = false;
        for (v, n) in walking.iter_mut().zip(npcs.values()) {
            let asked = *v;
            let radius = crate::blocking::walking_radius(n.ch.params.radius, actor_block_radii.get(&n.ch.id).copied());
            *v = if b.carried.contains(&n.ch.id) {
                crate::blocking::clip_pushing_nobody(n.ch.id, asked, n.ch.feet, radius, n.ch.params.height, dt, &mut all)
            } else {
                clip(n.ch.id, asked, n.ch.feet, radius, n.ch.params.height, dt, &mut all, &mut pushes)
            };
            stopped |= *v != asked;
        }
        b.steps_actor_blocked += u64::from(stopped);
        pushes.retain(|p| p.actor != PLAYER);
        b.steps_pushing[1] += u64::from(!pushes.is_empty());
        b.pushes.extend(pushes);
        all.truncate(placed);
        b.list = all;
    }
    move_npcs(world, npcs, walking, falls, fell, dt);
    listed
}

/// Feet, cell, heading and pitch last written to the simulation.
type SimPose = ([f32; 3], Option<u32>, f32, f32);

/// The simulation's X angle of the player (radians) for a view pitch: the view's pitch with
/// the sign turned, so that GetAngle X reads -89 looking up and 89 looking at the feet
/// [documented: GECK wiki GetAngle]; [`Player::pitch`] is negative looking down.
pub fn sim_pitch(pitch: f32) -> f32 {
    -pitch
}

/// The simulation's player reference stands where the character does, in the cell of the
/// worldspace being played (`PhysState::worldspace`, which door travel changes) or the interior.
fn sync_sim(
    phys: Res<PhysState>,
    player: Res<Player>,
    d: Res<PlayWorld>,
    mut sim: ResMut<SimState>,
    mut last: Local<Option<SimPose>>,
) {
    let Some(c) = &phys.player else { return };
    let cell = match phys.space {
        Space::Interior(id) => Some(id),
        Space::Exterior => {
            let g = cell_of(c.feet[0], c.feet[1]);
            d.index.worldspaces.get(&phys.worldspace).and_then(|w| w.grid.get(&g)).copied()
        }
    };
    let now = (c.feet, cell, player.heading, player.pitch);
    if *last == Some(now) {
        return;
    }
    *last = Some(now);
    match sim.world.get(FormId::PLAYER_REF) {
        Ok(s) => {
            s.pos = c.feet;
            s.cell = cell.map(FormId);
            // The reference's z rotation is its compass heading in radians [hypothesis:
            // as door teleport markers' rotations are read for Travel].
            s.rot[2] = player.heading;
            // Its x rotation is the view's pitch (`sim_pitch`); a script's SetAngle of the
            // player is not read back into the view [design: no shipped call].
            s.rot[0] = sim_pitch(player.pitch);
        }
        Err(e) => warn_once!("simulation player reference: {e}"),
    }
}

fn log_status(time: Res<Time>, phys: Res<PhysState>, player: Res<Player>, mut status: ResMut<Status>) {
    status.elapsed += time.delta_secs();
    if status.elapsed < 1.0 {
        return;
    }
    let s = std::mem::take(&mut *status);
    let Some(c) = &phys.player else { return };
    let f = c.feet;
    let terrain = phys.terrain_z(f[0], f[1]);
    let water = phys.world.water_surface_at(f[0], f[1]);
    let touched: Vec<String> = s.touched.iter().map(|id| format!("{id:08X}")).collect();
    debug!(
        "player: feet ({:.0}, {:.0}, {:.1}) heading {:.1}° pitch {:.1}° terrain {} water {} swimming {}/{} grounded {}/{} sliding {} moved {:.0} of {:.0} units/s commanded, touched{} [{}]; physics: {} cells, {} refs with collision, {} steps{}",
        f[0],
        f[1],
        f[2],
        player.heading.to_degrees(),
        player.pitch.to_degrees(),
        terrain.map_or("-".to_string(), |t| format!("z {t:.1} (feet {:+.1})", f[2] - t)),
        water.map_or("-".to_string(), |w| format!("z {w:.1} (feet {:+.1})", f[2] - w)),
        s.swimming_steps,
        s.steps,
        s.grounded_steps,
        s.steps,
        s.last.as_ref().is_some_and(|l| l.sliding),
        s.moved / s.elapsed,
        s.commanded / s.elapsed,
        if s.touched_terrain { " terrain" } else { "" },
        touched.join(" "),
        phys.cells.len(),
        phys.world.stats().refs,
        phys.world.steps(),
        if s.waiting_frames > 0 { format!(", waited {} frames for its cell", s.waiting_frames) } else { String::new() },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, PI};

    const SPEEDS: MoveSpeeds = MoveSpeeds { walk: 77.0, run: 308.0 };

    /// A menu, a conversation or a loading screen keeps the share of a step everything is
    /// drawn at: nothing drawn between steps shifts when one opens or closes.
    #[test]
    fn the_drawn_blend_holds_while_not_playing() {
        for state in [PlayState::Menu, PlayState::Dialogue, PlayState::Loading] {
            let mut app = App::new();
            let player = Player { heading: 0.0, pitch: 0.0, eye_height: 120.0, feet: [0.0; 3], body_eye: None, body_view: None, placed: None };
            app.insert_resource(State::new(state))
                .insert_resource(crate::script_draw::tests::play_over(&[]))
                .insert_resource(SPEEDS)
                .insert_resource(player)
                .insert_resource(PhysState::default())
                .init_resource::<Time>()
                .init_resource::<MoveIntent>()
                .init_resource::<Status>()
                .init_resource::<crate::furniture::PlayerSeat>()
                .init_resource::<NoClip>()
                .init_resource::<Lunge>()
                .init_resource::<crate::inertia::MovementInertia>()
                .add_message::<crate::rumble::Landed>()
                .add_message::<crate::falls::Fell>()
                .add_systems(Update, step);
            let dt = app.world().resource::<PhysState>().world.params.dt;
            app.world_mut().resource_mut::<PhysState>().stepper.accumulated = f64::from(dt) * 0.4;
            let before = app.world().resource::<PhysState>().draw_alpha();
            for _ in 0..3 {
                app.update();
            }
            let p = app.world().resource::<PhysState>();
            assert_eq!(p.draw_alpha(), before, "{state:?}");
            assert!((before - 0.4).abs() < 1e-5);
        }
    }

    /// #2201: at the default Vertical Sensitivity (1) the mouse and the right stick turn the
    /// view exactly as before; another value changes the pitch only.
    #[test]
    fn the_vertical_sensitivity_at_its_default_leaves_the_look_as_it_was() {
        let b = crate::bindings::Bindings::game_defaults();
        let mut pad = b.clone();
        pad.device = crate::bindings::InputDevice::Controller;
        // A mouse's counts, and a stick's turn as the look system builds it.
        let mouse = Vec2::new(37.0, -21.0);
        let stick = Vec2::new(0.6 * b.look_speed[0] * 0.016, -0.4 * b.look_speed[1] * 0.016);
        for (bind, delta, s) in [(&b, mouse, b.mouse_sensitivity), (&pad, stick, STICK_LOOK_RADIANS)] {
            assert_eq!(bind.vertical_sensitivity(), 1.0);
            let scaled = vertical_scaled(delta, bind.vertical_sensitivity());
            assert_eq!(scaled, delta);
            for inv in [false, true] {
                assert_eq!(look_with(0.3, 0.1, scaled, s, inv), look_with(0.3, 0.1, delta, s, inv));
            }
            // Half: the heading as before, the pitch turned half as far.
            let (h, p) = look_with(0.3, 0.1, vertical_scaled(delta, 0.5), s, false);
            let (h1, p1) = look_with(0.3, 0.1, delta, s, false);
            assert_eq!(h, h1);
            assert!(((p - 0.1) - (p1 - 0.1) / 2.0).abs() < 1e-6, "{p} {p1}");
        }
    }

    /// Owner 2026-11-15: the Aim Sensitivity scales the look by how far the view is zoomed
    /// for the aim (`crate::aim_zoom::AimZoom::share`); unzoomed (the hip look), and at 100%,
    /// the look is exactly as before.
    #[test]
    fn the_aim_sensitivity_scales_the_look_only_while_zoomed_for_the_aim() {
        use bevy::input::mouse::AccumulatedMouseMotion;
        use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
        for zoomed in [0.0, 0.25, 0.5, 1.0] {
            assert_eq!(aim_scale(1.0, zoomed), 1.0, "100%: as before");
        }
        for aim in [crate::bindings::SENSITIVITY_LEAST, 0.5, 1.5, crate::bindings::SENSITIVITY_MOST] {
            assert_eq!(aim_scale(aim, 0.0), 1.0, "unzoomed: as before");
            assert!((aim_scale(aim, 1.0) - aim).abs() < 1e-6, "{aim}");
        }
        assert!((aim_scale(0.5, 0.5) - 0.75).abs() < 1e-6, "in step with the zoom");
        // The zoom's share as the field of view moves: none at rest, all at the full zoom.
        let mut z = crate::aim_zoom::AimZoom::default();
        assert_eq!(z.share(), 0.0);
        z.blend = 1.0;
        assert_eq!(z.share(), 1.0);
        // Through the look system: the mouse's turn this frame at an Aim Sensitivity `aim`
        // with the aim's zoom at `blend`.
        let turned = |aim: f32, blend: f32| {
            let mut app = App::new();
            let mut zoom = crate::aim_zoom::AimZoom::default();
            zoom.blend = blend;
            let mut b = Bindings::game_defaults();
            b.aim = [aim, 1.0];
            app.insert_resource(State::new(PlayState::Playing))
                .insert_resource(Time::<()>::default())
                .insert_resource(b)
                .insert_resource(zoom)
                .init_resource::<crate::control::ControlLocks>()
                .init_resource::<PlayerControl>()
                .init_resource::<crate::furniture::PlayerSeat>()
                .insert_resource(AccumulatedMouseMotion { delta: Vec2::new(80.0, 20.0) })
                .insert_resource(Player { heading: 1.0, pitch: 0.0, eye_height: 112.0, feet: [0.0; 3], body_eye: None, body_view: None, placed: None })
                .add_systems(Update, mouse_look);
            app.world_mut().spawn((PrimaryWindow, CursorOptions { grab_mode: CursorGrabMode::Confined, ..default() }));
            app.update();
            let p = app.world().resource::<Player>();
            (p.heading - 1.0, p.pitch)
        };
        let hip = turned(1.0, 0.0);
        assert!(hip.0 > 0.0 && hip.1 < 0.0, "{hip:?}");
        assert_eq!(turned(0.5, 0.0), hip, "unzoomed the Aim Sensitivity does nothing");
        assert_eq!(turned(1.0, 1.0), hip, "at 100% the zoomed look is as before");
        let half = turned(0.5, 1.0);
        assert!((half.0 - hip.0 / 2.0).abs() < 1e-6 && (half.1 - hip.1 / 2.0).abs() < 1e-6, "both axes half as far: {half:?} {hip:?}");
    }

    fn close(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
    }

    #[test]
    fn forward_follows_the_heading() {
        let fwd = MoveIntent { forward: 1.0, ..default() };
        assert!(close(velocity(&fwd, 0.0, SPEEDS), [0.0, 308.0])); // north
        assert!(close(velocity(&fwd, FRAC_PI_2, SPEEDS), [308.0, 0.0])); // east
        assert!(close(velocity(&fwd, PI, SPEEDS), [0.0, -308.0])); // south
        let back = MoveIntent { forward: -1.0, walk: true, ..default() };
        assert!(close(velocity(&back, 0.0, SPEEDS), [0.0, -77.0 * BACK_SPEED_FRACTION]));
    }

    #[test]
    fn strafing_is_to_the_right_of_the_view() {
        let right = MoveIntent { right: 1.0, ..default() };
        // Facing north, right is east; facing east, right is south.
        assert!(close(velocity(&right, 0.0, SPEEDS), [308.0, 0.0]));
        assert!(close(velocity(&right, FRAC_PI_2, SPEEDS), [0.0, -308.0]));
        let left = MoveIntent { right: -1.0, ..default() };
        assert!(close(velocity(&left, 0.0, SPEEDS), [-308.0, 0.0]));
    }

    #[test]
    fn diagonals_are_not_faster_and_nothing_is_still() {
        let d = MoveIntent { forward: 1.0, right: 1.0, ..default() };
        let v = velocity(&d, 0.3, SPEEDS);
        assert!((v[0].hypot(v[1]) - 308.0).abs() < 1e-3);
        assert_eq!(velocity(&MoveIntent::default(), 1.0, SPEEDS), [0.0, 0.0]);
    }

    #[test]
    fn speeds_from_settings() {
        assert_eq!(MoveSpeeds::from_settings(Some(77.0), Some(4.0)), SPEEDS);
        assert_eq!(MoveSpeeds::from_settings(None, None), SPEEDS);
        assert_eq!(MoveSpeeds::from_settings(Some(100.0), Some(-1.0)), MoveSpeeds { walk: 100.0, run: 400.0 });
    }

    #[test]
    fn the_simulation_pitch_is_positive_looking_down() {
        let (_, down) = look(0.0, 0.0, Vec2::new(0.0, 1.0e6));
        assert_eq!(sim_pitch(down).to_degrees().round(), 89.0, "GetAngle X 89 at the feet");
        let (_, up) = look(0.0, 0.0, Vec2::new(0.0, -1.0e6));
        assert_eq!(sim_pitch(up).to_degrees().round(), -89.0, "GetAngle X -89 looking up");
    }

    #[test]
    fn looking_turns_right_and_clamps_pitch() {
        let (h, p) = look(0.0, 0.0, Vec2::new(100.0, 0.0));
        assert!(h > 0.0 && p == 0.0, "mouse right turns clockwise (towards east)");
        let (h, _) = look(0.0, 0.0, Vec2::new(-1.0, 0.0));
        assert!(h > 6.0 && h < std::f32::consts::TAU, "headings wrap into [0, 2π)");
        let (_, p) = look(0.0, 0.0, Vec2::new(0.0, 1.0e6));
        assert_eq!(p, -MAX_PITCH, "mouse down looks down, at most straight down");
    }

    #[test]
    fn swimming_follows_the_view_at_full_speed_backwards() {
        let fwd = MoveIntent { forward: 1.0, ..default() };
        let down30 = -30f32.to_radians();
        let v = swim_velocity(&fwd, FRAC_PI_2, down30, SWIM_SPEEDS);
        assert!((v[0] - 339.0 * 30f32.to_radians().cos()).abs() < 1e-2 && v[1].abs() < 1e-3, "{v:?}");
        assert!((v[2] + 169.5).abs() < 1e-2, "{v:?}");
        let back = MoveIntent { forward: -1.0, ..default() };
        let v = swim_velocity(&back, 0.0, 0.0, SWIM_SPEEDS);
        assert!((v[1] + 339.0).abs() < 1e-3, "{v:?}");
        let right = MoveIntent { right: 1.0, walk: true, ..default() };
        let v = swim_velocity(&right, 0.0, down30, SWIM_SPEEDS);
        assert!((v[0] - 86.0).abs() < 1e-3 && v[2] == 0.0, "{v:?}");
        assert_eq!(swim_velocity(&MoveIntent::default(), 1.0, 0.5, SWIM_SPEEDS), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn toggles_and_the_run_key() {
        let mut c = Controls::default();
        let mut i = MoveIntent::default();
        let frame = |c: &mut Controls, i: &mut MoveIntent, held: &[KeyCode], pressed: &[KeyCode]| {
            read_keys(c, i, |k| held.contains(&k) || pressed.contains(&k), |k| pressed.contains(&k));
        };
        // Always Run on: W runs, Shift+W walks.
        frame(&mut c, &mut i, &[keys::FORWARD], &[]);
        assert!(i.forward == 1.0 && !i.walk && !i.sneak);
        frame(&mut c, &mut i, &[keys::FORWARD, KeyCode::ShiftLeft], &[]);
        assert!(i.walk);
        // Caps Lock turns it off: W walks, Shift+W runs.
        frame(&mut c, &mut i, &[keys::FORWARD], &[keys::ALWAYS_RUN]);
        assert!(!c.always_run && i.walk);
        frame(&mut c, &mut i, &[keys::FORWARD, KeyCode::ShiftLeft], &[]);
        assert!(!i.walk);
        // Ctrl toggles sneaking; it stays on with no key held.
        frame(&mut c, &mut i, &[], &[keys::SNEAK]);
        assert!(c.sneaking && i.sneak);
        frame(&mut c, &mut i, &[], &[]);
        assert!(i.sneak);
        frame(&mut c, &mut i, &[], &[keys::SNEAK]);
        assert!(!i.sneak);
        // Auto Move walks forward until Back (or Forward) is pressed.
        frame(&mut c, &mut i, &[], &[keys::AUTO_MOVE]);
        assert_eq!(i.forward, 1.0);
        frame(&mut c, &mut i, &[], &[]);
        assert_eq!(i.forward, 1.0);
        frame(&mut c, &mut i, &[keys::BACK], &[keys::BACK]);
        assert!(!c.auto_move && i.forward == -1.0);
        // Jump is latched until a step clears it.
        frame(&mut c, &mut i, &[], &[keys::JUMP]);
        frame(&mut c, &mut i, &[], &[]);
        assert!(i.jump);
    }

    #[test]
    fn sneaking_is_slower() {
        let run = MoveIntent { forward: 1.0, ..default() };
        let sneak = MoveIntent { sneak: true, ..run };
        let v = velocity(&sneak, 0.0, MoveSpeeds::OBSERVED);
        assert!((v[1] - 197.7).abs() < 1e-3, "{v:?}");
        assert!((velocity(&run, 0.0, MoveSpeeds::OBSERVED)[1] - 344.0).abs() < 1e-3);
    }

    #[test]
    fn interpolation() {
        assert_eq!(lerp3([0.0; 3], [2.0, 4.0, 6.0], 0.5), [1.0, 2.0, 3.0]);
    }
}
