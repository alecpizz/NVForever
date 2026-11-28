//! The barter view behind `[UI] TileMenus=BarterMenu` [design: barter-menu.md section 9].
//! Commands, sounds and events remain `crate::menu`'s. Shared list state lives in
//! `super::item_menu`; comparisons with the original are in section 9. Off, this adapter
//! does no work.

use super::item_menu::{self, on, set, MenuTile, MenuTiles, Paths, SIDES};
use super::menus::Tiled;
use crate::menu::{barter, model::{Category, MenuModel, Mode, Side}};
use crate::pipboy::items::Info;
use bevy::prelude::*;
use fnv_ui::{Ui, Value};

/// [documented: menus\barter_menu.xml] The menu and the paths written by this adapter.
pub const BARTER_MENU: &str = "BarterMenu";
const BACKGROUND: &str = "BarterMenu/NOGLOW_BRANCH/BM_Background";
const ICON: &str = "BarterMenu/NOGLOW_BRANCH/BM_ItemIcon";
const CARD: &str = "BarterMenu/NOGLOW_BRANCH/BM_ItemData";
const FLOW: &str = "BarterMenu/NOGLOW_BRANCH/BM_CapsFlow";
const FLOW_ICON: &str = "BarterMenu/NOGLOW_BRANCH/BM_CapsFlow/BM_CapsFlowIcon";
const AMOUNT: &str = "BarterMenu/NOGLOW_BRANCH/BM_CapsFlow/BM_TransactionAmount";
const CAPS: &str = "BarterMenu/NOGLOW_BRANCH/BM_CapsFlow/BM_CapsLabel";
const ACCEPT: &str = "BarterMenu/NOGLOW_BRANCH/BM_ButtonRect/BM_ButtonX";
const EXIT: &str = "BarterMenu/NOGLOW_BRANCH/BM_ButtonRect/BM_ButtonB";
const PURSES: [&str; 2] = ["BarterMenu/NOGLOW_BRANCH/BM_ContainerRect/BM_Container_CapsLabel", "BarterMenu/NOGLOW_BRANCH/BM_ItemsRect/BM_Items_CapsLabel"];
const PRICE: &str = "BM_list_template_ItemValue";
const MARK: &str = "BM_list_template_ItemMarker";

/// [documented: barter_menu.xml] Names only; the file supplies places and expressions.
const PATHS: Paths = Paths {
    menu: BARTER_MENU,
    lists: ["BarterMenu/NOGLOW_BRANCH/BM_ContainerRect/BM_Container_InventoryList", "BarterMenu/NOGLOW_BRANCH/BM_ItemsRect/BM_Items_InventoryList"],
    titles: ["BarterMenu/NOGLOW_BRANCH/BM_ContainerRect/BM_ContainerTitle", "BarterMenu/NOGLOW_BRANCH/BM_ItemsRect/BM_ItemsTitle"],
    template: "BM_list_template", row: "BM_list_template_container", row_text: "ListItemText", row_mark: MARK,
    targets: &[
        ("BM_ButtonX", MenuTile::Accept), ("BM_ButtonB", MenuTile::Exit),
        ("BM_Items_LeftFilterArrow", MenuTile::Arrow(Side::Player, -1)),
        ("BM_Items_RightFilterArrow", MenuTile::Arrow(Side::Player, 1)),
        ("BM_Container_LeftFilterArrow", MenuTile::Arrow(Side::Container, -1)),
        ("BM_Container_RightFilterArrow", MenuTile::Arrow(Side::Container, 1)),
        ("BM_ItemsTitle", MenuTile::Title(Side::Player)),
        ("BM_ContainerTitle", MenuTile::Title(Side::Container)),
        ("BM_Items_FocusRect", MenuTile::Focus(Side::Player)),
        ("BM_Container_FocusRect", MenuTile::Focus(Side::Container)),
    ],
};

/// [design: menu::menu_keys, pad::menu_buttons] Physical keys already handled by the model.
/// Script MenuTapKey still follows the file's `_PCButton_A` and `_PCButton_X`.
pub const HAND_BUILT_KEYS: [&str; 16] = ["A", "X", "W", "S", "xup", "xdown", "xleft", "xright", "xbuttona", "xbuttonb", "xbuttonx", "xbuttonlb", "xbuttonrb", "xbuttonlt", "xbuttonrt", "xbuttonstart"];

/// [design] A short or restructured file keeps the entire hand-built view.
pub fn built(ui: &Ui) -> Tiled {
    let mut paths: Vec<String> = [BARTER_MENU, BACKGROUND, ICON, CARD, FLOW, FLOW_ICON, AMOUNT, CAPS, ACCEPT, EXIT].into_iter().chain(PURSES).chain(PATHS.titles).map(str::to_owned).collect();
    for button in [ACCEPT, EXIT] {
        paths.extend(["button_text", "PCShortcutLabel"].map(|t| format!("{button}/{t}")));
    }
    for side in ["BM_ContainerRect/BM_Container", "BM_ItemsRect/BM_Items"] {
        for arrow in ["LeftFilterArrow", "RightFilterArrow"] {
            paths.push(format!("{BARTER_MENU}/NOGLOW_BRANCH/{side}_{arrow}"));
        }
    }
    for path in PATHS.lists {
        paths.push(path.into());
        paths.push(format!("{path}/{}", super::list::HIGHLIGHT));
        for piece in [super::list::MARKER, super::list::UP, super::list::DOWN, super::list::PAGE_UP, super::list::PAGE_DOWN] {
            paths.push(format!("{path}/{}/{piece}", super::list::SCROLLBAR));
        }
    }
    paths.extend(super::item_card::CARDS.iter().chain(&[super::item_card::EFFECTS, super::item_card::CONDITION_ARROWS, super::item_card::CONDITION_METER, super::item_card::CONDITION_METER_IMAGE]).map(|c| format!("{CARD}/{c}")));
    for card in super::item_card::CARDS.iter().chain(&[super::item_card::EFFECTS]) {
        paths.extend(super::item_card::CARD_TEXTS.map(|t| format!("{CARD}/{card}/{t}")));
    }
    item_menu::built(ui, &PATHS, paths, &[("text", PATHS.row_text), ("text", PRICE), ("image", MARK)])
}

pub fn is_barter(m: Option<&MenuModel>) -> bool {
    m.is_some_and(|m| matches!(m.mode, Mode::Barter(_)))
}

/// [design] This decision is shared by the writer, drawing and prompt writer.
pub fn tiled(on: bool, game: &fnv_game::Game<'_>, open: bool) -> bool {
    on && open && game.ui_files.is_some() && game.menu == Some(crate::script::MENU_BARTER) && built(&game.ui_tree).yes()
}

/// [design: LEGAL.md] Setting for all items; our own functional category words elsewhere.
fn title(category: Category, items: &str) -> String {
    match category {
        Category::All => items,
        Category::Weapons => "Weapons",
        Category::Apparel => "Clothing",
        Category::Aid => "Consumables",
        Category::Misc => "Other items",
        Category::Ammo => "Ammunition",
    }.into()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    common: item_menu::Snapshot,
    rows: [Vec<(String, bool)>; 2],
    purses: [String; 2],
    caps: String,
    hints: [String; 2],
    balance: i64,
    /// The figure: what the trade moves, held at a merchant's purse (`Trade::paid`).
    amount: u64,
    chosen: bool,
    affordable: bool,
}

impl Snapshot {
    /// [design] Read model figures, never `look::half_figure`'s transcribed caps heading.
    pub fn of(m: &MenuModel, info: Option<&Info>, items: String, caps: String, hints: [String; 2]) -> Self {
        let mut common = item_menu::Snapshot::of(m, info, None, String::new());
        common.titles = SIDES.map(|side| {
            if side == Side::Container && m.category[side as usize] == Category::All {
                m.title.clone()
            } else {
                title(m.category[side as usize], &items)
            }
        });
        let trade = m.trade.as_ref();
        Self {
            common,
            rows: SIDES.map(|side| m.list(side).rows.iter().map(|r| (r.price.map_or_else(|| "--".into(), barter::price_text), r.bartered)).collect()),
            purses: SIDES.map(|side| format!("{caps}   {}", trade.map_or(0, |t| t.purse[side as usize]))),
            caps, hints,
            balance: trade.map_or(0, |t| t.balance),
            amount: trade.map_or(0, |t| t.paid().unsigned_abs()),
            chosen: trade.is_some_and(|t| !t.is_empty()),
            affordable: trade.is_none_or(|t| t.affordable()),
        }
    }
}

/// [design] A separate resource, with the same list implementation as the container.
#[derive(Resource)]
pub struct BarterTiles {
    pub(super) view: MenuTiles,
    shown: Option<Snapshot>,
    last_focus: Option<Side>,
    normal_quote: Option<crate::menu::model::Trade>,
}

impl Default for BarterTiles {
    fn default() -> Self { Self { view: MenuTiles::new(&PATHS), shown: None, last_focus: None, normal_quote: None } }
}

impl BarterTiles {
    pub fn menu(&self) -> Option<fnv_ui::TileId> { self.view.menu }
    pub(crate) fn normal_text_scale(&self)->f32 {self.view.text_scale()}

    #[cfg(test)]
    pub(crate) fn normal_touch_value(&self,side:Side)->f64 {self.view.value(side)}

    pub(crate) fn normal_touch_quantity(&self, ui: &Ui) -> bool {
        super::menus::normal_touch_quantity_shown(ui)
    }

    /// The ordinary native list's painted/clipped hit data. Disabled-side focus
    /// rectangles do not steal the row: the physical adapter already resolves a
    /// cross-column row through pointer_candidates. No file traits are changed here.
    pub(crate) fn normal_touch_hit(&self, ui: &Ui, m: &MenuModel, point: Vec2, window: Vec2)
        -> Option<crate::menu::normal_touch::Hit> {
        use crate::menu::{model::Command, normal_touch::{Hit, Source, Target}};
        use fnv_ui::input::{hit_candidates, no_measure, top_hit};
        if self.normal_quote.as_ref()!=m.trade.as_ref(){return None}
        if m.quantity.is_some() {
            return super::menus::normal_touch_quantity_hit(ui, point, window);
        }
        let at=super::input::to_menu_units(point,window,Vec2::splat(0.5));
        let menu=self.menu()?;
        let mut candidates=hit_candidates(ui,menu,&no_measure);
        for c in &mut candidates {
            match self.view.clicked(ui,c.under) {
                Some(MenuTile::Focus(_))=>c.event=None,
                Some(MenuTile::Row(..))=>c.event=Some(c.under),
                _=>{}
            }
        }
        if let Some(picked)=top_hit(ui,&candidates,at) {
            if let Some(what)=self.view.clicked(ui,picked.tile) {
                let (target,pane,key)=match what {
                    MenuTile::Row(side,index)=>{
                        let index=self.view.row_at(m,side,index)?;
                        let shown=self.shown.as_ref()?;
                        let painted=shown.common.lists[side as usize].rows.get(index)?;
                        let now=m.list(side).rows.get(index)?;
                        let price=now.price.map_or_else(||"--".into(),barter::price_text);
                        if painted.text!=crate::menu::look::row_text(now)
                            || shown.rows[side as usize].get(index)!=Some(&(price,now.bartered)) {return None}
                        (Target::Row(side,index),Some(side),self.view.rows[side as usize].get(index)?.tile)
                    }
                    MenuTile::Arrow(side,by)=>(Target::Command(Command::Category(side,by)),None,picked.tile),
                    MenuTile::Title(side)=>(Target::CategoryHeader(side),None,picked.tile),
                    MenuTile::Accept=>(Target::Command(Command::Accept),None,picked.tile),
                    MenuTile::Exit=>(Target::Command(Command::Close),None,picked.tile),
                    MenuTile::Focus(side)=>(Target::Pane(side),Some(side),picked.tile),
                    _=>return None,
                };
                return Some(Hit {source:Source::Tile(menu,key),target,pane,row_step:0.0});
            }
        }
        // Padded finger targets around category arrows and titles so small header
        // arrows and labels are easy to tap or swipe without resizing the artwork.
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
            match self.view.clicked(ui, c.under) {
                Some(MenuTile::Arrow(side, by)) => {
                    return Some(Hit {
                        source: Source::Direct(menu, c.under),
                        target: Target::Command(Command::Category(side, by)),
                        pane: None,
                        row_step: 0.0,
                    });
                }
                Some(MenuTile::Title(side)) => {
                    return Some(Hit {
                        source: Source::Direct(menu, c.under),
                        target: Target::CategoryHeader(side),
                        pane: None,
                        row_step: 0.0,
                    });
                }
                _ => {}
            }
        }
        // The ordinary inactive-column focus rectangle can also be empty.
        // Its actual clipped area remains a browse pane, never an action.
        let c=candidates.iter().rev().find(|c|{
            let r=c.region();
            r.width>0.0 && r.height>0.0 && at.0>=r.left && at.0<r.left+r.width
                && at.1>=r.top && at.1<r.top+r.height
                && matches!(self.view.clicked(ui,c.under),Some(MenuTile::Focus(_)))
        })?;
        let Some(MenuTile::Focus(side))=self.view.clicked(ui,c.under) else{return None};
        Some(Hit {source:Source::Tile(menu,c.under),target:Target::Pane(side),pane:Some(side),row_step:0.0})
    }

    /// Scroll this exact native view, without changing focus or selected item.
    pub(crate) fn normal_touch_scroll(&mut self, ui: &mut Ui, side: Side, pixels: f64, window: Vec2) {
        let Some(list)=self.view.list_tile(ui,side) else{return};
        let Some(delta)=ui.tile(list).get("_scroll_delta").map(Value::num).filter(|d|d.is_finite() && *d>0.0) else{return};
        let a=super::input::to_menu_units(Vec2::ZERO,window,Vec2::splat(0.5));
        let b=super::input::to_menu_units(Vec2::Y,window,Vec2::splat(0.5));
        let value=self.view.value(side)+pixels*(b.1-a.1)/delta;
        if value.is_finite() && self.view.scroll_to(side,value) {
            self.view.rewrite(ui);
            ui.update();
        }
    }

    fn close(&mut self) {
        self.normal_quote = None;
        self.view.close();
        self.shown = None;
        self.last_focus = None;
    }
}

/// [design] File places and textures stay in the tree. Section 9 distinguishes measured
/// barter writes from remaining container hypotheses; selection and affordability are the model's.
fn write(ui: &mut Ui, s: &Snapshot, st: &mut BarterTiles) {
    item_menu::write_lists(ui, &s.common, &mut st.view);
    for side in SIDES {
        let i = side as usize;
        set(ui, PURSES[i], "string", Value::Str(s.purses[i].clone()));
        for ((row, (price, bartered)), shown) in st.view.rows[i].iter().zip(&s.rows[i]).zip(&s.common.lists[i].rows) {
            if let Some(t) = super::list::child(ui, row.tile, PRICE) {
                ui.set_trait(t, "string", Value::Str(price.clone()));
            }
            if let Some(t) = super::list::child(ui, row.tile, MARK) {
                ui.set_trait(t, "visible", on(*bartered || shown.equipped));
                // [documented: barter-menu.md sections 1 and 3] Open for an offered item.
                ui.set_trait(t, "filename", Value::Str(if *bartered { "interface\\shared\\marker\\square.dds" } else { "interface\\shared\\marker\\square_filled.dds" }.into()));
            }
        }
    }
    if s.common.lists[s.common.focus as usize].selected.is_some() {
        st.last_focus = Some(s.common.focus);
    }
    set(ui, BARTER_MENU, "_IsContainerListSelected", st.last_focus.map_or(Value::Num(-1.0), |side| on(side == Side::Container)));
    // [observed 2026-11-06: ov_barter] Selling is 2, buying 1; no balance is empty.
    set(ui, BARTER_MENU, "_ArrowDirection", match s.balance.cmp(&0) {
        std::cmp::Ordering::Greater => Value::Num(2.0),
        std::cmp::Ordering::Less => Value::Num(1.0),
        std::cmp::Ordering::Equal => Value::Str(String::new()),
    });
    set(ui, FLOW, "visible", on(s.balance != 0));
    set(ui, FLOW, "_Value", Value::Num(s.amount as f64));
    set(ui, AMOUNT, "string", Value::Str(s.amount.to_string()));
    set(ui, CAPS, "string", Value::Str(s.caps.clone()));
    // [observed 2026-11-06: ov_barter] The flow's alpha is 64 when unaffordable.
    // [design: item_menu::mark_tiles] Texts use their own alpha; the file's icon copies FLOW.
    for path in [FLOW, AMOUNT, CAPS] {
        set(ui, path, "alpha", Value::Num(if s.affordable { 255.0 } else { 64.0 }));
    }
    // The file leaves both buttons' `visible` to code, and Exit stands under Accept only while
    // Accept's is set: both read 1 [observed 2026-11-06: ov_barter, Exit's y 47].
    set(ui, ACCEPT, "visible", on(true));
    set(ui, EXIT, "visible", on(true));
    set(ui, ACCEPT, "target", on(s.chosen));
    for (button, hint) in [ACCEPT, EXIT].into_iter().zip(&s.hints) {
        set(ui, button, "_PCButtonText", Value::Str(hint.clone()));
    }
    // [hypothesis] As with the container, the text inherits no parent alpha/brightness.
    for path in [ACCEPT.to_string(), format!("{ACCEPT}/button_text"), format!("{ACCEPT}/PCShortcutLabel")] {
        set(ui, &path, "brightness", Value::Num(if s.chosen { 255.0 } else { 128.0 }));
    }
    set(ui, ICON, "visible", on(s.common.card.is_some() && s.common.icon.is_some()));
    set(ui, ICON, "filename", Value::Str(s.common.icon.clone().unwrap_or_default()));
    super::item_card::write(ui, CARD, s.common.card.as_ref());
    // [hypothesis] Container backing measurement reused until the barter capture arrives.
    set(ui, BACKGROUND, "alpha", Value::Num(super::container_menu::BACKGROUND_ALPHA));
}

/// [design] The writer runs before input; only the enabled adapter reads item figures.
#[allow(clippy::too_many_arguments)]
pub(super) fn write_system(
    mut script: ResMut<crate::script::ScriptState>, switch: Res<super::vanilla::TileMenus>,
    active: Res<crate::menu::ActiveMenu>, mut sim: ResMut<crate::sim::SimState>,
    (combat, play): (Option<Res<crate::combat::Combat>>, Option<Res<crate::PlayWorld>>),
    mut forms: Local<Option<crate::pipboy::stats::read::Forms>>, mut fonts: super::draw::FontKit,
    mut st: ResMut<BarterTiles>,
    readability:Option<Res<crate::menu_readability::MenuReadability>>,
) {
    if !switch.barter { return; }
    let scale=crate::menu::readability::size(readability.as_deref(),crate::menu_readability::MenuKind::Barter);
    st.view.set_text_scale(scale);
    let model = active.0.as_ref().filter(|_| is_barter(active.0.as_ref()));
    let Some(rt) = script.rt.as_ref() else { return };
    let game = &rt.game;
    if model.is_some() && game.ui_files.is_some() && game.menu == Some(crate::script::MENU_BARTER) && !st.view.warned {
        if let Tiled::No(reason) = built(&game.ui_tree) {
            warn!("{BARTER_MENU} drawn by hand: {reason}");
            st.view.warned = true;
        }
    }
    let Some(m) = model else { st.close(); return };
    st.view.menu = if tiled(true, game, true) { game.ui_tree.menu(BARTER_MENU) } else { None };
    let Some(menu) = st.menu() else { return };
    let due = active.is_changed() || st.view.needs_snapshot(menu);
    let words = due.then(|| {
        let word = |key| fnv_game::ui_loading::game_string(game, key);
        let pad = crate::pad::at_menu(crate::script::MENU_BARTER);
        (word("sInventoryItems"), word("sInventoryCaps"), [
            crate::pad::game_hint(&word("sPCMenuHintA"), &[KeyCode::KeyA], pad),
            crate::pad::game_hint(&word("sPCMenuHintX"), &[KeyCode::KeyX], pad),
        ])
    });
    let new = words.map(|(items, caps, hints)| {
        if forms.is_none() { *forms = play.as_ref().map(|p| crate::pipboy::stats::read::Forms::new(p.lo, p.schema)); }
        let info = m.selected_row(m.focus).map(|r| crate::pipboy::items::info_of(r, combat.as_deref(), sim.bypass_change_detection(), forms.as_mut(), Some(script.bypass_change_detection())));
        Snapshot::of(m, info.as_ref(), items, caps, hints)
    });
    let Some(rt) = script.rt.as_mut() else { return };
    let ui = &mut rt.game.ui_tree;
    let mut told = false;
    if let Some(s) = new {
        let changed_menu = st.view.needs_snapshot(menu);
        st.view.is_new(menu, &s.common);
        if changed_menu || st.shown.as_ref() != Some(&s) {
            write(ui, &s, &mut st);
            told = true;
                st.shown = Some(s);
        }
            // A changed exact price can round to the same drawn text. The writer
            // has still established that this current quote is what the view shows.
            if st.normal_quote.as_ref() != m.trade.as_ref() {
                st.normal_quote = m.trade.clone();
            }
    }
    let texts = super::menus::texts(ui, menu);
    st.view.settle_shown(ui, &mut |ui, sizes| {
        let size = fonts.menu_measure(&super::menus::fonts_of(ui, &texts));
        super::menus::write_text_sizes_of(ui,&texts,sizes,&|font,text,wrap|
            crate::menu::readability::measured(&size,scale,font,text,wrap))
    });
    mark_tiles(ui);
    if let (true, Some(s)) = (told, st.shown.as_ref()) {
        let picked = s.common.lists[s.common.focus as usize].selected;
        let selected = picked.and_then(|i| s.common.lists[s.common.focus as usize].rows.get(i));
        let price = picked.and_then(|i| s.rows[s.common.focus as usize].get(i)).map_or("--", |r| r.0.as_str());
        info!("tiles: {BARTER_MENU} drawn from tiles: player {} rows, merchant {} rows; balance {}; offered {}; affordable {}; headings {:?}; purses {:?}; selected {:?} price {:?}; figures: {}",
            s.common.lists[1].rows.len(), s.common.lists[0].rows.len(), s.balance, s.chosen, s.affordable,
            s.common.titles, s.purses, selected.map_or("none", |r| r.text.as_str()), price, super::item_card::describe(ui, CARD));
    }
}

pub fn mark_tiles(ui: &mut Ui) { item_menu::mark_tiles(ui, &PATHS); }

/// [design: tiles/input.rs, fnv_ui::input] A focus rect stands over disabled rows. Resolve
/// this pointer step with that side enabled, then restore the written focus: the usual
/// guarded Hover or row click changes the model, including a click that crosses sides.
/// Only hit candidates are retained, so an empty area or a wheel-held selection cannot
/// leave the tree's focus ahead of the model. Higher menus keep their pointer priority.
pub(super) fn pointer_candidates(ui: &mut Ui, st: &BarterTiles, taking: &[fnv_ui::TileId], at: (f64, f64)) -> Option<Vec<fnv_ui::input::HitCandidate>> {
    let hit = fnv_ui::input::hit(ui, taking, at, &fnv_ui::input::no_measure)?;
    let MenuTile::Focus(side) = st.view.clicked(ui, hit.tile)? else { return None };
    let menu = st.menu()?;
    let previous = ui.tile(menu).get("_IsContainerListSelected")?.clone();
    ui.set_trait(menu, "_IsContainerListSelected", on(side == Side::Container));
    ui.update();
    let candidates = fnv_ui::input::hit_candidates(ui, menu, &fnv_ui::input::no_measure);
    ui.set_trait(menu, "_IsContainerListSelected", previous);
    ui.update();
    Some(candidates)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::menu::model::RowKey;
    use fnv_sim::FormId;
    use fnv_ui::MemSource;

    /// Install the actual writer in lean, authored normal-input Apps. This is a
    /// provider seam, not a replacement snapshot/effect writer.
    pub(crate) fn install_normal_writer(app: &mut App) {
        app.init_resource::<super::super::draw::TileFonts>()
            .add_systems(Update, write_system.after(crate::menu::MenuSystems));
    }

    /// [design] A fixture written for these tests, with its own dimensions and simple
    /// dependencies. No expressions or values were taken from the game menu file.
    pub(crate) fn fixture(change: impl FnOnce(String) -> String) -> Ui {
        let side = |prefix: &str, x: u32| format!(r#"
            <rect name="{prefix}Rect"><x>{x}</x>
              <text name="{prefix}Title"/><text name="{prefix}_CapsLabel"/>
              <image name="{prefix}_LeftFilterArrow"/><image name="{prefix}_RightFilterArrow"/>
              <hotrect name="{prefix}_InventoryList"><height>95</height><width>240</width>
                <clipwindow>1</clipwindow><_scroll_delta>13</_scroll_delta>
                <_offset><copy src="child(lb_scrollbar)" trait="_current_value"/><mul src="me()" trait="_scroll_delta"/></_offset>
                <hotrect name="lb_highlight_box"/>
                <image name="lb_scrollbar"><_current_value>0</_current_value>
                  <image name="scrollbar_vert_marker"/><image name="scrollbar_vert_up"/><image name="scrollbar_vert_down"/>
                  <hotrect name="scrollbar_vert_page_up"/><hotrect name="scrollbar_vert_page_down"/>
                </image>
              </hotrect>
            </rect>"#);
        let cards: String = super::super::item_card::CARDS.into_iter().chain([super::super::item_card::EFFECTS]).map(|card| {
            let extra = if card == "ConditionInfo" { r#"<rect name="ConditionMeter"><image name="MeterImage"/></rect><image name="CNDArrows"/>"# } else { "" };
            format!(r#"<rect name="{card}"><_Title>Own label</_Title><text name="CI_TitleText"/><text name="CI_ValueText"/>{extra}</rect>"#)
        }).collect();
        let xml = format!(r#"<menu name="BarterMenu"><rect name="NOGLOW_BRANCH">
          <image name="BM_Background"/>{}{}
          <rect name="BM_CapsFlow"><image name="BM_CapsFlowIcon">
            <_pic_1>test-buy.dds</_pic_1><_pic_2>test-sell.dds</_pic_2>
            <filename>discarded.dds<copy src="BarterMenu" trait="_ArrowDirection"/><copy src="me()" trait="_pic_"/></filename>
            <alpha><copy src="parent()" trait="alpha"/><mul>1</mul></alpha>
          </image><text name="BM_TransactionAmount"/><text name="BM_CapsLabel"/></rect>
          <rect name="BM_ButtonRect"><image name="BM_ButtonX"><text name="button_text"><string>Trade</string></text><text name="PCShortcutLabel"/></image>
          <image name="BM_ButtonB"><text name="button_text"><string>Leave</string></text><text name="PCShortcutLabel"/></image></rect>
          <image name="BM_ItemIcon"/><rect name="BM_ItemData"><user1>25</user1>{cards}</rect>
          </rect><template name="BM_list_template"><hotrect name="BM_list_template_container"><width>220</width><target>1</target>
          <_y>0</_y><_VerticalSpacing>8</_VerticalSpacing>
          <y><copy src="me()" trait="_y"/><sub src="parent()" trait="_offset"/></y>
          <text name="ListItemText"/><text name="BM_list_template_ItemValue"/><image name="BM_list_template_ItemMarker"/>
          </hotrect></template></menu>"#, side("BM_Items", 60), side("BM_Container", 460));
        let mut ui = Ui::new();
        ui.load_menu(&MemSource::default().with("fixture.xml", &change(xml)), "fixture.xml").unwrap();
        ui.update();
        ui
    }

    pub(crate) fn snapshot() -> Snapshot {
        let row = |id, equipped| item_menu::RowShown {
            key: RowKey { units: (FormId(id), None, 0), reference: None, nth: 0 },
            name: format!("Item {id}"), text: format!("Item {id}"), equipped, dim: false, ysi: None,
        };
        Snapshot {
            common: item_menu::Snapshot {
                lists: [item_menu::ListShown { rows: vec![row(1, false)], selected: None, ..Default::default() },
                    item_menu::ListShown { rows: vec![row(2, true), row(3, false), row(4, false), row(5, false)], selected: Some(0), ..Default::default() }],
                titles: ["Merchant".into(), "Personal items".into()], weight: String::new(), focus: Side::Player,
                pointer_pick: false, asking: false, navigated: 0,
                card: Some(Info { weight: Some("2.50".into()), value: Some("8".into()), ..Default::default() }),
                icon: None, take_all: false, take_all_hint: String::new(), note: None, ysi_icons: true,
            },
            rows: [vec![("0.9".into(), true)], vec![("--".into(), false), ("18".into(), false), ("2".into(), false), ("3".into(), false)]],
            purses: ["Coins   200".into(), "Coins   40".into()], caps: "Coins".into(), hints: ["[A]".into(), "[X]".into()],
            balance: 18, amount: 18, chosen: true, affordable: true,
        }
    }

    pub(crate) fn frame(ui: &mut Ui, s: &Snapshot, st: &mut BarterTiles) {
        let menu = ui.menu(BARTER_MENU).unwrap();
        st.view.menu = Some(menu);
        st.view.is_new(menu, &s.common);
        write(ui, s, st);
        let texts = super::super::menus::texts(ui, menu);
        st.view.settle_shown(ui, &mut |ui, sizes| super::super::menus::write_text_sizes_of(ui, &texts, sizes, &super::super::list::lines_size));
        mark_tiles(ui);
    }

    pub(crate) fn of_model(m: &MenuModel) -> Ui {
        let mut ui = fixture(|s| s);
        let s = Snapshot::of(m, None, "Personal items".into(), "Coins".into(), ["[A]".into(), "[X]".into()]);
        frame(&mut ui, &s, &mut BarterTiles::default());
        ui
    }

    fn number(ui: &Ui, path: &str, name: &str) -> f64 { ui.get_float(&format!("{path}/{name}")).unwrap() }
    fn string(ui: &Ui, path: &str, name: &str) -> String { ui.get_string(&format!("{path}/{name}")).unwrap() }

    #[test]
    fn incomplete_file_or_price_template_falls_back_whole() {
        assert_eq!(built(&fixture(|s| s)), Tiled::Yes);
        for missing in ["BM_CapsFlowIcon", "BM_Container_CapsLabel", "BM_list_template_ItemValue", "CI_ValueText", "PCShortcutLabel", "scrollbar_vert_marker"] {
            let ui = fixture(|s| s.replace(missing, "Absent"));
            assert!(!built(&ui).yes(), "{missing}");
        }
        assert!(!built(&Ui::new()).yes());
    }

    #[test]
    fn prices_purses_marks_and_unit_card_are_written() {
        let (mut ui, mut st) = (fixture(|s| s), BarterTiles::default());
        frame(&mut ui, &snapshot(), &mut st);
        let offered = st.view.rows[0][0].tile;
        let equipped = st.view.rows[1][0].tile;
        let child = |tile, name| super::super::list::child(&ui, tile, name).unwrap();
        assert_eq!(ui.tile(child(offered, PRICE)).get("string").unwrap().text(), "0.9");
        assert_eq!(ui.tile(child(equipped, PRICE)).get("string").unwrap().text(), "--");
        assert_eq!(ui.tile(child(offered, MARK)).get("filename").unwrap().text(), "interface\\shared\\marker\\square.dds");
        assert_eq!(ui.tile(child(equipped, MARK)).get("filename").unwrap().text(), "interface\\shared\\marker\\square_filled.dds");
        assert!(ui.tile(child(offered, MARK)).get("visible").unwrap().truthy());
        assert_eq!(string(&ui, PURSES[1], "string"), "Coins   40");
        assert_eq!(string(&ui, CARD, "WeightInfo/_Value"), "2.50");
        assert_eq!(string(&ui, CARD, "ValueInfo/_Value"), "8");
        assert_eq!(string(&ui, CARD, "WeightInfo/_Title"), "Own label");
        assert_eq!(string(&ui, ACCEPT, "_PCButtonText"), "[A]");
        assert_eq!(string(&ui, EXIT, "_PCButtonText"), "[X]");
    }

    #[test]
    fn balance_direction_affordability_and_empty_accept_are_distinct() {
        let (mut ui, mut st, mut s) = (fixture(|s| s), BarterTiles::default(), snapshot());
        frame(&mut ui, &s, &mut st);
        assert_eq!(number(&ui, BARTER_MENU, "_ArrowDirection"), 2.0);
        assert_eq!(string(&ui, FLOW_ICON, "filename"), "test-sell.dds");
        assert_eq!(number(&ui, FLOW, "alpha"), 255.0);
        s.balance = 37;
        s.amount = 25;
        s.affordable = false;
        s.common.focus = Side::Container;
        s.common.lists[0].selected = Some(0);
        s.common.lists[1].selected = None;
        frame(&mut ui, &s, &mut st);
        assert_eq!(number(&ui, BARTER_MENU, "_IsContainerListSelected"), 1.0);
        assert_eq!(number(&ui, FLOW, "alpha"), 64.0);
        let drawn = super::super::layout::layout(&ui, st.menu().unwrap(), &super::super::list::lines_size, true);
        for path in [FLOW_ICON, AMOUNT, CAPS] {
            let tile = ui.find_tile(path).unwrap();
            assert_eq!(drawn.iter().find(|d| d.tile == tile).unwrap().alpha, 64.0, "{path}");
            assert!(ui.tile(tile).get("brightness").is_none(), "{path}");
        }
        assert_eq!(string(&ui, AMOUNT, "string"), "25", "a sale beyond the merchant's purse shows that purse");
        assert_eq!(number(&ui, ACCEPT, "brightness"), 255.0, "a chosen trade stays bright even if unaffordable");
        s.balance = -37;
        s.amount = 37;
        s.affordable = true;
        frame(&mut ui, &s, &mut st);
        assert_eq!(number(&ui, BARTER_MENU, "_ArrowDirection"), 1.0);
        assert_eq!(string(&ui, FLOW_ICON, "filename"), "test-buy.dds");
        for path in [FLOW, FLOW_ICON, AMOUNT, CAPS] {
            assert_eq!(number(&ui, path, "alpha"), 255.0, "{path}");
        }
        s.balance = 0;
        s.amount = 0;
        frame(&mut ui, &s, &mut st);
        assert_eq!(string(&ui, BARTER_MENU, "_ArrowDirection"), "");
        assert_eq!(number(&ui, FLOW, "visible"), 0.0);
        assert_eq!(number(&ui, ACCEPT, "target"), 1.0, "a zero-net trade can still be accepted");
        s.chosen = false;
        frame(&mut ui, &s, &mut st);
        assert_eq!(number(&ui, ACCEPT, "target"), 0.0);
        assert_eq!(number(&ui, ACCEPT, "button_text/brightness"), 128.0);
    }

    #[test]
    fn focus_starts_unselected_and_retains_the_last_selected_side_until_close() {
        let (mut ui, mut st, mut s) = (fixture(|s| s), BarterTiles::default(), snapshot());
        s.common.lists[1].selected = None;
        frame(&mut ui, &s, &mut st);
        assert_eq!(number(&ui, BARTER_MENU, "_IsContainerListSelected"), -1.0);
        for side in [Side::Player, Side::Container] {
            s.common.focus = side;
            s.common.lists[side as usize].selected = Some(0);
            frame(&mut ui, &s, &mut st);
            let expected = if side == Side::Player { 0.0 } else { 1.0 };
            assert_eq!(number(&ui, BARTER_MENU, "_IsContainerListSelected"), expected);
            s.common.lists[side as usize].selected = None;
            frame(&mut ui, &s, &mut st);
            assert_eq!(number(&ui, BARTER_MENU, "_IsContainerListSelected"), expected);
        }
        st.close();
        frame(&mut ui, &s, &mut st);
        assert_eq!(number(&ui, BARTER_MENU, "_IsContainerListSelected"), -1.0);
    }

    #[test]
    fn pointer_crosses_focus_rects_to_disabled_rows_and_clicks_the_row_under_it() {
        use fnv_ui::input::{hit, no_measure, Input, Pointer};
        use crate::menu::model::Command;
        let mut ui = fixture(|mut xml| {
            for (prefix, active) in [("BM_Items", 0), ("BM_Container", 1)] {
                // [design] Synthetic equality gates, geometry and IDs for this test.
                let selected = format!(r#"<copy src="BarterMenu" trait="_IsContainerListSelected"/><eq>{active}</eq>"#);
                xml = xml.replace(&format!(r#"<hotrect name="{prefix}_InventoryList">"#), &format!(r#"<hotrect name="{prefix}_InventoryList"><_enabled>{selected}</_enabled>"#));
                xml = xml.replace(&format!(r#"<text name="{prefix}Title"/>"#), &format!(r#"<hotrect name="{prefix}_FocusRect"><id>{}</id><width>240</width><height>95</height><target><copy>1</copy><sub>{selected}</sub></target></hotrect><text name="{prefix}Title"/>"#, active + 30));
            }
            xml.replace("<target>1</target>", r#"<target><copy src="parent()" trait="_enabled"/></target>"#)
        });
        let (mut st, mut s, mut pointer) = (BarterTiles::default(), snapshot(), Pointer::default());
        frame(&mut ui, &s, &mut st);
        let menu = st.menu().unwrap();
        let step = |ui: &mut Ui, st: &BarterTiles, pointer: &mut Pointer, at, press| {
            let resolved = pointer_candidates(ui, st, &[menu], at);
            pointer.step(ui, &[menu], &Input { at: Some(at), press, resolved, ..Default::default() }, &no_measure)
        };
        let at = (480.0, 10.0);
        let rect = hit(&ui, &[menu], at, &no_measure).unwrap().tile;
        assert_eq!(st.view.clicked(&ui, rect), Some(MenuTile::Focus(Side::Container)));
        let report = step(&mut ui, &st, &mut pointer, at, false);
        let to = report.over.unwrap().1.unwrap();
        assert_eq!(st.view.clicked(&ui, to), Some(MenuTile::Row(Side::Container, 0)));
        assert_eq!(number(&ui, BARTER_MENU, "_IsContainerListSelected"), 0.0, "the model still owns focus until Hover applies");
        s.common.focus = Side::Container;
        s.common.lists[0].selected = Some(0);
        s.common.lists[1].selected = None;
        frame(&mut ui, &s, &mut st);
        // [design] A direct click across sides, without a hover frame, must act on row 1.
        let report = step(&mut ui, &st, &mut pointer, (80.0, 50.0), true);
        let click = report.clicks.first().unwrap();
        assert_eq!(st.view.clicked(&ui, click.tile), Some(MenuTile::Row(Side::Player, 1)));
        assert_eq!(item_menu::row_commands(Side::Player, 1, false, false), vec![Command::Hover(Side::Player, 1), Command::One]);
        let guard = st.view.guard(Mode::Barter(FormId(77)), Side::Player, 1, false, 2).unwrap();
        assert_eq!(guard.key, s.common.lists[1].rows[1].key);
        // [design] The focus rect reaches beyond row width, and cannot select a clipped row.
        for at in [(290.0, 10.0), (80.0, 100.0)] {
            let report = step(&mut ui, &st, &mut pointer, at, true);
            assert!(report.clicks.iter().all(|c| !matches!(st.view.clicked(&ui, c.tile), Some(MenuTile::Row(..)))));
        }
        assert!(pointer_candidates(&mut ui, &st, &[], (80.0, 10.0)).is_none(), "a prompt excludes barter from taking input");
        let overlay = ui.load_menu(&MemSource::default().with("overlay.xml", r#"<menu name="Overlay"><hotrect name="Cover"><width>900</width><height>400</height><target>1</target></hotrect></menu>"#), "overlay.xml").unwrap();
        ui.update();
        assert!(pointer_candidates(&mut ui, &st, &[menu, overlay], (80.0, 10.0)).is_none(), "a higher menu keeps the pointer");
    }

    #[test]
    fn children_stand_for_actions_and_stale_rows_are_guarded() {
        let (mut ui, mut st) = (fixture(|s| s), BarterTiles::default());
        frame(&mut ui, &snapshot(), &mut st);
        for (path, what) in [(format!("{ACCEPT}/button_text"), MenuTile::Accept), (format!("{EXIT}/PCShortcutLabel"), MenuTile::Exit),
            (PATHS.titles[0].into(), MenuTile::Title(Side::Container)),
            ("BarterMenu/NOGLOW_BRANCH/BM_ItemsRect/BM_Items_RightFilterArrow".into(), MenuTile::Arrow(Side::Player, 1))] {
            assert_eq!(st.view.clicked(&ui, ui.find_tile(&path).unwrap()), Some(what));
        }
        let old = st.view.rows[1][0].tile;
        let mut s = snapshot();
        let mode = Mode::Barter(FormId(77));
        assert_eq!(st.view.guard(mode, Side::Player, 0, true, 1).unwrap().key, s.common.lists[1].rows[0].key);
        s.common.lists[1].rows.remove(0);
        s.rows[1].remove(0);
        assert_eq!(st.view.row_in(Side::Player, 0, &s.common.lists[1].rows), None);
        frame(&mut ui, &s, &mut st);
        assert!(!ui.is_live(old));
        assert_eq!(st.view.clicked(&ui, old), None);
        st.close();
        assert!(st.menu().is_none());
        assert_eq!(st.view.name(), BARTER_MENU, "close preserves the adapter's paths");
    }

    #[test]
    fn shared_scroll_survives_rewrite_and_pointer_picks_do_not_scroll() {
        let (mut ui, mut st, mut s) = (fixture(|s| s), BarterTiles::default(), snapshot());
        frame(&mut ui, &s, &mut st);
        assert!(st.view.scroll_to(Side::Player, 1.0));
        s.common.pointer_pick = true;
        s.common.lists[1].selected = Some(3);
        frame(&mut ui, &s, &mut st);
        assert_eq!(st.view.value(Side::Player), 1.0);
        let list = st.view.list_tile(&ui, Side::Player).unwrap();
        super::super::list::write_value(&mut ui, list, 0.0);
        st.view.rewrite(&mut ui);
        ui.update();
        assert_eq!(super::super::list::value_shown(&ui, list), Some(1.0));
        s.common.pointer_pick = false;
        s.common.navigated += 1;
        frame(&mut ui, &s, &mut st);
        assert!(st.view.value(Side::Player) > 1.0);
    }

    #[test]
    fn physical_buttons_act_once_and_scripts_keep_the_file_bindings() {
        for key in ["A", "X", "xbuttonx", "xbuttonb", "xbuttona", "xleft", "xbuttonrt"] {
            assert!(!super::super::menus::key_click_acts(BARTER_MENU, key, &[key]));
            assert!(super::super::menus::key_click_acts(BARTER_MENU, key, &[]));
        }
        assert_eq!(title(Category::All, "Personal items"), "Personal items");
        assert_eq!(title(Category::Apparel, "Personal items"), "Clothing");
    }

    #[test]
    fn prompt_follows_only_a_successful_barter_view_and_blocks_it() {
        let mut state = crate::script::ScriptState::over_no_plugins();
        let game = &mut state.rt.as_mut().unwrap().game;
        game.ui_files = Some(Box::new(MemSource::default()));
        game.menu = Some(crate::script::MENU_BARTER);
        game.ui_tree = fixture(|s| s);
        let root = game.ui_tree.menu(BARTER_MENU);
        assert!(tiled(true, game, true));
        assert!(!tiled(false, game, true));
        assert!(!tiled(true, game, false));
        assert_eq!(item_menu::takes_input(root, true), None);
        assert!(!super::super::menus::quantity_tiled(false, game, true, true), "a missing prompt stays hand-built");
        game.ui_tree.load_menu(&MemSource::default().with("prompt.xml", r#"<menu name="QuantityMenu"><rect name="NOGLOW_BRANCH"><image name="QM_Background"/><rect name="QM_MainRect"><rect name="QM_AmountMeter"/></rect></rect></menu>"#), "prompt.xml").unwrap();
        assert!(super::super::menus::quantity_tiled(false, game, true, tiled(true, game, true)));
        let caps = game.ui_tree.find_tile(CAPS).unwrap();
        game.ui_tree.unload_tile(caps);
        assert!(!tiled(true, game, true));
        assert!(!super::super::menus::quantity_tiled(false, game, true, tiled(true, game, true)));
    }
}
