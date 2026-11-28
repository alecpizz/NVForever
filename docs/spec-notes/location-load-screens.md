# Spec note: optional location-specific loading screen preference (`location-load-screens`, #3844)

Covers the optional, off-by-default NVForever setting `location-load-screens`
(`Display › Interface` — **Location-specific loading screens**), preferring matching
location-specific `LSCR` records over generic loading screens when entering a cell or
worldspace, per the 2026-11-24 owner ruling on `#3607` (`docs/design/extender-compat.md`
§2 and §8: JIP LN `[GamePatches]` are not read from `jip_nvse.ini`; wanted patches become
individual named NVForever options, off by default).

## 1. Documented plugin behaviour

[documented: **JIP LN NVSE Plugin v57.30** (`https://www.nexusmods.com/newvegas/mods/58277`),
**JIP LN Settings INI v56.24** public `jip_nvse.ini` `[GamePatches]` section comment; no
plugin DLL, PDB, or third-party source code was consulted]

> `bLoadScreenFix=0` (comment text; optional INI ships `0` or `1` depending on user config)
> `; Forces the load menu to give priority to location-specific load screens (if any are found to match current location)`
> `; when selecting a load screen to display.`

NVForever does **not** read `jip_nvse.ini` for this option; it exposes a named NVForever
setting that defaults to **Off** (`false`) so unmodded vanilla loading-screen selection
(mixing location-specific and generic screens) is unchanged unless the player opts in.

## 2. NVForever design choices

All items below are NVForever's own engineering choices (`[design]`, not behaviour claimed
from JIP LN's plugin) or unobserved edge cases (`[hypothesis]`):

- **[design] Setting identity, placement, and default:** named NVForever switch
  `location-load-screens` (**Location-specific loading screens**), registered in
  `engine/nvf-play/src/loading.rs` (`LocationLoadScreens` resource, synced live into
  `Loading::prefer_location` by `sync_location_load_screens`), placed under the existing
  `Display › Interface` category in `crates/nvf-settings-layout/src/lib.rs` (after
  `crosshair-style`, before the scale sliders), persisted in `NVForever.ini` `[Settings]`,
  and **off by default** (`false`).
- **[design] Deterministic selection among multiple matching screens and script-forced
  generic screens:** `fnv_world::loadscreen::choose_with` selects from matching
  location-specific `LSCR` records using `own[(roll % own.len() as u64) as usize]` when
  `prefer_location` is `true` and at least one location-specific screen matches `to`. When
  a script calls `EnableLoadingMenu`, `Loading::generic_next` passes `to = None`
  (`anywhere`), so no location-specific screen matches and generic loading screens remain
  selected as documented for `EnableLoadingMenu`.
- **[hypothesis] Generic fallback and tiebreak under JIP LN:** the public `jip_nvse.ini`
  comment says "give priority to location-specific load screens (if any are found to match
  current location)" without specifying the exact random distribution across multiple
  matching screens or confirming whether zero-match locations fall back directly to generic
  screens; falling back to generic screens when no location-specific screen matches is
  inferred from "(if any are found to match)" and has not been separately observed in the
  original game with JIP LN loaded.
