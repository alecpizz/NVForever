use super::*;
use bevy::ecs::system::SystemState;
use fnv_sim::{FormId, FormStore, SimConfig};

fn scene() -> (App, Entity) {
    let (mut app, window, _, _) = super::super::world_outcome_tests::scene("Tap");
    let old = app
        .world_mut()
        .remove_resource::<nvf_play::sim::SimState>()
        .unwrap();
    let mut forms = FormStore::default();
    forms.bases = old.forms.bases.clone();
    let item = fnv_sim::forms::Item {
        kind: fnv_sim::forms::ItemKind::Armor,
        value: None,
        money: None,
        weight: None,
        slots: Some(1 << 6),
        projectile: None,
        health: None,
        non_playable: false,
        cant_drop: false,
        armor_class: default(),
        power_armor: false,
        armor_dr: None,
        armor_dt: None,
        weapon_damage: None,
        weight_mods: [0.; 3],
        drawable: false,
        not_for_npcs: false,
        thrown: false,
        quest_item: false,
        icon: None,
        placed_count: None,
        teaches: None,
        cant_be_taken: false,
        modulates_voice: false,
        weapon_traits: None,
    };
    let id = FormId(0xF201);
    forms.bases.insert(
        id,
        fnv_sim::forms::BaseForm {
            id,
            sig: *b"ARMO",
            edid: None,
            name: Some("Owned wrist".into()),
            script: None,
            prompt: None,
            voice_filter: false,
            data: fnv_sim::forms::BaseData::Item(item),
        },
    );
    let forms = Box::leak(Box::new(forms));
    let mut sim = nvf_play::sim::SimState {
        forms,
        refs: old.refs,
        world: fnv_sim::World::new(forms, old.refs, SimConfig::default()),
    };
    sim.world.add_item(FormId::PLAYER_REF, id, 1).unwrap();
    sim.world.equip(FormId::PLAYER_REF, id).unwrap();
    app.insert_resource(sim)
        .init_resource::<PipBoy>()
        .init_resource::<UsePipBoy>()
        .init_resource::<nvf_play::pad::PlatformInputGate>();
    let root = app.world_mut().spawn(Transform::IDENTITY).id();
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
    app.world_mut()
        .resource_mut::<super::super::drawer::Options>()
        .enabled = true;
    (app, window)
}
#[test]
fn actual_arm_bridge_loses_to_projected_controls_and_aborts_after_model_replacement() {
    let (mut app, window) = scene();
    let size = Vec2::splat(1000.);
    let point = super::super::drawer::handle(
        size,
        app.world().resource::<super::super::drawer::SafeBottom>().0,
    )
    .center();
    let edge = |phase, position| TouchInput {
        window,
        id: 71,
        phase,
        position,
        force: None,
    };
    let mut system =
        SystemState::<(Bridge, Res<nvf_play::pad::PlatformInputGate>)>::new(app.world_mut());
    {
        let (mut bridge, gate) = system.get_mut(app.world_mut()).expect("arm bridge fixture resources");
        assert!(
            !bridge.begin(&edge(TouchPhase::Started, point), size, false, Some(&gate)),
            "an actual painted header/button/scrollbar takes precedence over the closing handle"
        );
        assert!(!bridge.captures());
        assert!(bridge.begin(&edge(TouchPhase::Started, point), size, true, Some(&gate)));
        bridge.event(&edge(TouchPhase::Moved, point - Vec2::new(0., 48.)), size);
    }
    system.apply(app.world_mut());
    assert!(!app
        .world()
        .resource::<nvf_play::pad::PlatformInputGate>()
        .allows_actions());
    let progress = app.world().resource::<ArmGesture>().progress().unwrap();
    assert!((progress - 0.5).abs() < 0.0001);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyW);
    app.insert_resource(crate::player_body::PlayerBody::default());
    {
        let (mut bridge, gate) = system.get_mut(app.world_mut()).expect("arm bridge fixture resources");
        bridge.prepare(true, Some(&gate));
        bridge.event(&edge(TouchPhase::Moved, point - Vec2::new(0., 90.)), size);
    }
    system.apply(app.world_mut());
    assert_eq!(
        app.world().resource::<ArmGesture>().progress(),
        Some(progress),
        "a replaced rig cannot keep dragging the old pose"
    );
    assert!(app
        .world()
        .resource::<ButtonInput<KeyCode>>()
        .pressed(KeyCode::KeyW));
    assert!(
        !app.world()
            .resource::<nvf_play::pad::PlatformInputGate>()
            .allows_actions(),
        "native pose restoration, not the wrapper, releases capture"
    );
}

#[test]
fn actual_up_inventory_closing_handle_requires_unclaimed_region_and_native_script_permission() {
    let (mut app, window) = scene();
    let model = {
        let mut sim = app.world_mut().resource_mut::<nvf_play::sim::SimState>();
        nvf_play::menu::model::MenuModel::open_inventory(&mut sim.world).unwrap()
    };
    app.insert_resource(nvf_play::menu::ActiveMenu(Some(model)));
    app.world_mut().resource_mut::<PipBoy>().phase = Phase::Up;
    app.insert_resource(nvf_play::control::ControlLocks {
        pipboy: true,
        ..default()
    });
    let size = Vec2::splat(1000.);
    let point = super::super::drawer::handle(
        size,
        app.world().resource::<super::super::drawer::SafeBottom>().0,
    )
    .center();
    let travel=closing_travel(size,app.world().resource::<super::super::drawer::SafeBottom>().0,point).unwrap();
    let edge = TouchInput {
        window,
        id: 72,
        phase: TouchPhase::Started,
        position: point,
        force: None,
    };
    let mut system =
        SystemState::<(Bridge, Res<nvf_play::pad::PlatformInputGate>)>::new(app.world_mut());
    {
        let (mut bridge, gate) = system.get_mut(app.world_mut()).expect("arm bridge fixture resources");
        assert!(
            !bridge.begin(&edge, size, true, Some(&gate)),
            "native Pip-Boy lock also blocks preview"
        );
    }
    app.world_mut()
        .resource_mut::<nvf_play::control::ControlLocks>()
        .pipboy = false;
    {
        let (mut bridge, gate) = system.get_mut(app.world_mut()).expect("arm bridge fixture resources");
        assert!(bridge.inventory());
        assert!(!bridge.begin(&edge, size, false, Some(&gate)));
        assert!(bridge.begin(&edge, size, true, Some(&gate)));
        bridge.event(
            &TouchInput {
                phase: TouchPhase::Moved,
                position: point + Vec2::new(0., travel*0.5),
                ..edge
            },
            size,
        );
    }
    system.apply(app.world_mut());
    assert_eq!(app.world().resource::<ArmGesture>().progress(), Some(0.5));
    assert!(!app
        .world()
        .resource::<nvf_play::pad::PlatformInputGate>()
        .allows_actions());
}

#[test]
fn sole_parent_raw_arm_owner_cancels_on_focus_loss_without_replaying_tail_or_clearing_hardware() {
    let (mut app, window) = scene();
    let size = Vec2::splat(1000.);
    let point = super::super::drawer::handle(
        size,
        app.world().resource::<super::super::drawer::SafeBottom>().0,
    )
    .center();
    assert!(app
        .world()
        .resource::<super::super::Layout>()
        .config
        .hit_filtered(point, size, true, false, |_| true)
        .is_none());
    let edge = |phase, position| TouchInput {
        window,
        id: 73,
        phase,
        position,
        force: None,
    };
    app.world_mut()
        .write_message(edge(TouchPhase::Started, point));
    app.world_mut()
        .write_message(edge(TouchPhase::Moved, point - Vec2::new(0., 48.)));
    app.update();
    let before = app.world().resource::<ArmGesture>().progress();
    assert_eq!(before, Some(0.5));
    assert!(
        !app.world()
            .resource::<nvf_play::control::HeldSneakInput>()
            .active
    );
    app.world_mut()
        .write_message(bevy::input::mouse::MouseButtonInput {
            window,
            button: MouseButton::Middle,
            state: bevy::input::ButtonState::Pressed,
        });
    app.update();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    app.world_mut().write_message(bevy::window::WindowFocused {
        window,
        focused: false,
    });
    app.world_mut()
        .write_message(edge(TouchPhase::Moved, point - Vec2::new(0., 90.)));
    app.world_mut()
        .write_message(edge(TouchPhase::Ended, point - Vec2::new(0., 90.)));
    app.update();
    assert_eq!(app.world().resource::<ArmGesture>().progress(), before);
    assert!(
        !app.world()
            .resource::<nvf_play::control::HeldSneakInput>()
            .active
    );
    assert!(app
        .world()
        .resource::<ButtonInput<MouseButton>>()
        .pressed(MouseButton::Middle));
    assert!(app
        .world()
        .resource::<super::super::TouchState>()
        .contacts
        .is_empty());
    assert!(app
        .world()
        .resource::<bevy::ecs::message::Messages<nvf_play::menu::OpenInventory>>()
        .is_empty());
}

include!("tests/closing_parent.rs");

include!("tests/optional_controls.rs");

include!("tests/palette_tests.rs");

include!("tests/diagonal_gestures.rs");
