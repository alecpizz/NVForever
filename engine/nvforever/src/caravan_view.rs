//! The Caravan table in 3D (`nvf_play::caravan`, docs/spec-notes/caravan.md §8): the
//! planks and the cards as lit, shadowed 3D cards, drawn by a camera of their own into a
//! picture the game screen shows behind its text ([`nvf_play::caravan::CaravanView`]), as
//! the poker table is (`poker_view.rs`).
//!
//! - The original has 3D models for this screen (`meshes\terminals\nv_caravan\`: the
//!   table, one model per caravan place of each side with four cards in it, the decks)
//!   with no animation sequences in them [observed: the files' nodes]; where it puts them
//!   at run time is not known, and the player's places lie off the table model. NVForever
//!   lays the table out itself: every card where the flat screen draws it
//!   (`nvf_play::caravan::screen::table_cards`), so the names, values, outlines and
//!   buttons the screen draws over the picture fall on the cards [design].
//! - The camera looks straight down (orthographic), the frame's units being the table's:
//!   x right, y up the screen (the frame's y turned over), z up off the planks [design].
//! - The planks are the game's table texture (`nv_caravan-table01.dds`, its normal map
//!   `_n`) over the whole view; a warm lamp above the left of the table and a little
//!   ambient light leave the edges darker, as an in-game frame of the original [observed: buckallobill, in game, FNV 1.4.0.525]
//!   [design].
//! - Cards are the cards' own face and back textures (CCRD TX00/TX01). Each lies a hair
//!   above the one under it, turned by up to [`JITTER_DEG`] (the original's place models
//!   turn theirs by -7.5..+10 degrees [observed: player_*/opponent_* nodes' rotations]).
//!   A face card lies over the card it acts on, a quarter of a card to its right and
//!   lower, as in the original's place models [observed: their Card_02..04 nodes].
//! - Moves slide [design]: a card played goes from the hand to its place (the opponent's
//!   turns face up on the way), a card drawn from the deck to the hand, a card thrown away
//!   (a discard, a jack's victim, a caravan thrown away) off the left of the table. A card
//!   in flight is lifted off the table, so its shadow shows how high.
//! - The pot lies on the planks under the cards, as in the original: bottle caps, the
//!   Legion's denarius and aureus and NCR notes cut from the game's own Caravan money
//!   sheet (`caravanmoney_d.dds`, 4096 by 2048: the caps and coins along the top, the
//!   $100, $5 and $20 notes under them, each cut out by its alpha) [observed: the texture].
//!   How many and which follow the pot: more of them as it grows, Legion coins from 20,
//!   notes from 40, bigger notes and gold for bigger pots, each in a place and turn fixed
//!   by its number, so the pile only grows as the bet does [design: fill the caravan play area like the original,
//!   matching the bet].
//!   Caps and coins are the game's own Caravan models (`meshes\terminals\nv_caravan\
//!   currency-coin_1.nif` the cap, `_2` silver, `_3` gold [observed: their textures and
//!   shapes]), so they stand up off the planks and catch the lamp; the notes lie flat. A
//!   piece goes only where no card lies in a caravan of up to four cards, a hand or a deck
//!   (`money_zones`); one that finds no such place is left out [design: nothing on the table overlaps the cards, cash can go under
//!   everything else; and the planks past the caravans give space for more piles].

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ImageRenderTarget, RenderTarget, ScalingMode};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{AmbientLight, NotShadowCaster};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::window::PrimaryWindow;
use nvf_assets::model::ModelCtx;
use nvf_assets::{nif_material, NifMaterial};
use nvf_play::caravan::screen::{card_texture, money_zones, table_cards, text_zones, view_width, TableCard, FRAME_W};
use nvf_play::caravan::{CaravanScreen, CaravanView, Session, Stage};
use crate::caravan_pot::PotSim;
use nvf_play::pause::AidTime;
use std::collections::HashMap;
use std::f32::consts::PI;

/// The render layer of the table, its lights and its camera.
pub const CARAVAN_LAYER: usize = 14;
/// Where the table is put, far from the world (Bevy space) [design].
const PLACE: Vec3 = Vec3::new(300_000.0, -300_000.0, 0.0);
const PLANKS: &str = "textures\\terminals\\nv_caravan\\nv_caravan-table01.dds";
const PLANKS_NORMAL: &str = "textures\\terminals\\nv_caravan\\nv_caravan-table01_n.dds";
/// The camera's height over the planks (orthographic: only the depth range matters).
const EYE_UP: f32 = 3000.0;
/// The lamp: above the table's left, this high, its brightness and reach; the ambient
/// light [design: by eye against a frame of the original [observed: buckallobill, in game, FNV 1.4.0.525]].
const LAMP_AT: Vec2 = Vec2::new(-500.0, 250.0);
const LAMP_UP: f32 = 1100.0;
const LAMP: f32 = 1.25;
const LAMP_RANGE: f32 = 2400.0;
const AMBIENT: f32 = 0.12;
/// Each card this much higher than the one listed before it.
const CARD_STEP: f32 = 1.2;
/// A raised hand card, this much higher.
const RAISED_UP: f32 = 24.0;
/// The most a card is turned, either way (degrees).
const JITTER_DEG: f32 = 3.0;
/// How fast a card closes on its place (per second, exponential) and how high it is
/// lifted per unit still to go (at most [`LIFT_MAX`]).
const SLIDE_RATE: f32 = 9.0;
const LIFT_PER_UNIT: f32 = 0.18;
const LIFT_MAX: f32 = 70.0;
/// The money sheet, and where each piece is on it (u0, v0, u1, v1) [observed: the
/// texture's alpha; 4096 by 2048].
const MONEY_SHEET: &str = "textures\\terminals\\nv_caravan\\caravanmoney_d.dds";
const fn sheet(x0: f32, y0: f32, x1: f32, y1: f32) -> [f32; 4] {
    [x0 / 4096.0, y0 / 2048.0, x1 / 4096.0, y1 / 2048.0]
}
const CAP_PLAIN: [f32; 4] = sheet(0.0, 13.0, 506.0, 518.0);
const CAP_NUKA: [f32; 4] = sheet(524.0, 13.0, 1032.0, 518.0);
const DENARIUS: [f32; 4] = sheet(1159.0, 6.0, 1678.0, 526.0);
const AUREUS: [f32; 4] = sheet(2332.0, 1.0, 2852.0, 521.0);
const NOTE_100: [f32; 4] = sheet(26.0, 534.0, 1338.0, 1273.0);
const NOTE_5: [f32; 4] = sheet(1380.0, 534.0, 2706.0, 1276.0);
const NOTE_20: [f32; 4] = sheet(2745.0, 534.0, 4056.0, 1275.0);
/// Real sizes on the table's scale: a card on it is 147 high, a playing card 88.9 mm
/// (3.5 in), so 1.65 units to the millimetre [documented: playing cards are 2.5 by 3.5 in;
/// a US note is 156 by 66.3 mm; a crown cap is about 32 mm across; the Legion coins are
/// given a quarter's 24.3 mm (denarius) and an aureus's 20 mm, hypothesis: the game gives
/// no coin size].
pub(crate) const MM: f32 = 147.0 / 88.9;
/// A cap across, a coin across, a note's length; the note keeps its picture's shape
/// (1312 by 740 on the sheet: 156 by 88 mm), which a real note's 156 by 66 would squash.
const CAP_SIZE: Vec2 = Vec2::new(32.0 * MM, 32.0 * MM);
const DENARIUS_SIZE: Vec2 = Vec2::new(24.3 * MM, 24.3 * MM);
const AUREUS_SIZE: Vec2 = Vec2::new(20.0 * MM, 20.0 * MM);
const NOTE_SIZE: Vec2 = Vec2::new(156.0 * MM, 156.0 * MM * 740.0 / 1312.0);
/// The most pieces on the table [design: a pot of 10434 shown as 48 pieces
/// needs way more money].
const MONEY_MAX: usize = 240;
/// How many notes, coins and caps lie in one spot ([`money`]), fewest and most ([design]: "Bigger piles of caps ... 10 caps to 30 caps a pile"), and how far apart
/// spots keep (frame units) [design].
const SPOT_PIECES: [(usize, usize); 3] = [(3, 6), (8, 16), (10, 30)];
const SPOT_GAP: f32 = 10.0;
/// Spots may sit this much of their reaches apart, so piles overlap at their edges and
/// many more fit on the planks between the cards [design: more possible
/// pile spots and overlapping piles].
const SPOT_NEAR: f32 = 0.55;
/// The most pieces a spot of notes, coins and caps grows to once there is no room for
/// another of its kind [design: notes stay few, the pot test, 2026-11-12, had a fan of
/// many press its bottom note into the planks].
const SPOT_MOST: [usize; 3] = [8, 30, 45];
/// How many searches for a new spot of a kind may find no room before that kind only
/// grows its spots [design].
const SPOT_MISSES: usize = 3;
/// How far a spot's notes lie from its middle, in a note's widths [design: at 0.3 with up
/// to 8 notes, the pot test, 2026-11-12, had one pressed into the planks under the rest].
const NOTE_FAN: f32 = 0.45;
/// How many ways a note is tried before it is left out.
const NOTE_TRIES: u32 = 8;
/// How far a piece keeps from where a card can lie, and how many places it tries; the
/// pot's walls stand [`MONEY_WALL`] off those places, so a piece thrown a little off its
/// place still misses them (the pot test, 2026-11-12, at 5 apart: a cap leaning 40
/// degrees on one) [design].
const MONEY_CLEAR: f32 = 20.0;
const MONEY_WALL: f32 = 4.0;
const MONEY_TRIES: u32 = 400;
/// Each spot is a low pile [design: stacked in 3D, not all spread out]:
/// its caps spiral out from its middle this far apart (by the golden angle, so each new
/// one overlaps the ones before and rests on them) [design].
/// (0.35 piled the big piles so steeply that a cap wedged high in one rested 43 degrees
/// off flat, the pot test, 2026-11-12.)
const HEAP_STEP: f32 = CAP_SIZE.x * 0.42;
/// The golden angle (radians).
const GOLDEN: f32 = 2.399_963;
/// How tall a cap and a coin are [hypothesis: a crown cap is about 6 mm tall, a coin
/// about 2 mm; the game gives no size].
pub(crate) const CAP_THICK: f32 = 6.0 * MM;
const COIN_THICK: f32 = 2.0 * MM;
/// The most a piece resting on another leans (radians, about 14 degrees) [design].
const LEAN_MAX: f32 = 0.25;
/// A cap or coin on the planks sits on the notes under it (table units) [design].
const ROUND_BASE: f32 = 0.6;
/// The game's Caravan cap and coin models.
const CAP_MODEL: &str = "meshes\\terminals\\nv_caravan\\currency-coin_1.nif";
const SILVER_MODEL: &str = "meshes\\terminals\\nv_caravan\\currency-coin_2.nif";
const GOLD_MODEL: &str = "meshes\\terminals\\nv_caravan\\currency-coin_3.nif";
/// Each model has a flat disc round its piece (its shadow on the table); the piece itself
/// is this much of the disc across: the cap 0.58 of 0.74, a coin 0.55 [observed: the
/// models' vertices].
const CAP_BODY: f32 = 0.58 / 0.74;
const COIN_BODY: f32 = 0.55 / 0.74;
/// The models drew 31 units across where 53 were asked (a cap, 32 mm) in the first
/// capture, so the scale from their bounds is corrected by that [observed: 2026-11-09].
const MODEL_FIT: f32 = 53.0 / 31.0;

/// A piece of the pot on the table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Money {
    Cap,
    NukaCap,
    Denarius,
    Aureus,
    Note5,
    Note20,
    Note100,
}

impl Money {
    pub(crate) fn rect(self) -> [f32; 4] {
        match self {
            Money::Cap => CAP_PLAIN,
            Money::NukaCap => CAP_NUKA,
            Money::Denarius => DENARIUS,
            Money::Aureus => AUREUS,
            Money::Note5 => NOTE_5,
            Money::Note20 => NOTE_20,
            Money::Note100 => NOTE_100,
        }
    }

    pub(crate) fn size(self) -> Vec2 {
        match self {
            Money::Cap | Money::NukaCap => CAP_SIZE,
            Money::Denarius => DENARIUS_SIZE,
            Money::Aureus => AUREUS_SIZE,
            _ => NOTE_SIZE,
        }
    }

    pub(crate) fn note(self) -> bool {
        matches!(self, Money::Note5 | Money::Note20 | Money::Note100)
    }

    /// How tall the piece is on the table.
    pub(crate) fn thick(self) -> f32 {
        match self {
            Money::Cap | Money::NukaCap => CAP_THICK,
            Money::Denarius | Money::Aureus => COIN_THICK,
            _ => 0.0,
        }
    }

    /// The game's model for a cap or coin, and how much of the model across the piece is.
    fn model(self) -> Option<(&'static str, f32)> {
        match self {
            Money::Cap | Money::NukaCap => Some((CAP_MODEL, CAP_BODY)),
            Money::Denarius => Some((SILVER_MODEL, COIN_BODY)),
            Money::Aureus => Some((GOLD_MODEL, COIN_BODY)),
            _ => None,
        }
    }
}

/// Whether a piece of `size` at `at`, turned by `turn`, keeps out of every zone: its
/// corners, the middles of its sides and its middle are tested (a zone is far bigger than
/// the gaps between those points).
fn clear_of(zones: &[(Vec2, Vec2)], at: Vec2, size: Vec2, turn: f32) -> bool {
    let (sin, cos) = turn.sin_cos();
    let h = size / 2.0;
    let on_frame = (0..9).map(|k| {
        let local = Vec2::new(h.x * ((k % 3) as f32 - 1.0), h.y * ((k / 3) as f32 - 1.0));
        at + Vec2::new(local.x * cos - local.y * sin, local.x * sin + local.y * cos)
    });
    on_frame.into_iter().all(|p| zones.iter().all(|(a, b)| p.x < a.x || p.x > b.x || p.y < a.y || p.y > b.y))
}

/// A number from 0 to 1, the same each time for `i` and `salt`.
fn scatter(i: usize, salt: u32) -> f32 {
    let mut h = (i as u32).wrapping_mul(0x9E37_79B9) ^ salt.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h % 10_000) as f32 / 9_999.0
}

/// Whether a disc of radius `r` at `at` keeps out of every zone.
fn disc_clear(zones: &[(Vec2, Vec2)], at: Vec2, r: f32) -> bool {
    zones.iter().all(|(a, b)| at.clamp(*a, *b).distance(at) >= r)
}

/// A piece of the pot on the table: what it is, its middle (frame units, y down), its turn
/// about the up axis (radians), how high its bottom is over the planks and its lean (the
/// way its low side goes, frame units, as long as the angle in radians).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    pub kind: Money,
    pub at: Vec2,
    pub turn: f32,
    pub lift: f32,
    pub lean: Vec2,
}

/// Which kind of spot a piece lies in: notes, coins or caps.
fn spot_kind(kind: Money) -> usize {
    match kind {
        Money::Cap | Money::NukaCap => 2,
        Money::Denarius | Money::Aureus => 1,
        _ => 0,
    }
}

/// How far apart a spot's caps or coins spiral out (frame units).
fn spot_step(g: usize) -> f32 {
    if g == 1 { DENARIUS_SIZE.x * 0.42 } else { HEAP_STEP }
}

/// How many pieces the `s`-th spot of kind `g` holds: each a little different, within
/// [`SPOT_PIECES`].
fn spot_size(g: usize, s: usize) -> usize {
    let (lo, hi) = SPOT_PIECES[g];
    lo + ((hi - lo + 1) as f32 * scatter(s * 3 + g, 601)).min((hi - lo) as f32) as usize
}

/// The reach from its middle (frame units) of a spot of kind `g` holding `size`: its
/// outermost piece's far edge.
fn spot_reach(g: usize, size: usize) -> f32 {
    let out = (size.max(1) - 1) as f32;
    match g {
        0 => NOTE_SIZE.y * NOTE_FAN + NOTE_SIZE.length() / 2.0,
        1 => spot_step(1) * out.sqrt() + DENARIUS_SIZE.x / 2.0,
        _ => HEAP_STEP * out.sqrt() + CAP_SIZE.x / 2.0,
    }
}

/// The pot's pieces, each kind in spots of its own [design: each kind in
/// its own spots, caps here and here, cash here, coins here, not all piled in
/// one]: a spot holds [`SPOT_PIECES`] of one kind, its middle anywhere on the planks clear
/// of the cards and the screen's words and [`SPOT_GAP`] apart from every other spot; its
/// notes lie fanned flat round the middle, its caps or coins spiral out from it in a low
/// pile, each resting on the highest piece it overlaps and leaning away from it. Piece `i`
/// is the same for every pot that has it, so the pot only grows; a spot with no room, or a
/// piece over a card's or a word's place, is left out.
pub fn money(pot: u32) -> Vec<Piece> {
    if pot == 0 {
        return Vec::new();
    }
    let n = (((pot as f32).sqrt() * 3.0).ceil() as usize).clamp(2, MONEY_MAX);
    // Off the cards and off the screen's words [design].
    let mut zones = money_zones(MONEY_CLEAR);
    zones.extend(text_zones(MONEY_CLEAR));
    // Every spot's middle and reach, each kind's spots (index into `sites`), how full the
    // kind's newest spot is, whether the kind has no room for another (and then which of
    // its spots is next), how many spots were tried, and how many pieces each spot has.
    let mut sites: Vec<(Vec2, f32)> = Vec::new();
    let mut spots: [Vec<usize>; 3] = Default::default();
    let mut filled = [0usize; 3];
    let mut full = [false; 3];
    let mut missed = [0usize; 3];
    let mut over = [0usize; 3];
    let mut laid = 0usize;
    let mut heaps: Vec<usize> = Vec::new();
    // The caps and coins so far: middle, bottom's height, thickness, radius.
    let mut rounds: Vec<(Vec2, f32, f32, f32)> = Vec::new();
    let mut out = Vec::new();
    for i in 0..n {
        let kind = if pot >= 40 && i % 4 == 3 {
            match (pot, i % 3) {
                (0..=149, _) => Money::Note5,
                (150..=599, 0) => Money::Note5,
                (150..=599, _) => Money::Note20,
                (_, 0) => Money::Note20,
                _ => Money::Note100,
            }
        } else if pot >= 20 && i % 5 == 2 {
            // Legion coins; gold among them once the pot is big.
            if pot >= 300 && i % 3 == 0 { Money::Aureus } else { Money::Denarius }
        } else if scatter(i, 7) < 0.6 {
            Money::NukaCap
        } else {
            Money::Cap
        };
        // Its own kind's spot: the one being filled, or a new one once that is full; once
        // the planks have no room for another of its kind, the next of that kind's spots
        // in turn, which grows past its size (up to SPOT_MOST).
        let g = spot_kind(kind);
        let mut overflow = full[g];
        if !full[g] && (spots[g].is_empty() || filled[g] >= spot_size(g, spots[g].len() - 1)) {
            let j = laid;
            laid += 1;
            let reach = spot_reach(g, spot_size(g, spots[g].len()));
            // The middle wants this much clear round it: a fan of notes a note's width, a
            // pile of caps or coins a cap's (the planks between the cards are narrow; each
            // piece is still kept off the cards and words by itself, and moved on screen).
            let core = if g == 0 { NOTE_SIZE.y * 0.5 } else { (reach * 0.8).min(CAP_SIZE.x) };
            // (Its pieces past the screen's edge are moved in, [`NOTE_EDGE`] inside the
            // edge walls: the bands along the top and bottom are narrower than a pile.)
            let edge = core + NOTE_EDGE;
            let site = (0..MONEY_TRIES)
                .map(|t| Vec2::new(edge + scatter(j, 211 + t) * (FRAME_W - 2.0 * edge), edge + scatter(j, 307 + t) * (960.0 - 2.0 * edge)))
                .find(|c| disc_clear(&zones, *c, core) && sites.iter().all(|(o, r)| o.distance(*c) >= (r + reach) * SPOT_NEAR + SPOT_GAP));
            match site {
                Some(c) => {
                    spots[g].push(sites.len());
                    sites.push((c, reach));
                    heaps.push(0);
                    filled[g] = 0;
                }
                // This piece goes on the kind's spots; the next tries other places, until
                // [`SPOT_MISSES`] tries have found none.
                None => {
                    missed[g] += 1;
                    full[g] = missed[g] >= SPOT_MISSES;
                    overflow = true;
                }
            }
        }
        let heap = if overflow {
            if spots[g].is_empty() {
                continue;
            }
            let h = spots[g][over[g] % spots[g].len()];
            over[g] += 1;
            h
        } else {
            filled[g] += 1;
            *spots[g].last().expect("a spot was just laid out")
        };
        // Its place in its spot.
        let k = heaps[heap];
        if k >= SPOT_MOST[g] {
            continue;
        }
        heaps[heap] += 1;
        let (c, _) = sites[heap];
        let turn = scatter(i, 53) * 2.0 * PI;
        if kind.note() {
            // Fanned flat round the middle, a hair over the notes before it; turned and
            // placed again (up to [`NOTE_TRIES`] times) where it would lie over a card or
            // a word, rather than left out.
            let placed = (0..NOTE_TRIES).find_map(|t| {
                let turn = if t == 0 { turn } else { scatter(i, 53 + 17 * t) * 2.0 * PI };
                let a = scatter(i, 401 + 17 * t) * 2.0 * PI;
                let at = on_screen(c + Vec2::from_angle(a) * scatter(i, 409 + 17 * t) * NOTE_SIZE.y * NOTE_FAN, kind.size(), turn);
                clear_of(&zones, at, kind.size(), turn).then_some((at, turn))
            });
            if let Some((at, turn)) = placed {
                out.push(Piece { kind, at, turn, lift: 0.004 * k as f32, lean: Vec2::ZERO });
            }
            continue;
        }
        // Spiralling out from the middle by the golden angle, each overlapping the ones
        // before and resting on them: a low pile.
        let a = k as f32 * GOLDEN + scatter(heap, 503) * 2.0 * PI;
        let me = kind.size().x / 2.0;
        // Moved in, whole on the screen, rather than spilling over its edge.
        let at = on_screen(c + Vec2::from_angle(a) * spot_step(g) * (k as f32).sqrt(), Vec2::splat(2.0 * me), 0.0);
        if !disc_clear(&zones, at, me) {
            continue;
        }
        // On the highest piece it overlaps, leaning away from it.
        let mut lift = ROUND_BASE;
        let mut lean = Vec2::ZERO;
        // (Any spot's: a pile grown past its size may reach a neighbour.)
        for (p, l, t, r) in &rounds {
            let d = p.distance(at);
            if d < (r + me) * 0.95 && l + t > lift {
                lift = l + t;
                lean = (at - *p).normalize_or_zero() * (t / (d + me)).atan().min(LEAN_MAX);
            }
        }
        rounds.push((at, lift, kind.thick(), me));
        out.push(Piece { kind, at, turn, lift, lean });
    }
    out
}

/// How far in from the screen's edges every piece is laid whole (frame units): past the
/// pot's edge walls ([`crate::caravan_pot::EDGE_IN`]), which one placed across them could
/// not be pushed out of ([design]: two notes hung off the screen, and caps of the big heaps did too).
/// Well inside them: a cap thrown off its place or a note gliding as it falls that ends
/// against an edge wall leans on it, or is wedged into the planks (the pot test,
/// 2026-11-12, with 4 units: a cap 52 degrees off flat, a note pressed in).
const NOTE_EDGE: f32 = crate::caravan_pot::EDGE_IN + 16.0;

/// `at` moved in just far enough that a piece `size` turned by `turn` lies whole inside
/// the screen, [`NOTE_EDGE`] from its edges.
fn on_screen(at: Vec2, size: Vec2, turn: f32) -> Vec2 {
    let (s, c) = (turn.sin().abs(), turn.cos().abs());
    let reach = Vec2::new(c * size.x + s * size.y, s * size.x + c * size.y) / 2.0 + Vec2::splat(NOTE_EDGE);
    Vec2::new(at.x.clamp(reach.x, FRAME_W - reach.x), at.y.clamp(reach.y, 960.0 - reach.y))
}

/// A note's mesh: this many strips along its length, so it can bend.
const NOTE_STRIPS: usize = 12;

/// A note's mesh, a unit rectangle (x along its length) showing `rect` of the money sheet,
/// bent by `bend` (table units: the ends this far over the middle, the whole kept centred).
/// Two-sided: a note that lands face down shows the same print from below, the right way
/// round (the sheet has one face per note), rather than vanishing.
fn note_mesh(rect: [f32; 4], bend: f32, length: f32) -> Mesh {
    let (pos, normals) = note_shape(bend, length);
    let mut uv = Vec::new();
    for back in [false, true] {
        for i in 0..=NOTE_STRIPS {
            let x = i as f32 / NOTE_STRIPS as f32;
            let x = if back { 1.0 - x } else { x };
            for j in 0..2 {
                uv.push([rect[0] + x * (rect[2] - rect[0]), rect[1] + (1 - j) as f32 * (rect[3] - rect[1])]);
            }
        }
    }
    let mut idx = Vec::new();
    let behind = 2 * (NOTE_STRIPS as u32 + 1);
    for i in 0..NOTE_STRIPS as u32 {
        let (a, b, c, d) = (2 * i, 2 * i + 2, 2 * i + 3, 2 * i + 1);
        idx.extend([a, b, c, a, c, d]);
        idx.extend([a, c, b, a, d, c].map(|k| k + behind));
    }
    let m = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(bevy::mesh::Indices::U32(idx));
    m.clone().with_generated_tangents().unwrap_or(m)
}

/// A falling piece is drawn this far over its body's middle (table units).
const NOTE_UP: f32 = 0.05;
/// The least a drawn note's lowest point stays over the planks (table units).
const NOTE_CLEAR: f32 = 0.2;

/// The bend to draw a note with, `bend` or less, so that no point of it goes under the
/// planks: a note's middle sags as its ends rise ([`note_shape`]), and a note just landed
/// still bends while the air's push eases off [design: notes
/// do not clip through the table]. `z` is its middle's height drawn, `rot` its turn, `size`
/// its length and width, `grow` its drawn scale.
fn bend_above_table(z: f32, rot: Quat, size: Vec2, grow: f32, bend: f32) -> f32 {
    // The note's lowest point, for a bend: its height is linear in the bend at each point,
    // so the lowest is concave in it and scaling the bend down by t keeps at least
    // (1 - t) of the flat note's clearance plus t of the bent one's.
    let lowest = |b: f32| {
        let mut low = f32::MAX;
        for i in 0..=NOTE_STRIPS {
            let x = i as f32 / NOTE_STRIPS as f32 - 0.5;
            for y in [-0.5f32, 0.5] {
                let p = Vec3::new(x * size.x, y * size.y, b * (4.0 * x * x - 1.0 / 3.0));
                low = low.min(z + grow * (rot * p).z);
            }
        }
        low
    };
    let (flat, bent) = (lowest(0.0), lowest(bend));
    if bent >= NOTE_CLEAR {
        return bend;
    }
    if flat <= NOTE_CLEAR {
        return 0.0;
    }
    bend * ((flat - NOTE_CLEAR) / (flat - bent)).clamp(0.0, 1.0)
}

/// A note's points and normals bent by `bend` (see [`note_mesh`]): the face up, then the
/// same points again facing down.
fn note_shape(bend: f32, length: f32) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let (mut pos, mut normals) = (Vec::new(), Vec::new());
    for side in [1.0f32, -1.0] {
        for i in 0..=NOTE_STRIPS {
            let x = i as f32 / NOTE_STRIPS as f32 - 0.5;
            let z = bend * (4.0 * x * x - 1.0 / 3.0);
            let slope = bend * 8.0 * x / length.max(1.0);
            let n = Vec3::new(-slope, 0.0, 1.0).normalize() * side;
            for j in 0..2 {
                pos.push([x, j as f32 - 0.5, z]);
                normals.push(n.to_array());
            }
        }
    }
    (pos, normals)
}

/// The pot on the table: its bodies, and for each piece its entity and, for a note, its
/// mesh and the bend it was last drawn with.
struct PotDrawn {
    sim: PotSim,
    pieces: Vec<(Entity, Option<NoteDrawn>)>,
    /// The pot's caps, and whether its coming to rest was logged.
    pot: u32,
    told: bool,
}

/// A note's own mesh and the bend it was last drawn with.
type NoteDrawn = (Handle<Mesh>, f32);

/// Lays the pot out again when it changed: every piece falls in anew (`caravan_pot`).
fn place_money(commands: &mut Commands, models: &mut ModelCtx, b: &mut Built, pot: u32) {
    if b.money_pot == Some(pot) {
        return;
    }
    b.money_pot = Some(pot);
    if let Some(old) = b.pot.take() {
        for (e, _) in old.pieces {
            commands.entity(e).despawn();
        }
    }
    let pieces = money(pot);
    let mut zones = money_zones(MONEY_WALL);
    zones.extend(text_zones(MONEY_WALL));
    let mut sim = PotSim::new(&zones);
    sim.add(&pieces, 0);
    let mut drawn = Vec::new();
    for p in &sim.pieces {
        let kind = p.piece.kind;
        // Hidden until it falls; placed each frame where its body is.
        let holder = commands.spawn((Transform::default(), Visibility::Hidden, ChildOf(b.space))).id();
        let mut note = None;
        if let Some((path, body)) = kind.model() {
            spawn_piece(commands, models, b, holder, path, body * kind.size().x * MODEL_FIT, kind.thick());
        } else if let Some(mat) = money_material(models, &mut b.materials, MONEY_SHEET) {
            let mesh = models.meshes.add(note_mesh(kind.rect(), 0.0, kind.size().x));
            commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat), Transform::from_scale(kind.size().extend(1.0)), b.layer.clone(), ChildOf(holder)));
            note = Some((mesh, 0.0));
        }
        drawn.push((holder, note));
    }
    info!("caravan view: the pot of {pot} on the table as {} pieces, falling in", pieces.len());
    b.pot = Some(PotDrawn { sim, pieces: drawn, pot, told: false });
}

/// Runs the pot on by `dt` and draws each piece where its body is.
fn move_money(models: &mut ModelCtx, b: &mut Built, transforms: &mut Query<&mut Transform>, visible: &mut Query<&mut Visibility>, dt: f32) {
    let Some(pot) = b.pot.as_mut() else { return };
    if pot.sim.settled() && pot.pieces.iter().all(|(_, n)| n.as_ref().is_none_or(|(_, bend)| *bend == 0.0)) {
        return;
    }
    pot.sim.advance(dt);
    if pot.sim.settled() && !pot.told {
        pot.told = true;
        info!("caravan view: the pot of {} settled: {}", pot.pot, pot.sim.report());
    }
    for k in 0..pot.pieces.len() {
        let Some(sh) = pot.sim.shown(k, dt) else { continue };
        let (e, note) = &mut pot.pieces[k];
        if let Ok(mut v) = visible.get_mut(*e) {
            if *v != Visibility::Inherited {
                *v = Visibility::Inherited;
            }
        }
        // Seen from straight above: bigger the higher it falls, so the fall shows.
        let grow = sh.grow;
        if let Ok(mut t) = transforms.get_mut(*e) {
            *t = Transform::from_translation(sh.at + Vec3::Z * NOTE_UP).with_rotation(sh.rot).with_scale(Vec3::splat(grow));
        }
        if let Some((mesh, drawn)) = note {
            let size = pot.sim.pieces[k].piece.kind.size();
            let bend = bend_above_table(sh.at.z + NOTE_UP, sh.rot, size, grow, sh.bend);
            let bend = if bend.abs() < 0.05 { 0.0 } else { bend };
            if (bend - *drawn).abs() >= 0.05 || (bend == 0.0 && *drawn != 0.0) {
                if let Some(mut m) = models.meshes.get_mut(&*mesh) {
                    let (pos, normals) = note_shape(bend, pot.sim.pieces[k].piece.kind.size().x);
                    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
                    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
                }
                *drawn = bend;
            }
        }
    }
}

/// A cap or coin from the game's model under `holder` (which is placed where its body is,
/// the body's middle), `across` wide, `thick` tall: the model's bottom half the piece's
/// height under the holder.
#[allow(clippy::too_many_arguments)]
fn spawn_piece(commands: &mut Commands, models: &mut ModelCtx, b: &mut Built, holder: Entity, path: &str, across: f32, thick: f32) -> Option<()> {
    let pm = models.model(path)?;
    let (lo, hi) = pm.bounds?;
    let wide = (hi.x - lo.x).max(hi.y - lo.y);
    if wide <= 0.0 {
        return None;
    }
    let k = across / wide;
    // The model stands taller than the piece's body (a cap's is 2.4 times a 6 mm cap at
    // that width, the log's line below): pressed to the body's height, so a piece drawn
    // under another never reaches up through it [design].
    let kz = if hi.z > lo.z { thick / (hi.z - lo.z) } else { k };
    let inner = commands.spawn((Transform::from_translation(Vec3::Z * (-thick / 2.0 - lo.z * kz)).with_scale(Vec3::new(k, k, kz)), Visibility::Inherited, ChildOf(holder))).id();
    let root = commands.spawn((Transform::from_matrix(Mat4::from(pm.root)), Visibility::Inherited, ChildOf(inner))).id();
    let nodes: Vec<Entity> = pm.nodes.iter().map(|n| commands.spawn((Transform::from_matrix(Mat4::from(n.local)), Visibility::Inherited)).id()).collect();
    for (n, e) in pm.nodes.iter().zip(&nodes) {
        commands.entity(n.parent.map_or(root, |p| nodes[p])).add_child(*e);
    }
    for s in &pm.shapes {
        let material = two_sided(models, &mut b.materials, &s.material);
        let e = commands.spawn((Mesh3d(s.mesh.clone()), MeshMaterial3d(material), Transform::from_matrix(Mat4::from(s.own)), Visibility::Inherited, b.layer.clone())).id();
        commands.entity(s.node.map_or(root, |n| nodes[n])).add_child(e);
    }
    static NAMED: std::sync::Once = std::sync::Once::new();
    NAMED.call_once(|| info!("caravan view: {path}: shapes {:?}, {:.1} tall at {across:.1} across, drawn {thick:.1} tall", pm.shapes.iter().map(|s| s.name.clone()).collect::<Vec<_>>(), (hi.z - lo.z) * k));
    Some(())
}

/// `m` drawn from both sides: a cap's model is its crown and skirt with no inside, so one
/// lying crown down would show only its rim (the first pot-drop shots, 2026-11-12).
fn two_sided(models: &mut ModelCtx, cache: &mut HashMap<String, Handle<NifMaterial>>, m: &Handle<NifMaterial>) -> Handle<NifMaterial> {
    let key = format!("two-sided {:?}", m.id());
    if let Some(h) = cache.get(&key) {
        return h.clone();
    }
    let Some(mut both) = models.materials.get(m).cloned() else { return m.clone() };
    both.base.cull_mode = None;
    both.base.double_sided = true;
    let h = models.materials.add(both);
    cache.insert(key, h.clone());
    h
}

fn money_material(models: &mut ModelCtx, cache: &mut HashMap<String, Handle<NifMaterial>>, path: &str) -> Option<Handle<NifMaterial>> {
    if let Some(m) = cache.get(path) {
        return Some(m.clone());
    }
    let t = models.textures.get(path, true, &models.data, &mut models.images, models.formats.as_deref())?;
    let m = models.materials.add(nif_material(StandardMaterial { base_color_texture: Some(t), perceptual_roughness: 0.55, reflectance: 0.35, alpha_mode: AlphaMode::Mask(0.5), ..default() }));
    cache.insert(path.to_string(), m.clone());
    Some(m)
}

/// A card thrown away leaves to the left, and is gone after this long.
const GONE_X: f32 = -260.0;
const GONE_SECONDS: f32 = 0.9;

pub struct CaravanViewPlugin;

impl Plugin for CaravanViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, show.after(nvf_play::PlaySet::Ui).before(nvf_play::PlaySet::Sync));
    }
}

/// A card drawn: its entities, what it shows, where it is (without the lift) and how it
/// is turned; thrown away since then.
struct CardDrawn {
    root: Entity,
    front: Entity,
    back: Entity,
    form: fnv_sim::FormId,
    pos: Vec3,
    rot: Quat,
    gone: Option<f32>,
}

struct Built {
    entities: Vec<Entity>,
    space: Entity,
    camera: Entity,
    planks: Entity,
    layer: RenderLayers,
    card_mesh: Handle<Mesh>,
    cards: HashMap<String, CardDrawn>,
    leaving: Vec<CardDrawn>,
    materials: HashMap<String, Handle<NifMaterial>>,
    view_w: f32,
    /// The pot's pieces on the table, the pot they show, their meshes by kind.
    pot: Option<PotDrawn>,
    money_pot: Option<u32>,
}

#[derive(Default)]
struct Drawn {
    built: Option<Built>,
    warned: bool,
}

/// The frame's scale in the view (a view narrower than 16:9 shows the whole frame,
/// smaller), as the screen's.
fn frame_scale(view_w: f32) -> f32 {
    (view_w / FRAME_W).min(1.0)
}

/// A frame point (its units, y down) on the table (y up).
fn on_table(p: Vec2) -> Vec2 {
    Vec2::new(p.x, -p.y)
}

/// The view's centre on the table: the frame's centre.
fn centre() -> Vec2 {
    on_table(Vec2::new(FRAME_W / 2.0, 480.0))
}

/// The orthographic projection showing the whole view: `view_w` by 960 of the screen's
/// units, the frame scaled into it.
fn projection(view_w: f32) -> Projection {
    let k = frame_scale(view_w);
    Projection::Orthographic(OrthographicProjection { scaling_mode: ScalingMode::Fixed { width: view_w / k, height: 960.0 / k }, near: 1.0, far: EYE_UP * 2.0, ..OrthographicProjection::default_3d() })
}

/// A card's small turn, the same each time for its key.
fn jitter(key: &str) -> f32 {
    let h = key.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    ((h % 1000) as f32 / 999.0 * 2.0 - 1.0) * JITTER_DEG
}

fn turn(key: &str, face_up: bool) -> Quat {
    let flip = if face_up { Quat::IDENTITY } else { Quat::from_rotation_y(PI) };
    Quat::from_rotation_z(jitter(key).to_radians()) * flip
}

/// Where a card should be: its middle on the table, its height.
fn target(c: &TableCard, k: usize) -> Vec3 {
    let mid = on_table(c.at + c.size / 2.0);
    mid.extend(2.0 + CARD_STEP * k as f32 + if c.raised { RAISED_UP } else { 0.0 })
}

fn material(models: &mut ModelCtx, cache: &mut HashMap<String, Handle<NifMaterial>>, path: &str) -> Handle<NifMaterial> {
    if let Some(m) = cache.get(path) {
        return m.clone();
    }
    let full = format!("textures\\{}", path.trim_start_matches("textures\\").trim_start_matches("Textures\\"));
    let m = match models.textures.get(&full, true, &models.data, &mut models.images, models.formats.as_deref()) {
        Some(t) => models.materials.add(nif_material(StandardMaterial { base_color_texture: Some(t), perceptual_roughness: 0.8, reflectance: 0.25, alpha_mode: AlphaMode::Mask(0.5), ..default() })),
        None => {
            warn!("caravan view: no card picture {full}; that card is drawn plain");
            models.materials.add(nif_material(StandardMaterial { base_color: Color::srgb(0.85, 0.82, 0.74), perceptual_roughness: 0.8, ..default() }))
        }
    };
    cache.insert(path.to_string(), m.clone());
    m
}

#[allow(clippy::too_many_arguments)]
fn show(
    mut commands: Commands,
    screen: Res<CaravanScreen>,
    mut models: ModelCtx,
    window: Query<&Window, With<PrimaryWindow>>,
    time: AidTime,
    mut transforms: Query<&mut Transform>,
    mut visible: Query<&mut Visibility>,
    mut projections: Query<&mut Projection>,
    mut drawn: Local<Drawn>,
) {
    let session = screen.0.as_ref().filter(|s| s.stage == Stage::Play && s.game.is_some());
    let Some(s) = session else {
        if let Some(b) = drawn.built.take() {
            for e in b.entities {
                commands.entity(e).despawn();
            }
            commands.remove_resource::<CaravanView>();
            info!("caravan view: the table put away");
        }
        return;
    };
    if drawn.built.is_none() {
        match build(&mut commands, &mut models, window.single().ok()) {
            Some(b) => drawn.built = Some(b),
            None => {
                if !drawn.warned {
                    warn!("caravan view: the table's planks are not in the archives; the flat screen is shown");
                    drawn.warned = true;
                }
                return;
            }
        }
    }
    let Some(b) = drawn.built.as_mut() else { return };
    // The view's shape may change (a resized window).
    let view_w = view_width(window.single().ok());
    if (view_w - b.view_w).abs() > 0.5 {
        b.view_w = view_w;
        if let Ok(mut p) = projections.get_mut(b.camera) {
            *p = projection(view_w);
        }
        let k = frame_scale(view_w);
        commands.entity(b.planks).insert(Transform::from_translation(centre().extend(0.0)).with_scale(Vec3::new(view_w / k, 960.0 / k, 1.0)));
    }
    let now = time.elapsed_secs();
    let dt = (time.delta_secs_f64() as f32).clamp(0.0, 0.1);
    place_cards(&mut commands, &mut models, b, s, now);
    place_money(&mut commands, &mut models, b, s.bet.pot());
    move_money(&mut models, b, &mut transforms, &mut visible, dt);
    let close = 1.0 - (-SLIDE_RATE * dt).exp();
    let cards = table_cards(s);
    let goals: HashMap<&str, (Vec3, Quat)> = cards.iter().enumerate().map(|(k, c)| (c.key.as_str(), (target(c, k), turn(&c.key, c.face_up)))).collect();
    for (key, c) in b.cards.iter_mut() {
        let Some(&(goal, rot)) = goals.get(key.as_str()) else { continue };
        c.pos = c.pos.lerp(goal, close);
        c.rot = c.rot.slerp(rot, close);
        let lift = ((goal - c.pos).truncate().length() * LIFT_PER_UNIT).min(LIFT_MAX);
        set(&mut commands, &mut transforms, c.root, c.pos + Vec3::Z * lift, c.rot, cards.iter().find(|t| t.key == *key).map_or(Vec2::ONE, |t| t.size));
    }
    b.leaving.retain_mut(|c| {
        let since = c.gone.unwrap_or(now);
        if now - since > GONE_SECONDS {
            commands.entity(c.root).despawn();
            return false;
        }
        let goal = Vec3::new(GONE_X, c.pos.y, c.pos.z);
        c.pos = c.pos.lerp(goal, close);
        let lift = ((goal - c.pos).truncate().length() * LIFT_PER_UNIT).min(LIFT_MAX);
        if let Ok(mut t) = transforms.get_mut(c.root) {
            t.translation = c.pos + Vec3::Z * lift;
        }
        true
    });
}

fn set(commands: &mut Commands, transforms: &mut Query<&mut Transform>, e: Entity, at: Vec3, rot: Quat, size: Vec2) {
    let tr = Transform::from_translation(at).with_rotation(rot).with_scale(size.extend(1.0));
    match transforms.get_mut(e) {
        Ok(mut t) => *t = tr,
        Err(_) => {
            commands.entity(e).insert(tr);
        }
    }
}

/// Cards that came and went since the last frame: a card played is the hand's card that
/// left (it flies from there), a card drawn comes from its side's deck, a card gone leaves.
fn place_cards(commands: &mut Commands, models: &mut ModelCtx, b: &mut Built, s: &Session, now: f32) {
    let cards = table_cards(s);
    let keys: Vec<&str> = cards.iter().map(|c| c.key.as_str()).collect();
    let gone_keys: Vec<String> = b.cards.keys().filter(|k| !keys.contains(&k.as_str())).cloned().collect();
    let mut gone: Vec<(String, CardDrawn)> = gone_keys.into_iter().filter_map(|k| b.cards.remove(&k).map(|c| (k, c))).collect();
    for (k, c) in cards.iter().enumerate() {
        let face = material(models, &mut b.materials, &card_texture(s, c.form, true));
        let back = material(models, &mut b.materials, &card_texture(s, c.form, false));
        if let Some(d) = b.cards.get_mut(&c.key) {
            if d.form != c.form {
                d.form = c.form;
                commands.entity(d.front).insert(MeshMaterial3d(face));
                commands.entity(d.back).insert(MeshMaterial3d(back));
            }
            continue;
        }
        // Played: the hand's card of the same form that just left.
        let from_hand = (c.key.starts_with('c')).then(|| gone.iter().position(|(gk, g)| gk.starts_with('h') && g.form == c.form)).flatten();
        let drawn = match from_hand {
            Some(i) => {
                let (_, d) = gone.swap_remove(i);
                commands.entity(d.front).insert(MeshMaterial3d(face));
                commands.entity(d.back).insert(MeshMaterial3d(back));
                d
            }
            None => {
                let side = c.key[1..].chars().next().and_then(|ch| ch.to_digit(10)).unwrap_or(0);
                let (pos, rot) = if c.key.starts_with('h') {
                    // Drawn: from its side's deck, face down.
                    let deck = cards.iter().position(|d| d.key == format!("d{side}"));
                    let at = deck.map_or(target(c, k) + Vec3::new(120.0, 0.0, 0.0), |i| target(&cards[i], i) + Vec3::Z * 2.0);
                    (at, turn(&c.key, false))
                } else {
                    (target(c, k), turn(&c.key, c.face_up))
                };
                let root = commands.spawn((Transform::from_translation(pos).with_rotation(rot).with_scale(c.size.extend(1.0)), Visibility::Inherited, ChildOf(b.space))).id();
                let front = commands.spawn((Mesh3d(b.card_mesh.clone()), MeshMaterial3d(face), Transform::from_xyz(0.0, 0.0, 0.15), b.layer.clone(), ChildOf(root))).id();
                let back_e = commands
                    .spawn((Mesh3d(b.card_mesh.clone()), MeshMaterial3d(back), Transform::from_xyz(0.0, 0.0, -0.15).with_rotation(Quat::from_rotation_y(PI)), b.layer.clone(), ChildOf(root)))
                    .id();
                CardDrawn { root, front, back: back_e, form: c.form, pos, rot, gone: None }
            }
        };
        b.cards.insert(c.key.clone(), drawn);
    }
    for (_, mut d) in gone {
        d.gone = Some(now);
        b.leaving.push(d);
    }
}

fn build(commands: &mut Commands, models: &mut ModelCtx, window: Option<&Window>) -> Option<Built> {
    let planks_tex = models.textures.get(PLANKS, true, &models.data, &mut models.images, models.formats.as_deref())?;
    let normal = models.textures.get(PLANKS_NORMAL, false, &models.data, &mut models.images, models.formats.as_deref());
    let has_normal = normal.is_some();
    let layer = RenderLayers::layer(CARAVAN_LAYER);
    let view_w = view_width(window);
    let holder = commands.spawn((Name::new("Caravan table"), Transform::from_translation(PLACE), Visibility::Inherited)).id();
    // Game space (Z up) into Bevy's (Y up), as the world root does.
    let space = commands.spawn((Transform::from_rotation(Quat::from_rotation_x(-PI / 2.0)), Visibility::Inherited, ChildOf(holder))).id();
    let quad = || Mesh::from(Rectangle::new(1.0, 1.0)).with_generated_tangents().ok();
    let planks_mesh = models.meshes.add(quad()?);
    let planks_mat = models.materials.add(nif_material(StandardMaterial { base_color_texture: Some(planks_tex), normal_map_texture: normal, perceptual_roughness: 0.9, reflectance: 0.15, ..default() }));
    let k = frame_scale(view_w);
    let planks = commands
        .spawn((
            Mesh3d(planks_mesh),
            MeshMaterial3d(planks_mat),
            Transform::from_translation(centre().extend(0.0)).with_scale(Vec3::new(view_w / k, 960.0 / k, 1.0)),
            NotShadowCaster,
            layer.clone(),
            ChildOf(space),
        ))
        .id();
    // A card's rounded outline (`crate::card_shape`): the pictures' own corners are opaque.
    let card_mesh = models.meshes.add(crate::card_shape::card_mesh(Vec2::ONE, 0.0, true));
    let size = window.map(|w| Extent3d { width: w.physical_width().max(1), height: w.physical_height().max(1), depth_or_array_layers: 1 }).unwrap_or(Extent3d { width: 1280, height: 720, depth_or_array_layers: 1 });
    let mut image = Image::new_fill(size, TextureDimension::D2, &[0, 0, 0, 255], TextureFormat::Bgra8UnormSrgb, RenderAssetUsages::default());
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    let image = models.images.add(image);
    let to = |p: Vec3| PLACE + nvf_play::space::to_bevy(p.to_array());
    let c = centre();
    let camera = commands
        .spawn((
            Name::new("Caravan camera"),
            Camera3d::default(),
            Camera { order: -4, clear_color: ClearColorConfig::Custom(Color::BLACK), ..default() },
            RenderTarget::Image(ImageRenderTarget::from(image.clone())),
            projection(view_w),
            Tonemapping::Linear,
            Msaa::Sample4,
            AmbientLight { color: Color::srgb(1.0, 0.95, 0.88), brightness: AMBIENT * crate::sky::light_unit(), ..default() },
            // Straight down, the table's +y up the picture.
            Transform::from_translation(to(c.extend(EYE_UP))).looking_at(to(c.extend(0.0)), nvf_play::space::to_bevy([0.0, 1.0, 0.0])),
            layer.clone(),
        ))
        .id();
    let lamp_at = (c + LAMP_AT).extend(LAMP_UP);
    let lamp = commands
        .spawn((
            SpotLight { color: Color::srgb(1.0, 0.9, 0.76), intensity: crate::lights::lumens(LAMP, crate::sky::light_unit()), range: LAMP_RANGE, radius: 40.0, shadow_maps_enabled: true, inner_angle: 0.5, outer_angle: 1.2, ..default() },
            Transform::from_translation(to(lamp_at)).looking_at(to(c.extend(0.0)), Vec3::Z),
            layer.clone(),
        ))
        .id();
    commands.insert_resource(CaravanView { image });
    info!("caravan view: the table built ({PLANKS}, normal map {}), view {view_w:.0} wide", if has_normal { "found" } else { "missing" });
    Some(Built { entities: vec![holder, camera, lamp], space, camera, planks, layer, card_mesh, cards: HashMap::new(), leaving: Vec::new(), materials: HashMap::new(), view_w, pot: None, money_pot: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cards_turn_is_small_and_fixed_by_its_key() {
        for key in ["c0:0:0", "c1:2:5:f1", "h0:0012ABCD:0", "d1"] {
            let a = jitter(key);
            assert!(a.abs() <= JITTER_DEG, "{key}: {a}");
            assert_eq!(a, jitter(key));
        }
        assert_ne!(jitter("c0:0:0"), jitter("c0:0:1"));
    }

    #[test]
    fn the_pot_grows_into_more_money_and_bigger_notes() {
        assert!(money(0).is_empty());
        let small = money(10);
        assert!(small.iter().all(|m| !m.kind.note()), "a small pot is caps");
        let mid = money(164);
        let big = money(2000);
        assert!(mid.len() > small.len() && big.len() > mid.len());
        assert!(mid.iter().any(|m| m.kind.note()));
        assert!(big.iter().any(|m| m.kind == Money::Note100));
        assert!(mid.iter().any(|m| m.kind == Money::Denarius), "Legion coins in the pot");
        assert!(big.iter().any(|m| m.kind == Money::Aureus));
        assert!(money(100_000).len() <= MONEY_MAX);
        // The pile only grows: a piece is where it was in the smaller pot.
        assert_eq!(&money(170)[..4].iter().map(|m| m.at).collect::<Vec<_>>(), &mid[..4].iter().map(|m| m.at).collect::<Vec<_>>());
        let mut zones = money_zones(0.0);
        zones.extend(text_zones(0.0));
        for Piece { kind, at, turn, .. } in &big {
            assert!(at.x > 0.0 && at.x < FRAME_W && at.y > 0.0 && at.y < 960.0);
            // (A cap or coin is a disc, a note a turned card.)
            let clear = if kind.note() { clear_of(&zones, *at, kind.size(), *turn) } else { disc_clear(&zones, *at, kind.size().x / 2.0) };
            assert!(clear, "{kind:?} at {at} lies where a card or the screen's words can");
        }
        assert!(big.len() > 40, "the big pot still fills the free planks: {}", big.len());
    }

    #[test]
    fn every_piece_lies_whole_on_the_screen() {
        let mut zones = money_zones(0.0);
        zones.extend(text_zones(0.0));
        for pot in [40, 164, 600, 2000, 10_434, 100_000] {
            for p in money(pot) {
                // A cap or coin is a disc, whatever its turn.
                let reach = if p.kind.note() {
                    let (s, c) = (p.turn.sin().abs(), p.turn.cos().abs());
                    let size = p.kind.size();
                    Vec2::new(c * size.x + s * size.y, s * size.x + c * size.y) / 2.0
                } else {
                    Vec2::splat(p.kind.size().x / 2.0)
                };
                if !p.kind.note() {
                    assert!(disc_clear(&zones, p.at, reach.x), "pot {pot}: {:?} at {} lies on a card's or word's place", p.kind, p.at);
                }
                assert!(p.at.x - reach.x >= NOTE_EDGE - 0.01 && p.at.x + reach.x <= FRAME_W - NOTE_EDGE + 0.01, "pot {pot}: {:?} at {} turned {} off the side", p.kind, p.at, p.turn);
                assert!(p.at.y - reach.y >= NOTE_EDGE - 0.01 && p.at.y + reach.y <= 960.0 - NOTE_EDGE + 0.01, "pot {pot}: {:?} at {} turned {} off the top or bottom", p.kind, p.at, p.turn);
            }
        }
    }

    #[test]
    fn a_bent_note_is_never_drawn_under_the_table() {
        let size = Money::Note20.size();
        for (z, tip) in [(0.7, 0.0), (1.5, 0.0), (3.0, 0.1), (6.0, 0.3), (40.0, 0.0), (2.0, -0.2)] {
            let rot = Quat::from_rotation_x(tip) * Quat::from_rotation_z(0.4);
            for bend in [-9.0, -4.0, 4.0, 9.0] {
                let b = bend_above_table(z, rot, size, 1.0, bend);
                assert!(b.abs() <= bend.abs() + 1e-4 && b * bend >= 0.0);
                let (pos, _) = note_shape(b, size.x);
                let low = pos.iter().map(|p| z + (rot * Vec3::new(p[0] * size.x, p[1] * size.y, p[2])).z).fold(f32::MAX, f32::min);
                let flat = pos.iter().map(|p| z + (rot * Vec3::new(p[0] * size.x, p[1] * size.y, 0.0)).z).fold(f32::MAX, f32::min);
                assert!(low >= NOTE_CLEAR - 1e-3 || low >= flat - 1e-3, "at {z} tipped {tip}, bend {bend} drawn as {b}: lowest {low}");
            }
        }
        // High up, the bend is left alone.
        assert_eq!(bend_above_table(40.0, Quat::IDENTITY, size, 1.0, 9.0), 9.0);
    }

    #[test]
    fn the_caps_and_coins_stack_in_heaps_and_rest_on_each_other() {
        for pot in [5, 20, 54, 108, 164, 400, 2000, 100_000] {
            let round: Vec<Piece> = money(pot).into_iter().filter(|m| !m.kind.note()).collect();
            // A cap or coin over another (they overlap) is a whole piece's height above it:
            // none goes through another.
            for (i, a) in round.iter().enumerate() {
                for b in &round[i + 1..] {
                    let reach = (a.kind.size().x + b.kind.size().x) / 2.0 * 0.95;
                    if a.at.distance(b.at) < reach {
                        assert!(b.lift >= a.lift + a.kind.thick() - 1e-3, "pot {pot}: {:?} at {} goes through {:?} at {}", b.kind, b.at, a.kind, a.at);
                    }
                }
                assert!(a.lean.length() <= LEAN_MAX + 1e-6);
            }
        }
        // A big pot heaps up: pieces rest on pieces, several high.
        let big: Vec<Piece> = money(2000).into_iter().filter(|m| !m.kind.note()).collect();
        let on_others = big.iter().filter(|m| m.lift > ROUND_BASE).count();
        assert!(on_others * 2 > big.len(), "{on_others} of {} rest on another", big.len());
        assert!(big.iter().any(|m| m.lift >= ROUND_BASE + 2.0 * CAP_THICK), "a heap three or more high");
    }

    #[test]
    fn the_frame_maps_onto_the_table_turned_over() {
        assert_eq!(on_table(Vec2::new(10.0, 20.0)), Vec2::new(10.0, -20.0));
        assert_eq!(centre(), Vec2::new(FRAME_W / 2.0, -480.0));
        // A view narrower than 16:9 shows the whole frame, smaller.
        assert!(frame_scale(FRAME_W * 0.8) < 1.0);
        assert_eq!(frame_scale(FRAME_W * 1.2), 1.0);
    }
}
