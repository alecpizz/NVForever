//! The recipe menu: the workbench, the campfire and the reloading bench share it
//! (docs/spec-notes/crafting.md; the records and the check are `fnv_game::crafting`).
//!
//! A script opens it (`station.ShowRecipeMenu category`, the activators' own scripts);
//! it lists the recipes of that category or subcategory whose visibility conditions pass
//! for the player: the ones the player can make now first, then the rest, each part by
//! name (owner's request 2026-10-26). Seven rows show at once with a scroll bar right of
//! them when there are more; the keys and the wheel move the choice and scroll to keep it
//! in view, the pointer only chooses the row under it (the list never moves under it,
//! as in every menu). The left/right arrows over the list filter it by subcategory
//! (all, then each subcategory the list holds, by name) [hypothesis: the arrows of the
//! original's `RM_Items_LeftFilterArrow` / `RightFilterArrow` cycle the subcategories;
//! not captured]. At the right, for the chosen recipe: where it is made (its category's
//! name), the skill it needs with the player's value and the level ("Science (17/45)"),
//! and each ingredient with how many the player has and needs ("Fission Battery (0/1)")
//! [observed: the owner's screenshots, C:\fnv-private\Crafting]; under the list the
//! first output's weight and value. Accept (A, Enter) makes it: the ingredients leave the
//! player's inventory and the outputs arrive, with an "added" line in the corner; a
//! recipe the player cannot make yet stays in the list half bright and Accept refuses
//! it. Exit (X, Esc, Tab) closes the menu.
//!
//! Making several at once (2026-10-19): when the player has what it takes to make the
//! recipe, Accept opens the "How many?" prompt, even when only one can be made (owner
//! 2026-10-26: Sunny's healing powder asked nothing) of the other menus
//! (`menus\quantity_menu.xml`, drawn by `menu::look::spawn_quantity`), from 1 up to the
//! most the ingredients allow ([`times_possible`]); Left/Right (or the wheel, the arrows,
//! a press on the meter) set the amount, Enter or A makes that many, E or Esc goes back
//! [documented: the prompt's file; hypothesis: the original's recipe menu asks so]. Log
//! `recipe menu: how many? 1 of <n>`, `recipe menu: made <n> x "<name>" (...)`.
//!
//! The output's picture and its effects (2026-10-19) [documented: `RM_ItemIcon` at the
//! list box's left plus 22, 472 down, 256 square; the item stats display's EffectsInfo
//! card in `RM_ItemData`; hypothesis: the effects text as the Pip-Boy's ITEMS page writes
//! it, under the weight and value cards]: the first output's ICON, and an aid item's own effects
//! or an apparel's or a weapon's enchantment's ("HP +40, RAD -10").
//!
//! The look [documented: the original's `menus\recipe_menu.xml`, layout in its units:
//! the menu 1024 by 800 in the middle of the screen, the list's box at (20, 20) 464 wide,
//! the recipe's box 560 further right; the titles in font 6, the values in font 3, the
//! rows in font 2; Accept and Exit at y 670; observed: the screenshots for the rest].
//! With `[UI] TileMenus=RecipeMenu` the file itself draws the menu (`crate::tiles::recipe_menu`,
//! crafting.md section 4): the rules here stay, what it asks comes as [`RecipeInput`], and
//! [`draw`] spawns only a sheet that takes the pointer and the prompt.
//! Test aid `NVF_PLAY_TEST_RECIPE=<seconds>:<action>[,…]`: `down`, `up`, `filter` (the
//! right arrow), `accept`, `exit`, `more` and `less` (the prompt's amount), `cancel` (the
//! prompt), `shot` (a screenshot `recipe_menu.png` in the working folder). Open the menu through a script, e.g. the console aid's `player.craft
//! WorkbenchRecipes`.

use crate::interface::{amber, bc_supported, face, plain_font, u};
use crate::menu::readability::Typeface as Face;
use crate::menu::look::ROWS_BAR_W;
use crate::menu::model::keep_in_view;
use crate::pause::AidTime;
use crate::script::ScriptState;
use crate::sim::SimState;
use crate::start_menu::{color, highlight_colors, Shade, LINE};
use crate::ui_sounds::{UiEvent, UiMenu, UiSound};
use fnv_util::words::trf;
use crate::{PlaySet, PlayState};
use bevy::image::CompressedImageFormatSupport;
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use crate::ui_pointer::{Button, PointerState};
use fnv_game::crafting::{missing, recipe_count, skill_name, RecipeDef, RECIPE_MENU};
use fnv_sim::FormId;
use nvf_assets::GameData;

/// The menu's size and where its boxes are, in its file's units [documented:
/// recipe_menu.xml `RM_MainRect`, `RM_ItemsRect`, `RM_ContainerRect`].
const MAIN: Vec2 = Vec2::new(1024.0, 800.0);
const ITEMS_AT: Vec2 = Vec2::new(20.0, 20.0);
const BOX_WIDTH: f32 = 464.0;
const RIGHT_AT: Vec2 = Vec2::new(580.0, 20.0);
/// The list: its top, its rows' height and how many show [documented: `RM_Items_
/// InventoryList` at y 60, 365 high; observed: rows about 58 of 1080 pixels apart].
const LIST_TOP: f32 = 80.0;
const ROW: f32 = 52.0;
const ROWS: usize = 7;
/// The buttons' top [documented: `RM_ButtonRect` y 670].
const BUTTONS_TOP: f32 = 670.0;
/// The output's picture [documented: `RM_ItemIcon`, the list box's x plus 22, its y plus
/// 472, 256 square].
const ICON_AT: Vec2 = Vec2::new(ITEMS_AT.x + 22.0, ITEMS_AT.y + 472.0);
const ICON_SIZE: f32 = 256.0;
/// The sheet behind the menu [documented: `RM_Background` solid black at brightness 128
/// and the background fill alpha, 1144 by 970 from (-40, -105)]: faint, as the original's,
/// which shows only the frame's lines over the world [observed: the owner's screenshot,
/// 2026-10-19; the 0.75 of the other menus drew a dark box far past the frame]. The
/// original also blurs the world behind the menu (not built). Ours darkens the whole
/// screen as the character menus do: the owner found the world behind too bright for the
/// text (playtest list 7 item 2, `crate::interface::top_menu_backing`) [design].
/// A recipe that cannot be made yet: a little dimmer than one that can [observed: the
/// owner's screenshots, where no recipe could be made, show the rows a shade under the
/// titles; the brightness is a guess].
const NOT_READY: f32 = 190.0;

/// Fonts by the number the menu file gives them [documented: Fallout_default.ini].
const TITLE_FONT: usize = 6;
const VALUE_FONT: usize = 3;
const ROW_FONT: usize = 2;

pub struct CraftingPlugin;

#[derive(SystemSet,Debug,Clone,Copy,PartialEq,Eq,Hash)]
pub(crate) struct NormalInput;

impl Plugin for CraftingPlugin {
    fn build(&self, app: &mut App) {
        let mut aid = Vec::new();
        if let Ok(v) = std::env::var("NVF_PLAY_TEST_RECIPE") {
            for part in v.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                match part.split_once(':').and_then(|(t, a)| Some((t.trim().parse::<f32>().ok()?, a.trim().to_string()))) {
                    Some(x) => aid.push(x),
                    None => warn!("NVF_PLAY_TEST_RECIPE: {part:?}: expected <seconds>:<action>"),
                }
            }
            aid.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        app.init_resource::<RecipeMenu>()
            .add_message::<RecipeInput>()
            .insert_resource(RecipeAid(aid))
            .add_systems(Update, (scripts_open, (keys.in_set(NormalInput), draw).chain().run_if(in_state(PlayState::Menu))).chain().in_set(PlaySet::Ui))
            .add_systems(OnExit(PlayState::Menu), close)
            .add_systems(Update, forget_closed.in_set(PlaySet::Ui));
    }
}

/// `NVF_PLAY_TEST_RECIPE`: (seconds, action), soonest first.
#[derive(Resource, Default)]
struct RecipeAid(Vec<(f32, String)>);

/// The recipe menu while it is open.
#[derive(Resource, Default)]
pub struct RecipeMenu(pub Option<Open>);

impl RecipeMenu {
    pub(crate) fn normal_same_session(&self,category:FormId,station:Option<FormId>,filter:usize,
        selected:usize,quantity:Option<(i32,i32)>)->bool {
        self.0.as_ref().is_some_and(|o|o.category==category && o.station==station && o.filter==filter
            && o.selected==selected && o.quantity==quantity)
    }
    pub(crate) fn normal_guard(&self)->Option<crate::menu::normal_services::Guard> {
        self.0.as_ref()?.normal_guard()
    }
    pub(crate) fn normal_matches(&self,guard:&crate::menu::normal_services::Guard)->bool {
        let Some(o)=self.0.as_ref() else{return false};
        let crate::menu::normal_services::Guard::Workbench {category,station,filter,view}=guard else{return false};
        *category==o.category && *station==o.station && *filter==o.filter
            && o.drawn.as_ref().is_some_and(|v|v.normal_matches(view))
    }
    /// [design, #2403 / file refs] Logical recipe navigation and ingredient outcomes;
    /// `Open::navigated` and pointer/redraw bookkeeping are deliberately excluded.
    pub fn progress(&self) -> Option<impl std::fmt::Debug + '_> {
        self.0.as_ref().map(|o| (o.category, o.station, o.filter, o.selected, o.first, o.quantity, o.drawn.as_ref().map(|v| (&v.rows, &v.ingredients, &v.skill))))
    }
    pub fn is_open(&self) -> bool {
        self.0.is_some()
    }
}

/// What the menu drawn from its tiles asks (`crate::tiles::input`), read in [`keys`] beside
/// the keys and the hand-built buttons.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipeInput {
    /// Internal once-only request from the ordinary finger adapter.
    CommitNormal(u64),
    /// The pointer came onto the row at this place.
    Hover(usize),
    /// A click on the row at this place, not the selected one.
    Select(usize),
    /// A step of the subcategory filter.
    Filter(i32),
    Accept,
    Exit,
}

pub struct Open {
    category: FormId,
    station: Option<FormId>,
    /// 0: all; else the subcategory at this place of the view's filters, less one.
    pub(crate) filter: usize,
    pub(crate) selected: usize,
    /// The list's first row shown: the keys and the wheel move it to keep the choice in
    /// view, the pointer never does.
    first: usize,
    pub(crate) drawn: Option<View>,
    /// The "How many?" prompt: the amount and the most that can be made.
    pub(crate) quantity: Option<(i32, i32)>,
    /// The pointer last moved the selection: a list drawn from tiles stays where it is.
    pub(crate) pointer_pick: bool,
    /// A count of the keys' and the wheel's moves, for that list to show its row after one.
    pub(crate) navigated: u32,
}

impl Open {
    pub(crate) fn normal_guard(&self)->Option<crate::menu::normal_services::Guard> {
        Some(crate::menu::normal_services::Guard::Workbench {category:self.category,station:self.station,
            filter:self.filter,view:Box::new(self.drawn.as_ref()?.clone())})
    }
    pub(crate) fn normal_native_guard(&self)->Option<crate::menu::normal_services::Guard> {
        let mut guard=self.normal_guard()?;
        if let crate::menu::normal_services::Guard::Workbench {view,..}=&mut guard {view.quantity=None;}
        Some(guard)
    }
    pub(crate) fn normal_owner_matches(&self,guard:Option<&crate::menu::normal_services::Guard>)->bool {
        matches!(guard,Some(crate::menu::normal_services::Guard::Workbench {category,station,filter,..})
            if *category==self.category && *station==self.station && *filter==self.filter)
    }
}

/// How many times the player can make `r` with what it has: the fewest of each
/// ingredient's count over what one making takes (no positive ingredients: once).
pub fn times_possible(r: &RecipeDef, have: impl Fn(FormId) -> i64) -> u32 {
    r.ingredients
        .iter()
        .filter_map(|&(i, n)| {
            let need = recipe_count(n);
            (need > 0).then(|| (have(i).max(0) as u64 / u64::from(need)).min(u64::from(u32::MAX)) as u32)
        })
        .min()
        .unwrap_or(1)
}

#[derive(Component)]
struct RecipeRoot;

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum RecipeButton {
    Row(usize),
    Accept,
    Exit,
    Left,
    Right,
    Title,
}

/// A row of the list.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RowView {
    pub(crate) recipe: FormId,
    pub(crate) label: String,
    /// Can be made now.
    pub(crate) ready: bool,
    /// Resolved ySI tag/icon/template for this recipe's representative item (`None` when ySI is inactive).
    pub(crate) ysi: Option<fnv_game::ui_ysi::YsiItemDisplay>,
}

/// Everything the menu shows: drawn again when it changes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct View {
    title: String,
    pub(crate) rows: Vec<RowView>,
    /// The subcategories the arrows go through, by name.
    pub(crate) filters: usize,
    /// The subcategory's record name while it filters the list.
    pub(crate) subcategory: Option<String>,
    /// The recipes the filter leaves out.
    pub(crate) filtered: usize,
    pub(crate) selected: usize,
    /// The first row shown.
    first: usize,
    pub(crate) made_at: String,
    /// Skill name, the player's value, the level needed.
    pub(crate) skill: Option<(String, u32, u32)>,
    /// The skill's name in its record (`fnv_game::chargen::skill_name`), for the tiles.
    pub(crate) skill_record: Option<String>,
    /// The ingredient, its name, have, need.
    pub(crate) ingredients: Vec<(FormId, String, u32, u32)>,
    /// Resolved ySI display per ingredient row (parallel to `ingredients`, empty when ySI is inactive).
    pub(crate) ingredient_ysi: Vec<Option<fnv_game::ui_ysi::YsiItemDisplay>>,
    pub(crate) weight: Option<f32>,
    pub(crate) value: Option<i32>,
    /// The first output's picture and effects.
    pub(crate) icon: Option<String>,
    pub(crate) effects: Option<String>,
    /// The "How many?" prompt: amount and most.
    quantity: Option<(i32, i32)>,
    /// The names the hints use (`crate::pad::labels`).
    labels: crate::pad::Labels,
}

impl View {
    pub(crate) fn normal_quantity(&self)->Option<(i32,i32)> {self.quantity}
    pub(crate) fn normal_matches(&self,other:&Self)->bool {
        self.rows==other.rows && self.filters==other.filters
            && self.subcategory==other.subcategory && self.filtered==other.filtered && self.selected==other.selected
            && self.skill.as_ref().map(|(_,have,need)|(*have,*need))==other.skill.as_ref().map(|(_,have,need)|(*have,*need))
            && self.ingredients==other.ingredients && self.quantity==other.quantity
    }
}

/// The recipe's label: its name, and the number made after it when more than one and the
/// name does not say it already [observed: "Antivenom (2)", "Conversion, Fission Battery
/// to SEC (25)" on the owner's screenshots].
fn label(r: &RecipeDef) -> String {
    let n = r.made();
    if n > 1 && !r.name.trim_end().ends_with(')') {
        format!("{} ({n})", r.name)
    } else {
        r.name.clone()
    }
}

/// The recipes a category lists for the player now: its own or its subcategory's, their
/// conditions passing, by name.
fn listed(rt: &mut crate::script::Runtime, category: FormId) -> Vec<&'static RecipeDef> {
    let data: &'static fnv_game::data::GameData = rt.game.data;
    let mut out: Vec<&RecipeDef> = data.recipes.values().filter(|r| r.listed_under(category)).collect();
    // The record's conditions and AddRecipeCondition's (`fnv_game::nvse_jip12`).
    out.retain(|r| rt.game.conditions_pass(&r.conditions, Some(FormId::PLAYER_REF), None, "recipe") && rt.game.recipe_conditions_pass(r.form));
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.form.0.cmp(&b.form.0)));
    out
}

/// God mode with the Free Crafting tweak: recipes need nothing.
fn free_crafting(rt: &crate::script::Runtime) -> bool {
    crate::combat::god_mode() && rt.game.tweaks.god_mode_free_crafting
}

/// What the player lacks to make `r` once (with the world in the runtime).
fn lacks(rt: &mut crate::script::Runtime, r: &RecipeDef) -> Vec<fnv_game::crafting::Missing> {
    // The player's tweak: in god mode a recipe needs nothing [design: docs/design/tweaks.md].
    if free_crafting(rt) {
        return Vec::new();
    }
    let skill = r.skill.map_or(0.0, |(av, _)| fnv_game::functions::actor_value_capped(&mut rt.game, FormId::PLAYER_REF, av));
    let counts: Vec<(FormId, i64)> = r.ingredients.iter().map(|&(i, _)| (i, rt.game.world.item_count(FormId::PLAYER_REF, i).unwrap_or(0) as i64)).collect();
    missing(r, 1, |_| skill, |i| counts.iter().find(|c| c.0 == i).map_or(0, |c| c.1))
}

/// An output's weight as the weight card says it.
pub(crate) fn weight_text(w: f32) -> String {
    format!("{w:.1}").trim_end_matches(".0").to_string()
}

/// Rebuild an open recipe menu after a script called `RefreshItemsList`: clears the
/// cached `drawn` view so hand-built and tiled renderers redraw immediately, preserves
/// selection on the same recipe FormID when still listed (clamping otherwise), keeps the
/// scroll window valid, and clamps or clears any open quantity prompt.
pub(crate) fn refresh_open(rt: &mut crate::script::Runtime, sim: &mut SimState, open: &mut Open) {
    let keep = open.drawn.as_ref().and_then(|d| d.rows.get(d.selected)).map(|r| r.recipe);
    open.drawn = None;
    let v = view(rt, sim, open);
    open.selected = keep
        .and_then(|id| v.rows.iter().position(|r| r.recipe == id))
        .unwrap_or_else(|| open.selected.min(v.rows.len().saturating_sub(1)));
    open.first = keep_in_view(open.first, open.selected, v.rows.len(), ROWS);
    if let Some((amount, _)) = open.quantity {
        let now_recipe = v.rows.get(open.selected).map(|r| r.recipe);
        let max_make = now_recipe
            .filter(|&id| keep.is_none() || Some(id) == keep)
            .and_then(|id| rt.game.data.recipes.get(&id))
            .map(|r| {
                if free_crafting(rt) {
                    u32::MAX
                } else {
                    crate::script::with_world(rt, sim, |rt| {
                        if lacks(rt, r).is_empty() {
                            let counts: Vec<(FormId, i64)> = r.ingredients.iter().map(|&(i, _)| (i, rt.game.world.item_count(FormId::PLAYER_REF, i).unwrap_or(0) as i64)).collect();
                            times_possible(r, |i| counts.iter().find(|c| c.0 == i).map_or(0, |c| c.1))
                        } else {
                            0
                        }
                    })
                }
            })
            .unwrap_or(0);
        if max_make > 1 {
            let most = max_make.min(i32::MAX as u32) as i32;
            open.quantity = Some((amount.clamp(1, most), most));
        } else {
            open.quantity = None;
        }
    }
}

/// What the menu shows for `open`.
pub(crate) fn view(rt: &mut crate::script::Runtime, sim: &mut SimState, open: &Open) -> View {
    let forms = sim.forms;
    crate::script::with_world(rt, sim, |rt| {
        let data = rt.game.data;
        let all = listed(rt, open.category);
        // The subcategories in the list, by name.
        // The player's tweak shows them in capitals [design: docs/design/tweaks.md].
        let caps = rt.game.tweaks.capitalize_recipe_categories;
        let mut subs: Vec<(String, FormId)> = all.iter().filter_map(|r| r.subcategory).filter(|s| *s != open.category).map(|s| (data.recipe_categories.get(&s).map_or_else(String::new, |c| if caps { c.name.to_uppercase() } else { c.name.clone() }), s)).collect();
        subs.sort();
        subs.dedup();
        let filter = subs.get(open.filter.wrapping_sub(1)).map(|s| s.1);
        let known = all.len();
        let have = |rt: &mut crate::script::Runtime, item: FormId| rt.game.world.item_count(FormId::PLAYER_REF, item).unwrap_or(0) as i64;
        // The recipes that can be made now come first, each part by name (owner's
        // request 2026-10-26).
        let mut listed: Vec<(&RecipeDef, bool)> = all.into_iter().filter(|r| filter.is_none_or(|f| r.subcategory == Some(f))).map(|r| (r, lacks(rt, r).is_empty())).collect();
        listed.sort_by_key(|&(_, ready)| !ready);
        let shown: Vec<&RecipeDef> = listed.iter().map(|l| l.0).collect();
        let ysi_on = rt.game.ysi_active();
        let rows: Vec<RowView> = listed
            .iter()
            .map(|&(r, ready)| {
                let ysi = if ysi_on {
                    // [design] Breakdown recipes (1 ingredient, >1 outputs) show the
                    // ingredient's icon; other recipes show the first output's icon,
                    // falling back to the recipe form itself.
                    let rep = if r.ingredients.len() == 1 && r.outputs.len() > 1 {
                        r.ingredients.first().map(|x| x.0)
                    } else {
                        r.outputs.first().map(|x| x.0)
                    }
                    .unwrap_or(r.form);
                    rt.game.ysi_item_display(rep)
                } else {
                    None
                };
                RowView { recipe: r.form, label: label(r), ready, ysi }
            })
            .collect();
        let selected = open.selected.min(rows.len().saturating_sub(1));
        let subcategory = filter.map(|_| subs[open.filter - 1].0.clone());
        let title = match filter {
            None => crate::pipboy::labels::built_in_label("sRecipes").to_uppercase(),
            Some(_) => subs[open.filter - 1].0.to_uppercase(),
        };
        let chosen = shown.get(selected).copied();
        let name = |id: FormId| crate::menu::model::display_name(forms, id);
        let made_at = chosen.and_then(|r| r.category).and_then(|c| data.recipe_categories.get(&c)).map_or_else(String::new, |c| c.name.clone());
        let skill = chosen.and_then(|r| r.skill).map(|(av, level)| (skill_name(av).to_string(), fnv_game::functions::actor_value_capped(&mut rt.game, FormId::PLAYER_REF, av).max(0.0).floor() as u32, level));
        let skill_record = chosen.and_then(|r| r.skill).map(|(av, _)| fnv_game::chargen::skill_name(&rt.game, av));
        let ingredients: Vec<(FormId, String, u32, u32)> = chosen.map(|r| r.ingredients.iter().map(|&(i, n)| (i, name(i), have(rt, i).max(0) as u32, recipe_count(n))).collect()).unwrap_or_default();
        let ingredient_ysi = if ysi_on {
            ingredients.iter().map(|(id, _, _, _)| rt.game.ysi_item_display(*id)).collect()
        } else {
            Vec::new()
        };
        let item = chosen.and_then(|r| r.outputs.first()).and_then(|o| match forms.get(o.0).map(|b| &b.data) {
            Some(fnv_sim::forms::BaseData::Item(it)) => Some((it.weight, it.value, it.icon.clone())),
            _ => None,
        });
        let (weight, value, icon) = item.map_or((None, None, None), |i| (i.0, i.1, i.2));
        let first = if open.pointer_pick {open.first.min(rows.len().saturating_sub(1))}
            else {keep_in_view(open.first, selected, rows.len(), ROWS)};
        let filtered = known - rows.len();
        View { title, rows, filters: subs.len(), subcategory, filtered, selected, first, made_at, skill, skill_record, ingredients, ingredient_ysi, weight, value, icon, effects: None, quantity: open.quantity, labels: crate::pad::labels() }
    })
}

/// The recipe menus scripts asked for, opened while playing or over a conversation (a
/// line's result script, as the repair services, #2308; `crate::repair::after_close`).
#[allow(clippy::too_many_arguments)]
fn scripts_open(
    script: Option<ResMut<ScriptState>>,
    mut sim: ResMut<SimState>,
    state: Res<State<PlayState>>,
    mut next: ResMut<NextState<PlayState>>,
    mut menu: ResMut<RecipeMenu>,
    active: Res<crate::menu::ActiveMenu>,
    mut sounds: MessageWriter<UiSound>,
) {
    let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) else { return };
    let here = matches!(*state.get(), PlayState::Playing | PlayState::Dialogue);
    if rt.game.recipe_requests.is_empty() || !here || menu.is_open() || active.0.is_some() {
        return;
    }
    let Some(r) = rt.game.recipe_requests.pop() else { return };
    rt.game.recipe_requests.clear();
    let open = Open { category: r.category, station: r.station, filter: 0, selected: 0, first: 0, drawn: None, quantity: None, pointer_pick: false, navigated: 0 };
    let v = view(rt, &mut sim, &open);
    let name = rt.game.data.recipe_categories.get(&r.category).map_or_else(|| r.category.to_string(), |c| c.name.clone());
    let first: Vec<&str> = v.rows.iter().take(6).map(|r| r.label.as_str()).collect();
    info!(
        "recipe menu: open {name} ({} recipes listed of {} known, {} ready; station {}): {}",
        v.rows.len(),
        rt.game.data.recipes.len(),
        v.rows.iter().filter(|r| r.ready).count(),
        open.station.map_or("none".to_string(), |s| s.to_string()),
        first.join(", ")
    );
    menu.0 = Some(open);
    next.set(PlayState::Menu);
    sounds.write(UiSound { menu: UiMenu::Recipe, event: UiEvent::Opened });
}

/// The "How many?" prompt's buttons and its meter (`crate::menu`).
type AmountButton = (&'static PointerState, &'static crate::menu::CmdButton);
type AmountPress = (&'static PointerState, &'static bevy::ui::RelativeCursorPosition);
/// The records the effects line is read from.
type EffectForms = crate::pipboy::stats::read::Forms;

fn filter_step(open:&mut Open,filters:usize,by:i32) {
    open.filter=if by<0 {(open.filter+filters-1)%filters}else{(open.filter+1)%filters};
    open.selected=0;
}

#[allow(clippy::too_many_arguments)]
fn keys(
    keys: crate::pad::ActionButtons<KeyCode>,
    time: AidTime,
    mut aid: ResMut<RecipeAid>,
    mut menu: ResMut<RecipeMenu>,
    script: Option<ResMut<ScriptState>>,
    mut sim: ResMut<SimState>,
    buttons: Query<(&PointerState, &RecipeButton), Changed<PointerState>>,
    (amount_buttons, meter): (Query<AmountButton, Changed<PointerState>>, Query<AmountPress, With<crate::menu::AmountMeter>>),
    scroll: Option<Res<AccumulatedMouseScroll>>,
    mut next: ResMut<NextState<PlayState>>,
    mut sounds: MessageWriter<UiSound>,
    mut corner: Option<ResMut<crate::corner_messages::CornerMessages>>,
    mut moved: MessageReader<bevy::window::CursorMoved>,
    mut asked: MessageReader<RecipeInput>,
    mut commands: Commands,
    (mut normal,preferences):(crate::menu::normal_services::Commits,crate::maintenance_touch::Requests),
) {
    let keys = keys.snapshot();
    let asked: Vec<RecipeInput> = asked.read().copied().collect();
    let normal_tokens:Vec<_>=asked.iter().filter_map(|a|if let RecipeInput::CommitNormal(t)=a {Some(*t)}else{None}).collect();
    // Only a pointer that moves takes the selection: the rows drawn when the menu opens
    // (or redrawn) under a resting pointer leave the keys' choice alone (as `crate::menu`).
    let pointer_moved = moved.read().count() > 0;
    let Some(open) = menu.0.as_mut() else { return };
    let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) else { return };
    // A world aid (`crate::pause::WorldAidClock`).
    let now = time.world_secs();
    let mut acts = Vec::new();
    while aid.0.first().is_some_and(|(t, _)| now.is_some_and(|now| *t <= now)) {
        let (_, a) = aid.0.remove(0);
        info!("test aid NVF_PLAY_TEST_RECIPE: {a}");
        acts.push(a);
    }
    let act = |n: &str| acts.iter().any(|a| a == n);
    if act("shot") {
        info!("recipe menu: screenshot recipe_menu.png");
        commands.spawn(bevy::render::view::screenshot::Screenshot::primary_window()).observe(bevy::render::view::screenshot::save_to_disk("recipe_menu.png"));
    }
    let v = open.drawn.clone().unwrap_or_else(|| view(rt, &mut sim, open));
    // The "How many?" prompt has the keys while it is open (module notes).
    if let Some((amount, most)) = open.quantity {
        use crate::menu::model::Command;
        let mut to = amount;
        let (mut make, mut back) = (false, false);
        for (i, b) in &amount_buttons {
            if *i == PointerState::Pressed {
                match b.0 {
                    Command::AmountStep(d) => to += d,
                    Command::AmountOk => make = true,
                    Command::AmountCancel => back = true,
                    _ => {}
                }
            }
        }
        for (i, at) in &meter {
            if let (PointerState::Pressed, Some(p)) = (i, at.normalized) {
                to = crate::menu::model::amount_at(p.x + 0.5, most);
            }
        }
        let wasd = rt.game.tweaks.wasd_as_arrow_keys;
        let wheel = scroll.as_ref().map_or(0.0, |s| s.delta.y);
        if keys.any_just_pressed([KeyCode::ArrowRight, KeyCode::KeyD]) || wheel > 0.0 || act("more") {
            to += 1;
        }
        let less_keys: &[KeyCode] = if wasd { &[KeyCode::ArrowLeft, KeyCode::KeyA] } else { &[KeyCode::ArrowLeft] };
        if keys.any_just_pressed(less_keys.iter().copied()) || wheel < 0.0 || act("less") {
            to -= 1;
        }
        let other_changed=to!=amount || make || back;
        let mut normal_confirmed=false;
        for &token in &normal_tokens {
            // Existing physical/script prompt intent wins over an older proposal.
            if other_changed {normal.discard(token);continue}
            let live=view(rt,&mut sim,open);
            let current=crate::menu::normal_services::Guard::Workbench {category:open.category,
                station:open.station,filter:open.filter,view:Box::new(live)};
            if let Some(crate::menu::normal_services::Action::Amount(action))=
                normal.resolve(token,&current,&rt.game,preferences.enabled()) {
                match action {
                    Command::Amount(value)=>to=value,
                    Command::AmountStep(by)=>to=to.saturating_add(by),
                    Command::AmountOk=>{make=true;normal_confirmed=true;},
                    Command::AmountCancel=>back=true,
                    _=>{},
                }
            }
        }
        let to = to.clamp(1, most.max(1));
        if to != amount {
            open.quantity = Some((to, most));
            sounds.write(UiSound { menu: UiMenu::Recipe, event: UiEvent::Focus });
        }
        if keys.any_just_pressed([KeyCode::KeyE, KeyCode::Escape]) || back || act("cancel") || act("exit") {
            info!("recipe menu: how many? cancelled");
            open.quantity = None;
            sounds.write(UiSound { menu: UiMenu::Recipe, event: UiEvent::Back });
            return;
        }
        let qty_accept: &[KeyCode] = if wasd {
            &[KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]
        } else {
            &[KeyCode::KeyA, KeyCode::Enter, KeyCode::NumpadEnter]
        };
        if keys.any_just_pressed(qty_accept.iter().copied()) || make || act("accept") {
            open.quantity = None;
            let chosen = v.rows.get(open.selected.min(v.rows.len().saturating_sub(1))).map(|r| r.recipe);
            if let Some(recipe) = chosen.and_then(|f| rt.game.data.recipes.get(&f)) {
                make_recipe(rt, &mut sim, recipe, to as u32, &mut sounds, corner.as_deref_mut());
                follow_made(rt, &mut sim, open, recipe.form);
                if normal_confirmed {open.pointer_pick=false;}
            }
            open.drawn = None;
        }
        return;
    }
    let n = v.rows.len();
    let before = (open.selected, open.filter);
    let mut clicked = None;
    for (i, b) in &buttons {
        match (i, b) {
            (PointerState::Pressed, b) => clicked = Some(*b),
            (PointerState::Hovered, RecipeButton::Row(r)) if pointer_moved => open.selected = *r,
            _ => {}
        }
    }
    // The menu drawn from its tiles asks what the hand-built buttons would.
    for input in asked {
        match input {
            RecipeInput::CommitNormal(_)=>{},
            RecipeInput::Hover(r) => {
                open.selected = r;
                open.pointer_pick = true;
            }
            RecipeInput::Select(r) => {
                clicked = Some(RecipeButton::Row(r));
                open.pointer_pick = true;
            }
            RecipeInput::Filter(by) => clicked = Some(if by < 0 { RecipeButton::Left } else { RecipeButton::Right }),
            RecipeInput::Accept => clicked = Some(RecipeButton::Accept),
            RecipeInput::Exit => clicked = Some(RecipeButton::Exit),
        }
    }
    let wheel = scroll.as_ref().map_or(0.0, |s| s.delta.y);
    if keys.any_just_pressed([KeyCode::ArrowUp, KeyCode::KeyW]) || wheel > 0.0 || act("up") {
        open.selected = open.selected.min(n.saturating_sub(1)).saturating_sub(1);
        open.pointer_pick = false;
        open.navigated = open.navigated.wrapping_add(1);
    }
    if keys.any_just_pressed([KeyCode::ArrowDown, KeyCode::KeyS]) || wheel < 0.0 || act("down") {
        open.selected = (open.selected + 1).min(n.saturating_sub(1));
        open.pointer_pick = false;
        open.navigated = open.navigated.wrapping_add(1);
    }
    let wasd = rt.game.tweaks.wasd_as_arrow_keys;
    let filters = v.filters + 1;
    let right_keys: &[KeyCode] = if wasd { &[KeyCode::ArrowRight, KeyCode::KeyD] } else { &[KeyCode::ArrowRight] };
    let left_keys: &[KeyCode] = if wasd { &[KeyCode::ArrowLeft, KeyCode::KeyA] } else { &[KeyCode::ArrowLeft] };
    if keys.any_just_pressed(right_keys.iter().copied()) || clicked == Some(RecipeButton::Right) || clicked == Some(RecipeButton::Title) || act("filter") {
        filter_step(open,filters,1);
    }
    if keys.any_just_pressed(left_keys.iter().copied()) || clicked == Some(RecipeButton::Left) {
        filter_step(open,filters,-1);
    }
    if let Some(RecipeButton::Row(r)) = clicked {
        open.selected = r;
    }
    // All pre-existing selection/filter controls have run. Resolve the typed
    // request against current requirements and ingredient counts here, directly
    // before the original quantity/make/close consumers below.
    for token in normal_tokens {
        let live=view(rt,&mut sim,open);
        let current=crate::menu::normal_services::Guard::Workbench {category:open.category,
            station:open.station,filter:open.filter,view:Box::new(live)};
        let action=normal.resolve(token,&current,&rt.game,preferences.enabled());
        let crate::menu::normal_services::Guard::Workbench {view:live,..}=current else{continue};
        use crate::menu::normal_services::Action;
        match action {
            Some(Action::Workbench(RecipeInput::Select(i)|RecipeInput::Hover(i))) if i<live.rows.len()=>{
                open.selected=i;open.pointer_pick=true;open.drawn=None;
            }
            Some(Action::Workbench(RecipeInput::Filter(by)))=>filter_step(open,live.filters+1,by),
            Some(Action::Workbench(RecipeInput::Accept))=>clicked=Some(RecipeButton::Accept),
            Some(Action::Workbench(RecipeInput::Exit))=>clicked=Some(RecipeButton::Exit),
            Some(Action::Browse(0,by))=>{
                open.first=(open.first as i64).saturating_add(by).clamp(0,live.rows.len().saturating_sub(1) as i64) as usize;
                open.pointer_pick=true;open.drawn=None;
            }
            _=>{},
        }
    }
    if open.filter != before.1 {
        open.pointer_pick = false;
    }
    if !open.pointer_pick {open.first = keep_in_view(open.first, open.selected, n, ROWS);}
    if (open.selected, open.filter) != before {
        sounds.write(UiSound { menu: UiMenu::Recipe, event: if open.filter != before.1 { UiEvent::Switch } else { UiEvent::Focus } });
    }
    if keys.any_just_pressed([KeyCode::KeyX, KeyCode::Escape, KeyCode::Tab]) || clicked == Some(RecipeButton::Exit) || act("exit") {
        info!("recipe menu: exit");
        sounds.write(UiSound { menu: UiMenu::Recipe, event: UiEvent::Back });
        next.set(crate::repair::after_close(&rt.game));
        return;
    }
    let accept_keys: &[KeyCode] = if wasd {
        &[KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]
    } else {
        &[KeyCode::KeyA, KeyCode::Enter, KeyCode::NumpadEnter]
    };
    if keys.any_just_pressed(accept_keys.iter().copied()) || clicked == Some(RecipeButton::Accept) || act("accept") {
        let chosen = v.rows.get(open.selected.min(n.saturating_sub(1))).map(|r| r.recipe);
        let Some(recipe) = chosen.and_then(|f| rt.game.data.recipes.get(&f)) else { return };
        // Any can be made: how many? (module notes)
        let most = crate::script::with_world(rt, &mut sim, |rt| {
            if !lacks(rt, recipe).is_empty() {
                return 0;
            }
            let counts: Vec<(FormId, i64)> = recipe.ingredients.iter().map(|&(i, _)| (i, rt.game.world.item_count(FormId::PLAYER_REF, i).unwrap_or(0) as i64)).collect();
            times_possible(recipe, |i| counts.iter().find(|c| c.0 == i).map_or(0, |c| c.1))
        });
        if most >= 1 {
            info!("recipe menu: how many? 1 of {most} ({:?})", recipe.name);
            open.quantity = Some((1, most.min(i32::MAX as u32) as i32));
            sounds.write(UiSound { menu: UiMenu::Recipe, event: UiEvent::Opened });
            return;
        }
        make_recipe(rt, &mut sim, recipe, 1, &mut sounds, corner.as_deref_mut());
        follow_made(rt, &mut sim, open, recipe.form);
        // What the player has changed: draw it again.
        open.drawn = None;
    }
}

/// After making `recipe` the ready recipes may have moved (they are listed first): the
/// choice stays on it, kept in view.
fn follow_made(rt: &mut crate::script::Runtime, sim: &mut SimState, open: &mut Open, recipe: FormId) {
    let v = view(rt, sim, open);
    if let Some(i) = v.rows.iter().position(|r| r.recipe == recipe) {
        open.selected = i;
        open.pointer_pick = false;
        open.first = keep_in_view(open.first, i, v.rows.len(), ROWS);
    }
}

/// Make `recipe` `times` times: the ingredients go, the outputs come, an "added" line
/// each in the corner (module notes).
fn make_recipe(rt: &mut crate::script::Runtime, sim: &mut SimState, recipe: &RecipeDef, times: u32, sounds: &mut MessageWriter<UiSound>, corner: Option<&mut crate::corner_messages::CornerMessages>) {
    let forms = sim.forms;
    let made = crate::script::with_world(rt, sim, |rt| {
        let skill = recipe.skill.map_or(0.0, |(av, _)| fnv_game::functions::actor_value_capped(&mut rt.game, FormId::PLAYER_REF, av));
        let counts: Vec<(FormId, i64)> = recipe.ingredients.iter().map(|&(i, _)| (i, rt.game.world.item_count(FormId::PLAYER_REF, i).unwrap_or(0) as i64)).collect();
        let lack = missing(recipe, times, |_| skill, |i| counts.iter().find(|c| c.0 == i).map_or(0, |c| c.1));
        let free = free_crafting(rt);
        if !lack.is_empty() && !free {
            return Err(format!("{lack:?}"));
        }
        for &(i, k) in &recipe.ingredients {
            // Free crafting takes what there is of each part.
            let want = i64::from(recipe_count(k)) * i64::from(times);
            let n = if free { want.min(counts.iter().find(|c| c.0 == i).map_or(0, |c| c.1)) } else { want };
            let n = n.min(i64::from(i32::MAX)) as i32;
            if n > 0 {
                rt.game.world.remove_item(FormId::PLAYER_REF, i, n).map_err(|e| format!("{e:?}"))?;
            }
        }
        let mut added = Vec::new();
        // The player's tweaks: Repair-scaled condition (`bRepairScalesCraftingCondition`)
        // or fixed crafted condition percentage (`fCraftedItemHealthPct`) [design: docs/design/tweaks.md].
        let repair = if rt.game.tweaks.repair_scales_crafting {
            fnv_game::functions::actor_value_capped(&mut rt.game, FormId::PLAYER_REF, fnv_game::actor_values::REPAIR) as f32
        } else {
            0.0
        };
        let condition = fnv_game::tweaks::crafted_output_condition(&rt.game.tweaks, repair);
        for &(i, k) in &recipe.outputs {
            let count = recipe_count(k).saturating_mul(times).min(i32::MAX as u32);
            if count == 0 {
                continue;
            }
            let worn = matches!(rt.game.data.sig(i), Some(s) if &s == b"WEAP" || &s == b"ARMO");
            match condition.filter(|c| worn && *c < 1.0) {
                Some(c) => {
                    rt.game.world.add_item_at_condition(FormId::PLAYER_REF, i, count as i32, Some(c)).map_err(|e| format!("{e:?}"))?;
                    info!("recipe menu: made at {:.0} % condition (crafting condition tweak)", c * 100.0);
                }
                None => {
                    rt.game.world.add_item(FormId::PLAYER_REF, i, count as i32).map_err(|e| format!("{e:?}"))?;
                }
            }
            added.push((i, count));
        }
        Ok(added)
    });
    match made {
        Ok(added) => {
            rt.game.count_stat(fnv_game::misc_stats::ITEMS_CRAFTED, times as f64);
            let ingredients: Vec<FormId> = recipe.ingredients.iter().map(|&(i, _)| i).collect();
            rt.game.challenge_crafted(&ingredients, times);
            // JIP's crafting event, once per output (`fnv_game::jip_events`).
            crate::script::with_world(rt, sim, |rt| {
                for &(i, k) in &added {
                    rt.game.item_crafted(i, i64::from(k));
                }
            });
            let what = added.iter().map(|(i, k)| format!("{k} x {}", crate::menu::model::display_name(forms, *i))).collect::<Vec<_>>().join(", ");
            if times > 1 {
                info!("recipe menu: made {times} x {:?} ({what})", recipe.name);
            } else {
                info!("recipe menu: made {:?} ({what})", recipe.name);
            }
            sounds.write(UiSound { menu: UiMenu::Recipe, event: UiEvent::Picked });
            // ToggleCraftingMessages 0 keeps them off (`fnv_game::nvse_jip6`).
            if let Some(c) = corner.filter(|_| rt.game.crafting_messages_shown()) {
                for (i, k) in added {
                    let name = crate::menu::model::display_name(forms, i);
                    c.push(if k > 1 { trf("{name} ({count}) added", &[("name", &name), ("count", &k)]) } else { trf("{name} added", &[("name", &name)]) }.into());
                }
            }
        }
        Err(why) => {
            info!("recipe menu: cannot make {:?}: {why}", recipe.name);
            sounds.write(UiSound { menu: UiMenu::Recipe, event: UiEvent::Refused });
        }
    }
}

/// The menu's fonts and the arrow pictures, read once.
#[derive(Resource)]
struct RecipeLook {
    title: Face,
    value: Face,
    row: Face,
    arrows: Option<(Handle<Image>, Handle<Image>)>,
}

fn text(p: &mut ChildSpawnerCommands, f: &Face, s: &str, x: f32, top: f32, c: Color) {
    if let Some(line) = f.font.lines(s, None).into_iter().next() {
        crate::menu::readability::line(p,f,&line,Vec2::new(x,top+f.font.size*0.8*f.scale),f.scale,c);
    }
}

fn wrapped(p: &mut ChildSpawnerCommands, f: &Face, s: &str, x: f32, top: f32, width: f32, c: Color) -> f32 {
    let lines = f.font.lines(s, Some(width/f.scale));
    for (i, line) in lines.iter().enumerate() {
        crate::menu::readability::line(p,f,line,Vec2::new(x,top+f.font.size*(0.8+i as f32*0.9)*f.scale),f.scale,c);
    }
    lines.len().max(1) as f32 * f.font.size * 0.9*f.scale
}

fn at(x: f32, y: f32, w: f32, h: f32) -> Node {
    Node { position_type: PositionType::Absolute, left: u(x), top: u(y), width: u(w), height: u(h), ..default() }
}

/// A line of the brackets.
fn rule(p: &mut ChildSpawnerCommands, x: f32, y: f32, w: f32, h: f32) {
    p.spawn((at(x, y, w, h), BackgroundColor(amber(200.0))));
}

/// The two boxes' brackets: a line along the top with short sides going down, and one
/// along the bottom with short sides going up [documented: recipe_menu.xml headline and
/// footer lines, 40 high sides].
fn brackets(p: &mut ChildSpawnerCommands, x: f32, y: f32, w: f32, bottom: f32, title_gap: (f32, f32)) {
    rule(p, x, y, title_gap.0, LINE);
    rule(p, x + title_gap.1, y, w - title_gap.1, LINE);
    rule(p, x, y, LINE, 40.0);
    rule(p, x + w - LINE, y, LINE, 40.0);
    rule(p, x, bottom - 40.0, LINE, 40.0);
    rule(p, x, bottom, w, LINE);
    rule(p, x + w - LINE, bottom - 40.0, LINE, 40.0);
}

fn recipe_row_height(face:&Face,label:&str)->f32 {
    let wrap=(BOX_WIDTH-15.0-ROWS_BAR_W-52.0)/face.scale;
    let lines=face.font.lines(label,Some(wrap)).len().max(1);
    ((ROW.max(face.font.size*0.9*lines as f32+12.0))*face.scale).max(ROW)
}
fn recipe_window(look:&RecipeLook,v:&View,follow:bool)->std::ops::Range<usize> {
    let heights:Vec<_>=v.rows.iter().map(|r|recipe_row_height(&look.row,&r.label)).collect();
    let mut first=if follow {v.first.min(v.selected)}else{v.first}.min(heights.len());
    loop {
        let mut used=0.0;let mut end=first;
        while end<heights.len() && used+heights[end]<=ROWS as f32*ROW {used+=heights[end];end+=1;}
        if !follow || v.selected<end || end>=heights.len() || first>=v.selected {
            return first..end.max(first+1).min(heights.len());
        }
        first+=1;
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn(commands: &mut Commands, look: &RecipeLook, presentation:(&View,crate::menu::normal_services::Guard), words: &[String; 2], kit: &mut crate::menu::look::Kit, data: Option<&GameData>, images: &mut Assets<Image>, bc: bool, glyphs: &crate::pad::PadGlyphs) {
    let (v,guard)=presentation;
    let left = Node { position_type: PositionType::Absolute, left: Val::Percent(50.0), margin: UiRect::left(u(-MAIN.x / 2.0)), top: crate::interface::from_top((960.0 - MAIN.y) / 2.0, 0.5), width: u(MAIN.x), height: u(MAIN.y), ..default() };
    commands.spawn((RecipeRoot, crate::menu::normal_services::Rendered::new(guard,look.row.scale), Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, GlobalZIndex(crate::interface::TOP_MENU_Z), crate::interface::top_menu_backing())).with_children(|root| {
        root.spawn(left).with_children(|m| {
            let bright = color(Shade::Bright);
            // The list's box: "◀ RECIPES ▶" in the top line.
            let tw = look.title.width(&v.title);
            brackets(m, ITEMS_AT.x, ITEMS_AT.y, BOX_WIDTH, ITEMS_AT.y + 480.0, (25.0, 50.0 + tw + 40.0));
            // Spawn the title hit zone first so left/right arrow buttons sit above it in UiStack.
            m.spawn((Button, RecipeButton::Title, at(ITEMS_AT.x + 50.0, ITEMS_AT.y - 28.0, (tw + 4.0).max(80.0), 56.0))).with_children(|t| text(t, &look.title, &v.title, 0.0, 0.0, bright));
            if let Some((l, r)) = &look.arrows {
                let hit_w = 52.0;
                let hit_h = 52.0;
                let dx = (hit_w - 17.5) * 0.5;
                let dy = (hit_h - 35.0) * 0.5;
                m.spawn((Button, RecipeButton::Left, at(ITEMS_AT.x + 30.0 - dx, ITEMS_AT.y - 17.0 - dy, hit_w, hit_h))).with_children(|btn| {
                    btn.spawn((at(dx, dy, 17.5, 35.0), ImageNode { color: amber(255.0), ..ImageNode::new(l.clone()) }));
                });
                m.spawn((Button, RecipeButton::Right, at(ITEMS_AT.x + 55.0 + tw - dx, ITEMS_AT.y - 17.0 - dy, hit_w, hit_h))).with_children(|btn| {
                    btn.spawn((at(dx, dy, 17.5, 35.0), ImageNode { color: amber(255.0), ..ImageNode::new(r.clone()) }));
                });
            }
            // The rows from the first shown.
            let shown=recipe_window(look,v,false);
            m.spawn(Node {overflow:Overflow::clip(),..at(ITEMS_AT.x+5.0,LIST_TOP,BOX_WIDTH-15.0-ROWS_BAR_W,ROWS as f32*ROW)}).with_children(|list| {
            let mut y=0.0;
            for i in shown.clone() {
                let r=&v.rows[i];
                let height=recipe_row_height(&look.row,&r.label);
                let (line, fill) = highlight_colors();
                let chosen = i == v.selected;
                let node = Node { border: UiRect::all(u(LINE)), ..at(0.0, y, BOX_WIDTH - 15.0 - ROWS_BAR_W, height) };
                let tint = if r.ready { bright } else { amber(NOT_READY) };
                let row_icon = r
                    .ysi
                    .as_ref()
                    .and_then(|d| d.icon.as_deref())
                    .and_then(|path| {
                        let full = format!("textures\\{}", path.trim_start_matches("textures\\").trim_start_matches("Textures\\"));
                        data.and_then(|d| crate::interface::texture(&full, d, images, bc)).map(|x| x.0)
                    });
                list.spawn((Button, RecipeButton::Row(i), node, BorderColor::all(if chosen { line } else { Color::NONE }), BackgroundColor(if chosen { fill } else { Color::NONE }))).with_children(|row| {
                    if let Some(img) = row_icon {
                        row.spawn((at(10.0, 14.0*look.row.scale, 24.0, 24.0), ImageNode { color: tint, ..ImageNode::new(img) }));
                    }
                    wrapped(row, &look.row, &r.label, 40.0, 6.0*look.row.scale, BOX_WIDTH-15.0-ROWS_BAR_W-52.0, tint);
                });
                y+=height;
            }
            });
            // The scroll bar, right of the rows, when they do not all fit (owner's request
            // 2026-10-26; drawn as the container menu's).
            let bar = (ITEMS_AT.x + BOX_WIDTH - 5.0 - ROWS_BAR_W, LIST_TOP, ROWS as f32 * ROW);
            crate::menu::look::spawn_rows_bar(m, kit, data, images, bc, glyphs, bar, v.rows.len(), v.first, shown.len());
            if v.rows.is_empty() {
                m.spawn(at(ITEMS_AT.x + 5.0, LIST_TOP, BOX_WIDTH, ROW)).with_children(|t| text(t, &look.row, "No recipes", 40.0, 6.0, color(Shade::Dim)));
            }
            // The output's picture.
            if let Some(icon) = v.icon.as_deref() {
                let path = format!("textures\\{}", icon.trim_start_matches("textures\\").trim_start_matches("Textures\\"));
                if let Some((img, _)) = data.and_then(|d| crate::interface::texture(&path, d, images, bc)) {
                    m.spawn((at(ICON_AT.x, ICON_AT.y, ICON_SIZE, ICON_SIZE), ImageNode { color: amber(255.0), ..ImageNode::new(img) }));
                }
            }
            // Its effects, under the weight and value cards.
            if let Some(e) = v.effects.as_deref() {
                m.spawn(at(340.0, 690.0, 290.0, 100.0)).with_children(|t| {
                    text(t, &look.row, "EFFECTS", 6.0, 0.0, bright);
                    wrapped(t, &look.row, e, 6.0, 36.0, 280.0, bright);
                });
            }
            // The output's weight and value.
            let wg = v.weight.map_or("--".to_string(), weight_text);
            let val = v.value.map_or("--".to_string(), |x| x.to_string());
            // The cards' words: the item stats display's settings, as the Pip-Boy's cards
            // read them [design, #4495: in place of the abbreviations read from the
            // original's screen].
            let words = ["sInventoryWeightUpper", "sInventoryValue"].map(crate::pipboy::labels::built_in_label);
            for (k, (name, s)) in [(words[0], wg), (words[1], val)].into_iter().enumerate() {
                let x = 340.0 + k as f32 * 150.0;
                rule(m, x, 640.0, 140.0, LINE);
                rule(m, x + 140.0 - LINE, 640.0, LINE, 30.0);
                m.spawn(at(x, 645.0, 140.0, 50.0)).with_children(|t| {
                    text(t, &look.row, name, 6.0, 0.0, bright);
                    let w = look.row.width(&s);
                    text(t, &look.row, &s, 130.0 - w, 0.0, bright);
                });
            }
            // The recipe's box.
            let rx = RIGHT_AT.x;
            // The titles are the settings recipe_menu.xml names (`sMadeAt`,
            // `sSkillRequirement`, `sIngredients`; the list's `sRecipes`): the load order's
            // text, else NVForever's own label [design, #4495: in place of the words read
            // from the original's screen].
            let [made_at, skill_needed, ingredients] =
                ["sMadeAt", "sSkillRequirement", "sIngredients"].map(crate::pipboy::labels::built_in_label);
            let mw = look.title.width(made_at);
            brackets(m, rx, RIGHT_AT.y, BOX_WIDTH, RIGHT_AT.y + 515.0, (25.0, 50.0 + mw + 20.0));
            m.spawn(at(rx, RIGHT_AT.y - 28.0, BOX_WIDTH, 600.0)).with_children(|b| {
                text(b, &look.title, made_at, 50.0, 0.0, bright);
                text(b, &look.value, &v.made_at, 60.0, 78.0, bright);
                if let Some((name, have, need)) = &v.skill {
                    text(b, &look.title, skill_needed, 50.0, 128.0, bright);
                    let ok = have >= need;
                    text(b, &look.value, &format!("{name} ({have}/{need})"), 60.0, 188.0, if ok { bright } else { amber(NOT_READY) });
                }
                text(b, &look.title, ingredients, 50.0, 238.0, bright);
                let mut y = 298.0;
                for (idx, (_, name, have, need)) in v.ingredients.iter().enumerate() {
                    let ok = have >= need;
                    let tint = if ok { bright } else { amber(NOT_READY) };
                    if let Some(img) = v
                        .ingredient_ysi
                        .get(idx)
                        .and_then(|d| d.as_ref())
                        .and_then(|d| d.icon.as_deref())
                        .and_then(|path| {
                            let full = format!("textures\\{}", path.trim_start_matches("textures\\").trim_start_matches("Textures\\"));
                            data.and_then(|d| crate::interface::texture(&full, d, images, bc)).map(|x| x.0)
                        })
                    {
                        b.spawn((at(78.0, y + 2.0, 24.0, 24.0), ImageNode { color: tint, ..ImageNode::new(img) }));
                    }
                    y += wrapped(b, &look.row, &format!("{name} ({have}/{need})"), 110.0, y, 340.0, tint) + 8.0;
                }
            });
            // Accept and Exit [observed: "Accept  A)", "Exit  X)"].
            let ready = v.rows.get(v.selected).is_some_and(|r| r.ready);
            let hint_at = crate::pad::at_menu(fnv_game::crafting::RECIPE_MENU);
            let accept = format!("Accept  {}", crate::pad::hint_as(&[KeyCode::KeyA], hint_at, v.labels));
            let exit = format!("Exit  {}", crate::pad::hint_as(&[KeyCode::KeyX, KeyCode::Escape], hint_at, v.labels));
            for (k, (label, b, on)) in [(accept.as_str(), RecipeButton::Accept, ready), (exit.as_str(), RecipeButton::Exit, true)].into_iter().enumerate() {
                let w = crate::pad::hint_width(&look.row, label, glyphs)*look.row.scale;
                let step=48.0*look.row.scale;
                let y=BUTTONS_TOP+k as f32*step-2.0*(step-48.0);
                m.spawn((Button, b, at(rx + BOX_WIDTH - w - 10.0, y, w + 10.0, 44.0*look.row.scale))).with_children(|t| {
                    let _units=crate::interface::units_at(crate::interface::unit_scale()*look.row.scale);
                    crate::pad::set_hint(t, &look.row, label, 0.0, look.row.font.size * 0.8, if on { bright } else { amber(NOT_READY) }, glyphs);
                });
            }
        });
        // The "How many?" prompt over the menu (module notes).
        if let Some(quantity) = v.quantity {
            spawn_prompt(root, quantity, words, kit, data, images, bc, glyphs);
        }
    });
}

/// The recipe menu closed: the scripts' getters give nothing again.
fn forget_closed(menu: Res<RecipeMenu>, script: Option<ResMut<ScriptState>>, mut was_open: Local<bool>) {
    let open = menu.0.is_some();
    if *was_open && !open {
        if let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) {
            rt.game.set_recipe_menu(None);
        }
    }
    *was_open = open;
}

/// The "How many?" prompt over the menu (module notes): the amount and the most.
#[allow(clippy::too_many_arguments)]
fn spawn_prompt(root: &mut ChildSpawnerCommands, (amount, most): (i32, i32), words: &[String; 2], kit: &mut crate::menu::look::Kit, data: Option<&GameData>, images: &mut Assets<Image>, bc: bool, glyphs: &crate::pad::PadGlyphs) {
    let screen = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() };
    let q = crate::menu::model::Quantity { cmd: crate::menu::model::Command::One, side: crate::menu::model::Side::Player, row: 0, max: most, amount };
    root.spawn(screen).with_children(|s| crate::menu::look::spawn_quantity(s, &q, words, kit, data, images, bc, glyphs, None));
}

/// [`view`] with the selected output's effects (`forms`: where they are read from).
pub(crate) fn view_with_effects(rt: &mut crate::script::Runtime, sim: &mut SimState, open: &Open, forms: Option<&mut EffectForms>) -> View {
    let mut v = view(rt, sim, open);
    let output = v.rows.get(v.selected).and_then(|r| rt.game.data.recipes.get(&r.recipe)).and_then(|r| r.outputs.first()).map(|o| o.0);
    v.effects = output.zip(forms).and_then(|(o, f)| effects_of(sim, f, o));
    v
}

/// The prompt the sheet was last spawned with and the names its hints used
/// (`crate::pad::labels`), once it has been spawned.
type WasTiled = Option<(Option<(i32, i32)>, crate::pad::Labels)>;

type DrawState<'s>=(Local<'s,Option<EffectForms>>,Local<'s,Option<crate::menu::look::Kit>>,Local<'s,u64>);

/// What the tiled sheet's prompt shows now: the amount and the hints' names, so a switch
/// of controller (Auto button style: an Xbox pad, then a PlayStation one) spawns it again
/// as the hand-built menu's `View::labels` does [design].
fn tiled_key(quantity: Option<(i32, i32)>) -> (Option<(i32, i32)>, crate::pad::Labels) {
    (quantity, crate::pad::labels())
}

/// Draw the menu again when what it shows changed.
#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    mut menu: ResMut<RecipeMenu>,
    script: Option<ResMut<ScriptState>>,
    mut sim: ResMut<SimState>,
    roots: Query<Entity, With<RecipeRoot>>,
    look: Option<ResMut<RecipeLook>>,
    data: Option<Res<GameData>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    glyphs: Res<crate::pad::PadGlyphs>,
    play: Option<Res<crate::PlayWorld>>,
    (mut forms, mut kit,mut painted_revision): DrawState,
    // The tile writer's decision, and the prompt the sheet was last spawned with.
    (tiles, mut was_tiled): (Option<Res<crate::tiles::recipe_menu::RecipeTiles>>, Local<WasTiled>),
    (readability,mut previous_size):(Option<Res<crate::menu_readability::MenuReadability>>,Local<f32>),
) {
    let Some(open) = menu.0.as_mut() else { return };
    let Some(mut look) = look else {
        let bc = bc_supported(formats.as_deref());
        let font = |n: usize, images: &mut Assets<Image>| face(crate::pipboy::frame::FONTS[n - 1], plain_font(30.0, 14.0), data.as_deref(), images, bc);
        let pic = |p: &str, images: &mut Assets<Image>| data.as_deref().and_then(|d| crate::interface::texture(p, d, images, bc)).map(|x| x.0);
        let arrows = pic("textures\\interface\\main\\main_arrow_left.dds", &mut images).zip(pic("textures\\interface\\main\\main_arrow_right.dds", &mut images));
        commands.insert_resource(RecipeLook { title: font(TITLE_FONT, &mut images).into(), value: font(VALUE_FONT, &mut images).into(), row: font(ROW_FONT, &mut images).into(), arrows });
        return;
    };
    let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) else { return };
    let text_size=crate::menu::readability::size(readability.as_deref(),crate::menu_readability::MenuKind::Workbench);
    let resized=*previous_size!=text_size;*previous_size=text_size;
    if look.row.scale!=text_size {look.title.scale=text_size;look.value.scale=text_size;look.row.scale=text_size;}
    // GetRecipeMenuCategory and GetRecipeMenuSelection (`fnv_game::nvse_jip12`), before
    // the tiled sheet may return.
    let mut shown = view(rt, &mut sim, open);
    rt.game.set_recipe_menu(Some((Some(open.category), shown.rows.get(shown.selected).map(|r| r.recipe))));
    // Drawn from its tiles: only a sheet taking the pointer, and the prompt, which stays
    // hand-built (`crate::tiles::menus::quantity_tiled`). Its `drawn` is the tile writer's.
    if let Some(tiles)=tiles.as_ref().filter(|t|t.menu.is_some()) {
        // The native list's writer owns its painted revision. The prompt itself
        // is drawn here after input, so publish this same current view as its
        // source even when its amount just changed earlier in this frame.
        if let Some(old)=open.drawn.as_mut().filter(|old|
            old.rows.get(old.selected).map(|r|r.recipe)==shown.rows.get(shown.selected).map(|r|r.recipe)) {
            shown.effects=old.effects.take();
        }
        let revision=tiles.normal_revision();
        let key = tiled_key(open.quantity);
        if *was_tiled == Some(key) && !roots.is_empty() && !resized && *painted_revision==revision {
            open.drawn=Some(shown);
            return;
        }
        *was_tiled = Some(key);*painted_revision=revision;
        for e in &roots {
            commands.entity(e).despawn();
        }
        let bc = bc_supported(formats.as_deref());
        let kit = kit.get_or_insert_with(|| crate::menu::look::Kit::load(data.as_deref(), &mut images, bc));
        kit.set_text_size(text_size);
        let words = crate::menu::look::quantity_words(Some(&rt.game));
        let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
        commands.spawn((RecipeRoot, crate::menu::normal_services::Rendered::new(crate::menu::normal_services::Guard::Workbench {category:open.category,station:open.station,filter:open.filter,view:Box::new(shown.clone())},text_size), full, GlobalZIndex(crate::interface::TOP_MENU_Z))).with_children(|root| {
            root.spawn(crate::tiles::menus::modal_sheet());
            if let Some(quantity) = open.quantity {
                spawn_prompt(root, quantity, &words, kit, data.as_deref(), &mut images, bc, &glyphs);
            }
        });
        open.drawn=Some(shown);
        return;
    }
    // Back from the tiles: the sheet alone stands, so the hand-built menu is drawn again.
    if was_tiled.take().is_some() {
        open.drawn = None;
    }
    if forms.is_none() {
        *forms = play.as_ref().map(|p| crate::pipboy::stats::read::Forms::new(p.lo, p.schema));
    }
    let mut v = view_with_effects(rt, &mut sim, open, forms.as_mut());
    if open.drawn.as_ref() == Some(&v) && !roots.is_empty() && !resized {
        return;
    }
    // Measure row windows only for an actual repaint, not every idle menu frame.
    v.first=recipe_window(&look,&v,!open.pointer_pick).start;open.first=v.first;
    for e in &roots {
        commands.entity(e).despawn();
    }
    let bc = bc_supported(formats.as_deref());
    let kit = kit.get_or_insert_with(|| crate::menu::look::Kit::load(data.as_deref(), &mut images, bc));
        kit.set_text_size(text_size);
    let words = crate::menu::look::quantity_words(Some(&rt.game));
    spawn(&mut commands, &look, (&v,crate::menu::normal_services::Guard::Workbench {category:open.category,station:open.station,filter:open.filter,view:Box::new(v.clone())}), &words, kit, data.as_deref(), &mut images, bc, &glyphs);
    if v.effects.is_some() && open.drawn.as_ref().is_none_or(|d| d.selected != v.selected) {
        info!("recipe menu: {} effects: {}", v.rows.get(v.selected).map_or("", |r| r.label.as_str()), v.effects.as_deref().unwrap_or(""));
    }
    open.selected = v.selected;
    open.drawn = Some(v);
}

/// An item's effects as the Pip-Boy's ITEMS page writes them: an aid item's own, an
/// apparel's or a weapon's enchantment's.
fn effects_of(sim: &SimState, forms: &mut crate::pipboy::stats::read::Forms, item: FormId) -> Option<String> {
    use crate::pipboy::stats::model::{changes_text, Change};
    let aid = sim.forms.item(item).is_some_and(|it| it.kind == fnv_sim::forms::ItemKind::Aid);
    let ench = if aid { Some(item.0) } else { forms.item_ench(item.0) }?;
    let e = forms.ench(ench);
    if e.changes.is_empty() {
        return None;
    }
    let changes: Vec<Change> = e.changes.iter().map(|&(code, amount, detrimental)| Change { value: forms.short_name(code), amount, detrimental }).collect();
    Some(changes_text(&changes))
}

fn close(mut commands: Commands, mut menu: ResMut<RecipeMenu>, roots: Query<Entity, With<RecipeRoot>>, script: Option<ResMut<ScriptState>>) {
    if menu.0.take().is_none() {
        return;
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
    if let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) {
        if rt.game.menu == Some(RECIPE_MENU) {
            rt.game.close_menu();
        }
    }
    info!("recipe menu: closed");
}

/// [design, #2403] Synthetic logical/ineffective states for detector regressions.
#[cfg(feature = "report-test-support")]
#[doc(hidden)]
pub fn report_progress_fixture(step: usize) -> RecipeMenu {
    let mut o = Open { category: FormId(42), station: None, filter: 0, selected: 0, first: 0, drawn: None, quantity: None, pointer_pick: false, navigated: step as u32 };
    if step >= 1 {
        o.selected = 1;
    }
    if step >= 2 {
        o.first = 1;
    }
    if step >= 3 {
        o.filter = 1;
    }
    if step >= 4 {
        o.quantity = Some((1, 3));
    }
    if step >= 5 {
        o.quantity = Some((2, 3));
    }
    o.drawn = Some(View {
        title: String::new(),
        rows: (0..20).map(|i| RowView { recipe: FormId(100 + i), label: format!("Synthetic recipe {i}"), ready: true, ysi: None }).collect(),
        filters: 1,
        subcategory: None,
        filtered: 0,
        selected: o.selected,
        first: o.first,
        made_at: String::new(),
        skill: None,
        skill_record: None,
        ingredients: vec![(FormId(43), "Synthetic ingredient".into(), if step >= 6 { 2 } else { 3 }, 1)],
        ingredient_ysi: Vec::new(),
        weight: None,
        value: None,
        icon: None,
        effects: None,
        quantity: o.quantity,
        labels: Default::default(),
    });
    o.pointer_pick = step >= 7;
    RecipeMenu(Some(o))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_most_that_can_be_made_is_set_by_the_scarcest_ingredient() {
        let r = RecipeDef { ingredients: vec![(FormId(1), 2), (FormId(2), 1)], ..Default::default() };
        let have = |i: FormId| if i == FormId(1) { 7 } else { 5 };
        assert_eq!(times_possible(&r, have), 3);
        assert_eq!(times_possible(&r, |_| 0), 0);
        assert_eq!(times_possible(&RecipeDef::default(), |_| 0), 1);
    }

    #[test]
    fn a_weight_drops_a_whole_numbers_point() {
        assert_eq!(weight_text(2.0), "2");
        assert_eq!(weight_text(0.5), "0.5");
    }

    #[test]
    fn a_label_says_how_many_are_made_once() {
        let r = |name: &str, n: i32| RecipeDef { name: name.into(), outputs: vec![(FormId(1), n)], ..Default::default() };
        assert_eq!(label(&r("Antivenom", 2)), "Antivenom (2)");
        assert_eq!(label(&r("Conversion, Fission Battery to SEC (25)", 25)), "Conversion, Fission Battery to SEC (25)");
        assert_eq!(label(&r("Battle Brew", 1)), "Battle Brew");
        assert_eq!(label(&r("Negated Output", -5)), "Negated Output");
    }

    #[test]
    fn negative_script_recipe_counts_clamp_to_zero_across_menu_paths() {
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
        fn top(sig: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut out = Vec::with_capacity(24 + body.len());
            out.extend_from_slice(b"GRUP");
            out.extend_from_slice(&((24 + body.len()) as u32).to_le_bytes());
            out.extend_from_slice(sig);
            out.extend_from_slice(&[0u8; 12]);
            out.extend_from_slice(body);
            out
        }

        // 1. times_possible clamps -5 to 0 (ignored rather than treated as 4294967291).
        let r_only_neg = RecipeDef { ingredients: vec![(FormId(1), -5)], outputs: vec![(FormId(3), -5)], ..Default::default() };
        assert_eq!(times_possible(&r_only_neg, |_| 0), 1);
        let r_mixed = RecipeDef { ingredients: vec![(FormId(1), -5), (FormId(2), 2)], outputs: vec![(FormId(3), -5)], ..Default::default() };
        assert_eq!(times_possible(&r_mixed, |i| if i == FormId(2) { 7 } else { 0 }), 3);

        // 2. View row text (`need`) and make_recipe both clamp -5 to 0 without overflow or
        // adding/removing negative item counts.
        const CAT: u32 = 0x800;
        const ING_NEG: u32 = 0x801;
        const ING_POS: u32 = 0x802;
        const OUT_NEG: u32 = 0x803;
        const OUT_POS: u32 = 0x804;
        const RCPE: u32 = 0x810;
        let rcpe_data = [(-1i32).to_le_bytes(), 0u32.to_le_bytes(), CAT.to_le_bytes(), 0u32.to_le_bytes()].concat();
        let misc = |id: u32, edid: &[u8], full: &[u8]| rec(b"MISC", id, &[sub(b"EDID", edid), sub(b"FULL", full), sub(b"DATA", &[1, 0, 0, 0, 0, 0, 0, 0])]);
        let neg5 = (-5i32).to_le_bytes();
        let bytes = [
            rec(b"TES4", 0, &[sub(b"HEDR", &[0, 0, 0x80, 0x3F, 0, 0, 0, 0, 0, 0, 0, 0])]),
            top(b"NPC_", &rec(b"NPC_", 0x7, &[sub(b"EDID", b"Player\0")])),
            top(
                b"MISC",
                &[
                    misc(ING_NEG, b"NegIng\0", b"Neg Scrap\0"),
                    misc(ING_POS, b"PosIng\0", b"Pos Scrap\0"),
                    misc(OUT_NEG, b"NegOut\0", b"Neg Widget\0"),
                    misc(OUT_POS, b"PosOut\0", b"Pos Widget\0"),
                ]
                .concat(),
            ),
            top(b"RCCT", &rec(b"RCCT", CAT, &[sub(b"EDID", b"TestBench\0"), sub(b"FULL", b"Bench\0")])),
            top(
                b"RCPE",
                &rec(
                    b"RCPE",
                    RCPE,
                    &[
                        sub(b"EDID", b"NegCountRecipe\0"),
                        sub(b"FULL", b"Signed Brew\0"),
                        sub(b"DATA", &rcpe_data),
                        sub(b"RCIL", &ING_NEG.to_le_bytes()),
                        sub(b"RCQY", &neg5),
                        sub(b"RCIL", &ING_POS.to_le_bytes()),
                        sub(b"RCQY", &2u32.to_le_bytes()),
                        sub(b"RCOD", &OUT_NEG.to_le_bytes()),
                        sub(b"RCQY", &neg5),
                        sub(b"RCOD", &OUT_POS.to_le_bytes()),
                        sub(b"RCQY", &3u32.to_le_bytes()),
                    ],
                ),
            ),
        ]
        .concat();
        let plugin = fnv_esm::Plugin::parse(&bytes, "SignedCraft.esm".into()).unwrap();
        let mut script = ScriptState::over_plugins(&[plugin]);
        let rt = script.rt.as_mut().unwrap();
        let mut sim = SimState { forms: rt.game.world.forms, refs: rt.game.world.placed, world: fnv_sim::World::new(rt.game.world.forms, rt.game.world.placed, fnv_sim::SimConfig::default()) };
        sim.world.add_item(FormId::PLAYER_REF, FormId(ING_NEG), 4).unwrap();
        sim.world.add_item(FormId::PLAYER_REF, FormId(ING_POS), 5).unwrap();

        let open = Open { category: FormId(CAT), station: None, filter: 0, selected: 0, first: 0, drawn: None, quantity: None, pointer_pick: false, navigated: 0 };
        let v = view(rt, &mut sim, &open);
        assert_eq!(v.rows.len(), 1);
        assert!(v.rows[0].ready, "recipe with -5 ingredient and sufficient positive ingredient is ready");
        assert_eq!(v.rows[0].label, "Signed Brew", "negative first output count clamps to 0 in label");
        assert_eq!(v.ingredients.len(), 2);
        assert_eq!((v.ingredients[0].0, v.ingredients[0].2, v.ingredients[0].3), (FormId(ING_NEG), 4, 0));
        assert_eq!((v.ingredients[1].0, v.ingredients[1].2, v.ingredients[1].3), (FormId(ING_POS), 5, 2));

        let recipe = rt.game.data.recipes[&FormId(RCPE)].clone();
        let counts: Vec<(FormId, i64)> = recipe
            .ingredients
            .iter()
            .map(|&(i, _)| (i, sim.world.item_count(FormId::PLAYER_REF, i).unwrap_or(0) as i64))
            .collect();
        assert_eq!(times_possible(&recipe, |i| counts.iter().find(|c| c.0 == i).map_or(0, |c| c.1)), 2);

        // Exercise make_recipe directly through a Bevy world with UiSound message writer.
        let mut app = App::new();
        app.add_message::<UiSound>();
        app.insert_resource(script);
        app.insert_resource(sim);
        let recipe_for_sys = recipe.clone();
        app.add_systems(Update, move |mut script: ResMut<ScriptState>, mut sim: ResMut<SimState>, mut sounds: MessageWriter<UiSound>| {
            let rt = script.rt.as_mut().unwrap();
            make_recipe(rt, &mut sim, &recipe_for_sys, 2, &mut sounds, None);
        });
        app.update();

        let mut sim = app.world_mut().resource_mut::<SimState>();
        assert_eq!(sim.world.item_count(FormId::PLAYER_REF, FormId(ING_NEG)).unwrap(), 4, "-5 ingredient removes 0 items");
        assert_eq!(sim.world.item_count(FormId::PLAYER_REF, FormId(ING_POS)).unwrap(), 1, "+2 ingredient x2 removes 4 of 5");
        assert_eq!(sim.world.item_count(FormId::PLAYER_REF, FormId(OUT_NEG)).unwrap(), 0, "-5 output adds 0 items");
        assert_eq!(sim.world.item_count(FormId::PLAYER_REF, FormId(OUT_POS)).unwrap(), 6, "+3 output x2 adds 6 items");
    }

    #[test]
    fn the_tiled_prompt_is_spawned_again_when_the_hints_names_change() {
        use crate::pad::{set_labels, Labels};
        set_labels(Labels::Xbox);
        let was: WasTiled = Some(tiled_key(Some((1, 4))));
        assert_eq!(was, Some(tiled_key(Some((1, 4)))), "same amount, same pad: kept");
        set_labels(Labels::PlayStation);
        assert_ne!(was, Some(tiled_key(Some((1, 4)))), "a PlayStation pad after an Xbox one: drawn again");
        assert_ne!(tiled_key(Some((1, 4))), tiled_key(Some((2, 4))), "a new amount: drawn again");
        set_labels(Labels::Keys);
    }

    #[test]
    fn refresh_open_rebuilds_recipe_rows_and_preserves_selected_recipe() {
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
        fn top(sig: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut out = Vec::with_capacity(24 + body.len());
            out.extend_from_slice(b"GRUP");
            out.extend_from_slice(&((24 + body.len()) as u32).to_le_bytes());
            out.extend_from_slice(sig);
            out.extend_from_slice(&[0u8; 12]);
            out.extend_from_slice(body);
            out
        }

        const CAT: u32 = 0x800;
        const ING: u32 = 0x801;
        const OUT: u32 = 0x802;
        const RCPE_A: u32 = 0x810;
        const RCPE_B: u32 = 0x811;
        // RCPE DATA: skill (-1 none), level (0), category (CAT), subcategory (0).
        let rcpe_data = [(-1i32).to_le_bytes(), 0u32.to_le_bytes(), CAT.to_le_bytes(), 0u32.to_le_bytes()].concat();
        let misc = |id: u32, edid: &[u8], full: &[u8]| rec(b"MISC", id, &[sub(b"EDID", edid), sub(b"FULL", full), sub(b"DATA", &[1, 0, 0, 0, 0, 0, 0, 0])]);
        let bytes = [
            rec(b"TES4", 0, &[sub(b"HEDR", &[0, 0, 0x80, 0x3F, 0, 0, 0, 0, 0, 0, 0, 0])]),
            top(b"NPC_", &rec(b"NPC_", 0x7, &[sub(b"EDID", b"Player\0")])),
            top(b"MISC", &[misc(ING, b"OwnedIng\0", b"Scrap\0"), misc(OUT, b"OwnedOut\0", b"Widget\0")].concat()),
            top(b"RCCT", &rec(b"RCCT", CAT, &[sub(b"EDID", b"OwnedBench\0"), sub(b"FULL", b"Bench\0")])),
            top(
                b"RCPE",
                &[
                    // Alpha needs 1 ING (initially NOT ready -> sorts below Beta).
                    rec(
                        b"RCPE",
                        RCPE_A,
                        &[
                            sub(b"EDID", b"OwnedAlpha\0"),
                            sub(b"FULL", b"Alpha Brew\0"),
                            sub(b"DATA", &rcpe_data),
                            sub(b"RCIL", &ING.to_le_bytes()),
                            sub(b"RCQY", &1u32.to_le_bytes()),
                            sub(b"RCOD", &OUT.to_le_bytes()),
                            sub(b"RCQY", &1u32.to_le_bytes()),
                        ],
                    ),
                    // Beta needs no ingredients (initially ready -> sorts first at row 0).
                    rec(
                        b"RCPE",
                        RCPE_B,
                        &[
                            sub(b"EDID", b"OwnedBeta\0"),
                            sub(b"FULL", b"Beta Brew\0"),
                            sub(b"DATA", &rcpe_data),
                            sub(b"RCOD", &OUT.to_le_bytes()),
                            sub(b"RCQY", &1u32.to_le_bytes()),
                        ],
                    ),
                ]
                .concat(),
            ),
        ]
        .concat();
        let plugin = fnv_esm::Plugin::parse(&bytes, "OwnedCraft.esm".into()).unwrap();
        let mut script = ScriptState::over_plugins(&[plugin]);
        let rt = script.rt.as_mut().unwrap();
        let mut sim = SimState { forms: rt.game.world.forms, refs: rt.game.world.placed, world: fnv_sim::World::new(rt.game.world.forms, rt.game.world.placed, fnv_sim::SimConfig::default()) };

        let mut open = Open { category: FormId(CAT), station: None, filter: 0, selected: 0, first: 0, drawn: None, quantity: None, pointer_pick: false, navigated: 0 };
        let initial = view(rt, &mut sim, &open);
        assert_eq!(initial.rows.iter().map(|r| (r.recipe, r.ready)).collect::<Vec<_>>(), vec![(FormId(RCPE_B), true), (FormId(RCPE_A), false)]);
        open.selected = 0;
        open.drawn = Some(initial);

        // Script adds ING so Alpha becomes ready and sorts alphabetically ahead of Beta.
        sim.world.add_item(FormId::PLAYER_REF, FormId(ING), 1).unwrap();
        refresh_open(rt, &mut sim, &mut open);
        assert!(open.drawn.is_none(), "cached view cleared so hand-built & tile renderers redraw");
        assert_eq!(open.selected, 1, "selection stays on Beta Brew after Alpha Brew sorts above it");
        let after = view(rt, &mut sim, &open);
        assert_eq!(after.rows.iter().map(|r| (r.recipe, r.ready)).collect::<Vec<_>>(), vec![(FormId(RCPE_A), true), (FormId(RCPE_B), true)]);
        assert_eq!(after.rows[after.selected].recipe, FormId(RCPE_B));
    }
}
