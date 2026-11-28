# The remaining xNVSE functions: tasks

*2026-10-30: core xNVSE is built in full (§3); what is left is the plugin
libraries (JIP LN, JohnnyGuitar, ShowOff, SUP, …), counted in §3. The next batch for the
whole extender integration is docs/handoff/nvse-next-batch.md.*

Written 2026-10-26, after the core plan (docs/handoff/extender-core-tasks.md, D1–D6) and
next to the JIP plan (jip-tasks.md). Same rules: no extender source; behaviour from the
wiki export `[documented]` or the game `[observed]`.

## 1. Where they go

- **Pure functions** (strings, arrays, math, conversions) stay in the script VM,
  `crates/fnv-script/src/vm.rs` (and `vm/jip.rs` for JIP).
- **Functions on the game** (records, inventories, lists, references, settings) go in
  `crates/fnv-game/src/nvse.rs`, reached by name: a function the VM does not build is
  passed to `Host::ext_call(name, on, args)`, which the game answers from `nvse::call`.
  A new family can be its own file next to it, so two sessions do not edit one file.
- Record fields come from `GameData::record_fields` (schema-decoded subrecords, enums as
  numbers; add a record type to `FIELD_SIGS` when a getter needs it).
- Check each getter headless with `fnvtool script run --actions <doc>` and `query`
  actions on the real data (the jip-records pattern), and against in-game probes when a
  value cannot come from a record.

## 2. Built so far (nvse.rs, 2026-10-26)

The GetWeapon… getters (clip rounds, damage, ammo, ammo use, action points, aim arc,
animation fields, VATS chance, crit chance/damage/effect, fire delays and rate, flags,
grip, limb damage mult, ranges, spreads, projectile count and form, reach, regen rate,
reload animation, skill and strength requirements, resist type, rumble, sight FOV and
usage, skill, type, is automatic, has scope, long bursts), GetWeight, GetItemValue,
GetBaseHealth, GetArmorArmorRating, GetArmorDT, GetAmmoSpeed, GetAmmoConsumedPercent,
GetAmmoCasing, GetName, GetParentCell, GetInventoryObject, GetEquippedObject, and the
form-list functions ListGetNthForm, ListGetFormIndex, ListAddForm, ListRemoveForm,
ListRemoveNthForm, ListClear (session-only changes [documented: ListAddForm]); the
delayed calls (`delayed.rs`); GetFirstRef/GetNextRef/GetNumRefs and the …InCell forms,
GetRefs/GetRefsInCell, GetParentWorldspace, IsPersistent, GetOwner (`nvse_refs.rs`); the
variables by name (Get/Set/HasVariable, Get/SetRefVariable, Get/SetStringVariable,
GetArrayVariable), GetScript, IsScripted, GetRace, GetNthModName, GetNumLoadedMods,
GetEquipType, GetRepairList, GetModelPath, GetIconPath; the input functions
(`script_input.rs`, task D6). In the VM: the character and bit functions (IsDigit …
ToLower, CharToAscii, AsciiToChar, Get/Set/ClearBit, shifts, Logical…, IntToBin),
GetBaseObject, ListGetCount, GetNumItems, IsFormValid, IsReference, the D1–D5 tables.

## 3. Not built: the census (2026-10-30)

Counted by calling every extender function of the catalogue once
(`cargo test --release -p fnv-game --test synthetic -- --ignored extender_census
--nocapture`; the names go to `target/tmp/extender-census.txt`). A function counts as
not built when the call ends in "not built yet". Each name is called once (a name two
plugins share counts under the first), with placeholder arguments, then fewer when the
compiler takes fewer than the catalogue lists; the statements (`let`, `while`, `ForEach`,
`Loop`, `Break`, `Continue`, `eval`, `search`) are syntax and skipped (#1022). The
catalogue's empty argument templates (`{{FunctionArgument |Name = |Type = }}`, a page's
placeholder for no arguments) are no longer read as an argument (14 entries).

GetFOSEVersion, GetFOSERevision and GetFOSEBeta are not New Vegas functions: the game with
xNVSE 6.4.9 gives them no opcode [observed: schemas/script/command_opcodes.json, from
GetCommandOpcode in the original], so no script can call them. They are left out.

**Core xNVSE (origins NVSE and FOSE1): all 609 built.** SetNthPackage from in-game probes
(#1021, ext-last-core.md §4); PrintActiveTile on the tile tree's pointer (#957, #1021).

Built in the 2026-10-30 batch (`nvse_rest.rs`, the VM, `ui_functions.rs`): SetEyes,
SetHairLength, SetNPCWeight, SetActorBaseFlagsHigh, SetOwningFactionRequiredRank,
SetPackageTargetDistance, SetRefCount, SetCurrentQuest, SetStringGameSettingEx,
SetAltControl, MenuHoldKey, MenuReleaseKey, ShowLevelUpMenu, GetUIFloatInherited,
CompileScript, PrintVar, DebugPrintVar; PrintF, PrintDebugF, Ar_DumpF, DecompileScript,
DumpDocs and DumpCommandWikiDoc write nothing [decision: NVForever writes nothing beside
the game]; GetFontFile and GetNthPerkEntryString (JIP and LU).

JIP batch 4 (2026-10-30, #955, `nvse_jip4.rs`, docs/spec-notes/jip-traits-4.md): 46 built,
JIP 390 → 344. The trait and flag getters and setters of classes, combat styles, lights,
grass, texture-set textures, climates, quests, faction reputations, cells (climate,
lighting template, noise texture), waters (sound, noise texture) and GetSoundFlag; and
Get/SetGameDifficulty, GetSessionTime, GetPCDetectionState, GetRefType, GetTeleportDoor,
GetDelayElapsed, Get/SetGlobalRef, GetEditorPosition, GetIsPoisoned. Of the 344 left, the
owner's mod lists call 40 (`ToggleHUDCursor` 20 calls, `SetMaterialPropertyValue` 21,
`SetUIFloatGradual` 18, the rest under 15), nearly all on the engine (cursor, NIF
materials, menus); the next record families are destruction data, body-part data, land
textures, water traits and the texture sets' decal traits (DODT, not decoded yet).

Built since: Anh 31 of 34 (#1015, ext-anh.md; not installed in the probe game, so from the
pages alone) and ShowOff 28 (#1010, ext-showoff.md §5, probe set showoff2 49/50).
JohnnyGuitar batch 1: 20 (#1007, ext-jg1.md, probe set jg1 71/71). Census after it: 1,441.
JohnnyGuitar batch 2: 13 (#1007, ext-jg2.md, probe set jg2 37/38).
JIP batch 5: 27 (#1006, jip-records-5.md, probe set jip5 99/100).
ITR batch 1: 62 (#1011, ext-itr.md §1-6, `nvse_itr1.rs` and `vm/itr.rs`; not installed in
the probe game, so from the pages alone). Census after it: 1,216 (ITR 119 → 57).
ITR batch 2: the other 57 (#1011, ext-itr.md §7-12, `nvse_itr2.rs`, the navmesh lender
`nav_ask.rs`). Census after it: 1,159 (ITR 0).
ITR probes (itr-nvse.dll installed 2026-11-01): set `itr`, lab `L5.ext.itr` 91 of 92 rows
match, the rest explained (ext-itr.md §13); 13 first-run differences fixed to follow the game. Census with the small libraries merged: 1,067 (ITR 0).
The small libraries (2026-11-01): 92, every one of kNVSE 16 (#1018, ext-knvse.md, probe
set knvse2 29/31), JIP PP LN 10 (#1019, ext-jippp.md), vheyes-nvse 21 (#1016, ext-vheyes.md),
CacheUI 8 (#1013), Anh 3 (#1015), NX 1 (#1017) and the #1020 plugins 33 (ext-small.md).
kNVSE's file-based side, which the census does not count (it counts functions): the
AnimGroupOverride folders (#1461, knvse-anim-folders.md), read and matched game-side;
the engine plays their standing locomotion files only.
Census after it: 1,186 (from 1,278).
One111: all 51 (#1012, ext-one111.md, probe set one111 117/118 after OneOneOneOne.dll was
installed). Census after it, with the small libraries merged: 1,138.
JohnnyGuitar batch 3: 24 (#1007, ext-jg3.md, from the pages and the records, not probed).
Census after it, with One111 merged: 1,111; JohnnyGuitar 242 left.
ppNVSE: all 42 (#1014, ext-pp.md, probe set pp 35/37, the two left the probe save's
third-person camera). Census after it, with ITR, One111 and JohnnyGuitar batch 3 all merged: 950.
JohnnyGuitar batch 4: 19 (#1007, ext-jg4.md, from the pages and the records, not probed;
branched from main beside batch 3). Census after it, with batch 3 and ppNVSE merged: 931;
JohnnyGuitar 223 left.
JIP batch 6: 27 (#1006, jip-records-6.md: destruction data, creature data, messages; from
the pages and the records, not probed). Census after it, with JohnnyGuitar batch 4 merged: 904
(JIP 311 → 284).
JIP batch 7: 27 (#1006, jip-records-7.md: ammunition, ammo effects, projectiles, explosions,
impact data, weathers and climates, armour; from the pages and the records, not probed).
Census after it, with batch 6 merged: 877 (JIP 284 → 257).
SUP batch 1: 19 (#1008, ext-sup1.md, from the pages; SUP is not installed here, nothing
probed). Census after it, with JIP batches 6 and 7 merged: 858; SUP 122 left.
Lutana batch 1: 92 (#1009, ext-lutana.md: challenges, casinos, recipes, actor AI data,
cells and water, zone flags, activators, terminals and locks, form flags, weapon fields,
load screen types; from the pages and the records, not probed). Census after it: 765;
LU 145 → 53.
ShowOff batch 3: 38 (#1010, ext-showoff.md §6: the equipped-item family, the seeded
randomizer, aux timer start and update handlers, temporary faction relations, a
reference's radiation; from the pages, not probed). Census after it, with Lutana batch 1
and #1775's TTW functions merged: 718 (ShowOff 111 → 73).
JIP batch 8: 47 (#1006, jip-records-8.md: actor base data, actor effects, ingestibles and
base effects, weapons, idles, body parts, cells and water; probe set jip8 282 rows, 281 match
after the fixes, the one left explained). Census after it: 634 (JIP 248 → 201).
SUP batch 2: 16 (#1008, ext-sup2.md: the dialogue setters of SUP with JohnnyGuitar's related
topics, said-once and parent-topic functions and ShowOff's response-text setters, read by the
dialogue through `crate::dialogue_edits`; probed, lab L5.ext.sup2 37 of 38). Census after it:
702 (SUP 122 → 114, JohnnyGuitar 222 → 216, ShowOff 73 → 71).
SUP batch 3: 21 (#1008, ext-sup3.md: the fly camera's place, the angles to a point, positions at
an angle and distance, the INI readers; probed, lab L5.ext.sup3 29 of 35, the rest explained; an
axis argument beside a reference named `Z` fixed in the compiler). Census after it: 697 (SUP
122 → 101), with batch 2 merged: 681 (SUP 114 → 93).
JohnnyGuitar batch 5: 32 (#1007, ext-jg5.md: alternate textures, FaceGen model flags and
icons, worldspace map textures and zones, region map names (REGN RDMP added to the schema)
and the location name, camera shots, body part strings, explosion and projectile sounds,
equip types, recipe and weapon mod lookups, actor effect types, the time played, GetRGBColor;
probed, lab L5.ext.jg5 88 of 88).
Census after it: 649 (JohnnyGuitar 216 → 184).
With JIP batch 8 merged: 602 (JIP 201, JohnnyGuitar 184) [observed: the extender_census
test, 2026-11-08].
JIP batch 9: 23 (#1006, jip-ui-9.md: the HUD cursor, the mouse, vanity wheel and camera
collision switches, CloseActiveMenu, the hotkeys' inventory references, the Pip-Boy radio,
the game volumes, SetFontFile, SetSystemColor, the health bar's actor, immortal mode,
interaction by type, GetPosEx, the NIF block material values and the Alt translation and
rotation; engine side in `script_switches.rs` and `nif_mirror.rs`; not probed). Census
after it: 579 (JIP 201 → 178).
JohnnyGuitar batch 6: 37 (#1007, ext-jg6.md: actor values by code, UnsetAV, life states,
temporary ingestible effects, available perks, hotkey slots, the owner's trespass alarm,
visited and expired cells, lighting template cells, the moon phase, radios, talking
activator actors, prompt overrides, primitives, effect shaders, armed mines, CallPerRef and
CallPerMobileObject; probe set jg6 written, not run yet). JohnnyGuitar 184 → 147.
JohnnyGuitar batch 7: 38 (#1007, ext-jg7.md: the combat music, level-up menu, menu arrow key,
yield timer, viewmodel clip, NiLOD and save switches, UI update sounds, reputation and map
marker icons, barter hiding, the queued cinematic and objective texts, TogglePipBoy, the
engine requests, HSVtoRGB and RGBtoHSV into script variables, GetJohnnyPatch,
GetDefaultHeapSize, IsMenuPaused; probe set jg7 written, not run yet). JohnnyGuitar 147 → 109.
JohnnyGuitar batch 8: 31 (#1007, ext-jg8.md: the last 14 event handlers (failed quests, lost
limbs, seen cells, processing levels, NPCs' actor values, reputations, NPC responses, sleep and
wait, general subtitles, the crosshair, settings, keyboard selection, radio sounds, taking items
back), the custom map marker, MD5File and SHA1File, IsBSALoaded, the DDS texture getters,
GetPixelFromBMP, Ar_Shuffle and Ar_SortEditor, the system colours, the sleep/wait menu state,
TuneRadioRef; probe set jg8 written, not run yet). JohnnyGuitar 109 → 78.
JohnnyGuitar batch 9: 31 (#1007, ext-jg9.md: sounds played from files' paths with fades,
PlaySoundFade, StopSoundLooping, PlayHolotape and StopHolotape, terminal menu items added and
removed, the audio markers, Jump, SetAutoMove, SetPlayerMovementFlags, SetVelEx,
GetLinearVelocity, GetRunSpeed, the karma title, the item effect line, the spread, grenade
hold, compass hostile, sky colour and camera FOV; probe set jg9 written, not run yet).
JohnnyGuitar 78 → 47.
ShowOff batch 5: the last 37 (#1010, ext-showoff.md §8: the level-up and pause menus on request, live
explosions, the shot switches, fast travel, detection and combat counts, VATS and item values, the
player's small states; from the pages, not probed). Census after it, beside batch 4: 565 (ShowOff 71 → 34;
0 with batch 4).
SUP batch 6: 22 (#1008, ext-sup6.md: the mouse, the cursor's traits, debug texts, the screen,
fonts and plugin header texts, the HUD flags as JohnnyGuitar's override, the lockpick menu and
the throw held, the engine's side in nvf_play::sup_screen and cursor.rs; GetPCTrait 0 by
decision; not probed yet, set sup6). Census after it: 580 (SUP 93 → 71).
Lutana batch 2: 29 (#1009, ext-lutana.md §9: map markers and radio data decoded on placed references,
hotkey units, the INI writers, skill magazines, ingestible flags, the quest items switch, form counts and
form plugins; from the pages and the records, not probed). Census after it: 573 (LU 53 → 24).
ShowOff batch 4: 34 (#1010, ext-showoff.md §7: matrices and quaternions, SayTo topics, the INI cache,
JSON files and folders through NVForever's layer, the pickpocket, quest-message and ammo-regen
switches, the corner-message handler, ResetInteriorAlt, SetOwnershipTemp, the enable-parent
functions; from the pages, not probed). Census after it: 568 (ShowOff 71 → 37).
SUP batch 5: 42 (#1008, ext-sup5.md: Excel workbooks (an .xls reader and writer of our own,
MS-CFB/MS-XLS), JSON files, file sizes and times, the load-order dump, screenshots through the
engine and QR codes; the network, desktop, clipboard and system-font functions give their
nothing values by the owner's decisions of 2026-11-08; not probed yet, set sup5). Census after
it: 560 (SUP 93 → 51).
With both ShowOff batch 4 and SUP batch 5 merged: 526 (ShowOff 37, SUP 51) [hypothesis: 602 − 34 − 42;
the extender_census test decides].
Lutana batch 3: the last 24 (#1009, ext-lutana.md §10: the activator prompt, ANIO and LSCT fields and the
loading screen's texture as session record edits, race, template and template flags, CopyFaceGenFrom
through the appearance changes the engine draws, SetBaseForm, EquipItemData and SetEquippedData,
SetPerkRank, the perk entry setters read by the perk effects, GetPlayerRegions, the cell enter and exit
events, GetCellRefs, SetVariableFloat, BaseAddItemHealth; set lu3, not probed yet). LU 24 → 0.
JIP batch 10: 36 (#1006, jip-scripts-10.md: quest objectives and their targets, the quest lists,
SetStageAlt, perks and factions, script variables added at run time (saved), scripts and event
blocks switched off, FakeScriptEvent, linked and enable children, the references' extra data
(saved), script map markers, the text files through NVForever's layer; probed once, set jip10,
6 of 21 before the fixes that follow it, jip-scripts-10.md §7). Census after it: JIP 178 → 142.
SUP batch 7: 12 (#1008, ext-sup7.md: the NIF block interpolations run frame by frame through the
scene mirror's edits, their finish events, the vertices told by the engine's `nif_mirror.rs`,
the HUD bars' script-variable (a variable handed over by place, `Host::var_place`) and UDF
sources; not probed yet, set sup7). Census after it: SUP 29 → 17 (the 17 of batch 4).
With both JIP batch 10 and SUP batch 7 merged: JIP 142, SUP 17 [hypothesis: 378 − 36 − 12 = 330 in all; the extender_census test decides].
JIP batch 11: 40 (#1006, jip-actors-11.md: combat allies and targets, detection values and
ForceActorDetectionValue, packages, the actor's alpha, gravity, light, breath, movement,
velocity, leveled list and unarmed effect, base values of a base form, kill XP, radiation at
an NPC, ResetFallTime, active effects and effect lists, the equipped items, armour DR/DT by
condition and GetArmourPenetrated, SetBaseFactionRank, ClearDeadActors, the hardcore stages'
effects, SetConditionDamagePenalty; four nothing values by decision, §7; probed, set
jip11, 22 of 28 after the fixes that follow it, jip-actors-11.md §6). Census after it: JIP 142 → 102.
JIP batch 13: 39 (#1006, jip-physics-13.md: the collision objects' velocities, layers and
properties through fnv_phys::script, contacts, the ground under an actor and its material,
GetRayCastMaterial, IsGrabbable, ToggleObjectCollision, SetLocalGravityVector,
PushActorAwayAlt and PushActorNoRagdoll, TravelToRef and MoveAwayFrom as made Travel
packages, MoveToNode, MoveToReticle, MoveToFadeDelay, PlaceAtCell, RotateAroundPoint,
SetAngleEx, FaceObject and Turn with the turn clip, SynchronizePosition, GetReticleNode,
GetReticlePosAlt; not probed yet, set jip13). Census after it: JIP 102 → 63 on main, 29 with
batch 12 (#1929).
JIP batch 14, the last: 29 (#1006, jip-last-14.md: AddNewEffect, IsSpellTargetList, the
primitives, AuxVarGetFltCond, DismemberLimb, FireWeaponEx, PlayIdleEx, PlaySound3DNode,
SetNoGunWobble, SetVATSTargetable and GetVATSTargets, SetRefrModelPath, SwapTextureEx,
GetBlockTextureSet, the detonation timers, SetProjectileRefWeapon, GetSelfAsInventoryRef,
the sequence frequencies; six nothing values by decision, §6; not probed yet, set jip14).
Census after it: JIP 102 → 73 on main, 0 with batches 12 (#1929) and 13 (#1931).
SUP batch 4: the last 17 (#1008, ext-sup4.md: random array keys, invalid references, Ar_SetFloatsFrom through
the VM's output variables, CallFunctionNextFrame (kept across a load with removeOnLoad 0), the compass targets,
the last save's and load's names and sizes told by the engine, the cells and map markers near the player,
MarkScriptOnLoad, KillAll2, the caught-pickpocketing flag, TranslitText; set sup4, not probed yet). SUP 17 → 0.
JohnnyGuitar NIF and camera slice: 21 (#1007, ext-jg-nif-camera.md: distances to drawn nodes and the hit point,
bounds, particle systems' values through the scene mirror and the engine's live systems, alpha and stencil
properties read from the models, switch nodes through the block flags, the sequences playing, Update3DAlt, the
camera's place, overrides and WorldToScreen through `nvf_play::script_camera`; set jgnif, not probed yet).
Census after it: JohnnyGuitar 78 → 57.
JIP batch 12: 34 (#1006, jip-world-12.md: ForceClimate and SetCurrentClimate drawn by the sky,
the wind, the moons' textures, TriggerLightningFX, the image space modifiers on, the buffered
cells, exterior and persistent cells, north rotations, the light at a point, fast travel and
the load order, the game settings as a map, purged references and internal markers, the barter
trade and its price multipliers, the item menus' filters and lists, the recipe menu and
AddRecipeCondition, ShowQuantityMenu and ShowTextInputMenu with their engine menus,
GetWheelDisabled; GetStringHash 0 by decision, §2; not probed yet, set jip12). Census after
it: JIP 142 → 108 on main, 102 → 68 with batch 11 (#1915).
JohnnyGuitar batch 10: the last 26 (#1007, ext-jg10.md: navmesh obstacles, triangles through `nav_ask` and paths,
weapon poisons (GetEquippedWeaponPoison reads them), EjectCasing, DisableMuzzleFlashLights, the scope picture and
RemoveScopeModelPath, FaceGen values by type, body part highlights, GetLandTextureUnderFeet through the engine's
land-texture cast, RemapLand, RefreshIdle, ShowBarberMenuEx, TriggerScreenSplatterEx, the prompt and repair menu
refreshes; engine follow-ups in §3 of the note; set jg10, not probed yet). Census after it: JohnnyGuitar 78 → 52
on main, 0 with #1928 and #1930.

**The plugin libraries: 1,519 left** (recounted with #1022's census). JIP LN 349,
JohnnyGuitar 313, SUP 184, LU 145, ShowOff 139, ITR 119, One111 51, CacheUI 45, PP 42,
Anh 34, vheyes 21, NX 18, kNVSE 16, JIPPPLN 10, Command Extender 8, Overcharge 6, and 19
more in plugins with four or fewer. One issue per library, listed in #1027. The next batches
should follow what mods call: `fnvtool ext scan` over a mod list counts the functions
their scripts use (docs/handoff/nvse-next-batch.md).

## 4. Order

Done 2026-10-26: references and cells, delayed calls, input, setters, variables,
factions, packages, appearance, weapon mods, quaternions, mod local data, clones, the
ammo / condition / inventory-reference part of "other", then the rest of "other" and the
player-control locks and hotkeys (`nvse_other.rs`; the engine reads
`Game::controls_locked`). Still open from "other": GetPaired (with the tokens),
GetSoldItemInvRef (OnSell is not dispatched yet), GetCurrentQuestObjectiveTeleportLinks,
GetFalloutDirectory (the game does not know its folder), GetCommandOpcode (debug group).
Hypotheses waiting on the peer's probes: the AltEx bits of looking/rollover/sneaking
(1, 5, 6), SetDoorTeleport moving the player, RunScript on a non-function script, the
console echo's start state. Next:
1. (done)
2. (done 2026-10-26: packages, paths and form flags; `Game::package_def` and
   `Game::package_list` carry the scripts' package changes to every reader, the record
   header flags are kept in `GameData::form_flags`.) Claimed by the peer session the
   same day: quaternion Ex forms, settings and INI, tokens (GetPaired with them),
   event-handler lists, debug and docs (merged with #844). Animations, generic forms
   (`nvse_lists.rs`) and the console functions (`nvse_console.rs`, each runs
   NVForever's console command; output only with the console echo on) done the same
   day; the tile tree (#834) merged into this branch and its UI functions wired into
   `nvse::call` (`ui_functions.rs`), SetUIStringEx and InjectUIComponent formatted by
   the VM.
3. Was: Packages and paths setters, form flags (needs the records' header flags kept).
3. Event-handler lists, tokens, animations, generic forms.
4. Console (`con_*`) and debug/documentation commands; UI waits on the owner
   (extender-compat.md §5.6).

## 5. Built by the peer session (claude/nvse-core-rest, 2026-10-27)

Probed first in the original game (set nvse-sixth of tools/capture/jip_probes.py, #828):
- **Settings and INI** (`nvse_settings.rs`): GetNumericINISetting (-1 when missing or
  without a ":category"), GetStringIniSetting (no value when missing), SetNumericINISetting
  (1 / 0), SetStringIniSetting, on `GameData::ini_texts` (the host passes the game's INI
  text; `fnvtool script run --ini`), changed for the session only. `iSize W:Display` and
  `iSize H:Display` read the live window's size once the engine tells it, above the INI
  text (the original sizes its window from them; NVForever does not) [design]. Not built:
  SetStringGameSettingEx (the original refuses it at compile: it is deprecated).
- **Tokens** (`nvse_tokens.rs`): the five functions, with no item a token (a carried
  stimpak keeps 0 in the game; the Category:Tokens page is missing). GetPaired: not built.
- **Event-handler lists** (`nvse_handler_lists.rs`): GetEventHandlers (StringMap /
  priority Map / ar_Null for an event nobody knows), DumpEventHandlers,
  GetHigher/LowerPriorityEventHandlers, IsEventHandlerFirst/Last.
- **Debug** (`nvse_debug.rs`, the VM): GetCommandOpcode from
  `schemas/script/command_opcodes.json` (every catalogue name run through GetCommandOpcode
  in the game: 2,922 opcodes, all 455 vanilla ones agreeing with the catalogue),
  GetDebugSelection (none: the console selection is the engine's), PrintDebug,
  GetDebugMode, Assert (a script error, the script goes on), Ar_DumpID. Not built: the
  file writers (PrintF, PrintDebugF, Ar_DumpF, DecompileScript, DumpDocs,
  DumpCommandWikiDoc: owner decision J9), CompileScript, PrintVar/DebugPrintVar (need the
  variable's name from the compiler).
- **Quaternion Ex forms** (vm/quat.rs): the nine, writing the variables given.
