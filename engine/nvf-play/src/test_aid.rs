//! Play-wide test aids for unattended in-game checks (`--play` only; no effect unless set;
//! docs/design/layer4-play.md §6), next to `NVF_PLAY_TEST_ACTIVATE` (activate.rs),
//! `NVF_PLAY_TEST_NEAR` (player.rs), `NVF_PLAY_TEST_MENU` and `NVF_PLAY_TEST_DROP`
//! (menu/test_aid.rs), `NVF_PLAY_TEST_TILE_MENUS` (tiles/vanilla.rs: `[UI] TileMenus=` for
//! the run, read at start). Times are seconds after the start.
//!
//! **Which clock.** With `NVF_PLAY_AIDS_FROM_SCRIPTS=1` (the smoke runner sets it for
//! `--play` runs) the aids that act on the world wait until the scripts first run and
//! count on the world aids' clock, held back by as long as that start came after
//! `crate::pause::SCRIPTS_START_SECONDS` ([`crate::pause::WorldAidClock`]; the log says
//! `test aids: clock starts when the scripts first run, at <N> s`). These are: here
//! SHOTS, LOOK, MOVE, CONTROL, KEYS; `NVF_PLAY_TEST_CONSOLE`, `_SCRIPT`, `_ACTIVATE`,
//! `_TALK`, `_WAIT`, `_HIT`, `_MENU`, `_BARTER`, `_DROP`, `_PIPBOY`, `_PIPBOY_STATS`,
//! `_LOCKPICK`, `_TERMINAL`, `_POKER`, `_ROULETTE`, `_SLOTS`, `_BLACKJACK`, `_RECIPE`, `_REPAIR`,
//! `_LEVELUP`, `_CHARGEN`, `_CREATOR` (the character menus open in the world, from a
//! script or the console) and `_BATTLE`. Seconds after the start in every run: GOTO and
//! FACE (they set the stage for the scripts' first frame), `NVF_PLAY_TEST_MAIN_MENU`,
//! `_INTRO_SKIP`, `_INTRO_SHOT`, `_PAUSE`, `_PAUSE_MENU`, `_SETTINGS` (they work before
//! the world too) and `_AUDIO_REOPEN`; `_VIGOR` counts from its menu's first frame and
//! `_RADIO` on the radio's own clock. Without the switch every aid counts from the start.
//!
//! - `NVF_PLAY_TEST_SHOT_FLATNESS=1`: each of those shots also logs how much of the view is
//!   one colour ([`one_colour_percent`]), for a scenario to check without reading the file.
//! - `NVF_PLAY_TEST_SHOT_DIFF=1`: each of those shots also logs how far it is from the one before
//!   and from the one before that ([`picture_difference`]), with the tolerance that gives another
//!   pair as far apart in time ([`difference_tolerance`]), for a scenario that switches something
//!   between shots of one still view. Only in tests, debug builds and builds with the `dev-aids`
//!   feature (nvforever dev_aids.rs).
//! - `NVF_PLAY_TEST_SHOTS=<seconds>[:<path.png>][,...]`: save a screenshot at each time,
//!   to the path given, else as `play-<seconds>s.png` in `NVF_PLAY_TEST_SHOT_DIR`
//!   (default: the current folder).
//! - `NVF_PLAY_TEST_FACE=<seconds:reference FormID hex,...>`: at each time, put the player
//!   [`FACE_DISTANCE`] units in front of the reference, by the arrival rule, facing it
//!   level. In front: towards the door's own arrival point (where its partner door's XTEL
//!   puts travellers, so on the side the door is used from), else towards the player. A
//!   stand-in for walking up to a door after a travel, so one run can go through a door
//!   and back. Not a game behaviour.
//! - `NVF_PLAY_TEST_LOOK=<seconds>:<heading deg>:<pitch deg>[,...]` turns the view (the
//!   compass heading, 0 north, 90 east; the pitch, negative looks down) at those times,
//!   as if the mouse had moved there (ignored, as the mouse is, while an animation
//!   drives the view, `player::ViewDriven`; and while a zoomed conversation locks the view,
//!   `dialogue_look::view_locked`).
//! - `NVF_PLAY_TEST_GOTO=<seconds>:<x>:<y>:<z>[,...]` puts the player's feet on whatever
//!   is under that point (game space, in the current cell or worldspace) at those times.
//! - `NVF_PLAY_TEST_GIVE=<item FormID hex>[:<count>][,...]` adds items to the player's
//!   inventory at the start (`World::add_item`), e.g. something to drop.
//! - `NVF_PLAY_TEST_EQUIP=<item FormID hex>[,...]` gives the player one of each item
//!   unless carried, and equips it (`World::equip`; a weapon for the player body to draw).
//! - `NVF_PLAY_TEST_MOVE=<seconds>:<forward>:<right>[:walk][,...]` holds the movement keys
//!   from each time on: `forward` and `right` in −1..1 (W/S, D/A), `walk` as Shift held;
//!   `<seconds>:0:0` stops.
//! - `NVF_PLAY_TEST_CONTROL=<seconds>:<what>[,...]` presses a control at each time:
//!   `sneak` (Left Ctrl), `weapon` (R), `aim` (the right mouse button, pressed or
//!   released), `view` (F), `jump` (Space), `zoom=<units>` (the third-person distance),
//!   `orbit=<yaw deg>:<pitch deg>` (Toggle POV held with the camera circled that far
//!   round the player) or `release` (Toggle POV let go); for combat (step C4): `fire`
//!   (Use pressed, or let go if held), `reload` (a Ready Item tap with the weapon out),
//!   `swap` (Ammo Swap), `aimactor` (turn the view to the nearest actor's aim point: its
//!   torso, or the middle of its single capsule), `aimflat` (face the nearest actor
//!   with the view level, as a player looking ahead at a short creature), `aimcorpse` (to the trunk of the
//!   nearest ragdoll), `aimnpc` (the same, NPCs only),
//!   `aimnpc=<share>` (at that share of the NPC's height, 0 feet to 1 top: the head or
//!   legs for limb hits; `aimnpc=<share>@<n>` at that height over body part `n`'s centre,
//!   e.g. the forehead of a leaning NPC whose head is off the body's middle), `aimpart=<n>` (at the nearest NPC's body part `n`, its index in
//!   the NPC's BPTD, step C8), `aimref=<FormID hex>` (at the collision body of a placed
//!   object, e.g. a bottle to shoot), `equip=<FormID hex>` (give one unless carried and
//!   equip it, as `NVF_PLAY_TEST_EQUIP`) and `condition=<percent>` (set the condition of
//!   everything the player has equipped, e.g. a weapon about to break). `aimpart=254`
//!   aims at the weapon the nearest NPC holds drawn (`hitvol::WEAPON_PART`);
//!   `aimcreaturepart=<n>` aims at the nearest creature's body part `n` (its BPTD index,
//!   e.g. a bighorner's calf, which its head does not hide when it faces the player). The `aim…`
//!   presses put the point under the crosshair: in third person the view turns so the
//!   over-the-shoulder camera's line passes through it (`control::Crosshair::aim_at`).
//!   `flashlight` switches the flashlight as a hold of Menu Mode does (`crate::flashlight`).
//!   `aimvats` aims at the nearest living actor that has no VATS mode mark yet, as
//!   `aimactor` does, so a click marks it (`crate::vats_mode`, docs/design/vats-mode.md §13).
//! - `NVF_PLAY_TEST_KEYS=<seconds>:<key>[:<hold seconds>][,...]` presses a real key or
//!   mouse button at each time and lets it go after the hold (default: the next frame),
//!   through Bevy's input state, so the game's bindings (`crate::bindings`) are what
//!   read it: `ctrl` (left Ctrl), `shift`, `space`, `tab`, `esc`, `enter`, a letter or
//!   digit (`e`, `2`), `f1`..`f12`, an arrow (`up`, `down`, `left`, `right`), `mouse-left` /
//!   `mouse-right` / `mouse-middle`, or a notch of the wheel (`wheel-up`, `wheel-down`).
//!   `pad-X` (or another button named by `crate::pad::parse_pad`) presses a synthetic
//!   controller's button through `pad::menu_keys`; automatic switching consumes its
//!   first press when it changes Input Device [design, #2200; `pad::device`].
//!   Unlike `NVF_PLAY_TEST_CONTROL` it tests the key's path, not just what it does.
//!
//! - `NVF_PLAY_TEST_MELEE_KNOCKBACK=<target FormID hex>:<x>:<y>:<z>`: one player power blow
//!   on one actor put `x`, `y` from the player and `z` over the ground there (it may start
//!   far away and unloaded) and left standing, gated on readiness
//!   ([`melee_knockback`], with nvforever `residents::melee_knockback`).
//!
//! The aids that stand in for the keys and the mouse (`NVF_PLAY_TEST_MOVE`,
//! `NVF_PLAY_TEST_CONTROL`'s presses, `NVF_PLAY_TEST_LOOK`, `NVF_PLAY_TEST_ACTIVATE`) are
//! held by a script's `DisablePlayerControls` like the keys and the mouse
//! (`control::ControlLocks`); the ones that place the player or aim the view directly
//! (`GOTO`, `FACE`, the `aim…` presses) are not.
//!
//! - `NVF_PLAY_TEST_NO_SPREAD=1` (combat.rs): the player's shots leave exactly along the
//!   view, with no spread, bloom or recoil, so a shot aimed with `aimref` or `aimpart`
//!   lands there.
//! - `NVF_PLAY_TEST_TRACERS=<seconds>` (combat.rs): every projectile of the player's
//!   draws a line, and the lines and a marker at the muzzle stay that long, so one
//!   screenshot shows where the shots were drawn from. Muzzle flashes and projectile
//!   models are not held.
//!
//! - `NVF_PLAY_TEST_PHYS_TRACE=<seconds>[:<every seconds>]` (phys/trace.rs): log every
//!   placed reference with a dynamic body for that long after its cell loads — offsets,
//!   turns, speeds and the overlaps found after the first step (physics.md §4.3.1).
//!
//! - `NVF_PLAY_TEST_PBR_MOUNT_PACK=1` (nvf-assets pbr_field/reflect.rs) and
//!   `NVF_PLAY_TEST_PBR_GROUND_PACK=1` (nvf-assets pbr_field/ground.rs, with nvforever
//!   terrain.rs `ground_pack_diffuses`): generated authored sets written into a fresh folder
//!   of the run under an explicit `NVFOREVER_HOME` and mounted over every other loose layer
//!   for that run only; the ground pack gives every landscape texture the load order names a
//!   height-only set (a ridged `_p` from a formula, depth 0.08) for the terrain parallax
//!   (#3322), and with `=alpha` a diffuse too: one flat colour a texture, a checkerboard in
//!   the alpha (#3789); `=alpha-height`: no sets, and a diffuse with the height map in its
//!   alpha (#3486).
//! - `NVF_PLAY_TEST_TONE_PRESET=1` (nvforever tone_presets.rs, #4690): the tonemapping
//!   preset "Smoke Dusk" written into the `Tonemapping` folder of an explicit
//!   `NVFOREVER_HOME` before the folder is read.
//! - `NVF_PLAY_TEST_TERRAIN_PARALLAX_AB=<seconds>[:hold][:grazing|head-on][,...]` (nvforever
//!   terrain_parallax_ab.rs): three screenshots at each time, the Terrain parallax switch
//!   off, on, off (or left alone with `hold`), optionally from its own pose on a spot of
//!   ground the march moves, and log lines for what lies under the view's centre and how the
//!   ground's texture there moved between them (`moved`, `still`, `bare`, ...).
//! - `NVF_PLAY_TEST_PICK_LOG=1` (activate.rs): each new crosshair target is logged with
//!   what it was picked from: the raw first collision hit, every render box the view ray
//!   enters (entry and exit distances, rough, bed) and the boxes set aside (#3323).
//!
//! - `NVF_PLAY_TEST_POSE_WATCH=[<seconds>:]<text>[,<text>...]` (nvforever's
//!   `swimming.rs`): from that time on, a line whenever what a watched actor plays
//!   changes (its name contains a text, any case): its base layer's and fading layer's
//!   clips and weights, its set, its idle layers, whether it has a character and whether
//!   that swims and rests on the bed (#2980). It only measures.
//!
//! With any `NVF_PLAY_TEST_*` variable set the run is unattended ([`Unattended`]): the
//! cursor is not grabbed, so the desktop's mouse neither leaves the user nor turns the
//! view (as with `--auto-walk`). Not the aids that only measure ([`MEASURING_AIDS`]:
//! `NVF_PLAY_TEST_SAMPLE`, `NVF_PLAY_TEST_PROFILE`, `NVF_PLAY_TEST_PROFILE_COUNTS`,
//! `NVF_PLAY_TEST_PROFILE_HITCH_MS`, `NVF_PLAY_TEST_PROFILE_HITCH_SPANS`,
//! `NVF_PLAY_TEST_TILE_COUNTS`, `NVF_PLAY_TEST_HITVOL_SCAN`,
//! `NVF_PLAY_TEST_HITVOL_COMPARE`, `NVF_PLAY_TEST_MOD_LOG`, `NVF_PLAY_TEST_GAME_DIGEST`,
//! `NVF_PLAY_TEST_POSE_WATCH`): with them alone the run is played by
//! hand as usual and only logs its frame times, and a load's new run keeps them
//! (nvforever handoff.rs) [owner 2026-10-27: hitch logging for a session played from MO2].
//!
//! Debug switches for a session played by hand (off unless set; development tools, not
//! part of a release; the run stays attended):
//!
//! - `NVF_PLAY_DEBUG_GIVE` and `NVF_PLAY_DEBUG_EQUIP`: as `NVF_PLAY_TEST_GIVE` and
//!   `NVF_PLAY_TEST_EQUIP`, e.g. a weapon of every animation type and its ammunition to
//!   look at the player body's clips with.

use crate::control::{Crosshair, Pov, PlayerControl};
use crate::phys::PhysState;
use crate::player::{MoveIntent, Player};
use crate::sim::SimState;
use crate::PlaySet;
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot, ScreenshotCaptured};
use fnv_sim::FormId;
use std::path::{Path, PathBuf};
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
use std::sync::Mutex;

pub mod melee_knockback;
pub(crate) mod activation_sweep;

/// How far in front of the reference `NVF_PLAY_TEST_FACE` puts the feet (game units;
/// inside the activation reach).
pub const FACE_DISTANCE: f32 = 100.0;

/// Parse `<seconds>:<rest>` items separated by commas; `rest` keeps its own colons.
pub fn timed(v: &str) -> Result<Vec<(f32, &str)>, String> {
    v.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|item| {
            let (t, rest) = item.split_once(':').ok_or_else(|| format!("{item:?}: expected <seconds>:..."))?;
            let t: f32 = t.trim().parse().map_err(|_| format!("{item:?}: bad seconds {t:?}"))?;
            Ok((t, rest.trim()))
        })
        .collect()
}

/// `<heading deg>:<pitch deg>` → radians.
pub fn look_angles(rest: &str) -> Result<(f32, f32), String> {
    let (h, p) = rest.split_once(':').ok_or_else(|| format!("{rest:?}: expected <heading>:<pitch>"))?;
    let num = |s: &str| s.trim().parse::<f32>().map_err(|_| format!("{rest:?}: bad angle {s:?}"));
    Ok((num(h)?.to_radians(), num(p)?.to_radians()))
}

/// `<x>:<y>:<z>` → a game-space point.
pub fn point(rest: &str) -> Result<[f32; 3], String> {
    let v: Vec<f32> = rest.split(':').map(|s| s.trim().parse::<f32>()).collect::<Result<_, _>>().map_err(|_| format!("{rest:?}: expected <x>:<y>:<z>"))?;
    <[f32; 3]>::try_from(v).map_err(|_| format!("{rest:?}: expected <x>:<y>:<z>"))
}

/// `NVF_PLAY_TEST_GIVE` items: `<item>[:<count>]` (count 1 by default).
fn gifts(v: &str) -> Result<Vec<(FormId, i32)>, String> {
    v.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|item| {
            let (id, n) = item.split_once(':').unwrap_or((item, "1"));
            let id = id.trim().parse::<FormId>().map_err(|e| format!("{item:?}: {e}"))?;
            let n = n.trim().parse::<i32>().map_err(|_| format!("{item:?}: bad count {n:?}"))?;
            Ok((id, n))
        })
        .collect()
}

/// `NVF_PLAY_TEST_MOVE` items: `<forward>:<right>[:walk]`.
fn move_keys(rest: &str) -> Result<MoveIntent, String> {
    let parts: Vec<&str> = rest.split(':').map(str::trim).collect();
    let num = |s: &str| s.parse::<f32>().map(|v| v.clamp(-1.0, 1.0)).map_err(|_| format!("{rest:?}: bad axis {s:?}"));
    match parts.as_slice() {
        [f, r] => Ok(MoveIntent { forward: num(f)?, right: num(r)?, ..Default::default() }),
        [f, r, "walk"] => Ok(MoveIntent { forward: num(f)?, right: num(r)?, walk: true, ..Default::default() }),
        _ => Err(format!("{rest:?}: expected <forward>:<right>[:walk]")),
    }
}

/// A control pressed by `NVF_PLAY_TEST_CONTROL`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Press {
    Sneak,
    Weapon,
    Aim,
    View,
    Jump,
    Zoom(f32),
    Orbit(f32, f32),
    Release,
    Fire,
    Reload,
    /// Lower Weapon (H): lower or raise the weapon (`PlayerControl::lowered`).
    Lower,
    Swap,
    AimActor,
    AimFlat,
    AimCorpse,
    AimNpc,
    AimNpcAt(f32),
    /// A share of the NPC's height, over a body part's centre.
    AimNpcOver(f32, u8),
    AimPart(u8),
    /// `aimcreaturepart=<n>`: the nearest creature's body part `n` (its BPTD index).
    AimCreaturePart(u8),
    AimRef(u32),
    Equip(FormId),
    Condition(f32),
    Flashlight,
    /// The nearest living actor with no VATS mode mark yet (`crate::vats_mode`).
    AimVats,
}

fn press(rest: &str) -> Result<Press, String> {
    Ok(match rest {
        "sneak" => Press::Sneak,
        "weapon" => Press::Weapon,
        "aim" => Press::Aim,
        "view" => Press::View,
        "jump" => Press::Jump,
        "release" => Press::Release,
        "fire" => Press::Fire,
        "reload" => Press::Reload,
        "lower" => Press::Lower,
        "flashlight" => Press::Flashlight,
        "swap" => Press::Swap,
        "aimactor" => Press::AimActor,
        "aimflat" => Press::AimFlat,
        "aimcorpse" => Press::AimCorpse,
        "aimnpc" => Press::AimNpc,
        "aimvats" => Press::AimVats,
        _ => {
            if let Some(Ok(id)) = rest.strip_prefix("equip=").map(|h| u32::from_str_radix(h.trim().trim_start_matches("0x"), 16)) {
                Press::Equip(FormId(id))
            } else if let Some(Ok(p)) = rest.strip_prefix("condition=").map(|p| p.trim().parse::<f32>()) {
                Press::Condition(p)
            } else if let Some((Ok(h), Ok(n))) = rest.strip_prefix("aimnpc=").and_then(|v| v.split_once('@')).map(|(h, n)| (h.trim().parse::<f32>(), n.trim().parse::<u8>())) {
                Press::AimNpcOver(h, n)
            } else if let Some(Ok(h)) = rest.strip_prefix("aimnpc=").map(|h| h.trim().parse::<f32>()) {
                Press::AimNpcAt(h)
            } else if let Some(Ok(n)) = rest.strip_prefix("aimcreaturepart=").map(|n| n.trim().parse::<u8>()) {
                Press::AimCreaturePart(n)
            } else if let Some(Ok(n)) = rest.strip_prefix("aimpart=").map(|n| n.trim().parse::<u8>()) {
                Press::AimPart(n)
            } else if let Some(Ok(id)) = rest.strip_prefix("aimref=").map(|h| u32::from_str_radix(h.trim().trim_start_matches("0x"), 16)) {
                Press::AimRef(id)
            } else if let Some(Ok(d)) = rest.strip_prefix("zoom=").map(|d| d.trim().parse::<f32>()) {
                Press::Zoom(d)
            } else if let Some(Ok((y, p))) = rest.strip_prefix("orbit=").map(look_angles) {
                Press::Orbit(y, p)
            } else {
                return Err(format!("{rest:?}: expected sneak, weapon, aim, view, jump, zoom=<units>, orbit=<yaw>:<pitch>, release, fire, reload, swap or aimactor"));
            }
        }
    })
}

/// A real input pressed by `NVF_PLAY_TEST_KEYS`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RealInput {
    Key(KeyCode),
    Mouse(MouseButton),
    /// [design] A synthetic controller's button, routed by crate::pad::menu_keys.
    Pad(GamepadButton),
    /// [design] The synthetic controller's left stick pushed all the way one way
    /// (`pad-Left Stick Down` …), read as [`crate::pad::StickButtons`].
    Stick(crate::pad::StickDir),
    /// A notch of the wheel: up is positive.
    Wheel(f32),
}

/// `<key>[:<hold seconds>]` of `NVF_PLAY_TEST_KEYS`.
pub fn real_input(rest: &str) -> Result<(RealInput, f32), String> {
    let (name, hold) = match rest.split_once(':') {
        Some((n, h)) => (n.trim(), h.trim().parse::<f32>().map_err(|_| format!("{rest:?}: bad hold {h:?}"))?),
        None => (rest.trim(), 0.0),
    };
    let lower = name.to_ascii_lowercase();
    let letters = [
        KeyCode::KeyA, KeyCode::KeyB, KeyCode::KeyC, KeyCode::KeyD, KeyCode::KeyE, KeyCode::KeyF, KeyCode::KeyG, KeyCode::KeyH, KeyCode::KeyI,
        KeyCode::KeyJ, KeyCode::KeyK, KeyCode::KeyL, KeyCode::KeyM, KeyCode::KeyN, KeyCode::KeyO, KeyCode::KeyP, KeyCode::KeyQ, KeyCode::KeyR,
        KeyCode::KeyS, KeyCode::KeyT, KeyCode::KeyU, KeyCode::KeyV, KeyCode::KeyW, KeyCode::KeyX, KeyCode::KeyY, KeyCode::KeyZ,
    ];
    let digits = [
        KeyCode::Digit0, KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9,
    ];
    let fkeys = [
        KeyCode::F1, KeyCode::F2, KeyCode::F3, KeyCode::F4, KeyCode::F5, KeyCode::F6, KeyCode::F7, KeyCode::F8, KeyCode::F9, KeyCode::F10, KeyCode::F11, KeyCode::F12,
    ];
    let input = match lower.as_str() {
        "ctrl" => RealInput::Key(KeyCode::ControlLeft),
        "shift" => RealInput::Key(KeyCode::ShiftLeft),
        "alt" => RealInput::Key(KeyCode::AltLeft),
        "space" => RealInput::Key(KeyCode::Space),
        "tab" => RealInput::Key(KeyCode::Tab),
        "esc" => RealInput::Key(KeyCode::Escape),
        "enter" => RealInput::Key(KeyCode::Enter),
        "up" => RealInput::Key(KeyCode::ArrowUp),
        "down" => RealInput::Key(KeyCode::ArrowDown),
        "left" => RealInput::Key(KeyCode::ArrowLeft),
        "right" => RealInput::Key(KeyCode::ArrowRight),
        // [design] Names for the item-menu smoke scenarios' existing keyboard controls.
        "pageup" => RealInput::Key(KeyCode::PageUp),
        "pagedown" => RealInput::Key(KeyCode::PageDown),
        "bracketleft" => RealInput::Key(KeyCode::BracketLeft),
        "bracketright" => RealInput::Key(KeyCode::BracketRight),
        "wheel-up" => RealInput::Wheel(1.0),
        "wheel-down" => RealInput::Wheel(-1.0),
        "mouse-left" => RealInput::Mouse(MouseButton::Left),
        "mouse-right" => RealInput::Mouse(MouseButton::Right),
        "mouse-middle" => RealInput::Mouse(MouseButton::Middle),
        l if l.starts_with("pad-") => match crate::pad::parse_pad(&l[4..]) {
            Some(crate::pad::PadInput::Button(b)) => RealInput::Pad(b),
            Some(crate::pad::PadInput::Stick(d)) => RealInput::Stick(d),
            None => return Err(format!("{rest:?}: unknown pad button {name:?}")),
        },
        l if l.len() == 1 && l.as_bytes()[0].is_ascii_lowercase() => RealInput::Key(letters[(l.as_bytes()[0] - b'a') as usize]),
        l if l.len() == 1 && l.as_bytes()[0].is_ascii_digit() => RealInput::Key(digits[(l.as_bytes()[0] - b'0') as usize]),
        l => match l.strip_prefix('f').and_then(|n| n.parse::<usize>().ok()).filter(|n| (1..=12).contains(n)) {
            Some(n) => RealInput::Key(fkeys[n - 1]),
            None => return Err(format!("{rest:?}: unknown key {name:?}")),
        },
    };
    Ok((input, hold))
}

/// `NVF_PLAY_TEST_KEYS`: what is still to press, and what is down until when.
#[derive(Resource, Default)]
pub(crate) struct RealPresses {
    due: Vec<(f32, (RealInput, f32))>,
    down: Vec<(RealInput, f32)>,
    pad: Option<Entity>,
}

/// Press and let go the `NVF_PLAY_TEST_KEYS` inputs, right after Bevy read the real ones.
pub(crate) fn real_presses(time: crate::pause::AidTime, mut p: ResMut<RealPresses>, mut keys: ResMut<ButtonInput<KeyCode>>, mut mouse: ResMut<ButtonInput<MouseButton>>, mut wheel: MessageWriter<bevy::input::mouse::MouseWheel>, mut pads: Query<&mut Gamepad>, mut switching: Option<ResMut<crate::pad::SwitchingInput>>) {
    let Some(now) = time.world_secs() else { return };
    let p = &mut *p;
    let mut pad = p.pad.and_then(|e| pads.get_mut(e).ok());
    if let Some(g) = pad.as_mut() {
        g.digital_mut().clear();
    }
    p.down.retain(|&(input, until)| {
        if now < until {
            return true;
        }
        match input {
            RealInput::Key(k) => {
                keys.release(k);
                if let Some(s) = switching.as_mut() {
                    s.keyboard_input(k, false);
                }
            }
            RealInput::Mouse(m) => mouse.release(m),
            RealInput::Pad(b) => {
                if let Some(g) = pad.as_mut() {
                    g.digital_mut().release(b);
                }
            }
            RealInput::Stick(d) => {
                if let Some(g) = pad.as_mut() {
                    g.analog_mut().set(stick_axis(d).0, 0.0);
                }
            }
            RealInput::Wheel(_) => {}
        }
        info!("test aid: real input {input:?} let go");
        false
    });
    for (input, hold) in take_due(&mut p.due, now) {
        match input {
            RealInput::Key(k) => {
                keys.press(k);
                if let Some(s) = switching.as_mut() {
                    s.keyboard_input(k, true);
                }
            }
            RealInput::Mouse(m) => mouse.press(m),
            RealInput::Pad(b) => {
                if let Some(g) = pad.as_mut() {
                    g.digital_mut().press(b);
                }
            }
            RealInput::Stick(d) => {
                if let Some(g) = pad.as_mut() {
                    let (axis, value) = stick_axis(d);
                    g.analog_mut().set(axis, value);
                }
            }
            RealInput::Wheel(y) => {
                wheel.write(bevy::input::mouse::MouseWheel { unit: bevy::input::mouse::MouseScrollUnit::Line, x: 0.0, y, window: Entity::PLACEHOLDER, phase: bevy::input::touch::TouchPhase::Moved });
            }
        }
        info!("test aid: real input {input:?} pressed");
        // At least one frame down.
        p.down.push((input, now + hold.max(1e-4)));
    }
}

/// The left stick's axis a push one way moves, and its value there.
fn stick_axis(d: crate::pad::StickDir) -> (GamepadAxis, f32) {
    use crate::pad::StickDir;
    match d {
        StickDir::Up => (GamepadAxis::LeftStickY, 1.0),
        StickDir::Down => (GamepadAxis::LeftStickY, -1.0),
        StickDir::Left => (GamepadAxis::LeftStickX, -1.0),
        StickDir::Right => (GamepadAxis::LeftStickX, 1.0),
    }
}

/// `NVF_PLAY_TEST_SHOTS` items: `<seconds>` (saved as `play-<seconds>s.png` in `dir`) or
/// `<seconds>:<path>`; bad entries are skipped.
fn shots(v: &str, dir: &Path) -> Vec<(f32, PathBuf)> {
    v.split(',')
        .map(str::trim)
        .filter_map(|item| match item.split_once(':') {
            Some((t, path)) => Some((t.trim().parse().ok()?, PathBuf::from(path.trim()))),
            None => {
                let t: f32 = item.parse().ok()?;
                Some((t, dir.join(format!("play-{t}s.png"))))
            }
        })
        .collect()
}

/// `"14:116424, 30:0x13BB0D"` → (time, reference); bad entries are skipped.
fn faces(v: &str) -> Vec<(f32, FormId)> {
    v.split(',')
        .filter_map(|e| {
            let (t, id) = e.trim().split_once(':')?;
            Some((t.trim().parse().ok()?, id.trim().parse().ok()?))
        })
        .collect()
}

/// The ones of `due` whose time has come (removed from it), in time order.
fn take_due<T: Clone>(due: &mut Vec<(f32, T)>, now: f32) -> Vec<T> {
    let mut fired: Vec<(f32, T)> = due.iter().filter(|(t, _)| *t <= now).cloned().collect();
    fired.sort_by(|a, b| a.0.total_cmp(&b.0));
    due.retain(|(t, _)| *t > now);
    fired.into_iter().map(|(_, v)| v).collect()
}

/// The test aids that only log what the run costs or does (nvforever stress_sample.rs and
/// profile.rs, [`crate::tiles`], [`crate::hitvol`]): they leave the run attended, and a
/// load's new run keeps them. A run that only measures and must not wait for anyone also
/// sets `NVF_PLAY_TEST_UNATTENDED`, which does nothing but drive the run: `drives_run` looks
/// at the name alone, so any value counts, and a load's new run drops it like any driving
/// aid (`profile-slow-frames`).
pub const MEASURING_AIDS: [&str; 11] = [
    "NVF_PLAY_TEST_SAMPLE",
    "NVF_PLAY_TEST_PROFILE",
    "NVF_PLAY_TEST_PROFILE_COUNTS",
    "NVF_PLAY_TEST_PROFILE_HITCH_MS",
    "NVF_PLAY_TEST_PROFILE_HITCH_SPANS",
    "NVF_PLAY_TEST_TILE_COUNTS",
    "NVF_PLAY_TEST_HITVOL_SCAN",
    "NVF_PLAY_TEST_HITVOL_COMPARE",
    // The mod log is for a mod list played by hand (`crate::mod_log`).
    crate::mod_log::ENV,
    // Logs digests of the game's state at given times (nvforever game_digest.rs).
    "NVF_PLAY_TEST_GAME_DIGEST",
    // Logs what watched actors play (nvforever swimming.rs pose watch, #2980).
    "NVF_PLAY_TEST_POSE_WATCH",
];

/// Whether this environment variable is a test aid that drives the run (any
/// `NVF_PLAY_TEST_*` but the [`MEASURING_AIDS`]).
pub fn drives_run(key: &str) -> bool {
    key.starts_with("NVF_PLAY_TEST_") && !MEASURING_AIDS.contains(&key)
}

/// Whether a test aid drives this run (some `NVF_PLAY_TEST_*` variable is set, not only
/// the [`MEASURING_AIDS`]).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Unattended(pub bool);

impl Unattended {
    pub fn from_env() -> Unattended {
        Unattended(std::env::vars_os().any(|(k, _)| k.to_str().is_some_and(drives_run)))
    }
}

/// Scheduled actions.
#[derive(Resource, Default)]
struct Aids {
    shots: Vec<(f32, PathBuf)>,
    face: Vec<(f32, FormId)>,
    looks: Vec<(f32, (f32, f32))>,
    gotos: Vec<(f32, [f32; 3])>,
    moves: Vec<(f32, MoveIntent)>,
    /// The movement keys held since the last `moves` entry fired.
    held: Option<MoveIntent>,
    presses: Vec<(f32, Press)>,
    /// Block held by the `aim` press (kept every frame: the real button's state is read
    /// each frame).
    aim_held: Option<bool>,
    /// Use held by `fire` presses (combat).
    fire_held: bool,
}

pub(crate) fn setup(app: &mut App) {
    app.insert_resource(Unattended::from_env());
    activation_sweep::setup(app);
    // The test aid and the debug switch of the same name do the same; only the test aid
    // makes the run unattended.
    for (name, what) in [("NVF_PLAY_TEST_GIVE", "test aid"), ("NVF_PLAY_DEBUG_GIVE", "debug switch")] {
        let Ok(v) = std::env::var(name) else { continue };
        match gifts(&v) {
            Ok(g) => {
                app.add_systems(Startup, move |mut sim: ResMut<SimState>| {
                    for &(item, n) in &g {
                        match sim.world.add_item(FormId::PLAYER_REF, item, n) {
                            Ok(_) => info!("{what}: gave the player {n} x {item}"),
                            Err(e) => warn!("{name} {item}: {e}"),
                        }
                    }
                });
            }
            Err(e) => warn!("{name}: {e}"),
        }
    }
    for (name, what) in [("NVF_PLAY_TEST_EQUIP", "test aid"), ("NVF_PLAY_DEBUG_EQUIP", "debug switch")] {
        let Ok(v) = std::env::var(name) else { continue };
        match gifts(&v) {
            Ok(g) => {
                app.add_systems(Startup, move |mut sim: ResMut<SimState>| {
                    for &(item, _) in &g {
                        let w = &mut sim.world;
                        let carried = w.item_count(FormId::PLAYER_REF, item).unwrap_or(0) > 0;
                        let r = if carried { Ok(Vec::new()) } else { w.add_item(FormId::PLAYER_REF, item, 1) };
                        match r.and_then(|_| w.equip(FormId::PLAYER_REF, item)) {
                            Ok(e) => info!("{what}: the player equips {item} (unequipped {:?})", e.unequipped),
                            Err(e) => warn!("{name} {item}: {e}"),
                        }
                    }
                });
            }
            Err(e) => warn!("{name}: {e}"),
        }
    }
    melee_knockback::setup(app);
    let var = |name: &str| std::env::var(name).ok();
    if let Some(v) = var("NVF_PLAY_TEST_KEYS") {
        match timed(&v).and_then(|items| items.into_iter().map(|(s, r)| real_input(r).map(|k| (s, k))).collect::<Result<Vec<_>, _>>()) {
            Ok(due) => {
                let pad = due.iter().any(|(_, (i, _))| matches!(i, RealInput::Pad(_) | RealInput::Stick(_))).then(|| app.world_mut().spawn((Gamepad::default(), Name::new("Scripted controller"))).id());
                app.insert_resource(RealPresses { due, down: Vec::new(), pad })
                    .add_systems(PreUpdate, real_presses.after(bevy::input::InputSystems).before(crate::pad::device::automatic).before(crate::pad::menu_keys).before(crate::pad::update_stick_buttons).before(bevy::ui::UiSystems::Focus));
            }
            Err(e) => warn!("NVF_PLAY_TEST_KEYS: {e}"),
        }
    }
    let mut aids = Aids::default();
    if let Some(v) = var("NVF_PLAY_TEST_SHOTS") {
        let dir = std::env::var_os("NVF_PLAY_TEST_SHOT_DIR").map_or_else(|| PathBuf::from("."), PathBuf::from);
        aids.shots = shots(&v, &dir);
    }
    if let Some(v) = var("NVF_PLAY_TEST_FACE") {
        aids.face = faces(&v);
    }
    if let Some(v) = var("NVF_PLAY_TEST_LOOK") {
        match timed(&v).and_then(|items| items.into_iter().map(|(s, r)| look_angles(r).map(|a| (s, a))).collect()) {
            Ok(l) => aids.looks = l,
            Err(e) => warn!("NVF_PLAY_TEST_LOOK: {e}"),
        }
    }
    if let Some(v) = var("NVF_PLAY_TEST_GOTO") {
        match timed(&v).and_then(|items| items.into_iter().map(|(s, r)| point(r).map(|p| (s, p))).collect()) {
            Ok(g) => aids.gotos = g,
            Err(e) => warn!("NVF_PLAY_TEST_GOTO: {e}"),
        }
    }
    if let Some(v) = var("NVF_PLAY_TEST_MOVE") {
        match timed(&v).and_then(|items| items.into_iter().map(|(s, r)| move_keys(r).map(|m| (s, m))).collect()) {
            Ok(m) => aids.moves = m,
            Err(e) => warn!("NVF_PLAY_TEST_MOVE: {e}"),
        }
    }
    if let Some(v) = var("NVF_PLAY_TEST_CONTROL") {
        match timed(&v).and_then(|items| items.into_iter().map(|(s, r)| press(r).map(|p| (s, p))).collect()) {
            Ok(p) => aids.presses = p,
            Err(e) => warn!("NVF_PLAY_TEST_CONTROL: {e}"),
        }
    }
    if aids.shots.is_empty() && aids.face.is_empty() && aids.looks.is_empty() && aids.gotos.is_empty() && aids.moves.is_empty() && aids.presses.is_empty() {
        return;
    }
    app.insert_resource(aids).add_systems(
        Update,
        (take_shots, face_reference, goto_and_look, keys_and_controls).chain().in_set(PlaySet::Input).after(crate::control::read_controls),
    );
}

/// `NVF_PLAY_TEST_MOVE` and `NVF_PLAY_TEST_CONTROL`, after the real keys were read.
#[allow(clippy::too_many_arguments)]
fn keys_and_controls(
    time: crate::pause::AidTime,
    mut aids: ResMut<Aids>,
    mut intent: ResMut<MoveIntent>,
    mut control: ResMut<PlayerControl>,
    phys: Res<PhysState>,
    mut player: ResMut<Player>,
    play: Res<crate::PlayWorld>,
    mut sim: ResMut<SimState>,
    mut flashlight: ResMut<crate::flashlight::Flashlight>,
    (vats, script): (Res<crate::vats_mode::VatsMode>, Option<Res<crate::script::ScriptState>>),
) {
    let Some(now) = time.world_secs() else { return };
    for m in take_due(&mut aids.moves, now) {
        info!("test aid: movement keys forward {} right {}{}", m.forward, m.right, if m.walk { " walking" } else { "" });
        aids.held = Some(m);
    }
    if let Some(m) = aids.held {
        intent.analog_scale = m.analog_scale;
        (intent.forward, intent.right, intent.walk) = (m.forward, m.right, m.walk);
    }
    let presses = take_due(&mut aids.presses, now);
    // The `aim…` presses put the point under the crosshair: the third-person camera's
    // when it is out (`control::Crosshair`), the eye's otherwise.
    let cross = control.crosshair;
    for p in presses {
        match p {
            Press::Sneak => control.sneak = !control.sneak,
            Press::Weapon => control.weapon_out = !control.weapon_out,
            Press::Aim => aids.aim_held = Some(!aids.aim_held.unwrap_or(false)),
            Press::View => control.pov = if control.pov == Pov::First { Pov::Third } else { Pov::First },
            Press::Jump => intent.jump = true,
            Press::Zoom(d) => control.distance = d.clamp(crate::control::MIN_DISTANCE, crate::control::MAX_DISTANCE),
            Press::Orbit(yaw, pitch) => {
                control.orbit = crate::control::Orbit { yaw, pitch, held: true, moved: true, free: false };
            }
            Press::Release => control.orbit.held = false,
            Press::Fire => aids.fire_held = !aids.fire_held,
            Press::Reload => control.reload = true,
            Press::Lower => control.lowered = !control.lowered,
            Press::Swap => control.ammo_swap = true,
            Press::Flashlight => flashlight.toggle(),
            Press::AimActor => aim_at_nearest_actor(&phys, &mut player, cross.as_ref(), |_| true, None),
            Press::AimFlat => {
                aim_at_nearest_actor(&phys, &mut player, cross.as_ref(), |_| true, None);
                player.pitch = 0.0;
            }
            Press::AimCorpse => aim_at_nearest_corpse(&phys, &mut player, cross.as_ref()),
            Press::AimNpc => aim_at_nearest_actor(&phys, &mut player, cross.as_ref(), |id| play.index.refs.get(&id).is_some_and(|e| &e.sig == b"ACHR"), None),
            Press::AimNpcAt(h) => aim_at_nearest_actor(&phys, &mut player, cross.as_ref(), |id| play.index.refs.get(&id).is_some_and(|e| &e.sig == b"ACHR"), Some(Aim::Share(h))),
            Press::AimNpcOver(h, n) => aim_at_nearest_actor(&phys, &mut player, cross.as_ref(), |id| play.index.refs.get(&id).is_some_and(|e| &e.sig == b"ACHR"), Some(Aim::ShareOver(h, n))),
            Press::AimPart(n) => aim_at_nearest_actor(&phys, &mut player, cross.as_ref(), |id| play.index.refs.get(&id).is_some_and(|e| &e.sig == b"ACHR"), Some(Aim::Part(n))),
            Press::AimCreaturePart(n) => aim_at_nearest_actor(&phys, &mut player, cross.as_ref(), |id| play.index.refs.get(&id).is_some_and(|e| &e.sig == b"ACRE"), Some(Aim::Part(n))),
            Press::AimRef(id) => aim_at_ref(&phys, &mut player, cross.as_ref(), id),
            Press::AimVats => {
                let marked = vats.marked();
                let alive = |id: u32| !crate::combat::is_dead(script.as_deref(), id) && !marked.contains(&id);
                aim_at_nearest_actor(&phys, &mut player, cross.as_ref(), alive, None)
            }
            Press::Condition(percent) => {
                let c = (percent / 100.0).clamp(0.0, 1.0);
                if let Ok(inv) = sim.world.inventory_mut(FormId::PLAYER_REF) {
                    for item in inv.equipped() {
                        inv.map_equipped(item, |e| fnv_sim::inventory::Extra { condition: Some(c), ..e });
                    }
                }
            }
            Press::Equip(item) => {
                let w = &mut sim.world;
                let carried = w.item_count(FormId::PLAYER_REF, item).unwrap_or(0) > 0;
                let r = if carried { Ok(Vec::new()) } else { w.add_item(FormId::PLAYER_REF, item, 1) };
                match r.and_then(|_| w.equip(FormId::PLAYER_REF, item)) {
                    Ok(_) => info!("test aid: the player equips {item}"),
                    Err(e) => warn!("test aid: equip {item}: {e}"),
                }
            }
        }
        info!(
            "test aid: control {p:?}: sneak {} weapon out {} aiming {:?} view {:?} distance {:.0}",
            control.sneak, control.weapon_out, aids.aim_held, control.pov, control.distance
        );
    }
    if let Some(a) = aids.aim_held {
        control.aiming = a;
    }
    if aids.fire_held {
        control.trigger = control.weapon_out;
    }
}

/// Turn the view to the middle of the nearest actor hit volume that `keep` accepts
/// (combat test aid).
/// Where on an actor a test aid aims.
#[derive(Debug, Clone, Copy)]
enum Aim {
    /// A share of its height.
    Share(f32),
    /// A share of its height, over a body part's centre.
    ShareOver(f32, u8),
    /// A body part (its index in the actor's BPTD).
    Part(u8),
}

/// Aim at the nearest kept actor: its aim point (`PhysWorld::actor_aim_point`), a share of
/// its height or one of its body parts.
fn aim_at_nearest_actor(phys: &PhysState, player: &mut Player, cross: Option<&Crosshair>, keep: impl Fn(u32) -> bool, aim: Option<Aim>) {
    let eye = player.eye();
    let d2 = |c: [f32; 3]| (c[0] - eye[0]).powi(2) + (c[1] - eye[1]).powi(2) + (c[2] - eye[2]).powi(2);
    let point = |id: u32| match aim {
        Some(Aim::Share(s)) => phys.world.actor_volume(id).map(|v| [v.feet[0], v.feet[1], v.feet[2] + v.height * s]),
        Some(Aim::ShareOver(s, n)) => phys.world.actor_volume(id).zip(phys.world.actor_part_center(id, n)).map(|(v, c)| [c[0], c[1], v.feet[2] + v.height * s]),
        Some(Aim::Part(n)) => phys.world.actor_part_center(id, n),
        None => phys.world.actor_aim_point(id),
    };
    let nearest = phys.world.actor_volumes().filter(|&id| keep(id)).filter_map(|id| point(id).map(|p| (id, p))).min_by(|a, b| d2(a.1).total_cmp(&d2(b.1)));
    let Some((id, c)) = nearest else {
        warn!("test aid: aimactor: no actor hit volume");
        return;
    };
    (player.heading, player.pitch) = Crosshair::aim_at(cross, eye, c);
    let line = cross.map_or(String::new(), |x| format!(", the third-person crosshair's line {:.0} right and {:.0} up of the pivot", x.shoulder[0], x.shoulder[1]));
    info!("test aid: aiming at actor {id:08X} ({:.0} units away{line})", d2(c).sqrt());
}

/// Aim at an actor's aim point (`PhysWorld::actor_aim_point`), else at the collision body
/// of a placed object (its first body's centre of mass).
fn aim_at_ref(phys: &PhysState, player: &mut Player, cross: Option<&Crosshair>, id: u32) {
    let Some(c) = phys.world.actor_aim_point(id).or_else(|| phys.world.ref_center(id)) else {
        warn!("test aid: aimref: {id:08X} has no hit volume or collision body");
        return;
    };
    let eye = player.eye();
    let (dx, dy, dz) = (c[0] - eye[0], c[1] - eye[1], c[2] - eye[2]);
    (player.heading, player.pitch) = Crosshair::aim_at(cross, eye, c);
    info!("test aid: aiming at {id:08X} ({:.0} units away)", (dx * dx + dy * dy + dz * dz).sqrt());
}

/// Aim at the trunk of the nearest ragdoll (a dead actor's body in the physics world).
fn aim_at_nearest_corpse(phys: &PhysState, player: &mut Player, cross: Option<&Crosshair>) {
    let eye = player.eye();
    let d2 = |c: [f32; 3]| (c[0] - eye[0]).powi(2) + (c[1] - eye[1]).powi(2) + (c[2] - eye[2]).powi(2);
    let nearest = phys.world.ragdolls().filter_map(|id| phys.world.ragdoll_center(id).map(|p| (id, p))).min_by(|a, b| d2(a.1).total_cmp(&d2(b.1)));
    let Some((id, c)) = nearest else {
        warn!("test aid: aimcorpse: no ragdoll");
        return;
    };
    (player.heading, player.pitch) = Crosshair::aim_at(cross, eye, c);
    info!("test aid: aiming at the corpse of {id:08X} ({:.0} units away, at ({:.0}, {:.0}, {:.0}))", d2(c).sqrt(), c[0], c[1], c[2]);
}

fn take_shots(time: crate::pause::AidTime, mut aids: ResMut<Aids>, mut commands: Commands) {
    let Some(now) = time.world_secs() else { return };
    for path in take_due(&mut aids.shots, now) {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            let _ = std::fs::create_dir_all(dir);
        }
        info!("test aid: screenshot {}", path.display());
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut shot = commands.spawn(Screenshot::primary_window());
        shot.observe(save_to_disk(path));
        #[cfg(any(test, debug_assertions, feature = "dev-aids"))]
        if std::env::var_os("NVF_PLAY_TEST_SHOT_DIFF").is_some() {
            let name = name.clone();
            shot.observe(move |ev: On<ScreenshotCaptured>| {
                let size = ev.image.texture_descriptor.size;
                let Some(data) = ev.image.data.as_deref() else { return };
                let shot: Shot = ((size.width, size.height), data.to_vec());
                let mut last = LAST_SHOTS.lock().expect("the last shots");
                for ((was, pixels), which) in last.iter().rev().zip(["the shot before", "two shots before"]) {
                    if *was == shot.0 {
                        let (levels, changed) = picture_difference(pixels, data, was.0 as usize, was.1 as usize);
                        info!("test aid: {name}: {levels:.2} levels of 255 from {which} (tolerance {:.2}), {changed:.1} % of the view changed", difference_tolerance(levels));
                    }
                }
                if last.len() == 2 {
                    last.remove(0);
                }
                last.push(shot);
            });
        }
        if std::env::var_os("NVF_PLAY_TEST_SHOT_FLATNESS").is_some() {
            shot.observe(move |ev: On<ScreenshotCaptured>| {
                let size = ev.image.texture_descriptor.size;
                if let Some(data) = ev.image.data.as_deref() {
                    info!("test aid: {name}: {} % of the view is one colour", one_colour_percent(data, size.width as usize, size.height as usize));
                }
            });
        }
    }
}

/// A shot's size and pixels.
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
type Shot = ((u32, u32), Vec<u8>);

/// The last two shots, the older first (`NVF_PLAY_TEST_SHOT_DIFF`; pictures of the window, kept
/// while the aid is on).
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
static LAST_SHOTS: Mutex<Vec<Shot>> = Mutex::new(Vec::new());

/// How far a picture may be from one it should equal, where two of the same kind as far apart in
/// time differ by `levels` [engineering choice: half as much again, and a tenth of a level for a
/// view that stood still].
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
fn difference_tolerance(levels: f64) -> f64 {
    1.5 * levels + 0.1
}

/// Test aid `NVF_PLAY_TEST_SHOT_DIFF`: the mean absolute difference of two pictures of one size
/// over the view, its lower fifth left out (the HUD), in levels of 255 over the three colour
/// channels, and the share of its pixels (percent) with a channel that moved by 4 levels or more.
/// Four bytes a pixel, row by row.
#[cfg(any(test, debug_assertions, feature = "dev-aids"))]
fn picture_difference(a: &[u8], b: &[u8], width: usize, height: usize) -> (f64, f64) {
    let (mut sum, mut changed, mut seen) = (0u64, 0u64, 0u64);
    for (p, q) in a.as_chunks::<4>().0.iter().zip(b.as_chunks::<4>().0).take(width * (height * 4 / 5)) {
        let d = [0, 1, 2].map(|i| p[i].abs_diff(q[i]));
        sum += d.iter().map(|&d| u64::from(d)).sum::<u64>();
        changed += u64::from(d.iter().any(|&d| d >= 4));
        seen += 1;
    }
    let seen = seen.max(1) as f64;
    (sum as f64 / (3.0 * seen), 100.0 * changed as f64 / seen)
}

/// Test aid `NVF_PLAY_TEST_SHOT_FLATNESS`: the share of the view, its lower fifth left out
/// (the HUD), taken by its commonest colour at 32 levels a channel. A view the world has not
/// been drawn into is one colour (#1520). `pixels`: four bytes each, row by row.
fn one_colour_percent(pixels: &[u8], width: usize, height: usize) -> u32 {
    let mut counts = std::collections::HashMap::<[u8; 3], u32>::new();
    let mut seen = 0u32;
    for y in (0..height * 4 / 5).step_by(4) {
        for x in (0..width).step_by(4) {
            if let Some(p) = pixels.get((y * width + x) * 4..(y * width + x) * 4 + 3) {
                *counts.entry([p[0] >> 3, p[1] >> 3, p[2] >> 3]).or_default() += 1;
                seen += 1;
            }
        }
    }
    counts.values().max().map_or(0, |n| n * 100 / seen.max(1))
}

fn face_reference(time: crate::pause::AidTime, mut aids: ResMut<Aids>, sim: Res<SimState>, mut phys: ResMut<PhysState>, mut player: ResMut<Player>) {
    for id in take_due(&mut aids.face, time.elapsed_secs()) {
        let Some(at) = sim.world.peek(id).map(|s| s.pos).or_else(|| sim.refs.get(id).map(|p| p.pos)) else {
            warn!("test aid: face {id}: unknown reference");
            continue;
        };
        let feet = phys.player.as_ref().map_or(player.feet, |c| c.feet);
        let arrival = sim.refs.get(id).and_then(|p| p.teleport).and_then(|tp| sim.refs.get(tp.door)?.teleport).filter(|t| t.door == id);
        let from = arrival.map_or(feet, |t| t.pos);
        let (dx, dy) = (at[0] - from[0], at[1] - from[1]);
        let len = dx.hypot(dy).max(1e-3);
        let stand = [at[0] - dx / len * FACE_DISTANCE, at[1] - dy / len * FACE_DISTANCE, at[2]];
        let a = phys.arrive_player(stand);
        player.feet = a.feet;
        player.heading = dx.atan2(dy);
        player.pitch = 0.0;
        info!(
            "test aid: face {id} at ({:.0}, {:.0}, {:.0}): feet ({:.0}, {:.0}, {:.1}), heading {:.1}",
            at[0],
            at[1],
            at[2],
            a.feet[0],
            a.feet[1],
            a.feet[2],
            player.heading.to_degrees().rem_euclid(360.0)
        );
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn goto_and_look(
    time: crate::pause::AidTime,
    mut aids: ResMut<Aids>,
    mut phys: ResMut<PhysState>,
    mut player: ResMut<Player>,
    locks: Res<crate::control::ControlLocks>,
    mut stats: ResMut<crate::control::LockStats>,
    (driven, kill_cam): (Option<Res<crate::player::ViewDriven>>, Option<Res<crate::kill_cam::KillCam>>),
    (state, zoom): (Res<State<crate::PlayState>>, Option<Res<crate::dialogue_zoom::DialogueZoom>>),
) {
    let now = time.elapsed_secs();
    if phys.player.is_some() {
        for at in take_due(&mut aids.gotos, now) {
            let hit = phys.place_player(at, 60.0, 1000.0);
            let feet = phys.player.as_ref().map_or(at, |c| c.feet);
            player.feet = feet;
            info!(
                "test aid: go to ({:.0}, {:.0}, {:.0}): feet ({:.1}, {:.1}, {:.1}) on {:?}",
                at[0],
                at[1],
                at[2],
                feet[0],
                feet[1],
                feet[2],
                hit.map(|h| h.owner)
            );
        }
    }
    // The looks are world aids (`crate::pause::WorldAidClock`, they go with the
    // screenshots); the go-tos set the stage before the scripts' first frame and keep the
    // aids' clock, as the faces do.
    let Some(now) = time.world_secs() else { return };
    for (h, p) in take_due(&mut aids.looks, now) {
        // The aid stands in for the mouse, which a zoomed conversation's lock holds
        // (`player::mouse_look`; owner decision 2026-11-13, #2254).
        if *state.get() == crate::PlayState::Dialogue && zoom.as_deref().is_some_and(crate::dialogue_look::view_locked) {
            info!("test aid: look heading {:.1} pitch {:.1} ignored: dialogue view locked", h.to_degrees(), p.to_degrees());
            continue;
        }
        // The aid stands in for the mouse, which a script's lock holds (control.rs).
        if locks.looking {
            info!("test aid: look heading {:.1} pitch {:.1} ignored: a script has disabled looking", h.to_degrees(), p.to_degrees());
            stats.held[crate::control::group::LOOKING] += 1;
            continue;
        }
        // As the mouse, held while an animation drives the view (`ViewDriven`).
        if driven.as_deref().is_some_and(|d| d.0) {
            info!("test aid: look heading {:.1} pitch {:.1} ignored: an animation drives the view", h.to_degrees(), p.to_degrees());
            continue;
        }
        // As the mouse, held while a kill camera plays (`crate::kill_cam`).
        if kill_cam.as_deref().is_some_and(|k| k.active()) {
            info!("test aid: look heading {:.1} pitch {:.1} ignored: the kill camera has the view", h.to_degrees(), p.to_degrees());
            continue;
        }
        info!("test aid: look heading {:.1} pitch {:.1}", h.to_degrees(), p.to_degrees());
        (player.heading, player.pitch) = (h, p);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_flat_view_is_one_colour_and_a_varied_one_is_not() {
        let (w, h) = (64, 40);
        let flat = [92u8, 116, 148, 255].repeat(w * h);
        assert_eq!(one_colour_percent(&flat, w, h), 100);
        let varied: Vec<u8> = (0..w * h).flat_map(|i| [(i % 251) as u8, (i % 127) as u8 * 2, (i % 61) as u8 * 4, 255]).collect();
        assert!(one_colour_percent(&varied, w, h) < 10);
        // The lower fifth is the HUD's: not counted.
        let mut hud = flat.clone();
        hud[(h * 4 / 5) * w * 4..].fill(0);
        assert_eq!(one_colour_percent(&hud, w, h), 100);
    }

    use super::*;

    #[test]
    fn the_key_aid_names_arrows_and_wheel_notches() {
        assert_eq!(real_input("down"), Ok((RealInput::Key(KeyCode::ArrowDown), 0.0)));
        for (name, key) in [("pageup", KeyCode::PageUp), ("pagedown", KeyCode::PageDown), ("bracketleft", KeyCode::BracketLeft), ("bracketright", KeyCode::BracketRight)] {
            assert_eq!(real_input(name), Ok((RealInput::Key(key), 0.0)));
        }
        assert_eq!(real_input("left:0.5"), Ok((RealInput::Key(KeyCode::ArrowLeft), 0.5)));
        assert_eq!(real_input("wheel-down"), Ok((RealInput::Wheel(-1.0), 0.0)));
        assert_eq!(real_input("wheel-up"), Ok((RealInput::Wheel(1.0), 0.0)));
        assert_eq!(real_input("e"), Ok((RealInput::Key(KeyCode::KeyE), 0.0)));
        assert_eq!(real_input("pad-X:0.2"), Ok((RealInput::Pad(GamepadButton::West), 0.2)));
        assert_eq!(real_input("pad-A"), Ok((RealInput::Pad(GamepadButton::South), 0.0)));
        assert!(real_input("pad-unknown").is_err());
    }

    /// The stick aid pushes the synthetic controller's left stick and lets it go.
    #[test]
    fn scripted_stick_pushes_and_lets_go() {
        use bevy::ecs::system::RunSystemOnce;
        assert_eq!(real_input("pad-Left Stick Down"), Ok((RealInput::Stick(crate::pad::StickDir::Down), 0.0)));
        let mut world = World::new();
        world.insert_resource(Time::<()>::default());
        world.init_resource::<crate::pause::PlayClock>();
        world.init_resource::<ButtonInput<KeyCode>>();
        world.init_resource::<ButtonInput<MouseButton>>();
        world.init_resource::<Messages<bevy::input::mouse::MouseWheel>>();
        let pad = world.spawn(Gamepad::default()).id();
        world.insert_resource(RealPresses { due: vec![(0.0, (RealInput::Stick(crate::pad::StickDir::Down), 0.0))], down: Vec::new(), pad: Some(pad) });
        world.run_system_once(real_presses).unwrap();
        assert_eq!(world.get::<Gamepad>(pad).unwrap().left_stick(), Vec2::new(0.0, -1.0));
        world.resource_mut::<Time>().advance_by(std::time::Duration::from_millis(1));
        world.run_system_once(real_presses).unwrap();
        assert_eq!(world.get::<Gamepad>(pad).unwrap().left_stick(), Vec2::ZERO);
    }

    #[test]
    fn scripted_keyboard_switching_press_is_consumed_and_works_after_release() {
        #[derive(Resource, Default)]
        struct Seen([bool; 3]);
        fn read(keys: crate::pad::ActionButtons<KeyCode>, mut seen: ResMut<Seen>) {
            seen.0 = [keys.pressed(KeyCode::Enter), keys.just_pressed(KeyCode::Enter), keys.just_released(KeyCode::Enter)];
        }
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin));
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(250)));
        app.init_resource::<crate::pause::PlayClock>().init_resource::<crate::pad::SwitchingInput>();
        let mut bindings = crate::bindings::Bindings::default();
        bindings.device = crate::bindings::InputDevice::Controller;
        app.insert_resource(bindings);
        app.insert_resource(RealPresses {
            due: vec![(0.0, (RealInput::Key(KeyCode::Enter), 0.5)), (1.0, (RealInput::Key(KeyCode::Enter), 0.5))],
            down: Vec::new(), pad: None,
        });
        app.init_resource::<Seen>().add_systems(Update, read);
        app.add_systems(PreUpdate, (crate::pad::device::read_keyboard, real_presses, crate::pad::device::automatic).chain().after(bevy::input::InputSystems));
        app.update();
        assert_eq!(app.world().resource::<crate::bindings::Bindings>().device, crate::bindings::InputDevice::KeyboardMouse);
        assert_eq!(app.world().resource::<Seen>().0, [false; 3]);
        app.update();
        assert_eq!(app.world().resource::<Seen>().0, [true, false, false]);
        app.update();
        assert_eq!(app.world().resource::<Seen>().0, [false, false, true]);
        app.update();
        app.update();
        assert!(app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::Enter));
    }

    #[test]
    fn scripted_pad_presses_hold_release_and_press_again() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.insert_resource(Time::<()>::default());
        world.init_resource::<crate::pause::PlayClock>();
        world.init_resource::<ButtonInput<KeyCode>>();
        world.init_resource::<ButtonInput<MouseButton>>();
        world.init_resource::<Messages<bevy::input::mouse::MouseWheel>>();
        let pad = world.spawn(Gamepad::default()).id();
        world.insert_resource(RealPresses {
            due: vec![(0.0, (RealInput::Pad(GamepadButton::West), 0.5)), (1.0, (RealInput::Pad(GamepadButton::West), 0.0))],
            down: Vec::new(), pad: Some(pad),
        });
        world.run_system_once(real_presses).unwrap();
        assert!(world.get::<Gamepad>(pad).unwrap().just_pressed(GamepadButton::West));
        world.resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.25));
        world.run_system_once(real_presses).unwrap();
        let g = world.get::<Gamepad>(pad).unwrap();
        assert!(g.pressed(GamepadButton::West));
        assert!(!g.just_pressed(GamepadButton::West));
        world.resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.25));
        world.run_system_once(real_presses).unwrap();
        assert!(world.get::<Gamepad>(pad).unwrap().just_released(GamepadButton::West));
        world.resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f32(0.5));
        world.run_system_once(real_presses).unwrap();
        assert!(world.get::<Gamepad>(pad).unwrap().just_pressed(GamepadButton::West));
        world.resource_mut::<Time>().advance_by(std::time::Duration::from_millis(1));
        world.run_system_once(real_presses).unwrap();
        assert!(!world.get::<Gamepad>(pad).unwrap().pressed(GamepadButton::West));
        assert!(world.resource::<RealPresses>().down.is_empty());
    }

    #[test]
    fn parses_and_fires_once() {
        let dir = Path::new("out");
        assert_eq!(
            shots("6, 12.5,x, 7:C:/a/b.png", dir),
            vec![(6.0, dir.join("play-6s.png")), (12.5, dir.join("play-12.5s.png")), (7.0, PathBuf::from("C:/a/b.png"))]
        );
        assert_eq!(faces("14:116424, 30:0x13BB0D, bad"), vec![(14.0, FormId(0x116424)), (30.0, FormId(0x13BB0D))]);
        let mut due = vec![(2.0, 'b'), (1.0, 'a')];
        assert_eq!(take_due(&mut due, 1.5), vec!['a']);
        assert_eq!(take_due(&mut due, 1.6), Vec::<char>::new());
        assert_eq!(take_due(&mut due, 5.0), vec!['b']);
        assert!(due.is_empty());
        let mut due = vec![(2.0, 'b'), (1.0, 'a')];
        assert_eq!(take_due(&mut due, 5.0), vec!['a', 'b'], "in time order");
    }

    #[test]
    fn measuring_aids_leave_the_run_attended() {
        assert!(!drives_run("NVF_PLAY_TEST_SAMPLE"));
        assert!(!drives_run("NVF_PLAY_TEST_PROFILE"));
        assert!(!drives_run("NVF_PLAY_TEST_PROFILE_HITCH_MS"));
        assert!(!drives_run("NVF_PLAY_TEST_PROFILE_HITCH_SPANS"));
        assert!(!drives_run("NVF_PLAY_TEST_TILE_COUNTS"));
        // The mod log's run is the owner's, by hand: with the aid driving it the cursor
        // was not grabbed and the mouse did not turn the view (the first run of 2026-11-12).
        assert!(!drives_run(crate::mod_log::ENV));
        assert!(drives_run("NVF_PLAY_TEST_UNATTENDED"));
        assert!(drives_run("NVF_PLAY_TEST_GOD"));
        assert!(drives_run("NVF_PLAY_TEST_SAMPLE_X"), "only the exact names measure");
        assert!(!drives_run("NVF_PLAY_DEBUG_GIVE"));
    }

    #[test]
    fn timed_items() {
        assert_eq!(timed("3:a.png, 7.5:b:c").unwrap(), vec![(3.0, "a.png"), (7.5, "b:c")]);
        assert_eq!(timed("").unwrap(), vec![]);
        assert!(timed("x:a").is_err());
        assert!(timed("3").is_err());
        let (h, p) = look_angles("90:-45").unwrap();
        assert!((h - std::f32::consts::FRAC_PI_2).abs() < 1e-6 && (p + std::f32::consts::FRAC_PI_4).abs() < 1e-6);
        assert!(look_angles("90").is_err());
        assert_eq!(point("1:-2.5:3").unwrap(), [1.0, -2.5, 3.0]);
        assert_eq!(gifts("00015169:2, 0x0000000F").unwrap(), vec![(FormId(0x15169), 2), (FormId(0xF), 1)]);
        assert!(gifts("00015169:x").is_err());
        assert!(point("1:2").is_err());
        assert_eq!(move_keys("1:-0.5").unwrap(), MoveIntent { forward: 1.0, right: -0.5, ..Default::default() });
        assert!(move_keys("-1:0:walk").unwrap().walk);
        assert!(move_keys("1").is_err() && move_keys("1:0:run").is_err());
        assert_eq!(press("zoom=250").unwrap(), Press::Zoom(250.0));
        assert_eq!(press("weapon").unwrap(), Press::Weapon);
        assert_eq!(press("fire").unwrap(), Press::Fire);
        assert_eq!(press("aimactor").unwrap(), Press::AimActor);
        assert_eq!(press("aimflat").unwrap(), Press::AimFlat);
        assert_eq!(press("aimnpc=0.9").unwrap(), Press::AimNpcAt(0.9));
        assert_eq!(press("aimnpc=0.95@0").unwrap(), Press::AimNpcOver(0.95, 0));
        assert_eq!(press("aimref=0010A202").unwrap(), Press::AimRef(0x10A202));
        assert_eq!(press("aimpart=4").unwrap(), Press::AimPart(4));
        assert_eq!(press("aimcreaturepart=1").unwrap(), Press::AimCreaturePart(1));
        assert_eq!(press("condition=2.5").unwrap(), Press::Condition(2.5));
        assert!(press("shoot").is_err());
        assert_eq!(press("orbit=180:0").unwrap(), Press::Orbit(std::f32::consts::PI, 0.0));
        assert_eq!(press("release").unwrap(), Press::Release);
    }
}
