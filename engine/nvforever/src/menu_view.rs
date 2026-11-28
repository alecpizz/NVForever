//! The main menu draws no 3D (issue #4918).
//!
//! The game opens as a new game in Doc Mitchell's house with the main menu over it
//! (`nvf_play::main_menu`), held as under the pause menu. The menu's slideshow covers the
//! whole window, but the world camera went on drawing the house behind it every frame:
//! its depth prepass, sun shadow cascades, post-processing and the rest [observed: player
//! report R-5NDGPRFB, an RX 6700 XT at 73 % on the menu alone; changing Quality there
//! allocated the directional shadow maps, about 470 MiB in one second].
//!
//! While the menu is open, every active 3D camera is switched off and a 2D camera of the
//! window's draws the interface alone ([`MenuUiCamera`], the default UI camera then, since
//! the interface is otherwise drawn through the player's camera). From the frame after the menu closes the
//! 2D camera goes and the cameras switched off here are switched on again; a camera another
//! system switched on meanwhile (the water's reflection) is switched off again each frame,
//! so the menu holds them all [design]. Cameras that were already off (the Pip-Boy's, the
//! terminal's) are left as they are.
//!
//! Log: `main menu: 3D view off (<n> cameras)` and `main menu: 3D view on again (<n> cameras)`.

use bevy::prelude::*;
use bevy::ui::IsDefaultUiCamera;

/// The 2D camera that draws the interface while the main menu is open.
#[derive(Component)]
pub struct MenuUiCamera;

/// A 3D camera switched off by the main menu, to be switched on when it closes.
#[derive(Component)]
pub struct MenuHeld;

/// Whether the main menu holds the 3D view now: while it is open.
pub fn holds_view(menu: Option<&nvf_play::main_menu::MainMenu>) -> bool {
    menu.is_some_and(|m| m.open)
}

/// The 3D cameras of the window (not the menu's interface camera).
type WorldCameras<'w, 's> = Query<'w, 's, (Entity, &'static mut Camera, Has<MenuHeld>), (With<Camera3d>, Without<MenuUiCamera>)>;
/// The 3D cameras the menu switched off.
type HeldCameras<'w, 's> = Query<'w, 's, (Entity, &'static mut Camera), (With<MenuHeld>, Without<MenuUiCamera>)>;

/// While the menu is open: the interface camera, and every active 3D camera off.
fn hold(
    mut commands: Commands,
    menu: Option<Res<nvf_play::main_menu::MainMenu>>,
    mut cameras: WorldCameras,
    ui: Query<(), With<MenuUiCamera>>,
) {
    if !holds_view(menu.as_deref()) {
        return;
    }
    if ui.is_empty() {
        // Above every other camera of the window, clearing to black under the menu.
        commands.spawn((Camera2d, Camera { order: 1000, clear_color: ClearColorConfig::Custom(Color::BLACK), ..default() }, IsDefaultUiCamera, MenuUiCamera, Name::new("Main menu interface camera")));
    }
    let mut n = 0;
    for (e, mut cam, held) in &mut cameras {
        if cam.is_active {
            cam.is_active = false;
            if !held {
                commands.entity(e).insert(MenuHeld);
                n += 1;
            }
        }
    }
    if n > 0 {
        info!("main menu: 3D view off ({n} cameras)");
    }
}

/// Once the menu has closed: the held cameras on again and the interface camera gone. In
/// `First`, so the frame's PostUpdate works out the cameras' views (the sun's shadow
/// cascades among them) before they are drawn: switched on after it, bevy_pbr found no
/// cascades for the view and stopped [observed: smoke script-runner-menu, 2026-11-28].
fn release(
    mut commands: Commands,
    menu: Option<Res<nvf_play::main_menu::MainMenu>>,
    mut cameras: HeldCameras,
    ui: Query<Entity, With<MenuUiCamera>>,
) {
    if holds_view(menu.as_deref()) || ui.is_empty() {
        return;
    }
    for e in &ui {
        commands.entity(e).despawn();
    }
    let mut n = 0;
    for (e, mut cam) in &mut cameras {
        cam.is_active = true;
        commands.entity(e).remove::<MenuHeld>();
        n += 1;
    }
    info!("main menu: 3D view on again ({n} cameras)");
}

pub struct MenuViewPlugin;

impl Plugin for MenuViewPlugin {
    fn build(&self, app: &mut App) {
        // Held in Last: after every system of the frame that switches a camera on (the
        // reflection's in PostUpdate), before the frame is drawn.
        app.add_systems(First, release).add_systems(Last, hold);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(open: bool) -> App {
        let mut app = App::new();
        let mut menu = nvf_play::main_menu::MainMenu::default();
        menu.open = open;
        app.insert_resource(menu).add_plugins(MenuViewPlugin);
        app
    }

    #[test]
    fn the_open_menu_switches_the_3d_view_off_and_back_on() {
        let mut app = app(true);
        let world_cam = app.world_mut().spawn((Camera3d::default(), Camera::default())).id();
        let off_cam = app.world_mut().spawn((Camera3d::default(), Camera { is_active: false, ..default() })).id();
        app.update();
        app.update();
        assert!(!app.world().get::<Camera>(world_cam).unwrap().is_active);
        let mut ui = app.world_mut().query_filtered::<Entity, With<MenuUiCamera>>();
        assert_eq!(ui.iter(app.world()).count(), 1, "one interface camera");
        // Something switches the world camera on while the menu is open: off again.
        app.world_mut().get_mut::<Camera>(world_cam).unwrap().is_active = true;
        app.update();
        assert!(!app.world().get::<Camera>(world_cam).unwrap().is_active);
        // New Game: the menu closes.
        app.world_mut().resource_mut::<nvf_play::main_menu::MainMenu>().open = false;
        app.update();
        assert!(app.world().get::<Camera>(world_cam).unwrap().is_active, "on again at the start of the next frame");
        assert!(!app.world().get::<Camera>(off_cam).unwrap().is_active, "a camera that was off stays off");
        assert_eq!(ui.iter(app.world()).count(), 0, "the interface camera is gone");
    }

    #[test]
    fn no_menu_leaves_the_view_alone() {
        let mut app = App::new();
        app.add_plugins(MenuViewPlugin);
        let cam = app.world_mut().spawn((Camera3d::default(), Camera::default())).id();
        app.update();
        assert!(app.world().get::<Camera>(cam).unwrap().is_active);
    }
}
