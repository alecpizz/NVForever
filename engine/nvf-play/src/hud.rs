//! The HUD of the original: health and compass at the bottom left, action points,
//! weapon condition and ammo at the bottom right (docs/spec-notes/hud.md).
//!
//! The pieces, their sizes and their textures are the game's own interface files'
//! (§2, §3); where they stand is left to the game's code by those files and is set here
//! (§4). Lengths are in the interface's units, 960 to the height of the view.
//!
//! - [`gather`] reads what the HUD shows from the game: the player's health and action
//!   points (the script runtime's actor values), the heading, the equipped weapon's
//!   condition and ammo (`combat`).
//! - [`Places`] sets the pieces out (pure, tested without the game's files).
//! - The pieces are spawned once; after that only the bars' lengths, the compass strip's
//!   place and the ammo's digits change.
//! - Without the game's textures the bars are plain blocks and the brackets plain lines.
//! - The enemy health bar (bottom middle, `crate::hud_target` says whose) and the sneak
//!   word (top middle, while sneaking) are parts of the same HUD (§8, §9).
//! - A bar's pips are set on whole pixels, all as wide as each other, whatever the
//!   view's size (§4).

use crate::combat::{Combat, HudWeapon};
use crate::compass_strip::{StripNode, StripTexels, StripWindow};
use crate::control::ControlLocks;
use crate::hud_target::HudTarget;
use crate::player::Player;
use crate::repeat::Repeats;
use crate::reticle::crime_colour;
use crate::script::{with_world, ScriptState};
use crate::sim::SimState;
use crate::tiles::vanilla::{DrawnByTiles, EnemyPiece};
use crate::interface::{amber, at, bc_supported, face, plain_font, set_line, texture, u, Face, Picture, Pictures};
use std::ops::DerefMut;
use crate::{PlaySet, PlayState};
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use fnv_sim::FormId;
use fnv_font::Font;
use fnv_game::actor_values::ACTION_POINTS;
use fnv_game::awareness::State as Awake;
use nvf_assets::GameData;

// ---- the interface files' numbers [documented: hud.md §2, §3] -------------------------

/// The two corner pieces (`HitPoints`, `ActionPoints`): width and height.
pub(crate) const HP_PANEL: Vec2 = Vec2::new(369.0, 127.0);
const AP_PANEL: Vec2 = Vec2::new(386.0, 127.0);
/// The bracket pictures' room; the pictures are drawn texel for unit.
const BRACKET: Vec2 = Vec2::new(512.0, 256.0);
/// One tick of a bar: the room it takes and its height. (The file's own length for a full
/// bar, 300, is about the 296 measured, §4.)
const TICK: Vec2 = Vec2::new(8.0, 20.0);
/// The tick picture's block: 6 texels wide, then 2 clear [observed: `hud_tick_mark.dds`,
/// its opacity rising from top to bottom].
const TICK_BLOCK: f32 = 6.0;
/// The health and action points ticks' brightness [hypothesis: brightness.xml's "meter
/// and compass in HUD" 175, as the enemy bar's meter reads (hud.md §14.8); the
/// original's ticks are about 0.7 of its lines' colour at their full rows, hud.md
/// §14.9]. The tile HUD writes it too (`tiles::vanilla::write_corners`).
pub(crate) const METER_BRIGHTNESS: f32 = 175.0;
/// The compass window in the health piece: place and size.
const COMPASS_AT: Vec2 = Vec2::new(20.0, 65.0);
pub const COMPASS: Vec2 = Vec2::new(345.0, 64.0);
/// The heading's x in compass-window units: where a mark straight ahead stands
/// [observed: hud.md §14.5].
pub const COMPASS_HEADING: f32 = 148.0;
/// Where the strip's own heading (its letter N at heading 0) stands in the window: 6 units
/// right of the marks' [observed: hud_compare compass-north at 1280 x 720, heading 0, the
/// original's N centred at x 159.5 pixels, both NVForever drawings' at 155 with the strip at
/// 148; hud.md §14.5, N's middle about 172 of `HitPoints` on the heading-0 shot, 4 right of
/// the bearing-0 tick's 168]. Until T12e the strip shared the marks' 148 [design].
pub const STRIP_HEADING: f32 = COMPASS_HEADING + 6.0;
/// The whole strip, north to north: 1024 texels for 360 degrees [observed: the texture].
pub const STRIP: Vec2 = Vec2::new(1024.0, 64.0);
/// The solid bar's ground (weapon condition): length and height [documented: the file
/// puts it at x 90 of a frame the code sets; where it stands is measured, below].
const CND_BAR: Vec2 = Vec2::new(60.0, 20.0);
/// The condition fill's full width, starting at the ground's x [observed: hud.md §14,
/// §14.3].
const CND_FILL_WIDTH: f32 = 59.0;
/// The condition's arrows, `CNDArrows`: a 16 x 16 picture, placed by the code
/// [documented: hud_main_menu.xml]; black arrows at the picture's top and bottom edges
/// pointing in, their middle 3.5 texels from its left [observed: the texture].
const CND_ARROWS: &str = "textures\\interface\\shared\\arrow\\repair_meter_pips.dds";
const ARROWS_MIDDLE: f32 = 3.5 / 16.0;
/// Where the arrows stand on the condition bar: below this condition a weapon may jam
/// [owner's playtest list 5, item 73: "the 75% threshold on condition … when jamming
/// occurs"; observed: the arrows at about three quarters on the owner's screenshot].
pub const JAM_CONDITION: f32 = 0.75;
/// The arrows' picture is drawn this much taller than the bar at the top and the bottom
/// and cut to the bar: the arrows' bases are the picture's first and last rows
/// [observed: the texture], and drawn exactly as high as the bar the filtered edge rows
/// left a strip of the bar showing above and below them (owner's playtest 2026-10-16).
const ARROWS_BLEED: f32 = 1.0;

/// The enemy's health piece, `EnemyHealth`, and its bracket picture's place and size in
/// it (`glow_enemy_health_seperator.dds`) [documented: hud_main_menu.xml,
/// hudtemplates.xml].
const ENEMY: Vec2 = Vec2::new(350.0, 52.0);
const ENEMY_BRACKET_AT: Vec2 = Vec2::new(-7.0, 3.0);
/// [documented: hudtemplates.xml, template_enemy_health_bracket; observed: private
/// jobs hud_enemy2, e5_back; hud_enemy3, g2_gecko_half]. Previously 512 wide [design].
const ENEMY_BRACKET: Vec2 = Vec2::new(500.0, 64.0);
/// The sneak piece, `SneakMeter`: width and height [documented: hud_main_menu.xml].
const SNEAK: Vec2 = Vec2::new(356.0, 34.0);

// ---- what the bracket pictures show [observed: the textures, §3] ----------------------

/// The upper edge of the brackets' long line.
const LINE_Y: f32 = 66.0;
/// The left bracket's upright and the right bracket's two (the end and the divider).
const LEFT_UPRIGHT: f32 = 17.0;
const RIGHT_UPRIGHT: f32 = 369.0;
const DIVIDER: f32 = 207.0;
/// How far in from the view's right edge the right bracket's end upright stands, its outer
/// side (the upright about 2 units wide): the line other pieces keep inside on the right
/// (`crate::quick_loot`) [design, #2933: step 10].
pub(crate) const RIGHT_EDGE_IN: f32 = MARGIN_X + AP_PANEL.x - RIGHT_UPRIGHT - 2.0;
/// The small corner marks on the long line: where a full bar ends on the left piece and
/// starts on the right one.
const LEFT_MARK: f32 = 320.0;
const RIGHT_MARK: f32 = 70.0;
/// The enemy bracket's long line, taken as `LINE_Y` is on the corner pieces (2 rows into
/// the solid line, rows 21-23; `LINE_Y` is 2 into rows 64-66), and the inner edges of its
/// two corner marks, between which its bar stands [observed: the texture]. At 19 (the glow
/// above the line) the line stood 4 units under the corner pieces' and its ticks 7 pixels
/// over it at 1080 lines, where the original's line is level with the corner pieces'
/// upper solid line and its ticks stand on it [observed: `Target healthbar` screenshot;
/// owner's playtest 2026-10-16].
const ENEMY_LINE_Y: f32 = 23.0;
/// The enemy bar's ticks stand right on its solid line (they end at row 21), not
/// `METER_GAP` over it as the player's bars do [observed: `Target healthbar` screenshot;
/// at `METER_GAP` a 3-pixel strip showed at 1080 lines, owner's playtest 2026-10-16].
const ENEMY_METER_GAP: f32 = 2.0;
/// Texture columns scaled to the measured 500-wide bracket [design; observed: private
/// job hud_enemy3, g2_gecko_half]. Previously unscaled (56, 307).
const ENEMY_MARKS: (f32, f32) = (56.0 * 500.0 / 512.0, 307.0 * 500.0 / 512.0);
/// The enemy bar's visible middle in `EnemyHealth`: halfway between the bracket's corner
/// marks, where its ticks are centred. The name and the Living Anatomy line are centred
/// here, not on the piece's width (175), which put them right of the bar (#1826) [design].
const ENEMY_MIDDLE: f32 = ENEMY_BRACKET_AT.x + (ENEMY_MARKS.0 + ENEMY_MARKS.1) / 2.0;

/// The left `x` that centres a line's drawn letters on `middle`: from the first letter's
/// picture to the end of the last one's, leaving out the empty room after the last letter
/// that `Line::width` counts (and the room before the first) [design: #1826].
fn centred_x(font: &Font, line: &fnv_font::Line, middle: f32) -> f32 {
    let drawn = line.glyphs.iter().map(|p| (p, font.bitmap_glyph(p))).filter(|(_, g)| g.drawn());
    let (left, right) = drawn.fold((f32::INFINITY, f32::NEG_INFINITY), |(l, r), (p, g)| (l.min(p.x + g.before), r.max(p.x + g.before + g.width)));
    if left > right {
        // Nothing drawn (stand-in font, spaces): its set width.
        return middle - line.width / 2.0;
    }
    middle - (left + right) / 2.0
}

// ---- set here; the files leave these to the game's code [hypothesis, §4] --------------

/// From the view's side and bottom edges to the pieces.
pub(crate) const MARGIN_X: f32 = 40.0;
/// The right bracket's atlas piece stands this far right of the right piece's corner
/// [observed: hud_compare at 1280 x 720 (needs_off, ammo-label, limbs_hidden, xp_meter):
/// the piece drawn at the corner put the divider at x 1111-1113, the upright from 1231-1232
/// and the step cap at 998, the original's at 1114-1116, 1234 and 1001-1002: 3 pixels, 4
/// units]. The tile HUD's `ActionPoints/right_bracket` stands as far in
/// (`tiles::vanilla::RIGHT_BRACKET_X`).
pub(crate) const RIGHT_BRACKET_X: f32 = 4.0;
pub(crate) const MARGIN_Y: f32 = 30.0;
/// The bars stand this far above the long line.
const METER_GAP: f32 = 4.0;
/// Room between a label and what it labels.
const LABEL_GAP: f32 = 6.0;
/// The line the texts of the lower row stand on.
const LOWER_BASE: f32 = 106.0;
/// The condition bar's fill and its ground: the HUD's colour at brightness 175 and an
/// opaque 100 [observed: the original's at 175/255 and 100/255 of its colour on the private
/// shots `hud_corners2` c0, c4 (T12c, hud.md §14.7); hud_compare hud-ammo-label, the fill
/// (175, 125, 45)]. Until T12e the fill at 255 over the fill's colour at a quarter's alpha
/// [hypothesis: as `meter.xml`'s ground]. The tile HUD writes the same
/// (`tiles::vanilla::write_corners`).
pub(crate) const CONDITION_BRIGHTNESS: f32 = 175.0;
pub(crate) const GROUND_BRIGHTNESS: f32 = 100.0;

// ---- measured on the owner's 1920 x 1080 screenshot of the original [observed, §4] -----

/// A full bar: 37 ticks, from just inside the bracket's upright to its corner mark (on
/// the left; mirrored on the right).
const PIPS: usize = 37;
const METER_WIDTH: f32 = PIPS as f32 * TICK.x;
/// The bars stand this far left of where they were set from the bracket alone (the
/// corner mark on the left piece, the upright on the right): the first tick starts at
/// the upright's inside edge, the last ends clear of the mark and of the upright
/// [observed: 3 pixels at 1080 lines against the `Target healthbar` screenshot, owner's
/// playtest 2026-10-16, after the ticks ran into the mark]. The health bar at x 20 of
/// `HitPoints`, 4 left [observed: hud.md §14, the bar's tile read at x 20; hud_compare
/// needs_off at 1280 x 720, the original's first tick at x 45 pixels and its last ending
/// at 265, a pixel before the step cap at 267, where 21 put them at 46 and 266, against
/// the cap]; 3 until T12e. The action points bar 2: its ticks stood 2 pixels left of the
/// original's at 1080 lines [observed: the same screenshot, tick edges 1520 … 1844
/// against 1518 … 1842, 2026-10-16]; its last tick now ends the tick's clear strip short
/// of the upright, as the original's at 720 lines ([`row_ticks`]).
const METER_SHIFT: f32 = 4.0;
const AP_METER_SHIFT: f32 = 2.0;
/// "HP" and "AP" stand above their bars near the uprights: the line they stand on, where
/// "HP" starts and where "AP" ends [observed: as above, "HP" 11 and "AP" 8 pixels left of
/// the earlier 33 and 362].
pub(crate) const UPPER_BASE: f32 = 33.0;
pub(crate) const HP_LABEL_X: f32 = 23.0;
const AP_LABEL_END: f32 = 355.0;
/// The condition bar ends this far before the divider, and its bottom stands this far
/// above the line "CND" stands on; "CND" ends this far before the bar [observed: on the
/// `Target healthbar` screenshot the bar starts about 8 pixels after the "D" and is about
/// as high as the letters; owner's playtest 2026-10-16, after 15 / 0 / 6 left 2 pixels
/// between "D" and the bar and the bar lower than the letters]. The bar ends 17 before
/// the divider [observed: hud_weapon capture 2026-10-28, hud.md §14: the 60 x 20 ground's
/// GetUIFloatInherited x 1410 with the divider at 1487, and on its screenshot the bar at
/// 1058-1102 of 1280 pixels; 10, from the screenshot of 2026-10-16, stood 7 too far right,
/// the label with it].
const CND_BEFORE_DIVIDER: f32 = 17.0;
const CND_RAISE: f32 = 2.0;
const CND_LABEL_GAP: f32 = 11.0;
/// The ammo type's abbreviation starts here, just right of the divider, whatever the
/// counter's length [observed: the owner's screenshot 2026-10-25 of the original marks
/// its room at about x 1680-1765 of 1920 at 1080 lines, the divider at 1672: 214-290
/// units; owner 2026-10-25: anchored to the divider, not against the counter; commit
/// 5887ee0e "at the owner's request"]. The maintainer's choice, kept (T12e): the
/// original draws it left of the divider, right-justified at `AmmoTypeLabel`'s W - 236,
/// and moves the condition row left to make room (hud.md §4; the tile HUD does that).
const AMMO_LABEL_X: f32 = 214.0;
/// How far below the count's baseline the ammo label goes with the Move Ammo Type Label
/// tweak, within the panel [design: docs/design/tweaks.md].
const AMMO_LABEL_DROP: f32 = 17.0;
/// The enemy bar's pips: as many as fit between the bracket's corner marks.
const ENEMY_PIPS: usize = 31;
/// The enemy's name: the line it stands on in its piece, centred.
const ENEMY_NAME_BASE: f32 = 53.0;
/// Living Anatomy's "Health … DT …" line stands on this y of the enemy's piece, a line under
/// the name [observed: the owner's screenshot 2026-10-24 of the original, the two lines
/// about 20 of 528 rows apart: 36 interface units].
const ENEMY_STATS_BASE: f32 = ENEMY_NAME_BASE + 36.0;
/// The sneak word: the line it stands on, from the view's top, centred.
const SNEAK_BASE: f32 = 51.0;

/// The labels [hypothesis: the game takes them from its settings (`sStats…`); the
/// owner's screenshot shows these words].
pub(crate) const HP: &str = "HP";
const AP: &str = "AP";
const CND: &str = "CND";

/// The font of every text on the HUD: font 7 [documented: `hudtemplates.xml`;
/// Fallout_default.ini `sFontFile_7`].
pub(crate) const FONT: &str = "textures\\fonts\\baked-in_monofonto_large.fnt";

const HUD_TEXTURES: &str = "textures\\interface\\hud\\";

/// What the HUD shows.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct HudView {
    /// The HUD is on the screen: the player is in the world, and no script holds the
    /// player's movement (§5).
    pub shown: bool,
    /// [design, #2659] The enemy piece can stay visible during classic playback.
    pub enemy_shown: bool,
    /// Health and action points, 0 to 1 of their full values.
    pub health: f32,
    pub action_points: f32,
    /// Compass heading in degrees (0 north, 90 east).
    pub heading: f32,
    pub weapon: HudWeapon,
    /// The sneak word, while the player sneaks (§9).
    pub sneak: Option<Sneak>,
    /// `[SPOTTED]` with who detects the player, under the Detected by Whom tweak.
    pub detected_by: Option<String>,
    /// The body armour's condition, 0 to 1, under the Armor Condition tweak.
    pub armor: Option<f32>,
    /// A kill camera has the view, which hides the HUD (`crate::kill_cam`).
    pub kill_cam: bool,
    /// Why the HUD is not shown, in words for the log (`crate::xp_meter`'s level-up wait,
    /// #3882); `None` while it is shown.
    pub hidden: Option<&'static str>,
}

/// A slideshow's room, by its cell's Editor ID: the ending's slide room `Endgame` and the
/// add-ons' intro rooms `NVDLC01Slideshow`, `NVDLC02SlideShow`, `NVDLC03Slideshow` and
/// `NVDLC04SlideShow` [observed: the base game's and the four add-ons' interior cells'
/// Editor IDs; no other interior's has "slideshow" in it]. The rooms hold only the slide,
/// its light and the narrator: a player is there only for the slides, and the slideshow is
/// over once a script moves the player out (or the ending's credits end the game). Co-op
/// reads it (`nvforever`'s `coop/scene.rs`): a guest stands in the host's slide room too.
pub fn slideshow_room(edid: &str) -> bool {
    edid.eq_ignore_ascii_case("Endgame") || edid.to_ascii_lowercase().contains("slideshow")
}

/// Whether the interior cell `interior` is a slideshow's room ([`slideshow_room`]), its
/// Editor ID looked up in the load order's cells (`RefTable::cell_edids`, lower case). A
/// co-op guest stands in the host's slide room too (the host's MoveTo takes it along,
/// `nvforever`'s `coop/scene.rs`), so its own cell says it.
pub fn in_slideshow(interior: Option<u32>, cell_edids: &std::collections::HashMap<String, FormId>) -> bool {
    interior.is_some_and(|c| cell_edids.iter().any(|(e, id)| id.0 == c && slideshow_room(e)))
}

impl Default for HudView {
    fn default() -> Self {
        HudView { shown: false, enemy_shown: false, health: 1.0, action_points: 1.0, heading: 0.0, weapon: HudWeapon::default(), sneak: None, detected_by: None, armor: None, kill_cam: false, hidden: None }
    }
}

/// The sneak word at the top middle while the player sneaks (§9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sneak {
    Hidden,
    Detected,
    Caution,
    Danger,
}

impl Sneak {
    /// [design: NVForever's own words, #2984.]
    pub fn word(self) -> &'static str {
        match self {
            Sneak::Hidden => "[UNSEEN]",
            Sneak::Detected => "[SPOTTED]",
            Sneak::Caution => "[ALERT]",
            Sneak::Danger => "[COMBAT]",
        }
    }

    /// The two warnings are in the HUD's alternative colour, the others in its own
    /// [observed: the screenshots].
    pub fn warning(self) -> bool {
        matches!(self, Sneak::Caution | Sneak::Danger)
    }

    /// Which word: someone fights the player; else someone hostile searches for the
    /// player; else someone detects the player; else hidden [hypothesis: the words'
    /// meaning as players describe them; the original's rule is not documented].
    pub fn of(fighting: bool, searching: bool, detected: bool) -> Sneak {
        if fighting {
            Sneak::Danger
        } else if searching {
            Sneak::Caution
        } else if detected {
            Sneak::Detected
        } else {
            Sneak::Hidden
        }
    }
}

/// `[SPOTTED]` naming who detects the player, under the player's Detected by Whom tweak:
/// the first `max` names, then how many more; with `max` 0 only how many [design: our
/// own words, docs/design/tweaks.md].
pub fn detected_word(names: &[String], max: usize) -> String {
    if names.is_empty() {
        return Sneak::Detected.word().to_string();
    }
    if max == 0 {
        return format!("[SPOTTED BY {}]", names.len());
    }
    let shown = &names[..names.len().min(max)];
    let rest = names.len() - shown.len();
    let list = match (shown, rest) {
        ([one], 0) => one.clone(),
        (many, 0) => format!("{} and {}", many[..many.len() - 1].join(", "), many[many.len() - 1]),
        (some, 1) => format!("{} and 1 other", some.join(", ")),
        (some, n) => format!("{} and {n} others", some.join(", ")),
    };
    format!("[SPOTTED BY {list}]")
}

/// The sneak word from the game's detection states (`fnv_game::awareness`) and the
/// latest detection values (`Game::detection`) [design: shared by drawing and tree].
/// Teammates and actors following the player (`Game::with_player`) count for neither
/// `[ALERT]` nor `[SPOTTED]` (#1159); one in Combat with the player still means
/// `[COMBAT]` [design: a fight is a fight].
pub(crate) fn sneak_now(g: &fnv_game::Game) -> Sneak {
    let me = FormId::PLAYER_REF;
    let (mut fighting, mut searching) = (false, false);
    for (id, a) in &g.awareness.actors {
        if g.is_dead(*id) {
            continue;
        }
        match a.state {
            Awake::Combat => fighting |= a.combat_target().is_some_and(|(t, _)| t == me),
            Awake::Alert | Awake::Lost => searching |= !g.with_player(*id) && (a.targets.contains_key(&me) || a.alarms.iter().any(|x| x.owner == me)),
            Awake::Normal => {}
        }
    }
    Sneak::of(fighting, searching, g.player_detected_for_sneak())
}

// ---- layout --------------------------------------------------------------------------

/// How many of a bar's `count` ticks are lit at `value` (0 to 1): rounded down for the
/// health and action points bars [observed: hud.md §14.3]. None at 0 or below, or for
/// non-finite values [design: bounded shares].
pub fn lit(value: f32, count: usize) -> usize {
    if value.is_finite() {
        ((value.clamp(0.0, 1.0) * count as f32).floor() as usize).min(count)
    } else {
        0
    }
}

/// The ticks lit of a bar that empties from both ends towards its middle (the enemy's
/// [observed: owner's playtest list 5, item 32]): from and to (exclusive), counted from
/// the left. An odd tick left over goes from the right end first [design: centred ticks].
/// The count is a tick for any part of its share, kept with the 31 ticks [hypothesis;
/// the original's differs, hud.md §8]: an actor still alive keeps a lit tick.
pub fn lit_middle(value: f32, count: usize) -> (usize, usize) {
    let n = if value.is_finite() { ((value.clamp(0.0, 1.0) * count as f32).ceil() as usize).min(count) } else { 0 };
    let from = (count - n) / 2;
    (from, from + n)
}

/// The condition fill's width in whole interface units [observed: hud.md §14.3].
fn condition_width(value: f32) -> f32 {
    (value.clamp(0.0, 1.0) * CND_FILL_WIDTH).round()
}

/// A tick's step and width in whole pixels for a view `height` pixels high, so that every
/// tick is as wide as the next.
pub fn tick_pixels(height: f32) -> (f32, f32) {
    let s = height / 960.0;
    let step = (TICK.x * s).round().max(2.0);
    (step, (TICK_BLOCK * s).round().clamp(1.0, step - 1.0))
}

/// Where a row's `count` ticks start, in whole pixels from the row's start, and their
/// width: the first tick at 0 and the last ending `length` pixels on, the ticks all
/// `width` wide (less only where they would touch), the gaps between them differing by a
/// pixel at most. A whole-pixel step of `round(8 x scale)` left the row up to a step and
/// more short of its bracket wherever 8 x scale is not whole (owner's playtest
/// 2026-10-16: "a gap at the end of each").
pub fn tick_lefts(length: f32, width: f32, count: usize) -> (Vec<f32>, f32) {
    if count < 2 {
        return (vec![0.0; count], width);
    }
    let gaps = (count - 1) as f32;
    // A gap of a pixel at least: (length - width) / gaps >= width + 1.
    let width = width.min(((length - gaps) / count as f32).floor()).max(1.0);
    let pitch = (length - width) / gaps;
    ((0..count).map(|i| (i as f32 * pitch).round()).collect(), width)
}

/// The compass strip fades out towards the window's right end: full to this x of the
/// window, nothing from that, linearly between [observed: hud_compare needs_off and
/// xp_meter at 1280 x 720, the original's strip line at about 0.68 of its colour over the
/// ground at x 266 pixels, 0.38 at 280, 0.21 at 290, 0.05 at 298, the letters there dim:
/// full at about 250 and gone at about 300.5 pixels, the window from 45 to 303.75; its
/// left end and the marks over it (the quest marker at SE, needs_limbs) are not faded;
/// hypothesis: linear]. Nothing in the menu files says how: `hud_compass_alphamap.dds` is
/// a white band of 20 texels and black [observed: the texture]. Both drawings bake the
/// window's strip into one image, each texel column's alpha on this ramp
/// (`crate::compass_strip`) [design]. Until T12e the strip ran at full strength to the
/// window's end [design].
pub const STRIP_FADE: (f32, f32) = (273.0, 341.0);

/// A bar's ticks in a row `room` pixels long, `step` and `width` from [`tick_pixels`]:
/// where the first stands, every tick's left from it, and their width. A full row runs
/// the whole room but the clear strip after its last tick, each tick's block at the start
/// of its step as in the picture (`hud_tick_mark.dds`, texels 6-7 clear) [observed: the
/// texture; §4: at 1080 lines the first health tick at the upright, the last 2 pixels short
/// of the mark]: the health and action points bars alike, so the action points' last tick
/// stands that strip clear of the upright [observed: hud_compare needs_off at 1280 x 720,
/// the original's last tick at x 1229-1232, a pixel clear of the upright at 1234]. Until
/// T12e the action points' row put the strip before its first tick instead, mirrored, and
/// its last tick met the upright [design: after "the last meets the upright", measured by
/// eye on the 1080-line `Target healthbar` screenshot]. The enemy's stand in the middle.
fn row_ticks(bar: Bar, room: f32, step: f32, width: f32, count: usize) -> (f32, Vec<f32>, f32) {
    let clear = (step - width).max(1.0);
    let (lefts, width) = tick_lefts((room - clear).round(), width, count);
    let first = match bar {
        Bar::Health | Bar::ActionPoints => 0.0,
        Bar::Enemy => (clear / 2.0).round(),
    };
    (first, lefts, width)
}

/// Where the doubled compass strip starts in its window (0 or less), so that `heading`
/// (degrees) stands at [`STRIP_HEADING`] [observed: hud.md §14.5].
pub fn strip_left(heading: f32) -> f32 {
    let at = heading.rem_euclid(360.0) / 360.0 * STRIP.x;
    -(at - STRIP_HEADING).rem_euclid(STRIP.x)
}

/// The ammo counter's text: rounds in the weapon / rounds carried besides.
pub fn ammo_text(ammo: Option<(u32, u32)>) -> String {
    match ammo {
        Some((loaded, carried)) => format!("{loaded}/{carried}"),
        None => String::new(),
    }
}

/// Where the pieces stand in their corner piece, from its top left corner.
#[derive(Debug, Clone, PartialEq)]
pub struct Places {
    /// The "HP" label's start, and the health bar's left end.
    pub hp_label: f32,
    pub hp_meter: f32,
    /// The action points bar's left end (it empties from the left), and the "AP" label's
    /// start.
    pub ap_meter: f32,
    pub ap_label: f32,
    /// The bars' top.
    pub meter_y: f32,
    /// The "CND" label's start, the condition bar's start and top (its bottom on the
    /// label's line), the arrows' left, and where the ammo's text ends.
    pub cnd_label: f32,
    pub cnd_x: f32,
    pub cnd_y: f32,
    pub arrows_x: f32,
    pub ammo_right: f32,
}

impl Places {
    pub fn new(font: &Font) -> Places {
        // [observed, §4] A full bar runs from the upright's inside edge to just short of
        // the bracket's corner mark; its label stands above it, near the upright.
        let hp_meter = LEFT_MARK - METER_WIDTH - METER_SHIFT;
        let ap_meter = RIGHT_MARK - AP_METER_SHIFT;
        let cnd_x = DIVIDER - CND_BEFORE_DIVIDER - CND_BAR.x;
        Places {
            hp_label: HP_LABEL_X,
            hp_meter,
            ap_meter,
            ap_label: AP_LABEL_END - font.width(AP),
            meter_y: LINE_Y - METER_GAP - TICK.y,
            cnd_label: cnd_x - CND_LABEL_GAP - font.width(CND),
            cnd_x,
            cnd_y: LOWER_BASE - CND_RAISE - CND_BAR.y,
            arrows_x: cnd_x + JAM_CONDITION * CND_BAR.x - ARROWS_MIDDLE * (CND_BAR.y + 2.0 * ARROWS_BLEED),
            ammo_right: RIGHT_UPRIGHT - 2.0 * LABEL_GAP,
        }
    }
}

/// Where this HUD sets its corner labels, for the tile tree's code-made copies while the HUD
/// is drawn from tiles (`crate::tiles::vanilla::write_corners`; the captures give none of
/// their places, hud.md §14.6) [design]: "HP"'s start, the ends of "AP", "CND" and the
/// ammo counter, each with the line it stands on, from its corner piece's top left (the
/// right piece's as drawn here, [`MARGIN_X`] in from the view's edge). The ammo
/// abbreviation is not among them: the tile HUD keeps `AmmoTypeLabel` at its captured
/// place, where the original draws it (T12e; this HUD's [`AMMO_LABEL_X`] is the owner's).
pub(crate) fn label_places(font: &Font) -> [(f32, f32); 4] {
    let p = Places::new(font);
    [(p.hp_label, UPPER_BASE), (AP_LABEL_END, UPPER_BASE), (p.cnd_x - CND_LABEL_GAP, LOWER_BASE), (p.ammo_right, LOWER_BASE)]
}

// ---- reading the game ------------------------------------------------------------------

/// Read what the HUD shows. Health and action points come from the script runtime's
/// actor values; until it runs they read full.
#[allow(clippy::too_many_arguments)]
fn gather(
    state: Res<State<PlayState>>,
    player: Option<Res<Player>>,
    combat: Option<Res<Combat>>,
    locks: Option<Res<ControlLocks>>,
    control: Option<Res<crate::control::PlayerControl>>,
    mut script: Option<ResMut<ScriptState>>,
    mut sim: ResMut<SimState>,
    mut view: ResMut<HudView>,
    (vats, wheel): (Option<Res<crate::vats_mode::VatsMode>>, Option<Res<crate::weapon_wheel::WeaponWheel>>),
    driven: Option<Res<crate::player::ViewDriven>>,
    kill_cam: Option<Res<crate::kill_cam::KillCam>>,
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
) {
    // [documented: GECK wiki DisablePlayerControls, the movement flag: "HUD is partially
    // disabled: Action Points, Health, Crosshair".]
    let held = locks.is_some_and(|l| l.movement);
    // Nor over the character creator (its camera is up), whatever state the opening
    // leaves the game in [owner 2026-10-17: "hide the UI so no health bar or AP or
    // dialogue"].
    let creating = control.as_ref().is_some_and(|c| c.portrait.is_some());
    // Nor while an idle holds the camera (the opening's wake-up, `ViewDriven`), whatever
    // the scripts' locks: only subtitles then [owner 2026-10-24]; nor while the weapon
    // wheel is open (docs/design/weapon-wheel.md §7) [design].
    let posed = driven.is_some_and(|d| d.0);
    let wheel_open = wheel.is_some_and(|w| w.is_open());
    // Nor while a kill camera plays (`crate::kill_cam`) [owner's playtest request #1034].
    // Nor while the classic VATS style plays its queued attacks (`VatsMode::playing_back`).
    let playback = vats.as_ref().is_some_and(|v| v.playing_back());
    let camera = kill_cam.is_some_and(|k| k.active());
    let killcam = camera || playback;
    if killcam != view.kill_cam {
        let why = if camera { "the kill camera has the view" } else { "the VATS attacks play" };
        info!("hud: {}", if killcam { format!("hidden: {why}") } else { "back after the kill camera".to_string() });
    }
    let hidden = if *state.get() != PlayState::Playing {
        Some("the game is not in play (a menu, a conversation or a loading screen)")
    } else if held {
        Some("a script holds the player's movement (DisablePlayerControls)")
    } else if creating {
        Some("the character creator is up")
    } else if posed {
        Some("an idle holds the camera")
    } else if wheel_open {
        Some("the weapon wheel is open")
    } else if killcam {
        Some("a kill camera or VATS playback has the view")
    } else {
        None
    };
    let mut now = HudView { shown: hidden.is_none(), kill_cam: killcam, hidden, ..view.clone() };
    // [design, #2659] Only the classic enemy piece survives playback, including its
    // attack camera. All other HUD visibility rules keep using `shown`.
    now.enemy_shown = *state.get() == PlayState::Playing && !held && !creating && !posed && !wheel_open
        && (!killcam || vats.as_ref().is_some_and(|v| v.style() == crate::vats_classic::VatsStyle::Classic && v.active()));
    if let Some(p) = &player {
        // Where the camera looks, not where the character faces (third person).
        now.heading = control.as_deref().map_or(p.heading, |c| c.view_heading(p.heading)).to_degrees();
    }
    // The player's tweak: the body armour's condition [design: docs/design/tweaks.md].
    now.armor = combat.as_ref().filter(|_| tweaks.as_ref().is_some_and(|t| t.game.armor_condition)).and_then(|c| c.hud_armor(&mut sim));
    if let Some(c) = &combat {
        now.weapon = c.hud_weapon(&sim);
        if let Some(v) = &vats {
            now.weapon.ammo = v.displayed_ammo(now.weapon.ammo);
        }
        // The player's tweak hides the ammo counter [design: docs/design/tweaks.md].
        if tweaks.as_ref().is_some_and(|t| t.hide_ammo_label) {
            now.weapon.ammo = None;
            now.weapon.abbrev = None;
        }
    }
    if now.weapon.ammo != view.weapon.ammo {
        info!("hud: ammo count {}", ammo_text(now.weapon.ammo));
    }
    // The player's tweak leaves the sneak label off [design: docs/design/tweaks.md].
    // The player's tweaks: the label off, or shown standing too [design: docs/design/tweaks.md].
    let standing = tweaks.as_ref().is_some_and(|t| t.sneak_label_standing);
    let sneaking = (control.is_some_and(|c| c.sneak) || standing) && !tweaks.as_ref().is_some_and(|t| t.no_sneak_label);
    if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
        now.sneak = sneaking.then(|| sneak_now(&rt.game));
        // The player's tweak names who detects the player [design: docs/design/tweaks.md].
        now.detected_by = tweaks.as_ref().filter(|t| t.detected_by_whom && now.sneak == Some(Sneak::Detected)).map(|t| {
            let names: Vec<String> = rt.game.player_detectors_for_sneak().into_iter().filter_map(|d| sim.ref_name(Some(&rt.game), d.0).map(str::to_string)).collect();
            detected_word(&names, t.detected_names as usize)
        });
        (now.health, now.action_points) = with_world(rt, &mut sim, |rt| {
            // An aid item's temporary modifier (Buffout's Health, Jet's Action Points) raises
            // the maximum with the value [hypothesis: the bar stays a share of the whole].
            let temporary = |g: &fnv_game::Game, code| fnv_game::functions::actor_value_temporary(g, FormId::PLAYER_REF, code);
            let max = rt.game.max_health(FormId::PLAYER_REF) + temporary(&rt.game, fnv_game::actor_values::HEALTH);
            let health = fnv_game::functions::actor_health(&mut rt.game, FormId::PLAYER_REF).unwrap_or(max);
            let health_share = if max > 0.0 { (health / max) as f32 } else { 0.0 };
            // Log tick changes even under the console [design: smoke hud-bars-partial].
            let n = lit(health_share, PIPS);
            if n != lit(view.health, PIPS) {
                info!("hud: bars health {n} of 37 ({health:.0} of {max:.0})");
            }
            // Base, permanent and temporary modifiers, damage: the bar is what is left of
            // the first three.
            let ap = match fnv_game::functions::actor_value_parts(&mut rt.game, FormId::PLAYER_REF, ACTION_POINTS) {
                Ok([base, modifier, damage]) => {
                    let whole = base + modifier + temporary(&rt.game, ACTION_POINTS);
                    // VATS mode's marks hold their AP: the bar shows what is left after
                    // them (docs/design/vats-mode.md §8) [design].
                    let reserved = vats.as_ref().map_or(0.0, |v| v.reserved() as f64);
                    let share = if whole > 0.0 {
                        ((whole - damage - reserved) / whole).max(0.0)
                    } else {
                        1.0
                    };
                    let n = lit(share as f32, PIPS);
                    if n != lit(view.action_points, PIPS) {
                        let current = (whole - damage - reserved).max(0.0);
                        info!("hud: bars action points {n} of 37 ({current:.0} of {whole:.0})");
                    }
                    share
                }
                _ => 1.0,
            };
            (health_share, ap as f32)
        });
    } else {
        now.sneak = sneaking.then_some(Sneak::Hidden);
    }
    if now.sneak != view.sneak && now.shown {
        match now.sneak {
            Some(s) => info!("hud: sneak {}", s.word()),
            None => info!("hud: sneak off"),
        }
    }
    if now.weapon.abbrev != view.weapon.abbrev && now.shown {
        info!("hud: ammo label {}", now.weapon.abbrev.as_deref().unwrap_or("(none)"));
    }
    view.set_if_neq(now);
}

// ---- drawing ---------------------------------------------------------------------------

/// The HUD's systems: what it shows is read, then drawn.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct HudSet;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HudView>()
            .add_plugins((crate::hud_target::HudTargetPlugin, crate::hud_limbs::HudLimbsPlugin, crate::hud_needs::HudNeedsPlugin, crate::hud_breath::HudBreathPlugin, crate::grenade_indicator::GrenadeIndicatorPlugin, crate::heartbeat::HeartbeatPlugin, crate::marker_added::MarkerAddedPlugin))
            .init_resource::<HudHidden>()
            .add_systems(Update, (gather, draw, pips, texts, enemy_colours, place_name, armor_row).chain().in_set(HudSet).in_set(PlaySet::Ui))
            .add_systems(PostUpdate, hide_parts.before(crate::tiles::vanilla::hide_hand_built).before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate));
    }
}

/// What the HUD is drawn with.
struct Art {
    font: Face,
    left_bracket: Option<Picture>,
    right_bracket: Option<Picture>,
    enemy_bracket: Option<Handle<Image>>,
    strip: Option<Picture>,
    /// The strip's texels for its baked window (`crate::compass_strip`), taken when the
    /// HUD is spawned.
    strip_texels: Option<StripTexels>,
    arrows: Option<Handle<Image>>,
}

/// The corner brackets and the compass strip are their pieces of the interface's atlas
/// (`interfaceshared.tai`), as the HUD's files name them with `texatlas` [documented:
/// hudtemplates.xml, `template_left_bracket`, `template_right_bracket`,
/// `template_compass_window`], each stretched over its box: the original draws those
/// pieces, with their stepped end caps, the health underline's hard end and the strip's
/// thick bars between the letters [observed: hud_compare at 1280 x 720 against the tile
/// HUD, which draws the pieces; the private shots `hud_corners2` c0, c4, T12c]; their own
/// files are other art (square corners, single lines, a triangle cap, evenly spaced
/// ticks) [observed: menu-xml.md §3.1.3]. Until T12e this HUD drew the own files [design:
/// no owner request]. Without the atlas, the own files.
fn load_art(data: Option<&GameData>, images: &mut Assets<Image>, bc: bool) -> Art {
    let mut pictures = Pictures::load(data);
    let mut piece = |name: &str| pictures.picture(&format!("interface\\hud\\{name}.dds"), data, images, bc);
    let (left_bracket, right_bracket) = (piece("glow_hud_left_seperatorglow"), piece("glow_hud_right_seperatorglow"));
    let strip_name = "interface\\hud\\glow_hud_comp_direction_strip.dds";
    let strip = piece("glow_hud_comp_direction_strip");
    let strip_texels = match (&strip, data) {
        (Some((_, rect)), Some(data)) => StripTexels::read(&mut pictures, strip_name, None, *rect, &mut Repeats::default(), data),
        _ => None,
    };
    let mut read = |path: &str| data.and_then(|d| texture(path, d, images, bc)).map(|t| t.0);
    let hud = |name: &str| format!("{HUD_TEXTURES}{name}.dds");
    let enemy_bracket = read(&hud("glow_enemy_health_seperator"));
    let arrows = read(CND_ARROWS);
    Art { font: face(FONT, plain_font(36.0, 15.0), data, images, bc), left_bracket, right_bracket, enemy_bracket, strip, strip_texels, arrows }
}

/// A picture over a node's box: its atlas piece's part of the texture, stretched.
fn picture_node(picture: &Picture, color: Color) -> ImageNode {
    ImageNode { color, rect: picture.1, image_mode: NodeImageMode::Stretch, ..ImageNode::new(picture.0.clone()) }
}

#[derive(Component)]
pub(crate) struct HudRoot;

/// The health and compass pieces, bottom left (separately scaled, hidden together).
#[derive(Component)]
struct HpPiece;

/// The name of the place ahead, over the health and compass piece (the HUD Marker Name
/// tweak).
#[derive(Component)]
struct PlaceNameText;

/// Game units in a metre [observed: 70, as the physics takes it (`fnv-phys`)].
const UNITS_PER_METRE: f32 = 70.0;

/// The words for the place the player faces under the HUD Marker Name tweak: of the map
/// markers `(place, name, found)` within `range` of `here`, the one whose bearing is
/// nearest `heading` (degrees, 0 north) and within `max_angle` of it, places not found
/// left out with `hide_unfound`; its name, with its distance in metres (`units` 1) or feet
/// (2) [design: docs/design/tweaks.md].
pub fn place_ahead<'a>(markers: impl IntoIterator<Item = ([f32; 2], &'a str, bool)>, here: [f32; 2], heading: f32, range: f32, max_angle: f32, hide_unfound: bool, units: u8) -> Option<String> {
    let mut best: Option<(f32, f32, &str)> = None;
    for (at, name, found) in markers {
        let away = (at[0] - here[0]).hypot(at[1] - here[1]);
        if name.trim().is_empty() || (hide_unfound && !found) || away > range {
            continue;
        }
        let off = (crate::compass::offset(heading, here, at) * 360.0 / STRIP.x).abs();
        if off <= max_angle && best.is_none_or(|b| off < b.0) {
            best = Some((off, away, name));
        }
    }
    let (_, away, name) = best?;
    let metres = away / UNITS_PER_METRE;
    Some(match units {
        1 => format!("{name}  {metres:.0} m"),
        2 => format!("{name}  {:.0} ft", metres * 3.2808),
        _ => name.to_string(),
    })
}

/// The HUD Marker Name tweak's line: the place ahead (`place_ahead`, among the markers the
/// compass would show: in the worldspace outside, within its reach), centred over the
/// compass's heading point above the piece [design: docs/design/tweaks.md].
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn place_name(
    mut commands: Commands,
    view: Res<HudView>,
    (tweaks, markers, location, player, control): (Option<Res<crate::tweaks::TweakSettings>>, Option<Res<crate::pipboy::data::DataState>>, Option<Res<crate::Location>>, Option<Res<Player>>, Option<Res<crate::control::PlayerControl>>),
    (sim, script): (Res<SimState>, Option<Res<crate::script::ScriptState>>),
    (data, mut images, formats): (Option<Res<GameData>>, ResMut<Assets<Image>>, Option<Res<CompressedImageFormatSupport>>),
    mut face_: Local<Option<Face>>,
    (mut shown, mut region): (Local<(Option<String>, Option<String>)>, Local<(Option<fnv_sim::FormId>, Option<String>)>),
    text: Query<Entity, With<PlaceNameText>>,
    mut font_revision: Local<u32>,
) {
    let _hud = crate::interface::hud_units();
    let Ok(e) = text.single() else { return };
    let words = (|| {
        let t = tweaks.as_deref().filter(|t| t.hud_marker_name && view.shown)?;
        let (st, loc, player) = (markers.as_deref()?, location.as_deref().filter(|l| l.interior.is_none())?, player.as_deref()?);
        let heading = control.as_deref().map_or(player.heading, |c| c.view_heading(player.heading)).to_degrees();
        let range = crate::compass::place_range();
        let game = script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game);
        let list = st.named_markers_of(loc.world, game);
        let list = list.iter().filter(|m| sim.present(m.0)).map(|m| ([m.1[0], m.1[1]], m.2, m.3));
        place_ahead(list, [player.feet[0], player.feet[1]], heading, range, t.marker_name_angle, t.marker_name_hide_unfound, t.marker_name_units)
    })();
    // The HUD Region Names tweak: the map name of the region the player stands in, kept
    // per cell [design: docs/design/tweaks.md].
    let region_words = (|| {
        tweaks.as_deref().filter(|t| t.hud_region_names && view.shown)?;
        let game = &script.as_deref()?.rt.as_ref()?.game;
        let cell = game.cell_of(fnv_sim::FormId::PLAYER_REF);
        if region.0 != cell || cell.is_none() {
            *region = (cell, fnv_game::nvse_jg5::player_region_name(game));
        }
        region.1.clone()
    })();
    let now = (region_words, words);
    let revision = crate::cjk_font::revision();
    let font_changed = std::mem::replace(&mut *font_revision, revision) != revision;
    if *shown == now && !font_changed {
        return;
    }
    if now.0 != shown.0 {
        info!("hud: region name {:?}", now.0);
    }
    *shown = now.clone();
    let (region_words, words) = now;
    let face_ = face_.get_or_insert_with(|| face(FONT, plain_font(36.0, 15.0), data.as_deref(), &mut images, bc_supported(formats.as_deref())));
    commands.entity(e).despawn_related::<Children>().with_children(|p| {
        let marker = words.as_deref().and_then(|w| face_.font.lines(w, None).into_iter().next());
        // The region's line over the marker's when both show [design].
        let region_y = if marker.is_some() { -28.0 } else { -8.0 };
        if let Some(line) = marker {
            set_line(p, face_, &line, COMPASS_AT.x + COMPASS_HEADING - line.width / 2.0, -8.0, amber(255.0));
        }
        if let Some(line) = region_words.as_deref().and_then(|w| face_.font.lines(w, None).into_iter().next()) {
            set_line(p, face_, &line, COMPASS_AT.x + COMPASS_HEADING - line.width / 2.0, region_y, amber(255.0));
        }
    });
}

/// The action points, condition and ammo piece, bottom right.
#[derive(Component)]
struct ApPiece;

/// The parts of the HUD the player's Hide HUD key has hidden: `on` while hidden, and which
/// parts it hides (the tweak's switches, set by `crate::tweaks`) [design:
/// docs/design/tweaks.md].
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq)]
pub struct HudHidden {
    pub on: bool,
    pub compass: bool,
    pub ammo: bool,
    pub needs: bool,
    pub sneak: bool,
}

impl HudHidden {
    /// The bottom-left and bottom-right pieces' visibility, which nothing else sets but
    /// the tiles' hiding (after this).
    fn corner(&self, part: bool) -> Visibility {
        if self.on && part {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        }
    }
}

/// The Hide HUD key's parts hidden, after every drawer has set its pieces: the corner
/// pieces and the sneak word given back (`Inherited`) otherwise, as nothing else sets
/// theirs but the tiles' hiding (after this); the needs words' drawer shows them again
/// itself.
#[allow(clippy::type_complexity)]
fn hide_parts(
    hidden: Res<HudHidden>,
    mut hp: Query<&mut Visibility, (With<HpPiece>, Without<ApPiece>, Without<SneakText>, Without<crate::tiles::vanilla::WordsRoot>)>,
    mut ap: Query<&mut Visibility, (With<ApPiece>, Without<HpPiece>, Without<SneakText>, Without<crate::tiles::vanilla::WordsRoot>)>,
    mut sneak: Query<&mut Visibility, (With<SneakText>, Without<HpPiece>, Without<ApPiece>, Without<crate::tiles::vanilla::WordsRoot>)>,
    mut words: Query<&mut Visibility, (With<crate::tiles::vanilla::WordsRoot>, Without<HpPiece>, Without<ApPiece>, Without<SneakText>)>,
) {
    for mut v in &mut hp {
        v.set_if_neq(hidden.corner(hidden.compass));
    }
    for mut v in &mut ap {
        v.set_if_neq(hidden.corner(hidden.ammo));
    }
    for mut v in &mut sneak {
        v.set_if_neq(hidden.corner(hidden.sneak));
    }
    if hidden.on && hidden.needs {
        for mut v in &mut words {
            v.set_if_neq(Visibility::Hidden);
        }
    }
}

#[derive(Component, Clone, Copy, PartialEq, Debug)]
enum Bar {
    Health,
    ActionPoints,
    Enemy,
}

/// The weapon condition's bar.
#[derive(Component)]
struct ConditionBar;

/// The Armor Condition tweak's row (label and bar), under the weapon's, and its fill.
#[derive(Component)]
struct ArmorRow;
#[derive(Component)]
struct ArmorBar;

/// The armour row's label [design: our own word, beside the game's "CND"] and how far
/// under the weapon's row it stands [design].
const ARM: &str = "ARM";
const ARMOR_DROP: f32 = 26.0;

/// What the Armor Condition tweak's row shows at `condition` (0 to 1): shown at all (below
/// `visible_below` when that is above 0), red (below `red_below`, or flashing below
/// `flash_below` in the half of each second `flash_on` says) [design: docs/design/tweaks.md].
pub fn armor_look(condition: f32, visible_below: f32, red_below: f32, flash_below: f32, flash_on: bool) -> (bool, bool) {
    let shown = visible_below <= 0.0 || condition < visible_below;
    let flashing = flash_below > 0.0 && condition < flash_below;
    (shown, (red_below > 0.0 && condition < red_below) || (flashing && flash_on))
}

/// The Armor Condition tweak's row: the body armour's condition bar under the weapon's,
/// or in its place with no weapon under In the Weapon's Place.
#[allow(clippy::type_complexity)]
fn armor_row(
    view: Res<HudView>,
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
    time: Res<Time<Real>>,
    mut row: Query<(&mut Visibility, &mut Node), (With<ArmorRow>, Without<ArmorBar>)>,
    mut bar: Query<(&mut Node, &mut BackgroundColor), (With<ArmorBar>, Without<ArmorRow>)>,
) {
    let Some(t) = tweaks.as_deref() else { return };
    let look = view.armor.map(|c| (c, armor_look(c, t.armor_visible_below, t.armor_red_below, t.armor_flash_below, time.elapsed_secs().fract() < 0.5)));
    let drop = if view.weapon.condition.is_none() && t.armor_in_weapon_place { 0.0 } else { ARMOR_DROP };
    for (mut v, mut node) in &mut row {
        v.set_if_neq(if look.is_some_and(|(_, (shown, _))| shown) { Visibility::Inherited } else { Visibility::Hidden });
        if node.top != u(drop) {
            node.top = u(drop);
        }
    }
    let Some((c, (_, red))) = look else { return };
    let width = u(condition_width(c));
    let colour = if red { crime_colour() } else { amber(CONDITION_BRIGHTNESS) };
    for (mut node, mut background) in &mut bar {
        if node.width != width {
            node.width = width;
        }
        if background.0 != colour {
            background.0 = colour;
        }
    }
}

/// A bar's row of ticks: which bar, its colour, and how the ticks stand in it.
#[derive(Component, Clone, Copy)]
struct PipRow {
    bar: Bar,
    color: Color,
    /// The row's width (interface units) and its tick count.
    width: f32,
    count: usize,
}

/// A tick of a bar, `index` from the bar's left end.
#[derive(Component)]
struct Pip {
    bar: Bar,
    index: usize,
}

/// The compass window's strip: one baked image, or the doubled strip moved by the
/// heading without its texels (`crate::compass_strip`).
#[derive(Component)]
struct Strip(StripNode);

/// The marks over the compass (`crate::compass` fills it): the window's place and size,
/// clipped at its sides only, as the marks reach above and below the strip.
#[derive(Component)]
pub struct CompassMarks;

/// The actors' ticks, under the bracket and the strip: the original draws both over them,
/// their dark halos shading the bar's top and end [observed: hud.md §11]. As
/// [`CompassMarks`] otherwise.
#[derive(Component)]
pub struct CompassTicks;

/// The lower row of the right piece: the condition's label and bar.
#[derive(Component)]
struct ConditionRow;

/// Holds the ammo counter's letters.
#[derive(Component)]
struct AmmoText;

/// The node that holds the enemy name's letters.
#[derive(Component)]
struct EnemyName;
/// Holds Living Anatomy's line under the name (`crate::hud_target::TargetHealth::anatomy_line`).
#[derive(Component)]
struct EnemyStats;

/// Holds the sneak word's letters.
#[derive(Component)]
struct SneakText;

/// A bar's row of ticks at `x`, `y` (`width` long); the ticks are set in it on whole
/// pixels by [`pips`].
fn spawn_row(p: &mut ChildSpawnerCommands, bar: Bar, x: f32, y: f32, width: f32, count: usize, color: Color) {
    p.spawn((PipRow { bar, color, width, count }, at(x, y, width, TICK.y)));
}

/// A bracket: the game's picture `x` right of its piece's corner, or plain lines where its
/// lines are.
fn spawn_bracket(p: &mut ChildSpawnerCommands, image: &Option<Picture>, x: f32, uprights: &[f32], color: Color) {
    match image {
        Some(image) => {
            p.spawn((at(x, 0.0, BRACKET.x, BRACKET.y), picture_node(image, color)));
        }
        None => {
            p.spawn((at(LEFT_UPRIGHT, LINE_Y, RIGHT_UPRIGHT - LEFT_UPRIGHT, 2.0), BackgroundColor(color)));
            for &x in uprights {
                p.spawn((at(x, if x == DIVIDER { LINE_Y } else { 10.0 }, 2.0, if x == DIVIDER { 55.0 } else { 115.0 }), BackgroundColor(color)));
            }
        }
    }
}

fn spawn_hud(commands: &mut Commands, art: &mut Art, images: &mut Assets<Image>, heading: f32) {
    let window = art.strip_texels.take().and_then(|t| StripWindow::new(t, COMPASS.x, heading, images));
    let art = &*art;
    let places = Places::new(&art.font.font);
    let color = amber(255.0);
    let font = &art.font.font;
    let label = |p: &mut ChildSpawnerCommands, text: &str, x: f32, base: f32| {
        set_line(p, &art.font, &font.lines(text, None).remove(0), x, base, color);
    };
    let screen = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
    commands.spawn((HudRoot, screen.clone(), Visibility::Hidden, Pickable::IGNORE)).with_children(|root| {
        // Health and compass, bottom left.
        let piece = Node { position_type: PositionType::Absolute, left: u(MARGIN_X), bottom: u(MARGIN_Y), width: u(HP_PANEL.x), height: u(HP_PANEL.y), ..default() };
        root.spawn((screen.clone(), Pickable::IGNORE, crate::hud_readability::HudSize(crate::hud_readability::Part::Health))).with_children(|wrapper| {
          wrapper.spawn((HpPiece, piece.clone(), DrawnByTiles)).with_children(|p| {
            spawn_bracket(p, &art.left_bracket, 0.0, &[LEFT_UPRIGHT], color);
            label(p, HP, places.hp_label, UPPER_BASE);
            spawn_row(p, Bar::Health, places.hp_meter, places.meter_y, METER_WIDTH, PIPS, amber(METER_BRIGHTNESS));
          });
        });
        // The compass is a sibling so changing health size cannot resize its marks.
        root.spawn((screen.clone(), Pickable::IGNORE, crate::hud_readability::HudSize(crate::hud_readability::Part::Compass))).with_children(|wrapper| {
          wrapper.spawn((HpPiece, piece, DrawnByTiles)).with_children(|p| {
            let mut marks = at(COMPASS_AT.x, COMPASS_AT.y, COMPASS.x, COMPASS.y);
            marks.overflow = Overflow::clip_x();
            p.spawn((CompassTicks, marks.clone()));
            info!("compass: heading at x {x} of {w}, the strip's N at {n}, fading from {f}", x = COMPASS_HEADING, w = COMPASS.x, n = STRIP_HEADING, f = STRIP_FADE.0);
            let mut window_node = at(COMPASS_AT.x, COMPASS_AT.y, COMPASS.x, COMPASS.y);
            window_node.overflow = Overflow::clip();
            p.spawn(window_node).with_children(|w| match window {
                // One image of the window's strip, its right end faded
                // (`crate::compass_strip`).
                Some(window) => {
                    let (tw, th) = window.texels();
                    info!("compass: the strip baked into one window image of {tw} x {th} texels, faded from {f} to {t}", f = STRIP_FADE.0, t = STRIP_FADE.1);
                    let size = window.size();
                    let image = ImageNode { color, image_mode: NodeImageMode::Stretch, ..ImageNode::new(window.image()) };
                    w.spawn((Strip(StripNode::Baked(window)), at(0.0, 0.0, size.x, size.y), image));
                }
                // Without the strip's texels: the doubled strip moved by the heading,
                // unfaded.
                None => {
                    info!("compass: the strip drawn unfaded (its texels not read)");
                    w.spawn((Strip(StripNode::Moving), at(strip_left(heading), 0.0, 2.0 * STRIP.x, STRIP.y))).with_children(|s| {
                        if let Some(image) = &art.strip {
                            for i in 0..2 {
                                s.spawn((at(i as f32 * STRIP.x, 0.0, STRIP.x, STRIP.y), picture_node(image, color)));
                            }
                        } else {
                            // The eight directions' letters, 45 degrees apart.
                            for i in 0..16 {
                                let name = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"][i % 8];
                                let line = font.lines(name, None).remove(0);
                                let x = i as f32 * STRIP.x / 8.0 - line.width / 2.0;
                                set_line(s, &art.font, &line, x, 44.0, color);
                            }
                        }
                    });
                }
            });
            p.spawn((CompassMarks, marks));
            p.spawn((PlaceNameText, at(0.0, 0.0, HP_PANEL.x, 0.0)));
          });
        });
        // Action points, condition and ammo, bottom right.
        let piece = Node { position_type: PositionType::Absolute, right: u(MARGIN_X), bottom: u(MARGIN_Y), width: u(AP_PANEL.x), height: u(AP_PANEL.y), ..default() };
        root.spawn((screen.clone(), Pickable::IGNORE, crate::hud_readability::HudSize(crate::hud_readability::Part::ActionPoints))).with_children(|wrapper| {
          wrapper.spawn((ApPiece, piece, DrawnByTiles)).with_children(|p| {
            spawn_bracket(p, &art.right_bracket, RIGHT_BRACKET_X, &[RIGHT_UPRIGHT, DIVIDER], color);
            spawn_row(p, Bar::ActionPoints, places.ap_meter, places.meter_y, METER_WIDTH, PIPS, amber(METER_BRIGHTNESS));
            label(p, AP, places.ap_label, UPPER_BASE);
            p.spawn((ConditionRow, at(0.0, 0.0, DIVIDER, AP_PANEL.y), Visibility::Inherited)).with_children(|row| {
                label(row, CND, places.cnd_label, LOWER_BASE);
                row.spawn((at(places.cnd_x, places.cnd_y, CND_BAR.x, CND_BAR.y), BackgroundColor(amber(GROUND_BRIGHTNESS))));
                row.spawn((ConditionBar, at(places.cnd_x, places.cnd_y, CND_FILL_WIDTH, CND_BAR.y), BackgroundColor(amber(CONDITION_BRIGHTNESS))));
                // The jamming threshold's two arrows, over the bar (§4).
                match &art.arrows {
                    Some(image) => {
                        let mut clip = at(places.cnd_x, places.cnd_y, CND_BAR.x, CND_BAR.y);
                        clip.overflow = Overflow::clip();
                        let side = CND_BAR.y + 2.0 * ARROWS_BLEED;
                        row.spawn(clip).with_children(|c| {
                            c.spawn((at(places.arrows_x - places.cnd_x, -ARROWS_BLEED, side, side), ImageNode::new(image.clone())));
                        });
                    }
                    None => {
                        let x = places.cnd_x + JAM_CONDITION * CND_BAR.x - 2.0;
                        row.spawn((at(x, places.cnd_y, 4.0, 5.0), BackgroundColor(Color::BLACK)));
                        row.spawn((at(x, places.cnd_y + CND_BAR.y - 5.0, 4.0, 5.0), BackgroundColor(Color::BLACK)));
                    }
                }
            });
            p.spawn((ArmorRow, at(0.0, ARMOR_DROP, DIVIDER, AP_PANEL.y), Visibility::Hidden)).with_children(|row| {
                label(row, ARM, places.cnd_label, LOWER_BASE);
                row.spawn((at(places.cnd_x, places.cnd_y, CND_BAR.x, CND_BAR.y), BackgroundColor(amber(GROUND_BRIGHTNESS))));
                row.spawn((ArmorBar, at(places.cnd_x, places.cnd_y, CND_FILL_WIDTH, CND_BAR.y), BackgroundColor(amber(CONDITION_BRIGHTNESS))));
            });
            p.spawn((AmmoText, at(0.0, 0.0, AP_PANEL.x, AP_PANEL.y)));
          });
        });
        // The sneak word, top middle.
        let piece = Node { position_type: PositionType::Absolute, left: Val::Percent(50.0), margin: UiRect::left(u(-SNEAK.x / 2.0)), top: u(0.0), width: u(SNEAK.x), height: u(SNEAK.y + SNEAK_BASE), ..default() };
        root.spawn((SneakText, piece, DrawnByTiles));
    });
    // [design, #2659] Independent of the corner root hidden during playback.
    commands.spawn((screen, Pickable::IGNORE)).with_children(|root| {
        // The enemy's health, bottom middle: its long line level with the corner pieces'
        // (§8).
        let red = crime_colour();
        let enemy_top = enemy_top();
        let piece = Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(50.0),
            margin: UiRect::left(u(-ENEMY.x / 2.0)),
            bottom: u(MARGIN_Y + enemy_top),
            width: u(ENEMY.x),
            height: u(ENEMY.y),
            ..default()
        };
        root.spawn((EnemyPiece, piece, Visibility::Hidden)).with_children(|p| {
            let (bx, by) = (ENEMY_BRACKET_AT.x, ENEMY_BRACKET_AT.y);
            match &art.enemy_bracket {
                Some(image) => {
                    p.spawn((at(bx, by, ENEMY_BRACKET.x, ENEMY_BRACKET.y), ImageNode { color: red, ..ImageNode::new(image.clone()) }));
                }
                None => {
                    p.spawn((at(bx + ENEMY_MARKS.0 - 20.0, by + ENEMY_LINE_Y, ENEMY_MARKS.1 - ENEMY_MARKS.0 + 40.0, 2.0), BackgroundColor(red)));
                }
            }
            let (from, to) = (bx + ENEMY_MARKS.0, bx + ENEMY_MARKS.1);
            spawn_row(p, Bar::Enemy, from, by + ENEMY_LINE_Y - ENEMY_METER_GAP - TICK.y, to - from, ENEMY_PIPS, red);
            p.spawn((EnemyName, at(0.0, 0.0, ENEMY.x, ENEMY.y)));
            p.spawn((EnemyStats, at(0.0, 0.0, ENEMY.x, ENEMY.y)));
        });
    });
}

/// The enemy piece's place from the HUD's bottom margin: its long line level with the
/// corner pieces' (§8).
fn enemy_top() -> f32 {
    HP_PANEL.y - LINE_Y + ENEMY_LINE_Y + ENEMY_BRACKET_AT.y - ENEMY.y
}

/// How strongly a tick of the enemy bar is drawn that the damage in sight would take
/// (VATS, `crate::hud_target::TargetHealth::after`) [design].
const ENEMY_SIGHT_ALPHA: f32 = 0.08;
/// They go from that to whole and back once in this many real seconds [design: the fork
/// owner 2026-11-10, "make them blink by going from the minimum to max opacity"].
const ENEMY_SIGHT_BLINK: f32 = 1.0;

/// How strongly a tick the damage in sight would take is drawn `at` real seconds.
pub fn sight_alpha(at: f32) -> f32 {
    let wave = 0.5 - 0.5 * (at / ENEMY_SIGHT_BLINK * std::f32::consts::TAU).cos();
    ENEMY_SIGHT_ALPHA + (1.0 - ENEMY_SIGHT_ALPHA) * wave
}

/// [design] Dead Eye's enemy piece top, in interface units; classic uses the bottom.
const ENEMY_VATS_TOP: f32 = 40.0;

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw(
    mut commands: Commands,
    view: Res<HudView>,
    target: Res<HudTarget>,
    data: Option<Res<GameData>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    mut art: Local<Option<Art>>,
    mut root: Query<&mut Visibility, (With<HudRoot>, Without<ConditionRow>, Without<EnemyPiece>)>,
    mut condition: Query<&mut Visibility, (With<ConditionRow>, Without<HudRoot>, Without<EnemyPiece>)>,
    mut enemy: Query<(&mut Visibility, &mut Node), (With<EnemyPiece>, Without<HudRoot>, Without<ConditionRow>, Without<ConditionBar>, Without<Strip>)>,
    mut bar: Query<(&mut Node, &mut BackgroundColor), (With<ConditionBar>, Without<Strip>, Without<EnemyPiece>)>,
    mut strip: Query<(&mut Strip, &mut Node), (Without<ConditionBar>, Without<EnemyPiece>)>,
    (tweaks, time, vats): (Option<Res<crate::tweaks::TweakSettings>>, Res<Time<Real>>, Option<Res<crate::vats_mode::VatsMode>>),
    mut classic_bar: Local<Option<bool>>,
) {
    // The HUD's lengths at the UI scale (`crate::interface::hud_units`, #1631).
    let _hud = crate::interface::hud_units();
    if art.is_none() {
        // The first frame the HUD is wanted: read the game's pictures and set it up.
        if view.shown {
            let mut loaded = load_art(data.as_deref(), &mut images, bc_supported(formats.as_deref()));
            spawn_hud(&mut commands, &mut loaded, &mut images, view.heading);
            *art = Some(loaded);
        }
        return;
    }
    let show = |on: bool| if on { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in &mut root {
        v.set_if_neq(show(view.shown));
    }
    for mut v in &mut condition {
        v.set_if_neq(show(view.weapon.condition.is_some()));
    }
    // [design, #2659] Classic uses the normal bottom anchor, including playback;
    // Dead Eye retains its existing top placement.
    let at_top = vats.as_ref().is_some_and(|v| v.style() == crate::vats_classic::VatsStyle::DeadEye && v.active());
    let (top, bottom) = if at_top { (u(ENEMY_VATS_TOP), Val::Auto) } else { (Val::Auto, u(MARGIN_Y + enemy_top())) };
    for (mut v, mut node) in &mut enemy {
        v.set_if_neq(show(view.enemy_shown && target.0.is_some()));
        if node.top != top || node.bottom != bottom {
            node.top = top;
            node.bottom = bottom;
        }
    }
    let classic = vats.as_ref().filter(|v| v.style() == crate::vats_classic::VatsStyle::Classic && v.active()).map(|v| v.playing_back());
    if classic != *classic_bar {
        if let Some(playback) = classic {
            info!("hud: classic VATS enemy bar bottom centre ({})", if playback { "playback" } else { "target screen" });
        }
        *classic_bar = classic;
    }
    if !view.shown {
        return;
    }
    let cnd = view.weapon.condition.unwrap_or(0.0);
    let width = u(condition_width(cnd));
    // The player's tweak: the weapon's condition bar red when low, flashing when lower
    // [design: docs/design/tweaks.md].
    let (red_below, flash_below) = tweaks.as_ref().map_or((0.0, 0.0), |t| (t.weapon_cnd_red_below, t.weapon_cnd_flash_below));
    let flashing = flash_below > 0.0 && cnd < flash_below;
    let red = (red_below > 0.0 && cnd < red_below) || (flashing && time.elapsed_secs().fract() < 0.5);
    let colour = if red { crime_colour() } else { amber(CONDITION_BRIGHTNESS) };
    for (mut node, mut background) in &mut bar {
        if node.width != width {
            node.width = width;
        }
        if background.0 != colour {
            background.0 = colour;
        }
    }
    for (mut strip, node) in &mut strip {
        strip.0.follow(view.heading, node, &mut images);
    }
}

/// Set the bars' ticks on whole pixels when the view's size changes, and light those the
/// values call for.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn pips(
    mut commands: Commands,
    view: Res<HudView>,
    target: Res<HudTarget>,
    data: Option<Res<GameData>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    rows: Query<(Entity, &PipRow)>,
    mut ticks: Query<(&Pip, &mut Visibility, Option<&mut ImageNode>, Option<&mut BackgroundColor>)>,
    real: Res<Time<Real>>,
    mut set_for: Local<Option<(f32, f32)>>,
    mut tick: Local<Option<Option<Handle<Image>>>>,
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
) {
    if !view.shown && !view.enemy_shown {
        return;
    }
    let Ok(window) = windows.single() else { return };
    // The ticks are set for the HUD's height at the UI scale: the view's at 100% (#1631).
    let key = (window.physical_height() as f32 * crate::interface::ui_scale(), window.scale_factor());
    if rows.is_empty() {
        return;
    }
    if *set_for != Some(key) {
        let image = tick
            .get_or_insert_with(|| data.as_deref().and_then(|d| texture(&format!("{HUD_TEXTURES}hud_tick_mark.dds"), d, &mut images, bc_supported(formats.as_deref()))).map(|t| t.0))
            .clone();
        let (height, sf) = key;
        let (step, width) = tick_pixels(height);
        let px = |v: f32| Val::Px(v / sf);
        for (e, row) in &rows {
            let (first, lefts, width) = row_ticks(row.bar, row.width * height / 960.0, step, width, row.count);
            commands.entity(e).despawn_related::<Children>().with_children(|r| {
                for (index, left) in lefts.iter().enumerate() {
                    let node = Node { position_type: PositionType::Absolute, left: px(first + left), top: Val::Px(0.0), width: px(width), height: Val::Percent(100.0), ..default() };
                    let pip = Pip { bar: row.bar, index };
                    match &image {
                        // The picture's block: its top part, as high as a tick.
                        Some(image) => r.spawn((pip, node, ImageNode { rect: Some(Rect::new(0.0, 0.0, TICK_BLOCK, TICK.y)), color: row.color, ..ImageNode::new(image.clone()) })),
                        None => r.spawn((pip, node, BackgroundColor(row.color))),
                    };
                }
            });
        }
        *set_for = Some(key);
        return;
    }
    let enemy = target.0.as_ref().map_or(0.0, |t| t.health);
    // The player's tweak: the HP and AP bars red when low [design: docs/design/tweaks.md].
    let (red_hp, red_ap) = tweaks.as_ref().map_or((0.0, 0.0), |t| (t.red_hp_below, t.red_ap_below));
    let low = |value: f32, below: f32| below > 0.0 && value < below;
    let (hp_red, ap_red) = (low(view.health, red_hp), low(view.action_points, red_ap));
    // In VATS the ticks the damage in sight would take are faint (`TargetHealth::after`):
    // the bar empties towards its middle, so they are its outer lit ticks.
    let after = target.0.as_ref().and_then(|t| t.after).map(|a| lit_middle(a, ENEMY_PIPS));
    for (pip, mut v, mut image, mut fill) in &mut ticks {
        let red = match pip.bar {
            Bar::Health => Some(hp_red),
            Bar::ActionPoints => Some(ap_red),
            Bar::Enemy => None,
        };
        if let Some(red) = red {
            set_tick_colour(image.as_deref_mut(), fill.as_deref_mut(), if red { crime_colour() } else { amber(METER_BRIGHTNESS) });
        }
        let on = match pip.bar {
            Bar::Health => pip.index < lit(view.health, PIPS),
            Bar::ActionPoints => PIPS - 1 - pip.index < lit(view.action_points, PIPS),
            Bar::Enemy => {
                let (from, to) = lit_middle(enemy, ENEMY_PIPS);
                (from..to).contains(&pip.index)
            }
        };
        v.set_if_neq(if on { Visibility::Inherited } else { Visibility::Hidden });
        if pip.bar == Bar::Enemy {
            // They blink, from faint to whole and back, in real time (the world may stand
            // still).
            let alpha = if after.is_some_and(|(from, to)| !(from..to).contains(&pip.index)) { sight_alpha(real.elapsed_secs()) } else { 1.0 };
            set_tick_alpha(image, fill, alpha);
        }
    }
}

/// `colour` at a brightness (0 to 255, as the interface files give it), its alpha kept.
fn at_brightness(colour: Color, brightness: f32) -> Color {
    let c = colour.to_srgba();
    let b = brightness / 255.0;
    Color::srgba(c.red * b, c.green * b, c.blue * b, c.alpha)
}

/// An enemy tick's strength (`alpha`, the VATS blink): on its picture when it has one,
/// else on its fill (the stand-in block drawn when the picture is missing). A pictured
/// tick's fill is the transparent background every UI node carries and stays so: made
/// opaque it was a solid block behind the picture, hiding the picture's fade, and the
/// enemy's ticks stood solid while the health and action points ticks faded towards their
/// tops [observed: smoke hud-enemy-bar-hurt-neutral, #2696].
fn set_tick_alpha(image: Option<impl DerefMut<Target = ImageNode>>, fill: Option<impl DerefMut<Target = BackgroundColor>>, alpha: f32) {
    match (image, fill) {
        (Some(mut image), _) => {
            if image.color.alpha() != alpha {
                image.color = image.color.with_alpha(alpha);
            }
        }
        (None, Some(mut fill)) => {
            if fill.0.alpha() != alpha {
                fill.0 = fill.0.with_alpha(alpha);
            }
        }
        (None, None) => {}
    }
}

/// A health or action points tick's colour (amber, or red under the low-bar tweak): on
/// its picture when it has one, else on its fill (the stand-in block), as
/// [`set_tick_alpha`]. Painting a pictured tick's transparent background too made a solid
/// block that hid `hud_tick_mark.dds`'s fade towards the top [observed: owner's
/// screenshot 2026-11-18, #2951; from #1892 on].
fn set_tick_colour(image: Option<&mut ImageNode>, fill: Option<&mut BackgroundColor>, colour: Color) {
    match (image, fill) {
        (Some(image), _) => {
            if image.color != colour {
                image.color = colour;
            }
        }
        (None, Some(fill)) => {
            if fill.0 != colour {
                fill.0 = colour;
            }
        }
        (None, None) => {}
    }
}

/// The texts that change: the ammo counter, the enemy's name, the sneak word.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn texts(
    mut commands: Commands,
    view: Res<HudView>,
    target: Res<HudTarget>,
    data: Option<Res<GameData>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    mut face_: Local<Option<Face>>,
    mut shown: Local<(Option<String>, Option<String>, Option<(Sneak, Option<String>)>, Option<String>)>,
    ammo: Query<Entity, With<AmmoText>>,
    name: Query<Entity, With<EnemyName>>,
    stats: Query<Entity, With<EnemyStats>>,
    sneak: Query<Entity, With<SneakText>>,
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
) {
    // The HUD's lengths at the UI scale (`crate::interface::hud_units`, #1631).
    let _hud = crate::interface::hud_units();
    if (!view.shown && !view.enemy_shown) || ammo.is_empty() {
        return;
    }
    let face_ = face_.get_or_insert_with(|| face(FONT, plain_font(36.0, 15.0), data.as_deref(), &mut images, bc_supported(formats.as_deref())));
    let font = &face_.font;
    let text = ammo_text(view.weapon.ammo);
    let abbrev = view.weapon.abbrev.as_deref().unwrap_or("");
    // Both texts make the key: swapping ammo of the same count changes only the label.
    // The player's tweak: the ammo's label under the count [design: docs/design/tweaks.md].
    let under = tweaks.as_ref().is_some_and(|t| t.ammo_label_under);
    let key = format!("{abbrev}|{text}|{under}");
    if shown.0.as_deref() != Some(key.as_str()) {
        if let Ok(e) = ammo.single() {
            let places = Places::new(font);
            commands.entity(e).despawn_related::<Children>().with_children(|p| {
                if let Some(line) = font.lines(&text, None).into_iter().next() {
                    set_line(p, face_, &line, places.ammo_right - line.width, LOWER_BASE, amber(255.0));
                    if let Some(label) = font.lines(abbrev, None).into_iter().next().filter(|_| !abbrev.is_empty()) {
                        if under {
                            set_line(p, face_, &label, places.ammo_right - label.width, LOWER_BASE + AMMO_LABEL_DROP, amber(255.0));
                        } else {
                            set_line(p, face_, &label, AMMO_LABEL_X, LOWER_BASE, amber(255.0));
                        }
                    }
                }
            });
            shown.0 = Some(key);
        }
    }
    let enemy = target.0.as_ref().map(|t| t.name.clone());
    if shown.1 != enemy {
        if let Ok(e) = name.single() {
            commands.entity(e).despawn_related::<Children>().with_children(|p| {
                if let Some(line) = enemy.as_deref().and_then(|n| font.lines(n, None).into_iter().next()) {
                    set_line(p, face_, &line, centred_x(font, &line, ENEMY_MIDDLE), ENEMY_NAME_BASE, crime_colour());
                }
            });
            shown.1 = enemy;
        }
    }
    // Living Anatomy: "Health … DT …" centred under the name, in its colour.
    let anatomy = target.0.as_ref().and_then(|t| t.anatomy_line());
    if shown.3 != anatomy {
        if let Ok(e) = stats.single() {
            commands.entity(e).despawn_related::<Children>().with_children(|p| {
                if let Some(line) = anatomy.as_deref().and_then(|n| font.lines(n, None).into_iter().next()) {
                    set_line(p, face_, &line, centred_x(font, &line, ENEMY_MIDDLE), ENEMY_STATS_BASE, crime_colour());
                }
            });
            shown.3 = anatomy;
        }
    }
    let sneak_now = view.sneak.map(|s| (s, view.detected_by.clone().filter(|_| s == Sneak::Detected)));
    if shown.2 != sneak_now {
        if let Ok(e) = sneak.single() {
            commands.entity(e).despawn_related::<Children>().with_children(|p| {
                if let Some((s, by)) = &sneak_now {
                    let s = *s;
                    if let Some(line) = font.lines(by.as_deref().unwrap_or(s.word()), None).into_iter().next() {
                        let colour = if s.warning() { crime_colour() } else { amber(255.0) };
                        set_line(p, face_, &line, (SNEAK.x - line.width) / 2.0, SNEAK_BASE, colour);
                    }
                }
            });
            shown.2 = sneak_now;
        }
    }
}

/// Every part follows the shown actor's hostility, including existing letters/ticks
/// when only hostility changes [observed: private job hud_enemy3, n2_away (main),
/// g2_gecko_half (alt)]. Previously always red [design]. Traverse after spawning so
/// new and resized rows, names, anatomy and fallback blocks share the colour [design].
/// The ticks are drawn at the meters' brightness, [`METER_BRIGHTNESS`], as the health and
/// action points ticks [observed: the enemy meter's brightness 175, private jobs
/// hud_enemy2 e5, hud_enemy3 n2, h1, g2; hud.md §14.8]; until #2696 at the full colour.
#[allow(clippy::type_complexity)]
fn enemy_colours(
    target: Res<HudTarget>,
    enemy: Query<Entity, With<EnemyPiece>>,
    children: Query<&Children>,
    mut ink: Query<(Option<&mut ImageNode>, Option<&mut TextColor>, Option<&mut BackgroundColor>, Has<Pip>)>,
) {
    let full = if target.0.as_ref().is_some_and(|t| t.hostile) { crime_colour() } else { amber(255.0) };
    let meter = at_brightness(full, METER_BRIGHTNESS);
    for root in &enemy {
        for entity in children.iter_descendants(root) {
            if let Ok((image, text, fill, tick)) = ink.get_mut(entity) {
                let colour = if tick { meter } else { full };
                // Only the hue changes: each node keeps its own alpha, so the transparent
                // backgrounds every UI node carries stay transparent [design].
                let hue = |old: Color| colour.with_alpha(old.alpha());
                if let Some(mut image) = image {
                    let new = hue(image.color);
                    if image.color != new {
                        image.color = new;
                    }
                }
                if let Some(mut text) = text {
                    let new = TextColor(hue(text.0));
                    text.set_if_neq(new);
                }
                if let Some(mut fill) = fill {
                    if fill.0.alpha() > 0.0 {
                        let new = BackgroundColor(hue(fill.0));
                        fill.set_if_neq(new);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_enemy_bar_stays_at_the_bottom_when_playback_hides_the_corners() {
        use crate::hud_target::TargetHealth;
        let _hud = crate::interface::hud_units();
        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .init_resource::<Time<Real>>()
            .insert_resource(HudView { shown: true, enemy_shown: true, ..default() })
            .insert_resource(HudTarget(Some(TargetHealth { form_id: 1, name: "Practice robot".into(), health: 1.0, hostile: true, anatomy: None, anatomy_dr: None, after: None })))
            .add_systems(Update, draw);
        app.update();
        app.update();
        app.world_mut().resource_mut::<HudView>().shown = false;
        app.update();
        let world = app.world_mut();
        let mut enemies = world.query_filtered::<(&Node, &Visibility, &ChildOf), With<EnemyPiece>>();
        let (node, visible, parent) = enemies.single(world).unwrap();
        assert_eq!(*visible, Visibility::Inherited);
        assert_eq!(node.top, Val::Auto);
        assert_eq!(node.bottom, u(MARGIN_Y + enemy_top()));
        assert!(world.get::<HudRoot>(parent.parent()).is_none());
        let mut roots = world.query_filtered::<&Visibility, With<HudRoot>>();
        assert_eq!(*roots.single(world).unwrap(), Visibility::Hidden);
    }

    #[test]
    fn the_armor_row_shows_reddens_and_flashes_by_its_thresholds() {
        // Always shown with no visibility threshold; only below it with one.
        assert_eq!(armor_look(0.9, 0.0, 0.0, 0.0, true), (true, false));
        assert_eq!(armor_look(0.9, 0.5, 0.0, 0.0, true), (false, false));
        assert_eq!(armor_look(0.4, 0.5, 0.0, 0.0, true), (true, false));
        // Red below its threshold; flashing below the other, red in its on half.
        assert_eq!(armor_look(0.2, 0.0, 0.25, 0.0, false), (true, true));
        assert_eq!(armor_look(0.05, 0.0, 0.0, 0.1, true), (true, true));
        assert_eq!(armor_look(0.05, 0.0, 0.0, 0.1, false), (true, false));
    }

    #[test]
    fn detected_names_the_first_detectors_then_counts_the_rest() {
        let n = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(detected_word(&[], 2), "[SPOTTED]");
        assert_eq!(detected_word(&n(&["Easy Pete"]), 2), "[SPOTTED BY Easy Pete]");
        assert_eq!(detected_word(&n(&["Easy Pete", "Sunny"]), 2), "[SPOTTED BY Easy Pete and Sunny]");
        assert_eq!(detected_word(&n(&["A", "B", "C"]), 3), "[SPOTTED BY A, B and C]");
        assert_eq!(detected_word(&n(&["A", "B", "C"]), 2), "[SPOTTED BY A, B and 1 other]");
        assert_eq!(detected_word(&n(&["A", "B", "C", "D"]), 1), "[SPOTTED BY A and 3 others]");
        assert_eq!(detected_word(&n(&["A", "B"]), 0), "[SPOTTED BY 2]");
    }

    #[test]
    fn the_place_ahead_is_the_nearest_bearing_within_the_angle() {
        let m = [([0.0, 700.0], "North Camp", true), ([700.0, 700.0], "North East", true), ([-70.0, 7000.0], "Far North", false), ([0.0, -700.0], "", true)];
        // Facing north: the camp straight ahead, 10 m off; Far North is a hair off the line.
        assert_eq!(place_ahead(m, [0.0, 0.0], 0.0, 10_000.0, 10.0, false, 1).as_deref(), Some("North Camp  10 m"));
        assert_eq!(place_ahead(m, [0.0, 0.0], 0.0, 10_000.0, 10.0, false, 2).as_deref(), Some("North Camp  33 ft"));
        // Facing north east: that one; south: the unnamed marker is no answer.
        assert_eq!(place_ahead(m, [0.0, 0.0], 45.0, 10_000.0, 10.0, false, 0).as_deref(), Some("North East"));
        assert_eq!(place_ahead(m, [0.0, 0.0], 180.0, 10_000.0, 10.0, false, 0), None);
        // Out of range, or not found and hidden.
        let far = [([-70.0, 7000.0], "Far North", false)];
        assert_eq!(place_ahead(far, [0.0, 0.0], 0.0, 1000.0, 10.0, false, 0), None);
        assert_eq!(place_ahead(far, [0.0, 0.0], 0.0, 10_000.0, 10.0, true, 0), None);
        assert_eq!(place_ahead(far, [0.0, 0.0], 0.0, 10_000.0, 10.0, false, 0).as_deref(), Some("Far North"));
    }

    #[test]
    fn the_hide_hud_key_hides_only_its_chosen_parts() {
        let mut h = HudHidden { on: false, compass: true, ammo: false, needs: true, sneak: true };
        assert_eq!((h.corner(h.compass), h.corner(h.ammo)), (Visibility::Inherited, Visibility::Inherited));
        h.on = true;
        assert_eq!((h.corner(h.compass), h.corner(h.ammo)), (Visibility::Hidden, Visibility::Inherited));
    }

    #[test]
    fn enemy_ink_tracks_hostility_without_rebuilding_any_children() {
        use crate::hud_target::TargetHealth;
        let mut app = App::new();
        app.insert_resource(HudTarget(Some(TargetHealth { form_id: 1, name: "Target".into(), health: 0.5, hostile: false, anatomy: Some((50.0, 2.0)), anatomy_dr: None, after: None })))
            .add_systems(Update, enemy_colours);
        let bracket = app.world_mut().spawn(ImageNode::default()).id();
        // A stand-in tick (no picture) and a pictured one with its node's transparent
        // background.
        let block = app.world_mut().spawn((Pip { bar: Bar::Enemy, index: 0 }, BackgroundColor(Color::WHITE))).id();
        let tick = app.world_mut().spawn((Pip { bar: Bar::Enemy, index: 1 }, ImageNode::default(), BackgroundColor(Color::NONE))).id();
        let name = app.world_mut().spawn(TextColor(Color::WHITE)).id();
        // A container's transparent background (every UI node has one) stays transparent.
        let group = app.world_mut().spawn(BackgroundColor(Color::NONE)).add_children(&[block, tick, name]).id();
        app.world_mut().spawn(EnemyPiece).add_children(&[bracket, group]);
        for hostile in [false, true, false] {
            app.world_mut().resource_mut::<HudTarget>().0.as_mut().unwrap().hostile = hostile;
            app.update();
            let colour = if hostile { crime_colour() } else { amber(255.0) };
            // The ticks at the meters' brightness, as the health and action points ticks.
            let meter = at_brightness(colour, METER_BRIGHTNESS);
            assert_eq!(app.world().get::<ImageNode>(bracket).unwrap().color, colour);
            assert_eq!(app.world().get::<BackgroundColor>(block).unwrap().0, meter);
            assert_eq!(app.world().get::<ImageNode>(tick).unwrap().color, meter);
            assert_eq!(app.world().get::<BackgroundColor>(tick).unwrap().0, Color::NONE);
            assert_eq!(app.world().get::<TextColor>(name).unwrap().0, colour);
            assert_eq!(app.world().get::<BackgroundColor>(group).unwrap().0, Color::NONE);
        }
        assert!((at_brightness(amber(255.0), METER_BRIGHTNESS).to_srgba().red - amber(METER_BRIGHTNESS).to_srgba().red).abs() < 1e-6);
    }

    /// The enemy ticks' strength goes on the picture and never on a pictured tick's
    /// transparent background, which made a solid block that hid the picture's fade
    /// (#2696); a stand-in block takes it on its fill.
    #[test]
    fn an_enemy_tick_keeps_its_background_clear_so_its_picture_fades() {
        for alpha in [1.0, 0.3] {
            let mut image = ImageNode::default();
            let mut fill = BackgroundColor(Color::NONE);
            set_tick_alpha(Some(&mut image), Some(&mut fill), alpha);
            assert_eq!(image.color.alpha(), alpha);
            assert_eq!(fill.0, Color::NONE);
            let mut block = BackgroundColor(Color::WHITE);
            set_tick_alpha(None::<&mut ImageNode>, Some(&mut block), alpha);
            assert_eq!(block.0.alpha(), alpha);
        }
    }

    /// The health and action points ticks' colour, amber or the low-bar red, goes on the
    /// picture and leaves a pictured tick's background clear, so the picture's fade shows
    /// (#2951); a stand-in block takes it on its fill.
    #[test]
    fn a_meter_tick_keeps_its_background_clear_so_its_picture_fades() {
        for colour in [amber(METER_BRIGHTNESS), crime_colour()] {
            let mut image = ImageNode::default();
            let mut fill = BackgroundColor(Color::NONE);
            set_tick_colour(Some(&mut image), Some(&mut fill), colour);
            assert_eq!(image.color, colour);
            assert_eq!(fill.0, Color::NONE);
            let mut block = BackgroundColor(Color::WHITE);
            set_tick_colour(None, Some(&mut block), colour);
            assert_eq!(block.0, colour);
        }
    }

    /// Letters 15 apart, as the HUD's font sets them.
    fn font() -> Font {
        plain_font(36.0, 15.0)
    }

    #[test]
    fn a_bar_rounds_its_lit_ticks_down() {
        assert_eq!(lit(1.0, PIPS), 37);
        assert_eq!(lit(0.5, PIPS), 18);
        assert_eq!(lit(0.01, PIPS), 0);
        assert_eq!(lit(0.0, PIPS), 0);
        // Out of range or non-finite: bounded ticks [design: lit].
        assert_eq!(lit(-0.2, PIPS), 0);
        assert_eq!(lit(1.7, PIPS), 37);
        assert_eq!(lit(f32::NAN, PIPS), 0);
        assert_eq!(lit(f32::INFINITY, PIPS), 0);
        assert_eq!(lit(f32::NEG_INFINITY, PIPS), 0);
        assert_eq!(lit(1.0, 0), 0);
    }

    #[test]
    fn the_corners_match_the_observed_tick_counts() {
        // [observed: hud.md §14.3] Health, then action points.
        for (current, max, ticks) in [
            (200.0, 200.0, 37), (100.0, 200.0, 18), (20.0, 200.0, 3),
            (43.68, 80.0, 20), (52.15, 80.0, 24), (69.7, 80.0, 32), (80.0, 80.0, 37),
        ] {
            assert_eq!(lit(current / max, PIPS), ticks, "{current} of {max}");
        }
    }

    #[test]
    fn the_condition_fill_matches_the_observed_widths() {
        // [observed: hud.md §14.3] Whole units inside the 60-wide ground.
        for (condition, width) in [(0.999, 59.0), (0.5, 30.0), (0.4987, 29.0), (0.1, 6.0)] {
            assert_eq!(condition_width(condition), width, "{condition}");
        }
    }

    #[test]
    fn the_enemy_bar_empties_from_both_ends() {
        assert_eq!(lit_middle(1.0, 31), (0, 31));
        assert_eq!(lit_middle(0.0, 31), (15, 15));
        // Half: 16 lit, 15 gone, 7 from the left and 8 from the right.
        assert_eq!(lit_middle(0.5, 31), (7, 23));
        // Alive at 2 % (0.62 of a tick): one tick still lit.
        assert_eq!(lit_middle(0.02, 31), (15, 16));
        for i in 0..=100 {
            let (from, to) = lit_middle(i as f32 / 100.0, 31);
            assert!(to <= 31 && (31 - to) - from <= 1, "{i}: {from}..{to}");
        }
    }

    #[test]
    fn ticks_stand_on_whole_pixels() {
        // The screenshots' sizes: 6 of 8 at 720 lines, 9 at 1080.
        assert_eq!(tick_pixels(720.0), (6.0, 5.0));
        assert_eq!(tick_pixels(1080.0), (9.0, 7.0));
        assert_eq!(tick_pixels(1440.0), (12.0, 9.0));
        // Never a tick without a gap, even tiny.
        let (step, width) = tick_pixels(100.0);
        assert!(width >= 1.0 && width < step);
        // A full bar spans its room at any size: equal ticks, gaps a pixel apart at most.
        for lines in [600, 720, 768, 800, 900, 1009, 1024, 1050, 1080, 1200, 1440, 1600, 2160] {
            let h = lines as f32;
            let (step, width) = tick_pixels(h);
            let length = (METER_WIDTH * h / 960.0 - (step - width)).round();
            let (lefts, w) = tick_lefts(length, width, PIPS);
            assert_eq!(lefts[0], 0.0, "{lines} lines");
            assert_eq!(lefts[PIPS - 1] + w, length, "{lines} lines");
            let gaps: Vec<f32> = lefts.windows(2).map(|p| p[1] - p[0] - w).collect();
            let (lo, hi) = gaps.iter().fold((f32::MAX, 0.0f32), |(a, b), &g| (a.min(g), b.max(g)));
            assert!(lo >= 1.0 && hi - lo <= 1.0, "{lines} lines: gaps {lo}..{hi}");
            assert_eq!(w, width, "{lines} lines: ticks kept their width");
        }
        // At 1080 lines the step is the even 9 of the original's screenshot.
        let (lefts, w) = tick_lefts(331.0, 7.0, PIPS);
        assert!(w == 7.0 && lefts.windows(2).all(|p| p[1] - p[0] == 9.0));
    }

    /// T12e: at every size the action points' last tick ends at least a pixel short of its
    /// row's end (the upright), as the health bar's before its corner mark.
    #[test]
    fn the_last_action_point_tick_stands_clear_of_the_upright() {
        for lines in [600, 720, 768, 800, 900, 1009, 1024, 1050, 1080, 1200, 1440, 1600, 2160] {
            let h = lines as f32;
            let (step, width) = tick_pixels(h);
            let room = METER_WIDTH * h / 960.0;
            for bar in [Bar::Health, Bar::ActionPoints] {
                let (first, lefts, w) = row_ticks(bar, room, step, width, PIPS);
                assert_eq!(first, 0.0, "{lines} lines");
                let end = first + lefts[PIPS - 1] + w;
                assert!(room - end >= 0.5 && room - end <= step - w + 0.5, "{lines} lines: {bar:?} ends {end} of {room}");
            }
        }
        // At 720 lines: ticks every 6 pixels, the last ending 1 pixel short (the original's
        // 1229-1232 with the upright at 1234).
        let (first, lefts, w) = row_ticks(Bar::ActionPoints, 222.0, 6.0, 5.0, PIPS);
        assert_eq!((first, lefts[1], lefts[PIPS - 1] + w), (0.0, 6.0, 221.0));
    }

    #[test]
    fn the_sneak_word_is_the_worst_news() {
        assert_eq!(Sneak::of(false, false, false), Sneak::Hidden);
        assert_eq!(Sneak::of(false, false, true), Sneak::Detected);
        assert_eq!(Sneak::of(false, true, true), Sneak::Caution);
        assert_eq!(Sneak::of(true, true, true), Sneak::Danger);
        assert!(Sneak::Danger.warning() && Sneak::Caution.warning() && !Sneak::Detected.warning());
        assert_eq!(Sneak::Hidden.word(), "[UNSEEN]");
    }

    #[test]
    fn the_compass_keeps_the_heading_at_the_triangle() {
        // Read the strip at x 154 (T12e; 148 before, 172.5 at first); its scale stays 1024
        // per turn [observed: hud.md §14.5].
        assert_eq!(STRIP_HEADING, 154.0);
        let at = |heading: f32| (STRIP_HEADING - strip_left(heading)).rem_euclid(STRIP.x);
        assert!((at(0.0) - 0.0).abs() < 1e-3 || (at(0.0) - 1024.0).abs() < 1e-3);
        assert!((at(90.0) - 256.0).abs() < 1e-3);
        assert!((at(225.0) - 640.0).abs() < 1e-3);
        assert!((at(-90.0) - 768.0).abs() < 1e-3);
        assert!((at(450.0) - 256.0).abs() < 1e-3);
        // The doubled strip always covers the window.
        for h in 0..720 {
            let left = strip_left(h as f32 * 0.5);
            assert!(left <= 0.0 && left + 2.0 * STRIP.x >= COMPASS.x, "heading {h}: {left}");
        }
        // Turning right moves the strip left.
        assert!(strip_left(100.0) < strip_left(90.0));
    }

    #[test]
    fn ammo_is_loaded_then_carried() {
        assert_eq!(ammo_text(Some((0, 0))), "0/0");
        assert_eq!(ammo_text(Some((6, 42))), "6/42");
        assert_eq!(ammo_text(None), "");
    }

    #[test]
    fn the_ammo_label_stands_right_of_the_divider() {
        const { assert!(AMMO_LABEL_X > DIVIDER && AMMO_LABEL_X < DIVIDER + 2.0 * LABEL_GAP) };
    }

    #[test]
    fn the_pieces_stand_inside_their_brackets() {
        let p = Places::new(&font());
        // A full bar ends at the corner mark on the left and starts at it on the right,
        // and runs to just inside the upright.
        assert_eq!(p.hp_meter + METER_WIDTH, LEFT_MARK - METER_SHIFT);
        // T12e: the health bar where the original's tile stands [observed: hud.md §14].
        assert_eq!(p.hp_meter, 20.0);
        assert_eq!(p.ap_meter, RIGHT_MARK - AP_METER_SHIFT);
        assert!(p.hp_meter > LEFT_UPRIGHT && p.hp_meter - LEFT_UPRIGHT < 10.0);
        assert!(p.ap_meter + METER_WIDTH < RIGHT_UPRIGHT && RIGHT_UPRIGHT - (p.ap_meter + METER_WIDTH) < 10.0);
        // The bars stand on the long line, above it; the labels above the bars.
        assert_eq!(p.meter_y + TICK.y + METER_GAP, LINE_Y);
        assert!(UPPER_BASE < p.meter_y);
        assert!(p.hp_label > LEFT_UPRIGHT && p.ap_label + font().width(AP) < RIGHT_UPRIGHT);
        // Condition's label, then its bar next to the divider; the ammo right of it.
        assert_eq!(p.cnd_label + font().width(CND) + CND_LABEL_GAP, p.cnd_x);
        assert_eq!(p.cnd_x + CND_BAR.x + CND_BEFORE_DIVIDER, DIVIDER);
        assert!(p.ammo_right > DIVIDER && p.ammo_right < RIGHT_UPRIGHT);
        // The bar's bottom is on the label's line.
        assert_eq!(p.cnd_y + CND_BAR.y + CND_RAISE, LOWER_BASE);
        assert!(p.cnd_y > LINE_Y);
        // The arrows' middle at three quarters of the bar.
        assert!((p.arrows_x + ARROWS_MIDDLE * (CND_BAR.y + 2.0 * ARROWS_BLEED) - (p.cnd_x + 0.75 * CND_BAR.x)).abs() < 1e-3);
        // The enemy's ticks fit between its bracket's marks.
        // Keep the owner's 31 ticks; the row packs them into the scaled marks [design].
        let room = ENEMY_MARKS.1 - ENEMY_MARKS.0;
        let (lefts, width) = tick_lefts(room, TICK_BLOCK, ENEMY_PIPS);
        assert!((lefts[ENEMY_PIPS - 1] + width - room).abs() < 0.6);
    }

    /// The enemy's name, and the Living Anatomy line, stand centred on its tick row, not
    /// on the piece's width, whatever their length (#1826).
    #[test]
    fn the_enemy_name_is_centred_on_its_ticks() {
        // Letters with room before and after their pictures, as the game's fonts have.
        let mut font = plain_font(36.0, 0.0);
        for g in font.glyphs.iter_mut().skip(0x21).take(0x5E) {
            *g = fnv_font::Glyph { width: 10.0, height: 20.0, before: 1.0, after: 4.0, ascent: 16.0, ..*g };
        }
        font.glyphs[b' ' as usize].after = 6.0;
        let from = ENEMY_BRACKET_AT.x + ENEMY_MARKS.0;
        let (lefts, width) = tick_lefts(ENEMY_MARKS.1 - ENEMY_MARKS.0, TICK_BLOCK, ENEMY_PIPS);
        let ticks = from + (lefts[0] + lefts[ENEMY_PIPS - 1] + width) / 2.0;
        assert!((ticks - ENEMY_MIDDLE).abs() < 0.5, "ticks {ticks} middle {ENEMY_MIDDLE}");
        for name in ["Fiend", "Gecko", "Cazador", "NCR Trooper", "Legion Veteran Decanus", "Health 120  DT 4.0"] {
            let line = font.lines(name, None).remove(0);
            let x = centred_x(&font, &line, ENEMY_MIDDLE);
            let first = &line.glyphs[0];
            let last = line.glyphs.last().unwrap();
            let left = x + first.x + font.glyph(first.code).before;
            let right = x + last.x + font.glyph(last.code).before + font.glyph(last.code).width;
            assert!(((left + right) / 2.0 - ticks).abs() < 0.5, "{name}: {left}..{right} against {ticks}");
        }
        // Nothing drawn: its set width is centred.
        let blank = font.lines("x", None).remove(0);
        let x = centred_x(&plain_font(36.0, 15.0), &blank, ENEMY_MIDDLE);
        assert_eq!(x, ENEMY_MIDDLE - blank.width / 2.0);
    }
}
