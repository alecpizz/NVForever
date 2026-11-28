# The small extender plugins, CacheUI's and Anh's last (2026-11-01)

Built in `crates/fnv-game/src/nvse_small.rs` (ArrayToString in the VM,
`crates/fnv-script/src/vm/anh.rs`): the plugins of #1020 (BookMenu, ButcherPete,
ClimateControl, Command Extender, HotReload, Overcharge, PostalOut, TTW, Stewie's Tweaks,
hNVSE, yUI, and the catalogue's "Sandbox" row), CacheUI's last eight (#1013), Anh's last
three (#1015, ext-anh.md) and NX_GetQVEVFL (#1017, ext-nx.md). Sources: the GECK wiki
export [documented: the page named]. None of these plugins is installed in the probe
game, so nothing here is `[observed]`. Changes last for the session.

## 1. Anh and NX

| Function | NVForever |
|---|---|
| ArrayToString `array` | PrintVar's text of the array (`[1, 2]`, a map's `key: value`) [documented: "Returns the specified array variable as a string"; decision: the form, the page shows none]; "" for no array [hypothesis] |
| GetCameraFOV `1stPerson` | the last SetCameraFOV's value, else the one the player set on the settings page (Field of view, Viewmodel field of view; `--fov`, `--viewmodel-fov`), which the engine tells the game state at start and after each change [decision: it stands for the original's INI values, which its FOV setting writes], else the INI's fDefaultWorldFOV / fDefault1stPersonFOV ([Display]), else 75 / 55 [documented: "Returns the FOV value for the camera"; hypothesis: the defaults] |
| UpdateTubeMesh (QuitMeshinWithMeAndUpdateAlready) | 0 [decision: NVForever builds no tube mesh] |
| NX_GetQVEVFL | 0 [decision: the page gives no arguments and leaves its use to an outside tutorial] |

## 2. CacheUI

| Function | NVForever |
|---|---|
| GetFPS | 1 / the last frame's seconds [documented: "Not 100% Accurate"] |
| SetTileScript `path udf`, RemoveTileScript `path` | the UDF runs each frame while the trait is not 0; one per trait [documented] |
| SetFrameControllerLoop `udf set [time] [frames]` (SFCL) | registered or removed; every `time` seconds the call waits `frames` frames, the timer starting again at once [documented]; real frame seconds, menus included; one loop per UDF [hypothesis] |
| SetUITypewriter `path text time [fade] [mode] [invert]` | the letters into `_TS0…` at once (at most 260); the alphas `_TA0…` from 0 to 255, letter `i` starting at its place's share of the duration and taking `fade`; modes 0/1 from the left, 2/3 from the middle, 5/6 shuffled; 1, 3 and 6 go out again; invert mirrors the order (5/6: fades out instead); no duration stops and sets the alphas to 0 [documented: the mode tables, "the entire duration … is always duration + fade"]. The out modes come in over the first half and go out over the second, and the shuffle has a fixed seed [hypothesis; decision]. The clock is real seconds, menus included: each frame's own length added up (`SmallState::real_seconds`), as the engine gives every frame its own `frame_seconds` [decision] |
| SetFOVRatio `current strength` | kept for the aiming field of view [documented: the formula; follow-up: the engine's camera does not read it] |
| SplitStringToRef `text symbol` | the parts by editor ID, `player`, or `Plugin.esm:hex`, in order; a part that names nothing is left out [documented; hypothesis: left out] |
| WriteToXML | 0, nothing written [decision: NVForever never writes into the game folder, and the change only shows after a restart] |

## 3. The small plugins

| Function | NVForever |
|---|---|
| Sandbox | 0 [documented: "This is NOT a function"; it is in the catalogue as a page] |
| ShowBookMenu `[showTake] [book]` | on a reference, `Activate player` [documented: "behaves similarly to 'bookref.activate player'"; decision: no Take choice, the engine's book screen has none] |
| GetButcherPeteVersion | 0 [documented: "if the mod isn't installed it will simply return 0"] |
| GetLockedAlt | a terminal: 0 open (no lock data, a script unlocked it, hacked), 2 locked out (a failed hack, a Requires Key level), else 1; anything else as GetLocked [documented: the table; hypothesis: the password note is not checked] |
| GetQuestFailedAlt `quest` | GetQuestFailed [documented] |
| IsKeyPressedAlt `code` | IsKeyPressed counting disabled keys [documented] |
| GetOutgoingWeather | the weather the sky showed before the one it shows (`WeatherState::outgoing`, set when the engine tells a change) [documented: "Returns the outgoing weather"; hypothesis: kept after the change ends] |
| FakeQuantityMenu | nothing [documented: "for Fallout 3 only"] |
| GetCommandExtenderVersion, GetRockItAmmoCount | 0 [decision: a Fallout 3 plugin; New Vegas has no Rock-It Launcher] |
| GetSelectedItemBaseRef | GetSelectedItemRef's base form [documented] |
| PrintActiveUIComponentFullName | GetActiveUIComponentFullName to the console [documented] |
| ToggleContainerMenuItemTransfer, ToggleItemQuantitySuffix `state`; ToggleMuzzleFlashLights `[state]` | kept (1 off); the last switches without a state [documented; hypothesis: 1 means off; follow-up: the engine's menus and lights do not read them] |
| GetGameHotReloaded, ToGECK | 0 [decision: NVForever does not hot-reload scripts and has no GECK beside it] |
| GetOCSettings / SetOCSettings `trait [value]` | what SetOCSettings put (traits 1-12), else 0 [decision: Overcharge's files are not read] |
| GetOCWeaponConfig | 0 [decision: the same] |
| GetOCWeaponState / SetOCWeaponState `weapon trait [value]` | by actor, weapon and trait (1-19); the setter returns the value [hypothesis] |
| CreateOCLightClone `light actor` | a TempCloneForm copy of the light [documented: "Returns with the cloned light form"; decision: its colour stays, no Overcharge heat is kept] |
| IsPlayerIdlePlaying `idle` | the player plays the idle (one idle per actor, both views) [documented] |
| TTW_CRC32 `plugin` | the CRC-32 (IEEE) of the plugin file; 0 when it cannot be read [documented; decision: the variant]. Table-driven, and kept per plugin name for the session (a plugin does not change while the game runs; FalloutNV.esm is too large to read on every call) [decision] |
| TTW_GetEquippedWeaponSkill `skill` | 1 when the equipped weapon's skill (DNAM, an actor value code) is that one [documented] |
| TTW_GetPlugins | `[name, version, description]` per loaded plugin [documented]: the version is the TES4 HEDR version (as GetPluginHeaderVersion), the description "" [decision]. Until 2026-11-08 the version was 0, and TTW's start-up check (a plugin below 1.3 is an unconverted Fallout 3 mod) showed its "Fallout 3 mods installed" message for every load order |
| TTW_ShowGeneProjector | opens the race menu (1036), NVForever's character creator, as ShowRaceMenu does [observed: TTW's scripts wait in `MenuMode 1036` after it; design: owner 2026-11-02] |
| `TTW.ini` `[Karma Titles]` | optional per-level karma title overrides (`sKarmicTitle<Good\|Neutral\|Evil><level>`, levels `1..=50`, `1..=60`, and higher) read from `Data\NVSE\Plugins\TTW.ini` (`fnv_game::karma_titles::TtwKarmaTitles`) and wired into `GetPlayerKarmaTitle` and the Pip-Boy Stats page (both hand-built and tile-drawn `StatsMenu`) [documented: ROOGNVSE Plugin / TTW NVSE Plugin (Nexus Mods ID 77415, `https://www.nexusmods.com/newvegas/mods/77415`, author Roy Batty / RoyBatterian, versions 3.3.2 / 3.3.3; bundled in Tale of Two Wastelands 3.3.x as `ttw_nvse.dll`) public description ("Karma Title Expansion: Set custom titles for each level of Karma (Good, Neutral, Evil) via INI") and shipped `Data\NVSE\Plugins\TTW.ini` `[Karma Titles]` schema (`sKarmicTitleGood<N>`, `sKarmicTitleNeutral<N>`, `sKarmicTitleEvil<N>`, e.g. levels `1..=60` in TTW 3.3.3 / Consistent Karma Titles - ESPless, Nexus Mods ID 89822, `https://www.nexusmods.com/newvegas/mods/89822`, author VeryLastKiss); DLL/source not consulted; hypothesis: ASCII case-insensitive section/key matching, optional leading-zero digits, `;`/`#` comments and one optional surrounding quote pair stripped, blank values ignored; design: levels below `1` clamp to `1`, levels above `30` with no exact entry clamp to the highest defined `TTW.ini` level when `>= 30`, sparse overrides below `30` fall back to the plugins' settings (pipboy.md, Stats), lazy `OnceLock` cache on `ModFiles`]. Absent or empty leaves the plugins' karma titles unchanged |
| StewieGetValue `"key:section"` (StewVal) | [documented: StewieGetValue] Reads a setting. [design] After handoff, the mapped [critical](stewie-stat-profile.md) and [inventory](#live-inventory-settings) keys read NVForever choices. Other keys and unbound games keep the layered `Data\NVSE\Plugins\nvse_stewie_tweaks.ini` lookup, with modular `Data\NVSE\Plugins\Tweaks\INIs\*.ini` files merged on top in case-insensitive filename order ([stewie-modular-inis.md](stewie-modular-inis.md)): a number when it reads as one, otherwise text, or 0 when absent. |
| GetCellRegions `cell` | an exterior cell's regions (XCLR); none for an interior [documented]. When `Data\NVSE\Plugins\hNVSE.ini` sets `[Main] bMergeRegionLists=1` at load time ([documented: hNVSE 1.2 (Nexus Mods ID 85748, `https://www.nexusmods.com/newvegas/mods/85748`, authors DuncanWasHere & A User) public changelog and shipped `Data\NVSE\Plugins\hNVSE.ini` comments ("Force-merges cell region lists (XCLR subrecord in xEdit) during loading"); DLL/PDB/source not consulted]; [observed: FalloutNV 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 + hNVSE 120 on exterior cell `188TradingPost` (`0x000DDF1C`) with a single-record test override ESP: `[Main]` is strictly required (`[General]` is ignored) and merged `XCLR` lists place the overriding plugin's regions first followed by earlier plugins' remaining non-duplicate regions]), overridden exterior cells return the deduplicated overriding-plugin-first union (`fnv_world::audio::cell_regions_merged`, `Game::cell_regions`) [design: read once at load so world and script consumers agree and mid-session INI writes do not toggle it; `GameData::merged_cell_regions` is populated only when enabled at load so startup cost is zero when absent or disabled; per-record `XCLR` order preserved within each plugin; interior cells and deleted overrides skipped]. Absent or `0` leaves vanilla last-writer-wins unchanged |
| SetWorldspaceMapBounds `world nwX nwY seX seY` | the WRLD MNAM cells, read by every reader of the record (`Game::world_map_bounds`) [documented; follow-up: the engine's map reads its bounds once] |
| ySIGetTrait / ySISetTrait `"tag"\|"string"\|"icon"\|"filename" [value] [form]` | ySI's sorting tag (`"tag"` / `"string"` exact bidirectional alias) and icon texture path (`"icon"` / `"filename"` share the exact same non-aborting path and tag-scoped override store; `(trait, value, form)` arg order returning `1` on `"tag"`/`"string"`/`"icon"`/`"filename"` and `0` on rejected metadata/unknown names); dot-receiver calls unwrap a placed world reference to its base form, whereas explicit form arguments are not unwrapped; when `menus/ySI/*.json` packs are loaded, unmatched/untagged/non-item `"icon"` and `"filename"` queries return `"interface\\icons\\Misc_Unknown.dds"` (and `""` when no ySI JSON packs are loaded); metadata trait names (`"template"`, `"systemcolor"`, `"category"`, `"name"`, `"tab"`, `"count"`) return `""` on `ySIGetTrait` and `0` (no-op) on `ySISetTrait` [documented: GECK wiki `YSIGetTrait`, `YSISetTrait`; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + yUI v1.4d (`140`), 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated clean-room staging copy (read-only `PrintToConsole` probes; zero writes to game install or Saves), `w4e_01_ysi_string_alias`, `w4e_02_ysi_metadata_traits`, `w4e_03_ysi_unmatched_icon_fallback`, `w4e_04_ysi_worldref_arg_vs_dot`, `w4e_c01_ysi_armordt_sibling_formlist2` (`tools/capture/nvse/catalogs/wave4e_mods.json`, #3803) and `w4c_b4_15b_ysi_no_json_fallback` (`tools/capture/nvse/catalogs/wave4c_extenders.json`, #3679): `PROBE\|w4c_b4_15b_ysi_no_json_fallback\|st=[](0)\|wp=[](0)\|pl=[](0)`, and `w6t2_26_ysi_traits_set_get_and_metadata` (`tools/capture/nvse/catalogs/wave6_t2_observed_matches.json`): `PROBE\|w6t2_26_ysi_traits_set_get_and_metadata\|set(str=1,ic=1,tpl=0,cat=0,unk=0)\|g1=W6T2Tag,fn=w6t2_ic.dds\|meta(,,,,,)`; zero DLL, PDB, or third-party source consulted] |

### Live inventory settings

[documented: Stewie's Tweaks readme and generated `nvse_stewie_tweaks.ini` (see
[tweaks](../design/tweaks.md#sources-and-clean-room)), `bHardcoreTweaks`, `bAmmoWeight`
and `bIndividialItemStats`] Hardcore features can be chosen independently of the
game's Hardcore mode; its ammo option selects whether ammunition has weight.
Individual item figures show one item's weight and value for a stack.

[design] After the engine hands its authoritative settings to the game,
`StewieGetValue` and `StewVal` return the existing choices below as numeric 0 or 1.
Both functions use the same read-only path; names take `Key:Section` and do NOT trim
leading, trailing, or around-colon whitespace (`Section:Key` and bare keys also return 0)
[observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + lStewieAl's Tweaks 1005 + JIP LN
57.30, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated
clean-room staging copy (read-only `PrintToConsole` probe; zero writes to game install or
Saves), `w4h_08_stewie_get_value_whitespace`
(`tools/capture/nvse/catalogs/wave4h_reconcile.json`, #3803):
`PROBE|w4h_08_stewie_get_value_whitespace|sk(0=0,L=0,R=0,M=0)|ks(0=1,L=0,R=0,M=0)|bare(0=0,L=0,R=0)`;
zero DLL, PDB, or third-party source consulted]. Names ignore ASCII case [hypothesis: not
probed in `w4h_08_stewie_get_value_whitespace`]. The base `nvse_stewie_tweaks.ini` never
supplies NVForever defaults; mod-shipped `Data\NVSE\Plugins\Tweaks\INIs\*.ini` files may
override NVForever tweak settings at startup beneath `NVForever.ini` (maintainer decision
2026-11-24, [stewie-modular-inis.md](stewie-modular-inis.md)). No new settings are added.

| Query | Existing choice and consumer |
|---|---|
| `bHardcoreTweaks:Tweaks` | `Tweaks::hardcore_tweaks`; enables the separate Hardcore rules |
| `bAmmoWeight:Hardcore Tweaks` | `Tweaks::hardcore_ammo_weight`; the inventory rules apply this only when the parent choice is enabled |
| `bIndividialItemStats:Tweaks` | `Tweaks::single_item_figures`; the Pip-Boy item figures choose one item or the stack's count |
| `bDisableStealthEffectInPipboy:Tweaks` | `Tweaks::no_stealth_shimmer_in_pipboy`; disables the stealth shimmer while the Pip-Boy is open |
| `bKeepPipboyLightOnCellChange:Tweaks` | `1` when bound; NVForever already keeps the Pip-Boy light on across cell changes ([tweaks](../design/tweaks.md)) |
| `bWeightlessWornArmor:Tweaks` | `Tweaks::weightless_worn_armor`; worn armour weighs nothing in inventory totals and `GetCalculatedEquippedWeight` |
| `bWeightlessWornPowerArmor:Tweaks` | `Tweaks::weightless_worn_power_armor`; worn power armour weighs nothing in inventory totals and `GetCalculatedEquippedWeight` |
| `bSynchronizeContainerCategories:Tweaks` | `Tweaks::sync_categories`; synchronizes Container and Barter column categories |
| `bPreventSwitchWhenEmptyingCategory:Synchronized Container Categories` | `Tweaks::prevent_switch_when_emptying_category`; retains the child choice (default `1` under disabled parent) and controls whether emptying a synchronized category stays on it or switches both columns to All |
| `bBarterCheckActorBuySellFlags:Tweaks` | `Tweaks::vendor_lists_both`; holds the player's barter side to the merchant's service flags (Stimpaks follow Stimpaks, ignoring Chems) |
| `bDontOpenDoorsAfterLockpick:Tweaks` | `Tweaks::no_door_after_pick`; picked doors stay shut after lockpicking |
| `bNoSkipDialog:Tweaks` | `Tweaks::no_skip_dialogue`; clicking does not skip spoken dialogue lines |
| `bRestoreLockedDoorSound:Tweaks` | `1` when bound; NVForever already plays `DRSLocked` when activating a refused lock ([tweaks](../design/tweaks.md)) |
| `bArmorConditionLabel:Tweaks` | `Tweaks::armor_condition`; shows the worn body armour's condition bar on the HUD |
| `bFasterSleepWait:Tweaks` | `Tweaks::faster_sleep_wait`; shortens the Sleep/Wait hour countdown interval (`wait::FASTER_HOUR_SECONDS`) |
| `bUseWASDAsArrowKeys:Tweaks` | `Tweaks::wasd_as_arrow_keys`; adds WASD as arrow keys and Space as accept across Barter, Quantity, Sleep/Wait, and Recipe menus |
| `fCraftedItemHealthPct:Tweaks` | `Tweaks::crafted_item_health_pct`; sets crafted weapon/armour condition percentage (`0` leaves vanilla crafting unchanged) |

[hypothesis] The ammo query retains the child choice while its parent is disabled,
as the plugin's own file keeps both keys; not observed in the original game. Its answer
does not claim that ammo currently weighs anything: the existing inventory
consumer also considers the parent and the game's Hardcore mode. The original
spelling `Individial` is preserved.

Before the authoritative handoff, and for all other keys, the established INI
fallback applies. Weightless-item parent and child switches remain on that path:
the engine's per-kind mask does not store independent disabled-parent choices.
These three mappings establish no provider version or complete mod compatibility.
Original-game comparison and execution of the installed Item Cards and Pip-Boy UI
Tweaks workloads remain separate acceptance work.

Authored controls cover both aliases at the source boundary, explicit off versus a
conflicting INI, unknown/string/malformed fallback, current changes and restore
rebinding; actual ammunition totals across parent, child and Hardcore-mode choices;
actual Pip-Boy stack weight/value; and settings/startup/restored caches through
the existing handoff. The opt-in selector requires a dedicated
`NVF_STEWIE_PROFILE_HOME`, `--ignored` and `--test-threads=1`; skipping it supplies
no validation evidence.

[implementation evidence: head `68a9c978`, 2026-11-17] The
[natural hosted run](https://github.com/LootJunkieAzza/NVForever/actions/runs/44155882635)
at checkout `2a932d9b`, parents `[13011912,68a9c978]`, passed all seven checks.
Five owning core controls and the ordinary Pip-Boy control each passed once,
including these two inventory core controls and the actual stack-figure consumer.
Its three ignored opt-ins received no credit. On exact original `68a9c978`, a
later Windows Cargo 1.99 QA runtime-only r2 passed all three opt-ins once in
separate processes. The inventory case reset each source-cache sentinel to 9
before Restart, NewGame and restored Restart/Load, then checked expected values
individually, explicit off and retained-child behavior, and unchanged INI bytes.
The other two opt-ins cover critical profiles ([profile note](stewie-stat-profile.md)).

The first coordinated compile failed with E0624 at a private cross-crate
inventory method. Its fixture now uses public `World::inventory_mut`, retaining
the same inventory admission, `totals_with` consumer and every weight oracle.
Revision326 later compiled five core controls: four passed and the all-off query
failed at its first ammo-child read. It had used `Tweaks::default()`, which
retains the child true under a disabled parent. Revision68 explicitly selects
the intended three false inputs; production defaults, the zero oracle and all
later assertions remain unchanged. Later assertions were not reached in that
failed run. The earlier focused68 Windows query/ammo-total passes remain scoped
to their own run and do not change that failure.

The Windows compile used for the three profile opt-ins returned zero, but its
controller failed descendant cleanup with unknown child identity; strict
compile-controller and overall success remain false. Runtime-only r1 stopped with
`KeyError` on missing `priorResult` metadata before any Job, discovery or test. Corrected r2
reused the sealed source-bound image without a rebuild and qualified only its
three actual runtime results. No failed attempt is relabelled as passed.

Compiler, runtime, strict-lint and hosted checks on the corrected integration remain
UNRUN. The historical six ordinary and three opt-in results do not qualify this
candidate. Identical-fixture baselines, original-game and installed-workload
comparisons, native profile semantics and full Stewie compatibility remain open.

## 4. Sky plugin INI configs (`MoonlightNVSE.ini` and `CloudUpgrade.ini`)

Built in `engine/nvforever/src/sky_ini.rs` and wired into `engine/nvforever/src/sky.rs`,
`worldspace.rs`, `cloud_shadows.rs`, and `main.rs`.

**Provenance.** Neither plugin's DLL nor source code was ever consulted or disassembled.
Section/key names, install paths, and default values come strictly from the public Nexus
Mods pages and shipped INI files of:
- **MoonlightNVSE v2.00** (by WallSoGB): `https://www.nexusmods.com/newvegas/mods/77683`,
  shipped `Data\nvse\plugins\MoonlightNVSE.ini`;
- **Cloud Upgrade NVSE v1.0** (by doodlum): `https://www.nexusmods.com/newvegas/mods/79612`,
  shipped `Data\NVSE\Plugins\CloudUpgrade.ini`.

Exact third-party runtime internals are unknown: the 8-phase moon mapping remains
`[hypothesis]`, and NVForever's VFS loading, clamping, omitted-key fallback, and
**CloudUpgrade cloud speed modulation formula/mapping are NVForever's own `[design]`,
not the Cloud Upgrade NVSE plugin's implementation**.

- **Vanilla invariance `[design]`:** Both INI files are discovered case-insensitively
  through the engine VFS at startup (`Data\NVSE\Plugins\MoonlightNVSE.ini` [documented:
  MoonlightNVSE v2.00 shipped `nvse\plugins\MoonlightNVSE.ini`] and
  `Data\NVSE\Plugins\CloudUpgrade.ini` [documented: Cloud Upgrade NVSE v1.0 shipped
  `NVSE\Plugins\CloudUpgrade.ini`]) and carried across worldspace/climate sky rebuilds
  (`worldspace::carry_sky`). When either file is absent or unreadable, the corresponding
  option stays `None` and vanilla sky lighting and cloud animation remain bit-for-bit
  unchanged. Neither plugin is added to `IsPluginInstalled` / `GetPluginVersion`
  reporting (`ext-version.md`).
- **Shared INI parsing `[design]`:** Lossy UTF-8 decoding with optional leading UTF-8 BOM
  stripped; `;` and `#` comments ignored (full-line and inline); section and key names
  matched ASCII case-insensitively; unknown sections/keys and non-finite or malformed
  values ignored without aborting loading; duplicate keys take the last valid value.
- **`MoonlightNVSE.ini`:**
  - `[documented: MoonlightNVSE v2.00 (https://www.nexusmods.com/newvegas/mods/77683),
    public Nexus page and shipped Data\nvse\plugins\MoonlightNVSE.ini]` `[Main]` contains
    `bMoonPhaseBrightness=1` (toggle enabling dynamic nighttime brightness by moon phase)
    and `fMoonMult0=0`, `fMoonMult1=0.33`, `fMoonMult2=0.5`, `fMoonMult3=0.9`,
    `fMoonMult4=1` (multipliers from new moon = `0` to full moon = `4`).
  - `[design]` When `MoonlightNVSE.ini` is present and an individual key is omitted or
    invalid, NVForever falls back to those exact shipped v2.00 INI defaults
    (`bMoonPhaseBrightness=1`, `fMoonMult0..=fMoonMult4 = [0.0, 0.33, 0.5, 0.9, 1.0]`).
  - `[hypothesis]` The 8-phase lunar cycle (`fnv_world::sky::MOON_PHASES`: `full`,
    `three_wan`, `half_wan`, `one_wan`, `new`, `one_wax`, `half_wax`, `three_wax`) maps
    symmetrically onto `fMoonMult4` (full), `fMoonMult3` (gibbous), `fMoonMult2` (half),
    `fMoonMult1` (crescent), and `fMoonMult0` (new).
  - `[design]` Multipliers are clamped to `0.0..=10.0` (so negative presets produce pitch
    dark directional moonlight without negative PBR illuminance). Scaling multiplies
    `MOON_SHARE` inside `sky::aim_sun` only outdoors when the sun is below the horizon and
    the active climate displays a moon (`Climate::has_moon()`); daytime directional light,
    interiors, and `bMoonPhaseBrightness=0` stay bit-for-bit equal to vanilla.
- **`CloudUpgrade.ini`:**
  - `[documented: Cloud Upgrade NVSE v1.0 (https://www.nexusmods.com/newvegas/mods/79612),
    public Nexus page and shipped Data\NVSE\Plugins\CloudUpgrade.ini]` `[Global]` defines
    `Speed = 100.0`, `MaxSpeed = 5.0`, and `Volatility = 5.0`; `[Layer 1]`..`[Layer 4]`
    define per-layer `SubSpeed` (`0.2`, `1.0`, `0.7`, `0.3`) and `SubVolatility` (`0.2`,
    `0.3`, `0.4`, `0.5`) for the four WTHR cloud layers (`0..4`).
  - `[design]` When `CloudUpgrade.ini` is present and an individual key is omitted or
    invalid, NVForever falls back to those exact shipped v1.0 INI defaults.
  - `[design: NVForever's own cloud speed modulation formula and mapping, not the Cloud
    Upgrade NVSE plugin's]` Parsed values are clamped (`Speed` `0..=10000`, `MaxSpeed`
    `0..=50`, `SubSpeed` `0..=50`, `Volatility`/`SubVolatility` `0..=100`). Effective
    layer scroll scale is `(Speed / 100.0) * SubSpeed` capped at `MaxSpeed`. Because
    vanilla WTHR horizon layers commonly store `ONAM` speed `0`, active
    `CloudUpgrade.ini` uses a fallback base rate of `32 * CLOUD_SCROLL_PER_UNIT` for
    `ONAM == 0` layers (while `Speed == 0`, `MaxSpeed == 0`, or `SubSpeed == 0` stops the
    layer). Non-zero `Volatility * SubVolatility` applies NVForever's own smooth, bounded
    cosine speed modulation (`amplitude = 0.5 * vol / (vol + 5.0)`, distinct per-layer
    periods `[180, 145, 115, 90]` s, peak rate capped at `base * MaxSpeed`) integrated
    analytically in `f64` so UV scroll offsets remain monotonic, stateless, deterministic,
    and synced with `cloud_shadows`.

## 5. ManOnFireNVSE INI fire spread

Built in `crates/fnv-game/src/man_on_fire.rs`, ticked from `Game::tick` during unpaused,
non-frozen game mode (`!in_menu && !time_frozen`). Clean-room implementation from the
public Nexus Mods page and shipped INI file comments only: **Man On Fire NVSE v1.0.1**
(`https://www.nexusmods.com/newvegas/mods/94962`, shipped `Config\ManOnFireNVSE.ini`
installed to `Data\Config\ManOnFireNVSE.ini`). The plugin's DLL and source code were
never consulted.

### Configuration lookup and schema

- **Paths:** `Data\Config\ManOnFireNVSE.ini` primary [documented: Man On Fire NVSE
  v1.0.1 (`https://www.nexusmods.com/newvegas/mods/94962`), archive install path
  `Data\Config\ManOnFireNVSE.ini`], falling back to
  `Data\NVSE\Plugins\ManOnFireNVSE.ini` [design: NVForever fallback lookup path, not
  stated in the shipped v1.0.1 release]. When neither file is present, no fire-spread
  logic runs and unmodded vanilla combat is unchanged [design].
- **Parser:** BOM-safe (UTF-8 `\u{feff}` and Latin-1-decoded UTF-8 BOM), case-insensitive
  `[Settings]` section and key names, `;` and `#` comments stripped, invalid or
  non-finite numeric values keep defaults, negative numeric values clamp to `0.0`, and
  `fMaxRange <= 0` disables spread [design].
- **`[Settings]` keys and defaults** [documented: Man On Fire NVSE v1.0.1
  (`https://www.nexusmods.com/newvegas/mods/94962`), shipped
  `Data\Config\ManOnFireNVSE.ini` and its inline `;` comments; only the public mod page
  and shipped INI were consulted, never the DLL or source]:

| Key | Default (v1.0.1 INI) | Role (from v1.0.1 shipped INI comments) |
|---|---|---|
| `fIgnitionThreshold` | `1.0` | Seconds of exposure to fire before catching fire ("Higher = slower spread") [documented: v1.0.1 INI] |
| `fDecayRate` | `0.3` | Exposure decay per second when moving away from fire [documented: v1.0.1 INI] |
| `fMaxRange` | `100.0` | Maximum 3D distance in game units for fire spread ("Closer = faster accumulation") [documented: v1.0.1 INI] |
| `bUseFireResist` | `1` | Factor in Fire Resistance actor value (code 52, [documented: GECK wiki Actor Value list]) [documented: v1.0.1 INI] |
| `bPlayerImmune` | `0` | Player immune to fire spread [documented: v1.0.1 INI] |
| `bCompanionsImmune` | `0` | Companions (`Game::is_teammate` [design]) immune to fire spread [documented: v1.0.1 INI] |

### Spread, decay, and ignition rules

- **Burning source detection:** a loaded actor with an active, non-permanent, non-worn
  effect whose base effect (`MGEF`) is harmful (`Hostile` or `Detrimental`) and resisted
  by Fire Resistance (`combat_lines::FIRE_RESISTANCE`, code 52; same test as
  `hit-effects.md` §3 and `grass-fire.md` §1) [hypothesis]. Multiple fire effects on the
  same burning actor collapse to one source (highest magnitude) so an item with two
  fire MGEFs does not double-count [design]. Burning corpses whose burn outlasts death
  (`No Death Dispel`) remain valid sources while the burn lasts [design].
- **Proximity accumulation:** for each loaded, living, non-immune, non-burning actor in
  the same cell/worldspace within `fMaxRange`, each burning source contributes linear
  distance falloff `clamp(1.0 - dist / fMaxRange, 0.0, 1.0)` per second, summed across
  nearby burning sources [hypothesis].
- **Fire Resistance:** when `bUseFireResist` is true, accumulation scales by
  `clamp(1.0 - FireResist / 100.0, 0.0, 1.0)` (`100%` blocks spread) and ignition
  applies through the hit-effect resistance path (`Mods::hit = true`) [hypothesis]. When
  `bUseFireResist` is false, accumulation ignores Fire Resistance and ignition applies
  with `Mods::hit = false` so resistance does not suppress spread ignition [design].
- **Decay and bounds:** actors not receiving positive exposure decay by
  `fDecayRate * dt` per frame and are removed once at `0.0`. Already-burning actors clear
  spread exposure so two burning actors do not infinitely re-ignite each other [design].
  Concurrent tracked exposure entries are capped at `MAX_TRACKED_EXPOSURE` (`1024`)
  [design].
- **Immunity:** dead, `Game::is_immune` (child, invulnerable, ghost), god-mode player,
  player when `bPlayerImmune` is true, and hired teammates (`Game::is_teammate`) when
  `bCompanionsImmune` is true never accumulate exposure or ignite from spread [design].
- **Ignition:** when exposure reaches `fIgnitionThreshold`, the closest burning source's
  active fire magic item (all effects when all are fire-resisted, else only its
  fire-resisted effect indices) is applied to the victim with the source's caster
  (falling back to the burning actor) and triggers `OnFire` combat lines [hypothesis;
  design].
- **Scope note:** verified with synthetic unit and integration controls in `fnv-game`;
  original-game comparison with the installed plugin remains separate acceptance work.
