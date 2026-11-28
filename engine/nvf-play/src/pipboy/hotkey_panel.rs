//! The ITEMS page's hotkey panel (docs/design/weapon-wheel.md §3): hold a hotkey's key
//! (1 to 8) and left-click an item in the list to put it in that slot [design: ?3].
//! [design, #2312] With Controller chosen, RB opens it for the selected row;
//! D-pad steps slots, A assigns, B cancels (observer check 211).
//!
//! - **The panel** [documented: menus\main\inventory_menu.xml `IM_HotKeyWheel`, 380, 52
//!   in the menus' rectangle, 290 × 250, drawn further right ([`PANEL`]);
//!   menus\prefabs\hotkeys.xml, hotkey.xml]: eight 60-square boxes in its corners and the middles of its sides (key 1 top left, 2 top
//!   middle, 3 top right, 4 left, 5 right, 6 bottom left, 7 bottom middle, 8 bottom right),
//!   each a border 5 in with the slot's icon in it and its number at its right foot; the
//!   chosen slot's box filled (`_fill_alpha` 80). Key 2's place is the Ammo Swap picture,
//!   50 square, centred at the top. The compass rose (`hot_keys_rose.dds`, 150 square at
//!   100, 80) behind, the pointed-at item's icon in its middle (`IM_HotkeyIcon`, 120.5,
//!   102, 50 square) and a line of font 2 centred at 150, 262 (`IM_HotKeyText`). While
//!   it shows, the item's picture, skill badge and the Repair and Mod hints do not
//!   (`IM_ButtonRect` is hidden with it) [documented]; the figures stay [observed: the
//!   owner's screenshot of the original].
//! - **When it shows** [design, the owner 2026-10-26: "hold your key and left click your
//!   item to assign"; the original opens it with its Hotkey button instead]: while a
//!   hotkey's key is held on the ITEMS page. A left click on a row then puts that row's
//!   item in the held slot ([`crate::hotkeys::Hotkeys::assign`]) instead of selecting or
//!   using it; Enter does the same for the selected row. Letting go of the key hides it.
//! - **The line under it**: what the held slot holds ("Empty"), or what the last click
//!   did ("Can't hotkey Wonderglue", "Key 2 is Ammo Swap").
//!
//! Log lines: `pipboy hotkeys: key <n> held`, `pipboy hotkeys: let go`,
//! `hotkeys: slot <n> holds <name> (<form>)`.

use super::frame::{Align, Ctx, Kit, LINE, MAIN, THICK};
use crate::bindings::Bindings;
use crate::hotkeys::{Hotkeys, Refused, Slot, AMMO_SWAP};
use crate::interface::{amber, at};
use crate::menu::model::Row;
use crate::sim::SimState;
use bevy::prelude::*;
use fnv_sim::forms::ItemKind;
use fnv_sim::FormId;

/// The panel inside the menus' rectangle: 290 × 250 at 52 down [documented:
/// `IM_HotKeyWheel`], but 420 in rather than the file's 380, 10 clear of the list's rows
/// (32 + 393 from its left at -15), where key 1's box met the selected row [design: the
/// owner 2026-10-27, "the hotkeys need moving right so they dont overlap the list"; further
/// right, the line under it runs into the STR card].
pub const PANEL: Rect = Rect { min: Vec2::new(420.0, 52.0), max: Vec2::new(710.0, 302.0) };
/// A slot's box, its border's inset and the Ammo Swap picture's side [documented: hotkey.xml,
/// hotkeys.xml].
const BOX: f32 = 60.0;
const INSET: f32 = 5.0;
const SWAP_SIDE: f32 = 50.0;
/// The chosen box's fill: alpha 80 of 255 [documented: `HK_SelectionBox` `_fill_alpha`].
const CHOSEN_ALPHA: f32 = 80.0 / 255.0;
/// The compass rose and the icon in its middle [documented: `IM_CompassRose`, `IM_HotkeyIcon`].
const ROSE: &str = "Interface\\HotKeys\\hot_keys_rose.dds";
const ROSE_AT: Rect = Rect { min: Vec2::new(100.0, 80.0), max: Vec2::new(250.0, 230.0) };
const CENTRE_ICON: Rect = Rect { min: Vec2::new(120.5, 102.0), max: Vec2::new(170.5, 152.0) };
/// The line under the panel, centred, font 2 [documented: `IM_HotKeyText`].
const TEXT_AT: Vec2 = Vec2::new(150.0, 262.0);
const TEXT_FONT: usize = 2;
const SWAP: &str = "Interface\\Icons\\typeicons\\hot_keys_icon_ammoswap.dds";

/// [design, #2312; this module] The held key or controller's chosen slot and last result.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct HotkeyHold {
    /// [design / file refs: track, controller] The chosen slot (0 is key 1).
    pub slot: Option<usize>,
    /// [design / file refs: panel_line] The last result while the panel is open.
    pub said: Option<String>,
    /// The test aid's held key, in place of the keyboard's (`super::test_aid`).
    pub aid: Option<usize>,
    /// [design, #2312] The row RB opened the modal for; pointer/list changes cannot retarget it.
    pub controller: Option<Row>,
}

/// [design, #2312] Provisional order until observer check 211: right/down advance,
/// left/up go back, wrapping through all eight boxes including Ammo Swap.
const PAD_SLOT_COUNT: usize = 8;

impl HotkeyHold {
    pub fn open_controller(&mut self, row: &Row) {
        self.controller = Some(row.clone());
        self.slot = Some(0);
        self.said = None;
        info!("pipboy hotkeys: controller opened, key 1 chosen");
    }

    fn close_controller(&mut self) {
        self.controller = None;
        self.slot = None;
        self.said = None;
        info!("pipboy hotkeys: controller closed");
    }

    fn step_controller(&mut self, forward: bool) {
        if let Some(slot) = self.slot.as_mut().filter(|_| self.controller.is_some()) {
            *slot = (*slot + if forward { 1 } else { PAD_SLOT_COUNT - 1 }) % PAD_SLOT_COUNT;
            self.said = None;
            info!("pipboy hotkeys: controller key {} chosen", *slot + 1);
        }
    }

    fn assign_controller(&mut self, hotkeys: &mut Hotkeys, deselect: bool) {
        let (Some(row), Some(slot)) = (self.controller.as_ref(), self.slot) else { return };
        self.said = Some(assign_row(hotkeys, slot, row, deselect));
        if slot != AMMO_SWAP && row.kind.is_some_and(crate::hotkeys::can_hold) {
            self.close_controller();
        }
    }
}

/// [design, #2312] Read pad edges before either ITEMS input path. KeysTaken also
/// protects the closing frame; pad::menu_keys withholds synthetic keys for this modal.
#[allow(clippy::too_many_arguments)]
pub fn controller(
    pip: Res<super::PipBoy>,
    lists: (Res<super::repair::PipRepair>, Res<super::item_mod::PipItemMod>),
    active: Res<crate::menu::ActiveMenu>,
    pads: crate::pad::ActionPads,
    bindings: Option<Res<Bindings>>,
    paused: Option<Res<crate::pause::Paused>>,
    script: Option<Res<crate::script::ScriptState>>,
    mut hold: ResMut<HotkeyHold>,
    mut hotkeys: ResMut<Hotkeys>,
    mut taken: ResMut<super::KeysTaken>,
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
) {
    let model = active.0.as_ref().filter(|m| m.mode == crate::menu::model::Mode::Inventory && m.quantity.is_none());
    let on = bindings.as_ref().is_some_and(|b| b.device == crate::bindings::InputDevice::Controller);
    let modal = script.as_ref().and_then(|s| s.rt.as_ref()).and_then(|rt| rt.game.top_menu()).is_some_and(|m| m != fnv_game::game::menus::INVENTORY);
    let available = on && pip.open() && pip.page == super::Page::Items && !lists.0.is_open() && !lists.1.is_open() && model.is_some() && !paused.is_some_and(|p| p.0) && !modal;
    if !available {
        if hold.controller.is_some() {
            hold.close_controller();
        }
        return;
    }
    let pressed = |b| crate::pad::menu_button(&pads, bindings.as_deref(), b);
    if hold.controller.is_none() {
        if pressed(GamepadButton::RightTrigger) {
            taken.take();
            if let Some(row) = model.and_then(|m| m.selected_row(crate::menu::model::Side::Player)).filter(|r| r.folder.is_none()) {
                hold.open_controller(row);
            }
        }
        return;
    }
    taken.take();
    if pressed(GamepadButton::East) {
        hold.close_controller();
        return;
    }
    for (button, forward) in [(GamepadButton::DPadLeft, false), (GamepadButton::DPadUp, false), (GamepadButton::DPadRight, true), (GamepadButton::DPadDown, true)] {
        if pressed(button) {
            hold.step_controller(forward);
        }
    }
    if pressed(GamepadButton::South) {
        hold.assign_controller(&mut hotkeys, tweaks.is_some_and(|t| t.deselect_hotkeys));
    }
}

/// A slot box's top left inside the panel [documented: hotkeys.xml's `x`/`y` traits on a
/// panel of 290 × 250].
pub fn box_at(slot: usize) -> Vec2 {
    let (w, h) = (PANEL.width(), PANEL.height());
    let (left, mid, right) = (0.0, (w - BOX) / 2.0, w - BOX);
    let (top, middle, bottom) = (0.0, (h - BOX) / 2.0, h - BOX);
    match slot {
        0 => Vec2::new(left, top),
        1 => Vec2::new((w - SWAP_SIDE) / 2.0, top),
        2 => Vec2::new(right, top),
        3 => Vec2::new(left, middle),
        4 => Vec2::new(right, middle),
        5 => Vec2::new(left, bottom),
        6 => Vec2::new(mid, bottom),
        _ => Vec2::new(right, bottom),
    }
}

/// The hotkey type icon of an item [observed: the files in `textures\interface\icons\typeicons`;
/// the original's boxes show these, not the items' own pictures (owner's screenshot)].
pub fn type_icon(kind: ItemKind, skill: Option<&str>) -> &'static str {
    match (kind, skill.unwrap_or("")) {
        (ItemKind::Weapon, "EnergyWeapons") => "Interface\\Icons\\typeicons\\hot_keys_icon_energyweapons.dds",
        (ItemKind::Weapon, "MeleeWeapons") => "Interface\\Icons\\typeicons\\hot_keys_icon_meleeweapons.dds",
        (ItemKind::Weapon, "Unarmed") => "Interface\\Icons\\typeicons\\hot_keys_icon_unarmedweapons.dds",
        (ItemKind::Weapon, "Explosives") => "Interface\\Icons\\typeicons\\hot_keys_icon_throwweapons.dds",
        (ItemKind::Weapon, "BigGuns") => "Interface\\Icons\\typeicons\\hot_keys_icon_bigguns.dds",
        (ItemKind::Weapon, _) => "Interface\\Icons\\typeicons\\hot_keys_icon_smallguns.dds",
        (ItemKind::Armor, _) => "Interface\\Icons\\typeicons\\hot_keys_icon_bodywear.dds",
        (ItemKind::Book, _) => "Interface\\Icons\\typeicons\\hot_keys_icon_books.dds",
        _ => "Interface\\Icons\\typeicons\\hot_keys_icon_stimpack.dds",
    }
}

/// An item's hotkey icon: its `SetItemHotkeyIconPath` override when set, else its hotkey
/// type icon by kind and (a weapon's) skill [documented: SetItemHotkeyIconPath; observed:
/// the original's boxes show type icons by default].
pub fn icon_of(id: FormId, sim: &SimState, combat: Option<&crate::combat::Combat>, script: Option<&crate::script::ScriptState>) -> Option<String> {
    let kind = sim.forms.item(id)?.kind;
    if let Some(over) = crate::hotkeys::icon_override(id, script) {
        return Some(over);
    }
    let skill = combat.and_then(|c| c.forms.weapons.get(&id)).and_then(|w| w.skill.as_deref());
    Some(type_icon(kind, skill).to_string())
}

/// The resolved icon paths currently shown on the hotkey panel (the pointed-at centre icon
/// and each non-Ammo-Swap slot box), used to trigger live redraws when
/// `SetItemHotkeyIconPath` updates an icon while the panel is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelIcons {
    pub centre: Option<String>,
    pub slots: [Option<String>; crate::hotkeys::SLOTS],
}

/// Resolve the hotkey panel's centre icon and eight slot icons for change tracking and tests.
pub fn panel_icons(hotkeys: &Hotkeys, hold: &HotkeyHold, pointed: Option<&Row>, sim: &SimState, combat: Option<&crate::combat::Combat>, script: Option<&crate::script::ScriptState>) -> PanelIcons {
    let pointed = hold.controller.as_ref().or(pointed);
    let centre = pointed.and_then(|r| icon_of(r.item, sim, combat, script));
    let slots = std::array::from_fn(|slot| {
        if slot == AMMO_SWAP {
            None
        } else {
            hotkeys.0[slot].and_then(|s| icon_of(FormId(s.item), sim, combat, script))
        }
    });
    PanelIcons { centre, slots }
}

/// The slot whose key is held now, by the player's bindings (keys only).
fn held_slot(bindings: &Bindings, keys: &ButtonInput<KeyCode>) -> Option<usize> {
    crate::hotkeys::CONTROLS.iter().position(|&c| bindings.get(c).key.is_some_and(|k| keys.pressed(k)))
}

/// Follow the held key while the ITEMS page shows (the repair list and mod menu have the
/// keys while one is open).
pub fn track(
    pip: Res<super::PipBoy>,
    (repair, modding): (Res<super::repair::PipRepair>, Res<super::item_mod::PipItemMod>),
    keys: crate::pad::ActionButtons<KeyCode>,
    bindings: Option<Res<Bindings>>,
    mut hold: ResMut<HotkeyHold>,
) {
    let keys = keys.snapshot();
    if hold.controller.is_some() {
        return;
    }
    // Laid out in the Pip-Boy's screen texture, at its size whatever the UI scale (#1631).
    let _canvas = crate::interface::original_units();
    let items = pip.open() && pip.page == super::Page::Items && !repair.is_open() && !modding.is_open();
    let now = if items { hold.aid.or_else(|| bindings.as_ref().and_then(|b| held_slot(b, &keys))) } else { None };
    if now != hold.slot {
        match now {
            Some(s) => info!("pipboy hotkeys: key {} held", s + 1),
            None => info!("pipboy hotkeys: let go"),
        }
        hold.slot = now;
        hold.said = None;
    }
}

/// Put a row's item in the held slot; returns the line the panel then shows.
pub fn assign_row(hotkeys: &mut Hotkeys, slot: usize, r: &Row, deselect: bool) -> String {
    let s = Slot { item: r.item.0, mods: r.mods };
    // The player's tweak: the slot it has already takes it off [design: docs/design/tweaks.md].
    if deselect && hotkeys.clear_if_holding(slot, s) {
        info!("hotkeys: slot {} emptied ({} was in it)", slot + 1, r.name);
        return String::new();
    }
    match hotkeys.assign(slot, s, r.kind) {
        Ok(from) => {
            let moved = from.map_or_else(String::new, |f| format!(" (moved from slot {})", f + 1));
            info!("hotkeys: slot {} holds {} ({}){moved}", slot + 1, r.name, r.item);
            r.name.clone()
        }
        Err(Refused::AmmoSwap) => {
            info!("hotkeys: slot 2 is Ammo Swap: {} not assigned", r.name);
            "Key 2 is Ammo Swap".into()
        }
        Err(Refused::NotUsable) => {
            info!("hotkeys: {} cannot be hotkeyed", r.name);
            format!("Can't hotkey {}", r.name)
        }
    }
}

/// The panel, at the menus' rectangle's `IM_HotKeyWheel`.
#[allow(clippy::too_many_arguments)]
pub fn draw(p: &mut ChildSpawnerCommands, c: &mut Ctx, hotkeys: &Hotkeys, hold: &HotkeyHold, pointed: Option<&Row>, sim: &SimState, combat: Option<&crate::combat::Combat>, script: Option<&crate::script::ScriptState>) {
    let pointed = hold.controller.as_ref().or(pointed);
    let o = MAIN.min + PANEL.min;
    let (line, bright) = (amber(LINE), c.bright());
    let rose = Rect::from_corners(o + ROSE_AT.min, o + ROSE_AT.max);
    c.image(p, ROSE, at(rose.min.x, rose.min.y, rose.width(), rose.height()), bright);
    if let Some(icon) = pointed.and_then(|r| icon_of(r.item, sim, combat, script)) {
        let r = Rect::from_corners(o + CENTRE_ICON.min, o + CENTRE_ICON.max);
        c.image(p, &icon, at(r.min.x, r.min.y, r.width(), r.height()), bright);
    }
    for (slot, &key) in hotkeys.0.iter().enumerate() {
        let b = o + box_at(slot);
        if slot == AMMO_SWAP {
            if hold.slot == Some(slot) {
                c.boxed(p, Rect::from_corners(b, b + Vec2::splat(SWAP_SIDE)), line, Some(bright.with_alpha(CHOSEN_ALPHA)));
            }
            c.image(p, SWAP, at(b.x, b.y, SWAP_SIDE, SWAP_SIDE), bright);
            continue;
        }
        if hold.slot == Some(slot) {
            c.boxed(p, Rect::from_corners(b, b + Vec2::splat(BOX)), line, Some(bright.with_alpha(CHOSEN_ALPHA)));
        }
        let inner = Rect::from_corners(b + Vec2::splat(INSET), b + Vec2::splat(BOX - INSET));
        c.boxed(p, inner, line, None);
        if let Some(icon) = key.and_then(|s| icon_of(FormId(s.item), sim, combat, script)) {
            c.image(p, &icon, at(inner.min.x + THICK, inner.min.y + THICK, inner.width() - 2.0 * THICK, inner.height() - 2.0 * THICK), bright);
        }
        // The number at the box's right foot: its bottom 5 under the box's [documented:
        // `HK_Number` y = height - its height + 5].
        let h = c.line_height(TEXT_FONT);
        c.text(p, TEXT_FONT, &(slot + 1).to_string(), b.x + BOX, b.y + BOX - h + INSET, Align::Left, bright);
    }
    if let Some(s) = panel_line(hotkeys, hold, sim) {
        c.text(p, TEXT_FONT, &s, o.x + TEXT_AT.x, o.y + TEXT_AT.y, Align::Centre, bright);
    }
}

/// Marks the panel drawn over the ITEMS page while that page is drawn from its file.
#[derive(Component)]
pub struct PanelOverTiles;

/// The panel over the ITEMS page drawn from its file (`crate::tiles::inventory_menu`), which has no
/// hand-built body to hold it: the same panel at the same place, the owner's [design: the file's own
/// wheel was not captured and is not drawn]. In the Pip-Boy's image, at the page's level.
#[allow(clippy::too_many_arguments)]
pub fn draw_over_tiles(
    mut commands: Commands,
    screen: Option<Res<super::PipBoyScreen>>,
    tiles: Option<Res<crate::tiles::inventory_menu::InventoryTiles>>,
    roots: Query<Entity, With<PanelOverTiles>>,
    (hold, hotkeys, active): (Res<HotkeyHold>, Res<Hotkeys>, Res<crate::menu::ActiveMenu>),
    (sim, combat, script): (Res<SimState>, Option<Res<crate::combat::Combat>>, Option<Res<crate::script::ScriptState>>),
    mut kit: ResMut<Kit>,
    data: Option<Res<nvf_assets::GameData>>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<bevy::image::CompressedImageFormatSupport>>,
    glyphs: Res<crate::pad::PadGlyphs>,
    mut drawn_icons: Local<Option<PanelIcons>>,
) {
    // Laid out in the Pip-Boy's screen texture, at its size whatever the UI scale (#1631).
    let _canvas = crate::interface::original_units();
    let slot = hold.slot.filter(|_| tiles.is_some_and(|t| t.ready.is_some()));
    let (Some(slot), Some(screen)) = (slot, screen) else {
        *drawn_icons = None;
        for e in &roots {
            commands.entity(e).despawn();
        }
        return;
    };
    let pointed = active.0.as_ref().and_then(|m| m.selected_row(crate::menu::model::Side::Player));
    let icons = panel_icons(&hotkeys, &hold, pointed, &sim, combat.as_deref(), script.as_deref());
    if !roots.is_empty() && !hold.is_changed() && !hotkeys.is_changed() && !active.is_changed() && drawn_icons.as_ref() == Some(&icons) {
        return;
    }
    *drawn_icons = Some(icons);
    for e in &roots {
        commands.entity(e).despawn();
    }
    info!("pipboy hotkeys: panel drawn, key {} chosen{}", slot + 1, hold.said.as_ref().map_or_else(String::new, |s| format!(", says {s:?}")));
    let mut c = Ctx::new(&mut kit, data.as_deref(), &mut images, formats.as_deref(), &glyphs);
    let root = Node { overflow: Overflow::clip(), ..at(0.0, 0.0, super::CANVAS.x, super::CANVAS.y) };
    commands
        .spawn((PanelOverTiles, root, UiTargetCamera(screen.camera), GlobalZIndex(crate::tiles::draw::PIPBOY_TILES_Z), Pickable::IGNORE, Name::new("Hotkey panel over the page")))
        .with_children(|p| draw(p, &mut c, &hotkeys, &hold, pointed, &sim, combat.as_deref(), script.as_deref()));
}

/// The line under the panel: what the last click did, else what the held slot holds.
pub fn panel_line(hotkeys: &Hotkeys, hold: &HotkeyHold, sim: &SimState) -> Option<String> {
    hold.said.clone().or_else(|| {
        let slot = hold.slot?;
        if slot == AMMO_SWAP {
            return Some("Ammo Swap".into());
        }
        Some(hotkeys.0[slot].map_or_else(|| "Empty".into(), |s| sim.name(FormId(s.item)).unwrap_or("?").to_string()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(kind: ItemKind) -> Row {
        Row { item: FormId(1), at: None, name: "Test tool".into(), count: 1, weight: None, value: None, kind: Some(kind), equipped: false, quest: false, condition: None, icon: None, price: None, bartered: false, loaded: false, dim: false, mods: 3, mods_value: 0, folder: None, reference: None, live: false, ysi: None }
    }

    #[test]
    fn controller_navigation_visits_all_eight_slots_and_wraps_both_ways() {
        let mut hold = HotkeyHold::default();
        hold.open_controller(&row(ItemKind::Weapon));
        for slot in (1..8).chain([0]) {
            hold.step_controller(true);
            assert_eq!(hold.slot, Some(slot));
        }
        for slot in (0..8).rev() {
            hold.step_controller(false);
            assert_eq!(hold.slot, Some(slot));
        }
    }

    #[test]
    fn controller_assignment_moves_the_same_modified_item_and_closes() {
        let (mut hold, mut keys) = (HotkeyHold::default(), Hotkeys::default());
        let item = row(ItemKind::Weapon);
        hold.open_controller(&item);
        hold.assign_controller(&mut keys, false);
        assert_eq!(keys.0[0], Some(Slot { item: 1, mods: 3 }));
        assert!(hold.controller.is_none());
        hold.open_controller(&item);
        hold.step_controller(true);
        hold.step_controller(true);
        hold.assign_controller(&mut keys, false);
        assert_eq!(keys.0[0], None);
        assert_eq!(keys.0[2], Some(Slot { item: 1, mods: 3 }));
        assert_eq!(hold.slot, None);
    }

    #[test]
    fn controller_refusals_and_cancel_leave_assignments_alone() {
        let (mut hold, mut keys) = (HotkeyHold::default(), Hotkeys::default());
        hold.open_controller(&row(ItemKind::Weapon));
        hold.step_controller(true);
        hold.assign_controller(&mut keys, false);
        assert_eq!(hold.said.as_deref(), Some("Key 2 is Ammo Swap"));
        assert!(hold.controller.is_some());
        hold.close_controller();
        assert_eq!(keys, Hotkeys::default());
        hold.open_controller(&row(ItemKind::Misc));
        hold.assign_controller(&mut keys, false);
        assert_eq!(hold.said.as_deref(), Some("Can't hotkey Test tool"));
        hold.close_controller();
        assert_eq!(keys, Hotkeys::default());
    }

    #[test]
    fn controller_assignment_honours_deselect_tweak() {
        let (mut hold, mut keys) = (HotkeyHold::default(), Hotkeys::default());
        let item = row(ItemKind::Armor);
        for _ in 0..2 {
            hold.open_controller(&item);
            hold.assign_controller(&mut keys, true);
            assert!(hold.controller.is_none());
        }
        assert_eq!(keys, Hotkeys::default());
    }

    // [design, #2312] Synthetic inventory and pad events exercise the scheduled reader
    // followed by the ordinary ITEMS keys, checking that modal actions do not escape.
    #[derive(Resource, Default)]
    struct EscapedCommands(usize);

    fn record_commands(mut menu: MessageReader<crate::menu::MenuInput>, mut pages: MessageReader<super::super::PipBoyInput>, mut seen: ResMut<EscapedCommands>) {
        seen.0 += menu.read().count() + pages.read().count();
    }

    fn controller_app() -> (App, Entity) {
        use fnv_sim::forms::{ActorBase, BaseData, BaseForm, FormStore};
        let mut forms = FormStore::default();
        forms.bases.insert(FormId::PLAYER_BASE, BaseForm {
            id: FormId::PLAYER_BASE, sig: *b"NPC_", edid: None, name: None, script: None,
            prompt: None, voice_filter: false,
            data: BaseData::Actor(ActorBase { entries: Vec::new(), attributes: Some([5; 7]) }),
        });
        let refs = fnv_sim::RefTable::default();
        let mut world = fnv_sim::World::new(&forms, &refs, fnv_sim::SimConfig::default());
        let mut model = crate::menu::model::MenuModel::open_inventory(&mut world).unwrap();
        model.category[crate::menu::model::Side::Player as usize] = super::super::items::TABS[0];
        model.lists[crate::menu::model::Side::Player as usize].rows = vec![row(ItemKind::Weapon)];
        let mut bindings = Bindings::default();
        bindings.device = crate::bindings::InputDevice::Controller;
        let mut app = App::new();
        app.add_plugins(bevy::input::InputPlugin)
            .insert_resource(bindings)
            .insert_resource(super::super::PipBoy { phase: super::super::Phase::Up, page: super::super::Page::Items, ..default() })
            .insert_resource(crate::menu::ActiveMenu(Some(model)))
            .init_resource::<HotkeyHold>()
            .init_resource::<Hotkeys>()
            .init_resource::<super::super::KeysTaken>()
            .init_resource::<super::super::repair::PipRepair>()
            .init_resource::<super::super::item_mod::PipItemMod>()
            .init_resource::<crate::menu::PromptKeys>()
            .init_resource::<EscapedCommands>()
            .add_message::<crate::menu::MenuInput>()
            .add_message::<super::super::PipBoyInput>()
            .add_systems(First, super::super::clear_keys_taken)
            .add_systems(Update, (controller, super::super::close_keys, super::super::keys, super::super::items::keys, record_commands).chain());
        let pad = app.world_mut().spawn(Gamepad::default()).id();
        (app, pad)
    }

    fn press(app: &mut App, pad: Entity, button: GamepadButton) {
        use bevy::input::gamepad::{RawGamepadButtonChangedEvent, RawGamepadEvent};
        for down in [true, false] {
            app.world_mut().write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(pad, button, if down { 1.0 } else { 0.0 })));
            app.update();
        }
    }

    #[test]
    fn controller_buttons_assign_and_cancel_without_item_commands() {
        use GamepadButton::*;
        let (mut app, pad) = controller_app();
        press(&mut app, pad, RightTrigger);
        assert_eq!(app.world().resource::<HotkeyHold>().slot, Some(0));
        press(&mut app, pad, DPadDown);
        press(&mut app, pad, South);
        assert!(app.world().resource::<HotkeyHold>().controller.is_some(), "Ammo Swap refuses assignment");
        press(&mut app, pad, DPadRight);
        press(&mut app, pad, South);
        let assigned = app.world().resource::<Hotkeys>().clone();
        assert_eq!(assigned.0[2], Some(Slot { item: 1, mods: 3 }));
        assert!(app.world().resource::<HotkeyHold>().controller.is_none());
        press(&mut app, pad, RightTrigger);
        press(&mut app, pad, DPadLeft);
        assert_eq!(app.world().resource::<HotkeyHold>().slot, Some(7));
        press(&mut app, pad, DPadUp);
        assert_eq!(app.world().resource::<HotkeyHold>().slot, Some(6));
        press(&mut app, pad, East);
        assert!(app.world().resource::<HotkeyHold>().controller.is_none());
        assert_eq!(*app.world().resource::<Hotkeys>(), assigned);
        assert_eq!(app.world().resource::<crate::menu::ActiveMenu>().0.as_ref().unwrap().navigated, 0);
        assert_eq!(app.world().resource::<EscapedCommands>().0, 0);
    }

    #[test]
    fn controller_panel_cancels_on_device_page_and_pause_changes() {
        for change in 0..3 {
            let (mut app, pad) = controller_app();
            press(&mut app, pad, GamepadButton::RightTrigger);
            match change {
                0 => app.world_mut().resource_mut::<Bindings>().device = crate::bindings::InputDevice::KeyboardMouse,
                1 => app.world_mut().resource_mut::<super::super::PipBoy>().page = super::super::Page::Stats,
                _ => { app.insert_resource(crate::pause::Paused(true)); }
            }
            app.update();
            assert!(app.world().resource::<HotkeyHold>().controller.is_none());
            assert_eq!(*app.world().resource::<Hotkeys>(), Hotkeys::default());
        }
    }

    #[test]
    fn the_boxes_sit_as_the_file_places_them() {
        assert_eq!(box_at(0), Vec2::ZERO);
        assert_eq!(box_at(1), Vec2::new(120.0, 0.0));
        assert_eq!(box_at(2), Vec2::new(230.0, 0.0));
        assert_eq!(box_at(3), Vec2::new(0.0, 95.0));
        assert_eq!(box_at(4), Vec2::new(230.0, 95.0));
        assert_eq!(box_at(5), Vec2::new(0.0, 190.0));
        assert_eq!(box_at(6), Vec2::new(115.0, 190.0));
        assert_eq!(box_at(7), Vec2::new(230.0, 190.0));
    }

    #[test]
    fn every_kind_has_a_type_icon() {
        for k in [ItemKind::Weapon, ItemKind::Armor, ItemKind::Aid, ItemKind::Book, ItemKind::Misc] {
            assert!(type_icon(k, None).ends_with(".dds"));
        }
        assert!(type_icon(ItemKind::Weapon, Some("MeleeWeapons")).contains("melee"));
    }

    #[test]
    fn hotkey_icon_override_updates_panel_icons_and_triggers_live_redraw() {
        use fnv_game::game::RunCtx;
        use fnv_sim::forms::{BaseData, BaseForm, FormStore, Item};

        const WEAPON: FormId = FormId(0x0000_434F);
        const AID: FormId = FormId(0x0001_5169);

        let make_item = |kind: ItemKind| Item {
            kind,
            value: Some(10),
            money: None,
            weight: Some(1.0),
            slots: None,
            projectile: None,
            health: None,
            non_playable: false,
            cant_drop: false,
            armor_class: Default::default(),
            power_armor: false,
            armor_dr: None,
            armor_dt: None,
            weapon_damage: None,
            weight_mods: [0.0; 3],
            drawable: false,
            not_for_npcs: false,
            thrown: false,
            quest_item: false,
            icon: Some("Interface\\Icons\\PipboyImages\\Weapons\\test_weap.dds".into()),
            placed_count: None,
            teaches: None,
            cant_be_taken: false,
            modulates_voice: false,
            weapon_traits: None,
        };
        let mut forms = FormStore::default();
        forms.bases.insert(WEAPON, BaseForm { id: WEAPON, sig: *b"WEAP", edid: Some("TestWeap".into()), name: Some("Test Weap".into()), script: None, prompt: None, voice_filter: false, data: BaseData::Item(make_item(ItemKind::Weapon)) });
        forms.bases.insert(AID, BaseForm { id: AID, sig: *b"ALCH", edid: Some("TestAid".into()), name: Some("Test Aid".into()), script: None, prompt: None, voice_filter: false, data: BaseData::Item(make_item(ItemKind::Aid)) });

        let forms: &'static FormStore = Box::leak(Box::new(forms));
        let refs: &'static fnv_sim::RefTable = Box::leak(Box::new(fnv_sim::RefTable::default()));
        let sim = SimState { forms, refs, world: fnv_sim::World::new(forms, refs, fnv_sim::SimConfig::default()) };
        let mut hotkeys = Hotkeys::default();
        hotkeys.0[0] = Some(Slot { item: WEAPON.0, mods: 0 });
        hotkeys.0[3] = Some(Slot { item: AID.0, mods: 0 });
        let pointed = Row { item: WEAPON, ..row(ItemKind::Weapon) };
        let hold = HotkeyHold { slot: Some(0), ..default() };
        let mut script = crate::script::ScriptState::over_no_plugins();

        // Vanilla (no override): panel uses type_icon (unchanged) and slot 2 is None (Ammo Swap).
        assert_eq!(icon_of(WEAPON, &sim, None, None).as_deref(), Some(type_icon(ItemKind::Weapon, None)));
        let baseline = panel_icons(&hotkeys, &hold, Some(&pointed), &sim, None, Some(&script));
        assert_eq!(baseline.centre.as_deref(), Some(type_icon(ItemKind::Weapon, None)));
        assert_eq!(baseline.slots[0].as_deref(), Some(type_icon(ItemKind::Weapon, None)));
        assert_eq!(baseline.slots[AMMO_SWAP], None);
        assert_eq!(baseline.slots[3].as_deref(), Some(type_icon(ItemKind::Aid, None)));

        // Custom SetItemHotkeyIconPath overrides both centre icon and slot box, normalised.
        let run = |script: &mut crate::script::ScriptState, body: &str| {
            script.rt.as_mut().unwrap().game.run_query(&format!("Begin Function {{ }}\n{body}\nEnd\n"), RunCtx::default()).unwrap();
        };
        run(&mut script, &format!(r#"SetItemHotkeyIconPath {} "Textures/Interface/Icons/CustomPanel.dds""#, WEAPON.0));
        let custom = panel_icons(&hotkeys, &hold, Some(&pointed), &sim, None, Some(&script));
        assert_eq!(custom.centre.as_deref(), Some("interface\\icons\\custompanel.dds"));
        assert_eq!(custom.slots[0].as_deref(), Some("interface\\icons\\custompanel.dds"));
        assert_eq!(custom.slots[3].as_deref(), Some(type_icon(ItemKind::Aid, None)));

        // Verify live ECS redraw in draw_over_tiles when SetItemHotkeyIconPath updates and clears.
        let mut app = App::new();
        let camera = app.world_mut().spawn_empty().id();
        let mut tiles = crate::tiles::inventory_menu::InventoryTiles::default();
        tiles.ready = Some(fnv_ui::Ui::new().screen());
        app.insert_resource(super::super::PipBoyScreen { image: Handle::default(), camera, on_model: false, pointer: None, scale: 1.0 })
            .insert_resource(tiles)
            .insert_resource(hold)
            .insert_resource(hotkeys)
            .insert_resource(crate::menu::ActiveMenu(None))
            .insert_resource(sim)
            .insert_resource(script)
            .init_resource::<Kit>()
            .init_resource::<Assets<Image>>()
            .init_resource::<crate::pad::PadGlyphs>()
            .add_systems(Update, draw_over_tiles);

        let root_entity = |app: &mut App| -> Entity {
            let mut q = app.world_mut().query_filtered::<Entity, With<PanelOverTiles>>();
            let entities: Vec<Entity> = q.iter(app.world()).collect();
            assert_eq!(entities.len(), 1);
            entities[0]
        };

        app.update();
        let first = root_entity(&mut app);
        // Idle frame with no change: panel is NOT needlessly respawned.
        app.update();
        assert_eq!(root_entity(&mut app), first);

        // Live SetItemHotkeyIconPath update: panel respawns to show the new icon.
        run(app.world_mut().resource_mut::<crate::script::ScriptState>().as_mut(), &format!(r#"SetItemHotkeyIconPath {} "interface\icons\live_update.dds""#, WEAPON.0));
        app.update();
        let second = root_entity(&mut app);
        assert_ne!(second, first, "draw_over_tiles must redraw when SetItemHotkeyIconPath changes live");

        // Clearing with "" respawns again and restores default type_icon.
        run(app.world_mut().resource_mut::<crate::script::ScriptState>().as_mut(), &format!(r#"SetItemHotkeyIconPath {} """#, WEAPON.0));
        app.update();
        let third = root_entity(&mut app);
        assert_ne!(third, second, "draw_over_tiles must redraw when SetItemHotkeyIconPath clears");
        let after_clear = icon_of(WEAPON, app.world().resource::<SimState>(), None, Some(app.world().resource::<crate::script::ScriptState>()));
        assert_eq!(after_clear.as_deref(), Some(type_icon(ItemKind::Weapon, None)));
    }
}
