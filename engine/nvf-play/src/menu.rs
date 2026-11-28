//! Part U (docs/design/layer4-play.md §4): container, barter and inventory menus.
//!
//! [`model`] is the menu as plain data over the simulation (rows, selection, commands
//! moving items through `fnv_sim::World`), unit-tested without Bevy; [`barter`] the
//! prices. [`look`] draws the container and barter menus as the original does
//! (docs/spec-notes/barter-menu.md); the inventory is the Pip-Boy's ITEMS page
//! (`crate::pipboy`), which draws it and takes its keys. This file opens and closes the
//! menus and turns the container and barter menus' keys and clicks into
//! [`model::Command`]s.
//!
//! - `OpenContainer { container }` (Part A, or the F10 test aid) opens the container menu:
//!   player | container, in `PlayState::Menu`.
//! - [`OpenBarter`] opens the barter menu: player | merchant. Scripts ask for it with
//!   `ShowBarterMenu` (`fnv_game::game::Game::barter_requests`), mostly from a dialogue
//!   choice's result script; then it opens over the conversation, which waits under it
//!   (`crate::dialogue::held`) and goes on when it closes [hypothesis: the original
//!   returns to the conversation; its BarterExit lines are not said yet].
//! - Tab tapped while playing opens the player's inventory alone, when it is let go (a
//!   hold switches the flashlight, `crate::flashlight`), and with it the Pip-Boy
//!   (`crate::pipboy`): equip/unequip (aid items are used), drop.
//! - Keys of the container menu: Up/Down (or W/S) select, Left/Right (or A/D) switch
//!   list, Enter/Space take or store one, Shift+Enter/Space the whole stack, R take all, E,
//!   Esc or Tab close [observed: the screenshot's "Take All R)" and "Exit E)"]. Barter:
//!   the same, but Left/Right only, A accepts and X exits [observed: "Accept A)", "Exit
//!   X)"]; Esc and Tab exit too. In both, `[` and `]` change the focused list's category
//!   [design]; with a controller LT and RT pick the list (the arrow keys) and the D-pad's
//!   left and right change its category (`[` and `]`), the hints "LT)" and "RT)" under
//!   the lists [observed: the original, #1827; `crate::pad::menu_buttons`], the pointer selects the row it is on and a click acts on it, Shift+click
//!   on the whole stack; the title arrows and the buttons are clickable. The inventory's
//!   keys are the Pip-Boy's (`crate::pipboy::items`). [hypothesis:
//!   the original's PC keys beyond the hints shown are not captured]
//! - The cursor is shown and released on entering a menu; Part P grabs it again when
//!   play resumes.
//! - The game clock stops while a menu is open (the simulation's clock runs only in
//!   `PlayState::Playing`, sim.rs). [hypothesis: not captured for the container and
//!   inventory menus]
//! - Dropping creates references in the simulation (`World::drop_item`); the menu sends
//!   `RefChanged` for each, and `crate::dropped` and `crate::phys` give them a model and
//!   collision. Each drop is logged.
//! - Transfers between inventories do not change any reference's presence, so they send
//!   no `RefChanged`.
//! - "How many?" (`model::Quantity`, drawn by `look::spawn_quantity` over any of these
//!   menus, the Pip-Boy's inventory included): Left/Right (and the wheel) change the
//!   amount by one, Page Up/Page Down by ten, Home/End go to 1 and the whole stack; A,
//!   Enter or Space is Ok, E or Esc Cancel [documented: the interface file's "A)" and
//!   "E)"; the other keys are ours]. The meter takes a press and a drag, its arrows and
//!   the two buttons a click. While it is open no other key of the menus or the Pip-Boy
//!   acts ([`PromptKeys`]).
//!
//! Test aids (only with `--play`, since this plugin exists only then):
//! - F10 while playing, with the developer keys on (`NVF_PLAY_DEV_KEYS`, or a test aid's
//!   `NVF_PLAY_TEST_KEYS`; [`DevKeys`]), opens the container menu on the container
//!   reference nearest to the camera (in the current interior, or the worldspace), logging
//!   which one.
//! - `NVF_PLAY_TEST_MENU=<container FormID hex | nearest>` runs a scripted check (see
//!   [`test_aid`]).
//! - `NVF_PLAY_TEST_BARTER=<merchant FormID hex>[:discount]` opens the barter menu and
//!   makes one trade (see [`test_aid`]).
//! - `NVF_PLAY_TEST_DROP=<seconds>:<item FormID hex>[:stack|:use|:ask|:<count>][,...]`
//!   drops one unit (the stack, or that many through "How many?") of a carried item through
//!   the inventory menu at those times (see [`test_aid`]).

pub mod barter;
pub mod barter_touch;
pub mod normal_touch;
pub(crate) mod readability;
pub mod normal_services;
pub(crate) mod look;
pub mod model;
mod test_aid;

use crate::sim::SimState;
use crate::{space, ItemUsed, Location, OpenContainer, PlaySet, PlayState, PlayerCamera, RefChanged};
use bevy::image::CompressedImageFormatSupport;
use bevy::ecs::system::SystemParam;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use crate::ui_pointer::PointerState;
use bevy::ui::{RelativeCursorPosition,UiGlobalTransform};
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use fnv_sim::forms::BaseData;
use fnv_sim::FormId;
use model::{Command, MenuModel, Mode, Side};
use nvf_assets::GameData;

/// The open menu, if any: `Some` while in `PlayState::Menu`, and while a barter menu is
/// open over a conversation (`PlayState::Dialogue`).
#[derive(Resource, Default)]
pub struct ActiveMenu(pub Option<MenuModel>);

/// Open the barter menu with this merchant (an actor reference); `discount` as
/// `ShowBarterMenu`'s.
#[derive(Message, Clone, Copy, Debug)]
pub struct OpenBarter {
    pub merchant: u32,
    pub discount: i32,
}

/// The teammates' inventories scripts asked for (OpenTeammateContainer,
/// `fnv_game::followers`), as [`OpenContainer`] once no conversation holds the game: the
/// calls are in result scripts of companions' lines [design: the container menu opens
/// only while playing, so it waits for the conversation to end].
fn scripts_open_teammate_containers(state: Res<State<PlayState>>, script: Option<ResMut<crate::script::ScriptState>>, mut out: MessageWriter<OpenContainer>) {
    if *state.get() != PlayState::Playing {
        return;
    }
    let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) else { return };
    for r in std::mem::take(&mut rt.game.teammate_containers) {
        info!("menu: a script opens {}'s inventory (OpenTeammateContainer)", rt.game.data.name(r));
        out.write(OpenContainer { container: r.0 });
    }
}

/// The player's Barter skill when the script runtime cannot say (no scripts): every
/// skill of a new character [observed: actor-values.md §7, the player's skills all 15].
const DEFAULT_BARTER_SKILL: f32 = 15.0;

/// The line under the container menu for a container that refills [observed: the
/// owner's container screenshot].
const RESPAWN_NOTE: &str = "This container respawns and is unsafe to store your items.";

/// Set when a menu opened this frame: the key that opened it (E on a container) is still
/// "just pressed" when the menu's keys are read later in the same frame, and E also
/// closes the container menu, so the menu's keys wait for the next frame [design: the
/// fix of the playtest's "cannot open containers", 2026-10-11].
#[derive(Resource, Default)]
pub struct FreshMenu(pub bool);

/// A command for the open menu (keys, mouse, test aid).
#[derive(Message, Clone, Copy, Debug)]
pub struct MenuInput(pub Command);

/// Open the player's inventory (Tab tapped, `crate::flashlight`; or the test aid).
#[derive(Message, Clone, Copy, Debug, Default)]
pub struct OpenInventory;

/// The menus' systems (the Pip-Boy follows what they opened or closed).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct MenuSystems;

/// Test aid: open the nearest container (F10, or `NVF_PLAY_TEST_MENU=nearest`).
#[derive(Message, Clone, Copy, Debug, Default)]
pub struct OpenNearestContainer;

#[derive(Component)]
struct MenuRoot;

/// A clickable row.
#[derive(Component, Clone, Copy)]
struct RowButton(Side, usize);

/// A clickable command in the button bar.
#[derive(Component, Clone, Copy)]
pub(crate) struct CmdButton(pub(crate) Command);

/// A list's heading (its arrows and title): the wheel over it steps the list's category
/// ([`Command::wheel`]). It lets the pointer through to the arrows under it.
#[derive(Component, Clone, Copy)]
pub(crate) struct CategoryTitle(pub(crate) Side);

/// The "How many?" prompt's meter, which takes a press and a drag.
#[derive(Component)]
pub(crate) struct AmountMeter;

/// Whether the "How many?" prompt had the keys this frame: open when the menus' keys
/// were read, or open now. Other key readers of the same frame (the Pip-Boy's) leave the
/// keys alone, so the A that says Ok does not also change the Pip-Boy's tab.
#[derive(Resource, Default)]
pub struct PromptKeys(pub bool);

impl PromptKeys {
    /// Whether the keys are the prompt's this frame.
    pub fn held(&self, active: &ActiveMenu) -> bool {
        self.0 || active.0.as_ref().is_some_and(|m| m.quantity.is_some())
    }
}

/// [`wheel_hold`], first: what reads [`WheelHold`] this frame runs after it, the hand-built
/// pointer ([`menu_mouse`]) and the tiles' (`crate::tiles::input::feed`), which runs before the
/// other menu systems: a wheel step and a pointer's move in one frame leave the row alone
/// [owner 2026-10-22, #1192].
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WheelHoldSet;

/// Named NVForever option `quantity-select-short-keys` (off by default; #3799):
/// Shift transfers/drops the full stack, Ctrl transfers/drops a single item skipping
/// `QuantityMenu`, and picking up a weightless stack (`count > 1`) from a container opens
/// `QuantityMenu` [documented: JIP LN v57.30 `jip_nvse.ini` `bQttSelectShortKeys`; design:
/// docs/spec-notes/quantity-select-short-keys.md].
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct QuantitySelectShortKeys(pub bool);

/// Keep the active menu's `qtt_select_short_keys` flag aligned with the live Gameplay
/// setting (so toggling it from the pause menu takes effect immediately).
pub(crate) fn sync_quantity_select_short_keys(setting: Option<Res<QuantitySelectShortKeys>>, mut active: ResMut<ActiveMenu>) {
    let want = setting.as_deref().is_some_and(|s| s.0);
    if let Some(m) = active.bypass_change_detection().0.as_mut() {
        if m.qtt_select_short_keys != want {
            m.qtt_select_short_keys = want;
        }
    }
}

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        barter_touch::install(app);
        normal_touch::install(app);
        normal_services::install(app);
        app.configure_sets(Update, WheelHoldSet.before(MenuSystems)).add_systems(Update, wheel_hold.in_set(WheelHoldSet));
        app.init_resource::<ActiveMenu>()
            .init_resource::<QuantitySelectShortKeys>()
            .insert_resource(DevKeys::from_env())
            .init_resource::<FreshMenu>()
            .init_resource::<PromptKeys>()
            .init_resource::<WheelHold>()
            .add_message::<MenuInput>()
            .add_message::<OpenInventory>()
            .add_message::<OpenNearestContainer>()
            .add_message::<OpenBarter>()
            .add_systems(
                Update,
                (
                    f9_opens_nearest_container.run_if(in_state(PlayState::Playing)),
                    test_aid::run,
                    test_aid::run_drops,
                    test_aid::run_barter,
                    scripts_open_barter,
                    scripts_open_teammate_containers,
                    open_menus,
                    sync_quantity_select_short_keys,
                    keep_menu_state,
                    sync_cards,
                    sync_refresh_items_list,
                    publish_menu_picks,
                    publish_jip_menus,
                    (quantity_keys, menu_keys, menu_mouse, quantity_mouse, apply_input).chain().run_if(menu_open),
                    draw,
                )
                    .chain()
                    .in_set(PlaySet::Ui)
                    .in_set(MenuSystems),
            )
            .add_systems(OnEnter(PlayState::Menu), release_cursor)
            .add_systems(OnExit(PlayState::Menu), close_menu);
        crate::settings::add_setting(
            app,
            crate::settings::Setting {
                key: "quantity-select-short-keys",
                group: crate::pause::GROUP_MENUS,
                label: "Quantity select shortcuts",
                note: "In inventory, container and barter menus, Shift transfers or drops the whole stack, Ctrl acts on one item, and weightless container stacks open the quantity prompt.",
                switches: "",
                get: |w| w.resource::<QuantitySelectShortKeys>().0,
                set: |w, on| w.resource_mut::<QuantitySelectShortKeys>().0 = on,
                slider: None,
            },
        );
        test_aid::setup(app);
    }
}

/// Whether the developer keys work in play (F10, the nearest container): only with
/// `NVF_PLAY_DEV_KEYS` set, or while a test aid presses keys (`NVF_PLAY_TEST_KEYS`, as the
/// smoke scenarios do). In plain play the key did nothing the game has, and opened any
/// container in the cell or the worldspace, at any distance [#1988; design].
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct DevKeys(pub bool);

impl DevKeys {
    pub fn from_env() -> DevKeys {
        DevKeys(std::env::var_os("NVF_PLAY_DEV_KEYS").is_some() || std::env::var_os("NVF_PLAY_TEST_KEYS").is_some())
    }
}

/// Test aid: F10 opens the nearest container (F9 is the game's Quick Load), with the
/// developer keys on ([`DevKeys`]); `OpenNearestContainer` always.
fn f9_opens_nearest_container(
    keys: crate::pad::ActionButtons<KeyCode>,
    dev: Res<DevKeys>,
    here: Res<Location>,
    sim: Res<SimState>,
    cam: Query<&GlobalTransform, With<PlayerCamera>>,
    mut requests: MessageReader<OpenNearestContainer>,
    mut out: MessageWriter<OpenContainer>,
) {
    let keys = keys.snapshot();
    if !(dev.0 && keys.just_pressed(KeyCode::F10)) && requests.read().count() == 0 {
        return;
    }
    let Ok(cam) = cam.single() else { return };
    let pos = space::to_game(cam.translation());
    match model::nearest_container(&sim.world, sim.refs, here.interior.map(FormId), FormId(here.world), pos) {
        Some((id, dist)) => {
            let name = sim.base_of(id.0).and_then(|b| sim.name(b)).unwrap_or("?");
            info!("F10: nearest container {id} ({name}) at {dist:.0} units");
            out.write(OpenContainer { container: id.0 });
        }
        None => info!("F10: no container here"),
    }
}

/// For tests: opening a menu and its keys, as the plugin chains them, after `before`.
#[cfg(test)]
pub(crate) fn add_open_and_keys<M>(app: &mut App, before: impl IntoScheduleConfigs<bevy::ecs::system::ScheduleSystem, M>) {
    app.init_resource::<ActiveMenu>()
        .init_resource::<FreshMenu>()
        .init_resource::<PromptKeys>()
        .add_message::<MenuInput>()
        .add_message::<OpenBarter>()
        .add_message::<OpenInventory>()
        .add_message::<ItemUsed>()
        .add_message::<crate::ui_sounds::UiSound>()
        .add_message::<crate::crime::ItemsTaken>()
        .add_systems(Update, (before, open_menus, (quantity_keys, menu_keys, apply_input).chain().run_if(menu_open)).chain().in_set(MenuSystems))
        .add_systems(OnExit(PlayState::Menu),close_menu);
}

/// Whether one of these menus is open.
pub fn menu_open(active: Res<ActiveMenu>) -> bool {
    active.0.is_some()
}

/// Whether none is (for keys other menus take, such as the pause menu's Esc).
pub fn no_menu(active: Res<ActiveMenu>) -> bool {
    active.0.is_none()
}

/// The barter menus scripts asked for (ShowBarterMenu), as [`OpenBarter`].
fn scripts_open_barter(script: Option<ResMut<crate::script::ScriptState>>, mut out: MessageWriter<OpenBarter>) {
    let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) else { return };
    for r in std::mem::take(&mut rt.game.barter_requests) {
        info!("menu: a script opens the barter menu with {} (discount {})", r.merchant, r.discount);
        out.write(OpenBarter { merchant: r.merchant.0, discount: r.discount });
    }
}

/// The container of a merchant's goods: a script's SetMerchantContainer, else its
/// reference's (`fnv_game::Game::merchant_container`); the merchant itself without one.
fn merchant_stock(script: Option<&mut crate::script::ScriptState>, sim: &mut SimState, merchant: FormId) -> FormId {
    let placed = sim.refs.get(merchant).and_then(|p| p.merchant_container);
    match script.and_then(|s| s.rt.as_mut()) {
        Some(rt) => crate::script::with_world(rt, sim, |rt| rt.game.merchant_container(merchant)).unwrap_or(merchant),
        None => placed.unwrap_or(merchant),
    }
}

#[cfg(test)]
mod merchant_clear_tests {
    use super::*;

    #[test]
    fn barter_stock_does_not_restore_an_explicitly_cleared_authored_container() {
        let merchant = FormId(0x301); let stock = FormId(0x310);
        let mut reference = crate::sim::placed_ref(merchant.0);
        reference.merchant_container = Some(stock);
        let mut sim = SimState::over_placed([reference]);
        let mut script = crate::script::ScriptState::over_no_plugins();
        assert_eq!(merchant_stock(Some(&mut script), &mut sim, merchant), stock);
        script.rt.as_mut().unwrap().game.extra.entry(merchant).or_default().merchant_container_cleared = true;
        assert_eq!(merchant_stock(Some(&mut script), &mut sim, merchant), merchant);
        assert_eq!(merchant_stock(None, &mut sim, merchant), stock);
    }
}

#[cfg(test)]
#[path = "menu/merchant_source_tests.rs"]
mod merchant_source_tests;

/// What `merchant` sells, by its services (`fnv_game::services`,
/// docs/spec-notes/vendor-services.md §4), logged with whether it offers its services now;
/// `None` without the script data (everything is sold).
fn vendor_sells(script: Option<&mut crate::script::ScriptState>, sim: &mut SimState, merchant: FormId) -> Option<model::Sells> {
    let rt = script.and_then(|s| s.rt.as_mut())?;
    let (services, now) = crate::script::with_world(rt, sim, |rt| (rt.game.services(merchant), rt.game.offers_services_now(merchant)));
    let data: &'static fnv_game::GameData = rt.game.data;
    let names = fnv_game::services::names(services);
    info!(
        "barter: {} sells {} (its services); offers services now: {}",
        data.name(merchant),
        if names.is_empty() { "nothing".to_string() } else { names.join(", ") },
        if now { "yes" } else { "no" }
    );
    Some(model::Sells(std::sync::Arc::new(move |item| fnv_game::services::item_service(data, item).filter(|c| services & c == 0).map(fnv_game::services::name))))
}

/// The player's Barter skill as scripts see it (`GetActorValue Barter`, within 0 to
/// 100), else [`DEFAULT_BARTER_SKILL`].
fn barter_skill(script: Option<&mut crate::script::ScriptState>, sim: &mut SimState) -> f32 {
    let Some(rt) = script.and_then(|s| s.rt.as_mut()) else { return DEFAULT_BARTER_SKILL };
    crate::script::with_world(rt, sim, |rt| fnv_game::functions::actor_value_capped(&mut rt.game, FormId::PLAYER_REF, fnv_game::actor_values::BARTER)) as f32
}

/// What the No Pip-Boy In Combat tweak reads: the fight, the tweak, the corner line.
type CombatGate<'w> = (Option<Res<'w, crate::after_combat::AfterCombat>>, Option<Res<'w, crate::tweaks::TweakSettings>>, Option<ResMut<'w, crate::corner_messages::CornerMessages>>);

/// Open a menu on request while playing. The inventory stays shut while a script has
/// turned the Pip-Boy off [documented: GECK wiki DisablePlayerControls, PipboyFlag]; a
/// container still opens (a script's own `Activate` asks for it). The barter menu also
/// opens over a conversation.
#[allow(clippy::too_many_arguments)]
pub(crate) fn open_menus(
    (mut containers, mut steals): (MessageReader<OpenContainer>, MessageReader<crate::pickpocket::OpenPickpocket>),
    mut inventory: MessageReader<OpenInventory>,
    mut barters: MessageReader<OpenBarter>,
    state: Res<State<PlayState>>,
    mut next: ResMut<NextState<PlayState>>,
    mut active: ResMut<ActiveMenu>,
    mut sim: ResMut<SimState>,
    script: Option<ResMut<crate::script::ScriptState>>,
    locks: Option<Res<crate::control::ControlLocks>>,
    stats: Option<ResMut<crate::control::LockStats>>,
    wait: Option<Res<crate::wait::WaitMenu>>,
    mut fresh: ResMut<FreshMenu>,
    animated: (Option<ResMut<crate::doors::Doors>>, Option<ResMut<crate::container_anim::ContainerOpening>>),
    (after, tweaks, mut corner): CombatGate,
) {
    let mut script = script.map(|s| s.into_inner());
    let steal = steals.read().last().copied();
    let container = containers.read().last().copied().or(steal.map(|s| OpenContainer { container: s.victim }));
    let barter = barters.read().last().copied();
    let mut inv = inventory.read().count() > 0;
    if inv && locks.is_some_and(|l| l.pipboy) {
        info!("menu: the inventory stays shut, a script has disabled the player's controls (Pip-Boy)");
        if let Some(mut s) = stats {
            s.held[crate::control::group::PIPBOY] += 1;
        }
        inv = false;
    }
    // The player's tweak: no Pip-Boy while something fights the player [design: docs/design/tweaks.md].
    if inv && tweaks.as_ref().is_some_and(|t| t.no_pipboy_in_combat) && after.is_some_and(|a| a.fighting) {
        info!("menu: the Pip-Boy stays shut in combat (tweak)");
        if let Some(c) = corner.as_deref_mut() {
            c.push(crate::tweaks::NO_PIPBOY_LINE.into());
        }
        inv = false;
    }
    if container.is_none() && !inv && barter.is_none() {
        return;
    }
    let talking = *state.get() == PlayState::Dialogue;
    let allowed = *state.get() == PlayState::Playing || (barter.is_some() && talking);
    if !allowed || active.0.is_some() || wait.is_some_and(|w| w.is_open()) {
        info!("menu request ignored: a menu is already open");
        if let Some(b) = barter {
            // Not left open for the scripts either.
            crate::script::engine_menu_closed(script.as_deref_mut(), Mode::Barter(FormId(b.merchant)));
        }
        return;
    }
    // A container whose model opens (a locker, a safe): its "Open" plays first
    // (`crate::container_anim`).
    if let (None, Some(c), (Some(mut doors), Some(mut opening))) = (barter, container, animated) {
        if opening.hold(&mut doors, c.container) {
            return;
        }
    }
    let opened = match (barter, container) {
        (Some(b), _) => {
            let merchant = FormId(b.merchant);
            let stock = merchant_stock(script.as_deref_mut(), &mut sim, merchant);
            let skill = barter_skill(script.as_deref_mut(), &mut sim);
            info!("menu: barter with {merchant}: stock in {stock}, Barter skill {skill}");
            let sells = vendor_sells(script.as_deref_mut(), &mut sim, merchant);
            let barter_hidden = script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| rt.game.barter_hidden_for(merchant)).unwrap_or_default();
            MenuModel::open_barter(&mut sim.world, merchant, stock, model::CAPS, skill, b.discount).and_then(|mut m| {
                if !barter_hidden.is_empty() {
                    m.set_barter_hidden(&mut sim.world, barter_hidden)?;
                }
                if let Some(s) = sells {
                    m.set_sells(&mut sim.world, s)?;
                    let held = m.trade.as_ref().map_or(&[][..], |t| &t.withheld[..]);
                    let list: Vec<String> = held.iter().map(|(i, c)| format!("{} ({c})", model::display_name(sim.world.forms, *i))).collect();
                    info!("barter: {merchant} holds back {} item(s) outside its services: {}", held.len(), if list.is_empty() { "none".into() } else { list.join(", ") });
                }
                // Its own items lying loose in its cell, offered too (`model::owned_loose`, #3350).
                let loose = model::owned_loose(&sim.world, merchant);
                if !loose.is_empty() {
                    let names: Vec<String> = loose.iter().filter_map(|r| sim.world.peek(*r).map(|s| s.base).or_else(|| sim.refs.get(*r).map(|p| p.base))).map(|i| model::display_name(sim.world.forms, i)).collect();
                    info!("barter: {merchant} also offers {} owned loose item(s) lying in its cell: {}", loose.len(), names.join(", "));
                }
                Ok(m)
            })
        }
        (None, Some(c)) if steal.is_some() => {
            let victim = FormId(c.container);
            let rules = script.as_deref_mut().and_then(|s| s.rt.as_mut()).map(|rt| crate::script::with_world(rt, &mut sim, |rt| (rt.game.pickpocket_settings(), rt.game.pickpocket_skills(victim), rt.game.pickpocket_worn(victim), rt.game.pickpocket_overhaul(victim))));
            let (rules, (thief, target), worn, overhaul) = rules.unwrap_or((fnv_game::pickpocket::Rules::default(), (0.0, 0.0), false, None));
            info!("pickpocket: open on {victim}: Sneak {thief:.0} against {target:.0}");
            MenuModel::open_container(&mut sim.world, victim).and_then(|mut m| {
                let mut steal = model::Steal::new(victim, rules, thief, target, model::steal_seed_now(victim));
                steal.worn = worn;
                // The player's Pickpocket Overhaul tweak [design: docs/design/tweaks.md].
                if let Some(o) = &overhaul {
                    info!("pickpocket: the overhaul's figures: Agility {:.0}, Luck {:.0}, Sneak {:.0}, Perception {:.0}, detection {:.0} (tweak)", o.agility, o.luck, o.sneak, o.perception, o.detection);
                }
                steal.overhaul = overhaul;
                m.steal = Some(steal);
                m.refresh(&mut sim.world)?;
                Ok(m)
            })
        }
        (None, Some(c)) => {
            // A co-op guest's container: the host's quest items are not listed.
            let guest = script.as_deref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| rt.game.coop_guest);
            MenuModel::open_container(&mut sim.world, FormId(c.container)).and_then(|mut m| {
                if guest {
                    m.guest = true;
                    m.refresh(&mut sim.world)?;
                }
                Ok(m)
            })
        }
        (None, None) => MenuModel::open_inventory(&mut sim.world),
    };
    match opened {
        Ok(mut m) => {
            // An actor is named by its template's name where the template supplies it
            // (`SimState::ref_name`; playtest list 4, item 20).
            let who = match m.mode {
                Mode::Container(r) | Mode::Barter(r) => Some(r),
                _ => None,
            };
            let game = script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game);
            if let Some(name) = who.filter(|r| sim.refs.get(*r).is_some_and(|p| matches!(&p.sig, b"ACHR" | b"ACRE"))).and_then(|r| sim.ref_name(game, r.0)) {
                m.title = name.to_string();
            }
            let hidden = if matches!(m.mode, Mode::Inventory) { 0 } else { m.list(Side::Container).hidden };
            info!("menu: open {:?} \"{}\"{}", m.mode, m.title, if hidden > 0 { format!(" ({hidden} non-playable stacks hidden)") } else { String::new() });
            if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
                sync_ysi_model(&mut m, &mut rt.game);
            }
            // Scripts run MenuMode 1008 / 1002 / 1053 from now on (layer5-play.md).
            crate::script::engine_menu_opened(script, m.mode);
            // The player's tweak: the world (and the player) runs on while the Pip-Boy comes
            // up; `keep_menu_state` stops it once the arm is up [design: docs/design/tweaks.md].
            let walk_on = matches!(m.mode, Mode::Inventory) && tweaks.as_ref().is_some_and(|t| t.move_during_pipboy_raise);
            active.0 = Some(m);
            fresh.0 = true;
            if walk_on {
                info!("menu: the player moves on while the Pip-Boy comes up (tweak)");
            } else if !talking {
                next.set(PlayState::Menu);
            }
        }
        Err(e) => {
            warn!("menu: cannot open: {e}");
            if let Some(b) = barter {
                crate::script::engine_menu_closed(script, Mode::Barter(FormId(b.merchant)));
            }
        }
    }
}

/// A barter menu opened over a conversation stays a menu when the conversation ends
/// under it.
/// Under the player's Move During the Pip-Boy's Opening tweak, the inventory's Pip-Boy
/// is let come up first: the state stays Playing while its arm is raising, and for at
/// most [`RAISE_START_FRAMES`] frames before it starts (the arm follows the menu later in
/// the frame) [design: docs/design/tweaks.md].
fn keep_menu_state(
    active: Res<ActiveMenu>,
    state: Res<State<PlayState>>,
    mut next: ResMut<NextState<PlayState>>,
    (pip, tweaks): (Option<Res<crate::pipboy::PipBoy>>, Option<Res<crate::tweaks::TweakSettings>>),
    mut waited: Local<u8>,
) {
    if active.0.is_none() || *state.get() != PlayState::Playing {
        *waited = 0;
        return;
    }
    let inventory = active.0.as_ref().is_some_and(|m| m.mode == Mode::Inventory);
    if inventory && tweaks.is_some_and(|t| t.move_during_pipboy_raise) {
        match pip.map(|p| p.phase) {
            Some(crate::pipboy::Phase::Raising) => return,
            Some(crate::pipboy::Phase::Down) if *waited < RAISE_START_FRAMES => {
                *waited += 1;
                return;
            }
            _ => {}
        }
    }
    *waited = 0;
    next.set(PlayState::Menu);
}

/// Frames the Move During the Pip-Boy's Opening tweak waits for the arm to start up.
const RAISE_START_FRAMES: u8 = 3;

fn release_cursor(mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>) {
    if cfg!(target_os = "android") {
        return;
    }
    if let Ok(mut c) = cursor.single_mut() {
        if !c.visible || c.grab_mode != CursorGrabMode::None {
            c.visible = true;
            c.grab_mode = CursorGrabMode::None;
        }
    }
}

fn close_menu(
    mut commands: Commands,
    mut active: ResMut<ActiveMenu>,
    roots: Query<Entity, With<MenuRoot>>,
    script: Option<ResMut<crate::script::ScriptState>>,
) {
    if let Some(m) = active.0.take() {
        crate::script::engine_menu_closed(script.map(|s| s.into_inner()), m.mode);
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
}

fn menu_keys(
    keys: crate::pad::ActionButtons<KeyCode>,
    active: Res<ActiveMenu>,
    mut fresh: ResMut<FreshMenu>,
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
    mut out: MessageWriter<MenuInput>,
) {
    let keys = keys.snapshot();
    use KeyCode::*;
    // The press that opened the menu is not one of its keys (see [`FreshMenu`]).
    if std::mem::take(&mut fresh.0) {
        return;
    }
    let Some(m) = active.0.as_ref().filter(|m| m.mode != Mode::Inventory && m.quantity.is_none()) else { return };
    let wasd = tweaks.as_ref().is_some_and(|t| t.game.wasd_as_arrow_keys);
    let shift = keys.any_pressed([ShiftLeft, ShiftRight]);
    let ctrl_single = m.qtt_select_short_keys && keys.any_pressed([ControlLeft, ControlRight]);
    let mut send = |c| {
        out.write(MenuInput(c));
    };
    // The original's letters for the buttons (barter: A accept, X exit; container: R
    // take all, E exit). Under bUseWASDAsArrowKeys, Barter adds A/D on top of Left/Right
    // for column focus and maps Space to Accept while Enter/NumpadEnter keep picking items
    // [design].
    let barter = matches!(m.mode, Mode::Barter(_));
    let close: &[KeyCode] = if barter { &[Escape, Tab, KeyX] } else { &[Escape, Tab, KeyE] };
    if keys.any_just_pressed(close.iter().copied()) {
        send(Command::Close);
        return;
    }
    let left: &[KeyCode] = if !barter || wasd { &[ArrowLeft, KeyA] } else { &[ArrowLeft] };
    let right: &[KeyCode] = if !barter || wasd { &[ArrowRight, KeyD] } else { &[ArrowRight] };
    // The player's list is drawn at the left of the container and barter menus.
    let map: [(&[KeyCode], Command); 8] = [
        (&[ArrowUp, KeyW], Command::Up),
        (&[ArrowDown, KeyS], Command::Down),
        (&[PageUp], Command::PageUp),
        (&[PageDown], Command::PageDown),
        (left, Command::Focus(Side::Player)),
        (right, Command::Focus(Side::Container)),
        (&[BracketLeft], Command::Category(m.focus, -1)),
        (&[BracketRight], Command::Category(m.focus, 1)),
    ];
    for (k, c) in map {
        if keys.any_just_pressed(k.iter().copied()) {
            send(c);
        }
    }
    let pick: &[KeyCode] = if barter && wasd { &[Enter, NumpadEnter] } else { &[Enter, NumpadEnter, Space] };
    if keys.any_just_pressed(pick.iter().copied()) {
        send(if shift { Command::Stack } else if ctrl_single { Command::OneSingle } else { Command::One });
    }
    match m.mode {
        Mode::Barter(_) if (!wasd && keys.just_pressed(KeyA)) || (wasd && keys.just_pressed(Space)) => send(Command::Accept),
        // The player's tweak: Shift with Take All stores everything instead [design: docs/design/tweaks.md].
        Mode::Container(_) if keys.just_pressed(KeyR) => send(if shift && m.store_all { Command::StoreAll } else { Command::TakeAll }),
        _ => {}
    }
}

/// How far (pixels) the pointer travels after a wheel step before its row takes the
/// selection again [design: owner 2026-10-22, the wheel wins over the row under a
/// resting pointer, else a redrawn list hands the selection back and the wheel sticks].
const WHEEL_HOLD_TRAVEL: f32 = 8.0;

/// Whether the wheel holds the selection against the pointer's row: set by a wheel
/// step, let go once the pointer travels [`WHEEL_HOLD_TRAVEL`]. The item menus and the
/// Pip-Boy's ITEMS list read it.
#[derive(Resource, Default)]
pub struct WheelHold {
    held: bool,
    travel: f32,
}

impl WheelHold {
    pub fn held(&self) -> bool {
        self.held
    }
}

/// Keep [`WheelHold`]: a wheel step holds (moves in the same frame do not count), the
/// pointer's travel lets go.
fn wheel_hold(mut wheel: MessageReader<MouseWheel>, mut moved: MessageReader<CursorMoved>, mut hold: ResMut<WheelHold>) {
    let travel: f32 = moved.read().map(|m| m.delta.map_or(0.0, |d| d.length())).sum();
    if wheel.read().count() > 0 {
        *hold = WheelHold { held: true, travel: 0.0 };
    } else if hold.held {
        hold.travel += travel;
        if hold.travel >= WHEEL_HOLD_TRAVEL {
            hold.held = false;
        }
    }
}

/// [`menu_mouse`]'s view of the item menus drawn from their files.
type MouseTiles<'w> = (Option<Res<'w, crate::tiles::container_menu::ContainerTiles>>, Option<Res<'w, crate::tiles::barter_menu::BarterTiles>>, Option<Res<'w, crate::tiles::container_menu::TileWheelTitle>>);

/// The pointer selects the row it comes onto (the list's highlight follows it
/// [documented: list_box.xml, the template's rows take the pointer]), unless the wheel
/// holds the selection ([`WheelHold`]); a click acts on it, Shift+click on its whole
/// stack.
#[allow(clippy::too_many_arguments)]
fn menu_mouse(
    rows: Query<(&PointerState, &RowButton), Changed<PointerState>>,
    buttons: Query<(&PointerState, &CmdButton), Changed<PointerState>>,
    titles: Query<(&PointerState, &CategoryTitle)>,
    keys: crate::pad::ActionButtons<KeyCode>,
    active: Res<ActiveMenu>,
    hold: Res<WheelHold>,
    mut moved: MessageReader<CursorMoved>,
    mut wheel: MessageReader<MouseWheel>,
    mut lines: Local<f32>,
    mut out: MessageWriter<MenuInput>,
    (tile_menu, barter_tiles, tile_title): MouseTiles,
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
    mouse: Res<ButtonInput<MouseButton>>,
) {
    let keys = keys.snapshot();
    let right_steps_back = tweaks.as_ref().is_some_and(|t| t.right_click_previous_category);
    // The player's tweak turns the wheel over a category title the other way [design: docs/design/tweaks.md].
    let invert_titles = tweaks.as_ref().is_some_and(|t| t.invert_title_wheel);
    let Some(m) = active.0.as_ref().filter(|m| m.mode != Mode::Inventory && m.quantity.is_none()) else {
        wheel.clear();
        return;
    };
    // T15c: drawn from its file the pointer on rows and buttons is `crate::tiles::input`'s,
    // also on the frame the hand-built drawing is still there, which would act on the same
    // press; the wheel stays here, with the heading the tiles say the pointer is over.
    let tiled = tile_menu.is_some_and(|t| t.menu.is_some()) || barter_tiles.is_some_and(|t| t.menu().is_some());
    let hover = !tiled;
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let ctrl_single = m.qtt_select_short_keys && keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    // Only a pointer that moves takes the selection: a redrawn row under a resting
    // pointer leaves the keys' choice alone.
    let pointer_moved = moved.read().count() > 0;
    for (i, r) in &rows {
        let on_it = m.focus == r.0 && m.selected[r.0 as usize] == r.1;
        match *i {
            PointerState::Hovered if hover && !on_it && pointer_moved && !hold.held() => {
                out.write(MenuInput(Command::Hover(r.0, r.1)));
            }
            PointerState::Pressed if hover && on_it && shift => {
                out.write(MenuInput(Command::Stack));
            }
            PointerState::Pressed if hover && on_it && ctrl_single => {
                out.write(MenuInput(Command::OneSingle));
            }
            PointerState::Pressed if hover && !on_it => {
                out.write(MenuInput(Command::Hover(r.0, r.1)));
                out.write(MenuInput(if shift { Command::Stack } else if ctrl_single { Command::OneSingle } else { Command::One }));
            }
            PointerState::Pressed if hover => {
                out.write(MenuInput(Command::Select(r.0, r.1)));
            }
            _ => {}
        }
    }
    for (i, b) in &buttons {
        if *i == PointerState::Pressed && !tiled {
            out.write(MenuInput(b.0));
        }
    }
    for w in wheel.read() {
        *lines += match w.unit {
            MouseScrollUnit::Line => w.y,
            MouseScrollUnit::Pixel => w.y / 20.0,
        };
    }
    // The player's tweak: a right click on a list's heading steps back a category [design: docs/design/tweaks.md].
    if right_steps_back && mouse.just_pressed(MouseButton::Right) {
        let title = if tiled { tile_title.as_ref().and_then(|t| t.0) } else { titles.iter().find(|(i, _)| **i != PointerState::None).map(|(_, t)| t.0) };
        if let Some(side) = title {
            out.write(MenuInput(Command::Category(side, -1)));
        }
    }
    let whole = lines.trunc();
    if whole != 0.0 {
        let title = if tiled { tile_title.and_then(|t| t.0) } else { titles.iter().find(|(i, _)| **i != PointerState::None).map(|(_, t)| t.0) };
        let steps = if invert_titles && title.is_some() { -(whole as i32) } else { whole as i32 };
        out.write(MenuInput(Command::wheel(title, steps)));
        *lines -= whole;
    }
}

/// The "How many?" prompt's keys, while it is open (see the module's notes).
fn quantity_keys(
    keys: crate::pad::ActionButtons<KeyCode>,
    active: Res<ActiveMenu>,
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
    mut held: ResMut<PromptKeys>,
    mut out: MessageWriter<MenuInput>,
) {
    let keys = keys.snapshot();
    use KeyCode::*;
    held.0 = active.0.as_ref().is_some_and(|m| m.quantity.is_some());
    if !held.0 {
        return;
    }
    let wasd = tweaks.as_ref().is_some_and(|t| t.game.wasd_as_arrow_keys);
    let ok: &[KeyCode] = if wasd { &[Enter, NumpadEnter, Space] } else { &[KeyA, Enter, NumpadEnter, Space] };
    let left: &[KeyCode] = if wasd { &[ArrowLeft, KeyA] } else { &[ArrowLeft] };
    let right: &[KeyCode] = if wasd { &[ArrowRight, KeyD] } else { &[ArrowRight] };
    let map = [
        (ok, Command::AmountOk),
        (&[KeyE, Escape][..], Command::AmountCancel),
        (left, Command::AmountStep(-1)),
        (right, Command::AmountStep(1)),
        (&[PageDown][..], Command::AmountStep(-model::QUANTITY_JUMP)),
        (&[PageUp][..], Command::AmountStep(model::QUANTITY_JUMP)),
        (&[Home][..], Command::Amount(1)),
        (&[End][..], Command::Amount(i32::MAX)),
    ];
    for (k, c) in map {
        if keys.any_just_pressed(k.iter().copied()) {
            out.write(MenuInput(c));
        }
    }
}

// A receipt on the existing painted meter, not another input owner. Only the
// original Amount consumer may approve retaining that pressed leaf on repaint.
#[derive(Clone,Copy,PartialEq)]
struct PhysicalMeterStamp {frame:barter_touch::Frame,units:f32,text_size:f32}
#[derive(SystemParam)]
struct PhysicalMeterEnvironment<'w,'s> {
    default_camera:bevy::ui::DefaultUiCamera<'w,'s>,
    windows:Query<'w,'s,(Entity,&'static Window),With<PrimaryWindow>>,
    cameras:Query<'w,'s,(&'static Camera,&'static bevy::camera::RenderTarget)>,
    scale:Option<Res<'w,UiScale>>,safe:Option<Res<'w,crate::touch_safe_area::SafeInsets>>,
    paused:Option<Res<'w,crate::pause::Paused>>,capture:Option<Res<'w,crate::bindings::KeyboardCapture>>,
    free_cam:Option<Res<'w,crate::free_cam::FreeCam>>,fresh:Option<Res<'w,FreshMenu>>,
    creator:Option<Res<'w,crate::creator::CreatorMenu>>,
    readability:Option<Res<'w,crate::menu_readability::MenuReadability>>,
    mouse:crate::pad::ActionButtons<'w,MouseButton>,
}
impl PhysicalMeterEnvironment<'_,'_> {
    fn held(&self)->bool {self.mouse.physical_pressed(MouseButton::Left)}
    fn stamp(&self,m:&MenuModel,script:Option<&crate::script::ScriptState>)->Option<PhysicalMeterStamp> {
        if self.paused.as_ref().is_some_and(|p|p.0)
            || self.capture.as_ref().is_some_and(|c|c.0)
            || self.free_cam.as_ref().is_some_and(|c|c.active)
            || self.fresh.as_ref().is_some_and(|f|f.0)
            || matches!(m.mode,Mode::Barter(_)) && !barter_touch::menu_allows(script,self.creator.as_deref(),m) {return None}
        let (window,w)=self.windows.single().ok()?;let camera=self.default_camera.get()?;
        let (c,target)=self.cameras.get(camera).ok()?;let size=w.resolution.size();
        let density=w.scale_factor();let ui_scale=self.scale.as_ref().map_or(1.0,|s|s.0);
        let units=crate::interface::unit_scale();
        if !w.focused || !c.is_active || !size.is_finite() || size.min_element()<=0.0
            || !density.is_finite() || density<=0.0 || !ui_scale.is_finite() || ui_scale<=0.0
            || !units.is_finite() || units<=0.0
            || !target.normalize(Some(window)).is_some_and(|t|matches!(t,
                bevy::camera::NormalizedRenderTarget::Window(t) if t.entity()==window))
            || c.logical_viewport_size().is_none_or(|s|!s.abs_diff_eq(size,0.01))
            || c.physical_viewport_rect()!=Some(URect::from_corners(UVec2::ZERO,UVec2::new(w.physical_width(),w.physical_height())))
            || c.physical_target_size()!=Some(UVec2::new(w.physical_width(),w.physical_height())) {return None}
        let text_size=if matches!(m.mode,Mode::Barter(_)) {
            readability::size(self.readability.as_deref(),crate::menu_readability::MenuKind::Barter)
        } else {1.0};
        Some(PhysicalMeterStamp {frame:barter_touch::Frame {window,camera,size,
            safe:self.safe.as_ref().map_or_else(Default::default,|s|**s),density,ui_scale,
            admitted:true,fresh:false,neutral:false},units,text_size})
    }
}
fn meter_shape(node:&ComputedNode,at:UiGlobalTransform)->Option<[Vec2;3]> {
    let basis=[node.normalize_point(at,Vec2::ZERO)?,node.normalize_point(at,Vec2::X)?,node.normalize_point(at,Vec2::Y)?];
    basis.iter().all(|v|v.is_finite()).then_some(basis)
}
struct PhysicalMeterRequest {before:model::Quantity,amount:i32,shape:[Vec2;3],style:Node}
#[derive(Component)]
struct PhysicalMeterPaint {
    stamp:Option<PhysicalMeterStamp>,prompt:normal_touch::RenderedMenu,style:Option<Node>,held_shape:Option<[Vec2;3]>,
    requested:Option<PhysicalMeterRequest>,accepted:Option<PhysicalMeterRequest>,
}
impl PhysicalMeterPaint {
    fn new(m:&MenuModel,text_size:f32,stamp:Option<PhysicalMeterStamp>)->Self {
        Self {stamp,prompt:normal_touch::RenderedMenu::of(m,text_size),style:None,held_shape:None,requested:None,accepted:None}
    }
    fn applied(&mut self,cmd:Command,before:Option<model::Quantity>,m:&MenuModel) {
        self.accepted=None;
        let Some(request)=self.requested.take() else{return};
        let Some(stamp)=self.stamp else{return};
        let Some(after)=m.quantity else{return};
        if cmd==Command::Amount(request.amount) && before==Some(request.before)
            && after.amount==request.amount && before.is_some_and(|q|q.amount!=after.amount)
            && self.prompt.amount_repaint(m,stamp.text_size) {
            self.accepted=Some(request);
        }
    }
}
type PhysicalMeterInput<'w,'s>=Query<'w,'s,(&'static PointerState,&'static RelativeCursorPosition,
    Option<(&'static Node,&'static ComputedNode,&'static bevy::ui::UiGlobalTransform,&'static bevy::ui::ComputedUiTargetCamera)>,
    Option<&'static mut PhysicalMeterPaint>),With<AmountMeter>>;
type PhysicalMeterNodes<'w,'s>=Query<'w,'s,(Entity,&'static PointerState,&'static mut PhysicalMeterPaint,
    &'static Node,&'static ComputedNode,&'static bevy::ui::UiGlobalTransform,&'static bevy::ui::ComputedUiTargetCamera),With<AmountMeter>>;

pub(crate) fn retire_physical_quantity_paint(world:&mut World) {
    if !world.get_resource::<ActiveMenu>().and_then(|active|active.0.as_ref())
        .is_some_and(|m|matches!(m.mode,Mode::Barter(_))) {return}
    let mut paints=world.query::<&mut PhysicalMeterPaint>();
    for mut paint in paints.iter_mut(world) {paint.stamp=None;paint.style=None;paint.requested=None;paint.accepted=None;}
}

/// The prompt's pointer: a press or drag on the meter sets the amount under it, the
/// arrows and buttons act, the wheel moves the amount (up: more).
fn quantity_mouse(
    active: Res<ActiveMenu>,
    mut meter: PhysicalMeterInput,
    buttons: Query<(&PointerState, &CmdButton), Changed<PointerState>>,
    mut wheel: MessageReader<MouseWheel>,
    mut lines: Local<f32>,
    mut out: MessageWriter<MenuInput>,
    (environment,script):(PhysicalMeterEnvironment,Option<Res<crate::script::ScriptState>>),
) {
    let Some(q) = active.0.as_ref().and_then(|m| m.quantity) else {
        *lines = 0.0;
        return;
    };
    let stamp=active.0.as_ref().and_then(|m|environment.stamp(m,script.as_deref()));
    for (i,at,geometry,paint) in &mut meter {
        let mut paint=paint;
        if let Some(paint)=paint.as_mut(){paint.requested=None;}
        // Geometry is required only by a known retained-paint contract. Ordinary
        // meters keep their original two-component input admission.
        if let Some(p)=paint.as_ref() {
            let Some((style,node,transform,camera))=geometry else{continue};
            if p.stamp!=stamp || p.style.as_ref()!=Some(style)
                || active.0.as_ref().is_none_or(|m|!p.prompt.quantity_current(m))
                || p.held_shape.is_some_and(|shape|meter_shape(node,*transform)!=Some(shape))
                || stamp.is_some_and(|s|camera.get()!=Some(s.frame.camera)) {continue}
        }
        if let (PointerState::Pressed,Some(p))=(i,at.normalized) {
            let n=model::amount_at(p.x+0.5,q.max);
            if n!=q.amount {
                if environment.held() && stamp.is_some() {
                    if let Some((paint,(style,node,transform,_)))=paint.as_mut().zip(geometry) {
                        if let Some(shape)=meter_shape(node,*transform) {
                            paint.requested=Some(PhysicalMeterRequest {before:q,amount:n,shape,style:style.clone()});
                        }
                    }
                }
                out.write(MenuInput(Command::Amount(n)));
            }
        }
    }
    for (i, b) in &buttons {
        if *i == PointerState::Pressed {
            out.write(MenuInput(b.0));
        }
    }
    for w in wheel.read() {
        *lines += match w.unit {
            MouseScrollUnit::Line => w.y,
            MouseScrollUnit::Pixel => w.y / 20.0,
        };
    }
    let whole = lines.trunc();
    if whole != 0.0 {
        out.write(MenuInput(Command::AmountStep(whole as i32)));
        *lines -= whole;
    }
}

/// The inventory's Caravan cards follow the player's (`fnv_game::caravan`; kept by the
/// scripts' game, not in the inventory): the Misc tab's card folder
/// (`model::Folder::Cards`, or ySI's Caravan Deck category when `bEnableCategories` is on).
fn sync_cards(mut active: ResMut<ActiveMenu>, mut sim: ResMut<SimState>, mut script: Option<ResMut<crate::script::ScriptState>>) {
    let Some(cards) = script.as_ref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game.player.caravan.cards) else { return };
    if !active.0.as_ref().is_some_and(|m| m.mode == Mode::Inventory && m.cards != *cards) {
        return;
    }
    let cards = cards.clone();
    if let Some(m) = active.0.as_mut() {
        if let Err(e) = m.set_cards(&mut sim.world, cards) {
            warn!("menu: the Caravan cards: {e}");
        } else if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
            sync_ysi_model(m, &mut rt.game);
        }
    }
}

/// Attach resolved ySI sorting tags, row icons, and Pip-Boy category folders to `m` when
/// ySI is active in `game`, honoring `yUI.ini` `[Sorting and Icons]` `bSortInventory`,
/// `bAddInventoryIcons`, and `bEnableCategories` (zero-cost no-op in vanilla sessions
/// without ySI) [design: `docs/spec-notes/ysi-filename.md`, `docs/spec-notes/yui-ini.md`].
pub(crate) fn sync_ysi_model(m: &mut MenuModel, game: &mut fnv_game::Game<'_>) {
    let sort_by_tag = game.ysi_sort_inventory();
    let add_icons = game.ysi_add_inventory_icons();
    let categories = if m.mode == Mode::Inventory {
        game.ysi_categories()
    } else {
        None
    };
    let has_categories = categories.is_some();
    m.set_ysi_categories(categories);
    m.ysi_icons = add_icons;
    if sort_by_tag || add_icons || has_categories {
        m.apply_ysi_options(sort_by_tag, add_icons, |item| game.ysi_item_display(item));
    }
}

/// Consume a pending script `RefreshItemsList` request (`fnv_game::nvse_jip3`) and rebuild
/// any open item/recipe menu's rows without closing the menu or losing valid selection
/// [documented: GECK wiki RefreshItemsList; design: docs/spec-notes/ext-menus.md].
pub(crate) fn sync_refresh_items_list(
    mut active: ResMut<ActiveMenu>,
    mut recipe: Option<ResMut<crate::crafting::RecipeMenu>>,
    script: Option<ResMut<crate::script::ScriptState>>,
    mut sim: ResMut<SimState>,
) {
    let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) else { return };
    if !rt.game.take_refresh_items_list() {
        return;
    }
    if let Some(m) = active.0.as_mut() {
        if let Err(e) = m.refresh(&mut sim.world) {
            warn!("menu: RefreshItemsList could not refresh active item list: {e}");
        } else {
            sync_ysi_model(m, &mut rt.game);
        }
    }
    if let Some(open) = recipe.as_deref_mut().and_then(|r| r.0.as_mut()) {
        crate::crafting::refresh_open(rt, &mut sim, open);
    }
}

/// Tell the scripts' game what the open item menus are about and have highlighted, for
/// GetMenuTargetRef and GetSelectedItemRef (`fnv_game::game::MenuPicks`, #983): the
/// container or merchant and the focused column's row (container 1008, inventory 1002,
/// barter 1053), the item being repaired and the part (1035), the weapon being modded and
/// the mod (1061). A closed menu's picks are cleared.
fn publish_menu_picks(
    active: Res<ActiveMenu>,
    modding: Option<Res<crate::pipboy::item_mod::PipItemMod>>,
    repairing: Option<Res<crate::pipboy::repair::PipRepair>>,
    script: Option<ResMut<crate::script::ScriptState>>,
) {
    use fnv_game::game::{MenuPick, MenuPicks};
    let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) else { return };
    let player = FormId::PLAYER_REF;
    let item = |owner, item| Some(MenuPick::Item { owner, item });
    let mut picks: Vec<(u16, MenuPicks)> = [crate::script::MENU_CONTAINER, crate::script::MENU_INVENTORY, crate::script::MENU_BARTER, fnv_game::game::menus::REPAIR, fnv_game::game::menus::ITEM_MOD]
        .into_iter()
        .map(|c| (c, MenuPicks::default()))
        .collect();
    let mut put = |code: u16, p: MenuPicks| {
        if let Some(e) = picks.iter_mut().find(|(c, _)| *c == code) {
            e.1 = p;
        }
    };
    if let Some(m) = active.0.as_ref() {
        let other = match m.mode {
            Mode::Container(c) | Mode::Barter(c) => Some(c),
            Mode::Inventory => None,
        };
        // The merchant's column is what its merchant container holds (`Trade::stock`), not
        // the merchant: the item reference names that container; the target stays the
        // merchant.
        let column = m.trade.as_ref().map(|t| t.stock).or(other);
        let owner = if m.focus == Side::Player { Some(player) } else { column };
        let selected = m.selected_row(m.focus).zip(owner).and_then(|(r, o)| item(o, r.item));
        put(crate::script::menu_code(m.mode), MenuPicks { target: other.map(MenuPick::Form), selected });
    }
    if let Some(s) = modding.as_ref().and_then(|r| r.0.as_ref()) {
        let chosen = s.selected.checked_sub(1).and_then(|i| s.offer.choices.get(i)).map_or(s.offer.item, |c| c.item);
        put(fnv_game::game::menus::ITEM_MOD, MenuPicks { target: item(player, s.offer.item), selected: item(player, chosen) });
    }
    if let Some(s) = repairing.as_ref().and_then(|r| r.0.as_ref()) {
        let chosen = s.selected.checked_sub(1).and_then(|i| s.offer.parts.get(i)).map_or(s.offer.item, |p| p.item);
        put(fnv_game::game::menus::REPAIR, MenuPicks { target: item(player, s.offer.item), selected: item(player, chosen) });
    }
    for (code, p) in picks {
        rt.game.set_menu_picks(code, p);
    }
}

/// Tell the scripts' game the open item menu's filters, lists and trade, and take back a
/// script's barter price multipliers (`fnv_game::nvse_jip12`: GetMenuItemFilter,
/// GetMenuItemListRefs, GetBarterGoldAlt, GetBarterItems, Get/SetBarterPriceMult). The
/// lists are the rows shown: NVForever's lists hold only the rows the category lets
/// through, keyed by their place among them [decision: the original's keys count the
/// filtered rows too].
fn publish_jip_menus(mut active: ResMut<ActiveMenu>, script: Option<ResMut<crate::script::ScriptState>>, mut sim: ResMut<SimState>, mut was: Local<Option<u16>>) {
    use fnv_game::nvse_jip12::{BarterView, ItemMenuView, MenuList};
    let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) else { return };
    let code = active.0.as_ref().map(|m| crate::script::menu_code(m.mode));
    if let Some(old) = was.filter(|&w| Some(w) != code) {
        rt.game.set_item_menu_view(old, None);
        if old == crate::script::MENU_BARTER {
            rt.game.set_barter_view(None);
        }
    }
    *was = code;
    // Read through `as_ref`: taking the menu mutably every frame would mark it changed every
    // frame, and the item menus and the Pip-Boy's ITEMS page redraw on a change, so a click's
    // press and release would meet different rows. Only a script's new multipliers change it.
    let (Some(m), Some(code)) = (active.0.as_ref(), code) else { return };
    let list = |m: &MenuModel, side: Side, owner: Option<FormId>| MenuList { owner, rows: m.list(side).rows.iter().enumerate().filter(|(_, r)| r.folder.is_none()).map(|(i, r)| (i, r.item, true)).collect() };
    let category = |side: Side| model::Category::ORDER.iter().position(|c| *c == m.category[side as usize]).unwrap_or(0) as u8;
    let other = match m.mode {
        Mode::Container(c) => Some(c),
        Mode::Barter(_) => m.trade.as_ref().map(|t| t.stock),
        Mode::Inventory => None,
    };
    let view = ItemMenuView { filter: [category(Side::Player), category(Side::Container)], lists: [list(m, Side::Player, Some(FormId::PLAYER_REF)), list(m, Side::Container, other)] };
    rt.game.set_item_menu_view(code, Some(view));
    let Some(t) = m.trade.as_ref() else { return };
    let units = |side: Side| t.offered[side as usize].keys().map(|u| u.0).collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>();
    rt.game.set_barter_view(Some(BarterView { merchant_caps: t.purse[Side::Container as usize], stock: t.stock, bought: units(Side::Container), sold: units(Side::Player), mult: t.mults() }));
    let set = rt.game.barter_price_mults();
    let hidden = rt.game.barter_hidden_for(t.merchant);
    let mult_changed = t.mult_set != set;
    let hidden_changed = t.hidden != hidden;
    if mult_changed || hidden_changed {
        let Some(m) = active.0.as_mut() else { return };
        if let Some(t) = m.trade.as_mut() {
            t.mult_set = set;
            t.hidden = hidden;
        }
        if mult_changed {
            info!("barter: a script set the price multipliers to {set:?}");
        }
        if let Err(e) = crate::script::with_world(rt, &mut sim, |rt| m.refresh(&mut rt.game.world)) {
            warn!("barter: the trade cannot be listed again: {e}");
        } else {
            sync_ysi_model(m, &mut rt.game);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_input(
    mut input: MessageReader<MenuInput>,
    mut active: ResMut<ActiveMenu>,
    mut sim: ResMut<SimState>,
    state: Res<State<PlayState>>,
    mut next: ResMut<NextState<PlayState>>,
    mut changed: MessageWriter<RefChanged>,
    mut used: MessageWriter<ItemUsed>,
    script: Option<ResMut<crate::script::ScriptState>>,
    mut sounds: MessageWriter<crate::ui_sounds::UiSound>,
    (mut taken, mut stolen): (MessageWriter<crate::crime::ItemsTaken>, MessageWriter<crate::pickpocket::Pickpocketed>),
    mut held: ResMut<PromptKeys>,
    after: Option<Res<crate::after_combat::AfterCombat>>,
    combat: Option<Res<crate::combat::Combat>>,
    (mut mobile_barter, mut normal_barter): (barter_touch::Commits, normal_touch::Commits),
    mut physical_meter:Query<&mut PhysicalMeterPaint>,
) {
    let mut script = script.map(|s| s.into_inner());
    let fighting = after.is_some_and(|a| a.fighting);
    // The commands left of a guard that failed ([`Command::Expect`]).
    let mut dropping = 0;
    for mut paint in &mut physical_meter {paint.accepted=None;}
    for &MenuInput(cmd) in input.read() {
        if !matches!(cmd,Command::Amount(_)) {
            for mut paint in &mut physical_meter {paint.requested=None;paint.accepted=None;}
        }
        let Some(m) = active.0.as_mut() else { return };
        if let Command::Expect(guard) = cmd {
            dropping = if m.row_is(&guard) { 0 } else { guard.then };
            if dropping > 0 {
                info!("menu: a command on a row that changed before it applied is dropped");
            }
            continue;
        }
        if dropping > 0 {
            dropping -= 1;
            if let Command::CommitBarter(token) = cmd { mobile_barter.discard(token); }
            if let Command::CommitNormalBarter(token) = cmd { normal_barter.discard(token); }
            continue;
        }
        let normal_token=if let Command::CommitNormalBarter(token)=cmd {Some(token)} else {None};
        let commands = if let Command::CommitBarter(token) = cmd {
            let Some(commands) = mobile_barter.resolve(token, m, &mut sim.world, script.as_deref()) else { continue };
            commands
        } else if let Command::CommitNormalBarter(token) = cmd {
            let Some(commands) = normal_barter.resolve(token, m, &mut sim.world, script.as_deref()) else { continue };
            commands
        } else { [Some(cmd), None] };
        for cmd in commands.into_iter().flatten() {
            let Some(m) = active.0.as_mut() else { return };
            // The player's tweak: the quantity to store with a companion stops at what it can
            // carry [design: docs/design/tweaks.md].
            if let (Command::One, Mode::Container(c)) = (cmd, m.mode) {
                let cap = script.as_mut().and_then(|s| s.rt.as_mut()).filter(|rt| rt.game.tweaks.quantity_companion_cap && rt.game.is_teammate(c)).map(|rt| {
                    crate::script::with_world(rt, &mut sim, |rt| fnv_game::functions::actor_value(&mut rt.game, c, fnv_game::actor_values::CARRY_WEIGHT).unwrap_or(0.0)) as f32
                });
                m.companion_carry = cap;
            }
            // ShowOff:OnPreActivateInventoryItem / OnPreActivateInventoryItemAlt before
            // inventory item equip/unequip/use (Enter/Space or click on the selected row).
            let inv_activating = m.mode == Mode::Inventory
                && m.quantity.is_none()
                && match cmd {
                    Command::One => true,
                    Command::Select(side, idx) => m.focus == side && m.selected[side as usize] == idx,
                    _ => false,
                };
            if inv_activating {
                let row = m.selected_row(m.focus).filter(|r| r.folder.is_none() && r.kind != Some(fnv_sim::forms::ItemKind::CaravanCard)).cloned();
                if let (Some(row), Some(rt)) = (row, script.as_mut().and_then(|s| s.rt.as_mut()).filter(|rt| rt.game.has_pre_activate_inventory_handlers())) {
                    let allowed = crate::script::with_world(rt, &mut sim, |rt| rt.game.pre_activate_inventory_item(FormId::PLAYER_REF, row.item, 0, !row.equipped));
                    if !allowed {
                        info!("menu: {cmd:?}: {} activation prevented by script", row.name);
                        if m.refresh(&mut sim.world).is_ok() {
                            sync_ysi_model(m, &mut rt.game);
                        }
                        continue;
                    }
                }
            }
            // The player's tweak: no healing aid in a fight [design: docs/design/tweaks.md].
            if matches!(cmd, Command::One) && m.mode == Mode::Inventory {
                let item = m.selected_row(m.focus).filter(|r| !r.equipped && r.folder.is_none()).map(|r| r.item);
                if let (Some(item), Some(rt)) = (item, script.as_mut().and_then(|s| s.rt.as_mut())) {
                    // The player's tweak: no skill book for a skill at 100 [design: docs/design/tweaks.md].
                    if crate::script::with_world(rt, &mut sim, |rt| rt.game.book_refused(item)) {
                        info!("menu: {cmd:?}: refused, the book's skill is at 100 (tweak)");
                        m.status = Some(crate::tweaks::BOOK_AT_MAX_LINE.into());
                        sounds.write(crate::ui_sounds::UiSound { menu: crate::ui_sounds::UiMenu::of(m.mode), event: crate::ui_sounds::UiEvent::Refused });
                        continue;
                    }
                    // The player's tweak: a weapon's requirements must be met [design: docs/design/tweaks.md].
                    if let Some(why) = crate::script::with_world(rt, &mut sim, |rt| rt.game.weapon_requirement_unmet(item)) {
                        info!("menu: {cmd:?}: refused, {why} (tweak)");
                        m.status = Some(why);
                        sounds.write(crate::ui_sounds::UiSound { menu: crate::ui_sounds::UiMenu::of(m.mode), event: crate::ui_sounds::UiEvent::Refused });
                        continue;
                    }
                    if crate::tweaks::healing_barred(rt, &mut sim, item, fighting) {
                        info!("menu: {cmd:?}: refused, healing in combat (tweak)");
                        m.status = Some(crate::tweaks::NO_HEALING_LINE.into());
                        sounds.write(crate::ui_sounds::UiSound { menu: crate::ui_sounds::UiMenu::of(m.mode), event: crate::ui_sounds::UiEvent::Refused });
                        continue;
                    }
                }
            }
            let before = crate::ui_sounds::MenuBefore::of(m);
            let asking = m.quantity.is_some();
            let quantity_before=m.quantity;
            let folder_before = m.folder;
            let a = m.apply(&mut sim.world, cmd);
            let refreshed_rows = a.changed
                || m.folder != folder_before
                || matches!(
                    cmd,
                    Command::Category(..)
                        | Command::SetCategory(..)
                        | Command::SetSort(..)
                        | Command::Back
                        | Command::One
                        | Command::OneSingle
                        | Command::Stack
                        | Command::TakeAll
                        | Command::StoreAll
                        | Command::Drop
                        | Command::DropSingle
                        | Command::DropStack
                        | Command::Accept
                        | Command::AmountOk
                );
            if refreshed_rows {
                if let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) {
                    sync_ysi_model(m, &mut rt.game);
                }
            }
            for mut paint in &mut physical_meter {paint.applied(cmd,quantity_before,m);}
            normal_barter.applied(normal_token,cmd,quantity_before,m);
            held.0 |= m.quantity.is_some();
            if let (false, Some(q)) = (asking, m.quantity) {
                info!("menu: {cmd:?}: how many? ({} of {})", q.amount, q.max);
            }
            if asking && m.quantity.is_none() && matches!(cmd, Command::AmountCancel | Command::Close) {
                info!("menu: {cmd:?}: prompt closed");
            }
            match cmd {
                Command::Category(side, _) | Command::SetCategory(side, _) if m.mode != Mode::Inventory => {
                    info!("menu: {cmd:?}: the {side:?} list shows {:?}", m.category[side as usize]);
                }
                Command::Focus(_) => info!("menu: {cmd:?}: the {:?} list has the focus", m.focus),
                _ => {}
            }
            // What the command did, for its sound (`crate::ui_sounds`).
            if let Some(event) = crate::ui_sounds::menu_event(cmd, &before, m, &a) {
                sounds.write(crate::ui_sounds::UiSound { menu: crate::ui_sounds::UiMenu::of(m.mode), event });
            }
            let moves_items = matches!(cmd, Command::One | Command::OneSingle | Command::Stack | Command::TakeAll | Command::StoreAll | Command::Drop | Command::DropSingle | Command::DropStack | Command::Accept | Command::AmountOk);
            if moves_items || a.changed {
                if let Some(s) = m.status.as_deref().filter(|s| !s.is_empty()) {
                    info!("menu: {cmd:?}: {s}");
                }
            }
            if let Some(item) = a.used {
                used.write(ItemUsed { item: item.0, limb: None });
            }
            // Power armour without the training: the game's message, once the Pip-Boy is
            // closed (`fnv_game::functions::refuse_power_armor`).
            if a.power_armor_refused {
                if let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) {
                    let m = fnv_game::functions::refuse_power_armor(&mut rt.game);
                    info!("menu: power armour refused: no Power Armor Training (message {})", m.map_or_else(|| "none".to_string(), |m| m.to_string()));
                }
            }
            if a.traded > 0 {
                if let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) {
                    rt.game.count_stat(fnv_game::misc_stats::BARTER_AMOUNT_TRADED, f64::from(a.traded));
                }
            }
            // Picking a pocket: the pickpocket rules judge it (`crate::pickpocket`). A live
            // explosive placed is armed now, and the pocket listed again, before the next
            // command reads its rows (#1640).
            if let Some(victim) = m.steal.as_ref().map(|s| s.victim) {
                let mut placed = Vec::new();
                if !a.given.is_empty() {
                    if let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) {
                        let c = combat.as_deref();
                        // The player's tweak: under the weapon's skill requirement an explosive
                        // goes in as a plain item [design: docs/design/tweaks.md].
                        let short: Vec<FormId> = a.given.iter().map(|g| g.0).filter(|&item| c.is_some_and(|c| crate::pickpocket::under_skill(rt, &mut sim, c, item))).collect();
                        placed = crate::pickpocket::arm_given(rt, &mut sim, Some(&mut *m), |item| c.filter(|_| !short.contains(&item)).and_then(|c| crate::pickpocket::explosive_of(c, item)), victim, &a.given);
                    }
                }
                if !a.taken.is_empty() || !a.given.is_empty() || a.caught.is_some() {
                    stolen.write(crate::pickpocket::Pickpocketed { victim, taken: a.taken.clone(), given: a.given.clone(), placed, caught: a.caught });
                }
            } else if let (Mode::Container(c), false) = (m.mode, a.taken.is_empty()) {
                // Taking owned items is theft (`crate::crime`).
                taken.write(crate::crime::ItemsTaken { container: c, items: a.taken.clone() });
            }
            // A dropped item starts turned as its holder faces (GetAngle Z of the new reference
            // equals the player's, X and Y 0 [observed: capture pc_drop_fall, physics.md
            // §4.3]); `World::drop_item` leaves the rotation 0.
            let heading = sim.world.peek(FormId::PLAYER_REF).map_or(0.0, |p| p.rot[2]);
            for id in &a.created {
                if let Ok(s) = sim.world.get(*id) {
                    s.rot = [0.0, 0.0, heading];
                }
                let (at, count) = sim.world.peek(*id).map(|s| (s.pos, s.count)).unwrap_or_default();
                info!("menu: dropped reference {id} (count {count}) at ({:.1}, {:.1}, {:.1})", at[0], at[1], at[2]);
                changed.write(RefChanged::new(id.0));
            }
            // A corpse's dropped weapon taken from its loot menu: picked up from the world,
            // so its collision and drawing go (`model::Row::reference`).
            for id in &a.picked {
                info!("menu: picked up reference {id} lying beside the container");
                changed.write(RefChanged::new(id.0));
            }
            if a.close {
                info!("menu: close");
                if *state.get() == PlayState::Menu {
                    next.set(PlayState::Playing);
                } else if let Some(m) = active.0.take() {
                    // Over a conversation: it goes on (the drawing goes with the model).
                    crate::script::engine_menu_closed(script.as_deref_mut(), m.mode);
                }
                return;
            }
        }
    }
}

// ---- drawing -----------------------------------------------------------------------

/// The line under a container menu: the note for a container that refills.
pub(crate) fn note(m: &MenuModel, sim: &SimState) -> Option<String> {
    let refused = m.status.as_deref().filter(|s| s.ends_with("enough caps.") || s.starts_with("Cannot"));
    if let Some(s) = refused {
        return Some(s.to_string());
    }
    // Picking a pocket: the selected row's chance [design: the original's display is not
    // captured; docs/spec-notes/pickpocket.md].
    if let (Some(st), Some(r)) = (m.steal.as_ref(), m.selected_row(m.focus)) {
        let verb = if m.focus == Side::Container { "steal" } else { "place" };
        return Some(format!("Chance to {verb} {}: {}%", r.name, st.chance(r, 1)));
    }
    let Mode::Container(c) = m.mode else { return None };
    let base = sim.world.peek(c).map(|s| s.base)?;
    matches!(sim.world.forms.get(base).map(|b| &b.data), Some(BaseData::Container(k)) if k.respawns).then(|| RESPAWN_NOTE.to_string())
}

/// What the menu's drawing reads besides the model: the combat records, the plugins and
/// the scripts' game (an aid item's effects, `crate::pipboy::items::info_of`).
type DrawSources<'w> = (Option<Res<'w, crate::combat::Combat>>, Option<Res<'w, crate::PlayWorld>>, Option<ResMut<'w, crate::script::ScriptState>>);

/// [design] The item drawing reads the switch, its previous drawing choice and both
/// item adapters' tile state.
type DrawTiles<'w, 's> = (Option<Res<'w, crate::tiles::vanilla::TileMenus>>, Local<'s, (bool, bool)>, Option<Res<'w, crate::tiles::container_menu::ContainerTiles>>, Option<Res<'w, crate::tiles::barter_menu::BarterTiles>>, Option<Res<'w, crate::tiles::inventory_menu::InventoryTiles>>);

/// Rebuild the container or barter menu's UI when the model changed (the inventory is
/// the Pip-Boy's).
#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    mut active: ResMut<ActiveMenu>,
    roots: Query<Entity, With<MenuRoot>>,
    mut sim: ResMut<SimState>,
    data: Option<Res<GameData>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    glyphs: Res<crate::pad::PadGlyphs>,
    mut kit: Local<Option<look::Kit>>,
    (combat, play, mut script): DrawSources,
    mut forms: Local<Option<crate::pipboy::stats::read::Forms>>,
    (tiles, mut was_tiled, tile_menu, barter_tiles, inventory_tiles): DrawTiles,
    (readability,mut last_text_size):(Option<Res<crate::menu_readability::MenuReadability>>,Local<f32>),
    (mut physical_meter,environment,mut repaint_input):(PhysicalMeterNodes,PhysicalMeterEnvironment,MessageReader<MenuInput>),
) {
    // Drain even on unchanged/absent-menu early returns; queued input is not proof
    // of consumption. The leaf receipt below is produced only by apply_input.
    let mut requested=None;let mut amount_only=true;
    for &MenuInput(cmd) in repaint_input.read() {
        if let Command::Amount(n)=cmd {
            if requested.replace(n).is_some(){amount_only=false;}
        } else {amount_only=false;}
    }
    let stamp=active.0.as_ref().and_then(|m|environment.stamp(m,script.as_deref()));
    let mut retained=None;let mut retained_shape=None;let mut count=0;let mut changed_presentation=false;
    for (e,state,mut paint,style,node,at,camera) in &mut physical_meter {
        count+=1;changed_presentation|=paint.stamp!=stamp || paint.style.as_ref()!=Some(style)
            || paint.held_shape.is_some_and(|shape|meter_shape(node,*at)!=Some(shape));
        let accepted=paint.accepted.take();paint.requested=None;
        if amount_only && environment.held() && *state==PointerState::Pressed && stamp.is_some()
            && paint.stamp==stamp && camera.get()==stamp.map(|s|s.frame.camera)
            && active.0.as_ref().is_some_and(|m|paint.prompt.amount_repaint(m,stamp.unwrap().text_size))
            && accepted.as_ref().is_some_and(|a|Some(a.amount)==requested && meter_shape(node,*at)==Some(a.shape) && a.style==*style)
            && paint.held_shape.is_none_or(|shape|meter_shape(node,*at)==Some(shape)) {
            retained=Some(e);
            // Keep the first accepted visible basis through the held chain.
            retained_shape=accepted.map(|a|paint.held_shape.unwrap_or(a.shape));
        }
    }
    if count!=1 {retained=None;retained_shape=None;}

    // [design] With ContainerMenu or BarterMenu enabled the item menu draws from tiles
    // once its writer has the file built; a sheet takes the
    // pointer here instead (no click past it [documented: `&no_click_past;`]).
    let item_tiled = tile_menu.is_some_and(|t| t.menu.is_some()) || barter_tiles.is_some_and(|t| t.menu().is_some());
    // [design] With QuantityMenu enabled, or over either item menu drawn from tiles,
    // the prompt is drawn from its tiles (`crate::tiles::menus`) once the tree has built its
    // file; drawn again here when that changes (the frame the prompt opens, before the tree
    // builds it, draws it by hand).
    let prompt = active.0.as_ref().is_some_and(|m| m.quantity.is_some());
    let inventory_tiled = inventory_tiles.is_some_and(|t| t.ready.is_some());
    let tiled = script.as_ref().and_then(|s| s.rt.as_ref()).is_some_and(|rt| crate::tiles::menus::quantity_tiled(tiles.as_ref().is_some_and(|t| t.quantity), &rt.game, prompt, item_tiled || inventory_tiled));
    if tiled || !prompt {retained=None;retained_shape=None;}
    let text_size=if active.0.as_ref().is_some_and(|m|matches!(m.mode,Mode::Barter(_))) {
        readability::size(readability.as_deref(),crate::menu_readability::MenuKind::Barter)
    } else {1.0};
    if !active.is_changed() && !roots.is_empty() && (tiled, item_tiled) == *was_tiled && *last_text_size==text_size && !changed_presentation {
        return;
    }
    *was_tiled = (tiled, item_tiled);
    *last_text_size=text_size;
    if let Some(e)=retained {commands.entity(e).remove::<ChildOf>();}
    for e in &roots {
        commands.entity(e).despawn();
    }
    let Some(m) = active.0.as_ref() else { return };
    // The inventory itself is the Pip-Boy's; its prompt is drawn here.
    if m.mode == Mode::Inventory && m.quantity.is_none() {
        return;
    }
    let bc = crate::interface::bc_supported(formats.as_deref());
    let kit = kit.get_or_insert_with(|| look::Kit::load(data.as_deref(), &mut images, bc));
    kit.set_text_size(text_size);
    // The lists' scroll positions follow the rows drawn (`MenuModel::pointer_pick`).
    if m.mode != Mode::Inventory && !item_tiled {
        let first = [Side::Container, Side::Player].map(|s| look::drawn_first(m, s, kit));
        if let Some(m) = active.bypass_change_detection().0.as_mut() {
            m.scroll = first;
        }
    }
    let Some(m) = active.0.as_ref() else { return };
    let screen = Node {
        position_type: PositionType::Absolute,
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        ..default()
    };
    if item_tiled {
        commands.spawn((MenuRoot, normal_touch::RenderedMenu::of(m,text_size), screen.clone(), GlobalZIndex(100))).with_children(|root| {
            root.spawn(crate::tiles::menus::modal_sheet());
            // [design] Barter has no subtitle tile; keep the model's own refusal words.
            if matches!(m.mode, Mode::Barter(_)) {
                if let Some(note) = note(m, &sim) {
                    look::spawn_note(root, &note, kit, data.as_deref(), &mut images, bc, &glyphs);
                }
            }
        });
    } else if m.mode != Mode::Inventory {
        let note = note(m, &sim);
        // The selected item's figures, as the Pip-Boy's ITEMS page reads them
        // (`crate::pipboy::items::info_of`; owner's playtest list 7 item 25).
        if forms.is_none() {
            *forms = play.as_ref().map(|p| crate::pipboy::stats::read::Forms::new(p.lo, p.schema));
        }
        let info = m.selected_row(m.focus).map(|r| crate::pipboy::items::info_of(r, combat.as_deref(), sim.bypass_change_detection(), forms.as_mut(), script.as_mut().map(|s| s.bypass_change_detection())));
        commands.spawn((MenuRoot, normal_touch::RenderedMenu::of(m,text_size), screen.clone(), GlobalZIndex(100))).with_children(|root| {
            look::spawn(root, m, note.as_deref(), info.as_ref(), kit, data.as_deref(), &mut images, bc, &glyphs);
        });
    }
    // Drawn from tiles, the prompt still keeps the pointer from the menu under it (a sheet
    // alone, `crate::tiles::menus::modal_sheet`); it goes with the prompt.
    match (crate::tiles::menus::modal(m.quantity.is_some(), tiled), &m.quantity) {
        (crate::tiles::menus::Modal::HandBuilt, Some(q)) => {
            let words = look::quantity_words(script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game));
            // The player's tweaks: the price in a barter, the weight in a container [design: docs/design/tweaks.md].
            let t = script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| (rt.game.tweaks.quantity_shows_price, rt.game.tweaks.quantity_shows_weight));
            let row = m.list(q.side).rows.get(q.row);
            let (price, weight) = match (m.mode, t) {
                (Mode::Barter(_), Some((true, _))) => (row.and_then(|r| r.price), None),
                (Mode::Container(_), Some((_, true))) => (None, row.and_then(|r| r.weight)),
                _ => (None, None),
            };
            let extra = look::quantity_line(price, weight, q.amount);
            commands.spawn((MenuRoot, normal_touch::RenderedMenu::of(m,text_size), screen, GlobalZIndex(101))).with_children(|root| {
                let paint=matches!(m.mode,Mode::Barter(_)).then(|| {
                    let mut paint=PhysicalMeterPaint::new(m,text_size,stamp);
                    paint.held_shape=retained_shape;paint
                });
                look::spawn_quantity_reusing(root,q,&words,(kit,data.as_deref(),&mut images,bc,&glyphs,extra.as_deref()),
                    (retained,paint));
            });
        }
        (crate::tiles::menus::Modal::Sheet, _) => {
            commands.spawn((MenuRoot, normal_touch::RenderedMenu::of(m,text_size), screen, GlobalZIndex(101))).with_children(|root| {
                root.spawn(crate::tiles::menus::modal_sheet());
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod wheel_hold_tests {
    use super::*;
    use bevy::input::touch::TouchPhase;

    fn step(app: &mut App, wheel: bool, travel: f32) {
        let window = app.world_mut().spawn_empty().id();
        if wheel {
            app.world_mut().write_message(MouseWheel { unit: MouseScrollUnit::Line, x: 0.0, y: -1.0, window, phase: TouchPhase::Moved });
        }
        app.world_mut().write_message(CursorMoved { window, position: Vec2::ZERO, delta: Some(Vec2::new(travel, 0.0)) });
        app.update();
    }

    #[derive(Resource, Default)]
    struct SeenHeld(bool);

    #[test]
    fn a_reader_after_the_set_sees_this_frames_wheel() {
        let mut app = App::new();
        app.init_resource::<WheelHold>()
            .init_resource::<SeenHeld>()
            .add_message::<MouseWheel>()
            .add_message::<CursorMoved>()
            .add_systems(Update, (wheel_hold.in_set(WheelHoldSet), (|hold: Res<WheelHold>, mut seen: ResMut<SeenHeld>| seen.0 = hold.held()).after(WheelHoldSet)));
        step(&mut app, true, 50.0);
        assert!(app.world().resource::<SeenHeld>().0, "the frame's own wheel step, as the tile hover reads it");
    }

    #[test]
    fn the_wheel_holds_until_the_pointer_travels() {
        let mut app = App::new();
        app.init_resource::<WheelHold>().add_message::<MouseWheel>().add_message::<CursorMoved>().add_systems(Update, wheel_hold);
        step(&mut app, true, 50.0);
        assert!(app.world().resource::<WheelHold>().held(), "moves in the wheel's frame do not let go");
        step(&mut app, false, 3.0);
        assert!(app.world().resource::<WheelHold>().held(), "a jitter keeps the hold");
        step(&mut app, false, WHEEL_HOLD_TRAVEL);
        assert!(!app.world().resource::<WheelHold>().held());
    }

    #[test]
    fn wasd_as_arrow_keys_switches_barter_columns_and_steps_quantity_only_when_enabled() {
        let run_barter_key = |wasd: bool, key: KeyCode| -> Vec<Command> {
            let mut app = App::new();
            let m = MenuModel::test_empty(Mode::Barter(FormId(0x100)));
            let mut tweaks = crate::tweaks::TweakSettings::default();
            tweaks.game.wasd_as_arrow_keys = wasd;
            let mut keys = ButtonInput::<KeyCode>::default();
            keys.press(key);
            app.insert_resource(keys)
                .insert_resource(ActiveMenu(Some(m)))
                .insert_resource(FreshMenu(false))
                .insert_resource(tweaks)
                .add_message::<MenuInput>()
                .add_systems(Update, menu_keys);
            app.update();
            app.world_mut()
                .resource_mut::<bevy::ecs::message::Messages<MenuInput>>()
                .drain()
                .map(|e| e.0)
                .collect()
        };

        // Vanilla default (off): arrows switch columns, KeyA accepts, KeyD does nothing, Enter/Space pick items.
        assert_eq!(run_barter_key(false, KeyCode::ArrowLeft), vec![Command::Focus(Side::Player)]);
        assert_eq!(run_barter_key(false, KeyCode::ArrowRight), vec![Command::Focus(Side::Container)]);
        assert_eq!(run_barter_key(false, KeyCode::KeyA), vec![Command::Accept]);
        assert!(run_barter_key(false, KeyCode::KeyD).is_empty());
        assert_eq!(run_barter_key(false, KeyCode::Enter), vec![Command::One]);
        assert_eq!(run_barter_key(false, KeyCode::Space), vec![Command::One]);

        // Enabled (bUseWASDAsArrowKeys): arrows still switch columns, KeyA/KeyD also switch columns,
        // Enter still picks items, and Space accepts the barter trade.
        assert_eq!(run_barter_key(true, KeyCode::ArrowLeft), vec![Command::Focus(Side::Player)]);
        assert_eq!(run_barter_key(true, KeyCode::ArrowRight), vec![Command::Focus(Side::Container)]);
        assert_eq!(run_barter_key(true, KeyCode::KeyA), vec![Command::Focus(Side::Player)]);
        assert_eq!(run_barter_key(true, KeyCode::KeyD), vec![Command::Focus(Side::Container)]);
        assert_eq!(run_barter_key(true, KeyCode::Enter), vec![Command::One]);
        assert_eq!(run_barter_key(true, KeyCode::Space), vec![Command::Accept]);

        let run_qty_key = |wasd: bool, key: KeyCode| -> Vec<Command> {
            let mut app = App::new();
            let mut m = MenuModel::test_empty(Mode::Container(FormId(0x100)));
            m.quantity = Some(model::Quantity {
                cmd: Command::One,
                side: Side::Player,
                row: 0,
                max: 5,
                amount: 2,
            });
            let mut tweaks = crate::tweaks::TweakSettings::default();
            tweaks.game.wasd_as_arrow_keys = wasd;
            let mut keys = ButtonInput::<KeyCode>::default();
            keys.press(key);
            app.insert_resource(keys)
                .insert_resource(ActiveMenu(Some(m)))
                .insert_resource(PromptKeys(false))
                .insert_resource(tweaks)
                .add_message::<MenuInput>()
                .add_systems(Update, quantity_keys);
            app.update();
            app.world_mut()
                .resource_mut::<bevy::ecs::message::Messages<MenuInput>>()
                .drain()
                .map(|e| e.0)
                .collect()
        };

        // Vanilla default (off): arrows step -1/+1, KeyA/Enter/Space confirm Quantity, KeyD does nothing.
        assert_eq!(run_qty_key(false, KeyCode::ArrowLeft), vec![Command::AmountStep(-1)]);
        assert_eq!(run_qty_key(false, KeyCode::ArrowRight), vec![Command::AmountStep(1)]);
        assert_eq!(run_qty_key(false, KeyCode::KeyA), vec![Command::AmountOk]);
        assert!(run_qty_key(false, KeyCode::KeyD).is_empty());

        // Enabled (bUseWASDAsArrowKeys): arrows still step -1/+1, KeyA/KeyD also step -1/+1, and Enter/Space confirm.
        assert_eq!(run_qty_key(true, KeyCode::ArrowLeft), vec![Command::AmountStep(-1)]);
        assert_eq!(run_qty_key(true, KeyCode::ArrowRight), vec![Command::AmountStep(1)]);
        assert_eq!(run_qty_key(true, KeyCode::KeyA), vec![Command::AmountStep(-1)]);
        assert_eq!(run_qty_key(true, KeyCode::KeyD), vec![Command::AmountStep(1)]);
        assert_eq!(run_qty_key(true, KeyCode::Enter), vec![Command::AmountOk]);
        assert_eq!(run_qty_key(true, KeyCode::Space), vec![Command::AmountOk]);
    }

    #[test]
    fn release_cursor_skips_redundant_mutation_when_already_released() {
        let mut app = App::new();
        app.add_systems(Update, release_cursor);
        let entity = app
            .world_mut()
            .spawn((
                PrimaryWindow,
                CursorOptions {
                    visible: true,
                    grab_mode: CursorGrabMode::None,
                    ..default()
                },
            ))
            .id();
        // Clear initial Added tick so the next update only flags actual writes.
        app.world_mut().clear_trackers();
        app.update();
        let changed = app
            .world_mut()
            .query_filtered::<Entity, Changed<CursorOptions>>()
            .iter(app.world())
            .any(|e| e == entity);
        assert!(!changed, "already-released cursor should not be marked changed");

        app.world_mut()
            .entity_mut(entity)
            .get_mut::<CursorOptions>()
            .unwrap()
            .grab_mode = CursorGrabMode::Locked;
        app.world_mut().clear_trackers();
        app.update();
        let cursor = app.world().get::<CursorOptions>(entity).unwrap();
        assert_eq!(cursor.grab_mode, CursorGrabMode::None);
        assert!(cursor.visible);
    }
}

#[cfg(test)]
#[path = "menu/preactivation_tests.rs"]
mod preactivation_tests;

#[cfg(test)]
mod preactivation_mutation_tests;

#[cfg(test)]
pub(crate) mod classic_dialogue_tests;
