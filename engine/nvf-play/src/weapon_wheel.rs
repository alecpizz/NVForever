//! The weapon wheel (docs/design/weapon-wheel.md): a tap of a hotkey uses its slot
//! (`crate::hotkeys`); a hold opens a wheel of all eight, the world slowed and blurred
//! behind it, and letting go uses the slice picked.
//!
//! - **Tap or hold** (§2): a press shorter than [`HOLD`] real seconds is a tap, acted on
//!   when the key is let go; at [`HOLD`] the wheel opens with the held key's slice picked.
//! - **Picking** (§6): another hotkey picks its slice; the mouse's movement since the wheel
//!   opened (or the right stick, on a controller) points from the centre, and past
//!   [`DEAD_ZONE`] the slice is the one in that direction ([`slot_at`]). Letting go of the
//!   key that opened the wheel, or a left click, uses the picked slice; Esc or a right
//!   click closes it doing nothing.
//! - **While it is open** (§5): the game clock eases to [`WHEEL_SCALE`] over [`EASE`]
//!   ([`WeaponWheel::time_scale`], multiplied in by `script::time_multiplier`), the view
//!   blurs ([`WeaponWheel::blur`], nvforever `imagespace.rs`), the HUD is hidden, mouse
//!   look stops and Attack, Block and Activate are not the game's
//!   ([`crate::control::PlayerControl::wheel`]). Not in VATS mode. It is not a menu: the
//!   world keeps running, slowly.
//! - Slot 2 is Ammo Swap: tapping it, or letting go on it, asks `crate::combat` for the
//!   next ammo (`PlayerControl::ammo_swap`).
//! - A weapon equipped from a slot is drawn [design: as RDR2's wheel and the original's
//!   hotkeys, which ready the weapon (hypothesis)].
//! - A slot used, by a tap or from the wheel, plays what the Pip-Boy's ITEMS page plays
//!   for the same thing (`crate::ui_sounds`): the item's put-on or take-off sound, an aid
//!   item's consume sound, the refusal sound for an item not carried or refused.
//!
//! Test aid `NVF_PLAY_TEST_HOTKEYS=<seconds>:<step>[,…]` (world seconds): `assign:<slot>:<item>`
//! (an editor ID or a FormID the player carries), `tap:<slot>`, `hold:<slot>` (the key
//! goes down and stays down), `press:<slot>` (another key, pressed and let go),
//! `point:<degrees>` (the pointer that way, clockwise from up), `release` (the held key
//! goes up), `click`, `cancel`. Log lines: `weapon wheel: open on slot <n> (clock <s>)`,
//! `weapon wheel: the clock runs at <s>, blur <r>`, `weapon wheel: picked slot <n> (<how>)`, `weapon wheel: closed: <outcome>`,
//! `hotkeys: slot <n>: <outcome>`; at exit, when used, `weapon wheel summary: …`.

pub mod draw;

use crate::bindings::{ControlInput, InputDevice};
use crate::control::PlayerControl;
use crate::hotkeys::{self, Hotkeys, Slot, Used, AMMO_SWAP, CONTROLS, SLOTS};
use crate::pause::AidTime;
use crate::sim::SimState;
use crate::ui_sounds::{UiEvent, UiMenu, UiSound};
use crate::{ItemUsed, PlaySet, PlayState};
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use fnv_sim::FormId;

/// A press held this long (real seconds) opens the wheel [design, §2].
pub const HOLD: f32 = 0.25;
/// The game clock's speed while the wheel is open [design, §5].
pub const WHEEL_SCALE: f32 = 0.2;
/// Real seconds the clock and the blur take to ease in, and out [design].
pub const EASE: f32 = 0.15;
/// The blur radius at full strength, the wait menu's (`WAIT_MENU_BLUR`) [design].
pub const BLUR: f32 = 3.0;
/// How dark the view behind the wheel is drawn at full strength [design].
pub const DARKEN: f32 = 0.35;
/// The pointer picks a slice once this far from the centre, interface units [design].
pub const DEAD_ZONE: f32 = 40.0;
/// The pointer goes no farther from the centre than this [design].
pub const REACH: f32 = 200.0;
/// A controller's right stick picks once pushed this far [design].
const STICK_DEAD: f32 = 0.5;

/// A slot's slice's middle, degrees clockwise from straight up: slot 2 (index 1) at 0,
/// slot 4 at 90, slot 8 at 270 (the owner's layout).
pub fn angle_of(index: usize) -> f32 {
    ((index as f32 - 1.0) * 45.0).rem_euclid(360.0)
}

/// The slot whose slice lies in direction `p` from the centre (y down).
pub fn slot_at(p: Vec2) -> usize {
    let a = p.x.atan2(-p.y).to_degrees().rem_euclid(360.0);
    let k = ((a + 22.5) / 45.0).floor() as usize % SLOTS;
    (k + 1) % SLOTS
}

/// The pointer in direction `degrees` (clockwise from up) at `r` from the centre (y down).
pub fn point(degrees: f32, r: f32) -> Vec2 {
    let a = degrees.to_radians();
    Vec2::new(a.sin() * r, -a.cos() * r)
}

fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// One frame's input to the wheel.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame {
    /// Real seconds since the last frame.
    pub dt: f32,
    /// The slot whose key went down this frame (the first, when several did).
    pub pressed: Option<usize>,
    /// Which slots' keys are down.
    pub down: [bool; SLOTS],
    /// The pointer's movement, interface units (y down).
    pub moved: Vec2,
    /// A controller's stick, where it points now (replaces the pointer when pushed).
    pub stick: Option<Vec2>,
    pub click: bool,
    pub cancel: bool,
    /// The wheel may not open (a menu, VATS mode): an open one closes doing nothing.
    pub blocked: bool,
    /// The player's No Hotkey Wheel tweak: a held key never opens the wheel; let go, it
    /// is a tap [design: docs/design/tweaks.md].
    pub no_hold_open: bool,
}

/// What a frame did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// A short press: use the slot.
    Tap(usize),
    Opened(usize),
    /// Another slice picked, by a key (`true`) or the pointer.
    Picked(usize, bool),
    /// Let go (or clicked) on this slot: use it.
    Chose(usize),
    Cancelled,
}

/// The open wheel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Open {
    /// The key that opened it: letting it go uses the picked slice.
    pub opener: usize,
    pub picked: usize,
    /// The pointer from the centre, interface units (y down).
    pub pointer: Vec2,
}

#[derive(Resource, Debug, Default)]
pub struct WeaponWheel {
    /// A key held, and for how long (real seconds), before the wheel opens.
    press: Option<(usize, f32)>,
    pub open: Option<Open>,
    /// 0 closed … 1 open: the clock's and the blur's easing.
    ease: f32,
    /// Closed this frame: the click or key that closed it is not the game's.
    closed_now: bool,
    /// The clock's full slowing was logged since it opened.
    told_slow: bool,
}

impl WeaponWheel {
    /// Cancel pending/open hotkey use without choosing a slot [design, #2200].
    pub(crate) fn cancel_input_hold(&mut self) {
        self.press = None;
        self.closed_now = self.open.take().is_some();
    }

    /// Feed one frame; returns what happened.
    pub fn step(&mut self, f: &Frame) -> Vec<Outcome> {
        let mut out = Vec::new();
        self.closed_now = false;
        if f.blocked {
            self.press = None;
            if self.open.take().is_some() {
                self.closed_now = true;
                out.push(Outcome::Cancelled);
            }
        } else if let Some(o) = self.open.as_mut() {
            if f.cancel {
                self.open = None;
                self.closed_now = true;
                out.push(Outcome::Cancelled);
            } else if f.click || !f.down[o.opener] {
                let picked = o.picked;
                self.open = None;
                self.closed_now = true;
                out.push(Outcome::Chose(picked));
            } else {
                if let Some(k) = f.pressed.filter(|&k| k != o.picked) {
                    o.picked = k;
                    o.pointer = Vec2::ZERO;
                    out.push(Outcome::Picked(k, true));
                }
                match f.stick {
                    Some(s) if s.length() >= STICK_DEAD => o.pointer = s.normalize() * REACH,
                    _ => o.pointer = (o.pointer + f.moved).clamp_length_max(REACH),
                }
                if o.pointer.length() >= DEAD_ZONE {
                    let k = slot_at(o.pointer);
                    if k != o.picked {
                        o.picked = k;
                        out.push(Outcome::Picked(k, false));
                    }
                }
            }
        } else if let Some((k, held)) = self.press.as_mut() {
            *held += f.dt;
            let k = *k;
            if !f.down[k] {
                self.press = None;
                out.push(Outcome::Tap(k));
            } else if *held >= HOLD && !f.no_hold_open {
                self.press = None;
                self.open = Some(Open { opener: k, picked: k, pointer: Vec2::ZERO });
                out.push(Outcome::Opened(k));
            }
        } else if let Some(k) = f.pressed {
            self.press = Some((k, 0.0));
            // A press shorter than a frame: a tap at once.
            if !f.down[k] {
                self.press = None;
                out.push(Outcome::Tap(k));
            }
        }
        let target = if self.open.is_some() { 1.0 } else { 0.0 };
        let step = f.dt / EASE;
        self.ease = if target > self.ease { (self.ease + step).min(1.0) } else { (self.ease - step).max(0.0) };
        out
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// The wheel has the mouse and the attack keys: open, or closed this frame.
    pub fn holds_input(&self) -> bool {
        self.open.is_some() || self.closed_now
    }

    /// How far it is eased in, 0 to 1.
    pub fn amount(&self) -> f32 {
        smoothstep(self.ease)
    }

    /// The game clock's speed it asks for.
    pub fn time_scale(&self) -> f32 {
        1.0 - (1.0 - WHEEL_SCALE) * self.amount()
    }

    /// The blur radius it asks for.
    pub fn blur(&self) -> f32 {
        BLUR * self.amount()
    }
}

/// The test aid's keys (module notes).
#[derive(Resource, Default)]
struct WheelAid {
    steps: Vec<(f32, AidStep)>,
    /// The slot whose key the aid holds down.
    held: Option<usize>,
    /// What the aid does this frame.
    pressed: Option<usize>,
    tapped: Option<usize>,
    point: Option<Vec2>,
    click: bool,
    cancel: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum AidStep {
    Assign(usize, String),
    Tap(usize),
    Hold(usize),
    Press(usize),
    Point(f32),
    Release,
    Click,
    Cancel,
}

fn slot_number(s: &str) -> Result<usize, String> {
    match s.parse::<usize>() {
        Ok(n) if (1..=SLOTS).contains(&n) => Ok(n - 1),
        _ => Err(format!("{s:?}: expected a slot 1 to 8")),
    }
}

fn parse_aid(v: &str) -> Result<Vec<(f32, AidStep)>, String> {
    crate::test_aid::timed(v)?
        .into_iter()
        .map(|(t, what)| {
            let mut parts = what.splitn(3, ':');
            let step = match (parts.next().unwrap_or(""), parts.next(), parts.next()) {
                ("assign", Some(n), Some(item)) if !item.is_empty() => AidStep::Assign(slot_number(n)?, item.to_string()),
                ("tap", Some(n), None) => AidStep::Tap(slot_number(n)?),
                ("hold", Some(n), None) => AidStep::Hold(slot_number(n)?),
                ("press", Some(n), None) => AidStep::Press(slot_number(n)?),
                ("point", Some(d), None) => AidStep::Point(d.parse::<f32>().map_err(|_| format!("{d:?}: expected degrees"))?),
                ("release", None, None) => AidStep::Release,
                ("click", None, None) => AidStep::Click,
                ("cancel", None, None) => AidStep::Cancel,
                _ => return Err(format!("{what:?}: expected assign:<slot>:<item>, tap:<slot>, hold:<slot>, press:<slot>, point:<degrees>, release, click or cancel")),
            };
            Ok((t, step))
        })
        .collect()
}

/// What happened, for the summary.
#[derive(Resource, Default)]
struct WheelStats {
    taps: usize,
    opened: usize,
    chosen: Vec<usize>,
    cancelled: usize,
}

/// A release-confirmed touch hotbar action. Uses the normal wheel equip/use path.
#[derive(Message, Clone, Copy, Debug)]
pub struct TouchHotkey(pub usize);

pub struct WeaponWheelPlugin;

impl Plugin for WeaponWheelPlugin {
    fn build(&self, app: &mut App) {
        let steps = match std::env::var("NVF_PLAY_TEST_HOTKEYS").map(|v| parse_aid(&v)) {
            Ok(Ok(a)) => a,
            Ok(Err(e)) => {
                warn!("NVF_PLAY_TEST_HOTKEYS: {e}");
                Vec::new()
            }
            Err(_) => Vec::new(),
        };
        app.init_resource::<WeaponWheel>()
            .add_message::<TouchHotkey>()
            .add_message::<UiSound>()
            .init_resource::<Hotkeys>()
            .init_resource::<WheelStats>()
            .insert_resource(WheelAid { steps, ..default() })
            .add_systems(Update, (run_aid, read).chain().in_set(PlaySet::Input).before(crate::control::read_controls))
            .add_systems(Last, report);
        crate::save::add_save_hook(app, hotkeys::SAVE_HOOK);
        draw::build(app);
    }
}

fn run_aid(time: AidTime, mut aid: ResMut<WheelAid>, mut hotkeys: ResMut<Hotkeys>, mut sim: ResMut<SimState>, script: Option<Res<crate::script::ScriptState>>) {
    aid.pressed = None;
    aid.tapped = None;
    aid.point = None;
    aid.click = false;
    aid.cancel = false;
    if aid.steps.is_empty() {
        return;
    }
    let Some(now) = time.world_secs() else { return };
    let mut due: Vec<(f32, AidStep)> = aid.steps.iter().filter(|(t, _)| *t <= now).cloned().collect();
    due.sort_by(|a, b| a.0.total_cmp(&b.0));
    aid.steps.retain(|(t, _)| *t > now);
    for (_, step) in due {
        info!("test aid NVF_PLAY_TEST_HOTKEYS: {step:?}");
        match step {
            AidStep::Assign(n, item) => {
                let id = script.as_ref().and_then(|s| s.rt.as_ref()).and_then(|rt| fnv_game::runner::resolve(&rt.game, &item).ok()).or_else(|| u32::from_str_radix(item.trim_start_matches("0x"), 16).ok().map(FormId));
                let Some(id) = id else {
                    warn!("test aid NVF_PLAY_TEST_HOTKEYS: {item}: no such form");
                    continue;
                };
                let kind = sim.forms.item(id).map(|i| i.kind);
                let mods = sim.world.inventory_mut(FormId::PLAYER_REF).ok().and_then(|inv| inv.groups().find(|g| g.0 == id).map(|g| g.1.mods)).unwrap_or(0);
                let name = sim.name(id).unwrap_or("?").to_string();
                match hotkeys.assign(n, Slot { item: id.0, mods }, kind) {
                    Ok(_) => info!("hotkeys: slot {} holds {name} ({id})", n + 1),
                    Err(e) => warn!("test aid NVF_PLAY_TEST_HOTKEYS: {name} not assigned: {e:?}"),
                }
            }
            AidStep::Tap(n) => aid.tapped = Some(n),
            AidStep::Hold(n) => {
                aid.held = Some(n);
                aid.pressed = Some(n);
            }
            AidStep::Press(n) => aid.pressed = Some(n),
            AidStep::Point(d) => aid.point = Some(point(d, REACH)),
            AidStep::Release => aid.held = None,
            AidStep::Click => aid.click = true,
            AidStep::Cancel => aid.cancel = true,
        }
    }
}

/// Real time, the game clock and the play state.
type Clocks<'w> = (Res<'w, Time<Real>>, Res<'w, Time<Virtual>>, Res<'w, State<PlayState>>);

type Availability<'w> = (
    Option<Res<'w, crate::vats_mode::VatsMode>>,
    Res<'w, WheelAid>,
    Option<Res<'w, NextState<PlayState>>>,
);

/// The wheel's scroll, the scope (whose wheel zoom it yields to) and combat (a reload under way).
type Extras<'w> = (Option<Res<'w, bevy::input::mouse::AccumulatedMouseScroll>>, Option<Res<'w, crate::aim_zoom::Scope>>, Option<Res<'w, crate::combat::Combat>>);

/// The frame's keys, mouse and stick into the wheel; its outcomes carried out.
#[allow(clippy::too_many_arguments)]
fn read(
    input: ControlInput,
    (motion, mut touch_requests, touch_switch): (
        Res<AccumulatedMouseMotion>, MessageReader<TouchHotkey>, Option<Res<crate::pad::TouchDeviceSwitch>>,
    ),
    (real, clock, state): Clocks,
    windows: Query<&Window, With<PrimaryWindow>>,
    (vats, aid, next_play): Availability<'_>,
    mut wheel: ResMut<WeaponWheel>,
    mut control: ResMut<PlayerControl>,
    (hotkeys, mut sim, mut used, mut stats): (Res<Hotkeys>, ResMut<SimState>, MessageWriter<ItemUsed>, ResMut<WheelStats>),
    (mut sounds, anim, mut wait): (MessageWriter<UiSound>, Option<Res<crate::combat::WeaponAnim>>, Local<SwapWait>),
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
    mut script: Option<ResMut<crate::script::ScriptState>>,
    (scroll, scope, combat): Extras<'_>,
) {
    // The player's Weapon Requirements Matter tweak: a slot whose weapon the player may
    // not equip says why instead [design: docs/design/tweaks.md].
    fn slot_unmet(hotkeys: &Hotkeys, n: usize, sim: &mut SimState, script: Option<&mut crate::script::ScriptState>) -> Option<String> {
        let item = FormId(hotkeys.0.get(n).copied().flatten()?.item);
        // One already in hand is drawn or put away as ever.
        if sim.world.inventory_mut(FormId::PLAYER_REF).is_ok_and(|i| i.is_equipped(item)) {
            return None;
        }
        let rt = script.and_then(|s| s.rt.as_mut())?;
        crate::script::with_world(rt, sim, |rt| rt.game.weapon_requirement_unmet(item))
    }
    // The player's tweak: no weapon hotkeys while reloading [design: docs/design/tweaks.md].
    // Reloading now, not `WeaponAnim::reload` (the weapon's reload clip, set for every weapon
    // held: it blocked the hotkeys for good, #4611).
    let reloading_blocks = tweaks.as_ref().is_some_and(|t| t.no_hotkeys_while_reloading) && combat.as_deref().is_some_and(crate::combat::Combat::player_reloading);
    let toggle_in_hand = tweaks.as_ref().is_some_and(|t| t.hotkey_toggles_in_hand);
    let quiet = tweaks.as_ref().is_some_and(|t| t.quiet_hotkeys);
    let hotkeys_off = tweaks.as_ref().is_some_and(|t| t.no_weapon_hotkeys);
    // A swap waiting for the weapon in hand to go away: the clips of the last frame say
    // whether it has (`crate::combat::WeaponAnim`; no body to time it: at once).
    let away = anim.as_deref().is_none_or(|a| !a.ready && !a.drawing && !a.holstering);
    match wait.step(clock.delta_secs(), control.weapon_out, away) {
        Some(SwapStep::Equip(n)) => match slot_unmet(&hotkeys, n, &mut sim, script.as_deref_mut()) {
            Some(why) => info!("hotkeys: slot {}: refused, {why} (tweak)", n + 1),
            None => {
                act(n, &hotkeys, &mut sim, &mut control, &mut used, &mut sounds, &mut wait, script.as_deref_mut(), true, toggle_in_hand, quiet);
            }
        },
        Some(SwapStep::CalledOff(n)) => info!("hotkeys: slot {}: the swap called off, the weapon in hand drawn again", n + 1),
        None => {}
    }
    // The player's tweak: holding a hotkey does not open the wheel [design: docs/design/tweaks.md].
    let mut f = Frame { dt: real.delta_secs(), no_hold_open: tweaks.as_ref().is_some_and(|t| t.no_hotkey_wheel), ..default() };
    // The player's tweak: the D-pad's left and right step through the hotkeyed weapons
    // instead of being Hotkey 4 and Ammo Swap [design: docs/design/tweaks.md].
    let pad_cycles = tweaks.as_ref().is_some_and(|t| t.dpad_cycles_hotkeys) && input.bindings.device == InputDevice::Controller;
    let dpad = |b: GamepadButton, just: bool| pad_cycles && input.pads.iter().any(|p| if just { p.just_pressed(b) } else { p.pressed(b) });
    let pad_step = if dpad(GamepadButton::DPadRight, true) {
        Some(true)
    } else if dpad(GamepadButton::DPadLeft, true) {
        Some(false)
    } else {
        None
    };
    for (i, &c) in CONTROLS.iter().enumerate() {
        // The D-pad's own presses are the stepping's, not these controls'.
        let taken = (c == crate::bindings::Control::AmmoSwap && dpad(GamepadButton::DPadRight, false)) || (c == crate::bindings::Control::Hotkey4 && dpad(GamepadButton::DPadLeft, false));
        if taken {
            continue;
        }
        f.down[i] = input.pressed(c);
        if f.pressed.is_none() && input.just_pressed(c) {
            f.pressed = Some(i);
        }
    }
    // The aid's keys: a tap is down for this frame only (a press shorter than a frame).
    if let Some(n) = aid.held {
        f.down[n] = true;
    }
    if let Some(n) = aid.tapped {
        f.pressed = Some(n);
        f.down[n] = false;
    }
    if let Some(n) = aid.pressed {
        f.pressed = Some(n);
    }
    // The pointer's travel in units of the wheel as drawn, at the UI scale (#1631).
    let k = windows.single().map_or(1.0, |w| 960.0 / w.height().max(1.0)) / crate::interface::unit_scale();
    f.moved = motion.delta * k;
    if let Some(p) = aid.point {
        f.moved = p - wheel.open.map_or(Vec2::ZERO, |o| o.pointer);
    }
    if input.bindings.device == InputDevice::Controller {
        let s = crate::pad::stick(&input.pads, false);
        f.stick = Some(Vec2::new(s.x, -s.y));
    }
    f.click = input.mouse.just_pressed(MouseButton::Left) || aid.click;
    f.cancel = input.mouse.just_pressed(MouseButton::Right) || input.keys.just_pressed(KeyCode::Escape) || aid.cancel;
    f.blocked = *state.get() != PlayState::Playing || vats.is_some_and(|v| v.holds_input()) || input.kill_cam.as_ref().is_some_and(|k| k.active());
    // At most one touch action per frame. Covered/menu/controller/script-disabled
    // requests are discarded now, never replayed after their admission context changes.
    let touch_playing = next_play.as_deref().is_none_or(|next| !matches!(next,NextState::Pending(state)|NextState::PendingIfDifferent(state) if *state!=PlayState::Playing));
    let touch_request = touch_requests.read().find_map(|r| (r.0 < SLOTS).then_some(r.0));
    touch_requests.clear();
    if let Some(n) = touch_request.filter(|&n| !f.blocked && touch_playing && f.pressed.is_none() && !wheel.holds_input()
        && touch_switch.as_ref().is_none_or(|s| s.allows_actions())
        && input.virtual_action_allowed(CONTROLS[n])) {
        f.pressed = Some(n);
        f.down[n] = false;
    }
    let was_open = wheel.is_open();
    for o in wheel.step(&f) {
        match o {
            Outcome::Opened(n) => {
                stats.opened += 1;
                info!("weapon wheel: open on slot {} (clock {WHEEL_SCALE} over {EASE} s)", n + 1);
            }
            Outcome::Picked(n, by_key) => {
                info!("weapon wheel: picked slot {} ({})", n + 1, if by_key { "key" } else { "pointer" });
                // The Pip-Boy's click for a selection that moves (`ui_sounds`: the ITEMS
                // page's row change, UIPipBoyScroll) [design: weapon-wheel.md §8, #985].
                sounds.write(UiSound { menu: UiMenu::Inventory, event: UiEvent::Focus });
            }
            Outcome::Cancelled => {
                stats.cancelled += 1;
                info!("weapon wheel: closed: nothing used");
            }
            // The player's tweak: the number keys and the D-pad pick no weapon [design: docs/design/tweaks.md].
            Outcome::Tap(n) if hotkeys_off => {
                info!("weapon wheel: slot {} not used, weapon hotkeys are off (tweak)", n + 1);
            }
            Outcome::Tap(n) | Outcome::Chose(n) if reloading_blocks => {
                info!("weapon wheel: slot {} not used while reloading (tweak)", n + 1);
            }
            Outcome::Tap(n) | Outcome::Chose(n) => {
                if matches!(o, Outcome::Tap(_)) {
                    stats.taps += 1;
                } else {
                    stats.chosen.push(n);
                }
                let line = match slot_unmet(&hotkeys, n, &mut sim, script.as_deref_mut()) {
                    Some(why) => {
                        info!("hotkeys: slot {}: refused, {why} (tweak)", n + 1);
                        why
                    }
                    None => act(n, &hotkeys, &mut sim, &mut control, &mut used, &mut sounds, &mut wait, script.as_deref_mut(), false, toggle_in_hand, quiet),
                };
                if matches!(o, Outcome::Chose(_)) {
                    info!("weapon wheel: closed: slot {}: {line}", n + 1);
                }
            }
        }
    }
    // The player's tweak: in first person, with no scope up and the wheel closed, the
    // mouse wheel steps to the next hotkey (up) or the one before holding a carried
    // weapon not in hand [design: docs/design/tweaks.md].
    let notches = scroll.as_ref().map_or(0.0, |s| s.delta.y);
    let wheel_steps = tweaks.as_ref().is_some_and(|t| t.wheel_cycles_hotkeys) && notches != 0.0 && !crate::quick_loot::holds_input() && !control.third_person_view() && !scope.is_some_and(|s| s.on);
    // Up the list: the wheel rolled up, or the D-pad's right (the tweak above).
    let up = if wheel_steps { Some(notches > 0.0) } else { pad_step };
    if let (Some(up), false, false, false, false, false) = (up, f.blocked, was_open, wheel.is_open(), hotkeys_off, reloading_blocks) {
        let notches = if up { 1.0 } else { -1.0 };
        let in_hand = sim.world.equipment(FormId::PLAYER_REF).unwrap_or_default().into_iter().find(|&e| sim.forms.item(e).is_some_and(|i| i.kind == fnv_sim::forms::ItemKind::Weapon));
        let from = in_hand.and_then(|w| hotkeys.0.iter().position(|s| s.is_some_and(|s| FormId(s.item) == w))).unwrap_or(0);
        let step = if notches > 0.0 { 1 } else { hotkeys::SLOTS - 1 };
        let next = (1..hotkeys::SLOTS).map(|k| (from + step * k) % hotkeys::SLOTS).find(|&n| n != hotkeys::AMMO_SWAP && hotkeys::takes_hand(&mut sim.world, hotkeys.0[n]));
        if let Some(n) = next {
            let line = match slot_unmet(&hotkeys, n, &mut sim, script.as_deref_mut()) {
                Some(why) => why,
                None => act(n, &hotkeys, &mut sim, &mut control, &mut used, &mut sounds, &mut wait, script.as_deref_mut(), false, toggle_in_hand, quiet),
            };
            info!("hotkeys: the mouse wheel or the D-pad steps to slot {}: {line} (tweak)", n + 1);
        }
    }
    // Once the game clock has followed (`script::time_multiplier` sets it).
    if wheel.is_open() && wheel.amount() >= 1.0 && !wheel.told_slow && clock.relative_speed() <= wheel.time_scale() + 1e-3 {
        wheel.told_slow = true;
        info!("weapon wheel: the clock runs at {:.2}, blur {:.1}", clock.relative_speed(), wheel.blur());
    }
    if !wheel.is_open() {
        wheel.told_slow = false;
    }
    if was_open && !wheel.is_open() {
        // The keys that closed it are not the game's this frame.
        control.trigger = false;
    }
    control.wheel = wheel.holds_input();
}

/// The inventory's sound for what a slot did, as the Pip-Boy's ITEMS page plays it
/// (`crate::ui_sounds`): the item's own put-on or take-off sound, an aid item's consume
/// sound, the refusal sound; none for an empty slot or a weapon already in hand.
fn sound_of(slot: Option<Slot>, outcome: &Used) -> Option<UiEvent> {
    let item = slot.map(|s| FormId(s.item));
    match outcome {
        Used::Equipped => Some(UiEvent::Equipped { item: item?, on: true }),
        Used::TookOff => Some(UiEvent::Equipped { item: item?, on: false }),
        Used::UsedUp(item) => Some(UiEvent::Used { item: *item }),
        Used::NotCarried | Used::Refused(_) => Some(UiEvent::Refused),
        Used::Empty | Used::InHand => None,
    }
}

/// Use slot `n` (a tap, or the wheel let go on it); returns what happened. `put_away`:
/// a waiting swap whose weapon in hand has just gone away (`SwapWait`).
#[allow(clippy::too_many_arguments)]
fn act(n: usize, hotkeys: &Hotkeys, sim: &mut SimState, control: &mut PlayerControl, used: &mut MessageWriter<ItemUsed>, sounds: &mut MessageWriter<UiSound>, wait: &mut SwapWait, script: Option<&mut crate::script::ScriptState>, put_away: bool, toggle_in_hand: bool, quiet: bool) -> String {
    // `quiet`: the player's No Hotkey Sounds tweak [design: docs/design/tweaks.md].
    if n == AMMO_SWAP {
        control.ammo_swap = true;
        info!("hotkeys: slot 2: Ammo Swap");
        return "Ammo Swap".into();
    }
    let slot = hotkeys.0[n];
    let name = slot.map_or_else(String::new, |s| crate::menu::model::display_name(sim.forms, FormId(s.item)));
    if !put_away {
        if let Some(s) = slot {
            let item = FormId(s.item);
            if hotkeys::carried(&mut sim.world, s).0 > 0 {
                if let Some(rt) = script.and_then(|st| st.rt.as_mut()).filter(|rt| rt.game.has_pre_activate_inventory_handlers()) {
                    let worn = sim.world.inventory_mut(FormId::PLAYER_REF).is_ok_and(|i| i.is_equipped(item));
                    let allowed = crate::script::with_world(rt, sim, |rt| rt.game.pre_activate_inventory_item(FormId::PLAYER_REF, item, (n + 1) as i32, !worn));
                    if !allowed {
                        let line = format!("{name}: activation prevented by script");
                        info!("hotkeys: slot {}: {line}", n + 1);
                        return line;
                    }
                }
            }
        }
    }
    // A new weapon with one drawn: holster first, equip once it is away. Equipping now
    // would swap the model at once and play the holster clip and the Unequip sound on
    // the new weapon (the #1258 review). Another weapon picked while one waits takes
    // its place.
    if (control.weapon_out || wait.slot.is_some()) && hotkeys::takes_hand(&mut sim.world, slot) {
        holster_for_new_weapon(control);
        *wait = SwapWait { slot: Some(n), waited: 0.0 };
        info!("hotkeys: slot {}: putting the weapon in hand away for {name}", n + 1);
        return format!("putting the weapon in hand away for {name}");
    }
    let outcome = hotkeys::use_slot(&mut sim.world, slot);
    if let Some(event) = sound_of(slot, &outcome).filter(|_| !quiet) {
        sounds.write(UiSound { menu: UiMenu::Inventory, event });
    }
    match &outcome {
        Used::UsedUp(item) => {
            used.write(ItemUsed { item: item.0, limb: None });
        }
        Used::Equipped if slot.is_some_and(|s| sim.forms.item(FormId(s.item)).is_some_and(|i| i.kind == fnv_sim::forms::ItemKind::Weapon)) => {
            holster_for_new_weapon(control);
            info!("hotkeys: slot {}: {name} equipped holstered ({})", n + 1, if put_away { "the weapon in hand put away" } else { "nothing was drawn" });
        }
        // The player's tweak: the hotkey of the weapon in hand draws or holsters it [design: docs/design/tweaks.md].
        Used::InHand if toggle_in_hand => {
            control.weapon_out = !control.weapon_out;
            info!("hotkeys: slot {}: {name} {} (tweak)", n + 1, if control.weapon_out { "drawn" } else { "holstered" });
            return if control.weapon_out { format!("{name} drawn") } else { format!("{name} holstered") };
        }
        _ => {}
    }
    let line = outcome.describe(&name);
    info!("hotkeys: slot {}: {line}", n + 1);
    line
}

/// A weapon taken from a hotkey or the wheel comes in holstered: one in hand is put
/// away, and the player draws the new one with Ready Item [design, the owner 2026-10-31,
/// #1106: may differ from the original]. Returns whether a weapon was out.
fn holster_for_new_weapon(control: &mut PlayerControl) -> bool {
    let was_out = control.weapon_out;
    control.weapon_out = false;
    control.lowered = false;
    control.ready_held = None;
    control.trigger = false;
    was_out
}

/// The longest a swap waits for the holster clip (game seconds); a clip that never ends
/// does not keep the new weapon out of reach [engineering choice].
const SWAP_WAIT_MAX: f32 = 4.0;

/// A weapon from a slot waiting for the weapon in hand to be put away.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct SwapWait {
    slot: Option<usize>,
    waited: f32,
}

/// What a waiting swap does this frame.
#[derive(Debug, PartialEq)]
enum SwapStep {
    /// The weapon in hand is away: equip the slot's.
    Equip(usize),
    /// Ready Item drew the weapon in hand again before it was away: no swap.
    CalledOff(usize),
}

impl SwapWait {
    fn step(&mut self, dt: f32, weapon_out: bool, away: bool) -> Option<SwapStep> {
        let n = self.slot?;
        self.waited += dt;
        if weapon_out {
            self.slot = None;
            Some(SwapStep::CalledOff(n))
        } else if away || self.waited >= SWAP_WAIT_MAX {
            self.slot = None;
            Some(SwapStep::Equip(n))
        } else {
            None
        }
    }
}

fn report(mut exit: MessageReader<AppExit>, stats: Res<WheelStats>) {
    if exit.read().next().is_none() || (stats.taps == 0 && stats.opened == 0) {
        return;
    }
    let chosen: Vec<String> = stats.chosen.iter().map(|n| (n + 1).to_string()).collect();
    info!("weapon wheel summary: {} taps, opened {} times, used slots [{}], {} cancelled", stats.taps, stats.opened, chosen.join(", "), stats.cancelled);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_cancels_pending_and_open_hotkeys_without_using_a_slot() {
        use bevy::input::{gamepad::{RawGamepadButtonChangedEvent, RawGamepadEvent}, keyboard::{Key, KeyboardInput}, ButtonState};
        for open in [false, true] {
            let forms = Box::leak(Box::new(fnv_sim::FormStore::default()));
            let refs = Box::leak(Box::new(fnv_sim::RefTable::default()));
            let sim = SimState { forms, refs, world: fnv_sim::World::new(forms, refs, fnv_sim::SimConfig::default()) };
            let mut app = App::new();
            app.add_plugins((MinimalPlugins, bevy::input::InputPlugin, bevy::state::app::StatesPlugin, crate::pad::InputSwitchPlugin))
                .insert_state(PlayState::Playing)
                .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(50)))
                .init_resource::<crate::bindings::Bindings>().init_resource::<PlayerControl>()
                .init_resource::<WeaponWheel>().init_resource::<Hotkeys>().init_resource::<WheelAid>().init_resource::<WheelStats>()
                .insert_resource(sim)
                .add_message::<ItemUsed>().add_message::<UiSound>().add_message::<TouchHotkey>()
                .add_systems(Update, read);
            let pad = app.world_mut().spawn(Gamepad::default()).id();
            let code = app.world().resource::<crate::bindings::Bindings>().get(CONTROLS[0]).key.unwrap();
            let key = KeyboardInput { key_code: code, logical_key: Key::Character("1".into()), state: ButtonState::Pressed, text: None, repeat: false, window: Entity::PLACEHOLDER };
            app.world_mut().write_message(key.clone());
            app.update();
            if open {
                for _ in 0..20 { app.update(); }
                assert!(app.world().resource::<WeaponWheel>().is_open());
                app.world_mut().resource_mut::<WeaponWheel>().open.as_mut().unwrap().picked = 3;
            } else {
                assert!(app.world().resource::<WeaponWheel>().press.is_some());
            }
            app.world_mut().write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(pad, GamepadButton::North, 1.0)));
            app.update();
            assert_eq!(app.world().resource::<crate::bindings::Bindings>().device, InputDevice::KeyboardMouse);
            crate::bindings::set_device(app.world_mut(), InputDevice::Controller);
            assert_eq!(app.world().resource::<crate::bindings::Bindings>().device, InputDevice::Controller);
            app.world_mut().write_message(KeyboardInput { state: ButtonState::Released, ..key });
            app.update();
            let wheel = app.world().resource::<WeaponWheel>();
            assert!(!wheel.is_open() && wheel.press.is_none());
            let stats = app.world().resource::<WheelStats>();
            assert_eq!(stats.taps, 0);
            assert!(stats.chosen.is_empty());
            assert!(app.world().resource::<Messages<ItemUsed>>().is_empty());
        }
    }

    #[test]
    fn a_hotkeyed_weapon_comes_in_holstered() {
        // Holstered: it stays holstered.
        let mut c = PlayerControl { weapon_out: false, ..default() };
        assert!(!holster_for_new_weapon(&mut c));
        assert!(!c.weapon_out, "a holstered player is not made to draw");
        // Drawn (and lowered, mid hold of Ready Item, trigger down): the weapon in hand is
        // put away and nothing carries over to the new one.
        let mut c = PlayerControl { weapon_out: true, lowered: true, ready_held: Some(0.2), trigger: true, ..default() };
        assert!(holster_for_new_weapon(&mut c));
        assert!(!c.weapon_out && !c.lowered && !c.trigger && c.ready_held.is_none(), "{c:?}");
    }

    #[test]
    fn a_swap_waits_for_the_weapon_in_hand_to_go_away() {
        let mut w = SwapWait::default();
        assert_eq!(w.step(0.1, false, true), None, "nothing waiting");
        // Holstering: wait; holstered: equip, once.
        w.slot = Some(2);
        assert_eq!(w.step(0.1, false, false), None);
        assert_eq!(w.step(0.1, false, true), Some(SwapStep::Equip(2)));
        assert_eq!(w.step(0.1, false, true), None);
        // Drawn again before it was away: no swap.
        let mut w = SwapWait { slot: Some(0), waited: 0.0 };
        assert_eq!(w.step(0.1, true, false), Some(SwapStep::CalledOff(0)));
        assert_eq!(w.slot, None);
        // A holster that never ends gives up waiting.
        let mut w = SwapWait { slot: Some(3), waited: 0.0 };
        assert_eq!(w.step(SWAP_WAIT_MAX - 0.5, false, false), None);
        assert_eq!(w.step(1.0, false, false), Some(SwapStep::Equip(3)));
    }

    fn frame(dt: f32) -> Frame {
        Frame { dt, ..default() }
    }

    fn down(mut f: Frame, k: usize) -> Frame {
        f.down[k] = true;
        f
    }

    #[test]
    fn a_used_slot_sounds_as_the_items_page() {
        let slot = Some(Slot { item: 0x1234, mods: 0 });
        let item = FormId(0x1234);
        assert_eq!(sound_of(slot, &Used::Equipped), Some(UiEvent::Equipped { item, on: true }));
        assert_eq!(sound_of(slot, &Used::TookOff), Some(UiEvent::Equipped { item, on: false }));
        assert_eq!(sound_of(slot, &Used::UsedUp(item)), Some(UiEvent::Used { item }));
        assert_eq!(sound_of(slot, &Used::NotCarried), Some(UiEvent::Refused));
        assert_eq!(sound_of(slot, &Used::InHand), None);
        assert_eq!(sound_of(None, &Used::Empty), None);
    }

    #[test]
    fn slices_lie_clockwise_from_the_top_left() {
        // The owner's layout: 1 top left, 2 top, 4 right, 6 bottom, 8 left.
        assert_eq!(slot_at(Vec2::new(0.0, -100.0)), 1);
        assert_eq!(slot_at(Vec2::new(-100.0, -100.0)), 0);
        assert_eq!(slot_at(Vec2::new(100.0, -100.0)), 2);
        assert_eq!(slot_at(Vec2::new(100.0, 0.0)), 3);
        assert_eq!(slot_at(Vec2::new(100.0, 100.0)), 4);
        assert_eq!(slot_at(Vec2::new(0.0, 100.0)), 5);
        assert_eq!(slot_at(Vec2::new(-100.0, 100.0)), 6);
        assert_eq!(slot_at(Vec2::new(-100.0, 0.0)), 7);
        for i in 0..SLOTS {
            assert_eq!(slot_at(point(angle_of(i), 100.0)), i, "slot {}", i + 1);
            // Just inside each edge of the slice.
            assert_eq!(slot_at(point(angle_of(i) + 22.0, 100.0)), i);
            assert_eq!(slot_at(point(angle_of(i) - 22.0, 100.0)), i);
        }
    }

    #[test]
    fn a_short_press_is_a_tap_on_release() {
        let mut w = WeaponWheel::default();
        let mut f = down(frame(0.016), 2);
        f.pressed = Some(2);
        assert!(w.step(&f).is_empty());
        assert!(w.step(&down(frame(0.1), 2)).is_empty());
        assert_eq!(w.step(&frame(0.016)), vec![Outcome::Tap(2)]);
        assert!(!w.is_open());
        // Down and up within one frame.
        let mut f = frame(0.016);
        f.pressed = Some(4);
        assert_eq!(w.step(&f), vec![Outcome::Tap(4)]);
    }

    #[test]
    fn a_hold_opens_and_letting_go_uses_the_pick() {
        let mut w = WeaponWheel::default();
        let mut f = down(frame(0.016), 0);
        f.pressed = Some(0);
        w.step(&f);
        assert_eq!(w.step(&down(frame(HOLD), 0)), vec![Outcome::Opened(0)]);
        assert!(w.is_open() && w.holds_input());
        // The mouse to the right picks slot 4.
        let mut f = down(frame(0.016), 0);
        f.moved = Vec2::new(30.0, 0.0);
        assert!(w.step(&f).is_empty(), "inside the dead zone the key's slice stays");
        f.moved = Vec2::new(30.0, 0.0);
        assert_eq!(w.step(&f), vec![Outcome::Picked(3, false)]);
        // Another key picks its slice and centres the pointer.
        let mut f = down(frame(0.016), 0);
        f.pressed = Some(6);
        assert_eq!(w.step(&f), vec![Outcome::Picked(6, true)]);
        assert_eq!(w.open.unwrap().pointer, Vec2::ZERO);
        // Letting go of the key that opened it: slot 7 is used.
        assert_eq!(w.step(&frame(0.016)), vec![Outcome::Chose(6)]);
        assert!(!w.is_open() && w.holds_input(), "the closing frame keeps the input");
        assert!(w.step(&frame(0.016)).is_empty());
        assert!(!w.holds_input());
    }

    #[test]
    fn cancel_and_blocking_close_without_using() {
        let mut w = WeaponWheel::default();
        let mut f = down(frame(HOLD), 5);
        f.pressed = Some(5);
        w.step(&f);
        w.step(&down(frame(HOLD), 5));
        assert!(w.is_open());
        let mut c = down(frame(0.016), 5);
        c.cancel = true;
        assert_eq!(w.step(&c), vec![Outcome::Cancelled]);
        // A held key that is still down after a cancel opens nothing until pressed again.
        assert!(w.step(&down(frame(1.0), 5)).is_empty());
        assert!(!w.is_open());
        // Opened, then a menu takes the screen.
        let mut f = down(frame(0.0), 1);
        f.pressed = Some(1);
        w.step(&f);
        w.step(&down(frame(HOLD), 1));
        let mut b = down(frame(0.016), 1);
        b.blocked = true;
        assert_eq!(w.step(&b), vec![Outcome::Cancelled]);
    }

    #[test]
    fn the_clock_and_blur_ease_in_and_out() {
        let mut w = WeaponWheel::default();
        assert_eq!(w.time_scale(), 1.0);
        let mut f = down(frame(0.0), 0);
        f.pressed = Some(0);
        w.step(&f);
        w.step(&down(frame(HOLD - 0.01), 0));
        assert!(!w.is_open());
        assert_eq!(w.step(&down(frame(0.02), 0)), vec![Outcome::Opened(0)]);
        assert!(w.time_scale() < 1.0 && w.time_scale() > WHEEL_SCALE, "easing in");
        w.step(&down(frame(EASE), 0));
        assert!((w.time_scale() - WHEEL_SCALE).abs() < 1e-6);
        assert!((w.blur() - BLUR).abs() < 1e-6);
        w.step(&frame(EASE));
        assert_eq!(w.time_scale(), 1.0);
        assert_eq!(w.blur(), 0.0);
    }

    #[test]
    fn the_stick_points_where_it_is_pushed() {
        let mut w = WeaponWheel::default();
        let mut f = down(frame(0.0), 0);
        f.pressed = Some(0);
        w.step(&f);
        w.step(&down(frame(HOLD), 0));
        let mut f = down(frame(0.016), 0);
        f.stick = Some(Vec2::new(0.0, 0.9));
        assert_eq!(w.step(&f), vec![Outcome::Picked(5, false)]);
        // Let go of the stick: the pick stays.
        f.stick = Some(Vec2::ZERO);
        assert!(w.step(&f).is_empty());
        assert_eq!(w.open.unwrap().picked, 5);
    }

    #[test]
    fn the_aid_reads_its_steps() {
        let s = parse_aid("10:assign:1:WeapNV357Revolver,11:tap:1,12:hold:3,12.5:point:90,13:press:6,13.2:release,14:cancel,15:click").unwrap();
        assert_eq!(s[0], (10.0, AidStep::Assign(0, "WeapNV357Revolver".into())));
        assert_eq!(s[1].1, AidStep::Tap(0));
        assert_eq!(s[2].1, AidStep::Hold(2));
        assert_eq!(s[3].1, AidStep::Point(90.0));
        assert_eq!(s[4].1, AidStep::Press(5));
        assert_eq!(s[5].1, AidStep::Release);
        assert!(parse_aid("1:tap:9").is_err());
        assert!(parse_aid("1:spin").is_err());
    }

    #[test]
    fn the_smoke_scenarios_steps_parse() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/smoke/scenarios.toml")).unwrap();
        let mut n = 0;
        for part in text.split("NVF_PLAY_TEST_HOTKEYS = \"").skip(1) {
            let value = part.split('"').next().unwrap();
            parse_aid(value).unwrap_or_else(|e| panic!("{value:?}: {e}"));
            n += 1;
        }
        assert!(n > 0, "a scenario uses the aid");
    }
    fn touch_app() -> App {
        let forms=Box::leak(Box::new(fnv_sim::FormStore::default()));
        let refs=Box::leak(Box::new(fnv_sim::RefTable::default()));
        let sim=SimState{forms,refs,world:fnv_sim::World::new(forms,refs,fnv_sim::SimConfig::default())};
        let mut app=App::new();
        app.add_plugins((MinimalPlugins,bevy::input::InputPlugin,bevy::state::app::StatesPlugin,crate::pad::InputSwitchPlugin))
            .insert_state(PlayState::Playing)
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(16)))
            .init_resource::<crate::bindings::Bindings>().init_resource::<PlayerControl>()
            .init_resource::<WeaponWheel>().init_resource::<Hotkeys>().init_resource::<WheelAid>().init_resource::<WheelStats>()
            .insert_resource(sim).add_message::<ItemUsed>().add_message::<UiSound>().add_message::<TouchHotkey>()
            .add_systems(Update,read);
        app
    }
    #[test]
    fn touch_release_uses_actual_ammo_swap_without_synthetic_hardware() {
        let mut app=touch_app();
        app.world_mut().resource_mut::<crate::bindings::Bindings>().set(CONTROLS[AMMO_SWAP],crate::bindings::Binding{key:Some(KeyCode::KeyK),mouse:None});
        app.world_mut().write_message(TouchHotkey(AMMO_SWAP));app.update();
        assert!(app.world().resource::<PlayerControl>().ammo_swap);
        assert!(app.world().resource::<ButtonInput<KeyCode>>().get_pressed().next().is_none());
        assert_eq!(app.world().resource::<WheelStats>().taps,1);
        assert!(app.world().resource::<Messages<ItemUsed>>().is_empty());
    }
    #[test]
    fn rejected_touch_requests_are_not_replayed_after_cover_menu_or_script_capture() {
        for gate in 0..4 {
            let mut app=touch_app();
            match gate {
                0=>{let mut g=crate::pad::PlatformInputGate::default();g.set_blocked(true);app.insert_resource(g);},
                1=>{app.insert_state(PlayState::Menu);},
                2=>{app.insert_resource(crate::script_input::ScriptBlocked{keys:vec![app.world().resource::<crate::bindings::Bindings>().get(CONTROLS[AMMO_SWAP]).key.unwrap()],mouse:vec![]});},
                _=>{app.world_mut().resource_mut::<crate::bindings::Bindings>().device=InputDevice::Controller;},
            }
            app.world_mut().write_message(TouchHotkey(AMMO_SWAP));app.world_mut().write_message(TouchHotkey(usize::MAX));app.update();
            assert!(!app.world().resource::<PlayerControl>().ammo_swap,"gate {gate}");
            app.world_mut().remove_resource::<crate::pad::PlatformInputGate>();app.world_mut().remove_resource::<crate::script_input::ScriptBlocked>();
            app.insert_state(PlayState::Playing);app.world_mut().resource_mut::<crate::bindings::Bindings>().device=InputDevice::KeyboardMouse;
            app.update();assert!(!app.world().resource::<PlayerControl>().ammo_swap,"rejected request replayed after gate {gate}");
        }
    }

}
