//! The Pip-Boy's STATS page ([`TILED_TABS`]) drawn from `menus\main\stats_menu.xml`,
//! behind `[UI] TileMenus=StatsMenu` [design: ui-tile-tree.md §6.2 step 3; the contracts:
//! docs/spec-notes/pipboy.md §5.1.1 to §5.1.3; issue #1513].
//!
//! "run07" and "run07b" in this file are the private jobs `run07_stats_tabs` and `run07b_stats`, the
//! original running at 1280 x 720.
//!
//! - **The one decision** ([`StatsTiles::ready`]): the switch on, one of those tabs showing, the
//!   menu files read, the file's tiles there ([`paths`]), the rows made and a usable measured
//!   layout. Painting (`super::draw`), hand-built suppression (`crate::pipboy::frame::draw`) and
//!   input read it and nothing else, and [`settle`] publishes it each frame before any of them.
//!   Eligible to prepare is not ready to display: a pass that fails leaves the whole hand-built
//!   page, says why once, and is tried again the next frame.
//! - **What the model writes** ([`page`]): values from `Snapshot` and `StatsView`; this module
//!   works no figure out again. Places and sizes are the file's, worked out at run time, from
//!   the text sizes and row heights written here.
//! - **Page content only**: the page paints into the Pip-Boy's image, whose background and
//!   scanlines are the shell's, so the file's `background_layout` stays hidden in both frames.
//! - **General** ([`GeneralPaths`]): both lists hold their rows; `stats_genrep_container/user0`
//!   picks the list and its side, by the file, from `StatsView::reputation` [observed: private
//!   job run07_stats_tabs]. It has no display, so that is left empty. The input acts on the rows
//!   of the list shown ([`Written::rows`]).
//! - **Status** ([`StatusPaths`], [`StatusShown`]): the sub-page by
//!   `stats_status_container/user0`, code writing what the file leaves to it: the limbs' meters
//!   and pictures, the hints' lines, the rads and the effects' rows. The bars and "crippled" words
//!   are limb healing targets on CND (Stimpak outside Hardcore [owner 2026-10-23], Doctor's Bag in
//!   Hardcore [#3431]), which the file does not make them, as are the limb's picture, and over the
//!   head the face, which it does while hurt. The aimed limb's bar or "Crippled" word is framed by
//!   a selection box appended to `pieces` after `usable` [design: #3431].
//!   H2O, FOD and SLP are written on RAD's shape [observed: run07b, the same places and scale].
//! - **The tabs not listed** stay fully hand-built. The menu persists across tabs and reopenings, and
//!   its rows while their list is the one shown ([`Written`]); going to another list unloads
//!   them, as a new tree would, and a new menu tile (a rebuilt tree) starts over.
//! - **A list longer than it shows** scrolls by the model's first row shown
//!   (`StatsView::top`), the file moving the rows from the bar's value and `_scroll_delta`
//!   (`super::list`). So every row of such a list must have one height; a wrapped name leaves
//!   the page to the hand-built one [design: as `StatsView`, whose rows are whole rows].
//! - **The order in a frame**: [`write`] after the tile input, before the scripts, so they read
//!   this frame's model and the scroll outlasts a click's pulse; then `crate::pipboy::stats::prepare`
//!   (keys, test aid, what is shown) and [`settle`], which writes what they changed, works the
//!   tree out and decides, before `frame::draw` and `super::draw::draw`. So a tab change or a key
//!   is drawn in the frame it happens, from one state.
//! - **Input** ([`feed`], before the scripts): the pointer is the Pip-Boy's own, in canvas units
//!   (`crate::pipboy::PipBoyScreen::pointer`: the device's ray or the flat image's crop), acting
//!   on the page painted last frame, what the player saw, with the pointer found this frame
//!   (`crate::pipboy::PointerSet`). A row under the pointer selects and boxes as the hand-built
//!   list does, not one drawn under a pointer that has not moved; that is the tile under the
//!   pointer itself, not the highlight, which a file's `xup` or `xdown` can move. Such a move
//!   selects only for a key the hand-built keys did not also act on ([`keyed_row`]). A
//!   click on a row selects it; a tab sends `PipBoyInput::Tab`; the list's and the description's
//!   arrows, page areas and markers become `StatsView` commands. The values are the model's:
//!   written back after input, and the wheel stays `crate::pipboy::stats::prepare`'s, so nothing
//!   acts twice. A row that comes under a still pointer as the list moves takes the selection,
//!   as the original's does [observed: run07]; a row drawn under one when the page comes up does not. The
//!   keys stay the hand-built ones ([`HAND_BUILT_KEYS`]): a file's binding for one does not act
//!   for a physical press, a script's click does. The screen menu's header stays hand-built, and
//!   no tile lies under it to take its click; nor do the file's hover sounds play here, so a
//!   tab's is `TabHovered`, once.
//! - **A menu over the page** takes the frame's input ([`modal_over`]), for all of the frame it
//!   closes in as well: its key closed it in the scripts, before the page's keys run.

use super::draw::{FontKit, Look, Piece};
use super::input::{frame_keys, log_report, Devices};
use super::layout;
use super::list::{self, BarPiece, Counts};
use super::menus::{self, SizesWritten};
use super::vanilla::TileMenus;
use crate::pipboy::stats::model::{Row, RowKey, Snapshot};
use crate::pipboy::stats::{self, Prepared, StatsView};
use crate::pipboy::sounds::TabHovered;
use crate::pipboy::{Frame, Page, PipBoy, PipBoyAid, PipBoyInput, PipBoyScreen, CANVAS};
use crate::script::ScriptState;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use fnv_ui::{TileId, Ui, Value};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

/// The menu's name [documented: stats_menu.xml].
pub const STATS_MENU: &str = "StatsMenu";
/// The tabs drawn from tiles; a tab not here stays hand-built with the switch on.
pub const TILED_TABS: [usize; 5] = [0, 1, 2, 3, 4];
/// Test aid: a tile or template of the file to take as missing, so a smoke scenario sees the
/// fallback of a cut-down file without a changed game file: `<name>`, or `<name>@<seconds>` to
/// stop at that time, for the page's recovery.
pub const CUT_AID: &str = "NVF_PLAY_TEST_STATS_TILE_CUT";

/// The tiles the model writes and the input reads, by name [observed: the vanilla files,
/// docs/spec-notes/pipboy.md §5.1.1]. They are looked for by name under the menu, so a file
/// that nests them otherwise still serves.
const LISTS: [&str; 3] = ["stats_special_container", "stats_skills_container", "stats_perks_container"];
/// General's tab, and the tiles it has of its own.
const GENERAL_TAB: usize = 4;
const GENREP: &str = "stats_genrep_container";
const GENERAL_LIST: &str = "stats_general_container";
const REPUTATION_LIST: &str = "stats_reputation_container";
const KARMA_RECT: &str = "stats_karma_rect";
const KARMA_ALIGNMENT: &str = "stats_karma_alignment";
const KARMA_IMAGE: &str = "stats_karma_image";
const KARMA_TITLE: &str = "stats_karma_title";
const REP_RECT: &str = "stats_reptitle_rect";
const REP_IMAGE: &str = "stats_reputation_image";
const REP_TITLE: &str = "stats_reputation_title";
const REP_NAME: &str = "stats_reputation_rep_title";
const SWITCH: &str = "stats_genrep_button";
const BUTTON_TEXT: &str = "button_text";
/// The setting with the words after General's switch: as Take All's
/// (`super::container_menu::TAKE_ALL_HINT`), R being its key too.
const SWITCH_HINT: &str = "sPCMenuHintR";
/// The setting the file means for the switch with the reputations up. Its one use on this button
/// lacks the closing `;`, so the reader keeps the characters as they stand (menu-xml.md §2.4):
/// code says the word, as with `stats_title` ([`write_model`]) [observed: run07, the label shows].
const SWITCH_GENERAL: &str = "sStatsGeneral";
/// The switch's word with the statistics up: no setting has it [documented: `fnvtool esm inventory`,
/// FalloutNV.esm has no GMST named for reputations; stats_menu.xml has no such word], so it is
/// NVForever's own [design]: the list's name, as `crate::pipboy::stats::general` writes it.
const SWITCH_REPUTATION: &str = "Reputation";
const BADGE: &str = "stats_icon_badge";
/// Status's tab, and the tiles it has of its own [documented: the file's tree, by fnvtool ui tree].
const STATUS_TAB: usize = 0;
const STATUS: &str = "stats_status_container";
/// CND, RAD, EFF, H2O, FOD, SLP, as `stats_status_container/user0` reads them.
const SUB_BUTTONS: [&str; 6] = ["stats_CND_button", "stats_RAD_button", "stats_EFF_button", "stats_H20_button", "stats_FOD_button", "stats_SLP_button"];
/// Stimpak, Doctor's Bag, RadAway, Rad-X, as `crate::pipboy::stats::model::Hints`.
const HINT_BUTTONS: [&str; 4] = ["stats_stimpak_button", "stats_drbag_button", "stats_radaway_button", "stats_radx_button"];
/// The settings of their keys' words, as Take All's.
const HINT_SETTINGS: [&str; 4] = ["sPCMenuHintS", "sPCMenuHintE", "sPCMenuHintA", "sPCMenuHintX"];
const HINT_KEY_LABEL: &str = "PCShortcutLabel";
/// The limbs, as `model::LIMB_NAMES`: `stats_player_<this>`, and each one's picture.
const LIMB_TILES: [&str; 6] = ["head", "torso", "leftarm", "rightarm", "leftleg", "rightleg"];
const LIMB_PICTURES: [&str; 6] = ["head", "torso", "left_arm", "right_arm", "left_leg", "right_leg"];
const FACE: &str = "stats_player_face";
const PLAYER_NAME: &str = "stats_player_name";
const LEVEL_WORD: &str = "sStatsLVLAbbrev";
const RAD_RESIST_LEVEL: &str = "stats_rad_resist_level";
const PLAYER_RADS: &str = "stats_player_rads";
const RADS_MID: &str = "stats_rads_mid";
const RADS_MAX: &str = "stats_rads_max";
/// Each need's figure and the scale's two: the tiles H2O, FOD and SLP name as RAD does its own.
const NEEDS: [[&str; 3]; 3] = [["stats_player_h20", "stats_h20_mid", "stats_h20_max"], ["stats_player_hunger", "stats_hunger_mid", "stats_hunger_max"], ["stats_player_sleep", "stats_sleep_mid", "stats_sleep_max"]];
const EFFECTS: &str = "stats_effects_container";
const EFFECT_TEMPLATE: &str = "stats_effects_template";
const EFFECT_NAME: &str = "stats_effect_name";
const EFFECT_TEXT: &str = "stats_effect_string";
const NO_EFFECTS: &str = "stats_no_effects";
const HEALING: &str = "stats_healing_mode";
const ROW_TEMPLATE: &str = "stats_list_template";
const ROW_TEXT: &str = "ListItemText";
const DISPLAY: &str = "stats_display_rect";
const ICON: &str = "stats_icon";
const DESC_RECT: &str = "stats_description_rect";
const DESC: &str = "stats_description";
const BAR: &str = "stats_description_scrollbar";
const TITLE: &str = "stats_title";
const LINE: &str = "stats_headline_h2";
const BACKGROUND: &str = "background_layout";
/// The tabs, by the numbers `crate::pipboy::PipBoyInput::Tab` uses [observed: their ids 0 to 4].
const TABS: [&str; 5] = ["stats_tailline_status", "stats_tailline_SPECIAL", "stats_tailline_skills", "stats_tailline_perks", "stats_tailline_general"];
/// The rect inside a tab that holds its words and box [observed: by fnvtool ui tree].
const TAB_INNER: &str = "stb_text_box";
/// A row's spacing where the row's `_VerticalSpacing` reads 0 [hypothesis: pipboy.md §5.1.1].
const ROW_SPACING: f64 = 20.0;

fn num(ui: &Ui, t: TileId, name: &str) -> f64 {
    ui.tile(t).get(name).map_or(0.0, Value::num)
}

fn flag(on: bool) -> Value {
    Value::Num(f64::from(u8::from(on)))
}

/// What General has of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeneralPaths {
    pub container: TileId,
    pub general: TileId,
    pub reputation: TileId,
    pub karma_rect: TileId,
    pub karma_alignment: TileId,
    pub karma_image: TileId,
    pub karma_title: TileId,
    pub rep_rect: TileId,
    pub rep_image: TileId,
    pub rep_title: TileId,
    pub rep_name: TileId,
    pub button: TileId,
    /// The button's words.
    pub button_text: Option<TileId>,
    /// Which list these were last used for ([`Paths::in_mode`]).
    pub showing_reputation: bool,
}

/// A limb on CND.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LimbPaths {
    pub picture: TileId,
    pub pct: TileId,
    pub meter: TileId,
    pub crippled: TileId,
}

/// A hint on Status: the button, its words and its key's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HintPaths {
    pub button: TileId,
    pub words: TileId,
    pub key: TileId,
}

/// What Status has of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusPaths {
    pub container: TileId,
    pub subs: [TileId; 6],
    pub hints: [HintPaths; 4],
    pub limbs: [LimbPaths; 6],
    pub face: TileId,
    pub name: TileId,
    pub resist: TileId,
    pub rads: TileId,
    pub rads_mid: TileId,
    pub rads_max: TileId,
    pub needs: [[TileId; 3]; 3],
    pub effects: TileId,
    pub no_effects: TileId,
    /// The limb-switch hint button (`stats_healing_mode`) and its words, in a file that has them
    /// ([`write_status`]).
    pub healing: Option<TileId>,
    pub healing_text: Option<TileId>,
}

/// What a tab has of its own.
// Found once per tab and copied around; boxing the larger variants would lose `Copy`.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabPaths {
    /// S.P.E.C.I.A.L., Skills, Perks: a list beside the display.
    List { list: TileId },
    /// General: two lists, one shown.
    General(GeneralPaths),
    /// Status: its sub-pages, one list among them.
    Status(StatusPaths),
}

/// The tiles found, by name, for one tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paths {
    /// The tab these are for.
    pub page: usize,
    pub tab: TabPaths,
    pub menu: TileId,
    pub icon: TileId,
    /// A weapon skill's mark, in a file that has it.
    pub badge: Option<TileId>,
    pub desc_rect: TileId,
    pub desc: TileId,
    pub bar: TileId,
    pub title: TileId,
    pub line: TileId,
    /// The file's page background, hidden when the file has one ([`write_model`]).
    pub background: Option<TileId>,
    pub tabs: [TileId; 5],
    /// Each tab's inner rect, in a file that has one ([`write_tabs`]).
    pub tab_inners: [Option<TileId>; 5],
}

impl Paths {
    /// The list the tab shows.
    pub fn list(&self) -> TileId {
        match self.tab {
            TabPaths::List { list } => list,
            TabPaths::General(g) if g.showing_reputation => g.reputation,
            TabPaths::General(g) => g.general,
            TabPaths::Status(st) => st.effects,
        }
    }

    /// The template of the list's rows, and the text inside it that says a row's height.
    fn row_parts(&self) -> (&'static str, &'static str) {
        match self.tab {
            TabPaths::Status(_) => (EFFECT_TEMPLATE, EFFECT_NAME),
            _ => (ROW_TEMPLATE, ROW_TEXT),
        }
    }

    /// General's list that is not shown.
    fn other_list(&self) -> Option<TileId> {
        match self.tab {
            TabPaths::General(g) => Some(if g.showing_reputation { g.general } else { g.reputation }),
            _ => None,
        }
    }

    /// These paths for General's list `reputation` says; any other tab's are as they are.
    fn in_mode(mut self, reputation: bool) -> Paths {
        if let TabPaths::General(g) = &mut self.tab {
            g.showing_reputation = reputation;
        }
        self
    }

    /// Whether `t` is a list of this tab.
    fn is_list(&self, t: TileId) -> bool {
        match self.tab {
            TabPaths::List { list } => t == list,
            TabPaths::General(g) => t == g.general || t == g.reputation,
            TabPaths::Status(st) => t == st.effects,
        }
    }

    /// The tab's own tiles.
    fn tab_tiles(&self) -> Vec<TileId> {
        match self.tab {
            TabPaths::List { list } => vec![list],
            TabPaths::General(g) => vec![g.container, g.general, g.reputation, g.karma_rect, g.karma_alignment, g.karma_image, g.karma_title, g.rep_rect, g.rep_image, g.rep_title, g.rep_name, g.button],
            TabPaths::Status(st) => {
                let mut all = vec![st.container, st.face, st.name, st.resist, st.rads, st.rads_mid, st.rads_max, st.effects, st.no_effects];
                all.extend(st.subs);
                all.extend(st.hints.iter().flat_map(|h| [h.button, h.words, h.key]));
                all.extend(st.limbs.iter().flat_map(|l| [l.picture, l.pct, l.meter, l.crippled]));
                all.extend(st.needs.iter().flatten());
                all
            }
        }
    }

    fn live(&self, ui: &Ui) -> bool {
        [self.menu, self.icon, self.desc_rect, self.desc, self.bar, self.title, self.line].into_iter().chain(self.tab_tiles()).chain(self.tabs).chain(self.background).chain(self.badge).chain(self.tab_inners.into_iter().flatten()).all(|t| ui.is_live(t))
    }
}

/// The first tile named `name` under `root` (or `root`), in file order, case not mattering.
pub(super) fn find_named(ui: &Ui, root: TileId, name: &str) -> Option<TileId> {
    let mut stack = vec![root];
    while let Some(t) = stack.pop() {
        let tile = ui.tile(t);
        if !tile.live {
            continue;
        }
        if tile.name.eq_ignore_ascii_case(name) {
            return Some(t);
        }
        stack.extend(tile.children.iter().rev().copied());
    }
    None
}

/// [design] Resolve a tile by its exact vanilla path first (leaving unmodded trees
/// unchanged), falling back segment-by-segment to direct child or live descendant search
/// under each enclosing container when a UI mod wraps a tile inside an extra `<rect>` or
/// container. Each segment stays scoped beneath the previously resolved container so
/// generic child names (such as `button_text` or `CI_TitleText`) never escape into a
/// sibling subtree.
pub(super) fn resolve_tile(ui: &Ui, path: &str) -> Option<TileId> {
    if let Some(exact) = ui.find_tile(path) {
        return Some(exact);
    }
    let mut parts = path.split('/').filter(|s| !s.is_empty());
    let root = parts.next()?;
    let mut cur = ui.menu(root)?;
    for seg in parts {
        cur = list::child(ui, cur, seg).or_else(|| {
            ui.tile(cur)
                .children
                .iter()
                .copied()
                .find_map(|c| find_named(ui, c, seg))
        })?;
    }
    Some(cur)
}

/// Whether the row's template can be found from `list` as the tree looks for it: on that tile
/// or on one above it (`fnv_ui::Ui::add_code_tile_from_template`).
fn has_template(ui: &Ui, list: TileId, template: &str) -> bool {
    let mut at = Some(list);
    while let Some(t) = at {
        if ui.tile(t).templates.iter().any(|tp| tp.name.eq_ignore_ascii_case(template)) {
            return true;
        }
        at = ui.tile(t).parent;
    }
    false
}

/// Every tile the model writes or the input reads on tab `page`, and the row's template; the
/// reason when one is missing (a missing or cut-down file keeps the hand-built page). `cut`:
/// [`CUT_AID`].
pub fn paths(ui: &Ui, page: usize, cut: Option<&str>) -> Result<Paths, String> {
    let menu = ui.menu(STATS_MENU).ok_or_else(|| format!("{STATS_MENU} is not in the tree"))?;
    let mut found: HashMap<String, TileId> = HashMap::new();
    let mut stack = vec![menu];
    while let Some(t) = stack.pop() {
        let tile = ui.tile(t);
        if !tile.live {
            continue;
        }
        found.entry(tile.name.to_ascii_lowercase()).or_insert(t);
        stack.extend(tile.children.iter().rev().copied());
    }
    let cut = cut.map(str::to_ascii_lowercase);
    if let Some(c) = &cut {
        found.remove(c);
    }
    let get = |name: &str| found.get(&name.to_ascii_lowercase()).copied().ok_or_else(|| format!("{name} missing"));
    let tab = if page == STATUS_TAB {
        let mut subs = [menu; 6];
        for (t, name) in subs.iter_mut().zip(SUB_BUTTONS) {
            *t = get(name)?;
        }
        let mut hints = [HintPaths { button: menu, words: menu, key: menu }; 4];
        for (h, name) in hints.iter_mut().zip(HINT_BUTTONS) {
            let button = get(name)?;
            let part = |part: &str| find_named(ui, button, part).ok_or_else(|| format!("{name} has no {part}"));
            *h = HintPaths { button, words: part(BUTTON_TEXT)?, key: part(HINT_KEY_LABEL)? };
        }
        let mut limbs = [LimbPaths { picture: menu, pct: menu, meter: menu, crippled: menu }; 6];
        for (l, name) in limbs.iter_mut().zip(LIMB_TILES) {
            *l = LimbPaths {
                picture: get(&format!("stats_player_{name}"))?,
                pct: get(&format!("stats_player_{name}_pct"))?,
                meter: get(&format!("stats_player_{name}_pct_meter"))?,
                crippled: get(&format!("stats_player_{name}_crippled"))?,
            };
        }
        let mut needs = [[menu; 3]; 3];
        for (tiles, names) in needs.iter_mut().zip(NEEDS) {
            for (t, name) in tiles.iter_mut().zip(names) {
                *t = get(name)?;
            }
        }
        TabPaths::Status(StatusPaths {
            container: get(STATUS)?,
            subs,
            hints,
            limbs,
            face: get(FACE)?,
            name: get(PLAYER_NAME)?,
            resist: get(RAD_RESIST_LEVEL)?,
            rads: get(PLAYER_RADS)?,
            rads_mid: get(RADS_MID)?,
            rads_max: get(RADS_MAX)?,
            needs,
            effects: get(EFFECTS)?,
            no_effects: get(NO_EFFECTS)?,
            healing: get(HEALING).ok(),
            healing_text: get(HEALING).ok().and_then(|b| find_named(ui, b, BUTTON_TEXT)),
        })
    } else if page == GENERAL_TAB {
        TabPaths::General(GeneralPaths {
            container: get(GENREP)?,
            general: get(GENERAL_LIST)?,
            reputation: get(REPUTATION_LIST)?,
            karma_rect: get(KARMA_RECT)?,
            karma_alignment: get(KARMA_ALIGNMENT)?,
            karma_image: get(KARMA_IMAGE)?,
            karma_title: get(KARMA_TITLE)?,
            rep_rect: get(REP_RECT)?,
            rep_image: get(REP_IMAGE)?,
            rep_title: get(REP_TITLE)?,
            rep_name: get(REP_NAME)?,
            button: get(SWITCH)?,
            button_text: get(SWITCH).ok().and_then(|b| find_named(ui, b, BUTTON_TEXT)),
            showing_reputation: true,
        })
    } else {
        let name = page.checked_sub(1).and_then(|i| LISTS.get(i)).ok_or_else(|| format!("tab {page} has no list"))?;
        TabPaths::List { list: get(name)? }
    };
    get(DISPLAY)?;
    let mut tabs = [menu; 5];
    for (t, name) in tabs.iter_mut().zip(TABS) {
        *t = get(name)?;
    }
    let list = match tab {
        TabPaths::List { list } => list,
        TabPaths::General(g) => g.general,
        TabPaths::Status(st) => st.effects,
    };
    // A file without the lists' own template is cut down on every tab, Status's too, so the page
    // does not change hands by tab [design].
    let wanted_templates = if page == STATUS_TAB { &[ROW_TEMPLATE, EFFECT_TEMPLATE][..] } else { &[ROW_TEMPLATE][..] };
    for template in wanted_templates {
        if cut.as_deref() == Some(&template.to_ascii_lowercase()) || !has_template(ui, list, template) {
            return Err(format!("{template} missing"));
        }
    }
    Ok(Paths {
        page,
        tab,
        menu,
        icon: get(ICON)?,
        badge: get(BADGE).ok(),
        desc_rect: get(DESC_RECT)?,
        desc: get(DESC)?,
        bar: get(BAR)?,
        title: get(TITLE)?,
        line: get(LINE)?,
        background: get(BACKGROUND).ok(),
        tabs,
        tab_inners: tabs.map(|t| find_named(ui, t, TAB_INNER)),
    })
}

/// A row made from the template, known by what it stands for: it keeps its place in the list
/// whatever else is taken away or made again. Its tile has the name the original's has on
/// S.P.E.C.I.A.L., the value's AVIF name [observed: private job pipboy_special_a,
/// `GetActiveUIComponentName` under the pointer], and the row's name on the other lists
/// [hypothesis: the same]; scripts read that name, this code never looks a row up by it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MadeRow {
    pub key: RowKey,
    pub tile: TileId,
    text: TileId,
    /// The template gives its height (a number or an expression): it is kept.
    explicit: bool,
}

/// The rows `wanted` under `list`, by key and name, in that order; the ones whole kept. A row
/// unwanted, renamed or missing its text is unloaded whole, so no root is left without its
/// parts. A failure makes none and leaves none of its own: a retry cannot double them.
/// Returns whether anything was unloaded, made or put in another order: what was written of the
/// rows is then out of date, and with it what was measured and checked. No rows wanted is a list
/// that shows none, as the hand-built one: a player with no perks keeps the page [design].
fn ensure_rows(ui: &mut Ui, list: TileId, (template, text_name): (&str, &str), rows: &mut Vec<MadeRow>, wanted: &[(RowKey, &str)]) -> Result<bool, String> {
    let mut changed = false;
    rows.retain(|r| {
        let whole = ui.is_live(r.tile) && ui.is_live(r.text);
        let keep = whole && wanted.iter().any(|&(key, name)| key == r.key && ui.tile(r.tile).name == name);
        if !keep {
            if ui.is_live(r.tile) {
                ui.unload_tile(r.tile);
            }
            changed = true;
        }
        keep
    });
    let in_order = |rows: &mut Vec<MadeRow>| {
        let same = rows.iter().map(|r| r.key).eq(wanted.iter().map(|w| w.0));
        if !same {
            rows.sort_by_key(|r| wanted.iter().position(|w| w.0 == r.key));
        }
        !same
    };
    let missing: Vec<(RowKey, &str)> = wanted.iter().copied().filter(|&(key, _)| !rows.iter().any(|r| r.key == key)).collect();
    if missing.is_empty() {
        return Ok(in_order(rows) || changed);
    }
    let at = ui.path_of(list);
    let mut made: Vec<(RowKey, TileId)> = Vec::with_capacity(missing.len());
    let undo = |ui: &mut Ui, made: &[(RowKey, TileId)]| {
        for &(_, t) in made {
            ui.unload_tile(t);
        }
        ui.update();
    };
    for &(key, name) in &missing {
        // The template's includes were spliced in when the file was read (menu-xml.md §2.2),
        // so no files are needed to make a row.
        match ui.add_code_tile_from_template(&fnv_ui::MemSource::default(), &at, template, Some(name)) {
            Some(t) => made.push((key, t)),
            None => {
                undo(ui, &made);
                return Err(format!("{template} made no row"));
            }
        }
    }
    ui.update();
    let mut new = Vec::with_capacity(made.len());
    for &(key, tile) in &made {
        let Some(text) = find_named(ui, tile, text_name) else {
            undo(ui, &made);
            return Err(format!("{template} has no {text_name}"));
        };
        let row = ui.tile(tile);
        let computed = row.trait_index("height").is_some_and(|k| row.traits[k].expr.is_some());
        new.push(MadeRow { key, tile, text, explicit: computed || num(ui, tile, "height") > 0.0 });
    }
    rows.extend(new);
    in_order(rows);
    Ok(true)
}

fn level_text(snap: &Snapshot) -> String {
    snap.level.map_or("--".into(), |l| l.to_string())
}

/// The rows of a tab's list, without a copy where the model holds them as they are.
fn rows_of(snap: &Snapshot, tab: usize, reputation: bool, status: usize) -> Cow<'_, [Row]> {
    match tab {
        // The EFF list's lines, in name order [observed: run07, at four rows]. `stats::effects`
        // keeps the model's, and on the other sub-pages nothing is made for it [design].
        STATUS_TAB if status == 2 => {
            let mut lines: Vec<&crate::pipboy::stats::model::EffectLine> = snap.effects.iter().collect();
            lines.sort_by(|a, b| a.name.cmp(&b.name));
            Cow::Owned(lines.into_iter().enumerate().map(|(i, e)| Row { key: RowKey::Effect(i as u16), name: e.name.clone(), value: Some(e.text.clone()), ..Row::default() }).collect())
        }
        STATUS_TAB => Cow::Borrowed(&[]),
        1 => Cow::Borrowed(&snap.special),
        2 => Cow::Borrowed(&snap.skills),
        3 => Cow::Borrowed(&snap.perks),
        4 if !reputation => Cow::Borrowed(&snap.general),
        _ => Cow::Owned(snap.rows(tab, reputation)),
    }
}

/// Which of the model's views the page shows now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct At {
    pub tab: usize,
    pub reputation: bool,
    /// The status page's button.
    pub status: usize,
    /// A row is boxed: a key or the pointer has it.
    pub highlight: bool,
    pub selected: usize,
    pub device: bool,
    /// The limb hovered or chosen on Status > CND (`StatsView::aimed`).
    pub aimed: Option<usize>,
}

/// What General shows beside its list, as written last. No word here comes from the hand-built
/// page's own: its "Karma: " before the alignment, its button's names and its empty list's line
/// are not carried over, and a karma title is the setting's alone [design: the string rule,
/// LEGAL.md]. "--" is the figures' without the scripts' game.
#[derive(Clone, Debug, PartialEq)]
pub struct GeneralShown {
    pub karma_word: String,
    pub karma_picture: String,
    pub karma_title: String,
    pub rep_picture: String,
    pub rep_title: String,
    pub rep_name: String,
    pub hint: String,
    pub switch_word: String,
}

impl GeneralShown {
    fn of(snap: &Snapshot, at: At, words: &PageWords) -> GeneralShown {
        // Nothing on this side until a row is pointed at, unlike the other lists' display [observed:
        // run07b]; [hypothesis: and none after the pointer leaves].
        let standing = snap.standings.get(at.selected).filter(|_| at.reputation && at.highlight);
        GeneralShown {
            karma_word: snap.karma.map_or("--".to_string(), |k| k.1.to_string()),
            karma_picture: snap.karma.map_or(String::new(), |k| stats::karma_picture(k.2)),
            karma_title: snap.karma_setting_title.clone().unwrap_or_else(|| if snap.karma.is_some() { String::new() } else { "--".to_string() }),
            rep_picture: standing.and_then(|s| s.icon.clone()).unwrap_or_default(),
            rep_title: standing.map_or(String::new(), |s| s.title.to_string()),
            rep_name: standing.map_or(String::new(), |s| s.name.clone()),
            hint: words.switch_hint.clone(),
            switch_word: if at.reputation { words.switch_general.clone() } else { SWITCH_REPUTATION.to_string() },
        }
    }
}

/// What Status shows besides its rows, as written last. [hypothesis: the root's `user1` holds a
/// need's value on its page as it does the rads on RAD.] A hint's line is a count, the item's name
/// from its record, and the key (`model::hint_text`, [`PageWords`]); the name under the Vault Boy
/// has his level by the setting's word; every other word is the file's. Its "Crippled",
/// "RAD RESIST" and "No Effects" on the hand-built page are not carried over [design: the string
/// rule, LEGAL.md].
#[derive(Clone, Debug, PartialEq)]
pub struct StatusShown {
    pub limbs: [f64; 6],
    pub pictures: [String; 6],
    pub name: String,
    pub hints: [StatusHint; 4],
    pub resist: String,
    /// The value of the meter on the sub-page showing: the rads, or a need.
    pub meter: f64,
    pub rads: String,
    pub needs: [String; 3],
    pub hardcore: bool,
    pub no_effects: bool,
    pub switch_limbs: String,
}

/// A hint's line.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusHint {
    pub words: String,
    pub key: String,
    /// Bright and taking the pointer [design: owner's request 2026-10-27, `model::Hints::lit`, when;
    /// observed: run07b, how: `target`, the file's alpha following it].
    pub lit: bool,
    pub shown: bool,
}

/// The figure beside a meter: none below half a unit [design], as the original's rads read empty
/// at 0 [observed: run07].
fn meter_figure(v: f64) -> String {
    if v >= 0.5 {
        crate::pipboy::stats::model::whole(v)
    } else {
        String::new()
    }
}

impl StatusShown {
    fn of(snap: &Snapshot, at: At, words: &PageWords) -> StatusShown {
        use crate::pipboy::stats::model::whole;
        let line = |i: usize| StatusHint {
            words: snap.hints.texts[i].clone(),
            key: words.hint_keys[i].clone(),
            lit: snap.hints.lit[i],
            // CND's two, RAD's two [observed: run07], none on the others.
            shown: (at.status == 0 && i < 2) || (at.status == 1 && i >= 2),
        };
        StatusShown {
            limbs: snap.limbs,
            pictures: std::array::from_fn(|i| stats::boy_picture(LIMB_PICTURES[i], snap.limbs[i] <= 0.0)),
            name: match snap.level {
                Some(l) => format!("{} - {} {l}", snap.name, words.level_word),
                None => snap.name.clone(),
            },
            hints: std::array::from_fn(line),
            resist: snap.rad_resist.map_or("--".to_string(), |r| format!("{}%", whole(r))),
            meter: at.status.checked_sub(3).and_then(|i| snap.needs.get(i)).copied().unwrap_or(snap.rads),
            rads: meter_figure(snap.rads),
            needs: snap.needs.map(meter_figure),
            hardcore: snap.hardcore,
            no_effects: at.status == 2 && snap.effects.is_empty(),
            switch_limbs: snap.switch_limbs.clone(),
        }
    }
}

/// What the model shows, as written last.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub at: At,
    pub rows: Vec<Row>,
    pub general: Option<GeneralShown>,
    pub status: Option<StatusShown>,
    pub level: String,
    pub hp: String,
    pub ap: String,
    pub xp: String,
}

impl Shown {
    pub fn of(snap: &Snapshot, at: At, words: &PageWords) -> Shown {
        Shown {
            at,
            rows: rows_of(snap, at.tab, at.reputation, at.status).into_owned(),
            general: (at.tab == GENERAL_TAB).then(|| GeneralShown::of(snap, at, words)),
            status: (at.tab == STATUS_TAB).then(|| StatusShown::of(snap, at, words)),
            level: level_text(snap),
            hp: stats::figures(snap.hp),
            ap: stats::figures(snap.ap),
            xp: stats::figures(snap.xp),
        }
    }

    /// Whether this is what `snap` shows, without copying its rows.
    fn same(&self, snap: &Snapshot, at: At, words: &PageWords) -> bool {
        self.at == at
            && self.rows.as_slice() == rows_of(snap, at.tab, at.reputation, at.status).as_ref()
            && (at.tab != STATUS_TAB || self.status.as_ref() == Some(&StatusShown::of(snap, at, words)))
            && (at.tab != GENERAL_TAB || self.general.as_ref() == Some(&GeneralShown::of(snap, at, words)))
            && self.level == level_text(snap)
            && self.hp == stats::figures(snap.hp)
            && self.ap == stats::figures(snap.ap)
            && self.xp == stats::figures(snap.xp)
    }
}

/// What a row says. A perk's rank is in its own words, after its name, from 2 [observed: private
/// job run07b_stats, `(2)` on three perks, no number tile]; [hypothesis: none at a rank of 1 of more].
fn row_string(row: &Row) -> String {
    match (row.key, row.value.as_deref()) {
        (RowKey::Perk(_), Some(rank)) if rank != "1" => format!("{} ({rank})", row.name),
        _ => row.name.clone(),
    }
}

/// A list's rows, as §5.1.1 has them.
fn write_rows(ui: &mut Ui, rows: &[MadeRow], data: &[Row]) {
    for (i, (made, row)) in rows.iter().zip(data).enumerate() {
        ui.write_changed(made.tile, "listindex", Value::Num(i as f64));
        // [observed: private job pipboy_special_a]
        ui.write_changed(made.tile, "id", Value::Num(-1.0));
        ui.write_changed(made.tile, "string", Value::Str(row_string(row)));
        // A text value keeps the "--" of a game without scripts (`Value::text`); -1 hides the
        // number [observed: the template's own value; run07b, on every Perks row].
        let number = row.value.clone().filter(|_| !matches!(row.key, RowKey::Perk(_)));
        ui.write_changed(made.tile, "user1", number.map_or(Value::Num(-1.0), Value::Str));
        ui.write_changed(made.tile, "user0", flag(row.mark.is_some()));
        ui.write_changed(made.tile, "user2", Value::Str(row.mark.clone().unwrap_or_default()));
    }
}

/// The EFF list's rows: write `user0` (effect name) and `user1` (effect text) on the row tile
/// for modded templates that bind child text strings via `<copy src="parent" trait="user0"/>` /
/// `user1` (e.g. Vanilla UI Plus `stats_menu.xml`), and write literal child `string` traits when
/// no expression drives them (vanilla `stats_menu.xml`) [design].
fn write_effect_rows(ui: &mut Ui, rows: &[MadeRow], data: &[Row]) {
    let has_string_expr = |ui: &Ui, t: TileId| {
        let tile = ui.tile(t);
        tile.trait_index("string").is_some_and(|k| tile.traits[k].expr.is_some())
    };
    for (i, (made, row)) in rows.iter().zip(data).enumerate() {
        let value = row.value.clone().unwrap_or_default();
        ui.write_changed(made.tile, "listindex", Value::Num(i as f64));
        ui.write_changed(made.tile, "user0", Value::Str(row.name.clone()));
        ui.write_changed(made.tile, "user1", Value::Str(value.clone()));
        // A line under each row but the last: the file ties it to this [documented: the file;
        // observed: run07b, 3 rows, 2 lines].
        ui.write_changed(made.tile, "user2", flag(i + 1 < rows.len()));
        if !has_string_expr(ui, made.text) {
            ui.write_changed(made.text, "string", Value::Str(row.name.clone()));
        }
        if let Some(text) = find_named(ui, made.tile, EFFECT_TEXT) {
            if !has_string_expr(ui, text) {
                ui.write_changed(text, "string", Value::Str(value));
            }
        }
    }
}

/// Write what the page shows ([`Shown`]), each value only when it differs from what this code
/// wrote last, so what a script set meanwhile stands until the model changes (`Ui::write_changed`).
pub fn write_model(ui: &mut Ui, p: &Paths, rows: &[MadeRow], s: &Shown) {
    ui.write_changed(p.menu, "user0", Value::Num(s.at.tab as f64));
    if let Some(background) = p.background {
        ui.write_changed(background, "visible", flag(false));
    }
    for (name, v) in [("user8", &s.level), ("user5", &s.hp), ("user6", &s.ap), ("user7", &s.xp)] {
        ui.write_changed(p.menu, name, Value::Str(v.clone()));
    }
    // Only these lists' rows have marks, and only these lists have the trait [documented: the
    // file's tree, by fnvtool ui tree].
    if matches!(s.at.tab, 1 | 2) {
        ui.write_changed(p.list(), "user0", flag(s.rows.iter().any(|r| r.mark.is_some())));
    }
    if let (TabPaths::General(g), Some(side)) = (&p.tab, &s.general) {
        write_general(ui, g, side, s.at.reputation);
    }
    if let (TabPaths::Status(st), Some(side)) = (&p.tab, &s.status) {
        write_status(ui, p.menu, st, side, s.at.status);
    }
    if s.at.tab == STATUS_TAB {
        write_effect_rows(ui, rows, &s.rows);
    } else {
        write_rows(ui, rows, &s.rows);
    }
    // A row without a picture keeps `visible` with an empty `filename`: the line and the text
    // follow it in the file, and the hand-built page draws them without a picture.
    // General has no display: left empty, so nothing of another tab's row stands in it.
    let selected = s.rows.get(s.at.selected).filter(|_| s.at.tab != GENERAL_TAB && s.at.tab != STATUS_TAB);
    ui.write_changed(p.icon, "filename", Value::Str(selected.and_then(|r| r.icon.clone()).unwrap_or_default()));
    ui.write_changed(p.icon, "visible", flag(selected.is_some()));
    // Only `visible` counts: the original's name goes stale on a row with none [observed: run07].
    if let Some(badge) = p.badge {
        let mark = selected.and_then(|r| r.badge.clone());
        ui.write_changed(badge, "visible", flag(mark.is_some()));
        if let Some(file) = mark {
            ui.write_changed(badge, "filename", Value::Str(file));
        }
    }
    ui.write_changed(p.desc, "string", Value::Str(selected.map(|r| r.description.clone()).unwrap_or_default()));
    // Only a name the reader left unresolved: a replacement's or a script's words stand.
    if ui.tile(p.title).get("string").is_some_and(|v| v.text().trim_start().starts_with('&')) {
        ui.set_trait(p.title, "string", Value::Str(Page::Stats.title().to_string()));
    }
}

/// General's side and switch. The mode's trait is the list's parent's `user0`, 1 the reputations
/// [observed: run07]: the file shows each list and side from it. Each picture and word is shown
/// as it has something to show [observed: run07, the reputation's picture hidden with none]; the
/// switch writes a word, never its key alone, and the key is the setting's or the pad's
/// ([`SWITCH_HINT`]).
fn write_general(ui: &mut Ui, g: &GeneralPaths, s: &GeneralShown, reputation: bool) {
    ui.write_changed(g.container, "user0", flag(reputation));
    for (tile, text) in [(g.karma_alignment, &s.karma_word), (g.karma_title, &s.karma_title), (g.rep_title, &s.rep_title), (g.rep_name, &s.rep_name)] {
        ui.write_changed(tile, "string", Value::Str(text.clone()));
        ui.write_changed(tile, "visible", flag(!text.is_empty()));
    }
    for (tile, file) in [(g.karma_image, &s.karma_picture), (g.rep_image, &s.rep_picture)] {
        ui.write_changed(tile, "filename", Value::Str(file.clone()));
        ui.write_changed(tile, "visible", flag(!file.is_empty()));
    }
    ui.write_changed(g.button, "_PCButtonText", Value::Str(s.hint.clone()));
    if let Some(words) = g.button_text {
        ui.write_changed(words, "string", Value::Str(s.switch_word.clone()));
    }
}

/// Status: the sub-page, the root's meter, CND's limbs and hints, RAD's figures, the needs' on the
/// same shape, and the three buttons Hardcore shows, whose `visible` the file leaves to code
/// [observed: run07, run07b]. pipboy.md §5.1.3.
fn write_status(ui: &mut Ui, menu: TileId, st: &StatusPaths, s: &StatusShown, sub: usize) {
    use crate::pipboy::stats::model::whole;
    ui.write_changed(st.container, "user0", Value::Num(sub as f64));
    ui.write_changed(menu, "user1", Value::Num(s.meter));
    ui.write_changed(menu, "user2", Value::Num(stats::RAD_MAX));
    for &button in &st.subs[3..] {
        ui.write_changed(button, "visible", flag(s.hardcore));
    }
    for (i, l) in st.limbs.iter().enumerate() {
        ui.write_changed(l.meter, "_Value", Value::Num(s.limbs[i]));
        ui.write_changed(l.picture, "filename", Value::Str(s.pictures[i].clone()));
        // The bar, and the word in its place at 0, take the Stimpak [owner 2026-10-23].
        ui.write_changed(l.pct, "target", flag(true));
        ui.write_changed(l.crippled, "target", flag(true));
    }
    ui.write_changed(st.face, "filename", Value::Str(stats::BOY_FACE.to_string()));
    ui.write_changed(st.name, "string", Value::Str(s.name.clone()));
    for (h, line) in st.hints.iter().zip(&s.hints) {
        ui.write_changed(h.button, "visible", flag(line.shown));
        ui.write_changed(h.words, "string", Value::Str(line.words.clone()));
        ui.write_changed(h.button, "_PCButtonText", Value::Str(line.key.clone()));
        // The file's alpha follows it; a dim hint lets the pointer through [observed: run07b].
        ui.write_changed(h.button, "target", flag(line.lit));
    }
    ui.write_changed(st.resist, "string", Value::Str(s.resist.clone()));
    ui.write_changed(st.rads, "string", Value::Str(s.rads.clone()));
    ui.write_changed(st.rads_mid, "string", Value::Str(whole(stats::RAD_MAX / 2.0)));
    ui.write_changed(st.rads_max, "string", Value::Str(whole(stats::RAD_MAX)));
    for (tiles, figure) in st.needs.iter().zip(&s.needs) {
        ui.write_changed(tiles[0], "string", Value::Str(figure.clone()));
        ui.write_changed(tiles[1], "string", Value::Str(whole(stats::RAD_MAX / 2.0)));
        ui.write_changed(tiles[2], "string", Value::Str(whole(stats::RAD_MAX)));
    }
    ui.write_changed(st.no_effects, "visible", flag(s.no_effects));
    // The controller's hint between the Stimpak's and the bag's keeps its place while hidden
    // [observed: run07, the bag's y 114, two boxes of 52 down]: with no words it would measure
    // nothing, and the bag would stand under the wrong point. When `sSwitchLimbs` is present,
    // writing it also gives the controller-visible tile its label (#3431).
    if let Some(words) = st.healing_text {
        if !s.switch_limbs.is_empty() {
            ui.write_changed(words, "string", Value::Str(s.switch_limbs.clone()));
        } else if ui.tile(words).get("string").is_none_or(|v| v.text().is_empty()) {
            ui.write_changed(words, "string", Value::Str(" ".to_string()));
        }
    }
}

/// What the screen menu changed of the file, to give back on the device.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct FlatChanges {
    title: Option<Value>,
    line: Option<(f64, f64)>,
}

/// The screen menu's layout, the maintainer's [design: the owner, 2026-10-31, `frame::heading_title`,
/// docs/design/screen-menu.md §2]: no title, the heading's line running on from the lead line.
/// On the device what it changed goes back. Called with the texts measured, as the
/// file may place the line by the title's width: what the file works out again for the line is
/// put right here, on every pass.
fn write_frame(ui: &mut Ui, p: &Paths, device: bool, flat: &mut Option<FlatChanges>) {
    write_frame_of(ui, p.title, p.line, device, flat);
}

/// [`write_frame`] by the page's title and heading line.
pub(super) fn write_frame_of(ui: &mut Ui, title_tile: TileId, line_tile: TileId, device: bool, flat: &mut Option<FlatChanges>) {
    if device {
        if let Some(was) = flat.take() {
            ui.set_trait(title_tile, "visible", was.title.unwrap_or(flag(true)));
            if let Some((x, len)) = was.line {
                ui.set_trait(line_tile, "x", Value::Num(x));
                ui.set_trait(line_tile, "_length", Value::Num(len));
            }
        }
        return;
    }
    if flat.is_none() {
        let title = ui.tile(title_tile).get("visible").cloned();
        let line = ui.tile(line_tile).get("_length").map(Value::num).map(|len| (num(ui, line_tile, "x"), len));
        ui.set_trait(title_tile, "visible", flag(false));
        *flat = Some(FlatChanges { title, line });
    }
    if let Some((x, len)) = flat.as_ref().and_then(|f| f.line) {
        let from = f64::from(crate::pipboy::frame::lead_line_end());
        if num(ui, line_tile, "x") != from {
            ui.set_trait(line_tile, "x", Value::Num(from));
            ui.set_trait(line_tile, "_length", Value::Num(x + len - from));
        }
    }
}

/// What this slice's layout needs of a tab, as the tree reads it [observed: by fnvtool ui
/// tree]: the tab has no `locus`, its inner rect has; the tab's place is read from the inner
/// rect's, so the two repeat each other, and writing either collapses the other; and the
/// inner rect has `alpha` 0 over shown words and lines. The original stands the tab at its one
/// place [observed: private job pipboy_special_a] and shows the words, so it does not add the
/// two places [hypothesis] nor pass the rect's `alpha` down [hypothesis]. `super::layout` and the
/// tile input read no `locus` and multiply a rect's `alpha` for every menu, so here alone,
/// once, marks say so: the tab gives its children no place ([`fnv_ui::input::NO_ORIGIN`]), and
/// what is under the rect keeps its own `alpha` ([`super::layout::OWN_ALPHA`]). The file's
/// places are not touched [design].
fn write_tabs(ui: &mut Ui, p: &Paths, marked: &mut bool) {
    if std::mem::replace(marked, true) {
        return;
    }
    mark_inner_rects(ui, p.tabs.into_iter().zip(p.tab_inners));
}

/// The marks of [`write_tabs`] on buttons and their inner rects. The status page's six are the
/// same prefab [documented: the file's tree]: unmarked they stand twice as far down, over each other.
fn mark_inner_rects(ui: &mut Ui, buttons: impl IntoIterator<Item = (TileId, Option<TileId>)>) {
    for (tab, inner) in buttons {
        let Some(inner) = inner else { continue };
        ui.set_trait(tab, fnv_ui::input::NO_ORIGIN, Value::Num(1.0));
        let mut stack = ui.tile(inner).children.clone();
        while let Some(t) = stack.pop() {
            stack.extend(ui.tile(t).children.iter().copied());
            ui.set_trait(t, layout::OWN_ALPHA, Value::Num(1.0));
        }
    }
}

/// The list bar's pictures [design: `super::list` names; `super::layout::OWN_ALPHA`
/// lets each keep its own alpha under the transparent bar, as in `super::dialog_menu::own_tiles`].
fn list_bar_pictures(ui: &Ui, list: TileId) -> [Option<TileId>; 3] {
    let bar = list::child(ui, list, list::SCROLLBAR);
    [list::UP, list::DOWN, list::MARKER].map(|name| bar.and_then(|b| list::child(ui, b, name)))
}

/// The places code gives the file to work from: each row's height and `_y`, the list's height,
/// the box's place, the description's scroll steps, and, for a list that does not hold its rows,
/// the scroll bar's counts.
#[derive(Clone, Debug, PartialEq)]
pub struct Geometry {
    pub rows: Vec<(f64, f64)>,
    pub list: f64,
    pub highlight: (f64, f64),
    pub desc_bound: usize,
    /// Rows shown at once, and the first of them (or scroll step for variable-height lists).
    pub visible: usize,
    pub top: usize,
    /// Rows wholly inside the list window at this scroll position.
    pub shown: std::ops::Range<usize>,
    pub counts: Option<Counts>,
}

/// How many whole rows of `tallest` a list shows and how high it is: `_number_of_visible_items`
/// rows when the file gives them, else what fits the height the file gives (General's, read as
/// 542 and ten whole rows [observed: run07]), else the hand-built count.
fn list_window(ui: &Ui, list: TileId, tallest: f64) -> (usize, f64) {
    let count = num(ui, list, "_number_of_visible_items").floor();
    if count >= 1.0 {
        return (count as usize, count * tallest);
    }
    let own = num(ui, list, "height");
    match (own > 0.0, tallest > 0.0) {
        (true, true) => (((own / tallest).floor() as usize).max(1), own),
        (true, false) => (stats::VISIBLE, own),
        (false, _) => (stats::VISIBLE, stats::VISIBLE as f64 * tallest),
    }
}

/// [`Geometry`] of `list` from the sizes measured, shown from `top`; the reason when no usable
/// layout comes of them. A list that scrolls moves by whole rows of one height, each value by
/// `list::counts`' step (the row's height on most lists, a little less on General's [observed:
/// run07]): rows of another height leave it to the hand-built page.
pub fn geometry(ui: &Ui, p: &Paths, list: TileId, rows: &[MadeRow], selected: usize, highlight: bool, top: usize) -> Result<Geometry, String> {
    let mut places = Vec::with_capacity(rows.len());
    let mut y = 0.0;
    let mut tallest: f64 = 0.0;
    for (i, row) in rows.iter().enumerate() {
        let h = if row.explicit {
            num(ui, row.tile, "height")
        } else {
            let spacing = num(ui, row.tile, "_VerticalSpacing");
            num(ui, row.text, "height") + if spacing > 0.0 { spacing } else { ROW_SPACING }
        };
        if h <= 0.0 {
            return Err(format!("row {i} has no height"));
        }
        places.push((h, y));
        y += h;
        tallest = tallest.max(h);
    }
    let base_row = places.iter().map(|r| r.0).fold(f64::INFINITY, f64::min);
    let base_row = if base_row.is_finite() { base_row } else { tallest };
    // The EFF list is as high as its rows, for the file to centre [documented: the file; hypothesis:
    // run07's bar heights], up to what its area holds; the rest is cut, as on the hand-built page.
    let (visible, height) = match p.tab {
        TabPaths::Status(st) => {
            let area = num(ui, st.container, "height");
            let most = if tallest > 0.0 && area > 0.0 { ((area / tallest).floor() as usize).max(1) } else { rows.len().max(1) };
            (most, rows.len().min(most) as f64 * tallest)
        }
        _ => list_window(ui, list, base_row),
    };
    let scrolls = (rows.len() > visible || y > height + 0.5) && !matches!(p.tab, TabPaths::Status(_));
    let counts = scrolls
        .then(|| list::counts(&places.iter().map(|r| r.0).collect::<Vec<f64>>(), height))
        .filter(|c| c.delta.is_some());
    let uniform = places.iter().all(|r| tallest - r.0 <= 0.5);
    let most = counts.map_or(0, |c| c.most().round() as usize);
    let top = if counts.is_some() { top.min(most) } else { top.min(rows.len().saturating_sub(visible)) };
    let delta = counts.and_then(|c| c.delta).unwrap_or(0.0);
    let scroll_px = top as f64 * delta;
    let shown = if counts.is_none() {
        0..rows.len().min(visible)
    } else if uniform {
        top..(top + visible).min(rows.len())
    } else {
        let first = places.iter().position(|&(h, ry)| ry >= scroll_px - 1.0 && ry + h <= scroll_px + height + 1.0);
        let last = places.iter().rposition(|&(h, ry)| ry >= scroll_px - 1.0 && ry + h <= scroll_px + height + 1.0);
        match (first, last) {
            (Some(a), Some(b)) => a..b + 1,
            _ => 0..0,
        }
    };
    let desc_bound = if p.page == GENERAL_TAB || p.page == STATUS_TAB {
        0
    } else {
        let window = num(ui, p.desc_rect, "height");
        if window <= 0.0 {
            return Err(format!("{DESC_RECT} has no height"));
        }
        ((num(ui, p.desc, "height") - window) / f64::from(stats::DESC_STEP)).ceil().max(0.0) as usize
    };
    // The box where its row stands after the scroll; none on a row scrolled out of sight, as the
    // hand-built list has none (`crate::pipboy::stats::list`).
    let box_at = places
        .get(selected)
        .filter(|&&(h, ry)| {
            highlight
                && if counts.is_none() || uniform {
                    (top..top + visible).contains(&selected)
                } else {
                    ry + h > scroll_px + 0.5 && ry < scroll_px + height - 0.5
                }
        })
        .map_or((-1.0, 0.0), |&(h, ry)| (ry - scroll_px, h));
    Ok(Geometry { rows: places, list: height, highlight: box_at, desc_bound, visible, top, shown, counts })
}

fn write_geometry(ui: &mut Ui, p: &Paths, list: TileId, rows: &[MadeRow], g: &Geometry, counted: &mut Vec<(TileId, f64)>) {
    for (row, &(h, y)) in rows.iter().zip(&g.rows) {
        if !row.explicit {
            ui.write_changed(row.tile, "height", Value::Num(h));
        }
        ui.write_changed(row.tile, "_y", Value::Num(y));
    }
    // A height the file gives stands.
    if num(ui, list, "height") != g.list {
        ui.write_changed(list, "height", Value::Num(g.list));
    }
    ui.write_changed(list, "_highlight_y", Value::Num(g.highlight.0));
    ui.write_changed(list, "_selected_height", Value::Num(g.highlight.1));
    // A list that fits and was never counted keeps the file's counts [hypothesis: pipboy.md
    // §5.1.1]; one that has stopped scrolling gets them back as `super::list` writes them.
    let was = counted.iter().position(|c| c.0 == list);
    match (g.counts, was) {
        (Some(c), _) => {
            let file = was.map_or_else(|| num(ui, list, "_scroll_delta"), |i| counted[i].1);
            if was.is_none() {
                counted.push((list, file));
            }
            list::write_counts(ui, list, &c, file);
        }
        (None, Some(i)) => {
            let (_, file) = counted.remove(i);
            list::write_counts(ui, list, &list::counts(&[], 0.0), file);
            list::write_value(ui, list, 0.0);
        }
        (None, None) => {}
    }
    // Hidden at 1 [design: pipboy.md §5.1.1]; the +1 is our adaptation [design].
    ui.write_changed(p.bar, "_number_of_items", Value::Num(g.desc_bound as f64 + 1.0));
}

/// The model's scroll over the file's own count of the clicks, as the prefab asks of code that
/// sets the value [observed: pipboy.md §5.1.1; design: as `super::tutorial_menu::write_scroll`].
pub fn write_scroll(ui: &mut Ui, bar: TileId, steps: usize) {
    ui.set_trait(bar, "_current_value", Value::Num(steps as f64));
    ui.set_trait(bar, "_SetInCode", Value::Num(1.0));
}

/// Whether a rectangle has a size and lies wholly on the canvas.
fn wholly_on_canvas(r: &layout::Rect) -> bool {
    r.width > 0.0 && r.height > 0.0 && r.left >= 0.0 && r.top >= 0.0 && r.left + r.width <= f64::from(CANVAS.x) && r.top + r.height <= f64::from(CANVAS.y)
}

/// Whether a painted piece shows whole: on the canvas, and inside what clips it (a unit's slack
/// for rounding). Its middle, when it does.
pub(super) fn shows_whole(piece: &Piece) -> Option<(f64, f64)> {
    let r = piece.rect;
    let in_clip = piece.clip.is_none_or(|c| r.left >= c.left - 1.0 && r.top >= c.top - 1.0 && r.left + r.width <= c.left + c.width + 1.0 && r.top + r.height <= c.top + c.height + 1.0);
    (wholly_on_canvas(&r) && in_clip).then_some((r.left + r.width / 2.0, r.top + r.height / 2.0))
}

/// Whether a list row's text fits inside its clip window: accepts [`shows_whole`], or when the
/// row tile itself sits wholly inside the list clip window and the text stays on-canvas with its
/// horizontal bounds, top, and upper half inside the clip (accommodating modded font-centering
/// offsets such as VUI+ `_VUI+FontY_2` whose bottom leading margin slightly overhangs tightly
/// packed rows like 13-row Skills) [design].
fn row_fits(ui: &Ui, row_tile: TileId, piece: &Piece) -> Option<(f64, f64)> {
    if let Some(mid) = shows_whole(piece) {
        return Some(mid);
    }
    let r = piece.rect;
    if !wholly_on_canvas(&r) {
        return None;
    }
    let c = piece.clip?;
    if r.left < c.left - 1.0 || r.left + r.width > c.left + c.width + 1.0 || r.top < c.top - 1.0 {
        return None;
    }
    let row_box = layout::tile_box(ui, row_tile, &|_, _, _| (0.0, 0.0), true)?;
    let rb = row_box.rect;
    if rb.top < c.top - 1.0 || rb.top + rb.height > c.top + c.height + 1.0 {
        return None;
    }
    let clip_bottom = c.top + c.height;
    if clip_bottom - r.top < r.height * 0.5 {
        return None;
    }
    // Probe inside the row tile's own clickable box (clamped inside `c`), since the text's
    // vertical offset may push the text box's midpoint near or past `rb`'s bottom edge [design].
    let mid_y = (rb.top + rb.height / 2.0).clamp(c.top + 1.0, clip_bottom - 1.0);
    Some((r.left + r.width / 2.0, mid_y))
}

/// The window the description is read through: what clips it, else its parent's place and
/// height at its own width (the scroll moves the text itself, not the window).
fn description_window(ui: &Ui, p: &Paths, desc: &Piece) -> layout::Rect {
    desc.clip.unwrap_or(layout::Rect { left: desc.rect.left, top: desc.rect.top - num(ui, p.desc, "y"), width: desc.rect.width, height: num(ui, p.desc_rect, "height") })
}

/// Whether what is about to be painted, `pieces`, can be used, on every final pass, so what a
/// script did since is seen: the words of every row in sight (`shown`: those the list scrolled
/// to, the others being outside its window by design) and of every tab among the pieces, whole
/// on the canvas and inside their clips, the description's window on the canvas if it has words,
/// and those rows and the tabs but the one showing taking the pointer at their middles, whatever
/// else came to lie over them; the tab showing need not, as a click on it does nothing. The
/// description's area, for the wheel, or the reason for the hand-built page.
pub fn usable(ui: &Ui, p: &Paths, rows: &[MadeRow], shown: std::ops::Range<usize>, pieces: &[Piece]) -> Result<Option<Rect>, String> {
    let is_text = |x: &&Piece| matches!(x.look, Look::Text { .. });
    let mut wanted: Vec<(StatsTile, (f64, f64))> = Vec::with_capacity(rows.len() + p.tabs.len());
    for (i, row) in rows.iter().enumerate().filter(|(i, _)| shown.contains(i)) {
        let piece = pieces.iter().filter(is_text).find(|x| x.tile == row.text).ok_or_else(|| format!("row {i} is not shown"))?;
        let middle = row_fits(ui, row.tile, piece).ok_or_else(|| format!("row {i} does not fit"))?;
        // The EFF list's rows are only read.
        if !matches!(p.tab, TabPaths::Status(_)) {
            wanted.push((StatsTile::Row(i), middle));
        }
    }
    for (i, &tab) in p.tabs.iter().enumerate() {
        // A tab is a hotrect, which draws nothing of its own: its words, as they are painted.
        let piece = pieces.iter().filter(is_text).find(|x| inside(ui, x.tile, tab)).ok_or_else(|| format!("tab {i} is not shown"))?;
        let middle = shows_whole(piece).ok_or_else(|| format!("tab {i} does not fit"))?;
        if i != p.page {
            wanted.push((StatsTile::Tab(i), middle));
        }
    }
    let says = ui.tile(p.desc).get("string").is_some_and(|v| !v.text().is_empty());
    let desc = pieces.iter().filter(is_text).find(|x| x.tile == p.desc);
    if says && desc.is_none() {
        return Err("the description is not shown".into());
    }
    let area = desc.map(|d| description_window(ui, p, d));
    if area.is_some_and(|a| !wholly_on_canvas(&a)) {
        return Err("the description does not fit".into());
    }
    // One walk of the tree serves them all.
    let candidates = fnv_ui::input::hit_candidates(ui, p.menu, &fnv_ui::input::no_measure);
    for (what, at) in wanted {
        let hit = fnv_ui::input::top_hit(ui, &candidates, at);
        if hit.and_then(|h| stats_tile(ui, p, rows, h.tile)) != Some(what) {
            return Err(format!("{what:?} does not take the pointer"));
        }
    }
    Ok(area.map(|a| Rect::new(a.left as f32, a.top as f32, (a.left + a.width) as f32, (a.top + a.height) as f32)))
}

/// Whether `t` is `of` or under it.
pub(super) fn inside(ui: &Ui, t: TileId, of: TileId) -> bool {
    let mut at = Some(t);
    while let Some(x) = at {
        if x == of {
            return true;
        }
        at = ui.tile(x).parent;
    }
    false
}

/// What [`page`] keeps: the rows, what was last written, and the reasons warned of.
#[derive(Default)]
pub(crate) struct Written {
    menu: Option<TileId>,
    paths: Option<Paths>,
    /// The list the rows are under: the one shown.
    rows_in: Option<TileId>,
    rows: Vec<MadeRow>,
    /// General's other list, its rows and what was written of them.
    other_rows: Vec<MadeRow>,
    other_shown: Option<Vec<Row>>,
    other_geometry: Option<Geometry>,
    /// The switch's words as the file has them, read once a menu.
    file_switch_word: Option<String>,
    /// The file and zoom each picture's size was written for ([`write_file_sizes`]).
    file_sizes: HashMap<TileId, (String, f64)>,
    /// Each file's own size in texels, zoom left out, as found ([`write_file_sizes`]).
    file_texels: HashMap<String, Option<(f64, f64)>>,
    /// What is under the status page's buttons' inner rects is marked ([`write_tabs`]).
    subs_marked: bool,
    /// Each list code has counted for its scroll bar, and the `_scroll_delta` the file gave it.
    counted: Vec<(TileId, f64)>,
    shown: Option<Shown>,
    geometry: Option<Geometry>,
    sizes: SizesWritten,
    flat: Option<FlatChanges>,
    /// What is under the tabs' inner rects is marked ([`write_tabs`]).
    tabs_marked: bool,
    /// What is shown changed since the log last said so.
    tell: bool,
    /// The description's scroll, the text's place and the bound last logged.
    drawn: Option<(f64, f64, usize)>,
    /// The list's scroll last logged.
    list_drawn: Option<f64>,
    /// Last `(tab, selected)` seen by [`page`], to keep variable-height selections in view.
    last_selected: Option<(usize, usize)>,
    fallbacks: HashSet<String>,
}

/// The decision every reader shares, and what the page keeps.
#[derive(Resource, Default)]
pub struct StatsTiles {
    /// The menu, while the page is ready to display this frame ([`settle`]).
    pub ready: Option<TileId>,
    /// The menu painted by the last drawing (`super::draw`): what the player sees.
    pub shown: Option<TileId>,
    /// The description's area on the canvas, while ready.
    pub desc_area: Option<Rect>,
    /// The tab and frame that area was found for.
    pub desc_for: Option<(usize, Frame)>,
    /// The pieces [`usable`] checked this frame, for the drawing (`super::draw`) to paint: one
    /// measured layout, so what was checked is what is painted.
    pub(crate) pieces: Vec<Piece>,
    /// [`CUT_AID`].
    pub cut: Option<String>,
    /// [`CUT_AID`]'s time.
    pub cut_until: Option<f32>,
    /// A menu over the page when this frame began ([`modal_over`]): set before the scripts,
    /// so it holds through the frame that menu closes in.
    pub modal: bool,
    /// Another menu took this frame's tile input (`super::input::feed`).
    pub others: bool,
    /// Words of the settings, found where the game is.
    pub words: PageWords,
    written: Written,
}

/// What the page writes from the game's settings ([`pass`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageWords {
    /// General's switch's key ([`SWITCH_HINT`]).
    pub switch_hint: String,
    /// Its word with the reputations up ([`SWITCH_GENERAL`]).
    pub switch_general: String,
    /// The hints' keys ([`HINT_SETTINGS`]).
    pub hint_keys: [String; 4],
    /// The word before the level under the Vault Boy ([`LEVEL_WORD`]).
    pub level_word: String,
}

impl StatsTiles {
    /// The description's area, only when it belongs to the tab and frame shown now: the tab can
    /// change after the last decision and before the next.
    pub fn desc_area_for(&self, tab: usize, frame: Frame) -> Option<Rect> {
        self.desc_area.filter(|_| self.ready.is_some() && self.desc_for == Some((tab, frame)))
    }
}

/// What a pass needs of the fonts.
pub(crate) trait PageFonts {
    /// One round of measuring the given text tiles, with the tree worked out.
    fn write_sizes(&mut self, ui: &mut Ui, texts: &[TileId], written: &mut SizesWritten) -> usize;
    /// The menu's pieces, as the drawing paints them.
    fn set_out(&mut self, ui: &Ui, menu: TileId) -> Vec<Piece>;
    /// A picture's own file's size in texels, when the page needs it.
    fn picture_texels(&mut self, _name: &str) -> Option<(f64, f64)> {
        None
    }
    /// A text's size in a font number, wrapped at `wrap`, as [`Self::write_sizes`] measures it;
    /// `None` when the page cannot tell.
    fn text_size(&mut self, _font: f64, _text: &str, _wrap: Option<f64>) -> Option<(f64, f64)> {
        None
    }
}

impl PageFonts for FontKit<'_> {
    fn write_sizes(&mut self, ui: &mut Ui, texts: &[TileId], written: &mut SizesWritten) -> usize {
        let size = self.menu_measure(&menus::fonts_of(ui, texts));
        menus::write_text_sizes_of(ui, texts, written, &size)
    }
    fn set_out(&mut self, ui: &Ui, menu: TileId) -> Vec<Piece> {
        super::draw::whole_pieces(ui, menu, self, true)
    }
    fn picture_texels(&mut self, name: &str) -> Option<(f64, f64)> {
        self.picture_size(name).map(|s| (f64::from(s.x), f64::from(s.y)))
    }
    fn text_size(&mut self, font: f64, text: &str, wrap: Option<f64>) -> Option<(f64, f64)> {
        Some(self.menu_measure(&[font])(font, text, wrap))
    }
}

/// `filewidth` and `fileheight` of every picture under `menu`, once for each file and zoom it
/// shows: the file works places and sizes out from them, the RAD meter's place and the limbs'
/// brackets' size among them, and they read nothing until written [documented: the file's comment
/// "filewidth/fileheight already adjusted for zoom"; observed: run07, the meter's x 189.4 = 855 -
/// 512 x 1.3, the karma's picture 384 = 256 x 1.5; design: as `super::tutorial_menu::write_symbol`].
/// The file's own size is kept in texels and the zoom the tile has now applied on every pass, so a
/// zoom changed on the same file writes the sizes again [design: as
/// `tile-repair-menu-1468:engine/nvf-play/src/tiles/repair_menu.rs::write_file_sizes`]. A picture
/// of an atlas has no file of its own.
pub(super) fn write_file_sizes(ui: &mut Ui, menu: TileId, done: &mut HashMap<TileId, (String, f64)>, texels: &mut HashMap<String, Option<(f64, f64)>>, fonts: &mut dyn PageFonts) {
    let mut due = Vec::new();
    let mut stack = vec![menu];
    while let Some(t) = stack.pop() {
        let tile = ui.tile(t);
        if !tile.live {
            continue;
        }
        stack.extend(tile.children.iter().copied());
        if tile.kind != "image" || tile.get("texatlas").is_some() {
            continue;
        }
        let name = tile.get("filename").map(|v| v.text().trim().to_string()).unwrap_or_default();
        if name.is_empty() || super::runtime_images::is_runtime(&name) {
            continue;
        }
        let zoom = tile.get("zoom").filter(|v| !matches!(v, Value::Sym(_))).map_or(100.0, Value::num);
        let zoom = if zoom > 0.0 { zoom } else { 100.0 };
        if done.get(&t).is_none_or(|(n, z)| *n != name || *z != zoom) {
            due.push((t, name, zoom));
        }
    }
    for (t, name, zoom) in due {
        if let Some((w, h)) = *texels.entry(name.clone()).or_insert_with(|| fonts.picture_texels(&name)) {
            ui.set_trait(t, "filewidth", Value::Num(w * zoom / 100.0));
            ui.set_trait(t, "fileheight", Value::Num(h * zoom / 100.0));
        }
        done.insert(t, (name, zoom));
    }
}

/// One pass over tab `tab`; the menu when all of it went through. `late`: the pass before the
/// drawing; the other, before the scripts, does the sizes only for a model that changed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn page(ui: &mut Ui, snap: &Snapshot, view: &mut StatsView, tab: usize, device: bool, late: bool, tiles: &mut StatsTiles, fonts: &mut dyn PageFonts) -> Result<TileId, String> {
    let mut words = tiles.words.clone();
    let w = &mut tiles.written;
    let p = match w.paths.filter(|c| c.page == tab && c.live(ui) && ui.menu(STATS_MENU) == Some(c.menu)) {
        Some(p) => p,
        None => paths(ui, tab, tiles.cut.as_deref())?,
    }
    .in_mode(view.reputation);
    // A new menu tile (the tree built again): its rows, sizes and writes are all to do again.
    if w.menu != Some(p.menu) {
        let fallbacks = std::mem::take(&mut w.fallbacks);
        *w = Written { menu: Some(p.menu), fallbacks, ..Default::default() };
    }
    w.paths = Some(p);
    let (list, other) = (p.list(), p.other_list());
    // Another list: its rows are its own, as with a new tree; General's two keep theirs.
    if w.rows_in != Some(list) {
        if other.is_some() && w.rows_in == other {
            std::mem::swap(&mut w.rows, &mut w.other_rows);
        } else {
            for r in std::mem::take(&mut w.rows).into_iter().chain(std::mem::take(&mut w.other_rows)) {
                ui.unload_tile(r.tile);
            }
        }
        w.rows_in = Some(list);
        w.shown = None;
        w.geometry = None;
        w.other_shown = None;
        w.other_geometry = None;
        w.list_drawn = None;
    }
    // A word of the file's own stands; a name the reader left as it is, or none, is the setting's.
    if let TabPaths::General(g) = &p.tab {
        let file = w.file_switch_word.get_or_insert_with(|| g.button_text.and_then(|t| ui.tile(t).get("string")).map(|v| v.text().to_string()).unwrap_or_default());
        if !file.trim().is_empty() && !file.trim_start().starts_with('&') {
            words.switch_general = file.clone();
        }
    }
    if tab == STATUS_TAB {
        view.set_status_pages(if snap.hardcore { SUB_BUTTONS.len() } else { 3 });
    }
    let aimed = (tab == STATUS_TAB && view.status == 0).then(|| view.aimed()).flatten();
    let at = At { tab, reputation: view.reputation, status: view.status, highlight: view.highlight, selected: view.selected[tab], device, aimed };
    let rows = rows_of(snap, tab, at.reputation, at.status);
    let wanted: Vec<(RowKey, &str)> = rows.iter().map(|r| (r.key, r.name.as_str())).collect();
    let row_parts = p.row_parts();
    if ensure_rows(ui, list, row_parts, &mut w.rows, &wanted)? {
        // Rows made or gone: nothing written, measured or checked for the last set holds.
        w.shown = None;
        w.geometry = None;
    }
    // General's other list keeps its rows too [observed: run07, the 43 in both modes].
    let mut changed = false;
    if let Some(other) = other {
        let data = rows_of(snap, tab, !at.reputation, at.status);
        let wanted: Vec<(RowKey, &str)> = data.iter().map(|r| (r.key, r.name.as_str())).collect();
        if ensure_rows(ui, other, row_parts, &mut w.other_rows, &wanted)? {
            w.other_shown = None;
            w.other_geometry = None;
        }
        if w.other_shown.as_deref() != Some(&*data) {
            write_rows(ui, &w.other_rows, &data);
            w.other_shown = Some(data.into_owned());
            changed = true;
        }
    }
    let changed = changed || !w.shown.as_ref().is_some_and(|s| s.same(snap, at, &words));
    if changed {
        let shown = Shown::of(snap, at, &words);
        write_model(ui, &p, &w.rows, &shown);
        w.shown = Some(shown);
        w.tell = true;
    }
    if changed || late {
        write_file_sizes(ui, p.menu, &mut w.file_sizes, &mut w.file_texels, fonts);
        ui.update();
        let texts = menus::texts(ui, p.menu);
        // Three rounds: a size leads to a place and a place to a wrap width [design: as
        // `super::tutorial_menu::settle`].
        for _ in 0..3 {
            if fonts.write_sizes(ui, &texts, &mut w.sizes) == 0 {
                break;
            }
            ui.update();
        }
        write_tabs(ui, &p, &mut w.tabs_marked);
        if let TabPaths::Status(st) = &p.tab {
            if !std::mem::replace(&mut w.subs_marked, true) {
                let subs: Vec<(TileId, Option<TileId>)> = st.subs.iter().map(|&b| (b, find_named(ui, b, TAB_INNER))).collect();
                mark_inner_rects(ui, subs);
            }
        }
        write_frame(ui, &p, device, &mut w.flat);
        ui.update();
        let mut g = geometry(ui, &p, list, &w.rows, at.selected, view.highlight, view.top[tab])?;
        // The file's count of the rows, for the model's scroll; what it held is kept in range.
        view.set_rows_shown(tab, g.visible);
        view.set_scroll_most(tab, g.counts.map(|c| c.most().round() as usize));
        // When selection moved on a variable-height scrolling list, keep the selected row in view.
        if let Some(c) = g.counts {
            let delta = c.delta.unwrap_or(0.0);
            let sel_changed = w.last_selected != Some((tab, at.selected));
            if sel_changed {
                if let Some(&(h, ry)) = g.rows.get(at.selected) {
                    let want = list::keep_row_shown(view.top[tab] as f64, ry, h, g.list, delta, c.most()).round() as usize;
                    if want != view.top[tab] {
                        view.top[tab] = want;
                        g = geometry(ui, &p, list, &w.rows, at.selected, view.highlight, view.top[tab])?;
                    }
                }
            }
        }
        w.last_selected = Some((tab, at.selected));
        if view.desc_bound() != g.desc_bound {
            view.set_desc_bound(g.desc_bound);
        }
        if w.geometry.as_ref() != Some(&g) {
            write_geometry(ui, &p, list, &w.rows, &g, &mut w.counted);
            ui.update();
            w.geometry = Some(g);
            w.tell = true;
        }
        // The other list is out of sight: what it cannot lay out sends nothing back.
        if let Some(g) = other.and_then(|o| geometry(ui, &p, o, &w.other_rows, 0, false, 0).ok().map(|g| (o, g))) {
            if w.other_geometry.as_ref() != Some(&g.1) {
                write_geometry(ui, &p, g.0, &w.other_rows, &g.1, &mut w.counted);
                list::write_value(ui, g.0, 0.0);
                ui.update();
                w.other_geometry = Some(g.1);
            }
        }
        // [design: layout::OWN_ALPHA] Only the pictures' alpha is independent; the bar's
        // visibility still hides them when the list fits. Cover pictures injected later too.
        for picture in list_bar_pictures(ui, list).into_iter().flatten() {
            ui.write_changed(picture, layout::OWN_ALPHA, Value::Num(1.0));
        }
        // On every drawn frame, so a window injected or enabled later crops too.
        fnv_ui::input::mark_clip_windows(ui, p.menu);
        list::unclip_row_children(ui, list, w.rows.iter().map(|r| r.tile));
        if let Some(other) = other {
            list::unclip_row_children(ui, other, w.other_rows.iter().map(|r| r.tile));
        }
    }
    write_scroll(ui, p.bar, view.desc);
    // The model's first row over the file's own count of the clicks, as the description's.
    let list_most = w.geometry.as_ref().and_then(|g| g.counts).map(|c| c.most());
    if let Some(most) = list_most {
        list::write_value(ui, list, (view.top[tab] as f64).min(most));
    }
    if let Some(o) = other.filter(|_| w.other_geometry.as_ref().is_some_and(|g| g.counts.is_some())) {
        list::write_value(ui, o, 0.0);
    }
    if late {
        ui.update();
        // Every final pass, so readiness is about the tree that is about to be painted.
        let mut pieces = fonts.set_out(ui, p.menu);
        let in_sight = w.geometry.as_ref().map_or(0..w.rows.len(), |g| g.shown.clone());
        tiles.desc_area = usable(ui, &p, &w.rows, in_sight, &pieces)?;
        if let (TabPaths::Status(st), Some(limb)) = (&p.tab, aimed) {
            append_limb_box(ui, &mut pieces, st.limbs[limb]);
        }
        tiles.pieces = pieces;
        let w = &mut tiles.written;
        if std::mem::take(&mut w.tell) {
            info!("{}", describe(ui, &p, w));
        }
        if let Some(most) = list_most {
            let drawn = list::value_shown(ui, list).unwrap_or(0.0);
            if w.list_drawn != Some(drawn) {
                let n = |x: f64| Value::Num(x).text();
                info!("tiles: {STATS_MENU} list scrolled to {} of {}", n(drawn), n(most));
                let [up, down, marker] = list_bar_pictures(ui, list).map(|t| {
                    t.is_some_and(|tile| tiles.pieces.iter().any(|p| p.tile == tile && matches!(p.look, Look::Picture { .. }) && p.rect.width > 0.0 && p.rect.height > 0.0))
                });
                info!("tiles: {STATS_MENU} list bar pictures to paint: up {up}, down {down}, marker {marker}");
                if let Some(marker) = list::child(ui, list, list::SCROLLBAR).and_then(|b| list::child(ui, b, list::MARKER)) {
                    info!("tiles: {STATS_MENU} list marker {} high, {} a step", n(num(ui, marker, "height")), n(num(ui, marker, "_step_height")));
                }
                w.list_drawn = Some(drawn);
            }
        }
        let now = (num(ui, p.bar, "_current_value"), num(ui, p.desc, "y"), view.desc_bound());
        if w.drawn != Some(now) {
            let (scroll, text_y) = (Value::Num(now.0).text(), Value::Num(now.1).text());
            info!("tiles: {STATS_MENU} description scroll drawn at {scroll} of {}, text y {text_y}", view.desc_bound());
            w.drawn = Some(now);
        }
    }
    Ok(p.menu)
}

/// Frame the aimed limb's condition bar (or its "Crippled" word at 0) on tile-drawn
/// Status > CND, matching the hand-built page's selection box (`stats::condition`)
/// [design: #3431]. Appended after [`usable`] so hit-testing and tile counts stay untouched.
fn append_limb_box(ui: &Ui, pieces: &mut Vec<Piece>, limb: LimbPaths) {
    let target = pieces
        .iter()
        .find(|p| p.tile == limb.pct)
        .or_else(|| pieces.iter().find(|p| inside(ui, p.tile, limb.pct)))
        .or_else(|| pieces.iter().find(|p| p.tile == limb.crippled))
        .or_else(|| pieces.iter().find(|p| p.tile == limb.picture))
        .cloned();
    let Some(base) = target else { return };
    let pad = if base.tile == limb.crippled { 4.0 } else { 0.0 };
    let r = layout::Rect {
        left: base.rect.left - pad,
        top: base.rect.top - pad,
        width: base.rect.width + pad * 2.0,
        height: base.rect.height + pad * 2.0,
    };
    let t = f64::from(crate::pipboy::frame::THICK);
    if r.width <= 0.0 || r.height <= 0.0 {
        return;
    }
    let scale = crate::pipboy::frame::LINE / 255.0;
    let colour = [base.colour[0] * scale, base.colour[1] * scale, base.colour[2] * scale, base.colour[3]];
    let depth = base.depth + 1.0;
    let edges = [
        layout::Rect { left: r.left, top: r.top, width: r.width, height: t },
        layout::Rect { left: r.left, top: (r.top + r.height - t).max(r.top), width: r.width, height: t },
        layout::Rect { left: r.left, top: r.top, width: t, height: r.height },
        layout::Rect { left: (r.left + r.width - t).max(r.left), top: r.top, width: t, height: r.height },
    ];
    for rect in edges {
        pieces.push(Piece { tile: base.tile, look: Look::Box, rect, clip: base.clip, colour, depth, tex: None, turn: None, crop: None });
    }
}

/// A line for the log when what the page shows changes (smoke reads these).
fn describe(ui: &Ui, p: &Paths, w: &Written) -> String {
    let n = |x: f64| Value::Num(x).text();
    let (Some(s), Some(g)) = (w.shown.as_ref(), w.geometry.as_ref()) else { return format!("tiles: {STATS_MENU} page written") };
    let row = s.rows.get(s.at.selected);
    let says = row.map_or(String::new(), row_string);
    // The first tab's words are the first slice's, which its scenarios read.
    let name = match (p.page, s.at.reputation) {
        (STATUS_TAB, _) => format!("Status {}", stats::STATUS_PAGES[s.at.status.min(5)]),
        (1, _) => "page".to_string(),
        (GENERAL_TAB, true) => "General (Reputation)".to_string(),
        (GENERAL_TAB, false) => "General (General)".to_string(),
        (tab, _) => Page::Stats.tabs()[tab].to_string(),
    };
    let status = s.status.as_ref().map_or(String::new(), |st| {
        let bars: Vec<String> = st.limbs.iter().map(|c| n(*c)).collect();
        let hints: Vec<String> = st.hints.iter().filter(|h| h.shown).map(|h| format!("{} {} {}", h.words, h.key, if h.lit { "lit" } else { "dim" })).collect();
        format!("limbs {}, hints {}, rads {}, hardcore {}, ", bars.join(" "), hints.join(" and "), crate::pipboy::stats::model::whole(st.meter), if st.hardcore { "on" } else { "off" })
    });
    let side = match (&s.general, s.rows.get(s.at.selected).and_then(|r| r.badge.as_deref())) {
        (Some(g), _) if s.at.reputation => format!("reputation picture {}, switch {}, ", g.rep_picture, g.switch_word),
        (Some(g), _) => format!("karma picture {}, karma title {}, switch {}, ", g.karma_picture, g.karma_title, g.switch_word),
        (None, Some(badge)) => format!("badge {badge}, "),
        (None, None) => String::new(),
    };
    format!(
        "tiles: {STATS_MENU} {name} from tiles: {} rows, \"{}\" selected, value {}, boxed at {}, list height {}, row height {}, description {} high, scrolls {}, {status}{side}{}",
        s.rows.len(),
        says,
        row.and_then(|r| r.value.as_deref()).unwrap_or("none"),
        n(g.highlight.0),
        n(g.list),
        n(g.rows.first().map_or(0.0, |r| r.0)),
        n(num(ui, p.desc, "height")),
        g.desc_bound,
        if s.at.device { "device" } else { "screen menu" },
    )
}

/// What a pass over the page reads.
#[derive(SystemParam)]
pub(crate) struct StatsPage<'w> {
    script: Option<ResMut<'w, ScriptState>>,
    switch: Option<Res<'w, TileMenus>>,
    pip: Option<Res<'w, PipBoy>>,
    prepared: Option<Res<'w, Prepared>>,
    view: Option<ResMut<'w, StatsView>>,
    tiles: ResMut<'w, StatsTiles>,
    fonts: FontKit<'w>,
    time: crate::pause::AidTime<'w>,
}

/// Whether the page is eligible to prepare: the switch on and a tab of [`TILED_TABS`] showing.
pub fn eligible(switch: TileMenus, pip: &PipBoy) -> bool {
    switch.stats && pip.shown() && pip.page == Page::Stats && TILED_TABS.contains(&pip.tab())
}

/// A pass; `None` when the page is not eligible (nothing is touched then).
fn pass(p: &mut StatsPage<'_>, late: bool) -> Option<Result<TileId, String>> {
    let pip = p.pip.as_deref()?;
    if !eligible(p.switch.as_deref().copied().unwrap_or_default(), pip) {
        return None;
    }
    let (tab, device) = (pip.tab(), pip.frame == Frame::Device);
    let game = &mut p.script.as_mut()?.rt.as_mut()?.game;
    game.ui_files.as_ref()?;
    // The load order's settings over NVForever's labels; with a controller the button that does
    // it (`crate::pad::menu_buttons`), as the hand-built page says.
    let setting = |name: &str| fnv_game::ui_loading::game_string(&*game, name);
    let mut words = p.tiles.words.clone();
    if tab == GENERAL_TAB {
        words.switch_hint = crate::pad::game_hint(&setting(SWITCH_HINT), &[KeyCode::KeyR], crate::pad::MenuAt { code: Some(fnv_game::game::menus::STATS), tab });
        words.switch_general = setting(SWITCH_GENERAL);
    }
    if tab == STATUS_TAB {
        let rad = p.view.as_deref().is_some_and(|v| v.status == 1);
        let at = crate::pad::MenuAt { code: Some(fnv_game::game::menus::STATS), tab: if rad { crate::pad::STATS_RAD } else { 0 } };
        words.hint_keys = std::array::from_fn(|i| crate::pad::game_hint(&setting(HINT_SETTINGS[i]), &[stats::HINT_KEY_CODES[i]], at));
        words.level_word = setting(LEVEL_WORD);
    }
    if p.tiles.words != words {
        p.tiles.words = words;
    }
    let Some(snap) = p.prepared.as_deref().and_then(|x| x.snap.as_ref()) else {
        return late.then(|| Err("nothing to show yet".to_string()));
    };
    // What the page does to the view (the bound) is no new look for the hand-built page.
    let view = p.view.as_mut()?.bypass_change_detection();
    Some(page(&mut game.ui_tree, snap, view, tab, device, late, &mut p.tiles, &mut p.fonts))
}

/// Before the scripts, after the tile input: this frame's model in the tree, and the scroll.
pub(crate) fn write(mut p: StatsPage) {
    // A failure here is told by `settle`, which decides.
    let _ = pass(&mut p, false);
}

/// Before the drawing, after the model's keys: what they changed, the tree worked out, and the
/// one decision every reader of this frame shares.
pub(crate) fn settle(mut p: StatsPage) {
    if p.tiles.cut_until.is_some_and(|until| p.time.elapsed_secs() >= until) {
        info!("test aid: {CUT_AID} over");
        p.tiles.cut = None;
        p.tiles.cut_until = None;
    }
    let ready = match pass(&mut p, true) {
        Some(Ok(menu)) => Some(menu),
        Some(Err(reason)) => {
            let line = format!("{STATS_MENU} drawn by hand: {reason}");
            if p.tiles.written.fallbacks.insert(line.clone()) {
                warn!("{line}");
            }
            None
        }
        None => None,
    };
    if ready.is_none() {
        // The next time it is written whole; the rows stay.
        let w = &mut p.tiles.written;
        w.shown = None;
        w.geometry = None;
        w.drawn = None;
        w.list_drawn = None;
        p.tiles.desc_area = None;
        p.tiles.pieces.clear();
    }
    if p.tiles.ready != ready {
        p.tiles.ready = ready;
    }
    p.tiles.desc_for = ready.and(p.pip.as_deref()).map(|pip| (pip.tab(), pip.frame));
}

/// The keys the hand-built page acts on itself (`crate::pipboy::stats::keys`,
/// `crate::pipboy::keys`), by the tile input's names, and the controller's triggers standing in
/// for its pages' keys (`crate::pad::menu_buttons`). R, E and X and the controller's A, Y and X
/// too, for General's switch and Status's hints: the file's root binds these keys to those buttons
/// [documented: its `_PCButton_*`], which would act twice.
pub const HAND_BUILT_KEYS: [&str; 16] = ["W", "S", "A", "D", "R", "E", "X", "xup", "xdown", "xleft", "xright", "xbuttonlt", "xbuttonrt", "xbuttona", "xbuttony", "xbuttonx"];

/// Whether the open menu is one over the Pip-Boy's pages and not one of them.
pub fn modal_over(open: Option<u16>) -> bool {
    use fnv_game::game::menus;
    open.is_some_and(|code| !menus::answers(menus::PIPBOY, code))
}

/// The menu, when the page takes this frame's tile input: painted last frame, and the page up,
/// with nothing over it. Every reader of the frame's input decides by this.
pub fn takes(tiles: &StatsTiles, switch: TileMenus, pip: &PipBoy, game: &fnv_game::Game<'_>, paused: bool) -> Option<TileId> {
    if !pip.open() || !eligible(switch, pip) || paused || modal_over(game.menu) {
        return None;
    }
    tiles.shown.filter(|&m| game.ui_files.is_some() && game.ui_tree.is_live(m))
}

/// What a tile of the page stands for, by the tile or a parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatsTile {
    Row(usize),
    Tab(usize),
    StepUp,
    StepDown,
    PageUp,
    PageDown,
    Marker,
    /// A piece of the list's own scroll bar.
    ListBar(BarPiece),
    /// General's switch between its lists.
    Switch,
    /// A button of the status page.
    Sub(usize),
    /// A hint on Status.
    Hint(usize),
    /// The limb-switch hint on Status > CND (`stats_healing_mode`, shown with a controller).
    SwitchLimbs,
    /// A limb's bar, its word or its picture on CND: Stimpak outside Hardcore [owner 2026-10-23],
    /// Doctor's Bag in Hardcore (#3431).
    Limb(usize),
}

/// What `tile` stands for: a row the page made, a tab, General's switch, or a piece of the
/// description's or the list's scroll bar [documented: scrollbar_vert.xml's names].
pub fn stats_tile(ui: &Ui, p: &Paths, rows: &[MadeRow], tile: TileId) -> Option<StatsTile> {
    let mut at = Some(tile);
    let (mut in_bar, mut in_list) = (false, false);
    while let Some(t) = at {
        in_bar |= t == p.bar;
        in_list |= p.is_list(t);
        at = ui.tile(t).parent;
    }
    let status = match &p.tab {
        TabPaths::Status(st) => Some(st),
        _ => None,
    };
    let mut at = Some(tile);
    while let Some(t) = at {
        if let Some(i) = rows.iter().position(|r| r.tile == t).filter(|_| status.is_none()) {
            return Some(StatsTile::Row(i));
        }
        if let Some(st) = status {
            if let Some(i) = st.subs.iter().position(|&x| x == t) {
                return Some(StatsTile::Sub(i));
            }
            if let Some(i) = st.hints.iter().position(|h| h.button == t) {
                return Some(StatsTile::Hint(i));
            }
            if st.healing == Some(t) {
                return Some(StatsTile::SwitchLimbs);
            }
            if let Some(i) = st.limbs.iter().position(|l| l.pct == t || l.crippled == t || l.picture == t) {
                return Some(StatsTile::Limb(i));
            }
            // Over the head the pointer has the face [observed: run07b, id 24 and not 12].
            if st.face == t {
                return Some(StatsTile::Limb(0));
            }
        }
        if let Some(i) = p.tabs.iter().position(|&x| x == t) {
            return Some(StatsTile::Tab(i));
        }
        if matches!(p.tab, TabPaths::General(g) if g.button == t) {
            return Some(StatsTile::Switch);
        }
        if in_bar {
            match ui.tile(t).name.to_ascii_lowercase().as_str() {
                "scrollbar_vert_up" => return Some(StatsTile::StepUp),
                "scrollbar_vert_down" => return Some(StatsTile::StepDown),
                "scrollbar_vert_page_up" => return Some(StatsTile::PageUp),
                "scrollbar_vert_page_down" => return Some(StatsTile::PageDown),
                "scrollbar_vert_marker" => return Some(StatsTile::Marker),
                _ => {}
            }
        }
        if in_list {
            if let Some(piece) = list::bar_piece(ui, t) {
                return Some(StatsTile::ListBar(piece));
            }
        }
        at = ui.tile(t).parent;
    }
    None
}

/// The scroll steps of a page area's click: the window's less a step, at least one [design].
pub fn page_steps(ui: &Ui, p: &Paths) -> i32 {
    ((num(ui, p.desc_rect, "height") / f64::from(stats::DESC_STEP)).floor() as i32 - 1).max(1)
}

/// Which scroll bar a marker is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarOf {
    Description,
    List,
}

/// A press on a marker, held: its bar, the bar's top and how far under the marker's top the
/// pointer took it (canvas units).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarkerDrag {
    of: BarOf,
    marker: TileId,
    top: f64,
    grab: f64,
}

/// The [`MarkerDrag`] of a press on the marker of `bar`, in `p`'s menu; worked out once a press,
/// as the tutorial's.
pub fn marker_grab(ui: &Ui, p: &Paths, of: BarOf, pointer_y: f64) -> Option<MarkerDrag> {
    let bar = match of {
        BarOf::Description => p.bar,
        BarOf::List => list::child(ui, p.list(), list::SCROLLBAR)?,
    };
    let marker = find_named(ui, bar, list::MARKER)?;
    let placed = layout::layout(ui, p.menu, &|_, _, _| (0.0, 0.0), true).into_iter().find(|i| i.tile == marker)?.rect;
    Some(MarkerDrag { of, marker, top: placed.top - num(ui, marker, "y"), grab: pointer_y - placed.top })
}

/// The scroll steps with the marker held at `pointer_y`, by the file's `_step_height`
/// [documented: scrollbar_vert.xml], within `most`.
pub fn steps_under(ui: &Ui, drag: MarkerDrag, pointer_y: f64, most: usize) -> Option<usize> {
    let step = num(ui, drag.marker, "_step_height");
    (step > 0.0).then(|| ((pointer_y - drag.grab - drag.top) / step + 1e-9).floor().clamp(0.0, most as f64) as usize)
}

/// What [`feed`] keeps between frames.
#[derive(Default)]
pub(crate) struct Feed {
    /// The page took input last frame, in this frame of the Pip-Boy, on this tab and list.
    taking: Option<(Frame, usize, bool)>,
    /// The pointer when the page came up, until it moves: a row under it does not take the
    /// selection, as the hand-built list's rule.
    drawn_at: Option<Vec2>,
    /// The row the pointer last selected, while it stays on it.
    acted: Option<usize>,
    drag: Option<MarkerDrag>,
    touch_drag: bool,
    /// Where the pointer was last frame, and the tile under it then.
    last_at: Option<(f64, f64)>,
    under: Option<TileId>,
}

/// What the row under the pointer does to the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoverAct {
    Select(usize),
    BoxOff,
}

/// The row under the pointer, `under`, against the one it last selected ([`Feed::acted`]).
/// `settled`: the pointer has moved since the page came up.
fn follow_hover(acted: &mut Option<usize>, under: Option<usize>, settled: bool) -> Option<HoverAct> {
    match under {
        Some(i) if settled && *acted != Some(i) => {
            *acted = Some(i);
            Some(HoverAct::Select(i))
        }
        Some(_) => None,
        None => acted.take().map(|_| HoverAct::BoxOff),
    }
}

/// The row the pointer may select by being over it: one in sight. A list's window can show a strip
/// of the row after its last whole one; selecting that would scroll it in, bring the next under
/// a still pointer and walk the list a row a frame. A click on it still selects [design].
fn row_to_hover(under: Option<usize>, in_sight: &std::ops::Range<usize>) -> Option<usize> {
    under.filter(|i| in_sight.contains(i))
}

/// The row a file's own key binding moved the highlight onto in this step, wherever it went,
/// the row under the pointer too: the step says what moved it. Not for a press the hand-built
/// keys act on as well, or the page would step twice.
fn keyed_row(ui: &Ui, p: &Paths, rows: &[MadeRow], report: &fnv_ui::input::Report, physical: &[&str]) -> Option<usize> {
    if !report.over_by_key || physical.iter().any(|k| menus::hand_built_takes(STATS_MENU, k)) {
        return None;
    }
    let (_, to) = report.over?;
    match stats_tile(ui, p, rows, to?)? {
        StatsTile::Row(i) => Some(i),
        _ => None,
    }
}

/// What the page's tile input reads of the Pip-Boy.
#[derive(SystemParam)]
pub(crate) struct FeedPip<'w> {
    pip: Res<'w, PipBoy>,
    screen: Option<Res<'w, PipBoyScreen>>,
    aid: Res<'w, PipBoyAid>,
    paused: Option<Res<'w, crate::pause::Paused>>,
    touch:Option<Res<'w,crate::pipboy::touch_items::TouchItems>>,
    admission: crate::pipboy::touch_items::Admission<'w>,

}

pub(crate) fn touch_target(tiles:&StatsTiles,ui:&Ui,menu:TileId,point:Vec2,view:&StatsView)->Option<crate::pipboy::touch_scroll::NativeTarget>{
    use crate::pipboy::touch_scroll::{NativeRow,NativeTarget};
    NativeTarget::at(ui,menu,point,|tile| {
        let paths=tiles.written.paths?;
        match stats_tile(ui,&paths,&tiles.written.rows,tile) {
            Some(StatsTile::Row(index))=>tiles.written.rows.get(index).map(|r|NativeRow::Stats(r.key)),
            Some(StatsTile::Hint(hint))=>Some(NativeRow::StatsHint{hint,status:view.status,limb:if hint==stats::model::STIMPAK {view.limb}else{None}}),
            _=>None,
        }
    },|_|None,|_|true)
        .map(|target| target.with_control(|tile| tiles.written.paths.is_some_and(|p|matches!(stats_tile(ui,&p,&tiles.written.rows,tile),Some(StatsTile::Marker|StatsTile::ListBar(BarPiece::Marker))))))
}
pub(crate) fn touch_navigation(tiles:&StatsTiles,ui:&Ui)->Vec<crate::pipboy::tab_swipe::Region> {
    use crate::pipboy::tab_swipe::{native_region,Kind};
    let Some(paths)=tiles.written.paths else {return Vec::new()};
    tiles.pieces.iter().filter_map(|p| {
        let kind=if p.tile==paths.title {Kind::Sections}
            else if matches!(stats_tile(ui,&paths,&tiles.written.rows,p.tile),Some(StatsTile::Tab(_))) {Kind::Subtabs}
            else {return None};
        native_region(p,kind)
    }).collect()
}
/// The page's tile input, on a frame it takes it ([`takes`]): see the module notes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn feed(
    mut script: ResMut<ScriptState>,
    switch: Res<TileMenus>,
    from: FeedPip,
    mut tiles: ResMut<StatsTiles>,
    devices: Devices,
    view: Option<ResMut<StatsView>>,
    (mut tabs, mut hovered, mut asks): (MessageWriter<PipBoyInput>, MessageWriter<TabHovered>, MessageWriter<stats::UseHint>),
    mut state: Local<Feed>,
) {
    let FeedPip { pip, screen, aid, paused, touch, admission } = from;
    let Some(rt) = script.rt.as_mut() else { return };
    let game = &mut rt.game;
    let modal = eligible(*switch, &pip) && pip.open() && modal_over(game.menu);
    if tiles.modal != modal {
        tiles.modal = modal;
    }
    let is_paused = paused.as_ref().is_some_and(|p| p.0);
    let tab = pip.tab();
    let menu = takes(&tiles, *switch, &pip, game, is_paused).filter(|_| !tiles.others);
    // The tiles found for this tab: a tab changed before this step was not painted yet.
    let (Some(menu), Some(p), Some(mut view)) = (menu, tiles.written.paths.filter(|p| Some(p.menu) == tiles.shown && p.page == tab), view) else {
        if menu.is_some() {
            fnv_game::ui_input::idle(game);
        }
        *state = Feed::default();
        return;
    };
    let mut mobile=crate::pipboy::touch_scroll::native_frame(touch.as_deref(),&devices.mouse);
    if !admission.allows_contact(touch.as_ref().and_then(|t|t.contact_id())) { mobile=Default::default(); }
    if mobile.tap {
        let target=mobile.at.and_then(|at|touch_target(&tiles,&game.ui_tree,menu,at,&view));
        mobile.tap=touch.as_ref().is_some_and(|t|t.native_release_valid(target));
    }
    if mobile.control && mobile.started {
        let target=mobile.at.and_then(|at|touch_target(&tiles,&game.ui_tree,menu,at,&view));
        mobile.started=target.is_some_and(|t|t.control()) && touch.as_ref().is_some_and(|t|t.native_release_valid(target));
    }
    let pointer = mobile.at.or_else(||screen.as_ref().and_then(|s| s.pointer));
    let taking = (pip.frame, tab, view.reputation);
    if state.taking != Some(taking) {
        *state = Feed { taking: Some(taking), drawn_at: pointer, ..Default::default() };
    }
    if pointer != state.drawn_at {
        state.drawn_at = None;
    }
    let settled = state.drawn_at.is_none() || pointer.is_none();
    // The test aid's click (`crate::pipboy::drive_pointer`): 1 is its press's frame.
    let aid_press = aid.click == 1 && !aid.right;
    let held = mobile.held || aid_press || devices.mouse.pressed(MouseButton::Left);
    let at = pointer.map(|p| (f64::from(p.x), f64::from(p.y)));
    let mut input = fnv_ui::input::Input {
        at,
        press: mobile.press() || aid_press || devices.mouse.just_pressed(MouseButton::Left),
        shift: devices.keys.pressed(KeyCode::ShiftLeft) || devices.keys.pressed(KeyCode::ShiftRight),
        ..Default::default()
    };
    let pad_tab = if tab == STATUS_TAB && view.status == 1 { crate::pad::STATS_RAD } else { tab };
    let pad_menu = crate::pad::MenuAt { code: Some(fnv_game::game::menus::STATS), tab: pad_tab };
    let physical = frame_keys(&mut input, &devices, game, pad_menu);
    let hardcore = game.hardcore();
    let report = fnv_game::ui_input::step(game, &[menu], &input, &fnv_ui::input::no_measure);
    let ui = &game.ui_tree;
    // What is under the pointer itself, whatever a key moved the highlight to; looked for again
    // only when the pointer moved or the step's own look changed something.
    let pointer_moved = at != state.last_at;
    if report.over.is_some() || pointer_moved {
        state.under = at.and_then(|at| fnv_ui::input::hit(ui, &[menu], at, &fnv_ui::input::no_measure)).map(|h| h.tile);
        state.last_at = at;
    }
    log_report(ui, &report);
    let rows = &tiles.written.rows;
    let what = |t: TileId| stats_tile(ui, &p, rows, t);
    if let Some((_, Some(to))) = report.over.filter(|_| !mobile.control) {
        if let Some(StatsTile::Tab(i)) = what(to) {
            hovered.write(TabHovered(i));
        }
    }
    let under = state.under;
    // What is under the pointer of what the hand-built page calls so (`Spot`): the limb a
    // Stimpak (or in Hardcore, a Doctor's Bag) goes on while there (`StatsView::aimed`); the
    // file's own `mouseover` boxes draw the buttons' look. An unmoved pointer on a limb does
    // not reclaim a limb chosen by keyboard or controller (`cycle_limb` / `choose_limb`, #1898).
    let spot = under.and_then(what).and_then(|w| match w {
        StatsTile::Switch => Some(stats::Spot::Switch),
        StatsTile::Sub(i) => Some(stats::Spot::Sub(i)),
        StatsTile::Hint(i) => Some(stats::Spot::Hint(i)),
        StatsTile::Limb(i) => Some(stats::Spot::Limb(i)),
        _ => None,
    });
    if matches!(spot, Some(stats::Spot::Limb(_))) && !pointer_moved && view.hover != spot {
        // Keep the keyboard/controller limb choice until the pointer moves.
    } else if view.hover != spot {
        view.hover = spot;
    }
    let under_row = under.and_then(what).and_then(|w| if let StatsTile::Row(i) = w { Some(i) } else { None });
    let in_sight = tiles.written.geometry.as_ref().map_or(0..usize::MAX, |g| g.top..g.top + g.visible);
    let under_row = row_to_hover(under_row, &in_sight);
    // Only a real change touches the view, which the page's other readers watch.
    match if mobile.control { None } else { follow_hover(&mut state.acted, under_row, settled) } {
        Some(HoverAct::Select(i)) => {
            view.select(tab, i);
            view.highlight = true;
        }
        Some(HoverAct::BoxOff) => view.highlight = false,
        None => {}
    }
    if let Some(i) = keyed_row(ui, &p, rows, &report, &physical) {
        view.select(tab, i);
        view.highlight = true;
    }
    if !held || (state.touch_drag && !mobile.control) {
        state.drag = None;
        state.touch_drag = false;
    }
    let mut pressed_marker = false;
    for c in report.clicks.iter().filter(|c| ui.is_live(c.tile)) {
        if let fnv_ui::input::By::Key(k) = &c.by {
            if !menus::key_click_acts(STATS_MENU, k, &physical) {
                continue;
            }
        }
        let Some(tile) = what(c.tile) else { continue };
        info!("tiles: {STATS_MENU} {tile:?} on its tiles");
        match tile {
            StatsTile::Row(i) => view.select(tab, i),
            StatsTile::Tab(i) => {
                tabs.write(PipBoyInput::Tab(i));
            }
            StatsTile::StepUp => view.scroll_desc(-1),
            StatsTile::StepDown => view.scroll_desc(1),
            StatsTile::PageUp => view.scroll_desc(-page_steps(ui, &p)),
            StatsTile::PageDown => view.scroll_desc(page_steps(ui, &p)),
            StatsTile::Marker => {
                pressed_marker = true;
                state.drag = at.filter(|_| held && matches!(c.by, fnv_ui::input::By::Pointer)).and_then(|(_, y)| marker_grab(ui, &p, BarOf::Description, y));
                state.touch_drag = mobile.control && state.drag.is_some();
            }
            StatsTile::Switch => view.switch(),
            StatsTile::Sub(i) => view.status = i,
            StatsTile::Hint(i) => {
                if let Some(ask) = stats::hint_pressed(&view, i, hardcore) {
                    asks.write(ask);
                }
            }
            StatsTile::SwitchLimbs => view.cycle_limb(),
            StatsTile::Limb(i) => {
                asks.write(stats::UseHint { hint: stats::limb_click_hint(hardcore), limb: Some(i) });
            }
            StatsTile::ListBar(BarPiece::Up) => view.scroll(tab, -1),
            StatsTile::ListBar(BarPiece::Down) => view.scroll(tab, 1),
            StatsTile::ListBar(piece @ (BarPiece::PageUp | BarPiece::PageDown)) => {
                // The page's size is the file's; the model holds the value in its range.
                let first = view.top[tab];
                if let Some(to) = list::after_piece(ui, p.list(), piece, first as f64) {
                    view.scroll(tab, to as i32 - first as i32);
                }
            }
            StatsTile::ListBar(BarPiece::Marker) => {
                pressed_marker = true;
                state.drag = at.filter(|_| held && matches!(c.by, fnv_ui::input::By::Pointer)).and_then(|(_, y)| marker_grab(ui, &p, BarOf::List, y));
                state.touch_drag = mobile.control && state.drag.is_some();
            }
        }
    }
    // A drag: the scroll under the pointer while the press holds.
    if let (false, Some(drag), Some((_, y))) = (pressed_marker, state.drag, at) {
        match drag.of {
            BarOf::Description => {
                if let Some(steps) = steps_under(ui, drag, y, view.desc_bound()).filter(|&s| s != view.desc) {
                    view.desc = steps;
                }
            }
            BarOf::List => {
                if let Some(first) = steps_under(ui, drag, y, view.list_most(tab)).filter(|&s| s != view.top[tab]) {
                    view.top[tab] = first;
                }
            }
        }
    }
}

/// The resource, with the test aid read.
pub(crate) fn setup(app: &mut App) {
    let (cut, cut_until) = std::env::var(CUT_AID).ok().map_or((None, None), |v| {
        let (name, until) = parse_cut(&v);
        (Some(name), until)
    });
    app.insert_resource(StatsTiles { cut, cut_until, ..Default::default() });
}

/// [`CUT_AID`]'s value: the name and the time it stops at.
pub(super) fn parse_cut(v: &str) -> (String, Option<f32>) {
    match v.split_once('@') {
        Some((name, until)) => (name.trim().to_string(), until.trim().parse().ok()),
        None => (v.trim().to_string(), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("stats_menu/tab_swipe_tests.rs");
    use fnv_ui::input::{no_measure, By, Input, Pointer};
    use fnv_ui::MemSource;

    /// The tab the first slice drew, which most of these tests are on.
    const PAGE: usize = 1;

    /// [design] A small layout written for these tests, with its own structure and numbers and
    /// plain literals; it does not come from the game's files and says nothing about them. It
    /// keeps the tile and template names the model and the input look up, which are the engine's
    /// interface ([`paths`]). Its only expressions are a row's place from what the model wrote
    /// and a stand-in for a file's own count of clicks, read from a test tile; a list that
    /// scrolls moves its rows by a value and a step, in an expression of its own. So these tests do
    /// not cover the file's own places, how its expressions read what is written (a row's text
    /// from its `string`, the text's `y` and the marker's from the value, a list's rows from its
    /// bar's value and `_scroll_delta`, which list is shown by the root's `user0`), or whether a
    /// name is where the file has it: the smoke scenarios `tiles-stats-*` check those on the real file.
    /// The Status tiles, at the fixture's own place clear of its lists: this test's own numbers,
    /// and no expressions: so these tests do not cover what the file does with what is written
    /// (a meter's bar and its word from `_Value`, the arrow from the root, which sub-page shows,
    /// the hints by the sub-page): `tiles-stats-status*` is for that.
    fn status_xml() -> String {
        let mut x = String::from(r#"<hotrect name="stats_status_container"><x>700</x><y>220</y><width>200</width><height>112</height>"#);
        for (i, name) in SUB_BUTTONS.iter().enumerate() {
            x += &format!(r#"<hotrect name="{name}"><target>&true;</target><x>{}</x><y>-40</y><width>18</width><height>18</height></hotrect>"#, i * 20);
        }
        for (i, name) in HINT_BUTTONS.iter().enumerate() {
            x += &format!(r#"<hotrect name="{name}"><target>&true;</target><x>{}</x><y>-20</y><width>18</width><height>18</height><text name="button_text"/><text name="PCShortcutLabel"/></hotrect>"#, i * 110);
        }
        for (i, name) in LIMB_TILES.iter().enumerate() {
            x += &format!(r#"<image name="stats_player_{name}"><x>{}</x><y>130</y><image name="stats_player_{name}_pct"><width>18</width><height>18</height><rect name="stats_player_{name}_pct_meter"/></image><text name="stats_player_{name}_crippled"><y>20</y><string>W</string></text></image>"#, i * 20);
        }
        x += r#"<hotrect name="stats_healing_mode"><visible>0</visible><text name="button_text"/></hotrect><image name="stats_player_face"/><text name="stats_player_name"/><text name="stats_rad_resist_level"/><text name="stats_player_rads"/><text name="stats_rads_mid"/><text name="stats_rads_max"/>"#;
        for names in NEEDS {
            for name in names {
                x += &format!(r#"<text name="{name}"/>"#);
            }
        }
        x += r#"<hotrect name="stats_effects_container"><x>0</x><y>0</y><width>180</width></hotrect><text name="stats_no_effects"><string>Empty</string></text></hotrect>"#;
        x
    }

    const FIXTURE: &str = r#"<menu name="StatsMenu"><rect name="Branch">
      <template name="stats_effects_template"><hotrect name="TestEffect"><height>40</height>
        <y><add src="me()" trait="_y"/></y>
        <text name="stats_effect_name"/><text name="stats_effect_string"><x>60</x></text>
      </hotrect></template>
      <rect name="StatusHere"/>
      <template name="stats_list_template"><hotrect name="TestRow"><target>&true;</target><x>8</x><width>90</width>
        <_VerticalSpacing>6</_VerticalSpacing>
        <y><add src="me()" trait="_y"/><sub src="parent()" trait="_offset"/></y>
        <text name="ListItemText"><string>Row</string></text>
      </hotrect></template>
      <image name="background_layout"/>
      <text name="stats_title"><string>Words</string></text>
      <rect name="stats_headline_h2"><x>300</x><_length>40</_length></rect>
      <hotrect name="stats_special_container"><x>12</x><y>30</y><width>200</width><clipwindow>&true;</clipwindow><_number_of_visible_items>8</_number_of_visible_items><_offset>0</_offset></hotrect>
      <hotrect name="stats_skills_container"><x>12</x><y>30</y><width>200</width><clipwindow>&true;</clipwindow><_number_of_visible_items>9</_number_of_visible_items><_scroll_delta>7</_scroll_delta>
        <_offset><copy src="child(lb_scrollbar)" trait="_current_value"/><mul src="me()" trait="_scroll_delta"/></_offset>
        <image name="lb_scrollbar"><x>170</x><_step_size>1</_step_size><_jump_size>3</_jump_size>
          <hotrect name="scrollbar_vert_up"><target>&true;</target><width>12</width><height>12</height></hotrect>
          <hotrect name="scrollbar_vert_page_up"><target>&true;</target><y>12</y><width>12</width><height>20</height></hotrect>
          <hotrect name="scrollbar_vert_page_down"><target>&true;</target><y>32</y><width>12</width><height>20</height></hotrect>
          <hotrect name="scrollbar_vert_down"><target>&true;</target><y>52</y><width>12</width><height>12</height></hotrect>
          <image name="scrollbar_vert_marker"><target>&true;</target><x>14</x><width>6</width><height>10</height><_step_height>11</_step_height></image>
        </image>
      </hotrect>
      <hotrect name="stats_perks_container"><x>12</x><y>30</y><width>200</width><clipwindow>&true;</clipwindow><_number_of_visible_items>9</_number_of_visible_items><_offset>0</_offset></hotrect>
      <hotrect name="stats_genrep_container"><x>20</x><y>50</y><user0>1</user0>
        <hotrect name="stats_general_container"><visible><copy>1</copy><sub src="parent()" trait="user0"/></visible><width>300</width><clipwindow>&true;</clipwindow><_number_of_visible_items>6</_number_of_visible_items><_scroll_delta>9</_scroll_delta>
          <_offset><copy src="child(lb_scrollbar)" trait="_current_value"/><mul src="me()" trait="_scroll_delta"/></_offset>
          <image name="lb_scrollbar"><x>280</x><_step_size>1</_step_size><_jump_size>2</_jump_size>
            <hotrect name="scrollbar_vert_down"><target>&true;</target><width>12</width><height>12</height></hotrect>
          </image>
        </hotrect>
        <rect name="stats_karma_rect"><visible><copy>1</copy><sub src="parent()" trait="user0"/></visible><x>330</x><text name="stats_karma_alignment"/><image name="stats_karma_image"/><text name="stats_karma_title"/></rect>
        <hotrect name="stats_reputation_container"><visible><copy src="parent()" trait="user0"/></visible><width>300</width><clipwindow>&true;</clipwindow><_number_of_visible_items>6</_number_of_visible_items><_offset>0</_offset></hotrect>
        <rect name="stats_reptitle_rect"><visible><copy src="parent()" trait="user0"/></visible><x>330</x><image name="stats_reputation_image"/><text name="stats_reputation_title"/><text name="stats_reputation_rep_title"/></rect>
        <hotrect name="stats_genrep_button"><target>&true;</target><x>500</x><width>60</width><height>30</height><text name="button_text"><string>&-sLeftOpen</string></text></hotrect>
      </hotrect>
      <rect name="stats_display_rect"><x>400</x><y>34</y>
        <image name="stats_icon"/><image name="stats_icon_badge"><filename>pics\\none.dds</filename></image>
        <hotrect name="stats_description_rect"><y>100</y><width>200</width><height>60</height>
          <text name="stats_description"/>
          <rect name="Pulse"><user3>0</user3></rect>
          <hotrect name="stats_description_scrollbar"><x>210</x>
            <_current_value><copy src="sibling(Pulse)" trait="user3"/></_current_value>
            <hotrect name="scrollbar_vert_up"><target>&true;</target><width>12</width><height>12</height></hotrect>
            <hotrect name="scrollbar_vert_page_up"><target>&true;</target><y>12</y><width>12</width><height>8</height></hotrect>
            <hotrect name="scrollbar_vert_page_down"><target>&true;</target><y>20</y><width>12</width><height>8</height></hotrect>
            <hotrect name="scrollbar_vert_down"><target>&true;</target><y>40</y><width>12</width><height>12</height></hotrect>
            <image name="scrollbar_vert_marker"><target>&true;</target><x>20</x><width>12</width><height>6</height><_step_height>3</_step_height></image>
          </hotrect>
        </hotrect>
      </rect>
      <hotrect name="stats_tailline_status"><target>&true;</target><x>10</x><y>500</y><width>50</width><height>20</height><text name="TabWords"><string>Tab</string></text></hotrect>
      <hotrect name="stats_tailline_SPECIAL"><target>&true;</target><x>70</x><y>500</y><width>50</width><height>20</height><text name="TabWords"><string>Tab</string></text></hotrect>
      <hotrect name="stats_tailline_skills"><target>&true;</target><x>130</x><y>500</y><width>50</width><height>20</height><text name="TabWords"><string>Tab</string></text></hotrect>
      <hotrect name="stats_tailline_perks"><target>&true;</target><x>190</x><y>500</y><width>50</width><height>20</height><text name="TabWords"><string>Tab</string></text></hotrect>
      <hotrect name="stats_tailline_general"><target>&true;</target><x>250</x><y>500</y><width>50</width><height>20</height><text name="TabWords"><string>Tab</string></text></hotrect>
    </rect></menu>"#;

    /// Each row's height in the fixture: its text's 30 and its own spacing of 6.
    const ROW: f64 = 36.0;

    fn tree_changed(change: impl FnOnce(&str) -> String) -> Ui {
        let src = MemSource::default().with("s.xml", &change(&FIXTURE.replace(r#"<rect name="StatusHere"/>"#, &status_xml())));
        let mut ui = Ui::new();
        ui.load_menu(&src, "s.xml").unwrap();
        ui.update();
        ui
    }

    fn tree() -> Ui {
        tree_changed(str::to_string)
    }

    fn rows() -> Vec<Row> {
        (0..7).map(|i| Row { key: RowKey::Av(i), name: format!("Stat {i}"), value: Some((i + 1).to_string()), mark: None, description: "one|two".into(), icon: Some(format!("pics\\{i}.dds")), badge: None, bold: false }).collect()
    }

    fn skills() -> Vec<Row> {
        (0..13).map(|i| Row { key: RowKey::Av(100 + i), name: format!("Skill {i}"), value: Some((i + 1).to_string()), description: "one".into(), badge: (i == 3).then(|| "pics\\mark.dds".to_string()), ..Row::default() }).collect()
    }

    /// A perk of ranks, and one of none.
    fn perks() -> Vec<Row> {
        vec![
            Row { key: RowKey::Perk(fnv_sim::FormId(0x20)), name: "Perk A".into(), value: Some("2".into()), description: "about a".into(), icon: Some("pics\\a.dds".into()), ..Row::default() },
            Row { key: RowKey::Perk(fnv_sim::FormId(0x10)), name: "Perk B".into(), description: "about b".into(), ..Row::default() },
        ]
    }

    /// A view on General's statistics, `rows` long.
    fn general_view(rows: usize) -> StatsView {
        let mut v = StatsView::with_lens([0, 7, 13, 2, rows]);
        v.reputation = false;
        v
    }

    /// Rows and a side of this test's own for General.
    fn general_snap() -> Snapshot {
        Snapshot {
            general: (0..20).map(|i| Row { key: RowKey::Stat(i), name: format!("Count {i}"), value: Some(i.to_string()), ..Row::default() }).collect(),
            standings: vec![crate::pipboy::stats::model::Standing { form: fnv_sim::FormId(0x40), name: "Town".into(), title: "A title", fame: 1.0, infamy: 0.0, icon: Some("pics\\town.dds".into()) }],
            karma: Some((5.0, "A word", "a_picture")),
            karma_title: Some("From the table".into()),
            karma_setting_title: Some("From a setting".into()),
            ..snap()
        }
    }

    fn snap() -> Snapshot {
        Snapshot { level: Some(3), hp: Some((20.0, 40.0)), ap: Some((5.0, 6.0)), xp: Some((7.0, 8.0)), special: rows(), skills: skills(), perks: perks(), ..Default::default() }
    }

    fn view() -> StatsView {
        StatsView::with_lens([0, 7, 13, 2, 0])
    }

    /// A measure of 10 a character and 30 a line, lines parted by `|`.
    fn measure(_: f64, text: &str, _: Option<f64>) -> (f64, f64) {
        (10.0 * text.chars().count() as f64, 30.0 * text.split('|').count() as f64)
    }

    struct TestFonts;

    impl PageFonts for TestFonts {
        fn picture_texels(&mut self, name: &str) -> Option<(f64, f64)> {
            (name != "pics\\nofile.dds").then_some((200.0, 100.0))
        }
        fn write_sizes(&mut self, ui: &mut Ui, texts: &[TileId], written: &mut SizesWritten) -> usize {
            menus::write_text_sizes_of(ui, texts, written, &measure)
        }
        fn set_out(&mut self, ui: &Ui, menu: TileId) -> Vec<Piece> {
            super::super::draw::pieces(ui, menu, &measure, super::super::draw::Select::Whole { kept: &[], strip: None }, true).0
        }
    }

    fn run(ui: &mut Ui, snap: &Snapshot, view: &mut StatsView, device: bool, tiles: &mut StatsTiles) -> Result<TileId, String> {
        page(ui, snap, view, PAGE, device, true, tiles, &mut TestFonts)
    }

    fn run_tab(ui: &mut Ui, snap: &Snapshot, view: &mut StatsView, tab: usize, tiles: &mut StatsTiles) -> Result<TileId, String> {
        page(ui, snap, view, tab, true, true, tiles, &mut TestFonts)
    }

    /// The rows under a list, leaving out its own pieces.
    fn rows_under(ui: &Ui, list: TileId) -> usize {
        ui.tile(list).children.iter().filter(|&&c| ui.is_live(c) && ui.tile(c).get("listindex").is_some()).count()
    }

    /// The menu's list `name`.
    fn list_named(ui: &Ui, name: &str) -> TileId {
        find_named(ui, ui.menu(STATS_MENU).unwrap(), name).unwrap()
    }

    fn text(ui: &Ui, t: TileId, name: &str) -> String {
        ui.tile(t).get(name).map(|v| v.text().to_string()).unwrap_or_default()
    }

    fn live_children(ui: &Ui, t: TileId) -> usize {
        ui.tile(t).children.iter().filter(|&&c| ui.is_live(c)).count()
    }

    #[test]
    fn the_model_writes_the_figures_and_seven_rows() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        let menu = run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        assert_eq!(num(&ui, menu, "user0"), 1.0, "the page");
        assert_eq!(text(&ui, menu, "user8"), "3", "the level");
        assert_eq!(text(&ui, menu, "user5"), "20/40");
        assert_eq!(text(&ui, menu, "user6"), "5/6");
        assert_eq!(text(&ui, menu, "user7"), "7/8");
        let p = paths(&ui, PAGE, None).unwrap();
        let made = t.written.rows.clone();
        assert_eq!((made.len(), live_children(&ui, p.list())), (7, 7));
        assert_eq!(text(&ui, made[2].tile, "string"), "Stat 2");
        assert_eq!(ui.tile(made[2].tile).name, "Stat 2", "a row's tile has its name");
        assert_eq!(num(&ui, made[2].tile, "id"), -1.0);
        assert_eq!(text(&ui, made[2].tile, "user1"), "3");
        assert_eq!(num(&ui, made[2].tile, "listindex"), 2.0);
        assert_eq!(num(&ui, made[2].tile, "height"), ROW);
        assert_eq!(num(&ui, made[2].tile, "_y"), 2.0 * ROW);
        assert_eq!(num(&ui, p.list(), "height"), 8.0 * ROW, "the file's visible rows, not a hand-built height");
        assert_eq!((num(&ui, p.list(), "_highlight_y"), num(&ui, p.list(), "_selected_height")), (-1.0, 0.0), "no box until a key or the pointer");
        assert_eq!(num(&ui, p.list(), "user0"), 0.0);
        assert_eq!(text(&ui, p.icon, "filename"), "pics\\0.dds");
        assert_eq!(text(&ui, p.desc, "string"), "one|two");
    }

    #[test]
    fn a_placeholder_stays_text_and_no_value_hides_the_number() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        let mut s = snap();
        s.special[0].value = Some("--".into());
        s.special[1].value = None;
        s.special[2].mark = Some("(+)".into());
        run(&mut ui, &s, &mut v, true, &mut t).unwrap();
        let made = t.written.rows.clone();
        assert_eq!(text(&ui, made[0].tile, "user1"), "--", "a numeric write would lose it");
        assert_eq!(num(&ui, made[1].tile, "user1"), -1.0);
        assert_eq!((num(&ui, made[2].tile, "user0"), text(&ui, made[2].tile, "user2")), (1.0, "(+)".to_string()));
        assert_eq!(num(&ui, made[0].tile, "user0"), 0.0);
        let p = paths(&ui, PAGE, None).unwrap();
        assert_eq!(num(&ui, p.list(), "user0"), 1.0, "the list knows a row is marked");
    }

    #[test]
    fn the_box_follows_the_selection_only_while_highlighted() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        v.select(PAGE, 3);
        v.highlight = true;
        run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        let p = paths(&ui, PAGE, None).unwrap();
        assert_eq!((num(&ui, p.list(), "_highlight_y"), num(&ui, p.list(), "_selected_height")), (3.0 * ROW, ROW));
        assert_eq!(text(&ui, p.icon, "filename"), "pics\\3.dds");
        v.highlight = false;
        run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        assert_eq!(num(&ui, p.list(), "_highlight_y"), -1.0);
        assert_eq!(text(&ui, p.icon, "filename"), "pics\\3.dds", "the display stays filled without the box");
    }

    #[test]
    fn a_row_without_a_picture_keeps_the_display() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        let mut s = snap();
        s.special[0].icon = None;
        run(&mut ui, &s, &mut v, true, &mut t).unwrap();
        let p = paths(&ui, PAGE, None).unwrap();
        assert_eq!(text(&ui, p.icon, "filename"), "");
        assert_eq!(num(&ui, p.icon, "visible"), 1.0, "what follows it in the file stays shown");
        assert_eq!(text(&ui, p.desc, "string"), "one|two");
    }

    #[test]
    fn the_description_scrolls_by_the_tiles_own_measure_and_the_value_is_the_models() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        let mut s = snap();
        // 150 high in a window of 60, 20 a step: 5 steps.
        s.special[0].description = "a|b|c|d|e".into();
        run(&mut ui, &s, &mut v, true, &mut t).unwrap();
        let p = paths(&ui, PAGE, None).unwrap();
        assert_eq!(v.desc_bound(), 5);
        assert_eq!(num(&ui, p.bar, "_number_of_items"), 6.0);
        v.scroll_desc(2);
        run(&mut ui, &s, &mut v, true, &mut t).unwrap();
        assert_eq!(num(&ui, p.bar, "_current_value"), 2.0);
        // A pulse in the value's own source: the model's value outlasts it and its end.
        let pulse = find_named(&ui, p.desc_rect, "Pulse").unwrap();
        for count in [1.0, 0.0] {
            ui.set_trait(pulse, "user3", Value::Num(count));
            run(&mut ui, &s, &mut v, true, &mut t).unwrap();
            assert_eq!(num(&ui, p.bar, "_current_value"), 2.0);
        }
        // A short text: nothing to scroll, the scroll pulled back, the bar's count at 1.
        run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        assert_eq!((v.desc_bound(), v.desc), (0, 0));
        assert_eq!(num(&ui, p.bar, "_number_of_items"), 1.0);
    }

    #[test]
    fn passes_and_reopenings_keep_the_rows_and_a_new_tree_makes_its_own() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        let made = t.written.rows.clone();
        for _ in 0..3 {
            run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        }
        assert_eq!(t.written.rows, made);
        let p = paths(&ui, PAGE, None).unwrap();
        assert_eq!(live_children(&ui, p.list()), 7);
        // The tree built again.
        let mut again = tree();
        run(&mut again, &snap(), &mut v, true, &mut t).unwrap();
        let p = paths(&again, PAGE, None).unwrap();
        assert_eq!(live_children(&again, p.list()), 7);
        assert!(t.written.rows.iter().all(|r| again.is_live(r.tile)));
    }

    /// A middle row taken away: the others keep their tiles and places, and the new one has
    /// that place and everything written to it.
    #[test]
    fn a_row_taken_away_is_made_again_in_its_own_place() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        let before = t.written.rows.clone();
        ui.unload_tile(before[3].tile);
        ui.update();
        run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        let after = t.written.rows.clone();
        assert_eq!(after.len(), 7);
        for i in [0, 1, 2, 4, 5, 6] {
            assert_eq!(after[i], before[i], "row {i} stays itself");
        }
        assert_ne!(after[3].tile, before[3].tile);
        assert_eq!(after[3].key, before[3].key);
        assert_eq!(text(&ui, after[3].tile, "string"), "Stat 3");
        assert_eq!(text(&ui, after[3].tile, "user1"), "4");
        assert_eq!(num(&ui, after[3].tile, "listindex"), 3.0);
        assert_eq!(num(&ui, after[3].tile, "height"), ROW);
        assert_eq!(num(&ui, after[3].tile, "_y"), 3.0 * ROW);
    }

    /// A row that lost its text is unloaded whole, leaving no root without its parts.
    #[test]
    fn a_row_without_its_text_is_made_again_whole() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        let before = t.written.rows.clone();
        ui.unload_tile(before[2].text);
        ui.update();
        run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        let list = paths(&ui, PAGE, None).unwrap().list();
        assert_eq!(live_children(&ui, list), 7);
        assert!(!ui.is_live(before[2].tile));
        assert!(ui.is_live(t.written.rows[2].text));
    }

    /// Whatever a pass finds wrong, the reason, no row left and none doubled on the retry.
    fn falls_back(change: impl FnOnce(&str) -> String, reason: &str) {
        let (mut ui, mut v, mut t) = (tree_changed(change), view(), StatsTiles::default());
        for _ in 0..2 {
            let got = run(&mut ui, &snap(), &mut v, true, &mut t).unwrap_err();
            assert!(got.contains(reason), "{got}");
            let list = ui.menu(STATS_MENU).and_then(|m| find_named(&ui, m, LISTS[0]));
            assert!(list.is_none_or(|l| live_children(&ui, l) == 0 || reason.contains("rows")), "{reason}: rows left");
        }
    }

    #[test]
    fn a_cut_down_file_is_refused_with_its_reason() {
        falls_back(|x| x.replace(r#"template name="stats_list_template""#, r#"template name="other""#), "stats_list_template missing");
        falls_back(|x| x.replace("ListItemText", "Other"), "has no ListItemText");
        falls_back(|x| x.replace("stats_display_rect", "other"), "stats_display_rect missing");
        falls_back(|x| x.replace("stats_tailline_perks", "other"), "stats_tailline_perks missing");
        falls_back(|x| x.replace("stats_special_container", "other"), "stats_special_container missing");
        falls_back(|x| x.replace(r#"menu name="StatsMenu""#, r#"menu name="Other""#), "StatsMenu is not in the tree");
    }

    /// [design] A list longer than it shows, in the fixture's own numbers: the model's first row
    /// is the bar's value, the row's height the step, and the list stands at the height of the
    /// rows it shows.
    #[test]
    fn a_list_longer_than_it_shows_scrolls_by_the_models_first_row() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        let menu = run_tab(&mut ui, &snap(), &mut v, 2, &mut t).unwrap();
        assert_eq!(num(&ui, menu, "user0"), 2.0, "the tab");
        let p = paths(&ui, 2, None).unwrap();
        let list = list_named(&ui, "stats_skills_container");
        assert_eq!(p.list(), list);
        let made = t.written.rows.clone();
        assert_eq!((made.len(), rows_under(&ui, list)), (13, 13));
        assert_eq!(ui.tile(made[3].tile).name, "Skill 3");
        assert_eq!(num(&ui, made[12].tile, "_y"), 12.0 * ROW);
        assert_eq!(num(&ui, list, "height"), 9.0 * ROW);
        assert_eq!(num(&ui, list, "_scroll_delta"), ROW);
        let bar = list::child(&ui, list, list::SCROLLBAR).unwrap();
        assert_eq!((num(&ui, bar, "_number_of_items"), num(&ui, bar, "_number_of_visible_items")), (5.0, 1.0));
        assert_eq!(list::value_shown(&ui, list), Some(0.0));
        assert_eq!(v.list_most(2), 4, "the model's range, from the file's count");
        // Two rows down: the value is the model's, the rows move by the file, and what is under
        // the pointer goes with them.
        v.scroll(2, 2);
        run_tab(&mut ui, &snap(), &mut v, 2, &mut t).unwrap();
        assert_eq!(list::value_shown(&ui, list), Some(2.0));
        assert_eq!(t.written.geometry.as_ref().map(|g| (g.top, g.visible)), Some((2, 9)));
        let at_top = fnv_ui::input::hit(&ui, &[menu], (50.0, 30.0 + 0.5 * ROW), &no_measure).unwrap();
        assert_eq!(stats_tile(&ui, &p, &t.written.rows, at_top.tile), Some(StatsTile::Row(2)));
        // A row scrolled out takes no pointer past the list's window, and is not asked to.
        let past = fnv_ui::input::hit(&ui, &[menu], (50.0, 30.0 + 9.5 * ROW), &no_measure);
        assert_eq!(past.and_then(|h| stats_tile(&ui, &p, &t.written.rows, h.tile)), None);
        // The file's count of the rows, not the hand-built nine.
        let (mut ui, mut v, mut t) = (tree_changed(|x| x.replace("<_number_of_visible_items>9<", "<_number_of_visible_items>11<")), view(), StatsTiles::default());
        run_tab(&mut ui, &snap(), &mut v, 2, &mut t).unwrap();
        assert_eq!(v.list_most(2), 2);
        // The tab with the file's count too small for its rows, the first slice's refusal gone.
        let (mut ui, mut v, mut t) = (tree_changed(|x| x.replace("<_number_of_visible_items>8<", "<_number_of_visible_items>5<")), view(), StatsTiles::default());
        run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        assert_eq!(v.list_most(PAGE), 2);
    }

    #[test]
    fn the_box_stands_where_its_row_is_scrolled_to_and_goes_with_it() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        v.highlight = true;
        v.select(2, 3);
        v.scroll(2, 2);
        run_tab(&mut ui, &snap(), &mut v, 2, &mut t).unwrap();
        let list = list_named(&ui, "stats_skills_container");
        assert_eq!((num(&ui, list, "_highlight_y"), num(&ui, list, "_selected_height")), (ROW, ROW));
        // Row 3 out of sight, the selection staying on it: no box, the display still its own.
        v.scroll(2, 2);
        run_tab(&mut ui, &snap(), &mut v, 2, &mut t).unwrap();
        assert_eq!((num(&ui, list, "_highlight_y"), num(&ui, list, "_selected_height")), (-1.0, 0.0));
        assert_eq!(v.selected[2], 3);
    }

    #[test]
    fn a_list_that_stops_scrolling_gets_the_files_step_back() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        run_tab(&mut ui, &snap(), &mut v, 2, &mut t).unwrap();
        let list = list_named(&ui, "stats_skills_container");
        let bar = list::child(&ui, list, list::SCROLLBAR).unwrap();
        let mut s = snap();
        s.skills.truncate(5);
        run_tab(&mut ui, &s, &mut v, 2, &mut t).unwrap();
        assert_eq!(num(&ui, list, "_scroll_delta"), 7.0);
        assert_eq!((num(&ui, bar, "_number_of_items"), list::value_shown(&ui, list)), (1.0, Some(0.0)));
        assert_eq!(rows_under(&ui, list), 5);
        assert!(t.written.counted.is_empty());
        // A list that was never counted is not written.
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        run_tab(&mut ui, &s, &mut v, 2, &mut t).unwrap();
        let bar = list::child(&ui, list_named(&ui, "stats_skills_container"), list::SCROLLBAR).unwrap();
        assert!(ui.tile(bar).get("_number_of_items").is_none());
    }

    /// [design] Pictures and a count-driven visibility rule written for this test (#1512),
    /// with independent sizes and alpha values; no layout or expression from a menu file.
    #[test]
    fn list_bar_pictures_keep_their_alpha_and_hide_when_the_rows_fit() {
        const BAR_PICTURES: &str = r#"<image name="lb_scrollbar"><alpha>0</alpha><x>150</x><width>8</width><height>70</height>
          <_number_of_items>1</_number_of_items><_current_value>0</_current_value>
          <visible><copy src="me()" trait="_number_of_items"/><gt>1</gt></visible>
          <image name="scrollbar_vert_up"><filename>test_up.dds</filename><y>3</y><width>8</width><height>5</height><alpha>210</alpha></image>
          <image name="scrollbar_vert_down"><filename>test_down.dds</filename><y>65</y><width>8</width><height>5</height><alpha>180</alpha></image>
          <image name="scrollbar_vert_marker"><filename>test_marker.dds</filename><y>20</y><width>3</width><height>9</height><alpha>150</alpha></image>
        </image>"#;
        for tab in [2, GENERAL_TAB] {
            let (mut ui, mut s, mut t) = (tree(), general_snap(), StatsTiles::default());
            let mut v = if tab == GENERAL_TAB { general_view(s.general.len()) } else { view() };
            let list = paths(&ui, tab, None).unwrap().in_mode(false).list();
            ui.unload_tile(list::child(&ui, list, list::SCROLLBAR).unwrap());
            assert!(ui.inject_text(&MemSource::default(), &ui.path_of(list), BAR_PICTURES));
            let bar = list::child(&ui, list, list::SCROLLBAR).unwrap();
            let pictures = list_bar_pictures(&ui, list).map(|p| p.unwrap());
            ui.set_trait(bar, "_number_of_items", Value::Num(2.0));
            ui.update();
            let before = TestFonts.set_out(&ui, ui.menu(STATS_MENU).unwrap());
            assert!(before.iter().all(|p| !pictures.contains(&p.tile)), "unmarked pictures inherit zero alpha");

            run_tab(&mut ui, &s, &mut v, tab, &mut t).unwrap();
            let painted = |tiles: &StatsTiles| pictures.map(|id| tiles.pieces.iter().find(|p| p.tile == id).map(|p| p.colour[3]));
            let alphas = [Some(210.0 / 255.0), Some(180.0 / 255.0), Some(150.0 / 255.0)];
            assert_eq!(painted(&t), alphas, "all three pictures reach the painter at their own alpha");
            assert_eq!(num(&ui, bar, "alpha"), 0.0);
            for picture in pictures {
                assert_eq!(num(&ui, picture, layout::OWN_ALPHA), 1.0);
                let piece = t.pieces.iter().find(|p| p.tile == picture).unwrap();
                assert!(matches!(piece.look, Look::Picture { .. }) && piece.rect.width > 0.0 && piece.rect.height > 0.0);
            }
            v.scroll(tab, 1);
            run_tab(&mut ui, &s, &mut v, tab, &mut t).unwrap();
            assert_eq!(list::value_shown(&ui, list), Some(1.0));
            assert_eq!(painted(&t), alphas, "scrolling keeps the pictures in the paint set");
            ui.set_trait(pictures[2], "alpha", Value::Num(0.0));
            run_tab(&mut ui, &s, &mut v, tab, &mut t).unwrap();
            assert_eq!(painted(&t), [alphas[0], alphas[1], None], "a picture's own zero alpha still hides it");
            ui.set_trait(pictures[2], "alpha", Value::Num(150.0));
            s.skills.truncate(3);
            s.general.truncate(3);
            run_tab(&mut ui, &s, &mut v, tab, &mut t).unwrap();
            assert_eq!(num(&ui, bar, "_number_of_items"), 1.0);
            assert_eq!(num(&ui, bar, "visible"), 0.0);
            assert_eq!(painted(&t), [None; 3], "the bar's visibility still hides all three pictures");
        }
    }

    /// A name that wraps makes one row taller: a scrolling list uses `list::counts` across the
    /// variable row heights instead of falling back to the hand-built page (e.g. modded Perks),
    /// and a list that fits also stays on tiles.
    #[test]
    fn rows_of_different_heights_scroll_on_tiles() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        run_tab(&mut ui, &snap(), &mut v, 2, &mut t).unwrap();
        let tall = t.written.rows[3].text;
        ui.set_trait(tall, "string", Value::Str("two|lines".into()));
        ui.update();
        run_tab(&mut ui, &snap(), &mut v, 2, &mut t).expect("scrolling list with wrapped row stays on tiles");
        let counts = t.written.geometry.as_ref().and_then(|g| g.counts).expect("variable-height list scrolls");
        assert!(counts.most() >= 4.0);
        // Scrolling to the end keeps the last row in view without falling back.
        v.scroll(2, counts.most() as i32);
        run_tab(&mut ui, &snap(), &mut v, 2, &mut t).expect("scrolling to end stays on tiles");
        assert!(t.written.geometry.as_ref().unwrap().shown.contains(&12));
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        let tall = t.written.rows[3].text;
        ui.set_trait(tall, "string", Value::Str("two|lines".into()));
        ui.update();
        assert!(run(&mut ui, &snap(), &mut v, true, &mut t).is_ok());
    }

    /// Modded `stats_effects_template` (e.g. Vanilla UI Plus `stats_menu.xml`) binds
    /// `stats_effect_name` and `stats_effect_string` to `parent/user0` and `parent/user1`.
    #[test]
    fn effects_template_bound_to_user0_and_user1_renders_on_tiles() {
        let modded = |x: &str| {
            x.replace(
                r#"<text name="stats_effect_name"/><text name="stats_effect_string"><x>60</x></text>"#,
                r#"<user0></user0><user1></user1><text name="stats_effect_name"><string><copy src="parent()" trait="user0"/></string></text><text name="stats_effect_string"><x>60</x><string><copy src="parent()" trait="user1"/></string></text>"#,
            )
        };
        let (mut ui, mut v, mut t) = (tree_changed(modded), view(), status_tiles());
        let mut s = status_snap();
        s.effects = vec![crate::pipboy::stats::model::EffectLine { name: "Well Rested".into(), text: "+10% XP".into() }];
        v.status = 2;
        run_tab(&mut ui, &s, &mut v, STATUS_TAB, &mut t).expect("VUI+ EFF rows stay on tiles");
        assert!(t.pieces.iter().any(|p| matches!(&p.look, Look::Text { string, .. } if string == "Well Rested")));
        assert!(t.pieces.iter().any(|p| matches!(&p.look, Look::Text { string, .. } if string == "+10% XP")));
    }

    /// Modded 13-row Skills list (e.g. Vanilla UI Plus) sized to fit all 13 rows (`13 * row_h`)
    /// with a vertical font-centering offset (`_VUI+FontY_2`) stays on tiles instead of failing
    /// on row 12's bottom leading margin, while a genuinely clipped or off-canvas row still fails.
    #[test]
    fn tightly_packed_skills_list_with_font_y_offset_fits_last_row_on_tiles() {
        // 13 skills × (text 30 + spacing 8.5 = 38.5) = 500.5 exact list height, with text y = 14
        // (matching VUI+ `_VUI+FontY_2 + _VerticalSpacing/2 - 1`), so row 12's tile fits [462, 500.5]
        // while ListItemText's box ends at 506.0 (5.5 units of bottom leading slack).
        // Shift fixture tabs from y=500 to y=560 so the taller 500.5px list doesn't overlap them.
        let tight = |x: &str| {
            x.replace("<y>500</y>", "<y>560</y>")
                .replace(
                    r#"<hotrect name="stats_skills_container"><x>12</x><y>30</y><width>200</width><clipwindow>&true;</clipwindow><_number_of_visible_items>9</_number_of_visible_items>"#,
                    r#"<hotrect name="stats_skills_container"><x>12</x><y>30</y><width>200</width><height>500.5</height><clipwindow>&true;</clipwindow><_VerticalSpacing>8.5</_VerticalSpacing>"#,
                )
                .replace(
                    r#"<_VerticalSpacing>6</_VerticalSpacing>"#,
                    r#"<clips>&true;</clips><_VerticalSpacing><copy src="parent()" trait="_VerticalSpacing"/></_VerticalSpacing>"#,
                )
                .replace(
                    r#"<text name="ListItemText"><string>Row</string></text>"#,
                    r#"<text name="ListItemText"><y>14</y><string>Row</string></text>"#,
                )
        };
        let (mut ui, mut v, mut t) = (tree_changed(tight), view(), StatsTiles::default());
        run_tab(&mut ui, &snap(), &mut v, 2, &mut t).expect("13-row VUI+ Skills list must stay on tiles");
        assert_eq!(t.written.rows.len(), 13);
        assert_eq!(t.written.geometry.as_ref().unwrap().shown, 0..13);
    }

    /// Each list has its own rows, unloaded when another is shown, as a new tree would.
    #[test]
    fn going_to_another_list_makes_its_rows_and_takes_the_others_away() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        let (special, skills) = (list_named(&ui, "stats_special_container"), list_named(&ui, "stats_skills_container"));
        let first = t.written.rows[0].tile;
        run_tab(&mut ui, &snap(), &mut v, 2, &mut t).unwrap();
        assert_eq!((rows_under(&ui, special), rows_under(&ui, skills)), (0, 13));
        assert!(!ui.is_live(first));
        assert_eq!(t.written.paths.map(|p| p.page), Some(2));
        let menu = run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        assert_eq!((rows_under(&ui, special), rows_under(&ui, skills)), (7, 0));
        assert_eq!(num(&ui, menu, "user0"), 1.0);
        // One list's cut leaves the other tab alone.
        t.cut = Some("stats_skills_container".into());
        t.written.paths = None;
        assert!(run(&mut ui, &snap(), &mut v, true, &mut t).is_ok());
        assert_eq!(run_tab(&mut ui, &snap(), &mut v, 2, &mut t).unwrap_err(), "stats_skills_container missing");
        t.cut = None;
        assert!(run_tab(&mut ui, &snap(), &mut v, 2, &mut t).is_ok(), "a failed pass is tried again");
    }

    /// Rows whose order the model changes keep their tiles, and take their new places.
    #[test]
    fn the_rows_stand_in_the_models_order_not_by_their_keys() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        let mut s = snap();
        s.skills.reverse();
        run_tab(&mut ui, &s, &mut v, 2, &mut t).unwrap();
        assert_eq!(ui.tile(t.written.rows[0].tile).name, "Skill 12", "as the model has them");
        assert_eq!(num(&ui, t.written.rows[12].tile, "_y"), 12.0 * ROW);
        let moved = t.written.rows[0].tile;
        s.skills.swap(0, 1);
        run_tab(&mut ui, &s, &mut v, 2, &mut t).unwrap();
        assert_eq!(t.written.rows[1].tile, moved, "kept");
        assert_eq!(num(&ui, moved, "listindex"), 1.0);
        assert_eq!(num(&ui, moved, "_y"), ROW);
    }

    /// Perks are known by their records and stand as the model has them, whatever their
    /// numbers; a rank is the value, none hides it; the list has no mark to speak of.
    #[test]
    fn a_perk_row_shows_its_rank_or_none() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        let menu = run_tab(&mut ui, &snap(), &mut v, 3, &mut t).unwrap();
        assert_eq!(num(&ui, menu, "user0"), 3.0, "the tab");
        let list = list_named(&ui, "stats_perks_container");
        let made = t.written.rows.clone();
        assert_eq!(rows_under(&ui, list), 2);
        assert_eq!(made[0].key, RowKey::Perk(fnv_sim::FormId(0x20)));
        assert_eq!((text(&ui, made[0].tile, "string"), num(&ui, made[0].tile, "user1")), ("Perk A (2)".to_string(), -1.0), "a rank in its words, the number hidden");
        assert_eq!((text(&ui, made[1].tile, "string"), num(&ui, made[1].tile, "user1")), ("Perk B".to_string(), -1.0));
        assert_eq!(text(&ui, made[1].tile, "user2"), "");
        assert!(ui.tile(list).get("user0").is_none(), "this list has no such trait to write");
        let p = paths(&ui, 3, None).unwrap();
        assert_eq!(text(&ui, p.icon, "filename"), "pics\\a.dds");
        assert_eq!(text(&ui, p.desc, "string"), "about a");
        // A perk gained keeps the others' tiles, in the model's order.
        let mut s = snap();
        s.perks.insert(1, Row { key: RowKey::Perk(fnv_sim::FormId(0x30)), name: "Perk Aa".into(), ..Row::default() });
        run_tab(&mut ui, &s, &mut v, 3, &mut t).unwrap();
        assert_eq!(t.written.rows[0], made[0]);
        assert_eq!(t.written.rows[2], made[1]);
        assert_eq!(num(&ui, made[1].tile, "listindex"), 2.0);
        assert_eq!(num(&ui, made[1].tile, "_y"), 2.0 * ROW);
    }

    /// A player with no perks: the page stands with no rows and no display, its tabs at work;
    /// the first perk gained is made in place.
    #[test]
    fn an_empty_list_is_a_page_of_its_own() {
        let (mut ui, mut v, mut t) = (tree(), StatsView::with_lens([0, 7, 13, 0, 0]), StatsTiles::default());
        let mut s = snap();
        s.perks.clear();
        let menu = run_tab(&mut ui, &s, &mut v, 3, &mut t).unwrap();
        let list = list_named(&ui, "stats_perks_container");
        let p = paths(&ui, 3, None).unwrap();
        assert_eq!(rows_under(&ui, list), 0);
        assert_eq!(num(&ui, p.icon, "visible"), 0.0);
        assert_eq!(text(&ui, p.desc, "string"), "");
        assert_eq!(t.desc_area, None);
        assert_eq!((num(&ui, list, "_highlight_y"), num(&ui, list, "_selected_height")), (-1.0, 0.0));
        let tab = fnv_ui::input::hit(&ui, &[menu], (140.0, 505.0), &no_measure).unwrap();
        assert_eq!(stats_tile(&ui, &p, &t.written.rows, tab.tile), Some(StatsTile::Tab(2)));
        // And twice over, and with rows after.
        assert!(run_tab(&mut ui, &s, &mut v, 3, &mut t).is_ok());
        assert_eq!(rows_under(&ui, list), 0);
        s.perks = perks();
        run_tab(&mut ui, &s, &mut v, 3, &mut t).unwrap();
        assert_eq!(rows_under(&ui, list), 2);
        s.perks.clear();
        run_tab(&mut ui, &s, &mut v, 3, &mut t).unwrap();
        assert_eq!(rows_under(&ui, list), 0, "the last taken away too");
    }

    /// Both lists hold their rows, the mode's trait shows one and its side (by this fixture's own
    /// expressions), the input acts on that one; no display of its own.
    #[test]
    fn general_fills_both_lists_and_the_mode_shows_one() {
        let words = PageWords { switch_hint: "<key>".into(), switch_general: "A word".into(), ..PageWords::default() };
        let (mut ui, mut v, mut t) = (tree(), StatsView::with_lens([0, 7, 13, 2, 1]), StatsTiles { words, ..Default::default() });
        let s = general_snap();
        let menu = run_tab(&mut ui, &s, &mut v, 4, &mut t).unwrap();
        assert_eq!(num(&ui, menu, "user0"), 4.0, "the tab");
        let name = list_named;
        let (general, reputation) = (name(&ui, "stats_general_container"), name(&ui, "stats_reputation_container"));
        assert_eq!((rows_under(&ui, general), rows_under(&ui, reputation)), (20, 1));
        assert_eq!(t.written.rows[0].key, RowKey::Rep(fnv_sim::FormId(0x40)));
        assert_eq!(num(&ui, name(&ui, "stats_genrep_container"), "user0"), 1.0);
        assert_eq!((num(&ui, general, "visible"), num(&ui, reputation, "visible")), (0.0, 1.0), "the fixture's expression, from what code wrote");
        // Nothing on the side until a row is pointed at.
        assert_eq!((num(&ui, name(&ui, "stats_reputation_image"), "visible"), num(&ui, name(&ui, "stats_reputation_title"), "visible")), (0.0, 0.0));
        v.highlight = true;
        run_tab(&mut ui, &s, &mut v, 4, &mut t).unwrap();
        assert_eq!(text(&ui, name(&ui, "stats_reputation_rep_title"), "string"), "Town");
        assert_eq!(text(&ui, name(&ui, "stats_reputation_title"), "string"), "A title");
        assert_eq!(text(&ui, name(&ui, "stats_reputation_image"), "filename"), "pics\\town.dds");
        assert_eq!(num(&ui, name(&ui, "stats_reputation_image"), "visible"), 1.0);
        assert_eq!(text(&ui, name(&ui, "stats_genrep_button"), "_PCButtonText"), "<key>");
        let p = paths(&ui, 4, None).unwrap();
        assert_eq!((num(&ui, p.icon, "visible"), text(&ui, p.desc, "string")), (0.0, String::new()), "no display on this tab");
        assert_eq!((t.desc_area, v.desc_bound()), (None, 0));
        let line = describe(&ui, t.written.paths.as_ref().unwrap(), &t.written);
        assert!(line.contains("General (Reputation) from tiles: 1 rows, \"Town\" selected, value none,"), "{line}");
        assert!(line.contains("reputation picture pics\\town.dds, switch A word, device"), "{line}");
        // The other list's rows stand where they are, its bar counted, at its top.
        let made_general = t.written.other_rows.clone();
        assert_eq!(num(&ui, general, "_scroll_delta"), ROW);
        assert_eq!(num(&ui, made_general[3].tile, "_y"), 3.0 * ROW);
        assert_eq!(list::value_shown(&ui, general), Some(0.0));
        // Switched: the same tiles, the input's now.
        v = general_view(20);
        run_tab(&mut ui, &s, &mut v, 4, &mut t).unwrap();
        assert_eq!((rows_under(&ui, general), rows_under(&ui, reputation)), (20, 1));
        assert_eq!(t.written.rows, made_general, "kept across the switch");
        assert_eq!(t.written.rows[3].key, RowKey::Stat(3));
        assert_eq!(num(&ui, name(&ui, "stats_genrep_container"), "user0"), 0.0);
        assert_eq!((num(&ui, general, "visible"), num(&ui, reputation, "visible")), (1.0, 0.0));
        assert_eq!((num(&ui, name(&ui, "stats_karma_rect"), "visible"), num(&ui, name(&ui, "stats_reptitle_rect"), "visible")), (1.0, 0.0));
        assert_eq!(text(&ui, name(&ui, "stats_karma_alignment"), "string"), "A word", "the word alone, no label of the hand-built page");
        assert_eq!(text(&ui, name(&ui, "stats_karma_image"), "filename"), stats::karma_picture("a_picture"));
        assert_eq!((num(&ui, name(&ui, "stats_karma_image"), "visible"), num(&ui, name(&ui, "stats_karma_title"), "visible")), (1.0, 1.0));
        assert_eq!(text(&ui, name(&ui, "stats_karma_title"), "string"), "From a setting");
        assert_eq!(v.list_most(4), 14);
        let line = describe(&ui, t.written.paths.as_ref().unwrap(), &t.written);
        assert!(line.contains("General (General) from tiles: 20 rows,"), "{line}");
        assert!(line.contains("karma title From a setting, switch Reputation, device"), "{line}");
        // Its scroll by the model, its own bar told apart.
        v.scroll(4, 3);
        run_tab(&mut ui, &s, &mut v, 4, &mut t).unwrap();
        assert_eq!(list::value_shown(&ui, general), Some(3.0));
        let p = t.written.paths.unwrap();
        let down = find_named(&ui, general, list::DOWN).unwrap();
        assert_eq!(stats_tile(&ui, &p, &t.written.rows, down), Some(StatsTile::ListBar(BarPiece::Down)));
        // Going to another tab takes both lists' rows away.
        run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        assert_eq!((rows_under(&ui, general), rows_under(&ui, reputation)), (0, 0));
    }

    /// A name the reader kept as plain text is never shown: the switch has the setting's word with
    /// the reputations up and NVForever's with the statistics, and a word of a file's own stands.
    #[test]
    fn the_switch_always_has_a_word() {
        let words = PageWords { switch_hint: "<key>".into(), switch_general: "From a setting".into(), ..PageWords::default() };
        let button_text = |ui: &Ui| text(ui, find_named(ui, list_named(ui, "stats_genrep_button"), "button_text").unwrap(), "string");
        let (mut ui, mut v, mut t) = (tree(), StatsView::with_lens([0, 7, 13, 2, 1]), StatsTiles { words: words.clone(), ..Default::default() });
        let s = general_snap();
        run_tab(&mut ui, &s, &mut v, 4, &mut t).unwrap();
        assert_eq!(button_text(&ui), "From a setting");
        v.switch();
        run_tab(&mut ui, &s, &mut v, 4, &mut t).unwrap();
        assert_eq!(button_text(&ui), SWITCH_REPUTATION);
        v.switch();
        run_tab(&mut ui, &s, &mut v, 4, &mut t).unwrap();
        assert_eq!(button_text(&ui), "From a setting", "and back");
        let (mut ui, mut v, mut t) = (tree_changed(|x| x.replace("&-sLeftOpen", "Its own")), StatsView::with_lens([0, 7, 13, 2, 1]), StatsTiles { words, ..Default::default() });
        run_tab(&mut ui, &s, &mut v, 4, &mut t).unwrap();
        assert_eq!(button_text(&ui), "Its own");
        v.switch();
        run_tab(&mut ui, &s, &mut v, 4, &mut t).unwrap();
        assert_eq!(button_text(&ui), SWITCH_REPUTATION);
        v.switch();
        run_tab(&mut ui, &s, &mut v, 4, &mut t).unwrap();
        assert_eq!(button_text(&ui), "Its own");
    }

    /// With no count of rows, the height the file gives holds as many whole rows as fit, and a
    /// step is what is left over by the positions after the first, not a row's height.
    #[test]
    fn a_list_with_a_height_of_its_own_keeps_it() {
        let own = |x: &str| x.replace("<_number_of_visible_items>6</_number_of_visible_items>", "<height>200</height>");
        let (mut ui, mut v, mut t) = (tree_changed(own), general_view(20), StatsTiles::default());
        run_tab(&mut ui, &general_snap(), &mut v, 4, &mut t).unwrap();
        let list = list_named(&ui, "stats_general_container");
        assert_eq!(num(&ui, list, "height"), 200.0, "stands");
        assert_eq!(v.list_most(4), 15, "five whole rows of 36");
        let bar = list::child(&ui, list, list::SCROLLBAR).unwrap();
        assert_eq!(num(&ui, bar, "_number_of_items"), 16.0);
        let step = (20.0 * ROW - 200.0) / 15.0;
        assert!((num(&ui, list, "_scroll_delta") - step).abs() < 1e-9);
        // The box is where the row stands by that step.
        v.highlight = true;
        v.select(4, 4);
        v.scroll(4, 3);
        run_tab(&mut ui, &general_snap(), &mut v, 4, &mut t).unwrap();
        assert!((num(&ui, list, "_highlight_y") - (4.0 * ROW - 3.0 * step)).abs() < 1e-9);
    }

    /// With a strip of the next row in the list's window, a pointer resting on it selects nothing and
    /// moves nothing, frame after frame; a click there still brings the row in.
    #[test]
    fn a_strip_of_a_row_under_a_still_pointer_does_not_walk_the_list() {
        let own = |x: &str| x.replace("<_number_of_visible_items>6</_number_of_visible_items>", "<height>200</height>");
        let (mut ui, mut v, mut t) = (tree_changed(own), general_view(20), StatsTiles::default());
        let s = general_snap();
        let menu = run_tab(&mut ui, &s, &mut v, 4, &mut t).unwrap();
        let mut acted = None;
        for _ in 0..6 {
            let p = t.written.paths.unwrap();
            // Five whole rows of 36 from the list's 50 down: the strip of the sixth, to its 200.
            let hit = fnv_ui::input::hit(&ui, &[menu], (50.0, 50.0 + 190.0), &no_measure).unwrap();
            let under = match stats_tile(&ui, &p, &t.written.rows, hit.tile) {
                Some(StatsTile::Row(i)) => Some(i),
                other => panic!("{other:?}"),
            };
            assert_eq!(under, Some(5), "the partial row is what is there");
            let in_sight = t.written.geometry.as_ref().map(|g| g.top..g.top + g.visible).unwrap();
            if let Some(HoverAct::Select(i)) = follow_hover(&mut acted, row_to_hover(under, &in_sight), true) {
                v.select(4, i);
            }
            run_tab(&mut ui, &s, &mut v, 4, &mut t).unwrap();
        }
        assert_eq!((v.top[4], v.selected[4]), (0, 0));
        v.select(4, 5);
        assert_eq!(v.top[4], 1, "the click's select");
        assert_eq!(row_to_hover(Some(5), &(1..6)), Some(5), "in sight now, a row like the others");
    }

    /// A weapon skill's mark shows by `visible`; a row with none hides it, and on other tabs too.
    #[test]
    fn the_badge_shows_only_on_a_row_with_one() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        run_tab(&mut ui, &snap(), &mut v, 2, &mut t).unwrap();
        let badge = list_named(&ui, "stats_icon_badge");
        assert_eq!(num(&ui, badge, "visible"), 0.0);
        v.select(2, 3);
        run_tab(&mut ui, &snap(), &mut v, 2, &mut t).unwrap();
        assert_eq!((num(&ui, badge, "visible"), text(&ui, badge, "filename")), (1.0, "pics\\mark.dds".to_string()));
        assert!(describe(&ui, t.written.paths.as_ref().unwrap(), &t.written).contains("badge pics\\mark.dds, "));
        v.select(2, 4);
        run_tab(&mut ui, &snap(), &mut v, 2, &mut t).unwrap();
        assert_eq!(num(&ui, badge, "visible"), 0.0, "only visible counts");
        // A file without one serves as well.
        let (mut ui, mut v, mut t) = (tree_changed(|x| x.replace("stats_icon_badge", "other")), view(), StatsTiles::default());
        assert!(run_tab(&mut ui, &snap(), &mut v, 2, &mut t).is_ok());
    }

    fn status_snap() -> Snapshot {
        let mut s = snap();
        s.name = "Player".into();
        s.limbs = [1.0, 0.4, 0.0, 1.0, 1.0, 1.0];
        s.hints.texts = ["(2) Item A".to_string(), "(1) Item B".to_string(), "Item C".to_string(), "(3) Item D".to_string()];
        s.hints.lit = [true, false, false, false];
        s.rad_resist = Some(8.0);
        s.rads = 320.0;
        s
    }

    fn status_tiles() -> StatsTiles {
        let words = PageWords { hint_keys: ["<S>".to_string(), "<E>".to_string(), "<A>".to_string(), "<X>".to_string()], level_word: "Lv".into(), ..PageWords::default() };
        StatsTiles { words, ..Default::default() }
    }

    fn status_paths(ui: &Ui) -> StatusPaths {
        match paths(ui, STATUS_TAB, None).unwrap().tab {
            TabPaths::Status(st) => st,
            _ => unreachable!(),
        }
    }

    #[test]
    fn cnd_writes_the_limbs_and_the_hints() {
        let (mut ui, mut v, mut t) = (tree(), view(), status_tiles());
        let menu = run_tab(&mut ui, &status_snap(), &mut v, STATUS_TAB, &mut t).unwrap();
        let st = status_paths(&ui);
        assert_eq!(num(&ui, menu, "user0"), 0.0, "the tab");
        assert_eq!(num(&ui, st.container, "user0"), 0.0, "the sub-page");
        assert_eq!((num(&ui, menu, "user1"), num(&ui, menu, "user2")), (320.0, 1000.0));
        assert_eq!(num(&ui, st.limbs[1].meter, "_Value"), 0.4, "a fraction");
        assert_eq!(text(&ui, st.limbs[1].picture, "filename"), stats::boy_picture("torso", false));
        assert_eq!(text(&ui, st.limbs[2].picture, "filename"), stats::boy_picture("left_arm", true), "crippled");
        assert_eq!(num(&ui, st.limbs[0].pct, "target"), 1.0, "the owner's click on the bar");
        assert_eq!(text(&ui, st.face, "filename"), stats::BOY_FACE);
        assert_eq!(text(&ui, st.name, "string"), "Player - Lv 3", "the setting's word");
        assert_eq!(text(&ui, st.hints[0].words, "string"), "(2) Item A");
        assert_eq!(text(&ui, st.hints[0].button, "_PCButtonText"), "<S>");
        assert!(ui.tile(st.hints[0].words).get("brightness").is_none(), "not the signal");
        assert_eq!((num(&ui, st.hints[0].button, "visible"), num(&ui, st.hints[1].button, "visible")), (1.0, 1.0));
        assert_eq!((num(&ui, st.hints[2].button, "visible"), num(&ui, st.hints[3].button, "visible")), (0.0, 0.0), "RAD's");
        assert_eq!((num(&ui, st.hints[0].button, "target"), num(&ui, st.hints[1].button, "target")), (1.0, 0.0), "lit while usable");
        assert_eq!(num(&ui, st.subs[3], "visible"), 0.0, "Hardcore's");
        assert_eq!(rows_under(&ui, st.effects), 0);
        // No display on this tab.
        let p = paths(&ui, STATUS_TAB, None).unwrap();
        assert_eq!((num(&ui, p.icon, "visible"), text(&ui, p.desc, "string")), (0.0, String::new()));
        let line = describe(&ui, t.written.paths.as_ref().unwrap(), &t.written);
        assert!(line.contains("Status CND from tiles: 0 rows,"), "{line}");
        assert!(line.contains("limbs 1 0.4 0 1 1 1,"), "{line}");
        assert!(line.contains("hints (2) Item A <S> lit and (1) Item B <E> dim, rads 320, hardcore off, device"), "{line}");
    }

    #[test]
    fn rad_writes_the_figures_and_its_own_hints() {
        let (mut ui, mut v, mut t) = (tree(), view(), status_tiles());
        v.status = 1;
        let menu = run_tab(&mut ui, &status_snap(), &mut v, STATUS_TAB, &mut t).unwrap();
        let st = status_paths(&ui);
        assert_eq!(num(&ui, st.container, "user0"), 1.0);
        assert_eq!(text(&ui, st.resist, "string"), "8%");
        assert_eq!(text(&ui, st.rads, "string"), "320");
        assert_eq!((text(&ui, st.rads_mid, "string"), text(&ui, st.rads_max, "string")), ("500".to_string(), "1000".to_string()));
        assert_eq!((num(&ui, st.hints[0].button, "visible"), num(&ui, st.hints[2].button, "visible")), (0.0, 1.0));
        assert_eq!(num(&ui, menu, "user1"), 320.0);
        // Nothing at no rads.
        let mut s = status_snap();
        s.rads = 0.0;
        s.rad_resist = None;
        run_tab(&mut ui, &s, &mut v, STATUS_TAB, &mut t).unwrap();
        assert_eq!((text(&ui, st.rads, "string"), text(&ui, st.resist, "string")), (String::new(), "--".to_string()));
    }

    #[test]
    fn eff_makes_a_row_a_line_in_name_order_and_the_list_is_as_high_as_they_are() {
        let (mut ui, mut v, mut t) = (tree(), view(), status_tiles());
        v.status = 2;
        let mut s = status_snap();
        s.effects = vec![crate::pipboy::stats::model::EffectLine { name: "Zed".into(), text: "z +1".into() }, crate::pipboy::stats::model::EffectLine { name: "Amy".into(), text: "a +2".into() }];
        run_tab(&mut ui, &s, &mut v, STATUS_TAB, &mut t).unwrap();
        let st = status_paths(&ui);
        assert_eq!(rows_under(&ui, st.effects), 2);
        let first = t.written.rows[0];
        assert_eq!(ui.tile(first.tile).name, "Amy");
        assert_eq!(text(&ui, find_named(&ui, first.tile, "stats_effect_name").unwrap(), "string"), "Amy");
        assert_eq!(text(&ui, find_named(&ui, first.tile, "stats_effect_string").unwrap(), "string"), "a +2");
        assert_eq!(num(&ui, st.effects, "height"), 80.0, "the rows' total");
        assert_eq!(num(&ui, t.written.rows[1].tile, "_y"), 40.0);
        assert_eq!((num(&ui, first.tile, "user2"), num(&ui, t.written.rows[1].tile, "user2")), (1.0, 0.0), "a line under each but the last");
        assert_eq!(num(&ui, st.no_effects, "visible"), 0.0);
        // More than the area holds are cut, not scrolled; none says so; another sub-page has none.
        s.effects = (0..5).map(|i| crate::pipboy::stats::model::EffectLine { name: format!("L{i}"), text: "x".into() }).collect();
        run_tab(&mut ui, &s, &mut v, STATUS_TAB, &mut t).unwrap();
        assert_eq!(num(&ui, st.effects, "height"), 80.0, "two of 40 in 112");
        assert_eq!(t.written.geometry.as_ref().map(|g| g.counts), Some(None));
        s.effects.clear();
        run_tab(&mut ui, &s, &mut v, STATUS_TAB, &mut t).unwrap();
        assert_eq!(num(&ui, st.no_effects, "visible"), 1.0);
        v.status = 0;
        run_tab(&mut ui, &s, &mut v, STATUS_TAB, &mut t).unwrap();
        assert_eq!(num(&ui, st.no_effects, "visible"), 0.0);
    }

    /// In Hardcore code shows the three buttons and each need's page reads its figure, on RAD's
    /// shape (see production Hardcore status panel above); out of it a page of theirs goes back among those shown.
    #[test]
    fn hardcore_shows_its_buttons_and_pages() {
        let (mut ui, mut v, mut t) = (tree(), view(), status_tiles());
        let mut s = status_snap();
        s.hardcore = true;
        s.needs = [710.0, 0.0, 130.0];
        v.status = 3;
        let menu = run_tab(&mut ui, &s, &mut v, STATUS_TAB, &mut t).unwrap();
        let st = status_paths(&ui);
        for &sub in &st.subs[3..6] {
            assert_eq!(num(&ui, sub, "visible"), 1.0);
        }
        assert_eq!(num(&ui, st.container, "user0"), 3.0);
        assert_eq!(num(&ui, menu, "user1"), 710.0, "the page's own meter");
        assert_eq!((text(&ui, st.needs[0][0], "string"), text(&ui, st.needs[1][0], "string")), ("710".to_string(), String::new()));
        assert_eq!(text(&ui, st.needs[2][2], "string"), "1000");
        assert!(describe(&ui, t.written.paths.as_ref().unwrap(), &t.written).contains("Status H2O from tiles"));
        s.hardcore = false;
        run_tab(&mut ui, &s, &mut v, STATUS_TAB, &mut t).unwrap();
        assert_eq!(v.status, 2);
        assert_eq!(num(&ui, st.subs[3], "visible"), 0.0);
    }

    #[test]
    fn status_tiles_take_the_pointer() {
        let (mut ui, mut v, mut t) = (tree(), view(), status_tiles());
        let mut s = status_snap();
        s.hardcore = true;
        let menu = run_tab(&mut ui, &s, &mut v, STATUS_TAB, &mut t).unwrap();
        let p = paths(&ui, STATUS_TAB, None).unwrap();
        let tile_at = |ui: &Ui, x: f64, y: f64| fnv_ui::input::hit(ui, &[menu], (x, y), &no_measure).and_then(|h| stats_tile(ui, &p, &[], h.tile));
        for i in 0..6 {
            assert_eq!(tile_at(&ui, 705.0 + i as f64 * 20.0, 185.0), Some(StatsTile::Sub(i)));
            // Under the bar and its word, by this fixture's own places.
            assert_eq!(tile_at(&ui, 705.0 + i as f64 * 20.0, 355.0), Some(StatsTile::Limb(i)), "the bar");
            assert_eq!(tile_at(&ui, 705.0 + i as f64 * 20.0, 375.0), Some(StatsTile::Limb(i)), "the word");
        }
        assert_eq!(tile_at(&ui, 705.0, 205.0), Some(StatsTile::Hint(0)));
        assert_eq!(tile_at(&ui, 815.0, 205.0), None, "a dim hint lets the pointer through");
        // The picture and, over the head, the face: the file makes them targets while hurt.
        let st = status_paths(&ui);
        assert_eq!(stats_tile(&ui, &p, &[], st.limbs[3].picture), Some(StatsTile::Limb(3)));
        assert_eq!(stats_tile(&ui, &p, &[], st.face), Some(StatsTile::Limb(0)));
    }

    /// [design] A sub-page's button as `mark_inner_rects` meets it, in numbers of this test's own:
    /// an inner rect repeating the button's place, transparent, over words of their own.
    #[test]
    fn a_sub_buttons_inner_rect_neither_moves_it_twice_nor_hides_it() {
        let plain = r#"<hotrect name="stats_RAD_button"><target>&true;</target><x>20</x><y>-40</y><width>18</width><height>18</height></hotrect>"#;
        let nested = r#"<hotrect name="stats_RAD_button"><target>&true;</target><x>20</x><y>-40</y><width>18</width><height>18</height>
          <rect name="stb_text_box"><target>&true;</target><x>20</x><y>-40</y><width>18</width><height>18</height><alpha>0</alpha><text name="InnerWords"><alpha>200</alpha><string>S</string></text></rect></hotrect>"#;
        let (mut ui, mut v, mut t) = (tree_changed(|x| x.replace(plain, nested)), view(), status_tiles());
        let menu = run_tab(&mut ui, &status_snap(), &mut v, STATUS_TAB, &mut t).unwrap();
        let p = paths(&ui, STATUS_TAB, None).unwrap();
        let items = layout::layout(&ui, menu, &|_, _, _| (0.0, 0.0), true);
        let words = items.iter().find(|i| ui.tile(i.tile).name == "InnerWords").unwrap();
        assert_eq!((words.rect.left, words.rect.top, words.alpha), (720.0, 180.0, 200.0), "at its one place, its own alpha");
        let tile_at = |x: f64, y: f64| fnv_ui::input::hit(&ui, &[menu], (x, y), &no_measure).and_then(|h| stats_tile(&ui, &p, &[], h.tile));
        assert_eq!(tile_at(725.0, 185.0), Some(StatsTile::Sub(1)));
        assert_eq!(tile_at(745.0, 145.0), None, "not where the two places added would be");
    }

    /// The file works from a picture's size: written for the file it shows, its zoom in, again
    /// when the file changes; one of an atlas, or a file not found, gets none.
    #[test]
    fn a_pictures_size_is_written_for_the_file_to_work_from() {
        let extra = r#"<image name="stats_icon"/>
          <image name="Sized"><filename>pics\a.dds</filename><zoom>150</zoom><width><copy src="me()" trait="filewidth"/></width></image>
          <image name="Atlas"><filename>pics\b.dds</filename><texatlas>x.tai</texatlas></image>
          <image name="Lost"><filename>pics\nofile.dds</filename></image>"#;
        let (mut ui, mut v, mut t) = (tree_changed(|x| x.replace(r#"<image name="stats_icon"/>"#, extra)), view(), StatsTiles::default());
        run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        let (sized, atlas, lost) = (list_named(&ui, "Sized"), list_named(&ui, "Atlas"), list_named(&ui, "Lost"));
        assert_eq!((num(&ui, sized, "filewidth"), num(&ui, sized, "fileheight"), num(&ui, sized, "width")), (300.0, 150.0, 300.0));
        assert!(ui.tile(atlas).get("filewidth").is_none());
        assert!(ui.tile(lost).get("filewidth").is_none());
        let icon = list_named(&ui, "stats_icon");
        assert_eq!(num(&ui, icon, "filewidth"), 200.0, "a picture code names");
        ui.set_trait(icon, "filewidth", Value::Num(1.0));
        v.select(PAGE, 3);
        run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        assert_eq!(num(&ui, icon, "filewidth"), 200.0, "again for another file");
    }

    /// A zoom changed on the same file (a mod's script) writes the sizes again with the new zoom,
    /// and what the file works out from them follows.
    #[test]
    fn a_pictures_size_follows_a_new_zoom_on_the_same_file() {
        let extra = r#"<image name="stats_icon"/>
          <image name="Zoomed"><filename>pics\a.dds</filename><zoom>50</zoom><width><copy src="me()" trait="filewidth"/></width></image>"#;
        let (mut ui, mut v, mut t) = (tree_changed(|x| x.replace(r#"<image name="stats_icon"/>"#, extra)), view(), StatsTiles::default());
        run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        let zoomed = list_named(&ui, "Zoomed");
        assert_eq!((num(&ui, zoomed, "filewidth"), num(&ui, zoomed, "fileheight"), num(&ui, zoomed, "width")), (100.0, 50.0, 100.0));
        ui.set_trait(zoomed, "zoom", Value::Num(200.0));
        run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        assert_eq!((num(&ui, zoomed, "filewidth"), num(&ui, zoomed, "fileheight"), num(&ui, zoomed, "width")), (400.0, 200.0, 400.0), "the same file at the new zoom");
        ui.set_trait(zoomed, "zoom", Value::Num(100.0));
        run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        assert_eq!((num(&ui, zoomed, "filewidth"), num(&ui, zoomed, "fileheight")), (200.0, 100.0), "and back");
    }

    /// The hidden hint keeps a box's height for what stands under it.
    #[test]
    fn the_hidden_hint_is_measured_with_no_words_of_its_own() {
        let (mut ui, mut v, mut t) = (tree(), view(), status_tiles());
        run_tab(&mut ui, &status_snap(), &mut v, STATUS_TAB, &mut t).unwrap();
        let words = status_paths(&ui).healing_text.unwrap();
        assert_eq!((text(&ui, words, "string"), num(&ui, words, "height")), (" ".to_string(), 30.0));
    }

    #[test]
    fn a_file_short_of_statuss_tiles_leaves_that_tab_hand_built() {
        for (cut, reason) in [("stats_CND_button", "stats_CND_button missing"), ("stats_player_leftleg_pct_meter", "stats_player_leftleg_pct_meter missing"), ("stats_effects_template", "stats_effects_template missing"), ("stats_list_template", "stats_list_template missing")] {
            let (mut ui, mut v) = (tree(), view());
            let mut t = StatsTiles { cut: Some(cut.into()), ..status_tiles() };
            assert_eq!(run_tab(&mut ui, &status_snap(), &mut v, STATUS_TAB, &mut t).unwrap_err(), reason);
        }
        // Without one of the file's, Status alone: S.P.E.C.I.A.L. stands.
        let (mut ui, mut v) = (tree(), view());
        let mut t = StatsTiles { cut: Some("stats_CND_button".into()), ..status_tiles() };
        assert!(run(&mut ui, &status_snap(), &mut v, true, &mut t).is_ok());
    }

    /// A title the plugins do not hold has no words here: the table's are from no data file.
    /// Without the scripts' game the side says so as the figures do.
    #[test]
    fn a_karma_title_is_a_settings_or_none() {
        let (mut ui, mut v, mut t) = (tree(), general_view(20), StatsTiles::default());
        let mut s = general_snap();
        s.karma_setting_title = None;
        run_tab(&mut ui, &s, &mut v, 4, &mut t).unwrap();
        let (title, word) = (list_named(&ui, "stats_karma_title"), list_named(&ui, "stats_karma_alignment"));
        assert_eq!(text(&ui, title, "string"), "");
        s.karma = None;
        s.karma_title = None;
        run_tab(&mut ui, &s, &mut v, 4, &mut t).unwrap();
        assert_eq!((text(&ui, title, "string"), text(&ui, word, "string")), ("--".to_string(), "--".to_string()));
        assert_eq!(text(&ui, list_named(&ui, "stats_karma_image"), "filename"), "");
    }

    #[test]
    fn generals_switch_takes_the_pointer_and_the_physical_key_is_the_models() {
        let (mut ui, mut v, mut t) = (tree(), StatsView::with_lens([0, 7, 13, 2, 1]), StatsTiles::default());
        let menu = run_tab(&mut ui, &general_snap(), &mut v, 4, &mut t).unwrap();
        let p = paths(&ui, 4, None).unwrap();
        let hit = fnv_ui::input::hit(&ui, &[menu], (20.0 + 510.0, 50.0 + 10.0), &no_measure).unwrap();
        assert_eq!(stats_tile(&ui, &p, &t.written.rows, hit.tile), Some(StatsTile::Switch));
        assert!(!menus::key_click_acts(STATS_MENU, "R", &["R"]), "the hand-built key switches: a file's binding would switch back");
        assert!(menus::key_click_acts(STATS_MENU, "R", &[]), "a script's tap");
        // Off the tab no tile is the switch.
        let other = paths(&ui, PAGE, None).unwrap();
        assert_eq!(stats_tile(&ui, &other, &[], p_button(&p)), None);
    }

    fn p_button(p: &Paths) -> TileId {
        match p.tab {
            TabPaths::General(g) => g.button,
            TabPaths::List { list } => list,
            TabPaths::Status(st) => st.effects,
        }
    }

    /// No reputations yet: the list empty, the page standing; nothing of the hand-built line.
    #[test]
    fn general_with_no_reputations_stands() {
        let (mut ui, mut v, mut t) = (tree(), StatsView::with_lens([0, 7, 13, 2, 0]), StatsTiles::default());
        let mut s = general_snap();
        s.standings.clear();
        run_tab(&mut ui, &s, &mut v, 4, &mut t).unwrap();
        assert_eq!(rows_under(&ui, list_named(&ui, "stats_reputation_container")), 0);
        assert_eq!(text(&ui, list_named(&ui, "stats_reputation_rep_title"), "string"), "");
    }

    #[test]
    fn a_file_short_of_generals_tiles_leaves_that_tab_hand_built() {
        for name in ["stats_genrep_container", "stats_general_container", "stats_reputation_container", "stats_karma_title", "stats_reputation_image", "stats_genrep_button"] {
            let (mut ui, mut v) = (tree(), StatsView::with_lens([0, 7, 13, 2, 1]));
            let mut t = StatsTiles { cut: Some(name.into()), ..Default::default() };
            assert_eq!(run_tab(&mut ui, &general_snap(), &mut v, 4, &mut t).unwrap_err(), format!("{name} missing"));
            assert!(run(&mut ui, &snap(), &mut StatsView::with_lens([0, 7, 13, 2, 1]), true, &mut t).is_ok(), "{name}: not the other tabs'");
        }
    }

    #[test]
    fn the_lists_bar_is_told_from_the_descriptions() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        let menu = run_tab(&mut ui, &snap(), &mut v, 2, &mut t).unwrap();
        let p = paths(&ui, 2, None).unwrap();
        let rows = t.written.rows.clone();
        let mut pointer = Pointer::default();
        // Under the list's 12, 30 and the bar's own 170 across.
        let (bx, by) = (12.0 + 170.0 + 5.0, 30.0);
        for (y, piece) in [(5.0, BarPiece::Up), (20.0, BarPiece::PageUp), (40.0, BarPiece::PageDown), (58.0, BarPiece::Down)] {
            let r = at(&mut ui, &mut pointer, menu, bx, by + y, true);
            assert_eq!(r.clicks.iter().filter_map(|c| stats_tile(&ui, &p, &rows, c.tile)).next_back(), Some(StatsTile::ListBar(piece)));
        }
        let r = at(&mut ui, &mut pointer, menu, 12.0 + 170.0 + 17.0, by + 5.0, true);
        assert_eq!(r.clicks.iter().filter_map(|c| stats_tile(&ui, &p, &rows, c.tile)).next_back(), Some(StatsTile::ListBar(BarPiece::Marker)));
        // The description's bar is still its own.
        let r = at(&mut ui, &mut pointer, menu, 400.0 + 210.0 + 5.0, 34.0 + 100.0 + 5.0, true);
        assert_eq!(r.clicks.iter().filter_map(|c| stats_tile(&ui, &p, &rows, c.tile)).next_back(), Some(StatsTile::StepUp));
    }

    #[test]
    fn the_lists_marker_drags_within_the_models_range() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        run_tab(&mut ui, &snap(), &mut v, 2, &mut t).unwrap();
        let p = paths(&ui, 2, None).unwrap();
        // The bar's top is the list's 30; pressed 2 under the marker at the top.
        let drag = marker_grab(&ui, &p, BarOf::List, 32.0).unwrap();
        assert_eq!(drag.of, BarOf::List);
        assert_eq!(steps_under(&ui, drag, 32.0, 4), Some(0), "held where it stands");
        assert_eq!(steps_under(&ui, drag, 54.0, 4), Some(2), "22 lower at 11 a step");
        assert_eq!(steps_under(&ui, drag, 900.0, 4), Some(4));
        assert_eq!(steps_under(&ui, drag, 0.0, 4), Some(0));
    }

    #[test]
    fn the_test_aid_cuts_a_name_and_the_page_can_come_back() {
        let (mut ui, mut v) = (tree(), view());
        let mut t = StatsTiles { cut: Some(ROW_TEMPLATE.into()), ..Default::default() };
        assert!(run(&mut ui, &snap(), &mut v, true, &mut t).is_err());
        t.cut = Some(ICON.to_ascii_uppercase());
        assert_eq!(run(&mut ui, &snap(), &mut v, true, &mut t).unwrap_err(), format!("{ICON} missing"));
        t.cut = None;
        assert!(run(&mut ui, &snap(), &mut v, true, &mut t).is_ok(), "a failed pass is tried again");
    }

    #[test]
    fn the_cut_aid_reads_a_name_and_a_time() {
        assert_eq!(parse_cut("stats_icon"), ("stats_icon".to_string(), None));
        assert_eq!(parse_cut(" stats_icon @ 33.5 "), ("stats_icon".to_string(), Some(33.5)));
    }

    /// [design] A tab as `write_tabs` meets it, in numbers of this test's own: an inner rect
    /// repeating the tab's place, transparent, over words and a line that are not.
    #[test]
    fn a_tabs_inner_rect_neither_moves_it_twice_nor_hides_it() {
        let tab = r#"<hotrect name="stats_tailline_skills"><target>&true;</target><x>130</x><y>500</y><width>50</width><height>20</height><text name="TabWords"><string>Tab</string></text></hotrect>"#;
        let nested = r#"<hotrect name="stats_tailline_skills"><target>&true;</target><x>130</x><y>500</y><width>50</width><height>20</height>
          <rect name="stb_text_box"><target>&true;</target><x>130</x><y>500</y><width>50</width><height>20</height><alpha>0</alpha>
            <text name="InnerWords"><alpha>200</alpha><string>Tab</string></text>
            <rect name="InnerBox"><alpha>0</alpha><image name="InnerLine"><filename>a.dds</filename><width>50</width><height>2</height></image></rect>
          </rect></hotrect>"#;
        let (mut ui, mut v, mut t) = (tree_changed(|x| x.replace(tab, nested)), view(), StatsTiles::default());
        let menu = run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        let p = paths(&ui, PAGE, None).unwrap();
        let items = layout::layout(&ui, menu, &|_, _, _| (0.0, 0.0), true);
        let item = |name: &str| items.iter().find(|i| ui.tile(i.tile).name == name).unwrap();
        let words = item("InnerWords");
        assert_eq!((words.rect.left, words.rect.top), (130.0, 500.0), "at the tab's one place");
        assert_eq!(words.alpha, 200.0, "its own alpha, through the transparent rect");
        assert_eq!(item("InnerLine").alpha, 255.0);
        // It takes the pointer there, and not where the two places added would be.
        let at_tab = fnv_ui::input::hit(&ui, &[menu], (140.0, 505.0), &no_measure).unwrap();
        assert_eq!(stats_tile(&ui, &p, &t.written.rows, at_tab.tile), Some(StatsTile::Tab(2)));
        assert!(fnv_ui::input::hit(&ui, &[menu], (270.0, 1005.0), &no_measure).is_none());
        // The file's places stand: the tab's place may be read from the rect's, as it is
        // in the game's file, where writing either would stand every tab at the line's start.
        let inner = p.tab_inners[2].unwrap();
        assert_eq!((num(&ui, p.tabs[2], "x"), num(&ui, inner, "x")), (130.0, 130.0));
        assert_eq!((num(&ui, p.tabs[2], "y"), num(&ui, inner, "y")), (500.0, 500.0));
    }

    #[test]
    fn a_row_past_the_canvas_is_no_usable_layout() {
        let (mut ui, mut v, mut t) = (tree_changed(|x| x.replace("<x>12</x><y>30</y>", "<x>12</x><y>750</y>")), view(), StatsTiles::default());
        let got = run(&mut ui, &snap(), &mut v, true, &mut t).unwrap_err();
        assert!(got.contains("does not fit"), "{got}");
    }

    /// What was checked is what the drawing gets, and a tab with no words is no tab.
    #[test]
    fn the_pieces_checked_are_the_ones_painted() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        let row = t.written.rows[2].text;
        assert!(t.pieces.iter().any(|x| x.tile == row));
        let empty = |x: &str| x.replace(r#"<x>130</x><y>500</y><width>50</width><height>20</height><text name="TabWords"><string>Tab</string>"#, r#"<x>130</x><y>500</y><width>50</width><height>20</height><text name="TabWords"><string></string>"#);
        let (mut ui, mut v, mut t) = (tree_changed(empty), view(), StatsTiles::default());
        assert_eq!(run(&mut ui, &snap(), &mut v, true, &mut t).unwrap_err(), "tab 2 is not shown");
    }

    #[test]
    fn a_page_off_the_canvas_or_unseen_or_untargeted_is_no_usable_layout() {
        for (change, reason) in [
            (("<x>12</x><y>30</y>", "<x>1020</x><y>30</y>"), "row 0 does not fit"),
            (("<x>70</x><y>500</y>", "<x>1020</x><y>500</y>"), "tab 1 does not fit"),
            ((r#"<rect name="Branch">"#, r#"<rect name="Branch"><alpha>0</alpha>"#), "row 0 is not shown"),
            ((r#"<hotrect name="TestRow"><target>&true;</target>"#, r#"<hotrect name="TestRow"><target>0</target>"#), "Row(0) does not take the pointer"),
            ((r#"<hotrect name="stats_tailline_skills"><target>&true;</target>"#, r#"<hotrect name="stats_tailline_skills"><target>0</target>"#), "Tab(2) does not take the pointer"),
        ] {
            let (mut ui, mut v, mut t) = (tree_changed(|x| x.replace(change.0, change.1)), view(), StatsTiles::default());
            let got = run(&mut ui, &snap(), &mut v, true, &mut t).unwrap_err();
            assert!(got.contains(reason), "{reason}: {got}");
        }
        // The tab showing need not take the pointer: a click on it does nothing.
        let (mut ui, mut v, mut t) = (tree_changed(|x| x.replace(r#"<hotrect name="stats_tailline_SPECIAL"><target>&true;</target>"#, r#"<hotrect name="stats_tailline_SPECIAL"><target>0</target>"#)), view(), StatsTiles::default());
        assert!(run(&mut ui, &snap(), &mut v, true, &mut t).is_ok());
    }

    /// A target that comes to lie over the rows takes every click from them: seen at once.
    #[test]
    fn a_target_over_the_rows_refuses_the_page_until_it_goes() {
        let cover = |x: &str| x.replace(r#"<image name="background_layout"/>"#, r#"<image name="background_layout"/><hotrect name="Cover"><x>12</x><y>30</y><width>200</width><height>300</height><depth>5</depth><target>0</target></hotrect>"#);
        let (mut ui, mut v, mut t) = (tree_changed(cover), view(), StatsTiles::default());
        let menu = run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        let over = find_named(&ui, menu, "Cover").unwrap();
        ui.set_trait(over, "target", Value::Num(1.0));
        ui.update();
        assert!(run(&mut ui, &snap(), &mut v, true, &mut t).unwrap_err().contains("Row(0)"));
        ui.set_trait(over, "target", Value::Num(0.0));
        ui.update();
        assert!(run(&mut ui, &snap(), &mut v, true, &mut t).is_ok());
    }

    /// The description's window off the canvas, and a row's words partly outside the list's
    /// window: neither reads, so neither passes.
    #[test]
    fn what_cannot_be_read_whole_is_no_usable_layout() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        let p = paths(&ui, PAGE, None).unwrap();
        ui.set_trait(p.desc_rect, "y", Value::Num(2000.0));
        ui.update();
        assert!(run(&mut ui, &snap(), &mut v, true, &mut t).unwrap_err().contains("description does not fit"));
        ui.set_trait(p.desc_rect, "y", Value::Num(100.0));
        ui.update();
        assert!(run(&mut ui, &snap(), &mut v, true, &mut t).is_ok());
        ui.set_trait(p.list(), "width", Value::Num(15.0));
        ui.update();
        assert!(run(&mut ui, &snap(), &mut v, true, &mut t).unwrap_err().contains("row 0 does not fit"));
    }

    /// A list row with `<clips>1</clips>` inside a `<clipwindow>1</clipwindow>` list opts
    /// into the list's window rather than self-clipping its own text children (`list_box_template.xml`,
    /// [documented: GECK wiki `clipwindow`]): a text vertical alignment offset exceeding
    /// `_VerticalSpacing` stays inside the list window instead of failing with `row 0 does not fit`,
    /// while narrowing the list window itself still rejects the page.
    #[test]
    fn a_clipped_list_row_with_vertical_text_offset_uses_the_lists_clipwindow() {
        let with_offset = |x: &str| {
            x.replace(
                r#"<hotrect name="TestRow"><target>&true;</target>"#,
                r#"<hotrect name="TestRow"><target>&true;</target><clips>&true;</clips>"#,
            )
            .replace(
                r#"<text name="ListItemText"><string>Row</string></text>"#,
                r#"<text name="ListItemText"><y>14</y><string>Row</string></text>"#,
            )
        };
        let (mut ui, mut v, mut t) = (tree_changed(with_offset), view(), StatsTiles::default());
        run(&mut ui, &snap(), &mut v, true, &mut t).expect("row text inside list clipwindow should be admitted");
        let p = paths(&ui, PAGE, None).unwrap();
        let row0_text = t.written.rows[0].text;
        let piece = t.pieces.iter().find(|piece| piece.tile == row0_text).expect("row 0 text piece");
        let clip = piece.clip.expect("list clipwindow clip");
        assert!(clip.height > ROW, "clip should come from the list window ({}), not the {ROW}-unit row box", clip.height);
        // Narrowing the list container itself still clips row 0 and refuses the page.
        ui.set_trait(p.list(), "width", Value::Num(15.0));
        ui.update();
        assert!(run(&mut ui, &snap(), &mut v, true, &mut t).unwrap_err().contains("row 0 does not fit"));
    }

    /// With nothing in the model changed, what a script does to the tree is seen on the next
    /// final pass, and the page comes back when it is put right.
    #[test]
    fn what_a_script_moves_or_hides_is_seen_with_the_model_unchanged() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        let p = paths(&ui, PAGE, None).unwrap();
        let area = t.desc_area;
        ui.set_trait(p.list(), "x", Value::Num(1020.0));
        ui.update();
        assert!(run(&mut ui, &snap(), &mut v, true, &mut t).unwrap_err().contains("does not fit"));
        ui.set_trait(p.list(), "x", Value::Num(12.0));
        ui.update();
        assert!(run(&mut ui, &snap(), &mut v, true, &mut t).is_ok());
        // The row's target off: the same places, and the pointer's check runs again.
        let row = t.written.rows[1].tile;
        ui.set_trait(row, "target", Value::Num(0.0));
        ui.update();
        assert!(run(&mut ui, &snap(), &mut v, true, &mut t).unwrap_err().contains("Row(1)"));
        ui.set_trait(row, "target", Value::Num(1.0));
        ui.update();
        assert!(run(&mut ui, &snap(), &mut v, true, &mut t).is_ok());
        // The description hidden; then moved, and the wheel's area goes with it.
        ui.set_trait(p.desc, "visible", Value::Num(0.0));
        ui.update();
        assert!(run(&mut ui, &snap(), &mut v, true, &mut t).unwrap_err().contains("description"));
        ui.set_trait(p.desc, "visible", Value::Num(1.0));
        ui.set_trait(p.desc, "x", Value::Num(40.0));
        ui.update();
        assert!(run(&mut ui, &snap(), &mut v, true, &mut t).is_ok());
        assert_ne!(t.desc_area, area);
    }

    #[test]
    fn the_screen_menu_has_no_title_or_background_and_gives_them_back() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        run(&mut ui, &snap(), &mut v, false, &mut t).unwrap();
        let p = paths(&ui, PAGE, None).unwrap();
        let from = f64::from(crate::pipboy::frame::lead_line_end());
        assert_eq!(num(&ui, p.title, "visible"), 0.0);
        assert_eq!((num(&ui, p.line, "x"), num(&ui, p.line, "_length")), (from, 340.0 - from), "the line runs on from the lead line to where it ended");
        run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        assert_eq!(num(&ui, p.title, "visible"), 1.0);
        assert_eq!((num(&ui, p.line, "x"), num(&ui, p.line, "_length")), (300.0, 40.0));
    }

    /// Only page content goes into the image: its background is the shell's, in either frame,
    /// and a file without one serves as well.
    #[test]
    fn the_page_paints_no_background_of_its_own() {
        for device in [true, false] {
            let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
            run(&mut ui, &snap(), &mut v, device, &mut t).unwrap();
            let p = paths(&ui, PAGE, None).unwrap();
            assert_eq!(num(&ui, p.background.unwrap(), "visible"), 0.0, "device {device}");
        }
        let (mut ui, mut v, mut t) = (tree_changed(|x| x.replace("background_layout", "other")), view(), StatsTiles::default());
        assert!(run(&mut ui, &snap(), &mut v, true, &mut t).is_ok());
    }

    #[test]
    fn only_an_unresolved_title_is_replaced() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        let p = paths(&ui, PAGE, None).unwrap();
        assert_eq!(text(&ui, p.title, "string"), "Words", "a replacement's words stand");
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        let title = paths(&ui, PAGE, None).unwrap().title;
        ui.set_trait(title, "string", Value::Str("&sSomeName".into()));
        run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        assert_eq!(text(&ui, title, "string"), Page::Stats.title());
    }

    /// One step of the pointer on the canvas.
    fn at(ui: &mut Ui, pointer: &mut Pointer, menu: TileId, x: f64, y: f64, press: bool) -> fnv_ui::input::Report {
        pointer.step(ui, &[menu], &Input { at: Some((x, y)), press, ..Default::default() }, &no_measure)
    }

    #[test]
    fn canvas_points_hit_rows_tabs_and_the_description_controls() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        let menu = run(&mut ui, &snap(), &mut v, true, &mut t).unwrap();
        let p = paths(&ui, PAGE, None).unwrap();
        let rows = t.written.rows.clone();
        let mut pointer = Pointer::default();
        // Row 2: the list's 12 + 8 across, its 30 + two rows down.
        let r = at(&mut ui, &mut pointer, menu, 50.0, 30.0 + 2.5 * ROW, true);
        assert_eq!(stats_tile(&ui, &p, &rows, pointer.over.unwrap()), Some(StatsTile::Row(2)));
        assert_eq!(r.clicks.len(), 1);
        assert!(matches!(r.clicks[0].by, By::Pointer));
        assert_eq!(stats_tile(&ui, &p, &rows, r.clicks[0].tile), Some(StatsTile::Row(2)));
        // The tabs by their own numbers.
        for (i, x) in [20.0, 80.0, 140.0, 200.0, 260.0].into_iter().enumerate() {
            let r = at(&mut ui, &mut pointer, menu, x, 505.0, true);
            assert_eq!(r.clicks.iter().filter_map(|c| stats_tile(&ui, &p, &rows, c.tile)).next_back(), Some(StatsTile::Tab(i)));
        }
        // The bar's pieces, under the display's 400, 34, the window's 100 and the bar's 210.
        let (bx, by) = (400.0 + 210.0 + 5.0, 34.0 + 100.0);
        for (y, piece) in [(5.0, StatsTile::StepUp), (15.0, StatsTile::PageUp), (24.0, StatsTile::PageDown), (45.0, StatsTile::StepDown)] {
            let r = at(&mut ui, &mut pointer, menu, bx, by + y, true);
            assert_eq!(r.clicks.iter().filter_map(|c| stats_tile(&ui, &p, &rows, c.tile)).next_back(), Some(piece));
        }
        // Nothing there.
        at(&mut ui, &mut pointer, menu, 900.0, 700.0, false);
        assert_eq!(pointer.over, None);
    }

    #[test]
    fn the_marker_drags_within_the_bound() {
        let (mut ui, mut v, mut t) = (tree(), view(), StatsTiles::default());
        let mut s = snap();
        s.special[0].description = "a|b|c|d|e".into();
        run(&mut ui, &s, &mut v, true, &mut t).unwrap();
        let p = paths(&ui, PAGE, None).unwrap();
        // At value 0 the marker's top is the bar's, 134 down: pressed 2 under it.
        let drag = marker_grab(&ui, &p, BarOf::Description, 136.0).unwrap();
        assert_eq!(steps_under(&ui, drag, 136.0, 5), Some(0), "held where it stands");
        assert_eq!(steps_under(&ui, drag, 142.0, 5), Some(2), "6 lower at 3 a step");
        assert_eq!(steps_under(&ui, drag, 900.0, 5), Some(5));
        assert_eq!(steps_under(&ui, drag, 0.0, 5), Some(0));
    }

    #[test]
    fn the_pointer_selects_as_the_hand_built_list_does() {
        let mut acted = None;
        // A row under a pointer that has not moved keeps the selection.
        assert_eq!(follow_hover(&mut acted, Some(4), false), None);
        assert_eq!(acted, None);
        // Moved onto it: selected and boxed, once.
        assert_eq!(follow_hover(&mut acted, Some(4), true), Some(HoverAct::Select(4)));
        assert_eq!(follow_hover(&mut acted, Some(4), true), None, "a key's step while it stays there is not undone");
        // Off the rows: no box, once, so a key's box after that holds.
        assert_eq!(follow_hover(&mut acted, None, true), Some(HoverAct::BoxOff));
        assert_eq!(follow_hover(&mut acted, None, true), None);
    }

    /// A menu whose own `xdown` moves the highlight to a row and `xup` back to another. A
    /// physical press is the hand-built key's, which steps the list: the moved highlight must
    /// not select as well, on that frame or while it is held there. A script's tap has no
    /// hand-built step, so it selects, the row under a stationary pointer too.
    #[test]
    fn a_files_navigation_acts_once() {
        let change = |x: &str| x.replace(r#"<menu name="StatsMenu">"#, r#"<menu name="StatsMenu"><xdown><ref src="Row3" trait="mouseover"/></xdown><xup><ref src="Row1" trait="mouseover"/></xup>"#);
        let mut s = snap();
        s.special[1].name = "Row1".into();
        s.special[3].name = "Row3".into();
        let spot = (50.0, 30.0 + 1.5 * ROW);
        let (mut ui, mut v, mut t) = (tree_changed(change), view(), StatsTiles::default());
        let menu = run(&mut ui, &s, &mut v, true, &mut t).unwrap();
        let p = paths(&ui, PAGE, None).unwrap();
        let rows = t.written.rows.clone();
        let mut pointer = Pointer::default();
        let mut step = |ui: &mut Ui, pads: &[&str]| pointer.step(ui, &[menu], &Input { at: Some(spot), pads: pads.iter().map(|k| k.to_string()).collect(), ..Default::default() }, &no_measure);
        // The pointer alone moves onto its own row: no key's doing.
        let r = step(&mut ui, &[]);
        assert_eq!(r.over.and_then(|o| o.1), Some(rows[1].tile));
        assert_eq!(keyed_row(&ui, &p, &rows, &r, &[]), None);
        // A physical Down: the highlight moves to the file's row, and nothing selects it.
        let r = step(&mut ui, &["xdown"]);
        assert_eq!(r.over.and_then(|o| o.1), Some(rows[3].tile), "the fixture's reference moved it");
        assert_eq!(keyed_row(&ui, &p, &rows, &r, &["xdown"]), None);
        // Held there, nothing further; the hover reads the row under the pointer.
        let r = step(&mut ui, &[]);
        assert_eq!(r.over, None);
        let mut acted = Some(1);
        assert_eq!(follow_hover(&mut acted, Some(1), true), None);
        // A script's tap back onto the row under the stationary pointer selects it, though
        // the hover has nothing new to say.
        let r = step(&mut ui, &["xup"]);
        assert_eq!(r.over.and_then(|o| o.1), Some(rows[1].tile));
        assert_eq!(keyed_row(&ui, &p, &rows, &r, &[]), Some(1));
        // And a tap away selects where it went.
        let r = step(&mut ui, &["xdown"]);
        assert_eq!(keyed_row(&ui, &p, &rows, &r, &[]), Some(3));
    }

    #[test]
    fn the_description_area_belongs_to_the_tab_and_frame_it_was_found_for() {
        let ui = tree();
        let area = Rect::new(1.0, 2.0, 3.0, 4.0);
        let t = StatsTiles { ready: ui.menu(STATS_MENU), desc_area: Some(area), desc_for: Some((PAGE, Frame::Device)), ..Default::default() };
        assert_eq!(t.desc_area_for(PAGE, Frame::Device), Some(area));
        assert_eq!(t.desc_area_for(2, Frame::Device), None, "the tab changed this frame");
        assert_eq!(t.desc_area_for(PAGE, Frame::Screen), None, "the frame changed this frame");
        let not_ready = StatsTiles { ready: None, ..t };
        assert_eq!(not_ready.desc_area_for(PAGE, Frame::Device), None);
    }

    #[test]
    fn a_physical_press_acts_once_and_a_script_or_a_file_only_binding_acts() {
        for key in ["S", "W", "A", "D", "xup", "xdown", "xleft", "xright"] {
            assert!(!menus::key_click_acts(STATS_MENU, key, &[key]), "{key} by hand");
            assert!(menus::key_click_acts(STATS_MENU, key, &[]), "{key} tapped by a script");
        }
        assert!(!menus::key_click_acts(STATS_MENU, "E", &["E"]), "the Doctor's Bag's");
        assert!(menus::key_click_acts(STATS_MENU, "E", &[]), "tapped by a script");
        assert!(menus::key_click_acts(STATS_MENU, "Q", &["Q"]), "no hand-built use for it");
    }

    fn stats_page_up(tab: usize) -> PipBoy {
        let mut pip = PipBoy::default();
        pip.raise();
        pip.apply(PipBoyInput::Page(Page::Stats));
        pip.apply(PipBoyInput::Tab(tab));
        pip
    }

    /// Input acts on the page painted, the tab up, with nothing over it; only the tabs moved are tiled,
    /// the switch off nothing takes it; a menu over the page has the frame.
    #[test]
    fn the_page_takes_input_only_while_painted_up_and_uncovered() {
        use fnv_game::game::menus as codes;
        let mut state = ScriptState::over_no_plugins();
        let game = &mut state.rt.as_mut().unwrap().game;
        game.ui_files = Some(Box::new(MemSource::default()));
        game.ui_tree = tree();
        game.menu = Some(codes::STATS);
        let menu = game.ui_tree.menu(STATS_MENU).unwrap();
        let on = TileMenus { stats: true, ..Default::default() };
        let pip = stats_page_up(PAGE);
        let mut t = StatsTiles::default();
        assert_eq!(takes(&t, on, &pip, game, false), None, "not painted yet");
        t.shown = Some(menu);
        assert_eq!(takes(&t, on, &pip, game, false), Some(menu));
        assert_eq!(takes(&t, TileMenus::default(), &pip, game, false), None, "the switch off");
        assert_eq!(takes(&t, on, &stats_page_up(2), game, false), Some(menu), "Skills");
        assert_eq!(takes(&t, on, &stats_page_up(0), game, false), Some(menu), "Status");
        assert_eq!(takes(&t, on, &pip, game, true), None, "paused");
        assert_eq!(takes(&t, on, &PipBoy::default(), game, false), None, "down");
        game.menu = Some(codes::TUTORIAL);
        assert_eq!(takes(&t, on, &pip, game, false), None, "a menu over it");
        game.menu = None;
        game.ui_files = None;
        assert_eq!(takes(&t, on, &pip, game, false), None, "no menu files");
    }

    #[test]
    fn a_menu_over_the_page_is_told_from_the_pages_own_codes() {
        use fnv_game::game::menus as codes;
        assert!(modal_over(Some(codes::TUTORIAL)));
        assert!(modal_over(Some(codes::MESSAGE)));
        for own in [codes::STATS, codes::INVENTORY, codes::MAP] {
            assert!(!modal_over(Some(own)));
        }
        assert!(!modal_over(None));
    }

    #[test]
    fn only_the_tabs_moved_are_eligible() {
        let on = TileMenus { stats: true, ..Default::default() };
        for tab in TILED_TABS {
            assert!(eligible(on, &stats_page_up(tab)));
            assert!(!eligible(TileMenus::default(), &stats_page_up(tab)), "the switch off");
        }
        for tab in (0..5).filter(|t| !TILED_TABS.contains(t)) {
            assert!(!eligible(on, &stats_page_up(tab)));
        }
        let mut items = stats_page_up(PAGE);
        items.apply(PipBoyInput::Page(Page::Items));
        assert!(!eligible(on, &items));
        assert!(!eligible(on, &PipBoy::default()), "down");
    }

    #[test]
    fn what_is_shown_is_compared_without_a_copy_of_the_rows() {
        let s = snap();
        let at = At { tab: PAGE, reputation: false, status: 0, highlight: false, selected: 1, device: true, aimed: None };
        let none = PageWords::default();
        let shown = Shown::of(&s, at, &none);
        assert!(shown.same(&s, at, &none));
        assert!(!shown.same(&s, At { selected: 2, ..at }, &none));
        assert!(!shown.same(&s, At { device: false, ..at }, &none));
        assert!(!shown.same(&s, At { tab: 2, ..at }, &none));
        let mut hurt = s.clone();
        hurt.hp = Some((10.0, 40.0));
        assert!(!shown.same(&hurt, at, &none));
        let mut other = s.clone();
        other.special[6].name = "Other".into();
        assert!(!shown.same(&other, at, &none));
        // General's side and its key are what it shows too.
        let at = At { tab: GENERAL_TAB, reputation: false, status: 0, highlight: false, selected: 0, device: true, aimed: None };
        let s = general_snap();
        let key = PageWords { switch_hint: "<key>".into(), ..PageWords::default() };
        let shown = Shown::of(&s, at, &key);
        assert!(shown.same(&s, at, &key));
        assert!(!shown.same(&s, at, &none));
        let mut word = s.clone();
        word.karma = Some((5.0, "Another", "a_picture"));
        assert!(!shown.same(&word, at, &key));
    }

    #[test]
    fn projected_touch_release_uses_native_tab_route_once_and_swipe_or_overlay_cannot_click_it() {
        use bevy::{prelude::*,input::{InputPlugin,touch::{TouchInput,TouchPhase}},picking::pointer::{PointerInput,PointerAction}};
        use crate::pipboy::{self,touch_items::TouchItems};
        #[derive(Resource,Default)] struct Seen {tabs:usize,bevy_presses:usize,bevy_releases:usize}
        fn project(mut touch:ResMut<TouchItems>){touch.project(Some);}
        fn read(mut tabs:MessageReader<PipBoyInput>,mut pointers:MessageReader<PointerInput>,mut seen:ResMut<Seen>){
            seen.tabs+=tabs.read().count();
            for p in pointers.read().filter(|p|p.pointer_id==pipboy::touch_items::POINTER){match p.action{PointerAction::Press(_)=>seen.bevy_presses+=1,PointerAction::Release(_)=>seen.bevy_releases+=1,_=>{}}}
        }
        for mode in 0..4 {
            let (mut ui,mut view,mut tiles)=(tree(),view(),StatsTiles::default());
            let menu=run(&mut ui,&snap(),&mut view,true,&mut tiles).unwrap();tiles.shown=Some(menu);
            let mut script=ScriptState::over_no_plugins();{
                let game=&mut script.rt.as_mut().unwrap().game;game.ui_files=Some(Box::new(MemSource::default()));game.ui_tree=ui;game.menu=Some(fnv_game::game::menus::STATS);
            }
            let mut pip=stats_page_up(PAGE);pip.phase=pipboy::Phase::Up;
            let mut app=App::new();app.add_plugins((MinimalPlugins,InputPlugin))
                .init_resource::<bevy::picking::pointer::PointerMap>().init_resource::<TouchItems>()
                .init_resource::<pipboy::touch_scroll::WheelFrame>().init_resource::<pipboy::PipBoyAid>()
                .init_resource::<pipboy::PointerMoved>().init_resource::<Seen>()
                .add_message::<PointerInput>().add_message::<bevy::window::WindowEvent>()
                .add_message::<PipBoyInput>().add_message::<TabHovered>().add_message::<stats::UseHint>()
                .insert_resource(script).insert_resource(view).insert_resource(tiles).insert_resource(pip)
                .insert_resource(TileMenus{stats:true,..Default::default()});
            let camera=app.world_mut().spawn_empty().id();
            app.insert_resource(PipBoyScreen{image:Handle::default(),camera,on_model:false,pointer:None,scale:1.0})
                .add_systems(PreUpdate,pipboy::drive_pointer)
                .add_systems(Update,(feed,pipboy::apply_input,read).chain()).add_systems(Last,project);
            app.update();
            let point=Vec2::new(200.0,505.0);let mut event=TouchInput{window:Entity::PLACEHOLDER,id:92,phase:TouchPhase::Started,position:point,force:None};
            app.world_mut().resource_mut::<TouchItems>().event(&event,0);
            if mode==1 {event.phase=TouchPhase::Moved;event.position.y-=40.;app.world_mut().resource_mut::<TouchItems>().event(&event,20);}
            event.phase=TouchPhase::Ended;event.position=point;app.world_mut().resource_mut::<TouchItems>().event(&event,80);
            if mode==2 {app.insert_resource(pipboy::touch_items::OverlayCapture(true));}
            if mode==3 {app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);}
            for _ in 0..5 {app.update();}
            assert_eq!(app.world().resource::<PipBoy>().tab(),if mode==0 {3}else{PAGE});
            let seen=app.world().resource::<Seen>();assert_eq!(seen.tabs,usize::from(mode==0));
            assert_eq!((seen.bevy_presses,seen.bevy_releases),(0,0),"native target has exactly one action owner");
            if mode==3 {assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));}
        }
    }

    #[test]
    fn native_hint_release_cannot_retarget_a_limb_changed_during_a_real_repaint() {
        use bevy::{prelude::*, input::{InputPlugin, touch::{TouchInput, TouchPhase}}};
        use crate::pipboy::{self, touch_items::TouchItems};
        #[derive(Resource, Default)] struct Seen(Vec<stats::UseHint>);
        #[derive(Resource)] struct Repaint(bool);
        fn project(mut touch: ResMut<TouchItems>) { touch.project(Some); }
        fn change(mut change: ResMut<Repaint>, touch: Res<TouchItems>, mut view: ResMut<StatsView>, mut script: ResMut<ScriptState>, mut tiles: ResMut<StatsTiles>) {
            if change.0 && touch.pointer_frame().tap {
                change.0 = false; view.limb = Some(2);
                let menu = run_tab(&mut script.rt.as_mut().unwrap().game.ui_tree, &status_snap(), &mut view, STATUS_TAB, &mut tiles).unwrap();
                tiles.shown = Some(menu);
            }
        }
        fn read(mut asks: MessageReader<stats::UseHint>, mut seen: ResMut<Seen>) { seen.0.extend(asks.read().copied()); }
        let (mut ui, mut view, mut tiles) = (tree(), view(), status_tiles()); view.limb = Some(1);
        let menu = run_tab(&mut ui, &status_snap(), &mut view, STATUS_TAB, &mut tiles).unwrap(); tiles.shown = Some(menu);
        let point = Vec2::new(705., 205.);
        assert!(matches!(stats_tile(&ui, &paths(&ui, STATUS_TAB, None).unwrap(), &[], fnv_ui::input::hit(&ui, &[menu], (705.,205.), &no_measure).unwrap().tile), Some(StatsTile::Hint(0))));
        let mut script = ScriptState::over_no_plugins(); { let game = &mut script.rt.as_mut().unwrap().game; game.ui_files = Some(Box::new(MemSource::default())); game.ui_tree = ui; game.menu = Some(fnv_game::game::menus::STATS); }
        let mut pip = stats_page_up(STATUS_TAB); pip.phase = pipboy::Phase::Up;
        let mut app = App::new(); app.add_plugins((MinimalPlugins, InputPlugin))
            .init_resource::<TouchItems>().init_resource::<pipboy::PipBoyAid>().init_resource::<pipboy::PointerMoved>()
            .init_resource::<Seen>().insert_resource(Repaint(true))
            .add_message::<PipBoyInput>().add_message::<TabHovered>().add_message::<stats::UseHint>()
            .insert_resource(script).insert_resource(view).insert_resource(tiles).insert_resource(pip)
            .insert_resource(TileMenus { stats: true, ..Default::default() });
        let camera = app.world_mut().spawn_empty().id();
        app.insert_resource(PipBoyScreen { image: Handle::default(), camera, on_model: false, pointer: None, scale: 1. })
            .add_plugins(pipboy::touch_scroll::TouchPointerPlugin)
            .add_systems(Update, (change, feed, read).chain()).add_systems(Last, project);
        app.update();
        let send = |app: &mut App, id, phase| { assert!(app.world_mut().resource_mut::<TouchItems>().event(&TouchInput { window: Entity::PLACEHOLDER, id, phase, position: point, force: None }, 100)); };
        send(&mut app, 201, TouchPhase::Started); app.update(); app.update();
        send(&mut app, 201, TouchPhase::Ended); for _ in 0..4 { app.update(); }
        assert!(!app.world().resource::<Repaint>().0); assert!(app.world().resource::<Seen>().0.is_empty());
        send(&mut app, 202, TouchPhase::Started); send(&mut app, 202, TouchPhase::Ended); for _ in 0..4 { app.update(); }
        assert_eq!(app.world().resource::<Seen>().0, vec![stats::UseHint { hint: stats::model::STIMPAK, limb: Some(2) }]);
    }

    #[test]
    fn cnd_aimed_limb_draws_selection_box_on_bar_or_crippled_word() {
        let (mut ui, mut v, mut t) = (tree(), view(), status_tiles());
        let s = status_snap();
        let st = status_paths(&ui);
        // Vanilla stats_menu.xml draws each pct bracket from a DDS file.
        ui.set_trait(st.limbs[1].pct, "filename", Value::Str("pics\\a.dds".into()));
        // No limb aimed: no Look::Box edges added for limb bars.
        run_tab(&mut ui, &s, &mut v, STATUS_TAB, &mut t).unwrap();
        let box_count = |pieces: &[Piece], tile: TileId| pieces.iter().filter(|p| p.tile == tile && p.look == Look::Box).count();
        assert_eq!(box_count(&t.pieces, st.limbs[1].pct), 0);
        assert_eq!(box_count(&t.pieces, st.limbs[2].crippled), 0);
        // Aim a hurt limb with a visible bar (torso = 1, condition 0.4): 4 box edges around pct.
        v.choose_limb(1);
        run_tab(&mut ui, &s, &mut v, STATUS_TAB, &mut t).unwrap();
        assert_eq!(box_count(&t.pieces, st.limbs[1].pct), 4);
        assert_eq!(box_count(&t.pieces, st.limbs[2].crippled), 0);
        // Hide limb 2's pct bar (simulating crippled condition == 0) and aim limb 2: 4 box edges around crippled word.
        ui.set_trait(st.limbs[2].pct, "visible", flag(false));
        v.hover = Some(stats::Spot::Limb(2));
        run_tab(&mut ui, &s, &mut v, STATUS_TAB, &mut t).unwrap();
        assert_eq!(box_count(&t.pieces, st.limbs[1].pct), 0);
        assert_eq!(box_count(&t.pieces, st.limbs[2].crippled), 4);
    }

    #[test]
    fn feed_routes_hardcore_limb_clicks_and_drbag_hint_to_selected_limb() {
        let run_case = |hardcore: bool, chosen: Option<usize>, click_at: Vec2| -> Vec<stats::UseHint> {
            let mut app = App::new();
            let mut state = ScriptState::over_no_plugins();
            let game = &mut state.rt.as_mut().unwrap().game;
            game.set_hardcore(hardcore);
            game.ui_files = Some(Box::new(MemSource::default()));
            game.ui_tree = tree();
            let mut s = status_snap();
            s.hardcore = hardcore;
            s.hints.lit = [true, true, false, false];
            let mut v = view();
            v.limb = chosen;
            let mut t = status_tiles();
            let menu = run_tab(&mut game.ui_tree, &s, &mut v, STATUS_TAB, &mut t).unwrap();
            t.ready = Some(menu);
            t.shown = Some(menu);
            let mut mouse = ButtonInput::<MouseButton>::default();
            mouse.press(MouseButton::Left);
            app.insert_resource(state)
                .insert_resource(TileMenus { stats: true, ..Default::default() })
                .insert_resource(stats_page_up(STATUS_TAB))
                .insert_resource(PipBoyScreen { image: Handle::default(), camera: Entity::PLACEHOLDER, on_model: true, pointer: Some(click_at), scale: 1.0 })
                .insert_resource(PipBoyAid::default())
                .insert_resource(t)
                .insert_resource(v)
                .insert_resource(mouse)
                .init_resource::<ButtonInput<KeyCode>>()
                .add_message::<PipBoyInput>()
                .add_message::<TabHovered>()
                .add_message::<stats::UseHint>()
                .add_systems(Update, feed);
            app.update();
            app.world_mut().resource_mut::<Messages<stats::UseHint>>().drain().collect()
        };
        // Fixture places limb 2's bar at x = 700 + 40 = 740..758, y = 350..368; drbag button at x = 810..828, y = 200..218.
        let limb2_bar = Vec2::new(745.0, 355.0);
        let drbag_btn = Vec2::new(815.0, 205.0);
        // Outside Hardcore: clicking limb 2 uses Stimpak on limb 2; clicking drbag uses bag on body even with limb chosen.
        assert_eq!(run_case(false, None, limb2_bar), vec![stats::UseHint { hint: stats::model::STIMPAK, limb: Some(2) }]);
        assert_eq!(run_case(false, Some(2), drbag_btn), vec![stats::UseHint { hint: stats::model::DOCTORS_BAG, limb: None }]);
        // In Hardcore: clicking limb 2 uses Doctor's Bag on limb 2; clicking drbag uses bag on chosen limb (or body if none).
        assert_eq!(run_case(true, None, limb2_bar), vec![stats::UseHint { hint: stats::model::DOCTORS_BAG, limb: Some(2) }]);
        assert_eq!(run_case(true, Some(2), drbag_btn), vec![stats::UseHint { hint: stats::model::DOCTORS_BAG, limb: Some(2) }]);
        assert_eq!(run_case(true, None, drbag_btn), vec![stats::UseHint { hint: stats::model::DOCTORS_BAG, limb: None }]);
    }

    #[test]
    fn stationary_pointer_on_limb_does_not_override_cycled_limb_in_feed() {
        let mut app = App::new();
        let mut state = ScriptState::over_no_plugins();
        let game = &mut state.rt.as_mut().unwrap().game;
        game.set_hardcore(true);
        game.ui_files = Some(Box::new(MemSource::default()));
        game.ui_tree = tree();
        let mut s = status_snap();
        s.hardcore = true;
        let mut v = view();
        let mut t = status_tiles();
        let menu = run_tab(&mut game.ui_tree, &s, &mut v, STATUS_TAB, &mut t).unwrap();
        t.ready = Some(menu);
        t.shown = Some(menu);
        let limb2_bar = Vec2::new(745.0, 355.0);
        app.insert_resource(state)
            .insert_resource(TileMenus { stats: true, ..Default::default() })
            .insert_resource(stats_page_up(STATUS_TAB))
            .insert_resource(PipBoyScreen { image: Handle::default(), camera: Entity::PLACEHOLDER, on_model: true, pointer: Some(limb2_bar), scale: 1.0 })
            .insert_resource(PipBoyAid::default())
            .insert_resource(t)
            .insert_resource(v)
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_message::<PipBoyInput>()
            .add_message::<TabHovered>()
            .add_message::<stats::UseHint>()
            .add_systems(Update, feed);
        // Frame 1: pointer moves onto limb 2 -> hover = Limb(2).
        app.update();
        assert_eq!(app.world().resource::<StatsView>().aimed(), Some(2));
        // Player cycles limb with X (advances to 3 and clears hover).
        app.world_mut().resource_mut::<StatsView>().cycle_limb();
        assert_eq!(app.world().resource::<StatsView>().aimed(), Some(3));
        // Frame 2: pointer has not moved -> feed must NOT reclaim hover back to limb 2.
        app.update();
        assert_eq!(app.world().resource::<StatsView>().aimed(), Some(3));
        // Frame 3: pointer moves slightly on limb 2 -> hover follows pointer again.
        app.world_mut().resource_mut::<PipBoyScreen>().pointer = Some(Vec2::new(746.0, 355.0));
        app.update();
        assert_eq!(app.world().resource::<StatsView>().aimed(), Some(2));
    }
}
