//! The casino games' score panel and end-of-match leaderboard, drawn in the HUD's look:
//! the HUD's font (font 7) and colour, lines in the menus' brackets (a top line with
//! short sides down, a bottom line with short sides up) over a dark glass fill [design:
//! match the HUD's font and color].
//!
//! Sizes are interface units (the view is 960 high), as the HUD's.

use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use fnv_util::words::{tr, trf};
use nvf_assets::GameData;

use crate::interface::{amber, bc_supported, scaled_face, u, Face};
use crate::start_menu::{text, Faces};

/// The faces, read once the game's files are there.
#[derive(Resource)]
pub struct CasinoFaces {
    pub faces: Faces,
    /// The leaderboard's title: the HUD font at 1.6 times.
    pub title: Face,
}

impl CasinoFaces {
    fn new(faces: Faces) -> CasinoFaces {
        let title = scaled_face(&faces.entry, 1.6);
        CasinoFaces { faces, title }
    }

    /// Stand-in fonts (tests, and before the game's files are read).
    pub fn plain() -> CasinoFaces {
        CasinoFaces::new(Faces::plain())
    }
}

pub struct CasinoHudPlugin;

impl Plugin for CasinoHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, make_soft).add_systems(Update, load.run_if(not(resource_exists::<CasinoFaces>)));
        if let Some(board) = std::env::var("NVF_PLAY_TEST_BOARD").ok().and_then(|s| parse_test_board(&s)) {
            info!("test aid: the leaderboard for {:?} with {} places", board.0, board.1.len());
            app.insert_resource(TestBoard(board)).add_systems(Update, draw_test_board.run_if(resource_exists::<CasinoFaces>));
        }
    }
}

/// Test aid `NVF_PLAY_TEST_BOARD=<winner>|<name>=<detail>|...`: the end-of-match
/// leaderboard over the view from the start, to look at its layout with any names.
#[derive(Resource)]
struct TestBoard((String, Vec<Place>));

#[derive(Component)]
struct TestBoardRoot;

fn parse_test_board(s: &str) -> Option<(String, Vec<Place>)> {
    let mut parts = s.split('|');
    let winner = parts.next()?.trim().to_string();
    let places: Vec<Place> = parts.map(|p| {
        let (name, detail) = p.split_once('=').unwrap_or((p, ""));
        Place { name: name.trim().into(), detail: detail.trim().into() }
    }).collect();
    (!winner.is_empty() && !places.is_empty()).then_some((winner, places))
}

fn draw_test_board(mut commands: Commands, board: Res<TestBoard>, f: Res<CasinoFaces>, old: Query<(), With<TestBoardRoot>>) {
    if !old.is_empty() {
        return;
    }
    let (winner, places) = &board.0;
    let l = board_layout(&f, &win_title(winner), places);
    info!("test aid: the leaderboard is {:.0} wide; names drawn {:?}", l.w, l.names);
    commands
        .spawn((TestBoardRoot, Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, GlobalZIndex(25)))
        .with_children(|p| leaderboard(p, &f, winner, places));
}

fn load(mut commands: Commands, data: Option<Res<GameData>>, mut images: Option<ResMut<Assets<Image>>>, formats: Option<Res<CompressedImageFormatSupport>>) {
    let (Some(data), Some(images)) = (data, images.as_deref_mut()) else { return };
    let bc = bc_supported(formats.as_deref());
    commands.insert_resource(CasinoFaces::new(Faces::load(Some(&data), images, bc)));
}

/// The menus' line width.
const LINE: f32 = 2.0;
/// The brackets' short sides.
const SIDE: f32 = 16.0;
/// The panels' inner margin.
const PAD: f32 = 14.0;
/// The HUD's distance from the view's left and top edges.
const MARGIN: f32 = 40.0;
const FILL: Color = Color::srgba(0.0, 0.0, 0.0, 0.55);

fn line_color() -> Color {
    amber(200.0)
}

fn rule(p: &mut ChildSpawnerCommands, x: f32, y: f32, w: f32, h: f32) {
    p.spawn((Node { position_type: PositionType::Absolute, left: u(x), top: u(y), width: u(w), height: u(h), ..default() }, BackgroundColor(line_color())));
}

/// A box `w` by `h` from its parent's corner: fill, and the brackets.
fn bracket_box(p: &mut ChildSpawnerCommands, w: f32, h: f32) {
    // The glass fades out toward the box's edges [design: soft edges].
    p.spawn((Node { position_type: PositionType::Absolute, left: u(0.0), top: u(0.0), width: u(w), height: u(h), ..default() }, soft_node(FILL.alpha() * 1.3)));
    rule(p, 0.0, 0.0, w, LINE);
    rule(p, 0.0, 0.0, LINE, SIDE);
    rule(p, w - LINE, 0.0, LINE, SIDE);
    rule(p, 0.0, h - LINE, w, LINE);
    rule(p, 0.0, h - SIDE, LINE, SIDE);
    rule(p, w - LINE, h - SIDE, LINE, SIDE);
}

/// The casino screens' information boxes (blackjack, roulette, slots): where the values'
/// column ends (they are right-aligned to it) and how wide the box is, for rows of (title
/// width, value width). The file's column and width while everything fits; else the
/// column moves right of the longest title and value by `gap`, and the box widens by as
/// much, keeping its margin past the column, so a long casino name and big earnings or
/// bets never run into each other or past the box [design].
pub fn info_fit(width: f32, row_x: f32, value_x: f32, gap: f32, rows: &[(f32, f32)]) -> (f32, f32) {
    let column = rows.iter().map(|&(t, v)| row_x + t + gap + v).fold(value_x, f32::max);
    (column, width + column - value_x)
}

/// One side's line on the score panel.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreRow {
    pub name: String,
    pub score: String,
    /// What it threw this turn, or other short notes.
    pub note: String,
    /// Its turn now.
    pub up: bool,
}

/// The score panel, top left at the HUD's margin. `foot` is a dimmer last line (the stake).
pub fn score_panel(p: &mut ChildSpawnerCommands, f: &CasinoFaces, rows: &[ScoreRow], foot: Option<&str>) {
    let face = &f.faces.entry;
    let small = &f.faces.small;
    let step = face.font.size * 1.05;
    let mark = face.font.width("> ");
    let name_w = rows.iter().map(|r| face.font.width(&r.name)).fold(0.0, f32::max);
    let score_w = rows.iter().map(|r| face.font.width(&r.score)).fold(0.0, f32::max);
    let note_w = rows.iter().map(|r| small.font.width(&r.note)).fold(0.0, f32::max);
    let foot_w = foot.map_or(0.0, |s| small.font.width(s));
    let gap = 24.0;
    let w = (PAD * 2.0 + mark + name_w + gap + score_w + if note_w > 0.0 { gap + note_w } else { 0.0 }).max(PAD * 2.0 + foot_w).max(220.0);
    let foot_h = if foot.is_some() { small.font.size * 1.1 } else { 0.0 };
    let h = PAD * 2.0 + step * rows.len() as f32 + foot_h;
    p.spawn(Node { position_type: PositionType::Absolute, left: u(MARGIN), top: u(MARGIN + 30.0), width: u(w), height: u(h), ..default() }).with_children(|b| {
        bracket_box(b, w, h);
        for (i, r) in rows.iter().enumerate() {
            let base = PAD + step * (i as f32 + 0.8);
            let c = if r.up { amber(255.0) } else { amber(190.0) };
            if r.up {
                text(b, face, ">", PAD, base, c);
            }
            text(b, face, &r.name, PAD + mark, base, c);
            let sx = PAD + mark + name_w + gap;
            text(b, face, &r.score, sx + score_w - face.font.width(&r.score), base, c);
            if !r.note.is_empty() {
                text(b, small, &r.note, sx + score_w + gap, base, amber(170.0));
            }
        }
        if let Some(s) = foot {
            text(b, small, s, PAD, PAD + step * rows.len() as f32 + small.font.size * 0.85, amber(170.0));
        }
    });
}

/// One place on the leaderboard.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub name: String,
    pub detail: String,
}

/// The order of the sides at a match's end: the winner first, then the rest by `key`
/// (smaller first), seat order breaking ties.
pub fn ranked(winner: usize, keys: &[i64]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..keys.len()).collect();
    order.sort_by_key(|&i| (i != winner, keys[i], i));
    order
}

const BOARD_W: f32 = 520.0;
/// The widest the leaderboard grows (the view is about 1700 wide at 16:9, 1280 at 4:3)
/// [design].
const BOARD_MAX_W: f32 = 1000.0;
const BOARD_TOP: f32 = 45.0;
/// The least room between a place's name and its detail column [design].
const BOARD_GAP: f32 = 24.0;

/// The leaderboard's width, where the names start and each name as drawn. The board is
/// 520 wide while everything fits; it widens for a long title or for a name and its
/// detail side by side, up to [`BOARD_MAX_W`]; a name still too long ends in "..." clear
/// of its detail. [observed: buckallobill, in game, FNV 1.4.0.525, pool: 'Gina "Bobcat" Griffin' ran into
/// "stripes, 1 left"] [design: the panel is NVForever's own, not the mods'].
pub struct BoardLayout {
    pub w: f32,
    pub name_x: f32,
    pub names: Vec<String>,
}

pub fn board_layout(f: &CasinoFaces, title: &str, places: &[Place]) -> BoardLayout {
    let face = &f.faces.entry;
    let small = &f.faces.small;
    let name_x = PAD * 2.0 + face.font.width("4") + 30.0;
    let row_w = |pl: &Place| name_x + face.font.width(&pl.name) + BOARD_GAP + small.font.width(&pl.detail) + PAD * 2.0;
    let w = places.iter().map(row_w).fold(BOARD_W.max(f.title.font.width(title) + PAD * 4.0), f32::max).min(BOARD_MAX_W);
    let names = places
        .iter()
        .map(|pl| {
            let room = w - PAD * 2.0 - small.font.width(&pl.detail) - BOARD_GAP - name_x;
            clip(face, &pl.name, room)
        })
        .collect();
    BoardLayout { w, name_x, names }
}

/// `s` as it fits in `room`: whole, or cut at a character with "..." after it.
fn clip(face: &Face, s: &str, room: f32) -> String {
    if face.font.width(s) <= room {
        return s.to_string();
    }
    let mut cut = s.to_string();
    while cut.pop().is_some() {
        let t = format!("{}...", cut.trim_end());
        if face.font.width(&t) <= room {
            return t;
        }
    }
    "...".to_string()
}

/// The leaderboard at the top middle of the view: "<winner> WINS" over a line, then the
/// places numbered from 1, each over a line.
pub fn leaderboard(p: &mut ChildSpawnerCommands, f: &CasinoFaces, winner: &str, places: &[Place]) {
    let title = win_title(winner);
    let face = &f.faces.entry;
    let small = &f.faces.small;
    let title_h = f.title.font.size * 1.2;
    let step = face.font.size * 1.25;
    let BoardLayout { w, name_x, names } = board_layout(f, &title, places);
    let title = clip(&f.title, &title, w - PAD * 2.0);
    let h = PAD * 2.0 + title_h + step * places.len() as f32;
    p.spawn(Node { position_type: PositionType::Absolute, left: Val::Percent(50.0), margin: UiRect::left(u(-w / 2.0)), top: u(BOARD_TOP), width: u(w), height: u(h), ..default() }).with_children(|b| {
        bracket_box(b, w, h);
        text(b, &f.title, &title, (w - f.title.font.width(&title)) / 2.0, PAD + f.title.font.size * 0.85, amber(255.0));
        rule(b, PAD, PAD + title_h, w - PAD * 2.0, LINE);
        for (i, pl) in places.iter().enumerate() {
            let top = PAD + title_h + step * i as f32;
            let base = top + step * 0.72;
            let c = if i == 0 { amber(255.0) } else { amber(200.0) };
            text(b, face, &format!("{}", i + 1), PAD * 2.0, base, c);
            text(b, face, &names[i], name_x, base, c);
            text(b, small, &pl.detail, w - PAD * 2.0 - small.font.width(&pl.detail), base, c);
            if i + 1 < places.len() {
                rule(b, PAD * 2.0, top + step - LINE, w - PAD * 4.0, 1.0);
            }
        }
    });
}

/// "<NAME> WINS"; "YOU WIN" and "<A> & <B> WIN" for this player and for teams.
pub fn win_title(winner: &str) -> String {
    if winner == "You" {
        return tr("YOU WIN").to_string();
    }
    let name = winner.to_uppercase();
    if winner.contains(" & ") { trf("{name} WIN", &[("name", &name)]) } else { trf("{name} WINS", &[("name", &name)]) }
}

/// A help line at the HUD's bottom left, in the HUD font.
pub fn help_line(p: &mut ChildSpawnerCommands, f: &CasinoFaces, lines: &str) {
    let face = &f.faces.small;
    let step = face.font.size * 1.1;
    let n = lines.lines().count().max(1) as f32;
    p.spawn(Node { position_type: PositionType::Absolute, left: u(MARGIN), bottom: u(MARGIN + 60.0), width: u(900.0), height: u(step * n + 6.0), ..default() }).with_children(|b| {
        for (i, l) in lines.lines().enumerate() {
            text(b, face, l, 0.0, step * (i as f32 + 0.85), amber(255.0));
        }
    });
}

// ---- Menu pages and banners [design: every minigame menu in the HUD's look] ----

/// The menu card's place: the HUD's left margin, under the score panel's line.
const CARD_LEFT: f32 = 60.0;
const CARD_TOP: f32 = 110.0;
const CARD_MIN_W: f32 = 440.0;
/// The widest a card grows (its foot always wraps at the card's width).
const CARD_MAX_W: f32 = 700.0;
const KEY_GAP: f32 = 14.0;

/// What a card shows: its title, a line under the title, the choices, the foot.
#[derive(Debug, Clone, Copy)]
pub struct CardText<'a> {
    pub title: &'a str,
    pub sub: &'a str,
    pub rows: &'a [MenuRow],
    pub foot: &'a str,
}

/// The card's size and where each part goes, interface units.
#[derive(Debug, Clone, PartialEq)]
pub struct CardLayout {
    pub w: f32,
    pub h: f32,
    pub key_w: f32,
    pub title_base: f32,
    pub sub_base: f32,
    pub rule_y: f32,
    pub row_bases: Vec<f32>,
    pub foot_rule: f32,
    pub foot_lines: Vec<String>,
    pub foot_base: f32,
}

fn key_text(key: &str) -> String {
    format!("[{key}]")
}

/// Words onto lines no wider than `width` (a word wider than that has a line to itself).
fn wrap(face: &Face, s: &str, width: f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in s.split_whitespace() {
        match lines.last_mut() {
            Some(l) if face.font.width(&format!("{l} {word}")) <= width => {
                l.push(' ');
                l.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

/// Where everything on a menu card goes.
pub fn card_layout(f: &CasinoFaces, c: CardText) -> CardLayout {
    let (title, entry, small) = (&f.title, &f.faces.entry, &f.faces.small);
    let key_w = c.rows.iter().map(|r| entry.font.width(&key_text(&r.key))).fold(0.0, f32::max);
    let row_w = |r: &MenuRow| key_w + KEY_GAP + entry.font.width(&r.label) + if r.note.is_empty() { 0.0 } else { KEY_GAP + small.font.width(&r.note) };
    let inner = c.rows.iter().map(row_w).fold(title.font.width(c.title).max(small.font.width(c.sub)), f32::max);
    let w = (inner + PAD * 2.0).clamp(CARD_MIN_W, CARD_MAX_W);
    let mut y = PAD;
    let title_base = y + title.font.size * 0.85;
    y += title.font.size * 1.05;
    let sub_base = y + small.font.size * 0.8;
    if !c.sub.is_empty() {
        y += small.font.size * 1.1;
    }
    let rule_y = y + 4.0;
    y = rule_y + LINE + 10.0;
    let step = entry.font.size * 1.2;
    let row_bases = (0..c.rows.len()).map(|i| y + step * (i as f32 + 0.8)).collect();
    y += step * c.rows.len() as f32;
    let foot_lines = if c.foot.is_empty() { Vec::new() } else { wrap(small, c.foot, w - PAD * 2.0) };
    let foot_rule = y + 6.0;
    let foot_base = foot_rule + 10.0 + small.font.size * 0.8;
    if !foot_lines.is_empty() {
        y = foot_rule + 10.0 + small.font.size * 1.1 * foot_lines.len() as f32;
    }
    CardLayout { w, h: y + PAD, key_w, title_base, sub_base, rule_y, row_bases, foot_rule, foot_lines, foot_base }
}

/// A minigame's menu page: one card at the HUD's left margin in the HUD's look (the
/// bracketed dark glass of the score panel, the HUD font and colour). The title in the
/// leaderboard's large face, a short line under it, the choices as the HUD writes its
/// prompts ("1) Challenge"; greyed ones dimmed, with their note), the foot in small print
/// [design: every minigame menu in line with the other UI changes].
pub fn menu_card(p: &mut ChildSpawnerCommands, f: &CasinoFaces, c: CardText) {
    let l = card_layout(f, c);
    let (entry, small) = (&f.faces.entry, &f.faces.small);
    p.spawn(Node { position_type: PositionType::Absolute, left: u(CARD_LEFT), top: u(CARD_TOP), width: u(l.w), height: u(l.h), ..default() }).with_children(|b| {
        bracket_box(b, l.w, l.h);
        text(b, &f.title, c.title, PAD, l.title_base, amber(255.0));
        if !c.sub.is_empty() {
            text(b, small, c.sub, PAD, l.sub_base, amber(200.0));
        }
        rule(b, PAD, l.rule_y, l.w - PAD * 2.0, LINE);
        for (r, base) in c.rows.iter().zip(&l.row_bases) {
            let (key_c, label_c, note_c) = if r.on { (amber(255.0), amber(225.0), amber(165.0)) } else { (amber(105.0), amber(105.0), amber(130.0)) };
            let k = key_text(&r.key);
            text(b, entry, &k, PAD + l.key_w - entry.font.width(&k), *base, key_c);
            let x = PAD + l.key_w + KEY_GAP;
            text(b, entry, &r.label, x, *base, label_c);
            if !r.note.is_empty() {
                text(b, small, &r.note, x + entry.font.width(&r.label) + KEY_GAP, *base, note_c);
            }
        }
        if !l.foot_lines.is_empty() {
            rule(b, PAD, l.foot_rule, l.w - PAD * 2.0, 1.0);
            for (i, line) in l.foot_lines.iter().enumerate() {
                text(b, small, line, PAD, l.foot_base + small.font.size * 1.1 * i as f32, amber(175.0));
            }
        }
    });
}

/// The soft backdrop's picture: dark, fading out over its edges (made here, no file).
const SOFT: Handle<Image> = bevy::asset::uuid_handle!("5d2b7e41-93c6-4a1f-b8e0-6c4f2a9d1e37");
/// The picture's side, and how far in from its edges it fades, in pixels.
const SOFT_SIDE: u32 = 66;
const SOFT_EDGE: f32 = 32.0;
/// How far past the text the fade reaches, interface units.
const SOFT_X: f32 = 44.0;
const SOFT_Y: f32 = 20.0;
const SOFT_ALPHA: f32 = 0.62;

fn soft_image() -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let n = SOFT_SIDE;
    let ramp = |i: u32| {
        let t = ((i.min(n - 1 - i)) as f32 / SOFT_EDGE).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let mut px = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            px.extend_from_slice(&[0, 0, 0, (ramp(x) * ramp(y) * 255.0) as u8]);
        }
    }
    Image::new(Extent3d { width: n, height: n, depth_or_array_layers: 1 }, TextureDimension::D2, px, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default())
}

fn make_soft(mut images: ResMut<Assets<Image>>) {
    let _ = images.insert(SOFT.id(), soft_image());
}

/// A soft dark backdrop under text `w` by `h` at (x, y) from the parent's corner: no box
/// and no lines, it fades out past the text's edges, so a table's words read on a busy
/// picture [design: backdrops have soft edges].
pub fn glass(p: &mut ChildSpawnerCommands, x: f32, y: f32, w: f32, h: f32) {
    p.spawn((Node { position_type: PositionType::Absolute, left: u(x - SOFT_X), top: u(y - SOFT_Y), width: u(w + SOFT_X * 2.0), height: u(h + SOFT_Y * 2.0), ..default() }, soft_node(SOFT_ALPHA)));
}

/// The soft picture, its edges kept their width whatever the node's size.
fn soft_node(alpha: f32) -> ImageNode {
    let slicer = TextureSlicer { border: BorderRect::all(SOFT_EDGE), center_scale_mode: SliceScaleMode::Stretch, sides_scale_mode: SliceScaleMode::Stretch, max_corner_scale: 1.0 };
    ImageNode { color: Color::srgba(1.0, 1.0, 1.0, alpha.min(1.0)), image_mode: NodeImageMode::Sliced(slicer), ..ImageNode::new(SOFT) }
}

/// How far up from the view's bottom a banner sits (above the HUD's bars).
const BANNER_BOTTOM: f32 = 230.0;
/// The widest a banner grows before its lines wrap.
const BANNER_MAX_W: f32 = 900.0;

/// A notice as a banner: centred low in the view, the bracketed dark glass behind it, in
/// the HUD font; long lines wrap, several stack. For the games' notices ("Your throw", a
/// hand's result, whose turn) [design].
pub fn banner(p: &mut ChildSpawnerCommands, f: &CasinoFaces, lines: &str) {
    let face = &f.faces.entry;
    let lines: Vec<String> = lines.lines().filter(|l| !l.trim().is_empty()).flat_map(|l| wrap(face, l, BANNER_MAX_W)).collect();
    if lines.is_empty() {
        return;
    }
    let step = face.font.size * 1.15;
    let w = lines.iter().map(|l| face.font.width(l)).fold(0.0, f32::max) + PAD * 4.0;
    let h = step * lines.len() as f32 + PAD * 1.4;
    p.spawn(Node { position_type: PositionType::Absolute, left: Val::Percent(50.0), margin: UiRect::left(u(-w / 2.0)), bottom: u(BANNER_BOTTOM), width: u(w), height: u(h), ..default() }).with_children(|b| {
        bracket_box(b, w, h);
        for (i, l) in lines.iter().enumerate() {
            text(b, face, l, (w - face.font.width(l)) / 2.0, PAD * 0.7 + step * (i as f32 + 0.8), amber(255.0));
        }
    });
}

/// One line in the HUD font at (x, base) interface units from the parent's corner: `size`
/// 0 the small face, 1 the HUD's, 2 the large title.
pub fn line(p: &mut ChildSpawnerCommands, f: &CasinoFaces, size: u8, s: &str, x: f32, base: f32, color: Color) {
    text(p, face_of(f, size), s, x, base, color);
}

/// The width of a line in one of [`line`]'s faces.
pub fn width(f: &CasinoFaces, size: u8, s: &str) -> f32 {
    face_of(f, size).font.width(s)
}

fn face_of(f: &CasinoFaces, size: u8) -> &Face {
    match size {
        0 => &f.faces.small,
        1 => &f.faces.entry,
        _ => &f.title,
    }
}

/// One line of a menu card: the key, what it does, a note after it, and whether it can be
/// picked now (a greyed line says why not in its note).
#[derive(Debug, Clone, PartialEq)]
pub struct MenuRow {
    pub key: String,
    pub label: String,
    pub note: String,
    pub on: bool,
}

/// A line that can be picked.
pub fn row(key: &str, label: impl Into<String>) -> MenuRow {
    MenuRow { key: key.into(), label: label.into(), note: String::new(), on: true }
}

impl MenuRow {
    /// With a note after the label.
    pub fn note(mut self, note: impl Into<String>) -> MenuRow {
        self.note = note.into();
        self
    }

    /// Greyed when `on` is false.
    pub fn enabled(mut self, on: bool) -> MenuRow {
        self.on = on;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_winner_leads_then_the_lowest_left() {
        // 301 counts down: the winner at 0, then 40 left before 100 left.
        assert_eq!(ranked(2, &[100, 40, 0]), vec![2, 1, 0]);
        // Ties keep seat order.
        assert_eq!(ranked(1, &[100, 0, 100, 100]), vec![1, 0, 2, 3]);
    }

    #[test]
    fn big_numbers_widen_the_box_and_keep_clear_of_titles() {
        // Short rows: the file's column and width.
        assert_eq!(info_fit(400.0, 20.0, 340.0, 16.0, &[(120.0, 40.0), (200.0, 60.0)]), (340.0, 400.0));
        // "Won at Atomic Wrangler:" and a seven-digit sum.
        let (column, w) = info_fit(400.0, 20.0, 340.0, 16.0, &[(120.0, 40.0), (300.0, 140.0)]);
        assert_eq!(column, 20.0 + 300.0 + 16.0 + 140.0);
        assert_eq!(w - column, 60.0, "the margin past the values stays");
        // Every row's value starts after its title.
        for (t, v) in [(120.0f32, 40.0f32), (300.0, 140.0)] {
            assert!(column - v >= 20.0 + t + 16.0);
        }
    }

    #[test]
    fn a_card_grows_to_its_longest_line_and_wraps_its_foot() {
        let f = CasinoFaces::plain();
        let rows = [row("1", "Challenge"), row("2", "Practice"), row("Space", "Start").enabled(false).note("needs caps")];
        let short = card_layout(&f, CardText { title: "Darts", sub: "", rows: &rows, foot: "" });
        assert_eq!(short.row_bases.len(), 3);
        assert!(short.row_bases.windows(2).all(|w| w[1] > w[0]));
        assert!(short.w >= CARD_MIN_W);
        let foot = "Three rounds of 12, 18 and 24 stabs. Press the key shown at the knife. Three mistakes lose; each costs 10 health.";
        let long = card_layout(&f, CardText { title: "Five Finger Fillet", sub: "", rows: &rows, foot });
        assert!(long.foot_lines.len() > 1, "the foot wraps");
        assert!(long.w <= CARD_MAX_W);
        assert!(long.h > long.foot_base);
        assert!(long.foot_lines[0].contains(' '), "the words keep their spaces");
        for l in &long.foot_lines {
            assert!(f.faces.small.font.width(l) <= long.w - PAD * 2.0 + 1.0, "{l:?} fits");
        }
    }

    #[test]
    fn a_long_name_never_runs_into_its_detail() {
        let f = CasinoFaces::plain();
        let (face, small) = (&f.faces.entry, &f.faces.small);
        let place = |name: &str, detail: &str| Place { name: name.into(), detail: detail.into() };
        let clear = |l: &BoardLayout, places: &[Place]| {
            for (n, pl) in l.names.iter().zip(places) {
                let detail_x = l.w - PAD * 2.0 - small.font.width(&pl.detail);
                assert!(l.name_x + face.font.width(n) + BOARD_GAP <= detail_x + 0.01, "{n:?} clear of {:?}", pl.detail);
            }
        };
        // Short names: the board keeps its width and the names as they are.
        let short = [place("You", "solids, 0 left"), place("Sunny", "stripes, 1 left")];
        let l = board_layout(&f, "YOU WIN", &short);
        assert_eq!(l.w, BOARD_W);
        assert_eq!(l.names, ["You", "Sunny"]);
        // A longer name still: the board widens and the whole name shows.
        let gina = [place("You", "solids, 0 left"), place("Gina \"Bobcat\" Griffin the Third", "stripes, 1 left")];
        let l = board_layout(&f, "YOU WIN", &gina);
        assert!(l.w > BOARD_W && l.w <= BOARD_MAX_W);
        assert_eq!(l.names[1], gina[1].name);
        clear(&l, &gina);
        // Past the widest board: cut, with "...", still clear.
        let huge = [place("You", "solids, 0 left"), place(&"Bobcat ".repeat(60), "stripes, 1 left")];
        let l = board_layout(&f, "YOU WIN", &huge);
        assert_eq!(l.w, BOARD_MAX_W);
        assert!(l.names[1].ends_with("...") && l.names[1].starts_with("Bobcat"));
        clear(&l, &huge);
    }

    #[test]
    fn the_test_board_reads_its_places() {
        let (w, p) = parse_test_board("You|You=solids, 0 left|Gina \"Bobcat\" Griffin=stripes, 1 left").unwrap();
        assert_eq!(w, "You");
        assert_eq!(p[1], Place { name: "Gina \"Bobcat\" Griffin".into(), detail: "stripes, 1 left".into() });
        assert!(parse_test_board("You").is_none());
    }

    #[test]
    fn the_title_agrees_with_its_winner() {
        assert_eq!(win_title("Guest"), "GUEST WINS");
        assert_eq!(win_title("You"), "YOU WIN");
        assert_eq!(win_title("You & Third"), "YOU & THIRD WIN");
    }
}
