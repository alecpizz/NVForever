# JohnnyGuitar NVSE, batch 7 (2026-11-08, #1007)

The switches and settings JohnnyGuitar gives scripts over the game's interface, music,
saves, camera and HUD, the queued screen texts, the colour conversions into script
variables, JohnnyGuitar.ini's patches and nvse_config.ini's heap size, in
`crates/fnv-game/src/nvse_jg7.rs`. Sources: the GECK wiki export [documented: the page
named, in our own words]; the original game through probe set `jg7`
(`tools/capture/jip_probes.py`, lab scenario `L5.ext.jg7`) [observed: "Observed
2026-11-08" at the end]. Tests: `crates/fnv-game/tests/jg7.rs`.

38 functions. The game keeps each value for the session (`OtherState::jg7`, not saved: a
loaded save starts with every switch at its default, as ToggleLevelUpMenu and
ToggleDisableSaves document [hypothesis for the others]). ToggleCombatMusic alone lasts
for the process.

## Functions

| Function | NVForever |
|---|---|
| ToggleCombatMusic `0/1`, IsCombatMusicEnabled | off or on "until the game is restarted" [documented], so for the process, not cleared by a load. The engine's music starts no battle music while it is off (`nvf-play` music.rs) |
| ToggleLevelUpMenu `0/1`, IsLevelUpMenuEnabled | the switch (`Game::level_up_menu_enabled`); loading a save turns it back on [documented]. `experience::level_up_due` and `xp_meter` hold the automatic level-up menu while off, releasing earned levels once re-enabled |
| DisableMenuArrowKeys, EnableMenuArrowKeys | the switch [documented: scripts still see the keys]; the menus do not read it yet (follow-up) |
| SetYieldTimer `seconds`, GetYieldTimer | counts down from the value set, in game seconds (`Game::time`), 0 when run out or never set [documented: a positive value keeps the player from yielding; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 with the 10 official base masters, clean-room script probe jg_25_yield_timer_and_viewmodel_clip_neg (GetYieldTimer/SetYieldTimer/GetViewmodelClipDistance/SetViewmodelClipDistance): 0.00,10.00,-5.00; no DLL or source consulted; hypothesis: game time countdown rate]. NVForever's combat has no yielding yet, so nothing else reads it |
| SetViewmodelClipDistance `distance`, GetViewmodelClipDistance | stores raw finite float values including negatives (`-5.00` reads back `-5.00`), 0 the default ("resets the value") [documented; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `jg_25_yield_timer_and_viewmodel_clip_neg` (`-5.00`) and `w4c_b3_05_jg7_bounds_heap` (`tools/capture/nvse/catalogs/wave4c_extenders.json`, #3679): `vc(0=0.0,neg=-5.0)|ui(badT=0,badF=0)|repIcon(ok=1,t0=0,t5=0,badF=0)|heap=200`; no DLL or source consulted]. The first-person camera does not read it yet |
| SetNiLODLevel `level`, GetNiLODLevel | the global LOD level override for NiLODNodes [documented], -1 before any SetNiLODLevel [observed 2026-11-08]. The renderer does not read it yet |
| ToggleDisableSaves `0/1` | per plugin: the load-order index (high byte) of the calling script's quest or reference names the plugin; saving waits until every plugin that disabled it calls 0, or a save is loaded [documented: "stored on a per-mod basis"]. A query or console script with neither counts as one plugin (0xFF) [documented: "the console … share one mod index"]. The engine's saves (`nvf-play` save.rs) refuse with "a script disabled saving"; ForceSave and SystemSave wait with them [documented] |
| SetUIUpdateSound `sound type` | type 1 quest failed, 2 quest added, 3 quest completed, 4 location discovered [documented]; another type or a form that is no SOUN changes nothing [hypothesis]. `quest_messages` plays `Game::ui_update_sound(type)` ahead of default notice sounds and the `discovered_sound` tweak [design] |
| SetCustomReputationChangeIcon `reputation tier path` | 1 when kept for tiers 1 and 4 on a `REPU` form, and 0 for tiers 0 and 5 or a form that is not a `REPU` [documented: returns success; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528, 2026-11-23/24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, read-only `PrintToConsole` probes `b3_jg7_01_ui_sound_and_rep_icon_bounds` (`tools/capture/nvse/catalogs/wave4b_extenders.json`, #3679): `PROBE\|b3_jg7_01_ui_sound_and_rep_icon_bounds\|t0=0,t1=1,t4=1,t5=0,badForm=0` and `w4c_b3_05_jg7_bounds_heap` (`tools/capture/nvse/catalogs/wave4c_extenders.json`, #3679): `repIcon(ok=1,t0=0,t5=0,badF=0)`; no DLL or source consulted]; tiers 2 and 3 (and other out-of-range tiers beyond 0 and 5) remain [hypothesis: the tiers]. The corner message does not show it yet |
| SetDisablePlayerControlsHUDVisibilityFlags `flags` | the bit mask kept [documented]; the HUD does not read it yet |
| ToggleNthPipboyLight `button on` | 0 Stats, 1 Items, 2 Data [documented]; the Pip-Boy does not draw them yet |
| PushUIQuestToTop `quest` | the quest kept (a QUST only; `Game::top_ui_quest`) [documented]; `pipboy::data` promotes the listed quest to index 0 of the Pip-Boy quest list while preserving the relative order of the rest [design] |
| HideItemBarterEx `item hide [side] [vendor]`, IsItemBarterHiddenEx `item [vendor]` | hide 0 shows the item again; side 1 keeps it shown on the player's side, 2 on the vendor's, none hides it on both [documented: HideItemBarterEx and the reader's notes, 0 shown … 3 hidden on both]; kept for the barter menu as that flag (`Game::barter_hide_flag` / `barter_hidden_for`; the vendor reference's own entry first, then its base form's, then the one for every vendor [hypothesis]). `menu` and `menu::model::Trade::refresh_trade` apply the flags on barter open and live sync. IsItemBarterHiddenEx reads 0: in the original it read 0 after every HideItemBarterEx, on the caps and on the 10mm pistol, with and without a vendor [observed 2026-11-08: jg7.barter, jg7.barter.pistol, outside the barter menu; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 with the 10 official base masters, clean-room script probe jg_40_barter_hide_ex_readback (HideItemBarterEx/IsItemBarterHiddenEx): 0,0,0; no DLL or source consulted] |
| QueueCinematicText `title subtitle [sound] [priority] [justification] [title font] [subtitle font]` | queued: priority 0 appends, 1 prepends, 2 clears the queue and shows at once; justification 0 left, 1 centre, 2 right; fonts are the [Fonts] slots [documented]. The HUD does not show the queue yet |
| QueueObjectiveText `text [completed] [multiple]` | queued [documented: QueueObjectiveText]; the HUD does not show it yet |
| TogglePipBoy `[tab]` | closes the Pip-Boy when it is open, else opens it on 1002 Inventory, 1003 Stats, 1023 Data [documented]; without a tab on the inventory [hypothesis: the page's "tab it closed on" is not kept by NVForever] |
| ExitGameAlt, RollCredits, StopVATSCam, SetCameraShake `multiplier seconds`, SetHUDShudderPower `power` | requests for the engine (`Game::take_jg7_requests`) [documented: each page's summary]; no engine system takes them yet (follow-up) |
| SetCustomMapMarkerIcon `marker|list path`, GetCustomMapMarkerIcon | the path for the marker reference, or for each member of a form list [documented]; an empty path clears the override back to the marker's type icon [hypothesis]; the getter reads it on the calling marker. Without one, the marker type's icon file (respecting `SetMapMarkerType`), `Interface\Icons\World Map\icon_map_<name>.dds` with the names of the world map's table (`MAP_MARKER_ICONS`, shared with the engine) [documented: "the default texture path"; observed 2026-11-08: the Goodsprings marker (type 3) read icon_map_encampment.dds in that spelling]; "" for a reference that is no map marker [hypothesis]. The Pip-Boy World Map and Local Map (`pipboy::data`, `tiles::map_menu`) now read `Game::custom_map_marker_icon` for discovered markers, falling back to the type icon when missing, while undiscovered markers keep `icon_map_undiscovered.dds`; drawing an undiscovered `FNAM=3` marker as discovered with its own icon (matching its observed `Vis=2, Trv=1` script values) is likewise [hypothesis] because the probes measured script values only and did not capture Pip-Boy icon drawing. For map-marker visibility script queries, authored `FNAM` flags read `0` (`FNAM=0`), `1` (`FNAM=1`), `2` with `GetMapMarkerTravel=1` (`FNAM=3`), and `0` with `GetMapMarkerTravel=0` (`FNAM=2`) before discovery (see `ext-lutana.md` §9) [observed: observer check 252 (#3468, `o31_mapmarker_visible`) and 2026-11-22 live map-marker probe on Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, JIP LN 57.3, JohnnyGuitar 5.28, fresh sandboxed world `coc GSProspectorSaloonInterior` via `nvf_capture` NVSE UDF script steps comparing Honest Hearts' `SouthernPassage` `0x01008885` (`FNAM=3`) and `FalloutNV.esm` `FNAM=0` markers] |
| HSVtoRGB `h s v rOut gOut bOut`, RGBtoHSV `r g b hOut sOut vOut` | hue 0-360 degrees, saturation and value 0-1, channels 0-255, through script variables, returning `0`; standard hexcone conversion with `HSVtoRGB` rounded to whole channels (`(30, 0.5, 0.5) -> (128, 96, 64)`) and `RGBtoHSV` dividing channels by `255` without input clamping (`(300, -10, 128) -> (333.290, 1.033, 1.176)`) [documented; observed: 2026-11-24, Fallout: New Vegas 1.4.0.525 (`040020D0`) + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 + JIP LN 57.30 (`5730`), 10 official base masters, isolated clean-room staging copy (read-only `PrintToConsole` probe; zero writes to game install or Saves), `GSDocMitchellHouse` (`0x00103DF9`), committed probe `w6_t1_07_jg_hsv_rgb` (`tools/capture/nvse/catalogs/wave6_t1_jg_colors.json`): `PROBE\|w6_t1_07_jg_hsv_rgb\|R=0(0.000,1.000,1.000)\|G=(120.000,1.000,1.000)\|M=(20.000,0.750,0.784)\|K=(0.000,0.000,0.000)\|H1=0(204.000,153.000,102.000)\|Hfrac=(128.000,96.000,64.000)\|oob=(333.290,1.033,1.176)`; no DLL, PDB, or third-party source consulted]. The three outputs go to their variables through the VM's out-parameter table (`fnv_script::vm::out_params`) |
| GetJohnnyPatch `id` | 1 for id 1 (bLoadEditorIDs, "no longer optional") [documented]; ids 3-8 read bFixFleeing, bFixItemStackCount, bFixNPCShootingAngle, bNoMuzzleFlashCooldown, bReset3rdPersonCamera and bJIPFixes from `Data\NVSE\Plugins\JohnnyGuitar.ini`, section MAIN [documented: the names; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 with the 10 official base masters, clean-room script probe jg_27_johnny_patch_and_heap_size (GetJohnnyPatch/GetDefaultHeapSize): 1,1,0,200; no DLL or source consulted]; 0 without the file or for another id |
| GetDefaultHeapSize | DefaultHeapInitialAllocMB from `Data\NVSE\nvse_config.ini`, any section [documented]; returns `200` (xNVSE default) when `nvse_config.ini` is absent [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `jg_27_johnny_patch_and_heap_size` (`200`) and `w4c_b3_05_jg7_bounds_heap` (`tools/capture/nvse/catalogs/wave4c_extenders.json`, #3679): `vc(0=0.0,neg=-5.0)|ui(badT=0,badF=0)|repIcon(ok=1,t0=0,t5=0,badF=0)|heap=200`; no DLL or source consulted]. Read only, through `mod_files::data_dir` |
| GetLocationSpecificLoadScreensOnly | the switch SetLocationSpecificLoadScreensOnly sets [documented] |
| IsMenuPaused `[menu]` | NVForever has no Real Time Menus, so 1 for any menu code, and without one 1 in menu mode [documented: "if Real Time Menus is not installed"] |
| AsmBreak | nothing [decision: NVForever runs no native debugger hooks for scripts] |
| RefAddr | 0 [decision: NVForever's forms have no memory addresses a script could use] |

## Nothing values

AsmBreak and RefAddr, for the reasons above.

## Follow-ups (engine)

Read by the engine in this batch: ToggleCombatMusic (music), ToggleDisableSaves (saves),
TogglePipBoy (the menus), ToggleLevelUpMenu (`experience::level_up_due` / `xp_meter`),
SetUIUpdateSound (`quest_messages`), PushUIQuestToTop (`pipboy::data` quest list),
HideItemBarterEx (`menu` / `menu::model::Trade`), and SetCustomMapMarkerIcon (Pip-Boy World
Map and Local Map). Still to read, each a change of its own system: the menu arrow keys, the
yield timer (no yielding yet), the viewmodel clip distance, the NiLOD override, the
reputation change icons, the HUD visibility flags under DisablePlayerControls, the Pip-Boy
button lights, the queued cinematic and objective texts, and the requests (exit to the main
menu, credits, the VATS camera, the camera shake, the HUD shudder).

## Observed 2026-11-08

The first run in the original (save nvfairport, JohnnyGuitar 5.28, xNVSE 6.4.9): 6 of the 8
rows that compiled matched; 9 of 17 probes did not compile there. Fixed since: IsItemBarterHiddenEx (always 0; the
rerun with the 10mm pistol read 0 too) and the marker's default icon (above), GetNiLODLevel's default -1. The music,
levelup, yield, lod and menupaused probes wrote a zero-argument command bare as an
sv_Construct argument, which the original's compiler refuses; they now put it in
parentheses (diagnostic forms read IsCombatMusicEnabled 1, IsLevelUpMenuEnabled 1,
GetYieldTimer 0, IsMenuPaused 0 outside menus and 1 with 1002, as NVForever). HSVtoRGB and
RGBtoHSV compiled at run time in none of seven forms, so their conversion stays a
hypothesis until a plugin-compiled script probes it. NVForever's compiler takes all nine
forms the original refuses (a follow-up for the compiler, not this batch).

The rerun (head de7253958): 13 of 14 rows equal, the 10mm pistol's barter row read 0 too,
so IsItemBarterHiddenEx now reads 0; lab `L5.ext.jg7` 14 of 14 against the private golden
`jip/jg7.json`. HSVtoRGB and RGBtoHSV are out of the runtime set and wait for a
plugin-compiled probe (observer-checks).
