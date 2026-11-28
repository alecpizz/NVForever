# JohnnyGuitar NVSE, batch 8 (2026-11-09, #1007)

The remaining event handlers, the player's custom map marker, file hashes, loaded
archives, DDS texture headers, BMP pixels, array shuffling and sorting by editor ID, the
system colours, the sleep/wait menu's state and radio tuning, in
`crates/fnv-game/src/nvse_jg8.rs`. Sources: the GECK wiki export [documented: the page
named, in our own words]; RFC 1321 (MD5) and RFC 3174 (SHA-1); the DDS layout of
`dds.md` §1; the BMP file and info headers [documented: the format's public headers].
Not probed yet: probe set `jg8` (`tools/capture/jip_probes.py`) and lab scenario
`L5.ext.jg8` are written, not run. Tests: `crates/fnv-game/tests/jg8.rs`.

31 functions. Census with batch 7 (#1908): JohnnyGuitar 109 → 78.

## 1. Event handlers

Every one takes `set|remove function flag [filters…]` [documented]. Setting the same
function with the same filters again replaces it; `0 …` with the same function and filters
removes it. The handlers are the session's state (`OtherState::jg8`), so a loaded save
starts without them [documented: "flushed every game load"]. A filter given as a form
matches that form, a reference of that base, or a member of that form list [documented:
"Formlists are also accepted as filters"]. A number filter matches an equal number.
`XMarker` (`0x0000003B`) in a form filter slot means no filter on
`SetOnReputationChangeEventHandler` dispatch (`xm=1`) and registers/unregisters returning
`0` with form type `21` on `SetJohnnyOnCrosshairEventHandler` (`xh(0,0)`) [observed
2026-11-24: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 + JIP LN
57.30, 10 official base masters, `GSDocMitchellHouse` (`0x00103DF9`), isolated clean-room
staging copy, committed probe `w6t2p2_12_jg_reputation_and_crosshair_xmarker`
(`tools/capture/nvse/catalogs/wave6c_jg8_xmarker_wildcard.json`):
`PROBE|w6t2p2_12_jg_reputation_and_crosshair_xmarker|xh(0,0)|fame(n=0,rOk=0,ax=0,v=0.0)|inf(n=1,ax=1.08423e+09,v=3.0)|xm=1`;
no DLL or source consulted]; applying `XMarker` as wildcard to the remaining unprobed
`nvse_jg8.rs` handlers (`FailQuest`, `LimbGone`, `SeenData`, `ProcessLevel`,
`NpcActorValue`, `GeneralSubtitle`, `RadioPostSound`, `TakeBackItem`) is [hypothesis]; a 0
where a form goes also means no filter [hypothesis: scripts pass 0 for "any"]. A handler may
take fewer arguments than it is given.

| Function | Fired by | Arguments |
|---|---|---|
| SetJohnnyOnFailQuestEventHandler (SetOnFailQuestEventHandler) `[quest]` | a quest's first failure (FailQuest, a Fail Quest stage), through `Game::emit` | quest [documented] |
| SetJohnnyOnLimbGoneEventHandler (SetOnLimbGoneEventHandler) `[actor] [limb]` | each part newly gone in `Game::dismember` | actor, limb [documented]; the limb is the body-part location the dismemberment uses [hypothesis: the page's 0-14 list is that index] |
| SetJohnnySeenDataEventHandler (SetSeenDataEventHandler) `[cell]` | a cell the player's loaded area holds for the first time (`Game::set_loaded`) | cell [documented]; "marked as seen" is read as first loaded [hypothesis: NVForever keeps no local-map fog] |
| SetOnProcessLevelChangeEventHandler `[actor] [new level]` | an actor entering or leaving the loaded area: 3 → 0 or 0 → 3, as GetActorsByProcessingLevel reads the levels (`nvse_gaps`) | actor, old, new [documented]; levels 1 and 2 never occur [hypothesis, as there] |
| SetOnActorValueChangeEventHandler `set function flag [code]` | every change of one of the player's values wherever it is written (`functions::actor::told`): scripts, aid and hit effects, hits; never another actor's, and not the derived skills NVForever works out on reading [documented: "limited to Player's actor value changes", the notes on derived skills]. Omitting `code` or passing `-1` acts as a wildcard, and `flag & 1 == 0` truncates (`trunc`) rather than rounds [observed 2026-11-24: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 + JIP LN 57.30, 10 official base masters, `GSDocMitchellHouse` (`0x00103DF9`), isolated clean-room staging copy, committed probes `w6t2p2_10_jg_on_av_change_timed` and `w6t2p3_10_av_m1_exact` (`tools/capture/nvse/catalogs/wave6c_jg_on_av_change_filter.json`): `PROBE\|w6t2p2_10_jg_on_av_change_timed\|h0=200.0,syn=0\|d06(ni=0:0.0->0.0,nf=0:0.00->0.00,m1=0,om=0)\|d12(ni=1:199.0->198.0)` and `PROBE\|w6t2p3_10_av_m1_exact\|int(n=1,200.0->199.0)\|flt(n=1,200.00->199.40)\|m1=1,om=1`; no DLL or source consulted]; skipping dispatch when `old.trunc() == new.trunc()` is [hypothesis]. A change a handler makes is not told again [our choice]. Before #2134 only the script functions told it, for any actor [observed: jip_events2, the original's first change was the player's Health +40 after the aid item] | code, old, new; truncated whole numbers unless flag bit 0 [documented; observed] |
| SetOnNPCActorValueChangeEventHandler `[actor] [code]` | the watched actor value functions (`functions::actor`), never for the player [documented] | actor, code, old, new; whole numbers unless flag bit 0 [documented] |
| SetOnReputationChangeEventHandler `[reputation]` | each reputation axis stored (`functions::reputation::store`) | reputation, fame, infamy [observed 2026-11-24: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 + JIP LN 57.30, 10 official base masters, `GSDocMitchellHouse` (`0x00103DF9`), isolated clean-room staging copy, committed probe `w6t2p3_12_rep_fame_infamy` (`tools/capture/nvse/catalogs/wave6c_jg8_reputation_args.json`): `PROBE\|w6t2p3_12_rep_fame_infamy\|s1(n=1,rOk=1,fame=5.0,inf=0.0)\|s2(n=2,fame=5.0,inf=3.0)`; no DLL or source consulted] |
| SetOnNPCResponseEventHandler `[emotion]` | each response of the player's conversation (`dialogue.rs` emit_response) | text, voice path, emotion, emotion value, response number [documented]; the path from the host's voice index (`VoiceLengths::response_path`, `fnv_voice::VoiceBank`), "" without one |
| SetJohnnyOnSleepWaitEventHandler `[mode]` | the hours of the wait menu starting to pass (`Game::begin_rest`) | 1 wait, 2 sleep; filter 0 both [documented] |
| SetOnGeneralSubtitleEventHandler `[speaker]` | a line said outside the player's conversation (Say, SayTo, greetings, `Game::say_line`) | speaker, the first response's text, topic [hypothesis: the page gives no arguments] |
| SetJohnnyOnCrosshairEventHandler `[form] [form type]` | the activation reach coming onto a reference (`nvf-play` activate.rs) | the reference [documented: SetJohnnyOnCrosshairEventHandler]; the type filter is the base's form type code (`fnv_script::vm::form_type_code`, covering all record types including `ACTI` = 21 [documented: Form Type IDs, SetJohnnyOnCrosshairEventHandler]) |
| SetJohnnyOnSettingsUpdateEventHandler (SetOnSettingsUpdateEventHandler) | `Game::jg8_settings_closed`, for the engine's settings menu | none [documented] |
| SetJohnnyOnKeyboardControllerSelectionChangeEventHandler (SetOnKBCTrlUIDeltaHandler) `[menu]` | keyboard/controller navigation moving tile highlight in `ui_input::step` (`Game::jg8_keyboard_selection`); compiles, accepts `0` as the menu filter on register/unregister, and returns `0` [observed: 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 + JIP LN 57.30 with the 10 official base masters, clean-room script probe `w4c_b3_08_jg8_event_regs` (`tools/capture/nvse/catalogs/wave4c_extenders.json`, #3678 / PR #3679), registering and unregistering a helper UDF with filter `0`: `limb=0,seen=0,proc=0,sub=0,kb=0`; no DLL or source consulted]; `MenuID` `0` or omitted matching any menu at dispatch time remains [hypothesis: menu ID filter 0 = any] | menu ID [hypothesis: mod UI handlers take `{iMenuID}` alongside `SetOnMouseoverChangeEventHandler`; observer check] |
| SetJohnnyOnRadioPostSoundAttachEventHandler, SetOnTakeBackItemEventHandler `[form] [form]` | kept; nothing fires them yet (follow-up) | radio, mode; [hypothesis] item, count |
| SetJohnnyOnStartQuestEventHandler, SetJohnnyOnStopQuestEventHandler, SetJohnnyOnAddPerkEventHandler, SetJohnnyOnRemovePerkEventHandler `[form]` (`nvse_gaps.rs`, `nvse_plugins.rs`) | quest start/stop and player perk gain/loss; `StartQuest` (`u=2, d=1, fl=1, xm=2`) and `AddPerk` (`u=1, d=1, fl=1, xm=1`) accept unfiltered, direct-form, `FormList` (`list_members`), and `XMarker` (`0x0000003B`) wildcard filters, and `StopQuest` (`qStopFl=1`) and `RemovePerk` (`pRemFl=1`) accept `FormList` filters [observed 2026-11-24: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 + JIP LN 57.30, 10 official base masters, `GSDocMitchellHouse` (`0x00103DF9`), isolated clean-room staging copy, committed probe `w6t2p3_09_11_flst_real` (`tools/capture/nvse/catalogs/wave6c_jg_quest_perk_flst_xmarker.json`): `PROBE\|w6t2p3_09_11_flst_real\|dyingFl=0\|qStart(u=2,d=1,fl=1,xm=2)\|qStopFl=1\|pAdd(u=1,d=1,fl=1,xm=1)\|pRemFl=1`; no DLL or source consulted]; direct-form and `XMarker` wildcard parity on `StopQuest` and `RemovePerk` is [hypothesis] | quest [documented: SetJohnnyOnStartQuestEventHandler; hypothesis: the arguments]; (perk, old rank, new rank) on add / (perk) on remove [observed: plugins3] |
| SetJohnnyOnDyingEventHandler `[actor]` (`nvse_jip3.rs`) | an actor dying (`Game::die`, `JgEvent::Dying`); unfiltered, `XMarker` (`0x0000003B`) wildcard, exact placed reference, and `NPC_` base-form filters all match, while a `FormList` containing an `NPC_` base form does not match [observed 2026-11-24: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 + JIP LN 57.30, 10 official base masters, `GSDocMitchellHouse` (`0x00103DF9`), isolated clean-room staging copy, committed probe `w6t2p2_09_jg_on_dying_all_filters` (`tools/capture/nvse/catalogs/wave6c_jg_on_dying_base_filter.json`): `PROBE\|w6t2p2_09_jg_on_dying_all_filters\|dead=1\|u=1,xm=1,ref=1,base=1,flst=0`; no DLL or source consulted]; applying `XMarker` wildcard to sibling `nvse_jip3.rs` handlers `CompleteQuest` and `ChallengeComplete` is [hypothesis] | dying actor reference [documented; observed] |

## 2. Other functions

| Function | NVForever |
|---|---|
| SetCustomMapMarker `x y z` | kept with the player's worldspace (or interior cell), 1; 0 with the player in no cell [hypothesis] (`Game::custom_map_marker`): the Pip-Boy's one custom marker, which both maps show and Place Marker sets, moves and removes, kept in saves (pipboy.md §5.3, #1954) |
| ClearCustomMapMarker | clears the custom marker and returns 1 even when no custom marker was set [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 with the 10 official base masters, clean-room script probe jg_29_custom_map_marker_set_clear_ret (SetCustomMapMarker/ClearCustomMapMarker): 1,1,1,1; no DLL or source consulted]. GetCustomMapMarker (`nvse_gaps`) stays a null reference: NVForever's marker is no reference [observed there: plugins2, 00000000 without one] |
| MD5File, SHA1File `path` | the hash of a file "relative to the Fallout folder" as upper-case hex [documented; observed: the case, probe set jg8], "" when it cannot be read. `Data\…` reads through NVForever's mod files (archives too), other paths the game folder, read only; no drive, leading slash or `..` |
| IsBSALoaded `name` | 1 for an archive the host loaded, by file name, any case [documented]; with `Data\` before the name, 0 [observed: probe set jg8]; the host's list (`mod_files::Roots::archives`: the engine's and fnvtool's archives) |
| GetTextureWidth, GetTextureHeight, GetTextureMipMapCount, GetTextureFormat `path` | from the DDS header of `Data\Textures\<path>`, loose or archived [documented]; DXT5 returns format `5` (width/height/mips `(64, 64, 0)` on `interface\icons\world map\icon_map_encampment.dds`) and a missing file returns `0` [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528, 2026-11-23/24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated clean-room staging copy (read-only `PrintToConsole` probes; zero writes to game install or Saves), `b3_jg8_01_texture_dds_queries` (`tools/capture/nvse/catalogs/wave4b_extenders.json`, #3679): `PROBE\|b3_jg8_01_texture_dds_queries\|ok=(64x64,mip=0,fmt=5)\|miss=(0,0)`, `w4c_b3_07_dds_texture_queries` (`tools/capture/nvse/catalogs/wave4c_extenders.json`, #3679): `PROBE\|w4c_b3_07_dds_texture_queries\|icon(64x64,mip=0,fmt=5)\|dirt(w=0,fmt=0)\|miss(w=0,fmt=0)`, and `w6t2_23_jg_dds_texture_format_and_bounds` (`tools/capture/nvse/catalogs/wave6_t2_observed_matches.json`): `PROBE\|w6t2_23_jg_dds_texture_format_and_bounds\|dds(w=64,h=64,m=0,fmt=5,miss=0)\|bndMiss(sz=4,0.0,0.0,0.0,0.0)`; zero DLL, PDB, or third-party source consulted]; formats 1–4 for DXT1–DXT4 and 0 for other formats remain [hypothesis] |
| GetPixelFromBMP `path rOut gOut bOut x y` | a 24-bit BMP's pixel into three variables [documented]; x and y counted from the top-left [hypothesis]; 0 0 0 without the file or pixel |
| Ar_Shuffle `array` | a new list in random order (the game's random source) [documented]. The VM hands array functions it does not know to the host now |
| Ar_SortEditor `array [reverse]` | a new list of the forms by editor ID, reversed with 1 [documented]; IDs compared without case [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 with the 10 official base masters, clean-room script probe jg_30_ar_sort_editor_case_order (Ar_SortEditor): 0000000F,00015169,0000434F ; 0000434F,00015169,0000000F; no DLL or source consulted], forms without one first [hypothesis] |
| GetSystemColor `id`, GetSystemColorAlt `id rOut gOut bOut` | 1 HUD, 2 HUD alt, 3 terminal, 4 Pip-Boy, 5 main menu [documented]: SetSystemColor's colour (`nvse_jip9`), else FFB642 for 1, 4 and 5, FF432A for 2 and 21E779 for 3 [observed: probe set jg8 on the owner's install; hypothesis: these are the owner's interface colour settings, which NVForever does not read]. GetSystemColor packs 0xRRGGBB and returns 0 for another id; GetSystemColorAlt writes `(R, G, B)` matching the packed bytes for ids 1–5, returns 0, and leaves `(rOut, gOut, bOut)` unchanged for invalid ids (0, 6) [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 with the 10 official base masters, `jg_31_get_system_color_packed` (`1.31071e+07,1.67289e+07,2.22194e+06,1.29761e+07,1.67583e+07,0`), and 2026-11-24 isolated clean-room staging run `GSDocMitchellHouse` (`0x00103DF9`), committed probe `w6_t1_08_jg_system_color_alt` (`tools/capture/nvse/catalogs/wave6_t1_jg_colors.json`): `PROBE\|w6_t1_08_jg_system_color_alt\|pack=(0,1.31071e+07,1.67289e+07,2.22194e+06,1.29761e+07,1.67583e+07,0)\|a0=0(111,222,333)\|a1=0(199,255,165)\|a2=0(255,67,42)\|a3=0(33,231,121)\|a4=0(197,255,255)\|a5=0(255,182,66)\|a6=0(111,222,333)`; no DLL, PDB, or third-party source consulted] |
| GetSleepWaitMenuState | 0 closed, 1 wait, 2 sleep [documented]; the engine marks the sleep as it opens menu 1012 (`nvf-play` wait.rs) |
| TuneRadioRef `[station]` | kept for the radio reference [documented]; the engine's radios do not read it yet |
| IsRadioRefPlaying | 0 [decision: the page says it "is currently broken and will crash the game", so no script relies on an answer] |

## 3. Follow-ups (engine)

Fire the radio sound and take-back handlers, call `jg8_settings_closed` when the
settings menu closes after a change, and fire `jg8_keyboard_selection` from any
hand-built menus not stepped through `ui_input::step`; draw the custom map
marker on the Pip-Boy map; let the radios read TuneRadioRef. The sound-by-path family
(PlaySoundFile, PlaySoundFromPath, PlaySound3DFromPath, PlaySoundFade, the Stop… calls,
StopSoundLooping, PlayHolotape, StopHolotape) is the next batch: it needs the engine's
audio to play files by path.

## 4. Observed (probe set jg8, 2026-11-09, the observer session)

- md5 `079F82235C010DDC16B9D5C794B76C4A|` (a missing file ""), sha1
  `FEE3C26918309D0AFCFA308B1ECCC307FD3979E3`: upper-case hex. NVForever's hex was lower
  case; now upper.
- bsa `1 1 0 0`: any case, but `Data\` before the name reads 0. NVForever took the
  prefix off; now it does not.
- colour `16758338 16728874 2221945 16758338 16758338 0`: 1, 4 and 5 FFB642, 2 FF432A,
  3 21E779, 6 nothing. NVForever answered amber for all five; now per id.
- These did not compile in the original, so they are written again: `texture` (`$sText`
  for a literal: the path is spelled out), `colour.alt` (as a statement: now `let`),
  `sorteditor` (array elements as GetFormIDString's argument: through ref variables
  first), `shuffle` (a sum of elements inside sv_Construct: summed with `let` first).
  Their answers wait for the rerun.
- `shuffle` did not compile in the rerun either (2026-11-10, #2033): `let aOut :=
  Ar_Shuffle (Ar_List 1, 2, 3, 4, 5)` then `let fB := aOut[0] + aOut[1] + …`. The data does
  not say which part is refused: an inline `ar_List` argument compiles for ShowOff's matrix
  functions, string `+` with an element compiles (nvse.n9 files.plugins), and numeric
  element arithmetic and Ar_Shuffle's argument appear in no other probe. NVForever keeps
  compiling it [decision: #2037, the owner's 2026-11-10 ruling to refuse what the original
  refuses needs a rule the data gives; ext-syntax.md §11]. A probe that splits the two
  (Ar_Shuffle on a variable; an element sum without Ar_Shuffle) would settle it.

## 5. Probe questions (set jg8)

1. `md5`, `sha1` of `Data\Credits.txt` (answered above).
2. `bsa`: IsBSALoaded with "Fallout - Meshes.bsa", lower case, with `Data\`, and a missing name.
3. `texture`: width, height, mip count and format of `interface\icons\world map\icon_map_encampment.dds`.
4. `colour`: GetSystemColor 1-5 and 6 (packed 0xRRGGBB?), GetSystemColorAlt 1.
5. `sleepwait`: GetSleepWaitMenuState outside the menu (0).
6. `sorteditor`: Ar_SortEditor on Stimpak, Caps001 and the 10mm pistol, and reversed.
7. `shuffle`: Ar_Shuffle keeps size and elements.
