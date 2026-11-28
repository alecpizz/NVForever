//! What the settings page shows, as data (#1953, the menu overhaul; docs/design/settings-page.md):
//! which lines a tab, the search or the Changed list lists, which line is chosen, and how the
//! keys move over them. `crate::pause` draws it and turns the keys and the pointer into these
//! steps; nothing here draws or reads input.
//!
//! **A tab** lists its categories (`crate::settings_layout`): each a heading the player opens
//! and closes ([`Folding`]; open by default, the Tweaks tab's closed), its rows while it is
//! open, and an empty line after each open category ([`ViewEntry::Gap`]) [design: the owner's
//! menu overhaul, 2026-11-15]. A heading is a line the keys can choose ([`RowId::Heading`]):
//! Enter, A or a click open or close it, Left closes and Right opens it. The Tweaks tab
//! shows one group at a time, picked by its first row, Category ([`RowId::Group`]).
//!
//! **Search and the Changed list** [design: #1953 points 5 and 6] list rows of every tab:
//! [`search`] the rows whose words hold every term of the query, [`changed`] the rows set
//! away from their default, each in the page's order ([`catalogue`]) under a line that
//! says where it lives (its tab and category, [`ViewEntry::Crumb`], [`listing`]), every
//! category open; a crumb is a line the keys can choose, and Enter, a click, Left or Right
//! close its place to the crumb alone and open it again, for the run only ([`Folding::crumbs`],
//! [design: owner 2026-11-15, the fifth build]). A row hidden where it lives (a colour's channel, a controller's row, a
//! control or sensitivity of the device not in use, a row its module hides) is listed as the
//! row that stands for it there ([`shown_as`]), once, and the page names what it stands for
//! ([`stands_for`]): a result of the other device is the Input Device row, so acting on it
//! switches the device and never edits the other device's binding through the one in use
//! [design: #1953].
//!
//! **A row** is a registered setting by its index in the registry (`crate::settings`,
//! whose order stays the one the values, the INI and the test aids go by), or one of the
//! Controls tab's own rows (`crate::bindings`: the input device, each control's key or
//! button, Invert Y, the sensitivities, Rumble), each by a name of its own ([`RowId`]), never
//! by where it stands.
//!
//! **What is shown** is worked out each frame from the game's state ([`Visibility`]): a
//! colour's red, green and blue only while the colour is Custom
//! (`crate::ui_colour::channel_shown`), the controller's rows of the Controls tab only with
//! the controller as the input device ([`crate::settings_layout::controller_only`]), and the
//! rows a module shows only while they apply (`crate::settings::set_shown_when`: the
//! upscaler's own rows, Generated Frames). A hidden row keeps its tab and category; a choice
//! on it moves to the row that stands for it ([`parent_of`]) [design].
//!
//! **The keys** [design: #1953, the overhaul]: Up and Down step over the rows and headings,
//! round the ends, passing the empty lines ([`step_selectable`]); Page Up and Page Down go to
//! the category before or after: its first row while it is open, else its heading
//! ([`step_heading`]). The window of `shown` lines keeps the chosen line in view, and the
//! heading too when the chosen row is its category's first ([`follow`]); a heading over rows
//! is never drawn alone on the window's last line ([`drawn_slots`]). How many lines the
//! window has is the page's layout's (`crate::pause`, #2377); the original's
//! `options_container` shows nine [documented: `_number_of_visible_items` 9].

use crate::bindings::{Bindings, Control, InputDevice, GAME_CONTROLS};
use crate::settings::Setting;
use crate::settings_layout::{controller_only, resolve_layout, LayoutEntry, LayoutItem, Placement, ResolvedLayout, CONTROLS_TAB, TAB_ORDER, TWEAKS_TAB};
use bevy::prelude::*;
use fnv_util::words::tr;

/// One of the Controls tab's own rows (`crate::bindings`), by what it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlRow {
    /// Input Device: keyboard and mouse, or a controller.
    Device,
    /// Saved opt-out beside Input Device [design, #2200].
    AutomaticInput,
    /// A control's key and mouse button (`KeyboardMouse`) or its controller button.
    Binding(Control, InputDevice),
    Invert,
    /// The look sensitivity of a device: Mouse Sensitivity or Stick Sensitivity.
    Sensitivity(InputDevice),
    /// A device's Vertical Sensitivity, a multiple of its horizontal look (#2201).
    Vertical(InputDevice),
    /// A device's Aim Sensitivity: its look while aiming down the sights or through a scope,
    /// a multiple of the unzoomed look [design: owner 2026-11-15].
    AimSensitivity(InputDevice),
    /// With a controller only.
    Rumble,
}

impl ControlRow {
    /// The Controls tab's own rows with `device` in use: the device, automatic switching
    /// [design, #2200], every control, Invert
    /// Y, the device's sensitivity, vertical sensitivity and aim sensitivity, and with a
    /// controller Rumble.
    pub fn rows(device: InputDevice) -> Vec<ControlRow> {
        let mut out = vec![ControlRow::Device, ControlRow::AutomaticInput];
        out.extend(GAME_CONTROLS.iter().map(|(c, _, _)| ControlRow::Binding(*c, device)));
        out.extend([ControlRow::Invert, ControlRow::Sensitivity(device), ControlRow::Vertical(device), ControlRow::AimSensitivity(device)]);
        if device == InputDevice::Controller {
            out.push(ControlRow::Rumble);
        }
        out
    }

    /// Every Controls row of both devices, in the order of [`ControlRow::rows`].
    pub fn all() -> Vec<ControlRow> {
        let mut out = vec![ControlRow::Device, ControlRow::AutomaticInput];
        for d in [InputDevice::KeyboardMouse, InputDevice::Controller] {
            out.extend(GAME_CONTROLS.iter().map(|(c, _, _)| ControlRow::Binding(*c, d)));
        }
        out.push(ControlRow::Invert);
        for d in [InputDevice::KeyboardMouse, InputDevice::Controller] {
            out.extend([ControlRow::Sensitivity(d), ControlRow::Vertical(d), ControlRow::AimSensitivity(d)]);
        }
        out.push(ControlRow::Rumble);
        out
    }

    /// The row's place in [`ControlRow::rows`], the same with either device (the UI sounds
    /// hear a move by it).
    fn ordinal(self) -> usize {
        match self {
            ControlRow::Device => 0,
            ControlRow::AutomaticInput => 1,
            ControlRow::Binding(c, _) => 2 + GAME_CONTROLS.iter().position(|(k, _, _)| *k == c).unwrap_or(0),
            ControlRow::Invert => GAME_CONTROLS.len() + 2,
            ControlRow::Sensitivity(_) => GAME_CONTROLS.len() + 3,
            ControlRow::Vertical(_) => GAME_CONTROLS.len() + 4,
            ControlRow::AimSensitivity(_) => GAME_CONTROLS.len() + 5,
            ControlRow::Rumble => GAME_CONTROLS.len() + 6,
        }
    }

    /// The row as the log and the test aids name it (`device`, `bind:Jump`,
    /// `padbind:Jump`, `invert`, `sensitivity`, `vertical`, `aim-sensitivity`, `rumble`).
    pub fn name(self) -> String {
        match self {
            ControlRow::Device => "device".into(),
            ControlRow::AutomaticInput => "automatic-input".into(),
            ControlRow::Binding(c, InputDevice::KeyboardMouse) => format!("bind:{}", Bindings::name(c)),
            ControlRow::Binding(c, InputDevice::Controller) => format!("padbind:{}", Bindings::name(c)),
            ControlRow::Invert => "invert".into(),
            ControlRow::Sensitivity(_) => "sensitivity".into(),
            ControlRow::Vertical(_) => "vertical".into(),
            ControlRow::AimSensitivity(_) => "aim-sensitivity".into(),
            ControlRow::Rumble => "rumble".into(),
        }
    }

    /// The row's name in the layout table (after `controls:`), the same for both devices
    /// (`bind:Jump`).
    pub fn table_name(self) -> String {
        match self {
            ControlRow::Binding(c, _) => format!("bind:{}", Bindings::name(c)),
            other => other.name(),
        }
    }

    /// The Controls row the table names `name` with `device` in use; `None`: no such row, or
    /// not shown with that device (Rumble without a controller).
    pub fn of_table(name: &str, device: InputDevice) -> Option<ControlRow> {
        Some(match name {
            "device" => ControlRow::Device,
            "automatic-input" => ControlRow::AutomaticInput,
            "invert" => ControlRow::Invert,
            "sensitivity" => ControlRow::Sensitivity(device),
            "vertical" => ControlRow::Vertical(device),
            "aim-sensitivity" => ControlRow::AimSensitivity(device),
            "rumble" if device == InputDevice::Controller => ControlRow::Rumble,
            "rumble" => return None,
            _ => ControlRow::Binding(Bindings::by_name(name.strip_prefix("bind:")?)?, device),
        })
    }

    /// The Controls rows of both devices the table names `name`: one, or a row for each device.
    fn of_table_all(name: &str) -> Vec<ControlRow> {
        let kb = ControlRow::of_table(name, InputDevice::KeyboardMouse);
        let pad = ControlRow::of_table(name, InputDevice::Controller);
        match (kb, pad) {
            (Some(a), Some(b)) if a != b => vec![a, b],
            (a, b) => a.or(b).into_iter().collect(),
        }
    }
}

/// A line of the settings page the keys can choose: a registered setting, one of the
/// Controls tab's own rows, a category's heading, or the Tweaks tab's Category row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowId {
    /// Its index in `crate::settings::Settings`.
    Setting(usize),
    Control(ControlRow),
    /// A category's heading: Enter opens or closes it.
    Heading(Placement),
    /// The Tweaks tab's Category row: which group the tab shows.
    Group,
}

/// What the UI sounds take for a Controls row: past any setting's index.
const CONTROL_SOUND_BASE: usize = 10_000;
/// What the UI sounds take for a heading and the Category row: past the Controls rows.
const HEADING_SOUND_BASE: usize = 20_000;
/// What the UI sounds take for no row (an empty tab).
pub const NO_ROW_SOUND: usize = usize::MAX;

impl RowId {
    /// The row as the log names it: a setting's key, a Controls row's [`ControlRow::name`],
    /// a heading's place in brackets, `category`.
    pub fn name(self, list: &[Setting]) -> String {
        match self {
            RowId::Setting(i) => list.get(i).map_or_else(|| format!("row {i}"), |s| s.key.to_string()),
            RowId::Control(c) => c.name(),
            RowId::Heading(p) => format!("[{}]", p.path()),
            RowId::Group => "category".into(),
        }
    }

    /// The number the UI sounds tell rows apart by (`crate::ui_sounds`).
    pub fn sound_id(self) -> usize {
        match self {
            RowId::Setting(i) => i,
            RowId::Control(c) => CONTROL_SOUND_BASE + c.ordinal(),
            RowId::Heading(p) => HEADING_SOUND_BASE + 1 + p.path().bytes().fold(0usize, |h, b| (h.wrapping_mul(31).wrapping_add(usize::from(b))) % 1_000_000),
            RowId::Group => HEADING_SOUND_BASE,
        }
    }
}

/// A line of the page's list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewEntry {
    /// A category's heading on a tab: the keys can choose it ([`RowId::Heading`]).
    Heading(Placement),
    /// On the search and the Changed list, where the rows after it live (`Gameplay >
    /// Dialogue`): a line the keys can choose, which opens and closes its place
    /// ([`RowId::Heading`]).
    Crumb(Placement),
    Row(RowId),
    /// The empty line after an open category.
    Gap,
}

impl ViewEntry {
    /// The line the keys can choose here: a row or a heading.
    pub fn selectable(self) -> Option<RowId> {
        match self {
            ViewEntry::Row(id) => Some(id),
            // A crumb opens and closes its place on the search and the Changed list
            // [design: owner 2026-11-15, the fifth build].
            ViewEntry::Heading(p) | ViewEntry::Crumb(p) => Some(RowId::Heading(p)),
            ViewEntry::Gap => None,
        }
    }

    /// A heading or a crumb: a line naming the rows after it (the page's heading links show
    /// while there is one).
    pub fn titles(self) -> bool {
        matches!(self, ViewEntry::Heading(_) | ViewEntry::Crumb(_))
    }
}

/// What the settings page lists: a tab, the search's results or the Changed list (#1953
/// points 5 and 6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsScope {
    Tab(&'static str),
    Search,
    Changed,
}

impl Default for SettingsScope {
    fn default() -> SettingsScope {
        SettingsScope::Tab(TAB_ORDER[0])
    }
}

impl SettingsScope {
    /// Its name in English (the log's; the page translates it).
    pub fn name(self) -> &'static str {
        match self {
            SettingsScope::Tab(t) => t,
            SettingsScope::Search => SEARCH,
            SettingsScope::Changed => CHANGED,
        }
    }
}

/// The rail's first two lines [design: #1953].
pub const SEARCH: &str = "Search";
pub const CHANGED: &str = "Changed";

/// A line of the rail left of the column: Search, Changed, the tabs, then the page's
/// Defaults and Back (drawn at the bottom right, reached by the rail's keys).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RailItem {
    Search,
    Changed,
    Tab(&'static str),
    Defaults,
    Back,
}

impl RailItem {
    /// Its name in English (the log's).
    pub fn name(self) -> &'static str {
        match self {
            RailItem::Search => SEARCH,
            RailItem::Changed => CHANGED,
            RailItem::Tab(t) => t,
            RailItem::Defaults => "Defaults",
            RailItem::Back => "Back",
        }
    }

    /// The rail's line for scope `s`.
    pub fn of(s: SettingsScope) -> RailItem {
        match s {
            SettingsScope::Tab(t) => RailItem::Tab(t),
            SettingsScope::Search => RailItem::Search,
            SettingsScope::Changed => RailItem::Changed,
        }
    }
}

/// Where the keys are on the settings page: on the rows, on the rail (F6, a
/// controller's Y), or in the search's text (typing) [design: #1953].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SettingsFocus {
    #[default]
    Rows,
    Rail(RailItem),
    SearchText,
}

/// The rail's lines: Search, Changed, every tab of `layout`, Defaults, Back.
pub fn rail(layout: &ResolvedLayout) -> Vec<RailItem> {
    let mut out = vec![RailItem::Search, RailItem::Changed];
    out.extend(layout.tab_names().into_iter().map(RailItem::Tab));
    out.extend([RailItem::Defaults, RailItem::Back]);
    out
}

/// What the game's state shows of the rows (module notes).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Visibility {
    pub device: InputDevice,
    /// The HUD's and the Pip-Boy's colour are Custom (their sliders shown).
    pub colours: [bool; 2],
    /// The rows their module hides now, each with the row that stands for it
    /// (`crate::settings::Settings::hidden`).
    pub hidden: Vec<(usize, usize)>,
}

impl Visibility {
    /// As the world has it now.
    pub fn of_world(world: &World) -> Visibility {
        let device = world.get_resource::<Bindings>().map_or(InputDevice::default(), |b| b.device);
        let hidden = world.get_resource::<crate::settings::Settings>().map(|s| s.hidden(world)).unwrap_or_default();
        Visibility { hidden, ..Visibility::of(device, world.get_resource::<crate::ui_colour::CustomColours>()) }
    }

    /// With `device` in use and the colours' Custom choices `flags`, no row hidden by its
    /// module.
    pub fn of(device: InputDevice, flags: Option<&crate::ui_colour::CustomColours>) -> Visibility {
        Visibility { device, colours: [crate::ui_colour::custom_shown(flags, 0), crate::ui_colour::custom_shown(flags, 1)], hidden: Vec::new() }
    }

    /// The same with the rows their module hides now (`hidden`, [`HiddenRows`]).
    pub fn with_hidden(self, hidden: Option<&HiddenRows>) -> Visibility {
        Visibility { hidden: hidden.map(|h| h.0.clone()).unwrap_or_default(), ..self }
    }

    /// Whether registered row `i` (`s`) is shown.
    pub fn shows(&self, i: usize, s: &Setting) -> bool {
        if self.hidden.iter().any(|(h, _)| *h == i) {
            return false;
        }
        if let Some((which, _)) = crate::ui_colour::channel_of(s.key) {
            return self.colours[which];
        }
        !controller_only(s.key) || self.device == InputDevice::Controller
    }

    /// Whether table line `item` is shown with the registry `list`.
    fn shows_item(&self, list: &[Setting], item: LayoutItem) -> bool {
        match item {
            LayoutItem::Setting(i) => list.get(i).is_some_and(|s| self.shows(i, s)),
            LayoutItem::Control(name) => ControlRow::of_table(name, self.device).is_some(),
        }
    }
}

/// The row that stands for registered row `i` while it is hidden: the row its module names,
/// its colour's list, or the Input Device row for a controller's row; `None` when it is
/// shown or has none.
pub fn parent_of(list: &[Setting], i: usize, vis: &Visibility) -> Option<RowId> {
    let s = list.get(i)?;
    if vis.shows(i, s) {
        return None;
    }
    if let Some((_, parent)) = vis.hidden.iter().find(|(h, p)| *h == i && *p != i) {
        return Some(RowId::Setting(*parent));
    }
    if let Some((_, colour)) = crate::ui_colour::channel_of(s.key) {
        return list.iter().position(|r| r.key == colour).map(RowId::Setting);
    }
    controller_only(s.key).then_some(RowId::Control(ControlRow::Device))
}

/// Which categories are open, and the Tweaks tab's group shown [design: the overhaul, owner
/// 2026-11-15: open by default, the Tweaks tab's closed; each opens and closes on its own and
/// stays as the player left it]. Only what differs from the default is kept (`crate::pause`
/// keeps it in `[UI]` of the INI).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Folding {
    /// Each category the player opened or closed against its default: its path
    /// ([`Placement::path`]) and whether it is open.
    pub set: Vec<(String, bool)>,
    /// The Tweaks tab's group shown (`None`: its first).
    pub group: Option<String>,
    /// The places closed on the search and the Changed list (their crumbs' paths), for this
    /// run only: every place opens again in a new run [design: owner 2026-11-15, the fifth
    /// build: "in the searched and changed menu the categories cannot be collapsed"].
    pub crumbs: Vec<String>,
}

impl Folding {
    /// Whether the category at `p` is open.
    pub fn is_open(&self, p: &Placement) -> bool {
        let path = p.path();
        self.set.iter().find(|(k, _)| *k == path).map_or(!p.closed_by_default(), |(_, open)| *open)
    }

    /// Open (`true`) or close the category at `p`; false: it already was.
    pub fn put(&mut self, p: &Placement, open: bool) -> bool {
        if self.is_open(p) == open {
            return false;
        }
        let path = p.path();
        self.set.retain(|(k, _)| *k != path);
        if open == p.closed_by_default() {
            self.set.push((path, open));
        }
        true
    }

    /// Whether the place at `p` is open on the search and the Changed list.
    pub fn crumb_open(&self, p: &Placement) -> bool {
        !self.crumbs.contains(&p.path())
    }

    /// Open (`true`) or close the place at `p` on the search and the Changed list; false: it
    /// already was.
    pub fn put_crumb(&mut self, p: &Placement, open: bool) -> bool {
        if self.crumb_open(p) == open {
            return false;
        }
        let path = p.path();
        if open {
            self.crumbs.retain(|k| *k != path);
        } else {
            self.crumbs.push(path);
        }
        true
    }

    /// The group of `tab`'s (`groups`) shown: the one chosen when it is one of them, else
    /// the first.
    pub fn group_of(&self, groups: &[&'static str]) -> Option<&'static str> {
        groups.iter().copied().find(|g| self.group.as_deref() == Some(*g)).or_else(|| groups.first().copied())
    }

    /// The text `[UI]` keeps: `-path` for a category closed, `+path` for one opened, `;`
    /// between them.
    pub fn to_ini(&self) -> String {
        self.set.iter().map(|(k, open)| format!("{}{k}", if *open { '+' } else { '-' })).collect::<Vec<_>>().join(";")
    }

    /// [`Folding::to_ini`]'s text read back (anything else in it left out).
    pub fn from_ini(text: &str) -> Vec<(String, bool)> {
        text.split(';')
            .filter_map(|e| {
                let e = e.trim();
                let open = match e.chars().next()? {
                    '+' => true,
                    '-' => false,
                    _ => return None,
                };
                let path = e[1..].trim();
                (!path.is_empty()).then(|| (path.to_string(), open))
            })
            .collect()
    }
}

/// The tab a row is on: a setting's by the table, a Controls row's Controls, a heading's its
/// own, the Category row's Tweaks.
pub fn home_tab(layout: &ResolvedLayout, id: RowId) -> &'static str {
    match id {
        RowId::Setting(i) => layout.placement_of(i).map_or(TAB_ORDER[0], |p| p.tab),
        RowId::Control(_) => CONTROLS_TAB,
        RowId::Heading(p) => p.tab,
        RowId::Group => TWEAKS_TAB,
    }
}

/// The rows the search found and the rows away from their default, as worked out last
/// ([`search`], [`changed`]): each the rows themselves, hidden ones too, in the page's
/// order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub search: Vec<RowId>,
    pub changed: Vec<RowId>,
}

/// The lines `scope` lists: a tab's categories as `folding` has them open (on the Tweaks
/// tab its Category row first, then its group's); the search's results or the Changed list
/// from `found`, each row as it is shown ([`shown_as`]) under a crumb of where it lives
/// ([`listing`]).
pub fn project(layout: &ResolvedLayout, list: &[Setting], scope: SettingsScope, vis: &Visibility, found: &Found, folding: &Folding) -> Vec<ViewEntry> {
    let tab = match scope {
        SettingsScope::Tab(tab) => tab,
        SettingsScope::Search => return listing_folded(layout, &shown_rows(layout, list, &found.search, vis), |p| folding.crumb_open(p)),
        SettingsScope::Changed => return listing_folded(layout, &shown_rows(layout, list, &found.changed, vis), |p| folding.crumb_open(p)),
    };
    let mut out = Vec::new();
    let groups = layout.tab(tab).map(|t| t.subgroups()).unwrap_or_default();
    let group = folding.group_of(&groups);
    if group.is_some() {
        out.extend([ViewEntry::Row(RowId::Group), ViewEntry::Gap]);
    }
    out.extend(layout.tab_entries(tab, group, |i| vis.shows_item(list, i), |p| folding.is_open(p)).into_iter().filter_map(|e| {
        Some(match e {
            LayoutEntry::Heading(p) => ViewEntry::Heading(p),
            LayoutEntry::Setting(i) => ViewEntry::Row(RowId::Setting(i)),
            LayoutEntry::Control(name) => ViewEntry::Row(RowId::Control(ControlRow::of_table(name, vis.device)?)),
            LayoutEntry::Gap => ViewEntry::Gap,
        })
    }));
    out
}

/// How many of the category at `p`'s rows are shown with `vis` (what its heading counts
/// while it is chosen).
pub fn category_count(layout: &ResolvedLayout, list: &[Setting], p: &Placement, vis: &Visibility) -> usize {
    layout.shown_count(p, |i| vis.shows_item(list, i))
}

/// The line `id` is on.
pub fn slot_of(entries: &[ViewEntry], id: RowId) -> Option<usize> {
    entries.iter().position(|e| e.selectable() == Some(id))
}

/// The first row; a list of closed categories only: the first heading (none: nothing to
/// choose).
pub fn first_row(entries: &[ViewEntry]) -> Option<RowId> {
    entries.iter().find_map(|e| if let ViewEntry::Row(id) = e { Some(*id) } else { None }).or_else(|| entries.iter().find_map(|e| e.selectable()))
}

/// The lines the keys can choose, in order.
fn rows(entries: &[ViewEntry]) -> Vec<RowId> {
    entries.iter().filter_map(|e| e.selectable()).collect()
}

/// The line `step` lines after (1) or before (-1) `selected` that the keys can choose (rows,
/// headings and crumbs), round the ends, empty lines passed; none chosen: the first (or last)
/// one.
pub fn step_selectable(entries: &[ViewEntry], selected: Option<RowId>, step: i32) -> Option<RowId> {
    let rows = rows(entries);
    if rows.is_empty() {
        return None;
    }
    let n = rows.len() as i32;
    match selected.and_then(|s| rows.iter().position(|r| *r == s)) {
        Some(at) => Some(rows[(at as i32 + step).rem_euclid(n) as usize]),
        None if step < 0 => rows.last().copied(),
        None => rows.first().copied(),
    }
}

/// The stops of Page Up and Page Down: each category or place (or the rows before the first,
/// without one) with its title, its first line to choose (its first row while it is open,
/// else its heading or crumb) and the lines it spans.
fn sections(entries: &[ViewEntry]) -> Vec<(Option<Placement>, RowId, std::ops::Range<usize>)> {
    let mut starts: Vec<usize> = entries.iter().enumerate().filter(|(_, e)| e.titles()).map(|(k, _)| k).collect();
    if entries.iter().position(|e| e.selectable().is_some()).is_some_and(|k| starts.first().is_none_or(|s| k < *s)) {
        starts.insert(0, entries.iter().position(|e| e.selectable().is_some()).unwrap_or(0));
    }
    let mut out = Vec::new();
    for (n, &start) in starts.iter().enumerate() {
        let end = starts.get(n + 1).copied().unwrap_or(entries.len());
        let title = match entries[start] {
            ViewEntry::Heading(p) | ViewEntry::Crumb(p) => Some(p),
            _ => None,
        };
        let first_row = entries[start..end].iter().find_map(|e| if let ViewEntry::Row(id) = e { Some(*id) } else { None });
        let stop = match (first_row, entries[start]) {
            (Some(r), _) => Some(r),
            (None, ViewEntry::Heading(p) | ViewEntry::Crumb(p)) => Some(RowId::Heading(p)),
            _ => None,
        };
        if let Some(stop) = stop {
            out.push((title, stop, start..end));
        }
    }
    out
}

/// Page Down (`step` 1) or Page Up (-1): the category after or before `selected`'s (its
/// first row while open, else its heading or crumb), round the ends, and its title (`None`: rows
/// without one). A tab without headings is one section: its first row. Nothing to choose:
/// `None`.
pub fn step_heading(entries: &[ViewEntry], selected: Option<RowId>, step: i32) -> Option<(RowId, Option<Placement>)> {
    let secs = sections(entries);
    if secs.is_empty() {
        return None;
    }
    let n = secs.len() as i32;
    let slot = selected.and_then(|s| slot_of(entries, s));
    let to = match slot.and_then(|k| secs.iter().position(|(_, _, span)| span.contains(&k))) {
        Some(at) => (at as i32 + step).rem_euclid(n) as usize,
        None if step < 0 => secs.len() - 1,
        None => 0,
    };
    let (title, stop, _) = secs[to];
    Some((stop, title))
}

/// The category or crumb over `id` (`None`: a section without one); a heading's own.
pub fn heading_of(entries: &[ViewEntry], id: RowId) -> Option<Placement> {
    if let RowId::Heading(p) = id {
        return Some(p);
    }
    let k = slot_of(entries, id)?;
    entries[..k].iter().rev().find_map(|e| match e {
        ViewEntry::Heading(p) | ViewEntry::Crumb(p) => Some(*p),
        ViewEntry::Row(_) | ViewEntry::Gap => None,
    })
}

/// Line `k` is a heading or crumb with a row right after it (drawn with that row, never alone
/// on a window's last line).
fn titles_rows(entries: &[ViewEntry], k: usize) -> bool {
    entries.get(k).is_some_and(|e| e.titles()) && matches!(entries.get(k + 1), Some(ViewEntry::Row(_)))
}

/// The first line of a window of `shown` from `first` keeping line `s` in view, and the row
/// after it too when `s` titles it (so `s` is not the window's dropped last line).
fn keep(entries: &[ViewEntry], first: usize, s: usize, shown: usize) -> usize {
    let n = entries.len();
    let first = crate::menu::model::keep_in_view(first, s, n, shown);
    if shown > 1 && titles_rows(entries, s) {
        crate::menu::model::keep_in_view(first, s + 1, n, shown)
    } else {
        first
    }
}

/// The first line shown of `entries` from `first`, `shown` (at least 1) at once, moved as
/// little as keeps the chosen line `selected` in view, and its heading too when it is the
/// category's first row and the window has room for both.
pub fn follow(entries: &[ViewEntry], first: usize, selected: Option<usize>, shown: usize) -> usize {
    let shown = shown.max(1);
    let n = entries.len();
    let last = n.saturating_sub(shown);
    let Some(s) = selected.filter(|s| *s < n) else { return first.min(last) };
    let mut first = keep(entries, first, s, shown);
    // A row's title, not a heading's (the closed heading over it is another category).
    if shown > 1 && s > 0 && entries[s - 1].titles() && matches!(entries[s], ViewEntry::Row(_)) {
        first = first.min(s - 1);
    }
    first.min(last)
}

/// The window's first line this frame, from `first`: when the choice is new since the
/// window last followed it (`followed`), as [`follow`] puts it (its heading too); else
/// only as far as keeps the chosen line in view, so a window the wheel or the scroll bar
/// moved stays where they put it and is not pulled back to the heading over the chosen
/// row [design: #1953].
pub fn window_first(entries: &[ViewEntry], first: usize, selected: Option<RowId>, followed: Option<RowId>, shown: usize) -> usize {
    let shown = shown.max(1);
    let at = selected.and_then(|id| slot_of(entries, id));
    if selected != followed {
        return follow(entries, first, at, shown);
    }
    let n = entries.len();
    let last = n.saturating_sub(shown);
    match at.filter(|s| *s < n) {
        Some(s) => keep(entries, first, s, shown).min(last),
        None => first.min(last),
    }
}

/// The lines drawn of a window of `shown` (at least 1) from `first`, less an empty line or a
/// heading or crumb with rows after it on the last line (it is drawn with its first row in
/// the next window) unless it is the window's only line. A closed one (no row after it) is
/// drawn there: the keys can choose it.
pub fn drawn_slots(entries: &[ViewEntry], first: usize, shown: usize) -> std::ops::Range<usize> {
    let first = first.min(entries.len());
    let mut end = (first + shown.max(1)).min(entries.len());
    // Never down to a window with no line the keys can choose (an empty line over a heading).
    while end > first + 1 && (entries[end - 1] == ViewEntry::Gap || titles_rows(entries, end - 1)) && entries[first..end - 1].iter().any(|e| e.selectable().is_some()) {
        end -= 1;
    }
    first..end
}

/// The first line of a window of `shown` lines the wheel or the scroll bar moved to `first`
/// (`step`: the way it moved, down for 0): a one-line window that would show only a line the
/// keys cannot choose goes on past it to the line beyond (else back to the one before), so the
/// window always draws a line the choice can move to [design: #2377].
pub fn land(entries: &[ViewEntry], first: usize, step: i64, shown: usize) -> usize {
    let blank = |k: usize| entries.get(k).is_some_and(|e| e.selectable().is_none());
    if shown > 1 || !blank(first) {
        return first;
    }
    let ahead = if step < 0 { first.checked_sub(1) } else { Some(first + 1) };
    let back = if step < 0 { Some(first + 1) } else { first.checked_sub(1) };
    [ahead, back].into_iter().flatten().find(|k| *k < entries.len() && !blank(*k)).unwrap_or(first)
}

/// The chosen line once the window moved to `first` (the wheel, the scroll bar): itself when
/// still drawn, else the drawn line nearest to it the keys can choose.
/// `shown`: the window's lines.
pub fn clamp_into_window(entries: &[ViewEntry], first: usize, selected: Option<RowId>, shown: usize) -> Option<RowId> {
    let drawn = drawn_slots(entries, first, shown);
    let at = selected.and_then(|s| slot_of(entries, s));
    if at.is_some_and(|k| drawn.contains(&k)) {
        return selected;
    }
    let shown: Vec<(usize, RowId)> = drawn.filter_map(|k| Some((k, entries[k].selectable()?))).collect();
    if shown.is_empty() {
        // A window of one line on an empty line: no line to move the choice to.
        return selected;
    }
    match at {
        // Above the window: its top line; below it: its bottom line.
        Some(k) if shown.first().is_some_and(|(f, _)| k < *f) => shown.first().map(|r| r.1),
        Some(_) => shown.last().map(|r| r.1),
        None => shown.first().map(|r| r.1),
    }
}

/// The settings page's state made whole for `layout`: on a tab, the tab of the chosen line
/// (a test aid or a search may choose one on another tab); the chosen line itself when
/// listed, else the line that stands for it (hidden, [`parent_of`]; a Controls row of the
/// other device, that row of the device in use; a row of a closed category, its heading),
/// else the first row; and the lines listed.
pub fn normalize(layout: &ResolvedLayout, list: &[Setting], scope: SettingsScope, selected: Option<RowId>, vis: &Visibility, found: &Found, folding: &Folding) -> (SettingsScope, Option<RowId>, Vec<ViewEntry>) {
    let scope = match (scope, selected) {
        (SettingsScope::Tab(_), Some(id)) => SettingsScope::Tab(home_tab(layout, id)),
        (SettingsScope::Tab(t), None) if layout.tab(t).is_none() => SettingsScope::default(),
        (s, _) => s,
    };
    let entries = project(layout, list, scope, vis, found, folding);
    let stand_in = selected.and_then(|id| match id {
        RowId::Setting(i) => parent_of(list, i, vis),
        RowId::Control(ControlRow::Binding(c, _)) => Some(RowId::Control(ControlRow::Binding(c, vis.device))),
        RowId::Control(ControlRow::Sensitivity(_)) => Some(RowId::Control(ControlRow::Sensitivity(vis.device))),
        RowId::Control(ControlRow::Vertical(_)) => Some(RowId::Control(ControlRow::Vertical(vis.device))),
        RowId::Control(ControlRow::AimSensitivity(_)) => Some(RowId::Control(ControlRow::AimSensitivity(vis.device))),
        RowId::Control(ControlRow::Rumble) => Some(RowId::Control(ControlRow::Device)),
        RowId::Control(_) | RowId::Heading(_) | RowId::Group => None,
    });
    let listed = |id: &RowId| slot_of(&entries, *id).is_some();
    // A row whose category is closed: its heading.
    let closed = |id: RowId| match id {
        RowId::Setting(i) => layout.placement_of(i).map(RowId::Heading),
        RowId::Control(c) => layout.control_placement(&c.table_name()).map(RowId::Heading),
        RowId::Heading(_) | RowId::Group => None,
    };
    let selected = selected
        .filter(listed)
        .or(stand_in.filter(listed))
        .or_else(|| stand_in.or(selected).and_then(closed).filter(listed))
        .or_else(|| first_row(&entries));
    (scope, selected, entries)
}

/// The row that shows `id` where every row is listed with `vis` (the search, the Changed
/// list): itself when shown, else the row that stands for it ([`parent_of`]; a control's
/// binding or the sensitivity of the device not in use, and Rumble without a controller:
/// the Input Device row, which switches the device) [design: #1953]; `None`: neither.
pub fn shown_as(list: &[Setting], id: RowId, vis: &Visibility) -> Option<RowId> {
    match id {
        RowId::Setting(i) if list.get(i).is_some_and(|s| vis.shows(i, s)) => Some(id),
        RowId::Setting(i) => parent_of(list, i, vis).and_then(|p| match p {
            // A row standing for a row its own module hides now (a chain): as that one shows.
            RowId::Setting(j) if j != i => shown_as(list, p, vis),
            _ => Some(p),
        }),
        RowId::Control(ControlRow::Binding(_, d) | ControlRow::Sensitivity(d) | ControlRow::Vertical(d) | ControlRow::AimSensitivity(d)) if d != vis.device => Some(RowId::Control(ControlRow::Device)),
        RowId::Control(ControlRow::Rumble) if vis.device != InputDevice::Controller => Some(RowId::Control(ControlRow::Device)),
        RowId::Control(_) | RowId::Heading(_) | RowId::Group => Some(id),
    }
}

/// `ids` each as it is shown ([`shown_as`]), each once, in the page's order of the rows
/// shown ([`catalogue`]).
fn shown_rows(layout: &ResolvedLayout, list: &[Setting], ids: &[RowId], vis: &Visibility) -> Vec<RowId> {
    let mut out: Vec<RowId> = Vec::new();
    for id in ids.iter().filter_map(|id| shown_as(list, *id, vis)) {
        if !out.contains(&id) {
            out.push(id);
        }
    }
    let order = catalogue(layout);
    // Each row's place looked up once (an empty search lists every row, each frame).
    out.sort_by_cached_key(|id| order.iter().position(|o| o == id).unwrap_or(usize::MAX));
    out
}

/// Of `ids` (the search's or the Changed list's rows), those listed as another row
/// ([`shown_as`]), by the row that stands for them, in their order: what the page names
/// under that row so it is plain what a choice there is about [design: #1953].
pub fn stands_for(list: &[Setting], ids: &[RowId], vis: &Visibility) -> Vec<(RowId, Vec<RowId>)> {
    let mut out: Vec<(RowId, Vec<RowId>)> = Vec::new();
    for &id in ids {
        let Some(by) = shown_as(list, id, vis).filter(|by| *by != id) else { continue };
        match out.iter_mut().find(|(b, _)| *b == by) {
            Some((_, v)) => v.push(id),
            None => out.push((by, vec![id])),
        }
    }
    out
}

/// Where row `id` lives: a setting's place by the table, a Controls row's by the table's
/// Controls categories, a heading's its own.
pub fn placement(layout: &ResolvedLayout, id: RowId) -> Placement {
    match id {
        RowId::Setting(i) => layout.placement_of(i).unwrap_or(Placement::tab(TAB_ORDER[0])),
        RowId::Control(c) => layout.control_placement(&c.table_name()).unwrap_or(Placement::tab(CONTROLS_TAB)),
        RowId::Heading(p) => p,
        RowId::Group => Placement::tab(TWEAKS_TAB),
    }
}

/// Every row in the page's order: tab by tab, category by category, the Controls rows as the
/// table places them (a device's row and the other device's after it) [design: #1953, the
/// overhaul].
pub fn catalogue(layout: &ResolvedLayout) -> Vec<RowId> {
    let mut out = Vec::new();
    for t in &layout.tabs {
        for s in &t.sections {
            for item in &s.items {
                match item {
                    LayoutItem::Setting(i) => out.push(RowId::Setting(*i)),
                    LayoutItem::Control(name) => out.extend(ControlRow::of_table_all(name).into_iter().map(RowId::Control)),
                }
            }
        }
    }
    out
}

/// Rows of several tabs as lines: before each run of rows that live in one place, a crumb
/// saying where (its tab and category: `Gameplay > Dialogue`) [design: #1953], an empty line
/// between the runs.
pub fn listing(layout: &ResolvedLayout, ids: &[RowId]) -> Vec<ViewEntry> {
    listing_folded(layout, ids, |_| true)
}

/// [`listing`] with the places `open` says are closed listed by their crumb alone.
pub fn listing_folded(layout: &ResolvedLayout, ids: &[RowId], open: impl Fn(&Placement) -> bool) -> Vec<ViewEntry> {
    let mut out = Vec::new();
    let mut at: Option<Placement> = None;
    for &id in ids {
        let p = placement(layout, id);
        if at != Some(p) {
            // An empty line between places, as after an open category [design: the overhaul].
            if at.is_some() {
                out.push(ViewEntry::Gap);
            }
            out.push(ViewEntry::Crumb(p));
            at = Some(p);
        }
        if open(&p) {
            out.push(ViewEntry::Row(id));
        }
    }
    out
}

/// How many of the search's or the Changed list's rows (`ids`, as shown with `vis`) live at
/// each place (what its crumb counts), the places in the page's order: the rows worked out
/// once for every crumb.
pub fn place_counts(layout: &ResolvedLayout, list: &[Setting], ids: &[RowId], vis: &Visibility) -> Vec<(Placement, usize)> {
    let mut out: Vec<(Placement, usize)> = Vec::new();
    for id in shown_rows(layout, list, ids, vis) {
        let p = placement(layout, id);
        match out.iter_mut().find(|(q, _)| *q == p) {
            Some((_, n)) => *n += 1,
            None => out.push((p, 1)),
        }
    }
    out
}

/// The query's terms: split at whitespace, lowercased (Unicode) [design: #1953].
pub fn search_terms(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_lowercase).collect()
}

/// A row's words to search, lowercased, a line each so no term spans two.
pub fn haystack(parts: &[&str]) -> String {
    parts.join("\n").to_lowercase()
}

/// A registered row's words: its label and note in the language shown and in English, its
/// key as the INI keeps it (a tweak's note names its Stewie's Tweaks key) [design: #1953]
/// and its command-line switches (`--msaa` finds Anti-aliasing, #2377).
pub fn setting_haystack(s: &Setting) -> String {
    haystack(&[tr(s.label), s.label, tr(s.note), s.note, s.key, s.switches])
}

/// Whether `hay` holds every term somewhere (terms may match different words; no
/// stemming, no ranking) [design: #1953].
pub fn matches(hay: &str, terms: &[String]) -> bool {
    terms.iter().all(|t| hay.contains(t.as_str()))
}

/// The rows the search finds for `query`, in the page's order ([`catalogue`]): every row
/// whose words ([`setting_haystack`]; a Controls row's from `control_words`) hold each
/// term; an empty query, every row. Hidden rows are found too; [`project`] lists them as
/// the row that stands for them.
pub fn search(layout: &ResolvedLayout, list: &[Setting], query: &str, control_words: impl Fn(ControlRow) -> String) -> Vec<RowId> {
    let terms = search_terms(query);
    catalogue(layout)
        .into_iter()
        .filter(|id| {
            terms.is_empty()
                || match id {
                    RowId::Setting(i) => list.get(*i).is_some_and(|s| matches(&setting_haystack(s), &terms)),
                    RowId::Control(c) => matches(&control_words(*c).to_lowercase(), &terms),
                    RowId::Heading(_) | RowId::Group => false,
                }
        })
        .collect()
}

/// The rows away from their default, in the page's order: the registered rows `settings`
/// names (indices, `crate::settings::changed_rows`) and the Controls rows `control_changed`
/// says so of (`crate::bindings::is_default`) [design: #1953].
pub fn changed(layout: &ResolvedLayout, settings: &[usize], control_changed: impl Fn(ControlRow) -> bool) -> Vec<RowId> {
    catalogue(layout)
        .into_iter()
        .filter(|id| match id {
            RowId::Setting(i) => settings.contains(i),
            RowId::Control(c) => control_changed(*c),
            RowId::Heading(_) | RowId::Group => false,
        })
        .collect()
}

/// The choice once a list lost the chosen row (a Changed row put back): the first row after
/// it in `before` still listed `now`, else the nearest before it; else as chosen.
pub fn reselect(before: &[ViewEntry], now: &[ViewEntry], selected: Option<RowId>) -> Option<RowId> {
    let id = selected?;
    if slot_of(now, id).is_some() {
        return selected;
    }
    let old = rows(before);
    let Some(k) = old.iter().position(|r| *r == id) else { return selected };
    let new = rows(now);
    old[k + 1..].iter().find(|r| new.contains(r)).or_else(|| old[..k].iter().rev().find(|r| new.contains(r))).copied().or(selected)
}

/// The rows their module hides now (`crate::settings::Settings::hidden`), worked out each
/// frame before the page reads its keys ([`update_hidden_rows`]), for the systems that do not
/// hold the world ([`Visibility::with_hidden`]).
#[derive(Resource, Default, Debug, Clone, PartialEq, Eq)]
pub struct HiddenRows(pub Vec<(usize, usize)>);

/// Work out [`HiddenRows`] from the world.
pub fn update_hidden_rows(world: &mut World) {
    let hidden = world.get_resource::<crate::settings::Settings>().map(|s| s.hidden(world)).unwrap_or_default();
    if world.get_resource::<HiddenRows>().is_none_or(|h| h.0 != hidden) {
        world.insert_resource(HiddenRows(hidden));
    }
}

/// The table applied to the registry, kept until a row is added ([`resolve_layout`]).
#[derive(Resource, Default)]
pub struct SettingsLayout {
    rows: Option<usize>,
    pub layout: ResolvedLayout,
}

impl SettingsLayout {
    /// The layout of `list`, worked out again when the registry grew.
    pub fn refresh(&mut self, list: &[Setting]) -> &ResolvedLayout {
        if self.rows != Some(list.len()) {
            self.layout = resolve_layout(list);
            self.rows = Some(list.len());
        }
        &self.layout
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    // Shared page cases describe the desktop profile; Android grouping has its own controls.
    use crate::settings_layout::{resolve_desktop_layout as resolve_layout, DESKTOP_TAB_ORDER as TAB_ORDER};

    /// The owner's fifth build note: places on the search and the Changed list close to
    /// their crumb (which the keys can choose), for the run only, apart from the tabs'.
    #[test]
    fn a_place_on_the_search_closes_to_its_crumb() {
        let layout = ResolvedLayout::default();
        let ids = [RowId::Setting(0), RowId::Setting(1)];
        let p = placement(&layout, ids[0]);
        let mut f = Folding::default();
        assert_eq!(listing_folded(&layout, &ids, |q| f.crumb_open(q)), [ViewEntry::Crumb(p), ViewEntry::Row(ids[0]), ViewEntry::Row(ids[1])]);
        assert!(f.put_crumb(&p, false) && !f.put_crumb(&p, false));
        assert_eq!(listing_folded(&layout, &ids, |q| f.crumb_open(q)), [ViewEntry::Crumb(p)]);
        assert_eq!(ViewEntry::Crumb(p).selectable(), Some(RowId::Heading(p)));
        assert!(f.is_open(&p) && f.to_ini().is_empty(), "the tab's category and the INI are untouched");
        assert!(f.put_crumb(&p, true) && f.crumbs.is_empty());
    }

    /// A closed place's crumb is a line like a closed heading: drawn on a window's last line,
    /// a stop of Page Up and Page Down, the choice once its rows close, and counted once per
    /// place.
    #[test]
    fn a_closed_crumb_is_drawn_stepped_to_and_chosen_as_a_closed_heading_is() {
        let list = worded(&WORDED);
        let l = resolve_layout(&list);
        let k = |n| key(&list, n);
        let vis = Visibility::default();
        let found = Found { search: Vec::new(), changed: vec![k("difficulty"), k("hud-colour"), k("god-mode")] };
        let (game, colours) = (heading(&list, &l, "difficulty"), heading(&list, &l, "hud-colour"));
        let mut f = open();
        f.put_crumb(&colours, false);
        let e = project(&l, &list, SettingsScope::Changed, &vis, &found, &f);
        assert_eq!(names(&list, &e), ["<Game>", "difficulty", "", "<Colours>", "", "<Debug>", "god-mode"]);
        // On the last line of a window: drawn, as the keys can choose it.
        assert_eq!(drawn_slots(&e, 0, 4), 0..4);
        let first = follow(&e, 0, slot_of(&e, RowId::Heading(colours)), 4);
        assert!(drawn_slots(&e, first, 4).contains(&3), "{first}");
        // Page Down from the row above stops on it, and from it goes on to the next place.
        assert_eq!(step_heading(&e, Some(k("difficulty")), 1), Some((RowId::Heading(colours), Some(colours))));
        assert_eq!(step_heading(&e, Some(RowId::Heading(colours)), 1).map(|(id, _)| id), Some(k("god-mode")));
        assert_eq!(step_heading(&e, Some(k("god-mode")), -1).map(|(id, _)| id), Some(RowId::Heading(colours)));
        // A row whose place closes: its crumb is chosen.
        f.put_crumb(&game, false);
        let (_, sel, _) = normalize(&l, &list, SettingsScope::Changed, Some(k("difficulty")), &vis, &found, &f);
        assert_eq!(sel, Some(RowId::Heading(game)));
        // Each place's rows, as listed.
        assert_eq!(place_counts(&l, &list, &found.changed, &vis), [(game, 1), (colours, 1), (heading(&list, &l, "god-mode"), 1)]);
    }

    const NONE: Found = Found { search: Vec::new(), changed: Vec::new() };

    fn open() -> Folding {
        Folding::default()
    }

    fn registry(rows: &[(&'static str, &'static str)]) -> Vec<Setting> {
        let mut s = Settings::default();
        for (key, group) in rows {
            s.add(Setting { key, group, label: key, note: "", switches: "", get: |_| false, set: |_, _| {}, slider: None });
        }
        s.list().to_vec()
    }

    const ROWS: [(&str, &str); 12] = [
        ("difficulty", "Gameplay"),
        ("hardcore", "Gameplay"),
        ("menu-pause", "Gameplay"),
        ("field-of-view", "Display"),
        ("autosave-travel", "Saving"),
        ("autosave-wait", "Saving"),
        ("hud-colour", "Interface"),
        ("hud-red", "Interface"),
        ("hud-green", "Interface"),
        ("hud-blue", "Interface"),
        ("button-style", "Controller"),
        ("god-mode", "Debug"),
    ];

    fn key(list: &[Setting], k: &str) -> RowId {
        RowId::Setting(list.iter().position(|s| s.key == k).unwrap())
    }

    fn names(list: &[Setting], entries: &[ViewEntry]) -> Vec<String> {
        entries
            .iter()
            .map(|e| match e {
                ViewEntry::Heading(p) => format!("[{}]", p.heading.unwrap_or(p.tab)),
                ViewEntry::Crumb(p) => format!("<{}>", p.heading.unwrap_or(p.tab)),
                ViewEntry::Gap => String::new(),
                ViewEntry::Row(id) => id.name(list),
            })
            .collect()
    }

    fn heading(list: &[Setting], l: &ResolvedLayout, k: &str) -> Placement {
        l.placement_of(list.iter().position(|s| s.key == k).unwrap()).unwrap()
    }

    #[test]
    fn a_tab_lists_its_categories_with_an_empty_line_after_each_open_one() {
        let list = registry(&ROWS);
        let l = resolve_layout(&list);
        let vis = Visibility::default();
        let g = project(&l, &list, SettingsScope::Tab("Gameplay"), &vis, &NONE, &open());
        assert_eq!(names(&list, &g), ["[Game]", "difficulty", "hardcore", "", "[Camera]", "field-of-view", "", "[Dialogue]", "menu-pause", "", "[Saving]", "autosave-travel", "autosave-wait"]);
        // A closed category: its heading alone, no empty line after it.
        let mut f = open();
        assert!(f.put(&heading(&list, &l, "field-of-view"), false));
        assert!(!f.put(&heading(&list, &l, "field-of-view"), false), "already closed");
        let g = project(&l, &list, SettingsScope::Tab("Gameplay"), &vis, &NONE, &f);
        assert_eq!(names(&list, &g)[..6], ["[Game]", "difficulty", "hardcore", "", "[Camera]", "[Dialogue]"]);
        assert_eq!(category_count(&l, &list, &heading(&list, &l, "field-of-view"), &vis), 1);
        // The colour's sliders only for Custom, the HUD's alone.
        let d = |colours| names(&list, &project(&l, &list, SettingsScope::Tab("Display"), &Visibility { colours, ..Visibility::default() }, &NONE, &open()));
        assert_eq!(d([false, false]), ["[Colours]", "hud-colour"]);
        assert_eq!(d([true, false]), ["[Colours]", "hud-colour", "hud-red", "hud-green", "hud-blue"]);
        // A row its module hides now.
        let hidden = Visibility { hidden: vec![(list.iter().position(|s| s.key == "hardcore").unwrap(), 0)], ..Visibility::default() };
        assert_eq!(names(&list, &project(&l, &list, SettingsScope::Tab("Gameplay"), &hidden, &NONE, &open()))[..3], ["[Game]", "difficulty", ""]);
        // The Controls tab: its own rows in the table's categories, the controller's with a controller only.
        let c = |device| names(&list, &project(&l, &list, SettingsScope::Tab(CONTROLS_TAB), &Visibility { device, ..Visibility::default() }, &NONE, &open()));
        let kb = c(InputDevice::KeyboardMouse);
        assert_eq!(kb[..10], ["[Input]", "device", "automatic-input", "", "[Look]", "invert", "sensitivity", "vertical", "aim-sensitivity", ""]);
        assert_eq!(kb[10..12], ["[Movement]", "bind:Forward"]);
        assert_eq!(kb.iter().filter(|n| n.starts_with("bind:")).count(), GAME_CONTROLS.len(), "every control once");
        let pad = c(InputDevice::Controller);
        assert_eq!(pad[..6], ["[Input]", "device", "button-style", "automatic-input", "rumble", ""]);
        assert!(pad.iter().any(|n| n == "padbind:Jump"));
        // An empty tab, and a search that found nothing: nothing.
        assert!(project(&l, &list, SettingsScope::Tab("Tweaks"), &vis, &NONE, &open()).is_empty());
        assert!(project(&l, &list, SettingsScope::Search, &vis, &NONE, &open()).is_empty());
    }

    #[test]
    fn the_folding_keeps_only_what_differs_from_the_default() {
        let game = Placement { tab: "Gameplay", subgroup: None, heading: Some("Game") };
        let tweak = Placement { tab: TWEAKS_TAB, subgroup: Some("Gameplay"), heading: Some("Weight") };
        let mut f = Folding::default();
        assert!(f.is_open(&game) && !f.is_open(&tweak), "open by default; the Tweaks tab's closed");
        f.put(&game, false);
        f.put(&tweak, true);
        assert_eq!(f.to_ini(), "-Gameplay > Game;+Tweaks > Gameplay > Weight");
        assert_eq!(Folding::from_ini(&f.to_ini()), f.set);
        f.put(&game, true);
        assert_eq!(f.to_ini(), "+Tweaks > Gameplay > Weight", "back to the default: not kept");
        assert_eq!(Folding::from_ini(" junk ; -A > B ;+"), [("A > B".to_string(), false)]);
        assert_eq!(f.group_of(&["Gameplay", "Combat"]), Some("Gameplay"));
        f.group = Some("Combat".into());
        assert_eq!(f.group_of(&["Gameplay", "Combat"]), Some("Combat"));
        assert_eq!(f.group_of(&[]), None);
    }

    #[test]
    fn up_and_down_take_the_headings_and_pass_the_empty_lines_round_the_ends() {
        let list = registry(&ROWS);
        let l = resolve_layout(&list);
        let g = project(&l, &list, SettingsScope::Tab("Gameplay"), &Visibility::default(), &NONE, &open());
        let k = |n| key(&list, n);
        let camera = RowId::Heading(heading(&list, &l, "field-of-view"));
        assert_eq!(step_selectable(&g, Some(k("hardcore")), 1), Some(camera), "over the empty line onto the heading");
        assert_eq!(step_selectable(&g, Some(camera), 1), Some(k("field-of-view")));
        assert_eq!(step_selectable(&g, Some(camera), -1), Some(k("hardcore")));
        assert_eq!(step_selectable(&g, Some(k("autosave-wait")), 1), Some(RowId::Heading(heading(&list, &l, "difficulty"))), "round the end");
        assert_eq!(step_selectable(&g, None, 1), Some(RowId::Heading(heading(&list, &l, "difficulty"))));
        assert_eq!(step_selectable(&[], None, 1), None, "an empty tab");
        // Nothing chosen: the first row, not its heading.
        assert_eq!(first_row(&g), Some(k("difficulty")));
        let closed = [ViewEntry::Heading(heading(&list, &l, "difficulty")), ViewEntry::Heading(heading(&list, &l, "field-of-view"))];
        assert_eq!(first_row(&closed), Some(RowId::Heading(heading(&list, &l, "difficulty"))), "only closed categories: the first heading");
    }

    #[test]
    fn page_up_and_down_go_from_category_to_category() {
        let list = registry(&ROWS);
        let l = resolve_layout(&list);
        let g = project(&l, &list, SettingsScope::Tab("Gameplay"), &Visibility::default(), &NONE, &open());
        let k = |n| key(&list, n);
        let to = |e: &[ViewEntry], from: RowId, step| step_heading(e, Some(from), step).map(|(id, p)| (id.name(&list), p.and_then(|p| p.heading)));
        assert_eq!(to(&g, k("difficulty"), 1), Some(("field-of-view".into(), Some("Camera"))));
        assert_eq!(to(&g, k("field-of-view"), 1), Some(("menu-pause".into(), Some("Dialogue"))));
        assert_eq!(to(&g, k("menu-pause"), -1), Some(("field-of-view".into(), Some("Camera"))));
        assert_eq!(to(&g, k("autosave-wait"), 1), Some(("difficulty".into(), Some("Game"))), "round the end");
        assert_eq!(to(&g, RowId::Heading(heading(&list, &l, "field-of-view")), 1), Some(("menu-pause".into(), Some("Dialogue"))), "from a heading too");
        // A closed category: its heading.
        let mut f = open();
        f.put(&heading(&list, &l, "field-of-view"), false);
        let closed = project(&l, &list, SettingsScope::Tab("Gameplay"), &Visibility::default(), &NONE, &f);
        assert_eq!(to(&closed, k("difficulty"), 1), Some(("[Gameplay > Camera]".into(), Some("Camera"))));
        // A tab without headings is one section: its first row; an empty one, nothing.
        let debug = project(&l, &list, SettingsScope::Tab("Debug"), &Visibility::default(), &NONE, &open());
        assert_eq!(step_heading(&debug, Some(k("god-mode")), 1), Some((k("god-mode"), None)));
        assert_eq!(step_heading(&[], None, 1), None);
        assert_eq!(heading_of(&g, k("hardcore")).and_then(|p| p.heading), Some("Game"));
    }

    #[test]
    fn the_window_keeps_the_choice_and_its_heading_and_never_ends_on_a_heading() {
        let h = |t| ViewEntry::Heading(Placement { tab: "T", subgroup: None, heading: Some(t) });
        let r = |i| ViewEntry::Row(RowId::Setting(i));
        // [A] 0..7, gap, [B] 7..12: the heading B is on line 10.
        let mut e = vec![h("A")];
        e.extend((0..8).map(r));
        e.push(ViewEntry::Gap);
        e.push(h("B"));
        e.extend((8..12).map(r));
        assert_eq!(e.len(), 15);
        // A window from 1 of 10 would end on B (line 10) and the gap before it: both left out.
        assert_eq!(drawn_slots(&e, 1, 10), 1..9);
        assert_eq!(drawn_slots(&e, 0, 9), 0..9);
        // Down onto B's first row (line 11): the window shows B too.
        let first = follow(&e, 0, Some(11), 9);
        assert!(first <= 10 && first + 9 > 11, "{first}");
        // Onto the heading B itself: its first row comes with it.
        let first = follow(&e, 0, Some(10), 9);
        assert!(drawn_slots(&e, first, 9).contains(&10), "{first}");
        // Up to A's first row (line 1): A's heading comes back into view.
        assert_eq!(follow(&e, 5, Some(1), 9), 0);
        // Never past the last full window.
        assert_eq!(follow(&e, 20, None, 9), e.len() - 9);
        // The wheel moved the window off the choice: the nearest drawn line is chosen.
        assert_eq!(clamp_into_window(&e, 3, Some(RowId::Setting(0)), 9), Some(RowId::Setting(2)));
        assert_eq!(clamp_into_window(&e, 0, Some(RowId::Setting(11)), 9), Some(RowId::Setting(7)));
        assert_eq!(clamp_into_window(&e, 0, Some(RowId::Setting(3)), 9), Some(RowId::Setting(3)));
    }

    /// #2377: the window works with any number of lines, the layout's: every line can be
    /// followed into view (a row's heading too where there is room), the window never passes
    /// the last full one, never ends on a heading over rows or an empty line unless that is
    /// its only line, and the wheel's window always has the choice drawn.
    #[test]
    fn the_window_works_with_any_number_of_lines() {
        let h = |t| ViewEntry::Heading(Placement { tab: "T", subgroup: None, heading: Some(t) });
        let c = |t| ViewEntry::Crumb(Placement { tab: "T", subgroup: None, heading: Some(t) });
        let r = |i| ViewEntry::Row(RowId::Setting(i));
        // [A] 0..5, gap, [B] 5..10, gap, [C] closed, [D] 10..15.
        let mut e = Vec::new();
        for (k, name) in ["A", "B"].into_iter().enumerate() {
            e.push(h(name));
            e.extend((k * 5..k * 5 + 5).map(r));
            e.push(ViewEntry::Gap);
        }
        e.push(h("C"));
        e.push(h("D"));
        e.extend((10..15).map(r));
        let short = vec![h("A"), r(0), r(1)];
        let crumbs = vec![c("A"), r(0), r(1), c("B"), r(2)];
        let rows_only: Vec<ViewEntry> = (0..4).map(r).collect();
        for shown in [1, 2, 9, 13, 30] {
            for list in [&e, &short, &crumbs, &rows_only] {
                let n = list.len();
                let last = n.saturating_sub(shown);
                for (s, entry) in list.iter().enumerate() {
                    let Some(id) = entry.selectable() else { continue };
                    for first in 0..n + 3 {
                        let f = follow(list, first, Some(s), shown);
                        let drawn = drawn_slots(list, f, shown);
                        assert!(f <= last, "{shown} lines: {f} past the last full window {last}");
                        assert!(drawn.contains(&s), "{shown} lines from {first}: line {s} not drawn in {drawn:?}");
                        if shown > 1 && s > 0 && list[s - 1].titles() && matches!(entry, ViewEntry::Row(_)) {
                            assert!(drawn.contains(&(s - 1)), "{shown} lines: the heading over the category's first row {s}");
                        }
                        assert_eq!(window_first(list, first, Some(id), None, shown), f);
                        let kept = window_first(list, first, Some(id), Some(id), shown);
                        assert!(drawn_slots(list, kept, shown).contains(&s) && kept <= last, "{shown} lines from {first}: kept {kept} loses {s}");
                    }
                }
                for first in 0..=last {
                    let drawn = drawn_slots(list, first, shown);
                    assert!(drawn.len() <= shown && drawn.start == first.min(n));
                    if drawn.len() > 1 && list[drawn.start..drawn.end - 1].iter().any(|e| e.selectable().is_some()) {
                        let end = drawn.end - 1;
                        assert!(list[end] != ViewEntry::Gap && !titles_rows(list, end), "{shown} lines from {first}: ends on a title or a gap");
                    }
                    for step in [-1, 1] {
                        let landed = land(list, first, step, shown);
                        let picked = clamp_into_window(list, landed, Some(RowId::Setting(0)), shown);
                        let k = picked.and_then(|id| slot_of(list, id)).expect("a line");
                        assert!(drawn_slots(list, landed, shown).contains(&k), "{shown} lines from {first} ({step}): {picked:?}");
                    }
                }
            }
        }
        // One line: the row, not its heading; two: both.
        assert_eq!(follow(&e, 0, Some(7), 1), 7);
        assert_eq!(follow(&e, 0, Some(8), 2), 7);
        // The wheel passes an empty line in a one-line window.
        assert_eq!(land(&e, 6, 1, 1), 7);
        assert_eq!(land(&e, 6, -1, 1), 5);
        // An empty list.
        assert_eq!(follow(&[], 3, None, 13), 0);
        assert_eq!(drawn_slots(&[], 0, 13), 0..0);
        assert_eq!(clamp_into_window(&[], 0, None, 13), None);
        assert_eq!(window_first(&[], 2, None, None, 13), 0);
        // A zero is taken as one line, never a window of none.
        assert_eq!(drawn_slots(&e, 1, 0), 1..2);
    }

    #[test]
    fn a_hidden_or_missing_choice_moves_to_the_line_that_stands_for_it() {
        let list = registry(&ROWS);
        let l = resolve_layout(&list);
        let k = |n| key(&list, n);
        let vis = Visibility::default();
        // A test aid chooses a hidden slider on another tab: its colour, on its tab.
        let (scope, sel, _) = normalize(&l, &list, SettingsScope::Tab("Gameplay"), Some(k("hud-red")), &vis, &NONE, &open());
        assert_eq!((scope, sel), (SettingsScope::Tab("Display"), Some(k("hud-colour"))));
        // Shown: itself.
        let (_, sel, _) = normalize(&l, &list, SettingsScope::Tab("Display"), Some(k("hud-red")), &Visibility { colours: [true, false], ..Visibility::default() }, &NONE, &open());
        assert_eq!(sel, Some(k("hud-red")));
        // A row its module hides: the row it names.
        let RowId::Setting(hard) = k("hardcore") else { unreachable!() };
        let RowId::Setting(diff) = k("difficulty") else { unreachable!() };
        let hidden = Visibility { hidden: vec![(hard, diff)], ..Visibility::default() };
        let (_, sel, _) = normalize(&l, &list, SettingsScope::Tab("Gameplay"), Some(k("hardcore")), &hidden, &NONE, &open());
        assert_eq!(sel, Some(k("difficulty")));
        assert_eq!(shown_as(&list, k("hardcore"), &hidden), Some(k("difficulty")));
        // A controller's row with keyboard and mouse: the Input Device row.
        let (scope, sel, _) = normalize(&l, &list, SettingsScope::Tab("Gameplay"), Some(k("button-style")), &vis, &NONE, &open());
        assert_eq!((scope, sel), (SettingsScope::Tab(CONTROLS_TAB), Some(RowId::Control(ControlRow::Device))));
        // A control's controller button once the keyboard is chosen: its key.
        let jump = RowId::Control(ControlRow::Binding(Control::Jump, InputDevice::Controller));
        let (_, sel, _) = normalize(&l, &list, SettingsScope::Tab(CONTROLS_TAB), Some(jump), &vis, &NONE, &open());
        assert_eq!(sel, Some(RowId::Control(ControlRow::Binding(Control::Jump, InputDevice::KeyboardMouse))));
        // A row of a closed category: its heading.
        let mut f = open();
        f.put(&heading(&list, &l, "menu-pause"), false);
        let (_, sel, _) = normalize(&l, &list, SettingsScope::Tab("Gameplay"), Some(k("menu-pause")), &vis, &NONE, &f);
        assert_eq!(sel, Some(RowId::Heading(heading(&list, &l, "menu-pause"))));
        let movement = l.control_placement("bind:Jump").unwrap();
        f.put(&movement, false);
        let (_, sel, _) = normalize(&l, &list, SettingsScope::Tab(CONTROLS_TAB), Some(jump), &vis, &NONE, &f);
        assert_eq!(sel, Some(RowId::Heading(movement)));
        // Nothing chosen: the first row; an empty tab keeps none.
        let (_, sel, _) = normalize(&l, &list, SettingsScope::Tab("Gameplay"), None, &vis, &NONE, &open());
        assert_eq!(sel, Some(k("difficulty")));
        let (scope, sel, entries) = normalize(&l, &list, SettingsScope::Tab("Tweaks"), None, &vis, &NONE, &open());
        assert_eq!((scope, sel, entries.len()), (SettingsScope::Tab("Tweaks"), None, 0));
        assert_eq!(parent_of(&list, list.iter().position(|s| s.key == "god-mode").unwrap(), &vis), None, "shown: none");
    }

    #[test]
    fn each_line_has_a_name_and_a_sound_of_its_own() {
        let list = registry(&ROWS);
        let l = resolve_layout(&list);
        let headings: Vec<RowId> = l.tabs.iter().flat_map(|t| &t.sections).filter(|s| s.placement.heading.is_some()).map(|s| RowId::Heading(s.placement)).collect();
        let all: Vec<RowId> = ControlRow::rows(InputDevice::Controller).into_iter().map(RowId::Control).chain((0..list.len()).map(RowId::Setting)).chain(headings).chain([RowId::Group]).collect();
        let mut sounds: Vec<usize> = all.iter().map(|r| r.sound_id()).collect();
        sounds.sort_unstable();
        sounds.dedup();
        assert_eq!(sounds.len(), all.len());
        assert!(!sounds.contains(&NO_ROW_SOUND));
        assert_eq!(RowId::Control(ControlRow::Binding(Control::Jump, InputDevice::KeyboardMouse)).name(&list), "bind:Jump");
        assert_eq!(RowId::Control(ControlRow::Binding(Control::Jump, InputDevice::Controller)).name(&list), "padbind:Jump");
        assert_eq!(RowId::Heading(heading(&list, &l, "difficulty")).name(&list), "[Gameplay > Game]");
        assert_eq!(key(&list, "god-mode").name(&list), "god-mode");
        assert_eq!(rail(&l).len(), 2 + TAB_ORDER.len() + 2);
        // Every Controls row has its place in the table.
        for c in ControlRow::all() {
            assert!(l.control_placement(&c.table_name()).is_some(), "{} has no place", c.name());
            assert_eq!(ControlRow::of_table(&c.table_name(), match c {
                ControlRow::Binding(_, d) | ControlRow::Sensitivity(d) | ControlRow::Vertical(d) | ControlRow::AimSensitivity(d) => d,
                _ => InputDevice::Controller,
            }), Some(c));
        }
    }

    /// A registry with the rows' own words: key, group, label, note, switches.
    fn worded(rows: &[(&'static str, &'static str, &'static str, &'static str, &'static str)]) -> Vec<Setting> {
        let mut s = Settings::default();
        for (key, group, label, note, switches) in rows {
            s.add(Setting { key, group, label, note, switches, get: |_| false, set: |_, _| {}, slider: None });
        }
        s.list().to_vec()
    }

    const WORDED: [(&str, &str, &str, &str, &str); 6] = [
        ("difficulty", "Gameplay", "Difficulty", "How hard the fights are.", ""),
        ("menu-pause", "Gameplay", "Freeze the Picture in Dialogue and Menus", "While the inventory is open, animations stand still; the world lives on otherwise.", "--menu-pause, --still-world"),
        ("hud-colour", "Interface", "HUD Colour", "The colour of the HUD.", ""),
        ("hud-red", "Interface", "HUD Red", "Its red.", ""),
        ("button-style", "Controller", "Button Style", "The controller's buttons.", ""),
        ("god-mode", "Debug", "God Mode", "Nothing hurts the player.", ""),
    ];

    /// What the search lists for `query` with `vis`, as names (a Controls row's words: its name).
    fn searched(list: &[Setting], query: &str, vis: &Visibility) -> Vec<String> {
        let l = resolve_layout(list);
        let found = Found { search: search(&l, list, query, |c| c.name()), changed: Vec::new() };
        names(list, &project(&l, list, SettingsScope::Search, vis, &found, &open()))
    }

    #[test]
    fn the_search_finds_rows_by_label_note_or_key_in_every_tab() {
        let list = worded(&WORDED);
        let vis = Visibility::default();
        // The INI key, a label's word, a note's words (each anywhere), any case and spacing.
        assert_eq!(searched(&list, "menu-pause", &vis), ["<Dialogue>", "menu-pause"]);
        assert_eq!(searched(&list, "FREEZE", &vis), ["<Dialogue>", "menu-pause"]);
        assert_eq!(searched(&list, "  inventory   animations ", &vis), ["<Dialogue>", "menu-pause"]);
        assert_eq!(searched(&list, "freeze lives", &vis), ["<Dialogue>", "menu-pause"], "terms in the label and the note");
        // A row's command-line switches, though the page does not show them (#2377).
        assert_eq!(searched(&list, "--Still-World", &vis), ["<Dialogue>", "menu-pause"]);
        assert!(searched(&list, "freeze nothing", &vis).is_empty(), "every term must be found");
        assert!(searched(&list, "zzz", &vis).is_empty());
        // Rows of several tabs, in the page's order, each under where it lives; every
        // category open on the search (a tab's closed one too), under crumbs, never headings.
        let l = resolve_layout(&list);
        let found = Found { search: search(&l, &list, "the", |_| String::new()), changed: Vec::new() };
        let mut closed = open();
        closed.put(&heading(&list, &l, "menu-pause"), false);
        let e = project(&l, &list, SettingsScope::Search, &vis, &found, &closed);
        let heads: Vec<String> = e.iter().filter_map(|e| if let ViewEntry::Crumb(p) = e { Some(p.path()) } else { None }).collect();
        assert_eq!(heads, ["Gameplay > Game", "Gameplay > Dialogue", "Display > Colours", "Controls > Input", "Debug"]);
        assert!(e.iter().all(|e| !matches!(e, ViewEntry::Heading(_))), "crumbs, never a tab's headings");
        // A hidden colour channel is found as its colour, once; a controller's row as the Input Device row.
        // (The tweaks' Hide HUD key (#1892) is found by its words too, its controller button as
        // the Input Device row.)
        assert_eq!(searched(&list, "hud", &vis), ["<Colours>", "hud-colour", "", "<Input>", "device", "", "<Additional Features>", "bind:Hide HUD"]);
        assert_eq!(searched(&list, "hud", &Visibility { colours: [true, false], ..Visibility::default() }), ["<Colours>", "hud-colour", "hud-red", "", "<Input>", "device", "", "<Additional Features>", "bind:Hide HUD"]);
        assert_eq!(searched(&list, "button-style", &vis), ["<Input>", "device"]);
        // A Controls row is found by its words; a control of the device not in use as the
        // Input Device row.
        assert_eq!(searched(&list, "bind:jump", &vis), ["<Input>", "device", "", "<Movement>", "bind:Jump"], "padbind:Jump holds the words too");
        assert_eq!(searched(&list, "padbind:jump", &vis), ["<Input>", "device"]);
        assert_eq!(searched(&list, "jump", &Visibility { device: InputDevice::Controller, ..Visibility::default() }), ["<Input>", "device", "", "<Movement>", "padbind:Jump"]);
        // An empty query: every row.
        let all = search(&l, &list, " ", |_| String::new());
        assert_eq!(all.len(), list.len() + ControlRow::all().len());
        assert_eq!(all, catalogue(&l));
        assert_eq!(search_terms(" Ab  CD "), ["ab", "cd"]);
        assert!(matches(&haystack(&["One", "two"]), &search_terms("TWO one")));
        assert!(!matches(&haystack(&["one", "two"]), &search_terms("etw")), "no term spans two words' lines");
    }

    #[test]
    fn the_changed_list_names_where_each_row_lives() {
        let list = worded(&WORDED);
        let l = resolve_layout(&list);
        let k = |n| key(&list, n);
        let RowId::Setting(god) = k("god-mode") else { unreachable!() };
        let RowId::Setting(red) = k("hud-red") else { unreachable!() };
        let RowId::Setting(diff) = k("difficulty") else { unreachable!() };
        let rows = changed(&l, &[god, red, diff], |c| c == ControlRow::Rumble);
        // In the page's order, hidden rows too.
        assert_eq!(rows, [k("difficulty"), k("hud-red"), RowId::Control(ControlRow::Rumble), k("god-mode")]);
        let found = Found { search: Vec::new(), changed: rows };
        let e = project(&l, &list, SettingsScope::Changed, &Visibility::default(), &found, &open());
        assert_eq!(names(&list, &e), ["<Game>", "difficulty", "", "<Colours>", "hud-colour", "", "<Input>", "device", "", "<Debug>", "god-mode"]);
        assert_eq!(heading_of(&e, k("hud-colour")).map(|p| p.path()).as_deref(), Some("Display > Colours"));
        // LB/RB go from place to place.
        assert_eq!(step_heading(&e, Some(k("difficulty")), 1).map(|(id, _)| id), Some(k("hud-colour")));
        assert!(project(&l, &list, SettingsScope::Changed, &Visibility::default(), &NONE, &open()).is_empty());
    }

    #[test]
    fn a_change_of_the_device_not_in_use_is_listed_as_the_input_device_row() {
        // Keyboard and mouse in use; only the controller's Jump button and the stick's
        // sensitivity changed: the Changed list shows the Input Device row (acting on it
        // switches the device), never the keyboard's Jump or the mouse's sensitivity.
        let list = worded(&WORDED);
        let l = resolve_layout(&list);
        let pad_jump = RowId::Control(ControlRow::Binding(Control::Jump, InputDevice::Controller));
        let stick = RowId::Control(ControlRow::Sensitivity(InputDevice::Controller));
        let rows = changed(&l, &[], |c| RowId::Control(c) == pad_jump || RowId::Control(c) == stick);
        assert_eq!(rows, [stick, pad_jump], "the rows themselves, in the page's order");
        let vis = Visibility::default();
        let found = Found { search: Vec::new(), changed: rows.clone() };
        let e = project(&l, &list, SettingsScope::Changed, &vis, &found, &open());
        assert_eq!(names(&list, &e), ["<Input>", "device"]);
        // What the Input Device row stands for, named under it.
        assert_eq!(stands_for(&list, &rows, &vis), [(RowId::Control(ControlRow::Device), vec![stick, pad_jump])]);
        // With the controller in use they are their own rows.
        let pad = Visibility { device: InputDevice::Controller, ..Visibility::default() };
        assert_eq!(names(&list, &project(&l, &list, SettingsScope::Changed, &pad, &found, &open())), ["<Look>", "sensitivity", "", "<Movement>", "padbind:Jump"]);
        assert!(stands_for(&list, &rows, &pad).is_empty());
    }

    #[test]
    fn a_row_put_back_gives_the_choice_to_the_row_after_it() {
        let r = |i| ViewEntry::Row(RowId::Setting(i));
        let h = ViewEntry::Crumb(Placement::tab("Debug"));
        let before = [h, r(1), r(2), r(3)];
        assert_eq!(reselect(&before, &[h, r(1), r(3)], Some(RowId::Setting(2))), Some(RowId::Setting(3)));
        assert_eq!(reselect(&before, &[h, r(1), r(2)], Some(RowId::Setting(3))), Some(RowId::Setting(2)), "the last: the one before");
        assert_eq!(reselect(&before, &[h, r(1), r(2), r(3)], Some(RowId::Setting(2))), Some(RowId::Setting(2)));
        assert_eq!(reselect(&before, &[], Some(RowId::Setting(2))), Some(RowId::Setting(2)), "none left: as chosen (the caller finds none listed)");
        assert_eq!(reselect(&before, &[h, r(1)], None), None);
    }
}
