//! Optional, owned bottom handle -> Inventory or the existing utility tray request.
//! [design] Only the safe, unclaimed bottom band starts a gesture. System edges, look, controls and item
//! taps are never global swipe listeners. Both destinations share this recognizer.
use bevy::{
    ecs::system::SystemParam,
    input::touch::{TouchInput, TouchPhase},
    prelude::*,
    window::PrimaryWindow,
};
use nvf_play::settings::{self, ChoiceList, Setting, Slider};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Destination {
    #[default]
    Inventory,
    UtilityTray,
}
impl Destination {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Inventory => "Inventory",
            Self::UtilityTray => "Utility tray",
        }
    }
    fn named(value: &str) -> Option<Self> {
        match value {
            "Inventory" => Some(Self::Inventory),
            "Utility tray" => Some(Self::UtilityTray),
            _ => None,
        }
    }
}
#[derive(Resource, Clone, Copy)]
pub(super) struct Options {
    pub enabled: bool,
    pub threshold: f32,
    pub destination: Destination,
}
impl Options {
    pub(super) fn for_platform(android: bool) -> Self {
        Self {
            enabled: android,
            threshold: 96.0,
            destination: Destination::Inventory,
        }
    }
}
impl Default for Options {
    fn default() -> Self {
        Self::for_platform(cfg!(target_os = "android"))
    }
}
#[derive(Resource)]
pub(super) struct SafeBottom(pub f32);
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct PublishSafeBottom;
impl Default for SafeBottom {
    fn default() -> Self {
        Self(24.0)
    }
}
#[derive(Clone, Copy)]
struct Drag {
    id: u64,
    origin: Vec2,
    position: Vec2,
    size: Vec2,
    canceled: bool,
    vertical: bool,
}
#[derive(Resource, Default)]
pub(super) struct Drawer {
    drag: Option<Drag>,
    swallowed: HashSet<u64>,
    focused: Option<bool>,
    stamp: Option<(u32, u32, Destination, nvf_play::touch_safe_area::SafeInsets)>,
}

pub(super) fn handle(size: Vec2, bottom: f32) -> Rect {
    let width = (size.x - 32.0).clamp(48.0, 144.0);
    Rect::from_center_size(
        Vec2::new(size.x * 0.5, size.y - bottom.max(0.0) - 36.0),
        Vec2::new(width, 48.0),
    )
}
/// Horizontal intent is retired once it leaves the initial slop. A diagonal
/// vertical drag can travel farther sideways without hitting an absolute cap.
pub(super) fn horizontal_intent(delta:Vec2) -> bool {
    delta.x.abs()>32.0 && delta.x.abs()>delta.y.abs()
}
pub(super) fn vertical_intent(delta:Vec2)->bool {
    delta.y.abs()>32.0 && delta.y.abs()>=delta.x.abs()
}
pub(super) fn opening_area(size:Vec2,bottom:f32,safe:nvf_play::touch_safe_area::SafeInsets)->Rect {
    if !size.is_finite() || size.min_element()<64.0 || !bottom.is_finite()
        || [safe.left,safe.top,safe.right,safe.bottom].iter().any(|v|!v.is_finite() || *v<0.0) {return Rect::EMPTY;}
    let h=handle(size,bottom);
    Rect::from_corners(Vec2::new(16.,h.min.y),Vec2::new(size.x-16.,h.max.y))
        .intersect(Rect::from_corners(Vec2::new(safe.left,safe.top),size-Vec2::new(safe.right,safe.bottom.max(bottom))))
}
impl Drawer {
    pub fn set_focus(&mut self, focused: bool) {
        self.focused = Some(focused);
    }
    pub fn cancel(&mut self) {
        self.drag = None;
        self.swallowed.clear();
    }
    fn progress(&self) -> f32 {
        self.drag
            .map_or(0.0, |d| (d.origin.y - d.position.y).max(0.0))
    }
    /// (consumed, open request). A canceled horizontal drag cannot later become a
    /// vertical swipe, and another finger cannot inherit the original finger's role.
    fn event(
        &mut self,
        event: &TouchInput,
        size: Vec2,
        area: Rect,
        threshold: f32,
        unclaimed: bool,
    ) -> (bool, bool) {
        if self.swallowed.contains(&event.id) {
            if matches!(event.phase, TouchPhase::Ended | TouchPhase::Canceled) {
                self.swallowed.remove(&event.id);
            }
            return (true, false);
        }
        if event.phase == TouchPhase::Started && unclaimed && area.contains(event.position) {
            if self.drag.is_some() {
                self.swallowed.insert(event.id);
            } else {
                self.drag = Some(Drag {
                    id: event.id,
                    origin: event.position,
                    position: event.position,
                    size,
                    canceled: false,vertical:false,
                });
            }
            return (true, false);
        }
        let Some(mut drag) = self.drag.filter(|d| d.id == event.id) else {
            return (false, false);
        };
        drag.position = event.position;
        let delta = drag.position - drag.origin;
        if !drag.vertical {
            drag.canceled |= horizontal_intent(delta);
            drag.vertical=vertical_intent(delta);
        }
        drag.canceled |= drag.size != size
            || delta.y > 24.0
            || !event.position.is_finite();
        let opens = event.phase == TouchPhase::Ended
            && !drag.canceled
            && -delta.y >= threshold
            && drag.vertical;
        self.drag = if matches!(event.phase, TouchPhase::Ended | TouchPhase::Canceled) {
            None
        } else {
            Some(drag)
        };
        (true, opens)
    }
}

#[derive(SystemParam)]
pub(super) struct Bridge<'w> {
    pub drawer: ResMut<'w, Drawer>,
    pub options: Res<'w, Options>,
    pub safe: Res<'w, SafeBottom>,
    insets: Option<Res<'w,nvf_play::touch_safe_area::SafeInsets>>,
    pub capture: Option<Res<'w, nvf_play::bindings::KeyboardCapture>>,
    open: MessageWriter<'w, nvf_play::menu::OpenInventory>,
    hotbar: Option<Res<'w, super::hotbar::Preferences>>,
}
impl Bridge<'_> {
    pub fn allowed(&self) -> bool {
        self.options.enabled
            && (self.options.destination == Destination::Inventory
                || self.hotbar.as_ref().is_some_and(|p| p.tray_available()))
            && self.drawer.focused != Some(false)
            && !self.capture.as_ref().is_some_and(|c| c.0)
    }
    pub fn opens_tray(&self) -> bool {
        self.options.destination == Destination::UtilityTray
    }
    pub fn event(&mut self, event: &TouchInput, size: Vec2, unclaimed: bool) -> (bool, bool) {
        let stamp = (
            self.options.threshold.to_bits(),
            self.safe.0.to_bits(),
            self.options.destination,
            self.insets.as_deref().copied().unwrap_or_default(),
        );
        if self.drawer.stamp != Some(stamp) {
            self.drawer.cancel();
            self.drawer.stamp = Some(stamp);
        }
        if !self.allowed() {
            self.drawer.cancel();
            return (false, false);
        }
        let result = self.drawer.event(
            event,
            size,
            opening_area(size, self.safe.0, self.insets.as_deref().copied().unwrap_or_default()),
            self.options.threshold,
            unclaimed,
        );
        if result.1 {
            match self.options.destination {
                Destination::Inventory => {
                    self.open.write(nvf_play::menu::OpenInventory);
                }
                Destination::UtilityTray => {}
            }
        }
        result
    }
}

#[derive(Component)]
pub(super) struct Handle;
#[derive(Component)]
struct Hint;
fn spawn(mut commands: Commands) {
    commands
        .spawn((
            Handle,
            Node {
                position_type: PositionType::Absolute,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_direction: FlexDirection::Column,
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(2001),
            Pickable::IGNORE,
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    width: px(44.0),
                    height: px(5.0),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.9, 1.0, 0.95, 0.9)),
                Pickable::IGNORE,
            ));
            p.spawn((
                Hint,
                Text::new("Pip-Boy"),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(Color::WHITE),
                Pickable::IGNORE,
            ));
        });
}
type HandleFilter = (With<Handle>, Without<Hint>);
type VisualState<'w> = (
    Res<'w, Options>,
    Res<'w, Drawer>,
    Res<'w, SafeBottom>,
    Res<'w, super::TouchState>,
    Res<'w, super::Layout>,
    Res<'w, super::modal::Capture>,
    Option<Res<'w, super::hotbar::Preferences>>,
    Option<Res<'w, nvf_play::pad::PlatformInputGate>>,
    Res<'w, super::practice::Practice>,
    Option<Res<'w,nvf_play::touch_safe_area::SafeInsets>>,
    Option<Res<'w,UiScale>>,
);

type HintFilter = (With<Hint>, Without<Handle>);

fn visual(
    (options, drawer, safe, state, layout, modal, hotbar, platform, practice, insets, ui): VisualState<'_>,
    capture: Option<Res<nvf_play::bindings::KeyboardCapture>>,
    bindings: Option<Res<nvf_play::bindings::Bindings>>,
    pads: Query<&Gamepad>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut handle_nodes: Query<(&mut Node, &mut Visibility), HandleFilter>,
    mut hints: Query<(&mut Text, &mut Visibility, &mut TextFont), HintFilter>,
) {
    let Ok(window) = windows.single() else { return };
    let size = Vec2::new(window.width(), window.height());
    let rect = opening_area(size, safe.0, insets.as_deref().copied().unwrap_or_default());
    let scale=ui.as_deref().map_or(1.0,|s|s.0);
    let valid_scale=scale.is_finite() && scale>0.0 && (size/scale).is_finite();
    let scale=if valid_scale {scale} else {1.0};
    let controller = bindings
        .is_some_and(|b| b.device == nvf_play::bindings::InputDevice::Controller)
        && !pads.is_empty();
    let show = valid_scale && options.enabled
        && (options.destination == Destination::Inventory
            || hotbar.as_ref().is_some_and(|p| p.tray_available()))
        // A consumed controller-return touch restores visible controls immediately.
        // Input admission remains gated independently until the next fresh contact.
        && platform.as_ref().is_none_or(|gate| gate.allows_actions())
        && !practice.blocking()
        && !state.ui_capture
        && !modal.blocking()
        && !controller
        && state.playing
        && window.focused
        && drawer.focused != Some(false)
        && !state.suspended
        && layout.editor.is_none()
        && !capture.is_some_and(|c| c.0)
        && rect.min.y >= 0.0 && rect.min.is_finite() && rect.max.is_finite() && rect.size().min_element()>0.0;
    for (mut node, mut visible) in &mut handle_nodes {
        *visible = if show {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        node.left = px(rect.min.x/scale);
        node.top = px((rect.min.y - drawer.progress().min(options.threshold * 1.25))/scale);
        node.width = px(rect.width()/scale);
        node.height = px(rect.height()/scale);
    }
    for (mut text, mut visible, mut font) in &mut hints {
        font.font_size=FontSize::Px(12.0/scale);
        text.0 = match options.destination {
            Destination::Inventory => "Pip-Boy",
            Destination::UtilityTray => "Utility tray",
        }
        .into();
        *visible = if layout.config.hints {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

#[cfg(any(target_os = "android", test))]
fn insets(
    common: Res<nvf_play::touch_safe_area::SafeInsets>,
    mut safe: ResMut<SafeBottom>,
) {
    if common.bottom.is_finite() {
        safe.0 = common.bottom.clamp(0.0, 512.0);
    }
}

#[cfg(any(target_os = "android", test))]
fn install_insets(app: &mut App) {
    app.init_resource::<nvf_play::touch_safe_area::SafeInsets>()
        .init_resource::<SafeBottom>()
        .add_systems(
            PreUpdate,
            insets
                .in_set(PublishSafeBottom)
                .after(nvf_play::touch_safe_area::Publish)
                .before(super::TouchInputSystems),
        );
}

#[cfg(test)]
mod common_inset_tests {
    use super::*;
    use nvf_play::touch_safe_area::{Publish, SafeInsets};

    #[derive(Resource, Default)]
    struct NextBottom(f32);
    #[derive(Resource, Default)]
    struct Seen(Vec<f32>);

    fn publish(next: Res<NextBottom>, mut common: ResMut<SafeInsets>) {
        common.bottom = next.0;
    }
    fn raw_consumer(bottom: Res<SafeBottom>, mut seen: ResMut<Seen>) {
        seen.0.push(bottom.0);
    }

    #[test]
    fn common_snapshot_reaches_bottom_handle_before_same_frame_raw_input() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<NextBottom>()
            .init_resource::<Seen>();
        install_insets(&mut app);
        app.add_systems(PreUpdate, publish.in_set(Publish))
            .add_systems(PreUpdate, raw_consumer.in_set(super::super::TouchInputSystems));
        assert_eq!(app.world().resource::<SafeBottom>().0, 24.0);
        for (published, expected) in [(0.0, 0.0), (32.0, 32.0), (8.0, 8.0), (f32::NAN, 8.0), (700.0, 512.0)] {
            app.world_mut().resource_mut::<NextBottom>().0 = published;
            app.update();
            assert_eq!(app.world().resource::<Seen>().0.last(), Some(&expected));
            let bounds = handle(Vec2::new(792.0, 800.0), expected);
            assert_eq!(bounds.center().y, 800.0 - expected - 36.0);
            assert!(bounds.max.y < 800.0 - expected);
        }
    }
}

fn set_destination(world: &mut World, name: &str) -> bool {
    let Some(destination) = Destination::named(name) else {
        return false;
    };
    world.resource_mut::<Drawer>().cancel();
    world.resource_mut::<Options>().destination = destination;
    true
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Options>()
        .init_resource::<Drawer>()
        .init_resource::<SafeBottom>()
        .add_message::<nvf_play::menu::OpenInventory>()
        .add_message::<super::hotbar::UtilityTrayRequest>()
        .add_systems(Startup, spawn)
        .add_systems(Update, visual);
    #[cfg(target_os = "android")]
    install_insets(app);
    settings::add_setting(app, Setting {
        key: "touch-pipboy-swipe-v1", group: "Controller", label: "Bottom swipe handle",
        note: "Show a bottom swipe band. Drag up to open the chosen target; controls and system gestures keep priority.",
        switches: "",
        get: |w| w.resource::<Options>().enabled, set: |w,v| w.resource_mut::<Options>().enabled = v, slider: None,
    });
    settings::add_choice_list(app, Setting {
        key: "touch-bottom-swipe-target-v1", group: "Controls", label: "Bottom swipe opens",
        note: "Choose Inventory or Utility tray for the same bottom handle. Utility tray needs Touch hotbar and Pinned consumable tray enabled.",
        switches: "",
        get: |_| false, set: |_,_| {}, slider: None,
    }, ChoiceList {
        names: |_| vec!["Inventory".into(), "Utility tray".into()],
        chosen: |w| w.resource::<Options>().destination.name().into(),
        pick: set_destination,
        save: |w| w.resource::<Options>().destination.name().into(),
        load: set_destination,
        ask_keep: false,
    });
    settings::set_choice_unavailable(app, "touch-bottom-swipe-target-v1", |w, name| {
        (name == "Utility tray"
            && !w
                .get_resource::<super::hotbar::Preferences>()
                .is_some_and(|p| p.tray_available()))
        .then(|| "Enable Touch hotbar and Pinned consumable tray first.".into())
    });
    settings::add_setting(
        app,
        Setting::slider(
            "touch-pipboy-distance-v1",
            "Controller",
            "Bottom swipe distance",
            "Upward distance needed before releasing the bottom handle opens the chosen destination.",
            Slider::choice(
                39,
                |w| (w.resource::<Options>().threshold - 48.0) / 152.0,
                |w, v| {
                    w.resource_mut::<Options>().threshold =
                        48.0 + (v.clamp(0.0, 1.0) * 38.0).round() * 4.0
                },
                |v| format!("{:.0} dp", 48.0 + (v * 38.0).round() * 4.0),
            ),
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(id: u64, phase: TouchPhase, position: Vec2) -> TouchInput {
        TouchInput {
            id,
            phase,
            position,
            window: Entity::PLACEHOLDER,
            force: None,
        }
    }
    #[test]
    fn a_handle_swipe_commits_once_only_after_release() {
        let size = Vec2::new(900., 450.);
        let area = handle(size, 24.);
        let start = area.center();
        let mut d = Drawer::default();
        assert_eq!(
            d.event(&event(1, TouchPhase::Started, start), size, area, 96., true),
            (true, false)
        );
        assert_eq!(
            d.event(
                &event(1, TouchPhase::Moved, start - Vec2::Y * 120.),
                size,
                area,
                96.,
                true
            ),
            (true, false)
        );
        assert_eq!(
            d.event(
                &event(1, TouchPhase::Ended, start - Vec2::Y * 120.),
                size,
                area,
                96.,
                true
            ),
            (true, true)
        );
        assert_eq!(
            d.event(
                &event(1, TouchPhase::Ended, start - Vec2::Y * 120.),
                size,
                area,
                96.,
                true
            ),
            (false, false)
        );
    }
    #[test]
    fn look_control_system_edge_and_short_taps_do_not_open() {
        let size = Vec2::new(900., 450.);
        let area = handle(size, 32.);
        let mut d = Drawer::default();
        for start in [Vec2::new(450., 449.), Vec2::new(700., 300.)] {
            assert_eq!(
                d.event(&event(1, TouchPhase::Started, start), size, area, 96., true),
                (false, false)
            );
        }
        let start = area.center();
        assert_eq!(
            d.event(
                &event(1, TouchPhase::Started, start),
                size,
                area,
                96.,
                false
            ),
            (false, false)
        );
        d.event(&event(1, TouchPhase::Started, start), size, area, 96., true);
        assert_eq!(
            d.event(
                &event(1, TouchPhase::Ended, start - Vec2::Y * 30.),
                size,
                area,
                96.,
                true
            ),
            (true, false)
        );
        assert!(area.max.y <= size.y - 32.);
    }
    #[test]
    fn horizontal_intent_cancel_resize_and_second_finger_never_open() {
        let size = Vec2::new(900., 450.);
        let area = handle(size, 24.);
        let start = area.center();
        let mut d = Drawer::default();
        d.event(&event(1, TouchPhase::Started, start), size, area, 96., true);
        assert_eq!(
            d.event(&event(2, TouchPhase::Started, start), size, area, 96., true),
            (true, false)
        );
        assert_eq!(
            d.event(
                &event(2, TouchPhase::Ended, start - Vec2::Y * 150.),
                size,
                area,
                96.,
                true
            ),
            (true, false)
        );
        d.event(
            &event(1, TouchPhase::Moved, start + Vec2::X * 40.),
            size,
            area,
            96.,
            true,
        );
        assert_eq!(
            d.event(
                &event(1, TouchPhase::Ended, start - Vec2::Y * 150.),
                size,
                area,
                96.,
                true
            ),
            (true, false)
        );
        d.event(&event(1, TouchPhase::Started, start), size, area, 96., true);
        assert_eq!(
            d.event(
                &event(1, TouchPhase::Canceled, start - Vec2::Y * 150.),
                size,
                area,
                96.,
                true
            ),
            (true, false)
        );
        d.event(&event(1, TouchPhase::Started, start), size, area, 96., true);
        assert_eq!(
            d.event(
                &event(1, TouchPhase::Ended, start - Vec2::Y * 150.),
                size + Vec2::X,
                area,
                96.,
                true
            ),
            (true, false)
        );
    }

    #[test]
    fn destination_defaults_legacy_and_roundtrips_ini_without_changing_enable_or_distance() {
        fn app(path: &std::path::Path) -> App {
            let mut app = App::new();
            install(&mut app);
            super::super::hotbar::install(&mut app);
            app.insert_resource(settings::SettingsFile(Some(path.to_path_buf())));
            app
        }
        let dir = std::env::temp_dir().join(format!("nvf-drawer-target-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("settings.ini");
        std::fs::write(&path, "[Other]\nkeep=unchanged\n").unwrap();
        let mut first = app(&path);
        settings::apply_stored(first.world_mut());
        assert_eq!(
            first.world().resource::<Options>().destination,
            Destination::Inventory
        );
        assert!(!first.world().resource::<Options>().enabled);
        assert_eq!(first.world().resource::<Options>().threshold, 96.);
        let row = first
            .world()
            .resource::<settings::Settings>()
            .index("touch-hotbar")
            .unwrap();
        assert_eq!(settings::flip(first.world_mut(), row), Some(true));
        let row = first
            .world()
            .resource::<settings::Settings>()
            .index("touch-bottom-swipe-target-v1")
            .unwrap();
        assert!(settings::pick(first.world_mut(), row, "Utility tray"));
        let mut back = app(&path);
        settings::apply_stored(back.world_mut());
        assert_eq!(
            back.world().resource::<Options>().destination,
            Destination::UtilityTray
        );
        assert!(
            !back.world().resource::<Options>().enabled,
            "target choice must not enable legacy switch"
        );
        assert_eq!(back.world().resource::<Options>().threshold, 96.);
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            fnv_install::ini_get(&text, "Other", "keep").as_deref(),
            Some("unchanged")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn swipe_setting_notes_fit_two_line_budget() {
        let mut app = App::new();
        app.init_resource::<settings::Settings>();
        install(&mut app);
        super::super::pipboy_drag::install(&mut app);
        let list = app.world().resource::<settings::Settings>();
        for key in ["touch-pipboy-swipe-v1", "touch-direct-pipboy-v1"] {
            let setting = &list.list()[list.index(key).unwrap()];
            assert!(
                settings::note_over_budget(setting.note).is_none(),
                "{key}: {:?}",
                settings::note_over_budget(setting.note)
            );
            assert!(
                setting.note.chars().count() <= 120,
                "{key} should fit cleanly within two lines: {} chars",
                setting.note.chars().count()
            );
        }
    }
}
