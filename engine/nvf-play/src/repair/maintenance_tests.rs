//! Real repair-services keys/draw consumer, with the alternate maintenance presentation.
use super::*;
use crate::maintenance_touch::{Action, tests::{fixture, raw_tap, tap, count, GEAR}};
use bevy::ecs::system::RunSystemOnce;

const VENDOR: FormId = FormId(0xFA104);

fn vendor_app(native_configuration: bool) -> App {
    let mut app = fixture(false, native_configuration);
    app.add_plugins(bevy::state::app::StatesPlugin).insert_state(PlayState::Menu)
        .init_resource::<crate::pause::PlayClock>().init_resource::<crate::pad::PadGlyphs>()
        .init_resource::<Assets<Image>>().insert_resource(RepairAid(Vec::new()))
        .add_message::<RepairServicesInput>().add_message::<bevy::window::CursorMoved>()
        .insert_resource(RepairServices(Some(Open { vendor: VENDOR, selected: 0, first: 0, drawn: None, pointer_pick: false, navigated: 0, confirming: false })))
        .insert_resource(crate::pipboy::PipBoy::default()).insert_resource(crate::menu::ActiveMenu(None))
        .insert_resource(crate::pipboy::repair::PipRepair::default())
        .insert_resource(crate::tiles::vanilla::TileMenus { repair_services: native_configuration, ..default() })
        .add_systems(Update, (keys, draw).chain());
    app.world_mut().resource_mut::<SimState>().world.add_item(FormId::PLAYER_REF, CAPS, 1000).unwrap();
    app.world_mut().run_system_once(|mut sim: ResMut<SimState>, mut script: ResMut<ScriptState>| {
        crate::script::with_world(script.rt.as_mut().unwrap(), &mut sim, |rt| fnv_game::functions::set_actor_value_current(&mut rt.game, VENDOR, fnv_game::actor_values::REPAIR, 80.0).unwrap());
    }).unwrap();
    app.update(); app.update(); app.update();
    let open = app.world().resource::<RepairServices>().0.as_ref().unwrap();
    assert!(open.drawn.as_ref().is_some_and(|v| v.rows.len() == 2 && v.rows[0].ready));
    app
}

#[test]
fn actual_vendor_consumer_charges_the_painted_quote_once_and_cancel_is_free() {
    for native_configuration in [false, true] {
        let mut app = vendor_app(native_configuration);
        tap(&mut app, 90, Action::Select(0));
        assert_eq!(count(&app, CAPS), 1000, "selecting never repairs or charges");
        let before = app.world().resource::<RepairServices>().0.as_ref().unwrap().drawn.as_ref().unwrap().clone();
        let row = &before.rows[before.selected];
        let Quote::Repair { to, cost } = row.quote else { panic!("fixture must have a genuine affordable quote") };
        raw_tap(&mut app, 91, Action::Confirm); raw_tap(&mut app, 92, Action::Confirm); app.update();
        assert_eq!(count(&app, CAPS), 1000 - cost as i32, "duplicate same-paint confirms charge once");
        let world = &app.world().resource::<SimState>().world;
        assert_eq!(world.peek(VENDOR).unwrap().inventory.as_ref().unwrap().count(CAPS), cost as i32);
        assert!(world.peek(FormId::PLAYER_REF).unwrap().inventory.as_ref().unwrap().groups().any(|(item, extra, n)| item == GEAR && n == 1 && extra.condition == Some(to)));
        let charged = count(&app, CAPS);
        tap(&mut app, 93, Action::Cancel);
        assert_eq!(count(&app, CAPS), charged);
        let expected = after_close(&app.world().resource::<ScriptState>().rt.as_ref().unwrap().game);
        assert!(matches!(app.world().resource::<NextState<PlayState>>(), NextState::Pending(next) if *next == expected));
    }
}

#[test]
fn actual_vendor_confirmation_requotes_after_caps_or_unit_changes() {
    for change_unit in [false, true] {
        let mut app = vendor_app(false);
        let painted = app.world().resource::<RepairServices>().0.as_ref().unwrap().drawn.as_ref().unwrap().clone();
        raw_tap(&mut app, 95, Action::Confirm);
        let before_caps = if change_unit {
            let row = &painted.rows[painted.selected];
            assert!(app.world_mut().resource_mut::<SimState>().world.inventory_mut(FormId::PLAYER_REF).unwrap().set_condition_one(row.item, row.extra, 0.95));
            1000
        } else {
            assert_eq!(app.world_mut().resource_mut::<SimState>().world.inventory_mut(FormId::PLAYER_REF).unwrap().remove(CAPS, 999), 999);
            1
        };
        app.update();
        assert_eq!(count(&app, CAPS), before_caps);
        assert_eq!(app.world().resource::<SimState>().world.peek(VENDOR).and_then(|s| s.inventory.as_ref()).map_or(0, |i| i.count(CAPS)), 0);
        assert_ne!(app.world().resource::<RepairServices>().0.as_ref().unwrap().drawn.as_ref(), Some(&painted), "real quote model refreshes instead of predicting a charge");
    }
}

#[test]
fn actual_script_message_opened_after_vendor_raw_confirmation_keeps_caps_until_fresh_owner_confirmation() {
    let mut app = vendor_app(true);
    let quote = app.world().resource::<RepairServices>().0.as_ref().unwrap().drawn.as_ref().unwrap().rows[0].quote;
    let cost = quote.cost().unwrap();
    raw_tap(&mut app, 115, Action::Confirm);
    app.world_mut().resource_mut::<ScriptState>().rt.as_mut().unwrap().game.open_menu(fnv_game::game::menus::MESSAGE);
    app.update(); assert_eq!(count(&app, CAPS), 1000);
    {
        let mut script = app.world_mut().resource_mut::<ScriptState>();
        let game = &mut script.rt.as_mut().unwrap().game;
        game.close_menu(); game.open_menu(REPAIR_SERVICES_MENU);
    }
    tap(&mut app, 116, Action::Confirm);
    assert_eq!(count(&app, CAPS), 1000 - cost as i32);
}
