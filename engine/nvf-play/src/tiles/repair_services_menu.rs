//! The vendor's repair services menu drawn from `menus\repair_services_menu.xml`, behind
//! `[UI] TileMenus=RepairServicesMenu` [design: repair.md §3.1; the shape of
//! `super::barter_menu`]. Off (the default), this adapter does no work and `crate::repair` draws
//! by hand as ever.
//!
//! - **The rules stay `crate::repair`'s**: what is listed, what a repair costs, how far it goes,
//!   its refusals, sounds and log lines. This module writes the model's [`View`] as the file's
//!   "set in code" traits, and says what a click on a tile stands for
//!   (`crate::repair::RepairServicesInput`, read by `crate::repair`'s `keys`). The keys stay the
//!   model's ([`HAND_BUILT_KEYS`]). Only the list's scroll is the view's own.
//! - **A frame**: `crate::repair::draw` works the view out at the end of each frame; this writer
//!   reads it at the next, before the input, so a click acts on what its tile stood for. The
//!   tree is one whole state of the view, as `super::container_menu` has it.
//! - **The words**: a setting through the strings table, a record's name or a number, or the
//!   one sentence of NVForever's own the model already has ([`crate::repair::BEYOND_WORDS`]).
//!   The headline, the empty list's text and the buttons' words are the file's; Repair All's
//!   gets its total after them. The words the hand-built menu shows from the original's screen
//!   are not read here (repair.md §3.1, a list of every string).
//! - **The captures** (private jobs run07_repair_services and run07c_repair_services, 1280x720)
//!   read lit and refused rows, the cards after a repair, the cost's dim and Repair All in each
//!   state; repair.md §3.1 says what they read and what stays open.

use super::item_menu::{on, set};
use super::list::{self, Drag, ListView, RowTile, Wanted};
use super::menus::{SizesWritten, Tiled};
use super::repair_cards::{self, Card, Figures};
use crate::repair::{RepairServices, RepairServicesInput, RowView, View};
use bevy::prelude::*;
use fnv_game::repair::{gain_percent, Quote, REPAIR_SERVICES_MENU as MENU_CODE};
use fnv_sim::inventory::Extra;
use fnv_sim::FormId;
use fnv_ui::{TileId, Ui, Value};

/// [documented: repair_services_menu.xml] The menu and the paths written by this adapter.
pub const REPAIR_SERVICES_MENU: &str = "RepairServicesMenu";
const BACKGROUND: &str = "RepairServicesMenu/NOGLOW_BRANCH/RSM_Background";
const SKILL: &str = "RepairServicesMenu/NOGLOW_BRANCH/RSM_MainRect/RSM_VendorSkillLabel";
const CAPS: &str = "RepairServicesMenu/NOGLOW_BRANCH/RSM_MainRect/RSM_CapsLabel";
const LIST: &str = "RepairServicesMenu/NOGLOW_BRANCH/RSM_MainRect/RSM_RepairList";
const EMPTY: &str = "RepairServicesMenu/NOGLOW_BRANCH/RSM_MainRect/RSM_EmptyRepairListText";
const ICON: &str = "RepairServicesMenu/NOGLOW_BRANCH/RSM_MainRect/RSM_ItemIcon";
const STATS: &str = "RepairServicesMenu/NOGLOW_BRANCH/RSM_MainRect/RSM_StatsDisplayRect";
const COST: &str = "RepairServicesMenu/NOGLOW_BRANCH/RSM_MainRect/RSM_StatsDisplayRect/RSM_RepairCostText";
const ERROR: &str = "RepairServicesMenu/NOGLOW_BRANCH/RSM_MainRect/RSM_StatsDisplayRect/RSM_ErrorText";
const REPAIR: &str = "RepairServicesMenu/NOGLOW_BRANCH/RSM_MainRect/RSM_ButtonRect/RSM_RepairButton";
const REPAIR_ALL: &str = "RepairServicesMenu/NOGLOW_BRANCH/RSM_MainRect/RSM_ButtonRect/RSM_RepairAllButton";
const DONE: &str = "RepairServicesMenu/NOGLOW_BRANCH/RSM_MainRect/RSM_ButtonRect/RSM_DoneButton";
/// The cards' names under [`STATS`], for `super::repair_cards`.
const PREFIX: &str = "RSM_";
const LIST_NAME: &str = "RSM_RepairList";
const TEMPLATE: &str = "RSM_RepairListTemplate";
const ROW: &str = "RSM_RepairListTemplateRect";
const ROW_TEXT: &str = "ListItemText";
const ROW_METER_BACK: &str = "RSM_Template_MeterBackground";
const ROW_METER: &str = "RSM_Template_Meter";
const ROW_MARK: &str = "RSM_Template_ItemMarker";
/// The square of an equipped row where the file names no picture [design: the hand-built menu's,
/// `crate::repair`].
const MARK_PICTURE: &str = "interface\\shared\\marker\\square_filled.dds";

/// A row refused or too dear: `alpha` on its text, fill and square, `brightness` 255; the cost of
/// a repair the player cannot pay alike [observed: private job run07c_repair_services, 1280x720,
/// read].
const DIM_ALPHA: f64 = 128.0;

/// [design: `crate::repair::keys`, `crate::pad::menu_buttons`] What the model takes itself: W, S,
/// the arrows, A and E, and the controller's D-pad, A (Enter), B (Esc) and X (A). A script's
/// MenuTapKey still follows the file's `_PCButton_A` and `_PCButton_E`.
pub const HAND_BUILT_KEYS: [&str; 9] = ["A", "E", "W", "S", "xup", "xdown", "xbuttona", "xbuttonb", "xbuttonx"];

/// What a row stands for [design: `crate::repair::RowView`, one group of units].
pub type RowKey = (FormId, Extra);

/// [design] A short or restructured file keeps the entire hand-built menu.
pub fn built(ui: &Ui) -> Tiled {
    let mut paths: Vec<String> = [REPAIR_SERVICES_MENU, BACKGROUND, SKILL, CAPS, LIST, EMPTY, ICON, STATS, COST, ERROR, REPAIR, REPAIR_ALL, DONE].into_iter().map(str::to_owned).collect();
    for button in [REPAIR, REPAIR_ALL, DONE] {
        paths.extend(["button_text", "PCShortcutLabel"].map(|t| format!("{button}/{t}")));
    }
    paths.push(format!("{LIST}/{}", list::HIGHLIGHT));
    for piece in [list::MARKER, list::UP, list::DOWN, list::PAGE_UP, list::PAGE_DOWN] {
        paths.push(format!("{LIST}/{}/{piece}", list::SCROLLBAR));
    }
    paths.extend(repair_cards::paths(STATS, PREFIX));
    for path in &paths {
        if ui.find_tile(path).is_none() {
            return Tiled::No(format!("{path} missing"));
        }
    }
    list::template_has(ui, REPAIR_SERVICES_MENU, TEMPLATE, ROW, &[("text", ROW_TEXT), ("image", ROW_METER_BACK), ("image", ROW_METER), ("image", ROW_MARK)])
}

/// [design] This decision is shared by the writer and the model's drawing.
pub fn tiled(on: bool, game: &fnv_game::Game<'_>, open: bool) -> bool {
    on && open && game.ui_files.is_some() && game.menu == Some(MENU_CODE) && built(&game.ui_tree).yes()
}

/// The settings a frame's words read, through the strings table.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Words {
    pub skill: String,
    pub caps: String,
    pub condition: String,
    pub damage: String,
    pub threshold: String,
    /// `sPCMenuHintA` and `sPCMenuHintE` as the device names them.
    pub hints: [String; 2],
}

impl Words {
    fn read(game: &fnv_game::Game<'_>) -> Self {
        let word = |key| fnv_game::ui_loading::game_string(game, key);
        let pad = crate::pad::at_menu(MENU_CODE);
        Words {
            skill: word("sRepairSkill"),
            caps: word("sInventoryCaps"),
            condition: word("sInventoryCondition"),
            damage: word("sInventoryDamage"),
            threshold: word("sInventoryDamageThreshold"),
            hints: [
                crate::pad::game_hint(&word("sPCMenuHintA"), &[KeyCode::KeyA], pad),
                crate::pad::game_hint(&word("sPCMenuHintE"), &[KeyCode::KeyE, KeyCode::Escape], pad),
            ],
        }
    }
}

/// A row as shown.
#[derive(Clone, Debug, PartialEq)]
pub struct RowShown {
    pub key: RowKey,
    /// The item's name, the tile's [design: as the other lists' rows, `super::list::Wanted`].
    pub name: String,
    pub text: String,
    pub equipped: bool,
    pub condition: f32,
    pub ready: bool,
    /// Resolved ySI tag/icon/template for this row when ySI is active (`None` otherwise).
    pub ysi: Option<fnv_game::ui_ysi::YsiItemDisplay>,
}

/// One whole state of the view, as the menu shows it: a frame with the same writes nothing but
/// sizes and scroll. A plain snapshot, so tests need no world.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub rows: Vec<RowShown>,
    pub selected: Option<usize>,
    /// The selection was last moved by the pointer: the list stays where it is.
    pub pointer_pick: bool,
    /// A count of the keys' moves.
    pub navigated: u32,
    pub skill: String,
    pub caps: String,
    pub icon: Option<String>,
    pub figures: Option<Figures>,
    /// The cost's line, and whether it can be paid.
    pub cost: Option<(String, bool)>,
    /// The chosen row is beyond the vendor's skill.
    pub beyond: bool,
    pub all_ready: bool,
    /// What Repair All costs, after the file's words on its button.
    pub total: u32,
    /// Repair All asks to be pressed again (the player's Repair All Confirmation tweak).
    pub confirming: bool,
    pub hints: [String; 2],
}

/// A figure and its setting: "Caps  100" [observed: private job run07_repair_services, 1280x720, a
/// gap about twice a word space; hypothesis: two spaces, the strings were not read].
fn labelled(word: &str, value: impl std::fmt::Display) -> String {
    format!("{word}  {value}")
}

/// [design: LEGAL.md] What a repair costs, in the setting's word and a number: the words the
/// original shows there are on its screen alone and are not transcribed. No suffix for a row of
/// several units: the original has no such row, and this is what one repair takes [observed:
/// private job run07c_repair_services, 1280x720; two at one condition are two rows there,
/// repair.md §3.1].
fn cost_line(caps: &str, cost: u32) -> String {
    labelled(caps, cost)
}

fn figures(r: &RowView, w: &Words) -> Figures {
    let to = match r.quote {
        Quote::Repair { to, .. } => Some(to),
        Quote::Beyond => None,
    };
    let stat_title = match r.stat {
        Some(("DAM", ..)) => w.damage.clone(),
        Some(_) => w.threshold.clone(),
        None => String::new(),
    };
    Figures {
        condition_title: w.condition.clone(),
        stat_title,
        now: Card { condition: r.condition, stat: r.stat.map(|s| s.1) },
        after: to.map(|to| Card { condition: to, stat: r.stat.map(|s| s.2) }),
        gain_percent: to.map_or(0, |to| gain_percent(r.condition, to)),
        gain_stat: to.and(r.stat.map(|s| s.2 - s.1)),
    }
}

impl Snapshot {
    pub(crate) fn of(v: &View, pointer_pick: bool, navigated: u32, w: &Words) -> Self {
        let chosen = v.rows.get(v.selected);
        Snapshot {
            rows: v.rows.iter().map(|r| RowShown { key: (r.item, r.extra), name: r.name.clone(), text: r.label.clone(), equipped: r.extra.equipped, condition: r.condition, ready: r.ready, ysi: r.ysi.clone() }).collect(),
            selected: chosen.map(|_| v.selected),
            pointer_pick,
            navigated,
            skill: labelled(&w.skill, v.skill),
            caps: labelled(&w.caps, v.caps),
            icon: chosen.and_then(|r| r.icon.clone()),
            figures: chosen.map(|r| figures(r, w)),
            cost: chosen.and_then(|r| r.quote.cost().map(|c| (cost_line(&w.caps, c), r.ready))),
            beyond: chosen.is_some_and(|r| r.quote == Quote::Beyond),
            all_ready: v.all_ready,
            total: v.total,
            confirming: v.confirming,
            hints: w.hints.clone(),
        }
    }

    /// The selected row can be repaired now.
    fn selected_ready(&self) -> bool {
        self.selected.and_then(|i| self.rows.get(i)).is_some_and(|r| r.ready)
    }
}

/// What a click on the menu's tiles stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepairServicesTile {
    Row(usize),
    Repair,
    RepairAll,
    Done,
    Bar(list::BarPiece),
    Other,
}

const TARGETS: &[(&str, RepairServicesTile)] = &[("RSM_RepairButton", RepairServicesTile::Repair), ("RSM_RepairAllButton", RepairServicesTile::RepairAll), ("RSM_DoneButton", RepairServicesTile::Done)];

/// What a click on row `index` asks: on the selected row its repair, as the hand-built menu's
/// click (`crate::repair::keys`), else that row chosen.
pub fn row_input(selected: Option<usize>, index: usize) -> RepairServicesInput {
    if selected == Some(index) {
        RepairServicesInput::Repair
    } else {
        RepairServicesInput::Select(index)
    }
}

/// The tile menu's state: what input and the writer share.
#[derive(Resource, Default)]
pub struct RepairServicesTiles {
    /// The menu while it is drawn from tiles (this frame's decision).
    pub menu: Option<TileId>,
    pub(super) rows: Vec<RowTile<RowKey>>,
    view: ListView<RowKey>,
    shown: Option<Snapshot>,
    written_in: Option<TileId>,
    text_scale:Option<f32>,
    normal_guard:Option<crate::menu::normal_services::Guard>,
    /// What the snapshot was made from, so a frame with the same makes none.
    from: Option<(View, bool, u32)>,
    pub sizes: SizesWritten,
    /// Repair All's words as the file has them, read once in each tree: what stands before the
    /// total.
    all_words: Option<String>,
    /// A press on the scroll marker, held.
    pub drag: Option<Drag>,
    /// The scroll last logged as drawn.
    drawn: Option<f64>,
    /// [design] The fallback's reason is warned once per session.
    warned: bool,
}

impl RepairServicesTiles {
    pub(crate) fn normal_hit(&self,ui:&Ui,frame:crate::menu::barter_touch::Frame,
        guard:&crate::menu::normal_services::Guard,point:Vec2,size:f32)->Option<crate::menu::normal_services::Hit> {
        use crate::menu::{normal_services::{Action,Hit},normal_touch::Source};
        if self.text_scale.unwrap_or(1.0)!=size || !self.normal_guard.as_ref().is_some_and(|g|g.same(guard)){return None}
        let menu=self.menu?;
        let at=super::input::to_menu_units(point,frame.size,Vec2::splat(0.5));
        let candidates=fnv_ui::input::hit_candidates(ui,menu,&fnv_ui::input::no_measure);
        if let Some(picked)=fnv_ui::input::top_hit(ui,&candidates,at) {
            let (action,pane,key)=match self.clicked(ui,picked.tile)? {
                RepairServicesTile::Row(index)=>{let i=self.row_at(index)?;(Action::Row(i),Some(0),self.rows.get(index)?.tile)}
                RepairServicesTile::Repair=>(Action::Vendor(RepairServicesInput::Repair),None,picked.tile),
                RepairServicesTile::RepairAll=>(Action::Vendor(RepairServicesInput::All),None,picked.tile),
                RepairServicesTile::Done=>(Action::Vendor(RepairServicesInput::Done),None,picked.tile),
                _=>return crate::menu::normal_services::native_pane(ui,frame,self.list_tile(ui)?,0,point),
            };
            return Some(Hit {source:Source::Tile(menu,key),action,pane,row_step:0.0})
        }
        crate::menu::normal_services::native_pane(ui,frame,self.list_tile(ui)?,0,point)
    }
    pub(crate) fn normal_scroll(&mut self,ui:&mut Ui,pixels:f64,frame:crate::menu::barter_touch::Frame,size:f32) {
        if self.text_scale.unwrap_or(1.0)!=size || self.menu.is_none(){return}
        let Some(list)=self.list_tile(ui) else{return};
        let Some(delta)=ui.tile(list).get("_scroll_delta").map(Value::num).filter(|v|v.is_finite() && *v>0.0) else{return};
        let a=super::input::to_menu_units(Vec2::ZERO,frame.size,Vec2::splat(0.5));
        let b=super::input::to_menu_units(Vec2::Y,frame.size,Vec2::splat(0.5));
        let next=self.value()+pixels*(b.1-a.1)/delta;
        if next.is_finite() && self.scroll_to(next){self.rewrite(ui);ui.update();}
    }

    /// The menu closed: nothing of it stays for the next (the warning does).
    fn close(&mut self) {
        *self = Self { warned: self.warned, ..Self::default() };
    }

    /// The row with the highlight as shown.
    pub fn selected(&self) -> Option<usize> {
        self.shown.as_ref().and_then(|s| s.selected)
    }

    /// The tile a click on `tile` stands for, while the menu is drawn from its tiles.
    pub fn clicked(&self, ui: &Ui, tile: TileId) -> Option<RepairServicesTile> {
        let menu = self.menu?;
        if !ui.is_live(tile) || ui.root_of(tile) != menu {
            return None;
        }
        Some(match list::hit(ui, tile, &[(LIST_NAME, self.rows.as_slice())], TARGETS) {
            list::Hit::Row(_, index) => RepairServicesTile::Row(index),
            list::Hit::Bar(_, piece) => RepairServicesTile::Bar(piece),
            list::Hit::Target(what) => what,
            list::Hit::Other => RepairServicesTile::Other,
        })
    }

    /// `index` when the row the tree shows there is still the row shown there: a tile whose row
    /// moved or left the list acts on nothing.
    pub fn row_at(&self, index: usize) -> Option<usize> {
        let tile_key = self.rows.get(index)?.key;
        (self.shown.as_ref()?.rows.get(index)?.key == tile_key).then_some(index)
    }

    pub fn list_tile(&self, ui: &Ui) -> Option<TileId> {
        ui.find_tile(LIST)
    }

    /// The list's scroll now.
    pub fn value(&self) -> f64 {
        self.view.value
    }

    /// Set the list's scroll, held in its bar's range: the view's own, nothing of the model's.
    pub fn scroll_to(&mut self, value: f64) -> bool {
        self.view.scroll_to(value)
    }

    /// The scroll and the highlight written from the view: after [`settle`]'s places, and after
    /// the input's step and its clicks (`super::input::feed`).
    pub fn rewrite(&self, ui: &mut Ui) {
        if let Some(l) = ui.find_tile(LIST) {
            self.view.rewrite(ui, l);
        }
    }

    /// Logged when the scroll drawn changes, and when it is not the value written (smoke reads
    /// both): the view's value must outlast a click's pulse.
    fn check_drawn(&mut self, ui: &Ui, log: &mut dyn FnMut(String)) {
        let Some(v) = ui.find_tile(LIST).and_then(|l| list::value_shown(ui, l)) else { return };
        let n = |x: f64| Value::Num(x).text();
        if self.drawn != Some(v) {
            log(format!("tiles: {REPAIR_SERVICES_MENU} list scrolled to {} of {}", n(v), n(self.view.counts.most())));
            self.drawn = Some(v);
        }
        if v != self.view.value {
            log(format!("tiles: {REPAIR_SERVICES_MENU} list drawn at {}, not the {} written", n(v), n(self.view.value)));
        }
    }
}

/// [design] File places and textures stay in the tree; what the file leaves to code is written.
/// Nothing writes the headline's title or the empty list's words: those are the file's settings.
fn write(ui: &mut Ui, s: &Snapshot, st: &mut RepairServicesTiles) {
    let wanted: Vec<Wanted<'_, RowKey>> = s.rows.iter().map(|r| Wanted { key: &r.key, name: &r.name }).collect();
    list::reconcile(ui, LIST, TEMPLATE, &mut st.rows, &wanted);
    for (row, shown) in st.rows.iter().zip(&s.rows) {
        let alpha = Value::Num(if shown.ready { 255.0 } else { DIM_ALPHA });
        if let Some(t) = list::child(ui, row.tile, ROW_TEXT) {
            ui.set_trait(t, "string", Value::Str(shown.text.clone()));
            ui.set_trait(t, "alpha", alpha.clone());
        }
        if let Some(t) = list::child(ui, row.tile, ROW_METER) {
            ui.set_trait(t, "alpha", alpha.clone());
        }
        if let Some(t) = list::child(ui, row.tile, ROW_MARK) {
            ui.set_trait(t, "visible", on(shown.equipped));
            ui.set_trait(t, "alpha", alpha);
            // [hypothesis] The file's picture if it has one.
            if ui.tile(t).get("filename").is_none_or(|v| v.text().is_empty()) {
                ui.set_trait(t, "filename", Value::Str(MARK_PICTURE.into()));
            }
        }
        fnv_game::ui_ysi::apply_row_icon(ui, &fnv_ui::MemSource::default(), row.tile, shown.ysi.as_ref());
    }
    set(ui, LIST, "_num_filtered", Value::Num(0.0));
    // [hypothesis] The one list of this menu takes the pointer, as the container's two
    // (`super::container_menu`).
    set(ui, LIST, "_enabled", on(true));
    set(ui, EMPTY, "visible", on(s.rows.is_empty()));
    set(ui, SKILL, "string", Value::Str(s.skill.clone()));
    set(ui, CAPS, "string", Value::Str(s.caps.clone()));
    set(ui, ICON, "visible", on(s.icon.is_some()));
    set(ui, ICON, "filename", Value::Str(s.icon.clone().unwrap_or_default()));
    set(ui, STATS, "visible", on(s.selected.is_some()));
    repair_cards::write(ui, STATS, PREFIX, s.figures.as_ref());
    set(ui, COST, "visible", on(s.cost.is_some()));
    set(ui, COST, "string", Value::Str(s.cost.as_ref().map_or_else(String::new, |c| c.0.clone())));
    set(ui, COST, "alpha", Value::Num(if s.cost.as_ref().is_none_or(|c| c.1) { 255.0 } else { DIM_ALPHA }));
    set(ui, ERROR, "visible", on(s.beyond));
    set(ui, ERROR, "string", Value::Str(if s.beyond { crate::repair::BEYOND_WORDS.into() } else { String::new() }));
    // Repair's `visible` is the file's, for the Xbox: on a PC it stays hidden and Repair All takes
    // its place; its `target` reads 1 with no row and on a lit row, 0 on a refused or too dear one
    // [documented: the file; observed: private job run07c_repair_services, 1280x720, read].
    set(ui, REPAIR, "target", on(s.selected.is_none() || s.selected_ready()));
    set(ui, REPAIR_ALL, "visible", on(true));
    // The file dims Repair All by `alpha` from this: 1 when the caps reach the total, 0 otherwise
    // and at a total of 0 [documented: the file, text_box.xml; observed: the same job, read].
    set(ui, REPAIR_ALL, "target", on(s.all_ready));
    let words = st.all_words.get_or_insert_with(|| ui.get_string(&format!("{REPAIR_ALL}/button_text/string")).unwrap_or_default());
    // The file's words, a space, the total in brackets, offered even when out of reach [observed:
    // the same job, read and in its pictures].
    // Asking under the player's Repair All Confirmation tweak [design: our own words].
    let ask = if s.confirming { "? Again to confirm" } else { "" };
    set(ui, &format!("{REPAIR_ALL}/button_text"), "string", Value::Str(format!("{words} ({}){ask}", s.total)));
    set(ui, DONE, "visible", on(true));
    set(ui, DONE, "target", on(true));
    for (button, hint) in [REPAIR_ALL, DONE].into_iter().zip(&s.hints) {
        set(ui, button, "_PCButtonText", Value::Str(hint.clone()));
    }
    // [observed: private job run07_repair_services, 1280x720, 193.8 read here too, the file's 32
    // being replaced as in the container's].
    set(ui, BACKGROUND, "alpha", Value::Num(super::container_menu::BACKGROUND_ALPHA));
}

/// The rows' condition bars: the fill as wide as the condition of the back's width [documented:
/// repair_services_menu.xml, set in code; observed: private job run07_repair_services, 1280x720,
/// `width` condition x 60]. On
/// the tree worked out, so a width the file computes reads true.
fn write_meters(ui: &mut Ui, rows: &[RowTile<RowKey>], shown: &[RowShown]) {
    for (row, s) in rows.iter().zip(shown) {
        let back = list::child(ui, row.tile, ROW_METER_BACK).and_then(|t| ui.tile(t).get("width")).map_or(0.0, Value::num);
        if let Some(t) = list::child(ui, row.tile, ROW_METER) {
            ui.set_trait(t, "width", Value::Num(f64::from(s.condition.clamp(0.0, 1.0)) * back));
        }
    }
}

/// `s` written if it differs from what was shown in `menu`: true when it was.
fn apply(ui: &mut Ui, menu: TileId, s: Snapshot, st: &mut RepairServicesTiles) -> bool {
    if st.written_in != Some(menu) {
        st.written_in = Some(menu);
        st.shown = None;
        st.all_words = None;
    }
    if st.shown.as_ref() == Some(&s) {
        return false;
    }
    write(ui, &s, st);
    st.shown = Some(s);
    true
}

/// A tiled frame's sizes and places, as `super::item_menu::settle`.
fn settle(ui: &mut Ui, st: &mut RepairServicesTiles, write_sizes: &mut dyn FnMut(&mut Ui, &mut SizesWritten) -> usize) {
    let Some(s) = st.shown.take() else { return };
    ui.update();
    write_meters(ui, &st.rows, &s.rows);
    for _ in 0..3 {
        if write_sizes(ui, &mut st.sizes) == 0 {
            break;
        }
        ui.update();
    }
    repair_cards::fit_meters(ui, STATS, PREFIX);
    if let Some(list_tile) = ui.find_tile(LIST) {
        list::settle_list_sized(ui,list_tile,&st.rows,ROW_TEXT,&mut st.view,
            list::RowLayout {selected:s.selected,group:0,pointer_pick:s.pointer_pick,navigated:s.navigated,text_scale:st.text_scale.unwrap_or(1.0),minimum_height:51.0});
    }
    st.rewrite(ui);
    ui.update();
    st.shown = Some(s);
}

/// The marks after the tree is worked out, as `super::item_menu::mark_menu`.
pub fn mark_tiles(ui: &mut Ui) {
    super::item_menu::mark_menu(ui, REPAIR_SERVICES_MENU, &[LIST]);
}

/// A line for the log when what the menu shows changes (smoke reads these).
fn describe(ui: &Ui, s: &Snapshot, st: &RepairServicesTiles) -> String {
    let selected = s.selected.and_then(|i| s.rows.get(i));
    let tile = s.selected.and_then(|i| st.rows.get(i)).map(|r| r.tile);
    // What the fill holds after the file's own say, for a capture to compare.
    let read = |name: &str| tile.and_then(|t| list::child(ui, t, name)).and_then(|t| ui.tile(t).get("width")).map_or(0.0, Value::num);
    format!(
        "tiles: {REPAIR_SERVICES_MENU} drawn from tiles: {} rows, {} ready; selected {:?}; skill {:?}; caps {:?}; cost {:?}; beyond {}; repair all ready {} ({} caps); meter {:.0} of {:.0}; figures: {}",
        s.rows.len(),
        s.rows.iter().filter(|r| r.ready).count(),
        selected.map_or("none", |r| r.text.as_str()),
        s.skill,
        s.caps,
        s.cost.as_ref().map_or("none", |c| c.0.as_str()),
        s.beyond,
        s.all_ready,
        s.total,
        read(ROW_METER),
        read(ROW_METER_BACK),
        repair_cards::describe(ui, STATS, PREFIX),
    )
}

/// [design] The writer runs before the input. Off, it returns at once.
pub(super) fn write_system(
    mut script: ResMut<crate::script::ScriptState>,
    switch: Res<super::vanilla::TileMenus>,
    services: Option<Res<RepairServices>>,
    mut fonts: super::draw::FontKit,
    mut st: ResMut<RepairServicesTiles>,
    readability:Option<Res<crate::menu_readability::MenuReadability>>,
) {
    if !switch.repair_services {
        return;
    }
    let st = &mut *st;
    let scale=crate::menu::readability::size(readability.as_deref(),crate::menu_readability::MenuKind::Repair);
    if st.text_scale!=Some(scale) {st.text_scale=Some(scale);st.sizes.clear();st.written_in=None;st.drag=None;}
    let Some(rt) = script.rt.as_ref() else { return };
    let game = &rt.game;
    let open = services.as_ref().and_then(|s| s.0.as_ref());
    if open.is_some() && game.ui_files.is_some() && game.menu == Some(MENU_CODE) && !st.warned {
        if let Tiled::No(reason) = built(&game.ui_tree) {
            warn!("{REPAIR_SERVICES_MENU} drawn by hand: {reason}");
            st.warned = true;
        }
    }
    let Some(open) = open else {
        if st.menu.is_some() || st.shown.is_some() {
            st.close();
        }
        return;
    };
    // The frame the menu opens the model has no view yet, and nothing is drawn but its sheet.
    let shown = open.shown();
    st.menu = if tiled(true, game, true) && shown.is_some() { game.ui_tree.menu(REPAIR_SERVICES_MENU) } else { None };
    let (Some(menu), Some((view, pointer_pick, navigated))) = (st.menu, shown) else { return };
    // Compared in place, cloned only when it changed. `is_changed` would not do: the model's
    // `keys` and `draw` take the resource mutably every open frame.
    let same = st.written_in == Some(menu) && st.from.as_ref().is_some_and(|(v, p, n)| v == view && *p == pointer_pick && *n == navigated);
    let new = (!same).then(|| {
        st.from = Some((view.clone(), pointer_pick, navigated));
        Snapshot::of(view, pointer_pick, navigated, &Words::read(game))
    });
    let Some(rt) = script.rt.as_mut() else { return };
    let ui = &mut rt.game.ui_tree;
    if new.is_some() || !open.normal_owner_matches(st.normal_guard.as_ref()) {
        st.normal_guard=open.normal_guard();
    }
    let told = new.is_some_and(|s| apply(ui, menu, s, st));
    let texts = super::menus::texts(ui, menu);
    settle(ui, st, &mut |ui, sizes| {
        let size = fonts.menu_measure(&super::menus::fonts_of(ui, &texts));
        super::menus::write_text_sizes_of(ui,&texts,sizes,&|font,text,wrap|
            crate::menu::readability::measured(&size,scale,font,text,wrap))
    });
    mark_tiles(ui);
    if let (true, Some(s)) = (told, st.shown.as_ref()) {
        info!("{}", describe(ui, s, st));
    }
}

/// The marks after the scripts (a tile injected this frame is covered), and the scroll drawn:
/// after `super::update`, in this module so a menu's lines stay its own.
pub(super) fn after_update(mut script: ResMut<crate::script::ScriptState>, mut st: ResMut<RepairServicesTiles>) {
    if st.menu.is_none() {
        return;
    }
    let Some(rt) = script.rt.as_mut() else { return };
    if rt.game.ui_files.is_none() {
        return;
    }
    mark_tiles(&mut rt.game.ui_tree);
    st.check_drawn(&rt.game.ui_tree, &mut |line| info!("{line}"));
}

// Module-owned provider: the same writer/order used by TilesPlugin, without
// installing unrelated menu consumers in this actual transaction fixture.
#[cfg(test)]
pub(crate) fn install_normal_fixture_writer(app:&mut App) {
    app.init_resource::<super::draw::TileFonts>()
        .add_systems(Update,write_system.before(super::input::feed));
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::repair::RepairServicesInput as Ask;
    use fnv_ui::MemSource;

    /// [design] A fixture written for these tests, with its own dimensions and simple dependencies
    /// (#1512). It keeps the tile and trait names this module looks up, which are the engine's
    /// interface; no value or expression comes from the game's file, and it says nothing about it.
    pub(crate) fn fixture(change: impl FnOnce(String) -> String) -> Ui {
        let card = |name: &str, inner: &str| format!(r#"<rect name="{name}"><text name="CI_TitleText"/><text name="CI_ValueText"/>{inner}</rect>"#);
        let meter = |name: &str| format!(r#"<rect name="{name}"><image name="MeterImage"/></rect>"#);
        let button = |name: &str, words: &str, x: u32| format!(r#"<image name="{name}"><x>{x}</x><width>70</width><height>30</height><target>1</target><text name="button_text"><string>{words}</string></text><text name="PCShortcutLabel"/></image>"#);
        let xml = format!(
            r#"<menu name="RepairServicesMenu"><rect name="NOGLOW_BRANCH"><image name="RSM_Background"/><rect name="RSM_MainRect">
              <text name="RSM_Headline_Title"><string>Own title</string></text>
              <text name="RSM_VendorSkillLabel"/><text name="RSM_CapsLabel"/>
              <hotrect name="RSM_RepairList"><width>240</width><height>95</height><clipwindow>1</clipwindow><_scroll_delta>13</_scroll_delta>
                <_offset><copy src="child(lb_scrollbar)" trait="_current_value"/><mul src="me()" trait="_scroll_delta"/></_offset>
                <hotrect name="lb_highlight_box"/>
                <image name="lb_scrollbar"><_current_value>0</_current_value><x>230</x><width>10</width><height>95</height>
                  <image name="scrollbar_vert_marker"/><image name="scrollbar_vert_up"/><image name="scrollbar_vert_down"/>
                  <hotrect name="scrollbar_vert_page_up"/><hotrect name="scrollbar_vert_page_down"/>
                </image>
              </hotrect>
              <text name="RSM_EmptyRepairListText"><string>Own empty words</string></text>
              <image name="RSM_ItemIcon"/>
              <rect name="RSM_StatsDisplayRect">{}{}<image name="RSM_ArrowIcon"/><text name="RSM_HealthImprovementText"/><text name="RSM_StatImprovementText"/><text name="RSM_ErrorText"/>{}{}<text name="RSM_RepairCostText"/></rect>
              <rect name="RSM_ButtonRect"><y>300</y>{}{}{}</rect>
              </rect></rect>
              <template name="RSM_RepairListTemplate"><hotrect name="RSM_RepairListTemplateRect"><width>220</width><target>1</target>
                <_y>0</_y><_VerticalSpacing>8</_VerticalSpacing>
                <y><copy src="me()" trait="_y"/><sub src="parent()" trait="_offset"/></y>
                <text name="ListItemText"/><image name="RSM_Template_MeterBackground"><width>50</width></image><image name="RSM_Template_Meter"/><image name="RSM_Template_ItemMarker"/>
              </hotrect></template></menu>"#,
            card("RSM_BrokenItemHealth", &meter("RSM_BrokenItemHealthMeter")),
            card("RSM_BrokenItemStat", ""),
            card("RSM_FixedItemHealth", &meter("RSM_FixedItemHealthMeter")),
            card("RSM_FixedItemStat", ""),
            button("RSM_RepairButton", "Own mend", 0),
            button("RSM_RepairAllButton", "Own mend all", 100),
            button("RSM_DoneButton", "Own leave", 200),
        );
        let mut ui = Ui::new();
        ui.load_menu(&MemSource::default().with("fixture.xml", &change(xml)), "fixture.xml").unwrap();
        ui.update();
        ui
    }

    fn words() -> Words {
        Words { skill: "Own skill".into(), caps: "Coins".into(), condition: "State".into(), damage: "Force".into(), threshold: "Guard".into(), hints: ["[A]".into(), "[E]".into()] }
    }

    fn row(id: u32, condition: f32, quote: Quote, ready: bool) -> RowView {
        RowView { item: FormId(id), extra: Extra { condition: Some(condition), ..Extra::default() }, count: 1, name: format!("Item {id}"), label: format!("Item {id}"), condition, quote, ready, icon: None, stat: None, ysi: None }
    }

    /// Two rows ready, one beyond the vendor; the first a weapon, worn at a quarter.
    fn view() -> View {
        let mut first = row(1, 0.25, Quote::Repair { to: 0.75, cost: 40 }, true);
        first.stat = Some(("DAM", 4, 9));
        first.icon = Some("interface\\icons\\own.dds".into());
        first.extra.equipped = true;
        let mut second = row(2, 0.5, Quote::Repair { to: 1.0, cost: 7 }, true);
        second.stat = Some(("DT", 2, 3));
        second.count = 3;
        second.label = "Item 2 (3)".into();
        let third = row(3, 0.9, Quote::Beyond, false);
        View { rows: vec![first, second, third], selected: 0, first: 0, skill: 60, caps: 90, total: 61, all_ready: true, confirming: false, labels: crate::pad::Labels::Keys }
    }

    fn frame(ui: &mut Ui, s: Snapshot, st: &mut RepairServicesTiles) {
        let menu = ui.menu(REPAIR_SERVICES_MENU).unwrap();
        st.menu = Some(menu);
        apply(ui, menu, s, st);
        let texts = super::super::menus::texts(ui, menu);
        settle(ui, st, &mut |ui, sizes| super::super::menus::write_text_sizes_of(ui, &texts, sizes, &list::lines_size));
        mark_tiles(ui);
    }

    fn snapshot(v: &View) -> Snapshot {
        Snapshot::of(v, false, 0, &words())
    }

    fn number(ui: &Ui, path: &str, name: &str) -> f64 {
        ui.get_float(&format!("{path}/{name}")).unwrap()
    }

    fn string(ui: &Ui, path: &str, name: &str) -> String {
        ui.get_string(&format!("{path}/{name}")).unwrap()
    }

    fn cards(name: &str) -> String {
        format!("{STATS}/{name}")
    }

    #[test]
    fn an_incomplete_file_falls_back_whole() {
        assert_eq!(built(&fixture(|s| s)), Tiled::Yes);
        for missing in ["RSM_Background", "RSM_CapsLabel", "RSM_RepairCostText", "RSM_ArrowIcon", "RSM_FixedItemHealthMeter", "CI_ValueText", "PCShortcutLabel", "scrollbar_vert_marker", "RSM_RepairListTemplateRect", "RSM_Template_MeterBackground", "RSM_Template_ItemMarker"] {
            let ui = fixture(|s| s.replace(missing, "Absent"));
            assert!(!built(&ui).yes(), "{missing}");
        }
        assert!(!built(&Ui::new()).yes());
    }

    #[test]
    fn the_decision_wants_the_switch_the_menu_and_the_file() {
        let mut state = crate::script::ScriptState::over_no_plugins();
        let game = &mut state.rt.as_mut().unwrap().game;
        game.ui_tree = fixture(|s| s);
        game.menu = Some(MENU_CODE);
        assert!(!tiled(true, game, true), "no menu files read");
        game.ui_files = Some(Box::new(MemSource::default()));
        assert!(tiled(true, game, true));
        assert!(!tiled(false, game, true));
        assert!(!tiled(true, game, false));
        game.menu = Some(crate::script::MENU_BARTER);
        assert!(!tiled(true, game, true));
        game.menu = Some(MENU_CODE);
        let label = game.ui_tree.find_tile(CAPS).unwrap();
        game.ui_tree.unload_tile(label);
        assert!(!tiled(true, game, true));
    }

    #[test]
    fn rows_show_the_name_the_bar_the_square_and_how_dim() {
        let (mut ui, mut st) = (fixture(|s| s), RepairServicesTiles::default());
        frame(&mut ui, snapshot(&view()), &mut st);
        assert_eq!(st.rows.len(), 3);
        let child = |row: usize, name| list::child(&ui, st.rows[row].tile, name).unwrap();
        let value = |row: usize, name, what| ui.tile(child(row, name)).get(what).unwrap().num();
        assert_eq!(ui.tile(child(1, ROW_TEXT)).get("string").unwrap().text(), "Item 2 (3)");
        assert_eq!(ui.tile(st.rows[1].tile).name, "Item 2", "a row's tile is named after its item");
        assert_eq!(value(0, ROW_METER, "width"), 12.5, "a quarter of the back's 50");
        assert_eq!(value(1, ROW_METER, "width"), 25.0);
        assert_eq!(value(0, ROW_MARK, "visible"), 1.0);
        assert_eq!(value(1, ROW_MARK, "visible"), 0.0);
        assert_eq!(ui.tile(child(0, ROW_MARK)).get("filename").unwrap().text(), MARK_PICTURE);
        assert_eq!(value(0, ROW_TEXT, "alpha"), 255.0);
        assert_eq!(value(0, ROW_METER, "alpha"), 255.0);
        assert_eq!(value(0, ROW_MARK, "alpha"), 255.0);
        assert_eq!(value(2, ROW_TEXT, "alpha"), DIM_ALPHA, "beyond the vendor's skill");
        assert_eq!(value(2, ROW_METER, "alpha"), DIM_ALPHA);
        assert_eq!(value(2, ROW_MARK, "alpha"), DIM_ALPHA);
        assert!(ui.tile(child(2, ROW_METER_BACK)).get("alpha").is_none(), "the back keeps the file's");
        assert!(ui.tile(st.rows[2].tile).get("alpha").is_none(), "the row's own stays the file's");
    }

    #[test]
    fn a_marker_picture_the_file_gives_stays() {
        let mut ui = fixture(|s| s.replace(r#"<image name="RSM_Template_ItemMarker"/>"#, r#"<image name="RSM_Template_ItemMarker"><filename>own.dds</filename></image>"#));
        let mut st = RepairServicesTiles::default();
        frame(&mut ui, snapshot(&view()), &mut st);
        let mark = list::child(&ui, st.rows[0].tile, ROW_MARK).unwrap();
        assert_eq!(ui.tile(mark).get("filename").unwrap().text(), "own.dds");
    }

    #[test]
    fn the_settings_give_every_word_and_the_files_words_stay() {
        let (mut ui, mut st) = (fixture(|s| s), RepairServicesTiles::default());
        let v = view();
        frame(&mut ui, snapshot(&v), &mut st);
        assert_eq!(string(&ui, SKILL, "string"), "Own skill  60");
        assert_eq!(string(&ui, CAPS, "string"), "Coins  90");
        assert_eq!(string(&ui, COST, "string"), "Coins  40");
        assert_eq!(string(&ui, &cards("RSM_BrokenItemStat"), "_Title"), "Force", "the setting's words, for a weapon");
        // [design: LEGAL.md] What the file holds is left alone, Repair All's total after its words.
        assert_eq!(string(&ui, &format!("{REPAIR_ALL}/button_text"), "string"), "Own mend all (61)");
        assert_eq!(string(&ui, &format!("{REPAIR}/button_text"), "string"), "Own mend");
        assert_eq!(string(&ui, &format!("{DONE}/button_text"), "string"), "Own leave");
        assert_eq!(string(&ui, EMPTY, "string"), "Own empty words");
        assert_eq!(string(&ui, "RepairServicesMenu/NOGLOW_BRANCH/RSM_MainRect/RSM_Headline_Title", "string"), "Own title");
        // A word from the original's screen in no string this writes.
        let all = format!("{} {} {}", string(&ui, COST, "string"), string(&ui, SKILL, "string"), string(&ui, CAPS, "string"));
        assert!(!all.contains("Cost") && !all.contains("caps"), "{all}");
    }

    #[test]
    fn repair_alls_total_follows_the_view_and_its_words_stay_the_files() {
        let (mut ui, mut st) = (fixture(|s| s), RepairServicesTiles::default());
        let mut v = view();
        frame(&mut ui, snapshot(&v), &mut st);
        v.total = 0;
        v.all_ready = false;
        frame(&mut ui, snapshot(&v), &mut st);
        assert_eq!(string(&ui, &format!("{REPAIR_ALL}/button_text"), "string"), "Own mend all (0)", "no total piles on the last");
        // A tree built again reads its own words.
        let mut other = fixture(|s| s.replace("Own mend all", "Other words"));
        frame(&mut other, snapshot(&v), &mut st);
        assert_eq!(string(&other, &format!("{REPAIR_ALL}/button_text"), "string"), "Other words (0)");
    }

    #[test]
    fn a_stack_has_no_suffix_and_armour_takes_its_own_setting() {
        let (mut ui, mut st) = (fixture(|s| s), RepairServicesTiles::default());
        let mut v = view();
        v.selected = 1;
        frame(&mut ui, snapshot(&v), &mut st);
        assert_eq!(string(&ui, COST, "string"), "Coins  7");
        assert_eq!(string(&ui, &cards("RSM_BrokenItemStat"), "_Title"), "Guard");
        assert_eq!(string(&ui, &cards("RSM_FixedItemStat"), "_Value"), "3");
        assert_eq!(string(&ui, &cards("RSM_StatImprovementText"), "string"), "+1");
    }

    #[test]
    fn a_repair_shows_the_figures_the_icon_and_the_cost() {
        let (mut ui, mut st) = (fixture(|s| s), RepairServicesTiles::default());
        frame(&mut ui, snapshot(&view()), &mut st);
        assert_eq!(string(&ui, ICON, "filename"), "interface\\icons\\own.dds");
        assert_eq!(number(&ui, ICON, "visible"), 1.0);
        assert_eq!(number(&ui, &cards("RSM_BrokenItemHealth"), "user0"), 0.25);
        assert_eq!(string(&ui, &cards("RSM_HealthImprovementText"), "string"), "+50%");
        assert_eq!(number(&ui, COST, "visible"), 1.0);
        assert_eq!(number(&ui, COST, "alpha"), 255.0);
        assert_eq!(number(&ui, ERROR, "visible"), 0.0);
        assert_eq!(string(&ui, ERROR, "string"), "");
    }

    #[test]
    fn beyond_the_vendor_shows_our_sentence_and_no_after_pair() {
        let (mut ui, mut st) = (fixture(|s| s), RepairServicesTiles::default());
        let mut v = view();
        v.selected = 2;
        frame(&mut ui, snapshot(&v), &mut st);
        assert_eq!(number(&ui, ERROR, "visible"), 1.0);
        assert_eq!(string(&ui, ERROR, "string"), crate::repair::BEYOND_WORDS);
        assert_eq!(number(&ui, COST, "visible"), 0.0);
        assert_eq!(number(&ui, &cards("RSM_FixedItemHealth"), "visible"), 0.0);
        assert!(ui.get_float(&format!("{}/visible", cards("RSM_ArrowIcon"))).is_none(), "the arrow stays the file's");
        assert_eq!(number(&ui, REPAIR, "target"), 0.0);
    }

    #[test]
    fn a_gain_of_nothing_still_shows() {
        let (mut ui, mut st) = (fixture(|s| s), RepairServicesTiles::default());
        let mut v = view();
        v.rows[0].quote = Quote::Repair { to: 0.25, cost: 1 };
        v.rows[0].stat = Some(("DAM", 4, 4));
        frame(&mut ui, snapshot(&v), &mut st);
        assert_eq!(string(&ui, &cards("RSM_HealthImprovementText"), "string"), "+0%");
        assert_eq!(string(&ui, &cards("RSM_StatImprovementText"), "string"), "+0");
        assert_eq!(number(&ui, &cards("RSM_StatImprovementText"), "visible"), 1.0);
    }

    #[test]
    fn a_cost_too_high_is_dim() {
        let (mut ui, mut st) = (fixture(|s| s), RepairServicesTiles::default());
        let mut v = view();
        v.rows[0].ready = false;
        v.all_ready = false;
        frame(&mut ui, snapshot(&v), &mut st);
        assert_eq!(number(&ui, COST, "alpha"), DIM_ALPHA);
        assert_eq!(string(&ui, COST, "string"), "Coins  40");
        assert_eq!(number(&ui, REPAIR, "target"), 0.0);
        assert_eq!(number(&ui, REPAIR_ALL, "target"), 0.0);
        assert!(ui.get_float(&format!("{REPAIR_ALL}/brightness")).is_none(), "the file dims it, from `target`");
        assert_eq!(number(&ui, DONE, "target"), 1.0, "Done can always be pressed");
    }

    #[test]
    fn the_buttons_stand_and_carry_the_hints() {
        let (mut ui, mut st) = (fixture(|s| s), RepairServicesTiles::default());
        frame(&mut ui, snapshot(&view()), &mut st);
        for button in [REPAIR_ALL, DONE] {
            assert_eq!(number(&ui, button, "visible"), 1.0, "{button}");
            assert_eq!(number(&ui, button, "target"), 1.0, "{button}");
        }
        assert_eq!(string(&ui, REPAIR_ALL, "_PCButtonText"), "[A]");
        assert_eq!(string(&ui, DONE, "_PCButtonText"), "[E]");
    }

    #[test]
    fn repair_keeps_the_files_visible_on_a_pc() {
        let mut ui = fixture(|s| s.replace(r#"<image name="RSM_RepairButton">"#, r#"<image name="RSM_RepairButton"><visible>0</visible>"#));
        let mut st = RepairServicesTiles::default();
        let mut v = view();
        frame(&mut ui, snapshot(&v), &mut st);
        assert_eq!(number(&ui, REPAIR, "visible"), 0.0, "a row it can repair");
        assert_eq!(number(&ui, REPAIR, "target"), 1.0);
        v.selected = 2;
        frame(&mut ui, snapshot(&v), &mut st);
        assert_eq!(number(&ui, REPAIR, "visible"), 0.0, "a refused row");
        assert_eq!(number(&ui, REPAIR, "target"), 0.0);
        v.rows.clear();
        frame(&mut ui, snapshot(&v), &mut st);
        assert_eq!(number(&ui, REPAIR, "target"), 1.0, "no row");
    }

    #[test]
    fn an_empty_list_shows_the_files_words_and_nothing_else() {
        let (mut ui, mut st) = (fixture(|s| s), RepairServicesTiles::default());
        let mut v = view();
        frame(&mut ui, snapshot(&v), &mut st);
        assert_eq!(number(&ui, EMPTY, "visible"), 0.0);
        let old = st.rows[0].tile;
        v.rows.clear();
        v.all_ready = false;
        frame(&mut ui, snapshot(&v), &mut st);
        assert!(!ui.is_live(old));
        assert_eq!(number(&ui, EMPTY, "visible"), 1.0);
        assert_eq!(number(&ui, ICON, "visible"), 0.0);
        assert_eq!(number(&ui, STATS, "visible"), 0.0);
        assert_eq!(number(&ui, COST, "visible"), 0.0);
        assert_eq!(number(&ui, ERROR, "visible"), 0.0);
        assert_eq!(number(&ui, DONE, "target"), 1.0);
        assert_eq!(st.selected(), None);
    }

    #[test]
    fn rows_keep_their_tiles_and_a_row_gone_acts_on_nothing() {
        let (mut ui, mut st) = (fixture(|s| s), RepairServicesTiles::default());
        let mut v = view();
        frame(&mut ui, snapshot(&v), &mut st);
        let (first, second) = (st.rows[0].tile, st.rows[1].tile);
        assert_eq!(st.clicked(&ui, first), Some(RepairServicesTile::Row(0)));
        assert_eq!(st.row_at(0), Some(0));
        // A repair changes the row's condition and so what it stands for.
        v.rows[0].extra.condition = Some(0.75);
        frame(&mut ui, snapshot(&v), &mut st);
        assert!(!ui.is_live(first));
        assert_eq!(st.clicked(&ui, first), None);
        assert_eq!(st.rows[1].tile, second, "the other row's tile stays");
        // The tree shows rows the view has since lost.
        st.shown.as_mut().unwrap().rows.remove(0);
        assert_eq!(st.row_at(0), None);
        assert_eq!(st.row_at(9), None);
        st.close();
        assert!(st.menu.is_none());
        assert_eq!(st.clicked(&ui, second), None);
    }

    #[test]
    fn children_stand_for_actions() {
        let (mut ui, mut st) = (fixture(|s| s), RepairServicesTiles::default());
        frame(&mut ui, snapshot(&view()), &mut st);
        for (path, what) in [
            (format!("{REPAIR}/button_text"), RepairServicesTile::Repair),
            (format!("{REPAIR_ALL}/PCShortcutLabel"), RepairServicesTile::RepairAll),
            (DONE.to_string(), RepairServicesTile::Done),
            (format!("{LIST}/lb_scrollbar/scrollbar_vert_up"), RepairServicesTile::Bar(list::BarPiece::Up)),
            (format!("{LIST}/lb_scrollbar/scrollbar_vert_marker"), RepairServicesTile::Bar(list::BarPiece::Marker)),
            (CAPS.to_string(), RepairServicesTile::Other),
        ] {
            assert_eq!(st.clicked(&ui, ui.find_tile(&path).unwrap()), Some(what), "{path}");
        }
        let meter = list::child(&ui, st.rows[1].tile, ROW_METER).unwrap();
        assert_eq!(st.clicked(&ui, meter), Some(RepairServicesTile::Row(1)));
    }

    #[test]
    fn a_click_on_the_selected_row_repairs_and_on_another_chooses() {
        assert_eq!(row_input(Some(1), 1), Ask::Repair);
        assert_eq!(row_input(Some(1), 0), Ask::Select(0));
        assert_eq!(row_input(None, 0), Ask::Select(0));
    }

    #[test]
    fn a_tile_in_another_menu_is_nothing_of_this() {
        let (mut ui, mut st) = (fixture(|s| s), RepairServicesTiles::default());
        frame(&mut ui, snapshot(&view()), &mut st);
        let other = ui.load_menu(&MemSource::default().with("o.xml", r#"<menu name="Other"><rect name="RSM_DoneButton"/></menu>"#), "o.xml").unwrap();
        assert_eq!(st.clicked(&ui, other), None);
    }

    #[test]
    fn physical_keys_act_once_and_scripts_keep_the_file_bindings() {
        for key in ["A", "E", "xbuttonx", "xbuttonb", "xdown"] {
            assert!(!super::super::menus::key_click_acts(REPAIR_SERVICES_MENU, key, &[key]), "{key}");
            assert!(super::super::menus::key_click_acts(REPAIR_SERVICES_MENU, key, &[]), "{key}");
        }
    }

    /// Rows tall enough to scroll in the fixture's 95: the keys bring theirs into view, the
    /// pointer's picks leave the list where it is, and a click's pulse does not last.
    #[test]
    fn the_scroll_survives_a_rewrite_and_pointer_picks_do_not_scroll() {
        let (mut ui, mut st) = (fixture(|s| s), RepairServicesTiles::default());
        let mut v = view();
        for r in &mut v.rows {
            r.label = format!("{}|two|three|four", r.label);
        }
        frame(&mut ui, snapshot(&v), &mut st);
        assert_eq!(st.value(), 0.0);
        assert!(st.scroll_to(1.0));
        let mut s = snapshot(&v);
        s.selected = Some(2);
        s.pointer_pick = true;
        frame(&mut ui, s.clone(), &mut st);
        assert_eq!(st.value(), 1.0, "a pointer's pick leaves the list");
        let list_tile = st.list_tile(&ui).unwrap();
        list::write_value(&mut ui, list_tile, 0.0);
        st.rewrite(&mut ui);
        ui.update();
        assert_eq!(list::value_shown(&ui, list_tile), Some(1.0));
        s.pointer_pick = false;
        s.navigated += 1;
        frame(&mut ui, s, &mut st);
        assert!(st.value() >= 1.0);
        assert!(number(&ui, LIST, "_selected_height") > 0.0, "the highlight is written");
    }

    #[test]
    fn the_same_snapshot_writes_nothing_and_a_rebuilt_menu_writes_again() {
        let (mut ui, mut st) = (fixture(|s| s), RepairServicesTiles::default());
        let menu = ui.menu(REPAIR_SERVICES_MENU).unwrap();
        assert!(apply(&mut ui, menu, snapshot(&view()), &mut st));
        assert!(!apply(&mut ui, menu, snapshot(&view()), &mut st));
        let mut moved = view();
        moved.selected = 1;
        assert!(apply(&mut ui, menu, snapshot(&moved), &mut st));
        let mut ui2 = fixture(|s| s);
        let menu2 = ui2.menu(REPAIR_SERVICES_MENU).unwrap();
        st.rows.clear();
        assert!(apply(&mut ui2, menu2, snapshot(&moved), &mut st), "a tree built again takes what was written with it");
    }

    #[test]
    fn the_log_line_says_what_smoke_reads() {
        let (mut ui, mut st) = (fixture(|s| s), RepairServicesTiles::default());
        let s = snapshot(&view());
        frame(&mut ui, s.clone(), &mut st);
        let line = describe(&ui, &s, &st);
        assert!(line.starts_with("tiles: RepairServicesMenu drawn from tiles: 3 rows, 2 ready; selected \"Item 1\";"), "{line}");
        assert!(line.contains("repair all ready true (61 caps)"), "{line}");
        assert!(line.contains("meter 12 of 50") || line.contains("meter 13 of 50"), "{line}");
        assert!(line.contains("Force 4"), "{line}");
    }

    #[test]
    fn ysi_row_icons_attach_when_ysi_xml_is_present_and_noop_when_absent() {
        let mut v = view();
        v.rows[0].ysi = Some(fnv_game::ui_ysi::YsiItemDisplay {
            tag: Some("(f01)".into()),
            icon: Some("icons\\pistol.dds".into()),
            template: fnv_game::ui_ysi::YSI_DEFAULT_TEMPLATE.into(),
        });

        // Without ySI.xml: complete no-op, zero ySIImage tiles created.
        let (mut plain_ui, mut plain_st) = (fixture(|s| s), RepairServicesTiles::default());
        frame(&mut plain_ui, snapshot(&v), &mut plain_st);
        assert!(list::child(&plain_ui, plain_st.rows[0].tile, fnv_game::ui_ysi::YSI_IMAGE_TILE).is_none());

        // With ySI.xml injected into RepairServicesMenu: tagged row gets ySIDefault + filename,
        // untagged row gets ySINoTag, and clicking ySIImage maps to Row(0).
        let ysi_xml = r#"
<rect name="ySI">
  <_ySI> 1 </_ySI>
  <_ySIBlanks> 1 </_ySIBlanks>
  <_ySIWidth> 24 </_ySIWidth>
  <_ySIHeight> 24 </_ySIHeight>
</rect>
<template name="ySIDefault">
  <image name="ySIImage">
    <filename> icons\default.dds </filename>
    <width> <copy src="ySI" trait="_ySIWidth"/> </width>
    <height> <copy src="ySI" trait="_ySIHeight"/> </height>
    <visible> 1 </visible>
  </image>
</template>
<template name="ySINoTag">
  <image name="ySIImage">
    <filename> icons\blank.dds </filename>
    <width> <copy src="ySI" trait="_ySIWidth"/> <onlyif src="ySI" trait="_ySIBlanks"/> </width>
    <height> <copy src="ySI" trait="_ySIHeight"/> </height>
    <visible> 1 </visible>
  </image>
</template>
"#;
        let (mut ui, mut st) = (fixture(|s| s), RepairServicesTiles::default());
        let src = MemSource::default().with(fnv_game::ui_ysi::YSI_XML_PATH, ysi_xml);
        let root = ui.menu(REPAIR_SERVICES_MENU).unwrap();
        assert!(fnv_game::ui_ysi::publish_for_menu(&mut ui, &src, root, Some(MENU_CODE)));
        ui.update();
        frame(&mut ui, snapshot(&v), &mut st);

        let img0 = list::child(&ui, st.rows[0].tile, fnv_game::ui_ysi::YSI_IMAGE_TILE).expect("tagged row icon");
        let img1 = list::child(&ui, st.rows[1].tile, fnv_game::ui_ysi::YSI_IMAGE_TILE).expect("untagged row icon");
        assert_eq!(ui.tile(img0).get("filename").map(Value::text).as_deref(), Some("icons\\pistol.dds"));
        assert_eq!(ui.tile(img1).get("filename").map(Value::text).as_deref(), Some("icons\\blank.dds"));
        assert_eq!(st.clicked(&ui, img0), Some(RepairServicesTile::Row(0)));
    }
}

