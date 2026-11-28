# ySI Menu XML Injection (`menus/ySI/ySI.xml`)

Clean-room specification for loading `menus/ySI/ySI.xml` into NVForever's tile tree
(`crates/fnv-game/src/ui_ysi.rs`, wired into `crates/fnv-game/src/ui_loading.rs`).
No native plugin DLL or C++ source was read.

## 1. Observed XML and UIO contract

- [observed: installed mod XML and UIO manifests] `menus/ySI/ySI.xml` is an XML fragment
  (not a `<menu>` document) declaring:
  - a `<rect name="ySI">` holding sizing, spacing, and UI-mod detection traits (`_ySI`,
    `_ySIBlanks`, `_ySIUIMod`, `_ySIPipBoy`, `_ySIWidth`, `_ySIHeight`, `_ySISpaceUp`,
    `_ySISpaceLeft`, `_ySISpaceRight`), where `_ySIPipBoy` checks
    `parent()`'s `class` against `&InventoryMenu;`;
  - top-level `<template name="ySIDefault">` and `<template name="ySINoTag">` blocks
    defining `<image name="ySIImage">` tiles whose expressions reference `src="ySI"` and
    `sibling(ListItemText)`.
- [observed: installed UIO manifests] Most UIO manifests that integrate with ySI only test
  `#..\ySI\ySI.xml` as a file-existence condition (or inject `ySI.xml` into `RecipeMenu`
  alone). They do not inject `menus/ySI/ySI.xml` into `HUDMainMenu` or the other item-list
  menus; without an injector, `HUDMainMenu/ySI` and the `ySIDefault`/`ySINoTag` templates
  are absent from the tile tree even when `menus/ySI/ySI.xml` is installed.

## 2. NVForever design (`fnv_game::ui_ysi`)

- **Off when absent** [design]: `ui_ysi::is_present` checks whether `menus\ySI\ySI.xml`
  exists in the UI `Source` / VFS. When absent, `ui_loading::load` and `ui_loading::sync`
  perform no injection and leave the tile tree unchanged.
- **Target menus** [hypothesis: deduced from `_ySIPipBoy` checking `parent()`'s `class`
  against `&InventoryMenu;`, HUD XML/script consumers reading `HUDMainMenu/ySI/*`, and
  item-list menus hosting `ySIImage` rows]: when `menus\ySI\ySI.xml` is present and
  non-empty, `ui_ysi::publish_for_menu` injects it directly into the root of:
  `HUDMainMenu`, `InventoryMenu`, `BarterMenu`, `ContainerMenu`, `RepairMenu`,
  `RepairServicesMenu`, `ItemModMenu`, and `RecipeMenu`.
- **Deduplication** [design]: before injecting into a menu root, `publish_for_menu` checks
  whether that root already has a direct child tile named `ySI` or a template named
  `ySIDefault` or `ySINoTag` (case-insensitively). If so (for example, when a UIO manifest
  or loose menu override already injected `menus/ySI/ySI.xml`), injection is skipped so
  neither tiles nor templates are duplicated.
- **Safe template instantiation** [design]: `ui_ysi::instantiate_icon` resolves a named
  template (such as `ySIDefault` or `ySINoTag`) along the ancestor chain, verifies that the
  template body contains at least one tile element before mutating the parent, deduplicates
  against an existing live child of the target name (default `ySIImage`), and returns
  `None` cleanly on blank, missing, or malformed templates without panicking or mutating
  the parent's traits.
- **Row spacing** [documented: Sorting Icons 1.4b package, `menus/ySI/ySI.xml`: the icon's
  `<x>` is "set in code", `user0` is the "additional space left between equip square and
  icon" and `user1` the "additional space left between icon and text"; no yUI/ySI DLL or
  source consulted]: `ui_ysi::apply_row_icon` puts `ySIImage` at the text's base place plus
  `_ySISpaceLeft` and moves `ListItemText` right by `_ySISpaceLeft` + `_ySIWidth` +
  `_ySISpaceRight` (no move for an untagged row with `_ySIBlanks` 0).
  - A literal `ListItemText/x` keeps its file value in the private `_ysi_base_x` and is
    written as base plus the move [design].
  - An expression `ListItemText/x` (VUI+ works it out from the font and the equip square's
    `_SquarePadding`) keeps the file's operators: they move to `_ysi_base_x` and `x`
    becomes `_ysi_base_x` plus the private `_ysi_x_shift`, so the expression still follows
    its sources and the text moves once however often the row is written; a later
    replacement of `x`'s operators is wrapped again [design]. Before, an expression `x` was
    left where it was and the icon drew over the name's first letters.
  - `ui_ysi::remove_row_icon` (icons switched off, `yUI.ini` `bAddInventoryIcons=0`) takes
    the icon off and puts the text back at its base place [design].
  - Without `menus/ySI/ySI.xml` none of this runs: rows keep their file traits.

## 3. Scope boundaries

- [design] This slice covers only XML/template injection and binding in `fnv-game`. It
  does not modify `ysi_filename.rs` or `nvse_small.rs`, does not implement inventory
  sorting or renderer-side icon drawing, and does **not** raise `GetPluginVersion "yUI"`
  to `140` (`docs/spec-notes/ext-version.md` remains truthful).
