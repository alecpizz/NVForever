//! Controller-only aiming preferences and cancelable gesture ownership.
use crate::pad::PadInput;
use crate::{
    bindings::{Control, ControlInput, InputDevice},
    control::HeldSneakInput,
    settings::{self, Setting, Slider},
    PlayState,
};
use bevy::{
    input::gamepad::Gamepad,
    prelude::*,
    window::{AppLifecycle, PrimaryWindow, WindowFocused},
};
use std::collections::HashSet;

pub const KEYS: [&str; 4] = [
    "controller-ads-v1",
    "controller-dead-zone-v1",
    "controller-toggle-aim-v1",
    "controller-hold-sneak-v1",
];

#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct Options {
    pub ads: f32,
    pub dead_zone: f32,
    pub toggle_aim: bool,
    pub hold_sneak: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            ads: 1.0,
            dead_zone: crate::pad::STICK_DEAD,
            toggle_aim: false,
            hold_sneak: false,
        }
    }
}
impl Options {
    pub fn look_scale(&self, aimed: bool) -> f32 {
        if aimed {
            self.ads
        } else {
            1.0
        }
    }
}

type Stamp = (InputDevice, Option<PadInput>, Option<PadInput>, Options);
#[derive(Resource, Default)]
pub struct Gestures {
    pub(crate) aim_requested: bool,
    latched: bool,
    raw_aim: bool,
    wait_aim: bool,
    wait_sneak: bool,
    stamp: Option<Stamp>,
    pads: HashSet<Entity>,
    suspended: bool,
    focused: Option<bool>,
}
impl Gestures {
    fn cancel(&mut self) {
        self.aim_requested = false;
        self.latched = false;
        self.wait_aim = true;
        self.wait_sneak = true;
    }
}
/// Backends and rebinding share this cancellation path; no hardware state is changed.
pub(crate) fn cancel(world: &mut World) {
    if let Some(mut gestures) = world.get_resource_mut::<Gestures>() {
        gestures.cancel();
    }
    if let Some(mut held) = world.get_resource_mut::<HeldSneakInput>() {
        held.controller_active = false;
    }
}

pub struct ControllerAimPlugin;
impl Plugin for ControllerAimPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Options>()
            .init_resource::<Gestures>()
            .init_resource::<HeldSneakInput>()
            .add_message::<AppLifecycle>()
            .add_message::<WindowFocused>()
            .add_systems(
                PreUpdate,
                route
                    .after(crate::pad::InputSwitchSystems::Switch)
                    .after(crate::pad::menu_keys),
            );
        for (i, label, note, slider) in [
            (
                0,
                "Controller ADS sensitivity",
                "Multiply Stick Sensitivity while the weapon actually uses its sights.",
                Slider {
                    get: |w| w.resource::<Options>().ads / 2.0,
                    set: |w, v| {
                        if v.is_finite() {
                            w.resource_mut::<Options>().ads = v.clamp(0.05, 1.0) * 2.0;
                        }
                    },
                    min: 0.05,
                    step: 0.05,
                    shown: Some(|v| format!("{:.2}x", v * 2.0)),
                },
            ),
            (
                1,
                "Controller stick dead zone",
                "Ignore small movement of either stick. Hardware filtering still applies.",
                Slider {
                    get: |w| w.resource::<Options>().dead_zone / 0.4,
                    set: |w, v| {
                        if v.is_finite() {
                            w.resource_mut::<Options>().dead_zone =
                                (v.clamp(0.0, 1.0) * 40.0).round() / 100.0;
                        }
                    },
                    min: 0.0,
                    step: 0.05,
                    shown: Some(|v| format!("{:.0}%", v * 40.0)),
                },
            ),
        ] {
            settings::add_setting(
                app,
                Setting {
                    key: KEYS[i],
                    group: "Controller",
                    label,
                    note,
                    switches: "",
                    get: |_| false,
                    set: |_, _| {},
                    slider: Some(slider),
                },
            );
        }
        settings::add_setting(app,Setting {key:KEYS[2],group:"Controller",label:"Controller aim: toggle",
            note:"Off: hold the aim/block control. On: press once to aim or block, press again to release. Menus and input handoffs cancel it.",
            switches: "",
            get:|w|w.resource::<Options>().toggle_aim,set:|w,v|w.resource_mut::<Options>().toggle_aim=v,slider:None});
        settings::add_setting(app,Setting {key:KEYS[3],group:"Controller",label:"Controller sneak: hold",
            note:"Off: press to toggle sneak. On: sneak while held, then restore the independent keyboard/script sneak state.",
            switches: "",
            get:|w|w.resource::<Options>().hold_sneak,set:|w,v|w.resource_mut::<Options>().hold_sneak=v,slider:None});
    }
}

fn physical_down(pads: &Query<(Entity, &Gamepad)>, binding: Option<PadInput>) -> bool {
    match binding {
        Some(PadInput::Button(button)) => pads.iter().any(|(_, pad)| pad.pressed(button)),
        Some(PadInput::Stick(direction)) => pads
            .iter()
            .any(|(_, pad)| direction.amount(pad.left_stick()) >= crate::pad::STICK_PRESS),
        None => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn route(
    input: ControlInput,
    options: Res<Options>,
    mut gestures: ResMut<Gestures>,
    mut held: ResMut<HeldSneakInput>,
    state: Option<Res<State<PlayState>>>,
    next: Option<Res<NextState<PlayState>>>,
    locks: Option<Res<crate::control::ControlLocks>>,
    pads: Query<(Entity, &Gamepad)>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    mut lifecycle: MessageReader<AppLifecycle>,
    mut focus: MessageReader<WindowFocused>,
) {
    held.controller_active = false;
    let mut interrupted = false;
    for e in lifecycle.read() {
        match e {
            AppLifecycle::Suspended => {
                gestures.suspended = true;
                interrupted = true;
            }
            AppLifecycle::Running => {
                gestures.suspended = false;
                interrupted = true;
            }
            _ => {}
        }
    }
    for e in focus.read() {
        if windows.iter().any(|(id, _)| id == e.window) {
            gestures.focused = Some(e.focused);
            interrupted = true;
        }
    }
    let playing = match next.as_deref() {
        Some(NextState::Pending(s) | NextState::PendingIfDifferent(s)) => *s == PlayState::Playing,
        _ => state
            .as_deref()
            .is_some_and(|s| *s.get() == PlayState::Playing),
    };
    let now: HashSet<_> = pads.iter().map(|(e, _)| e).collect();
    let stamp = (
        input.bindings.device,
        input.bindings.pad(Control::Block),
        input.bindings.pad(Control::CrouchSneak),
        *options,
    );
    let changed = gestures.stamp != Some(stamp) || gestures.pads != now;
    gestures.stamp = Some(stamp);
    gestures.pads = now;
    let raw_aim = physical_down(&pads, input.bindings.pad(Control::Block));
    let raw_sneak = physical_down(&pads, input.bindings.pad(Control::CrouchSneak));
    let allowed = playing
        && input.controller()
        && !input.suppressed()
        && !gestures.suspended
        && gestures.focused != Some(false)
        && !gestures.pads.is_empty()
        && windows.iter().all(|(_, w)| w.focused);
    if !allowed || changed || interrupted {
        gestures.cancel();
    }
    // Only genuine neutral clears quarantine, including controls held across a modal.
    if !raw_aim {
        gestures.wait_aim = false;
    }
    if !raw_sneak {
        gestures.wait_sneak = false;
    }
    if allowed && !changed && !interrupted {
        let aim_allowed =
            !input.held(Control::Block) && !locks.as_deref().is_some_and(|l| l.fighting);
        let sneak_allowed =
            !input.held(Control::CrouchSneak) && !locks.as_deref().is_some_and(|l| l.sneaking);
        if !aim_allowed {
            gestures.latched = false;
            gestures.wait_aim = true;
        }
        if options.toggle_aim
            && aim_allowed
            && !gestures.wait_aim
            && !gestures.raw_aim
            && input.pad_just_pressed(Control::Block)
        {
            gestures.latched = !gestures.latched;
        }
        gestures.aim_requested = aim_allowed
            && !gestures.wait_aim
            && if options.toggle_aim {
                gestures.latched
            } else {
                input.pad_pressed(Control::Block)
            };
        if !sneak_allowed {
            gestures.wait_sneak = true;
        }
        held.controller_active = options.hold_sneak
            && sneak_allowed
            && !gestures.wait_sneak
            && input.pad_pressed(Control::CrouchSneak);
    }
    gestures.raw_aim = raw_aim;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        bindings::Bindings,
        control::{ControlLocks, PlayerControl},
        player::Player,
    };
    use bevy::{
        input::{
            gamepad::{RawGamepadAxisChangedEvent, RawGamepadButtonChangedEvent, RawGamepadEvent},
            InputPlugin,
        },
        window::{CursorGrabMode, CursorOptions},
    };
    include!("controller_aim/input_union_tests.rs");
    #[derive(Resource, Default)]
    struct Sticks(Vec2, Vec2);
    fn observe(pads: crate::pad::ActionPads, mut seen: ResMut<Sticks>) {
        seen.0 = crate::pad::stick(&pads, true);
        seen.1 = crate::pad::stick(&pads, false);
    }
    fn scene() -> (App, Entity, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, InputPlugin, bevy::state::app::StatesPlugin))
            .insert_state(PlayState::Playing)
            .init_resource::<Bindings>()
            .init_resource::<PlayerControl>()
            .init_resource::<ControlLocks>()
            .init_resource::<crate::furniture::PlayerSeat>()
            .init_resource::<Sticks>()
            .init_resource::<crate::pad::StickButtons>()
            .init_resource::<crate::pad::MenuKeys>()
            .insert_resource(crate::pause::Paused(false))
            .insert_resource(settings::SettingsFile(None))
            .init_resource::<crate::aim_zoom::AimZoom>()
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                std::time::Duration::from_millis(50),
            ))
            .insert_resource(Player {
                heading: 1.0,
                pitch: 0.0,
                eye_height: 112.0,
                feet: [0.0; 3],
                body_eye: None,
                body_view: None,
                placed: None,
            })
            .add_plugins((crate::pad::InputSwitchPlugin, ControllerAimPlugin))
            .add_systems(
                PreUpdate,
                (crate::pad::update_stick_buttons, crate::pad::menu_keys)
                    .chain()
                    .after(crate::pad::InputSwitchSystems::Switch),
            )
            .add_systems(
                Update,
                (
                    crate::control::read_controls,
                    crate::player::mouse_look,
                    observe,
                )
                    .chain(),
            );
        app.world_mut().resource_mut::<Bindings>().device = InputDevice::Controller;
        let pad = app.world_mut().spawn(Gamepad::default()).id();
        let w = app
            .world_mut()
            .spawn((
                Window::default(),
                PrimaryWindow,
                CursorOptions {
                    grab_mode: CursorGrabMode::Confined,
                    ..default()
                },
            ))
            .id();
        app.update();
        app.update();
        (app, pad, w)
    }
    fn raw_button(app: &mut App, pad: Entity, control: Control, down: bool) {
        let Some(PadInput::Button(button)) = app.world().resource::<Bindings>().pad(control) else {
            panic!()
        };
        app.world_mut()
            .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                pad,
                button,
                if down { 1.0 } else { 0.0 },
            )));
    }
    fn button(app: &mut App, pad: Entity, control: Control, down: bool) {
        raw_button(app, pad, control, down);
        app.update();
    }
    fn axis(app: &mut App, pad: Entity, axis: GamepadAxis, value: f32) {
        app.world_mut()
            .write_message(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(
                pad, axis, value,
            )));
    }
    fn options(app: &mut App, toggle: bool, hold: bool) {
        {
            let mut o = app.world_mut().resource_mut::<Options>();
            o.toggle_aim = toggle;
            o.hold_sneak = hold;
        }
        app.update();
    }
    fn aiming(app: &App) -> bool {
        app.world().resource::<PlayerControl>().aiming
    }
    fn sneak(app: &App) -> bool {
        app.world().resource::<PlayerControl>().sneak
    }
    #[test]
    fn actual_sights_scale_the_live_right_stick_without_treating_a_held_aim_key_as_ads() {
        let (mut app, pad, _) = scene();
        app.world_mut().resource_mut::<Options>().ads = 0.5;
        app.update();
        axis(&mut app, pad, GamepadAxis::RightStickX, 0.6);
        raw_button(&mut app, pad, Control::Block, true);
        let before = app.world().resource::<Player>().heading;
        app.update();
        let normal = app.world().resource::<Player>().heading - before;
        assert!(normal > 0.0 && aiming(&app));
        assert!(!app.world().resource::<crate::aim_zoom::AimZoom>().aimed);
        app.world_mut()
            .resource_mut::<crate::aim_zoom::AimZoom>()
            .aimed = true;
        let before = app.world().resource::<Player>().heading;
        app.update();
        let scoped = app.world().resource::<Player>().heading - before;
        assert!((scoped - normal * 0.5).abs() < 1e-6);
    }
    #[test]
    fn configured_dead_zone_filters_both_action_sticks_without_overwriting_hardware_axes() {
        let (mut app, pad, _) = scene();
        axis(&mut app, pad, GamepadAxis::LeftStickX, 0.25);
        axis(&mut app, pad, GamepadAxis::RightStickX, 0.25);
        app.update();
        assert!(app.world().resource::<Sticks>().0.x > 0.0);
        assert!(app.world().resource::<Sticks>().1.x > 0.0);
        let physical = app.world().get::<Gamepad>(pad).unwrap().left_stick();
        app.world_mut().resource_mut::<Options>().dead_zone = 0.35;
        app.update();
        assert_eq!(app.world().resource::<Sticks>().0, Vec2::ZERO);
        assert_eq!(app.world().resource::<Sticks>().1, Vec2::ZERO);
        assert_eq!(
            app.world().get::<Gamepad>(pad).unwrap().left_stick(),
            physical
        );
        app.world_mut().resource_mut::<Options>().dead_zone = 0.0;
        app.update();
        assert_eq!(app.world().resource::<Sticks>().0, physical);
    }
    #[test]
    fn toggle_aim_uses_the_actual_remapped_control_and_release_does_not_toggle_again() {
        let (mut app, pad, _) = scene();
        options(&mut app, true, false);
        app.world_mut()
            .resource_mut::<Bindings>()
            .rebind_pad(Control::Block, PadInput::Button(GamepadButton::North));
        app.update();
        button(&mut app, pad, Control::Block, true);
        assert!(aiming(&app));
        button(&mut app, pad, Control::Block, false);
        assert!(aiming(&app));
        button(&mut app, pad, Control::Block, true);
        assert!(!aiming(&app));
    }
    #[test]
    fn overlapping_pads_cannot_toggle_an_already_held_aim_control_twice() {
        let (mut app, first, _) = scene();
        let second = app.world_mut().spawn(Gamepad::default()).id();
        app.update();
        options(&mut app, true, false);
        button(&mut app, first, Control::Block, true);
        assert!(aiming(&app));
        button(&mut app, second, Control::Block, true);
        assert!(aiming(&app));
        button(&mut app, first, Control::Block, false);
        assert!(aiming(&app));
        button(&mut app, second, Control::Block, false);
        assert!(aiming(&app));
        button(&mut app, second, Control::Block, true);
        assert!(!aiming(&app));
    }
    #[test]
    fn controller_hold_sneak_restores_each_independent_native_toggle_base() {
        for base in [false, true] {
            let (mut app, pad, _) = scene();
            options(&mut app, false, true);
            app.world_mut().resource_mut::<PlayerControl>().sneak = base;
            button(&mut app, pad, Control::CrouchSneak, true);
            assert!(sneak(&app));
            button(&mut app, pad, Control::CrouchSneak, false);
            assert_eq!(sneak(&app), base);
            assert!(!app.world().resource::<HeldSneakInput>().active);
        }
    }
    #[test]
    fn native_invalidation_cancels_toggle_and_hold_without_rearming_a_still_held_button() {
        let (mut app, pad, _) = scene();
        options(&mut app, true, true);
        raw_button(&mut app, pad, Control::Block, true);
        raw_button(&mut app, pad, Control::CrouchSneak, true);
        app.update();
        assert!(aiming(&app) && sneak(&app));
        crate::bindings::cancel_input_holds(app.world_mut());
        app.update();
        assert!(!aiming(&app) && !sneak(&app));
        app.update();
        assert!(!aiming(&app) && !sneak(&app));
        raw_button(&mut app, pad, Control::Block, false);
        raw_button(&mut app, pad, Control::CrouchSneak, false);
        app.update();
        raw_button(&mut app, pad, Control::Block, true);
        raw_button(&mut app, pad, Control::CrouchSneak, true);
        app.update();
        assert!(aiming(&app) && sneak(&app));
    }
    #[test]
    fn pause_focus_and_platform_block_release_requests_and_require_neutral_before_resume() {
        for reason in 0..4 {
            let (mut app, pad, w) = scene();
            options(&mut app, true, true);
            raw_button(&mut app, pad, Control::Block, true);
            raw_button(&mut app, pad, Control::CrouchSneak, true);
            app.update();
            match reason {
                0 => {
                    app.world_mut()
                        .resource_mut::<NextState<PlayState>>()
                        .set(PlayState::Menu);
                }
                1 => {
                    app.world_mut().write_message(WindowFocused {
                        window: w,
                        focused: false,
                    });
                }
                2 => {
                    app.init_resource::<crate::pad::PlatformInputGate>();
                    app.world_mut()
                        .resource_mut::<crate::pad::PlatformInputGate>()
                        .set_blocked(true);
                }
                _ => {
                    let mut locks = app.world_mut().resource_mut::<ControlLocks>();
                    locks.fighting = true;
                    locks.sneaking = true;
                }
            }
            app.update();
            assert!(!aiming(&app) && !sneak(&app));
            match reason {
                0 => {
                    app.world_mut()
                        .resource_mut::<NextState<PlayState>>()
                        .set(PlayState::Playing);
                }
                1 => {
                    app.world_mut().write_message(WindowFocused {
                        window: w,
                        focused: true,
                    });
                }
                2 => {
                    app.world_mut()
                        .resource_mut::<crate::pad::PlatformInputGate>()
                        .set_blocked(false);
                }
                _ => {
                    let mut locks = app.world_mut().resource_mut::<ControlLocks>();
                    locks.fighting = false;
                    locks.sneaking = false;
                }
            }
            app.update();
            app.update();
            assert!(!aiming(&app) && !sneak(&app));
            raw_button(&mut app, pad, Control::Block, false);
            raw_button(&mut app, pad, Control::CrouchSneak, false);
            app.update();
            raw_button(&mut app, pad, Control::Block, true);
            raw_button(&mut app, pad, Control::CrouchSneak, true);
            app.update();
            assert!(aiming(&app) && sneak(&app));
        }
    }
    #[test]
    fn removing_any_contributing_pad_and_device_change_clear_latches_without_false_sneak_edges() {
        let (mut app, pad, _) = scene();
        options(&mut app, true, true);
        raw_button(&mut app, pad, Control::Block, true);
        raw_button(&mut app, pad, Control::CrouchSneak, true);
        app.update();
        assert!(aiming(&app) && sneak(&app));
        app.world_mut().despawn(pad);
        app.update();
        assert!(!aiming(&app) && !sneak(&app));
        crate::bindings::set_device(app.world_mut(), InputDevice::KeyboardMouse);
        app.update();
        assert!(!aiming(&app) && !sneak(&app));
    }
    #[test]
    fn settings_roundtrip_defaults_and_bounded_options_in_the_real_ini_registry() {
        let mut app = App::new();
        app.add_plugins(ControllerAimPlugin);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "nvf-controller-aim-{}-{nonce}.ini",
            std::process::id()
        ));
        app.insert_resource(settings::SettingsFile(Some(path.clone())));
        // Saving another switch must not shift the default gains/dead zone on reload.
        let index = |app: &App, key| {
            app.world()
                .resource::<settings::Settings>()
                .index(key)
                .unwrap()
        };
        let i = index(&app, KEYS[2]);
        settings::flip(app.world_mut(), i);
        let mut loaded = App::new();
        loaded
            .add_plugins(ControllerAimPlugin)
            .insert_resource(settings::SettingsFile(Some(path.clone())));
        settings::apply_stored(loaded.world_mut());
        assert_eq!(loaded.world().resource::<Options>().ads, 1.0);
        assert_eq!(
            loaded.world().resource::<Options>().dead_zone,
            crate::pad::STICK_DEAD
        );
        for key in [KEYS[0], KEYS[1]] {
            let i = index(&app, key);
            settings::set_slider(app.world_mut(), i, 1.0);
        }
        settings::apply_stored(loaded.world_mut());
        assert_eq!(loaded.world().resource::<Options>().ads, 2.0);
        assert_eq!(loaded.world().resource::<Options>().dead_zone, 0.4);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn consumed_controller_and_first_touch_return_edges_cannot_rearm_the_aim_latch() {
        use bevy::input::touch::{TouchInput, TouchPhase};
        let (mut app, pad, w) = scene();
        options(&mut app, true, false);
        app.init_resource::<crate::pad::TouchDeviceSwitch>();
        button(&mut app, pad, Control::Block, true);
        button(&mut app, pad, Control::Block, false);
        app.update();
        assert!(aiming(&app));
        for phase in [TouchPhase::Started, TouchPhase::Ended] {
            app.world_mut().write_message(TouchInput {
                window: w,
                id: 42,
                phase,
                position: Vec2::splat(40.0),
                force: None,
            });
        }
        app.update();
        assert_eq!(
            app.world().resource::<Bindings>().device,
            InputDevice::KeyboardMouse
        );
        assert!(!app
            .world()
            .resource::<crate::pad::TouchDeviceSwitch>()
            .allows_actions());
        assert!(!aiming(&app));
        app.update();
        button(&mut app, pad, Control::Block, true);
        assert_eq!(
            app.world().resource::<Bindings>().device,
            InputDevice::Controller
        );
        assert!(!aiming(&app), "the device-selecting press is consumed");
        app.update();
        assert!(
            !aiming(&app),
            "a continued hold cannot become a fresh toggle"
        );
        button(&mut app, pad, Control::Block, false);
        button(&mut app, pad, Control::Block, true);
        assert!(aiming(&app));
    }
    #[test]
    fn explicit_script_sneak_requests_record_the_base_beneath_the_controller_hold() {
        for requested in [false, true] {
            let (mut app, pad, _) = scene();
            options(&mut app, false, true);
            app.insert_resource(crate::script::ScriptState::over_no_plugins())
                .add_systems(
                    Update,
                    crate::control::tell_scripts.after(crate::control::read_controls),
                );
            button(&mut app, pad, Control::CrouchSneak, true);
            assert!(sneak(&app));
            app.world_mut()
                .resource_mut::<crate::script::ScriptState>()
                .rt
                .as_mut()
                .unwrap()
                .game
                .gaps
                .sneak_request = Some(requested);
            app.update();
            app.update();
            assert!(sneak(&app));
            assert!(app
                .world()
                .resource::<crate::script::ScriptState>()
                .rt
                .as_ref()
                .unwrap()
                .game
                .gaps
                .sneak_request
                .is_none());
            button(&mut app, pad, Control::CrouchSneak, false);
            assert_eq!(sneak(&app), requested);
        }
    }
    #[cfg(not(target_os = "android"))]
    #[test]
    fn desktop_pad_plugin_omits_android_controller_rows_and_keeps_immediate_aim_after_menu_and_unfocused() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, InputPlugin, bevy::state::app::StatesPlugin))
            .init_resource::<Assets<Image>>()
            .insert_state(PlayState::Menu)
            .init_resource::<Bindings>()
            .init_resource::<PlayerControl>()
            .init_resource::<ControlLocks>()
            .init_resource::<crate::furniture::PlayerSeat>()
            .insert_resource(crate::pause::Paused(false))
            .insert_resource(settings::SettingsFile(None))
            .add_plugins(crate::pad::PadPlugin)
            .add_systems(Update, crate::control::read_controls);
        // Desktop PadPlugin must NOT install ControllerAimPlugin or register its 4 rows.
        assert!(!app.world().contains_resource::<Options>());
        assert!(!app.world().contains_resource::<Gestures>());
        if let Some(s) = app.world().get_resource::<settings::Settings>() {
            for key in KEYS {
                assert_eq!(s.index(key), None, "desktop must not register {key}");
            }
        }
        app.world_mut().resource_mut::<Bindings>().device = InputDevice::Controller;
        let pad = app.world_mut().spawn(Gamepad::default()).id();
        // Unfocused primary window on desktop still allows aiming (matches main).
        app.world_mut().spawn((Window { focused: false, ..default() }, PrimaryWindow));
        app.update();
        raw_button(&mut app, pad, Control::Block, true);
        app.update();
        assert!(!aiming(&app), "menu state does not aim");
        // Closing menu while LT is still held aims immediately on desktop (no release hold-off).
        app.world_mut().resource_mut::<NextState<PlayState>>().set(PlayState::Playing);
        app.update();
        app.update();
        assert!(aiming(&app), "desktop aims immediately on menu close even when unfocused");
    }
    include!("controller_aim/main_aim_union_tests.rs");
}
