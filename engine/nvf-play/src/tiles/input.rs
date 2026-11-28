//! T8: the pointer, the mouse buttons and the keys fed to the menus drawn from tiles
//! (`fnv_game::ui_input`, `fnv_ui::input`), each frame before scripts [design:
//! ui-tile-tree.md §4.1 step 1].
//!
//! - **Turned tiles** (`rotateangle`, `super::layout::Turn`): hit-tested on their unturned
//!   box, as `fnv_ui::input::hit` reads the tree [design: no tile the files turn takes the
//!   pointer, `map_menu.xml`'s arrows and dotted path lines have no `target`; menu-xml.md
//!   §3.1.4].
//! - **Which menus take input**: a menu drawn from its file that the player works: since
//!   T15a the message box and the "How many?" prompt while drawn from tiles
//!   (`[UI] TileMenus=MessageMenu,QuantityMenu`, or for the prompt a container menu drawn
//!   from its tiles, `super::menus`), the prompt under the box; since T11 the start menu while the pause menu's Mods page is up
//!   (`[UI] TileMenus=StartMenu`, `super::mods`). The HUD (`[UI] TileMenus=HUDMainMenu`) is drawn from tiles but has no pointer in
//!   play [hypothesis: the original's HUD takes no clicks]. With none, nothing is fed and
//!   the frame does no tree work [design: the maintainer's review of #902], unless the test
//!   aid below makes the HUD take input; but a script's ClickMenuButton or ClickMenuTile
//!   still ends (`clicked` back to 0) and fires its event the next frame
//!   (`fnv_game::ui_input::idle`) [design: the maintainer's review of #957].
//! - **What the clicks do** (T15a, [`MenuInputs`]): a pointer's or a script's click on a
//!   message box row answers the box with that button ([`MessageClick`], read by
//!   `crate::script_hud::menu_keys`), and the pointer over a row frames it (the hand-built
//!   box's rule, `crate::message_menu`); on the prompt OK, Cancel and the two arrows send
//!   their commands (`crate::menu::MenuInput`) and a press on the meter, held as it drags,
//!   sets the amount under the pointer (`crate::menu::model::amount_at`). The hand-built
//!   keys (`crate::menu`'s prompt keys, `crate::script_hud::menu_keys`) stay the menus'
//!   keys: a click a key made through the files' `_PCButton_*` or `x…` refs acts unless
//!   that key came from the keyboard (or a controller standing in for it) and the
//!   hand-built handler took it this frame (`super::menus::key_click_acts`), so a press acts
//!   once while a script's MenuTapKey and a file's own bindings still act; a highlight a
//!   ref moved for a hand-built Up or Down is not taken as the box's choice
//!   (`super::menus::chosen_row`) [design: the T15a review].
//! - **The Wait / Sleep menu** (T15b, `super::wait_menu`): its Wait and Cancel buttons and
//!   the slider's arrows and page areas send the hand-built menu's commands
//!   (`crate::wait::WaitInput`), a press on the marker, held as it drags, sets the hours
//!   under the pointer; the slider's value is written again after them.
//! - **The tutorial menu** (T15b, `super::tutorial_menu`): its buttons and the scrollbar's
//!   arrows and page areas send the hand-built menu's commands
//!   (`crate::tutorial_menu::TutorialInput`), a press on the marker, held as it drags, sets
//!   the scroll under the pointer. It is the only menu taking input while up: no click goes
//!   past it [documented: `&no_click_past;`]. The wheel stays the hand-built menu's. On the
//!   frame it is first built, when the hand-built buttons of the frame before still stand, it
//!   takes nothing, so a press acts once, on what was on screen [design].
//! - **The Pip-Boy's STATS page** (`super::stats_menu`): takes the pointer on its own
//!   canvas (`crate::pipboy::PipBoyScreen::pointer`), so its input is its own system,
//!   `super::stats_menu::feed`, on a frame no other menu takes input; this one then leaves
//!   the frame's one step to it. The ITEMS page likewise (`super::inventory_menu::feed`).
//! - **The Pip-Boy's repair page** (`super::repair_menu`): likewise, by its own system,
//!   `super::repair_menu::feed`; and the weapon mod menu (`super::item_mod_menu::feed`).
//! - **The container menu** (T15c, `super::container_menu`): a row the pointer comes onto is
//!   selected, unless the wheel holds the selection; a click on a row, a heading or its arrow,
//!   Take All and Exit send the hand-built menu's commands (`crate::menu::MenuInput`), to
//!   `apply_input`, a row's behind a guard saying what the row stood for, checked as they apply.
//!   A click on a heading steps it on [observed: `container__category_heading_click`]. The
//!   scrollbar's arrows, page areas and marker move the list's view, no
//!   command of the model's. Under the "How many?" prompt it takes nothing. The wheel stays
//!   `crate::menu::menu_mouse`'s, told which heading the pointer is over.
//! - **The recipe menu** (`super::recipe_menu`): a row the pointer comes onto is selected, a
//!   click on a row selects it or, on the selected row, accepts; the arrows and the title step
//!   the filter; Accept and Exit act. What it asks goes to `crate::crafting`'s `keys` as
//!   `crate::crafting::RecipeInput`. The scrollbars' arrows, page areas and markers move the
//!   lists' views. Under the "How many?" prompt it takes nothing. The wheel stays the model's.
//! - **The trait menu** (`super::trait_menu`): a row the pointer comes onto is chosen, a click on
//!   a row, Reset or Done is queued for `crate::chargen`'s `input`, named by its trait; the two
//!   bars' arrows, page areas and markers move their views. On the frame it is first ready, when
//!   the hand-built menu of the frame before still stands, it takes nothing, as the tutorial
//!   menu. The wheel and the keys stay `crate::chargen`'s.
//! - **The dialogue menu** (`super::dialog_menu`): while drawn from its tiles with nothing over
//!   it and no pause menu; what it asks goes to `crate::dialogue::input` through
//!   `crate::dialogue::ViewInput`, the same commands as the keys. It takes the input as drawn
//!   the frame before, so a click is on what was on screen.
//! - **The vendor's repair menu** (`super::repair_services_menu`): a row the pointer comes onto
//!   is selected; a click on the selected row or Repair repairs it, on another row selects it;
//!   Repair All and Done act. What it asks goes to `crate::repair`'s `keys` as
//!   `crate::repair::RepairServicesInput`. The scrollbar's arrows, page areas and marker move the
//!   list's view. The keys and the wheel stay the model's.
//! - **Units**: the cursor's logical pixels to interface units, 960 to the view's height,
//!   as the tile renderer draws (`super::draw`, `crate::interface::u`) [design].
//! - **Keys**: A to Z as `_PCButton_<letter>` [documented: the vanilla files' letters;
//!   LStewieAl's Tweaks adds digits, F-keys, Caps, Tab and Alt, so the unmodded game has
//!   none of those]; the arrow keys as `xup` … `xright` [documented: Input]; the
//!   controller's buttons as their `x…` traits (A `xbuttona`, the shoulders `xbuttonlb`
//!   `xbuttonrb`, the triggers `xbuttonlt` `xbuttonrt`, the sticks' buttons, Start, the
//!   D-pad) [design: the Xbox names the files use]; MenuTapKey's keys (`Game::ui_taps`)
//!   as one frame's presses [documented: MenuTapKey, "fakes a key press for one frame in
//!   menu mode"]. A press is a key going down [hypothesis].
//! - The text tiles without a size are not hit: the fonts' measure lives in the renderer
//!   (`super::draw`), and the HUD's measured texts have their width written into the tree
//!   (`FontKit::write_widths`) [design].
//!
//! Test aid `NVF_PLAY_TEST_POINTER=<seconds>:<x>:<y>[:click][,…]`: from that time the HUD
//! (drawn from tiles) takes input as an open menu would, with the pointer at (x, y) in
//! interface units, and `click` a left-button press on that frame; `<seconds>:off` lets
//! it go. The mouse, keys and controller then reach it too (`NVF_PLAY_TEST_KEYS`).

use crate::script::ScriptState;
use super::list;
use crate::menu::model::Side;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use fnv_ui::input::{no_measure, Input};

pub const AID: &str = "NVF_PLAY_TEST_POINTER";

/// One item of the test aid: a place and whether it clicks, or off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AidPointer {
    At { x: f64, y: f64, click: bool },
    Off,
}

/// `<x>:<y>[:click]` or `off`.
pub fn aid_pointer(rest: &str) -> Result<AidPointer, String> {
    if rest.trim().eq_ignore_ascii_case("off") {
        return Ok(AidPointer::Off);
    }
    let mut parts = rest.split(':').map(str::trim);
    let num = |s: Option<&str>| s.and_then(|s| s.parse::<f64>().ok()).ok_or_else(|| format!("{rest:?}: expected <x>:<y>[:click] or off"));
    let x = num(parts.next())?;
    let y = num(parts.next())?;
    let click = match parts.next() {
        None => false,
        Some(c) if c.eq_ignore_ascii_case("click") => true,
        Some(c) => return Err(format!("{rest:?}: unknown {c:?}")),
    };
    Ok(AidPointer::At { x, y, click })
}

/// The test aid's state: what is still to come, and the pointer it holds now.
#[derive(Resource, Default)]
pub(crate) struct PointerAid {
    due: Vec<(f32, AidPointer)>,
    now: Option<(f64, f64)>,
}

/// The letters' keys, A to Z.
const LETTERS: [(KeyCode, &str); 26] = [
    (KeyCode::KeyA, "A"), (KeyCode::KeyB, "B"), (KeyCode::KeyC, "C"), (KeyCode::KeyD, "D"), (KeyCode::KeyE, "E"), (KeyCode::KeyF, "F"),
    (KeyCode::KeyG, "G"), (KeyCode::KeyH, "H"), (KeyCode::KeyI, "I"), (KeyCode::KeyJ, "J"), (KeyCode::KeyK, "K"), (KeyCode::KeyL, "L"),
    (KeyCode::KeyM, "M"), (KeyCode::KeyN, "N"), (KeyCode::KeyO, "O"), (KeyCode::KeyP, "P"), (KeyCode::KeyQ, "Q"), (KeyCode::KeyR, "R"),
    (KeyCode::KeyS, "S"), (KeyCode::KeyT, "T"), (KeyCode::KeyU, "U"), (KeyCode::KeyV, "V"), (KeyCode::KeyW, "W"), (KeyCode::KeyX, "X"),
    (KeyCode::KeyY, "Y"), (KeyCode::KeyZ, "Z"),
];

const ARROWS: [(KeyCode, &str); 4] = [(KeyCode::ArrowUp, "xup"), (KeyCode::ArrowDown, "xdown"), (KeyCode::ArrowLeft, "xleft"), (KeyCode::ArrowRight, "xright")];

const PAD: [(GamepadButton, &str); 15] = [
    (GamepadButton::South, "xbuttona"),
    (GamepadButton::East, "xbuttonb"),
    (GamepadButton::West, "xbuttonx"),
    (GamepadButton::North, "xbuttony"),
    (GamepadButton::LeftTrigger2, "xbuttonlt"),
    (GamepadButton::RightTrigger2, "xbuttonrt"),
    (GamepadButton::LeftTrigger, "xbuttonlb"),
    (GamepadButton::RightTrigger, "xbuttonrb"),
    (GamepadButton::LeftThumb, "xbuttonls"),
    (GamepadButton::RightThumb, "xbuttonrs"),
    (GamepadButton::Start, "xbuttonstart"),
    (GamepadButton::DPadUp, "xup"),
    (GamepadButton::DPadDown, "xdown"),
    (GamepadButton::DPadLeft, "xleft"),
    (GamepadButton::DPadRight, "xright"),
];

/// A key's name for the tiles: a letter for `_PCButton_*`, or an arrow's trait.
pub fn key_name(k: KeyCode) -> Option<(bool, &'static str)> {
    LETTERS.iter().find(|(c, _)| *c == k).map(|(_, n)| (true, *n)).or_else(|| ARROWS.iter().find(|(c, _)| *c == k).map(|(_, n)| (false, *n)))
}

/// Adds a key once: a letter to the keys, an arrow or a button to the pads (the D-pad and
/// the arrow keys it stands for in menus, `crate::pad::menu_keys`, press `xup` once).
fn push(input: &mut Input, (letter, name): (bool, &'static str)) {
    let list = if letter { &mut input.keys } else { &mut input.pads };
    if !list.iter().any(|n| n == name) {
        list.push(name.to_string());
    }
}

/// A window position (logical pixels from the top left) in interface units.
pub fn to_units(at: Vec2, window_height: f32) -> (f64, f64) {
    let per = f64::from(window_height.max(1.0)) / 960.0;
    (f64::from(at.x) / per, f64::from(at.y) / per)
}

/// A window position (logical pixels from the top left, `window` the window's logical
/// size) in the interface units of a tile menu drawn at the UI scale in a frame kept at
/// `anchor` (`crate::interface::menu_frame`; `super::draw` draws the menus so): the
/// point where the menu, drawn at the original's size, would have what the pointer is on
/// [design: #1631]. [`to_units`] at the original's size.
pub fn to_menu_units(at: Vec2, window: Vec2, anchor: Vec2) -> (f64, f64) {
    to_units(crate::interface::from_menu_frame(at, window, anchor), window.y)
}

#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct Devices<'w, 's> {
    pub(super) keys: crate::pad::ActionButtons<'w, KeyCode>,
    pub(super) mouse: crate::pad::ActionButtons<'w, MouseButton>,
    pads: crate::pad::ActionPads<'w, 's>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    /// The window's pointer asleep under the controller is on nothing (#2310).
    pad_pointer: Option<Res<'w, crate::pad::pointer::PadPointer>>,
    touch_owner: Option<Res<'w, crate::ui_pointer::TouchUiCapture>>,
}

/// T15a: the message box button the pointer or a script clicked on its tiles this frame,
/// for `crate::script_hud::menu_keys` to answer the box with (taken there).
#[derive(Resource, Default)]
pub struct MessageClick(pub Option<usize>);

/// T15a: what the clicks on the message box and the prompt drawn from tiles act on.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct MenuInputs<'w, 's> {
    panel: Option<ResMut<'w, crate::message_menu::MessagePanel>>,
    click: ResMut<'w, MessageClick>,
    active: Option<Res<'w, crate::menu::ActiveMenu>>,
    commands: MessageWriter<'w, crate::menu::MenuInput>,
    normal: Option<ResMut<'w, crate::menu::normal_touch::NormalBarter>>,
    normal_event: Local<'s, Option<crate::menu::normal_touch::NativeTap>>,
    normal_emitted: Local<'s, bool>,
    normal_gate: crate::menu::normal_touch::NativeAdmission<'w,'s>,
    services:Option<ResMut<'w,crate::menu::normal_services::NormalServices>>,
    service_event:Local<'s,Option<crate::menu::normal_services::NativeTap>>,
    service_emitted:Local<'s,bool>,
    service_gate:crate::menu::normal_services::NativeAdmission<'w,'s>,
    /// A press on the prompt's meter, held: the amount follows the pointer.
    dragging: Local<'s, bool>,
    /// T15b: the Wait / Sleep menu, and its commands.
    wait: Option<Res<'w, crate::wait::WaitMenu>>,
    wait_commands: MessageWriter<'w, crate::wait::WaitInput>,
    /// A press on the Wait menu's slider, held: the hours follow the pointer.
    wait_dragging: Local<'s, bool>,
    /// The slider's left edge while it drags (`super::wait_menu::slider_left`), worked out
    /// at the press.
    wait_left: Local<'s, Option<f64>>,
    /// The hours the slider was written for this frame, for [`check_wait_drawn`].
    wait_check: ResMut<'w, WaitSliderCheck>,
    /// T15b: the tutorial menu's commands.
    tutorial_commands: MessageWriter<'w, crate::tutorial_menu::TutorialInput>,
    /// A press on the tutorial menu's scroll marker, held: what
    /// `super::tutorial_menu::marker_grab` worked out at the press.
    tutorial_drag: Local<'s, Option<super::tutorial_menu::Drag>>,
    /// The tutorial menu's hand-built buttons, while they stand.
    tutorial_hand_built: Query<'w, 's, (), With<crate::tutorial_menu::TmButton>>,
    /// T15c: the container menu's state, the writer's and this input's.
    container: ResMut<'w, super::container_menu::ContainerTiles>,
    barter: Option<ResMut<'w, super::barter_menu::BarterTiles>>,
    /// The recipe menu's state, the model and what the tiles ask of it.
    recipe: Option<ResMut<'w, super::recipe_menu::RecipeTiles>>,
    recipe_model: Option<Res<'w, crate::crafting::RecipeMenu>>,
    recipe_commands: MessageWriter<'w, crate::crafting::RecipeInput>,
    /// The Pip-Boy's repair page's decision, and whether its list is open.
    repair_page: Option<ResMut<'w, super::repair_menu::RepairTiles>>,
    repair_list: Option<Res<'w, crate::pipboy::repair::PipRepair>>,
    /// The vendor's repair menu's state, and what the tiles ask of its model.
    repair_services: Option<ResMut<'w, super::repair_services_menu::RepairServicesTiles>>,
    repair_commands: MessageWriter<'w, crate::repair::RepairServicesInput>,
    /// The pointer's place last frame, to know a list moved under it from it moving.
    last_at: Local<'s, Option<(f64, f64)>>,
    hold: Option<Res<'w, crate::menu::WheelHold>>,
    wheel_title: ResMut<'w, super::container_menu::TileWheelTitle>,
    /// The trait menu's state, the writer's and this input's.
    traits: Option<ResMut<'w, super::trait_menu::TraitTiles>>,
    tags: Option<ResMut<'w, super::tag_menu::TagTiles>>,
    /// The hand-built trait menu's rows, while they stand.
    trait_hand_built: Query<'w, 's, (), With<crate::chargen::lists::ListRow>>,
    /// The dialogue menu's one decision, what it asks of the conversation, and its scroll.
    dialog: Option<Res<'w, super::dialog_menu::DialogTiles>>,
    view_input: Option<ResMut<'w, crate::dialogue::ViewInput>>,
    dialog_written: Option<Res<'w, super::dialog_menu::SharedWritten>>,
}

impl MenuInputs<'_, '_> {
    /// Only the actual pointer click belonging to this owned native End may
    /// resolve its token. Script/key clicks keep their original command path.
    fn normal_pointer_commands(&mut self, ui:&fnv_ui::Ui, click:&fnv_ui::input::Click, commands:&[crate::menu::model::Command])->bool {
        use crate::menu::{model::Command,MenuInput};
        let Some(n)=*self.normal_event else{return false};
        if !matches!(click.by,fnv_ui::input::By::Pointer){return false}
        let crate::menu::normal_touch::Source::Tile(menu,_) = n.source else{return false};
        if *self.normal_emitted || ui.root_of(click.tile)!=menu {return true}
        if let Some(token)=self.normal.as_ref().and_then(|state|state.native_commands(n,commands)) {
            self.commands.write(MenuInput(Command::CommitNormalBarter(token)));
            *self.normal_emitted=true;
        }
        true
    }

    fn normal_service_command(&mut self,ui:&fnv_ui::Ui,click:&fnv_ui::input::Click,
        action:crate::menu::normal_services::Action)->bool {
        use crate::menu::normal_services::{Action,Kind};
        let Some(n)=*self.service_event else{return false};
        if !matches!(click.by,fnv_ui::input::By::Pointer){return false}
        let crate::menu::normal_touch::Source::Tile(menu,_)=n.source else{return false};
        if *self.service_emitted || ui.root_of(click.tile)!=menu{return true}
        if let Some(token)=self.services.as_ref().and_then(|state|state.native_command(n,action)) {
            match (n.kind,action) {
                (Kind::Vendor,Action::Vendor(_))=>{self.repair_commands.write(crate::repair::RepairServicesInput::CommitNormal(token));}
                (Kind::Workbench,Action::Workbench(_))=>{self.recipe_commands.write(crate::crafting::RecipeInput::CommitNormal(token));}
                _=>return true,
            }
            *self.service_emitted=true;
        }
        true
    }

    fn quantity(&self) -> Option<crate::menu::model::Quantity> {
        self.active.as_ref().and_then(|a| a.0.as_ref()).and_then(|m| m.quantity)
    }

    /// The recipe menu's own prompt is open: it stays hand-built, and has the input.
    fn recipe_asking(&self) -> bool {
        self.recipe_model.as_ref().and_then(|m| m.0.as_ref()).is_some_and(|o| o.quantity.is_some())
    }

    /// The recipe menu while it takes input.
    fn recipe_takes(&self) -> Option<fnv_ui::TileId> {
        super::container_menu::takes_input(self.recipe.as_ref().and_then(|r| r.menu), self.recipe_asking())
    }

    /// The menus drawn from tiles that take input this frame, back to front: the prompt,
    /// then the message box over it.
    fn taking(&self, menus: super::vanilla::TileMenus, game: &fnv_game::Game<'_>, inventory_tiled: bool) -> Vec<fnv_ui::TileId> {
        use super::menus::{message_tiled, quantity_tiled, MESSAGE_MENU, QUANTITY_MENU};
        let mut out = Vec::new();
        if super::tutorial_menu::tiled(menus.tutorial, game) {
            // The buttons of the frame the menu opened are what the player sees until this
            // frame's drawing: this frame's press is theirs, the file's from the next.
            if self.tutorial_hand_built.is_empty() {
                out.extend(game.ui_tree.menu(super::tutorial_menu::TUTORIAL_MENU));
            }
            return out;
        }
        out.extend(super::container_menu::takes_input(self.container.menu, self.quantity().is_some()));
        out.extend(super::container_menu::takes_input(self.barter.as_ref().and_then(|b| b.menu()), self.quantity().is_some()));
        out.extend(self.recipe_takes());
        out.extend(self.repair_services.as_ref().and_then(|r| r.menu));
        if self.trait_hand_built.is_empty() {
            out.extend(self.traits.as_ref().and_then(|t| t.takes()));
            out.extend(self.tags.as_ref().and_then(|t| t.takes()));
        }
        if quantity_tiled(menus.quantity, game, self.quantity().is_some(), self.container.menu.is_some() || self.barter.as_ref().is_some_and(|b| b.menu().is_some()) || inventory_tiled) {
            out.extend(game.ui_tree.menu(QUANTITY_MENU));
        }
        if message_tiled(menus.message, game) {
            out.extend(game.ui_tree.menu(MESSAGE_MENU));
        }
        if super::wait_menu::tiled(menus.sleep_wait, game) {
            out.extend(game.ui_tree.menu(super::wait_menu::SLEEP_WAIT_MENU));
        }
        out
    }

    /// T15b: act on a click on the Wait / Sleep menu's tiles: the hand-built menu's command
    /// for the piece (`super::wait_menu::slider_command`), unless a key made it that the
    /// hand-built menu took this frame. Returns the hours the slider then shows, for its
    /// value to be written again (`super::wait_menu::write_slider`).
    fn follow_wait(&mut self, ui: &fnv_ui::Ui, c: &fnv_ui::input::Click, at: Option<(f64, f64)>, held: bool, physical: &[&str]) -> Option<Option<u32>> {
        use super::wait_menu::{hours_at, slider_command, slider_left, wait_tile, WaitTile, SLEEP_WAIT_MENU};
        let model = self.wait.as_ref().and_then(|w| w.0.clone())?;
        let what = wait_tile(ui, c.tile)?;
        if let fnv_ui::input::By::Key(k) = &c.by {
            if !super::menus::key_click_acts(SLEEP_WAIT_MENU, k, physical) {
                return Some(None);
            }
        }
        let mut under = None;
        if what == WaitTile::Slider {
            *self.wait_dragging = held && matches!(c.by, fnv_ui::input::By::Pointer);
            *self.wait_left = slider_left(ui);
            under = at.zip(*self.wait_left).and_then(|((x, _), left)| hours_at(ui, left, x));
        }
        let Some(cmd) = slider_command(what, under) else { return Some(None) };
        info!("tiles: {SLEEP_WAIT_MENU} {cmd:?} on its tiles");
        self.wait_commands.write(crate::wait::WaitInput(cmd));
        let mut next = model.clone();
        next.apply(cmd);
        Some(Some(next.marker()))
    }

    /// T15b: act on a click on the tutorial menu's tiles: the hand-built menu's command for
    /// the piece, unless a key made it that the hand-built menu took this frame. `tiled` is
    /// the menu while it is drawn from tiles (`super::tutorial_menu::tiled_menu`, the drawing's
    /// own decision). `None` when the click is not for it, so the other menus' rules see it.
    fn follow_tutorial(&mut self, ui: &fnv_ui::Ui, c: &fnv_ui::input::Click, at: Option<(f64, f64)>, held: bool, physical: &[&str], tiled: Option<fnv_ui::TileId>) -> Option<()> {
        use super::tutorial_menu::{clicked, marker_grab, tile_command, TutorialTile, TUTORIAL_MENU};
        let what = clicked(ui, c.tile, tiled)?;
        if let fnv_ui::input::By::Key(k) = &c.by {
            if !super::menus::key_click_acts(TUTORIAL_MENU, k, physical) {
                return Some(());
            }
        }
        if what == TutorialTile::Marker {
            *self.tutorial_drag = at.filter(|_| held && matches!(c.by, fnv_ui::input::By::Pointer)).and_then(|(_, y)| marker_grab(ui, y));
            return Some(());
        }
        if let Some(cmd) = tile_command(ui, what) {
            info!("tiles: {TUTORIAL_MENU} {cmd:?} on its tiles");
            self.tutorial_commands.write(crate::tutorial_menu::TutorialInput(cmd));
        }
        Some(())
    }

    /// T15b: a drag on the tutorial menu's scroll marker: the scroll under the pointer, while
    /// `feed` keeps it ([`super::tutorial_menu::drag_holds`]).
    fn drag_tutorial(&mut self, ui: &fnv_ui::Ui, at: Option<(f64, f64)>) {
        use super::tutorial_menu::scroll_under;
        if let (Some(grab), Some((_, y))) = (*self.tutorial_drag, at) {
            if let Some(v) = scroll_under(ui, grab, y) {
                self.tutorial_commands.write(crate::tutorial_menu::TutorialInput(crate::tutorial_menu::Command::ScrollTo(v as f32)));
            }
        }
    }

    /// [design] Act on a click on the vendor's repair menu's tiles (see the module doc). `None` when
    /// the click is not for it. `row_acted`: as [`Self::follow_item_menu`]'s.
    fn follow_repair_services(&mut self, ui: &fnv_ui::Ui, c: &fnv_ui::input::Click, at: Option<(f64, f64)>, inputs: Pressed<'_>, row_acted: &mut bool) -> Option<()> {
        use super::repair_services_menu::{row_input, RepairServicesTile, REPAIR_SERVICES_MENU};
        use crate::repair::RepairServicesInput as Ask;
        use fnv_ui::input::By;
        let st = self.repair_services.as_mut()?;
        let what = st.clicked(ui, c.tile)?;
        if let By::Key(k) = &c.by {
            if !super::menus::key_click_acts(REPAIR_SERVICES_MENU, k, inputs.physical) {
                return Some(());
            }
        }
        let ask = match what {
            RepairServicesTile::Row(index) => {
                let at = if *row_acted { None } else { st.row_at(index) };
                let Some(index) = at else {
                    info!("tiles: {REPAIR_SERVICES_MENU} a click on a row no longer there is ignored");
                    return Some(());
                };
                *row_acted = true;
                row_input(st.selected(), index)
            }
            RepairServicesTile::Repair => Ask::Repair,
            RepairServicesTile::RepairAll => Ask::All,
            RepairServicesTile::Done => Ask::Done,
            RepairServicesTile::Bar(list::BarPiece::Marker) => {
                let menu = st.menu;
                let list_tile = st.list_tile(ui);
                st.drag = at.filter(|_| inputs.held && matches!(c.by, By::Pointer)).zip(menu).zip(list_tile).and_then(|(((_, y), menu), l)| list::marker_grab(ui, menu, l, y));
                return Some(());
            }
            RepairServicesTile::Bar(piece) => {
                let value = st.value();
                if let Some(v) = st.list_tile(ui).and_then(|l| list::after_piece(ui, l, piece, value)) {
                    info!("tiles: {REPAIR_SERVICES_MENU} {piece:?} on its tiles");
                    st.scroll_to(v);
                }
                return Some(());
            }
            RepairServicesTile::Other => return Some(()),
        };
        // A repair is of the row the tree shows selected, whatever a key did this frame, and no
        // pointer step or other row click after it this frame chooses another.
        let selected = st.selected();
        if ask == Ask::Repair {
            *row_acted = true;
        }
        // The owned finger envelope must reach its late guard before any legacy
        // painted-row Select can overwrite a newer actual selection.
        if self.normal_service_command(ui,c,crate::menu::normal_services::Action::Vendor(ask)){return Some(())}
        if let (Ask::Repair, Some(i)) = (ask, selected) {
            self.repair_commands.write(Ask::Select(i));
        }
        info!("tiles: {REPAIR_SERVICES_MENU} {ask:?} on its tiles");
        self.repair_commands.write(ask);
        Some(())
    }

    /// The vendor's repair menu's pointer besides clicks: a drag on the scroll marker, the row it
    /// came onto.
    fn follow_repair_services_pointer(&mut self, ui: &fnv_ui::Ui, report: &fnv_ui::input::Report, at: Option<(f64, f64)>, moved: bool, row_acted: bool) {
        use super::repair_services_menu::{RepairServicesTile, REPAIR_SERVICES_MENU};
        use crate::repair::RepairServicesInput as Ask;
        let holds = self.hold.as_ref().is_some_and(|h| h.held());
        let Some(st) = self.repair_services.as_mut() else { return };
        if let (Some(drag), Some((_, y))) = (st.drag, at) {
            if let Some(v) = list::value_under(ui, drag, y) {
                st.scroll_to(v);
            }
        }
        let Some((_, Some(to))) = report.over else { return };
        let Some(RepairServicesTile::Row(index)) = st.clicked(ui, to) else { return };
        let already = st.selected() == Some(index);
        if !row_acted && super::container_menu::hover_selects(moved, holds, already) && st.row_at(index).is_some() {
            let ask = Ask::Hover(index);
            info!("tiles: {REPAIR_SERVICES_MENU} {ask:?} on its tiles");
            self.repair_commands.write(ask);
        }
    }

    /// [design] Act on a click on the container or barter menu's tiles (see the module doc). `None` when
    /// the click is not for it, so the other menus' rules see it. `row_acted` is set by the
    /// row click that acts: the commands are applied later in the frame, so a second row
    /// click would be on rows the first may change.
    fn follow_item_menu(&mut self, ui: &fnv_ui::Ui, c: &fnv_ui::input::Click, at: Option<(f64, f64)>, inputs: Pressed<'_>, row_acted: &mut bool) -> Option<()> {
        use super::container_menu::{row_commands_with, ContainerTile};
        use crate::menu::{model::Command, MenuInput};
        use fnv_ui::input::By;
        let st = if self.container.menu.is_some() { &mut *self.container } else { &mut self.barter.as_mut()?.view };
        let menu_name = st.name();
        let what = st.clicked(ui, c.tile)?;
        if let By::Key(k) = &c.by {
            if !super::menus::key_click_acts(menu_name, k, inputs.physical) {
                return Some(());
            }
        }
        let m = self.active.as_ref().and_then(|a| a.0.as_ref())?;
        if m.quantity.is_some() {
            return Some(());
        }
        let ctrl_single = m.qtt_select_short_keys && inputs.ctrl;
        let mut guard = None;
        let commands = match what {
            ContainerTile::Row(side, index) => {
                let at = if *row_acted { None } else { st.row_at(m, side, index) };
                let Some(index) = at else {
                    info!("tiles: {menu_name} a click on a row no longer there is ignored");
                    return Some(());
                };
                *row_acted = true;
                let on_it = m.focus == side && m.selected[side as usize] == index;
                let commands = row_commands_with(side, index, on_it, inputs.shift, ctrl_single);
                guard = st.guard(m.mode, side, index, on_it, commands.len() as u8);
                commands
            }
            ContainerTile::Arrow(side, by) => vec![Command::Category(side, by)],
            ContainerTile::Title(side) => vec![Command::Category(side, 1)],
            ContainerTile::TakeAll => vec![Command::TakeAll],
            ContainerTile::Accept => vec![Command::Accept],
            ContainerTile::Exit => vec![Command::Close],
            ContainerTile::Bar(side, list::BarPiece::Marker) => {
                let menu = st.menu;
                let list_tile = st.list_tile(ui, side);
                st.drag = at.filter(|_| inputs.held && matches!(c.by, By::Pointer)).zip(menu).zip(list_tile).and_then(|(((_, y), menu), l)| list::marker_grab(ui, menu, l, y));
                return Some(());
            }
            ContainerTile::Bar(side, piece) => {
                let value = st.value(side);
                if let Some(v) = st.list_tile(ui, side).and_then(|l| list::after_piece(ui, l, piece, value)) {
                    info!("tiles: {menu_name} {piece:?} on its tiles");
                    st.scroll_to(side, v);
                }
                return Some(());
            }
            ContainerTile::Focus(_) | ContainerTile::Other => return Some(()),
        };
        if self.normal_pointer_commands(ui,c,&commands){return Some(())}
        if let Some(guard) = guard {
            self.commands.write(MenuInput(Command::Expect(guard)));
        }
        for cmd in commands {
            info!("tiles: {menu_name} {cmd:?} on its tiles");
            self.commands.write(MenuInput(cmd));
        }
        Some(())
    }

    /// [design] Act on a click on the recipe menu's tiles (see the module doc). `None` when the
    /// click is not for it. `row_acted`: as [`Self::follow_item_menu`]'s.
    fn follow_recipe(&mut self, ui: &fnv_ui::Ui, c: &fnv_ui::input::Click, at: Option<(f64, f64)>, inputs: Pressed<'_>, row_acted: &mut bool) -> Option<()> {
        use super::recipe_menu::{row_input, RecipeTile, RECIPE_MENU};
        use crate::crafting::RecipeInput;
        use fnv_ui::input::By;
        let asking = self.recipe_asking();
        let st = self.recipe.as_mut()?;
        let what = st.clicked(ui, c.tile)?;
        if let By::Key(k) = &c.by {
            if !super::menus::key_click_acts(RECIPE_MENU, k, inputs.physical) {
                return Some(());
            }
        }
        if asking {
            return Some(());
        }
        let input = match what {
            RecipeTile::Row(index) => {
                let at = if *row_acted { None } else { st.row_at(index) };
                let Some(index) = at else {
                    info!("tiles: {RECIPE_MENU} a click on a row no longer there is ignored");
                    return Some(());
                };
                *row_acted = true;
                row_input(st.selected(), index)
            }
            RecipeTile::Arrow(by) => RecipeInput::Filter(by),
            RecipeTile::Title => RecipeInput::Filter(1),
            RecipeTile::Accept => RecipeInput::Accept,
            RecipeTile::Exit => RecipeInput::Exit,
            RecipeTile::Bar(list_at, list::BarPiece::Marker) => {
                let menu = st.menu;
                let list_tile = st.list_tile(ui, list_at);
                st.drag = at.filter(|_| inputs.held && matches!(c.by, By::Pointer)).zip(menu).zip(list_tile).and_then(|(((_, y), menu), l)| list::marker_grab(ui, menu, l, y));
                return Some(());
            }
            RecipeTile::Bar(list_at, piece) => {
                let value = st.value(list_at);
                if let Some(v) = st.list_tile(ui, list_at).and_then(|l| list::after_piece(ui, l, piece, value)) {
                    info!("tiles: {RECIPE_MENU} {piece:?} on its tiles");
                    st.scroll_to(list_at, v);
                }
                return Some(());
            }
            RecipeTile::Other => return Some(()),
        };
        if self.normal_service_command(ui,c,crate::menu::normal_services::Action::Workbench(input)){return Some(())}
        info!("tiles: {RECIPE_MENU} {input:?} on its tiles");
        self.recipe_commands.write(input);
        Some(())
    }

    /// The recipe menu's pointer besides clicks: a drag on a scroll marker, the row it came onto.
    fn follow_recipe_pointer(&mut self, ui: &fnv_ui::Ui, report: &fnv_ui::input::Report, at: Option<(f64, f64)>, moved: bool, row_acted: bool) {
        use super::recipe_menu::{RecipeTile, RECIPE_MENU};
        use crate::crafting::RecipeInput;
        let holds = self.hold.as_ref().is_some_and(|h| h.held());
        let Some(st) = self.recipe.as_mut() else { return };
        if let (Some(drag), Some((_, y))) = (st.drag, at) {
            if let (Some(list_at), Some(v)) = (st.list_of(ui, drag.list), list::value_under(ui, drag, y)) {
                st.scroll_to(list_at, v);
            }
        }
        let Some((_, Some(to))) = report.over else { return };
        let Some(RecipeTile::Row(index)) = st.clicked(ui, to) else { return };
        let already = st.selected() == Some(index);
        if !row_acted && super::container_menu::hover_selects(moved, holds, already) && st.row_at(index).is_some() {
            let input = RecipeInput::Hover(index);
            info!("tiles: {RECIPE_MENU} {input:?} on its tiles");
            self.recipe_commands.write(input);
        }
    }

    /// T15c: the pointer's part besides clicks: a drag on a scroll marker, the row it came
    /// onto, the heading under it for the hand-built wheel.
    fn follow_item_menu_pointer(&mut self, ui: &fnv_ui::Ui, report: &fnv_ui::input::Report, over: Option<fnv_ui::TileId>, at: Option<(f64, f64)>, moved: bool, row_acted: bool) {
        use super::container_menu::{hover_selects, ContainerTile};
        use crate::menu::{model::Command, MenuInput};
        let st = if self.container.menu.is_some() { &mut *self.container } else {
            let Some(b) = self.barter.as_mut() else { return };
            &mut b.view
        };
        if let (Some(drag), Some((_, y))) = (st.drag, at) {
            let side = [Side::Container, Side::Player].into_iter().find(|&s| st.list_tile(ui, s) == Some(drag.list));
            if let (Some(side), Some(v)) = (side, list::value_under(ui, drag, y)) {
                st.scroll_to(side, v);
            }
        }
        self.wheel_title.0 = over.and_then(|t| st.clicked(ui, t)).and_then(|w| match w {
            ContainerTile::Title(side) | ContainerTile::Arrow(side, _) => Some(side),
            _ => None,
        });
        let Some((_, Some(to))) = report.over else { return };
        let Some(ContainerTile::Row(side, index)) = st.clicked(ui, to) else { return };
        let Some(m) = self.active.as_ref().and_then(|a| a.0.as_ref()).filter(|m| m.quantity.is_none()) else { return };
        let already = m.focus == side && m.selected[side as usize] == index;
        let holds = self.hold.as_ref().is_some_and(|h| h.held());
        if !row_acted && hover_selects(moved, holds, already) && st.row_at(m, side, index).is_some() {
            info!("tiles: {} Hover({side:?}, {index}) on its tiles", st.name());
            if let Some(guard) = st.guard(m.mode, side, index, false, 1) {
                self.commands.write(MenuInput(Command::Expect(guard)));
            }
            self.commands.write(MenuInput(Command::Hover(side, index)));
        }
    }

    /// Act on a frame's report (see the module doc): `at` the pointer, `over` the tile under
    /// it now, `held` the left button down, `shift` a Shift key down, `ctrl` a Control key
    /// down, `physical` the keys from the keyboard (or a controller standing in for them)
    /// this frame.
    #[allow(clippy::too_many_arguments)]
    fn follow(&mut self, ui: &fnv_ui::Ui, report: &fnv_ui::input::Report, at: Option<(f64, f64)>, over: Option<fnv_ui::TileId>, held: bool, shift: bool, ctrl: bool, physical: &[&str], tutorial: Option<fnv_ui::TileId>) -> Option<u32> {
        use super::menus::{chosen_row, key_click_acts, message_pick, nav_by_hand, quantity_tile, row_button, QuantityTile, QUANTITY_MENU};
        use crate::menu::{model::Command, MenuInput};
        use fnv_ui::input::By;
        if let (Some(i), Some(panel)) = (chosen_row(ui, report, nav_by_hand(physical)), self.panel.as_mut()) {
            if panel.framed() != Some(i) {
                panel.choose(i);
            }
        }
        let q = self.quantity();
        if q.is_none() || !held {
            *self.dragging = false;
        }
        let mut clicked = false;
        let mut slider = None;
        if self.wait.as_ref().is_none_or(|w| w.0.is_none()) || !held {
            *self.wait_dragging = false;
        }
        if !*self.wait_dragging {
            *self.wait_left = None;
        }
        let mut wait_clicked = false;
        self.drag_tutorial(ui, at);
        let mut row_acted = false;
        let moved = at != *self.last_at;
        *self.last_at = at;
        for c in report.clicks.iter().filter(|c| ui.is_live(c.tile)) {
            if self.follow_tutorial(ui, c, at, held, physical, tutorial).is_some() {
                continue;
            }
            if self.traits.as_mut().filter(|t| t.menu.is_some()).is_some_and(|t| super::trait_menu::follow_click(t, ui, c, at, held, physical).is_some()) {
                continue;
            }
            if self.follow_repair_services(ui, c, at, Pressed { held, shift, ctrl, physical }, &mut row_acted).is_some() {
                continue;
            }
            if self.tags.as_mut().filter(|t| t.menu.is_some()).is_some_and(|t| super::selection_menu::follow_click(t, ui, c, at, held, physical).is_some()) {
                continue;
            }
            if self.follow_item_menu(ui, c, at, Pressed { held, shift, ctrl, physical }, &mut row_acted).is_some() {
                continue;
            }
            if self.follow_recipe(ui, c, at, Pressed { held, shift, ctrl, physical }, &mut row_acted).is_some() {
                continue;
            }
            if let Some(hours) = self.follow_wait(ui, c, at, held, physical) {
                wait_clicked = true;
                slider = hours.or(slider);
                continue;
            }
            if row_button(ui, c.tile).is_some() {
                if let Some(i) = message_pick(ui, c, physical) {
                    info!("tiles: {} button {i} picked on its tiles", super::menus::MESSAGE_MENU);
                    self.click.0 = Some(i);
                }
                continue;
            }
            if let By::Key(k) = &c.by {
                if !key_click_acts(QUANTITY_MENU, k, physical) {
                    continue;
                }
            }
            let Some(q) = q else { continue };
            let cmd = match quantity_tile(ui, c.tile) {
                Some(QuantityTile::Ok) => Command::AmountOk,
                Some(QuantityTile::Cancel) => Command::AmountCancel,
                Some(QuantityTile::Decrease) => Command::AmountStep(-1),
                Some(QuantityTile::Increase) => Command::AmountStep(1),
                Some(QuantityTile::Meter) => {
                    *self.dragging = held && matches!(c.by, By::Pointer);
                    match amount_under(ui, at, q.max) {
                        Some(n) => Command::Amount(n),
                        None => continue,
                    }
                }
                None => continue,
            };
            clicked = true;
            info!("tiles: {} {cmd:?} on its tiles", super::menus::QUANTITY_MENU);
            if !self.normal_pointer_commands(ui,c,&[cmd]) {
                self.commands.write(MenuInput(cmd));
            }
        }
        // A drag: the amount under the pointer while the press holds.
        if let (true, false, Some(q)) = (*self.dragging, clicked, q) {
            if let Some(n) = amount_under(ui, at, q.max).filter(|&n| n != q.amount) {
                self.commands.write(MenuInput(Command::Amount(n)));
            }
        }
        // T15b: a drag on the Wait menu's slider, the hours under the pointer.
        let hours = self.wait.as_ref().and_then(|w| w.0.as_ref()).map(|m| m.hours);
        if let (true, false, Some(now), Some((x, _)), Some(left)) = (*self.wait_dragging, wait_clicked, hours, at, *self.wait_left) {
            if let Some(h) = super::wait_menu::hours_at(ui, left, x).filter(|&h| h != now) {
                self.wait_commands.write(crate::wait::WaitInput(crate::wait::model::Command::Hours(h)));
                slider = Some(h);
            }
        }
        if self.container.menu.is_some() || self.barter.as_ref().is_some_and(|b| b.menu().is_some()) {
            self.follow_item_menu_pointer(ui, report, over, at, moved, row_acted);
        }
        if self.recipe_takes().is_some() {
            self.follow_recipe_pointer(ui, report, at, moved, row_acted);
        }
        if let Some(t) = self.traits.as_mut().filter(|t| t.takes().is_some()) {
            super::trait_menu::follow_pointer(t, ui, report, at, moved);
        }
        if self.repair_services.as_ref().is_some_and(|r| r.menu.is_some()) {
            self.follow_repair_services_pointer(ui, report, at, moved, row_acted);
        }
        if let Some(t) = self.tags.as_mut().filter(|t| t.takes().is_some()) {
            super::selection_menu::follow_pointer(t, ui, report, at, moved);
        }
        slider
    }
}

/// What a frame's buttons and keys are, for a click's rules.
#[derive(Clone, Copy)]
struct Pressed<'a> {
    held: bool,
    shift: bool,
    ctrl: bool,
    physical: &'a [&'a str],
}

/// T15b: the hours the Wait menu's slider was written for this frame (`feed`), and the
/// value last seen drawn.
#[derive(Resource, Default)]
pub(crate) struct WaitSliderCheck {
    wrote: Option<u32>,
    drawn: Option<f64>,
}

/// T15b: the slider's value once the tree is worked out after scripts (what is drawn),
/// logged when it changes, and when it is not the value written this frame for the model's
/// hours (smoke reads both: the model's value must outlast a click's pulse, #1307 review).
pub(crate) fn check_wait_drawn(ui: &fnv_ui::Ui, check: &mut WaitSliderCheck) {
    use super::wait_menu::{slider_shown, slider_value, SLEEP_WAIT_MENU};
    let Some(h) = check.wrote.take() else { return };
    let Some(v) = slider_shown(ui) else { return };
    if check.drawn != Some(v) {
        info!("tiles: {SLEEP_WAIT_MENU} slider drawn at {}", fnv_ui::Value::Num(v).text());
        check.drawn = Some(v);
    }
    if slider_value(h) != v {
        info!("tiles: {SLEEP_WAIT_MENU} slider drawn at {}, not the {} written", fnv_ui::Value::Num(v).text(), slider_value(h));
    }
}

/// The amount under the pointer on the prompt's meter: the part of the meter's length left
/// of it, of the stack (`crate::menu::model::amount_at`, as the hand-built meter) [design:
/// the file's own drag value reads a `dragx` whose units are not known].
fn amount_under(ui: &fnv_ui::Ui, at: Option<(f64, f64)>, most: i32) -> Option<i32> {
    let (x, _) = at?;
    let b = super::menus::meter_box(ui).filter(|b| b.width > 0.0)?;
    Some(crate::menu::model::amount_at(((x - b.left) / b.width) as f32, most))
}

/// Whether a controller button that went down stood in for a key the hand-built handlers
/// saw this frame, in the menu `at` (`crate::pad::menu_key_at`).
fn stood_in(button: GamepadButton, at: crate::pad::MenuAt, just_pressed: &dyn Fn(KeyCode) -> bool) -> bool {
    crate::pad::menu_key_at(button, at).is_some_and(just_pressed)
}

/// The frame's keys into `input`: the keyboard's, a script's MenuTapKey and the controller's.
/// Returns the keys from the keyboard and the controller buttons that stood in for a key this
/// frame in the menu `pad_menu` (`crate::pad::menu_key`): what the hand-built handlers saw too.
pub(super) fn frame_keys(input: &mut Input, devices: &Devices, game: &mut fnv_game::Game<'_>, pad_menu: crate::pad::MenuAt) -> Vec<&'static str> {
    let mut physical: Vec<&'static str> = Vec::new();
    for k in devices.keys.get_just_pressed() {
        if let Some(n) = key_name(*k) {
            push(input, n);
            physical.push(n.1);
        }
    }
    for code in std::mem::take(&mut game.ui_taps) {
        if let Some(n) = u8::try_from(code).ok().and_then(crate::bindings::dik_key).and_then(key_name) {
            push(input, n);
        }
    }
    for (button, name) in PAD {
        if devices.pads.iter().any(|g| g.just_pressed(button)) {
            push(input, (false, name));
            if stood_in(button, pad_menu, &|k| devices.keys.just_pressed(k)) {
                physical.push(name);
            }
        }
    }
    physical
}

/// A step's log lines: what the pointer is over, each click, each key that named nothing.
pub(super) fn log_report(ui: &fnv_ui::Ui, report: &fnv_ui::input::Report) {
    if let Some((_, to)) = report.over {
        match to.filter(|&t| ui.is_live(t)) {
            Some(t) => {
                let id = ui.tile(t).get("id").map_or_else(|| "no id".to_string(), |v| format!("id {}", v.text()));
                let over = ui.tile(t).get(fnv_ui::input::MOUSEOVER).map_or(0.0, fnv_ui::Value::num);
                info!("tiles: pointer over {} ({id}), mouseover {over}", ui.path_of(t));
            }
            None => info!("tiles: pointer over nothing"),
        }
    }
    for c in &report.clicks {
        if ui.is_live(c.tile) {
            let clicked = ui.tile(c.tile).get(fnv_ui::input::CLICKED).map_or(0.0, fnv_ui::Value::num);
            info!("tiles: {}, clicked {clicked}", fnv_game::ui_input::click_line(ui, c));
        }
    }
    for line in &report.unfollowed {
        info!("tiles: key not followed: {line}");
    }
}

/// What else says who takes the frame's input: the pause menu and its page, and the Pip-Boy's
/// pages drawn from tiles (`super::stats_menu`, `super::inventory_menu`).
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct Others<'w> {
    paused: Option<Res<'w, crate::pause::Paused>>,
    page: Option<Res<'w, crate::pause::Page>>,
    stats: Option<ResMut<'w, super::stats_menu::StatsTiles>>,
    inventory: Option<ResMut<'w, super::inventory_menu::InventoryTiles>>,
    map: Option<ResMut<'w, super::map_menu::MapTiles>>,
    repair: Option<Res<'w, crate::pipboy::repair::PipRepair>>,
    modding: Option<Res<'w, crate::pipboy::item_mod::PipItemMod>>,
    /// The weapon mod menu's decision (`super::item_mod_menu`).
    item_mod: Option<ResMut<'w, super::item_mod_menu::ItemModTiles>>,
    pip: Option<Res<'w, crate::pipboy::PipBoy>>,
}

pub(crate) fn feed(
    mut script: ResMut<ScriptState>,
    menus: Res<super::vanilla::TileMenus>,
    aid: Option<ResMut<PointerAid>>,
    time: crate::pause::AidTime,
    devices: Devices,
    mut inputs: MenuInputs,
    others: Others,
) {
    let Others { paused, page, mut stats, mut inventory, mut map, repair, modding, mut item_mod, pip } = others;
    *inputs.normal_event=None;
    *inputs.normal_emitted=false;
    *inputs.service_event=None;*inputs.service_emitted=false;
    let Some(rt) = script.rt.as_mut() else { return };
    if rt.game.ui_files.is_none() {
        // Still end script clicks (ClickMenuTile / ClickMenuButton on injected tiles) and
        // fire SetOnMenuClickEventHandler even before menu XML files are loaded [design].
        if rt.game.ui_pointer.over.is_some() || rt.game.ui_pointer.pending() {
            fnv_game::ui_input::idle(&mut rt.game);
        }
        return;
    }
    if let Some(v) = inputs.view_input.as_deref_mut() {
        *v = Default::default();
    }
    // The test aid's pointer, moved on its clock.
    let mut aid_click = false;
    let mut aid_at = None;
    if let Some(mut aid) = aid {
        let now = time.elapsed_secs();
        let mut fired: Vec<(f32, AidPointer)> = aid.due.iter().copied().filter(|(t, _)| *t <= now).collect();
        fired.sort_by(|a, b| a.0.total_cmp(&b.0));
        aid.due.retain(|(t, _)| *t > now);
        for (_, item) in fired {
            match item {
                AidPointer::At { x, y, click } => {
                    info!("test aid: tile pointer at {x} {y}{}", if click { ", click" } else { "" });
                    aid.now = Some((x, y));
                    aid_click |= click;
                }
                AidPointer::Off => {
                    info!("test aid: tile pointer off");
                    aid.now = None;
                }
            }
        }
        aid_at = aid.now;
    }
    // The menus that take input, back to front (see the module doc): the HUD only under
    // the test aid, or with a script's HUD cursor (ToggleHUDCursor, `fnv_game::nvse_jip9`).
    let mut taking = Vec::new();
    let hud_cursor = rt.game.other.jip9.hud_cursor;
    // T11: the pause menu's Mods page takes the pointer and keys for the start menu's
    // tiles (`super::mods`); the test aid's pointer is then the page's, not the HUD's.
    let mods_page = menus.start && paused.as_ref().zip(page.as_ref()).is_some_and(|(p, pg)| crate::pause::mods_open(p, pg));
    if (aid_at.is_some() || hud_cursor) && menus.hud && !mods_page {
        taking.extend(rt.game.ui_tree.menu(super::draw::HUD_MENU));
    }
    // The dialogue menu, under anything over the conversation (it does not take with one up).
    let dialog_menu = inputs.dialog.as_ref().and_then(|d| d.taking(&rt.game, paused.as_ref().is_some_and(|p| p.0)));
    taking.extend(dialog_menu);
    taking.extend(inputs.taking(*menus, &rt.game, inventory.as_ref().is_some_and(|i| i.ready.is_some())));
    if mods_page {
        taking.extend(rt.game.ui_tree.menu(super::mods::START_MENU));
    }
    // MCM reads a click in two frames running (`fnv_ui::input::Pointer::click_steps`).
    rt.game.ui_pointer.click_steps = if mods_page { 2 } else { 1 };
    // T15b: a drag on the tutorial menu's marker ends with its press or that menu, whether or
    // not any menu takes input this frame.
    let tutorial = super::tutorial_menu::tiled_menu(menus.tutorial, &rt.game);
    let held = aid_click || devices.mouse.pressed(MouseButton::Left);
    *inputs.tutorial_drag = inputs.tutorial_drag.filter(|d| super::tutorial_menu::drag_holds(d, tutorial, held));
    // T15c: a drag on a scroll marker ends with its press, that menu or under a prompt,
    // whether or not any menu takes input this frame; so does the heading the wheel reads.
    let container_now = super::container_menu::takes_input(inputs.container.menu, inputs.quantity().is_some());
    inputs.container.drag = inputs.container.drag.filter(|d| list::drag_holds(d, container_now, held));
    let barter_now = super::container_menu::takes_input(inputs.barter.as_ref().and_then(|b| b.menu()), inputs.quantity().is_some());
    if let Some(b) = inputs.barter.as_mut() {
        b.view.drag = b.view.drag.filter(|d| list::drag_holds(d, barter_now, held));
    }
    if let Some(r) = inputs.repair_services.as_mut() {
        let now = r.menu;
        r.drag = r.drag.filter(|d| list::drag_holds(d, now, held));
    }
    if container_now.is_none() && barter_now.is_none() {
        inputs.wheel_title.0 = None;
    }
    let recipe_now = inputs.recipe_takes();
    if let Some(r) = inputs.recipe.as_mut() {
        r.drag = r.drag.filter(|d| list::drag_holds(d, recipe_now, held));
    }
    if let Some(t) = inputs.traits.as_mut().filter(|t| t.menu.is_some()) {
        t.hold_drag(held);
    }
    if let Some(t) = inputs.tags.as_mut().filter(|t| t.menu.is_some()) { t.hold_drag(held); }
    // The pointer on the menus as drawn at the UI scale (#1631): the dialogue menu keeps to
    // the view's bottom when it takes input alone, the others to its middle (`super::draw`).
    // The test aid's place is in the tree's units already and is read as given, through no
    // frame: so the HUD, which takes input only under the aid and is drawn in none, is hit
    // where its tiles stand.
    let anchor = super::draw::menu_anchor(dialog_menu.is_some() && taking.len() == 1);
    // The HUD alone under a script's cursor is hit where its tiles stand, in no frame.
    let hud_alone = hud_cursor && aid_at.is_none() && taking.len() == 1 && rt.game.ui_tree.menu(super::draw::HUD_MENU) == taking.first().copied();
    let sleeping = crate::pad::pointer::asleep(devices.pad_pointer.as_deref())
        || devices.touch_owner.as_ref().is_some_and(|owner| owner.pointer_suppressed());
    let cursor = || {
        aid_at.or_else(|| {
            devices.windows.single().ok().filter(|_| !sleeping).and_then(|w| {
                let p = w.cursor_position()?;
                Some(if hud_alone { to_units(p, w.height()) } else { to_menu_units(p, Vec2::new(w.width(), w.height()), anchor) })
            })
        })
    };
    let wait_tiled = super::wait_menu::tiled(menus.sleep_wait, &rt.game);
    inputs.wait_check.wrote = None;
    if !wait_tiled {
        inputs.wait_check.drawn = None;
    }
    let others = !taking.is_empty();
    if let Some(s) = stats.as_mut().filter(|s| s.others != others) {
        s.others = others;
    }
    if let Some(s) = inventory.as_mut().filter(|s| s.others != others) {
        s.others = others;
    }
    if let Some(r) = inputs.repair_page.as_mut().filter(|r| r.others != others) {
        r.others = others;
    }
    if let Some(m) = map.as_mut().filter(|m| m.others != others) {
        m.others = others;
    }
    if let Some(m) = item_mod.as_mut().filter(|m| m.others != others) {
        m.others = others;
    }
    if taking.is_empty() {
        // The Pip-Boy's page has the frame's step (`super::stats_menu::feed`).
        let is_paused = paused.as_ref().is_some_and(|p| p.0);
        if stats.as_deref().zip(pip.as_deref()).is_some_and(|(s, p)| super::stats_menu::takes(s, *menus, p, &rt.game, is_paused).is_some()) {
            return;
        }
        // Likewise the ITEMS page's (`super::inventory_menu::feed`).
        let lists_closed = super::inventory_menu::lists_closed(repair.as_deref(), modding.as_deref());
        if inventory.as_deref().zip(pip.as_deref()).is_some_and(|(s, p)| super::inventory_menu::takes(s, *menus, p, &rt.game, is_paused, lists_closed, inputs.quantity().is_some()).is_some()) {
            return;
        }
        // Likewise the DATA page's (`super::map_menu::feed`).
        if map.as_deref().zip(pip.as_deref()).is_some_and(|(m, p)| super::map_menu::takes(m, *menus, p, &rt.game, is_paused).is_some()) {
            return;
        }
        // Likewise the repair page's (`super::repair_menu::feed`).
        let list_open = inputs.repair_list.as_ref().is_some_and(|r| r.is_open());
        if inputs.repair_page.as_deref().zip(pip.as_deref()).is_some_and(|(r, p)| super::repair_menu::takes(r, *menus, p, list_open, &rt.game, is_paused).is_some()) {
            return;
        }
        // Likewise the weapon mod menu's (`super::item_mod_menu::feed`).
        let mod_open = modding.as_ref().is_some_and(|m| m.is_open());
        if item_mod.as_deref().zip(pip.as_deref()).is_some_and(|(m, p)| super::item_mod_menu::takes(m, *menus, p, mod_open, &rt.game, is_paused).is_some()) {
            return;
        }
        // A highlight left from the last menu that took input goes (mouseover back to 0),
        // and the scripts' clicks end and fire their events; otherwise nothing to do
        // [design: `fnv_game::ui_input::idle`].
        if fnv_game::ui_input::idle(&mut rt.game).is_some_and(|r| r.over.is_some()) {
            info!("tiles: pointer over nothing (no menu takes input)");
        }
        // Clear any stale tile-tree pointer position while no tile menu owns input so
        // GetCursorPos XML queries in hand-built menus fall back to the live window
        // cursor (`jip9.cursor_px`) [design].
        rt.game.ui_pointer.at = None;
        // A click's pulse ended there too: the scroll is the view's again.
        if inputs.container.menu.is_some() {
            inputs.container.rewrite(&mut rt.game.ui_tree);
        }
        if let Some(b) = inputs.barter.as_ref().filter(|b| b.menu().is_some()) {
            b.view.rewrite(&mut rt.game.ui_tree);
        }
        if let Some(r) = inputs.recipe.as_ref().filter(|r| r.menu.is_some()) {
            r.rewrite(&mut rt.game.ui_tree);
        }
        if let Some(r) = inputs.repair_services.as_ref().filter(|r| r.menu.is_some()) {
            r.rewrite(&mut rt.game.ui_tree);
        }
        if let Some(t) = inputs.traits.as_ref().filter(|t| t.menu.is_some()) {
            t.rewrite(&mut rt.game.ui_tree);
        }
        if let Some(t) = inputs.tags.as_ref().filter(|t| t.menu.is_some()) { t.rewrite(&mut rt.game.ui_tree); }
        // Where the pointer rests when a menu takes input again is no move onto a row.
        *inputs.last_at = cursor();
        return;
    }
    // The aid's pointer, else the cursor in the window.
    let native=inputs.normal.as_mut().and_then(|state|state.take_native());
    if let Some(n)=native {
        let valid=aid_at.is_none() && !aid_click
            && inputs.active.as_ref().and_then(|a|a.0.as_ref()).is_some_and(|m|
            inputs.normal_gate.allows(n,m,&rt.game)
                && (if m.quantity.is_some() && super::menus::normal_touch_quantity_shown(&rt.game.ui_tree) {
                    super::menus::normal_touch_quantity_hit(&rt.game.ui_tree,n.point,n.frame.size)
                } else {
                    inputs.barter.as_ref().and_then(|tiles|
                        tiles.normal_touch_hit(&rt.game.ui_tree,m,n.point,n.frame.size))
                })
                .is_some_and(|h|h.source==n.source && crate::menu::normal_touch::same_target(h.target,n.target)))
            && matches!(n.source,crate::menu::normal_touch::Source::Tile(menu,_) if taking.contains(&menu));
        if valid {*inputs.normal_event=Some(n);}
        else if let Some(state)=inputs.normal.as_mut(){state.finish_native(n.token,false);}
    }
    let service=inputs.services.as_mut().and_then(|state|state.take_native());
    if let Some(n)=service {
        let source=match n.kind {
            crate::menu::normal_services::Kind::Vendor=>inputs.repair_services.as_ref().and_then(|tiles|
                tiles.normal_hit(&rt.game.ui_tree,n.frame,inputs.services.as_ref()?.native_guard(n.token)?,n.point,n.text_size)),
            crate::menu::normal_services::Kind::Workbench=>inputs.recipe.as_ref().and_then(|tiles|
                tiles.normal_hit(&rt.game.ui_tree,n.frame,inputs.services.as_ref()?.native_guard(n.token)?,n.point,n.text_size)),
        };
        let valid=inputs.normal_event.is_none() && aid_at.is_none() && !aid_click
            && inputs.service_gate.allows(n,&rt.game)
            && source.is_some_and(|h|h.source==n.source && h.action==n.action)
            && matches!(n.source,crate::menu::normal_touch::Source::Tile(menu,_) if taking.contains(&menu));
        if valid {*inputs.service_event=Some(n);}
        else if let Some(state)=inputs.services.as_mut(){state.finish_native(n.token,false);}
    }
    let service_at=(*inputs.service_event).map(|n|to_menu_units(n.point,n.frame.size,anchor));
    let at = (*inputs.normal_event).map(|n|to_menu_units(n.point,n.frame.size,anchor)).or(service_at).or_else(cursor);
    let mut input = Input {
        at,
        press: inputs.normal_event.is_some() || inputs.service_event.is_some() || aid_click || devices.mouse.just_pressed(MouseButton::Left),
        shift: inputs.normal_event.is_none() && inputs.service_event.is_none() && (devices.keys.pressed(KeyCode::ShiftLeft) || devices.keys.pressed(KeyCode::ShiftRight)),
        ..Input::default()
    };
    // The tutorial menu's triggers stand for its own keys (`crate::pad::menu_buttons`), which
    // a press of them would otherwise turn a page with twice: here and through the file.
    // [design] X stands for R in the container and A in barter (the same table).
    let pad_menu = crate::pad::MenuAt { code: tutorial.map(|_| fnv_game::game::menus::TUTORIAL).or(inputs.container.menu.map(|_| crate::script::MENU_CONTAINER)).or(inputs.barter.as_ref().and_then(|b| b.menu()).map(|_| crate::script::MENU_BARTER)).or(inputs.recipe.as_ref().and_then(|r| r.menu).map(|_| fnv_game::crafting::RECIPE_MENU)).or(inputs.traits.as_ref().and_then(|t| t.takes()).map(|_| fnv_game::game::menus::TRAITS)).or(inputs.tags.as_ref().and_then(|t| t.takes()).map(|_| fnv_game::game::menus::TAG_SKILLS)).or(inventory.as_ref().and_then(|i| i.ready).map(|_| fnv_game::game::menus::INVENTORY)), tab: 0 };
    let pad_menu = crate::pad::MenuAt { code: pad_menu.code.or(inputs.repair_services.as_ref().and_then(|r| r.menu).map(|_| fnv_game::repair::REPAIR_SERVICES_MENU)), ..pad_menu };
    let physical = frame_keys(&mut input, &devices, &mut rt.game, pad_menu);
    // The dialogue menu's pointer decides on what was drawn (`super::dialog_menu`).
    if let (Some(_), [_], Some(d)) = (dialog_menu, taking.as_slice(), inputs.dialog.as_ref()) {
        input.resolved = Some(d.hits().to_vec());
    }
    if let (Some(st), Some(at)) = (inputs.barter.as_ref(), at) {
        if let Some(candidates) = super::barter_menu::pointer_candidates(&mut rt.game.ui_tree, st, &taking, at) {
            input.resolved = Some(candidates);
        }
    }
    // The Mods page is drawn by the `locus` rule (`super::layout::layout_by_locus`): the
    // pointer is matched against the same places.
    if let (true, [menu]) = (mods_page, taking.as_slice()) {
        input.resolved = Some(fnv_ui::input::hit_candidates_by_locus(&rt.game.ui_tree, *menu, &no_measure));
    }
    // A frame's own clicks are for the screen up now; a script's kept theirs when queued.
    rt.game.ui_pointer.stamp = crate::dialogue::stamp_now(&rt.game);
    let report = fnv_game::ui_input::step(&mut rt.game, &taking, &input, &no_measure);
    let over = rt.game.ui_pointer.active(&rt.game.ui_tree);
    let ui = &rt.game.ui_tree;
    let ctrl = devices.keys.pressed(KeyCode::ControlLeft) || devices.keys.pressed(KeyCode::ControlRight);
    let slider = inputs.follow(ui, &report, at, over, held, input.shift, ctrl, &physical, tutorial);
    if let Some(n)=*inputs.normal_event {
        if let Some(state)=inputs.normal.as_mut() {
            state.finish_native(n.token,*inputs.normal_emitted);
        }
    }
    if let Some(n)=*inputs.service_event {
        if let Some(state)=inputs.services.as_mut() {
            state.finish_native(n.token,*inputs.service_emitted);
        }
    }
    if let (Some(menu), Some(v)) = (dialog_menu, inputs.view_input.as_deref_mut()) {
        let mut steps = 0;
        super::dialog_menu::follow(ui, &report, &physical, menu, inputs.dialog.as_ref().and_then(|d| d.list()), v, &mut steps);
        if let Some(shared) = inputs.dialog_written.as_ref() {
            shared.scroll(steps);
        }
    }
    log_report(ui, &report);
    // T15b: the slider's value is the model's again on every frame the menu is tiled, over
    // the file's own count of the clicks and their pulses' ends (`super::wait_menu`), after
    // this frame's step and before the tree is worked out after scripts: the hours a command
    // this frame leads to, else the model's.
    if wait_tiled {
        let model = inputs.wait.as_ref().and_then(|w| w.0.as_ref()).map(crate::wait::model::Model::marker);
        if let Some(h) = slider.or(model) {
            super::wait_menu::write_slider(&mut rt.game.ui_tree, h);
            inputs.wait_check.wrote = Some(h);
        }
    }
    // T15c: the scroll and highlight are the view's again, over the file's own count of the
    // clicks and their pulses' ends, with what the frame's bar clicks and drag did.
    if inputs.container.menu.is_some() {
        inputs.container.rewrite(&mut rt.game.ui_tree);
    }
    if let Some(b) = inputs.barter.as_ref().filter(|b| b.menu().is_some()) {
        b.view.rewrite(&mut rt.game.ui_tree);
    }
    if let Some(r) = inputs.recipe.as_ref().filter(|r| r.menu.is_some()) {
        r.rewrite(&mut rt.game.ui_tree);
    }
    if let Some(r) = inputs.repair_services.as_ref().filter(|r| r.menu.is_some()) {
        r.rewrite(&mut rt.game.ui_tree);
    }
    if let Some(t) = inputs.traits.as_ref().filter(|t| t.menu.is_some()) {
        t.rewrite(&mut rt.game.ui_tree);
    }
    if let Some(t) = inputs.tags.as_ref().filter(|t| t.menu.is_some()) { t.rewrite(&mut rt.game.ui_tree); }
}

/// Read the test aid at start.
pub(crate) fn setup(app: &mut App) {
    app.init_resource::<MessageClick>().init_resource::<WaitSliderCheck>();
    let Ok(v) = std::env::var(AID) else { return };
    match crate::test_aid::timed(&v).and_then(|items| items.into_iter().map(|(s, r)| aid_pointer(r).map(|p| (s, p))).collect::<Result<Vec<_>, _>>()) {
        Ok(due) => {
            app.insert_resource(PointerAid { due, now: None });
        }
        Err(e) => warn!("{AID}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aid_items() {
        assert_eq!(aid_pointer("120:340"), Ok(AidPointer::At { x: 120.0, y: 340.0, click: false }));
        assert_eq!(aid_pointer(" 1.5 : 2 : click "), Ok(AidPointer::At { x: 1.5, y: 2.0, click: true }));
        assert_eq!(aid_pointer("OFF"), Ok(AidPointer::Off));
        assert!(aid_pointer("12").is_err());
        assert!(aid_pointer("1:2:press").is_err());
        let items = crate::test_aid::timed("3:10:20,4:10:20:click,9:off").unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(aid_pointer(items[1].1), Ok(AidPointer::At { x: 10.0, y: 20.0, click: true }));
    }

    #[test]
    fn controller_take_all_acts_once() {
        // X stands for R in the container menu: the hand-built handler takes all, so a file's
        // `xbuttonx` would not do it again; a controller not working the controls presses no
        // key, and the file's binding is the one press.
        use super::super::container_menu::CONTAINER_MENU;
        use super::super::menus::key_click_acts;
        let at = crate::pad::MenuAt { code: Some(crate::script::MENU_CONTAINER), tab: 0 };
        let r_down = |k: KeyCode| k == KeyCode::KeyR;
        assert!(stood_in(GamepadButton::West, at, &r_down));
        assert!(!key_click_acts(CONTAINER_MENU, "xbuttonx", &["R", "xbuttonx"]));
        assert!(!stood_in(GamepadButton::West, at, &|_| false));
        // Elsewhere X stands for nothing.
        assert!(!stood_in(GamepadButton::West, crate::pad::MenuAt { code: None, tab: 0 }, &r_down));
    }

    #[test]
    fn controller_repair_all_acts_once() {
        // X stands for A in the vendor's repair menu: the model repairs all, so the file's
        // `xbuttonx` does not do it again.
        use super::super::menus::key_click_acts;
        use super::super::repair_services_menu::REPAIR_SERVICES_MENU;
        let at = crate::pad::at_menu(fnv_game::repair::REPAIR_SERVICES_MENU);
        assert!(stood_in(GamepadButton::West, at, &|key| key == KeyCode::KeyA));
        assert!(!key_click_acts(REPAIR_SERVICES_MENU, "xbuttonx", &["A", "xbuttonx"]));
        // B stands for Esc, which closes it.
        assert!(stood_in(GamepadButton::East, at, &|key| key == KeyCode::Escape));
        assert!(!key_click_acts(REPAIR_SERVICES_MENU, "xbuttonb", &["xbuttonb"]));
    }

    #[test]
    fn controller_barter_accept_acts_once() {
        use super::super::barter_menu::BARTER_MENU;
        use super::super::menus::key_click_acts;
        let at = crate::pad::MenuAt { code: Some(crate::script::MENU_BARTER), tab: 0 };
        assert!(stood_in(GamepadButton::West, at, &|key| key == KeyCode::KeyA));
        assert!(!key_click_acts(BARTER_MENU, "xbuttonx", &["A", "xbuttonx"]));
        assert!(!key_click_acts(BARTER_MENU, "A", &["A", "xbuttonx"]));
        assert!(key_click_acts(BARTER_MENU, "xbuttonx", &[]));
        assert!(key_click_acts(BARTER_MENU, "A", &[]));
    }

    #[test]
    fn tutorial_triggers_turn_one_page() {
        // RT stands for A in the tutorial menu: the hand-built handler turns the page, so
        // neither the file's `xbuttonrt` click nor its `_PCButton_A` click turns it again.
        use super::super::menus::key_click_acts;
        use super::super::tutorial_menu::TUTORIAL_MENU;
        use fnv_game::game::menus::TUTORIAL;
        let tutorial = crate::pad::MenuAt { code: Some(TUTORIAL), tab: 0 };
        let a_down = |k: KeyCode| k == KeyCode::KeyA;
        assert!(stood_in(GamepadButton::RightTrigger2, tutorial, &a_down));
        let physical = ["A", "xbuttonrt"];
        assert!(!key_click_acts(TUTORIAL_MENU, "xbuttonrt", &physical));
        assert!(!key_click_acts(TUTORIAL_MENU, "A", &physical));
        // LT the same, for S.
        assert!(stood_in(GamepadButton::LeftTrigger2, tutorial, &|k| k == KeyCode::KeyS));
        assert!(!key_click_acts(TUTORIAL_MENU, "xbuttonlt", &["S", "xbuttonlt"]));
        // Elsewhere the triggers keep the shared Page Up and Page Down.
        let elsewhere = crate::pad::MenuAt { code: None, tab: 0 };
        assert!(!stood_in(GamepadButton::RightTrigger2, elsewhere, &a_down));
        assert!(stood_in(GamepadButton::RightTrigger2, elsewhere, &|k| k == KeyCode::PageDown));
        // A controller not working the controls presses no key: the file's binding is then
        // the one press.
        assert!(!stood_in(GamepadButton::RightTrigger2, tutorial, &|_| false));
        assert!(key_click_acts(TUTORIAL_MENU, "xbuttonrt", &[]));
    }

    #[test]
    fn physical_and_script_keys_act_once() {
        use super::super::menus::key_click_acts;
        use super::super::tutorial_menu::TUTORIAL_MENU;
        // A keyboard's keys are the hand-built handler's, a script's MenuTapKey (not physical)
        // the file's.
        for key in ["A", "S", "E", "xup", "xdown", "xleft", "xright"] {
            assert!(!key_click_acts(TUTORIAL_MENU, key, &[key]), "{key} by hand");
            assert!(key_click_acts(TUTORIAL_MENU, key, &[]), "{key} tapped by a script");
        }
        // RB and LB page the text through the file: the hand-built menu has no use for them.
        assert!(key_click_acts(TUTORIAL_MENU, "xbuttonrb", &["xbuttonrb"]));
        assert!(key_click_acts(TUTORIAL_MENU, "xbuttonlb", &["xbuttonlb"]));
    }

    /// Over no plugins, the tutorial menu open at three pages, its file not built yet, with
    /// what the frame's own systems need: the tile input, the hand-built keys and the
    /// hand-built drawing, in their order in a frame (`crate::script`, `super`).
    fn tutorial_app() -> App {
        use crate::tutorial_menu::{keys, TutorialMenuPlugin};
        let mut state = crate::script::ScriptState::over_no_plugins();
        let game = &mut state.rt.as_mut().unwrap().game;
        game.ui_files = Some(Box::new(fnv_ui::MemSource::default()));
        game.menu = Some(fnv_game::game::menus::TUTORIAL);
        game.tutorial = vec![fnv_sim::FormId(0x11), fnv_sim::FormId(0x12), fnv_sim::FormId(0x13)];
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<crate::pause::PlayClock>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<crate::pad::PadGlyphs>()
            .init_resource::<MessageClick>()
            .init_resource::<WaitSliderCheck>()
            .init_resource::<PointerAid>()
            .init_resource::<super::super::container_menu::ContainerTiles>()
            .init_resource::<super::super::container_menu::TileWheelTitle>()
            .insert_resource(super::super::vanilla::TileMenus { tutorial: true, ..Default::default() })
            .insert_resource(state)
            .add_message::<bevy::input::mouse::MouseWheel>()
            .add_message::<crate::menu::MenuInput>()
            .add_message::<crate::wait::WaitInput>()
            .add_message::<crate::crafting::RecipeInput>()
            .add_message::<crate::repair::RepairServicesInput>()
            .add_plugins(TutorialMenuPlugin)
            .add_systems(Update, (feed, keys).chain().before(crate::PlaySet::Ui));
        app
    }

    /// The test layout (`super::super::tutorial_menu::tests`, synthetic) as a built menu, its
    /// Next and Close at their own places, as a mod may move them: 1070..1170 x 196..246 and
    /// 1070..1170 x 496..546, under its background at 50, 40.
    fn built_tutorial(app: &mut App) {
        use super::super::tutorial_menu::{tests::{menu_changed, shown}, write};
        let mut ui = menu_changed(|xml| {
            let place = |xml: String, name: &str, y: u32| xml.replace(&format!(r#"<hotrect name="{name}">"#), &format!(r#"<hotrect name="{name}"><x>1020</x><y>{y}</y><width>100</width><height>50</height>"#));
            place(place(xml.to_string(), "TM_next_button", 156), "TM_close_button", 456)
        });
        let s = shown(&ui, "Title", "Text", 0, 3);
        write(&mut ui, &s);
        ui.update();
        app.world_mut().resource_mut::<crate::script::ScriptState>().rt.as_mut().unwrap().game.ui_tree = ui;
    }

    /// The pointer goes to `at` and the left button down on the next frame.
    fn press_at(app: &mut App, (x, y): (f64, f64)) {
        app.world_mut().resource_mut::<PointerAid>().due.push((0.0, AidPointer::At { x, y, click: true }));
    }

    /// What the menu did: the page, and whether it was asked to close.
    fn tutorial_acted(app: &App) -> (usize, bool) {
        (app.world().resource::<crate::tutorial_menu::TutorialView>().page, app.world().resource::<crate::script::ScriptState>().answered())
    }

    fn hand_built_buttons(app: &mut App) -> usize {
        let world = app.world_mut();
        world.query_filtered::<(), With<crate::tutorial_menu::TmButton>>().iter(world).count()
    }

    const ON_TILE_NEXT: (f64, f64) = (1100.0, 220.0);
    const ON_TILE_CLOSE: (f64, f64) = (1100.0, 520.0);
    const ON_NO_TILE: (f64, f64) = (300.0, 700.0);

    /// A script opens the menu in its tick, so its first frame is drawn by hand; the next
    /// builds the file, and the press of that frame reached the hand-built button on screen.
    /// Exactly that button acts, wherever the file's hitboxes are:
    /// - the tile input acting too turns two pages (three pages, so the end does not hide it);
    /// - the press given to the tile input alone is lost (no tile there), or does what the
    ///   tile under it does (Close for the hand-built Next).
    ///
    /// After it the buttons have gone and the tiles alone take a press.
    #[test]
    fn the_press_of_the_frame_tiles_take_over_acts_once_on_the_button_on_screen() {
        use crate::tutorial_menu::TmButton;
        use crate::ui_pointer::PointerState;
        for (button, under, acted) in [
            (TmButton::Next, ON_TILE_NEXT, (1, false)),
            (TmButton::Next, ON_NO_TILE, (1, false)),
            (TmButton::Next, ON_TILE_CLOSE, (1, false)),
            (TmButton::Close, ON_NO_TILE, (0, true)),
            (TmButton::Close, ON_TILE_NEXT, (0, true)),
        ] {
            let case = format!("{button:?} on screen, tile pointer at {under:?}");
            let mut app = tutorial_app();
            app.update();
            assert_eq!(hand_built_buttons(&mut app), 3, "{case}: not built yet, drawn by hand");
            assert_eq!(tutorial_acted(&app), (0, false), "{case}");
            // The frame the file is built: `tiles::write`, then the press (`ui_pointer`).
            built_tutorial(&mut app);
            let world = app.world_mut();
            for (b, mut state) in world.query::<(&TmButton, &mut PointerState)>().iter_mut(world) {
                if *b == button {
                    *state = PointerState::Pressed;
                }
            }
            press_at(&mut app, under);
            app.update();
            assert_eq!(tutorial_acted(&app), acted, "{case}");
            assert_eq!(hand_built_buttons(&mut app), 0, "{case}: gone with this frame's drawing");
            if acted.1 {
                continue;
            }
            // A frame without a press: nothing left over.
            app.update();
            assert_eq!(tutorial_acted(&app), acted, "{case}: nothing acts again");
            // The tiles own the pointer now, at their own places.
            press_at(&mut app, ON_NO_TILE);
            app.update();
            assert_eq!(tutorial_acted(&app), (1, false), "{case}: where no tile is");
            press_at(&mut app, ON_TILE_NEXT);
            app.update();
            assert_eq!(tutorial_acted(&app), (2, false), "{case}: the file's Next, once");
            press_at(&mut app, ON_TILE_CLOSE);
            app.update();
            assert_eq!(tutorial_acted(&app), (2, true), "{case}: the file's Close");
        }
    }

    /// The file built before the frame the menu is first drawn (activation opens it before
    /// `tiles::write`, or it was built before): no hand-built button ever stands, and the
    /// first press is the tiles', once.
    #[test]
    fn a_menu_built_when_it_opens_has_no_hand_built_buttons() {
        let mut app = tutorial_app();
        built_tutorial(&mut app);
        press_at(&mut app, ON_TILE_NEXT);
        app.update();
        assert_eq!(hand_built_buttons(&mut app), 0);
        assert_eq!(tutorial_acted(&app), (1, false));
        app.update();
        assert_eq!(tutorial_acted(&app), (1, false));
    }

    #[test]
    fn the_trait_menus_keys_act_once() {
        // Y and X stand for R and A there: the hand-built handler acts, so the file's bindings
        // do not act again; a controller not working the controls presses no key.
        use super::super::menus::key_click_acts;
        use super::super::trait_menu::TRAIT_MENU;
        let at = crate::pad::MenuAt { code: Some(fnv_game::game::menus::TRAITS), tab: 0 };
        assert!(stood_in(GamepadButton::North, at, &|k| k == KeyCode::KeyR));
        assert!(stood_in(GamepadButton::West, at, &|k| k == KeyCode::KeyA));
        for (key, physical) in [("R", vec!["R"]), ("A", vec!["A"]), ("xbuttony", vec!["R", "xbuttony"]), ("xbuttonx", vec!["A", "xbuttonx"])] {
            assert!(!key_click_acts(TRAIT_MENU, key, &physical), "{key} by hand");
            assert!(key_click_acts(TRAIT_MENU, key, &[]), "{key} by a script or the file");
        }
    }

    #[test]
    fn keys_and_units() {
        assert_eq!(key_name(KeyCode::KeyE), Some((true, "E")));
        assert_eq!(key_name(KeyCode::ArrowDown), Some((false, "xdown")));
        assert_eq!(key_name(KeyCode::Digit1), None, "no digits in the unmodded game");
        // 960 units to the height: at 1280 x 720, a pixel is 4/3 of a unit.
        assert_eq!(to_units(Vec2::new(640.0, 360.0), 720.0), (853.3333333333334, 480.0));
        assert_eq!(to_units(Vec2::new(0.0, 960.0), 960.0), (0.0, 960.0));
    }
    #[test]
    fn the_tag_menus_physical_keys_act_once_and_script_keys_still_act() {
        let menu = super::super::tag_menu::TAG_MENU;
        for key in super::super::trait_menu::HAND_BUILT_KEYS {
            assert!(!super::super::menus::key_click_acts(menu, key, &[key]));
            assert!(super::super::menus::key_click_acts(menu, key, &[]));
        }
    }

    /// #1631: a tile menu drawn at the UI scale (its root `crate::interface::framed_root`,
    /// its pieces placed with `crate::interface::at`, as `super::super::draw::paint` does)
    /// takes a click on a piece's drawn middle as a click on the piece in the tree's units,
    /// at 50% and 75% (the menus grow no larger, `crate::interface::menu_scale`), kept to the middle or (the dialogue menu) to the bottom; at 100% the
    /// pointer reads as [`to_units`] does, bit for bit.
    #[test]
    fn a_click_lands_on_the_tile_drawn_under_it_at_any_ui_scale() {
        let window = Vec2::new(1280.0, 720.0);
        let full = Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() };
        let px = |v: Val, along: f32| match v {
            Val::Percent(x) => x / 100.0 * along,
            Val::Vh(x) => x / 100.0 * window.y,
            other => panic!("{other:?}"),
        };
        {
            let _s = crate::interface::units_at(1.0);
            for at in [Vec2::new(640.0, 360.0), Vec2::new(3.5, 700.25)] {
                assert_eq!(to_menu_units(at, window, crate::interface::MIDDLE), to_units(at, window.y));
            }
        }
        for s in [0.5f32, 0.75] {
            let _s = crate::interface::units_at(s);
            for dialog in [false, true] {
                let anchor = super::super::draw::menu_anchor(dialog);
                let root = crate::interface::framed_root(full.clone(), anchor);
                // A button tile at (900, 700) units, 120 x 30, in a 1280 x 720 window
                // (1706.7 units wide).
                let piece = crate::interface::at(900.0, 700.0, 120.0, 30.0);
                let left = px(root.left, window.x) + px(piece.left, window.x);
                let top = px(root.top, window.y) + px(piece.top, window.y);
                let (w, h) = (px(piece.width, window.x), px(piece.height, window.y));
                assert!((w - 120.0 * s * 0.75).abs() < 1e-3, "{s}: drawn {w} px wide");
                let (x, y) = to_menu_units(Vec2::new(left + w / 2.0, top + h / 2.0), window, anchor);
                assert!((x - 960.0).abs() < 1e-3 && (y - 715.0).abs() < 1e-3, "{s} {dialog}: ({x}, {y})");
                // Just left of the drawn tile: left of the tile in units too.
                let (x, _) = to_menu_units(Vec2::new(left - 0.5, top + h / 2.0), window, anchor);
                assert!(x < 900.0, "{s} {dialog}: {x}");
            }
            assert_eq!(super::super::draw::menu_anchor(true), crate::interface::BOTTOM_MIDDLE);
        }
    }

    #[test]
    fn click_menu_tile_advances_tiled_menu_and_fires_menu_click_event_handler() {
        use fnv_game::game::RunCtx;
        use fnv_script::vm::XVal;
        const NEXT: &str = "TutorialMenu/NOGLOW_BRANCH/TM_Background/TM_depth_rect/TM_next_button";
        const NEXT_TEXT: &str = "TutorialMenu/NOGLOW_BRANCH/TM_Background/TM_depth_rect/TM_next_button/button_text";
        let mut app = tutorial_app();
        built_tutorial(&mut app);
        {
            let mut st = app.world_mut().resource_mut::<crate::script::ScriptState>();
            let game = &mut st.rt.as_mut().unwrap().game;
            let root = game.ui_tree.menu("TutorialMenu").unwrap();
            game.ui_tree.set_trait(root, "_scriptClicks", fnv_ui::Value::Num(0.0));
            game.ui_tree.set_trait(root, "_scriptName", fnv_ui::Value::Str(String::new()));
            game.run_query(
                &format!(
                    r#"Begin Function {{}}
SetOnMenuClickEventHandler (Begin Function {{int m, int t, string_var sN}}
ModUIFloat "TutorialMenu/_scriptClicks" 1
SetUIString "TutorialMenu/_scriptName" sN
End) 1 "{NEXT}"
ClickMenuTile "{NEXT}"
End"#
                ),
                RunCtx::default(),
            )
            .unwrap();
            assert_eq!(game.ui_tree.get_float(&format!("{NEXT}/clicked")), Some(1.0));
        }
        app.update();
        assert_eq!(tutorial_acted(&app), (1, false), "ClickMenuTile must advance the tiled TutorialMenu");
        {
            let st = app.world().resource::<crate::script::ScriptState>();
            let ui = &st.rt.as_ref().unwrap().game.ui_tree;
            assert_eq!(ui.get_float(&format!("{NEXT}/clicked")), Some(0.0), "clicked pulse ended");
            assert_eq!(ui.get_float("TutorialMenu/_scriptClicks"), Some(1.0), "SetOnMenuClickEventHandler fired");
            assert_eq!(ui.get_string("TutorialMenu/_scriptName").as_deref(), Some("tm_next_button"));
        }

        // Clicking a child inside a target <hotrect> also advances the menu and fires the handler.
        {
            let mut st = app.world_mut().resource_mut::<crate::script::ScriptState>();
            let game = &mut st.rt.as_mut().unwrap().game;
            let r = game.run_query(&format!("Begin Function {{}}\nSetFunctionValue (ClickMenuTile \"{NEXT_TEXT}\")\nEnd"), RunCtx::default()).unwrap();
            assert_eq!(r, XVal::Num(1.0));
        }
        app.update();
        assert_eq!(tutorial_acted(&app), (2, false), "ClickMenuTile on target-hotrect child advances TutorialMenu");
        {
            let ui = &app.world().resource::<crate::script::ScriptState>().rt.as_ref().unwrap().game.ui_tree;
            assert_eq!(ui.get_float("TutorialMenu/_scriptClicks"), Some(2.0));
        }

        // Even with `ui_files == None` (e.g. injected tiles before menu XMLs load), `feed`
        // still drains queued script clicks via `ui_input::idle`.
        {
            let mut st = app.world_mut().resource_mut::<crate::script::ScriptState>();
            let game = &mut st.rt.as_mut().unwrap().game;
            game.ui_files = None;
            let r = game.run_query(&format!("Begin Function {{}}\nSetFunctionValue (ClickMenuTile \"{NEXT}\")\nEnd"), RunCtx::default()).unwrap();
            assert_eq!(r, XVal::Num(1.0));
        }
        app.update();
        let ui = &app.world().resource::<crate::script::ScriptState>().rt.as_ref().unwrap().game.ui_tree;
        assert_eq!(ui.get_float("TutorialMenu/_scriptClicks"), Some(3.0), "idle drain fired SetOnMenuClickEventHandler with ui_files == None");
        assert_eq!(ui.get_float(&format!("{NEXT}/clicked")), Some(0.0));
    }

    #[test]
    fn idle_feed_clears_stale_ui_pointer_so_xml_cursor_pos_tracks_live_window() {
        use fnv_game::game::{menus, RunCtx};
        use fnv_script::vm::XVal;
        let mut app = tutorial_app();
        // Close tiled tutorial switch so taking is empty (hand-built menu open).
        app.world_mut().resource_mut::<super::super::vanilla::TileMenus>().tutorial = false;
        {
            let mut st = app.world_mut().resource_mut::<crate::script::ScriptState>();
            let game = &mut st.rt.as_mut().unwrap().game;
            game.menu = Some(menus::MESSAGE);
            game.other.jip9.window = Some((1280.0, 720.0));
            game.other.jip9.cursor_px = Some((300.0, 240.0));
            game.ui_pointer.at = Some((100.0, 80.0));
        }
        app.update();
        let mut st = app.world_mut().resource_mut::<crate::script::ScriptState>();
        let game = &mut st.rt.as_mut().unwrap().game;
        assert_eq!(game.ui_pointer.at, None, "stale ui_pointer.at cleared when no tile menu owns input");
        let x = game.run_query("Begin Function {}\nSetFunctionValue (GetCursorPos X 1)\nEnd", RunCtx::default()).unwrap();
        let y = game.run_query("Begin Function {}\nSetFunctionValue (GetCursorPos Y 1)\nEnd", RunCtx::default()).unwrap();
        assert_eq!(x, XVal::Num(400.0));
        assert_eq!(y, XVal::Num(320.0));
    }
}
