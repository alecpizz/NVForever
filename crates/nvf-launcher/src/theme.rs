//! The launcher's look: dark panels and the logo's gold.

#[cfg(feature = "desktop")]
use eframe::egui::{self, Color32, CornerRadius, FontId, Stroke, TextStyle};

// (Desktop colour, Android field, RGB) from the site's brand and dark-blue palette.
macro_rules! palette {
    ($(($name:ident, $field:literal, $r:literal, $g:literal, $b:literal)),+ $(,)?) => {
        $(
            #[cfg(feature = "desktop")]
            pub const $name: Color32 = Color32::from_rgb($r, $g, $b);
        )+
        #[cfg(any(feature = "mobile", test))]
        pub fn android() -> serde_json::Value {
            serde_json::json!({ $($field: 0xff00_0000u32 | ($r << 16) | ($g << 8) | $b),+ })
        }
    };
}

palette! {
    (GOLD, "gold", 0xF0, 0xA3, 0x2A),
    (ON_GOLD, "onGold", 0x15, 0x12, 0x0C),
    (BG, "bg", 0x0F, 0x11, 0x15),
    (PANEL, "panel", 0x15, 0x19, 0x22),
    (CARD, "card", 0x1B, 0x20, 0x2B),
    (BUTTON, "button", 0x21, 0x27, 0x34),
    (TEXT, "text", 0xE7, 0xE9, 0xEE),
    (WEAK, "weak", 0x9A, 0xA3, 0xB2),
}
/// The site's `--accent-hover`.
#[cfg(feature = "desktop")]
pub const GOLD_HOVER: Color32 = Color32::from_rgb(0xFF, 0xB8, 0x4D);
/// `GOLD` dimmed, for edges and borders only (3:1 or more on every background; too dark for
/// text, and white text on it is 3.4:1).
#[cfg(feature = "desktop")]
pub const GOLD_DIM: Color32 = Color32::from_rgb(0xA8, 0x72, 0x1D);
/// Small gold text (the dim headings): 5.1:1 on a card, 5.9:1 on the page (4.5:1 wanted).
#[cfg(feature = "desktop")]
pub const GOLD_TEXT: Color32 = Color32::from_rgb(0xC0, 0x84, 0x24);
/// Buttons under the pointer and pressed.
#[cfg(feature = "desktop")]
pub const BUTTON_HOVER: Color32 = Color32::from_rgb(0x2A, 0x31, 0x40);
#[cfg(feature = "desktop")]
pub const BUTTON_ACTIVE: Color32 = Color32::from_rgb(0x33, 0x3B, 0x4D);
/// The open page's button and the countdown: `--surface` with a touch of the accent.
#[cfg(feature = "desktop")]
pub const SELECTED: Color32 = Color32::from_rgb(0x26, 0x25, 0x24);
/// Selected text in a text box and a list's chosen item: the gold darkened until light text
/// reads on it (`TEXT` 7.7:1; on `GOLD_DIM` it was 3.4:1).
#[cfg(feature = "desktop")]
pub const SELECTION: Color32 = Color32::from_rgb(0x5A, 0x43, 0x18);
/// `--border`.
#[cfg(feature = "desktop")]
pub const LINE: Color32 = Color32::from_rgb(0x2A, 0x31, 0x40);
#[cfg(feature = "desktop")]
pub const BAD: Color32 = Color32::from_rgb(0xE0, 0x6A, 0x50);
#[cfg(feature = "desktop")]
pub const GOOD: Color32 = Color32::from_rgb(0x8C, 0xC0, 0x6A);

#[cfg(feature = "desktop")]
pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut style = (*ctx.global_style()).clone();
    style.text_styles = [
        (TextStyle::Heading, crate::fonts::title(26.0)),
        (TextStyle::Body, FontId::proportional(15.0)),
        (TextStyle::Button, FontId::proportional(15.0)),
        (TextStyle::Small, FontId::proportional(13.0)),
        (TextStyle::Monospace, FontId::monospace(13.0)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    // Text is not selectable: a drag over a list (the Mods page) painted rows of gold
    // selection. Text boxes keep their own selection.
    style.interaction.selectable_labels = false;
    let v = &mut style.visuals;
    *v = egui::Visuals::dark();
    // egui's dark theme thickens every glyph's edges (2c - c^2, about gamma 0.5), which suits
    // light text on dark but leaves the dark text on the gold buttons jagged and heavy. One
    // glyph atlas serves both, so a middle gamma: dark-on-gold edges smooth, light text still
    // full (compared side by side at 4x).
    v.text_options.color_transfer_function = egui::epaint::FontColorTransferFunction::Gamma(0.75);
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = BG;
    v.faint_bg_color = CARD;
    v.code_bg_color = BUTTON;
    v.override_text_color = Some(TEXT);
    v.hyperlink_color = GOLD;
    v.selection.bg_fill = SELECTION;
    v.selection.stroke = Stroke::new(1.0, TEXT);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CornerRadius::same(4);
    }
    v.widgets.inactive.weak_bg_fill = BUTTON;
    v.widgets.inactive.bg_fill = BUTTON;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, LINE);
    v.widgets.hovered.weak_bg_fill = BUTTON_HOVER;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, GOLD_DIM);
    v.widgets.active.weak_bg_fill = BUTTON_ACTIVE;
    v.widgets.active.bg_stroke = Stroke::new(1.0, GOLD);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    ctx.set_global_style(style);
}
