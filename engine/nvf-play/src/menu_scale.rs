//! Independent menu scale, retaining the effective legacy UI size on migration.
//! Policy and limits: docs/spec-notes/hud-menu-scale.md (#2519).
use crate::settings::{self, Setting};
use bevy::prelude::*;
use std::sync::atomic::{AtomicU32, Ordering};

pub const KEY: &str = "menu-scale";
/// The older slider's choices (50% to 100% in 5% steps), as an INI may still keep its part.
#[cfg(test)]
const CHOICES: usize = 11;
static DRAW_SCALE: AtomicU32 = AtomicU32::new(0x3f80_0000);

/// Pending next-start menu scale. It never follows later HUD setting edits.
#[derive(Resource, Clone, Copy)]
pub struct MenuScale(f32);

impl Default for MenuScale {
    fn default() -> Self { Self(1.0) }
}

impl MenuScale {
    pub fn scale(self) -> f32 { self.0 }
}

fn fit_for(scale: f32, android: bool) -> f32 {
    if !scale.is_finite() { return 1.0; }
    let clamped = if android { scale.clamp(0.5, 2.0) } else { scale.clamp(0.5, 1.0) };
    (clamped * 100.0).round() / 100.0
}

fn fit(scale: f32) -> f32 {
    fit_for(scale, cfg!(target_os = "android"))
}

fn part(scale: f32) -> f32 { ((fit_for(scale, false) - 0.5) * 2.0 * 1000.0).round() / 1000.0 }
#[cfg(any(not(target_os = "android"), test))]
fn of_part(value: f32) -> f32 {
    let value = if value.is_finite() { value.clamp(0.0, 1.0) } else { 1.0 };
    fit_for(0.5 + value * 0.5, false)
}

#[cfg(any(target_os = "android", test))]
const ANDROID_CHOICES: usize = 16; // 50%..=200% in 10% steps (>7 choices -> slider per §0.5)
#[cfg(any(target_os = "android", test))]
fn android_part(scale: f32) -> f32 { ((fit_for(scale, true) - 0.5) / 1.5).clamp(0.0, 1.0) }
#[cfg(any(target_os = "android", test))]
fn android_of_part(value: f32) -> f32 {
    let value = if value.is_finite() { value.clamp(0.0, 1.0) } else { 1.0 / 3.0 };
    (fit_for(0.5 + value * 1.5, true) * 10.0).round() / 10.0
}
#[cfg(target_os = "android")]
fn android_show(value: f32) -> String { format!("{:.0}%", android_of_part(value) * 100.0) }

/// Read the original 50..150% slider part or Main's named 50..100% list value.
/// Preserve the old HUD size before the independent menu list quantizes its own value.
pub(crate) fn legacy_menu(value: &str) -> Vec<(&'static str, String)> {
    let hud = if let Some(value) = value.parse::<f32>().ok().filter(|v| v.is_finite()) {
        let value = settings::choice_part(settings::choice_index(value, crate::interface::UI_SCALE_CHOICES), crate::interface::UI_SCALE_CHOICES);
        crate::interface::ui_scale_of_part(value)
    } else {
        let Some(scale) = crate::interface::UI_SCALE_LIST.iter().copied()
            .find(|scale| format!("{:.0}%", scale * 100.0) == value) else { return Vec::new() };
        scale
    };
    let scale = crate::interface::menu_scale_of(hud);
    vec![(KEY, format!("{:.3}", part(scale))),
        (crate::interface::HUD_SCALE_KEY, crate::interface::legacy_hud_scale(crate::interface::ui_scale_part(hud)))]
}

pub fn register(app: &mut App, group: &'static str) {
    app.init_resource::<MenuScale>();
    #[cfg(target_os = "android")]
    {
        settings::add_setting(
            app,
            Setting::slider(
                KEY,
                group,
                "Menu scale",
                "Size of the main, pause and other menus (50-200%, default 100%), independent of HUD scale. Takes effect at the next start.",
                settings::Slider::choice(
                    ANDROID_CHOICES,
                    |w| android_part(w.resource::<MenuScale>().0),
                    |w, v| w.resource_mut::<MenuScale>().0 = android_of_part(v),
                    android_show,
                ),
            ),
        );
    }
    #[cfg(not(target_os = "android"))]
    {
        // A list of 50% to 100% in steps of 10% [design: the menu overhaul, owner 2026-11-15];
        // an older slider's part (and the older ui-scale's split) loads as the choice nearest it.
        settings::add_choice_list(
            app,
            Setting { key: KEY, group, label: "Menu scale", note: "Size of the main, pause and other menus, independent of HUD scale. Takes effect at the next start.", switches: "", get: |_| false, set: |_, _| {}, slider: None },
            settings::ChoiceList {
                names: |_| list_names(),
                chosen: |w| list_names()[settings::nearest(&LIST, w.resource::<MenuScale>().0)].clone(),
                pick: take,
                save: |w| list_names()[settings::nearest(&LIST, w.resource::<MenuScale>().0)].clone(),
                load: take,
                ask_keep: false,
            },
        );
    }
    // Split missing independent menu/HUD values before ordinary settings loading.
    // store() writes both registered rows on any edit, freezing this initial value
    // even when the player changes only the HUD. An explicit menu key always wins.
    settings::set_older_key(app, KEY, "ui-scale");
    settings::set_older_split(app, "ui-scale", legacy_menu);
}

/// The Menu Scale row's choices [design: the menu overhaul].
#[cfg(not(target_os = "android"))]
const LIST: [f32; 6] = [0.5, 0.6, 0.7, 0.8, 0.9, 1.0];

#[cfg(not(target_os = "android"))]
fn list_names() -> Vec<String> {
    LIST.iter().map(|s| format!("{:.0}%", s * 100.0)).collect()
}

/// The menu scale `text` names (or an older slider's part stands for).
#[cfg(not(target_os = "android"))]
fn take(w: &mut World, text: &str) -> bool {
    match settings::choice_of(text, &list_names(), |v| settings::nearest(&LIST, of_part(v))) {
        Some(i) => {
            w.resource_mut::<MenuScale>().0 = LIST[i];
            true
        }
        None => false,
    }
}

pub(crate) fn publish(scale: f32) { DRAW_SCALE.store(fit(scale).to_bits(), Ordering::Relaxed); }
pub(crate) fn drawing() -> f32 { f32::from_bits(DRAW_SCALE.load(Ordering::Relaxed)) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_choices_retain_the_existing_effective_50_to_100_percent_range() {
        for i in 0..CHOICES {
            let value = settings::choice_part(i, CHOICES);
            assert!((of_part(value) - (0.5 + i as f32 * 0.05)).abs() < 0.00001);
            assert_eq!(part(of_part(value)), value);
        }
        assert_eq!((fit(0.1), fit(2.0), fit(f32::NAN)), (0.5, 1.0, 1.0));
        assert_eq!((of_part(0.0), of_part(0.5), of_part(1.0)), (0.5, 0.75, 1.0));
        assert_eq!((fit_for(0.1, true), fit_for(2.0, true), fit_for(3.0, true)), (0.5, 2.0, 2.0));
        for i in 0..ANDROID_CHOICES {
            let p = settings::choice_part(i, ANDROID_CHOICES);
            assert!((android_of_part(p) - (0.5 + i as f32 * 0.1)).abs() < 0.001);
            assert!((android_part(android_of_part(p)) - p).abs() < 0.001);
        }
    }

    #[test]
    fn legacy_ui_values_seed_only_a_missing_menu_value_and_keep_slider_units() {
        let mut app = App::new(); register(&mut app, "Display");
        for (old, expected) in [("0", "0.000"), ("0.25", "0.500"), ("0.5", "1.000"), ("0.75", "1.000"), ("7", "1.000"), ("-2", "0.000")] {
            let input = format!("[Settings]\nui-scale={old}\n[Other]\nkeep=yes\n");
            let text = app.world().resource::<settings::Settings>().split_older(&input);
            assert_eq!(fnv_install::ini_get(&text, "Settings", KEY).as_deref(), Some(expected));
            assert_eq!(fnv_install::ini_get(&text, "Settings", "ui-scale").as_deref(), Some(old));
            assert_eq!(fnv_install::ini_get(&text, "Other", "keep").as_deref(), Some("yes"));
        }
        for explicit in ["0.1", "0.5", "1.0", "bad"] {
            let input = format!("[Settings]\nui-scale=0.25\nmenu-scale={explicit}\n");
            assert_eq!(app.world().resource::<settings::Settings>().split_older(&input), input);
        }
        assert!(legacy_menu("NaN").is_empty());
        assert!(legacy_menu("big").is_empty());
    }

    #[test]
    fn independent_menu_units_keep_rendered_points_and_menu_hit_coordinates_together() {
        use crate::interface::{self, BOTTOM_MIDDLE, MIDDLE};
        let view = Vec2::new(1280.0, 720.0);
        let point = Vec2::new(823.0, 457.0);
        for (hud, menu) in [(0.75, 1.0), (1.5, 0.75), (1.0, 0.5)] {
            let _hud = interface::units_at(hud);
            assert_eq!(interface::u(96.0), Val::Vh(96.0 * hud / 9.6));
            {
                let _menu = interface::units_at(menu);
                assert_eq!(interface::u(96.0), Val::Vh(96.0 * menu / 9.6));
                for anchor in [MIDDLE, BOTTOM_MIDDLE] {
                    let drawn = interface::to_menu_frame(point, view, anchor);
                    assert!((drawn - (anchor * view * (1.0 - menu) + point * menu)).length() < 0.0001);
                    assert!((interface::from_menu_frame(drawn, view, anchor) - point).length() < 0.0001);
                }
                let _canvas = interface::original_units();
                assert_eq!(interface::u(96.0), Val::Vh(96.0 / 9.6));
            }
            assert_eq!(interface::unit_scale(), hud, "leaving a menu/canvas restores the HUD scope");
        }
    }
}
