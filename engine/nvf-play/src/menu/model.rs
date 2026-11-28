//! The menus as plain data over `fnv_sim::World`, without Bevy: rows, selection,
//! scrolling, and the commands that move items, all unit-tested on a synthetic plugin.
//!
//! Rows are one per base item (a stack), except that weapons and armour get one row per
//! condition: units stack only with units at the same condition [owner, 2026-10-16: six
//! Varmint Rifles, one at 95 %, list as a row of five at full condition and below it a row
//! of one]. Owner and the equipped mark do not split a row. A row moves, drops and
//! equips only its own units (`Row::at`); within them the simulation decides which go
//! (its removal order). Rows are sorted by display name in byte order, capitals before
//! small letters, FormID breaking ties [observed: the owner's barter screenshot lists
//! "LAER+" before "Lad's Life", and "$5 NCR" and ".223 Round" before letters;
//! docs/spec-notes/barter-menu.md §3], then the best condition first [owner, as above].
//!
//! The container and barter menus list one category at a time, chosen with the arrows
//! beside each list's title: all items, weapons, apparel, aid, misc, ammunition
//! [observed: "ITEMS" and "MISC" titles on the owner's screenshots; the other names and
//! their order are hypotheses, after the Pip-Boy's item pages]. The mouse wheel over a
//! list's heading steps its category as the arrows do (`Command::wheel`) [design: owner
//! request 2026-10-31, #1192].
//!
//! Non-playable items (`Item::non_playable`) are not listed in inventories other than
//! the player's, and non-playable armour (the Pip-Boy Glove) not in the player's either,
//! so they cannot be moved, equipped or dropped from a menu; the player's own
//! non-playable weapons and ammo are listed [documented: GECK wiki "Unplayable Items";
//! observed by the owner: the game never shows the glove in the inventory, and Boone's
//! rifle and "Magical Companion Ammo" do not show in his]. Take All leaves them in the
//! container [hypothesis: hidden items stay where they are].
//!
//! Items whose record has no name (no FULL, or a blank one) are not listed in any
//! inventory, the player's included ([`shown`]), so a row never reads as an Editor ID
//! (#1603: the vanilla book leveled list SublistBooks100 holds the nameless
//! DONOTUSEBookGeneric07 0x000199BB, which household and library clutter lists hand out)
//! [hypothesis: mods use nameless items as hidden tokens because the original's menus
//! leave them out; observer check in docs/handoff/observer-checks.md].
//!
//! Quest items (`World::is_quest_item`: the record's "Quest Item" header flag, or a
//! script's SetQuestObject) are listed and marked; dropping one is refused by the
//! simulation and the status line says so [documented: GECK wiki Category:Items, the
//! player cannot drop one]. Storing one in a container is allowed [hypothesis: the page
//! does not mention containers]. They cannot be sold [documented: the same page], so the
//! barter menu does not list them [hypothesis: hidden rather than refused]. Keys
//! (`ItemKind::Key`, `KEYM`) are also excluded from both sides of the barter menu (#3432)
//! [hypothesis: issue #3432 reports that keys do not appear in barter in the original
//! engine], while remaining listed in the inventory (`Keyring` folder) and in containers.
//!
//! Barter ([`Mode::Barter`], [`Trade`]): items are put up from either side and listed on
//! the other side, first, marked, until the trade is accepted [observed: the owner's
//! screenshot shows the sold Gold Bar at the top of the merchant's list with an open
//! square, gone from the player's]; caps are not listed but shown in each list's heading
//! [observed: no Bottle Cap row in the barter screenshots, while the container
//! screenshot lists one]. Accept moves the items and the caps the prices add up to; it
//! is refused when the side that pays has too few caps [hypothesis: the wording is ours].
//!
//! "How many?" ([`Quantity`]): taking or storing one unit of a stack of more than
//! [`ASK_QUANTITY_ABOVE`] units, putting one up for barter (or taking it back), and
//! dropping one from the inventory ask how many first [owner, 2026-10-15: "buy, transfer,
//! drop stacks of items when more than 5"; observed: the owner's screenshots of the
//! original's prompt over the Pip-Boy, a stack of six Varmint Rifles]. The prompt starts
//! at the whole stack [observed: the first screenshot shows 6 of 6, the later ones 4 and
//! 3], goes from 1 to the stack [hypothesis: 0 would move nothing], and Ok moves that
//! many; Cancel (or closing) moves nothing. While it is open the menu takes no other
//! command. Smaller stacks still move one unit per press, and the Stack commands move the
//! whole stack at once without asking [design: NVForever's shortcuts].

use super::barter::{self, Prices};
use fnv_sim::forms::{BaseData, FormStore, Item, ItemKind};
use fnv_sim::refs::PlacedRef;
use fnv_sim::world::{Encumbrance, SimError};
use fnv_sim::{FormId, Pick, RefTable, World};
use std::collections::BTreeMap;

mod search;
pub use search::{SearchError, MAX_SEARCH_CHARS};

/// Rows the inventory (the Pip-Boy's ITEMS list) shows at once; the list scrolls to keep
/// the selection visible. Its list is 468 high and a row 51 (`pipboy::items`).
pub const VISIBLE_ROWS: usize = 9;
/// Rows the container and barter lists show at once: the list is 365 high and a row 51
/// (docs/spec-notes/barter-menu.md §2).
pub const LIST_ROWS: usize = 7;

/// A stack of more than this many units asks how many to move ([`Quantity`]) [owner,
/// 2026-10-15; the original's own threshold is a game setting not in FalloutNV.esm and
/// not read].
pub const ASK_QUANTITY_ABOVE: i32 = 5;

/// What the quantity prompt's Page Up / Page Down move it by [design: the interface
/// file's "jump size" is set by the original's code, not read].
pub const QUANTITY_JUMP: i32 = 10;

/// The caps item, Bottle Cap [documented: FalloutNV.esm MISC 0x0000000F, the item the
/// vendor caps lists hold].
pub const CAPS: FormId = FormId(0x0000000F);

/// One stack in an inventory.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub item: FormId,
    /// The units the row stands for (`Extra::is_at`): weapons' and armour's at this
    /// condition, every unit (`None`) of other items.
    pub at: Option<f32>,
    pub name: String,
    pub count: i32,
    /// Weight of one unit as authored (`None`: not decoded). Ammunition weighs nothing in
    /// the totals outside Hardcore mode (the simulation's rule), but its row shows the
    /// authored weight.
    pub weight: Option<f32>,
    /// Value of one unit.
    pub value: Option<i32>,
    pub kind: Option<ItemKind>,
    /// At least one unit is equipped.
    pub equipped: bool,
    /// A quest item: the player cannot drop it (`World::is_quest_item`).
    pub quest: bool,
    /// The condition of the row's units, 0 to 1 (weapons and armour).
    pub condition: Option<f32>,
    /// The item's picture (ICON, a path under `textures\`).
    pub icon: Option<String>,
    /// Barter: what one unit fetches in this trade, in caps (before rounding).
    pub price: Option<f32>,
    /// Barter: units the other side puts up, listed here until the trade is accepted.
    pub bartered: bool,
    /// The ammo the player's weapon loads: marked as equipped is (it is not equipped in
    /// the inventory's sense).
    pub loaded: bool,
    /// Ammo the player's weapon cannot fire, in the Pip-Boy: its row is drawn at half
    /// brightness [observed: observer check 288, #3442].
    pub dim: bool,
    /// A weapon's attached mods (`Extra::mods`): its units are a row of their own, named
    /// with " +" (docs/spec-notes/weapon-mods.md §3).
    pub mods: u8,
    /// What the attached mods add to one unit's value: their IMOD values, which its
    /// condition does not lower (`FormStore::mods_value`, issue #982).
    pub mods_value: i32,
    /// The Misc tab's folder rows (the keyring, the caravan cards, and "Back" inside
    /// one): no item of their own ([`Folder`]).
    pub folder: Option<FolderRow>,
    /// A corpse's weapon lying beside it (`World::dropped_beside`): the created
    /// reference this row stands for. Taking the row picks that reference up, so the
    /// weapon on the ground and the row are one item (#1032).
    pub reference: Option<FormId>,
    /// A live explosive planted in this pocket (`Extra::live`): a row of its own, named
    /// with [`LIVE_NOTE`], that no command takes, moves or puts up (#1640).
    pub live: bool,
    /// Resolved ySI sorting tag, icon texture path, and XML template when ySI is active
    /// (`None` in vanilla sessions without ySI) [design: `docs/spec-notes/ysi-filename.md`].
    pub ysi: Option<fnv_game::ui_ysi::YsiItemDisplay>,
}

/// What a live explosive's row says after its name (`fnv_game::pickpocket`, a grenade or
/// mine planted in a pocket and not yet gone off). FalloutNV.esm has no game setting for
/// it [observed: none of its GMSTs names a live or armed item], so the word is NVForever's
/// own [design, #1640; owner-review.md].
pub const LIVE_NOTE: &str = "Armed";

/// A live explosive's row name: its item's name, then [`LIVE_NOTE`] in brackets.
pub fn live_name(name: String) -> String {
    format!("{name} ({LIVE_NOTE})")
}

/// The status line when a command is refused on a live explosive's row (`name`, the
/// row's name) [design, #1640: the words are NVForever's].
pub fn live_refusal(name: &str) -> String {
    format!("Cannot take or move {name}")
}

/// A folder of the inventory's list: vanilla Misc's keys and Caravan cards, or a clickable
/// ySI Pip-Boy category/subfolder ([`Folder::Ysi`], indexing `MenuModel::ysi_categories`)
/// [owner's playtest list 7 item 6: "a keyring item that all keys are added to ... the same
/// for caravan cards"; design: the rows' names and places and ySI category folding are
/// NVForever's].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Folder {
    /// KEYM items.
    Keys,
    /// The player's Caravan cards: those AddCardToPlayer gave (`fnv_game::caravan`, kept
    /// outside the inventory, [`MenuModel::cards`]) and any CCRD item carried.
    Cards,
    /// A clickable ySI category or nested subfolder (`index` into
    /// `YsiCategoryIndex::categories`) [design: `docs/spec-notes/yui-ini.md`].
    Ysi(u32),
}

impl Folder {
    /// The folder row's default name.
    pub fn name(self) -> &'static str {
        match self {
            Folder::Keys => "Keyring",
            Folder::Cards => "Caravan Cards",
            Folder::Ysi(_) => "Category",
        }
    }

    fn holds(self, kind: Option<ItemKind>) -> bool {
        matches!((self, kind), (Folder::Keys, Some(ItemKind::Key)) | (Folder::Cards, Some(ItemKind::CaravanCard)))
    }
}

/// What a folder row does when used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderRow {
    /// Opens the folder.
    Open(Folder),
    /// Back to the parent folder or tab root (the first row inside a folder).
    Back,
}

/// The name of the row that leaves a folder.
pub const BACK: &str = "Back";

impl Row {
    /// A folder row: `count` items inside (0 for "Back").
    fn of_folder(f: FolderRow, count: i32) -> Row {
        let name = match f {
            FolderRow::Open(f) => f.name().to_string(),
            FolderRow::Back => BACK.to_string(),
        };
        Row { item: FormId(0), at: None, name, count, weight: None, value: None, kind: None, equipped: false, quest: false, condition: None, icon: None, price: None, bartered: false, loaded: false, dim: false, mods: 0, mods_value: 0, folder: Some(f), reference: None, live: false, ysi: None }
    }

    /// A clickable ySI category or subfolder row carrying its resolved display name, member
    /// row count, large preview picture (`icon`), sorting tag, and row `ySIImage` icon/template
    /// [design: `docs/spec-notes/yui-ini.md`].
    fn of_ysi_category(id: u32, cat: &fnv_game::yui_ini::YsiCategoryDef, count: i32) -> Row {
        Row {
            item: FormId(0),
            at: None,
            name: cat.name.clone(),
            count,
            weight: None,
            value: None,
            kind: None,
            equipped: false,
            quest: false,
            condition: None,
            icon: cat.preview_icon.clone(),
            price: None,
            bartered: false,
            loaded: false,
            dim: false,
            mods: 0,
            mods_value: 0,
            folder: Some(FolderRow::Open(Folder::Ysi(id))),
            reference: None,
            live: false,
            ysi: Some(fnv_game::ui_ysi::YsiItemDisplay {
                tag: Some(cat.tag.clone()),
                icon: cat.row_icon.clone(),
                template: cat.template.clone(),
            }),
        }
    }
}

/// An inventory's rows (sorted) and totals.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Listing {
    pub rows: Vec<Row>,
    /// Total weight as the simulation counts it (`Inventory::totals`), hidden
    /// (non-playable) items included: they are carried.
    pub weight: f64,
    /// Value of the listed stacks.
    pub value: i64,
    /// Stacks not listed because they are non-playable (`Item::listed`).
    pub hidden: usize,
}

/// What a row says of a worn weapon or piece of armour after its name: the condition
/// of its units, nothing at full condition (docs/spec-notes/combat-rules.md §17).
pub fn condition_note(kind: Option<ItemKind>, condition: Option<f32>) -> String {
    match (kind, condition) {
        (Some(ItemKind::Weapon | ItemKind::Armor), Some(c)) if c <= 0.0 => " (broken)".into(),
        (Some(ItemKind::Weapon | ItemKind::Armor), Some(c)) if c < 1.0 => format!(" (CND {}%)", fnv_combat::condition::percent(c)),
        _ => String::new(),
    }
}

/// A row's name for units with the mod flags `mods`: " +" after it when any mod is on
/// [hypothesis: the GECK wiki's SetItemModFlags calls the flag "the inventory '+'";
/// weapon-mods.md §3].
pub fn modded_name(name: String, mods: u8) -> String {
    if mods != 0 {
        format!("{name} +")
    } else {
        name
    }
}

/// Whether a menu lists `item` in an inventory, the player's (`players`) or another
/// holder's: a playable item by `Item::listed`, and one with a name (module notes). A form
/// the store does not know is listed, by its FormID.
pub fn shown(forms: &FormStore, item: FormId, players: bool) -> bool {
    let Some(b) = forms.get(item) else { return true };
    b.name.as_deref().is_some_and(|n| !n.trim().is_empty()) && forms.item(item).is_none_or(|i| i.listed(players))
}

/// Display name of a form: its FULL name, else its Editor ID, else its FormID.
pub fn display_name(forms: &FormStore, id: FormId) -> String {
    match forms.get(id) {
        Some(b) => b.name.clone().filter(|n| !n.trim().is_empty()).or_else(|| b.edid.clone()).unwrap_or_else(|| id.to_string()),
        None => id.to_string(),
    }
}

/// A failed command in words for the player: items by name, not FormID. Starts with
/// "Cannot" (the menu draws such lines as warnings).
pub fn plain_error(forms: &FormStore, e: &SimError) -> String {
    let name = |id: &FormId| forms.get(*id).and_then(|b| b.name.clone().or_else(|| b.edid.clone())).unwrap_or_else(|| "this item".into());
    match e {
        SimError::NotEquippable(item) => format!("Cannot equip or use {}", name(item)),
        SimError::NotCarried { item, .. } => format!("Cannot find {} in the inventory", name(item)),
        SimError::NotAnItem(item) => format!("Cannot carry {}", name(item)),
        SimError::Deleted(item) => format!("Cannot reach {}: it is gone", name(item)),
        other => format!("Cannot: {other}"),
    }
}

/// What tells a row's units apart: the item, the condition's bits (a condition is never
/// negative, so the bits order as the numbers do) and the mod flags.
pub type Units = (FormId, Option<u32>, u8);

impl Row {
    /// One unit's value before a trade's multiplier: the item's value at its condition
    /// plus what its attached mods add, which the condition does not lower [observed:
    /// o26_value 2026-11-06, `FormStore::mods_value`; issue #982].
    pub fn worth(&self, prices: &Prices) -> Option<f32> {
        self.value.map(|v| prices.value_at(v, self.condition) + self.mods_value as f32)
    }

    pub fn units(&self) -> Units {
        (self.item, self.at.map(f32::to_bits), self.mods)
    }

    /// The units the row's actions take: its condition and its mods (weapon-mods.md §3).
    pub fn pick(&self) -> Pick {
        Pick::row(self.at, self.mods)
    }
}

/// The units [`Row::units`] names, for the simulation.
fn units_pick(&(_, at, mods): &Units) -> Pick {
    Pick::row(at.map(f32::from_bits), mods)
}

/// The most of `count` units weighing `weight` each that fit in `room` (at least one, so
/// the quantity menu still offers a unit).
pub fn carry_cap(count: i32, weight: f32, room: f32) -> i32 {
    if weight <= 0.0 {
        return count;
    }
    ((room / weight).floor().max(1.0) as i32).min(count)
}

/// The order of the player's inventory list under the Pip-Boy's Clicking the Tab Sorts
/// tweak: by name (as always), by value or by weight, the most first [design:
/// docs/design/tweaks.md; the readme names no orders].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortBy {
    #[default]
    Name,
    Value,
    Weight,
}

impl SortBy {
    /// The next order a click on the open tab gives.
    pub fn next(self) -> SortBy {
        match self {
            SortBy::Name => SortBy::Value,
            SortBy::Value => SortBy::Weight,
            SortBy::Weight => SortBy::Name,
        }
    }

    /// The corner line that says the order [design: our own words].
    pub fn line(self) -> &'static str {
        match self {
            SortBy::Name => "Sorted by name",
            SortBy::Value => "Sorted by value",
            SortBy::Weight => "Sorted by weight",
        }
    }

    /// Sort rows already in name order: stable, so equals keep it.
    pub fn apply(self, rows: &mut [Row]) {
        match self {
            SortBy::Name => {}
            SortBy::Value => rows.sort_by_key(|r| std::cmp::Reverse(r.value.unwrap_or(0))),
            SortBy::Weight => rows.sort_by(|a, b| b.weight.unwrap_or(0.0).total_cmp(&a.weight.unwrap_or(0.0))),
        }
    }
}

/// The order rows are listed in: put-up rows first, then (when `by_ysi_tag` is true and
/// any row carries a ySI sorting tag) by ySI tag byte order with untagged rows after
/// tagged ones [hypothesis: `docs/spec-notes/ysi-filename.md`], then by name in byte
/// order, then the best condition first. When `by_ysi_tag` is false or no row carries a
/// ySI tag the order is unchanged from vanilla.
fn sort_rows_by(rows: &mut [Row], by_ysi_tag: bool) {
    let has_ysi = by_ysi_tag
        && rows
            .iter()
            .any(|r| r.ysi.as_ref().and_then(|y| y.tag.as_deref()).is_some_and(|t| !t.is_empty()));
    if has_ysi {
        let key = |r: &Row| {
            let tag = r
                .ysi
                .as_ref()
                .and_then(|y| y.tag.as_deref())
                .filter(|t| !t.is_empty());
            (
                !r.bartered,
                tag.is_none(),
                tag.unwrap_or("").as_bytes().to_vec(),
                r.name.as_bytes().to_vec(),
                r.item,
                std::cmp::Reverse(r.at.map(f32::to_bits)),
                r.mods,
            )
        };
        rows.sort_by_cached_key(key);
    } else {
        let key = |r: &Row| (!r.bartered, r.name.as_bytes().to_vec(), r.item, std::cmp::Reverse(r.at.map(f32::to_bits)), r.mods);
        rows.sort_by_cached_key(key);
    }
}

/// Marks the ammo rows `firable` leaves out as dim (`Row::dim`); `None` dims nothing.
/// Only ammo rows: a weapon or a stimpak is never dim [observed: observer check 288].
fn mark_dim(rows: &mut [Row], firable: Option<&[FormId]>) {
    for r in rows.iter_mut() {
        r.dim = r.kind == Some(ItemKind::Ammo) && firable.is_some_and(|f| !f.contains(&r.item));
    }
}

fn sort_rows(rows: &mut [Row]) {
    sort_rows_by(rows, true);
}

/// The rows of `holder`'s inventory, sorted by name.
pub fn listing(world: &mut World<'_>, holder: FormId) -> Result<Listing, SimError> {
    let forms = world.forms;
    let hardcore = world.config().hardcore;
    // The holder's perks (Pack Rat, Heavyweight) in the total (perk-entry-points.md §7).
    let rules = world.weight_rules(holder);
    let s = world.get(holder)?;
    if s.deleted() {
        return Err(SimError::Deleted(holder));
    }
    let inv = s.inventory.as_ref().ok_or(SimError::NoInventory(holder))?;
    let totals = inv.totals_with(forms, hardcore, &rules);
    let mut rows: Vec<Row> = Vec::new();
    let mut hidden = 0;
    for (item, extra, count) in inv.groups() {
        let it = forms.item(item);
        if !shown(forms, item, holder == PLAYER) {
            hidden += 1;
            continue;
        }
        let kind = it.map(|i| i.kind);
        let at = matches!(kind, Some(ItemKind::Weapon | ItemKind::Armor)).then(|| extra.condition_or_whole());
        // Groups of one item come together (`Inventory::groups`); live units are a row of
        // their own.
        if let Some(r) = rows.iter_mut().rev().take_while(|r| r.item == item).find(|r| r.at == at && r.mods == extra.mods && r.live == extra.live) {
            r.count += count;
            r.equipped |= extra.equipped;
            continue;
        }
        let name = modded_name(display_name(forms, item), extra.mods);
        rows.push(Row {
            item,
            at,
            name: if extra.live { live_name(name) } else { name },
            count,
            weight: it.and_then(|i| i.weight),
            value: it.and_then(|i| i.worth()),
            kind,
            equipped: extra.equipped,
            quest: false, // below, once the inventory is no longer borrowed
            condition: at,
            icon: it.and_then(|i| i.icon.clone()),
            price: None,
            bartered: false, loaded: false, dim: false,
            mods: extra.mods,
            mods_value: forms.mods_value(item, extra.mods),
            folder: None,
            reference: None,
            live: extra.live,
            ysi: None,
        });
    }
    for r in &mut rows {
        r.quest = world.is_quest_item(r.item);
    }
    sort_rows(&mut rows);
    let value = rows.iter().map(|r| (r.value.unwrap_or(0) + r.mods_value) as i64 * r.count as i64).sum();
    Ok(Listing { rows, weight: totals.weight, value, hidden })
}

/// Add to a container column the weapons its holder dropped as it died and that still
/// lie in the world (`World::dropped_beside`): one row per reference, after the
/// inventory's rows and sorted with them. Their weight and value count in the column's
/// totals [design: the corpse's loot, as the owner's report asks (#1032)].
fn add_dropped_beside(world: &World<'_>, holder: FormId, l: &mut Listing) {
    let forms = world.forms;
    let mut added = false;
    for r in world.dropped_beside(holder) {
        let Some(s) = world.peek(r) else { continue };
        let it = forms.item(s.base);
        if !shown(forms, s.base, false) {
            continue;
        }
        let kind = it.map(|i| i.kind);
        let at = matches!(kind, Some(ItemKind::Weapon | ItemKind::Armor)).then(|| s.extra.condition_or_whole());
        let row = Row {
            item: s.base,
            at,
            name: modded_name(display_name(forms, s.base), s.extra.mods),
            count: s.count.max(1),
            weight: it.and_then(|i| i.weight),
            value: it.and_then(|i| i.worth()),
            kind,
            equipped: false,
            quest: world.is_quest_item(s.base),
            condition: at,
            icon: it.and_then(|i| i.icon.clone()),
            price: None,
            bartered: false, loaded: false, dim: false,
            mods: s.extra.mods,
            mods_value: forms.mods_value(s.base, s.extra.mods),
            folder: None,
            reference: Some(r),
            live: false,
            ysi: None,
        };
        l.weight += f64::from(row.weight.unwrap_or(0.0)) * f64::from(row.count);
        l.value += (row.value.unwrap_or(0) + row.mods_value) as i64 * row.count as i64;
        l.rows.push(row);
        added = true;
    }
    if added {
        sort_rows(&mut l.rows);
    }
}

/// A column of the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    /// The container, or the merchant (drawn on the right).
    Container = 0,
    /// The player (drawn on the left; the only column of the inventory menu).
    Player = 1,
}

impl Side {
    pub fn other(self) -> Side {
        match self {
            Side::Container => Side::Player,
            Side::Player => Side::Container,
        }
    }
}

/// The items a list shows, chosen with the arrows beside its title.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Category {
    #[default]
    All,
    Weapons,
    Apparel,
    Aid,
    Misc,
    Ammo,
}

impl Category {
    pub const ORDER: [Category; 6] = [Category::All, Category::Weapons, Category::Apparel, Category::Aid, Category::Misc, Category::Ammo];

    /// Whether an item of this kind is listed.
    pub fn holds(self, kind: Option<ItemKind>) -> bool {
        self == Category::All || self == Category::of(kind, false)
    }

    /// Whether this item is listed: as [`Category::holds`], a skill book with the aid
    /// items.
    pub fn holds_item(self, kind: Option<ItemKind>, item: Option<&Item>) -> bool {
        self == Category::All || self == Category::of(kind, item.is_some_and(|i| i.teaches.is_some()))
    }

    /// The tab an item is listed in. A skill book (a BOOK that teaches a skill) is with
    /// the aid items, other books with the misc ones [owner's playtest list 7 item 33:
    /// "skill books ... should be showing in the Aid tab"; hypothesis: books that teach
    /// nothing stay in Misc].
    pub fn of(kind: Option<ItemKind>, skill_book: bool) -> Category {
        match kind {
            Some(ItemKind::Weapon) => Category::Weapons,
            Some(ItemKind::Armor) => Category::Apparel,
            Some(ItemKind::Aid) => Category::Aid,
            Some(ItemKind::Book) if skill_book => Category::Aid,
            Some(ItemKind::Ammo) => Category::Ammo,
            _ => Category::Misc,
        }
    }

    /// The next category (`by` 1) or the one before (-1), round the ends.
    pub fn step(self, by: i32) -> Category {
        let n = Self::ORDER.len() as i32;
        let i = Self::ORDER.iter().position(|&c| c == self).unwrap_or(0) as i32;
        Self::ORDER[(i + by).rem_euclid(n) as usize]
    }

    /// The player's list title [observed: "ITEMS", "MISC"; the others hypotheses].
    pub fn title(self) -> &'static str {
        match self {
            Category::All => "ITEMS",
            Category::Weapons => "WEAPONS",
            Category::Apparel => "APPAREL",
            Category::Aid => "AID",
            Category::Misc => "MISC",
            Category::Ammo => "AMMO",
        }
    }
}

/// Which menu is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The container menu on this container reference: player | container.
    Container(FormId),
    /// The player's inventory alone.
    Inventory,
    /// The barter menu with this merchant (an actor reference): player | merchant.
    Barter(FormId),
}

/// What a row stands for: its units, the reference a corpse's dropped weapon is, and which of
/// rows alike it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowKey {
    pub units: Units,
    pub reference: Option<FormId>,
    pub nth: u32,
}

impl RowKey {
    /// The key of `rows[index]`.
    pub fn at(rows: &[Row], index: usize) -> Option<RowKey> {
        let row = rows.get(index)?;
        let (units, reference) = (row.units(), row.reference);
        let nth = match row.folder {
            Some(FolderRow::Open(Folder::Ysi(id))) => 0x8000_0000 | id,
            _ => rows[..index]
                .iter()
                .filter(|r| {
                    r.units() == units
                        && r.reference == reference
                        && !matches!(r.folder, Some(FolderRow::Open(Folder::Ysi(_))))
                })
                .count() as u32,
        };
        Some(RowKey { units, reference, nth })
    }
}

/// [design] A row as the input that clicked or came onto it saw it: the commands after
/// [`Command::Expect`] are for this row of this menu, and for no other row a command queued
/// before them leaves at its index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowGuard {
    pub mode: Mode,
    pub side: Side,
    pub index: usize,
    pub key: RowKey,
    /// The commands act on the row as the selected one.
    pub selected: bool,
    /// How many commands follow the guard.
    pub then: u8,
}

/// What the player asks the menu to do (keys, mouse and the test aid all go through
/// this).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// The next commands hold only if the row is still there ([`MenuModel::row_is`]);
    /// `crate::menu::apply_input` drops them otherwise.
    Expect(RowGuard),
    /// Internal one-shot touch envelope. Only menu::apply_input may resolve it
    /// against its exact live barter guard; the plain model treats it as a no-op.
    CommitBarter(u64),
    /// One-shot normal-presentation touch, resolved only by menu::apply_input.
    CommitNormalBarter(u64),
    Up,
    Down,
    PageUp,
    PageDown,
    /// Move the selection by this many rows (mouse wheel).
    Scroll(i32),
    /// Focus a column (container and barter menus).
    Focus(Side),
    /// Select a row (mouse). Selecting the row that is already selected does
    /// [`Command::One`].
    Select(Side, usize),
    /// Select a row without acting on it (the pointer came onto it).
    Hover(Side, usize),
    /// Show the next (1) or previous (-1) category of a column.
    Category(Side, i32),
    /// Show this category in a column (the Pip-Boy's ITEMS tabs).
    SetCategory(Side, Category),
    /// The order of the player's inventory list (the Clicking the Tab Sorts tweak).
    SetSort(SortBy),
    /// Container menu: move one unit of the selected stack to the other column (take
    /// from the container, store from the player). Barter: put one unit up, or take one
    /// back. Inventory: equip or unequip it (aid items are used).
    One,
    /// Container and barter menus: move one unit without opening the quantity prompt
    /// (`quantity-select-short-keys` Ctrl shortcut, #3799) [design].
    OneSingle,
    /// Container and barter menus: the same for the whole selected stack.
    Stack,
    /// Container menu: take everything from the container.
    TakeAll,
    /// Container menu: put everything listed on the player's side in the container, but
    /// what is worn or a quest's (the Store All tweak [design: docs/design/tweaks.md]).
    StoreAll,
    /// Barter: make the trade.
    Accept,
    /// Inventory: drop one unit of the selected stack into the world.
    Drop,
    /// Inventory: drop one unit without opening the quantity prompt
    /// (`quantity-select-short-keys` Ctrl shortcut, #3799) [design].
    DropSingle,
    /// Inventory: drop the whole selected stack.
    DropStack,
    /// Inventory: leave the open Misc folder ([`Folder`]).
    Back,
    /// The "How many?" prompt: set its amount (clamped to 1 and the stack).
    Amount(i32),
    /// The prompt: change its amount by this much.
    AmountStep(i32),
    /// The prompt's Ok: do what asked with the amount chosen.
    AmountOk,
    /// The prompt's Cancel.
    AmountCancel,
    Close,
}

impl Command {
    /// What whole wheel steps (`steps`, up positive) do: with the pointer over a list's
    /// heading (`title`) they step that list's category, down the next as the right arrow
    /// does; anywhere else they move the selection, up moving it up [design: owner
    /// request 2026-10-31, #1192].
    pub fn wheel(title: Option<Side>, steps: i32) -> Command {
        match title {
            Some(side) => Command::Category(side, -steps),
            None => Command::Scroll(-steps),
        }
    }
}

/// The "How many?" prompt open over a menu (`menus\quantity_menu.xml`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quantity {
    /// What asked: [`Command::One`] (take, store, put up, take back) or
    /// [`Command::Drop`]; done with [`Quantity::amount`] units on Ok.
    pub cmd: Command,
    /// The row it is for.
    pub side: Side,
    pub row: usize,
    /// The stack's size, the most it can move.
    pub max: i32,
    pub amount: i32,
}

impl Quantity {
    /// How full the prompt's meter is: the amount as a part of the stack.
    pub fn part(&self) -> f32 {
        self.amount as f32 / self.max.max(1) as f32
    }
}

/// The first row shown of a list of `rows`, `page` at once: `first` moved as little as
/// keeps `selected` in view. A row the pointer is over is already in view, so hovering
/// never scrolls a list kept this way; only the keys and the wheel do.
pub fn keep_in_view(first: usize, selected: usize, rows: usize, page: usize) -> usize {
    first.max((selected + 1).saturating_sub(page)).min(selected).min(rows.saturating_sub(page))
}

/// The amount at a point of the prompt's meter (`part` of its length from the left): that
/// part of the stack, rounded [documented: quantity_menu.xml, the drag's 0 to 1 times the
/// stack, rounded], at least 1.
pub fn amount_at(part: f32, max: i32) -> i32 {
    ((part.clamp(0.0, 1.0) * max as f32).round() as i32).clamp(1, max.max(1))
}

/// A trade being put together in the barter menu.
#[derive(Debug, Clone, PartialEq)]
pub struct Trade {
    pub merchant: FormId,
    /// What holds the merchant's goods and caps: its merchant container (XMRC), else the
    /// merchant itself [observed: Chet's container VendorContainerChet holds his stock
    /// and the vendor caps lists, his own inventory only pocket change; hypothesis: the
    /// merchant's own inventory is not offered].
    pub stock: FormId,
    /// The item that counts as money.
    pub caps: FormId,
    /// `ShowBarterMenu`'s percentage.
    pub discount: i32,
    /// The player's Barter skill.
    pub skill: f32,
    pub prices: Prices,
    /// Units put up, by the side they come from (`Side as usize`).
    pub offered: [BTreeMap<Units, i32>; 2],
    /// The caps each side holds (`Side as usize`).
    pub purse: [i32; 2],
    /// The caps the trade moves: positive when the player is paid.
    pub balance: i64,
    /// What the merchant sells (its services, docs/spec-notes/vendor-services.md §4):
    /// `None` sells everything (no script data to ask).
    pub sells: Option<Sells>,
    /// The merchant's items held back at the last refresh, with the category it does not
    /// sell them under, in row order.
    pub withheld: Vec<(FormId, &'static str)>,
    /// A script's price multipliers, buying and selling (SetBarterPriceMult,
    /// `fnv_game::nvse_jip12`), in place of the rules'.
    pub mult_set: [Option<f32>; 2],
    /// Items hidden by `HideItemBarterEx` for this merchant (`1` hidden on vendor's side,
    /// `2` hidden on player's side, `3` hidden on both) [documented: HideItemBarterEx].
    pub hidden: BTreeMap<FormId, i64>,
}

/// Whether the merchant sells an item: `None` when it does, else the name of the service
/// category it lacks (`fnv_game::services`). Vendors buy any item [documented: GECK wiki
/// Class, "vendors can buy any item"], so only the merchant's own rows are asked.
#[derive(Clone)]
pub struct Sells(pub std::sync::Arc<dyn Fn(FormId) -> Option<&'static str> + Send + Sync>);

impl std::fmt::Debug for Sells {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Sells(..)")
    }
}

impl PartialEq for Sells {
    fn eq(&self, other: &Sells) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Trade {
    /// Whether the side that pays holds the caps the balance asks of it. When it does not,
    /// the figure dims; only a player who is short is refused ([`Trade::paid`]).
    pub fn affordable(&self) -> bool {
        let (player, merchant) = (Side::Player as usize, Side::Container as usize);
        match self.balance {
            b if b < 0 => self.purse[player] as i64 >= -b,
            b => self.purse[merchant] as i64 >= b,
        }
    }

    /// The caps the trade moves, positive when the player is paid: the balance, held at the
    /// merchant's purse when it cannot pay all of it [observed 2026-11-06: barter-menu.md
    /// section 9, the original completes such a sale at that amount].
    pub fn paid(&self) -> i64 {
        self.balance.min(i64::from(self.purse[Side::Container as usize]))
    }

    /// What one unit of value is worth when it comes from `from`: a script's multiplier,
    /// else the rules'.
    fn mult(&self, from: Side) -> f32 {
        match from {
            Side::Player => self.mult_set[1].unwrap_or_else(|| self.prices.selling(self.skill, self.discount)),
            Side::Container => self.mult_set[0].unwrap_or_else(|| self.prices.buying(self.skill, self.discount)),
        }
    }

    /// The multipliers in use, buying and selling (GetBarterPriceMult).
    pub fn mults(&self) -> [f32; 2] {
        [self.mult(Side::Container), self.mult(Side::Player)]
    }

    /// What holds a side's items.
    fn holder(&self, side: Side) -> FormId {
        match side {
            Side::Container => self.stock,
            Side::Player => PLAYER,
        }
    }

    /// Whether `HideItemBarterEx` hides `item` on `side` (`1` hidden on vendor's side,
    /// `2` hidden on player's side, `3` hidden on both) [documented: HideItemBarterEx].
    pub fn is_hidden(&self, item: FormId, side: Side) -> bool {
        match self.hidden.get(&item).copied().unwrap_or(0) {
            1 => side == Side::Container,
            2 => side == Side::Player,
            3 => true,
            _ => false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.offered.iter().all(BTreeMap::is_empty)
    }
}

/// The state of an open menu.
#[derive(Debug, Clone)]
pub struct MenuModel {
    pub mode: Mode,
    search: search::Search,
    /// The column that has the keyboard (always `Player` in the inventory menu).
    pub focus: Side,
    /// Selected row per column, indexed by `Side as usize`.
    pub selected: [usize; 2],
    /// First visible row per column.
    pub scroll: [usize; 2],
    /// The selection was last moved by the pointer coming onto a row: the lists stay
    /// where they are (only the wheel and the keys scroll them; owner's playtest list 7,
    /// item 11). The menu keeps `scroll` at the rows it draws (`look::drawn_first`).
    pub pointer_pick: bool,
    /// Counts the keys' and the wheel's moves of the selection, also one that ends on the same
    /// row: asked to move, the tile menu shows the selected row again.
    pub navigated: u32,
    /// By column: the rows the category leaves out (the file's `_num_filtered`).
    pub filtered: [usize; 2],
    /// Rows per column; the container column is empty in the inventory menu.
    pub lists: [Listing; 2],
    /// The category each column shows.
    pub category: [Category; 2],
    /// The two columns change category together (the Matching Categories tweak, set
    /// by `crate::tweaks`).
    pub sync_categories: bool,
    /// When `sync_categories` is on, keep an emptied non-All category selected instead of
    /// switching both columns back to All (`bPreventSwitchWhenEmptyingCategory`, set by
    /// `crate::tweaks`).
    pub prevent_switch_when_emptying_category: bool,
    /// The player cannot offer items in a barter (the No Selling tweak, set by
    /// `crate::tweaks`).
    pub no_selling: bool,
    /// The merchant's buy and sell list holds the player's side too (the Vendors Keep to
    /// Their Lists tweak, set by `crate::tweaks`).
    pub vendor_lists_both: bool,
    /// The Pip-Boy's Misc list leaves quest items out (the tweak, set by `crate::tweaks`).
    pub hide_misc_quest: bool,
    /// The order of the player's inventory list ([`SortBy`]; the Pip-Boy's Clicking the
    /// Tab Sorts tweak).
    pub sort: SortBy,
    /// The container is a companion whose carry weight this is (the player's Quantity Menu
    /// Respects Companion Carry Cap tweak, set by `crate::menu` before a store): the
    /// quantity asked stops at what it can still carry.
    pub companion_carry: Option<f32>,
    /// The player's menu tweaks, set by `crate::tweaks`: Take All leaves the menu open;
    /// equipped items are left out of the player's side in barter and containers;
    /// category titles in title case.
    pub take_all_stays: bool,
    /// The player's Take All Confirmation tweak: Take All of at least this many units asks
    /// to be pressed again (`None`: off), and whether it is asking now [design:
    /// docs/design/tweaks.md].
    pub take_all_confirm: Option<i32>,
    confirming_take_all: bool,
    /// The player's Store All tweak: Shift with Take All stores instead.
    pub store_all: bool,
    /// Named NVForever option `quantity-select-short-keys` (off by default; #3799):
    /// Shift transfers/drops the full stack, Ctrl transfers/drops one unit skipping the
    /// quantity prompt, and picking up a weightless stack (`count > 1`) from a container
    /// opens the quantity prompt [documented: JIP LN v57.30 `jip_nvse.ini`
    /// `bQttSelectShortKeys`; design: docs/spec-notes/quantity-select-short-keys.md].
    pub qtt_select_short_keys: bool,
    /// The player's tweak: the barter figure shows the whole total, past what the
    /// merchant can pay [design: docs/design/tweaks.md].
    pub full_barter_total: bool,
    /// The player's tweak: each side's caps after the trade beside its caps now.
    pub barter_caps_change: bool,
    pub hide_equipped_barter: bool,
    pub hide_equipped_containers: bool,
    /// The player's tweaks: items on a hotkey kept off the player's side of a container
    /// and of a barter [design: docs/design/tweaks.md].
    pub hide_hotkeyed_containers: bool,
    pub hide_hotkeyed_barter: bool,
    /// And out of the Pip-Boy's repair parts (`crate::pipboy::repair::current`).
    pub hide_hotkeyed_repair: bool,
    /// The items on the hotkeys (base forms), for those tweaks.
    pub hotkeyed: Vec<FormId>,
    /// The ammo the player's carried weapons can fire, listed first in the inventory
    /// under the player's Sort Equipable Ammo tweak (empty: no such order) [design: docs/design/tweaks.md].
    pub usable_ammo: Vec<FormId>,
    /// Whether tile adapters should instantiate `ySIImage` row icons (`yUI.ini`
    /// `[Sorting and Icons] bAddInventoryIcons`, gated by `bSortingIcons`) [design:
    /// `docs/spec-notes/yui-ini.md`].
    pub ysi_icons: bool,
    /// The ammo the player's weapon loads now (`crate::combat::Combat::player_ammo`, set by
    /// `crate::tweaks` each frame): its row in the player's inventory carries the
    /// equipped mark; in a container's player column too under the player's Containers
    /// Show Equipped Ammo tweak (`ammo_mark_in_containers`).
    pub current_ammo: Option<FormId>,
    pub ammo_mark_in_containers: bool,
    /// The ammo the player's weapon can fire (`fnv_combat::data::FormStore::ammo_choices`
    /// of `crate::combat::Combat::player_weapon`, set by `crate::tweaks` each frame):
    /// every other ammo row of the player's inventory is dimmed (`Row::dim`). Empty
    /// with the fists out (every ammo row dim); `None` when not known (no dimming).
    pub firable_ammo: Option<Vec<FormId>>,
    /// What the log last said of the Ammo tab's rows ([`MenuModel::refresh`]): said again
    /// only when it changes.
    ammo_told: String,
    pub title_case_categories: bool,
    /// The price of things worth nothing in a barter (the tweak; 0: as they are).
    pub free_item_price: f32,
    /// The player's Barter Prices by Kind tweak (`None`: off), set by `crate::tweaks`.
    pub barter_kind_mults: Option<BarterKindMults>,
    /// The player's weight in red when taking the selected row would put them over (the
    /// tweak, set by `crate::tweaks`).
    pub warn_encumbrance: bool,
    /// The player's encumbrance, or why it is unknown.
    pub encumbrance: Result<Encumbrance, String>,
    /// Heading: the container's or merchant's name, or "Inventory".
    pub title: String,
    /// The last command's result or error, shown in the menu.
    pub status: Option<String>,
    /// References created by drops (the simulation has them; nothing renders them yet).
    pub dropped: Vec<FormId>,
    /// The barter menu's trade.
    pub trade: Option<Trade>,
    /// The "How many?" prompt, while it is open.
    pub quantity: Option<Quantity>,
    /// The amount the prompt chose, for the command it does on Ok.
    amount: Option<i32>,
    /// The container menu is picking a living actor's pocket ([`Steal`]).
    pub steal: Option<Steal>,
    /// A co-op guest's container menu: quest items are the host's (it carries the
    /// quests) and are not listed (design).
    pub guest: bool,
    /// Inventory: the folder open, if any ([`Folder`]).
    pub folder: Option<Folder>,
    /// Inventory: active Pip-Boy ySI category & subfolder definitions (empty in vanilla or
    /// when `yUI.ini` `[Sorting and Icons] bEnableCategories = 0`) [design:
    /// `docs/spec-notes/yui-ini.md`].
    ysi_categories: fnv_game::yui_ini::YsiCategoryIndex,
    /// Inventory: ancestor stack of open [`Folder::Ysi`] indices for multi-level subfolder
    /// navigation (`Back` pops back to the parent category before returning to tab root).
    ysi_folder_stack: Vec<u32>,
    /// Inventory: ySI category just exited via `Back`, restored onto the matching category
    /// row when [`MenuModel::apply_ysi_options`] rebuilds folded rows.
    last_exited_folder: Option<Folder>,
    /// Inventory: the player's Caravan cards, one entry a card, set by the engine from
    /// the scripts' game ([`MenuModel::set_cards`]).
    pub cards: Vec<FormId>,
}

/// What a command did besides changing the model.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Applied {
    /// The menu should close.
    pub close: bool,
    /// The inventories (or the trade) changed.
    pub changed: bool,
    /// References created by this command (drops).
    pub created: Vec<FormId>,
    /// An item the player used up (an aid item "equipped" from the inventory): its
    /// effects (and effect scripts) are the script runtime's (`crate::ItemUsed`).
    pub used: Option<FormId>,
    /// Items the player took out of the container, with their counts (`crate::crime`:
    /// taking owned items is theft).
    pub taken: Vec<(FormId, i32)>,
    /// Caps that changed hands in an accepted trade (Barter Amount Traded,
    /// `fnv_game::misc_stats`).
    pub traded: u32,
    /// Power armour the player has no training for was refused: the runtime shows the
    /// game's message (`fnv_game::functions::refuse_power_armor`).
    pub power_armor_refused: bool,
    /// Pickpocketing: items the player put into the victim's pocket, with their counts.
    pub given: Vec<(FormId, i32)>,
    /// Pickpocketing: the roll for this item failed, the victim noticed (the menu closes).
    pub caught: Option<FormId>,
    /// World references the player picked up from the menu (a corpse's dropped weapon,
    /// [`Row::reference`]): gone from the world, as a pick-up there would leave them.
    pub picked: Vec<FormId>,
}

/// Take a corpse's dropped weapon from its loot menu: the player picks the reference up,
/// the same as activating it on the ground (`World::activate`), so its condition and mods
/// come with it and it leaves the world. Returns the units taken.
fn take_reference(world: &mut World<'_>, r: FormId, picked: &mut Vec<FormId>) -> Result<i32, SimError> {
    match world.activate(PLAYER, r)?.outcome {
        fnv_sim::world::Outcome::PickedUp { count, .. } => {
            picked.push(r);
            Ok(count)
        }
        _ => Ok(0),
    }
}

/// The merchant's own items lying loose beside it, which its barter menu offers as well
/// as its stock: the item references in the merchant's cell owned by it (its base or its
/// reference), enabled and not taken, however far from it [observed: observer checks 256
/// and its follow-up, 2026-11-28, #3350: Blake's barter menu listed `VBSPsychologyBook`
/// and `VBSPediatricsBook`, persistent references owned by him on the Crimson Caravan's
/// ground, and also his owned loose beer (one row of 8) and 0x00032C75 (one row of 4),
/// which are not persistent; none of them was in his merchant container or his own
/// inventory. Hypothesis: the reach is his cell, by the counts (more units than lie
/// within 320 units of him); ownership by his faction or by the cell is not counted, as
/// it was not checked].
pub fn owned_loose(world: &World<'_>, merchant: FormId) -> Vec<FormId> {
    let placed = world.placed;
    let (cell, base) = match world.peek(merchant) {
        Some(s) => (s.cell, s.base),
        None => match placed.get(merchant) {
            Some(p) => (p.cell, p.base),
            None => return Vec::new(),
        },
    };
    let Some(cell) = cell else { return Vec::new() };
    let owns = |o: Option<FormId>| o.is_some_and(|o| o == base || o == merchant);
    placed
        .by_cell
        .get(&cell)
        .into_iter()
        .flatten()
        .copied()
        .filter(|&r| {
            let Some(p) = placed.get(r) else { return false };
            let (item, gone, owner) = match world.peek(r) {
                Some(s) => (s.base, s.deleted(), s.extra.owner),
                None => (p.base, false, p.owner),
            };
            !gone && owns(owner) && world.forms.item(item).is_some() && world.is_enabled(r).unwrap_or(false)
        })
        .collect()
}

/// Add the merchant's owned loose items ([`owned_loose`]) to its column: one row per
/// item, its units added to a row of the same units (stock or loose) if there is one
/// [observed: Blake's eight loose beers are one row, observer follow-up to check 256].
fn add_owned_loose(world: &World<'_>, merchant: FormId, l: &mut Listing) {
    let forms = world.forms;
    let mut added = false;
    for r in owned_loose(world, merchant) {
        let (item, count) = match world.peek(r) {
            Some(s) => (s.base, s.count.max(1)),
            None => match world.placed.get(r) {
                Some(p) => (p.base, p.count.unwrap_or(1).max(1)),
                None => continue,
            },
        };
        if !shown(forms, item, false) {
            continue;
        }
        let it = forms.item(item);
        l.weight += f64::from(it.and_then(|i| i.weight).unwrap_or(0.0)) * f64::from(count);
        l.value += it.and_then(|i| i.worth()).unwrap_or(0) as i64 * count as i64;
        if let Some(row) = l.rows.iter_mut().find(|row| row.item == item && row.at.is_none() && row.mods == 0) {
            row.count += count;
            continue;
        }
        l.rows.push(Row {
            item,
            at: None,
            name: display_name(forms, item),
            count,
            weight: it.and_then(|i| i.weight),
            value: it.and_then(|i| i.worth()),
            kind: it.map(|i| i.kind),
            equipped: false,
            quest: world.is_quest_item(item),
            condition: None,
            icon: it.and_then(|i| i.icon.clone()),
            price: None,
            bartered: false,
            loaded: false,
            dim: false,
            mods: 0,
            mods_value: 0,
            folder: None,
            reference: Some(r),
            live: false,
            ysi: None,
        });
        added = true;
    }
    if added {
        sort_rows(&mut l.rows);
    }
}

/// Buy `left` units of `item` from the merchant's owned loose items ([`owned_loose`]):
/// each reference goes into the player's inventory (owned by no one: bought, not
/// stolen) and leaves the world, as a pick-up there would. Returns the units moved.
fn buy_owned_loose(world: &mut World<'_>, merchant: FormId, item: FormId, mut left: i32, picked: &mut Vec<FormId>) -> Result<i32, SimError> {
    let mut moved = 0;
    for r in owned_loose(world, merchant) {
        if left <= 0 {
            break;
        }
        let s = world.get(r)?;
        if s.base != item {
            continue;
        }
        let have = s.count.max(1);
        let n = have.min(left);
        if n < have {
            s.count = have - n;
        }
        world.add_item(PLAYER, item, n)?;
        if n == have {
            world.set_deleted(r, true)?;
        }
        picked.push(r);
        (moved, left) = (moved + n, left - n);
    }
    Ok(moved)
}

/// The container menu on a living actor's inventory while the player sneaks: each take
/// or placement rolls against its chance ([`Steal::chance`], `fnv_game::pickpocket`);
/// a failed roll moves nothing and closes the menu ([`Applied::caught`]). Equipped items
/// are not listed ("By default, the player cannot pickpocket equipped items"
/// [documented: GECK wiki SetPlayerCanPickpocketEquippedItems]; hypothesis: a stack with
/// one unit equipped is hidden whole), and Take All is refused [design].
#[derive(Debug, Clone)]
pub struct Steal {
    pub victim: FormId,
    pub rules: fnv_game::pickpocket::Rules,
    /// The player's and the victim's Sneak.
    pub thief: f64,
    pub target: f64,
    /// Equipped items are listed too (ShowOff's SetPlayerCanPickpocketEquippedItems,
    /// `fnv_game::Game::pickpocket_worn`).
    pub worn: bool,
    /// The Pickpocket Overhaul tweak's figures (`fnv_game::Game::pickpocket_overhaul`).
    pub overhaul: Option<fnv_game::pickpocket::Overhaul>,
    rng: fnv_combat::rng::SplitMix,
}

impl Steal {
    pub fn new(victim: FormId, rules: fnv_game::pickpocket::Rules, thief: f64, target: f64, seed: u64) -> Steal {
        Steal { victim, rules, thief, target, worn: false, overhaul: None, rng: fnv_combat::rng::SplitMix(seed) }
    }

    /// The chance, percent, of moving `count` units of `row` either way.
    pub fn chance(&self, row: &Row, count: i32) -> i32 {
        let value = f64::from(row.value.unwrap_or(0).max(0)) * f64::from(count.max(1));
        match &self.overhaul {
            Some(o) => o.chance(&self.rules, value, f64::from(row.weight.unwrap_or(0.0).max(0.0)) * f64::from(count.max(1)), row.equipped),
            None => self.rules.chance(self.thief, self.target, value),
        }
    }

    /// Roll against `chance`: whether the move goes unnoticed.
    fn unnoticed(&mut self, chance: i32) -> bool {
        (self.rng.next_f32() * 100.0) < chance as f32
    }
}

/// The seed of one opening of `victim`'s pocket: `aid`, the test aid's number, gives the
/// same rolls at every opening of that victim (smoke scenarios that need a take to go
/// one way); else `clock` (the wall clock, nanoseconds) and `opening` (how many pockets
/// this session has opened) make every opening roll afresh, so a caught take can go
/// otherwise after a reload. Seeded by the victim alone, a victim whose first roll was
/// over fPickPocketMaxChance (85) could never be robbed at the first try: the Legion
/// assassin at Kimball's speech rolled 90.79 every time (#2751) [design: that the
/// original rolls afresh each time is taken from its being a chance at all, GECK wiki
/// Pickpocket; not captured].
pub fn steal_seed(victim: FormId, aid: Option<u64>, clock: u64, opening: u64) -> u64 {
    match aid {
        Some(a) => u64::from(victim.0) ^ a,
        None => clock ^ (u64::from(victim.0) << 24) ^ opening.wrapping_mul(0x9E37_79B9_7F4A_7C15),
    }
}

/// [`steal_seed`] for a pocket opened now. The test aid `NVF_PLAY_TEST_PICKPOCKET_SEED`
/// (a number, decimal or `0x` hexadecimal) fixes the rolls per victim.
pub fn steal_seed_now(victim: FormId) -> u64 {
    static OPENINGS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let aid = std::env::var("NVF_PLAY_TEST_PICKPOCKET_SEED").ok().and_then(|v| {
        let v = v.trim();
        match v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
            Some(h) => u64::from_str_radix(h, 16).ok(),
            None => v.parse().ok(),
        }
    });
    if let Some(a) = aid {
        bevy::log::info!("pickpocket: test aid NVF_PLAY_TEST_PICKPOCKET_SEED: the rolls on {victim} are seeded with {a:#X}, the same at every opening");
    }
    let clock = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    steal_seed(victim, aid, clock, OPENINGS.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

const PLAYER: FormId = FormId::PLAYER_REF;

impl MenuModel {
    /// Open the container menu. Fails when the reference has no inventory.
    pub fn open_container(world: &mut World<'_>, container: FormId) -> Result<MenuModel, SimError> {
        let base = world.get(container)?.base;
        let title = display_name(world.forms, base);
        let mut m = MenuModel::empty(Mode::Container(container), title);
        m.focus = Side::Container;
        m.refresh(world)?;
        Ok(m)
    }

    /// Open the player's inventory.
    pub fn open_inventory(world: &mut World<'_>) -> Result<MenuModel, SimError> {
        let mut m = MenuModel::empty(Mode::Inventory, "Inventory".into());
        m.focus = Side::Player;
        m.refresh(world)?;
        Ok(m)
    }

    /// Open the barter menu with `merchant`, whose goods and caps `stock` holds. Fails
    /// when either has no inventory.
    pub fn open_barter(world: &mut World<'_>, merchant: FormId, stock: FormId, caps: FormId, skill: f32, discount: i32) -> Result<MenuModel, SimError> {
        let base = world.get(merchant)?.base;
        let title = display_name(world.forms, base);
        let mut m = MenuModel::empty(Mode::Barter(merchant), title);
        m.trade = Some(Trade {
            merchant,
            stock,
            caps,
            discount,
            skill,
            prices: Prices::from_settings(world.forms),
            offered: [BTreeMap::new(), BTreeMap::new()],
            purse: [0, 0],
            balance: 0,
            sells: None,
            withheld: Vec::new(),
            mult_set: [None; 2],
            hidden: BTreeMap::new(),
        });
        m.focus = Side::Player;
        m.refresh(world)?;
        Ok(m)
    }

    /// Limit the merchant's rows to what it sells, and list the trade again.
    pub fn set_sells(&mut self, world: &mut World<'_>, sells: Sells) -> Result<(), SimError> {
        if let Some(t) = self.trade.as_mut() {
            t.sells = Some(sells);
        }
        self.refresh(world)
    }

    /// Apply active `HideItemBarterEx` item flags for this merchant and list the trade again.
    pub fn set_barter_hidden(&mut self, world: &mut World<'_>, hidden: BTreeMap<FormId, i64>) -> Result<(), SimError> {
        if let Some(t) = self.trade.as_mut() {
            t.hidden = hidden;
        }
        self.refresh(world)
    }

    #[cfg(test)]
    pub(super) fn test_empty(mode: Mode) -> MenuModel {
        Self::empty(mode, String::new())
    }

    /// Test-only empty inventory model for unit tests without a synthetic player plugin.
    #[cfg(test)]
    pub(crate) fn empty_inventory() -> MenuModel {
        MenuModel::empty(Mode::Inventory, "Inventory".into())
    }

    fn empty(mode: Mode, title: String) -> MenuModel {
        MenuModel {
            mode,
            search: Default::default(),
            focus: Side::Player,
            selected: [0; 2],
            scroll: [0; 2],
            pointer_pick: false,
            navigated: 0,
            filtered: [0; 2],
            lists: [Listing::default(), Listing::default()],
            category: [Category::All; 2],
            sync_categories: false,
            prevent_switch_when_emptying_category: true,
            no_selling: false,
            vendor_lists_both: false,
            hide_misc_quest: false,
            sort: SortBy::Name,
            companion_carry: None,
            take_all_stays: false,
            take_all_confirm: None,
            confirming_take_all: false,
            store_all: false,
            qtt_select_short_keys: false,
            full_barter_total: false,
            barter_caps_change: false,
            hide_equipped_barter: false,
            hide_equipped_containers: false,
            hide_hotkeyed_containers: false,
            hide_hotkeyed_barter: false,
            hide_hotkeyed_repair: false,
            hotkeyed: Vec::new(),
            usable_ammo: Vec::new(),
            ysi_icons: true,
            current_ammo: None,
            firable_ammo: None,
            ammo_mark_in_containers: false,
            ammo_told: String::new(),
            title_case_categories: false,
            free_item_price: 0.0,
            barter_kind_mults: None,
            warn_encumbrance: false,
            encumbrance: Err(String::new()),
            title,
            status: None,
            dropped: Vec::new(),
            trade: None,
            quantity: None,
            amount: None,
            steal: None,
            guest: false,
            folder: None,
            ysi_categories: fnv_game::yui_ini::YsiCategoryIndex::default(),
            ysi_folder_stack: Vec::new(),
            last_exited_folder: None,
            cards: Vec::new(),
        }
    }

    pub fn list(&self, side: Side) -> &Listing {
        &self.lists[side as usize]
    }

    /// The selected row of a column.
    pub fn selected_row(&self, side: Side) -> Option<&Row> {
        self.lists[side as usize].rows.get(self.selected[side as usize])
    }

    /// Rows shown at once.
    pub fn page(&self) -> usize {
        match self.mode {
            Mode::Inventory => VISIBLE_ROWS,
            _ => LIST_ROWS,
        }
    }

    /// Indices of the visible rows of a column.
    pub fn visible(&self, side: Side) -> std::ops::Range<usize> {
        let n = self.lists[side as usize].rows.len();
        let start = self.scroll[side as usize].min(n);
        start..(start + self.page()).min(n)
    }

    fn columns(&self) -> &'static [Side] {
        match self.mode {
            Mode::Container(_) | Mode::Barter(_) => &[Side::Container, Side::Player],
            Mode::Inventory => &[Side::Player],
        }
    }

    /// Re-read the inventories from the simulation, keeping each column's selection on
    /// the same row when it is still there (else on the same position).
    pub fn refresh(&mut self, world: &mut World<'_>) -> Result<(), SimError> {
        let keep = [Side::Container, Side::Player].map(|s| self.selected_row(s).map(|r| (r.units(), r.bartered, r.folder, r.live)));
        let search_keep = [Side::Container, Side::Player].map(|s| self.search.capture(self, s));
        if self.trade.is_some() {
            self.refresh_trade(world)?;
        } else {
            for &side in self.columns() {
                let holder = match (side, self.mode) {
                    (Side::Container, Mode::Container(c)) => c,
                    _ => PLAYER,
                };
                let mut l = listing(world, holder)?;
                if side == Side::Container && self.steal.is_none() {
                    add_dropped_beside(world, holder, &mut l);
                }
                let forms = world.forms;
                let all = l.rows.len();
                l.rows.retain(|r| self.category[side as usize].holds_item(r.kind, forms.item(r.item)));
                self.filtered[side as usize] = all - l.rows.len();
                if side == Side::Container && self.steal.as_ref().is_some_and(|s| !s.worn) {
                    l.rows.retain(|r| !r.equipped);
                }
                if side == Side::Container && self.guest {
                    let before = l.rows.len();
                    l.rows.retain(|r| !r.quest);
                    if l.rows.len() != before {
                        bevy::log::info!("menu: {} quest item row(s) left out: the host's quests' (co-op guest)", before - l.rows.len());
                    }
                }
                // The player's tweak keeps worn things out of a container's exchange [design: docs/design/tweaks.md].
                if side == Side::Player && self.hide_equipped_containers && matches!(self.mode, Mode::Container(_)) {
                    l.rows.retain(|r| !r.equipped);
                }
                // And things on a hotkey [design: docs/design/tweaks.md].
                if side == Side::Player && self.hide_hotkeyed_containers && matches!(self.mode, Mode::Container(_)) {
                    l.rows.retain(|r| !self.hotkeyed.contains(&r.item));
                }
                // The player's tweak: ammo a carried weapon fires first, in name order
                // within each part [design: docs/design/tweaks.md].
                if self.mode == Mode::Inventory && !self.usable_ammo.is_empty() {
                    l.rows.sort_by_key(|r| !self.usable_ammo.contains(&r.item));
                }
                // The ammo loaded now carries the equipped mark [hypothesis: as the original's
                // Pip-Boy shows it; observer check 193], in a container's player column under
                // the player's tweak [design: docs/design/tweaks.md].
                let mark_ammo = side == Side::Player && (self.mode == Mode::Inventory || (matches!(self.mode, Mode::Container(_)) && self.ammo_mark_in_containers));
                if let (true, Some(a)) = (mark_ammo, self.current_ammo) {
                    for r in l.rows.iter_mut().filter(|r| r.item == a) {
                        r.loaded = true;
                    }
                }
                // Ammo the weapon cannot fire is dimmed in the Pip-Boy; a weapon's other
                // kinds stay at full strength, and with the fists out every ammo row is dim
                // [observed: observer check 288, #3442].
                if side == Side::Player && self.mode == Mode::Inventory {
                    mark_dim(&mut l.rows, self.firable_ammo.as_deref());
                }
                // The Ammo tab's rows in the log, so a report about its marks can be read
                // (#3442): how many, and which carries the loaded mark.
                if self.mode == Mode::Inventory && self.category[side as usize] == Category::Ammo {
                    let marked = match (l.rows.iter().find(|r| r.loaded), self.current_ammo) {
                        (Some(r), _) => format!("{:?} ({}) carries the loaded mark", r.name, r.item),
                        (None, Some(a)) => format!("none carries the loaded mark: the weapon loads {a}, which is not in the list"),
                        (None, None) => "none carries the loaded mark: the weapon loads no ammo".to_string(),
                    };
                    let told = format!("menu: Ammo tab: {} rows, {marked}", l.rows.len());
                    if told != self.ammo_told {
                        bevy::log::info!("{told}");
                        self.ammo_told = told;
                    }
                }
                // The player's tweak: the list by value or weight, the most first, name
                // order kept between equals [design: docs/design/tweaks.md].
                if self.mode == Mode::Inventory {
                    self.sort.apply(&mut l.rows);
                }
                if self.mode == Mode::Inventory && self.category[side as usize] == Category::Misc {
                    // The player's tweak leaves quest items out of Misc [design: docs/design/tweaks.md].
                    if self.hide_misc_quest {
                        l.rows.retain(|r| !r.quest);
                    }
                    self.fold(&mut l, forms);
                }
                self.lists[side as usize] = l;
            }
        }
        let columns = self.columns();
        self.search.filter(&mut self.lists, columns);
        for &side in columns {
            let i = side as usize;
            let rows = &self.lists[i].rows;
            let at = match search_keep[i] {
                Some(key) => key.find(rows),
                None => keep[i].and_then(|k| rows.iter().position(|r| (r.units(), r.bartered, r.folder, r.live) == k)),
            };
            self.selected[i] = at.unwrap_or(self.selected[i]).min(rows.len().saturating_sub(1));
            self.ensure_visible(side);
        }
        self.encumbrance = world.encumbrance(PLAYER).map_err(|e| e.to_string());
        Ok(())
    }

    /// Install the active Pip-Boy ySI category & subfolder definitions (`None` clears them
    /// so vanilla inventory listing is restored). Resets an open `Folder::Ysi` if its index
    /// no longer exists in the updated table [design: `docs/spec-notes/yui-ini.md`].
    pub fn set_ysi_categories(&mut self, index: Option<fnv_game::yui_ini::YsiCategoryIndex>) {
        self.ysi_categories = index.unwrap_or_default();
        if let Some(Folder::Ysi(id)) = self.folder {
            if id as usize >= self.ysi_categories.categories.len() {
                self.folder = None;
                self.ysi_folder_stack.clear();
                self.last_exited_folder = None;
            }
        }
    }

    /// Pip-Boy Inventory tab index (`0..=4`) matching ySI JSON `"tab"` values (`None` for
    /// `Category::All`).
    fn tab_index(cat: Category) -> Option<u8> {
        match cat {
            Category::Weapons => Some(0),
            Category::Apparel => Some(1),
            Category::Aid => Some(2),
            Category::Misc => Some(3),
            Category::Ammo => Some(4),
            Category::All => None,
        }
    }

    fn has_ysi_categories_for_tab(&self, tab: u8) -> bool {
        self.ysi_categories.categories.iter().any(|c| c.tab == tab)
    }

    fn ysi_category_name(&self, id: u32) -> &str {
        self.ysi_categories
            .categories
            .get(id as usize)
            .map_or("Category", |c| c.name.as_str())
    }

    /// Walk `cat_id` -> ancestors up to root, returning `[root, ..., cat_id]`.
    fn ysi_category_chain(&self, mut cat_id: u32) -> Vec<u32> {
        let mut chain = Vec::new();
        for _ in 0..16 {
            let Some(def) = self.ysi_categories.categories.get(cat_id as usize) else {
                break;
            };
            if chain.contains(&cat_id) {
                break;
            }
            chain.push(cat_id);
            let Some(parent) = def.parent else {
                break;
            };
            cat_id = parent;
        }
        chain.reverse();
        chain
    }

    /// Attach resolved ySI sorting tags and icon display metadata to all non-folder rows,
    /// re-sorting each column's item rows while keeping folder rows (`Keyring`, `Caravan
    /// Cards`, `Back`) pinned at the top and preserving bartered-first, usable-ammo-first,
    /// `SortBy` (`Value`/`Weight`), and each column's selected row [design:
    /// `docs/spec-notes/ysi-filename.md`].
    pub fn apply_ysi(&mut self, resolve: impl FnMut(FormId) -> Option<fnv_game::ui_ysi::YsiItemDisplay>) {
        self.apply_ysi_options(true, true, resolve);
    }

    /// Honor `yUI.ini` `[Sorting and Icons]` `bSortInventory` (`sort_by_tag`),
    /// `bAddInventoryIcons` (`add_icons`), and `bEnableCategories` (via
    /// [`MenuModel::set_ysi_categories`]) independently while preserving folder prefix rows,
    /// bartered-first, usable-ammo-first, `SortBy`, and column selection [design:
    /// `docs/spec-notes/yui-ini.md`].
    pub fn apply_ysi_options(
        &mut self,
        sort_by_tag: bool,
        add_icons: bool,
        mut resolve: impl FnMut(FormId) -> Option<fnv_game::ui_ysi::YsiItemDisplay>,
    ) {
        self.ysi_icons = add_icons;
        let has_ysi_cats = self.mode == Mode::Inventory && !self.ysi_categories.categories.is_empty();
        let columns = self.columns();
        for &side in columns {
            let i = side as usize;
            let keep = self.selected_row(side).map(|r| (r.units(), r.bartered, r.folder, r.live));
            let rows = &mut self.lists[i].rows;
            let folder_prefix = rows.iter().take_while(|r| r.folder.is_some()).count();
            let items = &mut rows[folder_prefix..];
            if !sort_by_tag && !add_icons && !(side == Side::Player && has_ysi_cats) {
                for r in items.iter_mut() {
                    r.ysi = None;
                }
                continue;
            }
            for r in items.iter_mut() {
                if r.folder.is_none() {
                    r.ysi = resolve(r.item);
                }
            }
            if side == Side::Player && has_ysi_cats {
                self.fold_ysi_categories(sort_by_tag, add_icons, keep);
                continue;
            }
            sort_rows_by(items, sort_by_tag);
            if self.mode == Mode::Inventory && !self.usable_ammo.is_empty() {
                items.sort_by_key(|r| !self.usable_ammo.contains(&r.item));
            }
            if self.mode == Mode::Inventory {
                self.sort.apply(items);
            }
            if !add_icons {
                for r in items.iter_mut() {
                    r.ysi = None;
                }
            }
            let at = keep.and_then(|k| self.lists[i].rows.iter().position(|r| (r.units(), r.bartered, r.folder, r.live) == k));
            self.selected[i] = at.unwrap_or(self.selected[i]).min(self.lists[i].rows.len().saturating_sub(1));
            self.ensure_visible(side);
        }
    }

    /// Group the player's Pip-Boy inventory rows into clickable ySI category and nested
    /// subfolder rows when `yUI.ini` `[Sorting and Icons] bEnableCategories = 1` and the
    /// active tab has category definitions [design: `docs/spec-notes/yui-ini.md`].
    fn fold_ysi_categories(
        &mut self,
        sort_by_tag: bool,
        add_icons: bool,
        keep: Option<(Units, bool, Option<FolderRow>, bool)>,
    ) {
        let i = Side::Player as usize;
        let Some(tab) = Self::tab_index(self.category[i]).filter(|&t| self.has_ysi_categories_for_tab(t)) else {
            let rows = &mut self.lists[i].rows;
            let folder_prefix = rows.iter().take_while(|r| r.folder.is_some()).count();
            let items = &mut rows[folder_prefix..];
            sort_rows_by(items, sort_by_tag);
            if !self.usable_ammo.is_empty() {
                items.sort_by_key(|r| !self.usable_ammo.contains(&r.item));
            }
            self.sort.apply(items);
            if !add_icons {
                for r in items.iter_mut() {
                    r.ysi = None;
                }
            }
            let at = keep.and_then(|k| self.lists[i].rows.iter().position(|r| (r.units(), r.bartered, r.folder, r.live) == k));
            self.selected[i] = at.unwrap_or(self.selected[i]).min(self.lists[i].rows.len().saturating_sub(1));
            self.ensure_visible(Side::Player);
            return;
        };

        // Guard against duplicate folding if `apply_ysi_options` is called twice without a
        // fresh `refresh`.
        let already_folded = self.lists[i]
            .rows
            .iter()
            .any(|r| matches!(r.folder, Some(FolderRow::Open(Folder::Ysi(_)))))
            || (matches!(self.folder, Some(Folder::Ysi(_)))
                && self.lists[i]
                    .rows
                    .first()
                    .is_some_and(|r| r.folder == Some(FolderRow::Back)));
        if already_folded {
            if !add_icons {
                for r in &mut self.lists[i].rows {
                    r.ysi = None;
                }
            }
            return;
        }

        // Active search at tab root keeps all matching items flat so players can search
        // across collapsed categories directly.
        let searching = !self.search_text().is_empty();
        if searching && self.folder.is_none() {
            let rows = &mut self.lists[i].rows;
            sort_rows_by(rows, sort_by_tag);
            if !self.usable_ammo.is_empty() {
                rows.sort_by_key(|r| !self.usable_ammo.contains(&r.item));
            }
            self.sort.apply(rows);
            if !add_icons {
                for r in rows.iter_mut() {
                    r.ysi = None;
                }
            }
            let at = keep.and_then(|k| self.lists[i].rows.iter().position(|r| (r.units(), r.bartered, r.folder, r.live) == k));
            self.selected[i] = at.unwrap_or(self.selected[i]).min(self.lists[i].rows.len().saturating_sub(1));
            self.ensure_visible(Side::Player);
            return;
        }

        let flat = std::mem::take(&mut self.lists[i].rows);
        let mut built: Vec<Row> = Vec::new();
        let mut prefix_back = false;

        match self.folder {
            None => {
                // Tab root: group each row under its root category (for this tab) when the
                // category meets its `min_count` threshold.
                let mut counts: BTreeMap<u32, usize> = BTreeMap::new();
                let mut row_root: Vec<Option<u32>> = Vec::with_capacity(flat.len());
                for r in &flat {
                    let root = r
                        .ysi
                        .as_ref()
                        .and_then(|y| y.tag.as_deref())
                        .and_then(|t| self.ysi_categories.tag_to_category.get(t).copied())
                        .and_then(|cid| self.ysi_category_chain(cid).first().copied())
                        .filter(|&rid| {
                            self.ysi_categories
                                .categories
                                .get(rid as usize)
                                .is_some_and(|c| c.tab == tab)
                        });
                    if let Some(rid) = root {
                        *counts.entry(rid).or_insert(0) += 1;
                    }
                    row_root.push(root);
                }
                let mut emitted: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
                for (r, root) in flat.into_iter().zip(row_root) {
                    if let Some(rid) = root {
                        let cat = &self.ysi_categories.categories[rid as usize];
                        let n = counts.get(&rid).copied().unwrap_or(0);
                        if n >= cat.min_count.max(1) {
                            if emitted.insert(rid) {
                                built.push(Row::of_ysi_category(rid, cat, n as i32));
                            }
                            continue;
                        }
                    }
                    built.push(r);
                }
            }
            Some(Folder::Ysi(open_id)) => {
                prefix_back = true;
                // Keep only items belonging to `open_id` (directly or under any descendant
                // subfolder). When not searching, collapse immediate child subfolders whose
                // item row count meets `child.min_count`.
                let mut in_folder: Vec<(Row, Option<u32>)> = Vec::new();
                let mut child_counts: BTreeMap<u32, usize> = BTreeMap::new();
                for r in flat {
                    let Some(cid) = r
                        .ysi
                        .as_ref()
                        .and_then(|y| y.tag.as_deref())
                        .and_then(|t| self.ysi_categories.tag_to_category.get(t).copied())
                    else {
                        continue;
                    };
                    let chain = self.ysi_category_chain(cid);
                    let Some(pos) = chain.iter().position(|&id| id == open_id) else {
                        continue;
                    };
                    let immediate_child = (!searching).then(|| chain.get(pos + 1).copied()).flatten();
                    if let Some(ch) = immediate_child {
                        *child_counts.entry(ch).or_insert(0) += 1;
                    }
                    in_folder.push((r, immediate_child));
                }
                let mut emitted: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
                for (r, child) in in_folder {
                    if let Some(ch) = child {
                        let cat = &self.ysi_categories.categories[ch as usize];
                        let n = child_counts.get(&ch).copied().unwrap_or(0);
                        if n >= cat.min_count.max(1) {
                            if emitted.insert(ch) {
                                built.push(Row::of_ysi_category(ch, cat, n as i32));
                            }
                            continue;
                        }
                    }
                    built.push(r);
                }
            }
            Some(Folder::Keys | Folder::Cards) => {
                built = flat;
            }
        }

        sort_rows_by(&mut built, sort_by_tag);
        if !self.usable_ammo.is_empty() {
            built.sort_by_key(|r| !self.usable_ammo.contains(&r.item));
        }
        self.sort.apply(&mut built);

        if prefix_back {
            let mut rows = Vec::with_capacity(built.len() + 1);
            rows.push(Row::of_folder(FolderRow::Back, 0));
            rows.extend(built);
            built = rows;
        }
        if !add_icons {
            for r in &mut built {
                r.ysi = None;
            }
        }
        self.lists[i].rows = built;

        let exited = self.last_exited_folder.take();
        let at = exited
            .and_then(|f| {
                self.lists[i]
                    .rows
                    .iter()
                    .position(|r| r.folder == Some(FolderRow::Open(f)))
            })
            .or_else(|| {
                keep.and_then(|k| {
                    self.lists[i]
                        .rows
                        .iter()
                        .position(|r| (r.units(), r.bartered, r.folder, r.live) == k)
                })
            });
        self.selected[i] = at
            .unwrap_or(self.selected[i])
            .min(self.lists[i].rows.len().saturating_sub(1));
        self.ensure_visible(Side::Player);
    }

    /// The barter lists: each side's own rows less what it put up, and what the other
    /// side put up; prices; the caps on each side and the trade's balance.
    fn refresh_trade(&mut self, world: &mut World<'_>) -> Result<(), SimError> {
        let both_sides = self.vendor_lists_both;
        let hide_equipped = self.hide_equipped_barter;
        let hotkeyed: Vec<FormId> = if self.hide_hotkeyed_barter { self.hotkeyed.clone() } else { Vec::new() };
        let free_price = self.free_item_price;
        let kind_mults = self.barter_kind_mults;
        let Some(t) = self.trade.as_mut() else { return Ok(()) };
        let mut own: [Vec<Row>; 2] = [Vec::new(), Vec::new()];
        let mut weights = [0.0; 2];
        for side in [Side::Container, Side::Player] {
            let i = side as usize;
            let mut l = listing(world, t.holder(side))?;
            if side == Side::Container {
                add_owned_loose(world, t.merchant, &mut l);
            }
            weights[i] = l.weight;
            t.purse[i] = world.item_count(t.holder(side), t.caps)?;
            let mult = t.mult(side);
            let prices = t.prices;
            let mut withheld = Vec::new();
            own[i] = l
                .rows
                .into_iter()
                // Caps, quest items, keys (`ItemKind::Key`, #3432), and live explosives
                // (`Row::live`, #1640) are not for trade; items hidden by
                // `HideItemBarterEx` (`t.is_hidden`) stay off the requested side(s).
                .filter(|r| {
                    r.item != t.caps
                        // A merchant's owned loose item is offered though it is a quest
                        // item (observer check 256: Blake's two books, `owned_loose`).
                        && (!r.quest || r.reference.is_some())
                        && !r.live
                        && r.kind != Some(ItemKind::Key)
                        && world.resolve_item(r.item).is_none_or(|i| i.kind != ItemKind::Key)
                        && !t.is_hidden(r.item, side)
                        && !(side == Side::Player && hide_equipped && r.equipped)
                        && !(side == Side::Player && hotkeyed.contains(&r.item))
                })
                .filter(|r| {
                    // The player's tweak holds the player's side to the merchant's list too [design: docs/design/tweaks.md].
                    let why = (side == Side::Container || both_sides).then(|| t.sells.as_ref().and_then(|s| (s.0)(r.item))).flatten();
                    if let Some(w) = why.filter(|_| !withheld.iter().any(|(i, _)| *i == r.item)) {
                        withheld.push((r.item, w));
                    }
                    why.is_none()
                })
                .map(|mut r| {
                    // The player's Barter Prices by Kind tweak: the merchant's things are
                    // bought, the player's sold [design: docs/design/tweaks.md].
                    let kind = kind_mults.map_or(1.0, |k| {
                        let water = world.forms.get(r.item).and_then(|f| f.edid.as_deref()).is_some_and(|e| e.eq_ignore_ascii_case(PURIFIED_WATER));
                        k.of(side == Side::Container, r.kind, water)
                    });
                    // The player's tweak puts a price on things that are worth nothing [design: docs/design/tweaks.md].
                    r.price = r.worth(&prices).map(|v| v * kind * mult).map(|p| if p <= 0.0 && free_price > 0.0 { free_price } else { p });
                    r
                })
                .collect();
            if side == Side::Container {
                t.withheld = withheld;
            }
            // What is no longer there cannot stay put up.
            let rows = &own[i];
            t.offered[i].retain(|units, n| match rows.iter().find(|r| r.units() == *units) {
                Some(r) => {
                    *n = (*n).min(r.count);
                    *n > 0
                }
                None => false,
            });
        }
        let mut shown: [Vec<Row>; 2] = [Vec::new(), Vec::new()];
        let mut lines: [Vec<(f32, i32)>; 2] = [Vec::new(), Vec::new()];
        for side in [Side::Container, Side::Player] {
            let i = side as usize;
            for r in &own[i] {
                let up = t.offered[i].get(&r.units()).copied().unwrap_or(0);
                if r.count > up {
                    shown[i].push(Row { count: r.count - up, ..r.clone() });
                }
                if up > 0 {
                    lines[i].push((r.price.unwrap_or(0.0), up));
                    // Shown on the other side without the equipped mark [hypothesis].
                    shown[side.other() as usize].push(Row { count: up, bartered: true, equipped: false, ..r.clone() });
                }
            }
        }
        t.balance = barter::balance(lines[Side::Player as usize].iter().copied(), lines[Side::Container as usize].iter().copied());
        for side in [Side::Container, Side::Player] {
            let i = side as usize;
            let mut rows = std::mem::take(&mut shown[i]);
            rows.retain(|r| self.category[i].holds_item(r.kind, world.forms.item(r.item)));
            sort_rows(&mut rows);
            let value = rows.iter().map(|r| r.value.unwrap_or(0) as i64 * r.count as i64).sum();
            self.lists[i] = Listing { rows, weight: weights[i], value, hidden: 0 };
        }
        Ok(())
    }

    /// The Misc tab's folders: outside them, the keys and the cards are one row each at
    /// the top (only when there are some); inside one, a "Back" row and what it holds.
    /// When ySI defines Misc-tab categories (`tab == 3`), Caravan cards from `AddCardToPlayer`
    /// are merged into `l.rows` and vanilla Keyring/Caravan Cards pre-folding is deferred to
    /// [`MenuModel::fold_ysi_categories`] so ySI's own Misc categories govern grouping.
    fn fold(&self, l: &mut Listing, forms: &FormStore) {
        if self.has_ysi_categories_for_tab(3) {
            if !self.cards.is_empty() {
                for &card in &self.cards {
                    match l.rows.iter_mut().find(|r| r.item == card) {
                        Some(r) => r.count += 1,
                        None => l.rows.push(Row {
                            item: card,
                            at: None,
                            name: display_name(forms, card),
                            count: 1,
                            weight: None,
                            value: None,
                            kind: Some(ItemKind::CaravanCard),
                            equipped: false,
                            quest: false,
                            condition: None,
                            icon: forms.item(card).and_then(|i| i.icon.clone()),
                            price: None,
                            bartered: false,
                            loaded: false,
                            dim: false,
                            mods: 0,
                            mods_value: 0,
                            folder: None,
                            reference: None,
                            live: false,
                            ysi: None,
                        }),
                    }
                }
                sort_rows(&mut l.rows);
            }
            return;
        }
        let (inside, rest): (Vec<Row>, Vec<Row>) = std::mem::take(&mut l.rows).into_iter().partition(|r| Folder::Keys.holds(r.kind) || Folder::Cards.holds(r.kind));
        match self.folder {
            None => {
                let count = |f: Folder| inside.iter().filter(|r| f.holds(r.kind)).map(|r| r.count).sum::<i32>();
                let cards = count(Folder::Cards) + self.cards.len() as i32;
                let mut rows: Vec<Row> = [(Folder::Keys, count(Folder::Keys)), (Folder::Cards, cards)].into_iter().filter(|&(_, n)| n > 0).map(|(f, n)| Row::of_folder(FolderRow::Open(f), n)).collect();
                rows.extend(rest);
                l.rows = rows;
            }
            Some(f) => {
                let mut held: Vec<Row> = inside.into_iter().filter(|r| f.holds(r.kind)).collect();
                if f == Folder::Cards {
                    for &card in &self.cards {
                        match held.iter_mut().find(|r| r.item == card) {
                            Some(r) => r.count += 1,
                            None => held.push(Row {
                                item: card,
                                at: None,
                                name: display_name(forms, card),
                                count: 1,
                                weight: None,
                                value: None,
                                kind: Some(ItemKind::CaravanCard),
                                equipped: false,
                                quest: false,
                                condition: None,
                                icon: forms.item(card).and_then(|i| i.icon.clone()),
                                price: None,
                                bartered: false, loaded: false, dim: false,
                                mods: 0,
                                mods_value: 0,
                                folder: None,
                                reference: None,
                                live: false,
                                ysi: None,
                            }),
                        }
                    }
                    sort_rows(&mut held);
                }
                l.rows = std::iter::once(Row::of_folder(FolderRow::Back, 0)).chain(held).collect();
            }
        }
    }

    /// Leave the active folder: pops one level of [`MenuModel::ysi_folder_stack`] for nested
    /// ySI subfolders or returns to the tab root (`None`), remembering the exited folder so
    /// selection restores onto its row.
    fn exit_folder(&mut self, world: &mut World<'_>) -> Result<String, SimError> {
        match self.folder {
            Some(Folder::Ysi(leaving)) => {
                let parent = self.ysi_folder_stack.pop().map(Folder::Ysi);
                self.last_exited_folder = Some(Folder::Ysi(leaving));
                self.enter(world, parent)?;
                Ok(match parent {
                    Some(Folder::Ysi(pid)) => format!("Back to {}", self.ysi_category_name(pid)),
                    _ => format!("Back to {}", self.category[Side::Player as usize].title()),
                })
            }
            Some(_) => {
                self.ysi_folder_stack.clear();
                self.last_exited_folder = None;
                self.enter(world, None)?;
                Ok("Back to Misc".into())
            }
            None => Ok(String::new()),
        }
    }

    /// Open a Misc folder, or leave the one open (`None`, the folder's row then
    /// selected).
    fn enter(&mut self, world: &mut World<'_>, folder: Option<Folder>) -> Result<(), SimError> {
        let from = std::mem::replace(&mut self.folder, folder);
        let i = Side::Player as usize;
        self.selected[i] = 0;
        self.scroll[i] = 0;
        self.refresh(world)?;
        if let (None, Some(f)) = (folder, from) {
            if let Some(at) = self.lists[i].rows.iter().position(|r| r.folder == Some(FolderRow::Open(f))) {
                self.selected[i] = at;
                self.ensure_visible(Side::Player);
            }
        }
        Ok(())
    }

    /// The player's Caravan cards changed (the engine reads them from the scripts'
    /// game): the inventory's lists are read again.
    pub fn set_cards(&mut self, world: &mut World<'_>, cards: Vec<FormId>) -> Result<(), SimError> {
        if self.cards == cards {
            return Ok(());
        }
        self.cards = cards;
        if self.mode == Mode::Inventory {
            self.refresh(world)?;
        }
        Ok(())
    }

    fn ensure_visible(&mut self, side: Side) {
        let page = self.page();
        let i = side as usize;
        let n = self.lists[i].rows.len();
        let (sel, s) = (self.selected[i], &mut self.scroll[i]);
        if sel < *s {
            *s = sel;
        } else if sel >= *s + page {
            *s = sel + 1 - page;
        }
        *s = (*s).min(n.saturating_sub(page));
    }

    fn move_selection(&mut self, delta: i64) {
        let i = self.focus as usize;
        let n = self.lists[i].rows.len() as i64;
        if n == 0 {
            return;
        }
        self.selected[i] = (self.selected[i] as i64 + delta).clamp(0, n - 1) as usize;
        self.ensure_visible(self.focus);
    }

    /// Apply a command. Errors from the simulation become the status line; nothing
    /// panics.
    pub fn apply(&mut self, world: &mut World<'_>, cmd: Command) -> Applied {
        let mut out = Applied::default();
        if self.quantity.is_some() {
            return self.apply_quantity(world, cmd);
        }
        let page = self.page() as i64 - 1;
        if matches!(cmd, Command::Up | Command::Down | Command::PageUp | Command::PageDown | Command::Scroll(_)) {
            self.navigated = self.navigated.wrapping_add(1);
        }
        match cmd {
            Command::Hover(..) => self.pointer_pick = true,
            Command::Up | Command::Down | Command::PageUp | Command::PageDown | Command::Scroll(_) | Command::Select(..) | Command::SetCategory(..) => {
                self.pointer_pick = false
            }
            _ => {}
        }
        match cmd {
            Command::Up => self.move_selection(-1),
            Command::Down => self.move_selection(1),
            Command::PageUp => self.move_selection(-page),
            Command::PageDown => self.move_selection(page),
            Command::Scroll(d) => self.move_selection(d as i64),
            Command::Focus(side) => {
                if self.columns().contains(&side) {
                    self.focus = side;
                }
            }
            Command::Select(side, idx) | Command::Hover(side, idx) => {
                if !self.columns().contains(&side) || idx >= self.lists[side as usize].rows.len() {
                    return out;
                }
                if matches!(cmd, Command::Select(..)) && self.focus == side && self.selected[side as usize] == idx {
                    return self.apply(world, Command::One);
                }
                self.focus = side;
                self.selected[side as usize] = idx;
                if !self.pointer_pick {
                    self.ensure_visible(side);
                }
            }
            Command::SetCategory(side, cat) => {
                let i = side as usize;
                if !self.columns().contains(&side) || self.category[i] == cat {
                    return out;
                }
                self.category[i] = cat;
                self.folder = None;
                self.ysi_folder_stack.clear();
                self.last_exited_folder = None;
                self.selected[i] = 0;
                self.scroll[i] = 0;
                if self.sync_categories {
                    let o = 1 - i;
                    self.category[o] = cat;
                    self.selected[o] = 0;
                    self.scroll[o] = 0;
                }
                if let Err(e) = self.refresh(world) {
                    self.status = Some(format!("Cannot list the inventories: {e}"));
                }
            }
            Command::SetSort(sort) => {
                if self.sort == sort {
                    return out;
                }
                self.sort = sort;
                self.selected[Side::Player as usize] = 0;
                self.scroll[Side::Player as usize] = 0;
                if let Err(e) = self.refresh(world) {
                    self.status = Some(format!("Cannot list the inventories: {e}"));
                }
            }
            Command::Category(side, by) => {
                if self.mode == Mode::Inventory || !self.columns().contains(&side) {
                    return out;
                }
                let i = side as usize;
                self.category[i] = self.category[i].step(by);
                self.selected[i] = 0;
                self.scroll[i] = 0;
                // The player's tweak keeps the other side on the same category [design: docs/design/tweaks.md].
                if self.sync_categories {
                    let o = 1 - i;
                    self.category[o] = self.category[i];
                    self.selected[o] = 0;
                    self.scroll[o] = 0;
                }
                if let Err(e) = self.refresh(world) {
                    self.status = Some(format!("Cannot list the inventories: {e}"));
                }
            }
            Command::Close => out.close = true,
            Command::Back => {
                if self.mode == Mode::Inventory && self.folder.is_some() {
                    if let Err(e) = self.exit_folder(world) {
                        self.status = Some(format!("Cannot list the inventories: {e}"));
                    }
                }
            }
            Command::CommitBarter(_) | Command::CommitNormalBarter(_) | Command::Amount(_) | Command::AmountStep(_) | Command::AmountOk | Command::AmountCancel => {}
            _ => {
                // The player's Take All Confirmation tweak: the first Take All asks, the next
                // does it; anything else takes the question back [design: docs/design/tweaks.md].
                let confirmed = std::mem::take(&mut self.confirming_take_all);
                if let (Command::TakeAll, false, Some(n)) = (cmd, confirmed, self.take_all_asks()) {
                    self.confirming_take_all = true;
                    self.status = Some(format!("Take all {n} items? Take All again to confirm"));
                    return out;
                }
                if let Some(q) = self.asks_quantity(world, cmd) {
                    self.quantity = Some(q);
                    self.status = None;
                    return out;
                }
                let had_rows = [Side::Container, Side::Player].map(|s| !self.lists[s as usize].rows.is_empty());
                let r = self.act(world, cmd, &mut out);
                self.amount = None;
                self.status = Some(match r {
                    Ok(msg) => msg,
                    Err(e) => plain_error(world.forms, &e),
                });
                if out.changed {
                    if let Err(e) = self.refresh(world) {
                        self.status = Some(format!("Cannot list the inventories: {e}"));
                    } else if self.sync_categories
                        && !self.prevent_switch_when_emptying_category
                        && matches!(self.mode, Mode::Container(_) | Mode::Barter(_))
                    {
                        // When synchronized categories allow switching on empty
                        // (`bPreventSwitchWhenEmptyingCategory = 0`), emptying a non-All
                        // category returns both columns to All [design; Stewie's Tweaks
                        // public INI `nvse_stewie_tweaks.ini`].
                        let emptied = self.columns().iter().any(|&side| {
                            let i = side as usize;
                            self.category[i] != Category::All
                                && had_rows[i]
                                && self.lists[i].rows.is_empty()
                                && self.search_hidden(side) == 0
                        });
                        if emptied {
                            self.category = [Category::All; 2];
                            self.selected = [0; 2];
                            self.scroll = [0; 2];
                            if let Err(e) = self.refresh(world) {
                                self.status = Some(format!("Cannot list the inventories: {e}"));
                            }
                        }
                    }
                }
            }
        }
        out
    }

    /// Whether `g`'s row is still the row its input saw, in this menu and with no prompt over it.
    pub fn row_is(&self, g: &RowGuard) -> bool {
        self.mode == g.mode
            && self.quantity.is_none()
            && RowKey::at(&self.list(g.side).rows, g.index) == Some(g.key)
            && (!g.selected || (self.focus == g.side && self.selected[g.side as usize] == g.index))
    }

    /// Whether Take All asks first (the player's tweak): in a container, not stealing, with at
    /// least its count of units listed; the count.
    fn take_all_asks(&self) -> Option<i32> {
        let min = self.take_all_confirm?;
        let n: i32 = self.lists[Side::Container as usize].rows.iter().map(|r| r.count).sum();
        (matches!(self.mode, Mode::Container(_)) && self.steal.is_none() && n > 0 && n >= min).then_some(n)
    }

    /// Whether Take All is asking to be pressed again (the player's Take All Confirmation tweak).
    pub fn confirming_take_all(&self) -> bool {
        self.confirming_take_all
    }

    /// Whether `r` is effectively weightless for the player (ammo outside Hardcore mode,
    /// kinds marked weightless by `WeightRules`, or items with weight `<= 0` / none)
    /// [design: docs/spec-notes/quantity-select-short-keys.md].
    fn row_is_weightless(world: &World<'_>, r: &Row) -> bool {
        let rules = world.weight_rules(PLAYER);
        let ammo_has_weight = rules.ammo_weight.unwrap_or(world.config().hardcore);
        if r.kind == Some(ItemKind::Ammo) && !ammo_has_weight {
            return true;
        }
        if r.kind.is_some_and(|k| rules.weightless(k, false, false)) {
            return true;
        }
        r.weight.unwrap_or(0.0) <= 0.0
    }

    /// The prompt asked for by `cmd` on the selected row, if its stack is large enough
    /// (or, with `quantity-select-short-keys` enabled, when picking up a weightless stack
    /// of more than one unit from a container).
    fn asks_quantity(&self, world: &World<'_>, cmd: Command) -> Option<Quantity> {
        let asks = matches!((self.mode, cmd), (Mode::Container(_) | Mode::Barter(_), Command::One) | (Mode::Inventory, Command::Drop));
        let r = self.selected_row(self.focus)?;
        // The player's tweak: storing with a companion, no more than it can carry (at
        // least one) [design: docs/design/tweaks.md].
        let max = match (self.mode, self.focus, self.companion_carry) {
            (Mode::Container(_), Side::Player, Some(cap)) => carry_cap(r.count, r.weight.unwrap_or(0.0), cap - self.lists[Side::Container as usize].weight as f32),
            _ => r.count,
        };
        let weightless_container_pickup = self.qtt_select_short_keys
            && matches!((self.mode, self.focus, cmd), (Mode::Container(_), Side::Container, Command::One))
            && Self::row_is_weightless(world, r)
            && r.count > 1;
        let threshold_met = r.count > ASK_QUANTITY_ABOVE || weightless_container_pickup;
        (asks && self.amount.is_none() && threshold_met && r.folder.is_none() && !r.live && r.kind != Some(ItemKind::CaravanCard)).then_some(Quantity {
            cmd,
            side: self.focus,
            row: self.selected[self.focus as usize],
            max,
            amount: max,
        })
    }

    /// A command while the prompt is open: only the prompt's own (and Close, as Cancel).
    fn apply_quantity(&mut self, world: &mut World<'_>, cmd: Command) -> Applied {
        let Some(q) = self.quantity.as_mut() else { return Applied::default() };
        match cmd {
            Command::Amount(n) => q.amount = n.clamp(1, q.max),
            Command::AmountStep(d) => q.amount = q.amount.saturating_add(d).clamp(1, q.max),
            Command::AmountCancel | Command::Close => self.quantity = None,
            Command::AmountOk => {
                let q = *q;
                self.quantity = None;
                // The rows stand still while the prompt is open.
                self.focus = q.side;
                self.selected[q.side as usize] = q.row;
                self.amount = Some(q.amount);
                return self.apply(world, q.cmd);
            }
            _ => {}
        }
        Applied::default()
    }

    /// The units a command moves: the prompt's amount, else one or the whole stack.
    fn want(&mut self, single: bool, r: &Row) -> i32 {
        self.amount.take().unwrap_or(if single { 1 } else { r.count })
    }

    /// The commands that change inventories; returns the status message.
    fn act(&mut self, world: &mut World<'_>, cmd: Command, out: &mut Applied) -> Result<String, SimError> {
        let forms = world.forms;
        let sel = self.selected_row(self.focus).cloned();
        let name = |r: &Row| r.name.clone();
        match (self.mode, cmd) {
            (Mode::Container(_), Command::TakeAll) if self.steal.is_some() => Ok("Take items one at a time to steal them".into()),
            (Mode::Container(c), Command::TakeAll) => {
                // The listed stacks only: non-playable and nameless items stay hidden in the container.
                let mut moved = Vec::new();
                // A live explosive's row stays (`Row::live`).
                for r in self.lists[Side::Container as usize].rows.iter().filter(|r| !r.live) {
                    let n = match r.reference {
                        Some(at) => take_reference(world, at, &mut out.picked)?,
                        None => world.transfer_at(c, PLAYER, r.item, r.count, r.pick())?,
                    };
                    if n > 0 {
                        moved.push((r.item, n));
                    }
                }
                out.changed = !moved.is_empty();
                // Taking everything closes the menu (owner, list 5 item 60), unless the
                // player's tweak keeps it open [design: docs/design/tweaks.md].
                out.close = out.changed && !self.take_all_stays;
                let n: i64 = moved.iter().map(|m| m.1 as i64).sum();
                out.taken.clone_from(&moved);
                Ok(if moved.is_empty() { "Nothing to take".into() } else { format!("Took everything ({n} items)") })
            }
            (Mode::Container(_), Command::StoreAll) if self.steal.is_some() => Ok("Give items one at a time".into()),
            (Mode::Container(c), Command::StoreAll) => {
                let rows: Vec<Row> = self.lists[Side::Player as usize].rows.iter().filter(|r| r.folder.is_none() && !r.equipped && !r.quest).cloned().collect();
                let mut n: i64 = 0;
                for r in &rows {
                    n += world.transfer_at(PLAYER, c, r.item, r.count, r.pick())? as i64;
                }
                out.changed = n > 0;
                Ok(if n == 0 { "Nothing to store".into() } else { format!("Stored everything ({n} items)") })
            }
            (Mode::Container(c), Command::One | Command::OneSingle | Command::Stack) => {
                let Some(r) = sel else { return Ok("Nothing selected".into()) };
                let (from, to) = match self.focus {
                    Side::Container => (c, PLAYER),
                    Side::Player => (PLAYER, c),
                };
                // A live explosive in a pocket is taken by nobody, the player included,
                // and costs no roll [design, #1640; owner-review.md].
                if r.live {
                    self.amount = None;
                    return Ok(live_refusal(&r.name));
                }
                // Picking a pocket, nothing is rolled for a row whose units the simulation
                // will not move (one listed before its explosive was armed, #1640).
                if self.steal.is_some() && r.reference.is_none() {
                    let inv = world.peek(from).and_then(|s| s.inventory.as_ref());
                    if inv.map_or(0, |i| i.count_at(r.item, r.pick())) == 0 {
                        self.amount = None;
                        return Ok(match inv.is_some_and(|i| i.live_count(r.item) > 0) {
                            true => live_refusal(&live_name(r.name.clone())),
                            false => format!("Cannot find {} in the inventory", r.name),
                        });
                    }
                }
                let want = self.want(matches!(cmd, Command::One | Command::OneSingle), &r);
                if let Some(st) = self.steal.as_mut() {
                    let chance = st.chance(&r, want);
                    if !st.unnoticed(chance) {
                        out.caught = Some(r.item);
                        out.close = true;
                        return Ok(format!("Caught! ({} x{want}, {chance}%)", name(&r)));
                    }
                }
                let n = match (r.reference, self.focus) {
                    (Some(at), Side::Container) => take_reference(world, at, &mut out.picked)?,
                    _ => world.transfer_at(from, to, r.item, want, r.pick())?,
                };
                out.changed = n > 0;
                if self.focus == Side::Container && n > 0 {
                    out.taken.push((r.item, n));
                }
                if self.focus == Side::Player && n > 0 && self.steal.is_some() {
                    out.given.push((r.item, n));
                }
                let verb = if self.focus == Side::Container { "Took" } else { "Stored" };
                Ok(format!("{verb} {} x{n}", name(&r)))
            }
            (Mode::Barter(_), Command::One | Command::OneSingle | Command::Stack) => {
                let Some(r) = sel else { return Ok("Nothing selected".into()) };
                if r.live {
                    self.amount = None;
                    return Ok(live_refusal(&r.name));
                }
                let want = self.want(matches!(cmd, Command::One | Command::OneSingle), &r);
                let Some(t) = self.trade.as_mut() else { return Ok(String::new()) };
                out.changed = true;
                if r.bartered {
                    // Taken back by the side it came from.
                    let from = self.focus.other() as usize;
                    let left = t.offered[from].get(&r.units()).copied().unwrap_or(0) - want;
                    if left > 0 {
                        t.offered[from].insert(r.units(), left);
                    } else {
                        t.offered[from].remove(&r.units());
                    }
                    return Ok(format!("Took back {} x{want}", name(&r)));
                }
                // The player's tweak: nothing of the player's is sold [design: docs/design/tweaks.md].
                if self.focus == Side::Player && self.no_selling {
                    return Ok("You cannot sell here.".into());
                }
                *t.offered[self.focus as usize].entry(r.units()).or_insert(0) += want;
                let verb = if self.focus == Side::Player { "Selling" } else { "Buying" };
                Ok(format!("{verb} {} x{want}", name(&r)))
            }
            (Mode::Barter(_), Command::Accept) => self.accept(world, out),
            (Mode::Inventory, Command::One) => {
                let Some(r) = sel else { return Ok("Nothing selected".into()) };
                match r.folder {
                    Some(FolderRow::Open(Folder::Ysi(id))) => {
                        if let Some(Folder::Ysi(prev)) = self.folder {
                            self.ysi_folder_stack.push(prev);
                        }
                        self.last_exited_folder = None;
                        self.enter(world, Some(Folder::Ysi(id)))?;
                        return Ok(format!("Opened {}", self.ysi_category_name(id)));
                    }
                    Some(FolderRow::Open(f)) => {
                        self.ysi_folder_stack.clear();
                        self.last_exited_folder = None;
                        self.enter(world, Some(f))?;
                        return Ok(format!("Opened {}", f.name()));
                    }
                    Some(FolderRow::Back) => {
                        return self.exit_folder(world);
                    }
                    None => {}
                }
                // Caravan cards are for the Caravan table [design].
                if r.kind == Some(ItemKind::CaravanCard) {
                    return Ok(format!("{} is played at a Caravan table", name(&r)));
                }
                out.changed = true;
                // An item a script locked on or off stays as it is (EquipItem's
                // NoUnequipFlag, UnequipItem's NoEquipFlag, `fnv_sim::world::EquipLock`)
                // [documented: GECK wiki EquipItem, "actor (including the player) will be
                // unable to unequip the item"; the line is ours].
                if r.equipped {
                    match world.unequip(PLAYER, r.item) {
                        Err(SimError::EquipLocked { .. }) => return Ok(format!("{} can't be unequipped", name(&r))),
                        other => other?,
                    };
                    return Ok(format!("Unequipped {}", name(&r)));
                }
                let e = match world.equip_at(PLAYER, r.item, r.pick()) {
                    Err(SimError::EquipLocked { item, .. }) if item == r.item => return Ok(format!("{} can't be equipped", name(&r))),
                    Err(SimError::EquipLocked { item, .. }) => return Ok(format!("{} can't be equipped while {} is worn", name(&r), display_name(forms, item))),
                    Err(SimError::NoPowerArmorTraining(_)) => {
                        out.power_armor_refused = true;
                        return Ok(format!("Cannot wear {} without Power Armor Training", name(&r)));
                    }
                    other => other?,
                };
                if e.consumed {
                    out.used = Some(r.item);
                }
                let off: Vec<String> = e.unequipped.iter().map(|&i| display_name(forms, i)).collect();
                Ok(match (e.consumed, off.is_empty()) {
                    (true, _) => format!("Used {}", name(&r)),
                    (false, true) => format!("Equipped {}", name(&r)),
                    (false, false) => format!("Equipped {} (took off {})", name(&r), off.join(", ")),
                })
            }
            (Mode::Inventory, Command::Drop | Command::DropSingle | Command::DropStack) => {
                let Some(r) = sel else { return Ok("Nothing selected".into()) };
                // A folder row is no item; the cards AddCardToPlayer gave are not in the
                // inventory [design: carried CCRD items are not dropped either].
                if r.folder.is_some() {
                    return Ok("Nothing to drop".into());
                }
                if r.kind == Some(ItemKind::CaravanCard) {
                    return Ok(format!("{} can't be dropped", name(&r)));
                }
                let want = self.want(matches!(cmd, Command::Drop | Command::DropSingle), &r);
                let created = match world.player_drop_item_at(r.item, want, r.pick()) {
                    Err(SimError::QuestItem(_)) => return Ok(format!("{} is a quest item and can't be dropped", name(&r))),
                    other => other?,
                };
                out.changed = !created.is_empty();
                self.dropped.extend(&created);
                out.created = created;
                Ok(format!("Dropped {} x{want}", name(&r)))
            }
            (Mode::Container(_) | Mode::Barter(_), Command::Drop | Command::DropSingle | Command::DropStack) => Ok("Drop from the inventory menu (Tab)".into()),
            (Mode::Inventory, Command::OneSingle | Command::Stack | Command::TakeAll | Command::StoreAll) => Ok("No container open".into()),
            _ => Ok(String::new()),
        }
    }

    /// Barter: move what was put up and the caps it comes to, if the player can pay; a
    /// merchant who cannot pays what it holds ([`Trade::paid`]).
    fn accept(&mut self, world: &mut World<'_>, out: &mut Applied) -> Result<String, SimError> {
        let Some(t) = self.trade.as_mut() else { return Ok(String::new()) };
        if t.is_empty() {
            return Ok("Nothing to trade".into());
        }
        let (player, merchant) = (Side::Player as usize, Side::Container as usize);
        if t.balance < 0 && !t.affordable() {
            return Ok("You don't have enough caps.".into());
        }
        for (u, &n) in &t.offered[player] {
            world.transfer_at(PLAYER, t.stock, u.0, n, units_pick(u))?;
        }
        for (u, &n) in &t.offered[merchant] {
            // From its stock first, then from its owned loose items (`owned_loose`).
            let moved = world.transfer_at(t.stock, PLAYER, u.0, n, units_pick(u))?;
            if moved < n && u.1.is_none() && u.2 == 0 {
                buy_owned_loose(world, t.merchant, u.0, n - moved, &mut out.picked)?;
            }
        }
        let paid = t.paid();
        match paid {
            p if p > 0 => world.transfer(t.stock, PLAYER, t.caps, p as i32)?,
            p if p < 0 => world.transfer(PLAYER, t.stock, t.caps, (-p) as i32)?,
            _ => 0,
        };
        t.offered = [BTreeMap::new(), BTreeMap::new()];
        out.changed = true;
        out.traded = paid.unsigned_abs() as u32;
        Ok(match paid {
            p if p > 0 => format!("Traded: received {p} caps"),
            p if p < 0 => format!("Traded: paid {} caps", -p),
            _ => "Traded".into(),
        })
    }

    /// Key hints for the footer, for the current mode, one line each.
    pub fn bindings(&self) -> [&'static str; 2] {
        match self.mode {
            Mode::Container(_) => [
                "Up/Down select   Left/Right column   Enter/Space move one   Shift+Enter move stack   R take all",
                "Click a row to select it, again to move one   Wheel scroll   E/Esc/Tab close   * equipped   Q quest item",
            ],
            Mode::Barter(_) => [
                "Up/Down select   Left/Right column   Enter/Space put up or take back one   Shift+Enter the stack   A accept",
                "Click a row to put it up or take it back   [ ] categories   Wheel scroll   X/Esc/Tab exit",
            ],
            Mode::Inventory => [
                "Up/Down select   Enter/Space equip/unequip (aid: use)   Right click drop one   Shift+right click drop stack",
                "Click a row to select it, again to equip   Wheel scroll   Tab/Esc/E close   * equipped   Q quest item",
            ],
        }
    }
}

/// Test aid (F10): the container reference nearest to `pos` (game space) that is in the
/// world now, with its distance. `interior`: search that cell; `None`: the worldspace
/// `world`. Containers are references whose base is a CONT.
pub fn nearest_container(
    world: &World<'_>,
    refs: &RefTable,
    interior: Option<FormId>,
    worldspace: FormId,
    pos: [f32; 3],
) -> Option<(FormId, f32)> {
    let forms = world.forms;
    let candidates: Box<dyn Iterator<Item = &PlacedRef>> = match interior {
        Some(cell) => Box::new(refs.by_cell.get(&cell).into_iter().flatten().filter_map(|&id| refs.get(id))),
        None => Box::new(refs.refs.values().filter(|p| p.cell.and_then(|c| refs.cell_world.get(&c)) == Some(&worldspace))),
    };
    candidates
        .filter_map(|p| {
            let (base, at, gone) = match world.peek(p.id) {
                Some(s) => (s.base, s.pos, s.deleted()),
                None => (p.base, p.pos, false),
            };
            let is_container = matches!(forms.get(base).map(|b| &b.data), Some(BaseData::Container(_)));
            let present = !gone && world.is_enabled(p.id).unwrap_or(true);
            let d = ((at[0] - pos[0]).powi(2) + (at[1] - pos[1]).powi(2) + (at[2] - pos[2]).powi(2)).sqrt();
            (is_container && present).then_some((p.id, d))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
}

/// Whether the inventory's Enter (`Command::One` on the row `r`, an item row) does
/// anything: the game takes off what is worn ([`World::unequip_check`]) and puts on or uses
/// the rest ([`World::equip_check`], the checks `World::equip_at` runs: a book that teaches
/// nothing, a broken weapon, misc and ammo are refused); a Caravan card is for the table.
/// The Pip-Boy's equip hint reads it (#1896). `false` for a folder row (Enter opens it).
pub fn enter_acts(world: &World<'_>, r: &Row) -> bool {
    if r.folder.is_some() || r.kind == Some(ItemKind::CaravanCard) {
        return false;
    }
    if r.equipped {
        world.unequip_check(PLAYER, r.item).is_ok()
    } else {
        world.equip_check(PLAYER, r.item, r.pick()).is_ok()
    }
}


/// Purified water's Editor ID [observed: FalloutNV.esm ALCH `WaterPurified`], kept at its
/// own price under the Barter Prices by Kind tweak's Purified Water Unscaled.
const PURIFIED_WATER: &str = "WaterPurified";

/// The player's Barter Prices by Kind tweak: a price multiplier for each kind of item,
/// one set when the player sells and one when buying, before the barter skill's; Armor,
/// Weapon, Aid, Ammo, Misc (every other kind) and Weapon Mod [design: docs/design/tweaks.md].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarterKindMults {
    pub sell: [f32; 6],
    pub buy: [f32; 6],
    /// Purified water keeps its price (multiplier 1).
    pub water_unscaled: bool,
}

impl BarterKindMults {
    /// The multiplier of an item of `kind` the player is `buying` (else selling).
    pub fn of(&self, buying: bool, kind: Option<ItemKind>, water: bool) -> f32 {
        if water && self.water_unscaled {
            return 1.0;
        }
        let at = match kind {
            Some(ItemKind::Armor) => 0,
            Some(ItemKind::Weapon) => 1,
            Some(ItemKind::Aid) => 2,
            Some(ItemKind::Ammo) => 3,
            Some(ItemKind::ItemMod) => 5,
            _ => 4,
        };
        if buying { self.buy[at] } else { self.sell[at] }
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
#[path = "model/ysi_categories_tests.rs"]
mod ysi_categories_tests;
/// [design, #2403] Synthetic list/quantity/confirmation states for detector regressions.
#[cfg(feature = "report-test-support")]
#[doc(hidden)]
pub fn report_progress_fixture(mode: Mode, step: usize) -> MenuModel {
    let mut m = MenuModel::empty(mode, "Synthetic menu".into());
    let rows: Vec<_> = (0..20)
        .map(|i| Row {
            item: FormId(100 + i),
            at: None,
            name: format!("Synthetic item {i}"),
            count: 10,
            weight: Some(1.0),
            value: Some(1),
            kind: Some(ItemKind::Weapon),
            equipped: false,
            quest: false,
            condition: Some(1.0),
            icon: None,
            price: None,
            bartered: false,
            loaded: false, dim: false,
            mods: 0,
            mods_value: 0,
            folder: None,
            reference: None,
            live: false,
            ysi: None,
        })
        .collect();
    m.lists = [Listing { rows: rows.clone(), ..Default::default() }, Listing { rows, ..Default::default() }];
    if step >= 1 {
        m.selected[0] = 1;
    }
    if step >= 2 {
        m.scroll[0] = 1;
    }
    if step >= 3 {
        m.category[0] = Category::Weapons;
    }
    if step >= 4 {
        m.quantity = Some(Quantity { cmd: Command::One, side: Side::Player, row: 0, max: 3, amount: 1 });
    }
    if step >= 5 {
        m.quantity.as_mut().unwrap().amount = 2;
    }
    if step >= 6 {
        m.lists[0].value = 42;
    }
    if step >= 7 {
        m.confirming_take_all = true;
    }
    if step >= 8 {
        m.status = Some("Synthetic result".into());
    }
    m.navigated = step as u32;
    m.pointer_pick = step >= 9;
    m
}

/// [design, #2403] A zero-balance barter still changes when another item is offered.
#[cfg(feature = "report-test-support")]
#[doc(hidden)]
pub fn report_trade_progress_fixture(step: usize) -> MenuModel {
    let mut m = report_progress_fixture(Mode::Barter(FormId(42)), 0);
    let mut t = Trade {
        merchant: FormId(42),
        stock: FormId(43),
        caps: FormId(44),
        discount: 0,
        skill: 50.0,
        prices: Prices::from_settings(&FormStore::default()),
        offered: Default::default(),
        purse: [100; 2],
        balance: 0,
        sells: None,
        withheld: vec![],
        mult_set: [None; 2],
        hidden: BTreeMap::new(),
    };
    if step >= 1 {
        t.offered[0].insert((FormId(100), None, 0), 1);
    }
    m.trade = Some(t);
    m.navigated = step as u32;
    m
}
