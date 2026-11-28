//! The Caravan menu drawn flat from the game's textures: the table's planks
//! (`terminals\nv_caravan\nv_caravan-table01.dds`), the betting table's Interstate sign
//! (`…table02.dds`) and the cards' own faces and backs (their CCRD's TX00 and TX01). The
//! original draws 3D models of the same (caravan.md §8). Places and sizes are fitted to the
//! owner's screenshots (1280 × 720, here in the interface's units, 960 to the view's
//! height, in a frame 16:9 wide centred in the view, made smaller to fit a narrower view)
//! [observed]; the fonts, lines and help
//! box are the lockpicking menu's (`crate::lockpick::screen`).
//!
//! While nvforever draws the game's table in 3D ([`super::CaravanView`]), the game screen
//! shows that picture instead of the planks and cards drawn here; its cards lie where
//! [`table_cards`] puts them, so everything this screen still draws over them (names,
//! values, outlines, buttons) falls in place [design].

use fnv_util::words::{tr, trf};
use super::{CaravanInput, CaravanScreen, CaravanView, Pick, Session, Stage};
use crate::interface::{amber, at, u};
use crate::lockpick::screen::{Ctx, FontKind, Kit};
use crate::pause::AidTime;
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use crate::ui_pointer::Button;
use bevy::window::PrimaryWindow;
use fnv_game::caravan::{Caravan, Game, Move, MovePreview, Phase, OPPONENT, PLAYER, SOLD_MAX, SOLD_MIN, TRACKS};
use fnv_sim::FormId;
use nvf_assets::GameData;
use std::collections::HashMap;

/// The frame's width: 16:9 of the 960 high view.
pub const FRAME_W: f32 = 960.0 * 16.0 / 9.0;
const PLANKS: &str = "terminals\\nv_caravan\\nv_caravan-table01.dds";
const SIGN: &str = "terminals\\nv_caravan\\nv_caravan-table02.dds";
/// The sign's square, its left and top [observed: the screenshots' sign, -20 to 960 px
/// across, the top cut off].
const SIGN_AT: (f32, f32, f32) = (-27.0, -60.0, 1310.0);
/// A card on the table, in the hand, in the builder's rows and the builder's middle card.
pub const CARD: Vec2 = Vec2::new(111.0, 147.0);
pub const HAND_CARD: Vec2 = Vec2::new(125.0, 165.0);
const ROW_CARD: Vec2 = Vec2::new(150.0, 200.0);
const MIDDLE_CARD: Vec2 = Vec2::new(216.0, 299.0);
/// The middle of each track's caravan: the opponent's and the player's.
const OPPONENT_X: [f32; TRACKS] = [404.0, 667.0, 936.0];
const PLAYER_X: [f32; TRACKS] = [304.0, 576.0, 860.0];
/// The first card's top, and how far each next card lies (up for the opponent's).
const OPPONENT_TOP: f32 = 237.0;
const OPPONENT_STEP: f32 = -23.0;
const PLAYER_TOP: f32 = 480.0;
const PLAYER_STEP: f32 = 43.0;
/// A face card on a card: this far right and down of it, and of the face card before.
const FACE_STEP: Vec2 = Vec2::new(24.0, 12.0);
/// The track names and their values: the values' right ends, the names' left, the rows.
const VALUE_X: [f32; TRACKS] = [233.0, 500.0, 764.0];
const NAME_X: [f32; TRACKS] = [287.0, 551.0, 816.0];
const OPPONENT_ROW: f32 = 400.0;
const PLAYER_ROW: f32 = 440.0;
const OPPONENT_NAMES: [&str; TRACKS] = ["BONEYARD", "REDDING", "SHADY SANDS"];
const PLAYER_NAMES: [&str; TRACKS] = ["DAYGLOW", "NEW RENO", "THE HUB"];
/// Hands and decks.
const PLAYER_HAND: (f32, f32, f32) = (1067.0, 430.0, 28.0);
const OPPONENT_HAND: (f32, f32, f32) = (1080.0, 207.0, 22.0);
const PLAYER_DECK: (f32, f32) = (1360.0, 447.0);
const OPPONENT_DECK: (f32, f32) = (1340.0, 227.0);
/// The top right rows (titles' left, values' right) and the buttons' right end and tops.
const INFO_X: (f32, f32) = (1253.0, 1643.0);
const INFO_Y: [f32; 3] = [40.0, 84.0, 128.0];
const BUTTONS_RIGHT: f32 = 1664.0;
/// The game's four buttons and Tips under them, 48 apart to leave the guide room.
const PLAY_BUTTONS: [f32; 5] = [692.0, 740.0, 788.0, 836.0, 884.0];
/// The guide's box: left of the buttons and the opponent's "thinking" line,
/// under the hand and the deck, right of the player's third caravan with three face cards
/// on its last card, down to the frame's foot. Its lines start at the top, in the menu's
/// font while they fit, else in the help box's smaller one (a tie note
/// over the buttons ran into the "thinking" line; 2026-11-16 frames: a tie note with a
/// card's tip reached up over the hand) [design].
const GUIDE_X: f32 = 1000.0;
const GUIDE_TOP: f32 = 622.0;
const GUIDE_FLOOR: f32 = 930.0;
/// Between the box and the buttons or the "thinking" line.
const GUIDE_GAP: f32 = 24.0;
/// The narrowest the box gets, however wide the buttons' words.
const GUIDE_MIN_W: f32 = 240.0;
const GUIDE_STEP: f32 = 30.0;
const GUIDE_BODY_STEP: f32 = 24.0;
/// The opponent's "thinking" line, alone just above the first button.
const THINK_TOP: f32 = PLAY_BUTTONS[0] - 36.0;
const _: () = assert!(GUIDE_X >= PLAYER_X[2] + CARD.x / 2.0 + 3.0 * FACE_STEP.x, "the guide keeps right of the third caravan");
const _: () = assert!(GUIDE_TOP >= PLAYER_DECK.1 + HAND_CARD.y + 8.0 && GUIDE_TOP >= PLAYER_HAND.1 + HAND_CARD.y + 8.0, "the guide starts under the deck and the hand");
const _: () = assert!(THINK_TOP + 34.0 <= PLAY_BUTTONS[0], "the thinking line is clear of the first button");
const BET_BUTTONS: [f32; 4] = [705.0, 756.0, 808.0, 858.0];
const HINT_GAP: f32 = 16.0;
/// The betting box: rows, titles' left, values' right, its bracket.
const BET_ROWS: [f32; 3] = [778.0, 824.0, 868.0];
const BET_X: (f32, f32) = (67.0, 533.0);
/// The bet box's bracket: left, top, width, height.
const BET_BRACKET: (f32, f32, f32, f32) = (40.0, 750.0, 545.0, 170.0);
/// The gap the help box keeps from the bet screen's words and buttons [design].
const HELP_GAP: f32 = 40.0;
/// The builder: rows' tops, the middle card's tops, the step between cards.
const ROW_TOPS: (f32, f32) = (100.0, 600.0);
const MIDDLE_TOPS: (f32, f32) = (44.0, 560.0);
const ROW_STEP: f32 = 53.0;
const ROW_SHOWN: usize = 14;
const THICK: f32 = 2.0;
/// A place the chosen card can go: outlined, the one shown brighter.
const MARK: f32 = 3.0;
/// The carved names' colour [design: pale wood].
const CARVED: Color = Color::srgba(0.86, 0.83, 0.76, 0.55);
/// A caravan over 26 shows its value in red [observed].
const OVER: Color = Color::srgb(0.9, 0.15, 0.1);
/// A caravan in the sold range (21 to 26) shows its value in white [observed: buckallobill, in game, FNV 1.4.0.525; the menu file sets no colour for the values,
/// `menus\prefabs\caravan_game_menu.xml` `CGM_PlayerTrack1Value` …].
const SOLD: Color = Color::srgb(1.0, 1.0, 1.0);

/// A caravan value's colour: white once in the sold range, red over it, else the menu's.
pub fn value_colour(v: u32, normal: Color) -> Color {
    match v {
        SOLD_MIN..=SOLD_MAX => SOLD,
        _ if v > SOLD_MAX => OVER,
        _ => normal,
    }
}
/// A winning caravan (21–26, ahead of the opponent) highlighted in white when
/// BetterCaravan's `bHighlightWinningCaravans` is enabled [documented: Better Caravan
/// v1.9 (`https://www.nexusmods.com/newvegas/mods/81002`), shipped
/// `Data\NVSE\Plugins\BetterCaravan.ini` comment "; highlight winning caravans in white"].
const WINNING: Color = Color::WHITE;
/// A previewed caravan value that changes under the hovered move when BetterCaravan's
/// `bPreviewMoveResult` is enabled [design].
const PREVIEW: Color = Color::srgb(1.0, 0.96, 0.72);

#[derive(Component)]
pub struct CaravanRoot;

#[derive(Component, Clone, Copy)]
pub struct CaravanButton(pub CaravanInput);

#[derive(Component, Clone, Copy)]
pub struct DrawnAlpha(f32);

fn view_key(s: &Session, view_w: f32, table: bool) -> String {
    let g = s.game.as_ref().map(|g| {
        let side = |i: usize| g.sides[i].caravans.iter().map(|c| c.cards.iter().map(|p| format!("{}{}", p.card.short(), p.faces.len())).collect::<String>()).collect::<Vec<_>>().join("|");
        format!("{}:{}:{}:{}:{}:{:?}", side(0), side(1), g.sides[0].hand.len(), g.sides[1].hand.len(), g.sides[0].deck.len() * 100 + g.sides[1].deck.len(), g.turn)
    });
    format!(
        "{:?}:{:?}:{:?}:{}:{}:{:?}:{:?}:{}:{:?}:{}:{}:{}:{}:{}:{}:{view_w:.0}:{:?}",
        s.stage,
        s.bet,
        s.deck,
        s.cursor,
        s.hand,
        s.pick,
        s.status,
        s.help.is_some(),
        g,
        s.players_turn(),
        s.record.won,
        s.leaving.is_some(),
        s.better.highlight_winning_caravans,
        s.better.preview_move_result,
        s.better.view_npc_cards,
        crate::pad::labels(),
    ) + &s.guide()
        + if table { ":3d" } else { "" }
}

pub fn view_width(window: Option<&Window>) -> f32 {
    window.map_or(FRAME_W, |w| 960.0 * w.width() / w.height().max(1.0))
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut commands: Commands,
    screen: Res<CaravanScreen>,
    data: Option<Res<GameData>>,
    window: Query<&Window, With<PrimaryWindow>>,
    view: Option<Res<CaravanView>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    glyphs: Res<crate::pad::PadGlyphs>,
    roots: Query<Entity, With<CaravanRoot>>,
    _time: AidTime,
    (mut kit, mut shown, mut hints): (Local<Option<Kit>>, Local<Option<String>>, Local<String>),
) {
    let view_w = view_width(window.single().ok());
    let table = view.as_ref().map(|v| v.image.clone()).filter(|_| screen.0.as_ref().is_some_and(|s| s.stage == Stage::Play));
    let key = screen.0.as_ref().map(|s| view_key(s, view_w, table.is_some()));
    if key == *shown {
        return;
    }
    // The hints, logged as they change (the device's names, #1662).
    if let Some(line) = screen.0.as_ref().map(|s| s.hint_line()).filter(|l| *l != *hints) {
        info!("caravan: hints: {line}");
        *hints = line;
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
    *shown = key;
    let Some(s) = screen.0.as_ref() else { return };
    let bc = crate::interface::bc_supported(formats.as_deref());
    let kit = kit.get_or_insert_with(|| Kit::load(data.as_deref(), &mut images, bc));
    let mut c = Ctx { kit, data: data.as_deref(), images: &mut images, bc, glyphs: &glyphs };
    spawn(&mut commands, &mut c, s, view_w, table);
}

fn texture(s: &Session, f: FormId, face: bool) -> String {
    let (front, back) = s.textures.get(&f).cloned().unwrap_or_default();
    (if face { front } else { back }).unwrap_or_default()
}

fn spawn(commands: &mut Commands, c: &mut Ctx, s: &Session, view_w: f32, table: Option<Handle<Image>>) {
    let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    let dim = if s.help.is_some() { 0.47 } else { 1.0 };
    let flat = table.is_none();
    commands.spawn((CaravanRoot, full.clone(), GlobalZIndex(106), BackgroundColor(Color::BLACK))).with_children(|r| {
        match table {
            Some(image) => {
                r.spawn((full.clone(), ImageNode { color: Color::srgb(dim, dim, dim), ..ImageNode::new(image) }));
            }
            None => c.image(r, PLANKS, full.clone(), Color::srgb(0.78 * dim, 0.74 * dim, 0.68 * dim)),
        }
        // The table at the UI scale, in the middle of the view (`crate::interface::menu_frame`,
        // #1631); the planks (or the 3D table) fill the view.
        crate::interface::in_menu_frame(r, crate::interface::MIDDLE, |r| {
            // A view narrower than 16:9 (16:10, 4:3) shows the whole frame, smaller.
            let frame = Node { position_type: PositionType::Absolute, left: u((view_w - FRAME_W) / 2.0), top: u(0.0), width: u(FRAME_W), height: u(960.0), ..default() };
            let fit = UiTransform { scale: Vec2::splat((view_w / FRAME_W).min(1.0)), ..default() };
            r.spawn((frame, fit)).with_children(|f| match s.stage {
                Stage::Bet => bet(f, c, s, dim),
                Stage::Deck => deck(f, c, s, dim),
                Stage::Play => play(f, c, s, dim, flat),
                Stage::Results => results(f, c, s, dim),
        });
        if let Some(h) = &s.help {
            let clear = help_clear(c, s, view_w);
            crate::lockpick::help::spawn_clear(r, c, h, view_w, clear);
        }
        });
    });
}

/// The room the help box leaves the bet screen's words and buttons: above the bet box's
/// bracket and left of the buttons' labels, with a gap (issue #1663) [design]. The table's
/// frame is scaled about its middle in a view narrower than 16:9, so its places are taken
/// into the view's units. Other stages: no limits.
fn help_clear(c: &mut Ctx, s: &Session, view_w: f32) -> crate::lockpick::help::Clear {
    if s.stage != Stage::Bet {
        return crate::lockpick::help::Clear::NONE;
    }
    let k = (view_w / FRAME_W).min(1.0);
    let x = |fx: f32| view_w / 2.0 + (fx - FRAME_W / 2.0) * k;
    let y = |fy: f32| 480.0 + (fy - 480.0) * k;
    let labels = s.buttons().iter().map(|(label, ..)| BUTTONS_RIGHT - HINT_GAP - 34.0 - c.text_width(FontKind::Menu, label)).fold(BUTTONS_RIGHT, f32::min);
    let clear = crate::lockpick::help::Clear { bottom: y(BET_BRACKET.1 - HELP_GAP), right: x(labels - HELP_GAP) };
    let (bottom, right) = crate::lockpick::help::frame_lines(crate::lockpick::help::box_rect(view_w, clear));
    info!(
        "caravan: help box on the bet screen ({view_w:.0} wide): {:.0} above the bet box, {:.0} left of the buttons",
        y(BET_BRACKET.1) - bottom,
        x(labels) - right
    );
    clear
}

fn lit(on: bool) -> Color {
    amber(if on { 255.0 } else { 120.0 })
}

/// A button: its label right-aligned at `right`, its key after it; clickable when lit.
#[allow(clippy::too_many_arguments)]
fn button(p: &mut ChildSpawnerCommands, c: &mut Ctx, label: &str, key: &str, right: f32, top: f32, cmd: CaravanInput, on: bool) {
    let w = c.text_in(p, FontKind::Menu, label, right - HINT_GAP - 34.0, top, true, lit(on));
    c.text_in(p, FontKind::Menu, key, right - 34.0, top, false, lit(on));
    if on {
        p.spawn((Button, CaravanButton(cmd), at(right - HINT_GAP - 34.0 - w - 10.0, top - 9.0, w + HINT_GAP + 54.0, 54.0)));
    }
}

/// One soft backdrop under a column of buttons (labels and keys, their tops) [design
/// 2026-11-09].
fn buttons_backdrop(p: &mut ChildSpawnerCommands, c: &mut Ctx, rows: &[(&str, &str, f32)]) {
    let (Some(first), Some(last)) = (rows.first(), rows.last()) else { return };
    let lw = rows.iter().map(|r| c.text_width(FontKind::Menu, r.0)).fold(0.0, f32::max);
    let kw = rows.iter().map(|r| c.text_width(FontKind::Menu, r.1)).fold(0.0, f32::max);
    let left = BUTTONS_RIGHT - HINT_GAP - 34.0 - lw;
    crate::casino_hud::glass(p, left, first.2, BUTTONS_RIGHT - 34.0 + kw - left, last.2 - first.2 + 34.0);
}

fn status(p: &mut ChildSpawnerCommands, c: &mut Ctx, s: &Session, top: f32) {
    if let Some(line) = &s.status {
        let w = c.text_width(FontKind::Menu, line);
        crate::casino_hud::glass(p, BUTTONS_RIGHT - w, top + 2.0, w, 32.0);
        c.text_in(p, FontKind::Menu, line, BUTTONS_RIGHT, top, true, amber(255.0));
    }
}

fn sign(p: &mut ChildSpawnerCommands, c: &mut Ctx, dim: f32) {
    let (x, y, side) = SIGN_AT;
    c.image(p, SIGN, at(x, y, side, side), Color::srgb(0.85 * dim, 0.82 * dim, 0.76 * dim));
}

fn bracket(p: &mut ChildSpawnerCommands, c: &mut Ctx, left: f32, top: f32, w: f32, h: f32) {
    let b = amber(255.0);
    c.image(p, "interface\\shared\\line\\fade_to_top.dds", at(left, top, THICK, 30.0), b);
    c.image(p, "interface\\shared\\line\\solid.dds", at(left, top + 28.0, THICK, h - 28.0), b);
    c.image(p, "interface\\shared\\line\\solid.dds", at(left, top + h - THICK, w - 30.0, THICK), b);
    c.image(p, "interface\\shared\\line\\fade_to_right.dds", at(left + w - 32.0, top + h - THICK, 30.0, THICK), b);
}

fn bet(p: &mut ChildSpawnerCommands, c: &mut Ctx, s: &Session, dim: f32) {
    sign(p, c, dim);
    let b = amber(255.0);
    let (x, y, w, h) = BET_BRACKET;
    bracket(p, c, x, y, w, h);
    let rows = [(s.opponent_name.clone(), s.bet.bid), (tr("Current Ante:").into(), s.bet.ante), (tr("Total Funds:").into(), s.bet.funds)];
    crate::casino_hud::glass(p, BET_X.0, BET_ROWS[0] + 2.0, BET_X.1 - BET_X.0, BET_ROWS[2] - BET_ROWS[0] + 32.0);
    for ((title, value), y) in rows.iter().zip(BET_ROWS) {
        c.text_in(p, FontKind::Menu, title, BET_X.0, y, false, b);
        c.text_in(p, FontKind::Menu, &value.to_string(), BET_X.1, y, true, b);
    }
    let all = s.buttons();
    let rows: Vec<(&str, &str, f32)> = all.iter().zip(BET_BUTTONS).map(|((l, k, _, _), t)| (*l, k.as_str(), t)).collect();
    buttons_backdrop(p, c, &rows);
    for ((label, key, cmd, on), top) in all.into_iter().zip(BET_BUTTONS) {
        button(p, c, label, &key, BUTTONS_RIGHT, top, cmd, on);
    }
    status(p, c, s, 10.0);
}

/// A card's texture (a plain card colour without it).
fn card(p: &mut ChildSpawnerCommands, c: &mut Ctx, path: &str, x: f32, y: f32, size: Vec2, light: f32) {
    c.image(p, path, at(x, y, size.x, size.y), Color::srgb(light, light, light));
}

fn deck(p: &mut ChildSpawnerCommands, c: &mut Ctx, s: &Session, dim: f32) {
    let b = amber(255.0);
    let mid = FRAME_W / 2.0;
    for (row, (top, middle_top)) in [(ROW_TOPS.0, MIDDLE_TOPS.0), (ROW_TOPS.1, MIDDLE_TOPS.1)].into_iter().enumerate() {
        // The cards outside in first, the middle one last, on top.
        let n = s.owned.len();
        let mut order: Vec<usize> = Vec::new();
        for d in (1..=ROW_SHOWN).rev() {
            if s.cursor >= d {
                order.push(s.cursor - d);
            }
            if s.cursor + d < n {
                order.push(s.cursor + d);
            }
        }
        if s.cursor < n {
            order.push(s.cursor);
        }
        for i in order {
            let f = s.owned[i];
            // The top row shows the deck's cards face up, the bottom row the others.
            let face = (row == 0) == s.in_deck(f);
            let path = texture(s, f, face);
            let off = i as f32 - s.cursor as f32;
            let (x, y, size) = if i == s.cursor {
                (mid - MIDDLE_CARD.x / 2.0, middle_top, MIDDLE_CARD)
            } else {
                let x = mid + off * ROW_STEP + if off < 0.0 { -MIDDLE_CARD.x / 2.0 - ROW_CARD.x + ROW_STEP } else { MIDDLE_CARD.x / 2.0 - ROW_STEP };
                (x, top, ROW_CARD)
            };
            card(p, c, &path, x, y, size, dim);
            if i != s.cursor {
                p.spawn((Button, CaravanButton(CaravanInput::CursorTo(i)), at(x, y, ROW_STEP.min(size.x), size.y)));
            }
        }
    }
    c.text_in(p, FontKind::Menu, tr("Caravan Deck"), 87.0, 395.0, false, b);
    c.text_in(p, FontKind::Menu, &s.deck.len().to_string(), 507.0, 395.0, true, b);
    button(p, c, tr("Randomize Caravan Deck"), &s.hint(&[KeyCode::KeyS]), 1380.0, 395.0, CaravanInput::Randomize, true);
    c.image(p, "interface\\shared\\line\\solid.dds", at(33.0, 480.0, FRAME_W - 66.0, THICK), b);
    c.text_in(p, FontKind::Menu, "<", 33.0, 462.0, false, b);
    c.text_in(p, FontKind::Menu, ">", FRAME_W - 33.0, 462.0, true, b);
    p.spawn((Button, CaravanButton(CaravanInput::Step(-1)), at(20.0, 455.0, 50.0, 50.0)));
    p.spawn((Button, CaravanButton(CaravanInput::Step(1)), at(FRAME_W - 70.0, 455.0, 50.0, 50.0)));
    c.text_in(p, FontKind::Menu, tr("Total Cards:"), 87.0, 515.0, false, b);
    c.text_in(p, FontKind::Menu, &s.owned.len().to_string(), 507.0, 515.0, true, b);
    c.text_in(p, FontKind::Menu, tr("All Cards"), 965.0, 527.0, false, b);
    let here = s.owned.get(s.cursor).copied();
    button(p, c, tr("Add"), &s.hint(&[KeyCode::KeyW]), 600.0, 862.0, CaravanInput::Add, here.is_some_and(|f| !s.in_deck(f)));
    button(p, c, tr("Remove"), &s.hint(&[KeyCode::KeyA]), 1330.0, 862.0, CaravanInput::Remove, here.is_some_and(|f| s.in_deck(f)));
    button(p, c, tr("Play Caravan"), &s.hint(&[KeyCode::KeyF]), BUTTONS_RIGHT, 862.0, CaravanInput::PlayDeck, s.deck.len() >= fnv_game::caravan::MIN_DECK);
    if let Some(desc) = active_face_description(s) {
        c.text_in(p, FontKind::Menu, desc, 40.0, 915.0, false, b);
    }
    status(p, c, s, 10.0);
}

/// Everywhere a card can lie during a game, as rectangles (min, max) in the frame (y down),
/// with `pad` round each: the six caravans as far as they can grow (the opponent's up to
/// the top, the player's down to the bottom, faces stacked on the side), both hands at
/// their fullest with the raised card, and both decks [design: the layout above]. The pot's
/// money keeps out of them [design: "cash can go under everything else just
/// stay away"].
pub fn card_zones(pad: f32) -> Vec<(Vec2, Vec2)> {
    zones_for(pad, None)
}

/// How many cards of a caravan the pot's money keeps clear of ([`money_zones`]).
pub const MONEY_CARAVAN_CARDS: usize = 4;

/// Where the pot's money keeps out of: [`card_zones`], but each caravan only as far as
/// its first [`MONEY_CARAVAN_CARDS`] cards, so the planks past them (along the top over
/// the opponent's caravans, along the bottom under the player's) hold money too; a longer
/// caravan lies over it [design: "space that can be
/// used for more piles" on those planks].
pub fn money_zones(pad: f32) -> Vec<(Vec2, Vec2)> {
    zones_for(pad, Some(MONEY_CARAVAN_CARDS))
}

/// [`card_zones`], each caravan as far as `cards` cards reach (or all the way).
fn zones_for(pad: f32, cards: Option<usize>) -> Vec<(Vec2, Vec2)> {
    // Room for a few face cards stacked on a number card.
    let faces = FACE_STEP * 3.0;
    // The opponent's caravans grow up the screen, the player's down it.
    let (top, bottom) = match cards {
        Some(n) => ((OPPONENT_TOP + OPPONENT_STEP * (n - 1) as f32).max(0.0), (PLAYER_TOP + PLAYER_STEP * (n - 1) as f32 + CARD.y + faces.y).min(960.0)),
        None => (0.0, 960.0),
    };
    let mut z = Vec::new();
    for t in 0..TRACKS {
        let x0 = OPPONENT_X[t] - CARD.x / 2.0;
        z.push((Vec2::new(x0, top), Vec2::new(x0 + CARD.x + faces.x, OPPONENT_TOP + CARD.y + faces.y)));
        let x0 = PLAYER_X[t] - CARD.x / 2.0;
        z.push((Vec2::new(x0, PLAYER_TOP), Vec2::new(x0 + CARD.x + faces.x, bottom)));
    }
    let last = (fnv_game::caravan::HAND - 1) as f32;
    let (ox, oy, ostep) = OPPONENT_HAND;
    z.push((Vec2::new(ox, oy.min(OPPONENT_DECK.1)), Vec2::new((ox + ostep * last + CARD.x).max(OPPONENT_DECK.0 + CARD.x), (oy.max(OPPONENT_DECK.1)) + CARD.y)));
    let (hx, hy, hstep) = PLAYER_HAND;
    z.push((Vec2::new(hx, hy - 28.0), Vec2::new((hx + hstep * last + HAND_CARD.x).max(PLAYER_DECK.0 + HAND_CARD.x), hy.max(PLAYER_DECK.1) + HAND_CARD.y)));
    z.into_iter().map(|(a, b)| (a - Vec2::splat(pad), b + Vec2::splat(pad))).collect()
}

/// Where the game screen's words are during a game, as rectangles (min, max) in the frame
/// (y down), with `pad` round each: the caravans' values and names, the top right counters
/// (and the status line beside them), the guide's box left of the buttons, the "thinking"
/// line over them and the buttons [design: the layout above]. The pot's money
/// keeps out of them too, so no number or label lies on a cap [design].
pub fn text_zones(pad: f32) -> Vec<(Vec2, Vec2)> {
    let rows_left = VALUE_X[0] - 70.0;
    let rows_right = NAME_X[TRACKS - 1] + 230.0;
    let z = [
        (Vec2::new(rows_left, OPPONENT_ROW - 6.0), Vec2::new(rows_right, PLAYER_ROW + 38.0)),
        (Vec2::new(INFO_X.0 - 20.0, 0.0), Vec2::new(FRAME_W, INFO_Y[2] + 40.0)),
        (Vec2::new(GUIDE_X, GUIDE_TOP), Vec2::new(BUTTONS_RIGHT - 240.0, GUIDE_FLOOR)),
        (Vec2::new(BUTTONS_RIGHT - 380.0, THINK_TOP), Vec2::new(FRAME_W, PLAY_BUTTONS[0])),
        (Vec2::new(BUTTONS_RIGHT - 380.0, PLAY_BUTTONS[0] - 10.0), Vec2::new(FRAME_W, PLAY_BUTTONS[4] + 44.0)),
    ];
    z.into_iter().map(|(a, b)| (a - Vec2::splat(pad), b + Vec2::splat(pad))).collect()
}

/// Where a caravan's card `i` lies (its top left).
fn place(side: usize, track: usize, i: usize) -> Vec2 {
    if side == OPPONENT {
        Vec2::new(OPPONENT_X[track] - CARD.x / 2.0, OPPONENT_TOP + OPPONENT_STEP * i as f32)
    } else {
        Vec2::new(PLAYER_X[track] - CARD.x / 2.0, PLAYER_TOP + PLAYER_STEP * i as f32)
    }
}

fn caravan(p: &mut ChildSpawnerCommands, c: &mut Ctx, s: &Session, side: usize, track: usize, k: &Caravan, dim: f32) {
    for (i, placed) in k.cards.iter().enumerate() {
        let at0 = place(side, track, i);
        card(p, c, &texture(s, placed.card.form, true), at0.x, at0.y, CARD, dim);
        for (j, f) in placed.faces.iter().enumerate() {
            let o = at0 + FACE_STEP * (j + 1) as f32;
            card(p, c, &texture(s, f.form, true), o.x, o.y, CARD, dim);
        }
    }
}

/// An outline round a place: lit for the one shown; clickable.
fn mark(p: &mut ChildSpawnerCommands, c: &mut Ctx, at0: Vec2, size: Vec2, shown: bool, cmd: CaravanInput) {
    let col = if shown { amber(255.0) } else { amber(110.0) };
    let t = if shown { MARK + 1.0 } else { MARK };
    let solid = "interface\\shared\\line\\solid.dds";
    c.image(p, solid, at(at0.x - t, at0.y - t, size.x + 2.0 * t, t), col);
    c.image(p, solid, at(at0.x - t, at0.y + size.y, size.x + 2.0 * t, t), col);
    c.image(p, solid, at(at0.x - t, at0.y, t, size.y), col);
    c.image(p, solid, at(at0.x + size.x, at0.y, t, size.y), col);
    p.spawn((Button, CaravanButton(cmd), at(at0.x, at0.y, size.x, size.y)));
}

fn marks(p: &mut ChildSpawnerCommands, c: &mut Ctx, s: &Session, g: &Game) {
    match &s.pick {
        Pick::Place { moves, at: shown } => {
            for (n, m) in moves.iter().enumerate() {
                let Move::Play { side, track, on, .. } = *m else { continue };
                let k = &g.sides[side].caravans[track];
                let spot = match on {
                    None => place(side, track, k.cards.len()),
                    Some(i) => place(side, track, i) + FACE_STEP * (k.cards[i].faces.len() + 1) as f32,
                };
                mark(p, c, spot, CARD, n == *shown, CaravanInput::Place(n));
            }
        }
        Pick::Track { tracks, at: shown } => {
            for (n, &t) in tracks.iter().enumerate() {
                let len = g.sides[PLAYER].caravans[t].cards.len().max(1);
                let top = place(PLAYER, t, 0);
                let h = CARD.y + PLAYER_STEP * (len - 1) as f32;
                mark(p, c, top, Vec2::new(CARD.x, h), n == *shown, CaravanInput::Place(n));
            }
        }
        Pick::Hand => {}
    }
}

/// A card the game screen shows: its key (its place: `c<side>:<track>:<n>`, a face card on
/// it `…:f<j>`; a hand's `h<side>:<form>:<copy>`; a deck's top `d<side>`), its form, face
/// up or down, its top left and size in the frame's units, raised (chosen in the hand).
/// Listed bottom to top.
#[derive(Debug, Clone, PartialEq)]
pub struct TableCard {
    pub key: String,
    pub form: FormId,
    pub face_up: bool,
    pub at: Vec2,
    pub size: Vec2,
    pub raised: bool,
}

/// Every card of the game screen, where the flat screen draws it (shared with the 3D
/// table so the two agree).
pub fn table_cards(s: &Session) -> Vec<TableCard> {
    let mut out = Vec::new();
    let Some(g) = s.game.as_ref() else { return out };
    let mut push = |key: String, form: FormId, face_up: bool, at: Vec2, size: Vec2, raised: bool| out.push(TableCard { key, form, face_up, at, size, raised });
    // The opponent's caravans first: the player's lie over them.
    for side in [OPPONENT, PLAYER] {
        for (t, k) in g.sides[side].caravans.iter().enumerate() {
            for (i, placed) in k.cards.iter().enumerate() {
                let at0 = place(side, t, i);
                push(format!("c{side}:{t}:{i}"), placed.card.form, true, at0, CARD, false);
                for (j, f) in placed.faces.iter().enumerate() {
                    push(format!("c{side}:{t}:{i}:f{j}"), f.form, true, at0 + FACE_STEP * (j + 1) as f32, CARD, false);
                }
            }
        }
    }
    let hand_keys = |hand: &[fnv_game::caravan::Card], side: usize| {
        let mut seen: HashMap<FormId, usize> = HashMap::new();
        hand.iter()
            .map(|c| {
                let n = seen.entry(c.form).or_default();
                *n += 1;
                format!("h{side}:{:08X}:{}", c.form.0, *n - 1)
            })
            .collect::<Vec<_>>()
    };
    let (ox, oy, ostep) = OPPONENT_HAND;
    for (i, (card_, key)) in g.sides[OPPONENT].hand.iter().zip(hand_keys(&g.sides[OPPONENT].hand, OPPONENT)).enumerate() {
        push(key, card_.form, s.better.view_npc_cards, Vec2::new(ox + ostep * i as f32, oy), CARD, false);
    }
    if let Some(top) = g.sides[OPPONENT].deck.last() {
        push(format!("d{OPPONENT}"), top.form, false, Vec2::new(OPPONENT_DECK.0, OPPONENT_DECK.1), CARD, false);
    }
    let (hx, hy, hstep) = PLAYER_HAND;
    let mine = s.players_turn();
    let chosen = chosen_card(s);
    let hand = &g.sides[PLAYER].hand;
    for (i, (card_, key)) in hand.iter().zip(hand_keys(hand, PLAYER)).enumerate() {
        let raised = mine && (i == s.hand || chosen == Some(i));
        let y = hy - if raised { 28.0 } else { 0.0 };
        push(key, card_.form, true, Vec2::new(hx + hstep * i as f32, y), HAND_CARD, raised);
    }
    if let Some(top) = g.sides[PLAYER].deck.last() {
        push(format!("d{PLAYER}"), top.form, false, Vec2::new(PLAYER_DECK.0, PLAYER_DECK.1), HAND_CARD, false);
    }
    out
}

/// The hand's card a place is being picked for.
fn chosen_card(s: &Session) -> Option<usize> {
    match &s.pick {
        Pick::Place { moves, .. } => moves.first().and_then(|m| if let Move::Play { card, .. } = m { Some(*card) } else { None }),
        _ => None,
    }
}

/// Previewed caravan values and winners for the currently highlighted move when
/// `BetterCaravanConfig::preview_move_result` is enabled [design].
pub(crate) fn active_preview(s: &Session) -> Option<MovePreview> {
    if !s.better.preview_move_result || !s.players_turn() {
        return None;
    }
    let g = s.game.as_ref()?;
    let m = match &s.pick {
        Pick::Place { moves, at } => *moves.get(*at)?,
        Pick::Track { tracks, at } => Move::DiscardTrack { track: *tracks.get(*at)? },
        Pick::Hand => return None,
    };
    g.preview_move(PLAYER, m)
}

/// Displayed text and colour for a caravan's value on `(side, track)`, honoring optional
/// `preview` (`bPreviewMoveResult`) and `highlight_winning` (`bHighlightWinningCaravans`).
/// When both are off/absent, returns the original's text and colour ([`value_colour`]:
/// white in the sold range, `OVER` red past it, else amber).
pub(crate) fn value_display(g: &Game, side: usize, track: usize, preview: Option<&MovePreview>, highlight_winning: bool) -> (String, Color) {
    let current = g.sides[side].caravans[track].value();
    let b = amber(255.0);
    if let Some(p) = preview {
        let next = p.values[side][track];
        let winning = highlight_winning && p.winners[track] == Some(side);
        let text = if next != current { format!("{current}->{next}") } else { current.to_string() };
        let color = if next > SOLD_MAX {
            OVER
        } else if winning {
            WINNING
        } else if next != current {
            PREVIEW
        } else {
            value_colour(current, b)
        };
        (text, color)
    } else {
        let winning = highlight_winning && g.track_winner(track) == Some(side);
        let color = if current > SOLD_MAX {
            OVER
        } else if winning {
            WINNING
        } else {
            value_colour(current, b)
        };
        (current.to_string(), color)
    }
}

/// Face-card reminder configured in `BetterCaravan.ini` (`sDescriptionJack/Queen/King/Joker`)
/// for the card the player is currently viewing in `Deck` or `Play`, or `None` [design].
pub(crate) fn active_face_description(s: &Session) -> Option<&str> {
    match s.stage {
        Stage::Deck => {
            let f = s.owned.get(s.cursor)?;
            let rank = *s.card_ranks.get(f)?;
            s.better.face_description(rank)
        }
        Stage::Play => {
            let g = s.game.as_ref()?;
            let idx = match &s.pick {
                Pick::Place { moves, .. } => moves.first().and_then(|m| if let Move::Play { card, .. } = m { Some(*card) } else { None })?,
                Pick::Hand => s.hand,
                Pick::Track { .. } => return None,
            };
            let card = g.sides[PLAYER].hand.get(idx)?;
            s.better.face_description(card.rank)
        }
        _ => None,
    }
}

/// A card's face or back texture (under `textures\`), for the 3D table.
pub fn card_texture(s: &Session, f: FormId, face: bool) -> String {
    texture(s, f, face)
}

fn play(p: &mut ChildSpawnerCommands, c: &mut Ctx, s: &Session, dim: f32, flat: bool) {
    let Some(g) = s.game.as_ref() else { return };
    let b = amber(255.0);
    let preview = active_preview(s);
    // The carved names and the caravans' values.
    for t in 0..TRACKS {
        for (side, row, names) in [(OPPONENT, OPPONENT_ROW, OPPONENT_NAMES), (PLAYER, PLAYER_ROW, PLAYER_NAMES)] {
            c.text_in(p, FontKind::Menu, names[t], NAME_X[t], row, false, CARVED);
            // Both sides; BetterCaravan's preview and winning highlight over the original's colours.
            let (text, color) = value_display(g, side, t, preview.as_ref(), s.better.highlight_winning_caravans);
            c.text_in(p, FontKind::Menu, &text, VALUE_X[t], row, true, color);
        }
    }
    // The opponent's caravans are drawn first: the player's lie over them.
    if flat {
        for side in [OPPONENT, PLAYER] {
            for (t, k) in g.sides[side].caravans.iter().enumerate() {
                caravan(p, c, s, side, t, k, dim);
            }
        }
    }
    // Hands and decks.
    let (ox, oy, ostep) = OPPONENT_HAND;
    if flat {
        for (i, card_) in g.sides[OPPONENT].hand.iter().enumerate() {
            card(p, c, &texture(s, card_.form, s.better.view_npc_cards), ox + ostep * i as f32, oy, CARD, dim);
        }
        if let Some(top) = g.sides[OPPONENT].deck.last() {
            card(p, c, &texture(s, top.form, false), OPPONENT_DECK.0, OPPONENT_DECK.1, CARD, dim);
        }
    }
    c.text_in(p, FontKind::Menu, &g.sides[OPPONENT].deck.len().to_string(), OPPONENT_DECK.0 + CARD.x + 22.0, OPPONENT_DECK.1 + CARD.y - 40.0, true, b);
    let (hx, hy, hstep) = PLAYER_HAND;
    let mine = s.players_turn();
    let chosen = chosen_card(s);
    let hand = &g.sides[PLAYER].hand;
    for (i, card_) in hand.iter().enumerate() {
        let raised = mine && (i == s.hand || chosen == Some(i));
        let y = hy - if raised { 28.0 } else { 0.0 };
        let x = hx + hstep * i as f32;
        if flat {
            card(p, c, &texture(s, card_.form, true), x, y, HAND_CARD, if raised || !mine { dim } else { 0.85 * dim });
        }
        let w = if i + 1 == hand.len() { HAND_CARD.x } else { hstep };
        if mine {
            p.spawn((Button, CaravanButton(CaravanInput::PickCard(i)), at(x, y, w, HAND_CARD.y)));
        }
    }
    if let Some(top) = g.sides[PLAYER].deck.last().filter(|_| flat) {
        card(p, c, &texture(s, top.form, false), PLAYER_DECK.0, PLAYER_DECK.1, HAND_CARD, dim);
    }
    c.text_in(p, FontKind::Menu, &g.sides[PLAYER].deck.len().to_string(), PLAYER_DECK.0 + HAND_CARD.x + 25.0, PLAYER_DECK.1 + HAND_CARD.y - 40.0, true, b);
    if mine {
        marks(p, c, s, g);
    }
    // The top right rows.
    let rows = [(tr("Ante Pot:"), s.bet.pot().to_string()), (tr("Net Total:"), s.record.net().to_string()), (tr("Win/Loss:"), format!("{}/{}", s.record.won, s.record.lost))];
    crate::casino_hud::glass(p, INFO_X.0, INFO_Y[0] + 2.0, INFO_X.1 - INFO_X.0, INFO_Y[2] - INFO_Y[0] + 32.0);
    // A player game with no stake shows no pot [design: bets start at 0].
    for ((title, value), y) in rows.iter().zip(INFO_Y) {
        c.text_in(p, FontKind::Menu, title, INFO_X.0, y, false, b);
        c.text_in(p, FontKind::Menu, value, INFO_X.1, y, true, b);
    }
    let thinking_line = trf("{name} is thinking…", &[("name", &s.opponent_name)]);
    let thinking = g.phase != Phase::Over && !mine && s.help.is_none();
    let all = s.buttons();
    let rows: Vec<(&str, &str, f32)> = all.iter().zip(PLAY_BUTTONS).map(|((l, k, _, _), t)| (*l, k.as_str(), t)).collect();
    buttons_backdrop(p, c, &rows);
    let labels = rows.iter().map(|r| c.text_width(FontKind::Menu, r.0)).fold(0.0, f32::max);
    for ((label, key, cmd, on), top) in all.into_iter().zip(PLAY_BUTTONS) {
        button(p, c, label, &key, BUTTONS_RIGHT, top, cmd, on);
    }
    if let Some(desc) = active_face_description(s) {
        c.text_in(p, FontKind::Menu, desc, 40.0, 16.0, false, b);
    }
    // The guide in its box, in place of the short messages; with the guide
    // off, or nothing to say, the game's own line. The box is as wide whoever's turn it is.
    let guide = if s.forfeit_armed { String::new() } else { s.guide() };
    let text = if guide.is_empty() { s.status.clone().unwrap_or_default() } else { guide };
    let width = guide_width(labels, c.text_width(FontKind::Menu, &thinking_line));
    let set = guide_set(&|k, t| c.text_width(k, t), &text, width);
    guide_lines(p, c, &set);
    if thinking {
        let w = c.text_width(FontKind::Menu, &thinking_line);
        crate::casino_hud::glass(p, BUTTONS_RIGHT - w, THINK_TOP + 2.0, w, 32.0);
        c.text_in(p, FontKind::Menu, &thinking_line, BUTTONS_RIGHT, THINK_TOP, true, amber(180.0));
    }
}

/// A text broken into lines of at most `width` (whole words) in a font.
fn wrap_words(width_of: &dyn Fn(FontKind, &str) -> f32, kind: FontKind, text: &str, width: f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(l) if width_of(kind, &format!("{l} {word}")) <= width => {
                l.push(' ');
                l.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

/// The guide's lines as they are set: the font, the lines apart, the lines.
#[derive(Debug)]
struct GuideSet {
    kind: FontKind,
    step: f32,
    lines: Vec<String>,
}

impl GuideSet {
    /// The last line's foot.
    fn bottom(&self) -> f32 {
        GUIDE_TOP + self.step * self.lines.len() as f32
    }
}

/// The guide box's width: from [`GUIDE_X`] to the buttons' words (`labels` their widest)
/// or the "thinking" line (`thinking` wide), whichever reaches further left.
fn guide_width(labels: f32, thinking: f32) -> f32 {
    let buttons_left = BUTTONS_RIGHT - HINT_GAP - 34.0 - labels;
    let thinking_left = BUTTONS_RIGHT - thinking;
    (buttons_left.min(thinking_left) - GUIDE_GAP - GUIDE_X).max(GUIDE_MIN_W)
}

/// The guide's `text` wrapped at `width`: in the menu's font while it fits the box, else
/// in the help box's.
fn guide_set(width_of: &dyn Fn(FontKind, &str) -> f32, text: &str, width: f32) -> GuideSet {
    let menu = GuideSet { kind: FontKind::Menu, step: GUIDE_STEP, lines: wrap_words(width_of, FontKind::Menu, text, width) };
    if menu.bottom() <= GUIDE_FLOOR {
        return menu;
    }
    GuideSet { kind: FontKind::Body, step: GUIDE_BODY_STEP, lines: wrap_words(width_of, FontKind::Body, text, width) }
}

/// The guide's lines in its box, from the top.
fn guide_lines(p: &mut ChildSpawnerCommands, c: &mut Ctx, set: &GuideSet) {
    if set.lines.is_empty() {
        return;
    }
    if set.bottom() > GUIDE_FLOOR + 1.0 {
        warn!("caravan guide: {} lines reach {:.0}, past the box's floor at {GUIDE_FLOOR:.0}", set.lines.len(), set.bottom());
    }
    let w = set.lines.iter().map(|l| c.text_width(set.kind, l)).fold(0.0, f32::max);
    crate::casino_hud::glass(p, GUIDE_X, GUIDE_TOP + 2.0, w, set.step * set.lines.len() as f32 + 2.0);
    for (i, l) in set.lines.iter().enumerate() {
        c.text_in(p, set.kind, l, GUIDE_X, GUIDE_TOP + set.step * i as f32, false, amber(255.0));
    }
}

fn results(p: &mut ChildSpawnerCommands, c: &mut Ctx, s: &Session, dim: f32) {
    sign(p, c, dim);
    let b = amber(255.0);
    crate::casino_hud::glass(p, 235.0, 235.0, 570.0, 333.0 + 45.0 * 3.0 - 235.0 + 32.0);
    c.text_in(p, FontKind::Menu, tr("Press Any Button to Continue"), 436.0, 233.0, false, b);
    let r = &s.record;
    let rows = [
        (tr("Winnings to Date:"), r.winnings.to_string()),
        (tr("Losses to Date:"), r.losses.to_string()),
        (tr("Win/Loss Totals:"), format!("{}/{}", r.won, r.lost)),
        (tr("Biggest Ante Pot Won:"), r.biggest_pot.to_string()),
    ];
    for (i, (title, value)) in rows.iter().enumerate() {
        let y = 333.0 + 45.0 * i as f32;
        c.text_in(p, FontKind::Menu, title, 235.0, y, false, b);
        c.text_in(p, FontKind::Menu, value, 805.0, y, true, b);
    }
    status(p, c, s, 10.0);
}

type Faded = (Option<&'static mut ImageNode>, Option<&'static mut BackgroundColor>, Option<&'static mut TextColor>, Option<&'static DrawnAlpha>);

/// As the menu leaves, everything it draws fades out.
pub fn fade(mut commands: Commands, screen: Res<CaravanScreen>, time: AidTime, roots: Query<Entity, With<CaravanRoot>>, children: Query<&Children>, mut nodes: Query<Faded>) {
    let Some(s) = screen.0.as_ref() else { return };
    if s.leaving.is_none() {
        return;
    }
    let f = s.fade(time.elapsed_secs());
    for root in &roots {
        for e in std::iter::once(root).chain(children.iter_descendants(root)) {
            let Ok((image, background, text, drawn)) = nodes.get_mut(e) else { continue };
            let current = image.as_ref().map(|i| i.color.alpha()).or(background.as_ref().map(|b| b.0.alpha())).or(text.as_ref().map(|t| t.0.alpha()));
            let Some(current) = current else { continue };
            let base = match drawn {
                Some(d) => d.0,
                None => {
                    commands.entity(e).insert(DrawnAlpha(current));
                    current
                }
            };
            let a = base * f;
            if let Some(mut i) = image {
                i.color.set_alpha(a);
            }
            if let Some(mut b) = background {
                b.0.set_alpha(a);
            }
            if let Some(mut t) = text {
                t.0.set_alpha(a);
            }
        }
    }
}

#[cfg(test)]
mod value_colour_tests {
    use super::*;

    #[test]
    fn sold_values_are_white_and_over_26_red() {
        let b = amber(255.0);
        assert_eq!(value_colour(20, b), b);
        assert_eq!(value_colour(21, b), SOLD);
        assert_eq!(value_colour(26, b), SOLD);
        assert_eq!(value_colour(27, b), OVER);
        assert_eq!(value_colour(0, b), b);
    }

    /// The tie note ran into the opponent's "thinking" line; the
    /// 2026-11-16 frames: with a card's tip it reached up over the hand. The guide's box,
    /// left of the buttons and the thinking line, holds the longest notes.
    #[test]
    fn the_guide_box_holds_the_tie_notes_clear_of_the_buttons_and_the_thinking_line() {
        let width = |k: FontKind, t: &str| t.chars().count() as f32 * if k == FontKind::Body { 10.5 } else { 13.0 };
        let thinking = width(FontKind::Menu, "Benny is thinking…");
        let labels = width(FontKind::Menu, "Discard Track");
        let w = guide_width(labels, thinking);
        assert!(GUIDE_X + w + GUIDE_GAP <= BUTTONS_RIGHT - thinking, "left of the thinking line");
        assert!(GUIDE_X + w + GUIDE_GAP <= BUTTONS_RIGHT - HINT_GAP - 34.0 - labels, "left of the buttons");
        let note = "NEW RENO and REDDING are tied at 24: a tie sells neither until one is higher. The game ends when all three are sold.";
        let tip = "8: adds 8 to one of your caravans. It must keep the caravan's direction or match its suit, and can't repeat its last value.";
        let two = "NEW RENO and REDDING are tied at 24; THE HUB and SHADY SANDS are tied at 22: a tie sells neither until one is higher. The game ends when all three are sold.";
        for text in [note.to_string(), format!("{note} {tip}"), format!("{two} {tip}")] {
            let set = guide_set(&width, &text, w);
            assert!(set.bottom() <= GUIDE_FLOOR, "{:?} within the box", set.lines);
            assert!(set.lines.iter().all(|l| width(set.kind, l) <= w), "{:?}", set.lines);
        }
        assert_eq!(guide_set(&width, note, w).kind, FontKind::Menu);
        // The game-over line.
        assert_eq!(guide_set(&width, "Your opponent sold two of three caravans. You lose 54 caps.", w).lines.len(), 2);
        // Wide buttons' words: the box keeps its least width.
        assert_eq!(guide_width(2000.0, thinking), GUIDE_MIN_W);
        assert!(guide_set(&width, "", w).lines.is_empty());
    }
}
