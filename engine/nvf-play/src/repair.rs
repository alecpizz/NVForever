//! The repair services menu: a vendor repairs the player's weapons and armour for caps
//! (docs/spec-notes/repair.md; the rules are `fnv_game::repair`).
//!
//! A script opens it (`vendor.ShowRepairMenu`, Raul's dialogue among others). The list
//! holds the player's weapons and armour worn below full condition, each with its
//! condition as a small bar and a filled square for an equipped one; the rows the player
//! can have repaired (affordable, within the vendor's skill) stand first and bright, the
//! others after them and dim [observed: the owner's screenshots, C:\fnv-private\
//! repairmenu: four bright rows, then five dim ones; the order within each part is ours,
//! by name]. An item at full condition is not listed [owner's rule]. At the top right the
//! vendor's repair skill and the player's caps; under them "Repair All (total) A)"
//! (dim when the player cannot pay the total) and "Done E)". For the row under the pointer
//! or the keys: the item's picture, its condition and damage (weapons) or DT (armour) now
//! and after the repair with the gains between ("+66 %", "+3"), and "Cost: N caps" (dim
//! when it cannot be paid). Clicking a row (or Enter) repairs it; A repairs everything
//! the player can pay for at once; E, Esc or Tab closes the menu.
//!
//! The look [documented: the original's `menus\repair_services_menu.xml`, in its units:
//! the menu 1020 by 680 in the middle of the screen; the title in font 6 at 70; the
//! skill and caps in font 2 right-aligned 20 down; the list 60 down, 580 high, its rows
//! 440 wide with the name in font 2 at 40 wrapped at 260, the bar 60 by 16 at 370 and the
//! square 25 at 15; the picture 400 square at 500; the figures from (485, 380): the
//! condition and the stat as cards 263 by 60, the arrow (`repair_arrow_down.dds`) 80
//! square at (223, 50), the gains in font 4 centred at 120 and 400, 90 down, the cards
//! after the repair 140 down and the cost in font 6 230 down; the buttons right-aligned at
//! 1010 from 75 down, 47 apart; the bottom bracket at 680. Observed: the screenshots for
//! the words and how dim the dim rows are].
//!
//! Test aid `NVF_PLAY_TEST_REPAIR=<seconds>:<action>[,…]`: `down`, `up`, `repair` (the
//! chosen row), `all` (Repair All), `done`, `shot` (a screenshot `repair_menu.png` in the
//! working folder). Open the menu through a script, e.g. the console aid's
//! `GSChetRef.ShowRepairMenu`.
//!
//! Behind `[UI] TileMenus=RepairServicesMenu` the menu is drawn from its file instead
//! (`crate::tiles::repair_services_menu`, repair.md §3.1): the rules, sounds and log lines here
//! stay, [`draw`] spawns only a sheet that keeps the pointer from what is under the menu, and
//! what the tiles are asked comes in as [`RepairServicesInput`].

use crate::interface::{amber, bc_supported, face, plain_font, u, Pictures};
use crate::pause::AidTime;
use crate::menu::readability::Typeface as Face;
use crate::script::ScriptState;
use crate::sim::SimState;
use crate::start_menu::{color, highlight_colors, Shade, LINE};
use crate::ui_sounds::{UiEvent, UiMenu, UiSound};
use crate::{PlaySet, PlayState};
use bevy::image::CompressedImageFormatSupport;
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use crate::ui_pointer::{Button, PointerState};
use fnv_game::repair::{gain_percent, vendor_lists, vendor_quote, Gear, Quote, RepairSettings, CAPS, REPAIR_SERVICES_MENU};
use fnv_sim::forms::{BaseData, ItemKind};
use fnv_sim::inventory::Extra;
use fnv_sim::FormId;
use nvf_assets::GameData;

/// The menu's size [documented: repair_services_menu.xml `RSM_MainRect`].
const MAIN: Vec2 = Vec2::new(1020.0, 680.0);
/// The list's box: its top and height; a row's left and width within the menu
/// [documented: `RSM_RepairList` at (-15, 60), 580 high; the template's rows at 32, 440
/// wide].
const LIST_TOP: f32 = 60.0;
const LIST_HEIGHT: f32 = 580.0;
const ROW_X: f32 = 17.0;
const ROW_WIDTH: f32 = 440.0;
/// Within a row: the name, its wrap, the bar and the square [documented: the template].
const NAME_X: f32 = 40.0;
const NAME_WRAP: f32 = 260.0;
const BAR: Rect = Rect { min: Vec2::new(370.0, 20.0), max: Vec2::new(430.0, 36.0) };
const MARKER: Rect = Rect { min: Vec2::new(15.0, 17.5), max: Vec2::new(40.0, 42.5) };
/// Where a row's name stands, and its lines' pitch [documented: the template's text at
/// 20; observed: lines 35 apart, as the barter menu's rows (barter-menu.md §3)].
const NAME_Y: f32 = 20.0;
const LINE_PITCH: f32 = 35.0;
/// The item's picture [documented: `RSM_ItemIcon`].
const ICON: Rect = Rect { min: Vec2::new(500.0, 0.0), max: Vec2::new(900.0, 400.0) };
/// The figures' corner and their parts [documented: `RSM_StatsDisplayRect` and its
/// children].
const STATS: Vec2 = Vec2::new(485.0, 380.0);
const CARD: Vec2 = Vec2::new(263.0, 60.0);
const STAT_X: f32 = 273.0;
const FIXED_Y: f32 = 140.0;
const ARROW: Rect = Rect { min: Vec2::new(223.0, 50.0), max: Vec2::new(303.0, 130.0) };
const GAIN_Y: f32 = 90.0;
const COST_Y: f32 = 230.0;
const ERROR_AT: Vec2 = Vec2::new(250.0, 160.0);
/// The condition meter in a card [documented: `RSM_BrokenItemHealthMeter` at 85, three
/// above the title's line, 150 by 20].
const METER: Rect = Rect { min: Vec2::new(85.0, 17.0), max: Vec2::new(235.0, 37.0) };
/// The buttons' right end, top and spacing [documented: `RSM_ButtonRect`].
const BUTTONS: Vec2 = Vec2::new(1010.0, 75.0);
const BUTTON_STEP: f32 = 47.0;
/// The sheet behind the menu [documented: `RSM_Background`, solid black at brightness 128
/// and the background fill alpha, 1134 by 910 from (-55, -115)]: faint, as the original's,
/// whose edge cannot be seen [observed: the owner's screenshot, 2026-10-19; the 0.75 of
/// the other menus drew a dark box far past the frame]. The original also blurs the world
/// behind the menu (not built). Ours darkens the whole screen as the character menus do:
/// the owner found the world behind too bright for the text (playtest list 7 item 2,
/// `crate::interface::top_menu_backing`) [design].
/// A row or a figure that cannot be had: dimmer than the rest [observed: the dim rows of
/// the owner's screenshots stand at about two thirds of the bright ones].
const DIM: f32 = 165.0;
/// The bars' empty part [documented: the template's meter background alpha 64].
const BAR_BACK: f32 = 64.0 / 255.0;

/// Fonts by the number the menu file gives them [documented: Fallout_default.ini].
const TITLE_FONT: usize = 6;
const GAIN_FONT: usize = 4;
const ROW_FONT: usize = 2;

pub struct RepairPlugin;

#[derive(SystemSet,Debug,Clone,Copy,PartialEq,Eq,Hash)]
pub(crate) struct NormalInput;

impl Plugin for RepairPlugin {
    fn build(&self, app: &mut App) {
        let mut aid = Vec::new();
        if let Ok(v) = std::env::var("NVF_PLAY_TEST_REPAIR") {
            for part in v.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                match part.split_once(':').and_then(|(t, a)| Some((t.trim().parse::<f32>().ok()?, a.trim().to_string()))) {
                    Some(x) => aid.push(x),
                    None => warn!("NVF_PLAY_TEST_REPAIR: {part:?}: expected <seconds>:<action>"),
                }
            }
            aid.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        app.init_resource::<RepairServices>()
            .add_message::<RepairServicesInput>()
            .insert_resource(RepairAid(aid))
            .add_systems(Update, (scripts_open, (keys.in_set(NormalInput), draw).chain().run_if(in_state(PlayState::Menu))).chain().in_set(PlaySet::Ui))
            .add_systems(OnExit(PlayState::Menu), close);
    }
}

/// `NVF_PLAY_TEST_REPAIR`: (seconds, action), soonest first.
#[derive(Resource, Default)]
struct RepairAid(Vec<(f32, String)>);

/// The repair services menu while it is open.
#[derive(Resource, Default)]
pub struct RepairServices(pub Option<Open>);

impl RepairServices {
    pub(crate) fn normal_same_session(&self,vendor:FormId,selected:usize)->bool {
        self.0.as_ref().is_some_and(|o|o.vendor==vendor && o.selected==selected)
    }
    pub(crate) fn normal_guard(&self)->Option<crate::menu::normal_services::Guard> {
        self.0.as_ref()?.normal_guard()
    }
    pub(crate) fn normal_matches(&self,guard:&crate::menu::normal_services::Guard)->bool {
        let Some(o)=self.0.as_ref() else{return false};
        let crate::menu::normal_services::Guard::Vendor {vendor,view}=guard else{return false};
        *vendor==o.vendor && o.drawn.as_ref().is_some_and(|v|v.normal_matches(view))
    }
    /// [design, #2403 / file refs] Logical repair choices and displayed conditions/costs;
    /// excludes `Open::navigated`, pointer state and drawing labels.
    pub fn progress(&self) -> Option<impl std::fmt::Debug + '_> {
        self.0.as_ref().map(|o| (o.vendor, o.selected, o.first, o.confirming, o.drawn.as_ref().map(|v| (&v.rows, v.skill, v.caps, v.total, v.all_ready))))
    }
    pub fn is_open(&self) -> bool {
        self.0.is_some()
    }
}

pub struct Open {
    vendor: FormId,
    selected: usize,
    /// The list's first row shown: moved only to keep the choice in view, so the pointer
    /// never scrolls the list.
    first: usize,
    drawn: Option<View>,
    /// The selection was last moved by the pointer on the tiles: their list stays where it is.
    pointer_pick: bool,
    /// A count of the keys' and the wheel's moves: the tiles' list brings their row into view
    /// (`crate::tiles::list::settle_list`).
    navigated: u32,
    /// The player's Repair All Confirmation tweak asked once: the next Repair All does it,
    /// anything else takes the question back [design: docs/design/tweaks.md].
    confirming: bool,
}

impl Open {
    pub(crate) fn normal_guard(&self)->Option<crate::menu::normal_services::Guard> {
        Some(crate::menu::normal_services::Guard::Vendor {vendor:self.vendor,view:self.drawn.as_ref()?.clone()})
    }
    pub(crate) fn normal_owner_matches(&self,guard:Option<&crate::menu::normal_services::Guard>)->bool {
        matches!(guard,Some(crate::menu::normal_services::Guard::Vendor {vendor,..}) if *vendor==self.vendor)
    }

    pub(crate) fn touch_view(&self) -> Option<(FormId, &View)> {
        self.drawn.as_ref().map(|view| (self.vendor, view))
    }
    /// What the menu showed last, for the tiles' writer: the view, and [`Open::pointer_pick`]
    /// and [`Open::navigated`].
    pub(crate) fn shown(&self) -> Option<(&View, bool, u32)> {
        self.drawn.as_ref().map(|v| (v, self.pointer_pick, self.navigated))
    }

    /// A key's or the wheel's move.
    fn keyed(&mut self) {
        self.navigated = self.navigated.wrapping_add(1);
        self.pointer_pick = false;
    }

    /// The rows the tiles' input chose, in the order asked, among `rows` rows, up to a repair: it
    /// is of the row chosen when it was asked, the one whose cost was shown.
    fn chosen_on_tiles(&mut self, asked: &[RepairServicesInput], rows: usize) {
        for a in asked {
            match *a {
                RepairServicesInput::Hover(i) | RepairServicesInput::Select(i) if i < rows => {
                    self.selected = i;
                    self.pointer_pick = true;
                }
                RepairServicesInput::Repair => return,
                _ => {}
            }
        }
    }
}

/// What the menu drawn from its tiles asks of it (`crate::tiles::input`), as a click and a press
/// on the hand-built menu do [design: repair.md §3.1].
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepairServicesInput {
    /// Internal once-only request from the ordinary finger adapter.
    CommitNormal(u64),
    /// The pointer came onto a row.
    Hover(usize),
    /// A click on a row not selected.
    Select(usize),
    /// A click on the selected row or the Repair button: the selected row repaired.
    Repair,
    /// Repair All.
    All,
    /// Done.
    Done,
}

/// The sentence for an item beyond the vendor's skill: NVForever's own, the original sets this
/// text in its code [documented: repair.md §3].
pub(crate) const BEYOND_WORDS: &str = "The vendor's skill is too low to repair this.";

#[derive(Component)]
struct RepairRoot;

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum RepairButton {
    Row(usize),
    All,
    Done,
}

/// One unit group of the player's inventory the vendor lists.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RowView {
    pub(crate) item: FormId,
    pub(crate) extra: Extra,
    pub(crate) count: i32,
    /// The item's name, without the count.
    pub(crate) name: String,
    pub(crate) label: String,
    pub(crate) condition: f32,
    pub(crate) quote: Quote,
    /// Repairable now: within the vendor's skill and affordable.
    pub(crate) ready: bool,
    pub(crate) icon: Option<String>,
    /// ("DAM", now, after) or ("DT", now, after).
    pub(crate) stat: Option<(&'static str, i32, i32)>,
    /// Resolved ySI tag/icon/template when ySI is active (`None` otherwise).
    pub(crate) ysi: Option<fnv_game::ui_ysi::YsiItemDisplay>,
}

/// Everything the menu shows: drawn again when it changes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct View {
    pub(crate) rows: Vec<RowView>,
    pub(crate) selected: usize,
    /// The first row shown ([`window`]).
    pub(crate) first: usize,
    pub(crate) skill: i32,
    pub(crate) caps: i32,
    /// What Repair All costs, and whether the player can pay it.
    pub(crate) total: u32,
    pub(crate) all_ready: bool,
    /// Repair All asks to be pressed again (the player's Repair All Confirmation tweak).
    pub(crate) confirming: bool,
    /// The names the hints use (`crate::pad::labels`).
    pub(crate) labels: crate::pad::Labels,
}

impl View {
    pub(crate) fn normal_matches(&self,other:&Self)->bool {
        self.rows==other.rows && self.selected==other.selected && self.skill==other.skill
            && self.caps==other.caps && self.total==other.total && self.all_ready==other.all_ready
            && self.confirming==other.confirming && self.labels==other.labels
    }
}

/// What `open` lists now (with the world in the runtime).
fn view(rt: &mut crate::script::Runtime, sim: &mut SimState, combat: Option<&crate::combat::Combat>, open: &Open) -> View {
    let forms = sim.forms;
    let mut settings = RepairSettings::from_settings(|n| forms.setting_f32(n));
    crate::script::with_world(rt, sim, |rt| {
        let skill = fnv_game::functions::actor_value_capped(&mut rt.game, open.vendor, fnv_game::actor_values::REPAIR) as f32;
        // The player's tweak: the player's Barter takes off part of the price [design: docs/design/tweaks.md].
        let off = rt.game.tweaks.barter_repair_discount;
        if off > 0.0 {
            let barter = fnv_game::functions::actor_value_capped(&mut rt.game, FormId::PLAYER_REF, fnv_game::actor_values::BARTER) as f32;
            settings.cost_mult *= fnv_game::tweaks::barter_repair_factor(off, barter);
        }
        let caps = rt.game.world.item_count(FormId::PLAYER_REF, CAPS).unwrap_or(0);
        let ysi_on = rt.game.ysi_active();
        let groups: Vec<(FormId, Extra, i32)> = rt.game.world.inventory_mut(FormId::PLAYER_REF).map(|i| i.groups().collect()).unwrap_or_default();
        let mut rows = Vec::new();
        for (item, extra, count) in groups {
            let Some(BaseData::Item(it)) = forms.get(item).map(|b| &b.data) else { continue };
            let gear = match it.kind {
                ItemKind::Weapon => Gear::Weapon,
                ItemKind::Armor if !it.non_playable => Gear::Armor,
                _ => continue,
            };
            let condition = extra.condition_or_whole();
            // The player's tweak lists armour with no DT too [design: docs/design/tweaks.md].
            let dt = if rt.game.tweaks.repair_any_armor { Some(it.armor_dt.unwrap_or(1.0).max(1.0)) } else { it.armor_dt };
            if !vendor_lists(gear, it.health, dt, condition) {
                continue;
            }
            let quote = vendor_quote(&settings, it.value.unwrap_or(0), condition, skill);
            let ready = quote.cost().is_some_and(|c| c as i64 <= caps as i64);
            let to = match quote {
                Quote::Repair { to, .. } => to,
                Quote::Beyond => condition,
            };
            let stat = combat.and_then(|c| match gear {
                Gear::Weapon => c.forms.weapons.get(&item).filter(|w| w.base_damage > 0).map(|w| {
                    let at = |k: f32| (w.base_damage as f32 * fnv_combat::damage::condition_mult(k)).round() as i32;
                    ("DAM", at(condition), at(to))
                }),
                Gear::Armor => c.forms.armor.get(&item).map(|a| {
                    let at = |k: f32| (a.dt * fnv_combat::condition::armor_mult(k)).round() as i32;
                    ("DT", at(condition), at(to))
                }),
            });
            let name = crate::menu::model::display_name(forms, item);
            let label = if count > 1 { format!("{name} ({count})") } else { name.clone() };
            let ysi = ysi_on.then(|| rt.game.ysi_item_display(item)).flatten();
            rows.push(RowView { item, extra, count, name, label, condition, quote, ready, icon: it.icon.clone(), stat, ysi });
        }
        // Those that can be repaired first [observed], by name within each part [design].
        rows.sort_by(|a, b| b.ready.cmp(&a.ready).then_with(|| a.label.cmp(&b.label)).then(a.condition.total_cmp(&b.condition)));
        let total: u32 = rows.iter().filter_map(|r| r.quote.cost().map(|c| c * r.count.max(1) as u32)).sum();
        let selected = open.selected.min(rows.len().saturating_sub(1));
        View { all_ready: total > 0 && total as i64 <= caps as i64, rows, selected, first: open.first, skill: skill.max(0.0).floor() as i32, caps, total, confirming: open.confirming, labels: crate::pad::labels() }
    })
}

/// Repair one unit of a row: the caps go from the player to the vendor (back to the
/// player when the vendor is the player) [documented: GECK wiki ShowRepairMenu].
fn repair_one(rt: &mut crate::script::Runtime, vendor: FormId, r: &RowView) -> Result<(f32, u32), String> {
    let Quote::Repair { to, cost } = r.quote else { return Err("beyond the vendor's skill".into()) };
    let caps = rt.game.world.item_count(FormId::PLAYER_REF, CAPS).unwrap_or(0);
    if (cost as i64) > caps as i64 {
        return Err(format!("{cost} caps asked, {caps} carried"));
    }
    let inv = rt.game.world.inventory_mut(FormId::PLAYER_REF).map_err(|e| format!("{e:?}"))?;
    if !inv.set_condition_one(r.item, r.extra, to) {
        return Err("the item is gone".into());
    }
    if cost > 0 && vendor != FormId::PLAYER_REF {
        rt.game.world.remove_item(FormId::PLAYER_REF, CAPS, cost as i32).map_err(|e| format!("{e:?}"))?;
        // A vendor without an inventory keeps nothing [design]. The player's tweak puts
        // the caps in the merchant's container, out of a pickpocket's reach [design: docs/design/tweaks.md].
        let to = if rt.game.tweaks.repair_caps_to_container { rt.game.merchant_container(vendor).unwrap_or(vendor) } else { vendor };
        let _ = rt.game.world.add_item(to, CAPS, cost as i32);
    }
    Ok((to, cost))
}

/// The repair services scripts asked for, opened while playing or over a conversation (a
/// vendor's line's result script, #2308): the menu is a `PlayState::Menu` either way, the
/// conversation waits under it (`crate::dialogue::held`: `Game::menu` is set) and goes on
/// when it closes ([`after_close`]).
#[allow(clippy::too_many_arguments)]
fn scripts_open(
    script: Option<ResMut<ScriptState>>,
    mut sim: ResMut<SimState>,
    combat: Option<Res<crate::combat::Combat>>,
    state: Res<State<PlayState>>,
    mut next: ResMut<NextState<PlayState>>,
    mut menu: ResMut<RepairServices>,
    active: Res<crate::menu::ActiveMenu>,
    mut sounds: MessageWriter<UiSound>,
) {
    let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) else { return };
    let here = matches!(*state.get(), PlayState::Playing | PlayState::Dialogue);
    if rt.game.repair_requests.is_empty() || !here || menu.is_open() || active.0.is_some() {
        return;
    }
    let Some(r) = rt.game.repair_requests.pop() else { return };
    rt.game.repair_requests.clear();
    let open = Open { vendor: r.vendor, selected: 0, first: 0, drawn: None, pointer_pick: false, navigated: 0, confirming: false };
    let v = view(rt, &mut sim, combat.as_deref(), &open);
    let first: Vec<String> = v.rows.iter().take(6).map(|r| format!("{} {}%{}", r.label, (r.condition * 100.0).round(), r.quote.cost().map_or(" beyond".to_string(), |c| format!(" {c} caps")))).collect();
    info!(
        "repair menu: open, vendor {} (Repair {}), player caps {}: {} listed, {} repairable, all {} caps: {}",
        rt.game.data.name(r.vendor),
        v.skill,
        v.caps,
        v.rows.len(),
        v.rows.iter().filter(|r| r.ready).count(),
        v.total,
        first.join(", ")
    );
    menu.0 = Some(open);
    next.set(PlayState::Menu);
    sounds.write(UiSound { menu: UiMenu::Repair, event: UiEvent::Opened });
}

#[allow(clippy::too_many_arguments)]
fn keys(
    keys: crate::pad::ActionButtons<KeyCode>,
    time: AidTime,
    mut aid: ResMut<RepairAid>,
    mut menu: ResMut<RepairServices>,
    script: Option<ResMut<ScriptState>>,
    mut sim: ResMut<SimState>,
    combat: Option<Res<crate::combat::Combat>>,
    buttons: Query<(&PointerState, &RepairButton), Changed<PointerState>>,
    scroll: Option<Res<AccumulatedMouseScroll>>,
    mut next: ResMut<NextState<PlayState>>,
    mut sounds: MessageWriter<UiSound>,
    mut moved: MessageReader<bevy::window::CursorMoved>,
    mut commands: Commands,
    mut on_tiles: MessageReader<RepairServicesInput>,
    (mut touch,mut normal):(crate::maintenance_touch::Requests,crate::menu::normal_services::Commits),
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
) {
    let keys = keys.snapshot();
    // Only a pointer that moves takes the selection: the rows drawn when the menu opens
    // (or redrawn) under a resting pointer leave the keys' choice alone (as `crate::menu`).
    let pointer_moved = moved.read().count() > 0;
    // What the tiles asked: read whatever else happens, so none of it waits for a later frame.
    let asked: Vec<RepairServicesInput> = on_tiles.read().copied().collect();
    let normal_tokens:Vec<_>=asked.iter().filter_map(|a|if let RepairServicesInput::CommitNormal(t)=a {Some(*t)}else{None}).collect();
    let mobile = touch.take(crate::maintenance_touch::Kind::Vendor);
    let Some(open) = menu.0.as_mut() else { return };
    let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) else { return };
    // A world aid (`crate::pause::WorldAidClock`).
    let now = time.world_secs();
    let mut acts = Vec::new();
    while aid.0.first().is_some_and(|(t, _)| now.is_some_and(|now| *t <= now)) {
        let (_, a) = aid.0.remove(0);
        info!("test aid NVF_PLAY_TEST_REPAIR: {a}");
        acts.push(a);
    }
    let act = |n: &str| acts.iter().any(|a| a == n);
    if act("shot") {
        info!("repair menu: screenshot repair_menu.png");
        commands.spawn(bevy::render::view::screenshot::Screenshot::primary_window()).observe(bevy::render::view::screenshot::save_to_disk("repair_menu.png"));
    }
    let mut v = open.drawn.clone().unwrap_or_else(|| view(rt, &mut sim, combat.as_deref(), open));
    let n = v.rows.len();
    let before = open.selected;
    let mut clicked = None;
    for (i, b) in &buttons {
        match (i, b) {
            (PointerState::Pressed, b) => clicked = Some(*b),
            (PointerState::Hovered, RepairButton::Row(r)) if pointer_moved => open.selected = *r,
            _ => {}
        }
    }
    let wheel = scroll.as_ref().map_or(0.0, |s| s.delta.y);
    if keys.any_just_pressed([KeyCode::ArrowUp, KeyCode::KeyW]) || wheel > 0.0 || act("up") {
        open.selected = open.selected.min(n.saturating_sub(1)).saturating_sub(1);
        open.keyed();
    }
    if keys.any_just_pressed([KeyCode::ArrowDown, KeyCode::KeyS]) || wheel < 0.0 || act("down") {
        open.selected = (open.selected + 1).min(n.saturating_sub(1));
        open.keyed();
    }
    if let Some(RepairButton::Row(r)) = clicked {
        open.selected = r;
    }
    // After the keys: a click acts on the row it was on.
    open.chosen_on_tiles(&asked, n);
    let tile_asked = |a: RepairServicesInput| asked.contains(&a);
    let mut touch_one = false;
    let mut touch_done = false;
    let mut normal_all=false;
    // Existing physical/native selection has already run. Validate each mobile action
    // against the actual current quote/unit/caps and current selection, not the last
    // rendered vendor list. Only one confirmation may act in a batch.
    for request in mobile {
        if !crate::maintenance_touch::menu_allows(crate::maintenance_touch::Kind::Vendor, Some(&rt.game)) { continue; }
        let live = view(rt, &mut sim, combat.as_deref(), open);
        if !request.vendor(open.vendor, &live) { open.drawn = None; continue; }
        use crate::maintenance_touch::Action;
        match request.action {
            Action::Select(i) if i < live.rows.len() => { open.selected = i; open.pointer_pick = true; open.drawn = None; }
            Action::Scroll(by) => { open.first = crate::maintenance_touch::scroll(open.first, by, live.rows.len()); open.drawn = None; }
            Action::Confirm if !touch_one => {
                touch_one = live.rows.get(open.selected).is_some_and(|r| r.ready);
                v = live;
                if touch_one { break; }
            }
            Action::Cancel => { touch_done = true; break; }
            _ => {}
        }
    }
    // Resolve normal-menu touch only after existing physical/native selection.
    // The actual current view/quote supplies eligibility; original repair_one
    // and RepairAll below remain the only effect paths.
    for token in normal_tokens {
        let live=view(rt,&mut sim,combat.as_deref(),open);
        let current=crate::menu::normal_services::Guard::Vendor {vendor:open.vendor,view:live};
        let action=normal.resolve(token,&current,&rt.game,touch.enabled());
        let crate::menu::normal_services::Guard::Vendor {view:live,..}=current else{continue};
        use crate::menu::normal_services::Action;
        match action {
            Some(Action::Vendor(RepairServicesInput::Select(i)|RepairServicesInput::Hover(i))) if i<live.rows.len()=>{
                open.selected=i;open.pointer_pick=true;open.drawn=None;
            }
            Some(Action::Vendor(RepairServicesInput::Repair))=>{
                touch_one=live.rows.get(open.selected).is_some_and(|r|r.ready);v=live;
                if touch_one {break}
            }
            Some(Action::Vendor(RepairServicesInput::All))=>{normal_all=true;v=live;break}
            Some(Action::Vendor(RepairServicesInput::Done))=>{touch_done=true;break}
            Some(Action::Browse(0,by))=>{
                open.first=(open.first as i64).saturating_add(by).clamp(0,live.rows.len().saturating_sub(1) as i64) as usize;
                open.pointer_pick=true;open.drawn=None;
            }
            _=>{},
        }
    }
    if open.selected != before {
        sounds.write(UiSound { menu: UiMenu::Repair, event: UiEvent::Focus });
        if std::mem::take(&mut open.confirming) {
            open.drawn = None;
        }
    }
    if keys.any_just_pressed([KeyCode::KeyE, KeyCode::Escape, KeyCode::Tab]) || clicked == Some(RepairButton::Done) || act("done") || tile_asked(RepairServicesInput::Done) || touch_done {
        info!("repair menu: done");
        sounds.write(UiSound { menu: UiMenu::Repair, event: UiEvent::Back });
        next.set(after_close(&rt.game));
        return;
    }
    let forms = sim.forms;
    let vendor = open.vendor;
    let one = keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter]) || matches!(clicked, Some(RepairButton::Row(_))) || act("repair") || tile_asked(RepairServicesInput::Repair) || touch_one;
    let all = keys.just_pressed(KeyCode::KeyA) || clicked == Some(RepairButton::All) || act("all") || tile_asked(RepairServicesInput::All) || normal_all;
    // The player's Repair All Confirmation tweak: a Repair All at or over its price asks
    // first; any other repair takes the question back [design: docs/design/tweaks.md].
    let asks = tweaks.as_deref().filter(|t| t.repair_all_confirm).is_some_and(|t| v.total as f32 >= t.repair_all_threshold);
    if one && std::mem::take(&mut open.confirming) {
        open.drawn = None;
    }
    if all && asks && v.all_ready && !std::mem::take(&mut open.confirming) {
        info!("repair menu: Repair All for {} caps asks to be pressed again (Repair All Confirmation)", v.total);
        open.confirming = true;
        open.drawn = None;
        sounds.write(UiSound { menu: UiMenu::Repair, event: UiEvent::Focus });
        return;
    }
    // An idle frame between deliberate presses is not another repair intent.
    // Selection, close and a single repair retain their existing cancellation paths.
    if one || all {open.confirming = false;}
    if one {
        let Some(r) = v.rows.get(open.selected.min(n.saturating_sub(1))) else { return };
        let name = crate::menu::model::display_name(forms, r.item);
        match crate::script::with_world(rt, &mut sim, |rt| repair_one(rt, vendor, r)) {
            Ok((to, cost)) => {
                info!("repair menu: repaired {name} {:.0}% -> {:.0}% for {cost} caps", r.condition * 100.0, to * 100.0);
                sounds.write(UiSound { menu: UiMenu::Repair, event: UiEvent::Picked });
            }
            Err(why) => {
                info!("repair menu: cannot repair {name}: {why}");
                sounds.write(UiSound { menu: UiMenu::Repair, event: UiEvent::Refused });
            }
        }
        open.drawn = None;
    } else if all {
        if !v.all_ready {
            info!("repair menu: cannot repair all: {} caps asked, {} carried", v.total, v.caps);
            sounds.write(UiSound { menu: UiMenu::Repair, event: UiEvent::Refused });
            return;
        }
        let mut done = 0;
        let mut paid = 0;
        for r in v.rows.iter().filter(|r| r.ready) {
            for _ in 0..r.count.max(1) {
                if let Ok((_, cost)) = crate::script::with_world(rt, &mut sim, |rt| repair_one(rt, vendor, r)) {
                    done += 1;
                    paid += cost;
                }
            }
        }
        info!("repair menu: repaired all ({done} items) for {paid} caps");
        sounds.write(UiSound { menu: UiMenu::Repair, event: UiEvent::Picked });
        open.drawn = None;
    }
}

/// The menu's fonts and pictures, read once.
#[derive(Resource)]
struct RepairLook {
    title: Face,
    gain: Face,
    row: Face,
    pictures: Pictures,
}

/// A line of text whose top is at `top` (as the menu files place texts).
fn text(p: &mut ChildSpawnerCommands, f: &Face, s: &str, x: f32, top: f32, c: Color) {
    if let Some(line) = f.font.lines(s, None).into_iter().next() {
        crate::menu::readability::line(p,f,&line,Vec2::new(x,top+crate::pipboy::frame::base(&f.font)*f.scale),f.scale,c);
    }
}

fn right(p: &mut ChildSpawnerCommands, f: &Face, s: &str, x: f32, top: f32, c: Color) {
    text(p, f, s, x - f.width(s), top, c);
}

fn centred(p: &mut ChildSpawnerCommands, f: &Face, s: &str, x: f32, top: f32, c: Color) {
    text(p, f, s, x - f.width(s) / 2.0, top, c);
}

fn wrapped(p: &mut ChildSpawnerCommands, f: &Face, s: &str, x: f32, top: f32, width: f32, c: Color) {
    for (i, line) in f.font.lines(s, Some(width/f.scale)).iter().enumerate() {
        crate::menu::readability::line(p,f,line,Vec2::new(x,top+(crate::pipboy::frame::base(&f.font)+LINE_PITCH*i as f32)*f.scale),f.scale,c);
    }
}

/// Width reserved for a ySI row icon when ySI is active (0 in vanilla).
const YSI_ICON_ADVANCE: f32 = 30.0;

/// A row's height: its name's lines and the padding (the barter menu's rule).
fn row_height(f: &Face, label: &str, wrap: f32) -> f32 {
    (crate::menu::look::row_height(f.font.lines(label, Some(wrap/f.scale)).len().max(1))*f.scale).max(crate::menu::look::row_height(1))
}

fn at(x: f32, y: f32, w: f32, h: f32) -> Node {
    Node { position_type: PositionType::Absolute, left: u(x), top: u(y), width: u(w), height: u(h), ..default() }
}

fn rule(p: &mut ChildSpawnerCommands, x: f32, y: f32, w: f32, h: f32) {
    p.spawn((at(x, y, w, h), BackgroundColor(amber(200.0))));
}

/// A filled bar of `fill` (0 to 1) over its dim background.
fn bar(p: &mut ChildSpawnerCommands, r: Rect, fill: f32, c: Color) {
    p.spawn((at(r.min.x, r.min.y, r.width(), r.height()), BackgroundColor(c.with_alpha(BAR_BACK))));
    p.spawn((at(r.min.x, r.min.y, r.width() * fill.clamp(0.0, 1.0), r.height()), BackgroundColor(c)));
}

/// A card as `card_info.xml`: a line along its top, a short side fading down at its
/// right, the title at 5 and the value right-aligned 10 before the end, 20 down.
fn card(p: &mut ChildSpawnerCommands, f: &Face, x: f32, y: f32, title: &str, value: &str, c: Color) {
    rule(p, x, y, CARD.x, LINE);
    rule(p, x + CARD.x - LINE, y, LINE, 40.0);
    text(p, f, title, x + 5.0, y + 8.0, c);
    if !value.is_empty() {
        right(p, f, value, x + CARD.x - 10.0, y + 8.0, c);
    }
}

/// Which rows show: from the first shown, moved as little as keeps the chosen one in
/// view, as many as fit the list's height.
fn window(look: &RepairLook, v: &View, follow: bool, wrap: f32) -> std::ops::Range<usize> {
    let heights: Vec<f32> = v.rows.iter().map(|r| row_height(&look.row, &r.label, wrap)).collect();
    let mut first = if follow {v.first.min(v.selected)}else{v.first}.min(heights.len());
    loop {
        let mut used = 0.0;
        let mut end = first;
        while end < heights.len() && used + heights[end] <= LIST_HEIGHT {
            used += heights[end];
            end += 1;
        }
        if !follow || v.selected < end || end >= heights.len() || first >= v.selected {
            return first..end.max(first + 1).min(heights.len());
        }
        first += 1;
    }
}

fn spawn(commands: &mut Commands, look: &mut RepairLook, presentation:(&View,crate::menu::normal_services::Guard), data: Option<&GameData>, images: &mut Assets<Image>, bc: bool, glyphs: &crate::pad::PadGlyphs) {
    let (v,guard)=presentation;
    let node = Node { position_type: PositionType::Absolute, left: Val::Percent(50.0), margin: UiRect::left(u(-MAIN.x / 2.0)), top: crate::interface::from_top((960.0 - MAIN.y) / 2.0, 0.5), width: u(MAIN.x), height: u(MAIN.y), ..default() };
    let bright = color(Shade::Bright);
    let dim = amber(DIM);
    let marker = look.pictures.picture("interface\\shared\\marker\\square_filled.dds", data, images, bc);
    let arrow = look.pictures.picture("interface\\shared\\arrow\\repair_arrow_down.dds", data, images, bc);
    let chosen = v.rows.get(v.selected);
    let icon = chosen.and_then(|r| r.icon.as_deref()).and_then(|i| look.pictures.picture(i, data, images, bc));
    let has_ysi = v.rows.iter().any(|r| r.ysi.is_some());
    let icon_advance = if has_ysi { YSI_ICON_ADVANCE } else { 0.0 };
    let name_x = NAME_X + icon_advance;
    let name_wrap = NAME_WRAP - icon_advance;
    let visible = window(look, v, false, name_wrap);
    let row_icons: Vec<Option<crate::interface::Picture>> = visible
        .clone()
        .map(|i| {
            v.rows[i]
                .ysi
                .as_ref()
                .and_then(|d| d.icon.as_deref())
                .and_then(|p| look.pictures.picture(p, data, images, bc))
        })
        .collect();
    commands.spawn((RepairRoot, crate::menu::normal_services::Rendered::new(guard,look.row.scale), Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, GlobalZIndex(crate::interface::TOP_MENU_Z), crate::interface::top_menu_backing())).with_children(|root| {
        root.spawn(node).with_children(|m| {
            // The headline: the title, the vendor's skill and the player's caps, each part
            // closed by a short side fading down [documented]. The words here and on the
            // buttons are the settings the menu files name (`sRepairServicesTitle`,
            // `sRepairSkill`, `sInventoryCaps`, `sRepairAllItems`, `sDone`): the load order's
            // text, else NVForever's own label [design, #4495: in place of the words read
            // from the original's screen].
            let word = crate::pipboy::labels::built_in_label;
            let title = word("sRepairServicesTitle");
            let tw = look.title.width(title);
            let caps = format!("{}  {}", word("sInventoryCaps"), v.caps);
            let v2 = (1010.0 - look.row.width(&caps) - 35.0).max(860.0);
            rule(m, 0.0, 0.0, 50.0, LINE);
            rule(m, tw + 90.0, 0.0, (v2 - tw - 90.0).max(0.0), LINE);
            rule(m, v2 + 10.0, 0.0, (MAIN.x - v2 - 10.0).max(0.0), LINE);
            for x in [0.0, v2, MAIN.x - LINE] {
                rule(m, x, 0.0, LINE, 40.0);
            }
            text(m, &look.title, title, 70.0, -13.0, bright);
            right(m, &look.row, &format!("{}  {}", word("sRepairSkill"), v.skill), v2 - 15.0, 20.0, bright);
            right(m, &look.row, &caps, 1005.0, 20.0, bright);
            // The list.
            m.spawn(Node {overflow:Overflow::clip(),..at(ROW_X,LIST_TOP,ROW_WIDTH+30.0,LIST_HEIGHT)}).with_children(|list| {
            let mut y = 0.0;
            for (slot, i) in visible.enumerate() {
                let r = &v.rows[i];
                let h = row_height(&look.row, &r.label, name_wrap);
                let (line, fill) = highlight_colors();
                let on = i == v.selected;
                let c = if r.ready { bright } else { dim };
                let ysi_pic = &row_icons[slot];
                let n = Node { border: UiRect::all(u(LINE)), ..at(0.0, y, ROW_WIDTH + 30.0, h) };
                list.spawn((Button, RepairButton::Row(i), n, BorderColor::all(if on { line } else { Color::NONE }), BackgroundColor(if on { fill } else { Color::NONE }))).with_children(|row| {
                    if let Some((img, rect)) = ysi_pic {
                        row.spawn((at(NAME_X, 15.0*look.row.scale, 24.0, 24.0), ImageNode { rect: *rect, color: c, image_mode: NodeImageMode::Stretch, ..ImageNode::new(img.clone()) }));
                    }
                    wrapped(row, &look.row, &r.label, name_x, NAME_Y*look.row.scale, name_wrap, c);
                    bar(row, BAR, r.condition, c);
                    if r.extra.equipped {
                        let node = at(MARKER.min.x, MARKER.min.y, MARKER.width(), MARKER.height());
                        match &marker {
                            Some((img, rect)) => {
                                row.spawn((node, ImageNode { rect: *rect, color: c, image_mode: NodeImageMode::Stretch, ..ImageNode::new(img.clone()) }));
                            }
                            None => {
                                row.spawn((node, BackgroundColor(c)));
                            }
                        }
                    }
                });
                y += h;
            }
            });
            if v.rows.is_empty() {
                // [observed: none; the words are ours, the original's `sNoItemsToRepair` is
                // not in the masters].
                text(m, &look.row, "No items to repair.", 45.0, 75.0, bright);
            }
            // The chosen item: its picture and figures.
            if let Some(r) = chosen {
                if let Some((img, rect)) = &icon {
                    m.spawn((at(ICON.min.x, ICON.min.y, ICON.width(), ICON.height()), ImageNode { rect: *rect, color: bright, image_mode: NodeImageMode::Stretch, ..ImageNode::new(img.clone()) }));
                }
                let (sx, sy) = (STATS.x, STATS.y);
                let to = match r.quote {
                    Quote::Repair { to, .. } => to,
                    Quote::Beyond => r.condition,
                };
                let stat = |k: usize| r.stat.map_or(String::new(), |s| if k == 0 { s.1 } else { s.2 }.to_string());
                let stat_title = r.stat.map_or("", |s| s.0);
                for (k, (y, cnd)) in [(0.0, r.condition), (FIXED_Y, to)].into_iter().enumerate() {
                    if k == 1 && r.quote == Quote::Beyond {
                        break;
                    }
                    card(m, &look.row, sx, sy + y, "CND", "", bright);
                    bar(m, Rect::from_corners(Vec2::new(sx, sy + y) + METER.min, Vec2::new(sx, sy + y) + METER.max), cnd, bright);
                    card(m, &look.row, sx + STAT_X, sy + y, stat_title, &stat(k), bright);
                }
                match r.quote {
                    Quote::Repair { to, cost } => {
                        if let Some((img, rect)) = &arrow {
                            m.spawn((at(sx + ARROW.min.x, sy + ARROW.min.y, ARROW.width(), ARROW.height()), ImageNode { rect: *rect, color: bright, image_mode: NodeImageMode::Stretch, ..ImageNode::new(img.clone()) }));
                        }
                        let g = crate::pipboy::frame::base(&look.gain.font);
                        centred(m, &look.gain, &format!("+{}%", gain_percent(r.condition, to)), sx + 120.0, sy + GAIN_Y - g, bright);
                        let gain = r.stat.map_or(0, |s| s.2 - s.1);
                        centred(m, &look.gain, &format!("+{gain}"), sx + 400.0, sy + GAIN_Y - g, bright);
                        let each = if r.count > 1 { " each" } else { "" };
                        text(m, &look.title, &format!("Cost: {cost} caps{each}"), sx, sy + COST_Y, if r.ready { bright } else { dim });
                    }
                    Quote::Beyond => {
                        // [the words are ours: the original sets this text in its code].
                        centred(m, &look.row, BEYOND_WORDS, sx + ERROR_AT.x, sy + ERROR_AT.y, dim);
                    }
                }
            }
            // The buttons: the word, the total in brackets, the key hint [observed: the
            // order and the total; the words are the settings above].
            // A and E, with a controller X (`repair_services_menu.xml`'s Repair All) and B
            // (Esc) [#1662].
            let hint_at = crate::pad::at_menu(fnv_game::repair::REPAIR_SERVICES_MENU);
            // The player's Repair All Confirmation tweak: asking, the button says so [design:
            // our own words].
            let all = if v.confirming {
                format!("{} ({})? Again to confirm  {}", word("sRepairAllItems"), v.total, crate::pad::hint_as(&[KeyCode::KeyA], hint_at, v.labels))
            } else {
                format!("{} ({})  {}", word("sRepairAllItems"), v.total, crate::pad::hint_as(&[KeyCode::KeyA], hint_at, v.labels))
            };
            let done = format!("{}  {}", word("sDone"), crate::pad::hint_as(&[KeyCode::KeyE, KeyCode::Escape], hint_at, v.labels));
            let button_h = (48.0 * look.row.scale).max(48.0);
            for (k, (label, b, on)) in [(all.as_str(), RepairButton::All, v.all_ready), (done.as_str(), RepairButton::Done, true)].into_iter().enumerate() {
                let w = crate::pad::hint_width(&look.row, label, glyphs);
                m.spawn((Button, b, at(BUTTONS.x - w - 16.0, BUTTONS.y + k as f32 * BUTTON_STEP, w + 24.0, button_h))).with_children(|t| {
                    crate::pad::set_hint(t, &look.row, label, 6.0, crate::pipboy::frame::base(&look.row.font), if on { bright } else { dim }, glyphs);
                });
            }
            // The bottom bracket: a line along 680 with short sides fading up.
            rule(m, 0.0, MAIN.y, MAIN.x, LINE);
            rule(m, 0.0, MAIN.y - 40.0, LINE, 40.0);
            rule(m, MAIN.x - LINE, MAIN.y - 40.0, LINE, 40.0);
        });
    });
}

/// Draw the menu again when what it shows changed.
#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    mut menu: ResMut<RepairServices>,
    script: Option<ResMut<ScriptState>>,
    mut sim: ResMut<SimState>,
    combat: Option<Res<crate::combat::Combat>>,
    roots: Query<Entity, With<RepairRoot>>,
    look: Option<ResMut<RepairLook>>,
    data: Option<Res<GameData>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    glyphs: Res<crate::pad::PadGlyphs>,
    (switch, mut was_tiled): (Option<Res<crate::tiles::vanilla::TileMenus>>, Local<bool>),
    touch: Option<Res<crate::maintenance_touch::State>>,
    readability:Option<Res<crate::menu_readability::MenuReadability>>,
) {
    let Some(open) = menu.0.as_mut() else { return };
    if touch.as_ref().is_some_and(|s| s.visible()) {
        if let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) {
            // The alternate list owns its actual first row. Legacy window() keeps the
            // selected row visible and would otherwise undo an intentional touch scroll.
            open.drawn = Some(view(rt, &mut sim, combat.as_deref(), open));
        }
        return;
    }
    // The one decision with the tiles' writer, whose drawing replaces this one.
    let tiled = switch.is_some_and(|s| s.repair_services) && script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| crate::tiles::repair_services_menu::tiled(true, &rt.game, true));
    if tiled {
        let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) else { return };
        let v = view(rt, &mut sim, combat.as_deref(), open);
        if !*was_tiled || roots.is_empty() {
            for e in &roots {
                commands.entity(e).despawn();
            }
            commands.spawn((RepairRoot, Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, GlobalZIndex(crate::interface::TOP_MENU_Z))).with_children(|root| {
                root.spawn(crate::tiles::menus::modal_sheet());
            });
        }
        *was_tiled = true;
        open.selected = v.selected;
        open.drawn = Some(v);
        return;
    }
    let bc = bc_supported(formats.as_deref());
    let Some(mut look) = look else {
        let font = |n: usize, images: &mut Assets<Image>| face(crate::pipboy::frame::FONTS[n - 1], plain_font(30.0, 14.0), data.as_deref(), images, bc);
        commands.insert_resource(RepairLook { title: font(TITLE_FONT, &mut images).into(), gain: font(GAIN_FONT, &mut images).into(), row: font(ROW_FONT, &mut images).into(), pictures: Pictures::load(data.as_deref()) });
        return;
    };
    let text_size=crate::menu::readability::size(readability.as_deref(),crate::menu_readability::MenuKind::Repair);
    let resized=look.row.scale!=text_size;
    if resized {look.title.scale=text_size;look.gain.scale=text_size;look.row.scale=text_size;}
    let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) else { return };
    // Back from the tiles (the file gone, its tree dropped): the hand-built menu is drawn anew.
    let back = std::mem::take(&mut *was_tiled);
    let mut v = view(rt, &mut sim, combat.as_deref(), open);
    let wrap = NAME_WRAP - if v.rows.iter().any(|r| r.ysi.is_some()) { YSI_ICON_ADVANCE } else { 0.0 };
    v.first = window(&look, &v, !open.pointer_pick, wrap).start;
    open.first = v.first;
    if open.drawn.as_ref() == Some(&v) && !roots.is_empty() && !back && !resized {
        return;
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
    spawn(&mut commands, &mut look, (&v,crate::menu::normal_services::Guard::Vendor {vendor:open.vendor,view:v.clone()}), data.as_deref(), &mut images, bc, &glyphs);
    open.selected = v.selected;
    open.drawn = Some(v);
}

/// The state a closed menu goes back to: the conversation it was opened over, if it still
/// runs, else play. [hypothesis: the original returns to the topic list after a repair
/// line without the Goodbye flag, as after barter; not captured]
pub(crate) fn after_close(g: &fnv_game::Game) -> PlayState {
    if g.dialogue.is_some() {
        PlayState::Dialogue
    } else {
        PlayState::Playing
    }
}

fn close(mut commands: Commands, mut menu: ResMut<RepairServices>, roots: Query<Entity, With<RepairRoot>>, script: Option<ResMut<ScriptState>>) {
    if menu.0.take().is_none() {
        return;
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
    if let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) {
        if rt.game.menu == Some(REPAIR_SERVICES_MENU) {
            rt.game.close_menu();
        }
    }
    info!("repair menu: closed");
}

/// [design, #2403] Synthetic logical/ineffective states for detector regressions.
#[cfg(feature = "report-test-support")]
#[doc(hidden)]
pub fn report_progress_fixture(step: usize) -> RepairServices {
    let mut o = Open { vendor: FormId(42), selected: 0, first: 0, drawn: None, pointer_pick: step >= 5, navigated: step as u32, confirming: false };
    if step >= 1 {
        o.selected = 1;
    }
    if step >= 2 {
        o.first = 1;
    }
    if step >= 3 {
        o.confirming = true;
    }
    o.drawn = Some(View {
        rows: (0..20)
            .map(|i| RowView {
                item: FormId(100 + i),
                extra: Extra::default(),
                count: 1,
                name: format!("Synthetic item {i}"),
                label: format!("Synthetic item {i}"),
                condition: 0.5,
                quote: Quote::Repair { to: 1.0, cost: 10 },
                ready: true,
                icon: None,
                stat: None,
                ysi: None,
            })
            .collect(),
        selected: o.selected,
        first: o.first,
        skill: 50,
        caps: if step >= 4 { 50 } else { 100 },
        total: 20,
        all_ready: true,
        confirming: o.confirming,
        labels: Default::default(),
    });
    RepairServices(Some(o))
}

#[cfg(test)]
mod maintenance_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> Open {
        Open { vendor: FormId(1), selected: 0, first: 0, drawn: None, pointer_pick: false, navigated: 0, confirming: false }
    }

    #[test]
    fn the_tiles_choose_rows_there_are_and_the_keys_take_it_back() {
        let mut o = open();
        o.chosen_on_tiles(&[RepairServicesInput::Hover(2)], 3);
        assert_eq!((o.selected, o.pointer_pick), (2, true));
        o.chosen_on_tiles(&[RepairServicesInput::Select(1), RepairServicesInput::Repair], 3);
        assert_eq!(o.selected, 1);
        o.chosen_on_tiles(&[RepairServicesInput::Select(3)], 3);
        assert_eq!(o.selected, 1, "a row no longer there chooses nothing");
        o.keyed();
        assert_eq!((o.pointer_pick, o.navigated), (false, 1));
    }

    #[test]
    fn a_repair_is_of_the_row_chosen_when_it_was_asked() {
        let mut o = open();
        o.chosen_on_tiles(&[RepairServicesInput::Select(1), RepairServicesInput::Repair, RepairServicesInput::Hover(2)], 3);
        assert_eq!(o.selected, 1, "a pointer step after it in the frame moves nothing");
    }

    #[test]
    fn asking_for_nothing_changes_nothing() {
        let mut o = open();
        o.chosen_on_tiles(&[RepairServicesInput::All, RepairServicesInput::Done], 3);
        assert_eq!((o.selected, o.pointer_pick, o.navigated), (0, false, 0));
    }
}
