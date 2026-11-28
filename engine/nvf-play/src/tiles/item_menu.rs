//! Shared container and barter view state [design: `container_menu`, `barter-menu.md` section 8].
//! Paths and click meanings belong to each adapter; row identity, measuring, scroll,
//! guards and alpha marks follow one implementation.

use super::layout::{HOTRECT_FILL, OWN_ALPHA};
use super::list::{self, Drag, ListView, RowTile, Wanted};
use super::menus::{SizesWritten, Tiled};
use crate::menu::look;
use crate::menu::model::{Category, MenuModel, Mode, Row, RowGuard, RowKey, Side};
use crate::pipboy::items::Info;
use bevy::prelude::Resource;
use fnv_ui::{TileId, Ui, Value};

/// [design] File names and paths only; no menu expressions are duplicated here.
pub struct Paths {
    pub menu: &'static str,
    pub lists: [&'static str; 2],
    pub titles: [&'static str; 2],
    pub template: &'static str,
    pub row: &'static str,
    pub row_text: &'static str,
    pub row_mark: &'static str,
    pub targets: &'static [(&'static str, MenuTile)],
}

pub const SIDES: [Side; 2] = [Side::Container, Side::Player];

/// [design] Check the paths and row children each adapter writes or reads.
pub fn built(ui: &Ui, layout: &Paths, paths: Vec<String>, children: &[(&str, &str)]) -> Tiled {
    for path in &paths {
        if ui.find_tile(path).is_none() {
            return Tiled::No(format!("{path} missing"));
        }
    }
    list::template_has(ui, layout.menu, layout.template, layout.row, children)
}

/// A row as shown.
#[derive(Clone, Debug, PartialEq)]
pub struct RowShown {
    pub key: RowKey,
    /// [observed: the tile's name, the item's without the count].
    pub name: String,
    pub text: String,
    /// The row carries the equipped mark: a thing worn or held, and the ammo the weapon
    /// loads now ([`Row::loaded`]; #3442: the list drawn from tiles left that out, so the
    /// Ammo tab marked nothing).
    pub equipped: bool,
    /// Ammo the weapon cannot fire: the row's text at half alpha ([`Row::dim`], #3442).
    pub dim: bool,
    /// Resolved ySI tag/icon/template for this row when ySI is active (`None` otherwise).
    pub ysi: Option<fnv_game::ui_ysi::YsiItemDisplay>,
}

/// A list as shown.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ListShown {
    pub rows: Vec<RowShown>,
    pub category: Category,
    /// The row with the highlight: only in the list with the focus.
    pub selected: Option<usize>,
    /// The rows the category leaves out [observed: `_num_filtered` 12 of 15 in AID, 15 of 16 in
    /// AMMO, 0 in ITEMS].
    pub filtered: usize,
}

/// One whole state of the model, as the menu shows it: a frame with the same writes nothing
/// but sizes and scroll. A plain snapshot, so tests need no world.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    /// By [`Side`].
    pub lists: [ListShown; 2],
    pub titles: [String; 2],
    pub weight: String,
    pub focus: Side,
    /// The model's selection was last moved by the pointer: the lists stay where they are
    /// [owner's playtest list 7, item 11].
    pub pointer_pick: bool,
    /// The "How many?" prompt is open. Nothing here draws it; it is a change of what the menu
    /// shows, so the log reads the lists again once the prompt has closed [design].
    pub asking: bool,
    /// [`MenuModel::navigated`].
    pub navigated: u32,
    /// The selected item's figures ([`look::item_figures`]) when there is one.
    pub card: Option<Info>,
    pub icon: Option<String>,
    /// Take All has something to take.
    pub take_all: bool,
    /// The container adapter's Take All hint.
    pub take_all_hint: String,
    pub note: Option<String>,
    /// Whether `apply_row_icon` should instantiate `ySIImage` row tiles (`yUI.ini`
    /// `[Sorting and Icons] bAddInventoryIcons`, gated by `bSortingIcons`) [design:
    /// `docs/spec-notes/yui-ini.md`].
    pub ysi_icons: bool,
}

/// The rows of a listing with their keys.
pub fn rows_shown(rows: &[Row]) -> Vec<RowShown> {
    let mut out: Vec<RowShown> = Vec::with_capacity(rows.len());
    for (index, r) in rows.iter().enumerate() {
        let Some(key) = RowKey::at(rows, index) else { continue };
        out.push(RowShown { key, name: r.name.clone(), text: look::row_text(r), equipped: r.equipped || r.loaded, dim: r.dim, ysi: r.ysi.clone() });
    }
    out
}

/// The tile menu's state: what input and the writer share.
#[derive(Resource)]
pub struct MenuTiles {
    /// The menu while it is drawn from tiles (the adapter's decision this frame).
    pub menu: Option<TileId>,
    pub(super) rows: [Vec<RowTile<RowKey>>; 2],
    paths: &'static Paths,
    views: [ListView<RowKey>; 2],
    shown: Option<Snapshot>,
    written_in: Option<TileId>,
    pub sizes: SizesWritten,
    text_scale:f32,
    /// A press on a marker, held.
    pub drag: Option<Drag>,
    /// The scroll last logged as drawn.
    drawn: [Option<f64>; 2],
    /// [design] The fallback's reason is warned once per session.
    pub warned: bool,
}

/// The heading the pointer is over, for the hand-built wheel (`crate::menu::menu_mouse`), which
/// steps its category there [design: owner request #1192].
#[derive(Resource, Default)]
pub struct TileWheelTitle(pub Option<Side>);

/// What a click on the menu's tiles stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuTile {
    Row(Side, usize),
    Arrow(Side, i32),
    Title(Side),
    /// [documented: barter_menu.xml] A hotrect over the list without focus.
    Focus(Side),
    TakeAll,
    Accept,
    Exit,
    Bar(Side, list::BarPiece),
    /// Another tile of the menu: nothing to do.
    Other,
}


impl Default for MenuTiles {
    fn default() -> Self { Self::new(&super::container_menu::PATHS) }
}

impl MenuTiles {
    pub fn name(&self) -> &'static str { self.paths.menu }

    pub fn new(paths: &'static Paths) -> Self {
        Self { paths, menu: None, rows: Default::default(), views: Default::default(), shown: None,
            written_in: None, sizes: Default::default(), text_scale:1.0, drag: None, drawn: [None; 2], warned: false }
    }
}

pub(super) fn set(ui: &mut Ui, path: &str, name: &str, v: Value) {
    if let Some(t) = ui.find_tile(path) {
        ui.set_trait(t, name, v);
    }
}

pub(super) fn on(b: bool) -> Value {
    Value::Num(f64::from(u8::from(b)))
}

/// [design] Reconcile row identity and write the common list content.
pub fn write_lists(ui: &mut Ui, s: &Snapshot, st: &mut MenuTiles) {
    for side in SIDES {
        let i = side as usize;
        let wanted: Vec<Wanted<'_, RowKey>> = s.lists[i].rows.iter().map(|r| Wanted { key: &r.key, name: &r.name }).collect();
        list::reconcile(ui, st.paths.lists[i], st.paths.template, &mut st.rows[i], &wanted);
        for (row, shown) in st.rows[i].iter().zip(&s.lists[i].rows) {
            if let Some(t) = list::child(ui, row.tile, st.paths.row_text) {
                ui.set_trait(t, "string", Value::Str(shown.text.clone()));
                ui.set_trait(t, "alpha", Value::Num(if shown.dim { 128.0 } else { 255.0 }));
            }
            if let Some(t) = list::child(ui, row.tile, st.paths.row_mark) {
                ui.set_trait(t, "visible", on(shown.equipped));
            }
            if s.ysi_icons {
                fnv_game::ui_ysi::apply_row_icon(ui, &fnv_ui::MemSource::default(), row.tile, shown.ysi.as_ref());
            } else {
                fnv_game::ui_ysi::remove_row_icon(ui, row.tile);
            }
        }
        set(ui, st.paths.titles[i], "string", Value::Str(s.titles[i].clone()));
        set(ui, st.paths.lists[i], "_num_filtered", Value::Num(s.lists[i].filtered as f64));
    }
}

/// A tiled frame's sizes and places, after what changed was written (see the module notes):
/// the tree worked out, the texts measured until no wrap or size moves [design: three rounds,
/// a title's width to a card's value's wrap], the rows placed, the counts, scroll and highlight
/// written, the tree worked out again. `write_sizes` is one measuring round, each on the tree
/// just worked out, so it finds and loads a font computed from what the model wrote.
pub fn settle(ui: &mut Ui, s: &Snapshot, st: &mut MenuTiles, write_sizes: &mut dyn FnMut(&mut Ui, &mut SizesWritten) -> usize) {
    ui.update();
    for _ in 0..3 {
        if write_sizes(ui, &mut st.sizes) == 0 {
            break;
        }
        ui.update();
    }
    if s.ysi_icons {
        let mut moved = false;
        for side in SIDES {
            for row in &st.rows[side as usize] {
                moved |= fnv_game::ui_ysi::sync_row_icon_x(ui, row.tile);
            }
        }
        if moved {
            ui.update();
        }
    }
    for side in SIDES {
        let i = side as usize;
        let Some(list_tile) = ui.find_tile(st.paths.lists[i]) else { continue };
        let shown = &s.lists[i];
        list::settle_list_sized(ui,list_tile,&st.rows[i],st.paths.row_text,&mut st.views[i],
            list::RowLayout {selected:shown.selected,group:shown.category as u32,pointer_pick:s.pointer_pick,navigated:s.navigated,text_scale:st.text_scale,minimum_height:51.0});
    }
    st.rewrite(ui);
    ui.update();
}

impl MenuTiles {
    pub(crate) fn text_scale(&self)->f32 {self.text_scale}
    pub(crate) fn set_text_scale(&mut self,scale:f32) {
        if self.text_scale!=scale {self.text_scale=scale;self.sizes.clear();self.written_in=None;self.drag=None;}
    }

    /// [`settle`] with the snapshot written last, on every frame the menu is drawn.
    pub fn settle_shown(&mut self, ui: &mut Ui, write_sizes: &mut dyn FnMut(&mut Ui, &mut SizesWritten) -> usize) {
        if let Some(s) = self.shown.take() {
            settle(ui, &s, self, write_sizes);
            self.shown = Some(s);
        }
    }

    /// The scroll and the highlight written from the view: after [`settle`]'s places, and
    /// after the input's step and its clicks ([`super::input::feed`]).
    pub fn rewrite(&self, ui: &mut Ui) {
        for side in SIDES {
            let i = side as usize;
            let Some(list_tile) = ui.find_tile(self.paths.lists[i]) else { continue };
            self.views[i].rewrite(ui, list_tile);
        }
    }

    /// The tile a click on `tile` stands for, while the menu is drawn from its tiles
    /// ([`Self::menu`]). None otherwise: off or in the fallback a script's queued click on the
    /// file's tile acts on nothing.
    pub fn clicked(&self, ui: &Ui, tile: TileId) -> Option<MenuTile> {
        let menu = self.menu?;
        if !ui.is_live(tile) || ui.root_of(tile) != menu {
            return None;
        }
        Some(self.stands_for(ui, tile))
    }

    fn stands_for(&self, ui: &Ui, tile: TileId) -> MenuTile {
        let lists = SIDES.map(|side| (self.paths.lists[side as usize].rsplit('/').next().unwrap_or_default(), self.rows[side as usize].as_slice()));
        match list::hit(ui, tile, &lists, self.paths.targets) {
            list::Hit::Row(at, index) => MenuTile::Row(SIDES[at], index),
            list::Hit::Bar(at, piece) => MenuTile::Bar(SIDES[at], piece),
            list::Hit::Target(what) => what,
            list::Hit::Other => MenuTile::Other,
        }
    }

    /// The index of `side`'s row at `index` when the row the tree shows is still the model's
    /// row there; a tile whose row moved or left the list acts on nothing.
    pub fn row_at(&self, m: &MenuModel, side: Side, index: usize) -> Option<usize> {
        self.row_in(side, index, &rows_shown(&m.list(side).rows))
    }

    /// [`Self::row_at`] over the rows `now`.
    pub(super) fn row_in(&self, side: Side, index: usize, now: &[RowShown]) -> Option<usize> {
        let tile_key = &self.rows[side as usize].get(index)?.key;
        (now.get(index)?.key == *tile_key).then_some(index)
    }

    /// The guard for commands on `side`'s row `index`: what its tile stands for, in the menu
    /// `mode`. `selected`: they act on it as the selected row.
    pub fn guard(&self, mode: Mode, side: Side, index: usize, selected: bool, then: u8) -> Option<RowGuard> {
        let key = self.rows[side as usize].get(index)?.key;
        Some(RowGuard { mode, side, index, key, selected, then })
    }

    /// The list tile of `side`.
    pub fn list_tile(&self, ui: &Ui, side: Side) -> Option<TileId> {
        ui.find_tile(self.paths.lists[side as usize])
    }

    /// A list's scroll now.
    pub fn value(&self, side: Side) -> f64 {
        self.views[side as usize].value
    }
    /// Current list value, upper bound and page extent, independent of selection.
    pub fn scroll_extent(&self, side: Side) -> (f64, f64, f64) {
        let view = &self.views[side as usize];
        (view.value, view.counts.most(), view.counts.visible)
    }

    /// Set a list's scroll, held in its bar's range (the bar's pieces, the marker's drag): the
    /// view's own, no command of the model's. Returns whether it moved.
    pub fn scroll_to(&mut self, side: Side, value: f64) -> bool {
        self.views[side as usize].scroll_to(value)
    }

    /// Whether what `s` shows differs from what was written in `menu`, and if so that it is now.
    pub fn is_new(&mut self, menu: TileId, s: &Snapshot) -> bool {
        if self.written_in != Some(menu) {
            self.written_in = Some(menu);
            self.shown = None;
        }
        if self.shown.as_ref() == Some(s) {
            return false;
        }
        self.shown = Some(s.clone());
        true
    }

    /// The snapshot written last.
    pub fn shown(&self) -> Option<&Snapshot> {
        self.shown.as_ref()
    }

    /// Whether a new snapshot is needed whatever changed in the model: none yet in `menu`.
    pub fn needs_snapshot(&self, menu: TileId) -> bool {
        self.written_in != Some(menu) || self.shown.is_none()
    }

    /// The menu closed: nothing of it stays for the next (the warning does).
    pub fn close(&mut self) {
        *self = MenuTiles { warned: self.warned, ..Self::new(self.paths) };
    }

    /// Logged when the scroll drawn changes, and when it is not the value written (smoke reads
    /// both): the view's value must outlast a click's pulse, as the wait menu's slider.
    pub fn check_drawn(&mut self, ui: &Ui, log: &mut dyn FnMut(String)) {
        for side in SIDES {
            let i = side as usize;
            let Some(list_tile) = ui.find_tile(self.paths.lists[i]) else { continue };
            let Some(v) = list::value_shown(ui, list_tile) else { continue };
            let n = |x: f64| Value::Num(x).text();
            let name = side_name(side);
            let menu = self.paths.menu;
            if self.drawn[i] != Some(v) {
                log(format!("tiles: {menu} {name} list scrolled to {} of {}", n(v), n(self.views[i].counts.most())));
                self.drawn[i] = Some(v);
            }
            if v != self.views[i].value {
                log(format!("tiles: {menu} {name} list drawn at {}, not the {} written", n(v), n(self.views[i].value)));
            }
        }
    }
}

pub(super) fn side_name(side: Side) -> &'static str {
    match side {
        Side::Container => "container",
        Side::Player => "player",
    }
}

/// The mark and clip writes that follow what is in the tree, after the scripts and before
/// the drawing: a tile injected or a window enabled at any time is covered from that frame on.
/// A walk of the menu's tiles, writing only what is not there yet.
pub fn mark_tiles(ui: &mut Ui, paths: &Paths) {
    mark_menu(ui, paths.menu, &paths.lists);
}

/// [`mark_tiles`] by the menu's name and its lists' paths: for an adapter without [`Paths`].
pub(super) fn mark_menu(ui: &mut Ui, menu_name: &str, lists: &[&str]) {
    let Some(menu) = ui.menu(menu_name) else { return };
    fnv_ui::input::mark_clip_windows(ui, menu);
    let mut stack = vec![menu];
    while let Some(t) = stack.pop() {
        if !ui.is_live(t) {
            continue;
        }
        let tile = ui.tile(t);
        stack.extend(tile.children.iter().copied());
        if !tile.get(OWN_ALPHA).is_some_and(Value::truthy) {
            ui.set_trait(t, OWN_ALPHA, Value::Num(1.0));
        }
    }
    for list_path in lists {
        set(ui, &format!("{list_path}/{}", list::HIGHLIGHT), HOTRECT_FILL, Value::Num(1.0));
        if let Some(list_id) = ui.find_tile(list_path) {
            let rows: Vec<TileId> = ui
                .tile(list_id)
                .children
                .iter()
                .copied()
                .filter(|&c| ui.is_live(c) && ui.tile(c).get("listindex").is_some())
                .collect();
            list::unclip_row_children(ui, list_id, rows);
        }
    }
}

/// Whether the menu takes the pointer and keys this frame: drawn from its tiles and no prompt
/// over it, hand-built or from tiles (no click goes past the prompt [documented:
/// quantity_menu.xml, `&no_click_past;`]).
pub fn takes_input(menu: Option<TileId>, prompt: bool) -> Option<TileId> {
    menu.filter(|_| !prompt)
}

/// Whether the pointer's coming onto a row selects it: it moved there this frame, the wheel
/// does not hold the selection (`crate::menu::WheelHold`) and it is not selected already: so a
/// list moved under a resting pointer leaves the choice alone [owner 2026-10-22].
pub fn hover_selects(moved: bool, wheel_holds: bool, already: bool) -> bool {
    moved && !wheel_holds && !already
}

/// The commands a pointer click on a row sends, as the hand-built menu's (`menu_mouse`): the
/// row selected and shift, the stack; another row, it selected and acted on; the row selected,
/// its action.
pub fn row_commands(side: Side, index: usize, on_it: bool, shift: bool) -> Vec<crate::menu::model::Command> {
    row_commands_with(side, index, on_it, shift, false)
}

/// [`row_commands`] with optional Shift/Ctrl quantity-select shortcut routing
/// (`quantity-select-short-keys`, #3799) [design: Shift takes precedence over Ctrl when both
/// are held].
pub fn row_commands_with(side: Side, index: usize, on_it: bool, shift: bool, ctrl_single: bool) -> Vec<crate::menu::model::Command> {
    use crate::menu::model::Command;
    match (on_it, shift, ctrl_single) {
        (true, true, _) => vec![Command::Stack],
        (true, false, true) => vec![Command::OneSingle],
        (false, _, _) => vec![Command::Hover(side, index), if shift { Command::Stack } else if ctrl_single { Command::OneSingle } else { Command::One }],
        (true, false, false) => vec![Command::Select(side, index)],
    }
}

#[cfg(test)]
#[path = "item_menu_ysi_tests.rs"]
mod item_menu_ysi_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use fnv_sim::FormId;

    /// #3442: the ammo the weapon loads now carries the equipped mark in the list drawn
    /// from tiles, as in the hand-built one.
    #[test]
    fn the_loaded_ammo_s_row_carries_the_equipped_mark() {
        let row = |item: u32, equipped: bool, loaded: bool| Row {
            item: FormId(item),
            at: None,
            name: format!("Item {item}"),
            count: 12,
            weight: None,
            value: None,
            kind: None,
            equipped,
            quest: false,
            condition: None,
            icon: None,
            price: None,
            bartered: false,
            loaded,
            dim: false,
            mods: 0,
            mods_value: 0,
            folder: None,
            reference: None,
            live: false,
            ysi: None,
        };
        let shown = rows_shown(&[row(1, false, false), row(2, false, true), row(3, true, false)]);
        assert_eq!(shown.iter().map(|r| r.equipped).collect::<Vec<_>>(), [false, true, true]);
    }
}
