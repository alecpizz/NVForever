//! The Pip-Boy's weapon mod menu (docs/spec-notes/weapon-mods.md §4–§5; the figures are
//! `fnv_combat::data::WeaponStats::modded`): on the ITEMS page, X on any weapon with mod
//! slots lists its attached mods and the carried mods that fit a free slot; choosing a
//! carried one uses the item up and puts the slot's flag on the weapon (`Extra::mods`,
//! kept in saves), choosing an attached one takes it off and gives the item back
//! [design: the owner's rule, 2026-10-27]. The menu stays open until the player leaves it.
//!
//! - MenuMode 1061 while it is open [documented: GECK wiki GetMenuTargetRef]
//!   (`super::menu_code`).
//! - The list [documented: `menus\item_mod_menu.xml`, in the menus' rectangle]: the
//!   heading "ITEMS" with the weapon's name in font 4 right-aligned 100 short of the
//!   right edge; the list 15 left and 75 down, 430 wide, its rows 32 in and 393 wide, the
//!   name in font 2 at 40, 20 down, wrapped at 270, the equipped square 25 at 15 (on an
//!   attached mod's row too), under a mod's name its effect ("Ammunition capacity + 4")
//!   [design: the owner's picture of a modded menu, 2026-10-27]; the
//!   chosen row's picture 200 square at (450, 20) and the mod's description at
//!   (450, 220) wrapped at 350; the figures from (415, 400), 427 x 180: two cards of half
//!   that width at the top and, on a mod, an arrow 80 square in the middle, the gain in
//!   font 4 at three quarters, 30 under the arrow's top, and the cards after the mod at
//!   the bottom (on an attached mod, the weapon without it); "Mod Enter)" at 855, 75 down,
//!   dim on the weapon's own row, and under it "Exit E)", the words right-aligned at 855
//!   with the keys after them (#1896; `frame::Ctx::hint_column`).
//!   The first row is the weapon,
//!   boxed; the others the mods [hypothesis: as the repair list].
//! - Keys: Up/Down (W/S) choose, Enter or Space attaches or removes the chosen mod, E or
//!   Esc goes back [documented: `_PCButton_E` IMM_CancelButton; design: the rest]. The
//!   pointer as the repair list's: a row under it is chosen, a click on the chosen mod
//!   (or on the bright Mod button) attaches or removes it, a click on "Exit E)" goes
//!   back. Log:
//!   `pipboy mod: clicked row <n>`.
//! - Rows are silent [documented: the template's `&nosound;`]; attaching plays
//!   `UIRepairWeapon` [hypothesis].
//! - Test aid: the Pip-Boy aid's `mod=open|down|up|use|cancel` (`test_aid.rs`).
//! - Behind `[UI] TileMenus=ItemModMenu` the menu is drawn from its file instead
//!   (`crate::tiles::item_mod_menu`, weapon-mods.md §5.1): the rules, figures ([`compare`]),
//!   sounds and log lines here stay, and the drawing here stands while that page is not ready.

use super::frame::{self, Align, Ctx, Hint, LINE, MAIN};
use super::{Page, PipBoy};
use crate::interface::{pipboy_amber as amber, at};
use crate::menu::model::{Mode, Side};
use crate::menu::ActiveMenu;
use crate::sim::SimState;
use crate::ui_sounds::{UiEvent, UiMenu, UiSound};
use bevy::prelude::*;
use fnv_combat::data::{mod_flag, WeaponStats};
use fnv_sim::forms::FormStore;
use fnv_sim::inventory::Extra;
use fnv_sim::FormId;

/// The list and its rows [documented: `IMM_ItemModList`, the template].
const LIST: Rect = Rect { min: Vec2::new(-15.0, 75.0), max: Vec2::new(415.0, 543.0) };
const ROW_X: f32 = 32.0;
const ROW_W: f32 = 393.0;
const NAME_X: f32 = 40.0;
const NAME_Y: f32 = 20.0;
const WRAP: f32 = 270.0;
const MARK: Rect = Rect { min: Vec2::new(15.0, 17.5), max: Vec2::new(40.0, 42.5) };
/// Rows as the ITEMS list's [hypothesis: the barter menu's measure, barter-menu.md §3].
const ROW_H: f32 = super::items::ROW_H;
/// The rows the keys keep the choice among: the window moves when the choice would leave
/// the eight from the first shown (as many single-line rows as the list holds).
const SHOWN: usize = 8;
const LINE_PITCH: f32 = 35.0;
/// The picture, the description and the figures [documented: `IMM_ItemIcon`,
/// `IMM_ModDesc`, `IMM_CompareRect`].
const ICON: Rect = Rect { min: Vec2::new(450.0, 20.0), max: Vec2::new(650.0, 220.0) };
const DESC: Vec2 = Vec2::new(450.0, 220.0);
const DESC_WRAP: f32 = 350.0;
pub const DESC_LINES: usize = 3;
const COMPARE: Rect = Rect { min: Vec2::new(415.0, 400.0), max: Vec2::new(842.0, 580.0) };
const CARD_H: f32 = 60.0;
const ARROW: f32 = 80.0;
/// The condition meter in its card: 60 x 16, 15 from its right end [documented:
/// `IMM_ItemHealthMeter`].
const METER: Vec2 = Vec2::new(60.0, 16.0);
/// The name in the heading ends 100 short of the right edge [documented: `IMM_ItemName`].
const NAME_RIGHT_IN: f32 = 100.0;
/// The buttons [documented: `IMM_ButtonRect`].
const BUTTONS: Vec2 = Vec2::new(855.0, 75.0);
/// The effect's line ends this short of the row's right end [design].
const EFFECT_IN: f32 = 15.0;

/// The menu's two hints in drawing order (#1896), the mod button first and the exit button
/// under it [documented: the file lists the Mod button before `IMM_CancelButton`;
/// observed: "Mod" over "Exit" on the original's screenshots on issue #1896, taken with a
/// controller; hypothesis: the same two with the keyboard and mouse]: "Mod
/// Enter)" (Enter, with a controller A), attaching or removing the chosen mod and dim on
/// the weapon's own row, where it does nothing [documented: the file's Mod button names
/// `sInventoryMod`, `fnv_ui::strings`; design: its words "Mod" as the ITEMS page's, shown
/// on the PC too, and its key]; then "Exit E)" (E or Esc, with a controller B [#1662];
/// `IMM_CancelButton`'s `sExit` [documented], its words the setting's text, else "Exit"
/// [hypothesis: as the companion wheel's]).
pub fn menu_hints(s: &Session) -> [Hint; 2] {
    let chosen = s.selected.checked_sub(1).and_then(|i| s.offer.choices.get(i)).is_some();
    [
        Hint::new(super::labels::built_in_label("sInventoryMod"), crate::pad::hint(&[KeyCode::Enter], items_at()), !chosen),
        Hint::new(super::labels::built_in_label("sExit"), crate::pad::hint(&[KeyCode::KeyE, KeyCode::Escape], items_at()), false),
    ]
}

/// The ITEMS page, for its hints.
fn items_at() -> crate::pad::MenuAt {
    crate::pad::at_menu(fnv_game::game::menus::INVENTORY)
}
/// The meter's empty part [documented: the repair template's alpha 64, the same meter].
const BAR_BACK: f32 = 64.0 / 255.0;

/// A step from the keys or the test aid.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModInput {
    Open,
    Up,
    Down,
    Use,
    Cancel,
    /// The pointer came onto the row at this place (0 the weapon, then its mods).
    Hover(usize),
    /// A click on the row at this place: chooses it, or attaches the mod already chosen.
    Click(usize),
}

#[derive(Component, Clone, Copy)]
struct ModRow(usize);

#[derive(Component, Clone, Copy)]
struct ModExit;

#[derive(Component, Clone, Copy)]
struct ModAct;

/// Only a row not chosen yet (as the repair list's rows, `super::repair`). Not while the
/// menu drawn from its file has yet to decide (`crate::tiles::item_mod_menu::ItemModTiles::waiting`):
/// these rows stand for that frame, and a pointer at rest would choose a mod. Only a pointer
/// that moved (`super::hover_takes`, #2310): the rows redrawn under a resting pointer as the
/// keys or the D-pad step leave their choice alone.
fn on_row_over(over: On<PointerOver>, rows: Query<&ModRow>, menu: Res<PipItemMod>, tiles: Option<Res<crate::tiles::item_mod_menu::ItemModTiles>>, moved: Option<Res<super::PointerMoved>>, mut out: MessageWriter<ModInput>) {
    if tiles.is_some_and(|t| t.waiting) || !super::hover_takes(moved.as_deref()) {
        return;
    }
    let (Ok(r), Some(s)) = (rows.get(over.entity), menu.0.as_ref()) else { return };
    if s.selected != r.0 {
        out.write(ModInput::Hover(r.0));
    }
}

fn on_row_click(click: On<PointerClick>, rows: Query<&ModRow>, mut out: MessageWriter<ModInput>) {
    if let Ok(r) = rows.get(click.entity) {
        out.write(ModInput::Click(r.0));
    }
}

fn on_exit_click(click: On<PointerClick>, exit: Query<&ModExit>, mut out: MessageWriter<ModInput>) {
    if exit.get(click.entity).is_ok() {
        out.write(ModInput::Cancel);
    }
}

fn on_act_click(click: On<PointerClick>, act: Query<&ModAct>, mut out: MessageWriter<ModInput>) {
    if act.get(click.entity).is_ok() {
        out.write(ModInput::Use);
    }
}

/// What a step does with the list as it is: a click on the chosen mod attaches or removes
/// it, any other click chooses its row.
pub fn resolve(selected: Option<usize>, step: ModInput) -> ModInput {
    match (selected, step) {
        (Some(s), ModInput::Click(i)) if i == s && i > 0 => ModInput::Use,
        (_, ModInput::Click(i)) => ModInput::Hover(i),
        (_, x) => x,
    }
}

/// The mod menu while it is shown.
#[derive(Resource, Default)]
pub struct PipItemMod(pub Option<Session>);

impl PipItemMod {
    pub fn is_open(&self) -> bool {
        self.0.is_some()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub offer: Offer,
    /// The list's first row shown: moved only to keep the choice in view (the keys), so
    /// the pointer never scrolls the list.
    pub first: usize,
    /// 0: the weapon itself; else the mod at this place, less one.
    pub selected: usize,
}

/// A mod on the weapon, or one the player carries that fits a free slot.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    /// The IMOD.
    pub item: FormId,
    pub slot: u8,
    pub name: String,
    /// The units carried (spares, for an attached mod).
    pub count: i32,
    pub icon: Option<String>,
    /// On the weapon: choosing it takes it off.
    pub attached: bool,
    /// What it does, in words (`effect_text`).
    pub effect: Option<String>,
}

/// What the ITEMS page's selected weapon offers.
#[derive(Debug, Clone, PartialEq)]
pub struct Offer {
    pub item: FormId,
    /// The unit to mod (the equipped one first).
    pub extra: Extra,
    pub name: String,
    pub icon: Option<String>,
    pub choices: Vec<Choice>,
}

/// The offer of the row of `item` whose units are at `at` with the mods `mods` (the
/// equipped unit first when `equipped`): the mods on it and the carried mods that fit a
/// slot not taken yet, by name. `None` for an item that is not a weapon with mod slots.
pub fn offer(groups: &[(FormId, Extra, i32)], forms: &FormStore, w: &WeaponStats, item: FormId, at: Option<f32>, mods: u8, equipped: bool) -> Option<Offer> {
    if !w.mods.iter().any(|m| m.item.is_some() && mod_flag(m.slot) != 0) {
        return None;
    }
    let mine = groups.iter().filter(|g| g.0 == item && g.1.is_at(at) && g.1.mods == mods).max_by_key(|g| g.1.equipped == equipped)?;
    let carried = |id: FormId| groups.iter().filter(|g| g.0 == id).map(|g| g.2).sum::<i32>();
    let mut choices: Vec<Choice> = w
        .mods
        .iter()
        .filter(|m| mod_flag(m.slot) != 0)
        .filter_map(|m| {
            let id = m.item?;
            let count = carried(id);
            let attached = mine.1.mods & mod_flag(m.slot) != 0;
            (attached || count > 0).then(|| Choice {
                item: id,
                slot: m.slot,
                name: crate::menu::model::display_name(forms, id),
                count,
                icon: forms.item(id).and_then(|i| i.icon.clone()),
                attached,
                effect: effect_text(m.effect.as_deref(), m.value_a),
            })
        })
        .collect();
    choices.sort_by(|a, b| a.name.cmp(&b.name).then(a.slot.cmp(&b.slot)));
    Some(Offer {
        item,
        extra: mine.1,
        name: crate::menu::model::modded_name(crate::menu::model::display_name(forms, item), mine.1.mods),
        icon: forms.item(item).and_then(|i| i.icon.clone()),
        choices,
    })
}

/// The ITEMS page's selected row's offer, now.
pub fn current(active: &ActiveMenu, sim: &SimState, combat: Option<&crate::combat::Combat>) -> Option<Offer> {
    let m = active.0.as_ref().filter(|m| m.mode == Mode::Inventory)?;
    let r = m.list(Side::Player).rows.get(m.selected[Side::Player as usize])?;
    offer_now(sim, combat, r.item, r.at, r.mods, r.equipped)
}

fn offer_now(sim: &SimState, combat: Option<&crate::combat::Combat>, item: FormId, at: Option<f32>, mods: u8, equipped: bool) -> Option<Offer> {
    let w = combat?.forms.weapons.get(&item)?;
    let groups: Vec<(FormId, Extra, i32)> = sim.world.peek(FormId::PLAYER_REF).and_then(|s| s.inventory.as_ref()).map(|i| i.groups().collect()).unwrap_or_default();
    offer(&groups, sim.forms, w, item, at, mods, equipped)
}

/// A mod's effect in words, from its effect and value A as `WeaponStats::modded` uses
/// them (weapon-mods.md §2) [design: the words].
pub fn effect_text(effect: Option<&str>, a: f32) -> Option<String> {
    let n = |v: f32| {
        let s = format!("{v:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    Some(match effect? {
        "Increase Weapon Damage" => format!("Damage + {}", n(a)),
        "Increase Clip Capacity" => format!("Ammunition capacity + {}", n(a)),
        "Decrease Spread" => format!("Spread - {}", n(a)),
        "Decrease Weight" => format!("Weight - {}", n(a)),
        "Increase Max. Condition" => format!("Maximum condition + {}", n(a)),
        "Increase Rate of Fire" => format!("Rate of fire + {}%", n(a * 100.0)),
        "Increase Zoom" => "Zoom increased".to_string(),
        "Split Beam" => format!("Beams + {}", n(a)),
        "Silence" | "Suppressor" => "Silenced".to_string(),
        "Increase Projectile Speed" => format!("Projectile speed x {}", n(a)),
        "Regenerate Ammo (shots)" => "Every fourth shot is free".to_string(),
        "Regenerate Ammo (seconds)" => format!("Recharge time + {} s", n(a)),
        _ => return None,
    })
}

/// The keys of the mod menu and X on the ITEMS page; X on an item that is not a weapon
/// with mod slots, and Shift+X, do nothing (dropping is a right click, `super::items`).
/// The controller's LB presses X here (`IM_ModButton`'s `&xbuttonlb;` [documented:
/// inventory_menu.xml], `crate::pad::menu_buttons`).
#[allow(clippy::too_many_arguments)]
pub fn keys(
    pip: Res<PipBoy>,
    keys: crate::pad::ActionButtons<KeyCode>,
    mut aid: MessageReader<ModInput>,
    mut menu: ResMut<PipItemMod>,
    repair: Res<super::repair::PipRepair>,
    mut active: ResMut<ActiveMenu>,
    mut sim: ResMut<SimState>,
    combat: Option<Res<crate::combat::Combat>>,
    mut sounds: MessageWriter<UiSound>,
    prompt: Res<crate::menu::PromptKeys>,
    mut touch: crate::maintenance_touch::Requests,
    mut script: Option<ResMut<crate::script::ScriptState>>,
    mut taken: ResMut<super::KeysTaken>,
) {
    let keys = keys.snapshot();
    // Laid out in the Pip-Boy's screen texture, at its size whatever the UI scale (#1631).
    let _canvas = crate::interface::original_units();
    use KeyCode::*;
    // Only physical steps take keys; aid and touch requests retain their original paths.
    let mut steps: Vec<(ModInput, Option<crate::maintenance_touch::Request>, bool)> = aid.read().copied().map(|s| (s, None, false)).collect();
    let mobile = touch.take(crate::maintenance_touch::Kind::Mod);
    let items = pip.open() && pip.page == Page::Items && active.0.as_ref().is_some_and(|m| m.mode == Mode::Inventory);
    if !items {
        if menu.0.take().is_some() {
            info!("pipboy mod: closed with the Pip-Boy");
        }
        return;
    }
    if prompt.held(&active) || repair.is_open() {
        return;
    }
    let shift = keys.any_pressed([ShiftLeft, ShiftRight]);
    if keys.just_pressed(KeyX) && !shift && !menu.is_open() {
        steps.push((ModInput::Open, None, true));
    }
    if menu.is_open() {
        for (k, s) in [(&[ArrowUp, KeyW][..], ModInput::Up), (&[ArrowDown, KeyS], ModInput::Down), (&[Enter, NumpadEnter, Space], ModInput::Use), (&[KeyE, Escape], ModInput::Cancel)] {
            if keys.any_just_pressed(k.iter().copied()) {
                steps.push((s, None, true));
            }
        }
    }
    for request in mobile {
        use crate::maintenance_touch::Action;
        let step = match request.action { Action::Select(i) => ModInput::Hover(i), Action::Confirm => ModInput::Use, Action::Cancel => ModInput::Cancel, Action::Scroll(_) => ModInput::Hover(usize::MAX) };
        steps.push((step, Some(request), false));
    }
    for (step, request, physical) in steps {
        if physical && (menu.is_open() || step == ModInput::Open) {
            taken.take();
        }
        if let Some(request) = &request {
            if !crate::maintenance_touch::menu_allows(crate::maintenance_touch::Kind::Mod, script.as_ref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game)) { continue; }
            if !request.modding(menu.0.as_ref()) { continue; }
            let live = current(&active, &sim, combat.as_deref());
            if menu.0.as_ref().is_none_or(|s| Some(&s.offer) != live.as_ref()) {
                if let Some(s) = menu.0.as_mut() {
                    if let Some(offer) = live { s.offer = offer; s.selected = 0; s.first = 0; } else { menu.0 = None; }
                }
                continue;
            }
            if let crate::maintenance_touch::Action::Scroll(by) = request.action {
                if let Some(s) = menu.0.as_mut() { s.first = crate::maintenance_touch::scroll(s.first, by, s.offer.choices.len() + 1); }
                continue;
            }
        }
        if let ModInput::Click(i) = step {
            info!("pipboy mod: clicked row {i}");
        }
        let step = resolve(menu.0.as_ref().map(|s| s.selected), step);
        match (menu.0.as_mut(), step) {
            (None, ModInput::Open) => {
                // Not a weapon with mod slots: X does nothing.
                let Some(o) = current(&active, &sim, combat.as_deref()) else { continue };
                // The player's tweak: modding a weapon asks its skill requirement [design: docs/design/tweaks.md].
                let need = combat.as_deref().and_then(|c| c.forms.weapons.get(&o.item)).and_then(|w| Some((fnv_game::functions::av_by_name(w.skill.as_deref()?)?, w.skill_req)));
                if let (Some((skill, req)), Some(rt)) = (need, script.as_deref_mut().and_then(|s| s.rt.as_mut()).filter(|rt| rt.game.tweaks.modding_needs_skill)) {
                    let has = crate::script::with_world(rt, &mut sim, |rt| fnv_game::functions::actor_value_capped(&mut rt.game, FormId::PLAYER_REF, skill));
                    if has < f64::from(req) {
                        info!("pipboy mod: {} needs skill {req} to mod (the player has {has:.0}, tweak)", o.name);
                        sounds.write(UiSound { menu: UiMenu::Inventory, event: UiEvent::Refused });
                        continue;
                    }
                }
                info!("pipboy mod: open {} (flags {}): {}", o.name, o.extra.mods, o.choices.iter().map(|c| format!("{} (slot {}{})", c.name, c.slot, if c.attached { ", on" } else { "" })).collect::<Vec<_>>().join(", "));
                menu.0 = Some(Session { offer: o, first: 0, selected: 0 });
            }
            (None, _) => {}
            (Some(s), ModInput::Up) => {
                s.selected = s.selected.saturating_sub(1);
                s.first = crate::menu::model::keep_in_view(s.first, s.selected, s.offer.choices.len() + 1, SHOWN);
            }
            (Some(s), ModInput::Down) => {
                s.selected = (s.selected + 1).min(s.offer.choices.len());
                s.first = crate::menu::model::keep_in_view(s.first, s.selected, s.offer.choices.len() + 1, SHOWN);
            }
            (Some(s), ModInput::Hover(i)) => s.selected = i.min(s.offer.choices.len()),
            (Some(_), ModInput::Click(_) | ModInput::Open) => {}
            (Some(_), ModInput::Cancel) => {
                info!("pipboy mod: cancel");
                sounds.write(UiSound { menu: UiMenu::Repair, event: UiEvent::Back });
                menu.0 = None;
            }
            (Some(s), ModInput::Use) => {
                let Some(ch) = s.selected.checked_sub(1).and_then(|i| s.offer.choices.get(i)).cloned() else { continue };
                let o = &s.offer;
                // The slot's flag goes on, or off for an attached mod.
                let flags = o.extra.mods ^ mod_flag(ch.slot);
                let sign = if ch.attached { '-' } else { '+' };
                let done = sim.world.inventory_mut(FormId::PLAYER_REF).is_ok_and(|inv| if ch.attached { detach(inv, o.item, o.extra, ch.item, flags) } else { attach(inv, o.item, o.extra, ch.item, flags) });
                if done {
                    info!("pipboy mod: {} {sign} {} (slot {}, flags {flags})", o.name, ch.name, ch.slot);
                    sounds.write(UiSound { menu: UiMenu::Repair, event: UiEvent::Picked });
                } else {
                    info!("pipboy mod: {} {sign} {}: the items are gone", o.name, ch.name);
                    sounds.write(UiSound { menu: UiMenu::Repair, event: UiEvent::Refused });
                }
                if let Some(m) = active.0.as_mut() {
                    if let Err(e) = m.refresh(&mut sim.world) {
                        warn!("pipboy mod: the list: {e:?}");
                    } else if let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) {
                        crate::menu::sync_ysi_model(m, &mut rt.game);
                    }
                }
                // The menu stays open on the weapon as it is now, the same mod chosen
                // [design: the owner's rule, 2026-10-27].
                let unit = if done { Extra { mods: flags, ..o.extra } } else { o.extra };
                match offer_now(&sim, combat.as_deref(), o.item, Some(unit.condition_or_whole()), unit.mods, unit.equipped) {
                    Some(n) => {
                        s.selected = n.choices.iter().position(|c| c.item == ch.item && c.slot == ch.slot).map_or(0, |i| i + 1);
                        s.offer = n;
                        s.first = crate::menu::model::keep_in_view(s.first, s.selected, s.offer.choices.len() + 1, SHOWN);
                    }
                    None => {
                        info!("pipboy mod: the weapon is gone");
                        menu.0 = None;
                    }
                }
            }
        }
    }
}

/// Attach: one unit of the mod leaves the inventory (no script of it runs [documented:
/// GECK wiki Weapon Mods]) and the weapon's unit `unit` gets the flags. Nothing changes
/// unless both are there.
pub fn attach(inv: &mut fnv_sim::inventory::Inventory, weapon: FormId, unit: Extra, item_mod: FormId, flags: u8) -> bool {
    if inv.count(item_mod) < 1 || !inv.set_mods_one(weapon, unit, flags) {
        return false;
    }
    inv.remove(item_mod, 1) == 1
}

/// Remove: the weapon's unit `unit` gets the flags `flags` (the mod's taken away) and one
/// unit of the mod comes back to the inventory. Nothing changes unless the unit is there.
pub fn detach(inv: &mut fnv_sim::inventory::Inventory, weapon: FormId, unit: Extra, item_mod: FormId, flags: u8) -> bool {
    if !inv.set_mods_one(weapon, unit, flags) {
        return false;
    }
    inv.add(item_mod, 1);
    true
}

/// The figure a mod's effect changes, as a card's title and value, and as a number for
/// the gain (`None` for one without: Silence).
pub fn figure(effect: Option<&str>, w: &WeaponStats, condition: f32) -> Option<(&'static str, String, Option<f32>)> {
    let n = |title, v: f32, decimals: bool| Some((title, if decimals { format!("{v:.2}") } else { format!("{v:.0}") }, Some(v)));
    match effect? {
        "Increase Weapon Damage" => n(super::labels::built_in_label("sInventoryDamage"), w.base_damage as f32, false),
        "Increase Clip Capacity" => n("Clip", w.clip_size as f32, false),
        "Decrease Spread" => n("Spread", w.min_spread, true),
        "Decrease Weight" => n(super::labels::built_in_label("sInventoryWeightUpper"), w.weight, true),
        "Increase Max. Condition" => n("Max CND", w.health as f32, false),
        "Increase Rate of Fire" => n("DPS", super::items::weapon_dps(w, condition), false),
        "Increase Zoom" => n("Zoom", w.sight_fov, false),
        "Split Beam" => n("Beams", w.projectile_count.max(1) as f32, false),
        "Silence" | "Suppressor" => Some(("Noise", w.sound_level.clone().unwrap_or_default(), None)),
        _ => None,
    }
}

/// The gain's words: "+3", "-0.40".
pub fn gain_text(before: f32, after: f32) -> String {
    let d = after - before;
    let s = if d.fract().abs() > 1e-4 { format!("{d:.2}") } else { format!("{d:.0}") };
    if d >= 0.0 {
        format!("+{s}")
    } else {
        s
    }
}

fn bar(p: &mut ChildSpawnerCommands, r: Rect, fill: f32, color: Color) {
    p.spawn((at(r.min.x, r.min.y, r.width(), r.height()), BackgroundColor(color.with_alpha(BAR_BACK))));
    p.spawn((at(r.min.x, r.min.y, r.width() * fill.clamp(0.0, 1.0), r.height()), BackgroundColor(color)));
}

/// The mod menu in place of the ITEMS page's body. `desc` is the chosen mod's
/// description (IMOD DESC).
pub fn draw_list(p: &mut ChildSpawnerCommands, c: &mut Ctx, s: &Session, combat: Option<&crate::combat::Combat>, desc: Option<&str>) {
    let bright = c.bright();
    let line = amber(LINE);
    let o = &s.offer;
    frame::heading_named(p, c, Page::Items.title(), &o.name, NAME_RIGHT_IN);
    // The two hints as the ITEMS page's column (Mod, then Exit), each with its click area
    // over its drawn text and 10 past it.
    let by = MAIN.min.y + BUTTONS.y + frame::BUTTON_TEXT_Y;
    let hints = menu_hints(s);
    let rows = c.hint_column(p, &hints, MAIN.min.x + BUTTONS.x, by);
    let area = |r: &Rect| at(r.min.x, r.min.y - 5.0, r.width() + 10.0, 40.0);
    let chosen = chosen(s);
    if let (Some(r), true) = (rows.first(), chosen.is_some()) {
        p.spawn((area(r), ModAct, Pickable::default())).observe(on_act_click);
    }
    if let Some(r) = rows.get(1) {
        p.spawn((area(r), ModExit, Pickable::default())).observe(on_exit_click);
    }
    // The rows: the weapon, then the mods (an attached one marked, as the equipped
    // weapon), each mod's effect under its name.
    let rows: Vec<(String, bool, Option<&str>)> = std::iter::once((o.name.clone(), o.extra.equipped, None))
        .chain(o.choices.iter().map(|ch| (row_name(ch), ch.attached, ch.effect.as_deref())))
        .collect();
    let x0 = MAIN.min.x + LIST.min.x + ROW_X;
    let mut y = MAIN.min.y + LIST.min.y;
    let first = s.first;
    for (i, (name, worn, effect)) in rows.iter().enumerate().skip(first) {
        let mut lines = c.lines(2, name, Some(WRAP));
        if let Some(e) = effect {
            lines.extend(c.lines(2, e, Some(ROW_W - NAME_X - EFFECT_IN)));
        }
        let h = ROW_H + LINE_PITCH * lines.len().saturating_sub(1) as f32;
        if y + h > MAIN.min.y + LIST.max.y {
            break;
        }
        if i == 0 || i == s.selected {
            c.boxed(p, Rect::new(x0, y, x0 + ROW_W, y + h), line, None);
        }
        if *worn {
            let mk = Rect::new(x0 + MARK.min.x, y + MARK.min.y, x0 + MARK.max.x, y + MARK.max.y);
            if c.image(p, "square_filled.dds", at(mk.min.x, mk.min.y, mk.width(), mk.height()), bright).is_none() {
                p.spawn((at(mk.min.x, mk.min.y, mk.width(), mk.height()), BackgroundColor(bright)));
            }
        }
        for (k, l) in lines.iter().enumerate() {
            c.set(p, 2, l, x0 + NAME_X, y + NAME_Y - frame::THICK + LINE_PITCH * k as f32, Align::Left, bright);
        }
        p.spawn((at(x0, y, ROW_W, h), ModRow(i), Pickable::default())).observe(on_row_over).observe(on_row_click);
        y += h;
    }
    // The picture and the description.
    if let Some(icon) = chosen.map_or(o.icon.as_deref(), |ch| ch.icon.as_deref()) {
        let r = Rect::from_corners(MAIN.min + ICON.min, MAIN.min + ICON.max);
        c.image(p, icon, at(r.min.x, r.min.y, r.width(), r.height()), bright);
    }
    if let Some(d) = desc.filter(|d| chosen.is_some() && !d.is_empty()) {
        let pitch = c.line_height(2);
        let lines = c.lines(2, d, Some(DESC_WRAP));
        for (k, l) in lines.iter().take(DESC_LINES).enumerate() {
            c.set(p, 2, l, MAIN.min.x + DESC.x, MAIN.min.y + DESC.y + pitch * k as f32, Align::Left, bright);
        }
    }
    // The figures: the weapon as it is, and on a mod after attaching or removing it.
    let Some(w) = combat.and_then(|cb| cb.forms.weapons.get(&o.item)) else { return };
    let cmp = compare(s, w);
    let cr = Rect::from_corners(MAIN.min + COMPARE.min, MAIN.min + COMPARE.max);
    let half = cr.width() / 2.0;
    let cards = |p: &mut ChildSpawnerCommands, c: &mut Ctx, y: f32, stat: &Option<(&'static str, String)>| {
        frame::card(p, c, cr.min.x, y, half - 5.0, super::labels::built_in_label("sInventoryCondition"), "");
        let m = Rect::new(cr.min.x + half - 5.0 - METER.x - 15.0, y + 17.0, cr.min.x + half - 5.0 - 15.0, y + 17.0 + METER.y);
        bar(p, m, cmp.condition, bright);
        let (t, v) = stat.as_ref().map_or(("", ""), |(t, v)| (*t, v.as_str()));
        frame::card(p, c, cr.min.x + half + 5.0, y, half - 5.0, t, v);
    };
    cards(p, c, cr.min.y, &cmp.now);
    if let Some(after) = &cmp.after {
        let ay = cr.min.y + (cr.height() - ARROW) / 2.0 - 5.0;
        c.image(p, "interface\\shared\\arrow\\repair_arrow_down.dds", at(cr.min.x + (cr.width() - ARROW) / 2.0, ay, ARROW, ARROW), bright);
        if let Some(gain) = &cmp.gain {
            c.text(p, 4, gain, cr.min.x + cr.width() * 0.75, ay + 30.0, Align::Centre, bright);
        }
        cards(p, c, cr.max.y - CARD_H, after);
    }
}

/// What the figures compare, for the hand-built cards and the page drawn from the file's
/// (`crate::tiles::item_mod_menu`) alike: the weapon's condition, its stat card as it is (the
/// figure the chosen mod changes, else its damage when it has any), on a mod the same after
/// attaching or removing it, and the gain between them in words ([`gain_text`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Compare {
    pub condition: f32,
    /// The stat card's title and value; `None` for none to show.
    pub now: Option<(&'static str, String)>,
    /// On a mod: the stat card after it (itself `None` for none to show).
    pub after: Option<Option<(&'static str, String)>>,
    pub gain: Option<String>,
}

/// [`Compare`] for the session's chosen row, by the weapon's stats `w`.
pub fn compare(s: &Session, w: &WeaponStats) -> Compare {
    let o = &s.offer;
    let chosen = s.selected.checked_sub(1).and_then(|i| o.choices.get(i));
    let now = w.modded(o.extra.mods);
    let condition = o.extra.condition_or_whole();
    let effect = chosen.and_then(|ch| w.mods.iter().find(|m| m.slot == ch.slot)).and_then(|m| m.effect.clone());
    let stat = |ws: &WeaponStats| {
        if effect.is_some() {
            figure(effect.as_deref(), ws, condition).map(|(t, v, _)| (t, v))
        } else {
            (ws.base_damage > 0).then(|| (super::labels::built_in_label("sInventoryDamage"), ws.base_damage.to_string()))
        }
    };
    let after = chosen.map(|ch| w.modded(o.extra.mods ^ mod_flag(ch.slot)));
    let gain = after.as_ref().and_then(|after| match (figure(effect.as_deref(), &now, condition), figure(effect.as_deref(), after, condition)) {
        (Some((_, _, Some(a))), Some((_, _, Some(b)))) => Some(gain_text(a, b)),
        _ => None,
    });
    Compare { condition, now: stat(&now), after: after.as_ref().map(stat), gain }
}

/// The chosen row's mod, if a mod's row is chosen.
pub fn chosen(s: &Session) -> Option<&Choice> {
    s.selected.checked_sub(1).and_then(|i| s.offer.choices.get(i))
}

/// A row's name as the list shows it: a carried mod's count after its name when more than one
/// [design: the hand-built list's].
pub fn row_name(ch: &Choice) -> String {
    if ch.count > 1 && !ch.attached {
        format!("{} ({})", ch.name, ch.count)
    } else {
        ch.name.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fnv_combat::data::WeaponMod;
    use fnv_sim::forms::{BaseData, BaseForm, Item, ItemKind};
    use fnv_sim::inventory::Inventory;

    const GUN: FormId = FormId(0x100);
    const MAGS: FormId = FormId(0x201);
    const SIGHT: FormId = FormId(0x202);
    const SILENCER: FormId = FormId(0x203);

    fn store() -> FormStore {
        let mut f = FormStore::default();
        for (id, kind, name) in [(GUN, ItemKind::Weapon, "10mm Pistol"), (MAGS, ItemKind::ItemMod, "10mm Pistol Extended Mags"), (SIGHT, ItemKind::ItemMod, "10mm Pistol Laser Sight"), (SILENCER, ItemKind::ItemMod, "10mm Pistol Silencer")] {
            let it = Item { kind, value: Some(10), money: None, weight: Some(1.0), slots: None, projectile: None, health: None, non_playable: false, cant_drop: false, armor_class: Default::default(), power_armor: false, armor_dr: None, armor_dt: None, weapon_damage: None, weight_mods: [0.0; 3], drawable: false, not_for_npcs: false, thrown: false, quest_item: false, icon: Some(format!("{name}.dds")), placed_count: None, teaches: None, cant_be_taken: false, modulates_voice: false, weapon_traits: None };
            f.bases.insert(id, BaseForm { id, sig: *b"WEAP", edid: None, name: Some(name.into()), script: None, prompt: None, voice_filter: false, data: BaseData::Item(it) });
        }
        f
    }

    fn pistol() -> WeaponStats {
        let m = |slot, item, effect: &str, a| WeaponMod { slot, item: Some(item), effect: Some(effect.into()), value_a: a, value_b: 0.0 };
        WeaponStats { base_damage: 22, clip_size: 12, min_spread: 0.8, mods: vec![m(1, MAGS, "Increase Clip Capacity", 4.0), m(2, SIGHT, "Decrease Spread", 0.4), m(3, SILENCER, "Silence", 0.0)], ..Default::default() }
    }

    #[test]
    fn the_menu_lists_carried_mods_for_free_slots() {
        let forms = store();
        let on = Extra { equipped: true, ..Extra::default() };
        let groups = vec![(GUN, on, 1), (GUN, Extra { mods: 1, ..Extra::default() }, 1), (MAGS, Extra::default(), 2), (SILENCER, Extra::default(), 1)];
        // The plain, equipped pistol: the magazines and the silencer (no laser sight carried).
        let o = offer(&groups, &forms, &pistol(), GUN, Some(1.0), 0, true).unwrap();
        assert_eq!(o.extra, on);
        let listed: Vec<(FormId, u8, i32)> = o.choices.iter().map(|c| (c.item, c.slot, c.count)).collect();
        assert_eq!(listed, vec![(MAGS, 1, 2), (SILENCER, 3, 1)]);
        assert_eq!(o.choices[0].effect.as_deref(), Some("Ammunition capacity + 4"));
        assert_eq!(o.choices[1].effect.as_deref(), Some("Silenced"));
        // The modded one: its magazines are on (to take off), the silencer to put on; its
        // name says so.
        let o = offer(&groups, &forms, &pistol(), GUN, Some(1.0), 1, false).unwrap();
        assert_eq!(o.choices.iter().map(|c| (c.item, c.attached)).collect::<Vec<_>>(), vec![(MAGS, true), (SILENCER, false)]);
        assert_eq!(o.name, "10mm Pistol +");
        // Nothing carried: the menu still opens, empty. A weapon without slots: no offer.
        let bare = vec![(GUN, on, 1)];
        assert!(offer(&bare, &forms, &pistol(), GUN, Some(1.0), 0, true).unwrap().choices.is_empty());
        assert_eq!(offer(&bare, &forms, &WeaponStats::default(), GUN, Some(1.0), 0, true), None);
    }

    /// The menu's two hints (#1896): "Modify [Enter]" over "Exit [E]", the Mod hint dim on the
    /// weapon's own row and bright on a mod; with a controller A and B.
    #[test]
    fn the_menu_shows_its_mod_and_exit_hints() {
        let forms = store();
        let on = Extra { equipped: true, ..Extra::default() };
        let groups = vec![(GUN, on, 1), (MAGS, Extra::default(), 1)];
        let mut s = Session { offer: offer(&groups, &forms, &pistol(), GUN, Some(1.0), 0, true).unwrap(), first: 0, selected: 0 };
        let shown = |s: &Session| menu_hints(s).iter().map(|h| (h.text(), h.dim)).collect::<Vec<_>>();
        assert_eq!(shown(&s), [("Modify [Enter]".to_string(), true), ("Exit [E]".to_string(), false)]);
        s.selected = 1;
        assert_eq!(shown(&s), [("Modify [Enter]".to_string(), false), ("Exit [E]".to_string(), false)]);
        crate::pad::set_labels(crate::pad::Labels::Xbox);
        let keys: Vec<String> = menu_hints(&s).iter().map(|h| h.key.clone()).collect();
        crate::pad::set_labels(crate::pad::Labels::Keys);
        assert_eq!(keys, ["[A]", "[B]"]);
    }

    #[test]
    fn attaching_uses_the_mod_up_and_marks_the_weapon() {
        let mut inv = Inventory::new();
        inv.add(GUN, 1);
        assert!(inv.set_equipped(GUN));
        inv.add(MAGS, 2);
        let on = Extra { equipped: true, ..Extra::default() };
        assert!(attach(&mut inv, GUN, on, MAGS, 1));
        assert_eq!((inv.count(MAGS), inv.equipped_mods(GUN), inv.count(GUN)), (1, 1, 1));
        // A second time with the old unit: it is gone, nothing changes.
        assert!(!attach(&mut inv, GUN, on, MAGS, 1));
        assert_eq!(inv.count(MAGS), 1);
        // No mod carried: nothing changes.
        let now = Extra { mods: 1, ..on };
        assert!(!attach(&mut inv, GUN, now, SIGHT, 3));
        assert_eq!(inv.equipped_mods(GUN), 1);
    }

    #[test]
    fn removing_gives_the_mod_back_and_clears_the_flag() {
        let mut inv = Inventory::new();
        inv.add(GUN, 1);
        assert!(inv.set_equipped(GUN));
        inv.add(MAGS, 1);
        let on = Extra { equipped: true, ..Extra::default() };
        assert!(attach(&mut inv, GUN, on, MAGS, 1));
        assert_eq!(inv.count(MAGS), 0);
        let modded = Extra { mods: 1, ..on };
        assert!(detach(&mut inv, GUN, modded, MAGS, 0));
        assert_eq!((inv.count(MAGS), inv.equipped_mods(GUN), inv.count(GUN)), (1, 0, 1));
        // The modded unit is gone: nothing changes.
        assert!(!detach(&mut inv, GUN, modded, MAGS, 0));
        assert_eq!(inv.count(MAGS), 1);
    }

    #[test]
    fn effects_read_as_words() {
        assert_eq!(effect_text(Some("Decrease Weight"), 1.5).as_deref(), Some("Weight - 1.5"));
        assert_eq!(effect_text(Some("Increase Rate of Fire"), 0.2).as_deref(), Some("Rate of fire + 20%"));
        assert_eq!(effect_text(Some("Increase Weapon Damage"), 5.0).as_deref(), Some("Damage + 5"));
        assert_eq!(effect_text(None, 1.0), None);
    }

    #[test]
    fn the_figures_show_what_the_mod_changes() {
        let w = pistol();
        let (t, v, n) = figure(Some("Increase Clip Capacity"), &w.modded(0), 1.0).unwrap();
        assert_eq!((t, v.as_str(), n), ("Clip", "12", Some(12.0)));
        let after = figure(Some("Increase Clip Capacity"), &w.modded(1), 1.0).unwrap();
        assert_eq!(gain_text(12.0, after.2.unwrap()), "+4");
        assert_eq!(gain_text(0.8, 0.4), "-0.40");
        assert_eq!(figure(Some("Silence"), &w.modded(4), 1.0).map(|f| f.1), Some("Silent".into()));
        assert_eq!(figure(None, &w, 1.0), None);
    }

    /// What both drawings' cards compare (`compare`): the weapon's damage on its own row; on a mod
    /// the figure it changes before and after, whole, decimal or in words, and the gain.
    #[test]
    fn the_comparison_words_whole_decimal_and_worded_figures() {
        let forms = store();
        let on = Extra { equipped: true, ..Extra::default() };
        let groups = vec![(GUN, on, 1), (MAGS, Extra::default(), 1), (SIGHT, Extra::default(), 1), (SILENCER, Extra::default(), 1)];
        let offer = offer(&groups, &forms, &pistol(), GUN, Some(1.0), 0, true).unwrap();
        let at = |name: &str| offer.choices.iter().position(|c| c.name.ends_with(name)).unwrap() + 1;
        let cmp = |selected: usize| compare(&Session { offer: offer.clone(), first: 0, selected }, &pistol());
        let weapon = cmp(0);
        assert_eq!((weapon.now.map(|f| f.1), weapon.after, weapon.gain, weapon.condition), (Some("22".to_string()), None, None, 1.0));
        let mags = cmp(at("Mags"));
        assert_eq!((mags.now.map(|f| f.1), mags.after.flatten().map(|f| f.1), mags.gain.as_deref()), (Some("12".into()), Some("16".into()), Some("+4")));
        let sight = cmp(at("Sight"));
        assert_eq!((sight.now.map(|f| f.1), sight.after.flatten().map(|f| f.1), sight.gain.as_deref()), (Some("0.80".into()), Some("0.40".into()), Some("-0.40")));
        let quiet = cmp(at("Silencer"));
        assert_eq!(quiet.after.flatten(), Some(("Noise", "Silent".to_string())));
        assert_eq!(quiet.gain, None, "words have no gain");
    }

    #[test]
    fn a_click_chooses_a_row_and_a_second_click_attaches_the_mod() {
        assert_eq!(resolve(Some(0), ModInput::Click(1)), ModInput::Hover(1));
        assert_eq!(resolve(Some(1), ModInput::Click(1)), ModInput::Use);
        assert_eq!(resolve(Some(0), ModInput::Click(0)), ModInput::Hover(0));
        assert_eq!(resolve(None, ModInput::Down), ModInput::Down);
    }
}
