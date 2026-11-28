//! The name menu (`menus\dialog\texteditmenu.xml`, menu 1051, opened by `GetPlayerName`
//! in the opening): a box in the middle of the screen asking for the character's name,
//! the name as typed, and Ok (docs/spec-notes/chargen.md §2).

use super::draw::{Align, Ctx, Which, BODY_BASE, BODY_H, LINE_PITCH, THICK};
use crate::interface::{amber, at, set_line, u};
use bevy::prelude::*;
use crate::ui_pointer::Button;
use fnv_game::chargen;

// ---- the menu file's numbers [documented: texteditmenu.xml, §2] ---------------------------

/// The box: 720 × 180, centred on the screen.
pub const BOX: Vec2 = Vec2::new(720.0, 180.0);
/// The dark picture behind: 25 out on every side.
const BEHIND: f32 = 25.0;
/// The question, centred, 32 down.
const PROMPT_Y: f32 = 32.0;
/// The name wraps at 250, centred, in the middle of the box's height.
const TEXT_WRAP: f32 = 250.0;
/// Ok: right-aligned 16 in from the box's right side, its bottom 16 above the box's;
/// a button is its text and 20 (10 each side).
const OK_IN: f32 = 16.0;
const BUTTON_PAD: f32 = 10.0;
/// The top bracket's corner lines (top_bracket.xml, 50 high).
const TOP_CORNER: f32 = 50.0;

// ---- measured on the owner's screenshot [observed, §5] ----------------------------------------

/// The settings that name the question and the Ok button's word: the load order's GMST
/// when a plugin has one, else NVForever's own label (`fnv_ui::strings::BUILT_IN`;
/// `fnv_game::ui_loading::game_string`; LEGAL.md) [design, #1028].
pub const PROMPT_SETTING: &str = "sEnterName";
pub const OK_SETTING: &str = "sOk";
/// The question and the name are in font 1, the glowing one [observed: their letters
/// have the glow on the screenshot; hypothesis: font 1 is the default of a text the file
/// gives no font]; Ok in font 2, a text box's default [documented: text_box.xml].
///
/// How dark the box makes what is behind it: by eye, about the message box's; darker
/// since the owner's playtest list 7 item 2 (`crate::interface::TOP_MENU_SHADE`).
const SHADE: f32 = crate::interface::TOP_MENU_SHADE;

/// The name menu's state.
#[derive(Debug, Clone, PartialEq)]
pub struct NameMenu {
    pub text: String,
    pub hover_ok: bool,
    /// The question over the name ([`PROMPT_SETTING`]).
    pub prompt: String,
    /// The Ok button's word ([`OK_SETTING`]).
    pub ok: String,
    /// A script's text input menu (ShowTextInputMenu, `fnv_game::nvse_jip12`) in place
    /// of the character's name: its own title, size and rules.
    pub script: Option<ScriptInput>,
    /// The longest name taken (`fnv_game::chargen::name_limit`).
    pub limit: usize,
}

/// The rules of a script's text input menu.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptInput {
    pub size: Vec2,
    pub min: usize,
    pub max: usize,
    pub digits_only: bool,
    pub enter_closes: bool,
}

impl ScriptInput {
    /// The rules of the script's menu; the box the script's size, the name box's when it
    /// gives none [hypothesis: the size is the box's].
    pub fn of(t: &fnv_game::nvse_jip12::TextInput) -> ScriptInput {
        let size = Vec2::new(if t.size.0 > 0.0 { t.size.0 } else { BOX.x }, if t.size.1 > 0.0 { t.size.1 } else { BOX.y });
        ScriptInput { size, min: t.min, max: t.max, digits_only: t.digits_only, enter_closes: t.enter_closes }
    }
}

/// The Ok button.
#[derive(Component, Clone, Copy, Debug)]
pub struct OkButton;

impl NameMenu {
    /// The menu offering `offered`, its words from the running game's strings (or
    /// NVForever's labels alone without one).
    pub fn open(offered: String, game: Option<&fnv_game::Game<'_>>) -> NameMenu {
        let word = |name: &str| match game {
            Some(g) => fnv_game::ui_loading::game_string(g, name),
            None => fnv_ui::strings::built_in(name).unwrap_or_default().to_owned(),
        };
        let limit = game.map_or(chargen::NAME_MAX_CHARS, chargen::name_limit);
        NameMenu { text: chargen::clean_name_to(&offered, limit), hover_ok: false, prompt: word(PROMPT_SETTING), ok: word(OK_SETTING), script: None, limit }
    }

    /// A script's text input menu: the title over an empty text; the box the script's
    /// size, the name box's when it gives none [hypothesis: the size is the box's].
    pub fn script(t: &fnv_game::nvse_jip12::TextInput, game: Option<&fnv_game::Game<'_>>) -> NameMenu {
        let ok = match game {
            Some(g) => fnv_game::ui_loading::game_string(g, OK_SETTING),
            None => fnv_ui::strings::built_in(OK_SETTING).unwrap_or_default().to_owned(),
        };
        NameMenu { text: String::new(), hover_ok: false, prompt: t.title.clone(), ok, script: Some(ScriptInput::of(t)), limit: t.max }
    }

    /// The script's rules as they stand now: SetTextInputExtendedProps may change them
    /// while the box is open (#2018). The text is cut to a shorter limit.
    pub fn update_rules(&mut self, t: &fnv_game::nvse_jip12::TextInput) -> bool {
        let now = ScriptInput::of(t);
        if self.script.as_ref().is_none_or(|r| *r == now) {
            return false;
        }
        self.text = self.text.chars().take(now.max).collect();
        self.limit = now.max;
        self.script = Some(now);
        true
    }

    /// The box's size.
    pub fn size(&self) -> Vec2 {
        self.script.as_ref().map_or(BOX, |s| s.size)
    }

    /// Typed text goes after the name, as far as the name may be long; in a script's
    /// menu, as far as its limit, only digits and the point when it says so [documented:
    /// SetTextInputExtendedProps].
    pub fn type_text(&mut self, s: &str) -> bool {
        let next = match &self.script {
            Some(r) => {
                let typed: String = s.chars().filter(|c| !c.is_control() && (!r.digits_only || c.is_ascii_digit() || *c == '.')).collect();
                format!("{}{typed}", self.text).chars().take(r.max).collect()
            }
            None => chargen::clean_name_to(&format!("{}{s}", self.text), self.limit),
        };
        let changed = next != self.text;
        self.text = next;
        changed
    }

    /// SetTextInputString's text in place of what is typed.
    pub fn set_text(&mut self, s: &str) {
        let max = self.script.as_ref().map_or(usize::MAX, |r| r.max);
        self.text = s.chars().take(max).collect();
    }

    pub fn backspace(&mut self) -> bool {
        self.text.pop().is_some()
    }

    pub fn can_finish(&self) -> bool {
        match &self.script {
            Some(r) => self.text.chars().count() >= r.min,
            None => chargen::name_acceptable(&self.text),
        }
    }
}

/// Root layout for the name/text-input menu: centered on desktop; anchored near the top
/// of the viewport when an on-screen soft keyboard is active (Android) so the IME does
/// not cover the input box or the Ok button.
pub(crate) fn root_node(soft_keyboard: bool) -> Node {
    Node {
        position_type: PositionType::Absolute,
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        justify_content: JustifyContent::Center,
        align_items: if soft_keyboard { AlignItems::FlexStart } else { AlignItems::Center },
        padding: if soft_keyboard { UiRect::top(Val::Percent(8.0)) } else { UiRect::ZERO },
        ..default()
    }
}

/// [design: docs/design/android.md] Clamp the name/text-input box scale and apply safe-area
/// padding when `menu_scale > 1.0` or safe insets are non-zero. With a soft keyboard active,
/// keeps the box and its Ok button in the upper half of the viewport above the IME.
/// Returns `None` at `menu_scale <= 1.0` with zero safe insets so desktop stays untouched.
pub(crate) fn fit_layout(
    menu_scale: f32,
    view_px: Vec2,
    safe: crate::touch_safe_area::SafeInsets,
    size: Vec2,
    soft_keyboard: bool,
) -> Option<(f32, UiRect)> {
    let content_w = size.x + 2.0 * BEHIND + 32.0;
    let content_h = size.y + 2.0 * BEHIND + 32.0;
    if !soft_keyboard {
        return crate::interface::fit_centered_box(menu_scale, view_px, safe, content_w, content_h);
    }
    let scale = if menu_scale.is_finite() && menu_scale > 0.0 { menu_scale } else { 1.0 };
    let view_w = if view_px.x.is_finite() && view_px.x > 1.0 { view_px.x } else { 1280.0 };
    let view_h = if view_px.y.is_finite() && view_px.y > 1.0 { view_px.y } else { 720.0 };
    let clean = |v: f32, cap: f32| if v.is_finite() { v.clamp(0.0, cap) } else { 0.0 };
    let (l, r) = (clean(safe.left, view_w * 0.45), clean(safe.right, view_w * 0.45));
    let (t, b) = (clean(safe.top, view_h * 0.45), clean(safe.bottom, view_h * 0.45));
    let has_safe = l > 0.5 || t > 0.5 || r > 0.5 || b > 0.5;
    if scale <= 1.0 && !has_safe {
        return None;
    }
    let px_per_u1 = view_h / 960.0;
    let safe_w_u1 = (view_w - l - r).max(1.0) / px_per_u1;
    let safe_h_px = (view_h - t - b).max(1.0);
    let top_px = t + 0.08 * safe_h_px;
    let ime_budget_px = (view_h * 0.50 - top_px).max(safe_h_px * 0.36);
    let ime_budget_u1 = ime_budget_px / px_per_u1;
    let max_scale = (safe_w_u1 / content_w.max(1.0)).min(ime_budget_u1 / (size.y + BEHIND + 16.0).max(1.0));
    let draw_scale = scale.min(max_scale).max(0.1);
    let pad = UiRect { left: Val::Px(l), right: Val::Px(r), top: Val::Px(top_px), bottom: Val::Px(b) };
    Some((draw_scale, pad))
}

/// Spawn the box under `root` (a full-screen node that places its child).
pub fn spawn(root: &mut ChildSpawnerCommands, m: &NameMenu, c: &mut Ctx) {
    let bright = amber(255.0);
    let size = m.size();
    root.spawn(Node { width: u(size.x), height: u(size.y), ..default() }).with_children(|p| {
        let behind = at(-BEHIND, -BEHIND, size.x + 2.0 * BEHIND, size.y + 2.0 * BEHIND);
        let shade = Color::srgba(0.0, 0.0, 0.0, SHADE);
        if c.image(p, "interface\\shared\\background\\solid_black.dds", behind.clone(), shade, ()).is_none() {
            p.spawn((behind, BackgroundColor(shade)));
        }
        c.top_bracket(p, 0.0, 0.0, size.x, TOP_CORNER, bright);
        c.bottom_bracket(p, 0.0, size.y - THICK, size.x, bright);
        c.text(p, Which::Glow, &m.prompt, size.x / 2.0, PROMPT_Y, Align::Centre, bright);
        // The name, each line centred, the whole in the middle of the box's height (a
        // script's text wraps at the box's width less the margins [hypothesis]).
        let wrap = if m.script.is_some() { (size.x - 2.0 * OK_IN).max(TEXT_WRAP) } else { TEXT_WRAP };
        let lines = c.kit.glow.font.lines(&m.text, Some(wrap));
        let h = BODY_H + LINE_PITCH * lines.len().saturating_sub(1) as f32;
        let top = (size.y - h) / 2.0;
        for (k, line) in lines.iter().enumerate() {
            set_line(p, &c.kit.glow, line, (size.x - line.width) / 2.0, top + BODY_BASE + LINE_PITCH * k as f32, bright);
        }
        let ok_w = c.width(Which::Body, &m.ok) + 2.0 * BUTTON_PAD;
        let ok_h = BODY_H + 2.0 * BUTTON_PAD;
        let (ok_x, ok_y) = (size.x - OK_IN - ok_w, size.y - ok_h - OK_IN);
        let color = bright.with_alpha(if m.can_finish() { 1.0 } else { 128.0 / 255.0 });
        if m.hover_ok && m.can_finish() {
            c.outline(p, ok_x, ok_y, ok_w, ok_h, amber(200.0));
        }
        c.text(p, Which::Body, &m.ok, ok_x + ok_w - BUTTON_PAD, ok_y + BUTTON_PAD, Align::Right, color);
        p.spawn((Button, OkButton, at(ok_x, ok_y, ok_w, ok_h)));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_and_rubbing_out() {
        let mut m = NameMenu::open("Courier".into(), None);
        // Without a game, NVForever's own labels (#1028).
        assert_eq!((m.prompt.as_str(), m.ok.as_str()), (fnv_ui::strings::built_in(PROMPT_SETTING).unwrap(), fnv_ui::strings::built_in(OK_SETTING).unwrap()));
        assert!(m.type_text(" Six"));
        assert_eq!(m.text, "Courier Six");
        for _ in 0..11 {
            assert!(m.backspace());
        }
        assert!(!m.backspace());
        assert!(!m.can_finish());
        assert!(m.type_text("\u{8}A\r"));
        assert_eq!(m.text, "A");
        m.type_text(&"b".repeat(60));
        assert_eq!(m.text.chars().count(), chargen::NAME_MAX_CHARS);
    }

    #[test]
    fn android_soft_keyboard_places_text_entry_box_in_upper_viewport() {
        let desktop = root_node(false);
        assert_eq!(desktop.align_items, AlignItems::Center);
        assert_eq!(desktop.padding.top, Val::Px(0.0));
        let android = root_node(true);
        assert_eq!(android.align_items, AlignItems::FlexStart);
        let Val::Percent(top_pct) = android.padding.top else { panic!("expected percent top padding") };
        assert!(top_pct <= 10.0, "box starts near top of viewport: {top_pct}%");
        let box_pct = BOX.y / 960.0 * 100.0;
        assert!(top_pct + box_pct < 40.0, "box and Ok button stay well above the bottom 50% soft keyboard area");
    }

    #[test]
    fn high_menu_scale_keeps_name_box_and_ok_button_inside_safe_area_and_above_ime() {
        use crate::touch_safe_area::SafeInsets;
        let safe = SafeInsets { left: 40.0, top: 12.0, right: 40.0, bottom: 24.0 };
        assert_eq!(fit_layout(1.0, Vec2::new(1600.0, 900.0), SafeInsets::default(), BOX, false), None);
        for view in [Vec2::new(1280.0, 720.0), Vec2::new(960.0, 540.0), Vec2::new(2400.0, 1080.0)] {
            for scale in [1.25f32, 1.5, 2.0] {
                let (s, pad) = fit_layout(scale, view, safe, BOX, true).expect("clamped");
                let px_per_u = (view.y / 960.0) * s;
                let Val::Px(top_px) = pad.top else { panic!("expected px top") };
                assert!(BOX.x * px_per_u <= view.x - safe.left - safe.right + 0.5, "width @ {view:?} {scale}");
                assert!(top_px + BOX.y * px_per_u <= view.y * 0.52, "Ok stays above soft keyboard @ {view:?} {scale}");
            }
        }
    }
}
