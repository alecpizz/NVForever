//! Android native samples admitted through winit's sole queue reader.
//! See docs/design/android-controllers.md. No game/controller-specific IDs are hard-coded.
use bevy::{
    input::{
        gamepad::{
            GamepadConnection, GamepadConnectionEvent, RawGamepadAxisChangedEvent,
            RawGamepadButtonChangedEvent, RawGamepadEvent,
        },
        InputSystems,
    },
    prelude::*,
    window::{AppLifecycle, WindowFocused},
};
use nvf_play::bindings::InputDevice;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex},
};

#[cfg(target_os = "android")]
mod native;
const QUEUE_LIMIT: usize = 512;
const GAMEPAD_SOURCE: u32 = 0x401;
const DPAD_SOURCE: u32 = 0x201;
const JOYSTICK_SOURCE: u32 = 0x01000010;
#[cfg(target_os = "android")]
const AXES: [u32; 12] = [0, 1, 11, 12, 13, 14, 15, 16, 17, 18, 22, 23];

#[derive(Clone, Copy, Debug, PartialEq)]
struct MotionRange {
    min: f32,
    max: f32,
    flat: f32,
}
impl MotionRange {
    fn stick(self, value: f32) -> f32 {
        let (min, max, flat) = (
            f64::from(self.min),
            f64::from(self.max),
            f64::from(self.flat),
        );
        let center = (min + max) * 0.5;
        let radius = (max - min) * 0.5;
        let value = f64::from(value);
        if !value.is_finite() || radius <= 0.0 || (value - center).abs() <= flat {
            0.0
        } else {
            ((value - center) / radius).clamp(-1.0, 1.0) as f32
        }
    }
    fn trigger(self, value: f32) -> f32 {
        let (min, max, flat, value) = (
            f64::from(self.min),
            f64::from(self.max),
            f64::from(self.flat),
            f64::from(value),
        );
        if !value.is_finite() || max <= min || value <= min + flat {
            0.0
        } else {
            ((value - min) / (max - min)).clamp(0.0, 1.0) as f32
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
struct Device {
    id: i32,
    name: String,
    vendor: Option<u16>,
    product: Option<u16>,
    sources: u32,
    ranges: HashMap<u32, MotionRange>,
}
impl Device {
    fn generic(id: i32) -> Self {
        Self {
            id,
            name: "Android controller".into(),
            vendor: None,
            product: None,
            sources: GAMEPAD_SOURCE,
            ranges: HashMap::new(),
        }
    }
    fn supported(&self) -> bool {
        source(self.sources, GAMEPAD_SOURCE) || source(self.sources, JOYSTICK_SOURCE)
    }
    fn axis(&self, values: &HashMap<u32, f32>, axis: u32, trigger: bool) -> f32 {
        let range = self.ranges.get(&axis).copied().unwrap_or(MotionRange {
            min: if trigger { 0.0 } else { -1.0 },
            max: 1.0,
            flat: 0.0,
        });
        let value = values
            .get(&axis)
            .copied()
            .unwrap_or(if trigger { range.min } else { 0.0 });
        if trigger {
            range.trigger(value)
        } else {
            range.stick(value)
        }
    }
}
fn source(value: u32, wanted: u32) -> bool {
    value & wanted == wanted
}
/// Whether winit may translate this native input into ordinary window input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NativeKind {
    MappedKey,
    UnmappedKey,
    Motion,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ingress {
    Fallback,
    Drop,
    Admit,
}
fn ingress(queue: &Queue, device: i32, sources: u32, kind: NativeKind) -> Ingress {
    let explicit_controller = source(sources, GAMEPAD_SOURCE) || source(sources, JOYSTICK_SOURCE);
    let owns = match kind {
        // Unknown keys may be Android Home/Back/volume or another OS action.
        NativeKind::UnmappedKey => false,
        // Mouse and touchscreen motion always belongs to the ordinary window path.
        NativeKind::Motion => explicit_controller,
        NativeKind::MappedKey => {
            explicit_controller
                || queue.known.contains(&device)
                || (queue.retired.contains(&device) && source(sources, DPAD_SOURCE))
        }
    };
    if !owns {
        Ingress::Fallback
    } else if kind == NativeKind::Motion && !source(sources, JOYSTICK_SOURCE) {
        Ingress::Drop
    } else if queue.accepts(device, sources) {
        Ingress::Admit
    } else {
        Ingress::Drop
    }
}

#[derive(Clone, Debug)]
enum Sample {
    Devices(Vec<Device>),
    Key {
        device: i32,
        button: GamepadButton,
        down: bool,
    },
    Motion {
        device: i32,
        axes: HashMap<u32, f32>,
    },
    Release(i32),
    NeutralAll,
    Reset,
}
#[derive(Default)]
struct Queue {
    events: VecDeque<Sample>,
    known: HashSet<i32>,
    retired: HashSet<i32>,
    latest: Option<Vec<Device>>,
    epoch: Option<u64>,
    overflow: bool,
}
impl Queue {
    #[cfg(target_os = "android")]
    fn invalidate(&mut self) {
        self.invalidate_with(Sample::Reset);
    }
    fn invalidate_with(&mut self, reset: Sample) {
        self.events.clear();
        self.overflow = false;
        self.events.push_back(reset);
        if let Some(devices) = &self.latest {
            self.events.push_back(Sample::Devices(devices.clone()));
        }
    }
    fn snapshot(&mut self, text: &str) -> Result<(), String> {
        let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let epoch = value["reset"]
            .as_u64()
            .ok_or("Controller reset generation is missing")?;
        let devices = parse_devices(&value["devices"].to_string())?;
        if self.epoch.is_some_and(|before| before != epoch) {
            self.invalidate_with(Sample::NeutralAll);
        }
        self.epoch = Some(epoch);
        self.devices(devices);
        Ok(())
    }
    fn push(&mut self, sample: Sample) {
        if self.overflow {
            return;
        }
        if let Sample::Motion { device, .. } = &sample {
            if matches!(self.events.back(),Some(Sample::Motion{device:last,..}) if last==device) {
                self.events.pop_back();
            }
        }
        if self.events.len() >= QUEUE_LIMIT {
            self.events.clear();
            self.overflow = true;
            return;
        }
        self.events.push_back(sample);
    }
    fn drain(&mut self) -> Vec<Sample> {
        if std::mem::take(&mut self.overflow) {
            self.events.clear();
            let mut events = vec![Sample::Reset];
            if let Some(devices) = &self.latest {
                events.push(Sample::Devices(devices.clone()));
            }
            events
        } else {
            self.events.drain(..).collect()
        }
    }
    fn accepts(&self, id: i32, sources: u32) -> bool {
        !self.retired.contains(&id)
            && (self.known.contains(&id)
                || source(sources, GAMEPAD_SOURCE)
                || source(sources, JOYSTICK_SOURCE))
    }
    fn key(
        &mut self,
        id: i32,
        sources: u32,
        button: GamepadButton,
        down: bool,
        repeat: bool,
    ) -> bool {
        if !self.accepts(id, sources) {
            return false;
        }
        self.known.insert(id);
        if !repeat {
            self.push(Sample::Key {
                device: id,
                button,
                down,
            });
        }
        true
    }
    fn motion(&mut self, id: i32, sources: u32, axes: HashMap<u32, f32>) -> bool {
        if !source(sources, JOYSTICK_SOURCE) || !self.accepts(id, sources) {
            return false;
        }
        self.known.insert(id);
        self.push(Sample::Motion { device: id, axes });
        true
    }
    fn devices(&mut self, devices: Vec<Device>) {
        let now: HashSet<_> = devices
            .iter()
            .filter(|d| d.supported())
            .map(|d| d.id)
            .collect();
        self.retired.extend(self.known.difference(&now).copied());
        for id in &now {
            self.retired.remove(id);
        }
        self.known = now;
        self.latest = Some(devices.clone());
        self.push(Sample::Devices(devices));
    }
}
#[derive(Resource, Clone, Default)]
struct Inbox(Arc<Mutex<Queue>>);
impl Inbox {
    fn lock(&self) -> std::sync::MutexGuard<'_, Queue> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }
}
struct Pad {
    entity: Entity,
    device: Device,
    keys: HashSet<GamepadButton>,
    hat: HashSet<GamepadButton>,
    triggers: [f32; 2],
}
#[derive(Resource)]
struct State {
    pads: HashMap<i32, Pad>,
    active: Option<i32>,
    focused: bool,
    running: bool,
    focus: bevy::ecs::message::MessageCursor<WindowFocused>,
    lifecycle: bevy::ecs::message::MessageCursor<AppLifecycle>,
    retired: Vec<Entity>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            pads: HashMap::new(),
            active: None,
            focused: true,
            running: true,
            focus: default(),
            lifecycle: default(),
            retired: vec![],
        }
    }
}
#[cfg(target_os = "android")]
pub fn install() {
    native::install();
}
pub struct AndroidControllersPlugin {
    native: bool,
}
impl AndroidControllersPlugin {
    #[cfg(target_os = "android")]
    pub fn native() -> Self {
        Self { native: true }
    }
    #[cfg(test)]
    fn headless() -> Self {
        Self { native: false }
    }
}
impl Plugin for AndroidControllersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Inbox>()
            .init_resource::<State>()
            .add_message::<WindowFocused>()
            .add_message::<AppLifecycle>()
            .add_systems(PreUpdate, ingest.before(InputSystems))
            .add_systems(Last, retire);
        #[cfg(target_os = "android")]
        if self.native {
            app.insert_resource(Inbox(native::queue()));
            app.add_systems(PreUpdate, native::poll.before(ingest));
        }
        #[cfg(not(target_os = "android"))]
        let _ = self.native;
    }
}
fn emit(world: &mut World, event: RawGamepadEvent) {
    world.write_message(event);
}
fn button(world: &mut World, pad: &Pad, b: GamepadButton) {
    let value = match b {
        GamepadButton::LeftTrigger2 => pad.triggers[0],
        GamepadButton::RightTrigger2 => pad.triggers[1],
        _ => 0.0,
    }
    .max(if pad.keys.contains(&b) || pad.hat.contains(&b) {
        1.0
    } else {
        0.0
    });
    emit(
        world,
        RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(pad.entity, b, value)),
    );
}
const BUTTONS: [GamepadButton; 16] = [
    GamepadButton::South,
    GamepadButton::East,
    GamepadButton::West,
    GamepadButton::North,
    GamepadButton::LeftTrigger,
    GamepadButton::RightTrigger,
    GamepadButton::LeftTrigger2,
    GamepadButton::RightTrigger2,
    GamepadButton::Select,
    GamepadButton::Start,
    GamepadButton::LeftThumb,
    GamepadButton::RightThumb,
    GamepadButton::DPadUp,
    GamepadButton::DPadDown,
    GamepadButton::DPadLeft,
    GamepadButton::DPadRight,
];
fn neutral(world: &mut World, pad: &mut Pad) {
    pad.keys.clear();
    pad.hat.clear();
    pad.triggers = [0.0; 2];
    for b in BUTTONS {
        button(world, pad, b);
    }
    for axis in [
        GamepadAxis::LeftStickX,
        GamepadAxis::LeftStickY,
        GamepadAxis::RightStickX,
        GamepadAxis::RightStickY,
    ] {
        emit(
            world,
            RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(pad.entity, axis, 0.0)),
        );
    }
}
fn connection(world: &mut World, entity: Entity, c: GamepadConnection) {
    let event = GamepadConnectionEvent::new(entity, c);
    world.write_message(event.clone());
    emit(world, RawGamepadEvent::Connection(event));
}
fn ensure<'a>(world: &mut World, state: &'a mut State, id: i32) -> &'a mut Pad {
    state.pads.entry(id).or_insert_with(|| {
        let device = Device::generic(id);
        let entity = world.spawn_empty().id();
        connection(
            world,
            entity,
            GamepadConnection::Connected {
                name: device.name.clone(),
                vendor_id: None,
                product_id: None,
            },
        );
        Pad {
            entity,
            device,
            keys: HashSet::new(),
            hat: HashSet::new(),
            triggers: [0.0; 2],
        }
    })
}
fn parse_devices(text: &str) -> Result<Vec<Device>, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let list = value
        .as_array()
        .ok_or("Controller metadata must be an array")?;
    if list.len() > 32 {
        return Err("Too many controller devices".into());
    }
    let mut result = Vec::new();
    for value in list {
        let Some(id) = value["id"].as_i64().and_then(|n| i32::try_from(n).ok()) else {
            continue;
        };
        let sources = value["sources"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0);
        let mut d = Device {
            id,
            name: value["name"]
                .as_str()
                .unwrap_or("Android controller")
                .chars()
                .take(128)
                .collect(),
            vendor: value["vendor"]
                .as_u64()
                .and_then(|n| u16::try_from(n).ok())
                .filter(|n| *n != 0),
            product: value["product"]
                .as_u64()
                .and_then(|n| u16::try_from(n).ok())
                .filter(|n| *n != 0),
            sources,
            ranges: HashMap::new(),
        };
        if !d.supported() {
            continue;
        }
        if let Some(ranges) = value["axes"].as_array() {
            for range in ranges.iter().take(64) {
                let Some(axis) = range["axis"].as_u64().and_then(|n| u32::try_from(n).ok()) else {
                    continue;
                };
                let (Some(min), Some(max), Some(flat)) = (
                    range["min"].as_f64(),
                    range["max"].as_f64(),
                    range["flat"].as_f64(),
                ) else {
                    continue;
                };
                if min.is_finite()
                    && max.is_finite()
                    && flat.is_finite()
                    && max > min
                    && flat >= 0.0
                {
                    let range = MotionRange {
                        min: min as f32,
                        max: max as f32,
                        flat: flat as f32,
                    };
                    if range.min.is_finite() && range.max.is_finite() && range.flat.is_finite() {
                        d.ranges.insert(axis, range);
                    }
                }
            }
        }
        result.push(d);
    }
    Ok(result)
}
fn ingest(world: &mut World) {
    let mut state = world.remove_resource::<State>().unwrap_or_default();
    let before = state.focused && state.running;
    for event in state
        .focus
        .read(world.resource::<Messages<WindowFocused>>())
    {
        state.focused = event.focused;
    }
    for event in state
        .lifecycle
        .read(world.resource::<Messages<AppLifecycle>>())
    {
        match event {
            AppLifecycle::Suspended => state.running = false,
            AppLifecycle::Running => state.running = true,
            _ => {}
        }
    }
    let active = state.focused && state.running;
    if before && !active {
        for pad in state.pads.values_mut() {
            neutral(world, pad);
        }
        nvf_play::bindings::cancel_input_holds(world);
    }
    let mut samples = world.resource::<Inbox>().lock().drain();
    if !before && active {
        // Events queued while unfocused cannot become fresh actions merely because
        // focus returned before this frame drained the native queue.
        for pad in state.pads.values_mut() {
            neutral(world, pad);
        }
        nvf_play::bindings::cancel_input_holds(world);
        samples.retain(|sample| {
            matches!(
                sample,
                Sample::Devices(_) | Sample::Reset | Sample::NeutralAll
            )
        });
    }
    for sample in samples {
        match sample {
            Sample::Devices(devices) => {
                let ids: HashSet<_> = devices
                    .iter()
                    .filter(|d| d.supported())
                    .map(|d| d.id)
                    .collect();
                let removed: Vec<_> = state
                    .pads
                    .keys()
                    .filter(|id| !ids.contains(id))
                    .copied()
                    .collect();
                for id in removed {
                    if let Some(mut pad) = state.pads.remove(&id) {
                        // ActionPads aggregates controllers; even a non-last-used pad
                        // can own a pending release action.
                        nvf_play::bindings::cancel_input_holds(world);
                        neutral(world, &mut pad);
                        connection(world, pad.entity, GamepadConnection::Disconnected);
                        state.retired.push(pad.entity);
                        if state.active == Some(id) {
                            nvf_play::bindings::set_device(world, InputDevice::KeyboardMouse);
                            state.active = None;
                        }
                    }
                }
                for d in devices.into_iter().filter(|d| d.supported()) {
                    if state.pads.get(&d.id).is_some_and(|pad| pad.device != d) {
                        nvf_play::bindings::cancel_input_holds(world);
                    }
                    let pad = ensure(world, &mut state, d.id);
                    if pad.device != d {
                        neutral(world, pad);
                        connection(
                            world,
                            pad.entity,
                            GamepadConnection::Connected {
                                name: d.name.clone(),
                                vendor_id: d.vendor,
                                product_id: d.product,
                            },
                        );
                    }
                    pad.device = d;
                }
            }
            Sample::Release(id) => {
                if let Some(pad) = state.pads.get_mut(&id) {
                    nvf_play::bindings::cancel_input_holds(world);
                    neutral(world, pad);
                }
            }
            Sample::NeutralAll => {
                for pad in state.pads.values_mut() {
                    neutral(world, pad);
                }
                nvf_play::bindings::cancel_input_holds(world);
            }
            Sample::Reset => {
                for pad in state.pads.values_mut() {
                    neutral(world, pad);
                }
                nvf_play::bindings::cancel_input_holds(world);
                nvf_play::bindings::set_device(world, InputDevice::KeyboardMouse);
                state.active = None;
            }
            Sample::Key {
                device,
                button: b,
                down,
            } if active => {
                if down {
                    state.active = Some(device);
                }
                let pad = ensure(world, &mut state, device);
                if down {
                    pad.keys.insert(b);
                } else {
                    pad.keys.remove(&b);
                }
                button(world, pad, b);
            }
            Sample::Motion { device, axes } if active => {
                let pad = ensure(world, &mut state, device);
                let right = if pad.device.ranges.contains_key(&11)
                    && pad.device.ranges.contains_key(&14)
                {
                    [11, 14]
                } else if pad.device.ranges.contains_key(&12) && pad.device.ranges.contains_key(&13)
                {
                    [12, 13]
                } else {
                    [11, 14]
                };
                let values = [
                    pad.device.axis(&axes, 0, false),
                    -pad.device.axis(&axes, 1, false),
                    pad.device.axis(&axes, right[0], false),
                    -pad.device.axis(&axes, right[1], false),
                ];
                for (axis, value) in [
                    GamepadAxis::LeftStickX,
                    GamepadAxis::LeftStickY,
                    GamepadAxis::RightStickX,
                    GamepadAxis::RightStickY,
                ]
                .into_iter()
                .zip(values)
                {
                    emit(
                        world,
                        RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(
                            pad.entity, axis, value,
                        )),
                    );
                }
                let left = if pad.device.ranges.contains_key(&17)
                    || !pad.device.ranges.contains_key(&23)
                {
                    17
                } else {
                    23
                };
                let right = if pad.device.ranges.contains_key(&18)
                    || !pad.device.ranges.contains_key(&22)
                {
                    18
                } else {
                    22
                };
                pad.triggers = [
                    pad.device.axis(&axes, left, true),
                    pad.device.axis(&axes, right, true),
                ];
                let hat = Vec2::new(
                    pad.device.axis(&axes, 15, false),
                    pad.device.axis(&axes, 16, false),
                );
                pad.hat.clear();
                for (b, on) in [
                    (GamepadButton::DPadLeft, hat.x < -0.5),
                    (GamepadButton::DPadRight, hat.x > 0.5),
                    (GamepadButton::DPadUp, hat.y < -0.5),
                    (GamepadButton::DPadDown, hat.y > 0.5),
                ] {
                    if on {
                        pad.hat.insert(b);
                    }
                }
                for b in [
                    GamepadButton::DPadLeft,
                    GamepadButton::DPadRight,
                    GamepadButton::DPadUp,
                    GamepadButton::DPadDown,
                    GamepadButton::LeftTrigger2,
                    GamepadButton::RightTrigger2,
                ] {
                    button(world, pad, b);
                }
                if values.iter().any(|v| v.abs() > nvf_play::pad::STICK_DEAD)
                    || pad.triggers.iter().any(|v| *v > nvf_play::pad::STICK_DEAD)
                    || hat.length_squared() > 0.25
                {
                    state.active = Some(device);
                }
            }
            _ => {}
        }
    }
    world.insert_resource(state);
}
fn retire(mut commands: Commands, mut state: ResMut<State>) {
    for entity in state.retired.drain(..) {
        if let Ok(mut entity) = commands.get_entity(entity) {
            entity.despawn();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::{
        keyboard::{Key, KeyboardInput},
        touch::{TouchInput, TouchPhase},
        ButtonState,
    };
    use nvf_play::{
        bindings::{Bindings, Control, ControlInput},
        pad::{PadInput, TouchDeviceSwitch},
    };
    #[derive(Resource, Default)]
    struct Seen {
        jump: bool,
        forward: bool,
        fire: bool,
        enter: bool,
    }
    fn observe(input: ControlInput, keys: Res<ButtonInput<KeyCode>>, mut seen: ResMut<Seen>) {
        seen.jump = input.just_pressed(Control::Jump);
        seen.forward = input.pressed(Control::Forward);
        seen.fire = input.pressed(Control::Use);
        seen.enter = keys.just_pressed(KeyCode::Enter);
    }
    fn scene(touch: bool) -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::input::InputPlugin,
            bevy::state::app::StatesPlugin,
        ));
        app.insert_state(nvf_play::PlayState::Playing)
            .init_resource::<Bindings>()
            .init_resource::<Assets<Image>>()
            .init_resource::<nvf_play::control::PlayerControl>()
            .init_resource::<Seen>()
            .add_plugins((
                AndroidControllersPlugin::headless(),
                nvf_play::pad::PadPlugin,
            ))
            .add_systems(Update, observe);
        if touch {
            app.add_plugins(crate::touch::TouchControlsPlugin);
        }
        app.world_mut().spawn((
            Window {
                resolution: bevy::window::WindowResolution::new(1000, 1000),
                ..default()
            },
            bevy::window::PrimaryWindow,
        ));
        app.update();
        app
    }
    fn device() -> Device {
        Device {
            id: 7,
            name: "Generic Bluetooth Controller".into(),
            vendor: None,
            product: None,
            sources: GAMEPAD_SOURCE | JOYSTICK_SOURCE,
            ranges: HashMap::new(),
        }
    }
    fn attach(app: &mut App) {
        app.world()
            .resource::<Inbox>()
            .lock()
            .devices(vec![device()]);
        app.update();
    }
    fn key(app: &App, button: GamepadButton, down: bool) {
        assert!(app
            .world()
            .resource::<Inbox>()
            .lock()
            .key(7, GAMEPAD_SOURCE, button, down, false));
    }
    fn entity(app: &App) -> Entity {
        app.world().resource::<State>().pads[&7].entity
    }
    fn motion(app: &App, axes: &[(u32, f32)]) {
        assert!(app.world().resource::<Inbox>().lock().motion(
            7,
            JOYSTICK_SOURCE,
            axes.iter().copied().collect()
        ));
    }
    fn stick_visibility(app: &mut App) -> Visibility {
        let world = app.world_mut();
        let mut q = world.query::<(&crate::touch::TouchVisual, &Visibility)>();
        *q.iter(world)
            .find(|(visual, _)| matches!(visual, crate::touch::TouchVisual::Stick))
            .unwrap()
            .1
    }
    #[test]
    fn retired_controller_ingress_is_claimed_instead_of_becoming_window_touch_or_keys() {
        let mut queue = Queue::default();
        queue.devices(vec![device()]);
        queue.drain();
        queue.devices(vec![]);
        queue.drain();
        assert_eq!(
            ingress(&queue, 7, JOYSTICK_SOURCE, NativeKind::Motion),
            Ingress::Drop
        );
        assert_eq!(
            ingress(&queue, 7, GAMEPAD_SOURCE, NativeKind::Motion),
            Ingress::Drop,
            "unsupported controller motion is still owned"
        );
        assert_eq!(
            ingress(&queue, 7, GAMEPAD_SOURCE, NativeKind::MappedKey),
            Ingress::Drop
        );
        assert_eq!(
            ingress(&queue, 7, DPAD_SOURCE, NativeKind::MappedKey),
            Ingress::Drop
        );
        assert_eq!(
            ingress(&queue, 7, 0x1002, NativeKind::Motion),
            Ingress::Fallback,
            "real touchscreen"
        );
        assert_eq!(
            ingress(&queue, 7, 0x2002, NativeKind::Motion),
            Ingress::Fallback,
            "real mouse"
        );
        assert_eq!(
            ingress(&queue, 7, 0x101, NativeKind::MappedKey),
            Ingress::Fallback,
            "real keyboard arrows"
        );
        assert_eq!(
            ingress(&queue, 8, DPAD_SOURCE, NativeKind::MappedKey),
            Ingress::Fallback,
            "unrelated D-pad remote"
        );
        assert_eq!(
            ingress(&queue, 7, GAMEPAD_SOURCE, NativeKind::UnmappedKey),
            Ingress::Fallback,
            "OS-reserved unknown keys"
        );
        assert!(
            queue.drain().is_empty(),
            "rejected ingress enqueues nothing"
        );
        queue.devices(vec![device()]);
        queue.drain();
        assert_eq!(
            ingress(&queue, 7, JOYSTICK_SOURCE, NativeKind::Motion),
            Ingress::Admit
        );
        assert_eq!(
            ingress(&queue, 7, GAMEPAD_SOURCE, NativeKind::Motion),
            Ingress::Drop
        );
        assert_eq!(
            ingress(&queue, 7, DPAD_SOURCE, NativeKind::MappedKey),
            Ingress::Admit
        );
        assert_eq!(
            ingress(&queue, 7, GAMEPAD_SOURCE, NativeKind::UnmappedKey),
            Ingress::Fallback
        );
    }
    #[test]
    fn native_queue_events_reach_gamepad_remapping_and_generic_xbox_prompt_state() {
        let mut app = scene(false);
        attach(&mut app);
        assert_eq!(
            app.world().resource::<Bindings>().device,
            InputDevice::KeyboardMouse,
            "connection alone does not select controller"
        );
        key(&app, GamepadButton::South, true);
        app.update();
        assert_eq!(
            app.world().resource::<Bindings>().device,
            InputDevice::Controller
        );
        let pad = app.world().get::<Gamepad>(entity(&app)).unwrap();
        assert!(pad.pressed(GamepadButton::South));
        let in_use = app.world().resource::<nvf_play::pad::PadInUse>();
        assert_eq!(in_use.name.as_deref(), Some("Generic Bluetooth Controller"));
        assert!(!in_use.playstation);
        assert_eq!(
            app.world()
                .resource::<nvf_play::pad::ButtonStyle>()
                .labels(in_use.playstation),
            nvf_play::pad::Labels::Xbox
        );
        key(&app, GamepadButton::South, false);
        app.update();
        app.update();
        nvf_play::bindings::rebind_pad(
            app.world_mut(),
            Control::Jump,
            PadInput::Button(GamepadButton::West),
        );
        key(&app, GamepadButton::West, true);
        app.update();
        assert!(
            app.world().resource::<Seen>().jump,
            "native button uses the existing remapped action route"
        );
        motion(&app, &[(0, 0.0), (1, -0.9)]);
        app.update();
        assert!(
            app.world().resource::<Seen>().forward,
            "Android down-positive Y is converted to Bevy up-positive movement"
        );
    }
    #[test]
    fn controller_use_hides_overlay_and_first_touch_only_restores_it() {
        let mut app = scene(true);
        attach(&mut app);
        assert_eq!(
            stick_visibility(&mut app),
            Visibility::Inherited,
            "idle connection retains the active gameplay overlay"
        );
        key(&app, GamepadButton::South, true);
        app.update();
        assert_eq!(
            stick_visibility(&mut app),
            Visibility::Hidden,
            "the active gameplay movement control hides"
        );
        key(&app, GamepadButton::South, false);
        app.update();
        app.update();
        let window = {
            let world = app.world_mut();
            let mut q = world.query_filtered::<Entity, With<bevy::window::PrimaryWindow>>();
            q.single(world).unwrap()
        };
        app.world_mut().write_message(TouchInput {
            phase: TouchPhase::Started,
            position: Vec2::new(920.0, 900.0),
            window,
            id: 10,
            force: None,
        });
        app.update();
        assert_eq!(
            app.world().resource::<Bindings>().device,
            InputDevice::KeyboardMouse
        );
        assert!(!app.world().resource::<TouchDeviceSwitch>().allows_actions());
        assert_eq!(
            stick_visibility(&mut app),
            Visibility::Inherited,
            "the consumed touch returns the gameplay overlay"
        );
        assert!(
            !app.world().resource::<Seen>().fire,
            "switching tap must not also fire"
        );
        app.world_mut().write_message(TouchInput {
            phase: TouchPhase::Ended,
            position: Vec2::new(920.0, 900.0),
            window,
            id: 10,
            force: None,
        });
        app.update();
        app.world_mut().write_message(TouchInput {
            phase: TouchPhase::Started,
            position: Vec2::new(920.0, 900.0),
            window,
            id: 11,
            force: None,
        });
        app.update();
        assert!(
            app.world().resource::<Seen>().fire,
            "a fresh subsequent touch uses ordinary touch controls"
        );
    }
    #[test]
    fn active_disconnect_cancels_holds_returns_touch_and_rejects_stale_native_events() {
        let mut app = scene(true);
        attach(&mut app);
        key(&app, GamepadButton::RightTrigger2, true);
        app.update();
        app.update();
        app.world_mut()
            .resource_mut::<nvf_play::control::PlayerControl>()
            .ready_held = Some(0.4);
        let retired = entity(&app);
        app.world().resource::<Inbox>().lock().devices(vec![]);
        app.update();
        assert_eq!(
            app.world().resource::<Bindings>().device,
            InputDevice::KeyboardMouse
        );
        assert!(app
            .world()
            .resource::<nvf_play::control::PlayerControl>()
            .ready_held
            .is_none());
        assert!(app.world().get_entity(retired).is_err());
        assert!(!app.world().resource::<Inbox>().lock().key(
            7,
            GAMEPAD_SOURCE,
            GamepadButton::RightTrigger2,
            true,
            false
        ));
        app.update();
        assert!(app.world().resource::<State>().pads.is_empty());
        assert!(!app.world().resource::<Seen>().fire);
    }
    #[test]
    fn focus_loss_neutralizes_pad_without_repeating_down_rearming_it_on_resume() {
        let mut app = scene(false);
        attach(&mut app);
        key(&app, GamepadButton::RightTrigger2, true);
        app.update();
        app.update();
        assert!(app.world().resource::<Seen>().fire);
        app.world_mut()
            .resource_mut::<nvf_play::control::PlayerControl>()
            .ready_held = Some(0.4);
        let window = Entity::PLACEHOLDER;
        app.world_mut().write_message(WindowFocused {
            window,
            focused: false,
        });
        app.update();
        assert!(!app.world().resource::<Seen>().fire);
        assert!(app
            .world()
            .resource::<nvf_play::control::PlayerControl>()
            .ready_held
            .is_none());
        app.world_mut().write_message(WindowFocused {
            window,
            focused: true,
        });
        assert!(app.world().resource::<Inbox>().lock().key(
            7,
            GAMEPAD_SOURCE,
            GamepadButton::RightTrigger2,
            true,
            true
        ));
        app.update();
        assert!(!app.world().resource::<Seen>().fire);
        key(&app, GamepadButton::RightTrigger2, false);
        app.update();
        key(&app, GamepadButton::RightTrigger2, true);
        app.update();
        assert!(app.world().resource::<Seen>().fire);
        app.world()
            .resource::<Inbox>()
            .lock()
            .push(Sample::Release(7));
        app.update();
        assert!(!app.world().resource::<Seen>().fire);
    }
    #[test]
    fn button_and_hat_or_digital_and_analog_trigger_ownership_do_not_release_each_other() {
        let mut app = scene(false);
        attach(&mut app);
        app.world_mut().resource_mut::<Bindings>().device = InputDevice::Controller;
        key(&app, GamepadButton::DPadUp, true);
        motion(&app, &[(16, -1.0), (17, 0.9)]);
        key(&app, GamepadButton::LeftTrigger2, true);
        app.update();
        motion(&app, &[(16, 0.0), (17, 0.0)]);
        app.update();
        let g = app.world().get::<Gamepad>(entity(&app)).unwrap();
        assert!(g.pressed(GamepadButton::DPadUp));
        assert!(g.pressed(GamepadButton::LeftTrigger2));
        key(&app, GamepadButton::DPadUp, false);
        key(&app, GamepadButton::LeftTrigger2, false);
        app.update();
        let g = app.world().get::<Gamepad>(entity(&app)).unwrap();
        assert!(g.just_released(GamepadButton::DPadUp));
        assert!(g.just_released(GamepadButton::LeftTrigger2));
    }
    #[test]
    fn queue_overflow_releases_old_state_and_keeps_latest_device_truth() {
        let mut app = scene(false);
        attach(&mut app);
        key(&app, GamepadButton::RightTrigger2, true);
        app.update();
        app.update();
        {
            let inbox = app.world().resource::<Inbox>();
            let mut queue = inbox.lock();
            for _ in 0..=QUEUE_LIMIT {
                queue.push(Sample::Key {
                    device: 7,
                    button: GamepadButton::South,
                    down: true,
                });
            }
            queue.devices(vec![]);
        }
        app.update();
        assert!(app.world().resource::<State>().pads.is_empty());
        assert!(!app.world().resource::<Seen>().fire);
        assert_eq!(
            app.world().resource::<Bindings>().device,
            InputDevice::KeyboardMouse
        );
    }
    #[test]
    fn unknown_keyboard_and_touch_sources_are_not_admitted_as_controllers() {
        let mut app = scene(false);
        assert!(!app.world().resource::<Inbox>().lock().key(
            20,
            0x101,
            GamepadButton::South,
            true,
            false
        ));
        assert!(!app
            .world()
            .resource::<Inbox>()
            .lock()
            .motion(20, 0x1002, HashMap::new()));
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyA,
            logical_key: Key::Character("a".into()),
            state: ButtonState::Pressed,
            text: Some("a".into()),
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
        assert!(app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyA));
        assert!(app.world().resource::<State>().pads.is_empty());
    }
    #[test]
    fn opt_out_stays_controller_until_touch_has_no_available_pad() {
        let mut app = scene(true);
        attach(&mut app);
        {
            let mut b = app.world_mut().resource_mut::<Bindings>();
            b.device = InputDevice::Controller;
            b.automatic_input_switching = false;
        }
        let window = Entity::PLACEHOLDER;
        app.world_mut().write_message(TouchInput {
            phase: TouchPhase::Started,
            position: Vec2::new(10.0, 10.0),
            window,
            id: 5,
            force: None,
        });
        app.update();
        assert_eq!(
            app.world().resource::<Bindings>().device,
            InputDevice::Controller
        );
        app.world().resource::<Inbox>().lock().devices(vec![]);
        app.update();
        app.update();
        assert_eq!(
            app.world().resource::<Bindings>().device,
            InputDevice::KeyboardMouse,
            "missing pad cannot leave a saved manual choice unusable"
        );
    }
    #[test]
    fn native_menu_releases_preserve_an_independent_physical_arrow_hold() {
        let mut app = scene(false);
        attach(&mut app);
        {
            let mut b = app.world_mut().resource_mut::<Bindings>();
            b.device = InputDevice::Controller;
            b.automatic_input_switching = false;
        }
        app.world_mut()
            .resource_mut::<NextState<nvf_play::PlayState>>()
            .set(nvf_play::PlayState::Menu);
        app.update();
        let keyboard = |down| KeyboardInput {
            key_code: KeyCode::ArrowUp,
            logical_key: Key::ArrowUp,
            state: if down {
                ButtonState::Pressed
            } else {
                ButtonState::Released
            },
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        };
        app.world_mut().write_message(keyboard(true));
        key(&app, GamepadButton::DPadUp, true);
        app.update();
        key(&app, GamepadButton::DPadUp, false);
        app.update();
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        assert!(keys.pressed(KeyCode::ArrowUp));
        assert!(!keys.just_released(KeyCode::ArrowUp));
        app.world_mut().write_message(keyboard(false));
        app.update();
        assert!(app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .just_released(KeyCode::ArrowUp));
        key(&app, GamepadButton::South, true);
        app.update();
        assert!(
            app.world().resource::<Seen>().enter,
            "native A reaches existing menu confirmation"
        );
    }
    #[test]
    fn activity_reset_generation_and_resume_boundary_discard_stale_background_events() {
        for observed_while_inactive in [false, true] {
            let mut app = scene(false);
            let snapshot = |reset| {
                format!(
                    r#"{{"reset":{reset},"devices":[{{"id":7,"name":"Generic Bluetooth Controller","sources":16778257,"axes":[]}}]}}"#
                )
            };
            app.world()
                .resource::<Inbox>()
                .lock()
                .snapshot(&snapshot(0))
                .unwrap();
            app.update();
            key(&app, GamepadButton::RightTrigger2, true);
            app.update();
            app.update();
            assert!(app.world().resource::<Seen>().fire);
            app.world_mut().write_message(WindowFocused {
                window: Entity::PLACEHOLDER,
                focused: false,
            });
            app.update();
            if observed_while_inactive {
                app.world()
                    .resource::<Inbox>()
                    .lock()
                    .snapshot(&snapshot(1))
                    .unwrap();
                app.update();
            }
            key(&app, GamepadButton::RightTrigger2, true);
            if !observed_while_inactive {
                app.world()
                    .resource::<Inbox>()
                    .lock()
                    .snapshot(&snapshot(1))
                    .unwrap();
            }
            // In the second ordering no new metadata snapshot arrives at resume.
            app.world_mut().write_message(WindowFocused {
                window: Entity::PLACEHOLDER,
                focused: true,
            });
            app.update();
            assert!(
                !app.world().resource::<Seen>().fire,
                "old background Down cannot rearm after focus returns"
            );
            assert_eq!(
                app.world().resource::<Bindings>().device,
                InputDevice::Controller,
                "focus cleanup preserves the saved device choice"
            );
            key(&app, GamepadButton::RightTrigger2, false);
            app.update();
            key(&app, GamepadButton::RightTrigger2, true);
            app.update();
            assert!(app.world().resource::<Seen>().fire);
        }
    }
    #[test]
    fn invalidating_a_non_last_used_pad_cancels_its_contributing_holds() {
        for remove in [false, true] {
            let mut app = scene(false);
            let first = device();
            let mut second = device();
            second.id = 8;
            second.name = "Second generic controller".into();
            app.world()
                .resource::<Inbox>()
                .lock()
                .devices(vec![first, second.clone()]);
            app.update();
            app.world_mut().resource_mut::<Bindings>().device = InputDevice::Controller;
            key(&app, GamepadButton::RightTrigger2, true);
            app.update();
            assert!(app.world().resource::<Inbox>().lock().key(
                8,
                GAMEPAD_SOURCE,
                GamepadButton::South,
                true,
                false
            ));
            app.update();
            assert_eq!(app.world().resource::<State>().active, Some(8));
            app.world_mut()
                .resource_mut::<nvf_play::control::PlayerControl>()
                .ready_held = Some(0.4);
            if remove {
                app.world().resource::<Inbox>().lock().devices(vec![second]);
            } else {
                app.world()
                    .resource::<Inbox>()
                    .lock()
                    .push(Sample::Release(7));
            }
            app.update();
            assert!(
                app.world()
                    .resource::<nvf_play::control::PlayerControl>()
                    .ready_held
                    .is_none(),
                "any contributing pad invalidation cancels aggregate release actions"
            );
            assert_eq!(
                app.world().resource::<Bindings>().device,
                InputDevice::Controller,
                "the still-active second pad keeps selection"
            );
            assert!(!app.world().resource::<Seen>().fire);
        }
    }
    #[test]
    fn metadata_and_hardware_flat_ranges_select_analog_variant_without_nonfinite_values() {
        let mut app = scene(false);
        let text = r#"[{"id":7,"name":"Generic Bluetooth Controller","sources":16778257,"axes":[{"axis":12,"min":-1,"max":1,"flat":0.2},{"axis":13,"min":-1,"max":1,"flat":0.2},{"axis":17,"min":-1,"max":1,"flat":0.1}]}]"#;
        let devices = parse_devices(text).unwrap();
        assert_eq!(devices.len(), 1);
        app.world().resource::<Inbox>().lock().devices(devices);
        app.update();
        app.world_mut().resource_mut::<Bindings>().device = InputDevice::Controller;
        motion(&app, &[(12, 0.1), (13, -0.8), (17, -1.0)]);
        app.update();
        let g = app.world().get::<Gamepad>(entity(&app)).unwrap();
        assert_eq!(g.right_stick().x, 0.0);
        assert!(g.right_stick().y > 0.5);
        assert!(!g.pressed(GamepadButton::LeftTrigger2));
        motion(&app, &[(12, f32::NAN), (13, f32::INFINITY), (17, 1.0)]);
        app.update();
        let g = app.world().get::<Gamepad>(entity(&app)).unwrap();
        assert_eq!(g.right_stick(), Vec2::ZERO);
        assert!(g.pressed(GamepadButton::LeftTrigger2));
        let mut both = device();
        for axis in [11, 12, 13, 14] {
            both.ranges.insert(
                axis,
                MotionRange {
                    min: -1.0,
                    max: 1.0,
                    flat: 0.0,
                },
            );
        }
        app.world().resource::<Inbox>().lock().devices(vec![both]);
        app.update();
        motion(&app, &[(11, 0.8), (14, -0.8), (12, -0.8), (13, 0.8)]);
        app.update();
        let g = app.world().get::<Gamepad>(entity(&app)).unwrap();
        assert!(
            g.right_stick().x > 0.5 && g.right_stick().y > 0.5,
            "standard Z/RZ takes precedence when both Android axis variants exist"
        );
        let extremes = MotionRange {
            min: -f32::MAX,
            max: f32::MAX,
            flat: 0.0,
        };
        assert!(extremes.stick(f32::MAX).is_finite());
        assert!(extremes.trigger(f32::MAX).is_finite());
    }
}
