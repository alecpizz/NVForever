//! Real projection, picker, physics and native activation outcomes; authored data only.
use super::*;
use crate::{
    activate::{RefVisual, Target},
    sim::SimState,
};
use bevy::{
    camera::primitives::Aabb,
    camera::{CameraProjection, RenderTargetInfo},
    ecs::message::Messages,
    window::WindowResolution,
};
use fnv_sim::FormId;

fn scene(colliders: &[(u32, f32)], eye: [f32; 3], dir: [f32; 3]) -> (App, Entity, Entity) {
    let mut app = crate::activate::tests::scene(colliders);
    app.init_resource::<TouchActivation>()
        .init_resource::<Time<Real>>();
    let window = app
        .world_mut()
        .spawn((
            Window {
                resolution: WindowResolution::new(1000, 1000),
                ..default()
            },
            PrimaryWindow,
        ))
        .id();
    let mut camera = Camera::default();
    let projection = PerspectiveProjection {
        aspect_ratio: 1.0,
        ..default()
    };
    camera.computed.clip_from_view = projection.get_clip_from_view();
    camera.computed.target_info = Some(RenderTargetInfo {
        physical_size: UVec2::splat(1000),
        scale_factor: 1.0,
    });
    let transform = Transform::from_translation(crate::space::to_bevy(eye))
        .looking_to(crate::space::to_bevy(dir), Vec3::Y);
    let camera = app
        .world_mut()
        .spawn((
            camera,
            GlobalTransform::from(transform),
            RenderTarget::Window(WindowRef::Primary),
            crate::PlayerCamera,
        ))
        .id();
    (app, window, camera)
}
fn visual(app: &mut App, id: u32, at: [f32; 3]) -> Entity {
    app.world_mut()
        .spawn((
            RefVisual(id),
            Aabb {
                center: Vec3::ZERO.into(),
                half_extents: Vec3::splat(2.).into(),
            },
            GlobalTransform::from_translation(crate::space::to_bevy(at)),
            InheritedVisibility::VISIBLE,
            Visibility::Inherited,
        ))
        .id()
}
fn point(app: &App, window: Entity, camera: Entity, at: [f32; 3]) -> ScreenPoint {
    let p = app
        .world()
        .get::<Camera>(camera)
        .unwrap()
        .world_to_viewport(
            app.world().get::<GlobalTransform>(camera).unwrap(),
            crate::space::to_bevy(at),
        )
        .unwrap();
    ScreenPoint {
        window,
        id: 77,
        position: p,
    }
}
fn begin(app: &mut App, p: ScreenPoint, mode: Confirmation) {
    let mut touch = app.world_mut().resource_mut::<TouchActivation>();
    touch.configure(mode);
    touch.begin(p, None);
    app.update();
}
fn confirm(app: &mut App) {
    app.world_mut().resource_mut::<TouchActivation>().end(true);
    app.update();
}
fn drain<M: Message>(app: &mut App) -> Vec<M> {
    app.world_mut()
        .resource_mut::<Messages<M>>()
        .drain()
        .collect()
}
fn count(app: &App) -> i32 {
    app.world()
        .resource::<SimState>()
        .world
        .peek(FormId::PLAYER_REF)
        .and_then(|r| r.inventory.as_ref())
        .map_or(0, |i| i.count(FormId(0x100)))
}

#[test]
fn upstream_hidden_target_policy_cancels_touch_confirmation_until_a_fresh_contact() {
    let target = |action| Targeted {
        form_id: 0xF305,
        name: "Authored target".into(),
        action,
        distance: 40.0,
        source: crate::activate::pick::Source::Bounds,
        crime: false,
        empty: false,
        through: None,
    };
    let point = ScreenPoint { window: Entity::PLACEHOLDER, id: 801, position: Vec2::splat(100.0) };
    for mode in [Confirmation::Tap, Confirmation::Hold] {
        let mut touch = TouchActivation::default();
        touch.configure(mode);
        touch.begin(point, None);
        touch.publish(Some(target(crate::activate::Action::Hostile)), 0.1);
        touch.end(true);
        for _ in 0..8 {
            touch.publish(Some(target(crate::activate::Action::Talk)), 0.1);
            assert!(touch.target().is_none());
            assert!(touch.ready.is_none());
            assert_eq!(touch.progress(), 0.0);
        }
        touch.begin(ScreenPoint { id: 802, ..point }, None);
        touch.publish(Some(target(crate::activate::Action::Talk)), 0.1);
        if mode == Confirmation::Tap {
            touch.end(true);
            touch.publish(Some(target(crate::activate::Action::Talk)), 0.1);
        } else {
            for _ in 0..6 { touch.publish(Some(target(crate::activate::Action::Talk)), 0.1); }
        }
        assert_eq!(touch.ready.as_ref().map(|t| t.form_id), Some(0xF305));
    }
}

#[test]
fn off_reticle_screen_pick_takes_actual_stack_once_without_fire_or_native_target_replacement() {
    let (mut app, w, c) = scene(&[], [0., 0., 100.], [1., 0., 0.]);
    visual(&mut app, 0x301, [80., 20., 100.]);
    let p = point(&app, w, c, [80., 20., 100.]);
    assert!(p.position.distance(Vec2::splat(500.)) > 30.);
    begin(&mut app, p, Confirmation::Tap);
    assert!(
        app.world().resource::<Target>().0.is_none(),
        "screen selection must not replace script crosshair targeting"
    );
    assert_eq!(
        app.world()
            .resource::<TouchActivation>()
            .target()
            .unwrap()
            .form_id,
        0x301
    );
    confirm(&mut app);
    assert_eq!(count(&app), 3);
    assert_eq!(drain::<crate::world_sounds::Activated>(&mut app).len(), 1);
    for _ in 0..4 {
        app.update();
    }
    assert_eq!(count(&app), 3);
    assert!(drain::<crate::world_sounds::Activated>(&mut app).is_empty());
    assert!(!app
        .world()
        .resource::<ButtonInput<MouseButton>>()
        .pressed(MouseButton::Left));
}

#[test]
fn direct_activation_revalidates_native_lock_key_and_control_lock() {
    let (mut app, w, c) = scene(&[(0x303, 80.)], [0., 0., 100.], [1., 0., 0.]);
    let p = point(&app, w, c, [80., 0., 100.]);
    begin(&mut app, p, Confirmation::Tap);
    confirm(&mut app);
    assert_eq!(
        drain::<crate::lockpick::OpenLockpick>(&mut app)
            .iter()
            .map(|m| m.reference)
            .collect::<Vec<_>>(),
        vec![0x303]
    );
    assert!(drain::<crate::OpenContainer>(&mut app).is_empty());
    app.world_mut()
        .resource_mut::<SimState>()
        .world
        .add_item(FormId::PLAYER_REF, FormId(0x101), 1)
        .unwrap();
    begin(&mut app, p, Confirmation::Tap);
    confirm(&mut app);
    assert_eq!(drain::<crate::OpenContainer>(&mut app).len(), 1);
    app.insert_resource(crate::control::ControlLocks {
        movement: true,
        ..default()
    });
    begin(&mut app, p, Confirmation::Tap);
    confirm(&mut app);
    assert!(drain::<crate::OpenContainer>(&mut app).is_empty());
    assert!(app.world().resource::<TouchActivation>().point().is_none());
}

#[test]
fn camera_peek_range_and_foreign_window_cannot_bypass_player_los() {
    let cases = [
        (
            vec![(0xDEAD, 40.), (0x302, 100.)],
            [-80., 80., 100.],
            [180., -80., 0.],
            [100., 0., 100.],
        ),
        (
            vec![(0x302, 500.)],
            [0., 0., 100.],
            [1., 0., 0.],
            [500., 0., 100.],
        ),
    ];
    for (colliders, eye, dir, target) in cases {
        let (mut app, w, c) = scene(&colliders, eye, dir);
        let p = point(&app, w, c, target);
        begin(&mut app, p, Confirmation::Tap);
        confirm(&mut app);
        assert!(app.world().resource::<TouchActivation>().target().is_none());
        assert!(drain::<crate::OpenContainer>(&mut app).is_empty());
        let foreign = ScreenPoint {
            window: Entity::PLACEHOLDER,
            ..p
        };
        begin(&mut app, foreign, Confirmation::Tap);
        confirm(&mut app);
        assert!(drain::<crate::OpenContainer>(&mut app).is_empty());
    }
}

#[test]
fn hold_loss_never_rearms_until_new_contact_and_fresh_hold_executes_once() {
    let (mut app, w, c) = scene(&[], [0., 0., 100.], [1., 0., 0.]);
    #[derive(Resource, Default)]
    struct Heard(Vec<u32>);
    fn hear(mut events: MessageReader<crate::world_sounds::Activated>, mut heard: ResMut<Heard>) {
        heard.0.extend(events.read().map(|event| event.reference));
    }
    // The fixture schedules activate directly, not inside PlaySet::Activation.
    // Observe every update after the actual producer so a completed long hold
    // cannot age its message out before the eventual assertion.
    app.init_resource::<Heard>()
        .add_systems(Update, hear.after(crate::activate::activate));
    let entity = visual(&mut app, 0x301, [80., 20., 100.]);
    let p = point(&app, w, c, [80., 20., 100.]);
    begin(&mut app, p, Confirmation::Hold);
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(std::time::Duration::from_millis(100));
    for _ in 0..3 {
        app.update();
    }
    assert!(app.world().resource::<TouchActivation>().progress() > 0.);
    *app.world_mut()
        .get_mut::<InheritedVisibility>(entity)
        .unwrap() = InheritedVisibility::HIDDEN;
    app.update();
    *app.world_mut()
        .get_mut::<InheritedVisibility>(entity)
        .unwrap() = InheritedVisibility::VISIBLE;
    for _ in 0..10 {
        app.update();
    }
    assert_eq!(count(&app), 0);
    assert!(app.world().resource::<Heard>().0.is_empty(), "the invalidated old contact emits no activation");
    begin(&mut app, p, Confirmation::Hold);
    for _ in 0..8 {
        app.update();
    }
    assert_eq!(count(&app), 3);
    assert_eq!(app.world().resource::<Heard>().0.len(), 1);
    assert_eq!(app.world().resource::<Heard>().0, vec![0x301]);
    assert_eq!(app.world().resource::<TouchActivation>().progress(), 0.);
    app.world_mut().resource_mut::<TouchActivation>().end(true);
    for _ in 0..10 {
        app.update();
    }
    assert_eq!(count(&app), 3, "a completed hold and its later release cannot take twice");
    assert_eq!(app.world().resource::<Heard>().0.len(), 1);
    assert_eq!(app.world().resource::<Heard>().0, vec![0x301]);
}

#[test]
fn remapped_script_block_during_hold_requires_new_contact_after_controls_return() {
    let (mut app, w, c) = scene(&[], [0., 0., 100.], [1., 0., 0.]);
    visual(&mut app, 0x301, [80., 20., 100.]);
    app.world_mut()
        .resource_mut::<crate::bindings::Bindings>()
        .rebind(
            crate::bindings::Control::Activate,
            crate::bindings::Input::Key(KeyCode::KeyK),
        );
    let p = point(&app, w, c, [80., 20., 100.]);
    begin(&mut app, p, Confirmation::Hold);
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(std::time::Duration::from_millis(100));
    app.update();
    app.insert_resource(crate::script_input::ScriptBlocked {
        keys: vec![KeyCode::KeyK],
        mouse: vec![],
    });
    app.update();
    assert!(app.world().resource::<TouchActivation>().point().is_none());
    app.world_mut()
        .resource_mut::<crate::script_input::ScriptBlocked>()
        .keys
        .clear();
    for _ in 0..10 {
        app.update();
    }
    assert_eq!(count(&app), 0);
    begin(&mut app, p, Confirmation::Tap);
    confirm(&mut app);
    assert_eq!(count(&app), 3);
    assert_eq!(drain::<crate::world_sounds::Activated>(&mut app).len(), 1);
}
