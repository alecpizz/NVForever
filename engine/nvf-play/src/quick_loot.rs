//! **Quick Loot** [design, #2933, docs/design/quick-loot.md]: a container, a body
//! or (sneaking) a person under the crosshair shows what it holds in a small list beside the
//! crosshair, without the menu that stops the game.
//!
//! - **What shows it**: the activation target (`crate::activate::Target`) is a container
//!   that opens (`Action::Open` on a CONT; a locked one keeps its lock prompt), a body or ash
//!   pile to search (`Action::Search`) or a pocket the player may pick (`Action::Steal`
//!   with no refusal, `fnv_game::Game::pickpocket_refusal`), and the NVF Features row Quick
//!   Loot is on (off by default). Not for a reference whose script has an OnActivate block,
//!   a reference set off by its activate parents only or one that cannot be activated: the
//!   game's own activation decides those, so their prompt stays as it is.
//! - **Keys** (the player's bindings): Activate (E) takes the selected stack, Ready Item (R)
//!   takes everything (not from a pocket: each item is its own roll), Wait (T) opens the
//!   full container or pickpocket menu as Activate would without Quick Loot, and the mouse
//!   wheel moves the selection [design, #2933: fifth look, R and T swapped]. While the list shows, those keys do nothing else:
//!   no reload, no Wait menu, no camera zoom ([`holds_input`]).
//! - **The rules are the container menu's** (`crate::menu::model::MenuModel`, opened on the
//!   target as the menu opens it, its pickpocket rolls included): a take from someone
//!   else's container is theft (`crate::crime::ItemsTaken`), a pocket's take rolls its
//!   chance and a failure is caught (`crate::pickpocket::Pickpocketed`).
//! - **Drawn** in the HUD colour (`crate::interface::amber`, recoloured with it), the crime
//!   colour for theft, at its own scale (the Quick Loot section's Scale row, 50-100%, not the HUD's), in the font the
//!   Menu Font row picks (`crate::pause::MenuFont`: the game's font 7 or the sharp one).
//!   Columns for weight, value and value per weight each have a row of their own (off by
//!   default); with any column (or a pocket's chance) the count goes beside the name, else
//!   it is right-aligned in a column of its own [design, #2933].
//! - **Logs** (smoke): `quick loot: shown on 0x… "<name>" (<kind>, <n> rows)`,
//!   `quick loot: took …`, `quick loot: the full menu`, and per drawing `quick loot: drawn
//!   "<name>" … font Original|Sharp …: no overlaps` (or what overlaps, [`overlaps`]).

use crate::activate::outcome::Action;
use crate::activate::Target;
use crate::bindings::Control;
use crate::interface::{amber, at, bc_supported, set_line, Face};
use crate::menu::model::{Command, MenuModel, Row, Side};
use crate::start_menu::Faces;
use crate::sim::SimState;
use crate::{PlayState, RefChanged};
use bevy::image::CompressedImageFormatSupport;
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use fnv_sim::forms::BaseData;
use fnv_sim::FormId;
use fnv_game::game::event;
use fnv_util::words::tr;
use nvf_assets::GameData;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

// ---- the settings -----------------------------------------------------------------------

static ON: AtomicBool = AtomicBool::new(false);
static WEIGHT: AtomicBool = AtomicBool::new(false);
static VALUE: AtomicBool = AtomicBool::new(false);
static PER_WEIGHT: AtomicBool = AtomicBool::new(false);
/// The backing's darkness (the Background Opacity row, 0 to 1), the bits of an `f32`: 0.7 by
/// default, the darkness chosen after the first build [design, #2933: third
/// look].
static BACKING: AtomicU32 = AtomicU32::new(0x3F33_3333);

/// The backing's darkness, 0 to 1.
pub fn backing() -> f32 {
    f32::from_bits(BACKING.load(Ordering::Relaxed)).clamp(0.0, 1.0)
}

/// The list's own size (the Quick Loot section's Scale row), the bits of an `f32`.
static SCALE: AtomicU32 = AtomicU32::new(0x3F80_0000);

/// The Quick Loot section's Scale row's choices: 50% to 100% in steps of 10%, the list's own size,
/// independent of the HUD scale [design, #2933: second look].
pub const SCALES: [f32; 6] = [0.5, 0.6, 0.7, 0.8, 0.9, 1.0];

/// The size the list is drawn at.
pub fn scale() -> f32 {
    f32::from_bits(SCALE.load(Ordering::Relaxed))
}

fn scale_names() -> Vec<String> {
    SCALES.iter().map(|s| format!("{:.0}%", s * 100.0)).collect()
}

fn scale_chosen(_: &World) -> String {
    scale_names()[crate::settings::nearest(&SCALES, scale())].clone()
}

fn take_scale(_: &mut World, text: &str) -> bool {
    match crate::settings::choice_of(text, &scale_names(), |v| crate::settings::nearest(&SCALES, if v > 1.5 { v / 100.0 } else { v })) {
        Some(i) => {
            SCALE.store(SCALES[i].to_bits(), Ordering::Relaxed);
            true
        }
        None => false,
    }
}
/// The list shows and has its keys this frame ([`holds_input`]).
static HOLDS: AtomicBool = AtomicBool::new(false);
/// The list took something this frame: `crate::world_sounds` sounds the player's inventory
/// changes as the container menu's takes only then ([`took_this_frame`]), not a shot's
/// spent ammo or a hotkeyed Stimpak while the list shows.
static TOOK: AtomicBool = AtomicBool::new(false);
/// Wait asked for the full menu: `crate::activate::activate` activates the target as
/// Activate would ([`take_full_request`]).
static FULL: AtomicBool = AtomicBool::new(false);

/// Whether the Quick Loot list shows and has Activate, Wait, Ready Item and the mouse wheel:
/// the systems that read those leave them alone while it does (set in `PlaySet::Activation`;
/// the input systems before it see the last frame's).
pub fn holds_input() -> bool {
    HOLDS.load(Ordering::Relaxed)
}

/// The list took something since the last call (read once, by `crate::world_sounds`).
pub fn took_this_frame() -> bool {
    TOOK.swap(false, Ordering::Relaxed)
}

/// Wait asked for the full menu this frame (read once).
pub fn take_full_request() -> bool {
    FULL.swap(false, Ordering::Relaxed)
}

/// Which columns are on (the rows' switches) and whether a pocket's chance shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Columns {
    pub weight: bool,
    pub value: bool,
    pub per_weight: bool,
    pub chance: bool,
}

impl Columns {
    pub fn any(&self) -> bool {
        self.weight || self.value || self.per_weight || self.chance
    }

    fn chosen(chance: bool) -> Columns {
        Columns { weight: WEIGHT.load(Ordering::Relaxed), value: VALUE.load(Ordering::Relaxed), per_weight: PER_WEIGHT.load(Ordering::Relaxed), chance }
    }
}

// ---- what is shown ----------------------------------------------------------------------

/// What the list is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Container,
    Body,
    Pocket,
}

/// Rows shown at once; the list scrolls to keep the selection in them [design, #2933:
/// design A].
pub const VISIBLE: usize = 6;

/// The list on one target.
#[derive(Debug, Clone)]
pub struct Shown {
    /// The reference under the crosshair, and the one whose items these are (a body's for
    /// its ash pile).
    pub target: u32,
    pub holder: FormId,
    pub kind: Kind,
    /// Taking is theft (someone else's container, any pocket).
    pub crime: bool,
    pub title: String,
    pub model: MenuModel,
    /// Seconds since the rows were read from the simulation.
    since_read: f32,
}

impl Shown {
    pub fn rows(&self) -> &[Row] {
        &self.model.list(Side::Container).rows
    }

    pub fn selected(&self) -> usize {
        self.model.selected[Side::Container as usize].min(self.rows().len().saturating_sub(1))
    }

    /// The first row shown: the selection kept two rows from the top where it can be.
    pub fn first(&self) -> usize {
        let n = self.rows().len();
        self.selected().saturating_sub(2).min(n.saturating_sub(VISIBLE))
    }
}

#[derive(Resource, Default)]
pub struct QuickLoot {
    pub shown: Option<Shown>,
    /// The target a caught pickpocketing closed the list on: not shown on it again until
    /// the crosshair leaves it.
    closed_on: Option<u32>,
}

/// The rows' words: what [`layout`] lays out.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub title: String,
    /// "3/8": the selection's place and the rows.
    pub position: String,
    pub rows: Vec<RowSpec>,
    pub more_up: bool,
    pub more_down: bool,
    pub columns: Columns,
    /// The keys' lines ("[E] Take").
    pub prompts: Vec<String>,
    pub crime: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RowSpec {
    pub name: String,
    pub count: i32,
    pub weight: Option<f32>,
    pub value: Option<i32>,
    pub chance: Option<i32>,
    pub selected: bool,
}

/// What an empty list says [design].
pub const EMPTY: &str = "Empty";
const TAKE: &str = "Take";
const STEAL: &str = "Steal";
const TAKE_ALL: &str = "Take All";
const STEAL_ALL: &str = "Steal All";
const SEARCH: &str = "Search";
/// The translation key for [`SEARCH`] in the sense of searching a body or container: the
/// settings page's search box shares the English word but not its translation (fr
/// "Rechercher" looks up a text, "Fouiller" searches a body), so the list has its own key
/// and shows the English word where no table has it (review of #2935).
const SEARCH_KEY: &str = "Search (a body or container)";

fn search_word() -> &'static str {
    let w = tr(SEARCH_KEY);
    if w == SEARCH_KEY {
        SEARCH
    } else {
        w
    }
}
/// The names' head, on the columns' heads' line [design, #2933: sixth look].
const ITEM: &str = "ITEM";
/// The weight and value columns' heads: NVForever's own words (WT, an English abbreviation the
/// original does not show, to keep the box inside the HUD with every column) [design, #4495:
/// in place of the abbreviations read from the original's screen].
const WG: &str = "WT";
const VAL: &str = "VALUE";
const PER_WG: &str = "V/W";
const CHANCE: &str = "CHANCE";

impl Spec {
    pub fn of(s: &Shown, columns: Columns, key: impl Fn(Control) -> String) -> Spec {
        let rows = s.rows();
        let (first, sel) = (s.first(), s.selected());
        let chance_of = |r: &Row| s.model.steal.as_ref().map(|st| st.chance(r, r.count));
        let shown: Vec<RowSpec> = rows
            .iter()
            .enumerate()
            .skip(first)
            .take(VISIBLE)
            .map(|(i, r)| RowSpec { name: r.name.clone(), count: r.count, weight: r.weight, value: r.value, chance: chance_of(r), selected: i == sel })
            .collect();
        let steal = s.crime;
        // NVForever's own words in the chosen language (`lang/*.toml`, `fnv_util::words`).
        let mut prompts = vec![format!("{} {}", key(Control::Activate), tr(if steal { STEAL } else { TAKE }))];
        if s.kind != Kind::Pocket {
            prompts.push(format!("{} {}", key(Control::ReadyItem), if steal { tr(STEAL_ALL) } else { crate::pipboy::labels::label("sTakeAll", tr(TAKE_ALL)) }));
        }
        prompts.push(format!("{} {}", key(Control::Rest), search_word()));
        Spec {
            title: s.title.clone(),
            position: if rows.is_empty() { "0/0".into() } else { format!("{}/{}", sel + 1, rows.len()) },
            more_up: first > 0,
            more_down: first + VISIBLE < rows.len(),
            rows: shown,
            columns,
            prompts,
            crime: steal,
        }
    }
}

// ---- layout -----------------------------------------------------------------------------

/// The list's measures, interface units (960 to the view's height) [design, #2933:
/// design A, and the first look at it]: its left edge right
/// of the crosshair, its box centred on the crosshair's height.
pub const LEFT: f32 = 150.0;
/// Every line of words is one height, the rows' [design, #2933: second look, "all
/// text lines ... should be the same height even if their text size is different"]: the
/// name's, the columns' heads', each row's and the keys'; the words' capitals centred in it
/// ([`centred`]).
pub const LINE: f32 = 40.0;
pub const ROW_H: f32 = LINE;
/// Between the heading's lines and the rule, and round the rows (where the bars for more
/// rows stand).
const GAP_Y: f32 = 6.0;
const RULE: f32 = 1.5;
const ROW_GAP: f32 = 2.0;
/// The box's edge to the rows' box, and the rows' box to their words: the name, the heading
/// and the keys under the box start on one line [design, #2933: step 1]. The rows' box stands as
/// far in from the box's sides as the last row from its bottom, the frame and [`GAP_Y`] [design,
/// #2933: fourth look, the selection's box with the same room beside it as under
/// it].
const PAD_X: f32 = FRAME + GAP_Y;
const CELL: f32 = 10.0;
/// Between columns [design, #2933: step 6, wider than the first build's 22].
pub const GAP: f32 = 40.0;
const NAME_MIN: f32 = 200.0;
const PROMPT_GAP: f32 = 4.0;
const PROMPT_SPACE: f32 = 30.0;
const FRAME: f32 = 3.0;
const TICK: f32 = 14.0;

/// The faces the list is drawn in: the name's (title, bold in the sharp font), the columns'
/// heads' and the place's ("1/4": head, bold in the sharp font) and the rows' words, figures
/// and the keys' line (small, regular, the heads' size) [design, #2933: steps 4 and 4.5; the item
/// names at the figures' size, the third look; the keys at theirs, the fifth]. Entry is the
/// game's row face, kept for its measures.
#[derive(Clone)]
pub struct Fonts {
    pub entry: Face,
    pub title: Face,
    pub head: Face,
    pub small: Face,
}

impl Fonts {
    /// The game's faces as they are: font 7, and font 3 for the heads and figures (it has no
    /// bold).
    pub fn raster(f: &Faces) -> Fonts {
        Fonts { entry: f.entry.clone(), title: f.title.clone(), head: f.small.clone(), small: f.small.clone() }
    }

    /// The sharp font's (`crate::vector_font`, its metrics `m`): `f` as the Menu Font row
    /// makes it sharp, and the heads bold at the small size.
    pub fn sharp(f: &Faces, m: &crate::vector_font::Metrics, font: Handle<bevy::text::Font>) -> Fonts {
        use crate::vector_font::{face, BOLD, SMALL_EM};
        let s = f.sharp(m, font.clone());
        Fonts { head: face(m, font, SMALL_EM, BOLD, f.small.font.size), entry: s.entry, title: s.title, small: s.small }
    }
}

/// Which of the faces a text is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    Entry,
    Title,
    Head,
    Small,
}

/// How a text or a bar is coloured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    /// The HUD colour, full.
    Hud,
    /// The columns' heads (ITEM, WT …): the HUD colour at 50% [design, #2933: sixth
    /// look, "not that visible"].
    Head,
    /// The HUD colour, dim.
    Dim,
    /// A row's text: the crime colour for theft, else the HUD's; full when selected.
    Row(bool),
    /// The selection's fill under its row.
    Fill,
    /// The dark backing.
    Backing,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub text: String,
    pub which: Which,
    pub x: f32,
    pub base: f32,
    pub width: f32,
    pub ink: Ink,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    pub rect: Rect,
    pub ink: Ink,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Laid {
    pub size: Vec2,
    /// The framed box's height (the keys are under it).
    pub box_h: f32,
    pub pieces: Vec<Piece>,
    pub bars: Vec<Bar>,
    /// Each row's band (the selection's box), top to bottom.
    pub bands: Vec<Rect>,
    /// The top of the rule under the name.
    pub rule: f32,
    /// Every line of words ([`LINE`] high): the name's, the heads', the rows', the keys'.
    pub lines: Vec<Rect>,
}

fn face(fonts: &Fonts, w: Which) -> &Face {
    match w {
        Which::Entry => &fonts.entry,
        Which::Title => &fonts.title,
        Which::Head => &fonts.head,
        Which::Small => &fonts.small,
    }
}

/// How wide a text is drawn: its advances, or further where the last glyph's picture reaches
/// past its advance (the game's font 7 has glyphs with room after them less than nothing), so
/// right edges and the room after the keys' line are where the ink is [design, #2933: step 2].
pub fn ink_width(f: &Face, text: &str) -> f32 {
    f.font.lines(text, None).first().map_or(0.0, |l| {
        l.glyphs.iter().map(|p| {
            let g = f.font.glyph_of(p.ch);
            p.x + g.before + g.width
        })
        .fold(l.width, f32::max)
    })
}

/// The baseline that centres a face's capitals in a line from `top`, [`LINE`] high.
pub fn centred(f: &Face, top: f32) -> f32 {
    top + (LINE + cap(f)) / 2.0
}

/// How tall a face's capitals are drawn above the line: the inked part of `H`. A game font's
/// glyph picture has room round its letter for the glow, as much over it as under the line
/// (`H` has nothing under the line), so the ink stands `2 * ascent - height` high
/// [observed: the first build's screenshots, the words a few pixels low in their rows]; the
/// sharp font's metrics have none. Half the line for a stand-in font without measures.
pub fn cap(f: &Face) -> f32 {
    let g = f.font.glyph(b'H');
    let ink = 2.0 * g.ascent - g.height;
    if ink > 0.0 && g.height > 0.0 {
        ink
    } else if g.ascent > 0.0 {
        g.ascent
    } else {
        f.font.size * 0.5
    }
}

/// A value per unit of weight as written: whole above 10, else one place; "--" for nothing
/// carried or nothing worth [design].
pub fn per_weight_text(weight: Option<f32>, value: Option<i32>) -> String {
    match (weight.filter(|w| *w > 0.0), value.filter(|v| *v > 0)) {
        (Some(w), Some(v)) => {
            let r = v as f32 / w;
            if r >= 10.0 {
                format!("{}", r.round() as i64)
            } else {
                format!("{r:.1}")
            }
        }
        _ => "--".into(),
    }
}

/// The widest the list may be from [`LEFT`] in a view `half` units from its middle to its
/// right edge, `edge` units in from which the HUD's bottom right pieces end
/// (`crate::hud::RIGHT_EDGE_IN` at the HUD's scale): its right edge on theirs [design,
/// #2933: step 10]. Both in the list's own units.
pub fn widest(half: f32, edge: f32) -> f32 {
    half - edge - LEFT
}

/// The scale at which a list `width` units wide at `size` fits a room `max_w` units wide at
/// `size`: the room (`widest`) grows as the scale shrinks, the list's width does not.
pub fn fitted_scale(max_w: f32, width: f32, size: f32) -> f32 {
    ((max_w + LEFT) * size / (width + LEFT)).max(0.1)
}

/// The list laid out from its top left corner, measured by `fonts` (the drawn faces), at
/// most `max_w` wide: the names' room gives way (cut with "...") before the list passes it.
pub fn layout(spec: &Spec, fonts: &Fonts, max_w: f32) -> Laid {
    layout_on(spec, fonts, max_w, 0.0)
}

/// [`layout`] with the bars' edges on whole pixels `px` units wide (0: no snapping), so
/// rooms the same in units are the same on screen: the frame's sides and its bottom stand as
/// far from the selection's box [design, #2933: fifth look, the bottom line a pixel
/// closer than the left one, each edge rounded on its own].
pub fn layout_on(spec: &Spec, fonts: &Fonts, max_w: f32, px: f32) -> Laid {
    // A size in whole pixels (at least one), and a place on the nearest pixel.
    let snap = |v: f32| if px > 0.0 { (v / px).round().max(1.0) * px } else { v };
    let on_pixel = |v: f32| if px > 0.0 { (v / px).round() * px } else { v };
    // The box's width up to the next pixel, never down: rounded down, a name that just fits
    // was cut, its count with it [design, #2933: sixth look, "Sunset Sarsaparilla (..."].
    let wide = |v: f32| if px > 0.0 { (v / px - 1e-3).ceil() * px } else { v };
    let (frame, gap_y, half_gap, line_h, row_h, row_gap, rule_h, tick) = (snap(FRAME), snap(GAP_Y), snap(GAP_Y / 2.0), snap(LINE), snap(ROW_H), snap(ROW_GAP), snap(RULE), snap(TICK));
    let pad_x = frame + gap_y;
    let width = |w: Which, s: &str| ink_width(face(fonts, w), s);
    let c = spec.columns;
    let counted = !c.any();
    // Each row's words: the name (with its count where columns show), the count, the columns.
    let label = |r: &RowSpec| if !counted && r.count > 1 { format!("{} ({})", r.name, r.count) } else { r.name.clone() };
    let count = |r: &RowSpec| if counted && r.count > 1 { format!("({})", r.count) } else { String::new() };
    let mut cols: Vec<(&str, Vec<String>)> = Vec::new();
    if c.weight {
        cols.push((tr(WG), spec.rows.iter().map(|r| crate::reticle::weight_text(r.weight)).collect()));
    }
    if c.value {
        cols.push((tr(VAL), spec.rows.iter().map(|r| crate::reticle::value_text(r.value)).collect()));
    }
    if c.per_weight {
        cols.push((tr(PER_WG), spec.rows.iter().map(|r| per_weight_text(r.weight, r.value)).collect()));
    }
    if c.chance {
        cols.push((tr(CHANCE), spec.rows.iter().map(|r| r.chance.map_or_else(|| "--".into(), |p| format!("{p}%"))).collect()));
    }
    let col_w: Vec<f32> = cols.iter().map(|(h, cells)| cells.iter().map(|t| width(Which::Small, t)).fold(width(Which::Head, h), f32::max)).collect();
    let count_w = spec.rows.iter().map(|r| width(Which::Small, &count(r))).fold(0.0, f32::max);
    let item_w = if cols.is_empty() { 0.0 } else { width(Which::Head, tr(ITEM)) };
    let names = spec.rows.iter().map(|r| width(Which::Small, &label(r))).fold(width(Which::Small, tr(EMPTY)).max(item_w), f32::max);
    let x0 = pad_x + CELL;
    // The box is at least as wide as the keys' line with the words' room on both sides [design,
    // #2933: step 2 and third look].
    let prompts_w = spec.prompts.iter().map(|p| width(Which::Small, p)).sum::<f32>() + PROMPT_SPACE * spec.prompts.len().saturating_sub(1) as f32;
    let position_w = width(Which::Head, &spec.position);
    let title_w = width(Which::Title, &spec.title);
    let others = col_w.iter().map(|w| GAP + w).sum::<f32>() + if count_w > 0.0 { GAP + count_w } else { 0.0 };
    let wanted = (2.0 * x0 + names.max(NAME_MIN) + others).max(2.0 * x0 + prompts_w).max(2.0 * x0 + NAME_MIN.max(title_w) + GAP + position_w);
    let w = wide(wanted.min(max_w.max(2.0 * x0 + NAME_MIN + others).max(2.0 * x0 + prompts_w)));
    let x1 = w - x0;
    let mut pieces = Vec::new();
    let mut bars = Vec::new();
    let mut bands = Vec::new();
    let mut piece = |text: String, which: Which, x: f32, base: f32, ink: Ink| {
        let width = width(which, &text);
        pieces.push(Piece { text, which, x, base, width, ink });
    };
    let mut lines = Vec::new();
    let mut line = |top: f32| lines.push(Rect::new(0.0, top, w, top + LINE));
    // The heading: the name and its place in the rows ("1/4") on one line; the rule; the
    // columns' heads on a line of their own.
    let mut y = frame + half_gap;
    line(y);
    let title = crate::pause::fit_text(|t| width(Which::Title, t), &spec.title, x1 - x0 - GAP - position_w);
    piece(title, Which::Title, x0, centred(&fonts.title, y), Ink::Hud);
    piece(spec.position.clone(), Which::Head, x1 - position_w, centred(&fonts.head, y), Ink::Hud);
    y += line_h + half_gap;
    let rule = y;
    bars.push(Bar { rect: Rect::new(pad_x, rule, w - pad_x, rule + rule_h), ink: Ink::Dim });
    y += rule_h + half_gap;
    // The columns right-aligned at their ends, the last at the rows' right.
    let mut col_end = Vec::new();
    let mut end = x1;
    for cw in col_w.iter().rev() {
        col_end.push(end);
        end -= cw + GAP;
    }
    col_end.reverse();
    let count_end = end;
    if !cols.is_empty() {
        line(y);
        // The names' head where the names start [design, #2933: sixth look].
        piece(tr(ITEM).to_string(), Which::Head, x0, centred(&fonts.head, y), Ink::Head);
        for ((h, _), e) in cols.iter().zip(&col_end) {
            piece((*h).to_string(), Which::Head, e - width(Which::Head, h), centred(&fonts.head, y), Ink::Head);
        }
        y += line_h;
    }
    y += gap_y;
    // More rows above or below: a short bar in the room before or after them.
    let more = |y: f32| {
        let (x, t) = (on_pixel(w / 2.0 - 16.0), on_pixel(y - 1.5));
        Bar { rect: Rect::new(x, t, x + snap(32.0), t + snap(3.0)), ink: Ink::Dim }
    };
    if spec.more_up {
        bars.push(more(y - GAP_Y / 2.0));
    }
    let name_end = if count_w > 0.0 { count_end - count_w - GAP } else { count_end };
    for (i, r) in spec.rows.iter().enumerate() {
        let band = Rect::new(pad_x, y, w - pad_x, y + row_h);
        bands.push(band);
        line(y);
        if r.selected {
            bars.push(Bar { rect: band, ink: Ink::Fill });
            let edge = |rect: Rect| Bar { rect, ink: Ink::Row(true) };
            bars.push(edge(Rect::new(band.min.x, band.min.y, band.max.x, band.min.y + rule_h)));
            bars.push(edge(Rect::new(band.min.x, band.max.y - rule_h, band.max.x, band.max.y)));
            bars.push(edge(Rect::new(band.min.x, band.min.y, band.min.x + rule_h, band.max.y)));
            bars.push(edge(Rect::new(band.max.x - rule_h, band.min.y, band.max.x, band.max.y)));
        }
        let ink = Ink::Row(r.selected);
        // A name too long for its room gives way before its count, which always shows
        // [design, #2933: sixth look].
        let name = if !counted && r.count > 1 {
            let tail = format!(" ({})", r.count);
            let cut = crate::pause::fit_text(|t| width(Which::Small, &format!("{t}{tail}")), &r.name, name_end - x0);
            format!("{cut}{tail}")
        } else {
            crate::pause::fit_text(|t| width(Which::Small, t), &label(r), name_end - x0)
        };
        piece(name, Which::Small, x0, centred(&fonts.small, y), ink);
        let n = count(r);
        if !n.is_empty() {
            let nw = width(Which::Small, &n);
            piece(n, Which::Small, count_end - nw, centred(&fonts.small, y), ink);
        }
        for ((_, cells), e) in cols.iter().zip(&col_end) {
            let t = cells[i].clone();
            let tw = width(Which::Small, &t);
            piece(t, Which::Small, e - tw, centred(&fonts.small, y), ink);
        }
        y += row_h + row_gap;
    }
    if spec.rows.is_empty() {
        bands.push(Rect::new(pad_x, y, w - pad_x, y + row_h));
        line(y);
        piece(tr(EMPTY).into(), Which::Small, x0, centred(&fonts.small, y), Ink::Dim);
        y += row_h + row_gap;
    }
    y += gap_y - row_gap;
    if spec.more_down {
        bars.push(more(y - GAP_Y / 2.0));
    }
    let box_h = y + frame;
    // The frame: the backing, top and bottom lines with ticks down and up at their ends.
    let mut frame_bars = vec![
        Bar { rect: Rect::new(0.0, 0.0, w, box_h), ink: Ink::Backing },
        Bar { rect: Rect::new(0.0, 0.0, w, frame), ink: Ink::Hud },
        Bar { rect: Rect::new(0.0, box_h - frame, w, box_h), ink: Ink::Hud },
    ];
    for (x, y0) in [(0.0, 0.0), (w - frame, 0.0), (0.0, box_h - tick), (w - frame, box_h - tick)] {
        frame_bars.push(Bar { rect: Rect::new(x, y0, x + frame, y0 + tick), ink: Ink::Hud });
    }
    frame_bars.extend(bars);
    // The keys under the box, centred on it; the box at least as wide as they are with the
    // words' room before them on both sides [design, #2933: third look].
    let top = box_h + PROMPT_GAP;
    line(top);
    let mut x = (w - prompts_w) / 2.0;
    for (i, p) in spec.prompts.iter().enumerate() {
        let ink = if i == 0 || (i == 1 && spec.prompts.len() == 3) { Ink::Row(true) } else { Ink::Hud };
        piece(p.clone(), Which::Small, x, centred(&fonts.small, top), ink);
        x += width(Which::Small, p) + PROMPT_SPACE;
    }
    Laid { size: Vec2::new(w, top + LINE), box_h, pieces, bars: frame_bars, bands, rule, lines }
}

/// A text's box: from the top of its capitals to a little under its baseline.
pub fn text_box(p: &Piece, fonts: &Fonts) -> Rect {
    let c = cap(face(fonts, p.which));
    Rect::new(p.x, p.base - c, p.x + p.width, p.base + c * 0.3)
}

/// What overlaps in a laid-out list, as words for the log: two texts whose boxes meet, a
/// text outside the box or across the rule, a row's text off its row [design, #2933:
/// "no elements should overlap"].
pub fn overlaps(l: &Laid, fonts: &Fonts) -> Vec<String> {
    let mut out = Vec::new();
    let boxes: Vec<Rect> = l.pieces.iter().map(|p| text_box(p, fonts)).collect();
    for i in 0..boxes.len() {
        for j in i + 1..boxes.len() {
            let (a, b) = (boxes[i], boxes[j]);
            if a.width() > 0.0 && b.width() > 0.0 && !a.intersect(b).is_empty() {
                out.push(format!("\"{}\" and \"{}\"", l.pieces[i].text, l.pieces[j].text));
            }
        }
        let p = &l.pieces[i];
        let b = boxes[i];
        let inside = if p.base < l.box_h { Rect::new(PAD_X, FRAME, l.size.x - PAD_X, l.box_h - FRAME) } else { Rect::new(0.0, l.box_h, l.size.x, l.size.y) };
        if b.width() > 0.0 && (b.min.x < inside.min.x - 0.01 || b.max.x > inside.max.x + 0.01 || b.min.y < inside.min.y - 0.01 || b.max.y > inside.max.y + 0.01) {
            out.push(format!("\"{}\" outside the list", p.text));
        }
        if p.base < l.box_h && b.min.y < l.rule + RULE && b.max.y > l.rule {
            out.push(format!("\"{}\" across the rule", p.text));
        }
        if matches!(p.ink, Ink::Row(_)) && p.base < l.box_h && !l.bands.iter().any(|band| p.base > band.min.y && p.base < band.max.y) {
            out.push(format!("\"{}\" off its row", p.text));
        }
    }
    out
}

// ---- following the target ---------------------------------------------------------------

/// How often the rows are read again while the list stays on one target (a script or a
/// companion may change what it holds), seconds [design].
const REREAD: f32 = 0.5;

/// The list's kind for an activation `action`, before the target's own checks.
fn kind_of(action: &Action, container: bool) -> Option<Kind> {
    match action {
        Action::Search => Some(Kind::Body),
        Action::Open if container => Some(Kind::Container),
        Action::Steal => Some(Kind::Pocket),
        _ => None,
    }
}

/// The list's kind for target `t`, or none; `container`: its base is a container.
fn wanted(t: &crate::activate::Targeted, container: bool, g: &mut fnv_game::Game) -> Option<Kind> {
    let kind = kind_of(&t.action, container)?;
    let id = FormId(t.form_id);
    // The game's own activation decides these (module notes), pockets too: a refused or
    // parent-only reference, or one whose OnActivate block runs instead of the default
    // action, is never stolen from by hand (`script.rs` `activate_by_hand`).
    if !by_hand_default(id, g) {
        return None;
    }
    if kind == Kind::Pocket && g.pickpocket_refusal(id).is_some() {
        return None;
    }
    Some(kind)
}

/// Whether activating `id` by hand takes the default action: not refused, not parent-only,
/// and no OnActivate block on its script.
fn by_hand_default(id: FormId, g: &fnv_game::Game) -> bool {
    if g.activation_refused(id) || g.parent_activate_only(id) {
        return false;
    }
    !g.ref_script(id).and_then(|s| g.data.script(s)).is_some_and(|u| u.has_block(event::ON_ACTIVATE))
}

fn open(t: &crate::activate::Targeted, kind: Kind, sim: &mut SimState, rt: &mut crate::script::Runtime) -> Option<Shown> {
    let id = FormId(t.form_id);
    let holder = match kind {
        Kind::Body => rt.game.ash_pile_source(id).unwrap_or(id),
        _ => id,
    };
    let opened = match kind {
        Kind::Pocket => {
            let (rules, (thief, target), worn, overhaul) = crate::script::with_world(rt, sim, |rt| (rt.game.pickpocket_settings(), rt.game.pickpocket_skills(id), rt.game.pickpocket_worn(id), rt.game.pickpocket_overhaul(id)));
            MenuModel::open_container(&mut sim.world, id).and_then(|mut m| {
                let mut steal = crate::menu::model::Steal::new(id, rules, thief, target, crate::menu::model::steal_seed_now(id));
                steal.worn = worn;
                steal.overhaul = overhaul;
                m.steal = Some(steal);
                m.refresh(&mut sim.world)?;
                Ok(m)
            })
        }
        _ => MenuModel::open_container(&mut sim.world, holder),
    };
    match opened {
        Ok(model) => Some(Shown { target: t.form_id, holder, kind, crime: t.crime, title: t.name.clone(), model, since_read: 0.0 }),
        Err(e) => {
            warn!("quick loot: 0x{:08X} {}: cannot be listed: {e}", t.form_id, t.name);
            None
        }
    }
}

type Overlays<'w> = (
    Option<Res<'w, crate::weapon_wheel::WeaponWheel>>,
    Option<Res<'w, crate::vats_mode::VatsMode>>,
    Option<Res<'w, crate::creator::CreatorMenu>>,
    Option<Res<'w, crate::player::ViewDriven>>,
    Option<Res<'w, crate::kill_cam::KillCam>>,
    Option<Res<'w, crate::control::ControlLocks>>,
    Option<Res<'w, crate::hud::HudHidden>>,
);

type Out<'w> = (
    MessageWriter<'w, crate::crime::ItemsTaken>,
    MessageWriter<'w, crate::pickpocket::Pickpocketed>,
    MessageWriter<'w, RefChanged>,
    MessageWriter<'w, crate::ui_sounds::UiSound>,
);

/// Follows the target, reads the keys: in `PlaySet::Activation`, after the target is found
/// and before `crate::activate::activate` reads Activate.
#[allow(clippy::too_many_arguments)]
pub(crate) fn follow(
    input: crate::bindings::ControlInput,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    target: Res<Target>,
    mut ql: ResMut<QuickLoot>,
    mut sim: ResMut<SimState>,
    script: Option<ResMut<crate::script::ScriptState>>,
    (wheel, vats, creator, driven, kill_cam, locks, hud_hidden): Overlays,
    (mut taken, mut stolen, mut changed, mut sounds): Out,
    mut corner: Option<ResMut<crate::corner_messages::CornerMessages>>,
) {
    let mut script = script;
    let busy = wheel.is_some_and(|w| w.holds_input())
        || vats.is_some_and(|v| v.holds_input() || v.selected().is_some() || v.playing_back())
        || creator.is_some_and(|c| c.is_open())
        || driven.is_some_and(|d| d.0)
        || kill_cam.is_some_and(|k| k.active())
        || locks.is_some_and(|l| l.rollover || l.movement)
        // Not drawn, so not holding the keys either (module notes).
        || hud_hidden.is_some_and(|h| h.on);
    let t = target.0.as_ref().filter(|_| ON.load(Ordering::Relaxed) && !busy);
    if ql.closed_on.is_some() && t.map(|t| t.form_id) != ql.closed_on {
        ql.closed_on = None;
    }
    let t = t.filter(|t| Some(t.form_id) != ql.closed_on);
    // The list on the target: kept while it stays what it was, opened anew on another or when
    // the same reference's action changed (a pocket whose owner stood up, died or was
    // spotted; a knocked-out body that woke) or its theft did.
    let same = matches!((t, ql.shown.as_ref()), (Some(t), Some(s)) if s.target == t.form_id && s.crime == t.crime && kind_of(&t.action, s.kind == Kind::Container) == Some(s.kind));
    if !same {
        let container = t.is_some_and(|t| sim.base_of(t.form_id).and_then(|b| sim.forms.get(b)).is_some_and(|b| matches!(b.data, BaseData::Container(_))));
        let opened = match (t, script.as_mut().and_then(|s| s.rt.as_mut())) {
            (Some(t), Some(rt)) => crate::script::with_world(rt, &mut sim, |rt| wanted(t, container, &mut rt.game)).and_then(|kind| open(t, kind, &mut sim, rt)),
            _ => None,
        };
        if let Some(s) = &opened {
            info!("quick loot: shown on 0x{:08X} \"{}\" ({:?}{}, {} rows)", s.target, s.title, s.kind, if s.crime { ", theft" } else { "" }, s.rows().len());
        } else if let Some(s) = &ql.shown {
            info!("quick loot: hidden (was on 0x{:08X} \"{}\")", s.target, s.title);
        }
        ql.shown = opened;
    }
    let shown_now = ql.shown.is_some();
    HOLDS.store(shown_now, Ordering::Relaxed);
    let Some(s) = ql.shown.as_mut() else { return };
    // The frame a list opens, its keys are not read: the systems that read them before
    // `PlaySet::Activation` (Ready Item, the wheel) saw no list and may have used them.
    if !same {
        return;
    }
    s.since_read += time.delta_secs();
    if s.since_read >= REREAD {
        s.since_read = 0.0;
        if let Err(e) = s.model.refresh(&mut sim.world) {
            warn!("quick loot: 0x{:08X} cannot be listed again: {e}", s.target);
        }
    }
    // The wheel moves the selection, up moving it up.
    let notches = scroll.delta.y;
    if notches != 0.0 && !input.suppressed() {
        let steps = if notches > 0.0 { -1 } else { 1 };
        s.model.apply(&mut sim.world, Command::Scroll(steps));
    }
    let cmd = if input.just_pressed(Control::Activate) {
        Some(Command::Stack)
    } else if input.just_pressed(Control::ReadyItem) && s.kind != Kind::Pocket {
        Some(Command::TakeAll)
    } else {
        if input.just_pressed(Control::Rest) {
            info!("quick loot: the full menu on 0x{:08X} \"{}\"", s.target, s.title);
            FULL.store(true, Ordering::Relaxed);
        }
        None
    };
    let Some(cmd) = cmd else { return };
    if s.rows().is_empty() {
        return;
    }
    let forms = sim.world.forms;
    s.model.focus = Side::Container;
    let a = s.model.apply(&mut sim.world, cmd);
    if !a.taken.is_empty() || !a.picked.is_empty() {
        TOOK.store(true, Ordering::Relaxed);
    }
    let status = s.model.status.clone().unwrap_or_default();
    info!("quick loot: {cmd:?} on 0x{:08X} \"{}\": {status}", s.target, s.title);
    if s.kind == Kind::Pocket {
        if !a.taken.is_empty() || a.caught.is_some() {
            stolen.write(crate::pickpocket::Pickpocketed { victim: s.holder, taken: a.taken.clone(), given: Vec::new(), placed: Vec::new(), caught: a.caught });
        }
    } else if !a.taken.is_empty() {
        // Taking owned items is theft (`crate::crime`; a body's never is).
        taken.write(crate::crime::ItemsTaken { container: s.holder, items: a.taken.clone() });
    }
    // A body's dropped weapon taken: picked up from the world (`menu::model::Row::reference`).
    for id in &a.picked {
        info!("quick loot: picked up reference {id} lying beside 0x{:08X}", s.target);
        changed.write(RefChanged::new(id.0));
    }
    if cmd == Command::TakeAll && !a.taken.is_empty() {
        sounds.write(crate::ui_sounds::UiSound { menu: crate::ui_sounds::UiMenu::Container, event: crate::ui_sounds::UiEvent::TakeAll });
    }
    // What came into the inventory, as a script's AddItem says it (`crate::script_events`).
    if let Some(c) = corner.as_deref_mut() {
        const MANY: usize = 4;
        if a.taken.len() >= MANY {
            let total: i32 = a.taken.iter().map(|(_, n)| n).sum();
            c.push(format!("{} items added ({total} in all)", a.taken.len()).into());
        } else {
            for (item, n) in &a.taken {
                let name = crate::menu::model::display_name(forms, *item);
                c.push(if *n > 1 { format!("{name} ({n}) added") } else { format!("{name} added") }.into());
            }
        }
    }
    if a.caught.is_some() {
        info!("quick loot: caught on 0x{:08X} \"{}\": hidden", s.target, s.title);
        ql.closed_on = Some(s.target);
        ql.shown = None;
        // `HOLDS` stays set this frame: the E that was caught is not Activate's as well
        // (it would open the pocket); the next frame lets it go.
        return;
    }
    if let Err(e) = s.model.refresh(&mut sim.world) {
        warn!("quick loot: 0x{:08X} cannot be listed again: {e}", s.target);
    }
}

/// Out of play (a menu, dialogue, a loading screen) the list goes; it is opened again on
/// the target when play comes back, so what the full menu changed is read.
fn leave_play(mut ql: ResMut<QuickLoot>) {
    ql.shown = None;
    HOLDS.store(false, Ordering::Relaxed);
}

pub struct QuickLootPlugin;

// ---- drawing ----------------------------------------------------------------------------

#[derive(Component)]
struct Root;

#[derive(Component)]
struct Panel;

/// The game's raster faces, loaded once (the sharp ones are made from them as the Menu
/// Font row says, [`crate::pause::MenuFont::faces`]).
#[derive(Default)]
struct Kit {
    raster: Option<Faces>,
}

/// What the drawing shows: the words, whether in the sharp font, the room to the HUD's right
/// edge (whole units), the scale and the backing's darkness (percent), the view's height
/// (physical pixels) and the words' language (`fnv_util::words::generation`).
type Drawn = Option<(Spec, bool, i32, i32, i32, u32, u32, u32)>;

fn colour(ink: Ink, crime: bool) -> Color {
    let row = |full: bool| {
        let c = if crime { crate::reticle::crime_colour() } else { amber(255.0) };
        if full {
            c
        } else {
            c.with_alpha(0.72)
        }
    };
    match ink {
        Ink::Hud => amber(255.0),
        Ink::Head => amber(255.0).with_alpha(0.5),
        Ink::Dim => amber(255.0).with_alpha(0.6),
        Ink::Row(full) => row(full),
        Ink::Fill => row(true).with_alpha(0.16),
        Ink::Backing => Color::BLACK.with_alpha(backing()),
    }
}

fn spawn_panel(p: &mut ChildSpawnerCommands, laid: &Laid, faces: &Fonts, crime: bool) {
    for b in &laid.bars {
        p.spawn((at(b.rect.min.x, b.rect.min.y, b.rect.width(), b.rect.height()), BackgroundColor(colour(b.ink, crime)), Pickable::IGNORE));
    }
    for piece in &laid.pieces {
        let f = face(faces, piece.which);
        let line = f.font.lines(&piece.text, None).into_iter().next().unwrap_or_default();
        set_line(p, f, &line, piece.x, piece.base, colour(piece.ink, crime));
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw(
    mut commands: Commands,
    state: Res<State<PlayState>>,
    ql: Res<QuickLoot>,
    bindings: Option<Res<crate::bindings::Bindings>>,
    (menu_font, mut fonts, hidden): (Option<ResMut<crate::pause::MenuFont>>, Option<ResMut<Assets<bevy::text::Font>>>, Option<Res<crate::hud::HudHidden>>),
    (data, mut images, formats): (Option<Res<GameData>>, ResMut<Assets<Image>>, Option<Res<CompressedImageFormatSupport>>),
    mut kit: Local<Kit>,
    mut drawn: Local<Drawn>,
    mut root: Query<&mut Visibility, With<Root>>,
    mut panels: Query<(Entity, &mut Node), With<Panel>>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
) {
    // The list's own size, not the HUD's [design, #2933: second look].
    let size = scale();
    let units = crate::interface::units_at(size);
    let playing = *state.get() == PlayState::Playing;
    let shown = ql.shown.as_ref().filter(|_| playing && !hidden.is_some_and(|h| h.on));
    if kit.raster.is_none() {
        if shown.is_none() {
            return;
        }
        kit.raster = Some(Faces::load(data.as_deref(), &mut images, bc_supported(formats.as_deref())));
        commands
            .spawn((Root, Node { position_type: PositionType::Absolute, left: Val::Percent(50.0), top: Val::Percent(50.0), width: Val::Px(0.0), height: Val::Px(0.0), ..default() }, Visibility::Hidden, Pickable::IGNORE))
            .with_children(|r| {
                r.spawn((Panel, at(LEFT, 0.0, 0.0, 0.0), Pickable::IGNORE));
            });
        return;
    }
    let Ok(mut vis) = root.single_mut() else { return };
    let want = if shown.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    if *vis != want {
        *vis = want;
    }
    let Some(s) = shown else {
        *drawn = None;
        return;
    };
    let mut menu_font = menu_font;
    let sharp = menu_font.as_deref().is_some_and(crate::pause::MenuFont::drawn_sharp);
    let key = |c: Control| bindings.as_deref().map_or_else(|| "?)".into(), |b| b.prompt(c));
    let spec = Spec::of(s, Columns::chosen(s.kind == Kind::Pocket), key);
    // The room from the list's left to the HUD's right edge, in the list's units at `size`
    // (the HUD's are `ui_scale` of the view's).
    let aspect = window.single().map_or(16.0 / 9.0, |w| w.width() / w.height().max(1.0));
    let room = |size: f32| {
        let (half, edge) = (480.0 * aspect / size.max(0.1), crate::hud::RIGHT_EDGE_IN * crate::interface::ui_scale() / size.max(0.1));
        (half, edge, widest(half, edge))
    };
    let (half, edge, max_w) = room(size);
    // A physical pixel in the list's units (`crate::interface::u` is a 960th of the view's
    // height at the list's scale).
    let tall = window.single().map_or(0, |w| w.physical_height());
    let px_at = |size: f32| if tall > 0 { 960.0 / (tall as f32 * size.max(0.1)) } else { 0.0 };
    let now = Some((spec, sharp, max_w.round() as i32, (size * 100.0).round() as i32, (backing() * 100.0).round() as i32, tall, fnv_util::words::generation(), crate::cjk_font::revision()));
    if *drawn == now {
        return;
    }
    let (spec, ..) = now.clone().expect("set above");
    let raster = kit.raster.as_ref().expect("loaded above");
    let faces = match (sharp, crate::vector_font::metrics(), menu_font.as_deref_mut(), fonts.as_deref_mut()) {
        (true, Some(m), Some(menu), Some(fonts)) => Fonts::sharp(raster, m, menu.handle(fonts)),
        _ => Fonts::raster(raster),
    };
    let mut laid = layout_on(&spec, &faces, max_w, px_at(size));
    let (mut size, mut half, mut edge) = (size, half, edge);
    // Wider than its room even with the names cut (every column on a narrow screen, long
    // keys): drawn smaller until it fits. The room grows as the scale shrinks, so the scale
    // that fits is `(max_w + LEFT) * size / (width + LEFT)`.
    if laid.size.x > max_w + 0.01 {
        let fitted = fitted_scale(max_w, laid.size.x, size);
        info!("quick loot: {:.0} units wide in a room of {:.0}: drawn at {:.0}% instead of {:.0}%", laid.size.x, max_w, fitted * 100.0, size * 100.0);
        size = fitted;
        let fitted_room = room(size);
        (half, edge) = (fitted_room.0, fitted_room.1);
        laid = layout_on(&spec, &faces, fitted_room.2, px_at(size));
    }
    drop(units);
    let _units = crate::interface::units_at(size);
    // Logged when the words or the font change, not as the selection moves.
    let new_words = drawn.as_ref().is_none_or(|(d, f, _, z, ..)| d.title != spec.title || d.columns != spec.columns || *f != sharp || d.rows.len() != spec.rows.len() || *z != (size * 100.0).round() as i32);
    if new_words {
        let bad = overlaps(&laid, &faces);
        let font = if sharp { "Sharp" } else { "Original" };
        let c = |w: Which| cap(face(&faces, w));
        info!("quick loot: capitals {:.1} (rows), {:.1} (name), {:.1} (heads), {:.1} (figures) units high, font {}", c(Which::Entry), c(Which::Title), c(Which::Head), c(Which::Small), if sharp { "Sharp" } else { "Original" });
        if bad.is_empty() {
            info!("quick loot: drawn \"{}\" ({} rows, columns {:?}, font {font}, scale {:.0}%, {:.0} x {:.0} units, box centred, right edge {:.0} of {:.0}, keys \"{}\"): no overlaps", spec.title, spec.rows.len(), spec.columns, size * 100.0, laid.size.x, laid.size.y, LEFT + laid.size.x, half - edge, spec.prompts.join(" | "));
        } else {
            warn!("quick loot: drawn \"{}\" (font {font}): overlaps: {}", spec.title, bad.join(", "));
        }
    }
    // The box centred on the crosshair's height, however many rows it has [design,
    // #2933: step 3].
    for (e, mut node) in &mut panels {
        let (left, top) = (crate::interface::u(LEFT), crate::interface::u(-laid.box_h / 2.0));
        if node.top != top || node.left != left {
            node.top = top;
            node.left = left;
        }
        commands.entity(e).despawn_related::<Children>();
        let crime = spec.crime;
        let (laid, faces) = (laid.clone(), faces.clone());
        commands.entity(e).with_children(|p| spawn_panel(p, &laid, &faces, crime));
    }
    *drawn = now;
}

impl Plugin for QuickLootPlugin {
    fn build(&self, app: &mut App) {
        use crate::settings::{add_setting, Setting};
        let group = crate::pause::GROUP_MENUS;
        add_setting(
            app,
            Setting {
                key: "quick-loot",
                group,
                label: "Enabled",
                note: "On: a container, body or (sneaking) pocket under the crosshair lists its items without pausing. Activate takes, Ready Item takes all, Wait opens the menu.",
                switches: "",
                get: |_| ON.load(Ordering::Relaxed),
                set: |_, on| ON.store(on, Ordering::Relaxed),
                slider: None,
            },
        );
        crate::settings::add_choice_list(
            app,
            Setting {
                key: "quick-loot-scale",
                group,
                label: "Scale",
                note: "Size of the Quick Loot list, independent of HUD scale.",
                switches: "",
                get: |_| false,
                set: |_, _| {},
                slider: None,
            },
            crate::settings::ChoiceList { names: |_| scale_names(), chosen: scale_chosen, pick: take_scale, save: scale_chosen, load: take_scale, ask_keep: false },
        );
        add_setting(
            app,
            Setting {
                key: "quick-loot-background",
                group,
                label: "Background opacity",
                note: "How dark the box behind the Quick Loot list's words is.",
                switches: "",
                get: |_| false,
                set: |_, _| {},
                slider: Some(crate::settings::Slider { get: |_| backing(), set: |_, v| BACKING.store(v.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed), min: 0.0, step: 0.05, shown: None }),
            },
        );
        add_setting(
            app,
            Setting {
                key: "quick-loot-weight",
                group,
                label: "Show weight",
                note: "Quick Loot lists each item's weight.",
                switches: "",
                get: |_| WEIGHT.load(Ordering::Relaxed),
                set: |_, on| WEIGHT.store(on, Ordering::Relaxed),
                slider: None,
            },
        );
        add_setting(
            app,
            Setting {
                key: "quick-loot-value",
                group,
                label: "Show value",
                note: "Quick Loot lists each item's value.",
                switches: "",
                get: |_| VALUE.load(Ordering::Relaxed),
                set: |_, on| VALUE.store(on, Ordering::Relaxed),
                slider: None,
            },
        );
        add_setting(
            app,
            Setting {
                key: "quick-loot-value-per-weight",
                group,
                label: "Show value per weight",
                note: "Quick Loot lists each item's value divided by its weight.",
                switches: "",
                get: |_| PER_WEIGHT.load(Ordering::Relaxed),
                set: |_, on| PER_WEIGHT.store(on, Ordering::Relaxed),
                slider: None,
            },
        );
        app.init_resource::<QuickLoot>()
            .add_systems(OnExit(PlayState::Playing), leave_play)
            .add_systems(Update, draw.in_set(crate::PlaySet::Ui));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, count: i32, selected: bool) -> RowSpec {
        RowSpec { name: name.into(), count, weight: Some(3.0), value: Some(225), chance: None, selected }
    }

    fn spec(columns: Columns, rows: Vec<RowSpec>) -> Spec {
        let mut prompts = vec!["[E] Take".to_string()];
        if !columns.chance {
            prompts.push("[R] Take All".into());
        }
        prompts.push("[T] Search".into());
        Spec { title: "Footlocker".into(), position: "3/8".into(), more_up: true, more_down: true, rows, columns, prompts, crime: false }
    }

    fn rows() -> Vec<RowSpec> {
        vec![
            row("10mm Pistol", 1, false),
            row("10mm Round", 240, false),
            row("Stimpak", 2, true),
            row("Purified Water", 3, false),
            row("Sunset Sarsaparilla Star Bottle Cap", 1057, false),
            row("Scrap Metal", 1, false),
        ]
    }

    /// The faces the list is drawn in: the stand-ins of the game's fonts 7 and 3, and the
    /// sharp font as the Menu Font row's Sharp makes them.
    fn both_fonts() -> Vec<(&'static str, Fonts)> {
        let plain = Faces::plain();
        let mut out = vec![("Original", Fonts::raster(&plain))];
        if let Some(m) = crate::vector_font::metrics() {
            out.push(("Sharp", Fonts::sharp(&plain, m, Handle::default())));
        }
        out
    }

    const ALL: Columns = Columns { weight: true, value: true, per_weight: true, chance: false };

    /// A 16:9 view's room at HUD scale 1.
    fn room() -> f32 {
        widest(480.0 * 16.0 / 9.0, crate::hud::RIGHT_EDGE_IN)
    }

    fn find<'a>(l: &'a Laid, text: &str) -> &'a Piece {
        l.pieces.iter().find(|p| p.text == text).unwrap_or_else(|| panic!("no \"{text}\""))
    }

    /// A sneaking target whose script has an OnActivate block is not pickpocketed by hand on
    /// main (`script::activate_by_hand` runs the block instead), so the list never shows its
    /// pockets either; the same actor without the script does show them (review of #2935).
    #[test]
    fn a_scripted_actors_pockets_are_not_listed() {
        use crate::script::ScriptState;
        use crate::script_draw::tests::{play_over, record, sub, top};
        use fnv_script::vm::XVal;
        let text = |sig: &[u8; 4], s: &str| sub(sig, [s.as_bytes(), &[0]].concat());
        // An enabled Object script header (`menu::preactivation_tests`).
        let header = [0u32.to_le_bytes().to_vec(), 0u32.to_le_bytes().to_vec(), 0u32.to_le_bytes().to_vec(), 0u32.to_le_bytes().to_vec(), 0u16.to_le_bytes().to_vec(), 1u16.to_le_bytes().to_vec()].concat();
        // The string_var makes the build compile the unit from its source (`source_compile`).
        let source = "scn QuickLootGuardScript\nstring_var guardNote\nBegin OnActivate\nSetModLocalData \"guard.block\" 1\nEnd\n";
        let play = play_over(&[
            top(
                b"NPC_",
                &[
                    record(b"NPC_", 0, 7, &[text(b"EDID", "Player")]),
                    record(b"NPC_", 0, 0x810, &[text(b"EDID", "QuickLootGuard"), sub(b"SCRI", 0x820u32.to_le_bytes().to_vec())]),
                    record(b"NPC_", 0, 0x811, &[text(b"EDID", "QuickLootPlain")]),
                ]
                .concat(),
            ),
            top(b"SCPT", &record(b"SCPT", 0, 0x820, &[text(b"EDID", "QuickLootGuardScript"), sub(b"SCHR", header), sub(b"SCDA", vec![]), sub(b"SCTX", source.as_bytes().to_vec())])),
        ]);
        let mut scripts = ScriptState::over_plugins(play.lo.plugins);
        let mut sim = SimState::new(play.lo, play.schema, play.index);
        let rt = scripts.rt.as_mut().unwrap();
        let target = |form_id: u32| crate::activate::Targeted { form_id, name: "Guard".into(), action: Action::Steal, distance: 50.0, source: crate::activate::pick::Source::Collision, crime: true, empty: false, through: None };
        // Placed and judged with the world attached, as the frame does (`script::with_world`).
        let (guard, plain) = crate::script::with_world(rt, &mut sim, |rt| {
            let mut placed = |base: &str| {
                let body = format!("ref r\nBegin Function {{}}\nlet r := player.PlaceAtMe {base}\nSetFunctionValue r\nEnd");
                match rt.game.run_query(&body, default()).unwrap() {
                    XVal::Ref(id) if id != 0 => id,
                    other => panic!("placing {base}: {other:?}"),
                }
            };
            let guard = placed("QuickLootGuard");
            let plain = placed("QuickLootPlain");
            (wanted(&target(guard), false, &mut rt.game), wanted(&target(plain), false, &mut rt.game))
        });
        assert_eq!(guard, None, "an OnActivate block runs instead of the theft");
        assert_eq!(plain, Some(Kind::Pocket));
    }

    #[test]
    fn nothing_overlaps_in_either_font_with_any_columns() {
        assert_eq!(both_fonts().len(), 2, "the sharp font's metrics read");
        for (font, fonts) in both_fonts() {
            for bits in 0..16u8 {
                let c = Columns { weight: bits & 1 != 0, value: bits & 2 != 0, per_weight: bits & 4 != 0, chance: bits & 8 != 0 };
                let l = layout(&spec(c, rows()), &fonts, room());
                assert_eq!(overlaps(&l, &fonts), Vec::<String>::new(), "{font}, {c:?}");
                assert!(l.size.x <= room() + 0.01, "{font}, {c:?}: {} units wide passes the HUD's right edge ({})", l.size.x, room());
            }
            let empty = layout(&spec(Columns::default(), Vec::new()), &fonts, room());
            assert_eq!(overlaps(&empty, &fonts), Vec::<String>::new(), "{font}: an empty list");
            find(&empty, EMPTY);
        }
    }

    /// Third look (#2933): the keys' line centred under the box; a box no wider than its
    /// words is as wide as that line with the words' room (the name's start) on both sides.
    #[test]
    fn the_keys_are_centred_and_the_box_holds_them_with_room() {
        for (font, fonts) in both_fonts() {
            let short = Spec { title: "Box".into(), rows: vec![row("Gum", 1, true)], ..spec(Columns::default(), Vec::new()) };
            for (what, l) in [("short", layout(&short, &fonts, room())), ("wide", layout(&spec(ALL, rows()), &fonts, room()))] {
                let (first, last) = (find(&l, "[E] Take"), find(&l, "[T] Search"));
                let (before, after) = (first.x, l.size.x - (last.x + last.width));
                assert!((before - after).abs() < 0.01, "{font}, {what}: {before} before the keys, {after} after");
            }
            let l = layout(&short, &fonts, room());
            let (title, key) = (find(&l, "Box"), find(&l, "[E] Take"));
            assert!((key.x - title.x).abs() < 0.01, "{font}: the narrowest box has the words' room before E: {} and {}", key.x, title.x);
        }
    }

    /// Second look (#2933): every line of words is one height, the rows', whatever its
    /// face; the words' capitals centred in it; the place ("3/8") and the heads bold and alike,
    /// the figures at their size.
    #[test]
    fn every_line_is_one_height_with_its_words_centred() {
        for (font, fonts) in both_fonts() {
            for c in [Columns::default(), ALL, Columns { chance: true, ..Columns::default() }] {
                let l = layout(&spec(c, rows()), &fonts, room());
                assert!(l.lines.iter().all(|r| (r.height() - LINE).abs() < 0.01), "{font}: every line {LINE} high");
                assert_eq!(l.lines.len(), 1 + usize::from(c.any()) + 6 + 1, "{font}, {c:?}: the name's, the heads', six rows', the keys'");
                for p in &l.pieces {
                    let line = l.lines.iter().find(|r| p.base > r.min.y && p.base <= r.max.y).unwrap_or_else(|| panic!("{font}: \"{}\" on no line", p.text));
                    let middle = p.base - cap(face(&fonts, p.which)) / 2.0;
                    assert!((middle - (line.min.y + line.max.y) / 2.0).abs() < 0.01, "{font}: \"{}\" not centred in its line", p.text);
                }
            }
            let l = layout(&spec(ALL, rows()), &fonts, room());
            let (title, head) = (find(&l, "Footlocker"), find(&l, WG));
            assert!(title.base < l.rule && head.base > l.rule + RULE, "{font}: the rule between the name and the heads");
            let place = find(&l, "3/8");
            assert_eq!((place.which, head.which), (Which::Head, Which::Head), "{font}");
            assert!(l.pieces.iter().filter(|p| p.text == "225").all(|p| p.which == Which::Small), "{font}: the figures in the small face");
            assert_eq!(fonts.head.font.size, fonts.small.font.size, "{font}: heads and figures one size");
        }
    }

    /// Fifth look (#2933): on screen, every bar's edge on a whole pixel, and the rows'
    /// box as many pixels from the frame's sides as from its bottom; the keys at the item
    /// names' size.
    #[test]
    fn the_rows_box_has_as_many_pixels_beside_it_as_under_it() {
        for (font, fonts) in both_fonts() {
            for px in [960.0 / 1080.0, 960.0 / 1440.0, 960.0 / (2160.0 * 0.7), 960.0 / (1080.0 * 0.5), 960.0 / (1600.0 * 0.9)] {
                let l = layout_on(&spec(ALL, rows()), &fonts, room(), px);
                let last = *l.bands.last().expect("rows");
                let frame = l.bars[1].rect.height();
                let (side, under) = (last.min.x - frame, l.box_h - frame - last.max.y);
                assert!((side - under).abs() < 1e-3, "{font}, {px}: {side} beside, {under} under");
                let on_grid = |v: f32| (v / px - (v / px).round()).abs() < 1e-2;
                for b in &l.bars {
                    let r = b.rect;
                    assert!(on_grid(r.min.x) && on_grid(r.min.y) && on_grid(r.max.x) && on_grid(r.max.y), "{font}, {px}: {r:?} off the pixels");
                }
            }
            let l = layout(&spec(ALL, rows()), &fonts, room());
            assert!(l.pieces.iter().filter(|p| p.text.contains(") ")).all(|p| p.which == Which::Small), "{font}: the keys in the names' face");
        }
    }

    /// The bug check before the pull request: a pocket list stays only while the pocket is
    /// one (its owner standing up, dying or waking makes another list or none).
    #[test]
    fn the_kind_follows_the_action() {
        assert_eq!(kind_of(&Action::Steal, false), Some(Kind::Pocket));
        assert_eq!(kind_of(&Action::Search, false), Some(Kind::Body));
        assert_eq!(kind_of(&Action::Open, true), Some(Kind::Container));
        assert_eq!(kind_of(&Action::Open, false), None, "a door is no container");
        assert_eq!(kind_of(&Action::Talk, false), None, "a pocket's owner who stood up");
    }

    /// The bug check: a list wider than its room (every column on a 5:4 screen with long
    /// keys) is drawn at a scale where it fits, never past the HUD's edge.
    #[test]
    fn a_list_too_wide_for_its_room_is_drawn_smaller_to_fit() {
        for (font, fonts) in both_fonts() {
            let room_at = |s: f32| widest(480.0 * 5.0 / 4.0 / s, crate::hud::RIGHT_EDGE_IN / s);
            let mut wide = spec(ALL, rows());
            wide.prompts = vec!["[E] Tout prendre ici".into(), "[R] Tout voler partout".into(), "[T] Fouiller le conteneur".into()];
            let l = layout(&wide, &fonts, room_at(1.0));
            assert!(l.size.x > room_at(1.0) + 0.01, "{font}: the case is too wide at 100% ({} in {})", l.size.x, room_at(1.0));
            let s = fitted_scale(room_at(1.0), l.size.x, 1.0);
            assert!(s < 1.0, "{font}: drawn smaller");
            let fitted = layout(&wide, &fonts, room_at(s));
            assert!(fitted.size.x <= room_at(s) + 0.01, "{font}: {} units in a room of {} at {s}", fitted.size.x, room_at(s));
        }
    }

    /// The bug check: a container's long name is whole when the room allows it, however
    /// short its items' names.
    #[test]
    fn a_long_container_name_widens_the_box() {
        for (font, fonts) in both_fonts() {
            let mut long = spec(Columns::default(), vec![row("Pen", 1, true)]);
            long.title = "Vault 21 Overseer's Footlocker".into();
            let l = layout(&long, &fonts, room());
            find(&l, "Vault 21 Overseer's Footlocker");
            assert!(overlaps(&l, &fonts).is_empty(), "{font}: {:?}", overlaps(&l, &fonts));
        }
    }

    /// Sixth look (#2933): with columns, the names' head on their heads' line, where the
    /// names start; none without columns.
    #[test]
    fn the_item_head_stands_over_the_names() {
        for (font, fonts) in both_fonts() {
            let l = layout(&spec(ALL, rows()), &fonts, room());
            let (head, name, wg) = (find(&l, ITEM), find(&l, "10mm Pistol"), find(&l, WG));
            assert!((head.x - name.x).abs() < 0.01, "{font}: the head where the names start");
            assert_eq!((head.base, head.which), (wg.base, Which::Head), "{font}: on the heads' line, in their face");
            assert_eq!((head.ink, wg.ink), (Ink::Head, Ink::Head), "{font}: the heads faint");
            let plain = layout(&spec(Columns::default(), rows()), &fonts, room());
            assert!(!plain.pieces.iter().any(|p| p.text == ITEM), "{font}: no heads' line without columns");
        }
    }

    /// Sixth look (#2933): a count never gives way, the name before it does; and a name
    /// that fits is never cut, at any pixel size.
    #[test]
    fn a_cut_name_keeps_its_count_and_a_fitting_one_is_not_cut() {
        for (font, fonts) in both_fonts() {
            let mut r = rows();
            r[0].name = "A very long item name that runs on and on past any list the HUD has room for".into();
            r[0].count = 6;
            let l = layout(&spec(ALL, r), &fonts, room());
            let cut = l.pieces.iter().find(|p| p.text.starts_with("A very")).expect("the long row");
            assert!(cut.text.ends_with("... (6)"), "{font}: {:?}", cut.text);
            assert_eq!(overlaps(&l, &fonts), Vec::<String>::new(), "{font}");
            let mut r = rows();
            r[0].name = "Sunset Sarsaparilla".into();
            r[0].count = 6;
            for px in [0.0, 960.0 / 1080.0, 960.0 / 1440.0, 960.0 / (2160.0 * 0.7), 960.0 / (1600.0 * 0.9), 960.0 / (1080.0 * 0.5)] {
                let l = layout_on(&spec(ALL, r.clone()), &fonts, room(), px);
                // Found whole: neither the name nor its count cut.
                find(&l, "Sunset Sarsaparilla (6)");
            }
        }
    }

    /// Step 10 (#2933) and long names: the list stops at the HUD's right edge and the
    /// name gives way.
    #[test]
    fn a_long_name_is_cut_at_the_huds_edge() {
        for (font, fonts) in both_fonts() {
            let mut r = rows();
            r[0].name = "A very long item name that runs on and on past any list the HUD has room for".into();
            let l = layout(&spec(ALL, r), &fonts, room());
            assert!((l.size.x - room()).abs() < 0.01, "{font}: as wide as it may be: {}", l.size.x);
            assert!(l.pieces.iter().any(|p| p.text.contains("...")), "{font}: cut with the mark");
            assert_eq!(overlaps(&l, &fonts), Vec::<String>::new(), "{font}");
            let fits = layout(&spec(Columns::default(), rows()), &fonts, room());
            for name in ["10mm Pistol", "Purified Water", "Scrap Metal"] {
                find(&fits, name);
            }
        }
    }

    #[test]
    fn counts_stand_right_without_columns_and_beside_the_name_with_them() {
        for (font, fonts) in both_fonts() {
            let l = layout(&spec(Columns::default(), rows()), &fonts, room());
            let counts: Vec<&Piece> = l.pieces.iter().filter(|p| p.text.starts_with('(')).collect();
            assert_eq!(counts.len(), 4, "{font}: the four stacks' counts");
            let ends: Vec<f32> = counts.iter().map(|p| p.x + p.width).collect();
            assert!(ends.iter().all(|e| (e - ends[0]).abs() < 0.01), "{font}: the counts right-aligned: {ends:?}");
            find(&l, "Stimpak");
            let with = layout(&spec(ALL, rows()), &fonts, room());
            find(&with, "Stimpak (2)");
            assert!(!with.pieces.iter().any(|p| p.text == "(2)"), "{font}: no count column");
            for h in [WG, VAL, PER_WG] {
                let head = find(&with, h);
                let cells: Vec<&Piece> = with.pieces.iter().filter(|p| p.which == Which::Small && (p.x + p.width - head.x - head.width).abs() < 0.01).collect();
                assert_eq!(cells.len(), 6, "{font}: {h}'s figures right-aligned under it");
            }
        }
    }

    #[test]
    fn a_pocket_has_its_chance_and_no_take_all() {
        let pocket = spec(Columns { chance: true, ..Columns::default() }, rows());
        assert!(!pocket.prompts.iter().any(|p| p.contains(TAKE_ALL)));
        for (font, fonts) in both_fonts() {
            let l = layout(&pocket, &fonts, room());
            find(&l, CHANCE);
            assert_eq!(overlaps(&l, &fonts), Vec::<String>::new(), "{font}");
        }
    }

    #[test]
    fn value_per_weight() {
        assert_eq!(per_weight_text(Some(3.0), Some(225)), "75");
        assert_eq!(per_weight_text(Some(2.0), Some(5)), "2.5");
        assert_eq!(per_weight_text(None, Some(5)), "--");
        assert_eq!(per_weight_text(Some(1.0), None), "--");
    }
}
