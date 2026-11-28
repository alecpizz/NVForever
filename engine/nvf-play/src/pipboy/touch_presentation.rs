//! Independent device/flat presentation size; input follows the actual geometry.
use bevy::prelude::*;

/// Optional wrapper presentation request. No resource means the unchanged fit.
/// This reserves real flat-screen space, not an overlay over selectable content.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DismissalSpace { left: bool, right: bool, close: bool }
impl DismissalSpace {
    pub fn new(left: bool, right: bool, close: bool) -> Self { Self {left,right,close} }
    pub(super) fn padding(self, safe: crate::touch_safe_area::SafeInsets, window:Option<Vec2>) -> crate::touch_safe_area::SafeInsets {
        // 8 outer + 48 target + 8 inner; actual system insets remain additive.
        let mut padded=safe;
        if self.left { padded.left += 64.; }
        if self.right { padded.right += 64.; }
        if self.close { padded.bottom += 64.; }
        // A tiny window must retain its standard canvas rather than go blank.
        if window.is_some_and(|size|(size-Vec2::new(padded.left+padded.right,padded.top+padded.bottom)).min_element()>=96.) {padded} else {safe}
    }
}

const MIN: f32 = 0.85;
const MAX: f32 = 1.35;
pub const KEY: &str = "touch-pipboy-size-v1";

/// Actual flat picture dimensions in viewport-height percent. Fit both axes
/// together so the full canvas remains visible and its inverse stays affine.
pub(super) fn flat_dimensions(zoom: f32, window: Option<Vec2>, safe: crate::touch_safe_area::SafeInsets) -> Vec2 {
    let height = super::SCREEN_IMAGE_H * zoom * crate::interface::unit_scale() / 9.6;
    let size = Vec2::new(height * super::SCREEN_CROP.x / super::SCREEN_CROP.y, height);
    let fit = window.filter(|s| s.is_finite() && s.min_element() > 0.0)
        .map_or(1.0, |s| {
            let inner = (s - Vec2::new(safe.left + safe.right, safe.top + safe.bottom)).max(Vec2::ONE);
            (92.0 * inner.y / s.y / size.y).min(96.0 * inner.x / s.y / size.x).min(1.0)
        });
    size * fit
}

#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct PipBoySize {
    amount: f32,
}
impl Default for PipBoySize {
    fn default() -> Self {
        Self::for_platform(cfg!(target_os = "android"))
    }
}
impl PipBoySize {
    pub fn for_platform(android: bool) -> Self {
        Self { amount: if android { 1.10 } else { 1.0 } }
    }
    pub fn ratio(&self) -> f32 {
        if self.amount.is_finite() { self.amount.clamp(MIN, MAX) } else { 1.0 }
    }
    fn part(&self) -> f32 { (self.ratio() - MIN) / (MAX - MIN) }
    fn set_part(&mut self, part: f32) {
        if part.is_finite() { self.amount = MIN + part.clamp(0.0, 1.0) * (MAX - MIN); }
    }
    /// Preserve the exact old FOV at 100%, otherwise magnify the existing lens.
    pub fn device_fov(&self, degrees: f32) -> f32 {
        if self.ratio() == 1.0 { degrees }
        else { (2.0 * ((degrees.to_radians() * 0.5).tan() / self.ratio()).atan()).to_degrees() }
    }
}

pub(super) fn install(app: &mut App) {
    install_for_platform(app, cfg!(target_os = "android"));
}
fn install_for_platform(app: &mut App, android: bool) {
    if !app.world().contains_resource::<PipBoySize>() {
        app.insert_resource(PipBoySize::for_platform(android));
    }
    app.add_systems(PreUpdate, cancel_changed
        .after(crate::touch_safe_area::Publish)
        .before(super::TouchPointerSet));
    if !android { return; }
    use crate::settings::{add_setting, Setting, Slider};
    add_setting(app, Setting {
        key: KEY, group: crate::pause::GROUP_MENUS, label: "Pip-Boy size",
        note: "Device and screen size (85-135%), separate from HUD and other menus. Touch follows the resized view; changing size cancels a pending item gesture.",
        switches: "", get: |_| false, set: |_, _| {},
        slider: Some(Slider {
            get: |w| w.resource::<PipBoySize>().part(),
            set: |w, value| w.resource_mut::<PipBoySize>().set_part(value),
            min: 0.0, step: 0.1,
            shown: Some(|value| format!("{:.0}%", (MIN + value.clamp(0.0, 1.0) * (MAX - MIN)) * 100.0)),
        }),
    });
}
/// Geometry known before the pointer driver, rather than the preceding frame's
/// ComputedNode. The node's own flat_geometry check remains an additional guard.
#[derive(Clone, Copy, PartialEq)]
struct PresentationStamp {
    logical: Vec2,
    physical: UVec2,
    density: f32,
    safe: crate::touch_safe_area::SafeInsets,
    size: f32,
    menu_scale: f32,
    ui_scale: f32,
}
fn cancel_changed(
    size: Res<PipBoySize>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    safe: Option<Res<crate::touch_safe_area::SafeInsets>>,
    ui: Option<Res<UiScale>>,
    touch: Option<ResMut<super::touch_items::TouchItems>>,
    dismissal: Option<Res<DismissalSpace>>,
    mut old: Local<Option<Option<PresentationStamp>>>,
) {
    let next = windows.single().ok().and_then(|window| {
        let logical = window.resolution.size();
        let physical = UVec2::new(window.physical_width(), window.physical_height());
        let density = window.scale_factor();
        let safe = safe.as_deref().copied().unwrap_or_default();
        let safe = dismissal.as_deref().copied().unwrap_or_default().padding(safe,Some(logical));
        let menu_scale = crate::interface::unit_scale();
        let ui_scale = ui.as_deref().map_or(1.0, |scale| scale.0);
        if !logical.is_finite() || logical.min_element() <= 0.0 || physical.min_element() == 0
            || !density.is_finite() || density <= 0.0
            || !menu_scale.is_finite() || menu_scale <= 0.0
            || !ui_scale.is_finite() || ui_scale <= 0.0
            || crate::touch_safe_area::SafeInsets::from_array([safe.left, safe.top, safe.right, safe.bottom]).is_none()
            || safe.left + safe.right >= logical.x || safe.top + safe.bottom >= logical.y
        { return None; }
        Some(PresentationStamp { logical, physical, density, safe, size: size.ratio(), menu_scale, ui_scale })
    });
    // Invalid/missing metadata never admits an old projected point. In contrast,
    // the first valid observation does not consume a user's first fresh touch.
    if next.is_none() || old.is_some_and(|previous| previous != next) {
        if let Some(mut touch) = touch { touch.cancel(); }
    }
    *old = Some(next);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings;
    use bevy::input::touch::{TouchInput, TouchPhase};

    fn scene() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<super::super::touch_items::TouchItems>()
            .init_resource::<ButtonInput<MouseButton>>()
            .insert_resource(UiScale(1.25));
        crate::menu_scale::register(&mut app, "Display");
        install_for_platform(&mut app, true);
        app
    }
    fn set(app: &mut App, part: f32) {
        let index = app.world().resource::<settings::Settings>().index(KEY).unwrap();
        settings::set_slider(app.world_mut(), index, part).unwrap();
    }

    #[test]
    fn real_settings_roundtrip_preserves_independent_hud_and_menu_sizes() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("nvf-pip-size-{}-{nonce}.ini", std::process::id()));
        // Keep the real stored HUD preference without changing its global atomic
        // in a parallel test process. Bevy's active UiScale is isolated below.
        std::fs::write(&path, "[Settings]\nui-scale=0.75\n[Other]\nkeep=yes\n").unwrap();
        let mut app = scene();
        assert!((app.world().resource::<PipBoySize>().ratio() - 1.1).abs() < 1e-6);
        let before = (app.world().resource::<UiScale>().0, app.world().resource::<crate::menu_scale::MenuScale>().scale());
        app.insert_resource(settings::SettingsFile(Some(path.clone())));
        set(&mut app, 1.0);
        let stored = std::fs::read_to_string(&path).unwrap();
        assert!(stored.contains(KEY));
        assert_eq!(fnv_install::ini_get(&stored, settings::SECTION, "ui-scale").as_deref(), Some("0.75"));
        assert_eq!(fnv_install::ini_get(&stored, "Other", "keep").as_deref(), Some("yes"));
        assert_eq!(before, (app.world().resource::<UiScale>().0, app.world().resource::<crate::menu_scale::MenuScale>().scale()));
        let mut loaded = scene();
        loaded.insert_resource(settings::SettingsFile(Some(path.clone())));
        settings::apply_stored(loaded.world_mut());
        assert!((loaded.world().resource::<PipBoySize>().ratio() - MAX).abs() < 1e-6);
        assert_eq!(before, (loaded.world().resource::<UiScale>().0, loaded.world().resource::<crate::menu_scale::MenuScale>().scale()));
        assert_eq!(PipBoySize::for_platform(false).device_fov(47.0).to_bits(), 47_f32.to_bits());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn live_size_change_cancels_pending_projected_contact_without_clearing_physical_input() {
        let mut app = scene();
        app.update();
        let event = |phase| TouchInput { phase, position: Vec2::new(80.0, 90.0), window: Entity::PLACEHOLDER, id: 17, force: None };
        assert!(app.world_mut().resource_mut::<super::super::touch_items::TouchItems>().event(&event(TouchPhase::Started), 1));
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        set(&mut app, 0.0);
        app.update();
        let touch = &mut *app.world_mut().resource_mut::<super::super::touch_items::TouchItems>();
        assert!(!touch.busy());
        assert_eq!(touch.contact_id(), None);
        assert!(touch.take_cancel());
        assert!(touch.take_projected().is_none());
        assert!(!touch.event(&event(TouchPhase::Moved), 2));
        assert!(!touch.event(&event(TouchPhase::Ended), 3));
        assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    }

    #[derive(Resource, Default)]
    struct PendingInsets(Option<crate::touch_safe_area::SafeInsets>);
    fn publish_insets(mut pending: ResMut<PendingInsets>, mut safe: ResMut<crate::touch_safe_area::SafeInsets>) {
        if let Some(next) = pending.0.take() { *safe = next; }
    }
    fn pointer_scene() -> App {
        use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
        use bevy::asset::{AssetApp, AssetPlugin};
        use bevy::camera::{ComputedCameraValues, RenderTargetInfo, visibility::VisibilityPlugin};
        use bevy::ui::{ComputedUiRenderTargetInfo, ui_layout_system, ui_surface::UiSurface, update::propagate_ui_target_cameras};
        let mut app = scene();
        app.insert_resource(UiScale(1.0));
        app.add_plugins((AssetPlugin::default(), bevy::text::TextPlugin, bevy::transform::TransformPlugin))
            .init_asset::<Mesh>()
            .init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>()
            .add_plugins(VisibilityPlugin)
            .add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate))
            .add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate))
            .init_resource::<UiSurface>()
            .init_resource::<crate::touch_safe_area::SafeInsets>()
            .init_resource::<PendingInsets>()
            .init_resource::<bevy::picking::pointer::PointerMap>()
            .init_resource::<super::super::PipBoyAid>()
            .init_resource::<super::super::PointerMoved>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_message::<bevy::picking::pointer::PointerInput>()
            .add_message::<bevy::window::WindowEvent>()
            .add_systems(PreUpdate, publish_insets.in_set(crate::touch_safe_area::Publish))
            .add_systems(PreUpdate, super::super::drive_pointer.in_set(super::super::TouchPointerSet))
            .add_systems(Update, (super::super::flat_pointer, super::super::flat_screen).chain())
            .add_systems(PostUpdate, (propagate_ui_target_cameras, ui_layout_system).chain())
            .configure_sets(PostUpdate, PropagateSet::<ComputedUiTargetCamera>::default().after(propagate_ui_target_cameras).before(ui_layout_system))
            .configure_sets(PostUpdate, PropagateSet::<ComputedUiRenderTargetInfo>::default().after(propagate_ui_target_cameras).before(ui_layout_system));
        app.insert_resource(super::super::PipBoy { phase: super::super::Phase::Up, frame: super::super::Frame::Screen, ..default() });
        let camera = app.world_mut().spawn((Camera2d, Camera { computed: ComputedCameraValues { target_info: Some(RenderTargetInfo { physical_size: UVec2::new(1400, 800), scale_factor: 1.0 }), ..default() }, ..default() })).id();
        app.world_mut().spawn((Window { resolution: bevy::window::WindowResolution::new(1400, 800), ..default() }, bevy::window::PrimaryWindow));
        app.insert_resource(super::super::PipBoyScreen { image: Handle::default(), camera, on_model: false, pointer: None, scale: 1.0 });
        // Paint real geometry without observing the PreUpdate stamp yet. This
        // keeps the first-valid input control honest about node readiness.
        {
            use bevy::ecs::system::RunSystemOnce;
            app.world_mut().run_system_once(super::super::flat_screen).unwrap();
            app.world_mut().run_schedule(PostUpdate);
        }
        app
    }
    fn projected_contact(app: &mut App, id: u64) {
        let logical = {
            let mut query = app.world_mut().query_filtered::<(&ComputedNode, &UiGlobalTransform), With<super::super::FlatScreen>>();
            let (node, transform) = query.single(app.world()).unwrap();
            let physical = transform.translation - node.size() * 0.5 + node.size() * Vec2::new(0.4, 0.6);
            let mut windows = app.world_mut().query_filtered::<&Window, With<bevy::window::PrimaryWindow>>();
            physical / windows.single(app.world()).unwrap().scale_factor()
        };
        let mut touch = app.world_mut().resource_mut::<super::super::touch_items::TouchItems>();
        for (phase, time) in [(TouchPhase::Started, 1), (TouchPhase::Ended, 10)] {
            assert!(touch.event(&TouchInput { phase, position: logical, window: Entity::PLACEHOLDER, id, force: None }, time));
        }
    }
    fn pointer_events(app: &mut App) -> Vec<bevy::picking::pointer::PointerInput> {
        app.world_mut().resource_mut::<Messages<bevy::picking::pointer::PointerInput>>().drain().collect()
    }

    #[derive(Resource, Default)]
    struct FreshContact(Option<(u64, Vec2)>);
    fn fresh_contact_after_invalidation(mut fresh: ResMut<FreshContact>, mut touch: ResMut<super::super::touch_items::TouchItems>) {
        if let Some((id, position)) = fresh.0.take() {
            for (phase, now) in [(TouchPhase::Started, 1), (TouchPhase::Ended, 10)] {
                assert!(touch.event(&TouchInput { phase, position, window: Entity::PLACEHOLDER, id, force: None }, now));
            }
        }
    }

    #[test]
    fn fresh_start_after_inset_invalidation_cannot_project_through_the_old_flat_node() {
        use bevy::picking::pointer::PointerAction;
        let _units = crate::interface::units_at(1.0);
        let mut app = pointer_scene();
        app.init_resource::<FreshContact>().add_systems(PreUpdate, fresh_contact_after_invalidation
            .after(cancel_changed).before(super::super::TouchPointerSet));
        for _ in 0..4 { app.update(); }
        let before = {
            let mut query = app.world_mut().query_filtered::<(&ComputedNode, &UiGlobalTransform), With<super::super::FlatScreen>>();
            let (node, at) = query.single(app.world()).unwrap();
            (node.size(), at.translation)
        };
        pointer_events(&mut app);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.world_mut().resource_mut::<PendingInsets>().0 = Some(crate::touch_safe_area::SafeInsets { left: 120.0, ..default() });
        // This new contact is admitted after the PreUpdate invalidator, so only
        // the actual Update projection boundary can reject the old node.
        app.world_mut().resource_mut::<FreshContact>().0 = Some((730, before.1));
        app.update();
        assert!(!app.world().resource::<super::super::touch_items::TouchItems>().pending_contact(), "stale crop cannot publish the newly queued contact");
        app.update();
        let rejected = pointer_events(&mut app);
        assert!(rejected.iter().any(|p| p.pointer_id == super::super::touch_items::POINTER && matches!(p.action, PointerAction::Cancel)));
        assert!(!rejected.iter().any(|p| p.pointer_id == super::super::touch_items::POINTER && matches!(p.action, PointerAction::Press(_) | PointerAction::Release(_))));
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyW));
        assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::KeyW);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        for _ in 0..4 { app.update(); }
        let after = {
            let mut query = app.world_mut().query_filtered::<(&ComputedNode, &UiGlobalTransform), With<super::super::FlatScreen>>();
            let (node, at) = query.single(app.world()).unwrap();
            (node.size(), at.translation)
        };
        assert!(after.0.is_finite() && after.0.min_element() > 0.0);
        assert!(after.1.x > before.1.x, "actual newly painted center follows asymmetric safe padding");
        pointer_events(&mut app);
        projected_contact(&mut app, 731);
        for _ in 0..3 { app.update(); }
        let fresh = pointer_events(&mut app);
        assert_eq!(fresh.iter().filter(|p| p.pointer_id == super::super::touch_items::POINTER && matches!(p.action, PointerAction::Press(_))).count(), 1);
        assert_eq!(fresh.iter().filter(|p| p.pointer_id == super::super::touch_items::POINTER && matches!(p.action, PointerAction::Release(_))).count(), 1);
    }

    #[test]
    fn invalid_flat_presentation_retires_display_nodes_and_recovers_without_invalid_layout() {
        use bevy::ecs::system::RunSystemOnce;
        for missing_window in [false, true] {
            let _units = crate::interface::units_at(1.0);
            let mut app = pointer_scene();
            let window = {
                let mut query = app.world_mut().query_filtered::<Entity, With<bevy::window::PrimaryWindow>>();
                query.single(app.world()).unwrap()
            };
            if missing_window {
                app.world_mut().entity_mut(window).remove::<Window>();
            } else {
                app.world_mut().resource_mut::<crate::touch_safe_area::SafeInsets>().left = f32::NAN;
            }
            for _ in 0..2 {
                app.world_mut().run_system_once(super::super::flat_screen).unwrap();
                let mut images = app.world_mut().query_filtered::<Entity, With<super::super::FlatScreen>>();
                assert_eq!(images.iter(app.world()).count(), 0, "invalid metadata cannot retain or create a flat image");
                let mut shades = app.world_mut().query_filtered::<Entity, With<super::super::ScreenShade>>();
                assert_eq!(shades.iter(app.world()).count(), 0, "invalid metadata cannot publish padding into layout");
            }
            if missing_window {
                app.world_mut().entity_mut(window).insert(Window {
                    resolution: bevy::window::WindowResolution::new(1400, 800),
                    ..default()
                });
            } else {
                *app.world_mut().resource_mut::<crate::touch_safe_area::SafeInsets>() = default();
            }
            app.world_mut().run_system_once(super::super::flat_screen).unwrap();
            app.world_mut().run_schedule(PostUpdate);
            let mut images = app.world_mut().query_filtered::<(&ComputedNode, &super::super::FlatCrop), With<super::super::FlatScreen>>();
            let (node, crop) = images.single(app.world()).unwrap();
            assert!(crop.valid());
            assert!(node.size().is_finite() && node.size().min_element() > 0.0);
        }
    }

    #[test]
    fn missing_crop_or_incomplete_computed_geometry_never_publishes_virtual_input() {
        use bevy::ecs::system::RunSystemOnce;
        for cause in 0..3 {
            let _units = crate::interface::units_at(1.0);
            let mut app = pointer_scene();
            for _ in 0..4 { app.update(); }
            projected_contact(&mut app, 740 + cause);
            let mut nodes = app.world_mut().query_filtered::<Entity, With<super::super::FlatScreen>>();
            let entity = nodes.single(app.world()).unwrap();
            match cause {
                0 => { app.world_mut().entity_mut(entity).remove::<super::super::FlatCrop>(); }
                1 => app.world_mut().get_mut::<ComputedNode>(entity).unwrap().size = Vec2::ZERO,
                2 => *app.world_mut().get_mut::<UiGlobalTransform>(entity).unwrap() = UiGlobalTransform::from_xy(f32::NAN, 100.0),
                _ => unreachable!(),
            }
            app.world_mut().run_system_once(super::super::flat_pointer).unwrap();
            let mut touch = app.world_mut().resource_mut::<super::super::touch_items::TouchItems>();
            assert!(!touch.pending_contact(), "cause {cause}");
            assert!(touch.take_projected().is_none(), "cause {cause}");
            assert!(touch.take_cancel(), "cause {cause}");
        }
    }

    #[test]
    fn first_valid_presentation_keeps_a_fresh_projected_press_usable() {
        use bevy::picking::pointer::PointerAction;
        let _units = crate::interface::units_at(1.0);
        let mut app = pointer_scene();
        {
            let mut touch = app.world_mut().resource_mut::<super::super::touch_items::TouchItems>();
            assert!(touch.event(&TouchInput { phase: TouchPhase::Started, position: Vec2::splat(100.0), window: Entity::PLACEHOLDER, id: 700, force: None }, 1));
            touch.project(Some);
        }
        app.update();
        let events = pointer_events(&mut app);
        assert_eq!(events.iter().filter(|p| p.pointer_id == super::super::touch_items::POINTER && matches!(p.action, PointerAction::Press(_))).count(), 1);
        assert!(!events.iter().any(|p| p.pointer_id == super::super::touch_items::POINTER && matches!(p.action, PointerAction::Cancel)));
        assert_eq!(app.world().resource::<super::super::touch_items::TouchItems>().contact_id(), Some(700));
    }

    #[test]
    fn same_window_insets_cancel_a_queued_real_flat_release_before_the_pointer_driver() {
        use bevy::picking::pointer::PointerAction;
        let _units = crate::interface::units_at(1.0);
        let mut app = pointer_scene();
        for _ in 0..4 { app.update(); }
        pointer_events(&mut app);
        projected_contact(&mut app, 701);
        app.update(); // Actual flat_pointer projects Started from the painted node.
        app.update(); // Real PreUpdate driver presses; flat_pointer projects Ended.
        assert!(pointer_events(&mut app).iter().any(|p| p.pointer_id == super::super::touch_items::POINTER && matches!(p.action, PointerAction::Press(_))));
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyW);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        // Published in the actual shared publication set, not by the invalidator.
        app.world_mut().resource_mut::<PendingInsets>().0 = Some(crate::touch_safe_area::SafeInsets { left: 120.0, ..default() });
        app.update();
        let events = pointer_events(&mut app);
        assert!(events.iter().any(|p| p.pointer_id == super::super::touch_items::POINTER && matches!(p.action, PointerAction::Cancel)));
        assert!(!events.iter().any(|p| p.pointer_id == super::super::touch_items::POINTER && matches!(p.action, PointerAction::Press(_) | PointerAction::Release(_))), "old crop cannot release against a changed safe area");
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::KeyW));
        assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
        assert!(!app.world().resource::<super::super::touch_items::TouchItems>().pending_contact());
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::KeyW);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        for _ in 0..4 { app.update(); }
        pointer_events(&mut app);
        projected_contact(&mut app, 702);
        for _ in 0..3 { app.update(); }
        let fresh = pointer_events(&mut app);
        assert_eq!(fresh.iter().filter(|p| p.pointer_id == super::super::touch_items::POINTER && matches!(p.action, PointerAction::Press(_))).count(), 1);
        assert_eq!(fresh.iter().filter(|p| p.pointer_id == super::super::touch_items::POINTER && matches!(p.action, PointerAction::Release(_))).count(), 1, "fresh touch on the newly drawn bounds works once");
    }

    #[derive(Resource, Default)]
    struct SampledUnitScope(f32);
    fn sample_unit_scope(mut sample: ResMut<SampledUnitScope>) {
        sample.0 = crate::interface::unit_scale();
    }

    #[test]
    fn presentation_metadata_changes_retire_old_projection_and_invalid_metadata_stays_closed() {
        use bevy::picking::pointer::PointerAction;
        for case in 0..7 {
            let _units = crate::interface::units_at(1.0);
            let mut app = pointer_scene();
            if case == 6 {
                // units_at is deliberately thread-local, unlike the published
                // next-start menu scale. Keep this scoped case on its owning
                // thread; all ordinary metadata cases retain the real executor.
                app.edit_schedule(PreUpdate, |schedule| {
                    schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
                });
                app.init_resource::<SampledUnitScope>()
                    .add_systems(PreUpdate, sample_unit_scope.before(cancel_changed));
            }
            for _ in 0..4 { app.update(); }
            if case == 6 { assert_eq!(app.world().resource::<SampledUnitScope>().0, 1.0); }
            pointer_events(&mut app);
            projected_contact(&mut app, 710 + case);
            app.update(); // Pending real projected Started.
            match case {
                0 => {
                    let mut windows = app.world_mut().query_filtered::<&mut Window, With<bevy::window::PrimaryWindow>>();
                    windows.single_mut(app.world_mut()).unwrap().resolution.set_physical_resolution(1500, 800);
                }
                1 => {
                    let mut windows = app.world_mut().query_filtered::<&mut Window, With<bevy::window::PrimaryWindow>>();
                    windows.single_mut(app.world_mut()).unwrap().resolution.set_scale_factor_override(Some(2.0));
                }
                2 => set(&mut app, 0.0),
                3 => app.world_mut().resource_mut::<UiScale>().0 = 1.5,
                4 => app.world_mut().resource_mut::<PendingInsets>().0 = Some(crate::touch_safe_area::SafeInsets { left: f32::NAN, ..default() }),
                5 => {
                    let mut windows = app.world_mut().query_filtered::<Entity, With<bevy::window::PrimaryWindow>>();
                    let entity = windows.single(app.world()).unwrap();
                    app.world_mut().despawn(entity);
                }
                6 => {}, // The menu unit scope changes without changing any resource.
                _ => unreachable!(),
            }
            let _changed_units = (case == 6).then(|| crate::interface::units_at(0.9));
            // Exercise the real admission/driver boundary without asking layout
            // to interpret intentionally invalid platform metadata.
            app.world_mut().run_schedule(PreUpdate);
            if case == 6 { assert_eq!(app.world().resource::<SampledUnitScope>().0, 0.9); }
            let events = pointer_events(&mut app);
            assert!(events.iter().any(|p| p.pointer_id == super::super::touch_items::POINTER && matches!(p.action, PointerAction::Cancel)), "case {case}");
            assert!(!events.iter().any(|p| p.pointer_id == super::super::touch_items::POINTER && matches!(p.action, PointerAction::Press(_) | PointerAction::Release(_))), "case {case} must reject the old projected point");
            assert!(!app.world().resource::<super::super::touch_items::TouchItems>().pending_contact(), "case {case}");
        }
    }

    #[test]
    fn actual_flat_image_geometry_resizes_and_its_inverse_maps_the_same_canvas_point() {
        let mut app = scene();
        use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
        use bevy::asset::{AssetApp, AssetPlugin};
        use bevy::camera::{ComputedCameraValues, RenderTargetInfo, visibility::VisibilityPlugin};
        use bevy::ui::{ComputedUiRenderTargetInfo, ui_layout_system, ui_surface::UiSurface, update::propagate_ui_target_cameras};
        app.add_plugins((AssetPlugin::default(), bevy::text::TextPlugin, bevy::transform::TransformPlugin))
            .init_asset::<Mesh>()
            .init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>()
            .add_plugins(VisibilityPlugin)
            .add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate))
            .add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate))
            .init_resource::<UiSurface>()
            .add_systems(PostUpdate, (propagate_ui_target_cameras, ui_layout_system).chain())
            .configure_sets(PostUpdate, PropagateSet::<ComputedUiTargetCamera>::default().after(propagate_ui_target_cameras).before(ui_layout_system))
            .configure_sets(PostUpdate, PropagateSet::<ComputedUiRenderTargetInfo>::default().after(propagate_ui_target_cameras).before(ui_layout_system));
        app.insert_resource(super::super::PipBoy { phase: super::super::Phase::Up, frame: super::super::Frame::Screen, ..default() });
        let camera = app.world_mut().spawn((Camera2d, Camera { computed: ComputedCameraValues { target_info: Some(RenderTargetInfo { physical_size: UVec2::new(1400, 800), scale_factor: 1.0 }), ..default() }, ..default() })).id();
        app.world_mut().spawn((Window { resolution: bevy::window::WindowResolution::new(1400, 800), ..default() }, bevy::window::PrimaryWindow));
        app.insert_resource(super::super::PipBoyScreen { image: Handle::default(), camera, on_model: false, pointer: None, scale: 1.0 });
        for part in [0.0, 0.5, 1.0] {
            set(&mut app, part);
            // Fix the existing menu unit scale on this thread, independently of
            // parallel settings tests. Exercise production nodes through Taffy.
            let _units = crate::interface::units_at(0.8);
            for _ in 0..4 {
                use bevy::ecs::system::RunSystemOnce;
                app.world_mut().run_system_once(super::super::flat_screen).unwrap();
                app.update();
            }
            let zoom = app.world().resource::<PipBoySize>().ratio();
            let mut query = app.world_mut().query_filtered::<(&ComputedNode, &UiGlobalTransform), With<super::super::FlatScreen>>();
            let (node, transform) = query.single(app.world()).unwrap();
            let size = node.size();
            let expected_height = (800.0 * super::super::SCREEN_IMAGE_H * zoom * 0.8 / 960.0).min(736.0);
            assert!((size.y - expected_height).abs() < 1.5, "actual Taffy height {size:?}, expected {expected_height}");
            assert!((size.x / size.y - super::super::SCREEN_CROP.x / super::super::SCREEN_CROP.y).abs() < 0.01);
            let center = transform.translation;
            let ratio = Vec2::new(0.4, 0.6);
            for density in [1.0, 2.0, 3.0] {
                let physical = (center - size / 2.0 + size * ratio) * density;
                let actual = super::super::crop_point(physical, center * density, size * density).unwrap();
                assert!(actual.abs_diff_eq(super::super::SCREEN_CROP * ratio, 1e-3));
            }
        }
    }
}
