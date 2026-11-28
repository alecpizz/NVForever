# Spec note: extender version and plugin checks

Covers how scripts ask which script extender and which extender plugins are present:
GetNVSEVersion, GetNVSERevision, GetNVSEBeta, IsPluginInstalled, GetPluginVersion, the
plugins' own getters (GetLNVersion/GetJIPLNVersion, GetNVSEVersionFull, …), and what
NVForever should answer.

Written for task C5 of the extender plan ([extender-core-tasks.md](../handoff/extender-core-tasks.md))
from the GECK wiki export. Everything the pages state is tagged `[documented: <page>]`;
what they leave out is **open (to observe)**. Origins are from
`schemas/script/functions.json` [observed]. The list of known plugin names that
GetPluginVersion and IsPluginInstalled include is in §2a [documented: the GetPluginVersion page's list, pasted by the owner from geckwiki.com, 2026-10-25].

Behaviour is our own wording of the GECK wiki export (`C:\fnv-private\geckwiki\`,
private); no extender source, no opcode numbers or argument encodings (those come only
from a cited document or from compiling our own test scripts with xNVSE,
[extender-compat.md](../design/extender-compat.md) §4.1).

## 1. The extender's own version (NVSE)

| Function | Arguments | Returns | Behaviour |
|---|---|---|---|
| GetNVSEVersion | — | int | the installed extender's version number [documented: GetNVSEVersion] |
| GetNVSERevision | — | int | its revision number [documented: GetNVSERevision] |
| GetNVSEBeta | — | int | its beta number; **0 for a non-beta release** [documented: GetNVSEBeta] |

- The pages' checks: `if GetNVSEVersion < 4` (needs NVSE 4), `if GetNVSEVersion == 4 &&
  GetNVSERevision < 3` (needs 4 revision 3), `if GetNVSEBeta != 0 && GetNVSEBeta < 3`
  (needs beta 3) [documented: each page's example]. The NVSE Expressions page's example
  prints `$(GetNVSEVersion)` as `4` [documented: NVSE Expressions], an old example.
- **A version check only works in a script that uses nothing else from the extender**:
  without the extender, a script using any extender feature fails before the check runs
  [documented: GetNVSEVersion, "Usage Warning"].
- xNVSE 6.4.9 splits into `6 / 4 / 9` across `GetNVSEVersion`, `GetNVSERevision`, and
  `GetNVSEBeta` [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9, 2026-11-23,
  `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, read-only `PrintToConsole`
  probe `core_01_nvse_version_parts` (`tools/capture/nvse/catalogs/wave4a_extenders.json`,
  #3679, printing `GetNVSEVersion`, `GetNVSERevision`, `GetNVSEBeta`):
  `PROBE|core_01_nvse_version_parts|6,4,9`; no DLL or source consulted].

## 2. Extender plugins

| Function | Arguments | Returns | Behaviour |
|---|---|---|---|
| IsPluginInstalled | plugin name (string) | bool | 1 when that NVSE plugin is loaded [documented: IsPluginInstalled] |
| GetPluginVersion | plugin name (string) | float | the plugin's version, **-1 when it is not loaded** [documented: GetPluginVersion] |

- **The name is the one the plugin registers** (shown in `nvse.log`), **not its file
  name** [documented: IsPluginInstalled; GetPluginVersion].
- **The number is usually the version times 100** (ShowOff 1.80 → 180), **except JIP LN,
  which returns its real version as a float** (57.30) [documented: GetPluginVersion].
- Matching of the name (case, spaces): open.

**Plugin names the pages give** (exact strings):

| Plugin | Name to test | Evidence |
|---|---|---|
| JIP LN NVSE | `"JIP NVSE Plugin"` | [documented: GetPluginVersion; IsPluginInstalled] |
| JohnnyGuitar NVSE | `"JohnnyGuitarNVSE"` | [documented: IsPluginInstalled] |
| SUP NVSE | `"SUP NVSE Plugin"` | [documented: IsPluginInstalled; GetSUPVersion] |
| MCM | `"MCM Extensions"` | [documented: IsPluginInstalled] |
| NX (New Vegas Extender) | `"Extensions for NVSE"` | [documented: IsPluginInstalled] |
| ShowOff NVSE | `"ShowOffNVSE Plugin"` | [documented: GetPluginVersion; Category:Functions (ShowOff NVSE)] |
| JIP PP LN | `"JIP PP LN"` | [documented: AttachModel and the other JIP PP LN pages] |
| CacheUI | `"CUI"` | [documented: Category:Functions (CacheUI Plugin)] |
| hNVSE | `hNVSE` (unquoted on its page; `"hNVSE"` in §2a) | [documented: Category:Functions (hNVSE)] |

Two names are only on category pages, not in the list below: `"JIP PP LN"` and `"CUI"`.

## 2a. Known plugin names

The wiki's list of plugins and the name each registers [documented: the GetPluginVersion page's list, pasted by the owner from geckwiki.com, 2026-10-25]. These are
the strings mods pass to IsPluginInstalled and GetPluginVersion; C5 keys its answers on
them.

| Plugin | Registered name | Note |
|---|---|---|
| Aim Transition Patch | `"AimTransitionFix"` |  |
| AnhNVSE | `"AnhNVSE"` |  |
| ARES_NVSE | `"ARESPlugin"` |  |
| Book Menu Restored Plugin | `"Book Menu Restored"` |  |
| Charisma Tweaks | `"Charisma Overhaul"` |  |
| Climate Control NVSE | `"ClimateControl"` |  |
| Console Paste Support | `"Console Clipboard"` |  |
| Controller Support NVSE | `"Controller Support"` |  |
| Crash Logger | `"CobbCrashLogger"` |  |
| Cursor Unilaterally Matched | `"CUM"` |  |
| Fog-Based Object Culling | `"FogCulling"` |  |
| GaryGaryGary | `"GAAARY"` |  |
| GECK Extender | `"ZeGaryHax"` | cannot be tested with IsPluginInstalled or GetPluginVersion |
| hNVSE | `"hNVSE"` |  |
| Height Randomizer | `"HeightRandomizer"` |  |
| High Res Local Maps | `"HighResLocalMaps"` |  |
| High Resolution Bloom | `"HighResBloom"` |  |
| High Resolution Screens | `"HDPipboy"` |  |
| High Resolution Water Fog | `"HighResWaterDepth"` |  |
| Hot Reload (NVSE) | `"hot_reload"` | see also GetGameHotReloaded |
| Improved Console (NVSE) | `"Improved Console"` |  |
| Improved Lighting Shaders | `"Improved Lighting Shaders for FNV"` |  |
| JIP LN NVSE | `"JIP NVSE Plugin"` |  |
| JIP Realistic Weapon Overheating | `"RWO Plugin"` |  |
| JohnnyGuitar NVSE | `"JohnnyGuitarNVSE"` |  |
| kNVSE Animation Plugin | `"kNVSE"` |  |
| kNVSE JSON Generator | `"MyFirstPlugin"` |  |
| LStewieAl's Tweaks | `"lStewieAl's Tweaks"` |  |
| Manual Reload | `"Manual Reload"` |  |
| MCM | `"MCM Extensions"` |  |
| FNV Mod Limit Fix | `"MLF"` |  |
| MoonlightNVSE | `"MoonlightNVSE"` |  |
| NVAC - New Vegas Anti Crash | `"NVAC - New Vegas Anti Crash"` |  |
| New Vegas - Enhanced Camera | `"EnhancedCamera"` |  |
| New Vegas Reloaded | `"NewVegasReloaded"` |  |
| New Vegas Stutter Remover | `"sr_New_Vegas_Stutter_Remover"` |  |
| No Skill Tags | `"NO TAGS REDONE"` |  |
| NVTF - New Vegas Tick Fix | `"NVTF"` |  |
| NX | `"Extensions for NVSE"` |  |
| OneTweak | `"OneTweak"` |  |
| Pip-Boy Shading Fix | `"PipBoyShadingFix"` |  |
| Plugins+ | `"Plugins+"` |  |
| Project Nevada | `"pn_nvse"` |  |
| FNV ReShade Helper | `"FNVReShadeHelper"` |  |
| ROOG FNV - ROOG TTW | `"Hard Hardcore"` |  |
| ROOGNVSE Plugin | `"ttw_nvse"` |  |
| ShowOff NVSE | `"ShowOffNVSE Plugin"` |  |
| Simple Exterior Fog Remover | `"ExteriorFogRemover"` |  |
| SUP NVSE | `"SUP NVSE Plugin"` |  |
| SUPDate Checker | `"SUPDate Checker"` |  |
| UIO - User Interface Organizer | `"UI Organizer Plugin"` |  |
| YAPR - Yet Another Pip-Boy Remover | `"PipBoyTweaks"` |  |
| yGTM - Gameplay Tweaks and Madness | `"yGTM"` |  |
| yUI - User Interface | `"yUI"` |  |

## 3. Plugins' own version getters

| Function | Origin | Returns | Behaviour |
|---|---|---|---|
| GetLNVersion (alias GetJIPLNVersion) | LU | int | **always 0 now**; deprecated since NVSE 4.5.7 in favour of GetPluginVersion [documented: GetLNVersion] |
| GetNVSEVersionFull | JIP | float | the full extender version (version, revision and beta) as one float, e.g. `>= 5.02`; registered on an unused base-game opcode, so it returns 0 without NVSE and JIP and works as an NVSE check; broken before JIP 57.05 [documented: GetNVSEVersionFull] |
| GetPluginHeaderVersion | JIP | float | a plugin file's header version, e.g. of FalloutNV.esm [documented: GetPluginHeaderVersion] |
| GetSUPVersion | SUP | -1 always | prints SUP's version to the console; not a check [documented: GetSUPVersion] |
| GetNVSEOnlineVersion, GetNexusModVersion… | SUP | — | fetch versions from the internet; out of scope (NVForever does not reach the network for scripts) [documented: their pages] |
| IsDLLLoaded | JohnnyGuitar | bool | whether a named DLL is loaded into the game [documented: IsDLLLoaded] |

No "IsScriptExtenderLoaded"-style function is in the export; GetNVSEVersionFull is the
documented way to test for the extender from a script that must also run without it.

## 4. How the wiki's examples check versions (survey)

Counted over the private export [observed: every page's text]:

| Check | Mentions |
|---|---|
| GetPluginVersion "…" | 24 (mostly "use GetPluginVersion "X"" lines on plugin category pages) |
| GetNVSEVersion | 15 |
| IsPluginInstalled "…" | 9 |
| GetNVSEBeta | 5 |
| GetNVSERevision | 4 |
| GetLNVersion / GetJIPLNVersion | 1 each (its own deprecation page) |

So mods mostly gate on **GetPluginVersion with a plugin name** and on **GetNVSEVersion**
(compared with 4, the version that brought let, arrays and strings). Examples compare
with "at least" (`<` → warn), so reporting a version that is too high is safer than too
low.

## 5. What NVForever answers (for C5)

- **GetNVSEVersion / Revision / Beta**: xNVSE 6.4.9 [decision: the owner, 2026-10-26],
  split as §1 settles (the probe is asked for).
- **IsPluginInstalled / GetPluginVersion**: report a plugin as loaded only when NVForever
  implements that plugin's functions (C2's per-plugin modules), with the version that
  module claims; otherwise 0 and -1 [decision: claiming a plugin whose functions are
  missing would let mods take paths that then fail]. The names in §2 are the keys.
  Reported (`fnv_script::vm::jip::PLUGINS`): JIP LN 57.3, ShowOff 184, kNVSE 37,
  JohnnyGuitar 528, MCM Extensions 1 [observed: the owner's install], and since
  2026-11-08 `"lStewieAl's Tweaks"` 941 and `"ttw_nvse"` 1 [design, owner: "technically
  the dll is present, just in engine"]: Tale of Two Wastelands' start-up checks
  (`TTWGenericHandlerScript`, TTW 3.4) show a "missing" message for Stewie's Tweaks
  below 941 and for `IsPluginInstalled ttw_nvse` 0, while NVForever runs the functions
  TTW calls from both (`TTW_*`, StewieGetValue, `fnv_game::nvse_small`). Neither plugin
  is on the owner's install, so neither version was read: 941 was the lowest TTW accepts,
  and TTW never asks ttw_nvse's. Most of Stewie's tweaks were not built initially
  (ext-small.md); Stewie's was promoted to `945` on 2026-11-23 (#3179) and to `960` on
  2026-11-24 after completing the 9.50–9.60 qualification step (§5.1, #3183).
- **The other plugins the engine builds** (2026-11-12): until then a mod asking
  `IsPluginInstalled "SUP NVSE Plugin"` got 0 and `GetPluginVersion` -1 for every plugin
  outside the list above, though all of that plugin's catalogue functions run here
  [observed: `fnvtool ext queries`, 0 and -1 for the seven names below], so its start-up
  check stopped it. Now reported as installed, under §2a's names:
  - `"CUI"` (CacheUI) and `"hNVSE"` at 120: the highest version their function pages name
    (1.2, the catalogue's `origin_version`), times 100 [documented: those pages;
    GetPluginVersion, "usually the version times 100"]. For the plugins read on the owner's
    install that figure is at or under the number the game gave (JIP 57.30, JohnnyGuitar
    5.28 → 528, ShowOff 1.83 → 184 read, kNVSE 30 → 37 read), so it is a floor.
  - `"SUP NVSE Plugin"`, `"JIP PP LN"`, `"AnhNVSE"`, `"Extensions for NVSE"` (NX) and
    `"UI Organizer Plugin"` at 9999 [design: no page names a version and none was read;
    §4, too high is safer than too low; `fnv_script::vm::jip::UNOBSERVED_VERSION`], to be
    replaced by the numbers read in the original (observer check 118). UIO has no script
    functions; the engine reads its `uio\public` lists itself (menu-xml.md), which is
    what a mod asking for it needs.
  - `"Improved Console"` at `3` [documented: §2a registered name `"Improved Console"`;
    observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + Improved Console v3 (10
    official masters), `nvse.log` registered name `"Improved Console"` version `3`,
    `GetPluginVersion "Improved Console" == 3`]. Improved Console (NVSE) has no script
    functions of its own; NVForever implements persistent console locals
    (`int`/`float`/`ref`/`string_var`/`array_var`) and transparent expression/statement
    result printing directly in the interactive console (`docs/design/console.md`,
    `Game::run_console_line`).
  - `"Engine Optimizations"` at `100` and `"HighResLocalMaps"` at `12` (2026-11-25,
    #4491) [observed in FadedLight's Nuclear Sunset fork: an installed copy's `nvse.log`
    registered names and versions]. Neither has script functions; their work (engine
    speed-ups, a sharper local map) is the engine's own here, as UIO's lists are. Nuclear
    Sunset's start-up Script Runner check shows "Outdated xNVSE plugins detected!" without
    them.
  - Still not reported: `"yUI"` (its two ySI trait functions are built, its menus are
    not; a mod that finds it goes on to read its tiles [observed: JustAssortedMods.esp
    4.6 asks `IsPluginInstalled "yUI"` and then reads `HUDMainMenu/ySI/_ySI`]), and the
    plugins whose registered name no page gives (ITR, One111, ppNVSE, vheyes-nvse, the
    smaller ones; §6).
- **GetIsLAA** (the 4 GB patch): 1 [design, 2026-11-08: NVForever is a 64-bit program with
  no 2 GB limit; the unpatched original answers 0, observed in the gaps4 set; TTW's check
  showed "4GB patch missing"].
- Left as it is: TTW's `GetNumericIniSetting "bBackgroundLoadLipFiles:BackgroundLoad"`
  check reads the game's INI as NVForever reads it; its message ("bad INI") shows only
  when that setting is off or missing there. A Mod Organizer 2 profile's
  `FalloutCustom.ini` counts (mod-organizer-profile.md, #4491).
- **GetNVSEVersionFull**: a JIP function; answer only once JIP's module exists.
- **GetLNVersion**: 0, as documented.

### 5.1 Stewie's Tweaks: reported version by the features built

[design, updated 2026-11-24] Mods gate on `GetPluginVersion "lStewieAl's Tweaks"` for a
feature that came in that version [documented: the plugin's public changelog,
https://www.nexusmods.com/newvegas/mods/66347?tab=logs]. The version reported rises one
verified step at a time once every feature for that step is built and checked; it is
never the version of a DLL found on disk (the engine is not that DLL). A higher gate
rises only when its full step is verified, with a row here and tests
(`fnv_script::vm::jip::PLUGINS`; `stewie_945_960_970_qualification`).

`GetPluginVersion "lStewieAl's Tweaks"` was promoted from `941` to `945` (`#3179`,
`#3665`), and from `945` to **`960`** (`#3183`) once all public 9.50–9.60 changelog
features (`bVATSStopFollowingDeadTargets`, `bVATSThrowablesUseVisibility`,
`bArmorConditionLabel` + live `StewVal`/`StewieGetValue` + weapon-column X alignment,
`bImprovedHacking` keep-matching-characters, `bHideCrosshairWhileReloading`,
`bReplaceHelpWithConsole` + custom label, `bHostilesPrioritizePlayer`, and
`bRunningCostsAP` sneak options via `#3494`; `bAutoReadNotes` explicitly skipped as it
requires Book Menu Restored), live `StewVal`/`StewieGetValue` exposure for
`bKeepPipboyLightOnCellChange:Tweaks` and `bDisableStealthEffectInPipboy:Tweaks` (alongside
existing critical and inventory queries), live `GetPipBoyMode` (`nvse_jip2.rs`, observed
via `w4c_b3_06_toggle_pipboy_tab`), menu search-bar tile publication (`IM_SearchBar`,
`SM_SearchBar`, `MM_SearchBar`, `BM_SearchBar`, `CM_SearchBar`, `RM_SearchBar` via
`ui_search_bar::publish_for_menu`), and clean-room E2E contract tests for the Pip-Boy UI
Tweaks and Quality of Life ESPless `< 960` gates were built and verified. The higher gate
(`970`, `#3184`) remains unpromoted until its own verified step lands.

| Reported | Feature it stands for (changelog) | Status in NVForever | Mods gated (installed list) |
|---|---|---|---|
| 941 (superseded by 945) | TTW 3.4's minimum | Built & verified: TTW's `TTW_*` and StewieGetValue calls | Tale of Two Wastelands; gates 905/906 (Explosion Reactions, Animated Moments, Drowning Visuals, Modern Minimap's MCM list) |
| 945 (superseded by 960) | 9.45 "No Casino Bans - make GetCasinoWinningStage not return banned" and all 9.42–9.45 items (#3179) | Built & verified: `casino::script_winnings_level` (#2878, live `bNoCasinoBans=0/1` observed in `blackjack.md` §4 via #2962), `SetCasinoWinningsLevel` alignment (#3474 / #3490), Casino Rewards Fix 1.1 consumers (#3180), remaining 9.45 tweaks (#3481), and clean-room E2E contract tests (#3378, #3665) | Casino Rewards Fix 1.1 (`< 945` guard; then `GetCasinoWinningsLevel`, casino menu-close and dialogue-topic events) |
| 960 (reported) | 9.50–9.60 tweaks, "Add Command StewieGetValue (alias StewVal)", xNVSE 6.3.0 (#3183) | Built & verified: remaining 9.50–9.60 tweaks & live `bArmorConditionLabel` (#3494), live `StewVal`/`StewieGetValue` for `bKeepPipboyLightOnCellChange` / `bDisableStealthEffectInPipboy` / critical & inventory settings (`ext-small.md`, `stewie-stat-profile.md`), live `GetPipBoyMode` (`nvse_jip2.rs`, probe `w4c_b3_06_toggle_pipboy_tab`), menu search-bar tile publication (`native-menu-search.md`, `ui_search_bar::publish_for_menu`), and clean-room E2E contract tests (`stewie_945_960_970_qualification`) | Pip-Boy UI Tweaks, Quality of Life ESPless (`bKeepPipboyLightOnCellChange`, `GetPipBoyMode`), MCM Extender requirement lists asking 960 |
| 970 (not reported) | 9.70 inventory/AddItem refresh and WASD menu changes (#3184) | Held for its own verified step PR (#3184) | Barter Categories Icons |

[observed: the installed mods' scripts and requirement lists, read locally; only the
gate numbers and the function/key names they call are kept] The "Mods gated" column.
[observed: original FNV 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.3 + Stewie's Tweaks v1005,
2026-11-22, two-pass `bNoCasinoBans` 0 vs 1 capture] The `4 -> 3` clamp on
`GetCasinoWinningsLevel`, quarter thresholds, and `SetCasinoWinningsLevel` earnings write
/ untouched-casino no-op in `blackjack.md` §4. [observed: 2026-11-24, `GSDocMitchellHouse`
(`0x00103DF9`), FNV 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 + JIP LN 57.30 with
10 official base masters, committed probe `w4c_b3_06_toggle_pipboy_tab` in
`tools/capture/nvse/catalogs/wave4c_extenders.json`] Live `GetPipBoyMode` closed (`0`) vs
open (`3`) semantics in `nvse_jip2.rs`. [design] Reporting `960` is an engine design
choice (the engine is not the plugin and has no version of its own; requires owner
approval per #3665); `970` (#3184) remains unpromoted until its full step verification
lands; `nvf-play` syncs `GetPipBoyMode` opening (`2`, `Phase::Raising`) and closing (`4`, `Phase::Lowering`) via `Game::ui.pipboy_mode` (while animation sub-phases `1`/`5` remain not built and headless `fnv-game` retains the `3`/`0` fallback), `bAutoReadNotes` is skipped (requires Book Menu Restored), and unrepresented
switches (`bScaleCriticalDamage`, `bWeightlessItems`) retain INI fallback. [hypothesis]
Live-game execution of the third-party Pip-Boy UI Tweaks and Quality of Life ESPless mod
files against retail game data remains separate acceptance testing beyond the synthetic
clean-room E2E suite.

## 6. Open (to observe)

The version/revision/beta split of 6.4.9; GetPluginVersion's name matching (case,
spaces); plugins newer than the wiki's list; the versions of SUP, JIP PP LN, AnhNVSE, NX,
UIO, CacheUI and hNVSE, the registered names and versions of ITR, One111, ppNVSE and
vheyes-nvse, and IsDLLLoaded's file names for all of them (observer check 118).
