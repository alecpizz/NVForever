//! The poker table (`nvf_play::poker`, docs/design/poker.md §4): an oval card table made
//! here and textured with the game's own pictures (§5), the cards and chips on it, drawn by
//! a camera of its own into a picture the menu shows under its text
//! ([`nvf_play::poker::PokerView`]), as the lockpicking lock is (`lockpick_view.rs`).
//!
//! Everything is in game space (Z up), under a holder far from the world; lengths are game
//! units [design: sized as a real table, about 2.3 × 1.2, at one unit a centimetre's
//! worth — the scene is alone, so only its proportions matter].
//!
//! At a poker table in the world (`Session::table_ref`) the oval
//! lies on that table instead: scaled to its length, turned so the player's seat is on the
//! player's side, on its top; the world's table stays as its base. It is drawn with the
//! world, seen by the game's own camera (`roulette_view::TableCamera`), a lamp over it;
//! the stools and people at the table are hidden meanwhile, as at roulette [design].

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{AmbientLight, NotShadowCaster};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::window::PrimaryWindow;
use fnv_game::poker::{chips_for, Card, Street};
use nvf_assets::model::ModelCtx;
use nvf_assets::{nif_material, NifMaterial};
use nvf_play::pause::AidTime;
use nvf_play::poker::{Casino, PokerScreen, PokerView, Session, PLAYER};
use std::collections::HashMap;
use std::f32::consts::{PI, TAU};

/// The render layer of the table, its lights and its camera.
pub const POKER_LAYER: usize = 11;
/// Where the table is put, far from the world (Bevy space) [design].
const PLACE: Vec3 = Vec3::new(0.0, -300_000.0, 0.0);

/// The oval: half the straight sides' length, the ends' radius, the rail's width.
const HALF: f32 = 40.0;
const RADIUS: f32 = 48.0;
const RAIL: f32 = 11.0;
/// The floor under the table's top.
const FLOOR: f32 = -76.0;
/// The camera: where it is and what it looks at; the vertical field of view [design: the
/// mock-up's, drawn back a little so the ends' seats are in view].
const EYE: Vec3 = Vec3::new(0.0, -108.0, 70.0);
const AIM: Vec3 = Vec3::new(0.0, -3.0, 0.0);
const FOV_DEG: f32 = 38.0;
const CARD: Vec2 = Vec2::new(6.3, 8.8);
/// The lamp over the table: its brightness (what it adds to a white surface facing it, at
/// no distance), its reach; the ambient light [design: fitted to the mock-up by eye].
const LAMP: f32 = 1.25;
const LAMP_RANGE: f32 = 260.0;
const AMBIENT: f32 = 0.12;
const CHIP_R: f32 = 2.0;
const CHIP_T: f32 = 0.34;
/// Chips in one stack at most, and the room between stacks.
const STACK_MAX: u32 = 20;
const STACK_GAP: f32 = 4.4;
/// The board's cards: the first one's place and the room between them.
const BOARD: Vec2 = Vec2::new(-14.8, 5.0);
const BOARD_GAP: f32 = 7.4;
/// Where cards are dealt from (the far side, as a dealer would) and how long they slide.
const DECK: Vec3 = Vec3::new(0.0, 36.0, 0.3);
const DEAL_SECONDS: f32 = 0.3;

const FELT_TILE: f32 = 150.0;
const RAIL_TEXTURE: &str = "textures\\terminals\\nv_blackjack\\nv_blackjack-table01.dds";
const CHIPS_TEXTURE: &str = "textures\\terminals\\nv_chips\\nv_minigame-chips.dds";
const BUTTON_TEXTURE: &str = "textures\\clutter\\platinumchip\\platinumchip.dds";
const FLOOR_TEXTURE: &str = "textures\\clutter\\rugs\\nvtopscasinorug_d.dds";
/// The rail picture's bands, as fractions of its height, and the part across its width
/// that has no chip tray [observed: the picture, docs/design/poker.md §5].
const RAIL_PAD: (f32, f32) = (185.0 / 512.0, 345.0 / 512.0);
/// The trim band starts under the strip's "IRG CARDS" badges [observed: the picture, rows
/// to about 60; drawn on the band they were coloured specks].
const RAIL_TRIM: (f32, f32) = (64.0 / 512.0, 116.0 / 512.0);
const RAIL_U: f32 = 1400.0 / 2048.0;
/// The chips picture: six columns; a chip's face in the top 84 of 256, its stripes below
/// [observed: the picture].
const CHIP_FACE_V: (f32, f32) = (1.0 / 256.0, 83.0 / 256.0);
const CHIP_SIDE_V: f32 = 100.0 / 256.0;
const CHIP_SIDE_H: f32 = 18.0 / 256.0;

/// A seat's place: its cards' middle and their turn (degrees about Z) [design: as the
/// mock-up]. Seat 0 is the player's, nearest the camera; then round to the left.
const SEATS: [(Vec2, f32); 5] = [(Vec2::new(0.0, -35.0), 0.0), (Vec2::new(-56.0, 0.0), 80.0), (Vec2::new(-26.0, 30.0), 174.0), (Vec2::new(26.0, 30.0), 186.0), (Vec2::new(56.0, 0.0), -80.0)];

pub struct PokerViewPlugin;

impl Plugin for PokerViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, show.after(nvf_play::PlaySet::Ui).before(nvf_play::PlaySet::Sync));
    }
}

/// Positions, normals, texture coordinates and triangles of a mesh being made.
#[derive(Default)]
struct Geo {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    idx: Vec<u32>,
}

impl Geo {
    fn vertex(&mut self, p: Vec3, n: Vec3, uv: Vec2) -> u32 {
        self.pos.push(p.to_array());
        self.nrm.push(n.normalize_or_zero().to_array());
        self.uv.push(uv.to_array());
        self.pos.len() as u32 - 1
    }

    /// A quad of four vertices given round its edge.
    fn quad(&mut self, v: [(Vec3, Vec3, Vec2); 4]) {
        let i: Vec<u32> = v.iter().map(|(p, n, uv)| self.vertex(*p, *n, *uv)).collect();
        self.idx.extend([i[0], i[1], i[2], i[0], i[2], i[3]]);
    }

    fn mesh(self) -> Mesh {
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.pos);
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm);
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uv);
        m.insert_indices(Indices::U32(self.idx));
        m
    }
}

/// The oval's edge: `n` points round it (from the top straight's right end, against the
/// clock seen from above), each with its outward direction and its distance along the
/// edge.
fn oval(n: usize, out: f32) -> Vec<(Vec2, Vec2, f32)> {
    let r = RADIUS + out;
    let length = 4.0 * HALF + TAU * r;
    (0..=n)
        .map(|i| {
            let s = length * i as f32 / n as f32;
            let (p, d) = if s < 2.0 * HALF {
                (Vec2::new(HALF - s, r), Vec2::Y)
            } else if s < 2.0 * HALF + PI * r {
                let a = PI / 2.0 + (s - 2.0 * HALF) / r;
                (Vec2::new(-HALF + r * a.cos(), r * a.sin()), Vec2::new(a.cos(), a.sin()))
            } else if s < 4.0 * HALF + PI * r {
                (Vec2::new(-HALF + (s - 2.0 * HALF - PI * r), -r), -Vec2::Y)
            } else {
                let a = -PI / 2.0 + (s - 4.0 * HALF - PI * r) / r;
                (Vec2::new(HALF + r * a.cos(), r * a.sin()), Vec2::new(a.cos(), a.sin()))
            };
            (p, d, s)
        })
        .collect()
}

/// A ring of the oval `out` outside its edge (negative: inside), `width` wide, at `z`.
fn ring(out: f32, width: f32, z: f32) -> Geo {
    let mut g = Geo::default();
    let edge = oval(160, out);
    for w in edge.windows(2) {
        let (a, b) = (w[0], w[1]);
        let h = width / 2.0;
        let p = |e: (Vec2, Vec2, f32), k: f32| (e.0 + e.1 * k).extend(z);
        g.quad([(p(a, -h), Vec3::Z, Vec2::ZERO), (p(b, -h), Vec3::Z, Vec2::ZERO), (p(b, h), Vec3::Z, Vec2::ZERO), (p(a, h), Vec3::Z, Vec2::ZERO)]);
    }
    g
}

fn felt_mesh() -> Mesh {
    let mut g = Geo::default();
    let uv = |p: Vec2| Vec2::new(p.x / FELT_TILE, -p.y / FELT_TILE);
    let c = g.vertex(Vec3::ZERO, Vec3::Z, Vec2::ZERO);
    let edge = oval(160, 0.0);
    for w in edge.windows(2) {
        let a = g.vertex(w[0].0.extend(0.0), Vec3::Z, uv(w[0].0));
        let b = g.vertex(w[1].0.extend(0.0), Vec3::Z, uv(w[1].0));
        g.idx.extend([c, a, b]);
    }
    g.mesh()
}

/// The rail: a padded cushion over the felt's edge and a trim falling to the apron, swept
/// round the oval. Its picture repeats along the rail, each repeat its own quads so the
/// chip tray at the picture's right end is never used.
fn rail_mesh() -> Mesh {
    // The profile across the rail: (out from the edge, height, the picture's v).
    // It starts a little under the felt, so no slit opens between the felt's edge and the
    // cushion's lip.
    let mut profile: Vec<(f32, f32, f32)> = vec![(0.0, -0.5, RAIL_PAD.0)];
    // The cushion rolls from the felt over the rail's outer edge, and the wooden trim band
    // hangs straight down from under its lip: no dip between them (a dip there read as a
    // gap through to the floor).
    let pad = 16;
    for i in 0..=pad {
        let a = PI * i as f32 / pad as f32;
        let (out, h) = (RAIL / 2.0 * (1.0 - a.cos()), 1.0 + 4.0 * a.sin().powf(0.7));
        profile.push((out, h, RAIL_PAD.0 + (RAIL_PAD.1 - RAIL_PAD.0) * i as f32 / pad as f32));
    }
    // The band starts its own picture where the cushion ends (a zero-height step).
    profile.push((RAIL, 1.0, RAIL_TRIM.0));
    profile.push((RAIL, UNDER, RAIL_TRIM.1));
    let mut g = Geo::default();
    let edge = oval(192, 0.0);
    let length = edge.last().map_or(1.0, |e| e.2);
    let repeats = (length / 60.0).round().max(1.0);
    let tile = length / repeats;
    for w in edge.windows(2) {
        let (a, b) = (w[0], w[1]);
        // The picture's u along this piece, inside one repeat.
        let start = (a.2 / tile).floor();
        let (ua, ub) = ((a.2 / tile - start) * RAIL_U, (b.2 / tile - start) * RAIL_U);
        for k in profile.windows(2) {
            let (p0, p1) = (k[0], k[1]);
            let across = Vec2::new(p1.0 - p0.0, p1.1 - p0.1);
            // A step that only changes the picture has no surface (and no normal: lit, it
            // speckles).
            if across.length() < 1e-3 {
                continue;
            }
            // The profile's normal (outward and up, in the plane across the rail).
            let nrm = |e: (Vec2, Vec2, f32)| (e.1 * -across.y).extend(across.x);
            let at = |e: (Vec2, Vec2, f32), q: (f32, f32, f32)| (e.0 + e.1 * q.0).extend(q.1);
            g.quad([
                (at(a, p0), nrm(a), Vec2::new(ua, p0.2)),
                (at(a, p1), nrm(a), Vec2::new(ua, p1.2)),
                (at(b, p1), nrm(b), Vec2::new(ub, p1.2)),
                (at(b, p0), nrm(b), Vec2::new(ub, p0.2)),
            ]);
        }
    }
    g.mesh()
}

/// The table's underside: the oval out to the rail's edge, at the trim's foot, facing
/// down, so the top is closed seen from below.
const UNDER: f32 = -8.0;
fn under_mesh() -> Mesh {
    let mut g = Geo::default();
    // One plain spot of the padding: the whole picture would put the chip tray under there.
    let uv = |_: Vec2| Vec2::new(0.25, (RAIL_PAD.0 + RAIL_PAD.1) / 2.0);
    let c = g.vertex(Vec3::new(0.0, 0.0, UNDER), -Vec3::Z, Vec2::ZERO);
    // A little inside the band, so its edge never shows past it.
    let edge = oval(160, RAIL - 0.5);
    for w in edge.windows(2) {
        let a = g.vertex(w[0].0.extend(UNDER), -Vec3::Z, uv(w[0].0));
        let b = g.vertex(w[1].0.extend(UNDER), -Vec3::Z, uv(w[1].0));
        g.idx.extend([c, b, a]);
    }
    g.mesh()
}

fn quad_mesh(size: Vec2, uv_scale: Vec2) -> Mesh {
    let mut g = Geo::default();
    let (w, h) = (size.x / 2.0, size.y / 2.0);
    g.quad([
        (Vec3::new(-w, -h, 0.0), Vec3::Z, Vec2::new(0.0, uv_scale.y)),
        (Vec3::new(w, -h, 0.0), Vec3::Z, Vec2::new(uv_scale.x, uv_scale.y)),
        (Vec3::new(w, h, 0.0), Vec3::Z, Vec2::new(uv_scale.x, 0.0)),
        (Vec3::new(-w, h, 0.0), Vec3::Z, Vec2::ZERO),
    ]);
    g.mesh()
}

/// A card: its face up (+Z), its back down; a hair apart. Each is the card's rounded
/// outline (`crate::card_shape`): the pictures' own corners are opaque.
fn card_meshes() -> (Mesh, Mesh) {
    (crate::card_shape::card_mesh(CARD, 0.02, true), crate::card_shape::card_mesh(CARD, 0.0, false))
}

/// A stack of `count` chips of one colour (the picture's `column`), each a little off the
/// one under it (by `seed`).
fn stack_mesh(column: usize, count: u32, seed: u32) -> Mesh {
    let mut g = Geo::default();
    let segs = 24;
    let col = column as f32 / 6.0;
    let mut r = seed.wrapping_mul(2_654_435_761).max(1);
    let mut rnd = || {
        r ^= r << 13;
        r ^= r >> 17;
        r ^= r << 5;
        (r as f32 / u32::MAX as f32) - 0.5
    };
    for i in 0..count {
        let (z0, z1) = (i as f32 * CHIP_T, (i + 1) as f32 * CHIP_T);
        let off = Vec2::new(rnd(), rnd()) * 0.25;
        let turn = rnd() * TAU;
        let v0 = CHIP_SIDE_V + ((i * 9) % 120) as f32 / 256.0;
        for k in 0..segs {
            let (a0, a1) = (k as f32 / segs as f32 * TAU, (k + 1) as f32 / segs as f32 * TAU);
            let d0 = Vec2::from_angle(a0 + turn);
            let d1 = Vec2::from_angle(a1 + turn);
            // Three repeats of the stripes round a chip.
            let per = segs / 3;
            let (u0, u1) = (col + ((k % per) as f32 / per as f32) / 6.0, col + (((k % per) + 1) as f32 / per as f32) / 6.0);
            let p = |d: Vec2, z: f32| (off + d * CHIP_R).extend(z);
            g.quad([
                (p(d0, z0), d0.extend(0.0), Vec2::new(u0, v0 + CHIP_SIDE_H)),
                (p(d1, z0), d1.extend(0.0), Vec2::new(u1, v0 + CHIP_SIDE_H)),
                (p(d1, z1), d1.extend(0.0), Vec2::new(u1, v0)),
                (p(d0, z1), d0.extend(0.0), Vec2::new(u0, v0)),
            ]);
        }
        // The top face (only the top chip's shows).
        let centre = Vec2::new(col + 1.0 / 12.0, (CHIP_FACE_V.0 + CHIP_FACE_V.1) / 2.0);
        let half = Vec2::new(1.0 / 12.0 - 1.0 / 512.0, (CHIP_FACE_V.1 - CHIP_FACE_V.0) / 2.0);
        let c = g.vertex(off.extend(z1), Vec3::Z, centre);
        for k in 0..segs {
            let (a0, a1) = (k as f32 / segs as f32 * TAU, (k + 1) as f32 / segs as f32 * TAU);
            let uv = |a: f32| centre + Vec2::new(a.cos(), -a.sin()) * half;
            let a = g.vertex((off + Vec2::from_angle(a0 + turn) * CHIP_R).extend(z1), Vec3::Z, uv(a0));
            let b = g.vertex((off + Vec2::from_angle(a1 + turn) * CHIP_R).extend(z1), Vec3::Z, uv(a1));
            g.idx.extend([c, a, b]);
        }
    }
    g.mesh()
}

/// A card as the table shows it: where, its turn, face up, when it starts sliding in.
#[derive(Clone, Debug, PartialEq)]
struct CardPlace {
    key: String,
    card: Card,
    at: Vec2,
    turn_deg: f32,
    face_up: bool,
    since: f32,
}

/// A stack of chips: the picture's column, how many, where.
#[derive(Clone, Copy, Debug, PartialEq)]
struct StackPlace {
    column: usize,
    count: u32,
    at: Vec2,
}

/// Stacks for `amount` from `start`, one beside the other along `along`.
fn stacks(amount: u32, start: Vec2, along: Vec2, out: &mut Vec<StackPlace>) {
    let mut i = 0.0;
    for (column, mut n) in chips_for(amount) {
        while n > 0 {
            let count = n.min(STACK_MAX);
            out.push(StackPlace { column, count, at: start + along * (i * STACK_GAP) });
            n -= count;
            i += 1.0;
        }
    }
}

/// Stacks for the pot, in rows round the middle.
fn pot_stacks(amount: u32, out: &mut Vec<StackPlace>) {
    let mut i = 0;
    for (column, mut n) in chips_for(amount) {
        while n > 0 {
            let count = n.min(STACK_MAX / 2);
            let (row, col) = (i / 5, i % 5);
            let x = (col as f32 - 2.0) * STACK_GAP + if row % 2 == 1 { STACK_GAP / 2.0 } else { 0.0 };
            out.push(StackPlace { column, count, at: Vec2::new(x, -8.0 - row as f32 * STACK_GAP * 0.9) });
            n -= count;
            i += 1;
        }
    }
}

/// The seat's direction along its edge of the table, and out from the middle.
fn seat_axes(i: usize) -> (Vec2, Vec2) {
    let a = SEATS[i].1.to_radians();
    let along = Vec2::new(a.cos(), a.sin());
    (along, Vec2::new(along.y, -along.x))
}

/// Where a seat's bet goes: between it and the middle.
fn bet_spot(i: usize) -> Vec2 {
    if i == PLAYER {
        Vec2::new(0.5, -24.0)
    } else {
        SEATS[i].0 * 0.78
    }
}

fn layout(s: &Session) -> (Vec<CardPlace>, Vec<StackPlace>, Option<Vec2>, usize) {
    let t = &s.table;
    let mut cards = Vec::new();
    let mut chips = Vec::new();
    let over = t.street == Street::Over;
    for (i, seat) in t.seats.iter().enumerate() {
        if seat.out || seat.hole.is_empty() {
            continue;
        }
        let shown_down = over && t.showdown && seat.in_hand();
        if seat.folded {
            continue;
        }
        let (at, turn) = SEATS[i];
        let (along, _) = seat_axes(i);
        for (k, c) in seat.hole.iter().enumerate() {
            let side = if k == 0 { -1.0 } else { 1.0 };
            let (spot, tilt) = if i == PLAYER { (at + Vec2::new(3.5 * side, 0.25 * side), -6.5 * side) } else { (at + along * (2.6 * side), turn - 5.5 * side) };
            cards.push(CardPlace { key: format!("h{i}{k}"), card: *c, at: spot, turn_deg: tilt, face_up: i == PLAYER || shown_down, since: s.card_time(i, k) });
        }
    }
    for (j, c) in t.community.iter().enumerate() {
        cards.push(CardPlace { key: format!("b{j}"), card: *c, at: BOARD + Vec2::new(BOARD_GAP * j as f32, 0.0), turn_deg: ((j * 37 % 7) as f32 - 3.0) * 0.4, face_up: true, since: s.board_time(j) });
    }
    // Chips: each seat's stack and bet; the pot (what earlier streets put in); a won pot
    // at its winner's bet place until the next deal.
    let won: HashMap<usize, u32> = if over { t.winners.iter().fold(HashMap::new(), |mut m, w| {
        *m.entry(w.0).or_insert(0) += w.1;
        m
    }) } else { HashMap::new() };
    for (i, seat) in t.seats.iter().enumerate() {
        if seat.out {
            continue;
        }
        let (along, outward) = seat_axes(i);
        let bank = seat.stack.saturating_sub(won.get(&i).copied().unwrap_or(0));
        if i == PLAYER {
            stacks(bank, Vec2::new(13.0, -38.0), Vec2::new(1.0, 0.18), &mut chips);
        } else {
            let start = SEATS[i].0 + outward * 8.0 - along * 4.6;
            stacks(bank, start, along, &mut chips);
        }
        let bet = if over { won.get(&i).copied().unwrap_or(0) } else { seat.bet };
        stacks(bet, bet_spot(i), Vec2::new(1.0, 0.0), &mut chips);
    }
    if !over {
        let pot: u32 = t.seats.iter().map(|x| x.put_in - x.bet).sum();
        pot_stacks(pot, &mut chips);
    }
    // The dealer button beside the button's seat, toward the middle.
    let button = (!t.seats[t.button].out && t.hand > 0).then(|| {
        let (along, _) = seat_axes(t.button);
        bet_spot(t.button) * 0.95 + along * if t.button == PLAYER { -14.0 } else { 7.0 }
    });
    (cards, chips, button, 5usize.saturating_sub(t.community.len()))
}

/// The label anchor of a seat (game space): above its far edge.
fn anchor(i: usize) -> Vec3 {
    let (p, _) = SEATS[i];
    if p.y > 0.0 {
        Vec3::new(p.x * 1.12, p.y * 1.12 + 8.0, 8.0)
    } else {
        Vec3::new(p.x, p.y + 16.0, 2.0)
    }
}

/// What the table in the world needs (as `blackjack_view`'s), and the game's camera.
type InWorld<'w, 's> = (
    Option<Res<'w, nvf_play::sim::SimState>>,
    Option<Res<'w, nvf_play::PlayWorld>>,
    Query<'w, 's, Entity, With<crate::world::WorldRoot>>,
    Query<'w, 's, (Entity, &'static nvf_play::RefVisual)>,
    Query<'w, 's, &'static mut Visibility>,
    ResMut<'w, crate::roulette_view::TableCamera>,
    Query<'w, 's, Entity, With<nvf_play::PlayerCamera>>,
);

struct Built {
    entities: Vec<Entity>,
    space: Entity,
    camera: Entity,
    /// At a table in the world: the table's space into game space, and the game camera's
    /// place (Bevy space); what was hidden.
    world: Option<(bevy::math::Affine3A, Transform)>,
    hidden: Vec<(Entity, Visibility)>,
    layer: RenderLayers,
    casino: Casino,
    card_front: Handle<Mesh>,
    card_back: Handle<Mesh>,
    chips_material: Handle<NifMaterial>,
    button: Entity,
    cards: HashMap<String, (Entity, Entity)>,
    textures: HashMap<String, Handle<NifMaterial>>,
    back: Handle<NifMaterial>,
    stacks: Vec<StackPlace>,
    stack_entities: Vec<Entity>,
    slots: Vec<Entity>,
}

#[derive(Default)]
struct Drawn {
    built: Option<Built>,
    warned: bool,
}

fn material(models: &mut ModelCtx, path: &str, tint: Color, alpha: AlphaMode) -> Option<Handle<NifMaterial>> {
    let texture = models.textures.get(path, true, &models.data, &mut models.images, models.formats.as_deref())?;
    Some(models.materials.add(nif_material(StandardMaterial { base_color: tint, base_color_texture: Some(texture), perceptual_roughness: 0.85, reflectance: 0.2, alpha_mode: alpha, ..default() })))
}

#[allow(clippy::too_many_arguments)]
fn show(
    mut commands: Commands,
    screen: Res<PokerScreen>,
    view: Option<ResMut<PokerView>>,
    mut models: ModelCtx,
    window: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    time: AidTime,
    mut transforms: Query<&mut Transform>,
    mut drawn: Local<Drawn>,
    (sim, play, world_root, refs, mut visibility, mut table_camera, player_camera): InWorld,
) {
    let Some(s) = screen.0.as_ref() else {
        if let Some(mut b) = drawn.built.take() {
            for e in b.entities {
                commands.entity(e).despawn();
            }
            crate::roulette_view::unhide(&mut b.hidden, &mut visibility);
            if b.world.is_some() {
                table_camera.set_if_neq(crate::roulette_view::TableCamera(None));
            }
            commands.remove_resource::<PokerView>();
        }
        return;
    };
    if drawn.built.as_ref().is_some_and(|b| b.casino != s.casino) {
        if let Some(mut b) = drawn.built.take() {
            for e in b.entities {
                commands.entity(e).despawn();
            }
            crate::roulette_view::unhide(&mut b.hidden, &mut visibility);
        }
    }
    if drawn.built.is_none() {
        // At a poker table in the world: the oval on it.
        let world = s.table_ref.zip(sim.as_deref()).zip(play.as_deref()).zip(world_root.single().ok()).and_then(|(((t, sim), play), root)| {
            let a = table_place(sim, play, &mut models, t)?;
            let staff = crate::roulette_view::in_the_way(sim, play, &mut models, t);
            info!("poker view: at the world's table {t}: the table scaled {:.3}; {} things at the table hidden", a.matrix3.x_axis.length(), staff.len());
            Some((root, a, staff))
        });
        let Some(b) = build(&mut commands, &mut models, s.casino, window.single().ok(), world.as_ref().map(|w| (w.0, w.1)), player_camera.single().ok()) else {
            if !drawn.warned {
                warn!("poker view: the table's pictures are not in the archives; the table is drawn without them");
                drawn.warned = true;
            }
            return;
        };
        let mut b = b;
        for r in world.map(|w| w.2).unwrap_or_default() {
            let more = crate::roulette_view::hide_ref(r.0, &refs, &mut visibility);
            b.hidden.extend(more);
        }
        drawn.built = Some(b);
        return;
    }
    let Some(b) = drawn.built.as_mut() else { return };
    if let Some((_, at)) = b.world {
        table_camera.set_if_neq(crate::roulette_view::TableCamera(Some((at, FOV_DEG))));
    }
    let now = time.elapsed_secs();
    let layer = b.layer.clone();
    let (cards, chips, button, open_slots) = layout(s);
    // Cards: kept by their place's key, slid in from the deck and turned over.
    let mut seen: Vec<String> = Vec::new();
    for c in &cards {
        seen.push(c.key.clone());
        let path = s.casino.card_texture(c.card);
        let face = match b.textures.get(&path) {
            Some(m) => m.clone(),
            None => {
                let m = material(&mut models, &path, Color::WHITE, AlphaMode::Mask(0.5)).unwrap_or_else(|| b.back.clone());
                b.textures.insert(path.clone(), m.clone());
                m
            }
        };
        let (root, front) = *b.cards.entry(c.key.clone()).or_insert_with(|| {
            let root = commands.spawn((Transform::default(), Visibility::Inherited, ChildOf(b.space))).id();
            let front = commands.spawn((Mesh3d(b.card_front.clone()), MeshMaterial3d(face.clone()), Transform::default(), layer.clone(), ChildOf(root))).id();
            commands.spawn((Mesh3d(b.card_back.clone()), MeshMaterial3d(b.back.clone()), Transform::default(), layer.clone(), ChildOf(root)));
            (root, front)
        });
        commands.entity(front).insert(MeshMaterial3d(face));
        let t = ((now - c.since) / DEAL_SECONDS).clamp(0.0, 1.0);
        let ease = t * t * (3.0 - 2.0 * t);
        let target = c.at.extend(0.05);
        let pos = DECK.lerp(target, ease) + Vec3::Z * (4.0 * ease * (1.0 - ease));
        let flip = if c.face_up { ease } else { 0.0 };
        let rot = Quat::from_rotation_z(c.turn_deg.to_radians() * ease) * Quat::from_rotation_y(PI * (1.0 - flip));
        let tr = Transform::from_translation(pos).with_rotation(rot);
        if let Ok(mut x) = transforms.get_mut(root) {
            *x = tr;
        } else {
            commands.entity(root).insert(tr);
        }
        commands.entity(root).insert(if now < c.since { Visibility::Hidden } else { Visibility::Inherited });
    }
    b.cards.retain(|k, (root, _)| {
        let keep = seen.contains(k);
        if !keep {
            commands.entity(*root).despawn();
        }
        keep
    });
    // Chips: made again when they change.
    if chips != b.stacks {
        for e in b.stack_entities.drain(..) {
            commands.entity(e).despawn();
        }
        for (k, st) in chips.iter().enumerate() {
            let mesh = models.meshes.add(stack_mesh(st.column, st.count, k as u32 * 31 + st.count));
            let e = commands.spawn((Mesh3d(mesh), MeshMaterial3d(b.chips_material.clone()), Transform::from_translation(st.at.extend(0.0)), layer.clone(), ChildOf(b.space))).id();
            b.stack_entities.push(e);
        }
        b.stacks = chips;
    }
    // The dealer button and the outlines of the board's cards still to come.
    match button {
        Some(p) => {
            commands.entity(b.button).insert((Transform::from_translation(p.extend(0.0)), Visibility::Inherited));
        }
        None => {
            commands.entity(b.button).insert(Visibility::Hidden);
        }
    }
    let hand_on = s.table.street != Street::Over;
    for (k, e) in b.slots.iter().enumerate() {
        let open = hand_on && k >= 5 - open_slots;
        commands.entity(*e).insert(if open { Visibility::Inherited } else { Visibility::Hidden });
    }
    // Where the seats' labels go on the picture.
    if let (Some(mut v), Ok((cam, gt))) = (view, cameras.get(b.camera)) {
        let anchors: Vec<Vec2> = (0..SEATS.len())
            .map(|i| {
                let p = match b.world {
                    Some((a, _)) => nvf_play::space::to_bevy(a.transform_point3(anchor(i)).to_array()),
                    None => PLACE + nvf_play::space::to_bevy(anchor(i).to_array()),
                };
                cam.world_to_ndc(gt, p).map_or(Vec2::splat(-1.0), |n| Vec2::new((n.x + 1.0) / 2.0, (1.0 - n.y) / 2.0))
            })
            .collect();
        if v.anchors != anchors {
            v.anchors = anchors;
        }
    }
}

/// A piece of the table under `space`; `shadow`: it casts one.
#[allow(clippy::too_many_arguments)]
fn part(commands: &mut Commands, models: &mut ModelCtx, (space, layer): (Entity, &RenderLayers), mesh: Mesh, m: Handle<NifMaterial>, tr: Transform, shadow: bool) -> Entity {
    let h = models.meshes.add(mesh);
    let mut e = commands.spawn((Mesh3d(h), MeshMaterial3d(m), tr, layer.clone(), ChildOf(space)));
    if !shadow {
        e.insert(NotShadowCaster);
    }
    e.id()
}

/// The pedestal under the top: a column and a foot, of the rail's wood, down to the floor
/// [design: the casinos' card tables stand on one or two such columns].
const COLUMN_R: f32 = 9.0;
const FOOT_R: f32 = 26.0;
const FOOT_H: f32 = 3.0;

/// The table itself: felt, rail, the casino's name and the ring on the felt, the pedestal.
fn body(commands: &mut Commands, models: &mut ModelCtx, sp: (Entity, &RenderLayers), casino: Casino, felt: Handle<NifMaterial>, rail: Handle<NifMaterial>, ink: Handle<NifMaterial>) {
    // The rail's wood and padding are drawn from both sides: seen from standing height,
    // the band under the cushion was culled away in places and the floor showed through.
    if let Some(mut m) = models.materials.get_mut(&rail) {
        m.base.cull_mode = None;
        m.base.double_sided = true;
    }
    part(commands, models, sp, felt_mesh(), felt, Transform::IDENTITY, false);
    part(commands, models, sp, rail_mesh(), rail.clone(), Transform::IDENTITY, true);
    part(commands, models, sp, under_mesh(), rail.clone(), Transform::IDENTITY, true);
    // The casino's name printed faintly in the felt: its picture is light on black, added.
    if let Some(l) = casino.logo().and_then(|p| material(models, p, Color::srgb(0.42, 0.42, 0.42), AlphaMode::Add)) {
        part(commands, models, sp, quad_mesh(Vec2::new(52.0, 26.0), Vec2::ONE), l, Transform::from_xyz(0.0, 24.0, 0.04), false);
    }
    part(commands, models, sp, ring(-13.0, 0.6, 0.03).mesh(), ink, Transform::IDENTITY, false);
    // Bevy's cylinders stand on Y; the table's space is Z up.
    let upright = Quat::from_rotation_x(PI / 2.0);
    let column = -FLOOR - FOOT_H;
    part(commands, models, sp, Mesh::from(Cylinder::new(COLUMN_R, column)), rail.clone(), Transform::from_xyz(0.0, 0.0, FLOOR + FOOT_H + column / 2.0).with_rotation(upright), true);
    part(commands, models, sp, Mesh::from(Cylinder::new(FOOT_R, FOOT_H)), rail, Transform::from_xyz(0.0, 0.0, FLOOR + FOOT_H / 2.0).with_rotation(upright), true);
}

/// The table's space into game space at a world table `t`: the oval's long side along the
/// table's, as long; the seat nearest the camera on the player's side; its foot on the floor
/// and its top as high as the world table's (`roulette_view::fit`, then the height).
pub fn table_place(sim: &nvf_play::sim::SimState, play: &nvf_play::PlayWorld, models: &mut ModelCtx, t: fnv_sim::FormId) -> Option<bevy::math::Affine3A> {
    let place = crate::roulette_view::ref_place(sim, t)?;
    let base = sim.world.peek(t).map(|r| r.base).or_else(|| sim.base_of(t.0))?;
    let path = nvf_play::dropped::model_path(&play.0, base, 0)?;
    let (wlo, whi) = models.model(&format!("meshes\\{}", path.trim_start_matches("meshes\\").trim_start_matches("Meshes\\")))?.bounds?;
    // The player's side: where they stand now, else the table's front.
    let player = sim.world.peek(fnv_sim::FormId::PLAYER_REF).map(|p| Vec3::from(p.pos)).filter(|p| p.distance(place.translation.into()) < 600.0).unwrap_or(place.transform_point3(Vec3::new(0.0, -200.0, 0.0)));
    let (long, short) = (HALF + RADIUS + RAIL, RADIUS + RAIL);
    let f = crate::roulette_view::fit((Vec3::new(-long, -short, FLOOR), Vec3::new(long, short, 0.0)), (wlo, whi), EYE, place.inverse().transform_point3(player));
    let k = f.matrix3.x_axis.length().max(1e-3);
    let c = (whi.z - wlo.z) / (-FLOOR * k);
    let height = bevy::math::Affine3A::from_translation(Vec3::Z * FLOOR) * bevy::math::Affine3A::from_scale(Vec3::new(1.0, 1.0, c)) * bevy::math::Affine3A::from_translation(Vec3::Z * -FLOOR);
    Some(place * f * height)
}

fn build(commands: &mut Commands, models: &mut ModelCtx, casino: Casino, window: Option<&Window>, world: Option<(Entity, bevy::math::Affine3A)>, player_camera: Option<Entity>) -> Option<Built> {
    let world = world.zip(player_camera);
    let layer = if world.is_some() { RenderLayers::layer(0) } else { RenderLayers::layer(POKER_LAYER) };
    // A picture that does not load leaves its part a plain colour, never the whole table
    // undrawn (a refused chips picture once left Gomorrah's screen black) [design].
    let mut or_plain = |path: &str, alpha: AlphaMode, plain: Color| {
        material(models, path, Color::WHITE, alpha).unwrap_or_else(|| {
            warn!("poker view: no picture {path}; that part is drawn plain");
            models.materials.add(nif_material(StandardMaterial { base_color: plain, perceptual_roughness: 0.85, alpha_mode: alpha, ..default() }))
        })
    };
    let felt = or_plain(casino.felt(), AlphaMode::Opaque, Color::srgb(0.12, 0.3, 0.16));
    let rail = or_plain(RAIL_TEXTURE, AlphaMode::Opaque, Color::srgb(0.25, 0.16, 0.1));
    let chips_material = or_plain(CHIPS_TEXTURE, AlphaMode::Opaque, Color::srgb(0.7, 0.15, 0.12));
    let back = or_plain(&casino.back_texture(), AlphaMode::Opaque, Color::srgb(0.6, 0.1, 0.1));
    let floor = material(models, FLOOR_TEXTURE, Color::srgb(0.35, 0.35, 0.35), AlphaMode::Opaque);
    let platinum = material(models, BUTTON_TEXTURE, Color::WHITE, AlphaMode::Opaque);
    let ink = models.materials.add(nif_material(StandardMaterial { base_color: Color::srgba(0.93, 0.85, 0.55, 0.35), unlit: true, alpha_mode: AlphaMode::Blend, ..default() }));

    let (holder, space) = match world {
        // Under the world root, in the world table's place (the root turns game space).
        Some(((root, a), _)) => {
            let h = commands.spawn((Name::new("Poker table (at the world's)"), Transform::from_matrix(Mat4::from(a)), Visibility::Inherited, ChildOf(root))).id();
            (h, commands.spawn((Transform::IDENTITY, Visibility::Inherited, ChildOf(h))).id())
        }
        None => {
            let h = commands.spawn((Name::new("Poker table"), Transform::from_translation(PLACE), Visibility::Inherited)).id();
            // Game space (Z up) into Bevy's (Y up), as the world root does.
            (h, commands.spawn((Transform::from_rotation(Quat::from_rotation_x(-PI / 2.0)), Visibility::Inherited, ChildOf(h))).id())
        }
    };
    let sp = (space, &layer);
    body(commands, models, sp, casino, felt, rail.clone(), ink.clone());
    if let Some(f) = floor.filter(|_| world.is_none()) {
        part(commands, models, sp, quad_mesh(Vec2::splat(900.0), Vec2::splat(900.0 / 160.0)), f, Transform::from_xyz(0.0, 0.0, FLOOR), false);
    }
    // Outlines of the board's places.
    let mut slots = Vec::new();
    for k in 0..5 {
        let mut g = Geo::default();
        let (w, h, t) = (CARD.x / 2.0, CARD.y / 2.0, 0.18);
        for (a, b) in [(Vec2::new(-w, -h), Vec2::new(w, -h)), (Vec2::new(w, -h), Vec2::new(w, h)), (Vec2::new(w, h), Vec2::new(-w, h)), (Vec2::new(-w, h), Vec2::new(-w, -h))] {
            let d = (b - a).normalize();
            let n = Vec2::new(-d.y, d.x) * t;
            g.quad([((a - n).extend(0.03), Vec3::Z, Vec2::ZERO), ((b - n).extend(0.03), Vec3::Z, Vec2::ZERO), ((b + n).extend(0.03), Vec3::Z, Vec2::ZERO), ((a + n).extend(0.03), Vec3::Z, Vec2::ZERO)]);
        }
        let e = part(commands, models, sp, g.mesh(), ink.clone(), Transform::from_translation((BOARD + Vec2::new(BOARD_GAP * k as f32, 0.0)).extend(0.0)), false);
        commands.entity(e).insert(Visibility::Hidden);
        slots.push(e);
    }
    let button = match platinum {
        Some(m) => {
            let mut g = Geo::default();
            let segs = 32;
            let c = g.vertex(Vec3::new(0.0, 0.0, 0.4), Vec3::Z, Vec2::splat(0.5));
            for k in 0..segs {
                let (a0, a1) = (k as f32 / segs as f32 * TAU, (k + 1) as f32 / segs as f32 * TAU);
                let uv = |a: f32| Vec2::new(0.5 + 0.48 * a.cos(), 0.5 - 0.48 * a.sin());
                let a = g.vertex(Vec3::new(3.0 * a0.cos(), 3.0 * a0.sin(), 0.4), Vec3::Z, uv(a0));
                let b = g.vertex(Vec3::new(3.0 * a1.cos(), 3.0 * a1.sin(), 0.4), Vec3::Z, uv(a1));
                g.idx.extend([c, a, b]);
                g.quad([
                    (Vec3::new(3.0 * a0.cos(), 3.0 * a0.sin(), 0.0), Vec3::new(a0.cos(), a0.sin(), 0.0), uv(a0)),
                    (Vec3::new(3.0 * a1.cos(), 3.0 * a1.sin(), 0.0), Vec3::new(a1.cos(), a1.sin(), 0.0), uv(a1)),
                    (Vec3::new(3.0 * a1.cos(), 3.0 * a1.sin(), 0.4), Vec3::new(a1.cos(), a1.sin(), 0.0), uv(a1)),
                    (Vec3::new(3.0 * a0.cos(), 3.0 * a0.sin(), 0.4), Vec3::new(a0.cos(), a0.sin(), 0.0), uv(a0)),
                ]);
            }
            part(commands, models, sp, g.mesh(), m, Transform::IDENTITY, true)
        }
        None => commands.spawn((Transform::IDENTITY, Visibility::Hidden, ChildOf(space))).id(),
    };
    let (front, back_mesh) = card_meshes();
    let card_front = models.meshes.add(front);
    let card_back = models.meshes.add(back_mesh);

    if let Some(((_, a), camera)) = world {
        let to = |p: Vec3| nvf_play::space::to_bevy(a.transform_point3(p).to_array());
        let at = Transform::from_translation(to(EYE)).looking_at(to(AIM), Vec3::Y);
        let lamp = crate::roulette_view::table_lamp(commands, to(Vec3::new(0.0, -6.0, 0.0)));
        commands.insert_resource(PokerView { image: None, anchors: Vec::new() });
        info!("poker view: the table built at the world's for {} (felt {}, deck {})", casino.title(), casino.felt(), casino.back_texture());
        return Some(Built {
            entities: vec![holder, lamp],
            space,
            camera,
            world: Some((a, at)),
            hidden: Vec::new(),
            layer,
            casino,
            card_front,
            card_back,
            chips_material,
            button,
            cards: HashMap::new(),
            textures: HashMap::new(),
            back,
            stacks: Vec::new(),
            stack_entities: Vec::new(),
            slots,
        });
    }
    let size = window.map(|w| Extent3d { width: w.physical_width().max(1), height: w.physical_height().max(1), depth_or_array_layers: 1 }).unwrap_or(Extent3d { width: 1280, height: 720, depth_or_array_layers: 1 });
    let mut image = Image::new_fill(size, TextureDimension::D2, &[0, 0, 0, 255], TextureFormat::Bgra8UnormSrgb, RenderAssetUsages::default());
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    let image = models.images.add(image);
    let to = |p: Vec3| PLACE + nvf_play::space::to_bevy(p.to_array());
    let camera = commands
        .spawn((
            Name::new("Poker camera"),
            Camera3d::default(),
            Camera { order: -4, clear_color: ClearColorConfig::Custom(Color::srgb(0.01, 0.01, 0.012)), ..default() },
            RenderTarget::Image(ImageRenderTarget::from(image.clone())),
            Projection::Perspective(PerspectiveProjection { fov: FOV_DEG.to_radians(), near: 1.0, far: 3000.0, ..default() }),
            Tonemapping::Linear,
            Msaa::Sample4,
            AmbientLight { color: Color::srgb(1.0, 0.95, 0.85), brightness: AMBIENT * crate::sky::light_unit(), ..default() },
            Transform::from_translation(to(EYE)).looking_at(to(AIM), Vec3::Y),
            layer.clone(),
        ))
        .id();
    // A warm lamp over the table [design: the mock-up's light pool], in the engine's light
    // units: the game's attenuation replaces Bevy's (lights.rs), so its reach is `range`.
    let lamp = commands
        .spawn((
            SpotLight { color: Color::srgb(1.0, 0.93, 0.8), intensity: crate::lights::lumens(LAMP, crate::sky::light_unit()), range: LAMP_RANGE, radius: 6.0, shadow_maps_enabled: true, inner_angle: 0.55, outer_angle: 1.05, ..default() },
            Transform::from_translation(to(Vec3::new(0.0, -6.0, 95.0))).looking_at(to(Vec3::new(0.0, -6.0, 0.0)), Vec3::Z),
            layer.clone(),
        ))
        .id();
    commands.insert_resource(PokerView { image: Some(image), anchors: Vec::new() });
    info!("poker view: the table built for {} (felt {}, deck {})", casino.title(), casino.felt(), casino.back_texture());
    Some(Built {
        entities: vec![holder, camera, lamp],
        space,
        camera,
        world: None,
        hidden: Vec::new(),
        layer,
        casino,
        card_front,
        card_back,
        chips_material,
        button,
        cards: HashMap::new(),
        textures: HashMap::new(),
        back,
        stacks: Vec::new(),
        stack_entities: Vec::new(),
        slots,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_oval_goes_round_once() {
        let e = oval(200, 0.0);
        let (first, last) = (e[0], e[e.len() - 1]);
        assert!((first.0 - last.0).length() < 1e-3, "{first:?} {last:?}");
        // Every point lies on the oval.
        for (p, d, _) in &e {
            let q = Vec2::new((p.x.abs() - HALF).max(0.0), p.y);
            assert!((q.length() - RADIUS).abs() < 1e-3, "{p:?}");
            assert!((d.length() - 1.0).abs() < 1e-4);
        }
    }

    #[test]
    fn amounts_make_stacks_of_at_most_twenty() {
        let mut out = Vec::new();
        stacks(1999, Vec2::ZERO, Vec2::X, &mut out);
        assert!(out.iter().all(|s| s.count <= STACK_MAX));
        assert!(out.iter().any(|s| s.count == STACK_MAX), "{out:?}");
        // Side by side, one gap apart.
        assert!(out.windows(2).all(|w| (w[1].at.x - w[0].at.x - STACK_GAP).abs() < 1e-4));
    }
}
