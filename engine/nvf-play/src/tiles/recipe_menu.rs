//! The recipe menu drawn from `menus\recipe_menu.xml`, behind `[UI] TileMenus=RecipeMenu`
//! [design: crafting.md section 4]. Off (the default), none of this runs and the hand-built
//! menu (`crate::crafting`) draws. The rules stay `crate::crafting`'s: what is listed, its
//! order, the filter, what Accept makes and every sound. This module writes the model's view
//! as the file's "set in code" traits and says what a click on a tile stands for; what it
//! asks goes through `crate::crafting::RecipeInput` to the model's `keys`.
//!
//! - **A frame**, as the container's ([`write_system`], before `super::input::feed`): what
//!   changed written, the texts measured, the two lists placed, the tree worked out again.
//! - **The "How many?" prompt stays hand-built** over the tiles (`crate::crafting::draw`);
//!   under it this menu takes nothing (`super::item_menu::takes_input`).
//! - **What is left to the file**: the titles' words (`sMadeAt`, `sSkillRequirement`,
//!   `sIngredients`, `sRecipes`, Accept's and Exit's), every place and picture. A subcategory's
//!   name replaces the list's title while it filters; the file's title comes back after it.
//! - **The original was captured at 1280 x 720** (crafting.md section 4).
//! - **An ingredient's row takes no pointer** [observed: no hover and no box, though its `target`
//!   reads 1 under a list with `target` 0]. This tree's hit test would answer it, so its `target`
//!   is written 0 here, and its rows stay out of [`list::hit`] [design].
//! - **What the file does itself is not written**: it dims Accept from `target` through
//!   `_line_alpha` [documented: text_box.xml], and `_IsContainerListSelected` keeps the file's
//!   value [observed: -1 in every state read].

use super::item_menu::{mark_menu, on, set};
use super::list::{self, BarPiece, Drag, Hit, ListView, RowTile, Wanted};
use super::menus::{SizesWritten, Tiled};
use crate::crafting::{Open, RecipeInput, View};
use crate::pipboy::items::Info;
use bevy::prelude::*;
use fnv_sim::FormId;
use fnv_ui::{TileId, Ui, Value};

/// [documented: menus\recipe_menu.xml] The menu and the paths written by this adapter.
pub const RECIPE_MENU: &str = "RecipeMenu";
const BACKGROUND: &str = "RecipeMenu/NOGLOW_BRANCH/RM_Background";
const ICON: &str = "RecipeMenu/NOGLOW_BRANCH/RM_ItemIcon";
const CARD: &str = "RecipeMenu/NOGLOW_BRANCH/RM_ItemData";
const TITLE: &str = "RecipeMenu/NOGLOW_BRANCH/RM_ItemsRect/RM_ItemsTitle";
const LEFT_ARROW: &str = "RecipeMenu/NOGLOW_BRANCH/RM_ItemsRect/RM_Items_LeftFilterArrow";
const RIGHT_ARROW: &str = "RecipeMenu/NOGLOW_BRANCH/RM_ItemsRect/RM_Items_RightFilterArrow";
const MADE_AT: &str = "RecipeMenu/NOGLOW_BRANCH/RM_ContainerRect/RM_MadeAtVariable";
const SKILL_HEADER: &str = "RecipeMenu/NOGLOW_BRANCH/RM_ContainerRect/RM_SkillRequirementHeader";
const SKILL: &str = "RecipeMenu/NOGLOW_BRANCH/RM_ContainerRect/RM_SkillRequirement";
const ACCEPT: &str = "RecipeMenu/NOGLOW_BRANCH/RM_ButtonRect/RM_ButtonX";
const EXIT: &str = "RecipeMenu/NOGLOW_BRANCH/RM_ButtonRect/RM_ButtonB";
const TEMPLATE: &str = "RM_list_template";
const ROW: &str = "RM_list_template_container";
const ROW_TEXT: &str = "ListItemText";
const ROW_VALUE: &str = "RM_list_template_ItemValue";
const ROW_MARK: &str = "RM_list_template_ItemMarker";
/// The recipes' list, then the ingredients': a `text` of the ingredients' name stands before
/// that list in the file, so its path counts from the last of the name (`fnv_ui`'s `:N`)
/// [documented: the file].
const LISTS: [&str; 2] = ["RecipeMenu/NOGLOW_BRANCH/RM_ItemsRect/RM_Items_InventoryList", "RecipeMenu/NOGLOW_BRANCH/RM_ContainerRect/RM_Items_IngredientList:0"];
const LIST_NAMES: [&str; 2] = ["RM_Items_InventoryList", "RM_Items_IngredientList"];
/// A place in [`LISTS`].
pub const RECIPES: usize = 0;
const INGREDIENTS: usize = 1;

/// The `alpha` of a requirement the player cannot meet yet: a recipe's or an ingredient's
/// `ListItemText` and the skill line, at `brightness` 255 [observed]. Accept's 128 is the file's.
const DIM: f64 = 127.0;

/// [design: `crate::crafting::keys`, `crate::pad::menu_buttons`] Physical keys the model's
/// handler already takes. A script's MenuTapKey still follows the file's bindings.
pub const HAND_BUILT_KEYS: [&str; 17] = ["A", "X", "W", "S", "E", "xup", "xdown", "xleft", "xright", "xbuttona", "xbuttonb", "xbuttonx", "xbuttonlb", "xbuttonrb", "xbuttonlt", "xbuttonrt", "xbuttonstart"];

/// What a click on the menu's tiles stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecipeTile {
    Row(usize),
    Arrow(i32),
    Title,
    Accept,
    Exit,
    /// The list's place in [`LISTS`], the scrollbar's piece.
    Bar(usize, BarPiece),
    /// Another tile of the menu, an ingredient's row among them: nothing to do.
    Other,
}

const TARGETS: [(&str, RecipeTile); 5] = [
    ("RM_ButtonX", RecipeTile::Accept),
    ("RM_ButtonB", RecipeTile::Exit),
    ("RM_Items_LeftFilterArrow", RecipeTile::Arrow(-1)),
    ("RM_Items_RightFilterArrow", RecipeTile::Arrow(1)),
    ("RM_ItemsTitle", RecipeTile::Title),
];

/// [design] A short or restructured file keeps the entire hand-built view.
pub fn built(ui: &Ui) -> Tiled {
    let mut paths: Vec<String> = [RECIPE_MENU, BACKGROUND, ICON, CARD, TITLE, LEFT_ARROW, RIGHT_ARROW, MADE_AT, SKILL_HEADER, SKILL, ACCEPT, EXIT].into_iter().map(str::to_owned).collect();
    for button in [ACCEPT, EXIT] {
        paths.extend(["button_text", "PCShortcutLabel"].map(|t| format!("{button}/{t}")));
    }
    for path in LISTS {
        paths.push(format!("{path}/{}", list::HIGHLIGHT));
        for piece in [list::MARKER, list::UP, list::DOWN, list::PAGE_UP, list::PAGE_DOWN] {
            paths.push(format!("{path}/{}/{piece}", list::SCROLLBAR));
        }
    }
    paths.extend(super::item_card::CARDS.iter().chain(&[super::item_card::EFFECTS, super::item_card::CONDITION_ARROWS, super::item_card::CONDITION_METER, super::item_card::CONDITION_METER_IMAGE]).map(|c| format!("{CARD}/{c}")));
    for card in super::item_card::CARDS.iter().chain(&[super::item_card::EFFECTS]) {
        paths.extend(super::item_card::CARD_TEXTS.map(|t| format!("{CARD}/{card}/{t}")));
    }
    for path in &paths {
        if ui.find_tile(path).is_none() {
            return Tiled::No(format!("{path} missing"));
        }
    }
    list::template_has(ui, RECIPE_MENU, TEMPLATE, ROW, &[("text", ROW_TEXT), ("text", ROW_VALUE), ("image", ROW_MARK)])
}

/// Whether this file's menu is the one up: the first half of the writer's decision, which the
/// drawing and the input read ([`RecipeTiles::menu`]) [design].
fn up(game: &fnv_game::Game<'_>) -> bool {
    game.ui_files.is_some() && game.menu == Some(fnv_game::crafting::RECIPE_MENU)
}

/// A row as shown, in either list.
#[derive(Clone, Debug, PartialEq)]
pub struct RowShown {
    /// The recipe, or the ingredient.
    pub key: FormId,
    /// The tile's name, without the numbers an ingredient's words change by [hypothesis: as a
    /// container row's, `super::item_menu::RowShown`].
    pub name: String,
    pub text: String,
    /// Can be made now; there is enough of the ingredient.
    pub ready: bool,
    /// Resolved ySI tag/icon/template for this row when ySI is active (`None` otherwise).
    pub ysi: Option<fnv_game::ui_ysi::YsiItemDisplay>,
}

/// One whole state of the model, as the menu shows it. A plain snapshot, so tests need no world.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub rows: Vec<RowShown>,
    /// The row with the highlight.
    pub selected: Option<usize>,
    /// The recipes the filter leaves out.
    pub filtered: usize,
    /// Which filter: a new one starts the list at the top.
    pub filter: u32,
    /// The subcategory's record name while it filters; none: the file's title stands.
    pub subcategory: Option<String>,
    /// Where the chosen recipe is made: its category's record name.
    pub made_at: String,
    /// "{skill} ({have}/{need})", and whether the player has it.
    pub skill: Option<(String, bool)>,
    pub ingredients: Vec<RowShown>,
    pub card: Option<Info>,
    pub icon: Option<String>,
    /// Accept's and Exit's key hints.
    pub hints: [String; 2],
    /// The pointer last moved the selection: the list stays where it is.
    pub pointer_pick: bool,
    /// A count of the keys' and the wheel's moves.
    pub navigated: u32,
}

impl Snapshot {
    /// What the menu shows of the model's view: its rows, the chosen recipe's figures as the
    /// hand-built menu's (`crate::crafting::spawn`), and the two words the file leaves to code.
    pub(crate) fn of(v: &View, open: &Open, hints: [String; 2]) -> Self {
        let selected = (!v.rows.is_empty()).then_some(v.selected);
        // The hand-built WG and VAL, and its effects' text; the other cards stay hidden.
        let card = selected.map(|_| Info { weight: v.weight.map(crate::crafting::weight_text), value: v.value.map(|x| x.to_string()), effects: v.effects.clone(), ..Default::default() });
        Snapshot {
            rows: v.rows.iter().map(|r| RowShown { key: r.recipe, name: r.label.clone(), text: r.label.clone(), ready: r.ready, ysi: r.ysi.clone() }).collect(),
            selected,
            filtered: v.filtered,
            filter: open.filter as u32,
            subcategory: v.subcategory.clone(),
            made_at: v.made_at.clone(),
            skill: v.skill.as_ref().zip(v.skill_record.as_ref()).map(|((_, have, need), name)| (format!("{name} ({have}/{need})"), have >= need)),
            ingredients: v
                .ingredients
                .iter()
                .enumerate()
                .map(|(idx, (key, name, have, need))| RowShown {
                    key: *key,
                    name: name.clone(),
                    text: format!("{name} ({have}/{need})"),
                    ready: have >= need,
                    ysi: v.ingredient_ysi.get(idx).cloned().flatten(),
                })
                .collect(),
            card,
            icon: v.icon.clone(),
            hints,
            pointer_pick: open.pointer_pick,
            navigated: open.navigated,
        }
    }

    fn ready(&self) -> bool {
        self.selected.and_then(|i| self.rows.get(i)).is_some_and(|r| r.ready)
    }
}

/// The tile menu's state: what input and the writer share.
#[derive(Resource, Default)]
pub struct RecipeTiles {
    /// The menu while it is drawn from tiles (the writer's decision this frame).
    pub menu: Option<TileId>,
    /// By place in [`LISTS`].
    rows: [Vec<RowTile<FormId>>; 2],
    views: [ListView<FormId>; 2],
    shown: Option<Snapshot>,
    written_in: Option<TileId>,
    text_scale:Option<f32>,
    normal_guard:Option<crate::menu::normal_services::Guard>,
    normal_revision:u64,
    /// The file's title, once a subcategory's name has stood over it.
    file_title: Option<String>,
    /// [`built`]'s answer, and the menu and tree shape it was worked out on.
    built_on: Option<(Option<TileId>, u64, Tiled)>,
    sizes: SizesWritten,
    /// A press on a marker, held.
    pub drag: Option<Drag>,
    /// The scroll last logged as drawn.
    drawn: [Option<f64>; 2],
    /// [design] The fallback's reason is warned once per session.
    warned: bool,
}

impl RecipeTiles {
    pub(crate) fn normal_revision(&self)->u64 {self.normal_revision}
    pub(crate) fn normal_hit(&self,ui:&Ui,frame:crate::menu::barter_touch::Frame,
        guard:&crate::menu::normal_services::Guard,point:Vec2,size:f32)->Option<crate::menu::normal_services::Hit> {
        use crate::menu::{normal_services::{Action,Hit as NormalHit},normal_touch::Source};
        if self.text_scale.unwrap_or(1.0)!=size || !self.normal_guard.as_ref().is_some_and(|g|g.same(guard)){return None}
        let menu=self.menu?;
        let at=super::input::to_menu_units(point,frame.size,Vec2::splat(0.5));
        let candidates=fnv_ui::input::hit_candidates(ui,menu,&fnv_ui::input::no_measure);
        if let Some(picked)=fnv_ui::input::top_hit(ui,&candidates,at) {
            let what=self.clicked(ui,picked.tile)?;
            let (action,pane,key)=match what {
                RecipeTile::Row(index)=>{let i=self.row_at(index)?;(Action::Row(i),Some(RECIPES),self.rows[RECIPES].get(index)?.tile)}
                RecipeTile::Arrow(by)=>(Action::Workbench(RecipeInput::Filter(by)),None,picked.tile),
                RecipeTile::Title=>(Action::CategoryHeader,None,picked.tile),
                RecipeTile::Accept=>(Action::Workbench(RecipeInput::Accept),None,picked.tile),
                RecipeTile::Exit=>(Action::Workbench(RecipeInput::Exit),None,picked.tile),
                _=>{for pane in [RECIPES,INGREDIENTS] {
                    if let Some(hit)=self.list_tile(ui,pane).and_then(|list|crate::menu::normal_services::native_pane(ui,frame,list,pane,point)){return Some(hit)}
                }return None},
            };
            return Some(NormalHit {source:Source::Tile(menu,key),action,pane,row_step:0.0})
        }
        // Padded touch hit boxes around category filter arrows and title bar.
        const HEADER_PAD: f64 = 16.0;
        for c in candidates.iter().rev() {
            let r = c.region();
            if r.width <= 0.0 || r.height <= 0.0 {
                continue;
            }
            if at.0 < r.left - HEADER_PAD || at.0 >= r.left + r.width + HEADER_PAD
                || at.1 < r.top - HEADER_PAD || at.1 >= r.top + r.height + HEADER_PAD {
                continue;
            }
            match self.clicked(ui, c.under) {
                Some(RecipeTile::Arrow(by)) => {
                    return Some(NormalHit {
                        source: Source::Direct(menu, c.under),
                        action: Action::Workbench(RecipeInput::Filter(by)),
                        pane: None,
                        row_step: 0.0,
                    });
                }
                Some(RecipeTile::Title) => {
                    return Some(NormalHit {
                        source: Source::Direct(menu, c.under),
                        action: Action::CategoryHeader,
                        pane: None,
                        row_step: 0.0,
                    });
                }
                _ => {}
            }
        }
        for pane in [RECIPES,INGREDIENTS] {
            if let Some(hit)=self.list_tile(ui,pane).and_then(|list|crate::menu::normal_services::native_pane(ui,frame,list,pane,point)){return Some(hit)}
        }
        None
    }
    pub(crate) fn normal_scroll(&mut self,ui:&mut Ui,pane:usize,pixels:f64,frame:crate::menu::barter_touch::Frame,size:f32) {
        if pane>=LISTS.len() || self.text_scale.unwrap_or(1.0)!=size || self.menu.is_none(){return}
        let Some(list)=self.list_tile(ui,pane) else{return};
        let Some(delta)=ui.tile(list).get("_scroll_delta").map(Value::num).filter(|v|v.is_finite() && *v>0.0) else{return};
        let a=super::input::to_menu_units(Vec2::ZERO,frame.size,Vec2::splat(0.5));
        let b=super::input::to_menu_units(Vec2::Y,frame.size,Vec2::splat(0.5));
        let next=self.value(pane)+pixels*(b.1-a.1)/delta;
        if next.is_finite() && self.scroll_to(pane,next){self.rewrite(ui);ui.update();}
    }

    /// The menu closed: nothing of it stays for the next (the warning does).
    fn close(&mut self) {
        *self = RecipeTiles { warned: self.warned, ..Default::default() };
    }

    /// [`built`], worked out again only when the tree's shape changed: it reads tiles' names and
    /// the template, and writes many paths, on every frame the menu is up.
    fn built_now(&mut self, ui: &Ui) -> Tiled {
        let (menu, shape) = (ui.menu(RECIPE_MENU), ui.shape());
        match &self.built_on {
            Some((m, s, answer)) if *m == menu && *s == shape => answer.clone(),
            _ => {
                let answer = built(ui);
                self.built_on = Some((menu, shape, answer.clone()));
                answer
            }
        }
    }

    /// Whether what `s` shows differs from what was written in `menu`, and if so that it is now.
    fn is_new(&mut self, menu: TileId, s: &Snapshot) -> bool {
        if self.written_in != Some(menu) {
            self.written_in = Some(menu);
            self.shown = None;
            self.file_title = None;
        }
        self.shown.as_ref() != Some(s)
    }

    /// The tile a click on `tile` stands for, while the menu is drawn from its tiles. None
    /// otherwise: off or in the fallback a script's queued click on the file's tile acts on
    /// nothing.
    pub fn clicked(&self, ui: &Ui, tile: TileId) -> Option<RecipeTile> {
        let menu = self.menu?;
        if !ui.is_live(tile) || ui.root_of(tile) != menu {
            return None;
        }
        // An ingredient's rows are not given: they stand for nothing (module notes).
        let lists = [(LIST_NAMES[RECIPES], self.rows[RECIPES].as_slice()), (LIST_NAMES[INGREDIENTS], [].as_slice())];
        Some(match list::hit(ui, tile, &lists, &TARGETS) {
            Hit::Row(_, index) => RecipeTile::Row(index),
            Hit::Bar(at, piece) => RecipeTile::Bar(at, piece),
            Hit::Target(what) => what,
            Hit::Other => RecipeTile::Other,
        })
    }

    /// The row `index` when the tile there still stands for the snapshot's row there; a tile
    /// whose row moved or left the list acts on nothing.
    pub fn row_at(&self, index: usize) -> Option<usize> {
        let key = self.rows[RECIPES].get(index)?.key;
        (self.shown.as_ref()?.rows.get(index)?.key == key).then_some(index)
    }

    /// The row with the highlight.
    pub fn selected(&self) -> Option<usize> {
        self.shown.as_ref()?.selected
    }

    /// The list tile at a place in [`LISTS`].
    pub fn list_tile(&self, ui: &Ui, at: usize) -> Option<TileId> {
        ui.find_tile(LISTS[at])
    }

    /// The place in [`LISTS`] of a list's tile.
    pub fn list_of(&self, ui: &Ui, tile: TileId) -> Option<usize> {
        (0..LISTS.len()).find(|&at| self.list_tile(ui, at) == Some(tile))
    }

    /// A list's scroll now.
    pub fn value(&self, at: usize) -> f64 {
        self.views[at].value
    }

    /// Set a list's scroll, held in its bar's range: the view's own, no command of the model's.
    pub fn scroll_to(&mut self, at: usize, value: f64) -> bool {
        self.views[at].scroll_to(value)
    }

    /// The scroll and the highlight written from the views: after the frame's places, and after
    /// the input's step and its clicks (`super::input::feed`).
    pub fn rewrite(&self, ui: &mut Ui) {
        for (path, view) in LISTS.iter().zip(&self.views) {
            if let Some(tile) = ui.find_tile(path) {
                view.rewrite(ui, tile);
            }
        }
    }

    /// Logged when the scroll drawn changes, and when it is not the value written (smoke reads
    /// both): the view's value must outlast a click's pulse.
    pub fn check_drawn(&mut self, ui: &Ui, log: &mut dyn FnMut(String)) {
        for (at, name) in ["recipes", "ingredients"].into_iter().enumerate() {
            let Some(tile) = self.list_tile(ui, at) else { continue };
            let Some(v) = list::value_shown(ui, tile) else { continue };
            let n = |x: f64| Value::Num(x).text();
            if self.drawn[at] != Some(v) {
                log(format!("tiles: {RECIPE_MENU} {name} list scrolled to {} of {}", n(v), n(self.views[at].counts.most())));
                self.drawn[at] = Some(v);
            }
            if v != self.views[at].value {
                log(format!("tiles: {RECIPE_MENU} {name} list drawn at {}, not the {} written", n(v), n(self.views[at].value)));
            }
        }
    }
}

/// What a click on the recipe list's row `index` asks: the selected row, Accept; another, it
/// selected [design: the plan of #1468].
pub fn row_input(selected: Option<usize>, index: usize) -> RecipeInput {
    if selected == Some(index) {
        RecipeInput::Accept
    } else {
        RecipeInput::Select(index)
    }
}

/// Make the list's rows the rows `shown`, each with its words and brightness.
fn write_rows(ui: &mut Ui, path: &str, rows: &mut Vec<RowTile<FormId>>, shown: &[RowShown]) {
    let wanted: Vec<Wanted<'_, FormId>> = shown.iter().map(|r| Wanted { key: &r.key, name: &r.name }).collect();
    list::reconcile(ui, path, TEMPLATE, rows, &wanted);
    for (row, r) in rows.iter().zip(shown) {
        if let Some(t) = list::child(ui, row.tile, ROW_TEXT) {
            ui.set_trait(t, "string", Value::Str(r.text.clone()));
            ui.set_trait(t, "alpha", Value::Num(if r.ready { 255.0 } else { DIM }));
        }
        if let Some(t) = list::child(ui, row.tile, ROW_VALUE) {
            ui.set_trait(t, "string", Value::Str(String::new()));
        }
        if let Some(t) = list::child(ui, row.tile, ROW_MARK) {
            ui.set_trait(t, "visible", on(false));
        }
        fnv_game::ui_ysi::apply_row_icon(ui, &fnv_ui::MemSource::default(), row.tile, r.ysi.as_ref());
    }
}

/// The ingredients as rows of the one template, the whole of each in its `ListItemText`
/// [observed: crafting.md section 4].
fn write_ingredients(ui: &mut Ui, s: &Snapshot, st: &mut RecipeTiles) {
    write_rows(ui, LISTS[INGREDIENTS], &mut st.rows[INGREDIENTS], &s.ingredients);
    for row in &st.rows[INGREDIENTS] {
        ui.set_trait(row.tile, "target", on(false));
    }
}

/// A subcategory's name over the file's title while it filters; the file's again after it.
fn write_title(ui: &mut Ui, s: &Snapshot, st: &mut RecipeTiles) {
    match &s.subcategory {
        Some(name) => {
            if st.file_title.is_none() {
                st.file_title = ui.get_string(&format!("{TITLE}/string"));
            }
            set(ui, TITLE, "string", Value::Str(name.clone()));
        }
        None => {
            if let Some(title) = st.file_title.take() {
                set(ui, TITLE, "string", Value::Str(title));
            }
        }
    }
}

/// [design] File places and textures stay in the tree; selection and readiness are the model's.
fn write(ui: &mut Ui, s: &Snapshot, st: &mut RecipeTiles) {
    write_rows(ui, LISTS[RECIPES], &mut st.rows[RECIPES], &s.rows);
    write_ingredients(ui, s, st);
    set(ui, LISTS[RECIPES], "_num_filtered", Value::Num(s.filtered as f64));
    write_title(ui, s, st);
    set(ui, MADE_AT, "string", Value::Str(s.made_at.clone()));
    // Both stay shown, the skill's text empty with nothing to say [observed: `visible` 1 on the
    // skill line in every state read; a recipe needing no skill was not seen].
    set(ui, SKILL_HEADER, "visible", on(true));
    set(ui, SKILL, "visible", on(true));
    let (skill, met) = s.skill.as_ref().map_or(("", true), |(text, met)| (text.as_str(), *met));
    set(ui, SKILL, "string", Value::Str(skill.to_owned()));
    set(ui, SKILL, "alpha", Value::Num(if met { 255.0 } else { DIM }));
    // [observed] Both buttons' `visible` 1 always. Accept's `target` is all it takes: the file
    // dims it from there [documented: text_box.xml; observed: its brightness stays 255].
    set(ui, ACCEPT, "visible", on(true));
    set(ui, EXIT, "visible", on(true));
    set(ui, ACCEPT, "target", on(s.ready()));
    for (button, hint) in [ACCEPT, EXIT].into_iter().zip(&s.hints) {
        set(ui, button, "_PCButtonText", Value::Str(hint.clone()));
    }
    set(ui, ICON, "visible", on(s.card.is_some() && s.icon.is_some()));
    set(ui, ICON, "filename", Value::Str(s.icon.clone().unwrap_or_default()));
    super::item_card::write(ui, CARD, s.card.as_ref());
    // [observed] The recipe capture reads the container's number here too, once, at the opening.
    set(ui, BACKGROUND, "alpha", Value::Num(super::container_menu::BACKGROUND_ALPHA));
}

/// A tiled frame's sizes and places, after what changed was written: as
/// `super::item_menu::settle`, for the two lists. The ingredients' scroll starts at the top for
/// each recipe.
fn settle(ui: &mut Ui, st: &mut RecipeTiles, write_sizes: &mut dyn FnMut(&mut Ui, &mut SizesWritten) -> usize) {
    let Some(s) = st.shown.take() else { return };
    ui.update();
    for _ in 0..3 {
        if write_sizes(ui, &mut st.sizes) == 0 {
            break;
        }
        ui.update();
    }
    if let Some(tile) = ui.find_tile(LISTS[RECIPES]) {
        list::settle_list_sized(ui,tile,&st.rows[RECIPES],ROW_TEXT,&mut st.views[RECIPES],
            list::RowLayout {selected:s.selected,group:s.filter,pointer_pick:s.pointer_pick,navigated:s.navigated,text_scale:st.text_scale.unwrap_or(1.0),minimum_height:52.0});
    }
    if let Some(tile) = ui.find_tile(LISTS[INGREDIENTS]) {
        let recipe = s.selected.and_then(|i| s.rows.get(i)).map_or(0, |r| r.key.0);
        list::settle_list_sized(ui,tile,&st.rows[INGREDIENTS],ROW_TEXT,&mut st.views[INGREDIENTS],
            list::RowLayout {selected:None,group:recipe,pointer_pick:false,navigated:0,text_scale:st.text_scale.unwrap_or(1.0),minimum_height:52.0});
    }
    st.shown = Some(s);
    st.rewrite(ui);
    ui.update();
}

/// The marks after the scripts (`super::item_menu::mark_tiles`).
pub fn mark_tiles(ui: &mut Ui) {
    mark_menu(ui, RECIPE_MENU, &LISTS);
}

/// [design] The writer runs before input. The model's view is read here on each frame the menu
/// is drawn from tiles, as the hand-built drawing reads it on each of its frames.
#[allow(clippy::too_many_arguments)]
pub(super) fn write_system(
    mut script: ResMut<crate::script::ScriptState>, switch: Res<super::vanilla::TileMenus>,
    mut recipe: ResMut<crate::crafting::RecipeMenu>, mut sim: ResMut<crate::sim::SimState>,
    play: Option<Res<crate::PlayWorld>>, mut forms: Local<Option<crate::pipboy::stats::read::Forms>>,
    mut fonts: super::draw::FontKit, mut st: ResMut<RecipeTiles>,
    readability:Option<Res<crate::menu_readability::MenuReadability>>,
) {
    if !switch.recipe {
        return;
    }
    let scale=crate::menu::readability::size(readability.as_deref(),crate::menu_readability::MenuKind::Workbench);
    if st.text_scale!=Some(scale) {st.text_scale=Some(scale);st.sizes.clear();st.written_in=None;st.drag=None;}
    let Some(rt) = script.rt.as_ref() else { return };
    let game = &rt.game;
    if !recipe.is_open() {
        if st.menu.is_some() || st.shown.is_some() {
            st.close();
        }
        return;
    }
    st.menu = None;
    if up(game) {
        match st.built_now(&game.ui_tree) {
            Tiled::Yes => st.menu = game.ui_tree.menu(RECIPE_MENU),
            Tiled::No(reason) if !st.warned => {
                warn!("{RECIPE_MENU} drawn by hand: {reason}");
                st.warned = true;
            }
            Tiled::No(_) => {}
        }
    }
    let Some(menu) = st.menu else { return };
    let word = |key| fnv_game::ui_loading::game_string(game, key);
    let pad = crate::pad::at_menu(fnv_game::crafting::RECIPE_MENU);
    let hints = [
        crate::pad::game_hint(&word("sPCMenuHintA"), &[KeyCode::KeyA], pad),
        crate::pad::game_hint(&word("sPCMenuHintX"), &[KeyCode::KeyX, KeyCode::Escape], pad),
    ];
    if forms.is_none() {
        *forms = play.as_ref().map(|p| crate::pipboy::stats::read::Forms::new(p.lo, p.schema));
    }
    let Some(open) = recipe.0.as_mut() else { return };
    let Some(rt) = script.rt.as_mut() else { return };
    let v = crate::crafting::view_with_effects(rt, &mut sim, open, forms.as_mut());
    let s = Snapshot::of(&v, open, hints);
    open.selected = v.selected;
    open.drawn = Some(v);
    let ui = &mut rt.game.ui_tree;
    let told = st.is_new(menu, &s);
    if told || !open.normal_owner_matches(st.normal_guard.as_ref()) {
        st.normal_guard=open.normal_native_guard();st.normal_revision=st.normal_revision.wrapping_add(1);
    }
    if told {
        write(ui, &s, &mut st);
        st.shown = Some(s);
    }
    let texts = super::menus::texts(ui, menu);
    settle(ui, &mut st, &mut |ui, sizes| {
        let size = fonts.menu_measure(&super::menus::fonts_of(ui, &texts));
        super::menus::write_text_sizes_of(ui,&texts,sizes,&|font,text,wrap|
            crate::menu::readability::measured(&size,scale,font,text,wrap))
    });
    mark_tiles(ui);
    if let (true, Some(s)) = (told, st.shown.as_ref()) {
        let selected = s.selected.and_then(|i| s.rows.get(i)).map_or("none", |r| r.text.as_str());
        info!("tiles: {RECIPE_MENU} drawn from tiles: {} rows, {} ready; filtered {}; selected {selected:?}; ingredients {}; figures: {}",
            s.rows.len(), s.rows.iter().filter(|r| r.ready).count(), s.filtered, s.ingredients.len(), super::item_card::describe(ui, CARD));
    }
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
    use fnv_ui::MemSource;

    /// [design] A fixture written for these tests, with its own dimensions and simple
    /// dependencies. No expressions or values were taken from the game's menu file; the names
    /// are the ones this adapter looks up.
    pub(crate) fn fixture(change: impl FnOnce(String) -> String) -> Ui {
        let list = |name: &str, height: u32| format!(r#"
            <hotrect name="{name}"><height>{height}</height><width>200</width>
              <clipwindow>1</clipwindow><_scroll_delta>11</_scroll_delta>
              <_offset><copy src="child(lb_scrollbar)" trait="_current_value"/><mul src="me()" trait="_scroll_delta"/></_offset>
              <hotrect name="lb_highlight_box"/>
              <image name="lb_scrollbar"><_current_value>0</_current_value>
                <image name="scrollbar_vert_marker"/><image name="scrollbar_vert_up"/><image name="scrollbar_vert_down"/>
                <hotrect name="scrollbar_vert_page_up"/><hotrect name="scrollbar_vert_page_down"/>
              </image>
            </hotrect>"#);
        let cards: String = super::super::item_card::CARDS.into_iter().chain([super::super::item_card::EFFECTS]).map(|card| {
            let extra = if card == "ConditionInfo" { r#"<rect name="ConditionMeter"><image name="MeterImage"/></rect><image name="CNDArrows"/>"# } else { "" };
            format!(r#"<rect name="{card}"><_Title>Own label</_Title><text name="CI_TitleText"/><text name="CI_ValueText"/>{extra}</rect>"#)
        }).collect();
        let xml = format!(r#"<menu name="RecipeMenu"><_IsContainerListSelected>-3</_IsContainerListSelected><rect name="NOGLOW_BRANCH">
          <image name="RM_Background"/>
          <rect name="RM_ItemsRect"><x>30</x>
            <image name="RM_Items_LeftFilterArrow"/><text name="RM_ItemsTitle"><string>Own title</string></text><image name="RM_Items_RightFilterArrow"/>
            {}
          </rect>
          <rect name="RM_ContainerRect"><x>400</x>
            <text name="RM_MadeAtVariable"/><text name="RM_SkillRequirementHeader"><string>Own skill words</string></text><text name="RM_SkillRequirement"/>
            <text name="RM_Items_IngredientList"><string>Own list words</string></text>
            {}
          </rect>
          <rect name="RM_ButtonRect">
            <image name="RM_ButtonX"><text name="button_text"><string>Do</string></text><text name="PCShortcutLabel"/></image>
            <image name="RM_ButtonB"><text name="button_text"><string>Leave</string></text><text name="PCShortcutLabel"/></image>
          </rect>
          <image name="RM_ItemIcon"/><rect name="RM_ItemData"><user1>25</user1>{cards}</rect>
          </rect><template name="RM_list_template"><hotrect name="RM_list_template_container"><width>180</width><target>1</target>
            <_y>0</_y><_VerticalSpacing>6</_VerticalSpacing>
            <y><copy src="me()" trait="_y"/><sub src="parent()" trait="_offset"/></y>
            <text name="ListItemText"/><text name="RM_list_template_ItemValue"/><image name="RM_list_template_ItemMarker"/>
          </hotrect></template></menu>"#, list("RM_Items_InventoryList", 95), list("RM_Items_IngredientList", 70));
        let mut ui = Ui::new();
        ui.load_menu(&MemSource::default().with("fixture.xml", &change(xml)), "fixture.xml").unwrap();
        ui.update();
        ui
    }

    fn row(id: u32, ready: bool) -> RowShown {
        RowShown { key: FormId(id), name: format!("Thing {id}"), text: format!("Thing {id}"), ready, ysi: None }
    }

    fn snapshot() -> Snapshot {
        Snapshot {
            rows: vec![row(1, true), row(2, false), row(3, false), row(4, false), row(5, false)],
            selected: Some(0),
            filtered: 0,
            filter: 0,
            subcategory: None,
            made_at: "Own bench".into(),
            skill: Some(("Own skill (3/9)".into(), false)),
            ingredients: vec![row(7, true), row(8, false)],
            card: Some(Info { weight: Some("1.5".into()), value: Some("6".into()), effects: Some("Own effect".into()), ..Default::default() }),
            icon: Some("interface\\own.dds".into()),
            hints: ["[A]".into(), "[X]".into()],
            pointer_pick: false,
            navigated: 0,
        }
    }

    fn frame(ui: &mut Ui, s: &Snapshot, st: &mut RecipeTiles) {
        let menu = ui.menu(RECIPE_MENU).unwrap();
        st.menu = Some(menu);
        if st.is_new(menu, s) {
            write(ui, s, st);
            st.shown = Some(s.clone());
        }
        let texts = super::super::menus::texts(ui, menu);
        settle(ui, st, &mut |ui, sizes| super::super::menus::write_text_sizes_of(ui, &texts, sizes, &super::super::list::lines_size));
        mark_tiles(ui);
    }

    fn number(ui: &Ui, path: &str, name: &str) -> f64 { ui.get_float(&format!("{path}/{name}")).unwrap() }
    fn string(ui: &Ui, path: &str, name: &str) -> String { ui.get_string(&format!("{path}/{name}")).unwrap() }

    #[test]
    fn incomplete_file_falls_back_whole() {
        assert_eq!(built(&fixture(|s| s)), Tiled::Yes);
        for missing in ["RM_Background", "RM_MadeAtVariable", "RM_SkillRequirement", "RM_ItemIcon", "PCShortcutLabel", "scrollbar_vert_marker", "RM_list_template_ItemValue", "RM_list_template_ItemMarker", "CI_ValueText", "RM_Items_InventoryList"] {
            let ui = fixture(|s| s.replace(missing, "Absent"));
            assert!(!built(&ui).yes(), "{missing}");
        }
        assert!(!built(&Ui::new()).yes());
    }

    #[test]
    fn the_ingredients_list_is_the_list_not_the_text_of_its_name() {
        let ui = fixture(|s| s);
        let list = ui.find_tile(LISTS[INGREDIENTS]).unwrap();
        assert!(list::child(&ui, list, list::SCROLLBAR).is_some());
        let plain = ui.find_tile(&LISTS[INGREDIENTS].replace(":0", "")).unwrap();
        assert_ne!(plain, list, "the plain path finds the text standing before it");
        // With the text alone, no list to write rows in: the hand-built menu stays.
        let only_text = fixture(|s| s.replacen(r#"<hotrect name="RM_Items_IngredientList">"#, r#"<hotrect name="Elsewhere">"#, 1));
        assert!(!built(&only_text).yes());
    }

    #[test]
    fn rows_ingredients_and_their_dimming_are_written() {
        let (mut ui, mut st) = (fixture(|s| s), RecipeTiles::default());
        frame(&mut ui, &snapshot(), &mut st);
        let text = |ui: &Ui, tile, name: &str, value: &str| ui.tile(list::child(ui, tile, ROW_TEXT).unwrap()).get(name).map(|v| v.text()).as_deref() == Some(value);
        let (made, not) = (st.rows[RECIPES][0].tile, st.rows[RECIPES][1].tile);
        assert!(text(&ui, made, "string", "Thing 1"));
        assert_eq!(ui.tile(made).name, "Thing 1");
        assert_eq!(ui.tile(made).parent, ui.find_tile(LISTS[RECIPES]));
        assert!(text(&ui, not, "alpha", &Value::Num(DIM).text()), "a recipe that cannot be made");
        assert!(text(&ui, made, "alpha", &Value::Num(255.0).text()));
        assert!(ui.tile(not).get("brightness").is_none());
        assert_eq!(ui.tile(list::child(&ui, made, ROW_VALUE).unwrap()).get("string").unwrap().text(), "");
        assert!(!ui.tile(list::child(&ui, made, ROW_MARK).unwrap()).get("visible").unwrap().truthy());
        let (enough, short) = (st.rows[INGREDIENTS][0].tile, st.rows[INGREDIENTS][1].tile);
        assert!(fnv_ui::input::targets(&ui, made), "a recipe's row takes the pointer");
        assert!(!fnv_ui::input::targets(&ui, enough), "an ingredient's does not");
        assert!(!fnv_ui::input::targets(&ui, short));
        assert_eq!(ui.tile(enough).parent, ui.find_tile(LISTS[INGREDIENTS]));
        assert!(text(&ui, enough, "alpha", &Value::Num(255.0).text()));
        assert!(text(&ui, short, "string", "Thing 8"));
        assert!(text(&ui, short, "alpha", &Value::Num(DIM).text()), "each ingredient on its own");
        assert!(number(&ui, LISTS[RECIPES], "_selected_height") > 0.0, "the recipes' list has the highlight");
        assert_eq!(number(&ui, LISTS[INGREDIENTS], "_selected_height"), 0.0, "the ingredients' has none");
        assert_eq!(number(&ui, LISTS[INGREDIENTS], "_highlight_y"), -1.0);
    }

    #[test]
    fn the_right_box_the_card_and_the_buttons_follow_the_choice() {
        let (mut ui, mut st, mut s) = (fixture(|s| s), RecipeTiles::default(), snapshot());
        frame(&mut ui, &s, &mut st);
        assert_eq!(string(&ui, MADE_AT, "string"), "Own bench");
        assert_eq!(string(&ui, SKILL, "string"), "Own skill (3/9)");
        assert_eq!(number(&ui, SKILL, "alpha"), DIM);
        assert_eq!(number(&ui, SKILL_HEADER, "visible"), 1.0);
        assert_eq!(string(&ui, SKILL_HEADER, "string"), "Own skill words", "the file's words stay");
        assert_eq!(string(&ui, CARD, "WeightInfo/_Value"), "1.5");
        assert_eq!(string(&ui, CARD, "ValueInfo/_Value"), "6");
        assert_eq!(string(&ui, CARD, "EffectsInfo/_Value"), "Own effect");
        assert_eq!(string(&ui, CARD, "WeightInfo/_Title"), "Own label");
        assert_eq!(number(&ui, CARD, "DAMInfo/visible"), 0.0);
        assert_eq!(string(&ui, ICON, "filename"), "interface\\own.dds");
        assert_eq!(number(&ui, ICON, "visible"), 1.0);
        assert_eq!(string(&ui, ACCEPT, "_PCButtonText"), "[A]");
        assert_eq!(string(&ui, EXIT, "_PCButtonText"), "[X]");
        assert_eq!(number(&ui, ACCEPT, "visible"), 1.0);
        assert_eq!(number(&ui, EXIT, "visible"), 1.0);
        assert_eq!(number(&ui, ACCEPT, "target"), 1.0);
        assert_eq!(ui.get_float(&format!("{RECIPE_MENU}/_IsContainerListSelected")), Some(-3.0), "the file's");
        // A recipe that cannot be made, one needing no skill.
        s.selected = Some(1);
        s.skill = None;
        s.icon = None;
        frame(&mut ui, &s, &mut st);
        assert_eq!(number(&ui, ACCEPT, "target"), 0.0);
        for path in [ACCEPT.to_string(), format!("{ACCEPT}/button_text"), format!("{ACCEPT}/PCShortcutLabel")] {
            assert!(ui.get_float(&format!("{path}/brightness")).is_none(), "{path}: the file dims it");
        }
        assert_eq!(number(&ui, SKILL, "visible"), 1.0);
        assert_eq!(number(&ui, SKILL_HEADER, "visible"), 1.0);
        assert_eq!(string(&ui, SKILL, "string"), "", "nothing to say");
        assert_eq!(number(&ui, SKILL, "alpha"), 255.0);
        assert_eq!(number(&ui, ICON, "visible"), 0.0);
        // An empty list: no card, nothing to accept, no row selected.
        s.rows.clear();
        s.ingredients.clear();
        s.selected = None;
        s.card = None;
        frame(&mut ui, &s, &mut st);
        assert_eq!(number(&ui, ACCEPT, "target"), 0.0);
        assert_eq!(number(&ui, CARD, "WeightInfo/visible"), 0.0);
        assert!(st.rows[RECIPES].is_empty());
    }

    #[test]
    fn the_title_is_the_files_until_a_filter_and_after_it() {
        let (mut ui, mut st, mut s) = (fixture(|s| s), RecipeTiles::default(), snapshot());
        frame(&mut ui, &s, &mut st);
        assert_eq!(string(&ui, TITLE, "string"), "Own title", "nothing written");
        s.subcategory = Some("Own group".into());
        s.filter = 2;
        s.filtered = 7;
        frame(&mut ui, &s, &mut st);
        assert_eq!(string(&ui, TITLE, "string"), "Own group");
        assert_eq!(number(&ui, LISTS[RECIPES], "_num_filtered"), 7.0);
        s.subcategory = Some("Other group".into());
        frame(&mut ui, &s, &mut st);
        assert_eq!(string(&ui, TITLE, "string"), "Other group");
        s.subcategory = None;
        s.filter = 0;
        s.filtered = 0;
        frame(&mut ui, &s, &mut st);
        assert_eq!(string(&ui, TITLE, "string"), "Own title", "the file's again");
    }

    #[test]
    fn tiles_stand_for_their_actions_and_stale_rows_act_on_nothing() {
        let (mut ui, mut st) = (fixture(|s| s), RecipeTiles::default());
        assert_eq!(st.clicked(&ui, ui.find_tile(ACCEPT).unwrap()), None, "not drawn from tiles");
        frame(&mut ui, &snapshot(), &mut st);
        for (path, what) in [(format!("{ACCEPT}/button_text"), RecipeTile::Accept), (format!("{EXIT}/PCShortcutLabel"), RecipeTile::Exit),
            (LEFT_ARROW.into(), RecipeTile::Arrow(-1)), (RIGHT_ARROW.into(), RecipeTile::Arrow(1)), (TITLE.into(), RecipeTile::Title),
            (format!("{}/lb_scrollbar/{}", LISTS[RECIPES], list::DOWN), RecipeTile::Bar(RECIPES, BarPiece::Down)),
            (format!("{}/lb_scrollbar/{}", LISTS[INGREDIENTS], list::MARKER), RecipeTile::Bar(INGREDIENTS, BarPiece::Marker))] {
            assert_eq!(st.clicked(&ui, ui.find_tile(&path).unwrap()), Some(what), "{path}");
        }
        let text = list::child(&ui, st.rows[RECIPES][2].tile, ROW_TEXT).unwrap();
        assert_eq!(st.clicked(&ui, text), Some(RecipeTile::Row(2)));
        assert_eq!(st.clicked(&ui, st.rows[INGREDIENTS][0].tile), Some(RecipeTile::Other), "an ingredient's row is no recipe's");
        assert_eq!(row_input(Some(0), 0), RecipeInput::Accept);
        assert_eq!(row_input(Some(0), 2), RecipeInput::Select(2));
        assert_eq!(row_input(None, 0), RecipeInput::Select(0));
        // A row's tile outlives its row only as a click on nothing.
        assert_eq!(st.row_at(0), Some(0));
        let gone = st.rows[RECIPES][0].tile;
        let mut s = snapshot();
        s.rows.remove(0);
        frame(&mut ui, &s, &mut st);
        assert!(!ui.is_live(gone));
        assert_eq!(st.clicked(&ui, gone), None);
        assert_eq!(st.row_at(4), None);
        // A snapshot whose row 0 stands for another recipe than the tile there.
        st.shown.as_mut().unwrap().rows[0].key = FormId(99);
        assert_eq!(st.row_at(0), None);
        st.close();
        assert!(st.menu.is_none());
    }

    #[test]
    fn pointer_picks_do_not_scroll_keys_do_and_a_filter_starts_at_the_top() {
        let (mut ui, mut st, mut s) = (fixture(|s| s), RecipeTiles::default(), snapshot());
        frame(&mut ui, &s, &mut st);
        assert!(st.scroll_to(RECIPES, 1.0));
        s.pointer_pick = true;
        s.selected = Some(4);
        frame(&mut ui, &s, &mut st);
        assert_eq!(st.value(RECIPES), 1.0);
        let tile = st.list_tile(&ui, RECIPES).unwrap();
        assert_eq!(st.list_of(&ui, tile), Some(RECIPES));
        list::write_value(&mut ui, tile, 0.0);
        st.rewrite(&mut ui);
        ui.update();
        assert_eq!(list::value_shown(&ui, tile), Some(1.0), "the view's own, over a pulse's end");
        s.pointer_pick = false;
        s.navigated += 1;
        frame(&mut ui, &s, &mut st);
        assert!(st.value(RECIPES) > 1.0, "the key's pick brings its row in");
        s.filter = 3;
        s.selected = Some(0);
        frame(&mut ui, &s, &mut st);
        assert_eq!(st.value(RECIPES), 0.0, "another filter: the top");
        // The drawn line says so once.
        let mut lines = Vec::new();
        st.check_drawn(&ui, &mut |l| lines.push(l));
        assert!(lines.iter().any(|l| l == "tiles: RecipeMenu recipes list scrolled to 0 of 3"), "{lines:?}");
    }

    #[test]
    fn the_ingredients_scroll_starts_at_the_top_for_each_recipe() {
        let (mut ui, mut st, mut s) = (fixture(|s| s), RecipeTiles::default(), snapshot());
        s.ingredients = (10..18).map(|i| row(i, true)).collect();
        frame(&mut ui, &s, &mut st);
        assert!(st.scroll_to(INGREDIENTS, 2.0));
        frame(&mut ui, &s, &mut st);
        assert_eq!(st.value(INGREDIENTS), 2.0, "the bar's, kept");
        s.selected = Some(1);
        frame(&mut ui, &s, &mut st);
        assert_eq!(st.value(INGREDIENTS), 0.0);
    }

    #[test]
    fn physical_keys_act_once_and_scripts_keep_the_file_bindings() {
        for key in ["A", "X", "W", "S", "xbuttonx", "xbuttonb", "xbuttona", "xleft", "xbuttonrt"] {
            assert!(!super::super::menus::key_click_acts(RECIPE_MENU, key, &[key]), "{key}");
            assert!(super::super::menus::key_click_acts(RECIPE_MENU, key, &[]), "{key}");
        }
    }

    #[test]
    fn the_decision_needs_the_switch_the_menu_and_the_file() {
        let mut state = crate::script::ScriptState::over_no_plugins();
        let game = &mut state.rt.as_mut().unwrap().game;
        game.ui_files = Some(Box::new(MemSource::default()));
        game.menu = Some(fnv_game::crafting::RECIPE_MENU);
        game.ui_tree = fixture(|s| s);
        assert!(up(game));
        let mut st = RecipeTiles::default();
        assert_eq!(st.built_now(&game.ui_tree), Tiled::Yes);
        assert_eq!(super::super::item_menu::takes_input(game.ui_tree.menu(RECIPE_MENU), true), None, "under the prompt it takes nothing");
        // The answer stands while the shape does, and follows a tile going.
        assert_eq!(st.built_now(&game.ui_tree), Tiled::Yes);
        let shape = game.ui_tree.shape();
        let icon = game.ui_tree.find_tile(ICON).unwrap();
        game.ui_tree.unload_tile(icon);
        assert_ne!(game.ui_tree.shape(), shape);
        assert!(!st.built_now(&game.ui_tree).yes());
        game.menu = Some(crate::script::MENU_BARTER);
        assert!(!up(game));
    }

    #[test]
    fn ysi_row_icons_attach_to_recipes_and_ingredients_and_noop_without_ysi() {
        let tagged = fnv_game::ui_ysi::YsiItemDisplay {
            tag: Some("(d01)".into()),
            icon: Some("icons\\stimpak.dds".into()),
            template: fnv_game::ui_ysi::YSI_DEFAULT_TEMPLATE.into(),
        };
        let ing_tagged = fnv_game::ui_ysi::YsiItemDisplay {
            tag: Some("(j01)".into()),
            icon: Some("icons\\broc.dds".into()),
            template: fnv_game::ui_ysi::YSI_DEFAULT_TEMPLATE.into(),
        };
        let mut s = snapshot();
        s.rows[0].ysi = Some(tagged);
        s.ingredients[0].ysi = Some(ing_tagged);

        // Without ySI.xml: complete no-op, zero ySIImage tiles created.
        let (mut plain_ui, mut plain_st) = (fixture(|x| x), RecipeTiles::default());
        frame(&mut plain_ui, &s, &mut plain_st);
        assert!(list::child(&plain_ui, plain_st.rows[RECIPES][0].tile, fnv_game::ui_ysi::YSI_IMAGE_TILE).is_none());
        assert!(list::child(&plain_ui, plain_st.rows[INGREDIENTS][0].tile, fnv_game::ui_ysi::YSI_IMAGE_TILE).is_none());

        // With ySI.xml injected: recipe and ingredient rows both get ySIImage tiles.
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
        let (mut ui, mut st) = (fixture(|x| x), RecipeTiles::default());
        let src = MemSource::default().with(fnv_game::ui_ysi::YSI_XML_PATH, ysi_xml);
        let root = ui.menu(RECIPE_MENU).unwrap();
        assert!(fnv_game::ui_ysi::publish_for_menu(&mut ui, &src, root, Some(fnv_game::crafting::RECIPE_MENU)));
        ui.update();
        frame(&mut ui, &s, &mut st);

        let r0_img = list::child(&ui, st.rows[RECIPES][0].tile, fnv_game::ui_ysi::YSI_IMAGE_TILE).expect("recipe icon");
        let r1_img = list::child(&ui, st.rows[RECIPES][1].tile, fnv_game::ui_ysi::YSI_IMAGE_TILE).expect("untagged recipe icon");
        let i0_img = list::child(&ui, st.rows[INGREDIENTS][0].tile, fnv_game::ui_ysi::YSI_IMAGE_TILE).expect("ingredient icon");
        assert_eq!(ui.tile(r0_img).get("filename").map(Value::text).as_deref(), Some("icons\\stimpak.dds"));
        assert_eq!(ui.tile(r1_img).get("filename").map(Value::text).as_deref(), Some("icons\\blank.dds"));
        assert_eq!(ui.tile(i0_img).get("filename").map(Value::text).as_deref(), Some("icons\\broc.dds"));
        assert_eq!(st.clicked(&ui, r0_img), Some(RecipeTile::Row(0)));
    }
}

