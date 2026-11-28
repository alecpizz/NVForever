//! The ITEMS page (docs/spec-notes/pipboy.md §4): the player's inventory, one tab per
//! kind of item (Weapons, Apparel, Aid, Misc, Ammo), over the inventory model
//! (`menu::model`, `Mode::Inventory`).
//!
//! Lengths are the canvas's, from the menus' rectangle (`frame::MAIN`) as
//! `menus\main\inventory_menu.xml` gives them [documented] unless marked otherwise.
//!
//! - The heading's cards: Wg (carried weight / what the player can carry), HP, DT and
//!   Caps [observed: the owner's screenshots; documented: their places].
//! - The list: one row per stack, its name in font 2 with the count in brackets, a filled
//!   square before what is equipped; the selected row boxed.
//! - Keys: Up/Down (W/S) select, Enter or Space equip, unequip or use; the wheel
//!   scrolls; a left click on a row selects it, a left click on the selected row acts on
//!   it. A right click on a row drops one (a stack asks how many), Shift + right click
//!   the stack [design: owner 2026-11-02, #1393]. The keyboard has no drop key: X is the
//!   original's PC key for Mod (`IM_ModButton`), and X on the controller is its drop
//!   button (`IM_DropButton`'s `&xbuttonx;` [documented: inventory_menu.xml]), which
//!   drops one here when the controller works the controls.
//! - The scroll bar at the list's left, only while the list is longer than its window:
//!   the arrows at its ends and a line for the part shown [observed: the owner's scroll
//!   screenshot; the DATA lists' bar, `data::scroll_bar`].
//! - The selected (pointed at) item at the right: its picture (`IM_ItemIcon`, 450, 10,
//!   380 square) and its figures (`IM_ItemInfoRect`, 385, 340, 446 × 150, three cards a
//!   row of a third of it, 10 apart, a third of its height high, `item_stats_display.xml`)
//!   [documented: places]: [`info_tiles`].
//! - A weapon's skill badge (`IM_ItemIconBadge`, 390, 260, 64 square): the picture of
//!   the skill the weapon uses (`weap_skill_icon_*.dds`), and beside it the skill it
//!   needs (`IM_StrReq`, 440, 300, font 7, left), "0" too [documented: places; observed:
//!   the owner's ITEMS screenshots, Guns 0/25/50 under the bullets, the Throwing Spear's
//!   Melee Weapons 25 under the knife]. Not on the ITEMS page: DPS (the original shows
//!   none there, and the badge takes its place over DAM [observed]); the container and
//!   barter figures keep it ([`weapon_dps`]).
//! - "Mod X)" (`IM_ModButton`, `sInventoryMod`) on the Weapons tab, bright for a weapon
//!   with mod slots: the mod menu attaches and removes mods [documented: key; hypothesis:
//!   when it shows; design: always bright, the owner's rule 2026-10-27]. X on it opens the
//!   mod menu (`super::item_mod`, weapon-mods.md §5); elsewhere X does nothing, and the
//!   hint is dim (#1896).
//! - The hints at the top right, one under the other ([`page_hints`], #1896), by the input
//!   device. With a controller the original's whole column [observed: the original's
//!   screenshots on issue #1896, taken with a controller connected]: the equip action
//!   ("Equip", "Unequip" on what is worn, "Use" on aid; A), "Drop" (X), the repair action
//!   ("Repair" / "Maintain", Y, `super::repair`), "Hotkey" (RB, `super::hotkey_panel`)
//!   [design, #2312],
//!   and, on Weapons, "Mod" (LB). With the keyboard and mouse only "Repair R)" /
//!   "Maintain R)" on every tab and "Mod X)" under it on Weapons [observed 2026-11-09: the
//!   original's InventoryMenu button tiles read with keyboard and mouse on each ITEMS
//!   tab: the equip, drop and hotkey buttons hidden on every tab, the repair button shown
//!   on every tab, the mod button on Weapons only]; Enter, the right click and the number
//!   keys still do what they do, only their hints go. The set follows the device as it
//!   changes (the Controls page's Input Device). The first row at the Repair button's
//!   place (`IM_RepairButton`, y 45 [documented]), the rows `frame::HINT_PITCH` (46)
//!   apart, each word right-aligned at 855, the heading's right end, with its key after
//!   it (`frame::Ctx::hint_column`) [observed: the same screenshots: the first row just under the heading's cards, the
//!   words ending at the heading's right end, rows 0.90 of the list's row step apart;
//!   design: a column whose keys would run off the screen (a long key name) moves left as
//!   a whole]. They are drawn over the item's picture, whose documented box
//!   they share (as the original's words do), so the picture never hides them. A hint that
//!   does nothing for the selected item is drawn dim: repair on what nothing repairs,
//!   equip when the game would refuse it (`model::enter_acts`: misc, ammo, a book that
//!   teaches nothing, a broken weapon), hotkey on what a hotkey cannot hold, drop on a
//!   quest item [design]. The words are the settings `sInventoryEquip`, `sInventoryDrop`,
//!   `sInventoryHotKey`, else NVForever's labels for them (`fnv_ui::strings`); "Unequip"
//!   and "Use" are NVForever's own, no setting named.
//! - A modded weapon's figures are its modded ones (`WeaponStats::modded`).
//! - While a hotkey's key is held, the hotkey panel stands in the picture's place and a
//!   left click (or Enter) puts the row's item in that slot (`super::hotkey_panel`).

use super::frame::{self, Align, Card, Ctx, Hint, Kit, LINE, MAIN, THICK};
use super::hotkey_panel::HotkeyHold;
use crate::hotkeys::Hotkeys;
use super::labels::built_in_label;
use super::{Page, PageBody, PipBoy};
use crate::interface::{pipboy_amber as amber, at};
use crate::menu::model::{self, Category, Command, MenuModel, Mode, Row, Side};
use crate::menu::{ActiveMenu, MenuInput};
use crate::sim::SimState;
use bevy::image::CompressedImageFormatSupport;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use fnv_sim::FormId;
use nvf_assets::GameData;

/// The heading's cards: x and width inside the menus' rectangle.
const WG: (f32, f32) = (215.0, 155.0);
const HP: (f32, f32) = (380.0, 155.0);
const DT: (f32, f32) = (545.0, 110.0);
const CAPS: (f32, f32) = (665.0, 190.0);
/// The list: 15 left of the menus' rectangle, 75 down, 400 × 468; a row 32 in (past the
/// scroll bar) to 5 short of the list's right edge, as the DATA lists' (the Repair and
/// Mod pages' 393 is of their 430-wide lists; here it ran over the item's figures), its
/// name 40 in and 20 down, broken at 300; the equipped square 25 across at 15, 18.
const LIST: Rect = Rect { min: Vec2::new(-15.0, 75.0), max: Vec2::new(385.0, 543.0) };
const ROW_X: f32 = 32.0;
const ROW_W: f32 = 400.0 - ROW_X - 5.0;
const NAME_X: f32 = 40.0;
const NAME_Y: f32 = 20.0;
const WRAP: f32 = 300.0;
/// Width reserved for a ySI row icon when ySI is active (0 in vanilla).
const YSI_ICON_ADVANCE: f32 = 30.0;
const MARK: Rect = Rect { min: Vec2::new(15.0, 18.0), max: Vec2::new(40.0, 43.0) };
/// A row is 51 high and its name's further lines 35 below the first [hypothesis: as the
/// barter menu's rows, measured there, barter-menu.md §3].
pub const ROW_H: f32 = 51.0;
const LINE_PITCH: f32 = 35.0;

/// The category of each tab.
pub const TABS: [Category; 5] = [Category::Weapons, Category::Apparel, Category::Aid, Category::Misc, Category::Ammo];

/// A row's text: its name, and its count in brackets when more than one.
pub fn row_text(r: &Row) -> String {
    if r.count > 1 {
        format!("{} ({})", r.name, r.count)
    } else {
        r.name.clone()
    }
}

/// The item picture's box and the figures' rectangle, inside the menus' rectangle.
const ICON: Rect = Rect { min: Vec2::new(450.0, 10.0), max: Vec2::new(830.0, 390.0) };
const INFO: Rect = Rect { min: Vec2::new(385.0, 340.0), max: Vec2::new(831.0, 490.0) };
/// The condition meter inside its card: 62 in, 15 down, 20 high, 72 short of the card
/// [documented: item_stats_display.xml ConditionMeter].
const METER: (f32, f32, f32, f32) = (62.0, 15.0, 20.0, 72.0);
/// The weapon's skill badge and the skill it needs, font 7 [documented:
/// `IM_ItemIconBadge`, `IM_StrReq`].
const BADGE: Rect = Rect { min: Vec2::new(390.0, 260.0), max: Vec2::new(454.0, 324.0) };
const SKILL_REQ: Vec2 = Vec2::new(440.0, 300.0);
const SKILL_REQ_FONT: usize = 7;
/// The hints: the first row at the Repair button's place, the words right-aligned at its x
/// (the heading's right end), the rows `frame::HINT_PITCH` apart [documented:
/// `IM_RepairButton` at (855, 45); observed: the original's screenshots on issue #1896,
/// the first row (Equip) just under the heading's cards, the words ending at its right
/// end].
const HINTS: Vec2 = Vec2::new(855.0, 45.0);
/// The rows of the repair hint and, on Weapons, the mod hint in [`page_hints`] with the
/// labels `l`: with a controller the original's order Equip, Drop, Repair, Hot Key, Mod
/// [observed: the original's screenshots on issue #1896]; with the keyboard Repair, Mod
/// [observed 2026-11-09: the original's InventoryMenu button tiles read with keyboard and
/// mouse on each ITEMS tab].
pub fn hint_rows(l: crate::pad::Labels) -> (usize, usize) {
    if l == crate::pad::Labels::Keys {
        (0, 1)
    } else {
        (2, 4)
    }
}
/// The scroll bar: 32 wide, arrows 32 × 16 [documented: list_box.xml, the DATA lists].
const SCROLLBAR: f32 = 32.0;

/// Which figure the first card holds, apart from the words of its title: since #1902 the
/// title is the load order's setting (`sInventoryDamage`, `sInventoryDamageThreshold`),
/// localized, so code that places or reads the card goes by this, never by the title
/// [design].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirstStat {
    Damage,
    Threshold,
}

/// What the figures show of an item.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    /// The first card: a weapon's damage or armour's threshold, with its title and figure.
    pub first: Option<(FirstStat, &'static str, String)>,
    pub weight: Option<String>,
    pub value: Option<String>,
    /// A weapon's Strength requirement, over the value.
    pub strength: Option<u32>,
    /// Condition 0 to 1 of a weapon or armour.
    pub condition: Option<f32>,
    /// Beside the condition: a weapon's ammunition "20ga (2/30)", armour's class.
    pub beside: Option<String>,
    /// The effects' text ("HP +40", "AGL -1").
    pub effects: Option<String>,
    /// Omit the effects heading for ammo [design, #2502].
    pub ammo_effects: bool,
    /// A weapon's damage a second, over the DAM card.
    pub dps: Option<String>,
    /// A weapon's skill picture for the badge ([`skill_badge`]).
    pub badge: Option<String>,
    /// The skill the weapon needs, beside the badge.
    pub skill_req: Option<u32>,
    /// A weapon with mod slots: the "Mod X)" hint shows.
    pub mod_hint: bool,
}

/// The badge of a weapon's skill (WEAP DNAM skill, by its name) [documented: the six
/// `textures\interface\icons\typeicons\weap_skill_icon_*.dds` in Fallout - Textures;
/// observed: which shows for Guns and Melee Weapons on the owner's screenshots;
/// hypothesis: the other four by their names]. Other skills (Barter on binoculars) have
/// none.
pub fn skill_badge(skill: &str) -> Option<&'static str> {
    Some(match skill {
        "Guns" => "Interface\\Icons\\TypeIcons\\weap_skill_icon_sm_arms.dds",
        "Big Guns" => "Interface\\Icons\\TypeIcons\\weap_skill_icon_big_guns.dds",
        "Energy Weapons" => "Interface\\Icons\\TypeIcons\\weap_skill_icon_energy.dds",
        "Explosives" => "Interface\\Icons\\TypeIcons\\weap_skill_icon_explosives.dds",
        "Melee Weapons" => "Interface\\Icons\\TypeIcons\\weap_skill_icon_melee.dds",
        "Unarmed" => "Interface\\Icons\\TypeIcons\\weap_skill_icon_unarmed.dds",
        _ => return None,
    })
}

/// Damage a second of a weapon at `condition`: the damage a shot does at that condition
/// times its shots a second (the fire rate of an automatic weapon, else its attack shots a
/// second, else one) [hypothesis: the GECK's DPS calculator uses the damage and an
/// automatic weapon's fire rate, GECK wiki Weapons].
pub fn weapon_dps(w: &fnv_combat::data::WeaponStats, condition: f32) -> f32 {
    let per_second = if w.automatic && w.fire_rate > 0.0 {
        w.fire_rate
    } else if w.attack_shots_per_sec > 0.0 {
        w.attack_shots_per_sec
    } else {
        1.0
    };
    w.base_damage.max(0) as f32 * fnv_combat::damage::condition_mult(condition) * per_second
}

/// One card of the figures, canvas units inside the figures' rectangle.
#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    pub rect: Rect,
    pub title: String,
    pub value: String,
    /// The condition meter's fill.
    pub meter: Option<f32>,
}

/// The cards of the figures [documented: item_stats_display.xml: a card is a third of the
/// rectangle wide and high; damage or DT at the left of the first row, weight and value
/// 10 apart after it; the Strength requirement over the value; the second row the
/// condition and beside it the ammunition two cards and 10 wide; effects across the
/// three cards and 20 (hypothesis: on the first free row, as on the owner's apparel and
/// aid screenshots)].
pub fn info_tiles(i: &Info) -> Vec<Tile> {
    tiles_in(i, INFO.size())
}

/// [`info_tiles`] in a figures' rectangle of `size` (the container and barter menus'
/// is 530 × 120, `crate::menu::look`; the same `item_stats_display.xml`).
pub fn tiles_in(i: &Info, size: Vec2) -> Vec<Tile> {
    let (w, h) = (size.x / 3.0, size.y / 3.0);
    let col = |n: f32| n * (w + 10.0);
    let tile = |x: f32, y: f32, width: f32, title: &str, value: String| Tile { rect: Rect::new(x, y, x + width, y + h), title: title.into(), value, meter: None };
    let mut out = Vec::new();
    if let Some((_, title, v)) = &i.first {
        out.push(tile(0.0, 0.0, w, title, v.clone()));
    }
    out.push(tile(col(1.0), 0.0, w, built_in_label("sInventoryWeightUpper"), i.weight.clone().unwrap_or_else(|| "--".into())));
    out.push(tile(col(2.0), 0.0, w, built_in_label("sInventoryValue"), i.value.clone().unwrap_or_else(|| "--".into())));
    if let Some(s) = i.strength {
        out.push(tile(col(2.0), -h, w, built_in_label("sInventoryStrReq"), s.to_string()));
    }
    if let Some(d) = &i.dps {
        out.push(tile(0.0, -h, w, built_in_label("sInventoryDamagePerSecond"), d.clone()));
    }
    let mut row = 1.0;
    if i.condition.is_some() || i.beside.is_some() {
        if let Some(cnd) = i.condition {
            out.push(Tile { meter: Some(cnd.clamp(0.0, 1.0)), ..tile(0.0, h, w, built_in_label("sInventoryCondition"), String::new()) });
        }
        if let Some(b) = &i.beside {
            out.push(tile(col(1.0), h, 2.0 * w + 10.0, b, String::new()));
        }
        row = 2.0;
    }
    if let Some(e) = &i.effects {
        if i.ammo_effects {
            // Hand-built card text draws one line (`frame::Ctx::text`); give each
            // ammo effect its own row [design, #2502].
            for (n, line) in e.lines().enumerate() {
                out.push(tile(0.0, (row + n as f32) * h, 3.0 * w + 20.0, "", line.to_string()));
            }
        } else {
            out.push(tile(0.0, row * h, 3.0 * w + 20.0, built_in_label("sInventoryEffects"), e.clone()));
        }
    }
    out
}

/// A weight or value of a stack: "--" for nothing [observed: the Stimpak's WG "--"; its
/// VAL 675 is 9 × 75, so the figures are the stack's (hypothesis for the weight)].
fn figure(v: f32, decimals: bool) -> Option<String> {
    (v > 0.0).then(|| if decimals { format!("{v:.2}") } else { format!("{v:.0}") })
}

/// FULL wins; fallback words and number formatting are ours [design, #2502].
/// Operations follow `fnv_combat::damage::apply_effects` and `schemas/fnv/combat.json`.
fn ammo_effect_line(e: &fnv_combat::data::AmmoEffect) -> Option<String> {
    use fnv_util::words::tr;
    let value = match e.operation.as_deref()? {
        "Multiply" => format!("x{:.2}", e.value),
        "Add" => format!("{:+}", e.value),
        "Subtract" => format!("{:+}", -e.value),
        _ => return None,
    };
    let name = e.name.as_deref().filter(|n| !n.trim().is_empty()).map(str::to_string).unwrap_or_else(|| {
        tr(match e.kind.as_deref() {
            Some("Damage Mod") => "Damage",
            Some("DR Mod") => "Damage resist",
            Some("DT Mod") => "Damage threshold",
            Some("Spread Mod") => "Spread",
            Some("Weapon Condition Mod") => "Weapon wear",
            Some("Fatigue Mod") => "Fatigue",
            _ => "Ammo effect",
        }).to_string()
    });
    Some(format!("{name} {value}"))
}

/// RCIL order, resolved through the loaded combat forms [design, #2502; data.rs].
fn ammo_card_effects(i: &mut Info, item: FormId, forms: &fnv_combat::data::CombatForms) {
    i.ammo_effects = true;
    let lines: Vec<String> = forms.ammo.get(&item).into_iter().flat_map(|a| &a.effects)
        .filter_map(|id| forms.ammo_effects.get(id)).filter_map(ammo_effect_line).collect();
    i.effects = (!lines.is_empty()).then(|| lines.join("\n"));
}

/// What the figures show of a row, from the records (the ITEMS page's, and the
/// container and barter menus' item figures, `crate::menu::look`). An aid item's effects
/// are those that would apply to the player now (`fnv_game::Game::aid_preview`: the
/// effects' conditions, one line per value), read from the records only without the
/// scripts' game.
pub fn info_of(r: &Row, combat: Option<&crate::combat::Combat>, sim: &mut SimState, forms: Option<&mut super::stats::read::Forms>, mut script: Option<&mut crate::script::ScriptState>) -> Info {
    use fnv_sim::forms::ItemKind;
    let tweaks = script.as_ref().and_then(|s| s.rt.as_ref()).map(|rt| rt.game.tweaks.clone()).unwrap_or_default();
    // The player's tweaks: one item's figures, and none for quest items and keys [design: docs/design/tweaks.md].
    let n = if tweaks.single_item_figures { 1.0 } else { r.count.max(1) as f32 };
    let hidden = tweaks.hide_quest_item_figures && (r.quest || r.kind == Some(ItemKind::Key));
    let mut i = Info {
        weight: r.weight.filter(|_| !hidden).and_then(|w| figure(w * n, true)),
        value: r.value.filter(|_| !hidden).and_then(|v| figure(v as f32 * n, false)),
        condition: r.condition.filter(|_| matches!(r.kind, Some(ItemKind::Weapon | ItemKind::Armor))),
        ..Info::default()
    };
    let cf = combat.map(|c| &c.forms);
    match r.kind {
        Some(ItemKind::Ammo) => {
            if let Some(f) = cf {
                ammo_card_effects(&mut i, r.item, f);
            }
            return i;
        }
        Some(ItemKind::Weapon) => {
            if let Some(w) = cf.and_then(|f| f.weapons.get(&r.item)).map(|w| w.modded(r.mods)) {
                let w = &w;
                // Binoculars and the like are weapons without damage: no DAM or STR
                // [hypothesis].
                i.first = (w.base_damage > 0).then(|| (FirstStat::Damage, built_in_label("sInventoryDamage"), w.base_damage.to_string()));
                i.strength = (w.strength_req > 0).then_some(w.strength_req);
                i.badge = w.skill.as_deref().and_then(skill_badge).map(str::to_string);
                i.skill_req = i.badge.is_some().then_some(w.skill_req);
                if w.base_damage > 0 {
                    i.dps = Some(format!("{:.0}", weapon_dps(w, r.condition.unwrap_or(1.0))));
                }
                // The player's tweak compares with the weapon in hand, both whole [design: docs/design/tweaks.md].
                if tweaks.compare_weapons && w.base_damage > 0 {
                    let held = sim.world.equipment(FormId::PLAYER_REF).unwrap_or_default().into_iter().find(|&e| sim.forms.item(e).is_some_and(|it| it.kind == ItemKind::Weapon));
                    if let Some(cur) = held.filter(|&e| e != r.item).and_then(|e| cf.and_then(|f| f.weapons.get(&e))) {
                        let dam = i64::from(w.base_damage) - i64::from(cur.base_damage);
                        let dps = weapon_dps(w, 1.0) - weapon_dps(cur, 1.0);
                        if let Some((_, _, v)) = i.first.as_mut() {
                            *v = format!("{v} ({dam:+})");
                        }
                        if let Some(v) = i.dps.as_mut() {
                            *v = format!("{v} ({dps:+.0})");
                        }
                    }
                }
                // Always bright: the mod menu also takes mods off [design: the owner's
                // rule, 2026-10-27].
                i.mod_hint = w.mods.iter().any(|m| m.item.is_some());
                let f = cf.expect("weapon stats came from it");
                let choices = f.ammo_choices(w);
                let carried: Vec<i32> = choices.iter().map(|&a| sim.world.item_count(FormId::PLAYER_REF, a).unwrap_or(0)).collect();
                let pick = choices.iter().zip(&carried).find(|(_, n)| **n > 0).or(choices.iter().zip(&carried).next());
                if let Some((&ammo, &count)) = pick {
                    let a = &f.ammo[&ammo];
                    let name = a.short_name.clone().or_else(|| a.name.clone()).unwrap_or_default();
                    // In the clip: as much as a clip holds of what is carried [hypothesis:
                    // "(2/30)" on the owner's screenshot reads as clip/carried].
                    let clip = (w.clip_size as i32).min(count).max(0);
                    // The player's tweak: rounds a shot when more than one [design: docs/design/tweaks.md].
                    let per_shot = if tweaks.show_weapon_ammo_use && w.ammo_use > 1 { format!(" x{}", w.ammo_use) } else { String::new() };
                    // The player's tweak marks other ammo carried [design: docs/design/tweaks.md].
                    let other = if tweaks.mark_other_ammo && carried.iter().filter(|n| **n > 0).count() > 1 { "+" } else { "" };
                    i.beside = Some(format!("{name}{per_shot} ({clip}/{count}){other}"));
                }
            }
        }
        Some(ItemKind::Armor) => {
            if let Some(a) = cf.and_then(|f| f.armor.get(&r.item)) {
                i.first = Some((FirstStat::Threshold, built_in_label("sInventoryDamageThreshold"), format!("{:.0}", a.dt)));
                // The player's tweak compares with the worn pieces in the same slots, which
                // it would replace (DR is 0 on New Vegas armour, so DT only) [design: docs/design/tweaks.md].
                let worn = sim.world.equipment(FormId::PLAYER_REF).unwrap_or_default();
                if tweaks.compare_armor && a.slots != 0 && !worn.contains(&r.item) {
                    let replaced: f32 = worn.iter().filter_map(|e| cf.and_then(|f| f.armor.get(e))).filter(|o| o.slots & a.slots != 0).map(|o| o.dt).sum();
                    if let Some((_, _, v)) = i.first.as_mut() {
                        *v = format!("{v} ({:+.0})", a.dt - replaced);
                    }
                }
            }
            i.beside = sim.forms.item(r.item).map(|it| format!("{:?}", it.armor_class));
        }
        _ => {}
    }
    if let Some(forms) = forms {
        let preview = script
            .as_mut()
            .and_then(|s| s.rt.as_mut())
            .filter(|_| r.kind == Some(ItemKind::Aid))
            .map(|rt| crate::script::with_world(rt, sim, |rt| (rt.game.aid_preview(FormId::PLAYER_REF, r.item), rt.game.aid_preview_names(FormId::PLAYER_REF, r.item))));
        if let Some((lines, named)) = preview {
            let changes: Vec<super::stats::model::Change> = lines.into_iter().filter(|(_, amount)| amount.abs() >= 0.5).map(|(code, amount)| super::stats::model::Change { value: forms.short_name(code), amount: amount.abs(), detrimental: amount < 0.0 }).collect();
            // Script effects shown by name (Psycho's "Damage +25%", Cateye's "Low-Light
            // Vision") after the values (#3119).
            let text = super::stats::read::with_names(super::stats::model::changes_text(&changes), named.iter().map(|m| forms.text(m.0).name.clone()));
            i.effects = (!text.is_empty()).then_some(text);
            return i;
        }
        // The player's tweak: a skill book says what it teaches [design: docs/design/tweaks.md]: the skill and the
        // points it would give now (the perks' Adjust Book Skill Points applied).
        if tweaks.show_book_effects && r.kind == Some(ItemKind::Book) {
            if let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) {
                if let Some(skill) = rt.game.book_skill(r.item) {
                    let base = rt.game.world.setting_f32("fBookPerkBonus").map_or(fnv_game::books::DEFAULT_BOOK_POINTS, f64::from);
                    let (worth, _) = rt.game.entry_point(FormId::PLAYER_REF, fnv_game::perks::ADJUST_BOOK_SKILL_POINTS, base);
                    i.effects = Some(format!("Permanent {} +{}", fnv_game::chargen::skill_name(&rt.game, skill), worth.trunc()));
                    return i;
                }
            }
        }
        // Aid items carry their effects; apparel and weapons their enchantment's.
        let ench = if r.kind == Some(ItemKind::Aid) { Some(r.item.0) } else { forms.item_ench(r.item.0) };
        if let Some(e) = ench.map(|e| forms.ench(e)).filter(|e| !e.changes.is_empty()) {
            let changes: Vec<super::stats::model::Change> = e.changes.iter().map(|&(code, amount, detrimental)| super::stats::model::Change { value: forms.short_name(code), amount, detrimental }).collect();
            i.effects = Some(super::stats::model::changes_text(&changes));
        }
        // The player's Show Weapon Poison Effects tweak: a poisoned weapon's poison, its hits
        // left and its effects after the weapon's own (`fnv_game::weapon_poison`) [design:
        // docs/design/tweaks.md, our own words].
        let poison = script.as_ref().and_then(|s| s.rt.as_ref()).filter(|rt| rt.game.tweaks.show_poison_effects && r.kind == Some(ItemKind::Weapon)).and_then(|rt| rt.game.poison_on(r.item).map(|(p, hits)| (rt.game.data.name(p), p, hits)));
        if let Some((name, p, hits)) = poison {
            let e = forms.ench(p.0);
            let changes: Vec<super::stats::model::Change> = e.changes.iter().map(|&(code, amount, detrimental)| super::stats::model::Change { value: forms.short_name(code), amount, detrimental }).collect();
            let line = if changes.is_empty() { format!("{name} ({hits} hits)") } else { format!("{name} ({hits} hits): {}", super::stats::model::changes_text(&changes)) };
            i.effects = Some(match i.effects.take() {
                Some(own) => format!("{own}, {line}"),
                None => line,
            });
        }
    }
    i
}

/// A clickable row of the list.
#[derive(Component, Clone, Copy)]
struct RowButton(usize);

/// The ITEMS page's keys and wheel, while it is shown.
#[allow(clippy::too_many_arguments)]
pub fn keys(
    pip: Res<PipBoy>,
    (repair, modding): (Res<super::repair::PipRepair>, Res<super::item_mod::PipItemMod>),
    active: Res<ActiveMenu>,
    keys: crate::pad::ActionButtons<KeyCode>,
    (mut wheel,touch_wheel,physical): (MessageReader<MouseWheel>,Option<Res<super::touch_scroll::WheelFrame>>,Option<Res<super::touch_scroll::PhysicalPriority>>),
    mut lines: Local<f32>,
    mut out: MessageWriter<MenuInput>,
    (mut hold, mut hotkeys): (ResMut<HotkeyHold>, ResMut<Hotkeys>),
    (pads, bindings): (crate::pad::ActionPads, Option<Res<crate::bindings::Bindings>>),
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
    taken: Res<super::KeysTaken>,
) {
    let keys = keys.snapshot();
    let deselect = tweaks.as_ref().is_some_and(|t| t.deselect_hotkeys);
    let drop_key = tweaks.as_ref().is_some_and(|t| t.drop_key);
    // Laid out in the Pip-Boy's screen texture, at its size whatever the UI scale (#1631).
    let _canvas = crate::interface::original_units();
    use KeyCode::*;
    // [design, #2553] Repair/mod keys stay theirs through the frame that closes the list.
    let items = pip.open() && pip.page == Page::Items && !taken.taken() && !repair.is_open() && !modding.is_open() && active.0.as_ref().is_some_and(|m| m.mode == Mode::Inventory);
    let mut send = |c| {
        out.write(MenuInput(c));
    };
    if !items {
        wheel.clear();
        return;
    }
    // The model shows the tab's kind of item.
    let want = TABS[pip.tab()];
    if active.0.as_ref().is_some_and(|m| m.category[Side::Player as usize] != want) {
        send(Command::SetCategory(Side::Player, want));
    }
    // The order the player's tweak chose (`super::PipBoy::item_sort`).
    if active.0.as_ref().is_some_and(|m| m.sort != pip.item_sort) {
        send(Command::SetSort(pip.item_sort));
    }
    for (k, c) in [([ArrowUp, KeyW], Command::Up), ([ArrowDown, KeyS], Command::Down), ([PageUp, PageUp], Command::PageUp), ([PageDown, PageDown], Command::PageDown)] {
        if keys.any_just_pressed(k) {
            send(c);
        }
    }
    if keys.any_just_pressed([Enter, NumpadEnter, Space]) {
        // With a hotkey's key held, Enter puts the selected item in its slot.
        match hold.slot {
            Some(slot) => {
                if let Some(r) = active.0.as_ref().and_then(|m| m.selected_row(Side::Player)) {
                    hold.said = Some(super::hotkey_panel::assign_row(&mut hotkeys, slot, r, deselect));
                }
            }
            None => send(Command::One),
        }
    }
    // Backspace leaves an open Misc folder (the "Back" row does too) [design].
    if keys.just_pressed(Backspace) {
        send(Command::Back);
    }
    // The controller's X drops one (module notes); the keyboard's X is the mod menu's. Not
    // while "How many?" is up (the model would ignore it there too).
    let asking = active.0.as_ref().is_some_and(|m| m.quantity.is_some());
    if !asking && crate::pad::menu_button(&pads, bindings.as_deref(), GamepadButton::West) {
        send(Command::Drop);
    }
    // The player's tweak: Q drops the selected item too [design: docs/design/tweaks.md].
    // With `quantity-select-short-keys` enabled (#3799), Shift drops the whole stack and
    // Ctrl drops a single item without opening the quantity prompt [design].
    if !asking && drop_key && keys.just_pressed(KeyCode::KeyQ) {
        let short_keys = active.0.as_ref().is_some_and(|m| m.qtt_select_short_keys);
        let shift = short_keys && keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        let ctrl = short_keys && keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
        send(if shift { Command::DropStack } else if ctrl { Command::DropSingle } else { Command::Drop });
    }
    for w in wheel.read() {
        *lines += match w.unit {
            MouseScrollUnit::Line => w.y,
            MouseScrollUnit::Pixel => w.y / 20.0,
        };
    }
    if physical.as_ref().is_none_or(|p|!p.active) {*lines += touch_wheel.as_ref().map_or(0.0,|w|w.lines(&pip));}
    let whole = lines.trunc();
    if whole != 0.0 {
        send(Command::Scroll(-whole as i32));
        *lines -= whole;
    }
}

/// The row the pointer comes onto takes the selection, if the pointer moved onto it
/// (`super::hover_takes`: the list redrawn under a resting pointer as the keys or the D-pad
/// step it leaves their choice alone, #2310) and the wheel does not hold it
/// (`crate::menu::WheelHold`: a list redrawn under a resting pointer would undo the wheel).
fn on_row_over(over: On<PointerOver>, rows: Query<&RowButton>, active: Res<ActiveMenu>, hold: Option<Res<crate::menu::WheelHold>>, moved: Option<Res<super::PointerMoved>>, mut out: MessageWriter<MenuInput>) {
    if hold.is_some_and(|h| h.held()) || !super::hover_takes(moved.as_deref()) {
        return;
    }
    let (Ok(r), Some(m)) = (rows.get(over.entity), active.0.as_ref()) else { return };
    if m.selected[Side::Player as usize] != r.0 {
        out.write(MenuInput(Command::Hover(Side::Player, r.0)));
    }
}

/// A left click selects the row (and on the selected row acts on it); with a hotkey's key
/// held, it puts the row's item in that slot instead (`super::hotkey_panel`). A right
/// click selects the row without acting and drops one, with Shift the stack (module notes).
fn on_touch_row_press(press: On<PointerPress>, rows: Query<&RowButton>, active: Res<ActiveMenu>, mut touch: Option<ResMut<super::touch_items::TouchItems>>, admission: super::touch_items::Admission) {
    if press.pointer.id!=super::touch_items::POINTER || press.button!=PointerButton::Primary || !admission.allows() {return;}
    if let (Some(touch),Ok(row),Some(menu))=(touch.as_deref_mut(),rows.get(press.entity),active.0.as_ref()) {touch.capture_row(menu,row.0);}
}

#[cfg(test)]
pub(super) fn binding_fixture_row(world: &mut World, index: usize) -> Entity {
    world.spawn(RowButton(index)).observe(on_row_over).observe(on_touch_row_press).observe(on_row_click).id()
}
fn on_row_click(
    click: On<PointerClick>,
    (rows, active): (Query<&RowButton>, Res<ActiveMenu>),
    keys: crate::pad::ActionButtons<KeyCode>,
    (mut hold, mut hotkeys): (ResMut<HotkeyHold>, ResMut<Hotkeys>),
    mut out: MessageWriter<MenuInput>,
    (touch,admission): (Option<Res<super::touch_items::TouchItems>>,super::touch_items::Admission),
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
) {
    if click.pointer.id==super::touch_items::POINTER && (!admission.allows() || touch.as_ref().is_some_and(|t|t.suppress_click())) {return;}
    let keys = keys.snapshot();
    let deselect = tweaks.as_ref().is_some_and(|t| t.deselect_hotkeys);
    if hold.controller.is_some() {
        return;
    }
    // The player's tweak: a right click does not drop [design: docs/design/tweaks.md].
    let no_right_drop = tweaks.as_ref().is_some_and(|t| t.no_right_click_drop);
    let Ok(r) = rows.get(click.entity) else { return };
    if click.button == PointerButton::Secondary && !no_right_drop {
        if hold.slot.is_none() {
            let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
            let ctrl = active.0.as_ref().is_some_and(|m| m.qtt_select_short_keys) && keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
            out.write(MenuInput(Command::Hover(Side::Player, r.0)));
            out.write(MenuInput(if shift { Command::DropStack } else if ctrl { Command::DropSingle } else { Command::Drop }));
        }
        return;
    }
    match hold.slot {
        Some(slot) if click.button == PointerButton::Primary => {
            if let Some(row) = active.0.as_ref().and_then(|m| m.list(Side::Player).rows.get(r.0)) {
                hold.said = Some(super::hotkey_panel::assign_row(&mut hotkeys, slot, row, deselect));
            }
        }
        Some(_) => {}
        None => {
            out.write(MenuInput(Command::Select(Side::Player, r.0)));
        }
    }
}

type ItemsDrawState<'w, 's> = (Res<'w, PipBoy>, Local<'s, String>, Local<'s, Option<crate::pad::Labels>>, Option<ResMut<'w, super::scrollbar::ItemsScroll>>);

/// Cached heading figures and hotkey-panel icons from the last draw of the ITEMS page.
#[derive(Default)]
pub struct DrawnState {
    heading: Option<Heading>,
    panel_icons: Option<super::hotkey_panel::PanelIcons>,
}

/// Fill the ITEMS page's body when it is new or the inventory changed.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut commands: Commands,
    bodies: Query<(Entity, &PageBody)>,
    added: Query<(), Added<PageBody>>,
    mut active: ResMut<ActiveMenu>,
    mut sim: ResMut<SimState>,
    mut kit: ResMut<Kit>,
    data: Option<Res<GameData>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    glyphs: Res<crate::pad::PadGlyphs>,
    combat: Option<Res<crate::combat::Combat>>,
    play: Option<Res<crate::PlayWorld>>,
    mut forms: Local<Option<super::stats::read::Forms>>,
    (repair, modding, mut script): (Res<super::repair::PipRepair>, Res<super::item_mod::PipItemMod>, Option<ResMut<crate::script::ScriptState>>),
    (hold, hotkeys, mut drawn): (Res<HotkeyHold>, Res<Hotkeys>, Local<DrawnState>),
    (pip, mut logged, mut drawn_with, mut scroll): ItemsDrawState<'_, '_>,
) {
    // Laid out in the Pip-Boy's screen texture, at its size whatever the UI scale (#1631).
    let _canvas = crate::interface::original_units();
    let Some((body, _)) = bodies.iter().find(|(_, b)| b.page == Page::Items) else { return };
    // The hints' set and keys follow the input device: a change of labels draws again.
    let labels = crate::pad::labels();
    let relabelled = drawn_with.replace(labels) != Some(labels);
    // The heading's HP and DT, read every frame the page is up: an aid item used from the
    // list heals a frame after the list changed (the scripts run before the menu), and a
    // heal over time or a wear change touches no menu state, so the page draws again when
    // they differ from what it drew (#3120), as the STATS page does with its snapshot.
    let shown = active.0.as_ref().is_some_and(|m| m.mode == Mode::Inventory) && modding.0.is_none() && repair.0.is_none();
    let now = shown.then(|| heading(sim.bypass_change_detection(), script.as_mut().map(|s| s.bypass_change_detection()), combat.as_deref()));
    let refigured = now.is_some() && drawn.heading != now;
    let now_panel_icons = (shown && hold.slot.is_some()).then(|| {
        let selected_now = active.0.as_ref().and_then(|m| m.list(Side::Player).rows.get(m.selected[Side::Player as usize]));
        let script_ref = script.as_mut().map(|s| &*s.bypass_change_detection());
        super::hotkey_panel::panel_icons(&hotkeys, &hold, selected_now, sim.bypass_change_detection(), combat.as_deref(), script_ref)
    });
    let reiconed = now_panel_icons.is_some() && drawn.panel_icons != now_panel_icons;
    if now_panel_icons.is_none() {
        drawn.panel_icons = None;
    }
    if added.get(body).is_err() && !relabelled && !refigured && !reiconed && !active.is_changed() && !repair.is_changed() && !modding.is_changed() && !hold.is_changed() && !hotkeys.is_changed() && scroll.as_ref().is_none_or(|s| !s.is_changed()) {
        return;
    }
    if forms.is_none() {
        *forms = play.as_ref().map(|p| super::stats::read::Forms::new(p.lo, p.schema));
    }
    // The mod menu in place of the page's body (`super::item_mod`).
    if let Some(s) = modding.0.as_ref() {
        let chosen = s.selected.checked_sub(1).and_then(|i| s.offer.choices.get(i));
        let desc = chosen.zip(forms.as_mut()).map(|(ch, f)| f.text(ch.item.0).description.clone());
        let mut c = Ctx::new(&mut kit, data.as_deref(), &mut images, formats.as_deref(), &glyphs);
        let line = format!("mod menu: {}", frame::hints_line(&super::item_mod::menu_hints(s)));
        if *logged != line {
            info!("pipboy hints: {line}");
            *logged = line;
        }
        commands.entity(body).despawn_children().with_children(|p| super::item_mod::draw_list(p, &mut c, s, combat.as_deref(), desc.as_deref()));
        return;
    }
    // The repair list in place of the page's body (`super::repair`).
    if let Some(s) = repair.0.as_ref() {
        let mut c = Ctx::new(&mut kit, data.as_deref(), &mut images, formats.as_deref(), &glyphs);
        commands.entity(body).despawn_children().with_children(|p| super::repair::draw_list(p, &mut c, s, combat.as_deref()));
        return;
    }
    let Some(m) = active.0.as_ref().filter(|m| m.mode == Mode::Inventory) else { return };
    let sim = sim.bypass_change_detection();
    let mut script = script.as_mut().map(|s| s.bypass_change_detection());
    let caps = sim.world.item_count(FormId::PLAYER_REF, model::CAPS).unwrap_or(0);
    let selected = m.list(Side::Player).rows.get(m.selected[Side::Player as usize]);
    // No DPS on this page: the skill badge stands over DAM (`item_info`).
    let info = selected.map(|r| (r.icon.clone(), Info { dps: None, ..info_of(r, combat.as_deref(), sim, forms.as_mut(), script.as_deref_mut()) }));
    if let (Some(r), Some((_, i))) = (selected, &info) {
        info!("pipboy items: {} selected: {:?}", r.name, i);
    }
    let (hp, dt) = now.unwrap_or_else(|| heading(sim, script.as_deref_mut(), combat.as_deref()));
    drawn.heading = Some((hp.clone(), dt.clone()));
    drawn.panel_icons = now_panel_icons;
    let offer = super::repair::current(&active, sim, script.as_deref());
    // The tab's hints for the selected row (the hotkey panel hides them).
    let tab = TABS[pip.tab().min(TABS.len() - 1)];
    // Whether Enter does anything to the selected row: the game's own checks for it.
    let enter = selected.is_some_and(|r| model::enter_acts(&sim.world, r));
    let hints = page_hints(tab, selected, super::repair::hint_word(offer.as_ref()), info.as_ref().is_some_and(|(_, i)| i.mod_hint), enter);
    let panel = hold.slot.is_some();
    if !panel {
        let line = format!("items {tab:?}: {}", frame::hints_line(&hints));
        if *logged != line {
            info!("pipboy hints: {line}");
            *logged = line;
        }
    }
    let mut c = Ctx::new(&mut kit, data.as_deref(), &mut images, formats.as_deref(), &glyphs);
    // The rows' heights and the first row drawn: the selection is always drawn, rows of
    // two lines taking more room (owner's playtest list 7 item 15).
    let heights = row_heights(&mut c, m);
    let first = scroll.as_mut().map(|s| s.bypass_change_detection().resolve(m, &heights, LIST.height())).unwrap_or_else(|| {
        if m.pointer_pick { m.scroll[Side::Player as usize].min(heights.len()) } else { first_shown(&heights, m.scroll[Side::Player as usize], m.selected[Side::Player as usize], LIST.height()) }
    });
    if active.is_changed() || refigured {
        info!("pipboy items: heading HP {hp}, DT {dt}; rows {first}.. of {} drawn", heights.len());
    }
    commands.entity(body).despawn_children().with_children(|p| {
        let cards = [
            Card::new(WG.0, WG.1, built_in_label("sInventoryWeight"), wg_text(m)),
            Card::new(HP.0, HP.1, built_in_label("sHitPointsShort"), hp),
            Card::new(DT.0, DT.1, built_in_label("sInventoryDamageThreshold"), dt),
            Card::new(CAPS.0, CAPS.1, built_in_label("sInventoryCaps"), caps.to_string()),
        ];
        frame::heading(p, &mut c, Page::Items.title(), &cards);
        list(p, &mut c, m, first, &heights, pip.tab());
        // The hotkey panel hides the picture, the badge and the hints (`IM_ButtonRect`).
        if let (true, Some(k)) = (panel, hold.slot) {
            info!("pipboy hotkeys: panel drawn, key {} chosen{}", k + 1, hold.said.as_ref().map_or_else(String::new, |s| format!(", says {s:?}")));
            super::hotkey_panel::draw(p, &mut c, &hotkeys, &hold, selected, sim, combat.as_deref(), script.as_deref());
        } else {
            // The selected item's own lines for its Repair and Mod hints.
            let (repair_row, mod_row) = hint_rows(labels);
            if let (Some(o), Some(h)) = (offer.as_ref(), hints.get(repair_row)) {
                info!("pipboy items: {} {}{}", o.name, h.text(), if h.dim { " (dim)" } else { "" });
            }
            if let (Some(r), Some(h)) = (selected, hints.get(mod_row).filter(|h| tab == Category::Weapons && !h.dim)) {
                info!("pipboy items: {} {}", r.name, h.text());
            }
        }
        if let Some((icon, i)) = &info {
            if panel {
                item_info(p, &mut c, None, &Info { badge: None, skill_req: None, ..i.clone() });
            } else {
                item_info(p, &mut c, icon.as_deref(), i);
            }
        }
        // Every hint in its place, after the picture so that it is drawn over it (the
        // column shares the picture's box: module notes).
        if !panel {
            c.hint_column(p, &hints, MAIN.min.x + HINTS.x, MAIN.min.y + HINTS.y + frame::BUTTON_TEXT_Y);
        }
    });
    // The model keeps the first row drawn as its scroll position, so a row the pointer
    // picks later does not move the list back (as the container and barter menus do).
    if first != m.scroll[Side::Player as usize] {
        if let Some(m) = active.bypass_change_detection().0.as_mut() {
            m.scroll[Side::Player as usize] = first;
        }
    }
}

/// The ITEMS page's hints on the tab of `tab`, top to bottom, for the selected row
/// (module notes, #1896), by the labels in use (`crate::pad::labels`; rows:
/// [`hint_rows`]). With a controller, in the original's order: equip (`enter`: whether
/// Enter does anything to the row, `crate::menu::model::enter_acts`), drop, the repair
/// action (`repair`: its word and whether R does anything, `super::repair::hint_word`),
/// hotkey, and on Weapons the mod action (`mod_slots`: the selected weapon has mod slots).
/// With the keyboard and mouse only the repair action and, on Weapons, the mod action.
pub fn page_hints(tab: Category, row: Option<&Row>, repair: (&'static str, bool), mod_slots: bool, enter: bool) -> Vec<Hint> {
    use fnv_sim::forms::ItemKind;
    let at = crate::pad::at_menu(fnv_game::game::menus::INVENTORY);
    let key = |k: KeyCode| crate::pad::hint(&[k], at);
    let item = row.filter(|r| r.folder.is_none());
    // Enter equips or takes off (`menu::model`'s Command::One), or uses an aid item or a
    // skill book; whether the game takes it is `enter` (the checks `World::equip_at` and
    // `World::unequip` run: a book that teaches nothing, a broken weapon, misc and ammo are
    // refused). A hotkey holds weapons, apparel and aid (`crate::hotkeys::can_hold`).
    let holdable = item.and_then(|r| r.kind).is_some_and(crate::hotkeys::can_hold);
    let equip = match item {
        Some(r) if matches!(r.kind, Some(ItemKind::Aid | ItemKind::Book)) => USE,
        Some(r) if r.equipped => UNEQUIP,
        _ => super::labels::built_in_label("sInventoryEquip"),
    };
    let droppable = item.is_some_and(|r| !r.quest && r.kind != Some(ItemKind::CaravanCard));
    let repair = Hint::new(repair.0, key(KeyCode::KeyR), !repair.1);
    let mut out = if crate::pad::labels() == crate::pad::Labels::Keys {
        vec![repair]
    } else {
        vec![
            Hint::new(equip, key(KeyCode::Enter), !(enter && item.is_some())),
            Hint::new(super::labels::built_in_label("sInventoryDrop"), drop_key(), !droppable),
            repair,
            Hint::new(super::labels::built_in_label("sInventoryHotKey"), hotkey_button(), !holdable),
        ]
    };
    if tab == Category::Weapons {
        // X, with a controller LB (`inventory_menu.xml`'s, `crate::pad::menu_buttons`).
        out.push(Hint::new(built_in_label("sInventoryMod"), key(KeyCode::KeyX), !mod_slots));
    }
    out
}

/// The equip hint's words on an aid item or a book, and on what is worn: NVForever's own,
/// no game setting is named for them [design, #1896].
const USE: &str = "Use";
const UNEQUIP: &str = "Unequip";

/// [design, #2312] The existing hint/glyph path names RB (or R1) for the panel.
pub fn hotkey_button() -> String {
    format!("[{}]", crate::pad::pad_label(crate::pad::PadInput::Button(GamepadButton::RightTrigger), crate::pad::labels()))
}

/// The drop hint's key: a right click on the keyboard and mouse (owner 2026-11-02, #1393),
/// with a controller its X (`IM_DropButton`'s `&xbuttonx;` [documented:
/// inventory_menu.xml]), which [`keys`] reads itself.
fn drop_key() -> String {
    use crate::pad::{labels, pad_label, Labels, PadInput};
    match labels() {
        Labels::Keys => format!("[{}]", crate::bindings::mouse_name(MouseButton::Right)),
        l => format!("[{}]", pad_label(PadInput::Button(GamepadButton::West), l)),
    }
}

/// The selected item's picture and figures.
fn item_info(p: &mut ChildSpawnerCommands, c: &mut Ctx, icon: Option<&str>, i: &Info) {
    let bright = c.bright();
    if let Some(icon) = icon {
        let r = Rect::from_corners(MAIN.min + ICON.min, MAIN.min + ICON.max);
        c.image(p, icon, at(r.min.x, r.min.y, r.width(), r.height()), bright);
    }
    if let Some(b) = i.badge.as_deref() {
        let r = Rect::from_corners(MAIN.min + BADGE.min, MAIN.min + BADGE.max);
        c.image(p, b, at(r.min.x, r.min.y, r.width(), r.height()), bright);
    }
    if let Some(req) = i.skill_req {
        c.text(p, SKILL_REQ_FONT, &req.to_string(), MAIN.min.x + SKILL_REQ.x, MAIN.min.y + SKILL_REQ.y, Align::Left, bright);
    }
    let o = MAIN.min + INFO.min;
    for t in info_tiles(i) {
        let (x, y, w) = (o.x + t.rect.min.x, o.y + t.rect.min.y, t.rect.width());
        frame::card(p, c, x, y, w, &t.title, &t.value);
        if let Some(fill) = t.meter {
            let (mx, my, mh, short) = METER;
            let mw = w - short;
            p.spawn((at(x + mx, y + my, mw, mh), BackgroundColor(bright.with_alpha(0.15))));
            p.spawn((at(x + mx, y + my, mw * fill, mh), BackgroundColor(bright)));
        }
    }
}

/// The HP card's figure: health out of its whole, "--" unread.
pub(crate) fn hp_text(hp: Option<(f64, f64)>) -> String {
    hp.map_or_else(|| "--".to_string(), |(v, max)| format!("{}/{}", super::stats::model::whole(v), super::stats::model::whole(max)))
}

/// The heading's HP and DT figures as drawn.
type Heading = (String, String);

/// The heading's HP and DT figures: the STATS page's health, and the damage threshold a hit
/// on the player meets (worn armour at its condition and the DT actor value,
/// `crate::combat::target_armor`) [observed: "HP 205/205", "DT 8.0" on the owner's
/// screenshots; hypothesis: the figure is the one combat uses].
pub(crate) fn heading(sim: &mut SimState, mut script: Option<&mut crate::script::ScriptState>, combat: Option<&crate::combat::Combat>) -> Heading {
    let hp = super::stats::read::player_hp(sim, script.as_deref_mut());
    let dt = combat.map(|c| crate::combat::target_armor(c, script, sim, FormId::PLAYER_REF).0);
    (hp_text(hp), dt_text(dt))
}

/// The DT card's figure: one decimal, "--" unread.
pub(crate) fn dt_text(dt: Option<f32>) -> String {
    dt.map_or_else(|| "--".to_string(), |v| format!("{v:.1}"))
}

/// The Wg card's figure: carried weight out of what the player can carry, "--" unread.
pub(crate) fn wg_text(m: &MenuModel) -> String {
    match &m.encumbrance {
        Ok(e) => format!("{:.0}/{:.0}", e.weight, e.capacity),
        Err(_) => "--".into(),
    }
}

/// Each row's height as drawn: a name broken over more lines makes its row taller.
fn row_heights(c: &mut Ctx, m: &MenuModel) -> Vec<f32> {
    let rows = &m.list(Side::Player).rows;
    let wrap = WRAP - if rows.iter().any(|r| r.ysi.is_some()) { YSI_ICON_ADVANCE } else { 0.0 };
    rows.iter().map(|r| ROW_H + LINE_PITCH * c.lines(2, &row_text(r), Some(wrap)).len().saturating_sub(1) as f32).collect()
}

/// The first row to draw: the list's own first row, or a later one when the rows from
/// there to the selection do not fit in `room` (rows of two lines take more of it than the
/// model's nine rows a page count on).
pub fn first_shown(heights: &[f32], first: usize, selected: usize, room: f32) -> usize {
    let mut first = first.min(heights.len());
    let last = selected.min(heights.len().saturating_sub(1));
    while first < last && heights[first..=last].iter().sum::<f32>() > room {
        first += 1;
    }
    first
}

/// A dim row's strength: half, as the original draws ammo the weapon cannot fire
/// [observed: observer check 288, text about half as bright].
const DIM_ROW: f32 = 0.5;

fn list(p: &mut ChildSpawnerCommands, c: &mut Ctx, m: &MenuModel, first: usize, heights: &[f32], tab: usize) {
    let bright = c.bright();
    let line = amber(LINE);
    let x0 = MAIN.min.x + LIST.min.x + ROW_X;
    let mut y = MAIN.min.y + LIST.min.y;
    let rows = &m.list(Side::Player).rows;
    let icon_advance = if rows.iter().any(|r| r.ysi.is_some()) { YSI_ICON_ADVANCE } else { 0.0 };
    let name_x = NAME_X + icon_advance;
    let wrap = WRAP - icon_advance;
    let selected = m.selected[Side::Player as usize];
    let mut fit = 0;
    for i in first..rows.len() {
        let r = &rows[i];
        // Ammo the weapon cannot fire at half brightness (`Row::dim`, #3442).
        let bright = if r.dim { bright.with_alpha(bright.alpha() * DIM_ROW) } else { bright };
        let lines = c.lines(2, &row_text(r), Some(wrap));
        let h = heights[i];
        if y + h > MAIN.min.y + LIST.max.y {
            break;
        }
        fit += 1;
        if i == selected {
            c.boxed(p, Rect::new(x0, y, x0 + ROW_W, y + h), line, None);
        }
        if r.equipped || r.loaded {
            let mk = Rect::new(x0 + MARK.min.x, y + MARK.min.y, x0 + MARK.max.x, y + MARK.max.y);
            if c.image(p, "square_filled.dds", at(mk.min.x, mk.min.y, mk.width(), mk.height()), bright).is_none() {
                p.spawn((at(mk.min.x, mk.min.y, mk.width(), mk.height()), BackgroundColor(bright)));
            }
        }
        if let Some(icon) = r.ysi.as_ref().and_then(|d| d.icon.as_deref()) {
            c.image(p, icon, at(x0 + NAME_X, y + 14.0, 24.0, 24.0), bright);
        }
        for (k, l) in lines.iter().enumerate() {
            c.set(p, 2, l, x0 + name_x, y + NAME_Y - THICK + LINE_PITCH * k as f32, Align::Left, bright);
        }
        p.spawn((at(x0, y, ROW_W, h), RowButton(i), Pickable::default())).observe(on_row_over).observe(on_touch_row_press).observe(on_row_click);
        y += h;
    }
    // The scroll bar, only while some rows do not show.
    if let Some((from, part)) = scroll_part(first, fit, rows.len()) {
        let (x, top) = (MAIN.min.x + LIST.min.x, MAIN.min.y + LIST.min.y);
        p.spawn((at(x, top, SCROLLBAR, LIST.height()), Pickable::IGNORE)).with_children(|b| {
            super::data::scroll_bar(b, c, 0.0, LIST.height(), from, part);
            let (marker_top, marker_height) = super::data::bar_marker(LIST.height(), from, part);
            let (track_top, track_bottom) = super::data::bar_track(LIST.height());
            let track = Rect::new(0.0, track_top, SCROLLBAR, track_bottom);
            let marker = Rect::new(0.0, marker_top, SCROLLBAR, (marker_top+marker_height).min(track.max.y));
            super::scrollbar::hits(b, Page::Items, tab, Rect::new(0.0,0.0,SCROLLBAR,LIST.height()), track, marker, Vec2::new(x,top));
        });
    }
}

/// Where the scroll bar's line starts and how much of the track it takes, when the rows
/// from `first`, `fit` of them, are not all of `len`.
pub fn scroll_part(first: usize, fit: usize, len: usize) -> Option<(f32, f32)> {
    (len > 0 && (first > 0 || first + fit < len)).then(|| (first as f32 / len as f32, fit as f32 / len as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ammo_effect_lines_format_each_operation() {
        use fnv_combat::data::AmmoEffect;
        for (operation, value, want) in [
            ("Multiply", 0.8, "Test label x0.80"),
            ("Multiply", 2.0, "Test label x2.00"),
            ("Add", 5.0, "Test label +5"),
            ("Add", -1.25, "Test label -1.25"),
            ("Subtract", 5.0, "Test label -5"),
            ("Subtract", 0.75, "Test label -0.75"),
            ("Subtract", -2.0, "Test label +2"),
        ] {
            let e = AmmoEffect { name: Some("Test label".into()), operation: Some(operation.into()), value, ..AmmoEffect::default() };
            assert_eq!(ammo_effect_line(&e).as_deref(), Some(want));
        }
        assert_eq!(ammo_effect_line(&AmmoEffect { operation: Some("99".into()), ..AmmoEffect::default() }), None);
        assert_eq!(ammo_effect_line(&AmmoEffect::default()), None);
    }

    #[test]
    fn ammo_effect_lines_use_full_else_our_kind_label() {
        use fnv_combat::data::AmmoEffect;
        for (kind, label) in [
            ("Damage Mod", "Damage"), ("DR Mod", "Damage resist"),
            ("DT Mod", "Damage threshold"), ("Spread Mod", "Spread"),
            ("Weapon Condition Mod", "Weapon wear"), ("Fatigue Mod", "Fatigue"),
            ("99", "Ammo effect"),
        ] {
            let mut e = AmmoEffect { name: None, kind: Some(kind.into()), operation: Some("Add".into()), value: 3.5, ..AmmoEffect::default() };
            let want = format!("{label} +3.5");
            assert_eq!(ammo_effect_line(&e).as_deref(), Some(want.as_str()));
            e.name = Some("  ".into());
            assert_eq!(ammo_effect_line(&e).as_deref(), Some(want.as_str()));
            e.name = Some("Mod author's label".into());
            assert_eq!(ammo_effect_line(&e).as_deref(), Some("Mod author's label +3.5"));
        }
    }

    #[test]
    fn ammo_card_with_and_without_effects() {
        use fnv_combat::data::{AmmoEffect, AmmoStats, CombatForms};
        let mut forms = CombatForms::default();
        forms.ammo.insert(FormId(1), AmmoStats { effects: vec![FormId(3), FormId(99), FormId(2)], ..AmmoStats::default() });
        forms.ammo.insert(FormId(4), AmmoStats::default());
        forms.ammo_effects.insert(FormId(2), AmmoEffect { name: Some("Test damage".into()), operation: Some("Multiply".into()), value: 0.8, ..AmmoEffect::default() });
        forms.ammo_effects.insert(FormId(3), AmmoEffect { name: Some("Test threshold".into()), operation: Some("Subtract".into()), value: 5.0, ..AmmoEffect::default() });
        let mut info = Info { weight: Some("1.00".into()), value: Some("12".into()), ..Info::default() };
        ammo_card_effects(&mut info, FormId(1), &forms);
        assert_eq!(info.effects.as_deref(), Some("Test threshold -5\nTest damage x0.80"));
        for size in [INFO.size(), Vec2::new(530.0, 120.0)] {
            let tiles = tiles_in(&info, size);
            assert_eq!(tiles.len(), 4);
            assert_eq!(tiles[2].title, "");
            assert_eq!(tiles[2].value, "Test threshold -5");
            assert_eq!(tiles[3].value, "Test damage x0.80");
            assert_eq!(tiles[2].rect.min.y, size.y / 3.0);
            assert_eq!(tiles[3].rect.min.y, 2.0 * size.y / 3.0);
        }
        ammo_card_effects(&mut info, FormId(4), &forms);
        assert_eq!(info.effects, None);
        assert_eq!(info_tiles(&info), info_tiles(&Info { weight: Some("1.00".into()), value: Some("12".into()), ..Info::default() }));
        ammo_card_effects(&mut info, FormId(99), &forms);
        assert_eq!(info.effects, None);
    }

    #[test]
    fn the_selected_row_stops_short_of_the_figures() {
        // The box ends 5 short of the list's edge, left of the figures and the badge.
        assert_eq!(LIST.min.x + ROW_X + ROW_W, LIST.max.x - 5.0);
        assert!(LIST.min.x + ROW_X + ROW_W < INFO.min.x.min(BADGE.min.x));
    }

    #[test]
    fn a_stack_shows_its_count() {
        let mut r = Row {
            item: FormId(1),
            at: None,
            name: "Stimpak".into(),
            count: 9,
            weight: None,
            value: None,
            kind: None,
            equipped: false,
            quest: false,
            condition: None,
            icon: None,
            price: None,
            bartered: false, loaded: false, dim: false,
            mods: 0,
            mods_value: 0,
            folder: None,
            reference: None,
            live: false,
            ysi: None,
        };
        assert_eq!(row_text(&r), "Stimpak (9)");
        r.count = 1;
        assert_eq!(row_text(&r), "Stimpak");
    }

    fn row(kind: Option<fnv_sim::forms::ItemKind>) -> Row {
        Row { item: FormId(1), at: None, name: "Thing".into(), count: 1, weight: None, value: None, kind, equipped: false, quest: false, condition: None, icon: None, price: None, bartered: false, loaded: false, dim: false, mods: 0, mods_value: 0, folder: None, reference: None, live: false, ysi: None }
    }

    #[test]
    fn stewie_single_item_query_matches_actual_pipboy_stack_figures() {
        use fnv_script::vm::XVal;
        let mut script = crate::script::ScriptState::over_no_plugins();
        let mut sim = SimState::over_placed([]);
        let mut r = row(Some(fnv_sim::forms::ItemKind::Misc));
        r.count = 4;
        r.weight = Some(2.5);
        r.value = Some(7);
        for (one, weight, value) in [(false, "10.00", "28"), (true, "2.50", "7"), (false, "10.00", "28")] {
            {
                let game = &mut script.rt.as_mut().unwrap().game;
                let settings = fnv_game::tweaks::Tweaks { single_item_figures: one, ..Default::default() };
                game.bind_stewie_stat_settings(&settings);
                for name in ["StewVal", "StewieGetValue"] {
                    assert_eq!(game.run_query(&format!("Begin Function {{}}\nSetFunctionValue ({name} \"bIndividialItemStats:Tweaks\")\nEnd\n"), Default::default()).unwrap(), XVal::Num(f64::from(u8::from(one))));
                }
            }
            let info = info_of(&r, None, &mut sim, None, Some(&mut script));
            assert_eq!(info.weight.as_deref(), Some(weight));
            assert_eq!(info.value.as_deref(), Some(value));
        }
    }

    fn shown(h: &[Hint]) -> Vec<(String, bool)> {
        h.iter().map(|h| (h.text(), h.dim)).collect()
    }

    fn hint(text: &str, dim: bool) -> (String, bool) {
        (text.to_string(), dim)
    }

    /// With a controller every tab shows the original's whole column, each hint with its
    /// button, in the original's order (Equip, Drop, Repair, Hot Key, then Mod on Weapons);
    /// with the keyboard and mouse only Repair (Maintain) and, on Weapons, Mod. Those that
    /// do nothing for the selected item are dim (#1896). Whether Enter does anything is the
    /// game's check (`model::enter_acts`, tested with the menu model on a synthetic
    /// plugin); here it is given.
    #[test]
    fn each_tab_shows_its_hints_and_dims_what_does_nothing() {
        use crate::pad::{set_labels, Labels};
        use fnv_sim::forms::ItemKind;
        let gun = row(Some(ItemKind::Weapon));
        let worn = Row { equipped: true, ..row(Some(ItemKind::Armor)) };
        let stim = row(Some(ItemKind::Aid));
        let misc = |tab| match tab {
            Category::Misc => row(Some(ItemKind::Misc)),
            _ => row(Some(ItemKind::Ammo)),
        };

        // A controller (Xbox names): A equips, X drops, Y repairs, LB mods.
        set_labels(Labels::Xbox);
        // Weapons: Equip, Drop, Repair, Hotkey, Mod; a weapon with mod slots, repairable.
        let weapons = shown(&page_hints(Category::Weapons, Some(&gun), ("Repair", true), true, true));
        // A weapon without mod slots, nothing to repair it with: Repair and Mod dim, in
        // their rows (the drawing's own log lines read them there).
        let bare = page_hints(Category::Weapons, Some(&gun), ("Repair", false), false, true);
        let (repair_row, mod_row) = hint_rows(Labels::Xbox);
        let bare = (bare[repair_row].word, bare[repair_row].dim, bare[mod_row].word, bare[mod_row].dim, bare[0].dim);
        // Apparel worn, at full condition: Unequip, Maintain; no Mod hint.
        let apparel = shown(&page_hints(Category::Apparel, Some(&worn), ("Maintain", false), false, true));
        // Aid: Use; nothing repairs it. A book on the Aid tab is read: Use.
        let aid = shown(&page_hints(Category::Aid, Some(&stim), super::super::repair::hint_word(None), false, true));
        let book = page_hints(Category::Aid, Some(&row(Some(ItemKind::Book))), ("Repair", false), false, true)[0].word;
        // What the game refuses (a book that teaches nothing, a broken weapon) has a dim
        // equip hint; the hotkey hint goes by what a hotkey holds.
        let refused = page_hints(Category::Weapons, Some(&gun), ("Repair", true), true, false);
        let refused = (refused[0].text(), refused[0].dim, refused[3].dim);
        // Misc and Ammo: Equip and Hotkey dim (they do nothing there), Drop bright.
        let misc_ammo: Vec<_> = [Category::Misc, Category::Ammo].map(|tab| shown(&page_hints(tab, Some(&misc(tab)), ("Repair", false), false, false))).into();
        // A quest item cannot be dropped; an empty tab dims everything.
        let quest = Row { quest: true, ..row(Some(ItemKind::Misc)) };
        let quest = page_hints(Category::Misc, Some(&quest), ("Repair", false), false, false)[1].dim;
        let empty = page_hints(Category::Weapons, None, ("Repair", false), false, true);
        let empty = (empty.len(), empty.iter().all(|h| h.dim));

        // The keyboard and mouse: Repair (Maintain) on every tab, Mod under it on Weapons.
        set_labels(Labels::Keys);
        assert_eq!(hint_rows(Labels::Keys), (0, 1));
        let keys_weapons = shown(&page_hints(Category::Weapons, Some(&gun), ("Repair", true), true, true));
        let keys_bare = shown(&page_hints(Category::Weapons, Some(&gun), ("Repair", false), false, true));
        let keys_apparel = shown(&page_hints(Category::Apparel, Some(&worn), ("Maintain", false), false, true));
        let keys_aid = shown(&page_hints(Category::Aid, Some(&stim), super::super::repair::hint_word(None), false, true));
        let keys_misc_ammo: Vec<_> = [Category::Misc, Category::Ammo].map(|tab| shown(&page_hints(tab, Some(&misc(tab)), ("Repair", false), false, false))).into();
        let keys_empty = shown(&page_hints(Category::Weapons, None, ("Repair", false), false, true));

        assert_eq!(weapons, [hint("Equip [A]", false), hint("Drop [X]", false), hint("Repair [Y]", false), hint("Hotkey [RB]", false), hint("Modify [LB]", false)]);
        assert_eq!(bare, ("Repair", true, "Modify", true, false));
        assert_eq!(apparel, [hint("Unequip [A]", false), hint("Drop [X]", false), hint("Maintain [Y]", true), hint("Hotkey [RB]", false)]);
        assert_eq!(aid, [hint("Use [A]", false), hint("Drop [X]", false), hint("Repair [Y]", true), hint("Hotkey [RB]", false)]);
        assert_eq!(book, "Use");
        assert_eq!(refused, ("Equip [A]".to_string(), true, false));
        for h in &misc_ammo {
            assert_eq!(h, &[hint("Equip [A]", true), hint("Drop [X]", false), hint("Repair [Y]", true), hint("Hotkey [RB]", true)]);
        }
        assert!(quest);
        assert_eq!(empty, (5, true), "an empty Weapons tab: all five controller hints, all dim");

        assert_eq!(keys_weapons, [hint("Repair [R]", false), hint("Modify [X]", false)]);
        assert_eq!(keys_bare, [hint("Repair [R]", true), hint("Modify [X]", true)]);
        assert_eq!(keys_apparel, [hint("Maintain [R]", true)]);
        assert_eq!(keys_aid, [hint("Repair [R]", true)]);
        for h in &keys_misc_ammo {
            assert_eq!(h, &[hint("Repair [R]", true)]);
        }
        assert_eq!(keys_empty, [hint("Repair [R]", true), hint("Modify [X]", true)]);
    }

    /// Five hints 46 apart from the Repair button's place end above the figures' highest
    /// card (the STR card, a card's height over them); the words end at the heading's right
    /// end, and the keys may reach past it no further than the canvas.
    #[test]
    fn the_hints_fit_above_the_figures() {
        // The last hint's text top and a line of font 2 (31).
        let last = HINTS.y + frame::HINT_PITCH * 4.0 + frame::BUTTON_TEXT_Y + 31.0;
        assert!(last < INFO.min.y - INFO.height() / 3.0, "{last}");
        assert_eq!(HINTS.x, MAIN.width());
        // The key room stays inside the canvas (checked when the test compiles).
        const { assert!(MAIN.max.x + frame::HINT_KEY_ROOM <= crate::pipboy::CANVAS.x) };
    }

    #[test]
    fn dps_is_the_damage_at_the_condition_times_the_shots_a_second() {
        let auto = fnv_combat::data::WeaponStats { base_damage: 5, automatic: true, fire_rate: 10.0, attack_shots_per_sec: 2.0, ..Default::default() };
        assert!((weapon_dps(&auto, 1.0) - 50.0).abs() < 1e-3);
        let semi = fnv_combat::data::WeaponStats { automatic: false, ..auto.clone() };
        assert!((weapon_dps(&semi, 1.0) - 10.0).abs() < 1e-3);
        // Worn, it does less.
        assert!(weapon_dps(&semi, 0.5) < weapon_dps(&semi, 1.0));
        let none = fnv_combat::data::WeaponStats { base_damage: 7, ..Default::default() };
        assert!((weapon_dps(&none, 1.0) - 7.0).abs() < 1e-3);
    }

    /// Nine rows of 51 fill the list; one name over two lines (86) pushes the last of
    /// them out, so the first row drawn moves on to keep the selection in (owner's
    /// playtest list 7 item 15: the last weapon could not be scrolled into view).
    #[test]
    fn a_two_line_row_moves_the_first_row_on() {
        let mut heights = vec![ROW_H; 12];
        heights[6] = ROW_H + LINE_PITCH;
        let room = LIST.height();
        // The model scrolls nine rows a page: selecting the last row puts row 3 first.
        assert_eq!(first_shown(&heights, 3, 11, room), 4);
        // Without the long name it fits.
        assert_eq!(first_shown(&[ROW_H; 12], 3, 11, room), 3);
        // A selection above the first row is not moved here (the model scrolls up).
        assert_eq!(first_shown(&heights, 3, 2, room), 3);
        assert_eq!(first_shown(&[], 0, 0, room), 0);
    }

    #[test]
    fn the_scroll_bar_shows_only_for_a_longer_list() {
        assert_eq!(scroll_part(0, 5, 5), None);
        assert_eq!(scroll_part(0, 0, 0), None);
        assert_eq!(scroll_part(0, 9, 18), Some((0.0, 0.5)));
        assert_eq!(scroll_part(9, 9, 18), Some((0.5, 0.5)));
    }

    #[test]
    fn a_weapon_skill_has_its_badge() {
        assert_eq!(skill_badge("Guns"), Some("Interface\\Icons\\TypeIcons\\weap_skill_icon_sm_arms.dds"));
        assert_eq!(skill_badge("Melee Weapons"), Some("Interface\\Icons\\TypeIcons\\weap_skill_icon_melee.dds"));
        assert_eq!(skill_badge("Barter"), None);
    }

    #[test]
    fn the_figures_are_laid_out_as_the_file() {
        let (w, h) = (INFO.width() / 3.0, INFO.height() / 3.0);
        // A weapon: DAM, WG, VAL; STR over VAL; CND and the ammunition.
        let weapon = Info {
            first: Some((FirstStat::Damage, "DAM", "72".into())),
            weight: Some("5.50".into()),
            value: Some("299".into()),
            strength: Some(3),
            condition: Some(0.8),
            beside: Some("40mm Grenade (1/19)".into()),
            effects: None,
            ammo_effects: false,
            dps: Some("72".into()),
            badge: None,
            skill_req: None,
            mod_hint: false,
        };
        let t = info_tiles(&weapon);
        let titles: Vec<&str> = t.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, ["DAM", "WEIGHT", "Value", "Strength Req.", "DPS", "Condition", "40mm Grenade (1/19)"]);
        assert_eq!(t[4].rect.min, Vec2::new(0.0, -h));
        assert_eq!(t[1].rect.min, Vec2::new(w + 10.0, 0.0));
        assert_eq!(t[3].rect.min, Vec2::new(2.0 * (w + 10.0), -h));
        assert_eq!(t[5].meter, Some(0.8));
        assert!((t[6].rect.width() - (2.0 * w + 10.0)).abs() < 1e-4);
        // An aid item: WG and VAL, effects on the second row.
        let aid = Info { value: Some("675".into()), effects: Some("HP +40".into()), ..Info::default() };
        let t = info_tiles(&aid);
        assert_eq!(t[0].value, "--");
        assert_eq!((t[2].title.as_str(), t[2].rect.min.y), ("Effects", h));
        // Armour: effects under the condition row.
        let armour = Info { condition: Some(1.0), beside: Some("Medium".into()), effects: Some("AGL -1".into()), ..Info::default() };
        assert_eq!(info_tiles(&armour).last().map(|t| t.rect.min.y), Some(2.0 * h));
    }
}
