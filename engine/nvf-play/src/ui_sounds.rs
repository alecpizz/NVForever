//! Sounds of the menus the engine draws (docs/spec-notes/audio.md §12): the inventory,
//! container and barter menus, the dialogue menu's choices, the Wait and Sleep menu and
//! the pause menu with its settings page.
//!
//! - **What the interface files say** [observed: `clicksound` and `mouseoversound` in the
//!   base archive's `menus\*.xml`, audio.md §12.1]: a control names the Editor ID of a SOUN
//!   to play when it is clicked and when the pointer comes onto it [documented: GECK wiki
//!   Mouseoversound, Clicksound]. The list rows of the container, barter, dialogue and
//!   pause menus include `list_box_template.xml`: `UIMenuOK` on a click, `UIMenuFocus`
//!   when the pointer comes on; the dialogue menu's topics silence the click
//!   (`&nosound;`), the inventory's list silences both. The settings' switches and meters
//!   click `UIMenuPrevNext`, their back button `UIMenuCancel`.
//! - **What the files do not say** (opening and closing a menu, taking everything, a
//!   trade, the Wait menu's slider and buttons) is chosen by the SOUN records' names and
//!   tagged so in [`sound_for`] and audio.md §12.2.
//! - **How it is wired.** The menus' own state is watched here where it says enough:
//!   [`crate::menu::ActiveMenu`] (a menu opened or closed), [`crate::wait::WaitMenu`]
//!   (opened, slider, accept, cancel, done), [`crate::dialogue_menu::DialogueMenu`] (the
//!   selected choice, a choice answered) and the pause menu's page. What only the menu
//!   knows (a command's outcome: a row put up for barter, an item put on or used, take
//!   all, a trade or a refusal, a settings switch flipped) it sends as [`UiSound`];
//!   [`menu_event`] tells the inventory, container and barter menus' commands apart.
//! - Items moved in a container keep the item's own pick-up and drop sounds
//!   (`crate::world_sounds`); for Take All they are held back and `UIItemTakeAll` plays
//!   once. An aid item used from the inventory plays its record's consume sound (ALCH
//!   `ENIT` Sound - Consume [documented: xEdit, `schemas/fnv/objects.json`]).
//! - Played at the listener (`PlaySound { at: None }`). Message boxes (`world_sounds`) and
//!   the quest lines (`quest_messages`) play their own.
//!
//! A `ui sounds summary` line at the end.

use crate::menu::model::{Applied, Category, Command, MenuModel, Mode, Row, Side};
use crate::menu::ActiveMenu;
use crate::sfx::{PlaySound, Sfx};
use crate::world_sounds::WorldSounds;
use crate::{PlaySet, PlayWorld};
use bevy::prelude::*;
use fnv_esm::LoadOrder;
use fnv_sim::FormId;
use std::collections::{BTreeMap, HashMap};

/// A menu the engine draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UiMenu {
    Inventory,
    Container,
    Barter,
    Dialogue,
    Wait,
    Sleep,
    Pause,
    /// The pause menu's settings page.
    Settings,
    /// The lockpicking menu (`crate::lockpick`).
    Lockpick,
    /// The Vigor Tester's S.P.E.C.I.A.L. menu (`crate::chargen::vigor`).
    Vigor,
    /// The tag-skill menu (`crate::chargen`).
    TagSkills,
    /// The trait menu (`crate::chargen`).
    Traits,
    /// The level-up menu (`crate::levelup`).
    LevelUp,
    /// A terminal: its hacking game and its menu (`crate::terminal`).
    Terminal,
    /// The main menu (`crate::main_menu`).
    MainMenu,
    /// The recipe menu (`crate::crafting`).
    Recipe,
    /// The character creator (the race menu, `crate::creator`).
    Creator,
    /// The blackjack table (`crate::blackjack`).
    Blackjack,
    /// A vendor's repair services (`crate::repair`).
    Repair,
    /// The Caravan table (`crate::caravan`).
    Caravan,
}

impl UiMenu {
    pub fn of(mode: Mode) -> UiMenu {
        match mode {
            Mode::Inventory => UiMenu::Inventory,
            Mode::Container(_) => UiMenu::Container,
            Mode::Barter(_) => UiMenu::Barter,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            UiMenu::Inventory => "inventory",
            UiMenu::Container => "container",
            UiMenu::Barter => "barter",
            UiMenu::Dialogue => "dialogue",
            UiMenu::Wait => "wait",
            UiMenu::Sleep => "sleep",
            UiMenu::Pause => "pause",
            UiMenu::Settings => "settings",
            UiMenu::Lockpick => "lockpick",
            UiMenu::Vigor => "vigor tester",
            UiMenu::TagSkills => "tag skills",
            UiMenu::Traits => "traits",
            UiMenu::LevelUp => "level up",
            UiMenu::Terminal => "terminal",
            UiMenu::MainMenu => "main menu",
            UiMenu::Recipe => "recipe",
            UiMenu::Creator => "creator",
            UiMenu::Blackjack => "blackjack",
            UiMenu::Repair => "repair services",
            UiMenu::Caravan => "caravan",
        }
    }
}

/// What happened in a menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiEvent {
    Opened,
    Closed,
    /// The selection moved to another row (keys, wheel or pointer), or to the other list.
    Focus,
    /// Another category of a list.
    Switch,
    /// A row or a button acted on: an item put up for barter or taken back, the settings
    /// page chosen.
    Picked,
    /// A dialogue choice was answered.
    Answered,
    /// Back, cancel, exit: the menu (or its page) goes without doing anything more.
    Back,
    /// Container menu: everything taken.
    TakeAll,
    /// Barter: the trade made; `caps` the caps the player got (negative: paid).
    Traded { caps: i64 },
    /// An action the menu refused (not enough caps, nothing to take or trade).
    Refused,
    /// Inventory: an item put on (`on`) or taken off.
    Equipped { item: FormId, on: bool },
    /// Inventory: an aid item used up.
    Used { item: FormId },
    /// The Wait menu's slider moved.
    Slider,
    /// The Wait menu's hours accepted.
    Accepted,
    /// The Wait menu closed after the last hour passed.
    Finished,
    /// A settings switch flipped.
    Flipped,
    /// Lockpicking: the pin turned some way.
    PinMoved,
    /// Lockpicking: torque put on, the lock starts to turn.
    Turning,
    /// Lockpicking: the lock stopped short of opening.
    Stopped,
    /// Lockpicking: the pin under strain against the stop (`on`), or no longer.
    Strain { on: bool },
    /// Lockpicking: the pin broke.
    PinBroke,
    /// Lockpicking: the lock opened (picked or forced).
    Unlocked,
    /// Lockpicking: forcing the lock broke it.
    ForceFailed,
    /// A value raised (`up`) or lowered: a S.P.E.C.I.A.L. attribute.
    Value { up: bool },
    /// Reset: the menu's choices back to where they started.
    Reset,
    /// The hacking game: the password entered, access granted.
    Granted,
    /// Blackjack: a card dealt.
    CardDealt,
    /// Blackjack: the dealer's hole card turned over.
    CardFlipped,
    /// Blackjack: the shoe shuffled.
    Shuffled,
    /// Blackjack: the cards swept off the table (`dealer`: the dealer's).
    Collected { dealer: bool },
    /// Blackjack: a round won (`won`) or lost.
    RoundOver { won: bool },
}

impl UiEvent {
    pub fn name(self) -> &'static str {
        match self {
            UiEvent::Opened => "opened",
            UiEvent::Closed => "closed",
            UiEvent::Focus => "selection moved",
            UiEvent::Switch => "category changed",
            UiEvent::Picked => "row or button picked",
            UiEvent::Answered => "choice answered",
            UiEvent::Back => "back or cancel",
            UiEvent::TakeAll => "take all",
            UiEvent::Traded { .. } => "trade made",
            UiEvent::Refused => "refused",
            UiEvent::Equipped { on: true, .. } => "item put on",
            UiEvent::Equipped { on: false, .. } => "item taken off",
            UiEvent::Used { .. } => "item used",
            UiEvent::Slider => "slider moved",
            UiEvent::Accepted => "accepted",
            UiEvent::Finished => "hours passed",
            UiEvent::Flipped => "switch flipped",
            UiEvent::PinMoved => "pin moved",
            UiEvent::Turning => "lock turning",
            UiEvent::Stopped => "lock stopped",
            UiEvent::Strain { on: true } => "pin under strain",
            UiEvent::Strain { on: false } => "strain off",
            UiEvent::PinBroke => "pin broke",
            UiEvent::Unlocked => "lock opened",
            UiEvent::ForceFailed => "lock broken by forcing",
            UiEvent::Value { up: true } => "value raised",
            UiEvent::Value { up: false } => "value lowered",
            UiEvent::Reset => "reset",
            UiEvent::Granted => "access granted",
            UiEvent::CardDealt => "card dealt",
            UiEvent::CardFlipped => "card turned over",
            UiEvent::Shuffled => "shoe shuffled",
            UiEvent::Collected { .. } => "cards collected",
            UiEvent::RoundOver { won: true } => "round won",
            UiEvent::RoundOver { won: false } => "round lost",
        }
    }
}

/// A menu's event to sound (sent by the menus that know more than their state says).
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiSound {
    pub menu: UiMenu,
    pub event: UiEvent,
}

/// The sound an event makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// A SOUN by Editor ID.
    Named(&'static str),
    /// An item's pick-up (`up`) or drop sound, as `crate::world_sounds` picks it.
    Item { item: FormId, up: bool },
    /// The caps' pick-up or drop sound.
    Caps { up: bool },
    /// An aid item's consume sound (ALCH `ENIT`).
    Consume(FormId),
    /// A SOUN by Editor ID that goes round until it is stopped (`on` false).
    Loop(&'static str, bool),
    /// None, and why.
    Silent(&'static str),
}

/// The caps item [observed: MISC 0x0000000F `Caps001`, "Bottle Cap"].
pub const CAPS: FormId = crate::menu::model::CAPS;

/// Which sound an event in a menu makes. Tags: see audio.md §12.2, where this table is
/// written out with its sources.
pub fn sound_for(menu: UiMenu, event: UiEvent) -> Choice {
    use Choice::*;
    use UiEvent::*;
    use UiMenu::*;
    match (menu, event) {
        // Tab is the Pip-Boy's control; the inventory stands for its Items page [design:
        // by the records' names].
        (Inventory, Opened) => Named("UIPipBoyAccessUp"),
        (Inventory, Closed) => Named("UIPipBoyAccessDown"),
        // The Pip-Boy's list silences the template's sound (inventory_menu.xml
        // `&nosound;`) [observed]; its own scroll sound by name [hypothesis]; its file is
        // `ui_menu_focus.wav`, the same as UIMenuFocus's [observed].
        (Inventory, Focus) => Named("UIPipBoyScroll"),
        // Put on: the item's pick-up sound, taken off: its drop sound [design].
        (Inventory, Equipped { item, on }) => Item { item, up: on },
        (Inventory, Used { item }) => Consume(item),
        // A container has its own open and close sounds (CONT SNAM / QNAM, world_sounds).
        (Container, Opened | Closed) => Silent("the container's own sound (world_sounds)"),
        // The rows' mouse-over sound [observed: list_box_template.xml, included by the
        // container, barter, dialogue and pause menus' rows]; for the keys too [design].
        (Container | Barter | Dialogue | Settings | Recipe | Repair, Focus) => Named("UIMenuFocus"),
        // The arrows of other menus' lists (level up, character, tutorial) [design].
        (Container | Barter | Recipe, Switch) => Named("UIMenuPrevNext"),
        (Container, TakeAll) => Named("UIItemTakeAll"),
        // The rows' click sound [observed: list_box_template.xml].
        (Barter, Picked) => Named("UIMenuOK"),
        // The caps changing hands [design]; a trade of goods for goods clicks.
        (Barter, Traded { caps }) if caps > 0 => Caps { up: true },
        (Barter, Traded { caps }) if caps < 0 => Caps { up: false },
        (Barter, Traded { .. }) => Named("UIMenuOK"),
        // The menu modes' sound by its name [design].
        (Barter | Wait | Sleep | Pause | Recipe | Repair, Opened) => Named("UIMenuMode"),
        // Exit and cancel as the settings' back button [observed: start_menu.xml
        // main_back_button `UIMenuCancel`; design for these menus].
        (Barter | Wait | Sleep | Pause | Settings | Recipe | Repair, Back | Closed) => Named("UIMenuCancel"),
        // Lockpicking (lockpicking.md §6): the SOUN records named for it [observed:
        // FalloutNV.esm `UILockpicking…`; design: which event each is for, by its name].
        // Opening: the lock model's own sound key [observed: lockinterface01.nif
        // `Forward` "Sound: UILockpickingEnter"], played here as the model is drawn
        // without its sequence's sound keys.
        (Lockpick, Opened) => Named("UILockpickingEnter"),
        (Lockpick, Closed) => Silent("the lock's model goes without a sound (its Backward has no sound key)"),
        (Lockpick, PinMoved) => Named("UILockpickingPickMovement"),
        (Lockpick, Turning) => Named("UILockpickingCylinderTurn"),
        (Lockpick, Stopped) => Named("UILockpickingCylinderStop"),
        (Lockpick, Strain { on }) => Loop("UILockpickingPickTensionLPM", on),
        (Lockpick, PinBroke) => Named("UILockpickingPickBreak"),
        (Lockpick, Unlocked) => Named("UILockpickingUnlock"),
        (Lockpick, ForceFailed) => Named("UILockpickingForceFail"),
        // A lock the player cannot pick [hypothesis: by the record's name, `DRSLocked`].
        (Lockpick, Refused) => Named("DRSLocked"),
        // The Vigor Tester (list 5 item 39): its menu file names no sound and its models
        // no sound key [observed: love_tester_menu.xml, the two nv_vitomaticvigortester
        // models]. Fallout 3's S.P.E.C.I.A.L. book sounds, kept in the masters with two
        // added for New Vegas (`…NumberDown`, `…NumberFinished`) [observed: FalloutNV.esm
        // SOUN; hypothesis: that the Vigor Tester plays them, by their names]: a page
        // turned, an attribute raised or lowered, a row chosen, Done.
        (Vigor, Switch) => Named("OBJBookSpecialPageTurn"),
        (Vigor, Value { up: true }) => Named("OBJBookSpecialNumber"),
        (Vigor, Value { up: false }) => Named("OBJBookSpecialNumberDown"),
        (Vigor, Focus) => Named("OBJBookSpecialFocus"),
        (Vigor, Accepted) => Named("OBJBookSpecialNumberFinished"),
        // The tag-skill and trait menus (items 41, 42). Their rows' templates silence
        // both sounds [observed: char_gen_menu.xml CGM_SelectItem, trait_menu.xml
        // LUM_SkillItem `&nosound;`]; the owner wants them, so the pointer's is the one of
        // the template they include (`UIMenuFocus`, list_box_template.xml) and a row taken
        // or given back clicks as the trait file's perk rows do (`UIMenuPrevNext`,
        // trait_menu.xml LUM_Perk) [design]. Reset and Done are text boxes (`UIMenuFocus`
        // under the pointer, text_box.xml); Done clicks `UIMenuOK` [observed:
        // CGM_DoneButton, LUM_ContinueButton]; Reset names no click: `UIMenuCancel` as
        // the settings' back button [design].
        // The character creator (docs/spec-notes/character-creator.md §6) [design: the
        // tag-skill menu's sounds; a tab changed as the level-up menu's arrows].
        (Creator, Focus) => Named("UIMenuFocus"),
        (Creator, Switch | Value { .. } | Slider | Picked) => Named("UIMenuPrevNext"),
        (Creator, Reset) => Named("UIMenuCancel"),
        (Creator, Accepted) => Named("UIMenuOK"),
        (TagSkills | Traits, Focus) => Named("UIMenuFocus"),
        (TagSkills | Traits, Picked) => Named("UIMenuPrevNext"),
        // The attributes page's arrows [documented: char_gen_menu.xml
        // CGM_Template_LeftArrow, RightArrow `UIMenuPrevNext`].
        (TagSkills, Value { .. }) => Named("UIMenuPrevNext"),
        (TagSkills | Traits, Reset) => Named("UIMenuCancel"),
        (TagSkills | Traits, Accepted) => Named("UIMenuOK"),
        // The level-up menu (docs/spec-notes/levelup.md §5): the arrows and the perk rows
        // click `UIMenuPrevNext`, Continue, Back and Done `UIMenuOK` [documented:
        // levelup_menu.xml LUM_Template_LeftArrow, RightArrow, LUM_Perk,
        // LUM_ContinueButton, LUM_BackButton]; the pointer and Reset as the tag-skill
        // menu's [design]. Opening plays `MUSSuccess` (`fx\mus\mus_success.wav`, the
        // archives' `.ogg`) [owner, 2026-10-24: "the sound effect we actually want to play
        // when the level up screen is shown is mus_success.ogg"; observed: SOUN 0x00078594
        // names it]; `UILevelUpText` is the XP meter's filled bar (`crate::xp_meter`).
        (LevelUp, Opened) => Named("MUSSuccess"),
        (LevelUp, Focus) => Named("UIMenuFocus"),
        (LevelUp, Value { .. } | Picked) => Named("UIMenuPrevNext"),
        (LevelUp, Switch | Accepted) => Named("UIMenuOK"),
        (LevelUp, Reset) => Named("UIMenuCancel"),
        // Terminals (item 47): the SOUN records named for hacking [observed:
        // FalloutNV.esm `UIHacking…`, their files in `Fallout - Sound.bsa`; hypothesis:
        // which event each is for, by its name]: the cursor moving over the characters or
        // the menu's rows, a word or a row entered, the password right or wrong.
        (Terminal, Focus) => Named("UIHackingCharScroll"),
        (Terminal, Picked) => Named("UIHackingCharEnter"),
        (Terminal, Granted) => Named("UIHackingPassGood"),
        (Terminal, Refused) => Named("UIHackingPassBad"),
        // Caravan: a card that cannot go there (caravan.md) [observed: FalloutNV.esm
        // `GAMECaravanInvalidStackLocation`; design: for every refusal at the table].
        (Caravan, Refused) => Named("GAMECaravanInvalidStackLocation"),
        (_, Refused) => Named("UIMenuCancel"),
        // The topics silence the click [observed: dialog_menu.xml DM_Topic].
        (Dialogue, Answered) => Silent("&nosound; in dialog_menu.xml"),
        // The settings' meters click it [hypothesis: the files' slider; sleep_wait_menu.xml's
        // own scroll bar names none].
        (Wait | Sleep, Slider) => Named("UIMenuPrevNext"),
        // The basic button's click [observed: globals.xml `_sound_button_click`,
        // button_basic.xml; design: that these buttons are such].
        (Wait | Sleep, Accepted) => Named("UIMenuOK"),
        (Wait | Sleep, Finished) => Silent("the hours have passed"),
        // The pause menu's rows [observed: start_menu.xml, list_box_template.xml].
        (Settings | Pause | MainMenu, Opened | Picked) => Named("UIMenuOK"),
        // Making a recipe: the list's click [observed: list_box_template.xml `clicksound`;
        // design: no crafting sound is named in the files].
        (Recipe, Picked) => Named("UIMenuOK"),
        // A repair made: the SOUN named for it [observed: FalloutNV.esm `UIRepairWeapon`;
        // hypothesis: that it is the repair's sound, for armour too]. The rows' own click
        // is silenced [observed: repair_services_menu.xml `&nosound;`].
        (Repair, Picked) => Named("UIRepairWeapon"),
        // The pause and main menus' entries are list rows too [observed: start_menu.xml
        // `main_container` includes list_box.xml; list_box_template.xml `mouseoversound`].
        (Pause | MainMenu, Focus) => Named("UIMenuFocus"),
        // The settings' toggles [observed: start_menu.xml lb_toggle_left, _value, _right].
        (Settings, Flipped) => Named("UIMenuPrevNext"),
        // Blackjack (blackjack.md §6): the SOUN records named for it [observed:
        // FalloutNV.esm `GAMEBlackJack…`; design: which event each is for, by its name].
        (Blackjack, CardDealt) => Named("GAMEBlackJackCardDeal"),
        (Blackjack, CardFlipped) => Named("GAMEBlackJackCardFlip"),
        (Blackjack, Shuffled) => Named("GAMEBlackJackCardShuffle"),
        (Blackjack, Collected { dealer: false }) => Named("GAMEBlackJackCardCollect"),
        (Blackjack, Collected { dealer: true }) => Named("GAMEBlackJackCardCollectDealer"),
        (Blackjack, Value { up: true }) => Named("GAMEBlackJackIncreaseBet"),
        (Blackjack, Value { up: false }) => Named("GAMEBlackJackDecreaseBet"),
        (Blackjack, RoundOver { won: true }) => Named("GAMEBlackJackWin"),
        (Blackjack, RoundOver { won: false }) => Named("GAMEBlackJackLose"),
        (Blackjack, Opened) => Named("UIMenuMode"),
        (Blackjack, Back | Closed) => Named("UIMenuCancel"),
        // Caravan (caravan.md): the SOUN records named for it [observed: FalloutNV.esm
        // `GAMECaravan…`; design: which event each is for, by its name].
        (Caravan, Opened) => Named("UIMenuMode"),
        (Caravan, Accepted) => Named("GAMECaravanStartMatch"),
        (Caravan, Focus) => Named("GAMECaravanNavigate"),
        (Caravan, Switch) => Named("GAMECaravanToggleTrack"),
        (Caravan, Picked) => Named("GAMECaravanAddRemove"),
        (Caravan, Shuffled) => Named("GAMECaravanChangeDeck"),
        (Caravan, Value { .. }) => Named("GAMECaravanFundsDrop"),
        (Caravan, CardFlipped) => Named("GAMECaravanSwitchCard"),
        (Caravan, CardDealt) => Named("GAMECaravanAddToTrack"),
        (Caravan, Collected { .. }) => Named("GAMECaravanRemoveCard"),
        (Caravan, RoundOver { won: true }) => Named("GAMECaravanWin"),
        (Caravan, RoundOver { won: false }) => Named("GAMECaravanLose"),
        (Caravan, Back | Closed) => Named("UIMenuCancel"),
        _ => Silent("no sound for this"),
    }
}

// ---- the inventory, container and barter menus' commands ----------------------------------

/// What a menu was before a command.
#[derive(Debug, Clone)]
pub struct MenuBefore {
    focus: Side,
    selected: [usize; 2],
    category: [Category; 2],
    row: Option<Row>,
    paid: Option<i64>,
}

impl MenuBefore {
    pub fn of(m: &MenuModel) -> MenuBefore {
        MenuBefore { focus: m.focus, selected: m.selected, category: m.category, row: m.selected_row(m.focus).cloned(), paid: m.trade.as_ref().map(|t| t.paid()) }
    }
}

/// What a command did, for its sound (`None`: nothing to hear here). Moving an item in a
/// container sounds in `crate::world_sounds`, dropping one when it lands, closing when
/// the menu goes.
pub fn menu_event(cmd: Command, before: &MenuBefore, after: &MenuModel, a: &Applied) -> Option<UiEvent> {
    let acted_on_selected = matches!(cmd, Command::Select(side, i) if before.focus == side && before.selected[side as usize] == i);
    let moved = || after.focus != before.focus || after.selected[after.focus as usize] != before.selected[after.focus as usize];
    match cmd {
        Command::Select(..) if acted_on_selected => act(after, before, a),
        Command::Up | Command::Down | Command::PageUp | Command::PageDown | Command::Scroll(_) | Command::Focus(_) | Command::Select(..) | Command::Hover(..) => moved().then_some(UiEvent::Focus),
        Command::Category(..) => (after.category != before.category).then_some(UiEvent::Switch),
        Command::One | Command::OneSingle | Command::Stack => act(after, before, a),
        // Ok on "How many?": what asked is done now (a drop sounds when it lands).
        Command::AmountOk if !matches!(after.mode, Mode::Inventory) => act(after, before, a),
        Command::TakeAll if matches!(after.mode, Mode::Container(_)) => Some(if a.changed { UiEvent::TakeAll } else { UiEvent::Refused }),
        Command::Accept if matches!(after.mode, Mode::Barter(_)) => Some(if a.changed { UiEvent::Traded { caps: before.paid.unwrap_or(0) } } else { UiEvent::Refused }),
        _ => None,
    }
}

fn act(after: &MenuModel, before: &MenuBefore, a: &Applied) -> Option<UiEvent> {
    match after.mode {
        Mode::Container(_) => None,
        Mode::Barter(_) => a.changed.then_some(UiEvent::Picked),
        Mode::Inventory => {
            if let Some(item) = a.used {
                return Some(UiEvent::Used { item });
            }
            let row = before.row.as_ref()?;
            // Put on or taken off when its row says so now; else refused (ammunition, a
            // misc item: "Cannot equip or use").
            let now = after.list(Side::Player).rows.iter().find(|r| r.item == row.item).map(|r| r.equipped);
            Some(match now {
                Some(on) if on != row.equipped => UiEvent::Equipped { item: row.item, on },
                _ => UiEvent::Refused,
            })
        }
    }
}

// ---- watching the menus' state ------------------------------------------------------------

/// The Wait menu as watched: sleep, the hours on the slider, the hours accepted and left.
type WaitSeen = (bool, u32, Option<(u32, u32)>);

/// What the watched menus showed last frame.
#[derive(Default, PartialEq, Clone)]
struct Seen {
    menu: Option<Mode>,
    wait: Option<WaitSeen>,
    /// The dialogue menu's choices (labels) and the selected one.
    choices: Option<(Vec<String>, usize)>,
    /// The pause menu: open, and the settings page's selected row when that page is up.
    pause: (bool, Option<usize>),
}

/// The events between two frames of the watched menus.
fn watched(was: &Seen, now: &Seen) -> Vec<UiSound> {
    let mut out = Vec::new();
    let mut ev = |menu, event| out.push(UiSound { menu, event });
    // The inventory, container and barter menus.
    if was.menu != now.menu {
        if let Some(m) = was.menu {
            ev(UiMenu::of(m), UiEvent::Closed);
        }
        if let Some(m) = now.menu {
            ev(UiMenu::of(m), UiEvent::Opened);
        }
    }
    // The Wait and Sleep menu.
    let kind = |sleep: bool| if sleep { UiMenu::Sleep } else { UiMenu::Wait };
    match (was.wait, now.wait) {
        (None, Some((sleep, ..))) => ev(kind(sleep), UiEvent::Opened),
        (Some((sleep, _, passing)), None) => ev(kind(sleep), if passing.is_some_and(|p| p.1 == 0) { UiEvent::Finished } else { UiEvent::Back }),
        (Some((_, h0, p0)), Some((sleep, h1, p1))) => {
            if p0.is_none() && p1.is_some() {
                ev(kind(sleep), UiEvent::Accepted);
            } else if p1.is_none() && h0 != h1 {
                ev(kind(sleep), UiEvent::Slider);
            }
        }
        (None, None) => {}
    }
    // The dialogue menu: the selection moving among the same choices, the choices going.
    match (&was.choices, &now.choices) {
        (Some((a, i)), Some((b, j))) if a == b => {
            if i != j {
                ev(UiMenu::Dialogue, UiEvent::Focus);
            }
        }
        (Some(_), _) => ev(UiMenu::Dialogue, UiEvent::Answered),
        _ => {}
    }
    // The pause menu and its settings page.
    match (was.pause, now.pause) {
        ((false, _), (true, _)) => ev(UiMenu::Pause, UiEvent::Opened),
        ((true, _), (false, _)) => ev(UiMenu::Pause, UiEvent::Back),
        ((true, None), (true, Some(_))) => ev(UiMenu::Settings, UiEvent::Opened),
        ((true, Some(_)), (true, None)) => ev(UiMenu::Settings, UiEvent::Back),
        ((true, Some(a)), (true, Some(b))) if a != b => ev(UiMenu::Settings, UiEvent::Focus),
        _ => {}
    }
    out
}

// ---- playing --------------------------------------------------------------------------------

#[derive(Default, Debug)]
pub struct UiSoundStats {
    /// Sounds played, by Editor ID.
    pub played: BTreeMap<String, u32>,
    /// Events by menu (with or without a sound).
    pub by_menu: BTreeMap<UiMenu, u32>,
    /// Events without a sound, by why.
    pub silent: BTreeMap<&'static str, u32>,
    /// Sounds chosen but not found (no such SOUN, an item naming none).
    pub missing: u32,
    /// Items moved by Take All whose own sounds were held back (`world_sounds`).
    pub held_back: u32,
}

#[derive(Resource, Default)]
pub struct UiSounds {
    /// ALCH: the consume sound, where the record names one.
    consume: HashMap<FormId, FormId>,
    seen: Seen,
    pending_inventory_open: bool,
    by_edid: Option<HashMap<String, FormId>>,
    pub stats: UiSoundStats,
}

/// The consume sound of every ALCH that names one: `ENIT` bytes 16–19, a FormID
/// [documented: xEdit ALCH ENIT `sound_consume`, schemas/fnv/objects.json].
fn consume_sounds(lo: &LoadOrder) -> (u32, HashMap<FormId, FormId>) {
    let (mut records, mut out) = (0, HashMap::new());
    for (&g, defs) in &lo.index {
        let Some(d) = defs.last() else { continue };
        let pi = d.plugin as usize;
        let r = lo.records[pi][d.ordinal as usize];
        if &r.header.sig != b"ALCH" || r.header.flags & 0x20 != 0 {
            continue;
        }
        records += 1;
        let Ok(subs) = r.subrecords() else { continue };
        let Some(enit) = subs.iter().find(|s| &s.sig == b"ENIT") else { continue };
        let Some(local) = enit.data.get(16..20).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])) else { continue };
        if local == 0 {
            continue;
        }
        if let Ok(Some(s)) = fnv_sim::extract::global(lo, pi, local) {
            out.insert(FormId(g), s);
        }
    }
    (records, out)
}

fn init(mut commands: Commands, d: Res<PlayWorld>) {
    let (records, consume) = consume_sounds(d.lo);
    info!("ui sounds: {records} ALCH records, {} name a consume sound", consume.len());
    commands.insert_resource(UiSounds { consume, ..Default::default() });
}

/// What the watched menus show now.
fn seen_now(active: &ActiveMenu, wait: Option<&crate::wait::WaitMenu>, dialogue: Option<&crate::dialogue_menu::DialogueMenu>, paused: bool, page: Option<crate::pause::Page>) -> Seen {
    Seen {
        menu: active.0.as_ref().map(|m| m.mode),
        wait: wait.and_then(|w| w.0.as_ref()).map(|m| (m.sleep, m.hours, m.passing.map(|p| (p.asked, p.left)))),
        choices: dialogue.and_then(|d| d.view.as_ref()).filter(|v| v.line.is_none() && !v.choices.is_empty()).map(|v| (v.choices.iter().map(|c| c.label.clone()).collect(), d_selected(dialogue))),
        pause: (paused, page.and_then(|p| p.settings_row())),
    }
}

fn d_selected(d: Option<&crate::dialogue_menu::DialogueMenu>) -> usize {
    d.map_or(0, |d| d.selected)
}

/// [design: pipboy.rs, nvforever/player_body.rs] Access follows the visible raise;
/// ordinary openings and the screen menu retain their opening-frame sound.
fn inventory_access(events: &mut Vec<UiSound>, pending: &mut bool, inventory: bool, hidden: bool) {
    events.retain(|sound| {
        if sound.menu != UiMenu::Inventory {
            return true;
        }
        if sound.event == UiEvent::Opened && hidden {
            *pending = true;
            return false;
        }
        !(sound.event == UiEvent::Closed && *pending)
    });
    if !inventory {
        *pending = false;
    } else if *pending && !hidden {
        events.push(UiSound { menu: UiMenu::Inventory, event: UiEvent::Opened });
        *pending = false;
    }
}

#[allow(clippy::too_many_arguments)]
fn play(
    mut ui: ResMut<UiSounds>,
    mut sent: MessageReader<UiSound>,
    active: Res<ActiveMenu>,
    pip: Option<Res<crate::pipboy::PipBoy>>,
    wait: Option<Res<crate::wait::WaitMenu>>,
    dialogue: Option<Res<crate::dialogue_menu::DialogueMenu>>,
    paused: Option<Res<crate::pause::Paused>>,
    page: Option<Res<crate::pause::Page>>,
    sfx: Option<Res<Sfx>>,
    mut world: Option<ResMut<WorldSounds>>,
    mut out: MessageWriter<PlaySound>,
    mut loops: MessageWriter<crate::sfx::LoopSound>,
) {
    let ui = &mut *ui;
    let now = seen_now(&active, wait.as_deref(), dialogue.as_deref(), paused.is_some_and(|p| p.0), page.as_deref().copied());
    let mut events = watched(&ui.seen, &now);
    ui.seen = now;
    events.extend(sent.read().copied());
    let hidden = pip.is_some_and(|p| p.frame == crate::pipboy::Frame::Device && (p.warming.is_some() || p.pose_hidden));
    inventory_access(&mut events, &mut ui.pending_inventory_open, ui.seen.menu == Some(Mode::Inventory), hidden);
    let Some(sfx) = sfx.as_deref() else { return };
    for UiSound { menu, event } in events {
        *ui.stats.by_menu.entry(menu).or_default() += 1;
        let choice = sound_for(menu, event);
        let sound = match choice {
            Choice::Named(e) | Choice::Loop(e, _) => ui.by_edid.get_or_insert_with(|| by_edid(sfx)).get(e).copied(),
            Choice::Item { item, up } => world.as_deref_mut().and_then(|w| w.item_sound_of(Some(sfx), item, up)),
            Choice::Caps { up } => world.as_deref_mut().and_then(|w| w.item_sound_of(Some(sfx), CAPS, up)),
            Choice::Consume(item) => ui.consume.get(&item).copied(),
            Choice::Silent(why) => {
                *ui.stats.silent.entry(why).or_default() += 1;
                info!("ui sounds: {} {}: no sound ({why})", menu.name(), event.name());
                continue;
            }
        };
        let name = sound.map(|s| sfx.forms.sounds.get(&s).and_then(|x| x.edid.clone()).unwrap_or_else(|| s.to_string()));
        let line = format!("ui sounds: {} {}: {}", menu.name(), event.name(), name.as_deref().unwrap_or("none found"));
        // The lists' moves are many; the main and pause menus' few (their smoke checks read them).
        if event == UiEvent::Focus && !matches!(menu, UiMenu::MainMenu | UiMenu::Pause) {
            debug!("{line}");
        } else {
            info!("{line}");
        }
        match (sound, name) {
            (Some(sound), Some(_)) if matches!(choice, Choice::Loop(_, false)) => {
                loops.write(crate::sfx::LoopSound { sound, on: false });
            }
            (Some(sound), Some(name)) if matches!(choice, Choice::Loop(_, true)) => {
                *ui.stats.played.entry(name).or_default() += 1;
                loops.write(crate::sfx::LoopSound { sound, on: true });
            }
            (Some(sound), Some(name)) => {
                *ui.stats.played.entry(name).or_default() += 1;
                out.write(PlaySound { sound, at: None });
            }
            _ => ui.stats.missing += 1,
        }
    }
}

/// SOUN FormIDs by Editor ID.
fn by_edid(sfx: &Sfx) -> HashMap<String, FormId> {
    sfx.forms.sounds.iter().filter_map(|(&id, s)| s.edid.clone().map(|e| (e, id))).collect()
}

fn report(mut exit: MessageReader<AppExit>, ui: Option<Res<UiSounds>>, world: Option<Res<WorldSounds>>) {
    let (Some(_), Some(ui)) = (exit.read().last(), ui) else { return };
    let s = &ui.stats;
    let list = |m: Vec<String>| if m.is_empty() { "none".to_string() } else { m.join(", ") };
    let played: u32 = s.played.values().sum();
    info!(
        "ui sounds summary: {played} played ({}); events by menu: {}; {} without a sound ({}); {} sounds not found; {} items' own sounds held back for Take All",
        list(s.played.iter().map(|(k, n)| format!("{k} {n}")).collect()),
        list(s.by_menu.iter().map(|(k, n)| format!("{} {n}", k.name())).collect()),
        s.silent.values().sum::<u32>(),
        list(s.silent.iter().map(|(k, n)| format!("{k}: {n}")).collect()),
        s.missing,
        world.map_or(0, |w| w.stats.taken_quietly)
    );
}

pub struct UiSoundsPlugin;

impl Plugin for UiSoundsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, init).add_systems(Update, play.run_if(resource_exists::<UiSounds>).after(PlaySet::Ui).before(PlaySet::Sync)).add_systems(Last, report);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_access_waits_for_pose_restoration_and_cancels_silently() {
        let opened = UiSound { menu: UiMenu::Inventory, event: UiEvent::Opened };
        let closed = UiSound { menu: UiMenu::Inventory, event: UiEvent::Closed };
        let mut pending = false;
        let mut events = vec![opened];
        inventory_access(&mut events, &mut pending, true, true);
        assert!(pending && events.is_empty());
        inventory_access(&mut events, &mut pending, true, true);
        assert!(pending && events.is_empty());
        inventory_access(&mut events, &mut pending, true, false);
        assert_eq!(events, vec![opened]);
        assert!(!pending);
        events.clear();
        inventory_access(&mut events, &mut pending, true, false);
        assert!(events.is_empty());
        events.push(opened);
        inventory_access(&mut events, &mut pending, true, true);
        events.push(closed);
        inventory_access(&mut events, &mut pending, false, true);
        assert!(!pending && events.is_empty());
        // [design: this file] Screen-menu, ordinary and disabled-warm openings are immediate.
        events.push(opened);
        inventory_access(&mut events, &mut pending, true, false);
        assert_eq!(events, vec![opened]);
    }

    #[test]
    fn the_files_decide_where_they_name_a_sound() {
        // list_box_template.xml: rows click UIMenuOK, take the pointer with UIMenuFocus.
        assert_eq!(sound_for(UiMenu::Barter, UiEvent::Picked), Choice::Named("UIMenuOK"));
        for m in [UiMenu::Container, UiMenu::Barter, UiMenu::Dialogue, UiMenu::Settings] {
            assert_eq!(sound_for(m, UiEvent::Focus), Choice::Named("UIMenuFocus"));
        }
        // dialog_menu.xml silences the topics' click.
        assert!(matches!(sound_for(UiMenu::Dialogue, UiEvent::Answered), Choice::Silent(_)));
        // start_menu.xml: toggles UIMenuPrevNext, the back button UIMenuCancel.
        assert_eq!(sound_for(UiMenu::Settings, UiEvent::Flipped), Choice::Named("UIMenuPrevNext"));
        assert_eq!(sound_for(UiMenu::Settings, UiEvent::Back), Choice::Named("UIMenuCancel"));
    }

    #[test]
    fn the_chargen_and_terminal_menus_sound() {
        // Vigor Tester: the S.P.E.C.I.A.L. book's records.
        assert_eq!(sound_for(UiMenu::Vigor, UiEvent::Switch), Choice::Named("OBJBookSpecialPageTurn"));
        assert_eq!(sound_for(UiMenu::Vigor, UiEvent::Value { up: true }), Choice::Named("OBJBookSpecialNumber"));
        assert_eq!(sound_for(UiMenu::Vigor, UiEvent::Value { up: false }), Choice::Named("OBJBookSpecialNumberDown"));
        assert_eq!(sound_for(UiMenu::Vigor, UiEvent::Accepted), Choice::Named("OBJBookSpecialNumberFinished"));
        // Tag skills and traits: hover, take, reset, Done (char_gen_menu.xml's Done click).
        for m in [UiMenu::TagSkills, UiMenu::Traits] {
            assert_eq!(sound_for(m, UiEvent::Focus), Choice::Named("UIMenuFocus"));
            assert_eq!(sound_for(m, UiEvent::Picked), Choice::Named("UIMenuPrevNext"));
            assert_eq!(sound_for(m, UiEvent::Reset), Choice::Named("UIMenuCancel"));
            assert_eq!(sound_for(m, UiEvent::Accepted), Choice::Named("UIMenuOK"));
            assert_eq!(sound_for(m, UiEvent::Refused), Choice::Named("UIMenuCancel"));
        }
        // The level-up menu.
        assert_eq!(sound_for(UiMenu::LevelUp, UiEvent::Opened), Choice::Named("MUSSuccess"));
        assert_eq!(sound_for(UiMenu::LevelUp, UiEvent::Value { up: true }), Choice::Named("UIMenuPrevNext"));
        assert_eq!(sound_for(UiMenu::LevelUp, UiEvent::Value { up: false }), Choice::Named("UIMenuPrevNext"));
        assert_eq!(sound_for(UiMenu::LevelUp, UiEvent::Picked), Choice::Named("UIMenuPrevNext"));
        assert_eq!(sound_for(UiMenu::LevelUp, UiEvent::Switch), Choice::Named("UIMenuOK"));
        assert_eq!(sound_for(UiMenu::LevelUp, UiEvent::Accepted), Choice::Named("UIMenuOK"));
        assert_eq!(sound_for(UiMenu::LevelUp, UiEvent::Refused), Choice::Named("UIMenuCancel"));
        // Terminals: the hacking records.
        assert_eq!(sound_for(UiMenu::Terminal, UiEvent::Focus), Choice::Named("UIHackingCharScroll"));
        assert_eq!(sound_for(UiMenu::Terminal, UiEvent::Picked), Choice::Named("UIHackingCharEnter"));
        assert_eq!(sound_for(UiMenu::Terminal, UiEvent::Granted), Choice::Named("UIHackingPassGood"));
        assert_eq!(sound_for(UiMenu::Terminal, UiEvent::Refused), Choice::Named("UIHackingPassBad"));
    }

    #[test]
    fn items_and_caps_sound_as_themselves() {
        let item = FormId(0x15169);
        assert_eq!(sound_for(UiMenu::Inventory, UiEvent::Used { item }), Choice::Consume(item));
        assert_eq!(sound_for(UiMenu::Inventory, UiEvent::Equipped { item, on: false }), Choice::Item { item, up: false });
        assert_eq!(sound_for(UiMenu::Barter, UiEvent::Traded { caps: 12 }), Choice::Caps { up: true });
        assert_eq!(sound_for(UiMenu::Barter, UiEvent::Traded { caps: -3 }), Choice::Caps { up: false });
        assert_eq!(sound_for(UiMenu::Barter, UiEvent::Traded { caps: 0 }), Choice::Named("UIMenuOK"));
        assert_eq!(sound_for(UiMenu::Container, UiEvent::TakeAll), Choice::Named("UIItemTakeAll"));
        assert!(matches!(sound_for(UiMenu::Container, UiEvent::Opened), Choice::Silent(_)));
    }

    #[test]
    fn repair_services_sounds() {
        assert_eq!(sound_for(UiMenu::Repair, UiEvent::Picked), Choice::Named("UIRepairWeapon"));
        assert_eq!(sound_for(UiMenu::Repair, UiEvent::Focus), Choice::Named("UIMenuFocus"));
        assert_eq!(sound_for(UiMenu::Repair, UiEvent::Opened), Choice::Named("UIMenuMode"));
        assert_eq!(sound_for(UiMenu::Repair, UiEvent::Back), Choice::Named("UIMenuCancel"));
        assert_eq!(sound_for(UiMenu::Repair, UiEvent::Refused), Choice::Named("UIMenuCancel"));
    }

    #[test]
    fn lockpicking_sounds_by_their_records() {
        assert_eq!(sound_for(UiMenu::Lockpick, UiEvent::Opened), Choice::Named("UILockpickingEnter"));
        assert_eq!(sound_for(UiMenu::Lockpick, UiEvent::PinBroke), Choice::Named("UILockpickingPickBreak"));
        assert_eq!(sound_for(UiMenu::Lockpick, UiEvent::Unlocked), Choice::Named("UILockpickingUnlock"));
        assert_eq!(sound_for(UiMenu::Lockpick, UiEvent::Strain { on: true }), Choice::Loop("UILockpickingPickTensionLPM", true));
        assert_eq!(sound_for(UiMenu::Lockpick, UiEvent::Refused), Choice::Named("DRSLocked"));
        // Other menus' refusals stay as they were.
        assert_eq!(sound_for(UiMenu::Barter, UiEvent::Refused), Choice::Named("UIMenuCancel"));
    }

    fn seen() -> Seen {
        Seen::default()
    }

    #[test]
    fn menus_opening_and_closing_are_heard() {
        let open = Seen { menu: Some(Mode::Barter(FormId(7))), ..seen() };
        assert_eq!(watched(&seen(), &open), vec![UiSound { menu: UiMenu::Barter, event: UiEvent::Opened }]);
        assert_eq!(watched(&open, &seen()), vec![UiSound { menu: UiMenu::Barter, event: UiEvent::Closed }]);
        assert!(watched(&open, &open).is_empty());
    }

    #[test]
    fn the_wait_menu_is_heard_from_its_state() {
        let ev = |was: Option<WaitSeen>, now| watched(&Seen { wait: was, ..seen() }, &Seen { wait: now, ..seen() }).into_iter().map(|u| (u.menu, u.event)).collect::<Vec<_>>();
        assert_eq!(ev(None, Some((true, 1, None))), vec![(UiMenu::Sleep, UiEvent::Opened)]);
        assert_eq!(ev(Some((false, 1, None)), Some((false, 2, None))), vec![(UiMenu::Wait, UiEvent::Slider)]);
        assert_eq!(ev(Some((false, 2, None)), Some((false, 2, Some((2, 2))))), vec![(UiMenu::Wait, UiEvent::Accepted)]);
        // The hours going by move no slider.
        assert_eq!(ev(Some((false, 2, Some((2, 2)))), Some((false, 2, Some((2, 1))))), vec![]);
        assert_eq!(ev(Some((false, 2, Some((2, 0)))), None), vec![(UiMenu::Wait, UiEvent::Finished)]);
        assert_eq!(ev(Some((false, 2, Some((2, 1)))), None), vec![(UiMenu::Wait, UiEvent::Back)]);
        assert_eq!(ev(Some((false, 2, None)), None), vec![(UiMenu::Wait, UiEvent::Back)]);
    }

    #[test]
    fn dialogue_choices_and_the_pause_menu() {
        let list = |i| Seen { choices: Some((vec!["a".into(), "b".into()], i)), ..seen() };
        assert_eq!(watched(&list(0), &list(1)), vec![UiSound { menu: UiMenu::Dialogue, event: UiEvent::Focus }]);
        assert!(watched(&list(1), &list(1)).is_empty());
        assert_eq!(watched(&list(1), &seen()), vec![UiSound { menu: UiMenu::Dialogue, event: UiEvent::Answered }]);
        let p = |open, row| Seen { pause: (open, row), ..seen() };
        assert_eq!(watched(&p(false, None), &p(true, None))[0].event, UiEvent::Opened);
        assert_eq!(watched(&p(true, None), &p(true, Some(0))), vec![UiSound { menu: UiMenu::Settings, event: UiEvent::Opened }]);
        assert_eq!(watched(&p(true, Some(0)), &p(true, Some(2))), vec![UiSound { menu: UiMenu::Settings, event: UiEvent::Focus }]);
        assert_eq!(watched(&p(true, Some(2)), &p(true, None)), vec![UiSound { menu: UiMenu::Settings, event: UiEvent::Back }]);
        assert_eq!(watched(&p(true, Some(2)), &p(false, None)), vec![UiSound { menu: UiMenu::Pause, event: UiEvent::Back }]);
    }
}
