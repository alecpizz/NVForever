//! The Pip-Boy's ITEMS page drawn from `menus\main\inventory_menu.xml`, behind
//! `[UI] TileMenus=InventoryMenu` [design: ui-tile-tree.md §6.2 step 3; the contract:
//! docs/spec-notes/pipboy.md §5.2.1; issues #1513, #1468]. On by default [design, #2215;
//! super::vanilla::TileMenus::for_run]. Off, none of this runs and the hand-built page
//! (`crate::pipboy::items`) draws.
//!
//! - **The one decision** ([`InventoryTiles::ready`]): the switch on, ITEMS showing with the repair
//!   list and the mod menu closed, the menu files read, the file's tiles there ([`built`]), the
//!   tabs made and a usable measured layout ([`usable`]). Painting (`super::draw`), hand-built
//!   suppression (`crate::pipboy::frame::draw`), the prompt and input read it and nothing else,
//!   and [`settle`] publishes it each frame before any of them. A pass that fails leaves the whole
//!   hand-built page, says why once, and is tried again the next frame; after
//!   [`FAILING_PASSES`] in a row, only when what it would show or the tree changes ([`TreeMark`]).
//! - **The rules stay the hand-built page's** (`crate::menu::model`, `crate::pipboy::items`,
//!   `crate::pipboy::repair`, `crate::pipboy::item_mod`, `crate::pipboy::hotkey_panel`). This module
//!   writes values as the file's "set in code" traits and says what a tile stands for; commands,
//!   sounds and events go where the hand-built page sends them. Only the list's scroll is its own
//!   (`super::item_menu`), which the model never had in units.
//! - **The list** is `super::item_menu`'s with one side: the container's path is empty, which no
//!   tile answers to. A scrolled list stands partly outside its window by design, so [`usable`]
//!   asks of the rows whose words lie whole inside it, and of at least one.
//! - **The tabs** are template rows here, unlike STATS' fixed tiles [documented: the file]. Their
//!   words are their settings' where the load order defines them, [`TAB_WORDS`] otherwise
//!   ([`tab_words`]). A tab's `x` follows its inner rect's, which follows `_x`, and the file
//!   leaves `_x` to code [documented: the prefabs; observed: private job run07_inventory, 1280x720]: unwritten,
//!   all five stood at one place, which is what the first run met. So [`place_tabs`] writes `_x` and
//!   `_LeftLineLength` and nothing else; what shows as selected follows `_CurrentTab` in the file. A
//!   tab that does not show is a fallback; one that does not take the pointer is warned of and the
//!   page stays: the keys and the device's knob still turn the tabs.
//! - **The buttons**: on a PC Repair shows on every tab and Mod on Weapons, row or not; Equip, Drop
//!   and Hotkey are a controller's in the file; inside a
//!   Misc folder Cancel stands in Repair's place [observed: private job run07c_inventory, 1280x720, private job run07c_inventory_b, 1280x720]. The original's keyring has
//!   no "Back" row and stands last on Misc: NVForever's rows are the model's, a known difference.
//!   Their commands stay reachable by a script's click. [design, #2312] With Controller,
//!   the optional Hotkey button shows RB/R1 and opens the shared modal; Equip/Drop stay hidden.
//! - **Words too wide for their card** give way where the title is still a built-in label: a
//!   plugin's setting stands. [`TIGHT_TITLES`].
//! - **Page content only**, as `super::stats_menu`: the page paints into the Pip-Boy's image, whose
//!   background and scanlines are the shell's, so `IM_Background` stays hidden in both frames.
//!   The screen menu has no title and its heading line runs on [design: the owner, 2026-10-31,
//!   `stats_menu::write_frame_of`].
//! - **The hotkey panel** shows while a hotkey's key is held [design: the owner 2026-10-26, `hotkey_panel`
//!   module notes]: the owner's own panel at the owner's place (`hotkey_panel::draw_over_tiles`), over
//!   the page, whose picture, badge, skill and buttons give way. The file's `IM_HotKeyWheel` stays
//!   hidden: no capture has seen it open, so it is a known difference. [design, #2312]
//!   RB also opens this shared panel, with modal D-pad/A/B input before the tile feed.
//! - **The repair list and the mod menu** have their own files, not read here: while one is open
//!   the page is not eligible and the hand-built body draws it, as it does today.
//! - **The prompt** is `QuantityMenu`'s while the page is ready (`super::menus::quantity_tiled`),
//!   drawn on the window; [`feed`] takes nothing under it.
//! - **Input** ([`feed`], before the scripts): the pointer is the Pip-Boy's own, in canvas units
//!   (`crate::pipboy::PipBoyScreen::pointer`), read and never written here. A row the pointer
//!   moves onto is selected unless the wheel holds the selection; a click on a row sends what the
//!   hand-built row sends (`items::on_row_click`: `Command::Select`, or with a hotkey's key held
//!   the assignment), a right click the hand-built drop [owner 2026-11-02, #1393]. The keys and
//!   the wheel stay `items::keys`' and `pipboy::keys`' ([`HAND_BUILT_KEYS`]); a file's binding acts
//!   for a script's tap, not for a physical press.
//! - **Measuring**: with `NVF_PLAY_TEST_TILE_COUNTS` a line a frame for each pass, `tile counts:
//!   InventoryMenu pass=<early|late> writes=<n> changed=<n> micros=<n>`, the late one with
//!   `rows=<n> rows_in_clip=<n> hit_tests=<n> candidates=<n> pieces=<n>` (`super` module notes).

use super::draw::{Look, Piece};
use super::input::{frame_keys, log_report, Devices};
use super::item_card;
use super::item_menu::{self, on, ListShown, MenuTiles, Paths, Snapshot};
use super::list::{self, BarPiece, RowTile, Wanted};
use super::menus::{self, SizesWritten, Tiled};
use super::stats_menu::{self, find_named, modal_over, resolve_tile, shows_whole, FlatChanges, PageFonts};
use super::vanilla::TileMenus;
use crate::hotkeys::Hotkeys;
use crate::menu::model::{Command, MenuModel, Mode, Side};
use crate::menu::{ActiveMenu, MenuInput};
use crate::pipboy::hotkey_panel::{self, HotkeyHold};
use crate::pipboy::item_mod::{ModInput, PipItemMod};
use crate::pipboy::items::{self, Info};
use crate::pipboy::repair::{self, PipRepair, RepairInput};
use crate::pipboy::sounds::TabHovered;
use crate::pipboy::stats::read::Forms;
use crate::pipboy::{Frame, Page, PipBoy, PipBoyAid, PipBoyInput, PipBoyScreen};
use crate::script::ScriptState;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use fnv_sim::forms::ItemKind;
use fnv_sim::FormId;
use fnv_ui::{TileId, Ui, Value};
use std::collections::{HashMap, HashSet};

/// The menu's name [documented: inventory_menu.xml].
pub const INVENTORY_MENU: &str = "InventoryMenu";
/// Test aid: a tile or template of the file to take as missing, as `stats_menu::CUT_AID`:
/// `<name>`, or `<name>@<seconds>` to stop at that time, for the page's recovery.
pub const CUT_AID: &str = "NVF_PLAY_TEST_INVENTORY_TILE_CUT";

macro_rules! glow {
    ($path:literal) => {
        concat!("InventoryMenu/GLOW_BRANCH/", $path)
    };
}
macro_rules! main_rect {
    ($path:literal) => {
        concat!("InventoryMenu/GLOW_BRANCH/IM_MainRect/", $path)
    };
}

// The tiles the model writes and the input reads, by path [documented: inventory_menu.xml].
const BACKGROUND: &str = glow!("IM_Background");
const TABLINE: &str = glow!("IM_Tabline");
const TITLE: &str = main_rect!("IM_HeadlineRect/IM_Headline_Title");
const LINE: &str = main_rect!("IM_HeadlineRect/IM_Headline_h2");
/// Wg, HP, DT and Caps, as [`Shown::figures`].
const HEADLINE: [&str; 4] = [
    main_rect!("IM_HeadlineRect/IM_Headline_PlayerWGInfo"),
    main_rect!("IM_HeadlineRect/IM_Headline_PlayerHPInfo"),
    main_rect!("IM_HeadlineRect/IM_Headline_PlayerDTInfo"),
    main_rect!("IM_HeadlineRect/IM_Headline_PlayerCapsInfo"),
];
/// DR, which this game's page has no card for [documented: `crate::pipboy::items`' heading]; a
/// file without it serves.
const HEADLINE_DR: &str = main_rect!("IM_HeadlineRect/IM_Headline_PlayerDRInfo");
const LIST: &str = main_rect!("IM_InventoryList");
const WHEEL: &str = main_rect!("IM_HotKeyWheel");
const BUTTON_RECT: &str = main_rect!("IM_ButtonRect");
const ICON: &str = main_rect!("IM_ItemIcon");
const BADGE: &str = main_rect!("IM_ItemIconBadge");
const SKILL_REQ: &str = main_rect!("IM_StrReq");
const CARD: &str = main_rect!("IM_ItemInfoRect");
const TEMPLATE: &str = "IM_InventoryListTemplate";
const ROW: &str = "IM_InventoryListTemplateRect";
const ROW_TEXT: &str = "ListItemText";
const ROW_MARK: &str = "IM_Template_ItemMarker";
const TAB_TEMPLATE: &str = "TabButtonTemplate";
/// The tile a tab's template makes [documented: the file]; it keeps this name [design until the
/// capture says what the original names its tabs].
const TAB_ROW: &str = "TabButton";
/// Inside a tab, as STATS' fixed tabs have them [hypothesis for the template's: the same prefab].
const TAB_INNER: &str = "stb_text_box";
/// The setting whose word fits the condition card's room before its meter.
const CONDITION_TITLE: &str = "sStatsCNDAbbrev";
/// The setting for the Repair button's word on an item that needs no repair, read first, as the
/// hand-built hint reads it (`crate::pipboy::repair::draw_hint`, #1902): the English masters define
/// none, a localized install's language plugin does [observed: docs/spec-notes/pipboy.md §4].
const MAINTAIN_SETTING: &str = "sMaintainItem";
/// The word on the Repair button for an item that needs no repair when the load order defines no
/// [`MAINTAIN_SETTING`]: NVForever's own [design: LEGAL.md; the strings table holds none, and the
/// data's help text has only the verb's other form]. The hand-built hint's fallback is its own
/// (`fnv_game::repair::hint`).
const REPAIR_UPKEEP: &str = "Upkeep";
/// The picture's place and side: `x`, side [documented: inventory_menu.xml]; on an ammunition row
/// [observed: private job run07c_inventory, 1280x720, on ammunition no weapon carried takes; hypothesis: on all].
const ICON_PLACE: (f64, f64) = (450.0, 380.0);
const ICON_PLACE_AMMO: (f64, f64) = (410.0, 350.0);
/// The effects' card under two rows of cards, where the card's own sum gives less [observed: private job run07_inventory_pipboy, 1280x720].
const EFFECTS_LOW: f64 = 110.0;

/// The tabs' words when the load order defines no [`TAB_SETTINGS`] [design: LEGAL.md]. The English
/// masters define none of those settings [observed: read-only scan, capture notes; pipboy.md §4],
/// and the strings table has none, as no menu file names one. "Apparel" is the data's own word,
/// spelled as it spells it [observed: FalloutNV.esm MESG `HelpApparel`, repair.md §1 and §5; #1740].
/// "Weapons", "Aid", "Misc" and "Ammo" cite no record: they are generic words for the kinds of item,
/// chosen here as NVForever's own. All five are the same as `crate::pipboy::Page::tabs`' fallbacks,
/// whose tag says they were read off the owner's screenshots; that list is not read here, and the
/// likeness is for the maintainer to see in the pull request.
pub const TAB_WORDS: [&str; 5] = ["Weapons", "Apparel", "Aid", "Misc", "Ammo"];
/// The tabs' settings, read first through `crate::pipboy::labels` as the hand-built page reads them
/// (`crate::pipboy::Page::tabs`, #1902): a localized install's language plugin defines them
/// [observed: docs/spec-notes/pipboy.md §4].
const TAB_SETTINGS: [&str; 5] = ["sInventoryWeapons", "sInventoryApparel", "sInventoryAid", "sInventoryMisc", "sInventoryAmmo"];

/// The tabs' words: each setting's text when the load order defines it, else [`TAB_WORDS`]'.
fn tab_words() -> [&'static str; 5] {
    tab_words_by(crate::pipboy::labels::label)
}

/// [`tab_words`] through a lookup of (setting, fallback), so tests need no load order.
fn tab_words_by(look: impl Fn(&str, &'static str) -> &'static str) -> [&'static str; 5] {
    std::array::from_fn(|i| look(TAB_SETTINGS[i], TAB_WORDS[i]))
}

/// NVForever's own short words for titles whose built-in label leaves a card's value no room: the
/// value then runs into the next card, as it may not wrap with no space in it [observed: smoke `tiles-items`, 2026-11-07,
/// "Weight" putting Wg's value at the title's end plus 15 and past HP's title]; the condition's
/// is [`CONDITION_TITLE`]'s.
const TIGHT_TITLES: [(&str, &str); 2] = [(HEADLINE[0], "Wt"), (main_rect!("IM_ItemInfoRect/StrengthReqInfo"), "Str")];
const CONDITION_CARD: &str = main_rect!("IM_ItemInfoRect/ConditionInfo");

/// The buttons, by their tiles' names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Equip,
    Drop,
    Repair,
    Hotkey,
    Mod,
    Cancel,
}

const BUTTONS: [(&str, Button); 6] = [("IM_EquipButton", Button::Equip), ("IM_DropButton", Button::Drop), ("IM_RepairButton", Button::Repair), ("IM_HotkeyButton", Button::Hotkey), ("IM_ModButton", Button::Mod), ("IM_CancelButton", Button::Cancel)];

/// The list's paths for `super::item_menu`: one list, on the player's side. Clicks are told apart
/// here ([`items_tile`]), not by a table of names there.
pub(super) const PATHS: Paths = Paths { menu: INVENTORY_MENU, lists: ["", LIST], titles: ["", ""], template: TEMPLATE, row: ROW, row_text: ROW_TEXT, row_mark: ROW_MARK, targets: &[] };

fn button_path(name: &str) -> String {
    format!("{BUTTON_RECT}/{name}")
}

/// Every path the model writes or input reads.
fn required_paths() -> Vec<String> {
    let mut paths: Vec<String> = [INVENTORY_MENU, BACKGROUND, TABLINE, TITLE, LINE, LIST, BUTTON_RECT, ICON, BADGE, SKILL_REQ, CARD].iter().map(|s| (*s).to_string()).collect();
    paths.extend(HEADLINE.iter().map(|s| (*s).to_string()));
    paths.push(format!("{LIST}/{}", list::HIGHLIGHT));
    for piece in [list::MARKER, list::UP, list::DOWN, list::PAGE_UP, list::PAGE_DOWN] {
        paths.push(format!("{LIST}/{}/{piece}", list::SCROLLBAR));
    }
    for (name, button) in BUTTONS {
        // [design, #2312] Hotkey is optional: older/cut-down menus still serve.
        if button != Button::Hotkey {
            paths.push(button_path(name));
            paths.push(format!("{}/button_text", button_path(name)));
        }
    }
    paths.extend(item_card::CARDS.iter().chain(&[item_card::EFFECTS, item_card::CONDITION_ARROWS, item_card::CONDITION_METER, item_card::CONDITION_METER_IMAGE]).map(|c| format!("{CARD}/{c}")));
    for card in item_card::CARDS.iter().chain(&[item_card::EFFECTS]) {
        paths.extend(item_card::CARD_TEXTS.map(|t| format!("{CARD}/{card}/{t}")));
    }
    paths
}

/// Whether the file has every tile and template the page needs; a file a mod restructured keeps
/// the hand-built page whole. `cut`: [`CUT_AID`].
pub fn built(ui: &Ui, cut: Option<&str>) -> Tiled {
    let paths = required_paths();
    if let Some(cut) = cut {
        if cut.eq_ignore_ascii_case(TEMPLATE) || cut.eq_ignore_ascii_case(TAB_TEMPLATE) {
            return Tiled::No(format!("{cut} missing"));
        }
        if let Some(path) = paths.iter().find(|p| p.rsplit('/').next().is_some_and(|n| n.eq_ignore_ascii_case(cut))) {
            return Tiled::No(format!("{path} missing"));
        }
    }
    for path in &paths {
        if resolve_tile(ui, path).is_none() {
            return Tiled::No(format!("{path} missing"));
        }
    }
    // Exact vanilla check first (`<image>` marker); accept `<rect>` when a modded row template
    // implements the equipped marker as a composite container [design].
    let list_built = match list::template_has(ui, INVENTORY_MENU, TEMPLATE, ROW, &[("text", ROW_TEXT), ("image", ROW_MARK)]) {
        Tiled::Yes => Tiled::Yes,
        Tiled::No(reason) if reason == format!("{TEMPLATE}/{ROW}/{ROW_MARK} missing") => {
            list::template_has(ui, INVENTORY_MENU, TEMPLATE, ROW, &[("text", ROW_TEXT), ("rect", ROW_MARK)])
        }
        other => other,
    };
    if !list_built.yes() {
        return list_built;
    }
    let tab_template = ui.menu(INVENTORY_MENU).is_some_and(|m| ui.tile(m).templates.iter().any(|t| t.name.eq_ignore_ascii_case(TAB_TEMPLATE)));
    if !tab_template {
        return Tiled::No(format!("{TAB_TEMPLATE} missing"));
    }
    Tiled::Yes
}

/// The Repair button as shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RepairButton {
    pub shown: bool,
    /// R does something: lit, the file dimming it otherwise by `target`.
    pub usable: bool,
    /// The word when it is not the file's (`fnv_game::repair::hint`, the hand-built hint's rule).
    pub word: Option<&'static str>,
}

/// The keys' hints beside the buttons' words (`_PCButtonText`), for the buttons that show.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hints {
    pub repair: String,
    pub modify: String,
    pub cancel: String,
    /// [design, #2312] Empty with keys; the current pad's RB/R1 otherwise.
    pub hotkey: String,
}

/// What the page shows of the model, as written last. A plain snapshot, so tests need no world.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    /// The list, the item's picture and its figures.
    pub list: Snapshot,
    pub tab: usize,
    pub device: bool,
    /// Wg, HP, DT and Caps.
    pub figures: [String; 4],
    pub repair: RepairButton,
    /// The Mod button: on the Weapons tab.
    pub modify: bool,
    /// A row is selected, lighting Mod.
    pub modify_lit: bool,
    /// The selected row is ammunition (the picture's place).
    pub ammo: bool,
    /// The selected row can be worn, wielded or used, and is not broken [observed: private job run07_inventory_pipboy, 1280x720,
    /// private job run07c_inventory, 1280x720]: the root's `_EquippableItem`, which the file's
    /// marker beside the highlighted row follows.
    pub equippable: bool,
    /// [design, #2312; hotkeys::can_hold] The selected row accepts a hotkey.
    pub hotkey_lit: bool,
    /// A Misc folder is open [hypothesis: the root's `_KeyringOpen`, which Cancel follows].
    pub keyring: bool,
    pub hints: Hints,
    /// [`CONDITION_TITLE`]'s word.
    pub condition_title: String,
    /// The hotkey's key held: its slot (0 is key 1). The panel is `hotkey_panel`'s.
    pub panel: Option<usize>,
}

impl Shown {
    /// What `m` shows; `card` the selected row's figures, without DPS as on the hand-built page.
    #[allow(clippy::too_many_arguments)]
    pub fn of(m: &MenuModel, card: Option<Info>, tab: usize, device: bool, figures: [String; 4], repair: RepairButton, (hints, condition_title): (Hints, String), panel: Option<usize>) -> Shown {
        let selected = m.selected_row(Side::Player);
        let list = ListShown {
            rows: item_menu::rows_shown(&m.list(Side::Player).rows),
            category: m.category[Side::Player as usize],
            selected: selected.map(|_| m.selected[Side::Player as usize]),
            filtered: m.filtered[Side::Player as usize],
        };
        Shown {
            list: Snapshot {
                lists: [ListShown::default(), list],
                titles: Default::default(),
                weight: String::new(),
                focus: Side::Player,
                pointer_pick: m.pointer_pick,
                asking: m.quantity.is_some(),
                navigated: m.navigated,
                card,
                icon: selected.and_then(|r| r.icon.clone()),
                take_all: false,
                take_all_hint: String::new(),
                note: None,
                ysi_icons: m.ysi_icons,
            },
            tab,
            device,
            figures,
            repair,
            modify: tab == 0 && (m.folder.is_none() || matches!(m.folder, Some(crate::menu::model::Folder::Ysi(_)))),
            modify_lit: selected.is_some_and(|r| r.folder.is_none()),
            ammo: selected.is_some_and(|r| r.kind == Some(ItemKind::Ammo)),
            equippable: selected.is_some_and(|r| matches!(r.kind, Some(ItemKind::Weapon | ItemKind::Armor | ItemKind::Aid)) && r.condition.is_none_or(|c| c > 0.0)),
            hotkey_lit: selected.is_some_and(|r| r.folder.is_none() && r.kind.is_some_and(crate::hotkeys::can_hold)),
            // Only vanilla Misc folders (Keyring / Caravan Cards) swap IM_RepairButton for
            // IM_CancelButton (`_KeyringOpen`). Inside `Folder::Ysi(_)`, IM_RepairButton
            // stays visible so the two buttons never overlap on the same slot [design].
            keyring: matches!(m.folder, Some(crate::menu::model::Folder::Keys | crate::menu::model::Folder::Cards)),
            hints,
            condition_title,
            panel,
        }
    }
}

/// Sets a trait by path as the game's code does (`Ui::write_changed`): what a script set
/// meanwhile stands until the model changes.
fn put(ui: &mut Ui, path: &str, name: &str, v: Value) {
    if let Some(t) = resolve_tile(ui, path) {
        ui.write_changed(t, name, v);
    }
}

fn text(s: &str) -> Value {
    Value::Str(s.to_string())
}

/// Write what the page shows of `s`. A list's places are `item_menu::settle`'s, once the texts
/// are measured.
pub fn write_model(ui: &mut Ui, s: &Shown, view: &mut MenuTiles, file_repair_word: &mut Option<String>) {
    item_menu::write_lists(ui, &s.list, view);
    put(ui, BACKGROUND, "visible", on(false));
    put(ui, HEADLINE_DR, "visible", on(false));
    write_tight_titles(ui, &s.condition_title);
    for (card, figure) in HEADLINE.iter().zip(&s.figures) {
        put(ui, card, "_Value", text(figure));
    }
    put(ui, INVENTORY_MENU, "_EquippableItem", on(s.equippable));
    put(ui, INVENTORY_MENU, "_KeyringOpen", on(s.keyring));
    // The panel hides the picture, the badge, the skill and the buttons; the figures stay
    // [design: `crate::pipboy::hotkey_panel`'s notes].
    let panel = s.panel.is_some();
    let card = s.list.card.as_ref();
    let (icon_x, icon_side) = if s.ammo { ICON_PLACE_AMMO } else { ICON_PLACE };
    put(ui, ICON, "x", Value::Num(icon_x));
    put(ui, ICON, "width", Value::Num(icon_side));
    put(ui, ICON, "height", Value::Num(icon_side));
    put(ui, ICON, "filename", text(s.list.icon.as_deref().unwrap_or_default()));
    put(ui, ICON, "visible", on(!panel && s.list.icon.is_some()));
    let badge = card.and_then(|c| c.badge.as_deref());
    put(ui, BADGE, "filename", text(badge.unwrap_or_default()));
    put(ui, BADGE, "visible", on(!panel && badge.is_some()));
    let skill = card.and_then(|c| c.skill_req);
    put(ui, SKILL_REQ, "string", Value::Str(skill.map_or_else(String::new, |n| n.to_string())));
    put(ui, SKILL_REQ, "visible", on(!panel && skill.is_some()));
    item_card::write(ui, CARD, card);
    // No row, no figures, the card rect included [observed: private job run07_inventory, 1280x720].
    put(ui, CARD, "visible", on(card.is_some()));
    if card.is_some_and(|c| c.condition.is_some() || c.beside.is_some()) {
        if let Some(effects) = resolve_tile(ui, &format!("{CARD}/{}", item_card::EFFECTS)) {
            ui.set_trait(effects, "y", Value::Num(EFFECTS_LOW));
        }
    }
    write_buttons(ui, s, file_repair_word);
    put(ui, WHEEL, "visible", on(false));
    put(ui, BUTTON_RECT, "visible", on(s.panel.is_none()));
}

/// Titles too wide for their card ([`TIGHT_TITLES`]), once, only while the title is the strings
/// table's label: a plugin's setting or a script's word stands (`Ui::built_in_label`).
fn write_tight_titles(ui: &mut Ui, condition: &str) {
    for (path, word) in TIGHT_TITLES.into_iter().chain([(CONDITION_CARD, condition)]) {
        let Some(t) = resolve_tile(ui, path) else { continue };
        if ui.built_in_label(t, "_Title").is_some() {
            ui.set_trait(t, "_Title", text(word));
        }
    }
}

/// The buttons' `visible`, hints, Repair's word and the `target`s the file dims by [observed: the
/// capture]. [design, #2312] Hotkey shows with Controller; Equip and Drop stay hidden.
/// Repair's word follows the
/// condition and its light a spare, two questions: the longer word can be lit. Mod shows on the Weapons
/// tab, lit with a row, whatever the row, so the owner's "always bright" holds for any row.
fn write_buttons(ui: &mut Ui, s: &Shown, file_repair_word: &mut Option<String>) {
    for (name, button) in BUTTONS {
        let path = button_path(name);
        let (shown, hint) = match button {
            Button::Repair => (s.repair.shown, Some(&s.hints.repair)),
            Button::Mod => (s.modify, Some(&s.hints.modify)),
            Button::Cancel => (s.keyring, Some(&s.hints.cancel)),
            Button::Hotkey => (!s.hints.hotkey.is_empty(), Some(&s.hints.hotkey)),
            Button::Equip | Button::Drop => (false, None),
        };
        put(ui, &path, "visible", on(shown));
        if let Some(hint) = hint {
            put(ui, &path, "_PCButtonText", text(hint));
        }
    }
    let path = button_path("IM_RepairButton");
    put(ui, &path, "target", on(s.repair.usable));
    put(ui, &button_path("IM_ModButton"), "target", on(s.modify_lit));
    // [design, #2312; hotkeys::can_hold] Match the immediate-mode Hotkey hint's light.
    put(ui, &button_path("IM_HotkeyButton"), "target", on(s.hotkey_lit));
    let Some(button) = resolve_tile(ui, &path) else { return };
    // The file's word (a setting's, a plugin's) is kept to come back to.
    let file_word = file_repair_word.get_or_insert_with(|| ui.tile(button).get("string").map_or_else(String::new, |v| v.text().to_string()));
    let word = s.repair.word.unwrap_or(file_word.as_str());
    if ui.tile(button).get("string").map(Value::text).as_deref() != Some(word) {
        ui.set_trait(button, "string", text(word));
    }
}

/// The Repair button's word when it is not the file's: `upkeep` ([`MAINTAIN_SETTING`]'s text, else
/// [`REPAIR_UPKEEP`]) for an item that needs no repair, by the hand-built hint's rule (`hint`,
/// `fnv_game::repair::hint`, "Repair" below it). With no row it stays what the last selection left
/// on the two tabs of gear, and is the file's on the others [observed: private job
/// run07c_inventory, 1280x720].
fn repair_word(hint: Option<&str>, tab: usize, last: Option<&'static str>, upkeep: &'static str) -> Option<&'static str> {
    match hint {
        Some("Repair") => None,
        Some(_) => Some(upkeep),
        None if tab <= 1 => last,
        None => None,
    }
}

/// The five tabs made from their template, each kept for good; whether any was made.
fn ensure_tabs(ui: &mut Ui, tabs: &mut Vec<RowTile<usize>>) -> Result<bool, String> {
    if tabs.len() == TAB_WORDS.len() && tabs.iter().all(|t| ui.is_live(t.tile)) {
        return Ok(false);
    }
    let keys = [0, 1, 2, 3, 4];
    let wanted: Vec<Wanted<'_, usize>> = keys.iter().map(|key| Wanted { key, name: TAB_ROW }).collect();
    let tabline = resolve_tile(ui, TABLINE).map(|t| ui.path_of(t)).unwrap_or_else(|| TABLINE.to_string());
    if !list::reconcile(ui, &tabline, TAB_TEMPLATE, tabs, &wanted) {
        return Err(format!("{TAB_TEMPLATE} made no tab"));
    }
    ui.update();
    Ok(true)
}

/// What the tab line is told, and the same marks as STATS' tabs have (`stats_menu::write_tabs`):
/// the tab gives its children no place. The word goes on the tab, whose inner tiles copy it
/// [documented: the prefabs], as `_selected` and its look follow `_CurrentTab`.
fn write_tabs(ui: &mut Ui, tabs: &[RowTile<usize>], tab: usize) {
    put(ui, TABLINE, "_ButtonCount", Value::Num(TAB_WORDS.len() as f64));
    put(ui, TABLINE, "_CurrentTab", Value::Num(tab as f64));
    let words = tab_words();
    for row in tabs {
        ui.write_changed(row.tile, "string", text(words[row.key]));
        if find_named(ui, row.tile, TAB_INNER).is_some() {
            ui.write_changed(row.tile, fnv_ui::input::NO_ORIGIN, Value::Num(1.0));
        }
    }
}

fn num(ui: &Ui, t: TileId, name: &str) -> f64 {
    ui.tile(t).get(name).map_or(0.0, Value::num)
}

/// Puts a value right that the file may work out otherwise, on every pass, writing only when it
/// differs, as `stats_menu::write_frame_of` does for the heading's line.
fn keep(ui: &mut Ui, t: TileId, name: &str, want: f64) {
    if ui.tile(t).get(name).map(Value::num) != Some(want) {
        ui.set_trait(t, name, Value::Num(want));
    }
}

/// The width of the tab line's lines, and each tab's `_x` in the line, by the tabs' own widths: a
/// line, a tab, a line, a tab, the rest for the last line [observed: private job run07_inventory, 1280x720, read for one set
/// of words; hypothesis: the floor].
pub(super) fn tab_xs(line_width: f64, widths: &[f64]) -> (f64, Vec<f64>) {
    let gap = ((line_width - widths.iter().sum::<f64>()) / (widths.len() + 1) as f64).floor().max(0.0);
    let mut x = gap;
    let xs = widths
        .iter()
        .map(|w| {
            let here = x;
            x += w + gap;
            here
        })
        .collect();
    (gap, xs)
}

/// What the file leaves to code of the tabs' places: each tab's `_x`, from which its inner rect's
/// and its own `x` follow [documented: the prefabs], and the line's `_LeftLineLength`. On every pass,
/// as a tab's width follows its measured word.
fn place_tabs(ui: &mut Ui, tabs: &[RowTile<usize>]) {
    let Some(line) = resolve_tile(ui, TABLINE) else { return };
    let widths: Vec<f64> = tabs.iter().map(|r| num(ui, r.tile, "width")).collect();
    let (gap, xs) = tab_xs(num(ui, line, "width"), &widths);
    keep(ui, line, "_LeftLineLength", gap);
    for (row, x) in tabs.iter().zip(xs) {
        keep(ui, row.tile, "_x", x);
    }
}

/// Where the tabs stand as the tree holds them, for the log when it changes.
fn tab_places(ui: &Ui, tabs: &[RowTile<usize>]) -> String {
    let n = |x: f64| Value::Num(x).text();
    let places: Vec<String> = tabs.iter().map(|r| format!("{} at {}, {} size {} x {} target {}", r.key, n(num(ui, r.tile, "x")), n(num(ui, r.tile, "y")), n(num(ui, r.tile, "width")), n(num(ui, r.tile, "height")), n(num(ui, r.tile, "target")))).collect();
    places.join("; ")
}

/// What [`usable`] went through, for the frame's counts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Checked {
    pub rows: usize,
    pub rows_in_clip: usize,
    pub hit_tests: usize,
    pub candidates: usize,
    /// The first tab that did not take the pointer, and what did: warned of, not a fallback.
    pub tab_missed: Option<String>,
}

/// Whether what is about to be painted, `pieces`, can be used, on every final pass, so what a
/// script did since is seen: every tab's words among the pieces, whole on the canvas and inside
/// their clips; the rows whose words lie whole on the canvas and inside the list's window, at
/// least one when there are rows, and these taking the pointer at their middles, whatever else
/// came to lie over them. A row wholly or partly outside the window is passed over: a scrolled
/// list has those by design [design: plan §5]. A list that scrolls must have a window. The tabs but
/// the one showing are asked the same, and one that does not take the pointer is told in
/// [`Checked::tab_missed`]: the page does without its click, the keys and the knob turning the tabs
/// [design]. The hit test is one walk of the menu however many rows there are.
pub fn usable(ui: &Ui, menu: TileId, view: &MenuTiles, tabs: &[RowTile<usize>], tab: usize, pieces: &[Piece]) -> Result<Checked, String> {
    let is_text = |x: &&Piece| matches!(x.look, Look::Text { .. });
    let texts: HashMap<TileId, &Piece> = pieces.iter().filter(is_text).map(|x| (x.tile, x)).collect();
    let rows = &view.rows[Side::Player as usize];
    let scrolls = resolve_tile(ui, LIST).and_then(|l| list::child(ui, l, list::SCROLLBAR)).and_then(|b| ui.tile(b).get("_number_of_items").map(Value::num)).is_some_and(|n| n > 1.0);
    let mut wanted: Vec<(ItemsTile, (f64, f64))> = Vec::with_capacity(rows.len() + tabs.len());
    for (i, row) in rows.iter().enumerate() {
        let Some(piece) = list::child(ui, row.tile, ROW_TEXT).and_then(|t| texts.get(&t)) else { continue };
        if scrolls && piece.clip.is_none() {
            return Err("the list scrolls and has no window".into());
        }
        if let Some(middle) = shows_whole(piece) {
            wanted.push((ItemsTile::Row(i), middle));
        }
    }
    let rows_in_clip = wanted.len();
    if !rows.is_empty() && rows_in_clip == 0 {
        return Err("no row shows whole".into());
    }
    for row in tabs {
        // A tab's words, as they are painted.
        let piece = pieces.iter().filter(is_text).find(|x| stats_menu::inside(ui, x.tile, row.tile)).ok_or_else(|| format!("tab {} is not shown", row.key))?;
        let middle = shows_whole(piece).ok_or_else(|| format!("tab {} does not fit", row.key))?;
        // The tab showing need not take it: a click on it does nothing.
        if row.key != tab {
            wanted.push((ItemsTile::Tab(row.key), middle));
        }
    }
    let found = fnv_ui::input::hit_candidates(ui, menu, &fnv_ui::input::no_measure);
    let mut tab_missed = None;
    for &(what, at) in &wanted {
        let hit = fnv_ui::input::top_hit(ui, &found, at);
        if hit.and_then(|h| items_tile(ui, view, tabs, h.tile)) != Some(what) {
            let there = hit.map_or_else(|| "nothing".to_string(), |h| ui.path_of(h.tile));
            let line = format!("{what:?} does not take the pointer at ({:.0}, {:.0}): {there} does", at.0, at.1);
            match what {
                ItemsTile::Tab(_) => tab_missed = tab_missed.or(Some(line)),
                _ => return Err(line),
            }
        }
    }
    Ok(Checked { rows: rows.len(), rows_in_clip, hit_tests: wanted.len(), candidates: found.len(), tab_missed })
}

/// What a tile of the page stands for, by the tile or a parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemsTile {
    Row(usize),
    Tab(usize),
    Button(Button),
    Bar(BarPiece),
}

/// What `tile` stands for: a tab or a button, else what the list makes of it.
pub fn items_tile(ui: &Ui, view: &MenuTiles, tabs: &[RowTile<usize>], tile: TileId) -> Option<ItemsTile> {
    let mut at = Some(tile);
    while let Some(t) = at {
        if let Some(row) = tabs.iter().find(|r| r.tile == t) {
            return Some(ItemsTile::Tab(row.key));
        }
        if let Some((_, button)) = BUTTONS.iter().find(|(name, _)| ui.tile(t).name.eq_ignore_ascii_case(name)) {
            return Some(ItemsTile::Button(*button));
        }
        at = ui.tile(t).parent;
    }
    match view.clicked(ui, tile)? {
        item_menu::MenuTile::Row(Side::Player, i) => Some(ItemsTile::Row(i)),
        item_menu::MenuTile::Bar(Side::Player, piece) => Some(ItemsTile::Bar(piece)),
        _ => None,
    }
}

/// What [`page`] keeps: the tabs, what was last written, and the reasons warned of.
#[derive(Default)]
pub(crate) struct Written {
    menu: Option<TileId>,
    /// The tiles [`built`] looked at, while the file is known to have them.
    checked: Vec<TileId>,
    tabs: Vec<RowTile<usize>>,
    shown: Option<Shown>,
    flat: Option<FlatChanges>,
    /// What is shown changed since the log last said so.
    tell: bool,
    /// The panel's key last logged.
    panel_told: Option<usize>,
    /// The Pip-Boy was open at the last pass: an opening starts the list at its top.
    was_open: bool,
    /// What the last final pass went through.
    counted: Checked,
    /// The tabs' places last logged.
    places_told: Option<String>,
    /// The Repair button's word in the file, before any of ours.
    file_repair_word: Option<String>,
    /// Final passes in a row that failed: a state that lasts a frame or two is no fallback to warn of.
    failing: u32,
    /// The tab and frame the last failure was for, the tree it met, and its reason.
    tried: Option<(usize, bool)>,
    tried_tree: Option<TreeMark>,
    reason: String,
    fallbacks: HashSet<String>,
}

/// The decision every reader shares, and what the page keeps.
#[derive(Resource)]
pub struct InventoryTiles {
    navigation_scroll: Option<(TileId,crate::menu::model::Category,crate::menu::model::RowKey,f64)>,
    /// The menu, while the page is ready to display this frame ([`settle`]).
    pub ready: Option<TileId>,
    /// The menu painted by the last drawing (`super::draw`): what the player sees.
    pub shown: Option<TileId>,
    /// The pieces [`usable`] checked this frame, for the drawing to paint: one measured layout, so
    /// what was checked is what is painted.
    pub(crate) pieces: Vec<Piece>,
    /// The list's rows, scroll and drag (`super::item_menu`).
    pub view: MenuTiles,
    /// [`CUT_AID`].
    pub cut: Option<String>,
    /// [`CUT_AID`]'s time.
    pub cut_until: Option<f32>,
    /// A menu over the page when this frame began (`stats_menu::modal_over`): set before the
    /// scripts, so it holds through the frame that menu closes in.
    pub modal: bool,
    /// Another menu took this frame's tile input (`super::input::feed`).
    pub others: bool,
    written: Written,
}

impl Default for InventoryTiles {
    fn default() -> Self {
        InventoryTiles { navigation_scroll:None,ready: None, shown: None, pieces: Vec::new(), view: MenuTiles::new(&PATHS), cut: None, cut_until: None, modal: false, others: false, written: Written::default() }
    }
}
impl InventoryTiles {
    pub(crate) fn queue_navigation_scroll(&mut self,category:crate::menu::model::Category,row:crate::menu::model::RowKey,value:f64) {
        self.navigation_scroll=self.written.menu.filter(|_|value.is_finite()).map(|menu|(menu,category,row,value.max(0.0)));
    }
    pub(crate) fn clear_navigation_scroll(&mut self) {self.navigation_scroll=None;}
}

/// The tiles of [`required_paths`], or why the page cannot be drawn.
fn check_file(ui: &Ui, cut: Option<&str>) -> Result<Vec<TileId>, String> {
    match built(ui, cut) {
        Tiled::No(reason) => Err(reason),
        Tiled::Yes => Ok(required_paths().iter().filter_map(|p| resolve_tile(ui, p)).collect()),
    }
}

/// One pass over the page; the menu when all of it went through. `new`: what the model shows, when
/// it may have changed. `late`: the pass before the drawing; the other, before the scripts, does
/// the sizes only for a model that changed.
pub(crate) fn page(ui: &mut Ui, new: Option<Shown>, late: bool, tiles: &mut InventoryTiles, fonts: &mut dyn PageFonts) -> Result<TileId, String> {
    let menu = ui.menu(INVENTORY_MENU).ok_or_else(|| format!("{INVENTORY_MENU} is not in the tree"))?;
    // A new menu tile (the tree built again): its rows, sizes and writes are all to do again.
    if tiles.written.menu != Some(menu) {
        let fallbacks = std::mem::take(&mut tiles.written.fallbacks);
        let was_open = tiles.written.was_open;
        tiles.written = Written { menu: Some(menu), fallbacks, was_open, ..Default::default() };
        tiles.view.close();
    }
    let w = &mut tiles.written;
    if w.checked.is_empty() || !w.checked.iter().all(|&t| ui.is_live(t)) {
        w.checked = check_file(ui, tiles.cut.as_deref())?;
    }
    let made = ensure_tabs(ui, &mut w.tabs)?;
    tiles.view.menu = Some(menu);
    let mut changed = false;
    if let Some(s) = new {
        let list_new = tiles.view.is_new(menu, &s.list);
        if list_new || w.shown.as_ref() != Some(&s) {
            write_model(ui, &s, &mut tiles.view, &mut w.file_repair_word);
            w.shown = Some(s);
            w.tell = true;
            changed = true;
        }
    }
    let Some(s) = w.shown.as_ref() else { return Err("nothing to show yet".into()) };
    let (tab, device) = (s.tab, s.device);
    if changed || made || late {
        write_tabs(ui, &w.tabs, tab);
        let texts = menus::texts(ui, menu);
        tiles.view.settle_shown(ui, &mut |ui: &mut Ui, sizes: &mut SizesWritten| fonts.write_sizes(ui, &texts, sizes));
        place_tabs(ui, &w.tabs);
        let title = resolve_tile(ui, TITLE).ok_or("the title is gone")?;
        let line = resolve_tile(ui, LINE).ok_or("the heading's line is gone")?;
        stats_menu::write_frame_of(ui, title, line, device, &mut w.flat);
        item_menu::mark_tiles(ui, &PATHS);
        ui.update();
    } else {
        // The scroll over the file's own count of the clicks and their pulses' ends.
        tiles.view.rewrite(ui);
    }
    if late {
        if let Some((owner,category,row,value))=tiles.navigation_scroll.take() {
            let list=&s.list.lists[Side::Player as usize];
            if owner==menu && list.category==category && list.selected.and_then(|i|list.rows.get(i)).is_some_and(|r|r.key==row) {
                tiles.view.scroll_to(Side::Player,value);
                tiles.view.rewrite(ui);
            }
        }
        ui.update();
        // Every final pass, so readiness is about the tree that is about to be painted.
        let pieces = fonts.set_out(ui, menu);
        let places = tab_places(ui, &w.tabs);
        if w.places_told.as_ref() != Some(&places) {
            info!("tiles: {INVENTORY_MENU} tabs stand: {places}");
            w.places_told = Some(places);
        }
        w.counted = usable(ui, menu, &tiles.view, &w.tabs, tab, &pieces)?;
        if let Some(missed) = w.counted.tab_missed.clone() {
            if w.fallbacks.insert(told_once(&missed)) {
                warn!("{INVENTORY_MENU}: {missed}");
            }
        }
        tiles.pieces = pieces;
        if std::mem::take(&mut w.tell) {
            info!("{}", describe(ui, w));
        }
        let panel = w.shown.as_ref().and_then(|s| s.panel);
        if w.panel_told != panel {
            match panel {
                Some(slot) => info!("tiles: {INVENTORY_MENU} hotkey panel shown, key {}", slot + 1),
                None => info!("tiles: {INVENTORY_MENU} hotkey panel hidden"),
            }
            w.panel_told = panel;
        }
        tiles.view.check_drawn(ui, &mut |line: String| info!("{line}"));
    }
    Ok(menu)
}

/// A line for the log when what the page shows changes (smoke reads these).
fn describe(ui: &Ui, w: &Written) -> String {
    let Some(s) = w.shown.as_ref() else { return format!("tiles: {INVENTORY_MENU} page written") };
    let list = &s.list.lists[Side::Player as usize];
    let selected = list.selected.and_then(|i| list.rows.get(i)).map_or("none", |r| r.text.as_str());
    format!(
        "tiles: {INVENTORY_MENU} page from tiles: {} rows, \"{selected}\" selected, tab {}, Wg {}, HP {}, DT {}, Caps {}, figures: {}, {}",
        list.rows.len(),
        s.tab,
        s.figures[0],
        s.figures[1],
        s.figures[2],
        s.figures[3],
        item_card::describe(ui, CARD),
        if s.device { "device" } else { "screen menu" },
    )
}

/// Whether the page is eligible to prepare: the switch on, ITEMS showing, and the repair list and
/// the mod menu closed.
pub fn eligible(switch: TileMenus, pip: &PipBoy, lists_closed: bool) -> bool {
    switch.inventory && pip.shown() && pip.page == Page::Items && lists_closed
}

/// Whether the repair list and the mod menu are closed.
pub fn lists_closed(repair: Option<&PipRepair>, modding: Option<&PipItemMod>) -> bool {
    !repair.is_some_and(PipRepair::is_open) && !modding.is_some_and(PipItemMod::is_open)
}

/// The keys the hand-built page acts on itself (`crate::pipboy::items::keys`,
/// `crate::pipboy::repair::keys`, `crate::pipboy::item_mod::keys`, `crate::pipboy::keys`), by the
/// tile input's names, and the controller's buttons standing in for them or acting there
/// (`crate::pad::menu_buttons`, the X drop in `items::keys`).
pub const HAND_BUILT_KEYS: [&str; 19] = ["W", "S", "A", "D", "E", "R", "X", "xup", "xdown", "xleft", "xright", "xbuttona", "xbuttonb", "xbuttonx", "xbuttony", "xbuttonlb", "xbuttonrb", "xbuttonlt", "xbuttonrt"];

/// Whether a click a key made on the page's tiles acts: as `menus::key_click_acts`, and never for
/// a controller's button the hand-built page answers to, which only a real press can make, stood
/// in or not (its X drops by `items::keys`, on a controller working the controls alone).
fn key_acts(key: &str, physical: &[&str]) -> bool {
    menus::key_click_acts(INVENTORY_MENU, key, physical) && !(key.to_ascii_lowercase().starts_with("xbutton") && menus::hand_built_takes(INVENTORY_MENU, key))
}

/// The menu, when the page takes this frame's tile input: painted last frame, and the page up,
/// with nothing over it and no prompt. Every reader of the frame's input decides by this.
pub fn takes(tiles: &InventoryTiles, switch: TileMenus, pip: &PipBoy, game: &fnv_game::Game<'_>, paused: bool, closed: bool, prompt: bool) -> Option<TileId> {
    if !pip.open() || !eligible(switch, pip, closed) || paused || prompt || modal_over(game.menu) {
        return None;
    }
    tiles.shown.filter(|&m| game.ui_files.is_some() && game.ui_tree.is_live(m))
}

/// The page's keys in the hints: what works on the hand-built page, by the game's settings where
/// they name the key [design: as `super::container_menu`'s Take All]. Cancel shows inside a Misc
/// folder, where E goes back ([`feed`]). With a controller it has no hint: its B stands in for Esc,
/// which closes the Pip-Boy on the hand-built page too (`pipboy::keys`), and the way out is the
/// folder's "Back" row.
fn hints_of(game: &fnv_game::Game<'_>) -> Hints {
    use fnv_game::ui_loading::game_string;
    let at = crate::pad::at_menu(fnv_game::game::menus::INVENTORY);
    let cancel = match crate::pad::labels() {
        crate::pad::Labels::Keys => game_string(game, "sPCMenuHintE"),
        _ => String::new(),
    };
    Hints {
        repair: crate::pad::game_hint(&game_string(game, "sPCMenuHintR"), &[KeyCode::KeyR], at),
        modify: crate::pad::game_hint(&game_string(game, "sPCMenuHintX"), &[KeyCode::KeyX], at),
        cancel,
        hotkey: if crate::pad::labels() == crate::pad::Labels::Keys { String::new() } else { crate::pipboy::items::hotkey_button() },
    }
}

/// What a pass reads besides the model.
#[derive(SystemParam)]
pub(crate) struct PageSources<'w> {
    repair: Option<Res<'w, PipRepair>>,
    modding: Option<Res<'w, PipItemMod>>,
    hold: Option<Res<'w, HotkeyHold>>,
    combat: Option<Res<'w, crate::combat::Combat>>,
    play: Option<Res<'w, crate::PlayWorld>>,
}

/// What a pass over the page reads.
#[derive(SystemParam)]
pub(crate) struct ItemsPage<'w, 's> {
    script: Option<ResMut<'w, ScriptState>>,
    switch: Option<Res<'w, TileMenus>>,
    pip: Option<Res<'w, PipBoy>>,
    active: Option<Res<'w, ActiveMenu>>,
    sim: Option<ResMut<'w, crate::sim::SimState>>,
    sources: PageSources<'w>,
    tiles: ResMut<'w, InventoryTiles>,
    fonts: super::draw::FontKit<'w>,
    time: crate::pause::AidTime<'w>,
    forms: Local<'s, Option<Forms>>,
}

/// A pass; `None` when the page is not eligible (nothing is touched then).
fn pass(p: &mut ItemsPage<'_, '_>, late: bool) -> Option<Result<TileId, String>> {
    let switch = p.switch.as_deref().copied().unwrap_or_default();
    if !switch.inventory {
        return None;
    }
    let pip = p.pip.as_deref()?;
    // A new opening has a new model, its selection at the top: the list too (`m.scroll` is the
    // hand-built page's). A list kept on STATS and back stays where it was.
    if p.tiles.written.was_open != pip.open() {
        p.tiles.written.was_open = pip.open();
        if pip.open() {
            p.tiles.view.scroll_to(Side::Player, 0.0);
        }
    }
    let closed = lists_closed(p.sources.repair.as_deref(), p.sources.modding.as_deref());
    if !eligible(switch, pip, closed) {
        return None;
    }
    let device = pip.frame == Frame::Device;
    let tab = pip.tab();
    let script = p.script.as_mut()?.bypass_change_detection();
    script.rt.as_ref()?.game.ui_files.as_ref()?;
    let active = p.active.as_deref()?;
    let model = active.0.as_ref().filter(|m| m.mode == Mode::Inventory);
    let hold = p.sources.hold.as_ref();
    let stale = p.tiles.written.shown.as_ref().is_none_or(|s| s.tab != tab || s.device != device);
    // No model and nothing written (the Pip-Boy going down from the hand-built page, a model that
    // comes later): nothing to decide yet, and no failure.
    if model.is_none() && p.tiles.written.shown.is_none() {
        return None;
    }
    let changed = p.active.as_ref().is_some_and(|a| a.is_changed()) || hold.is_some_and(|h| h.is_changed());
    // A page that has fallen back is tried again when what it would show changes, not every
    // frame: a snapshot walks the inventory and asks combat for the armour.
    // Nor while the tree stays as it was: a script that puts a missing tile back, or a menu
    // built again, is a repair to try.
    let tree = TreeMark::of(&script.rt.as_ref()?.game.ui_tree);
    let w = &p.tiles.written;
    if w.waits(tab, device, tree, changed) {
        return Some(Err(w.reason.clone()));
    }
    // The heading's HP and DT changed with nothing else (an aid item's heal a frame after
    // the list changed, a heal over time, a wear change): written again (#3120).
    let refigured = !stale && !changed && model.is_some() && {
        let (hp, dt) = match p.sim.as_mut() {
            Some(sim) => items::heading(sim.bypass_change_detection(), Some(&mut *script), p.sources.combat.as_deref()),
            None => ("--".into(), "--".into()),
        };
        p.tiles.written.shown.as_ref().is_some_and(|s| s.figures[1] != hp || s.figures[2] != dt)
    };
    let due = stale || changed || refigured;
    // With the model gone (the Pip-Boy going down) the page stays as it was last written.
    let new = model.filter(|_| due).and_then(|m| {
        if p.forms.is_none() {
            *p.forms = p.sources.play.as_ref().map(|w| Forms::new(w.lo, w.schema));
        }
        let forms = &mut *p.forms;
        let sim = p.sim.as_mut()?.bypass_change_detection();
        let combat = p.sources.combat.as_deref();
        let selected = m.selected_row(Side::Player);
        // No DPS on this page, as the hand-built one (`items::draw`).
        let card = selected.map(|r| Info { dps: None, ..items::info_of(r, combat, sim, forms.as_mut(), Some(&mut *script)) });
        let (hp, dt) = items::heading(sim, Some(&mut *script), combat);
        let caps = sim.world.item_count(FormId::PLAYER_REF, crate::menu::model::CAPS).unwrap_or(0);
        // Whole units cut off, not rounded [observed: private job run07_repair_menu, 1280x720, one case; as the container's].
        let wg = m.encumbrance.as_ref().map_or_else(|_| "--".to_string(), |e| format!("{:.0}/{:.0}", e.weight.floor(), e.capacity));
        let figures = [wg, hp, dt, caps.to_string()];
        let offer = repair::current(active, sim, Some(&*script));
        let last = p.tiles.written.shown.as_ref().and_then(|s| s.repair.word);
        let upkeep = crate::pipboy::labels::label(MAINTAIN_SETTING, REPAIR_UPKEEP);
        let word = repair_word(offer.as_ref().map(|o| fnv_game::repair::hint(o.gear, o.condition)), tab, last, upkeep);
        // Inside a vanilla Misc folder Cancel stands in Repair's place; inside a ySI category
        // folder (e.g. Weapons/Apparel) Repair remains available for selected items [design].
        let in_vanilla_misc_folder = matches!(m.folder, Some(crate::menu::model::Folder::Keys | crate::menu::model::Folder::Cards));
        let repair = RepairButton { shown: !in_vanilla_misc_folder, usable: offer.as_ref().is_some_and(|o| o.usable()), word };
        let panel = hold.and_then(|h| h.slot);
        let game = &script.rt.as_ref()?.game;
        let words = (hints_of(game), fnv_game::ui_loading::game_string(game, CONDITION_TITLE));
        Some(Shown::of(m, card, tab, device, figures, repair, words, panel))
    });
    let rt = script.rt.as_mut()?;
    Some(page(&mut rt.game.ui_tree, new, late, &mut p.tiles, &mut p.fonts))
}

/// The trait writes so far, while `NVF_PLAY_TEST_TILE_COUNTS` asks for them.
fn counts_before(p: &ItemsPage<'_, '_>) -> Option<((u64, u64), std::time::Instant)> {
    let rt = p.script.as_ref()?.rt.as_ref()?;
    super::counts_on().then(|| (rt.game.ui_tree.trait_write_counts(), std::time::Instant::now()))
}

/// The pass's share of the frame's counts (module notes); a page that is not up has none.
fn tell_counts(p: &ItemsPage<'_, '_>, before: Option<((u64, u64), std::time::Instant)>, late: bool, up: bool) {
    let (Some(((writes, changes), start)), true, Some(rt)) = (before, up, p.script.as_ref().and_then(|s| s.rt.as_ref())) else { return };
    let micros = start.elapsed().as_micros();
    let (writes_now, changes_now) = rt.game.ui_tree.trait_write_counts();
    let (writes, changed) = (writes_now - writes, changes_now - changes);
    if late {
        let c = &p.tiles.written.counted;
        info!(
            "tile counts: {INVENTORY_MENU} pass=late writes={writes} changed={changed} micros={micros} rows={} rows_in_clip={} hit_tests={} candidates={} pieces={}",
            c.rows,
            c.rows_in_clip,
            c.hit_tests,
            c.candidates,
            p.tiles.pieces.len(),
        );
    } else {
        info!("tile counts: {INVENTORY_MENU} pass=early writes={writes} changed={changed} micros={micros}");
    }
}

/// Before the scripts, after the tile input: this frame's model in the tree, and the scroll.
pub(crate) fn write(mut p: ItemsPage) {
    let before = counts_before(&p);
    // A failure here is told by `settle`, which decides.
    let up = matches!(pass(&mut p, false), Some(Ok(_)));
    tell_counts(&p, before, false, up);
}

/// Before the drawing, after the model's keys and commands: what they changed, the tree worked
/// out, and the one decision every reader of this frame shares.
pub(crate) fn settle(mut p: ItemsPage) {
    if p.tiles.cut_until.is_some_and(|until| p.time.elapsed_secs() >= until) {
        info!("test aid: {CUT_AID} over");
        p.tiles.cut = None;
        p.tiles.cut_until = None;
        p.tiles.written.checked.clear();
        // The cut never touched the tree, so its end alone is no change the page waits on: the
        // menu is built again from its file, a real change of the tree, and the page recovers
        // only through [`Written::waits`] seeing it.
        if let Some(game) = p.script.as_mut().and_then(|s| s.bypass_change_detection().rt.as_mut()).map(|rt| &mut rt.game) {
            rebuild_menu(game);
        }
    }
    let before = counts_before(&p);
    let ready = match pass(&mut p, true) {
        Some(Ok(menu)) => {
            p.tiles.written.failing = 0;
            Some(menu)
        }
        Some(Err(reason)) => {
            // The hand-built page has this frame in any case; the third frame in a row is no
            // layout still settling.
            let tried = p.pip.as_deref().map(|pip| (pip.tab(), pip.frame == Frame::Device));
            let tree = p.script.as_ref().and_then(|s| s.rt.as_ref()).map(|rt| TreeMark::of(&rt.game.ui_tree));
            let w = &mut p.tiles.written;
            w.failed(tried, tree);
            let line = format!("{INVENTORY_MENU} drawn by hand: {reason}");
            if w.failing >= FAILING_PASSES && w.fallbacks.insert(told_once(&line)) {
                warn!("{line}");
            }
            w.reason = reason;
            None
        }
        None => {
            p.tiles.written.failing = 0;
            None
        }
    };
    tell_counts(&p, before, true, ready.is_some());
    if ready.is_none() {
        // The next time it is written whole; the rows stay.
        p.tiles.written.shown = None;
        p.tiles.pieces.clear();
        p.tiles.view.menu = None;
    }
    if p.tiles.ready != ready {
        p.tiles.ready = ready;
    }
}

/// Final passes in a row that fail before the reason is warned of, and before the page waits for
/// a change to try again.
const FAILING_PASSES: u32 = 3;

/// What the tree was at a failed pass: the menu's tile (a new one when the menu is built again)
/// and the tree's shape (`Ui::shape`: a tile made, unloaded or renamed) [design]. Taken after the
/// pass, so what the page itself made in it counts as no change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TreeMark {
    menu: Option<TileId>,
    shape: u64,
}

impl TreeMark {
    fn of(ui: &Ui) -> TreeMark {
        TreeMark { menu: ui.menu(INVENTORY_MENU), shape: ui.shape() }
    }
}

impl Written {
    /// A final pass failed, for this tab and frame, in this tree.
    fn failed(&mut self, tried: Option<(usize, bool)>, tree: Option<TreeMark>) {
        self.failing += 1;
        self.tried = tried;
        self.tried_tree = tree;
    }

    /// The page has fallen back and waits: nothing it would show changed, the tab and frame are the
    /// failure's, and the tree is as that failure met it.
    fn waits(&self, tab: usize, device: bool, tree: TreeMark, changed: bool) -> bool {
        self.failing >= FAILING_PASSES && !changed && self.tried == Some((tab, device)) && self.tried_tree == Some(tree)
    }
}

/// [`CUT_AID`]'s end: the menu built again from its file, as at load (`fnv_game::ui_loading::load`).
fn rebuild_menu(game: &mut fnv_game::Game<'_>) {
    let (Some(files), Some(path)) = (game.ui_files.as_deref(), game.ui_menus.path(INVENTORY_MENU)) else { return };
    match game.ui_tree.load_menu(files, path) {
        Ok(_) => {
            game.ui_tree.update();
            info!("test aid: {INVENTORY_MENU} built again from {path}");
        }
        Err(e) => warn!("test aid: {INVENTORY_MENU} not built again: {e}"),
    }
}

/// What a warning is told once by: its line without the place and the row, so a list scrolled through
/// adds nothing to what is kept.
fn told_once(line: &str) -> String {
    line.split(" at (").next().unwrap_or(line).chars().filter(|c| !c.is_ascii_digit()).collect()
}

/// What [`feed`] keeps between frames.
#[derive(Default)]
pub(crate) struct Feed {
    /// The page took input last frame, in this frame of the Pip-Boy.
    taking: Option<Frame>,
    /// Where the pointer was last frame, and the tile under it then.
    last_at: Option<(f64, f64)>,
    under: Option<TileId>,
    touch_drag: bool,
}

/// What the page's tile input reads of the Pip-Boy.
#[derive(SystemParam)]
pub(crate) struct FeedPip<'w> {
    pip: Res<'w, PipBoy>,
    screen: Option<Res<'w, PipBoyScreen>>,
    aid: Res<'w, PipBoyAid>,
    paused: Option<Res<'w, crate::pause::Paused>>,
    active: Option<Res<'w, ActiveMenu>>,
    repair: Option<Res<'w, PipRepair>>,
    modding: Option<Res<'w, PipItemMod>>,
    wheel: Option<Res<'w, crate::menu::WheelHold>>,
    touch:Option<ResMut<'w,crate::pipboy::touch_items::TouchItems>>,
    admission: crate::pipboy::touch_items::Admission<'w>,

}

/// What the page's tile input sends: where the hand-built page sends the same.
#[derive(SystemParam)]
pub(crate) struct FeedOut<'w> {
    tabs: MessageWriter<'w, PipBoyInput>,
    hovered: MessageWriter<'w, TabHovered>,
    commands: MessageWriter<'w, MenuInput>,
    repair: MessageWriter<'w, RepairInput>,
    modding: MessageWriter<'w, ModInput>,
    hold: ResMut<'w, HotkeyHold>,
    hotkeys: ResMut<'w, Hotkeys>,
    tweaks: Option<Res<'w, crate::tweaks::TweakSettings>>,
    taken: Res<'w, crate::pipboy::KeysTaken>,
}

/// Commands on a row behind a guard saying what its tile stood for, checked as they apply.
fn send_on_row(out: &mut FeedOut<'_>, view: &MenuTiles, index: usize, on_it: bool, commands: &[Command]) {
    if let Some(guard) = view.guard(Mode::Inventory, Side::Player, index, on_it, commands.len() as u8) {
        out.commands.write(MenuInput(Command::Expect(guard)));
    }
    for &cmd in commands {
        info!("tiles: {INVENTORY_MENU} {cmd:?} on its tiles");
        out.commands.write(MenuInput(cmd));
    }
}

/// What the tiled Drop button (click or R key) emits: with `quantity-select-short-keys`
/// enabled (#3799), Shift drops the whole stack and Ctrl drops a single item without opening
/// the quantity prompt [design: Shift takes precedence over Ctrl when both are held].
pub(crate) fn tiled_drop_command(qtt_select_short_keys: bool, shift: bool, ctrl: bool) -> Command {
    if qtt_select_short_keys && shift {
        Command::DropStack
    } else if qtt_select_short_keys && ctrl {
        Command::DropSingle
    } else {
        Command::Drop
    }
}

pub(crate) fn touch_target(tiles:&InventoryTiles,ui:&Ui,menu:TileId,point:Vec2,model:&crate::menu::model::MenuModel)->Option<crate::pipboy::touch_scroll::NativeTarget>{
    use crate::pipboy::touch_scroll::{NativeRow,NativeTarget};
    NativeTarget::at(ui,menu,point,|tile| {
        let surface=|needs_selection|Some(NativeRow::InventorySurface{mode:model.mode,folder:model.folder,category:model.category,needs_selection});
        match items_tile(ui,&tiles.view,&tiles.written.tabs,tile) {
            Some(ItemsTile::Row(index))=>tiles.view.guard(Mode::Inventory,Side::Player,index,false,0)
                .map(|guard|NativeRow::Inventory{guard,folder:model.folder,category:model.category}),
            Some(ItemsTile::Button(Button::Equip|Button::Drop|Button::Repair|Button::Mod))=> {
                // A button acts on the row the painted page showed selected at press.
                let guard=tiles.written.shown.as_ref().and_then(|s|s.list.lists[Side::Player as usize].selected)
                    .and_then(|index|tiles.view.guard(Mode::Inventory,Side::Player,index,true,0));
                guard.map(|guard|NativeRow::Inventory{guard,folder:model.folder,category:model.category}).or_else(||surface(true))
            }
            _=>surface(false),
        }
    },|row|match row {Some(NativeRow::Inventory{guard,..}) if !guard.selected=>Some(model.focus==guard.side && model.selected[guard.side as usize]==guard.index),_=>None},
    |row|match row {Some(NativeRow::Inventory{guard,..})=>model.row_is(&guard),Some(NativeRow::InventorySurface{needs_selection,..})=>!needs_selection,_=>true})
        .map(|target| target.with_control(|tile| matches!(items_tile(ui,&tiles.view,&tiles.written.tabs,tile),Some(ItemsTile::Bar(BarPiece::Marker)))))
}
pub(crate) fn touch_navigation(tiles:&InventoryTiles,ui:&Ui)->Vec<crate::pipboy::tab_swipe::Region> {
    use crate::pipboy::tab_swipe::{native_region,Kind};
    let title=ui.find_tile(TITLE);
    tiles.pieces.iter().filter_map(|p| {
        let kind=if Some(p.tile)==title {Kind::Sections}
            else if matches!(items_tile(ui,&tiles.view,&tiles.written.tabs,p.tile),Some(ItemsTile::Tab(_))) {Kind::Subtabs}
            else {return None};
        native_region(p,kind)
    }).collect()
}
/// The page's tile input, on a frame it takes it ([`takes`]): see the module notes. The Pip-Boy's
/// pointer and its test aid are only read here.
pub(crate) fn feed(mut script: ResMut<ScriptState>, switch: Res<TileMenus>, from: FeedPip, mut tiles: ResMut<InventoryTiles>, devices: Devices, mut out: FeedOut, mut state: Local<Feed>) {
    if !switch.inventory {
        return;
    }
    let FeedPip { pip, screen, aid, paused, active, repair, modding, wheel, mut touch, admission } = from;
    let Some(rt) = script.rt.as_mut() else { return };
    let game = &mut rt.game;
    let closed = lists_closed(repair.as_deref(), modding.as_deref());
    let modal = eligible(*switch, &pip, closed) && pip.open() && modal_over(game.menu);
    if tiles.modal != modal {
        tiles.modal = modal;
    }
    let is_paused = paused.as_ref().is_some_and(|p| p.0);
    let model = active.as_ref().and_then(|a| a.0.as_ref()).filter(|m| m.mode == Mode::Inventory);
    let prompt = model.is_some_and(|m| m.quantity.is_some());
    let claimed = takes(&tiles, *switch, &pip, game, is_paused, closed, prompt);
    let menu = claimed.filter(|_| !tiles.others && !out.taken.taken() && out.hold.controller.is_none());
    let (Some(menu), Some(m)) = (menu, model) else {
        // `tiles::input::feed` skipped `idle` because `takes` claimed this frame; still
        // drain any queued script clicks (`ClickMenuTile` / `ClickMenuButton`) [design].
        if claimed.is_some() && !tiles.others {
            fnv_game::ui_input::idle(game);
        }
        *state = Feed::default();
        tiles.view.drag = None;
        return;
    };
    let mut mobile=crate::pipboy::touch_scroll::native_frame(touch.as_deref(),&devices.mouse);
    if !admission.allows_contact(touch.as_ref().and_then(|t|t.contact_id())) { mobile=Default::default(); }
    // Armed assignment proposes a guarded item for explicit confirmation. It
    // cannot also dispatch the native page's select/use/drop button click.
    if touch.as_ref().is_some_and(|t| t.binding_slot().is_some()) { mobile.tap = false; }
    if mobile.tap {
        let target=mobile.at.and_then(|at|touch_target(&tiles,&game.ui_tree,menu,at,m));
        mobile.tap=touch.as_ref().is_some_and(|t|t.native_release_valid(target));
    }
    if mobile.control && mobile.started {
        let target=mobile.at.and_then(|at|touch_target(&tiles,&game.ui_tree,menu,at,m));
        mobile.started=target.is_some_and(|t|t.control()) && touch.as_ref().is_some_and(|t|t.native_release_valid(target));
    }
    let pointer = mobile.at.or_else(||screen.as_ref().and_then(|s| s.pointer));
    let at = pointer.map(|p| (f64::from(p.x), f64::from(p.y)));
    if state.taking != Some(pip.frame) {
        // Where the pointer rests when the page comes up is no move onto a row.
        *state = Feed { taking: Some(pip.frame), last_at: at, under: None, touch_drag: false };
    }
    // The test aid's click (`crate::pipboy::drive_pointer`): 1 is its press's frame.
    let aid_press = aid.click == 1 && !aid.right;
    let aid_right = aid.click == 1 && aid.right;
    let held = mobile.held || aid_press || devices.mouse.pressed(MouseButton::Left);
    let mut input = fnv_ui::input::Input {
        at,
        press: mobile.press() || aid_press || devices.mouse.just_pressed(MouseButton::Left),
        shift: devices.keys.pressed(KeyCode::ShiftLeft) || devices.keys.pressed(KeyCode::ShiftRight),
        ..Default::default()
    };
    if mobile.started {
        let index=at.and_then(|at|fnv_ui::input::hit(&game.ui_tree,&[menu],at,&fnv_ui::input::no_measure))
            .and_then(|hit|items_tile(&game.ui_tree,&tiles.view,&tiles.written.tabs,hit.tile))
            .and_then(|what|match what {ItemsTile::Row(index)=>tiles.view.row_at(m,Side::Player,index),_=>None});
        if let Some((touch,index))=touch.as_deref_mut().zip(index) {touch.capture_row(m,index);}
    }
    let shift = input.shift;
    let ctrl = devices.keys.pressed(KeyCode::ControlLeft) || devices.keys.pressed(KeyCode::ControlRight);
    let ctrl_single = m.qtt_select_short_keys && ctrl;
    let pad_menu = crate::pad::MenuAt { code: Some(fnv_game::game::menus::INVENTORY), tab: 0 };
    let physical = frame_keys(&mut input, &devices, game, pad_menu);
    let report = fnv_game::ui_input::step(game, &[menu], &input, &fnv_ui::input::no_measure);
    let ui = &game.ui_tree;
    let moved = at != state.last_at;
    let right = aid_right || devices.mouse.just_pressed(MouseButton::Right);
    // What is under the pointer itself; looked for again only when it is needed.
    if report.over.is_some() || moved || right {
        state.under = at.and_then(|at| fnv_ui::input::hit(ui, &[menu], at, &fnv_ui::input::no_measure)).map(|h| h.tile);
        state.last_at = at;
    }
    log_report(ui, &report);
    let tiles = &mut *tiles;
    let tabs = &tiles.written.tabs;
    if !held || (state.touch_drag && !mobile.control) {
        tiles.view.drag = None;
        state.touch_drag = false;
    }
    // At most one row click acts a frame: the commands apply later, on rows the first may change.
    let mut row_acted = false;
    let mut pressed_marker = false;
    for c in report.clicks.iter().filter(|c| ui.is_live(c.tile)) {
        if let fnv_ui::input::By::Key(k) = &c.by {
            // E has nothing to do on the hand-built page: in a vanilla Misc folder, the
            // file's Cancel button has it (`_KeyringOpen`). Inside `Folder::Ysi(_)`,
            // `IM_CancelButton` is hidden so `Back` / `Backspace` leaves the folder.
            let folder_e = matches!(m.folder, Some(crate::menu::model::Folder::Keys | crate::menu::model::Folder::Cards))
                && k.eq_ignore_ascii_case("E");
            if !folder_e && !key_acts(k, &physical) {
                continue;
            }
        }
        let Some(tile) = items_tile(ui, &tiles.view, tabs, c.tile) else { continue };
        info!("tiles: {INVENTORY_MENU} {tile:?} on its tiles");
        match tile {
            ItemsTile::Row(index) => {
                let Some(index) = tiles.view.row_at(m, Side::Player, index).filter(|_| !row_acted) else {
                    info!("tiles: {INVENTORY_MENU} a click on a row no longer there is ignored");
                    continue;
                };
                row_acted = true;
                // With a hotkey's key held the row's item goes in that slot; else as the
                // hand-built row: a click selects it, on the selected row it acts
                // (`items::on_row_click`).
                match out.hold.slot {
                    Some(slot) => {
                        if let Some(row) = m.list(Side::Player).rows.get(index) {
                            // The player's tweak: the slot holding it already lets it go [design: docs/design/tweaks.md].
                            let deselect = out.tweaks.as_ref().is_some_and(|t| t.deselect_hotkeys);
                            let said = hotkey_panel::assign_row(&mut out.hotkeys, slot, row, deselect);
                            out.hold.said = Some(said);
                        }
                    }
                    None => {
                        if mobile.tap && mobile.initially_selected==Some(false) {
                            // Hover after the finger went down must not upgrade a select into use.
                            send_on_row(&mut out,&tiles.view,index,false,&[Command::Hover(Side::Player,index)]);
                        } else {
                            let on_it = m.selected[Side::Player as usize] == index;
                            send_on_row(&mut out, &tiles.view, index, on_it, &[Command::Select(Side::Player, index)]);
                        }
                    }
                }
            }
            ItemsTile::Tab(i) => {
                out.tabs.write(PipBoyInput::Tab(i));
            }
            ItemsTile::Button(Button::Equip) => {
                out.commands.write(MenuInput(Command::One));
            }
            ItemsTile::Button(Button::Drop) => {
                out.commands.write(MenuInput(tiled_drop_command(m.qtt_select_short_keys, shift, ctrl)));
            }
            ItemsTile::Button(Button::Repair) => {
                out.repair.write(RepairInput::Open);
            }
            ItemsTile::Button(Button::Mod) => {
                out.modding.write(ModInput::Open);
            }
            // It shows only inside a Misc folder (such as a keyring), where the hand-built
            // page's way out is `Back`. Outside a folder the button is hidden and a script's
            // `ClickMenuButton` on it must not close the Pip-Boy [design, #3915].
            ItemsTile::Button(Button::Cancel) => {
                if matches!(m.folder, Some(crate::menu::model::Folder::Keys | crate::menu::model::Folder::Cards)) {
                    out.commands.write(MenuInput(Command::Back));
                }
            }
            ItemsTile::Button(Button::Hotkey) => {
                // [design, #2312] The optional tile opens the same modal as pad RB.
                if crate::pad::labels() != crate::pad::Labels::Keys {
                    if let Some(row) = m.selected_row(Side::Player).filter(|r| r.folder.is_none()) {
                        out.hold.open_controller(row);
                    }
                }
            }
            ItemsTile::Bar(BarPiece::Marker) => {
                pressed_marker = true;
                let list_tile = tiles.view.list_tile(ui, Side::Player);
                tiles.view.drag = at.filter(|_| held && matches!(c.by, fnv_ui::input::By::Pointer)).zip(list_tile).and_then(|((_, y), l)| list::marker_grab(ui, menu, l, y));
                state.touch_drag = mobile.control && tiles.view.drag.is_some();
            }
            ItemsTile::Bar(piece) => {
                let value = tiles.view.value(Side::Player);
                if let Some(v) = tiles.view.list_tile(ui, Side::Player).and_then(|l| list::after_piece(ui, l, piece, value)) {
                    tiles.view.scroll_to(Side::Player, v);
                }
            }
        }
    }
    // A drag: the scroll under the pointer while the press holds.
    if let (false, Some(drag), Some((_, y))) = (pressed_marker, tiles.view.drag, at) {
        if let Some(v) = list::value_under(ui, drag, y) {
            tiles.view.scroll_to(Side::Player, v);
        }
    }
    // A right click on a row drops one, with Shift the stack; not with a hotkey's key held
    // [design: owner 2026-11-02, #1393; `items::on_row_click`].
    if right && !row_acted && out.hold.slot.is_none() {
        if let Some(ItemsTile::Row(index)) = state.under.and_then(|t| items_tile(ui, &tiles.view, tabs, t)) {
            if let Some(index) = tiles.view.row_at(m, Side::Player, index) {
                row_acted = true;
                send_on_row(&mut out, &tiles.view, index, false, &[Command::Hover(Side::Player, index), if shift { Command::DropStack } else if ctrl_single { Command::DropSingle } else { Command::Drop }]);
            }
        }
    }
    if let Some((_, Some(to))) = report.over.filter(|_| !mobile.control) {
        match items_tile(ui, &tiles.view, tabs, to) {
            Some(ItemsTile::Tab(i)) => {
                out.hovered.write(TabHovered(i));
            }
            // The row the pointer comes onto takes the selection, unless the wheel holds it. Not
            // a row a file's own key binding moved the highlight onto: the hand-built keys moved
            // the selection for that press, and this would mark it as the pointer's.
            Some(ItemsTile::Row(index)) if !report.over_by_key => {
                let already = m.selected[Side::Player as usize] == index;
                let holds = wheel.as_ref().is_some_and(|h| h.held());
                if !row_acted && item_menu::hover_selects(moved, holds, already) && tiles.view.row_at(m, Side::Player, index).is_some() {
                    send_on_row(&mut out, &tiles.view, index, false, &[Command::Hover(Side::Player, index)]);
                }
            }
            _ => {}
        }
    }
}

/// The resource, with the test aid read.
pub(crate) fn setup(app: &mut App) {
    let (cut, cut_until) = std::env::var(CUT_AID).ok().map_or((None, None), |v| {
        let (name, until) = stats_menu::parse_cut(&v);
        (Some(name), until)
    });
    app.insert_resource(InventoryTiles { cut, cut_until, ..Default::default() });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::model::{Category, RowKey};
    use fnv_ui::MemSource;

    /// The text inside a tab in the tests' layout.
    const TAB_TEXT: &str = "button_text";

    /// [design] A small layout written for these tests, with its own structure, numbers and
    /// expressions; it does not come from the game's files and says nothing about them. It keeps
    /// the tile, template and path names the model and the input look up, which are the engine's
    /// interface ([`required_paths`]), and only the dependencies the tests exercise: a row stands at
    /// its `_y` less its list's scroll, a tab at its `_x` with its `string` in a text under it. So
    /// these tests do not cover the file's own places, how its expressions read what
    /// is written (the tab template's inside, `_CurrentTab`, `_LeftLineLength`, a tab's `_selected`,
    /// `_EquippableItem` and the marker that follows it, `_KeyringOpen`, a button's `target` and its
    /// dim look, the panel's traits, the headline cards' `_Value`), or whether a name is where the file has it: the smoke scenarios
    /// `tiles-items*` check those on the real file, and their shots what they look like.
    fn tree_changed(change: impl FnOnce(String) -> String) -> Ui {
        let mut ui = Ui::new();
        ui.load_menu(&MemSource::default().with("i.xml", &fixture_xml(change)), "i.xml").unwrap();
        ui.update();
        ui
    }

    /// The fixture's menu built again whole in `ui`, as a menu loaded again replaces the one of its name.
    fn rebuild_fixture(ui: &mut Ui) {
        ui.load_menu(&MemSource::default().with("i.xml", &fixture_xml(|x| x)), "i.xml").unwrap();
        ui.update();
    }

    /// The fixture's text, changed by `change`.
    fn fixture_xml(change: impl FnOnce(String) -> String) -> String {
        let card = |name: &str, inner: &str| format!(r#"<rect name="{name}"><_Title>{name}</_Title><_Value></_Value><visible>&false;</visible><text name="CI_TitleText"></text><text name="CI_ValueText"></text>{inner}</rect>"#);
        let cards: String = ["DAMInfo", "DamageThresholdInfo", "DPSInfo", "WeightInfo", "ValueInfo", "StrengthReqInfo", "AmmoInfo", "EffectsInfo"].iter().map(|c| card(c, "")).collect::<String>() + &card("ConditionInfo", r#"<rect name="ConditionMeter"><image name="MeterImage"></image></rect><image name="CNDArrows"></image>"#);
        let headline: String = ["WG", "HP", "DT", "DR", "Caps"].iter().map(|c| format!(r#"<rect name="IM_Headline_Player{c}Info"><_Value></_Value></rect>"#)).collect();
        let button = |name: &str, y: u32| format!(r#"<image name="{name}"><target>&true;</target><y>{y}</y><width>70</width><height>20</height><text name="button_text"><string>Go</string></text><text name="PCShortcutLabel"></text></image>"#);
        let buttons: String = [("IM_EquipButton", 0), ("IM_DropButton", 30), ("IM_RepairButton", 60), ("IM_ModButton", 90), ("IM_CancelButton", 120)].iter().map(|(n, y)| button(n, *y)).collect();
        let xml = format!(
            r#"<menu name="InventoryMenu"><rect name="NOGLOW_BRANCH"/><rect name="GLOW_BRANCH">
            <image name="IM_Background"/>
            <rect name="IM_MainRect">
              <rect name="IM_HeadlineRect"><text name="IM_Headline_Title"><string>Words</string></text>
                <rect name="IM_Headline_h2"><x>300</x><_length>40</_length></rect>{headline}</rect>
              <hotrect name="IM_InventoryList"><x>20</x><y>50</y><width>300</width><height>200</height><clipwindow>&true;</clipwindow>
                <_scroll_delta>15</_scroll_delta>
                <_offset><copy src="child(lb_scrollbar)" trait="_current_value"/><mul src="me()" trait="_scroll_delta"/></_offset>
                <hotrect name="lb_highlight_box"/>
                <image name="lb_scrollbar"><_step_size>1</_step_size><_jump_size>2</_jump_size><_current_value>0</_current_value>
                  <hotrect name="scrollbar_vert_page_up"/><hotrect name="scrollbar_vert_page_down"/>
                  <image name="scrollbar_vert_marker"/><image name="scrollbar_vert_up"/><image name="scrollbar_vert_down"/>
                </image>
              </hotrect>
              <rect name="IM_HotKeyWheel"><visible>&false;</visible></rect>
              <rect name="IM_ButtonRect"><x>500</x><y>300</y>{buttons}</rect>
              <image name="IM_ItemIcon"/><image name="IM_ItemIconBadge"/><text name="IM_StrReq"></text>
              <rect name="IM_ItemInfoRect"><user1>30</user1>{cards}</rect>
            </rect>
            <rect name="IM_Tabline"><y>560</y></rect>
            </rect>
            <template name="IM_InventoryListTemplate"><hotrect name="IM_InventoryListTemplateRect"><target>&true;</target><x>10</x><width>250</width>
              <_y>0</_y><_VerticalSpacing>10</_VerticalSpacing>
              <y><copy src="me()" trait="_y"/><sub src="parent()" trait="_offset"/></y>
              <text name="ListItemText"></text><image name="IM_Template_ItemMarker"/>
            </hotrect></template>
            <template name="TabButtonTemplate"><hotrect name="TabButton"><target>&true;</target><width>100</width><height>30</height><_x>0</_x>
              <x><copy src="me()" trait="_x"/></x>
              <text name="button_text"><string><copy src="parent()" trait="string"/></string></text>
            </hotrect></template></menu>"#
        );
        change(xml)
    }

    fn tree() -> Ui {
        tree_changed(|x| x)
    }

    /// Each row's height in the fixture: its text's 30 and its own spacing of 10.
    const ROW_H: f64 = 40.0;

    fn row(item: u32, text: &str) -> item_menu::RowShown {
        item_menu::RowShown { key: RowKey { units: (FormId(item), None, 0), reference: None, nth: 0 }, name: format!("Item {item}"), text: text.into(), equipped: item == 1, dim: false, ysi: None }
    }

    fn shown(rows: usize, selected: usize) -> Shown {
        let list = ListShown { rows: (1..=rows as u32).map(|i| row(i, &format!("Row {i}"))).collect(), category: Category::Weapons, selected: (selected < rows).then_some(selected), filtered: 0 };
        Shown {
            list: Snapshot {
                lists: [ListShown::default(), list],
                titles: Default::default(),
                weight: String::new(),
                focus: Side::Player,
                pointer_pick: false,
                asking: false,
                navigated: 0,
                card: (selected < rows).then(|| Info { value: Some("30".into()), weight: Some("5.00".into()), badge: Some("pics\\badge.dds".into()), skill_req: Some(25), ..Info::default() }),
                icon: (selected < rows).then(|| "pics\\item.dds".to_string()),
                take_all: false,
                take_all_hint: String::new(),
                note: None,
                ysi_icons: true,
            },
            tab: 0,
            device: true,
            figures: ["52/200".into(), "90/100".into(), "8.0".into(), "1234".into()],
            repair: RepairButton { shown: true, usable: false, word: None },
            modify: false,
            modify_lit: selected < rows,
            ammo: false,
            equippable: true,
            hotkey_lit: true,
            keyring: false,
            hints: Hints { repair: "[R]".into(), modify: "[X]".into(), cancel: "[Back]".into(), hotkey: String::new() },
            condition_title: "Short".into(),
            panel: None,
        }
    }

    /// A measure of 10 a character and 30 a line.
    fn measure(_: f64, text: &str, _: Option<f64>) -> (f64, f64) {
        (10.0 * text.chars().count() as f64, 30.0 * text.split('|').count() as f64)
    }

    struct TestFonts;

    impl PageFonts for TestFonts {
        fn write_sizes(&mut self, ui: &mut Ui, texts: &[TileId], written: &mut SizesWritten) -> usize {
            menus::write_text_sizes_of(ui, texts, written, &measure)
        }
        fn set_out(&mut self, ui: &Ui, menu: TileId) -> Vec<Piece> {
            super::super::draw::pieces(ui, menu, &measure, super::super::draw::Select::Whole { kept: &[], strip: None }, true).0
        }
    }

    fn run(ui: &mut Ui, s: Option<Shown>, t: &mut InventoryTiles) -> Result<TileId, String> {
        page(ui, s, true, t, &mut TestFonts)
    }

    fn at(ui: &Ui, path: &str, name: &str) -> f64 {
        ui.get_float(&format!("{path}/{name}")).unwrap_or(-999.0)
    }

    fn words(ui: &Ui, path: &str, name: &str) -> String {
        ui.get_string(&format!("{path}/{name}")).unwrap_or_default()
    }

    fn rows(t: &InventoryTiles) -> &[RowTile<RowKey>] {
        &t.view.rows[Side::Player as usize]
    }

    /// The middle of a tile's text piece, as painted.
    fn middle(t: &InventoryTiles, tile: TileId) -> (f64, f64) {
        let p = t.pieces.iter().find(|p| p.tile == tile).unwrap();
        (p.rect.left + p.rect.width / 2.0, p.rect.top + p.rect.height / 2.0)
    }

    #[test]
    fn the_model_writes_the_figures_rows_tabs_and_cards() {
        let (mut ui, mut t) = (tree(), InventoryTiles::default());
        let menu = run(&mut ui, Some(shown(3, 1)), &mut t).unwrap();
        for (card, want) in HEADLINE.iter().zip(["52/200", "90/100", "8.0", "1234"]) {
            assert_eq!(words(&ui, card, "_Value"), want);
        }
        assert_eq!(at(&ui, HEADLINE_DR, "visible"), 0.0, "this page has no DR card");
        assert_eq!(at(&ui, BACKGROUND, "visible"), 0.0);
        // The rows, named after their items, the list's own places.
        assert_eq!(rows(&t).len(), 3);
        assert_eq!(ui.tile(rows(&t)[2].tile).name, "Item 3");
        assert_eq!(ui.tile(rows(&t)[2].tile).get("_y").map(Value::num), Some(2.0 * ROW_H));
        let text_tile = list::child(&ui, rows(&t)[1].tile, ROW_TEXT).unwrap();
        assert_eq!(ui.tile(text_tile).get("string").map(|v| v.text().to_string()).as_deref(), Some("Row 2"));
        let mark = |ui: &Ui, i: usize| ui.tile(list::child(ui, rows(&t)[i].tile, ROW_MARK).unwrap()).get("visible").map(Value::num);
        assert_eq!((mark(&ui, 0), mark(&ui, 1)), (Some(1.0), Some(0.0)), "the equipped mark");
        assert_eq!((at(&ui, LIST, "_highlight_y"), at(&ui, LIST, "_selected_height")), (ROW_H, ROW_H));
        // The tabs: five made under the tab line, each with its word.
        let tabs = t.written.tabs.clone();
        assert_eq!(tabs.len(), 5);
        for (row, word) in tabs.iter().zip(TAB_WORDS) {
            assert_eq!(ui.tile(row.tile).get("string").map(|v| v.text().to_string()).as_deref(), Some(word));
            let inner = find_named(&ui, row.tile, TAB_TEXT).unwrap();
            assert_eq!(ui.tile(inner).get("string").map(|v| v.text().to_string()).as_deref(), Some(word), "the file copies it down");
        }
        assert_eq!((at(&ui, TABLINE, "_ButtonCount"), at(&ui, TABLINE, "_CurrentTab")), (5.0, 0.0));
        // The picture, the badge, the skill, the figures, the buttons.
        assert_eq!(words(&ui, ICON, "filename"), "pics\\item.dds");
        assert_eq!(at(&ui, ICON, "visible"), 1.0);
        assert_eq!(words(&ui, BADGE, "filename"), "pics\\badge.dds");
        assert_eq!(words(&ui, SKILL_REQ, "string"), "25");
        assert_eq!(words(&ui, &format!("{CARD}/WeightInfo"), "_Value"), "5.00");
        for hidden in ["IM_EquipButton", "IM_DropButton", "IM_CancelButton", "IM_ModButton"] {
            assert_eq!(at(&ui, &button_path(hidden), "visible"), 0.0, "{hidden}");
        }
        assert_eq!(at(&ui, &button_path("IM_RepairButton"), "visible"), 1.0);
        assert_eq!(words(&ui, &button_path("IM_RepairButton"), "_PCButtonText"), "[R]");
        assert_eq!(at(&ui, CARD, "visible"), 1.0);
        assert_eq!((at(&ui, INVENTORY_MENU, "_EquippableItem"), at(&ui, INVENTORY_MENU, "_KeyringOpen")), (1.0, 0.0));
        assert_eq!(at(&ui, WHEEL, "visible"), 0.0);
        assert_eq!(at(&ui, BUTTON_RECT, "visible"), 1.0);
        assert_eq!(ui.menu(INVENTORY_MENU), Some(menu));
        assert!(describe(&ui, &t.written).contains("3 rows, \"Row 2\" selected, tab 0, Wg 52/200"), "{}", describe(&ui, &t.written));
    }

    #[test]
    fn a_scrolled_list_is_usable_and_only_rows_inside_its_window_are_asked_of() {
        let (mut ui, mut t) = (tree(), InventoryTiles::default());
        // 12 rows of 40 in a window of 200: five whole.
        let s = shown(12, 0);
        let menu = run(&mut ui, Some(s.clone()), &mut t).unwrap();
        assert_eq!(t.written.counted.rows, 12);
        assert_eq!(t.written.counted.rows_in_clip, 5, "the rows below the window are passed over");
        // A tab less (the one showing) and the five rows.
        assert_eq!(t.written.counted.hit_tests, 9);
        assert!(t.view.scroll_to(Side::Player, 4.0));
        run(&mut ui, None, &mut t).unwrap();
        let (kept, total) = (t.written.counted.rows_in_clip, t.written.counted.rows);
        assert!(kept > 0 && kept < total, "{kept} of {total}");
        // What stands under the window's top row is the row the view moved there.
        let scale = at(&ui, LIST, "_scroll_delta");
        let first = (4.0 * scale / ROW_H).ceil() as usize;
        let tile = list::child(&ui, rows(&t)[first].tile, ROW_TEXT).unwrap();
        let hit = fnv_ui::input::hit(&ui, &[menu], middle(&t, tile), &fnv_ui::input::no_measure).unwrap();
        assert_eq!(items_tile(&ui, &t.view, &t.written.tabs, hit.tile), Some(ItemsTile::Row(first)));
        // Past the window's bottom nothing of the list takes the pointer, though rows stand there.
        let list_rect = t.pieces.iter().find(|p| p.tile == list::child(&ui, rows(&t)[11].tile, ROW_TEXT).unwrap()).unwrap().clip.unwrap();
        let below = (list_rect.left + 50.0, list_rect.top + list_rect.height + 5.0);
        let past = fnv_ui::input::hit(&ui, &[menu], below, &fnv_ui::input::no_measure).and_then(|h| items_tile(&ui, &t.view, &t.written.tabs, h.tile));
        assert_eq!(past, None);
    }

    #[test]
    fn a_list_that_scrolls_without_a_window_falls_back() {
        let mut ui = tree_changed(|x| x.replace("<clipwindow>&true;</clipwindow>", ""));
        let mut t = InventoryTiles::default();
        assert_eq!(run(&mut ui, Some(shown(12, 0)), &mut t), Err("the list scrolls and has no window".to_string()));
        // A list that fits needs none.
        let (mut ui, mut t) = (tree_changed(|x| x.replace("<clipwindow>&true;</clipwindow>", "")), InventoryTiles::default());
        assert!(run(&mut ui, Some(shown(2, 0)), &mut t).is_ok());
    }

    #[test]
    fn an_empty_tab_is_a_page() {
        let (mut ui, mut t) = (tree(), InventoryTiles::default());
        run(&mut ui, Some(shown(0, 0)), &mut t).unwrap();
        assert!(rows(&t).is_empty());
        assert_eq!((at(&ui, LIST, "_highlight_y"), at(&ui, LIST, "_selected_height")), (-1.0, 0.0));
        assert_eq!(at(&ui, ICON, "visible"), 0.0);
        assert_eq!(at(&ui, CARD, "visible"), 0.0, "no row, no figures");
        assert_eq!(at(&ui, &button_path("IM_RepairButton"), "visible"), 1.0);
        assert_eq!(at(&ui, &button_path("IM_RepairButton"), "target"), 0.0);
    }

    #[test]
    fn rows_change_with_the_tab_and_keep_their_tiles_tabs_stay() {
        let (mut ui, mut t) = (tree(), InventoryTiles::default());
        run(&mut ui, Some(shown(3, 0)), &mut t).unwrap();
        let (tabs, first) = (t.written.tabs.clone(), rows(&t)[0].tile);
        let mut s = shown(2, 0);
        s.tab = 4;
        run(&mut ui, Some(s), &mut t).unwrap();
        assert_eq!(t.written.tabs, tabs, "the tabs are made once");
        assert_eq!(rows(&t)[0].tile, first, "a row keeps what it stands for");
        assert_eq!(rows(&t).len(), 2);
        assert_eq!(at(&ui, TABLINE, "_CurrentTab"), 4.0);
    }

    #[test]
    fn a_tile_stands_for_its_row_tab_button_or_bar_piece() {
        let (mut ui, mut t) = (tree(), InventoryTiles::default());
        run(&mut ui, Some(shown(2, 0)), &mut t).unwrap();
        let of = |path: &str| items_tile(&ui, &t.view, &t.written.tabs, ui.find_tile(path).unwrap());
        assert_eq!(of(&format!("{}/button_text", button_path("IM_DropButton"))), Some(ItemsTile::Button(Button::Drop)));
        assert_eq!(of(&button_path("IM_CancelButton")), Some(ItemsTile::Button(Button::Cancel)));
        assert_eq!(of(&format!("{LIST}/lb_scrollbar/scrollbar_vert_page_down")), Some(ItemsTile::Bar(BarPiece::PageDown)));
        assert_eq!(of(&format!("{LIST}/lb_scrollbar")), None);
        assert_eq!(of(ICON), None, "an unnamed path of the absent side is no list");
        let tab = t.written.tabs[3].tile;
        assert_eq!(items_tile(&ui, &t.view, &t.written.tabs, find_named(&ui, tab, TAB_TEXT).unwrap()), Some(ItemsTile::Tab(3)));
        let row = list::child(&ui, rows(&t)[1].tile, ROW_TEXT).unwrap();
        assert_eq!(items_tile(&ui, &t.view, &t.written.tabs, row), Some(ItemsTile::Row(1)));
        // The tab line stands where the fixture put it: a tab takes the pointer at its words.
        let menu = ui.menu(INVENTORY_MENU).unwrap();
        let words_tile = find_named(&ui, t.written.tabs[2].tile, TAB_TEXT).unwrap();
        let hit = fnv_ui::input::hit(&ui, &[menu], middle(&t, words_tile), &fnv_ui::input::no_measure).unwrap();
        assert_eq!(items_tile(&ui, &t.view, &t.written.tabs, hit.tile), Some(ItemsTile::Tab(2)));
    }

    #[test]
    fn a_row_or_a_tab_something_lies_over_falls_back() {
        // A hotrect of the file's own over everything: nothing takes the pointer but it.
        let mut ui = tree_changed(|x| x.replace(r#"<image name="IM_Background"/>"#, r#"<image name="IM_Background"/><hotrect name="Cover"><target>&true;</target><depth>500</depth><width>1000</width><height>800</height></hotrect>"#));
        let mut t = InventoryTiles::default();
        let reason = run(&mut ui, Some(shown(2, 0)), &mut t).unwrap_err();
        assert!(reason.starts_with("Row(0) does not take the pointer at ("), "{reason}");
        assert!(t.pieces.is_empty(), "nothing goes to the painter");
    }

    /// The tabs' middles as painted take the pointer, each its own.
    #[track_caller]
    fn assert_each_tab_takes_the_pointer(ui: &Ui, t: &InventoryTiles) {
        let menu = ui.menu(INVENTORY_MENU).unwrap();
        for row in &t.written.tabs {
            let words = find_named(ui, row.tile, TAB_TEXT).unwrap();
            let hit = fnv_ui::input::hit(ui, &[menu], middle(t, words), &fnv_ui::input::no_measure).unwrap();
            assert_eq!(items_tile(ui, &t.view, &t.written.tabs, hit.tile), Some(ItemsTile::Tab(row.key)));
        }
    }

    #[test]
    fn the_tabs_follow_the_x_code_gives_them() {
        // Written for the test: unwritten, the fixture's tabs all stand at its `_x` of 0.
        let (mut ui, mut t) = (tree_changed(|x| x.replace(r#"<rect name="IM_Tabline"><y>560</y></rect>"#, r#"<rect name="IM_Tabline"><y>560</y><width>800</width></rect>"#)), InventoryTiles::default());
        run(&mut ui, Some(shown(2, 0)), &mut t).unwrap();
        assert_eq!(t.written.counted.tab_missed, None);
        assert_each_tab_takes_the_pointer(&ui, &t);
        // Five tabs of 100 in 800: 300 over six lines.
        let xs: Vec<f64> = t.written.tabs.iter().map(|r| num(&ui, r.tile, "x")).collect();
        assert_eq!(xs, [50.0, 200.0, 350.0, 500.0, 650.0]);
        assert_eq!(at(&ui, TABLINE, "_LeftLineLength"), 50.0);
    }

    #[test]
    fn a_line_and_a_tab_by_turns_the_rest_at_the_end() {
        let (gap, xs) = tab_xs(100.0, &[10.0, 20.0, 15.0]);
        assert_eq!(gap, 13.0, "whole lines: 55 over 4");
        assert_eq!(xs, [13.0, 36.0, 69.0]);
        assert_eq!(tab_xs(10.0, &[20.0]), (0.0, vec![0.0]), "too wide: no line");
    }

    #[test]
    fn a_tab_with_an_inner_rect_gives_it_no_place_twice() {
        let mut ui = tree_changed(|x| {
            x.replace(r#"<rect name="IM_Tabline"><y>560</y></rect>"#, r#"<rect name="IM_Tabline"><y>560</y><width>800</width></rect>"#).replace(
                r#"<text name="button_text"><string><copy src="parent()" trait="string"/></string></text>
            </hotrect></template></menu>"#,
                r#"<rect name="stb_text_box"><x>7</x><text name="button_text"><string>Word</string></text></rect>
            </hotrect></template></menu>"#,
            )
        });
        let mut t = InventoryTiles::default();
        run(&mut ui, Some(shown(2, 0)), &mut t).unwrap();
        let tab = t.written.tabs[1].tile;
        assert_eq!(num(&ui, tab, "_x"), 200.0, "the adapter's write");
        assert_eq!(num(&ui, tab, fnv_ui::input::NO_ORIGIN), 1.0);
        let words = find_named(&ui, tab, TAB_TEXT).unwrap();
        let piece = t.pieces.iter().find(|p| p.tile == words).unwrap();
        assert_eq!(piece.rect.left, 7.0, "from the line, not from the tab as well");
    }

    #[test]
    fn a_warning_is_told_once_whatever_the_place_or_the_row() {
        let line = |row: u32, y: u32| told_once(&format!("Row({row}) does not take the pointer at (51, {y}): nothing does"));
        assert_eq!(line(1, 151), line(2, 203));
        assert_ne!(told_once("Tab(1) does not take the pointer at (1, 2): x does"), line(1, 151));
        assert_eq!(told_once("InventoryMenu drawn by hand: a thing missing"), "InventoryMenu drawn by hand: a thing missing");
    }

    #[test]
    fn a_built_in_title_too_wide_gives_way_and_a_setting_stands() {
        let loaded = |built_in: &[&str]| {
            let mut xml = String::new();
            let _ = tree_changed(|x| {
                xml = x.replace("<_Value></_Value></rect>", "<_Title>&-sInventoryWeight;</_Title><_Value></_Value></rect>");
                x
            });
            let mut src = MemSource::default().with("i.xml", &xml);
            src.strings.insert("sInventoryWeight".into(), "Long Word".into());
            src.built_in_strings = built_in.iter().map(|n| n.to_ascii_lowercase()).collect();
            let mut ui = Ui::new();
            ui.load_menu(&src, "i.xml").unwrap();
            ui.update();
            ui
        };
        let (mut ui, mut t) = (loaded(&["sInventoryWeight"]), InventoryTiles::default());
        run(&mut ui, Some(shown(2, 0)), &mut t).unwrap();
        assert_eq!(words(&ui, HEADLINE[0], "_Title"), "Wt", "the strings table's label");
        let (mut ui, mut t) = (loaded(&[]), InventoryTiles::default());
        run(&mut ui, Some(shown(2, 0)), &mut t).unwrap();
        assert_eq!(words(&ui, HEADLINE[0], "_Title"), "Long Word", "a plugin's setting");
    }

    #[test]
    fn cancel_shows_in_a_folder_and_equip_and_drop_never() {
        let (mut ui, mut t) = (tree(), InventoryTiles::default());
        let mut s = shown(2, 0);
        s.keyring = true;
        s.repair.shown = false;
        run(&mut ui, Some(s), &mut t).unwrap();
        assert_eq!(at(&ui, &button_path("IM_CancelButton"), "visible"), 1.0);
        assert_eq!(words(&ui, &button_path("IM_CancelButton"), "_PCButtonText"), "[Back]");
        assert_eq!(at(&ui, &button_path("IM_RepairButton"), "visible"), 0.0, "Cancel in its place");
        assert_eq!(at(&ui, &button_path("IM_EquipButton"), "visible"), 0.0);
        assert_eq!(at(&ui, INVENTORY_MENU, "_KeyringOpen"), 1.0);
    }

    #[test]
    fn mod_shows_on_its_tab_lit_with_a_row() {
        let (mut ui, mut t) = (tree(), InventoryTiles::default());
        let mod_button = button_path("IM_ModButton");
        let mut s = shown(2, 0);
        s.modify = true;
        run(&mut ui, Some(s.clone()), &mut t).unwrap();
        assert_eq!((at(&ui, &mod_button, "visible"), at(&ui, &mod_button, "target")), (1.0, 1.0));
        s.modify_lit = false;
        run(&mut ui, Some(s), &mut t).unwrap();
        assert_eq!((at(&ui, &mod_button, "visible"), at(&ui, &mod_button, "target")), (1.0, 0.0), "dim with no row");
    }

    #[test]
    fn repair_s_word_follows_the_condition_and_holds_on_the_gear_tabs_with_no_row() {
        assert_eq!(repair_word(Some("Repair"), 0, Some(REPAIR_UPKEEP), REPAIR_UPKEEP), None, "the file's");
        assert_eq!(repair_word(Some("Anything else"), 1, None, REPAIR_UPKEEP), Some(REPAIR_UPKEEP));
        assert_eq!(repair_word(None, 1, Some(REPAIR_UPKEEP), REPAIR_UPKEEP), Some(REPAIR_UPKEEP), "as the last selection left it");
        assert_eq!(repair_word(None, 2, Some(REPAIR_UPKEEP), REPAIR_UPKEEP), None, "the file's on the others");
        // A load order defining the setting (a word written for the test): its text, not ours.
        assert_eq!(repair_word(Some("Anything else"), 0, None, "Wartung-T"), Some("Wartung-T"));
    }

    #[test]
    fn the_tabs_read_their_settings_and_fall_back_to_our_words() {
        // Before the settings are read (tests): our words.
        assert_eq!(tab_words(), TAB_WORDS);
        // A load order defining two of them (words written for the test).
        let look = |name: &str, ours: &'static str| match name {
            "sInventoryWeapons" => "Waffen-T",
            "sInventoryAmmo" => "Munition-T",
            _ => ours,
        };
        assert_eq!(tab_words_by(look), ["Waffen-T", "Apparel", "Aid", "Misc", "Munition-T"]);
    }

    #[test]
    fn the_picture_stands_elsewhere_for_ammunition_and_comes_back() {
        let (mut ui, mut t) = (tree(), InventoryTiles::default());
        let mut s = shown(2, 0);
        s.ammo = true;
        run(&mut ui, Some(s), &mut t).unwrap();
        assert_eq!((at(&ui, ICON, "x"), at(&ui, ICON, "width")), ICON_PLACE_AMMO);
        run(&mut ui, Some(shown(2, 0)), &mut t).unwrap();
        assert_eq!((at(&ui, ICON, "x"), at(&ui, ICON, "height")), ICON_PLACE);
    }

    #[test]
    fn effects_stand_low_under_two_rows_and_on_the_first_free_row_else() {
        let (mut ui, mut t) = (tree(), InventoryTiles::default());
        let effects = format!("{CARD}/{}", item_card::EFFECTS);
        let mut s = shown(2, 0);
        s.list.card = Some(Info { condition: Some(0.5), effects: Some("x".into()), ..Info::default() });
        run(&mut ui, Some(s.clone()), &mut t).unwrap();
        assert_eq!(at(&ui, &effects, "y"), EFFECTS_LOW);
        s.list.card = Some(Info { effects: Some("y".into()), ..Info::default() });
        run(&mut ui, Some(s), &mut t).unwrap();
        assert_eq!(at(&ui, &effects, "y"), at(&ui, CARD, "user1"), "one row up: the card's own");
    }

    #[test]
    fn a_tab_something_lies_over_is_told_and_the_page_stays() {
        let mut ui = tree_changed(|x| x.replace(r#"<rect name="IM_Tabline">"#, r#"<hotrect name="Cover"><target>&true;</target><depth>500</depth><y>550</y><width>1000</width><height>60</height></hotrect><rect name="IM_Tabline">"#));
        let mut t = InventoryTiles::default();
        run(&mut ui, Some(shown(2, 0)), &mut t).unwrap();
        let missed = t.written.counted.tab_missed.clone().unwrap();
        assert!(missed.starts_with("Tab(1) does not take the pointer at ("), "{missed}");
        assert!(missed.contains("Cover"), "{missed}");
        assert!(!t.pieces.is_empty(), "the page goes to the painter");
    }

    #[test]
    fn tabs_without_words_fall_back() {
        let mut ui = tree_changed(|x| x.replace(r#"<text name="button_text"><string><copy src="parent()" trait="string"/></string></text>
            </hotrect></template></menu>"#, r#"</hotrect></template></menu>"#));
        let mut t = InventoryTiles::default();
        // The fixture's tab then holds no text tile: its word is written on the tab alone.
        assert_eq!(run(&mut ui, Some(shown(2, 0)), &mut t), Err("tab 0 is not shown".to_string()));
    }

    #[test]
    fn optional_hotkey_button_follows_device_and_selected_item() {
        let mut ui = tree_changed(|xml| xml.replace("</rect>\n              <image name=\"IM_ItemIcon\"/>", "<image name=\"IM_HotkeyButton\"><text name=\"button_text\"><string>Bind</string></text></image></rect>\n              <image name=\"IM_ItemIcon\"/>"));
        let path = button_path("IM_HotkeyButton");
        assert!(ui.find_tile(&path).is_some());
        let mut tiles = InventoryTiles::default();
        let mut snapshot = shown(1, 0);
        snapshot.hints.hotkey = "RB)".into();
        run(&mut ui, Some(snapshot.clone()), &mut tiles).unwrap();
        assert_eq!(at(&ui, &path, "visible"), 1.0);
        assert_eq!(at(&ui, &path, "target"), 1.0);
        assert_eq!(words(&ui, &path, "_PCButtonText"), "RB)");
        assert_eq!(items_tile(&ui, &tiles.view, &tiles.written.tabs, ui.find_tile(&path).unwrap()), Some(ItemsTile::Button(Button::Hotkey)));
        snapshot.hotkey_lit = false;
        run(&mut ui, Some(snapshot.clone()), &mut tiles).unwrap();
        assert_eq!(at(&ui, &path, "target"), 0.0);
        snapshot.hints.hotkey.clear();
        run(&mut ui, Some(snapshot), &mut tiles).unwrap();
        assert_eq!(at(&ui, &path, "visible"), 0.0);
    }

    #[test]
    fn a_missing_tile_or_template_keeps_the_hand_built_page() {
        assert_eq!(built(&tree(), None), Tiled::Yes);
        for (name, missing) in [("IM_Tabline", TABLINE), ("IM_ItemIcon", ICON), ("IM_ButtonRect", BUTTON_RECT)] {
            let ui = tree_changed(|x| x.replace(&format!("name=\"{name}\""), "name=\"Modded\""));
            assert_eq!(built(&ui, None), Tiled::No(format!("{missing} missing")));
        }
        for piece in ["IM_DropButton", "scrollbar_vert_page_up", "WeightInfo", "IM_Headline_PlayerCapsInfo"] {
            let ui = tree_changed(|x| x.replace(&format!("name=\"{piece}\""), "name=\"Modded\""));
            assert!(matches!(built(&ui, None), Tiled::No(reason) if reason.contains(piece)), "{piece}");
        }
        let ui = tree_changed(|x| x.replace("name=\"ListItemText\"", "name=\"Modded\""));
        assert_eq!(built(&ui, None), Tiled::No("IM_InventoryListTemplate/IM_InventoryListTemplateRect/ListItemText missing".into()));
        let ui = tree_changed(|x| x.replace("name=\"IM_InventoryListTemplate\"", "name=\"Modded\""));
        assert_eq!(built(&ui, None), Tiled::No("IM_InventoryListTemplate missing".into()));
        let ui = tree_changed(|x| x.replace("name=\"TabButtonTemplate\"", "name=\"Modded\""));
        assert_eq!(built(&ui, None), Tiled::No("TabButtonTemplate missing".into()));
        // None of those the page can do without.
        for piece in ["IM_HotkeyButton", "IM_Headline_PlayerDRInfo", "IM_HotKeyWheel"] {
            let ui = tree_changed(|x| x.replace(&format!("name=\"{piece}\""), "name=\"Modded\""));
            assert_eq!(built(&ui, None), Tiled::Yes, "{piece}");
        }
    }

    #[test]
    fn the_test_aid_cuts_a_name_and_the_page_recovers() {
        let (mut ui, mut t) = (tree(), InventoryTiles::default());
        t.cut = Some("IM_InventoryListTemplate".into());
        assert_eq!(run(&mut ui, Some(shown(2, 0)), &mut t), Err("IM_InventoryListTemplate missing".to_string()));
        assert!(rows(&t).is_empty(), "nothing made");
        t.cut = Some("IM_ItemIcon".into());
        assert_eq!(run(&mut ui, Some(shown(2, 0)), &mut t), Err(format!("{ICON} missing")));
        t.cut = None;
        assert!(run(&mut ui, Some(shown(2, 0)), &mut t).is_ok());
    }

    /// Three failed passes and the page waits while nothing changes; a tile a script puts back, or
    /// the menu built again, is a change of the tree, and the next pass tries again.
    #[test]
    fn a_page_that_fell_back_tries_again_when_the_tree_changes() {
        let mut ui = tree_changed(|x| x.replace(r#"<image name="IM_ItemIcon"/>"#, r#"<image name="Modded"/>"#));
        let mut t = InventoryTiles::default();
        let fail = |ui: &mut Ui, t: &mut InventoryTiles| {
            let reason = run(ui, Some(shown(2, 0)), t).unwrap_err();
            t.written.failed(Some((0, false)), Some(TreeMark::of(ui)));
            reason
        };
        for _ in 0..FAILING_PASSES {
            assert_eq!(fail(&mut ui, &mut t), format!("{ICON} missing"));
        }
        assert!(t.written.waits(0, false, TreeMark::of(&ui), false), "an unchanged tree waits");
        assert!(!t.written.waits(0, false, TreeMark::of(&ui), true), "a changed model is tried");
        // A script puts the missing picture back: same menu, another shape.
        assert!(ui.inject_text(&MemSource::default(), "InventoryMenu/GLOW_BRANCH/IM_MainRect", r#"<image name="IM_ItemIcon"/>"#));
        assert!(!t.written.waits(0, false, TreeMark::of(&ui), false), "a tile put back is tried again");
        // The same again, the menu built again whole this time.
        let mut ui = tree_changed(|x| x.replace(r#"<image name="IM_ItemIcon"/>"#, r#"<image name="Modded"/>"#));
        let mut t = InventoryTiles::default();
        for _ in 0..FAILING_PASSES {
            fail(&mut ui, &mut t);
        }
        assert!(t.written.waits(0, false, TreeMark::of(&ui), false));
        let menu = ui.menu(INVENTORY_MENU);
        rebuild_fixture(&mut ui);
        assert_ne!(ui.menu(INVENTORY_MENU), menu, "a new menu tile");
        assert!(!t.written.waits(0, false, TreeMark::of(&ui), false), "a menu built again is tried again");
        assert!(run(&mut ui, Some(shown(2, 0)), &mut t).is_ok(), "and it comes from its tiles");
    }

    #[test]
    fn the_panel_hides_the_buttons_and_the_picture_and_the_figures_stay() {
        let (mut ui, mut t) = (tree(), InventoryTiles::default());
        let mut s = shown(2, 0);
        s.panel = Some(2);
        run(&mut ui, Some(s), &mut t).unwrap();
        assert_eq!((at(&ui, WHEEL, "visible"), at(&ui, BUTTON_RECT, "visible")), (0.0, 0.0), "the panel is the owner's, not the file's wheel");
        assert_eq!((at(&ui, ICON, "visible"), at(&ui, BADGE, "visible"), at(&ui, SKILL_REQ, "visible")), (0.0, 0.0, 0.0));
        assert_eq!(words(&ui, &format!("{CARD}/WeightInfo"), "_Value"), "5.00", "the figures stay");
        // Let go: back as it was.
        run(&mut ui, Some(shown(2, 0)), &mut t).unwrap();
        assert_eq!((at(&ui, WHEEL, "visible"), at(&ui, BUTTON_RECT, "visible"), at(&ui, ICON, "visible")), (0.0, 1.0, 1.0));
    }

    #[test]
    fn repair_shows_dim_by_target_and_its_word_follows_the_condition() {
        let mut ui = tree_changed(|x| x.replace(r#"<image name="IM_RepairButton"><target>&true;</target>"#, r#"<image name="IM_RepairButton"><string>File</string><target>&true;</target>"#));
        let mut t = InventoryTiles::default();
        let mut s = shown(2, 0);
        s.repair = RepairButton { shown: true, usable: false, word: Some("Other") };
        run(&mut ui, Some(s.clone()), &mut t).unwrap();
        let repair = button_path("IM_RepairButton");
        assert_eq!((at(&ui, &repair, "visible"), at(&ui, &repair, "target")), (1.0, 0.0));
        assert_eq!(words(&ui, &repair, "string"), "Other");
        // Back to the file's own, whatever it is.
        s.repair = RepairButton { shown: true, usable: true, word: None };
        run(&mut ui, Some(s), &mut t).unwrap();
        assert_eq!(at(&ui, &repair, "target"), 1.0);
        assert_eq!(words(&ui, &repair, "string"), "File");
    }

    #[test]
    fn the_screen_menu_has_no_title_and_the_device_gets_it_back() {
        let (mut ui, mut t) = (tree(), InventoryTiles::default());
        let mut s = shown(2, 0);
        s.device = false;
        run(&mut ui, Some(s), &mut t).unwrap();
        assert_eq!(at(&ui, TITLE, "visible"), 0.0);
        run(&mut ui, Some(shown(2, 0)), &mut t).unwrap();
        assert_eq!(at(&ui, LINE, "x"), 300.0, "the file's place again");
        assert_ne!(at(&ui, TITLE, "visible"), 0.0);
    }

    #[test]
    fn a_new_tree_starts_over_and_a_new_pass_makes_nothing_twice() {
        let (mut ui, mut t) = (tree(), InventoryTiles::default());
        let s = shown(2, 0);
        run(&mut ui, Some(s.clone()), &mut t).unwrap();
        run(&mut ui, Some(s.clone()), &mut t).unwrap();
        run(&mut ui, None, &mut t).unwrap();
        let line = ui.find_tile(TABLINE).unwrap();
        let live = |ui: &Ui, p: TileId| ui.tile(p).children.iter().filter(|&&c| ui.is_live(c)).count();
        assert_eq!(live(&ui, line), 5, "five tabs, not ten");
        let old = rows(&t)[0].tile;
        let mut ui = tree();
        run(&mut ui, Some(s), &mut t).unwrap();
        assert_ne!(rows(&t)[0].tile, old);
        assert_eq!(rows(&t).len(), 2);
        assert_eq!(live(&ui, ui.find_tile(TABLINE).unwrap()), 5);
    }

    #[test]
    fn nothing_written_yet_is_no_page() {
        let (mut ui, mut t) = (tree(), InventoryTiles::default());
        assert_eq!(run(&mut ui, None, &mut t), Err("nothing to show yet".to_string()));
    }

    #[test]
    fn a_physical_key_is_the_hand_built_pages_a_scripts_tap_the_files() {
        // A key the page acts on itself, pressed: not through the file.
        for key in ["R", "X", "E"] {
            assert!(!key_acts(key, &[key]), "{key}");
            assert!(key_acts(key, &[]), "a script's tap of {key}");
        }
        // The controller's X has no key stood in for it and drops by `items::keys`: never twice.
        assert!(!key_acts("xbuttonx", &[]));
        assert!(!key_acts("xbuttony", &["xbuttony"]));
        // A key the page does not take is the file's.
        assert!(key_acts("Q", &["Q"]));
    }

    #[test]
    fn every_hint_and_word_written_has_its_source() {
        assert_eq!(TAB_WORDS.len(), crate::pipboy::items::TABS.len());
        // The settings the hints and the condition's title read are in the strings table.
        for name in ["sPCMenuHintR", "sPCMenuHintX", "sPCMenuHintE", CONDITION_TITLE] {
            assert!(fnv_ui::strings::built_in(name).is_some(), "{name}");
        }
    }

    #[test]
    fn the_page_is_eligible_only_on_items_with_its_lists_closed() {
        let on = TileMenus { inventory: true, ..TileMenus::default() };
        let mut pip = PipBoy::default();
        pip.page = Page::Items;
        assert!(!eligible(on, &pip, true), "down: not shown");
        pip.phase = crate::pipboy::Phase::Up;
        assert!(eligible(on, &pip, true));
        assert!(!eligible(TileMenus::default(), &pip, true), "off, never");
        assert!(!eligible(on, &pip, false), "the repair list or the mod menu has the page");
        pip.page = Page::Stats;
        assert!(!eligible(on, &pip, true));
        // The two lists read as closed without their resources.
        assert!(lists_closed(None, None));
    }

    struct NativeInventoryFixture {app:bevy::prelude::App,row:Vec2,tab:Vec2}
    #[derive(bevy::prelude::Resource,Default)]
    struct NativeGuardSeen {selects:usize,used:usize,tabs:usize,ones:usize}
    fn native_inventory_fixture()->NativeInventoryFixture {
        use bevy::{prelude::*,input::InputPlugin,picking::pointer::PointerInput};
        use crate::pipboy::{self,touch_items::TouchItems};
        fn project(mut touch:ResMut<TouchItems>){touch.project(Some);}
        fn apply(mut input:MessageReader<MenuInput>,mut active:ResMut<ActiveMenu>,mut sim:ResMut<crate::sim::SimState>,mut touch:ResMut<TouchItems>,mut keys:ResMut<Hotkeys>,mut seen:ResMut<NativeGuardSeen>,mut tabs:MessageReader<PipBoyInput>){
            seen.tabs+=tabs.read().count();
            for cmd in input.read(){if let Some(m)=active.0.as_mut(){
                if matches!(cmd.0,crate::menu::model::Command::Select(..)){seen.selects+=1;}
                if matches!(cmd.0,crate::menu::model::Command::One){seen.ones+=1;}
                seen.used+=usize::from(m.apply(&mut sim.world,cmd.0).used.is_some());
            }}
            touch.assign_pending(&active,&mut keys,&mut sim.world);
        }
        let mut sim=crate::sim::test_world::sim((1..=12).map(|i|{let mut b=crate::sim::test_world::item_base(0xF200+i);b.name=Some(format!("Item {i:02}"));b}).collect(),vec![]);
        for i in 1..=12 {sim.world.add_item(FormId::PLAYER_REF,FormId(0xF200+i),3).unwrap();}
        let mut model=crate::menu::model::MenuModel::open_inventory(&mut sim.world).unwrap();model.apply(&mut sim.world,crate::menu::model::Command::SetCategory(Side::Player,Category::Aid));
        let mut drawing=shown(12,0);drawing.tab=2;drawing.list.lists[Side::Player as usize].category=Category::Aid;
        drawing.list.lists[Side::Player as usize].rows=model.list(Side::Player).rows.iter().enumerate().map(|(i,r)|item_menu::RowShown{
            key:RowKey::at(&model.list(Side::Player).rows,i).unwrap(),name:r.name.clone(),text:r.name.clone(),equipped:false,dim:false,ysi:None}).collect();
        let (mut ui,mut tiles)=(tree(),InventoryTiles::default());let menu=run(&mut ui,Some(drawing),&mut tiles).unwrap();tiles.shown=Some(menu);
        let row_text=list::child(&ui,rows(&tiles)[0].tile,ROW_TEXT).unwrap();
        let tab_text=find_named(&ui,tiles.written.tabs.iter().find(|t|t.key==4).unwrap().tile,TAB_TEXT).unwrap();
        let row_point=middle(&tiles,row_text);let tab_point=middle(&tiles,tab_text);
        for (point,want) in [(row_point,ItemsTile::Row(0)),(tab_point,ItemsTile::Tab(4))] {
            let hit=fnv_ui::input::hit(&ui,&[menu],point,&fnv_ui::input::no_measure).unwrap();
            assert_eq!(items_tile(&ui,&tiles.view,&tiles.written.tabs,hit.tile),Some(want),"fixture's painted text center must reach its actual native target");
        }
        let mut script=ScriptState::over_no_plugins();{let game=&mut script.rt.as_mut().unwrap().game;game.ui_files=Some(Box::new(MemSource::default()));game.ui_tree=ui;game.menu=Some(fnv_game::game::menus::INVENTORY);}
        let mut pip=PipBoy::default();pip.phase=pipboy::Phase::Up;pip.page=Page::Items;pip.tabs=[0,2,0];
        let mut app=App::new();app.add_plugins((MinimalPlugins,InputPlugin)).init_resource::<bevy::picking::pointer::PointerMap>()
            .init_resource::<TouchItems>().init_resource::<pipboy::touch_scroll::WheelFrame>().init_resource::<NativeGuardSeen>()
            .init_resource::<pipboy::PipBoyAid>().init_resource::<pipboy::PointerMoved>()
            .init_resource::<PipRepair>().init_resource::<PipItemMod>().init_resource::<HotkeyHold>().init_resource::<Hotkeys>()
            .init_resource::<pipboy::KeysTaken>().add_systems(First,pipboy::clear_keys_taken)
            .add_message::<PointerInput>().add_message::<bevy::window::WindowEvent>().add_message::<PipBoyInput>()
            .add_message::<TabHovered>().add_message::<MenuInput>().add_message::<RepairInput>().add_message::<ModInput>()
            .insert_resource(script).insert_resource(sim).insert_resource(ActiveMenu(Some(model))).insert_resource(tiles)
            .insert_resource(TileMenus{inventory:true,..default()})
            .insert_resource(pip);

        let camera=app.world_mut().spawn_empty().id();app.insert_resource(PipBoyScreen{image:Handle::default(),camera,on_model:false,pointer:None,scale:1.0})
            .add_plugins(pipboy::touch_scroll::TouchPointerPlugin)
            .add_systems(Update,(feed,pipboy::apply_input,pipboy::items::keys,apply).chain()).add_systems(Last,project);
        app.update();
        NativeInventoryFixture {app,row:Vec2::new(row_point.0 as f32,row_point.1 as f32),tab:Vec2::new(tab_point.0 as f32,tab_point.1 as f32)}
    }
    #[test]
    fn actual_mobile_inventory_swipe_scrolls_without_using_or_assigning_and_native_tab_tap_dispatches_once(){
        use crate::pipboy::touch_items::TouchItems;
        use bevy::{prelude::*,input::touch::{TouchInput,TouchPhase}};
        let NativeInventoryFixture{mut app,row:row_point,tab:tab_point}=native_inventory_fixture();
        let mut e=TouchInput{window:Entity::PLACEHOLDER,id:94,phase:TouchPhase::Started,position:Vec2::new(row_point.x,row_point.y),force:None};
        app.world_mut().resource_mut::<TouchItems>().event(&e,0);app.update();app.update();
        e.phase=TouchPhase::Moved;e.position.y-=60.;app.world_mut().resource_mut::<TouchItems>().event(&e,50);
        e.phase=TouchPhase::Ended;app.world_mut().resource_mut::<TouchItems>().event(&e,100);app.world_mut().resource_mut::<TouchItems>().release_over(Some(2));
        for _ in 0..4 {app.update();}
        assert_eq!(app.world().resource::<ActiveMenu>().0.as_ref().unwrap().selected[Side::Player as usize],3);
        assert!(app.world().resource::<Hotkeys>().0.iter().all(Option::is_none));
        for i in 1..=12 {assert_eq!(app.world_mut().resource_mut::<crate::sim::SimState>().world.item_count(FormId::PLAYER_REF,FormId(0xF200+i)).unwrap(),3);}
        app.world_mut().resource_mut::<Messages<PipBoyInput>>().clear();
        e.id=95;e.position=Vec2::new(tab_point.x,tab_point.y);e.phase=TouchPhase::Started;app.world_mut().resource_mut::<TouchItems>().event(&e,200);
        e.phase=TouchPhase::Ended;app.world_mut().resource_mut::<TouchItems>().event(&e,250);for _ in 0..4 {app.update();}
        assert_eq!(app.world().resource::<PipBoy>().tab(),4);
        let tabs:Vec<_>=app.world_mut().resource_mut::<Messages<PipBoyInput>>().drain().collect();
        assert_eq!(tabs.iter().filter(|i|**i==PipBoyInput::Tab(4)).count(),1);
    }


    fn redraw_mobile_inventory(model:&crate::menu::model::MenuModel,script:&mut ScriptState,tiles:&mut InventoryTiles){
        let mut drawing=shown(model.list(Side::Player).rows.len(),model.selected[Side::Player as usize]);
        drawing.tab=2;drawing.list.lists[Side::Player as usize].category=Category::Aid;
        drawing.list.lists[Side::Player as usize].rows=model.list(Side::Player).rows.iter().enumerate().map(|(i,r)|item_menu::RowShown{
            key:RowKey::at(&model.list(Side::Player).rows,i).unwrap(),name:r.name.clone(),text:r.name.clone(),equipped:false,dim:false,ysi:None}).collect();
        let ui=&mut script.rt.as_mut().unwrap().game.ui_tree;
        let menu=run(ui,Some(drawing),tiles).unwrap();tiles.shown=Some(menu);
    }
    fn native_guard_event(app:&mut bevy::prelude::App,id:u64,phase:bevy::input::touch::TouchPhase,point:Vec2){
        use crate::pipboy::touch_items::TouchItems;
        assert!(app.world_mut().resource_mut::<TouchItems>().event(&bevy::input::touch::TouchInput{
            window:Entity::PLACEHOLDER,id,phase,position:point,force:None},100));
    }
    fn swipe_navigation(app:&mut bevy::prelude::App,id:u64,start:Vec2,delta:Vec2){
        use bevy::input::touch::TouchPhase;
        native_guard_event(app,id,TouchPhase::Started,start);app.update();app.update();
        native_guard_event(app,id,TouchPhase::Moved,start+delta);
        native_guard_event(app,id,TouchPhase::Ended,start+delta);
        for _ in 0..6 {app.update();}
    }
    fn repaint_navigation_inventory(app:&mut bevy::prelude::App){
        let tab=app.world().resource::<PipBoy>().tab();
        let drawing={let m=app.world().resource::<ActiveMenu>().0.as_ref().unwrap();let mut d=shown(m.list(Side::Player).rows.len(),m.selected[Side::Player as usize]);
            d.tab=tab;d.list.lists[Side::Player as usize].category=m.category[Side::Player as usize];d.list.pointer_pick=m.pointer_pick;d.list.navigated=m.navigated;
            d.list.lists[Side::Player as usize].rows=m.list(Side::Player).rows.iter().enumerate().map(|(i,r)|item_menu::RowShown{key:RowKey::at(&m.list(Side::Player).rows,i).unwrap(),name:r.name.clone(),text:r.name.clone(),equipped:false,dim:false,ysi:None}).collect();d};
        let mut tiles=app.world_mut().remove_resource::<InventoryTiles>().unwrap();
        {let mut script=app.world_mut().resource_mut::<ScriptState>();let menu=run(&mut script.rt.as_mut().unwrap().game.ui_tree,Some(drawing),&mut tiles).unwrap();tiles.shown=Some(menu);}
        app.insert_resource(tiles);
    }
    #[test]
    fn actual_native_tab_swipe_tap_vertical_and_cancel_have_one_action_owner(){
        use bevy::{input::touch::TouchPhase,prelude::*};
        for mode in 0..4 {
            let NativeInventoryFixture{mut app,tab,..}=native_inventory_fixture();
            app.world_mut().resource_mut::<crate::pipboy::tab_swipe::Options>().enabled=true;
            let region={let t=app.world().resource::<InventoryTiles>();let script=app.world().resource::<ScriptState>();
                touch_navigation(t,&script.rt.as_ref().unwrap().game.ui_tree).into_iter().find(|r|r.rect.contains(tab)).expect("actual painted tab text region")};
            assert_eq!(region.kind,crate::pipboy::tab_swipe::Kind::Subtabs);
            native_guard_event(&mut app,270,TouchPhase::Started,tab);app.update();app.update();
            assert_eq!(app.world().resource::<PipBoy>().tab(),2,"provisional press must not change native tab");
            match mode {
                0=>{native_guard_event(&mut app,270,TouchPhase::Moved,tab-Vec2::X*60.);native_guard_event(&mut app,270,TouchPhase::Ended,tab-Vec2::X*60.);},
                1=>native_guard_event(&mut app,270,TouchPhase::Ended,tab),
                2=>{native_guard_event(&mut app,270,TouchPhase::Moved,tab-Vec2::Y*60.);native_guard_event(&mut app,270,TouchPhase::Ended,tab);},
                _=>{native_guard_event(&mut app,270,TouchPhase::Moved,tab-Vec2::X*60.);native_guard_event(&mut app,270,TouchPhase::Canceled,tab-Vec2::X*60.);},
            }
            for _ in 0..6 {app.update();}
            assert_eq!(app.world().resource::<PipBoy>().tab(),match mode {0=>3,1=>4,_=>2});
            let seen=app.world().resource::<NativeGuardSeen>();
            assert_eq!(seen.tabs,usize::from(mode<2));assert_eq!((seen.ones,seen.used),(0,0));
            assert!(app.world().resource::<Hotkeys>().0.iter().all(Option::is_none));
            for i in 1..=12 {assert_eq!(app.world_mut().resource_mut::<crate::sim::SimState>().world.item_count(FormId::PLAYER_REF,FormId(0xF200+i)).unwrap(),3);}
        }
    }
    #[test]
    fn actual_painted_native_title_swipe_changes_main_section_without_consuming_inventory(){
        let NativeInventoryFixture{mut app,..}=native_inventory_fixture();
        app.world_mut().resource_mut::<crate::pipboy::tab_swipe::Options>().enabled=true;
        let title={let t=app.world().resource::<InventoryTiles>();let script=app.world().resource::<ScriptState>();
            touch_navigation(t,&script.rt.as_ref().unwrap().game.ui_tree).into_iter().find(|r|r.kind==crate::pipboy::tab_swipe::Kind::Sections).expect("actual painted title").rect.center()};
        swipe_navigation(&mut app,271,title,Vec2::X*60.);
        assert_eq!(app.world().resource::<PipBoy>().page,Page::Stats);
        assert_eq!(app.world().resource::<NativeGuardSeen>().tabs,1);
        assert_eq!(app.world().resource::<NativeGuardSeen>().used,0);
    }
    #[test]
    fn actual_native_swipe_return_restores_inventory_row_identity_and_scroll_after_category_route(){
        let NativeInventoryFixture{mut app,tab,..}=native_inventory_fixture();
        app.world_mut().resource_mut::<crate::pipboy::tab_swipe::Options>().enabled=true;
        {
            let mut active=app.world_mut().resource_mut::<ActiveMenu>();let model=active.0.as_mut().unwrap();
            model.selected[Side::Player as usize]=6;model.scroll[Side::Player as usize]=3;
        }
        let expected={let m=app.world().resource::<ActiveMenu>().0.as_ref().unwrap();RowKey::at(&m.list(Side::Player).rows,6).unwrap()};
        repaint_navigation_inventory(&mut app);
        {
            let mut tiles=app.world_mut().remove_resource::<InventoryTiles>().unwrap();
            assert!(tiles.view.scroll_to(Side::Player,3.0));
            {let mut script=app.world_mut().resource_mut::<ScriptState>();let ui=&mut script.rt.as_mut().unwrap().game.ui_tree;
                tiles.view.rewrite(ui);run(ui,None,&mut tiles).unwrap();
                assert_eq!(ui.find_tile(LIST).and_then(|l|list::value_shown(ui,l)),Some(3.0));}
            app.insert_resource(tiles);
        }
        swipe_navigation(&mut app,272,tab,-Vec2::X*60.);
        assert_eq!(app.world().resource::<PipBoy>().tab(),3);
        assert_eq!(app.world().resource::<ActiveMenu>().0.as_ref().unwrap().category[Side::Player as usize],Category::Misc);
        // Repaint the actual new category into the same native tile tree before returning.
        repaint_navigation_inventory(&mut app);
        swipe_navigation(&mut app,273,tab,Vec2::X*60.);
        assert_eq!(app.world().resource::<PipBoy>().tab(),2);
        let model=app.world().resource::<ActiveMenu>().0.as_ref().unwrap();
        assert_eq!(RowKey::at(&model.list(Side::Player).rows,model.selected[Side::Player as usize]),Some(expected));
        assert_eq!(model.scroll[Side::Player as usize],3);
        assert_eq!((app.world().resource::<NativeGuardSeen>().ones,app.world().resource::<NativeGuardSeen>().used),(0,0));
        repaint_navigation_inventory(&mut app);
        let tiles=app.world().resource::<InventoryTiles>();let script=app.world().resource::<ScriptState>();let ui=&script.rt.as_ref().unwrap().game.ui_tree;
        assert_eq!(tiles.view.value(Side::Player),3.0);
        assert_eq!(ui.find_tile(LIST).and_then(|l|list::value_shown(ui,l)),Some(3.0),"actual repainted native scroll must match bookmark");
    }
    #[test]
    fn native_row_release_cannot_use_a_repainted_neighbor_and_a_fresh_tap_uses_only_that_row_once(){
        use bevy::prelude::*;use bevy::input::touch::TouchPhase;
        use crate::pipboy::touch_items::TouchItems;
        #[derive(Resource)]struct Refresh(bool);
        fn refresh(mut flag:ResMut<Refresh>,touch:Res<TouchItems>,mut active:ResMut<ActiveMenu>,mut script:ResMut<ScriptState>,mut tiles:ResMut<InventoryTiles>){
            if flag.0 && touch.pointer_frame().tap {
                flag.0=false;
                let model=active.0.as_mut().unwrap();model.lists[Side::Player as usize].rows.swap(0,1);
                model.selected[Side::Player as usize]=0;
                redraw_mobile_inventory(model,&mut script,&mut tiles);
            }
        }
        let NativeInventoryFixture{mut app,row,..}=native_inventory_fixture();
        app.insert_resource(Refresh(true)).add_systems(Update,refresh.before(feed));
        native_guard_event(&mut app,101,TouchPhase::Started,row);app.update();app.update();
        native_guard_event(&mut app,101,TouchPhase::Ended,row);for _ in 0..4{app.update();}
        assert!(!app.world().resource::<Refresh>().0,"actual page repaint must have run after projected release but before native feed");
        assert_eq!((app.world().resource::<NativeGuardSeen>().selects,app.world().resource::<NativeGuardSeen>().used),(0,0));
        for id in [0xF201,0xF202] {assert_eq!(app.world_mut().resource_mut::<crate::sim::SimState>().world.item_count(FormId::PLAYER_REF,FormId(id)).unwrap(),3);}
        assert!(app.world().resource::<Hotkeys>().0.iter().all(Option::is_none));
        native_guard_event(&mut app,102,TouchPhase::Started,row);native_guard_event(&mut app,102,TouchPhase::Ended,row);
        for _ in 0..4{app.update();}
        assert_eq!((app.world().resource::<NativeGuardSeen>().selects,app.world().resource::<NativeGuardSeen>().used),(1,1));
        assert_eq!(app.world_mut().resource_mut::<crate::sim::SimState>().world.item_count(FormId::PLAYER_REF,FormId(0xF202)).unwrap(),2);
        assert_eq!(app.world_mut().resource_mut::<crate::sim::SimState>().world.item_count(FormId::PLAYER_REF,FormId(0xF201)).unwrap(),3);
    }
    #[test]
    fn native_tab_border_jitter_cannot_choose_an_unpressed_tab_but_a_fresh_matching_tap_works(){
        use bevy::prelude::*;use bevy::input::touch::TouchPhase;
        let NativeInventoryFixture{mut app,..}=native_inventory_fixture();
        let (start,end)={
            let tiles=app.world().resource::<InventoryTiles>();let script=app.world().resource::<ScriptState>();let ui=&script.rt.as_ref().unwrap().game.ui_tree;
            let menu=tiles.shown.unwrap();let hits=fnv_ui::input::hit_candidates(ui,menu,&fnv_ui::input::no_measure);
            let rect=|key|{let tile=tiles.written.tabs.iter().find(|t|t.key==key).unwrap().tile;hits.iter().find(|h|h.under==tile).unwrap().region()};
            let a=rect(3);let b=rect(4);let y=(a.top.max(b.top)+(a.top+a.height).min(b.top+b.height))*0.5;
            (Vec2::new((a.left+a.width-1.)as f32,y as f32),Vec2::new((b.left+1.)as f32,y as f32))
        };
        assert!(start.distance(end)<12.0,"fixture must remain below the drag threshold");
        native_guard_event(&mut app,103,TouchPhase::Started,start);app.update();app.update();
        native_guard_event(&mut app,103,TouchPhase::Ended,end);for _ in 0..4{app.update();}
        assert_eq!(app.world().resource::<PipBoy>().tab(),2);
        assert_eq!(app.world().resource::<NativeGuardSeen>().tabs,0);
        native_guard_event(&mut app,104,TouchPhase::Started,end);native_guard_event(&mut app,104,TouchPhase::Ended,end);for _ in 0..4{app.update();}
        assert_eq!(app.world().resource::<PipBoy>().tab(),4);
        assert_eq!(app.world().resource::<NativeGuardSeen>().tabs,1);
    }


    #[test]
    fn native_row_hover_cannot_upgrade_an_initial_select_into_use_on_release(){
        use bevy::input::touch::TouchPhase;
        let NativeInventoryFixture{mut app,..}=native_inventory_fixture();
        let point={let t=app.world().resource::<InventoryTiles>();let script=app.world().resource::<ScriptState>();
            let ui=&script.rt.as_ref().unwrap().game.ui_tree;let text=list::child(ui,rows(t)[1].tile,ROW_TEXT).unwrap();
            let p=middle(t,text);Vec2::new(p.0 as f32,p.1 as f32)};
        assert_eq!(app.world().resource::<ActiveMenu>().0.as_ref().unwrap().selected[Side::Player as usize],0);
        native_guard_event(&mut app,105,TouchPhase::Started,point);app.update();app.update();
        native_guard_event(&mut app,105,TouchPhase::Ended,point);for _ in 0..4{app.update();}
        assert_eq!(app.world().resource::<ActiveMenu>().0.as_ref().unwrap().selected[Side::Player as usize],1);
        assert_eq!((app.world().resource::<NativeGuardSeen>().selects,app.world().resource::<NativeGuardSeen>().used),(0,0));
        assert_eq!(app.world_mut().resource_mut::<crate::sim::SimState>().world.item_count(FormId::PLAYER_REF,FormId(0xF202)).unwrap(),3);
        native_guard_event(&mut app,106,TouchPhase::Started,point);native_guard_event(&mut app,106,TouchPhase::Ended,point);for _ in 0..4{app.update();}
        assert_eq!((app.world().resource::<NativeGuardSeen>().selects,app.world().resource::<NativeGuardSeen>().used),(1,1));
        assert_eq!(app.world_mut().resource_mut::<crate::sim::SimState>().world.item_count(FormId::PLAYER_REF,FormId(0xF202)).unwrap(),2);
    }


    fn native_scrollbar_fixture() -> (bevy::prelude::App, Vec2) {
        let NativeInventoryFixture { mut app, .. } = native_inventory_fixture();
        let mut script = app.world_mut().remove_resource::<ScriptState>().unwrap();
        let mut tiles = app.world_mut().remove_resource::<InventoryTiles>().unwrap();
        script.rt.as_mut().unwrap().game.ui_tree = tree_changed(|xml| xml
            .replace("<image name=\"lb_scrollbar\">", "<image name=\"lb_scrollbar\"><x>270</x><width>20</width><height>200</height>")
            .replace("<image name=\"scrollbar_vert_marker\"/>", r#"<image name="scrollbar_vert_marker"><target>&true;</target><width>20</width><height>40</height><_step_height>20</_step_height><y><copy src="parent()" trait="_current_value"/><mul>20</mul></y></image>"#)
            .replace("<hotrect name=\"scrollbar_vert_page_down\"/>", r#"<hotrect name="scrollbar_vert_page_down"><target>&true;</target><y>40</y><width>20</width><height>160</height></hotrect>"#));
        let model = app.world().resource::<ActiveMenu>().0.as_ref().unwrap();
        redraw_mobile_inventory(model, &mut script, &mut tiles);
        let ui = &script.rt.as_ref().unwrap().game.ui_tree;
        let list_tile = tiles.view.list_tile(ui, Side::Player).unwrap();
        let marker = list::child(ui, list::child(ui, list_tile, list::SCROLLBAR).unwrap(), list::MARKER).unwrap();
        let rect = super::super::layout::layout(ui, marker, &|_,_,_| (0.0,0.0), true).into_iter().find(|p| p.tile==marker).unwrap().rect;
        let point = Vec2::new((rect.left+rect.width*0.5) as f32, (rect.top+rect.height*0.5) as f32);
        assert!(touch_target(&tiles,ui,tiles.shown.unwrap(),point,model).unwrap().control(), "actual painted native marker must authorize Control");
        app.insert_resource(script).insert_resource(tiles);
        fn repaint(active:bevy::prelude::Res<ActiveMenu>,mut script:bevy::prelude::ResMut<ScriptState>,mut tiles:bevy::prelude::ResMut<InventoryTiles>) {
            redraw_mobile_inventory(active.0.as_ref().unwrap(), &mut script, &mut tiles);
        }
        app.add_systems(bevy::prelude::Update, repaint.after(feed));
        (app, point)
    }

    #[test]
    fn actual_native_marker_queued_drag_scrolls_painted_list_without_selection_use_or_assignment() {
        use bevy::input::touch::TouchPhase;
        let (mut app, point) = native_scrollbar_fixture();
        // A complete OS batch may arrive before the first point is projected.
        native_guard_event(&mut app, 190, TouchPhase::Started, point);
        native_guard_event(&mut app, 190, TouchPhase::Moved, point+Vec2::new(0.0,80.0));
        native_guard_event(&mut app, 190, TouchPhase::Ended, point+Vec2::new(0.0,80.0));
        for _ in 0..7 { app.update(); }
        let tiles = app.world().resource::<InventoryTiles>();
        assert_eq!(tiles.view.value(Side::Player),4.0,"the real native marker drag changed the list view");
        assert!(tiles.view.drag.is_none(),"projected Ended releases the native drag");
        let script=app.world().resource::<ScriptState>();let ui=&script.rt.as_ref().unwrap().game.ui_tree;
        let list_tile=tiles.view.list_tile(ui,Side::Player).unwrap();
        assert_eq!(list::value_shown(ui,list_tile),Some(4.0),"the actual native tree paints the same value");
        let marker=list::child(ui,list::child(ui,list_tile,list::SCROLLBAR).unwrap(),list::MARKER).unwrap();
        assert_eq!(ui.tile(marker).get("y").unwrap().num(),80.0,"the actual marker's model position follows the scroll");
        assert_eq!(app.world().resource::<ActiveMenu>().0.as_ref().unwrap().selected[Side::Player as usize],0);
        let seen=app.world().resource::<NativeGuardSeen>();assert_eq!((seen.selects,seen.used,seen.ones),(0,0,0));
        assert!(app.world().resource::<Hotkeys>().0.iter().all(Option::is_none));
        assert_eq!(app.world_mut().resource_mut::<crate::sim::SimState>().world.item_count(FormId::PLAYER_REF,FormId(0xF201)).unwrap(),3);
    }

    #[test]
    fn actual_native_scrollbar_page_tap_and_hardware_mouse_keep_same_authoritative_view() {
        use bevy::input::touch::TouchPhase;
        use bevy::prelude::*;
        let (mut app, point) = native_scrollbar_fixture();
        let down = point+Vec2::new(0.0,100.0);
        native_guard_event(&mut app,191,TouchPhase::Started,down);
        native_guard_event(&mut app,191,TouchPhase::Ended,down);
        for _ in 0..5 {app.update();}
        assert_eq!(app.world().resource::<InventoryTiles>().view.value(Side::Player),2.0,"a guarded actual page-region tap pages the list");
        assert_eq!(app.world().resource::<ActiveMenu>().0.as_ref().unwrap().selected[Side::Player as usize],0);
        app.world_mut().resource_mut::<PipBoyScreen>().pointer=Some(down);
        // InputPlugin owns edge reset; its real input event is the sole press source.
        app.world_mut().write_message(bevy::input::mouse::MouseButtonInput {button:MouseButton::Left,state:bevy::input::ButtonState::Pressed,window:Entity::PLACEHOLDER});
        app.update();
        assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left),"scrollbar never clears physical mouse ownership");
        assert_eq!(app.world().resource::<InventoryTiles>().view.value(Side::Player),4.0,"the physical mouse pages the same native view once");
    }

    #[test]
    fn actual_native_touch_marker_releases_before_hardware_mouse_takes_a_new_action() {
        use bevy::{input::touch::TouchPhase,prelude::*};
        let (mut app,point)=native_scrollbar_fixture();
        native_guard_event(&mut app,192,TouchPhase::Started,point);app.update();app.update();
        native_guard_event(&mut app,192,TouchPhase::Moved,point+Vec2::new(0.0,60.0));app.update();app.update();
        assert_eq!(app.world().resource::<InventoryTiles>().view.value(Side::Player),3.0);
        assert!(app.world().resource::<InventoryTiles>().view.drag.is_some());
        let mouse=point+Vec2::new(0.0,100.0);
        app.world_mut().resource_mut::<PipBoyScreen>().pointer=Some(mouse);
        app.world_mut().write_message(bevy::input::mouse::MouseButtonInput{button:MouseButton::Left,state:bevy::input::ButtonState::Pressed,window:Entity::PLACEHOLDER});
        app.update();
        assert!(app.world().resource::<InventoryTiles>().view.drag.is_none(),"canceled touch drag cannot inherit the physical left hold");
        assert_eq!(app.world().resource::<InventoryTiles>().view.value(Side::Player),5.0,"hardware page action still executes exactly once");
        app.world_mut().resource_mut::<PipBoyScreen>().pointer=Some(point);app.update();
        assert_eq!(app.world().resource::<InventoryTiles>().view.value(Side::Player),5.0,"subsequent physical motion cannot resume the old finger's marker drag");
        assert!(app.world().resource::<ButtonInput<MouseButton>>().pressed(MouseButton::Left));
    }

    #[test]
    fn actual_native_marker_sparse_motion_keeps_ownership_until_projected_release_without_replaying_actions() {
        use bevy::input::touch::TouchPhase;
        let (mut app,point)=native_scrollbar_fixture();
        native_guard_event(&mut app,193,TouchPhase::Started,point);
        app.update();app.update();
        assert!(app.world().resource::<InventoryTiles>().view.drag.is_some(),"the actual native marker press must establish ownership");
        for _ in 0..8 {
            app.update();
            let tiles=app.world().resource::<InventoryTiles>();
            assert!(tiles.view.drag.is_some(),"an idle OS frame is not a release");
            assert_eq!(tiles.view.value(Side::Player),0.0,"idle hold cannot page or replay its press");
            let frame=app.world().resource::<crate::pipboy::touch_items::TouchItems>().pointer_frame();
            assert!(frame.held && frame.control && frame.native);
            assert!(!frame.started && !frame.tap && !frame.scrolling && !frame.press(),"idle continuation has no action edge");
        }
        native_guard_event(&mut app,193,TouchPhase::Moved,point+Vec2::new(0.0,60.0));
        app.update();app.update();
        assert_eq!(app.world().resource::<InventoryTiles>().view.value(Side::Player),3.0);
        for _ in 0..8 {
            app.update();
            let tiles=app.world().resource::<InventoryTiles>();
            assert!(tiles.view.drag.is_some());
            assert_eq!(tiles.view.value(Side::Player),3.0,"no new motion means no new scroll");
        }
        native_guard_event(&mut app,193,TouchPhase::Ended,point+Vec2::new(0.0,60.0));
        app.update();app.update();
        assert!(app.world().resource::<InventoryTiles>().view.drag.is_none(),"the projected Ended retires the owned hold");
        assert!(!app.world().resource::<crate::pipboy::touch_items::TouchItems>().pointer_frame().held);
        for _ in 0..4 {app.update();}
        assert_eq!(app.world().resource::<InventoryTiles>().view.value(Side::Player),3.0);
        assert_eq!(app.world().resource::<ActiveMenu>().0.as_ref().unwrap().selected[Side::Player as usize],0);
        let seen=app.world().resource::<NativeGuardSeen>();
        assert_eq!((seen.selects,seen.used,seen.ones),(0,0,0),"control continuation never repeats a gameplay action");
        assert!(app.world().resource::<Hotkeys>().0.iter().all(Option::is_none));
        assert_eq!(app.world_mut().resource_mut::<crate::sim::SimState>().world.item_count(FormId::PLAYER_REF,FormId(0xF201)).unwrap(),3);
    }

    #[test]
    fn native_inventory_action_button_keeps_its_painted_selected_row_until_release(){
        use bevy::prelude::*;use bevy::input::touch::TouchPhase;
        use crate::pipboy::touch_items::TouchItems;
        #[derive(Resource)]struct ChangeSelection(bool);
        fn enable_button(ui:&mut Ui)->TileId {
            // Script-authored visibility is a supported native tile path. The stock
            // writer hides Equip/Drop, but a visible native target must still be guarded.
            let tile=ui.find_tile(&button_path("IM_EquipButton")).unwrap();
            ui.set_trait(tile,"visible",Value::Num(1.));ui.set_trait(tile,"target",Value::Num(1.));ui.update();tile
        }
        fn change(mut flag:ResMut<ChangeSelection>,touch:Res<TouchItems>,mut active:ResMut<ActiveMenu>,mut script:ResMut<ScriptState>,mut tiles:ResMut<InventoryTiles>){
            if flag.0 && touch.pointer_frame().tap {
                flag.0=false;let model=active.0.as_mut().unwrap();model.selected[Side::Player as usize]=1;
                redraw_mobile_inventory(model,&mut script,&mut tiles);enable_button(&mut script.rt.as_mut().unwrap().game.ui_tree);
            }
        }
        let NativeInventoryFixture{mut app,..}=native_inventory_fixture();
        let (button,point)={
            let mut script=app.world_mut().resource_mut::<ScriptState>();let ui=&mut script.rt.as_mut().unwrap().game.ui_tree;
            let button=enable_button(ui);let menu=ui.menu(INVENTORY_MENU).unwrap();
            let hits=fnv_ui::input::hit_candidates(ui,menu,&fnv_ui::input::no_measure);let r=hits.iter().find(|h|h.under==button).unwrap().region();
            (button,Vec2::new((r.left+r.width*0.5)as f32,(r.top+r.height*0.5)as f32))
        };
        app.insert_resource(ChangeSelection(true)).add_systems(Update,change.before(feed));
        native_guard_event(&mut app,107,TouchPhase::Started,point);app.update();app.update();
        native_guard_event(&mut app,107,TouchPhase::Ended,point);for _ in 0..4{app.update();}
        assert!(!app.world().resource::<ChangeSelection>().0);
        assert_eq!(app.world().resource::<ScriptState>().rt.as_ref().unwrap().game.ui_tree.find_tile(&button_path("IM_EquipButton")),Some(button),"the same button tile survives the actual selected-row repaint");
        assert_eq!((app.world().resource::<NativeGuardSeen>().ones,app.world().resource::<NativeGuardSeen>().used),(0,0));
        for id in [0xF201,0xF202] {assert_eq!(app.world_mut().resource_mut::<crate::sim::SimState>().world.item_count(FormId::PLAYER_REF,FormId(id)).unwrap(),3);}
        native_guard_event(&mut app,108,TouchPhase::Started,point);native_guard_event(&mut app,108,TouchPhase::Ended,point);for _ in 0..4{app.update();}
        assert_eq!((app.world().resource::<NativeGuardSeen>().ones,app.world().resource::<NativeGuardSeen>().used),(1,1));
        assert_eq!(app.world_mut().resource_mut::<crate::sim::SimState>().world.item_count(FormId::PLAYER_REF,FormId(0xF202)).unwrap(),2);
        assert_eq!(app.world_mut().resource_mut::<crate::sim::SimState>().world.item_count(FormId::PLAYER_REF,FormId(0xF201)).unwrap(),3);
    }

    include!("inventory_menu/touch_binding_tests.rs");

    #[test]
    fn a_clipped_inventory_row_with_vertical_text_offset_stays_clipped_by_the_list_window() {
        let with_offset = |x: String| {
            x.replace(
                r#"<hotrect name="IM_InventoryListTemplateRect"><target>&true;</target>"#,
                r#"<hotrect name="IM_InventoryListTemplateRect"><target>&true;</target><clips>&true;</clips>"#,
            )
            .replace(
                r#"<text name="ListItemText"></text>"#,
                r#"<text name="ListItemText"><y>16</y></text>"#,
            )
        };
        let (mut ui, mut t) = (tree_changed(with_offset), InventoryTiles::default());
        run(&mut ui, Some(shown(4, 0)), &mut t).expect("clipped inventory row with vertical text offset should stay inside list clipwindow");
        let row0_text = list::child(&ui, rows(&t)[0].tile, ROW_TEXT).unwrap();
        let piece = t.pieces.iter().find(|p| p.tile == row0_text).expect("row 0 text piece");
        let clip = piece.clip.expect("list clipwindow clip");
        assert_eq!(clip.height, 200.0, "clip must be the 200-unit IM_InventoryList window, not the row box");
        // Narrowing the list window itself still rejects the page.
        let list_id = ui.find_tile(LIST).unwrap();
        ui.set_trait(list_id, "width", Value::Num(15.0));
        ui.update();
        assert_eq!(run(&mut ui, Some(shown(4, 0)), &mut t), Err("no row shows whole".to_string()));
    }

    #[test]
    fn nested_icon_mod_button_and_rect_marker_stay_on_tiles_while_vanilla_path_wins() {
        // Synthetic restructured layout: IM_ItemIcon wrapped in a child rect under IM_MainRect,
        // IM_ModButton wrapped in a child rect under IM_ButtonRect, and IM_Template_ItemMarker
        // authored as a composite `<rect>` container in the row template.
        let restructured = |x: String| {
            x.replace(
                r#"<image name="IM_ItemIcon"/>"#,
                r#"<rect name="IconWrapper"><x>10</x><image name="IM_ItemIcon"/></rect>"#,
            )
            .replace(
                r#"<image name="IM_ModButton"><target>&true;</target><y>90</y><width>70</width><height>20</height><text name="button_text"><string>Go</string></text><text name="PCShortcutLabel"></text></image>"#,
                r#"<rect name="ModButtonWrapper"><image name="IM_ModButton"><target>&true;</target><y>90</y><width>70</width><height>20</height><text name="button_text"><string>Go</string></text><text name="PCShortcutLabel"></text></image></rect>"#,
            )
            .replace(
                r#"<image name="IM_Template_ItemMarker"/>"#,
                r#"<rect name="IM_Template_ItemMarker"><image name="MarkerFill"/></rect>"#,
            )
        };
        let (mut ui, mut t) = (tree_changed(restructured), InventoryTiles::default());
        assert_eq!(built(&ui, None), Tiled::Yes);
        let mut s = shown(3, 0);
        s.modify = true;
        run(&mut ui, Some(s), &mut t).expect("restructured InventoryMenu should stay on tiles");
        let icon = resolve_tile(&ui, ICON).expect("nested IM_ItemIcon");
        assert_eq!(ui.path_of(icon), "InventoryMenu/GLOW_BRANCH/IM_MainRect/IconWrapper/IM_ItemIcon");
        assert_eq!(ui.tile(icon).get("filename").map(Value::text).as_deref(), Some("pics\\item.dds"));
        assert_eq!(ui.tile(icon).get("visible").map(Value::num), Some(1.0));
        assert_eq!(ui.tile(icon).get("x").map(Value::num), Some(ICON_PLACE.0));
        let mod_btn = resolve_tile(&ui, &button_path("IM_ModButton")).expect("nested IM_ModButton");
        assert_eq!(ui.path_of(mod_btn), "InventoryMenu/GLOW_BRANCH/IM_MainRect/IM_ButtonRect/ModButtonWrapper/IM_ModButton");
        assert_eq!(ui.tile(mod_btn).get("visible").map(Value::num), Some(1.0));
        assert_eq!(ui.tile(mod_btn).get("target").map(Value::num), Some(1.0));
        assert_eq!(ui.tile(mod_btn).get("_PCButtonText").map(Value::text).as_deref(), Some("[X]"));
        assert_eq!(items_tile(&ui, &t.view, &t.written.tabs, mod_btn), Some(ItemsTile::Button(Button::Mod)));
        let mark = list::child(&ui, rows(&t)[0].tile, ROW_MARK).expect("rect row marker");
        assert_eq!(ui.tile(mark).get("visible").map(Value::num), Some(1.0));

        // Exact vanilla direct-child path takes precedence when both direct and nested exist.
        let both = |x: String| {
            x.replace(
                r#"<image name="IM_ItemIcon"/>"#,
                r#"<rect name="IconWrapper"><image name="IM_ItemIcon"/></rect><image name="IM_ItemIcon"/>"#,
            )
        };
        let (mut ui, mut t) = (tree_changed(both), InventoryTiles::default());
        run(&mut ui, Some(shown(2, 0)), &mut t).unwrap();
        let exact = ui.find_tile(ICON).unwrap();
        assert_eq!(resolve_tile(&ui, ICON), Some(exact));
        assert_eq!(ui.tile(exact).get("filename").map(Value::text).as_deref(), Some("pics\\item.dds"));

        // Missing IM_ItemIcon everywhere (even with wrapper containers present) still falls back cleanly.
        let missing = |x: String| {
            x.replace(
                r#"<image name="IM_ItemIcon"/>"#,
                r#"<rect name="IconWrapper"><image name="OtherIcon"/></rect>"#,
            )
        };
        let mut ui = tree_changed(missing);
        assert_eq!(built(&ui, None), Tiled::No(format!("{ICON} missing")));
        assert_eq!(run(&mut ui, Some(shown(2, 0)), &mut InventoryTiles::default()), Err(format!("{ICON} missing")));
    }

    #[test]
    fn tiled_drop_button_honors_qtt_select_short_keys() {
        // Off by default (vanilla unchanged): Drop button always emits Command::Drop.
        assert_eq!(tiled_drop_command(false, false, false), Command::Drop);
        assert_eq!(tiled_drop_command(false, true, false), Command::Drop);
        assert_eq!(tiled_drop_command(false, false, true), Command::Drop);
        assert_eq!(tiled_drop_command(false, true, true), Command::Drop);

        // Enabled (#3799): Shift drops stack, Ctrl drops single, Shift+Ctrl prefers Shift.
        assert_eq!(tiled_drop_command(true, false, false), Command::Drop);
        assert_eq!(tiled_drop_command(true, true, false), Command::DropStack);
        assert_eq!(tiled_drop_command(true, false, true), Command::DropSingle);
        assert_eq!(tiled_drop_command(true, true, true), Command::DropStack);
    }

    #[test]
    fn script_click_on_cancel_button_exits_folder_and_is_noop_outside_folder() {
        use crate::menu::model::Folder;
        use fnv_script::vm::XVal;

        let run_click = |folder: Option<Folder>, button_name: &str| -> Vec<Command> {
            let mut app = App::new();
            let mut state = ScriptState::over_no_plugins();
            let game = &mut state.rt.as_mut().unwrap().game;
            game.ui_files = Some(Box::new(MemSource::default()));
            game.ui_tree = tree_changed(|xml| {
                xml.replace(
                    r#"<image name="IM_CancelButton">"#,
                    r#"<image name="IM_CancelButton"><id>10</id>"#,
                )
                .replace(
                    r#"<image name="IM_DropButton">"#,
                    r#"<image name="IM_DropButton"><id>11</id>"#,
                )
            });
            let mut s = shown(2, 0);
            s.keyring = folder.is_some();
            s.repair.shown = folder.is_none();
            let mut t = InventoryTiles::default();
            let menu = run(&mut game.ui_tree, Some(s), &mut t).unwrap();
            t.ready = Some(menu);
            t.shown = Some(menu);

            let cancel_vis = at(&game.ui_tree, &button_path("IM_CancelButton"), "visible");
            assert_eq!(cancel_vis, if folder.is_some() { 1.0 } else { 0.0 });

            let click_path = format!("InventoryMenu\\GLOW_BRANCH\\IM_MainRect\\IM_ButtonRect\\{button_name}");
            let clicked = fnv_game::ui_functions::call(game, "ClickMenuButton", &[XVal::Str(click_path)])
                .expect("ClickMenuButton handled")
                .expect("ClickMenuButton succeeded");
            assert_eq!(clicked, XVal::Num(1.0));

            let mut model = MenuModel::empty_inventory();
            model.folder = folder;
            let mut pip = PipBoy::default();
            pip.page = Page::Items;
            pip.phase = crate::pipboy::Phase::Up;

            app.insert_resource(state)
                .insert_resource(TileMenus { inventory: true, ..Default::default() })
                .insert_resource(pip)
                .insert_resource(PipBoyScreen { image: Handle::default(), camera: Entity::PLACEHOLDER, on_model: true, pointer: None, scale: 1.0 })
                .insert_resource(PipBoyAid::default())
                .insert_resource(ActiveMenu(Some(model)))
                .insert_resource(t)
                .init_resource::<HotkeyHold>()
                .init_resource::<Hotkeys>()
                .init_resource::<crate::pipboy::KeysTaken>()
                .init_resource::<ButtonInput<MouseButton>>()
                .init_resource::<ButtonInput<KeyCode>>()
                .add_message::<PipBoyInput>()
                .add_message::<TabHovered>()
                .add_message::<MenuInput>()
                .add_message::<RepairInput>()
                .add_message::<ModInput>()
                .add_systems(Update, feed);

            app.update();
            let mut reader = app.world_mut().resource_mut::<Messages<MenuInput>>().get_cursor();
            let messages = app.world().resource::<Messages<MenuInput>>();
            reader.read(messages).map(|m| m.0).collect()
        };

        // Outside a folder (IM_CancelButton hidden): script ClickMenuButton is a no-op and must NOT close the Pip-Boy.
        assert_eq!(run_click(None, "IM_CancelButton"), Vec::<Command>::new());
        // Inside a folder (IM_CancelButton shown): script ClickMenuButton emits Command::Back.
        assert_eq!(run_click(Some(Folder::Keys), "IM_CancelButton"), vec![Command::Back]);
        // Control: hidden buttons that still have an active top-level command (e.g. Drop) keep working via ClickMenuButton.
        assert_eq!(run_click(None, "IM_DropButton"), vec![Command::Drop]);
    }

    #[test]
    fn ysi_folder_keeps_repair_and_hides_cancel_mutually_exclusively() {
        use crate::menu::model::Folder;

        let check = |folder: Option<Folder>, tab: usize| {
            let mut m = MenuModel::empty_inventory();
            m.folder = folder;
            let in_vanilla_misc = matches!(folder, Some(Folder::Keys | Folder::Cards));
            let repair = RepairButton { shown: !in_vanilla_misc, usable: true, word: None };
            let s = Shown::of(&m, None, tab, false, Default::default(), repair, (Hints::default(), String::new()), None);
            // IM_RepairButton and IM_CancelButton occupy the same slot and must be mutually exclusive.
            assert_ne!(s.repair.shown, s.keyring);
            let mut ui = tree();
            let mut t = InventoryTiles::default();
            run(&mut ui, Some(s), &mut t).unwrap();
            let repair_vis = at(&ui, &button_path("IM_RepairButton"), "visible");
            let cancel_vis = at(&ui, &button_path("IM_CancelButton"), "visible");
            let keyring_open = at(&ui, INVENTORY_MENU, "_KeyringOpen");
            (repair_vis, cancel_vis, keyring_open)
        };

        // Inside a ySI category folder on Weapons tab: Repair shown, Cancel/_KeyringOpen hidden (no overlap).
        assert_eq!(check(Some(Folder::Ysi(0)), 0), (1.0, 0.0, 0.0));
        // Inside vanilla Misc Keyring / Caravan Cards: Cancel/_KeyringOpen shown, Repair hidden.
        assert_eq!(check(Some(Folder::Keys), 3), (0.0, 1.0, 1.0));
        assert_eq!(check(Some(Folder::Cards), 3), (0.0, 1.0, 1.0));
        // Tab root: Repair shown, Cancel/_KeyringOpen hidden.
        assert_eq!(check(None, 0), (1.0, 0.0, 0.0));
    }
}
