use super::*;
use bevy::input::{
    keyboard::{Key, KeyboardInput},
    ButtonState,
};

fn app() -> App {
    let mut app = crate::activate::tests::scene(&[]);
    app.world_mut().resource_mut::<Schedules>().remove(Update);
    app.add_plugins((bevy::state::app::StatesPlugin, bevy::input::InputPlugin))
        .init_state::<PlayState>()
        .init_resource::<Time>()
        .insert_resource(crate::pause::PlayClock::frozen_frame(0.05))
        .init_resource::<PipBoy>()
        .init_resource::<PlayerControl>()
        .init_resource::<super::super::UsePipBoy>()
        .init_resource::<ArmGesture>()
        .init_resource::<crate::pad::PlatformInputGate>();
    // Authored wearable, using the existing authored simulation fixture's item fields.
    let old = app
        .world_mut()
        .remove_resource::<crate::sim::SimState>()
        .unwrap();
    let mut forms = fnv_sim::FormStore::default();
    forms.bases = old.forms.bases.clone();
    let mut device = forms.bases[&fnv_sim::FormId(0x100)].clone();
    device.id = fnv_sim::FormId(0xF200);
    device.name = Some("Authored wrist device".into());
    let fnv_sim::forms::BaseData::Item(item) = &mut device.data else {
        panic!("authored item")
    };
    item.kind = fnv_sim::forms::ItemKind::Armor;
    item.slots = Some(1 << 6);
    forms.bases.insert(device.id, device);
    let forms = Box::leak(Box::new(forms));
    let mut sim = crate::sim::SimState {
        forms,
        refs: old.refs,
        world: fnv_sim::World::new(forms, old.refs, fnv_sim::SimConfig::default()),
    };
    sim.world
        .add_item(fnv_sim::FormId::PLAYER_REF, fnv_sim::FormId(0xF200), 1)
        .unwrap();
    sim.world
        .equip(fnv_sim::FormId::PLAYER_REF, fnv_sim::FormId(0xF200))
        .unwrap();
    assert!(crate::pipboy::wears_pipboy(&sim));
    app.insert_resource(sim);
    app.world_mut().resource_mut::<PlayerControl>().pov = Pov::Third;
    crate::menu::add_open_and_keys(&mut app, requests);
    app.add_systems(
        Update,
        (super::super::follow_menu, drive, super::super::follow_frame)
            .chain()
            .after(MenuSystems),
    )
    .add_systems(PreUpdate, cancel_hardware.after(bevy::input::InputSystems));
    app
}
fn begin(app: &mut App, open: bool) {
    let model = Entity::from_raw_u32(900).unwrap();
    assert!(app
        .world_mut()
        .resource_mut::<ArmGesture>()
        .begin(open, model));
    app.world_mut()
        .resource_mut::<crate::pad::PlatformInputGate>()
        .set_arm_blocked(true);
}
fn settle(app: &mut App) {
    for _ in 0..24 {
        app.update();
        if !app.world().resource::<ArmGesture>().captures() {
            return;
        }
    }
    panic!("gesture must settle or fail closed through native menu admission");
}
#[test]
fn direct_preview_is_reversible_and_does_not_report_or_open_a_native_menu() {
    let mut app = app();
    app.world_mut().resource_mut::<PlayerControl>().orbit.yaw = 0.4;
    begin(&mut app, false);
    app.world_mut().resource_mut::<ArmGesture>().drag(0.75);
    app.update();
    let high = app.world().resource::<PipBoy>().clip_time;
    assert!((high - HOLD_SECONDS * 0.75).abs() < 1e-6);
    assert!(!app.world().resource::<PipBoy>().open());
    assert!(!app.world().resource::<PipBoy>().opening());
    assert!(app.world().resource::<ActiveMenu>().0.is_none());
    app.world_mut().resource_mut::<ArmGesture>().drag(0.25);
    app.update();
    assert!(app.world().resource::<PipBoy>().clip_time < high);
    assert_eq!(app.world().resource::<PlayerControl>().pov, Pov::First);
    app.world_mut().resource_mut::<ArmGesture>().release();
    settle(&mut app);
    assert_eq!(app.world().resource::<PipBoy>().phase, Phase::Down);
    assert_eq!(app.world().resource::<PlayerControl>().pov, Pov::Third);
    assert_eq!(app.world().resource::<PlayerControl>().orbit.yaw, 0.4);
    assert!(!app.world().resource::<PlayerControl>().orbit.held);
    assert!(app.world().resource::<ActiveMenu>().0.is_none());
    assert!(app
        .world()
        .resource::<crate::pad::PlatformInputGate>()
        .allows_actions());
}
#[test]
fn actual_native_inventory_opens_and_closes_only_after_arm_settle() {
    let mut app = app();
    begin(&mut app, false);
    app.world_mut().resource_mut::<ArmGesture>().drag(0.8);
    app.update();
    assert!(app.world().resource::<ActiveMenu>().0.is_none());
    app.world_mut().resource_mut::<ArmGesture>().release();
    settle(&mut app);
    app.update();
    assert_eq!(
        *app.world().resource::<State<PlayState>>().get(),
        PlayState::Menu
    );
    assert!(inventory(app.world().resource::<ActiveMenu>()));
    assert_eq!(app.world().resource::<PipBoy>().phase, Phase::Up);
    assert_eq!(app.world().resource::<PipBoy>().clip_time, HOLD_SECONDS);
    begin(&mut app, true);
    app.world_mut().resource_mut::<ArmGesture>().drag(0.2);
    app.update();
    assert!(inventory(app.world().resource::<ActiveMenu>()));
    app.world_mut().resource_mut::<ArmGesture>().release();
    settle(&mut app);
    app.update();
    assert_eq!(
        *app.world().resource::<State<PlayState>>().get(),
        PlayState::Playing
    );
    assert!(app.world().resource::<ActiveMenu>().0.is_none());
    assert_eq!(app.world().resource::<PipBoy>().phase, Phase::Down);
    assert_eq!(app.world().resource::<PlayerControl>().pov, Pov::Third);
}
#[test]
fn native_pipboy_script_lock_refuses_open_and_preview_settles_back_down() {
    let mut app = app();
    app.insert_resource(crate::control::ControlLocks {
        pipboy: true,
        ..default()
    });
    begin(&mut app, false);
    app.world_mut().resource_mut::<ArmGesture>().drag(0.9);
    app.world_mut().resource_mut::<ArmGesture>().release();
    settle(&mut app);
    assert!(app.world().resource::<ActiveMenu>().0.is_none());
    assert_eq!(app.world().resource::<PipBoy>().phase, Phase::Down);
    assert_eq!(app.world().resource::<PlayerControl>().pov, Pov::Third);
}
#[test]
fn real_back_cancels_preview_and_native_loading_keeps_its_independent_gate() {
    let mut app = app();
    begin(&mut app, false);
    app.world_mut().resource_mut::<ArmGesture>().drag(0.9);
    app.update();
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::Escape,
        logical_key: Key::Escape,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
    app.update();
    settle(&mut app);
    assert!(app
        .world()
        .resource::<ButtonInput<KeyCode>>()
        .pressed(KeyCode::Escape));
    assert!(app.world().resource::<ActiveMenu>().0.is_none());
    begin(&mut app, false);
    app.world_mut().resource_mut::<ArmGesture>().drag(0.7);
    app.update();
    app.world_mut()
        .resource_mut::<crate::pad::PlatformInputGate>()
        .set_blocked(true);
    app.update();
    assert!(!app.world().resource::<ArmGesture>().captures());
    assert_eq!(app.world().resource::<PipBoy>().phase, Phase::Down);
    let gate = app.world().resource::<crate::pad::PlatformInputGate>();
    assert!(gate.native_blocked());
    assert!(!gate.allows_actions());
}
