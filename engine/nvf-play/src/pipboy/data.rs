//! The DATA page (docs/spec-notes/pipboy.md §5.3): Local Map, World Map, Quests, Misc
//! (notes) and Radio, laid out as `menus\main\map_menu.xml`.
//!
//! - **Heading**: the place the player is in and the game's date and time
//!   ([`model::date_time`], [`read::place_name`]).
//! - **Quests**: the quests with an objective shown, running ones first, newest on top
//!   ([`model::sort_quests`]); the active quest's square filled, those done dimmed; the
//!   selected quest's objectives at the right, each behind a square, filled when done.
//!   A click or Enter makes a quest the active one.
//! - **Misc**: the player's notes (AddNote) by name and the selected note's text; an
//!   image note's picture; an audio note's time remaining and
//!   meter, its marker moving while that note plays (at the right end, nothing left,
//!   for a note not playing). Using an audio note plays it from its start, using the
//!   one playing pauses it and goes on (`crate::holotape`). The notes' filter (X,
//!   `sShowActiveNotes`) keeps the notes of the active quest.
//! - **Radio**: the stations the Pip-Boy receives here (`crate::radio`'s list), the
//!   playing one's square filled; a click or Enter tunes to a station or, on the playing
//!   one, turns the radio off; the waveform panel at the right (a trace that runs to the
//!   left and rises with the sound while it plays, the flat line while it is off;
//!   [`wave_moves`]).
//! - **World Map**: the worldspace's map picture with the map markers the player knows
//!   (found within their radius, made visible by ShowMap or by their flag), the active
//!   quest's markers and the player's arrow; dragged to pan, the wheel zooms; the
//!   highlight box follows the pointer and takes a marker's name when over one. A click
//!   on a marker — or the controller's A on one — asks whether to fast travel there, in
//!   a box over the map ([`travel`],
//!   docs/spec-notes/fast-travel.md).
//!   The picture's place, the pointer's window and the controller's cursor come from one
//!   viewport ([`view::MapView`]) that the page drawn from `map_menu.xml` paints from too
//!   (`crate::tiles::map_menu`, which publishes its window as laid out, [`DataState::publish_map`]);
//!   over that page the question boxes are painted by [`popup_over_tiles`]. The Local Map
//!   reads the same viewport, with its own projection ([`view::LocalProjection`]).
//! - **Local Map**: the doors leading elsewhere, the known map markers and the quest
//!   markers around the player, with the player's arrow, over an outline of the place
//!   made from the physics world's collision ([`outline`]; the original's picture of the
//!   place from above is not drawn). Indoors the map is drawn closer
//!   ([`outline::INTERIOR_SCALE`]).
//! - **Place Marker** (#1954, [`marker`]): the maps' X button (X, the controller's X, a
//!   click on its hint; a click on a map away from any marker, #3324) asks, in the same
//!   question box, whether to set the player's one custom marker at the highlight box,
//!   or to move, remove or keep the one placed. Both
//!   maps show it (the game keeps it, `fnv_game::Game::custom_map_marker`, so scripts'
//!   SetCustomMapMarker shows too, and it is saved), and the World Map draws dotted lines
//!   from the player's arrow to it and to the active quest's targets.
//! - Both maps pan while the left button is held on them and the pointer moves (the
//!   pointer's own movement, not Bevy's drag: the map's nodes are drawn again as the
//!   highlight box follows the pointer, which ended a drag at once).
//! - On a controller the right stick moves the highlight box — pushed past an edge it
//!   pans the map ([`scroll_map`]) — and the left stick's up and down zooms in and out
//!   smoothly while held ([`stick_cursor`], [`stick_zoom`]).
//! - The right pane's objectives (Quests) and note text (Misc) scroll as one
//!   ([`scroll_pane`]): Page Up/Down (a controller's LB/RB) a window, the wheel over the
//!   pane, and on a controller the right stick smoothly while held; a new quest or note
//!   starts at the top. Their scroll bars take the pointer ([`bar_spots`]): an arrow
//!   steps, the track above or below the marker pages, the marker drags.
//!
//! Selection: Up/Down (W/S) and the pointer (the row under it is selected and boxed);
//! the test aid's `key=down|up|use` steps (`crate::menu::MenuInput`) do the same here.
//! Everything the page shows is gathered into a [`View`] each frame and drawn again only
//! when it changed. The page's state is [`DataState`], made on first use (the Pip-Boy's
//! plugin knows only [`draw`]).

pub mod marker;
pub mod touch_map;
pub(crate) mod model;
pub(crate) mod outline;
pub(crate) mod read;
pub mod travel;
pub mod view;

use super::frame::{Align, Ctx, Hint, Kit, FADE, FILL, LINE, MAIN, THICK};
use super::{Page, PageBody, PipBoy, PipBoyAid, PipBoyInput, PipBoyScreen};
use crate::interface::{pipboy_amber as amber, at, u};
use crate::menu::model::Command;
use crate::menu::MenuInput;
use crate::player::Player;
use crate::radio::{Radio, RadioPlaces};
use crate::script::ScriptState;
use crate::sim::SimState;
use crate::bindings::{Bindings, InputDevice};
use crate::{Location, PlayWorld};
use bevy::ecs::system::SystemParam;
use bevy::image::CompressedImageFormatSupport;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use fnv_game::quest_targets::{DoorGraph, Place as QuestPlace};
use fnv_sim::FormId;
use marker::{CustomMarker, LineTo, Piece};
use model::{ObjectiveLine, QuestKey, WorldMap};
use outline::Outline;
use travel::{Ask, FastTravelAsk, FastTravelGo, Popup};
pub(crate) use view::MapView;
use nvf_assets::GameData;
use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::Arc;

// ---- the tabs ---------------------------------------------------------------------------

pub(crate) const LOCAL: usize = 0;
pub(crate) const WORLD: usize = 1;
pub(crate) const QUESTS: usize = 2;
pub(crate) const NOTES: usize = 3;
pub(crate) const RADIO: usize = 4;

// ---- the menu file's places, inside the menus' rectangle [documented: map_menu.xml] ------

/// Heading: the title 70 in after a line of 50; a line from 180, 470 long; the place
/// right-aligned at 630 and the time at 845, both 20 down in font 2; lines fading down at
/// 650 and 855, a line from 660 to the end.
const LEAD_LINE: f32 = 50.0;
const LINE2: (f32, f32) = (180.0, 470.0);
const PLACE_X: f32 = 630.0;
const SPLIT_X: f32 = 650.0;
const LINE3_X: f32 = 660.0;
const TIME_X: f32 = 845.0;
const HEAD_TEXT_Y: f32 = 20.0;
/// The maps' window: 50 down, 855 × 500; the picture's point shown at its middle.
const MAP: Rect = Rect { min: Vec2::new(0.0, 50.0), max: Vec2::new(855.0, 550.0) };
const MAP_MID: Vec2 = Vec2::new(427.5, 250.0);
/// The highlight box: 160 square, its corners 40-long arms with 15-long fading tips;
/// the name in font 1 centred 80 in and 180 down; a dot of 8 at its middle.
const HBOX: f32 = 160.0;
const HBOX_ARM: f32 = 40.0;
const HBOX_TIP: f32 = 15.0;
const HBOX_NAME_Y: f32 = 180.0;
const DOT: f32 = 8.0;
/// A marker is drawn this much larger under the highlight box, and brightness 220
/// (255 under it).
const MARKER_GROW: f32 = 1.75;
const MARKER_BRIGHTNESS: (f32, f32) = (220.0, 255.0);
/// The map picture: alpha 128 at brightness 255.
pub(crate) const MAP_ALPHA: f32 = 128.0;
/// The player's arrow: `glow_cursor.dds`, 1.28 times as high as wide.
const CURSOR_FILE: &str = "Interface\\Icons\\Misc\\glow_cursor.dds";
const CURSOR_ASPECT: f32 = 1.28;
/// Lists: 15 left of the menus, 75 down, 400 × 468; a scroll bar of 32 on the left; a
/// row from the scroll bar to 5 short of the list's right edge, its text 40 in and 20
/// down in font 2 broken at 310, its square of 25 at 15, 20.
const LIST: Rect = Rect { min: Vec2::new(-15.0, 75.0), max: Vec2::new(385.0, 543.0) };
const LIST_W: f32 = 400.0;
const SCROLLBAR: f32 = 32.0;
const ARROW_H: f32 = 16.0;
const ROW_SHORT: f32 = 5.0;
const ROW_TEXT: Vec2 = Vec2::new(40.0, 20.0);
const MARK_AT: Vec2 = Vec2::new(15.0, 20.0);
const MARK: f32 = 25.0;
const WRAP: f32 = 310.0;
/// The hints at the right (`MM_ButtonRect`): right-aligned at 855, the first from 75 down
/// (the lists' top), each a box of its text with 20 across and 15 up and down, one under
/// the other where the one above is shown [documented]. Drawn as the ITEMS page's column
/// (`frame::Ctx::hint_column`): each word ends at 855, the heading's right end, its key
/// after it, the rows `frame::HINT_PITCH` (46) apart, the text `frame::BUTTON_TEXT_Y`
/// under the row's top [observed: the original's screenshots on issue #1897, 1920 ×
/// 1080: the words end within 3 px of the heading's right end, rows 43 px apart, the
/// same as the ITEMS column's] ([`button_boxes`], #1897).
const BUTTONS: Vec2 = Vec2::new(855.0, 75.0);
const BUTTON_HORBUF: f32 = 20.0;
const BUTTON_VERBUF: f32 = 15.0;
/// The item data at the right: 400 wide at the menus' right edge, under the hints' rect
/// (75 + 120) when a hint shows, a button's height higher on the tabs up to Misc while
/// the A hint does not show (`show`); a note's text 17 left of it and 20 down,
/// 400 × 385, in font 1, scrolled 20 a step; an image note's picture 360 square at its
/// right; the objectives 400 × 400 with the scroll bar on the right.
const DATA_X: f32 = 455.0;
const DATA_W: f32 = 400.0;
const BUTTON_RECT_H: f32 = 120.0;
const TEXT_AT: Vec2 = Vec2::new(-17.0, 20.0);
const TEXT_H: f32 = 385.0;
const TEXT_STEP: f32 = 20.0;
const TEXT_PAD: f32 = 10.0;
const IMAGE: f32 = 360.0;
const OBJECTIVES_H: f32 = 400.0;
/// The radio's waveform panel: 500, 125, 350 square; the tick meters 32 deep along its
/// foot and right side (2 past them); the flat line 2 high, 5 short of the width, 8
/// over the middle.
const WAVE: Rect = Rect { min: Vec2::new(500.0, 125.0), max: Vec2::new(850.0, 475.0) };
const TICKS: f32 = 32.0;

// ---- NVForever's own [design or hypothesis, pipboy.md §5.3] -----------------------------

/// A row is 54 high and its text's further lines 34 below the first [observed: fitted to
/// the owner's Quests and Radio screenshots, rows 33.5 px apart and two-line rows 54.5,
/// at 0.62 px a unit].
const ROW_H: f32 = 54.0;
const ROW_PITCH: f32 = 34.0;
/// The selection box starts 9 left of the row [observed: the owner's Radio screenshot
/// boxes "Mojave Music Radio" from 448 px, the rows' squares start at 456 px as ours].
const BOX_LEFT: f32 = 9.0;
/// A done quest's or objective's text and square at half strength [hypothesis: the
/// owner's screenshot shows the completed quest greyed].
const DONE_ALPHA: f32 = 0.5;
/// Map marker, player arrow and quest marker widths ("set in code" in the menu file)
/// [observed: fitted to the owner's World Map screenshot, the marker about 13 px, the
/// arrow's triangle 25 px and the quest marker's 12 × 23 px, at 0.62 px a unit; the
/// pictures fill part of their squares].
pub(crate) const MARKER: f32 = 24.0;
pub(crate) const CURSOR: f32 = 40.0;
pub(crate) const QUEST_MARKER: f32 = 56.0;
/// The quest marker: the HUD compass's objective marker [hypothesis: the shape on the
/// owner's screenshots]; 1.25 times as high as wide, set 15 right and 5 down "to match
/// the marker textures" [documented: map_menu.xml quest marker templates].
pub(crate) const QUEST_FILE: &str = "glow_hud_compass_objective_marker.dds";
const QUEST_ASPECT: f32 = 1.25;
const QUEST_NUDGE: Vec2 = Vec2::new(15.0, 5.0);
/// The pointer takes the highlight box to a marker this near [hypothesis].
const SNAP: f32 = 40.0;
/// A press and release on the World Map is a click (it asks to travel) while the pointer
/// moved at most this far between them [design].
const CLICK_SLOP: f32 = 6.0;
/// The fast-travel question box (fast-travel.md §3) [design: by the owner's screenshot]:
/// its width, the room round its text, its text's wrap, the room under an answer's text
/// and the answer's frame round the widest answer (the message box's 42, message-menu.md
/// §3).
const POPUP_W: f32 = 520.0;
const POPUP_PAD: f32 = 30.0;
const POPUP_ROW_PAD: f32 = 20.0;
const POPUP_FRAME_PAD: f32 = 42.0;
/// The local map: game units to one canvas unit at a magnification of 1 [observed:
/// fitted to the owner's Local Map screenshot, where the gas station's door (-75242, 4076)
/// and the quest marker at the saloon's door (-67714, 3620) stand 168 px apart; the
/// original's zoom at the time is not known].
const LOCAL_SCALE: f32 = 28.0;
/// The local map reads the doors of the cells this many across and down from the one at
/// its middle (enough for the window at a magnification of 1).
const LOCAL_CELLS: (i32, i32) = (3, 2);
/// How strong the Local Map Shows Reset Areas tweak's red is [design].
const RESET_CELL_ALPHA: f32 = 0.18;
/// Each wheel step zooms a map by this factor, between 1 and 4 times [design].
const ZOOM_STEP: f32 = 1.25;
const ZOOM: (f32, f32) = (1.0, 4.0);
/// The controller's right stick moves the map cursor this fast at a full push, canvas
/// units a second (the window is 855 wide) [design].
const CURSOR_SPEED: f32 = 600.0;
/// The controller's left stick zooms a map this fast at a full push, wheel steps a
/// second (the wheel's 1.25 a step, from 1 to 4 times) [design: tuned by playtest to
/// match the original's zoom feel, observed].
const ZOOM_STICK_STEPS: f32 = 4.5;
/// The controller's right stick scrolls the right pane (a note's text, a quest's
/// objectives) this fast at a full push, canvas units a second [design: one note window
/// (385) a second, so a full push goes as far in a second as one LB/RB (Page Down) press
/// and a light push reads a line at a time; that the original scrolls the pane with the
/// right stick is observed (issue #1667), its speed was not measured].
const PANE_STICK_SPEED: f32 = TEXT_H;
/// The trace drawn while the radio plays: its greatest height either side of the middle
/// [observed: the capture, about a third of the panel at its loudest], the waves across
/// the panel [observed: 8], how fast its envelope runs to the left, panels a second
/// [observed: 7; the waves themselves stand nearly still], how fast the waves drift
/// [design], the loudness (root mean square) drawn at full height [design: the radio's
/// songs measure about 0.025 through, `mus_roundhouse_rock.mp3` -32 dB] and the
/// straight pieces it is drawn of.
const WAVE_AMPLITUDE: f32 = 110.0;
const WAVE_CYCLES: f32 = 8.0;
const WAVE_SCROLL: f32 = 7.0;
const WAVE_DRIFT: f32 = 0.4;
const WAVE_LOUD: f32 = 0.04;
const WAVE_SEGMENTS: usize = 128;
/// The envelope's rise and fall: a knot every this many pieces, each at a height between
/// these shares of the loudness [design: the capture's bursts and lulls].
const WAVE_KNOT: u64 = 10;
const WAVE_SWING: (f32, f32) = (0.3, 1.0);
/// An audio note's meter (`MM_DataAudioInfo` of map_menu.xml) [documented]: the time
/// remaining right-aligned at the top, under it the tick meter (32 high, its top 2 cut),
/// and the marker (8 wide, 32 high, a quarter of its height below the meter's top) at
/// the share played of the meter's width less its own.
const AUDIO_METER: f32 = 32.0;
const AUDIO_METER_CROP: f32 = 2.0;
const AUDIO_MARKER: f32 = 8.0;
/// How often the page looks at the quests and the markers while the game runs, seconds.
const POLL: f32 = 0.25;

/// The custom marker's picture, drawn as a quest marker is (its size, place and the Local
/// Map's clamp) one depth above them [observed: the original's MapMenu, 2026-11-10: a
/// tile `glow_hud_compass_pc_marker.dds` at depth 5, as wide as the quest markers on both
/// maps (50.59 on the World Map, 75 on the Local Map); hypothesis: the quest markers'
/// template].
pub(crate) const PC_MARKER_FILE: &str = "glow_hud_compass_pc_marker.dds";
/// The World Map's waypoint nodes and the dotted line's picture [documented: map_menu.xml
/// `WaypointTemplate`, `PathLineTemplate`].
const NODE_FILE: &str = "Interface\\Worldmap\\waypoint_node.dds";
const PATH_FILE: &str = "Interface\\Worldmap\\waypoint_path.dds";
/// A node is 0.8 of the map markers' width, the line a third of the node's height
/// [documented: `WaypointTemplate` width `user0` × 0.8; `PathLineTemplate` height; observed:
/// 17.41 to the markers' 21.76 in the capture].
const NODE: f32 = MARKER * 0.8;
const PATH_H: f32 = NODE / 3.0;
/// One repeat of the line's picture along it: the file's 64 × 16 kept in shape at the
/// line's height [hypothesis: the template tiles the file (`tile`) at the menu's scale].
const PATH_TILE: f32 = PATH_H * 4.0;
const DOOR_ICON: &str = "interface\\icons\\local map\\icon_local_door.dds";
/// The doors' pictures under the player's Exterior and Interior Door Icons tweak: the
/// files Stewie's Tweaks names (its readme), drawn when the load order has them; without
/// them the plain door, an interior one dimmer [design: docs/design/tweaks.md].
const DOOR_ICON_OUTSIDE: &str = "interface\\icons\\local map\\iron_exterior_door.dds";
const DOOR_ICON_INSIDE: &str = "interface\\icons\\local map\\iron_interior_door.dds";
/// How bright an interior door is drawn, of a door's brightness, with the tweak on and its
/// own pictures missing [design].
const INSIDE_DOOR_DIM: f32 = 0.55;

// ---- state --------------------------------------------------------------------------------

/// What the player did on the page with the pointer, taken by [`draw`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Act {
    /// The pointer came onto a row of the tab's list.
    Hover(usize, usize),
    /// The pointer left the row.
    Leave(usize, usize),
    /// A row was clicked.
    Use(usize, usize),
    /// A button was clicked.
    Button(ButtonKind),
    /// The pointer came onto an answer of the question box, or clicked it.
    PopupOver(usize),
    PopupOut(usize),
    PopupPick(usize),
    /// A part of the right pane's scroll bar was clicked (the marker: pressed).
    Bar(BarPart),
    /// The page drawn from the file (`crate::tiles::map_menu`): the pointer came onto, left or
    /// clicked the row of a tab's list standing for record `.1` (a quest's FormID, [`row_keys`]),
    /// found in the list as it is when the act is taken, so a list a script changed meanwhile
    /// acts on that record or on none ([`by_record`]).
    HoverRecord(usize, u32),
    LeaveRecord(usize, u32),
    UseRecord(usize, u32),
    /// The page drawn from the file: its right pane's scroll bar's marker dragged to `.0`, by
    /// the file's own bar (its marker's `_step_height`): the pane's scroll there.
    PaneTo(f32),
}

/// A part of the right pane's scroll bar ([`bar_spots`]): an arrow steps it as a wheel
/// notch does, the track above or below the marker pages it as Page Up/Down does, the
/// marker held drags it (`.0`: the pane's units a unit of the bar) [design: as the
/// tile menus' bars, `crate::tiles::list`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum BarPart {
    Up,
    Down,
    PageUp,
    PageDown,
    Marker(f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ButtonKind {
    PlaceMarker,
    ShowLocation,
    NotesFilter,
    Challenges,
    /// X while the challenges show (`sNextChallenge`): not built.
    NextChallenge,
    /// The A button's actions (Enter): travel to the marker under the highlight box, make
    /// the selected quest the active one, play or stop the selected audio note, tune to
    /// the selected station (#1897).
    Travel,
    MakeActive,
    PlayAudio,
    Tune,
}

impl ButtonKind {
    /// The A button's (`MM_ButtonA`): what Enter does on the tab.
    pub(crate) fn is_a(self) -> bool {
        matches!(self, ButtonKind::Travel | ButtonKind::MakeActive | ButtonKind::PlayAudio | ButtonKind::Tune)
    }
}

/// A place name's cell: worldspace, interior, grid square.
type PlaceKey = (u32, Option<u32>, (i32, i32));
/// The local map's doors: interior, or the grid square at its middle.
type DoorKey = (Option<u32>, (i32, i32));
/// What the right pane shows: the tab, whether Misc shows the challenges, and the quest
/// or note (a challenge by its row).
type PaneKey = (usize, bool, u32);

/// What scrolls the right pane in a frame ([`scroll_pane`]): Page Down presses less Page
/// Up presses (LB/RB press those, and the scroll bar's track), wheel notches over the
/// pane and the scroll bar's arrow clicks (up positive), the right stick's up and down
/// (up positive) for `dt` seconds, and how far the scroll bar's marker was dragged (the
/// pane's units, down positive).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct PaneMoves {
    pages: f32,
    notches: f32,
    stick: f32,
    dt: f32,
    drag: f32,
}

/// A marked target's doors on the maps: the World Map's door outdoors, the Local Map's
/// first door of the way (`None`: the target itself, or no way).
type TargetDoors = (Option<FormId>, Option<FormId>);

/// The DATA page's state.
#[derive(Resource, Default)]
pub struct DataState {
    /// Each tab's selected row and first row shown, and how many rows fit when last drawn.
    sel: [usize; 5],
    first: [usize; 5],
    /// Direct scrollbar view movement, independent of the selected gameplay row.
    manual_list: Option<(usize, usize)>,
    fit: [usize; 5],
    /// The selected row is boxed (the pointer is on it, or a key moved it).
    boxed: bool,
    /// How far the right pane (a note's text, a quest's objectives) is scrolled, and how
    /// far it can be ([`scroll_pane`]).
    text_first: f32,
    text_max: f32,
    /// What the right pane showed when last scrolled (a new quest or note starts at its
    /// top), and whether the right stick was scrolling it (logged when let go).
    pane_of: Option<PaneKey>,
    pane_stick: bool,
    /// The objectives' window when last drawn (canvas; the wheel over it scrolls them).
    pane_rect: Option<Rect>,
    /// The scroll bar's marker held: the pointer's last height and the pane's units a
    /// unit of the bar ([`drag_bar`]).
    bar_drag: Option<(f32, f32)>,
    /// The active quest: the player's pick while it runs, else the newest running quest
    /// [design].
    active_quest: Option<u32>,
    /// How many ForceActiveQuest calls the page has taken (`fnv_game::functions::ui`).
    forced_seen: u64,
    /// When each quest was first seen listed (a running count).
    seen: HashMap<u32, u64>,
    seen_count: u64,
    /// A co-op guest's quest list as last logged (`track`).
    guest_listed: Vec<u32>,
    /// The notes' filter is on: only the active quest's notes (`sShowActiveNotes`).
    notes_of_active: bool,
    /// The note whose page was last shown, so it is marked read once (GetNoteRead).
    note_shown: Option<u32>,
    /// The Notes tab shows the challenges instead ("Challenges", R; `crate::challenges`).
    challenges: bool,
    /// The point shown at each map's middle (world: the picture's 0–1; local: game x, y)
    /// and its magnification.
    centre: [Option<[f32; 2]>; 2],
    magnify: [f32; 2],
    /// Where the highlight box is on each map (in the map's window).
    cursor: [Option<Vec2>; 2],
    /// Map markers the player has been within the radius of.
    discovered: BTreeSet<u32>,
    /// Markers found since `crate::compass` last took them (for "Place found").
    newly_found: Vec<u32>,
    /// Markers found since this run started, not saved (the Color Recently Added Map
    /// Markers tweak) [design: docs/design/tweaks.md].
    found_this_run: BTreeSet<u32>,
    /// Where the player was last outdoors: worldspace, feet, heading.
    outdoors: Option<(u32, [f32; 3], f32)>,
    // The plugins' records, read when first wanted.
    markers: Option<Vec<read::Marker>>,
    notes: HashMap<u32, Option<read::Note>>,
    maps: HashMap<u32, Option<WorldMap>>,
    places: HashMap<PlaceKey, String>,
    doors: HashMap<DoorKey, Vec<read::Door>>,
    /// What was drawn last.
    shown: Option<View>,
    /// What the page showed when last prepared ([`step`]), whatever draws it: the hints are
    /// logged when they change from it. Kept while the page is closed.
    last_view: Option<View>,
    /// What the page shows, as last prepared while open: the hand-built body paints it with
    /// `MapMenu` on ([`prepare`]), the tile page writes it. Kept while the page goes down.
    prepared: Option<Prepared>,
    /// The pointer moved the selection last (a key: not), and the keys' moves so far: for the tile
    /// page's list, which brings a row a key picked into view ([`Prepared`]).
    pointer_pick: bool,
    navigated: u32,
    /// Wheel notches over the list while the tile page showed it, for that page's own scroll
    /// (`crate::tiles::map_menu`); the hand-built list's is `first`.
    list_notches: f32,
    /// The right pane's window as the page drawn from the file measured it (its height a page);
    /// none from the hand-built drawing, whose windows are the file's numbers ([`pane_page`]).
    pane_page: Option<f32>,
    was_open: bool,
    last_tab: Option<usize>,
    /// The doors outdoors into each interior a quest target is in (`Game::doors_into`).
    doors_into: HashMap<u32, Vec<FormId>>,
    /// The places joined by load doors, built once, for the ways to targets several doors
    /// away (`fnv_game::quest_targets::DoorGraph`, #2458).
    door_graph: Option<fnv_game::quest_targets::DoorGraph>,
    /// Each marked target check's (quest, objective, place in its list) door on the maps
    /// last logged: the world map's door outdoors and the local map's first door of the
    /// way (`None`: the target itself, or no way).
    target_doors_logged: HashMap<(u32, i32, usize), TargetDoors>,
    /// The references the World Map's quest markers stand for, last logged.
    world_quests_logged: Option<Vec<u32>>,
    /// The active quest's targets of the objectives shown and not done, with whether their
    /// conditions passed when last evaluated (`refresh_quest_targets`).
    target_checks: Vec<fnv_game::quest_targets::TargetCheck>,
    /// Seconds until the targets' conditions are evaluated again.
    target_poll: f32,
    /// The quest whose targets were last evaluated.
    target_quest: Option<FormId>,
    poll: f32,
    acts: Vec<Act>,
    drag: Vec2,
    /// Where the pointer was while the left button is held on a map (dragging it).
    drag_from: Option<Vec2>,
    /// The Local Map's outline of the place, as far as it is made.
    outline: Option<Outline>,
    /// The last outline finished of the same place and floor, shown while [`DataState::outline`]
    /// is made after a pan took the map off it (#3326: outdoors a grid takes several frames, and
    /// the map blinked empty each time a drag passed a quarter of it).
    outline_last: Option<Outline>,
    /// The landscape surfaces of the cells the outline read (by grid square; `None` for a
    /// square without land), smoothed or stored as the run draws and collides them
    /// (`crate::smooth_terrain`, #2149), kept while one outline is made.
    outline_land: HashMap<(i32, i32), Option<fnv_world::land_surface::Surface>>,
    /// How many markers the world map showed when last logged.
    world_logged: Option<(u32, usize, usize, usize, usize)>,
    /// The World Map's companion count at the last log.
    companions_logged: Option<usize>,
    /// The companions under the World Map's highlight box (the player's Map Selectable
    /// Companions tweak).
    companions_under: Vec<FormId>,
    /// The World Map's marker under the highlight box, when last drawn.
    cursor_marker: Option<u32>,
    /// The marker the Travel hint, Enter and the controller's A travel to
    /// ([`travel_target`], #1897).
    travel_marker: Option<u32>,
    /// The boxes of the buttons at the right as last drawn, and their tab
    /// ([`button_boxes`], [`on_button`]).
    button_rects: (usize, Vec<Rect>),
    /// Where the left button went down on a map, and how far the pointer has moved since
    /// (a click asks to travel or places the custom marker, a drag does neither).
    press: Option<(Vec2, f32)>,
    /// A click on map tab `.0` this frame ([`travel_ask`], #3324), resolved once the map is laid
    /// out for good against the highlight box as drawn then ([`take_click`]): companions
    /// summoned, a fast-travel question for the World Map's marker under it, else Place Marker's
    /// question of the place under it.
    map_click: Option<usize>,
    /// The question box over the map: fast travel's (`travel`) or Place Marker's.
    popup: Option<Popup>,
    /// The pointer on the page this frame, whatever the input device (the question box's
    /// answers take the pointer's hover with either).
    pointer_now: Option<Vec2>,
    /// The question box's pointer state: the answer under the pointer and where the
    /// pointer was when a key last moved the choice ([`BoxHold`], [`choose`]).
    popup_hold: BoxHold,
    /// The World Map's waypoints as last worked out, by what they depend on
    /// ([`cached_waypoints`]).
    waypoints_cache: Option<(WaypointKey, Waypoints)>,
    /// The pointer is in use this frame (the mouse's, not with Controller chosen): the Travel
    /// target is kept on the way to the hint only then ([`travel_target`]); a refit reads it as
    /// the preparation did ([`DataState::refit_map`]).
    aiming: bool,
    /// A fast-travel question asked this frame (a click on a marker, Enter, A): the marker it was
    /// asked of, kept as asked until it is sent once the map is drawn ([`flush_travel`]).
    travel_pending: Option<u32>,
    /// The map was drawn again after the question was asked (a refit, a fallback,
    /// [`DataState::follow_the_box`]): the question then asks of the Travel target as drawn last,
    /// or is not asked with none; only the last drawing decides, so a refit onto empty space
    /// followed by a fallback onto a marker still asks [design].
    travel_redrawn: bool,
    /// The preparation is the early one (`MapMenu` on, [`prepare`]): only then are the Local Map's
    /// rows, the marks' names and reputation lines for the page drawn from the file gathered
    /// ([`WorldParts::rows`]). With the switch off the hand-built page gathers none of them; it
    /// still works out each mark's texture coordinates, a little work it did not do before.
    tile_rows: bool,
    /// The map window as the page drawn from the file laid it out, and the tab it drew (canvas
    /// units, [`DataState::publish_map`]); none from the hand-built drawing, whose window is the
    /// file's numbers ([`map_window`]).
    map_window: Option<(usize, Rect)>,
    /// The player's custom map marker as the game holds it (`Game::custom_map_marker`),
    /// taken each frame; the page's own while no game runs (tests).
    custom: Option<CustomMarker>,
    /// The place under each map's highlight box when last drawn: where Place Marker puts
    /// the marker.
    spot: [Option<CustomMarker>; 2],
    /// Where each map drew the custom marker when last drawn (window units), for the No
    /// Place Marker Popup tweak's "on it".
    custom_on_map: [Option<Vec2>; 2],
    /// Where each map showed the custom marker when last logged (whole units; `None`:
    /// not shown).
    custom_logged: [Option<Option<(i32, i32)>>; 2],
    /// What the World Map's dotted lines led to, and the custom marker, when last logged.
    lines_logged: Option<(Vec<LineTo>, Option<CustomMarker>)>,
}

/// [design, #2403] Synthetic list/note scrolling for reports integration tests only.
#[cfg(feature = "report-test-support")]
#[doc(hidden)]
pub fn report_progress_fixture(step: usize) -> DataState {
    let mut st = DataState { text_max: 1000.0, pane_of: Some((NOTES, false, 42)), ..Default::default() };
    if step >= 1 {
        scroll_pane(&mut st, NOTES, Some((NOTES, false, 42)), PaneMoves { pages: 1.0, ..Default::default() });
    }
    if step >= 2 {
        st.sel[NOTES] = 1;
    }
    if step >= 3 {
        st.first[NOTES] = 1;
    }
    st.navigated = step as u32;
    st.poll = step as f32;
    st
}

// ---- what is shown ------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Mark {
    None,
    Empty,
    Filled,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Row {
    pub(crate) text: String,
    pub(crate) mark: Mark,
    pub(crate) done: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Right {
    None,
    Objectives { rows: Vec<Row>, first: f32 },
    Text { text: String, first: f32 },
    Image(String),
    /// A Sound or Voice note: tenths of a second still to play and the share played
    /// (thousandths); the note not playing: none left, the marker at the right end.
    Audio { left: u32, played: u16 },
    Wave { on: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Button {
    pub(crate) hint: Hint,
    pub(crate) kind: ButtonKind,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MapMark {
    /// A map marker's type and whether the player has been there; `None` for a door.
    pub(crate) icon: Option<(u8, bool)>,
    /// Optional custom icon texture path set by `SetCustomMapMarkerIcon` (applied when
    /// discovered, falling back to `model::marker_icon` when absent or unreadable).
    pub(crate) custom_icon: Option<String>,
    pub(crate) highlighted: bool,
    /// The marker's middle in the map's window.
    pub(crate) at: Vec2,
    /// The marker's or the door's reference (the World Map's rows are kept by it,
    /// `crate::tiles::map_menu`), and its texture coordinates.
    pub(crate) form: u32,
    pub(crate) uv: Option<[f32; 2]>,
    /// For the page drawn from the file only (`MapMenu` on, [`DataState::tile_rows`]; else
    /// empty, the hand-built page keeps its names apart): the name, and a World Map marker's
    /// reputation line ([`model::reputation_line`]), which the highlight box shows when a refit
    /// puts it on this marker.
    pub(crate) name: String,
    pub(crate) detail: Option<String>,
    /// Drawn in red: found in this run, under the player's tweak.
    pub(crate) recent: bool,
    /// A door under the player's Exterior and Interior Door Icons tweak: whether it leads
    /// outside.
    pub(crate) outside: Option<bool>,
}

impl MapMark {
    pub(crate) fn picture(&self) -> String {
        match self.icon {
            Some((kind, found)) => {
                if found {
                    if let Some(custom) = self.custom_icon.as_deref().filter(|p| !p.trim().is_empty()) {
                        return custom.to_string();
                    }
                }
                model::marker_icon(kind, found)
            }
            None => match self.outside {
                Some(true) => DOOR_ICON_OUTSIDE.into(),
                Some(false) => DOOR_ICON_INSIDE.into(),
                None => DOOR_ICON.into(),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MapShown {
    /// The map picture and where it lies in the window.
    picture: Option<(String, Rect)>,
    /// The Local Map's outline picture and where it lies in the window.
    outline: Option<(Handle<Image>, Rect)>,
    /// The Local Map's squares that will reset at their next load, in the window (the
    /// player's Local Map Shows Reset Areas tweak).
    reset_cells: Arc<Vec<Rect>>,
    marks: Vec<MapMark>,
    /// The player's arrow: its middle and heading (radians).
    player: Option<(Vec2, f32)>,
    /// The active quest's markers (their top left in the window).
    quests: Vec<Vec2>,
    /// The custom marker (its top left, as a quest marker's).
    custom: Option<Vec2>,
    /// The World Map's waypoint nodes (their middles) and its dotted lines, shared with the
    /// cache they come from ([`cached_waypoints`]).
    nodes: Arc<Vec<Vec2>>,
    lines: Arc<Vec<(LineTo, Vec<Piece>)>>,
    /// The highlight box's middle and the name it shows.
    cursor: Vec2,
    label: Option<String>,
    /// The line under the name: a World Map marker's reputation and the player's
    /// standing with it ([`model::reputation_line`]).
    detail: Option<String>,
    /// The World Map's companions ([`map_companions`]): where each name stands, the name,
    /// and whether it is knocked out (shown in red, under the player's tweak).
    companions: Vec<(Vec2, String, bool)>,
    /// Their names in the locations' font (the player's tweak), else font 2.
    companion_font: usize,
    /// The World Map's view and its parts in texture coordinates, for the page drawn from the
    /// file (`crate::tiles::map_menu`); none on the Local Map.
    pub(crate) world: Option<WorldParts>,
}

/// What the World Map shows, by what each part stands for and in the picture's texture
/// coordinates (0..1), so the page drawn from the file writes them as the file's `_x`/`_y`
/// [documented: map_menu.xml, the templates' `_x`/`_y` "in map texture coords"] and a pan or a
/// zoom moves only the picture [design].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WorldParts {
    /// The viewport the hand-built drawing paints from too.
    pub(crate) view: MapView,
    /// The picture's file and the magnification.
    pub(crate) texture: String,
    pub(crate) mag: f32,
    /// The active quest's markers: each target's reference and where it is.
    pub(crate) quests: Vec<(u32, [f32; 2])>,
    /// The custom marker.
    pub(crate) custom: Option<[f32; 2]>,
    /// The player's arrow and heading (radians, clockwise from north).
    pub(crate) arrow: Option<([f32; 2], f32)>,
    /// The dotted lines' far ends, from the arrow ([`waypoints`]): what each leads to, its
    /// target's reference (0 for the custom marker) and where. None on the Local Map.
    pub(crate) ends: Vec<(LineTo, u32, [f32; 2])>,
    /// The Local Map's outline picture, once made, and its size in texels: the picture its
    /// frame shows ([`outline`]); none on the World Map.
    pub(crate) outline: Option<(Handle<Image>, Vec2)>,
    /// Where the highlight box was put before it took a marker's place (the window's units: the
    /// pointer's place, or the controller's cursor), for the page drawn from the file to pick again
    /// in another window ([`nearest`]).
    pub(crate) want: Vec2,
    /// The Local Map's rows for the page drawn from the file: every door and marker here, those
    /// outside the window too (the hand-built page leaves them out, [`MapShown::marks`]), so a pan
    /// keeps their rows; none on the World Map, whose rows are its marks.
    pub(crate) rows: Option<Vec<MapMark>>,
    /// The World Map's companions (`map_companions`): each teammate's FormID, texture
    /// coordinates, display name, and knocked-out state; empty on the Local Map.
    pub(crate) companions: Vec<(u32, [f32; 2], String, bool)>,
    /// The font number for companion names on the World Map (2 by default, 1 with
    /// `bCompanionsUseLocationFontOnMap`).
    pub(crate) companion_font: u8,
}

impl MapShown {
    /// The highlight box's middle (the window's units), its name and the line under it.
    pub(crate) fn highlight(&self) -> (Vec2, Option<&str>, Option<&str>) {
        (self.cursor, self.label.as_deref(), self.detail.as_deref())
    }

    /// The markers, in the order the model found them.
    pub(crate) fn marks(&self) -> &[MapMark] {
        &self.marks
    }
}

#[cfg(test)]
impl MapShown {
    /// A World Map gathered for a test of the page drawn from the file.
    pub(crate) fn for_test(marks: Vec<MapMark>, world: Option<WorldParts>, cursor: Vec2, label: Option<&str>, detail: Option<&str>) -> MapShown {
        MapShown { marks, world, cursor, label: label.map(Into::into), detail: detail.map(Into::into), ..empty_map() }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Body {
    /// A map: boxed, as it is far larger than a list (its markers' and lines' places).
    Map(Box<MapShown>),
    List { rows: Vec<Row>, first: usize, boxed: Option<usize>, right: Right },
}

/// Everything the page shows.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub(crate) tab: usize,
    pub(crate) place: String,
    pub(crate) when: String,
    pub(crate) buttons: Vec<Button>,
    pub(crate) body: Body,
    pub(crate) popup: Option<Popup>,
}

/// What the page shows as last prepared ([`step`]), with what the drawing from the file
/// (`crate::tiles::map_menu`) needs besides: each list row's record (a quest's or note's FormID, a
/// station's base, a challenge's FormID), the selected row, whether the pointer picked it and a count
/// of the keys' moves (the tile list brings the row a key picked into view, not the pointer's);
/// on Misc whether the challenges show instead of the notes and whether the notes' filter is on
/// (another filter starts the list at its top).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Prepared {
    pub(crate) view: View,
    pub(crate) keys: Vec<u32>,
    pub(crate) sel: usize,
    pub(crate) pointer_pick: bool,
    pub(crate) navigated: u32,
    pub(crate) challenges: bool,
    pub(crate) filtered: bool,
}

// ---- the system -------------------------------------------------------------------------

/// The game the page reads.
#[derive(SystemParam)]
pub struct Game<'w> {
    script: Option<ResMut<'w, ScriptState>>,
    sim: Option<Res<'w, SimState>>,
    world: Option<Res<'w, PlayWorld>>,
    location: Option<Res<'w, Location>>,
    player: Option<Res<'w, Player>>,
    radio: Option<Res<'w, Radio>>,
    places: Option<Res<'w, RadioPlaces>>,
    phys: Option<ResMut<'w, crate::phys::PhysState>>,
    holotape: Option<ResMut<'w, crate::holotape::Holotape>>,
}

/// What the player presses.
#[derive(SystemParam)]
pub struct Input<'w, 's> {
    keys: crate::pad::ActionButtons<'w, KeyCode>,
    wheel: MessageReader<'w, 's, MouseWheel>,
    touch_wheel: Option<Res<'w,super::touch_scroll::WheelFrame>>,
    physical:Option<Res<'w,super::touch_scroll::PhysicalPriority>>,
    touch_pip: Option<Res<'w,super::PipBoy>>,
    touch_map: Option<ResMut<'w,touch_map::TouchMap>>,
    map_gate: touch_map::Gate<'w,'s>,
    // Separate from the raw bridge's shared gate: its parent owns this overlay mutably.
    map_overlay: Option<Res<'w, super::touch_items::OverlayCapture>>,
    menu: MessageReader<'w, 's, MenuInput>,
    pip_out: MessageWriter<'w, PipBoyInput>,
    time: Res<'w, Time<Real>>,
    buttons: crate::pad::ActionButtons<'w, MouseButton>,
    aid: ResMut<'w, PipBoyAid>,
    ask: MessageWriter<'w, FastTravelAsk>,
    summon: MessageWriter<'w, SummonCompanions>,
    go: MessageWriter<'w, FastTravelGo>,
    pads: crate::pad::ActionPads<'w, 's>,
    bindings: Option<Res<'w, Bindings>>,
}

/// The page's drawing tools.
#[derive(SystemParam)]
pub struct Tools<'w> {
    kit: ResMut<'w, Kit>,
    data: Option<Res<'w, GameData>>,
    images: ResMut<'w, Assets<Image>>,
    formats: Option<Res<'w, CompressedImageFormatSupport>>,
    glyphs: Res<'w, crate::pad::PadGlyphs>,
}

/// With the DATA page drawn from its file switched on (`[UI] TileMenus=MapMenu`), the page's
/// preparation, before the tile page's decision and the frame (docs/spec-notes/pipboy.md §5.3.1):
/// the quests and markers kept track of, the player's input taken, notes marked read, the Local
/// Map's outline advanced and what the page shows gathered ([`step`]), for the DATA page as the
/// Pip-Boy shows it, whatever draws it. [`draw`] then only paints the hand-built body while it is
/// there. Off, this does nothing and [`draw`] prepares and paints as it always did.
#[allow(clippy::too_many_arguments)]
pub fn prepare(
    mut commands: Commands,
    switch: Option<Res<crate::tiles::vanilla::TileMenus>>,
    pip: Res<PipBoy>,
    screen: Option<Res<PipBoyScreen>>,
    state: Option<ResMut<DataState>>,
    mut game: Game,
    mut input: Input,
    mut images: ResMut<Assets<Image>>,
    tiles: Option<Res<crate::tiles::map_menu::MapTiles>>,
) {
    if !prepares_early(switch.as_deref()) {
        return;
    }
    // Laid out in the Pip-Boy's screen texture, at its size whatever the UI scale (#1631).
    let _canvas = crate::interface::original_units();
    let Some(mut state) = state else {
        commands.init_resource::<DataState>();
        return;
    };
    let open = (pip.open() && pip.page == Page::Data).then(|| pip.tab());
    let early = Early {
        // The list the player saw last frame was the tile page's, on a tab it draws: the wheel
        // over it is that page's scroll.
        tiles_list: tiles.as_ref().is_some_and(|t| t.shown.is_some()) && open.is_some_and(|tab| crate::tiles::map_menu::TILED_TABS.contains(&tab)),
        // A menu over the page this frame (set before the scripts, so through the frame it closes
        // in): the page's input is that menu's (`crate::tiles::map_menu::MapTiles::over_data`).
        hold: tiles.as_ref().is_some_and(|t| t.over_data),
    };
    step(&mut state, open, screen.as_deref(), &mut game, &mut input, &mut images, Some(early));
}

/// What the early preparation ([`prepare`], `MapMenu` on) knows besides the hand-built drawing:
/// whether the tile page's list showed last frame (the wheel over it scrolls that list), and
/// whether a menu over the page holds its input this frame.
#[derive(Clone, Copy, Debug, Default)]
struct Early {
    tiles_list: bool,
    hold: bool,
}

/// Whether the page is prepared by [`prepare`], before the frame: with `MapMenu` switched on.
pub(crate) fn prepares_early(switch: Option<&crate::tiles::vanilla::TileMenus>) -> bool {
    switch.is_some_and(|s| s.map)
}

/// Fill the page's body when what it shows changed. With `MapMenu` off it first prepares the
/// page ([`step`]) while its body is there, as it always did; on, [`prepare`] did that before
/// the frame and this paints what it gathered.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut commands: Commands,
    bodies: Query<(Entity, &PageBody)>,
    added: Query<(), Added<PageBody>>,
    pip: Res<PipBoy>,
    screen: Option<Res<PipBoyScreen>>,
    state: Option<ResMut<DataState>>,
    switch: Option<Res<crate::tiles::vanilla::TileMenus>>,
    mut game: Game,
    mut input: Input,
    mut tools: Tools,
) {
    // Laid out in the Pip-Boy's screen texture, at its size whatever the UI scale (#1631).
    let _canvas = crate::interface::original_units();
    let Some(mut state) = state else {
        commands.init_resource::<DataState>();
        return;
    };
    let st = &mut *state;
    let body = bodies.iter().find(|(_, b)| b.page == Page::Data && pip.open());
    let view = if prepares_early(switch.as_deref()) {
        // Prepared before the frame, for the tab the body was made for.
        let tab = match (body, st.prepared.as_ref()) {
            (Some((_, b)), Some(p)) if p.view.tab == b.tab => b.tab,
            _ => {
                // Drawn from its tiles (or not shown): its refit, if any, is done, so the frame's
                // click is taken now, against the highlight box as laid out for good (#3324).
                if let Some(s) = take_click(st, &mut game, input.keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])) {
                    input.summon.write(s);
                }
                flush_travel(st, &mut input);
                return;
            }
        };
        // A map prepared in the window the page drawn from the file published, now drawn by hand
        // (the tile page fell back this frame): gathered again in the hand-built window, so what
        // is drawn and where the pointer is read agree. No input is taken again.
        let pointer = screen.as_deref().and_then(|s| s.pointer).filter(|_| !input.bindings.as_ref().is_some_and(|b| b.device == InputDevice::Controller));
        let mut view = match refit_hand_built(st, tab, &game, pointer) {
            Some(v) => v,
            None => st.prepared.as_ref().map(|p| p.view.clone()).unwrap_or_else(|| unreachable!()),
        };
        // The frame's click, against the map as drawn here (#3324).
        if let Some(s) = take_click(st, &mut game, input.keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])) {
            input.summon.write(s);
        }
        view.popup = st.popup.clone();
        if let Some(p) = st.prepared.as_mut() {
            p.view = view.clone();
        }
        flush_travel(st, &mut input);
        view
    } else {
        // `MapMenu` off: prepared and drawn here, the question sent by this system in the frame it
        // was asked in, after the page's own preparation (before 2026-11-12 it was sent during it).
        let view = step(st, body.map(|(_, b)| b.tab), screen.as_deref(), &mut game, &mut input, &mut tools.images, None);
        flush_travel(st, &mut input);
        match view {
            Some(v) => v,
            None => return,
        }
    };
    let Some((body, _)) = body else { return };
    // The hand-built body is drawn: its map window is the file's numbers again.
    st.map_window = None;
    if added.get(body).is_err() && st.shown.as_ref() == Some(&view) {
        return;
    }
    // While a press is held (the mouse's left button, or the test aid's click between its
    // press and its release) a playing audio note's meter alone does not draw the page
    // again: the redraw replaced the hints and rows under the pointer every frame or two,
    // so the release fell on another node than the press and no click came (the
    // Challenges hint under a playing voice note, issue #2571).
    let pressing = input.buttons.pressed(MouseButton::Left) || input.aid.click == 1;
    if added.get(body).is_err() && pressing && st.shown.as_ref().is_some_and(|s| meter_only_moved(s, &view)) {
        return;
    }
    let mut c = Ctx::new(&mut tools.kit, tools.data.as_deref(), &mut tools.images, tools.formats.as_deref(), &tools.glyphs);
    st.button_rects = (view.tab, button_boxes(&c, &view.buttons));
    // The question box's answers are spawned anew with the page: the answer the pointer was
    // on is gone with its entity (no PointerOut comes from it), so it is forgotten until
    // the new answers' PointerOver ([`BoxHold::rebuilt`]).
    if view.popup.is_some() {
        st.popup_hold.rebuilt();
    }
    commands.entity(body).despawn_children().with_children(|p| show(p, &mut c, st, &view));
    st.shown = Some(view);
}

/// Whether `now` differs from `shown` only in a playing audio note's time and meter.
fn meter_only_moved(shown: &View, now: &View) -> bool {
    fn still(v: &View) -> View {
        let mut v = v.clone();
        if let Body::List { right: Right::Audio { left, played }, .. } = &mut v.body {
            (*left, *played) = (0, 0);
        }
        v
    }
    matches!(now.body, Body::List { right: Right::Audio { .. }, .. }) && still(shown) == still(now)
}

/// The page's preparation, a frame's: keep track of the quests and markers, and while the page is
/// open on tab `open` take the player's input and gather what it shows ([`View`], kept in
/// [`DataState::prepared`] and returned). Closed (`None`), what was open is let go. `early`: the
/// early preparation's ([`prepare`], `MapMenu` on), which keeps what it gathered for the tile page;
/// `None` for the hand-built drawing's, which keeps nothing more than it always did.
#[allow(clippy::too_many_arguments)]
fn step(st: &mut DataState, open: Option<usize>, screen: Option<&PipBoyScreen>, game: &mut Game, input: &mut Input, images: &mut Assets<Image>, early: Option<Early>) -> Option<View> {
    st.tile_rows = early.is_some();
    st.poll += input.time.delta_secs();
    if st.poll >= POLL {
        st.poll = 0.0;
        track(st, game);
    }
    let Some(tab) = open else {
        if let Some(touch)=input.touch_map.as_deref_mut() {touch.cancel();}
        st.was_open = false;
        st.popup = None;
        st.note_shown = None;
        st.travel_marker = None;
        st.bar_drag = None;
        // A press held while the page closes (another page, the Pip-Boy down) is let go with it:
        // no click or drag on the map the page comes back to (#3324).
        st.press = None;
        st.drag_from = None;
        st.map_click = None;
        input.wheel.clear();
        input.menu.clear();
        return None;
    };
    if st.last_tab != Some(tab) {
        st.last_tab = Some(tab);
        st.boxed = false;
        st.popup = None;
        st.travel_marker = None;
        st.press = None;
    }
    if !st.was_open {
        st.was_open = true;
        opened(st, game);
    }
    let lists = lists(st, game);
    // The custom marker is the game's (scripts set it too, and the save keeps it); the
    // page keeps its own only while no game runs.
    if let Some(rt) = game.script.as_ref().and_then(|s| s.rt.as_ref()) {
        st.custom = rt.game.custom_map_marker().map(CustomMarker::from_game);
    }
    // With Controller chosen the controls answer the controller only (pad.rs): the mouse
    // pointer is ignored so it cannot take the highlight box back from the stick.
    let mouse_pointer = screen.and_then(|s| s.pointer).filter(|_| !input.bindings.as_ref().is_some_and(|b| b.device == InputDevice::Controller));
    let e = early.unwrap_or_default();
    let map_allowed = !e.hold && touch_map::source_allows(game.script.as_deref());
    let direct_map = touch_map::take(st,tab,screen,input,map_allowed);
    let pointer = direct_map.map(|frame|frame.0).or(mouse_pointer);
    st.pointer_now = pointer.or_else(||screen.and_then(|s|s.pointer));
    st.aiming = pointer.is_some();
    if e.hold {
        hold_input(st, tab, &lists, input);
    } else {
        take_input(st, tab, &lists, game, input, pointer, e.tiles_list);
    }
    if tab == NOTES && lists.challenges.is_none() {
        mark_read(st, &lists, game);
    } else {
        // Left the notes: the next note shown comes up again.
        st.note_shown = None;
    }
    if tab == LOCAL {
        advance_outline(st, game, images);
    }
    // Over a button at the right the pointer is off the map: the highlight box stays where
    // it was, and with it the Travel hint's marker (#1897).
    let on_map = pointer.filter(|_| !on_button(st, tab, pointer));
    let mut view = build(st, tab, &lists, game, on_map, pointer.is_some());
    touch_map::finish(st, &view, direct_map.and_then(|frame| frame.1));
    // A click on a map is resolved against the highlight box as drawn for good (#3324): here for
    // the hand-built page; the page drawn from the file may lay the map out again (its refit,
    // `DataState::refit_map`, or the fall back to the hand-built window), so with the early
    // preparation [`draw`] takes it once that is done.
    if early.is_none() {
        if let Some(s) = take_click(st, game, input.keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])) {
            input.summon.write(s);
        }
        view.popup = st.popup.clone();
    }
    // The hints are logged when they change from what was last shown: the hand-built drawing's
    // last view, or with the early preparation the last one prepared.
    let last = if early.is_some() { st.last_view.as_ref() } else { st.shown.as_ref() };
    if last.is_none_or(|s| s.tab != view.tab || s.buttons != view.buttons) {
        let line = super::frame::hints_line(view.buttons.iter().map(|b| &b.hint));
        info!("pipboy hints: data {}: {}", TAB_NAMES[view.tab.min(RADIO)], if line.is_empty() { "none" } else { &line });
    }
    if early.is_some() {
        st.last_view = Some(view.clone());
        st.prepared = Some(Prepared {
            view: view.clone(),
            keys: row_keys(tab, &lists),
            sel: st.sel[tab],
            pointer_pick: st.pointer_pick,
            navigated: st.navigated,
            challenges: lists.challenges.is_some(),
            filtered: st.notes_of_active,
        });
    }
    Some(view)
}

/// The map on tab `tab` gathered again in the hand-built window, when it was gathered in the
/// window the page drawn from the file published (`map_window` for that tab) and is to be drawn by
/// hand: that window is let go and the map's view, pointer and highlight box worked out again from
/// the state as it is (no input taken). `None` when the view already is the hand-built window's.
/// `pointer`: the pointer on the page, as the preparation reads it.
fn refit_hand_built(st: &mut DataState, tab: usize, game: &Game, pointer: Option<Vec2>) -> Option<View> {
    if !(tab == LOCAL || tab == WORLD) || !st.map_window.is_some_and(|w| w.0 == tab) {
        return None;
    }
    let from = map_window(st, tab);
    st.map_window = None;
    // The highlight box keeps its place on the canvas, in the hand-built window.
    let to = hand_built_window();
    if let Some(c) = st.cursor[tab].as_mut() {
        *c = (from.min + *c - to.min).clamp(Vec2::ZERO, to.size());
    }
    let lists = lists(st, game);
    let on_map = pointer.filter(|_| !on_button(st, tab, pointer));
    let view = build(st, tab, &lists, game, on_map, pointer.is_some());
    st.follow_the_box(tab);
    info!("pipboy data: {} drawn by hand in its own window", TAB_NAMES[tab]);
    Some(view)
}

/// Send the fast-travel question asked this frame ([`DataState::travel_pending`]) once the map is
/// drawn, so it asks about the marker the highlight box is on as drawn [design: one question, the
/// frame it was asked in].
fn flush_travel(st: &mut DataState, input: &mut Input) {
    if let Some(m) = take_travel(st) {
        input.ask.write(FastTravelAsk(m));
    }
}

/// The marker the question asked this frame is about, taken: the one it was asked of, or after
/// the map was drawn again the Travel target as drawn last (none: not asked, said once).
fn take_travel(st: &mut DataState) -> Option<u32> {
    let asked = st.travel_pending.take()?;
    let redrawn = std::mem::take(&mut st.travel_redrawn);
    let m = if redrawn { st.travel_marker } else { Some(asked) };
    if m.is_none() {
        info!("pipboy data: fast travel to {} not asked: no marker under the highlight box as drawn", FormId(asked));
    }
    m
}

/// What each row of tab `tab`'s list stands for: a quest's, note's or challenge's FormID, a
/// station's base.
fn row_keys(tab: usize, l: &Lists) -> Vec<u32> {
    match tab {
        QUESTS => l.quests.iter().map(|q| q.form).collect(),
        NOTES if l.challenges.is_some() => l.challenges.iter().flatten().map(|c| c.challenge.0).collect(),
        NOTES => l.notes.iter().map(|n| n.form).collect(),
        RADIO => l.stations.iter().map(|s| s.0).collect(),
        _ => Vec::new(),
    }
}

/// An act of the page drawn from the file that names a record, as the act on that record's row in
/// the list as it is now (`keys`, [`row_keys`]); `None` for a record no longer there or another
/// tab's. Other acts are as they are.
fn by_record(a: Act, tab: usize, keys: &[u32]) -> Option<Act> {
    let at = |t: usize, k: u32| (t == tab).then(|| keys.iter().position(|x| *x == k)).flatten();
    Some(match a {
        Act::HoverRecord(t, k) => Act::Hover(t, at(t, k)?),
        Act::LeaveRecord(t, k) => Act::Leave(t, at(t, k)?),
        Act::UseRecord(t, k) => Act::Use(t, at(t, k)?),
        other => other,
    })
}

/// A menu over the page holds its input (`MapMenu` on, [`Early::hold`]): the keys, the wheel, the
/// test aid's steps and the pointer's acts this frame are that menu's, not the page's, so Enter
/// closing a message box does not also use the selected row. What the page shows still follows the
/// lists (the selection held in them, a new pane at its top).
fn hold_input(st: &mut DataState, tab: usize, l: &Lists, input: &mut Input) {
    input.wheel.clear();
    input.menu.clear();
    st.acts.clear();
    st.drag = Vec2::ZERO;
    st.drag_from = None;
    st.press = None;
    st.bar_drag = None;
    input.aid.drag = Vec2::ZERO;
    // The question box's pointer events went with the acts: the answer the pointer was on is
    // forgotten, as after a redraw ([`BoxHold::rebuilt`]); a pointer still on an answer once the
    // menu is gone enters it again. The aid's `placemarker` waits, as its `note=` does.
    st.popup_hold.rebuilt();
    let len = list_len(tab, l);
    if len > 0 {
        st.sel[tab] = st.sel[tab].min(len - 1);
    }
    let shows = pane_key(st, tab, l);
    scroll_pane(st, tab, shows, PaneMoves::default());
}

/// Note the order quests are listed in, the markers the player finds, and where the
/// player was last outdoors.
impl DataState {
    #[cfg(test)]
    pub(crate) fn take_touch_test_actions(&mut self) -> Vec<Act> { std::mem::take(&mut self.acts) }

    /// Identity of actions deferred by a native touch contact. No action or state changes here.
    pub(crate) fn touch_action_context(&self, tab: usize) -> super::touch_scroll::DataActionContext {
        let marker = |m: CustomMarker| (m.space, m.pos.map(f32::to_bits));
        let selection = self.sel.get(tab).copied().unwrap_or(0);
        super::touch_scroll::DataActionContext {
            selection,
            key: self.prepared.as_ref().filter(|p| p.view.tab == tab).and_then(|p| p.keys.get(selection)).copied(),
            challenges: self.challenges,
            filtered: self.notes_of_active,
            quest: self.active_quest,
            travel: self.travel_marker,
            custom: self.custom.map(marker),
            spot: self.spot.get(tab).copied().flatten().map(marker),
        }
    }

    /// What the page shows as last prepared, for the drawing from the file
    /// (`crate::tiles::map_menu`).
    pub(crate) fn prepared(&self) -> Option<&Prepared> {
        self.prepared.as_ref()
    }

    /// What the player did on the page drawn from the file, taken by the next preparation as the
    /// hand-built page's nodes' acts are (`crate::tiles::map_menu::feed`).
    pub(crate) fn push_act(&mut self, a: Act) {
        self.acts.push(a);
    }

    /// The wheel's notches over the list since the tile page last took them.
    pub(crate) fn take_list_notches(&mut self) -> f32 {
        std::mem::take(&mut self.list_notches)
    }

    /// The World Map drawn from the file: its clip window as laid out and the hints' boxes (canvas
    /// units), which the model's pointer reads as the hand-built page's ([`map_window`],
    /// [`on_button`]): the pointer over a hint is the hint's, never the map's, and the map's window
    /// is where the picture is drawn. One presentation publishes these a frame.
    pub(crate) fn publish_map(&mut self, tab: usize, window: Rect, hints: Vec<Rect>) {
        self.map_window = Some((tab, window));
        self.button_rects = (tab, hints);
    }

    /// The map on tab `tab` drawn from the file in another window than the one it was gathered in
    /// (`crate::tiles::map_menu`'s refit, `view` the drawing's and `cursor` its highlight box, the
    /// window's units): the model's window, highlight box and Place Marker's place follow the
    /// drawing, so the next frame's pointer and a Place Marker now read what is drawn. No input is
    /// taken again; the marker the box is on stays the one it was on.
    ///
    /// `want` is where the box was put in the new window (before it takes a marker), `cursor` where
    /// it stands (on the marker `picked`, picked again there by [`nearest`]), `forms` the markers
    /// shown. The World Map's marker under the box and its Travel target follow the pick
    /// ([`travel_target`]); a Place Marker question already up asks about the new place. Returns
    /// the Travel target, for the hints.
    pub(crate) fn refit_map(&mut self, tab: usize, view: MapView, want: Vec2, cursor: Vec2, picked: Option<u32>, forms: &[u32]) -> Option<u32> {
        if !(tab == LOCAL || tab == WORLD) {
            return self.travel_marker;
        }
        self.map_window = Some((tab, view.window));
        self.cursor[tab] = Some(want);
        let point = match view.local {
            Some(_) => view.local_point(cursor),
            None => self.spot[tab].and_then(|s| self.maps.get(&s.space).cloned().flatten()).map(|m| m.position(view.to_uv(cursor))),
        };
        if let (Some(p), Some(s)) = (point.filter(|p| p.iter().all(|v| v.is_finite())), self.spot[tab].as_mut()) {
            (s.pos[0], s.pos[1]) = (p[0], p[1]);
        }
        if tab == WORLD {
            self.cursor_marker = picked;
            self.travel_marker = travel_target(self.travel_marker, picked, self.aiming, forms);
        }
        self.follow_the_box(tab);
        self.travel_marker
    }

    /// What was asked this frame of the place or marker under the highlight box follows the box
    /// as it is now drawn (a refit, a fallback): a Place Marker question asks of the place under
    /// it, a fast-travel question not sent yet of the Travel target picked again (none: not asked).
    /// No input is taken again.
    fn follow_the_box(&mut self, tab: usize) {
        if let (Some(Popup { ask: Ask::Marker(ask), .. }), Some(at)) = (self.popup.as_mut(), self.spot[tab]) {
            *ask = ask.at(at);
        }
        if tab == WORLD && self.travel_pending.is_some() {
            self.travel_redrawn = true;
        }
    }

    /// Whether the DATA page currently owns a popup; external input must leave it in control.
    pub fn has_popup(&self) -> bool { self.popup().is_some() }

    /// The question box over the map (fast travel's or Place Marker's), while it is up.
    pub(crate) fn popup(&self) -> Option<&Popup> {
        self.popup.as_ref()
    }

    /// What the page drawn from the file measured, as the hand-built drawing publishes it
    /// ([`show`]): how many rows of tab `tab`'s list show whole, how far the right pane can
    /// scroll and its window (the wheel's, canvas units). One presentation publishes these a frame.
    pub(crate) fn publish_drawn(&mut self, tab: usize, fit: usize, text_max: f32, pane: Option<Rect>) {
        if let Some(f) = self.fit.get_mut(tab) {
            *f = fit;
        }
        self.text_max = text_max;
        self.pane_rect = pane;
        self.pane_page = pane.map(|r| r.height());
    }

    /// A tab's selected row and whether Misc shows the challenges, for the scroll sound
    /// (`super::sounds`).
    pub fn selection(&self, tab: usize) -> (usize, bool) {
        (self.sel.get(tab).copied().unwrap_or(0), self.challenges)
    }
    pub fn list_scroll(&self, tab: usize) -> Option<usize> {
        (tab >= QUESTS && tab < self.first.len()).then(|| self.first[tab])
    }
    pub(crate) fn scroll_list_by(&mut self, tab: usize, by: super::scrollbar::ScrollBy) -> bool {
        let Some(view) = self.shown.as_ref().filter(|v| v.tab == tab).or_else(|| self.prepared.as_ref().map(|p|&p.view).filter(|v|v.tab==tab)) else { return false };
        let Body::List { rows, .. } = &view.body else { return false };
        let now = self.first[tab];
        let page = self.fit[tab].max(1);
        let Some(to) = by.value(now as f64, rows.len().saturating_sub(page) as f64, page as f64) else { return false };
        self.first[tab] = to as usize;
        self.manual_list = Some((tab, self.sel[tab]));
        self.pointer_pick = true;
        now != self.first[tab]
    }

    /// [design, #2403 / file refs] Read-only logical list/pane/map progress. Excludes
    /// radio playback/wave clocks, pointer hover and attempted-input counters.
    pub fn progress(&self, tab: usize) -> impl std::fmt::Debug + '_ {
        (self.selection(tab), self.first.get(tab), self.text_first, self.pane_of, self.notes_of_active,
            self.active_quest, self.centre.get(tab).filter(|_| tab < 2), self.magnify.get(tab).filter(|_| tab < 2),
            self.cursor.get(tab).filter(|_| tab < 2), &self.popup, &self.custom)
    }

    /// The active quest, for a save (`crate::save`): a ForceActiveQuest the Pip-Boy has
    /// not taken yet wins, as it would on the Pip-Boy's next refresh, and the game's copy
    /// stands in while the Pip-Boy has none (it fills only when its lists are made; #1610).
    pub fn active_quest(&self, game: Option<&fnv_game::Game<'_>>) -> Option<u32> {
        let ui = game.map(|g| &g.ui);
        match ui {
            Some(u) if u.active_quest_count != self.forced_seen && u.active_quest.is_some() => u.active_quest.map(|q| q.0),
            _ => self.active_quest.or(ui.and_then(|u| u.active_quest).map(|q| q.0)),
        }
    }

    /// A save's active quest, for the Pip-Boy and for scripts (GetActiveQuest and the
    /// functions that read it take the game's copy, which the save does not hold; #1610).
    pub fn restore_active_quest(world: &mut bevy::prelude::World, q: u32) {
        if !world.contains_resource::<DataState>() {
            world.init_resource::<DataState>();
        }
        world.resource_mut::<DataState>().active_quest = Some(q);
        if let Some(rt) = world.get_resource_mut::<ScriptState>().and_then(|s| s.into_inner().rt.as_mut()) {
            // The quest is set without counting a ForceActiveQuest.
            rt.game.ui.active_quest = Some(FormId(q));
        }
    }

    /// The map markers the player found, for a save (`crate::compass`).
    pub fn discovered(&self) -> &BTreeSet<u32> {
        &self.discovered
    }

    /// The active quest's targets marked now (shown objectives, not done, whose target
    /// conditions pass; `refresh_quest_targets`), for the compass (`crate::compass`).
    pub(crate) fn marked_targets(&self) -> Vec<fnv_game::quest_targets::TargetCheck> {
        self.target_checks.iter().filter(|c| c.shown && Some(c.quest) == self.target_quest).copied().collect()
    }

    /// A save's found markers: found already, so no "Place found" for them.
    pub fn restore_discovered(world: &mut bevy::prelude::World, found: BTreeSet<u32>) {
        if !world.contains_resource::<DataState>() {
            world.init_resource::<DataState>();
        }
        let mut st = world.resource_mut::<DataState>();
        st.newly_found.retain(|f| !found.contains(f));
        st.discovered = found;
    }

    /// The markers found since the last call, with their effective names (respecting
    /// `SetMapMarkerName` when the script runtime is present).
    pub fn take_newly_found(&mut self, game: Option<&fnv_game::Game>) -> Vec<(u32, String)> {
        let new = std::mem::take(&mut self.newly_found);
        let name = |f: u32| {
            game.and_then(|g| g.map_marker_name_override(FormId(f)))
                .map(str::to_string)
                .or_else(|| self.markers.as_ref().and_then(|ms| ms.iter().find(|m| m.form == f)).map(|m| m.name.clone()))
                .unwrap_or_default()
        };
        new.into_iter().map(|f| (f, name(f))).collect()
    }

    /// The named map markers of worldspace `world` the plugins place (read once the DATA
    /// page first tracked the player): reference, place, effective name, and whether the
    /// player found it.
    pub fn named_markers_of<'a>(&'a self, world: u32, game: Option<&'a fnv_game::Game>) -> Vec<(u32, [f32; 3], &'a str, bool)> {
        self.markers
            .iter()
            .flatten()
            .filter(|m| m.world == world)
            .map(|m| {
                let name = game.and_then(|g| g.map_marker_name_override(FormId(m.form))).unwrap_or(m.name.as_str());
                (m.form, m.pos, name, self.discovered.contains(&m.form))
            })
            .collect()
    }

    pub fn markers_of(&self, world: u32) -> Vec<(u32, [f32; 3], bool)> {
        self.markers.iter().flatten().filter(|m| m.world == world).map(|m| (m.form, m.pos, self.discovered.contains(&m.form))).collect()
    }

    /// The markers of `world` not found yet that the map does not show either (no
    /// Visible flag, not shown by a script), for the compass's Hide Unmapped Places
    /// tweak [design: docs/design/tweaks.md].
    pub fn unmapped_of(&self, world: u32, game: Option<&fnv_game::Game>) -> std::collections::HashSet<u32> {
        self.markers
            .iter()
            .flatten()
            .filter(|m| {
                if m.world != world || self.discovered.contains(&m.form) {
                    return false;
                }
                let flags = game.and_then(|g| g.marker_flags(FormId(m.form))).unwrap_or(m.flags);
                if flags & fnv_game::nvse_lu2::MARKER_VISIBLE != 0 {
                    return false;
                }
                !game.is_some_and(|g| g.map_marker_shown(FormId(m.form), flags & fnv_game::nvse_lu2::MARKER_HIDDEN != 0))
            })
            .map(|m| m.form)
            .collect()
    }
}

/// The custom map marker in a save (#1954), under `custom_map_marker`: the game's marker
/// (the one Place Marker and SetCustomMapMarker set), the page's own while no game runs;
/// nothing without one, so a save without it loads with none (a new game has none)
/// [design: the original very likely keeps it, docs/spec-notes/pipboy.md §5.3].
pub const CUSTOM_MARKER_SAVE_HOOK: crate::save::SaveHook = crate::save::SaveHook {
    key: "custom_map_marker",
    save: |w| {
        let m = match w.get_resource::<ScriptState>().and_then(|s| s.rt.as_ref()) {
            Some(rt) => rt.game.custom_map_marker().map(CustomMarker::from_game),
            None => w.get_resource::<DataState>()?.custom,
        };
        serde_json::to_value(m?).ok()
    },
    // Put back earlier, into the restored game before its LoadGame handlers and load-time
    // files run (`restore_custom_marker`, from `crate::script`); here, after them, it would
    // undo what they did. The page takes it from the game on its next frame.
    load: |_, _| {},
    // The worldspace or cell by FormID: moved to the current load order; one of a plugin no
    // longer loaded takes the marker away.
    remap: Some(|v, r| match v.get("space").and_then(serde_json::Value::as_u64).and_then(|n| r.map(n as u32)) {
        Some(n) => v["space"] = serde_json::Value::from(n),
        None => *v = serde_json::Value::Null,
    }),
};

/// The save's custom marker (its `custom_map_marker` part) in the current load order:
/// the worldspace or cell remapped, none when its plugin is no longer loaded.
pub(crate) fn marker_from_save(v: &serde_json::Value, remap: Option<&fnv_save::PluginRemap>) -> Option<CustomMarker> {
    let mut v = v.clone();
    if let (Some(f), Some(r)) = (CUSTOM_MARKER_SAVE_HOOK.remap, remap) {
        f(&mut v, r);
    }
    serde_json::from_value(v).ok()
}

/// Put the save's custom marker into the game just restored from it (`crate::script`,
/// before the LoadGame and PostLoadGame handlers and the Script Runner's load files), so
/// a handler's SetCustomMapMarker or ClearCustomMapMarker wins over the save [design: a
/// load's handlers see the loaded game]. A save without one leaves the restored game's
/// none.
pub(crate) fn restore_custom_marker(game: &mut fnv_game::Game, pending: Option<&crate::save::PendingEngine>) {
    let Some(p) = pending else { return };
    let Some(v) = p.0.extra.get(CUSTOM_MARKER_SAVE_HOOK.key) else { return };
    let m = marker_from_save(v, p.2.as_ref().map(|o| &o.remap));
    game.other.jg8.custom_marker = m.map(CustomMarker::to_game);
    match m {
        Some(c) => info!("pipboy map: custom marker back from the save at ({:.0}, {:.0}) in {}", c.pos[0], c.pos[1], FormId(c.space)),
        None => info!("pipboy map: the save's custom marker was in a plugin no longer loaded"),
    }
}

/// The distance a map marker without its own radius is found within, when no plugin sets
/// `iMapMarkerRevealDistance` [documented: GECK wiki "Complete List of GameSettings", its
/// Fallout: New Vegas default 1000; not in FalloutNV.esm]. That this setting is that
/// distance is a [hypothesis] (#2338; observer check 128): the wiki does not describe it.
const REVEAL_DISTANCE: f32 = 1000.0;

/// The radius a map marker is found within: its own (`XRDS`), else `reveal`. No marker in
/// the masters carries an `XRDS` of 0 [documented: fnvtool esm decode of every named map
/// marker of FalloutNV.esm and the four story DLCs; docs/spec-notes/pipboy.md §5.3], so an
/// absent radius (read as 0) and a non-positive one are treated alike (#2338). A radius
/// that comes out 0 or less (a plugin's `iMapMarkerRevealDistance` of 0) finds nothing by
/// walking [design, #2338].
fn discovery_radius(radius: f32, reveal: f32) -> f32 {
    if radius > 0.0 { radius } else { reveal }
}

fn track(st: &mut DataState, game: &Game) {
    if let Some(rt) = game.script.as_ref().and_then(|s| s.rt.as_ref()) {
        // A co-op guest's list as it changes (the host's quests in, and out with why).
        if rt.game.coop_guest {
            let now: Vec<u32> = guest_quests(&rt.game).into_iter().map(|k| k.form).collect();
            for q in st.guest_listed.iter().filter(|q| !now.contains(q)) {
                let story = rt.game.coop_host_story.quests.get(&FormId(*q)).map(|s| format!("story: ended {} failed {} objectives {:?}", s.ended, s.failed, s.objectives)).unwrap_or_else(|| "not in the story".into());
                info!("pipboy data: the host's quest {} left this guest's list ({story})", rt.game.data.name(FormId(*q)));
            }
            st.guest_listed = now;
        }
        // A co-op guest lists the host's quests (`guest_quests`), as they first come.
        let mut new: Vec<u32> = if rt.game.coop_guest {
            guest_quests(&rt.game).into_iter().map(|k| k.form).filter(|q| !st.seen.contains_key(q)).collect()
        } else {
            rt.game.quests.iter().filter(|(q, s)| listed(&rt.game, **q, s) && !st.seen.contains_key(&q.0)).map(|(q, _)| q.0).collect()
        };
        if !new.is_empty() {
            st.seen_count += 1;
            new.sort_unstable();
            if rt.game.coop_guest {
                let names: Vec<String> = new.iter().map(|q| rt.game.data.name(FormId(*q))).collect();
                info!("pipboy data: the host's quests listed here now: {}", names.join(", "));
            }
            for q in new {
                st.seen.insert(q, st.seen_count);
            }
        }
    }
    let (Some(loc), Some(player)) = (game.location.as_ref(), game.player.as_ref()) else { return };
    if loc.interior.is_none() {
        st.outdoors = Some((loc.world, player.feet, player.heading));
        let feet = player.feet;
        let world = loc.world;
        // A plugin's value as it is (0 included); the default only when none sets it.
        let reveal = game.sim.as_ref().and_then(|s| s.forms.setting_f32("iMapMarkerRevealDistance")).unwrap_or(REVEAL_DISTANCE);
        // A marker not in the world now (Initially Disabled and never enabled, a script's
        // Disable) is not found: Fallout3.esm's 42 switched-off design markers (`DC23b`)
        // never are, a quest's marker is once enabled; a found marker disabled later stays
        // found (#3123) [hypothesis: as `known` leaves it off the map].
        let present = |form: u32| game.sim.as_ref().is_none_or(|s| s.present(form));
        let found: Vec<u32> = markers(st, game)
            .into_iter()
            .flatten()
            .filter(|m| {
                let r = discovery_radius(m.radius, reveal);
                m.world == world && r > 0.0 && (m.pos[0] - feet[0]).hypot(m.pos[1] - feet[1]) <= r && present(m.form)
            })
            .map(|m| m.form)
            .collect();
        for f in found {
            if st.discovered.insert(f) {
                info!("pipboy data: map marker {} found", FormId(f));
                st.newly_found.push(f);
                st.found_this_run.insert(f);
            }
        }
    }
}

/// The map markers, read once.
fn markers<'a>(st: &'a mut DataState, game: &Game) -> Option<&'a Vec<read::Marker>> {
    if st.markers.is_none() {
        let (w, sim) = (game.world.as_ref()?, game.sim.as_ref()?);
        let ms = read::markers(w.lo, w.schema, sim.refs);
        info!("pipboy data: {} map markers ({} with a reputation)", ms.len(), ms.iter().filter(|m| m.reputation.is_some()).count());
        st.markers = Some(ms);
    }
    st.markers.as_ref()
}

/// The player's quest list, in order ([`model::sort_quests`]; `st` gives when each was
/// first seen).
fn player_quests(st: Option<&DataState>, g: &fnv_game::game::Game) -> Vec<QuestKey> {
    let seen = |q: u32| st.map_or(0, |st| st.seen.get(&q).copied().unwrap_or(st.seen_count + 1));
    let mut out: Vec<QuestKey> = g.quests.iter().filter(|(q, s)| listed(g, **q, s) && !(g.tweaks.hide_completed_quests && (s.completed || s.failed))).map(|(q, s)| QuestKey { seen: seen(q.0), form: q.0, done: s.completed || s.failed }).collect();
    model::sort_quests_with_top(&mut out, g.top_ui_quest().map(|q| q.0));
    out
}

/// The quest the player has active, as the DATA page has it whether open or not: the
/// player's pick, else a script's ForceActiveQuest, else the newest running quest. A co-op
/// host sends it to its guests (nvforever `coop`).
pub fn active_quest(st: Option<&DataState>, g: &fnv_game::game::Game) -> Option<FormId> {
    let running: Vec<u32> = player_quests(st, g).into_iter().filter(|q| !q.done).map(|q| q.form).collect();
    let pick = st.and_then(|st| st.active_quest).filter(|a| running.contains(a));
    let forced = g.ui.active_quest.map(|q| q.0).filter(|a| running.contains(a));
    // A ForceActiveQuest the page has not taken yet wins, as it will there.
    let forced_new = st.is_none_or(|st| g.ui.active_quest_count != st.forced_seen);
    let chosen = if forced_new { forced.or(pick) } else { pick.or(forced) };
    chosen.or(running.first().copied()).map(FormId)
}

/// When the quest list first showed `quest` (a running count, larger is newer; 0: not
/// yet): a co-op host sends it with its story, for its guests' order.
pub fn quest_seen(st: &DataState, quest: u32) -> u64 {
    st.seen.get(&quest).copied().unwrap_or(0)
}

/// A co-op guest's quest list, the host's (design: the guest
/// listed only the host's active quest): each quest of the host's story with a name and an
/// objective shown or done, as [`listed`] has the host's own (`Game::coop_host_story`,
/// which reaches a guest that joins later within a round of its pages); the host's active
/// quest; the quests that ended while this guest played (`Game::coop_host_ended`, at
/// once, before the story's next page). Done when completed or failed. In the host's
/// order: [`model::sort_quests`] by when the host's list first showed each
/// (`HostQuestState::seen`; a late join in testing listed them by when the guest did).
fn guest_quests(g: &fnv_game::game::Game) -> Vec<QuestKey> {
    let named = |q: FormId| g.data.quests.get(&q).and_then(|d| d.name.as_deref()).is_some_and(|n| !n.trim().is_empty());
    host_quest_log(&g.coop_host_story, g.coop_host_quest.as_ref(), &g.coop_host_ended, named, g.tweaks.hide_completed_quests)
}

/// The host's quest log as a guest's list has it ([`guest_quests`]): every named quest of the host's story
/// with an objective shown or done, the host's active quest (`host`), and the quests that
/// ended while this guest played (`ended`); done when completed or failed. In the host's
/// order. `hide_completed`: this player's own tweak leaves the done ones out, as
/// [`player_quests`] does with a player's own quests [design: docs/design/tweaks.md].
fn host_quest_log(story: &fnv_game::coop::HostStory, host: Option<&fnv_game::coop::HostQuest>, ended: &[(FormId, bool)], named: impl Fn(FormId) -> bool, hide_completed: bool) -> Vec<QuestKey> {
    // The host's order (`HostQuestState::seen`); a quest the story has not ordered yet (one
    // that ended a moment ago) is newest, as the host's own newest quest is.
    let seen = |q: u32| story.quests.get(&FormId(q)).map(|s| s.seen).filter(|s| *s > 0).unwrap_or(u64::MAX);
    let mut done: BTreeMap<u32, bool> = BTreeMap::new();
    for (q, s) in &story.quests {
        if named(*q) && !s.objectives.is_empty() {
            done.insert(q.0, s.ended);
        }
    }
    if let Some(h) = host.filter(|h| named(h.quest)) {
        *done.entry(h.quest.0).or_default() |= h.completed || h.failed;
    }
    for (q, _) in ended.iter().filter(|(q, _)| named(*q)) {
        done.insert(q.0, true);
    }
    let mut out: Vec<QuestKey> = done.into_iter().filter(|(_, done)| !(hide_completed && *done)).map(|(form, done)| QuestKey { seen: seen(form), form, done }).collect();
    model::sort_quests(&mut out);
    out
}

/// Whether a quest is in the player's list: it has a name and an objective shown or done
/// (docs/spec-notes/quest-messages.md §5).
fn listed(g: &fnv_game::game::Game, q: FormId, s: &fnv_game::quest::QuestState) -> bool {
    g.data.quests.get(&q).and_then(|d| d.name.as_deref()).is_some_and(|n| !n.trim().is_empty()) && s.objectives.values().any(|o| o.displayed || o.completed)
}

/// The Pip-Boy came up on this page: the maps centre on the player, the quest list on
/// the active quest.
fn opened(st: &mut DataState, game: &Game) {
    // The player's tweak leaves the maps where they were [design: docs/design/tweaks.md].
    if !game.script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.remember_map_position) {
        st.centre = [None, None];
    }
    st.cursor = [None, None];
    st.boxed = false;
    st.text_first = 0.0;
    st.magnify = [st.magnify[0].max(ZOOM.0), st.magnify[1].max(ZOOM.0)];
    let l = lists(st, game);
    if let Some(i) = st.active_quest.and_then(|a| l.quests.iter().position(|q| q.form == a)) {
        st.sel[QUESTS] = i;
    }
}

/// The lists' rows, with what each stands for.
#[derive(Default)]
struct Lists {
    quests: Vec<QuestKey>,
    notes: Vec<read::Note>,
    /// The player has a note, whether or not the notes' filter keeps it (the notes' X
    /// hint, [`tab_buttons`]).
    has_notes: bool,
    stations: Vec<(u32, String)>,
    /// The station the Pip-Boy plays (its base), when on.
    playing: Option<u32>,
    /// The Notes tab lists these instead of the notes.
    challenges: Option<Vec<crate::challenges::Row>>,
}

fn lists(st: &mut DataState, game: &Game) -> Lists {
    let mut out = Lists::default();
    let Some(rt) = game.script.as_ref().and_then(|s| s.rt.as_ref()) else { return out };
    let g = &rt.game;
    if g.coop_guest {
        // A co-op guest's list is the host's: its quests as the host's story
        // carries them, its active quest and the quests that ended while it played.
        out.quests = guest_quests(g);
        st.active_quest = g.coop_host_quest.as_ref().map(|h| h.quest.0);
    } else {
        out.quests = player_quests(Some(st), g);
    }
    // A script's ForceActiveQuest makes its quest the active one [documented: GECK wiki
    // ForceActiveQuest]; the player's pick afterwards wins again.
    if !g.coop_guest && g.ui.active_quest_count != st.forced_seen {
        st.forced_seen = g.ui.active_quest_count;
        if let Some(q) = g.ui.active_quest {
            st.active_quest = Some(q.0);
            info!("pipboy data: active quest {q} (ForceActiveQuest)");
        }
    }
    if !g.coop_guest && !st.active_quest.is_some_and(|a| out.quests.iter().any(|q| q.form == a && !q.done)) {
        // The player's tweaks: no quest is made the active one by itself, neither when
        // the active one is done nor when objectives come with none active [design: docs/design/tweaks.md].
        let by_hand_only = g.tweaks.no_quest_set_on_objective || g.tweaks.no_quest_set_on_complete;
        st.active_quest = if by_hand_only { None } else { out.quests.iter().find(|q| !q.done).map(|q| q.form) };
    }
    out.has_notes = !g.player.notes.is_empty();
    if let Some(w) = game.world.as_ref() {
        for n in &g.player.notes {
            // The player's tweak: notes already read are left out, but for the one on
            // screen (showing a note reads it) [design: docs/design/tweaks.md].
            if g.tweaks.hide_read_notes && g.note_read(*n) && st.note_shown != Some(n.0) {
                continue;
            }
            let note = st.notes.entry(n.0).or_insert_with(|| read::note(w.lo, w.schema, n.0));
            if let Some(note) = note {
                // The player's tweak takes in every quest under way [design: docs/design/tweaks.md].
                let under_way = g.tweaks.notes_of_all_quests && note.quest.and_then(|q| g.quests.get_key_value(&FormId(q))).is_some_and(|(q, s)| listed(g, *q, s) && !s.completed && !s.failed);
                if !st.notes_of_active || (note.quest.is_some() && note.quest == st.active_quest) || under_way {
                    // The player string codes as they read now (the bounty notices'
                    // "&PCName;", "&PCRace;" [observed: FalloutNV.esm]); the cache keeps
                    // the record's text.
                    let mut note = note.clone();
                    note.name = g.expand_codes(&note.name);
                    note.text = note.text.map(|t| g.expand_codes(&t));
                    out.notes.push(note);
                }
            }
        }
    }
    out.notes.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.form.cmp(&b.form)));
    if st.challenges {
        out.challenges = Some(crate::challenges::rows(g));
    }
    if let (Some(radio), Some(places)) = (game.radio.as_ref(), game.places.as_ref()) {
        out.stations = radio
            .pipboy_list
            .iter()
            .map(|&b| {
                let s = places.forms.stations.get(&b);
                (b, s.and_then(|s| s.name.clone().or_else(|| s.edid.clone())).unwrap_or_else(|| FormId(b).to_string()))
            })
            .collect();
        let p = &g.radio.pipboy;
        out.playing = p.station.filter(|_| p.on).and_then(|s| places.sites.get(&s.0)).map(|s| s.base);
    }
    out
}

/// The note whose page is shown is read, once each time it comes up (GetNoteRead,
/// `fnv_game::game::Game::mark_note_read`), so a script's SetNoteRead 0 holds while it stays.
/// It comes up again after the player leaves the notes or closes the Pip-Boy; a frame
/// with no scripts' game marks nothing and tries again on the next.
fn mark_read(st: &mut DataState, l: &Lists, game: &mut Game) {
    let shown = l.notes.get(st.sel[NOTES]).map(|n| n.form);
    if shown == st.note_shown {
        return;
    }
    let Some(f) = shown else {
        st.note_shown = None;
        return;
    };
    let Some(rt) = game.script.as_mut().and_then(|s| s.rt.as_mut()) else { return };
    rt.game.mark_note_read(FormId(f));
    st.note_shown = shown;
}

fn list_len(tab: usize, l: &Lists) -> usize {
    match tab {
        QUESTS => l.quests.len(),
        NOTES => l.challenges.as_ref().map_or(l.notes.len(), Vec::len),
        RADIO => l.stations.len(),
        _ => 0,
    }
}

/// Keys, the wheel, the test aid's steps and what the pointer did.
fn take_input(st: &mut DataState, tab: usize, l: &Lists, game: &mut Game, input: &mut Input, pointer: Option<Vec2>, tiles_list: bool) {
    use KeyCode::*;
    // The test aid's `placemarker`: the maps' X button, as X or a click on its hint does.
    let aid_place = std::mem::take(&mut input.aid.place_marker);
    if st.popup.is_some() {
        popup_input(st, input, game);
        return;
    }
    st.popup_hold = BoxHold::default();
    if aid_place {
        match tab {
            LOCAL | WORLD => st.acts.push(Act::Button(ButtonKind::PlaceMarker)),
            _ => info!("pipboy data: placemarker is for the maps; tab {} shown", TAB_NAMES[tab.min(RADIO)]),
        }
    }
    let len = list_len(tab, l);
    let mut moves: Vec<i32> = Vec::new();
    let mut use_it = input.keys.any_just_pressed([Enter, NumpadEnter, Space]);
    for (k, by) in [([ArrowUp, KeyW], -1), ([ArrowDown, KeyS], 1)] {
        if input.keys.any_just_pressed(k) {
            moves.push(by);
        }
    }
    for m in input.menu.read() {
        match m.0 {
            Command::Up => moves.push(-1),
            Command::Down => moves.push(1),
            Command::One => use_it = true,
            _ => {}
        }
    }
    // The test aid's `note=`: that note selected and used, as a click on its row.
    if let Some(f) = input.aid.note.take() {
        match l.notes.iter().position(|n| n.form == f).filter(|_| tab == NOTES && l.challenges.is_none()) {
            Some(i) => {
                st.sel[NOTES] = i;
                st.boxed = true;
                use_row(st, tab, i, l, game);
            }
            None => info!("pipboy data: note {} is not in the Notes list shown", FormId(f)),
        }
    }
    step_rows(st, tab, &moves, len);
    if tab == NOTES && input.keys.just_pressed(KeyR) {
        st.acts.push(Act::Button(ButtonKind::Challenges));
    }
    // The player's tweak: a right click marks the selected note unread [design: docs/design/tweaks.md].
    if tab == NOTES && !st.challenges && input.buttons.just_pressed(MouseButton::Right) {
        if let (Some(note), Some(rt)) = (l.notes.get(st.sel[NOTES]).map(|n| n.form), game.script.as_mut().and_then(|s| s.rt.as_mut()).filter(|rt| rt.game.tweaks.mark_notes_unread)) {
            rt.game.mark_note_unread(FormId(note));
            info!("pipboy data: note {} marked unread (tweak)", FormId(note));
        }
    }
    // The player's tweak: C puts a map back on the player [design: docs/design/tweaks.md].
    let recentre = game.script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.map_recentre_key);
    if recentre && (tab == LOCAL || tab == WORLD) && input.keys.just_pressed(KeyC) && st.centre[tab].take().is_some() {
        info!("pipboy data: map centred on the player again (C, tweak)");
    }
    // X (with a controller X, `crate::pad::menu_buttons`) is the tab's X button (#1897).
    if input.keys.just_pressed(KeyX) {
        // The player's tweak drops Show Location [design: docs/design/tweaks.md].
        let show_location = !game.script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.no_show_quest_location);
        match tab {
            LOCAL | WORLD => st.acts.push(Act::Button(ButtonKind::PlaceMarker)),
            QUESTS if !l.quests.is_empty() && show_location => st.acts.push(Act::Button(ButtonKind::ShowLocation)),
            NOTES if l.challenges.is_none() => st.acts.push(Act::Button(ButtonKind::NotesFilter)),
            NOTES => st.acts.push(Act::Button(ButtonKind::NextChallenge)),
            _ => {}
        }
    }
    // The right pane (a note's text, a quest's objectives) scrolls by Page Up/Down (a
    // controller's LB/RB), the wheel over it and the right stick ([`scroll_pane`]).
    let mut pane = PaneMoves { dt: input.time.delta_secs(), ..default() };
    if tab == NOTES || tab == QUESTS {
        if input.keys.just_pressed(PageDown) {
            pane.pages += 1.0;
        }
        if input.keys.just_pressed(PageUp) {
            pane.pages -= 1.0;
        }
    }
    // The wheel zooms a map, scrolls the right pane under the pointer, else the list.
    let mut notches = if input.physical.as_ref().is_some_and(|p|p.active) {0.0} else {input.touch_wheel.as_ref().zip(input.touch_pip.as_deref()).map_or(0.0,|(w,pip)|w.lines(pip))};
    let pointer=input.touch_wheel.as_ref().filter(|_|notches!=0.0).and_then(|w|w.at).or(pointer);
    for w in input.wheel.read() {
        notches += match w.unit {
            MouseScrollUnit::Line => w.y,
            MouseScrollUnit::Pixel => w.y / 20.0,
        };
    }
    if notches != 0.0 {
        match tab {
            LOCAL | WORLD => {
                let before = st.magnify[tab].max(ZOOM.0);
                st.magnify[tab] = (before * ZOOM_STEP.powf(notches)).clamp(ZOOM.0, ZOOM.1);
                // The player's tweak keeps the place under the pointer there: the map is
                // moved as a drag would by the pointer's offset from the middle times the
                // growth [design: docs/design/tweaks.md].
                let at_pointer = game.script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.map_zoom_at_pointer);
                if let Some(p) = in_window(map_window(st, tab), pointer).filter(|_| at_pointer) {
                    st.drag -= (p - MAP_MID) * (st.magnify[tab] / before - 1.0);
                }
            }
            NOTES | QUESTS if wheel_on_pane(tab, pointer, st.pane_rect) => pane.notches += notches,
            // The tile page's list scrolls itself (`crate::tiles::map_menu`).
            _ if tiles_list => st.list_notches += notches,
            _ => {
                let fit = st.fit[tab].max(1);
                st.first[tab] = (st.first[tab] as f32 - notches).round().clamp(0.0, len.saturating_sub(fit) as f32) as usize;
            }
        }
    }
    // The controller's sticks drive the maps (docs/design/controller.md §3): the right
    // stick moves the highlight box, the left stick's up and down zooms in and out
    // smoothly while held [observed: the original].
    if input.bindings.as_ref().is_some_and(|b| b.device == InputDevice::Controller) && (tab == LOCAL || tab == WORLD) {
        let right = crate::pad::stick(&input.pads, false);
        let left = crate::pad::stick(&input.pads, true);
        let dt = input.time.delta_secs();
        if right != Vec2::ZERO {
            let win = MapView::bare(map_window(st, tab));
            let (cursor, overflow) = win.stick(st.cursor[tab].unwrap_or(win.mid()), right, dt, CURSOR_SPEED);
            st.cursor[tab] = Some(cursor);
            scroll_map(st, tab, game, overflow);
        }
        if left.y != 0.0 {
            st.magnify[tab] = stick_zoom(st.magnify[tab], left.y, dt);
        }
    }
    // On Quests and Misc the right stick's up and down scrolls the right pane smoothly
    // while held [observed: the original, issue #1667; the speed is design].
    if input.bindings.as_ref().is_some_and(|b| b.device == InputDevice::Controller) && (tab == NOTES || tab == QUESTS) {
        pane.stick = crate::pad::stick(&input.pads, false).y;
    }
    let acts = std::mem::take(&mut st.acts);
    let keys = if acts.iter().any(|a| matches!(a, Act::HoverRecord(..) | Act::LeaveRecord(..) | Act::UseRecord(..))) { row_keys(tab, l) } else { Vec::new() };
    for a in acts {
        let Some(a) = by_record(a, tab, &keys) else {
            info!("pipboy data: an act on a row no longer listed is ignored");
            continue;
        };
        match a {
            // Another row shown starts its pane at the top (`scroll_pane`, by `pane_key`).
            Act::Hover(t, i) if t == tab && i < len => {
                st.sel[tab] = i;
                st.boxed = true;
                st.pointer_pick = true;
            }
            Act::Leave(t, i) if t == tab && i == st.sel[tab] => st.boxed = false,
            Act::Use(t, i) if t == tab && i < len => {
                st.sel[tab] = i;
                use_row(st, tab, i, l, game);
            }
            // The A button's hints do what Enter does on the tab.
            Act::Button(k) if k.is_a() => use_it = true,
            Act::Button(ButtonKind::PlaceMarker) => place_marker(st, tab, game, input.keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])),
            Act::Button(k) => button(st, k, input),
            Act::Bar(b) if tab == QUESTS || tab == NOTES => bar_act(st, b, pointer, &mut pane),
            Act::PaneTo(v) if tab == QUESTS || tab == NOTES => pane.drag += v - st.text_first,
            _ => {}
        }
    }
    drag_bar(st, input.buttons.pressed(MouseButton::Left), pointer, &mut pane);
    // On the World Map Enter (or Space, or the Travel hint's click) asks to travel to the
    // Travel hint's marker, as the controller's A does (#1897). A also presses Enter
    // (`crate::pad`), so the two ask once.
    let travel = tab == WORLD && (use_it || crate::pad::menu_button(&input.pads, input.bindings.as_deref(), GamepadButton::South));
    if use_it && len > 0 {
        use_row(st, tab, st.sel[tab].min(len - 1), l, game);
    }
    if len > 0 {
        st.sel[tab] = st.sel[tab].min(len - 1);
        if st.manual_list.is_some_and(|(old_tab, selected)| old_tab != tab || (selected != st.sel[tab] && !st.pointer_pick)) {
            st.manual_list = None;
        }
        if st.manual_list.is_some() {
            st.first[tab] = st.first[tab].min(len.saturating_sub(st.fit[tab].max(1)));
            st.manual_list = Some((tab, st.sel[tab]));
        } else {
            st.first[tab] = model::keep_in_view(st.sel[tab], st.first[tab].min(len - 1), st.fit[tab]);
        }
    }
    let shows = pane_key(st, tab, l);
    scroll_pane(st, tab, shows, pane);
    // A click on the World Map (the left button let go where it went down) on a marker
    // asks to travel there (`travel`); so does the controller's A (the confirm button) on a
    // highlighted marker [observed: the original; docs/design/controller.md §3].
    let mouse = (input.buttons.just_pressed(MouseButton::Left), input.buttons.just_released(MouseButton::Left));
    // A click on the map is taken once it is drawn ([`take_click`]): the Map Selectable
    // Companions tweak's summon, a travel or Place Marker.
    if let Some(m) = travel_ask(st, tab, pointer, mouse, travel) {
        st.travel_pending = Some(m);
    }
    // Dragging a map moves the point at its middle: the left button held down on it (not
    // on a button at the right), the pointer moving (or the test aid's drag).
    let held = input.buttons.pressed(MouseButton::Left);
    if !held || !(tab == LOCAL || tab == WORLD) {
        st.drag_from = None;
    } else if input.buttons.just_pressed(MouseButton::Left) {
        let on_map = pointer.filter(|_| !on_button(st, tab, pointer));
        st.drag_from = in_window(map_window(st, tab), on_map).and(on_map);
    } else if let (Some(from), Some(p)) = (st.drag_from, pointer) {
        st.drag += p - from;
        st.drag_from = Some(p);
    }
    let aid_drag = std::mem::take(&mut input.aid.drag);
    st.drag += aid_drag;
    let drag = std::mem::take(&mut st.drag);
    if aid_drag != Vec2::ZERO {
        info!("pipboy data: map dragged by ({:.0}, {:.0}), centre was {:?}", aid_drag.x, aid_drag.y, st.centre.get(tab).copied().flatten());
    }
    if drag != Vec2::ZERO && (tab == LOCAL || tab == WORLD) {
        let mag = st.magnify[tab].max(ZOOM.0);
        let size = map_of(st, game).map_or(Vec2::splat(2048.0), |m| Vec2::from(m.usable)) * mag;
        if let Some(c) = st.centre[tab].as_mut() {
            if tab == WORLD {
                c[0] = (c[0] - drag.x / size.x).clamp(0.0, 1.0);
                c[1] = (c[1] - drag.y / size.y).clamp(0.0, 1.0);
            } else {
                let scale = local_scale(game.location.as_ref().is_some_and(|l| l.interior.is_some()));
                c[0] -= drag.x * scale / mag;
                c[1] += drag.y * scale / mag;
            }
        }
    }
}

/// What the right pane shows now: the selected quest's objectives, the selected note or
/// challenge; `None` on the other tabs.
fn pane_key(st: &DataState, tab: usize, l: &Lists) -> Option<PaneKey> {
    let sel = st.sel[tab];
    let id = match tab {
        QUESTS => l.quests.get(sel).map(|q| q.form),
        NOTES if st.challenges => Some(sel as u32),
        NOTES => l.notes.get(sel).map(|n| n.form),
        _ => None,
    }?;
    Some((tab, tab == NOTES && st.challenges, id))
}

/// A page of the right pane: the objectives' window on Quests, a note's on Misc (the
/// windows 400 and 385 high [documented: map_menu.xml]; a page a window [design]); the window as
/// the page drawn from the file measured it, while it published one (`measured`).
fn pane_page(tab: usize, measured: Option<f32>) -> f32 {
    match measured {
        Some(h) if h > 0.0 => h,
        _ if tab == QUESTS => OBJECTIVES_H,
        _ => TEXT_H,
    }
}

/// Scroll the right pane (a note's text or a quest's objectives; one scroll for both):
/// it starts at the top when it shows another quest, note or tab (`shows`), moves a page a
/// Page Up/Down, [`TEXT_STEP`] a wheel notch and [`PANE_STICK_SPEED`] a second at the
/// right stick's full push (up scrolls back toward the top), and stays between its top
/// and how far it can go (`text_max`, from when it was last drawn).
fn scroll_pane(st: &mut DataState, tab: usize, shows: Option<PaneKey>, m: PaneMoves) {
    // A new pane starts at the top; this frame's moves wait until it is drawn and its
    // length known (`text_max` is still the last pane's).
    if shows != st.pane_of {
        st.pane_of = shows;
        st.text_first = 0.0;
        st.pane_stick = false;
        st.bar_drag = None;
        return;
    }
    let max = st.text_max.max(0.0);
    let before = st.text_first;
    if shows.is_some() {
        st.text_first += m.pages * pane_page(tab, st.pane_page) - m.notches * TEXT_STEP - m.stick * PANE_STICK_SPEED * m.dt + m.drag;
    }
    st.text_first = st.text_first.clamp(0.0, max);
    let what = match shows {
        Some((QUESTS, ..)) => "objectives",
        Some((_, true, _)) => "challenge text",
        _ => "note text",
    };
    if shows.is_some() && (m.pages != 0.0 || m.notches != 0.0) {
        info!("pipboy data: {what} scrolled from {before:.0} to {:.0} of {max:.0}", st.text_first);
    }
    let stick = shows.is_some() && m.stick != 0.0;
    if st.pane_stick && !stick {
        info!("pipboy data: {what} scrolled by the right stick to {:.0} of {max:.0}", st.text_first);
    }
    st.pane_stick = stick;
}

/// Up/Down (and the test aid's steps) move the selection, held at the list's ends. The
/// right pane is not touched here: it starts at the top only when it shows another quest
/// or note ([`scroll_pane`] by [`pane_key`]), so Up on the first row or Down on the last
/// keeps the place read.
fn step_rows(st: &mut DataState, tab: usize, moves: &[i32], len: usize) {
    for &by in moves {
        if len > 0 {
            st.sel[tab] = model::step(st.sel[tab], by, len);
            st.boxed = true;
            st.pointer_pick = false;
            st.navigated = st.navigated.wrapping_add(1);
        }
    }
}

/// The objectives' window drawn at `data` (canvas): 400 × 400 [documented: map_menu.xml].
fn objectives_rect(data: Vec2) -> Rect {
    Rect::from_corners(data, data + Vec2::new(LIST_W, OBJECTIVES_H))
}

/// The wheel scrolls the right pane when the pointer is over it: on Quests the
/// objectives' window as last drawn (`pane`), on Misc anywhere right of the note text's
/// left edge (as before the objectives scrolled); elsewhere it scrolls the list.
fn wheel_on_pane(tab: usize, pointer: Option<Vec2>, pane: Option<Rect>) -> bool {
    let Some(p) = pointer else { return false };
    match tab {
        QUESTS => pane.is_some_and(|r| r.contains(p)),
        NOTES => p.x >= MAIN.min.x + DATA_X + TEXT_AT.x,
        _ => false,
    }
}

/// A click on the right pane's scroll bar: an arrow steps as a wheel notch, the track a
/// page; a press on the marker holds it for [`drag_bar`].
fn bar_act(st: &mut DataState, b: BarPart, pointer: Option<Vec2>, pane: &mut PaneMoves) {
    match b {
        BarPart::Up => pane.notches += 1.0,
        BarPart::Down => pane.notches -= 1.0,
        BarPart::PageUp => pane.pages -= 1.0,
        BarPart::PageDown => pane.pages += 1.0,
        BarPart::Marker(scale) => st.bar_drag = pointer.map(|p| (p.y, scale)),
    }
}

/// The marker held (`held`: the left button still down) follows the pointer's own
/// movement, as the maps do (the pane is drawn again as it scrolls, which would end a
/// Bevy drag at once); let go, it stops.
fn drag_bar(st: &mut DataState, held: bool, pointer: Option<Vec2>, pane: &mut PaneMoves) {
    match (st.bar_drag, pointer) {
        (Some(_), _) if !held => {
            st.bar_drag = None;
            info!("pipboy data: scroll bar let go at {:.0} of {:.0}", st.text_first, st.text_max.max(0.0));
        }
        (Some((y, scale)), Some(p)) => {
            pane.drag += (p.y - y) * scale;
            st.bar_drag = Some((p.y, scale));
        }
        _ => {}
    }
}

/// A click or Enter on a row: a quest becomes the active one [hypothesis: the PC's
/// click does the controller's "Make Active Quest"]; a station is tuned to, or the radio
/// turned off on the one playing [hypothesis: "Tune"].
fn use_row(st: &mut DataState, tab: usize, i: usize, l: &Lists, game: &mut Game) {
    match tab {
        // A co-op guest's list is the host's log, read only: the host's active quest
        // stays the marked one [design].
        QUESTS if game.script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.coop_guest) => {
            info!("pipboy data: the host's quest log is read only here; the host's active quest stays marked");
        }
        QUESTS => {
            if let Some(q) = l.quests.get(i).filter(|q| !q.done) {
                st.active_quest = Some(q.form);
                info!("pipboy data: active quest {}", FormId(q.form));
                // Scripts read the player's pick too (not counted as a ForceActiveQuest).
                if let Some(rt) = game.script.as_mut().and_then(|s| s.rt.as_mut()) {
                    rt.game.ui.active_quest = Some(FormId(q.form));
                }
            }
        }
        RADIO => {
            let (Some((base, name)), Some(places)) = (l.stations.get(i), game.places.as_ref()) else { return };
            let Some(&station) = places.station_ref.get(base) else { return };
            let Some(rt) = game.script.as_mut().and_then(|s| s.rt.as_mut()) else { return };
            let p = &mut rt.game.radio.pipboy;
            if l.playing == Some(*base) {
                p.on = false;
                info!("pipboy data: radio off");
            } else {
                (p.on, p.station) = (true, Some(FormId(station)));
                info!("pipboy data: radio tuned to {name}");
            }
        }
        NOTES if l.challenges.is_none() => {
            // A Sound or Voice note plays from its start, or pauses and goes on when it
            // is the one playing (`crate::holotape`, holotapes.md §3).
            let Some(n) = l.notes.get(i).filter(|n| n.kind == 0 || n.kind == 3) else { return };
            info!("pipboy data: note {} ({}) picked", n.name, FormId(n.form));
            if let Some(h) = game.holotape.as_mut() {
                h.picks.push(FormId(n.form));
            }
        }
        _ => {}
    }
}

/// The question box's keys and clicks (fast-travel.md §3): Up and Down (W, S) move the
/// choice, Enter, Space or E take it, Y and N answer; the pointer chooses the answer it
/// is on and a click takes it. Nothing else on the page moves meanwhile.
fn popup_input(st: &mut DataState, input: &mut Input, game: &mut Game) {
    use KeyCode::*;
    input.wheel.clear();
    st.press = None;
    st.drag_from = None;
    input.aid.drag = Vec2::ZERO;
    let acts = std::mem::take(&mut st.acts);
    let Some(p) = st.popup.as_mut() else { return };
    let mut b = BoxInput::default();
    if input.keys.any_just_pressed([ArrowUp, KeyW]) {
        b.steps.push(-1);
    }
    if input.keys.any_just_pressed([ArrowDown, KeyS]) {
        b.steps.push(1);
    }
    b.confirm = input.keys.any_just_pressed([Enter, NumpadEnter, Space, KeyE]);
    if input.keys.just_pressed(KeyY) {
        b.answer = Some(0);
    }
    if input.keys.just_pressed(KeyN) {
        b.answer = Some(p.buttons.len().saturating_sub(1));
    }
    for m in input.menu.read() {
        match m.0 {
            Command::Up => b.steps.push(-1),
            Command::Down => b.steps.push(1),
            Command::One => b.confirm = true,
            _ => {}
        }
    }
    for a in acts {
        match a {
            Act::PopupOver(i) => b.pointer.push((i, true)),
            Act::PopupOut(i) => b.pointer.push((i, false)),
            Act::PopupPick(i) => b.answer = Some(i),
            _ => {}
        }
    }
    let Some(i) = box_frame(p, &mut st.popup_hold, st.pointer_now, &b) else { return };
    let (ask, word, travel) = (p.ask.clone(), p.buttons[i].clone(), p.take(i));
    st.popup = None;
    match ask {
        Ask::Marker(ask) => {
            info!("pipboy map: answered {word:?}");
            answer_marker(st, game, marker::answer(ask, i));
        }
        Ask::Travel(_) | Ask::Refused => {
            info!("fast travel: answered {word:?}");
            if let Some(m) = travel {
                input.go.write(FastTravelGo(m));
            }
        }
    }
}

/// How far the pointer moves before the answer under it takes the question box's choice
/// back from the keys (canvas units) [design].
const KEYED_STILL: f32 = 2.0;

/// The question box's pointer state, kept while the box is up.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct BoxHold {
    /// Where the pointer was when a key last moved the choice (`None`: no key holds it).
    keyed: Option<Option<Vec2>>,
    /// The answer the pointer is on (its last PointerOver not followed by its PointerOut),
    /// of the answers drawn now.
    hover: Option<usize>,
}

impl BoxHold {
    /// The box's answers were spawned again: the answer entity the pointer was on is
    /// despawned and its PointerOut never comes, so the hover is dropped; a pointer still on
    /// an answer enters the new one, and its PointerOver sets it again. Without this a
    /// pointer moved off onto blank space after a redraw kept the old answer's hover, and
    /// once the keys' hold let go that stale answer took the choice back [design].
    fn rebuilt(&mut self) {
        self.hover = None;
    }
}

/// What the question box was given in a frame.
#[derive(Debug, Clone, Default, PartialEq)]
struct BoxInput {
    /// Up −1, Down +1, in order.
    steps: Vec<i32>,
    /// Enter, Space, E or the menus' confirm: take the choice.
    confirm: bool,
    /// An answer taken outright: Y the first, N the last, a click on one.
    answer: Option<usize>,
    /// The pointer onto (`true`) or off an answer, in order.
    pointer: Vec<(usize, bool)>,
}

/// Move the question box's choice for the frame (`choose`), then the answer taken, if
/// any: one taken outright, else the choice as it stands after the frame's Up and Down
/// (Down and Enter in one frame take the answer below) [design].
fn box_frame(p: &mut Popup, hold: &mut BoxHold, pointer: Option<Vec2>, b: &BoxInput) -> Option<usize> {
    choose(p, hold, pointer, b);
    b.answer.filter(|&i| i < p.buttons.len()).or(b.confirm.then_some(p.selected))
}

/// Move the question box's choice: the answer under the pointer is followed from its
/// PointerOver and PointerOut events, the keys' `steps` move the choice, and the answer
/// under the pointer takes it unless a key moved it since the pointer last moved. The box
/// is drawn anew whenever its choice moves, and the answer under a pointer at rest is
/// entered again each time, so without that hold the keys' Down was taken back at once
/// (#1954's smoke run); once the pointer moves past [`KEYED_STILL`] the answer it is on
/// takes the choice, whether or not a fresh PointerOver came [design: as the item lists
/// hold the wheel's selection, `crate::menu` `WHEEL_HOLD_TRAVEL`].
fn choose(p: &mut Popup, hold: &mut BoxHold, pointer: Option<Vec2>, b: &BoxInput) {
    for &(i, onto) in &b.pointer {
        if onto {
            hold.hover = Some(i);
        } else if hold.hover == Some(i) {
            hold.hover = None;
        }
    }
    let still = |at: Option<Vec2>| match (at, pointer) {
        (Some(a), Some(b)) => a.distance(b) <= KEYED_STILL,
        (None, None) => true,
        _ => false,
    };
    if hold.keyed.is_some_and(|at| !still(at)) {
        hold.keyed = None;
    }
    for &by in &b.steps {
        p.step(by);
        hold.keyed = Some(pointer);
    }
    if hold.keyed.is_none() {
        if let Some(i) = hold.hover.filter(|&i| i < p.buttons.len()) {
            p.selected = i;
        }
    }
}

/// Place Marker: ask about the place under the highlight box ([`marker::ask`]) in the
/// question box, its words from the load order's settings, else NVForever's own
/// ([`marker::Words`]). Under the player's No Place Marker Popup tweak it answers at once
/// ([`marker::quick`]) [design: docs/design/tweaks.md].
fn place_marker(st: &mut DataState, tab: usize, game: &mut Game, shift: bool) {
    // A map that cannot be drawn now (no place, or a worldspace without map data: entered on
    // this frame, before the map was built) has nothing under its box, whatever another map left
    // there (#3324).
    let shown = game.location.is_some()
        && match tab {
            WORLD => map_of(st, game).is_some(),
            LOCAL => game.player.is_some(),
            _ => false,
        };
    if !shown {
        blank_map(st, tab);
    }
    let Some(at) = st.spot.get(tab).copied().flatten() else {
        info!("pipboy map: Place Marker: no place under the highlight box");
        return;
    };
    let quick = game.script.as_ref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game.tweaks).filter(|t| t.no_marker_popup).map(|t| marker::Quick::of(t.marker_shift_removes, t.marker_removed_on_it));
    if let Some(how) = quick {
        let box_at = st.cursor.get(tab).copied().flatten();
        let on_it = matches!((box_at, st.custom_on_map[tab]), (Some(b), Some(c)) if b.distance(c) <= SNAP);
        info!("pipboy map: Place Marker answers at once ({how:?}, tweak)");
        answer_marker(st, game, marker::quick(st.custom, at, how, shift, on_it));
        return;
    }
    let ask = marker::ask(st.custom, at);
    let (text, buttons) = marker::Words::of(ask, |(name, ours)| super::labels::label(name, ours).to_string());
    info!("pipboy map: Place Marker asks {text:?} ({} answers) for ({:.0}, {:.0}) in {}", buttons.len(), at.pos[0], at.pos[1], FormId(at.space));
    st.popup = Some(Popup::marker(ask, text, buttons));
}

/// A click on a map this frame ([`DataState::map_click`]), taken once the map is laid out for
/// good, so it reads the highlight box as drawn on the frame the button was let go (#3324):
/// - on the World Map with companions under the box and the player's Map Selectable
///   Companions tweak on, it summons them (returned for the caller to send) [design:
///   docs/design/tweaks.md];
/// - else on the World Map with a marker under the box it asks to travel there and makes it the
///   Travel target;
/// - else (the Local Map: anywhere) it drops the Travel target and asks Place Marker's question
///   of the place under the box ([`place_marker`]); a map with no place under the box (a
///   worldspace without map data) takes no click.
///
/// The click wins over Enter (or A, the Travel hint) in the same frame: the travel those asked
/// is dropped, or becomes the click's own, so one question is asked [design]. The original's
/// mouse gesture is a hypothesis (docs/handoff/observer-checks.md).
fn take_click(st: &mut DataState, game: &mut Game, shift: bool) -> Option<SummonCompanions> {
    let tab = st.map_click.take()?;
    if !(tab == LOCAL || tab == WORLD) || st.popup.is_some() {
        return None;
    }
    let name = if tab == WORLD { "world" } else { "local" };
    let under = if tab == WORLD { st.cursor_marker } else { None };
    if tab == WORLD && !st.companions_under.is_empty() {
        drop_travel(st);
        return Some(SummonCompanions(st.companions_under.clone()));
    }
    if tab == WORLD {
        st.travel_marker = under;
    }
    match under {
        Some(m) => st.travel_pending = Some(m),
        None => {
            drop_travel(st);
            if st.spot[tab].is_none() {
                info!("pipboy map: a click on the {name} map: no place under the highlight box");
            } else {
                info!("pipboy map: a click on the {name} map away from any marker: Place Marker");
                place_marker(st, tab, game, shift);
            }
        }
    }
    None
}

/// A fast-travel question asked this frame (Enter, A, the Travel hint) not sent: a click took
/// the frame ([`take_click`]).
fn drop_travel(st: &mut DataState) {
    if let Some(m) = st.travel_pending.take() {
        info!("pipboy data: fast travel to {} not asked: a click on the map took the frame", FormId(m));
    }
    st.travel_redrawn = false;
}

/// Do what the answer to Place Marker's question says, on the game's marker (the one
/// scripts and the save see) and the page's copy.
fn answer_marker(st: &mut DataState, game: &mut Game, a: marker::MarkerAnswer) {
    use marker::MarkerAnswer::*;
    let after = marker::apply(st.custom, a);
    st.custom = after;
    if let Some(rt) = game.script.as_mut().and_then(|s| s.rt.as_mut()) {
        rt.game.other.jg8.custom_marker = after.map(CustomMarker::to_game);
    }
    match a {
        Placed(m) => info!("pipboy map: custom marker placed at ({:.0}, {:.0}) in {}", m.pos[0], m.pos[1], FormId(m.space)),
        Moved(m) => info!("pipboy map: custom marker moved to ({:.0}, {:.0}) in {}", m.pos[0], m.pos[1], FormId(m.space)),
        Removed => info!("pipboy map: custom marker removed"),
        Left => info!("pipboy map: custom marker left where it was"),
        Declined => info!("pipboy map: no custom marker placed"),
    }
}

fn button(st: &mut DataState, k: ButtonKind, input: &mut Input) {
    match k {
        ButtonKind::NotesFilter => {
            st.notes_of_active = !st.notes_of_active;
            st.sel[NOTES] = 0;
            st.first[NOTES] = 0;
        }
        ButtonKind::ShowLocation => {
            input.pip_out.write(PipBoyInput::Tab(WORLD));
        }
        ButtonKind::Challenges => {
            st.challenges = !st.challenges;
            st.sel[NOTES] = 0;
            st.first[NOTES] = 0;
            st.text_first = 0.0;
            info!("pipboy data: {} shown", if st.challenges { "challenges" } else { "notes" });
        }
        // With the game, in the acts' loop ([`place_marker`]).
        ButtonKind::PlaceMarker => {}
        ButtonKind::NextChallenge => info!("pipboy data: {k:?} is not built"),
        // Taken as Enter in `take_input`.
        ButtonKind::Travel | ButtonKind::MakeActive | ButtonKind::PlayAudio | ButtonKind::Tune => {}
    }
}

fn map_of(st: &mut DataState, game: &Game) -> Option<WorldMap> {
    let world = game.location.as_ref()?.world;
    map_for(st, game, world)
}

/// The root's picture and this worldspace's own projection [design: read::world_map].
fn map_for(st: &mut DataState, game: &Game, world: u32) -> Option<WorldMap> {
    if let Entry::Vacant(e) = st.maps.entry(world) {
        let w = game.world.as_ref()?;
        e.insert(read::world_map(w.lo, w.schema, world));
    }
    st.maps.get(&world).cloned().flatten()
}

/// Gather what the page shows.
/// The page as it is now; `pointer` is the pointer over the page off its buttons, `aiming`
/// whether the pointer is in use at all ([`travel_target`]).
fn build(st: &mut DataState, tab: usize, l: &Lists, game: &Game, pointer: Option<Vec2>, aiming: bool) -> View {
    // Under the question box the map stands still.
    let pointer = pointer.filter(|_| st.popup.is_none());
    let place = place(st, game);
    let twelve = game.script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.pipboy_12_hour);
    let when = game.sim.as_ref().and_then(|s| s.world.calendar().ok()).map(|c| model::date_time_with(&c, twelve)).unwrap_or_default();
    let body = match tab {
        LOCAL | WORLD => Body::Map(Box::new(if tab == WORLD { world_map(st, game, pointer, aiming) } else { local_map(st, game, pointer) })),
        QUESTS => {
            let rows = l
                .quests
                .iter()
                .map(|q| Row { text: quest_name(game, q.form), mark: if Some(q.form) == st.active_quest { Mark::Filled } else { Mark::None }, done: q.done })
                .collect();
            let right = l.quests.get(st.sel[QUESTS]).map_or(Right::None, |q| Right::Objectives { rows: objectives(game, q.form), first: st.text_first });
            Body::List { rows, first: st.first[QUESTS], boxed: st.boxed.then_some(st.sel[QUESTS]), right }
        }
        NOTES if l.challenges.is_some() => {
            let list = l.challenges.as_deref().unwrap_or_default();
            let rows = list.iter().map(|c| Row { text: c.name.clone(), mark: Mark::None, done: c.completed }).collect();
            let right = list.get(st.sel[NOTES]).map_or(Right::None, |c| {
                let state = if c.completed { "Completed".to_string() } else { format!("{}/{}", c.progress, c.threshold) };
                // The player's tweak says a challenge repeats [design: docs/design/tweaks.md].
                let state = if c.recurring { format!("{state} (repeats)") } else { state };
                Right::Text { text: format!("{}\n\n{state}", c.description), first: st.text_first }
            });
            Body::List { rows, first: st.first[NOTES], boxed: st.boxed.then_some(st.sel[NOTES]), right }
        }
        NOTES => {
            let rows = l.notes.iter().map(|n| Row { text: n.name.clone(), mark: Mark::None, done: false }).collect();
            let right = match l.notes.get(st.sel[NOTES]) {
                Some(n) if n.kind == 1 => Right::Text { text: n.text.clone().unwrap_or_default(), first: st.text_first },
                Some(n) if n.kind == 2 => n.image.clone().map_or(Right::None, Right::Image),
                Some(n) => match game.holotape.as_ref().and_then(|h| h.progress()).filter(|p| p.0 == FormId(n.form)) {
                    Some((_, left, share)) => Right::Audio { left: (left * 10.0) as u32, played: (share * 1000.0) as u16 },
                    None => Right::Audio { left: 0, played: 1000 },
                },
                None => Right::None,
            };
            Body::List { rows, first: st.first[NOTES], boxed: st.boxed.then_some(st.sel[NOTES]), right }
        }
        _ => {
            let rows = l.stations.iter().map(|(b, n)| Row { text: n.clone(), mark: if l.playing == Some(*b) { Mark::Filled } else { Mark::None }, done: false }).collect();
            Body::List { rows, first: st.first[RADIO], boxed: st.boxed.then_some(st.sel[RADIO]), right: Right::Wave { on: l.playing.is_some() } }
        }
    };
    // The hints after the body: the World Map's marker under the highlight box is found
    // while its map is gathered.
    let tape = game.holotape.as_ref().map_or((None, false), |h| (h.playing, h.paused));
    let audio = audio_hint(l.notes.get(st.sel[NOTES]).filter(|_| l.challenges.is_none()), tape);
    let facts = HintFacts {
        pad: crate::pad::labels() != crate::pad::Labels::Keys,
        marker: st.travel_marker.is_some(),
        quests: !l.quests.is_empty(),
        no_show_location: game.script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.no_show_quest_location),
        quest_done: l.quests.get(st.sel[QUESTS]).map(|q| q.done),
        quests_read_only: game.script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.coop_guest),
        notes: l.has_notes,
        notes_of_active: st.notes_of_active,
        audio,
        challenges: l.challenges.is_some(),
        stations: !l.stations.is_empty(),
    };
    let buttons = tab_buttons(tab, &facts);
    View { tab, place, when, buttons, body, popup: st.popup.clone() }
}

/// The Misc tab's A hint for the selected note (`note`, none while the challenges show):
/// for a Sound or Voice note whether it reads Stop (`Some(true)`) or Play. Stop only for a
/// Voice note that is the holotape playing and not paused (`tape`: `Holotape::playing`,
/// `paused`). A Sound note's sound is played once and not followed (`crate::holotape`), so
/// its hint stays Play, which is what the press does again [design].
fn audio_hint(note: Option<&read::Note>, (playing, paused): (Option<FormId>, bool)) -> Option<bool> {
    match note? {
        n if n.kind == NOTE_VOICE => Some(playing == Some(FormId(n.form)) && !paused),
        n if n.kind == NOTE_SOUND => Some(false),
        _ => None,
    }
}

/// A note's type (`DATA`): 0 Sound, 3 Voice [documented: docs/spec-notes/holotapes.md].
const NOTE_SOUND: u8 = 0;
const NOTE_VOICE: u8 = 3;

/// What a tab's hints depend on ([`tab_buttons`]).
#[derive(Debug, Default, Clone, Copy)]
struct HintFacts {
    /// The hints are a controller's (`crate::pad::labels`, the Input Device), not the
    /// keyboard and mouse's.
    pad: bool,
    /// The World Map has a Travel target ([`travel_target`]).
    marker: bool,
    /// Quests are listed, and whether the selected one is done.
    quests: bool,
    quest_done: Option<bool>,
    /// The list is the host's log, read only (a co-op guest): the
    /// make-active hint is dim, as on a quest done [design].
    quests_read_only: bool,
    /// The player's tweak drops Show Location [design: docs/design/tweaks.md].
    no_show_location: bool,
    /// The player has a note (whatever the filter keeps); the notes' filter is on; the
    /// selected note is an audio note, and whether it plays; Misc shows the challenges.
    notes: bool,
    notes_of_active: bool,
    audio: Option<bool>,
    challenges: bool,
    /// The Pip-Boy receives a station here.
    stations: bool,
}

/// The names of the tabs, for the log.
const TAB_NAMES: [&str; 5] = ["local map", "world map", "quests", "misc", "radio"];

/// The A button's key: Enter, with a controller A (the shared menu buttons,
/// `crate::pad`).
fn a_key() -> String {
    crate::pad::hint(&[KeyCode::Enter], crate::pad::at_menu(fnv_game::game::menus::MAP))
}

/// The X button's key: X, with a controller X (`crate::pad::menu_buttons`' map entry,
/// #1897).
fn x_key() -> String {
    crate::pad::hint(&[KeyCode::KeyX], crate::pad::at_menu(fnv_game::game::menus::MAP))
}

/// A tab's hints at the right, top to bottom (#1897): the A, X and Y buttons of
/// `map_menu.xml` (`MM_ButtonA`, `MM_ButtonX`, `MM_ButtonY`), each under the one above
/// where that one shows (docs/spec-notes/pipboy.md §5.3) [documented: which shows when, and
/// each one's title by tab].
/// - A, with a controller only: World Map Travel; Quests the make-active hint when a quest
///   is listed; Misc Play (Stop while that note plays) on an audio note; Radio Tune when
///   a station is received.
/// - X: the maps' Place Marker with either device (with the keyboard and mouse the word
///   alone, a button to click [observed: the original, 2026-11-10]); Quests Show Location when a quest
///   is listed; Misc, when the player has a note, the notes' filter (Active Notes while
///   all show, All Notes while filtered) or, while the challenges show, Next Challenge.
/// - Y: Misc's challenges toggle, with either device [documented]; the file also names the
///   maps with a controller, but the original's controller screenshots show no Y hint on
///   either map [observed: issue #1897], so it is left out there.
///
/// The words are the settings' texts, else NVForever's labels for them in the game
/// strings table (`fnv_ui::strings`, `labels::built_in_label`); `sChallengeToggle` has no
/// label there and keeps NVForever's "Challenges". A hint that does nothing now is dim
/// [design]: Travel with no Travel target ([`travel_target`]), Next Challenge (not
/// built), the make-active hint on a quest done or with none selected. Place Marker is
/// bright: it asks about the custom marker (#1954).
fn tab_buttons(tab: usize, f: &HintFacts) -> Vec<Button> {
    use super::labels::{built_in_label, label};
    let b = |word, key, dim, kind| Button { hint: Hint::new(word, key, dim), kind };
    let a = match tab {
        WORLD => Some(b(built_in_label("sTravel"), a_key(), !f.marker, ButtonKind::Travel)),
        QUESTS if f.quests => Some(b(built_in_label("sMakeActiveQuest"), a_key(), f.quest_done != Some(false) || f.quests_read_only, ButtonKind::MakeActive)),
        NOTES => f.audio.map(|playing| b(built_in_label(if playing { "sStopAudio" } else { "sPlayAudio" }), a_key(), false, ButtonKind::PlayAudio)),
        RADIO if f.stations => Some(b(built_in_label("sTune"), a_key(), false, ButtonKind::Tune)),
        _ => None,
    };
    let x = match tab {
        // Both devices: with the keyboard and mouse the word alone, a button to click [observed:
        // the original's World Map and Local Map with no controller, `MM_ButtonX` visible and a
        // target, "Place Marker" at the top right with no key; capture 2026-11-10].
        LOCAL | WORLD => Some(b(built_in_label("sPlaceMarker"), if f.pad { x_key() } else { String::new() }, false, ButtonKind::PlaceMarker)),
        QUESTS if f.quests && !f.no_show_location => Some(b(built_in_label("sShowLocation"), x_key(), false, ButtonKind::ShowLocation)),
        NOTES if f.notes && f.challenges => Some(b(built_in_label("sNextChallenge"), x_key(), true, ButtonKind::NextChallenge)),
        NOTES if f.notes => {
            let filter = built_in_label(if f.notes_of_active { "sShowAllNotes" } else { "sShowActiveNotes" });
            Some(b(filter, x_key(), false, ButtonKind::NotesFilter))
        }
        _ => None,
    };
    let y = (tab == NOTES).then(|| b(label("sChallengeToggle", CHALLENGES), challenges_hint(), false, ButtonKind::Challenges));
    a.filter(|_| f.pad).into_iter().chain(x).chain(y).collect()
}

/// The challenges toggle's word (`MM_ButtonY`'s `sChallengeToggle`, which the masters do not
/// define): NVForever's own, the same while the challenges show [documented: one string
/// for both views; design: the word].
const CHALLENGES: &str = "Challenges";

fn quest_name(game: &Game, q: u32) -> String {
    game.script.as_ref().and_then(|s| s.rt.as_ref()).and_then(|rt| rt.game.data.quests.get(&FormId(q))).and_then(|d| d.name.clone()).unwrap_or_default()
}

/// The quest's objectives shown or done, with text: on a co-op guest, the host's.
fn objectives(game: &Game, q: u32) -> Vec<Row> {
    let Some(rt) = game.script.as_ref().and_then(|s| s.rt.as_ref()) else { return Vec::new() };
    let g = &rt.game;
    let Some(def) = g.data.quests.get(&FormId(q)) else { return Vec::new() };
    let shown: Vec<(i32, bool)> = match (&g.coop_host_quest, g.quests.get(&FormId(q))) {
        (Some(h), _) if g.coop_guest && h.quest.0 == q => h.objectives.iter().map(|(i, done)| (*i, *done)).collect(),
        // Another quest of the host's: its objectives as the host's story carries them,
        // else as the host's quest view last showed them.
        _ if g.coop_guest => match g.coop_host_story.quests.get(&FormId(q)).filter(|s| !s.objectives.is_empty()) {
            Some(s) => s.objectives.iter().map(|(i, done)| (*i, *done)).collect(),
            None => g.coop_host_objectives.get(&FormId(q)).map(|o| o.iter().map(|(i, done)| (*i, *done)).collect()).unwrap_or_default(),
        },
        (_, Some(state)) => state.objectives.iter().filter(|(_, o)| o.displayed || o.completed).map(|(i, o)| (*i, o.completed)).collect(),
        (_, None) => Vec::new(),
    };
    let mut lines: Vec<ObjectiveLine> = shown
        .into_iter()
        .filter_map(|(i, done)| {
            def.objectives.get(&i)?;
            // SetObjectiveText's first (`fnv_game::nvse_jip10`).
            let text = g.expand_codes(&g.objective_text(FormId(q), i)?);
            (!text.trim().is_empty()).then(|| ObjectiveLine { index: i, text: text.trim().to_string(), done })
        })
        .collect();
    model::sort_objectives(&mut lines);
    let mut rows: Vec<Row> = lines.into_iter().map(|o| Row { text: o.text, mark: if o.done { Mark::Filled } else { Mark::Empty }, done: o.done }).collect();
    if let Some(line) = claimed_line(g, FormId(q)) {
        rows.push(Row { text: line, mark: Mark::None, done: false });
    }
    rows
}

/// A co-op guest's quest whose stages it claimed this session (joint progression,
/// `fnv_game::coop_joint`, coop-shared-progress.md §3.5): a last line under its objectives saying they also
/// count in this character's own save; a quest it does not sync (its line sat out:
/// docs/design/coop-quest-sync.md §3) says so instead [design: our own words].
fn claimed_line(g: &fnv_game::game::Game, q: FormId) -> Option<String> {
    if !g.coop_guest {
        return None;
    }
    if g.player.quest_sync.on == Some(true) && !g.coop_takes_quest(Some(q)) {
        return Some(fnv_util::words::tr("Sitting out: not shared with your own save").to_string());
    }
    let n = g.player.claims.stages.get(&q).map_or(0, |s| s.len());
    (n > 0).then(|| fnv_util::words::tr("Shared quest progress: also counts in your own save").to_string())
}

/// The heading's place name, kept per cell.
fn place(st: &mut DataState, game: &Game) -> String {
    let (Some(loc), Some(w)) = (game.location.as_ref(), game.world.as_ref()) else { return String::new() };
    let feet = game.player.as_ref().map_or([0.0; 3], |p| p.feet);
    let grid = if loc.interior.is_some() { (0, 0) } else { ((feet[0] / model::CELL).floor() as i32, (feet[1] / model::CELL).floor() as i32) };
    st.places.entry((loc.world, loc.interior, grid)).or_insert_with(|| read::place_name(w.lo, w.schema, w.index, loc.world, loc.interior, feet)).clone()
}

/// Where the player's arrow goes on the map of `world`: where the player is, or outdoors
/// where they were last [hypothesis].
fn player_at(st: &DataState, game: &Game, world: u32) -> Option<([f32; 3], f32)> {
    let (loc, player) = (game.location.as_ref()?, game.player.as_ref()?);
    if loc.interior.is_none() && loc.world == world {
        return Some((player.feet, player.heading));
    }
    st.outdoors.filter(|o| o.0 == world).map(|o| (o.1, o.2))
}

/// The pointer in the hand-built map's window, when it is over it.
#[cfg(test)]
fn in_map(pointer: Option<Vec2>) -> Option<Vec2> {
    in_window(hand_built_window(), pointer)
}

/// The pointer in the map window `window` (canvas units), when it is over it.
fn in_window(window: Rect, pointer: Option<Vec2>) -> Option<Vec2> {
    MapView::bare(window).pointer(pointer)
}

/// The hand-built maps' window on the canvas: 50 down in the menus' rectangle, 855 × 500
/// [documented: map_menu.xml].
fn hand_built_window() -> Rect {
    Rect::from_corners(MAIN.min + MAP.min, MAIN.min + MAP.max)
}

/// The map window the model reads on tab `tab`: the page drawn from the file's, laid out, while
/// it published one for this tab ([`DataState::publish_map`]); else the hand-built page's. With
/// the file's numbers the two are the same.
fn map_window(st: &DataState, tab: usize) -> Rect {
    st.map_window.filter(|w| w.0 == tab).map_or_else(hand_built_window, |w| w.1)
}

/// Move the map cursor `at` by the right stick `v` for `dt` seconds, kept in the window;
/// the stick's up is the screen's up, but the window's y runs down. Returns the cursor
/// and how far it was pushed past an edge (that much pans the map, [`scroll_map`]).
#[cfg(test)]
fn stick_cursor(at: Vec2, v: Vec2, dt: f32) -> (Vec2, Vec2) {
    MapView::bare(hand_built_window()).stick(at, v, dt, CURSOR_SPEED)
}

/// Zoom `mag` by the left stick's up and down `y` (up positive, in; down out) for `dt`
/// seconds, smoothly while held, between [`ZOOM`].
fn stick_zoom(mag: f32, y: f32, dt: f32) -> f32 {
    (mag * ZOOM_STEP.powf(y * ZOOM_STICK_STEPS * dt)).clamp(ZOOM.0, ZOOM.1)
}

/// Pan the map's centre by `d` window units: the cursor pushed past an edge moves the
/// view toward it (the opposite of a drag, which moves the picture by hand).
fn scroll_map(st: &mut DataState, tab: usize, game: &Game, d: Vec2) {
    if d == Vec2::ZERO {
        return;
    }
    let mag = st.magnify[tab].max(ZOOM.0);
    let world_size = if tab == WORLD {
        map_of(st, game).map_or(Vec2::splat(2048.0), |m| Vec2::from(m.usable)) * mag
    } else {
        Vec2::ZERO
    };
    let scale = if tab == LOCAL {
        local_scale(game.location.as_ref().is_some_and(|l| l.interior.is_some()))
    } else {
        0.0
    };
    let Some(c) = st.centre[tab].as_mut() else { return };
    if tab == WORLD {
        c[0] = (c[0] + d.x / world_size.x).clamp(0.0, 1.0);
        c[1] = (c[1] + d.y / world_size.y).clamp(0.0, 1.0);
    } else {
        c[0] += d.x * scale / mag;
        c[1] -= d.y * scale / mag;
    }
}

/// Whether the pointer is over one of the tab's buttons at the right, as last drawn
/// ([`button_boxes`]): there it is the buttons' pointer, never the map's (#1897).
fn on_button(st: &DataState, tab: usize, pointer: Option<Vec2>) -> bool {
    let (drawn, rects) = &st.button_rects;
    *drawn == tab && pointer.is_some_and(|p| rects.iter().any(|r| r.contains(p)))
}

/// The marker the Travel hint, Enter and the controller's A travel to (#1897): the one
/// under the highlight box; while the pointer is in use (`aiming`), else the last one the
/// box was on, kept while the pointer goes on to the Travel hint (the box does not follow
/// it over the buttons, [`on_button`]) until a click on the map away from any marker
/// ([`travel_ask`]); always one still shown (`shown`) [design].
fn travel_target(kept: Option<u32>, under: Option<u32>, aiming: bool, shown: &[u32]) -> Option<u32> {
    under.or(kept.filter(|_| aiming)).filter(|m| shown.contains(m))
}

/// The marker the World Map asks to travel to this frame by `travel` (Enter, the controller's
/// A, the Travel hint's click): the Travel target ([`travel_target`]). A click on a map (the
/// left button let go, `mouse.1`, near where it went down, `mouse.0`) sets
/// [`DataState::map_click`], resolved once the map is drawn against the highlight box as
/// drawn on that frame ([`take_click`], #1897, #3324): a marker under it asks to travel there,
/// none asks Place Marker's question. A press on a button at the right is the button's, never
/// a map click; a press that moved more than [`CLICK_SLOP`] before it was let go is a drag.
fn travel_ask(st: &mut DataState, tab: usize, pointer: Option<Vec2>, (pressed, released): (bool, bool), travel: bool) -> Option<u32> {
    if !(tab == WORLD || tab == LOCAL) {
        st.press = None;
        return None;
    }
    let over_button = on_button(st, tab, pointer);
    if pressed {
        st.press = in_window(map_window(st, tab), pointer).filter(|_| !over_button).and(pointer).map(|p| (p, 0.0));
    } else if let (Some((from, moved)), Some(p)) = (st.press.as_mut(), pointer) {
        *moved = moved.max(p.distance(*from));
    }
    if released {
        if let Some((_, moved)) = st.press.take().filter(|_| !over_button) {
            if moved <= CLICK_SLOP && in_window(map_window(st, tab), pointer).is_some() {
                st.map_click = Some(tab);
            }
        }
    }
    st.travel_marker.filter(|_| travel && tab == WORLD)
}

/// The highlight box: at the pointer while it is over the map, else where it was left
/// (the window's middle at first); on a marker within reach it takes the marker's place
/// and name [hypothesis: the owner's screenshots show it away from the middle, once round
/// a door with the door's place name].
fn place_cursor(st: &mut DataState, tab: usize, pointer: Option<Vec2>, marks: &mut [MapMark], names: &[String]) -> (Vec2, Option<String>, Option<usize>) {
    let win = MapView::bare(map_window(st, tab));
    let want = win.pointer(pointer).or(st.cursor[tab]).unwrap_or(win.mid());
    st.cursor[tab] = Some(want);
    match nearest(marks.iter().map(|m| m.at), want) {
        Some(i) => {
            marks[i].highlighted = true;
            (marks[i].at, names.get(i).cloned(), Some(i))
        }
        None => (want, None, None),
    }
}

/// The marker the highlight box at `want` takes ([`place_cursor`]): the nearest of those at
/// `places` (the window's units) within [`SNAP`], the first of equals.
pub(crate) fn nearest(places: impl Iterator<Item = Vec2>, want: Vec2) -> Option<usize> {
    places.enumerate().map(|(i, at)| (i, at.distance(want))).filter(|(_, d)| *d <= SNAP).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i)
}

/// Whether the player knows a marker (`Some`), and whether it is drawn as discovered
/// (`true`): found within its radius, made a travel target by ShowMap's or
/// ShowAllMapMarkers' canTravel (owner 2026-10-23: `tmm 1` shows every marker with its
/// own icon), explicitly enabled as a travel target by a script's `SetMapMarkerTravel 1`
/// once visible/shown, or authored with both Visible and Can Travel To (`FNAM=3`, drawn
/// as discovered with its own icon [hypothesis: mirrors observed `GetMapMarkerVisible=2`
/// / `GetMapMarkerTravel=1` script values from observer check 252 (#3468) and the
/// 2026-11-22 live map-marker probe without an observed Pip-Boy icon capture]). The
/// authored Can Travel To flag (`FNAM` bit 2) alone does not mark an undiscovered marker
/// as known or discovered. Known also when shown by ShowMap, ShowAllMapMarkers (which
/// leaves out markers flagged "Show All" Hidden, FNAM bit 4 or `SetMapMarkerHidden`
/// [documented: xEdit's flag names, schemas/fnv/world.json], when asked to) or by its
/// Visible flag (`FNAM` bit 1 or `SetMapMarkerVisible`) [hypothesis]; those keep the
/// undiscovered icon.
fn known(st: &DataState, game: &Game, m: &read::Marker) -> Option<bool> {
    let rt = game.script.as_ref().and_then(|s| s.rt.as_ref());
    let id = FormId(m.form);
    let flags = rt.and_then(|rt| rt.game.marker_flags(id)).unwrap_or(m.flags);
    let hidden = flags & fnv_game::nvse_lu2::MARKER_HIDDEN != 0;
    let visible_flag = flags & fnv_game::nvse_lu2::MARKER_VISIBLE != 0;
    let script_visible = rt.and_then(|rt| rt.game.marker_flag_override(id, fnv_game::nvse_lu2::MARKER_VISIBLE));
    let script_travel = rt.and_then(|rt| rt.game.marker_flag_override(id, fnv_game::nvse_lu2::MARKER_TRAVEL));
    let shown = rt.is_some_and(|rt| rt.game.map_marker_shown(id, hidden));
    let enabled = game.sim.as_ref().is_none_or(|s| s.present(m.form));
    let discovered = st.discovered.contains(&m.form);
    let visible = match script_visible {
        Some(false) => shown,
        _ => discovered || shown || visible_flag,
    };
    if !(enabled && visible) {
        return None;
    }
    let travel = if let Some(rt) = rt.filter(|rt| rt.game.marker_flags(id).is_some()) {
        if script_travel == Some(false) {
            false
        } else {
            discovered || rt.game.marker_travel(id)
        }
    } else {
        let by_script = shown
            && script_travel != Some(false)
            && rt.is_some_and(|rt| {
                let p = &rt.game.player;
                p.travel_markers.contains(&id) || p.all_markers.is_some_and(|a| a.shown && a.travel)
            });
        discovered || by_script || (m.flags & 3 == 3)
    };
    Some(travel)
}

fn marker_kind(rt: Option<&crate::script::Runtime>, m: &read::Marker) -> u8 {
    rt.and_then(|rt| rt.game.marker_kind(FormId(m.form))).unwrap_or(m.kind)
}

fn marker_custom_icon(rt: Option<&crate::script::Runtime>, m: &read::Marker) -> Option<String> {
    rt.and_then(|rt| rt.game.custom_map_marker_icon(FormId(m.form))).map(str::to_string)
}

fn marker_name(rt: Option<&crate::script::Runtime>, m: &read::Marker) -> String {
    rt.and_then(|rt| rt.game.map_marker_name_override(FormId(m.form))).map(str::to_string).unwrap_or_else(|| m.name.clone())
}

fn marker_reputation(rt: Option<&crate::script::Runtime>, m: &read::Marker) -> Option<(u32, String)> {
    match rt.and_then(|rt| rt.game.map_marker_reputation_override(FormId(m.form))) {
        Some(Some(rep)) => {
            let g = &rt?.game;
            if g.data.sig(rep) != Some(*b"REPU") {
                return None;
            }
            let name = g.data.full_names.get(&rep).filter(|s| !s.trim().is_empty())?;
            Some((rep.0, name.clone()))
        }
        Some(None) => None,
        None => m.reputation.clone(),
    }
}

/// The original's companions on the World Map: their names, in font 2 [documented: Stewie's
/// Tweaks' readme, bCompanionsUseLocationFontOnMap, "Vanilla is hardcoded to use font 2"],
/// left off within a distance of the player [documented: the same readme,
/// bNoMinCompanionMapDistance; hypothesis: the distance, [`COMPANION_MIN_DISTANCE`]] and while
/// knocked out [documented: the same readme, bMapShowUnconsciousCompanions]. The original's
/// map file has no companion template and the archives no companion icon [observed:
/// map_menu.xml, the texture archives' list], so they are text only.
const COMPANION_FONT: usize = 2;
/// The font the highlight box names locations in.
const LOCATION_FONT: usize = 1;
/// Within this distance of the player (game units, same worldspace) a companion is not
/// written [hypothesis: about a cell, so the companions at the player's side do not cover
/// the arrow].
pub const COMPANION_MIN_DISTANCE: f32 = 4096.0;
/// How far above its place a companion's name stands (canvas units) [design].
pub(crate) const COMPANION_NAME_RISE: f32 = 14.0;
/// How near the highlight box's middle a companion's place must be for a click to summon it,
/// under the player's Map Selectable Companions tweak (canvas units) [design].
const SUMMON_REACH: f32 = 24.0;

/// The World Map asks for these companions beside the player (the player's Map Selectable
/// Companions tweak); `summon_companions` does it.
#[derive(Message, Clone, Debug, PartialEq)]
pub struct SummonCompanions(pub Vec<FormId>);

/// Put the companions the World Map asked for beside the player
/// (`fnv_game::Game::summon_teammates`), and log them.
pub(crate) fn summon_companions(mut asks: MessageReader<SummonCompanions>, script: Option<ResMut<ScriptState>>, sim: Option<ResMut<SimState>>) {
    let asked: Vec<FormId> = asks.read().flat_map(|a| a.0.iter().copied()).collect();
    if asked.is_empty() {
        return;
    }
    let (Some(rt), Some(mut sim)) = (script.and_then(|s| s.into_inner().rt.as_mut()), sim) else { return };
    let done = crate::script::with_world(rt, &mut sim, |rt| rt.game.summon_teammates(&asked));
    let names: Vec<String> = done.iter().map(|f| rt.game.data.name(*f)).collect();
    info!("pipboy map: summoned {} beside the player (Map Selectable Companions)", if names.is_empty() { "nobody".to_string() } else { names.join(", ") });
}

/// A companion written on the World Map.
struct MapCompanion {
    form: FormId,
    world: u32,
    pos: [f32; 3],
    name: String,
    down: bool,
}

/// The player's companions (teammates) in an exterior cell, with their worldspace; those
/// within [`COMPANION_MIN_DISTANCE`] of `player` (worldspace, place) left out when it is
/// given, and the knocked-out ones unless `show_down`. Unmaterialized placed teammates
/// fall back to their placement cell and position (`sim.refs`), as `quest_targets` does.
fn map_companions(game: &Game, player: Option<(u32, [f32; 3])>, show_down: bool) -> Vec<MapCompanion> {
    let (Some(rt), Some(sim)) = (game.script.as_ref().and_then(|s| s.rt.as_ref()), game.sim.as_ref()) else { return Vec::new() };
    let mut out = Vec::new();
    for id in rt.game.teammates() {
        let (cell, pos) = match sim.world.peek(id) {
            Some(s) => (s.cell, s.pos),
            None => match sim.refs.get(id) {
                Some(p) => (p.cell, p.pos),
                None => continue,
            },
        };
        let Some(world) = cell.and_then(|c| sim.refs.cell_world.get(&c)).map(|w| w.0) else { continue };
        let down = rt.game.knockout(id).is_some();
        if (down && !show_down) || !sim.present(id.0) {
            continue;
        }
        if player.is_some_and(|(w, p)| w == world && (pos[0] - p[0]).hypot(pos[1] - p[1]) < COMPANION_MIN_DISTANCE) {
            continue;
        }
        let name = sim.ref_name(Some(&rt.game), id.0).map(str::to_string).unwrap_or_else(|| rt.game.data.name(id));
        out.push(MapCompanion { form: id, world, pos, name, down });
    }
    out
}

fn world_map(st: &mut DataState, game: &Game, pointer: Option<Vec2>, aiming: bool) -> MapShown {
    let Some(world) = game.location.as_ref().map(|l| l.world) else { return blank_map(st, WORLD) };
    let Some(m) = map_of(st, game) else { return blank_map(st, WORLD) };
    let mag = st.magnify[WORLD].max(ZOOM.0);
    let player = player_at(st, game, world);
    let centre = *st.centre[WORLD].get_or_insert_with(|| player.map_or([0.5, 0.5], |(p, _)| m.uv(p[0], p[1])));
    // The one viewport both drawings paint from and the pointer is read in.
    let view = MapView::world(map_window(st, WORLD), centre, Vec2::from(m.usable), mag);
    let (left, size) = (view.left, view.size);
    let on_map = |uv: [f32; 2]| view.to_window(uv);
    let mut marks = Vec::new();
    let mut names = Vec::new();
    let mut forms = Vec::new();
    let mut details = Vec::new();
    let rt = game.script.as_ref().and_then(|s| s.rt.as_ref());
    let show_distance = rt.is_some_and(|rt| rt.game.tweaks.map_marker_distance);
    let mut total = 0;
    let mut drawn_other = 0;
    let red_recent = rt.is_some_and(|rt| rt.game.tweaks.recent_markers_red);
    let rep_off = rt.is_some_and(|rt| rt.game.tweaks.hide_marker_reputation);
    let all = markers(st, game).cloned().unwrap_or_default();
    for mk in &all {
        let Some(projection) = map_for(st, game, mk.world).filter(|p| p.root == m.root) else { continue };
        total += 1;
        let Some(found) = known(st, game, mk) else { continue };
        drawn_other += usize::from(mk.world != world);
        let recent = red_recent && st.found_this_run.contains(&mk.form);
        let uv = projection.uv(mk.pos[0], mk.pos[1]);
        let kind = marker_kind(rt, mk);
        let custom_icon = marker_custom_icon(rt, mk);
        let label_name = marker_name(rt, mk);
        let marker_rep = marker_reputation(rt, mk);
        let detail = marker_rep.as_ref().map(|(rep, name)| model::reputation_line(name, standing(game, *rep)));
        let (name, tile_detail) = if st.tile_rows { (label_name.clone(), detail.clone()) } else { (String::new(), None) };
        marks.push(MapMark { icon: Some((kind, found)), custom_icon, highlighted: false, at: on_map(uv), form: mk.form, uv: Some(uv), name, detail: tile_detail, recent, outside: None });
        names.push(label_name);
        forms.push(mk.form);
        // The player's tweak leaves the reputation line out [design: docs/design/tweaks.md].
        let rep = marker_rep.as_ref().filter(|_| !rep_off).map(|(rep, name)| model::reputation_line(name, standing(game, *rep)));
        // The player's tweak: how far the marker is, on the same line [design: docs/design/tweaks.md].
        let far = player.filter(|_| show_distance && mk.world == world).map(|(p, _)| model::distance_text(((mk.pos[0] - p[0]).powi(2) + (mk.pos[1] - p[1]).powi(2)).sqrt()));
        details.push(match (rep, far) {
            (Some(r), Some(d)) => Some(format!("{r}, {d}")),
            (r, d) => r.or(d),
        });
    }
    let mut quests = Vec::new();
    let mut targets = Vec::new();
    let mut target_uvs = Vec::new();
    for t in quest_targets(st, game).into_iter().filter(|t| t.interior.is_none() && t.world_map) {
        let Some(projection) = map_for(st, game, t.world).filter(|p| p.root == m.root) else { continue };
        let uv = projection.uv(t.pos[0], t.pos[1]);
        let at = on_map(uv);
        targets.push(at);
        target_uvs.push((t.form, uv));
        quests.push(quest_corner(at));
    }
    let drawn: Vec<u32> = target_uvs.iter().map(|t| t.0).collect();
    if st.world_quests_logged.as_ref() != Some(&drawn) {
        let list: Vec<String> = drawn.iter().map(|&f| FormId(f).to_string()).collect();
        info!("pipboy data: world map draws {} quest marker(s): [{}]", drawn.len(), list.join(", "));
        st.world_quests_logged = Some(drawn);
    }
    // The custom marker, placed in any worldspace of this map's family.
    let custom_uv = st.custom.and_then(|c| {
        let projection = map_for(st, game, c.space).filter(|p| p.root == m.root)?;
        Some(projection.uv(c.pos[0], c.pos[1]))
    });
    let custom = st.custom.zip(custom_uv).map(|(c, uv)| (c, on_map(uv)));
    log_custom(st, WORLD, custom.map(|c| c.0));
    st.custom_on_map[WORLD] = custom.map(|c| c.1);
    let drawn_found = marks.iter().filter(|mk| matches!(mk.icon, Some((_, true)))).count();
    let logged = (world, total, marks.len(), drawn_found, drawn_other);
    if st.world_logged != Some(logged) {
        st.world_logged = Some(logged);
        info!("pipboy data: world map shows {} of {total} markers of this map family, {drawn_found} as discovered; map family {}, {drawn_other} shown from other worldspaces", marks.len(), FormId(m.root));
    }
    let (cursor, label, under) = place_cursor(st, WORLD, pointer, &mut marks, &names);
    let detail = under.and_then(|i| details[i].clone());
    if let Some(i) = under.filter(|&i| st.cursor_marker != Some(forms[i])) {
        info!("pipboy data: world map highlights {} ({}){}", names[i], FormId(forms[i]), detail.as_ref().map(|d| format!(", reputation line \"{d}\"")).unwrap_or_default());
    }
    st.cursor_marker = under.map(|i| forms[i]);
    st.travel_marker = travel_target(st.travel_marker, st.cursor_marker, aiming, &forms);
    // Place Marker's place: the position under the highlight box's middle, in this
    // worldspace (on a marker, the marker's) [observed: the original puts it there].
    let uv = [(cursor.x - left.x) / size.x, (cursor.y - left.y) / size.y];
    let spot = m.position(uv);
    st.spot[WORLD] = spot.iter().all(|v| v.is_finite()).then(|| CustomMarker { space: world, pos: [spot[0], spot[1], player.map_or(0.0, |(p, _)| p[2])] });
    let arrow_uv = player.map(|(p, h)| (m.uv(p[0], p[1]), h));
    let arrow = arrow_uv.map(|(uv, _)| on_map(uv));
    let (nodes, lines) = cached_waypoints(&mut st.waypoints_cache, arrow, custom.map(|c| c.1), &targets, mag);
    // The lines' far ends as [`waypoints`] makes them: with the arrow shown, the custom marker's
    // then each quest target's.
    let ends = if arrow.is_some() { custom_uv.map(|uv| (LineTo::Marker, 0, uv)).into_iter().chain(target_uvs.iter().map(|&(f, uv)| (LineTo::Quest, f, uv))).collect() } else { Vec::new() };
    let kinds: Vec<LineTo> = lines.iter().map(|l| l.0).collect();
    if st.lines_logged.as_ref() != Some(&(kinds.clone(), st.custom)) {
        for (to, pieces) in lines.iter() {
            info!("pipboy map: dotted line to {} ({} segments)", to.name(), pieces.len());
        }
        if lines.is_empty() && st.lines_logged.is_some() {
            info!("pipboy map: no dotted line");
        }
        st.lines_logged = Some((kinds, st.custom));
    }
    // The player's companions out in this map's worldspaces [see `map_companions`].
    let tw = game.script.as_ref().and_then(|s| s.rt.as_ref()).map(|rt| (rt.game.tweaks.no_min_companion_distance, rt.game.tweaks.map_unconscious_companions, rt.game.tweaks.companions_location_font));
    let (no_min, show_down, location_font) = tw.unwrap_or_default();
    let summon = game.script.as_ref().and_then(|s| s.rt.as_ref()).map(|rt| (rt.game.tweaks.map_summon_companions, rt.game.tweaks.summon_unconscious));
    let mut companions = Vec::new();
    let mut companion_uvs = Vec::new();
    let mut forms_shown = Vec::new();
    for c in map_companions(game, player.filter(|_| !no_min).map(|(p, _)| (world, p)), show_down) {
        let Some(projection) = map_for(st, game, c.world).filter(|p| p.root == m.root) else { continue };
        let uv = projection.uv(c.pos[0], c.pos[1]);
        forms_shown.push((c.form, c.down));
        companions.push((on_map(uv), c.name.clone(), c.down));
        companion_uvs.push((c.form.0, uv, c.name, c.down));
    }
    // The player's Map Selectable Companions tweak: the companions under the highlight box,
    // which a click summons [design: docs/design/tweaks.md].
    st.companions_under = match summon {
        Some((true, down_too)) => companions
            .iter()
            .zip(&forms_shown)
            .filter(|((at_, _, _), (_, down))| at_.distance(cursor) <= SUMMON_REACH && (!down || down_too))
            .map(|(_, (f, _))| *f)
            .collect(),
        _ => Vec::new(),
    };
    if st.companions_logged != Some(companions.len()) {
        st.companions_logged = Some(companions.len());
        info!("pipboy data: world map shows {} companions", companions.len());
    }
    let companion_font = if location_font { LOCATION_FONT } else { COMPANION_FONT };
    MapShown {
        companions,
        companion_font,
        picture: Some((m.texture.clone(), Rect::from_corners(left, left + size))),
        outline: None,
        reset_cells: Arc::default(),
        marks,
        player: arrow.zip(player).map(|(a, (_, h))| (a, h)),
        quests,
        custom: custom.map(|c| quest_corner(c.1)),
        nodes,
        lines,
        cursor,
        label,
        detail,
        world: Some(WorldParts {
            view,
            texture: m.texture.clone(),
            mag,
            quests: target_uvs,
            custom: custom_uv,
            arrow: arrow_uv,
            ends,
            outline: None,
            rows: None,
            want: st.cursor[WORLD].unwrap_or(view.mid()),
            companions: companion_uvs,
            companion_font: companion_font as u8,
        }),
    }
}

/// The arrow in `glow_cursor.dds` (32 × 64 texels): a pointer in its upper half, its tip at
/// (9, 9) and its base from (24, 23) to (9, 29), an isosceles triangle whose axis, base middle
/// to tip, runs `ARROW_AXIS` texels (left and up) [documented: the texels in the load order's
/// archive, the opaque core read at alpha 200].
const ARROW_AXIS: Vec2 = Vec2::new(-7.5, -17.0);

/// The part of `glow_cursor.dds` an arrow `w` × `h` shows: the file draws it at its texels' size
/// times its zoom, `w` × 3.5 (1.4 times at the 40 wide arrow), from its top left, cut to the
/// box [documented: `MM_WorldMapCursor`'s `zoom`]; so the hand-built page shows the same part at
/// the same scale, the arrow's shape undistorted (until 2026-11-12 it stretched the whole
/// picture over the box) [design].
fn arrow_texels(w: f32, h: f32) -> Vec2 {
    Vec2::new(w, h) / (w * 3.5 / 100.0)
}

/// The turn of the player's arrow for heading `heading` (radians, clockwise from north): the
/// heading and the turn that brings the picture's arrow, its tip to the left of up unturned
/// ([`ARROW_AXIS`], about 0.416), to point up; so it points the way the player faces, up at
/// heading 0 as the original's [observed: a capture of the original's World Map at 1280x720,
/// 2026-11, heading 0: a sharp symmetric triangle pointing up]. Both drawings turn it so, the
/// hand-built page and the file's `MM_WorldMapCursor`/`MM_LocalMapCursor`, about the box's
/// middle (the file's axis traits). Until 2026-11-12 the hand-built arrow was turned by the
/// heading itself, its tip some 24 degrees left of the way the player faced, and stretched.
pub(crate) fn arrow_turn(heading: f32) -> f32 {
    heading + (-ARROW_AXIS.x).atan2(-ARROW_AXIS.y)
}

/// The player's standing with reputation `rep` (its title), when the scripts' game runs.
fn standing(game: &Game, rep: u32) -> Option<&'static str> {
    let rt = game.script.as_ref().and_then(|s| s.rt.as_ref())?;
    let [infamy, fame] = rt.game.player.reputation.get(&FormId(rep)).copied().unwrap_or_default();
    let scale = rt.game.data.reputations.get(&FormId(rep)).copied().unwrap_or(0.0) as f64;
    Some(crate::reputation::title(fnv_game::functions::reputation_level(fame, scale), fnv_game::functions::reputation_level(infamy, scale)))
}

/// A map that cannot be drawn (no place, or a worldspace without map data): nothing under its
/// highlight box, so Place Marker and a click find no place left from another map, and no
/// marker to travel to (#3324).
fn blank_map(st: &mut DataState, tab: usize) -> MapShown {
    st.spot[tab] = None;
    st.custom_on_map[tab] = None;
    if tab == WORLD {
        st.cursor_marker = None;
        st.travel_marker = None;
        st.companions_under.clear();
    }
    empty_map()
}

fn empty_map() -> MapShown {
    MapShown { picture: None, outline: None, reset_cells: Arc::default(), marks: Vec::new(), player: None, quests: Vec::new(), custom: None, nodes: Arc::default(), lines: Arc::default(), cursor: MAP_MID, label: None, detail: None, world: None, companions: Vec::new(), companion_font: COMPANION_FONT }
}

/// The local map's scale, game units to a canvas unit at a magnification of 1: outdoors
/// [`LOCAL_SCALE`], indoors closer ([`outline::INTERIOR_SCALE`]).
fn local_scale(interior: bool) -> f32 {
    if interior {
        outline::INTERIOR_SCALE
    } else {
        LOCAL_SCALE
    }
}

/// How long the Local Map's outline may cast rays each frame, and how many at a time.
const OUTLINE_BUDGET: std::time::Duration = std::time::Duration::from_millis(6);
const OUTLINE_BATCH: usize = 256;

/// Make the Local Map's outline of the place (a grid of rays down through the physics
/// world, [`outline`]; outdoors a ray landing nowhere takes the cell's landscape height)
/// a few hundred rays a frame, and its picture when it is done; a new one when the map is
/// panned off it or the player is elsewhere.
fn advance_outline(st: &mut DataState, game: &mut Game, images: &mut Assets<Image>) {
    let (Some(loc), Some(player)) = (game.location.as_ref(), game.player.as_ref()) else { return };
    let (interior, world, feet) = (loc.interior, loc.world, player.feet);
    let scale = local_scale(interior.is_some());
    let centre = st.centre[LOCAL].unwrap_or([feet[0], feet[1]]);
    let key = Outline::key_for(interior, world, centre, feet[2], MAP.size(), scale);
    if st.outline.as_ref().is_none_or(|o| o.key != key) {
        let old = st.outline.replace(Outline::new(interior, world, centre, feet[2], MAP.size(), scale));
        // The picture on screen stays until the new one is made, if it shows the same place.
        if let Some(old) = old.filter(|o| o.image.is_some()) {
            st.outline_last = Some(old);
        }
        st.outline_last = st.outline_last.take().filter(|o| same_place(&o.key, &key));
        st.outline_land.clear();
    }
    let Some(o) = st.outline.as_mut().filter(|o| !o.done()) else { return };
    let lands = &mut st.outline_land;
    let data = game.world.as_ref().filter(|_| interior.is_none()).map(|w| (w.lo, w.schema, w.index));
    let grid = data.and_then(|(_, _, index)| index.worldspaces.get(&world)).map(|ws| &ws.grid);
    let mut ground = |x: f32, y: f32| -> Option<f32> {
        let ((lo, schema, index), grid) = (data?, grid?);
        let g = ((x / model::CELL).floor() as i32, (y / model::CELL).floor() as i32);
        // The surface the loaded collision has too, so the outline does not change where
        // the rays leave the loaded cells.
        let surface = lands.entry(g).or_insert_with(|| {
            let ids = fnv_world::land_surface::land_ids(g, |n| grid.get(&n).and_then(|c| index.cells.get(c)).and_then(|c| c.land));
            fnv_world::land_surface::load_surface(lo, schema, &ids, crate::smooth_terrain::spec())
        });
        surface.as_ref()?.height_at(x - g.0 as f32 * model::CELL, y - g.1 as f32 * model::CELL)
    };
    let Some(phys) = game.phys.as_mut() else { return };
    let player_key = phys.player.as_ref().map(|c| c.key());
    phys.world.update_queries();
    let q = phys.world.queries();
    let mut filter = fnv_phys::Filter { exclude_layers: fnv_phys::layers::character_ignores().union(fnv_phys::layers::actor_volumes()), no_dynamic: true, ..Default::default() };
    filter.exclude.extend(player_key);
    let started = std::time::Instant::now();
    while !o.done() && started.elapsed() < OUTLINE_BUDGET {
        o.advance(OUTLINE_BATCH, |p, reach| q.ray(p, [0.0, 0.0, -1.0], reach, &filter).map(|h| h.point[2]).or_else(|| ground(p[0], p[1])));
    }
    if o.done() {
        o.image = Some(images.add(o.picture()));
        st.outline_last = None;
        info!(
            "pipboy data: local map outline {}x{} samples {} apart, {} landed ({})",
            o.size[0],
            o.size[1],
            o.step,
            o.hits(),
            if o.interior { "interior" } else { "exterior" }
        );
    }
}

/// Whether two outlines' keys are of the same place and floor (the interior, the worldspace
/// and the feet's step), whatever their anchors.
fn same_place(a: &outline::OutlineKey, b: &outline::OutlineKey) -> bool {
    a.0 == b.0 && a.1 == b.1 && a.2[2] == b.2[2]
}

/// The outline the Local Map shows: the one being made once its picture is, until then the last
/// one finished of the same place ([`DataState::outline_last`]).
fn shown_outline(st: &DataState) -> Option<&Outline> {
    st.outline.as_ref().filter(|o| o.image.is_some()).or_else(|| st.outline_last.as_ref().filter(|l| st.outline.as_ref().is_none_or(|o| same_place(&l.key, &o.key))))
}

/// The local map: the player's surroundings at [`local_scale`], the outline of the place,
/// the doors leading elsewhere and the known map markers [design: the scale; the outline
/// stands for the original's picture from above].
fn local_map(st: &mut DataState, game: &Game, pointer: Option<Vec2>) -> MapShown {
    let (Some(loc), Some(player)) = (game.location.as_ref(), game.player.as_ref()) else { return blank_map(st, LOCAL) };
    let (feet, heading) = (player.feet, player.heading);
    let mag = st.magnify[LOCAL].max(ZOOM.0);
    let centre = *st.centre[LOCAL].get_or_insert([feet[0], feet[1]]);
    let scale = local_scale(loc.interior.is_some());
    // The frame the picture covers: the shown outline's grid (made before this, `advance_outline`;
    // while a new one is made, the last one's), the one being made, or the one it would make.
    let frame = match shown_outline(st).or(st.outline.as_ref()) {
        Some(o) => (o.origin, Vec2::new(o.size[0] as f32, o.size[1] as f32) * o.step),
        None => Outline::frame_for(loc.interior, loc.world, centre, feet[2], MAP.size(), scale),
    };
    // The one viewport both drawings paint from and the pointer is read in.
    let view = MapView::local(map_window(st, LOCAL), view::LocalProjection { centre, mag, scale, origin: frame.0, extent: frame.1 });
    let on_map = |x: f32, y: f32| view.place(x, y);
    let outline = shown_outline(st).and_then(|o| {
        let image = o.image.clone()?;
        let top_left = on_map(o.origin[0], o.origin[1]);
        let size = Vec2::new(o.size[0] as f32, o.size[1] as f32) * o.step * mag / scale;
        Some((image, Rect::from_corners(top_left, top_left + size)))
    });
    let outline_texels = shown_outline(st).and_then(|o| Some((o.image.clone()?, Vec2::new(o.size[0] as f32, o.size[1] as f32))));
    let extent = view.extent();
    let inside = |p: Vec2| p.x >= -MARKER && p.y >= -MARKER && p.x <= extent.x + MARKER && p.y <= extent.y + MARKER;
    let grid = ((centre[0] / model::CELL).floor() as i32, (centre[1] / model::CELL).floor() as i32);
    let key = (loc.interior, if loc.interior.is_some() { (0, 0) } else { grid });
    if let Entry::Vacant(e) = st.doors.entry(key) {
        let doors = match (game.world.as_ref(), game.sim.as_ref()) {
            (Some(w), Some(sim)) => {
                let cells: Vec<u32> = match loc.interior {
                    Some(c) => vec![c],
                    None => {
                        let g = w.index.worldspaces.get(&loc.world).map(|ws| &ws.grid);
                        (-LOCAL_CELLS.0..=LOCAL_CELLS.0).flat_map(|dx| (-LOCAL_CELLS.1..=LOCAL_CELLS.1).map(move |dy| (grid.0 + dx, grid.1 + dy))).filter_map(|k| g.and_then(|g| g.get(&k)).copied()).collect()
                    }
                };
                read::doors(w.lo, w.schema, w.index, sim.refs, &cells)
            }
            _ => Vec::new(),
        };
        info!("pipboy data: local map: {} doors leading elsewhere", doors.len());
        e.insert(doors);
    }
    // The player's tweak: the squares around that will reset at their next load, tinted
    // [design: docs/design/tweaks.md].
    let rt = game.script.as_ref().and_then(|s| s.rt.as_ref());
    let reset_cells: Vec<Rect> = match (rt.filter(|rt| rt.game.tweaks.local_map_reset_cells), loc.interior, game.world.as_ref()) {
        (Some(rt), None, Some(w)) => {
            let g = w.index.worldspaces.get(&loc.world).map(|ws| &ws.grid);
            (-LOCAL_CELLS.0 - 1..=LOCAL_CELLS.0 + 1)
                .flat_map(|dx| (-LOCAL_CELLS.1 - 1..=LOCAL_CELLS.1 + 1).map(move |dy| (grid.0 + dx, grid.1 + dy)))
                .filter(|k| g.and_then(|g| g.get(k)).is_some_and(|&c| rt.game.cell_expired(FormId(c)) == Some(true)))
                .map(|(x, y)| {
                    let (x0, y0) = (x as f32 * model::CELL, y as f32 * model::CELL);
                    Rect::from_corners(on_map(x0, y0 + model::CELL), on_map(x0 + model::CELL, y0))
                })
                .collect()
        }
        _ => Vec::new(),
    };
    let mut rows = Vec::new();
    let mut marks = Vec::new();
    let mut names = Vec::new();
    // A door disabled now (a script's Disable, an enable parent off) is not drawn
    // [hypothesis: as a disabled marker is not, `known`].
    let enabled = |form: u32| game.sim.as_ref().is_none_or(|s| s.present(form));
    // The player's tweak tells doors leading outside from those leading in [design:
    // docs/design/tweaks.md].
    let door_kinds = game.script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.tweaks.map_door_kinds);
    for d in st.doors.get(&key).into_iter().flatten().filter(|d| enabled(d.form)) {
        let at = on_map(d.pos[0], d.pos[1]);
        let uv = Some(view.local_uv(d.pos[0], d.pos[1]));
        if inside(at) {
            marks.push(MapMark { icon: None, custom_icon: None, highlighted: false, at, form: d.form, uv, name: String::new(), detail: None, recent: false, outside: door_kinds.then_some(d.outside) });
            names.push(d.name.clone());
        }
        if st.tile_rows {
            rows.push(MapMark { icon: None, custom_icon: None, highlighted: false, at, form: d.form, uv, name: d.name.clone(), detail: None, recent: false, outside: door_kinds.then_some(d.outside) });
        }
    }
    if loc.interior.is_none() {
        let all = markers(st, game).cloned().unwrap_or_default();
        for mk in all.iter().filter(|mk| mk.world == loc.world) {
            let Some(found) = known(st, game, mk) else { continue };
            let at = on_map(mk.pos[0], mk.pos[1]);
            let uv = Some(view.local_uv(mk.pos[0], mk.pos[1]));
            let kind = marker_kind(rt, mk);
            let custom_icon = marker_custom_icon(rt, mk);
            let label_name = marker_name(rt, mk);
            if inside(at) {
                marks.push(MapMark { icon: Some((kind, found)), custom_icon: custom_icon.clone(), highlighted: false, at, form: mk.form, uv, name: String::new(), detail: None, recent: false, outside: None });
                names.push(label_name.clone());
            }
            if st.tile_rows {
                rows.push(MapMark { icon: Some((kind, found)), custom_icon, highlighted: false, at, form: mk.form, uv, name: label_name, detail: None, recent: false, outside: None });
            }
        }
    }
    // Quest markers stay inside the window [documented: map_menu.xml
    // LocalMapQuestMarkerTemplate, clamped to the view rectangle].
    let targets: Vec<Target> = quest_targets(st, game).into_iter().filter(|t| t.interior == loc.interior && (t.interior.is_some() || t.world == loc.world)).collect();
    let quests = targets.iter().map(|t| local_quest_corner(on_map(t.pos[0], t.pos[1]))).collect();
    let target_uvs = targets.iter().map(|t| (t.form, view.local_uv(t.pos[0], t.pos[1]))).collect();
    let (cursor, label, under) = place_cursor(st, LOCAL, pointer, &mut marks, &names);
    // The page drawn from the file keeps every door and marker here as its rows, those panned out
    // of the window too (its window cuts them), the one under the highlight box marked.
    let picked = under.map(|i| marks[i].form);
    for r in &mut rows {
        r.highlighted = Some(r.form) == picked;
    }
    // Place Marker's place: the point under the highlight box's middle, in this interior
    // or worldspace; the custom marker shows when it is in the space shown, clamped
    // inside the window as the quest markers are [hypothesis: their template]. No dotted
    // line here [observed: the original's Local Map has no waypoint nodes].
    let space = loc.interior.unwrap_or(loc.world);
    let spot = view.local_point(cursor).unwrap_or([centre[0], centre[1]]);
    st.spot[LOCAL] = Some(CustomMarker { space, pos: [spot[0], spot[1], feet[2]] });
    let custom = st.custom.filter(|c| c.space == space);
    log_custom(st, LOCAL, custom);
    st.custom_on_map[LOCAL] = custom.map(|c| on_map(c.pos[0], c.pos[1]));
    let custom_uv = custom.map(|c| view.local_uv(c.pos[0], c.pos[1]));
    let custom = custom.map(|c| local_quest_corner(on_map(c.pos[0], c.pos[1])));
    let world = WorldParts {
        view,
        texture: String::new(),
        mag,
        quests: target_uvs,
        custom: custom_uv,
        arrow: Some((view.local_uv(feet[0], feet[1]), heading)),
        ends: Vec::new(),
        outline: outline_texels,
        rows: st.tile_rows.then_some(rows),
        want: st.cursor[LOCAL].unwrap_or(view.mid()),
        companions: Vec::new(),
        companion_font: COMPANION_FONT as u8,
    };
    MapShown { picture: None, outline, reset_cells: Arc::new(reset_cells), marks, player: Some((on_map(feet[0], feet[1]), heading)), quests, custom, nodes: Arc::default(), lines: Arc::default(), cursor, label, detail: None, world: Some(world) , companions: Vec::new(), companion_font: COMPANION_FONT }
}

/// A quest marker's top left for a target at `at` in the window.
fn quest_corner(at: Vec2) -> Vec2 {
    at - Vec2::new(QUEST_MARKER, QUEST_MARKER * QUEST_ASPECT) / 2.0 + QUEST_NUDGE
}

/// A Local Map quest marker's top left, kept inside the window [documented: map_menu.xml
/// LocalMapQuestMarkerTemplate, clamped to the view rectangle].
fn local_quest_corner(at: Vec2) -> Vec2 {
    let (w, h) = (QUEST_MARKER, QUEST_MARKER * QUEST_ASPECT);
    let c = quest_corner(at);
    Vec2::new(c.x.clamp(0.0, MAP.width() - w), c.y.clamp(0.0, MAP.height() - 0.75 * h))
}

/// The World Map's waypoint nodes and dotted lines (#1954): with the player's arrow
/// shown, a line from it to the custom marker and one to each quest target shown, each
/// with a node at both ends [observed: the original's World Map with a marker placed
/// shows two chains from the arrow's place, one to the marker and one to the quest
/// target; hypothesis: one line to each of several targets]. `mag`: the map's
/// magnification.
fn waypoints(arrow: Option<Vec2>, custom: Option<Vec2>, quests: &[Vec2], mag: f32) -> (Vec<Vec2>, Vec<(LineTo, Vec<Piece>)>) {
    let Some(from) = arrow else { return (Vec::new(), Vec::new()) };
    let ends: Vec<(LineTo, Vec2)> = custom.map(|c| (LineTo::Marker, c)).into_iter().chain(quests.iter().map(|&q| (LineTo::Quest, q))).collect();
    if ends.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let nodes = std::iter::once(from).chain(ends.iter().map(|e| e.1)).collect();
    let lines = ends.iter().map(|&(to, at)| (to, marker::dotted_line(from, at, mag, PATH_TILE))).collect();
    (nodes, lines)
}

/// What the World Map's waypoints depend on: the arrow, the custom marker, the quest
/// targets (window units) and the magnification.
type WaypointKey = (Option<Vec2>, Option<Vec2>, Vec<Vec2>, f32);
/// The World Map's waypoint nodes and dotted lines ([`waypoints`]), shared so that a frame
/// whose waypoints did not move copies no vectors.
type Waypoints = (Arc<Vec<Vec2>>, Arc<Vec<(LineTo, Vec<Piece>)>>);

/// [`waypoints`], worked out again only when what they depend on moved (the World Map is
/// gathered every frame while it is open) [design].
fn cached_waypoints(cache: &mut Option<(WaypointKey, Waypoints)>, arrow: Option<Vec2>, custom: Option<Vec2>, quests: &[Vec2], mag: f32) -> Waypoints {
    let fresh = cache.as_ref().is_none_or(|(k, _)| k.0 != arrow || k.1 != custom || k.2 != quests || k.3 != mag);
    if fresh {
        let (nodes, lines) = waypoints(arrow, custom, quests, mag);
        *cache = Some(((arrow, custom, quests.to_vec(), mag), (Arc::new(nodes), Arc::new(lines))));
    }
    cache.as_ref().map(|c| c.1.clone()).unwrap_or_default()
}

/// Log where map `tab` shows the custom marker (`shown`: the one shown there, if any)
/// whenever that changes.
fn log_custom(st: &mut DataState, tab: usize, shown: Option<CustomMarker>) {
    let key = shown.map(|c| (c.pos[0].round() as i32, c.pos[1].round() as i32));
    if st.custom_logged[tab] == Some(key) {
        return;
    }
    let first = st.custom_logged[tab].is_none();
    st.custom_logged[tab] = Some(key);
    let map = if tab == WORLD { "world map" } else { "local map" };
    match (shown, first) {
        (Some(c), _) => info!("pipboy map: {map} shows the custom marker at ({:.0}, {:.0})", c.pos[0], c.pos[1]),
        (None, false) => info!("pipboy map: {map} shows no custom marker"),
        (None, true) => {}
    }
}

/// Where a quest target is: in an interior (`interior`), or outdoors in `world`; and what
/// stands there (the target's reference, or the door's into its interior).
struct Target {
    interior: Option<u32>,
    world: u32,
    pos: [f32; 3],
    form: u32,
    /// Drawn on the World Map too (outdoors); `false` for the door of the player's place
    /// that starts the way to a target elsewhere, which only the Local Map shows.
    world_map: bool,
}

/// How often the active quest's target conditions are evaluated, in seconds [hypothesis:
/// the original's rate is not documented; quests-dialogue.md §2].
const TARGET_POLL: f32 = 0.25;

/// Evaluate the active quest's target conditions (`Game::check_quest_targets`) four times
/// a second, and log each target that is marked or loses its mark.
pub(crate) fn refresh_quest_targets(time: Res<Time<Real>>, state: Option<ResMut<DataState>>, mut script: Option<ResMut<ScriptState>>, sim: Option<ResMut<SimState>>) {
    let (Some(mut st), Some(mut sim)) = (state, sim) else { return };
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    st.target_poll -= time.delta_secs();
    if st.target_poll > 0.0 {
        return;
    }
    st.target_poll = TARGET_POLL;
    // A co-op guest marks the host's quest (`Game::coop_host_quest`).
    let quest = if rt.game.coop_guest { rt.game.coop_host_quest.as_ref().map(|h| h.quest) } else { st.active_quest.map(FormId).or(rt.game.ui.active_quest) };
    // The conditions read the world played (`crate::script::with_world`).
    let checks = quest.map(|q| crate::script::with_world(rt, &mut sim, |rt| rt.game.check_quest_targets(q))).unwrap_or_default();
    if quest != st.target_quest {
        st.target_quest = quest;
        info!("quest targets: evaluating {} ({} target(s))", quest.map_or("no quest".into(), |q| rt.game.data.name(q)), checks.len());
    }
    let key = |c: &fnv_game::quest_targets::TargetCheck| (c.quest, c.objective, c.index);
    for c in &checks {
        let before = st.target_checks.iter().find(|o| key(o) == key(c)).map(|o| o.shown);
        if before != Some(c.shown) {
            let d = rt.game.data;
            let name = rt.game.world.placed.get(c.target).and_then(|p| p.edid.clone()).unwrap_or_else(|| d.name(c.target));
            let what = if c.shown { "marked" } else { "not marked (its conditions fail)" };
            info!("quest targets: {} objective {} target {} {name} {what}", d.name(c.quest), c.objective, c.target);
        }
    }
    st.target_checks = checks;
}

/// The targets of the active quest's objectives shown and not done whose conditions pass
/// (`refresh_quest_targets`), where they are now; a target indoors also at a door outdoors
/// for the World Map (`quest_targets::outdoor_marker_door`: a door straight into its
/// interior, one not locked unless the target ignores locks, else the door outdoors that
/// starts the way through other interiors, #2458) [hypothesis: as the owner's screenshots
/// show the marker of "Talk to Sunny Smiles in the Prospector Saloon" outdoors in
/// Goodsprings]; and a target in another place than the player's also at the door of the
/// player's place that starts the way there, for the Local Map, as the compass marks it
/// (`crate::compass`) [hypothesis: observer-checks.md question 179].
fn quest_targets(st: &mut DataState, game: &Game) -> Vec<Target> {
    let mut out = Vec::new();
    let (Some(q), Some(sim)) = (st.active_quest, game.sim.as_ref()) else { return out };
    let Some(rt) = game.script.as_ref().and_then(|s| s.rt.as_ref()) else { return out };
    let refs = sim.refs;
    let here = game.location.as_deref().map(|l| match l.interior {
        Some(c) => QuestPlace::Interior(FormId(c)),
        None => QuestPlace::World(FormId(l.world)),
    });
    let checks: Vec<_> = st.target_checks.iter().filter(|c| c.quest.0 == q && c.shown).copied().collect();
    let graph = st.door_graph.get_or_insert_with(|| DoorGraph::build(refs));
    // A door's place now: its interior (or `None`), its worldspace, where it stands.
    let door_at = |d: FormId| {
        let pos = sim.world.peek(d).map(|s| s.pos).or_else(|| refs.get(d).map(|p| p.pos))?;
        let cell = refs.get(d)?.cell?;
        Some(match refs.cell_world.get(&cell) {
            Some(w) => (None, w.0, pos),
            None => (Some(cell.0), 0, pos),
        })
    };
    for c in checks {
        let r = c.target;
        let (cell, pos) = match sim.world.peek(r) {
            Some(s) => (s.cell, s.pos),
            None => match refs.get(r) {
                Some(p) => (p.cell, p.pos),
                None => continue,
            },
        };
        let Some(cell) = cell else { continue };
        let there = QuestPlace::of_cell(refs, cell);
        let mut outdoors = None;
        match refs.cell_world.get(&cell) {
            Some(w) => out.push(Target { interior: None, world: w.0, pos, form: r.0, world_map: true }),
            None => {
                out.push(Target { interior: Some(cell.0), world: 0, pos, form: r.0, world_map: true });
                let direct = st.doors_into.entry(cell.0).or_insert_with(|| rt.game.doors_into(cell));
                outdoors = fnv_game::quest_targets::outdoor_marker_door(&sim.world, graph, direct, cell, c.ignores_locks);
                if let Some((d, (None, w, p))) = outdoors.and_then(|d| Some((d, door_at(d)?))) {
                    out.push(Target { interior: None, world: w, pos: p, form: d.0, world_map: true });
                }
            }
        }
        let first = here.filter(|&h| h != there).and_then(|h| graph.first_door(&sim.world, h, there, c.ignores_locks));
        if let Some((d, (interior, w, p))) = first.filter(|&d| Some(d) != outdoors).and_then(|d| Some((d, door_at(d)?))) {
            out.push(Target { interior, world: w, pos: p, form: d.0, world_map: false });
        }
        let key = (c.quest.0, c.objective, c.index);
        if st.target_doors_logged.get(&key) != Some(&(outdoors, first)) {
            st.target_doors_logged.insert(key, (outdoors, first));
            let world_map = match outdoors.and_then(|d| Some((d, door_at(d)?))) {
                Some((d, (_, w, p))) => format!("at door {d} in worldspace {} ({:.0}, {:.0})", FormId(w), p[0], p[1]),
                None if refs.cell_world.contains_key(&cell) => format!("itself ({:.0}, {:.0})", pos[0], pos[1]),
                None => "nowhere (no way outdoors)".to_string(),
            };
            let local = match first {
                Some(d) => format!("at door {d}, the first of the way there"),
                None if here == Some(there) => "itself".to_string(),
                None => "nowhere (no way from here)".to_string(),
            };
            info!("quest targets: {r} in cell {cell} marked on the world map {world_map}; on the local map {local}");
        }
    }
    out
}

// ---- drawing ------------------------------------------------------------------------------

/// A row of a tab's list (tab, row).
#[derive(Component, Clone, Copy)]
struct RowHit(usize, usize);

/// A button at the right.
#[derive(Component, Clone, Copy)]
struct ButtonHit(ButtonKind);

/// The map's window, dragged to pan.
#[derive(Component, Clone, Copy)]
struct MapHit;

/// An answer of the question box (0-based).
#[derive(Component, Clone, Copy)]
struct PopupHit(usize);

fn on_popup_over(e: On<PointerOver>, hits: Query<&PopupHit>, state: Option<ResMut<DataState>>) {
    if let (Ok(h), Some(mut s)) = (hits.get(e.entity), state) {
        s.acts.push(Act::PopupOver(h.0));
    }
}

fn on_popup_out(e: On<PointerOut>, hits: Query<&PopupHit>, state: Option<ResMut<DataState>>) {
    if let (Ok(h), Some(mut s)) = (hits.get(e.entity), state) {
        s.acts.push(Act::PopupOut(h.0));
    }
}

fn on_popup_click(e: On<PointerClick>, hits: Query<&PopupHit>, state: Option<ResMut<DataState>>) {
    if let (Ok(h), Some(mut s)) = (hits.get(e.entity), state) {
        s.acts.push(Act::PopupPick(h.0));
    }
}

/// A row the pointer moved onto (`super::hover_takes`, #2310): one redrawn under a resting
/// pointer as the keys or the D-pad step leaves their choice alone.
fn on_row_over(e: On<PointerOver>, rows: Query<&RowHit>, state: Option<ResMut<DataState>>, moved: Option<Res<super::PointerMoved>>) {
    if !super::hover_takes(moved.as_deref()) {
        return;
    }
    if let (Ok(r), Some(mut s)) = (rows.get(e.entity), state) {
        s.acts.push(Act::Hover(r.0, r.1));
    }
}

fn on_row_out(e: On<PointerOut>, rows: Query<&RowHit>, state: Option<ResMut<DataState>>) {
    if let (Ok(r), Some(mut s)) = (rows.get(e.entity), state) {
        s.acts.push(Act::Leave(r.0, r.1));
    }
}

fn on_row_click(e: On<PointerClick>, rows: Query<&RowHit>, state: Option<ResMut<DataState>>) {
    if let (Ok(r), Some(mut s)) = (rows.get(e.entity), state) {
        s.acts.push(Act::Use(r.0, r.1));
    }
}

fn on_button_click(e: On<PointerClick>, buttons: Query<&ButtonHit>, state: Option<ResMut<DataState>>) {
    if let (Ok(b), Some(mut s)) = (buttons.get(e.entity), state) {
        s.acts.push(Act::Button(b.0));
    }
}

/// A part of the right pane's scroll bar ([`bar_spots`]).
#[derive(Component, Clone, Copy)]
struct BarHit(BarPart);

fn on_bar_click(e: On<PointerClick>, hits: Query<&BarHit>, state: Option<ResMut<DataState>>) {
    if let (Ok(h), Some(mut s)) = (hits.get(e.entity), state) {
        s.acts.push(Act::Bar(h.0));
    }
}

/// The marker is taken when the button goes down on it, to be dragged ([`drag_bar`]).
fn on_bar_press(e: On<PointerPress>, hits: Query<&BarHit>, state: Option<ResMut<DataState>>) {
    if e.button != PointerButton::Primary {
        return;
    }
    if let (Ok(h), Some(mut s)) = (hits.get(e.entity), state) {
        s.acts.push(Act::Bar(h.0));
    }
}

fn show(p: &mut ChildSpawnerCommands, c: &mut Ctx, st: &mut DataState, v: &View) {
    heading(p, c, &v.place, &v.when);
    match &v.body {
        Body::Map(m) => map(p, c, m),
        Body::List { rows, first, boxed, right } => {
            st.fit[v.tab] = list(p, c, MAIN.min + LIST.min, LIST.height(), true, rows, *first, *boxed, Some(v.tab));
            // Under the hints' rect when any shows, a button's height higher on the tabs
            // up to Misc while the A hint does not show [documented: `MM_DataRect` y].
            let a_shown = v.buttons.first().is_some_and(|b| b.kind.is_a());
            let below = if v.buttons.is_empty() { 0.0 } else { BUTTON_RECT_H };
            let data_y = BUTTONS.y + below - if v.tab <= NOTES && !a_shown { button_height(c) } else { 0.0 };
            let data = MAIN.min + Vec2::new(DATA_X, data_y);
            // Only a note's text and the objectives scroll; the other panes leave none.
            st.text_max = 0.0;
            st.pane_rect = None;
            st.pane_page = None;
            match right {
                Right::None => {}
                Right::Objectives { rows, first } => {
                    st.text_max = objectives_pane(p, c, data, rows, *first);
                    st.pane_rect = Some(objectives_rect(data));
                }
                Right::Text { text, first } => st.text_max = note_text(p, c, data + TEXT_AT, text, *first),
                Right::Image(path) => {
                    c.image(p, path, at(data.x + DATA_W - IMAGE, data.y + TEXT_AT.y, IMAGE, IMAGE), c.bright());
                }
                Right::Audio { left, played } => audio_meter(p, c, data + Vec2::new(0.0, TEXT_AT.y), *left, *played),
                Right::Wave { on } => wave(p, c, *on),
            }
        }
    }
    // The hints after the body: a later sibling is drawn and picked over an earlier one, so
    // a click on a hint over the map's window (the World Map's Travel, the maps' Place
    // Marker) is the hint's, never the map's (`MapHit`, its markers) [design]. The
    // question box comes last, over everything.
    buttons(p, c, &v.buttons);
    if let Some(q) = &v.popup {
        popup(p, c, q, MAIN.min + MAP.min + MAP_MID);
    }
}

/// Marks the question box drawn over the page drawn from its file.
#[derive(Component)]
pub struct PopupOverTiles;

/// The question box (fast travel's, Place Marker's) over the DATA page drawn from `map_menu.xml`,
/// which has no hand-built body to hold it (`crate::tiles::map_menu::MapTiles::ready`): the same
/// box ([`popup`]) in the middle of the map window as laid out, in the Pip-Boy's image above the
/// page's tiles, its answers taking the pointer as the hand-built box's do; the page stays drawn
/// from its tiles under it [design]. Its input is the model's (`popup_input`); the page's tile
/// input waits while it is up (`crate::tiles::map_menu::MapTiles::popup_owns`). Spawned again when
/// the box changes (its choice moves), which forgets the answer under the pointer as the hand-built
/// drawing does ([`BoxHold::rebuilt`]).
#[allow(clippy::too_many_arguments)]
pub fn popup_over_tiles(
    mut commands: Commands,
    screen: Option<Res<PipBoyScreen>>,
    tiles: Option<Res<crate::tiles::map_menu::MapTiles>>,
    pip: Res<PipBoy>,
    state: Option<ResMut<DataState>>,
    roots: Query<Entity, With<PopupOverTiles>>,
    mut tools: Tools,
    mut drawn: Local<Option<(Popup, Rect)>>,
) {
    // Laid out in the Pip-Boy's screen texture, at its size whatever the UI scale (#1631).
    let _canvas = crate::interface::original_units();
    let ready = tiles.as_ref().is_some_and(|t| t.ready.is_some()) && pip.open() && pip.page == Page::Data;
    let (Some(mut st), Some(screen)) = (state, screen) else {
        for e in &roots {
            commands.entity(e).despawn();
        }
        *drawn = None;
        return;
    };
    let want = st.popup.clone().filter(|_| ready).map(|q| (q, map_window(&st, pip.tab())));
    if want == *drawn && (want.is_none() || !roots.is_empty()) {
        return;
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
    *drawn = want.clone();
    let Some((q, window)) = want else { return };
    st.popup_hold.rebuilt();
    let mut c = Ctx::new(&mut tools.kit, tools.data.as_deref(), &mut tools.images, tools.formats.as_deref(), &tools.glyphs);
    c.kit.screen = pip.frame != super::Frame::Device;
    info!("pipboy data: question box over the tiles ({} answers)", q.buttons.len());
    let root = Node { overflow: Overflow::clip(), ..at(0.0, 0.0, super::CANVAS.x, super::CANVAS.y) };
    commands
        .spawn((PopupOverTiles, root, UiTargetCamera(screen.camera), GlobalZIndex(crate::tiles::draw::PIPBOY_TILES_Z + 1), Pickable::IGNORE, Name::new("Question box over the page")))
        .with_children(|p| popup(p, &mut c, &q, window.center()));
}

/// The question box in the middle of the map's window, `mid` on the canvas (fast-travel.md §3):
/// black with a line all round, the text centred at the top, the answers centred one under the
/// other, the chosen one framed. It takes the pointer over the whole page, so nothing under it
/// is clicked.
fn popup(p: &mut ChildSpawnerCommands, c: &mut Ctx, q: &Popup, mid: Vec2) {
    let lines = c.lines(2, &q.text, Some(POPUP_W - 2.0 * POPUP_PAD));
    let pitch = c.line_height(2) + 4.0;
    let row = c.line_height(2) + POPUP_ROW_PAD;
    let text_h = c.line_height(2) + pitch * lines.len().saturating_sub(1) as f32;
    let h = POPUP_PAD + text_h + POPUP_PAD + row * q.buttons.len() as f32 + POPUP_PAD;
    let r = Rect::new(mid.x - POPUP_W / 2.0, mid.y - h / 2.0, mid.x + POPUP_W / 2.0, mid.y + h / 2.0);
    p.spawn((at(0.0, 0.0, super::CANVAS.x, super::CANVAS.y), Pickable::default()));
    c.boxed(p, r, amber(LINE), Some(Color::BLACK));
    for (i, l) in lines.iter().enumerate() {
        c.set(p, 2, l, mid.x, r.min.y + POPUP_PAD + pitch * i as f32, Align::Centre, c.bright());
    }
    let frame_w = q.buttons.iter().map(|b| c.width(2, b)).fold(0.0, f32::max) + POPUP_FRAME_PAD;
    let top = r.min.y + POPUP_PAD + text_h + POPUP_PAD;
    for (i, b) in q.buttons.iter().enumerate() {
        let y = top + row * i as f32;
        let frame = Rect::new(mid.x - frame_w / 2.0, y, mid.x + frame_w / 2.0, y + row - 1.0);
        if i == q.selected {
            c.boxed(p, frame, amber(LINE), Some(amber(FILL.0).with_alpha(FILL.1 / 255.0)));
        }
        // The text stands at the bottom of its row, as the message box's buttons do
        // (message-menu.md section 2: a row is the text and 20, the text at its bottom).
        c.text(p, 2, b, mid.x, y + POPUP_ROW_PAD, Align::Centre, c.bright());
        p.spawn((at(frame.min.x, frame.min.y, frame.width(), frame.height()), PopupHit(i), Pickable::default())).observe(on_popup_over).observe(on_popup_out).observe(on_popup_click);
    }
}

/// The heading: the title, the place and the date and time between their lines.
fn heading(p: &mut ChildSpawnerCommands, c: &mut Ctx, place: &str, when: &str) {
    let (x0, y0) = (MAIN.min.x, MAIN.min.y);
    let line = amber(LINE);
    let bright = c.bright();
    c.fade(p, x0, y0, FADE, false, line);
    c.line(p, x0, y0, LEAD_LINE, THICK, line);
    // The title, or in the screen menu nothing (the header stands there) and the line
    // runs on from the lead line (`frame::heading_title`).
    let after = super::frame::heading_title(p, c, Page::Data.title()) - x0;
    let from = if c.kit.screen { LEAD_LINE } else { LINE2.0.max(after) };
    c.line(p, x0 + from, y0, LINE2.0 + LINE2.1 - from, THICK, line);
    c.text(p, 2, place, x0 + PLACE_X, y0 + HEAD_TEXT_Y, Align::Right, bright);
    c.fade(p, x0 + SPLIT_X, y0, FADE, false, line);
    c.line(p, x0 + LINE3_X, y0, MAIN.width() - LINE3_X, THICK, line);
    c.text(p, 2, when, x0 + TIME_X, y0 + HEAD_TEXT_Y, Align::Right, bright);
    c.fade(p, x0 + MAIN.width() - THICK, y0, FADE, false, line);
}

fn button_height(c: &Ctx) -> f32 {
    c.line_height(2) + BUTTON_VERBUF
}

/// The Challenges button's key: R, with a controller Y (`map_menu.xml`'s, as
/// `crate::pad::menu_buttons` has it) [#1662].
fn challenges_hint() -> String {
    crate::pad::hint(&[KeyCode::KeyR], crate::pad::at_menu(fnv_game::game::menus::MAP))
}

/// The hints at the right as a column ([`BUTTONS`], `frame::Ctx::hint_column_rects`):
/// each row's word and key (canvas).
fn button_rows(c: &Ctx, list: &[Button]) -> Vec<Rect> {
    let hints: Vec<Hint> = list.iter().map(|b| b.hint.clone()).collect();
    c.hint_column_rects(&hints, MAIN.min.x + BUTTONS.x, MAIN.min.y + BUTTONS.y + super::frame::BUTTON_TEXT_Y)
}

/// Each hint's click area: its row widened to the file's box round the text, 20 across
/// and 15 up and down, the text `frame::BUTTON_TEXT_Y` under the box's top [documented:
/// `text_box.xml`, map_menu.xml's `_horbuf`, `_verbuf`]; the boxes of rows 46 apart meet.
fn button_boxes(c: &Ctx, list: &[Button]) -> Vec<Rect> {
    let h = button_height(c);
    button_rows(c, list)
        .into_iter()
        .map(|r| {
            let top = r.min.y - super::frame::BUTTON_TEXT_Y;
            Rect::new(r.min.x - BUTTON_HORBUF / 2.0, top, r.max.x + BUTTON_HORBUF / 2.0, top + h)
        })
        .collect()
}

/// The hints at the right, one under the other, each taking the pointer.
fn buttons(p: &mut ChildSpawnerCommands, c: &mut Ctx, list: &[Button]) {
    let hints: Vec<Hint> = list.iter().map(|b| b.hint.clone()).collect();
    c.hint_column(p, &hints, MAIN.min.x + BUTTONS.x, MAIN.min.y + BUTTONS.y + super::frame::BUTTON_TEXT_Y);
    for (b, r) in list.iter().zip(button_boxes(c, list)) {
        p.spawn((at(r.min.x, r.min.y, r.width(), r.height()), ButtonHit(b.kind), Pickable::default())).observe(on_button_click);
    }
}

/// A list's rows from `first` in a window `height` high at `origin` (canvas), the scroll
/// bar on the left or the right; rows of a tab's list take the pointer. Returns how many
/// rows fit.
#[allow(clippy::too_many_arguments)]
fn list(p: &mut ChildSpawnerCommands, c: &mut Ctx, origin: Vec2, height: f32, bar_left: bool, rows: &[Row], first: usize, boxed: Option<usize>, tab: Option<usize>) -> usize {
    let x0 = if bar_left { SCROLLBAR } else { 0.0 };
    let row_w = LIST_W - SCROLLBAR - ROW_SHORT;
    let mut fit = 0;
    p.spawn((Node { overflow: Overflow::clip(), ..at(origin.x, origin.y, LIST_W, height) }, Pickable::IGNORE)).with_children(|p| {
        let mut y = 0.0;
        for (i, r) in rows.iter().enumerate().skip(first) {
            let lines = c.lines(2, &r.text, Some(WRAP));
            let h = row_height(&lines);
            if y + h > height && fit > 0 {
                break;
            }
            row(p, c, x0, y, h, r, &lines, boxed == Some(i));
            if let Some(t) = tab {
                p.spawn((at(x0, y, row_w, h), RowHit(t, i), Pickable::default())).observe(on_row_over).observe(on_row_out).observe(on_row_click);
            }
            y += h;
            fit += 1;
        }
        if first > 0 || first + fit < rows.len() {
            let bar_x = if bar_left { 0.0 } else { LIST_W - SCROLLBAR };
            let from = first as f32 / rows.len() as f32;
            let part = fit as f32 / rows.len() as f32;
            scroll_bar(p, c, bar_x, height, from, part);
            if let Some(tab) = tab {
                let (top, h) = bar_marker(height, from, part);
                let track = Rect::new(bar_x, ARROW_H+4.0, bar_x+SCROLLBAR, height-ARROW_H-4.0);
                super::scrollbar::hits(p, Page::Data, tab, Rect::new(bar_x,0.0,bar_x+SCROLLBAR,height),track,
                    Rect::new(bar_x,top,bar_x+SCROLLBAR,(top+h).min(track.max.y)),origin);
            }
        }
    });
    fit
}

/// A row's height: its text's first line and the further ones.
fn row_height(lines: &[fnv_font::Line]) -> f32 {
    ROW_H + ROW_PITCH * lines.len().saturating_sub(1) as f32
}

/// One row `h` high at `x0`, `y` in its list's window: its square, its text broken into
/// `lines`, boxed when selected.
#[allow(clippy::too_many_arguments)]
fn row(p: &mut ChildSpawnerCommands, c: &mut Ctx, x0: f32, y: f32, h: f32, r: &Row, lines: &[fnv_font::Line], boxed: bool) {
    let bright = c.bright();
    let row_w = LIST_W - SCROLLBAR - ROW_SHORT;
    let colour = if r.done { bright.with_alpha(DONE_ALPHA) } else { bright };
    if boxed {
        c.boxed(p, Rect::new(x0 - BOX_LEFT, y, x0 + row_w, y + h - 1.0), amber(LINE), None);
    }
    let mark = match r.mark {
        Mark::None => None,
        Mark::Empty => Some("Interface\\Shared\\Marker\\square.dds"),
        Mark::Filled => Some("Interface\\Shared\\Marker\\square_filled.dds"),
    };
    if let Some(m) = mark {
        if c.image(p, m, at(x0 + MARK_AT.x, y + MARK_AT.y, MARK, MARK), colour).is_none() {
            p.spawn((at(x0 + MARK_AT.x, y + MARK_AT.y, MARK, MARK), BackgroundColor(colour)));
        }
    }
    for (k, l) in lines.iter().enumerate() {
        c.set(p, 2, l, x0 + ROW_TEXT.x, y + ROW_TEXT.y - THICK + ROW_PITCH * k as f32, Align::Left, colour);
    }
}

/// The quest's objectives at `origin`, scrolled `first` down as a note's text is (one
/// scroll for both, [`scroll_pane`]), the scroll bar on the right while they are longer
/// than the window; returns how far they can scroll.
fn objectives_pane(p: &mut ChildSpawnerCommands, c: &mut Ctx, origin: Vec2, rows: &[Row], first: f32) -> f32 {
    let laid: Vec<Vec<fnv_font::Line>> = rows.iter().map(|r| c.lines(2, &r.text, Some(WRAP))).collect();
    let total: f32 = laid.iter().map(Vec::as_slice).map(row_height).sum();
    p.spawn((Node { overflow: Overflow::clip(), ..at(origin.x, origin.y, LIST_W, OBJECTIVES_H) }, Pickable::IGNORE)).with_children(|p| {
        let mut y = -first;
        for (r, lines) in rows.iter().zip(&laid) {
            let h = row_height(lines);
            if y + h > 0.0 && y < OBJECTIVES_H {
                row(p, c, 0.0, y, h, r, lines, false);
            }
            y += h;
        }
        if total > OBJECTIVES_H {
            let (from, part) = (first / total, OBJECTIVES_H / total);
            scroll_bar(p, c, LIST_W - SCROLLBAR, OBJECTIVES_H, from, part);
            bar_hits(p, bar_spots(LIST_W - SCROLLBAR, OBJECTIVES_H, from, part, total / bar_run(OBJECTIVES_H)));
        }
    });
    objectives_max(total)
}

/// The run a scroll bar `height` high leaves its marker between the arrows.
fn bar_run(height: f32) -> f32 {
    height - 2.0 * ARROW_H - 8.0
}

/// A scroll bar's marker in a bar `height` high: its top and height for the part shown
/// (`part`) from `from` (shares of the whole).
pub(super) fn bar_marker(height: f32, from: f32, part: f32) -> (f32, f32) {
    let run = bar_run(height);
    (ARROW_H + 4.0 + run * from.clamp(0.0, 1.0), (run * part.clamp(0.0, 1.0)).max(8.0))
}
pub(super) fn bar_track(height: f32) -> (f32, f32) { (ARROW_H+4.0, height-ARROW_H-4.0) }

/// The places on a scroll bar at `x`, `height` high ([`scroll_bar`]) that take the
/// pointer: the arrows, the track above and below the marker, and the marker (as wide as
/// the bar), whose drag moves the pane `scale` units a unit of the bar.
fn bar_spots(x: f32, height: f32, from: f32, part: f32, scale: f32) -> Vec<(Rect, BarPart)> {
    let (top, h) = bar_marker(height, from, part);
    let foot = height - ARROW_H;
    let mut out = vec![(Rect::new(x, 0.0, x + SCROLLBAR, ARROW_H), BarPart::Up)];
    if top > ARROW_H {
        out.push((Rect::new(x, ARROW_H, x + SCROLLBAR, top), BarPart::PageUp));
    }
    out.push((Rect::new(x, top, x + SCROLLBAR, (top + h).min(foot)), BarPart::Marker(scale)));
    if foot > top + h {
        out.push((Rect::new(x, top + h, x + SCROLLBAR, foot), BarPart::PageDown));
    }
    out.push((Rect::new(x, foot, x + SCROLLBAR, height), BarPart::Down));
    out
}

/// Spawn a scroll bar's places that take the pointer (drawn after what they cover).
fn bar_hits(p: &mut ChildSpawnerCommands, spots: Vec<(Rect, BarPart)>) {
    for (r, b) in spots {
        let mut e = p.spawn((at(r.min.x, r.min.y, r.width(), r.height()), BarHit(b), Pickable::default()));
        if matches!(b, BarPart::Marker(_)) {
            e.observe(on_bar_press);
        } else {
            e.observe(on_bar_click);
        }
    }
}

/// How far objectives `total` high can scroll in their window.
fn objectives_max(total: f32) -> f32 {
    (total - OBJECTIVES_H).max(0.0)
}

/// A scroll bar: an arrow at each end and a line between them for the part shown
/// [hypothesis: the marker's look; the arrows are the file's, 32 × 16].
pub(super) fn scroll_bar(p: &mut ChildSpawnerCommands, c: &mut Ctx, x: f32, height: f32, from: f32, part: f32) {
    let line = amber(LINE);
    c.image(p, "Interface\\Shared\\scrollbar\\arrow_up.dds", at(x, 0.0, SCROLLBAR, ARROW_H), line);
    c.image(p, "Interface\\Shared\\scrollbar\\arrow_down.dds", at(x, height - ARROW_H, SCROLLBAR, ARROW_H), line);
    let (top, h) = bar_marker(height, from, part);
    c.line(p, x + (SCROLLBAR - THICK) / 2.0, top, THICK, h, line);
}

/// A note's text from `first` down, in a window at `origin`; returns how far it can
/// scroll.
fn note_text(p: &mut ChildSpawnerCommands, c: &mut Ctx, origin: Vec2, text: &str, first: f32) -> f32 {
    let bright = c.bright();
    let text = text.replace("\r\n", "\n");
    let pitch = c.line_height(1);
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let l = c.lines(1, para, Some(DATA_W));
        if l.is_empty() {
            lines.push(None);
        }
        lines.extend(l.into_iter().map(Some));
    }
    let total = pitch * lines.len() as f32;
    // The window reaches a little over the first line so its letters are not cut
    // [design].
    p.spawn((Node { overflow: Overflow::clip(), ..at(origin.x, origin.y - TEXT_PAD, DATA_W, TEXT_H + TEXT_PAD) }, Pickable::IGNORE)).with_children(|p| {
        for (i, l) in lines.iter().enumerate() {
            let y = pitch * i as f32 - first;
            if y + pitch < 0.0 || y > TEXT_H {
                continue;
            }
            if let Some(l) = l {
                c.set(p, 1, l, 0.0, y + TEXT_PAD, Align::Left, bright);
            }
        }
    });
    if total > TEXT_H {
        let line = amber(LINE);
        let x = origin.x + DATA_W;
        c.image(p, "Interface\\Shared\\scrollbar\\arrow_up.dds", at(x, origin.y, SCROLLBAR, ARROW_H), line);
        c.image(p, "Interface\\Shared\\scrollbar\\arrow_down.dds", at(x, origin.y + TEXT_H - ARROW_H, SCROLLBAR, ARROW_H), line);
        // The note's bar is its arrows only [documented]: each steps as a wheel notch.
        bar_hits(p, note_arrows(origin));
    }
    (total - TEXT_H).max(0.0)
}

/// A note's arrows, at the right of its text window at `origin` (canvas), where
/// [`note_text`] draws them.
fn note_arrows(origin: Vec2) -> Vec<(Rect, BarPart)> {
    let x = origin.x + DATA_W;
    vec![
        (Rect::new(x, origin.y, x + SCROLLBAR, origin.y + ARROW_H), BarPart::Up),
        (Rect::new(x, origin.y + TEXT_H - ARROW_H, x + SCROLLBAR, origin.y + TEXT_H), BarPart::Down),
    ]
}

/// An audio note's time remaining and meter, the box's top left at `o`.
fn audio_meter(p: &mut ChildSpawnerCommands, c: &mut Ctx, o: Vec2, left: u32, played: u16) {
    c.text(p, 1, &remaining(left), o.x + DATA_W, o.y, Align::Right, c.bright());
    let top = o.y + c.line_height(1);
    c.image(p, "Interface\\Shared\\Meter\\tickmark_horiz.dds", at(o.x, top, DATA_W, AUDIO_METER - AUDIO_METER_CROP), amber(LINE));
    let x = (DATA_W - AUDIO_MARKER) * played as f32 / 1000.0;
    c.line(p, o.x + x, top + AUDIO_METER / 4.0, AUDIO_MARKER, AUDIO_METER, c.bright());
}

/// The time remaining as the original writes it: minutes, seconds and tenths
/// [observed: "00:30:9 remaining" at the start of a 30.9 s note].
pub(crate) fn remaining(tenths: u32) -> String {
    format!("{:02}:{:02}:{} remaining", tenths / 600, tenths / 10 % 60, tenths % 10)
}

/// The radio's waveform panel; the trace's pieces are placed by [`wave_moves`].
fn wave(p: &mut ChildSpawnerCommands, c: &mut Ctx, on: bool) {
    let line = amber(LINE);
    let o = MAIN.min + WAVE.min;
    let (w, h) = (WAVE.width(), WAVE.height());
    c.image(p, "Interface\\Shared\\Meter\\tickmark_horiz.dds", at(o.x, o.y + h - TICKS + 2.0, w, TICKS), line);
    c.image(p, "Interface\\Shared\\Meter\\tickmark_vert.dds", at(o.x + w - TICKS + 2.0, o.y, TICKS, h), line);
    let g = WaveGeom::hand_built();
    if !on {
        c.line(p, g.left, g.mid, g.len, THICK, line);
        return;
    }
    trace(p, g, line);
}

/// Where the trace lies in the node its pieces stand in: the flat line's left end, its top and
/// its length, and the trace's greatest height either side of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WaveGeom {
    pub(crate) left: f32,
    pub(crate) mid: f32,
    pub(crate) len: f32,
    pub(crate) amplitude: f32,
}

impl WaveGeom {
    /// The trace in a panel `size` with its top left at `origin` (the node's units): the
    /// hand-built panel's proportions [documented: the flat line 2 high, 5 short of the width, 8
    /// over the middle; observed: the trace's height, [`WAVE_AMPLITUDE`]] scaled from its 350
    /// square [design]: at that size exactly the hand-built panel's numbers.
    pub(crate) fn of(origin: Vec2, size: Vec2) -> WaveGeom {
        let (sx, sy) = (size.x / WAVE.width(), size.y / WAVE.height());
        WaveGeom { left: origin.x, mid: origin.y + (size.y - THICK) / 2.0 - 8.0 * sy, len: size.x - 5.0 * sx, amplitude: WAVE_AMPLITUDE * sy }
    }

    /// The hand-built panel's trace, in the page body's canvas units.
    fn hand_built() -> WaveGeom {
        WaveGeom::of(MAIN.min + WAVE.min, WAVE.size())
    }
}

/// The trace's pieces in their parent node, along `g`, in `colour`; [`wave_moves`] moves them.
/// The flat line under it is the caller's (the hand-built panel draws none while it plays; the
/// file's panel has its own centre line, `crate::tiles::map_menu`).
pub(crate) fn trace(p: &mut ChildSpawnerCommands, g: WaveGeom, colour: Color) {
    let piece = g.len / WAVE_SEGMENTS as f32;
    for i in 0..WAVE_SEGMENTS {
        p.spawn((at(g.left + i as f32 * piece, g.mid, piece, THICK), BackgroundColor(colour), UiTransform::default(), WaveSegment { i, g }, Pickable::IGNORE));
    }
}

/// One straight piece of the waveform panel's trace, and the trace it is a piece of.
#[derive(Component)]
pub struct WaveSegment {
    i: usize,
    g: WaveGeom,
}

/// Marks the trace painted over the Radio tab drawn from its file.
#[derive(Component)]
pub struct TraceOverTiles;

/// The trace over the Radio tab drawn from `map_menu.xml` (`crate::tiles::map_menu::MapTiles::wave`,
/// published by the one decision): the hand-built trace inside `MM_WaveformRect`'s measured box,
/// cut to it, over the file's frame (its tick meters and centre line), the way the hotkey panel is
/// painted over ITEMS (`super::hotkey_panel::draw_over_tiles`) [design: the original's
/// `MM_Waveform` is an image its code animates, which animation is not known, so it stays hidden].
/// Nothing while the radio is off: the file's centre line is the flat line then. Spawned again only
/// when the box or the radio's state changes; [`wave_moves`] moves its pieces.
pub fn trace_over_tiles(mut commands: Commands, screen: Option<Res<PipBoyScreen>>, tiles: Option<Res<crate::tiles::map_menu::MapTiles>>, roots: Query<Entity, With<TraceOverTiles>>, mut drawn: Local<Option<crate::tiles::map_menu::WaveShown>>) {
    // Laid out in the Pip-Boy's screen texture, at its size whatever the UI scale (#1631).
    let _canvas = crate::interface::original_units();
    let want = tiles.as_ref().and_then(|t| t.wave).filter(|w| w.on);
    let screen = screen.filter(|_| want.is_some());
    if want == *drawn && (want.is_none() || !roots.is_empty()) {
        return;
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
    *drawn = None;
    let (Some(w), Some(screen)) = (want, screen) else { return };
    *drawn = Some(w);
    info!("pipboy data: waveform trace over the tiles at ({:.0}, {:.0}) {:.0} x {:.0}", w.rect.min.x, w.rect.min.y, w.rect.width(), w.rect.height());
    let root = Node { overflow: Overflow::clip(), ..at(0.0, 0.0, super::CANVAS.x, super::CANVAS.y) };
    let window = Node { overflow: Overflow::clip(), ..at(w.rect.min.x, w.rect.min.y, w.rect.width(), w.rect.height()) };
    commands
        .spawn((TraceOverTiles, root, UiTargetCamera(screen.camera), GlobalZIndex(crate::tiles::draw::PIPBOY_TILES_Z + 1), Pickable::IGNORE, Name::new("Waveform trace over the page")))
        .with_children(|p| {
            p.spawn((window, Pickable::IGNORE)).with_children(|p| trace(p, WaveGeom::of(Vec2::ZERO, w.rect.size()), amber(LINE)));
        });
}

/// The waveform panel's trace [observed: a capture of the original's Radio tab]: waves
/// standing nearly still across the panel under an envelope that runs to the left, its
/// height the sound's loudness as it came in (silence draws the flat line, a lull small
/// ripples).
#[derive(Resource, Default)]
pub struct WaveScope {
    /// Per point of the trace, oldest at the left: the envelope and the share of the
    /// second, faster wave.
    points: VecDeque<(f32, f32)>,
    /// Pieces run in, not yet whole.
    scroll: f32,
    /// Points made so far (picks the envelope's knots).
    made: u64,
    /// The waves' drift, cycles.
    phase: f32,
}

impl WaveScope {
    /// Run the trace on by `dt` seconds with the sound at `loudness`.
    fn step(&mut self, dt: f32, loudness: f32) {
        let level = (loudness.max(0.0) / WAVE_LOUD).sqrt().min(1.0);
        self.scroll += dt * WAVE_SCROLL * WAVE_SEGMENTS as f32;
        let fresh = if self.points.len() <= WAVE_SEGMENTS { WAVE_SEGMENTS + 1 } else { (self.scroll as usize).min(WAVE_SEGMENTS + 1) };
        self.scroll = self.scroll.fract();
        for _ in 0..fresh {
            self.made += 1;
            let swing = WAVE_SWING.0 + (WAVE_SWING.1 - WAVE_SWING.0) * knotted(self.made, 0);
            self.points.push_back((level * swing, knotted(self.made, 1)));
        }
        while self.points.len() > WAVE_SEGMENTS + 1 {
            self.points.pop_front();
        }
        self.phase = (self.phase + dt * WAVE_DRIFT).fract();
    }

    /// The trace's height at point `i` (0 at the left, [`WAVE_SEGMENTS`] at the right),
    /// -1 to 1, up positive.
    fn height(&self, i: usize) -> f32 {
        let Some(&(envelope, mix)) = self.points.get(i) else { return 0.0 };
        let t = (i as f32 / WAVE_SEGMENTS as f32 * WAVE_CYCLES + self.phase) * std::f32::consts::TAU;
        envelope * ((1.0 - 0.4 * mix) * t.sin() + 0.4 * mix * (2.7 * t + 1.3).sin())
    }
}

/// A smooth value from 0 to 1 at point `n`, eased between knots every [`WAVE_KNOT`]
/// points; `stream` picks one of several unrelated runs.
fn knotted(n: u64, stream: u64) -> f32 {
    let knot = |k: u64| {
        let mut x = k.wrapping_add(stream.wrapping_mul(0x9E37_79B9_7F4A_7C15)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        x ^= x >> 31;
        x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
        x ^= x >> 29;
        (x >> 40) as f32 / (1u64 << 24) as f32
    };
    let (k, f) = (n / WAVE_KNOT, (n % WAVE_KNOT) as f32 / WAVE_KNOT as f32);
    let f = f * f * (3.0 - 2.0 * f);
    knot(k) + (knot(k + 1) - knot(k)) * f
}

/// Move the waveform panel's trace while it is shown: the Pip-Boy radio's loudness
/// (`crate::radio`) or a holotape's steady one.
pub fn wave_moves(time: Res<Time<Real>>, radio: Option<Res<Radio>>, mut scope: ResMut<WaveScope>, mut pieces: Query<(&WaveSegment, &mut Node, &mut UiTransform)>) {
    // Laid out in the Pip-Boy's screen texture, at its size whatever the UI scale (#1631).
    let _canvas = crate::interface::original_units();
    if pieces.is_empty() {
        return;
    }
    scope.step(time.delta_secs(), radio.map_or(0.0, |r| r.pipboy_loudness()));
    for (s, mut node, mut turn) in &mut pieces {
        let g = s.g;
        let point = |i: usize| Vec2::new(i as f32 / WAVE_SEGMENTS as f32 * g.len, g.mid - g.amplitude * scope.height(i));
        let (a, b) = (point(s.i), point(s.i + 1));
        let d = b - a;
        let middle = (a + b) / 2.0;
        let length = d.length() + THICK / 2.0;
        node.left = u(g.left + middle.x - length / 2.0);
        node.top = u(middle.y - THICK / 2.0);
        node.width = u(length);
        *turn = UiTransform::from_rotation(Rot2::radians(d.y.atan2(d.x)));
    }
}

/// A map in its window: the picture, the markers, the player's arrow and the highlight
/// box; the window takes drags.
fn map(p: &mut ChildSpawnerCommands, c: &mut Ctx, m: &MapShown) {
    let o = MAIN.min + MAP.min;
    p.spawn((Node { overflow: Overflow::clip(), ..at(o.x, o.y, MAP.width(), MAP.height()) }, Pickable::IGNORE)).with_children(|p| {
        if let Some((path, r)) = &m.picture {
            c.image(p, path, at(r.min.x, r.min.y, r.width(), r.height()), c.bright().with_alpha(MAP_ALPHA / 255.0));
        }
        if let Some((image, r)) = &m.outline {
            p.spawn((at(r.min.x, r.min.y, r.width(), r.height()), ImageNode { color: c.bright(), image_mode: NodeImageMode::Stretch, ..ImageNode::new(image.clone()) }, Pickable::IGNORE));
        }
        // The player's Local Map Shows Reset Areas tweak: a faint red over each square
        // [design: docs/design/tweaks.md].
        for r in m.reset_cells.iter() {
            p.spawn((at(r.min.x, r.min.y, r.width(), r.height()), BackgroundColor(Color::srgba(1.0, 0.2, 0.15, RESET_CELL_ALPHA)), Pickable::IGNORE));
        }
        // In the menu file's depth order: markers 1, quest markers 4, the player's arrow
        // 7, the highlighted marker 11 [documented: map_menu.xml].
        let marker = |p: &mut ChildSpawnerCommands, c: &mut Ctx, mk: &MapMark| {
            let (size, brightness) = if mk.highlighted { (MARKER * MARKER_GROW, MARKER_BRIGHTNESS.1) } else { (MARKER, MARKER_BRIGHTNESS.0) };
            // The player's tweak draws a marker found in this run red [design: docs/design/tweaks.md].
            let colour = if mk.recent { Color::srgb(1.0, 0.25, 0.2).with_alpha(amber(brightness).alpha()) } else { amber(brightness) };
            let node = at(mk.at.x - size / 2.0, mk.at.y - size / 2.0, size, size);
            if c.image(p, &mk.picture(), node.clone(), colour).is_none() {
                if let Some((kind, found)) = mk.icon.filter(|_| mk.custom_icon.is_some()) {
                    c.image(p, &model::marker_icon(kind, found), node, colour);
                } else if mk.outside.is_some() {
                    // The tweak's own pictures missing: the plain door, an interior one dimmer.
                    let colour = if mk.outside == Some(false) { colour.with_alpha(colour.alpha() * INSIDE_DOOR_DIM) } else { colour };
                    c.image(p, DOOR_ICON, node, colour);
                }
            }
        };
        for mk in m.marks.iter().filter(|mk| !mk.highlighted) {
            marker(p, c, mk);
        }
        // The dotted lines under their nodes (depths -1 and 3 [documented]), the custom
        // marker over the quest markers (5 [observed: the original's MapMenu]).
        for piece in m.lines.iter().flat_map(|l| &l.1) {
            path_piece(p, c, piece);
        }
        for n in m.nodes.iter() {
            c.image(p, NODE_FILE, at(n.x - NODE / 2.0, n.y - NODE / 2.0, NODE, NODE), c.bright());
        }
        for q in &m.quests {
            c.image(p, QUEST_FILE, at(q.x, q.y, QUEST_MARKER, QUEST_MARKER * QUEST_ASPECT), c.bright());
        }
        if let Some(q) = m.custom {
            c.image(p, PC_MARKER_FILE, at(q.x, q.y, QUEST_MARKER, QUEST_MARKER * QUEST_ASPECT), c.bright());
        }
        // The companions' names where they stand, under the player's arrow [design: the
        // order]; a knocked-out one red (the player's tweak).
        for (at_, name, down) in &m.companions {
            let colour = if *down { Color::srgb(1.0, 0.25, 0.2).with_alpha(c.bright().alpha()) } else { c.bright() };
            c.text(p, m.companion_font, name, at_.x, at_.y - COMPANION_NAME_RISE, Align::Centre, colour);
        }
        if let Some((spot, heading)) = m.player {
            if let Some((image, rect)) = c.picture(CURSOR_FILE) {
                let (w, h) = (CURSOR, CURSOR * CURSOR_ASPECT);
                // The picture's top left part at the file's scale ([`arrow_texels`]).
                let from = rect.map_or(Vec2::ZERO, |r| r.min);
                let rect = Some(Rect::from_corners(from, from + arrow_texels(w, h)));
                p.spawn((
                    at(spot.x - w / 2.0, spot.y - h / 2.0, w, h),
                    ImageNode { rect, color: c.bright(), image_mode: NodeImageMode::Stretch, ..ImageNode::new(image) },
                    UiTransform::from_rotation(Rot2::radians(arrow_turn(heading))),
                ));
            }
        }
        for mk in m.marks.iter().filter(|mk| mk.highlighted) {
            marker(p, c, mk);
        }
        highlight(p, c, m.cursor, m.label.as_deref(), m.detail.as_deref());
        p.spawn((at(0.0, 0.0, MAP.width(), MAP.height()), MapHit, Pickable::default()));
    });
}

/// A piece of a dotted line ([`marker::dotted_line`]): the line's picture over its length,
/// the last piece showing only its share of the file, turned along the line about its
/// middle.
fn path_piece(p: &mut ChildSpawnerCommands, c: &mut Ctx, piece: &Piece) {
    let Some((image, rect)) = c.picture(PATH_FILE) else { return };
    let rect = match rect {
        Some(r) => Some(Rect::new(r.min.x, r.min.y, r.min.x + r.width() * piece.share, r.max.y)),
        None => c.picture_size(PATH_FILE).map(|s| Rect::new(0.0, 0.0, s.x * piece.share, s.y)),
    };
    p.spawn((
        at(piece.middle.x - piece.len / 2.0, piece.middle.y - PATH_H / 2.0, piece.len, PATH_H),
        ImageNode { rect, color: c.bright(), image_mode: NodeImageMode::Stretch, ..ImageNode::new(image) },
        UiTransform::from_rotation(Rot2::radians(piece.angle)),
        Pickable::IGNORE,
    ));
}

/// The highlight box round `centre`: four corners of two arms each with fading tips, the
/// dot at its middle and the name under it; `detail` (a marker's reputation) on the line
/// under the name [hypothesis: map_menu.xml has one text, `MM_HighlightLocationName`, its
/// string set in code; where the original puts the reputation is not documented].
fn highlight(p: &mut ChildSpawnerCommands, c: &mut Ctx, centre: Vec2, label: Option<&str>, detail: Option<&str>) {
    let line = amber(LINE);
    let o = centre - Vec2::splat(HBOX / 2.0);
    let (arm, tip, t) = (HBOX_ARM, HBOX_TIP, THICK);
    for (left, top) in [(true, true), (false, true), (true, false), (false, false)] {
        let x = if left { o.x } else { o.x + HBOX - t };
        let y = if top { o.y } else { o.y + HBOX - t };
        // Across from the corner, then a tip fading further along.
        c.line(p, if left { o.x } else { o.x + HBOX - arm }, y, arm, t, line);
        let (name, tip_x) = if left { ("fade_to_right.dds", o.x + arm) } else { ("fade_to_left.dds", o.x + HBOX - arm - tip) };
        c.image(p, name, at(tip_x, y, tip, t), line);
        // Down or up from the corner, with its tip.
        c.line(p, x, if top { o.y } else { o.y + HBOX - arm }, t, arm, line);
        let (name, tip_y) = if top { ("fade_to_bottom.dds", o.y + arm) } else { ("fade_to_top.dds", o.y + HBOX - arm - tip) };
        c.image(p, name, at(x, tip_y, t, tip), line);
    }
    c.image(p, "Interface\\Worldmap\\waypoint_node.dds", at(centre.x - DOT / 2.0, centre.y - DOT / 2.0, DOT, DOT), line);
    if let Some(name) = label {
        c.text(p, 1, name, centre.x, o.y + HBOX_NAME_Y, Align::Centre, c.bright());
        if let Some(d) = detail {
            let below = o.y + HBOX_NAME_Y + c.line_height(1);
            c.text(p, 1, d, centre.x, below, Align::Centre, c.bright());
        }
    }
}

/// [design, #2403] Synthetic logical/ineffective states for detector regressions.
#[cfg(feature = "report-test-support")]
#[doc(hidden)]
pub fn report_map_progress_fixture(step: usize) -> DataState {
    let mut s = DataState::default();
    if step >= 1 {
        s.centre[1] = Some([0.5, 0.5]);
    }
    if step >= 2 {
        s.magnify[1] = 2.0;
    }
    if step >= 3 {
        s.cursor[1] = Some(Vec2::ONE);
    }
    if step >= 4 {
        s.popup = Some(Popup::travel(42, "Synthetic question".into()));
    }
    if step >= 5 {
        s.popup.as_mut().unwrap().step(1);
    }
    if step >= 6 {
        s.active_quest = Some(43);
    }
    if step >= 7 {
        s.custom = Some(CustomMarker { space: 42, pos: [1.0, 2.0, 3.0] });
    }
    s.navigated = step as u32;
    s.poll = step as f32;
    s
}

/// [design, #2403] The same logical map transitions on the Local Map tab.
#[cfg(feature = "report-test-support")]
#[doc(hidden)]
pub fn report_local_map_progress_fixture(step: usize) -> DataState {
    let mut s = report_map_progress_fixture(step);
    s.centre.swap(0, 1);
    s.magnify.swap(0, 1);
    s.cursor.swap(0, 1);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exterior and Interior Door Icons: each kind its picture; off, the plain door.
    #[test]
    fn doors_take_their_kind_s_picture_under_the_tweak() {
        let door = |outside| MapMark { icon: None, custom_icon: None, highlighted: false, at: Vec2::ZERO, form: 0, uv: None, name: String::new(), detail: None, recent: false, outside };
        assert_eq!(door(None).picture(), DOOR_ICON);
        assert_eq!(door(Some(true)).picture(), DOOR_ICON_OUTSIDE);
        assert_eq!(door(Some(false)).picture(), DOOR_ICON_INSIDE);
    }

    /// #2571: a playing audio note's time and meter alone are what a held press does not
    /// redraw the page for; any other change is.
    #[test]
    fn only_the_audio_meter_moving_is_held_under_a_press() {
        let view = |right: Right, first: usize| View {
            tab: NOTES,
            place: "Goodsprings".into(),
            when: "12:05".into(),
            buttons: Vec::new(),
            body: Body::List { rows: Vec::new(), first, boxed: Some(0), right },
            popup: None,
        };
        let a = view(Right::Audio { left: 265, played: 120 }, 0);
        assert!(meter_only_moved(&a, &view(Right::Audio { left: 264, played: 123 }, 0)));
        assert!(!meter_only_moved(&a, &view(Right::Audio { left: 264, played: 123 }, 1)), "the list scrolled");
        assert!(!meter_only_moved(&a, &view(Right::Text { text: "x".into(), first: 0.0 }, 0)), "another note");
    }

    /// #2310: a row that comes under a resting pointer (the list redrawn as the D-pad or the
    /// keys step it) takes nothing; one the pointer moved onto takes the selection.
    #[test]
    fn only_a_pointer_that_moved_takes_a_row() {
        use bevy::camera::NormalizedRenderTarget;
        use bevy::picking::backend::HitData;
        use bevy::picking::events::Pointer;
        use bevy::picking::pointer::{Location, PointerId};
        let mut app = App::new();
        app.init_resource::<DataState>().init_resource::<super::super::PointerMoved>();
        let row = app.world_mut().spawn(RowHit(QUESTS, 2)).observe(on_row_over).id();
        let over = || PointerOver {
            entity: row,
            pointer: Pointer::new(PointerId::Mouse, Location { target: NormalizedRenderTarget::None { width: 1, height: 1 }, position: Vec2::ZERO }),
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        };
        app.world_mut().trigger(over());
        assert!(app.world().resource::<DataState>().acts.is_empty(), "a resting pointer");
        app.world_mut().insert_resource(super::super::PointerMoved(true));
        app.world_mut().trigger(over());
        assert_eq!(app.world().resource::<DataState>().acts, vec![Act::Hover(QUESTS, 2)], "the pointer moved onto it");
    }

    /// A world with what the page's systems read, the DATA page's state fresh, `MapMenu` on or off
    /// and the Pip-Boy as given; no hand-built body (the frame did not run).
    fn page_world(map: bool, pip: PipBoy) -> World {
        let mut w = World::new();
        w.insert_resource(crate::tiles::vanilla::TileMenus { map, ..Default::default() });
        w.insert_resource(pip);
        w.init_resource::<DataState>();
        w.init_resource::<ButtonInput<KeyCode>>();
        w.init_resource::<ButtonInput<MouseButton>>();
        w.init_resource::<Time<Real>>();
        w.init_resource::<PipBoyAid>();
        w.init_resource::<Assets<Image>>();
        w.init_resource::<Kit>();
        w.init_resource::<crate::pad::PadGlyphs>();
        w.init_resource::<Messages<MouseWheel>>();
        w.init_resource::<Messages<MenuInput>>();
        w.init_resource::<Messages<PipBoyInput>>();
        w.init_resource::<Messages<FastTravelAsk>>();
        w.init_resource::<Messages<SummonCompanions>>();
        w.init_resource::<Messages<FastTravelGo>>();
        w
    }

    /// The Pip-Boy up on DATA's tab `tab`.
    fn data_open(tab: usize) -> PipBoy {
        let mut p = PipBoy { phase: super::super::Phase::Up, page: Page::Data, ..default() };
        p.tabs[Page::Data.index()] = tab;
        p
    }

    /// With `MapMenu` on the page is prepared before the frame for the tab the Pip-Boy shows, with
    /// no hand-built body: the input taken, a new tab starting unboxed, a closing letting go and a
    /// reopening opening again, and what was gathered kept while the page is closed. The
    /// hand-built drawing then neither prepares nor paints without its body.
    #[test]
    fn with_mapmenu_on_the_page_is_prepared_without_its_body() {
        use bevy::ecs::system::RunSystemOnce;
        let mut w = page_world(true, data_open(QUESTS));
        w.run_system_once(prepare).unwrap();
        let st = w.resource::<DataState>();
        assert!(st.was_open, "opened");
        assert_eq!(st.last_tab, Some(QUESTS));
        assert_eq!(st.prepared.as_ref().map(|p| p.view.tab), Some(QUESTS), "what Quests shows, gathered");
        // Another tab: the input is taken there (Misc's filter, as its X hint's click), unboxed.
        {
            let mut st = w.resource_mut::<DataState>();
            st.boxed = true;
            st.acts.push(Act::Button(ButtonKind::NotesFilter));
        }
        w.resource_mut::<PipBoy>().tabs[Page::Data.index()] = NOTES;
        w.run_system_once(prepare).unwrap();
        let st = w.resource::<DataState>();
        assert!(st.notes_of_active, "the act was taken with no body");
        assert!(st.acts.is_empty());
        assert!(!st.boxed, "a new tab starts unboxed");
        assert_eq!((st.last_tab, st.prepared.as_ref().map(|p| p.view.tab)), (Some(NOTES), Some(NOTES)));
        assert_eq!(st.prepared.as_ref().map(|p| (p.filtered, p.challenges)), Some((true, false)), "the notes' filter, for the tile list's top");
        // The hand-built drawing paints only: no body, nothing prepared or drawn by it.
        w.resource_mut::<DataState>().was_open = false;
        w.run_system_once(draw).unwrap();
        assert!(!w.resource::<DataState>().was_open, "the drawing did not prepare");
        assert!(w.resource::<DataState>().shown.is_none(), "nor paint");
        w.resource_mut::<DataState>().was_open = true;
        // Closed: let go, what was gathered kept; open again: opened again.
        w.resource_mut::<DataState>().note_shown = Some(7);
        w.resource_mut::<PipBoy>().phase = super::super::Phase::Down;
        w.run_system_once(prepare).unwrap();
        let st = w.resource::<DataState>();
        assert!(!st.was_open && st.note_shown.is_none());
        assert_eq!(st.prepared.as_ref().map(|p| p.view.tab), Some(NOTES), "kept while the page goes down");
        w.resource_mut::<DataState>().boxed = true;
        w.resource_mut::<PipBoy>().phase = super::super::Phase::Up;
        w.run_system_once(prepare).unwrap();
        let st = w.resource::<DataState>();
        assert!(st.was_open && !st.boxed, "a reopening opens the page again");
        // Another page: closed for DATA.
        w.resource_mut::<PipBoy>().page = Page::Items;
        w.run_system_once(prepare).unwrap();
        assert!(!w.resource::<DataState>().was_open);
    }

    /// With `MapMenu` on, a menu over the page (a message box, the tutorial menu, through the frame it
    /// closes in) holds the page's input: its keys, the wheel and the pointer's acts are that menu's,
    /// so Enter answering it does not also use the selected row. The page is still prepared.
    #[test]
    fn with_mapmenu_on_a_menu_over_the_page_holds_its_input() {
        use bevy::ecs::system::RunSystemOnce;
        let mut w = page_world(true, data_open(NOTES));
        w.init_resource::<crate::tiles::map_menu::MapTiles>();
        w.resource_mut::<crate::tiles::map_menu::MapTiles>().over_data = true;
        {
            let mut st = w.resource_mut::<DataState>();
            st.acts.push(Act::Button(ButtonKind::NotesFilter));
            st.acts.push(Act::UseRecord(NOTES, 5));
        }
        w.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyR);
        w.run_system_once(prepare).unwrap();
        let st = w.resource::<DataState>();
        assert!(!st.notes_of_active && !st.challenges, "neither the act nor R acted");
        assert!(st.acts.is_empty(), "the menu's acts are dropped, not kept for later");
        assert_eq!(st.prepared.as_ref().map(|p| p.view.tab), Some(NOTES), "still prepared");
        // The menu gone: the page takes its input again.
        w.resource_mut::<crate::tiles::map_menu::MapTiles>().over_data = false;
        w.resource_mut::<DataState>().acts.push(Act::Button(ButtonKind::NotesFilter));
        w.run_system_once(prepare).unwrap();
        assert!(w.resource::<DataState>().notes_of_active);
        // Off, the early preparation does nothing, held or not.
        let mut w = page_world(false, data_open(NOTES));
        w.init_resource::<crate::tiles::map_menu::MapTiles>();
        w.resource_mut::<crate::tiles::map_menu::MapTiles>().over_data = true;
        w.resource_mut::<DataState>().acts.push(Act::Button(ButtonKind::NotesFilter));
        w.run_system_once(prepare).unwrap();
        assert_eq!(w.resource::<DataState>().acts.len(), 1);
    }

    /// An act of the page drawn from the file names its record: taken, it is the act on that
    /// record's row in the list as it is then, though a script changed the list after the click
    /// was queued; a record gone, or another tab's, acts on nothing.
    #[test]
    fn an_act_by_record_finds_its_row_in_the_list_as_it_is_when_taken() {
        let queued = Act::UseRecord(QUESTS, 20);
        assert_eq!(by_record(queued, QUESTS, &[10, 20, 30]), Some(Act::Use(QUESTS, 1)));
        assert_eq!(by_record(queued, QUESTS, &[20, 30]), Some(Act::Use(QUESTS, 0)), "a quest above it gone");
        assert_eq!(by_record(queued, QUESTS, &[30, 40, 20]), Some(Act::Use(QUESTS, 2)), "the list resorted");
        assert_eq!(by_record(queued, QUESTS, &[10, 30]), None, "the quest gone");
        assert_eq!(by_record(queued, NOTES, &[20]), None, "another tab");
        assert_eq!(by_record(Act::HoverRecord(QUESTS, 30), QUESTS, &[30]), Some(Act::Hover(QUESTS, 0)));
        assert_eq!(by_record(Act::LeaveRecord(QUESTS, 30), QUESTS, &[10, 30]), Some(Act::Leave(QUESTS, 1)));
        assert_eq!(by_record(Act::Use(QUESTS, 4), QUESTS, &[]), Some(Act::Use(QUESTS, 4)), "the hand-built page's acts as they are");
    }

    /// A page of the right pane is the window the page drawn from the file measured while it
    /// published one, else the file's numbers; the file's bar's marker drags it to a place.
    #[test]
    fn a_page_is_the_measured_window_and_the_files_marker_sets_the_place() {
        assert_eq!(pane_page(QUESTS, None), OBJECTIVES_H);
        assert_eq!(pane_page(NOTES, None), TEXT_H);
        assert_eq!(pane_page(QUESTS, Some(150.0)), 150.0);
        let key = Some((QUESTS, false, 7));
        let mut st = DataState { pane_of: key, text_max: 1000.0, ..DataState::default() };
        st.publish_drawn(QUESTS, 3, 1000.0, Some(Rect::new(10.0, 20.0, 410.0, 170.0)));
        scroll_pane(&mut st, QUESTS, key, PaneMoves { pages: 1.0, ..PaneMoves::default() });
        assert_eq!(st.text_first, 150.0, "a page of the measured 150");
        let drag = 640.0 - st.text_first;
        scroll_pane(&mut st, QUESTS, key, PaneMoves { drag, ..PaneMoves::default() });
        assert_eq!(st.text_first, 640.0);
    }

    /// With `MapMenu` off (the default) the early preparation does nothing, and the hand-built
    /// drawing prepares only while its body is there, as before: with none, the page is closed.
    #[test]
    fn with_mapmenu_off_nothing_is_prepared_early() {
        use bevy::ecs::system::RunSystemOnce;
        let mut w = page_world(false, data_open(QUESTS));
        w.resource_mut::<DataState>().acts.push(Act::Button(ButtonKind::NotesFilter));
        w.run_system_once(prepare).unwrap();
        let st = w.resource::<DataState>();
        assert!(!st.was_open && st.prepared.is_none() && st.last_tab.is_none());
        assert_eq!(st.acts.len(), 1, "nothing taken");
        w.resource_mut::<DataState>().was_open = true;
        w.run_system_once(draw).unwrap();
        let st = w.resource::<DataState>();
        assert!(!st.was_open, "no body: the drawing closed the page, as before");
        assert!(st.prepared.is_none() && st.last_tab.is_none());
        assert!(!prepares_early(None) && !prepares_early(Some(&crate::tiles::vanilla::TileMenus::default())));
    }

    #[test]
    fn unmapped_places_are_the_unfound_ones_without_the_visible_flag() {
        let marker = |form, world, flags| read::Marker { form, world, pos: [0.0; 3], name: format!("Place {form}"), kind: 1, flags, radius: 10.0, reputation: None };
        let mut st = DataState { markers: Some(vec![marker(10, 1, 0), marker(20, 1, 1), marker(30, 1, 0), marker(40, 2, 0)]), ..DataState::default() };
        st.discovered.insert(30);
        assert_eq!(st.unmapped_of(1, None), [10].into_iter().collect());
        assert_eq!(st.unmapped_of(2, None), [40].into_iter().collect());
    }

    #[test]
    fn family_markers_share_the_map_but_discovery_and_the_local_map_stay_in_their_worldspace() {
        use bevy::ecs::system::SystemState;
        use model::MapTransform;

        let root = WorldMap { root: 1, transform: MapTransform::default(), texture: "root.dds".into(), usable: [2048.0; 2], nw: [-2, 3], se: [1, 2] };
        let child = WorldMap { transform: MapTransform { offset: [0.0, 3.0 * model::CELL], ..MapTransform::default() }, ..root.clone() };
        let other = WorldMap { root: 3, ..root.clone() };
        // Identical raw coordinates in different worldspaces must not discover each
        // other's markers. Only the child projects onto the player's map centre.
        let marker = |form, world| read::Marker { form, world, pos: [0.0; 3], name: format!("Place {form}"), kind: 1, flags: 1, radius: 10.0, reputation: None };
        let mut st = DataState {
            maps: [(1, Some(root)), (2, Some(child)), (3, Some(other))].into_iter().collect(),
            markers: Some(vec![marker(10, 1), marker(20, 2), marker(30, 3)]),
            ..DataState::default()
        };
        let mut world = World::new();
        world.insert_resource(Location { world: 2, interior: None });
        world.insert_resource(Player { heading: 0.7, pitch: 0.0, eye_height: 0.0, feet: [0.0; 3], body_eye: None, body_view: None, placed: None });
        let mut state = SystemState::<Game>::new(&mut world);
        {
            let game = state.get_mut(&mut world).unwrap();
            track(&mut st, &game);
            assert_eq!(st.discovered, BTreeSet::from([20]));
            let shown = world_map(&mut st, &game, None, false);
            assert_eq!(shown.marks.len(), 2);
            assert_eq!(shown.picture.as_ref().unwrap().0, "root.dds");
            assert_eq!(shown.player, Some((MAP_MID, 0.7)));
            assert_eq!(shown.marks[1].at, MAP_MID);
            assert_eq!(shown.marks[0].icon, Some((1, false)));
            assert_eq!(shown.marks[1].icon, Some((1, true)));
            assert_eq!(st.cursor_marker, Some(20));
            assert_eq!(shown.label.as_deref(), Some("Place 20"));
            assert_eq!(st.world_logged, Some((2, 2, 2, 1, 1)));
            // This is also the reference used by the map click to ask for travel.
            assert_eq!(st.markers_of(2), vec![(20, [0.0; 3], true)]);
            let local = local_map(&mut st, &game, None);
            assert_eq!(local.marks.len(), 1);
            assert_eq!(local.label.as_deref(), Some("Place 20"));
        }
        // The root view includes the child's marker too.
        world.resource_mut::<Location>().world = 1;
        let game = state.get_mut(&mut world).unwrap();
        st.centre[WORLD] = None;
        let shown = world_map(&mut st, &game, None, false);
        assert_eq!(shown.marks.len(), 2);
        assert_eq!(st.world_logged, Some((1, 2, 2, 1, 1)));
    }

    /// #2338: a marker without `XRDS` (read as radius 0) is found within the default
    /// distance (`iMapMarkerRevealDistance`, 1000 when no plugin sets it); a marker with its
    /// own radius keeps it, smaller or larger than the default.
    #[test]
    fn a_marker_without_a_radius_is_found_within_the_default_distance() {
        use bevy::ecs::system::SystemState;

        assert_eq!(discovery_radius(0.0, 1000.0), 1000.0);
        assert_eq!(discovery_radius(-5.0, 1000.0), 1000.0);
        assert_eq!(discovery_radius(250.0, 1000.0), 250.0);
        assert_eq!(discovery_radius(2500.0, 1000.0), 2500.0);

        // Along x from the player at 0: 10 none (999 away, so found), 20 none (1001 away),
        // 30 with its own 250 (600 away), 40 with its own 2500 (2000 away).
        let marker = |form, x: f32, radius| read::Marker { form, world: 1, pos: [x, 0.0, 0.0], name: format!("Place {form}"), kind: 5, flags: 0, radius, reputation: None };
        let mut st = DataState { markers: Some(vec![marker(10, 999.0, 0.0), marker(20, -1001.0, 0.0), marker(30, 600.0, 250.0), marker(40, -2000.0, 2500.0)]), ..DataState::default() };
        let mut world = World::new();
        world.insert_resource(Location { world: 1, interior: None });
        world.insert_resource(Player { heading: 0.0, pitch: 0.0, eye_height: 0.0, feet: [0.0; 3], body_eye: None, body_view: None, placed: None });
        let mut state = SystemState::<Game>::new(&mut world);
        let game = state.get_mut(&mut world).unwrap();
        track(&mut st, &game);
        assert_eq!(st.discovered, BTreeSet::from([10, 40]));
        assert_eq!(st.newly_found, vec![10, 40]);
    }

    /// #3123: a marker that is Initially Disabled (Fallout3.esm's `DC23b`) is not found in
    /// range; once a script enables it (a quest's marker) it is found the next time the
    /// player is in range; a found marker disabled later stays found. Enabled markers as
    /// before.
    #[test]
    fn a_disabled_marker_is_not_found_until_it_is_enabled() {
        use bevy::ecs::system::SystemState;
        use fnv_sim::refs::PlacedRef;

        const OFF: u32 = 0x0009_5D47;
        const ON: u32 = 0x0004_BA53;
        let off = PlacedRef { sig: *b"REFR", flags: fnv_sim::refs::FLAG_INITIALLY_DISABLED, ..crate::sim::placed_ref(OFF) };
        let on = crate::sim::placed_ref(ON);
        let marker = |form, x: f32| read::Marker { form, world: 1, pos: [x, 0.0, 0.0], name: format!("Place {form}"), kind: 5, flags: 0, radius: 0.0, reputation: None };
        let mut st = DataState { markers: Some(vec![marker(OFF, 100.0), marker(ON, -100.0)]), ..DataState::default() };
        let mut world = World::new();
        world.insert_resource(SimState::over_placed([off, on]));
        world.insert_resource(Location { world: 1, interior: None });
        world.insert_resource(Player { heading: 0.0, pitch: 0.0, eye_height: 0.0, feet: [0.0; 3], body_eye: None, body_view: None, placed: None });
        let mut state = SystemState::<Game>::new(&mut world);
        {
            let game = state.get_mut(&mut world).unwrap();
            track(&mut st, &game);
        }
        assert_eq!(st.discovered, BTreeSet::from([ON]), "the disabled marker is in range but not found");
        assert_eq!(st.newly_found, vec![ON], "no banner or XP for the disabled one");

        // A script enables it: found on the next tracking in range.
        world.resource_mut::<SimState>().world.set_enabled(FormId(OFF), true).unwrap();
        {
            let game = state.get_mut(&mut world).unwrap();
            track(&mut st, &game);
        }
        assert_eq!(st.discovered, BTreeSet::from([OFF, ON]));
        assert_eq!(st.newly_found, vec![ON, OFF]);

        // Disabled again: it keeps its found state.
        world.resource_mut::<SimState>().world.set_enabled(FormId(OFF), false).unwrap();
        {
            let game = state.get_mut(&mut world).unwrap();
            track(&mut st, &game);
        }
        assert_eq!(st.discovered, BTreeSet::from([OFF, ON]));
    }

    /// #2338: a plugin's `iMapMarkerRevealDistance` is used as it is: 0 finds no marker
    /// without `XRDS` by walking (not even one under the player's feet), while markers with
    /// their own radius are still found; a positive value replaces the default 1000.
    #[test]
    fn a_plugins_reveal_distance_is_used_as_it_is_zero_included() {
        use bevy::ecs::system::SystemState;
        use fnv_sim::settings::Setting;

        let found_with = |setting: Setting| {
            let mut forms = fnv_sim::FormStore::default();
            forms.settings.insert("imapmarkerrevealdistance".into(), setting);
            let forms: &'static fnv_sim::FormStore = Box::leak(Box::new(forms));
            let refs: &'static fnv_sim::RefTable = Box::leak(Box::new(fnv_sim::RefTable::default()));
            // Along x from the player at 0: 10 none (on the spot), 20 none (1200 away),
            // 30 with its own 250 (200 away), 40 none (1600 away).
            let marker = |form, x: f32, radius| read::Marker { form, world: 1, pos: [x, 0.0, 0.0], name: format!("Place {form}"), kind: 5, flags: 0, radius, reputation: None };
            let mut st = DataState { markers: Some(vec![marker(10, 0.0, 0.0), marker(20, 1200.0, 0.0), marker(30, -200.0, 250.0), marker(40, -1600.0, 0.0)]), ..DataState::default() };
            let mut world = World::new();
            world.insert_resource(SimState { forms, refs, world: fnv_sim::World::new(forms, refs, fnv_sim::SimConfig::default()) });
            world.insert_resource(Location { world: 1, interior: None });
            world.insert_resource(Player { heading: 0.0, pitch: 0.0, eye_height: 0.0, feet: [0.0; 3], body_eye: None, body_view: None, placed: None });
            let mut state = SystemState::<Game>::new(&mut world);
            let game = state.get_mut(&mut world).unwrap();
            track(&mut st, &game);
            st.discovered
        };
        assert_eq!(found_with(Setting::Int(0)), BTreeSet::from([30]), "an authored 0: only the marker with its own radius");
        assert_eq!(found_with(Setting::Int(1500)), BTreeSet::from([10, 20, 30]), "an authored 1500 reaches 1200, not 1600");
    }

    /// [design] The World Map's parts as the page drawn from the file writes them: each marker by
    /// its reference with its texture coordinates, the arrow's, from one viewport; in a child
    /// worldspace each marker projects by its own worldspace's numbers onto the root's picture,
    /// and the inverse gives the place back. A window the tile page published is the one the
    /// model reads (the highlight box starts at its middle), for its tab alone; the hand-built
    /// drawing's body puts the file's numbers back.
    #[test]
    fn the_world_maps_parts_come_from_one_viewport_in_the_window_published() {
        use bevy::ecs::system::SystemState;
        use model::MapTransform;

        let root = WorldMap { root: 1, transform: MapTransform::default(), texture: "root.dds".into(), usable: [2048.0; 2], nw: [-2, 3], se: [1, 2] };
        let child = WorldMap { transform: MapTransform { offset: [0.0, 3.0 * model::CELL], ..MapTransform::default() }, ..root.clone() };
        let marker = |form, world, x: f32| read::Marker { form, world, pos: [x, 0.0, 0.0], name: format!("Place {form}"), kind: 1, flags: 1, radius: 10.0, reputation: None };
        let mut st = DataState {
            maps: [(1, Some(root.clone())), (2, Some(child.clone()))].into_iter().collect(),
            markers: Some(vec![marker(10, 1, 0.0), marker(20, 2, 0.0), marker(30, 2, 4096.0)]),
            ..DataState::default()
        };
        // The tile page's window, 100 right and 20 down of the hand-built one, 600 × 400.
        let measured = Rect::new(150.0, 120.0, 750.0, 520.0);
        st.publish_map(WORLD, measured, vec![Rect::new(800.0, 100.0, 900.0, 140.0)]);
        assert_eq!(map_window(&st, WORLD), measured);
        assert_eq!(map_window(&st, LOCAL), hand_built_window(), "the Local Map keeps the file's numbers");
        let mut world = World::new();
        world.insert_resource(Location { world: 2, interior: None });
        world.insert_resource(Player { heading: 0.7, pitch: 0.0, eye_height: 0.0, feet: [0.0; 3], body_eye: None, body_view: None, placed: None });
        let mut state = SystemState::<Game>::new(&mut world);
        let game = state.get_mut(&mut world).unwrap();
        let shown = world_map(&mut st, &game, None, false);
        let parts = shown.world.clone().expect("the World Map's parts");
        assert_eq!(parts.view.window, measured);
        assert_eq!((parts.texture.as_str(), parts.mag), ("root.dds", 1.0));
        // The player at the window's middle (the map centred on them), heading kept.
        assert_eq!(shown.player.map(|p| p.0), Some(Vec2::new(300.0, 200.0)));
        let (arrow, heading) = parts.arrow.unwrap();
        assert_eq!((child.uv(0.0, 0.0), heading), (arrow, 0.7));
        // Each marker by its reference, at its own worldspace's projection, drawn where the view puts it.
        assert_eq!(shown.marks.iter().map(|m| m.form).collect::<Vec<_>>(), vec![10, 20, 30]);
        for m in &shown.marks {
            let uv = m.uv.unwrap();
            assert_eq!(m.at, parts.view.to_window(uv));
            let back = parts.view.to_uv(m.at);
            assert!((back[0] - uv[0]).abs() < 1e-5 && (back[1] - uv[1]).abs() < 1e-5);
        }
        assert_eq!(shown.marks[0].uv, Some(root.uv(0.0, 0.0)));
        assert_eq!(shown.marks[2].uv, Some(child.uv(4096.0, 0.0)));
        let pos = child.position(shown.marks[2].uv.unwrap());
        assert!((pos[0] - 4096.0).abs() < 0.05 && pos[1].abs() < 0.05, "the inverse in the child: {pos:?}");
        // The highlight box started at the measured window's middle, on the marker there.
        assert_eq!(st.cursor[WORLD], Some(Vec2::new(300.0, 200.0)));
        assert_eq!(shown.highlight().1, Some("Place 20"));
        assert!(shown.marks[1].highlighted);
        // The pointer is read in the measured window, the hints' boxes are the hints'.
        assert_eq!(in_window(map_window(&st, WORLD), Some(Vec2::new(160.0, 130.0))), Some(Vec2::new(10.0, 10.0)));
        assert!(on_button(&st, WORLD, Some(Vec2::new(850.0, 120.0))));
        assert!(!on_button(&st, LOCAL, Some(Vec2::new(850.0, 120.0))), "the hints published were the World Map's");
        // No dotted line without a marker or a quest target.
        assert!(parts.ends.is_empty() && parts.quests.is_empty() && parts.custom.is_none());
        // A custom marker placed: its line's end, from the arrow.
        st.custom = Some(CustomMarker { space: 1, pos: [2048.0, 0.0, 0.0] });
        let shown = world_map(&mut st, &game, None, false);
        let parts = shown.world.unwrap();
        assert_eq!(parts.custom, Some(root.uv(2048.0, 0.0)));
        assert_eq!(parts.ends, vec![(LineTo::Marker, 0, root.uv(2048.0, 0.0))]);
    }

    /// [design] A World Map prepared in a window the tile page published, then drawn by hand (the
    /// tile page fell back): gathered again in the hand-built window, the window let go, the
    /// player at that window's middle; nothing to do once it is the hand-built window's.
    #[test]
    fn a_map_prepared_in_the_tile_window_is_gathered_again_when_drawn_by_hand() {
        use bevy::ecs::system::SystemState;
        use model::MapTransform;

        let root = WorldMap { root: 1, transform: MapTransform::default(), texture: "root.dds".into(), usable: [2048.0; 2], nw: [-2, 3], se: [1, 2] };
        let mut st = DataState { maps: [(1, Some(root))].into_iter().collect(), markers: Some(Vec::new()), ..DataState::default() };
        let measured = Rect::new(150.0, 120.0, 750.0, 520.0);
        st.publish_map(WORLD, measured, Vec::new());
        let mut world = World::new();
        world.insert_resource(Location { world: 1, interior: None });
        // The player at the picture's middle ([0.5, 0.5]): 3 cells north on this root.
        world.insert_resource(Player { heading: 0.0, pitch: 0.0, eye_height: 0.0, feet: [0.0, 3.0 * model::CELL, 0.0], body_eye: None, body_view: None, placed: None });
        let mut state = SystemState::<Game>::new(&mut world);
        let game = state.get_mut(&mut world).unwrap();
        let lists = Lists::default();
        let prepared = build(&mut st, WORLD, &lists, &game, None, false);
        let Body::Map(m) = &prepared.body else { panic!("a map") };
        assert_eq!(m.player.map(|p| p.0), Some(Vec2::new(300.0, 200.0)), "prepared in the measured window");
        assert_eq!(refit_hand_built(&mut st, QUESTS, &game, None), None, "not a map");
        // A Place Marker question asked this frame of the place under the box in the tile window.
        let asked = st.spot[WORLD].expect("the place in the tile window");
        st.popup = Some(Popup::marker(marker::MarkerAsk::Set(asked), "Here?".into(), vec!["Yes".into(), "No".into()]));
        let again = refit_hand_built(&mut st, WORLD, &game, None).expect("gathered again");
        let now = st.spot[WORLD].expect("the place in the hand-built window");
        assert!(matches!(st.popup.as_ref().map(|p| &p.ask), Some(Ask::Marker(marker::MarkerAsk::Set(at))) if *at == now), "the question asks of the place under the box as drawn by hand");
        let Body::Map(m) = &again.body else { panic!("a map") };
        assert_eq!(m.player.map(|p| p.0), Some(MAP_MID), "at the hand-built window's middle");
        assert_eq!(m.world.as_ref().map(|w| w.view.window), Some(hand_built_window()));
        assert_eq!(st.map_window, None, "the measured window let go");
        // The highlight box kept its place on the canvas: (150, 120) + (300, 200) less (50, 100).
        assert_eq!(st.cursor[WORLD], Some(Vec2::new(400.0, 220.0)));
        assert_eq!(refit_hand_built(&mut st, WORLD, &game, None), None, "already the hand-built window's");
    }

    /// [design] The Local Map's parts as the page drawn from the file writes them, from the one
    /// viewport the hand-built drawing paints from: indoors at 8 units a unit and outdoors at 28,
    /// the frame its outline's grid; each door and marker by its reference at its frame's texture
    /// coordinates, drawn where the projection puts it; the arrow's place and heading; the
    /// outline's picture and its texels once made; a new outline's frame replacing the old.
    #[test]
    fn the_local_maps_parts_come_from_one_viewport_indoors_and_out() {
        use bevy::ecs::system::SystemState;

        let door = |form, x: f32, y: f32| read::Door { form, pos: [x, y, 0.0], name: format!("Door {form}"), outside: true };
        for interior in [Some(0x0001_2345_u32), None] {
            // The early preparation's (`MapMenu` on): the tile page's rows gathered too.
            let mut st = DataState { tile_rows: true, ..DataState::default() };
            // The doors' cells: the interior, or the grid square at the map's middle (10, 20).
            let key = (interior, (0, 0));
            st.doors.insert(key, vec![door(7, 100.0, 50.0), door(8, -300.0, -200.0), door(9, 100_000.0, 20.0)]);
            st.markers = Some(Vec::new());
            let mut world = World::new();
            world.insert_resource(Location { world: 1, interior });
            world.insert_resource(Player { heading: 1.2, pitch: 0.0, eye_height: 0.0, feet: [10.0, 20.0, 0.0], body_eye: None, body_view: None, placed: None });
            let mut state = SystemState::<Game>::new(&mut world);
            let game = state.get_mut(&mut world).unwrap();
            let shown = local_map(&mut st, &game, None);
            let parts = shown.world.clone().expect("the Local Map's parts");
            let p = parts.view.local.expect("the Local Map's projection");
            assert_eq!(p.scale, if interior.is_some() { outline::INTERIOR_SCALE } else { LOCAL_SCALE });
            assert_eq!(p.centre, [10.0, 20.0]);
            let (origin, extent) = outline::Outline::frame_for(interior, 1, [10.0, 20.0], 0.0, MAP.size(), p.scale);
            assert_eq!((p.origin, p.extent), (origin, extent), "the frame the outline's grid would cover");
            assert_eq!(parts.view.window, hand_built_window());
            // The player at the window's middle, as the hand-built drawing has it.
            assert_eq!(shown.player, Some((MAP_MID, 1.2)));
            let (arrow, heading) = parts.arrow.unwrap();
            assert_eq!(heading, 1.2);
            assert!((parts.view.to_window(arrow) - MAP_MID).length() < 1e-2);
            for m in &shown.marks {
                let uv = m.uv.expect("the door's place on the frame");
                assert!((parts.view.to_window(uv) - m.at).length() < 1e-2, "{m:?}");
            }
            assert_eq!(shown.marks.iter().map(|m| m.form).collect::<Vec<_>>(), vec![7, 8], "the hand-built page leaves the door far outside the window out");
            let rows = parts.rows.clone().expect("the tile page's rows");
            assert_eq!(rows.iter().map(|m| m.form).collect::<Vec<_>>(), vec![7, 8, 9], "the tile page keeps it: its window cuts it, a pan keeps its row");
            let picked: Vec<u32> = shown.marks.iter().filter(|m| m.highlighted).map(|m| m.form).collect();
            assert_eq!(rows.iter().filter(|m| m.highlighted).map(|m| m.form).collect::<Vec<_>>(), picked, "the same one under the highlight box");
            // With `MapMenu` off the hand-built page alone: no rows gathered for a tile page.
            st.tile_rows = false;
            let off = local_map(&mut st, &game, None);
            assert_eq!((off.world.as_ref().and_then(|w| w.rows.clone()), off.marks.iter().map(|m| m.form).collect::<Vec<_>>()), (None, vec![7, 8]));
            assert_eq!((parts.outline.clone(), parts.texture.as_str(), parts.ends.len()), (None, "", 0), "no outline yet, no line");
            // The outline made: its picture and texels; a new outline (another place) another frame.
            let mut o = outline::Outline::new(interior, 1, [10.0, 20.0], 0.0, MAP.size(), p.scale);
            let handle = Handle::<Image>::default();
            o.image = Some(handle.clone());
            let texels = Vec2::new(o.size[0] as f32, o.size[1] as f32);
            st.outline = Some(o);
            let parts = local_map(&mut st, &game, None).world.unwrap();
            assert_eq!(parts.outline, Some((handle.clone(), texels)));
            // A pan far off it (#3326): the new grid's picture is not made yet, so the last one
            // stays, in its own frame; once the new one is made, it is shown in its frame.
            let old_origin = st.outline.as_ref().unwrap().origin;
            let moved = outline::Outline::new(interior, 1, [50000.0, -40000.0], 0.0, MAP.size(), p.scale);
            let new_origin = moved.origin;
            st.outline_last = st.outline.replace(moved);
            let parts = local_map(&mut st, &game, None).world.unwrap();
            assert_eq!((parts.outline, parts.view.local.unwrap().origin), (Some((handle.clone(), texels)), old_origin), "the last picture kept while the new one is made");
            let made = Assets::<Image>::default().add(Image::default());
            st.outline.as_mut().unwrap().image = Some(made.clone());
            let parts = local_map(&mut st, &game, None).world.unwrap();
            assert_eq!((parts.outline.map(|o| o.0), parts.view.local.unwrap().origin), (Some(made), new_origin), "the new picture, in the new grid's frame");
            // Another floor (or place): nothing of the last one.
            st.outline = Some(outline::Outline::new(interior, 1, [50000.0, -40000.0], 900.0, MAP.size(), p.scale));
            let parts = local_map(&mut st, &game, None).world.unwrap();
            assert_eq!(parts.outline, None, "another floor's grid, its picture not made yet");
        }
    }

    /// The review's case: the tile page drew the World Map in another window than the model
    /// gathered it in; the model's window, highlight box and Place Marker's place follow it.
    #[test]
    fn a_refitted_map_brings_the_highlight_box_and_place_markers_place_along() {
        use bevy::ecs::system::SystemState;
        use model::MapTransform;

        let root = WorldMap { root: 1, transform: MapTransform::default(), texture: "root.dds".into(), usable: [2048.0; 2], nw: [-2, 3], se: [1, 2] };
        let mut st = DataState { maps: [(1, Some(root.clone()))].into_iter().collect(), markers: Some(Vec::new()), ..DataState::default() };
        let mut world = World::new();
        world.insert_resource(Location { world: 1, interior: None });
        world.insert_resource(Player { heading: 0.0, pitch: 0.0, eye_height: 0.0, feet: [0.0, 3.0 * model::CELL, 0.0], body_eye: None, body_view: None, placed: None });
        let mut state = SystemState::<Game>::new(&mut world);
        let game = state.get_mut(&mut world).unwrap();
        let shown = world_map(&mut st, &game, None, false);
        let view = shown.world.unwrap().view;
        // Drawn in a smaller window elsewhere, the box 50 right of its middle.
        let moved = view.with_window(Rect::new(150.0, 120.0, 750.0, 520.0));
        let cursor = moved.mid() + Vec2::new(50.0, 0.0);
        // Place Marker's question asked this frame, of the place before the refit; a marker
        // picked again under the box.
        let before = st.spot[WORLD].expect("the place before");
        st.popup = Some(Popup::marker(marker::MarkerAsk::Set(before), "Here?".into(), vec!["Yes".into(), "No".into()]));
        // And a fast-travel question asked this frame of marker 41, not sent yet.
        st.travel_pending = Some(41);
        let travel = st.refit_map(WORLD, moved, cursor, cursor, Some(42), &[41, 42]);
        assert_eq!((st.travel_pending, st.travel_redrawn), (Some(41), true), "kept as asked, the map drawn again");
        assert_eq!(take_travel(&mut st), Some(42), "the question asks of the marker the box is on as drawn");
        assert_eq!((map_window(&st, WORLD), st.cursor[WORLD]), (moved.window, Some(cursor)));
        assert_eq!((st.cursor_marker, st.travel_marker, travel), (Some(42), Some(42), Some(42)), "the marker picked again is the Travel target");
        let spot = st.spot[WORLD].expect("Place Marker's place");
        let want = root.position(moved.to_uv(cursor));
        assert!((spot.pos[0] - want[0]).abs() < 0.5 && (spot.pos[1] - want[1]).abs() < 0.5, "{spot:?} {want:?}");
        assert!(spot.pos[0] > 400.0, "east of the player, under the box (about 500)");
        assert!(matches!(st.popup.as_ref().map(|p| &p.ask), Some(Ask::Marker(marker::MarkerAsk::Set(at))) if *at == spot), "the question asks of the new place");
        // No marker there after all: none under the box, no Travel target with no pointer in use
        // (a mouse pointer there while Controller is chosen is not in use: the preparation's
        // `aiming`, not the raw pointer).
        st.pointer_now = Some(Vec2::new(300.0, 300.0));
        st.aiming = false;
        st.travel_pending = Some(42);
        assert_eq!(st.refit_map(WORLD, moved, cursor, cursor, None, &[41, 42]), None);
        assert_eq!(take_travel(&mut st), None, "no marker under the box: not asked");
        // Not drawn again: asked of the marker it was asked of.
        st.travel_pending = Some(41);
        assert_eq!(take_travel(&mut st), Some(41));
        assert_eq!(st.cursor_marker, None);
        // Another tab: nothing.
        st.refit_map(QUESTS, moved, Vec2::ZERO, Vec2::ZERO, None, &[]);
        assert_eq!(st.cursor[WORLD], Some(cursor));
    }

    /// The arrow points the way the player faces: up (north) at heading 0, right (east) at a
    /// quarter turn, and so on round, the picture pointing right unturned; a turned picture's
    /// right is where (cos, sin) of its turn points with y down.
    #[test]
    fn the_arrow_points_the_way_the_player_faces() {
        use std::f32::consts::{FRAC_PI_2, PI};
        // The picture's arrow, its axis turned clockwise (y down) by the turn.
        let turned = |t: f32| {
            let (s, c) = t.sin_cos();
            Vec2::new(ARROW_AXIS.x * c - ARROW_AXIS.y * s, ARROW_AXIS.x * s + ARROW_AXIS.y * c).normalize()
        };
        assert!((arrow_turn(0.0) - 0.4157).abs() < 1e-3, "the picture's tip some 24 degrees left of up");
        for (heading, want) in [(0.0, Vec2::new(0.0, -1.0)), (FRAC_PI_2, Vec2::new(1.0, 0.0)), (PI, Vec2::new(0.0, 1.0)), (-FRAC_PI_2, Vec2::new(-1.0, 0.0))] {
            let points = turned(arrow_turn(heading));
            assert!((points - want).length() < 1e-5, "heading {heading}: {points:?}");
            // The way the player walks at that heading, north up on the map (y down).
            let forward = Vec2::new(heading.sin(), -heading.cos());
            assert!((points - forward).length() < 1e-5);
        }
    }

    /// The hand-built arrow shows the part of the picture the file's zoom shows: 1.4 times its
    /// texels at the 40 wide arrow, 28.6 × 36.6 texels of it, the whole of its arrow (its texels
    /// run to 24 across and 29 down).
    #[test]
    fn the_hand_built_arrow_shows_the_files_part_of_the_picture() {
        let shown = arrow_texels(CURSOR, CURSOR * CURSOR_ASPECT);
        assert!((shown - Vec2::new(40.0 / 1.4, 51.2 / 1.4)).length() < 1e-4, "{shown:?}");
        assert!(shown.x > 24.0 && shown.y > 29.0, "the arrow whole");
    }

    /// The review's two-stage case: a Travel request (the controller's A) whose tile refit puts the
    /// box on empty space, and a fallback in the same frame that draws it by hand on a marker: the
    /// last drawing decides, and the question asks of that marker.
    #[test]
    fn a_travel_request_is_decided_by_the_last_drawing() {
        use bevy::ecs::system::SystemState;
        use model::MapTransform;

        let root = WorldMap { root: 1, transform: MapTransform::default(), texture: "root.dds".into(), usable: [2048.0; 2], nw: [-2, 3], se: [1, 2] };
        // The player and marker 20 at the picture's middle.
        let here = [0.0, 3.0 * model::CELL, 0.0];
        let marker = read::Marker { form: 20, world: 1, pos: here, name: "Place 20".into(), kind: 1, flags: 1, radius: 10.0, reputation: None };
        let mut st = DataState { maps: [(1, Some(root))].into_iter().collect(), markers: Some(vec![marker]), ..DataState::default() };
        let mut world = World::new();
        world.insert_resource(Location { world: 1, interior: None });
        world.insert_resource(Player { heading: 0.0, pitch: 0.0, eye_height: 0.0, feet: here, body_eye: None, body_view: None, placed: None });
        let mut state = SystemState::<Game>::new(&mut world);
        let game = state.get_mut(&mut world).unwrap();
        world_map(&mut st, &game, None, false);
        assert_eq!(st.travel_marker, Some(20), "the box on marker 20 at the middle");
        // A asked of marker 20; the tile page's window, moved, puts the box's canvas place (the
        // hand-built middle, 477.5, 350) 27.5 right and 30 down of its own middle: out of reach.
        st.travel_pending = Some(20);
        let moved = MapView::bare(Rect::new(150.0, 120.0, 750.0, 520.0));
        let view = world_map(&mut st, &game, None, false).world.unwrap().view.with_window(moved.window);
        let want = Vec2::new(477.5, 350.0) - moved.window.min;
        assert_eq!(nearest(std::iter::once(view.to_window(view.to_uv(view.mid()))), want), None, "out of reach in the tile window");
        assert_eq!(st.refit_map(WORLD, view, want, want, None, &[20]), None, "no Travel target in the tile window");
        // The tile page falls back: drawn by hand, the box back on marker 20.
        refit_hand_built(&mut st, WORLD, &game, None).expect("gathered again");
        assert_eq!(st.travel_marker, Some(20));
        assert_eq!(take_travel(&mut st), Some(20), "asked of the marker under the box as drawn last");
    }

    fn shown(tab: usize, f: &HintFacts) -> Vec<(String, bool)> {
        tab_buttons(tab, f).iter().map(|b| (b.hint.text(), b.hint.dim)).collect()
    }

    fn hint(text: &str, dim: bool) -> (String, bool) {
        (text.to_string(), dim)
    }

    /// Every fact a hint can wait for, true: a Travel target, a quest running and
    /// selected, a note, a station.
    fn facts(pad: bool) -> HintFacts {
        HintFacts { pad, marker: true, quests: true, quest_done: Some(false), notes: true, stations: true, ..HintFacts::default() }
    }

    /// With the keyboard and mouse the original's PC set (#1897; docs/spec-notes/pipboy.md
    /// §5.3): no A hint anywhere, so Radio shows none; the maps Place Marker alone, bright
    /// (it asks about the custom marker, #1954), with no key;
    /// Quests Show Location with a quest; Misc the notes' filter with a note, and the
    /// challenges toggle always, Next Challenge (dim) in the filter's place while the
    /// challenges show.
    #[test]
    fn with_the_keyboard_each_tab_shows_the_pc_hints() {
        crate::pad::set_labels(crate::pad::Labels::Keys);
        let all = facts(false);
        let none = HintFacts::default();
        assert!(shown(RADIO, &all).is_empty(), "{:?}", shown(RADIO, &all));
        for tab in [LOCAL, WORLD] {
            assert_eq!(shown(tab, &all), [hint("Place Marker", false)], "tab {tab}");
        }
        assert_eq!(shown(QUESTS, &all), [hint("Show Location [X]", false)]);
        assert_eq!(shown(NOTES, &all), [hint("Active Notes [X]", false), hint("Challenges [R]", false)]);
        assert_eq!(shown(NOTES, &HintFacts { notes_of_active: true, ..all }), [hint("All Notes [X]", false), hint("Challenges [R]", false)]);
        assert_eq!(shown(NOTES, &HintFacts { challenges: true, ..all }), [hint("Next Challenge [X]", true), hint("Challenges [R]", false)]);
        // An audio note's Play is the controller's only.
        assert_eq!(shown(NOTES, &HintFacts { audio: Some(false), ..all }), shown(NOTES, &all));
        // No quest: nothing on Quests; no note: only the challenges toggle.
        assert!(shown(QUESTS, &none).is_empty());
        assert_eq!(shown(NOTES, &none), [hint("Challenges [R]", false)]);
        assert_eq!(shown(NOTES, &HintFacts { challenges: true, ..none }), [hint("Challenges [R]", false)]);
    }

    /// With a controller the original's column (#1897, the issue's screenshots): Local
    /// Map Place Marker; World Map Travel (dim without a Travel target) and Place Marker;
    /// Quests the make-active hint (dim on a quest done) and Show Location, none without a
    /// quest; Misc Play (Stop while it plays) on an audio note, the notes' filter and the
    /// challenges toggle; Radio Tune, none without a station. No Y hint on the maps.
    #[test]
    fn with_a_controller_each_tab_shows_the_originals_column() {
        crate::pad::set_labels(crate::pad::Labels::Xbox);
        let all = facts(true);
        let none = HintFacts { pad: true, ..HintFacts::default() };
        let got = [
            shown(LOCAL, &all),
            shown(WORLD, &all),
            shown(WORLD, &HintFacts { marker: false, ..all }),
            shown(QUESTS, &all),
            shown(QUESTS, &HintFacts { quest_done: Some(true), ..all }),
            shown(QUESTS, &none),
            shown(NOTES, &all),
            shown(NOTES, &HintFacts { audio: Some(false), ..all }),
            shown(NOTES, &HintFacts { audio: Some(true), notes_of_active: true, ..all }),
            shown(NOTES, &HintFacts { challenges: true, ..all }),
            shown(NOTES, &none),
            shown(RADIO, &all),
            shown(RADIO, &none),
        ];
        crate::pad::set_labels(crate::pad::Labels::Keys);
        let want: [Vec<(String, bool)>; 13] = [
            vec![hint("Place Marker [X]", false)],
            vec![hint("Travel [A]", false), hint("Place Marker [X]", false)],
            vec![hint("Travel [A]", true), hint("Place Marker [X]", false)],
            vec![hint("Set Active [A]", false), hint("Show Location [X]", false)],
            vec![hint("Set Active [A]", true), hint("Show Location [X]", false)],
            vec![],
            vec![hint("Active Notes [X]", false), hint("Challenges [Y]", false)],
            vec![hint("Play [A]", false), hint("Active Notes [X]", false), hint("Challenges [Y]", false)],
            vec![hint("Stop [A]", false), hint("All Notes [X]", false), hint("Challenges [Y]", false)],
            vec![hint("Next Challenge [X]", true), hint("Challenges [Y]", false)],
            vec![hint("Challenges [Y]", false)],
            vec![hint("Tune In [A]", false)],
            vec![],
        ];
        for (k, (g, w)) in got.iter().zip(&want).enumerate() {
            assert_eq!(g, w, "case {k}");
        }
    }

    /// The Travel hint's marker is kept while the pointer goes from the marker to the hint
    /// (#1897): the highlight box leaves the marker on the way, the target stays; with no
    /// pointer (a controller) it is the marker under the box; only a marker still shown.
    #[test]
    fn the_travel_target_is_kept_on_the_way_to_the_hint() {
        let shown = [5, 7];
        assert_eq!(travel_target(None, Some(5), true, &shown), Some(5));
        // The pointer leaves the marker for the hint over empty map: kept.
        assert_eq!(travel_target(Some(5), None, true, &shown), Some(5));
        // Onto another marker: that one.
        assert_eq!(travel_target(Some(5), Some(7), true, &shown), Some(7));
        // No pointer: only the marker under the box.
        assert_eq!(travel_target(Some(5), None, false, &shown), None);
        assert_eq!(travel_target(Some(5), Some(7), false, &shown), Some(7));
        // A marker no longer shown is no target.
        assert_eq!(travel_target(Some(9), None, true, &shown), None);
    }

    /// Over a button the pointer is not on the map (#1897): a press and release on the
    /// Travel hint asks to travel once, to the Travel target, though another marker lies
    /// under the button; a click on the map on a marker asks for it and makes it the
    /// target, and a click away from any marker drops the target.
    #[test]
    fn a_click_on_a_button_is_never_a_map_click() {
        // A button where the buttons start, inside the map's window.
        let button = Rect::new(MAIN.min.x + BUTTONS.x - 200.0, MAIN.min.y + BUTTONS.y, MAIN.min.x + BUTTONS.x, MAIN.min.y + BUTTONS.y + 45.0);
        let over = Some(button.center());
        assert!(in_map(over).is_some());
        let mut st = DataState { button_rects: (WORLD, vec![button]), cursor_marker: Some(7), travel_marker: Some(5), ..DataState::default() };
        assert!(on_button(&st, WORLD, over));
        assert!(!on_button(&st, QUESTS, over), "the rects of another tab's buttons");
        // The press on the button is no press on the map.
        assert_eq!(travel_ask(&mut st, WORLD, over, (true, false), false), None);
        assert_eq!(st.press, None);
        // The release: the button's click (`travel`) asks once, for the Travel target.
        assert_eq!(travel_ask(&mut st, WORLD, over, (false, true), true), Some(5));
        // A release over the button alone asks nothing.
        assert_eq!(travel_ask(&mut st, WORLD, over, (false, true), false), None);
        assert_eq!(st.travel_marker, Some(5));
        // On the map, a click on the marker under the box (as drawn on the release's frame:
        // the box is not drawn here, so the marker it was on), taken once the map is drawn.
        let on_map = Some(MAIN.min + MAP.min + Vec2::new(100.0, 300.0));
        assert!(!on_button(&st, WORLD, on_map));
        let (mut world, mut game) = blank_game();
        let mut game = game.get_mut(&mut world).unwrap();
        assert_eq!(travel_ask(&mut st, WORLD, on_map, (true, false), false), None);
        assert_eq!(travel_ask(&mut st, WORLD, on_map, (false, true), false), None);
        take_click(&mut st, &mut game, false);
        assert_eq!((st.travel_pending.take(), st.travel_marker), (Some(7), Some(7)));
        // Enter in the same frame as that click still asks once.
        assert_eq!(travel_ask(&mut st, WORLD, on_map, (true, false), false), None);
        assert_eq!(travel_ask(&mut st, WORLD, on_map, (false, true), true), Some(7));
        take_click(&mut st, &mut game, false);
        assert_eq!(st.travel_pending.take(), Some(7));
        // A click away from any marker drops the target.
        st.cursor_marker = None;
        travel_ask(&mut st, WORLD, on_map, (true, false), false);
        assert_eq!(travel_ask(&mut st, WORLD, on_map, (false, true), false), None);
        take_click(&mut st, &mut game, false);
        assert_eq!((st.travel_pending, st.travel_marker), (None, None));
    }

    /// A game with nothing in it: no location, no player, no scripts.
    fn blank_game() -> (World, bevy::ecs::system::SystemState<Game<'static>>) {
        let mut world = World::new();
        let state = bevy::ecs::system::SystemState::<Game>::new(&mut world);
        (world, state)
    }

    /// #3324: a press and release on a map within the slop is a click ([`DataState::map_click`])
    /// on either map, resolved later against the box as drawn; a press that moved more than the
    /// slop before it was let go (the move seen in the release's own frame too) is a drag; a
    /// press on a hint is the hint's; the other tabs have no map to click; the Local Map never
    /// travels by Enter.
    #[test]
    fn a_click_away_from_any_marker_places_the_custom_marker() {
        let at = MAIN.min + MAP.min + Vec2::new(100.0, 300.0);
        let p = Some(at);
        let moved = Some(MAIN.min + MAP.min + Vec2::new(100.0 + CLICK_SLOP + 4.0, 300.0));
        let mut st = DataState::default();
        assert_eq!(travel_ask(&mut st, WORLD, p, (true, false), false), None);
        assert_eq!(st.map_click, None, "the press alone is no click");
        assert_eq!(travel_ask(&mut st, WORLD, p, (false, true), false), None);
        assert_eq!(st.map_click, Some(WORLD), "the click, taken once the map is drawn");
        st.map_click = None;
        // A drag: moved past the slop while held, or in the release's frame.
        travel_ask(&mut st, WORLD, p, (true, false), false);
        travel_ask(&mut st, WORLD, moved, (false, false), false);
        travel_ask(&mut st, WORLD, p, (false, true), false);
        assert_eq!(st.map_click, None, "moved away and back: a drag");
        travel_ask(&mut st, LOCAL, p, (true, false), false);
        travel_ask(&mut st, LOCAL, moved, (false, true), false);
        assert_eq!(st.map_click, None, "the pointer moved in the release's frame: a drag");
        // The Local Map: a click, and Enter never travels there.
        st.travel_marker = Some(5);
        travel_ask(&mut st, LOCAL, p, (true, false), false);
        assert_eq!(travel_ask(&mut st, LOCAL, p, (false, true), true), None);
        assert_eq!(st.map_click, Some(LOCAL));
        st.map_click = None;
        // A press on a hint over the map is the hint's.
        let hint = Rect::from_center_size(at, Vec2::splat(40.0));
        st.button_rects = (WORLD, vec![hint]);
        travel_ask(&mut st, WORLD, p, (true, false), false);
        travel_ask(&mut st, WORLD, p, (false, true), false);
        assert_eq!(st.map_click, None);
        // No map on Quests.
        travel_ask(&mut st, QUESTS, p, (true, false), false);
        travel_ask(&mut st, QUESTS, p, (false, true), false);
        assert!(st.map_click.is_none() && st.press.is_none());
    }

    /// The page's frame as `draw` runs it with `MapMenu` off (the hand-built page; open while the
    /// Pip-Boy shows DATA, as its body is there then), or with it on `prepare` and then `draw`
    /// (no body: the tile page drew, and the frame's click and travel are taken there).
    fn page_frame(w: &mut World, tiles: bool) {
        page_prepare(w, tiles);
        page_finish(w, tiles);
    }

    /// The first half of [`page_frame`]: the preparation (with `MapMenu` on, before the tile page
    /// lays the map out).
    fn page_prepare(w: &mut World, tiles: bool) {
        use bevy::ecs::system::RunSystemOnce;
        fn hand_built(mut st: ResMut<DataState>, pip: Res<PipBoy>, screen: Option<Res<PipBoyScreen>>, mut game: Game, mut input: Input, mut images: ResMut<Assets<Image>>) {
            let open = (pip.open() && pip.page == Page::Data).then(|| pip.tab());
            step(&mut st, open, screen.as_deref(), &mut game, &mut input, &mut images, None);
        }
        if tiles {
            w.run_system_once(prepare).unwrap();
        } else {
            w.run_system_once(hand_built).unwrap();
        }
    }

    /// The second half of [`page_frame`]: with `MapMenu` on `draw` after the tile page; then the
    /// frame's input is let go.
    fn page_finish(w: &mut World, tiles: bool) {
        use bevy::ecs::system::RunSystemOnce;
        if tiles {
            w.run_system_once(draw).unwrap();
        }
        w.resource_mut::<ButtonInput<MouseButton>>().clear();
        w.resource_mut::<ButtonInput<KeyCode>>().clear();
    }

    /// The fast-travel question asked: still pending (the hand-built frame here sends nothing),
    /// or sent by `draw`; taken.
    fn travel_asked(w: &mut World) -> Option<u32> {
        let sent: Vec<FastTravelAsk> = w.resource_mut::<Messages<FastTravelAsk>>().drain().collect();
        w.resource_mut::<DataState>().travel_pending.take().or(sent.last().map(|a| a.0))
    }

    /// A page world on the World Map of worldspace 1 (a 2048 picture, the player at its middle,
    /// `markers` the known map markers), the pointer at `at`; with `tiles` the page drawn from the
    /// file, which published `window`.
    fn map_world(tiles: bool, window: Rect, at: Vec2, markers: Vec<read::Marker>) -> World {
        let mut w = page_world(tiles, data_open(WORLD));
        let map = WorldMap { root: 1, transform: model::MapTransform::default(), texture: "root.dds".into(), usable: [2048.0; 2], nw: [-2, 3], se: [1, 2] };
        let discovered = markers.iter().map(|m| m.form).collect();
        w.insert_resource(DataState { maps: [(1, Some(map))].into_iter().collect(), markers: Some(markers), discovered, map_window: tiles.then_some((WORLD, window)), ..DataState::default() });
        w.insert_resource(Location { world: 1, interior: None });
        w.insert_resource(Player { heading: 0.0, pitch: 0.0, eye_height: 0.0, feet: MAP_FEET, body_eye: None, body_view: None, placed: None });
        w.insert_resource(PipBoyScreen { image: Handle::default(), camera: Entity::PLACEHOLDER, on_model: false, pointer: Some(at), scale: 1.0 });
        w
    }

    /// Where [`map_world`]'s player stands: the picture's middle.
    const MAP_FEET: [f32; 3] = [0.0, 3.0 * model::CELL, 100.0];

    /// The map window for the layout: the page drawn from the file publishes its own, 30 right
    /// and 20 down of the hand-built one here, so a click is read where that page draws the map.
    fn layout_window(tiles: bool) -> Rect {
        let shift = if tiles { Vec2::new(30.0, 20.0) } else { Vec2::ZERO };
        Rect::from_corners(hand_built_window().min + shift, hand_built_window().max + shift)
    }

    fn mouse(w: &mut World, down: bool) {
        let mut b = w.resource_mut::<ButtonInput<MouseButton>>();
        if down {
            b.press(MouseButton::Left);
        } else {
            b.release(MouseButton::Left);
        }
    }

    /// #3324 through the page's own frame with real mouse button input, on both layouts: the
    /// hand-built page and the page drawn from the file (`prepare`, its published window). A press
    /// and release 50 right of the map window's middle, the map on the player, puts up Place
    /// Marker's question for the world position there (what the frame's view carries too); with
    /// the box up another click places nothing; Y places it there. A press dragged past the slop
    /// pans the map and asks nothing; a press on a hint over the map asks nothing.
    #[test]
    fn a_real_click_on_the_map_asks_place_marker_at_the_point_clicked() {
        let x = 50.0 / 2048.0 / model::MAP_SPAN * 4.0 * model::CELL;
        for tiles in [false, true] {
            let window = layout_window(tiles);
            let at = window.min + MAP_MID + Vec2::new(50.0, 0.0);
            let mut w = map_world(tiles, window, at, Vec::new());
            // The page comes up (the maps centre on the player); then the press and release.
            page_frame(&mut w, tiles);
            mouse(&mut w, true);
            page_frame(&mut w, tiles);
            assert!(w.resource::<DataState>().popup.is_none(), "tiles {tiles}: the press alone asks nothing");
            mouse(&mut w, false);
            page_frame(&mut w, tiles);
            let st = w.resource::<DataState>();
            let (ask, answers) = asked(st);
            let marker::MarkerAsk::Set(spot) = ask else { panic!("tiles {tiles}: {ask:?}") };
            assert_eq!(answers, 2);
            assert!((spot.pos[0] - x).abs() < 0.5 && (spot.pos[1] - MAP_FEET[1]).abs() < 0.5 && spot.space == 1, "tiles {tiles}: {spot:?}");
            // The box is modal: a click under it places nothing more.
            mouse(&mut w, true);
            page_frame(&mut w, tiles);
            mouse(&mut w, false);
            page_frame(&mut w, tiles);
            assert!(w.resource::<DataState>().custom.is_none(), "tiles {tiles}");
            assert!(matches!(asked(w.resource::<DataState>()).0, marker::MarkerAsk::Set(s) if s == spot), "tiles {tiles}: the same question");
            // Y: placed there.
            w.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyY);
            page_frame(&mut w, tiles);
            let st = w.resource::<DataState>();
            assert!(st.popup.is_none());
            assert_eq!(st.custom, Some(spot), "tiles {tiles}");
            // A drag: the press moved 40 before it was let go pans the map and asks nothing.
            let centre = w.resource::<DataState>().centre[WORLD];
            mouse(&mut w, true);
            page_frame(&mut w, tiles);
            w.resource_mut::<PipBoyScreen>().pointer = Some(at + Vec2::new(40.0, 0.0));
            page_frame(&mut w, tiles);
            mouse(&mut w, false);
            page_frame(&mut w, tiles);
            let st = w.resource::<DataState>();
            assert!(st.popup.is_none(), "tiles {tiles}: a drag asks nothing");
            assert_ne!(st.centre[WORLD], centre, "tiles {tiles}: the drag panned the map");
            // A press and release on a hint over the map: the hint's, not the map's.
            let hint = Rect::from_center_size(at, Vec2::splat(40.0));
            w.resource_mut::<PipBoyScreen>().pointer = Some(at);
            w.resource_mut::<DataState>().button_rects = (WORLD, vec![hint]);
            mouse(&mut w, true);
            page_frame(&mut w, tiles);
            mouse(&mut w, false);
            page_frame(&mut w, tiles);
            assert!(w.resource::<DataState>().popup.is_none(), "tiles {tiles}: the hint's click");
        }
    }

    /// #3324 (review): a click is resolved against the highlight box as drawn on the frame the
    /// button is let go, across the snap boundary both ways. A map marker at the player (the
    /// window's middle): pressed 38 right of it (the box on the marker), let go 43 right (within
    /// the slop; the box off it) asks Place Marker's question and no travel; pressed 43 right, let
    /// go 38 right asks to travel to the marker and puts up no question. Both layouts.
    #[test]
    fn a_click_reads_the_highlight_as_drawn_when_the_button_is_let_go() {
        let marker = read::Marker { form: 0x42, world: 1, pos: MAP_FEET, name: "Here".into(), kind: 1, flags: 0, radius: 0.0, reputation: None };
        for tiles in [false, true] {
            let window = layout_window(tiles);
            let mid = window.min + MAP_MID;
            for (down, up, travels) in [(38.0, 43.0, false), (43.0, 38.0, true)] {
                let mut w = map_world(tiles, window, mid + Vec2::new(down, 0.0), vec![marker.clone()]);
                page_frame(&mut w, tiles);
                mouse(&mut w, true);
                page_frame(&mut w, tiles);
                assert_eq!(w.resource::<DataState>().cursor_marker, (down < SNAP).then_some(0x42), "tiles {tiles}: the box as pressed");
                w.resource_mut::<PipBoyScreen>().pointer = Some(mid + Vec2::new(up, 0.0));
                mouse(&mut w, false);
                page_frame(&mut w, tiles);
                let travel = travel_asked(&mut w);
                let st = w.resource::<DataState>();
                assert_eq!(st.cursor_marker, travels.then_some(0x42), "tiles {tiles}: the box as let go");
                if travels {
                    assert_eq!(travel, Some(0x42), "tiles {tiles}: pressed off the marker, let go on it: travel");
                    assert!(st.popup.is_none(), "tiles {tiles}: and no Place Marker question");
                } else {
                    assert_eq!(travel, None, "tiles {tiles}: pressed on the marker, let go off it: no travel");
                    assert!(matches!(asked(st).0, marker::MarkerAsk::Set(_)), "tiles {tiles}: Place Marker's question");
                }
            }
        }
    }

    /// #3324 (review): a worldspace without map data draws a blank World Map, and nothing is left
    /// under its highlight box from the map shown before: a click there places nothing and asks
    /// no travel, and Place Marker finds no place.
    #[test]
    fn a_blank_world_map_takes_no_click() {
        for tiles in [false, true] {
            let window = layout_window(tiles);
            let at = window.min + MAP_MID + Vec2::new(50.0, 0.0);
            let marker = read::Marker { form: 0x42, world: 1, pos: MAP_FEET, name: "Here".into(), kind: 1, flags: 0, radius: 0.0, reputation: None };
            let mut w = map_world(tiles, window, at, vec![marker]);
            page_frame(&mut w, tiles);
            assert!(w.resource::<DataState>().spot[WORLD].is_some(), "tiles {tiles}: worldspace 1's map, a place under the box");
            w.resource_mut::<DataState>().cursor_marker = Some(0x42);
            w.resource_mut::<DataState>().travel_marker = Some(0x42);
            w.resource_mut::<DataState>().companions_under = vec![FormId(9)];
            // Worldspace 2 has no map data.
            w.resource_mut::<DataState>().maps.insert(2, None);
            w.resource_mut::<Location>().world = 2;
            page_frame(&mut w, tiles);
            let st = w.resource::<DataState>();
            assert_eq!((st.spot[WORLD], st.cursor_marker, st.travel_marker), (None, None, None), "tiles {tiles}: nothing left from worldspace 1");
            assert!(st.companions_under.is_empty(), "tiles {tiles}: no companions left under the box");
            mouse(&mut w, true);
            page_frame(&mut w, tiles);
            mouse(&mut w, false);
            page_frame(&mut w, tiles);
            let travel = travel_asked(&mut w);
            let st = w.resource::<DataState>();
            assert!(st.popup.is_none() && st.custom.is_none() && travel.is_none(), "tiles {tiles}: the click takes nothing");
        }
    }

    /// #3324 (review): a press held while DATA closes (another page and back, the button still
    /// down, let go where it went down) is no click on the map it comes back to, and no drag.
    #[test]
    fn a_press_held_across_leaving_data_is_no_click() {
        for tiles in [false, true] {
            let window = layout_window(tiles);
            let at = window.min + MAP_MID + Vec2::new(50.0, 0.0);
            let mut w = map_world(tiles, window, at, Vec::new());
            page_frame(&mut w, tiles);
            mouse(&mut w, true);
            page_frame(&mut w, tiles);
            assert!(w.resource::<DataState>().press.is_some());
            w.resource_mut::<PipBoy>().page = Page::Stats;
            page_frame(&mut w, tiles);
            let st = w.resource::<DataState>();
            assert!(st.press.is_none() && st.drag_from.is_none(), "tiles {tiles}: let go as DATA closed");
            w.resource_mut::<PipBoy>().page = Page::Data;
            page_frame(&mut w, tiles);
            let centre = w.resource::<DataState>().centre[WORLD];
            // Moved 3 (within the slop) while still held, then let go where it went down.
            w.resource_mut::<PipBoyScreen>().pointer = Some(at + Vec2::new(3.0, 0.0));
            page_frame(&mut w, tiles);
            w.resource_mut::<PipBoyScreen>().pointer = Some(at);
            mouse(&mut w, false);
            page_frame(&mut w, tiles);
            let travel = travel_asked(&mut w);
            let st = w.resource::<DataState>();
            assert!(st.popup.is_none() && travel.is_none(), "tiles {tiles}: no click");
            assert_eq!(st.centre[WORLD], centre, "tiles {tiles}: no drag");
        }
    }

    /// #3324 (review): with `MapMenu` on, the tile page may lay the map out again after the
    /// preparation (its refit, `DataState::refit_map`, here crossing the snap boundary), so the
    /// click is taken after that: let go 43 right of a marker (off it as prepared) and refitted
    /// onto it, it asks to travel and puts up no question; let go 38 right (on it as prepared) and
    /// refitted off it, it asks Place Marker's question and no travel.
    #[test]
    fn a_click_reads_the_highlight_after_the_tile_refit() {
        let marker = read::Marker { form: 0x42, world: 1, pos: MAP_FEET, name: "Here".into(), kind: 1, flags: 0, radius: 0.0, reputation: None };
        let window = layout_window(true);
        let mid = window.min + MAP_MID;
        for (off, refit_on, travels) in [(43.0, true, true), (38.0, false, false)] {
            let mut w = map_world(true, window, mid + Vec2::new(off, 0.0), vec![marker.clone()]);
            page_frame(&mut w, true);
            mouse(&mut w, true);
            page_frame(&mut w, true);
            mouse(&mut w, false);
            page_prepare(&mut w, true);
            assert_eq!(w.resource::<DataState>().cursor_marker, (off < SNAP).then_some(0x42), "as prepared");
            assert!(w.resource::<DataState>().popup.is_none(), "nothing asked before the tile page laid the map out");
            // The tile page's refit: the box picked again in the window it drew.
            {
                let mut st = w.resource_mut::<DataState>();
                let view = MapView::world(window, st.centre[WORLD].unwrap(), Vec2::splat(2048.0), st.magnify[WORLD].max(ZOOM.0));
                let want = MAP_MID + Vec2::new(off, 0.0);
                let (cursor, picked) = if refit_on { (MAP_MID, Some(0x42)) } else { (want, None) };
                st.refit_map(WORLD, view, want, cursor, picked, &[0x42]);
            }
            page_finish(&mut w, true);
            let travel = travel_asked(&mut w);
            let st = w.resource::<DataState>();
            if travels {
                assert_eq!(travel, Some(0x42), "refitted onto the marker: travel");
                assert!(st.popup.is_none(), "and no Place Marker question");
            } else {
                assert_eq!(travel, None, "refitted off the marker: no travel");
                assert!(matches!(asked(st).0, marker::MarkerAsk::Set(_)), "Place Marker's question");
            }
        }
    }

    /// #3324 (review): the Map Selectable Companions tweak's summon is the same click, taken with
    /// the box as drawn: companions under it are summoned, before a travel or Place Marker, and
    /// the travel Enter asked in the frame is dropped; a blank map has no companions under it.
    #[test]
    fn a_click_on_companions_summons_them_as_drawn() {
        let (mut world, mut game) = blank_game();
        let mut game = game.get_mut(&mut world).unwrap();
        let mut st = DataState { map_click: Some(WORLD), companions_under: vec![FormId(9)], cursor_marker: Some(7), travel_pending: Some(5), ..DataState::default() };
        let summoned = take_click(&mut st, &mut game, false);
        assert_eq!(summoned.map(|s| s.0), Some(vec![FormId(9)]));
        assert_eq!((st.travel_pending, st.map_click), (None, None));
        assert!(st.popup.is_none());
        // No click this frame: nothing summoned.
        assert!(take_click(&mut st, &mut game, false).is_none());
        blank_map(&mut st, WORLD);
        assert!(st.companions_under.is_empty());
    }

    /// #3324 (review): Enter and a click on an empty spot in the same frame (the Travel target
    /// kept from a marker the box left): the click wins, Place Marker asks and no travel is
    /// asked [design]. Both layouts.
    #[test]
    fn a_click_wins_over_enter_in_the_same_frame() {
        let marker = read::Marker { form: 0x42, world: 1, pos: MAP_FEET, name: "Here".into(), kind: 1, flags: 0, radius: 0.0, reputation: None };
        for tiles in [false, true] {
            let window = layout_window(tiles);
            let mut w = map_world(tiles, window, window.min + MAP_MID + Vec2::new(80.0, 0.0), vec![marker.clone()]);
            page_frame(&mut w, tiles);
            w.resource_mut::<DataState>().travel_marker = Some(0x42);
            mouse(&mut w, true);
            page_frame(&mut w, tiles);
            assert_eq!(w.resource::<DataState>().travel_marker, Some(0x42), "tiles {tiles}: the Travel target kept");
            mouse(&mut w, false);
            w.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
            page_frame(&mut w, tiles);
            assert_eq!(travel_asked(&mut w), None, "tiles {tiles}: Enter's travel dropped");
            assert!(matches!(asked(w.resource::<DataState>()).0, marker::MarkerAsk::Set(_)), "tiles {tiles}: the click's question");
        }
    }

    /// #3324 (review): X on the first frame in a worldspace without map data (before its blank map
    /// is built) finds no place, not the one the map shown before left. Both layouts.
    #[test]
    fn place_marker_on_the_first_frame_of_a_blank_map_finds_no_place() {
        for tiles in [false, true] {
            let window = layout_window(tiles);
            let mut w = map_world(tiles, window, window.min + MAP_MID + Vec2::new(50.0, 0.0), Vec::new());
            page_frame(&mut w, tiles);
            assert!(w.resource::<DataState>().spot[WORLD].is_some());
            w.resource_mut::<DataState>().maps.insert(2, None);
            w.resource_mut::<Location>().world = 2;
            w.resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::KeyX);
            page_frame(&mut w, tiles);
            let st = w.resource::<DataState>();
            assert!(st.popup.is_none() && st.custom.is_none(), "tiles {tiles}: no question, nothing placed");
            assert_eq!(st.spot[WORLD], None);
        }
    }

    #[test]
    fn held_map_click_refuses_switch_and_releases_on_the_original_device() {
        use crate::{bindings::InputDevice, pad::device::tests::{button, playing_app}};
        use bevy::input::{mouse::MouseButtonInput, ButtonState};
        fn read(mut st: ResMut<DataState>, mut game: Game, mut input: Input) {
            take_input(&mut st, WORLD, &Lists::default(), &mut game, &mut input,
                Some(MAIN.min + MAP.min + Vec2::new(100.0, 300.0)), false);
            // As `step` takes the click once the map is drawn (not drawn here: the marker stays
            // under the box).
            take_click(&mut st, &mut game, false);
        }
        let (mut app, pad) = playing_app(InputDevice::KeyboardMouse);
        app.insert_resource(DataState { cursor_marker: Some(7), ..default() })
            .init_resource::<PipBoyAid>().add_message::<MenuInput>().add_message::<PipBoyInput>()
            .add_message::<FastTravelAsk>().add_message::<FastTravelGo>().add_message::<SummonCompanions>().add_systems(Update, read);
        let mouse = MouseButtonInput { button: MouseButton::Left, state: ButtonState::Pressed, window: Entity::PLACEHOLDER };
        app.world_mut().write_message(mouse);
        app.update();
        assert!(app.world().resource::<DataState>().press.is_some());
        button(&mut app, pad, true);
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
        assert!(app.world().resource::<DataState>().press.is_some());
        assert_eq!(app.world().resource::<DataState>().travel_pending, None);
        app.world_mut().write_message(MouseButtonInput { state: ButtonState::Released, ..mouse });
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::KeyboardMouse);
        assert!(app.world().resource::<DataState>().press.is_none());
        assert_eq!(app.world().resource::<DataState>().travel_pending, Some(7));
        app.world_mut().resource_mut::<DataState>().travel_pending = None;
        button(&mut app, pad, false);
        app.update();
        button(&mut app, pad, true);
        app.update();
        assert_eq!(app.world().resource::<Bindings>().device, InputDevice::Controller);
        assert_eq!(app.world().resource::<DataState>().travel_pending, None);
    }

    /// The Misc tab's A hint (#1897): Play on a Sound or Voice note, Stop only on the Voice
    /// note the holotape plays (not paused); a Sound note keeps Play while its sound plays,
    /// even were the holotape's note its own (it never is: `crate::holotape` follows Voice
    /// notes only); none on a text or image note, or with no note.
    #[test]
    fn the_audio_hint_reads_stop_only_on_the_voice_note_playing() {
        let note = |form, kind| read::Note { form, name: String::new(), kind, text: None, image: None, quest: None };
        let (voice, sound, text, image) = (note(1, NOTE_VOICE), note(2, NOTE_SOUND), note(3, 1), note(4, 2));
        assert_eq!(audio_hint(Some(&voice), (None, false)), Some(false));
        assert_eq!(audio_hint(Some(&voice), (Some(FormId(1)), false)), Some(true));
        assert_eq!(audio_hint(Some(&voice), (Some(FormId(1)), true)), Some(false), "paused");
        assert_eq!(audio_hint(Some(&voice), (Some(FormId(9)), false)), Some(false), "another note plays");
        assert_eq!(audio_hint(Some(&sound), (None, false)), Some(false));
        assert_eq!(audio_hint(Some(&sound), (Some(FormId(2)), false)), Some(false), "a Sound note is not followed");
        assert_eq!(audio_hint(Some(&text), (None, false)), None);
        assert_eq!(audio_hint(Some(&image), (None, false)), None);
        assert_eq!(audio_hint(None, (Some(FormId(1)), false)), None);
    }

    /// The hints are spawned after the map's window, its markers and its `MapHit` (and
    /// before the question box), as siblings with no z-index of their own: a later sibling
    /// is drawn and picked over an earlier one, so a click on the World Map's Travel hint,
    /// which lies over the map's window, is the hint's (#1897: the hints came first and the
    /// map took the click). Drawn through the page's own `show` into a world.
    #[test]
    fn the_hints_are_spawned_over_the_map_and_under_the_question_box() {
        use bevy::ecs::world::CommandQueue;
        crate::pad::set_labels(crate::pad::Labels::Xbox);
        let mut kit = Kit::default();
        let mut images = Assets::<Image>::default();
        let glyphs = crate::pad::PadGlyphs::default();
        let mut c = Ctx::new(&mut kit, None, &mut images, None, &glyphs);
        let buttons = tab_buttons(WORLD, &facts(true));
        crate::pad::set_labels(crate::pad::Labels::Keys);
        let map = empty_map();
        let view = View { tab: WORLD, place: String::new(), when: String::new(), buttons, body: Body::Map(Box::new(map)), popup: Some(Popup::travel(7, "Travel?".into())) };
        let mut st = DataState::default();
        let mut world = World::new();
        let mut queue = CommandQueue::default();
        let mut commands = Commands::new(&mut queue, &world);
        let root = commands.spawn(Node::default()).with_children(|p| show(p, &mut c, &mut st, &view)).id();
        queue.apply(&mut world);
        // Each child of the page's root by what its subtree holds.
        fn holds<T: Component>(world: &World, e: Entity) -> bool {
            world.get::<T>(e).is_some() || world.get::<Children>(e).is_some_and(|c| c.iter().any(|k| holds::<T>(world, k)))
        }
        let children: Vec<Entity> = world.get::<Children>(root).unwrap().iter().collect();
        let at = |f: &dyn Fn(Entity) -> bool| children.iter().position(|&e| f(e));
        let map_at = at(&|e| holds::<MapHit>(&world, e)).expect("the map's window");
        let hints: Vec<usize> = children.iter().enumerate().filter(|(_, &e)| world.get::<ButtonHit>(e).is_some()).map(|(k, _)| k).collect();
        let popup_at = at(&|e| holds::<PopupHit>(&world, e)).expect("the question box");
        assert_eq!(hints.len(), 2, "Travel and Place Marker");
        assert!(hints.iter().all(|&k| k > map_at && k < popup_at), "map {map_at}, hints {hints:?}, question box {popup_at}");
        // The order alone decides: nothing on the page sets a z-index (every node has the
        // default 0, a component `Node` requires).
        let mut all = vec![root];
        let mut k = 0;
        while k < all.len() {
            all.extend(world.get::<Children>(all[k]).into_iter().flat_map(|c| c.iter()));
            k += 1;
        }
        assert!(all.iter().all(|&e| world.get::<ZIndex>(e).is_none_or(|z| z.0 == 0) && world.get::<GlobalZIndex>(e).is_none()));
    }

    /// The hints are the ITEMS page's column at the file's place (#1897): the first row's
    /// text `BUTTON_TEXT_Y` under 75, the rows 46 apart, every word ending on one line at
    /// the heading's right end unless a long key moves the column left, each hint inside
    /// what the 1280 x 720 screen shows of the canvas (`super::super::SCREEN_CROP`) and
    /// right of the list, with every set of labels (a PlayStation button's name written
    /// out, the glyphs not loaded, too); the click boxes meet and do not overlap.
    #[test]
    fn the_hints_are_a_column_at_the_files_place() {
        use super::super::frame::{BUTTON_TEXT_Y, HINT_PITCH};
        let mut kit = Kit::default();
        let mut images = Assets::<Image>::default();
        let glyphs = crate::pad::PadGlyphs::default();
        let c = Ctx::new(&mut kit, None, &mut images, None, &glyphs);
        let top = MAIN.min.y + BUTTONS.y + BUTTON_TEXT_Y;
        for labels in [crate::pad::Labels::Keys, crate::pad::Labels::Xbox, crate::pad::Labels::PlayStation] {
            crate::pad::set_labels(labels);
            let pad = labels != crate::pad::Labels::Keys;
            let sets = [
                HintFacts { audio: Some(false), ..facts(pad) },
                HintFacts { audio: Some(true), notes_of_active: true, ..facts(pad) },
                HintFacts { challenges: true, ..facts(pad) },
            ];
            for f in sets {
                for tab in [LOCAL, WORLD, QUESTS, NOTES, RADIO] {
                    let list = tab_buttons(tab, &f);
                    let rows = button_rows(&c, &list);
                    let ends: Vec<f32> = list.iter().zip(&rows).map(|(b, r)| r.min.x + crate::pad::hint_width(c.kit.face(2), b.hint.word, c.glyphs)).collect();
                    for (k, (b, r)) in list.iter().zip(&rows).enumerate() {
                        assert!((r.min.y - (top + HINT_PITCH * k as f32)).abs() < 1e-3, "{labels:?} {:?} at {}", b.hint, r.min.y);
                        assert!((ends[k] - ends[0]).abs() < 1e-3 && ends[k] <= MAIN.max.x + 1e-3, "{labels:?} {:?}: words end at {}", b.hint, ends[k]);
                        assert!(r.max.x < super::super::SCREEN_CROP.x, "{labels:?} {:?} ends at {}", b.hint, r.max.x);
                        assert!(r.min.x > MAIN.min.x + LIST.max.x, "{labels:?} {:?} starts at {}", b.hint, r.min.x);
                    }
                    // The keyboard's keys and the Xbox names: the words end at the heading's
                    // right end, or as far left of it as the widest key needs to stay in its
                    // room (a bracketed "[Enter]", #4495; the rule of #1896).
                    if labels != crate::pad::Labels::PlayStation {
                        let face = c.kit.face(2);
                        let keys = list.iter().map(|b| crate::pad::hint_width(face, &b.hint.text(), c.glyphs) - crate::pad::hint_width(face, b.hint.word, c.glyphs));
                        let right = super::super::frame::hint_words_right(MAIN.max.x, MAIN.max.x + super::super::frame::HINT_KEY_ROOM, keys);
                        assert!(right > MAIN.max.x - 10.0, "{labels:?} tab {tab}: a key moves the column {} left", MAIN.max.x - right);
                        assert!(ends.iter().all(|e| (e - right).abs() < 1e-3), "{labels:?} tab {tab}: {ends:?}");
                    }
                    let boxes = button_boxes(&c, &list);
                    for w in boxes.windows(2) {
                        assert!((w[1].min.y - w[0].max.y).abs() < 1e-3, "{labels:?} tab {tab}: {boxes:?}");
                    }
                }
            }
        }
        crate::pad::set_labels(crate::pad::Labels::Keys);
    }

    /// The trace's place, shared by the hand-built panel and the one drawn from the file: at the
    /// file's 350 square exactly the hand-built panel's numbers (the flat line 5 short of the
    /// width, 8 over the middle, the trace 110 high), elsewhere scaled to the box measured.
    #[test]
    fn the_trace_lies_as_the_hand_built_panel_at_its_size_and_scales_to_a_measured_box() {
        let o = MAIN.min + WAVE.min;
        assert_eq!(WaveGeom::hand_built(), WaveGeom { left: o.x, mid: o.y + (350.0 - THICK) / 2.0 - 8.0, len: 345.0, amplitude: WAVE_AMPLITUDE });
        assert_eq!(WaveGeom::of(Vec2::ZERO, WAVE.size()), WaveGeom { left: 0.0, mid: (350.0 - THICK) / 2.0 - 8.0, len: 345.0, amplitude: WAVE_AMPLITUDE });
        let half = WaveGeom::of(Vec2::new(10.0, 20.0), WAVE.size() / 2.0);
        assert_eq!((half.left, half.len, half.amplitude), (10.0, 172.5, WAVE_AMPLITUDE / 2.0));
        assert_eq!(half.mid, 20.0 + (175.0 - THICK) / 2.0 - 4.0);
    }

    #[test]
    fn silence_draws_the_flat_line() {
        let mut scope = WaveScope::default();
        scope.step(1.0 / 60.0, 0.0);
        assert!((0..=WAVE_SEGMENTS).all(|i| scope.height(i) == 0.0));
    }

    #[test]
    fn the_envelope_runs_to_the_left_and_follows_the_sound() {
        let mut scope = WaveScope::default();
        scope.step(1.0 / 60.0, WAVE_LOUD);
        let before: Vec<f32> = scope.points.iter().map(|p| p.0).collect();
        assert!(before.iter().all(|&e| e >= WAVE_SWING.0 && e <= WAVE_SWING.1));
        scope.scroll = 0.0;
        // A lull comes in at the right while the loud part moves left.
        scope.step(0.05, 0.0);
        let moved = (0.05 * WAVE_SCROLL * WAVE_SEGMENTS as f32) as usize;
        assert_eq!(scope.points.len(), WAVE_SEGMENTS + 1);
        assert_eq!(scope.points[0].0, before[moved]);
        assert_eq!(scope.points[WAVE_SEGMENTS].0, 0.0);
        assert!((0..=WAVE_SEGMENTS).any(|i| scope.height(i).abs() > 0.2));
    }

    #[test]
    fn time_remaining_reads_minutes_seconds_tenths() {
        assert_eq!(remaining(309), "00:30:9 remaining");
        assert_eq!(remaining(0), "00:00:0 remaining");
        assert_eq!(remaining(754), "01:15:4 remaining");
    }

    #[test]
    fn the_right_stick_moves_the_cursor_and_inverts_y() {
        // Right moves right; up moves up (the window's y runs down, so up is negative).
        assert_eq!(stick_cursor(Vec2::new(300.0, 300.0), Vec2::new(1.0, 0.0), 0.1).0, Vec2::new(360.0, 300.0));
        assert_eq!(stick_cursor(Vec2::new(300.0, 300.0), Vec2::new(0.0, 1.0), 0.1).0, Vec2::new(300.0, 240.0));
        // Kept inside the window at every corner.
        assert_eq!(stick_cursor(Vec2::new(0.0, 0.0), Vec2::new(-1.0, 1.0), 1.0).0, Vec2::new(0.0, 0.0));
        assert_eq!(stick_cursor(Vec2::new(MAP.width(), 0.0), Vec2::new(1.0, 1.0), 1.0).0, Vec2::new(MAP.width(), 0.0));
        assert_eq!(stick_cursor(Vec2::new(0.0, MAP.height()), Vec2::new(-1.0, -1.0), 1.0).0, Vec2::new(0.0, MAP.height()));
        assert_eq!(stick_cursor(Vec2::new(MAP.width(), MAP.height()), Vec2::new(1.0, -1.0), 1.0).0, Vec2::new(MAP.width(), MAP.height()));
    }

    #[test]
    fn the_cursor_pushed_past_an_edge_reports_the_overflow() {
        // Pushed right past the right edge: the overflow is the distance beyond it.
        let (cursor, overflow) = stick_cursor(Vec2::new(500.0, 250.0), Vec2::new(1.0, 0.0), 1.0);
        assert_eq!(cursor.x, MAP.width());
        assert!(overflow.x > 0.0);
        assert_eq!(overflow.y, 0.0);
        // Pushed up past the top edge: the overflow is negative (toward the top).
        let (_, overflow) = stick_cursor(Vec2::new(100.0, 100.0), Vec2::new(0.0, 1.0), 1.0);
        assert!(overflow.y < 0.0);
        // Within the window there is no overflow.
        assert_eq!(stick_cursor(Vec2::new(300.0, 300.0), Vec2::new(1.0, 0.0), 0.1).1, Vec2::ZERO);
    }

    #[test]
    fn the_highlight_box_keeps_the_stick_cursor_with_no_pointer() {
        // With no pointer (the controller is in use), the highlight box stays where the
        // stick put it instead of falling back to the window's middle or the mouse.
        let mut st = DataState::default();
        let at = Vec2::new(120.0, 90.0);
        st.cursor[WORLD] = Some(at);
        let mut marks: Vec<MapMark> = Vec::new();
        let names: Vec<String> = Vec::new();
        let (cursor, _, _) = place_cursor(&mut st, WORLD, None, &mut marks, &names);
        assert_eq!(cursor, at);
        assert_eq!(st.cursor[WORLD], Some(at));
    }

    fn quests(forms: &[u32]) -> Lists {
        Lists { quests: forms.iter().map(|&form| QuestKey { seen: 1, form, done: false }).collect(), ..default() }
    }

    fn pages(n: f32) -> PaneMoves {
        PaneMoves { pages: n, ..default() }
    }

    #[test]
    fn the_objectives_scroll_by_pages_and_the_wheel_and_clamp_at_both_ends() {
        let l = quests(&[0x10, 0x20]);
        // Objectives 600 high in their 400-high window scroll 200 at most.
        let mut st = DataState { text_max: objectives_max(600.0), ..default() };
        assert_eq!(st.text_max, 200.0);
        let shows = pane_key(&st, QUESTS, &l);
        assert_eq!(shows, Some((QUESTS, false, 0x10)));
        // The frame the pane comes up it only starts at the top (its length not drawn yet).
        scroll_pane(&mut st, QUESTS, shows, pages(1.0));
        assert_eq!(st.text_first, 0.0);
        scroll_pane(&mut st, QUESTS, shows, PaneMoves { notches: -2.0, ..default() });
        assert_eq!(st.text_first, 2.0 * TEXT_STEP);
        // A page down goes past the end: held at the bottom; two up: held at the top.
        scroll_pane(&mut st, QUESTS, shows, pages(1.0));
        assert_eq!(st.text_first, 200.0);
        scroll_pane(&mut st, QUESTS, shows, pages(-2.0));
        assert_eq!(st.text_first, 0.0);
        // Objectives that fit do not scroll.
        st.text_max = objectives_max(300.0);
        scroll_pane(&mut st, QUESTS, shows, pages(1.0));
        assert_eq!(st.text_first, 0.0);
    }

    #[test]
    fn another_quest_or_note_starts_at_the_top() {
        let mut l = quests(&[0x10, 0x20]);
        let mut st = DataState { text_max: 1000.0, ..default() };
        let shows = pane_key(&st, QUESTS, &l);
        scroll_pane(&mut st, QUESTS, shows, PaneMoves::default());
        let shows = pane_key(&st, QUESTS, &l);
        scroll_pane(&mut st, QUESTS, shows, pages(1.0));
        assert_eq!(st.text_first, OBJECTIVES_H);
        st.sel[QUESTS] = 1;
        let shows = pane_key(&st, QUESTS, &l);
        assert_eq!(shows, Some((QUESTS, false, 0x20)));
        scroll_pane(&mut st, QUESTS, shows, PaneMoves::default());
        assert_eq!(st.text_first, 0.0);
        // The same quest moved in the list keeps its place.
        scroll_pane(&mut st, QUESTS, shows, pages(1.0));
        l.quests.swap(0, 1);
        st.sel[QUESTS] = 0;
        let shows = pane_key(&st, QUESTS, &l);
        scroll_pane(&mut st, QUESTS, shows, PaneMoves::default());
        assert_eq!(st.text_first, OBJECTIVES_H);
        // Misc pages by a note's window; the challenges are another pane.
        let notes = Lists { notes: vec![read::Note { form: 0x30, name: "A".into(), kind: 1, text: None, image: None, quest: None }], ..default() };
        let shows = pane_key(&st, NOTES, &notes);
        scroll_pane(&mut st, NOTES, shows, PaneMoves::default());
        assert_eq!(st.text_first, 0.0);
        let shows = pane_key(&st, NOTES, &notes);
        scroll_pane(&mut st, NOTES, shows, pages(1.0));
        assert_eq!(st.text_first, TEXT_H);
        st.challenges = true;
        let shows = pane_key(&st, NOTES, &notes);
        scroll_pane(&mut st, NOTES, shows, PaneMoves::default());
        assert_eq!(st.text_first, 0.0);
        // The other tabs have no pane to scroll.
        assert_eq!(pane_key(&st, RADIO, &notes), None);
    }

    #[test]
    fn the_right_stick_scrolls_the_pane_on_both_tabs() {
        let l = Lists { notes: vec![read::Note { form: 0x30, name: "A".into(), kind: 1, text: None, image: None, quest: None }], ..quests(&[0x10]) };
        for tab in [QUESTS, NOTES] {
            let mut st = DataState { text_max: 1000.0, ..default() };
            let shows = pane_key(&st, tab, &l);
            scroll_pane(&mut st, tab, shows, PaneMoves::default());
            // Pushed down (negative y) it scrolls on, at the speed for the push.
            scroll_pane(&mut st, tab, shows, PaneMoves { stick: -1.0, dt: 0.5, ..default() });
            assert_eq!(st.text_first, PANE_STICK_SPEED * 0.5);
            assert!(st.pane_stick);
            scroll_pane(&mut st, tab, shows, PaneMoves { stick: -0.5, dt: 0.5, ..default() });
            assert_eq!(st.text_first, PANE_STICK_SPEED * 0.75);
            // Up goes back, held at the top; let go, it stops.
            scroll_pane(&mut st, tab, shows, PaneMoves { stick: 1.0, dt: 5.0, ..default() });
            assert_eq!(st.text_first, 0.0);
            scroll_pane(&mut st, tab, shows, PaneMoves { dt: 0.5, ..default() });
            assert!(!st.pane_stick);
            // Held at the bottom too.
            scroll_pane(&mut st, tab, shows, PaneMoves { stick: -1.0, dt: 10.0, ..default() });
            assert_eq!(st.text_first, 1000.0);
        }
    }

    #[test]
    fn the_wheel_scrolls_the_objectives_only_over_their_window() {
        let rect = objectives_rect(MAIN.min + Vec2::new(DATA_X, 150.0));
        let at = |x: f32, y: f32| Some(MAIN.min + Vec2::new(x, y));
        // Over the objectives: they scroll.
        assert!(wheel_on_pane(QUESTS, at(600.0, 300.0), Some(rect)));
        // Over the buttons above them, the bottom tabs below, the heading or the list:
        // the list scrolls, as before the objectives did.
        assert!(!wheel_on_pane(QUESTS, at(800.0, 90.0), Some(rect)));
        assert!(!wheel_on_pane(QUESTS, at(600.0, 600.0), Some(rect)));
        assert!(!wheel_on_pane(QUESTS, at(700.0, 10.0), Some(rect)));
        assert!(!wheel_on_pane(QUESTS, at(100.0, 300.0), Some(rect)));
        // No objectives drawn, no pointer: the list.
        assert!(!wheel_on_pane(QUESTS, at(600.0, 300.0), None));
        assert!(!wheel_on_pane(QUESTS, None, Some(rect)));
        // Misc keeps its rule: right of the note text's left edge.
        assert!(wheel_on_pane(NOTES, at(800.0, 90.0), None));
        assert!(!wheel_on_pane(NOTES, at(100.0, 300.0), None));
        assert!(!wheel_on_pane(RADIO, at(600.0, 300.0), Some(rect)));
    }

    #[test]
    fn up_on_the_first_row_or_down_on_the_last_keeps_the_place_read() {
        let l = quests(&[0x10, 0x20]);
        let mut st = DataState { text_max: 1000.0, ..default() };
        let settle = |st: &mut DataState, moves: &[i32], m: PaneMoves| {
            step_rows(st, QUESTS, moves, 2);
            let shows = pane_key(st, QUESTS, &l);
            scroll_pane(st, QUESTS, shows, m);
        };
        settle(&mut st, &[], PaneMoves::default());
        settle(&mut st, &[], pages(1.0));
        assert_eq!(st.text_first, OBJECTIVES_H);
        // Up on the first quest: the same quest shown, the same place.
        settle(&mut st, &[-1], PaneMoves::default());
        assert_eq!((st.sel[QUESTS], st.text_first), (0, OBJECTIVES_H));
        // Down: another quest, from its top.
        settle(&mut st, &[1], PaneMoves::default());
        assert_eq!((st.sel[QUESTS], st.text_first), (1, 0.0));
        settle(&mut st, &[], pages(1.0));
        // Down on the last: kept.
        settle(&mut st, &[1], PaneMoves::default());
        assert_eq!((st.sel[QUESTS], st.text_first), (1, OBJECTIVES_H));
    }

    #[test]
    fn the_scroll_bar_steps_pages_and_drags() {
        // A bar 400 high: arrows of 16, the marker's run 360; half way, a quarter shown.
        let spots = bar_spots(368.0, 400.0, 0.5, 0.25, 4.0);
        let part_at = |y: f32| spots.iter().find(|(r, _)| r.contains(Vec2::new(380.0, y))).map(|s| s.1);
        assert_eq!(part_at(8.0), Some(BarPart::Up));
        assert_eq!(part_at(100.0), Some(BarPart::PageUp));
        assert_eq!(part_at(250.0), Some(BarPart::Marker(4.0)));
        assert_eq!(part_at(350.0), Some(BarPart::PageDown));
        assert_eq!(part_at(392.0), Some(BarPart::Down));
        assert_eq!(bar_marker(400.0, 0.5, 0.25), (200.0, 90.0));
        // At the top the marker starts 4 under the arrow.
        let top = bar_spots(368.0, 400.0, 0.0, 0.25, 4.0);
        assert!(top.iter().any(|(r, b)| *b == BarPart::Marker(4.0) && r.min.y == ARROW_H + 4.0));
        // A note's arrows stand at the right of its window.
        let o = Vec2::new(500.0, 200.0);
        assert_eq!(note_arrows(o)[0], (Rect::new(o.x + DATA_W, o.y, o.x + DATA_W + SCROLLBAR, o.y + ARROW_H), BarPart::Up));
        assert_eq!(note_arrows(o)[1].0.max.y, o.y + TEXT_H);

        let l = quests(&[0x10]);
        let mut st = DataState { text_max: 1000.0, ..default() };
        let shows = pane_key(&st, QUESTS, &l);
        scroll_pane(&mut st, QUESTS, shows, PaneMoves::default());
        let click = |st: &mut DataState, b: BarPart, y: f32| {
            let mut m = PaneMoves::default();
            bar_act(st, b, Some(Vec2::new(380.0, y)), &mut m);
            scroll_pane(st, QUESTS, shows, m);
        };
        // The track below the marker pages down, an arrow steps a wheel notch.
        click(&mut st, BarPart::PageDown, 350.0);
        assert_eq!(st.text_first, OBJECTIVES_H);
        click(&mut st, BarPart::Up, 8.0);
        assert_eq!(st.text_first, OBJECTIVES_H - TEXT_STEP);
        click(&mut st, BarPart::Down, 392.0);
        click(&mut st, BarPart::PageUp, 100.0);
        assert_eq!(st.text_first, 0.0);
        // The marker pressed follows the pointer while the button is held, 4 units a unit.
        click(&mut st, BarPart::Marker(4.0), 100.0);
        assert_eq!(st.bar_drag, Some((100.0, 4.0)));
        let drag = |st: &mut DataState, held: bool, y: f32| {
            let mut m = PaneMoves::default();
            drag_bar(st, held, Some(Vec2::new(380.0, y)), &mut m);
            scroll_pane(st, QUESTS, shows, m);
        };
        drag(&mut st, true, 110.0);
        assert_eq!(st.text_first, 40.0);
        drag(&mut st, true, 105.0);
        assert_eq!(st.text_first, 20.0);
        // Let go, it stays.
        drag(&mut st, false, 200.0);
        assert_eq!((st.bar_drag, st.text_first), (None, 20.0));
        drag(&mut st, true, 300.0);
        assert_eq!(st.text_first, 20.0);
    }

    #[test]
    fn the_left_stick_zooms_in_up_and_out_down() {
        assert!(stick_zoom(1.0, 1.0, 1.0) > 1.0);
        assert!(stick_zoom(4.0, -1.0, 1.0) < 4.0);
        assert_eq!(stick_zoom(1.0, 0.0, 1.0), 1.0);
        assert_eq!(stick_zoom(4.0, 1.0, 100.0), ZOOM.1);
        assert_eq!(stick_zoom(1.0, -1.0, 100.0), ZOOM.0);
    }

    /// The keys move the question box's choice although the pointer rests on an answer:
    /// the answer it is on, entered again as the box is drawn anew, takes the choice back
    /// only once the pointer moves (#1954: the smoke run's Down was undone at once).
    fn three() -> Popup {
        Popup::marker(marker::MarkerAsk::Change(CustomMarker { space: 1, pos: [0.0; 3] }), "?".into(), vec!["a".into(), "b".into(), "c".into()])
    }

    fn moves(steps: &[i32], pointer: &[(usize, bool)]) -> BoxInput {
        BoxInput { steps: steps.to_vec(), pointer: pointer.to_vec(), ..BoxInput::default() }
    }

    #[test]
    fn the_keys_choice_holds_under_a_resting_pointer() {
        let mut p = three();
        let mut hold = BoxHold::default();
        let rest = Some(Vec2::new(527.0, 350.0));
        // The pointer comes onto the first answer: it is chosen.
        choose(&mut p, &mut hold, rest, &moves(&[], &[(0, true)]));
        assert_eq!((p.selected, hold.keyed, hold.hover), (0, None, Some(0)));
        // Down twice; the redrawn box's first answer is entered again under the pointer.
        choose(&mut p, &mut hold, rest, &moves(&[1], &[]));
        choose(&mut p, &mut hold, rest, &moves(&[], &[(0, true)]));
        choose(&mut p, &mut hold, rest, &moves(&[1], &[(0, true)]));
        assert_eq!(p.selected, 2);
        // Within the slack the pointer still rests.
        choose(&mut p, &mut hold, Some(Vec2::new(528.0, 351.0)), &moves(&[], &[(0, true)]));
        assert_eq!(p.selected, 2);
        // Moved, the answer it comes onto takes the choice.
        choose(&mut p, &mut hold, Some(Vec2::new(527.0, 380.0)), &moves(&[], &[(0, false), (1, true)]));
        assert_eq!((p.selected, hold.keyed), (1, None));
        // With no pointer at all (a controller, the aid without `point=`) the keys move it.
        let mut hold = BoxHold::default();
        choose(&mut p, &mut hold, None, &moves(&[-1], &[]));
        assert_eq!((p.selected, hold.keyed), (0, Some(None)));
    }

    /// The review's case: the pointer moves on within the answer whose PointerOver came
    /// while a key held the choice (or entered within the slack): no fresh PointerOver
    /// comes, yet once it has moved that answer takes the choice.
    #[test]
    fn the_answer_under_a_moving_pointer_takes_the_choice_without_a_fresh_hover() {
        let mut p = three();
        let mut hold = BoxHold::default();
        let at = Vec2::new(500.0, 350.0);
        choose(&mut p, &mut hold, Some(at), &moves(&[1, 1], &[]));
        assert_eq!(p.selected, 2);
        // Onto the second answer within the slack: held.
        choose(&mut p, &mut hold, Some(at + Vec2::new(1.0, 1.0)), &moves(&[], &[(1, true)]));
        assert_eq!(p.selected, 2);
        // On within it, no event: it takes the choice.
        choose(&mut p, &mut hold, Some(at + Vec2::new(20.0, 1.0)), &moves(&[], &[]));
        assert_eq!(p.selected, 1);
        // Off every answer: the choice stays where it was.
        choose(&mut p, &mut hold, Some(at + Vec2::new(300.0, 1.0)), &moves(&[], &[(1, false)]));
        assert_eq!((p.selected, hold.hover), (1, None));
    }

    /// The review's sequence: the pointer on the first answer, Down (the box is drawn
    /// anew, the answer the pointer was on despawned with no PointerOut), the pointer
    /// moved onto blank space before the new answers were picked: the choice stays the
    /// keys' one.
    #[test]
    fn a_hover_does_not_outlive_its_redrawn_answer() {
        let mut p = three();
        let mut hold = BoxHold::default();
        let at = Vec2::new(527.0, 350.0);
        choose(&mut p, &mut hold, Some(at), &moves(&[], &[(0, true)]));
        assert_eq!(p.selected, 0);
        choose(&mut p, &mut hold, Some(at), &moves(&[1], &[]));
        assert_eq!(p.selected, 1);
        hold.rebuilt();
        choose(&mut p, &mut hold, Some(at + Vec2::new(0.0, 200.0)), &moves(&[], &[]));
        assert_eq!((p.selected, hold.keyed, hold.hover), (1, None, None));
        // A pointer still on an answer after a redraw enters the new one and has it again.
        hold.rebuilt();
        choose(&mut p, &mut hold, Some(at + Vec2::new(0.0, 230.0)), &moves(&[], &[(2, true)]));
        assert_eq!(p.selected, 2);
    }

    /// Down and Enter in one frame take the answer below (fast travel's No, not Yes);
    /// Y, N or a click take theirs whatever the choice.
    #[test]
    fn the_confirm_takes_the_choice_after_the_frames_moves() {
        let mut p = Popup::travel(7, "Travel?".into());
        let mut hold = BoxHold::default();
        let b = BoxInput { steps: vec![1], confirm: true, ..BoxInput::default() };
        assert_eq!(box_frame(&mut p, &mut hold, None, &b), Some(1));
        assert_eq!(p.take(1), None);
        let mut p = three();
        let b = BoxInput { steps: vec![1], confirm: true, answer: Some(0), ..BoxInput::default() };
        assert_eq!(box_frame(&mut p, &mut BoxHold::default(), None, &b), Some(0));
        let b = BoxInput { answer: Some(9), ..BoxInput::default() };
        assert_eq!(box_frame(&mut p, &mut BoxHold::default(), None, &b), None);
    }

    #[test]
    fn the_waypoints_are_worked_out_again_only_when_they_move() {
        let mut cache = None;
        let a = cached_waypoints(&mut cache, Some(Vec2::ZERO), Some(Vec2::new(100.0, 0.0)), &[], 1.0);
        assert_eq!(a.1.len(), 1);
        // A stale cache entry stands in for the same inputs.
        if let Some((_, w)) = cache.as_mut() {
            w.0 = Arc::default();
        }
        assert!(cached_waypoints(&mut cache, Some(Vec2::ZERO), Some(Vec2::new(100.0, 0.0)), &[], 1.0).0.is_empty());
        // Unmoved, the frame shares the cached vectors.
        let b = cached_waypoints(&mut cache, Some(Vec2::ZERO), Some(Vec2::new(100.0, 0.0)), &[], 1.0);
        assert!(Arc::ptr_eq(&b.1, &cache.as_ref().unwrap().1 .1));
        // Zoomed, or the marker moved: worked out again.
        assert_eq!(cached_waypoints(&mut cache, Some(Vec2::ZERO), Some(Vec2::new(100.0, 0.0)), &[], 2.0).0.len(), 2);
        assert_eq!(cached_waypoints(&mut cache, Some(Vec2::ZERO), None, &[Vec2::ONE * 50.0], 2.0).1[0].0, LineTo::Quest);
    }

    fn near(a: Vec2, b: Vec2) -> bool {
        a.distance(b) < 0.01
    }

    /// The question Place Marker puts up, and its answers.
    fn asked(st: &DataState) -> (marker::MarkerAsk, usize) {
        match st.popup.as_ref().map(|p| (&p.ask, p.buttons.len())) {
            Some((Ask::Marker(a), n)) => (*a, n),
            other => panic!("no marker question: {other:?}"),
        }
    }

    /// #1954: one custom marker, placed at the World Map's highlight box, shown on the Local
    /// Map at the matching place, moved from the Local Map, kept, removed; the World Map
    /// draws a dotted line from the arrow to it, the Local Map none.
    #[test]
    fn one_custom_marker_is_shared_by_both_maps() {
        use bevy::ecs::system::SystemState;
        let map = WorldMap { root: 1, transform: model::MapTransform::default(), texture: "root.dds".into(), usable: [2048.0; 2], nw: [-2, 3], se: [1, 2] };
        let mut st = DataState { maps: [(1, Some(map))].into_iter().collect(), markers: Some(Vec::new()), ..DataState::default() };
        let mut world = World::new();
        world.insert_resource(Location { world: 1, interior: None });
        // The player at the picture's middle ([0.5, 0.5]).
        let feet = [0.0, 3.0 * model::CELL, 100.0];
        world.insert_resource(Player { heading: 0.0, pitch: 0.0, eye_height: 0.0, feet, body_eye: None, body_view: None, placed: None });
        let mut state = SystemState::<Game>::new(&mut world);
        let mut game = state.get_mut(&mut world).unwrap();
        // The World Map opens on the player; the highlight box 50 right of the middle.
        let off = Vec2::new(50.0, 0.0);
        st.cursor[WORLD] = Some(MAP_MID + off);
        let shown = world_map(&mut st, &game, None, false);
        assert_eq!((shown.custom, shown.nodes.len(), shown.lines.len()), (None, 0, 0));
        place_marker(&mut st, WORLD, &mut game, false);
        let (ask, answers) = asked(&st);
        assert!(matches!(ask, marker::MarkerAsk::Set(_)));
        assert_eq!(answers, 2);
        st.popup = None;
        answer_marker(&mut st, &mut game, marker::answer(ask, 0));
        let placed = st.custom.expect("placed");
        // 50 of the 2048 picture: 0.797 of it spans the 4 cells across.
        let x = 50.0 / 2048.0 / model::MAP_SPAN * 4.0 * model::CELL;
        assert!((placed.pos[0] - x).abs() < 0.5 && (placed.pos[1] - feet[1]).abs() < 0.5, "{placed:?}");
        assert_eq!((placed.space, placed.pos[2]), (1, 100.0));
        let shown = world_map(&mut st, &game, None, false);
        assert!(near(shown.custom.unwrap(), quest_corner(MAP_MID + off)), "{:?}", shown.custom);
        // A line from the arrow to the marker (50 less 4 long), a node at each end.
        assert_eq!(shown.nodes.len(), 2);
        assert_eq!(shown.lines.len(), 1);
        assert_eq!(shown.lines[0].0, LineTo::Marker);
        let long: f32 = shown.lines[0].1.iter().map(|p| p.len).sum();
        assert!((long - 46.0).abs() < 0.01, "{long}");
        // The Local Map shows the same marker x / 28 right of its middle, and no line.
        let local = local_map(&mut st, &game, None);
        assert!(near(local.custom.unwrap(), local_quest_corner(MAP_MID + Vec2::new(x / LOCAL_SCALE, 0.0))), "{:?}", local.custom);
        assert!(local.lines.is_empty() && local.nodes.is_empty());
        // Moved from the Local Map: 100 left of its middle is 2800 west of the player.
        st.cursor[LOCAL] = Some(MAP_MID - Vec2::new(100.0, 0.0));
        local_map(&mut st, &game, None);
        place_marker(&mut st, LOCAL, &mut game, false);
        let (ask, answers) = asked(&st);
        assert_eq!(answers, 3);
        answer_marker(&mut st, &mut game, marker::answer(ask, 0));
        let moved = st.custom.unwrap();
        assert!((moved.pos[0] + 2800.0).abs() < 0.01 && (moved.pos[1] - feet[1]).abs() < 0.01, "{moved:?}");
        // The World Map shows it there.
        let shown = world_map(&mut st, &game, None, false);
        let left = -2800.0 / (4.0 * model::CELL) * model::MAP_SPAN * 2048.0;
        assert!(near(shown.custom.unwrap(), quest_corner(MAP_MID + Vec2::new(left, 0.0))), "{:?}", shown.custom);
        // Keep leaves it; remove takes it away from both maps.
        place_marker(&mut st, WORLD, &mut game, false);
        let (ask, _) = asked(&st);
        answer_marker(&mut st, &mut game, marker::answer(ask, 2));
        assert_eq!(st.custom, Some(moved));
        answer_marker(&mut st, &mut game, marker::answer(ask, 1));
        assert_eq!(st.custom, None);
        assert_eq!(world_map(&mut st, &game, None, false).custom, None);
        assert_eq!(local_map(&mut st, &game, None).custom, None);
    }

    /// A marker indoors shows on that interior's Local Map only, never on the World Map.
    #[test]
    fn a_marker_indoors_stays_in_its_cell() {
        use bevy::ecs::system::SystemState;
        let mut st = DataState { markers: Some(Vec::new()), custom: Some(CustomMarker { space: 0x77, pos: [0.0; 3] }), ..DataState::default() };
        st.maps.insert(0x77, None);
        let mut world = World::new();
        world.insert_resource(Location { world: 1, interior: Some(0x77) });
        world.insert_resource(Player { heading: 0.0, pitch: 0.0, eye_height: 0.0, feet: [0.0; 3], body_eye: None, body_view: None, placed: None });
        let mut state = SystemState::<Game>::new(&mut world);
        let game = state.get_mut(&mut world).unwrap();
        assert!(local_map(&mut st, &game, None).custom.is_some());
        assert_eq!(st.spot[LOCAL].map(|s| s.space), Some(0x77));
        world.resource_mut::<Location>().interior = Some(0x78);
        let game = state.get_mut(&mut world).unwrap();
        assert!(local_map(&mut st, &game, None).custom.is_none());
    }

    #[test]
    fn the_dotted_lines_lead_to_the_marker_and_the_quest_targets() {
        assert_eq!(waypoints(None, Some(Vec2::ONE), &[], 1.0), (Vec::new(), Vec::new()));
        assert_eq!(waypoints(Some(Vec2::ZERO), None, &[], 1.0), (Vec::new(), Vec::new()));
        let (nodes, lines) = waypoints(Some(Vec2::ZERO), Some(Vec2::new(100.0, 0.0)), &[Vec2::new(0.0, 60.0)], 1.0);
        assert_eq!(nodes, [Vec2::ZERO, Vec2::new(100.0, 0.0), Vec2::new(0.0, 60.0)]);
        assert_eq!(lines.iter().map(|l| l.0).collect::<Vec<_>>(), [LineTo::Marker, LineTo::Quest]);
        assert!(lines.iter().all(|l| !l.1.is_empty()));
    }

    #[test]
    fn the_save_keeps_the_custom_marker() {
        let m = CustomMarker { space: 0x000D_A726, pos: [-70000.0, 2560.0, 8700.0] };
        let mut w = World::new();
        w.insert_resource(DataState { custom: Some(m), ..DataState::default() });
        let v = (CUSTOM_MARKER_SAVE_HOOK.save)(&w).expect("saved");
        assert_eq!(marker_from_save(&v, None), Some(m));
        // None placed: nothing saved; a part that does not read is none.
        w.resource_mut::<DataState>().custom = None;
        assert!((CUSTOM_MARKER_SAVE_HOOK.save)(&w).is_none());
        assert_eq!(marker_from_save(&serde_json::Value::Null, None), None);
        // Another load order: a plugin moved takes its marker along, one gone drops it.
        let entry = |name: &str| fnv_save::PluginEntry { name: name.into(), size: 0 };
        let saved = [entry("FalloutNV.esm"), entry("Mod.esp")];
        let moved = fnv_save::PluginRemap::from_names(&saved, &["FalloutNV.esm".into(), "Other.esp".into(), "Mod.esp".into()]);
        let in_mod = CustomMarker { space: 0x0100_0123, ..m };
        let v = serde_json::to_value(in_mod).unwrap();
        assert_eq!(marker_from_save(&v, Some(&moved)).map(|c| c.space), Some(0x0200_0123));
        let gone = fnv_save::PluginRemap::from_names(&saved, &["FalloutNV.esm".into()]);
        assert_eq!(marker_from_save(&v, Some(&gone)), None);
        assert_eq!(marker_from_save(&serde_json::to_value(m).unwrap(), Some(&gone)), Some(m));
    }

    /// The host's quest log on a guest: every named quest of the host's story with an objective shown or done,
    /// running and done alike, in the host's order (running first, newest first); the
    /// host's active quest even before the story carries it; a quest that ended a moment
    /// ago done at once; unnamed quests and quests with no objective shown left out; the
    /// player's own hide-completed tweak drops the done ones.
    #[test]
    fn a_guests_list_is_the_hosts_whole_quest_log() {
        use fnv_game::coop::{HostQuest, HostQuestState, HostStory};
        let state = |seen: u64, ended: bool, objectives: &[(i32, bool)]| HostQuestState { stage: 10, running: !ended, ended, done: vec![10], failed: false, objectives: objectives.iter().copied().collect(), seen };
        let mut story = HostStory::default();
        story.quests.insert(FormId(0x100), state(1, true, &[(10, true)])); // done, oldest
        story.quests.insert(FormId(0x200), state(2, false, &[(10, true), (20, false)])); // running
        story.quests.insert(FormId(0x300), state(3, false, &[(10, false)])); // running, newest
        story.quests.insert(FormId(0x400), state(4, false, &[])); // started, nothing shown yet
        story.quests.insert(FormId(0x500), state(5, false, &[(10, false)])); // no name (a quest the list never shows)
        story.quests.insert(FormId(0x600), state(6, false, &[(10, false)])); // ends a moment ago
        let named = |q: FormId| q.0 != 0x500;
        // The host's active quest, which the story has not carried yet.
        let host = HostQuest { quest: FormId(0x700), objectives: [(10, false)].into(), completed: false, failed: false };
        let ended = [(FormId(0x600), false)];
        let rows = |hide| host_quest_log(&story, Some(&host), &ended, named, hide).iter().map(|k| (k.form, k.done)).collect::<Vec<_>>();
        assert_eq!(rows(false), [(0x700, false), (0x300, false), (0x200, false), (0x600, true), (0x100, true)]);
        assert_eq!(rows(true), [(0x700, false), (0x300, false), (0x200, false)]);
        // Nothing heard yet: an empty list, not the guest's own quests.
        assert!(host_quest_log(&HostStory::default(), None, &[], |_| true, false).is_empty());
    }

    /// The host's log is read only on a guest: the make-active hint is dim there, as on a
    /// quest done, and bright on the host's own running quest.
    #[test]
    fn a_guests_make_active_hint_is_dim() {
        let make_active = |f: &HintFacts| tab_buttons(QUESTS, f).into_iter().find(|b| matches!(b.kind, ButtonKind::MakeActive)).map(|b| b.hint.dim);
        assert_eq!(make_active(&facts(true)), Some(false));
        assert_eq!(make_active(&HintFacts { quests_read_only: true, ..facts(true) }), Some(true));
    }

    #[test]
    fn projected_mobile_wheel_reaches_native_data_list_channel_and_stale_tab_is_refused(){
        use bevy::ecs::system::RunSystemOnce;
        fn read(mut st:ResMut<DataState>,mut game:Game,mut input:Input){
            take_input(&mut st,NOTES,&Lists::default(),&mut game,&mut input,None,true);
        }
        let mut world=page_world(true,data_open(NOTES));
        let mut wheel=super::super::touch_scroll::WheelFrame::default();
        wheel.set(world.resource::<PipBoy>(),Vec2::new(100.,100.),-60.);world.insert_resource(wheel);
        world.run_system_once(read).unwrap();assert_eq!(world.resource_mut::<DataState>().take_list_notches(),-3.0);
        world.resource_mut::<PipBoy>().tabs[Page::Data.index()]=QUESTS;
        world.run_system_once(read).unwrap();assert_eq!(world.resource_mut::<DataState>().take_list_notches(),0.0);
        assert!(world.resource::<Messages<FastTravelAsk>>().is_empty());
    }

    #[test]
    fn script_map_marker_overrides_reach_world_local_discovery_compass_and_hud() {
        use bevy::ecs::system::SystemState;
        use model::MapTransform;

        fn sub(sig: &[u8; 4], data: &[u8]) -> Vec<u8> {
            let mut out = Vec::with_capacity(6 + data.len());
            out.extend_from_slice(sig);
            out.extend_from_slice(&(data.len() as u16).to_le_bytes());
            out.extend_from_slice(data);
            out
        }
        fn rec(sig: &[u8; 4], id: u32, subs: &[Vec<u8>]) -> Vec<u8> {
            let body = subs.concat();
            let mut out = Vec::with_capacity(24 + body.len());
            out.extend_from_slice(sig);
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&id.to_le_bytes());
            out.extend_from_slice(&[0u8; 8]);
            out.extend_from_slice(&body);
            out
        }
        fn grp(label: [u8; 4], kind: u32, body: &[u8]) -> Vec<u8> {
            let mut out = Vec::with_capacity(24 + body.len());
            out.extend_from_slice(b"GRUP");
            out.extend_from_slice(&((24 + body.len()) as u32).to_le_bytes());
            out.extend_from_slice(&label);
            out.extend_from_slice(&kind.to_le_bytes());
            out.extend_from_slice(&[0u8; 8]);
            out.extend_from_slice(body);
            out
        }
        fn top(sig: &[u8; 4], body: &[u8]) -> Vec<u8> {
            grp(*sig, 0, body)
        }

        const STAT: u32 = 0x800;
        const REPU_A: u32 = 0x810;
        const REPU_B: u32 = 0x811;
        const ROOM: u32 = 0x900;
        const MARKER_A: u32 = 0x910;
        const MARKER_B: u32 = 0x911;

        let bytes = [
            rec(b"TES4", 0, &[sub(b"HEDR", &[0, 0, 0x80, 0x3F, 0, 0, 0, 0, 0, 0, 0, 0])]),
            top(b"STAT", &rec(b"STAT", STAT, &[sub(b"EDID", b"OwnedMarkerStat\0")])),
            top(
                b"REPU",
                &[
                    rec(b"REPU", REPU_A, &[sub(b"EDID", b"OwnedRepA\0"), sub(b"FULL", b"Ridge Runners\0")]),
                    rec(b"REPU", REPU_B, &[sub(b"EDID", b"OwnedRepB\0"), sub(b"FULL", b"Canyon Guild\0")]),
                ]
                .concat(),
            ),
            top(b"NPC_", &rec(b"NPC_", 0x7, &[sub(b"EDID", b"Player\0")])),
            top(
                b"CELL",
                &grp(
                    0i32.to_le_bytes(),
                    2,
                    &grp(
                        0i32.to_le_bytes(),
                        3,
                        &[
                            rec(b"CELL", ROOM, &[sub(b"EDID", b"OwnedRoom\0"), sub(b"DATA", &[1])]),
                            grp(
                                ROOM.to_le_bytes(),
                                6,
                                &grp(
                                    ROOM.to_le_bytes(),
                                    9,
                                    &[
                                        // Marker A: Visible (1), not travelable yet, type 2 (Settlement), rep A.
                                        rec(
                                            b"REFR",
                                            MARKER_A,
                                            &[
                                                sub(b"EDID", b"OwnedMarkerA\0"),
                                                sub(b"NAME", &STAT.to_le_bytes()),
                                                sub(b"XMRK", &[]),
                                                sub(b"FNAM", &[1]),
                                                sub(b"FULL", b"Alpha Camp\0"),
                                                sub(b"TNAM", &[2, 0]),
                                                sub(b"WMI1", &REPU_A.to_le_bytes()),
                                                sub(b"DATA", &[0; 24]),
                                            ],
                                        ),
                                        // Marker B: hidden/unmapped (flags 0), type 5 (Cave).
                                        rec(
                                            b"REFR",
                                            MARKER_B,
                                            &[
                                                sub(b"EDID", b"OwnedMarkerB\0"),
                                                sub(b"NAME", &STAT.to_le_bytes()),
                                                sub(b"XMRK", &[]),
                                                sub(b"FNAM", &[0]),
                                                sub(b"FULL", b"Beta Cave\0"),
                                                sub(b"TNAM", &[5, 0]),
                                                sub(b"DATA", &[0; 24]),
                                            ],
                                        ),
                                    ]
                                    .concat(),
                                ),
                            ),
                        ]
                        .concat(),
                    ),
                ),
            ),
        ]
        .concat();
        let plugin = fnv_esm::Plugin::parse(&bytes, "OwnedMarkers.esm".into()).unwrap();
        let mut script = crate::script::ScriptState::over_plugins(&[plugin]);

        let run = |script: &mut crate::script::ScriptState, body: &str| {
            script.rt.as_mut().unwrap().game.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), Default::default()).unwrap();
        };

        let root = WorldMap { root: 1, transform: MapTransform::default(), texture: "root.dds".into(), usable: [2048.0; 2], nw: [-2, 1], se: [1, -2] };
        let mut st = DataState {
            tile_rows: true,
            maps: [(1, Some(root))].into_iter().collect(),
            markers: Some(vec![
                read::Marker { form: MARKER_A, world: 1, pos: [0.0; 3], name: "Alpha Camp".into(), kind: 2, flags: 1, radius: 10.0, reputation: Some((REPU_A, "Ridge Runners".into())) },
                read::Marker { form: MARKER_B, world: 1, pos: [4096.0, 0.0, 0.0], name: "Beta Cave".into(), kind: 5, flags: 0, radius: 10.0, reputation: None },
            ]),
            ..DataState::default()
        };

        // Undiscovered custom icon stays icon_map_undiscovered.dds until discovered/travelable;
        // type, name, reputation, visibility, and hidden flags all take effect immediately.
        run(
            &mut script,
            r#"
            SetCustomMapMarkerIcon OwnedMarkerA "Interface\Icons\Owned\custom_alpha.dds"
            OwnedMarkerA.SetMapMarkerType 14
            OwnedMarkerA.SetMapMarkerName "Renamed Outpost"
            OwnedMarkerA.SetMapMarkerRep OwnedRepB
            "#,
        );

        let mut world = World::new();
        world.insert_resource(Location { world: 1, interior: None });
        world.insert_resource(Player { heading: 0.0, pitch: 0.0, eye_height: 0.0, feet: [0.0; 3], body_eye: None, body_view: None, placed: None });
        world.insert_resource(script);
        let mut state = SystemState::<Game>::new(&mut world);

        {
            let game = state.get_mut(&mut world).unwrap();
            let g = &game.script.as_ref().unwrap().rt.as_ref().unwrap().game;
            assert_eq!(st.unmapped_of(1, Some(g)), [MARKER_B].into_iter().collect());
            assert_eq!(st.named_markers_of(1, Some(g))[0].2, "Renamed Outpost");
            st.newly_found.push(MARKER_A);
            assert_eq!(st.take_newly_found(Some(g)), vec![(MARKER_A, "Renamed Outpost".to_string())]);

            let shown = world_map(&mut st, &game, None, false);
            assert_eq!(shown.marks.len(), 1, "Marker B starts unmapped");
            assert_eq!(shown.marks[0].icon, Some((14, false)));
            assert_eq!(shown.marks[0].picture(), model::marker_icon(14, false), "undiscovered marker keeps undiscovered icon");
            assert_eq!(shown.marks[0].name, "Renamed Outpost");
            assert_eq!(shown.label.as_deref(), Some("Renamed Outpost"));
            assert_eq!(shown.detail.as_deref(), Some("Canyon Guild: Neutral"));
            assert_eq!(shown.marks[0].detail.as_deref(), Some("Canyon Guild: Neutral"));

            let local = local_map(&mut st, &game, None);
            assert_eq!(local.marks.len(), 1);
            assert_eq!(local.marks[0].icon, Some((14, false)));
            assert_eq!(local.label.as_deref(), Some("Renamed Outpost"));
        }

        // Making Marker A travelable (discovered) activates its custom icon on both World and Local maps;
        // making Marker B visible brings it onto the map and removes it from unmapped_of.
        run(
            world.resource_mut::<crate::script::ScriptState>().as_mut(),
            r#"
            OwnedMarkerA.SetMapMarkerTravel 1
            OwnedMarkerB.SetMapMarkerVisible 1
            "#,
        );
        {
            let game = state.get_mut(&mut world).unwrap();
            let g = &game.script.as_ref().unwrap().rt.as_ref().unwrap().game;
            assert!(st.unmapped_of(1, Some(g)).is_empty());

            let shown = world_map(&mut st, &game, None, false);
            assert_eq!(shown.marks.len(), 2);
            assert_eq!(shown.marks[0].icon, Some((14, true)));
            assert_eq!(shown.marks[0].picture(), "Interface\\Icons\\Owned\\custom_alpha.dds");
            assert_eq!(shown.marks[1].icon, Some((5, false)));

            let local = local_map(&mut st, &game, None);
            assert_eq!(local.marks[0].picture(), "Interface\\Icons\\Owned\\custom_alpha.dds");
        }

        // Clearing custom icon with "" restores SetMapMarkerType's icon (Vault); clearing rep with 0
        // hides the reputation line; SetMapMarkerVisible 0 + SetMapMarkerHidden 1 hides Marker B
        // even under ShowAllMapMarkers 1 1 1 (skipHidden = 1).
        run(
            world.resource_mut::<crate::script::ScriptState>().as_mut(),
            r#"
            SetCustomMapMarkerIcon OwnedMarkerA ""
            OwnedMarkerA.SetMapMarkerRep 0
            OwnedMarkerB.SetMapMarkerVisible 0
            OwnedMarkerB.SetMapMarkerHidden 1
            ShowAllMapMarkers 1 1 1
            "#,
        );
        {
            let game = state.get_mut(&mut world).unwrap();
            let g = &game.script.as_ref().unwrap().rt.as_ref().unwrap().game;
            assert_eq!(st.unmapped_of(1, Some(g)), [MARKER_B].into_iter().collect());

            let shown = world_map(&mut st, &game, None, false);
            assert_eq!(shown.marks.len(), 1, "Hidden Marker B is skipped by ShowAllMapMarkers(skipHidden=1)");
            assert_eq!(shown.marks[0].picture(), model::marker_icon(14, true));
            assert_eq!(shown.detail, None, "SetMapMarkerRep 0 clears reputation line");
            assert_eq!(shown.marks[0].detail, None);
        }
    }

    #[test]
    fn authored_can_travel_to_does_not_discover_or_unmap_before_player_finds_marker() {
        use bevy::ecs::system::SystemState;
        use model::MapTransform;

        const STAT: u32 = 0x800;
        const ROOM: u32 = 0x900;
        const MARKER_VIS_TRAVEL: u32 = 0x910;
        const MARKER_TRAVEL_ONLY: u32 = 0x911;
        const MARKER_VIS_ONLY: u32 = 0x912;

        let sub = |sig: &[u8; 4], data: &[u8]| [sig.as_slice(), &(data.len() as u16).to_le_bytes(), data].concat();
        let rec = |sig: &[u8; 4], id: u32, subs: &[Vec<u8>]| {
            let body: Vec<u8> = subs.iter().flat_map(|s| s.iter().copied()).collect();
            [sig.as_slice(), &(body.len() as u32).to_le_bytes(), &0u32.to_le_bytes(), &id.to_le_bytes(), &0u32.to_le_bytes(), &0u32.to_le_bytes(), &body].concat()
        };
        let grp = |label: [u8; 4], kind: u32, body: &[u8]| [b"GRUP".as_slice(), &((24 + body.len()) as u32).to_le_bytes(), &label, &kind.to_le_bytes(), &0u64.to_le_bytes(), body].concat();
        let top = |sig: &[u8; 4], body: &[u8]| grp(*sig, 0, body);
        let tes4 = rec(b"TES4", 0, &[sub(b"HEDR", &[1.34f32.to_le_bytes().as_slice(), &0u32.to_le_bytes(), &0x800u32.to_le_bytes()].concat())]);
        let bytes = [
            tes4,
            top(b"STAT", &rec(b"STAT", STAT, &[sub(b"EDID", b"VanillaMarkerStat\0")])),
            top(b"NPC_", &rec(b"NPC_", 0x7, &[sub(b"EDID", b"Player\0")])),
            top(
                b"CELL",
                &grp(
                    0i32.to_le_bytes(),
                    2,
                    &grp(
                        0i32.to_le_bytes(),
                        3,
                        &[
                            rec(b"CELL", ROOM, &[sub(b"EDID", b"VanillaRoom\0"), sub(b"DATA", &[1])]),
                            grp(
                                ROOM.to_le_bytes(),
                                6,
                                &grp(
                                    ROOM.to_le_bytes(),
                                    9,
                                    &[
                                        // Undiscovered marker authored Visible | Can Travel To (FNAM = 1 | 2).
                                        rec(
                                            b"REFR",
                                            MARKER_VIS_TRAVEL,
                                            &[
                                                sub(b"EDID", b"VanillaVisTravelRef\0"),
                                                sub(b"NAME", &STAT.to_le_bytes()),
                                                sub(b"XMRK", &[]),
                                                sub(b"FNAM", &[1 | 2]),
                                                sub(b"FULL", b"Visible Ridge\0"),
                                                sub(b"TNAM", &[5, 0]),
                                                sub(b"DATA", &[0; 24]),
                                            ],
                                        ),
                                        // Undiscovered marker authored Can Travel To only (FNAM = 2), as typical vanilla locations.
                                        rec(
                                            b"REFR",
                                            MARKER_TRAVEL_ONLY,
                                            &[
                                                sub(b"EDID", b"VanillaTravelOnlyRef\0"),
                                                sub(b"NAME", &STAT.to_le_bytes()),
                                                sub(b"XMRK", &[]),
                                                sub(b"FNAM", &[2]),
                                                sub(b"FULL", b"Hidden Valley Camp\0"),
                                                sub(b"TNAM", &[2, 0]),
                                                sub(b"DATA", &[0; 24]),
                                            ],
                                        ),
                                        // Undiscovered marker authored Visible only (FNAM = 1).
                                        rec(
                                            b"REFR",
                                            MARKER_VIS_ONLY,
                                            &[
                                                sub(b"EDID", b"VanillaVisOnlyRef\0"),
                                                sub(b"NAME", &STAT.to_le_bytes()),
                                                sub(b"XMRK", &[]),
                                                sub(b"FNAM", &[1]),
                                                sub(b"FULL", b"Lookout Ridge\0"),
                                                sub(b"TNAM", &[3, 0]),
                                                sub(b"DATA", &[0; 24]),
                                            ],
                                        ),
                                    ]
                                    .concat(),
                                ),
                            ),
                        ]
                        .concat(),
                    ),
                ),
            ),
        ]
        .concat();
        let plugin = fnv_esm::Plugin::parse(&bytes, "VanillaMarkers.esm".into()).unwrap();
        let script = crate::script::ScriptState::over_plugins(&[plugin]);

        let run = |script: &mut crate::script::ScriptState, body: &str| {
            script.rt.as_mut().unwrap().game.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), Default::default()).unwrap();
        };

        let m_vis_travel = read::Marker { form: MARKER_VIS_TRAVEL, world: 1, pos: [0.0; 3], name: "Visible Ridge".into(), kind: 5, flags: 1 | 2, radius: 10.0, reputation: None };
        let m_travel_only = read::Marker { form: MARKER_TRAVEL_ONLY, world: 1, pos: [4096.0, 0.0, 0.0], name: "Hidden Valley Camp".into(), kind: 2, flags: 2, radius: 10.0, reputation: None };
        let m_vis_only = read::Marker { form: MARKER_VIS_ONLY, world: 1, pos: [-4096.0, 0.0, 0.0], name: "Lookout Ridge".into(), kind: 3, flags: 1, radius: 10.0, reputation: None };
        let root = WorldMap { root: 1, transform: MapTransform::default(), texture: "root.dds".into(), usable: [2048.0; 2], nw: [-2, 1], se: [1, -2] };
        let mut st = DataState {
            tile_rows: true,
            maps: [(1, Some(root))].into_iter().collect(),
            markers: Some(vec![m_vis_travel.clone(), m_travel_only.clone(), m_vis_only.clone()]),
            ..DataState::default()
        };

        let mut world = World::new();
        world.insert_resource(Location { world: 1, interior: None });
        world.insert_resource(Player { heading: 0.0, pitch: 0.0, eye_height: 0.0, feet: [0.0; 3], body_eye: None, body_view: None, placed: None });
        world.insert_resource(script);
        let mut state = SystemState::<Game>::new(&mut world);

        // Before discovery (#3468 check 252 + 2026-11-22 live map-marker probe for script
        // values; Pip-Boy drawing an undiscovered FNAM=3 marker as discovered with its own
        // icon follows `known_marker` above and docs/spec-notes/ext-lutana.md §9):
        // - Visible | Can Travel To (FNAM=3) is known and travelable (Some(true)).
        // - Visible alone (FNAM=1) is known as undiscovered (Some(false)).
        // - Can Travel To alone (FNAM=2) is unknown (None) and stays in unmapped_of (Hide Unmapped Places).
        {
            let game = state.get_mut(&mut world).unwrap();
            let g = &game.script.as_ref().unwrap().rt.as_ref().unwrap().game;
            assert_eq!(known(&st, &game, &m_vis_travel), Some(true), "authored FNAM=3 starts travelable (2)");
            assert_eq!(known(&st, &game, &m_vis_only), Some(false), "authored FNAM=1 starts undiscovered (1)");
            assert_eq!(known(&st, &game, &m_travel_only), None, "authored FNAM=2 alone stays hidden before discovery");
            assert_eq!(st.unmapped_of(1, Some(g)), [MARKER_TRAVEL_ONLY].into_iter().collect(), "Hide Unmapped Places keeps authored Can Travel To-only marker");
            assert_eq!(st.unmapped_of(1, None), [MARKER_TRAVEL_ONLY].into_iter().collect());

            let shown = world_map(&mut st, &game, None, false);
            assert_eq!(shown.marks.len(), 2);
            assert_eq!(shown.marks[0].icon, Some((5, true)));
            assert_eq!(shown.marks[0].picture(), model::marker_icon(5, true));
            assert_eq!(shown.marks[1].icon, Some((3, false)));
            assert_eq!(shown.marks[1].picture(), model::marker_icon(3, false));
        }

        // SetMapMarkerTravel 0 on FNAM=3 drops it to undiscovered (Some(false));
        // ShowAllMapMarkers 1 0 0 (Explorer perk) reveals the Can Travel To-only marker as undiscovered (not discovered).
        run(
            world.resource_mut::<crate::script::ScriptState>().as_mut(),
            r#"
            VanillaVisTravelRef.SetMapMarkerTravel 0
            VanillaVisTravelRef.SetMapMarkerHidden 1
            ShowAllMapMarkers 1 0 0
            "#,
        );
        {
            let game = state.get_mut(&mut world).unwrap();
            let g = &game.script.as_ref().unwrap().rt.as_ref().unwrap().game;
            assert_eq!(known(&st, &game, &m_vis_travel), Some(false), "SetMapMarkerTravel 0 drops FNAM=3 to undiscovered");
            assert_eq!(known(&st, &game, &m_travel_only), Some(false), "Explorer ShowAllMapMarkers 1 0 0 shows FNAM=2 marker undiscovered");
            assert!(st.unmapped_of(1, Some(g)).is_empty());
        }

        // Clearing ShowAllMapMarkers restores Hide Unmapped Places for the Can Travel To-only marker;
        // SetMapMarkerVisible 0 on Visible | Can Travel To moves it into unmapped_of and off the map.
        run(
            world.resource_mut::<crate::script::ScriptState>().as_mut(),
            r#"
            ShowAllMapMarkers 0
            VanillaVisTravelRef.SetMapMarkerVisible 0
            "#,
        );
        {
            let game = state.get_mut(&mut world).unwrap();
            let g = &game.script.as_ref().unwrap().rt.as_ref().unwrap().game;
            assert_eq!(known(&st, &game, &m_vis_travel), None);
            assert_eq!(st.unmapped_of(1, Some(g)), [MARKER_VIS_TRAVEL, MARKER_TRAVEL_ONLY].into_iter().collect());
        }

        // Discovering a marker or ShowMap with canTravel=1 marks it travelable (Some(true));
        // subsequent ShowMap 0 leaves it at Some(true).
        st.discovered.insert(MARKER_VIS_ONLY);
        run(
            world.resource_mut::<crate::script::ScriptState>().as_mut(),
            r#"
            ShowMap VanillaTravelOnlyRef 1
            ShowMap VanillaTravelOnlyRef 0
            "#,
        );
        {
            let game = state.get_mut(&mut world).unwrap();
            assert_eq!(known(&st, &game, &m_vis_only), Some(true));
            assert_eq!(known(&st, &game, &m_travel_only), Some(true));
        }
    }

    #[test]
    fn world_map_discovers_materialized_and_placed_teammates_and_excludes_inactive() {
        use bevy::ecs::system::SystemState;
        use model::MapTransform;

        const WORLD_ID: u32 = 0x800;
        const EXT_CELL: u32 = 0x801;
        const NPC_A: u32 = 0x810;
        const NPC_B: u32 = 0x811;
        const NPC_DEAD: u32 = 0x812;
        const NPC_DISABLED: u32 = 0x813;
        const NPC_NON_TEAM: u32 = 0x814;
        const NPC_NEAR: u32 = 0x815;
        const REF_PLACED_ONLY: u32 = 0x820;
        const REF_MATERIALIZED: u32 = 0x821;
        const REF_DEAD: u32 = 0x822;
        const REF_DISABLED: u32 = 0x823;
        const REF_NON_TEAM: u32 = 0x824;
        const REF_NEAR: u32 = 0x825;

        let sub = |sig: &[u8; 4], data: &[u8]| [sig.as_slice(), &(data.len() as u16).to_le_bytes(), data].concat();
        let rec = |sig: &[u8; 4], flags: u32, id: u32, subs: &[Vec<u8>]| {
            let body: Vec<u8> = subs.iter().flat_map(|s| s.iter().copied()).collect();
            [sig.as_slice(), &(body.len() as u32).to_le_bytes(), &flags.to_le_bytes(), &id.to_le_bytes(), &0u32.to_le_bytes(), &0u32.to_le_bytes(), &body].concat()
        };
        let grp = |label: [u8; 4], kind: u32, body: &[u8]| [b"GRUP".as_slice(), &((24 + body.len()) as u32).to_le_bytes(), &label, &kind.to_le_bytes(), &0u64.to_le_bytes(), body].concat();
        let top = |sig: &[u8; 4], body: &[u8]| grp(*sig, 0, body);
        let pos_bytes = |x: f32, y: f32, z: f32| [x, y, z, 0.0, 0.0, 0.0].into_iter().flat_map(f32::to_le_bytes).collect::<Vec<u8>>();
        let npc = |id: u32, edid: &[u8], full: &[u8]| rec(b"NPC_", 0, id, &[sub(b"EDID", edid), sub(b"FULL", full)]);
        let achr = |id: u32, flags: u32, edid: &[u8], base: u32, pos: [f32; 3]| {
            rec(b"ACHR", flags, id, &[sub(b"EDID", edid), sub(b"NAME", &base.to_le_bytes()), sub(b"DATA", &pos_bytes(pos[0], pos[1], pos[2]))])
        };

        let tes4 = rec(b"TES4", 0, 0, &[sub(b"HEDR", &[1.34f32.to_le_bytes().as_slice(), &0u32.to_le_bytes(), &0x800u32.to_le_bytes()].concat())]);
        let npcs = [
            npc(0x7, b"Player\0", b"Player\0"),
            npc(NPC_A, b"PlacedMate\0", b"Scout Alpha\0"),
            npc(NPC_B, b"MatMate\0", b"Scout Bravo\0"),
            npc(NPC_DEAD, b"DeadMate\0", b"Fallen Scout\0"),
            npc(NPC_DISABLED, b"DisMate\0", b"Hidden Scout\0"),
            npc(NPC_NON_TEAM, b"Bystander\0", b"Town Drifter\0"),
            npc(NPC_NEAR, b"NearMate\0", b"Close Scout\0"),
        ]
        .concat();
        let persistent = [
            achr(REF_PLACED_ONLY, 0x400, b"PlacedMateRef\0", NPC_A, [8000.0, 0.0, 0.0]),
            achr(REF_MATERIALIZED, 0x400, b"MatMateRef\0", NPC_B, [-8000.0, 2048.0, 0.0]),
            achr(REF_DEAD, 0x400, b"DeadMateRef\0", NPC_DEAD, [9000.0, 0.0, 0.0]),
            achr(REF_DISABLED, 0x400 | 0x800, b"DisMateRef\0", NPC_DISABLED, [10000.0, 0.0, 0.0]),
            achr(REF_NON_TEAM, 0x400, b"BystanderRef\0", NPC_NON_TEAM, [11000.0, 0.0, 0.0]),
            achr(REF_NEAR, 0x400, b"NearMateRef\0", NPC_NEAR, [500.0, 0.0, 0.0]),
        ]
        .concat();
        let wrld = [
            rec(b"WRLD", 0, WORLD_ID, &[sub(b"EDID", b"TestWasteland\0")]),
            grp(
                WORLD_ID.to_le_bytes(),
                1,
                &[
                    rec(b"CELL", 0, EXT_CELL, &[sub(b"EDID", b"TestExtCell\0"), sub(b"DATA", &[0])]),
                    grp(EXT_CELL.to_le_bytes(), 6, &grp(EXT_CELL.to_le_bytes(), 8, &persistent)),
                ]
                .concat(),
            ),
        ]
        .concat();
        let bytes = [tes4, top(b"NPC_", &npcs), top(b"WRLD", &wrld)].concat();
        let plugin = fnv_esm::Plugin::parse(&bytes, "CompanionMap.esm".into()).unwrap();
        let mut script = crate::script::ScriptState::over_plugins(&[plugin]);

        // Build matching SimState from the same plugin.
        let sim_plugin = fnv_esm::Plugin::parse(&bytes, "CompanionMap.esm".into()).unwrap();
        let plugins: &'static [fnv_esm::Plugin] = Box::leak(Box::new([sim_plugin]));
        let lo = fnv_esm::LoadOrder::new(plugins).unwrap();
        let schema = fnv_esm::Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();
        let forms: &'static fnv_sim::FormStore = Box::leak(Box::new(fnv_sim::FormStore::build(&lo, &schema)));
        let refs: &'static fnv_sim::RefTable = Box::leak(Box::new(fnv_sim::RefTable::build(&lo, &schema)));
        let mut sim = SimState { forms, refs, world: fnv_sim::World::new(forms, refs, fnv_sim::SimConfig::default()) };

        // Materialize only REF_MATERIALIZED (moving it to a new exterior position); leave REF_PLACED_ONLY unmaterialized!
        sim.world.get(FormId(REF_MATERIALIZED)).unwrap().pos = [-9000.0, 4096.0, 0.0];
        assert!(sim.world.peek(FormId(REF_PLACED_ONLY)).is_none(), "REF_PLACED_ONLY must remain unmaterialized");

        // Mark teammates in script state (leaving REF_NON_TEAM untagged, REF_DEAD dead, REF_DISABLED initially disabled).
        {
            let g = &mut script.rt.as_mut().unwrap().game;
            for r in [REF_PLACED_ONLY, REF_MATERIALIZED, REF_DEAD, REF_DISABLED, REF_NEAR] {
                g.extra.entry(FormId(r)).or_default().teammate = true;
            }
            g.extra.entry(FormId(REF_DEAD)).or_default().dead = true;
        }

        let root = WorldMap { root: WORLD_ID, transform: MapTransform::default(), texture: "world.dds".into(), usable: [2048.0; 2], nw: [-4, 4], se: [4, -4] };
        let mut st = DataState {
            tile_rows: true,
            maps: [(WORLD_ID, Some(root))].into_iter().collect(),
            markers: Some(Vec::new()),
            ..DataState::default()
        };

        let mut world = World::new();
        world.insert_resource(Location { world: WORLD_ID, interior: None });
        world.insert_resource(Player { heading: 0.0, pitch: 0.0, eye_height: 0.0, feet: [0.0; 3], body_eye: None, body_view: None, placed: None });
        world.insert_resource(script);
        world.insert_resource(sim);
        let mut state = SystemState::<Game>::new(&mut world);

        {
            let game = state.get_mut(&mut world).unwrap();
            let shown = world_map(&mut st, &game, None, false);
            let names: Vec<&str> = shown.companions.iter().map(|c| c.1.as_str()).collect();
            assert_eq!(names, vec!["Scout Alpha", "Scout Bravo"], "discovers both unmaterialized and materialized exterior teammates; excludes dead, disabled, non-teammate, and too-near");
            let parts = shown.world.as_ref().expect("world parts");
            assert_eq!(parts.companion_font, COMPANION_FONT as u8);
            assert_eq!(parts.companions.iter().map(|c| (c.0, c.2.as_str(), c.3)).collect::<Vec<_>>(), vec![(REF_PLACED_ONLY, "Scout Alpha", false), (REF_MATERIALIZED, "Scout Bravo", false)]);
        }

        // Enabling bNoMinCompanionMapDistance + bCompanionsUseLocationFontOnMap reveals the nearby teammate and switches to font 1.
        {
            let mut script = world.resource_mut::<crate::script::ScriptState>();
            let g = &mut script.rt.as_mut().unwrap().game;
            g.tweaks.no_min_companion_distance = true;
            g.tweaks.companions_location_font = true;
        }
        {
            let game = state.get_mut(&mut world).unwrap();
            let shown = world_map(&mut st, &game, None, false);
            let names: Vec<&str> = shown.companions.iter().map(|c| c.1.as_str()).collect();
            assert_eq!(names, vec!["Scout Alpha", "Scout Bravo", "Close Scout"]);
            assert_eq!(shown.world.as_ref().unwrap().companion_font, LOCATION_FONT as u8);
        }
    }
}
