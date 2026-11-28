// Real Taffy + UiPointerPlugin -> actual Main Menu chooser. No RenderApp/GPU or JNI.
fn retained_cursor_main_menu_scene() -> (App, Entity, Entity) {
    use bevy::{camera::{visibility::VisibilityPlugin, ComputedCameraValues, RenderTargetInfo},
        app::{HierarchyPropagatePlugin, PropagateSet},
        ui::{ComputedUiTargetCamera, ComputedUiRenderTargetInfo, UiScale, UiStack,
            ui_surface::UiSurface, update::propagate_ui_target_cameras}};
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::input::InputPlugin, bevy::asset::AssetPlugin::default(),
        bevy::transform::TransformPlugin, bevy::text::TextPlugin))
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>()
        .add_plugins((VisibilityPlugin,
            HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate),
            HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate),
            crate::ui_pointer::UiPointerPlugin))
        .init_resource::<UiScale>().init_resource::<UiSurface>().init_resource::<UiStack>()
        .insert_resource(MainMenu { open: true, selected: 1, ..default() })
        .init_resource::<Page>().init_resource::<PageRequest>()
        .init_resource::<crate::pause::PlayClock>().init_resource::<MenuAid>()
        .add_message::<crate::save::LoadRequest>().add_message::<crate::intro::PlayIntro>()
        .add_message::<crate::ui_sounds::UiSound>().add_systems(Update, choose);
    app.add_systems(PostUpdate, (ApplyDeferred, propagate_ui_target_cameras, bevy::ui::ui_layout_system).chain());
    app.configure_sets(PostUpdate, PropagateSet::<ComputedUiTargetCamera>::default()
        .after(propagate_ui_target_cameras).before(bevy::ui::ui_layout_system));
    app.configure_sets(PostUpdate, PropagateSet::<ComputedUiRenderTargetInfo>::default()
        .after(propagate_ui_target_cameras).before(bevy::ui::ui_layout_system));
    let camera = app.world_mut().spawn((Camera2d, Camera {
        computed: ComputedCameraValues { target_info: Some(RenderTargetInfo {
            physical_size: UVec2::splat(400), scale_factor: 1.0 }), ..default() }, ..default()
    })).id();
    let window = app.world_mut().spawn((Window {
        resolution: bevy::window::WindowResolution::new(400, 400), ..default()
    }, bevy::window::PrimaryWindow)).id();
    let button = app.world_mut().spawn((crate::ui_pointer::Button, EntryNode(3),
        Node { position_type: PositionType::Absolute, left: px(20.0), top: px(20.0),
            width: px(100.0), height: px(50.0), ..default() }, UiTargetCamera(camera))).id();
    for _ in 0..4 { app.update(); }
    assert_eq!(Entry::ALL[3], Entry::Settings);
    assert!(app.world().get::<ComputedNode>(button).unwrap().size().abs_diff_eq(Vec2::new(100.0, 50.0), 0.01));
    assert!(app.world().get::<bevy::ui::UiGlobalTransform>(button).unwrap().translation.abs_diff_eq(Vec2::new(70.0, 45.0), 0.01));
    assert!(app.world().get::<InheritedVisibility>(button).unwrap().get());
    assert_eq!(app.world().get::<ComputedUiTargetCamera>(button).unwrap().get(), Some(camera));
    let mut stack = app.world_mut().resource_mut::<UiStack>();
    stack.uinodes = vec![button]; stack.partition.clear(); stack.partition.push(0..1);
    (app, window, button)
}

#[test]
fn actual_main_menu_settings_tap_uses_touch_after_retained_cursor_focus_roundtrip() {
    use bevy::{input::touch::{TouchInput, TouchPhase}, window::{AppLifecycle, WindowFocused}};
    for covered_roundtrip in [false, true] {
        let (mut app, window, button) = retained_cursor_main_menu_scene();
        app.add_message::<WindowFocused>().add_message::<AppLifecycle>()
            .init_resource::<crate::pad::PlatformInputGate>();
        let mut sounds = bevy::ecs::message::MessageCursor::<crate::ui_sounds::UiSound>::default();
        let mut intros = bevy::ecs::message::MessageCursor::<crate::intro::PlayIntro>::default();
        let mut loads = bevy::ecs::message::MessageCursor::<crate::save::LoadRequest>::default();
        app.world_mut().get_mut::<Window>(window).unwrap().set_cursor_position(Some(Vec2::splat(300.0)));
        if covered_roundtrip {
            app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
            app.world_mut().write_message(WindowFocused { window, focused: false });
            app.world_mut().write_message(AppLifecycle::Suspended);
            app.world_mut().resource_mut::<crate::pad::PlatformInputGate>().set_blocked(true);
            app.update();
            assert!(app.world().resource::<PageRequest>().0.is_none());
            app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
            app.world_mut().write_message(WindowFocused { window, focused: true });
            app.world_mut().write_message(AppLifecycle::Running);
            app.world_mut().resource_mut::<crate::pad::PlatformInputGate>().set_blocked(false);
        }
        app.update();
        assert!(app.world().resource::<PageRequest>().0.is_none());
        assert_eq!(*app.world().get::<PointerState>(button).unwrap(), PointerState::None);
        app.world_mut().write_message(TouchInput { window, id: 81, phase: TouchPhase::Started,
            position: Vec2::splat(40.0), force: None });
        app.update();
        assert!(matches!(app.world().resource::<PageRequest>().0.as_ref(), Some(Page::Settings { selected: None })),
            "actual main-menu choose must receive the fresh touch, not the passive cursor");
        assert_eq!(*app.world().get::<PointerState>(button).unwrap(), PointerState::Pressed);
        assert!(app.world().resource::<MainMenu>().open);
        let picked = sounds.read(app.world().resource::<Messages<crate::ui_sounds::UiSound>>())
            .filter(|s| s.event == crate::ui_sounds::UiEvent::Picked).count();
        assert_eq!(picked, 1, "one actual menu action on touch press");
        app.world_mut().write_message(TouchInput { window, id: 81, phase: TouchPhase::Ended,
            position: Vec2::splat(40.0), force: None });
        app.update();
        assert_eq!(sounds.read(app.world().resource::<Messages<crate::ui_sounds::UiSound>>())
            .filter(|s| s.event == crate::ui_sounds::UiEvent::Picked).count(), 0,
            "release cannot replay the Settings action");
        assert_eq!(intros.read(app.world().resource::<Messages<crate::intro::PlayIntro>>()).count(), 0);
        assert_eq!(loads.read(app.world().resource::<Messages<crate::save::LoadRequest>>()).count(), 0);
        assert!(!app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    }
}
