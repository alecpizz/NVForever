//! The Pip-Boy's DATA page drawn from `menus\main\map_menu.xml`, behind `[UI] TileMenus=MapMenu`
//! [design: ui-tile-tree.md §6.2 step 3; the contract: docs/spec-notes/pipboy.md §5.3.1; issue
//! #1513]. On by default [design, #2215; super::vanilla::TileMenus::for_run]. Off, none
//! of this runs and the hand-built page (`crate::pipboy::data`) prepares and draws as it
//! always did.
//!
//! - **The tabs it draws** ([`TILED_TABS`]): Quests, Misc (the notes or the challenges, a note's
//!   text, picture or audio), Radio (the stations, the waveform's frame from the file with the
//!   hand-built trace painted over it, `crate::pipboy::data::trace_over_tiles`) and the World Map
//!   (the picture placed from the model's viewport, `crate::pipboy::data::MapView`, its markers,
//!   quest markers, custom marker and dotted lines as the file's template rows kept by record, the
//!   player's arrow and the highlight box; the question boxes painted over it,
//!   `crate::pipboy::data::popup_over_tiles`), and the Local Map likewise (its frame the outline's
//!   grid, its picture the outline as the runtime picture `nvf:local_map`, its doors, markers,
//!   quest markers and custom marker, no dotted line).
//! - **The one decision** ([`MapTiles::ready`]): the switch on, DATA showing on a tab it draws, the
//!   page prepared for that tab (`crate::pipboy::data::prepare`, before this), the file's tiles
//!   there ([`built`]), the tabs made and a usable measured layout ([`usable`]). A question box up
//!   does not change it: the box is drawn over the page, and while it is up it takes the page's
//!   tile input ([`MapTiles::popup_owns`]); Tab and Esc still close the Pip-Boy.
//!   Painting (`super::draw`), the frame (`crate::pipboy::frame::draw`) and input read it and
//!   nothing else; [`settle`] publishes it before them. A pass that fails leaves the whole
//!   hand-built page, says why once, and after [`FAILING_PASSES`] in a row is tried again only when
//!   what it would show or the tree changes ([`TreeMark`]).
//! - **The rules stay the hand-built page's**: what the page shows is `pipboy::data`'s prepared view
//!   ([`crate::pipboy::data::Prepared`]); this module writes it as the file's "set in code" traits
//!   and says what a tile stands for; the player's acts go back through `DataState`'s acts as the
//!   hand-built nodes' do. Only each list's scroll is its own (`super::list`), which the model keeps
//!   in rows, as ITEMS' list (`super::inventory_menu`).
//! - **Exactly one presentation publishes** what the page's rules read of the drawing: how many
//!   rows show, how far the right pane (the objectives, a text) scrolls and its window
//!   ([`DataState::publish_drawn`]), from
//!   a pass that went through only.
//! - **The hints** are the hand-built page's set (`data::tab_buttons`, #1897 / #1952), bound to the
//!   file's A, X and Y tiles by what each does, never by its place in the list ([`Slot::of`]).
//! - **Input** ([`feed`], before the scripts): the pointer is the Pip-Boy's own in canvas units
//!   (`crate::pipboy::PipBoyScreen::pointer`), read and never written; ignored with Controller
//!   chosen, as on the hand-built page. The keys, the wheel and the sticks stay `pipboy::data`'s
//!   ([`HAND_BUILT_KEYS`]); a file's binding acts for a script's tap, not for a physical press.
//!   On the World Map the tiles take the tabs and the hints only: the map's pan, zoom, highlight
//!   box, picking and travel stay the model's, read in the window this page publishes
//!   (`DataState::publish_map`), so one owner starts travel.

use super::draw::{Fit, Look, Piece, Source};
use super::input::{frame_keys, log_report, Devices};
use super::list::{self, BarPiece, ListView, RowTile, Wanted};
use super::menus::{self, SizesWritten, Tiled};
use super::stats_menu::{self, find_named, modal_over, resolve_tile, shows_whole, FlatChanges, PageFonts};
use super::vanilla::TileMenus;
use crate::pipboy::data::{self, Act, BarPart, Body, ButtonKind, DataState, Mark, Prepared, Right};
use crate::pipboy::sounds::TabHovered;
use crate::pipboy::{Frame, Page, PipBoy, PipBoyAid, PipBoyInput, PipBoyScreen};
use crate::script::ScriptState;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use fnv_ui::{TileId, Ui, Value};
use std::collections::{HashMap, HashSet};

/// The menu's name [documented: map_menu.xml].
pub const MAP_MENU: &str = "MapMenu";
/// Test aid: a tile or template of the file to take as missing, as `stats_menu::CUT_AID`:
/// `<name>`, or `<name>@<seconds>` to stop at that time, for the page's recovery.
pub const CUT_AID: &str = "NVF_PLAY_TEST_MAP_TILE_CUT";

macro_rules! glow {
    ($path:literal) => {
        concat!("MapMenu/GLOW_BRANCH/", $path)
    };
}
macro_rules! main_rect {
    ($path:literal) => {
        concat!("MapMenu/GLOW_BRANCH/MM_MainRect/", $path)
    };
}

// The tiles the model writes and the input reads, by path [documented: map_menu.xml].
const BACKGROUND: &str = glow!("MM_Background");
const TABLINE: &str = glow!("MM_Tabline");
const TITLE: &str = main_rect!("MM_HeadlineRect/MM_Headline_Title");
const LINE: &str = main_rect!("MM_HeadlineRect/MM_Headline_h2");
const PLACE: &str = main_rect!("MM_HeadlineRect/MM_Headline_LocationInfo");
const TIME: &str = main_rect!("MM_HeadlineRect/MM_Headline_TimeDateInfo");
const QUESTS_LIST: &str = main_rect!("MM_QuestsList");
const CHALLENGE_TYPE: &str = main_rect!("MM_Headline_ChallengeType");
const BUTTON_RECT: &str = main_rect!("MM_ButtonRect");
const DATA_RECT: &str = main_rect!("MM_DataRect");
const OBJECTIVES: &str = main_rect!("MM_DataRect/MM_QuestObjectivesList");
const NOTES_LIST: &str = main_rect!("MM_NotesList");
const CHALLENGE_LIST: &str = main_rect!("MM_ChallengeList");
const STATIONS_LIST: &str = main_rect!("MM_RadioStationList");
/// The World Map's window, its picture (a hotrect whose children are the markers), the player's
/// arrow, and the highlight box in a window of its own [documented: map_menu.xml].
const WORLD_CLIP: &str = main_rect!("MM_WorldMap_ClipWindow");
const WORLD_PICTURE: &str = main_rect!("MM_WorldMap_ClipWindow/MM_WorldMap_ParentImage");
const WORLD_CURSOR: &str = main_rect!("MM_WorldMap_ClipWindow/MM_WorldMapCursor");
const HIGHLIGHT_CLIP: &str = main_rect!("MM_Highlight_ClipWindow");
const HIGHLIGHT_BOX: &str = main_rect!("MM_Highlight_ClipWindow/MM_MapHighlightBox");
const HIGHLIGHT_NAME: &str = main_rect!("MM_Highlight_ClipWindow/MM_MapHighlightBox/MM_HighlightLocationName");
/// The World Map's templates and the rows they make [documented: map_menu.xml].
const MARKER_TEMPLATE: &str = "MapMarkerTemplate";
pub(super) const MARKER_ROW: &str = "MapMarker";
const QUEST_TEMPLATE: &str = "WorldMapQuestMarkerTemplate";
const QUEST_ROW: &str = "WorldMapQuestMarker";
const NODE_TEMPLATE: &str = "WaypointTemplate";
const NODE_ROW: &str = "WaypointIcon";
const PATH_TEMPLATE: &str = "PathLineTemplate";
const PATH_ROW: &str = "PathLine";
/// The node the dotted lines start from, at the player's arrow: named apart from the ends' so its
/// lines can be made under it by path [design].
const START_NODE_ROW: &str = "WaypointStart";
const WORLD_TEMPLATES: [&str; 4] = [MARKER_TEMPLATE, QUEST_TEMPLATE, NODE_TEMPLATE, PATH_TEMPLATE];
/// The Local Map's window, its picture's hotrect (no picture in the file: set in code, and hidden
/// until code shows it) and the player's arrow, and its quest markers' template, which keeps them
/// inside the window [documented: map_menu.xml].
const LOCAL_CLIP: &str = main_rect!("MM_LocalMap_ClipWindow");
const LOCAL_PICTURE: &str = main_rect!("MM_LocalMap_ClipWindow/MM_LocalMap_ParentImage");
const LOCAL_CURSOR: &str = main_rect!("MM_LocalMap_ClipWindow/MM_LocalMapCursor");
const LOCAL_QUEST_TEMPLATE: &str = "LocalMapQuestMarkerTemplate";
const LOCAL_QUEST_ROW: &str = "LocalMapQuestMarker";
const LOCAL_TEMPLATES: [&str; 2] = [MARKER_TEMPLATE, LOCAL_QUEST_TEMPLATE];
/// The Local Map's outline picture's runtime name (`super::runtime_images`).
const LOCAL_IMAGE: &str = "local_map";

/// The tiles one of the maps is written into [documented: map_menu.xml].
struct MapPaths {
    /// For the log and the reasons.
    name: &'static str,
    clip: &'static str,
    picture: &'static str,
    cursor: &'static str,
    quest_template: &'static str,
    quest_row: &'static str,
    /// The other map's window, hidden while this one shows.
    other_clip: &'static str,
}

const WORLD_PATHS: MapPaths = MapPaths { name: "world map", clip: WORLD_CLIP, picture: WORLD_PICTURE, cursor: WORLD_CURSOR, quest_template: QUEST_TEMPLATE, quest_row: QUEST_ROW, other_clip: LOCAL_CLIP };
const LOCAL_PATHS: MapPaths = MapPaths { name: "local map", clip: LOCAL_CLIP, picture: LOCAL_PICTURE, cursor: LOCAL_CURSOR, quest_template: LOCAL_QUEST_TEMPLATE, quest_row: LOCAL_QUEST_ROW, other_clip: WORLD_CLIP };

/// Radio's waveform panel: its frame (the tick meters, the centre line) and the image the
/// original's code animates in it [documented: map_menu.xml `MM_WaveformRect`, `MM_Waveform`].
const WAVE_RECT: &str = main_rect!("MM_WaveformRect");
const WAVE_IMAGE: &str = main_rect!("MM_WaveformRect/MM_Waveform");
/// A note's text, its window and its scroll bar; a note's picture; an audio note's time and meter
/// [documented: map_menu.xml `MM_DataRect`].
const TEXT_RECT: &str = main_rect!("MM_DataRect/MM_DataTextRect");
const TEXT_BAR: &str = main_rect!("MM_DataRect/MM_DataTextRect/MM_TextScrollbar");
const DATA_TEXT: &str = main_rect!("MM_DataRect/MM_DataTextRect/MM_DataText");
const DATA_IMAGE: &str = main_rect!("MM_DataRect/MM_DataImage");
const AUDIO_INFO: &str = main_rect!("MM_DataRect/MM_DataAudioInfo");
const AUDIO_TIME: &str = main_rect!("MM_DataRect/MM_DataAudioInfo/MM_AudioTimeRemaining");
/// The lists' own names, for what a click is on (`list::hit`).
const QUESTS_NAME: &str = "MM_QuestsList";
const NOTES_NAME: &str = "MM_NotesList";
const CHALLENGES_NAME: &str = "MM_ChallengeList";
const STATIONS_NAME: &str = "MM_RadioStationList";
const OBJECTIVES_NAME: &str = "MM_QuestObjectivesList";
const TEXT_RECT_NAME: &str = "MM_DataTextRect";
const ROW_TEMPLATE: &str = "MM_ListMarkerTemplate";
const ROW: &str = "MM_ListItem";
const ROW_TEXT: &str = "ListItemText";
const ROW_MARK: &str = "MM_Template_ItemMarker";
const TAB_TEMPLATE: &str = "TabButtonTemplate";
const TAB_ROW: &str = "TabButton";
/// Inside a tab, as ITEMS' (`super::inventory_menu`) [hypothesis: the same prefab].
const TAB_INNER: &str = "stb_text_box";
/// `MM_DataRect`'s `_ItemType`: a note's text, its picture, an audio note, the objectives
/// [documented: the `visible` of `MM_DataTextRect`, `MM_DataImage`, `MM_DataAudioInfo` and
/// `MM_QuestObjectivesList`].
const ITEM_TEXT: f64 = 1.0;
const ITEM_IMAGE: f64 = 2.0;
const ITEM_AUDIO: f64 = 3.0;
const ITEM_OBJECTIVES: f64 = 4.0;
/// A done row's `_TextAlpha`: half strength, as the hand-built page dims it (`data::DONE_ALPHA`)
/// [design]; the template's own is 255 [documented].
const DONE_ALPHA: f64 = 128.0;
const FULL_ALPHA: f64 = 255.0;

/// The tabs drawn from the file: all five [design, #1513]. The Local Map's picture is the outline
/// NVForever makes (a runtime picture). Radio's waveform is the file's frame with the hand-built
/// trace painted over it: the file's `MM_Waveform` is an image its code animates (which animation
/// is not known), which stays hidden. The maps' picking stays the model's.
pub const TILED_TABS: [usize; 5] = [data::LOCAL, data::WORLD, data::QUESTS, data::NOTES, data::RADIO];

/// The tab's list the page shows: Quests' quests, Misc's notes or its challenges, Radio's stations
/// [documented: the lists' `visible`; `MM_NotesList` and `MM_ChallengeList` both show on Misc, so
/// the code hides the one not shown, design].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum ListKind {
    #[default]
    Quests,
    Notes,
    Challenges,
    Stations,
}

impl ListKind {
    const ALL: [ListKind; 4] = [ListKind::Quests, ListKind::Notes, ListKind::Challenges, ListKind::Stations];

    fn index(self) -> usize {
        self as usize
    }

    fn path(self) -> &'static str {
        match self {
            ListKind::Quests => QUESTS_LIST,
            ListKind::Notes => NOTES_LIST,
            ListKind::Challenges => CHALLENGE_LIST,
            ListKind::Stations => STATIONS_LIST,
        }
    }

    fn name(self) -> &'static str {
        match self {
            ListKind::Quests => QUESTS_NAME,
            ListKind::Notes => NOTES_NAME,
            ListKind::Challenges => CHALLENGES_NAME,
            ListKind::Stations => STATIONS_NAME,
        }
    }

    fn tab(self) -> usize {
        match self {
            ListKind::Quests => data::QUESTS,
            ListKind::Notes | ListKind::Challenges => data::NOTES,
            ListKind::Stations => data::RADIO,
        }
    }

    /// Misc's two lists, which its toggle swaps.
    fn misc(self) -> bool {
        matches!(self, ListKind::Notes | ListKind::Challenges)
    }

    /// For the log and the reasons: the list's rows and one of them.
    fn words(self) -> (&'static str, &'static str) {
        match self {
            ListKind::Quests => ("quests", "quest"),
            ListKind::Notes => ("notes", "note"),
            ListKind::Challenges => ("challenges", "challenge"),
            ListKind::Stations => ("stations", "station"),
        }
    }
}

/// The file's three hint tiles [documented: map_menu.xml `MM_ButtonRect`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    A,
    X,
    Y,
}

const BUTTONS: [(&str, Slot); 3] = [("MM_ButtonA", Slot::A), ("MM_ButtonX", Slot::X), ("MM_ButtonY", Slot::Y)];

impl Slot {
    /// The tile a hint stands on, by what it does: Enter's work on A (Travel, Set Active, Play,
    /// Tune), the challenges' toggle on Y (the root's Y and R bind it), the rest on X (Place Marker,
    /// Show Location, the notes' filter, Next Challenge; the root's X binds it) [documented: the
    /// tiles' `_Title_*` and the root's bindings].
    pub(crate) fn of(kind: ButtonKind) -> Slot {
        if kind.is_a() {
            Slot::A
        } else if kind == ButtonKind::Challenges {
            Slot::Y
        } else {
            Slot::X
        }
    }
}

fn button_path(name: &str) -> String {
    format!("{BUTTON_RECT}/{name}")
}

const BAR_PIECES: [&str; 5] = [list::MARKER, list::UP, list::DOWN, list::PAGE_UP, list::PAGE_DOWN];

/// Every path the model writes or input reads on tab `tab` (`None`: what every tab needs): a file
/// short of Misc's tiles still draws Quests from the file, and the other way round.
fn required_paths(tab: Option<usize>) -> Vec<String> {
    let mut paths: Vec<String> = [MAP_MENU, BACKGROUND, TABLINE, TITLE, LINE, PLACE, TIME, BUTTON_RECT, DATA_RECT].iter().map(|s| (*s).to_string()).collect();
    let lists: &[&str] = match tab {
        Some(data::QUESTS) => &[QUESTS_LIST],
        Some(data::NOTES) => &[NOTES_LIST, CHALLENGE_LIST],
        Some(data::RADIO) => &[STATIONS_LIST],
        _ => &[],
    };
    for l in lists {
        paths.push((*l).to_string());
        paths.push(format!("{l}/{}", list::HIGHLIGHT));
        paths.extend(BAR_PIECES.iter().map(|piece| format!("{l}/{}/{piece}", list::SCROLLBAR)));
    }
    match tab {
        Some(data::QUESTS) => {
            paths.push(OBJECTIVES.to_string());
            paths.extend(BAR_PIECES.iter().map(|piece| format!("{OBJECTIVES}/{}/{piece}", list::SCROLLBAR)));
        }
        Some(data::NOTES) => {
            paths.extend([TEXT_RECT, TEXT_BAR, DATA_TEXT, DATA_IMAGE, AUDIO_INFO, AUDIO_TIME].iter().map(|s| (*s).to_string()));
            paths.extend(BAR_PIECES.iter().map(|piece| format!("{TEXT_BAR}/{piece}")));
        }
        // The waveform's frame; its animated image is hidden when there and needs nothing.
        Some(data::RADIO) => paths.push(WAVE_RECT.to_string()),
        Some(data::WORLD) => paths.extend([WORLD_CLIP, WORLD_PICTURE, WORLD_CURSOR, HIGHLIGHT_CLIP, HIGHLIGHT_BOX, HIGHLIGHT_NAME].iter().map(|s| (*s).to_string())),
        Some(data::LOCAL) => paths.extend([LOCAL_CLIP, LOCAL_PICTURE, LOCAL_CURSOR, HIGHLIGHT_CLIP, HIGHLIGHT_BOX, HIGHLIGHT_NAME].iter().map(|s| (*s).to_string())),
        _ => {}
    }
    paths.extend(BUTTONS.iter().map(|(name, _)| button_path(name)));
    paths
}

/// Whether the file has every tile and template the page needs on tab `tab`; a file a mod
/// restructured keeps the hand-built page whole. `cut`: [`CUT_AID`].
pub fn built(ui: &Ui, cut: Option<&str>, tab: Option<usize>) -> Tiled {
    let paths = required_paths(tab);
    let templates: &[&str] = match tab {
        Some(data::WORLD) => &WORLD_TEMPLATES,
        Some(data::LOCAL) => &LOCAL_TEMPLATES,
        _ => &[],
    };
    if let Some(cut) = cut {
        if cut.eq_ignore_ascii_case(ROW_TEMPLATE) || cut.eq_ignore_ascii_case(TAB_TEMPLATE) || templates.iter().any(|t| cut.eq_ignore_ascii_case(t)) {
            return Tiled::No(format!("{cut} missing"));
        }
        if let Some(path) = paths.iter().find(|p| p.rsplit('/').next().is_some_and(|n| n.eq_ignore_ascii_case(cut))) {
            return Tiled::No(format!("{path} missing"));
        }
    }
    if let Some(missing) = paths.iter().find(|p| resolve_tile(ui, p).is_none()) {
        return Tiled::No(format!("{missing} missing"));
    }
    let rows = list::template_has(ui, MAP_MENU, ROW_TEMPLATE, ROW, &[("text", ROW_TEXT), ("image", ROW_MARK)]);
    if !rows.yes() {
        return rows;
    }
    let has_template = |name: &str| ui.menu(MAP_MENU).is_some_and(|m| ui.tile(m).templates.iter().any(|t| t.name.eq_ignore_ascii_case(name)));
    if !has_template(TAB_TEMPLATE) {
        return Tiled::No(format!("{TAB_TEMPLATE} missing"));
    }
    if let Some(missing) = templates.iter().find(|t| !has_template(t)) {
        return Tiled::No(format!("{missing} missing"));
    }
    Tiled::Yes
}

/// A World Map marker as written: its reference, its place on the picture (texture
/// coordinates), its icon, whether it is the one the highlight box is on, and its name.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MarkerShown {
    pub form: u32,
    pub uv: [f32; 2],
    pub icon: String,
    pub selected: bool,
    pub name: String,
    /// Its reputation line, the World Map's markers' with one.
    pub detail: Option<String>,
}

/// What a quest-marker row stands for: an active quest's target (its reference), or the player's
/// custom marker (drawn from the same template [hypothesis: the original's capture shows it as
/// wide as the quest markers, pipboy.md §5.3]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuestRow {
    Target(u32),
    Custom,
}

/// What a waypoint node or a dotted line stands for: the start at the player's arrow, or a line's
/// far end (the custom marker, a quest target by reference).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NodeRow {
    Start,
    End(data::marker::LineTo, u32),
}

/// The World Map as written (`data::WorldParts` and the model's highlight box): the picture's
/// file, place and size in the window's units, the window the model read it in (canvas units),
/// the magnification and the picture's size at 1, the markers, the quest markers, the custom
/// marker, the player's arrow (texture coordinates and heading), the dotted lines' ends, and the
/// highlight box (the window's units), its name and the line under it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WorldShown {
    /// The Local Map (`MM_LocalMap_*`, its picture the outline as a runtime image) or the World
    /// Map.
    pub local: bool,
    /// The Local Map's outline picture and its size in texels, once made (`data::WorldParts`).
    pub outline: Option<(Handle<Image>, Vec2)>,
    /// The model's viewport, in the window it was made in (`window`).
    pub view: data::MapView,
    pub texture: String,
    pub picture: Rect,
    pub window: Rect,
    pub mag: f32,
    pub usable: Vec2,
    pub markers: Vec<MarkerShown>,
    pub quests: Vec<(u32, [f32; 2])>,
    pub custom: Option<[f32; 2]>,
    pub arrow: Option<([f32; 2], f32)>,
    pub ends: Vec<(data::marker::LineTo, u32, [f32; 2])>,
    pub cursor: Vec2,
    /// Where the highlight box was put before it took a marker's place (`data::WorldParts::want`).
    pub want: Vec2,
    pub label: Option<String>,
    pub detail: Option<String>,
    /// The World Map's companions: each teammate's FormID, texture coordinates, display
    /// name, and knocked-out state; empty on the Local Map.
    pub companions: Vec<(u32, [f32; 2], String, bool)>,
    /// Font number for companion names (2 by default, 1 with `bCompanionsUseLocationFontOnMap`).
    pub companion_font: u8,
}

impl WorldShown {
    /// The map the model gathered, `m`: none for a map without its viewport (no worldspace map,
    /// no place). The Local Map's picture is its outline, by its runtime name once made.
    pub(crate) fn of(m: &data::MapShown) -> Option<WorldShown> {
        let w = m.world.as_ref()?;
        let local = w.view.local.is_some();
        let texture = if local { w.outline.as_ref().map(|_| super::runtime_images::filename(LOCAL_IMAGE)).unwrap_or_default() } else { w.texture.clone() };
        // The Local Map's rows are every record here; the World Map's are its marks.
        let markers = w
            .rows
            .as_deref()
            .unwrap_or(m.marks())
            .iter()
            .filter_map(|mk| Some(MarkerShown { form: mk.form, uv: mk.uv?, icon: mk.picture(), selected: mk.highlighted, name: mk.name.clone(), detail: mk.detail.clone() }))
            .collect();
        let (cursor, label, detail) = m.highlight();
        Some(WorldShown {
            local,
            outline: w.outline.clone(),
            view: w.view,
            texture,
            picture: w.view.picture(),
            window: w.view.window,
            mag: w.mag,
            usable: if w.mag > 0.0 { w.view.size / w.mag } else { w.view.size },
            markers,
            quests: w.quests.clone(),
            custom: w.custom,
            arrow: w.arrow,
            ends: w.ends.clone(),
            cursor,
            want: w.want,
            label: label.map(str::to_string),
            detail: detail.map(str::to_string),
            companions: if local { Vec::new() } else { w.companions.clone() },
            companion_font: w.companion_font,
        })
    }

    /// The same map in the window `window` (canvas units) the page laid out, when the model
    /// gathered it in another (the first frame drawn from the file, a window a script moved): the
    /// same point at the window's middle, the picture's size kept; the highlight box on the marker
    /// it is on, else at its place on the canvas, kept in the window. The rows' texture coordinates
    /// do not change [design: so the drawing and the window published for the next frame's pointer
    /// agree from the first frame].
    ///
    /// The highlight box is put where it was put on the canvas (the pointer's place, the
    /// controller's cursor), kept in the window, and takes again the nearest marker within reach
    /// there, as the model picks (`data::nearest`): it may be another marker than before, or none.
    /// Returns the marker picked; no input is taken again.
    fn refit(&mut self, window: Rect) -> Option<u32> {
        let picked = |s: &WorldShown| s.markers.iter().find(|m| m.selected).map(|m| m.form);
        if self.window == window {
            return picked(self);
        }
        let view = self.view.with_window(window);
        let want = (self.window.min + self.want - window.min).clamp(Vec2::ZERO, window.size());
        let pick = data::nearest(self.markers.iter().map(|m| view.to_window(m.uv)), want);
        let before = picked(self);
        for (i, m) in self.markers.iter_mut().enumerate() {
            m.selected = Some(i) == pick;
        }
        self.cursor = pick.map_or(want, |i| view.to_window(self.markers[i].uv));
        self.label = pick.map(|i| self.markers[i].name.clone());
        // Another marker picked: its own reputation line (none for one without).
        if picked(self) != before {
            self.detail = pick.and_then(|i| self.markers[i].detail.clone());
        }
        self.want = want;
        self.picture = view.picture();
        self.window = window;
        self.view = view;
        picked(self)
    }

    /// The highlight box's title: the name, and a marker's reputation on the line under it
    /// [observed: run08, the original shows the reputation on a second line; design: its words
    /// are the hand-built page's, `data::model::reputation_line`].
    fn title(&self) -> String {
        match (&self.label, &self.detail) {
            (Some(l), Some(d)) => format!("{l}\n{d}"),
            (Some(l), None) => l.clone(),
            _ => String::new(),
        }
    }

    /// The tiles this map is written into.
    fn paths(&self) -> &'static MapPaths {
        if self.local { &LOCAL_PATHS } else { &WORLD_PATHS }
    }
}

/// A list row as written: what it stands for, its words, its square filled and whether it is done.
#[derive(Clone, Debug, PartialEq)]
pub struct RowShown {
    pub key: u32,
    pub text: String,
    pub filled: bool,
    pub done: bool,
}

/// A hint as written on its tile.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HintShown {
    pub slot: Slot,
    pub kind: ButtonKind,
    pub word: String,
    pub key: String,
    pub dim: bool,
}

/// What the page shows of the model, as written last. A plain snapshot, so tests need no world.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Shown {
    pub tab: usize,
    pub device: bool,
    pub place: String,
    pub when: String,
    pub rows: Vec<RowShown>,
    /// The selected row, and whether it is boxed (the pointer on it, or a key moved it).
    pub selected: Option<usize>,
    pub boxed: bool,
    pub pointer_pick: bool,
    pub navigated: u32,
    /// The tab's list, and its filter (Misc's notes' filter: another one starts the list at its
    /// top).
    pub list: ListKind,
    pub group: u32,
    /// The right pane: the selected quest's objectives, a note's text, picture or audio, a
    /// challenge's text.
    pub pane: Pane,
    /// How far the objectives or the text are scrolled (`text_first`, one scroll for both).
    pub pane_first: f32,
    pub hints: Vec<HintShown>,
    /// The World Map, on its tab: the tab shows no list (the list's parts above are empty).
    pub world: Option<WorldShown>,
}

/// The right pane as written (`MM_DataRect` by its `_ItemType`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Pane {
    None,
    /// The objectives, their place their key.
    Objectives(Vec<RowShown>),
    /// A text note's text or a challenge's description and state.
    Text(String),
    /// An image note's picture (its file).
    Image(String),
    /// An audio note: tenths of a second still to play and the share played, thousandths
    /// (`data::Right::Audio`).
    Audio { left: u32, played: u16 },
    /// Radio's waveform panel, the radio playing or not (`data::Right::Wave`): the file's frame,
    /// the hand-built trace over it (`data::trace_over_tiles`).
    Wave { on: bool },
}

impl Pane {
    fn item_type(&self) -> f64 {
        match self {
            Pane::None | Pane::Wave { .. } => 0.0,
            Pane::Text(_) => ITEM_TEXT,
            Pane::Image(_) => ITEM_IMAGE,
            Pane::Audio { .. } => ITEM_AUDIO,
            Pane::Objectives(_) => ITEM_OBJECTIVES,
        }
    }

    /// Whether the pane is a part of `MM_DataRect` (Radio's waveform is a panel of its own).
    fn in_data_rect(&self) -> bool {
        !matches!(self, Pane::None | Pane::Wave { .. })
    }

    fn objectives(&self) -> &[RowShown] {
        match self {
            Pane::Objectives(rows) => rows,
            _ => &[],
        }
    }
}

impl Shown {
    /// What the prepared page `p` shows, in the frame `device`; why it cannot be drawn from the
    /// file otherwise.
    pub(crate) fn of(p: &Prepared, device: bool) -> Result<Shown, String> {
        let v = &p.view;
        let hints = v.buttons.iter().map(|b| HintShown { slot: Slot::of(b.kind), kind: b.kind, word: b.hint.word.to_string(), key: b.hint.key.clone(), dim: b.hint.dim }).collect();
        // A question box up changes nothing here: it is drawn over the page
        // (`data::popup_over_tiles`).
        if let Body::Map(m) = &v.body {
            let world = match v.tab {
                data::WORLD => WorldShown::of(m).filter(|w| !w.local).ok_or("the world map has no picture here")?,
                data::LOCAL => WorldShown::of(m).filter(|w| w.local).ok_or("the local map has no place here")?,
                _ => return Err("the tab is not drawn from the file".into()),
            };
            return Ok(Shown {
                tab: v.tab,
                device,
                place: v.place.clone(),
                when: v.when.clone(),
                rows: Vec::new(),
                selected: None,
                boxed: false,
                pointer_pick: false,
                navigated: 0,
                list: ListKind::default(),
                group: 0,
                pane: Pane::None,
                pane_first: 0.0,
                hints,
                world: Some(world),
            });
        }
        let Body::List { rows, boxed, right, .. } = &v.body else { return Err("the tab shows no list".into()) };
        if rows.len() != p.keys.len() {
            return Err("the rows and their records differ".into());
        }
        let list = match v.tab {
            data::QUESTS => ListKind::Quests,
            data::NOTES if p.challenges => ListKind::Challenges,
            data::NOTES => ListKind::Notes,
            data::RADIO => ListKind::Stations,
            _ => return Err("the tab is not drawn from the file".into()),
        };
        let row = |key: u32, r: &data::Row| RowShown { key, text: r.text.clone(), filled: r.mark == Mark::Filled, done: r.done };
        let (pane, pane_first) = match right {
            Right::Objectives { rows, first } if list == ListKind::Quests => (Pane::Objectives(rows.iter().enumerate().map(|(i, r)| row(i as u32, r)).collect()), *first),
            Right::Text { text, first } if list.misc() => (Pane::Text(text.replace("\r\n", "\n")), *first),
            Right::Image(file) if list == ListKind::Notes => (Pane::Image(file.clone()), 0.0),
            &Right::Audio { left, played } if list == ListKind::Notes => (Pane::Audio { left, played }, 0.0),
            &Right::Wave { on } if list == ListKind::Stations => (Pane::Wave { on }, 0.0),
            Right::None if list != ListKind::Stations => (Pane::None, 0.0),
            _ => return Err("the right pane is not the tab's".into()),
        };
        Ok(Shown {
            tab: v.tab,
            device,
            place: v.place.clone(),
            when: v.when.clone(),
            rows: rows.iter().zip(&p.keys).map(|(r, k)| row(*k, r)).collect(),
            selected: (!rows.is_empty()).then(|| p.sel.min(rows.len() - 1)),
            boxed: boxed.is_some(),
            pointer_pick: p.pointer_pick,
            navigated: p.navigated,
            list,
            group: u32::from(list == ListKind::Notes && p.filtered),
            pane,
            pane_first,
            hints,
            world: None,
        })
    }
}

/// Sets a trait by path as the game's code does (`Ui::write_changed`): what a script set
/// meanwhile stands until the model changes.
fn put(ui: &mut Ui, path: &str, name: &str, v: Value) {
    if let Some(t) = resolve_tile(ui, path) {
        ui.write_changed(t, name, v);
    }
}

fn text(s: &str) -> Value {
    Value::Str(s.to_string())
}

fn on(b: bool) -> Value {
    Value::Num(if b { 1.0 } else { 0.0 })
}

fn num(ui: &Ui, t: TileId, name: &str) -> f64 {
    ui.tile(t).get(name).map_or(0.0, Value::num)
}

/// One row's code-fed traits: its words on the row, which the template's text copies; its square
/// by `_selected` (and `_ShowEmptyMarker` for one shown unselected); its words' and square's
/// strength by `_TextAlpha` [documented: `MM_ListMarkerTemplate`].
fn write_row(ui: &mut Ui, tile: TileId, r: &RowShown, empty_marker: bool) {
    ui.write_changed(tile, "string", text(&r.text));
    ui.write_changed(tile, "_selected", on(r.filled));
    ui.write_changed(tile, "_ShowEmptyMarker", on(empty_marker));
    ui.write_changed(tile, "_TextAlpha", Value::Num(if r.done { DONE_ALPHA } else { FULL_ALPHA }));
}

/// Write what the page shows of `s`: the heading, the tab's list as template rows (the other lists
/// emptied and taking no pointer), the right pane, and the hints. The rows' places and the scrolls
/// are [`page`]'s, once measured.
fn write_model(ui: &mut Ui, s: &Shown, w: &mut Written) -> Result<(), String> {
    put(ui, BACKGROUND, "visible", on(false));
    put(ui, CHALLENGE_TYPE, "visible", on(false));
    put(ui, PLACE, "string", text(&s.place));
    put(ui, TIME, "string", text(&s.when));
    for kind in ListKind::ALL {
        let ours = kind == s.list && s.world.is_none();
        // The list shown takes the pointer (its rows' `target`, its highlight) [documented:
        // list_box.xml]; the others none.
        put(ui, kind.path(), "_enabled", on(ours));
        let rows: &[RowShown] = if ours { &s.rows } else { &[] };
        let wanted: Vec<Wanted<'_, u32>> = rows.iter().map(|r| Wanted { key: &r.key, name: ROW }).collect();
        let list_path = resolve_tile(ui, kind.path()).map(|t| ui.path_of(t)).unwrap_or_else(|| kind.path().to_string());
        if !list::reconcile(ui, &list_path, ROW_TEMPLATE, &mut w.rows[kind.index()], &wanted) {
            return Err(format!("{ROW_TEMPLATE} made no {}'s row", kind.words().1));
        }
        for (row, r) in w.rows[kind.index()].iter().zip(rows) {
            write_row(ui, row.tile, r, false);
        }
    }
    put(ui, DATA_RECT, "visible", on(s.pane.in_data_rect()));
    put(ui, DATA_RECT, "_ItemType", Value::Num(s.pane.item_type()));
    // A note's text, picture and audio: what their tiles show while `_ItemType` shows them, emptied
    // otherwise [documented: `MM_DataText`'s and `MM_DataImage`'s "set in code", the audio's
    // `_ItemStringInfo` and `_AudioFileCompletion`].
    put(ui, DATA_TEXT, "string", text(if let Pane::Text(t) = &s.pane { t } else { "" }));
    put(ui, DATA_IMAGE, "filename", text(if let Pane::Image(f) = &s.pane { f } else { "" }));
    let (left, played) = if let Pane::Audio { left, played } = s.pane { (data::remaining(left), f64::from(played) / 1000.0) } else { (String::new(), 0.0) };
    put(ui, DATA_RECT, "_ItemStringInfo", text(&left));
    put(ui, DATA_RECT, "_AudioFileCompletion", Value::Num(played));
    let objectives = s.pane.objectives();
    let keys: Vec<usize> = (0..objectives.len()).collect();
    let wanted: Vec<Wanted<'_, usize>> = keys.iter().map(|k| Wanted { key: k, name: ROW }).collect();
    let objectives_path = resolve_tile(ui, OBJECTIVES).map(|t| ui.path_of(t)).unwrap_or_else(|| OBJECTIVES.to_string());
    if !list::reconcile(ui, &objectives_path, ROW_TEMPLATE, &mut w.objectives, &wanted) {
        return Err(format!("{ROW_TEMPLATE} made no objective's row"));
    }
    for (row, r) in w.objectives.iter().zip(objectives) {
        write_row(ui, row.tile, r, true);
    }
    if let Some(world) = &s.world {
        write_world(ui, world, w.map_rows(world.local))?;
    }
    ui.update();
    Ok(())
}

/// What [`write_world`] keeps: each template's rows by what they stand for, and what it wrote last
/// (so a frame whose markers did not change writes none of them).
#[derive(Default)]
struct WorldRows {
    markers: Vec<RowTile<u32>>,
    quests: Vec<RowTile<QuestRow>>,
    nodes: Vec<RowTile<NodeRow>>,
    lines: Vec<RowTile<NodeRow>>,
    written: Option<WorldShown>,
}

impl WorldRows {
    fn all_live(&self, ui: &Ui) -> bool {
        self.markers.iter().all(|r| ui.is_live(r.tile))
            && self.quests.iter().all(|r| ui.is_live(r.tile))
            && self.nodes.iter().all(|r| ui.is_live(r.tile))
            && self.lines.iter().all(|r| ui.is_live(r.tile))
    }
}

/// Make `rows` stand for `keys` in order under the tile at `parent` from `template`, named `name`
/// (`list::reconcile`), only when the keys changed: a frame whose rows stand for the same records
/// touches none [design: the markers are hundreds; their moves are writes, not rows].
fn keep_rows<K: PartialEq + Clone>(ui: &mut Ui, parent: &str, template: &str, rows: &mut Vec<RowTile<K>>, keys: &[K], name: &str) -> Result<(), String> {
    let same = rows.len() == keys.len() && rows.iter().zip(keys).all(|(r, k)| r.key == *k && ui.is_live(r.tile));
    if same {
        return Ok(());
    }
    let wanted: Vec<Wanted<'_, K>> = keys.iter().map(|key| Wanted { key, name }).collect();
    if !list::reconcile(ui, parent, template, rows, &wanted) {
        return Err(format!("{template} made no {name}"));
    }
    Ok(())
}

/// The map's picture as a piece the page paints itself, outside the tree: `MM_WorldMap_ParentImage`
/// is a hotrect, which the tile painter does not paint (no hotrect with a `filename` is painted in
/// general), so its picture is drawn from its traits as they are resolved: its `filename` (the
/// model's, or what a script wrote since), stretched over its place and size (the model's
/// viewport, [`keep_world`]), at its `alpha` and `brightness` in its colour (`systemcolor` above
/// it), at its depth (under its rows), cut to the map's window [design: the original's code draws
/// it in a way not known]. None while it is hidden or names no picture.
fn map_picture_piece(ui: &Ui, clip: &str, picture: &str) -> Option<Piece> {
    let parent = ui.find_tile(picture)?;
    if !shown_up(ui, parent) {
        return None;
    }
    let tile = ui.tile(parent);
    let name = tile.get("filename").map(|v| v.text().trim().to_string()).filter(|n| !n.is_empty())?;
    let window = map_window_of(ui, clip)?;
    let (x, y) = origin(ui, parent);
    let rect = super::layout::Rect { left: x, top: y, width: num(ui, parent, "width"), height: num(ui, parent, "height") };
    let cut = super::layout::Rect { left: f64::from(window.min.x), top: f64::from(window.min.y), width: f64::from(window.width()), height: f64::from(window.height()) };
    let trait_or = |name: &str, or: f64| tile.get(name).map_or(or, Value::num);
    let own = ["red", "green", "blue"].map(|n| (trait_or(n, 255.0).clamp(0.0, 255.0) / 255.0) as f32);
    let b = (trait_or("brightness", 255.0).clamp(0.0, 255.0) / 255.0) as f32;
    let [r, g, bl] = super::draw::base_colour(ui, parent, own).map(|c| c * b);
    let alpha = (trait_or("alpha", 255.0).clamp(0.0, 255.0) / 255.0) as f32;
    let mut depth = 0.0;
    let mut at = Some(parent);
    while let Some(t) = at {
        depth += num(ui, t, "depth");
        at = ui.tile(t).parent;
    }
    // A runtime picture's name (the Local Map's outline, `nvf:local_map`) before any atlas or file,
    // as the tile painter takes an image tile's (`super::draw::pieces`), so it is resolved from the
    // runtime pictures and stamped with their revision.
    let source = match tile.get("texatlas").map(|a| a.text().trim().to_string()).filter(|a| !a.is_empty()) {
        _ if super::runtime_images::is_runtime(&name) => Source::Runtime(None),
        Some(atlas) => Source::Index(atlas),
        None => Source::Own,
    };
    Some(Piece { tile: parent, look: Look::Picture { name, source, fit: Fit::Stretch }, rect, clip: Some(cut), colour: [r, g, bl, alpha], depth, tex: None, turn: None, crop: None })
}

/// Whether `tile` is one of the maps' location icons (a [`MARKER_ROW`] row, World Map and Local
/// Map alike), which keep the file's brightness on the device: the hand-built page draws them at
/// 220 (255 under the highlight box) in both frames (`crate::pipboy::data`), as the file's
/// `MapMarkerTemplate` resolves them [documented: map_menu.xml] (`super::draw::device_brightness`).
pub(super) fn keeps_file_brightness(ui: &Ui, tile: TileId) -> bool {
    ui.tile(tile).name.eq_ignore_ascii_case(MARKER_ROW)
}

/// `pieces` (in drawing order, by depth) with the map's picture ([`map_picture_piece`]) among
/// them, under every piece deeper than it and under its own rows.
fn with_map_picture(ui: &Ui, mut pieces: Vec<Piece>, picture: Option<Piece>) -> Vec<Piece> {
    if let Some(p) = picture {
        let at = pieces.iter().position(|q| q.depth > p.depth || stats_menu::inside(ui, q.tile, p.tile)).unwrap_or(pieces.len());
        pieces.insert(at, p);
    }
    pieces
}

/// The World Map's companion names painted outside the file's tree (`map_menu.xml` has no
/// companion template [observed: `data.rs`]): centred above each teammate's place on the
/// map picture, clipped to the map's window, in `s.companion_font` (font 2 by default
/// [documented: Stewie's Tweaks readme, `bCompanionsUseLocationFontOnMap`], font 1 with
/// the tweak), red while knocked out (`bMapShowUnconsciousCompanions`), under the player's
/// arrow [design: mirrors the hand-built World Map in `pipboy::data`].
fn map_companion_pieces(ui: &Ui, s: &WorldShown) -> Vec<Piece> {
    if s.local || s.companions.is_empty() {
        return Vec::new();
    }
    let Some(clip) = ui.find_tile(WORLD_CLIP).filter(|&t| shown_up(ui, t)) else { return Vec::new() };
    let Some(window) = map_window_of(ui, WORLD_CLIP) else { return Vec::new() };
    let cut = super::layout::Rect {
        left: f64::from(window.min.x),
        top: f64::from(window.min.y),
        width: f64::from(window.width()),
        height: f64::from(window.height()),
    };
    let tile_depth = |t: TileId| {
        let mut depth = 0.0;
        let mut at = Some(t);
        while let Some(id) = at {
            depth += num(ui, id, "depth");
            at = ui.tile(id).parent;
        }
        depth
    };
    let cursor = ui.find_tile(WORLD_CURSOR);
    let colour_tile = cursor.unwrap_or(clip);
    let [r, g, bl] = super::draw::base_colour(ui, colour_tile, [1.0, 1.0, 1.0]);
    let depth = cursor.map(|c| tile_depth(c) - 0.1).unwrap_or_else(|| ui.find_tile(WORLD_PICTURE).map_or(5.5, |p| tile_depth(p) + 5.5));
    let font = super::draw::font_number(f64::from(s.companion_font));
    s.companions
        .iter()
        .map(|(_, uv, name, down)| {
            let at = s.view.to_window(*uv);
            let colour = if *down { [1.0, 0.25, 0.2, 1.0] } else { [r, g, bl, 1.0] };
            Piece {
                tile: clip,
                look: Look::Text { string: name.clone(), font, wrap: None, alignment: super::layout::Alignment::Center },
                rect: super::layout::Rect {
                    left: f64::from(window.min.x + at.x),
                    top: f64::from(window.min.y + at.y - data::COMPANION_NAME_RISE),
                    width: 0.0,
                    height: 0.0,
                },
                clip: Some(cut),
                colour,
                depth,
                tex: None,
                turn: None,
                crop: None,
            }
        })
        .collect()
}

/// Insert companion name pieces into `pieces` right under the player's arrow (`WORLD_CURSOR`),
/// or at their depth order when the arrow is absent [design].
fn with_map_companions(ui: &Ui, mut pieces: Vec<Piece>, companions: Vec<Piece>) -> Vec<Piece> {
    if companions.is_empty() {
        return pieces;
    }
    let at = ui
        .find_tile(WORLD_CURSOR)
        .and_then(|c| pieces.iter().position(|p| p.tile == c))
        .or_else(|| pieces.iter().position(|p| p.depth > companions[0].depth))
        .unwrap_or(pieces.len());
    pieces.splice(at..at, companions);
    pieces
}

fn num_of(v: f32) -> Value {
    Value::Num(f64::from(v))
}

/// Write the World Map `s` as the file's "set in code" traits [documented: map_menu.xml's
/// comments; design: the values, the model's]:
/// - `MM_WorldMap_ParentImage`: `filename`, `filewidth`/`fileheight` (the picture's size at a
///   magnification of 1), `_Magnification`, the markers' widths `user0` (map markers), `user1`
///   (quest markers) and `user2` (the arrow), and its place and size from the model's viewport
///   ([`keep_world`] puts those right every pass); the picture is painted from those traits
///   ([`map_picture_piece`]).
/// - Under it, kept by record: a `MapMarkerTemplate` row per marker (`_x`/`_y` its texture
///   coordinates, `filename` its icon, `_selected` under the highlight box, `_LocationName`,
///   `_MarkerIndex`); a `WorldMapQuestMarkerTemplate` row per quest target and one for the custom
///   marker (its own picture, a depth over them); a `WaypointTemplate` node at the arrow and at
///   each line's end, and under the start node a `PathLineTemplate` line to each end (`user0` its
///   length at a magnification of 1, `rotateangle` its direction).
/// - `MM_WorldMapCursor`: `_x`/`_y` the player's place, `rotateangle` the heading, shown while the
///   player is on this map.
/// - `MM_MapHighlightBox`: `_Title` (the name and the line under it).
///
/// The Local Map likewise into `MM_LocalMap_*` ([`LOCAL_PATHS`]): its frame (the outline's grid)
/// is the picture's place, its `filename` the outline's runtime name once made (given by [`pass`]
/// before this writes it; empty before), the doors and the markers from `MapMarkerTemplate`, the
/// quest markers and the custom marker from `LocalMapQuestMarkerTemplate` (which keeps them inside
/// the window), no dotted line [observed: the original's Local Map has no waypoint nodes,
/// pipboy.md §5.3].
fn write_world(ui: &mut Ui, s: &WorldShown, rows: &mut WorldRows) -> Result<(), String> {
    let paths = s.paths();
    let parent = ui.find_tile(paths.picture).ok_or("the map's picture is gone")?;
    give_arrow_brightness(ui, paths.cursor);
    ui.write_changed(parent, "filename", text(&s.texture));
    ui.write_changed(parent, "filewidth", num_of(s.usable.x));
    ui.write_changed(parent, "fileheight", num_of(s.usable.y));
    ui.write_changed(parent, "_Magnification", num_of(s.mag));
    ui.write_changed(parent, "user0", num_of(data::MARKER));
    ui.write_changed(parent, "user1", num_of(data::QUEST_MARKER));
    ui.write_changed(parent, "user2", num_of(data::CURSOR));
    let same_markers = rows.written.as_ref().is_some_and(|w| w.markers == s.markers) && rows.markers.len() == s.markers.len() && rows.markers.iter().all(|r| ui.is_live(r.tile));
    if !same_markers {
        let keys: Vec<u32> = s.markers.iter().map(|m| m.form).collect();
        keep_rows(ui, paths.picture, MARKER_TEMPLATE, &mut rows.markers, &keys, MARKER_ROW)?;
        for (i, (row, m)) in rows.markers.iter().zip(&s.markers).enumerate() {
            ui.write_changed(row.tile, "_x", num_of(m.uv[0]));
            ui.write_changed(row.tile, "_y", num_of(m.uv[1]));
            ui.write_changed(row.tile, "filename", text(&m.icon));
            ui.write_changed(row.tile, "_selected", on(m.selected));
            ui.write_changed(row.tile, "_LocationName", text(&m.name));
            ui.write_changed(row.tile, "_MarkerIndex", Value::Num(i as f64));
        }
    }
    let mut quests: Vec<(QuestRow, [f32; 2])> = s.quests.iter().map(|&(f, uv)| (QuestRow::Target(f), uv)).collect();
    quests.extend(s.custom.map(|uv| (QuestRow::Custom, uv)));
    let keys: Vec<QuestRow> = quests.iter().map(|q| q.0).collect();
    keep_rows(ui, paths.picture, paths.quest_template, &mut rows.quests, &keys, paths.quest_row)?;
    for (row, (key, uv)) in rows.quests.iter().zip(&quests) {
        ui.write_changed(row.tile, "_x", num_of(uv[0]));
        ui.write_changed(row.tile, "_y", num_of(uv[1]));
        let (file, depth) = if *key == QuestRow::Custom { (data::PC_MARKER_FILE, 5.0) } else { (data::QUEST_FILE, 4.0) };
        ui.write_changed(row.tile, "filename", text(file));
        ui.write_changed(row.tile, "depth", Value::Num(depth));
    }
    // The dotted lines: nodes at the arrow and each end, a line from the start to each end.
    let start = s.arrow.filter(|_| !s.ends.is_empty()).map(|a| a.0);
    let mut nodes: Vec<(NodeRow, [f32; 2])> = start.map(|uv| (NodeRow::Start, uv)).into_iter().collect();
    nodes.extend(s.ends.iter().filter(|_| start.is_some()).map(|&(to, f, uv)| (NodeRow::End(to, f), uv)));
    // The start node is named apart, so its lines are made under it by path.
    let start_keys: Vec<NodeRow> = nodes.iter().filter(|n| n.0 == NodeRow::Start).map(|n| n.0).collect();
    let end_keys: Vec<NodeRow> = nodes.iter().filter(|n| n.0 != NodeRow::Start).map(|n| n.0).collect();
    let mut start_rows: Vec<RowTile<NodeRow>> = rows.nodes.iter().filter(|r| r.key == NodeRow::Start).cloned().collect();
    let mut end_rows: Vec<RowTile<NodeRow>> = rows.nodes.iter().filter(|r| r.key != NodeRow::Start).cloned().collect();
    keep_rows(ui, paths.picture, NODE_TEMPLATE, &mut start_rows, &start_keys, START_NODE_ROW)?;
    keep_rows(ui, paths.picture, NODE_TEMPLATE, &mut end_rows, &end_keys, NODE_ROW)?;
    rows.nodes = start_rows.into_iter().chain(end_rows).collect();
    for row in &rows.nodes {
        if let Some((_, uv)) = nodes.iter().find(|n| n.0 == row.key) {
            ui.write_changed(row.tile, "_x", num_of(uv[0]));
            ui.write_changed(row.tile, "_y", num_of(uv[1]));
        }
    }
    match (start, rows.nodes.iter().find(|r| r.key == NodeRow::Start)) {
        (Some(from), Some(node)) => {
            let at = ui.path_of(node.tile);
            let keys: Vec<NodeRow> = s.ends.iter().map(|&(to, f, _)| NodeRow::End(to, f)).collect();
            keep_rows(ui, &at, PATH_TEMPLATE, &mut rows.lines, &keys, PATH_ROW)?;
            for (row, &(_, _, to)) in rows.lines.iter().zip(&s.ends) {
                let (len, angle) = path_line(from, to, s.usable);
                ui.write_changed(row.tile, "user0", num_of(len));
                ui.write_changed(row.tile, "rotateangle", num_of(angle));
                // The start node's `clips` does not crop its line to the node's box: the
                // original's line runs from the arrow to the far end, far past the node
                // [observed: #4685's screenshot of the original's World Map]. The line is cut
                // by the map's window above the node, as a list row's children are
                // (`super::list::unclip_row_children`).
                ui.write_changed(row.tile, fnv_ui::input::OUTSIDE_CLIP, Value::Num(1.0));
            }
        }
        _ => {
            for r in std::mem::take(&mut rows.lines) {
                ui.unload_tile(r.tile);
            }
        }
    }
    if let Some(cursor) = ui.find_tile(paths.cursor) {
        if let Some((uv, heading)) = s.arrow {
            ui.write_changed(cursor, "_x", num_of(uv[0]));
            ui.write_changed(cursor, "_y", num_of(uv[1]));
            ui.write_changed(cursor, "rotateangle", num_of(data::arrow_turn(heading)));
        }
    }
    if let Some(b) = ui.find_tile(HIGHLIGHT_BOX) {
        ui.write_changed(b, "_Title", text(&s.title()));
    }
    rows.written = Some(s.clone());
    Ok(())
}

/// The arrow's `brightness`, which the waypoint nodes and the dotted lines copy [documented:
/// `WaypointTemplate`'s and `PathLineTemplate`'s `brightness`, a copy of `MM_WorldMapCursor`'s],
/// given the painter's own default, 255, while the arrow has none: the file gives it none, and a
/// trait a tile does not have reads 0 in the tree (`fnv_ui`) where the painter draws a tile without
/// one at 255 (`super::layout`), so the copies would draw black [design: the code-owned value
/// put in before the copies are worked out; a value the file, a mod or a script gave stays].
fn give_arrow_brightness(ui: &mut Ui, cursor: &str) {
    if let Some(c) = ui.find_tile(cursor).filter(|&c| ui.tile(c).get("brightness").is_none()) {
        ui.set_trait(c, "brightness", Value::Num(255.0));
    }
}

/// A dotted line from texture coordinates `from` to `to` on a picture `usable` across at a
/// magnification of 1: its length there (the file takes 4 off and multiplies by the
/// magnification, `PathLineTemplate`'s `width`) and its direction, radians clockwise from the
/// window's x axis (its y runs down), as the hand-built line turns (`data::marker::dotted_line`).
fn path_line(from: [f32; 2], to: [f32; 2], usable: Vec2) -> (f32, f32) {
    let d = Vec2::new((to[0] - from[0]) * usable.x, (to[1] - from[1]) * usable.y);
    (d.length(), d.y.atan2(d.x))
}

/// [documented: GECK wiki `clipwindow`] Under the map clip windows (`MM_Highlight_ClipWindow`,
/// `MM_WorldMap_ClipWindow`, `MM_LocalMap_ClipWindow`), `<clips> 1 </clips>` on a descendant
/// opts that tile into clipping by the enclosing `<clipwindow>` rather than self-clipping its
/// own children. UI mods such as Vanilla UI Plus convert `MM_HighlightLocationName` into a
/// centered `<rect>` with `<clips> 1 </clips>` and child `<text>` tiles (`original`,
/// `VUI+Shadow`, `VUI+Outline`), and attach shadow children to `<clips> 1 </clips>` map markers.
/// Because `fnv_ui::input::clips_children` treats `<clips>` as a self-clipper for legacy
/// layouts, mark live direct children of `<clips>`-only descendants with
/// [`fnv_ui::input::OUTSIDE_CLIP`] so they inherit the enclosing `<clipwindow>` instead of
/// clipping off the left half of centered place names (as `super::list::unclip_row_children`
/// does for list rows) [design].
fn unclip_map_window_children(ui: &mut Ui) {
    for path in [HIGHLIGHT_CLIP, WORLD_CLIP, LOCAL_CLIP] {
        let Some(window) = resolve_tile(ui, path) else { continue };
        if !ui.tile(window).get("clipwindow").is_some_and(Value::truthy) {
            continue;
        }
        let mut stack: Vec<TileId> = ui.tile(window).children.clone();
        while let Some(tile) = stack.pop() {
            if !ui.is_live(tile) {
                continue;
            }
            if ui.tile(tile).get("clipwindow").is_some_and(Value::truthy) {
                continue;
            }
            let clips_only = ui.tile(tile).get("clips").is_some_and(Value::truthy);
            let children = ui.tile(tile).children.clone();
            if clips_only {
                for &child in &children {
                    if ui.is_live(child) {
                        ui.write_changed(child, fnv_ui::input::OUTSIDE_CLIP, Value::Num(1.0));
                    }
                }
            }
            stack.extend(children);
        }
    }
}

/// The World Map's places, put right on every pass as the hints are: the file works the picture's
/// place out from its drag and its size from its magnification, and the highlight box's place and
/// the windows' `visible` from the tab, again whenever what they read changes [design: as
/// [`keep_hints`]]. The picture stands where the model's viewport puts it (and is painted
/// there, [`map_picture_piece`]); the highlight box on the model's highlight (its window may stand apart from the map's).
fn keep_world(ui: &mut Ui, s: &WorldShown) {
    let paths = s.paths();
    // The map's window and the highlight box shown; the other map's window hidden. The Local
    // Map's picture's hotrect is hidden in the file and shown in code [documented: its `visible`,
    // "set in code"]; the World Map's is the file's (and a script's) to show or hide.
    for path in [paths.clip, HIGHLIGHT_CLIP, HIGHLIGHT_BOX] {
        if let Some(t) = ui.find_tile(path) {
            keep(ui, t, "visible", 1.0);
        }
    }
    if s.local {
        if let Some(t) = ui.find_tile(paths.picture) {
            keep(ui, t, "visible", 1.0);
        }
    }
    if let Some(t) = ui.find_tile(paths.other_clip) {
        keep(ui, t, "visible", 0.0);
    }
    let r = s.picture;
    if let Some(p) = ui.find_tile(paths.picture) {
        keep(ui, p, "width", f64::from(r.width()));
        keep(ui, p, "height", f64::from(r.height()));
        keep(ui, p, "dragdeltax", f64::from(r.min.x));
        keep(ui, p, "dragdeltay", f64::from(r.min.y));
        keep(ui, p, "x", f64::from(r.min.x));
        keep(ui, p, "y", f64::from(r.min.y));
    }
    if let Some(c) = ui.find_tile(paths.cursor) {
        keep(ui, c, "visible", if s.arrow.is_some() { 1.0 } else { 0.0 });
    }
    // The box's middle on the model's highlight, in its own window's units.
    let (Some(b), Some(clip), Some(map)) = (ui.find_tile(HIGHLIGHT_BOX), ui.find_tile(HIGHLIGHT_CLIP), ui.find_tile(paths.clip)) else { return };
    let (c, m) = (origin(ui, clip), origin(ui, map));
    let shift = (m.0 - c.0, m.1 - c.1);
    let (w, h) = (num(ui, b, "width"), num(ui, b, "height"));
    keep(ui, b, "x", f64::from(s.cursor.x) + shift.0 - w / 2.0);
    keep(ui, b, "y", f64::from(s.cursor.y) + shift.1 - h / 2.0);
}

/// The hints on the file's tiles: each tile shows its hint or none, the hint's word on `string`, its
/// key on `_PCButtonText`, lit or dim by `target` (the file's line alpha follows it) [documented:
/// text_box.xml; design: the hand-built page's rules, not the file's expressions]. The file works
/// `visible` and `string` out by expressions, which work them out again when what they read
/// changes (the tab, a list's rows): so these are put right on every pass, not written once
/// [design: as `inventory_menu::keep`].
fn keep_hints(ui: &mut Ui, hints: &[HintShown]) {
    for (name, slot) in BUTTONS {
        let Some(t) = resolve_tile(ui, &button_path(name)) else { continue };
        let hint = hints.iter().find(|h| h.slot == slot);
        keep(ui, t, "visible", if hint.is_some() { 1.0 } else { 0.0 });
        if let Some(h) = hint {
            keep_text(ui, t, "string", &h.word);
            keep_text(ui, t, "_PCButtonText", &h.key);
            keep(ui, t, "target", if h.dim { 0.0 } else { 1.0 });
        }
    }
}

/// The tab's lists' `visible`: the file shows both `MM_NotesList` and `MM_ChallengeList` on Misc
/// [documented: their `visible`], so the one not shown is hidden, put right on every pass as the
/// hints are (the file works it out again when `_CurrentTab` changes) [design]. Radio's waveform
/// panel likewise: shown on Radio, its animated image always hidden (the trace is painted over the
/// frame, `data::trace_over_tiles`).
fn keep_lists(ui: &mut Ui, s: &Shown) {
    for kind in ListKind::ALL.into_iter().filter(|k| k.tab() == s.tab) {
        if let Some(t) = resolve_tile(ui, kind.path()) {
            keep(ui, t, "visible", if kind == s.list { 1.0 } else { 0.0 });
        }
    }
    if let Some(t) = resolve_tile(ui, WAVE_RECT) {
        keep(ui, t, "visible", if matches!(s.pane, Pane::Wave { .. }) { 1.0 } else { 0.0 });
    }
    if let Some(t) = resolve_tile(ui, WAVE_IMAGE) {
        keep(ui, t, "visible", 0.0);
    }
}

/// A note's or challenge's text scrolled as DATA's one right pane (`data::scroll_pane`), as the
/// objectives are ([`write_pane`]): the file moves the text by its bar's value times `_ScrollDelta`
/// [documented: `MM_DataText`'s `y`], so that is 1 and the value the pane's scroll; the bar's count
/// how far it can scroll and 1 [design]. Returns how far it can scroll: the text's measured height
/// past its window's.
fn write_text_pane(ui: &mut Ui, first: f32) -> f64 {
    let (Some(rect), Some(words), Some(bar)) = (resolve_tile(ui, TEXT_RECT), resolve_tile(ui, DATA_TEXT), resolve_tile(ui, TEXT_BAR)) else { return 0.0 };
    // The window the text is seen through: what clips it (its rect, and any clip above that is
    // smaller), as readiness and a page of it read it; the rect's height without a clip.
    let window = text_clip(ui, words).map_or_else(|| num(ui, rect, "height"), |c| c.height);
    let most = (num(ui, words, "height") - window).max(0.0);
    ui.set_trait(bar, "_ScrollDelta", Value::Num(1.0));
    ui.set_trait(bar, "_number_of_items", Value::Num(most.ceil() + 1.0));
    ui.set_trait(bar, "_number_of_visible_items", Value::Num(1.0));
    list::write_bar_value(ui, bar, f64::from(first).clamp(0.0, most));
    most
}

/// What clips the text `words` as laid out (canvas units).
fn text_clip(ui: &Ui, words: TileId) -> Option<super::layout::Rect> {
    super::layout::layout(ui, words, &|_, _, _| (0.0, 0.0), true).into_iter().find(|i| i.tile == words)?.clip
}

/// The right pane's scroll bar, while it shows one that scrolls: the objectives' or the text's.
fn pane_bar(ui: &Ui, pane: &Pane) -> Option<TileId> {
    match pane {
        Pane::Objectives(_) => resolve_tile(ui, OBJECTIVES).and_then(|l| list::child(ui, l, list::SCROLLBAR)),
        Pane::Text(_) => resolve_tile(ui, TEXT_BAR),
        _ => None,
    }
}

/// [`keep`] for words.
fn keep_text(ui: &mut Ui, t: TileId, name: &str, want: &str) {
    if ui.tile(t).get(name).map(|v| v.text().to_string()).as_deref() != Some(want) {
        ui.set_trait(t, name, text(want));
    }
}

/// The five tabs made from their template under the tab line, each kept for good; whether any was
/// made.
fn ensure_tabs(ui: &mut Ui, tabs: &mut Vec<RowTile<usize>>) -> Result<bool, String> {
    if tabs.len() == 5 && tabs.iter().all(|t| ui.is_live(t.tile)) {
        return Ok(false);
    }
    let keys = [0, 1, 2, 3, 4];
    let wanted: Vec<Wanted<'_, usize>> = keys.iter().map(|key| Wanted { key, name: TAB_ROW }).collect();
    let tabline = resolve_tile(ui, TABLINE).map(|t| ui.path_of(t)).unwrap_or_else(|| TABLINE.to_string());
    if !list::reconcile(ui, &tabline, TAB_TEMPLATE, tabs, &wanted) {
        return Err(format!("{TAB_TEMPLATE} made no tab"));
    }
    ui.update();
    Ok(true)
}

/// What the tab line is told: as ITEMS' tabs (`inventory_menu::write_tabs`), the words the
/// hand-built page's (`Page::tabs`).
fn write_tabs(ui: &mut Ui, tabs: &[RowTile<usize>], tab: usize) {
    put(ui, TABLINE, "_ButtonCount", Value::Num(5.0));
    put(ui, TABLINE, "_CurrentTab", Value::Num(tab as f64));
    let words = Page::Data.tabs();
    for row in tabs {
        ui.write_changed(row.tile, "string", text(words[row.key]));
        if find_named(ui, row.tile, TAB_INNER).is_some() {
            ui.write_changed(row.tile, fnv_ui::input::NO_ORIGIN, Value::Num(1.0));
        }
    }
}

/// Puts a value right that the file may work out otherwise, writing only when it differs.
fn keep(ui: &mut Ui, t: TileId, name: &str, want: f64) {
    if ui.tile(t).get(name).map(Value::num) != Some(want) {
        ui.set_trait(t, name, Value::Num(want));
    }
}

/// The tabs' `_x` and the line's `_LeftLineLength`, by the tabs' own widths, as ITEMS'
/// (`inventory_menu::tab_xs`).
fn place_tabs(ui: &mut Ui, tabs: &[RowTile<usize>]) {
    let Some(line) = resolve_tile(ui, TABLINE) else { return };
    let widths: Vec<f64> = tabs.iter().map(|r| num(ui, r.tile, "width")).collect();
    let (gap, xs) = super::inventory_menu::tab_xs(num(ui, line, "width"), &widths);
    keep(ui, line, "_LeftLineLength", gap);
    for (row, x) in tabs.iter().zip(xs) {
        keep(ui, row.tile, "_x", x);
    }
}

/// The objectives' scroll as DATA's one right pane (`data::scroll_pane`): the rows move by exactly
/// the pane's units, `_scroll_delta` 1 and the bar's value the pane's scroll; its count how far it
/// can scroll and 1 [design]. Returns how far it can scroll.
fn write_pane(ui: &mut Ui, list_tile: TileId, heights: &[f64], first: f32) -> f64 {
    let total: f64 = heights.iter().sum();
    let most = (total - num(ui, list_tile, "height")).max(0.0);
    ui.set_trait(list_tile, "_scroll_delta", Value::Num(1.0));
    let value = f64::from(first).clamp(0.0, most);
    if let Some(bar) = list::child(ui, list_tile, list::SCROLLBAR) {
        ui.set_trait(bar, "_number_of_items", Value::Num(most.ceil() + 1.0));
        ui.set_trait(bar, "_number_of_visible_items", Value::Num(1.0));
        list::write_bar_value(ui, bar, value);
    }
    list::write_highlight(ui, list_tile, None, value);
    most
}

/// What a tile of the page stands for, by the tile or a parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataTile {
    /// A row of the tab's list (the quests, the notes or the challenges).
    Row(usize),
    Tab(usize),
    Button(Slot),
    /// A piece of the tab's list's own scroll bar.
    ListBar(BarPiece),
    /// A piece of the right pane's scroll bar: the objectives' or the text's.
    PaneBar(BarPiece),
}

/// What `tile` stands for: a tab, a row of the tab's list, a hint, or a piece of that list's or the
/// right pane's scroll bar.
pub fn data_tile(ui: &Ui, w: &Written, tile: TileId) -> Option<DataTile> {
    let mut at = Some(tile);
    while let Some(t) = at {
        if let Some(row) = w.tabs.iter().find(|r| r.tile == t) {
            return Some(DataTile::Tab(row.key));
        }
        at = ui.tile(t).parent;
    }
    // The World Map's tiles stand for nothing here: its pointer is the model's.
    if w.shown.as_ref().is_some_and(|s| s.world.is_some()) {
        let none: [(&str, &[RowTile<u32>]); 0] = [];
        return match list::hit(ui, tile, &none, &BUTTONS) {
            list::Hit::Target(slot) => Some(DataTile::Button(slot)),
            _ => None,
        };
    }
    let none: &[RowTile<u32>] = &[];
    let shown = w.list().index();
    let lists = [
        (ListKind::Quests.name(), w.rows[0].as_slice()),
        (ListKind::Notes.name(), &w.rows[1]),
        (ListKind::Challenges.name(), &w.rows[2]),
        (ListKind::Stations.name(), &w.rows[3]),
        (OBJECTIVES_NAME, none),
        (TEXT_RECT_NAME, none),
    ];
    match list::hit(ui, tile, &lists, &BUTTONS) {
        list::Hit::Row(l, i) if l == shown => Some(DataTile::Row(i)),
        list::Hit::Bar(l, piece) if l == shown => Some(DataTile::ListBar(piece)),
        list::Hit::Bar(4 | 5, piece) => Some(DataTile::PaneBar(piece)),
        list::Hit::Target(slot) => Some(DataTile::Button(slot)),
        _ => None,
    }
}

/// What [`usable`] measured and went through.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Checked {
    pub rows: usize,
    pub rows_in_clip: usize,
    pub hit_tests: usize,
    /// The right pane's window (canvas units) while it scrolls (the objectives, a text), from what
    /// clips it.
    pub pane: Option<Rect>,
    /// The first tab or lit hint that did not take the pointer, and what did: warned of, not a
    /// fallback.
    pub missed: Option<String>,
    /// Radio's waveform panel as laid out (canvas units), the box the trace is painted in.
    pub wave: Option<Rect>,
    /// The World Map's window as laid out and the hints' boxes (canvas units), for the model's
    /// pointer (`DataState::publish_map`).
    pub map: Option<(Rect, Vec<Rect>)>,
}

/// Whether what is about to be painted, `pieces`, can be used, on every final pass: every tab's
/// words among the pieces and whole; the tab's list's rows whose words lie whole inside the list's
/// window, at least one when it has rows, taking the pointer at their middles; a list that scrolls
/// has a window; the right pane shows what it stands for ([`pane_shows`]). The tabs but the one
/// showing and the lit hints are asked the same, and one that does not take the pointer is told in
/// [`Checked::missed`]: its key does the work [design]. One walk of the menu serves every hit test.
pub fn usable(ui: &Ui, menu: TileId, w: &Written, tab: usize, pieces: &[Piece]) -> Result<Checked, String> {
    let is_text = |x: &&Piece| matches!(x.look, Look::Text { .. });
    let texts: HashMap<TileId, &Piece> = pieces.iter().filter(is_text).map(|x| (x.tile, x)).collect();
    let world = w.shown.as_ref().and_then(|s| s.world.as_ref());
    let kind = w.list();
    let no_rows: Vec<RowTile<u32>> = Vec::new();
    let rows = if world.is_some() { &no_rows } else { &w.rows[kind.index()] };
    let mut wanted: Vec<(DataTile, (f64, f64))> = Vec::new();
    if world.is_none() {
        let (many, one) = kind.words();
        let list_tile = resolve_tile(ui, kind.path()).ok_or_else(|| format!("the {many} list is gone"))?;
        let scrolls = list::child(ui, list_tile, list::SCROLLBAR).is_some_and(|b| num(ui, b, "_number_of_items") > 1.0);
        for (i, row) in rows.iter().enumerate() {
            let Some(piece) = list::child(ui, row.tile, ROW_TEXT).and_then(|t| texts.get(&t)) else { continue };
            if scrolls && piece.clip.is_none() {
                return Err(format!("the {many} list scrolls and has no window"));
            }
            if let Some(middle) = shows_whole(piece) {
                wanted.push((DataTile::Row(i), middle));
            }
        }
        if !rows.is_empty() && wanted.is_empty() {
            return Err(format!("no {one}'s row shows whole"));
        }
    }
    let rows_in_clip = wanted.len();
    let mut optional = Vec::new();
    for row in &w.tabs {
        let piece = pieces.iter().filter(is_text).find(|x| stats_menu::inside(ui, x.tile, row.tile)).ok_or_else(|| format!("tab {} is not shown", row.key))?;
        let middle = shows_whole(piece).ok_or_else(|| format!("tab {} does not fit", row.key))?;
        if row.key != tab {
            optional.push((DataTile::Tab(row.key), middle));
        }
    }
    for (name, slot) in BUTTONS {
        let Some(b) = resolve_tile(ui, &button_path(name)) else { continue };
        if !ui.tile(b).get("visible").is_some_and(Value::truthy) || !ui.tile(b).get("target").is_some_and(Value::truthy) {
            continue;
        }
        // The hint's word, not its key: the key's text may stand beside the tile's own box.
        let word = w.shown.as_ref().and_then(|s| s.hints.iter().find(|h| h.slot == slot)).map(|h| h.word.as_str());
        let says = |x: &&Piece| matches!(&x.look, Look::Text { string, .. } if Some(string.as_str()) == word);
        let inside: Vec<&Piece> = pieces.iter().filter(is_text).filter(|x| stats_menu::inside(ui, x.tile, b)).collect();
        if let Some(middle) = inside.iter().find(|x| says(x)).or(inside.first()).and_then(|p| shows_whole(p)) {
            optional.push((DataTile::Button(slot), middle));
        }
    }
    let found = fnv_ui::input::hit_candidates(ui, menu, &fnv_ui::input::no_measure);
    let mut missed = None;
    for &(what, at) in wanted.iter().chain(&optional) {
        let hit = fnv_ui::input::top_hit(ui, &found, at);
        if hit.and_then(|h| data_tile(ui, w, h.tile)) != Some(what) {
            let there = hit.map_or_else(|| "nothing".to_string(), |h| ui.path_of(h.tile));
            let line = format!("{what:?} does not take the pointer at ({:.0}, {:.0}): {there} does", at.0, at.1);
            match what {
                DataTile::Row(_) => return Err(line),
                _ => missed = missed.or(Some(line)),
            }
        }
    }
    let pane = match w.shown.as_ref() {
        Some(s) => pane_shows(ui, w, &s.pane, pieces, &texts)?,
        None => None,
    };
    let wave = match w.shown.as_ref().map(|s| &s.pane) {
        Some(Pane::Wave { .. }) => Some(wave_box(ui)?),
        _ => None,
    };
    let map = match world {
        Some(s) => Some((world_shows(ui, s, pieces)?, hint_boxes(ui))),
        None => None,
    };
    Ok(Checked { rows: rows.len(), rows_in_clip, hit_tests: wanted.len() + optional.len(), pane, missed, wave, map })
}

/// Where a tile's top left stands (canvas units): its and its parents' `x` and `y` summed, as the
/// layout places a tile under its parent (`super::layout`), without walking what is under it (the
/// World Map's window holds hundreds of markers).
fn origin(ui: &Ui, t: TileId) -> (f64, f64) {
    let (mut x, mut y) = (0.0, 0.0);
    let mut at = Some(t);
    while let Some(id) = at {
        x += num(ui, id, "x");
        y += num(ui, id, "y");
        at = ui.tile(id).parent;
    }
    (x, y)
}

/// Whether a tile and every tile above it show (their `visible`).
fn shown_up(ui: &Ui, t: TileId) -> bool {
    let mut at = Some(t);
    while let Some(id) = at {
        if !ui.tile(id).get("visible").is_none_or(Value::truthy) {
            return false;
        }
        at = ui.tile(id).parent;
    }
    true
}

/// A tile's box as laid out (canvas units), when shown: its own, or for a hotrect (which lays out
/// none) what its shown children cover.
fn shown_box(ui: &Ui, t: TileId) -> Option<Rect> {
    let items = super::layout::layout(ui, t, &|_, _, _| (0.0, 0.0), true);
    let rect = |r: &super::layout::Rect| Rect::new(r.left as f32, r.top as f32, (r.left + r.width) as f32, (r.top + r.height) as f32);
    if let Some(i) = items.iter().find(|i| i.tile == t) {
        return i.visible.then(|| rect(&i.rect));
    }
    items.iter().filter(|i| i.visible && i.rect.width > 0.0 && i.rect.height > 0.0).map(|i| rect(&i.rect)).reduce(|a, b| a.union(b))
}

/// The hints that show and take the pointer, by their boxes as laid out (canvas units): the
/// pointer over one is the hint's, never the map's (`data::on_button`).
fn hint_boxes(ui: &Ui) -> Vec<Rect> {
    BUTTONS
        .iter()
        .filter_map(|(name, _)| resolve_tile(ui, &button_path(name)))
        .filter(|&b| ui.tile(b).get("visible").is_some_and(Value::truthy) && ui.tile(b).get("target").is_some_and(Value::truthy))
        .filter_map(|b| shown_box(ui, b))
        .collect()
}

/// Whether the World Map shows what it stands for, and its window (canvas units): the window laid
/// out with a size, wholly on the canvas and shown (what the model reads the pointer in, and
/// where the picture is cut); the picture's image painted inside it; the highlight box shown
/// [design]. A file whose window is gone or hidden leaves the hand-built page.
fn world_shows(ui: &Ui, s: &WorldShown, pieces: &[Piece]) -> Result<Rect, String> {
    let paths = s.paths();
    let name = paths.name;
    let clip = ui.find_tile(paths.clip).ok_or_else(|| format!("the {name}'s window is gone"))?;
    // A hotrect lays out no box of its own: its place and size are its traits, under its parents'.
    if !shown_up(ui, clip) {
        return Err(format!("the {name}'s window is not shown"));
    }
    let at = origin(ui, clip);
    let r = super::layout::Rect { left: at.0, top: at.1, width: num(ui, clip, "width"), height: num(ui, clip, "height") };
    if !on_canvas(&r) {
        return Err(format!("the {name}'s window does not fit"));
    }
    let window = Rect::new(r.left as f32, r.top as f32, (r.left + r.width) as f32, (r.top + r.height) as f32);
    let picture = ui.find_tile(paths.picture).ok_or("the map's picture is gone")?;
    if !s.texture.is_empty() && !pieces.iter().any(|p| p.tile == picture && matches!(p.look, Look::Picture { .. }) && p.clip.is_some_and(|c| c.width > 0.0 && c.height > 0.0)) {
        return Err("the map's picture is not shown in its window".into());
    }
    if ui.find_tile(HIGHLIGHT_BOX).and_then(|b| shown_box(ui, b)).is_none() {
        return Err("the highlight box is not shown".into());
    }
    Ok(window)
}

/// Whether the right pane shows what it stands for among what is painted (a tile hidden, by its own
/// `visible` or one above it, or placed outside its window shows nothing, and the pane would be
/// blank), and its window while it scrolls: objectives with at least one of them whole inside their
/// window; a text with some of its words inside its window; a picture painted on the canvas; an
/// audio note's time whole [design].
fn pane_shows(ui: &Ui, w: &Written, pane: &Pane, pieces: &[Piece], texts: &HashMap<TileId, &Piece>) -> Result<Option<Rect>, String> {
    let within = |window: Rect, p: &Piece, whole: bool| {
        let r = p.rect;
        let (l, t, rr, b) = (f64::from(window.min.x) - 1.0, f64::from(window.min.y) - 1.0, f64::from(window.max.x) + 1.0, f64::from(window.max.y) + 1.0);
        if whole {
            l <= r.left && t <= r.top && r.left + r.width <= rr && r.top + r.height <= b
        } else {
            r.left < rr && r.left + r.width > l && r.top < b && r.top + r.height > t
        }
    };
    match pane {
        // Radio's panel is checked by [`wave_box`].
        Pane::None | Pane::Wave { .. } => Ok(None),
        Pane::Objectives(_) => {
            let bar = resolve_tile(ui, OBJECTIVES).and_then(|l| list::child(ui, l, list::SCROLLBAR)).ok_or("the objectives' scroll bar is gone")?;
            let window = pane_window(ui, bar, ("the objectives'", "the objectives have"))?;
            if !w.objectives.is_empty() {
                let shows = w.objectives.iter().filter_map(|r| list::child(ui, r.tile, ROW_TEXT)).filter_map(|t| texts.get(&t)).any(|p| shows_whole(p).is_some() && within(window, p, true));
                if !shows {
                    return Err("no objective shows in its window".into());
                }
            }
            Ok(Some(window))
        }
        // The window is what clips the text itself: its rect and its scroll bar are hotrects, which
        // lay out no box of their own, and the bar shows only while the text scrolls [documented:
        // map_menu.xml, scrollbar_vert.xml's `visible`].
        Pane::Text(words) if words.trim().is_empty() => Ok(None),
        Pane::Text(_) => {
            let text = resolve_tile(ui, DATA_TEXT).ok_or("the text is gone")?;
            let window = pane_window(ui, text, ("the text's", "the text has"))?;
            if !texts.get(&text).is_some_and(|p| within(window, p, false)) {
                return Err("the text does not show in its window".into());
            }
            // A text that scrolls shows its bar's marker, to drag; one that fits needs no bar.
            // Use `tile_box` so a `<hotrect>` marker container works identically to an `<image>` [design].
            if w.pane_most > 0.0 {
                let marker = resolve_tile(ui, TEXT_BAR).and_then(|b| list::child(ui, b, list::MARKER)).ok_or("the text's scroll bar is gone")?;
                let shown = super::layout::tile_box(ui, marker, &|_, _, _| (0.0, 0.0), true).is_some_and(|b| b.visible);
                if !shown {
                    return Err("the text scrolls and its scroll bar is not shown".into());
                }
            }
            Ok(Some(window))
        }
        Pane::Image(file) => {
            let image = resolve_tile(ui, DATA_IMAGE).ok_or("the picture is gone")?;
            // Painted whole: on the canvas and inside what clips it (a clip above that cuts it off,
            // even wholly, leaves a piece on the canvas).
            if !file.is_empty() && !pieces.iter().any(|p| p.tile == image && on_canvas(&p.rect) && shows_whole(p).is_some()) {
                return Err("the picture is not shown".into());
            }
            Ok(None)
        }
        Pane::Audio { .. } => {
            let time = resolve_tile(ui, AUDIO_TIME).ok_or("the audio note's time is gone")?;
            if !texts.get(&time).is_some_and(|p| shows_whole(p).is_some()) {
                return Err("the audio note's time is not shown".into());
            }
            Ok(None)
        }
    }
}

/// Radio's waveform panel as laid out (canvas units): shown, with a size, wholly on the canvas;
/// the trace is painted inside it (`data::trace_over_tiles`), so a panel the file hides or moves
/// off the page leaves the hand-built page [design].
fn wave_box(ui: &Ui) -> Result<Rect, String> {
    let rect = ui.find_tile(WAVE_RECT).ok_or("the waveform panel is gone")?;
    let item = super::layout::layout(ui, rect, &|_, _, _| (0.0, 0.0), true).into_iter().find(|i| i.tile == rect).ok_or("the waveform panel is not laid out")?;
    if !item.visible {
        return Err("the waveform panel is not shown".into());
    }
    let r = item.rect;
    if !on_canvas(&r) {
        return Err("the waveform panel does not fit".into());
    }
    Ok(Rect::new(r.left as f32, r.top as f32, (r.left + r.width) as f32, (r.top + r.height) as f32))
}

/// Whether a painted rectangle lies wholly on the Pip-Boy's canvas.
fn on_canvas(r: &super::layout::Rect) -> bool {
    let canvas = (f64::from(crate::pipboy::CANVAS.x), f64::from(crate::pipboy::CANVAS.y));
    r.width > 0.0 && r.height > 0.0 && r.left >= -1.0 && r.top >= -1.0 && r.left + r.width <= canvas.0 + 1.0 && r.top + r.height <= canvas.1 + 1.0
}

/// The right pane's window as laid out (canvas units): what clips `inside`, a tile of the pane that
/// is laid out whatever it shows (the objectives' scroll bar [documented: list_box.xml], a note's
/// text); why it cannot serve otherwise: no clip (a pane without its window would show what is
/// scrolled out of it), no size, or not wholly on the canvas. The wheel over it and a page of it
/// are its own ([`DataState::publish_drawn`]). `(whose, has)`: the pane's possessive and its "has",
/// for the reasons.
fn pane_window(ui: &Ui, inside: TileId, (whose, has): (&str, &str)) -> Result<Rect, String> {
    // Use `tile_box` so a `<hotrect>` scrollbar container (`lb_scrollbar` in modded `list_box.xml`)
    // resolves its inherited clip window identically to a vanilla `<image>` scrollbar [design].
    let item = super::layout::tile_box(ui, inside, &|_, _, _| (0.0, 0.0), true).ok_or_else(|| format!("{whose} window is not laid out"))?;
    let c = item.clip.ok_or_else(|| format!("{has} no window"))?;
    let canvas = (f64::from(crate::pipboy::CANVAS.x), f64::from(crate::pipboy::CANVAS.y));
    if c.width <= 0.0 || c.height <= 0.0 || c.left < 0.0 || c.top < 0.0 || c.left + c.width > canvas.0 || c.top + c.height > canvas.1 {
        return Err(format!("{whose} window does not fit"));
    }
    Ok(Rect::new(c.left as f32, c.top as f32, (c.left + c.width) as f32, (c.top + c.height) as f32))
}

/// What [`page`] keeps: the rows, what was last written, and the reasons warned of.
#[derive(Default)]
pub struct Written {
    menu: Option<TileId>,
    /// The tiles [`built`] looked at, while the file is known to have them, and the tab they were
    /// for.
    checked: Vec<TileId>,
    checked_tab: Option<usize>,
    tabs: Vec<RowTile<usize>>,
    /// Each list's rows ([`ListKind::index`]), by their records' FormIDs (only the list shown has
    /// any), and the objectives', by place.
    rows: [Vec<RowTile<u32>>; 4],
    objectives: Vec<RowTile<usize>>,
    /// Each list's own scroll, and the list Misc showed last (the notes or the challenges).
    views: [ListView<u32>; 4],
    /// The World Map's rows and the Local Map's ([`write_world`]).
    world: WorldRows,
    local: WorldRows,
    misc_list: Option<ListKind>,
    sizes: SizesWritten,
    /// Actual own-image dimensions, including invisible metadata children used by
    /// replacement map-marker templates to derive their zoom and crop.
    file_sizes: HashMap<TileId, (String, f64)>,
    file_texels: HashMap<String, Option<(f64, f64)>>,
    shown: Option<Shown>,
    /// What the last pass was asked to show, kept through a failure ([`Written::waits`]).
    asked: Option<Shown>,
    flat: Option<FlatChanges>,
    /// How far the right pane (the objectives, a text) can scroll, as last measured.
    pane_most: f64,
    /// What is shown changed since the log last said so, and the line it said.
    tell: bool,
    told_line: Option<String>,
    counted: Checked,
    places_told: Option<String>,
    /// Final passes in a row that failed, the tab and frame the last was for, the tree it met, and
    /// its reason.
    failing: u32,
    tried: Option<(usize, bool)>,
    tried_tree: Option<TreeMark>,
    reason: String,
    fallbacks: HashSet<String>,
    /// The last failure was the layout's ([`usable`]): places and traits a script or the file can
    /// put right without changing the tree's shape, so it is tried again now and then
    /// ([`LAYOUT_RETRY_PASSES`]); a missing tile or template waits for the tree to change.
    layout_failure: bool,
    /// Passes waited since the last try of a layout failure.
    waited: u32,
}

/// Radio's waveform panel as the page drawn from its file shows it: its box (canvas units) and
/// whether the radio plays, for the trace painted over it (`data::trace_over_tiles`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaveShown {
    pub rect: Rect,
    pub on: bool,
}

/// The decision every reader shares, and what the page keeps.
#[derive(Resource, Default)]
pub struct MapTiles {
    /// The menu, while the page is ready to display this frame ([`settle`]).
    pub ready: Option<TileId>,
    /// The menu painted by the last drawing (`super::draw`): what the player sees.
    pub shown: Option<TileId>,
    /// The pieces [`usable`] checked this frame, for the drawing to paint.
    pub(crate) pieces: Vec<Piece>,
    /// Radio's waveform panel, while the page is ready on Radio ([`settle`]).
    pub wave: Option<WaveShown>,
    /// The Local Map's outline picture last given as its runtime picture ([`sync_outline`]).
    local_image: Option<Handle<Image>>,
    /// The frames of this visit to a tab drawn from the file, by what drew them ([`settle`]).
    frames: Frames,
    /// [`CUT_AID`] and its time.
    pub cut: Option<String>,
    pub cut_until: Option<f32>,
    /// A menu over the page when this frame began (`stats_menu::modal_over`), while it is drawn
    /// from its tiles: the shell's keys wait (`crate::pipboy::takes_keys`).
    pub modal: bool,
    /// A menu over the DATA page, on any tab, when this frame began: set before the scripts, so it
    /// holds through the frame that menu closes in. The page's own input waits
    /// (`crate::pipboy::data::prepare`), with the switch on only.
    pub over_data: bool,
    /// The question box over the page drawn from its tiles owns this frame's tile input (the
    /// pointer and the file's key bindings on the page's tiles): set by [`feed`] before it acts
    /// while the box is up, and held after it is answered until the left button is let go, so the
    /// answer's click goes through to nothing under it [design]. The shell's keys are not held
    /// (`crate::pipboy::takes_keys`): Tab and Esc close the Pip-Boy, the box with it, as on the
    /// hand-built page. The box's own keys and clicks are the model's (`data::popup_input`);
    /// `over_data` is not used for it, as that stops the model's input altogether.
    pub popup_owns: bool,
    /// Another menu took this frame's tile input (`super::input::feed`).
    pub others: bool,
    /// The quests list's marker held, and the objectives' (the right pane's, by the file's bar).
    drag: Option<list::Drag>,
    pane_drag: Option<list::Drag>,
    /// The page's draw state last logged: the tab, and drawn from tiles or why not.
    told: Option<(usize, Result<(), String>)>,
    written: Written,
}
impl MapTiles {
    pub fn list_scroll(&self) -> Option<f64> {
        self.written.shown.as_ref().filter(|s| s.tab >= data::QUESTS)?;
        Some(self.written.views[self.written.list().index()].value)
    }
    pub(crate) fn scroll_list_by(&mut self, by: crate::pipboy::scrollbar::ScrollBy) -> bool {
        if self.list_scroll().is_none() { return false; }
        let view = self.written.view();
        by.value(view.value, view.counts.most(), view.counts.visible).is_some_and(|to| view.scroll_to(to))
    }
}

/// The tiles of [`required_paths`] for tab `tab`, or why the page cannot be drawn.
fn check_file(ui: &Ui, cut: Option<&str>, tab: Option<usize>) -> Result<Vec<TileId>, String> {
    match built(ui, cut, tab) {
        Tiled::No(reason) => Err(reason),
        Tiled::Yes => Ok(required_paths(tab).iter().filter_map(|p| resolve_tile(ui, p)).collect()),
    }
}

impl Written {
    /// One map's rows: the Local Map's or the World Map's.
    fn map_rows(&mut self, local: bool) -> &mut WorldRows {
        if local { &mut self.local } else { &mut self.world }
    }

    /// The list the page shows (Quests' before anything is shown).
    fn list(&self) -> ListKind {
        self.shown.as_ref().map(|s| s.list).unwrap_or_default()
    }

    /// The shown list's own scroll.
    fn view(&mut self) -> &mut ListView<u32> {
        let k = self.list().index();
        &mut self.views[k]
    }
}

/// One pass over the page; the menu when all of it went through. `new`: what the model shows, when
/// it may have changed. `late`: the pass before the drawing, which measures and checks.
pub(crate) fn page(ui: &mut Ui, new: Option<Shown>, late: bool, tiles: &mut MapTiles, fonts: &mut dyn PageFonts) -> Result<TileId, String> {
    let menu = ui.menu(MAP_MENU).ok_or_else(|| format!("{MAP_MENU} is not in the tree"))?;
    // A new menu tile (the tree built again): its rows, sizes and writes are all to do again.
    if tiles.written.menu != Some(menu) {
        let fallbacks = std::mem::take(&mut tiles.written.fallbacks);
        tiles.written = Written { menu: Some(menu), fallbacks, ..Default::default() };
        tiles.drag = None;
    }
    let w = &mut tiles.written;
    w.layout_failure = false;
    let tab = new.as_ref().or(w.shown.as_ref()).map(|s| s.tab);
    if w.checked.is_empty() || w.checked_tab != tab || !w.checked.iter().all(|&t| ui.is_live(t)) {
        w.checked.clear();
        w.checked = check_file(ui, tiles.cut.as_deref(), tab)?;
        w.checked_tab = tab;
    }
    let made = ensure_tabs(ui, &mut w.tabs)?;
    let mut changed = false;
    if let Some(s) = new {
        let rows_dead = !w.rows.iter().flatten().all(|r| ui.is_live(r.tile)) || !w.objectives.iter().all(|r| ui.is_live(r.tile)) || !w.world.all_live(ui) || !w.local.all_live(ui);
        if rows_dead || w.shown.as_ref() != Some(&s) {
            write_model(ui, &s, w)?;
            w.shown = Some(s);
            w.tell = true;
            changed = true;
        }
    }
    let Some(s) = w.shown.clone() else { return Err("nothing to show yet".into()) };
    let kind = s.list;
    let world = s.world.as_ref();
    // Misc toggled between the notes and the challenges: the model starts the list it shows at its
    // first row (`data::button`), so that list starts at its top too, whatever its own scroll was
    // when it last showed [design].
    if world.is_none() && kind.misc() && w.misc_list != Some(kind) {
        if w.misc_list.is_some() {
            w.views[kind.index()].restart();
        }
        w.misc_list = Some(kind);
    }
    // The tab's list; the World Map has none.
    let list_tile = match world {
        Some(_) => None,
        None => Some(resolve_tile(ui, kind.path()).ok_or_else(|| format!("the {} list is gone", kind.words().0))?),
    };
    if changed || made || late {
        write_tabs(ui, &w.tabs, s.tab);
        ui.update();
        // After what the hints' and the lists' expressions read, before their words are measured.
        keep_hints(ui, &s.hints);
        keep_lists(ui, &s);
        stats_menu::write_file_sizes(ui, menu, &mut w.file_sizes, &mut w.file_texels, fonts);
        let texts = menus::texts(ui, menu);
        ui.update();
        for _ in 0..3 {
            if fonts.write_sizes(ui, &texts, &mut w.sizes) == 0 {
                break;
            }
            ui.update();
        }
        place_tabs(ui, &w.tabs);
        // The clip windows on before the panes read what clips them.
        super::item_menu::mark_menu(ui, MAP_MENU, &[QUESTS_LIST, NOTES_LIST, CHALLENGE_LIST, STATIONS_LIST]);
        unclip_map_window_children(ui);
        if let Some(objectives) = resolve_tile(ui, OBJECTIVES) {
            list::unclip_row_children(ui, objectives, w.objectives.iter().map(|r| r.tile));
        }
        if let Some(list_tile) = list_tile {
            list::settle_list(ui, list_tile, &w.rows[kind.index()], ROW_TEXT, &mut w.views[kind.index()], s.selected, s.group, s.pointer_pick, s.navigated);
        }
        w.pane_most = match &s.pane {
            Pane::Objectives(_) => {
                let objectives = resolve_tile(ui, OBJECTIVES).ok_or("the objectives list is gone")?;
                let heights = list::measure_rows(ui, &w.objectives, ROW_TEXT);
                write_pane(ui, objectives, &heights, s.pane_first)
            }
            Pane::Text(_) => write_text_pane(ui, s.pane_first),
            _ => 0.0,
        };
        let title = resolve_tile(ui, TITLE).ok_or("the title is gone")?;
        let line = resolve_tile(ui, LINE).ok_or("the heading's line is gone")?;
        stats_menu::write_frame_of(ui, title, line, s.device, &mut w.flat);
    }
    keep_hints(ui, &s.hints);
    keep_lists(ui, &s);
    match (list_tile, world) {
        // The scrolls over the file's own count of the clicks.
        (Some(list_tile), _) => {
            let view = &w.views[kind.index()];
            view.rewrite(ui, list_tile);
            if !s.boxed {
                list::write_highlight(ui, list_tile, None, view.value);
            }
        }
        (None, Some(world)) => {
            // The picture's size first, then its place: the file works its place out from its size.
            keep_world(ui, world);
            ui.update();
            keep_world(ui, world);
        }
        (None, None) => {}
    }
    if let Some(bar) = pane_bar(ui, &s.pane) {
        list::write_bar_value(ui, bar, f64::from(s.pane_first).clamp(0.0, w.pane_most));
    }
    ui.update();
    if late {
        let mut pieces = fonts.set_out(ui, menu);
        if let Some(world) = s.world.as_ref() {
            let paths = world.paths();
            pieces = with_map_picture(ui, pieces, map_picture_piece(ui, paths.clip, paths.picture));
            pieces = with_map_companions(ui, pieces, map_companion_pieces(ui, world));
        }
        let places = tab_places(ui, &w.tabs);
        if w.places_told.as_ref() != Some(&places) {
            info!("tiles: {MAP_MENU} tabs stand: {places}");
            w.places_told = Some(places);
        }
        w.counted = match usable(ui, menu, w, s.tab, &pieces) {
            Ok(c) => c,
            Err(reason) => {
                w.layout_failure = true;
                return Err(reason);
            }
        };
        if let Some(missed) = w.counted.missed.clone() {
            if w.fallbacks.insert(told_once(&missed)) {
                warn!("{MAP_MENU}: {missed}");
            }
        }
        tiles.pieces = pieces;
        // Told when the line changes: an audio note playing changes what is shown every frame (its
        // time left), not the line.
        if std::mem::take(&mut w.tell) {
            let line = describe(w);
            if w.told_line.as_ref() != Some(&line) {
                info!("{line}");
                w.told_line = Some(line);
            }
        }
    }
    Ok(menu)
}

/// Where the tabs stand as the tree holds them, for the log when it changes.
fn tab_places(ui: &Ui, tabs: &[RowTile<usize>]) -> String {
    let n = |x: f64| Value::Num(x).text();
    let places: Vec<String> = tabs.iter().map(|r| format!("{} at {}, {} size {} x {}", r.key, n(num(ui, r.tile, "x")), n(num(ui, r.tile, "y")), n(num(ui, r.tile, "width")), n(num(ui, r.tile, "height")))).collect();
    places.join("; ")
}

/// The tab's name for the log, as the hand-built page's hints line names it.
fn tab_name(tab: usize) -> &'static str {
    ["local map", "world map", "quests", "misc", "radio"].get(tab).copied().unwrap_or("?")
}

/// A line for the log when what the page shows changes (smoke reads these).
fn describe(w: &Written) -> String {
    let Some(s) = w.shown.as_ref() else { return format!("tiles: {MAP_MENU} page written") };
    let selected = s.selected.and_then(|i| s.rows.get(i)).map_or("none", |r| r.text.as_str());
    let hints: Vec<String> = s.hints.iter().map(|h| format!("{:?} {} {}{}", h.slot, h.word, h.key, if h.dim { " (dim)" } else { "" })).collect();
    let hints = if hints.is_empty() { "none".to_string() } else { hints.join(", ") };
    let frame = if s.device { "device" } else { "screen menu" };
    if let Some(m) = s.world.as_ref().filter(|m| m.local) {
        return format!(
            "tiles: {MAP_MENU} page from tiles: tab local map, {} markers, {} quest markers, {}, picture {}, \"{}\" highlighted, magnification {:.2}, hints: {hints}, {frame}",
            m.markers.len(),
            m.quests.len(),
            if m.custom.is_some() { "custom marker" } else { "no custom marker" },
            m.outline.as_ref().map_or_else(|| "not made yet".to_string(), |o| format!("{} x {}", o.1.x, o.1.y)),
            m.label.as_deref().unwrap_or("nothing"),
            m.mag,
        );
    }
    if let Some(m) = &s.world {
        return format!(
            "tiles: {MAP_MENU} page from tiles: tab world map, {} markers, {} quest markers, {}, {} dotted lines, \"{}\" highlighted, magnification {:.2}, hints: {hints}, {frame}",
            m.markers.len(),
            m.quests.len(),
            if m.custom.is_some() { "custom marker" } else { "no custom marker" },
            if m.arrow.is_some() { m.ends.len() } else { 0 },
            m.label.as_deref().unwrap_or("nothing"),
            m.mag,
        );
    }
    // Quests: how many objectives; Misc: which list, and what the right pane shows.
    let (list, pane) = match (s.list, &s.pane) {
        (ListKind::Quests, p) => (String::new(), format!("{} objectives", p.objectives().len())),
        (k, Pane::Text(_)) => (format!(" {}", k.words().0), "text".to_string()),
        (k, Pane::Image(_)) => (format!(" {}", k.words().0), "picture".to_string()),
        (k, Pane::Audio { .. }) => (format!(" {}", k.words().0), "audio".to_string()),
        (k, Pane::Wave { on }) => (format!(" {}", k.words().0), format!("waveform {}", if *on { "on" } else { "off" })),
        (k, _) => (format!(" {}", k.words().0), "no pane".to_string()),
    };
    format!("tiles: {MAP_MENU} page from tiles: tab {}{list}, {} rows, \"{selected}\" selected, {pane}, hints: {hints}, {frame}", tab_name(s.tab), s.rows.len())
}

/// What a warning is told once by: its line without the place and the row.
fn told_once(line: &str) -> String {
    line.split(" at (").next().unwrap_or(line).chars().filter(|c| !c.is_ascii_digit()).collect()
}

/// Whether the page is eligible to prepare: the switch on and DATA showing on a tab it draws.
pub fn eligible(switch: TileMenus, pip: &PipBoy) -> bool {
    switch.map && pip.shown() && pip.page == Page::Data && TILED_TABS.contains(&pip.tab())
}

/// The keys the hand-built page acts on itself (`crate::pipboy::data`'s keys, `crate::pipboy::keys`),
/// by the tile input's names, and the controller's buttons standing in for them: the file's root
/// binds X, Y and R to its hint tiles [documented], which would act twice.
pub const HAND_BUILT_KEYS: [&str; 19] = ["W", "S", "A", "D", "E", "R", "X", "xup", "xdown", "xleft", "xright", "xbuttona", "xbuttonb", "xbuttonx", "xbuttony", "xbuttonlb", "xbuttonrb", "xbuttonlt", "xbuttonrt"];

/// Whether a click a key made on the page's tiles acts: as `menus::key_click_acts`, and never for a
/// controller's button the hand-built page answers to.
fn key_acts(key: &str, physical: &[&str]) -> bool {
    menus::key_click_acts(MAP_MENU, key, physical) && !(key.to_ascii_lowercase().starts_with("xbutton") && menus::hand_built_takes(MAP_MENU, key))
}

/// The menu, when the page takes this frame's tile input: painted last frame, and the page up with
/// nothing over it. Every reader of the frame's input decides by this.
pub fn takes(tiles: &MapTiles, switch: TileMenus, pip: &PipBoy, game: &fnv_game::Game<'_>, paused: bool) -> Option<TileId> {
    if !pip.open() || !eligible(switch, pip) || paused || modal_over(game.menu) {
        return None;
    }
    tiles.shown.filter(|&m| game.ui_files.is_some() && game.ui_tree.is_live(m))
}

/// What a pass over the page reads.
#[derive(SystemParam)]
pub(crate) struct MapPage<'w> {
    script: Option<ResMut<'w, ScriptState>>,
    switch: Option<Res<'w, TileMenus>>,
    pip: Option<Res<'w, PipBoy>>,
    data: Option<ResMut<'w, DataState>>,
    tiles: ResMut<'w, MapTiles>,
    /// Holds the runtime pictures read-only too (`super::draw::FontKit`): the Local Map's outline
    /// is given to them by [`give_outline`], a system of its own.
    fonts: super::draw::FontKit<'w>,
    time: crate::pause::AidTime<'w>,
}

/// The Local Map's outline as its runtime picture ([`sync_outline`]), from what the preparation
/// gathered this frame (`data::prepare`, before this), before the decision writes its name
/// ([`settle`]): a system of its own, as the page's passes read the runtime pictures through the
/// drawing's fonts and cannot also change them.
pub(crate) fn give_outline(switch: Option<Res<TileMenus>>, pip: Option<Res<PipBoy>>, data: Option<Res<DataState>>, images: Option<ResMut<super::runtime_images::RuntimeImages>>, mut tiles: ResMut<MapTiles>) {
    let (Some(switch), Some(pip), Some(data), Some(mut images)) = (switch, pip, data, images) else { return };
    if !eligible(*switch, &pip) || pip.tab() != data::LOCAL {
        return;
    }
    let Some(prepared) = data.prepared().filter(|p| p.view.tab == data::LOCAL) else { return };
    let Body::Map(m) = &prepared.view.body else { return };
    let outline = m.world.as_ref().and_then(|w| w.outline.as_ref());
    let tiles = tiles.bypass_change_detection();
    if sync_outline(&mut images, outline, &mut tiles.local_image) {
        info!("tiles: {MAP_MENU} local map picture {}", outline.map_or_else(|| "taken away".to_string(), |o| format!("given, {} x {} texels", o.1.x, o.1.y)));
    }
}

/// The Local Map's outline picture as the runtime picture [`LOCAL_IMAGE`]: given again for each
/// new picture (the outline is made anew when the map is panned off it, the player goes elsewhere
/// or goes in or out; its picture is made once its grid is filled in), before the page writes its
/// name; taken away when there is none. `last` is the picture given last. Returns whether it
/// changed. The texels of a picture given never change in place (a new outline is a new picture),
/// so it is never touched [design: `outline::Outline`].
fn sync_outline(images: &mut super::runtime_images::RuntimeImages, outline: Option<&(Handle<Image>, Vec2)>, last: &mut Option<Handle<Image>>) -> bool {
    match outline {
        Some((handle, size)) if last.as_ref() != Some(handle) => {
            images.set(LOCAL_IMAGE, handle.clone(), *size);
            *last = Some(handle.clone());
            true
        }
        None if last.is_some() => {
            images.remove(LOCAL_IMAGE);
            *last = None;
            true
        }
        _ => false,
    }
}

/// A pass; `None` when the page is not eligible or has nothing to show yet (nothing is touched).
fn pass(p: &mut MapPage<'_>, late: bool) -> Option<Result<TileId, String>> {
    let switch = p.switch.as_deref().copied().unwrap_or_default();
    let pip = p.pip.as_deref()?;
    if !eligible(switch, pip) {
        return None;
    }
    let (tab, device) = (pip.tab(), pip.frame == Frame::Device);
    let script = p.script.as_mut()?.bypass_change_detection();
    script.rt.as_ref()?.game.ui_files.as_ref()?;
    let data = p.data.as_mut()?;
    // What the preparation gathered this frame for this tab (`data::prepare` ran before this).
    let new = match data.prepared().filter(|pr| pr.view.tab == tab) {
        Some(pr) => match Shown::of(pr, device) {
            Ok(s) => Some(s),
            Err(reason) => return Some(Err(reason)),
        },
        None => None,
    };
    if new.is_none() && p.tiles.written.shown.is_none() {
        return None;
    }
    let notches = data.take_list_notches();
    if notches != 0.0 {
        let view = p.tiles.written.view();
        let v = view.value;
        view.scroll_to(v - f64::from(notches).round());
    }
    let tree = TreeMark::of(&script.rt.as_ref()?.game.ui_tree);
    // A map in the window this page lays out, before anything is written of it.
    let tree_ui = &script.rt.as_ref()?.game.ui_tree;
    let gathered_in = new.as_ref().and_then(|s| s.world.as_ref()).map(|w| w.window);
    let mut new = new.map(|s| refit_to_tree(tree_ui, s));
    // Drawn in another window than it was gathered in: the model's highlight box, its pick, the
    // Travel target and Place Marker's place follow the drawing (no input taken again), and the
    // Travel hint is lit or dim by the Travel target picked again.
    if let Some(s) = new.as_mut().filter(|s| s.world.as_ref().is_some_and(|w| Some(w.window) != gathered_in)) {
        let w = s.world.as_ref()?;
        let picked = w.markers.iter().find(|m| m.selected).map(|m| m.form);
        let forms: Vec<u32> = w.markers.iter().map(|m| m.form).collect();
        let travel = data.refit_map(tab, w.view, w.want, w.cursor, picked, &forms);
        for h in s.hints.iter_mut().filter(|h| h.kind == ButtonKind::Travel) {
            h.dim = travel.is_none();
        }
    }
    // What it would show changed since the last pass, written or not (a failure forgets what was
    // written, not what was asked for).

    let changed = new.is_some() && new != p.tiles.written.asked;
    if new.is_some() {
        p.tiles.written.asked.clone_from(&new);
    }
    if p.tiles.written.waits(tab, device, tree, changed) {
        return Some(Err(p.tiles.written.reason.clone()));
    }
    let rt = script.rt.as_mut()?;
    let out = page(&mut rt.game.ui_tree, new, late, &mut p.tiles, &mut p.fonts);
    if late && out.is_ok() {
        // The one presentation this frame: what the page's rules read of the drawing.
        let c = &p.tiles.written.counted;
        data.publish_drawn(tab, c.rows_in_clip, p.tiles.written.pane_most as f32, c.pane);
        if let Some((window, hints)) = c.map.clone() {
            data.publish_map(tab, window, hints);
        }
    }
    Some(out)
}

/// The map's window as the tree places it (canvas units: the clip window's place summed up its
/// parents, its size), when it has one with a size.
fn map_window_of(ui: &Ui, clip: &str) -> Option<Rect> {
    let t = ui.find_tile(clip)?;
    let (x, y) = origin(ui, t);
    let (w, h) = (num(ui, t, "width"), num(ui, t, "height"));
    (w > 0.0 && h > 0.0).then(|| Rect::new(x as f32, y as f32, (x + w) as f32, (y + h) as f32))
}

/// `s` with its map ([`WorldShown::refit`]) in the window the tree places, so the page writes it
/// where it lays it out; any other page as it is.
fn refit_to_tree(ui: &Ui, mut s: Shown) -> Shown {
    if let Some(w) = s.world.as_mut() {
        if let Some(window) = map_window_of(ui, w.paths().clip) {
            w.refit(window);
        }
    }
    s
}

/// Before the scripts, after the tile input: the scrolls written again over the file's own count of
/// the clicks.
pub(crate) fn write(mut p: MapPage) {
    // A failure here is told by `settle`, which decides.
    let _ = pass(&mut p, false);
}

/// Before the drawing, after the page's preparation: the tree worked out, and the one decision
/// every reader of this frame shares.
pub(crate) fn settle(mut p: MapPage) {
    if cut_over(p.switch.as_deref().copied().unwrap_or_default(), p.tiles.cut_until, p.time.elapsed_secs()) {
        info!("test aid: {CUT_AID} over");
        p.tiles.cut = None;
        p.tiles.cut_until = None;
        p.tiles.written.checked.clear();
        if let Some(game) = p.script.as_mut().and_then(|s| s.bypass_change_detection().rt.as_mut()).map(|rt| &mut rt.game) {
            rebuild_menu(game);
        }
    }
    let switch = p.switch.as_deref().copied().unwrap_or_default();
    let on_data = switch.map && p.pip.as_deref().is_some_and(|pip| pip.shown() && pip.page == Page::Data);
    let tab = p.pip.as_deref().map_or(0, PipBoy::tab);
    let (ready, state) = match pass(&mut p, true) {
        Some(Ok(menu)) => {
            p.tiles.written.failing = 0;
            (Some(menu), Some(Ok(())))
        }
        Some(Err(reason)) => {
            let tried = p.pip.as_deref().map(|pip| (pip.tab(), pip.frame == Frame::Device));
            let tree = p.script.as_ref().and_then(|s| s.rt.as_ref()).map(|rt| TreeMark::of(&rt.game.ui_tree));
            let w = &mut p.tiles.written;
            w.failed(tried, tree);
            let line = format!("{MAP_MENU} drawn by hand: {reason}");
            if w.failing >= FAILING_PASSES && w.fallbacks.insert(told_once(&line)) {
                warn!("{line}");
            }
            let state = (w.failing >= FAILING_PASSES).then(|| Err(reason.clone()));
            w.reason = reason;
            (None, state)
        }
        None => {
            p.tiles.written.failing = 0;
            let state = (on_data && !TILED_TABS.contains(&tab)).then(|| Err("not drawn from the file yet".to_string()));
            (None, state)
        }
    };
    // Every frame on a tab drawn from the file counted by what drew it, told when the visit ends,
    // so a run can show that the page never fell back, not even for a frame (smoke reads these).
    let counted = on_data.then_some(tab).filter(|t| TILED_TABS.contains(t));
    let up = p.pip.as_deref().is_some_and(PipBoy::open);
    let drew = match (&ready, &state) {
        (Some(_), _) => Drew::Tiles,
        (None, Some(Err(_))) => Drew::Failed,
        (None, _) if p.tiles.written.failing > 0 => Drew::Failed,
        (None, _) if up => Drew::Unprepared,
        (None, _) => Drew::Moving,
    };
    if let Some((tab, [tiles, failed, unprepared, moving])) = p.tiles.frames.step(counted, drew) {
        info!("tiles: {MAP_MENU} {} frames: {tiles} from tiles, {failed} by hand after a failed pass, {unprepared} by hand with nothing prepared, {moving} by hand as the Pip-Boy came up or went down", tab_name(tab));
    }
    // One line a change of the page's draw state, on DATA (smoke reads these).
    if let Some(state) = state.filter(|_| on_data) {
        let now = Some((tab, state.clone()));
        if p.tiles.told != now {
            match &state {
                Ok(()) => info!("tiles: {MAP_MENU} draws {} from tiles", tab_name(tab)),
                Err(reason) => info!("tiles: {MAP_MENU} leaves {} hand-built: {reason}", tab_name(tab)),
            }
            p.tiles.told = now;
        }
    } else if !on_data {
        p.tiles.told = None;
    }
    // Radio's panel for the trace painted over it, while the page is ready on Radio.
    let wave = ready.and_then(|_| wave_of(&p.tiles.written));
    if p.tiles.wave != wave {
        p.tiles.wave = wave;
    }
    if ready.is_none() {
        // The next time it is written whole; the rows stay.
        p.tiles.written.shown = None;
        p.tiles.pieces.clear();
    }
    if p.tiles.ready != ready {
        p.tiles.ready = ready;
    }
}

/// Radio's waveform panel as the last pass that went through wrote and measured it: its box and
/// whether the radio plays; none on the other tabs.
fn wave_of(w: &Written) -> Option<WaveShown> {
    let Pane::Wave { on } = w.shown.as_ref()?.pane else { return None };
    Some(WaveShown { rect: w.counted.wave?, on })
}

/// What drew a frame of a tab drawn from the file ([`Frames`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drew {
    /// The page from its tiles.
    Tiles,
    /// The hand-built page, the tiles' pass having failed (warned of only after
    /// [`FAILING_PASSES`]).
    Failed,
    /// The hand-built page with the Pip-Boy up, the tiles having nothing to draw from (nothing
    /// prepared for the tab, no menu files, no game): a fallback shown as surely as a failure.
    Unprepared,
    /// The hand-built page while the Pip-Boy comes up or goes down (shown, not open) with
    /// nothing prepared for the tab: the first time it comes up on the tab [design: the
    /// preparation runs only while the Pip-Boy is open].
    Moving,
}

/// The frames of one visit to a tab drawn from the file, by what drew them: a positive record
/// that it never fell back (a fallback is warned of only after [`FAILING_PASSES`]) [design].
#[derive(Default, Debug)]
pub(crate) struct Frames {
    tab: Option<usize>,
    counts: [u32; 4],
}

impl Frames {
    /// Count a frame on tab `tab` (none: off DATA, or a tab drawn by hand), drawn by `drew`; the
    /// visit that ended, with its counts by [`Drew`] (from tiles, after a failed pass, with nothing
    /// prepared, as the Pip-Boy moved), when the tab changed.
    fn step(&mut self, tab: Option<usize>, drew: Drew) -> Option<(usize, [u32; 4])> {
        let ended = (self.tab != tab).then(|| self.tab.map(|t| (t, std::mem::take(&mut self.counts)))).flatten();
        if self.tab != tab {
            self.tab = tab;
            self.counts = [0; 4];
        }
        if tab.is_some() {
            self.counts[drew as usize] += 1;
        }
        ended
    }
}

/// Whether [`CUT_AID`]'s time is over now: only with the switch on, as nothing else of the page runs
/// off (its end builds the menu again).
fn cut_over(switch: TileMenus, until: Option<f32>, now: f32) -> bool {
    switch.map && until.is_some_and(|until| now >= until)
}

/// Final passes in a row that fail before the reason is warned of, and before the page waits for
/// a change to try again.
const FAILING_PASSES: u32 = 3;
/// Passes (two a frame) a layout's failure waits before it is tried again with nothing changed:
/// about half a second at 60 frames a second, one measured pass then [design].
const LAYOUT_RETRY_PASSES: u32 = 60;

/// What the tree was at a failed pass: the menu's tile and the tree's shape (`Ui::shape`) [design].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TreeMark {
    menu: Option<TileId>,
    shape: u64,
}

impl TreeMark {
    fn of(ui: &Ui) -> TreeMark {
        TreeMark { menu: ui.menu(MAP_MENU), shape: ui.shape() }
    }
}

impl Written {
    /// A final pass failed, for this tab and frame, in this tree.
    fn failed(&mut self, tried: Option<(usize, bool)>, tree: Option<TreeMark>) {
        self.failing += 1;
        self.tried = tried;
        self.tried_tree = tree;
    }

    /// The page has fallen back and waits: nothing it would show changed, the tab and frame are the
    /// failure's, and the tree is as that failure met it. A layout's failure is tried again every
    /// [`LAYOUT_RETRY_PASSES`] all the same: what it met (a place, `visible`, `target`) can change
    /// without the tree's shape changing.
    fn waits(&mut self, tab: usize, device: bool, tree: TreeMark, changed: bool) -> bool {
        let same = self.failing >= FAILING_PASSES && !changed && self.tried == Some((tab, device)) && self.tried_tree == Some(tree);
        if !same {
            self.waited = 0;
            return false;
        }
        if self.layout_failure {
            self.waited += 1;
            if self.waited >= LAYOUT_RETRY_PASSES {
                self.waited = 0;
                return false;
            }
        }
        true
    }
}

/// [`CUT_AID`]'s end: the menu built again from its file, as at load.
fn rebuild_menu(game: &mut fnv_game::Game<'_>) {
    let (Some(files), Some(path)) = (game.ui_files.as_deref(), game.ui_menus.path(MAP_MENU)) else { return };
    match game.ui_tree.load_menu(files, path) {
        Ok(_) => {
            game.ui_tree.update();
            info!("test aid: {MAP_MENU} built again from {path}");
        }
        Err(e) => warn!("test aid: {MAP_MENU} not built again: {e}"),
    }
}

/// What [`feed`] keeps between frames.
#[derive(Default)]
pub(crate) struct Feed {
    /// The page took input last frame, in this frame of the Pip-Boy.
    taking: Option<Frame>,
    touch_drag: bool,
}

/// What the page's tile input reads of the Pip-Boy.
#[derive(SystemParam)]
pub(crate) struct FeedPip<'w> {
    pip: Res<'w, PipBoy>,
    screen: Option<Res<'w, PipBoyScreen>>,
    aid: Res<'w, PipBoyAid>,
    paused: Option<Res<'w, crate::pause::Paused>>,
    bindings: Option<Res<'w, crate::bindings::Bindings>>,
    touch:Option<Res<'w,crate::pipboy::touch_items::TouchItems>>,
    admission: crate::pipboy::touch_items::Admission<'w>,

}

/// What the page's tile input sends: where the hand-built page sends the same.
#[derive(SystemParam)]
pub(crate) struct FeedOut<'w> {
    tabs: MessageWriter<'w, PipBoyInput>,
    hovered: MessageWriter<'w, TabHovered>,
    data: Option<ResMut<'w, DataState>>,
}

/// The right pane's bar piece for a piece of the file's scroll bar: an arrow a wheel notch, a page
/// area a page of the window as measured (`data::bar_act`); the marker is held by the file's own
/// bar instead ([`pane_grab`]).
fn pane_part(piece: BarPiece) -> Option<BarPart> {
    Some(match piece {
        BarPiece::Up => BarPart::Up,
        BarPiece::Down => BarPart::Down,
        BarPiece::PageUp => BarPart::PageUp,
        BarPiece::PageDown => BarPart::PageDown,
        BarPiece::Marker => return None,
    })
}

pub(crate) fn touch_target(tiles:&MapTiles,ui:&Ui,menu:TileId,point:Vec2,data:&DataState)->Option<crate::pipboy::touch_scroll::NativeTarget>{
    use crate::pipboy::touch_scroll::{NativeRow,NativeTarget};
    NativeTarget::at(ui,menu,point,|tile|match data_tile(ui,&tiles.written,tile) {
        Some(DataTile::Row(index))=> {let list=tiles.written.list().index();tiles.written.rows[list].get(index).map(|r|NativeRow::Data{list,key:r.key})},
        Some(DataTile::Button(slot))=> {let shown=tiles.written.shown.as_ref()?;let hint=shown.hints.iter().find(|h|h.slot==slot)?;
            Some(NativeRow::DataButton{kind:hint.kind as u8,list:shown.list.index(),group:shown.group,key:shown.selected.and_then(|i|shown.rows.get(i)).map(|r|r.key),context:data.touch_action_context(shown.tab)})},
        _=>None,
    },|_|None,|_|true)
        .map(|target| target.with_control(|tile|matches!(data_tile(ui,&tiles.written,tile),Some(DataTile::ListBar(BarPiece::Marker)|DataTile::PaneBar(BarPiece::Marker)))))
}
pub(crate) fn touch_navigation(tiles:&MapTiles,ui:&Ui)->Vec<crate::pipboy::tab_swipe::Region> {
    use crate::pipboy::tab_swipe::{native_region,Kind};
    let title=ui.find_tile(TITLE);
    tiles.pieces.iter().filter_map(|p| {
        let kind=if Some(p.tile)==title {Kind::Sections}
            else if matches!(data_tile(ui,&tiles.written,p.tile),Some(DataTile::Tab(_))) {Kind::Subtabs}
            else {return None};
        native_region(p,kind)
    }).collect()
}
/// The page's tile input, on a frame it takes it ([`takes`]): see the module notes.
pub(crate) fn feed(mut script: ResMut<ScriptState>, switch: Res<TileMenus>, from: FeedPip, mut tiles: ResMut<MapTiles>, devices: Devices, mut out: FeedOut, mut state: Local<Feed>) {
    if !switch.map {
        return;
    }
    let FeedPip { pip, screen, aid, paused, bindings, touch, admission } = from;
    let Some(rt) = script.rt.as_mut() else { return };
    let game = &mut rt.game;
    let over_data = pip.open() && pip.page == Page::Data && modal_over(game.menu);
    if tiles.over_data != over_data {
        tiles.over_data = over_data;
    }
    let modal = over_data && eligible(*switch, &pip);
    if tiles.modal != modal {
        tiles.modal = modal;
    }
    let is_paused = paused.as_ref().is_some_and(|p| p.0);
    // The question box over the page drawn from its tiles owns the frame's tile input while it is up,
    // and until the left button is let go after it (its answer's click acts on nothing under it).
    let box_up = tiles.shown.is_some() && pip.open() && pip.page == Page::Data && out.data.as_ref().is_some_and(|d| d.popup().is_some());
    let button_held = (aid.click > 0 && !aid.right) || devices.mouse.pressed(MouseButton::Left);
    let owns = popup_owns(box_up, tiles.popup_owns, button_held);
    if tiles.popup_owns != owns {
        tiles.popup_owns = owns;
    }
    let menu = takes(&tiles, *switch, &pip, game, is_paused).filter(|_| !tiles.others && !owns);
    let (Some(menu), Some(data)) = (menu, out.data.as_mut()) else {
        *state = Feed::default();
        tiles.drag = None;
        tiles.pane_drag = None;
        return;
    };
    // With Controller chosen the hand-built page ignores the mouse pointer; so does this one.
    let controller = bindings.as_ref().is_some_and(|b| b.device == crate::bindings::InputDevice::Controller);
    let mut mobile=crate::pipboy::touch_scroll::native_frame(touch.as_deref(),&devices.mouse);
    if !admission.allows_contact(touch.as_ref().and_then(|t|t.contact_id())) { mobile=Default::default(); }
    if mobile.tap {
        let target=mobile.at.and_then(|at|touch_target(&tiles,&game.ui_tree,menu,at,data));
        mobile.tap=touch.as_ref().is_some_and(|t|t.native_release_valid(target));
    }
    if mobile.control && mobile.started {
        let target=mobile.at.and_then(|at|touch_target(&tiles,&game.ui_tree,menu,at,data));
        mobile.started=target.is_some_and(|t|t.control()) && touch.as_ref().is_some_and(|t|t.native_release_valid(target));
    }
    let pointer = mobile.at.or_else(||screen.as_ref().and_then(|s| s.pointer)).filter(|_| !controller);
    let at = pointer.map(|p| (f64::from(p.x), f64::from(p.y)));
    if state.taking != Some(pip.frame) {
        *state = Feed { taking: Some(pip.frame), touch_drag: false };
    }
    let aid_press = aid.click == 1 && !aid.right;
    let held = mobile.held || aid_press || devices.mouse.pressed(MouseButton::Left);
    let mut input = fnv_ui::input::Input { at, press: mobile.press() || aid_press || devices.mouse.just_pressed(MouseButton::Left), ..Default::default() };
    let pad_menu = crate::pad::MenuAt { code: Some(fnv_game::game::menus::MAP), tab: pip.tab() };
    let physical = frame_keys(&mut input, &devices, game, pad_menu);
    let report = fnv_game::ui_input::step(game, &[menu], &input, &fnv_ui::input::no_measure);
    let ui = &game.ui_tree;
    log_report(ui, &report);
    let tab = pip.tab();
    // What each click and the pointer's move stand for, read before anything changes.
    let w = &tiles.written;
    let clicks: Vec<(DataTile, bool)> = report
        .clicks
        .iter()
        .filter(|c| ui.is_live(c.tile))
        .filter(|c| !matches!(&c.by, fnv_ui::input::By::Key(k) if !key_acts(k, &physical)))
        .filter_map(|c| data_tile(ui, w, c.tile).map(|t| (t, matches!(c.by, fnv_ui::input::By::Pointer))))
        .collect();
    let over = report.over.map(|(from, to)| (from.and_then(|t| data_tile(ui, w, t)), to.and_then(|t| data_tile(ui, w, t))));
    let hints: Vec<(Slot, ButtonKind)> = w.shown.as_ref().map(|s| s.hints.iter().map(|h| (h.slot, h.kind)).collect()).unwrap_or_default();
    if !held || (state.touch_drag && !mobile.control) {
        tiles.drag = None;
        tiles.pane_drag = None;
        state.touch_drag = false;
    }
    let mut row_acted = false;
    let mut pressed_marker = false;
    for (tile, by_pointer) in clicks {
        info!("tiles: {MAP_MENU} {tile:?} on its tiles");
        match tile {
            DataTile::Row(i) => {
                let Some(act) = row_act(&tiles.written, tab, i, RowAct::Use).filter(|_| !row_acted) else { continue };
                row_acted = true;
                data.push_act(act);
            }
            DataTile::Tab(i) => {
                out.tabs.write(PipBoyInput::Tab(i));
            }
            DataTile::Button(slot) => {
                if let Some((_, kind)) = hints.iter().find(|(s, _)| *s == slot) {
                    data.push_act(Act::Button(*kind));
                }
            }
            DataTile::ListBar(BarPiece::Marker) => {
                pressed_marker = true;
                let list_tile = resolve_tile(ui, tiles.written.list().path());
                tiles.drag = at.filter(|_| held && by_pointer).zip(list_tile).and_then(|((_, y), l)| list::marker_grab(ui, menu, l, y));
                state.touch_drag = mobile.control && tiles.drag.is_some();
            }
            DataTile::ListBar(piece) => {
                let value = tiles.written.view().value;
                if let Some(v) = resolve_tile(ui, tiles.written.list().path()).and_then(|l| list::after_piece(ui, l, piece, value)) {
                    tiles.written.view().scroll_to(v);
                }
            }
            DataTile::PaneBar(BarPiece::Marker) => {
                pressed_marker = true;
                let pane = tiles.written.shown.as_ref().map_or(&Pane::None, |s| &s.pane);
                tiles.pane_drag = at.filter(|_| held && by_pointer).and_then(|(_, y)| pane_grab(ui, menu, pane, y));
                state.touch_drag = mobile.control && tiles.pane_drag.is_some();
            }
            DataTile::PaneBar(piece) => {
                if let Some(part) = pane_part(piece) {
                    data.push_act(Act::Bar(part));
                }
            }
        }
    }
    if let (false, Some((_, y))) = (pressed_marker, at) {
        if let Some(v) = tiles.drag.and_then(|d| list::value_under(ui, d, y)) {
            tiles.written.view().scroll_to(v);
        }
        if let Some(v) = tiles.pane_drag.and_then(|d| list::value_under(ui, d, y)) {
            data.push_act(Act::PaneTo(v as f32));
        }
    }
    if let Some((was, is)) = over.filter(|_| !mobile.control) {
        let w = &tiles.written;
        // Off the selected row: no longer boxed, as the hand-built row's pointer leaving.
        if let Some(DataTile::Row(j)) = was.filter(|_| was != is) {
            if let Some(act) = row_act(w, tab, j, RowAct::Leave) {
                data.push_act(act);
            }
        }
        match is {
            Some(DataTile::Tab(i)) => {
                out.hovered.write(TabHovered(i));
            }
            // The row the pointer comes onto is selected and boxed; not one a file's own key
            // binding moved the highlight onto.
            Some(DataTile::Row(i)) if !report.over_by_key && !row_acted => {
                if let Some(act) = row_act(w, tab, i, RowAct::Hover) {
                    data.push_act(act);
                }
            }
            _ => {}
        }
    }
}

/// Whether the question box owns this frame's tile input ([`MapTiles::popup_owns`]): while it is up
/// over the page drawn from its tiles (`up`), and after, while it owned the last frame (`owned`)
/// and the left button is still held (`held`), so the press that answered it, or one begun under
/// it, never reaches a tile.
fn popup_owns(up: bool, owned: bool, held: bool) -> bool {
    up || (owned && held)
}

/// What the pointer did on a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowAct {
    Hover,
    Leave,
    Use,
}

/// The act for the row tile `i` of the list as written: by the record it stands for (its quest's,
/// note's or challenge's FormID), which the page finds in its list as that is when it takes the act
/// (`data::by_record`), never by the row's place: a script may change the list in between.
fn row_act(w: &Written, tab: usize, i: usize, what: RowAct) -> Option<Act> {
    let key = w.rows[w.list().index()].get(i)?.key;
    Some(match what {
        RowAct::Hover => Act::HoverRecord(tab, key),
        RowAct::Leave => Act::LeaveRecord(tab, key),
        RowAct::Use => Act::UseRecord(tab, key),
    })
}

/// A press at height `y` on the right pane's scroll bar's marker (the objectives' or the text's),
/// held to drag it by the file's own bar: its marker's place over its `_step_height` is the pane's
/// scroll (the bar's value is the pane's, [`write_pane`], [`write_text_pane`]) [documented:
/// scrollbar_vert.xml's `_drag_value`].
fn pane_grab(ui: &Ui, menu: TileId, pane: &Pane, y: f64) -> Option<list::Drag> {
    let owner = match pane {
        Pane::Objectives(_) => resolve_tile(ui, OBJECTIVES)?,
        Pane::Text(_) => resolve_tile(ui, TEXT_RECT)?,
        _ => return None,
    };
    list::marker_grab_on(ui, menu, owner, pane_bar(ui, pane)?, y)
}

/// The resource, with the test aid read.
pub(crate) fn setup(app: &mut App) {
    let (cut, cut_until) = std::env::var(CUT_AID).ok().map_or((None, None), |v| {
        let (name, until) = stats_menu::parse_cut(&v);
        (Some(name), until)
    });
    app.insert_resource(MapTiles { cut, cut_until, ..Default::default() });
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("map_menu/tab_swipe_tests.rs");
    use crate::pipboy::frame::Hint;
    use fnv_ui::MemSource;

    struct ImageFonts {
        data: nvf_assets::GameData,
        images: Assets<Image>,
        pictures: crate::interface::Pictures,
    }

    impl PageFonts for ImageFonts {
        fn write_sizes(&mut self, ui: &mut Ui, texts: &[TileId], written: &mut SizesWritten) -> usize {
            menus::write_text_sizes_of(ui, texts, written, &measure)
        }

        fn set_out(&mut self, ui: &Ui, menu: TileId) -> Vec<Piece> {
            super::super::draw::pieces(ui, menu, &measure, super::super::draw::Select::Whole { kept: &[], strip: None }, true).0
        }

        fn picture_texels(&mut self, name: &str) -> Option<(f64, f64)> {
            self.pictures.own_sized(name, Some(&self.data), &mut self.images, false).map(|(_, _, size)| (f64::from(size.x), f64::from(size.y)))
        }
    }

    #[test]
    fn replacement_map_icon_dimensions_reach_the_actual_crop_expressions_and_pieces() {
        use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        struct Owned(std::path::PathBuf);
        impl Drop for Owned {
            fn drop(&mut self) {
                for name in ["icon_21.dds", "icon_22.dds"] { let _ = std::fs::remove_file(self.0.join("textures").join(name)); }
                let _ = std::fs::remove_dir(self.0.join("textures"));
                let _ = std::fs::remove_dir(&self.0);
            }
        }
        let root = std::env::temp_dir().join(format!("nvf-owned-map-icon-size-{}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&root).unwrap(); std::fs::create_dir(root.join("textures")).unwrap();
        let owned = Owned(root);
        for (name, size) in [("icon_21.dds", 512), ("icon_22.dds", 1024)] {
            std::fs::write(owned.0.join("textures").join(name), crate::interface::tests::authored_rgba_dds(size, size)).unwrap();
        }
        let data = nvf_assets::GameData::new(Arc::new(fnv_vfs::Vfs::open(&owned.0, &[]).unwrap()), Arc::new(fnv_nif::Schema::default()));
        let pictures = crate::interface::Pictures::load(Some(&data));
        let mut fonts = ImageFonts { data, images: Assets::default(), pictures };
        // Independently authored ratio-driven marker: the invisible child requests the
        // own image's raw dimensions, while its parent computes zoom/crop from them.
        let marker_start = r#"<image name="MapMarker"><clips>&true;</clips>"#;
        let metadata = r#"<image name="MapMarker"><clips>&true;</clips>
          <image name="SizeSource"><filename><copy src="parent()" trait="filename"/></filename></image>
          <zoom><copy src="me()" trait="width"/><div src="child(SizeSource)" trait="filewidth"/><div>0.75</div><mul>100</mul></zoom>
          <cropx><copy src="child(SizeSource)" trait="filewidth"/><mul>0.125</mul><mul src="me()" trait="zoom"/><div>100</div></cropx>
          <cropy><copy src="child(SizeSource)" trait="fileheight"/><mul>0.125</mul><mul src="me()" trait="zoom"/><div>100</div></cropy>"#;
        let mut ui = tree_changed(|x| with_world(x).replace(marker_start, metadata));
        let mut tiles = MapTiles::default();
        let mut shown = world(1.0, [0.5, 0.5], vec![marker(21, [0.5, 0.5], false)]);
        let mut original_box = None;
        for (name, expected) in [("icon_21.dds", 512.0), ("icon_22.dds", 1024.0)] {
            shown.world.as_mut().unwrap().markers[0].icon = name.into();
            page(&mut ui, Some(shown.clone()), true, &mut tiles, &mut fonts).expect("actual map page must remain ready");
            let marker = tiles.written.world.markers[0].tile;
            let child = find_named(&ui, marker, "SizeSource").unwrap();
            assert_eq!(num(&ui, child, "filewidth"), expected, "selected image dimensions must reach the ratio child before drawing");
            assert_eq!(num(&ui, child, "fileheight"), expected);
            let piece = tiles.pieces.iter().find(|p| p.tile == marker).expect("actual marker draw piece");
            let Look::Picture { fit: Fit::Zoom(zoom), .. } = &piece.look else { panic!("ratio image must retain numeric zoom") };
            let full_width = expected * *zoom / 100.0;
            assert!((full_width - piece.rect.width / 0.75).abs() < 0.001);
            assert!((piece.crop.unwrap().0 / full_width - 0.125).abs() < 0.001, "selected high-resolution art must have its normalized left crop");
            let geometry = (piece.rect.left, piece.rect.top, piece.rect.width, piece.rect.height);
            if let Some(was) = original_box { assert_eq!(geometry, was, "a different density/file must not resize or move its marker"); } else { original_box = Some(geometry); }
        }
    }

    /// [design] A small layout written for these tests, with its own structure, numbers and
    /// expressions; it does not come from the game's files and says nothing about them. It keeps
    /// the tile, template and path names the model and the input look up, which are the engine's
    /// interface ([`required_paths`]), and only the dependencies the tests exercise: a row stands at
    /// its `_y` less its list's scroll, its text copies the row's `string`, a tab stands at its `_x`
    /// with its `string` in a text under it, a hint's text copies the hint's `string`. So these
    /// tests do not cover the file's own places or how its expressions read what is written
    /// (`_CurrentTab` and the lists' `visible`, the lists' `_enabled` and the rows' `target`,
    /// `_selected`, `_ShowEmptyMarker` and `_TextAlpha` in the row template, `_ItemType`, the hints'
    /// `_y` and dim look): the smoke scenarios `tiles-data*` check those on the real file.
    fn fixture_xml(change: impl FnOnce(String) -> String) -> String {
        let bar = r#"<image name="lb_scrollbar"><_step_size>1</_step_size><_jump_size>3</_jump_size><_current_value>0</_current_value>
              <hotrect name="scrollbar_vert_page_up"/><hotrect name="scrollbar_vert_page_down"/>
              <image name="scrollbar_vert_marker"/><image name="scrollbar_vert_up"/><image name="scrollbar_vert_down"/></image>"#;
        // The objectives' bar and the text's: its marker 16 down the bar, a value every 2 units of
        // its drag.
        // The text's bar is a hotrect, which lays out no box of its own; its marker shows while
        // the bar has more than one item.
        let text_bar = r#"<hotrect name="MM_TextScrollbar"><_current_value>0</_current_value><_ScrollDelta>7</_ScrollDelta><x>330</x><width>20</width><height>150</height>
              <hotrect name="scrollbar_vert_page_up"/><hotrect name="scrollbar_vert_page_down"/>
              <image name="scrollbar_vert_up"><width>20</width><height>10</height></image><image name="scrollbar_vert_down"/>
              <image name="scrollbar_vert_marker"><y>16</y><width>20</width><height>10</height><_step_height>2</_step_height>
                <visible><copy src="parent()" trait="_number_of_items"/><gt>1</gt></visible></image></hotrect>"#;
        let pane_bar = bar.replace(r#"<image name="scrollbar_vert_marker"/>"#, r#"<image name="scrollbar_vert_marker"><y>16</y><width>20</width><height>10</height><_step_height>2</_step_height></image>"#);
        let offset = r#"<_offset><copy src="child(lb_scrollbar)" trait="_current_value"/><mul src="me()" trait="_scroll_delta"/></_offset>"#;
        let button = |name: &str, y: u32| format!(r#"<image name="{name}"><y>{y}</y><width>150</width><height>30</height><target>&true;</target><text name="button_text"><string><copy src="parent()" trait="string"/></string></text></image>"#);
        let buttons: String = [("MM_ButtonA", 0), ("MM_ButtonX", 40), ("MM_ButtonY", 80)].iter().map(|(n, y)| button(n, *y)).collect();
        let xml = format!(
            r#"<menu name="MapMenu"><rect name="NOGLOW_BRANCH"/><rect name="GLOW_BRANCH">
            <image name="MM_Background"/>
            <rect name="MM_MainRect"><x>40</x><y>40</y>
              <rect name="MM_HeadlineRect">
                <text name="MM_Headline_Title"><string>Words</string></text>
                <rect name="MM_Headline_h2"><x>200</x><_length>300</_length></rect>
                <text name="MM_Headline_LocationInfo"><x>500</x></text>
                <text name="MM_Headline_TimeDateInfo"><x>700</x></text>
              </rect>
              <hotrect name="MM_QuestsList"><y>60</y><width>380</width><height>240</height><clipwindow>&true;</clipwindow>
                <_scroll_delta>12</_scroll_delta>{offset}
                <hotrect name="lb_highlight_box"/>{bar}
              </hotrect>
              <hotrect name="MM_NotesList"><y>60</y><width>380</width><height>240</height><clipwindow>&true;</clipwindow><visible>&true;</visible>
                <_scroll_delta>12</_scroll_delta>{offset}
                <hotrect name="lb_highlight_box"/>{bar}
              </hotrect>
              <hotrect name="MM_ChallengeList"><y>60</y><width>380</width><height>240</height><clipwindow>&true;</clipwindow><visible>&true;</visible>
                <_scroll_delta>12</_scroll_delta>{offset}
                <hotrect name="lb_highlight_box"/>{bar}
              </hotrect>
              <text name="MM_Headline_ChallengeType"></text>
              <rect name="MM_ButtonRect"><x>600</x><y>60</y>{buttons}</rect>
              <rect name="MM_DataRect"><x>450</x><y>200</y><visible>&false;</visible>
                <hotrect name="MM_QuestObjectivesList"><width>380</width><height>200</height><clipwindow>&true;</clipwindow>
                  <_scroll_delta>9</_scroll_delta>{offset}
                  <hotrect name="lb_highlight_box"/>{pane_bar}
                </hotrect>
                <hotrect name="MM_DataTextRect"><x>10</x><y>5</y><width>360</width><height>150</height><clipwindow>&true;</clipwindow>
                  {text_bar}
                  <text name="MM_DataText"><y><copy src="sibling(MM_TextScrollbar)" trait="_ScrollDelta"/><mul src="sibling(MM_TextScrollbar)" trait="_current_value"/><mul>-1</mul></y></text>
                </hotrect>
                <image name="MM_DataImage"><x>20</x><y>10</y><width>100</width><height>100</height></image>
                <rect name="MM_DataAudioInfo"><y>170</y><text name="MM_AudioTimeRemaining"><string><copy src="MM_DataRect" trait="_ItemStringInfo"/></string></text></rect>
              </rect>
            </rect>
            <rect name="MM_Tabline"><x>40</x><y>650</y><width>900</width></rect>
            </rect>
            <template name="MM_ListMarkerTemplate"><hotrect name="MM_ListItem"><target>&true;</target><x>25</x><width>330</width>
              <_y>0</_y><_VerticalSpacing>10</_VerticalSpacing>
              <y><copy src="me()" trait="_y"/><sub src="parent()" trait="_offset"/></y>
              <text name="ListItemText"><string><copy src="parent()" trait="string"/></string></text>
              <image name="MM_Template_ItemMarker"/>
            </hotrect></template>
            <template name="TabButtonTemplate"><hotrect name="TabButton"><target>&true;</target><width>100</width><height>30</height><_x>0</_x>
              <x><copy src="me()" trait="_x"/></x>
              <text name="button_text"><string><copy src="parent()" trait="string"/></string></text>
            </hotrect></template></menu>"#
        );
        change(xml)
    }

    fn tree_changed(change: impl FnOnce(String) -> String) -> Ui {
        let mut ui = Ui::new();
        ui.load_menu(&MemSource::default().with("m.xml", &fixture_xml(change)), "m.xml").unwrap();
        ui.update();
        ui
    }

    fn tree() -> Ui {
        tree_changed(|x| x)
    }

    /// A measure of 10 a character and 30 a line (`|` breaks one).
    fn measure(_: f64, text: &str, _: Option<f64>) -> (f64, f64) {
        (10.0 * text.chars().count() as f64, 30.0 * text.split('|').count() as f64)
    }

    struct TestFonts;

    impl PageFonts for TestFonts {
        fn write_sizes(&mut self, ui: &mut Ui, texts: &[TileId], written: &mut SizesWritten) -> usize {
            menus::write_text_sizes_of(ui, texts, written, &measure)
        }
        fn set_out(&mut self, ui: &Ui, menu: TileId) -> Vec<Piece> {
            super::super::draw::pieces(ui, menu, &measure, super::super::draw::Select::Whole { kept: &[], strip: None }, true).0
        }
    }

    /// Each row's height in the fixture: a line's 30 and its own spacing of 10.
    const ROW_H: f64 = 40.0;

    fn quest(i: usize, filled: bool, done: bool) -> RowShown {
        RowShown { key: 100 + i as u32, text: format!("Quest {i}"), filled, done }
    }

    fn x_hint() -> HintShown {
        HintShown { slot: Slot::X, kind: ButtonKind::ShowLocation, word: "Look There".into(), key: "[X]".into(), dim: false }
    }

    /// Quests `n` of them, the first active and the last done, `sel` selected and boxed, with `objectives`
    /// objectives (the second done), and the keyboard's one hint.
    fn shown(n: usize, sel: usize, objectives: usize) -> Shown {
        Shown {
            tab: data::QUESTS,
            device: true,
            place: "A Place".into(),
            when: "1.2.34, 5:06".into(),
            rows: (0..n).map(|i| quest(i, i == 0, n > 1 && i == n - 1)).collect(),
            selected: (n > 0).then_some(sel),
            boxed: true,
            pointer_pick: false,
            navigated: 0,
            list: ListKind::Quests,
            group: 0,
            pane: if n > 0 { Pane::Objectives((0..objectives).map(|i| RowShown { key: i as u32, text: format!("Step {i}"), filled: i == 1, done: i == 1 }).collect()) } else { Pane::None },
            pane_first: 0.0,
            hints: vec![x_hint()],
            world: None,
        }
    }

    fn run(ui: &mut Ui, s: Option<Shown>, t: &mut MapTiles) -> Result<TileId, String> {
        page(ui, s, true, t, &mut TestFonts)
    }

    fn at(ui: &Ui, path: &str, name: &str) -> f64 {
        ui.get_float(&format!("{path}/{name}")).unwrap_or(-999.0)
    }

    fn words(ui: &Ui, path: &str, name: &str) -> String {
        ui.get_string(&format!("{path}/{name}")).unwrap_or_default()
    }

    fn trait_of(ui: &Ui, t: TileId, name: &str) -> f64 {
        ui.tile(t).get(name).map_or(-999.0, Value::num)
    }

    /// The middle of a tile's text piece, as painted.
    fn middle(t: &MapTiles, tile: TileId) -> (f64, f64) {
        let p = t.pieces.iter().find(|p| p.tile == tile).unwrap();
        (p.rect.left + p.rect.width / 2.0, p.rect.top + p.rect.height / 2.0)
    }

    #[test]
    fn the_model_writes_the_heading_quests_objectives_tabs_and_hints() {
        let (mut ui, mut t) = (tree(), MapTiles::default());
        let menu = run(&mut ui, Some(shown(3, 1, 2)), &mut t).unwrap();
        assert_eq!(ui.menu(MAP_MENU), Some(menu));
        assert_eq!((words(&ui, PLACE, "string"), words(&ui, TIME, "string")), ("A Place".to_string(), "1.2.34, 5:06".to_string()));
        assert_eq!((at(&ui, BACKGROUND, "visible"), at(&ui, CHALLENGE_TYPE, "visible")), (0.0, 0.0));
        assert_eq!(at(&ui, QUESTS_LIST, "_enabled"), 1.0, "the list takes the pointer");
        // The quests, in the model's order, each its row's words, square and strength.
        let rows = t.written.rows[0].clone();
        assert_eq!(rows.iter().map(|r| r.key).collect::<Vec<_>>(), vec![100, 101, 102]);
        let text_of = |ui: &Ui, row: TileId| list::child(ui, row, ROW_TEXT).and_then(|x| ui.tile(x).get("string").map(|v| v.text().to_string()));
        assert_eq!(text_of(&ui, rows[1].tile).as_deref(), Some("Quest 1"), "the template's text copies the row's words");
        assert_eq!((trait_of(&ui, rows[0].tile, "_selected"), trait_of(&ui, rows[1].tile, "_selected")), (1.0, 0.0), "the active quest's square");
        assert_eq!((trait_of(&ui, rows[0].tile, "_TextAlpha"), trait_of(&ui, rows[2].tile, "_TextAlpha")), (FULL_ALPHA, DONE_ALPHA), "a done quest at half strength");
        assert_eq!(trait_of(&ui, rows[0].tile, "_ShowEmptyMarker"), 0.0);
        assert_eq!(ui.tile(rows[2].tile).get("_y").map(Value::num), Some(2.0 * ROW_H));
        assert_eq!((at(&ui, QUESTS_LIST, "_highlight_y"), at(&ui, QUESTS_LIST, "_selected_height")), (ROW_H, ROW_H), "the selected row boxed");
        // The objectives at the right.
        assert_eq!((at(&ui, DATA_RECT, "visible"), at(&ui, DATA_RECT, "_ItemType")), (1.0, ITEM_OBJECTIVES));
        let steps = t.written.objectives.clone();
        assert_eq!(steps.len(), 2);
        assert_eq!(text_of(&ui, steps[0].tile).as_deref(), Some("Step 0"));
        assert_eq!((trait_of(&ui, steps[0].tile, "_ShowEmptyMarker"), trait_of(&ui, steps[1].tile, "_selected"), trait_of(&ui, steps[1].tile, "_TextAlpha")), (1.0, 1.0, DONE_ALPHA));
        // The tabs: five, the hand-built page's words, the tab showing told.
        assert_eq!(t.written.tabs.len(), 5);
        for (row, word) in t.written.tabs.iter().zip(Page::Data.tabs()) {
            assert_eq!(ui.tile(row.tile).get("string").map(|v| v.text().to_string()).as_deref(), Some(word));
        }
        assert_eq!((at(&ui, TABLINE, "_ButtonCount"), at(&ui, TABLINE, "_CurrentTab")), (5.0, data::QUESTS as f64));
        // The keyboard's hints: X alone, its word, key and light.
        assert_eq!((at(&ui, &button_path("MM_ButtonA"), "visible"), at(&ui, &button_path("MM_ButtonY"), "visible")), (0.0, 0.0));
        let x = button_path("MM_ButtonX");
        assert_eq!((at(&ui, &x, "visible"), at(&ui, &x, "target")), (1.0, 1.0));
        assert_eq!((words(&ui, &x, "string"), words(&ui, &x, "_PCButtonText")), ("Look There".to_string(), "[X]".to_string()));
        // Measured and checked: three rows whole, the objectives' window from what clips them.
        assert_eq!((t.written.counted.rows, t.written.counted.rows_in_clip), (3, 3));
        let pane = t.written.counted.pane.expect("the objectives' window");
        assert_eq!((pane.min.x, pane.min.y, pane.width(), pane.height()), (490.0, 240.0, 380.0, 200.0));
        assert!(describe(&t.written).contains("tab quests, 3 rows, \"Quest 1\" selected, 2 objectives, hints: X Look There [X]"), "{}", describe(&t.written));
    }

    #[test]
    fn a_rows_tile_stays_its_quests_and_a_click_reads_where_it_stands_now() {
        let (mut ui, mut t) = (tree(), MapTiles::default());
        run(&mut ui, Some(shown(3, 0, 0)), &mut t).unwrap();
        let tile_of = |t: &MapTiles, key: u32| t.written.rows[0].iter().find(|r| r.key == key).map(|r| r.tile);
        let (first, second) = (tile_of(&t, 100).unwrap(), tile_of(&t, 101).unwrap());
        // Another order and a quest gone: the first quest's tile is kept, the gone one's unloaded.
        let mut s = shown(3, 0, 0);
        s.rows = vec![quest(2, false, false), quest(0, true, false)];
        s.selected = Some(1);
        run(&mut ui, Some(s), &mut t).unwrap();
        assert_eq!(tile_of(&t, 100), Some(first));
        assert!(!ui.is_live(second), "the gone quest's tile");
        assert_eq!(data_tile(&ui, &t.written, first), Some(DataTile::Row(1)));
        let text = list::child(&ui, first, ROW_TEXT).unwrap();
        assert_eq!(data_tile(&ui, &t.written, text), Some(DataTile::Row(1)), "inside a row is the row");
        // What a click or the pointer on that tile sends names its quest, not its place: the page
        // finds the quest in its list as that is when it takes the act (`data::by_record`).
        assert_eq!(row_act(&t.written, data::QUESTS, 1, RowAct::Use), Some(Act::UseRecord(data::QUESTS, 100)));
        assert_eq!(row_act(&t.written, data::QUESTS, 0, RowAct::Hover), Some(Act::HoverRecord(data::QUESTS, 102)));
        assert_eq!(row_act(&t.written, data::QUESTS, 1, RowAct::Leave), Some(Act::LeaveRecord(data::QUESTS, 100)));
        assert_eq!(row_act(&t.written, data::QUESTS, 2, RowAct::Use), None, "no such row");
    }

    #[test]
    fn tiles_stand_for_rows_tabs_hints_and_each_lists_bar() {
        let (mut ui, mut t) = (tree(), MapTiles::default());
        let menu = run(&mut ui, Some(shown(2, 0, 1)), &mut t).unwrap();
        let w = &t.written;
        assert_eq!(data_tile(&ui, w, w.tabs[3].tile), Some(DataTile::Tab(3)));
        let x = ui.find_tile(&button_path("MM_ButtonX")).unwrap();
        assert_eq!(data_tile(&ui, w, find_named(&ui, x, "button_text").unwrap()), Some(DataTile::Button(Slot::X)));
        let piece = |list: &str, name: &str| ui.find_tile(&format!("{list}/{}/{name}", list::SCROLLBAR)).unwrap();
        assert_eq!(data_tile(&ui, w, piece(QUESTS_LIST, list::MARKER)), Some(DataTile::ListBar(BarPiece::Marker)));
        assert_eq!(data_tile(&ui, w, piece(OBJECTIVES, list::UP)), Some(DataTile::PaneBar(BarPiece::Up)));
        assert_eq!(data_tile(&ui, w, w.objectives[0].tile), None, "the objectives take no click");
        // The pointer on a row's words and on the hint finds them, as painted.
        let row_text = list::child(&ui, w.rows[0][1].tile, ROW_TEXT).unwrap();
        let hit = fnv_ui::input::hit(&ui, &[menu], middle(&t, row_text), &fnv_ui::input::no_measure).unwrap();
        assert_eq!(data_tile(&ui, w, hit.tile), Some(DataTile::Row(1)));
        let x_text = find_named(&ui, x, "button_text").unwrap();
        let hit = fnv_ui::input::hit(&ui, &[menu], middle(&t, x_text), &fnv_ui::input::no_measure).unwrap();
        assert_eq!(data_tile(&ui, w, hit.tile), Some(DataTile::Button(Slot::X)));
    }

    #[test]
    fn the_hints_stand_on_their_tiles_by_what_they_do() {
        for kind in [ButtonKind::Travel, ButtonKind::MakeActive, ButtonKind::PlayAudio, ButtonKind::Tune] {
            assert_eq!(Slot::of(kind), Slot::A, "{kind:?}");
        }
        for kind in [ButtonKind::PlaceMarker, ButtonKind::ShowLocation, ButtonKind::NotesFilter, ButtonKind::NextChallenge] {
            assert_eq!(Slot::of(kind), Slot::X, "{kind:?}");
        }
        assert_eq!(Slot::of(ButtonKind::Challenges), Slot::Y);
        // A controller's set: A then X, whatever their order; a dim hint is no target.
        let (mut ui, mut t) = (tree(), MapTiles::default());
        let mut s = shown(2, 1, 0);
        s.hints = vec![x_hint(), HintShown { slot: Slot::A, kind: ButtonKind::MakeActive, word: "Pick".into(), key: "[A]".into(), dim: true }];
        run(&mut ui, Some(s), &mut t).unwrap();
        let a = button_path("MM_ButtonA");
        assert_eq!((at(&ui, &a, "visible"), at(&ui, &a, "target"), words(&ui, &a, "string")), (1.0, 0.0, "Pick".to_string()));
        assert_eq!(words(&ui, &button_path("MM_ButtonX"), "string"), "Look There");
    }

    #[test]
    fn the_objectives_move_by_the_panes_own_units() {
        let (mut ui, mut t) = (tree(), MapTiles::default());
        // Seven steps of 40 in a window of 200: 80 to scroll.
        let mut s = shown(1, 0, 7);
        s.pane_first = 30.0;
        run(&mut ui, Some(s.clone()), &mut t).unwrap();
        assert_eq!(t.written.pane_most, 80.0);
        assert_eq!(at(&ui, OBJECTIVES, "_scroll_delta"), 1.0);
        let bar = format!("{OBJECTIVES}/{}", list::SCROLLBAR);
        assert_eq!((at(&ui, &bar, "_current_value"), at(&ui, &bar, "_number_of_items"), at(&ui, &bar, "_number_of_visible_items")), (30.0, 81.0, 1.0));
        let second = t.written.objectives[1].tile;
        assert_eq!(trait_of(&ui, second, "y"), ROW_H - 30.0, "the rows moved by the pane's scroll");
        // Further than it can go: held at its end.
        s.pane_first = 500.0;
        run(&mut ui, Some(s), &mut t).unwrap();
        assert_eq!(at(&ui, &bar, "_current_value"), 80.0);
        assert_eq!(pane_part(BarPiece::PageDown), Some(BarPart::PageDown));
        // The marker held: the pane's scroll is where the file's bar puts it, its marker's place
        // over its step (the fixture's 16 down and 2 a value): 30 lower is (16 + 30) / 2.
        let menu = ui.menu(MAP_MENU).unwrap();
        let marker = ui.find_tile(&format!("{bar}/{}", list::MARKER)).unwrap();
        let top = super::super::layout::layout(&ui, marker, &|_, _, _| (0.0, 0.0), true).into_iter().find(|i| i.tile == marker).unwrap().rect.top;
        let drag = pane_grab(&ui, menu, &Pane::Objectives(Vec::new()), top + 3.0).expect("the marker taken");
        assert_eq!(list::value_under(&ui, drag, top + 3.0), Some(8.0), "where it stands");
        assert_eq!(list::value_under(&ui, drag, top + 33.0), Some(23.0));
        assert_eq!(list::value_under(&ui, drag, top + 500.0), Some(80.0), "held at the end");
        // A page is the window as measured: the fixture's 200.
        assert_eq!(t.written.counted.pane.map(|r| r.height()), Some(200.0));
        assert_eq!(pane_part(BarPiece::Marker), None, "the marker is the file's bar's to drag");
    }

    #[test]
    fn the_quests_list_scrolls_by_itself_and_a_key_brings_the_row_into_view() {
        let (mut ui, mut t) = (tree(), MapTiles::default());
        // Ten rows of 40 in a window of 240.
        run(&mut ui, Some(shown(10, 0, 0)), &mut t).unwrap();
        assert!(t.written.views[0].counts.most() > 0.0);
        assert_eq!(t.written.counted.rows_in_clip, 6);
        // A key picks the last row: it is brought into view.
        let mut s = shown(10, 9, 0);
        s.navigated = 1;
        run(&mut ui, Some(s.clone()), &mut t).unwrap();
        let most = t.written.views[0].counts.most();
        assert_eq!(t.written.views[0].value, most, "scrolled to the end");
        // The pointer's pick does not move it.
        assert!(t.written.views[0].scroll_to(0.0));
        s.selected = Some(8);
        s.pointer_pick = true;
        run(&mut ui, Some(s), &mut t).unwrap();
        assert_eq!(t.written.views[0].value, 0.0);
        assert_eq!(at(&ui, &format!("{QUESTS_LIST}/{}", list::SCROLLBAR), "_current_value"), 0.0);
    }

    #[test]
    fn an_unboxed_selection_shows_no_box_and_no_quest_shows_no_objectives() {
        let (mut ui, mut t) = (tree(), MapTiles::default());
        let mut s = shown(2, 1, 1);
        s.boxed = false;
        run(&mut ui, Some(s), &mut t).unwrap();
        assert_eq!((at(&ui, QUESTS_LIST, "_highlight_y"), at(&ui, QUESTS_LIST, "_selected_height")), (-1.0, 0.0));
        // No quest: an empty list is a page, no objectives, no hint.
        let mut s = shown(0, 0, 0);
        s.hints.clear();
        run(&mut ui, Some(s), &mut t).unwrap();
        assert!(t.written.rows[0].is_empty() && t.written.objectives.is_empty());
        assert_eq!(at(&ui, DATA_RECT, "visible"), 0.0);
        assert_eq!(at(&ui, &button_path("MM_ButtonX"), "visible"), 0.0);
        assert_eq!(t.written.counted.pane, None);
    }

    /// Misc with `n` rows of list `list` (the second a done one), `sel` selected and boxed, the right
    /// pane `pane` and the keyboard's notes filter and challenges toggle.
    fn misc(list: ListKind, n: usize, sel: usize, pane: Pane) -> Shown {
        let hint = |slot, kind, word: &str, key: &str| HintShown { slot, kind, word: word.into(), key: key.into(), dim: false };
        Shown {
            tab: data::NOTES,
            list,
            rows: (0..n).map(|i| RowShown { key: 500 + i as u32, text: format!("Item {i}"), filled: false, done: list == ListKind::Challenges && i == 1 }).collect(),
            selected: (n > 0).then_some(sel),
            pane,
            hints: vec![hint(Slot::X, ButtonKind::NotesFilter, "Fewer", "[X]"), hint(Slot::Y, ButtonKind::Challenges, "Feats", "[R]")],
            ..shown(0, 0, 0)
        }
    }

    /// A text of `lines` lines (the test measure breaks a line at `|`).
    fn lines(lines: usize) -> String {
        (0..lines).map(|i| format!("L{i}")).collect::<Vec<_>>().join("|")
    }

    #[test]
    fn misc_writes_the_notes_in_their_list_and_a_text_note_in_its_scrolling_pane() {
        let (mut ui, mut t) = (tree(), MapTiles::default());
        // Quests first: then Misc empties the quests list and fills the notes'.
        run(&mut ui, Some(shown(2, 0, 1)), &mut t).unwrap();
        let mut s = misc(ListKind::Notes, 3, 1, Pane::Text(lines(8)));
        s.pane_first = 30.0;
        run(&mut ui, Some(s.clone()), &mut t).unwrap();
        assert!(t.written.rows[0].is_empty() && t.written.rows[2].is_empty() && t.written.objectives.is_empty());
        let rows = t.written.rows[1].clone();
        assert_eq!(rows.iter().map(|r| r.key).collect::<Vec<_>>(), vec![500, 501, 502]);
        assert_eq!(rows.iter().map(|r| ui.tile(r.tile).parent).collect::<Vec<_>>(), vec![ui.find_tile(NOTES_LIST); 3], "under the notes list");
        assert_eq!((trait_of(&ui, rows[1].tile, "_selected"), trait_of(&ui, rows[1].tile, "_TextAlpha")), (0.0, FULL_ALPHA), "a note has no square and full strength");
        assert_eq!((at(&ui, NOTES_LIST, "_enabled"), at(&ui, QUESTS_LIST, "_enabled"), at(&ui, CHALLENGE_LIST, "_enabled")), (1.0, 0.0, 0.0), "only the list shown takes the pointer");
        assert_eq!((at(&ui, NOTES_LIST, "visible"), at(&ui, CHALLENGE_LIST, "visible")), (1.0, 0.0), "the challenges hidden while the notes show");
        assert_eq!((at(&ui, NOTES_LIST, "_highlight_y"), at(&ui, NOTES_LIST, "_selected_height")), (ROW_H, ROW_H), "the selected note boxed");
        // The text: its pane shown, its words, scrolled by the pane's own units.
        assert_eq!((at(&ui, DATA_RECT, "visible"), at(&ui, DATA_RECT, "_ItemType")), (1.0, ITEM_TEXT));
        assert_eq!(words(&ui, DATA_TEXT, "string"), lines(8));
        // Eight lines of 30 in a window of 150: 90 to scroll, 30 of it scrolled.
        assert_eq!(t.written.pane_most, 90.0);
        assert_eq!((at(&ui, TEXT_BAR, "_ScrollDelta"), at(&ui, TEXT_BAR, "_number_of_items"), at(&ui, TEXT_BAR, "_number_of_visible_items")), (1.0, 91.0, 1.0));
        assert_eq!((at(&ui, TEXT_BAR, "_current_value"), at(&ui, DATA_TEXT, "y")), (30.0, -30.0));
        // The window as measured: what clips the text (the main rect's 40, the data rect's 450 and
        // 200, the text's own 10 and 5).
        let pane = t.written.counted.pane.expect("the text's window");
        assert_eq!((pane.min.x, pane.min.y, pane.width(), pane.height()), (500.0, 245.0, 360.0, 150.0));
        // The hints: the filter on X, the challenges toggle on Y.
        assert_eq!((words(&ui, &button_path("MM_ButtonX"), "string"), words(&ui, &button_path("MM_ButtonY"), "string")), ("Fewer".to_string(), "Feats".to_string()));
        assert_eq!(words(&ui, &button_path("MM_ButtonY"), "_PCButtonText"), "[R]");
        assert!(describe(&t.written).contains("tab misc notes, 3 rows, \"Item 1\" selected, text, hints: X Fewer [X], Y Feats [R]"), "{}", describe(&t.written));
        // Further than it can go: held at its end; a page and the wheel are the pane's (data).
        s.pane_first = 500.0;
        run(&mut ui, Some(s), &mut t).unwrap();
        assert_eq!(at(&ui, TEXT_BAR, "_current_value"), 90.0);
    }

    #[test]
    fn the_challenges_take_misc_s_list_and_its_rows_stand_for_their_records() {
        let (mut ui, mut t) = (tree(), MapTiles::default());
        run(&mut ui, Some(misc(ListKind::Notes, 2, 0, Pane::Text("A".into()))), &mut t).unwrap();
        let note = t.written.rows[1][0].tile;
        run(&mut ui, Some(misc(ListKind::Challenges, 3, 2, Pane::Text("B|C".into()))), &mut t).unwrap();
        assert!(!ui.is_live(note) && t.written.rows[1].is_empty(), "the notes' rows go with the toggle");
        let rows = t.written.rows[2].clone();
        assert_eq!(rows.len(), 3);
        assert_eq!(trait_of(&ui, rows[1].tile, "_TextAlpha"), DONE_ALPHA, "a challenge completed at half strength");
        assert_eq!((at(&ui, CHALLENGE_LIST, "visible"), at(&ui, NOTES_LIST, "visible"), at(&ui, CHALLENGE_LIST, "_enabled")), (1.0, 0.0, 1.0));
        assert_eq!(data_tile(&ui, &t.written, rows[2].tile), Some(DataTile::Row(2)));
        assert_eq!(row_act(&t.written, data::NOTES, 2, RowAct::Use), Some(Act::UseRecord(data::NOTES, 502)));
        let bar_up = ui.find_tile(&format!("{CHALLENGE_LIST}/{}/{}", list::SCROLLBAR, list::UP)).unwrap();
        assert_eq!(data_tile(&ui, &t.written, bar_up), Some(DataTile::ListBar(BarPiece::Up)), "the list shown's own bar");
        let notes_up = ui.find_tile(&format!("{NOTES_LIST}/{}/{}", list::SCROLLBAR, list::UP)).unwrap();
        assert_eq!(data_tile(&ui, &t.written, notes_up), None, "not the hidden list's");
        assert_eq!(t.written.counted.rows_in_clip, 3);
        assert!(describe(&t.written).contains("tab misc challenges, 3 rows, \"Item 2\" selected, text"), "{}", describe(&t.written));
        // Each list keeps its own scroll.
        t.written.views[1].value = 4.0;
        run(&mut ui, Some(misc(ListKind::Challenges, 3, 2, Pane::Text("B|C".into()))), &mut t).unwrap();
        assert_eq!(t.written.views[2].value, 0.0);
    }

    #[test]
    fn the_texts_bar_is_the_right_panes_and_its_marker_drags_by_the_files_bar() {
        let (mut ui, mut t) = (tree(), MapTiles::default());
        let menu = run(&mut ui, Some(misc(ListKind::Notes, 1, 0, Pane::Text(lines(20)))), &mut t).unwrap();
        // Twenty lines of 30 in 150: 450 to scroll.
        assert_eq!(t.written.pane_most, 450.0);
        let piece = |name: &str| ui.find_tile(&format!("{TEXT_BAR}/{name}")).unwrap();
        assert_eq!(data_tile(&ui, &t.written, piece(list::UP)), Some(DataTile::PaneBar(BarPiece::Up)));
        assert_eq!(data_tile(&ui, &t.written, piece(list::PAGE_DOWN)), Some(DataTile::PaneBar(BarPiece::PageDown)));
        assert_eq!(data_tile(&ui, &t.written, ui.find_tile(DATA_TEXT).unwrap()), None, "the text itself takes no click");
        let marker = piece(list::MARKER);
        let top = super::super::layout::layout(&ui, marker, &|_, _, _| (0.0, 0.0), true).into_iter().find(|i| i.tile == marker).unwrap().rect.top;
        let pane = t.written.shown.as_ref().unwrap().pane.clone();
        let drag = pane_grab(&ui, menu, &pane, top + 3.0).expect("the marker taken");
        // 16 down and 2 a value: 40 lower is (16 + 40) / 2.
        assert_eq!(list::value_under(&ui, drag, top + 3.0), Some(8.0));
        assert_eq!(list::value_under(&ui, drag, top + 43.0), Some(28.0));
        assert_eq!(pane_grab(&ui, menu, &Pane::None, top), None, "no pane, no drag");
    }

    #[test]
    fn a_picture_and_an_audio_note_show_in_their_parts_of_the_pane() {
        let (mut ui, mut t) = (tree(), MapTiles::default());
        run(&mut ui, Some(misc(ListKind::Notes, 2, 0, Pane::Image("pictures\\test.dds".into()))), &mut t).unwrap();
        assert_eq!((at(&ui, DATA_RECT, "visible"), at(&ui, DATA_RECT, "_ItemType")), (1.0, ITEM_IMAGE));
        assert_eq!((words(&ui, DATA_IMAGE, "filename"), words(&ui, DATA_TEXT, "string")), ("pictures\\test.dds".to_string(), String::new()));
        assert_eq!(t.written.counted.pane, None, "a picture does not scroll");
        assert!(describe(&t.written).contains(", picture, "));
        // An audio note 30.9 s from its end, a quarter played.
        run(&mut ui, Some(misc(ListKind::Notes, 2, 1, Pane::Audio { left: 309, played: 250 })), &mut t).unwrap();
        assert_eq!(at(&ui, DATA_RECT, "_ItemType"), ITEM_AUDIO);
        assert_eq!((words(&ui, DATA_RECT, "_ItemStringInfo"), at(&ui, DATA_RECT, "_AudioFileCompletion")), (data::remaining(309), 0.25));
        assert_eq!(words(&ui, AUDIO_TIME, "string"), data::remaining(309), "the time's text reads it");
        assert_eq!(words(&ui, DATA_IMAGE, "filename"), "", "the picture let go");
        // No note: no pane.
        let mut s = misc(ListKind::Notes, 0, 0, Pane::None);
        s.hints.truncate(1);
        run(&mut ui, Some(s), &mut t).unwrap();
        assert_eq!(at(&ui, DATA_RECT, "visible"), 0.0);
        assert!(describe(&t.written).contains("tab misc notes, 0 rows, \"none\" selected, no pane"), "{}", describe(&t.written));
    }

    /// The smoke run's case: a text that fits its window (a challenge's) shows no scroll bar, the
    /// file's bar and rect being hotrects that lay out no box: the window is what clips the text,
    /// and only a text that scrolls asks for its bar's marker.
    #[test]
    fn a_short_text_needs_no_scroll_bar_and_a_long_one_shows_its_marker() {
        let (mut ui, mut t) = (tree(), MapTiles::default());
        run(&mut ui, Some(misc(ListKind::Challenges, 2, 0, Pane::Text("Short|text".into()))), &mut t).unwrap();
        assert_eq!(t.written.pane_most, 0.0, "it fits");
        let marker = ui.find_tile(&format!("{TEXT_BAR}/{}", list::MARKER)).unwrap();
        assert_eq!(trait_of(&ui, marker, "visible"), 0.0, "no bar shows");
        let pane = t.written.counted.pane.expect("the text's window, from what clips it");
        assert_eq!((pane.min.x, pane.min.y, pane.width(), pane.height()), (500.0, 245.0, 360.0, 150.0));
        // A long one scrolls: its marker shows.
        assert!(run(&mut ui, Some(misc(ListKind::Challenges, 2, 0, Pane::Text(lines(9)))), &mut t).is_ok());
        assert_eq!(trait_of(&ui, marker, "visible"), 1.0);
        // A long one whose marker the file hides falls back; the same file shows a short one.
        let hidden = |x: String| x.replace("<visible><copy src=\"parent()\" trait=\"_number_of_items\"/><gt>1</gt></visible>", "<visible>&false;</visible>");
        let mut ui = tree_changed(hidden);
        let mut t = MapTiles::default();
        assert_eq!(run(&mut ui, Some(misc(ListKind::Notes, 1, 0, Pane::Text(lines(9)))), &mut t), Err("the text scrolls and its scroll bar is not shown".to_string()));
        assert!(run(&mut ui, Some(misc(ListKind::Notes, 1, 0, Pane::Text("A".into()))), &mut t).is_ok());
        // An empty text asks for nothing.
        assert!(run(&mut ui, Some(misc(ListKind::Notes, 1, 0, Pane::Text(String::new()))), &mut MapTiles::default()).is_ok());
    }

    /// The review's case: the notes scrolled down with their first row selected, the challenges
    /// toggled on and off: the model shows the first note again, so the notes start at their top,
    /// not where they were scrolled (with that row out of sight). Coming back from Quests to the
    /// same list keeps its scroll.
    #[test]
    fn a_toggle_starts_the_list_it_shows_at_its_top() {
        let (mut ui, mut t) = (tree(), MapTiles::default());
        let mut notes = misc(ListKind::Notes, 10, 0, Pane::None);
        notes.pointer_pick = true;
        run(&mut ui, Some(notes.clone()), &mut t).unwrap();
        assert!(t.written.views[1].counts.most() >= 3.0, "ten rows overflow the window");
        assert!(t.written.views[1].scroll_to(3.0));
        run(&mut ui, None, &mut t).unwrap();
        assert_eq!(t.written.views[1].value, 3.0, "the bar's scroll holds while nothing changes");
        // Quests and back: the same list, its scroll kept.
        run(&mut ui, Some(shown(2, 0, 0)), &mut t).unwrap();
        run(&mut ui, Some(notes.clone()), &mut t).unwrap();
        assert_eq!(t.written.views[1].value, 3.0, "back from another tab, the notes as they were");
        // The challenges and back: the notes at their top.
        run(&mut ui, Some(misc(ListKind::Challenges, 2, 0, Pane::None)), &mut t).unwrap();
        run(&mut ui, Some(notes), &mut t).unwrap();
        assert_eq!(t.written.views[1].value, 0.0, "the first note in sight again");
        assert_eq!(at(&ui, &format!("{NOTES_LIST}/{}", list::SCROLLBAR), "_current_value"), 0.0);
    }

    /// The review's case: a clip above the text smaller than its rect. The text scrolls by what is
    /// seen of it (so its end can be reached) and asks for its bar by that; a picture cut by such a
    /// clip is not painted whole.
    #[test]
    fn the_text_scrolls_by_its_effective_window_and_a_clipped_picture_falls_back() {
        // The data rect clips its children to 95 high: the text's 150 (5 down) is seen 90 high.
        let clipped = |x: String| x.replace(r#"<rect name="MM_DataRect"><x>450</x><y>200</y>"#, r#"<rect name="MM_DataRect"><x>450</x><y>200</y><width>400</width><height>95</height><clips>&true;</clips>"#);
        let (mut ui, mut t) = (tree_changed(clipped), MapTiles::default());
        // Four lines of 30 fit the rect's 150 but not the 90 seen: 30 to scroll, and the bar shows.
        run(&mut ui, Some(misc(ListKind::Notes, 1, 0, Pane::Text(lines(4)))), &mut t).unwrap();
        assert_eq!(t.written.pane_most, 30.0);
        assert_eq!(at(&ui, TEXT_BAR, "_number_of_items"), 31.0);
        let pane = t.written.counted.pane.expect("the window seen");
        assert_eq!((pane.min.y, pane.height()), (245.0, 90.0));
        let mut s = misc(ListKind::Notes, 1, 0, Pane::Text(lines(4)));
        s.pane_first = 30.0;
        run(&mut ui, Some(s), &mut t).unwrap();
        assert_eq!(at(&ui, DATA_TEXT, "y"), -30.0, "its end reached");
        // The picture, 100 high at 10 down, cut at 95: not whole.
        assert_eq!(run(&mut ui, Some(misc(ListKind::Notes, 1, 0, Pane::Image("a.dds".into()))), &mut t), Err("the picture is not shown".to_string()));
        // Wholly outside the clip, it is still a piece on the canvas: not shown either.
        let away = |x: String| clipped(x).replace(r#"<image name="MM_DataImage"><x>20</x><y>10</y>"#, r#"<image name="MM_DataImage"><x>20</x><y>200</y>"#);
        let mut ui = tree_changed(away);
        assert_eq!(run(&mut ui, Some(misc(ListKind::Notes, 1, 0, Pane::Image("a.dds".into()))), &mut MapTiles::default()), Err("the picture is not shown".to_string()));
    }

    #[test]
    fn misc_falls_back_when_its_pane_does_not_show_and_its_tiles_are_only_misc_s() {
        // The text's window off the canvas, or its words outside it.
        let off = |x: String| x.replace(r#"<hotrect name="MM_DataTextRect"><x>10</x><y>5</y>"#, r#"<hotrect name="MM_DataTextRect"><x>10</x><y>900</y>"#);
        let mut ui = tree_changed(off);
        assert_eq!(run(&mut ui, Some(misc(ListKind::Notes, 1, 0, Pane::Text("A".into()))), &mut MapTiles::default()), Err("the text's window does not fit".to_string()));
        let (mut ui, mut t) = (tree(), MapTiles::default());
        assert!(run(&mut ui, Some(misc(ListKind::Notes, 1, 0, Pane::Text("A".into()))), &mut t).is_ok());
        ui.set_trait(ui.find_tile(DATA_TEXT).unwrap(), "x", Value::Num(2000.0));
        ui.update();
        assert_eq!(run(&mut ui, None, &mut t), Err("the text does not show in its window".to_string()));
        assert!(t.written.layout_failure);
        // The picture or the audio note's time hidden.
        let hide = |name: &'static str| move |x: String| x.replace(&format!(r#"name="{name}">"#), &format!(r#"name="{name}"><visible>&false;</visible>"#));
        let mut ui = tree_changed(hide("MM_DataImage"));
        assert_eq!(run(&mut ui, Some(misc(ListKind::Notes, 1, 0, Pane::Image("a.dds".into()))), &mut MapTiles::default()), Err("the picture is not shown".to_string()));
        let mut ui = tree_changed(hide("MM_AudioTimeRemaining"));
        assert_eq!(run(&mut ui, Some(misc(ListKind::Notes, 1, 0, Pane::Audio { left: 5, played: 0 })), &mut MapTiles::default()), Err("the audio note's time is not shown".to_string()));
        // A file without Misc's text: Misc falls back, Quests is still drawn from it.
        let mut ui = tree_changed(|x| x.replace("MM_DataTextRect", "SomethingElse"));
        let err = run(&mut ui, Some(misc(ListKind::Notes, 1, 0, Pane::None)), &mut MapTiles::default()).unwrap_err();
        assert!(err.contains("MM_DataTextRect missing"), "{err}");
        assert!(run(&mut ui, Some(shown(2, 0, 1)), &mut MapTiles::default()).is_ok(), "Quests needs none of Misc's tiles");
        // The test aid's cut is Misc's alone too.
        let (mut ui, mut t) = (tree(), MapTiles { cut: Some("MM_NotesList".into()), ..Default::default() });
        assert!(run(&mut ui, Some(shown(1, 0, 0)), &mut t).is_ok());
        let err = run(&mut ui, Some(misc(ListKind::Notes, 1, 0, Pane::None)), &mut t).unwrap_err();
        assert!(err.contains("MM_NotesList missing"), "{err}");
    }

    #[test]
    fn the_prepared_misc_page_is_read_by_its_list_and_pane() {
        let row = |text: &str| data::Row { text: text.into(), mark: Mark::None, done: false };
        let view = |right| data::View {
            tab: data::NOTES,
            place: String::new(),
            when: String::new(),
            buttons: Vec::new(),
            body: Body::List { rows: vec![row("N1")], first: 0, boxed: None, right },
            popup: None,
        };
        let p = |right, challenges, filtered| Prepared { view: view(right), keys: vec![3], sel: 0, pointer_pick: false, navigated: 0, challenges, filtered };
        let s = Shown::of(&p(Right::Text { text: "a\r\nb".into(), first: 4.0 }, false, true), true).unwrap();
        assert_eq!((s.list, s.group, s.pane.clone(), s.pane_first), (ListKind::Notes, 1, Pane::Text("a\nb".into()), 4.0));
        let s = Shown::of(&p(Right::Text { text: "c".into(), first: 0.0 }, true, true), true).unwrap();
        assert_eq!((s.list, s.group), (ListKind::Challenges, 0), "the filter is the notes'");
        assert_eq!(Shown::of(&p(Right::Image("i.dds".into()), false, false), true).map(|s| s.pane), Ok(Pane::Image("i.dds".into())));
        assert_eq!(Shown::of(&p(Right::Audio { left: 7, played: 9 }, false, false), true).map(|s| s.pane), Ok(Pane::Audio { left: 7, played: 9 }));
        assert_eq!(Shown::of(&p(Right::Objectives { rows: Vec::new(), first: 0.0 }, false, false), true), Err("the right pane is not the tab's".to_string()));
        // Radio: its stations' list and the waveform's panel; Misc's panes are not Radio's.
        let radio = Prepared { view: data::View { tab: data::RADIO, body: Body::List { rows: vec![row("S")], first: 0, boxed: None, right: Right::Wave { on: true } }, ..view(Right::None) }, ..p(Right::None, false, false) };
        assert_eq!(Shown::of(&radio, true).map(|s| (s.list, s.pane)), Ok((ListKind::Stations, Pane::Wave { on: true })));
        let radio_text = Prepared { view: data::View { tab: data::RADIO, body: Body::List { rows: vec![row("S")], first: 0, boxed: None, right: Right::Text { text: "x".into(), first: 0.0 } }, ..view(Right::None) }, ..p(Right::None, false, false) };
        assert_eq!(Shown::of(&radio_text, true), Err("the right pane is not the tab's".to_string()));
        // The waveform is Radio's only.
        assert_eq!(Shown::of(&p(Right::Wave { on: false }, false, false), true), Err("the right pane is not the tab's".to_string()));
    }

    /// The fixture with Radio's parts, written for these tests: a station list as the other lists
    /// and a waveform panel at 500, 125, 350 square under the main rect, with two meters, a centre
    /// line and an animated image the file shows (which the page hides).
    fn with_radio(x: String) -> String {
        let bar = r#"<image name="lb_scrollbar"><_step_size>1</_step_size><_jump_size>3</_jump_size><_current_value>0</_current_value>
              <hotrect name="scrollbar_vert_page_up"/><hotrect name="scrollbar_vert_page_down"/>
              <image name="scrollbar_vert_marker"/><image name="scrollbar_vert_up"/><image name="scrollbar_vert_down"/></image>"#;
        let offset = r#"<_offset><copy src="child(lb_scrollbar)" trait="_current_value"/><mul src="me()" trait="_scroll_delta"/></_offset>"#;
        let radio = format!(
            r#"<hotrect name="MM_RadioStationList"><y>60</y><width>380</width><height>240</height><clipwindow>&true;</clipwindow><visible>&true;</visible>
                <_scroll_delta>12</_scroll_delta>{offset}
                <hotrect name="lb_highlight_box"/>{bar}
              </hotrect>
              <rect name="MM_WaveformRect"><x>500</x><y>125</y><width>350</width><height>350</height>
                <image name="MM_WaveformMeterHoriz"><filename>h.dds</filename><y>320</y><width>350</width><height>32</height></image>
                <image name="MM_WaveformMeterVert"><filename>v.dds</filename><x>320</x><width>32</width><height>350</height></image>
                <image name="MM_WaveformCenterLine"><filename>line.dds</filename><y>166</y><width>345</width><height>2</height></image>
                <image name="MM_Waveform"><filename>wave.dds</filename><visible>&true;</visible><width>350</width><height>350</height></image>
              </rect>"#
        );
        x.replace(r#"<text name="MM_Headline_ChallengeType"></text>"#, &format!(r#"<text name="MM_Headline_ChallengeType"></text>{radio}"#))
    }

    /// Radio with `n` stations, `sel` selected, `playing` the one playing (its square filled) and
    /// the waveform on while one plays; the keyboard has no hint on Radio.
    fn radio(n: usize, sel: usize, playing: Option<usize>) -> Shown {
        Shown {
            tab: data::RADIO,
            list: ListKind::Stations,
            rows: (0..n).map(|i| RowShown { key: 700 + i as u32, text: format!("Station {i}"), filled: playing == Some(i), done: false }).collect(),
            selected: (n > 0).then_some(sel),
            pane: Pane::Wave { on: playing.is_some() },
            hints: Vec::new(),
            ..shown(0, 0, 0)
        }
    }

    const STATION_PATH: &str = main_rect!("MM_RadioStationList");

    #[test]
    fn radio_writes_its_stations_and_shows_the_files_waveform_panel() {
        let (mut ui, mut t) = (tree_changed(with_radio), MapTiles::default());
        // Quests first: Radio then empties the quests list and fills the stations'.
        run(&mut ui, Some(shown(2, 0, 1)), &mut t).unwrap();
        run(&mut ui, Some(radio(3, 1, Some(2))), &mut t).unwrap();
        assert!(t.written.rows[0].is_empty() && t.written.objectives.is_empty());
        let rows = t.written.rows[3].clone();
        assert_eq!(rows.iter().map(|r| r.key).collect::<Vec<_>>(), vec![700, 701, 702]);
        assert_eq!(rows.iter().map(|r| ui.tile(r.tile).parent).collect::<Vec<_>>(), vec![ui.find_tile(STATION_PATH); 3], "under the stations list");
        assert_eq!((trait_of(&ui, rows[2].tile, "_selected"), trait_of(&ui, rows[0].tile, "_selected")), (1.0, 0.0), "the playing station's square filled");
        assert_eq!((at(&ui, STATION_PATH, "_enabled"), at(&ui, QUESTS_LIST, "_enabled")), (1.0, 0.0), "the stations take the pointer");
        assert_eq!((at(&ui, STATION_PATH, "_highlight_y"), at(&ui, STATION_PATH, "_selected_height")), (ROW_H, ROW_H), "the selected station boxed");
        // The panel shown, its animated image hidden, the data rect's parts not shown.
        assert_eq!((at(&ui, WAVE_RECT, "visible"), at(&ui, WAVE_IMAGE, "visible")), (1.0, 0.0));
        assert_eq!((at(&ui, DATA_RECT, "visible"), at(&ui, DATA_RECT, "_ItemType")), (0.0, 0.0));
        assert_eq!(t.written.counted.pane, None, "nothing scrolls on Radio");
        // The box the trace is painted in: the main rect's 40 and the panel's 500, 125.
        let wave = wave_of(&t.written).expect("the panel's box");
        assert_eq!((wave.rect.min.x, wave.rect.min.y, wave.rect.width(), wave.rect.height(), wave.on), (540.0, 165.0, 350.0, 350.0, true));
        assert_eq!((t.written.counted.rows, t.written.counted.rows_in_clip), (3, 3));
        assert!(describe(&t.written).contains("tab radio stations, 3 rows, \"Station 1\" selected, waveform on, hints: none"), "{}", describe(&t.written));
        // A row stands for its station, by the station's record, not its place.
        assert_eq!(data_tile(&ui, &t.written, rows[1].tile), Some(DataTile::Row(1)));
        assert_eq!(row_act(&t.written, data::RADIO, 2, RowAct::Use), Some(Act::UseRecord(data::RADIO, 702)));
        let bar_up = ui.find_tile(&format!("{STATION_PATH}/{}/{}", list::SCROLLBAR, list::UP)).unwrap();
        assert_eq!(data_tile(&ui, &t.written, bar_up), Some(DataTile::ListBar(BarPiece::Up)), "the stations' own bar");
        // Back on Quests the panel hides again and the trace has no box.
        run(&mut ui, Some(shown(2, 0, 1)), &mut t).unwrap();
        assert_eq!(at(&ui, WAVE_RECT, "visible"), 0.0);
        assert_eq!(wave_of(&t.written), None);
    }

    #[test]
    fn another_station_or_the_radio_off_is_written_again() {
        let (mut ui, mut t) = (tree_changed(with_radio), MapTiles::default());
        run(&mut ui, Some(radio(3, 0, Some(0))), &mut t).unwrap();
        let tiles: Vec<TileId> = t.written.rows[3].iter().map(|r| r.tile).collect();
        // Tuned to the third: the same tiles, the third's square filled.
        run(&mut ui, Some(radio(3, 2, Some(2))), &mut t).unwrap();
        assert_eq!(t.written.rows[3].iter().map(|r| r.tile).collect::<Vec<_>>(), tiles, "the rows outlast the tuning");
        assert_eq!((trait_of(&ui, tiles[0], "_selected"), trait_of(&ui, tiles[2], "_selected")), (0.0, 1.0));
        // Off: no square filled, the panel still shown, the trace off.
        run(&mut ui, Some(radio(3, 2, None)), &mut t).unwrap();
        assert!(tiles.iter().all(|&r| trait_of(&ui, r, "_selected") == 0.0));
        assert_eq!(at(&ui, WAVE_RECT, "visible"), 1.0);
        assert_eq!(wave_of(&t.written).map(|w| w.on), Some(false));
        assert!(describe(&t.written).contains("waveform off"), "{}", describe(&t.written));
        // No station received: an empty list is a page, the panel still shown.
        run(&mut ui, Some(radio(0, 0, None)), &mut t).unwrap();
        assert!(t.written.rows[3].is_empty());
        assert!(wave_of(&t.written).is_some());
    }

    #[test]
    fn radio_falls_back_without_its_panel_and_its_tiles_are_only_radio_s() {
        // A panel moved off the page: by hand.
        let off = |x: String| with_radio(x).replace(r#"<rect name="MM_WaveformRect"><x>500</x><y>125</y>"#, r#"<rect name="MM_WaveformRect"><x>500</x><y>700</y>"#);
        let mut ui = tree_changed(off);
        let mut t = MapTiles::default();
        assert_eq!(run(&mut ui, Some(radio(2, 0, Some(0))), &mut t), Err("the waveform panel does not fit".to_string()));
        assert!(t.written.layout_failure, "a layout's failure");
        // A file without the panel: Radio falls back, Quests and Misc are still drawn from it.
        let mut ui = tree_changed(|x| with_radio(x).replace("MM_WaveformRect", "SomethingElse"));
        let err = run(&mut ui, Some(radio(2, 0, None)), &mut MapTiles::default()).unwrap_err();
        assert!(err.contains("MM_WaveformRect missing"), "{err}");
        assert!(run(&mut ui, Some(shown(2, 0, 1)), &mut MapTiles::default()).is_ok(), "Quests needs none of Radio's tiles");
        // The base fixture has no station list: Radio alone falls back.
        let err = run(&mut tree(), Some(radio(2, 0, None)), &mut MapTiles::default()).unwrap_err();
        assert!(err.contains("MM_RadioStationList missing"), "{err}");
    }

    /// The fixture with the World Map's parts, written for these tests with their own numbers and
    /// expressions (none of them the file's): a map window 50 down under the main rect, 855 × 500,
    /// the picture's hotrect in it sized by its `filewidth` and magnification and placed by its
    /// drag, the arrow placed on it by its texture coordinates, a highlight box in a window of its
    /// own at the same place, and the four templates, each row placed at its `_x`/`_y` times its
    /// parent's size (no centring), a line as long as its `user0`.
    fn with_world(x: String) -> String {
        let world = r#"<hotrect name="MM_WorldMap_ClipWindow"><y>50</y><width>855</width><height>500</height><clipwindow>&true;</clipwindow>
                <hotrect name="MM_WorldMap_ParentImage"><clips>&true;</clips><alpha>128</alpha><filename>fixture_map.dds</filename><filewidth>100</filewidth><fileheight>100</fileheight><_Magnification>1</_Magnification>
                  <width><copy src="me()" trait="filewidth"/><mul src="me()" trait="_Magnification"/></width>
                  <height><copy src="me()" trait="fileheight"/><mul src="me()" trait="_Magnification"/></height>
                  <x><copy src="me()" trait="dragdeltax"/></x><y><copy src="me()" trait="dragdeltay"/></y>
                </hotrect>
                <image name="MM_WorldMapCursor"><filename>arrow.dds</filename><clips>&true;</clips><height>20</height>
                  <width><copy src="sibling(MM_WorldMap_ParentImage)" trait="user2"/></width>
                  <x><copy src="sibling(MM_WorldMap_ParentImage)" trait="width"/><mul src="me()" trait="_x"/><add src="sibling(MM_WorldMap_ParentImage)" trait="x"/></x>
                  <y><copy src="sibling(MM_WorldMap_ParentImage)" trait="height"/><mul src="me()" trait="_y"/><add src="sibling(MM_WorldMap_ParentImage)" trait="y"/></y>
                </image>
              </hotrect>
              <hotrect name="MM_Highlight_ClipWindow"><y>50</y><width>855</width><height>500</height><clipwindow>&true;</clipwindow>
                <rect name="MM_MapHighlightBox"><width>160</width><height>160</height><visible>&false;</visible>
                  <text name="MM_HighlightLocationName"><x>80</x><y>170</y><string><copy src="parent()" trait="_Title"/></string></text>
                  <image name="MM_Highlight_CenterDot"><filename>dot.dds</filename><x>76</x><y>76</y><width>8</width><height>8</height></image>
                </rect>
              </hotrect>"#;
        let place = r#"<x><copy src="parent()" trait="width"/><mul src="me()" trait="_x"/></x><y><copy src="parent()" trait="height"/><mul src="me()" trait="_y"/></y>"#;
        let templates = format!(
            r#"<template name="MapMarkerTemplate"><image name="MapMarker"><clips>&true;</clips><width><copy src="parent()" trait="user0"/></width><height>10</height>{place}</image></template>
            <template name="WorldMapQuestMarkerTemplate"><image name="WorldMapQuestMarker"><clips>&true;</clips><width><copy src="parent()" trait="user1"/></width><height>12</height>{place}</image></template>
            <template name="WaypointTemplate"><image name="WaypointIcon"><filename>node.dds</filename><clips>&true;</clips><width>6</width><height>6</height>{place}</image></template>
            <template name="PathLineTemplate"><image name="PathLine"><filename>path.dds</filename><clips>&true;</clips><height>2</height><width><copy src="me()" trait="user0"/></width></image></template>"#
        );
        x.replace(r#"<text name="MM_Headline_ChallengeType"></text>"#, &format!(r#"<text name="MM_Headline_ChallengeType"></text>{world}"#)).replace("</menu>", &format!("{templates}</menu>"))
    }

    const WORLD_W: Rect = Rect { min: Vec2::new(40.0, 90.0), max: Vec2::new(895.0, 590.0) };

    fn marker(form: u32, uv: [f32; 2], selected: bool) -> MarkerShown {
        MarkerShown { form, uv, icon: format!("icon_{form}.dds"), selected, name: format!("Place {form}"), detail: (form == 21).then(|| "Town 21: Neutral".to_string()) }
    }

    /// The World Map at magnification `mag` centred on `centre` in the fixture's window (its
    /// picture 1000 across at 1), with `markers`, one quest target (form 77), the custom marker,
    /// the arrow and both lines, the highlight box on the selected marker, and Place Marker's hint.
    fn world(mag: f32, centre: [f32; 2], markers: Vec<MarkerShown>) -> Shown {
        let view = data::MapView::world(WORLD_W, centre, Vec2::splat(1000.0), mag);
        let on = markers.iter().find(|m| m.selected).map(|m| view.to_window(m.uv));
        let label = markers.iter().find(|m| m.selected).map(|m| m.name.clone());
        Shown {
            tab: data::WORLD,
            rows: Vec::new(),
            selected: None,
            pane: Pane::None,
            hints: vec![HintShown { slot: Slot::X, kind: ButtonKind::PlaceMarker, word: "Put Marker".into(), key: String::new(), dim: false }],
            world: Some(WorldShown {
                local: false,
                outline: None,
                view,
                texture: "map.dds".into(),
                picture: view.picture(),
                window: WORLD_W,
                mag,
                usable: Vec2::splat(1000.0),
                markers,
                quests: vec![(77, [0.6, 0.5])],
                custom: Some([0.4, 0.5]),
                arrow: Some(([0.5, 0.5], 0.25)),
                ends: vec![(data::marker::LineTo::Marker, 0, [0.4, 0.5]), (data::marker::LineTo::Quest, 77, [0.6, 0.5])],
                cursor: on.unwrap_or(view.mid()),
                want: on.unwrap_or(view.mid()),
                label,
                detail: Some("Town: Liked".into()),
                companions: Vec::new(),
                companion_font: 2,
            }),
            ..shown(0, 0, 0)
        }
    }

    fn laid_left_top(ui: &Ui, t: TileId) -> (f64, f64) {
        let i = super::super::layout::layout(ui, t, &|_, _, _| (0.0, 0.0), true).into_iter().find(|i| i.tile == t).unwrap();
        (i.rect.left, i.rect.top)
    }

    #[test]
    fn the_world_map_writes_its_picture_rows_arrow_and_highlight_from_the_models_view() {
        let (mut ui, mut t) = (tree_changed(with_world), MapTiles::default());
        let s = world(1.0, [0.5, 0.5], vec![marker(10, [0.25, 0.5], false), marker(20, [0.5, 0.5], true)]);
        let menu = run(&mut ui, Some(s.clone()), &mut t).unwrap();
        let ws = s.world.clone().unwrap();
        // The picture where the model's view puts it: 1000 across, its middle at the window's.
        let parent = ui.find_tile(WORLD_PICTURE).unwrap();
        assert_eq!((trait_of(&ui, parent, "x"), trait_of(&ui, parent, "y"), trait_of(&ui, parent, "width"), trait_of(&ui, parent, "height")), (427.5 - 500.0, 250.0 - 500.0, 1000.0, 1000.0));
        assert_eq!(f64::from(ws.picture.min.x), trait_of(&ui, parent, "x"));
        assert_eq!((words(&ui, WORLD_PICTURE, "filename"), at(&ui, WORLD_PICTURE, "_Magnification"), at(&ui, WORLD_PICTURE, "filewidth")), ("map.dds".to_string(), 1.0, 1000.0));
        assert_eq!((at(&ui, WORLD_PICTURE, "user0"), at(&ui, WORLD_PICTURE, "user1"), at(&ui, WORLD_PICTURE, "user2")), (f64::from(data::MARKER), f64::from(data::QUEST_MARKER), f64::from(data::CURSOR)));
        // Its picture painted from its traits, outside the tree: stretched over its place, at its
        // alpha, under its rows, cut to the window; nothing added to the tree.
        let piece = t.pieces.iter().find(|p| p.tile == parent).expect("the picture painted");
        assert_eq!(piece.look, Look::Picture { name: "map.dds".into(), source: Source::Own, fit: Fit::Stretch });
        assert_eq!((piece.rect.left, piece.rect.top, piece.rect.width, piece.rect.height), (40.0 - 72.5, 90.0 - 250.0, 1000.0, 1000.0));
        let clip = piece.clip.unwrap();
        assert_eq!((clip.left, clip.top, clip.width, clip.height), (40.0, 90.0, 855.0, 500.0), "cut to the window");
        assert_eq!(piece.colour[3], 128.0 / 255.0, "the file's alpha");
        let at_piece = t.pieces.iter().position(|p| p.tile == parent).unwrap();
        let first_marker = t.pieces.iter().position(|p| t.written.world.markers.iter().any(|r| r.tile == p.tile));
        assert!(first_marker.is_none_or(|m| m > at_piece), "under the markers");
        assert_eq!(ui.tile(parent).children.iter().filter(|&&c| ui.tile(c).name.starts_with("NVF")).count(), 0, "no tile of NVForever's in the file's tree");
        // The markers by record under the picture, at their texture coordinates, placed on it.
        let rows = t.written.world.markers.clone();
        assert_eq!(rows.iter().map(|r| r.key).collect::<Vec<_>>(), vec![10, 20]);
        assert!(rows.iter().all(|r| ui.tile(r.tile).parent == Some(parent)), "under the picture: the file's hierarchy");
        assert_eq!((trait_of(&ui, rows[0].tile, "_x"), trait_of(&ui, rows[0].tile, "_y")), (0.25, 0.5));
        assert_eq!((trait_of(&ui, rows[0].tile, "_selected"), trait_of(&ui, rows[1].tile, "_selected")), (0.0, 1.0), "the highlighted marker");
        assert_eq!(ui.tile(rows[1].tile).get("filename").map(|v| v.text().to_string()).as_deref(), Some("icon_20.dds"));
        assert_eq!(ui.tile(rows[0].tile).get("_LocationName").map(|v| v.text().to_string()).as_deref(), Some("Place 10"));
        let (x, y) = laid_left_top(&ui, rows[0].tile);
        assert_eq!((x, y), (40.0 - 72.5 + 250.0, 90.0 - 250.0 + 500.0), "the window's corner, the picture's place, a quarter across it");
        // The quest marker and the custom marker from the quest template, the custom one over it.
        let quests = &t.written.world.quests;
        assert_eq!(quests.iter().map(|r| r.key).collect::<Vec<_>>(), vec![QuestRow::Target(77), QuestRow::Custom]);
        assert_eq!(ui.tile(quests[1].tile).get("filename").map(|v| v.text().to_string()).as_deref(), Some(data::PC_MARKER_FILE));
        assert_eq!((trait_of(&ui, quests[0].tile, "depth"), trait_of(&ui, quests[1].tile, "depth")), (4.0, 5.0));
        // The dotted lines: nodes at the arrow and both ends, a line under the start to each.
        let nodes = &t.written.world.nodes;
        assert_eq!(nodes.iter().map(|r| r.key).collect::<Vec<_>>(), vec![NodeRow::Start, NodeRow::End(data::marker::LineTo::Marker, 0), NodeRow::End(data::marker::LineTo::Quest, 77)]);
        let start = nodes[0].tile;
        let lines = &t.written.world.lines;
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|l| ui.tile(l.tile).parent == Some(start)));
        // To the custom marker: 100 left at 1 (a tenth of 1000), pointing left; to the quest, right.
        assert!((trait_of(&ui, lines[0].tile, "user0") - 100.0).abs() < 1e-3);
        assert!((trait_of(&ui, lines[0].tile, "rotateangle") - std::f64::consts::PI).abs() < 1e-6);
        assert!(trait_of(&ui, lines[1].tile, "rotateangle").abs() < 1e-6);
        // #4685: each line is cut by the map's window, not by its 6-wide start node's box.
        for l in lines {
            let clip = super::super::layout::layout(&ui, l.tile, &|_, _, _| (0.0, 0.0), true).into_iter().find(|i| i.tile == l.tile).unwrap().clip.expect("cut by the window");
            assert!(clip.width > 100.0 && clip.height > 100.0, "the line's clip is the map's window, not the node: {clip:?}");
        }
        // The arrow: the player's place and heading, shown; its brightness the painter's default,
        // which the file does not give and the nodes and lines copy; its zoom the file's.
        assert_eq!((at(&ui, WORLD_CURSOR, "_x"), at(&ui, WORLD_CURSOR, "_y"), at(&ui, WORLD_CURSOR, "rotateangle"), at(&ui, WORLD_CURSOR, "visible")), (0.5, 0.5, f64::from(data::arrow_turn(0.25)), 1.0));
        assert_eq!(at(&ui, WORLD_CURSOR, "brightness"), 255.0);
        assert!(ui.tile(ui.find_tile(WORLD_CURSOR).unwrap()).get("zoom").is_none(), "nothing written over the file's zoom");
        let c = ui.find_tile(WORLD_CURSOR).unwrap();
        ui.set_trait(c, "brightness", Value::Num(90.0));
        give_arrow_brightness(&mut ui, WORLD_CURSOR);
        assert_eq!(trait_of(&ui, c, "brightness"), 90.0, "a brightness given stays");
        // The highlight box on the model's highlight (the window's middle, on marker 20), shown,
        // its title the name and the reputation's line.
        assert_eq!((at(&ui, HIGHLIGHT_BOX, "x"), at(&ui, HIGHLIGHT_BOX, "y"), at(&ui, HIGHLIGHT_BOX, "visible")), (427.5 - 80.0, 250.0 - 80.0, 1.0));
        assert_eq!(words(&ui, HIGHLIGHT_BOX, "_Title"), "Place 20\nTown: Liked");
        // Measured: the window and the hint's box, for the model's pointer.
        let (window, hints) = t.written.counted.map.clone().unwrap();
        assert_eq!(window, WORLD_W);
        assert_eq!(hints.len(), 1, "the one hint shown");
        // Only the tabs and the hints take the pointer here: the map's is the model's.
        assert_eq!(data_tile(&ui, &t.written, rows[0].tile), None);
        assert_eq!(data_tile(&ui, &t.written, t.written.tabs[2].tile), Some(DataTile::Tab(2)));
        let x = ui.find_tile(&button_path("MM_ButtonX")).unwrap();
        assert_eq!(data_tile(&ui, &t.written, find_named(&ui, x, "button_text").unwrap()), Some(DataTile::Button(Slot::X)));
        assert_eq!(ui.menu(MAP_MENU), Some(menu));
        assert!(describe(&t.written).contains("tab world map, 2 markers, 1 quest markers, custom marker, 2 dotted lines, \"Place 20\" highlighted, magnification 1.00, hints: X Put Marker , device"), "{}", describe(&t.written));
        assert_eq!(t.written.counted.rows, 0, "no list on the World Map");
    }

    /// [design] The rows are made once and kept by record: a pan or a zoom moves the picture and
    /// writes no marker; a marker found or gone adds or unloads its row alone; the page going
    /// down and up keeps them; a tree built again makes them again.
    #[test]
    fn the_world_maps_rows_are_kept_by_record_across_pan_zoom_reopen_and_made_again_in_a_new_tree() {
        let (mut ui, mut t) = (tree_changed(with_world), MapTiles::default());
        let marks = vec![marker(10, [0.25, 0.5], false), marker(20, [0.5, 0.5], true), marker(30, [0.75, 0.125], false)];
        run(&mut ui, Some(world(1.0, [0.5, 0.5], marks.clone())), &mut t).unwrap();
        let tiles: Vec<TileId> = t.written.world.markers.iter().map(|r| r.tile).collect();
        // Zoomed in twice and panned to a corner: the same rows, their texture coordinates kept,
        // the picture 2000 across, each row re-placed with it.
        let s = world(2.0, [0.0, 0.0], marks.clone());
        run(&mut ui, Some(s.clone()), &mut t).unwrap();
        assert_eq!(t.written.world.markers.iter().map(|r| r.tile).collect::<Vec<_>>(), tiles);
        let parent = ui.find_tile(WORLD_PICTURE).unwrap();
        assert_eq!((trait_of(&ui, parent, "x"), trait_of(&ui, parent, "y"), trait_of(&ui, parent, "width")), (427.5, 250.0, 2000.0), "the corner at the middle, no further");
        assert_eq!(t.pieces.iter().find(|p| p.tile == parent).map(|p| p.rect.width), Some(2000.0), "the picture painted at its new size");
        assert_eq!(trait_of(&ui, tiles[2], "_x"), 0.75);
        let (x, _) = laid_left_top(&ui, tiles[2]);
        assert_eq!(x, 40.0 + 427.5 + 0.75 * 2000.0);
        // A marker gone and one found: the others keep their rows.
        let marks2 = vec![marker(30, [0.75, 0.125], false), marker(10, [0.25, 0.5], false), marker(40, [0.0, 1.0], false)];
        run(&mut ui, Some(world(2.0, [0.0, 0.0], marks2.clone())), &mut t).unwrap();
        let rows = &t.written.world.markers;
        assert_eq!(rows.iter().map(|r| r.key).collect::<Vec<_>>(), vec![30, 10, 40]);
        assert_eq!((rows[0].tile, rows[1].tile), (tiles[2], tiles[0]));
        assert!(!ui.is_live(tiles[1]), "the gone marker's row unloaded");
        // The page goes down (the decision forgets what it wrote) and comes up: the same rows.
        let kept: Vec<TileId> = rows.iter().map(|r| r.tile).collect();
        t.written.shown = None;
        run(&mut ui, Some(world(2.0, [0.0, 0.0], marks2.clone())), &mut t).unwrap();
        assert_eq!(t.written.world.markers.iter().map(|r| r.tile).collect::<Vec<_>>(), kept);
        // No custom marker and no quest: no node or line left.
        let mut s = world(1.0, [0.5, 0.5], marks2.clone());
        if let Some(w) = s.world.as_mut() {
            (w.custom, w.quests, w.ends) = (None, Vec::new(), Vec::new());
        }
        let start = t.written.world.nodes[0].tile;
        run(&mut ui, Some(s), &mut t).unwrap();
        assert!(t.written.world.nodes.is_empty() && t.written.world.lines.is_empty() && t.written.world.quests.is_empty());
        assert!(!ui.is_live(start));
        // The tree built again: everything made again in the new menu.
        ui.load_menu(&MemSource::default().with("m.xml", &fixture_xml(with_world)), "m.xml").unwrap();
        ui.update();
        run(&mut ui, Some(world(1.0, [0.5, 0.5], marks)), &mut t).unwrap();
        assert_eq!(t.written.world.markers.len(), 3);
        assert!(t.written.world.markers.iter().all(|r| ui.is_live(r.tile)));
        assert!(t.pieces.iter().any(|p| Some(p.tile) == ui.find_tile(WORLD_PICTURE)), "the picture painted in the new tree");
    }

    /// Markers at the picture's corners and two on the same spot each have their row; the
    /// highlight box's place follows the model's cursor wherever it is in the window.
    #[test]
    fn edge_and_overlapping_markers_each_have_their_row() {
        let (mut ui, mut t) = (tree_changed(with_world), MapTiles::default());
        let marks = vec![marker(1, [0.0, 0.0], false), marker(2, [1.0, 1.0], false), marker(3, [0.5, 0.5], true), marker(4, [0.5, 0.5], false)];
        let mut s = world(1.0, [0.0, 0.0], marks);
        if let Some(w) = s.world.as_mut() {
            w.cursor = Vec2::new(0.0, 499.0);
        }
        run(&mut ui, Some(s), &mut t).unwrap();
        let rows = t.written.world.markers.clone();
        assert_eq!(rows.len(), 4);
        // At 1 with its corner at the window's middle.
        assert_eq!(laid_left_top(&ui, rows[0].tile), (40.0 + 427.5, 90.0 + 250.0));
        assert_eq!(laid_left_top(&ui, rows[1].tile), (40.0 + 427.5 + 1000.0, 90.0 + 250.0 + 1000.0));
        assert_eq!(laid_left_top(&ui, rows[2].tile), laid_left_top(&ui, rows[3].tile));
        assert_eq!((at(&ui, HIGHLIGHT_BOX, "x"), at(&ui, HIGHLIGHT_BOX, "y")), (-80.0, 499.0 - 80.0));
    }

    /// The review's case: the file places the map's window apart from the window the model
    /// gathered the map in (a moved, smaller window): the page writes the map in its own window,
    /// the same point at its middle and the highlight box on its marker, and publishes that window.
    #[test]
    fn a_map_gathered_in_another_window_is_written_in_the_files() {
        let moved = |x: String| with_world(x).replace(r#"<hotrect name="MM_WorldMap_ClipWindow"><y>50</y><width>855</width><height>500</height>"#, r#"<hotrect name="MM_WorldMap_ClipWindow"><x>100</x><y>80</y><width>600</width><height>400</height>"#);
        let (mut ui, mut t) = (tree_changed(moved), MapTiles::default());
        let s = refit_to_tree(&ui, world(1.0, [0.5, 0.5], vec![marker(20, [0.5, 0.5], true)]));
        let measured = Rect::new(140.0, 120.0, 740.0, 520.0);
        let ws = s.world.clone().unwrap();
        assert_eq!((ws.window, ws.cursor), (measured, Vec2::new(300.0, 200.0)));
        run(&mut ui, Some(s), &mut t).unwrap();
        let parent = ui.find_tile(WORLD_PICTURE).unwrap();
        assert_eq!((trait_of(&ui, parent, "x"), trait_of(&ui, parent, "y")), (300.0 - 500.0, 200.0 - 500.0), "the middle point at the measured window's middle");
        assert_eq!((at(&ui, HIGHLIGHT_BOX, "x"), at(&ui, HIGHLIGHT_BOX, "y")), (300.0 + 100.0 - 80.0, 200.0 + 30.0 - 80.0), "on the marker, in the highlight box's own window");
        assert_eq!(t.written.counted.map.as_ref().map(|m| m.0), Some(measured), "the window published for the model's pointer");
        // A cursor on no marker keeps its place on the canvas, held in the window.
        let mut s = world(1.0, [0.5, 0.5], vec![marker(20, [0.5, 0.5], false)]);
        if let Some(w) = s.world.as_mut() {
            (w.cursor, w.want) = (Vec2::new(10.0, 450.0), Vec2::new(10.0, 450.0));
        }
        let ws = refit_to_tree(&ui, s).world.unwrap();
        assert_eq!(ws.cursor, Vec2::new(0.0, 400.0), "(40 + 10, 90 + 450) in the window at (140, 120), held");
        // The window the model gathered it in: nothing changes.
        let same = refit_to_tree(&tree_changed(with_world), world(1.0, [0.5, 0.5], Vec::new()));
        assert_eq!(same, world(1.0, [0.5, 0.5], Vec::new()));
    }

    /// The fixture with the World Map's parts and the Local Map's, written for these tests with
    /// their own numbers and expressions (none of them the file's): the Local Map's window at the
    /// same place, its picture's hotrect hidden as the file has it and placed by its drag, its arrow
    /// on it by texture coordinates with a pivot at its middle, and a quest marker template of its
    /// own (placed as the others, no clamp).
    fn with_local(x: String) -> String {
        let local = r#"<hotrect name="MM_LocalMap_ClipWindow"><y>50</y><width>855</width><height>500</height><clipwindow>&true;</clipwindow>
                <hotrect name="MM_LocalMap_ParentImage"><clips>&true;</clips><visible>&false;</visible><_Magnification>1</_Magnification>
                  <width><copy src="me()" trait="filewidth"/><mul src="me()" trait="_Magnification"/></width>
                  <height><copy src="me()" trait="fileheight"/><mul src="me()" trait="_Magnification"/></height>
                  <x><copy src="me()" trait="dragdeltax"/></x><y><copy src="me()" trait="dragdeltay"/></y>
                </hotrect>
                <image name="MM_LocalMapCursor"><filename>arrow.dds</filename><clips>&true;</clips><visible>&false;</visible><height>20</height><rotateaxisx>5</rotateaxisx><rotateaxisy>10</rotateaxisy>
                  <width><copy src="sibling(MM_LocalMap_ParentImage)" trait="user2"/></width>
                  <x><copy src="sibling(MM_LocalMap_ParentImage)" trait="width"/><mul src="me()" trait="_x"/><add src="sibling(MM_LocalMap_ParentImage)" trait="x"/></x>
                  <y><copy src="sibling(MM_LocalMap_ParentImage)" trait="height"/><mul src="me()" trait="_y"/><add src="sibling(MM_LocalMap_ParentImage)" trait="y"/></y>
                </image>
              </hotrect>"#;
        let place = r#"<x><copy src="parent()" trait="width"/><mul src="me()" trait="_x"/></x><y><copy src="parent()" trait="height"/><mul src="me()" trait="_y"/></y>"#;
        let template = format!(r#"<template name="LocalMapQuestMarkerTemplate"><image name="LocalMapQuestMarker"><clips>&true;</clips><width><copy src="parent()" trait="user1"/></width><height>12</height>{place}</image></template>"#);
        with_world(x).replace(r#"<text name="MM_Headline_ChallengeType"></text>"#, &format!(r#"<text name="MM_Headline_ChallengeType"></text>{local}"#)).replace("</menu>", &format!("{template}</menu>"))
    }

    /// The Local Map in the fixture's window, its frame 24000 by 20000 game units round the
    /// player at `centre` (28 units a unit outdoors), `outline` its picture's texels once made,
    /// two doors, a quest target and the custom marker; the highlight box on the first door.
    fn local(outline: Option<Vec2>, mag: f32) -> Shown {
        let p = data::view::LocalProjection { centre: [1000.0, 2000.0], mag, scale: 28.0, origin: [-11000.0, 12000.0], extent: Vec2::new(24000.0, 20000.0) };
        let view = data::MapView::local(WORLD_W, p);
        let door = |form, x: f32, y: f32, selected| MarkerShown { form, uv: view.local_uv(x, y), icon: "door.dds".into(), selected, name: format!("Door {form}"), detail: None };
        Shown {
            tab: data::LOCAL,
            rows: Vec::new(),
            selected: None,
            pane: Pane::None,
            hints: vec![HintShown { slot: Slot::X, kind: ButtonKind::PlaceMarker, word: "Put Marker".into(), key: String::new(), dim: false }],
            world: Some(WorldShown {
                local: true,
                outline: outline.map(|t| (Handle::<Image>::default(), t)),
                view,
                texture: outline.map(|_| super::super::runtime_images::filename(LOCAL_IMAGE)).unwrap_or_default(),
                picture: view.picture(),
                window: WORLD_W,
                mag,
                usable: p.extent / p.scale,
                markers: vec![door(7, 1000.0, 2000.0, true), door(8, 1500.0, 1000.0, false)],
                quests: vec![(55, view.local_uv(2000.0, 2000.0))],
                custom: Some(view.local_uv(0.0, 2000.0)),
                arrow: Some((view.local_uv(1000.0, 2000.0), 0.75)),
                ends: Vec::new(),
                cursor: view.place(1000.0, 2000.0),
                want: view.place(1000.0, 2000.0),
                label: Some("Door 7".into()),
                detail: None,
                companions: Vec::new(),
                companion_font: 2,
            }),
            ..shown(0, 0, 0)
        }
    }

    #[test]
    fn the_local_map_writes_its_frame_outline_rows_and_turned_arrow() {
        let (mut ui, mut t) = (tree_changed(with_local), MapTiles::default());
        // The World Map first: its window hidden once the Local Map shows.
        run(&mut ui, Some(world(1.0, [0.5, 0.5], Vec::new())), &mut t).unwrap();
        let s = local(Some(Vec2::new(250.0, 209.0)), 1.0);
        let ws = s.world.clone().unwrap();
        run(&mut ui, Some(s), &mut t).unwrap();
        assert_eq!((at(&ui, LOCAL_CLIP, "visible"), at(&ui, WORLD_CLIP, "visible"), at(&ui, LOCAL_PICTURE, "visible")), (1.0, 0.0, 1.0), "its window and picture shown, the World Map's hidden");
        // The frame placed by the projection: its top left 12000 west and 10000 north of the
        // centre at 28 a unit from the window's middle; 24000 / 28 across.
        let parent = ui.find_tile(LOCAL_PICTURE).unwrap();
        let (x, y) = (427.5 - 12000.0 / 28.0, 250.0 - 10000.0 / 28.0);
        assert!((trait_of(&ui, parent, "x") - x).abs() < 1e-3 && (trait_of(&ui, parent, "y") - y).abs() < 1e-3);
        assert!((trait_of(&ui, parent, "width") - 24000.0 / 28.0).abs() < 1e-3);
        assert_eq!(words(&ui, LOCAL_PICTURE, "filename"), "nvf:local_map", "the outline by its runtime name");
        // The picture painted from the hotrect's traits, outside the tree.
        let piece = t.pieces.iter().find(|p| p.tile == parent).expect("the outline painted");
        assert_eq!(piece.look, Look::Picture { name: "nvf:local_map".into(), source: Source::Runtime(None), fit: Fit::Stretch }, "a runtime picture, not a file");
        // The doors and the quest and custom markers by record under the frame.
        let rows = &t.written.local.markers;
        assert_eq!(rows.iter().map(|r| r.key).collect::<Vec<_>>(), vec![7, 8]);
        assert!(rows.iter().all(|r| ui.tile(r.tile).parent == Some(parent)));
        assert_eq!(trait_of(&ui, rows[0].tile, "_selected"), 1.0);
        assert!(t.written.world.markers.is_empty() || t.written.world.markers.iter().all(|r| ui.tile(r.tile).parent != Some(parent)), "the World Map's rows are its own");
        let quests = &t.written.local.quests;
        assert_eq!(quests.iter().map(|r| r.key).collect::<Vec<_>>(), vec![QuestRow::Target(55), QuestRow::Custom]);
        assert!(quests.iter().all(|r| ui.tile(r.tile).name == "LocalMapQuestMarker"), "the Local Map's template");
        assert!(t.written.local.nodes.is_empty() && t.written.local.lines.is_empty(), "no dotted line on the Local Map");
        // The arrow: its place, turned by the heading about its pivot, shown.
        assert_eq!((at(&ui, LOCAL_CURSOR, "rotateangle"), at(&ui, LOCAL_CURSOR, "visible")), (f64::from(data::arrow_turn(0.75)), 1.0), "a quarter turn less than the heading: the picture points right");
        assert_eq!(at(&ui, LOCAL_CURSOR, "_x"), f64::from(ws.arrow.unwrap().0[0]));
        let cursor = ui.find_tile(LOCAL_CURSOR).unwrap();
        let arrow = t.pieces.iter().find(|p| p.tile == cursor).expect("the arrow painted");
        assert_eq!(arrow.turn.map(|r| (r.angle, r.pivot)), Some((f64::from(data::arrow_turn(0.75)), (5.0, 10.0))), "turned about the file's axis");
        // The highlight box on the door, its name.
        assert_eq!((at(&ui, HIGHLIGHT_BOX, "x"), at(&ui, HIGHLIGHT_BOX, "y")), (427.5 - 80.0, 250.0 - 80.0));
        assert_eq!(words(&ui, HIGHLIGHT_BOX, "_Title"), "Door 7");
        assert_eq!(t.written.counted.map.as_ref().map(|m| m.0), Some(WORLD_W));
        assert!(describe(&t.written).contains("tab local map, 2 markers, 1 quest markers, custom marker, picture 250 x 209, \"Door 7\" highlighted"), "{}", describe(&t.written));
    }

    /// [design] The outline's picture as it fills in and is replaced: no picture while its grid is
    /// being made (nothing painted, the page still drawn), its runtime picture given when made and
    /// given again for a new outline (a new revision, so the painter repaints), taken away when
    /// gone; the frame's rows kept across a zoom.
    #[test]
    fn the_outline_is_given_as_made_and_replaced_and_the_rows_outlast_a_zoom() {
        let (mut ui, mut t) = (tree_changed(with_local), MapTiles::default());
        run(&mut ui, Some(local(None, 1.0)), &mut t).unwrap();
        let parent = ui.find_tile(LOCAL_PICTURE).unwrap();
        assert_eq!(words(&ui, LOCAL_PICTURE, "filename"), "");
        assert!(!t.pieces.iter().any(|p| p.tile == parent), "nothing painted before the outline is made");
        assert!(describe(&t.written).contains("picture not made yet"));
        let tiles: Vec<TileId> = t.written.local.markers.iter().map(|r| r.tile).collect();
        run(&mut ui, Some(local(None, 2.0)), &mut t).unwrap();
        assert_eq!(t.written.local.markers.iter().map(|r| r.tile).collect::<Vec<_>>(), tiles, "a zoom keeps the rows");
        assert!((trait_of(&ui, parent, "width") - 2.0 * 24000.0 / 28.0).abs() < 1e-3);
        // The runtime picture: given, given again for a new outline, taken away.
        let mut images = super::super::runtime_images::RuntimeImages::default();
        let mut last = None;
        let mut assets = Assets::<Image>::default();
        let first = (assets.add(Image::default()), Vec2::new(250.0, 209.0));
        assert!(sync_outline(&mut images, Some(&first), &mut last));
        let r1 = images.get("nvf:local_map").map(|e| (e.revision, e.size));
        assert_eq!(r1.map(|r| r.1), Some(Vec2::new(250.0, 209.0)), "its texels' size");
        assert!(!sync_outline(&mut images, Some(&first), &mut last), "the same picture: nothing to do");
        let second = (assets.add(Image::default()), Vec2::new(300.0, 200.0));
        assert!(sync_outline(&mut images, Some(&second), &mut last));
        let r2 = images.get("nvf:local_map").map(|e| (e.revision, e.size)).unwrap();
        assert!(r2.0 > r1.unwrap().0 && r2.1 == Vec2::new(300.0, 200.0), "a new revision: repainted");
        assert!(sync_outline(&mut images, None, &mut last));
        assert!(images.get("nvf:local_map").is_none(), "gone with the outline");
    }

    /// The review's case: the piece this page makes for the Local Map's picture goes through the
    /// runtime pictures as the painter takes it (`runtime_images::mark`, `picture`): stamped with
    /// the outline given, repainted (a new stamp) when another is given, and drawing nothing once
    /// it is taken away; a World Map picture's file is never taken for a runtime one.
    #[test]
    fn the_local_maps_picture_piece_is_resolved_as_a_runtime_picture() {
        use super::super::runtime_images::{self as runtime, RuntimeImages};
        let (mut ui, mut t) = (tree_changed(with_local), MapTiles::default());
        run(&mut ui, Some(local(Some(Vec2::new(250.0, 209.0)), 1.0)), &mut t).unwrap();
        let parent = ui.find_tile(LOCAL_PICTURE).unwrap();
        let piece = || t.pieces.iter().find(|p| p.tile == parent).cloned().expect("the outline's piece");
        let mut images = RuntimeImages::default();
        let mut assets = Assets::<Image>::default();
        let mut last = None;
        let mut told = runtime::Told::default();
        let stamp = |images: &RuntimeImages, told: &mut runtime::Told| {
            let mut set = vec![piece()];
            runtime::mark(&mut set, Some(images), told);
            match &set[0].look {
                Look::Picture { source: Source::Runtime(s), .. } => *s,
                other => panic!("{other:?}"),
            }
        };
        let first = (assets.add(Image::default()), Vec2::new(250.0, 209.0));
        sync_outline(&mut images, Some(&first), &mut last);
        let s1 = stamp(&images, &mut told).expect("stamped with the outline given");
        assert_eq!(s1.size, Vec2::new(250.0, 209.0));
        assert_eq!(runtime::picture(Some(&images), "nvf:local_map", &mut told).map(|p| p.0), Some(first.0.clone()), "its texture");
        let second = (assets.add(Image::default()), Vec2::new(300.0, 200.0));
        sync_outline(&mut images, Some(&second), &mut last);
        let s2 = stamp(&images, &mut told).unwrap();
        assert!(s2 != s1 && s2.size == Vec2::new(300.0, 200.0), "another stamp: repainted");
        assert_eq!(runtime::picture(Some(&images), "nvf:local_map", &mut told).map(|p| p.0), Some(second.0));
        sync_outline(&mut images, None, &mut last);
        assert_eq!(stamp(&images, &mut told), None, "taken away: no stamp");
        assert!(runtime::picture(Some(&images), "nvf:local_map", &mut told).is_none(), "and nothing drawn");
        // The World Map's picture is a file.
        let (mut ui, mut t) = (tree_changed(with_world), MapTiles::default());
        run(&mut ui, Some(world(1.0, [0.5, 0.5], Vec::new())), &mut t).unwrap();
        let parent = ui.find_tile(WORLD_PICTURE).unwrap();
        assert!(matches!(&t.pieces.iter().find(|p| p.tile == parent).unwrap().look, Look::Picture { source: Source::Own, .. }));
    }

    #[test]
    fn the_local_map_falls_back_without_its_window_or_template_and_the_world_map_does_not_need_them() {
        // A file without the Local Map's quest template: the Local Map falls back, the World Map not.
        let mut ui = tree_changed(|x| with_local(x).replace("LocalMapQuestMarkerTemplate", "OtherTemplate"));
        assert_eq!(run(&mut ui, Some(local(None, 1.0)), &mut MapTiles::default()), Err("LocalMapQuestMarkerTemplate missing".to_string()));
        assert!(run(&mut ui, Some(world(1.0, [0.5, 0.5], Vec::new())), &mut MapTiles::default()).is_ok());
        // The World Map's fixture alone has no Local Map window.
        let err = run(&mut tree_changed(with_world), Some(local(None, 1.0)), &mut MapTiles::default()).unwrap_err();
        assert!(err.contains("MM_LocalMap_ClipWindow missing"), "{err}");
        // A window off the canvas.
        let off = |x: String| with_local(x).replace(r#"<hotrect name="MM_LocalMap_ClipWindow"><y>50</y>"#, r#"<hotrect name="MM_LocalMap_ClipWindow"><y>500</y>"#);
        assert_eq!(run(&mut tree_changed(off), Some(local(None, 1.0)), &mut MapTiles::default()), Err("the local map's window does not fit".to_string()));
        // The prepared Local Map is read with its parts; a Local Map without a place is not drawn.
        let p = data::view::LocalProjection { centre: [0.0, 0.0], mag: 1.0, scale: 8.0, origin: [-100.0, 100.0], extent: Vec2::new(200.0, 200.0) };
        let view = data::MapView::local(WORLD_W, p);
        let parts = data::WorldParts {
            view,
            texture: String::new(),
            mag: 1.0,
            quests: Vec::new(),
            custom: None,
            arrow: Some(([0.5, 0.5], 0.0)),
            ends: Vec::new(),
            outline: Some((Handle::<Image>::default(), Vec2::new(25.0, 25.0))),
            rows: None,
            want: Vec2::ZERO,
            companions: Vec::new(),
            companion_font: 2,
        };
        let map = data::MapShown::for_test(Vec::new(), Some(parts), Vec2::ZERO, None, None);
        let prepared = |tab, map: data::MapShown| Prepared { view: data::View { tab, place: String::new(), when: String::new(), buttons: Vec::new(), body: Body::Map(Box::new(map)), popup: None }, keys: Vec::new(), sel: 0, pointer_pick: false, navigated: 0, challenges: false, filtered: false };
        let s = Shown::of(&prepared(data::LOCAL, map.clone()), true).unwrap();
        let w = s.world.unwrap();
        assert!(w.local && w.texture == "nvf:local_map");
        assert!(Shown::of(&prepared(data::WORLD, map), true).is_err(), "a Local Map is not the World Map's");
        let bare = data::MapShown::for_test(Vec::new(), None, Vec2::ZERO, None, None);
        assert_eq!(Shown::of(&prepared(data::LOCAL, bare), true), Err("the local map has no place here".to_string()));
    }

    /// The review's case: the box was on one marker in the window the model gathered the map in;
    /// in the file's window, moved, the box's place on the canvas is nearest another marker, which
    /// it takes (the model's rule, `data::nearest`), its name with it and no other's reputation.
    #[test]
    fn a_refit_picks_the_marker_under_the_box_again() {
        let moved = |x: String| with_world(x).replace(r#"<hotrect name="MM_WorldMap_ClipWindow"><y>50</y><width>855</width><height>500</height>"#, r#"<hotrect name="MM_WorldMap_ClipWindow"><x>100</x><y>80</y><width>600</width><height>400</height>"#);
        let ui = tree_changed(moved);
        // Before: the box at the window's middle on marker 20; marker 21 34 units off.
        let s = world(1.0, [0.5, 0.5], vec![marker(20, [0.5, 0.5], true), marker(21, [0.5275, 0.52], false)]);
        assert_eq!(s.world.as_ref().map(|w| w.label.clone()), Some(Some("Place 20".to_string())));
        let w = refit_to_tree(&ui, s).world.unwrap();
        // The box kept its canvas place, (40 + 427.5, 90 + 250), in the window at (140, 120); marker
        // 21 is there now, 20 at the window's middle 34 away.
        assert!((w.want - Vec2::new(327.5, 220.0)).length() < 1e-3, "{:?}", w.want);
        assert_eq!(w.markers.iter().map(|m| (m.form, m.selected)).collect::<Vec<_>>(), vec![(20, false), (21, true)]);
        assert!((w.cursor - Vec2::new(327.5, 220.0)).length() < 1e-2, "on marker 21");
        assert_eq!((w.label.as_deref(), w.detail.as_deref()), (Some("Place 21"), Some("Town 21: Neutral")), "its name and its own reputation line, not marker 20's");
        // The same window: the pick as it was, the reputation kept.
        let same = refit_to_tree(&tree_changed(with_world), world(1.0, [0.5, 0.5], vec![marker(20, [0.5, 0.5], true), marker(21, [0.5275, 0.52], false)])).world.unwrap();
        assert_eq!((same.label.as_deref(), same.detail.as_deref()), (Some("Place 20"), Some("Town: Liked")));
    }

    #[test]
    fn the_world_map_falls_back_without_its_window_or_templates_and_its_tiles_are_only_its_own() {
        // A file without a waypoint template: the World Map falls back, Quests is still drawn.
        let mut ui = tree_changed(|x| with_world(x).replace("WaypointTemplate", "OtherTemplate"));
        assert_eq!(run(&mut ui, Some(world(1.0, [0.5, 0.5], Vec::new())), &mut MapTiles::default()), Err("WaypointTemplate missing".to_string()));
        assert!(run(&mut ui, Some(shown(2, 0, 1)), &mut MapTiles::default()).is_ok(), "Quests needs none of the map's");
        // The base fixture has no map window.
        let err = run(&mut tree(), Some(world(1.0, [0.5, 0.5], Vec::new())), &mut MapTiles::default()).unwrap_err();
        assert!(err.contains("MM_WorldMap_ClipWindow missing"), "{err}");
        // The test aid's cut of a template is the World Map's alone.
        let (mut ui, mut t) = (tree_changed(with_world), MapTiles { cut: Some("MapMarkerTemplate".into()), ..Default::default() });
        assert_eq!(run(&mut ui, Some(world(1.0, [0.5, 0.5], Vec::new())), &mut t), Err("MapMarkerTemplate missing".to_string()));
        assert!(run(&mut ui, Some(shown(1, 0, 0)), &mut t).is_ok());
        // A window off the canvas, or hidden: a layout's failure.
        let off = |x: String| with_world(x).replace(r#"<hotrect name="MM_WorldMap_ClipWindow"><y>50</y>"#, r#"<hotrect name="MM_WorldMap_ClipWindow"><y>500</y>"#);
        let (mut ui, mut t) = (tree_changed(off), MapTiles::default());
        assert_eq!(run(&mut ui, Some(world(1.0, [0.5, 0.5], Vec::new())), &mut t), Err("the world map's window does not fit".to_string()));
        assert!(t.written.layout_failure);
        let (mut ui, mut t) = (tree_changed(with_world), MapTiles::default());
        assert!(run(&mut ui, Some(world(1.0, [0.5, 0.5], Vec::new())), &mut t).is_ok());
        // A script's picture on the map's tile is the one painted, from its traits.
        let parent = ui.find_tile(WORLD_PICTURE).unwrap();
        ui.set_trait(parent, "filename", Value::Str("scripted.dds".into()));
        ui.update();
        assert!(run(&mut ui, None, &mut t).is_ok());
        assert!(matches!(&t.pieces.iter().find(|p| p.tile == parent).unwrap().look, Look::Picture { name, .. } if name == "scripted.dds"));
        // The picture's tile hidden (by a script, say): nothing of the map would show.
        ui.set_trait(parent, "visible", Value::Num(0.0));
        ui.update();
        assert_eq!(run(&mut ui, None, &mut t), Err("the map's picture is not shown in its window".to_string()));
    }

    #[test]
    fn the_prepared_world_map_is_read_with_its_parts_and_the_local_map_is_not() {
        let view = data::MapView::world(WORLD_W, [0.5, 0.5], Vec2::splat(1000.0), 1.0);
        let mark = |form, uv: Option<[f32; 2]>| data::MapMark { icon: Some((1, true)), custom_icon: None, highlighted: form == 2, at: Vec2::ZERO, form, name: format!("P{form}"), uv, detail: None, recent: false, outside: None };
        let parts = data::WorldParts {
            view,
            texture: "w.dds".into(),
            mag: 1.0,
            quests: vec![(9, [0.1, 0.2])],
            custom: None,
            arrow: Some(([0.5, 0.5], 1.0)),
            ends: Vec::new(),
            outline: None,
            rows: None,
            want: Vec2::new(5.0, 6.0),
            companions: Vec::new(),
            companion_font: 2,
        };
        let map = data::MapShown::for_test(vec![mark(1, Some([0.1, 0.1])), mark(2, Some([0.2, 0.2])), mark(3, None)], Some(parts.clone()), Vec2::new(5.0, 6.0), Some("P2"), None);
        let view_of = |tab, map: data::MapShown| data::View { tab, place: "Here".into(), when: "now".into(), buttons: Vec::new(), body: Body::Map(Box::new(map)), popup: None };
        let p = |tab, map| Prepared { view: view_of(tab, map), keys: Vec::new(), sel: 0, pointer_pick: false, navigated: 0, challenges: false, filtered: false };
        let s = Shown::of(&p(data::WORLD, map.clone()), true).unwrap();
        let w = s.world.expect("the World Map's parts");
        assert_eq!(w.markers.iter().map(|m| (m.form, m.selected)).collect::<Vec<_>>(), vec![(1, false), (2, true)], "a mark without texture coordinates is not the World Map's");
        assert_eq!((w.picture, w.window, w.usable, w.cursor, w.label.as_deref()), (view.picture(), WORLD_W, Vec2::splat(1000.0), Vec2::new(5.0, 6.0), Some("P2")));
        assert_eq!(w.markers[0].icon, data::MapMark { icon: Some((1, true)), custom_icon: None, highlighted: false, at: Vec2::ZERO, form: 1, name: String::new(), uv: None, detail: None, recent: false, outside: None }.picture());
        let custom_mark = data::MapMark { icon: Some((1, true)), custom_icon: Some("Interface\\Icons\\Owned\\custom.dds".into()), highlighted: false, at: Vec2::ZERO, form: 1, name: "P1".into(), uv: Some([0.1, 0.1]), detail: None, recent: false, outside: None };
        let custom_map = data::MapShown::for_test(vec![custom_mark], Some(parts), Vec2::new(5.0, 6.0), Some("P1"), None);
        let custom_shown = Shown::of(&p(data::WORLD, custom_map), true).unwrap().world.expect("custom world");
        assert_eq!(custom_shown.markers[0].icon, "Interface\\Icons\\Owned\\custom.dds");
        assert!(s.rows.is_empty() && s.pane == Pane::None);
        // The question box up: the same page, drawn under it.
        let mut boxed = p(data::WORLD, map.clone());
        boxed.view.popup = Some(crate::pipboy::data::travel::Popup::travel(1, "Go?".into()));
        assert_eq!(Shown::of(&boxed, true), Shown::of(&p(data::WORLD, map.clone()), true));
        // The Local Map, or a World Map without its viewport: not drawn from the file.
        assert!(Shown::of(&p(data::LOCAL, map), true).is_err());
        let bare = data::MapShown::for_test(Vec::new(), None, Vec2::ZERO, None, None);
        assert_eq!(Shown::of(&p(data::WORLD, bare), true), Err("the world map has no picture here".to_string()));
    }

    /// [design] The question box owns the frame's tile input while it is up, and after it is answered
    /// until the left button is let go; a press begun with no box never makes it own anything.
    #[test]
    fn the_question_box_owns_the_input_until_the_button_is_let_go() {
        assert!(popup_owns(true, false, false), "up: owns");
        assert!(popup_owns(true, true, true));
        assert!(popup_owns(false, true, true), "answered by a press still held: owns until let go");
        assert!(!popup_owns(false, true, false), "let go: the page's again");
        assert!(!popup_owns(false, false, true), "a press with no box: the page's");
    }

    #[test]
    fn a_missing_menu_tile_or_template_falls_back_and_comes_back() {
        // No menu.
        let mut t = MapTiles::default();
        assert_eq!(run(&mut Ui::new(), Some(shown(1, 0, 0)), &mut t), Err(format!("{MAP_MENU} is not in the tree")));
        // A hint tile cut by the test aid, then not.
        let (mut ui, mut t) = (tree(), MapTiles { cut: Some("MM_ButtonX".into()), ..Default::default() });
        let err = run(&mut ui, Some(shown(1, 0, 0)), &mut t).unwrap_err();
        assert!(err.contains("MM_ButtonX missing"), "{err}");
        assert!(!t.written.layout_failure, "a missing tile waits for the tree");
        t.cut = None;
        t.written.checked.clear();
        assert!(run(&mut ui, Some(shown(1, 0, 0)), &mut t).is_ok(), "recovered");
        // A file without the row template, or without the objectives' list.
        let mut ui = tree_changed(|x| x.replace("MM_ListMarkerTemplate", "OtherTemplate"));
        assert_eq!(run(&mut ui, Some(shown(1, 0, 0)), &mut MapTiles::default()), Err(format!("{ROW_TEMPLATE} missing")));
        let mut ui = tree_changed(|x| x.replace("MM_QuestObjectivesList", "SomethingElse"));
        assert_eq!(run(&mut ui, Some(shown(1, 0, 0)), &mut MapTiles::default()), Err(format!("{OBJECTIVES} missing")));
        // The tree built again: everything written again in the new menu.
        let (mut ui, mut t) = (tree(), MapTiles::default());
        let old = run(&mut ui, Some(shown(2, 0, 0)), &mut t).unwrap();
        ui.load_menu(&MemSource::default().with("m.xml", &fixture_xml(|x| x)), "m.xml").unwrap();
        ui.update();
        let new = run(&mut ui, Some(shown(2, 0, 0)), &mut t).unwrap();
        assert_ne!(old, new);
        assert!(t.written.rows[0].iter().all(|r| ui.is_live(r.tile)) && t.written.rows[0].len() == 2);
    }

    #[test]
    fn an_unusable_layout_falls_back() {
        // A list that scrolls without a window.
        let mut ui = tree_changed(|x| x.replacen("<clipwindow>&true;</clipwindow>", "", 1));
        assert_eq!(run(&mut ui, Some(shown(10, 0, 0)), &mut MapTiles::default()), Err("the quests list scrolls and has no window".to_string()));
        // A list off the canvas: no row shows whole.
        let mut ui = tree_changed(|x| x.replace(r#"<hotrect name="MM_QuestsList"><y>60</y>"#, r#"<hotrect name="MM_QuestsList"><y>2000</y>"#));
        assert_eq!(run(&mut ui, Some(shown(2, 0, 0)), &mut MapTiles::default()), Err("no quest's row shows whole".to_string()));
        // Objectives with no window, or a window off the canvas: no usable pane.
        let mut ui = tree_changed(|x| x.replace(r#"<hotrect name="MM_QuestObjectivesList"><width>380</width><height>200</height><clipwindow>&true;</clipwindow>"#, r#"<hotrect name="MM_QuestObjectivesList"><width>380</width><height>200</height>"#));
        let mut t = MapTiles::default();
        assert_eq!(run(&mut ui, Some(shown(2, 0, 1)), &mut t), Err("the objectives have no window".to_string()));
        assert!(t.written.layout_failure, "a layout's failure");
        let mut ui = tree_changed(|x| x.replace(r#"<rect name="MM_DataRect"><x>450</x><y>200</y>"#, r#"<rect name="MM_DataRect"><x>450</x><y>700</y>"#));
        assert_eq!(run(&mut ui, Some(shown(2, 0, 1)), &mut MapTiles::default()), Err("the objectives' window does not fit".to_string()));
        // Objectives whose list is hidden: none shows in the window.
        let mut ui = tree_changed(|x| x.replace(r#"<hotrect name="MM_QuestObjectivesList"><width>380</width>"#, r#"<hotrect name="MM_QuestObjectivesList"><visible>&false;</visible><width>380</width>"#));
        let mut t = MapTiles::default();
        let err = run(&mut ui, Some(shown(2, 0, 2)), &mut t).unwrap_err();
        // Whether a hidden list's window is laid out at all is the layout's; either way no pane.
        assert!(err == "no objective shows in its window" || err == "the objectives have no window", "{err}");
        assert!(t.written.layout_failure);
        // Their words hidden, or placed outside the window, after a pass that went through: the
        // next final pass falls back.
        for (name, value) in [("visible", 0.0), ("x", 2000.0), ("y", -500.0)] {
            let (mut ui, mut t) = (tree(), MapTiles::default());
            assert!(run(&mut ui, Some(shown(2, 0, 2)), &mut t).is_ok());
            for row in t.written.objectives.clone() {
                let words = list::child(&ui, row.tile, ROW_TEXT).unwrap();
                ui.set_trait(words, name, Value::Num(value));
            }
            ui.update();
            assert_eq!(run(&mut ui, None, &mut t), Err("no objective shows in its window".to_string()), "{name} {value}");
        }
        // A quest with no objective shown asks for the window only.
        assert!(run(&mut tree(), Some(shown(2, 0, 0)), &mut MapTiles::default()).is_ok());
        // No quest, no objectives: no pane is asked for.
        let mut s = shown(0, 0, 0);
        s.hints.clear();
        assert!(run(&mut ui, Some(s), &mut MapTiles::default()).is_ok());
        // Nothing to show yet: no snapshot, nothing written.
        let (mut ui, mut t) = (tree(), MapTiles::default());
        assert_eq!(run(&mut ui, None, &mut t), Err("nothing to show yet".to_string()));
    }

    /// A failure waits for a change once it has lasted: a missing name for the tree to change, a
    /// layout's failure only so long, as what it met can change without the tree's shape.
    #[test]
    fn a_layout_failure_is_tried_again_now_and_then_a_missing_name_waits_for_the_tree() {
        let tree = TreeMark { menu: None, shape: 7 };
        let mut w = Written { failing: FAILING_PASSES, tried: Some((data::QUESTS, true)), tried_tree: Some(tree), ..Default::default() };
        assert!((0..500).all(|_| w.waits(data::QUESTS, true, tree, false)), "a missing name waits");
        assert!(!w.waits(data::QUESTS, true, TreeMark { shape: 8, ..tree }, false), "the tree changed");
        assert!(!w.waits(data::QUESTS, true, tree, true), "what it would show changed");
        assert!(!w.waits(data::QUESTS, false, tree, false), "the other frame");
        w.layout_failure = true;
        w.waited = 0;
        let tries: Vec<usize> = (1..=150).filter(|_| !w.waits(data::QUESTS, true, tree, false)).collect();
        assert_eq!(tries.len(), 2, "tried again every {LAYOUT_RETRY_PASSES} passes");
        assert!(!Written { failing: FAILING_PASSES - 1, ..Default::default() }.waits(data::QUESTS, true, tree, false), "not before it lasted");
    }

    /// [design] Each visit to a tab drawn from the file is told with its frames by what drew them,
    /// when it ends: another tab, off DATA.
    #[test]
    fn each_visit_counts_its_frames_by_what_drew_them() {
        let mut f = Frames::default();
        assert_eq!(f.step(None, Drew::Moving), None, "off DATA: nothing counted");
        assert_eq!(f.step(Some(data::WORLD), Drew::Moving), None);
        assert_eq!(f.step(Some(data::WORLD), Drew::Tiles), None);
        assert_eq!(f.step(Some(data::WORLD), Drew::Tiles), None);
        assert_eq!(f.step(Some(data::WORLD), Drew::Failed), None);
        assert_eq!(f.step(Some(data::WORLD), Drew::Unprepared), None);
        assert_eq!(f.step(Some(data::QUESTS), Drew::Tiles), Some((data::WORLD, [2, 1, 1, 1])), "the World Map's visit ends");
        assert_eq!(f.step(None, Drew::Moving), Some((data::QUESTS, [1, 0, 0, 0])), "the Pip-Boy down");
        assert_eq!(f.step(None, Drew::Moving), None);
    }

    /// Every system this page and the DATA page add, with their neighbours, initialised in one
    /// schedule: a system parameter that conflicts with another (B0002) panics here, in the tests,
    /// not at the game's start (the 2026-11-12 smoke run: `write` held the runtime pictures twice).
    #[test]
    fn the_pages_systems_have_no_conflicting_parameters() {
        let mut world = World::new();
        let mut schedule = Schedule::default();
        schedule.add_systems((feed, write, settle, give_outline, crate::pipboy::data::prepare, crate::pipboy::data::draw, crate::pipboy::data::trace_over_tiles, crate::pipboy::data::popup_over_tiles, crate::pipboy::data::wave_moves).chain());
        schedule.add_systems((super::super::draw::draw, crate::pipboy::frame::draw));
        schedule.initialize(&mut world).expect("the schedule builds");
    }

    #[test]
    fn the_cut_aid_ends_only_with_the_switch_on() {
        let on = TileMenus { map: true, ..Default::default() };
        assert!(cut_over(on, Some(20.0), 20.0));
        assert!(!cut_over(on, Some(20.0), 19.0));
        assert!(!cut_over(on, None, 99.0));
        assert!(!cut_over(TileMenus::default(), Some(20.0), 99.0), "off: the menu is not built again");
    }

    #[test]
    fn every_data_tab_with_the_switch_on_is_eligible() {
        let mut pip = PipBoy::default();
        (pip.phase, pip.page) = (crate::pipboy::Phase::Up, Page::Data);
        pip.tabs[Page::Data.index()] = data::QUESTS;
        let on = TileMenus { map: true, ..Default::default() };
        assert!(eligible(on, &pip));
        assert!(!eligible(TileMenus::default(), &pip), "off: nothing runs");
        assert!(!eligible(TileMenus { stats: true, inventory: true, ..Default::default() }, &pip));
        pip.tabs[Page::Data.index()] = data::NOTES;
        assert!(eligible(on, &pip), "Misc is drawn from the file too");
        pip.tabs[Page::Data.index()] = data::RADIO;
        assert!(eligible(on, &pip), "and Radio");
        pip.tabs[Page::Data.index()] = data::WORLD;
        assert!(eligible(on, &pip), "and the World Map");
        pip.tabs[Page::Data.index()] = data::LOCAL;
        assert!(eligible(on, &pip), "and the Local Map");
        pip.tabs[Page::Data.index()] = data::QUESTS;
        pip.page = Page::Items;
        assert!(!eligible(on, &pip));
        pip.page = Page::Data;
        pip.phase = crate::pipboy::Phase::Down;
        assert!(!eligible(on, &pip), "not shown");
    }

    #[test]
    fn the_prepared_page_is_read_as_the_tile_page_writes_it() {
        let hint = |kind, word: &'static str, dim| data::Button { hint: Hint::new(word, "[K]".into(), dim), kind };
        let row = |text: &str, mark, done| data::Row { text: text.into(), mark, done };
        let view = data::View {
            tab: data::QUESTS,
            place: "A Place".into(),
            when: "now".into(),
            buttons: vec![hint(ButtonKind::MakeActive, "Pick", true), hint(ButtonKind::ShowLocation, "Look", false)],
            body: Body::List { rows: vec![row("Q1", Mark::Filled, false), row("Q2", Mark::None, true)], first: 0, boxed: Some(1), right: Right::Objectives { rows: vec![row("S1", Mark::Empty, false)], first: 12.0 } },
            popup: None,
        };
        let p = Prepared { view: view.clone(), keys: vec![7, 9], sel: 5, pointer_pick: true, navigated: 3, challenges: false, filtered: true };
        let s = Shown::of(&p, false).unwrap();
        assert_eq!(s.rows, vec![RowShown { key: 7, text: "Q1".into(), filled: true, done: false }, RowShown { key: 9, text: "Q2".into(), filled: false, done: true }]);
        assert_eq!((s.selected, s.boxed, s.pointer_pick, s.navigated, s.pane_first, s.device), (Some(1), true, true, 3, 12.0, false));
        assert_eq!(s.pane.objectives().len(), 1);
        assert!(!s.pane.objectives()[0].filled);
        assert_eq!((s.list, s.group), (ListKind::Quests, 0), "the notes' filter is Misc's");
        assert_eq!(s.hints.iter().map(|h| (h.slot, h.dim)).collect::<Vec<_>>(), vec![(Slot::A, true), (Slot::X, false)]);
        // A question box up is drawn over the page (`data::popup_over_tiles`): the page is read as
        // without it. Another body or rows without their records: not drawn from the file.
        let popup = Prepared { view: data::View { popup: Some(crate::pipboy::data::travel::Popup::travel(1, "Go?".into())), ..view.clone() }, ..p.clone() };
        assert_eq!(Shown::of(&popup, true), Shown::of(&p, true));
        let short = Prepared { keys: vec![7], ..p.clone() };
        assert!(Shown::of(&short, true).is_err());
        let text = Prepared { view: data::View { body: Body::List { rows: Vec::new(), first: 0, boxed: None, right: Right::Text { text: "x".into(), first: 0.0 } }, ..view }, keys: Vec::new(), ..p };
        assert_eq!(Shown::of(&text, true), Err("the right pane is not the tab's".to_string()));
    }
    #[test]
    fn native_data_button_release_keeps_its_original_selected_record_and_action() {
        use bevy::{prelude::*, input::{InputPlugin, touch::{TouchInput, TouchPhase}}};
        use crate::pipboy::{self, touch_items::TouchItems};
        #[derive(Resource, Default)] struct Seen(Vec<Act>);
        #[derive(Resource)] struct Repaint(Option<bool>);
        fn project(mut touch: ResMut<TouchItems>) { touch.project(Some); }
        fn change(mut change: ResMut<Repaint>, touch: Res<TouchItems>, mut script: ResMut<ScriptState>, mut tiles: ResMut<MapTiles>) {
            if touch.pointer_frame().tap {
                if let Some(kind_change) = change.0.take() {
                    let mut drawing = tiles.written.shown.as_ref().unwrap().clone();
                    if kind_change { drawing.hints[0].kind = ButtonKind::NotesFilter; } else { drawing.selected = Some(1); }
                    let menu = run(&mut script.rt.as_mut().unwrap().game.ui_tree, Some(drawing), &mut tiles).unwrap(); tiles.shown = Some(menu);
                }
            }
        }
        fn read(mut data: ResMut<DataState>, mut seen: ResMut<Seen>) { seen.0.extend(data.take_touch_test_actions().into_iter().filter(|a| matches!(a, Act::Button(_)))); }
        for kind_change in [false, true] {
            let (mut ui, mut tiles) = (tree(), MapTiles::default()); let menu = run(&mut ui, Some(shown(2,0,2)), &mut tiles).unwrap(); tiles.shown = Some(menu);
            let hits = fnv_ui::input::hit_candidates(&ui, menu, &fnv_ui::input::no_measure);
            let region = hits.iter().find(|h| matches!(data_tile(&ui, &tiles.written, h.under), Some(DataTile::Button(Slot::X)))).unwrap().region();
            let point = Vec2::new((region.left+region.width*0.5) as f32, (region.top+region.height*0.5) as f32);
            let button = fnv_ui::input::hit(&ui, &[menu], (point.x as f64, point.y as f64), &fnv_ui::input::no_measure).unwrap().tile;
            let mut script = ScriptState::over_no_plugins(); { let game = &mut script.rt.as_mut().unwrap().game; game.ui_files = Some(Box::new(MemSource::default())); game.ui_tree = ui; game.menu = Some(fnv_game::game::menus::MAP); }
            let mut pip = PipBoy::default(); pip.phase = pipboy::Phase::Up; pip.page = Page::Data; pip.tabs[Page::Data.index()] = data::QUESTS;
            let mut app = App::new(); app.add_plugins((MinimalPlugins, InputPlugin))
                .init_resource::<TouchItems>().init_resource::<DataState>().init_resource::<pipboy::PipBoyAid>().init_resource::<pipboy::PointerMoved>()
                .init_resource::<Seen>().insert_resource(Repaint(Some(kind_change)))
                .add_message::<PipBoyInput>().add_message::<TabHovered>()
                .insert_resource(script).insert_resource(tiles).insert_resource(pip)
                .insert_resource(TileMenus { map: true, ..Default::default() });
            let camera = app.world_mut().spawn_empty().id();
            app.insert_resource(PipBoyScreen { image: Handle::default(), camera, on_model: false, pointer: None, scale: 1. })
                .add_plugins(pipboy::touch_scroll::TouchPointerPlugin)
                .add_systems(Update, (change, feed, read).chain()).add_systems(Last, project);
            app.update();
            let send = |app: &mut App, id, phase| { assert!(app.world_mut().resource_mut::<TouchItems>().event(&TouchInput { window: Entity::PLACEHOLDER, id, phase, position: point, force: None }, 100)); };
            send(&mut app, 203, TouchPhase::Started); app.update(); app.update();
            send(&mut app, 203, TouchPhase::Ended); for _ in 0..4 { app.update(); }
            assert!(app.world().resource::<Repaint>().0.is_none()); assert!(app.world().resource::<Seen>().0.is_empty());
            let ui = &app.world().resource::<ScriptState>().rt.as_ref().unwrap().game.ui_tree;
            assert_eq!(fnv_ui::input::hit(ui, &[menu], (point.x as f64,point.y as f64), &fnv_ui::input::no_measure).unwrap().tile, button, "the same native button tile survives the repaint");
            send(&mut app, 204, TouchPhase::Started); send(&mut app, 204, TouchPhase::Ended); for _ in 0..4 { app.update(); }
            assert_eq!(app.world().resource::<Seen>().0, vec![Act::Button(if kind_change {ButtonKind::NotesFilter} else {ButtonKind::ShowLocation})]);
        }
    }

    include!("map_menu/touch_map_tests.rs");

    #[test]
    fn hotrect_scrollbar_and_nested_objectives_list_stay_on_tiles() {
        // Synthetic restructured list_box/scrollbar_vert where `lb_scrollbar` and
        // `scrollbar_vert_marker` are `<hotrect>` containers instead of `<image>` tiles.
        let hotrect_bars = |x: String| {
            x.replace(r#"<image name="lb_scrollbar">"#, r#"<hotrect name="lb_scrollbar">"#)
                .replace(r#"<image name="scrollbar_vert_down"/></image>"#, r#"<image name="scrollbar_vert_down"/></hotrect>"#)
                .replace(
                    r#"<image name="scrollbar_vert_marker"><y>16</y><width>20</width><height>10</height><_step_height>2</_step_height></image>"#,
                    r#"<hotrect name="scrollbar_vert_marker"><y>16</y><width>20</width><height>10</height><_step_height>2</_step_height><image name="ThumbFill"><width>6</width><height>10</height></image></hotrect>"#,
                )
                .replace(
                    r#"<image name="scrollbar_vert_marker"><y>16</y><width>20</width><height>10</height><_step_height>2</_step_height>
                <visible><copy src="parent()" trait="_number_of_items"/><gt>1</gt></visible></image>"#,
                    r#"<hotrect name="scrollbar_vert_marker"><y>16</y><width>20</width><height>10</height><_step_height>2</_step_height>
                <visible><copy src="parent()" trait="_number_of_items"/><gt>1</gt></visible><image name="ThumbFill"><width>6</width><height>10</height></image></hotrect>"#,
                )
        };
        let (mut ui, mut t) = (tree_changed(hotrect_bars), MapTiles::default());
        let menu = run(&mut ui, Some(shown(3, 0, 7)), &mut t).expect("Quests tab with hotrect scrollbar must stay on tiles");
        let pane = t.written.counted.pane.expect("objectives clip window");
        assert_eq!((pane.min.x, pane.min.y, pane.width(), pane.height()), (490.0, 240.0, 380.0, 200.0));
        assert_eq!(t.written.objectives.len(), 7);
        // Dragging the `<hotrect>` objectives scrollbar marker still works cleanly.
        let marker = resolve_tile(&ui, &format!("{OBJECTIVES}/{}/{}", list::SCROLLBAR, list::MARKER)).unwrap();
        let top = super::super::layout::tile_box(&ui, marker, &|_, _, _| (0.0, 0.0), true).unwrap().rect.top;
        let drag = pane_grab(&ui, menu, &Pane::Objectives(Vec::new()), top + 3.0).expect("hotrect marker grab");
        assert_eq!(list::value_under(&ui, drag, top + 33.0), Some(23.0));

        // Scrolling Misc text note with a `<hotrect>` scrollbar marker also stays on tiles.
        run(&mut ui, Some(misc(ListKind::Notes, 1, 0, Pane::Text(lines(9)))), &mut t)
            .expect("Misc scrolling text with hotrect scrollbar marker must stay on tiles");
        let text_marker = resolve_tile(&ui, &format!("{TEXT_BAR}/{}", list::MARKER)).unwrap();
        let text_top = super::super::layout::tile_box(&ui, text_marker, &|_, _, _| (0.0, 0.0), true).unwrap().rect.top;
        let text_drag = pane_grab(&ui, menu, &Pane::Text(lines(9)), text_top + 3.0).expect("hotrect text marker grab");
        assert_eq!(list::value_under(&ui, text_drag, text_top + 43.0), Some(28.0));

        // Wrapping `MM_QuestObjectivesList` in a container `<rect>` under `MM_DataRect` stays on tiles.
        let nested_objectives = |x: String| {
            hotrect_bars(x)
                .replace(
                    r#"<hotrect name="MM_QuestObjectivesList">"#,
                    r#"<rect name="ObjectivesWrap"><hotrect name="MM_QuestObjectivesList">"#,
                )
                .replace(
                    r#"<hotrect name="MM_DataTextRect">"#,
                    r#"</rect><hotrect name="MM_DataTextRect">"#,
                )
        };
        let (mut ui, mut t) = (tree_changed(nested_objectives), MapTiles::default());
        run(&mut ui, Some(shown(2, 0, 2)), &mut t).expect("nested objectives list must stay on tiles");
        assert_eq!(t.written.objectives.len(), 2);
        let obj = resolve_tile(&ui, OBJECTIVES).unwrap();
        assert_eq!(ui.path_of(obj), "MapMenu/GLOW_BRANCH/MM_MainRect/MM_DataRect/ObjectivesWrap/MM_QuestObjectivesList");

        // Regression guards: a `<hotrect>` scrollbar without a clipwindow or off-canvas still falls back.
        let no_clip = |x: String| {
            hotrect_bars(x).replace(
                r#"<hotrect name="MM_QuestObjectivesList"><width>380</width><height>200</height><clipwindow>&true;</clipwindow>"#,
                r#"<hotrect name="MM_QuestObjectivesList"><width>380</width><height>200</height>"#,
            )
        };
        assert_eq!(
            run(&mut tree_changed(no_clip), Some(shown(2, 0, 1)), &mut MapTiles::default()),
            Err("the objectives have no window".to_string())
        );
        let off_canvas = |x: String| hotrect_bars(x).replace(r#"<rect name="MM_DataRect"><x>450</x><y>200</y>"#, r#"<rect name="MM_DataRect"><x>450</x><y>700</y>"#);
        assert_eq!(
            run(&mut tree_changed(off_canvas), Some(shown(2, 0, 1)), &mut MapTiles::default()),
            Err("the objectives' window does not fit".to_string())
        );
    }

    #[test]
    fn the_world_map_draws_companion_names_in_the_tile_tree_clip_window() {
        let (mut ui, mut t) = (tree_changed(with_world), MapTiles::default());
        let mut s = world(1.0, [0.5, 0.5], vec![marker(10, [0.25, 0.5], false)]);
        {
            let w = s.world.as_mut().unwrap();
            w.companions = vec![(0x101, [0.25, 0.5], "Scout Alpha".into(), false), (0x102, [0.75, 0.5], "Scout Bravo".into(), true)];
            w.companion_font = 2;
        }
        run(&mut ui, Some(s.clone()), &mut t).unwrap();
        let clip = ui.find_tile(WORLD_CLIP).unwrap();
        let cursor = ui.find_tile(WORLD_CURSOR).unwrap();
        let comp_pieces: Vec<&Piece> = t.pieces.iter().filter(|p| p.tile == clip && matches!(p.look, Look::Text { .. })).collect();
        assert_eq!(comp_pieces.len(), 2, "both companions emitted as text pieces on the tiled World Map");
        assert_eq!(comp_pieces[0].look, Look::Text { string: "Scout Alpha".into(), font: 2, wrap: None, alignment: super::super::layout::Alignment::Center });
        assert_eq!(comp_pieces[1].look, Look::Text { string: "Scout Bravo".into(), font: 2, wrap: None, alignment: super::super::layout::Alignment::Center });
        assert_eq!(comp_pieces[1].colour, [1.0, 0.25, 0.2, 1.0], "knocked-out companion drawn in red");
        assert_ne!(comp_pieces[0].colour, [1.0, 0.25, 0.2, 1.0], "conscious companion uses Pip-Boy colour");
        let clip_rect = comp_pieces[0].clip.expect("companion text clipped to World Map window");
        assert_eq!((clip_rect.left, clip_rect.top, clip_rect.width, clip_rect.height), (40.0, 90.0, 855.0, 500.0));
        // Positioned by the viewport, centred above its map point.
        let view = s.world.as_ref().unwrap().view;
        let expected_at = view.to_window([0.25, 0.5]);
        assert!((comp_pieces[0].rect.left - f64::from(40.0 + expected_at.x)).abs() < 1e-3);
        assert!((comp_pieces[0].rect.top - f64::from(90.0 + expected_at.y - data::COMPANION_NAME_RISE)).abs() < 1e-3);
        // Ordered under the player's arrow.
        let last_comp = t.pieces.iter().rposition(|p| p.tile == clip && matches!(p.look, Look::Text { .. })).unwrap();
        if let Some(cursor_pos) = t.pieces.iter().position(|p| p.tile == cursor) {
            assert!(last_comp < cursor_pos, "companion names stand under the player's arrow");
        }
        // Location-font tweak switches companion font to 1, and zoom moves the pieces with the viewport.
        let mut zoomed = world(2.0, [0.5, 0.5], vec![marker(10, [0.25, 0.5], false)]);
        {
            let w = zoomed.world.as_mut().unwrap();
            w.companions = vec![(0x101, [0.25, 0.5], "Scout Alpha".into(), false)];
            w.companion_font = 1;
        }
        run(&mut ui, Some(zoomed.clone()), &mut t).unwrap();
        let z_pieces: Vec<&Piece> = t.pieces.iter().filter(|p| p.tile == clip && matches!(p.look, Look::Text { .. })).collect();
        assert_eq!(z_pieces.len(), 1);
        assert_eq!(z_pieces[0].look, Look::Text { string: "Scout Alpha".into(), font: 1, wrap: None, alignment: super::super::layout::Alignment::Center });
        let z_at = zoomed.world.as_ref().unwrap().view.to_window([0.25, 0.5]);
        assert!((z_pieces[0].rect.left - f64::from(40.0 + z_at.x)).abs() < 1e-3);
        // Local Map emits no companion pieces.
        let (mut ui_local, mut t_local) = (tree_changed(with_local), MapTiles::default());
        run(&mut ui_local, Some(local(None, 1.0)), &mut t_local).unwrap();
        assert!(t_local.pieces.iter().all(|p| !matches!(&p.look, Look::Text { string, .. } if string == "Scout Alpha")));
    }

    #[test]
    fn centered_highlight_location_name_rect_with_clips_uses_highlight_clipwindow() {
        // Modded map_menu.xml (e.g. Vanilla UI Plus `VUI+\outline_rect.xml`) turns
        // `MM_HighlightLocationName` into a `<rect>` with `<clips>&true;</clips>`, `<locus>1</locus>`,
        // centered at `parent.width / 2`, containing centered `<text>` children (`original`, `VUI+Shadow`).
        let modded = |x: String| {
            with_world(x).replace(
                r#"<text name="MM_HighlightLocationName"><x>80</x><y>170</y><string><copy src="parent()" trait="_Title"/></string></text>"#,
                r#"<rect name="MM_HighlightLocationName"><locus>1</locus><clips>&true;</clips><justify>&center;</justify><x><copy src="parent()" trait="width"/><div>2</div></x><y>170</y><width><copy src="child(original)" trait="width"/></width><height><copy src="child(original)" trait="height"/></height><string><copy src="parent()" trait="_Title"/></string>
                    <text name="original"><string><copy src="parent()" trait="string"/></string><justify>&center;</justify><clips>&true;</clips></text>
                    <text name="VUI+Shadow"><x>1.5</x><y>1.5</y><string><copy src="sibling(original)" trait="string"/></string><justify>&center;</justify><clips>&true;</clips></text>
                  </rect>"#,
            )
        };
        let (mut ui, mut t) = (tree_changed(modded), MapTiles::default());
        let mut place = marker(10, [0.5, 0.5], true);
        place.name = "Trading Shoppe".into();
        let s = world(1.0, [0.5, 0.5], vec![place]);
        run(&mut ui, Some(s), &mut t).unwrap();
        let orig = ui.find_tile("MapMenu/GLOW_BRANCH/MM_MainRect/MM_Highlight_ClipWindow/MM_MapHighlightBox/MM_HighlightLocationName/original").unwrap();
        let shadow = ui.find_tile("MapMenu/GLOW_BRANCH/MM_MainRect/MM_Highlight_ClipWindow/MM_MapHighlightBox/MM_HighlightLocationName/VUI+Shadow").unwrap();
        let orig_piece = t.pieces.iter().find(|p| p.tile == orig).expect("original location label piece");
        let shadow_piece = t.pieces.iter().find(|p| p.tile == shadow).expect("shadow location label piece");
        let clip = orig_piece.clip.expect("label stays clipped to MM_Highlight_ClipWindow");
        assert_eq!((clip.left, clip.top, clip.width, clip.height), (40.0, 90.0, 855.0, 500.0));
        assert_eq!(shadow_piece.clip, Some(clip));
        // Centered text starts half its width to the left of the highlight center (40 + 427.5 = 467.5),
        // so the entire left half of the place name sits well inside MM_Highlight_ClipWindow.
        let center_x = 40.0 + 855.0 / 2.0;
        assert!(orig_piece.rect.width > 100.0, "expected measured text width, got {}", orig_piece.rect.width);
        assert!(orig_piece.rect.left < center_x - 50.0, "left half should extend left of center: left={}, center={}", orig_piece.rect.left, center_x);
        assert!(orig_piece.rect.left >= clip.left && orig_piece.rect.left + orig_piece.rect.width <= clip.left + clip.width);
    }
}
