//! Original control palettes and placement presets. No bindings or game state change.
#[derive(Clone, Debug, PartialEq)]
pub struct Appearance {
    pub theme: String,
    pub shape: String,
    /// None keeps the existing global SVG-family setting authoritative.
    pub family: Option<String>,
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: "glass".into(),
            shape: "circle".into(),
            family: None,
        }
    }
}
#[derive(Clone, Copy)]
pub struct Theme {
    pub id: &'static str,
    pub label: &'static str,
    pub background: [f32; 3],
    pub foreground: [f32; 3],
    pub border: [f32; 3],
    pub pressed: [f32; 3],
    pub fill: f32,
}
/// Shared outer button boundary, including the border. The joystick does not use this.
#[derive(Clone, Copy, Debug)]
pub struct ButtonGeometry {
    pub width: f32,
    pub height: f32,
    pub radius: f32,
}
impl ButtonGeometry {
    pub fn contains(self, offset: [f32; 2]) -> bool {
        if !offset.into_iter().all(f32::is_finite) || self.width <= 0.0 || self.height <= 0.0 {
            return false;
        }
        let [x, y] = offset.map(f32::abs);
        if x > self.width * 0.5 || y > self.height * 0.5 {
            return false;
        }
        let x = (x - (self.width * 0.5 - self.radius)).max(0.0);
        let y = (y - (self.height * 0.5 - self.radius)).max(0.0);
        // Inclusive analytic edge, allowing only a few floating-point ULPs.
        let squared = self.radius * self.radius;
        x * x + y * y <= squared + f32::EPSILON * squared.max(1.0) * 4.0
    }
}
pub const THEMES: &[Theme] = &[
    Theme {
        id: "glass",
        label: "Glass",
        background: [0.02, 0.06, 0.07],
        foreground: [1.0, 1.0, 1.0],
        border: [0.7, 0.9, 0.85],
        pressed: [0.18, 0.55, 0.43],
        fill: 1.0,
    },
    Theme {
        id: "outline",
        label: "Outline",
        background: [0.0, 0.0, 0.0],
        foreground: [1.0, 1.0, 1.0],
        border: [1.0, 1.0, 1.0],
        pressed: [0.3, 0.5, 0.6],
        fill: 0.0,
    },
    Theme {
        id: "slate",
        label: "Slate",
        background: [0.06, 0.08, 0.12],
        foreground: [0.9, 0.95, 1.0],
        border: [0.48, 0.7, 0.95],
        pressed: [0.16, 0.36, 0.65],
        fill: 1.0,
    },
    Theme {
        id: "amber",
        label: "Amber",
        background: [0.10, 0.06, 0.01],
        foreground: [1.0, 0.78, 0.3],
        border: [1.0, 0.65, 0.16],
        pressed: [0.46, 0.25, 0.02],
        fill: 0.85,
    },
    Theme {
        id: "mint",
        label: "Mint",
        background: [0.01, 0.09, 0.06],
        foreground: [0.72, 1.0, 0.85],
        border: [0.35, 0.95, 0.7],
        pressed: [0.05, 0.4, 0.26],
        fill: 0.8,
    },
    Theme {
        id: "frost",
        label: "Frost",
        background: [0.88, 0.94, 1.0],
        foreground: [0.02, 0.06, 0.12],
        border: [0.15, 0.35, 0.6],
        pressed: [0.55, 0.75, 0.96],
        fill: 1.0,
    },
    Theme {
        id: "ember",
        label: "Ember",
        background: [0.12, 0.025, 0.035],
        foreground: [1.0, 0.86, 0.83],
        border: [1.0, 0.4, 0.3],
        pressed: [0.5, 0.08, 0.05],
        fill: 0.9,
    },
    Theme {
        id: "contrast",
        label: "High contrast",
        background: [0.0, 0.0, 0.0],
        foreground: [1.0, 1.0, 1.0],
        border: [1.0, 1.0, 1.0],
        pressed: [0.24, 0.24, 0.24],
        fill: 1.0,
    },
];
pub const SHAPES: &[(&str, &str)] = &[
    ("circle", "Circle"),
    ("rounded", "Rounded"),
    ("square", "Square"),
    ("capsule", "Capsule"),
];
pub const PRESETS: &[(&str, &str)] = &[
    ("classic", "Classic"),
    ("explorer", "Two thumbs"),
    ("combat", "Combat"),
    ("left", "Left handed"),
    ("claw", "Four fingers"),
    ("compact", "Compact grid"),
    ("wide", "Wide reach"),
    ("controller", "Pad backup"),
];
pub const SLOT_NAMES: &[&str] = &[
    "My layout",
    "Relaxed",
    "Precision",
    "Travel",
    "Close reach",
    "Two thumbs",
    "Four fingers",
    "Custom",
];
pub const MAX_SLOTS: usize = 8;
pub const MAX_FILE_BYTES: u64 = 128 * 1024;
pub const ACTIONS: [&str; 12] = [
    "Theme",
    "Shape",
    "Icons",
    "Preset",
    "Slot",
    "Load slot",
    "Store slot",
    "Rename slot",
    "Reset layout",
    "Save",
    "Cancel",
    "Layout",
];

pub fn cycle<'a>(current: &str, ids: impl IntoIterator<Item = &'a str>) -> &'a str {
    let values: Vec<_> = ids.into_iter().collect();
    let next = values
        .iter()
        .position(|v| *v == current)
        .map_or(0, |i| (i + 1) % values.len());
    values[next]
}
impl Appearance {
    pub fn palette(&self) -> Theme {
        *THEMES
            .iter()
            .find(|t| t.id == self.theme)
            .unwrap_or(&THEMES[0])
    }
    pub fn next_theme(&mut self) {
        self.theme = cycle(&self.theme, THEMES.iter().map(|t| t.id)).into();
    }
    pub fn next_shape(&mut self) {
        self.shape = cycle(&self.shape, SHAPES.iter().map(|s| s.0)).into();
    }
    pub fn shape_label(&self) -> &str {
        SHAPES
            .iter()
            .find(|s| s.0 == self.shape)
            .map_or("Circle", |s| s.1)
    }
    pub fn geometry(&self, diameter: f32) -> ButtonGeometry {
        if !diameter.is_finite() || diameter <= 0.0 {
            return ButtonGeometry {
                width: 0.0,
                height: 0.0,
                radius: 0.0,
            };
        }
        let height = if self.shape == "capsule" {
            (diameter * 0.72).max(48.0).min(diameter)
        } else {
            diameter
        };
        let radius = match self.shape.as_str() {
            "square" => 0.0,
            "rounded" => diameter * 0.2,
            _ => diameter * 0.5,
        }
        .min(height * 0.5);
        ButtonGeometry {
            width: diameter,
            height,
            radius,
        }
    }
}
/// Only established gameplay IDs are repositioned; menu and future actions keep their positions.
pub fn position(preset: &str, id: usize, original: [f32; 2]) -> [f32; 2] {
    // Order is the existing Pause/Pip-Boy/VATS/Reload/Jump/Use/Aim/Fire/Sneak IDs.
    // Keep every action reachable. These are opt-in placement templates, not new bindings.
    let positions: Option<&[[f32; 2]; 9]> = match preset {
        "explorer" => Some(&[
            [0.95, 0.10],
            [0.82, 0.10],
            [0.69, 0.10],
            [0.94, 0.51],
            [0.80, 0.64],
            [0.94, 0.69],
            [0.80, 0.84],
            [0.94, 0.85],
            [0.66, 0.84],
        ]),
        "combat" => Some(&[
            [0.95, 0.10],
            [0.82, 0.10],
            [0.69, 0.10],
            [0.95, 0.48],
            [0.77, 0.65],
            [0.95, 0.86],
            [0.77, 0.85],
            [0.92, 0.69],
            [0.64, 0.85],
        ]),
        // Two index-finger targets, leaving the right thumb a broad empty look area.
        "claw" => Some(&[
            [0.95, 0.10],
            [0.82, 0.10],
            [0.69, 0.10],
            [0.94, 0.50],
            [0.80, 0.65],
            [0.94, 0.85],
            [0.91, 0.25],
            [0.09, 0.25],
            [0.67, 0.85],
        ]),
        "compact" => Some(&[
            [0.95, 0.10],
            [0.82, 0.10],
            [0.69, 0.10],
            [0.72, 0.66],
            [0.84, 0.66],
            [0.96, 0.66],
            [0.72, 0.84],
            [0.84, 0.84],
            [0.96, 0.84],
        ]),
        "wide" => Some(&[
            [0.95, 0.10],
            [0.82, 0.10],
            [0.69, 0.10],
            [0.94, 0.46],
            [0.74, 0.57],
            [0.94, 0.66],
            [0.74, 0.79],
            [0.94, 0.85],
            [0.60, 0.85],
        ]),
        // Larger fallback touch targets; physical-controller detection stays authoritative.
        "controller" => Some(&[
            [0.95, 0.10],
            [0.82, 0.10],
            [0.69, 0.10],
            [0.94, 0.43],
            [0.76, 0.43],
            [0.94, 0.64],
            [0.76, 0.64],
            [0.94, 0.85],
            [0.76, 0.85],
        ]),
        _ => None,
    };
    let mut p = positions
        .and_then(|p| p.get(id))
        .copied()
        .unwrap_or(original);

    if preset == "left" && id < 9 {
        p[0] = 1.0 - p[0];
    }
    p
}
pub fn scale(preset: &str, id: usize) -> f32 {
    match (preset, id) {
        ("combat", 7) => 1.25,
        ("combat", 6) | ("claw", 6) | ("claw", 7) => 1.1,
        ("compact", 7) => 1.1,
        ("controller", 3..=8) => 1.15,
        _ => 1.0,
    }
}
pub fn stick_position(preset: &str, original: [f32; 2]) -> [f32; 2] {
    match preset {
        "left" => [0.83, 0.74],
        "compact" => [0.14, 0.74],
        "claw" => [0.115, 0.76],
        "explorer" | "combat" | "wide" | "controller" => [0.17, 0.74],
        _ => original,
    }
}
pub fn stick_scale(preset: &str) -> f32 {
    // Keep a useful thumb target while reserving room for an eight-slot tray on short screens.
    if preset == "claw" { 0.85 } else { 1.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_shape_uses_its_analytic_visible_boundary() {
        let d = 120.0;
        for (shape, _) in SHAPES {
            let appearance = Appearance { shape: (*shape).into(), ..Appearance::default() };
            let g = appearance.geometry(d);
            assert!(g.contains([0.0, 0.0]), "{shape}");
            assert!(g.contains([g.width * 0.5, 0.0]), "{shape}");
            assert!(g.contains([0.0, g.height * 0.5]), "{shape}");
            assert!(!g.contains([g.width * 0.5 + 0.01, 0.0]), "{shape}");
            assert!(!g.contains([0.0, g.height * 0.5 + 0.01]), "{shape}");
            if g.radius == 0.0 {
                assert!(g.contains([g.width * 0.5, g.height * 0.5]));
            } else {
                let x = g.width * 0.5 - g.radius + g.radius * std::f32::consts::FRAC_1_SQRT_2;
                let y = g.height * 0.5 - g.radius + g.radius * std::f32::consts::FRAC_1_SQRT_2;
                for sx in [-1.0, 1.0] {
                    for sy in [-1.0, 1.0] {
                        assert!(
                            g.contains([sx * (x - 0.01), sy * (y - 0.01)]),
                            "inside {shape}"
                        );
                        assert!(g.contains([sx * x, sy * y]), "edge {shape}");
                        assert!(
                            !g.contains([sx * (x + 0.01), sy * (y + 0.01)]),
                            "outside {shape}"
                        );
                    }
                }
                assert!(
                    !g.contains([g.width * 0.5, g.height * 0.5]),
                    "cut corner {shape}"
                );
            }
        }
    }
    #[test]
    fn capsule_does_not_accept_the_invisible_circle_above_its_top() {
        let a = Appearance { shape: "capsule".into(), ..Appearance::default() };
        assert!(!a.geometry(120.0).contains([0.0, 55.0]));
        assert_eq!(a.geometry(120.0).height, 86.4);
        for d in [24.0, 48.0, 60.0] {
            let g = a.geometry(d);
            assert!(g.height <= g.width && g.radius <= g.height * 0.5);
        }
        assert!(!a.geometry(f32::NAN).contains([0.0, 0.0]));
        assert!(!a.geometry(120.0).contains([f32::NAN, 0.0]));
    }
    #[test]
    fn every_choice_cycles_without_losing_the_first_choice() {
        let mut a = Appearance::default();
        for _ in THEMES {
            a.next_theme();
        }
        assert_eq!(a, Appearance::default());
        for _ in SHAPES {
            a.next_shape();
        }
        assert_eq!(a, Appearance::default());
        a.theme = "future".into();
        assert_eq!(a.palette().id, "glass");
        a.next_theme();
        assert_eq!(a.theme, "glass");
    }
    #[test]
    fn palettes_are_finite_bounded_and_have_readable_foreground_contrast() {
        for t in THEMES {
            for c in [t.background, t.foreground, t.border, t.pressed] {
                assert!(
                    c.into_iter()
                        .all(|n| n.is_finite() && (0.0..=1.0).contains(&n))
                );
            }
            let luminance = |c: [f32; 3]| {
                let c = c.map(|v| {
                    if v <= 0.04045 {
                        v / 12.92
                    } else {
                        ((v + 0.055) / 1.055).powf(2.4)
                    }
                });
                c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722
            };
            let a = luminance(t.background);
            let b = luminance(t.foreground);
            assert!((a.max(b) + 0.05) / (a.min(b) + 0.05) >= 4.5, "{}", t.id);
        }
    }
    #[test]
    fn handedness_mirrors_without_rebinding_or_moving_future_actions() {
        assert_eq!(position("left", 7, [0.9, 0.8]), [1.0 - 0.9, 0.8]);
        for (id, _) in PRESETS {
            assert_eq!(position(id, 18, [0.41, 0.32]), [0.41, 0.32]);
        }
    }
    #[test]
    fn gameplay_presets_remain_in_normalized_space() {
        let base = [
            [0.935, 0.09],
            [0.815, 0.09],
            [0.695, 0.09],
            [0.935, 0.62],
            [0.815, 0.62],
            [0.935, 0.76],
            [0.815, 0.76],
            [0.935, 0.90],
            [0.815, 0.90],
        ];
        for (id, _) in PRESETS {
            for (i, p) in base.iter().enumerate() {
                let p = position(id, i, *p);
                assert!(p.into_iter().all(|n| (0.0..=1.0).contains(&n)));
            }
        }
    }
    #[test]
    fn unknown_shapes_fall_back_without_destroying_the_stored_identity() {
        let a = Appearance { shape: "future-shape".into(), ..Appearance::default() };
        assert_eq!(a.geometry(80.0).radius, 40.0);
        assert_eq!(a.shape, "future-shape");
    }
    #[test]
    fn pad_backup_label_keeps_the_existing_controller_preset_identity_and_geometry() {
        assert_eq!(PRESETS.iter().find(|p| p.0 == "controller").unwrap().1, "Pad backup");
        let mut config = super::super::layout::Config::default();
        config.apply_preset("controller");
        let encoded = config.encode();
        assert_eq!(encoded["preset"], "controller");
        let decoded = super::super::layout::Config::decode(&encoded).unwrap();
        assert_eq!(decoded.preset_name, "controller");
        assert_eq!(decoded.encode(), encoded);
        assert_eq!(decoded.stick.center.to_array(), [0.17, 0.74]);
        assert_eq!(decoded.buttons[3].center.to_array(), [0.94, 0.43]);
        assert_eq!(decoded.buttons[3].scale, 1.15);
    }
}
