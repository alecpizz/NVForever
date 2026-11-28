// Parent-only arbitration fixture. Pose/settle/native endpoint execution is
// separately covered by the existing real core tests, not a fake close handler.
struct ClosingScene {
    app: App,
    window: Entity,
    camera: Entity,
    root: Entity,
    screen: Entity,
    button: Entity,
    knob: Entity,
}
fn closing_place(scene: &mut ClosingScene, entity: Entity, pixel: Vec2) {
    let world = scene.app.world();
    let camera = world.get::<Camera>(scene.camera).unwrap();
    let transform = world.get::<GlobalTransform>(scene.camera).unwrap();
    let ray = camera.viewport_to_world(transform, pixel).unwrap();
    let at = transform.translation() + ray.direction.as_vec3() * 10.0;
    scene
        .app
        .world_mut()
        .entity_mut(entity)
        .insert(GlobalTransform::from(
            Transform::from_translation(at).with_scale(Vec3::splat(0.18)),
        ));
}
fn closing_scene() -> ClosingScene {
    closing_scene_at(UVec2::splat(1000))
}
fn closing_scene_at(view: UVec2) -> ClosingScene {
    use bevy::camera::{CameraProjection, RenderTargetInfo};
    use bevy::window::PrimaryWindow;
    // Reuse the real authored wearable/form fixture, not a fabricated permission.
    let (mut donor, _) = scene();
    let mut sim = donor
        .world_mut()
        .remove_resource::<nvf_play::sim::SimState>()
        .unwrap();
    assert!(nvf_play::pipboy::wears_pipboy(&sim));
    let menu = nvf_play::menu::model::MenuModel::open_inventory(&mut sim.world).unwrap();
    // This fixture already installs the production follow_pointer/Posed publisher.
    let mut app = crate::pipboy_arm::tests::model_control_fixture();
    app.add_plugins((
        MinimalPlugins,
        bevy::input::InputPlugin,
        bevy::state::app::StatesPlugin,
    ))
    .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_millis(16),
    ))
    .insert_state(nvf_play::PlayState::Menu)
    .insert_resource(sim)
    .insert_resource(nvf_play::menu::ActiveMenu(Some(menu)))
    .init_resource::<nvf_play::bindings::Bindings>()
    .init_resource::<nvf_play::control::PlayerControl>()
    .init_resource::<nvf_play::hotkeys::Hotkeys>()
    .init_resource::<nvf_play::pipboy::PointerMoved>()
    .init_resource::<nvf_play::pad::PlatformInputGate>()
    .init_resource::<UsePipBoy>()
    .add_message::<bevy::window::AppLifecycle>()
    .add_message::<bevy::window::WindowFocused>();
    let window = {
        let mut q = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>();
        q.single(app.world()).unwrap()
    };
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution = bevy::window::WindowResolution::new(view.x, view.y);
    let camera = {
        let mut q = app
            .world_mut()
            .query_filtered::<Entity, With<nvf_play::PlayerCamera>>();
        q.single(app.world()).unwrap()
    };
    {
        let mut camera = app.world_mut().get_mut::<Camera>(camera).unwrap();
        camera.computed.clip_from_view = PerspectiveProjection {
            aspect_ratio: view.x as f32 / view.y as f32,
            ..default()
        }
        .get_clip_from_view();
        camera.computed.target_info = Some(RenderTargetInfo {
            physical_size: view,
            scale_factor: 1.,
        });
    }
    let root = app
        .world_mut()
        .spawn((
            Transform::IDENTITY,
            GlobalTransform::IDENTITY,
            Visibility::Inherited,
            InheritedVisibility::VISIBLE,
        ))
        .id();
    let actor = crate::limbs::ActorRig {
        reference: 0,
        base: 0,
        body: 0,
        skeleton: String::new(),
        skel: std::sync::Arc::default(),
        nodes: vec![],
        base_scale: 1.,
    };
    app.insert_resource(crate::player_body::PlayerBody::synthetic_script_models(
        (root, actor.clone()),
        Some((root, actor)),
    ));
    let parts = {
        let mut q = app
            .world_mut()
            .query::<(Entity, &crate::pipboy_arm::PipBoyPart)>();
        q.iter(app.world())
            .map(|(e, p)| (e, *p))
            .collect::<Vec<_>>()
    };
    let screen = parts
        .iter()
        .find(|(_, p)| *p == crate::pipboy_arm::PipBoyPart::Screen)
        .unwrap()
        .0;
    let button = parts
        .iter()
        .find(|(_, p)| *p == crate::pipboy_arm::PipBoyPart::Button(nvf_play::pipboy::Page::Stats))
        .unwrap()
        .0;
    let mesh = app
        .world()
        .get::<crate::pipboy_arm::PartMesh>(screen)
        .unwrap()
        .0
        .clone();
    let knob = app
        .world_mut()
        .spawn((
            crate::pipboy_arm::PipBoyPart::ScrollKnob,
            crate::pipboy_arm::PartMesh(mesh),
            GlobalTransform::IDENTITY,
            Visibility::Visible,
            InheritedVisibility::VISIBLE,
            ChildOf(root),
        ))
        .id();
    for (entity, _) in &parts {
        app.world_mut().entity_mut(*entity).insert(ChildOf(root));
    }
    app.add_plugins((
        super::super::TouchControlsPlugin,
        nvf_play::pipboy::touch_scroll::TouchPointerPlugin,
        nvf_play::pad::InputSwitchPlugin,
    ));
    app.insert_resource(nvf_play::settings::SettingsFile(None));
    let mut scene = ClosingScene {
        app,
        window,
        camera,
        root,
        screen,
        button,
        knob,
    };
    for (entity, part) in parts {
        let pixel = match part {
            crate::pipboy_arm::PipBoyPart::Screen => view.as_vec2() * Vec2::new(0.5, 0.35),
            crate::pipboy_arm::PipBoyPart::Button(page) => {
                view.as_vec2() * Vec2::new(0.15 + page.index() as f32 * 0.12, 0.18)
            }
            _ => continue,
        };
        closing_place(&mut scene, entity, pixel);
    }
    closing_place(&mut scene, knob, view.as_vec2() * Vec2::new(0.82, 0.35));
    scene.app.update();
    scene
        .app
        .world_mut()
        .resource_mut::<super::super::layout::Layout>()
        .config = super::super::layout::Config::default();
    let row = scene
        .app
        .world()
        .resource::<nvf_play::settings::Settings>()
        .index("touch-pipboy-swipe-v1")
        .unwrap();
    if !(scene
        .app
        .world()
        .resource::<nvf_play::settings::Settings>()
        .list()[row]
        .get)(scene.app.world())
    {
        assert_eq!(
            nvf_play::settings::flip(scene.app.world_mut(), row),
            Some(true)
        );
    }
    scene.app.update();
    scene.app.update();
    assert!(scene
        .app
        .world()
        .resource::<super::super::pipboy_model::Geometry>()
        .screen
        .is_some());
    assert_eq!(
        scene
            .app
            .world()
            .resource::<crate::player_body::PlayerBody>()
            .first_person_root(),
        Some(root)
    );
    let handle = closing_handle(&scene);
    assert!(
        scene
            .app
            .world()
            .resource::<super::super::layout::Layout>()
            .config
            .hit_filtered(handle, view.as_vec2(), false, false, |_| true)
            .is_none(),
        "the actual saved-default control layout must leave this authored handle case unclaimed"
    );
    scene
}
fn closing_size(scene: &ClosingScene) -> Vec2 {
    scene
        .app
        .world()
        .get::<Window>(scene.window)
        .unwrap()
        .resolution
        .size()
}
fn closing_safe_bottom(scene: &ClosingScene) -> f32 {
    scene
        .app
        .world()
        .resource::<super::super::drawer::SafeBottom>()
        .0
}
fn closing_handle(scene: &ClosingScene) -> Vec2 {
    super::super::drawer::handle(
        closing_size(scene),
        scene
            .app
            .world()
            .resource::<super::super::drawer::SafeBottom>()
            .0,
    )
    .center()
}
fn closing_edge(scene: &mut ClosingScene, id: u64, phase: TouchPhase, p: Vec2) {
    let size = closing_size(scene);
    let safe_end = size.y - closing_safe_bottom(scene).max(0.0);
    assert!(
        p.is_finite() && p.x >= 0.0 && p.x <= size.x && p.y >= 0.0 && p.y <= safe_end,
        "all synthetic raw contacts must remain inside the actual window/safe bottom: {p:?}"
    );
    scene.app.world_mut().write_message(TouchInput {
        window: scene.window,
        id,
        phase,
        position: p,
        force: None,
    });
}
fn closing_idle(scene: &mut ClosingScene, n: usize) {
    for _ in 0..n {
        scene.app.update();
    }
}
fn closing_raw_start(scene: &mut ClosingScene, id: u64) {
    let p = closing_handle(scene);
    closing_edge(scene, id, TouchPhase::Started, p);
    scene.app.update();
}

#[test]
fn actual_parent_closing_handle_loses_to_published_posed_screen_button_and_scrollbar() {
    for part in ["screen", "button", "scrollbar"] {
        let mut scene = closing_scene();
        let point = closing_handle(&scene);
        let entity = match part {
            "screen" => scene.screen,
            "button" => scene.button,
            _ => scene.knob,
        };
        closing_place(&mut scene, entity, point);
        closing_idle(&mut scene, 2);
        {
            let geometry = scene
                .app
                .world()
                .resource::<super::super::pipboy_model::Geometry>();
            assert!(
                match part {
                    "screen" => geometry.screen.unwrap().contains(point),
                    "button" => geometry.buttons.iter().any(|b| b.bounds.contains(point)),
                    _ => geometry.knob.unwrap().contains(point),
                },
                "overlap must come from actual camera/Posed publication"
            );
        }
        closing_raw_start(&mut scene, 710);
        assert!(
            !scene.app.world().resource::<ArmGesture>().captures(),
            "{part} owns the actual parent edge"
        );
        closing_edge(&mut scene, 710, TouchPhase::Ended, point);
        closing_idle(&mut scene, 4);
        assert!(!scene.app.world().resource::<ArmGesture>().captures());
    }
}

#[test]
fn actual_parent_free_closing_handle_reverses_and_hands_release_or_cancel_to_real_core_owner() {
    for cancel in [false, true] {
        let mut scene = closing_scene();
        let point = closing_handle(&scene);
        let travel =
            closing_travel(closing_size(&scene), closing_safe_bottom(&scene), point).unwrap();
        scene
            .app
            .world_mut()
            .write_message(bevy::input::keyboard::KeyboardInput {
                window: scene.window,
                key_code: KeyCode::KeyW,
                logical_key: bevy::input::keyboard::Key::Character("w".into()),
                state: bevy::input::ButtonState::Pressed,
                text: None,
                repeat: false,
            });
        scene.app.update();
        closing_raw_start(&mut scene, 720);
        assert_eq!(
            scene.app.world().resource::<ArmGesture>().progress(),
            Some(1.0)
        );
        for (dy, expected) in [
            (travel * 0.5, 0.5),
            (travel * 0.25, 0.75),
            (travel * 0.75, 0.25),
        ] {
            closing_edge(&mut scene, 720, TouchPhase::Moved, point + Vec2::Y * dy);
            scene.app.update();
            assert!(
                (scene
                    .app
                    .world()
                    .resource::<ArmGesture>()
                    .progress()
                    .unwrap()
                    - expected)
                    .abs()
                    < 1e-5
            );
        }
        closing_edge(
            &mut scene,
            720,
            if cancel {
                TouchPhase::Canceled
            } else {
                TouchPhase::Ended
            },
            point + Vec2::Y * (travel * 0.75),
        );
        scene.app.update();
        closing_edge(
            &mut scene,
            720,
            TouchPhase::Moved,
            point + Vec2::Y * (travel * 0.125),
        );
        scene.app.update();
        assert_eq!(
            scene.app.world().resource::<ArmGesture>().progress(),
            Some(0.25),
            "retired contact cannot edit settle target"
        );
        assert!(
            scene.app.world().resource::<ArmGesture>().captures(),
            "the real core, not a fixture handler, still owns settlement"
        );
        assert!(!scene
            .app
            .world()
            .resource::<nvf_play::pad::PlatformInputGate>()
            .allows_actions());
        assert!(scene
            .app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW));
        assert!(
            !scene
                .app
                .world()
                .resource::<nvf_play::control::HeldSneakInput>()
                .active
        );
        assert!(scene
            .app
            .world()
            .resource::<super::super::TouchState>()
            .contacts
            .is_empty());
    }
}

#[test]
fn actual_parent_missing_or_stale_closing_projection_fails_closed_then_fresh_publication_admits() {
    for cause in ["image", "missing_screen", "unpublished_part"] {
        let mut scene = closing_scene();
        let point = closing_handle(&scene);
        let mesh = scene
            .app
            .world()
            .get::<crate::pipboy_arm::PartMesh>(scene.screen)
            .unwrap()
            .0
            .clone();
        match cause {
            "image" => {
                let image = scene
                    .app
                    .world_mut()
                    .resource_mut::<Assets<Image>>()
                    .add(Image::default());
                scene
                    .app
                    .world_mut()
                    .resource_mut::<nvf_play::pipboy::PipBoyScreen>()
                    .image = image;
            }
            "missing_screen" => {
                scene.app.world_mut().entity_mut(scene.screen).despawn();
                closing_idle(&mut scene, 2);
            }
            _ => {
                let part = scene
                    .app
                    .world_mut()
                    .spawn((
                        crate::pipboy_arm::PipBoyPart::Button(nvf_play::pipboy::Page::Data),
                        crate::pipboy_arm::PartMesh(mesh.clone()),
                        GlobalTransform::IDENTITY,
                        Visibility::Visible,
                        InheritedVisibility::VISIBLE,
                        ChildOf(scene.root),
                    ))
                    .id();
                closing_place(&mut scene, part, Vec2::new(750., 180.));
            }
        }
        closing_raw_start(&mut scene, 730);
        assert!(
            !scene.app.world().resource::<ArmGesture>().captures(),
            "{cause}: stale/missing authority cannot mean free"
        );
        closing_edge(&mut scene, 730, TouchPhase::Ended, point);
        closing_idle(&mut scene, 4);
        if cause == "missing_screen" {
            scene.screen = scene
                .app
                .world_mut()
                .spawn((
                    crate::pipboy_arm::PipBoyPart::Screen,
                    crate::pipboy_arm::PartMesh(mesh),
                    GlobalTransform::IDENTITY,
                    Visibility::Visible,
                    InheritedVisibility::VISIBLE,
                    ChildOf(scene.root),
                ))
                .id();
            let screen = scene.screen;
            closing_place(&mut scene, screen, Vec2::new(500., 350.));
        }
        closing_idle(&mut scene, 3);
        closing_raw_start(&mut scene, 731);
        assert!(
            scene.app.world().resource::<ArmGesture>().captures(),
            "{cause}: a new touch can use the actual fresh publication"
        );
    }
}

#[test]
fn actual_parent_existing_projected_item_owner_precedes_new_closing_handle_contact() {
    let mut scene = closing_scene();
    let screen = scene
        .app
        .world()
        .resource::<super::super::pipboy_model::Geometry>()
        .screen
        .unwrap()
        .center();
    closing_edge(&mut scene, 740, TouchPhase::Started, screen);
    scene.app.update();
    assert!(scene
        .app
        .world()
        .resource::<nvf_play::pipboy::touch_items::TouchItems>()
        .owns(740));
    let point = closing_handle(&scene);
    closing_edge(&mut scene, 741, TouchPhase::Started, point);
    scene.app.update();
    assert!(!scene.app.world().resource::<ArmGesture>().captures());
    closing_edge(&mut scene, 741, TouchPhase::Ended, point);
    closing_edge(&mut scene, 740, TouchPhase::Ended, screen);
    closing_idle(&mut scene, 5);
    closing_raw_start(&mut scene, 742);
    assert!(scene.app.world().resource::<ArmGesture>().captures());
}

#[test]
fn actual_packed_pins_tray_reserves_handle_band_and_its_active_capture_precedes_closing() {
    let mut scene = closing_scene();
    let row = scene
        .app
        .world()
        .resource::<nvf_play::settings::Settings>()
        .index("touch-hotbar")
        .unwrap();
    if !(scene
        .app
        .world()
        .resource::<nvf_play::settings::Settings>()
        .list()[row]
        .get)(scene.app.world())
    {
        assert_eq!(
            nvf_play::settings::flip(scene.app.world_mut(), row),
            Some(true)
        );
    }
    closing_idle(&mut scene, 2);
    let tray = {
        let mut q = scene.app.world_mut().query::<(&Text, &ChildOf)>();
        let parent = q
            .iter(scene.app.world())
            .find(|(text, _)| text.0 == "Pins")
            .unwrap()
            .1
            .parent();
        let node = scene.app.world().get::<Node>(parent).unwrap();
        let (Val::Px(x), Val::Px(y), Val::Px(w), Val::Px(h)) =
            (node.left, node.top, node.width, node.height)
        else {
            panic!("drawn tray uses actual pixel bounds")
        };
        Rect::from_corners(Vec2::new(x, y), Vec2::new(x + w, y + h))
    };
    let handle = super::super::drawer::handle(
        Vec2::splat(1000.),
        scene
            .app
            .world()
            .resource::<super::super::drawer::SafeBottom>()
            .0,
    );
    let overlap = tray.intersect(handle);
    assert!(
        overlap.width() <= 0. || overlap.height() <= 0.,
        "actual packing reserves the bottom handle band"
    );
    scene
        .app
        .world_mut()
        .write_message(super::super::hotbar::UtilityTrayRequest(true));
    closing_idle(&mut scene, 2);
    assert!(super::super::hotbar::tray_open_collision_fixture(
        scene.app.world()
    ));
    closing_raw_start(&mut scene, 750);
    assert!(!scene.app.world().resource::<ArmGesture>().captures());
    assert!(
        scene
            .app
            .world()
            .resource::<nvf_play::pipboy::touch_items::OverlayCapture>()
            .0
    );
}

#[test]
fn actual_phone_safe_closing_travel_reaches_full_pose_from_center_and_handle_edges_without_entering_inset(
) {
    for view in [UVec2::new(640, 360), UVec2::new(792, 363)] {
        for edge in 0..5 {
            for canceled in [false, true] {
                let mut scene = closing_scene_at(view);
                let size = closing_size(&scene);
                let safe = closing_safe_bottom(&scene);
                let handle = super::super::drawer::handle(size, safe);
                let center = handle.center();
                let origin = match edge {
                    0 => center,
                    1 => Vec2::new(handle.min.x + 1., center.y),
                    2 => Vec2::new(handle.max.x - 1., center.y),
                    3 => Vec2::new(center.x, handle.min.y + 1.),
                    _ => Vec2::new(center.x, handle.max.y - 1.),
                };
                assert!(scene
                    .app
                    .world()
                    .resource::<super::super::layout::Layout>()
                    .config
                    .hit_filtered(origin, size, false, false, |_| true)
                    .is_none());
                let travel = closing_travel(size, safe, origin).unwrap();
                assert!(travel > 0. && origin.y + travel <= size.y - safe.max(0.));
                closing_edge(&mut scene, 760, TouchPhase::Started, origin);
                scene.app.update();
                assert_eq!(
                    scene.app.world().resource::<ArmGesture>().progress(),
                    Some(1.)
                );
                for (part, expected) in [(0.25, 0.75), (0.75, 0.25), (0.5, 0.5), (1., 0.)] {
                    closing_edge(
                        &mut scene,
                        760,
                        TouchPhase::Moved,
                        origin + Vec2::Y * (travel * part),
                    );
                    scene.app.update();
                    assert!((scene.app.world().resource::<ArmGesture>().progress().unwrap()-expected).abs()<1e-5,
                        "{view:?}/{edge}: actual raw pose must reverse and reach zero before the safe bottom");
                }
                closing_edge(
                    &mut scene,
                    760,
                    if canceled {
                        TouchPhase::Canceled
                    } else {
                        TouchPhase::Ended
                    },
                    origin + Vec2::Y * travel,
                );
                scene.app.update();
                closing_edge(&mut scene, 760, TouchPhase::Moved, origin);
                scene.app.update();
                assert_eq!(
                    scene.app.world().resource::<ArmGesture>().progress(),
                    Some(0.),
                    "ended/canceled raw owner cannot rearm"
                );
                assert!(
                    scene.app.world().resource::<ArmGesture>().captures(),
                    "real core still owns endpoint settlement"
                );
                assert!(!scene
                    .app
                    .world()
                    .resource::<nvf_play::pad::PlatformInputGate>()
                    .allows_actions());
            }
        }
    }
}

#[test]
fn closing_budget_rejects_nonfinite_and_nonpositive_metadata_before_division() {
    // Invalid metadata guards, not synthetic raw TouchInput outside an admitted area.
    let size = Vec2::new(640., 360.);
    for (size, safe, origin) in [
        (size, 24., Vec2::new(320., 336.)),
        (size, 24., Vec2::new(320., 340.)),
        (size, f32::NAN, Vec2::new(320., 300.)),
        (size, f32::INFINITY, Vec2::new(320., 300.)),
        (Vec2::new(f32::INFINITY, 360.), 24., Vec2::new(320., 300.)),
        (Vec2::new(640., f32::NAN), 24., Vec2::new(320., 300.)),
        (size, 24., Vec2::new(f32::NAN, 300.)),
        (size, 400., Vec2::new(320., 0.)),
    ] {
        assert_eq!(closing_travel(size, safe, origin), None);
    }
    assert_eq!(closing_travel(size, 24., Vec2::new(320., 300.)), Some(36.));
}

#[test]
fn actual_parent_changed_safe_inset_cancels_old_closing_range_without_replaying_raw_owner() {
    let mut scene = closing_scene_at(UVec2::new(640, 360));
    let point = closing_handle(&scene);
    let travel = closing_travel(closing_size(&scene), closing_safe_bottom(&scene), point).unwrap();
    closing_raw_start(&mut scene, 770);
    closing_edge(
        &mut scene,
        770,
        TouchPhase::Moved,
        point + Vec2::Y * (travel * 0.25),
    );
    scene.app.update();
    assert_eq!(
        scene.app.world().resource::<ArmGesture>().progress(),
        Some(0.75)
    );
    scene
        .app
        .world_mut()
        .resource_mut::<super::super::drawer::SafeBottom>()
        .0 += 8.;
    closing_edge(
        &mut scene,
        770,
        TouchPhase::Moved,
        point + Vec2::Y * (travel * 0.5),
    );
    scene.app.update();
    closing_edge(
        &mut scene,
        770,
        TouchPhase::Canceled,
        point + Vec2::Y * (travel * 0.5),
    );
    scene.app.update();
    assert_eq!(
        scene.app.world().resource::<ArmGesture>().progress(),
        Some(0.75)
    );
    assert!(scene.app.world().resource::<ArmGesture>().captures());
    assert!(
        !scene
            .app
            .world()
            .resource::<nvf_play::control::HeldSneakInput>()
            .active
    );
}

#[test]
fn actual_parent_opening_keeps_the_real_saved_distance_choice_instead_of_downward_budget() {
    let (mut app, window) = scene();
    let row = app
        .world()
        .resource::<nvf_play::settings::Settings>()
        .index("touch-pipboy-distance-v1")
        .unwrap();
    nvf_play::settings::set_slider(
        app.world_mut(),
        row,
        nvf_play::settings::choice_part(24, 39),
    )
    .unwrap();
    assert_eq!(
        app.world()
            .resource::<super::super::drawer::Options>()
            .threshold,
        144.
    );
    let size = Vec2::splat(1000.);
    let safe = app.world().resource::<super::super::drawer::SafeBottom>().0;
    let point = super::super::drawer::handle(size, safe).center();
    for (phase, dy, expected) in [
        (TouchPhase::Started, 0., 0.),
        (TouchPhase::Moved, -72., 0.5),
        (TouchPhase::Moved, -36., 0.25),
    ] {
        let position = point + Vec2::Y * dy;
        assert!(
            position.x >= 0.
                && position.x <= size.x
                && position.y >= 0.
                && position.y <= size.y - safe
        );
        app.world_mut().write_message(TouchInput {
            window,
            id: 780,
            phase,
            position,
            force: None,
        });
        app.update();
        assert_eq!(
            app.world().resource::<ArmGesture>().progress(),
            Some(expected)
        );
    }
    app.world_mut().write_message(TouchInput {
        window,
        id: 780,
        phase: TouchPhase::Canceled,
        position: point - Vec2::Y * 36.,
        force: None,
    });
    app.update();
    assert_eq!(app.world().resource::<ArmGesture>().progress(), Some(0.25));
}
