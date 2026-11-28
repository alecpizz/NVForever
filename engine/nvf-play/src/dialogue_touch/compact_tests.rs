//! Actual production draw, default-font measurement, Taffy layout and raw touch routes.
//! No renderer/device or game-derived content; platform policy is selected explicitly.
use super::*;
use crate::dialogue_menu::{ChoiceRow, MenuView};
use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::asset::{AssetApp, AssetPlugin};
use bevy::camera::{
    Camera2d, ComputedCameraValues, RenderTargetInfo, visibility::VisibilityPlugin,
};
use bevy::text::{TextPlugin, detect_text_needs_rerender, load_font_assets_into_font_collection};
use bevy::ui::{
    ComputedUiRenderTargetInfo, UiScale, sync_font_size_to_em_size, ui_layout_system,
    ui_surface::UiSurface,
    update::{propagate_ui_target_cameras, update_clipping_system},
    widget::measure_text_system,
};

fn publish_ui_targets(windows: Query<&Window, With<PrimaryWindow>>, mut cameras: Query<&mut Camera>) {
    let window = windows.single().unwrap();
    for mut camera in &mut cameras {
        camera.computed.target_info = Some(RenderTargetInfo {
            physical_size: UVec2::new(window.physical_width(), window.physical_height()),
            scale_factor: window.scale_factor(),
        });
    }
}

pub(super) fn layout_scene(android: bool, size: Vec2, density: f32) -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::input::InputPlugin,
        bevy::state::app::StatesPlugin,
        AssetPlugin::default(),
        TextPlugin,
        bevy::transform::TransformPlugin,
    ))
    .init_asset::<Mesh>()
    .init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>()
    .add_plugins(VisibilityPlugin)
    .add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(
        PostUpdate,
    ))
    .add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(
        PostUpdate,
    ))
    .insert_state(crate::PlayState::Dialogue)
    .init_resource::<crate::bindings::Bindings>()
    .init_resource::<DialogueMenu>()
    .init_resource::<UiScale>()
    .init_resource::<UiSurface>();
    install(&mut app);
    // This fixture measures the full-width Field Notes panel in Taffy.
    app.world_mut().resource_mut::<appearance::Appearance>().theme = appearance::Theme::SplitPanel;
    app.insert_resource(TouchLayout::for_platform(android));
    app.insert_resource(TouchDialogue {
        enabled: true,
        confirm: true,
        ..TouchDialogue::for_platform(android)
    });
    app.add_systems(
        PostUpdate,
        (
            ApplyDeferred,
            publish_ui_targets,
            propagate_ui_target_cameras,
            sync_font_size_to_em_size,
            measure_text_system,
            ui_layout_system,
            update_clipping_system,
        )
            .chain()
            .after(load_font_assets_into_font_collection)
            .after(detect_text_needs_rerender),
    );
    app.configure_sets(
        PostUpdate,
        PropagateSet::<ComputedUiTargetCamera>::default()
            .after(propagate_ui_target_cameras)
            .before(measure_text_system),
    );
    app.configure_sets(
        PostUpdate,
        PropagateSet::<ComputedUiRenderTargetInfo>::default()
            .after(propagate_ui_target_cameras)
            .before(measure_text_system),
    );
    let physical = (size * density).as_uvec2();
    app.world_mut().spawn((
        Camera2d,
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: physical,
                    scale_factor: density,
                }),
                ..default()
            },
            ..default()
        },
    ));
    let mut resolution = bevy::window::WindowResolution::new(physical.x, physical.y);
    resolution.set_scale_factor_override(Some(density));
    let window = app
        .world_mut()
        .spawn((
            Window {
                resolution,
                ..default()
            },
            PrimaryWindow,
        ))
        .id();
    let ids: Vec<u32> = (10..18).collect();
    let mut menu = app.world_mut().resource_mut::<DialogueMenu>();
    menu.list = Some(ListId::for_test(1, &ids));
    menu.view = Some(MenuView {
        speaker: "A speaker".into(),
        choices: (0..8)
            .map(|i| ChoiceRow {
                label: format!("Offered response {i}"),
                ..default()
            })
            .collect(),
        ..default()
    });
    settle(&mut app);
    (app, window)
}
pub(super) fn settle(app: &mut App) {
    for _ in 0..6 {
        app.update();
    }
}
fn rectangle(node: &ComputedNode, transform: &UiGlobalTransform) -> Rect {
    Rect::from_center_size(transform.transform_point2(Vec2::ZERO), node.size())
}
pub(super) fn rows(app: &mut App) -> Vec<(usize, Rect)> {
    let mut query = app
        .world_mut()
        .query::<(&Row, &ComputedNode, &UiGlobalTransform)>();
    let mut rows: Vec<_> = query
        .iter(app.world())
        .map(|(row, node, transform)| (row.index, rectangle(node, transform)))
        .collect();
    rows.sort_by_key(|r| r.0);
    rows
}
fn viewport(app: &mut App) -> Rect {
    let mut query = app
        .world_mut()
        .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<Viewport>>();
    let (node, transform) = query.single(app.world()).unwrap();
    rectangle(node, transform)
}
fn fully_visible(app: &mut App) -> Vec<(usize, Rect)> {
    let view = viewport(app);
    rows(app)
        .into_iter()
        .filter(|(_, r)| r.min.y >= view.min.y - 0.01 && r.max.y <= view.max.y + 0.01)
        .collect()
}
pub(super) fn send(app: &mut App, window: Entity, id: u64, phase: TouchPhase, position: Vec2) {
    app.world_mut().write_message(TouchInput {
        window,
        id,
        phase,
        position,
        force: None,
    });
}

#[test]
fn compact_android_defaults_and_existing_explicit_ini_sizes_remain_independent() {
    let android = TouchDialogue::for_platform(true);
    let desktop = TouchDialogue::for_platform(false);
    assert!(android.enabled && !desktop.enabled);
    assert_eq!((android.choice_scale, android.subtitle_scale), (0.85, 1.0));
    assert_eq!((desktop.choice_scale, desktop.subtitle_scale), (1.0, 1.0));
    assert!((from_part(part(android.choice_scale)) - android.choice_scale).abs() < 0.00001);
    let path = std::env::temp_dir().join(format!(
        "nvf-compact-dialogue-{}-{}.ini",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    for (stored, expected) in [
        ("0.200", 1.0),
        ("0.600", 1.5),
        ("1.000", 2.0),
        ("0.000", 0.75),
    ] {
        std::fs::write(&path,format!("[Settings]\ndialogue-choice-text-size={stored}\ndialogue-subtitle-text-size=0.600\n")).unwrap();
        let mut app = App::new();
        app.insert_resource(android);
        install(&mut app);
        app.insert_resource(crate::settings::SettingsFile(Some(path.clone())));
        crate::settings::apply_stored(app.world_mut());
        let settings = *app.world().resource::<TouchDialogue>();
        assert!((settings.choice_scale - expected).abs() < 0.00001);
        assert!((settings.subtitle_scale - 1.5).abs() < 0.00001);
        assert!(
            app.world()
                .resource::<crate::settings::Settings>()
                .index("dialogue-choice-text-size")
                .is_some()
        );
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn actual_phone_layout_fits_more_whole_choices_without_shrinking_targets_below_48() {
    for (size, density) in [
        (Vec2::new(640., 360.), 1.0),
        (Vec2::new(792., 363.), 1.0),
        (Vec2::new(792., 363.), 2.0),
    ] {
        let (mut old, _) = layout_scene(false, size, density);
        let (mut compact, _) = layout_scene(true, size, density);
        let before = fully_visible(&mut old).len();
        let after = fully_visible(&mut compact).len();
        assert!(
            after >= 3 && after > before,
            "{size:?}/{density}: actual measured compact list must show more whole options ({before}->{after})"
        );
        for (index, rect) in rows(&mut compact) {
            assert!(
                rect.height() >= 48.0 * density - 0.01,
                "choice{index} target too short"
            );
            assert!(
                rect.width() > 0.9 * size.x * density,
                "choice remains full width"
            );
        }
        let mut text = compact
            .world_mut()
            .query::<(&Text, &TextFont, &ComputedNode)>();
        assert!(
            text.iter(compact.world())
                .any(|(t, f, n)| t.0 == "Offered response 0"
                    && f.font_size == FontSize::Px(16.0 * 0.85)
                    && n.size().y > 0.0),
            "production text must actually be measured"
        );
        assert_eq!(
            old.world().resource::<TouchLayout>().row_min,
            56.0,
            "desktop touch layout keeps its former target minimum"
        );
    }
}

#[test]
fn compact_wrapped_choice_keeps_full_text_and_saved_scale_in_actual_layout() {
    let (mut app, _) = layout_scene(true, Vec2::new(640., 360.), 1.0);
    let long = "An offered response wraps completely instead of being truncated. ".repeat(10);
    app.world_mut()
        .resource_mut::<DialogueMenu>()
        .view
        .as_mut()
        .unwrap()
        .choices[1]
        .label = long.clone();
    app.world_mut().resource_mut::<TouchDialogue>().choice_scale = 1.5;
    settle(&mut app);
    let mut text = app
        .world_mut()
        .query::<(&Text, &TextFont, &TextLayout, &ComputedNode)>();
    assert!(text.iter(app.world()).any(|(t, f, l, n)| t.0 == long
        && f.font_size == FontSize::Px(16.0 * 1.5)
        && l.linebreak == bevy::text::LineBreak::WordOrCharacter
        && n.size().y > 60.0));
    assert!(
        rows(&mut app)[1].1.height() > 48.0,
        "wrapping grows its target instead of clipping"
    );
    assert_eq!(
        app.world().resource::<TouchDialogue>().subtitle_scale,
        1.0,
        "choice changes do not resize subtitles"
    );
}

#[test]
fn compact_computed_targets_keep_release_scroll_and_size_change_guards() {
    let (mut app, window) = layout_scene(true, Vec2::new(792., 363.), 2.0);
    app.world_mut().resource_mut::<TouchDialogue>().confirm = false;
    settle(&mut app);
    let visible = fully_visible(&mut app);
    let (index, rect) = visible[2];
    let point = rect.center() / 2.0;
    send(&mut app, window, 301, TouchPhase::Started, point);
    app.update();
    assert_eq!(
        app.world().resource::<TouchFrame>().asked.pick,
        None,
        "press alone cannot answer"
    );
    send(&mut app, window, 301, TouchPhase::Ended, point);
    app.update();
    assert_eq!(
        app.world().resource::<TouchFrame>().asked.pick,
        Some(index),
        "raw release chooses the actual offered row index"
    );
    let point = fully_visible(&mut app)[0].1.center() / 2.0;
    send(&mut app, window, 302, TouchPhase::Started, point);
    app.update();
    send(
        &mut app,
        window,
        302,
        TouchPhase::Moved,
        point - Vec2::new(0., 60.),
    );
    app.update();
    send(
        &mut app,
        window,
        302,
        TouchPhase::Ended,
        point - Vec2::new(0., 60.),
    );
    app.update();
    assert_eq!(
        app.world().resource::<TouchFrame>().asked.pick,
        None,
        "swipe must scroll without choosing"
    );
    let mut scroll = app
        .world_mut()
        .query_filtered::<&ScrollPosition, With<Viewport>>();
    assert!(scroll.single(app.world()).unwrap().y > 0.0);
    let point = fully_visible(&mut app)[0].1.center() / 2.0;
    send(&mut app, window, 303, TouchPhase::Started, point);
    app.update();
    app.world_mut().resource_mut::<TouchDialogue>().choice_scale = 1.5;
    send(&mut app, window, 303, TouchPhase::Ended, point);
    app.update();
    assert_eq!(
        app.world().resource::<TouchFrame>().asked.pick,
        None,
        "size change cancels the old target before repaint"
    );
    settle(&mut app);
    let (index, rect) = fully_visible(&mut app)[0];
    let point = rect.center() / 2.0;
    send(&mut app, window, 304, TouchPhase::Started, point);
    app.update();
    send(&mut app, window, 304, TouchPhase::Ended, point);
    app.update();
    assert_eq!(
        app.world().resource::<TouchFrame>().asked.pick,
        Some(index),
        "fresh resized target remains usable"
    );
}

#[test]
fn android_dialogue_text_is_smaller_by_default_and_the_mobile_sliders_resize_it_live() {
    let (android, desktop) = (TouchDialogue::for_platform(true), TouchDialogue::for_platform(false));
    assert!(android.choice_size() < desktop.choice_size() && android.subtitle_size() < desktop.subtitle_size());
    assert_eq!((android.choice_size(), android.subtitle_size()), (16.0 * 0.85, 16.0));
    let (mut app, _) = layout_scene(true, Vec2::new(792.0, 363.0), 1.0);
    let sizes = |app: &mut App| {
        let world = app.world_mut();
        world.query::<(&appearance::Paint, &TextFont)>().iter(world)
            .filter(|(p, _)| matches!(p, appearance::Paint::ChoiceText { .. }))
            .map(|(_, f)| match f.font_size { bevy::text::FontSize::Px(px) => px, other => panic!("{other:?}") })
            .collect::<Vec<_>>()
    };
    assert!(sizes(&mut app).iter().all(|s| (*s - 16.0 * 0.85).abs() < 0.01), "the painter uses the Android default");
    // The Mobile tab's real slider row changes the painted size without reopening.
    let row = app.world().resource::<crate::settings::Settings>().index("dialogue-choice-text-size").unwrap();
    assert!(crate::settings::set_slider(app.world_mut(), row, 0.0).is_some());
    settle(&mut app);
    assert!(sizes(&mut app).iter().all(|s| (*s - 16.0 * 0.75).abs() < 0.01), "smallest choice text");
    for (_, r) in rows(&mut app) { assert!(r.height() >= 47.99, "targets keep 48 units"); }
    let layout = crate::settings_layout::MOBILE_SECTIONS.iter().any(|s|
        s.keys.contains(&"dialogue-choice-text-size") && s.keys.contains(&"dialogue-subtitle-text-size"));
    assert!(layout, "both text sizes are on the Mobile tab");
}
