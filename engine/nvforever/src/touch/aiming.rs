//! Persisted touch-only aiming choices; ordinary mouse/controller sensitivity stays separate.
use bevy::prelude::{Rect, Vec2};
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aiming {
    pub look: f32,
    pub ads: f32,
    /// Fraction of joystick radius on each axis; 0.18 preserves the original threshold.
    pub dead_zone: f32,
    pub toggle_aim: bool,
    pub hold_sneak: bool,
}
impl Default for Aiming {
    fn default() -> Self {
        Self {
            look: 1.0,
            ads: 1.0,
            dead_zone: 0.18,
            toggle_aim: false,
            hold_sneak: false,
        }
    }
}
impl Aiming {
    pub fn look_scale(&self, ads: bool) -> f32 {
        if ads {
            self.ads
        } else {
            self.look
        }
    }
    pub fn encode(self) -> Value {
        json!({"look":self.look,"ads":self.ads,"dead_zone":self.dead_zone,"toggle_aim":self.toggle_aim,"hold_sneak":self.hold_sneak})
    }
    pub fn decode(v: &Value) -> Self {
        let mut a = Self::default();
        let number = |key: &str, lo: f32, hi: f32| {
            v[key]
                .as_f64()
                .filter(|n| n.is_finite())
                .map(|n| (n as f32).clamp(lo, hi))
        };
        if let Some(n) = number("look", 0.25, 3.0) {
            a.look = n;
        }
        if let Some(n) = number("ads", 0.1, 2.0) {
            a.ads = n;
        }
        if let Some(n) = number("dead_zone", 0.05, 0.5) {
            a.dead_zone = n;
        }
        if let Some(b) = v["toggle_aim"].as_bool() {
            a.toggle_aim = b;
        }
        if let Some(b) = v["hold_sneak"].as_bool() {
            a.hold_sneak = b;
        }
        a
    }
    pub fn adjust(&mut self, action: usize) {
        match action {
            0 => self.look = (self.look - 0.1).max(0.25),
            1 => self.look = (self.look + 0.1).min(3.0),
            2 => *self = Self::default(),
            3 => self.ads = (self.ads - 0.1).max(0.1),
            4 => self.ads = (self.ads + 0.1).min(2.0),
            6 => {
                const ZONES: [f32; 4] = [0.1, 0.18, 0.25, 0.35];
                let current = ZONES
                    .iter()
                    .position(|n| (*n - self.dead_zone).abs() < 0.001);
                self.dead_zone = ZONES[current.map_or(0, |i| (i + 1) % ZONES.len())];
            }
            7 => self.toggle_aim = !self.toggle_aim,
            8 => self.hold_sneak = !self.hold_sneak,
            _ => {}
        }
    }
}
pub const EDIT_ACTIONS: [&str; 12] = [
    "Look -",
    "Look +",
    "Reset aiming",
    "ADS -",
    "ADS +",
    "Aiming help",
    "Dead zone",
    "Aim mode",
    "Sneak mode",
    "Save",
    "Cancel",
    "Layout",
];

/// Independent settings sheet, so the HUD/layout canvas can evolve without a grid contract.
pub fn editor_panel(size: Vec2) -> Rect {
    let width = (size.x - 24.0).clamp(48.0, 600.0);
    let row = (size.y * 0.15).clamp(40.0, 54.0);
    Rect::from_corners(
        Vec2::new((size.x - width) * 0.5, size.y - 8.0 - row * 4.0 - 8.0),
        Vec2::new((size.x + width) * 0.5, size.y - 8.0),
    )
}
pub fn editor_rect(index: usize, size: Vec2) -> Rect {
    let panel = editor_panel(size);
    let origin = panel.min + Vec2::splat(4.0);
    let width = panel.width() - 8.0;
    let row = (panel.height() - 8.0) / 4.0;
    let (x, y, w) = match index {
        0 => (width * 0.5 - 100.0, 0.0, 48.0),
        1 => (width * 0.5 - 48.0, 0.0, 48.0),
        3 => (width - 100.0, 0.0, 48.0),
        4 => (width - 48.0, 0.0, 48.0),
        7 => (0.0, row, width * 0.5 - 4.0),
        8 => (width * 0.5, row, width * 0.5 - 4.0),
        6 => (0.0, row * 2.0, width * 0.5 - 4.0),
        2 => (width * 0.5, row * 2.0, width * 0.5 - 4.0),
        9 => (0.0, row * 3.0, width * 0.25 - 4.0),
        10 => (width * 0.25, row * 3.0, width * 0.25 - 4.0),
        11 => (width * 0.5, row * 3.0, width * 0.25 - 4.0),
        _ => (width * 0.75, row * 3.0, width * 0.25 - 4.0),
    };
    Rect::from_corners(
        origin + Vec2::new(x, y),
        origin + Vec2::new(x + w, y + row - 4.0),
    )
}
pub fn value_rect(ads: bool, size: Vec2) -> Rect {
    let panel = editor_panel(size);
    let width = panel.width() - 8.0;
    let origin = panel.min + Vec2::new(8.0 + if ads { width * 0.5 } else { 0.0 }, 10.0);
    Rect::from_corners(
        origin,
        origin + Vec2::new((width * 0.5 - 112.0).max(24.0), 32.0),
    )
}

#[cfg(test)]
mod tests {
    use super::super::layout::Config;
    use super::*;
    #[test]
    fn the_bottom_sheet_has_disjoint_phone_sized_action_targets() {
        let size = Vec2::new(792.0, 363.0);
        let panel = editor_panel(size);
        for i in 0..12 {
            let a = editor_rect(i, size);
            assert!(a.width() >= 44.0 && a.height() >= 44.0);
            assert!(panel.contains(a.min) && panel.contains(a.max));
            for j in 0..i {
                let b = editor_rect(j, size);
                let overlaps = a.min.x < b.max.x
                    && b.min.x < a.max.x
                    && a.min.y < b.max.y
                    && b.min.y < a.max.y;
                assert!(!overlaps, "action {i} overlaps action {j}");
            }
        }
    }
    #[test]
    fn old_layouts_keep_the_original_aiming_and_sneak_defaults() {
        assert_eq!(
            Config::decode(&json!({"version":1})).unwrap().aiming,
            Aiming::default()
        );
    }
    #[test]
    fn persisted_aiming_values_roundtrip_and_stay_finite_and_bounded() {
        let c = Config {
            aiming: Aiming {
                look: 1.5,
                ads: 0.5,
                dead_zone: 0.25,
                toggle_aim: true,
                hold_sneak: true,
            },
            ..Config::default()
        };
        assert_eq!(Config::decode(&c.encode()).unwrap(), c);
        let mut value = c.encode();
        value["aiming"] = json!({"look":1e300,"ads":-8,"dead_zone":999,"toggle_aim":"invalid"});
        let read = Config::decode(&value).unwrap().aiming;
        assert_eq!((read.look, read.ads, read.dead_zone), (3.0, 0.1, 0.5));
        assert!(!read.toggle_aim);
        assert_eq!(
            Aiming::decode(&json!({"look":"invalid","ads":null})),
            Aiming::default()
        );
    }
    #[test]
    fn resetting_aiming_does_not_replace_custom_button_positions() {
        let mut c = Config::default();
        c.buttons[7].center = bevy::prelude::Vec2::new(0.6, 0.7);
        let original = c.buttons.clone();
        c.aiming.look = 2.0;
        c.aiming.adjust(2);
        assert_eq!(c.aiming, Aiming::default());
        assert_eq!(c.buttons, original);
    }
}
