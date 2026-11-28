//! Raw touch admission for the live companion wheel; no independent command table.
use super::*;
use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::window::{AppLifecycle, WindowFocused};
use serde::{Deserialize, Serialize};
use std::io::Read;
const SETTINGS_LIMIT: usize = 4096;
fn may_write() -> bool {
    true
}
use std::collections::HashSet;
#[derive(Resource, Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub drag_release: bool,
    #[serde(skip, default = "may_write")]
    allow_write: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: cfg!(target_os = "android"),
            drag_release: false,
            allow_write: true,
        }
    }
}
impl Settings {
    fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > SETTINGS_LIMIT {
            return Err("Touch settings exceed the v1 size limit".into());
        }
        let document: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if document["version"].as_u64() != Some(1) {
            return Err("Unsupported touch settings version; file preserved".into());
        }
        if document
            .as_object()
            .is_none_or(|object| object.len() != 2 || !object.contains_key("settings"))
        {
            return Err("Unrecognized v1 settings fields; file preserved".into());
        }
        let mut value: Self =
            serde_json::from_value(document["settings"].clone()).map_err(|e| e.to_string())?;
        value.allow_write = true;
        Ok(value)
    }
    fn import(&mut self, bytes: &[u8]) -> Result<(), String> {
        match Self::decode(bytes) {
            Ok(value) => {
                *self = value;
                Ok(())
            }
            Err(error) => {
                self.allow_write = false;
                Err(error)
            }
        }
    }
    fn save_to(&self, path: &std::path::Path) -> Result<(), String> {
        if !self.allow_write {
            return Err("Unrecognized settings file is protected from rewriting".into());
        }
        let bytes = serde_json::to_vec_pretty(&serde_json::json!({"version":1,"settings":self}))
            .map_err(|e| e.to_string())?;
        if bytes.len() > SETTINGS_LIMIT {
            return Err("Touch settings exceed the v1 size limit".into());
        }
        let parent = path.parent().ok_or("Settings directory unavailable")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let temporary = path.with_file_name("companion-touch.json.tmp");
        std::fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(temporary, path).map_err(|e| e.to_string())
    }
    fn save(&self) -> Result<(), String> {
        let home = fnv_install::EngineHome::default_root().ok_or("Engine home unavailable")?;
        self.save_to(&home.join("companion-touch.json"))
    }
}
pub(super) fn load(mut settings: ResMut<Settings>) {
    if !cfg!(target_os = "android") {
        return;
    }
    let Some(home) = fnv_install::EngineHome::default_root() else {
        return;
    };
    match std::fs::File::open(home.join("companion-touch.json")) {
        Ok(file) => {
            let mut bytes = Vec::new();
            let result = file
                .take((SETTINGS_LIMIT + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())
                .and_then(|_| settings.import(&bytes));
            if let Err(error) = result {
                settings.allow_write = false;
                warn!("companion touch settings: {error}; existing file will not be rewritten");
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            settings.allow_write = false;
            warn!("companion touch settings: {error}; existing file will not be rewritten");
        }
    }
}
/// The HUD bridge uses this predicate to release/suppress its generic menu contacts.
pub fn captures(settings: &Settings, screen: &WheelScreen) -> bool {
    settings.enabled && screen.0.is_some()
}

/// Register the saved presentation switch for full or independently hosted menus.
pub fn register(app: &mut App) {
    crate::touch_ui_options::install(app);
    app.init_resource::<Settings>();
    crate::settings::add_setting(app, crate::settings::Setting {
        key: "touch-companion-v1", group: crate::pause::GROUP_MENUS,
        label: "Enhanced companion wheel", switches: "",
        note: "Direct sector taps and optional drag-release. Off keeps standard menu touch arrows, Confirm and Exit, plus mouse/controller input. Drag-release preference is kept.",
        get: |w| w.resource::<Settings>().enabled, set: set_enabled, slider: None,
    });
}
fn set_enabled(world: &mut World, enabled: bool) {
    if world.resource::<Settings>().enabled == enabled { return; }
    world.resource_mut::<Settings>().enabled = enabled;
    let (window, ids) = if let Some(mut state) = world.get_resource_mut::<TouchState>() {
        let owned = (state.viewport.map(|v| v.0), state.contacts.iter().copied().take(33).collect::<Vec<_>>());
        state.gesture = None;
        state.contacts.clear();
        owned
    } else { (None, Vec::new()) };
    crate::touch_ui_options::retire(world, crate::touch_ui_options::Feature::Companion, window, &ids);
}
#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Sector(usize),
    Cancel,
    Mode,
    Outside,
}
struct Gesture {
    id: u64,
    start: Vec2,
    hit: Hit,
    dragged: bool,
}
#[derive(Resource, Default)]
pub(super) struct TouchState {
    session: Option<FormId>,
    gesture: Option<Gesture>,
    contacts: HashSet<u64>,
    suspended: bool,
    focused: Option<bool>,
    viewport: Option<(Entity, Vec2)>,
    message: String,
}
#[derive(Component)]
pub(super) struct TouchRoot;
fn mode_rect(size: Vec2) -> Rect {
    Rect::from_corners(
        Vec2::new(size.x - 180.0, 8.0),
        Vec2::new(size.x - 8.0, 56.0),
    )
}
fn point(position: Vec2, size: Vec2) -> Hit {
    if mode_rect(size).contains(position) {
        return Hit::Mode;
    }
    let p = crate::interface::from_menu_frame(position, size, crate::interface::MIDDLE);
    let k = 960.0 / size.y.max(1.0);
    let centre = Vec2::new(size.x * k / 2.0, 480.0 - RAISE);
    match super::hit(p * k - centre) {
        Ok(Some(i)) => Hit::Sector(i),
        Err(()) => Hit::Cancel,
        _ => Hit::Outside,
    }
}
fn release(
    start: Hit,
    end: Hit,
    drag: bool,
    moved: bool,
    buttons: &[WheelButton],
) -> Option<WheelInput> {
    if matches!(start, Hit::Mode | Hit::Outside) {
        return None;
    }
    if !drag && (start != end || moved) {
        return None;
    }
    match end {
        Hit::Cancel => Some(WheelInput::Close),
        Hit::Sector(i) => buttons
            .get(i)
            .filter(|b| b.enabled)
            .map(|b| WheelInput::Use(b.command)),
        _ => None,
    }
}
#[allow(clippy::too_many_arguments)]
pub(super) fn input(
    mut screen: ResMut<WheelScreen>,
    mut state: ResMut<TouchState>,
    mut settings: ResMut<Settings>,
    mut touches: MessageReader<TouchInput>,
    mut focus: MessageReader<WindowFocused>,
    mut lifecycle: MessageReader<AppLifecycle>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    bindings: Option<Res<crate::bindings::Bindings>>,
    touch_switch: Option<Res<crate::pad::TouchDeviceSwitch>>,
    platform: Option<Res<crate::pad::PlatformInputGate>>,
    ui_capture: Option<Res<crate::ui_pointer::TouchUiCapture>>,
    retirements: Option<Res<crate::touch_ui_options::Retirements>>,
    mut out: MessageWriter<WheelInput>,
) {
    let mut interrupted = false;
    for event in lifecycle.read() {
        match event {
            AppLifecycle::Suspended => {
                state.suspended = true;
                interrupted = true;
            }
            AppLifecycle::Running => {
                state.suspended = false;
                interrupted = true;
            }
            _ => {}
        }
    }
    for event in focus.read() {
        if windows.iter().any(|(id, _)| id == event.window) {
            state.focused = Some(event.focused);
            interrupted = true;
        }
    }
    let session = screen.0.as_ref().map(|s| s.companion);
    if state.session != session || interrupted || screen.0.as_ref().is_some_and(|s| s.fresh) {
        state.session = session;
        state.gesture = None;
        state.contacts.clear();
        touches.clear();
        return;
    }
    let Ok((window_id, window)) = windows.single() else {
        state.viewport = None;
        state.gesture = None;
        state.contacts.clear();
        touches.clear();
        return;
    };
    let viewport = (window_id, Vec2::new(window.width(), window.height()));
    if state.viewport != Some(viewport) {
        state.viewport = Some(viewport);
        state.gesture = None;
        state.contacts.clear();
        touches.clear();
        return;
    }
    let device_allows_actions = touch_switch.as_deref().is_none_or(crate::pad::TouchDeviceSwitch::allows_actions);
    if retirements.as_ref().is_some_and(|r| r.barrier(crate::touch_ui_options::Feature::Companion)) {
        state.gesture = None;
        state.contacts.clear();
        touches.read().for_each(drop);
        return;
    }
    if !settings.enabled
        || state.suspended
        || !window.focused
        || state.focused == Some(false)
        || !device_allows_actions
        || platform.as_ref().is_some_and(|gate| !gate.allows_actions())
        || bindings.is_some_and(|b| b.device == crate::bindings::InputDevice::Controller)
    {
        state.gesture = None;
        state.contacts.clear();
        touches.clear();
        return;
    }
    let Some(session) = screen.0.as_mut() else {
        state.gesture = None;
        touches.clear();
        return;
    };
    let size = Vec2::new(window.width(), window.height());
    let mut sent = false;
    for touch in touches.read().filter(|e| e.window == window_id
        && ui_capture.as_ref().is_none_or(|capture| capture.allows(e.id))
        && platform.as_ref().is_none_or(|gate| gate.allows_touch(e.id))) {
        if !touch.position.is_finite() {
            state.gesture = None;
            state.contacts.remove(&touch.id);
            continue;
        }
        match touch.phase {
            TouchPhase::Started => {
                if !state.contacts.insert(touch.id) || state.contacts.len() != 1 {
                    state.gesture = None;
                    continue;
                }
                let hit = point(touch.position, size);
                state.gesture = Some(Gesture {
                    id: touch.id,
                    start: touch.position,
                    hit,
                    dragged: false,
                });
                session.selected = if let Hit::Sector(i) = hit {
                    Some(i)
                } else {
                    None
                };
                session.exit_hover = hit == Hit::Cancel;
            }
            TouchPhase::Moved => {
                if let Some(gesture) = state.gesture.as_mut().filter(|g| g.id == touch.id) {
                    gesture.dragged |= touch.position.distance(gesture.start) > 10.0;
                    let hit = point(touch.position, size);
                    session.selected = if let Hit::Sector(i) = hit {
                        Some(i)
                    } else {
                        None
                    };
                    session.exit_hover = hit == Hit::Cancel;
                }
            }
            TouchPhase::Canceled => {
                state.contacts.remove(&touch.id);
                state.gesture = None;
                session.selected = None;
                session.exit_hover = false;
            }
            TouchPhase::Ended => {
                state.contacts.remove(&touch.id);
                let Some(g) = state.gesture.take().filter(|g| g.id == touch.id) else {
                    continue;
                };
                let hit = point(touch.position, size);
                if g.hit == Hit::Mode
                    && hit == Hit::Mode
                    && !g.dragged
                    && touch.position.distance(g.start) <= 10.0
                {
                    settings.drag_release = !settings.drag_release;
                    state.message = match settings.save() {
                        Ok(()) => String::new(),
                        Err(e) => format!("Options not saved: {e}"),
                    };
                } else if !sent {
                    if let Some(action) = release(
                        g.hit,
                        hit,
                        settings.drag_release,
                        g.dragged || touch.position.distance(g.start) > 10.0,
                        &session.buttons,
                    ) {
                        out.write(action);
                        sent = true;
                    }
                }
            }
        }
    }
}
pub(super) fn draw(
    mut commands: Commands,
    screen: Res<WheelScreen>,
    settings: Res<Settings>,
    state: Res<TouchState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    roots: Query<Entity, With<TouchRoot>>,
    mut shown: Local<String>,
) {
    let Ok(window) = windows.single() else { return };
    let active = captures(&settings, &screen);
    let key = format!(
        "{active}:{}:{}:{}:{}:{:?}",
        window.width(),
        window.height(),
        settings.drag_release,
        state.message,
        screen
            .0
            .as_ref()
            .map(|s| (s.selected, s.buttons.clone(), s.unavailable.clone()))
    );
    if *shown == key {
        return;
    }
    *shown = key;
    for e in &roots {
        commands.entity(e).despawn();
    }
    if !active {
        return;
    }
    let rect = mode_rect(Vec2::new(window.width(), window.height()));
    commands
        .spawn((
            TouchRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(rect.min.x),
                top: Val::Px(rect.min.y),
                width: Val::Px(rect.width()),
                height: Val::Px(rect.height()),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.03, 0.02, 0.95)),
            GlobalZIndex(3500),
        ))
        .with_children(|root| {
            root.spawn((
                Text::new(if settings.drag_release {
                    "Drag → release"
                } else {
                    "Tap a sector"
                }),
                TextFont {
                    font_size: FontSize::Px(20.0),
                    ..default()
                },
                TextColor(Color::WHITE),
            ));
            if !state.message.is_empty() {
                root.spawn((
                    Text::new(&state.message),
                    TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                ));
            }
        });
    let preview = screen
        .0
        .as_ref()
        .and_then(|s| s.selected.and_then(|i| s.buttons.get(i).map(|b| (s, i, b))))
        .map_or_else(
            || "Tap a sector; the center cancels.".to_string(),
            |(s, i, b)| {
                format!(
                    "{}{}{}",
                    b.label,
                    s.unavailable
                        .get(i)
                        .and_then(Option::as_deref)
                        .map_or_else(String::new, |reason| format!(" · {reason}")),
                    b.context
                        .as_ref()
                        .map_or_else(String::new, |context| format!("\n{context}"))
                )
            },
        );
    commands
        .spawn((
            TouchRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(16.0),
                bottom: Val::Px(8.0),
                width: Val::Px(window.width() - 32.0),
                min_height: Val::Px(56.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.03, 0.02, 0.95)),
            GlobalZIndex(3500),
        ))
        .with_children(|root| {
            root.spawn((
                Text::new(preview),
                TextFont {
                    font_size: FontSize::Px(20.0),
                    ..default()
                },
                TextColor(Color::WHITE),
            ));
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scene() -> (App, Entity) {
        let mut script = crate::script::ScriptState::over_no_plugins();
        let game = &mut script.rt.as_mut().unwrap().game;
        let companion = FormId(0x99);
        game.extra.entry(companion).or_default().teammate = true;
        assert!(game.open_companion_wheel(companion));
        let buttons = game.wheel_buttons(companion);
        let unavailable = buttons
            .iter()
            .map(|b| game.wheel_unavailable_reason_for(companion, b))
            .collect();
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::input::InputPlugin,
            bevy::state::app::StatesPlugin,
        ))
        .insert_state(PlayState::Menu)
        .init_resource::<TouchState>()
        .init_resource::<crate::pause::PlayClock>()
        .init_resource::<WheelStats>()
        .init_resource::<crate::bindings::Bindings>()
        .insert_resource(Settings {
            enabled: true,
            drag_release: false,
            allow_write: false,
        })
        .insert_resource(WheelScreen(Some(Session {
            companion,
            name: "Synthetic companion".into(),
            buttons,
            unavailable,
            selected: None,
            exit_hover: false,
            fresh: false,
            refreshed: 0.0,
        })))
        .insert_resource(script)
        .insert_resource(crate::sim::test_world::sim(vec![], vec![]))
        .add_message::<TouchInput>()
        .add_message::<WindowFocused>()
        .add_message::<AppLifecycle>()
        .add_message::<WheelInput>()
        .add_systems(Update, (input, super::super::apply).chain());
        let window = app
            .world_mut()
            .spawn((
                Window {
                    resolution: bevy::window::WindowResolution::new(1000, 600),
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        app.update();
        app.update();
        (app, window)
    }
    fn at(command: Command) -> Vec2 {
        let slice = &SLICES[command.id()];
        let top = (960.0 - SIZE) / 2.0 - RAISE;
        Vec2::new(500.0, (480.0 - RAISE) * 600.0 / 960.0)
            + Vec2::new(
                slice.icon[0] + ICON / 2.0 - SIZE / 2.0,
                slice.icon[1] - top + ICON / 2.0 - SIZE / 2.0,
            ) * (600.0 / 960.0)
    }
    fn event(app: &mut App, w: Entity, id: u64, phase: TouchPhase, p: Vec2) {
        app.world_mut().write_message(TouchInput {
            window: w,
            id,
            phase,
            position: p,
            force: None,
        });
    }
    fn tap(app: &mut App, w: Entity, p: Vec2) {
        event(app, w, 1, TouchPhase::Started, p);
        event(app, w, 1, TouchPhase::Ended, p);
        app.update();
    }
    #[test]
    fn loading_cover_cancels_a_wheel_gesture_without_executing_it() {
        let (mut app, w) = scene();
        app.init_resource::<crate::pad::PlatformInputGate>();
        let p = at(Command::BackUp);
        event(&mut app, w, 79, TouchPhase::Started, p);
        app.update();
        assert!(app.world().resource::<TouchState>().gesture.is_some());
        app.world_mut().resource_mut::<crate::pad::PlatformInputGate>().set_blocked(true);
        event(&mut app, w, 79, TouchPhase::Ended, p);
        app.update();
        assert!(app.world().resource::<TouchState>().gesture.is_none());
        assert!(app.world().resource::<TouchState>().contacts.is_empty());
        assert!(app.world().resource::<WheelStats>().commands.is_empty());
        app.world_mut().resource_mut::<crate::pad::PlatformInputGate>().set_blocked(false);
        event(&mut app, w, 79, TouchPhase::Ended, p);
        app.update();
        assert!(app.world().resource::<WheelStats>().commands.is_empty());
        tap(&mut app, w, p);
        assert_eq!(app.world().resource::<WheelStats>().commands, vec![Command::BackUp.name().to_string()]);
    }
    #[test]
    fn v1_settings_roundtrip_and_unknown_versions_never_rewrite_the_existing_file() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "nvf-companion-touch-v1-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("companion-touch.json");
        let settings = Settings {
            enabled: true,
            ..default()
        };
        settings.save_to(&path).unwrap();
        let stored = std::fs::read(&path).unwrap();
        let decoded = Settings::decode(&stored).unwrap();
        assert!(decoded.enabled);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&stored).unwrap()["version"],
            1
        );
        let future = br#"{"version":99,"settings":{"enabled":true},"future":"keep exactly"}"#;
        std::fs::write(&path, future).unwrap();
        let mut blocked = settings.clone();
        assert!(blocked.import(future).is_err());
        assert!(blocked.save_to(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), future);
        assert!(Settings::decode(&vec![b' '; SETTINGS_LIMIT + 1]).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }
    #[test]
    fn direct_tap_reaches_actual_game_command_once_even_with_duplicate_producers() {
        let (mut app, w) = scene();
        app.world_mut()
            .write_message(WheelInput::Use(Command::BackUp));
        app.world_mut()
            .write_message(WheelInput::Use(Command::BackUp));
        tap(&mut app, w, at(Command::BackUp));
        assert_eq!(
            app.world().resource::<WheelStats>().commands,
            vec![Command::BackUp.name().to_string()]
        );
        let mut script = app.world_mut().resource_mut::<crate::script::ScriptState>();
        let game = &mut script.rt.as_mut().unwrap().game;
        assert!(game.take_back_up(FormId(0x99)).is_some());
        assert!(game.take_back_up(FormId(0x99)).is_none());
    }
    #[test]
    fn disabled_sector_selects_its_actual_reason_without_executing_and_hub_cancels() {
        let (mut app, w) = scene();
        tap(&mut app, w, at(Command::UseStimpak));
        let session = app.world().resource::<WheelScreen>().0.as_ref().unwrap();
        assert_eq!(session.selected, Some(Command::UseStimpak.id()));
        assert_eq!(
            session.unavailable[Command::UseStimpak.id()].as_deref(),
            Some("You have no stimpaks.")
        );
        assert!(app.world().resource::<WheelStats>().commands.is_empty());
        tap(
            &mut app,
            w,
            Vec2::new(500.0, (480.0 - RAISE) * 600.0 / 960.0),
        );
        assert!(app.world().resource::<WheelScreen>().0.is_none());
        assert!(app
            .world()
            .resource::<crate::script::ScriptState>()
            .rt
            .as_ref()
            .unwrap()
            .game
            .wheel_companion()
            .is_none());
    }
    #[test]
    fn drag_release_is_opt_in_and_uses_only_the_final_enabled_sector() {
        for drag in [false, true] {
            let (mut app, w) = scene();
            app.world_mut().resource_mut::<Settings>().drag_release = drag;
            event(&mut app, w, 1, TouchPhase::Started, at(Command::UseStimpak));
            event(&mut app, w, 1, TouchPhase::Moved, at(Command::BackUp));
            event(&mut app, w, 1, TouchPhase::Ended, at(Command::BackUp));
            app.update();
            assert_eq!(
                app.world().resource::<WheelStats>().commands.len(),
                drag as usize
            );
        }
    }
    #[test]
    fn opening_focus_suspend_cancel_multitouch_and_controller_return_do_not_execute() {
        for case in 0..6 {
            let (mut app, w) = scene();
            event(&mut app, w, 1, TouchPhase::Started, at(Command::BackUp));
            match case {
                0 => {
                    app.world_mut()
                        .resource_mut::<WheelScreen>()
                        .0
                        .as_mut()
                        .unwrap()
                        .fresh = true
                }
                1 => {
                    app.world_mut().write_message(WindowFocused {
                        window: w,
                        focused: false,
                    });
                }
                2 => {
                    app.world_mut().write_message(AppLifecycle::Suspended);
                }
                3 => event(&mut app, w, 1, TouchPhase::Canceled, at(Command::BackUp)),
                4 => event(&mut app, w, 2, TouchPhase::Started, at(Command::BackUp)),
                _ => {
                    app.world_mut()
                        .resource_mut::<crate::bindings::Bindings>()
                        .device = crate::bindings::InputDevice::Controller
                }
            }
            event(&mut app, w, 1, TouchPhase::Ended, at(Command::BackUp));
            app.update();
            assert!(
                app.world().resource::<WheelStats>().commands.is_empty(),
                "case {case}"
            );
        }
    }
    #[test]
    fn focus_resume_requires_a_fresh_post_resume_gesture() {
        let (mut app, w) = scene();
        event(&mut app, w, 1, TouchPhase::Started, at(Command::BackUp));
        app.update();
        app.world_mut().write_message(WindowFocused {
            window: w,
            focused: false,
        });
        app.update();
        app.world_mut().write_message(WindowFocused {
            window: w,
            focused: true,
        });
        event(&mut app, w, 1, TouchPhase::Ended, at(Command::BackUp));
        app.update();
        assert!(app.world().resource::<WheelStats>().commands.is_empty());
        tap(&mut app, w, at(Command::BackUp));
        assert_eq!(app.world().resource::<WheelStats>().commands.len(), 1);
    }
    #[test]
    fn out_and_back_drag_never_activates_direct_tap_but_opt_in_uses_the_final_sector() {
        for drag_release in [false, true] {
            let (mut app, w) = scene();
            app.world_mut().resource_mut::<Settings>().drag_release = drag_release;
            event(&mut app, w, 17, TouchPhase::Started, at(Command::BackUp));
            event(&mut app, w, 17, TouchPhase::Moved, at(Command::UseStimpak));
            event(&mut app, w, 17, TouchPhase::Moved, at(Command::BackUp));
            event(&mut app, w, 17, TouchPhase::Ended, at(Command::BackUp));
            app.update();
            assert_eq!(app.world().resource::<WheelStats>().commands.len(), usize::from(drag_release));
            let mut script = app.world_mut().resource_mut::<crate::script::ScriptState>();
            let game = &mut script.rt.as_mut().unwrap().game;
            assert_eq!(game.take_back_up(FormId(0x99)).is_some(), drag_release);
            assert!(game.take_back_up(FormId(0x99)).is_none());
        }
    }
    #[test]
    fn actual_controller_return_touch_only_restores_device_before_a_fresh_wheel_tap() {
        let (mut app, w) = scene();
        app.init_resource::<crate::pad::TouchDeviceSwitch>()
            .add_plugins(crate::pad::InputSwitchPlugin);
        app.world_mut().resource_mut::<crate::bindings::Bindings>().device = crate::bindings::InputDevice::Controller;
        tap(&mut app, w, at(Command::BackUp));
        assert_eq!(app.world().resource::<crate::bindings::Bindings>().device, crate::bindings::InputDevice::KeyboardMouse);
        assert!(!app.world().resource::<crate::pad::TouchDeviceSwitch>().allows_actions());
        assert!(app.world().resource::<WheelStats>().commands.is_empty());
        tap(&mut app, w, at(Command::BackUp));
        assert_eq!(app.world().resource::<WheelStats>().commands.len(), 1);
        let mut script = app.world_mut().resource_mut::<crate::script::ScriptState>();
        assert!(script.rt.as_mut().unwrap().game.take_back_up(FormId(0x99)).is_some());
    }

    #[test]
    fn mode_out_and_back_drag_keeps_setting_until_a_fresh_valid_tap() {
        let (mut app, w) = scene();
        let mode = mode_rect(Vec2::new(1000.0, 600.0)).center();
        event(&mut app, w, 51, TouchPhase::Started, mode);
        event(&mut app, w, 51, TouchPhase::Moved, at(Command::BackUp));
        event(&mut app, w, 51, TouchPhase::Moved, mode);
        event(&mut app, w, 51, TouchPhase::Ended, mode);
        app.update();
        assert!(!app.world().resource::<Settings>().drag_release);
        assert!(app.world().resource::<WheelStats>().commands.is_empty());
        app.update();
        assert!(!app.world().resource::<Settings>().drag_release);
        tap(&mut app, w, mode);
        assert!(app.world().resource::<Settings>().drag_release);
        assert!(app.world().resource::<WheelStats>().commands.is_empty());
    }
    include!("touch_toggle_tests.rs");
}
