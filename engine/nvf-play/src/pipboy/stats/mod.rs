//! The STATS page (docs/spec-notes/pipboy.md §5.1): Status (CND, RAD, EFF),
//! S.P.E.C.I.A.L., Skills, Perks and General, laid out as `menus\main\stats_menu.xml`
//! and the prefabs it includes (`list_box.xml`, `list_box_template.xml`,
//! `super_text_box.xml`, `text_box.xml`, `meter.xml`, `scrollbar_vert.xml`).
//!
//! Lengths are the canvas's (1024 × 768) and the numbers the file's [documented] unless
//! marked otherwise; where the file leaves a number to the game's code the note says what
//! it was fitted to (the owner's five screenshots, 1280 × 720).
//!
//! - **Heading**: LVL, HP (current/maximum), AP (current/maximum) and XP (experience/what
//!   the next level needs).
//! - **Status**: the CND, RAD and EFF buttons down the left; CND draws the Vault Boy with a
//!   bar of condition at each body part and the player's name and level under him, RAD
//!   the rad resistance and the rads meter, EFF the worn apparel's and aid items'
//!   changes. The Stimpak and Doctor's Bag hints (CND) and RadAway and Rad-X (RAD) are at
//!   the top right. On CND the Stimpak (S, or a click on its hint) is used on the body,
//!   or on the limb chosen; a click on a limb's bar uses one on that limb; the Doctor's
//!   Bag (E, or its hint) heals the limbs ([`use_hints`], `model::refusal` says when an
//!   item is not used, `fnv_game::aid::PARTS_LIMB_SHARE` what a Stimpak gives the limbs).
//!   RadAway (A) uses the carried item without leaving RAD; Rad-X is not built.
//! - **S.P.E.C.I.A.L., Skills, Perks**: a list at the left (nine rows at once, a scroll bar
//!   when longer), the selected row's picture and description at the right.
//! - **General**: the Reputation list (the file's first mode) or the General list of
//!   statistics with the karma's picture; R or the button at the top right switches.
//! - **Keys** [design]: Up/Down (W/S) select in a list, the arrows choose CND, RAD, EFF
//!   (S is the Stimpak's key there, E the Doctor's Bag's; 1-6 choose the head, torso,
//!   left arm, right arm, left leg or right leg for the Stimpak, the same key again the
//!   whole body; X cycles through the limbs and body; the pointer on a limb's bar
//!   chooses it while there), Page Up/Down
//!   scroll the description, the wheel
//!   scrolls the list or, over it, the description; R switches General's lists. The
//!   pointer selects the row it moves onto (as the list boxes do), clicks the buttons and
//!   the scroll arrows.
//!
//! The page's model runs in [`prepare`], before any view: the keys, the wheel, the test aid,
//! the log lines and what is shown. [`draw`] fills the hand-built page from what it left,
//! again whenever what it shows changes ([`model::Snapshot`]) or the view does
//! ([`StatsView`]).

pub mod model;
pub mod read;

#[cfg(test)]
mod input_tests;

use super::frame::{self, Align, Card, Ctx, Kit, DIMMED, FADE, FILL, LINE, THICK};
use super::labels::{built_in_label, label};
use super::{Page, PageBody, PipBoy, PipBoyScreen};
use crate::interface::{pipboy_amber as amber, at};
use bevy::ecs::system::SystemParam;
use bevy::image::CompressedImageFormatSupport;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use fnv_sim::FormId;
use model::{Row, Snapshot};
use nvf_assets::GameData;

/// The heading's cards: x and width inside the menus' rectangle (`stats_*_info`).
const LVL: (f32, f32) = (270.0, 100.0);
const HP: (f32, f32) = (380.0, 155.0);
const AP: (f32, f32) = (545.0, 135.0);
const XP: (f32, f32) = (690.0, 205.0);

/// The status page's area (`stats_status_container`): 50, 110, 855 wide, down to the tab
/// line's top (652 less its 60 high end lines).
const STATUS: Rect = Rect { min: Vec2::new(50.0, 110.0), max: Vec2::new(905.0, 592.0) };
/// The CND, RAD, EFF buttons: 84 wide, their text 20 across and 20 up and down
/// (`super_text_box.xml`), one under the other.
const SUB_WIDTH: f32 = 84.0;
const SUB_VERBUF: f32 = 20.0;
const SUB_HORBUF: f32 = 40.0;
/// The hints: right-aligned at 850, 10 in the status area, a text box 20 across and 20 up
/// and down (`text_box.xml`); the key after the box, 5 from it.
const HINT_X: f32 = 850.0;
const HINT_Y: f32 = 10.0;
const HINT_HORBUF: f32 = 20.0;
const HINT_VERBUF: f32 = 20.0;
const HINT_KEY_GAP: f32 = 5.0;
/// A text sits 3 under the middle of its box [observed: as the tab line, frame.rs].
const BOX_TEXT_DROP: f32 = 3.0;
/// The keys the hints name [observed: "S)", "E)", "A)", "X)" on the owner's
/// screenshots; documented: `_PCButton_S` and the others name these buttons].
const HINT_KEYS: [&str; 4] = ["S)", "E)", "A)", "X)"];
/// The keys themselves; with a controller the hints name the buttons that press them
/// (`crate::pad::menu_buttons`: A, X, Y on CND; A, X on RAD) [design: #1662, #1898].
pub(crate) const HINT_KEY_CODES: [KeyCode; 5] = [KeyCode::KeyS, KeyCode::KeyE, KeyCode::KeyA, KeyCode::KeyX, KeyCode::KeyX];
const SWITCH_LIMBS: usize = 4;

/// The status tab's hint `i` for the device in use.
fn hint_key(v: &StatsView, i: usize) -> String {
    let tab = if v.status == 1 { crate::pad::STATS_RAD } else { 0 };
    crate::pad::hint(&[HINT_KEY_CODES[i]], crate::pad::MenuAt { code: Some(fnv_game::game::menus::STATS), tab })
}

/// The Vault Boy (`stats_player_*`): the head 40 down, centred, at zoom 145, and each
/// piece's place and size before the zoom; the pictures fill the top left of their files
/// that size. Places are from the parent piece (the torso's from the head, the limbs'
/// from the torso).
const BOY_ZOOM: f32 = 1.45;
pub(crate) const HEAD: Vec2 = Vec2::new(85.0, 92.0);
const HEAD_Y: f32 = 40.0;
pub(crate) const FACE: (Vec2, Vec2) = (Vec2::new(21.0, 16.0), Vec2::new(48.0, 64.0));
pub(crate) const TORSO: (Vec2, Vec2) = (Vec2::new(-14.0, 86.0), Vec2::new(102.0, 128.0));
/// Left arm, right arm, left leg, right leg: place (from the torso) and size. The left
/// arm's y is not zoomed in the file.
pub(crate) const LIMB_PIECES: [(Vec2, Vec2, &str); 4] = [
    (Vec2::new(90.0, 0.0), Vec2::new(100.0, 52.0), "left_arm"),
    (Vec2::new(-88.0, -5.0), Vec2::new(96.0, 54.0), "right_arm"),
    (Vec2::new(47.0, 86.0), Vec2::new(72.0, 112.0), "left_leg"),
    (Vec2::new(-37.0, 83.0), Vec2::new(84.0, 112.0), "right_leg"),
];
/// The condition bars (`stats_player_*_pct`): 38 high; the bar in the bracket 24 down,
/// 77 long (`meter.xml`, solid).
const METER_HEIGHT: f32 = 38.0;
const BAR_Y: f32 = 24.0;
const BAR_LENGTH: f32 = 77.0;
pub(crate) const BOY_FACE: &str = "Interface\\Stats\\face_00.dds";

/// The RAD page (`stats_radiation_container`, zoom 130).
const RAD_ZOOM: f32 = 1.3;
const RAD_LEFT_EDGE: f32 = 83.0;
const RAD_Y: f32 = 360.0;
/// The meter's 0 and full marks in its picture (186 and 482, zoomed) and the most rads it
/// shows [observed: 1000 on the owner's screenshot; the file leaves it to the code].
const RAD_ZERO: f32 = 186.0;
const RAD_FULL: f32 = 482.0;
pub(crate) const RAD_MAX: f64 = 1000.0;
/// The arrow (`pointer_tail.dds`): 22 × 40 zoomed, 30 down.
const RAD_ARROW: (f32, f32, f32) = (22.0, 40.0, 30.0);
const RAD_BRACKET: &str = "Interface\\stats\\rad_effects_bracket.dds";
const RAD_METER: &str = "Interface\\Stats\\rad_meter_bracket.dds";

/// The EFF list (`stats_effects_container`): 780 wide, at the right less half a scroll
/// bar; each line 56 high, 20 in, the name 16 down and the changes 300 right of it, a
/// line along its foot.
const EFF_WIDTH: f32 = 780.0;
const EFF_ROW: f32 = 56.0;
const EFF_ROW_X: f32 = 20.0;
const EFF_TEXT_Y: f32 = 16.0;
const EFF_CHANGES_X: f32 = 300.0;

/// The lists (`stats_special_container` and the others): 15 left of the heading's line
/// (less half a scroll bar), under it (110 down); 325, 350 and 420 wide (25 and 50 more
/// when a row is marked); nine rows at once, each its text's height and 20; the rows 20
/// right of the scroll bar; a row's name 15 in and 20 down, its number right-aligned 10
/// before the right buffer (20, or 55 when a row is marked), the mark 10 after it.
const LIST_X: f32 = 35.0;
const LIST_Y: f32 = 110.0;
const LIST_WIDTH: [f32; 5] = [0.0, 325.0, 350.0, 420.0, 500.0];
const LIST_MARKED_WIDTH: [f32; 5] = [0.0, 25.0, 50.0, 0.0, 0.0];
pub(crate) const VISIBLE: usize = 9;
const ROW_SPACING: f32 = 20.0;
const SCROLLBAR: f32 = 32.0;
const ITEM_OFFSET: f32 = 20.0;
const NAME_X: f32 = 15.0;
const NAME_Y: f32 = 20.0;
/// The highlight box ends 5 before the list's right side; it is filled at alpha 40.
const HIGHLIGHT_END: f32 = 5.0;
const HIGHLIGHT_FILL_ALPHA: f32 = 40.0;
/// Scroll arrows are 32 × 16; the marker 6 wide, 12 in.
const ARROW: Vec2 = Vec2::new(32.0, 16.0);
const MARKER: (f32, f32) = (12.0, 6.0);

/// The display beside a list (`stats_display_rect`): the picture at zoom 150, 340 × 384,
/// centred, 60 above the list's top; a line 22 in and 319 down with a fading line down
/// from its right end; the description 22 in, 333 down, 175 high, scrolled 20 at a step,
/// its scroll bar's arrows at the right, 35 down and 130 apart.
const ICON_ZOOM: f32 = 1.5;
const ICON: Vec2 = Vec2::new(340.0, 384.0);
const ICON_Y: f32 = -60.0;
const SEPARATOR: (f32, f32) = (22.0, 319.0);
const DESC_Y: f32 = 333.0;
const DESC_HEIGHT: f32 = 175.0;
pub(crate) const DESC_STEP: f32 = 20.0;
const DESC_TOP: f32 = 5.0;
const DESC_BAR: (f32, f32) = (35.0, 130.0);
/// Lines of a paragraph are 35 apart [observed: message-menu.md §3, font 2].
const PITCH: f32 = 35.0;
/// The menus' right side.
const RIGHT: f32 = 905.0;

/// General (`stats_genrep_container`, 20 and 50 in): the list 500 wide (its area 542
/// high, nine rows shown, `visible`); beside it, 40 on, the karma's or the reputation's picture (zoom 150, 32 left
/// and 60 or 80 above the text block, 310 or 350 high) and their texts centred under it;
/// the switch at the top right (850, 50).
const GENERAL: Vec2 = Vec2::new(20.0, 50.0);
const GENERAL_LIST: Vec2 = Vec2::new(20.0, 50.0);
const GENERAL_HEIGHT: f32 = 542.0;
const GENERAL_BESIDE: f32 = 40.0;
const GENERAL_SWITCH: Vec2 = Vec2::new(850.0, 50.0);

/// What can be pointed at on the page.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spot {
    /// CND, RAD, EFF.
    Sub(usize),
    /// A row of the tab's list.
    Row(usize),
    /// Stimpak, Doctor's Bag, RadAway, Rad-X, limb switch.
    Hint(usize),
    /// A limb's bar on CND (`model::LIMB_NAMES`).
    Limb(usize),
    /// General's switch between its lists.
    Switch,
    DescUp,
    DescDown,
    ListUp,
    ListDown,
}

/// What the page shows of its lists: the status page's button, each tab's selected and
/// first shown row, how far the description is scrolled, General's list, what the
/// pointer is over.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct StatsView {
    /// 0 CND, 1 RAD, 2 EFF; on the tiles in Hardcore also 3 H2O, 4 FOD, 5 SLP.
    pub status: usize,
    pub selected: [usize; 5],
    pub top: [usize; 5],
    /// Steps of 20 the description is scrolled by.
    pub desc: usize,
    /// General shows the reputations (else the statistics) [documented: the file starts
    /// it in the reputation mode, `user0` 1].
    pub reputation: bool,
    pub hover: Option<Spot>,
    /// The limb chosen by keyboard or controller on CND for the Stimpak (`model::LIMB_NAMES`); `None` the
    /// whole body.
    pub limb: Option<usize>,
    /// The selected row is boxed: after a key moved the selection or while the pointer
    /// is on a row [observed: no box on the owner's S.P.E.C.I.A.L. and Skills
    /// screenshots, the pointer on the tab line or off the screen, the selected row's
    /// description shown; hypothesis: the rule].
    pub highlight: bool,
    /// The pointer when the page was drawn: a row under a pointer that has not moved
    /// since does not take the selection.
    drawn_at: Option<Vec2>,
    /// X changed `limb` since the last draw; the next draw turns it into `keep_limb`.
    cycled: bool,
    /// [design] This draw followed X changing `limb`: a limb's bar drawn under the
    /// unmoved pointer does not take the choice back (#1898). Other draws leave limb
    /// bars hovered under an unmoved pointer, as before.
    keep_limb: bool,
    /// How many rows each tab's list has, and the furthest the description scrolls.
    lens: [usize; 5],
    desc_max: usize,
    /// Rows shown at once in each tab's list, as the view that draws it says
    /// ([`StatsView::set_rows_shown`]) [hypothesis for General on the hand-built page: nine as
    /// the others; the file gives its list the area's height, 542 from 100 down, which would
    /// reach the tab line].
    rows_shown: [usize; 5],
    /// Optional explicit max scroll step per tab (from `list::counts` when variable-height
    /// wrapped rows scroll on the tiled page); `None` uses `lens - rows_shown` [design].
    scroll_most: [Option<usize>; 5],
    /// How many of the status page's buttons the view that draws it has.
    status_pages: usize,
}

impl Default for StatsView {
    fn default() -> StatsView {
        StatsView { status: 0, selected: [0; 5], top: [0; 5], desc: 0, reputation: true, hover: None, limb: None, highlight: false, drawn_at: None, cycled: false, keep_limb: false, lens: [0; 5], desc_max: 0, rows_shown: [VISIBLE; 5], scroll_most: [None; 5], status_pages: 3 }
    }
}

impl StatsView {
    /// Select a row of a tab's list, keeping it in view.
    pub fn select(&mut self, tab: usize, row: usize) {
        let len = self.lens[tab];
        let row = row.min(len.saturating_sub(1));
        if self.selected[tab] != row {
            self.selected[tab] = row;
            self.desc = 0;
        }
        self.top[tab] = model::keep_in_view(self.top[tab], row, self.rows_shown[tab], len);
    }

    /// Move the selection by `by` rows.
    pub fn step(&mut self, tab: usize, by: i32) {
        let row = (self.selected[tab] as i32 + by).max(0) as usize;
        self.select(tab, row);
        self.highlight = true;
    }

    /// Scroll a tab's list by `by` rows (the selection stays).
    pub fn scroll(&mut self, tab: usize, by: i32) {
        let most = self.list_most(tab);
        self.top[tab] = (self.top[tab] as i32 + by).clamp(0, most as i32) as usize;
    }

    /// The furthest a tab's list scrolls: the first row shown when the last row is,
    /// or the tiled scrollbar's `list::counts` maximum for variable-height lists [design].
    pub fn list_most(&self, tab: usize) -> usize {
        self.scroll_most[tab].unwrap_or_else(|| self.lens[tab].saturating_sub(self.rows_shown[tab]))
    }
    pub fn list_page(&self, tab: usize) -> usize { self.rows_shown.get(tab).copied().unwrap_or(1) }

    /// How many rows the view that draws a tab's list shows at once: each view gives its own, as
    /// with the description's bound, and what is scrolled stays within it.
    pub fn set_rows_shown(&mut self, tab: usize, rows: usize) {
        self.rows_shown[tab] = rows.max(1);
        self.top[tab] = self.top[tab].min(self.list_most(tab));
    }

    /// Explicit scroll bound for a tab's tiled list (when variable-height wrapped rows make
    /// the scrollbar's step count differ from `lens - rows_shown`) [design].
    pub fn set_scroll_most(&mut self, tab: usize, most: Option<usize>) {
        self.scroll_most[tab] = most;
        self.top[tab] = self.top[tab].min(self.list_most(tab));
    }

    pub fn scroll_desc(&mut self, by: i32) {
        self.desc = (self.desc as i32 + by).clamp(0, self.desc_max as i32) as usize;
    }

    /// How far the description scrolls, as the view that draws it measured it: each view
    /// gives its own bound (its font, wrap and window), and the scroll stays within it.
    pub fn set_desc_bound(&mut self, most: usize) {
        self.desc_max = most;
        self.desc = self.desc.min(most);
    }

    /// The furthest the description scrolls ([`StatsView::set_desc_bound`]).
    pub fn desc_bound(&self) -> usize {
        self.desc_max
    }

    /// A view with lists this long, for tests of the views over it.
    #[cfg(test)]
    pub(crate) fn with_lens(lens: [usize; 5]) -> StatsView {
        StatsView { lens, ..StatsView::default() }
    }

    /// The limb a Stimpak goes on: the one under the pointer, else the one chosen by key.
    pub fn aimed(&self) -> Option<usize> {
        match self.hover {
            Some(Spot::Limb(i)) => Some(i),
            _ => self.limb,
        }
    }

    /// Choose a limb by key; the same one again chooses the whole body.
    pub fn choose_limb(&mut self, i: usize) {
        self.limb = if self.limb == Some(i) { None } else { Some(i) };
        if matches!(self.hover, Some(Spot::Limb(_))) {
            self.hover = None;
            self.cycled = true;
        }
    }

    /// How many of the status page's buttons the view that draws it has: each view gives its own,
    /// and the page chosen stays among them.
    pub fn set_status_pages(&mut self, pages: usize) {
        self.status_pages = pages.max(1);
        self.status = self.status.min(self.status_pages - 1);
    }

    /// [design] Body, then the six limbs in model order, then body again.
    pub fn cycle_limb(&mut self) {
        self.limb = match self.aimed() {
            None => Some(0),
            Some(i) if i + 1 < model::LIMB_NAMES.len() => Some(i + 1),
            _ => None,
        };
        self.hover = None;
        self.cycled = true;
    }

    /// Switch General between its two lists.
    pub fn switch(&mut self) {
        self.reputation = !self.reputation;
        self.selected[4] = 0;
        self.top[4] = 0;
    }
}

/// Change the view through `f`, marking it changed only when it did.
fn change(view: &mut ResMut<StatsView>, f: impl FnOnce(&mut StatsView)) {
    let mut v = view.bypass_change_detection().clone();
    f(&mut v);
    if v != **view {
        **view = v;
    }
}

#[derive(SystemParam)]
pub struct Drawing<'w> {
    kit: ResMut<'w, Kit>,
    data: Option<Res<'w, GameData>>,
    images: ResMut<'w, Assets<Image>>,
    formats: Option<Res<'w, CompressedImageFormatSupport>>,
    glyphs: Res<'w, crate::pad::PadGlyphs>,
}

#[derive(SystemParam)]
pub struct Sources<'w> {
    sim: ResMut<'w, crate::sim::SimState>,
    script: Option<ResMut<'w, crate::script::ScriptState>>,
    play: Option<Res<'w, crate::PlayWorld>>,
}

#[derive(SystemParam)]
pub struct Input<'w, 's> {
    keys: crate::pad::ActionButtons<'w, KeyCode>,
    wheel: MessageReader<'w, 's, MouseWheel>,
    touch_wheel: Option<Res<'w,super::touch_scroll::WheelFrame>>,
    physical:Option<Res<'w,super::touch_scroll::PhysicalPriority>>,
    time: crate::pause::AidTime<'w>,
    asks: MessageWriter<'w, UseHint>,
    tiles: Option<Res<'w, crate::tiles::stats_menu::StatsTiles>>,
}

/// What [`prepare`] keeps between frames.
#[derive(Default)]
pub struct StateMemory {
    forms: Option<read::Forms>,
    last: Option<Snapshot>,
    wheel: f32,
    aid: Option<Vec<(f32, AidStep)>>,
    logged: String,
    /// The tab and frame the page last showed, while it shows.
    page: Option<(usize, super::Frame)>,
    /// The test aid's `longdesc`.
    long: bool,
    /// A box over the Pip-Boy held the keys last frame (`super::box_holds_keys`).
    boxed: bool,
}

/// What [`prepare`] leaves for the views of the page, the hand-built [`draw`] and the tiles'
/// (`crate::tiles::stats_menu`): what the page shows this frame, while STATS is up.
#[derive(Resource, Debug, Default)]
pub struct Prepared {
    pub snap: Option<Snapshot>,
    /// The page opened, or its tab or frame changed, this frame.
    pub new: bool,
}

/// The page's model, whichever view draws it: the opening's and a tab change's resets, the
/// test aid, the keys and the wheel, what is shown, the selection kept within the lists, and
/// the log lines. Runs before the views, every frame the page shows.
#[allow(clippy::too_many_arguments)]
pub fn prepare(
    mut commands: Commands,
    pip: Res<PipBoy>,
    view: Option<ResMut<StatsView>>,
    screen: Option<Res<PipBoyScreen>>,
    mut input: Input,
    mut src: Sources,
    mut prepared: ResMut<Prepared>,
    mut mem: Local<StateMemory>,
) {
    // Laid out in the Pip-Boy's screen texture, at its size whatever the UI scale (#1631).
    let _canvas = crate::interface::original_units();
    // [design] A box over the Pip-Boy (the tutorial; with the tiles, any menu over the page)
    // holds the view's keys and the wheel on every tab, by the rule the Pip-Boy's own keys
    // follow (`super::takes_keys`), and for the frame it closes in too: the press that closed
    // it was the box's, so A closing a tutorial over Status > RAD uses no RadAway.
    let menu = src.script.as_ref().and_then(|s| s.rt.as_ref()).and_then(|rt| rt.game.menu);
    let boxed = super::box_holds_keys(menu, input.tiles.as_deref());
    let was = std::mem::replace(&mut mem.boxed, boxed);
    let held = boxed || was;
    let Some(mut view) = view else {
        commands.init_resource::<StatsView>();
        return;
    };
    if !pip.shown() || pip.page != Page::Stats {
        input.wheel.clear();
        mem.page = None;
        prepared.snap = None;
        prepared.new = false;
        return;
    }
    let tab = pip.tab();
    let page = Some((tab, pip.frame));
    let new = mem.page != page;
    mem.page = page;
    prepared.new = new;
    if new {
        change(&mut view, |v| {
            v.highlight = false;
            v.hover = None;
            v.limb = None;
        });
    }
    // Test aid and keys.
    let aid = mem.aid.get_or_insert_with(|| std::env::var("NVF_PLAY_TEST_PIPBOY_STATS").map(|v| parse_aid(&v)).unwrap_or_default());
    // A world aid (`crate::pause::WorldAidClock`).
    let now = input.time.world_secs();
    let mut long = false;
    while let Some(now) = now.filter(|&now| aid.first().is_some_and(|a| a.0 <= now)) {
        let (_, s) = aid.remove(0);
        info!("test aid: pipboy stats {s:?} at {now:.1} s");
        let game = src.script.as_mut().and_then(|s| s.bypass_change_detection().rt.as_mut()).map(|rt| &mut rt.game);
        match (s, game) {
            (AidStep::Perk(f), Some(g)) => *g.player.perks.entry(fnv_sim::FormId(f)).or_default() += 1,
            (AidStep::Fame(f, n), Some(g)) => g.player.reputation.entry(fnv_sim::FormId(f)).or_default()[1] += n,
            (AidStep::Perk(_) | AidStep::Fame(..), None) => warn!("NVF_PLAY_TEST_PIPBOY_STATS: {s:?} needs the scripts' game"),
            (AidStep::Use(hint), game) => {
                let hardcore = game.as_ref().is_some_and(|g| g.hardcore());
                let limb = if hint == model::STIMPAK || (hint == model::DOCTORS_BAG && hardcore) { view.limb } else { None };
                input.asks.write(UseHint { hint, limb });
            }
            (AidStep::LongDesc, _) => long = true,
            _ => change(&mut view, |v| apply_aid(v, tab, s)),
        }
    }
    mem.long |= long;
    // A box over the page owns the keys and the wheel, also in the frame it closes in (`held`;
    // with the tiles, `crate::tiles::stats_menu::StatsTiles::modal` holds through that frame).
    let tiles = input.tiles.as_deref();
    if pip.open() && !held {
        let mut ask = None;
        // [design] A press that opened the page or changed its tab or frame this frame
        // is spent: the view's keys (hints, X, the digits) do not act on it, so the
        // keyboard's A stepping from S.P.E.C.I.A.L. to a Status left on RAD uses no
        // RadAway (#1899).
        if !new {
            let hardcore = src.script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.hardcore());
            change(&mut view, |v| ask = keys(&input.keys.snapshot(), v, tab, hardcore));
        }
        if let Some(a) = ask {
            input.asks.write(a);
        }
        let touch_lines=if input.physical.as_ref().is_some_and(|p|p.active) {0.0} else {input.touch_wheel.as_ref().map_or(0.0,|w|w.lines(&pip))};
        let pointer = input.touch_wheel.as_ref().filter(|_|touch_lines!=0.0).and_then(|w|w.at)
            .or_else(||screen.as_ref().and_then(|s|s.pointer));
        mem.wheel+=touch_lines;
        for w in input.wheel.read() {
            mem.wheel += match w.unit {
                MouseScrollUnit::Line => w.y,
                MouseScrollUnit::Pixel => w.y / 20.0,
            };
        }
        let whole = mem.wheel.trunc();
        if whole != 0.0 {
            mem.wheel -= whole;
            // The tiles' description is where the file put it, for the page they drew; a tab
            // or frame changed this frame has the hand-built place until they say otherwise.
            let area = tiles.and_then(|t| t.desc_area_for(tab, pip.frame)).or_else(|| desc_rect(tab));
            if pointer.is_some_and(|p| area.is_some_and(|r| r.contains(p))) {
                change(&mut view, |v| v.scroll_desc(-whole as i32));
            } else if tab > 0 {
                change(&mut view, |v| v.scroll(tab, -whole as i32));
            }
        }
    } else {
        input.wheel.clear();
    }
    // What is shown.
    let Some(play) = src.play.as_ref() else {
        prepared.snap = None;
        return;
    };
    let forms = mem.forms.get_or_insert_with(|| read::Forms::new(play.lo, play.schema));
    let script = src.script.as_mut().map(|s| s.bypass_change_detection());
    let mut snap = read::snapshot(forms, src.sim.bypass_change_detection(), script);
    if mem.long {
        for row in &mut snap.special {
            row.description = long_description();
        }
    }
    // Keep the selection within the lists.
    let lens = [0, snap.special.len(), snap.skills.len(), snap.perks.len(), snap.rows(4, view.reputation).len()];
    if view.lens != lens {
        change(&mut view, |v| {
            v.lens = lens;
            for t in 1..5 {
                let s = v.selected[t];
                v.select(t, s);
            }
        });
    }
    if new || view.is_changed() || mem.last.as_ref() != Some(&snap) {
        log_page(&view, &snap, tab, new, &mut mem);
        mem.last = Some(snap.clone());
    }
    prepared.snap = Some(snap);
}

/// The log lines of a page that changed (the smoke scenarios read them).
fn log_page(view: &StatsView, snap: &Snapshot, tab: usize, new: bool, mem: &mut StateMemory) {
    if new {
        let f = figures;
        info!(
            "pipboy stats: LVL {}, HP {}, AP {}, XP {}; {} S.P.E.C.I.A.L., {} skills, {} perks, {} effects, {} reputations, karma title {}{}",
            snap.level.map_or("--".into(), |l| l.to_string()),
            f(snap.hp),
            f(snap.ap),
            f(snap.xp),
            snap.special.len(),
            snap.skills.len(),
            snap.perks.len(),
            snap.effects.len(),
            snap.standings.len(),
            snap.karma_title.as_deref().unwrap_or("--"),
            if snap.game { "" } else { " (no scripts' game: figures not read)" }
        );
    }
    // What the CND bars show, when it changes (the smoke checks them against combat's).
    if tab == 0 && view.status == 0 && (new || mem.last.as_ref().is_none_or(|l| l.limbs != snap.limbs)) {
        let bars: Vec<String> = model::LIMB_NAMES.iter().zip(snap.limbs).map(|(n, c)| format!("{n} {:.0}%", c * 100.0)).collect();
        info!("pipboy stats: CND bars {}", bars.join(", "));
    }
    let says = describe(view, snap, tab);
    if says != mem.logged {
        info!("pipboy stats: {says}");
        mem.logged = says;
    }
}

/// Fill the STATS page's body when it is new or what it shows changed ([`prepare`]).
#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut commands: Commands,
    bodies: Query<(Entity, &PageBody)>,
    added: Query<(), Added<PageBody>>,
    view: Option<ResMut<StatsView>>,
    screen: Option<Res<PipBoyScreen>>,
    prepared: Res<Prepared>,
    mut d: Drawing,
    mut last: Local<Option<Snapshot>>,
) {
    let Some(mut view) = view else { return };
    let Some((body, b)) = bodies.iter().find(|(_, b)| b.page == Page::Stats) else { return };
    let Some(snap) = prepared.snap.as_ref() else { return };
    let tab = b.tab;
    let new = added.get(body).is_ok();
    if !new && !view.is_changed() && last.as_ref() == Some(snap) {
        return;
    }
    let mut c = Ctx::new(&mut d.kit, d.data.as_deref(), &mut d.images, d.formats.as_deref(), &d.glyphs);
    let v = view.bypass_change_detection();
    v.drawn_at = screen.as_ref().and_then(|s| s.pointer);
    v.keep_limb = std::mem::take(&mut v.cycled);
    commands.entity(body).despawn_children().with_children(|p| {
        let value = figures;
        let cards = [
            Card::new(LVL.0, LVL.1, built_in_label("sStatsLVLAbbrev"), snap.level.map_or("--".into(), |l| l.to_string())),
            Card::new(HP.0, HP.1, built_in_label("sStatsHP"), value(snap.hp)),
            Card::new(AP.0, AP.1, built_in_label("sStatsAP"), value(snap.ap)),
            Card::new(XP.0, XP.1, built_in_label("sStatsXP"), snap.xp_percent.map_or_else(|| value(snap.xp), |p| format!("{p:.0}%"))),
        ];
        frame::heading(p, &mut c, Page::Stats.title(), &cards);
        match tab {
            0 => status(p, &mut c, v, snap),
            4 => general(p, &mut c, v, snap),
            t => listed(p, &mut c, v, snap, t),
        }
    });
    *last = Some(snap.clone());
}

/// A heading card's figures, as every view of the page writes them: current/maximum, "--"
/// when they are not read.
pub fn figures(v: Option<(f64, f64)>) -> String {
    v.map_or("--".to_string(), |(a, b)| format!("{}/{}", model::whole(a), model::whole(b)))
}

/// The status page's buttons, for the log.
pub(crate) const STATUS_PAGES: [&str; 6] = ["CND", "RAD", "EFF", "H2O", "FOD", "SLP"];

/// One line for the log: what the tab shows selected.
fn describe(view: &StatsView, snap: &Snapshot, tab: usize) -> String {
    let name = Page::Stats.tabs()[tab];
    match tab {
        0 => {
            let limb = view.limb.filter(|_| view.status == 0).map_or(String::new(), |i| format!(" ({} chosen)", model::LIMB_NAMES[i]));
            format!("{name} {}{limb}", STATUS_PAGES[view.status.min(5)])
        }
        _ => {
            let rows = snap.rows(tab, view.reputation);
            let mode = match (tab, view.reputation) {
                (4, true) => " (Reputation)",
                (4, false) => " (General)",
                _ => "",
            };
            match rows.get(view.selected[tab]) {
                Some(r) => {
                    let figure: Vec<&str> = [Some(r.name.as_str()), r.value.as_deref(), r.mark.as_deref()].into_iter().flatten().collect();
                    format!("{name}{mode}: {} selected", figure.join(" "))
                }
                None => format!("{name}{mode}: empty"),
            }
        }
    }
}

/// The page's keys (module notes); a hint's item to use.
fn keys(keys: &ButtonInput<KeyCode>, view: &mut StatsView, tab: usize, hardcore: bool) -> Option<UseHint> {
    use KeyCode::*;
    if tab == 0 {
        if keys.just_pressed(ArrowUp) && view.status > 0 {
            view.status -= 1;
        }
        if keys.just_pressed(ArrowDown) && view.status + 1 < view.status_pages {
            view.status += 1;
        }
        if view.status == 0 {
            if keys.just_pressed(KeyX) {
                view.cycle_limb();
            }
            for (i, k) in [Digit1, Digit2, Digit3, Digit4, Digit5, Digit6].into_iter().enumerate() {
                if keys.just_pressed(k) {
                    view.choose_limb(i);
                }
            }
        }
        let mut ask = None;
        for (i, k) in [KeyS, KeyE, KeyA, KeyX].into_iter().enumerate() {
            let shown = (view.status == 0 && i < 2) || (view.status == 1 && i >= 2);
            if shown && keys.just_pressed(k) {
                ask = hint_pressed(view, i, hardcore);
            }
        }
        return ask;
    }
    if keys.any_just_pressed([ArrowUp, KeyW]) {
        view.step(tab, -1);
    }
    if keys.any_just_pressed([ArrowDown, KeyS]) {
        view.step(tab, 1);
    }
    if keys.just_pressed(PageUp) {
        view.scroll_desc(-1);
    }
    if keys.just_pressed(PageDown) {
        view.scroll_desc(1);
    }
    if tab == 4 && keys.just_pressed(KeyR) {
        view.switch();
    }
    None
}

/// Which hint item a click on a limb's bar, word or picture uses on CND: a Doctor's Bag in
/// Hardcore mode (where Stimpaks mend no limb), else a Stimpak [observed: FalloutNV.esm MESG
/// `HelpHealingLimbs`; design: #3431].
pub(crate) fn limb_click_hint(hardcore: bool) -> usize {
    if hardcore { model::DOCTORS_BAG } else { model::STIMPAK }
}

/// A hint's key or button: the item to use (a Stimpak on the limb aimed at, and in Hardcore
/// a Doctor's Bag on the limb aimed at or on all limbs when none is aimed), or `None` for
/// Rad-X, which is not used from here, or the separate limb-switch hint.
pub(crate) fn hint_pressed(view: &StatsView, hint: usize, hardcore: bool) -> Option<UseHint> {
    match hint {
        model::STIMPAK => Some(UseHint { hint, limb: view.aimed() }),
        model::DOCTORS_BAG => Some(UseHint { hint, limb: hardcore.then(|| view.aimed()).flatten() }),
        model::RADAWAY => Some(UseHint { hint, limb: None }),
        SWITCH_LIMBS => None,
        _ => {
            info!("pipboy stats: {} (using it from the STATS page is not built)", HINT_KEYS[hint]);
            None
        }
    }
}

// ---- pointer --------------------------------------------------------------------------

fn on_over(over: On<PointerOver>, spots: Query<&Spot>, view: Option<ResMut<StatsView>>, pip: Res<PipBoy>, screen: Option<Res<PipBoyScreen>>) {
    let (Ok(&spot), Some(mut view)) = (spots.get(over.entity), view) else { return };
    let tab = pip.tab();
    // A row drawn under a pointer that has not moved keeps the selection; [design] so
    // does the limb X just chose, against a limb's bar redrawn under it (`keep_limb`).
    let at = screen.as_ref().and_then(|s| s.pointer);
    let kept = match spot {
        Spot::Row(_) => true,
        Spot::Limb(_) => view.keep_limb,
        _ => false,
    };
    if kept && at.is_some() && at == view.drawn_at {
        return;
    }
    match spot {
        Spot::Row(i) => {
            change(&mut view, |v| {
                v.select(tab, i);
                v.highlight = true;
            });
        }
        s => change(&mut view, |v| v.hover = Some(s)),
    }
}

fn on_out(out: On<PointerOut>, spots: Query<&Spot>, view: Option<ResMut<StatsView>>) {
    let (Ok(&spot), Some(mut view)) = (spots.get(out.entity), view) else { return };
    change(&mut view, |v| {
        if v.hover == Some(spot) {
            v.hover = None;
        }
        if matches!(spot, Spot::Row(_)) {
            v.highlight = false;
        }
    });
}

fn on_click(click: On<PointerClick>, spots: Query<&Spot>, view: Option<ResMut<StatsView>>, pip: Res<PipBoy>, prepared: Option<Res<Prepared>>, mut asks: MessageWriter<UseHint>) {
    let (Ok(&spot), Some(mut view)) = (spots.get(click.entity), view) else { return };
    let tab = pip.tab();
    let hardcore = prepared.as_ref().and_then(|p| p.snap.as_ref()).is_some_and(|s| s.hardcore);
    let ask = match spot {
        Spot::Hint(i) => hint_pressed(&view, i, hardcore),
        Spot::Limb(i) => Some(UseHint { hint: limb_click_hint(hardcore), limb: Some(i) }),
        _ => None,
    };
    if let Some(a) = ask {
        asks.write(a);
    }
    change(&mut view, |v| match spot {
        Spot::Sub(i) => v.status = i,
        Spot::Row(i) => v.select(tab, i),
        Spot::Switch => v.switch(),
        Spot::Hint(SWITCH_LIMBS) => v.cycle_limb(),
        Spot::DescUp => v.scroll_desc(-1),
        Spot::DescDown => v.scroll_desc(1),
        Spot::ListUp => v.scroll(tab, -1),
        Spot::ListDown => v.scroll(tab, 1),
        Spot::Hint(_) | Spot::Limb(_) => {}
    });
}

/// A place that takes the pointer (drawn last, over what it covers).
fn spot(p: &mut ChildSpawnerCommands, r: Rect, s: Spot) {
    p.spawn((at(r.min.x, r.min.y, r.width(), r.height()), s, Pickable::default())).observe(on_over).observe(on_out).observe(on_click);
}

// ---- drawing --------------------------------------------------------------------------

/// A text box (`text_box.xml`): the text's width and `horbuf`, its height and `verbuf`.
fn text_box(c: &Ctx, s: &str, horbuf: f32, verbuf: f32) -> Vec2 {
    Vec2::new(c.width(2, s) + horbuf, c.line_height(2) + verbuf)
}

/// The text's top in a box `h` high from `top`.
fn box_text_top(c: &Ctx, top: f32, h: f32) -> f32 {
    top + (h - c.line_height(2)) / 2.0 + BOX_TEXT_DROP
}

fn status_hints(status: usize) -> &'static [(usize, usize)] {
    match status {
        0 => &[(0, 0), (SWITCH_LIMBS, 1), (1, 2)],
        1 => &[(2, 0), (3, 1)],
        _ => &[],
    }
}

fn status(p: &mut ChildSpawnerCommands, c: &mut Ctx, v: &mut StatsView, s: &Snapshot) {
    v.set_status_pages(3);
    let o = STATUS.min;
    let h = c.line_height(2) + SUB_VERBUF;
    let mut spots = Vec::new();
    let names = [built_in_label("sStatsCNDAbbrev"), built_in_label("sStatsRADAbbrev"), built_in_label("sStatsEFFAbbrev")];
    for (i, name) in names.iter().enumerate() {
        let r = Rect::new(o.x, o.y + h * i as f32, o.x + SUB_WIDTH, o.y + h * (i as f32 + 1.0));
        let selected = v.status == i;
        if selected || v.hover == Some(Spot::Sub(i)) {
            let fill = selected.then(|| amber(FILL.0).with_alpha(FILL.1 / 255.0));
            c.boxed(p, r, amber(LINE), fill);
        }
        c.text(p, 2, name, r.min.x + SUB_HORBUF / 2.0, box_text_top(c, r.min.y, h), Align::Left, c.bright());
        spots.push((r, Spot::Sub(i)));
    }
    // Keep the existing slots: Stimpak, limb switch, Doctor's Bag; or RadAway, Rad-X.
    for &(i, slot) in status_hints(v.status) {
        let text = if i == SWITCH_LIMBS { &s.switch_limbs } else { &s.hints.texts[i] };
        let size = text_box(c, text, HINT_HORBUF, HINT_VERBUF);
        let right = o.x + HINT_X;
        let top = o.y + HINT_Y + size.y * slot as f32;
        let r = Rect::new(right - size.x, top, right, top + size.y);
        let hovered = v.hover == Some(Spot::Hint(i));
        // Dim unless pointed at or usable [observed: dim on all three screenshots;
        // hypothesis: bright with its box when pointed at; design: owner 2026-10-27, a
        // Stimpak or Doctor's Bag that would heal is lit (`Hints::lit`)].
        let lit = i == SWITCH_LIMBS || s.hints.lit[i];
        let colour = if hovered || lit { c.bright() } else { amber(DIMMED) };
        if hovered {
            c.boxed(p, r, amber(LINE), None);
        }
        let ty = box_text_top(c, top, size.y);
        c.text(p, 2, text, right - HINT_HORBUF / 2.0, ty, Align::Right, colour);
        c.text(p, 2, &hint_key(v, i), right + HINT_KEY_GAP, ty, Align::Left, colour);
        spots.push((r, Spot::Hint(i)));
    }
    match v.status {
        0 => condition(p, c, v, s, &mut spots),
        1 => radiation(p, c, s),
        _ => effects(p, c, s),
    }
    for (r, sp) in spots {
        spot(p, r, sp);
    }
}

/// CND: the Vault Boy, his bars and the name; each bar takes the pointer (a Stimpak on
/// that limb), boxed while pointed at or chosen [design].
fn condition(p: &mut ChildSpawnerCommands, c: &mut Ctx, v: &StatsView, s: &Snapshot, spots: &mut Vec<(Rect, Spot)>) {
    let z = BOY_ZOOM;
    let head_size = HEAD * z;
    let head = Vec2::new(STATUS.min.x + (STATUS.width() - head_size.x) / 2.0, STATUS.min.y + HEAD_Y);
    let crippled = |i: usize| s.limbs[i] <= 0.0;
    // A crippled piece by its "_broken" picture [hypothesis: the files' names].
    let piece = boy_picture;
    // Only the actual own image supplies density, independently for every piece.
    // Atlas-backed pieces retain the existing zoom (pipboy-paperdoll-density.md).
    let head_path = piece("head", crippled(0));
    let head_zoom = z * c.picture_zoom(&head_path, 128.0);
    c.image_cut(p, &head_path, head.x, head.y, head_size.x, head_size.y, head_zoom, c.bright());
    // The face [hypothesis: face_00, the smile on the owner's screenshot; when the
    // others (01–04, 10) show is not known], centred in the head's height and 16 lower.
    let face_size = FACE.1 * z;
    let face_zoom = z * c.picture_zoom(BOY_FACE, 128.0);
    c.image_cut(p, BOY_FACE, head.x + FACE.0.x, head.y + (head_size.y - face_size.y) / 2.0 + FACE.0.y * z, face_size.x, face_size.y, face_zoom, c.bright());
    let torso = head + TORSO.0 * z;
    let torso_path = piece("torso", crippled(1));
    let torso_zoom = z * c.picture_zoom(&torso_path, 128.0);
    c.image_cut(p, &torso_path, torso.x, torso.y, TORSO.1.x * z, TORSO.1.y * z, torso_zoom, c.bright());
    for (i, (place, size, name)) in LIMB_PIECES.iter().enumerate() {
        let y = if i == 0 { place.y } else { place.y * z };
        let path = piece(name, crippled(i + 2));
        let zoom = z * c.picture_zoom(&path, 128.0);
        c.image_cut(p, &path, torso.x + place.x * z, torso.y + y, size.x * z, size.y * z, zoom, c.bright());
    }
    // The bars, from the head: its bracket's file, place, the bar's x in it, and where
    // "Crippled" goes (right of the bracket's x, and down by the bar's y or not).
    let head_w = c.picture_size("Interface\\VATS\\vats_head_divider.dds").map_or(128.0, |s| s.x);
    let centred = (head_size.x - head_w) / 2.0;
    let bars: [(&str, Vec2, f32, f32, bool); 6] = [
        ("vats_head_divider", Vec2::new(centred, -METER_HEIGHT - 5.0), 25.0, 10.0, true),
        ("vats_head_divider", Vec2::new(centred, head_size.y), 25.0, 10.0, true),
        ("vats_left_divider", Vec2::new(150.0, 44.0), 51.0, 10.0, true),
        ("vats_right_divider", Vec2::new(-190.0, 44.0), 25.0, 20.0, true),
        ("vats_left_divider", Vec2::new(150.0, 285.0), 51.0, 20.0, false),
        ("vats_right_divider", Vec2::new(-190.0, 285.0), 25.0, 20.0, false),
    ];
    for (i, (file, place, bar_x, text_x, text_down)) in bars.into_iter().enumerate() {
        let b = head + place;
        let path = format!("Interface\\VATS\\{file}.dds");
        let w = c.picture_size(&path).map_or(256.0, |s| s.x);
        let r = Rect::new(b.x, b.y, b.x + w, b.y + METER_HEIGHT);
        if v.limb == Some(i) || v.hover == Some(Spot::Limb(i)) {
            c.boxed(p, r, amber(LINE), None);
        }
        spots.push((r, Spot::Limb(i)));
        if crippled(i) {
            // The bracket gives way to the word [documented: the text shows when the bar
            // does not; hypothesis: the word, `sStatsCrippled` is not in the masters].
            c.text(p, 2, "Crippled", b.x + text_x, b.y + if text_down { BAR_Y } else { 0.0 }, Align::Left, c.bright());
            continue;
        }
        c.image_cut(p, &path, b.x, b.y, w, METER_HEIGHT, 1.0, c.bright());
        c.line(p, b.x + bar_x, b.y + BAR_Y, BAR_LENGTH * s.limbs[i] as f32, METER_HEIGHT - BAR_Y, c.bright());
    }
    // The name and level under him [observed: "Loot - Level 1"], centred, its middle on
    // the area's foot.
    let name = match s.level {
        Some(l) => format!("{} - {} {l}", s.name, label("sLevelAbbrev", "Level")),
        None => s.name.clone(),
    };
    let th = c.line_height(2);
    c.text(p, 2, &name, STATUS.min.x + STATUS.width() / 2.0, STATUS.max.y - th / 2.0, Align::Centre, c.bright());
}

/// RAD: the resistance and the meter.
fn radiation(p: &mut ChildSpawnerCommands, c: &mut Ctx, s: &Snapshot) {
    let z = RAD_ZOOM;
    let o = STATUS.min;
    let bracket = c.picture_size(RAD_BRACKET).unwrap_or(Vec2::new(512.0, 128.0)) * z;
    let bracket_x = STATUS.width() - bracket.x;
    let left_edge = RAD_LEFT_EDGE * z;
    // The resistance: a window up to 10 before the effects' label, showing the bracket's
    // right end.
    let width = bracket_x + left_edge - 10.0;
    let top = o.y + RAD_Y;
    p.spawn((Node { overflow: Overflow::clip(), ..at(o.x, top, width, bracket.y) }, Pickable::IGNORE)).with_children(|q| {
        c.image_cut(q, RAD_BRACKET, width - bracket.x, 0.0, bracket.x, bracket.y, z, c.bright());
        c.text(q, 2, "RAD RESIST", 0.0, NAME_Y, Align::Left, c.bright());
        let level = s.rad_resist.map_or("--".to_string(), |r| format!("{}%", model::whole(r)));
        c.text(q, 2, &level, width - 20.0, NAME_Y, Align::Right, c.bright());
    });
    // The meter.
    let meter = c.picture_size(RAD_METER).unwrap_or(Vec2::new(512.0, 128.0)) * z;
    let mx = o.x + bracket_x;
    c.image_cut(p, RAD_METER, mx, top, meter.x, meter.y, z, c.bright());
    c.text(p, 2, "RADS", mx + left_edge, top + NAME_Y, Align::Left, c.bright());
    let (zero, full) = (RAD_ZERO * z, RAD_FULL * z);
    // [observed: no "0" at the zero mark on the owner's screenshot, nor a number at the
    // arrow at 0 rads; hypothesis: a 0 is not written.]
    c.text(p, 2, &model::whole(RAD_MAX / 2.0), mx + zero + (full - zero) / 2.0, top - 28.0, Align::Centre, c.bright());
    c.text(p, 2, &model::whole(RAD_MAX), mx + meter.x, top - 28.0, Align::Right, c.bright());
    let arrow = Vec2::new(RAD_ARROW.0, RAD_ARROW.1) * z;
    let ax = mx + zero - arrow.x / 2.0 + (full - zero) * (s.rads.min(RAD_MAX) / RAD_MAX) as f32;
    c.image_cut(p, "Interface\\Shared\\Arrow\\pointer_tail.dds", ax, top + RAD_ARROW.2, arrow.x, arrow.y, z, c.bright());
    if s.rads >= 0.5 {
        c.text(p, 2, &model::whole(s.rads), ax, top + RAD_ARROW.2 + arrow.y - 24.0, Align::Right, c.bright());
    }
}

/// EFF: one line per source of changes, the list in the middle of the status area.
fn effects(p: &mut ChildSpawnerCommands, c: &mut Ctx, s: &Snapshot) {
    let th = c.line_height(2);
    if s.effects.is_empty() {
        // [hypothesis: the words; `sStatsNoEffects` is not in the masters.]
        c.text(p, 2, "No Effects", STATUS.min.x + STATUS.width() / 2.0, STATUS.min.y + (STATUS.height() - th) / 2.0, Align::Centre, c.bright());
        return;
    }
    // The list is as high as its lines [hypothesis: fitted to the owner's screenshot,
    // where the one line stands at the middle].
    let x = STATUS.min.x + STATUS.width() - EFF_WIDTH + SCROLLBAR / 2.0 + EFF_ROW_X;
    let height = EFF_ROW * s.effects.len() as f32;
    let mut y = STATUS.min.y + ((STATUS.height() - height) / 2.0).max(0.0);
    let line_w = EFF_WIDTH - SCROLLBAR - ITEM_OFFSET / 2.0 - HIGHLIGHT_END - EFF_ROW_X;
    for (i, e) in s.effects.iter().enumerate() {
        if y + EFF_ROW > STATUS.max.y {
            break;
        }
        c.text(p, 2, &e.name, x, y + EFF_TEXT_Y, Align::Left, c.bright());
        c.text(p, 2, &e.text, x + EFF_CHANGES_X, y + EFF_TEXT_Y, Align::Left, c.bright());
        // A line under each but the last [hypothesis: none under the one line on the
        // screenshot].
        if i + 1 < s.effects.len() {
            c.line(p, x, y + EFF_ROW - THICK, line_w, THICK, amber(LINE));
        }
        y += EFF_ROW;
    }
}

/// The list's place and width for a tab.
fn list_rect(tab: usize, rows: &[Row], row_h: f32) -> Rect {
    if tab == 4 {
        let o = GENERAL + GENERAL_LIST;
        return Rect::new(o.x, o.y, o.x + LIST_WIDTH[4], o.y + VISIBLE as f32 * row_h);
    }
    let marked = rows.iter().any(|r| r.mark.is_some());
    let w = LIST_WIDTH[tab] + if marked { LIST_MARKED_WIDTH[tab] } else { 0.0 };
    Rect::new(LIST_X, LIST_Y, LIST_X + w, LIST_Y + VISIBLE as f32 * row_h)
}

/// About where a list tab's description is, for the wheel.
fn desc_rect(tab: usize) -> Option<Rect> {
    (1..=3).contains(&tab).then(|| Rect::new(LIST_X + LIST_WIDTH[tab] + SEPARATOR.0, LIST_Y + DESC_Y, RIGHT, LIST_Y + DESC_Y + DESC_HEIGHT))
}

/// A list (`list_box.xml`) of a tab's rows in `r`; the places that take the pointer go to
/// `spots`.
/// How far right a bold row's name is drawn again, interface units [design].
const BOLD_SHIFT: f32 = 1.0;

fn list(p: &mut ChildSpawnerCommands, c: &mut Ctx, v: &StatsView, tab: usize, rows: &[Row], r: Rect, spots: &mut Vec<(Rect, Spot)>) {
    let row_h = c.line_height(2) + ROW_SPACING;
    let marked = rows.iter().any(|r| r.mark.is_some());
    // The General list leaves its right buffer to the code [hypothesis: 20 as the
    // others].
    let right_buffer = 20.0 + if marked && tab < 3 { 35.0 } else { 0.0 };
    let item_x = r.min.x + SCROLLBAR + ITEM_OFFSET;
    let item_w = r.max.x - item_x - HIGHLIGHT_END;
    let shown = ((r.height() / row_h).floor() as usize).max(1);
    let top = v.top[tab].min(rows.len().saturating_sub(shown));
    for (k, (i, row)) in rows.iter().enumerate().skip(top).take(shown).enumerate() {
        let y = r.min.y + row_h * k as f32;
        if i == v.selected[tab] && v.highlight {
            let fill = amber(FILL.0).with_alpha(HIGHLIGHT_FILL_ALPHA / 255.0);
            c.boxed(p, Rect::new(item_x, y, item_x + item_w, y + row_h - 1.0), amber(LINE), Some(fill));
        }
        c.text(p, 2, &row.name, item_x + NAME_X, y + NAME_Y, Align::Left, c.bright());
        // Bold, as the fonts have no bold face: the name again a unit to the right [design].
        if row.bold {
            c.text(p, 2, &row.name, item_x + NAME_X + BOLD_SHIFT, y + NAME_Y, Align::Left, c.bright());
        }
        let num_x = item_x + item_w - right_buffer - 10.0;
        if let Some(n) = &row.value {
            c.text(p, 2, n, num_x, y + NAME_Y, Align::Right, c.bright());
        }
        if let Some(m) = &row.mark {
            c.text(p, 2, m, num_x + 10.0, y + NAME_Y, Align::Left, c.bright());
        }
        spots.push((Rect::new(item_x, y, item_x + item_w, y + row_h), Spot::Row(i)));
    }
    // The scroll bar at the left when the rows do not all show: arrows at the list's top
    // and foot [hypothesis: fitted to the Skills screenshot], the marker between them,
    // its height the track's over the hidden rows and 2 [hypothesis: fitted there].
    if rows.len() > shown {
        let up = Rect::from_corners(r.min, r.min + ARROW);
        let down = Rect::new(r.min.x, r.max.y - ARROW.y, r.min.x + ARROW.x, r.max.y);
        arrow(p, c, up, "arrow_up.dds");
        arrow(p, c, down, "arrow_down.dds");
        let len = down.min.y - up.max.y;
        let hidden = rows.len() - shown;
        let mh = (len / (hidden + 2) as f32).max(32.0);
        let my = up.max.y + (len - mh) * top as f32 / hidden as f32;
        if c.image(p, "vert_marker.dds", at(r.min.x + MARKER.0, my, MARKER.1, mh), c.bright()).is_none() {
            c.line(p, r.min.x + MARKER.0, my, MARKER.1, mh, c.bright());
        }
        spots.push((up, Spot::ListUp));
        spots.push((down, Spot::ListDown));
        super::scrollbar::hits(p, Page::Stats, tab, Rect::from_corners(up.min, down.max),
            Rect::new(r.min.x,up.max.y,r.min.x+ARROW.x,down.min.y),
            Rect::new(r.min.x,my,r.min.x+ARROW.x,my+mh),Vec2::ZERO);
    }
}

fn arrow(p: &mut ChildSpawnerCommands, c: &mut Ctx, r: Rect, name: &str) {
    let path = format!("Interface\\Shared\\scrollbar\\{name}");
    if c.image(p, &path, at(r.min.x, r.min.y, r.width(), r.height()), c.bright()).is_none() {
        c.line(p, r.min.x, r.min.y + r.height() / 2.0, r.width(), THICK, c.bright());
    }
}

/// S.P.E.C.I.A.L., Skills, Perks: the list and the selected row's picture and
/// description.
fn listed(p: &mut ChildSpawnerCommands, c: &mut Ctx, v: &mut StatsView, s: &Snapshot, tab: usize) {
    v.set_rows_shown(tab, VISIBLE);
    let rows = s.rows(tab, false);
    let r = list_rect(tab, &rows, c.line_height(2) + ROW_SPACING);
    let mut spots = Vec::new();
    // The display beside the list.
    let d = Rect::new(r.max.x, r.min.y, RIGHT, r.max.y);
    v.desc_max = 0;
    if let Some(row) = rows.get(v.selected[tab]) {
        if let Some(icon) = &row.icon {
            let zoom = ICON_ZOOM * c.picture_zoom(icon, 256.0);
            c.image_cut(p, icon, d.min.x + (d.width() - ICON.x) / 2.0, d.min.y + ICON_Y, ICON.x, ICON.y, zoom, c.bright());
        }
        let sep = Vec2::new(d.min.x + SEPARATOR.0, d.min.y + SEPARATOR.1);
        c.line(p, sep.x, sep.y, d.max.x - sep.x, THICK, amber(LINE));
        c.fade(p, d.max.x - THICK, sep.y, FADE, false, amber(LINE));
        let dr = Rect::new(sep.x, d.min.y + DESC_Y, d.max.x - SCROLLBAR / 2.0, d.min.y + DESC_Y + DESC_HEIGHT);
        // It scrolls when its lines are more than the window holds [observed: five lines
        // (175) fit without a scroll bar on the owner's S.P.E.C.I.A.L. screenshot].
        let lines = c.lines(2, &row.description, Some(dr.width())).len();
        let text_h = PITCH * lines as f32;
        v.set_desc_bound(((text_h - DESC_HEIGHT) / DESC_STEP).ceil().max(0.0) as usize);
        let scrolled = v.desc as f32 * DESC_STEP;
        p.spawn((Node { overflow: Overflow::clip(), ..at(dr.min.x, dr.min.y, dr.width(), dr.height()) }, Pickable::IGNORE)).with_children(|q| {
            c.paragraph(q, 2, &row.description, 0.0, DESC_TOP - scrolled, dr.width(), PITCH, c.bright());
        });
        if v.desc_max > 0 {
            let x = dr.max.x;
            let up = Rect::new(x, dr.min.y + DESC_BAR.0 - ARROW.y, x + ARROW.x, dr.min.y + DESC_BAR.0);
            let down = Rect::new(x, dr.min.y + DESC_BAR.0 + DESC_BAR.1, x + ARROW.x, dr.min.y + DESC_BAR.0 + DESC_BAR.1 + ARROW.y);
            arrow(p, c, up, "arrow_up.dds");
            arrow(p, c, down, "arrow_down.dds");
            spots.push((up, Spot::DescUp));
            spots.push((down, Spot::DescDown));
        }
    }
    list(p, c, v, tab, &rows, r, &mut spots);
    for (r, sp) in spots {
        spot(p, r, sp);
    }
}

/// General: the reputations or the statistics, the reputation's or the karma's picture
/// beside, the switch at the top right.
fn general(p: &mut ChildSpawnerCommands, c: &mut Ctx, v: &mut StatsView, s: &Snapshot) {
    v.set_rows_shown(4, VISIBLE);
    // The player's tweak: no reputations, no reputation list [design: docs/design/tweaks.md].
    if s.hide_reputation {
        v.reputation = false;
    }
    let rows = s.rows(4, v.reputation);
    let mut spots = Vec::new();
    let th = c.line_height(2);
    let r = list_rect(4, &rows, th + ROW_SPACING);
    // Beside the list: from 40 after it to 40 before the menus' right side (the file
    // measures both from the area's left).
    let bx = GENERAL.x + GENERAL_LIST.x + LIST_WIDTH[4] + GENERAL_BESIDE;
    let bw = RIGHT - (GENERAL_LIST.x + LIST_WIDTH[4] + GENERAL_BESIDE) - GENERAL_BESIDE;
    let mid = bx + bw / 2.0;
    if v.reputation {
        if let Some(st) = s.standings.get(v.selected[4]) {
            // The block: the picture 80 above it, 350 high; the title 20 under the
            // picture, the reputation's name 50 lower [observed: the owner's screenshots,
            // the title over "Goodsprings"; the file leaves it to the code].
            let h = 290.0 + th;
            let top = r.min.y + (GENERAL_HEIGHT - h) / 2.0;
            if let Some(icon) = &st.icon {
                let zoom = ICON_ZOOM * c.picture_zoom(icon, 256.0);
                c.image_cut(p, icon, bx - 32.0, top - 80.0, 256.0 * ICON_ZOOM, 350.0, zoom, c.bright());
            }
            centred(p, c, st.title, mid, top + 290.0, bw);
            centred(p, c, &st.name, mid, top + 340.0, bw);
        }
    } else if let Some((karma, word, picture)) = s.karma {
        // The block: the alignment at its top, the picture 60 above it and 310 high, the
        // karma title 20 under the picture and the karma's number a line under that
        // [hypothesis: the places; the title by level and karma, `model::karma_title`].
        let h = 270.0 + 2.0 * th;
        let top = r.min.y + (GENERAL_HEIGHT - h) / 2.0;
        centred(p, c, &format!("Karma: {word}"), mid, top, bw);
        let path = karma_picture(picture);
        let zoom = ICON_ZOOM * c.picture_zoom(&path, 256.0);
        c.image_cut(p, &path, bx - 32.0, top - 60.0, 256.0 * ICON_ZOOM, 310.0, zoom, c.bright());
        let mut y = top + 270.0;
        if let Some(title) = &s.karma_title {
            centred(p, c, title, mid, y, bw);
            y += th;
        }
        centred(p, c, &model::whole(karma), mid, y, bw);
    }
    if !s.hide_reputation {
        // The switch: the other list's name [hypothesis], its key after it.
        let name = if v.reputation { built_in_label("sStatsGeneral") } else { "Reputation" };
        let size = text_box(c, name, HINT_HORBUF, HINT_VERBUF);
        let right = GENERAL.x + GENERAL_SWITCH.x;
        let top = GENERAL.y + GENERAL_SWITCH.y;
        let sr = Rect::new(right - size.x, top, right, top + size.y);
        if v.hover == Some(Spot::Switch) {
            c.boxed(p, sr, amber(LINE), None);
        }
        let ty = box_text_top(c, top, size.y);
        c.text(p, 2, name, right - HINT_HORBUF / 2.0, ty, Align::Right, c.bright());
        // R, with a controller X (`stats_menu.xml`'s, `crate::pad::menu_buttons`).
        let key = crate::pad::hint(&[KeyCode::KeyR], crate::pad::MenuAt { code: Some(fnv_game::game::menus::STATS), tab: 4 });
        c.text(p, 2, &key, right + HINT_KEY_GAP, ty, Align::Left, c.bright());
        spots.push((sr, Spot::Switch));
    }
    if rows.is_empty() && v.reputation {
        // [hypothesis: the words.]
        c.text(p, 2, "No reputations yet", r.min.x + r.width() / 2.0, r.min.y + (r.height() - th) / 2.0, Align::Centre, amber(DIMMED));
    }
    list(p, c, v, 4, &rows, r, &mut spots);
    for (r, sp) in spots {
        spot(p, r, sp);
    }
}

/// The file of a piece of the Vault Boy, a crippled one by its "_broken" picture, as every view of
/// the page reads it [hypothesis: the names of the head's and the others'; observed:
/// `left_leg_broken.dds`, private job run07_stats_tabs].
pub fn boy_picture(name: &str, broken: bool) -> String {
    format!("Interface\\Stats\\{name}{}.dds", if broken { "_broken" } else { "" })
}

/// The file of a karma's picture ([`model::karma_alignment`]'s name), as every view of the page
/// reads it.
pub fn karma_picture(picture: &str) -> String {
    format!("Interface\\Icons\\PipboyImages\\Karma Icons\\{picture}.dds")
}

/// `s` broken at `wrap`, each line centred on `mid`.
fn centred(p: &mut ChildSpawnerCommands, c: &mut Ctx, s: &str, mid: f32, top: f32, wrap: f32) {
    for (i, l) in c.lines(2, s, Some(wrap)).iter().enumerate() {
        c.set(p, 2, l, mid, top + PITCH * i as f32, Align::Centre, c.bright());
    }
}

// ---- using the hints' items -------------------------------------------------------------

/// Use a hint's item ([`model::STIMPAK`], [`model::DOCTORS_BAG`], [`model::RADAWAY`]) from the STATS
/// page; `limb` the body part a Stimpak goes on (`model::LIMB_NAMES`), `None` the body.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct UseHint {
    pub hint: usize,
    pub limb: Option<usize>,
}

/// Use the items the page asked for, as the inventory does (`crate::menu`): one is taken
/// from the player, its sound plays, and the script runtime uses it (`crate::ItemUsed`,
/// `crate::aid::use_item`), a Stimpak on the limb chosen (`fnv_game::Game::ingest_on`).
/// An item that would not heal is not used (`model::refusal`). Log lines:
/// `pipboy stats: used <item> on the <limb>|on the body (<n> left)`,
/// `pipboy stats: <item> not used: <why>`.
#[allow(clippy::too_many_arguments)]
pub fn use_hints(
    mut asks: MessageReader<UseHint>,
    mut sim: ResMut<crate::sim::SimState>,
    mut script: Option<ResMut<crate::script::ScriptState>>,
    mut used: MessageWriter<crate::ItemUsed>,
    mut sounds: MessageWriter<crate::ui_sounds::UiSound>,
    mut active: ResMut<crate::menu::ActiveMenu>,
    after: Option<Res<crate::after_combat::AfterCombat>>,
) {
    let fighting = after.is_some_and(|a| a.fighting);
    // Laid out in the Pip-Boy's screen texture, at its size whatever the UI scale (#1631).
    let _canvas = crate::interface::original_units();
    use crate::ui_sounds::{UiEvent, UiMenu, UiSound};
    for &UseHint { hint, limb } in asks.read() {
        let Some(item) = read::hint_item(&sim, hint) else {
            warn!("pipboy stats: hint {hint} names no item in the plugins");
            continue;
        };
        let name = sim.name(item).unwrap_or("?").to_string();
        let count = sim.world.item_count(FormId::PLAYER_REF, item).unwrap_or(0);
        // The player's tweak: no healing aid in a fight [design: docs/design/tweaks.md].
        if let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) {
            if crate::tweaks::healing_barred(rt, &mut sim, item, fighting) {
                info!("pipboy stats: {name} not used: healing in combat (tweak)");
                sounds.write(UiSound { menu: UiMenu::Inventory, event: UiEvent::Refused });
                continue;
            }
        }
        let why = if hint == model::RADAWAY {
            // [design] Like the inventory's aid action, carried RadAway can be used
            // regardless of health or limb condition; no item means a dim hint.
            (count <= 0).then(|| "none carried".to_string())
        } else {
            let Some((hp, limbs, hardcore, at_full)) = read::condition(&mut sim, script.as_mut().map(|s| s.bypass_change_detection())) else {
                info!("pipboy stats: {name} not used: the scripts' game is not running");
                continue;
            };
            model::refusal_with(hint, limb, count, hp, &limbs, hardcore, at_full)
        };
        if let Some(why) = why {
            info!("pipboy stats: {name} not used: {why}");
            sounds.write(UiSound { menu: UiMenu::Inventory, event: UiEvent::Refused });
            continue;
        }
        if let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()).filter(|rt| rt.game.has_pre_activate_inventory_handlers()) {
            let allowed = crate::script::with_world(rt, &mut sim, |rt| rt.game.pre_activate_inventory_item(FormId::PLAYER_REF, item, 0, true));
            if !allowed {
                info!("pipboy stats: {name} not used: activation prevented by script");
                if let Some(m) = active.0.as_mut().filter(|m| m.mode == crate::menu::model::Mode::Inventory) {
                    if m.refresh(&mut sim.world).is_ok() {
                        crate::menu::sync_ysi_model(m, &mut rt.game);
                    }
                }
                continue;
            }
        }
        if let Err(e) = sim.world.remove_item(FormId::PLAYER_REF, item, 1) {
            warn!("pipboy stats: {name} not used: {e}");
            continue;
        }
        let on = limb.map_or("on the body".to_string(), |i| format!("on the {}", model::LIMB_NAMES[i]));
        info!("pipboy stats: used {name} {on} ({} left)", count - 1);
        used.write(crate::ItemUsed { item: item.0, limb: limb.map(|i| model::LIMBS[i]) });
        sounds.write(UiSound { menu: UiMenu::Inventory, event: UiEvent::Used { item } });
        // The ITEMS list counts it at once.
        if let Some(m) = active.0.as_mut().filter(|m| m.mode == crate::menu::model::Mode::Inventory) {
            if let Err(e) = m.refresh(&mut sim.world) {
                warn!("pipboy stats: the inventory's lists: {e}");
            } else if let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) {
                crate::menu::sync_ysi_model(m, &mut rt.game);
            }
        }
    }
}

// ---- test aid -------------------------------------------------------------------------

/// `NVF_PLAY_TEST_PIPBOY_STATS=<seconds>:<step>,...`: `sub=<0-5>` (CND, RAD, EFF, H2O, FOD, SLP),
/// `down`, `up` (the list's selection), `mode` (General's lists), `desc=down|up`; and to
/// have something to show, `perk=<FormID hex>` (the player gains a rank of it, as
/// AddPerk) and `fame=<FormID hex>=<points>` (fame with a reputation); on CND
/// `limb=<0-5>` chooses a limb (`model::LIMB_NAMES`), `limb=none` the whole body, and
/// `use=stimpak` or `use=bag` uses the hint's item as its key does; `longdesc` gives every
/// S.P.E.C.I.A.L. row a description long enough to scroll, in words of the aid's own;
/// `scroll=<n>` makes row n the first the tab's list shows, within its range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AidStep {
    Sub(usize),
    Down,
    Up,
    Mode,
    Desc(i32),
    Perk(u32),
    Fame(u32, f64),
    Limb(Option<usize>),
    Use(usize),
    LongDesc,
    Scroll(usize),
}

/// The test aid's `longdesc`: twelve lines of its own words.
fn long_description() -> String {
    (1..=12).map(|i| format!("Test line {i}.")).collect::<Vec<_>>().join("\n")
}

fn parse_aid(v: &str) -> Vec<(f32, AidStep)> {
    let mut out: Vec<(f32, AidStep)> = v
        .split(',')
        .filter_map(|item| {
            let (t, s) = item.split_once(':')?;
            let step = match s.trim() {
                "down" => Some(AidStep::Down),
                "up" => Some(AidStep::Up),
                "mode" => Some(AidStep::Mode),
                "longdesc" => Some(AidStep::LongDesc),
                "desc=down" => Some(AidStep::Desc(1)),
                "desc=up" => Some(AidStep::Desc(-1)),
                "limb=none" => Some(AidStep::Limb(None)),
                "use=stimpak" => Some(AidStep::Use(model::STIMPAK)),
                "use=bag" => Some(AidStep::Use(model::DOCTORS_BAG)),
                s => match s.split_once('=') {
                    Some(("limb", n)) => n.trim().parse().ok().filter(|n| *n < 6).map(|n| AidStep::Limb(Some(n))),
                    Some(("sub", n)) => n.trim().parse().ok().filter(|n| *n < 6).map(AidStep::Sub),
                    Some(("scroll", n)) => n.trim().parse().ok().map(AidStep::Scroll),
                    Some(("perk", f)) => u32::from_str_radix(f.trim().trim_start_matches("0x"), 16).ok().map(AidStep::Perk),
                    Some(("fame", r)) => r.split_once('=').and_then(|(f, n)| Some(AidStep::Fame(u32::from_str_radix(f.trim().trim_start_matches("0x"), 16).ok()?, n.trim().parse().ok()?))),
                    _ => None,
                },
            };
            let parsed = t.trim().parse().ok().zip(step);
            if parsed.is_none() {
                warn!("NVF_PLAY_TEST_PIPBOY_STATS: cannot read {item:?}");
            }
            parsed
        })
        .collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

fn apply_aid(view: &mut StatsView, tab: usize, s: AidStep) {
    match s {
        AidStep::Sub(i) => view.status = i,
        AidStep::Down => view.step(tab, 1),
        AidStep::Up => view.step(tab, -1),
        AidStep::Mode => view.switch(),
        AidStep::Desc(by) => view.scroll_desc(by),
        AidStep::Limb(l) => view.limb = l,
        AidStep::Scroll(n) => view.scroll(tab, n as i32 - view.top[tab] as i32),
        AidStep::Perk(_) | AidStep::Fame(..) | AidStep::Use(_) | AidStep::LongDesc => {}
    }
}

#[cfg(test)]
#[path = "hd_art_tests.rs"]
mod hd_art_tests;

#[cfg(test)]
mod tests {
    use super::*;

    const PAPERDOLL_FILES: [&str; 8] = ["head", "face_00", "torso", "torso_broken", "left_arm", "right_arm", "left_leg", "right_leg"];
    type DrawnPiece = (Val, Val, Val, Val, Rect);
    struct PaperdollDraw {
        pieces: std::collections::BTreeMap<String, DrawnPiece>,
        hits: Vec<Rect>,
        sources: std::collections::BTreeMap<String, (bool, Vec2)>,
    }

    fn draw_paperdoll(files: &[(&str, u32)], atlas: Option<bool>) -> PaperdollDraw {
        use bevy::ecs::system::RunSystemOnce;
        use crate::interface::u;
        use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        #[derive(Resource)]
        struct ObservedHits(Vec<Rect>);
        #[derive(Resource)]
        struct ObservedSources(std::collections::BTreeMap<String, (bool, Vec2)>);
        struct Owned(std::path::PathBuf);
        impl Drop for Owned {
            fn drop(&mut self) {
                if let (Ok(path), Ok(temp)) = (self.0.canonicalize(), std::env::temp_dir().canonicalize()) {
                    if path.parent() == Some(temp.as_path()) && path.file_name().is_some_and(|n| n.to_string_lossy().starts_with("nvf-owned-paperdoll-")) {
                        let _ = std::fs::remove_dir_all(path);
                    }
                }
            }
        }
        let _canvas = crate::interface::original_units();
        let root = std::env::temp_dir().join(format!("nvf-owned-paperdoll-{}-{}", std::process::id(), SERIAL.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&root).unwrap();
        let owned = Owned(root);
        let dir = owned.0.join("textures/interface/stats");
        std::fs::create_dir_all(&dir).unwrap();
        for &(name, size) in files {
            assert!(PAPERDOLL_FILES.contains(&name));
            std::fs::write(dir.join(format!("{name}.dds")), crate::interface::tests::authored_rgba_dds(size, size)).unwrap();
        }
        if atlas == Some(true) {
            std::fs::write(owned.0.join("textures/interface/owned-paperdoll-atlas.dds"), crate::interface::tests::authored_rgba_dds(1024, 1024)).unwrap();
        }
        if atlas.is_some() {
            let index: String = PAPERDOLL_FILES.iter().map(|name| format!("{name}.dds owned-paperdoll-atlas.dds, 0, 2D, 0, 0, 0, 0.125, 0.125\n")).collect();
            std::fs::write(owned.0.join("textures/interface/interfaceshared.tai"), index).unwrap();
        }
        let data = nvf_assets::GameData::new(Arc::new(fnv_vfs::Vfs::open(&owned.0, &[]).unwrap()), Arc::new(fnv_nif::Schema::default()));
        let mut world = World::new();
        world.run_system_once(move |mut commands: Commands| {
            let mut kit = Kit::default();
            let mut images = Assets::<Image>::default();
            let glyphs = crate::pad::PadGlyphs::default();
            let mut c = Ctx::new(&mut kit, Some(&data), &mut images, None, &glyphs);
            let snap = Snapshot { limbs: [1.0; 6], ..default() };
            let mut spots = Vec::new();
            commands.spawn(Node::default()).with_children(|p| condition(p, &mut c, &StatsView::default(), &snap, &mut spots));
            let sources = PAPERDOLL_FILES.iter().filter_map(|name| {
                let (image, rect) = c.picture(&format!("Interface\\Stats\\{name}.dds"))?;
                Some((name.to_string(), (rect.is_some(), c.images.get(&image)?.size().as_vec2())))
            }).collect();
            commands.insert_resource(ObservedHits(spots.into_iter().map(|(r, _)| r).collect()));
            commands.insert_resource(ObservedSources(sources));
        }).unwrap();
        let z = BOY_ZOOM;
        let head_size = HEAD * z;
        let head = Vec2::new(STATUS.min.x + (STATUS.width() - head_size.x) / 2.0, STATUS.min.y + HEAD_Y);
        let torso = head + TORSO.0 * z;
        let face_size = FACE.1 * z;
        let mut positions = vec![("head", head), ("face_00", head + Vec2::new(FACE.0.x, (head_size.y - face_size.y) / 2.0 + FACE.0.y * z)), ("torso", torso)];
        positions.extend(LIMB_PIECES.iter().enumerate().map(|(i, (place, _, name))| (*name, torso + Vec2::new(place.x * z, if i == 0 { place.y } else { place.y * z }))));
        let pieces = positions.into_iter().map(|(name, at)| {
            let value = world.query::<(&Node, &ImageNode)>().iter(&world)
                .find(|(node, _)| node.left == u(at.x) && node.top == u(at.y))
                .map(|(node, image)| (node.left, node.top, node.width, node.height, image.rect.unwrap()))
                .unwrap_or_else(|| panic!("actual condition producer must draw {name} at its original position"));
            (name.to_string(), value)
        }).collect();
        PaperdollDraw { pieces, hits: world.resource::<ObservedHits>().0.clone(), sources: world.resource::<ObservedSources>().0.clone() }
    }

    fn paperdoll_files(default_size: u32, changes: &[(&str, u32)]) -> Vec<(&'static str, u32)> {
        PAPERDOLL_FILES.into_iter().filter_map(|name| {
            let size = changes.iter().find(|(n, _)| *n == name).map_or(default_size, |(_, size)| *size);
            (size != 0).then_some((name, size))
        }).collect()
    }

    #[test]
    fn paperdoll_actual_condition_draw_uses_source_density_without_moving_hit_regions() {
        let render = |scale: u32| {
            let drawn = draw_paperdoll(&paperdoll_files(128 * scale, &[]), None);
            (drawn.pieces["head"], drawn.hits)
        };
        let (native, native_hits) = render(1);
        let (hd, hd_hits) = render(4);
        assert_eq!(native_hits.len(), 6);
        assert_eq!(native_hits, hd_hits);
        assert_eq!((native.0, native.1, native.2, native.3), (hd.0, hd.1, hd.2, hd.3));
        assert!((native.4.width() - HEAD.x).abs() < 0.001);
        assert!((native.4.height() - HEAD.y).abs() < 0.001);
        assert!((hd.4.width() - HEAD.x * 4.0).abs() < 0.001);
        assert!((hd.4.height() - HEAD.y * 4.0).abs() < 0.001);
    }


    #[test]
    fn paperdoll_actual_native_atlas_ignores_shadowed_loose_hd_sources() {
        let native = draw_paperdoll(&paperdoll_files(128, &[]), None);
        let atlas = draw_paperdoll(&paperdoll_files(512, &[]), Some(true));
        assert_eq!(atlas.pieces, native.pieces);
        assert_eq!(atlas.hits, native.hits);
        assert_eq!(atlas.hits.len(), 6);
        assert!(atlas.sources.values().all(|&(is_atlas, size)| is_atlas && size == Vec2::splat(1024.0)));
    }

    #[test]
    fn paperdoll_actual_missing_atlas_uses_its_resolved_own_hd_fallback() {
        let native = draw_paperdoll(&paperdoll_files(128, &[]), None);
        let fallback = draw_paperdoll(&paperdoll_files(512, &[]), Some(false));
        assert_eq!(fallback.hits, native.hits);
        for (name, piece) in &native.pieces {
            let hd = fallback.pieces[name];
            assert_eq!((hd.0, hd.1, hd.2, hd.3), (piece.0, piece.1, piece.2, piece.3));
            assert!((hd.4.min - piece.4.min * 4.0).length() < 0.001);
            assert!((hd.4.max - piece.4.max * 4.0).length() < 0.001);
        }
        assert!(fallback.sources.values().all(|&(is_atlas, size)| !is_atlas && size == Vec2::splat(512.0)));
    }

    #[test]
    fn paperdoll_actual_torso_only_hd_does_not_change_head_or_face_density() {
        let native = draw_paperdoll(&paperdoll_files(128, &[]), None);
        let mixed = draw_paperdoll(&paperdoll_files(128, &[("torso", 512)]), None);
        assert_eq!(mixed.hits, native.hits);
        for name in ["head", "face_00", "left_arm", "right_arm", "left_leg", "right_leg"] {
            assert_eq!(mixed.pieces[name], native.pieces[name]);
        }
        let a = native.pieces["torso"];
        let b = mixed.pieces["torso"];
        assert_eq!((a.0, a.1, a.2, a.3), (b.0, b.1, b.2, b.3));
        assert!((b.4.size() - a.4.size() * 4.0).length() < 0.001);
    }

    #[test]
    fn paperdoll_actual_missing_unused_broken_torso_does_not_disable_head_hd() {
        let native = draw_paperdoll(&paperdoll_files(128, &[]), None);
        let mixed = draw_paperdoll(&paperdoll_files(128, &[("head", 512), ("torso_broken", 0)]), None);
        assert_eq!(mixed.hits, native.hits);
        let a = native.pieces["head"];
        let b = mixed.pieces["head"];
        assert_eq!((a.0, a.1, a.2, a.3), (b.0, b.1, b.2, b.3));
        assert!((b.4.size() - a.4.size() * 4.0).length() < 0.001);
        assert_eq!(mixed.pieces["face_00"], native.pieces["face_00"]);
        assert_eq!(mixed.pieces["torso"], native.pieces["torso"]);
        assert!(!mixed.sources.contains_key("torso_broken"));
    }

    #[test]
    fn the_test_aid_reads_its_steps() {
        let s = parse_aid("9:mode, 3:sub=1,4:down, 5:desc=down, 6:sub=7, x:up, 2:perk=00044CB1, 2.5:fame=104C22=4");
        assert_eq!(
            s,
            vec![(2.0, AidStep::Perk(0x44CB1)), (2.5, AidStep::Fame(0x104C22, 4.0)), (3.0, AidStep::Sub(1)), (4.0, AidStep::Down), (5.0, AidStep::Desc(1)), (9.0, AidStep::Mode)]
        );
        assert_eq!(parse_aid("7:longdesc"), vec![(7.0, AidStep::LongDesc)]);
        assert_eq!(parse_aid("8:scroll=3"), vec![(8.0, AidStep::Scroll(3))]);
        assert_eq!(long_description().lines().count(), 12);
        let s = parse_aid("1:limb=2,2:use=stimpak,3:limb=none,4:use=bag,5:limb=6");
        assert_eq!(s, vec![(1.0, AidStep::Limb(Some(2))), (2.0, AidStep::Use(0)), (3.0, AidStep::Limb(None)), (4.0, AidStep::Use(1))]);
    }

    #[test]
    fn the_stimpak_goes_on_the_limb_pointed_at_or_chosen() {
        let mut v = StatsView::default();
        assert_eq!(hint_pressed(&v, model::STIMPAK, false), Some(UseHint { hint: 0, limb: None }));
        v.choose_limb(4);
        assert_eq!(v.aimed(), Some(4));
        v.hover = Some(Spot::Limb(1));
        assert_eq!(hint_pressed(&v, model::STIMPAK, false), Some(UseHint { hint: 0, limb: Some(1) }));
        // Outside Hardcore the Doctor's Bag heals every limb; in Hardcore it aims at the
        // hovered or chosen limb, or the whole body when none is aimed (#3431).
        assert_eq!(hint_pressed(&v, model::DOCTORS_BAG, false), Some(UseHint { hint: 1, limb: None }));
        assert_eq!(hint_pressed(&v, model::DOCTORS_BAG, true), Some(UseHint { hint: 1, limb: Some(1) }));
        assert_eq!(limb_click_hint(false), model::STIMPAK);
        assert_eq!(limb_click_hint(true), model::DOCTORS_BAG);
        v.hover = None;
        assert_eq!(hint_pressed(&v, model::DOCTORS_BAG, true), Some(UseHint { hint: 1, limb: Some(4) }));
        v.choose_limb(4);
        assert_eq!(v.aimed(), None);
        assert_eq!(hint_pressed(&v, model::DOCTORS_BAG, true), Some(UseHint { hint: 1, limb: None }));
        assert_eq!(hint_pressed(&v, model::RADAWAY, false), Some(UseHint { hint: model::RADAWAY, limb: None }));
    }

    #[test]
    fn each_view_gives_the_description_its_bound_and_the_scroll_stays_within() {
        let mut v = StatsView::default();
        v.set_desc_bound(3);
        v.scroll_desc(9);
        assert_eq!((v.desc, v.desc_bound()), (3, 3));
        v.set_desc_bound(1);
        assert_eq!(v.desc, 1, "a shorter text pulls the scroll back");
        v.scroll_desc(-5);
        assert_eq!(v.desc, 0);
    }

    /// A view's own count of the rows it shows: the hand-built nine by default, what is
    /// scrolled and kept in view following what a tile view says.
    #[test]
    fn each_view_gives_a_list_its_rows_and_the_scroll_stays_within() {
        let mut v = StatsView { lens: [0, 7, 13, 0, 43], ..StatsView::default() };
        assert_eq!(v.list_most(2), 4, "nine at once");
        v.scroll(2, 9);
        assert_eq!(v.top[2], 4);
        v.set_rows_shown(2, 11);
        assert_eq!((v.list_most(2), v.top[2]), (2, 2), "more shown pulls the scroll back");
        v.select(2, 12);
        assert_eq!(v.top[2], 2, "the last row is among the eleven");
        v.set_rows_shown(2, 5);
        assert_eq!(v.top[2], 2, "a new count alone moves no row into view");
        v.select(2, 12);
        assert_eq!(v.top[2], 8);
        v.set_rows_shown(4, 0);
        assert_eq!(v.list_most(4), 42, "at least one row");
        let mut aid = StatsView { lens: [0, 7, 13, 0, 0], ..StatsView::default() };
        apply_aid(&mut aid, 2, AidStep::Scroll(3));
        assert_eq!(aid.top[2], 3);
        apply_aid(&mut aid, 2, AidStep::Scroll(30));
        assert_eq!(aid.top[2], 4, "within its range");
    }

    #[test]
    fn each_view_gives_the_status_page_its_buttons() {
        let mut v = StatsView { status: 2, ..StatsView::default() };
        v.set_status_pages(6);
        assert_eq!(v.status, 2);
        v.status = 5;
        v.set_status_pages(3);
        assert_eq!(v.status, 2, "Hardcore off, or the hand-built page: among those it has");
    }

    #[test]
    fn keys_move_the_selection_within_the_list() {
        let mut v = StatsView { lens: [0, 7, 13, 0, 0], ..StatsView::default() };
        v.step(2, -1);
        assert_eq!(v.selected[2], 0);
        for _ in 0..10 {
            v.step(2, 1);
        }
        assert_eq!((v.selected[2], v.top[2]), (10, 2));
        v.step(2, 5);
        assert_eq!((v.selected[2], v.top[2]), (12, 4));
        v.scroll(2, -9);
        assert_eq!(v.top[2], 0);
        v.step(1, 9);
        assert_eq!((v.selected[1], v.top[1]), (6, 0));
        // An empty list keeps row 0.
        v.step(3, 1);
        assert_eq!(v.selected[3], 0);
        v.switch();
        assert!(!v.reputation);
    }
}
