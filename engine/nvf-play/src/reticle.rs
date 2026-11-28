//! The crosshair and what is written under it, drawn as the original draws them
//! (docs/spec-notes/reticle.md).
//!
//! - The crosshair is the game's `glow_crosshair.dds`, two arrows pointing in; with
//!   something under it the player can use, `glow_crosshair_filled.dds`, the same arrows
//!   in a box [documented: the HUD's template names the two; observed: the owner's
//!   screenshots, §2, §3].
//! - Under it, the key and the action ("[E] Take"), the target's name, and for an item a
//!   line across with its weight and value either side of a stem (§3). [`layout`] sets
//!   them out (pure, tested) from the measurements of §3, in the HUD's font 7.
//! - **Spread**: while a weapon with a firing cycle is drawn the plain crosshair's two
//!   arrows stand apart by the cone a shot would leave in now (`Combat::crosshair_cone`,
//!   bloom included), each arrow's tip on the cone's edge [hypothesis: the HUD file's
//!   `_SpreadPercentage` trait says the code sets a spread; owner's playtest list 5,
//!   item 15]; they move there at `_SpreadChangeSpeed` [documented: 0.125, in
//!   hud_main_menu.xml; hypothesis: a share of the way per frame at 60 frames a second].
//!   A switch of the settings page's Gameplay group, on by default ([`DynamicCrosshair`];
//!   owner 2026-10-25): off, the arrows stay together whatever is drawn.
//! - **Gun cross**: with the switch on and such a weapon drawn, the plain crosshair is
//!   the HUD's four bars `target_top/bottom/left/right.dds` put together as a cross, each
//!   bar's inner end on the cone's edge, no nearer the middle than [`CROSS_REST`] [owner
//!   2026-10-27; observed: each picture's bar is 5 texels across, its middle line texel 2,
//!   its solid end at the picture's edge toward the middle]. Without the four pictures,
//!   the arrows as before. The settings page's Crosshair style ([`CrosshairStyle`]; owner
//!   2026-10-30) picks the cross or the parting arrows.
//! - **Hostile**: over an actor hostile to the player the crosshair is red, the crime
//!   colour (`crate::hud_target::AimHostile`) [owner's playtest list 5, item 58;
//!   documented: GECK wiki IsCrimeOrEnemy, "the red color of the crosshair"].
//! - Hidden outside play. A script's DisablePlayerControls hides the crosshair with the
//!   movement flag and the texts with the rollover text flag [documented: GECK wiki
//!   DisablePlayerControls, MovementFlag and RolloverTextFlag; controls.md §5].
//! - Without the game's files: a small square, and the texts in Bevy's font.

use crate::activate::outcome::{self, Action};
use crate::activate::{Target, Targeted};
use crate::control::ControlLocks;
use crate::interface::{amber, at, bc_supported, face, plain_font, set_text, Face, Picture, Pictures, Set};
use crate::sim::SimState;
use crate::tiles::vanilla::{DrawnByTiles, PromptTexts as Texts};
use crate::{PlaySet, PlayState};
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use fnv_font::{Font, Line};
use fnv_sim::forms::BaseData;
use nvf_assets::GameData;

// ---- the interface files' [documented: menus\prefabs\hudtemplates.xml, §2] -------------

const PLAIN: &str = "interface\\hud\\glow_crosshair.dds";
const FILLED: &str = "interface\\hud\\glow_crosshair_filled.dds";
/// The crosshair's picture, 64 by 64, its middle on the view's middle [observed: the
/// arrows' middle is the picture's, and on the screenshots the view's].
const RETICLE: f32 = 64.0;
/// The arrows' tips stand this far either side of the middle in the picture [observed:
/// `glow_crosshair.dds`, the left arrow's tip at texel 30, the right's at 34].
const TIP: f32 = 2.0;
/// The gun cross's four bars, top, bottom, left and right [documented: the four pictures
/// in Fallout - Textures2.bsa; owner 2026-10-27].
const BARS: [(&str, Vec2); 4] = [
    ("interface\\hud\\target_top.dds", Vec2::new(0.0, -1.0)),
    ("interface\\hud\\target_bottom.dds", Vec2::new(0.0, 1.0)),
    ("interface\\hud\\target_left.dds", Vec2::new(-1.0, 0.0)),
    ("interface\\hud\\target_right.dds", Vec2::new(1.0, 0.0)),
];
/// A bar's picture, along and across, drawn texel for unit [observed: 32 by 8 and 8 by
/// 32], and its middle line across from the picture's edge [observed: texel 2 of 5].
const BAR_LONG: f32 = 32.0;
const BAR_WIDE: f32 = 8.0;
const BAR_MIDDLE: f32 = 2.5;
/// The bars' inner ends stand at least this far from the middle [design].
pub const CROSS_REST: f32 = 4.0;
/// [documented: hud_main_menu.xml `_SpreadChangeSpeed`].
const SPREAD_SPEED: f32 = 0.125;
/// The arrows go no further apart than this from the middle [design].
const MAX_SPREAD: f32 = 240.0;
/// Font 7 [documented: the prompt's text box with the glow set and no font of its own;
/// Fallout_default.ini `sFontFile_7`].
const FONT: &str = "textures\\fonts\\baked-in_monofonto_large.fnt";
/// The item's line across, with its stem [hypothesis: the HUD's `info_seperator`
/// template, the only picture of its shape; §3].
const DIVIDER: &str = "interface\\hud\\glow_hud_bottom_info_seperator_divider.dds";
/// The picture's size, drawn texel for unit [observed: its double line is 9 texels and
/// 7 pixels of the 720-high screenshot], and where its stem's middle is in it [observed].
const DIVIDER_SIZE: Vec2 = Vec2::new(512.0, 128.0);
const STEM_IN_PICTURE: f32 = 160.5;

// ---- measured on the owner's screenshots [observed, §3] --------------------------------
// In the interface's units (960 to the view's height) from the view's middle.

/// The action's middle and the name's, across: not the same, on both screenshots.
const ACTION_X: f32 = -3.0;
const NAME_X: f32 = 1.0;
/// The action's line, and the name's under it.
const ACTION_BASE: f32 = 326.5;
const LINE_PITCH: f32 = 40.0;
/// From the key's end to the action's start.
const KEY_GAP: f32 = 11.0;
/// The divider picture's top under the name's line; its stem's middle, across.
const DIVIDER_BELOW: f32 = 8.5;
const STEM_X: f32 = -4.0;
/// The item's figures: their line under the divider picture's top; from the stem, where
/// the weight's word starts and the weight ends (left of it) and where the value's word
/// starts and the value ends (right of it).
const FIGURES_BELOW: f32 = 50.5;
const WG_X: f32 = -145.0;
const WEIGHT_END: f32 = -21.0;
const VAL_X: f32 = 9.5;
const VALUE_END: f32 = 135.0;

/// The key that activates when no binding says: E, the Activate control's default
/// (bindings.rs), in brackets as every hint is [design, #4495]. The prompt names the
/// Activate control's own key, or its controller button while a controller works the
/// controls (`crate::bindings::Bindings::prompt`: "[A]").
const KEY: &str = "[E]";
/// The least room between a figure's word and the figure, when a long word or figure
/// would otherwise meet it [design].
const FIGURE_GAP: f32 = 8.0;

/// The figures' words: the settings the item stats display's weight and value cards show
/// (`sInventoryWeightUpper`, `sInventoryValue`), as the Pip-Boy's cards read them: the load
/// order's text, else NVForever's own label (`fnv_ui::strings::BUILT_IN`) [design, #4495:
/// in place of the abbreviations read from the original's screen].
fn weight_word() -> &'static str {
    crate::pipboy::labels::built_in_label("sInventoryWeightUpper")
}

fn value_word() -> &'static str {
    crate::pipboy::labels::built_in_label("sInventoryValue")
}

/// What is written under the crosshair.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Prompt {
    pub action: String,
    pub name: String,
    /// An item's weight and value, as written.
    pub figures: Option<[String; 2]>,
    /// Drawn in the crime colour: someone else owns the target (`crate::crime`).
    pub crime: bool,
    /// The Activate control's key or button in brackets; empty: [`KEY`].
    pub key: String,
    /// A line under the name: [`EMPTY`] for a container or body with nothing in it.
    pub note: Option<String>,
    /// Drawn greyed: E would do nothing (a co-op revive with nothing to revive with).
    pub grey: bool,
}

/// Under the name of an empty container or body [owner 2026-10-27].
const EMPTY: &str = "[Empty]";
/// Under the name of a downed co-op player one has nothing to revive with [design].
const NOTHING_TO_REVIVE_WITH: &str = "[No Stimpak]";
/// A greyed prompt's strength [design].
const GREY: f32 = 0.45;

/// The player's tweak: a stack's weight and value are the whole stack's [design: docs/design/tweaks.md].
static STACK_FIGURES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Set by the Tweaks tab.
pub fn set_stack_figures(on: bool) {
    STACK_FIGURES.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The XATO Activation Prompt of reference `form_id`, else of its base (an ACTI).
pub fn activation_prompt(sim: &SimState, form_id: u32) -> Option<String> {
    let own = sim.refs.get(fnv_sim::FormId(form_id)).and_then(|r| r.prompt.clone());
    own.or_else(|| sim.base_of(form_id).and_then(|b| sim.forms.get(b)).and_then(|b| b.prompt.clone()))
}

impl Prompt {
    /// The drawn key, including its fallback [design: shared with the tile tree].
    pub(crate) fn key_text(&self) -> &str {
        if self.key.is_empty() { KEY } else { &self.key }
    }

    /// Bind the same prompt for drawing and tile writes [design].
    pub(crate) fn bound(t: &Targeted, sim: &SimState, bindings: Option<&crate::bindings::Bindings>) -> Prompt {
        let key = bindings.map(|b| b.prompt(crate::bindings::Control::Activate)).unwrap_or_default();
        Prompt { key, ..Prompt::of(t, sim) }
    }

    pub fn of(t: &Targeted, sim: &SimState) -> Prompt {
        let (mut action, name) = outcome::prompt(&t.name, &t.action);
        // Session changes share the script reader's authority; XATO below still wins
        // (docs/spec-notes/string-settings.md).
        if matches!(t.action, Action::Talk) {
            if let Some(fnv_sim::settings::Setting::Str(text)) = sim.world.setting_value("sTargetTypeTalk") {
                action = text.clone();
            }
        }
        // An owned item is stolen, not taken [hypothesis: the game's word; owner's
        // playtest list 2 of 2026-10-12].
        if t.crime && matches!(t.action, Action::Take { .. }) {
            action = "Steal".into();
        }
        // An Activation Prompt (XATO) of the reference, else of its activator base, in
        // place of the action word; not over a lock or an inaccessible door
        // (docs/spec-notes/cells-refs.md §3) [hypothesis: which states it replaces].
        if !matches!(t.action, Action::Locked { .. } | Action::UsePassword | Action::Unlock | Action::Name | Action::Hostile | Action::Inaccessible | Action::Revive { .. }) {
            if let Some(p) = activation_prompt(sim, t.form_id) {
                action = p;
            }
        }
        let item = match t.action {
            Action::Take { .. } => sim.base_of(t.form_id).and_then(|b| sim.forms.get(b)),
            _ => None,
        };
        let n = match t.action {
            Action::Take { count } if count > 1 && STACK_FIGURES.load(std::sync::atomic::Ordering::Relaxed) => count,
            _ => 1,
        };
        let figures = item.and_then(|b| match &b.data {
            BaseData::Item(i) => Some([weight_text(i.weight.map(|w| w * n as f32)), value_text(i.worth().map(|v| v.saturating_mul(n)))]),
            _ => None,
        });
        let mut note = t.empty.then(|| EMPTY.to_string());
        // A co-op revive with nothing to revive with says so, greyed [design: as the game
        // shows a missing key].
        let grey = matches!(t.action, Action::Revive { with: None });
        if grey {
            note = Some(NOTHING_TO_REVIVE_WITH.into());
        }
        Prompt { action, name, figures, crime: t.crime, key: String::new(), note, grey }
    }
}

/// An item's weight as written: "--" for none [observed: a bottle cap's, §3]; else
/// without the zeros after the point [hypothesis, §5].
pub fn weight_text(w: Option<f32>) -> String {
    match w.filter(|w| *w > 0.0) {
        Some(w) => {
            let s = format!("{w:.2}");
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        }
        None => "--".into(),
    }
}

/// An item's value as written; "--" for none [hypothesis: as the barter menu's price of
/// an item worth nothing, §5].
pub fn value_text(v: Option<i32>) -> String {
    match v.filter(|v| *v > 0) {
        Some(v) => v.to_string(),
        None => "--".into(),
    }
}

/// The texts set out from the view's middle, and the divider picture's top left corner.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub texts: Vec<Set>,
    pub divider: Option<Vec2>,
    /// A PlayStation face button's glyph in the key's place ("[Cross]" drawn as "[", the
    /// cross and "]"): the glyph, its top left corner and its side [design, #1452]. The
    /// "[" is the last of [`Layout::texts`].
    pub glyph: Option<(crate::pad::Glyph, Vec2, f32)>,
}

pub fn layout(p: &Prompt, font: &Font) -> Layout {
    let line = |s: &str| font.lines(s, None).into_iter().next().unwrap_or_default();
    let set = |line: Line, x: f32, base: f32| Set { line, x, base };
    let action = line(&p.action);
    // A face button under the PlayStation names: "[", its glyph, "]" [design, #1452]. The
    // labelled buttons (L1, Select…) are drawn by the menu hints, not the crosshair.
    let glyph = p.key_text().strip_prefix('[').and_then(|k| k.strip_suffix(']')).and_then(crate::pad::Glyph::named).filter(|g| g.is_face()).filter(|_| !p.action.is_empty());
    let key = line(if glyph.is_some() { "]" } else { p.key_text() });
    let name = line(&p.name);
    let action_x = ACTION_X - action.width / 2.0;
    let name_base = ACTION_BASE + LINE_PITCH;
    // No action (an actor shown by name only, `Action::Name`): the name alone, on its
    // own line [hypothesis: the line stays where it is with an action].
    let mut texts = if p.action.is_empty() {
        vec![set(name.clone(), NAME_X - name.width / 2.0, name_base)]
    } else {
        vec![
            set(key.clone(), action_x - KEY_GAP - key.width, ACTION_BASE),
            set(action, action_x, ACTION_BASE),
            set(name.clone(), NAME_X - name.width / 2.0, name_base),
        ]
    };
    // The note on its own line under the name, centred as the name is [design].
    if let Some(note) = p.note.as_deref().map(line) {
        texts.push(set(note.clone(), NAME_X - note.width / 2.0, name_base + LINE_PITCH));
    }
    let divider = p.figures.as_ref().map(|[weight, value]| {
        let top = name_base + DIVIDER_BELOW;
        let base = top + FIGURES_BELOW;
        let (weight, value, wg, val) = (line(weight), line(value), line(weight_word()), line(value_word()));
        // The weight ends against the stem; its word starts at its place, or further left
        // when the two would meet. The value's word starts by the stem; the value ends at
        // its place, or further right after its word.
        let weight_x = STEM_X + WEIGHT_END - weight.width;
        let wg_x = (STEM_X + WG_X).min(weight_x - FIGURE_GAP - wg.width);
        let value_x = (STEM_X + VALUE_END - value.width).max(STEM_X + VAL_X + val.width + FIGURE_GAP);
        texts.push(set(wg, wg_x, base));
        texts.push(set(weight.clone(), weight_x, base));
        texts.push(set(val, STEM_X + VAL_X, base));
        texts.push(set(value.clone(), value_x, base));
        Vec2::new(STEM_X - STEM_IN_PICTURE, top)
    });
    let glyph = glyph.map(|g| {
        let side = font.size * GLYPH_SHARE;
        let x = action_x - KEY_GAP - key.width - GLYPH_GAP - side;
        let open = line("[");
        texts.push(set(open.clone(), x - GLYPH_GAP - open.width, ACTION_BASE));
        (g, Vec2::new(x, ACTION_BASE - side * GLYPH_RISE), side)
    });
    Layout { texts, divider, glyph }
}

/// A glyph's side as a share of the font's size, how much of it stands above the line
/// and the room between it and its brackets [design, #1452].
const GLYPH_SHARE: f32 = 0.7;
const GLYPH_RISE: f32 = 0.92;
const GLYPH_GAP: f32 = 0.5;
/// The glyph pictures' side in pixels (drawn smaller) [design].
const GLYPH_PIXELS: u32 = 64;

/// The crime colour: the prompt over something another owns, and the crosshair with it
/// [documented: GECK wiki IsCrimeOrEnemy, the "red color of the crosshair or the
/// activation prompt"; HudBarSetSystemColor, the system colour "Crime" of the prompt].
/// Its value: the HUD's alternative colour, `iSystemColorHUDAlt*` 255, 67, 42 in
/// Fallout_default.ini [observed; hypothesis: that it is the crime colour].
pub fn crime_colour() -> Color {
    Color::srgb_u8(255, 67, 42)
}

// ---- drawing ----------------------------------------------------------------------------

/// The enemy health bar names the actor it shows (`crate::hud_target`): while it shows the
/// actor under the crosshair, the prompt under the crosshair is not drawn, so the name is
/// not written twice [owner's playtest, 2026-10-16: the bar's name and the prompt's both
/// over a cazador in combat].
pub fn under_bar(t: &Targeted, bar_form_id: u32) -> bool {
    t.form_id == bar_form_id
}

/// Whether the crosshair's arrows part with the weapon's spread (module notes).
#[derive(Resource, Clone, Copy, Debug)]
pub struct DynamicCrosshair(pub bool);

impl Default for DynamicCrosshair {
    fn default() -> Self {
        DynamicCrosshair(true)
    }
}

/// What the dynamic crosshair looks like with a gun or thrown weapon drawn (module
/// notes): the gun cross, or the arrows parting [owner 2026-10-30].
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CrosshairStyle {
    /// The four bars of §7.
    #[default]
    Cross,
    /// The plain crosshair's two arrows, parting with the spread (§6).
    Arrows,
}

impl CrosshairStyle {
    pub const ALL: [CrosshairStyle; 2] = [CrosshairStyle::Cross, CrosshairStyle::Arrows];

    pub fn name(self) -> &'static str {
        match self {
            CrosshairStyle::Cross => "Cross",
            CrosshairStyle::Arrows => "Arrows",
        }
    }

    /// The style of this name, any case.
    pub fn by_name(name: &str) -> Option<CrosshairStyle> {
        CrosshairStyle::ALL.into_iter().find(|s| s.name().eq_ignore_ascii_case(name.trim()))
    }
}

fn style_pick(w: &mut World, name: &str) -> bool {
    match CrosshairStyle::by_name(name) {
        Some(s) => {
            *w.resource_mut::<CrosshairStyle>() = s;
            true
        }
        None => false,
    }
}

pub struct ReticlePlugin;

impl Plugin for ReticlePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DynamicCrosshair>().init_resource::<CrosshairStyle>().add_systems(Update, draw.in_set(PlaySet::Ui).in_set(crate::test_aid::activation_sweep::Draw));
        crate::settings::add_setting(
            app,
            crate::settings::Setting {
                key: "dynamic-crosshair",
                group: crate::pause::GROUP_MENUS,
                label: "Dynamic crosshair",
                note: "On: with a gun or thrown weapon drawn the crosshair opens with how wide your shots spread now. Off: the arrows stay together.",
                switches: "",
                get: |w| w.resource::<DynamicCrosshair>().0,
                set: |w, on| w.resource_mut::<DynamicCrosshair>().0 = on,
                slider: None,
            },
        );
        crate::settings::add_choice_list(
            app,
            crate::settings::Setting {
                key: "crosshair-style",
                group: crate::pause::GROUP_MENUS,
                label: "Crosshair style",
                note: "With Dynamic crosshair on. Cross: four bars that part with the spread. Arrows: the plain crosshair's two arrows part instead.",
                switches: "",
                get: |_| false,
                set: |_, _| {},
                slider: None,
            },
            crate::settings::ChoiceList {
                names: |_| CrosshairStyle::ALL.iter().map(|s| s.name().to_string()).collect(),
                chosen: |w| w.resource::<CrosshairStyle>().name().to_string(),
                pick: style_pick,
                save: |w| w.resource::<CrosshairStyle>().name().to_string(),
                load: style_pick,
                ask_keep: false,
            },
        );
    }
}

/// What the crosshair and the prompt are drawn with, read the first time play starts.
struct Kit {
    face: Face,
    plain: Option<Picture>,
    filled: Option<Picture>,
    divider: Option<Picture>,
    /// The gun cross's bars, all four or none.
    bars: Option<[Picture; 4]>,
    /// NVForever's PlayStation face button glyphs, in [`crate::pad::Glyph::FACES`]'s order.
    glyphs: Vec<Handle<Image>>,
}

impl Kit {
    fn load(data: Option<&GameData>, images: &mut Assets<Image>, bc: bool) -> Kit {
        let mut pictures = Pictures::load(data);
        let mut picture = |path: &str| pictures.picture(path, data, images, bc);
        let (plain, filled, divider) = (picture(PLAIN), picture(FILLED), picture(DIVIDER));
        let bars: Vec<Picture> = BARS.iter().filter_map(|(path, _)| picture(path)).collect();
        let bars = bars.try_into().ok();
        let glyphs = crate::pad::Glyph::FACES.iter().map(|g| images.add(g.image(GLYPH_PIXELS))).collect();
        Kit { face: face(FONT, plain_font(36.0, 17.0), data, images, bc), plain, filled, divider, bars, glyphs }
    }
}

#[derive(Component)]
pub(crate) struct Root;

/// One of the two crosshairs: the filled one is shown while something can be used. The
/// plain one is drawn in two halves (`side` −1 left, 1 right) that the spread moves apart.
#[derive(Component)]
struct Cross {
    filled: bool,
    side: f32,
}

/// One of the gun cross's bars, pointing in from `dir` (module notes).
#[derive(Component)]
struct Bar {
    dir: Vec2,
}

/// A bar's picture's top left corner and size from the view's middle, its inner end
/// `gap` from the middle and its middle line on the cross's.
pub fn bar_rect(dir: Vec2, gap: f32) -> Rect {
    let (min, size) = match (dir.x as i32, dir.y as i32) {
        (0, -1) => (Vec2::new(-BAR_MIDDLE, -gap - BAR_LONG), Vec2::new(BAR_WIDE, BAR_LONG)),
        (0, _) => (Vec2::new(-BAR_MIDDLE, gap), Vec2::new(BAR_WIDE, BAR_LONG)),
        (-1, _) => (Vec2::new(-gap - BAR_LONG, -BAR_MIDDLE), Vec2::new(BAR_LONG, BAR_WIDE)),
        _ => (Vec2::new(gap, -BAR_MIDDLE), Vec2::new(BAR_LONG, BAR_WIDE)),
    };
    Rect::from_corners(min, min + size)
}

fn place(node: &mut Node, r: Rect) {
    let (left, top) = (crate::interface::u(r.min.x), crate::interface::u(r.min.y));
    if node.left != left || node.top != top {
        node.left = left;
        node.top = top;
    }
}

/// How far an arrow's tip stands from the view's middle for a cone of `cone` degrees
/// (half-angle) in a view `fov` radians high, in the interface's units (960 to the view's
/// height): where the cone's edge meets the screen.
pub fn spread_units(cone: f32, fov: f32) -> f32 {
    let half = (fov / 2.0).tan();
    if half <= 0.0 {
        return 0.0;
    }
    (cone.to_radians().tan() / half * 480.0).clamp(0.0, MAX_SPREAD)
}

/// The half of a picture: its left or right half.
fn half_rect(rect: Option<Rect>, side: f32) -> Rect {
    let r = rect.unwrap_or(Rect::new(0.0, 0.0, RETICLE, RETICLE));
    let mid = (r.min.x + r.max.x) / 2.0;
    if side < 0.0 {
        Rect::new(r.min.x, r.min.y, mid, r.max.y)
    } else {
        Rect::new(mid, r.min.y, r.max.x, r.max.y)
    }
}

fn picture_node((image, rect): Picture, color: Color) -> ImageNode {
    ImageNode { rect, color, image_mode: NodeImageMode::Stretch, ..ImageNode::new(image) }
}

fn spawn(commands: &mut Commands, kit: &Kit) {
    let middle = Node {
        position_type: PositionType::Absolute,
        left: Val::Percent(50.0),
        top: Val::Percent(50.0),
        width: Val::Px(0.0),
        height: Val::Px(0.0),
        ..default()
    };
    let color = amber(255.0);
    commands.spawn((Root, middle, Visibility::Hidden, Pickable::IGNORE)).with_children(|r| {
        let half = RETICLE / 2.0;
        match kit.plain.clone() {
            Some((image, rect)) => {
                for side in [-1.0, 1.0] {
                    let x = if side < 0.0 { -half } else { 0.0 };
                    r.spawn((Cross { filled: false, side }, at(x, -half, half, RETICLE), picture_node((image.clone(), Some(half_rect(rect, side))), color), Visibility::Hidden, DrawnByTiles));
                }
            }
            None => {
                for side in [-1.0, 1.0] {
                    r.spawn((Cross { filled: false, side }, at(if side < 0.0 { -5.0 } else { 2.0 }, -1.5, 3.0, 3.0), BackgroundColor(color), Visibility::Hidden, DrawnByTiles));
                }
            }
        }
        match kit.filled.clone() {
            Some(p) => r.spawn((Cross { filled: true, side: 0.0 }, at(-half, -half, RETICLE, RETICLE), picture_node(p, color), Visibility::Hidden, DrawnByTiles)),
            None => r.spawn((Cross { filled: true, side: 0.0 }, at(-3.0, -3.0, 6.0, 6.0), BackgroundColor(color), Visibility::Hidden, DrawnByTiles)),
        };
        if let Some(bars) = &kit.bars {
            for ((_, dir), p) in BARS.iter().zip(bars) {
                let rect = bar_rect(*dir, CROSS_REST);
                r.spawn((Bar { dir: *dir }, at(rect.min.x, rect.min.y, rect.width(), rect.height()), picture_node(p.clone(), color), Visibility::Hidden, DrawnByTiles));
            }
        }
        r.spawn((Texts, Node { position_type: PositionType::Absolute, ..default() }));
    });
}

fn spawn_texts(p: &mut ChildSpawnerCommands, kit: &Kit, prompt: &Prompt) {
    let color = if prompt.crime { crime_colour() } else { amber(255.0) };
    let color = if prompt.grey { color.with_alpha(GREY) } else { color };
    let l = layout(prompt, &kit.face.font);
    if let (Some(corner), Some(picture)) = (l.divider, kit.divider.clone()) {
        p.spawn((at(corner.x, corner.y, DIVIDER_SIZE.x, DIVIDER_SIZE.y), picture_node(picture, color)));
    }
    for set in &l.texts {
        set_text(p, &kit.face, set, Vec2::ZERO, color, ());
    }
    if let Some((g, corner, side)) = l.glyph {
        let i = crate::pad::Glyph::FACES.iter().position(|x| *x == g).unwrap_or(0);
        if let Some(image) = kit.glyphs.get(i) {
            p.spawn((at(corner.x, corner.y, side, side), ImageNode { color, ..ImageNode::new(image.clone()) }));
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw(
    mut commands: Commands,
    (state, wheel, style): (Res<State<PlayState>>, Option<Res<crate::weapon_wheel::WeaponWheel>>, Option<Res<CrosshairStyle>>),
    target: Res<Target>,
    sim: Res<SimState>,
    (combat, hostile, bar, camera, time, dynamic, control): (Option<Res<crate::combat::Combat>>, Option<Res<crate::hud_target::AimHostile>>, Option<Res<crate::hud_target::HudTarget>>, Query<&Projection, With<crate::PlayerCamera>>, Res<Time>, Option<Res<DynamicCrosshair>>, Option<Res<crate::control::PlayerControl>>),
    mut spread: Local<f32>,
    mut logged: Local<(bool, i32, bool, Option<String>, Option<bool>, Option<bool>, Option<CrosshairStyle>)>,
    (locks, bindings, creator, sights, scope, driven, kill_cam, tweaks, vats): (
        Option<Res<ControlLocks>>,
        Option<Res<crate::bindings::Bindings>>,
        Option<Res<crate::creator::CreatorMenu>>,
        Option<Res<crate::aim_zoom::TrueSights>>,
        Option<Res<crate::aim_zoom::Scope>>,
        Option<Res<crate::player::ViewDriven>>,
        Option<Res<crate::kill_cam::KillCam>>,
        Option<Res<crate::tweaks::TweakSettings>>,
        Option<Res<crate::vats_mode::VatsMode>>,
    ),
    data: Option<Res<GameData>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    mut kit: Local<Option<Kit>>,
    (mut shown, mut sweep): (Local<Option<Prompt>>, Option<ResMut<crate::test_aid::activation_sweep::ActivationSweep>>),
    mut root: Query<&mut Visibility, With<Root>>,
    (mut crosses, mut bars): (
        Query<(&Cross, &mut Visibility, &mut Node, Option<&mut ImageNode>), Without<Root>>,
        Query<(&Bar, &mut Visibility, &mut Node, &mut ImageNode), (Without<Root>, Without<Cross>)>,
    ),
    texts: Query<Entity, With<Texts>>,
) {
    // Record only what this drawing pass shows, including early hidden returns
    // [design: #2528, test_aid/activation_sweep.rs].
    if let Some(s) = sweep.as_deref_mut() {
        s.drawn = None;
    }
    // The HUD's lengths at the UI scale (`crate::interface::hud_units`, #1631).
    let _hud = crate::interface::hud_units();
    // Not over the character creator, whatever state the opening leaves the game in
    // [owner 2026-10-17: "E) Talk" stayed under it]. Nor while an idle holds the camera
    // (the opening's wake-up, `crate::player::ViewDriven`): only subtitles then [owner
    // 2026-10-24: "E) Talk Doc Mitchell" showed in the starting bed sequence].
    let held = driven.is_some_and(|d| d.0);
    // Nor while a kill camera plays (`crate::kill_cam`) [owner's playtest request #1034],
    // nor under the weapon wheel (docs/design/weapon-wheel.md §7).
    let killcam = kill_cam.is_some_and(|k| k.active());
    // Nor while the classic VATS style chooses targets: the pointer and the target screen
    // have the view [observed: the owner's screenshots of the original's VATS, 2026-11-10,
    // no crosshair and no prompt].
    // Nor while it plays its queued attacks (`VatsMode::playing_back`).
    let targeting = vats.is_some_and(|v| v.selected().is_some() || v.playing_back());
    let playing = *state.get() == PlayState::Playing && !creator.is_some_and(|c| c.is_open()) && !held && !killcam && !targeting && !wheel.is_some_and(|w| w.is_open());
    // What the idle keeps off the screen, once per target.
    let kept = held.then(|| target.0.as_ref().map(|t| t.name.clone())).flatten();
    if kept != logged.3 {
        if let Some(name) = &kept {
            info!("reticle: no prompt on \"{name}\": an idle holds the camera");
        }
        logged.3 = kept;
    }
    let Some(kit) = kit.as_ref() else {
        if playing {
            let loaded = Kit::load(data.as_deref(), &mut images, bc_supported(formats.as_deref()));
            spawn(&mut commands, &loaded);
            *kit = Some(loaded);
        }
        return;
    };
    let show = |on: bool| if on { Visibility::Inherited } else { Visibility::Hidden };
    // Down a gun's true iron sights the sights are the aim, through a scope its reticle
    // (docs/spec-notes/weapon-scopes.md §2): no crosshair.
    let scoped = scope.is_some_and(|s| s.on);
    // The player's tweak hides it while reloading too [design: docs/design/tweaks.md].
    // Reloading now (a reload or a jam's clearing), not `WeaponAnim::reload`, which names the
    // weapon's reload clip and is set for every weapon held: read as "reloading" it hid the
    // crosshair and every prompt for good with this tweak on (#4611).
    let reloading = tweaks.as_ref().is_some_and(|t| t.hide_crosshair_reloading) && combat.as_deref().is_some_and(crate::combat::Combat::player_reloading);
    // The player's tweak keeps it down the iron sights (a scope still hides it) [design: docs/design/tweaks.md].
    let keep = tweaks.as_ref().is_some_and(|t| t.crosshair_when_aiming);
    let sights = (sights.is_some_and(|s| s.0) && !keep) || scoped || reloading;
    if sights != logged.2 {
        logged.2 = sights;
        info!("crosshair: {}", if !sights { "shown again" } else if scoped { "hidden by the scope" } else if reloading { "hidden while reloading" } else { "hidden down the true iron sights" });
    }
    for mut v in &mut root {
        // The Tweaks tab's crosshair key hides it [design: docs/design/tweaks.md].
        v.set_if_neq(show(playing && !sights && !tweaks.as_ref().is_some_and(|t| t.crosshair_hidden)));
    }
    if !playing || sights {
        return;
    }
    let locks = locks.as_deref().copied().unwrap_or_default();
    let aimed = target.0.as_ref().is_some_and(Targeted::prompted);
    let foe = hostile.is_some_and(|h| h.0);
    let colour = if foe || target.0.as_ref().is_some_and(|t| t.crime) { crime_colour() } else { amber(255.0) };
    // The arrows' tips on the cone's edge, moving there a share of the way each frame.
    let fov = camera.iter().find_map(|p| match p {
        Projection::Perspective(p) => Some(p.fov),
        _ => None,
    });
    let dynamic = dynamic.is_none_or(|d| d.0);
    if logged.4 != Some(dynamic) {
        info!("reticle: dynamic crosshair {}", if dynamic { "on" } else { "off: the arrows stay together" });
        logged.4 = Some(dynamic);
    }
    let want = match (combat.as_ref().and_then(|c| c.crosshair_cone).filter(|_| dynamic), fov) {
        (Some(cone), Some(fov)) => spread_units(cone, fov),
        _ => 0.0,
    };
    let share = (SPREAD_SPEED * time.delta_secs() * 60.0).min(1.0);
    *spread += (want - *spread) * share;
    if (want - *spread).abs() < 0.05 {
        *spread = want;
    }
    // One line when the colour changes or the spread settles somewhere new.
    let now = (foe, spread.round() as i32);
    if now.0 != logged.0 || (*spread == want && (now.1 - logged.1).abs() >= 2) {
        info!("reticle: {} spread {:.1} (cone {:.2} deg)", if foe { "hostile" } else { "plain" }, *spread, combat.as_ref().and_then(|c| c.crosshair_cone).unwrap_or(0.0));
        (logged.0, logged.1) = now;
    }
    let style = style.map_or_else(CrosshairStyle::default, |s| *s);
    if logged.6 != Some(style) {
        info!("reticle: crosshair style {}", style.name());
        logged.6 = Some(style);
    }
    // A gun or thrown weapon drawn, with the switch on and the Cross style: the four bars
    // in place of the arrows (module notes).
    let cross = dynamic && style == CrosshairStyle::Cross && kit.bars.is_some() && combat.as_ref().is_some_and(|c| c.crosshair_cone.is_some());
    if logged.5 != Some(cross) {
        info!("reticle: {}", if cross { "gun cross (four bars)" } else { "arrows" });
        logged.5 = Some(cross);
    }
    // The player's tweaks: no crosshair in first or in third person (the prompt stays)
    // [design: docs/design/tweaks.md].
    let third = control.as_ref().is_some_and(|c| c.third_person_view());
    let view_hidden = tweaks.as_ref().is_some_and(|t| if third { t.hide_crosshair_third } else { t.hide_crosshair_first });
    let gap = spread.max(CROSS_REST);
    for (b, mut v, mut node, mut image) in &mut bars {
        v.set_if_neq(show(!view_hidden && !locks.movement && !aimed && cross));
        if image.color != colour {
            image.color = colour;
        }
        if cross {
            place(&mut node, bar_rect(b.dir, gap));
        }
    }
    let shift = (*spread - TIP).max(0.0);
    for (c, mut v, mut node, image) in &mut crosses {
        v.set_if_neq(show(!view_hidden && !locks.movement && c.filled == aimed && (c.filled || !cross)));
        if let Some(mut image) = image.filter(|i| i.color != colour) {
            image.color = colour;
        }
        if !c.filled {
            let half = RETICLE / 2.0;
            let x = if c.side < 0.0 { -half - shift } else { shift };
            let want_left = crate::interface::u(x);
            if node.left != want_left && kit.plain.is_some() {
                node.left = want_left;
            }
        }
    }
    let barred = |t: &Targeted| bar.as_ref().and_then(|b| b.0.as_ref()).is_some_and(|b| under_bar(t, b.form_id));
    // Quick Loot's list stands for the prompt while it shows (`crate::quick_loot`).
    let want = target.0.as_ref().filter(|t| !locks.rollover && t.prompted() && !barred(t) && !crate::quick_loot::holds_input()).map(|t| Prompt::bound(t, &sim, bindings.as_deref()));
    if *shown != want {
        let Ok(e) = texts.single() else { return };
        commands.entity(e).despawn_related::<Children>().with_children(|p| {
            if let Some(prompt) = &want {
                spawn_texts(p, kit, prompt);
            }
        });
        // What the prompt says, once per change (docs/spec-notes/cells-refs.md §3).
        if let Some(p) = &want {
            info!("reticle: prompt \"{}\" on \"{}\"", p.action, p.name);
        }
        *shown = want;
    }
    if let Some(s) = sweep.as_deref_mut() {
        s.drawn = shown.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character 17 wide: about the game's font 7.
    fn font() -> Font {
        plain_font(36.0, 17.0)
    }

    fn prompt(action: &str, name: &str, figures: Option<[&str; 2]>) -> Prompt {
        Prompt { action: action.into(), name: name.into(), figures: figures.map(|f| f.map(String::from)), crime: false, key: String::new(), note: None, grey: false }
    }

    #[test]
    fn crosshair_styles_are_known_by_their_names() {
        assert_eq!(CrosshairStyle::default(), CrosshairStyle::Cross);
        for s in CrosshairStyle::ALL {
            assert_eq!(CrosshairStyle::by_name(s.name()), Some(s));
        }
        assert_eq!(CrosshairStyle::by_name(" arrows "), Some(CrosshairStyle::Arrows));
        assert_eq!(CrosshairStyle::by_name("Dot"), None);
    }

    #[test]
    fn an_empty_container_says_so_under_its_name() {
        let p = Prompt { note: Some(EMPTY.into()), ..prompt("Open", "Footlocker", None) };
        let l = layout(&p, &font());
        let [_, _, name, note] = &l.texts[..] else { panic!("{:?}", l.texts) };
        assert_eq!((note.x + note.line.width / 2.0, note.base), (NAME_X, name.base + LINE_PITCH));
        assert_eq!(layout(&prompt("Open", "Footlocker", None), &font()).texts.len(), 3);
        assert_eq!(layout(&prompt("Open", "Footlocker", None), &font()).glyph, None);
        // A PlayStation face button: its glyph between a "[" and a "]" (#1452).
        let ps = Prompt { key: "[Cross]".into(), ..prompt("Open", "Footlocker", None) };
        let l = layout(&ps, &font());
        let (g, corner, side) = l.glyph.expect("a glyph");
        assert_eq!(g, crate::pad::Glyph::Cross);
        assert_eq!(l.texts[0].line.text(), "]");
        assert!(corner.x + side <= l.texts[0].x && side > 0.0);
        let open = l.texts.last().unwrap();
        assert_eq!(open.line.text(), "[");
        assert!(open.x + open.line.width <= corner.x);
        // "[L2]" has no glyph: written as it is.
        let l2 = Prompt { key: "[L2]".into(), ..prompt("Open", "Footlocker", None) };
        assert_eq!(layout(&l2, &font()).glyph, None);
    }

    #[test]
    fn the_action_and_the_name_are_centred_with_the_key_before_the_action() {
        let l = layout(&prompt("Open", "Gun Case", None), &font());
        assert_eq!(l.divider, None);
        let [key, action, name] = &l.texts[..] else { panic!("{:?}", l.texts) };
        // The key's end is the gap before the action, whatever the name's width.
        assert_eq!((key.x + key.line.width, key.base), (action.x - KEY_GAP, ACTION_BASE));
        assert_eq!(action.x + action.line.width / 2.0, ACTION_X);
        assert_eq!((name.x + name.line.width / 2.0, name.base), (NAME_X, ACTION_BASE + LINE_PITCH));
        let wider = layout(&prompt("Open", "A Much Longer Name", None), &font());
        assert_eq!(wider.texts[0], l.texts[0]);
    }

    #[test]
    fn an_items_figures_stand_either_side_of_the_stem() {
        let l = layout(&prompt("Take", "Bottle Cap", Some(["--", "1"])), &font());
        let corner = l.divider.unwrap();
        assert_eq!(corner.x + STEM_IN_PICTURE, STEM_X);
        assert_eq!(corner.y, ACTION_BASE + LINE_PITCH + DIVIDER_BELOW);
        let figures = &l.texts[3..];
        assert_eq!(figures.len(), 4);
        assert!(figures.iter().all(|s| s.base == corner.y + FIGURES_BELOW));
        let [wg, weight, val, value] = figures else { unreachable!() };
        // The weight's word starts at its place, or further left when it would meet the weight.
        assert!(wg.x <= STEM_X + WG_X && wg.x + wg.line.width + FIGURE_GAP <= weight.x + 1e-3, "{wg:?} before {weight:?}");
        assert_eq!(weight.x + weight.line.width, STEM_X + WEIGHT_END);
        assert_eq!(val.x, STEM_X + VAL_X);
        assert_eq!(value.x + value.line.width, STEM_X + VALUE_END);
        // The words are the item stats display's settings: NVForever's labels with no plugin's.
        let width = |s: &str| font().lines(s, None)[0].width;
        assert_eq!((wg.line.width, val.line.width), (width("WEIGHT"), width("Value")));
    }

    #[test]
    fn a_long_figure_and_its_word_never_meet() {
        let l = layout(&prompt("Take", "Anvil", Some(["123456789.5", "123456789012"])), &font());
        let [wg, weight, val, value] = &l.texts[3..] else { panic!("{:?}", l.texts) };
        assert_eq!(weight.x + weight.line.width, STEM_X + WEIGHT_END, "the weight stays against the stem");
        assert!(weight.x >= wg.x + wg.line.width + FIGURE_GAP - 1e-3, "{weight:?} after {wg:?}");
        assert!(value.x >= val.x + val.line.width + FIGURE_GAP, "{value:?} after {val:?}");
    }

    #[test]
    fn the_arrows_stand_on_the_cones_edge() {
        let fov = 2.0 * (0.5f32).atan();
        // tan(fov / 2) = 0.5: a cone of 45 degrees reaches twice the half height, capped.
        assert_eq!(spread_units(0.0, fov), 0.0);
        assert!((spread_units(1.0, fov) - 1f32.to_radians().tan() / 0.5 * 480.0).abs() < 1e-3);
        assert_eq!(spread_units(45.0, fov), MAX_SPREAD);
        assert_eq!(half_rect(None, -1.0), Rect::new(0.0, 0.0, 32.0, 64.0));
        assert_eq!(half_rect(Some(Rect::new(10.0, 0.0, 30.0, 20.0)), 1.0), Rect::new(20.0, 0.0, 30.0, 20.0));
    }

    #[test]
    fn the_bars_inner_ends_stand_the_gap_from_the_middle_on_one_line() {
        let gap = 10.0;
        let [top, bottom, left, right] = BARS.map(|(_, d)| bar_rect(d, gap));
        assert_eq!((top.max.y, bottom.min.y, left.max.x, right.min.x), (-gap, gap, -gap, gap));
        // Each bar's middle line (texel 2 of the 5 across) on the cross's middle.
        assert_eq!((top.min.x + BAR_MIDDLE, bottom.min.x + BAR_MIDDLE), (0.0, 0.0));
        assert_eq!((left.min.y + BAR_MIDDLE, right.min.y + BAR_MIDDLE), (0.0, 0.0));
        assert_eq!((top.size(), left.size()), (Vec2::new(BAR_WIDE, BAR_LONG), Vec2::new(BAR_LONG, BAR_WIDE)));
    }

    #[test]
    fn the_prompt_steps_aside_for_the_bar_over_the_same_actor() {
        let t = Targeted { form_id: 0x1234, name: "Cazador".into(), action: Action::Name, distance: 100.0, source: crate::activate::pick::Source::Bounds, crime: false, empty: false, through: None };
        assert!(under_bar(&t, 0x1234));
        assert!(!under_bar(&t, 0x5678));
    }

    /// Issue #2536: a living hostile under the crosshair is a target with no prompt (and
    /// no word for E); a peaceful creature with nothing to say keeps its name.
    #[test]
    fn a_hostile_actor_has_no_prompt() {
        let of = |action| Targeted { form_id: 0x1234, name: "Radscorpion".into(), action, distance: 100.0, source: crate::activate::pick::Source::Bounds, crime: false, empty: false, through: None };
        assert!(!of(Action::Hostile).prompted());
        assert_eq!(Action::Hostile.label(), "");
        for shown in [Action::Name, Action::Talk, Action::Steal, Action::Search] {
            assert!(of(shown.clone()).prompted(), "{shown:?}");
        }
    }

    #[test]
    fn weights_and_values_are_written_as_on_the_hud() {
        assert_eq!(weight_text(Some(0.0)), "--");
        assert_eq!(weight_text(None), "--");
        assert_eq!(weight_text(Some(2.0)), "2");
        assert_eq!(weight_text(Some(0.5)), "0.5");
        assert_eq!(weight_text(Some(0.25)), "0.25");
        assert_eq!(value_text(Some(1)), "1");
        assert_eq!(value_text(Some(0)), "--");
        assert_eq!(value_text(None), "--");
    }
}

#[cfg(test)]
mod string_settings_tests;
