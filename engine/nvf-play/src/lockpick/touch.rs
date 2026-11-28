//! Original split-thumb lockpicking input. Puzzle rules remain in fnv-game/lockpick.
// Desktop skips `lockpick::touch::install` so touch lockpick runtime stays Android/test-only.
#![cfg_attr(not(any(target_os = "android", test)), allow(dead_code))]
use super::{LockpickInput, LockpickScreen, KEY_DEGREES};
use crate::{
    bindings::ControlInput,
    pad::ActionPads,
    pause::{AidTime, Paused},
    settings,
};
use bevy::{
    input::{
        gamepad::GamepadConnectionEvent,
        touch::{TouchInput, TouchPhase},
    },
    prelude::*,
    window::{AppLifecycle, PrimaryWindow, WindowFocused},
};
use fnv_game::lockpick::{Phase, PIN_RANGE};

const SCHEME_KEY: &str = "lockpick-touch";
const SIZE_KEY: &str = "lockpick-touch-size";
pub const HINTS_KEY: &str = "lockpick-touch-hints";
#[derive(Resource)]
pub(super) struct Hints(pub bool);
impl Hints {
    pub(super) fn for_platform(android: bool) -> Self { Self(!android) }
}
impl Default for Hints { fn default() -> Self { Self::for_platform(cfg!(target_os = "android")) } }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scheme {
    Off,
    Arc,
    Slider,
}
impl Scheme {
    const ALL: [Self; 3] = [Self::Off, Self::Arc, Self::Slider];
    fn name(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Arc => "Arc + turn",
            Self::Slider => "Precision slider + turn",
        }
    }
}
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    scheme: Scheme,
    size: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            scheme: if cfg!(target_os = "android") {
                Scheme::Arc
            } else {
                Scheme::Off
            },
            size: 1,
        }
    }
}
const SIZES: [f32; 4] = [0.75, 1.0, 1.25, 1.5];
const SIZE_NAMES: [&str; 4] = ["75%", "100%", "125%", "150%"];
/// The general Android direction overlay yields only while dedicated controls are usable.
pub fn active(screen: &LockpickScreen, options: &Options) -> bool {
    options.scheme != Scheme::Off && picking(screen)
}
fn picking(screen: &LockpickScreen) -> bool {
    screen.0.as_ref().is_some_and(|s| {
        s.help.is_none()
            && s.leaving.is_none()
            && s.ended.is_none()
            && s.game.phase == Phase::Picking
    })
}
fn pick_scheme(world: &mut World, name: &str) -> bool {
    let Some(scheme) = Scheme::ALL
        .into_iter()
        .find(|s| s.name().eq_ignore_ascii_case(name.trim()))
    else {
        return false;
    };
    if world.resource::<Options>().scheme == scheme { return true; }
    world.resource_mut::<Options>().scheme = scheme;
    let (window, ids) = if let Some(mut gestures) = world.get_resource_mut::<Gestures>() {
        let owned = (gestures.window, [gestures.angle.map(|a| a.id), gestures.turn].into_iter().flatten().collect::<Vec<_>>());
        gestures.angle = None;
        gestures.turn = None;
        owned
    } else { (None, Vec::new()) };
    if let Some(mut held) = world.get_resource_mut::<HeldTorque>() { held.touch = false; }
    crate::touch_ui_options::retire(world, crate::touch_ui_options::Feature::Lockpick, window, &ids);
    true
}
fn pick_size(world: &mut World, name: &str) -> bool {
    let Some(size) = SIZE_NAMES
        .iter()
        .position(|s| s.eq_ignore_ascii_case(name.trim()))
    else {
        return false;
    };
    world.resource_mut::<Options>().size = size;
    true
}
fn register(app: &mut App) {
    crate::touch_ui_options::install(app);
    app.init_resource::<Hints>();
    settings::add_setting(app, settings::Setting {
        key: HINTS_KEY, group: "Touch", label: "Show lockpicking hints",
        note: "Show gesture instructions and the first-use tutorial. Turn, angle, lock status and error feedback remain visible when hints are off.",
        switches: "", get: |w| w.resource::<Hints>().0,
        set: |w, on| {
            w.resource_mut::<Hints>().0 = on;
            if !on && w.get_resource::<LockpickScreen>().is_some_and(|s| s.0.as_ref().is_some_and(|s| s.help.is_some())) {
                if let Some(mut commands) = w.get_resource_mut::<bevy::ecs::message::Messages<LockpickInput>>() {
                    commands.write(LockpickInput::CloseHelp);
                }
            }
        }, slider: None,
    });
    for (key,label,note,names,chosen,pick) in [
        (SCHEME_KEY,"Lockpicking touch controls","Drag the arc to choose an angle, or use a precision slider. Hold Turn with the other thumb.",
         (|_: &World| Scheme::ALL.iter().map(|s|s.name().to_string()).collect()) as fn(&World)->Vec<String>,
         (|w: &World| w.resource::<Options>().scheme.name().to_string()) as fn(&World)->String,
         pick_scheme as fn(&mut World,&str)->bool),
        (SIZE_KEY,"Lockpicking control size","Resize the visible angle and Turn controls and their touch targets together.",
         (|_: &World| SIZE_NAMES.iter().map(|s|s.to_string()).collect()) as fn(&World)->Vec<String>,
         (|w: &World| SIZE_NAMES[w.resource::<Options>().size].to_string()) as fn(&World)->String,
         pick_size as fn(&mut World,&str)->bool),
    ] {
        settings::add_choice_list(app,settings::Setting {key,group:"Touch",label,note,switches: "", get: |_|false,set:|_,_|{},slider:None},settings::ChoiceList {names,chosen,pick,save:chosen,load:pick,ask_keep:false});
    }
}
#[derive(Resource, Default)]
pub(super) struct HeldTorque {
    touch: bool,
    pad: bool,
}
pub(super) fn torque(physical: bool, controller: bool, held: &HeldTorque, aid: bool) -> bool {
    (!controller && physical) || held.touch || held.pad || aid
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct Geometry {
    angle: Rect,
    turn: Rect,
    scale: f32,
}
impl Geometry {
    fn new(size: Vec2, options: Options) -> Option<Self> {
        if !size.is_finite() || size.min_element() < 96.0 {
            return None;
        }
        let scale = SIZES[options.size] * (size.x / 640.0).min(size.y / 360.0).min(1.0);
        let angle_size = Vec2::new(176.0, 124.0) * scale;
        let turn_size = Vec2::splat(104.0 * scale);
        let margin = 12.0 * scale;
        let left = Vec2::new(
            (size.x * 0.22).max(angle_size.x / 2.0 + margin),
            size.y * 0.52,
        );
        let right = Vec2::new(
            (size.x * 0.82).min(size.x - turn_size.x / 2.0 - margin),
            size.y * 0.52,
        );
        Some(Self {
            angle: Rect::from_center_size(left, angle_size),
            turn: Rect::from_center_size(right, turn_size),
            scale,
        })
    }
    fn pivot(self) -> Vec2 {
        Vec2::new(self.angle.center().x, self.angle.max.y - 25.0 * self.scale)
    }
    fn radius(self) -> f32 {
        self.angle.width() * 0.42
    }
    fn arc_angle(self, p: Vec2) -> f32 {
        let d = p - self.pivot();
        (d.x.atan2((-d.y).max(0.0)).to_degrees() + 90.0).clamp(0.0, PIN_RANGE)
    }
    fn point(self, angle: f32) -> Vec2 {
        let a = angle.to_radians();
        self.pivot() + Vec2::new(-a.cos(), -a.sin()) * self.radius()
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Context {
    reference: u32,
    opened: u32,
    width: u32,
    height: u32,
    ui_scale: u32,
    options: Options,
    controller: bool,
}
#[derive(Clone, Copy)]
struct AngleContact {
    id: u64,
    x: f32,
    pin: f32,
}
#[derive(Resource, Default)]
pub(super) struct Gestures {
    context: Option<Context>,
    window: Option<Entity>,
    angle: Option<AngleContact>,
    turn: Option<u64>,
    pad_armed: bool,
    suspended: bool,
    focus_lost: bool,
    controller: bool,
}
impl Gestures {
    fn cancel(&mut self) {
        self.context = None;
        self.window = None;
        self.angle = None;
        self.turn = None;
        self.pad_armed = false;
    }
    fn enter(&mut self, context: Context) -> bool {
        if self.context == Some(context) {
            return false;
        }
        self.cancel();
        self.context = Some(context);
        true
    }
    fn event(
        &mut self,
        id: u64,
        phase: TouchPhase,
        p: Vec2,
        geometry: Geometry,
        scheme: Scheme,
        pin: f32,
    ) -> Option<f32> {
        if !p.is_finite() {
            self.angle = self.angle.filter(|a| a.id != id);
            if self.turn == Some(id) {
                self.turn = None
            }
            return None;
        }
        match phase {
            TouchPhase::Started => {
                if self.angle.is_some_and(|a| a.id == id) || self.turn == Some(id) {
                    return None;
                }
                if self.angle.is_none() && geometry.angle.contains(p) {
                    self.angle = Some(AngleContact { id, x: p.x, pin });
                    if scheme == Scheme::Arc {
                        return Some(geometry.arc_angle(p));
                    }
                } else if self.turn.is_none() && geometry.turn.contains(p) {
                    self.turn = Some(id)
                }
            }
            TouchPhase::Moved => {
                if let Some(owner) = self.angle.filter(|a| a.id == id) {
                    return Some(if scheme == Scheme::Arc {
                        geometry.arc_angle(p)
                    } else {
                        (owner.pin + (p.x - owner.x) * PIN_RANGE / (geometry.angle.width() * 2.5))
                            .clamp(0.0, PIN_RANGE)
                    });
                }
                if self.turn == Some(id) && !geometry.turn.contains(p) {
                    self.turn = None
                }
            }
            TouchPhase::Ended | TouchPhase::Canceled => {
                self.angle = self.angle.filter(|a| a.id != id);
                if self.turn == Some(id) {
                    self.turn = None
                }
            }
        }
        None
    }
    fn pad(&mut self, connected: bool, stick: Vec2, trigger: f32, dt: f32) -> (f32, bool) {
        if !connected || !stick.is_finite() || !trigger.is_finite() {
            self.pad_armed = false;
            return (0.0, false);
        }
        if !self.pad_armed {
            if stick.length() <= crate::pad::STICK_DEAD && trigger <= 0.15 {
                self.pad_armed = true
            }
            return (0.0, false);
        }
        (stick.x * KEY_DEGREES * dt.clamp(0.0, 0.1), trigger > 0.55)
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Options>()
        .init_resource::<Hints>()
        .init_resource::<Gestures>()
        .init_resource::<HeldTorque>()
        .add_message::<TouchInput>()
        .add_message::<AppLifecycle>()
        .add_message::<WindowFocused>()
        .add_message::<GamepadConnectionEvent>()
        .add_systems(PreUpdate, interruptions.after(bevy::input::InputSystems))
        .add_systems(Startup, spawn)
        .add_systems(
            Update,
            draw.after(crate::PlaySet::Ui).after(crate::ui_colour::Recolour).before(crate::PlaySet::Sync),
        );
    if cfg!(target_os = "android") {
        register(app)
    }
}
#[cfg(target_os = "android")]
pub(super) fn clear(mut gestures: ResMut<Gestures>, mut held: ResMut<HeldTorque>) {
    gestures.cancel();
    *held = HeldTorque::default();
}
fn interruptions(
    mut gestures: ResMut<Gestures>,
    mut held: ResMut<HeldTorque>,
    mut lifecycle: MessageReader<AppLifecycle>,
    mut focus: MessageReader<WindowFocused>,
    mut connections: MessageReader<GamepadConnectionEvent>,
    windows: Query<Entity, With<PrimaryWindow>>,
) {
    let mut cancelled = false;
    for event in lifecycle.read() {
        match event {
            AppLifecycle::Suspended => {
                gestures.suspended = true;
                cancelled = true
            }
            AppLifecycle::Running => gestures.suspended = false,
            _ => {}
        }
    }
    let window = windows.single().ok();
    for event in focus.read().filter(|e| Some(e.window) == window) {
        gestures.focus_lost = !event.focused;
        cancelled |= !event.focused;
    }
    cancelled |= connections.read().any(GamepadConnectionEvent::disconnected);
    if cancelled {
        gestures.cancel();
        *held = HeldTorque::default()
    }
}
#[allow(clippy::too_many_arguments)]
pub(super) fn input(
    screen: Res<LockpickScreen>,
    options: Res<Options>,
    mut gestures: ResMut<Gestures>,
    mut held: ResMut<HeldTorque>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    ui_scale: Option<Res<UiScale>>,
    paused: Option<Res<Paused>>,
    controls: ControlInput,
    pads: ActionPads,
    touch_switch: Option<Res<crate::pad::TouchDeviceSwitch>>,
    platform: Option<Res<crate::pad::PlatformInputGate>>,
    ui_capture: Option<Res<crate::ui_pointer::TouchUiCapture>>,
    retirements: Option<Res<crate::touch_ui_options::Retirements>>,
    time: AidTime,
    mut touches: MessageReader<TouchInput>,
    mut out: MessageWriter<LockpickInput>,
) {
    *held = HeldTorque::default();
    let events: Vec<_> = touches.read().copied().collect();
    let Ok((window_id, window)) = windows.single() else {
        gestures.cancel();
        return;
    };
    let size = Vec2::new(window.width(), window.height());
    let ui = ui_scale.as_ref().map_or(1.0, |v| v.0);
    let Some(geometry) = Geometry::new(size, *options) else {
        gestures.cancel();
        return;
    };
    if !picking(&screen)
        || gestures.suspended
        || gestures.focus_lost
        || !window.focused
        || paused.is_some_and(|p| p.0)
        || controls.suppressed()
        || platform.as_ref().is_some_and(|gate| !gate.allows_actions())
        || !ui.is_finite()
        || ui <= 0.0
    {
        gestures.cancel();
        return;
    }
    let session = screen.0.as_ref().expect("picking session");
    let controller = controls.controller();
    gestures.controller = controller;
    let fresh = gestures.enter(Context {
        reference: session.reference.0,
        opened: session.opened.to_bits(),
        width: size.x.to_bits(),
        height: size.y.to_bits(),
        ui_scale: ui.to_bits(),
        options: *options,
        controller,
    });
    gestures.window = Some(window_id);
    if fresh {
        return;
    } // consume the opening/help-dismissal touch batch, never inherit torque
    let mut desired = None;
    if !controller && options.scheme != Scheme::Off
        && retirements.as_ref().is_none_or(|r| !r.barrier(crate::touch_ui_options::Feature::Lockpick))
        && touch_switch.as_ref().is_none_or(|switch| switch.allows_actions()) {
        for event in events.iter().filter(|e| e.window == window_id
            && ui_capture.as_ref().is_none_or(|capture| capture.allows(e.id))
            && platform.as_ref().is_none_or(|gate| gate.allows_touch(e.id))) {
            if let Some(angle) = gestures.event(
                event.id,
                event.phase,
                event.position,
                geometry,
                options.scheme,
                session.game.pin,
            ) {
                desired = Some(angle)
            }
        }
        held.touch = gestures.turn.is_some();
    } else {
        gestures.angle = None;
        gestures.turn = None;
    }
    let stick = crate::pad::stick(&pads, true);
    let trigger = pads
        .iter()
        .map(|p| p.get(GamepadButton::RightTrigger2).unwrap_or(0.0))
        .fold(0.0_f32, f32::max);
    let connected = controller && pads.iter().next().is_some();
    let (pad_turn, pad_torque) =
        gestures.pad(connected, stick, trigger, time.delta_secs_f64() as f32);
    held.pad = controller && pad_torque;
    let delta = desired.map_or(pad_turn, |angle| angle - session.game.pin);
    if delta.is_finite() && delta != 0.0 {
        out.write(LockpickInput::Turn(delta));
    }
}

#[derive(Component, Clone, Copy)]
enum Visual {
    Angle,
    Turn,
    TurnLabel,
    Guide,
    Value,
    Track,
    Knob,
    Needle,
    Dot(u8),
}
fn spawn(mut commands: Commands) {
    let mut visuals = vec![
        Visual::Angle,
        Visual::Turn,
        Visual::TurnLabel,
        Visual::Guide,
        Visual::Value,
        Visual::Track,
        Visual::Knob,
        Visual::Needle,
    ];
    visuals.extend((0..=8).map(Visual::Dot));
    for visual in visuals {
        let mut e = commands.spawn((
            visual,
            Node {
                position_type: PositionType::Absolute,
                border_radius: BorderRadius::all(px(12.0)),
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(2200),
            Pickable::IGNORE,
            UiTransform::default(),
        ));
        if matches!(visual, Visual::Angle | Visual::Turn) {
            e.insert(BorderColor::all(crate::interface::amber(255.0).with_alpha(0.65)));
        }
        match visual {
            Visual::Guide | Visual::Value | Visual::TurnLabel => {
                e.insert((
                    Text::new(""),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    TextColor(Color::WHITE),
                ));
            }
            _ => {
                e.insert(BackgroundColor(Color::WHITE));
            }
        }
    }
}
fn ui_rect(rect: Rect, scale: f32, node: &mut Node) {
    node.left = px(rect.min.x / scale);
    node.top = px(rect.min.y / scale);
    node.width = px(rect.width() / scale);
    node.height = px(rect.height() / scale);
}
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn draw(
    screen: Res<LockpickScreen>,
    options: Res<Options>,
    hints: Res<Hints>,
    held: Res<HeldTorque>,
    gestures: Res<Gestures>,
    windows: Query<&Window, With<PrimaryWindow>>,
    ui_scale: Option<Res<UiScale>>,
    paused: Option<Res<Paused>>,
    mut visuals: Query<(
        &Visual,
        &mut Node,
        &mut Visibility,
        Option<&mut Text>,
        Option<&mut BackgroundColor>,
        &mut UiTransform,
        Option<&mut TextFont>,
        Option<&mut TextColor>,
        Option<&mut BorderColor>,
    )>,
) {
    let geometry = windows
        .single()
        .ok()
        .and_then(|w| Geometry::new(Vec2::new(w.width(), w.height()), *options));
    let ui = ui_scale.as_ref().map_or(1.0, |v| v.0);
    let show = picking(&screen)
        && !gestures.suspended
        && !gestures.focus_lost
        && !paused.is_some_and(|p| p.0)
        && geometry.is_some()
        && ui.is_finite()
        && ui > 0.0;
    let pin = screen.0.as_ref().map_or(90.0, |s| s.game.pin);
    let ink = crate::interface::amber(255.0);
    for (visual, mut node, mut visibility, text, background, mut transform, font, text_color, border_color) in &mut visuals {
        let touch = options.scheme != Scheme::Off;
        let visible = show
            && match visual {
                Visual::Guide => hints.0 && (touch || gestures.controller),
                Visual::Track | Visual::Knob => options.scheme == Scheme::Slider,
                Visual::Dot(_) | Visual::Needle => options.scheme == Scheme::Arc,
                _ => touch,
            };
        *visibility = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if !visible {
            continue;
        }
        let g = geometry.expect("visible geometry");
        let s = g.scale;
        transform.rotation = Rot2::IDENTITY;
        let mut label = None;
        let mut colour = ink.with_alpha(0.95);
        let rect = match *visual {
            Visual::Angle => {
                colour = Color::BLACK.with_alpha(0.28);
                node.border_radius = BorderRadius::all(px(6.0 * s / ui));
                g.angle
            }
            Visual::Turn => {
                colour = if held.touch || held.pad {
                    ink.with_alpha(0.22)
                } else {
                    Color::BLACK.with_alpha(0.35)
                };
                node.border_radius = BorderRadius::MAX;
                g.turn
            }
            Visual::TurnLabel => {
                label = Some(if held.touch || held.pad {
                    "Turning".to_string()
                } else {
                    "Turn".to_string()
                });
                Rect::from_center_size(
                    g.turn.center(),
                    Vec2::new(g.turn.width() - 16.0 * s, 20.0 * s),
                )
            }
            Visual::Guide => {
                label = Some(if gestures.controller {
                    "Left stick: angle / Right trigger: hold to turn".to_string()
                } else if options.scheme == Scheme::Arc {
                    "Left thumb: drag arc / Right thumb: hold Turn".to_string()
                } else {
                    "Drag slider to fine-tune / Hold Turn with other thumb".to_string()
                });
                let size = windows
                    .single()
                    .map(|w| Vec2::new(w.width(), w.height()))
                    .unwrap_or(Vec2::splat(1.0));
                Rect::from_center_size(
                    Vec2::new(size.x * 0.5, size.y * 0.73),
                    Vec2::new(size.x * 0.75, 32.0 * s),
                )
            }
            Visual::Value => {
                label = Some(format!("Angle {:.0}", pin));
                Rect::from_center_size(
                    Vec2::new(g.angle.center().x, g.angle.max.y - 12.0 * s),
                    Vec2::new(100.0 * s, 20.0 * s),
                )
            }
            Visual::Track => Rect::from_center_size(
                g.angle.center(),
                Vec2::new(g.angle.width() - 32.0 * s, 2.0 * s),
            ),
            Visual::Knob => Rect::from_center_size(
                Vec2::new(
                    g.angle.min.x + 16.0 * s + (g.angle.width() - 32.0 * s) * pin / PIN_RANGE,
                    g.angle.center().y,
                ),
                Vec2::splat(10.0 * s),
            ),
            Visual::Dot(i) => {
                Rect::from_center_size(g.point(i as f32 * PIN_RANGE / 8.0), Vec2::splat(3.0 * s))
            }
            Visual::Needle => {
                let a = g.pivot();
                let b = g.point(pin);
                transform.rotation = Rot2::radians((b.y - a.y).atan2(b.x - a.x));
                Rect::from_center_size((a + b) * 0.5, Vec2::new(g.radius(), 1.5 * s))
            }
        };
        ui_rect(rect, ui, &mut node);
        if matches!(visual, Visual::Angle | Visual::Turn) {
            node.border = UiRect::all(px(s / ui));
        }
        if let Some(mut value) = text {
            if let Some(label) = label {
                value.0 = label
            }
        }
        if let Some(mut bg) = background {
            bg.0 = colour
        }
        if let Some(mut font) = font {
            font.font_size = FontSize::Px(12.0 * s / ui)
        }
        if let Some(mut text_color) = text_color { text_color.0 = ink; }
        if let Some(mut border_color) = border_color { *border_color = BorderColor::all(ink.with_alpha(0.65)); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lockpick::{PinClip, Session, Words};
    use fnv_game::lockpick::{Event, Level, Picking, Settings as Rules};
    fn options(scheme: Scheme) -> Options {
        Options { scheme, size: 1 }
    }
    fn geometry() -> Geometry {
        Geometry::new(Vec2::new(960.0, 540.0), options(Scheme::Arc)).unwrap()
    }
    fn session() -> Session {
        Session {
            reference: fnv_sim::FormId(7),
            practice: true,
            name: "Authored lock".into(),
            game: Picking::new(Level::VeryEasy, 100.0, 2, 0.5, Rules::default()),
            status: None,
            opened: 1.0,
            pin_clip: PinClip::In,
            pin_since: 1.0,
            ended: None,
            help: None,
            words: Words {
                skill: "Skill".into(),
                picks: "Pins".into(),
                level: "Level".into(),
                exit: "Leave".into(),
                force_hint: "F".into(),
                exit_hint: "E".into(),
            },
            leaving: None,
            moved: 0.0,
            aid_torque_until: None,
        }
    }
    #[derive(Resource, Default)]
    struct Outcomes(Vec<Event>);
    fn apply_adapter(
        mut screen: ResMut<LockpickScreen>,
        mut commands: MessageReader<LockpickInput>,
        held: Res<HeldTorque>,
        mut outcomes: ResMut<Outcomes>,
    ) {
        let s = screen.0.as_mut().unwrap();
        for command in commands.read() {
            if let LockpickInput::Turn(delta) = command {
                super::super::turn_pin(&mut s.game, &mut s.moved, *delta)
            }
        }
        outcomes
            .0
            .extend(s.game.step(1.0 / 60.0, torque(false, false, &held, false)));
    }
    fn app() -> (App, Entity) {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<crate::pause::PlayClock>()
            .init_resource::<crate::bindings::Bindings>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .insert_resource(LockpickScreen(Some(session())))
            .init_resource::<Outcomes>()
            .add_message::<LockpickInput>();
        install(&mut app);
        app.insert_resource(options(Scheme::Arc));
        app.add_systems(
            Update,
            (input, apply_adapter).chain().in_set(crate::PlaySet::Ui),
        );
        let window = app
            .world_mut()
            .spawn((
                Window {
                    resolution: bevy::window::WindowResolution::new(960, 540),
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        (app, window)
    }
    fn touch(app: &mut App, window: Entity, id: u64, phase: TouchPhase, p: Vec2) {
        app.world_mut().write_message(TouchInput {
            window,
            id,
            phase,
            position: p,
            force: None,
        });
    }
    fn game(app: &App) -> &Picking {
        &app.world()
            .resource::<LockpickScreen>()
            .0
            .as_ref()
            .unwrap()
            .game
    }

    #[test]
    fn controller_selection_cannot_accept_raw_touch_torque() {
        let (mut app, window) = app();
        app.world_mut().resource_mut::<crate::bindings::Bindings>().device = crate::bindings::InputDevice::Controller;
        app.update();
        touch(&mut app, window, 91, TouchPhase::Started, geometry().turn.center());
        app.update();
        assert!(!game(&app).torque);
        assert_eq!(game(&app).turn, 0.0);
        assert!(app.world().resource::<Gestures>().turn.is_none());
    }
    #[test]
    fn platform_cover_cancels_existing_torque_and_cannot_inherit_a_move() {
        let (mut app, window) = app();
        app.init_resource::<crate::pad::PlatformInputGate>();
        app.update();
        touch(&mut app, window, 92, TouchPhase::Started, geometry().turn.center());
        app.update();
        assert!(game(&app).torque);
        app.world_mut().resource_mut::<crate::pad::PlatformInputGate>().set_blocked(true);
        app.update();
        assert!(!game(&app).torque);
        assert!(app.world().resource::<Gestures>().turn.is_none());
        app.world_mut().resource_mut::<crate::pad::PlatformInputGate>().set_blocked(false);
        touch(&mut app, window, 92, TouchPhase::Moved, geometry().turn.center());
        app.update();
        assert!(!game(&app).torque);
        touch(&mut app, window, 93, TouchPhase::Started, geometry().turn.center());
        app.update();
        assert!(game(&app).torque);
    }
    #[test]
    fn opening_tap_cannot_apply_torque_but_a_fresh_hold_can() {
        let (mut app, window) = app();
        let g = geometry();
        touch(&mut app, window, 1, TouchPhase::Started, g.turn.center());
        app.update();
        assert!(!game(&app).torque);
        assert_eq!(game(&app).turn, 0.0);
        touch(&mut app, window, 1, TouchPhase::Moved, g.turn.center());
        app.update();
        assert!(!game(&app).torque);
        touch(&mut app, window, 1, TouchPhase::Ended, g.turn.center());
        app.update();
        touch(&mut app, window, 1, TouchPhase::Started, g.turn.center());
        app.update();
        assert!(game(&app).torque);
        assert!(game(&app).turn > 0.0);
        assert_eq!(game(&app).health, 1.0);
        let pin = game(&app).pin;
        touch(&mut app, window, 9, TouchPhase::Started, g.point(0.0));
        app.update();
        assert_eq!(
            game(&app).pin,
            pin,
            "angle adapter must respect torque's pin lock"
        );
        touch(&mut app, window, 1, TouchPhase::Canceled, g.turn.center());
        app.update();
        assert!(!game(&app).torque);
        touch(
            &mut app,
            Entity::PLACEHOLDER,
            20,
            TouchPhase::Started,
            g.turn.center(),
        );
        app.update();
        assert!(!game(&app).torque, "foreign window cannot own a turn hold");
        touch(&mut app, window, 21, TouchPhase::Started, g.turn.center());
        app.update();
        assert!(game(&app).torque);
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .resolution = bevy::window::WindowResolution::new(640, 360);
        app.update();
        assert!(
            !game(&app).torque,
            "resize cancels geometry-dependent ownership"
        );
        touch(&mut app, window, 21, TouchPhase::Moved, g.turn.center());
        app.update();
        assert!(!game(&app).torque);
    }
    #[test]
    fn owners_do_not_steal_and_precision_press_does_not_jump() {
        let g = geometry();
        let mut state = Gestures::default();
        assert_eq!(
            state.event(
                1,
                TouchPhase::Started,
                g.angle.center(),
                g,
                Scheme::Slider,
                70.0
            ),
            None
        );
        state.event(
            2,
            TouchPhase::Started,
            g.angle.center(),
            g,
            Scheme::Slider,
            70.0,
        );
        assert_eq!(state.angle.unwrap().id, 1);
        assert_eq!(
            state.event(
                2,
                TouchPhase::Moved,
                g.angle.center() + Vec2::X * 40.0,
                g,
                Scheme::Slider,
                70.0
            ),
            None
        );
        let moved = state
            .event(
                1,
                TouchPhase::Moved,
                g.angle.center() + Vec2::X * 20.0,
                g,
                Scheme::Slider,
                70.0,
            )
            .unwrap();
        assert!((moved - (70.0 + 20.0 * 180.0 / (g.angle.width() * 2.5))).abs() < 1e-4);
        state.event(
            3,
            TouchPhase::Started,
            g.turn.center(),
            g,
            Scheme::Slider,
            70.0,
        );
        state.event(
            3,
            TouchPhase::Started,
            g.angle.center(),
            g,
            Scheme::Slider,
            70.0,
        );
        assert_eq!(state.turn, Some(3));
        assert_eq!(state.angle.unwrap().id, 1);
        state.event(
            3,
            TouchPhase::Moved,
            g.turn.max + Vec2::splat(1.0),
            g,
            Scheme::Slider,
            70.0,
        );
        assert_eq!(state.turn, None);
        state.event(
            3,
            TouchPhase::Moved,
            g.turn.center(),
            g,
            Scheme::Slider,
            70.0,
        );
        assert_eq!(state.turn, None);
        assert!((g.arc_angle(g.point(0.0)) - 0.0).abs() < 1e-4);
        assert!((g.arc_angle(g.point(90.0)) - 90.0).abs() < 1e-4);
        assert!((g.arc_angle(g.point(180.0)) - 180.0).abs() < 1e-4);
    }
    #[test]
    fn visible_and_hit_bounds_scale_together_through_ui_scale_and_rotation() {
        for size in [
            Vec2::new(960.0, 540.0),
            Vec2::new(360.0, 800.0),
            Vec2::new(800.0, 360.0),
        ] {
            for index in 0..SIZES.len() {
                let g = Geometry::new(
                    size,
                    Options {
                        scheme: Scheme::Arc,
                        size: index,
                    },
                )
                .unwrap();
                for scale in [0.75, 1.0, 1.5, 2.0] {
                    let mut node = Node::default();
                    ui_rect(g.turn, scale, &mut node);
                    let (Val::Px(x), Val::Px(y), Val::Px(w), Val::Px(h)) =
                        (node.left, node.top, node.width, node.height)
                    else {
                        panic!("pixel rectangle")
                    };
                    assert!((Vec2::new(x, y) * scale - g.turn.min).length() < 1e-3);
                    assert!((Vec2::new(w, h) * scale - g.turn.size()).length() < 1e-3);
                    assert!(g.turn.contains(Vec2::new(x + w / 2.0, y + h / 2.0) * scale));
                }
                assert!(g.angle.min.cmpge(Vec2::ZERO).all() && g.turn.max.cmple(size).all());
                assert!(g.angle.max.x < g.turn.min.x);
            }
        }
        assert!(Geometry::new(Vec2::splat(f32::NAN), options(Scheme::Arc)).is_none());
    }
    #[test]
    fn controller_requires_neutral_after_open_or_disconnect_and_aggregates_holds() {
        let mut g = Gestures::default();
        assert_eq!(g.pad(true, Vec2::X, 1.0, 1.0 / 60.0), (0.0, false));
        assert_eq!(g.pad(true, Vec2::ZERO, 0.0, 1.0 / 60.0), (0.0, false));
        let (turn, held) = g.pad(true, Vec2::X, 1.0, 1.0 / 60.0);
        assert!(turn > 0.0 && held);
        assert_eq!(g.pad(false, Vec2::ZERO, 0.0, 1.0 / 60.0), (0.0, false));
        assert_eq!(g.pad(true, Vec2::X, 1.0, 1.0 / 60.0), (0.0, false));
        assert!(torque(
            false,
            true,
            &HeldTorque {
                touch: true,
                pad: false
            },
            false
        ));
        assert!(torque(
            false,
            true,
            &HeldTorque {
                touch: false,
                pad: true
            },
            false
        ));
        assert!(
            !torque(true, true, &HeldTorque::default(), false),
            "left-stick movement must not become torque"
        );
        assert!(torque(true, false, &HeldTorque::default(), false));
    }
    #[test]
    fn focus_background_disconnect_and_help_transitions_drop_torque() {
        let (mut app, window) = app();
        app.update();
        let g = geometry();
        touch(&mut app, window, 2, TouchPhase::Started, g.turn.center());
        app.update();
        assert!(game(&app).torque);
        app.world_mut().write_message(WindowFocused {
            window,
            focused: false,
        });
        app.update();
        assert!(!game(&app).torque);
        touch(&mut app, window, 3, TouchPhase::Started, g.turn.center());
        app.update();
        assert!(!game(&app).torque);
        app.world_mut().write_message(WindowFocused {
            window,
            focused: true,
        });
        app.update();
        touch(&mut app, window, 4, TouchPhase::Started, g.turn.center());
        app.update();
        assert!(game(&app).torque);
        app.world_mut().write_message(AppLifecycle::Suspended);
        app.update();
        assert!(!game(&app).torque);
        app.world_mut().write_message(AppLifecycle::Running);
        app.update();
        touch(&mut app, window, 5, TouchPhase::Moved, g.turn.center());
        app.update();
        assert!(!game(&app).torque);
        touch(&mut app, window, 6, TouchPhase::Started, g.turn.center());
        app.update();
        assert!(game(&app).torque);
        app.world_mut().write_message(GamepadConnectionEvent::new(
            Entity::PLACEHOLDER,
            bevy::input::gamepad::GamepadConnection::Disconnected,
        ));
        app.update();
        assert!(!game(&app).torque);
        {
            let mut screen = app.world_mut().resource_mut::<LockpickScreen>();
            screen.0.as_mut().unwrap().help = Some(super::super::Help {
            title: "Authored help".into(),
            text: "Help".into(),
            words: super::super::help::BoxWords {
                manual: "Manual".into(),
                close: "Close".into(),
                close_hint: "E".into(),
            },
            });
        }
        app.update();
        assert!(!game(&app).torque);
        app.world_mut()
            .resource_mut::<LockpickScreen>()
            .0
            .as_mut()
            .unwrap()
            .help = None;
        touch(&mut app, window, 7, TouchPhase::Started, g.turn.center());
        app.update();
        assert!(!game(&app).torque);
    }
    #[test]
    fn adapter_angle_turn_wear_break_and_open_match_unchanged_rules() {
        for angle in [0.0, 90.0] {
            let (mut app, window) = app();
            app.update();
            let g = geometry();
            touch(&mut app, window, 1, TouchPhase::Started, g.point(angle));
            app.update();
            let mut baseline = Picking::new(Level::VeryEasy, 100.0, 2, 0.5, Rules::default());
            baseline.set_pin(game(&app).pin);
            assert!((game(&app).pin - angle).abs() < 1e-3);
            touch(&mut app, window, 2, TouchPhase::Started, g.turn.center());
            for _ in 0..3000 {
                app.update();
                baseline.step(1.0 / 60.0, true);
                assert_eq!(game(&app), &baseline);
                if baseline.phase != Phase::Picking {
                    break;
                }
            }
            if angle == 0.0 {
                assert!(app
                    .world()
                    .resource::<Outcomes>()
                    .0
                    .iter()
                    .any(|e| matches!(e, Event::PinBroke { left: 1 })));
            } else {
                assert_eq!(game(&app).phase, Phase::Open);
                assert!(app
                    .world()
                    .resource::<Outcomes>()
                    .0
                    .contains(&Event::Opened));
            }
        }
    }
    #[test]
    fn settings_choices_persist_both_scheme_and_size_and_reject_unknown() {
        use settings::{Settings, SettingsFile};
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "nvf-lockpick-touch-{}-{}.ini",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let start = |path: &std::path::Path| {
            let mut app = App::new();
            app.init_resource::<Options>()
                .insert_resource(SettingsFile(Some(path.to_owned())));
            register(&mut app);
            app
        };
        let mut app = start(&path);
        let scheme = app
            .world()
            .resource::<Settings>()
            .index(SCHEME_KEY)
            .unwrap();
        let size = app.world().resource::<Settings>().index(SIZE_KEY).unwrap();
        assert!(settings::pick(
            app.world_mut(),
            scheme,
            "Precision slider + turn"
        ));
        assert!(settings::pick(app.world_mut(), size, "150%"));
        let mut loaded = start(&path);
        settings::apply_stored(loaded.world_mut());
        assert_eq!(
            *loaded.world().resource::<Options>(),
            Options {
                scheme: Scheme::Slider,
                size: 3
            }
        );
        assert!(!settings::pick(loaded.world_mut(), scheme, "unknown"));
        assert!(!settings::pick(loaded.world_mut(), size, "nan"));
        assert_eq!(loaded.world().resource::<Options>().size, 3);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn android_defaults_lockpick_hints_off_while_desktop_keeps_them_on() {
        assert!(!Hints::for_platform(true).0);
        assert!(Hints::for_platform(false).0);
    }
    include!("touch_optional_tests.rs");
    include!("touch/hints_tests.rs");
}
