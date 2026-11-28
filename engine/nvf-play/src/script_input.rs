//! The extender's input functions' side in the engine (task D6;
//! docs/spec-notes/ext-input.md): each frame, right after Bevy reads the devices, the
//! keys and mouse buttons held and the controls' bindings go to the script runtime
//! (`fnv_game::script_input`), the keys scripts disabled go to [`ScriptBlocked`] (which
//! `bindings::ControlInput` leaves out, so the player's controls ignore them), and the
//! keys scripts tapped are pressed for one frame.

use crate::bindings::{dik_key, Binding, Bindings, Control, ControlInput, GAME_CONTROLS, GAME_INI_CONTROLS};
use bevy::prelude::*;
use bevy::input::mouse::MouseWheel;
use bevy::window::PrimaryWindow;
use fnv_sim::FormId;
use fnv_game::script_input::MOUSE_BASE;

pub struct ScriptInputPlugin;

/// Physical script input observes acknowledged cursor state before callbacks.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ScriptInputSet;

impl Plugin for ScriptInputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScriptBlocked>()
            .add_message::<MouseWheel>()
            .configure_sets(PreUpdate, ScriptInputSet.after(bevy::input::InputSystems))
            // [design, #2200] Scripts see consumed switching presses as absent too.
            .add_systems(PreUpdate, script_input.in_set(ScriptInputSet).after(crate::pad::device::automatic).after(crate::pad::update_stick_buttons))
            .add_systems(PreUpdate, publish_virtual_controls.run_if(resource_exists::<Bindings>).after(script_input).after(crate::pad::PlatformInputSystems::Remember).before(bevy::ui::UiSystems::Focus))
            // The player's state the scripts read (sights up, for the HUD bars' aiming
            // condition) is handed over after combat has updated it this frame and before
            // the scripts run [design]. `tell_magazine` keeps its own place: it also
            // applies the scripts' pending ammo writes, which must reach combat first.
            .add_systems(Update, tell_player_state.after(crate::PlaySet::Activation).before(crate::PlaySet::Scripts))
            .add_systems(Update, (tell_magazine, mod_file_roots))
            .add_systems(Update, script_moves.after(crate::PlaySet::Input));
    }
}

/// The keys and mouse buttons scripts disabled (DisableKey, DisableControl).
#[derive(Resource, Default)]
pub struct ScriptBlocked {
    pub keys: Vec<KeyCode>,
    pub mouse: Vec<MouseButton>,
}

/// The actual non-key movement view used by IsControlPressed, without inventing
/// physical keys. Both the publisher and App regression use this central admission.
pub fn admitted_virtual_controls(input: &ControlInput) -> std::collections::BTreeSet<u8> {
    GAME_CONTROLS[..GAME_INI_CONTROLS].iter().enumerate().filter_map(|(i,(c,_,_))|
        (matches!(c,Control::Forward|Control::Back|Control::SlideLeft|Control::SlideRight) && input.touch_movement_pressed(*c)).then_some(i as u8)).collect()
}
fn publish_virtual_controls(input: ControlInput, mut script: Option<ResMut<crate::script::ScriptState>>) {
    if let Some(rt)=script.as_deref_mut().and_then(|s|s.rt.as_mut()) {
        rt.game.input.virtual_controls=admitted_virtual_controls(&input);
    }
}

/// The extender's code of a mouse button: 256 left, 257 right, 258 middle, then the extra
/// buttons [documented: Template:DirectX Scancodes].
fn mouse_code(b: MouseButton) -> Option<u16> {
    Some(MOUSE_BASE
        + match b {
            MouseButton::Left => 0,
            MouseButton::Right => 1,
            MouseButton::Middle => 2,
            MouseButton::Back => 3,
            MouseButton::Forward => 4,
            MouseButton::Other(n) if n < 8 => n,
            MouseButton::Other(_) => return None,
        })
}

fn mouse_of(code: u16) -> Option<MouseButton> {
    [MouseButton::Left, MouseButton::Right, MouseButton::Middle, MouseButton::Back, MouseButton::Forward, MouseButton::Other(5), MouseButton::Other(6), MouseButton::Other(7)]
        .into_iter()
        .find(|&b| mouse_code(b) == Some(code))
}

fn key_code(k: KeyCode) -> Option<u16> {
    (0u8..=255).find(|&c| dik_key(c) == Some(k)).map(u16::from)
}

/// Let go of a key or mouse button the scripts pressed.
fn release(c: u16, keys: &mut ButtonInput<KeyCode>, mouse: &mut ButtonInput<MouseButton>) {
    match c.checked_sub(MOUSE_BASE) {
        Some(_) => mouse_of(c).into_iter().for_each(|b| mouse.release(b)),
        None => dik_key(c as u8).into_iter().for_each(|k| keys.release(k)),
    }
}

#[allow(clippy::too_many_arguments)]
fn script_input(
    mut script: Option<ResMut<crate::script::ScriptState>>,
    mut bindings: Option<ResMut<Bindings>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<MouseWheel>,
    primary: Query<Entity, With<PrimaryWindow>>,
    mut blocked: ResMut<ScriptBlocked>,
    switching: Option<Res<crate::pad::SwitchingInput>>,
    pads: crate::pad::ActionPads,
    sticks: Option<Res<crate::pad::StickButtons>>,
    mut tapped: Local<Vec<u16>>,
    mut script_held: Local<Vec<u16>>,
) {
    // Last frame's taps end.
    for c in tapped.drain(..) {
        release(c, &mut keys, &mut mouse);
    }
    // Wheel directions are physical pulses, not MouseButton::Other aliases. Drain
    // current messages even without a runtime/window so they cannot replay later.
    let window = primary.single().ok();
    let mut wheel_codes = [false; 2];
    for event in wheel.read() {
        if Some(event.window) != window || !event.y.is_finite() { continue; }
        if event.y > 0.0 { wheel_codes[0] = true; }
        else if event.y < 0.0 { wheel_codes[1] = true; }
    }
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    let input = &mut rt.game.input;
    // Keys scripts held last frame and no longer hold (ReleaseKey, ReleaseControl) are let go.
    for c in script_held.drain(..).filter(|c| !input.script_held.contains(c)) {
        release(c, &mut keys, &mut mouse);
    }
    // SetControl's changes reach the player's controls first.
    if let Some(b) = bindings.as_deref_mut() {
        for (c, mouse, code) in std::mem::take(&mut input.rebinds) {
            let Some(&(control, _, _)) = GAME_CONTROLS.get(c) else { continue };
            let old = b.get(control);
            let new = if mouse {
                Binding { mouse: code.and_then(mouse_of), ..old }
            } else {
                Binding { key: code.and_then(|k| u8::try_from(k).ok()).and_then(dik_key), ..old }
            };
            b.set(control, new);
        }
    }
    let action_keys = switching.as_ref().map_or_else(|| std::borrow::Cow::Borrowed(&*keys), |s| s.filter(&keys));
    let action_mouse = switching.as_ref().map_or_else(|| std::borrow::Cow::Borrowed(&*mouse), |s| s.filter(&mouse));
    input.held = action_keys.get_pressed().filter_map(|&k| key_code(k)).chain(action_mouse.get_pressed().filter_map(|&b| mouse_code(b))).collect();
    drop((action_keys, action_mouse));
    // 264/265 are the documented up/down wheel codes. Rebuilding held each frame
    // releases a direction immediately when that frame has no matching message.
    input.held.extend(wheel_codes.into_iter().zip([MOUSE_BASE + 8, MOUSE_BASE + 9])
        .filter_map(|(pressed, code)| pressed.then_some(code)));
    if let Some(b) = bindings.as_deref() {
        // The engine's controls are the extender's 0–27, in the same order.
        input.bindings = GAME_CONTROLS[..GAME_INI_CONTROLS].iter().map(|(c, _, _)| {
            let bind = b.get(*c);
            (bind.key.and_then(key_code), bind.mouse.and_then(mouse_code))
        }).collect();
    }
    // Before taps/holds below alter engine keys: use the published input snapshot,
    // selected device and the same controller binding reader as ControlInput.
    // DisableControl blocks gameplay, not these documented event transitions.
    input.event_controls = bindings.as_deref().map(|b| {
        let controller = b.device == crate::bindings::InputDevice::Controller;
        let keys = !controller || crate::bindings::keys_with_pad();
        GAME_CONTROLS.iter().take(fnv_game::script_input::CONTROLS).enumerate().fold(0, |mask, (c, (control, _, _))| {
            let keyboard = keys && input.bindings.get(c).is_some_and(|(k, m)| [*k, *m].into_iter().flatten().any(|code| input.held.contains(&code)));
            let pad = controller && crate::bindings::pad_binding_held(b.pad(*control), &pads, sticks.as_deref());
            mask | (u32::from(keyboard || pad) << c)
        })
    });
    let codes = input.blocked();
    blocked.keys = codes.iter().filter(|&&c| c < MOUSE_BASE).filter_map(|&c| dik_key(c as u8)).collect();
    blocked.mouse = codes.iter().filter_map(|&c| mouse_of(c)).collect();
    for c in std::mem::take(&mut input.taps).into_iter().chain(input.script_held.iter().copied()) {
        match c.checked_sub(MOUSE_BASE) {
            Some(_) => mouse_of(c).into_iter().for_each(|b| mouse.press(b)),
            None => dik_key(c as u8).into_iter().for_each(|k| keys.press(k)),
        }
        if !input.script_held.contains(&c) {
            tapped.push(c);
        }
    }
    script_held.extend(input.script_held.iter().copied());
    sync_pad_state(&mut rt.game, &pads);
}

/// The rounds in the player's magazine for GetPlayerCurrentAmmoRounds.
fn tell_magazine(mut combat: Option<ResMut<crate::combat::Combat>>, mut script: Option<ResMut<crate::script::ScriptState>>) {
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    // SetPlayerCurrentAmmoRounds: the rounds go in the weapon in hand.
    if let (Some(n), Some(c)) = (rt.game.other.set_magazine.take(), combat.as_deref_mut()) {
        if let Some((w, _)) = c.magazine_for_save() {
            c.restore_magazine((w, n), None);
        }
    }
    rt.game.player_magazine = combat.and_then(|c| c.magazine_for_save()).map(|m| m.1);
}

/// The player's speed resources script_moves reads back for GetRunSpeed.
type PlayerSpeeds<'w> = (Option<Res<'w, crate::player::MoveSpeeds>>, Option<Res<'w, crate::player::StatSpeed>>, Option<Res<'w, crate::player::PerkSpeed>>);

/// What scripts ask of the player's and actors' moves (`fnv_game::nvse_jg9`: Jump,
/// SetAutoMove, SetPlayerMovementFlags, SetVelEx), after the controls are read so this
/// frame's moves carry them; and what those functions read back: the player's run speed,
/// spread, grenade hold and view.
#[allow(clippy::too_many_arguments)]
fn script_moves(
    mut script: Option<ResMut<crate::script::ScriptState>>,
    mut control: Option<ResMut<crate::control::PlayerControl>>,
    mut intent: Option<ResMut<crate::player::MoveIntent>>,
    mut phys: Option<ResMut<crate::phys::PhysState>>,
    mut knocks: Option<ResMut<Messages<crate::combat::Knockback>>>,
    combat: Option<Res<crate::combat::Combat>>,
    speeds: PlayerSpeeds,
) {
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    let st = &mut rt.game.other.jg9;
    for who in std::mem::take(&mut st.jumps) {
        if who == FormId::PLAYER_REF {
            if let Some(i) = intent.as_deref_mut() {
                i.jump = true;
            }
        } else if let Some(b) = phys.as_deref_mut().and_then(|p| p.npcs.get_mut(&who.0)) {
            b.jump = true;
        }
        info!("script moves: {who} jumps");
    }
    if let (Some(on), Some(c)) = (st.auto_move.take(), control.as_deref_mut()) {
        c.auto_move = on;
        info!("script moves: auto move {}", if on { "on" } else { "off" });
    }
    // The flags as the keys: forward and back, left and right, walking, running and
    // sneaking, jumping (SetPlayerMovementFlags' page lists them).
    if let (Some(f), Some(i)) = (st.move_flags.take(), intent.as_deref_mut()) {
        i.analog_scale = 1.0; // An explicit script movement request owns its native pace.
        let axis = |plus: u32, minus: u32| f32::from(u8::from(f & plus != 0)) - f32::from(u8::from(f & minus != 0));
        i.forward = axis(1, 2);
        i.right = axis(8, 4);
        if f & (256 | 512) != 0 {
            i.walk = f & 256 != 0 && f & 512 == 0;
        }
        i.sneak = f & 1024 != 0;
        i.jump |= f & 4096 != 0;
    }
    for (who, v) in std::mem::take(&mut st.velocities) {
        let line = set_vel_ex(who, v, phys.as_deref_mut(), knocks.as_deref_mut());
        info!("script moves: {line}");
    }
    let (moves, stat, perk) = speeds;
    if let Some(m) = moves {
        let factor = stat.map_or(1.0, |s| s.0) * perk.map_or(1.0, |p| p.0);
        st.run_speeds.insert(FormId::PLAYER_REF, m.run * factor);
    }
    if let Some(c) = combat {
        if let Some(s) = c.player_spread {
            st.spreads.insert(FormId::PLAYER_REF, s);
        }
        st.grenade_hold = c.grenade_hold;
    }
    st.third_person = control.as_deref().is_some_and(|c| c.pov != crate::control::Pov::First);
}

/// SetVelEx's velocity on `who`: a character goes up or down at z and is pushed along
/// the ground at (x, y) as the console's SetVel pushes it (`crate::console::set_vel`, one
/// push along the two axes together); a loose object's bodies take all three.
fn set_vel_ex(who: FormId, v: [f32; 3], phys: Option<&mut crate::phys::PhysState>, knocks: Option<&mut Messages<crate::combat::Knockback>>) -> String {
    let Some(p) = phys else { return "SetVelEx: no physics in this run".into() };
    let id = who.0;
    let ch = if who == FormId::PLAYER_REF { p.player.as_mut() } else { p.npcs.get_mut(&id).map(|n| &mut n.ch) };
    let Some(ch) = ch else {
        let n: usize = (0..3).map(|axis| p.world.set_ref_velocity(id, axis, v[axis])).max().unwrap_or(0);
        return format!("SetVelEx: {who} at ({:.0}, {:.0}, {:.0}) units/s ({n} bodies)", v[0], v[1], v[2]);
    };
    if v[2] != 0.0 {
        ch.vertical_speed = v[2];
        ch.grounded = false;
    }
    let along = v[0].hypot(v[1]);
    if let (true, Some(k)) = (along > 0.0, knocks) {
        let dir = [v[0] / along, v[1] / along];
        k.write(crate::combat::Knockback { form_id: id, dir, speed: along, secs: crate::console::SET_VEL_SECS, attacker: 0, blow: 0, power: false });
    }
    format!("SetVelEx: {who} {:.0} units/s up, pushed {along:.0} units/s along the ground", v[2])
}

/// The pad's buttons as the extender's controller functions number them
/// (`fnv_game::nvse_jip2::Pad`).
const PAD_BITS: [(GamepadButton, u32); 14] = [
    (GamepadButton::DPadUp, 0x1),
    (GamepadButton::DPadDown, 0x2),
    (GamepadButton::DPadLeft, 0x4),
    (GamepadButton::DPadRight, 0x8),
    (GamepadButton::Start, 0x10),
    (GamepadButton::Select, 0x20),
    (GamepadButton::LeftThumb, 0x40),
    (GamepadButton::RightThumb, 0x80),
    (GamepadButton::LeftTrigger, 0x100),
    (GamepadButton::RightTrigger, 0x200),
    (GamepadButton::South, 0x1000),
    (GamepadButton::East, 0x2000),
    (GamepadButton::West, 0x4000),
    (GamepadButton::North, 0x8000),
];

/// Whether a physics character is airborne (`!grounded && !swimming`, matching
/// `crate::pipboy::data::travel` and `crate::combat`) [design].
fn character_in_air(c: &fnv_phys::Character) -> bool {
    !c.grounded && !c.swimming
}

/// Sync `Motion::in_air` for the player and every loaded physics NPC before scripts run,
/// preserving other `Motion` fields and clearing stale `in_air` flags when `PhysState` or
/// an actor's character is absent [design].
fn sync_motion_in_air(game: &mut fnv_game::game::Game, phys: Option<&crate::phys::PhysState>) {
    for (&id, motion) in &mut game.senses.motion {
        let ch = phys.and_then(|p| {
            if id == FormId::PLAYER_REF {
                p.player.as_ref()
            } else {
                p.npcs.get(&id.0).map(|n| &n.ch)
            }
        });
        motion.in_air = ch.is_some_and(character_in_air);
    }
    let Some(p) = phys else { return };
    if p.player.as_ref().is_some_and(character_in_air) && !game.motion(FormId::PLAYER_REF).in_air {
        let mut motion = game.motion(FormId::PLAYER_REF);
        motion.in_air = true;
        game.set_motion(FormId::PLAYER_REF, motion);
    }
    for (&id, npc) in &p.npcs {
        if character_in_air(&npc.ch) {
            let fid = FormId(id);
            if !game.motion(fid).in_air {
                let mut motion = game.motion(fid);
                motion.in_air = true;
                game.set_motion(fid, motion);
            }
        }
    }
}

/// Read one trigger's live analog value (`0.0..=1.0`) from an action-filtered pad,
/// falling back to `1.0` when a digital-only controller reports `pressed` with a zero/absent
/// analog reading [design: preserves `#2200` switching suppression because `ActionPad::get`
/// returns `Some(0.0)` and `ActionPad::pressed` returns `false` on consumed switching presses].
fn pad_trigger_value(pad: &crate::pad::device::ActionPad<'_>, button: GamepadButton) -> f32 {
    let analog = pad.get(button).filter(|v| v.is_finite()).map_or(0.0, |v| v.clamp(0.0, 1.0));
    if analog == 0.0 && pad.pressed(button) {
        1.0
    } else {
        analog
    }
}

/// Publish live gamepad state across all connected action-filtered pads so controller
/// queries, `OnButtonDown`/`OnButtonUp`, and `SetOnTriggerDownEventHandler`/
/// `SetOnTriggerUpEventHandler` (via `Game::jip_keys` in `Game::tick` / `Game::page_tick`)
/// see current-frame input regardless of controller enumeration order [design].
fn sync_pad_state(game: &mut fnv_game::game::Game, pads: &crate::pad::ActionPads) {
    let mut connected = false;
    let mut buttons = 0u32;
    let mut triggers = [0.0f32; 2];
    let mut left_stick = Vec2::ZERO;
    let mut right_stick = Vec2::ZERO;
    for g in pads.iter() {
        connected = true;
        for &(b, mask) in &PAD_BITS {
            if g.pressed(b) {
                buttons |= mask;
            }
        }
        triggers[0] = triggers[0].max(pad_trigger_value(&g, GamepadButton::LeftTrigger2));
        triggers[1] = triggers[1].max(pad_trigger_value(&g, GamepadButton::RightTrigger2));
        let (l, r) = (g.left_stick(), g.right_stick());
        if l.length_squared() > left_stick.length_squared() {
            left_stick = l;
        }
        if r.length_squared() > right_stick.length_squared() {
            right_stick = r;
        }
    }
    let pad = &mut game.jip2.told.pad;
    pad.connected = connected;
    pad.buttons = buttons;
    pad.sticks = [left_stick.x, left_stick.y, right_stick.x, right_stick.y];
    game.publish_pad_triggers(triggers[0], triggers[1]);
}

/// Tell the scripts the player's auto-move, aiming, kill-camera, actors' airborne state
/// (player and physics NPCs), and connected pads (GetAutoMove,
/// GetPCUsingScope/IronSights, IsInKillCam, IsInAir, the controller functions), and
/// whether the sights are up (`Combat::player_sights_up`: not while blocking with a melee
/// weapon or while a reload holds them down), for SUP's HUD bars' aiming condition.
fn tell_player_state(
    control: Option<Res<crate::control::PlayerControl>>,
    combat: Option<Res<crate::combat::Combat>>,
    anim: Option<Res<crate::combat::WeaponAnim>>,
    kill_cam: Option<Res<crate::kill_cam::KillCam>>,
    phys: Option<Res<crate::phys::PhysState>>,
    pads: crate::pad::ActionPads,
    mut script: Option<ResMut<crate::script::ScriptState>>,
) {
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    sync_motion_in_air(&mut rt.game, phys.as_deref());
    let told = &mut rt.game.jip2.told;
    if let Some(c) = control {
        told.auto_move = c.auto_move;
        told.aiming = c.aiming && c.weapon_out;
        told.sights_up = match (combat.as_deref(), anim.as_deref()) {
            (Some(cb), Some(a)) => cb.player_sights_up(a, told.aiming),
            _ => false,
        };
    }
    told.kill_cam = kill_cam.is_some_and(|k| k.kill_playing());
    sync_pad_state(&mut rt.game, &pads);
}

/// Marker inserted once [`init_mod_file_roots`] has populated `fnv_game::mod_files` for
/// this `App` (either early during `PlayPlugin::build` or on the first `Update`).
#[derive(Resource)]
pub(crate) struct ModFileRootsReady;

/// Populate `fnv_game::mod_files` read roots, archive list, and VFS hooks from `data`:
/// NVForever's mods layer first, then any mounted loose layers highest-precedence first,
/// with the game's `Data` folder last (so `mod_files::data_dir` remains `Data`).
pub(crate) fn init_mod_file_roots(d: &nvf_assets::GameData) {
    let vfs = d.vfs.clone();
    let reader_vfs = d.vfs.clone();
    let mods = fnv_install::EngineHome::default_root().map(|r| fnv_install::EngineHome::new(r).mods());
    let data_dir = vfs.data_dir().to_path_buf();
    let mut read_roots: Vec<std::path::PathBuf> = Vec::new();
    // The VFS's loose roots (a Mod Organizer 2 merge's mod folders) come after NVForever's
    // layer; the file questions ask the VFS's index for them (`Roots::hosted_from`).
    let hosted_from = usize::from(mods.is_some());
    if let Some(m) = mods {
        read_roots.push(m);
    }
    for root in vfs.loose_roots().iter().rev() {
        if !read_roots.iter().any(|existing| existing == root) {
            read_roots.push(root.clone());
        }
    }
    if read_roots.last() != Some(&data_dir) {
        read_roots.retain(|existing| existing != &data_dir);
        read_roots.push(data_dir);
    }
    let archives: Vec<String> = vfs.archives().map(str::to_string).collect();
    // With a Mod Organizer 2 profile merged over Data (`--mo2`) the mods' loose files are in
    // the VFS's index, not under Data on disk: the file readers ask it (`reader`, `lookup`) and
    // the folder listings list it (`list`), so a mod's own `Config` files and folders are found
    // as the merge has them [observed: Nuclear Sunset's MAPMO reads its colours from
    // `Config\Main And Pause Menus Overhaul - Presets.ini` in its mod folder; read as missing,
    // the HUD and Pip-Boy were coloured 0 0 0, black]. (Walking the 1,665 mod folders on disk
    // instead was tried: every lookup then walked them, and the start never finished.)
    let list_vfs = d.vfs.clone();
    fnv_game::mod_files::update_roots(move |r| {
        r.list = Some(std::sync::Arc::new(move |prefix: &str| {
            let pre = prefix.to_ascii_lowercase();
            list_vfs.loose_paths_under(&pre)
        }));
        r.read = read_roots;
        r.hosted_from = Some(hosted_from);
        r.archives = archives;
        r.reader = Some(std::sync::Arc::new(move |p: &str| reader_vfs.read(p).and_then(Result::ok)));
        r.lookup = Some(std::sync::Arc::new(move |p: &str| {
            if vfs.loose_path(p).is_some() {
                1
            } else if vfs.exists(p) {
                2
            } else {
                0
            }
        }));
    });
}

/// Once the game's Data is open: mods' files are read from NVForever's mods layer, then
/// the game's Data, and the archives answer FileExists (fnv_game::mod_files). Also reloads
/// `kNVSE.ini` (`[Anim Fixes] sLegacyAnimTimePaths`) because `Game::new` can run before
/// VFS roots are populated here.
fn mod_file_roots(
    data: Option<Res<nvf_assets::GameData>>,
    ready: Option<Res<ModFileRootsReady>>,
    mut script: Option<ResMut<crate::script::ScriptState>>,
    mut done: Local<bool>,
) {
    if ready.is_some() || *done {
        return;
    }
    let Some(d) = data else { return };
    *done = true;
    init_mod_file_roots(&d);
    if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
        rt.game.reload_knvse_ini();
    }
}

#[cfg(test)]
#[path = "script_input_wheel_tests.rs"]
mod wheel_tests;

#[cfg(test)]
#[path = "script_input_mcm_page_tests.rs"]
mod mcm_page_tests;

#[cfg(test)]
mod kill_cam_state_tests {
    use super::*;
    use crate::kill_cam::KillCam;
    use crate::script::ScriptState;
    use fnv_game::game::RunCtx;
    use fnv_script::vm::XVal;

    fn is_in_kill_cam(app: &mut App) -> XVal {
        let mut state = app.world_mut().resource_mut::<ScriptState>();
        let game = &mut state.rt.as_mut().unwrap().game;
        fnv_game::nvse::call(game, &RunCtx::default(), "isinkillcam", None, &[])
            .expect("IsInKillCam handler")
            .expect("IsInKillCam result")
    }

    #[test]
    fn tell_player_state_tracks_kill_cam_and_excludes_classic_attack_cam() {
        let mut app = App::new();
        app.insert_resource(ScriptState::over_no_plugins())
            .init_resource::<KillCam>()
            .add_systems(Update, tell_player_state);

        // Inactive by default.
        app.update();
        assert_eq!(is_in_kill_cam(&mut app), XVal::Num(0.0));

        // Active kill camera reports 1.
        app.insert_resource(KillCam::test_with_kill_show());
        app.update();
        assert_eq!(is_in_kill_cam(&mut app), XVal::Num(1.0));

        // Classic VATS attack camera (non-kill) reports 0.
        app.insert_resource(KillCam::test_with_attack_show());
        app.update();
        assert_eq!(is_in_kill_cam(&mut app), XVal::Num(0.0));

        // Back to active kill camera, then removed resource resets to 0 (no stale true).
        app.insert_resource(KillCam::test_with_kill_show());
        app.update();
        assert_eq!(is_in_kill_cam(&mut app), XVal::Num(1.0));

        app.world_mut().remove_resource::<KillCam>();
        app.update();
        assert_eq!(is_in_kill_cam(&mut app), XVal::Num(0.0));
    }

    fn is_in_air(app: &mut App, subject: Option<FormId>) -> XVal {
        let mut state = app.world_mut().resource_mut::<ScriptState>();
        let game = &mut state.rt.as_mut().unwrap().game;
        fnv_game::nvse::call(game, &RunCtx::default(), "isinair", subject, &[])
            .expect("IsInAir handler")
            .expect("IsInAir result")
    }

    #[test]
    fn tell_player_state_tracks_player_and_npc_in_air_from_physics() {
        let mut app = App::new();
        app.insert_resource(ScriptState::over_no_plugins())
            .add_systems(Update, tell_player_state);

        let npc_id = 0x1234u32;
        let npc_ref = FormId(npc_id);

        // Preserve existing Motion fields (e.g. sneaking) while updating in_air.
        {
            let mut state = app.world_mut().resource_mut::<ScriptState>();
            let game = &mut state.rt.as_mut().unwrap().game;
            game.set_motion(
                FormId::PLAYER_REF,
                fnv_game::functions::senses::Motion { sneaking: true, ..Default::default() },
            );
        }

        // Absent PhysState reports 0 for player and NPC.
        app.update();
        assert_eq!(is_in_air(&mut app, None), XVal::Num(0.0));
        assert_eq!(is_in_air(&mut app, Some(npc_ref)), XVal::Num(0.0));

        // Grounded player and grounded NPC report 0.
        let mut phys = crate::phys::PhysState::default();
        let mut ch = phys.world.add_character(FormId::PLAYER_REF.0, fnv_phys::CharacterParams::default(), [0.0, 0.0, 0.0]);
        ch.grounded = true;
        ch.swimming = false;
        phys.player = Some(ch);
        let mut npc_ch = phys.world.add_character(npc_id, fnv_phys::CharacterParams::default(), [64.0, 0.0, 0.0]);
        npc_ch.grounded = true;
        npc_ch.swimming = false;
        phys.npcs.insert(
            npc_id,
            crate::phys::NpcBody {
                standing_height: npc_ch.params.height,
                ch: npc_ch,
                prev_feet: [64.0, 0.0, 0.0],
                velocity: [0.0, 0.0],
                jump: false,
                sink: 0.0,
                lift: crate::phys::step_lift::StepLift::default(),
                sneak_crouch: false,
            },
        );
        app.insert_resource(phys);
        app.update();
        assert_eq!(is_in_air(&mut app, None), XVal::Num(0.0));
        assert_eq!(is_in_air(&mut app, Some(npc_ref)), XVal::Num(0.0));

        // Player airborne (not grounded, not swimming) reports 1 and preserves sneaking; NPC stays 0.
        app.world_mut().resource_mut::<crate::phys::PhysState>().player.as_mut().unwrap().grounded = false;
        app.update();
        assert_eq!(is_in_air(&mut app, None), XVal::Num(1.0));
        assert_eq!(is_in_air(&mut app, Some(npc_ref)), XVal::Num(0.0));
        assert!(app.world().resource::<ScriptState>().rt.as_ref().unwrap().game.motion(FormId::PLAYER_REF).sneaking);

        // Player swimming (not grounded, swimming) reports 0; NPC airborne reports 1.
        {
            let mut p = app.world_mut().resource_mut::<crate::phys::PhysState>();
            p.player.as_mut().unwrap().swimming = true;
            p.npcs.get_mut(&npc_id).unwrap().ch.grounded = false;
        }
        app.update();
        assert_eq!(is_in_air(&mut app, None), XVal::Num(0.0));
        assert_eq!(is_in_air(&mut app, Some(npc_ref)), XVal::Num(1.0));

        // Unloading the airborne NPC clears its in_air flag immediately (no stale true).
        app.world_mut().resource_mut::<crate::phys::PhysState>().npcs.remove(&npc_id);
        app.update();
        assert_eq!(is_in_air(&mut app, Some(npc_ref)), XVal::Num(0.0));

        // Player back airborne, then removing PhysState resets to 0 (no stale true).
        app.world_mut().resource_mut::<crate::phys::PhysState>().player.as_mut().unwrap().swimming = false;
        app.update();
        assert_eq!(is_in_air(&mut app, None), XVal::Num(1.0));

        app.world_mut().remove_resource::<crate::phys::PhysState>();
        app.update();
        assert_eq!(is_in_air(&mut app, None), XVal::Num(0.0));
    }

    #[test]
    fn mod_file_roots_reloads_knvse_ini_into_existing_script_runtime() {
        let mut previous = None;
        fnv_game::mod_files::update_roots(|r| previous = Some(std::mem::take(r)));
        struct Guard {
            root: std::path::PathBuf,
            previous: Option<fnv_game::mod_files::Roots>,
        }
        impl Drop for Guard {
            fn drop(&mut self) {
                if let Some(prev) = self.previous.take() {
                    fnv_game::mod_files::set_roots(prev);
                }
                let _ = std::fs::remove_dir_all(&self.root);
            }
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("nvf-mod-file-roots-knvse-ini-{}-{stamp}", std::process::id()));
        let plugins = root.join("NVSE").join("Plugins");
        std::fs::create_dir_all(&plugins).unwrap();
        let _guard = Guard { root: root.clone(), previous };

        std::fs::write(
            plugins.join("kNVSE.ini"),
            b"[Anim Fixes]\nsLegacyAnimTimePaths = B42Inject,B42Interact,B42Loot\n",
        )
        .unwrap();

        // Game::new runs BEFORE mod_file_roots populates VFS roots: defaults hold initially.
        let mut app = App::new();
        app.insert_resource(ScriptState::over_no_plugins())
            .add_systems(Update, mod_file_roots);
        {
            let game = &app.world().resource::<ScriptState>().rt.as_ref().unwrap().game;
            assert!(game.knvse_ini.legacy_anim_time_paths.is_empty());
        }

        // Once GameData opens, mod_file_roots updates roots and reloads kNVSE.ini immediately.
        let data = nvf_assets::GameData::new(
            std::sync::Arc::new(fnv_vfs::Vfs::open(&root, &[]).unwrap()),
            std::sync::Arc::new(fnv_nif::Schema::default()),
        );
        app.insert_resource(data);
        app.update();

        let game = &app.world().resource::<ScriptState>().rt.as_ref().unwrap().game;
        assert_eq!(game.knvse_ini.legacy_anim_time_paths, vec!["b42inject", "b42interact", "b42loot"]);
    }
}

#[cfg(test)]
mod pad_trigger_event_tests {
    use super::*;
    use crate::bindings::{Bindings, InputDevice};
    use crate::script::ScriptState;
    use crate::tiles::mods::{self, ModsPage};
    use fnv_game::game::{menus, RunCtx};
    use fnv_script::vm::XVal;

    fn tick_game(mut state: ResMut<ScriptState>) {
        state.rt.as_mut().unwrap().game.tick();
    }

    fn setup_app(with_switching: bool) -> App {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<Bindings>()
            .init_resource::<ScriptBlocked>()
            .init_resource::<crate::pad::StickButtons>()
            .add_message::<MouseWheel>()
            .insert_resource(ScriptState::over_no_plugins());
        if with_switching {
            app.init_resource::<crate::pad::SwitchingInput>()
                .add_systems(PreUpdate, (crate::pad::device::automatic, script_input).chain());
        } else {
            app.add_systems(PreUpdate, script_input);
        }
        app.add_systems(Update, (tell_player_state, tick_game).chain());
        {
            let mut state = app.world_mut().resource_mut::<ScriptState>();
            let game = &mut state.rt.as_mut().unwrap().game;
            let files = fnv_ui::MemSource::default().with(
                "triggers.xml",
                r#"<menu name="HUDMainMenu"><rect name="Trig"><_ltDown>0</_ltDown><_rtDown>0</_rtDown><_ltUp>0</_ltUp><_rtUp>0</_rtUp><_pageRt>0</_pageRt></rect></menu>"#,
            );
            game.ui_tree.load_menu(&files, "triggers.xml").unwrap();
            game.ui_tree.update();
            let setup = "Begin Function {}\n\
                SetOnTriggerDownEventHandler (Begin Function {int iTrig}\n\
                  SetUIFloat \"HUDMainMenu/Trig/_ltDown\" ((GetUIFloat \"HUDMainMenu/Trig/_ltDown\") + 1)\n\
                End) 1 0\n\
                SetOnTriggerDownEventHandler (Begin Function {int iTrig}\n\
                  SetUIFloat \"HUDMainMenu/Trig/_rtDown\" ((GetUIFloat \"HUDMainMenu/Trig/_rtDown\") + 1)\n\
                  if MenuMode 1013\n\
                    SetUIFloat \"HUDMainMenu/Trig/_pageRt\" ((GetUIFloat \"HUDMainMenu/Trig/_pageRt\") + 1)\n\
                  endif\n\
                End) 1 1\n\
                SetOnTriggerUpEventHandler (Begin Function {int iTrig}\n\
                  SetUIFloat \"HUDMainMenu/Trig/_ltUp\" ((GetUIFloat \"HUDMainMenu/Trig/_ltUp\") + 1)\n\
                End) 1 0\n\
                SetOnTriggerUpEventHandler (Begin Function {int iTrig}\n\
                  SetUIFloat \"HUDMainMenu/Trig/_rtUp\" ((GetUIFloat \"HUDMainMenu/Trig/_rtUp\") + 1)\n\
                End) 1 1\n\
                End\n";
            game.run_query(setup, RunCtx::default()).unwrap();
        }
        app
    }

    fn count(app: &App, name: &str) -> f64 {
        app.world()
            .resource::<ScriptState>()
            .rt
            .as_ref()
            .unwrap()
            .game
            .ui_tree
            .get_float(&format!("HUDMainMenu/Trig/{name}"))
            .unwrap()
    }

    fn trigger_getter(app: &mut App, name: &str) -> f64 {
        let mut state = app.world_mut().resource_mut::<ScriptState>();
        let game = &mut state.rt.as_mut().unwrap().game;
        match fnv_game::nvse::call(game, &RunCtx::default(), name, None, &[]).unwrap().unwrap() {
            XVal::Num(n) => n,
            v => panic!("{name}: {v:?}"),
        }
    }

    #[test]
    fn gamepad_triggers_fire_jip_handlers_across_analog_digital_multipad_and_disconnect() {
        let mut app = setup_app(false);
        let pad_idle = app.world_mut().spawn(Gamepad::default()).id();
        let pad_active = app.world_mut().spawn(Gamepad::default()).id();

        // Analog noise below 30/255 does not fire.
        app.world_mut().get_mut::<Gamepad>(pad_active).unwrap().analog_mut().set(GamepadButton::LeftTrigger2, 0.05);
        app.update();
        assert_eq!((count(&app, "_ltDown"), count(&app, "_rtDown")), (0.0, 0.0));

        // Analog LT above 30/255 (even below Bevy's 0.75 digital threshold) on second pad fires LT down once.
        app.world_mut().get_mut::<Gamepad>(pad_active).unwrap().analog_mut().set(GamepadButton::LeftTrigger2, 0.25);
        app.update();
        app.update();
        assert_eq!((count(&app, "_ltDown"), count(&app, "_rtDown")), (1.0, 0.0));
        assert!((trigger_getter(&mut app, "getlefttrigger") - 0.25).abs() < 1e-4);

        // Non-finite analog value on first pad is sanitized and does not poison active pad or fire RT.
        app.world_mut().get_mut::<Gamepad>(pad_idle).unwrap().analog_mut().set(GamepadButton::RightTrigger2, f32::NAN);
        app.update();
        assert_eq!((count(&app, "_ltDown"), count(&app, "_rtDown"), count(&app, "_ltUp")), (1.0, 0.0, 0.0));
        assert_eq!(trigger_getter(&mut app, "getrighttrigger"), 0.0);

        // Release LT -> fires LT up once.
        app.world_mut().get_mut::<Gamepad>(pad_active).unwrap().analog_mut().set(GamepadButton::LeftTrigger2, 0.0);
        app.update();
        assert_eq!((count(&app, "_ltUp"), count(&app, "_rtUp")), (1.0, 0.0));

        // Digital-only press on a pad with no analog reading fires LT down, then release fires LT up.
        let pad_digital = app.world_mut().spawn(Gamepad::default()).id();
        app.world_mut().get_mut::<Gamepad>(pad_digital).unwrap().digital_mut().press(GamepadButton::LeftTrigger2);
        app.update();
        assert_eq!(count(&app, "_ltDown"), 2.0);
        assert_eq!(trigger_getter(&mut app, "getlefttrigger"), 1.0);
        app.world_mut().get_mut::<Gamepad>(pad_digital).unwrap().digital_mut().release(GamepadButton::LeftTrigger2);
        app.update();
        assert_eq!(count(&app, "_ltUp"), 2.0);

        // Press RT on active pad, then despawn (disconnect) all pads -> fires RT down then RT up cleanly.
        app.world_mut().get_mut::<Gamepad>(pad_active).unwrap().analog_mut().set(GamepadButton::RightTrigger2, 0.9);
        app.update();
        assert_eq!((count(&app, "_rtDown"), count(&app, "_rtUp")), (1.0, 0.0));
        app.world_mut().despawn(pad_idle);
        app.world_mut().despawn(pad_active);
        app.world_mut().despawn(pad_digital);
        app.update();
        assert_eq!((count(&app, "_rtDown"), count(&app, "_rtUp")), (1.0, 1.0));
    }

    #[test]
    fn switching_frame_suppresses_trigger_edge_until_next_frame_and_mods_page_receives_triggers() {
        let mut app = setup_app(true);
        app.world_mut().resource_mut::<Bindings>().device = InputDevice::KeyboardMouse;
        app.world_mut().resource_mut::<Bindings>().automatic_input_switching = true;
        let pad = app.world_mut().spawn(Gamepad::default()).id();
        app.update();

        // Pull LT past switching threshold: device switches to Controller, switching frame is suppressed, next frame fires LT down.
        app.world_mut().get_mut::<Gamepad>(pad).unwrap().analog_mut().set(GamepadButton::LeftTrigger2, 0.8);
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller);
        assert_eq!(count(&app, "_ltDown"), 0.0, "consumed switching press must not fire on switching frame");
        app.update();
        assert_eq!(count(&app, "_ltDown"), 1.0, "held trigger fires once controller is active");

        // Under ModsPage (`page_tick(menus::START)`), pulling RT fires inside MenuMode 1013 and does not replay on tick().
        let mut page_app = App::new();
        page_app
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<ScriptBlocked>()
            .init_resource::<ModsPage>()
            .add_message::<MouseWheel>()
            .insert_resource(ScriptState::over_no_plugins())
            .add_systems(PreUpdate, script_input)
            .add_systems(Update, (tell_player_state, |mut s: ResMut<ScriptState>, mut p: ResMut<ModsPage>| mods::tick(&mut s.rt.as_mut().unwrap().game, &mut p)).chain());
        {
            let mut state = page_app.world_mut().resource_mut::<ScriptState>();
            let game = &mut state.rt.as_mut().unwrap().game;
            let files = fnv_ui::MemSource::default().with(
                "triggers.xml",
                r#"<menu name="HUDMainMenu"><rect name="Trig"><_ltDown>0</_ltDown><_rtDown>0</_rtDown><_ltUp>0</_ltUp><_rtUp>0</_rtUp><_pageRt>0</_pageRt></rect></menu>"#,
            );
            game.ui_tree.load_menu(&files, "triggers.xml").unwrap();
            game.run_query(
                "Begin Function {}\n\
                 SetOnTriggerDownEventHandler (Begin Function {int iTrig}\n\
                   SetUIFloat \"HUDMainMenu/Trig/_rtDown\" ((GetUIFloat \"HUDMainMenu/Trig/_rtDown\") + 1)\n\
                   if MenuMode 1013\n\
                     SetUIFloat \"HUDMainMenu/Trig/_pageRt\" ((GetUIFloat \"HUDMainMenu/Trig/_pageRt\") + 1)\n\
                   endif\n\
                 End) 1 1\n\
                 End\n",
                RunCtx::default(),
            )
            .unwrap();
        }
        let page_pad = page_app.world_mut().spawn(Gamepad::default()).id();
        page_app.world_mut().get_mut::<Gamepad>(page_pad).unwrap().analog_mut().set(GamepadButton::RightTrigger2, 0.85);
        page_app.update();
        assert_eq!((count(&page_app, "_rtDown"), count(&page_app, "_pageRt")), (1.0, 1.0));
        // Subsequent regular tick must not replay the already-consumed RT press.
        page_app.world_mut().resource_mut::<ScriptState>().rt.as_mut().unwrap().game.tick();
        assert_eq!((count(&page_app, "_rtDown"), count(&page_app, "_pageRt")), (1.0, 1.0));
        assert!(!page_app.world().resource::<ScriptState>().rt.as_ref().unwrap().game.menu_open(menus::START));
    }
}
