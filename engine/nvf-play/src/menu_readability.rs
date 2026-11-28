//! Independent sizes for existing trading, repair and crafting menus.
use crate::settings::{self, Setting, Slider};
use bevy::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKind { Barter, Repair, Workbench }

#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct MenuReadability {
    pub barter: f32,
    pub repair: f32,
    pub workbench: f32,
}
impl MenuReadability {
    pub fn for_platform(android: bool) -> Self {
        let size = if android { 1.50 } else { 1.0 };
        Self { barter: size, repair: size, workbench: size }
    }
}
impl Default for MenuReadability {
    fn default() -> Self { Self::for_platform(cfg!(target_os = "android")) }
}
fn fit(value: f32) -> f32 {
    if !value.is_finite() { return 1.0; }
    (value.clamp(0.5, 2.0) * 20.0).round() / 20.0
}
fn part(value: f32) -> f32 { (fit(value) - 0.5) / 1.5 }
fn of_part(value: f32) -> f32 {
    let v = if value.is_finite() { value.clamp(0.0, 1.0) } else { 1.0 / 3.0 };
    fit(0.5 + v * 1.5)
}
fn show(value: f32) -> String { format!("{:.0}%", of_part(value) * 100.0) }
impl MenuReadability {
    pub fn size(self, kind: MenuKind) -> f32 {
        fit(match kind { MenuKind::Barter => self.barter, MenuKind::Repair => self.repair,
            MenuKind::Workbench => self.workbench })
    }
}

// Retire held input at the setting consumer, including A -> B -> A in one frame.
// Equivalent normalized values preserve the existing contact.
fn set_size(world: &mut World, kind: MenuKind, value: f32) {
    let next = of_part(value);
    let previous = world.resource::<MenuReadability>().size(kind);
    {
        let mut sizes = world.resource_mut::<MenuReadability>();
        match kind {
            MenuKind::Barter => sizes.barter = next,
            MenuKind::Repair => sizes.repair = next,
            MenuKind::Workbench => sizes.workbench = next,
        }
    }
    if previous == next { return; }
    match kind {
        MenuKind::Barter => {
            crate::menu::retire_physical_quantity_paint(world);
            if let Some(mut touch) = world.get_resource_mut::<crate::menu::normal_touch::NormalBarter>() {
                touch.cancel();
            }
        }
        MenuKind::Repair | MenuKind::Workbench => {
            if let Some(mut touch) = world.get_resource_mut::<crate::menu::normal_services::NormalServices>() {
                touch.cancel_size(kind);
            }
        }
    }
}

/// Each consumer lays out its existing rows with this size, retaining viewport clipping.
pub fn register(app: &mut App, group: &'static str) {
    app.init_resource::<MenuReadability>();
    settings::add_setting(app, Setting::slider("barter-menu-size-v1", group, "Buying and selling size",
        "Text and row size in the normal buying and selling menus (50-200%; 150% default on Android). Other menus keep their own sizes.",
        Slider::choice(31, |w| part(w.resource::<MenuReadability>().barter),
            |w,v| set_size(w, MenuKind::Barter, v), show)));
    settings::add_setting(app, Setting::slider("repair-menu-size-v1", group, "Repair menu size",
        "Text and row size in repair menus (50-200%; 150% default on Android). Buying, selling and workbenches keep their own sizes.",
        Slider::choice(31, |w| part(w.resource::<MenuReadability>().repair),
            |w,v| set_size(w, MenuKind::Repair, v), show)));
    settings::add_setting(app, Setting::slider("workbench-menu-size-v1", group, "Workbench menu size",
        "Text and row size at workbenches, campfires and reloading benches (50-200%; 150% default on Android). Other menus keep their own sizes.",
        Slider::choice(31, |w| part(w.resource::<MenuReadability>().workbench),
            |w,v| set_size(w, MenuKind::Workbench, v), show)));
}

#[cfg(test)]
#[path = "menu_readability_tests.rs"]
mod tests;
