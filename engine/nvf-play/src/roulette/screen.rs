//! The roulette menu's text and buttons as `menus\roulette_menu.xml` sets them out
//! [documented], over the table's picture ([`super::RouletteView`], drawn by the engine).
//! Lengths are the interface's units (960 to the view's height); the main rectangle is
//! 30 in from the view's edges, as lockpicking's (`crate::lockpick::screen`).
//!
//! - The information box: 400 × 150, 10 in from the main rectangle's left and bottom, the
//!   bracket of lines, four rows in font 7 at 20 in, 35 apart from its top: the bet, the
//!   bets on the table, the chips and the casino's winnings, each value right-aligned 85
//!   short of the box's width [documented: `ROM_InfoRect`]. The titles, "Your bet:", "On
//!   the table:", "Your chips:" and "Won at <casino>:", are NVForever's own words
//!   [design, #4495: in place of the titles read from the original's screen].
//! - Six buttons right-aligned 50 from the main rectangle's right, under each other: Place
//!   Bet, Remove Bet, Finish Bet, Decrease Bet, Increase Bet, Exit, each with its key
//!   [documented: the order, `_PCButton_W/F/S/Q/E/R`; observed: the words]. The file puts
//!   them 144 above the box's top; on the screenshots the first stands about 606 down the
//!   screen and they are about 52 apart [observed], which NVForever follows. A button that
//!   cannot be used is dimmed: Remove and Finish Bet with nothing on the table, all of them
//!   while the wheel turns [observed: the screenshots].
//! - The status text at the top left, font 7: "Bet: …" and "Payout: … to 1" for the bet
//!   point under the marker [observed: "Bet:" and "Payout:" at the top left of the
//!   screenshot; hypothesis: what follows them], or a refusal, or the spin's result.
//! - The help box over all of it the first time (`crate::lockpick::help`).

use super::{Phase, RouletteInput, RouletteScreen, RouletteView, Session};
use crate::interface::{amber, at, u};
use crate::lockpick::screen::{Ctx, FontKind, Kit};
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use crate::ui_pointer::Button;
use bevy::window::PrimaryWindow;
use fnv_util::words::{tr, trf};
use nvf_assets::GameData;

const EDGE: f32 = 30.0;
const INFO: Vec2 = Vec2::new(400.0, 150.0);
const INFO_IN: f32 = 10.0;
const ROW_X: f32 = 20.0;
const ROW_STEP: f32 = 35.0;
const VALUE_X: f32 = INFO.x - 85.0 + ROW_X;
const THICK: f32 = 2.0;
const FADE: f32 = 30.0;
const BUTTONS_RIGHT: f32 = 50.0;
/// The first button's top, down the main rectangle, and the buttons' pitch [observed:
/// fitted to the screenshots, see above].
const BUTTONS_TOP: f32 = 549.0;
const BUTTON_H: f32 = 52.0;
const HINT_GAP: f32 = 16.0;
/// The status lines: from the view's top left, 40 apart [observed: the screenshot].
const STATUS: Vec2 = Vec2::new(120.0, 64.0);
const STATUS_STEP: f32 = 40.0;
const DIM: f32 = 110.0;
/// Under the help box the table is drawn darker [as lockpicking's].
const UNDER_HELP: f32 = 0.47;

/// The menu's root node.
#[derive(Component)]
pub struct RouletteRoot;

/// A button and what it does.
#[derive(Component, Clone)]
pub struct RouletteButton(pub RouletteInput);

fn view_key(s: &Session, has_view: bool, view_w: f32) -> String {
    format!(
        "{}:{}:{}:{}:{}:{:?}:{:?}:{}:{}:{has_view}:{view_w:.0}:{:?}",
        s.bet,
        s.table.total(),
        s.chips,
        s.earnings,
        s.cursor,
        s.status,
        std::mem::discriminant(&s.phase),
        s.help.is_some(),
        s.can_place(),
        crate::pad::labels(),
    )
}

fn view_width(window: Option<&Window>) -> f32 {
    window.map_or(960.0 * 16.0 / 9.0, |w| 960.0 * w.width() / w.height().max(1.0))
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut commands: Commands,
    screen: Res<RouletteScreen>,
    view: Option<Res<RouletteView>>,
    data: Option<Res<GameData>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    glyphs: Res<crate::pad::PadGlyphs>,
    roots: Query<Entity, With<RouletteRoot>>,
    (mut kit, mut shown): (Local<Option<Kit>>, Local<Option<String>>),
) {
    let view_w = view_width(window.single().ok());
    let key = screen.0.as_ref().map(|s| view_key(s, view.is_some(), view_w));
    if key == *shown {
        return;
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
    *shown = key;
    let Some(s) = screen.0.as_ref() else { return };
    let bc = crate::interface::bc_supported(formats.as_deref());
    let kit = kit.get_or_insert_with(|| Kit::load(data.as_deref(), &mut images, bc));
    let mut c = Ctx { kit, data: data.as_deref(), images: &mut images, bc, glyphs: &glyphs };
    spawn(&mut commands, &mut c, s, view.as_deref(), view_w);
}

fn spawn(commands: &mut Commands, c: &mut Ctx, s: &Session, view: Option<&RouletteView>, view_w: f32) {
    let bright = amber(255.0);
    let dim = amber(DIM);
    let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    // The table in the world shows through; the table drawn alone is a picture on black.
    let in_world = view.is_some_and(|v| v.image.is_none());
    let back = if !in_world {
        Color::BLACK
    } else if s.help.is_some() {
        Color::srgba(0.0, 0.0, 0.0, 1.0 - UNDER_HELP)
    } else {
        Color::NONE
    };
    let mut root = commands.spawn((RouletteRoot, full.clone(), GlobalZIndex(106), BackgroundColor(back)));
    if let Some(image) = view.and_then(|v| v.image.clone()) {
        let light = if s.help.is_some() { UNDER_HELP } else { 1.0 };
        root.with_children(|r| {
            r.spawn((full.clone(), ImageNode { color: Color::srgb(light, light, light), ..ImageNode::new(image) }));
        });
    }
    // The menu at the UI scale, in the middle of the view (`crate::interface::menu_frame`, #1631).
    root.with_children(|r| crate::interface::in_menu_frame(r, crate::interface::MIDDLE, |r| {
        // The status lines.
        let lines = s.status.clone().unwrap_or_else(|| if s.phase == Phase::Betting { s.bet_lines() } else { [String::new(), String::new()] });
        let status = Node { position_type: PositionType::Absolute, left: u(STATUS.x), top: u(STATUS.y), width: u(800.0), height: u(2.0 * STATUS_STEP), ..default() };
        r.spawn(status).with_children(|b| {
            let w = lines.iter().map(|l| c.text_width(FontKind::Menu, l)).fold(0.0, f32::max);
            if w > 0.0 {
                crate::casino_hud::glass(b, 0.0, 4.0, w, 2.0 * STATUS_STEP - 8.0);
            }
            for (i, l) in lines.iter().enumerate() {
                c.text_in(b, FontKind::Menu, l, 0.0, i as f32 * STATUS_STEP, false, bright);
            }
        });
        let main = Node { position_type: PositionType::Absolute, left: u(EDGE), right: u(EDGE), top: u(EDGE), bottom: u(EDGE), ..default() };
        r.spawn(main).with_children(|m| {
            // The information box.
            let rows = [
                (tr("Your bet:").to_string(), s.bet.to_string()),
                (tr("On the table:").into(), s.table.total().to_string()),
                (tr("Your chips:").into(), s.free_chips().to_string()),
                (trf("Won at {casino}:", &[("casino", &s.casino_name)]), s.earnings.to_string()),
            ];
            let widths: Vec<(f32, f32)> = rows.iter().map(|(t, v)| (c.text_width(FontKind::Menu, t), c.text_width(FontKind::Menu, v))).collect();
            let (column, info_w) = crate::casino_hud::info_fit(INFO.x, ROW_X, VALUE_X, 16.0, &widths);
            let info = Node { position_type: PositionType::Absolute, left: u(INFO_IN), bottom: u(INFO_IN), width: u(info_w), height: u(INFO.y), ..default() };
            m.spawn(info).with_children(|b| {
                crate::casino_hud::glass(b, ROW_X, 2.0, info_w - ROW_X * 2.0, (rows.len() - 1) as f32 * ROW_STEP + 30.0);
                c.image(b, "interface\\shared\\line\\fade_to_top.dds", at(0.0, 0.0, THICK, FADE), bright);
                c.image(b, "interface\\shared\\line\\solid.dds", at(0.0, FADE - THICK, THICK, INFO.y - FADE), bright);
                c.image(b, "interface\\shared\\line\\solid.dds", at(0.0, INFO.y - THICK, info_w - FADE, THICK), bright);
                c.image(b, "interface\\shared\\line\\fade_to_right.dds", at(info_w - FADE - THICK, INFO.y - THICK, FADE, THICK), bright);
                for (i, (title, value)) in rows.iter().enumerate() {
                    let y = i as f32 * ROW_STEP;
                    c.text_in(b, FontKind::Menu, title, ROW_X, y, false, bright);
                    c.text_in(b, FontKind::Menu, value, column, y, true, bright);
                }
            });
            let betting = s.phase == Phase::Betting && s.help.is_none();
            let any = !s.table.bets.is_empty();
            let h = |k: KeyCode| crate::pad::hint(&[k], crate::pad::at_menu(fnv_game::roulette::MENU_CODE));
            let buttons = [
                (tr("Place Bet"), h(KeyCode::KeyW), RouletteInput::Place, betting && s.can_place()),
                (tr("Remove Bet"), h(KeyCode::KeyF), RouletteInput::Remove, betting && any),
                (tr("Finish Bet"), h(KeyCode::KeyS), RouletteInput::Spin, betting && any),
                (tr("Decrease Bet"), h(KeyCode::KeyQ), RouletteInput::Decrease, betting && s.bet > s.table.min_bet),
                (tr("Increase Bet"), h(KeyCode::KeyE), RouletteInput::Increase, betting && s.bet < s.table.max_bet),
                (tr("Exit"), h(KeyCode::KeyR), RouletteInput::Exit, betting),
            ];
            let column = Node { position_type: PositionType::Absolute, right: u(BUTTONS_RIGHT), top: u(BUTTONS_TOP), width: u(0.0), height: u(6.0 * BUTTON_H), ..default() };
            m.spawn(column).with_children(|b| {
                let (lw, hw) = buttons.iter().fold((0.0f32, 0.0f32), |(l, h), (label, hint, _, _)| (l.max(c.text_width(FontKind::Menu, label)), h.max(c.text_width(FontKind::Menu, hint))));
                if !buttons.is_empty() {
                    crate::casino_hud::glass(b, -lw, 4.0, lw + HINT_GAP + hw, buttons.len() as f32 * BUTTON_H - 8.0);
                }
                for (i, (label, hint, cmd, on)) in buttons.into_iter().enumerate() {
                    let color = if on { bright } else { dim };
                    let y = i as f32 * BUTTON_H;
                    let w = c.text_in(b, FontKind::Menu, label, 0.0, y + 9.0, true, color);
                    c.text_in(b, FontKind::Menu, &hint, HINT_GAP, y + 9.0, false, color);
                    if on {
                        b.spawn((Button, RouletteButton(cmd), at(-w - 10.0, y, w + 20.0 + HINT_GAP + 30.0, BUTTON_H)));
                    }
                }
            });
        });
        if let Some(h) = &s.help {
            crate::lockpick::help::spawn(r, c, h, view_w);
        }
    }));
}
