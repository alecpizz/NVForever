use super::*;
use crate::touch_safe_area::SafeInsets;
use bevy::{app::{HierarchyPropagatePlugin, PropagateSet}, camera::{ComputedCameraValues, RenderTarget, RenderTargetInfo, visibility::VisibilityPlugin}, input::{InputPlugin, InputSystems, touch::Touches}, ui::{ComputedUiRenderTargetInfo, UiSystems, ui_surface::UiSurface, update::propagate_ui_target_cameras}, window::{CursorMoved, PrimaryWindow, Window, WindowRef}};

#[derive(Resource, Default)]
struct Values { switches: u32, slider: f32, choice: String }

#[derive(Resource)]
struct Owner(Frame);

fn publish(owner: Res<Owner>, touches: Res<Touches>, mut raw: MessageReader<TouchInput>, mut settings: ResMut<SettingsTouch>, mut mask: ResMut<crate::ui_pointer::TouchUiCapture>, mut edges: Local<Vec<TouchInput>>) {
    let enabled = settings.enabled();
    mask.begin_frame();
    for e in raw.read() {
        if enabled {
            edges.push(*e);
            mask.block(e.id);
        }
    }
    let mut frame = owner.0;
    frame.neutral = touches.iter().next().is_none() && !touches.any_just_released() && !touches.any_just_canceled();
    for touch in touches.iter() {
        if enabled { mask.block(touch.id()); }
    }
    settings.publish(enabled.then_some(frame), &mut edges);
}

fn scene(mobile: bool, size: UVec2, density: f32) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, InputPlugin, bevy::asset::AssetPlugin::default(), bevy::transform::TransformPlugin, bevy::text::TextPlugin))
        .init_resource::<Assets<Mesh>>().init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>()
        .add_plugins((VisibilityPlugin, HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate),
            HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate), crate::ui_pointer::UiPointerPlugin))
        .init_resource::<UiSurface>().init_resource::<UiStack>().init_resource::<UiScale>()
        .init_resource::<crate::ui_pointer::TouchUiCapture>().init_resource::<Values>()
        .init_resource::<PlayClock>().init_resource::<PauseAid>().init_resource::<NextState<PlayState>>()
        .init_resource::<PageRequest>().init_resource::<Listening>().init_resource::<SettingsLayout>()
        .init_resource::<SettingsMenuScale>()
        .init_resource::<crate::graphics_resume::GraphicsResume>().init_resource::<LoadList>()
        .init_resource::<Assets<Image>>().init_resource::<crate::save::SaveConfig>().init_resource::<PauseStatus>()
        .init_resource::<crate::pad::PadGlyphs>().insert_resource(PauseLook::plain())
        .insert_resource(crate::settings::SettingsFile(None)).add_message::<CursorMoved>()
        .add_message::<AppExit>().add_message::<crate::ui_sounds::UiSound>().add_message::<crate::save::SaveRequest>()
        .add_message::<crate::save::LoadRequest>().add_message::<MainMenuRequest>();
    let setting = |key| Setting { key, group: "Fixture", label: key, note: "", switches: "", get: |_| false, set: |_, _| {}, slider: None };
    add_setting(&mut app, Setting { get: |w| w.resource::<Values>().switches % 2 != 0,
        set: |w, _| w.resource_mut::<Values>().switches += 1, ..setting("toggle") });
    add_setting(&mut app, Setting { slider: Some(crate::settings::Slider { get: |w| w.resource::<Values>().slider,
        set: |w, value| w.resource_mut::<Values>().slider = value, min: 0.1, step: 0.1, shown: None }), ..setting("slider") });
    crate::settings::add_choice_list(&mut app, setting("choice"), crate::settings::ChoiceList {
        names: |_| (0..15).map(|i| format!("Option {i}")).collect(), chosen: |w| w.resource::<Values>().choice.clone(),
        pick: |w, name| { w.resource_mut::<Values>().choice = name.to_owned(); true },
        save: |w| w.resource::<Values>().choice.clone(), load: |w, name| { w.resource_mut::<Values>().choice = name.to_owned(); true }, ask_keep: false,
    });
    for key in [
        "one", "two", "three", "four", "five", "six", "seven", "eight",
        "nine", "ten", "eleven", "twelve", "thirteen", "fourteen", "fifteen",
    ] {
        add_setting(&mut app, setting(key));
    }
    add_setting(&mut app, Setting { group: "Other", ..setting("other") });
    app.world_mut().resource_mut::<Values>().choice = "Option 0".into();
    app.world_mut().resource_mut::<Values>().slider = 0.1;
    let safe = SafeInsets { left: 22.0, top: 14.0, right: 18.0, bottom: 14.0 };
    let mut resolution = bevy::window::WindowResolution::new(size.x, size.y);
    resolution.set_scale_factor_override(Some(density));
    let window = app.world_mut().spawn((Window { resolution, focused: true, ..default() }, PrimaryWindow)).id();
    let camera = app.world_mut().spawn((Camera2d, bevy::ui::IsDefaultUiCamera, Camera {
        computed: ComputedCameraValues { target_info: Some(RenderTargetInfo { physical_size: size, scale_factor: density }), ..default() }, ..default()
    }, RenderTarget::Window(WindowRef::Entity(window)))).id();
    app.insert_resource(safe).insert_resource(Owner(Frame { window, camera, size: size.as_vec2() / density, safe, ui_scale: 1.0, density,
        admitted: true, fresh: false, neutral: true })).insert_resource(SettingsTouch { enabled: mobile, ..default() });
    app.insert_resource(Paused(true)).insert_resource(Page::Settings { selected: Some(RowId::Setting(0)) })
        .insert_resource(PauseView { scope: SettingsScope::Tab("Fixture"), ..default() });
    app.add_systems(PreUpdate, publish.after(InputSystems).before(UiSystems::Focus));
    app.add_systems(Update, (choose, draw).chain());
    app.add_systems(PostUpdate, (ApplyDeferred, propagate_ui_target_cameras,
        bevy::ui::sync_font_size_to_em_size, bevy::ui::widget::measure_text_system, bevy::ui::ui_layout_system,
        bevy::ui::update::update_clipping_system, ApplyDeferred, crate::menu::barter_touch::tests::normal_ui_stack).chain()
        .after(bevy::text::load_font_assets_into_font_collection).after(bevy::text::detect_text_needs_rerender));
    app.configure_sets(PostUpdate, PropagateSet::<ComputedUiTargetCamera>::default().after(propagate_ui_target_cameras).before(bevy::ui::widget::measure_text_system));
    app.configure_sets(PostUpdate, PropagateSet::<ComputedUiRenderTargetInfo>::default().after(propagate_ui_target_cameras).before(bevy::ui::widget::measure_text_system));
    for _ in 0..3 { app.update(); }
    app
}

fn view(app: &App) -> &PauseView { app.world().resource::<PauseView>() }
fn values(app: &App) -> &Values { app.world().resource::<Values>() }
fn rect(app: &mut App, action: PauseButton) -> Rect {
    let density = app.world().resource::<Owner>().0.density;
    let world = app.world_mut();
    let (_, node, transform) = world.query::<(&PauseButton, &ComputedNode, &UiGlobalTransform)>().iter(world).find(|(b, _, _)| **b == action)
        .unwrap_or_else(|| panic!("missing painted {action:?}"));
    Rect::from_center_size(transform.translation / density, node.size() / density)
}
fn edge(app: &mut App, phase: TouchPhase, point: Vec2) {
    let window = app.world().resource::<Owner>().0.window;
    app.world_mut().write_message(TouchInput { window, id: 1, force: None, position: point, phase });
}
fn tap(app: &mut App, point: Vec2) {
    edge(app, TouchPhase::Started, point);
    edge(app, TouchPhase::Ended, point);
    app.update();
}

#[test]
fn settings_touch_keeps_exact_desktop_layout_and_proportions_at_default_scale() {
    let mut mobile = scene(true, UVec2::new(2400, 1080), 2.0);
    let mut desktop = scene(false, UVec2::new(2400, 1080), 2.0);
    assert_eq!(mobile.world().resource::<SettingsMenuScale>().scale(), 1.0);
    assert_eq!(view(&mobile).lines(), view(&desktop).lines(), "same desktop row window");
    for b in [
        PauseButton::Rail(RailItem::Tab("Other")),
        PauseButton::Row(RowId::Setting(0)),
        PauseButton::ScrollTrack,
        PauseButton::Defaults,
        PauseButton::Back,
    ] {
        let mr = rect(&mut mobile, b);
        let dr = rect(&mut desktop, b);
        assert!((mr.min - dr.min).length() < 0.5 && (mr.max - dr.max).length() < 0.5, "{b:?}: mobile {mr:?} vs desktop {dr:?}");
    }
    // Right-side rail and scrollbar stay on the right, never replaced by a zoomed grid.
    let row = rect(&mut mobile, PauseButton::Row(RowId::Setting(0)));
    let track = rect(&mut mobile, PauseButton::ScrollTrack);
    let tab = rect(&mut mobile, PauseButton::Rail(RailItem::Tab("Other")));
    assert!(track.min.x >= row.max.x && tab.min.x >= track.max.x, "desktop list -> scrollbar -> right rail order");
}

#[test]
fn settings_touch_drag_follows_and_clamps_without_activating_a_row() {
    let mut app = scene(true, UVec2::new(2400, 1080), 2.0);
    let row0 = rect(&mut app, PauseButton::Row(RowId::Setting(0)));
    let row_h = row0.height();
    let p = row0.center();
    edge(&mut app, TouchPhase::Started, p);
    app.update();
    assert_eq!(values(&app).switches, 0, "not on touch-down");
    edge(&mut app, TouchPhase::Moved, p - Vec2::Y * row_h * 2.0);
    app.update();
    assert_eq!(view(&app).scroll, 2, "two painted desktop row heights track the finger 1:1");
    edge(&mut app, TouchPhase::Moved, p - Vec2::Y * 2000.0);
    app.update();
    let d = view(&app).drawn.as_ref().unwrap();
    assert_eq!(view(&app).scroll, d.entries.len() - d.lines);
    edge(&mut app, TouchPhase::Moved, p);
    app.update();
    assert_eq!(view(&app).scroll, 0, "reversing at the end does not have to pay back overshoot");
    edge(&mut app, TouchPhase::Ended, p);
    edge(&mut app, TouchPhase::Started, p);
    edge(&mut app, TouchPhase::Ended, p);
    app.update();
    assert_eq!(values(&app).switches, 0, "release on the original row and a trailing same-frame tap do not activate");
    assert_eq!(values(&app).slider, 0.1);
    assert_eq!(values(&app).choice, "Option 0");
    tap(&mut app, p);
    assert_eq!(values(&app).switches, 1, "a new gesture after the batch still taps");
}

#[test]
fn settings_touch_slider_uses_normal_fit_and_clamps_past_the_meter() {
    let mut app = scene(true, UVec2::new(2400, 1080), 2.0);
    let meter = rect(&mut app, PauseButton::Meter(RowId::Setting(1)));
    let p = Vec2::new(meter.min.x + meter.width() * 0.15, meter.center().y);
    edge(&mut app, TouchPhase::Started, p);
    app.update();
    assert_eq!(values(&app).slider, 0.1);
    let mid = Vec2::new(meter.min.x + meter.width() * 0.72, p.y);
    edge(&mut app, TouchPhase::Moved, mid);
    app.update();
    let fit = app.world().resource::<Settings>().list()[1].slider.unwrap().fit(0.72);
    assert_eq!(values(&app).slider, fit);
    edge(&mut app, TouchPhase::Ended, meter.max + Vec2::X * 200.0);
    app.update();
    assert_eq!(values(&app).slider, 1.0);
    assert_eq!(view(&app).scroll, 0);
    assert_eq!(values(&app).switches, 0);
}

#[test]
fn settings_touch_dropdown_scrolls_picks_and_closes_without_click_through() {
    let mut app = scene(true, UVec2::new(2400, 1080), 2.0);
    let row = rect(&mut app, PauseButton::Row(RowId::Setting(2))).center();
    tap(&mut app, row);
    let choice0 = rect(&mut app, PauseButton::Choice(2, 0));
    let p = choice0.center();
    edge(&mut app, TouchPhase::Started, p);
    edge(&mut app, TouchPhase::Moved, p - Vec2::Y * 3.0 * choice0.height());
    edge(&mut app, TouchPhase::Ended, p - Vec2::Y * 3.0 * choice0.height());
    app.update();
    assert_eq!(view(&app).dropdown.unwrap().first, 3);
    assert_eq!(values(&app).choice, "Option 0");
    let option = rect(&mut app, PauseButton::Choice(2, 5)).center();
    tap(&mut app, option);
    assert_eq!(values(&app).choice, "Option 5");
    assert_eq!(view(&app).dropdown, None);
    tap(&mut app, row);
    assert!(view(&app).dropdown.is_some());
    let back = rect(&mut app, PauseButton::Back).center();
    tap(&mut app, back);
    assert_eq!(view(&app).dropdown, None);
    assert_eq!(values(&app).choice, "Option 5");
    assert!(matches!(*app.world().resource::<Page>(), Page::Settings { .. }), "outside tap did not click through to Back");
    tap(&mut app, back);
    assert_eq!(*app.world().resource::<Page>(), Page::Main);
}

#[test]
fn settings_touch_tabs_scrollbar_and_ui_scale_work_cleanly() {
    let mut app = scene(true, UVec2::new(2400, 1080), 2.0);
    let base_h = rect(&mut app, PauseButton::Row(RowId::Setting(0))).height();
    // Touch-drag the desktop scrollbar track, then return to top so Row(0) stays visible.
    let track = rect(&mut app, PauseButton::ScrollTrack);
    tap(&mut app, Vec2::new(track.center().x, track.max.y - 4.0));
    assert!(view(&app).scroll > 0, "tapping scrollbar track scrolls list");
    tap(&mut app, Vec2::new(track.center().x, track.min.y + 4.0));
    assert_eq!(view(&app).scroll, 0, "tapping top of scrollbar returns to row 0");
    // Live Settings Menu UI Scale: 120% enlarges rows without overflowing the viewport,
    // and the ceiling reaches 200%.
    assert!(take_settings_menu_scale(app.world_mut(), "200%"));
    assert_eq!(app.world().resource::<SettingsMenuScale>().scale(), 2.0);
    assert!(take_settings_menu_scale(app.world_mut(), "120%"));
    for _ in 0..3 { app.update(); }
    let scaled = rect(&mut app, PauseButton::Row(RowId::Setting(0)));
    assert!(scaled.height() > base_h * 1.15 && scaled.height() < base_h * 1.25, "scaled {scaled:?} vs base {base_h}");
    let screen = Rect::from_corners(Vec2::ZERO, app.world().resource::<Owner>().0.size);
    for b in [PauseButton::Rail(RailItem::Tab("Other")), PauseButton::Back, PauseButton::Defaults] {
        let r = rect(&mut app, b);
        assert!(screen.inflate(1.0).contains(r.min) && screen.inflate(1.0).contains(r.max), "{b:?} inside viewport: {r:?}");
    }
    let tab = rect(&mut app, PauseButton::Rail(RailItem::Tab("Other"))).center();
    tap(&mut app, tab);
    assert_eq!(view(&app).scope, SettingsScope::Tab("Other"));
}

#[test]
fn settings_touch_desktop_keeps_mouse_press_and_keyboard_activation() {
    let mut app = scene(false, UVec2::new(1920, 1080), 1.0);
    assert!(!view(&app).drawn.as_ref().unwrap().touch);
    let p = rect(&mut app, PauseButton::Row(RowId::Setting(0))).center();
    let window = app.world().resource::<Owner>().0.window;
    app.world_mut().get_mut::<Window>(window).unwrap().set_cursor_position(Some(p));
    app.world_mut().write_message(bevy::input::mouse::MouseButtonInput { window, button: MouseButton::Left, state: ButtonState::Pressed });
    app.update();
    assert_eq!(values(&app).switches, 1, "desktop mouse fires on down, not release");
    app.world_mut().write_message(bevy::input::mouse::MouseButtonInput { window, button: MouseButton::Left, state: ButtonState::Released });
    app.update();
    let key = KeyboardInput { window, key_code: KeyCode::Enter, logical_key: Key::Enter, state: ButtonState::Pressed, text: None, repeat: false };
    app.world_mut().write_message(key);
    app.update();
    assert_eq!(values(&app).switches, 2, "keyboard Enter uses the same selected desktop row");
}
