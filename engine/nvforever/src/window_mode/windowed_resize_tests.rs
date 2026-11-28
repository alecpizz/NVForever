//! Scoped regression tests verifying that `window_mode::apply_display` does not snap a
//! user-resized or maximized windowed surface back to `--size` when `MonitorModes`
//! refreshes, while still applying explicit `--size` changes and full-screen exits.

use super::*;
use bevy::window::WindowResolution;
use clap::Parser;

fn test_app(initial_size: &str, physical: UVec2) -> App {
    let mut app = App::new();
    let args = Args::parse_from(["nvforever", "--window", "windowed", "--size", initial_size]);
    app.insert_resource(args)
        .init_resource::<FullScreenSize>()
        .init_resource::<MonitorChoice>()
        .init_resource::<MonitorModes>()
        .add_systems(Update, apply_display);
    let w = Window {
        mode: WindowMode::Windowed,
        resolution: WindowResolution::new(physical.x, physical.y),
        ..default()
    };
    app.world_mut().spawn((w, PrimaryWindow));
    app
}

fn primary_physical(app: &mut App) -> UVec2 {
    let world = app.world_mut();
    world.query_filtered::<&Window, With<PrimaryWindow>>().single(world).unwrap().resolution.physical_size()
}

#[test]
fn monitor_refresh_preserves_user_maximized_window_size() {
    let mut app = test_app("1360x768", UVec2::new(1360, 768));
    // Initial apply sets baseline state.
    app.update();
    assert_eq!(primary_physical(&mut app), UVec2::new(1360, 768));

    // User maximizes the window on an ultrawide display.
    {
        let world = app.world_mut();
        let mut w = world.query_filtered::<&mut Window, With<PrimaryWindow>>().single_mut(world).unwrap();
        w.resolution.set_physical_resolution(3440, 1369);
    }
    assert_eq!(primary_physical(&mut app), UVec2::new(3440, 1369));

    // A subsequent MonitorModes change (e.g. monitor discovery/refresh) must NOT snap
    // the maximized window back to 1360x768.
    app.world_mut().resource_mut::<MonitorModes>().desktop = Some(UVec2::new(3440, 1440));
    app.update();
    assert_eq!(primary_physical(&mut app), UVec2::new(3440, 1369));

    // Explicit settings change to --size DOES resize the window.
    app.world_mut().resource_mut::<Args>().size = "1920x1080".to_string();
    app.update();
    assert_eq!(primary_physical(&mut app), UVec2::new(1920, 1080));
}

#[test]
fn leaving_borderless_fullscreen_restores_windowed_size() {
    let mut app = test_app("1360x768", UVec2::new(1360, 768));
    app.update();

    // Enter borderless fullscreen and simulate the compositor resizing the surface.
    app.world_mut().resource_mut::<Args>().window = WindowChoice::Borderless;
    app.update();
    {
        let world = app.world_mut();
        let mut w = world.query_filtered::<&mut Window, With<PrimaryWindow>>().single_mut(world).unwrap();
        w.resolution.set_physical_resolution(800, 600);
    }

    // Returning to Windowed mode restores --size (1360x768).
    app.world_mut().resource_mut::<Args>().window = WindowChoice::Windowed;
    app.update();
    assert_eq!(primary_physical(&mut app), UVec2::new(1360, 768));
}
