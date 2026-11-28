//! Scoped regression tests verifying `GetPipBoyMode` reports transition phases
//! (`0` closed, `2` raising, `3` up, `4` lowering) across the full Pip-Boy open/close
//! animation lifecycle so scripts do not hide `pipboyscreen:0` during dismissal, while
//! resting phases defer to live `Game::menu_open(menus::PIPBOY)` for same-frame toggles.

use super::*;
use bevy::ecs::system::RunSystemOnce;
use fnv_game::game::RunCtx;
use fnv_script::vm::XVal;

fn query_mode(world: &mut World) -> f64 {
    let mut state = world.resource_mut::<crate::script::ScriptState>();
    let rt = state.rt.as_mut().unwrap();
    match rt
        .game
        .run_query(
            "Begin Function {}\nSetFunctionValue (GetPipBoyMode)\nEnd",
            RunCtx::default(),
        )
        .unwrap()
    {
        XVal::Num(n) => n,
        other => panic!("GetPipBoyMode must return a number, got {other:?}"),
    }
}

#[test]
fn get_pipboy_mode_reports_raising_up_lowering_and_down_transitions() {
    let mut world = World::new();
    world.insert_resource(crate::script::ScriptState::over_no_plugins());
    world.insert_resource(PipBoy::default());

    // Initial state: closed (0).
    world.run_system_once(sync_pipboy_mode).unwrap();
    assert_eq!(query_mode(&mut world), 0.0);

    // Tab opens Pip-Boy: arm begins raising -> mode 2.
    world.resource_mut::<crate::script::ScriptState>().rt.as_mut().unwrap().game.open_menu(menus::INVENTORY);
    world.resource_mut::<PipBoy>().raise();
    assert_eq!(world.resource::<PipBoy>().phase, Phase::Raising);
    world.run_system_once(sync_pipboy_mode).unwrap();
    assert_eq!(query_mode(&mut world), 2.0);

    // Raise finishes at HOLD_SECONDS -> Phase::Up -> mode 3 (via live menu_open).
    world.resource_mut::<PipBoy>().advance(HOLD_SECONDS + 0.01);
    assert_eq!(world.resource::<PipBoy>().phase, Phase::Up);
    world.run_system_once(sync_pipboy_mode).unwrap();
    assert_eq!(query_mode(&mut world), 3.0);

    // Tab closes Pip-Boy: menu closes immediately while arm plays lowering clip -> mode 4
    // (must NOT drop straight to 0 while lowering!).
    world.resource_mut::<crate::script::ScriptState>().rt.as_mut().unwrap().game.close_menu();
    world.resource_mut::<PipBoy>().lower();
    assert_eq!(world.resource::<PipBoy>().phase, Phase::Lowering);
    world.run_system_once(sync_pipboy_mode).unwrap();
    assert_eq!(query_mode(&mut world), 4.0);

    // Mid-lowering frame still reports 4.
    world.resource_mut::<PipBoy>().advance(0.15);
    assert_eq!(world.resource::<PipBoy>().phase, Phase::Lowering);
    world.run_system_once(sync_pipboy_mode).unwrap();
    assert_eq!(query_mode(&mut world), 4.0);

    // Lowering clip completes -> Phase::Down -> mode 0.
    world.resource_mut::<PipBoy>().advance(CLIP_SECONDS);
    assert_eq!(world.resource::<PipBoy>().phase, Phase::Down);
    world.run_system_once(sync_pipboy_mode).unwrap();
    assert_eq!(query_mode(&mut world), 0.0);
}

#[test]
fn hidden_startup_warming_still_reports_closed_mode_zero() {
    let mut world = World::new();
    world.insert_resource(crate::script::ScriptState::over_no_plugins());
    world.insert_resource(PipBoy {
        phase: Phase::Raising,
        warming: Some(Warming {
            since: std::time::Instant::now(),
            frames: 0,
            asked_at: 0,
            covered: true,
            device: true,
            saved_frame: Frame::Device,
            saved_page: None,
            drawn: false,
        }),
        ..default()
    });
    world.run_system_once(sync_pipboy_mode).unwrap();
    assert_eq!(query_mode(&mut world), 0.0);
}

#[test]
fn same_frame_toggle_pipboy_updates_get_pipboy_mode_immediately_in_resting_phase() {
    let mut world = World::new();
    world.insert_resource(crate::script::ScriptState::over_no_plugins());
    world.insert_resource(PipBoy::default());

    // Sync in resting Phase::Down leaves `ui.pipboy_mode == None` so live script menu
    // toggles are reflected immediately on the same frame before the next sync runs.
    world.run_system_once(sync_pipboy_mode).unwrap();
    assert_eq!(query_mode(&mut world), 0.0);

    let mode_after_toggle = {
        let mut state = world.resource_mut::<crate::script::ScriptState>();
        let rt = state.rt.as_mut().unwrap();
        match rt
            .game
            .run_query(
                "Begin Function {}\nTogglePipBoy 1003\nSetFunctionValue (GetPipBoyMode)\nEnd",
                RunCtx::default(),
            )
            .unwrap()
        {
            XVal::Num(n) => n,
            other => panic!("GetPipBoyMode must return a number, got {other:?}"),
        }
    };
    assert_eq!(mode_after_toggle, 3.0);
}
