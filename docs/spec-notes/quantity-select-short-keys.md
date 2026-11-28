# Spec note: optional quantity-select shortcuts (`quantity-select-short-keys`, #3799)

Covers the optional, off-by-default NVForever setting `quantity-select-short-keys`
(`Gameplay › Menus`), implementing the player-facing Shift/Ctrl quantity-select shortcuts
and weightless container quantity prompt per the 2026-11-24 owner ruling on `#3607`
(`docs/design/extender-compat.md` §2 and §8: JIP LN `[GamePatches]` are not read from
`jip_nvse.ini`; wanted patches become individual named NVForever options, off by default).

## 1. Documented plugin behaviour

[documented: **JIP LN NVSE Plugin v57.30** (`https://www.nexusmods.com/newvegas/mods/58277`),
public `jip_nvse.ini` `[GamePatches]` section comment; no plugin DLL or source code was
consulted]

> `bQttSelectShortKeys=1`
> `; If enabled, (a) when selecting an item stack in either the inventory, container, or barter menus, holding SHIFT will transfer/drop the entire stack, whilst holding CTRL will transfer/drop a single item from the stack - thus skipping the quantity-select menu. (b) Picking up from a container a stack of items that are weightless (such as ammo on non-hardcore mode) will bring up the quantity-select menu, instead of automatically picking the entire stack.`

JIP LN ships `bQttSelectShortKeys=1` (enabled by default in `jip_nvse.ini`). NVForever
does **not** read `jip_nvse.ini` for this option; it exposes a named NVForever setting
that defaults to **Off** (`false`) so unmodded vanilla behaviour is unchanged unless the
player opts in.

## 2. NVForever design choices

All items below are NVForever's own engineering choices (`[design]`, not behaviour claimed
from JIP LN's plugin) or unobserved edge cases (`[hypothesis]`):

- **[design] Setting identity, placement, and default:** named NVForever switch
  `quantity-select-short-keys` (`Quantity select shortcuts`), registered in
  `engine/nvf-play/src/menu.rs` (`QuantitySelectShortKeys` resource), placed under
  `Gameplay › Menus` in `crates/nvf-settings-layout/src/lib.rs`, persisted in
  `NVForever.ini` `[Settings]`, and **off by default** (`false`).
- **[design] Shift-over-Ctrl precedence:** when both Shift and Ctrl are held at the same
  time while the option is enabled, Shift takes precedence (full stack) over Ctrl (single
  item).
- **[design] Input-route coverage:**
  - **Container and Barter menus:** Enter/Space (`menu_keys`), hand-built row click
    (`menu_mouse`), and tiled row click (`tiles::input::follow_item_menu` ->
    `item_menu::row_commands_with`) emit `Command::Stack` with Shift and
    `Command::OneSingle` with Ctrl (when the option is on). Both bypass `asks_quantity`.
  - **Inventory menu:** tiled Drop button / R key (`tiles::inventory_menu::tiled_drop_command`),
    Q drop key (`pipboy::items::keys`), and right-click row drop (both `pipboy::items::on_row_click`
    and `tiles::inventory_menu::feed`) emit `Command::DropStack` with Shift and
    `Command::DropSingle` with Ctrl (when the option is on). When the option is off, the
    tiled Drop button and Q key always emit `Command::Drop` and Ctrl has no override on any
    route.
- **[design] Weightless container pickup rule & threshold:** `MenuModel::asks_quantity`
  keeps NVForever's existing `ASK_QUANTITY_ABOVE = 5` threshold for all ordinary transfers
  and drops. When `quantity-select-short-keys` is enabled and the player takes a stack from
  a container (`Mode::Container(_)`, `Side::Container`, `Command::One`), any stack with
  `count > 1` whose effective player weight is zero (ammo when Hardcore ammo weight is off,
  kinds covered by `WeightRules::weightless`, or items with weight `<= 0` / none) opens
  `QuantityMenu` with `amount = max = r.count`. Storing a weightless stack from the
  player into a container keeps the normal `ASK_QUANTITY_ABOVE` rule.
- **[hypothesis] Exact original-game threshold and modifier precedence:** the public
  `jip_nvse.ini` comment does not specify the Shift+Ctrl simultaneous-press precedence or
  exact stack-size threshold (`> 1` vs `> 5`) for weightless container items; these are not
  separately observed in the original game with JIP LN loaded.
