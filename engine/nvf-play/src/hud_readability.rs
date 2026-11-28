//! Independent HUD sizing [design: Android accessibility follow-up to #2436].
//! Settings use the existing INI writer. V1 keys default to the unchanged drawing;
//! sizes change the real rendered subtree, including text and clipped compass marks.
use crate::settings::{self, Setting, Slider};
use bevy::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    Health,
    ActionPoints,
    Compass,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(UiTransform)]
pub(crate) struct HudSize(pub Part);

#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct HudReadability {
    pub health: f32,
    pub action_points: f32,
    pub compass: f32,
}

impl Default for HudReadability {
    fn default() -> Self {
        Self {
            health: 1.0,
            action_points: 1.0,
            compass: 1.0,
        }
    }
}
fn fit(value: f32) -> f32 {
    if !value.is_finite() {
        return 1.0;
    }
    (value.clamp(0.75, 2.0) * 20.0).round() / 20.0
}
fn of_part(value: f32) -> f32 {
    fit(0.75 + value.clamp(0.0, 1.0) * 1.25)
}
fn as_part(value: f32) -> f32 {
    (fit(value) - 0.75) / 1.25
}
fn show(value: f32) -> String {
    format!("{:.0}%", of_part(value) * 100.0)
}

impl HudReadability {
    fn scale(self, part: Part) -> f32 {
        fit(match part {
            Part::Health => self.health,
            Part::ActionPoints => self.action_points,
            Part::Compass => self.compass,
        })
    }
}

/// Each marked node is a separate subtree. Preserve its bottom corner while growing.
/// The full-window tile slots use the same transform as the hand-built panel nodes.
fn transform(part: Part, scale: f32) -> UiTransform {
    let anchor_x = if part == Part::ActionPoints { 1.0 } else { 0.0 };
    UiTransform {
        scale: Vec2::splat(scale),
        translation: Val2::percent(
            (anchor_x - 0.5) * (1.0 - scale) * 100.0,
            (1.0 - scale) * 50.0,
        ),
        ..UiTransform::IDENTITY
    }
}

fn apply(sizes: Res<HudReadability>, mut nodes: Query<(&HudSize, &mut UiTransform)>) {
    for (part, mut current) in &mut nodes {
        current.set_if_neq(transform(part.0, sizes.scale(part.0)));
    }
}

/// Only known built-in semantic groups are tagged. Unrelated and mod HUD groups keep
/// their own appearance. A compass is classified before its health-panel ancestor.
pub(crate) fn tile_part(path: &str) -> Option<Part> {
    let path = path.strip_prefix("HUDMainMenu/")?;
    if path == "HitPoints/compass_window"
        || path.starts_with("HitPoints/compass_window/")
        || path.starts_with("HitPoints/compass_icon")
    {
        Some(Part::Compass)
    } else if path == "HitPoints" || path.starts_with("HitPoints/") {
        Some(Part::Health)
    } else if path == "ActionPoints" || path.starts_with("ActionPoints/")
        || path == "AmmoTypeLabel" || path.starts_with("AmmoTypeLabel/")
        || path == "CNDArrows" || path.starts_with("CNDArrows/") {
        Some(Part::ActionPoints)
    } else {
        None
    }
}

/// Registered once with the engine settings so persistence, user ownership and reset
/// follow the same routes as the existing global UI scale.
pub fn register(app: &mut App, group: &'static str) {
    app.init_resource::<HudReadability>()
        .add_systems(PostUpdate, apply.before(bevy::ui::UiSystems::Layout));
    settings::add_setting(app, Setting::slider("hud-health-size-v1", group, "Health size",
        "Size of the health display. 100% restores its usual size; other HUD sizes stay unchanged.",
        Slider::choice(26, |w| as_part(w.resource::<HudReadability>().health), |w,v| w.resource_mut::<HudReadability>().health = of_part(v), show)));
    settings::add_setting(app, Setting::slider("hud-action-points-size-v1", group, "Action points size",
        "Size of the action points, ammo and weapon condition display. 100% restores its usual size.",
        Slider::choice(26, |w| as_part(w.resource::<HudReadability>().action_points), |w,v| w.resource_mut::<HudReadability>().action_points = of_part(v), show)));
    settings::add_setting(app, Setting::slider("hud-compass-size-v1", group, "Compass size",
        "Size of the compass, including its direction and location markers. 100% restores its usual size.",
        Slider::choice(26, |w| as_part(w.resource::<HudReadability>().compass), |w,v| w.resource_mut::<HudReadability>().compass = of_part(v), show)));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn individual_sizes_survive_the_real_ini_write_and_reload_route() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("nvf-hud-size-v1-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("NVForever.ini");
        let start = || {
            let mut app = App::new();
            app.insert_resource(settings::SettingsFile(Some(path.clone())));
            register(&mut app, "Display");
            app.add_systems(PostStartup, settings::apply_stored);
            app.update();
            app
        };
        let mut app = start();
        assert_eq!(
            *app.world().resource::<HudReadability>(),
            HudReadability::default()
        );
        let hp = app
            .world()
            .resource::<settings::Settings>()
            .index("hud-health-size-v1")
            .unwrap();
        let compass = app
            .world()
            .resource::<settings::Settings>()
            .index("hud-compass-size-v1")
            .unwrap();
        settings::set_slider(app.world_mut(), hp, as_part(1.5));
        settings::set_slider(app.world_mut(), compass, as_part(1.25));
        let restored = start();
        assert_eq!(
            *restored.world().resource::<HudReadability>(),
            HudReadability {
                health: 1.5,
                compass: 1.25,
                action_points: 1.0
            }
        );
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("hud-health-size-v1=") && saved.contains("hud-compass-size-v1="));
        drop((app, restored));
        std::fs::remove_file(&path).unwrap();
        // The settings writer may retain its own backup; remove only known owned files.
        for file in ["NVForever.ini.bak", "NVForever.ini.tmp"] {
            let p = dir.join(file);
            if p.is_file() {
                std::fs::remove_file(p).unwrap();
            }
        }
        std::fs::remove_dir(&dir).unwrap();
    }
    #[test]
    fn known_compass_groups_are_independent_of_health_and_unknown_hud() {
        assert_eq!(
            tile_part("HUDMainMenu/HitPoints/compass_window/strip"),
            Some(Part::Compass)
        );
        assert_eq!(tile_part("HUDMainMenu/HitPoints/meter"), Some(Part::Health));
        assert_eq!(
            tile_part("HUDMainMenu/ActionPoints/meter:1"),
            Some(Part::ActionPoints)
        );
        assert_eq!(tile_part("HUDMainMenu/AmmoTypeLabel"), Some(Part::ActionPoints));
        assert_eq!(tile_part("HUDMainMenu/CNDArrows"), Some(Part::ActionPoints));
        assert_eq!(tile_part("HUDMainMenu/SomeMod/HitPoints"), None);
        assert_eq!(tile_part("InventoryMenu/HitPoints"), None);
    }
    #[test]
    fn sizing_bounds_and_reset_keep_independent_values() {
        assert_eq!(fit(f32::NAN), 1.0);
        assert_eq!(fit(f32::INFINITY), 1.0);
        assert_eq!(fit(-1.0), 0.75);
        assert_eq!(fit(99.0), 2.0);
        for i in 0..26 {
            let value = 0.75 + i as f32 * 0.05;
            assert!((of_part(as_part(value)) - value).abs() < 0.001);
        }
        let mut sizes = HudReadability {
            health: 1.5,
            ..default()
        };
        assert_eq!(sizes.scale(Part::Compass), 1.0);
        assert_eq!(sizes.scale(Part::ActionPoints), 1.0);
        sizes.health = 1.0;
        assert_eq!(sizes, HudReadability::default());
    }
    #[test]
    fn live_subtree_scaling_keeps_bottom_anchors_and_default_geometry() {
        let mut app = App::new();
        app.init_resource::<HudReadability>()
            .add_systems(Update, apply);
        let hp = app
            .world_mut()
            .spawn((HudSize(Part::Health), UiTransform::IDENTITY))
            .id();
        let ap = app
            .world_mut()
            .spawn((HudSize(Part::ActionPoints), UiTransform::IDENTITY))
            .id();
        app.update();
        assert_eq!(
            *app.world().get::<UiTransform>(hp).unwrap(),
            UiTransform::IDENTITY
        );
        app.world_mut().resource_mut::<HudReadability>().health = 1.5;
        app.update();
        let actual = *app.world().get::<UiTransform>(hp).unwrap();
        assert_eq!(actual.scale, Vec2::splat(1.5));
        assert_eq!(actual.translation, Val2::percent(25.0, -25.0));
        assert_eq!(
            *app.world().get::<UiTransform>(ap).unwrap(),
            UiTransform::IDENTITY
        );
        assert_eq!(
            transform(Part::ActionPoints, 1.5).translation,
            Val2::percent(-25.0, -25.0)
        );
    }
}
