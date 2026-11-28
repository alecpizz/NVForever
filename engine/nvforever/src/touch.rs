//! Touch and physical input share aggregate key/mouse state without releasing each other (#2436).
//! Positions and sensitivity are NVForever design choices, not original-game facts.
use bevy::input::{
    keyboard::{KeyboardFocusLost, KeyboardInput},
    mouse::{MouseButtonInput, MouseMotion, MouseWheel},
    touch::{TouchInput, TouchPhase},
    ButtonState, InputSystems,
};
use bevy::prelude::*;
use bevy::window::{AppLifecycle, CursorMoved, PrimaryWindow, WindowFocused};
use nvf_play::bindings::{Bindings, Control};
use std::collections::{HashMap, HashSet};

mod action_presentation;
mod aiming;
mod drawer;
mod pipboy_drag;
mod pipboy_options;
mod world_activate;
mod vats_targeting;
mod gyro;
mod safe_area;
mod tray_editor;
mod icons;
mod layout;
mod modal;
mod menu_sheets;
mod practice;
mod entrypoints;
mod profiles;
mod movement;

mod hotbar;
pub(crate) mod pipboy_model;

use layout::{Layout, Target};

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TouchInputSystems;

pub struct TouchControlsPlugin;
impl Plugin for TouchControlsPlugin {
    fn build(&self, app: &mut App) {
        drawer::install(app);
        pipboy_options::install(app);
        pipboy_drag::install(app);
        world_activate::install(app);
        vats_targeting::install(app);
        app.init_resource::<nvf_play::pipboy::data::touch_map::TouchMap>();
        icons::install(app);
        action_presentation::install(app);
        practice::install(app);
        entrypoints::install(app);
        gyro::install(app);
        safe_area::install(app);
        tray_editor::install(app);
        movement::install(app);

        hotbar::install(app);
        pipboy_model::install(app);
        menu_sheets::install(app);

        app.init_resource::<TouchState>()
            .init_resource::<HintFade>()
            .add_message::<CursorMoved>()
            .init_resource::<Layout>()
            .init_resource::<modal::Capture>()
            .init_resource::<nvf_play::pad::TouchDeviceSwitch>()
            .init_resource::<nvf_play::control::HeldSneakInput>()
            .init_resource::<Assets<Image>>()
            .add_systems(
                PreUpdate,
                (capture_physical, input).chain().in_set(TouchInputSystems).before(InputSystems),
            )
            .add_systems(
                PreUpdate,
                merge_sources
                    .after(InputSystems)
                    .before(nvf_play::pad::InputSwitchSystems::Switch)
                    .before(bevy::ui::UiSystems::Focus),
            )
            .add_systems(Startup, (layout::load, labels).chain())
            .add_systems(Update, show_labels);
    }
}
const HINT_AUTO_HIDE_SECS: f32 = 15.0;
const HINT_FADE_SECS: f32 = 1.5;
const HIDE_TOGGLE_SIZE: f32 = 44.0;

fn hide_toggle_rect(size: Vec2, safe: nvf_play::touch_safe_area::SafeInsets) -> Rect {
    let min = Vec2::new(
        (safe.left + 8.0).clamp(0.0, (size.x - HIDE_TOGGLE_SIZE).max(0.0)),
        (safe.top + 8.0).clamp(0.0, (size.y - HIDE_TOGGLE_SIZE).max(0.0)),
    );
    Rect::from_corners(min, min + Vec2::splat(HIDE_TOGGLE_SIZE))
}

#[derive(Resource, Default, Debug, Clone, Copy)]
struct HintFade {
    elapsed: f32,
    auto_hidden: bool,
    manual_override: bool,
}

impl HintFade {
    fn alpha(self, hints: bool, editing: bool) -> f32 {
        if editing {
            return 1.0;
        }
        if !hints {
            return 0.0;
        }
        if self.manual_override || self.elapsed <= HINT_AUTO_HIDE_SECS {
            return 1.0;
        }
        (1.0 - (self.elapsed - HINT_AUTO_HIDE_SECS) / HINT_FADE_SECS).clamp(0.0, 1.0)
    }

    fn tick(&mut self, dt: f32, active: bool, hints: &mut bool) {
        if *hints && self.auto_hidden {
            // Player explicitly re-enabled hints after auto-fade: respect their choice.
            self.manual_override = true;
            self.auto_hidden = false;
            return;
        }
        if !*hints || self.manual_override || !active || !dt.is_finite() || dt <= 0.0 {
            return;
        }
        self.elapsed += dt;
        if self.elapsed >= HINT_AUTO_HIDE_SECS + HINT_FADE_SECS {
            *hints = false;
            self.auto_hidden = true;
        }
    }
}

#[derive(Clone, Copy)]
struct Contact {
    origin: Vec2,
    position: Vec2,
    role: Role,
    started: f64,
    dragged: bool,
}
#[derive(Clone, Copy, Debug)]
enum Role {
    Move,
    Look,
    Button(usize),
    RunLock,
    HideToggle,
    SettingsOverlay(usize),
    DialogueOverlay(usize),
}
#[derive(Resource, Default)]
struct TouchState {
    // Raw hardware intent, sampled before aggregate touch/controller synthesis.
    // CursorMoved is physical window activity; touch-look MouseMotion is not.
    hardware_intent: bool,
    controls_hidden: bool,
    contacts: HashMap<u64, Contact>,
    keys: HashSet<KeyCode>,
    mouse: HashSet<MouseButton>,
    playing: bool,
    suspended: bool,
    saved_volume: Option<f32>,
    move_deadzone: Vec2,
    aim_latched: bool,
    toggle_aim: bool,
    hold_sneak: bool,
    movement_options: movement::Options,
    look_owner: Option<u64>,
    taps: movement::Taps,
    pulses: Vec<Control>,
    viewport: Option<Vec2>,
    interactive_actions: bool,
    ui_capture: bool,
    physical_keys: HashSet<KeyCode>,
    physical_mouse: HashSet<MouseButton>,
    managed_keys: HashSet<KeyCode>,
    managed_mouse: HashSet<MouseButton>,
    combined_keys: ButtonInput<KeyCode>,
    combined_mouse: ButtonInput<MouseButton>,
}
impl TouchState {
    fn cancel_touch(&mut self) {
        self.contacts.clear();
        self.aim_latched = false;
        self.look_owner = None;
        self.taps.clear();
        self.pulses.clear();
        self.interactive_actions = false;
    }
}
#[derive(Clone, Copy)]
struct ButtonSpec {
    text: &'static str,
    x: f32,
    y: f32,
    control: Option<Control>,
    key: Option<KeyCode>,
    playing: bool,
}
const SETTINGS_BREADCRUMB: &str = if cfg!(target_os = "android") {
    "Pause > Settings > Mobile"
} else { "Pause > Settings > Controls > Touch" };
const PAUSE_BUTTON: usize = 0;
const TAB_BUTTON: usize = 15;

fn button_active(i: usize, playing: bool, paused_now: bool) -> bool {
    if paused_now {
        if i == PAUSE_BUTTON {
            return true;
        }
        if i == TAB_BUTTON {
            return false;
        }
    }
    BUTTONS[i].playing == playing
}

const BUTTONS: &[ButtonSpec] = &[
    ButtonSpec {
        text: "Pause",
        x: 0.88,
        y: 0.03,
        control: None,
        key: Some(KeyCode::Escape),
        playing: true,
    },
    ButtonSpec {
        text: "Pip-Boy",
        x: 0.76,
        y: 0.03,
        control: Some(Control::MenuMode),
        key: None,
        playing: true,
    },
    ButtonSpec {
        text: "V.A.T.S.",
        x: 0.64,
        y: 0.03,
        control: Some(Control::Vats),
        key: None,
        playing: true,
    },
    ButtonSpec {
        text: "Reload",
        x: 0.88,
        y: 0.56,
        control: Some(Control::ReadyItem),
        key: None,
        playing: true,
    },
    ButtonSpec {
        text: "Jump",
        x: 0.76,
        y: 0.56,
        control: Some(Control::Jump),
        key: None,
        playing: true,
    },
    ButtonSpec {
        text: "Use",
        x: 0.88,
        y: 0.70,
        control: Some(Control::Activate),
        key: None,
        playing: true,
    },
    ButtonSpec {
        text: "Aim",
        x: 0.76,
        y: 0.70,
        control: Some(Control::Block),
        key: None,
        playing: true,
    },
    ButtonSpec {
        text: "Fire",
        x: 0.88,
        y: 0.84,
        control: Some(Control::Use),
        key: None,
        playing: true,
    },
    ButtonSpec {
        text: "Sneak",
        x: 0.76,
        y: 0.84,
        control: Some(Control::CrouchSneak),
        key: None,
        playing: true,
    },
    ButtonSpec {
        text: "Up",
        x: 0.12,
        y: 0.56,
        control: None,
        key: Some(KeyCode::ArrowUp),
        playing: false,
    },
    ButtonSpec {
        text: "Down",
        x: 0.12,
        y: 0.84,
        control: None,
        key: Some(KeyCode::ArrowDown),
        playing: false,
    },
    ButtonSpec {
        text: "Left",
        x: 0.00,
        y: 0.70,
        control: None,
        key: Some(KeyCode::ArrowLeft),
        playing: false,
    },
    ButtonSpec {
        text: "Right",
        x: 0.24,
        y: 0.70,
        control: None,
        key: Some(KeyCode::ArrowRight),
        playing: false,
    },
    ButtonSpec {
        text: "OK",
        x: 0.88,
        y: 0.84,
        control: None,
        key: Some(KeyCode::Enter),
        playing: false,
    },
    ButtonSpec {
        text: "Back",
        x: 0.76,
        y: 0.84,
        control: None,
        key: Some(KeyCode::Escape),
        playing: false,
    },
    ButtonSpec {
        text: "Tab",
        x: 0.88,
        y: 0.03,
        control: None,
        key: Some(KeyCode::Tab),
        playing: false,
    },
    ButtonSpec {
        text: "Page +",
        x: 0.88,
        y: 0.56,
        control: None,
        key: Some(KeyCode::PageUp),
        playing: false,
    },
    ButtonSpec {
        text: "Page -",
        x: 0.88,
        y: 0.70,
        control: None,
        key: Some(KeyCode::PageDown),
        playing: false,
    },
    ButtonSpec {
        text: "Gyro",
        x: 0.02,
        y: 0.42,
        control: None,
        key: None,
        playing: true,
    },
];
/// Record only real hardware events, before Bevy consumes them. Touch never injects
/// fake release messages into this stream; raw typing and rebinding stay physical.
fn capture_physical(
    mut state: ResMut<TouchState>,
    mut keys: MessageReader<KeyboardInput>,
    mut mouse: MessageReader<MouseButtonInput>,
    mut focus_lost: MessageReader<KeyboardFocusLost>,
    (mut cursor, mut wheel, windows): (MessageReader<CursorMoved>, MessageReader<MouseWheel>, Query<Entity, With<PrimaryWindow>>),
    previous_keys: Res<ButtonInput<KeyCode>>,
    previous_mouse: Res<ButtonInput<MouseButton>>,
) {
    let window = windows.single().ok();
    state.hardware_intent = cursor.read().any(|event| {
        Some(event.window) == window && event.delta.is_some_and(|delta| delta.is_finite() && delta.length_squared() > 0.0)
    });
    state.hardware_intent |= wheel.read().any(|event| {
        Some(event.window) == window && event.x.is_finite() && event.y.is_finite() && (event.x != 0.0 || event.y != 0.0)
    });
    // Drain the full streams even when an early event established intent.
    cursor.clear();
    wheel.clear();
    state.combined_keys = (*previous_keys).clone();
    state.combined_mouse = (*previous_mouse).clone();
    state.combined_keys.clear();
    state.combined_mouse.clear();
    for event in keys.read() {
        if event.state == ButtonState::Pressed {
            state.hardware_intent |= Some(event.window) == window;
            state.physical_keys.insert(event.key_code);
        } else {
            state.physical_keys.remove(&event.key_code);
        }
        let held =
            state.physical_keys.contains(&event.key_code) || state.keys.contains(&event.key_code);
        if held {
            state.combined_keys.press(event.key_code);
        } else {
            state.combined_keys.release(event.key_code);
        }
    }
    for event in mouse.read() {
        if event.state == ButtonState::Pressed {
            state.hardware_intent |= Some(event.window) == window;
            state.physical_mouse.insert(event.button);
        } else {
            state.physical_mouse.remove(&event.button);
        }
        let held =
            state.physical_mouse.contains(&event.button) || state.mouse.contains(&event.button);
        if held {
            state.combined_mouse.press(event.button);
        } else {
            state.combined_mouse.release(event.button);
        }
    }
    if focus_lost.read().next().is_some() {
        clear_physical(&mut state);
    }
}
fn clear_physical(state: &mut TouchState) {
    state.cancel_touch();
    state.physical_keys.clear();
    state.physical_mouse.clear();
    let keys: Vec<_> = state.combined_keys.get_pressed().copied().collect();
    for key in keys {
        if !state.keys.contains(&key) {
            state.combined_keys.release(key);
        }
    }
    let mouse: Vec<_> = state.combined_mouse.get_pressed().copied().collect();
    for button in mouse {
        if !state.mouse.contains(&button) {
            state.combined_mouse.release(button);
        }
    }
}

/// Copy the complete combined state and both edges, including press+release within
/// one frame. A touch-held action masks a physical release until the touch ends.
fn aggregate<T: Copy + Eq + std::hash::Hash + Send + Sync + 'static>(
    input: &mut ButtonInput<T>,
    managed: &HashSet<T>,
    combined: &ButtonInput<T>,
) {
    for &button in managed {
        input.reset(button);
        if combined.pressed(button) || combined.just_released(button) {
            input.press(button);
        }
        if combined.just_released(button) {
            input.release(button);
        }
        if combined.pressed(button) {
            input.press(button);
        }
        if !combined.just_pressed(button) {
            input.clear_just_pressed(button);
        }
    }
}
fn merge_sources(
    state: Res<TouchState>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    touch_device: Option<ResMut<nvf_play::pad::TouchDeviceSwitch>>,
    layout: Res<Layout>,
    practice: Res<practice::Practice>,
) {
    if let Some(mut touch_device) = touch_device {
        touch_device.set_editor_active(layout.editor.is_some() || practice.blocking());
        touch_device.set_actions(state.keys.iter().copied(), state.mouse.iter().copied());
    }
    aggregate(&mut keys, &state.managed_keys, &state.combined_keys);
    aggregate(&mut mouse, &state.managed_mouse, &state.combined_mouse);
}

/// Update virtual ownership after each touch event, preserving quick taps even if
/// both edges arrive in one frame. Hardware events are applied first, then touches.
fn sync_touch(state: &mut TouchState, bindings: Option<&Bindings>, pause: bool) {
    let mut wanted_keys = HashSet::new();
    if pause {
        wanted_keys.insert(KeyCode::Escape);
    }
    let mut wanted_mouse = HashSet::new();
    let mut controls = Vec::new();
    for c in state.contacts.values() {
        match c.role {
            Role::Move if state.playing && !state.movement_options.analog => {
                let d = c.position - c.origin;
                if d.x > state.move_deadzone.x {
                    controls.push(Control::SlideRight);
                }
                if d.x < -state.move_deadzone.x {
                    controls.push(Control::SlideLeft);
                }
                if d.y > state.move_deadzone.y {
                    controls.push(Control::Back);
                }
                if d.y < -state.move_deadzone.y {
                    controls.push(Control::Forward);
                }
            }
            Role::Button(i) | Role::DialogueOverlay(i) => {
                let b = BUTTONS[i];
                if b.playing != state.playing {
                    continue;
                }
                if let Some(key) = b.key {
                    wanted_keys.insert(key);
                }
                if let Some(control) = b.control {
                    if (control == Control::Block && state.toggle_aim)
                        || (control == Control::CrouchSneak && state.hold_sneak)
                    {
                        continue;
                    }
                    controls.push(control);
                }
            }
            Role::SettingsOverlay(i) if i != PAUSE_BUTTON => {
                if let Some(key) = BUTTONS[i].key {
                    wanted_keys.insert(key);
                }
            }
            _ => {}
        }
    }
    controls.extend(state.pulses.iter().copied());
    if state.playing && state.aim_latched {
        controls.push(Control::Block);
    }
    if let Some(bindings) = bindings {
        for control in controls {
            let b = bindings.get(control);
            if let Some(k) = b.key {
                wanted_keys.insert(k);
            } else if let Some(m) = b.mouse {
                wanted_mouse.insert(m);
            }
        }
    }
    for &key in state.keys.symmetric_difference(&wanted_keys) {
        if wanted_keys.contains(&key) || state.physical_keys.contains(&key) {
            state.combined_keys.press(key);
        } else {
            state.combined_keys.release(key);
        }
    }
    for &button in state.mouse.symmetric_difference(&wanted_mouse) {
        if wanted_mouse.contains(&button) || state.physical_mouse.contains(&button) {
            state.combined_mouse.press(button);
        } else {
            state.combined_mouse.release(button);
        }
    }
    state.managed_keys.extend(wanted_keys.iter().copied());
    state.managed_mouse.extend(wanted_mouse.iter().copied());
    state.keys = wanted_keys;
    state.mouse = wanted_mouse;
}

#[derive(bevy::ecs::system::SystemParam)]
struct WorldInputBridges<'w,'s> {
    world_touch:world_activate::Bridge<'w>,
    map:nvf_play::pipboy::data::touch_map::Bridge<'w,'s>,
    vats:vats_targeting::Bridge<'w,'s>,
}
#[derive(bevy::ecs::system::SystemParam)]
struct TouchInputState<'w, 's> {
    state: ResMut<'w, TouchState>,
    held_sneak: ResMut<'w, nvf_play::control::HeldSneakInput>,
    platform: Option<Res<'w, nvf_play::pad::PlatformInputGate>>,
    movement: movement::Bridge<'w>,
    editor_session: ResMut<'w, entrypoints::Session>,
    safe: Option<Res<'w, nvf_play::touch_safe_area::SafeInsets>>,
    creator: Option<Res<'w, nvf_play::creator::CreatorMenu>>,
    paused: Option<Res<'w, nvf_play::pause::Paused>>,
    commands: Commands<'w, 's>,
}
#[allow(clippy::too_many_arguments)]
fn input(
    TouchInputState { mut state, mut held_sneak, platform, mut movement, mut editor_session, safe, creator, paused, mut commands }: TouchInputState,
    mut layout: ResMut<Layout>,
    (ads, gyro_options): (Option<Res<nvf_play::aim_zoom::AimZoom>>, Res<gyro::Options>),
    mut touches: MessageReader<TouchInput>,
    mut lifecycle: MessageReader<AppLifecycle>,
    mut focus: MessageReader<WindowFocused>,
    volume: Option<Res<nvf_play::audio_out::SoundVolumes>>,
    window: Query<(Entity, &Window), With<PrimaryWindow>>,
    play: Option<Res<State<nvf_play::PlayState>>>,
    next_play: Option<Res<NextState<nvf_play::PlayState>>>,
    bindings: Option<Res<Bindings>>,
    mut motion: MessageWriter<MouseMotion>,
    (mut drawer,mut practice,mut modal,mut bar,mut model,mut arm,WorldInputBridges{mut world_touch,mut map,mut vats},mut sheets): (drawer::Bridge,practice::Bridge,modal::Bridge,hotbar::Bridge,pipboy_model::Bridge,pipboy_drag::Bridge,WorldInputBridges,menu_sheets::Ingress),
) {
    sheets.reset();
    vats.begin(state.hardware_intent, touches.is_empty());
    let modal_captures = modal.refresh();
    let previous_move = movement.clear_sample();
    state.pulses.clear(); state.interactive_actions = false; state.ui_capture = false;
    let options = movement.options();
    if state.movement_options != options { world_touch.cancel(); map.cancel(); state.cancel_touch(); state.movement_options = options; }

    let Ok((window_id, window)) = window.single() else {
        vats.cancel();
        model.cancel();
        arm.cancel();
        bar.cancel();
        practice.cancel();
        drawer.drawer.cancel();
        touches.clear();
        world_touch.cancel(); map.cancel(); state.cancel_touch();
        held_sneak.active = false;
        sync_touch(&mut state, bindings.as_deref(), false);
        return;
    };
    let size = Vec2::new(window.width().max(1.0), window.height().max(1.0));
    modal.prepare_retirements(window_id, state.contacts.keys().copied());
    let resized = state.viewport.is_some_and(|previous| previous != size);
    state.viewport = Some(size);
    // Bevy applies StateTransition after PreUpdate. Release old-mode contacts for
    // the pending transition now, before the new state's gameplay/UI reads input.
    let effective_play = match next_play.as_deref() {
        Some(NextState::Pending(next) | NextState::PendingIfDifferent(next)) => Some(*next),
        _ => play.map(|state| *state.get()),
    };
    let playing = effective_play == Some(nvf_play::PlayState::Playing);
    let mut just_suspended = false;
    for event in lifecycle.read() {
        match event {
            AppLifecycle::Suspended => {
                just_suspended = !state.suspended;
                state.suspended = true;
                clear_physical(&mut state);
                if let Some(volume) = &volume {
                    if state.saved_volume.is_none() {
                        state.saved_volume = Some(volume.master());
                    }
                    volume.set_master(0.0);
                }
            }
            AppLifecycle::Running => {
                state.suspended = false;
                if let (Some(saved), Some(volume)) = (state.saved_volume.take(), &volume) {
                    volume.set_master(saved);
                }
            }
            _ => {}
        }
    }
    let mut lost_focus = false;
    for event in focus.read().filter(|f| f.window == window_id) {
        drawer.drawer.set_focus(event.focused);
        lost_focus |= !event.focused;
    }
    let suspended = state.suspended;
    let interactive = !suspended && window.focused && !lost_focus && size.min_element() >= 64.0;
    let safe_insets = safe.as_deref().copied().unwrap_or_default();
    let toggle_rect = hide_toggle_rect(size, safe_insets);
    let creator_open = creator.as_deref().is_some_and(|c| c.is_open()) && !paused.as_deref().is_some_and(|p| p.0);
    if layout.editor.is_some() {
        state.controls_hidden = false;
    }
    let controller = bindings
        .as_deref()
        .is_some_and(|b| b.device == nvf_play::bindings::InputDevice::Controller);
    if !interactive || resized { editor_session.interrupt(&mut layout); }
    let sheet_menu = sheets.prepare(window_id, window, interactive && !resized && layout.editor.is_none(), practice.form_over());
    arm.prepare(interactive&&!resized&&!controller&&!creator_open&&!state.controls_hidden&&layout.editor.is_none()&&!modal_captures&&!sheet_menu
        &&(playing||arm.inventory()),platform.as_deref());
    let drawer_allowed =
        playing && interactive && !controller && !creator_open && !state.controls_hidden && layout.editor.is_none() && drawer.allowed();
    if !drawer_allowed {
        drawer.drawer.cancel();
    }
    if playing != state.playing || !interactive || controller || creator_open {
        world_touch.cancel(); map.cancel(); state.cancel_touch();
    }
    state.playing = playing;
    if lost_focus {
        clear_physical(&mut state);
    }
    if state.toggle_aim != layout.config.aiming.toggle_aim
        || state.hold_sneak != layout.config.aiming.hold_sneak
    {
        world_touch.cancel(); map.cancel(); state.cancel_touch();
    }
    state.toggle_aim = layout.config.aiming.toggle_aim;
    state.hold_sneak = layout.config.aiming.hold_sneak;
    state.move_deadzone = Vec2::splat(
        layout.config.diameter(Target::Stick, size) * 0.5 * layout.config.aiming.dead_zone,
    ) / size;
    let mut reserved = Vec::new();
    if options.run_lock { reserved.push((movement::fixed_lock_rect(&layout,size),0.0)); }
    if layout.config.floating {
        if let Some(contact)=state.contacts.values().find(|c|matches!(c.role,Role::Move)) {
            let origin=contact.origin*size;
            let radius=layout.config.diameter(Target::Stick,size)*0.65;
            if origin != layout.config.center(Target::Stick,size) {
                reserved.push((Rect::from_center_size(origin,Vec2::splat(radius*2.0)),radius));
            }
        }
    }
    bar.prepare(size, interactive && !controller && !creator_open && !state.controls_hidden && !sheet_menu && (layout.editor.is_none() || layout.tray_editing()) && !modal_captures
        && platform.as_ref().is_none_or(|gate|gate.allows_actions()), playing,
        &layout.config,gyro_options.enabled,&reserved);
    model.prepare(interactive && !controller && !sheet_menu && layout.editor.is_none() && !modal_captures
        && platform.as_ref().is_none_or(|gate|gate.allows_actions())
        && !bar.captures_actions() && state.physical_mouse.is_empty() && state.physical_keys.is_empty(), size, touches.is_empty());
    arm.prepare_regions(size, model.dismissal_display(), |rect| model.closing_region_unclaimed(rect)
        && !bar.items_pending() && !bar.blocks(rect));
    state.ui_capture = bar.captures_actions();
    if state.ui_capture {
        world_touch.cancel(); map.cancel(); state.cancel_touch(); held_sneak.active = false; drawer.drawer.cancel();
    }
    let mut pause = just_suspended && playing;
    sync_touch(&mut state, bindings.as_deref(), pause);
    if resized {
        vats.cancel();
        practice.cancel(); world_touch.cancel(); map.cancel(); state.cancel_touch(); drawer.drawer.cancel(); touches.clear(); held_sneak.active = false;
        sync_touch(&mut state, bindings.as_deref(), pause); return;
    }
    if platform.as_ref().is_some_and(|gate| !gate.allows_actions() && (gate.native_blocked() || !arm.captures())) {
        vats.cancel();
        arm.cancel();
        bar.cancel();
        practice.cancel();
        drawer.drawer.cancel();
        world_touch.cancel(); map.cancel();
        state.cancel_touch();
        held_sneak.active = false;
        sync_touch(&mut state, bindings.as_deref(), false);
        touches.clear();
        return;
    }
    if editor_session.blocking() && editor_session.discard_batch() {
        vats.cancel();
        sheets.cancel(); arm.cancel(); model.cancel(); bar.cancel(); drawer.drawer.cancel();
        world_touch.cancel(); map.cancel(); state.cancel_touch(); held_sneak.active = false;
        sync_touch(&mut state, bindings.as_deref(), false);
        for event in touches.read() { editor_session.record(event); }
        touches.clear(); return;
    }
    if practice.capture(&mut touches, window_id, size, interactive,
        effective_play == Some(nvf_play::PlayState::Menu)) {
        vats.cancel();
        sheets.cancel();
        arm.cancel();
        bar.cancel();
        drawer.drawer.cancel();
        world_touch.cancel(); map.cancel(); state.cancel_touch();
        held_sneak.active = false;
        sync_touch(&mut state, bindings.as_deref(), false);
        return;
    }
    if creator_open && layout.editor.is_none() && !practice.blocking() {
        // Character appearance customizer handles its own touches directly.
        vats.cancel(); arm.cancel(); model.cancel(); bar.cancel(); drawer.drawer.cancel();
        world_touch.cancel(); map.cancel(); state.cancel_touch(); held_sneak.active = false;
        sync_touch(&mut state, bindings.as_deref(), false);
        return;
    }
    let paused_now = paused.as_deref().is_some_and(|p| p.0);
    if sheets.settings() && !controller && !modal_captures {
        // Settings has its own in-page controls. The overlay still owns and
        // paints its press; tapping Pause toggles the Pause menu closed cleanly.
        for t in touches.read() {
            if t.window != window_id || modal.tail(t) || !t.position.is_finite()
                || platform.as_ref().is_some_and(|gate| !gate.allows_touch(t.id)) {
                continue;
            }
            if t.phase == TouchPhase::Started && !state.contacts.contains_key(&t.id) { sheets.release_overlay(t.id); }
            if sheets.overlay_owns(t.id) {
                match t.phase {
                    TouchPhase::Moved => {
                        if let Some(c) = state.contacts.get_mut(&t.id) {
                            let p = t.position / size;
                            c.dragged |= ((p - c.origin) * size).length() > 12.0;
                            c.position = p;
                        }
                    }
                    TouchPhase::Ended => {
                        if let Some(c) = state.contacts.remove(&t.id) {
                            if matches!(c.role, Role::HideToggle) && !c.dragged && toggle_rect.contains(t.position) {
                                state.controls_hidden = !state.controls_hidden;
                                if state.controls_hidden { state.cancel_touch(); }
                            } else if paused_now
                                && matches!(c.role, Role::SettingsOverlay(PAUSE_BUTTON))
                                && !c.dragged
                                && layout.config.button_hit(t.position, size, PAUSE_BUTTON, false)
                            {
                                commands.queue(nvf_play::pause::close_pause);
                            }
                        }
                    }
                    TouchPhase::Canceled => { state.contacts.remove(&t.id); }
                    _ => {}
                }
            } else if sheets.tail(t) {
                continue;
            } else if t.phase == TouchPhase::Started && toggle_rect.contains(t.position) {
                sheets.claim_overlay(t.id);
                let p = t.position / size;
                state.contacts.insert(t.id, Contact { origin: p, position: p,
                    role: Role::HideToggle, started: movement.now(), dragged: false });
            } else if paused_now && !state.controls_hidden && t.phase == TouchPhase::Started
                && layout.config.opacity > 0.0
                && layout.config.button_hit(t.position, size, PAUSE_BUTTON, false)
            {
                sheets.claim_overlay(t.id);
                let p = t.position / size;
                state.contacts.insert(t.id, Contact { origin: p, position: p,
                    role: Role::SettingsOverlay(PAUSE_BUTTON), started: movement.now(), dragged: false });
            } else if let Some(i) = (!state.controls_hidden && t.phase == TouchPhase::Started && layout.config.opacity > 0.0)
                .then(|| layout.config.hit_filtered(t.position, size, false, false, |i| !(paused_now && i == TAB_BUTTON))).flatten()
                .and_then(|hit| match hit { Target::Button(i) => Some(i), _ => None })
            {
                sheets.claim_overlay(t.id);
                let p = t.position / size;
                state.contacts.insert(t.id, Contact { origin: p, position: p,
                    role: Role::SettingsOverlay(i), started: movement.now(), dragged: false });
            } else {
                sheets.settings_event(t);
            }
            sync_touch(&mut state, bindings.as_deref(), false);
        }
        vats.cancel(); arm.cancel(); model.cancel(); bar.cancel(); drawer.drawer.cancel();
        world_touch.cancel(); map.cancel(); held_sneak.active = false;
        return;
    }
    if sheets.capture(&mut touches) {
        vats.cancel();
        arm.cancel(); model.cancel(); bar.cancel(); drawer.drawer.cancel(); map.cancel();
        world_touch.cancel(); state.cancel_touch(); state.ui_capture = true; held_sneak.active = false;
        sync_touch(&mut state, bindings.as_deref(), false);
        return;
    }
    if modal_captures {
        vats.cancel();
        arm.cancel();
        bar.cancel();
        drawer.drawer.cancel();
        if modal.dialogue_only() && !controller && layout.editor.is_none() {
            state.contacts.retain(|_, c| matches!(c.role, Role::DialogueOverlay(_)));
            state.aim_latched = false;
            state.look_owner = None;
            state.taps.clear();
            world_touch.cancel(); map.cancel(); held_sneak.active = false;
            sync_touch(&mut state, bindings.as_deref(), false);
            for t in touches.read() {
                if t.window != window_id || platform.as_ref().is_some_and(|gate| !gate.allows_touch(t.id)) {
                    continue;
                }
                if t.phase == TouchPhase::Started && !state.contacts.contains_key(&t.id) { sheets.release_overlay(t.id); }
            if sheets.overlay_owns(t.id) {
                match t.phase {
                    TouchPhase::Moved => {
                        if let Some(c) = state.contacts.get_mut(&t.id) {
                            let p = t.position / size;
                            c.dragged |= ((p - c.origin) * size).length() > 12.0;
                            c.position = p;
                        }
                    }
                    TouchPhase::Ended => {
                        if let Some(c) = state.contacts.remove(&t.id) {
                            if matches!(c.role, Role::HideToggle) && !c.dragged && toggle_rect.contains(t.position) {
                                state.controls_hidden = !state.controls_hidden;
                                if state.controls_hidden { state.cancel_touch(); }
                            }
                        }
                    }
                    TouchPhase::Canceled => { state.contacts.remove(&t.id); }
                    _ => {}
                }
            } else if !sheets.tail(t) && !modal.tail(t) {
                if t.phase == TouchPhase::Started && t.position.is_finite() && interactive && toggle_rect.contains(t.position) {
                    sheets.claim_overlay(t.id);
                    let p = t.position / size;
                    state.contacts.insert(t.id, Contact { origin: p, position: p,
                        role: Role::HideToggle, started: movement.now(), dragged: false });
                } else {
                    let overlay = (!state.controls_hidden && t.phase == TouchPhase::Started && t.position.is_finite() && interactive
                        && !state.ui_capture && layout.config.opacity > 0.0)
                        .then(|| layout.config.hit(t.position, size, false, false)).flatten()
                        .and_then(|hit| match hit { Target::Button(i) => Some(i), _ => None });
                    if let Some(i) = overlay {
                        sheets.claim_overlay(t.id);
                        let p = t.position / size;
                        state.contacts.insert(t.id, Contact { origin: p, position: p,
                            role: Role::DialogueOverlay(i), started: movement.now(), dragged: false });
                    } else {
                        modal.capture_event(t);
                    }
                }
            }
                sync_touch(&mut state, bindings.as_deref(), false);
            }
            return;
        }
        world_touch.cancel(); map.cancel(); state.cancel_touch();
        held_sneak.active = false;
        sync_touch(&mut state, bindings.as_deref(), false);
        modal.capture_batch(&mut touches);
        return;
    }
    // The first touch restores the overlay/device; do not let it also fire or open
    // an editor whose pause edge is consumed by the switching frame.
    if controller {
        vats.cancel();
        arm.cancel();
        bar.cancel();
        drawer.drawer.cancel();
        world_touch.cancel(); map.cancel(); state.cancel_touch();
        held_sneak.active = false;
        sync_touch(&mut state, bindings.as_deref(), pause);
        for event in touches.read() { vats.consume_return(event, window_id); }
        return;
    }
    if arm.capture_batch(&mut touches,window_id,size) {
        vats.cancel();
        bar.cancel(); drawer.drawer.cancel(); state.ui_capture=true;
        world_touch.cancel(); map.cancel(); state.cancel_touch(); held_sneak.active=false;
        sync_touch(&mut state,bindings.as_deref(),false);
        return;
    }
    if bar.take_editor_request() {
        vats.cancel();
        if interactive {match layout.open_tray() {Ok(())=>{pause|=playing;},Err(error)=>layout.message=error}}
        arm.cancel();model.cancel();world_touch.cancel(); map.cancel();bar.cancel();drawer.drawer.cancel();state.cancel_touch();held_sneak.active=false;
        sync_touch(&mut state,bindings.as_deref(),pause);touches.clear();return;
    }
    state.interactive_actions = playing && interactive && layout.editor.is_none() && !state.ui_capture;
    world_touch.prepare(state.interactive_actions,size);
    map.prepare(interactive && !controller && !modal_captures && layout.editor.is_none()
        && !bar.captures_actions() && !bar.items_pending() && !arm.captures()
        && model.map_projection_ready(),layout.config.profile_epoch);
    state.ui_capture|=map.captures();
    vats.prepare(window_id, window, state.interactive_actions && !state.ui_capture
        && !arm.captures() && !bar.items_pending(), touches.is_empty(),
        state.hardware_intent, !state.physical_keys.is_empty() || !state.physical_mouse.is_empty());
    let mut drawer_committed = false;
    let mut gesture_committed = false;
    let mut editor_closed=false;
    for t in touches.read() {
        editor_session.record(t);
        if modal.tail(t) { vats.cancel(); continue; }
        if t.window != window_id || platform.as_ref().is_some_and(|gate| !gate.allows_touch(t.id)) {
            vats.cancel();
            continue;
        }
        if sheets.tail(t) { vats.cancel(); continue; }
        // A claim outlives its contact only to swallow that contact's own tail. A
        // fresh Start reusing the id (Android reuses pointer ids) is a new touch.
        if t.phase == TouchPhase::Started && !state.contacts.contains_key(&t.id) { sheets.release_overlay(t.id); }
        let owned_overlay = sheets.overlay_owns(t.id);
        if owned_overlay && !state.contacts.contains_key(&t.id) { continue; }
        if arm.captures(){vats.cancel();arm.event(t,size);continue;}
        if !t.position.is_finite() { vats.cancel(); world_touch.cancel(); map.cancel(); state.cancel_touch(); sync_touch(&mut state, bindings.as_deref(), false); continue; }
        let p = t.position / size;
        let map_unclaimed=layout.config.hit_filtered(t.position,size,playing,false,|i|i!=gyro::BUTTON||gyro_options.enabled).is_none()
            && !bar.blocks(Rect::from_center_size(t.position,Vec2::splat(1.0)));
        if map.captures() {
            vats.cancel();
            map.event(t,map_unclaimed);world_touch.cancel();state.cancel_touch();held_sneak.active=false;
            sync_touch(&mut state,bindings.as_deref(),pause);continue;
        }
        if state.ui_capture {
            vats.cancel();
            // The UI owner still receives its close/drag events; background routes do not.
            bar.event(t,false);
            world_touch.cancel(); map.cancel(); state.cancel_touch(); held_sneak.active = false; drawer.drawer.cancel();
            sync_touch(&mut state,bindings.as_deref(),pause);
            continue;
        }
        if layout.editor.is_some() {
            vats.cancel();
            bar.cancel();
            drawer.drawer.cancel();
            if interactive {layout.event(t,size);} else {layout.cancel_editor_drag();}
            model.cancel();world_touch.cancel(); map.cancel(); state.cancel_touch();
            sync_touch(&mut state, bindings.as_deref(), pause);
            if layout.editor.is_none() {editor_session.closed();editor_closed=true;break;}
            continue;
        }
        if !owned_overlay {
        // A restored presentation and existing body/Fire owner precede new
        // geometry. The actual Fire contact continues outside its button.
        if vats.returning(t) || (vats.owns(t.id) && vats.event(t, false, false)) {
            world_touch.cancel(); map.cancel(); state.cancel_touch(); held_sneak.active = false;
            sync_touch(&mut state, bindings.as_deref(), pause); continue;
        }
        if drawer_allowed {
            let unclaimed = layout.config.hit_filtered(t.position,size,playing,false,
                |i| i != gyro::BUTTON || gyro_options.enabled).is_none();
            let arm_unclaimed=unclaimed
                && !bar.blocks(Rect::from_center_size(t.position,Vec2::splat(1.0)));
            if arm.begin(t,size,arm_unclaimed,platform.as_deref()) {
                vats.cancel();
                bar.cancel();drawer.drawer.cancel();state.ui_capture=true;
                world_touch.cancel(); map.cancel(); state.cancel_touch();held_sneak.active=false;
                sync_touch(&mut state,bindings.as_deref(),false);
                continue;
            }
            let (consumed, opened) = drawer.event(t, size, arm_unclaimed);
            if consumed {
                vats.cancel();
                if opened {
                    drawer.drawer.cancel();
                    world_touch.cancel(); map.cancel(); state.cancel_touch();
                    held_sneak.active = false;
                }
                sync_touch(&mut state, bindings.as_deref(), pause);
                if opened {
                    // Share queued-request admission before leaving this input
                    // batch, including its projected item pointer and raw gyro.
                    state.ui_capture = drawer.opens_tray() && bar.apply_tray_request(true);
                    drawer_committed = true;
                    break;
                }
                continue;
            }
        }
        let unclaimed=layout.config.hit_filtered(t.position,size,playing,false,
            |i| i != gyro::BUTTON || gyro_options.enabled).is_none();
        if model.event(t, unclaimed && !bar.items_pending()
            && !bar.blocks(Rect::from_center_size(t.position, Vec2::splat(1.0)))) {
            vats.cancel();
            state.taps.clear();
            continue;
        }
        if arm.inventory() && !bar.items_pending() && !bar.blocks(Rect::from_center_size(t.position,Vec2::splat(1.0)))
            && model.closing_unclaimed(t.position) && arm.begin(t,size,unclaimed,platform.as_deref()) {
            vats.cancel();
            model.cancel();bar.cancel();drawer.drawer.cancel();state.ui_capture=true;
            world_touch.cancel(); map.cancel();state.cancel_touch();held_sneak.active=false;
            sync_touch(&mut state,bindings.as_deref(),false);continue;
        }
        // Hotbar's event also feeds ordinary TouchItems when its optional bar
        // is disabled. Map canvas owns its gestures before that generic route;
        // actual slot/tray/utility geometry and existing item contacts win.
        if map.event(t,map_unclaimed) {
            vats.cancel();
            state.ui_capture=true;world_touch.cancel();state.cancel_touch();held_sneak.active=false;drawer.drawer.cancel();
            sync_touch(&mut state,bindings.as_deref(),pause);continue;
        }
        let consumed = bar.event(t,unclaimed);
        if bar.captures_actions() {
            // Capture can begin on a long-press release midway through this batch.
            state.ui_capture = true;
            world_touch.cancel(); map.cancel(); state.cancel_touch(); held_sneak.active = false; drawer.drawer.cancel();
            sync_touch(&mut state,bindings.as_deref(),pause);
        }
        if consumed || state.ui_capture { vats.cancel(); world_touch.cancel(); map.cancel(); state.taps.clear(); continue; }
        let fire_hit = matches!(layout.config.hit_filtered(t.position,size,playing,false,
            |i| i != gyro::BUTTON || gyro_options.enabled), Some(Target::Button(i)) if BUTTONS[i].control == Some(Control::Use));
        if vats.event(t, fire_hit, unclaimed) {
            world_touch.cancel(); map.cancel(); state.cancel_touch(); held_sneak.active = false;
            sync_touch(&mut state, bindings.as_deref(), pause); continue;
        }
        let run_lock_hit=playing&&options.run_lock&&movement::lock_available(&layout,&state,size)
            &&movement::lock_rect(&layout,&state,size).contains(t.position);
        let world_eligible=unclaimed&&playing&&interactive&&state.contacts.is_empty()&&!run_lock_hit
            &&!(layout.config.floating&&layout.config.movement_area(p));
        if world_touch.event(t,world_eligible)&&t.phase!=TouchPhase::Started {
            if let Some(contact)=state.contacts.get_mut(&t.id){contact.dragged=true;}
        }
        }
        match t.phase {
            TouchPhase::Started if interactive => {
                if state.contacts.contains_key(&t.id) { continue; }
                let role = if toggle_rect.contains(t.position) {
                    Some(Role::HideToggle)
                } else if state.controls_hidden || layout.config.opacity <= 0.0 {
                    None
                } else if playing && options.run_lock && movement::lock_available(&layout, &state, size) && !bar.blocks(movement::lock_rect(&layout,&state,size)) && movement::lock_rect(&layout, &state, size).contains(t.position) {
                    Some(Role::RunLock)
                } else if paused_now && layout.config.button_hit(t.position, size, PAUSE_BUTTON, false) {
                    Some(Role::Button(PAUSE_BUTTON))
                } else { match layout.config.hit_filtered(t.position,size,playing,false,
                    |i| (i != gyro::BUTTON || gyro_options.enabled) && !(paused_now && i == TAB_BUTTON)) {

                    Some(Target::Button(i)) => Some(Role::Button(i)),
                    Some(Target::Stick) => Some(Role::Move),
                    Some(Target::Tray) => None,
                    None if playing && layout.config.floating && layout.config.movement_area(p) => {
                        Some(Role::Move)
                    }
                    None if playing && layout.config.look_area(p) => Some(Role::Look),
                    _ => None,
                }};
                let role = role.filter(|role| {
                    !(matches!(role, Role::Look) && state.look_owner.is_some()) && !state.contacts.values().any(|c| {
                        matches!(
                            (role, c.role),
                            (Role::Move, Role::Move) | (Role::Look, Role::Look)
                        )
                    })
                });
                if let Some(role) = role {
                    if matches!(role, Role::Button(_) | Role::RunLock | Role::HideToggle)
                        || (matches!(role, Role::Move) && layout.config.hit(t.position, size, playing, false) == Some(Target::Stick)) {
                        sheets.claim_overlay(t.id);
                    }
                    let multiple = !state.contacts.is_empty();
                    if !matches!(role, Role::Look) || multiple { state.taps.clear(); }
                    if multiple { for c in state.contacts.values_mut() { if matches!(c.role, Role::Look) { c.dragged = true; } } }
                    if (matches!(role, Role::Look) || (options.fire_drag && matches!(role, Role::Button(i) if BUTTONS[i].control == Some(Control::Use))))
                        && state.look_owner.is_none() {
                        state.look_owner = Some(t.id);
                    }
                    if let Role::Button(i) = role {
                        if state.toggle_aim
                            && BUTTONS[i].control == Some(Control::Block)
                            && !controller
                        {
                            let already_down = state.contacts.values().any(|c| matches!(c.role, Role::Button(j) if BUTTONS[j].control == Some(Control::Block)));
                            if !already_down {
                                state.aim_latched = !state.aim_latched;
                            }
                        }
                    }
                    state.contacts.insert(
                        t.id,
                        Contact {
                            origin: if matches!(role, Role::Move) && !layout.config.floating {
                                layout.config.center(Target::Stick, size) / size
                            } else {
                                p
                            },
                            position: p,
                            role, started: movement.now(), dragged: multiple && matches!(role, Role::Look),
                        },
                    );
                }
            }
            TouchPhase::Moved => {
                let owns_look = state.look_owner == Some(t.id);
                if let Some(c) = state.contacts.get_mut(&t.id) {
                    c.dragged |= ((p - c.origin) * size).length() > 12.0;
                    if owns_look && playing && interactive {
                        motion.write(MouseMotion {
                            delta: (p - c.position)
                                * size
                                * layout
                                    .config
                                    .aiming
                                    .look_scale(ads.as_deref().is_some_and(|a| a.aimed)),
                        });
                    }
                    c.position = p;
                }
            }
            TouchPhase::Ended => {
                let lock = movement::lock_rect(&layout, &state, size);
                let lock_available = movement::lock_available(&layout, &state, size) && !bar.blocks(lock);
                let contact = state.contacts.remove(&t.id);
                if state.look_owner == Some(t.id) { state.look_owner = None; }
                if let Some(c) = contact {
                    let dragged = c.dragged || ((p-c.origin)*size).length() > 12.0;
                    if matches!(c.role, Role::HideToggle) {
                        if interactive && !dragged && toggle_rect.contains(t.position) {
                            state.controls_hidden = !state.controls_hidden;
                            if state.controls_hidden {
                                vats.cancel();
                                bar.cancel();
                                drawer.drawer.cancel();
                                world_touch.cancel();
                                state.cancel_touch();
                                held_sneak.active = false;
                            }
                        }
                    } else if paused_now
                        && matches!(c.role, Role::Button(PAUSE_BUTTON))
                        && interactive
                        && !dragged
                        && layout.config.button_hit(t.position, size, PAUSE_BUTTON, false)
                    {
                        commands.queue(nvf_play::pause::close_pause);
                    } else if playing && interactive && options.run_lock && lock_available && lock.contains(t.position)
                        && ((matches!(c.role, Role::Move) && !movement.auto_move()) || (matches!(c.role, Role::RunLock) && !dragged))
                        && !state.pulses.contains(&Control::AutoMove) {
                        state.pulses.push(Control::AutoMove);
                    }
                    if playing && interactive && !state.controls_hidden && options.vats_taps && matches!(c.role, Role::Look) && !dragged
                        && state.contacts.is_empty() && movement.now() - c.started <= 0.25 {
                        if state.taps.tap(movement.now(), t.position) {
                            world_touch.cancel(); map.cancel(); state.cancel_touch(); state.pulses.push(Control::Vats);
                            gesture_committed = true; break;
                        }
                    } else if matches!(c.role, Role::Look) { state.taps.clear(); }
                }
            }
            TouchPhase::Canceled => {
                state.contacts.remove(&t.id);
                if state.look_owner == Some(t.id) { state.look_owner = None; }
                state.taps.clear();
                state.aim_latched = false;
            }
            _ => {}
        }
        sync_touch(&mut state, bindings.as_deref(), pause);
    }
    // Drop the unread tail too: breaking the iterator alone would replay it on the
    // next frame, before or after the ordinary menu consumer applies its request.
    if drawer_committed || gesture_committed || editor_closed {
        if editor_closed { for event in touches.read() { editor_session.record(event); } }
        touches.clear();
    }
    if options.analog && state.interactive_actions && !state.ui_capture {
        let axes = state.contacts.values().find(|c| matches!(c.role, Role::Move)).map_or(Vec2::ZERO, |c|
            movement::axes((c.position-c.origin)*size, layout.config.diameter(Target::Stick, size)*0.5, layout.config.aiming.dead_zone));
        movement.publish(axes, previous_move);
    }
    sync_touch(&mut state, bindings.as_deref(), pause);
    held_sneak.active = state.hold_sneak && playing && interactive && !controller
        && layout.editor.is_none() && !state.ui_capture && state.contacts.values().any(|c| matches!(c.role, Role::Button(i) if BUTTONS[i].control == Some(Control::CrouchSneak)));
}

#[derive(Component, Clone, Copy)]
pub(super) enum TouchVisual {
    EditorShield,
    Button(usize),
    HideToggle,
    HideToggleLabel,
    Caption,
    Glyph,
    Stick,
    Knob,
    Instructions,
    EditorPanel,
    EditorAction(usize),
    EditorToggle,
    EditorDock,
    EditorMessage,
    AimingValue(bool),
}
fn labels(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.spawn((Node {position_type:PositionType::Absolute,width:percent(100.0),height:percent(100.0),..default()},GlobalZIndex(2000),TouchVisual::EditorShield));
    for (i, b) in BUTTONS.iter().enumerate() {
        commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    border: UiRect::all(px(1.5)),
                    border_radius: BorderRadius::MAX,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.02, 0.06, 0.07, 0.7)),
                BorderColor::all(Color::srgba(0.7, 0.9, 0.85, 0.7)),
                GlobalZIndex(2500),
                TouchVisual::Button(i),
            ))
            .with_children(|p| {
                let semantic = if i == gyro::BUTTON { icons::semantic_index("gyro") } else { Some(i) };
                if let Some(index) = semantic {
                    p.spawn((
                        ImageNode::default(), icons::TouchIcon(index), TouchVisual::Glyph,
                        Node { width: percent(66.0), height: percent(66.0), ..default() },
                        Pickable::IGNORE,
                    ));
                } else {
                    // Explicit readable fallback; a local action index is not a glyph ID.
                    p.spawn((ImageNode::new(images.add(Image::transparent())), TouchVisual::Glyph,
                        Node { align_items: AlignItems::Center, justify_content: JustifyContent::Center, ..default() },
                        Pickable::IGNORE)).with_children(|glyph| {
                            glyph.spawn((Text::new("GYR"), TextFont { font_size: FontSize::Px(14.0), ..default() },
                                TextColor(Color::WHITE), gyro::Fallback, Pickable::IGNORE));
                        });
                }

                p.spawn((
                    Text::new(b.text),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                    TextColor(Color::WHITE),
                    Node {
                        position_type: PositionType::Absolute,
                        top: percent(102.0),
                        ..default()
                    },
                    TouchVisual::Caption,
                    Pickable::IGNORE,
                ));
            });
    }
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            border: UiRect::all(px(1.5)),
            border_radius: BorderRadius::all(px(12.0)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(0.02, 0.06, 0.07, 0.65)),
        BorderColor::all(Color::srgba(0.7, 0.9, 0.85, 0.65)),
        GlobalZIndex(2600),
        TouchVisual::HideToggle,
        Pickable::IGNORE,
    )).with_children(|p| {
        p.spawn((
            Text::new("Hide"),
            TextFont { font_size: FontSize::Px(11.0), ..default() },
            TextColor(Color::WHITE),
            TouchVisual::HideToggleLabel,
            Pickable::IGNORE,
        ));
    });
    for visual in [TouchVisual::Stick, TouchVisual::Knob] {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                border: UiRect::all(px(2.0)),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BorderColor::all(Color::srgba(0.7, 0.9, 0.85, 0.8)),
            BackgroundColor(Color::srgba(0.04, 0.1, 0.1, 0.35)),
            GlobalZIndex(2000),
            visual,
            Pickable::IGNORE,
        ));
    }
    commands.spawn((Text::new(format!("Left thumb: drag the stick to move. Right thumb: swipe empty space to look.\n{SETTINGS_BREADCRUMB}: Customize layout or Controls help / gesture practice.")),
        TextFont {font_size:FontSize::Px(14.0),..default()},TextColor(Color::WHITE),
        Node {position_type:PositionType::Absolute,left:percent(25.0),top:percent(18.0),width:percent(50.0),padding:UiRect::all(px(8.0)),..default()},
        BackgroundColor(Color::srgba(0.01,0.02,0.03,0.8)),GlobalZIndex(2100),TouchVisual::Instructions));
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            ..default()
        },
        BackgroundColor(Color::srgba(0.01, 0.02, 0.03, 0.94)),
        GlobalZIndex(2900),
        TouchVisual::EditorPanel,
    ));
    for (i, label) in layout::EDIT_ACTIONS.iter().enumerate() {
        commands.spawn((
            Text::new(*label),
            TextFont {
                font_size: FontSize::Px(14.0),
                ..default()
            },
            TextColor(Color::WHITE),
            Node {
                position_type: PositionType::Absolute,
                padding: UiRect::all(px(8.0)),
                border: UiRect::all(px(1.0)),
                ..default()
            },
            BorderColor::all(Color::srgba(0.7, 0.9, 0.85, 0.6)),
            BackgroundColor(Color::srgba(0.1, 0.18, 0.2, 1.0)),
            GlobalZIndex(3000),
            TouchVisual::EditorAction(i),
        ));
    }
    for (label, visual) in [("Hide tools", TouchVisual::EditorToggle), ("Dock bottom", TouchVisual::EditorDock)] {
        commands.spawn((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(13.0),
                ..default()
            },
            TextColor(Color::WHITE),
            Node {
                position_type: PositionType::Absolute,
                padding: UiRect::all(px(6.0)),
                border: UiRect::all(px(1.5)),
                ..default()
            },
            BorderColor::all(Color::srgba(0.95, 0.78, 0.35, 0.85)),
            BackgroundColor(Color::srgba(0.14, 0.22, 0.24, 0.96)),
            GlobalZIndex(3050),
            visual,
        ));
    }
    for ads in [false, true] {
        commands.spawn((
            Text::new(""),
            TextFont {
                font_size: FontSize::Px(14.0),
                ..default()
            },
            TextColor(Color::WHITE),
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            GlobalZIndex(3001),
            TouchVisual::AimingValue(ads),
        ));
    }
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(14.0),
            ..default()
        },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            left: percent(31.0),
            top: percent(2.0),
            width: percent(32.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.01, 0.02, 0.03, 0.94)),
        GlobalZIndex(3100),
        TouchVisual::EditorMessage,
    ));
}
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn show_labels(
    (state, mut layout, mut hint_fade, time, practice, modal, gyro_options, gyro_state): (Res<TouchState>, ResMut<Layout>, ResMut<HintFade>, Res<Time<Real>>, Res<practice::Practice>, Res<modal::Capture>, Res<gyro::Options>, Res<gyro::State>),
    (control, creator, paused, safe): (Option<Res<nvf_play::control::PlayerControl>>, Option<Res<nvf_play::creator::CreatorMenu>>, Option<Res<nvf_play::pause::Paused>>, Option<Res<nvf_play::touch_safe_area::SafeInsets>>),
    window: Query<&Window, With<PrimaryWindow>>,
    bindings: Option<Res<Bindings>>,
    pads: Query<&Gamepad>,
    (dialogue_camera, mut commands): (Query<(Entity, &Camera), With<nvf_play::dialogue_split::OverlayCamera>>, Commands),
    mut visuals: Query<(
        Entity,
        Option<&UiTargetCamera>,
        &TouchVisual,
        &mut Node,
        &mut Visibility,
        Option<&mut BackgroundColor>,
        Option<&mut Text>,
        Option<&mut ImageNode>,
        Option<&mut BorderColor>,
        Option<&mut TextColor>,
    )>,
) {
    let Ok(window) = window.single() else { return };
    let size = Vec2::new(window.width().max(32.0), window.height().max(32.0));
    let safe_insets = safe.as_deref().copied().unwrap_or_default();
    let editing = layout.editor.is_some();
    let creator_open = creator.as_deref().is_some_and(|c| c.is_open()) && !paused.as_deref().is_some_and(|p| p.0);
    let controller = bindings
        .is_some_and(|b| b.device == nvf_play::bindings::InputDevice::Controller)
        && !pads.is_empty();
    let fade_active = state.playing && !editing && !practice.blocking() && !state.suspended
        && !state.controls_hidden && !controller && !creator_open;
    hint_fade.tick(time.delta_secs(), fade_active, &mut layout.config.hints);
    let hint_alpha = hint_fade.alpha(layout.config.hints, editing);
    let aiming_page = layout.editor.as_ref().is_some_and(|e| e.aiming);
    let styles_page = layout.editor.as_ref().is_some_and(|e| e.styles);
    let tray_page=layout.tray_editing();
    let editor_dragging = layout.editor.as_ref().is_some_and(layout::Editor::dragging);
    let editor_collapsed = layout.editor.as_ref().is_some_and(|e| e.collapsed && !e.aiming && !e.styles && !e.tray);
    let editor_bottom = layout.editor.as_ref().is_some_and(|e| e.docked_bottom);
    let panel_hidden = editor_dragging || editor_collapsed;
    let palette = layout.config.appearance.palette();
    let color = |c: [f32; 3], a: f32| Color::srgba(c[0], c[1], c[2], a);
    let playing = layout.editor.as_ref().map_or(state.playing, |e| e.playing);
    let movement = state
        .contacts
        .values()
        .find(|c| matches!(c.role, Role::Move));
    let stick_center = movement.map_or_else(
        || layout.config.center(Target::Stick, size),
        |c| c.origin * size,
    );
    let stick_diameter = layout.config.diameter(Target::Stick, size);
    let dialogue_camera = dialogue_camera.single().ok().filter(|(_, camera)| camera.is_active)
        .map(|(entity, _)| entity);
    for (entity, target, visual, mut node, mut visibility, mut background, text, image, mut border, mut text_color) in
        &mut visuals
    {
        if matches!(visual, TouchVisual::Button(i) if !BUTTONS[*i].playing) {
            match (target.map(|target| target.0), dialogue_camera) {
                (current, Some(camera)) if current != Some(camera) => {
                    commands.entity(entity).insert(UiTargetCamera(camera));
                }
                (Some(_), None) => { commands.entity(entity).remove::<UiTargetCamera>(); }
                _ => {}
            }
        }
        let mut rect = None;
        let visible = match *visual {
            TouchVisual::EditorShield=>editing,
            TouchVisual::Button(i) => {
                let target = Target::Button(i);
                let d = layout.config.diameter(target, size);
                let geometry=layout.config.appearance.geometry(d);
                rect = Some(Rect::from_center_size(
                    layout.config.center(target, size),
                    Vec2::new(geometry.width,geometry.height),
                ));
                node.border_radius = BorderRadius::all(px(geometry.radius));
                let pressed = state
                    .contacts
                    .values()
                    .any(|c| matches!(c.role, Role::Button(j) | Role::SettingsOverlay(j) | Role::DialogueOverlay(j) if j==i))
                    || (i == gyro::BUTTON && gyro_state.active(&gyro_options))
                    || (BUTTONS[i].control == Some(Control::Block) && state.aim_latched)
                    || (BUTTONS[i].control == Some(Control::CrouchSneak)
                        && control.as_deref().is_some_and(|c| c.sneak));
                let selected = layout.editor.as_ref().is_some_and(|e| e.selected == target);
                if let Some(bg) = background.as_deref_mut() {
                    bg.0 = if pressed || selected {
                        color(palette.pressed, layout.config.opacity)
                    } else {
                        color(
                            palette.background,
                            if layout.config.buttons[i].visible {
                                layout.config.opacity * palette.fill
                            } else {
                                0.2
                            },
                        )
                    };
                }
                let paused_now = !editing && paused.as_deref().is_some_and(|p| p.0);
                !aiming_page
                    && !state.controls_hidden
                    && button_active(i, playing, paused_now)
                    && (editing || (layout.config.buttons[i].visible && (i != gyro::BUTTON || gyro_options.enabled)))
            }
            TouchVisual::HideToggle => {
                rect = Some(hide_toggle_rect(size, safe_insets));
                let toggle_alpha = if state.controls_hidden {
                    layout.config.opacity.max(0.35) * 0.65
                } else {
                    layout.config.opacity.max(0.35)
                };
                if let Some(bg) = background.as_deref_mut() {
                    let pressed = state.contacts.values().any(|c| matches!(c.role, Role::HideToggle));
                    bg.0 = if pressed {
                        color(palette.pressed, toggle_alpha)
                    } else {
                        color(palette.background, toggle_alpha * palette.fill)
                    };
                }
                if let Some(b) = border.as_deref_mut() {
                    *b = BorderColor::all(color(palette.border, toggle_alpha));
                }
                !editing && !controller
            }
            TouchVisual::HideToggleLabel => {
                let want = if state.controls_hidden { "Show" } else { "Hide" };
                if let Some(mut text) = text {
                    if text.0 != want { text.0 = want.to_string(); }
                }
                if let Some(tc) = text_color.as_deref_mut() {
                    let toggle_alpha = if state.controls_hidden {
                        layout.config.opacity.max(0.35) * 0.75
                    } else {
                        layout.config.opacity.max(0.35)
                    };
                    tc.0 = Color::srgba(palette.foreground[0], palette.foreground[1], palette.foreground[2], toggle_alpha);
                }
                !editing && !controller
            }
            TouchVisual::Caption => !state.controls_hidden && (editing || (layout.config.hints && hint_alpha > 0.0)),
            TouchVisual::Glyph => !state.controls_hidden,
            TouchVisual::Stick => {
                rect = Some(Rect::from_center_size(
                    stick_center,
                    Vec2::splat(stick_diameter),
                ));
                !state.controls_hidden && playing && !aiming_page
            }
            TouchVisual::Knob => {
                let offset = movement.map_or(Vec2::ZERO, |c| {
                    ((c.position - c.origin) * size).clamp_length_max(stick_diameter * 0.32)
                });
                rect = Some(Rect::from_center_size(
                    stick_center + offset,
                    Vec2::splat(stick_diameter * 0.35),
                ));
                !state.controls_hidden && playing && !aiming_page
            }
            TouchVisual::Instructions => {
                if let Some(mut text) = text {
                    text.0=format!("{} thumb: {}. {} thumb: swipe empty space to look.\n{}{}{SETTINGS_BREADCRUMB}: Customize layout or Controls help / gesture practice.",
                        if layout.config.stick.center.x > 0.5 {"Right"} else {"Left"},
                        if state.movement_options.analog {"small pushes walk; larger pushes run with Always Run"} else {"drag beyond the dead zone for directional movement"},
                        if layout.config.stick.center.x > 0.5 {"Left"} else {"Right"},
                        if state.movement_options.fire_drag {"Hold Fire and drag that finger to look. "} else {"Hold an icon to act. "},
                        if state.movement_options.run_lock {"Release the movement thumb on the arrow for Auto Move; tap it to stop. "} else {""});

                }
                if let Some(bg) = background.as_deref_mut() {
                    bg.0 = Color::srgba(0.01, 0.02, 0.03, 0.8 * hint_alpha);
                }
                if let Some(tc) = text_color.as_deref_mut() {
                    tc.0 = Color::srgba(1.0, 1.0, 1.0, hint_alpha);
                }
                !state.controls_hidden && !editing && layout.config.hints && hint_alpha > 0.0
            }
            TouchVisual::EditorPanel => {
                rect = Some(if tray_page {tray_editor::panel(size,layout.tray_safe)} else if aiming_page {
                    aiming::editor_panel(size)
                } else {
                    layout::editor_panel_rect(size, editor_bottom)
                });
                editing && !panel_hidden
            }
            TouchVisual::AimingValue(ads) => {
                rect = Some(aiming::value_rect(ads, size));
                if let Some(mut text) = text {
                    text.0 = format!(
                        "{} {:.0}%",
                        if ads { "ADS" } else { "Look" },
                        if ads {
                            layout.config.aiming.ads
                        } else {
                            layout.config.aiming.look
                        } * 100.0
                    );
                }
                aiming_page && !editor_dragging
            }
            TouchVisual::EditorAction(i) => {
                if tray_page && ![0,1,9,10].contains(&i) {*visibility=Visibility::Hidden;continue;}
                if (aiming_page || styles_page) && i >= 12 {
                    *visibility = Visibility::Hidden;
                    continue;
                }
                rect = Some(if tray_page {tray_editor::action_rect_with([0,1,9,10].iter().position(|v|*v==i).unwrap_or(0),size,layout.tray_safe)} else if aiming_page {
                    aiming::editor_rect(i, size)
                } else {
                    layout::editor_rect_in(i, size, editor_bottom)
                });
                if let Some(mut text) = text {
                    text.0 = if tray_page {layout::EDIT_ACTIONS[i].into()} else if styles_page {
                        let slot = layout.editor.as_ref().map_or(0, |e| e.slot);
                        match i {
                            0 => format!("Theme\n{}", palette.label),
                            1 => format!("Shape\n{}", layout.config.appearance.shape_label()),
                            2 => format!(
                                "Icons\n{}",
                                layout
                                    .config
                                    .appearance
                                    .family
                                    .as_deref()
                                    .unwrap_or("Global")
                            ),
                            3 => format!(
                                "Preset\n{}",
                                profiles::PRESETS
                                    .iter()
                                    .find(|p| p.0 == layout.config.preset_name)
                                    .map_or("Custom", |p| p.1)
                            ),
                            4 => format!(
                                "Slot {}\n{}",
                                slot + 1,
                                layout.config.slot_name(slot).unwrap_or("Empty").chars().take(12).collect::<String>()
                            ),
                            _ => profiles::ACTIONS.get(i).unwrap_or(&"").to_string(),
                        }
                    } else if layout.editor.as_ref().is_some_and(|e| e.aiming) {
                        match i {
                            6 => {
                                format!("Dead zone {:.0}%", layout.config.aiming.dead_zone * 100.0)
                            }
                            7 => if layout.config.aiming.toggle_aim {
                                "Aim: toggle"
                            } else {
                                "Aim: hold"
                            }
                            .into(),
                            8 => if layout.config.aiming.hold_sneak {
                                "Sneak: hold"
                            } else {
                                "Sneak: toggle"
                            }
                            .into(),
                            _ => aiming::EDIT_ACTIONS[i].into(),
                        }
                    } else {
                        match i {
                            2 => format!("Opacity {:.0}%", layout.config.opacity * 100.0),
                            3 => if layout.config.southpaw {
                                "Left handed"
                            } else {
                                "Right handed"
                            }
                            .into(),
                            4 => if layout.config.floating {
                                "Floating stick"
                            } else {
                                "Fixed stick"
                            }
                            .into(),
                            5 => if playing { "Edit menus" } else { "Edit game" }.into(),
                            _ => layout::EDIT_ACTIONS[i].into(),
                        }
                    };
                }
                editing && !panel_hidden && (!(aiming_page || styles_page) || i < 12)
            }
            TouchVisual::EditorToggle => {
                rect = Some(layout::editor_toggle_rect(size, editor_collapsed, editor_bottom));
                if let Some(mut text) = text {
                    let want = if editor_collapsed { "Show tools" } else { "Hide tools" };
                    if text.0 != want { text.0 = want.into(); }
                }
                editing && !editor_dragging && !aiming_page && !styles_page && !tray_page
            }
            TouchVisual::EditorDock => {
                rect = Some(layout::editor_dock_rect(size, editor_bottom));
                if let Some(mut text) = text {
                    let want = if editor_bottom { "Dock top" } else { "Dock bottom" };
                    if text.0 != want { text.0 = want.into(); }
                }
                editing && !panel_hidden && !aiming_page && !styles_page && !tray_page
            }
            TouchVisual::EditorMessage => {
                if !aiming_page && !tray_page {
                    let panel = layout::editor_panel_rect(size, editor_bottom);
                    let msg_w = panel.width().min(size.x - 16.0);
                    let msg_h = 34.0_f32;
                    let msg_y = if editor_bottom {
                        (panel.min.y - msg_h - 4.0).max(4.0)
                    } else {
                        (panel.max.y + 4.0).min((size.y - msg_h - 4.0).max(4.0))
                    };
                    rect = Some(Rect::from_corners(
                        Vec2::new((size.x - msg_w) * 0.5, msg_y),
                        Vec2::new((size.x + msg_w) * 0.5, msg_y + msg_h),
                    ));
                }
                if let Some(mut text) = text {
                    text.0 = layout.editor.as_ref().map_or_else(
                        || layout.message.clone(),
                        |e| {
                            if e.tray {return format!("Hot tray · {} slots · {:.0}%\n{}",layout.tray_preview.as_ref().map_or(0,|p|p.slots.len()),layout.config.tray.scale*100.0,layout.message);}
                            if e.styles {
                                return format!(
                                    "Appearance and profiles · slot {}\n{}",
                                    e.slot + 1,
                                    layout.message
                                );
                            }
                            if e.aiming {
                                return format!(
                                    "Touch aiming · look {:.0}% · ADS {:.0}%\n{}",
                                    layout.config.aiming.look * 100.0,
                                    layout.config.aiming.ads * 100.0,
                                    layout.message
                                );
                            }
                            format!(
                                "{} · {}\n{}",
                                e.selected.name(),
                                if playing {
                                    "Game controls"
                                } else {
                                    "Menu controls"
                                },
                                layout.message
                            )
                        },
                    );
                }
                editing && !panel_hidden
            }
        };
        // Opacity belongs to the entire control, not just its dark backing.
        if matches!(
            visual,
            TouchVisual::Button(_)
                | TouchVisual::Caption
                | TouchVisual::Glyph
                | TouchVisual::Stick
                | TouchVisual::Knob
        ) {
            let alpha = layout.config.opacity
                * if matches!(visual, TouchVisual::Caption) { hint_alpha } else { 1.0 };
            if let Some(mut image) = image {
                image.color = color(palette.foreground, alpha);
            }
            if let Some(mut border) = border {
                *border = BorderColor::all(color(palette.border, alpha));
            }
            if let Some(mut color) = text_color {
                color.0 = Color::srgba(
                    palette.foreground[0],
                    palette.foreground[1],
                    palette.foreground[2],
                    alpha,
                );
            }
            if matches!(visual, TouchVisual::Stick | TouchVisual::Knob) {
                if let Some(bg) = background.as_deref_mut() {
                    bg.0 = color(
                        palette.background,
                        alpha
                            * if matches!(visual, TouchVisual::Knob) {
                                0.8
                            } else {
                                0.35
                            },
                    );
                }
            }
        }
        *visibility =
            if visible && !creator_open && !state.ui_capture && !practice.blocking() && !modal.hides_overlay() && (!controller || editing) {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        if let Some(rect) = rect {
            node.left = px(rect.min.x);
            node.top = px(rect.min.y);
            node.width = px(rect.width());
            node.height = px(rect.height());
        }
    }
}

#[cfg(test)]
mod tests {
    include!("touch/pipboy_model_parent_ownership_tests.rs");
    include!("touch/vats_parent_tests.rs");
    include!("touch/tray_editor_tests.rs");
    include!("touch/map_parent_tests.rs");
    use super::*;
    use bevy::input::{keyboard::Key, mouse::AccumulatedMouseMotion, InputPlugin};
    use bevy::state::app::StatesPlugin;

    pub(super) fn scene() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, InputPlugin, StatesPlugin));
        app.init_state::<nvf_play::PlayState>();
        app.insert_resource(Bindings::default());
        app.insert_resource(movement::Options::legacy());
        app.add_message::<AppLifecycle>()
            .add_message::<WindowFocused>();
        app.add_plugins(TouchControlsPlugin);
        let window = app
            .world_mut()
            .spawn((
                Window {
                    resolution: bevy::window::WindowResolution::new(1000, 1000),
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        app.update();
        (app, window)
    }
    pub(super) fn touch(app: &mut App, window: Entity, id: u64, phase: TouchPhase, x: f32, y: f32) {
        app.world_mut().write_message(TouchInput {
            phase,
            position: Vec2::new(x, y),
            window,
            force: None,
            id,
        });
    }
    fn controls_scene() -> (App, Entity) {
        let (mut app, window) = scene();
        app.init_resource::<nvf_play::control::PlayerControl>()
            .add_systems(Update, nvf_play::control::read_controls);
        app.update();
        (app, window)
    }
    // Stage the actual Settings context without running the touch/action stages:
    // old touch ownership must remain until the registered editor entry's frame.
    fn open_registered_editor_from_settings(app: &mut App, window: Entity) {
        use nvf_play::{pause::{FormOver, Page, Paused}, settings};
        app.insert_resource(Paused(true)).insert_resource(Page::Settings { selected: None });
        app.world_mut().resource_mut::<NextState<nvf_play::PlayState>>()
            .set(nvf_play::PlayState::Menu);
        app.world_mut().run_schedule(bevy::state::state::StateTransition);
        assert_eq!(*app.world().resource::<State<nvf_play::PlayState>>().get(), nvf_play::PlayState::Menu);
        let row = app.world().resource::<settings::Settings>().index(entrypoints::KEY).unwrap();
        assert!(settings::pick(app.world_mut(), row, "Customize layout"));
        assert!(app.world().resource::<Layout>().editor.is_some());
        assert!(app.world().resource::<entrypoints::Session>().blocking());
        assert_eq!(app.world().resource::<entrypoints::Session>().window(), Some(window));
        assert!(app.world().resource::<FormOver>().0);
    }
    #[test]
    fn menu_hardware_intent_uses_raw_window_activity_and_not_virtual_touch_look() {
        let (mut app, window) = scene();
        app.world_mut().resource_mut::<NextState<nvf_play::PlayState>>().set(nvf_play::PlayState::Playing);
        app.update();
        touch(&mut app, window, 90, TouchPhase::Started, 600.0, 410.0);
        app.update();
        touch(&mut app, window, 90, TouchPhase::Moved, 630.0, 420.0);
        app.update();
        assert_ne!(app.world().resource::<AccumulatedMouseMotion>().delta, Vec2::ZERO);
        assert!(!app.world().resource::<TouchState>().hardware_intent);
        app.update();
        assert!(!app.world().resource::<TouchState>().hardware_intent, "previous touch MouseMotion is not hardware next frame");
        let foreign = app.world_mut().spawn_empty().id();
        app.world_mut().write_message(CursorMoved {window:foreign,position:Vec2::ONE,delta:Some(Vec2::ONE)});
        app.update();
        assert!(!app.world().resource::<TouchState>().hardware_intent);
        app.world_mut().write_message(CursorMoved {window,position:Vec2::splat(20.0),delta:Some(Vec2::new(20.0,0.0))});
        app.update();
        assert!(app.world().resource::<TouchState>().hardware_intent);
        app.update();
        assert!(!app.world().resource::<TouchState>().hardware_intent);
        raw_key(&mut app, window, KeyCode::ArrowDown, ButtonState::Pressed);
        app.update();
        assert!(app.world().resource::<TouchState>().hardware_intent);
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::ArrowDown), "the observer does not consume hardware");
    }
    fn button_touch(app: &mut App, window: Entity, id: u64, phase: TouchPhase, control: Control) {
        let index = BUTTONS
            .iter()
            .position(|b| b.control == Some(control))
            .unwrap();
        let center = app
            .world()
            .resource::<Layout>()
            .config
            .center(Target::Button(index), Vec2::splat(1000.0));
        touch(app, window, id, phase, center.x, center.y);
    }
    fn tap_control(app: &mut App, window: Entity, id: u64, control: Control) {
        button_touch(app, window, id, TouchPhase::Started, control);
        button_touch(app, window, id, TouchPhase::Ended, control);
        app.update();
    }
    fn raw_key(app: &mut App, window: Entity, key: KeyCode, state: ButtonState) {
        app.world_mut().write_message(KeyboardInput {
            key_code: key,
            logical_key: Key::Character("owned input fixture".into()),
            state,
            text: None,
            repeat: false,
            window,
        });
    }
    fn aim_requested(app: &App) -> bool {
        app.world()
            .resource::<nvf_play::control::PlayerControl>()
            .aiming
    }
    fn sneak_requested(app: &App) -> bool {
        app.world()
            .resource::<nvf_play::control::PlayerControl>()
            .sneak
    }
    #[test]
    fn controller_takeover_cancels_touch_latches_and_first_touch_returns_without_actions() {
        use bevy::input::gamepad::{RawGamepadButtonChangedEvent, RawGamepadEvent};
        use nvf_play::bindings::InputDevice;
        let pad_edge = |app: &mut App, pad, down| {
            app.world_mut().write_message(RawGamepadEvent::Button(
                RawGamepadButtonChangedEvent::new(
                    pad,
                    GamepadButton::South,
                    if down { 1.0 } else { 0.0 },
                ),
            ));
        };
        for native_sneak in [false, true] {
            let (mut app, w) = controls_scene();
            app.add_plugins(nvf_play::pad::InputSwitchPlugin);
            let pad = app.world_mut().spawn(Gamepad::default()).id();
            {
                let mut layout = app.world_mut().resource_mut::<Layout>();
                layout.config.aiming.toggle_aim = true;
                layout.config.aiming.hold_sneak = true;
            }
            if native_sneak {
                let key = app
                    .world()
                    .resource::<Bindings>()
                    .get(Control::CrouchSneak)
                    .key
                    .unwrap();
                raw_key(&mut app, w, key, ButtonState::Pressed);
                app.update();
                raw_key(&mut app, w, key, ButtonState::Released);
                app.update();
            }
            app.update();
            assert_eq!(sneak_requested(&app), native_sneak);
            tap_control(&mut app, w, 1, Control::Block);
            app.update(); // The finger is gone; its aim latch must not block takeover.
            assert!(aim_requested(&app));
            pad_edge(&mut app, pad, true);
            app.update();
            assert_eq!(
                app.world().resource::<Bindings>().device,
                InputDevice::Controller,
                "accepted controller activity takes over a released touch aim latch"
            );
            assert!(!aim_requested(&app));
            pad_edge(&mut app, pad, false);
            app.update();
            app.update();
            assert!(!app.world().resource::<TouchState>().aim_latched);
            tap_control(&mut app, w, 2, Control::Block);
            assert_eq!(
                app.world().resource::<Bindings>().device,
                InputDevice::KeyboardMouse
            );
            assert!(!app
                .world()
                .resource::<nvf_play::pad::TouchDeviceSwitch>()
                .allows_actions());
            assert!(!aim_requested(&app) && !app.world().resource::<TouchState>().aim_latched);
            assert_eq!(sneak_requested(&app), native_sneak);
            tap_control(&mut app, w, 3, Control::Block);
            assert!(aim_requested(&app), "a subsequent aim tap works");
            tap_control(&mut app, w, 4, Control::Block);
            button_touch(&mut app, w, 5, TouchPhase::Started, Control::CrouchSneak);
            app.update();
            assert!(
                app.world()
                    .resource::<nvf_play::control::HeldSneakInput>()
                    .active
            );
            assert!(sneak_requested(&app));
            // A live touch keeps the automatic selector busy; an explicit choice
            // must still clear that touch source while preserving the native base.
            nvf_play::bindings::set_device(app.world_mut(), InputDevice::Controller);
            app.update();
            assert!(
                !app.world()
                    .resource::<nvf_play::control::HeldSneakInput>()
                    .active
            );
            assert!(app.world().resource::<TouchState>().contacts.is_empty());
            assert_eq!(sneak_requested(&app), native_sneak);
            button_touch(&mut app, w, 5, TouchPhase::Ended, Control::CrouchSneak);
            app.update();
            tap_control(&mut app, w, 6, Control::CrouchSneak);
            assert_eq!(
                app.world().resource::<Bindings>().device,
                InputDevice::KeyboardMouse
            );
            assert!(
                !app.world()
                    .resource::<nvf_play::control::HeldSneakInput>()
                    .active
            );
            assert_eq!(
                sneak_requested(&app),
                native_sneak,
                "first return tap cannot buffer sneak"
            );
            button_touch(&mut app, w, 7, TouchPhase::Started, Control::CrouchSneak);
            app.update();
            assert!(sneak_requested(&app), "a fresh hold works");
            button_touch(&mut app, w, 7, TouchPhase::Ended, Control::CrouchSneak);
            app.update();
            assert_eq!(sneak_requested(&app), native_sneak);
            nvf_play::bindings::set_device(app.world_mut(), InputDevice::Controller);
            app.update();
            let gear = layout::utility_rect(false, Vec2::splat(1000.0)).center();
            touch(&mut app, w, 8, TouchPhase::Started, gear.x, gear.y);
            touch(&mut app, w, 8, TouchPhase::Ended, gear.x, gear.y);
            app.update();
            assert!(
                app.world().resource::<Layout>().editor.is_none(),
                "return tap cannot open an unpaused editor"
            );
            assert!(!app
                .world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::Escape));
            touch(&mut app, w, 9, TouchPhase::Started, gear.x, gear.y);
            touch(&mut app, w, 9, TouchPhase::Ended, gear.x, gear.y);
            app.update();
            assert!(app.world().resource::<Layout>().editor.is_none(),
                "the removed shortcut cannot return after controller takeover");
        }
    }
    fn drawer_visibility(app: &mut App) -> Visibility {
        let world = app.world_mut();
        let mut query = world.query_filtered::<&Visibility, With<drawer::Handle>>();
        *query.single(world).unwrap()
    }
    #[test]
    fn explicit_controller_takeover_cancels_drawer_and_first_swipe_only_restores_overlay() {
        use bevy::input::gamepad::{RawGamepadButtonChangedEvent, RawGamepadEvent};
        use nvf_play::bindings::InputDevice;
        let (mut app, w) = controls_scene();
        app.add_plugins(nvf_play::pad::InputSwitchPlugin);
        let pad = app.world_mut().spawn(Gamepad::default()).id();
        app.world_mut().resource_mut::<drawer::Options>().enabled = true;
        app.update();
        let start = drawer::handle(Vec2::splat(1000.0), 24.0).center();
        touch(&mut app, w, 10, TouchPhase::Started, start.x, start.y);
        app.update();
        app.world_mut()
            .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                pad,
                GamepadButton::South,
                1.0,
            )));
        app.update();
        assert_eq!(
            app.world().resource::<Bindings>().device,
            InputDevice::KeyboardMouse,
            "an owned contact retains the existing old-device-busy policy"
        );
        app.world_mut()
            .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                pad,
                GamepadButton::South,
                0.0,
            )));
        app.update();
        nvf_play::bindings::set_device(app.world_mut(), InputDevice::Controller);
        app.update();
        assert_eq!(drawer_visibility(&mut app), Visibility::Hidden);
        touch(&mut app, w, 10, TouchPhase::Ended, start.x, start.y - 140.0);
        app.update();
        assert_eq!(
            app.world()
                .resource::<Messages<nvf_play::menu::OpenInventory>>()
                .len(),
            0,
            "old release cannot open inventory after takeover"
        );
        for (phase, y) in [
            (TouchPhase::Started, start.y),
            (TouchPhase::Moved, start.y - 140.0),
            (TouchPhase::Ended, start.y - 140.0),
        ] {
            touch(&mut app, w, 11, phase, start.x, y);
        }
        app.update();
        assert_eq!(
            app.world().resource::<Bindings>().device,
            InputDevice::KeyboardMouse
        );
        assert!(!app
            .world()
            .resource::<nvf_play::pad::TouchDeviceSwitch>()
            .allows_actions());
        assert!(!app.world().resource::<TouchState>().interactive_actions,
            "the switching touch is consumed even while the overlay is restored");
        assert_eq!(drawer_visibility(&mut app), Visibility::Inherited);
        assert_eq!(
            app.world()
                .resource::<Messages<nvf_play::menu::OpenInventory>>()
                .len(),
            0,
            "first return swipe is consumed in full"
        );
        for (phase, y) in [
            (TouchPhase::Started, start.y),
            (TouchPhase::Moved, start.y - 140.0),
            (TouchPhase::Ended, start.y - 140.0),
        ] {
            touch(&mut app, w, 12, phase, start.x, y);
            app.update();
        }
        assert_eq!(
            app.world()
                .resource::<Messages<nvf_play::menu::OpenInventory>>()
                .len(),
            1,
            "a fresh owned swipe reaches the real shared request route once"
        );
    }
    #[test]
    fn drawer_commit_releases_touch_aim_and_hold_sneak_without_erasing_native_base() {
        for native_sneak in [false, true] {
            let (mut app, w) = controls_scene();
            app.world_mut().resource_mut::<drawer::Options>().enabled = true;
            {
                let mut layout = app.world_mut().resource_mut::<Layout>();
                layout.config.aiming.toggle_aim = true;
                layout.config.aiming.hold_sneak = true;
            }
            if native_sneak {
                let key = app
                    .world()
                    .resource::<Bindings>()
                    .get(Control::CrouchSneak)
                    .key
                    .unwrap();
                raw_key(&mut app, w, key, ButtonState::Pressed);
                app.update();
                raw_key(&mut app, w, key, ButtonState::Released);
                app.update();
            }
            tap_control(&mut app, w, 1, Control::Block);
            assert!(aim_requested(&app));
            button_touch(&mut app, w, 2, TouchPhase::Started, Control::CrouchSneak);
            app.update();
            assert!(
                app.world()
                    .resource::<nvf_play::control::HeldSneakInput>()
                    .active
                    && sneak_requested(&app)
            );
            let start = drawer::handle(Vec2::splat(1000.0), 24.0).center();
            for (phase, y) in [
                (TouchPhase::Started, start.y),
                (TouchPhase::Moved, start.y - 140.0),
                (TouchPhase::Ended, start.y - 140.0),
            ] {
                touch(&mut app, w, 10, phase, start.x, y);
                app.update();
            }
            assert_eq!(
                app.world()
                    .resource::<Messages<nvf_play::menu::OpenInventory>>()
                    .len(),
                1
            );
            assert!(!app.world().resource::<TouchState>().aim_latched && !aim_requested(&app));
            assert!(
                !app.world()
                    .resource::<nvf_play::control::HeldSneakInput>()
                    .active
            );
            assert_eq!(sneak_requested(&app), native_sneak);
        }
    }
    #[test]
    fn drawer_commit_drops_later_gameplay_starts_in_the_same_batch_and_keeps_physical_owners() {
        for action in [Control::Use, Control::Block, Control::CrouchSneak] {
            let (mut app, w) = controls_scene();
            app.world_mut().resource_mut::<drawer::Options>().enabled = true;
            {
                let mut layout = app.world_mut().resource_mut::<Layout>();
                layout.config.aiming.toggle_aim = true;
                layout.config.aiming.hold_sneak = true;
            }
            raw_key(&mut app, w, KeyCode::KeyW, ButtonState::Pressed);
            app.world_mut().write_message(MouseButtonInput {
                button: MouseButton::Middle,
                state: ButtonState::Pressed,
                window: w,
            });
            app.update();
            let start = drawer::handle(Vec2::splat(1000.0), 24.0).center();
            touch(&mut app, w, 10, TouchPhase::Started, start.x, start.y);
            app.update();
            touch(&mut app, w, 10, TouchPhase::Moved, start.x, start.y - 140.0);
            touch(&mut app, w, 10, TouchPhase::Ended, start.x, start.y - 140.0);
            button_touch(&mut app, w, 20, TouchPhase::Started, action);
            app.update();
            assert_eq!(
                app.world()
                    .resource::<Messages<nvf_play::menu::OpenInventory>>()
                    .len(),
                1
            );
            for _ in 0..2 {
                let state = app.world().resource::<TouchState>();
                assert!(
                    state.contacts.is_empty() && !state.aim_latched,
                    "post-commit {action:?} cannot own a gameplay contact"
                );
                assert!(
                    state.physical_keys.contains(&KeyCode::KeyW)
                        && state.physical_mouse.contains(&MouseButton::Middle)
                );
                assert!(!app
                    .world()
                    .resource::<ButtonInput<MouseButton>>()
                    .pressed(MouseButton::Left));
                assert!(
                    !aim_requested(&app)
                        && !app
                            .world()
                            .resource::<nvf_play::control::HeldSneakInput>()
                            .active
                );
                assert!(app
                    .world()
                    .resource::<ButtonInput<KeyCode>>()
                    .pressed(KeyCode::KeyW));
                assert!(app
                    .world()
                    .resource::<ButtonInput<MouseButton>>()
                    .pressed(MouseButton::Middle));
                app.update(); // An unread tail must not replay next frame either.
            }
        }
    }
    #[test]
    fn touch_look_uses_actual_ads_state_and_does_not_rescale_hardware_motion() {
        let (mut app, w) = controls_scene();
        app.init_resource::<nvf_play::aim_zoom::AimZoom>();
        {
            let mut l = app.world_mut().resource_mut::<Layout>();
            l.config.aiming.look = 2.0;
            l.config.aiming.ads = 0.25;
        }
        touch(&mut app, w, 1, TouchPhase::Started, 600.0, 400.0);
        button_touch(&mut app, w, 2, TouchPhase::Started, Control::Block);
        app.update();
        assert!(aim_requested(&app));
        assert!(
            !app.world().resource::<nvf_play::aim_zoom::AimZoom>().aimed,
            "a held aim binding alone does not establish ADS"
        );
        touch(&mut app, w, 1, TouchPhase::Moved, 620.0, 410.0);
        app.world_mut().write_message(MouseMotion {
            delta: Vec2::new(3.0, -2.0),
        });
        app.update();
        assert!(
            (app.world().resource::<AccumulatedMouseMotion>().delta - Vec2::new(43.0, 18.0))
                .length()
                < 0.001
        );
        app.world_mut()
            .resource_mut::<nvf_play::aim_zoom::AimZoom>()
            .aimed = true;
        touch(&mut app, w, 1, TouchPhase::Moved, 640.0, 420.0);
        app.world_mut().write_message(MouseMotion {
            delta: Vec2::new(3.0, -2.0),
        });
        app.update();
        assert!(
            (app.world().resource::<AccumulatedMouseMotion>().delta - Vec2::new(8.0, 0.5)).length()
                < 0.001
        );
    }
    #[test]
    fn toggle_aim_taps_release_only_the_touch_key_owner() {
        let (mut app, w) = controls_scene();
        app.world_mut()
            .resource_mut::<Layout>()
            .config
            .aiming
            .toggle_aim = true;
        tap_control(&mut app, w, 1, Control::Block);
        assert!(aim_requested(&app));
        let key = app
            .world()
            .resource::<Bindings>()
            .get(Control::Block)
            .key
            .unwrap();
        raw_key(&mut app, w, key, ButtonState::Pressed);
        app.update();
        tap_control(&mut app, w, 2, Control::Block);
        assert!(!app.world().resource::<TouchState>().aim_latched);
        assert!(aim_requested(&app), "the physical owner still holds Block");
        raw_key(&mut app, w, key, ButtonState::Released);
        app.update();
        assert!(!aim_requested(&app));
    }
    #[test]
    fn latched_aim_clears_on_cancel_menu_focus_suspend_and_editor() {
        for case in 0..6 {
            let (mut app, w) = controls_scene();
            app.world_mut()
                .resource_mut::<Layout>()
                .config
                .aiming
                .toggle_aim = true;
            tap_control(&mut app, w, 1, Control::Block);
            assert!(aim_requested(&app));
            match case {
                0 => touch(&mut app, w, 2, TouchPhase::Canceled, 600.0, 400.0),
                1 => app
                    .world_mut()
                    .resource_mut::<NextState<nvf_play::PlayState>>()
                    .set(nvf_play::PlayState::Menu),
                2 => {
                    app.world_mut().write_message(WindowFocused {
                        window: w,
                        focused: false,
                    });
                    button_touch(&mut app, w, 3, TouchPhase::Started, Control::Block);
                }
                3 => {
                    app.world_mut().write_message(AppLifecycle::Suspended);
                }
                4 => {
                    open_registered_editor_from_settings(&mut app, w);
                    assert!(aim_requested(&app), "Settings setup must leave the actual held aim for the ordinary reader to cancel");
                    assert!(app.world().resource::<TouchState>().aim_latched);
                }
                _ => {
                    app.world_mut().write_message(KeyboardFocusLost);
                }
            }
            app.update();
            assert!(!aim_requested(&app), "cleanup case {case}");
            assert!(!app.world().resource::<TouchState>().aim_latched);
        }
    }
    #[test]
    fn hold_sneak_preserves_keyboard_toggle_base_and_restores_on_lift() {
        let (mut app, w) = controls_scene();
        app.world_mut()
            .resource_mut::<Layout>()
            .config
            .aiming
            .hold_sneak = true;
        let key = app
            .world()
            .resource::<Bindings>()
            .get(Control::CrouchSneak)
            .key
            .unwrap();
        button_touch(&mut app, w, 1, TouchPhase::Started, Control::CrouchSneak);
        app.update();
        assert!(sneak_requested(&app));
        assert!(
            !app.world().resource::<ButtonInput<KeyCode>>().pressed(key),
            "touch hold is not a fabricated toggle press"
        );
        raw_key(&mut app, w, key, ButtonState::Pressed);
        app.update();
        raw_key(&mut app, w, key, ButtonState::Released);
        app.update();
        button_touch(&mut app, w, 1, TouchPhase::Ended, Control::CrouchSneak);
        app.update();
        assert!(
            sneak_requested(&app),
            "the independent keyboard toggle survives touch release"
        );
        raw_key(&mut app, w, key, ButtonState::Pressed);
        app.update();
        raw_key(&mut app, w, key, ButtonState::Released);
        app.update();
        assert!(!sneak_requested(&app));
        button_touch(&mut app, w, 2, TouchPhase::Started, Control::CrouchSneak);
        app.update();
        assert!(sneak_requested(&app));
        button_touch(&mut app, w, 2, TouchPhase::Ended, Control::CrouchSneak);
        app.update();
        assert!(!sneak_requested(&app));
    }
    #[test]
    fn hold_sneak_fast_tap_cancellation_and_input_gates_do_not_toggle_the_base() {
        let (mut app, w) = controls_scene();
        app.world_mut()
            .resource_mut::<Layout>()
            .config
            .aiming
            .hold_sneak = true;
        tap_control(&mut app, w, 1, Control::CrouchSneak);
        assert!(!sneak_requested(&app));
        button_touch(&mut app, w, 2, TouchPhase::Started, Control::CrouchSneak);
        app.update();
        assert!(sneak_requested(&app));
        app.world_mut().write_message(AppLifecycle::Suspended);
        app.update();
        assert!(!sneak_requested(&app));
        app.world_mut().write_message(AppLifecycle::Running);
        app.update();
        app.insert_resource(nvf_play::bindings::KeyboardCapture(true));
        button_touch(&mut app, w, 3, TouchPhase::Started, Control::CrouchSneak);
        app.update();
        assert!(!sneak_requested(&app));
        app.world_mut()
            .remove_resource::<nvf_play::bindings::KeyboardCapture>();
        app.update();
        assert!(sneak_requested(&app));
        app.world_mut().resource_mut::<Bindings>().device =
            nvf_play::bindings::InputDevice::Controller;
        app.update();
        assert!(!sneak_requested(&app));
        assert!(
            !app.world()
                .resource::<nvf_play::control::HeldSneakInput>()
                .active
        );
    }
    #[test]
    fn touch_joystick_uses_the_configured_dead_zone_in_the_event_route() {
        let (mut app, w) = scene();
        app.world_mut()
            .resource_mut::<Layout>()
            .config
            .aiming
            .dead_zone = 0.35;
        touch(&mut app, w, 1, TouchPhase::Started, 200.0, 800.0);
        touch(&mut app, w, 1, TouchPhase::Moved, 200.0, 760.0);
        app.update();
        assert!(!app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW));
        touch(&mut app, w, 1, TouchPhase::Moved, 200.0, 740.0);
        app.update();
        assert!(app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW));
        app.world_mut()
            .resource_mut::<Layout>()
            .config
            .aiming
            .dead_zone = 0.5;
        app.update();
        assert!(app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .just_released(KeyCode::KeyW));
    }
    #[test]
    fn aiming_editor_settings_cancel_with_the_layout() {
        let (mut app, w) = scene();
        let original = app.world().resource::<Layout>().config.clone();
        app.world_mut().resource_mut::<Layout>().open(true);
        app.update();
        for action in [11, 1, 7, 8, 6] {
            let size = Vec2::splat(1000.0);
            let p = if app
                .world()
                .resource::<Layout>()
                .editor
                .as_ref()
                .unwrap()
                .aiming
            {
                aiming::editor_rect(action, size)
            } else {
                layout::editor_rect(action, size)
            }
            .center();
            touch(&mut app, w, 2, TouchPhase::Started, p.x, p.y);
            touch(&mut app, w, 2, TouchPhase::Ended, p.x, p.y);
            app.update();
        }
        let config = &app.world().resource::<Layout>().config;
        assert!(config.aiming.look > 1.0 && config.aiming.toggle_aim && config.aiming.hold_sneak);
        let p = aiming::editor_rect(10, Vec2::splat(1000.0)).center();
        touch(&mut app, w, 3, TouchPhase::Started, p.x, p.y);
        app.update();
        assert_eq!(app.world().resource::<Layout>().config, original);
    }
    #[test]
    fn toggle_aim_touch_from_controller_selection_does_not_buffer_a_latch() {
        let (mut app, w) = controls_scene();
        app.world_mut()
            .resource_mut::<Layout>()
            .config
            .aiming
            .toggle_aim = true;
        app.world_mut().resource_mut::<Bindings>().device =
            nvf_play::bindings::InputDevice::Controller;
        tap_control(&mut app, w, 1, Control::Block);
        assert!(!aim_requested(&app));
        app.world_mut().resource_mut::<Bindings>().device =
            nvf_play::bindings::InputDevice::KeyboardMouse;
        app.update();
        assert!(!aim_requested(&app));
        tap_control(&mut app, w, 2, Control::Block);
        assert!(aim_requested(&app));
    }
    #[test]
    fn drawer_release_requests_the_real_inventory_route_without_fire_or_look() {
        let (mut app, w) = scene();
        app.world_mut().resource_mut::<drawer::Options>().enabled = true;
        for (phase, y) in [
            (TouchPhase::Started, 940.0),
            (TouchPhase::Moved, 820.0),
            (TouchPhase::Ended, 820.0),
        ] {
            touch(&mut app, w, 1, phase, 530.0, y);
            app.update();
            assert!(!app
                .world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left));
            assert_eq!(
                app.world().resource::<AccumulatedMouseMotion>().delta,
                Vec2::ZERO
            );
        }
        assert_eq!(
            app.world_mut()
                .resource_mut::<Messages<nvf_play::menu::OpenInventory>>()
                .drain()
                .count(),
            1
        );
        touch(&mut app, w, 1, TouchPhase::Ended, 530.0, 820.0);
        app.update();
        assert_eq!(
            app.world()
                .resource::<Messages<nvf_play::menu::OpenInventory>>()
                .len(),
            0
        );
    }
    #[test]
    fn drawer_never_steals_an_edited_fire_button_hit_target() {
        let (mut app, w) = scene();
        app.world_mut().resource_mut::<drawer::Options>().enabled = true;
        let fire = BUTTONS
            .iter()
            .position(|b| b.control == Some(Control::Use))
            .unwrap();
        app.world_mut().resource_mut::<Layout>().config.buttons[fire].center =
            Vec2::new(0.53, 0.94);
        touch(&mut app, w, 1, TouchPhase::Started, 530.0, 940.0);
        app.update();
        assert!(app
            .world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left));
        touch(&mut app, w, 1, TouchPhase::Ended, 530.0, 800.0);
        app.update();
        assert_eq!(
            app.world()
                .resource::<Messages<nvf_play::menu::OpenInventory>>()
                .len(),
            0
        );
    }
    #[test]
    fn drawer_cancels_on_pending_menu_suspend_focus_text_capture_and_editor() {
        for scenario in 0..5 {
            let (mut app, w) = scene();
            app.world_mut().resource_mut::<drawer::Options>().enabled = true;
            touch(&mut app, w, 1, TouchPhase::Started, 530.0, 940.0);
            app.update();
            match scenario {
                0 => app
                    .world_mut()
                    .resource_mut::<NextState<nvf_play::PlayState>>()
                    .set(nvf_play::PlayState::Menu),
                1 => {
                    app.world_mut().write_message(AppLifecycle::Suspended);
                }
                2 => {
                    app.world_mut().write_message(WindowFocused {
                        window: w,
                        focused: false,
                    });
                }
                3 => {
                    app.world_mut()
                        .insert_resource(nvf_play::bindings::KeyboardCapture(true));
                }
                _ => app.world_mut().resource_mut::<Layout>().open(true),
            }
            touch(&mut app, w, 1, TouchPhase::Ended, 530.0, 800.0);
            app.update();
            assert_eq!(
                app.world()
                    .resource::<Messages<nvf_play::menu::OpenInventory>>()
                    .len(),
                0,
                "scenario {scenario}"
            );
        }
    }
    #[test]
    fn drawer_remembers_focus_loss_until_an_explicit_gain() {
        let (mut app, w) = scene();
        app.world_mut().resource_mut::<drawer::Options>().enabled = true;
        app.world_mut().write_message(WindowFocused {
            window: w,
            focused: false,
        });
        app.update();
        for (phase, y) in [(TouchPhase::Started, 940.0), (TouchPhase::Ended, 800.0)] {
            touch(&mut app, w, 1, phase, 530.0, y);
            app.update();
        }
        assert_eq!(
            app.world()
                .resource::<Messages<nvf_play::menu::OpenInventory>>()
                .len(),
            0
        );
        app.world_mut().write_message(WindowFocused {
            window: w,
            focused: true,
        });
        app.update();
        for (phase, y) in [(TouchPhase::Started, 940.0), (TouchPhase::Ended, 800.0)] {
            touch(&mut app, w, 2, phase, 530.0, y);
            app.update();
        }
        assert_eq!(
            app.world()
                .resource::<Messages<nvf_play::menu::OpenInventory>>()
                .len(),
            1
        );
    }
    #[test]
    fn editing_releases_attack_and_cancel_restores_dragged_layout() {
        let (mut app, w) = scene();
        touch(&mut app, w, 1, TouchPhase::Started, 920.0, 900.0);
        app.update();
        assert!(app
            .world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left));
        open_registered_editor_from_settings(&mut app, w);
        assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left),
            "Settings setup must leave the actual touch attack held until the ordinary entry frame");
        app.update();
        assert!(app.world().resource::<Layout>().editor.is_some());
        assert!(!app
            .world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left));
        assert!(!app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::Escape),
            "the Settings entry already owns pause; editor entry must not toggle it");
        let original = app.world().resource::<Layout>().config.clone();
        touch(&mut app, w, 3, TouchPhase::Started, 180.0, 760.0);
        app.update();
        touch(&mut app, w, 3, TouchPhase::Moved, 240.0, 820.0);
        app.update();
        assert_ne!(
            app.world().resource::<Layout>().config.stick.center,
            original.stick.center
        );
        assert!(!app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyS));
        touch(&mut app, w, 3, TouchPhase::Ended, 240.0, 820.0);
        let cancel = layout::editor_rect(10, Vec2::splat(1000.0)).center();
        touch(&mut app, w, 4, TouchPhase::Started, cancel.x, cancel.y);
        app.update();
        let layout = app.world().resource::<Layout>();
        assert!(layout.editor.is_none());
        assert_eq!(layout.config, original);
    }
    #[test]
    fn a_second_finger_changing_editor_mode_cancels_the_first_drag() {
        let (mut app, w) = scene();
        app.world_mut().resource_mut::<Layout>().open(true);
        app.update();
        let size = Vec2::splat(1000.0);
        let fire = app
            .world()
            .resource::<Layout>()
            .config
            .center(Target::Button(7), size);
        touch(&mut app, w, 2, TouchPhase::Started, fire.x, fire.y);
        app.update();
        let before = app.world().resource::<Layout>().config.stick.clone();
        let mode = layout::editor_rect(5, size).center();
        touch(&mut app, w, 3, TouchPhase::Started, mode.x, mode.y);
        app.update();
        touch(&mut app, w, 2, TouchPhase::Moved, 600.0, 700.0);
        app.update();
        assert_eq!(app.world().resource::<Layout>().config.stick, before);
    }
    #[test]
    fn opacity_applies_to_icons_borders_and_labels_not_only_backing() {
        let (mut app, _) = scene();
        app.world_mut().resource_mut::<Layout>().config.opacity = 0.35;
        app.update();
        let world = app.world_mut();
        let mut q = world.query::<(
            &TouchVisual,
            Option<&ImageNode>,
            Option<&BorderColor>,
            Option<&TextColor>,
        )>();
        let mut icons = 0;
        for (visual, image, border, text) in q.iter(world) {
            if matches!(visual, TouchVisual::Glyph) {
                assert!((image.unwrap().color.to_srgba().alpha - 0.35).abs() < 0.001);
                icons += 1;
            }
            if matches!(visual, TouchVisual::Button(_)) {
                assert!((border.unwrap().top.to_srgba().alpha - 0.35).abs() < 0.001);
            }
            if matches!(visual, TouchVisual::Caption) {
                assert!((text.unwrap().0.to_srgba().alpha - 0.35).abs() < 0.001);
            }
        }
        assert_eq!(icons, BUTTONS.len());
    }
    #[test]
    fn a_repositioned_floating_stick_still_moves_at_its_visible_anchor() {
        let (mut app, w) = scene();
        app.world_mut().resource_mut::<Layout>().config.stick.center = Vec2::new(0.60, 0.76);
        touch(&mut app, w, 1, TouchPhase::Started, 600.0, 760.0);
        app.update();
        touch(&mut app, w, 1, TouchPhase::Moved, 600.0, 660.0);
        app.update();
        assert!(app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW));
        assert_eq!(
            app.world().resource::<AccumulatedMouseMotion>().delta,
            Vec2::ZERO
        );
    }
    #[test]
    fn southpaw_fixed_stick_uses_its_visible_center_and_releases_on_lift() {
        let (mut app, w) = scene();
        {
            let mut l = app.world_mut().resource_mut::<Layout>();
            l.config = layout::Config::preset(true);
            l.config.floating = false;
        }
        let center = app
            .world()
            .resource::<Layout>()
            .config
            .center(Target::Stick, Vec2::splat(1000.0));
        touch(&mut app, w, 1, TouchPhase::Started, center.x, center.y);
        app.update();
        assert!(!app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW));
        touch(
            &mut app,
            w,
            1,
            TouchPhase::Moved,
            center.x,
            center.y - 100.0,
        );
        app.update();
        assert!(app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW));
        touch(
            &mut app,
            w,
            1,
            TouchPhase::Ended,
            center.x,
            center.y - 100.0,
        );
        app.update();
        assert!(app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .just_released(KeyCode::KeyW));
    }
    #[test]
    fn touch_move_look_and_fire_work_together_then_cancel_releases() {
        let (mut app, w) = scene();
        touch(&mut app, w, 1, TouchPhase::Started, 200.0, 800.0);
        touch(&mut app, w, 2, TouchPhase::Started, 600.0, 400.0);
        touch(&mut app, w, 3, TouchPhase::Started, 920.0, 900.0);
        app.update();
        touch(&mut app, w, 1, TouchPhase::Moved, 200.0, 700.0);
        touch(&mut app, w, 2, TouchPhase::Moved, 620.0, 410.0);
        app.update();
        assert!(app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW));
        assert!(app
            .world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left));
        assert!(
            (app.world().resource::<AccumulatedMouseMotion>().delta - Vec2::new(20.0, 10.0))
                .length()
                < 0.01
        );
        touch(&mut app, w, 1, TouchPhase::Canceled, 200.0, 700.0);
        touch(&mut app, w, 3, TouchPhase::Ended, 920.0, 900.0);
        app.update();
        assert!(!app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW));
        assert!(!app
            .world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left));
    }
    #[test]
    fn entering_a_menu_releases_game_controls_and_uses_menu_navigation() {
        let (mut app, w) = scene();
        touch(&mut app, w, 1, TouchPhase::Started, 920.0, 900.0);
        app.update();
        app.world_mut()
            .resource_mut::<NextState<nvf_play::PlayState>>()
            .set(nvf_play::PlayState::Menu);
        app.update();
        assert!(!app
            .world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left));
        touch(&mut app, w, 2, TouchPhase::Started, 150.0, 600.0);
        app.update();
        assert!(app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .just_pressed(KeyCode::ArrowUp));
    }
    #[test]
    fn suspension_releases_held_controls_and_requests_pause() {
        let (mut app, w) = scene();
        touch(&mut app, w, 1, TouchPhase::Started, 920.0, 900.0);
        app.update();
        app.world_mut().write_message(AppLifecycle::Suspended);
        app.update();
        assert!(!app
            .world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left));
        assert!(app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .just_pressed(KeyCode::Escape));
        app.update();
        assert!(!app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::Escape));
    }
    fn physical_key(app: &mut App, window: Entity, state: ButtonState) {
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyW,
            logical_key: Key::Character("w".into()),
            state,
            text: None,
            repeat: false,
            window,
        });
    }
    fn physical_mouse(app: &mut App, window: Entity, state: ButtonState) {
        app.world_mut().write_message(MouseButtonInput {
            button: MouseButton::Left,
            state,
            window,
        });
    }
    fn move_finger(app: &mut App, w: Entity) {
        touch(app, w, 1, TouchPhase::Started, 200.0, 800.0);
        touch(app, w, 1, TouchPhase::Moved, 200.0, 700.0);
    }
    #[test]
    fn lifting_movement_touch_preserves_a_physical_key_and_its_edges() {
        let (mut app, w) = scene();
        physical_key(&mut app, w, ButtonState::Pressed);
        app.update();
        move_finger(&mut app, w);
        app.update();
        touch(&mut app, w, 1, TouchPhase::Ended, 200.0, 700.0);
        app.update();
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        assert!(keys.pressed(KeyCode::KeyW));
        assert!(!keys.just_released(KeyCode::KeyW) && !keys.just_pressed(KeyCode::KeyW));
        physical_key(&mut app, w, ButtonState::Released);
        app.update();
        assert!(app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .just_released(KeyCode::KeyW));
    }
    #[test]
    fn releasing_physical_key_preserves_movement_touch_without_false_edges() {
        let (mut app, w) = scene();
        move_finger(&mut app, w);
        app.update();
        physical_key(&mut app, w, ButtonState::Pressed);
        app.update();
        physical_key(&mut app, w, ButtonState::Released);
        app.update();
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        assert!(keys.pressed(KeyCode::KeyW));
        assert!(!keys.just_released(KeyCode::KeyW) && !keys.just_pressed(KeyCode::KeyW));
        touch(&mut app, w, 1, TouchPhase::Ended, 200.0, 700.0);
        app.update();
        assert!(app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .just_released(KeyCode::KeyW));
    }
    #[test]
    fn touch_and_mouse_attack_holds_release_only_after_both_sources_release() {
        for touch_releases_first in [false, true] {
            let (mut app, w) = scene();
            physical_mouse(&mut app, w, ButtonState::Pressed);
            app.update();
            touch(&mut app, w, 1, TouchPhase::Started, 920.0, 900.0);
            app.update();
            if touch_releases_first {
                touch(&mut app, w, 1, TouchPhase::Ended, 920.0, 900.0);
            } else {
                physical_mouse(&mut app, w, ButtonState::Released);
            }
            app.update();
            let mouse = app.world().resource::<ButtonInput<MouseButton>>();
            assert!(mouse.pressed(MouseButton::Left));
            assert!(
                !mouse.just_pressed(MouseButton::Left) && !mouse.just_released(MouseButton::Left)
            );
            if touch_releases_first {
                physical_mouse(&mut app, w, ButtonState::Released);
            } else {
                touch(&mut app, w, 1, TouchPhase::Ended, 920.0, 900.0);
            }
            app.update();
            assert!(app
                .world()
                .resource::<ButtonInput<MouseButton>>()
                .just_released(MouseButton::Left));
        }
    }
    #[test]
    fn fast_physical_key_and_mouse_taps_keep_both_edges_after_touch_ownership() {
        let (mut app, w) = scene();
        move_finger(&mut app, w);
        touch(&mut app, w, 2, TouchPhase::Started, 920.0, 900.0);
        app.update();
        touch(&mut app, w, 1, TouchPhase::Ended, 200.0, 700.0);
        touch(&mut app, w, 2, TouchPhase::Ended, 920.0, 900.0);
        app.update();
        physical_key(&mut app, w, ButtonState::Pressed);
        physical_key(&mut app, w, ButtonState::Released);
        physical_mouse(&mut app, w, ButtonState::Pressed);
        physical_mouse(&mut app, w, ButtonState::Released);
        app.update();
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        assert!(!keys.pressed(KeyCode::KeyW));
        assert!(keys.just_pressed(KeyCode::KeyW) && keys.just_released(KeyCode::KeyW));
        let mouse = app.world().resource::<ButtonInput<MouseButton>>();
        assert!(!mouse.pressed(MouseButton::Left));
        assert!(mouse.just_pressed(MouseButton::Left) && mouse.just_released(MouseButton::Left));
    }
    #[test]
    fn a_touch_held_through_a_physical_tap_masks_both_physical_edges() {
        let (mut app, w) = scene();
        move_finger(&mut app, w);
        app.update();
        physical_key(&mut app, w, ButtonState::Pressed);
        physical_key(&mut app, w, ButtonState::Released);
        app.update();
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        assert!(keys.pressed(KeyCode::KeyW));
        assert!(!keys.just_pressed(KeyCode::KeyW) && !keys.just_released(KeyCode::KeyW));
    }
    #[test]
    fn touch_start_and_end_in_one_frame_preserve_tap_edges() {
        let (mut app, w) = scene();
        touch(&mut app, w, 1, TouchPhase::Started, 920.0, 900.0);
        touch(&mut app, w, 1, TouchPhase::Ended, 920.0, 900.0);
        app.update();
        let mouse = app.world().resource::<ButtonInput<MouseButton>>();
        assert!(!mouse.pressed(MouseButton::Left));
        assert!(mouse.just_pressed(MouseButton::Left) && mouse.just_released(MouseButton::Left));
    }
    #[test]
    fn dedicated_terminal_capture_releases_touch_only_owners_before_state_transition_and_drains_close() {
        for native_sneak in [false, true] {
            let (mut app, w) = controls_scene();
            app.world_mut().resource_mut::<Layout>().config.aiming.toggle_aim = true;
            app.world_mut().resource_mut::<Layout>().config.aiming.hold_sneak = true;
            app.world_mut().resource_mut::<drawer::Options>().enabled = true;
            if native_sneak {
                let key = app.world().resource::<Bindings>().get(Control::CrouchSneak).key.unwrap();
                raw_key(&mut app, w, key, ButtonState::Pressed);
                app.update();
                raw_key(&mut app, w, key, ButtonState::Released);
                app.update();
            }
            tap_control(&mut app, w, 41, Control::Block);
            button_touch(&mut app, w, 42, TouchPhase::Started, Control::CrouchSneak);
            app.update();
            assert!(aim_requested(&app));
            assert!(app.world().resource::<nvf_play::control::HeldSneakInput>().active);
            raw_key(&mut app, w, KeyCode::KeyW, ButtonState::Pressed);
            app.world_mut().write_message(MouseButtonInput { window: w, button: MouseButton::Middle, state: ButtonState::Pressed });
            app.init_resource::<nvf_play::terminal::touch::Settings>();
            app.world_mut().resource_mut::<nvf_play::terminal::touch::Settings>().enabled = true;
            app.insert_resource(nvf_play::terminal::TerminalScreen(Some(nvf_play::terminal::Session {
                reference: fnv_sim::FormId(1), base: fnv_sim::FormId(2),
                page: nvf_play::terminal::Page::LockedOut(["Local fixture".into(), "No game data".into()]),
            })));
            button_touch(&mut app, w, 43, TouchPhase::Started, Control::Use);
            app.update();
            assert_eq!(*app.world().resource::<State<nvf_play::PlayState>>().get(), nvf_play::PlayState::Playing);
            let state = app.world().resource::<TouchState>();
            assert!(state.contacts.is_empty() && !state.aim_latched && state.keys.is_empty() && state.mouse.is_empty());
            assert!(state.physical_keys.contains(&KeyCode::KeyW) && state.physical_mouse.contains(&MouseButton::Middle));
            assert!(!aim_requested(&app));
            assert!(!app.world().resource::<nvf_play::control::HeldSneakInput>().active);
            assert_eq!(sneak_requested(&app), native_sneak);
            assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyW));
            assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Middle));
            assert!(!app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
            assert_eq!(drawer_visibility(&mut app), Visibility::Hidden);
            app.world_mut().resource_mut::<nvf_play::terminal::TerminalScreen>().0 = None;
            tap_control(&mut app, w, 44, Control::Block);
            assert!(!aim_requested(&app), "the first closing batch still belongs to the modal screen");
            tap_control(&mut app, w, 45, Control::Block);
            assert!(aim_requested(&app), "a fresh subsequent input returns to the normal layout");
        }
    }
    #[test]
    fn platform_loading_cover_cancels_touch_latch_sneak_and_drawer_but_keeps_physical_owners() {
        for native_sneak in [false, true] {
            let (mut app, w) = controls_scene();
            app.init_resource::<nvf_play::pad::PlatformInputGate>();
            app.world_mut().resource_mut::<Layout>().config.aiming.toggle_aim = true;
            app.world_mut().resource_mut::<Layout>().config.aiming.hold_sneak = true;
            if native_sneak {
                let key = app.world().resource::<Bindings>().get(Control::CrouchSneak).key.unwrap();
                raw_key(&mut app, w, key, ButtonState::Pressed); app.update();
                raw_key(&mut app, w, key, ButtonState::Released); app.update();
            }
            tap_control(&mut app, w, 80, Control::Block);
            button_touch(&mut app, w, 81, TouchPhase::Started, Control::CrouchSneak); app.update();
            assert!(aim_requested(&app) && app.world().resource::<nvf_play::control::HeldSneakInput>().active);
            raw_key(&mut app, w, KeyCode::KeyW, ButtonState::Pressed);
            app.world_mut().write_message(MouseButtonInput { window: w, button: MouseButton::Middle, state: ButtonState::Pressed });
            app.world_mut().resource_mut::<nvf_play::pad::PlatformInputGate>().set_blocked(true);
            button_touch(&mut app, w, 82, TouchPhase::Started, Control::Use); app.update();
            let state = app.world().resource::<TouchState>();
            assert!(state.contacts.is_empty() && !state.aim_latched);
            assert!(state.physical_keys.contains(&KeyCode::KeyW) && state.physical_mouse.contains(&MouseButton::Middle));
            assert!(!app.world().resource::<nvf_play::control::HeldSneakInput>().active && !aim_requested(&app));
            assert_eq!(sneak_requested(&app), native_sneak);
            app.world_mut().resource_mut::<nvf_play::pad::PlatformInputGate>().set_blocked(false);
            button_touch(&mut app, w, 82, TouchPhase::Ended, Control::Use); app.update();
            assert!(app.world().resource::<TouchState>().contacts.is_empty());
            tap_control(&mut app, w, 83, Control::Block);
            assert!(aim_requested(&app), "a fresh deliberate action works after cover release");
        }
    }

    #[test]
    fn dialogue_capture_releases_held_touch_sources_preserves_hardware_and_drains_closure() {
        for native_sneak in [false,true] {
            let (mut app,w)=controls_scene();
            app.world_mut().resource_mut::<Layout>().config.aiming.toggle_aim=true;
            app.world_mut().resource_mut::<Layout>().config.aiming.hold_sneak=true;
            if native_sneak {
                let key=app.world().resource::<Bindings>().get(Control::CrouchSneak).key.unwrap();
                raw_key(&mut app,w,key,ButtonState::Pressed);app.update();
                raw_key(&mut app,w,key,ButtonState::Released);app.update();
            }
            tap_control(&mut app,w,61,Control::Block);
            button_touch(&mut app,w,62,TouchPhase::Started,Control::CrouchSneak);app.update();
            assert!(aim_requested(&app));assert!(app.world().resource::<nvf_play::control::HeldSneakInput>().active);
            raw_key(&mut app,w,KeyCode::KeyW,ButtonState::Pressed);
            app.world_mut().write_message(MouseButtonInput {window:w,button:MouseButton::Middle,state:ButtonState::Pressed});
            app.insert_resource(nvf_play::dialogue_touch::TouchDialogue {enabled:true,..default()});
            app.world_mut().resource_mut::<NextState<nvf_play::PlayState>>().set(nvf_play::PlayState::Dialogue);
            button_touch(&mut app,w,63,TouchPhase::Started,Control::Use);app.update();
            // Held gameplay sources are released. A press that lands on the menu
            // overlay kept for dialogue (A12) owns only that menu key, never gameplay.
            let survivors: Vec<Role> = app.world().resource::<TouchState>().contacts.values().map(|c| c.role).collect();
            assert!(survivors.iter().all(|r| matches!(r, Role::DialogueOverlay(i) if !BUTTONS[*i].playing)), "{survivors:?}");
            let enter_overlay = survivors.iter().any(|r| matches!(r, Role::DialogueOverlay(i) if BUTTONS[*i].key == Some(KeyCode::Enter)));
            assert!(!aim_requested(&app));assert!(!app.world().resource::<nvf_play::control::HeldSneakInput>().active);
            assert_eq!(sneak_requested(&app),native_sneak);
            assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyW));
            assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Middle));
            assert_eq!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::Enter), enter_overlay);
            button_touch(&mut app,w,63,TouchPhase::Ended,Control::Use);app.update();
            assert!(app.world().resource::<TouchState>().contacts.is_empty());
            app.world_mut().resource_mut::<NextState<nvf_play::PlayState>>().set(nvf_play::PlayState::Playing);
            tap_control(&mut app,w,64,Control::Block);assert!(!aim_requested(&app));
            tap_control(&mut app,w,65,Control::Block);assert!(aim_requested(&app));
        }
    }

    #[test]
    fn dialogue_preserves_menu_overlay_without_leaking_gameplay_input() {
        let (mut app,w)=scene();
        app.insert_resource(nvf_play::dialogue_touch::TouchDialogue {enabled:true,..default()});
        app.world_mut().resource_mut::<NextState<nvf_play::PlayState>>().set(nvf_play::PlayState::Dialogue);
        app.update();app.update();
        touch(&mut app,w,1,TouchPhase::Started,920.0,900.0);app.update();
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::Enter));
        assert!(!app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(1));
        let world=app.world_mut();let mut q=world.query::<(&TouchVisual,&Visibility)>();
        assert!(q.iter(world).any(|(kind,v)| matches!(kind, TouchVisual::Button(i) if BUTTONS[*i].key == Some(KeyCode::Enter)) && *v == Visibility::Inherited));
        assert!(q.iter(world).all(|(kind,v)| !matches!(kind, TouchVisual::Button(i) if BUTTONS[*i].playing) || *v == Visibility::Hidden));
    }

    #[test]
    fn local_gyro_action_uses_its_semantic_glyph_without_settings_or_help_hud_icons() {
        let (mut app,_)=scene();
        assert_eq!(BUTTONS[gyro::BUTTON].text,"Gyro");
        let world=app.world_mut();
        let mut glyphs=world.query::<(&ChildOf,&icons::TouchIcon)>();
        assert!(glyphs.iter(world).any(|(parent,icon)|
            matches!(world.get::<TouchVisual>(parent.parent()),Some(TouchVisual::Button(gyro::BUTTON)))
                && Some(icon.0)==icons::semantic_index("gyro")));
        let mut utilities=world.query::<(&TouchVisual,&icons::TouchIcon)>();
        assert!(!utilities.iter(world).any(|(_,i)|Some(i.0)==icons::semantic_index("settings") || Some(i.0)==icons::semantic_index("help")),
            "persistent settings/help nodes must not be spawned");
    }

    fn movement_scene() -> (App, Entity) {
        let (mut app, window) = scene();
        app.insert_resource(movement::Options::default())
            .init_resource::<nvf_play::player::MoveIntent>()
            .init_resource::<nvf_play::control::PlayerControl>()
            .init_resource::<nvf_play::control::ControlLocks>()
            .init_resource::<nvf_play::control::LockStats>()
            .init_resource::<nvf_play::furniture::PlayerSeat>()
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_millis(40)))
            .add_systems(Update, (nvf_play::player::read_input, nvf_play::control::read_controls, nvf_play::control::enforce_locks).chain());
        app.update(); (app, window)
    }
    fn native_velocity(app: &App) -> [f32; 2] {
        nvf_play::player::velocity(app.world().resource::<nvf_play::player::MoveIntent>(), 0.0, nvf_play::player::MoveSpeeds::OBSERVED)
    }
    fn analog_push(app: &mut App, w: Entity, id: u64, amount: f32) {
        let layout = app.world().resource::<Layout>();
        let radius = layout.config.diameter(Target::Stick, Vec2::splat(1000.0))*0.5;
        let dead = layout.config.aiming.dead_zone;
        touch(app,w,id,TouchPhase::Moved,200.0,700.0-radius*(dead+amount*(1.0-dead)));
        app.update();
    }
    #[test]
    fn default_touch_stick_changes_actual_native_velocity_continuously_and_keeps_legacy_scheme() {
        let (mut app,w)=movement_scene();
        touch(&mut app,w,1,TouchPhase::Started,200.0,700.0);app.update();
        analog_push(&mut app,w,1,0.25);let gentle=native_velocity(&app)[1];
        analog_push(&mut app,w,1,0.5);let stronger=native_velocity(&app)[1];
        assert!(gentle>0.0 && (stronger/gentle-2.0).abs()<0.01,"actual walk speed must scale, not merely its direction");
        analog_push(&mut app,w,1,1.0);
        assert!((native_velocity(&app)[1]-nvf_play::player::MoveSpeeds::OBSERVED.run).abs()<0.01);
        assert!(!app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyW),"analog does not invent a hardware key");
        touch(&mut app,w,1,TouchPhase::Ended,200.0,500.0);app.update();assert_eq!(native_velocity(&app),[0.0;2]);
        app.world_mut().resource_mut::<movement::Options>().analog=false;app.update();
        touch(&mut app,w,2,TouchPhase::Started,200.0,700.0);app.update();analog_push(&mut app,w,2,0.25);
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyW));
        assert!((native_velocity(&app)[1]-nvf_play::player::MoveSpeeds::OBSERVED.run).abs()<0.01,"explicit legacy scheme preserves directional speed");
    }
    #[test]
    fn analog_touch_obeys_physical_direction_priority_script_locks_and_platform_auto_move_hold() {
        let (mut app,w)=movement_scene();
        touch(&mut app,w,1,TouchPhase::Started,200.0,700.0);app.update();analog_push(&mut app,w,1,0.25);
        raw_key(&mut app,w,KeyCode::KeyW,ButtonState::Pressed);app.update();
        assert!((native_velocity(&app)[1]-nvf_play::player::MoveSpeeds::OBSERVED.run).abs()<0.01);
        touch(&mut app,w,1,TouchPhase::Canceled,200.0,600.0);app.update();
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyW));
        app.world_mut().resource_mut::<nvf_play::control::ControlLocks>().movement=true;app.update();assert_eq!(native_velocity(&app),[0.0;2]);
        app.world_mut().resource_mut::<nvf_play::control::ControlLocks>().movement=false;
        raw_key(&mut app,w,KeyCode::KeyW,ButtonState::Released);app.update();
        touch(&mut app,w,2,TouchPhase::Started,200.0,700.0);app.update();
        app.insert_resource(nvf_play::script_input::ScriptBlocked{keys:vec![KeyCode::KeyW],..default()});
        analog_push(&mut app,w,2,0.8);assert_eq!(native_velocity(&app),[0.0;2],"disabled bound Forward cannot be bypassed by analog");
        app.world_mut().resource_mut::<nvf_play::control::PlayerControl>().auto_move=true;
        app.init_resource::<nvf_play::pad::PlatformInputGate>();app.world_mut().resource_mut::<nvf_play::pad::PlatformInputGate>().set_blocked(true);
        app.update();assert_eq!(native_velocity(&app),[0.0;2]);
        assert!(app.world().resource::<nvf_play::control::PlayerControl>().auto_move,"cover neutralizes intentional toggle without replacing it");
    }
    #[test]
    fn fire_drag_looks_while_native_trigger_stays_held_and_preserves_physical_fire_owner() {
        let (mut app,w)=movement_scene();app.world_mut().resource_mut::<nvf_play::control::PlayerControl>().weapon_out=true;
        button_touch(&mut app,w,1,TouchPhase::Started,Control::Use);app.update();
        assert!(app.world().resource::<nvf_play::control::PlayerControl>().trigger);
        let at=app.world().resource::<TouchState>().contacts[&1].position*1000.0;
        touch(&mut app,w,1,TouchPhase::Moved,at.x+50.0,at.y-15.0);app.update();
        let delta=app.world().resource::<AccumulatedMouseMotion>().delta;
        assert!((delta-Vec2::new(50.0,-15.0)).length()<0.01);
        assert!(app.world().resource::<nvf_play::control::PlayerControl>().trigger,"dragging outside the circle keeps the fire contact");
        physical_mouse(&mut app,w,ButtonState::Pressed);app.update();
        touch(&mut app,w,1,TouchPhase::Canceled,at.x+50.0,at.y-15.0);app.update();
        assert!(app.world().resource::<nvf_play::control::PlayerControl>().trigger);
        physical_mouse(&mut app,w,ButtonState::Released);app.update();assert!(!app.world().resource::<nvf_play::control::PlayerControl>().trigger);
        touch(&mut app,w,1,TouchPhase::Moved,at.x+100.0,at.y);app.update();assert_eq!(app.world().resource::<AccumulatedMouseMotion>().delta,Vec2::ZERO);
    }
    #[test]
    fn fire_drag_and_empty_look_have_one_owner_and_stationary_fire_scheme_remains_available() {
        let (mut app,w)=movement_scene();
        touch(&mut app,w,1,TouchPhase::Started,600.0,400.0);app.update();
        button_touch(&mut app,w,2,TouchPhase::Started,Control::Use);app.update();
        let at=app.world().resource::<TouchState>().contacts[&2].position*1000.0;
        touch(&mut app,w,2,TouchPhase::Moved,at.x+70.0,at.y);app.update();assert_eq!(app.world().resource::<AccumulatedMouseMotion>().delta,Vec2::ZERO);
        touch(&mut app,w,1,TouchPhase::Moved,620.0,410.0);app.update();assert!((app.world().resource::<AccumulatedMouseMotion>().delta-Vec2::new(20.0,10.0)).length()<0.01);
        app.world_mut().resource_mut::<movement::Options>().fire_drag=false;app.update();
        button_touch(&mut app,w,3,TouchPhase::Started,Control::Use);app.update();
        touch(&mut app,w,3,TouchPhase::Moved,at.x+60.0,at.y);app.update();assert_eq!(app.world().resource::<AccumulatedMouseMotion>().delta,Vec2::ZERO);
    }
    #[test]
    fn run_lock_uses_rebound_native_auto_move_and_fresh_push_or_target_tap_stops_it() {
        let (mut app,w)=movement_scene();
        app.world_mut().resource_mut::<movement::Options>().run_lock=true;
        app.world_mut().resource_mut::<Bindings>().rebind(Control::AutoMove,nvf_play::bindings::Input::Key(KeyCode::F8));app.update();
        touch(&mut app,w,1,TouchPhase::Started,200.0,700.0);app.update();
        let target=movement::lock_rect(app.world().resource::<Layout>(),app.world().resource::<TouchState>(),Vec2::splat(1000.0)).center();
        touch(&mut app,w,1,TouchPhase::Moved,target.x,target.y);app.update();
        touch(&mut app,w,1,TouchPhase::Ended,target.x,target.y);app.update();app.update();
        assert!(app.world().resource::<nvf_play::control::PlayerControl>().auto_move);assert_eq!(native_velocity(&app)[1],nvf_play::player::MoveSpeeds::OBSERVED.run);
        let target=movement::lock_rect(app.world().resource::<Layout>(),app.world().resource::<TouchState>(),Vec2::splat(1000.0)).center();
        touch(&mut app,w,2,TouchPhase::Started,target.x,target.y);touch(&mut app,w,2,TouchPhase::Ended,target.x,target.y);app.update();app.update();
        assert!(!app.world().resource::<nvf_play::control::PlayerControl>().auto_move);assert_eq!(native_velocity(&app),[0.0;2]);
        app.world_mut().resource_mut::<nvf_play::control::PlayerControl>().auto_move=true;
        touch(&mut app,w,3,TouchPhase::Started,200.0,700.0);app.update();analog_push(&mut app,w,3,0.5);
        assert!(!app.world().resource::<nvf_play::control::PlayerControl>().auto_move,"fresh admitted forward/back push is native Auto Move cancellation");
    }
    #[test]
    fn run_lock_cancel_out_and_back_and_disabled_binding_never_toggle_native_auto_move() {
        let (mut app,w)=movement_scene();app.world_mut().resource_mut::<movement::Options>().run_lock=true;app.update();
        let target=movement::lock_rect(app.world().resource::<Layout>(),app.world().resource::<TouchState>(),Vec2::splat(1000.0)).center();
        for cancel in [false,true] {
            touch(&mut app,w,4,TouchPhase::Started,target.x,target.y);app.update();
            if cancel {touch(&mut app,w,4,TouchPhase::Canceled,target.x,target.y);} else {
                touch(&mut app,w,4,TouchPhase::Moved,target.x+60.0,target.y);touch(&mut app,w,4,TouchPhase::Ended,target.x,target.y);
            }
            app.update();assert!(!app.world().resource::<nvf_play::control::PlayerControl>().auto_move);
        }
        let key=app.world().resource::<Bindings>().get(Control::AutoMove).key.unwrap();
        app.insert_resource(nvf_play::script_input::ScriptBlocked{keys:vec![key],..default()});
        touch(&mut app,w,5,TouchPhase::Started,target.x,target.y);touch(&mut app,w,5,TouchPhase::Ended,target.x,target.y);app.update();
        assert!(!app.world().resource::<nvf_play::control::PlayerControl>().auto_move);
    }
    #[derive(Resource,Default)]
    struct VatsPresses(usize);
    fn count_vats(input:nvf_play::bindings::ControlInput,mut seen:ResMut<VatsPresses>){seen.0+=usize::from(input.just_pressed(Control::Vats));}
    #[test]
    fn optional_vats_double_tap_uses_binding_once_and_drops_same_batch_fire_tail() {
        let (mut app,w)=movement_scene();app.init_resource::<VatsPresses>().add_systems(Update,count_vats);
        app.world_mut().resource_mut::<movement::Options>().vats_taps=true;
        app.world_mut().resource_mut::<Bindings>().rebind(Control::Vats,nvf_play::bindings::Input::Key(KeyCode::F9));
        app.world_mut().resource_mut::<nvf_play::control::PlayerControl>().weapon_out=true;app.update();
        touch(&mut app,w,1,TouchPhase::Started,600.0,400.0);touch(&mut app,w,1,TouchPhase::Ended,600.0,400.0);app.update();assert_eq!(app.world().resource::<VatsPresses>().0,0);
        touch(&mut app,w,2,TouchPhase::Started,602.0,400.0);touch(&mut app,w,2,TouchPhase::Ended,602.0,400.0);
        button_touch(&mut app,w,3,TouchPhase::Started,Control::Use);app.update();
        assert_eq!(app.world().resource::<VatsPresses>().0,1);assert!(!app.world().resource::<nvf_play::control::PlayerControl>().trigger);
        app.update();assert_eq!(app.world().resource::<VatsPresses>().0,1);assert!(!app.world().resource::<nvf_play::control::PlayerControl>().trigger);
        button_touch(&mut app,w,4,TouchPhase::Started,Control::Use);app.update();assert!(app.world().resource::<nvf_play::control::PlayerControl>().trigger);
    }

    #[test]
    fn optional_vats_no_target_hold_jitter_keeps_same_batch_fire_tail_quarantined() {
        let (mut app,w)=movement_scene();
        app.init_resource::<VatsPresses>().add_systems(Update,count_vats);
        app.world_mut().resource_mut::<movement::Options>().vats_taps=true;
        app.world_mut().resource_mut::<Bindings>().rebind(Control::Vats,nvf_play::bindings::Input::Key(KeyCode::F9));
        app.world_mut().resource_mut::<nvf_play::control::PlayerControl>().weapon_out=true;
        app.update();
        for id in [41,42] {
            touch(&mut app,w,id,TouchPhase::Started,600.0,400.0);
            touch(&mut app,w,id,TouchPhase::Moved,604.0,401.0);
            touch(&mut app,w,id,TouchPhase::Ended,602.0,400.0);
            if id==42 {button_touch(&mut app,w,43,TouchPhase::Started,Control::Use);}
            app.update();
            assert_eq!(app.world().resource::<VatsPresses>().0,usize::from(id==42));
            assert!(!app.world().resource::<nvf_play::control::PlayerControl>().trigger);
        }
        app.update();
        assert_eq!(app.world().resource::<VatsPresses>().0,1);
        assert!(!app.world().resource::<nvf_play::control::PlayerControl>().trigger);
    }

    #[derive(Resource,Default)]
    struct VatsNativeActivations(usize);
    fn count_vats_native_activations(mut events:MessageReader<nvf_play::world_sounds::Activated>,mut seen:ResMut<VatsNativeActivations>) {
        seen.0+=events.read().count();
    }
    #[test]
    fn optional_vats_yields_to_actual_sampled_native_hold_target_and_confirms_that_hold_once() {
        let (mut app,w,door,point)=super::world_outcome_tests::scene("Tap");
        let setting=app.world().resource::<nvf_play::settings::Settings>().index("touch-world-activation-v1").unwrap();
        assert!(nvf_play::settings::pick(app.world_mut(),setting,"Hold"));
        app.init_resource::<VatsPresses>().init_resource::<VatsNativeActivations>()
            .add_systems(Update,(count_vats,count_vats_native_activations));
        app.world_mut().resource_mut::<movement::Options>().vats_taps=true;
        for id in [51,52] {
            touch(&mut app,w,id,TouchPhase::Started,point.x,point.y);app.update();
            assert_eq!(app.world().resource::<nvf_play::activate::touch::TouchActivation>().target().map(|target|target.form_id),Some(door.0),"native probe must actually claim this target");
            touch(&mut app,w,id,TouchPhase::Ended,point.x,point.y);app.update();
            assert_eq!(app.world().resource::<VatsPresses>().0,0,"a sampled world target keeps priority over VATS");
            assert!(!app.world().resource::<nvf_play::sim::SimState>().world.peek(door).unwrap().open,"short Hold release does not confirm native activation");
        }
        touch(&mut app,w,53,TouchPhase::Started,point.x,point.y);app.update();
        for _ in 0..40 {app.update();}
        assert!(app.world().resource::<nvf_play::sim::SimState>().world.peek(door).unwrap().open,"the same actual target still activates through a full Hold");
        assert_eq!(app.world().resource::<VatsNativeActivations>().0,1);
        touch(&mut app,w,53,TouchPhase::Ended,point.x,point.y);app.update();app.update();
        assert_eq!(app.world().resource::<VatsNativeActivations>().0,1);
        assert_eq!(app.world().resource::<VatsPresses>().0,0);
    }
    #[test]
    fn vats_gesture_rejects_out_and_back_cancel_and_controller_return_touch() {
        let (mut app,w)=movement_scene();app.init_resource::<VatsPresses>().add_systems(Update,count_vats);
        app.world_mut().resource_mut::<movement::Options>().vats_taps=true;app.update();
        for cancel in [false,true] {
            touch(&mut app,w,1,TouchPhase::Started,600.0,400.0);app.update();
            if cancel {touch(&mut app,w,1,TouchPhase::Canceled,600.0,400.0);} else {
                touch(&mut app,w,1,TouchPhase::Moved,650.0,400.0);touch(&mut app,w,1,TouchPhase::Ended,600.0,400.0);
            }
            app.update();touch(&mut app,w,2,TouchPhase::Started,600.0,400.0);touch(&mut app,w,2,TouchPhase::Ended,600.0,400.0);app.update();
            assert_eq!(app.world().resource::<VatsPresses>().0,0);
            app.world_mut().resource_mut::<TouchState>().cancel_touch();
        }
        app.add_plugins(nvf_play::pad::InputSwitchPlugin);app.world_mut().spawn(Gamepad::default());
        app.world_mut().resource_mut::<Bindings>().device=nvf_play::bindings::InputDevice::Controller;
        touch(&mut app,w,3,TouchPhase::Started,200.0,700.0);app.update();
        assert_eq!(native_velocity(&app),[0.0;2]);assert!(!app.world().resource::<nvf_play::pad::TouchDeviceSwitch>().allows_actions());
        touch(&mut app,w,3,TouchPhase::Moved,200.0,600.0);app.update();assert_eq!(native_velocity(&app),[0.0;2]);
        assert_eq!(app.world().resource::<VatsPresses>().0,0);
    }

    #[derive(Resource,Default)]
    struct MovementQueries { control:bool, digital:bool, fresh:bool, script:bool, raw_held:bool }
    fn observe_movement(input:nvf_play::bindings::ControlInput,keys:Res<ButtonInput<KeyCode>>,mut seen:ResMut<MovementQueries>) {
        let script=fnv_game::script_input::ScriptInput { virtual_controls:nvf_play::script_input::admitted_virtual_controls(&input), ..Default::default() };
        seen.control=input.pressed(Control::Forward);seen.digital=input.digital_movement_pressed(Control::Forward);
        seen.fresh=input.just_pressed(Control::Forward);seen.script=script.control_pressed(0,false);
        seen.raw_held=keys.pressed(KeyCode::KeyW);
    }
    #[test]
    fn analog_is_script_visible_without_quantizing_its_magnitude_or_faking_a_rebound_key() {
        let (mut app,w)=movement_scene();app.init_resource::<MovementQueries>().add_systems(Update,observe_movement);
        app.world_mut().resource_mut::<Bindings>().rebind(Control::Forward,nvf_play::bindings::Input::Key(KeyCode::F10));
        touch(&mut app,w,1,TouchPhase::Started,200.0,700.0);app.update();analog_push(&mut app,w,1,0.25);
        let seen=app.world().resource::<MovementQueries>();
        assert!(seen.control && seen.script && seen.fresh && !seen.digital && !seen.raw_held);
        assert!((app.world().resource::<nvf_play::player::MoveIntent>().analog_scale-0.25).abs()<0.001);
        assert!(!app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F10));
        app.update();assert!(!app.world().resource::<MovementQueries>().fresh);
        raw_key(&mut app,w,KeyCode::F10,ButtonState::Pressed);app.update();
        assert!(app.world().resource::<MovementQueries>().digital);assert_eq!(app.world().resource::<nvf_play::player::MoveIntent>().analog_scale,1.0);
        raw_key(&mut app,w,KeyCode::F10,ButtonState::Released);
        app.insert_resource(nvf_play::script_input::ScriptBlocked{keys:vec![KeyCode::F10],..default()});app.update();
        let seen=app.world().resource::<MovementQueries>();assert!(!seen.control && !seen.script && !seen.digital);assert_eq!(native_velocity(&app),[0.0;2]);
        app.world_mut().resource_mut::<nvf_play::script_input::ScriptBlocked>().keys.clear();
        app.init_resource::<nvf_play::pad::PlatformInputGate>();app.world_mut().resource_mut::<nvf_play::pad::PlatformInputGate>().set_blocked(true);app.update();
        assert!(!app.world().resource::<MovementQueries>().control && !app.world().resource::<MovementQueries>().script);
    }
    #[test]
    fn auto_move_target_has_a_specific_large_node_and_hides_during_platform_capture() {
        let (mut app,w)=movement_scene();app.world_mut().resource_mut::<movement::Options>().run_lock=true;app.update();
        let marker=app.world_mut().query_filtered::<Entity,With<movement::LockMarker>>().single(app.world()).unwrap();
        assert_eq!(*app.world().get::<Visibility>(marker).unwrap(),Visibility::Inherited);
        let node=app.world().get::<Node>(marker).unwrap();assert_eq!((node.width,node.height),(px(56.0),px(56.0)));
        app.init_resource::<nvf_play::pad::PlatformInputGate>();app.world_mut().resource_mut::<nvf_play::pad::PlatformInputGate>().set_blocked(true);
        touch(&mut app,w,1,TouchPhase::Started,200.0,700.0);app.update();
        assert_eq!(*app.world().get::<Visibility>(marker).unwrap(),Visibility::Hidden);
        assert!(app.world().resource::<TouchState>().contacts.is_empty());assert_eq!(native_velocity(&app),[0.0;2]);

    }

    fn mixed_hotbar_phone_scene(size: Vec2, preset: &str, count: usize) -> (App, Entity) {
        use nvf_play::{hotkeys::Hotkeys, settings};
        let (mut app, window) = movement_scene();
        app.insert_resource(settings::SettingsFile(None));
        let forms = Box::leak(Box::new(fnv_sim::FormStore::default()));
        let refs = Box::leak(Box::new(fnv_sim::RefTable::default()));
        app.insert_resource(nvf_play::sim::SimState {
            forms, refs, world: fnv_sim::World::new(forms, refs, fnv_sim::SimConfig::default()),
        }).init_resource::<Hotkeys>();
        hotbar::reset_collision_fixture(app.world_mut());
        app.world_mut().get_mut::<Window>(window).unwrap().resolution.set(size.x, size.y);
        app.world_mut().resource_mut::<Layout>().config = layout::Config::default();
        app.world_mut().resource_mut::<Layout>().config.apply_preset(preset);
        for key in ["touch-hotbar", "touch-gyro-v1", "touch-auto-move-lock-v1"] {
            let index = app.world().resource::<settings::Settings>().index(key).unwrap();
            assert_eq!(settings::flip(app.world_mut(), index), Some(true), "production setting {key}");
        }
        let index = app.world().resource::<settings::Settings>().index("touch-hotbar-slots").unwrap();
        let wanted = count.to_string();
        let current = (app.world().resource::<settings::Settings>().choices(index).unwrap().chosen)(app.world());
        if current != wanted {
            assert!(settings::pick(app.world_mut(), index, &wanted));
        } else {
            assert_eq!(current, wanted, "default production choice is already selected");
        }
        app.update(); // Viewport/settings admission drains this frame.
        app.update();
        (app, window)
    }

    fn mixed_hotbar_tiles(app: &mut App) -> Vec<(String, Rect)> {
        let world = app.world_mut();
        let mut out = Vec::new();
        let mut query = world.query::<(&Text, &ChildOf)>();
        for (label, parent) in query.iter(world) {
            if world.get::<GlobalZIndex>(parent.parent()).is_none_or(|z| z.0 != 2200) { continue; }
            let Some(node) = world.get::<Node>(parent.parent()) else { continue };
            let (Val::Px(x), Val::Px(y), Val::Px(w), Val::Px(h)) = (node.left, node.top, node.width, node.height) else { panic!("hotbar tile lost pixel bounds") };
            if label.0 == "Pins" || label.0.split_whitespace().next().is_some_and(|n| n.parse::<usize>().is_ok()) {
                out.push((label.0.clone(), Rect::from_corners(Vec2::new(x,y), Vec2::new(x+w,y+h))));
            }
        }
        out
    }

    #[test]
    fn enabled_hotbar_preserves_real_jump_stick_and_fixed_gyro_routes_on_phone_presets() {
        for size in [Vec2::new(640.0,360.0), Vec2::new(792.0,363.0)] {
            for (preset, _) in profiles::PRESETS {
                for count in [4,6,8] {
                    let (mut app, window) = mixed_hotbar_phone_scene(size, preset, count);
                    let case = format!("{size:?}/{preset}/{count}");
                    let jump = BUTTONS.iter().position(|b| b.control == Some(Control::Jump)).unwrap();
                    let center = app.world().resource::<Layout>().config.center(Target::Button(jump),size);
                    touch(&mut app,window,300,TouchPhase::Started,center.x,center.y); app.update();
                    assert!(app.world().resource::<nvf_play::player::MoveIntent>().jump, "{case}: hotbar stole Jump center");
                    touch(&mut app,window,300,TouchPhase::Ended,center.x,center.y); app.update();
                    app.world_mut().resource_mut::<nvf_play::player::MoveIntent>().jump = false;
                    let config = &app.world().resource::<Layout>().config;
                    let center = config.center(Target::Stick,size);
                    let r = config.diameter(Target::Stick,size)*0.45;
                    for (probe, direction) in [Vec2::NEG_Y,Vec2::new(0.707,-0.707),Vec2::new(-0.707,-0.707)].into_iter().enumerate() {
                        let point = center + direction*r;
                        assert_eq!(app.world().resource::<Layout>().config.hit(point,size,true,false),Some(Target::Stick));
                        let id = 310+probe as u64;
                        touch(&mut app,window,id,TouchPhase::Started,point.x,point.y); app.update();
                        touch(&mut app,window,id,TouchPhase::Moved,point.x,point.y-24.0); app.update();
                        assert!(native_velocity(&app)[1] > 0.0, "{case}: hotbar stole admitted stick point {point:?}");
                        touch(&mut app,window,id,TouchPhase::Ended,point.x,point.y-24.0); app.update();
                    }
                    let gyro = app.world().resource::<Layout>().config.center(Target::Button(gyro::BUTTON),size);
                    let expected_x = if *preset == "left" { 1.0 - 0.075 } else { 0.075 };
                    assert!((gyro - Vec2::new(size.x * expected_x, size.y * 0.48)).length() < 0.01, "{case}: fixed gyro anchor moved (got {gyro:?})");
                    let mut reader = bevy::ecs::message::MessageCursor::<nvf_play::weapon_wheel::TouchHotkey>::default();
                    reader.read(app.world().resource::<Messages<nvf_play::weapon_wheel::TouchHotkey>>()).for_each(drop);
                    touch(&mut app,window,320,TouchPhase::Started,gyro.x,gyro.y); app.update();
                    assert!(app.world().resource::<TouchState>().contacts.get(&320).is_some_and(|c| matches!(c.role,Role::Button(i) if i==gyro::BUTTON)), "{case}: hotbar stole gyro target");
                    assert!(app.world().resource::<gyro::State>().active(app.world().resource::<gyro::Options>()));
                    touch(&mut app,window,320,TouchPhase::Ended,gyro.x,gyro.y); app.update();
                    assert_eq!(reader.read(app.world().resource::<Messages<nvf_play::weapon_wheel::TouchHotkey>>()).count(),0,"{case}: gyro also activated hotbar");
                }
            }
        }
    }

    #[test]
    fn rendered_hotbar_slots_and_pins_are_reachable_without_covering_native_targets() {
        for size in [Vec2::new(640.0,360.0),Vec2::new(792.0,363.0)] {
            for (preset, _) in profiles::PRESETS {
                for count in [4,6,8] {
                    let (mut app, window) = mixed_hotbar_phone_scene(size,preset,count);
                    let case = format!("{size:?}/{preset}/{count}");
                    let tiles = mixed_hotbar_tiles(&mut app);
                    assert_eq!(tiles.len(),count+1,"{case}: missing rendered slot/tray");
                    for (label,rect) in &tiles {
                        assert!(rect.width() >= 48.0 - 1e-3 && rect.height() >= 48.0 - 1e-3, "{case}/{label}: rect={rect:?}");
                        let config=&app.world().resource::<Layout>().config;
                        for i in (0..9).chain(std::iter::once(gyro::BUTTON)) {
                            let center=config.center(Target::Button(i),size);
                            let point=center.clamp(rect.min+Vec2::splat(0.25),rect.max-Vec2::splat(0.25));
                            assert!(config.hit(point,size,true,false).is_none(),"{case}/{label}: visible bar intersects gameplay control {i}");
                        }
                        let point=config.center(Target::Stick,size).clamp(rect.min+Vec2::splat(0.25),rect.max-Vec2::splat(0.25));
                        assert!(config.hit(point,size,true,false).is_none(),"{case}/{label}: visible bar intersects stick");
                        let lock=movement::lock_rect(app.world().resource::<Layout>(),app.world().resource::<TouchState>(),size);
                        if movement::lock_available(app.world().resource::<Layout>(),app.world().resource::<TouchState>(),size) {
                            let overlap=rect.intersect(lock);
                            assert!(overlap.width()<=0.5 || overlap.height()<=0.5,"{case}/{label}: bar covers AutoMove target");
                        }
                    }
                    let mut reader=bevy::ecs::message::MessageCursor::<nvf_play::weapon_wheel::TouchHotkey>::default();
                    for (i,(label,rect)) in tiles.iter().enumerate() {
                        let center=rect.center();let id=400+i as u64;
                        touch(&mut app,window,id,TouchPhase::Started,center.x,center.y);
                        touch(&mut app,window,id,TouchPhase::Ended,center.x,center.y);app.update();
                        let emitted=reader.read(app.world().resource::<Messages<nvf_play::weapon_wheel::TouchHotkey>>()).map(|m|m.0).collect::<Vec<_>>();
                        if label=="Pins" { assert!(hotbar::tray_open_collision_fixture(app.world())); assert!(emitted.is_empty()); }
                        else { assert_eq!(emitted,[label.split_whitespace().next().unwrap().parse::<usize>().unwrap()-1],"{case}/{label}: rendered slot not reachable exactly once"); }
                    }
                }
            }
        }
    }

    #[test]
    fn hotbar_context_move_is_owned_ui_and_never_also_activates_raw_gyro() {
        let (mut app,window)=mixed_hotbar_phone_scene(Vec2::new(640.0,360.0),"classic",8);
        let tiles=mixed_hotbar_tiles(&mut app);
        let first=tiles.iter().find(|(label,_)|label.starts_with("1 ")).unwrap().1.center();
        touch(&mut app,window,600,TouchPhase::Started,first.x,first.y);app.update();
        for _ in 0..14 {app.update();} // Actual production hold threshold, not synthetic Context state.
        touch(&mut app,window,600,TouchPhase::Ended,first.x,first.y);app.update();
        let point={
            let world=app.world_mut();
            let mut query=world.query::<(&Text,&ChildOf)>();
            let parent=query.iter(world).find(|(text,_)|text.0=="Move bar").unwrap().1.parent();
            let node=world.get::<Node>(parent).unwrap();
            let (Val::Px(x),Val::Px(y),Val::Px(w),Val::Px(h))=(node.left,node.top,node.width,node.height) else {panic!("context pixel bounds")};
            Vec2::new(x+w*0.5,y+h*0.5)
        };
        touch(&mut app,window,601,TouchPhase::Started,point.x,point.y);app.update();
        assert!(!app.world().resource::<gyro::State>().active(app.world().resource::<gyro::Options>()),
            "hotbar context and raw gyro reader cannot own the same finger");
        touch(&mut app,window,601,TouchPhase::Ended,point.x,point.y);app.update();
        app.update(); // The existing context action enters the dedicated transaction.
        assert!(app.world().resource::<Layout>().tray_editing(),"actual Move command must still execute");
        assert!(app.world().resource::<Layout>().tray_preview.is_some());
    }


    fn choose_swipe_destination(app: &mut App, name: &str) {
        let registry=app.world().resource::<nvf_play::settings::Settings>();
        let row=registry.index("touch-bottom-swipe-target-v1").unwrap();
        let current=(registry.choices(row).unwrap().chosen)(app.world());
        if current != name { assert!(nvf_play::settings::pick(app.world_mut(),row,name)); }
        assert_eq!(app.world().resource::<drawer::Options>().destination.name(),name);
    }
    #[test]
    fn bottom_swipe_routes_one_real_destination_and_utility_capture_drops_raw_tail() {
        for target in ["Inventory","Utility tray"] {
            let (mut app,w)=mixed_hotbar_phone_scene(Vec2::new(640.,360.),"classic",4);
            app.world_mut().resource_mut::<drawer::Options>().enabled=true;
            choose_swipe_destination(&mut app,target);
            raw_key(&mut app,w,KeyCode::KeyW,ButtonState::Pressed);app.update();
            let start=drawer::handle(Vec2::new(640.,360.),24.).center();
            touch(&mut app,w,810,TouchPhase::Started,start.x,start.y);app.update();
            touch(&mut app,w,810,TouchPhase::Ended,start.x,start.y-120.);
            let gyro=app.world().resource::<Layout>().config.center(Target::Button(gyro::BUTTON),Vec2::new(640.,360.));
            touch(&mut app,w,811,TouchPhase::Started,gyro.x,gyro.y);
            touch(&mut app,w,811,TouchPhase::Ended,gyro.x,gyro.y);
            app.update();
            let inventory=app.world().resource::<Messages<nvf_play::menu::OpenInventory>>().len();
            assert_eq!(inventory,usize::from(target=="Inventory"),"single recognizer must not emit both destinations");
            assert_eq!(hotbar::tray_open_collision_fixture(app.world()),target=="Utility tray");
            assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyW),"swipe cannot erase hardware owner");
            assert!(app.world().resource::<TouchState>().contacts.is_empty());
            assert!(app.world().resource::<TouchState>().taps.is_empty());
            if target=="Utility tray" {
                assert!(app.world().resource::<TouchState>().ui_capture);
                assert!(!app.world().resource::<gyro::State>().active(app.world().resource::<gyro::Options>()),"raw gyro tail cannot coactivate with utility commit");
            }
        }
    }
    #[test]
    fn utility_swipe_cancels_on_target_inset_disable_and_mode_changes() {
        for scenario in 0..4 {
            let (mut app,w)=mixed_hotbar_phone_scene(Vec2::new(640.,360.),"classic",4);
            app.world_mut().resource_mut::<drawer::Options>().enabled=true;
            choose_swipe_destination(&mut app,"Utility tray");
            let start=drawer::handle(Vec2::new(640.,360.),24.).center();
            touch(&mut app,w,820,TouchPhase::Started,start.x,start.y);app.update();
            match scenario {
                0=>choose_swipe_destination(&mut app,"Inventory"),
                1=>app.world_mut().resource_mut::<drawer::SafeBottom>().0=48.,
                2=>app.world_mut().resource_mut::<drawer::Options>().enabled=false,
                _=>app.world_mut().resource_mut::<NextState<nvf_play::PlayState>>().set(nvf_play::PlayState::Menu),
            }
            touch(&mut app,w,820,TouchPhase::Ended,start.x,start.y-120.);app.update();
            assert_eq!(app.world().resource::<Messages<nvf_play::menu::OpenInventory>>().len(),0,"scenario{scenario}");
            assert!(!hotbar::tray_open_collision_fixture(app.world()),"scenario{scenario}: stale contact reopened tray");
        }
    }
    #[test]
    fn utility_swipe_is_disabled_with_hidden_tray_and_keeps_system_bottom_edge_free() {
        for scenario in 0..3 {
            let (mut app,w)=mixed_hotbar_phone_scene(Vec2::new(640.,360.),"classic",4);
            choose_swipe_destination(&mut app,"Utility tray");
            app.world_mut().resource_mut::<drawer::Options>().enabled=scenario!=0;
            app.world_mut().resource_mut::<drawer::SafeBottom>().0=48.;
            if scenario==1 {
                let row=app.world().resource::<nvf_play::settings::Settings>().index("touch-hotbar-tray").unwrap();
                assert_eq!(nvf_play::settings::flip(app.world_mut(),row),Some(false));
            }
            app.update();
            let start=if scenario==2 { Vec2::new(320.,358.) } else { drawer::handle(Vec2::new(640.,360.),48.).center() };
            touch(&mut app,w,830,TouchPhase::Started,start.x,start.y);app.update();
            touch(&mut app,w,830,TouchPhase::Ended,start.x,start.y-120.);app.update();
            assert_eq!(app.world().resource::<Messages<nvf_play::menu::OpenInventory>>().len(),0);
            assert!(!hotbar::tray_open_collision_fixture(app.world()),"disabled/hidden/system-edge scenario{scenario}");
        }
    }

    #[test]
    fn utility_handle_visibility_matches_platform_input_admission_and_fresh_return() {
        let (mut app,w)=mixed_hotbar_phone_scene(Vec2::new(640.,360.),"classic",4);
        app.init_resource::<nvf_play::pad::PlatformInputGate>();
        app.world_mut().resource_mut::<drawer::Options>().enabled=true;
        choose_swipe_destination(&mut app,"Utility tray");app.update();
        assert_eq!(drawer_visibility(&mut app),Visibility::Inherited);
        app.world_mut().resource_mut::<nvf_play::pad::PlatformInputGate>().set_blocked(true);
        app.update();assert_eq!(drawer_visibility(&mut app),Visibility::Hidden);
        let start=drawer::handle(Vec2::new(640.,360.),24.).center();
        touch(&mut app,w,840,TouchPhase::Started,start.x,start.y);
        touch(&mut app,w,840,TouchPhase::Ended,start.x,start.y-120.);app.update();
        assert!(!hotbar::tray_open_collision_fixture(app.world()));
        app.world_mut().resource_mut::<nvf_play::pad::PlatformInputGate>().set_blocked(false);
        app.update();assert_eq!(drawer_visibility(&mut app),Visibility::Inherited);
        touch(&mut app,w,841,TouchPhase::Started,start.x,start.y);app.update();
        touch(&mut app,w,841,TouchPhase::Ended,start.x,start.y-120.);app.update();
        assert!(hotbar::tray_open_collision_fixture(app.world()),"fresh unblocked gesture reaches the actual tray");
    }

    #[test]
    fn open_pins_cancellation_releases_hidden_capture_and_readmits_fresh_native_routes() {
        use nvf_play::{bindings::InputDevice, settings};
        for scenario in ["disabled", "controller", "focus", "modal", "platform"] {
            let size = Vec2::new(640., 360.);
            let (mut app, w) = mixed_hotbar_phone_scene(size, "classic", 4);
            app.add_plugins(nvf_play::pad::InputSwitchPlugin);
            app.world_mut().spawn(Gamepad::default());
            raw_key(&mut app, w, KeyCode::KeyW, ButtonState::Pressed);
            app.world_mut().write_message(MouseButtonInput {
                window: w, button: MouseButton::Middle, state: ButtonState::Pressed,
            });
            app.update();
            let pins = mixed_hotbar_tiles(&mut app).into_iter()
                .find(|(label, _)| label == "Pins").unwrap().1.center();
            touch(&mut app, w, 900, TouchPhase::Started, pins.x, pins.y);
            touch(&mut app, w, 900, TouchPhase::Ended, pins.x, pins.y);
            app.update(); app.update();
            assert!(hotbar::tray_open_collision_fixture(app.world()), "{scenario}: real Pins tap must open tray");
            assert!(app.world().resource::<TouchState>().ui_capture, "{scenario}: open tray owns gameplay input");
            match scenario {
                "disabled" => {
                    let row = app.world().resource::<settings::Settings>().index("touch-hotbar").unwrap();
                    assert_eq!(settings::flip(app.world_mut(), row), Some(false));
                }
                "controller" => nvf_play::bindings::set_device(app.world_mut(), InputDevice::Controller),
                "focus" => {
                    app.world_mut().get_mut::<Window>(w).unwrap().focused = false;
                    app.world_mut().write_message(WindowFocused { window: w, focused: false });
                    // Winit's focus-loss adapter emits this as well as WindowFocused;
                    // InputPlugin's keyboard system releases raw keys on this message.
                    app.world_mut().write_message(KeyboardFocusLost);
                }
                "modal" => {
                    app.init_resource::<nvf_play::terminal::touch::Settings>();
                    app.world_mut().resource_mut::<nvf_play::terminal::touch::Settings>().enabled = true;
                    app.insert_resource(nvf_play::terminal::TerminalScreen(Some(nvf_play::terminal::Session {
                        reference: fnv_sim::FormId(1), base: fnv_sim::FormId(2),
                        page: nvf_play::terminal::Page::LockedOut(["Local fixture".into(), "No game data".into()]),
                    })));
                }
                "platform" => {
                    app.init_resource::<nvf_play::pad::PlatformInputGate>();
                    app.world_mut().resource_mut::<nvf_play::pad::PlatformInputGate>().set_blocked(true);
                }
                _ => unreachable!(),
            }
            app.update();
            assert!(!hotbar::tray_open_collision_fixture(app.world()), "{scenario}: cancellation must close Pins");
            assert!(!app.world().resource::<TouchState>().ui_capture, "{scenario}: hidden hotbar cannot retain capture");
            let hardware_survives = scenario != "focus";
            assert_eq!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyW), hardware_survives,
                "{scenario}: preserve hardware except the existing focus-loss release contract");
            // Bevy's mouse input system consumes actual MouseButtonInput only;
            // focus loss cancels wrapper tracking without inventing a raw release.
            assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Middle));
            if scenario == "focus" {
                let state = app.world().resource::<TouchState>();
                assert!(state.physical_keys.is_empty() && state.physical_mouse.is_empty(),
                    "focus loss must clear wrapper ownership, independently of raw hardware state");
            }
            match scenario {
                "disabled" => {
                    let row = app.world().resource::<settings::Settings>().index("touch-hotbar").unwrap();
                    assert_eq!(settings::flip(app.world_mut(), row), Some(true));
                }
                "controller" => {
                    let jump = BUTTONS.iter().position(|b| b.control == Some(Control::Jump)).unwrap();
                    let point = app.world().resource::<Layout>().config.center(Target::Button(jump), size);
                    touch(&mut app, w, 901, TouchPhase::Started, point.x, point.y);
                    touch(&mut app, w, 901, TouchPhase::Ended, point.x, point.y);
                    app.update();
                    assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
                    assert!(!app.world().resource::<nvf_play::pad::TouchDeviceSwitch>().allows_actions());
                    assert!(!app.world().resource::<nvf_play::player::MoveIntent>().jump, "return touch only switches device");
                }
                "focus" => {
                    app.world_mut().get_mut::<Window>(w).unwrap().focused = true;
                    app.world_mut().write_message(WindowFocused { window: w, focused: true });
                }
                "modal" => app.world_mut().resource_mut::<nvf_play::terminal::TerminalScreen>().0 = None,
                "platform" => app.world_mut().resource_mut::<nvf_play::pad::PlatformInputGate>().set_blocked(false),
                _ => unreachable!(),
            }
            app.update(); app.update(); // Includes the dedicated modal's closing-frame drain.
            assert!(!app.world().resource::<TouchState>().ui_capture);
            assert!(!hotbar::tray_open_collision_fixture(app.world()), "{scenario}: return cannot resurrect Pins");
            if scenario == "focus" {
                raw_key(&mut app, w, KeyCode::KeyW, ButtonState::Pressed);
                app.world_mut().write_message(MouseButtonInput {
                    window: w, button: MouseButton::Middle, state: ButtonState::Pressed,
                });
                app.update();
            }
            let jump = BUTTONS.iter().position(|b| b.control == Some(Control::Jump)).unwrap();
            let point = app.world().resource::<Layout>().config.center(Target::Button(jump), size);
            touch(&mut app, w, 902, TouchPhase::Started, point.x, point.y); app.update();
            assert!(app.world().resource::<nvf_play::player::MoveIntent>().jump, "{scenario}: fresh native Jump admitted");
            touch(&mut app, w, 902, TouchPhase::Ended, point.x, point.y); app.update();
            let point = app.world().resource::<Layout>().config.center(Target::Button(gyro::BUTTON), size);
            touch(&mut app, w, 903, TouchPhase::Started, point.x, point.y); app.update();
            assert!(app.world().resource::<gyro::State>().active(app.world().resource::<gyro::Options>()),
                "{scenario}: fresh raw gyro route admitted");
            assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyW));
            assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Middle));
        }
    }

    #[test]
    fn queued_utility_capture_precedes_same_frame_fire_run_lock_vats_and_analog() {
        let size=Vec2::new(640.,360.);
        for action in 0..4 {
            let (mut app,w)=mixed_hotbar_phone_scene(size,"classic",4);
            app.init_resource::<VatsPresses>().add_systems(Update,count_vats);
            app.world_mut().resource_mut::<movement::Options>().vats_taps=true;
            app.world_mut().resource_mut::<nvf_play::control::PlayerControl>().weapon_out=true;
            app.update();
            let look=Vec2::new(600.,180.);
            if action==2 {
                touch(&mut app,w,851,TouchPhase::Started,look.x,look.y);
                touch(&mut app,w,851,TouchPhase::Ended,look.x,look.y);app.update();
                assert_eq!(app.world().resource::<VatsPresses>().0,0);
            }
            if action==3 {
                let stick=app.world().resource::<Layout>().config.center(Target::Stick,size);
                touch(&mut app,w,852,TouchPhase::Started,stick.x,stick.y);app.update();
                touch(&mut app,w,852,TouchPhase::Moved,stick.x,stick.y-40.);app.update();
                assert!(native_velocity(&app)[1]>0.,"fixture must move before capture");
            }
            raw_key(&mut app,w,KeyCode::F12,ButtonState::Pressed);
            app.world_mut().write_message(hotbar::UtilityTrayRequest(true));
            match action {
                0=>{
                    let i=BUTTONS.iter().position(|b|b.control==Some(Control::Use)).unwrap();
                    let p=app.world().resource::<Layout>().config.center(Target::Button(i),size);
                    touch(&mut app,w,853,TouchPhase::Started,p.x,p.y);
                }
                1=>{
                    let p=movement::fixed_lock_rect(app.world().resource::<Layout>(),size).center();
                    touch(&mut app,w,853,TouchPhase::Started,p.x,p.y);
                    touch(&mut app,w,853,TouchPhase::Ended,p.x,p.y);
                }
                2=>{
                    touch(&mut app,w,853,TouchPhase::Started,look.x,look.y);
                    touch(&mut app,w,853,TouchPhase::Ended,look.x,look.y);
                }
                _=>{
                    let p=app.world().resource::<Layout>().config.center(Target::Stick,size);
                    touch(&mut app,w,852,TouchPhase::Moved,p.x,p.y-48.);
                }
            }
            app.update();
            assert!(hotbar::tray_open_collision_fixture(app.world()),"case{action}: opening-batch contact dismissed queued tray");
            assert!(app.world().resource::<TouchState>().ui_capture,"case{action}: opening batch lost UI ownership");
            let control=app.world().resource::<nvf_play::control::PlayerControl>();
            assert!(!control.trigger && !control.auto_move,"case{action}: queued capture leaked a touch action");
            assert_eq!(app.world().resource::<VatsPresses>().0,0,"case{action}: VATS history leaked");
            assert_eq!(native_velocity(&app),[0.;2],"case{action}: analog sample leaked");
            let hardware=app.world().resource::<ButtonInput<KeyCode>>();
            assert!(hardware.pressed(KeyCode::F12) && hardware.just_pressed(KeyCode::F12),"real physical edge must remain");
        }
    }

    #[test]
    fn missing_window_cancels_queued_tray_request_without_replaying_it_on_return() {
        let (mut app, w) = mixed_hotbar_phone_scene(Vec2::new(640., 360.), "classic", 4);
        raw_key(&mut app, w, KeyCode::F12, ButtonState::Pressed);
        app.update();
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12));
        app.world_mut().write_message(hotbar::UtilityTrayRequest(true));
        app.world_mut().entity_mut(w).remove::<PrimaryWindow>();
        app.update();
        assert!(!hotbar::tray_open_collision_fixture(app.world()));
        assert!(!app.world().resource::<nvf_play::pipboy::touch_items::OverlayCapture>().0);
        app.world_mut().entity_mut(w).insert(PrimaryWindow);
        app.update();
        app.update();
        assert!(!hotbar::tray_open_collision_fixture(app.world()), "old request cannot replay after window restoration");
        assert!(!app.world().resource::<TouchState>().ui_capture);
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12));
        app.world_mut().write_message(hotbar::UtilityTrayRequest(true));
        app.update();
        assert!(hotbar::tray_open_collision_fixture(app.world()), "a fresh stable-frame request still opens the tray");
        assert!(app.world().resource::<TouchState>().ui_capture);
        assert!(app.world().resource::<nvf_play::pipboy::touch_items::OverlayCapture>().0);
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12));
    }


    #[derive(Resource, Default)]
    struct DefaultMenuTouchSeen { presses: usize, releases: usize, scrolls: usize, cancels: usize }
    fn observe_default_menu_touch(
        mut events: MessageReader<bevy::picking::pointer::PointerInput>,
        mut seen: ResMut<DefaultMenuTouchSeen>,
    ) {
        use bevy::picking::pointer::PointerAction;
        for event in events.read().filter(|e| e.pointer_id == nvf_play::pipboy::touch_items::POINTER) {
            match event.action {
                PointerAction::Press(_) => seen.presses += 1,
                PointerAction::Release(_) => seen.releases += 1,
                PointerAction::Scroll { y, .. } if y != 0. => seen.scrolls += 1,
                PointerAction::Cancel => seen.cancels += 1,
                _ => {}
            }
        }
    }
    fn default_menu_touch_scene(page: nvf_play::pipboy::Page, bar_enabled: bool) -> (App, Entity) {
        use nvf_play::pipboy::{self, touch_scroll::TouchPointerPlugin};
        use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
        use bevy::asset::{AssetApp, AssetPlugin};
        use bevy::camera::{ComputedCameraValues, RenderTargetInfo, visibility::VisibilityPlugin};
        use bevy::ui::{ComputedUiRenderTargetInfo, ui_layout_system, ui_surface::UiSurface, update::propagate_ui_target_cameras};
        let (mut app, window) = scene();
        app.insert_resource(nvf_play::settings::SettingsFile(None));
        app.add_plugins((AssetPlugin::default(), bevy::text::TextPlugin, bevy::transform::TransformPlugin))
            .init_asset::<Mesh>()
            .init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>()
            .add_plugins(VisibilityPlugin)
            .add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate))
            .add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate))
            .init_resource::<UiSurface>()
            .init_resource::<UiScale>()
            .add_systems(PostUpdate, (propagate_ui_target_cameras, ui_layout_system).chain())
            .configure_sets(PostUpdate, PropagateSet::<ComputedUiTargetCamera>::default().after(propagate_ui_target_cameras).before(ui_layout_system))
            .configure_sets(PostUpdate, PropagateSet::<ComputedUiRenderTargetInfo>::default().after(propagate_ui_target_cameras).before(ui_layout_system));
        // Authored empty player inventory, no installed game content or production test helper.
        let mut forms = fnv_sim::FormStore::default();
        let base = fnv_sim::forms::BaseForm {
            id: fnv_sim::FormId::PLAYER_BASE, sig: *b"NPC_", edid: None, name: None,
            script: None, prompt: None, voice_filter: false,
            data: fnv_sim::forms::BaseData::Actor(fnv_sim::forms::ActorBase { entries: Vec::new(), attributes: None }),
        };
        forms.sigs.insert(base.id, base.sig);
        forms.bases.insert(base.id, base);
        let forms = Box::leak(Box::new(forms));
        let refs = Box::leak(Box::new(fnv_sim::RefTable::default()));
        let mut sim = nvf_play::sim::SimState {
            forms, refs, world: fnv_sim::World::new(forms, refs, fnv_sim::SimConfig::default()),
        };
        let menu = nvf_play::menu::model::MenuModel::open_inventory(&mut sim.world).unwrap();
        app.insert_resource(sim).insert_resource(nvf_play::menu::ActiveMenu(Some(menu)));
        let mut pip = pipboy::PipBoy::default();
        pip.phase = pipboy::Phase::Up; pip.frame = pipboy::Frame::Screen; pip.page = page;
        app.insert_resource(pip).init_resource::<pipboy::PipBoyAid>()
            .init_resource::<pipboy::PointerMoved>().init_resource::<nvf_play::hotkeys::Hotkeys>()
            .init_resource::<DefaultMenuTouchSeen>();
        let camera = app.world_mut().spawn((Camera2d, Camera { computed: ComputedCameraValues { target_info: Some(RenderTargetInfo { physical_size: UVec2::splat(1000), scale_factor: 1.0 }), ..default() }, ..default() })).id();
        app.insert_resource(pipboy::PipBoyScreen {
            image: Handle::default(), camera, on_model: false, pointer: None, scale: 1.,
        });
        app.add_plugins(TouchPointerPlugin)
            .add_systems(Update, pipboy::flat_screen)
            .add_systems(Last, pipboy::flat_pointer)
            .add_systems(Update, observe_default_menu_touch);
        app.world_mut().resource_mut::<Layout>().config = layout::Config::default();
        let row = app.world().resource::<nvf_play::settings::Settings>().index("touch-hotbar").unwrap();
        assert!(!(app.world().resource::<nvf_play::settings::Settings>().list()[row].get)(app.world()),
            "basic menu test must start with actual default-OFF preference");
        if bar_enabled { assert_eq!(nvf_play::settings::flip(app.world_mut(), row), Some(true)); }
        app.world_mut().resource_mut::<NextState<nvf_play::PlayState>>().set(nvf_play::PlayState::Menu);
        for _ in 0..4 { app.update(); }
        (app, window)
    }
    fn default_menu_point(app: &mut App) -> Vec2 {
        let size = Vec2::splat(1000.);
        let tiles = mixed_hotbar_tiles(app);
        let config = &app.world().resource::<Layout>().config;
        for y in [0.35, 0.45, 0.55, 0.65] {
            for x in [0.35, 0.45, 0.55, 0.65] {
                let point = Vec2::new(x, y) * size;
                if config.hit(point, size, false, false).is_none()
                    && tiles.iter().all(|(_, r)| !r.contains(point)) { return point; }
            }
        }
        panic!("actual menu configuration must leave an unclaimed canvas point");
    }
    fn flush_default_menu_touch(app: &mut App) { for _ in 0..5 { app.update(); } }
    #[test]
    fn default_off_hotbar_preserves_raw_pipboy_taps_on_all_pages_and_existing_menu_controls() {
        use nvf_play::pipboy::{self, touch_items::TouchItems};
        for enabled in [false, true] {
            for page in pipboy::Page::ALL {
                let (mut app, w) = default_menu_touch_scene(page, enabled);
                assert_eq!(!mixed_hotbar_tiles(&mut app).is_empty(), enabled, "bar presentation remains opt-in");
                let point = default_menu_point(&mut app);
                touch(&mut app, w, 950, TouchPhase::Started, point.x, point.y);
                touch(&mut app, w, 950, TouchPhase::Ended, point.x, point.y);
                app.update();
                assert!(app.world().resource::<TouchItems>().owns(950), "raw wrapper reader must admit {page:?}, enabled={enabled}");
                assert!(app.world().resource::<TouchState>().contacts.is_empty(), "menu pointer must not become gameplay/look input");
                flush_default_menu_touch(&mut app);
                let seen = app.world().resource::<DefaultMenuTouchSeen>();
                assert_eq!((seen.presses, seen.releases, seen.scrolls), (1, 1, 0), "real pointer tap {page:?}, enabled={enabled}");
                assert!(app.world().resource::<nvf_play::hotkeys::Hotkeys>().0.iter().all(Option::is_none));
                assert_eq!(app.world().resource::<Messages<nvf_play::weapon_wheel::TouchHotkey>>().len(), 0);
                let i = BUTTONS.iter().position(|b| !b.playing && b.key == Some(KeyCode::Enter)).unwrap();
                let point = app.world().resource::<Layout>().config.center(Target::Button(i), Vec2::splat(1000.));
                touch(&mut app, w, 951, TouchPhase::Started, point.x, point.y); app.update();
                assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::Enter), "existing Select control still owns its hit target");
                assert!(!app.world().resource::<TouchItems>().owns(951));
            }
        }
    }
    #[test]
    fn default_off_hotbar_raw_vertical_swipes_reach_real_scroll_on_every_pipboy_page() {
        use nvf_play::pipboy;
        for page in pipboy::Page::ALL {
            let (mut app, w) = default_menu_touch_scene(page, false);
            // Data maps intentionally reject vertical wheel input. Exercise real
            // list tabs (Stats Skills / Data Notes), rather than map zoom.
            app.world_mut().resource_mut::<pipboy::PipBoy>().tabs[page.index()] = match page {
                pipboy::Page::Stats => 2,
                pipboy::Page::Items => 0,
                pipboy::Page::Data => 3,
            };
            app.update();
            let point = default_menu_point(&mut app);
            touch(&mut app, w, 960, TouchPhase::Started, point.x, point.y); app.update();
            touch(&mut app, w, 960, TouchPhase::Moved, point.x, point.y - 80.); app.update();
            touch(&mut app, w, 960, TouchPhase::Ended, point.x, point.y - 80.); app.update();
            flush_default_menu_touch(&mut app);
            let seen = app.world().resource::<DefaultMenuTouchSeen>();
            assert!(seen.scrolls > 0, "raw {page:?} swipe must reach real projected scroll with bar OFF");
            assert_eq!(seen.releases, 0, "scroll cannot become an item use/tab click");
            assert!(mixed_hotbar_tiles(&mut app).is_empty());
            assert!(app.world().resource::<nvf_play::hotkeys::Hotkeys>().0.iter().all(Option::is_none));
        }
    }
    #[test]
    fn default_off_menu_touch_session_changes_cancel_old_release_then_accept_a_fresh_contact() {
        use nvf_play::pipboy::{self, touch_items::TouchItems};
        for cause in 0..3 {
            let (mut app, w) = default_menu_touch_scene(pipboy::Page::Items, false);
            let point = default_menu_point(&mut app);
            touch(&mut app, w, 970, TouchPhase::Started, point.x, point.y); app.update(); app.update();
            assert_eq!(app.world().resource::<DefaultMenuTouchSeen>().presses, 1);
            match cause {
                0 => app.world_mut().resource_mut::<pipboy::PipBoy>().page = pipboy::Page::Stats,
                1 => app.world_mut().resource_mut::<pipboy::PipBoy>().tabs[pipboy::Page::Items.index()] = 1,
                _ => app.world_mut().resource_mut::<pipboy::PipBoy>().frame = pipboy::Frame::Device,
            }
            touch(&mut app, w, 970, TouchPhase::Ended, point.x, point.y); app.update();
            flush_default_menu_touch(&mut app);
            assert!(!app.world().resource::<TouchItems>().owns(970));
            let seen = app.world().resource::<DefaultMenuTouchSeen>();
            assert_eq!(seen.releases, 0, "old release must not cross session cause{cause}");
            assert!(seen.cancels > 0);
            // Restore the flat presentation only after the stale contact has been drained.
            app.world_mut().resource_mut::<pipboy::PipBoy>().frame = pipboy::Frame::Screen;
            app.update();
            touch(&mut app, w, 971, TouchPhase::Started, point.x, point.y);
            touch(&mut app, w, 971, TouchPhase::Ended, point.x, point.y); app.update();
            flush_default_menu_touch(&mut app);
            assert_eq!(app.world().resource::<DefaultMenuTouchSeen>().releases, 1, "fresh session cause{cause}");
        }
    }


    #[test]
    fn drawer_stays_hidden_through_real_practice_close_drain_during_return_to_playing() {
        use nvf_play::{pause::{Page, Paused}, settings};
        let (mut app, _) = controls_scene();
        app.world_mut().resource_mut::<drawer::Options>().enabled = true;
        app.update();
        assert_eq!(drawer_visibility(&mut app), Visibility::Inherited);
        app.insert_resource(Paused(true)).insert_resource(Page::Settings { selected: None });
        app.world_mut().resource_mut::<NextState<nvf_play::PlayState>>().set(nvf_play::PlayState::Menu);
        app.update();
        let row = app.world().resource::<settings::Settings>().index("touch-practice-v1").unwrap();
        assert!(settings::pick(app.world_mut(), row, "Open practice"));
        app.update();
        assert!(app.world().resource::<practice::Practice>().blocking());
        assert_eq!(drawer_visibility(&mut app), Visibility::Hidden);
        app.world_mut().resource_mut::<Paused>().0 = false;
        app.world_mut().resource_mut::<NextState<nvf_play::PlayState>>().set(nvf_play::PlayState::Playing);
        app.update();
        assert!(app.world().resource::<TouchState>().playing);
        assert!(app.world().resource::<practice::Practice>().blocking(), "practice close owns its drain frames");
        assert_eq!(drawer_visibility(&mut app), Visibility::Hidden, "returning gameplay must not paint a handle through the closing lesson");
        app.update();
        assert!(app.world().resource::<practice::Practice>().blocking());
        assert_eq!(drawer_visibility(&mut app), Visibility::Hidden);
        app.update(); app.update();
        assert!(!app.world().resource::<practice::Practice>().blocking());
        assert_eq!(drawer_visibility(&mut app), Visibility::Inherited, "fresh gameplay restores the visible handle");
    }


    #[test]
    fn utility_request_quarantines_opening_batch_and_old_release_but_fresh_outside_tap_closes() {
        let size=Vec2::new(640.,360.);
        let (mut app,w)=mixed_hotbar_phone_scene(size,"classic",4);
        let slot=hotbar::test_slot_center(app.world(),0);
        let mut hotkeys=bevy::ecs::message::MessageCursor::<nvf_play::weapon_wheel::TouchHotkey>::default();
        touch(&mut app,w,860,TouchPhase::Started,slot.x,slot.y);app.update();
        raw_key(&mut app,w,KeyCode::F12,ButtonState::Pressed);
        app.world_mut().write_message(hotbar::UtilityTrayRequest(true));
        touch(&mut app,w,860,TouchPhase::Ended,slot.x,slot.y);
        touch(&mut app,w,861,TouchPhase::Started,20.,20.); // Opening-batch outside contact.
        app.update();
        assert!(hotbar::tray_open_collision_fixture(app.world()),"opening events must not dismiss the new tray");
        assert!(app.world().resource::<TouchState>().ui_capture);
        assert!(hotkeys.read(app.world().resource::<Messages<nvf_play::weapon_wheel::TouchHotkey>>()).next().is_none(),"old held slot release cannot use an item after request admission");
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12));
        assert!(app.world().resource::<Layout>().editor.is_none());
        touch(&mut app,w,861,TouchPhase::Ended,20.,20.);app.update();
        assert!(hotbar::tray_open_collision_fixture(app.world()),"release of the quarantined outside contact cannot close next frame");
        touch(&mut app,w,862,TouchPhase::Started,20.,20.);
        touch(&mut app,w,862,TouchPhase::Ended,20.,20.);app.update();
        assert!(!hotbar::tray_open_collision_fixture(app.world()),"a fresh later outside tap must still dismiss");
        assert!(app.world().resource::<TouchState>().ui_capture,"closing batch still owns its tail");
        assert!(app.world().resource::<Layout>().editor.is_none(),"outside dismissal cannot also open the gear control");
        app.update();app.update();
        assert!(!app.world().resource::<TouchState>().ui_capture);
        touch(&mut app,w,863,TouchPhase::Started,slot.x,slot.y);
        touch(&mut app,w,863,TouchPhase::Ended,slot.x,slot.y);app.update();
        assert_eq!(hotkeys.read(app.world().resource::<Messages<nvf_play::weapon_wheel::TouchHotkey>>()).map(|m|m.0).collect::<Vec<_>>(),[0],"a new deliberate slot tap works after closing drain");
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12));
    }

    #[test]
    fn opening_request_retires_actual_move_and_ignored_release_with_anchor_rollback() {
        let (mut app,w)=mixed_hotbar_phone_scene(Vec2::new(640.,360.),"classic",4);
        let anchor=hotbar::test_anchor(app.world());
        let slot=hotbar::test_slot_center(app.world(),0);
        touch(&mut app,w,870,TouchPhase::Started,slot.x,slot.y);app.update();
        touch(&mut app,w,871,TouchPhase::Started,slot.x,slot.y);app.update(); // Ignored second owner.
        for _ in 0..14 {app.update();}
        touch(&mut app,w,870,TouchPhase::Ended,slot.x,slot.y);app.update();
        let button={
            let world=app.world_mut();let mut query=world.query::<(&Text,&ChildOf)>();
            let parent=query.iter(world).find(|(t,_)|t.0=="Move bar").unwrap().1.parent();
            let n=world.get::<Node>(parent).unwrap();
            let (Val::Px(x),Val::Px(y),Val::Px(width),Val::Px(height))=(n.left,n.top,n.width,n.height) else {panic!("actual context button must retain pixel bounds")};
            Vec2::new(x+width*0.5,y+height*0.5)
        };
        touch(&mut app,w,872,TouchPhase::Started,button.x,button.y);
        touch(&mut app,w,872,TouchPhase::Ended,button.x,button.y);app.update();
        app.update();
        assert!(app.world().resource::<Layout>().tray_editing(),"actual context Move command must execute");
        fn painted_slot(app:&mut App)->Vec2 {
            mixed_hotbar_tiles(app).into_iter().find(|(label,_)|label.starts_with("1 ")).unwrap().1.center()
        }
        let original_rect_center=painted_slot(&mut app);
        let slot=hotbar::test_slot_center(app.world(),0);
        touch(&mut app,w,873,TouchPhase::Started,slot.x,slot.y);app.update();
        let mut moved=slot;
        for candidate in [Vec2::new(500.,150.),Vec2::new(200.,80.),Vec2::new(400.,100.),Vec2::new(250.,220.)] {
            moved=candidate;
            touch(&mut app,w,873,TouchPhase::Moved,moved.x,moved.y);app.update();app.update();
            if painted_slot(&mut app)!=original_rect_center {break;}
        }
        assert_ne!(painted_slot(&mut app),original_rect_center,"fixture must visibly relocate the responsive bar before canceling");
        assert_ne!(hotbar::test_anchor(app.world()),anchor,"fixture must change the live drag anchor");
        assert!(!hotbar::test_ui_owners_retired(app.world()));
        // Moving now belongs to the same layout transaction as resize. Explicit
        // Cancel restores the preview before a separate utility-opening request.
        let cancel=tray_editor::action_rect(3,Vec2::new(640.,360.)).center();
        touch(&mut app,w,876,TouchPhase::Started,cancel.x,cancel.y);app.update();
        assert!(app.world().resource::<Layout>().editor.is_none());
        app.world_mut().write_message(hotbar::UtilityTrayRequest(true));
        touch(&mut app,w,873,TouchPhase::Ended,moved.x,moved.y);
        touch(&mut app,w,871,TouchPhase::Ended,slot.x,slot.y);app.update();
        assert!(hotbar::tray_open_collision_fixture(app.world()));
        assert!(hotbar::test_ui_owners_retired(app.world()),"opening quarantine must retire move, context and ignored owners whose ends it consumed");
        assert_eq!(hotbar::test_anchor(app.world()),anchor,"canceled drag restores original anchor instead of persisting it");
        assert_eq!(painted_slot(&mut app),original_rect_center,"opening frame must already draw the restored placement, not the dragged cache");
        assert_eq!(hotbar::test_slot_center(app.world(),0),original_rect_center,"same-frame admission geometry follows restored visible rectangle");
        touch(&mut app,w,874,TouchPhase::Started,20.,20.);
        touch(&mut app,w,874,TouchPhase::Ended,20.,20.);app.update();
        assert!(!hotbar::tray_open_collision_fixture(app.world()));
        app.update();app.update();
        assert!(!app.world().resource::<TouchState>().ui_capture,"no hidden moving owner may keep capture alive after fresh dismissal");
        let slot=hotbar::test_slot_center(app.world(),0);
        let mut reader=bevy::ecs::message::MessageCursor::<nvf_play::weapon_wheel::TouchHotkey>::default();
        assert!(reader.read(app.world().resource::<Messages<nvf_play::weapon_wheel::TouchHotkey>>()).next().is_none());
        touch(&mut app,w,875,TouchPhase::Started,slot.x,slot.y);
        touch(&mut app,w,875,TouchPhase::Ended,slot.x,slot.y);app.update();
        assert_eq!(reader.read(app.world().resource::<Messages<nvf_play::weapon_wheel::TouchHotkey>>()).map(|m|m.0).collect::<Vec<_>>(),[0]);
    }

    #[test]
    fn queued_tray_requests_crossing_resize_or_disabled_bar_do_not_replay_but_fresh_ones_open() {
        for cause in ["resize","disabled"] {
            let (mut app,w)=mixed_hotbar_phone_scene(Vec2::new(640.,360.),"classic",4);
            raw_key(&mut app,w,KeyCode::F12,ButtonState::Pressed);app.update();
            app.world_mut().write_message(hotbar::UtilityTrayRequest(true));
            if cause=="resize" {app.world_mut().get_mut::<Window>(w).unwrap().resolution.set_physical_resolution(672,384);}
            else {
                let row=app.world().resource::<nvf_play::settings::Settings>().index("touch-hotbar").unwrap();
                assert_eq!(nvf_play::settings::flip(app.world_mut(),row),Some(false));
            }
            app.update();
            assert!(!hotbar::tray_open_collision_fixture(app.world()),"{cause}: old request must not open");
            assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12));
            if cause=="disabled" {
                let row=app.world().resource::<nvf_play::settings::Settings>().index("touch-hotbar").unwrap();
                assert_eq!(nvf_play::settings::flip(app.world_mut(),row),Some(true));
            }
            app.update();
            assert!(!hotbar::tray_open_collision_fixture(app.world()),"{cause}: old request cannot replay after return");
            app.world_mut().write_message(hotbar::UtilityTrayRequest(true));app.update();
            assert!(hotbar::tray_open_collision_fixture(app.world()),"{cause}: a fresh stable request opens");
            assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::F12));
        }
    }
    #[test]
    fn hide_toggle_hides_other_touch_controls_passes_touches_through_and_restores() {
        let (mut app, window) = controls_scene();
        app.world_mut().resource_mut::<nvf_play::control::PlayerControl>().weapon_out = true;
        let toggle = hide_toggle_rect(Vec2::splat(1000.0), Default::default()).center();
        let fire_idx = BUTTONS.iter().position(|b| b.control == Some(Control::Use)).unwrap();
        let fire = app.world().resource::<Layout>().config.center(Target::Button(fire_idx), Vec2::splat(1000.0));
        // Before hiding: tapping Fire activates trigger.
        touch(&mut app, window, 901, TouchPhase::Started, fire.x, fire.y);
        app.update();
        assert!(app.world().resource::<nvf_play::control::PlayerControl>().trigger);
        touch(&mut app, window, 901, TouchPhase::Ended, fire.x, fire.y);
        app.update();
        assert!(!app.world().resource::<nvf_play::control::PlayerControl>().trigger);

        // Tap compact Hide toggle button: hides all other controls and leaves toggle visible.
        touch(&mut app, window, 902, TouchPhase::Started, toggle.x, toggle.y);
        touch(&mut app, window, 902, TouchPhase::Ended, toggle.x, toggle.y);
        app.update();
        assert!(app.world().resource::<TouchState>().controls_hidden);
        {
            let world = app.world_mut();
            let mut q = world.query::<(&TouchVisual, &Visibility)>();
            let mut saw_toggle = false;
            for (v, vis) in q.iter(world) {
                match v {
                    TouchVisual::HideToggle | TouchVisual::HideToggleLabel => {
                        assert_eq!(*vis, Visibility::Inherited, "restore toggle must remain visible");
                        saw_toggle = true;
                    }
                    TouchVisual::Button(_) | TouchVisual::Stick | TouchVisual::Knob | TouchVisual::Instructions => {
                        assert_eq!(*vis, Visibility::Hidden, "other touch controls must be hidden");
                    }
                    _ => {}
                }
            }
            assert!(saw_toggle);
        }

        // While hidden, touching over Fire does NOT fire or claim TouchUiCapture.
        touch(&mut app, window, 903, TouchPhase::Started, fire.x, fire.y);
        app.update();
        assert!(!app.world().resource::<nvf_play::control::PlayerControl>().trigger, "hidden buttons must not fire");
        assert!(app.world().resource::<nvf_play::ui_pointer::TouchUiCapture>().allows(903), "touches must pass through freely while hidden");
        touch(&mut app, window, 903, TouchPhase::Ended, fire.x, fire.y);
        app.update();

        // Tap toggle again: restores all controls.
        touch(&mut app, window, 904, TouchPhase::Started, toggle.x, toggle.y);
        touch(&mut app, window, 904, TouchPhase::Ended, toggle.x, toggle.y);
        app.update();
        assert!(!app.world().resource::<TouchState>().controls_hidden);
        touch(&mut app, window, 905, TouchPhase::Started, fire.x, fire.y);
        app.update();
        assert!(app.world().resource::<nvf_play::control::PlayerControl>().trigger, "restored Fire button works again");
        touch(&mut app, window, 905, TouchPhase::Ended, fire.x, fire.y);
        app.update();
    }

    #[test]
    fn default_touch_hints_and_captions_auto_fade_after_15_seconds_and_turn_off() {
        let mut fade = HintFade::default();
        let mut hints = true;
        // Full visibility during the first 15 seconds of gameplay.
        fade.tick(10.0, true, &mut hints);
        assert!(hints);
        assert_eq!(fade.alpha(hints, false), 1.0);
        // Paused / non-gameplay does not advance the timer.
        fade.tick(20.0, false, &mut hints);
        assert!(hints);
        assert_eq!(fade.alpha(hints, false), 1.0);
        fade.tick(5.0, true, &mut hints);
        assert!(hints);
        assert_eq!(fade.alpha(hints, false), 1.0);
        // Smooth fade between 15.0 s and 16.5 s.
        fade.tick(0.75, true, &mut hints);
        assert!(hints);
        let mid = fade.alpha(hints, false);
        assert!(mid > 0.4 && mid < 0.6, "expected midpoint fade alpha ~0.5, got {mid}");
        // Turns off automatically once fade finishes.
        fade.tick(1.0, true, &mut hints);
        assert!(!hints, "hints must turn off automatically after fade");
        assert_eq!(fade.alpha(hints, false), 0.0);
        // Layout editor still keeps captions at full opacity.
        assert_eq!(fade.alpha(hints, true), 1.0);
        // If user manually turns hints back on, auto-fade respects manual override.
        hints = true;
        fade.tick(30.0, true, &mut hints);
        assert!(hints);
        assert_eq!(fade.alpha(hints, false), 1.0);
    }

    #[test]
    fn android_platform_defaults_match_live_device_spec_and_preserve_desktop() {
        let gyro_android = gyro::Options::for_platform(true);
        let gyro_desktop = gyro::Options::for_platform(false);
        assert!(gyro_android.enabled && gyro_android.invert_x && gyro_android.invert_y);
        assert!(!gyro_desktop.enabled && !gyro_desktop.invert_x && !gyro_desktop.invert_y);

        let pip_android = pipboy_options::Options::for_platform(true);
        let pip_desktop = pipboy_options::Options::for_platform(false);
        assert!(pip_android.left && pip_android.right && !pip_android.graphics);
        assert!(!pip_desktop.left && !pip_desktop.right && pip_desktop.graphics);

        let drawer_android = drawer::Options::for_platform(true);
        let drawer_desktop = drawer::Options::for_platform(false);
        assert!(drawer_android.enabled && !drawer_desktop.enabled);
    }

    #[test]
    fn pause_touch_button_stays_visible_while_paused_and_toggles_pause_closed() {
        assert!(button_active(PAUSE_BUTTON, false, true));
        assert!(!button_active(TAB_BUTTON, false, true));
        assert!(button_active(TAB_BUTTON, false, false), "Tab stays active in non-pause menus");

        let (mut app, window) = scene();
        app.insert_resource(nvf_play::pause::Paused(true));
        app.insert_resource(nvf_play::pause::Page::Main);
        app.insert_resource(nvf_play::pause::PauseView::default());
        app.insert_resource(State::new(nvf_play::PlayState::Menu));
        app.init_resource::<NextState<nvf_play::PlayState>>();
        app.update();
        let pause_pos = app.world().resource::<Layout>().config.center(Target::Button(PAUSE_BUTTON), Vec2::splat(1000.0));
        touch(&mut app, window, 950, TouchPhase::Started, pause_pos.x, pause_pos.y);
        app.update();
        touch(&mut app, window, 950, TouchPhase::Ended, pause_pos.x, pause_pos.y);
        app.update();
        assert!(!app.world().resource::<nvf_play::pause::Paused>().0, "tapping Pause button while paused must clear Paused");
        assert!(
            matches!(
                *app.world().resource::<NextState<nvf_play::PlayState>>(),
                NextState::Pending(nvf_play::PlayState::Playing)
            ) || *app.world().resource::<State<nvf_play::PlayState>>().get() == nvf_play::PlayState::Playing,
            "tapping Pause button while paused must request or complete transition back to Playing"
        );
    }

    #[test]
    fn settings_overlay_virtual_controls_emit_navigation_keys() {
        let mut state = TouchState::default();
        let down = BUTTONS.iter().position(|b| b.key == Some(KeyCode::ArrowDown)).unwrap();
        let ok = BUTTONS.iter().position(|b| b.key == Some(KeyCode::Enter)).unwrap();
        state.contacts.insert(1, Contact {
            origin: Vec2::ZERO, position: Vec2::ZERO, role: Role::SettingsOverlay(down), started: 0.0, dragged: false,
        });
        state.contacts.insert(2, Contact {
            origin: Vec2::ZERO, position: Vec2::ZERO, role: Role::SettingsOverlay(ok), started: 0.0, dragged: false,
        });
        state.contacts.insert(3, Contact {
            origin: Vec2::ZERO, position: Vec2::ZERO, role: Role::SettingsOverlay(PAUSE_BUTTON), started: 0.0, dragged: false,
        });
        sync_touch(&mut state, None, false);
        assert!(state.keys.contains(&KeyCode::ArrowDown));
        assert!(state.keys.contains(&KeyCode::Enter));
        assert!(!state.keys.contains(&KeyCode::Escape), "Pause button closes pause on release rather than spamming Escape");
        state.contacts.clear();
        sync_touch(&mut state, None, false);
        assert!(state.keys.is_empty());
    }

    include!("touch/menu_sheet_router_tests.rs");
    include!("touch/entrypoint_tests.rs");
    include!("touch/optional_ui_tests.rs");
    include!("touch/optional_ui_router_tests.rs");
    include!("touch/pipboy_optional_parent_tests.rs");
    include!("touch/pipboy_diagonal_parent_tests.rs");
    include!("touch/pipboy_display_parent_tests.rs");
}

#[cfg(test)]
#[path="touch/world_outcome_tests.rs"]
mod world_outcome_tests;
