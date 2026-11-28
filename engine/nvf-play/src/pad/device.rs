//! Saved device selection on use and consumption of the switching input [design, #2200].

use super::STICK_DEAD;
use crate::bindings::{Bindings, InputDevice};
use bevy::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::input::gamepad::GamepadInput;
use std::{borrow::Cow, hash::Hash};
use std::collections::{HashMap, HashSet};

/// Ordering shared by native/touch backends and the saved device selector.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputSwitchSystems { ReadActivity, Switch }

/// Opt-in touch ownership for Android. Absent on the ordinary desktop path.
#[derive(Resource, Default)]
pub struct TouchDeviceSwitch {
    contacts: HashSet<u64>,
    fresh: bool,
    busy: bool,
    consumed: bool,
    editor_active: bool,
    keys: Vec<KeyCode>,
    mouse: Vec<MouseButton>,
}
impl TouchDeviceSwitch {
    /// Virtual holds (for example touch-only hold-sneak) share switching consumption.
    pub fn allows_actions(&self) -> bool { !self.consumed }
    /// Touch editing owns its input until Save/Cancel; do not switch a background menu.
    pub fn set_editor_active(&mut self, active: bool) { self.editor_active=active; }
    /// The touch backend publishes only its synthesized ownership, never hardware.
    pub fn set_actions(&mut self, keys: impl IntoIterator<Item=KeyCode>, mouse: impl IntoIterator<Item=MouseButton>) {
        self.keys.clear(); self.keys.extend(keys);
        self.mouse.clear(); self.mouse.extend(mouse);
    }
}
fn read_touch(
    mut touches: MessageReader<bevy::input::touch::TouchInput>,
    mut focus: MessageReader<bevy::input::keyboard::KeyboardFocusLost>,
    mut lifecycle: MessageReader<bevy::window::AppLifecycle>,
    touch: Option<ResMut<TouchDeviceSwitch>>,
) {
    let Some(mut state)=touch else {touches.clear();focus.clear();lifecycle.clear();return};
    state.fresh=false; state.consumed=false; state.busy = !state.contacts.is_empty();
    let suspended=lifecycle.read().any(|e|matches!(e,bevy::window::AppLifecycle::Suspended));
    let lost=!focus.is_empty() || suspended;
    focus.clear();
    if lost {state.contacts.clear();state.busy=true;touches.clear();return;}
    for event in touches.read() {
        use bevy::input::touch::TouchPhase;
        match event.phase {
            TouchPhase::Started=>{state.contacts.insert(event.id);state.fresh=true;},
            TouchPhase::Ended|TouchPhase::Canceled=>{state.contacts.remove(&event.id);state.busy=true;},
            TouchPhase::Moved=>{},
        }
    }
}

/// Hardware activity and old-device busy flags; analog activity means a crossing
/// from neutral [design, #2200].
#[derive(Default, Clone, Copy)]
struct Activity {
    key: bool,
    mouse: bool,
    button: bool,
    stick: bool,
    trigger: bool,
    keyboard_busy: bool,
    controller_busy: bool,
}

/// The old device must be idle; simultaneous activity keeps the selected device.
/// Connection/motion is absent. Causes have a stable priority [design, #2200].
fn switch_on_use(current: InputDevice, enabled: bool, a: Activity) -> Option<(InputDevice, &'static str)> {
    let old_busy = match current {
        InputDevice::KeyboardMouse => a.keyboard_busy,
        InputDevice::Controller => a.controller_busy,
    };
    if !enabled || old_busy {
        return None;
    }
    let keyboard = a.key || a.mouse;
    let controller = a.button || a.stick || a.trigger;
    match (current, keyboard, controller) {
        (InputDevice::KeyboardMouse, false, true) => Some((InputDevice::Controller, if a.button { "button" } else if a.stick { "stick" } else { "trigger" })),
        (InputDevice::Controller, true, false) => Some((InputDevice::KeyboardMouse, if a.key { "key" } else { "mouse button" })),
        _ => None,
    }
}

/// Held at frame start or released in this frame: either keeps the old device busy.
/// Fresh presses are handled by the simultaneous-activity rule [design, #2200].
fn digital_busy<T: Copy + Eq + Hash + Send + Sync + 'static>(input: &ButtonInput<T>) -> bool {
    input.get_pressed().any(|b| !input.just_pressed(*b)) || input.get_just_released().next().is_some()
}

fn analog_active(g: &Gamepad) -> [bool; 4] {
    [g.left_stick().length() > STICK_DEAD, g.right_stick().length() > STICK_DEAD,
        g.get(GamepadButton::LeftTrigger2).unwrap_or(0.0) > STICK_DEAD,
        g.get(GamepadButton::RightTrigger2).unwrap_or(0.0) > STICK_DEAD]
}

/// Optional platform-owned cover. Action views are neutral while covered and held
/// hardware remains quarantined through its release/neutral frame. Raw state is intact.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlatformInputSystems { Publish, Remember }

#[derive(Resource, Default)]
pub struct PlatformInputGate {
    active: bool,
    native: bool,
    arm: bool,
    cancel_pending: bool,
    touches: HashSet<u64>,
    keys: HashSet<KeyCode>,
    mouse: HashSet<MouseButton>,
    pads: HashMap<Entity, PlatformPadHold>,
}
#[derive(Default)]
struct PlatformPadHold { buttons: HashSet<GamepadButton>, analog: [bool; 4] }
impl PlatformInputGate {
    pub fn set_blocked(&mut self, blocked: bool) { self.native = blocked; self.update_active(); }
    /// Independent owned arm capture; never changes native loading authority.
    pub fn set_arm_blocked(&mut self, blocked: bool) { self.arm = blocked; self.update_active(); }
    fn update_active(&mut self) {
        let next = self.native || self.arm;
        self.cancel_pending |= next && !self.active;
        self.active = next;
    }
    /// Only loading recovery and an already-owned arm contact may use this narrower view.
    pub fn native_blocked(&self) -> bool { self.native }
    pub fn allows_actions(&self) -> bool { !self.active }
    pub fn allows_touch(&self, id: u64) -> bool { !self.active && !self.touches.contains(&id) }
}
fn publish_platform_gate(
    gate: Option<ResMut<PlatformInputGate>>,
    (keys, mouse, touches): (Res<ButtonInput<KeyCode>>, Res<ButtonInput<MouseButton>>, Res<bevy::input::touch::Touches>), pads: Query<(Entity, &Gamepad)>,
    switching: Option<ResMut<SwitchingInput>>, mut commands: Commands,
) {
    let mut active = false; let mut held_keys = HashSet::new(); let mut held_mouse = HashSet::new();
    if let Some(mut gate) = gate {
        active = gate.active;
        // Before InputSystems: last frame's neutral state releases quarantine, while
        // the following real press may act immediately. Release frames stayed masked.
        if !active {
            gate.touches.retain(|id| touches.get_pressed(*id).is_some());
            gate.keys.retain(|k| keys.pressed(*k)); gate.mouse.retain(|b| mouse.pressed(*b));
            for (entity, raw) in &pads {
                if let Some(held) = gate.pads.get_mut(&entity) {
                    held.buttons.retain(|b| raw.pressed(*b));
                    let now = analog_active(raw); for (i, value) in now.into_iter().enumerate() { held.analog[i] &= value; }
                }
            }
        }
        held_keys.clone_from(&gate.keys); held_mouse.clone_from(&gate.mouse);
        if std::mem::take(&mut gate.cancel_pending) { commands.queue(crate::bindings::cancel_input_holds); }
    }
    if let Some(mut switching) = switching {
        switching.platform_blocked = active; switching.platform_keys = held_keys; switching.platform_mouse = held_mouse;
    }
}
fn remember_platform_holds(
    gate: Option<ResMut<PlatformInputGate>>,
    touches: Res<bevy::input::touch::Touches>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    pads: Query<(Entity, &Gamepad)>,
) {
    let Some(mut gate) = gate else { return };
    let active = gate.active;
    gate.touches.retain(|id| touches.get_pressed(*id).is_some() || touches.just_released(*id) || touches.just_canceled(*id));
    if active { gate.touches.extend(touches.iter().chain(touches.iter_just_pressed()).map(|t| t.id())); }
    gate.keys.retain(|k| keys.pressed(*k) || keys.just_released(*k));
    gate.mouse.retain(|b| mouse.pressed(*b) || mouse.just_released(*b));
    if active {
        gate.keys.extend(keys.get_pressed().copied());
        gate.mouse.extend(mouse.get_pressed().copied());
    }
    gate.pads.retain(|e, _| pads.get(*e).is_ok());
    for (entity, pad) in &pads {
        let now = analog_active(pad);
        let held = gate.pads.entry(entity).or_default();
        held.buttons.retain(|b| pad.pressed(*b) || pad.just_released(*b));
        if active { held.buttons.extend(pad.get_pressed().copied()); }
        for (i, value) in now.into_iter().enumerate() {
            held.analog[i] = if active { held.analog[i] || value } else { held.analog[i] && value };
        }
    }
}

/// The switching press is ignored by actions for one frame; releases and subsequent
/// holds/repeats are always available. No hardware state is masked [design, #2200].
#[derive(Resource, Default)]
pub struct SwitchingInput {
    platform_blocked: bool,
    platform_keys: HashSet<KeyCode>,
    platform_mouse: HashSet<MouseButton>,
    physical_keys: ButtonInput<KeyCode>,
    physical_mouse: ButtonInput<MouseButton>,
    keys: Vec<KeyCode>,
    mouse: Vec<MouseButton>,
    buttons: HashMap<Entity, Vec<GamepadButton>>,
    analog: HashMap<Entity, [bool; 4]>,
    previous: HashMap<Entity, [bool; 4]>,
    previous_buttons: bool,
}

impl SwitchingInput {
    pub(crate) fn physical_key_pressed(&self, key: KeyCode) -> bool { self.physical_keys.pressed(key) }

    /// Hardware/aids only; translated controller menu keys never enter detection
    /// [design, #2200; crate::test_aid::real_presses, super::menu_keys].
    pub(crate) fn keyboard_input(&mut self, key: KeyCode, pressed: bool) {
        if pressed { self.physical_keys.press(key); } else { self.physical_keys.release(key); }
    }

    /// Shared raw-message filter, including text/repeats on the switching frame.
    /// Releases pass even when a press/release pair arrives together [design, #2200].
    pub fn allows(&self, event: &bevy::input::keyboard::KeyboardInput) -> bool {
        !self.platform_blocked && !self.platform_keys.contains(&event.key_code) && (!event.state.is_pressed() || !self.keys.contains(&event.key_code))
    }

    /// Raw picking events use exactly the same press filter [design, #2200].
    pub fn allows_mouse(&self, event: &bevy::input::mouse::MouseButtonInput) -> bool {
        !self.platform_blocked && !self.platform_mouse.contains(&event.button) && (!event.state.is_pressed() || !self.mouse.contains(&event.button))
    }

    /// A derived action view; the original release bits survive unchanged, including
    /// a complete press/release pair in one frame [design, #2200].
    pub fn filter<'a, T: ActionButton>(&self, raw: &'a ButtonInput<T>) -> Cow<'a, ButtonInput<T>> {
        let blocked = T::blocked(self);
        if blocked.is_empty() { return Cow::Borrowed(raw); }
        let mut view = raw.clone();
        for &button in blocked {
            view.reset(button);
            if raw.just_released(button) {
                view.press(button);
                view.release(button);
                view.clear_just_pressed(button);
            }
        }
        Cow::Owned(view)
    }
}

/// Types accepted by the central action filter [design, #2200].
pub trait ActionButton: Copy + Eq + Hash + Send + Sync + 'static {
    fn blocked(filter: &SwitchingInput) -> &[Self];
    fn platform_held(gate: &PlatformInputGate) -> &HashSet<Self>;
    fn physical(filter: &SwitchingInput) -> &ButtonInput<Self>;
}
impl ActionButton for KeyCode {
    fn blocked(filter: &SwitchingInput) -> &[Self] { &filter.keys }
    fn platform_held(gate: &PlatformInputGate) -> &HashSet<Self> { &gate.keys }
    fn physical(filter: &SwitchingInput) -> &ButtonInput<Self> { &filter.physical_keys }
}
impl ActionButton for MouseButton {
    fn blocked(filter: &SwitchingInput) -> &[Self] { &filter.mouse }
    fn platform_held(gate: &PlatformInputGate) -> &HashSet<Self> { &gate.mouse }
    fn physical(filter: &SwitchingInput) -> &ButtonInput<Self> { &filter.physical_mouse }
}

/// Action readers use this parameter instead of raw ButtonInput. Tests/tools without
/// the controller plugin retain ordinary Bevy input [design, #2200].
#[derive(SystemParam)]
pub struct ActionButtons<'w, T: ActionButton> {
    raw: Res<'w, ButtonInput<T>>,
    filter: Option<Res<'w, SwitchingInput>>,
    platform: Option<Res<'w, PlatformInputGate>>,
}
impl<T: ActionButton> ActionButtons<'_, T> {
    pub fn snapshot(&self) -> Cow<'_, ButtonInput<T>> {
        if self.platform.as_ref().is_some_and(|g| g.active) { return Cow::Owned(ButtonInput::default()); }
        let filtered = self.filter.as_ref().map_or_else(|| Cow::Borrowed(&*self.raw), |f| f.filter(&self.raw));
        if let Some(gate) = self.platform.as_ref().filter(|g| !T::platform_held(g).is_empty()) {
            let mut view = filtered.into_owned();
            for &button in T::platform_held(gate) { view.reset(button); }
            Cow::Owned(view)
        } else { filtered }
    }
    /// Raw provenance captured by the existing selector, still through action gates.
    /// Standalone consumers without the selector retain the historical input view.
    pub(crate) fn physical_pressed(&self, b:T)->bool {
        !self.blocked(b) && self.filter.as_ref().map_or_else(||self.raw.pressed(b),|f|T::physical(f).pressed(b))
    }
    pub(crate) fn physical_just_pressed(&self, b:T)->bool {
        !self.blocked(b) && self.filter.as_ref().map_or_else(||self.raw.just_pressed(b),|f|T::physical(f).just_pressed(b))
    }
    pub(crate) fn physical_just_released(&self, b:T)->bool {
        !self.platform_blocked(b) && self.filter.as_ref().map_or_else(||self.raw.just_released(b),|f|T::physical(f).just_released(b))
    }
    pub fn pressed(&self, b: T) -> bool { self.raw.pressed(b) && !self.blocked(b) }
    pub fn just_pressed(&self, b: T) -> bool { self.raw.just_pressed(b) && !self.blocked(b) }
    pub fn just_released(&self, b: T) -> bool { self.raw.just_released(b) && !self.platform_blocked(b) }
    fn platform_blocked(&self, b: T) -> bool { self.platform.as_ref().is_some_and(|g| g.active || T::platform_held(g).contains(&b)) }
    fn blocked(&self, b: T) -> bool { self.platform_blocked(b) || self.filter.as_ref().is_some_and(|f| T::blocked(f).contains(&b)) }
    pub fn any_pressed(&self, buttons: impl IntoIterator<Item = T>) -> bool { buttons.into_iter().any(|b| self.pressed(b)) }
    pub fn any_just_pressed(&self, buttons: impl IntoIterator<Item = T>) -> bool { buttons.into_iter().any(|b| self.just_pressed(b)) }
    pub fn get_pressed(&self) -> impl Iterator<Item = &T> { self.raw.get_pressed().filter(|b| !self.blocked(**b)) }
    pub fn get_just_pressed(&self) -> impl Iterator<Item = &T> { self.raw.get_just_pressed().filter(|b| !self.blocked(**b)) }
    pub fn get_just_released(&self) -> impl Iterator<Item = &T> { self.raw.get_just_released().filter(|b| !self.platform_blocked(**b)) }
}

/// Pad action view, using the same filter as keys/mouse [design, #2200].
#[derive(SystemParam)]
pub struct ActionPads<'w, 's> {
    raw: Query<'w, 's, (Entity, &'static Gamepad)>,
    aim: Option<Res<'w, crate::controller_aim::Options>>,
    filter: Option<Res<'w, SwitchingInput>>,
    platform: Option<Res<'w, PlatformInputGate>>,
}
impl ActionPads<'_, '_> {
    /// Explicit per-stick Tweaks override the saved generic preference only while on.
    /// Disabling restores that preference without rewriting either setting.
    pub(crate) fn dead_zone(&self, left: bool) -> f32 {
        super::custom_dead_zone(left)
            .unwrap_or_else(||self.aim.as_deref().map_or(super::STICK_DEAD, |o|o.dead_zone))
    }
    /// Filter current stick activity while retaining physical previous edges
    /// [design, #2200; super::update_stick_buttons].
    pub fn filter_sticks(&self, raw: &super::StickButtons) -> super::StickButtons {
        let v = super::stick(self, true);
        super::StickButtons {
            now: std::array::from_fn(|i| raw.now[i] && super::StickDir::ALL[i].amount(v) >= super::STICK_PRESS),
            before: raw.before,
        }
    }

    /// Physical history uses the same normalization as actions [design, #2200; super::stick].
    pub(crate) fn physical_stick(&self, left: bool) -> Vec2 {
        super::stick_vector_at(self.raw.iter().map(|(_, g)| if left { g.left_stick() } else { g.right_stick() }), self.dead_zone(left))
    }

    pub fn iter(&self) -> impl Iterator<Item = ActionPad<'_>> {
        self.raw.iter().map(|(entity, raw)| ActionPad { entity, raw, filter: self.filter.as_deref(), platform: self.platform.as_deref() })
    }
}
pub struct ActionPad<'a> {
    entity: Entity,
    raw: &'a Gamepad,
    filter: Option<&'a SwitchingInput>,
    platform: Option<&'a PlatformInputGate>,
}
impl ActionPad<'_> {
    fn platform_blocked(&self, b: GamepadButton) -> bool { self.platform.is_some_and(|g| g.active || g.pads.get(&self.entity).is_some_and(|p| p.buttons.contains(&b))) }
    fn blocked(&self, b: GamepadButton) -> bool { self.platform_blocked(b) || self.filter.and_then(|f| f.buttons.get(&self.entity)).is_some_and(|v| v.contains(&b)) }
    pub fn pressed(&self, b: GamepadButton) -> bool { self.raw.pressed(b) && !self.blocked(b) }
    pub fn just_pressed(&self, b: GamepadButton) -> bool { self.raw.just_pressed(b) && !self.blocked(b) }
    pub fn just_released(&self, b: GamepadButton) -> bool { self.raw.just_released(b) && !self.platform_blocked(b) }
    fn analog_blocked(&self, i: usize) -> bool { self.platform.is_some_and(|g| g.active || g.pads.get(&self.entity).is_some_and(|p| p.analog[i])) || self.filter.and_then(|f| f.analog.get(&self.entity)).is_some_and(|a| a[i]) }
    pub fn left_stick(&self) -> Vec2 { if self.analog_blocked(0) { Vec2::ZERO } else { self.raw.left_stick() } }
    pub fn right_stick(&self) -> Vec2 { if self.analog_blocked(1) { Vec2::ZERO } else { self.raw.right_stick() } }
    pub fn get(&self, input: impl Into<GamepadInput>) -> Option<f32> {
        let input = input.into();
        if self.platform.is_some_and(|g| g.active) { return Some(0.0); }
        let blocked = match input {
            GamepadInput::Button(b) => self.blocked(b) || match b {
                GamepadButton::LeftTrigger2 => self.analog_blocked(2),
                GamepadButton::RightTrigger2 => self.analog_blocked(3),
                _ => false,
            },
            GamepadInput::Axis(GamepadAxis::LeftStickX | GamepadAxis::LeftStickY) => self.analog_blocked(0),
            GamepadInput::Axis(GamepadAxis::RightStickX | GamepadAxis::RightStickY) => self.analog_blocked(1),
            _ => false,
        };
        if blocked { Some(0.0) } else { self.raw.get(input) }
    }
}

/// Capture physical input independently of menu translation, mirroring focus loss
/// from Bevy's keyboard_input_system [design, #2200; bevy_input/keyboard.rs].
pub(crate) fn read_keyboard(
    mut events: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut mouse: MessageReader<bevy::input::mouse::MouseButtonInput>,
    mut focus_lost: MessageReader<bevy::input::keyboard::KeyboardFocusLost>,
    mut held: ResMut<SwitchingInput>,
) {
    held.physical_keys.clear();
    held.physical_mouse.clear();
    for event in events.read() { held.keyboard_input(event.key_code, event.state.is_pressed()); }
    for event in mouse.read() {
        if event.state.is_pressed() { held.physical_mouse.press(event.button); }
        else { held.physical_mouse.release(event.button); }
    }
    if !focus_lost.is_empty() {
        held.physical_keys.release_all();
        held.physical_mouse.release_all();
        focus_lost.clear();
    }
}

/// Runs once after hardware/aids and before action readers. Only this frame's fresh
/// incoming edges enter the filter; releases never do [design, #2200].
pub(crate) fn automatic(world: &mut World) {
    let Some(b) = world.get_resource::<Bindings>() else { return };
    let (current, enabled) = (b.device, b.automatic_input_switching);
    let listening = world.get_resource::<crate::pause::Listening>().is_some_and(|l| l.0.is_some());
    let mut held = world.remove_resource::<SwitchingInput>().unwrap_or_default();
    held.keys.clear(); held.mouse.clear(); held.buttons.clear(); held.analog.clear();
    // A touch aim latch may keep the aggregate mouse binding down after the finger
    // lifts. Only hardware mouse ownership and actual touch contacts keep touch busy.
    // Preserve the existing aggregate path when touch ownership is not installed.
    let physical_mouse = if world.contains_resource::<TouchDeviceSwitch>() {
        &held.physical_mouse
    } else {
        world.resource::<ButtonInput<MouseButton>>()
    };
    let mut activity = Activity {
        key: held.physical_keys.get_just_pressed().next().is_some(),
        mouse: physical_mouse.get_just_pressed().next().is_some(),
        keyboard_busy: digital_busy(&held.physical_keys) || digital_busy(physical_mouse),
        // Include pads removed this frame: disconnect must not split a pending hold
        // from the old device's readers [design, #2200].
        controller_busy: held.previous_buttons || held.previous.values().flatten().any(|active| *active),
        ..default()
    };
    let mut pads = world.query::<(Entity, &Gamepad)>();
    let no_pads=pads.iter(world).next().is_none();
    let (touch_fresh,touch_busy,touch_held)=world.get_resource::<TouchDeviceSwitch>().map_or((false,false,false),|t|(t.fresh,t.busy||t.editor_active,!t.contacts.is_empty()));
    // A saved Controller choice cannot strand touch controls after the pad is gone.
    let touch_fallback=current==InputDevice::Controller && no_pads && touch_held;
    activity.key|=touch_fresh || touch_fallback;
    activity.keyboard_busy|=touch_busy;
    let mut previous = HashMap::new();
    let mut buttons_down = false;
    for (e, g) in pads.iter(world) {
        let now = analog_active(g);
        let before = held.previous.get(&e).copied().unwrap_or_default();
        activity.controller_busy |= digital_busy(g.digital());
        buttons_down |= g.get_pressed().next().is_some();
        activity.button |= g.get_just_pressed().next().is_some();
        activity.stick |= (0..2).any(|i| now[i] && !before[i]);
        activity.trigger |= (2..4).any(|i| now[i] && !before[i]);
        previous.insert(e, now);
    }
    // On Android (or when touch/platform gating is installed), Stewie's explicit A/left-click
    // choice uses this selector and consumes its edge. Desktop keeps tweaks.rs::toggle_device.
    let selector_tweak = cfg!(target_os = "android")
        || world.contains_resource::<TouchDeviceSwitch>()
        || world.contains_resource::<PlatformInputGate>();
    let tweak = selector_tweak && world.get_resource::<crate::tweaks::TweakSettings>().is_some_and(|t| t.toggle_device);
    let gate = world.get_resource::<PlatformInputGate>();
    let tweak_switch = tweak && !held.platform_blocked && gate.is_none_or(|g|!g.active) && match current {
        InputDevice::KeyboardMouse => pads.iter(world).any(|(e,g)|g.just_pressed(GamepadButton::South)
            && gate.is_none_or(|p|!p.pads.get(&e).is_some_and(|h|h.buttons.contains(&GamepadButton::South)))),
        InputDevice::Controller => held.physical_mouse.just_pressed(MouseButton::Left)
            && !held.platform_mouse.contains(&MouseButton::Left),
    };
    let switch = switch_on_use(current, (enabled || touch_fallback || tweak_switch) && !listening, activity);
    if let Some((device, cause)) = switch {
        crate::bindings::set_device(world, device);
        info!("controller: input device switched to {} ({cause})", device.name());
        if device == InputDevice::KeyboardMouse {
            held.keys.extend(held.physical_keys.get_just_pressed().copied());
            held.mouse.extend(world.resource::<ButtonInput<MouseButton>>().get_just_pressed().copied());
            if touch_fresh || touch_fallback {
                if let Some(mut touch)=world.get_resource_mut::<TouchDeviceSwitch>() {
                    held.keys.extend(touch.keys.iter().copied());
                    held.mouse.extend(touch.mouse.iter().copied());
                    touch.consumed=true;
                }
            }
        } else {
            for (e, g) in pads.iter(world) {
                held.buttons.insert(e, g.get_just_pressed().copied().collect());
                let now = previous[&e];
                let before = held.previous.get(&e).copied().unwrap_or_default();
                held.analog.insert(e, std::array::from_fn(|i| now[i] && !before[i]));
            }
        }
    }
    held.previous = previous;
    held.previous_buttons = buttons_down;
    world.insert_resource(held);
}

/// Hardware selection/filter schedule, shared by PadPlugin and headless readers
/// [design, #2200].
pub struct InputSwitchPlugin;
impl Plugin for InputSwitchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SwitchingInput>()
            .add_systems(PreUpdate, publish_platform_gate.in_set(PlatformInputSystems::Publish).before(bevy::input::InputSystems))
            .add_systems(PreUpdate, remember_platform_holds.in_set(PlatformInputSystems::Remember).after(InputSwitchSystems::Switch)
                .after(crate::pad::menu_keys).before(bevy::ui::UiSystems::Focus))
            .add_message::<bevy::window::AppLifecycle>()
            .add_systems(PreUpdate, (read_keyboard, read_touch).chain()
                .in_set(InputSwitchSystems::ReadActivity)
                .after(bevy::input::InputSystems).before(crate::test_aid::real_presses))
            .add_systems(PreUpdate, automatic.in_set(InputSwitchSystems::Switch)
                .after(InputSwitchSystems::ReadActivity).after(crate::test_aid::real_presses)
                .before(crate::full_screen_key::read).before(bevy::ui::UiSystems::Focus)
                .before(bevy::picking::PickingSystems::ProcessInput));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    /// A race fixture can sample the real pre-existing held-touch state between
    /// raw ingress and InputSystems, then reopen global admission before DATA.
    pub(crate) fn sample_platform_holds_now(world:&mut World) {
        use bevy::ecs::system::RunSystemOnce;
        world.run_system_once(super::remember_platform_holds).unwrap();
    }

    use super::*;

    fn app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin, InputSwitchPlugin));
        app.init_resource::<Bindings>().init_resource::<super::super::MenuKeys>();
        app.insert_resource(crate::pause::Paused(true));
        app.add_systems(PreUpdate, super::super::menu_keys.after(automatic));
        let pad = app.world_mut().spawn(Gamepad::default()).id();
        (app, pad)
    }

    pub(crate) fn button(app: &mut App, pad: Entity, down: bool) {
        use bevy::input::gamepad::{RawGamepadButtonChangedEvent, RawGamepadEvent};
        app.world_mut().write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(pad, GamepadButton::South, if down { 1.0 } else { 0.0 })));
    }

    fn key(app: &mut App, down: bool, repeat: bool) {
        keyboard(app, KeyCode::Enter, down, repeat);
    }

    fn keyboard(app: &mut App, code: KeyCode, down: bool, repeat: bool) {
        use bevy::input::{keyboard::{Key, KeyboardInput}, ButtonState};
        app.world_mut().write_message(KeyboardInput {
            key_code: code, logical_key: match code {
                KeyCode::Enter => Key::Enter,
                KeyCode::ArrowRight => Key::ArrowRight,
                KeyCode::AltLeft | KeyCode::AltRight => Key::Alt,
                _ => Key::Character("x".into()),
            },
            state: if down { ButtonState::Pressed } else { ButtonState::Released },
            text: None, repeat, window: Entity::PLACEHOLDER,
        });
    }

    pub(crate) fn playing_app(device: InputDevice) -> (App, Entity) {
        let (mut app, pad) = app();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .insert_state(crate::PlayState::Playing)
            .init_resource::<crate::control::PlayerControl>()
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(50)))
            .add_systems(Update, crate::control::read_controls);
        app.world_mut().resource_mut::<Bindings>().device = device;
        app.world_mut().resource_mut::<crate::pause::Paused>().0 = false;
        (app, pad)
    }

    pub(crate) fn control_button(app: &mut App, pad: Entity, device: InputDevice, control: crate::bindings::Control, down: bool) {
        use bevy::input::{gamepad::{RawGamepadButtonChangedEvent, RawGamepadEvent}, mouse::MouseButtonInput, ButtonState};
        let bindings = app.world().resource::<Bindings>();
        if device == InputDevice::Controller {
            let Some(super::super::PadInput::Button(b)) = bindings.pad(control) else { panic!("test needs a pad button") };
            app.world_mut().write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(pad, b, if down { 1.0 } else { 0.0 })));
        } else {
            let binding = bindings.get(control);
            if let Some(code) = binding.key {
                keyboard(app, code, down, false);
            } else {
                app.world_mut().write_message(MouseButtonInput {
                    button: binding.mouse.expect("test needs a mouse button"),
                    state: if down { ButtonState::Pressed } else { ButtonState::Released }, window: Entity::PLACEHOLDER,
                });
            }
        }
    }

    fn switch_from(app: &mut App, pad: Entity, device: InputDevice, down: bool) {
        if device == InputDevice::KeyboardMouse {
            button(app, pad, down);
        } else {
            keyboard(app, KeyCode::KeyX, down, false);
        }
    }

    fn manual_switch_from(app: &mut App, pad: Entity, device: InputDevice) {
        switch_from(app, pad, device, true);
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, device, "a held old device refuses automatic switching");
        let target = if device == InputDevice::KeyboardMouse { InputDevice::Controller } else { InputDevice::KeyboardMouse };
        crate::bindings::set_device(app.world_mut(), target);
    }

    #[test]
    fn manual_switching_cancels_held_orbit_without_a_pov_tap_and_fresh_taps_still_work() {
        use crate::{bindings::Control, control::{PlayerControl, Pov}};
        for origin in [InputDevice::KeyboardMouse, InputDevice::Controller] {
            for moved in [false, true] {
                let (mut app, pad) = playing_app(origin);
                control_button(&mut app, pad, origin, Control::TogglePov, true);
                app.update();
                assert!(app.world().resource::<PlayerControl>().orbit.held);
                {
                    let mut c = app.world_mut().resource_mut::<PlayerControl>();
                    c.orbit.moved = moved;
                    c.orbit.yaw = if moved { 0.7 } else { 0.0 };
                }
                manual_switch_from(&mut app, pad, origin);
                let target = app.world().resource::<Bindings>().device;
                assert_ne!(target, origin);
                let c = app.world().resource::<PlayerControl>();
                assert!(!c.orbit.held && !c.orbit.moved);
                assert_eq!(c.pov, Pov::First);
                assert_eq!(c.orbit.yaw, if moved { 0.7 } else { 0.0 });
                control_button(&mut app, pad, origin, Control::TogglePov, false);
                switch_from(&mut app, pad, origin, false);
                app.update();
                if target == InputDevice::Controller {
                    app.world_mut().get_mut::<Gamepad>(pad).unwrap().analog_mut().set(GamepadAxis::RightStickX, 0.7);
                    app.update();
                    assert!(!app.world().resource::<PlayerControl>().orbit.held);
                    assert_eq!(app.world().resource::<PlayerControl>().pov, Pov::First);
                    app.world_mut().get_mut::<Gamepad>(pad).unwrap().analog_mut().set(GamepadAxis::RightStickX, 0.0);
                }
                control_button(&mut app, pad, target, Control::TogglePov, true);
                app.update();
                assert!(app.world().resource::<PlayerControl>().orbit.held);
                control_button(&mut app, pad, target, Control::TogglePov, false);
                app.update();
                assert_eq!(app.world().resource::<PlayerControl>().pov, Pov::Third);
            }
        }
    }

    #[test]
    fn manual_switching_cancels_ready_item_without_reloading_or_holstering() {
        use crate::{bindings::Control, control::PlayerControl};
        for origin in [InputDevice::KeyboardMouse, InputDevice::Controller] {
            let (mut app, pad) = playing_app(origin);
            app.world_mut().resource_mut::<PlayerControl>().weapon_out = true;
            control_button(&mut app, pad, origin, Control::ReadyItem, true);
            app.update();
            assert!(app.world().resource::<PlayerControl>().ready_held.is_some());
            manual_switch_from(&mut app, pad, origin);
            control_button(&mut app, pad, origin, Control::ReadyItem, false);
            switch_from(&mut app, pad, origin, false);
            for _ in 0..15 {
                app.update();
                let c = app.world().resource::<PlayerControl>();
                assert!(c.ready_held.is_none() && c.weapon_out && !c.reload);
            }
            let target = app.world().resource::<Bindings>().device;
            control_button(&mut app, pad, target, Control::ReadyItem, true);
            app.update();
            control_button(&mut app, pad, target, Control::ReadyItem, false);
            app.update();
            assert!(app.world().resource::<PlayerControl>().reload);
        }
    }

    #[test]
    fn switching_ends_aim_block_and_attack_holds_and_allows_fresh_actions() {
        use crate::{bindings::Control, control::PlayerControl};
        for origin in [InputDevice::KeyboardMouse, InputDevice::Controller] {
            for action in [Control::Block, Control::Use] {
                let (mut app, pad) = playing_app(origin);
                app.world_mut().resource_mut::<PlayerControl>().weapon_out = true;
                control_button(&mut app, pad, origin, action, true);
                app.update();
                let c = app.world().resource::<PlayerControl>();
                assert!(if action == Control::Block { c.aiming } else { c.trigger });
                manual_switch_from(&mut app, pad, origin);
                control_button(&mut app, pad, origin, action, false);
                switch_from(&mut app, pad, origin, false);
                app.update();
                let c = app.world().resource::<PlayerControl>();
                assert!(!c.aiming && !c.trigger);
                let target = app.world().resource::<Bindings>().device;
                control_button(&mut app, pad, target, action, true);
                app.update();
                let c = app.world().resource::<PlayerControl>();
                assert!(if action == Control::Block { c.aiming } else { c.trigger });
            }
        }
    }

    #[test]
    fn switching_preserves_sneak_and_run_toggles_and_tracks_old_key_releases() {
        use crate::{bindings::Control, control::PlayerControl};
        let (mut app, pad) = playing_app(InputDevice::KeyboardMouse);
        for action in [Control::CrouchSneak, Control::AlwaysRun, Control::AutoMove, Control::Run] {
            control_button(&mut app, pad, InputDevice::KeyboardMouse, action, true);
        }
        app.update();
        assert!(app.world().resource::<PlayerControl>().sneak);
        let always_run = app.world().resource::<PlayerControl>().always_run;
        assert!(app.world().resource::<PlayerControl>().auto_move);
        button(&mut app, pad, true);
        app.update();
        for action in [Control::CrouchSneak, Control::AlwaysRun, Control::AutoMove, Control::Run] {
            control_button(&mut app, pad, InputDevice::KeyboardMouse, action, false);
        }
        button(&mut app, pad, false);
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
        button(&mut app, pad, true);
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller);
        assert!(app.world().resource::<SwitchingInput>().physical_keys.get_pressed().next().is_none());
        assert!(app.world().resource::<ButtonInput<KeyCode>>().get_pressed().next().is_none());
        let c = app.world().resource::<PlayerControl>();
        assert!(c.sneak && c.auto_move);
        assert_eq!(c.always_run, always_run);
    }

    #[test]
    fn manual_device_change_cancels_holds_but_same_device_assignment_preserves_them() {
        use crate::control::{Orbit, PlayerControl};
        let (mut app, _) = playing_app(InputDevice::KeyboardMouse);
        let before = PlayerControl {
            orbit: Orbit { held: true, moved: true, yaw: 0.7, pitch: 0.2, free: true },
            ready_held: Some(0.2), aiming: true, trigger: true, sneak: true, auto_move: true,
            ..default()
        };
        app.world_mut().insert_resource(before);
        crate::bindings::set_device(app.world_mut(), InputDevice::KeyboardMouse);
        assert_eq!(*app.world().resource::<PlayerControl>(), before);
        crate::bindings::set_device(app.world_mut(), InputDevice::Controller);
        assert_eq!(*app.world().resource::<PlayerControl>(), PlayerControl {
            orbit: Orbit { held: false, moved: false, ..before.orbit },
            ready_held: None, aiming: false, trigger: false, ..before
        });
    }

    #[derive(Resource, Default)]
    struct Seen {
        keys: ButtonInput<KeyCode>,
        mouse: ButtonInput<MouseButton>,
        pad: [bool; 3],
        stick: Vec2,
        repeats: Vec<bool>,
    }

    fn see(keys: ActionButtons<KeyCode>, mouse: ActionButtons<MouseButton>, pads: ActionPads,
        mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>, filter: Res<SwitchingInput>, mut seen: ResMut<Seen>) {
        seen.keys = keys.snapshot().into_owned();
        seen.mouse = mouse.snapshot().into_owned();
        if let Some(g) = pads.iter().next() {
            seen.pad = [g.pressed(GamepadButton::South), g.just_pressed(GamepadButton::South), g.just_released(GamepadButton::South)];
            seen.stick = g.left_stick();
        }
        let typed: Vec<_> = typed.read().filter(|e| filter.allows(e)).cloned().collect();
        let repeats = super::super::sideways(&typed, &seen.keys);
        seen.repeats.extend(repeats);
    }

    fn observed_app(device: InputDevice) -> (App, Entity) {
        let (mut app, pad) = app();
        app.world_mut().resource_mut::<Bindings>().device = device;
        app.init_resource::<Seen>().add_systems(Update, see);
        (app, pad)
    }

    #[test]
    fn switching_filters_only_the_fresh_edge_and_always_delivers_releases() {
        let (mut app, pad) = observed_app(InputDevice::KeyboardMouse);
        button(&mut app, pad, true);
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller);
        assert_eq!(app.world().resource::<Seen>().pad, [false; 3]);
        assert!(app.world().get::<Gamepad>(pad).unwrap().pressed(GamepadButton::South));
        assert!(!app.world().resource::<Seen>().keys.just_pressed(KeyCode::Enter));
        app.update();
        assert_eq!(app.world().resource::<Seen>().pad, [true, false, false]);
        button(&mut app, pad, false);
        key(&mut app, true, false);
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller);
        assert_eq!(app.world().resource::<Seen>().pad, [false, false, true]);
        key(&mut app, false, false);
        app.update();
        key(&mut app, true, false);
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
        let seen = app.world().resource::<Seen>();
        assert!(!seen.keys.just_pressed(KeyCode::Enter));
        key(&mut app, true, true);
        app.update();
        assert!(app.world().resource::<Seen>().keys.pressed(KeyCode::Enter));
        key(&mut app, false, false);
        app.update();
        assert!(app.world().resource::<Seen>().keys.just_released(KeyCode::Enter));
        key(&mut app, true, false);
        app.update();
        assert!(app.world().resource::<Seen>().keys.just_pressed(KeyCode::Enter));
    }

    #[test]
    fn switching_filters_initial_menu_adjustment_but_later_repeats_work() {
        let (mut app, _) = observed_app(InputDevice::Controller);
        keyboard(&mut app, KeyCode::ArrowRight, true, false);
        app.update();
        assert!(app.world().resource::<Seen>().repeats.is_empty());
        keyboard(&mut app, KeyCode::ArrowRight, true, true);
        app.update();
        keyboard(&mut app, KeyCode::ArrowRight, false, false);
        app.update();
        assert_eq!(app.world().resource::<Seen>().repeats, vec![true]);
        assert!(app.world().resource::<Seen>().keys.just_released(KeyCode::ArrowRight));
    }

    #[test]
    fn switching_from_held_menu_a_releases_synthetic_enter_and_a_fresh_enter_works() {
        let (mut app, pad) = observed_app(InputDevice::Controller);
        button(&mut app, pad, true);
        app.update();
        assert!(app.world().resource::<Seen>().keys.pressed(KeyCode::Enter));
        keyboard(&mut app, KeyCode::KeyX, true, false);
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller);
        crate::bindings::set_device(app.world_mut(), InputDevice::KeyboardMouse);
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
        assert!(app.world().resource::<super::super::MenuKeys>().held.is_empty());
        assert!(!app.world().resource::<Seen>().keys.pressed(KeyCode::Enter));
        assert!(app.world().resource::<Seen>().keys.just_released(KeyCode::Enter));
        button(&mut app, pad, false);
        keyboard(&mut app, KeyCode::KeyX, false, false);
        app.update();
        key(&mut app, true, false);
        app.update();
        assert!(app.world().resource::<Seen>().keys.just_pressed(KeyCode::Enter));
    }

    #[test]
    fn physical_enter_sharing_a_synthetic_enter_keeps_its_hold_and_release() {
        let (mut app, pad) = observed_app(InputDevice::Controller);
        button(&mut app, pad, true);
        app.update();
        key(&mut app, true, false);
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller);
        crate::bindings::set_device(app.world_mut(), InputDevice::KeyboardMouse);
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
        assert!(app.world().resource::<Seen>().keys.pressed(KeyCode::Enter));
        button(&mut app, pad, false);
        app.update();
        assert!(app.world().resource::<Seen>().keys.pressed(KeyCode::Enter));
        key(&mut app, false, false);
        app.update();
        assert!(app.world().resource::<Seen>().keys.just_released(KeyCode::Enter));
        key(&mut app, true, false);
        app.update();
        assert!(app.world().resource::<Seen>().keys.just_pressed(KeyCode::Enter));
    }

    #[test]
    fn alt_release_after_two_switches_leaves_plain_enter_for_the_menu() {
        let (mut app, pad) = observed_app(InputDevice::KeyboardMouse);
        app.init_resource::<crate::full_screen_key::Alts>().init_resource::<crate::full_screen_key::Asked>();
        app.add_systems(PreUpdate, crate::full_screen_key::read.after(automatic).before(super::super::menu_keys));
        for alt in [KeyCode::AltLeft, KeyCode::AltRight] {
            keyboard(&mut app, alt, true, false);
            app.update();
            button(&mut app, pad, true);
            app.update();
            assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
            crate::bindings::set_device(app.world_mut(), InputDevice::Controller);
            button(&mut app, pad, false);
            app.update();
            keyboard(&mut app, KeyCode::KeyX, true, false);
            app.update();
            assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
            keyboard(&mut app, alt, false, false);
            keyboard(&mut app, KeyCode::KeyX, false, false);
            app.update();
            assert!(app.world().resource::<Seen>().keys.just_released(alt));
            key(&mut app, true, false);
            app.update();
            assert!(app.world().resource::<Seen>().keys.just_pressed(KeyCode::Enter));
            key(&mut app, false, false);
            app.update();
        }
    }

    #[test]
    fn focus_loss_clears_physical_detection_for_the_next_press() {
        let (mut app, _) = observed_app(InputDevice::Controller);
        key(&mut app, true, false);
        app.update();
        app.world_mut().write_message(bevy::input::keyboard::KeyboardFocusLost);
        app.update();
        assert!(app.world().resource::<Seen>().keys.just_released(KeyCode::Enter));
        assert!(app.world().resource::<SwitchingInput>().physical_keys.get_pressed().next().is_none());
        key(&mut app, true, false);
        app.update();
        assert!(app.world().resource::<Seen>().keys.just_pressed(KeyCode::Enter));
    }

    #[test]
    fn diagonal_stick_thresholds_keep_normalization_with_switching_off() {
        use crate::bindings::{Control, ControlInput};
        #[derive(Resource, Default)]
        struct Direction([bool; 3]);
        fn read(input: ControlInput, mut seen: ResMut<Direction>) {
            let sticks = input.stick_buttons().unwrap();
            seen.0 = [input.pressed(Control::SlideRight), input.just_pressed(Control::SlideRight), sticks.just_pressed(super::super::StickDir::Right)];
        }
        let (mut app, pad) = observed_app(InputDevice::Controller);
        app.world_mut().resource_mut::<Bindings>().automatic_input_switching = false;
        app.init_resource::<super::super::StickButtons>().init_resource::<Direction>()
            .add_systems(PreUpdate, super::super::update_stick_buttons.after(automatic))
            .add_systems(Update, read);
        app.world_mut().get_mut::<Gamepad>(pad).unwrap().analog_mut().set(GamepadAxis::LeftStickY, 1.0);
        app.update();
        assert_eq!(app.world().resource::<Direction>().0, [false; 3]);
        app.world_mut().get_mut::<Gamepad>(pad).unwrap().analog_mut().set(GamepadAxis::LeftStickX, 0.55);
        app.update();
        assert_eq!(app.world().resource::<Direction>().0, [false; 3], "the normalized horizontal component remains below STICK_PRESS");
        app.update();
        assert_eq!(app.world().resource::<Direction>().0, [false; 3]);
        app.world_mut().get_mut::<Gamepad>(pad).unwrap().analog_mut().set(GamepadAxis::LeftStickX, 0.8);
        app.update();
        assert_eq!(app.world().resource::<Direction>().0, [true; 3]);
        app.update();
        assert_eq!(app.world().resource::<Direction>().0, [true, false, false]);
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller);
    }

    #[test]
    fn a_switching_stick_crossing_does_not_become_a_delayed_press() {
        use crate::bindings::{Control, ControlInput};
        #[derive(Resource, Default)]
        struct Direction([bool; 3]);
        fn read(input: ControlInput, mut seen: ResMut<Direction>) {
            seen.0 = [input.pressed(Control::Forward), input.just_pressed(Control::Forward), input.just_released(Control::Forward)];
        }
        let (mut app, pad) = observed_app(InputDevice::KeyboardMouse);
        app.init_resource::<super::super::StickButtons>().init_resource::<Direction>()
            .add_systems(PreUpdate, super::super::update_stick_buttons.after(automatic))
            .add_systems(Update, read);
        app.world_mut().get_mut::<Gamepad>(pad).unwrap().analog_mut().set(GamepadAxis::LeftStickY, 0.8);
        app.update();
        assert_eq!(app.world().resource::<Direction>().0, [false; 3]);
        app.update();
        assert_eq!(app.world().resource::<Direction>().0, [true, false, false]);
        keyboard(&mut app, KeyCode::KeyX, true, false);
        app.update();
        app.world_mut().get_mut::<Gamepad>(pad).unwrap().analog_mut().set(GamepadAxis::LeftStickY, 0.0);
        app.update();
        assert_eq!(app.world().resource::<Direction>().0, [false, false, true]);
    }

    #[test]
    fn analog_crossing_filters_one_frame_and_a_held_stick_does_not_steal_back() {
        let (mut app, pad) = observed_app(InputDevice::KeyboardMouse);
        app.update();
        for input in [GamepadInput::Axis(GamepadAxis::LeftStickX), GamepadAxis::RightStickY.into(),
            GamepadButton::LeftTrigger2.into(), GamepadButton::RightTrigger2.into()] {
            app.world_mut().resource_mut::<Bindings>().device = InputDevice::KeyboardMouse;
            app.world_mut().get_mut::<Gamepad>(pad).unwrap().analog_mut().set(input, STICK_DEAD);
            app.update();
            assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
            app.world_mut().get_mut::<Gamepad>(pad).unwrap().analog_mut().set(input, STICK_DEAD + 0.01);
            app.update();
            assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller);
            assert_eq!(app.world().get::<Gamepad>(pad).unwrap().get(input), Some(STICK_DEAD + 0.01));
            assert!(app.world().resource::<SwitchingInput>().analog[&pad].into_iter().any(|b| b));
            key(&mut app, true, false);
            app.update();
            app.update();
            assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller);
            app.world_mut().get_mut::<Gamepad>(pad).unwrap().analog_mut().set(input, 0.0);
            app.update();
            assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller);
            key(&mut app, false, false);
            app.update();
            key(&mut app, true, false);
            app.update();
            assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
            assert!(app.world().resource::<SwitchingInput>().analog.is_empty());
            key(&mut app, false, false);
            app.world_mut().get_mut::<Gamepad>(pad).unwrap().analog_mut().set(input, 0.0);
            app.update();
        }
    }

    #[test]
    fn connection_rebinding_disabled_policy_and_simultaneous_input_do_not_switch() {
        let (mut app, pad) = observed_app(InputDevice::KeyboardMouse);
        app.update();
        button(&mut app, pad, true);
        key(&mut app, true, false);
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
        button(&mut app, pad, false);
        key(&mut app, false, false);
        app.update();
        app.world_mut().resource_mut::<Bindings>().automatic_input_switching = false;
        button(&mut app, pad, true);
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
        app.world_mut().resource_mut::<Bindings>().automatic_input_switching = true;
        app.world_mut().insert_resource(crate::pause::Listening(Some((crate::bindings::Control::Jump, InputDevice::KeyboardMouse))));
        button(&mut app, pad, false);
        app.update();
        button(&mut app, pad, true);
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
    }

    #[test]
    fn mouse_motion_does_not_switch_and_switching_click_keeps_its_release() {
        use bevy::input::{mouse::{MouseButtonInput, MouseMotion}, ButtonState};
        let (mut app, _) = observed_app(InputDevice::Controller);
        app.world_mut().write_message(MouseMotion { delta: Vec2::new(0.1, -0.1) });
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller);
        for state in [ButtonState::Pressed, ButtonState::Released, ButtonState::Pressed] {
            app.world_mut().write_message(MouseButtonInput { button: MouseButton::Left, state, window: Entity::PLACEHOLDER });
            app.update();
            assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
            assert_eq!(app.world().resource::<Seen>().mouse.just_released(MouseButton::Left), state == ButtonState::Released);
        }
        assert!(app.world().resource::<Seen>().mouse.just_pressed(MouseButton::Left));
    }

    #[test]
    fn switching_press_release_pairs_preserve_all_release_bits_and_raw_events() {
        use bevy::input::{mouse::MouseButtonInput, ButtonState};
        let mut filter = SwitchingInput { keys: vec![KeyCode::Enter], mouse: vec![MouseButton::Left], ..default() };
        let mut raw = ButtonInput::default();
        raw.press(KeyCode::Enter); raw.release(KeyCode::Enter);
        assert!(!filter.filter(&raw).just_pressed(KeyCode::Enter));
        assert!(filter.filter(&raw).just_released(KeyCode::Enter));
        let event = MouseButtonInput { button: MouseButton::Left, state: ButtonState::Released, window: Entity::PLACEHOLDER };
        assert!(filter.allows_mouse(&event));
        assert!(!filter.allows_mouse(&MouseButtonInput { state: ButtonState::Pressed, ..event }));
        filter.mouse.clear();
        assert!(filter.allows_mouse(&MouseButtonInput { state: ButtonState::Pressed, ..event }));
    }

    #[test]
    fn switch_on_use_needs_fresh_activity_from_only_the_other_device() {
        use InputDevice::{Controller as Pad, KeyboardMouse as Keys};
        for a in [Activity { button: true, ..default() }, Activity { stick: true, ..default() }, Activity { trigger: true, ..default() }] {
            assert_eq!(switch_on_use(Keys, true, a).map(|s| s.0), Some(Pad));
            assert_eq!(switch_on_use(Pad, true, a), None);
            assert_eq!(switch_on_use(Keys, false, a), None);
            assert_eq!(switch_on_use(Keys, true, Activity { key: true, ..a }), None);
        }
        for a in [Activity { key: true, ..default() }, Activity { mouse: true, ..default() }] {
            assert_eq!(switch_on_use(Pad, true, a).map(|s| s.0), Some(Keys));
            assert_eq!(switch_on_use(Keys, true, a), None);
            assert_eq!(switch_on_use(Pad, false, a), None);
            assert_eq!(switch_on_use(Pad, true, Activity { button: true, ..a }), None);
        }
        assert_eq!(switch_on_use(Keys, true, Activity::default()), None);
        assert_eq!(switch_on_use(Pad, true, Activity::default()), None);
    }

    #[test]
    fn old_device_busy_refuses_every_incoming_cause() {
        for a in [Activity { button: true, ..default() }, Activity { stick: true, ..default() }, Activity { trigger: true, ..default() }] {
            assert_eq!(switch_on_use(InputDevice::KeyboardMouse, true, Activity { keyboard_busy: true, ..a }), None);
        }
        for a in [Activity { key: true, ..default() }, Activity { mouse: true, ..default() }] {
            assert_eq!(switch_on_use(InputDevice::Controller, true, Activity { controller_busy: true, ..a }), None);
        }
        let mut keys = ButtonInput::default();
        assert!(!digital_busy(&keys));
        keys.press(KeyCode::KeyF);
        assert!(!digital_busy(&keys));
        keys.clear();
        assert!(digital_busy(&keys));
        keys.release(KeyCode::KeyF);
        assert!(digital_busy(&keys));
        keys.press(KeyCode::KeyF);
        assert!(digital_busy(&keys), "release/repress cannot hide the start-of-frame hold");
        keys.release(KeyCode::KeyF);
        keys.clear();
        keys.press(KeyCode::KeyF);
        keys.release(KeyCode::KeyF);
        assert!(digital_busy(&keys), "a same-frame tap is also busy");
        keys.clear();
        assert!(!digital_busy(&keys));
    }

    #[test]
    fn held_old_device_and_release_frame_refuse_switch_until_a_fresh_press() {
        use crate::bindings::Control;
        for origin in [InputDevice::KeyboardMouse, InputDevice::Controller] {
            for action in [Control::Forward, Control::Block, Control::TogglePov] {
                let (mut app, pad) = observed_app(origin);
                // Forward is a stick on Controller; the other cases use digital buttons
                // [design, #2200; Bindings::pad].
                if origin == InputDevice::Controller && action == Control::Forward {
                    app.world_mut().get_mut::<Gamepad>(pad).unwrap().analog_mut().set(GamepadAxis::LeftStickY, 0.8);
                } else {
                    control_button(&mut app, pad, origin, action, true);
                }
                app.update();
                switch_from(&mut app, pad, origin, true);
                app.update();
                assert_eq!(app.world().resource::<Bindings>().device, origin);
                switch_from(&mut app, pad, origin, false);
                if origin == InputDevice::Controller && action == Control::Forward {
                    app.world_mut().get_mut::<Gamepad>(pad).unwrap().analog_mut().set(GamepadAxis::LeftStickY, 0.0);
                } else {
                    control_button(&mut app, pad, origin, action, false);
                }
                switch_from(&mut app, pad, origin, true);
                app.update();
                assert_eq!(app.world().resource::<Bindings>().device, origin, "old release/neutral frame");
                app.update();
                assert_eq!(app.world().resource::<Bindings>().device, origin, "no queued switch on continued hold");
                switch_from(&mut app, pad, origin, false);
                app.update();
                switch_from(&mut app, pad, origin, true);
                app.update();
                assert_ne!(app.world().resource::<Bindings>().device, origin);
            }
        }
    }

    #[test]
    fn disconnect_of_a_busy_pad_waits_out_the_old_device_frame() {
        for analog in [false, true] {
            let (mut app, pad) = observed_app(InputDevice::Controller);
            if analog {
                app.world_mut().get_mut::<Gamepad>(pad).unwrap().analog_mut().set(GamepadAxis::LeftStickX, 0.8);
            } else {
                button(&mut app, pad, true);
            }
            app.update();
            app.world_mut().despawn(pad);
            key(&mut app, true, false);
            app.update();
            assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller);
            app.update();
            assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller);
            key(&mut app, false, false);
            app.update();
            key(&mut app, true, false);
            app.update();
            assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
        }
    }

    #[test]
    fn refused_analog_crossings_need_neutral_and_a_fresh_crossing_after_old_keys_release() {
        for input in [GamepadInput::Axis(GamepadAxis::LeftStickX), GamepadAxis::RightStickY.into(),
            GamepadButton::LeftTrigger2.into(), GamepadButton::RightTrigger2.into()] {
            let (mut app, pad) = observed_app(InputDevice::KeyboardMouse);
            keyboard(&mut app, KeyCode::KeyW, true, false);
            app.update();
            app.world_mut().get_mut::<Gamepad>(pad).unwrap().analog_mut().set(input, STICK_DEAD + 0.01);
            app.update();
            assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
            keyboard(&mut app, KeyCode::KeyW, false, false);
            app.update();
            app.update();
            assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
            app.world_mut().get_mut::<Gamepad>(pad).unwrap().analog_mut().set(input, 0.0);
            app.update();
            app.world_mut().get_mut::<Gamepad>(pad).unwrap().analog_mut().set(input, STICK_DEAD + 0.01);
            app.update();
            assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller);
        }
    }
    #[test]
    fn platform_cover_filters_real_keys_mouse_pad_and_axes_until_neutral_without_mutating_hardware() {
        use bevy::input::{gamepad::{RawGamepadAxisChangedEvent, RawGamepadEvent}, mouse::MouseButtonInput, ButtonState};
        let (mut app, pad) = observed_app(InputDevice::Controller);
        app.world_mut().resource_mut::<Bindings>().automatic_input_switching = false;
        app.init_resource::<PlatformInputGate>();
        app.world_mut().resource_mut::<PlatformInputGate>().set_blocked(true);
        keyboard(&mut app, KeyCode::ArrowRight, true, false);
        app.world_mut().write_message(MouseButtonInput { window: Entity::PLACEHOLDER, button: MouseButton::Left, state: ButtonState::Pressed });
        button(&mut app, pad, true);
        app.world_mut().write_message(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(pad, GamepadAxis::LeftStickX, 0.8)));
        app.update();
        let seen = app.world().resource::<Seen>();
        assert!(!seen.keys.pressed(KeyCode::ArrowRight) && !seen.mouse.pressed(MouseButton::Left));
        assert_eq!(seen.pad, [false; 3]); assert_eq!(seen.stick, Vec2::ZERO); assert!(seen.repeats.is_empty());
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::ArrowRight));
        assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
        assert!(app.world().get::<Gamepad>(pad).unwrap().pressed(GamepadButton::South));
        assert!(app.world().get::<Gamepad>(pad).unwrap().left_stick().x > 0.5);
        app.world_mut().resource_mut::<PlatformInputGate>().set_blocked(false);
        keyboard(&mut app, KeyCode::ArrowRight, true, true);
        app.update();
        assert!(app.world().resource::<Seen>().repeats.is_empty());
        assert!(!app.world().resource::<Seen>().keys.pressed(KeyCode::ArrowRight));
        assert_eq!(app.world().resource::<Seen>().pad, [false; 3]);
        assert_eq!(app.world().resource::<Seen>().stick, Vec2::ZERO);
        keyboard(&mut app, KeyCode::ArrowRight, false, false);
        app.world_mut().write_message(MouseButtonInput { window: Entity::PLACEHOLDER, button: MouseButton::Left, state: ButtonState::Released });
        button(&mut app, pad, false);
        app.world_mut().write_message(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(pad, GamepadAxis::LeftStickX, 0.0)));
        app.update();
        let seen = app.world().resource::<Seen>();
        assert!(!seen.keys.just_released(KeyCode::ArrowRight) && !seen.mouse.just_released(MouseButton::Left));
        assert_eq!(seen.pad, [false; 3]);
        keyboard(&mut app, KeyCode::ArrowRight, true, false);
        app.world_mut().write_message(MouseButtonInput { window: Entity::PLACEHOLDER, button: MouseButton::Left, state: ButtonState::Pressed });
        button(&mut app, pad, true);
        app.world_mut().write_message(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(pad, GamepadAxis::LeftStickX, 0.8)));
        app.update();
        let seen = app.world().resource::<Seen>();
        assert!(seen.keys.just_pressed(KeyCode::ArrowRight) && seen.mouse.just_pressed(MouseButton::Left));
        assert!(seen.pad[0] && seen.pad[1]); assert!(seen.stick.x > 0.5);
    }
    #[test]
    fn platform_cover_uses_real_control_input_and_cancels_pending_releases_before_unlock() {
        use crate::{bindings::Control, control::PlayerControl};
        for device in [InputDevice::KeyboardMouse, InputDevice::Controller] {
            let (mut app, pad) = playing_app(device);
            app.world_mut().resource_mut::<Bindings>().automatic_input_switching = false;
            app.world_mut().resource_mut::<PlayerControl>().weapon_out = true;
            app.init_resource::<PlatformInputGate>();
            control_button(&mut app, pad, device, Control::ReadyItem, true); app.update();
            assert!(app.world().resource::<PlayerControl>().ready_held.is_some());
            app.world_mut().resource_mut::<PlatformInputGate>().set_blocked(true);
            control_button(&mut app, pad, device, Control::Use, true); app.update();
            assert!(app.world().resource::<PlayerControl>().ready_held.is_none());
            assert!(!app.world().resource::<PlayerControl>().trigger);
            app.world_mut().resource_mut::<PlatformInputGate>().set_blocked(false); app.update();
            assert!(!app.world().resource::<PlayerControl>().trigger);
            control_button(&mut app, pad, device, Control::ReadyItem, false);
            control_button(&mut app, pad, device, Control::Use, false); app.update();
            assert!(app.world().resource::<PlayerControl>().weapon_out, "masked release cannot holster/reload");
            control_button(&mut app, pad, device, Control::Use, true); app.update();
            assert!(app.world().resource::<PlayerControl>().trigger, "fresh post-neutral action works");
        }
    }
    #[test]
    fn absent_platform_gate_preserves_the_existing_action_views() {
        let (mut app, pad) = observed_app(InputDevice::KeyboardMouse);
        app.world_mut().resource_mut::<Bindings>().automatic_input_switching = false;
        key(&mut app, true, false); button(&mut app, pad, true); app.update();
        assert!(!app.world().contains_resource::<PlatformInputGate>());
        assert!(app.world().resource::<Seen>().keys.just_pressed(KeyCode::Enter));
        assert_eq!(app.world().resource::<Seen>().pad, [true, true, false]);
    }

    #[test]
    fn platform_cover_quarantines_real_touch_ids_through_release_without_losing_a_fresh_finger() {
        use bevy::input::touch::{TouchInput, TouchPhase, Touches};
        let (mut app, _) = observed_app(InputDevice::KeyboardMouse);
        app.world_mut().resource_mut::<Bindings>().automatic_input_switching = false;
        app.init_resource::<PlatformInputGate>();
        let touch = |app: &mut App, id, phase| { app.world_mut().write_message(TouchInput {
            window: Entity::PLACEHOLDER, id, phase, position: Vec2::ONE, force: None,
        }); };
        app.world_mut().resource_mut::<PlatformInputGate>().set_blocked(true);
        touch(&mut app, 7, TouchPhase::Started); app.update();
        app.world_mut().resource_mut::<PlatformInputGate>().set_blocked(false); app.update();
        assert!(!app.world().resource::<PlatformInputGate>().allows_touch(7));
        assert!(app.world().resource::<Touches>().get_pressed(7).is_some());
        touch(&mut app, 8, TouchPhase::Started); app.update();
        assert!(!app.world().resource::<PlatformInputGate>().allows_touch(7));
        assert!(app.world().resource::<PlatformInputGate>().allows_touch(8));
        touch(&mut app, 7, TouchPhase::Ended); app.update();
        assert!(!app.world().resource::<PlatformInputGate>().allows_touch(7), "old release cannot click a menu");
        touch(&mut app, 7, TouchPhase::Started); app.update();
        assert!(app.world().resource::<PlatformInputGate>().allows_touch(7), "fresh post-neutral press works without an idle frame");
    }


    #[test]
    fn native_and_arm_capture_reasons_overlap_and_quarantine_real_holds_until_neutral() {
        use bevy::input::{gamepad::{RawGamepadAxisChangedEvent, RawGamepadEvent}, mouse::MouseButtonInput,
            touch::{TouchInput, TouchPhase}, ButtonState};
        for release_native_first in [false, true] {
            let (mut app, pad) = observed_app(InputDevice::Controller);
            app.world_mut().resource_mut::<Bindings>().automatic_input_switching = false;
            app.init_resource::<PlatformInputGate>();
            {
                let mut gate=app.world_mut().resource_mut::<PlatformInputGate>();
                gate.set_arm_blocked(true);
                gate.set_blocked(true);
            }
            keyboard(&mut app,KeyCode::ArrowRight,true,false);
            button(&mut app,pad,true);
            app.world_mut().write_message(MouseButtonInput { window: Entity::PLACEHOLDER,
                button:MouseButton::Left,state:ButtonState::Pressed });
            app.world_mut().write_message(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(pad,GamepadAxis::LeftStickX,0.8)));
            app.world_mut().write_message(TouchInput { window:Entity::PLACEHOLDER,id:77,
                phase:TouchPhase::Started,position:Vec2::new(20.,20.),force:None });
            app.update();
            {
                let mut gate=app.world_mut().resource_mut::<PlatformInputGate>();
                if release_native_first { gate.set_blocked(false); } else { gate.set_arm_blocked(false); }
                assert!(!gate.allows_actions());
                assert_eq!(gate.native_blocked(),!release_native_first);
            }
            app.update();
            assert!(!app.world().resource::<Seen>().keys.pressed(KeyCode::ArrowRight));
            assert_eq!(app.world().resource::<Seen>().stick,Vec2::ZERO);
            {
                let mut gate=app.world_mut().resource_mut::<PlatformInputGate>();
                if release_native_first { gate.set_arm_blocked(false); } else { gate.set_blocked(false); }
                assert!(gate.allows_actions());
                assert!(!gate.allows_touch(77));
            }
            app.update();
            let seen=app.world().resource::<Seen>();
            assert!(!seen.keys.pressed(KeyCode::ArrowRight)&&!seen.mouse.pressed(MouseButton::Left));
            assert_eq!(seen.pad,[false;3]);
            assert_eq!(seen.stick,Vec2::ZERO);
            assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::ArrowRight));
            assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
            assert!(app.world().get::<Gamepad>(pad).unwrap().pressed(GamepadButton::South));
            keyboard(&mut app,KeyCode::ArrowRight,false,false);
            button(&mut app,pad,false);
            app.world_mut().write_message(MouseButtonInput { window:Entity::PLACEHOLDER,
                button:MouseButton::Left,state:ButtonState::Released });
            app.world_mut().write_message(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(pad,GamepadAxis::LeftStickX,0.)));
            app.world_mut().write_message(TouchInput { window:Entity::PLACEHOLDER,id:77,
                phase:TouchPhase::Ended,position:Vec2::new(20.,20.),force:None });
            app.update();
            assert!(!app.world().resource::<Seen>().keys.just_released(KeyCode::ArrowRight));
            assert!(!app.world().resource::<Seen>().mouse.just_released(MouseButton::Left));
            assert!(!app.world().resource::<PlatformInputGate>().allows_touch(77));
            app.update();
            assert!(app.world().resource::<PlatformInputGate>().allows_touch(77));
            keyboard(&mut app,KeyCode::ArrowRight,true,false);
            button(&mut app,pad,true);
            app.world_mut().write_message(MouseButtonInput { window:Entity::PLACEHOLDER,
                button:MouseButton::Left,state:ButtonState::Pressed });
            app.world_mut().write_message(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(pad,GamepadAxis::LeftStickX,0.8)));
            app.update();
            let seen=app.world().resource::<Seen>();
            assert!(seen.keys.just_pressed(KeyCode::ArrowRight)&&seen.mouse.just_pressed(MouseButton::Left));
            assert!(seen.pad[0]&&seen.pad[1]);
            assert!(seen.stick.x>0.5);
        }
    }
}
