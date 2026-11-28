//! Touch movement/action choices. Native controls remain authoritative.
use super::{
    icons,
    layout::{Layout, Target},
    Role, TouchState,
};
use bevy::{ecs::system::SystemParam, prelude::*, window::PrimaryWindow};
use nvf_play::{
    control::PlayerControl,
    settings::{self, Setting},
    touch_movement::TouchMovementInput,
};

#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Options {
    pub analog: bool,
    pub fire_drag: bool,
    pub run_lock: bool,
    pub vats_taps: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self::for_platform(cfg!(target_os = "android"))
    }
}
impl Options {
    pub(super) fn for_platform(android: bool) -> Self {
        Self {
            analog: true,
            fire_drag: true,
            run_lock: android,
            vats_taps: android,
        }
    }
    #[cfg(test)]
    pub(super) fn legacy() -> Self {
        Self {
            analog: false,
            fire_drag: false,
            run_lock: false,
            vats_taps: false,
        }
    }
}

#[derive(SystemParam)]
pub(super) struct Bridge<'w> {
    options: Option<Res<'w, Options>>,
    sample: Option<ResMut<'w, TouchMovementInput>>,
    real: Res<'w, Time<Real>>,
    control: Option<Res<'w, PlayerControl>>,
}
impl Bridge<'_> {
    pub fn options(&self) -> Options {
        self.options.as_deref().copied().unwrap_or_default()
    }
    pub fn now(&self) -> f64 {
        self.real.elapsed_secs_f64()
    }
    pub fn auto_move(&self) -> bool {
        self.control.as_ref().is_some_and(|c| c.auto_move)
    }
    pub fn clear_sample(&mut self) -> Vec2 {
        let previous = self.sample.as_deref().map_or(Vec2::ZERO, |s| s.axes);
        if let Some(sample) = self.sample.as_deref_mut() {
            *sample = TouchMovementInput::default();
        }
        previous
    }
    pub fn publish(&mut self, axes: Vec2, previous: Vec2) {
        if let Some(sample) = self.sample.as_deref_mut() {
            sample.axes = axes;
            sample.previous_axes = previous;
        }
    }
}

pub(super) fn axes(delta_pixels: Vec2, radius: f32, dead_zone: f32) -> Vec2 {
    if !delta_pixels.is_finite() || !radius.is_finite() || radius <= 0.0 {
        return Vec2::ZERO;
    }
    let raw = delta_pixels / radius;
    let length = raw.length();
    let dead = dead_zone.clamp(0.0, 0.95);
    if length <= dead {
        Vec2::ZERO
    } else {
        let amount = ((length.min(1.0) - dead) / (1.0 - dead)).clamp(0.0, 1.0);
        raw / length * Vec2::new(1.0, -1.0) * amount
    }
}

#[derive(Default)]
pub(super) struct Taps(Option<(f64, Vec2)>);
impl Taps {
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }
    pub fn clear(&mut self) {
        self.0 = None;
    }
    pub fn tap(&mut self, now: f64, point: Vec2) -> bool {
        let matched = self.0.is_some_and(|(last, at)| {
            now >= last && now - last <= 0.35 && point.distance(at) <= 40.0
        });
        self.0 = (!matched).then_some((now, point));
        matched
    }
}

pub(super) fn lock_rect(layout: &Layout, state: &TouchState, size: Vec2) -> Rect {
    if !size.is_finite() || size.min_element() < 64.0 {
        return Rect::default();
    }
    let origin = state
        .contacts
        .values()
        .find(|c| matches!(c.role, Role::Move))
        .map_or_else(
            || layout.config.center(Target::Stick, size),
            |c| c.origin * size,
        );
    lock_rect_at(layout, size, origin)
}
pub(super) fn fixed_lock_rect(layout: &Layout, size: Vec2) -> Rect {
    if !size.is_finite() || size.min_element() < 64.0 {
        return Rect::default();
    }
    lock_rect_at(layout, size, layout.config.center(Target::Stick, size))
}
fn lock_rect_at(layout: &Layout, size: Vec2, origin: Vec2) -> Rect {
    let radius = layout.config.diameter(Target::Stick, size) * 0.5;
    let center =
        (origin - Vec2::Y * radius * 1.95).clamp(Vec2::splat(32.0), size - Vec2::splat(32.0));
    Rect::from_center_size(center, Vec2::splat(56.0))
}

pub(super) fn lock_available(layout: &Layout, state: &TouchState, size: Vec2) -> bool {
    if !size.is_finite() || size.min_element() < 64.0 {
        return false;
    }
    let rect = lock_rect(layout, state, size);
    !super::BUTTONS.iter().enumerate().any(|(i, b)| {
        b.playing && layout.config.buttons[i].visible && {
            let center = layout.config.center(Target::Button(i), size);
            center.distance(center.clamp(rect.min, rect.max))
                < layout.config.diameter(Target::Button(i), size) * 0.5
        }
    })
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Options>()
        .init_resource::<TouchMovementInput>()
        .add_systems(Startup, spawn)
        .add_systems(Update, show);
    settings::add_setting(app, Setting { key: "touch-analog-movement-v1", group: "Touch", label: "Analog movement",
        note: "On: stick distance scales native movement speed; small pushes walk and larger pushes run with Always Run. Off: legacy directional keys. Desktop/controller behavior is unchanged.",
        switches: "",
        get: |w| w.resource::<Options>().analog, set: |w,v| w.resource_mut::<Options>().analog=v, slider: None });
    settings::add_setting(app, Setting { key: "touch-fire-drag-look-v1", group: "Touch", label: "Drag Fire to look",
        note: "Keep firing through the Use binding while dragging the Fire finger to turn. One finger owns look at a time. Off keeps the stationary Fire button.",
        switches: "",
        get: |w| w.resource::<Options>().fire_drag, set: |w,v| w.resource_mut::<Options>().fire_drag=v, slider: None });
    settings::add_setting(app, Setting { key: "touch-auto-move-lock-v1", group: "Touch", label: "Auto-move lock target",
        note: "Show an arrow above the stick: drag the movement finger onto it and release to engage native Auto Move; tap to toggle it. Uses your binding and normal walk/run rules; no sprint or AP change.",
        switches: "",
        get: |w| w.resource::<Options>().run_lock, set: |w,v| w.resource_mut::<Options>().run_lock=v, slider: None });
    settings::add_setting(app, Setting { key: "touch-vats-double-tap-v1", group: "Touch", label: "Double-tap look space for V.A.T.S.",
        note: "Two short stationary taps on empty look space, with no other fingers down, pulse your V.A.T.S. binding. Drags/cancellation do not. Native weapon, target and AP rules decide the result.",
        switches: "",
        get: |w| w.resource::<Options>().vats_taps, set: |w,v| w.resource_mut::<Options>().vats_taps=v, slider: None });
}

#[derive(Component)]
pub(super) struct LockMarker;
fn spawn(mut commands: Commands) {
    commands
        .spawn((
            LockMarker,
            Node {
                position_type: PositionType::Absolute,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.06, 0.07, 0.75)),
            Visibility::Hidden,
            GlobalZIndex(2050),
            Pickable::IGNORE,
        ))
        .with_children(|p| {
            // The icon alone marks the target; Controls help explains it.
            p.spawn((
                ImageNode::default(),
                icons::TouchIcon(9),
                Node {
                    width: percent(65.0),
                    height: percent(65.0),
                    ..default()
                },
                Pickable::IGNORE,
            ));
        });
}
fn show(
    options: Res<Options>,
    state: Res<TouchState>,
    layout: Res<Layout>,
    occlusion: Option<Res<super::hotbar::Occlusion>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut markers: Query<(&mut Node, &mut Visibility), With<LockMarker>>,
) {
    let size = window
        .single()
        .ok()
        .map(|w| Vec2::new(w.width(), w.height()));
    for (mut node, mut visibility) in &mut markers {
        let visible = options.run_lock
            && state.interactive_actions
            && !state.ui_capture
            && layout.editor.is_none()
            && size.is_some_and(|size| {
                lock_available(&layout, &state, size)
                    && occlusion
                        .as_ref()
                        .is_none_or(|bar| !bar.blocks(lock_rect(&layout, &state, size)))
            });
        *visibility = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if let Some(size) = size {
            let rect = lock_rect(&layout, &state, size);
            node.left = px(rect.min.x);
            node.top = px(rect.min.y);
            node.width = px(rect.width());
            node.height = px(rect.height());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_touch_settings_round_trip_all_schemes_without_rewriting_other_sections() {
        let file =
            std::env::temp_dir().join(format!("nvf-touch-movement-{}.ini", uuid::Uuid::new_v4()));
        std::fs::write(&file, "[Unrelated]\nKeep=yes\n").unwrap();
        let mut app = App::new();
        install(&mut app);
        app.insert_resource(settings::SettingsFile(Some(file.clone())));
        for key in [
            "touch-analog-movement-v1",
            "touch-fire-drag-look-v1",
            "touch-auto-move-lock-v1",
            "touch-vats-double-tap-v1",
        ] {
            let index = app
                .world()
                .resource::<settings::Settings>()
                .index(key)
                .unwrap();
            assert!(settings::flip(app.world_mut(), index).is_some());
        }
        let expected = *app.world().resource::<Options>();
        assert_eq!(
            expected,
            Options {
                analog: false,
                fire_drag: false,
                run_lock: true,
                vats_taps: true
            }
        );
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("Keep=yes"));
        let mut restored = App::new();
        install(&mut restored);
        restored.insert_resource(settings::SettingsFile(Some(file.clone())));
        settings::apply_stored(restored.world_mut());
        assert_eq!(*restored.world().resource::<Options>(), expected);
        std::fs::remove_file(file).unwrap();
    }

    #[test]
    fn auto_move_target_keeps_its_icon_without_a_text_label() {
        let mut app = App::new();
        app.add_systems(Startup, spawn);
        app.update();
        let world = app.world_mut();
        let marker = world.query_filtered::<Entity, With<LockMarker>>().single(world).unwrap();
        let children: Vec<Entity> = world.get::<Children>(marker).unwrap().iter().collect();
        assert_eq!(children.len(), 1, "only the arrow icon");
        assert!(world.get::<icons::TouchIcon>(children[0]).is_some());
        assert!(world.query::<&Text>().iter(world).next().is_none(), "no visible Auto move text");
    }

    #[test]
    fn android_defaults_enable_run_lock_and_vats_double_tap_while_desktop_keeps_off() {
        let android = Options::for_platform(true);
        assert!(android.run_lock && android.vats_taps && android.analog && android.fire_drag);
        let desktop = Options::for_platform(false);
        assert!(!desktop.run_lock && !desktop.vats_taps && desktop.analog && desktop.fire_drag);
    }

    #[test]
    fn auto_move_lock_target_sits_cleanly_above_stick_and_clear_of_middle_left_gyro() {
        let layout = Layout::default();
        let state = TouchState::default();
        for size in [Vec2::new(792.0, 363.0), Vec2::new(880.0, 396.0), Vec2::new(1280.0, 720.0)] {
            let stick_center = layout.config.center(Target::Stick, size);
            let stick_top = stick_center.y - layout.config.diameter(Target::Stick, size) * 0.5;
            let rect = fixed_lock_rect(&layout, size);
            assert!(rect.max.y <= stick_top - 12.0, "lock target {rect:?} sits cleanly above stick top {stick_top} at {size}");
            assert!(lock_available(&layout, &state, size), "lock target clear of default buttons incl. Gyro at {size}");
        }
    }
}
