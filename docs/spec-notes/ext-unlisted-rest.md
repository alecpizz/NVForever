# The rest of the registered extender commands with no wiki page (2026-11-13, #2326)

After the 33 that the mod lists call (`ext-unlisted-commands.md`, #2318), the installed
plugins register 205 more commands with no wiki page and no catalogue row, by nvse.log:

| Plugin | Count |
|---|---|
| JIP LN | ~50 |
| ShowOff | 41 |
| SUP | 46 |
| kNVSE | 20 |
| JohnnyGuitar | 17 |
| One111 | 8 |
| ITR | 6 |

20 more log entries named `NONE` are placeholders, not commands. No script in the owner's
mod lists calls any of these, so they go family by family. This note grows with each
batch. Code: `crates/fnv-game/src/nvse_unlisted2.rs`; ShowOff's string maps live in the
VM, `crates/fnv-script/src/vm/jip.rs`. Catalogue rows come from `PAGELESS` in
`tools/capture/script_functions.py`.

## 1. Sources

- **Names, opcodes and plugins**: nvse.log of the original game (xNVSE 6.4.9), read the
  same way as `ext-unlisted-commands.md` §1. The plugins, by their nvse.log load lines:
  JIP LN 57.30, ShowOff 184, SUP 855, kNVSE 37, JohnnyGuitar 528, One111 120, ITR 20203
  (no DLL is newer than 2026-11-10, so these are the versions of the 2026-11-13 runs). The opcodes of every command the catalogue
  gained in #2318 and here are added to `schemas/script/command_opcodes.json` from those
  log lines. That file still lacks the other commands of the plugins that were not
  installed when it was recorded (ShowOff, kNVSE, SUP, One111, ITR); that is a follow-up.
- **Argument shapes**: a compile-only run in the original. Each name was compiled with 23
  argument patterns (none; one to nine zeros; quoted strings; a ref variable; mixes; a bare
  `X`) inside `if 0 … endif`, so the game compiled each call and ran none [observed: the
  sandbox's stock game, save nvfairport, 2026-11-13]. The run was split into jobs of 150
  calls (one of 4,715 hung the game before its first frame). The generator is
  `jip_probes.py shapes` (`unlisted_rest_probes.shape_steps`: the 205 names and the 23
  patterns); the results stay private like the goldens. How to read the results:
  - A command that compiles with every pattern takes no arguments; the compiler then
    ignores extra words.
  - A command with parameters refuses extra words, which gives exact counts.
  - A string parameter also takes a bare word or a number, so it shows as
    `0 | "a" | rP | X`. A number or form parameter shows as `0 | rP`.

  These are [observed] for the counts and [hypothesis] for the kinds.
- **Behaviour**: probe sets in `tools/capture/jip_probes.py`, run unattended by nvf_capture
  v10 in the Blue Moon sandbox's stock game, save nvfairport, on 2026-11-13: `rest1` there,
  and `json1`, `json2`, `knv1`-`knv3`, `rest4`, `rest5` with the reruns of their risky rows
  (`json2r0`-`r5`, `knv2r0`-`r2`, `rest4r0`-`r5`, `rest5r00`-`r11`, one row per launch) in
  `tools/capture/unlisted_rest_probes.py`. Each has its job file in
  `tools/capture/jip-probes/` and a lab scenario `L5.ext.<set>` (parity-lab/scenarios/
  layer5_ext.json), save for knv2r0 and knv2r1, which stopped the game before any row. Rows
  are cited as `set.label`; the goldens are private. A "crash" below is a launch that ended
  before its last row: json1 after 37 of 52 rows, knv1 after 60 of 64, json2r0 and json2r1
  after the file was cached, knv2r0 and knv2r1 at once.
- **Not read**: no plugin DLL or source, and no nv-rs.

## 2. Network services, tests, logs, an address: neutral answers (batch 1)

NVForever answers 0 and does nothing for the commands below [decision: extender-compat.md
§5.8]. None of them was run in the original. The network ones would reach outside services,
and the rest test or reset the plugin itself or read the original's memory.

| Command | What it reaches | Shape [observed] |
|---|---|---|
| BBPostMessage, BBReportMessage, BBDeleteUserMessage, BBLikeMessage, BBHideMessage, BBGetUserMessages, BBGetUserInfo, BBGetInfoForReference, BBIsReferenceNote, BBOnCellChange, BBReturnAllMessagesToDefaultCell, BBGetCurrentNotesInCell, BBIsBBLoaded, BBAreMessagesFetched, BBGetBBKey, BBAPIKeyExist, BBNexusAPIKeyExist, BBIsTextEditMenuActive, BBGetTextMenuString, BBSetTextMenuTitle | SUP's bulletin board, a web service [hypothesis: by the names] | none, one id, one reference, one string, or (BBPostMessage) a string and a reference, (BBReportMessage) an id and a string |
| UpdateCheckerCheckForUpdates, UpdateCheckerReturnArray, GetSUPDateCheckerCurrentStage, GetSUPDateCheckerDoneStage, GetPastebinReaderCurrentStage, GetPastebinReaderDoneStage | SUP's update checker and pastebin reader (web) | `0 0 "a"`; one number; none |
| SUPTest, LunetteCMD, ERDTestFunc | the plugins' own tests | none; one number |
| SUPWRITETOLOG, DumpTileInfoChilds | SUP's own log | one string; an optional string |
| RefAddrxData | JohnnyGuitar: a reference's data in the original's memory | one number |
| NullArgs | JohnnyGuitar: answers 0 [observed: rest1.nullargs, nullargs.1] | an optional number |
| kNVSEReset | resets kNVSE | none |

BBIsBBLoaded's 0 lets a mod see that the board is not there.

## 3. ShowOff's string maps (batch 1)

`AuxStringMapArray…` (aliases `AuxStringMap…`) keep named maps from string keys to a
number, a form or a string. [observed: set rest1]:

- **One store for everyone.** The map is the same whether the caller is the player, Doc
  Mitchell or no reference (`sm.other.ref`, `sm.noref.*`). Names starting with `*` are kept
  apart, as JIP's temporary variables are [hypothesis: unsaved, like those]. NVForever keeps
  the others in the saved store.
- **Keys ignore case** (`sm.getfloat.case`).
- **Setters**: SetFloat, SetString and SetRef `name key value` answer 0; setting a key again
  replaces its value.
- **Getters**: GetFloat, GetString and GetRef `name key` give 0, "" or no form when the
  value is of another type or the key is missing.
- **GetValueType** `name key`: 1 number, 2 form, 4 string, 0 none (also for no such map).
- **GetSize** `name`: the number of keys.
- **GetKeys**: a list of the keys. The original returns keys in unordered hash-table order
  (neither insertion order nor alphabetical) [observed: Fallout: New Vegas 1.4.0.525 +
  xNVSE 6.4.9 + ShowOffNVSE 184 with the 10 official base masters, clean-room script probe
  `so_08_auxstrmap_keys_order`: inserting `Bravo`, `charlie`, `alpha` returned
  `alpha,charlie,Bravo`; no DLL or source consulted]. NVForever keeps deterministic
  lower-case key order [design].
- **GetAsArray**: a string map of the keys and values.
- **GetFirst / GetNext**: a `[key, value]` list each; after the last, no array.
- **GetAll** `0|1`: no array when the store has no map; with `0` a string map of the saved
  (non-`*`) maps by name and with `1` a string map of the temporary (`*`) maps by name
  [observed: getall.0, getall.1; Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 +
  ShowOffNVSE 184, 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base
  masters, read-only `PrintToConsole` probe `so_09_auxstrmap_getall_0_vs_1`
  (`tools/capture/nvse/catalogs/wave4a_extenders.json`, #3679, checking `ar_HasKey` on
  `AuxStringMapArrayGetAll 0` and `1` for `"w4_perm_map"` and `"*w4_temp_map"`):
  `PROBE|so_09_auxstrmap_getall_0_vs_1|1,0,0,1`; no DLL or source consulted].
- **SetFromArray** `name array [keep]`: the map becomes the array's string keys and values,
  or with `keep` non-zero gains them; answers 0 (`fromarray`, `fromarray.keep`,
  `fromarray.replace`).
- **EraseKey**: the number of keys left, 0 for a key that was not there.
- **ValidateValues**: the keys left. All values were alive in the probe [hypothesis: a value
  whose form is gone is dropped, as RefMapArrayValidate does].
- **Destroy**: 0 either way.

## 4. ShowOff's `_OLD` versions and IsJumping_BROKEN (batch 1)

Matrix_IsMatrix_OLD, Matrix_Transpose_OLD, Matrix3x3_GetQuaternion_OLD,
Quaternion_GetMatrix_OLD, Matrix_Dump_OLD, Matrix_ApplyOperationWithScalar_OLD,
Matrix_ApplyOperationWithMatrix_OLD, ListAddArray_OLD and TopicInfoSetResponseStrings_OLD
take the current command's arguments [observed: the shape run] and answered as the current
command wherever both compiled [observed: rest1.old.*/new.*: IsMatrix, Dump, ListAddArray
with the count before, the answer, the count after and after the removal all equal]. The
probe rows for the other matrix ones did not compile, in either version (a fault of the
probe script). NVForever hands each `_OLD` command to the current one [hypothesis: the same
for the rows not compared].

`IsJumping_BROKEN` read `1` in the earlier `rest1.old.isjumping` run, while bare `IsJumping`
does not compile (`Unquoted argument IsJumping`). Follow-up Wave 4C clean-room read-only
console probes resolved its live predicate [observed: Fallout: New Vegas 1.4.0.525 +
xNVSE 6.4.9 + JIP LN 57.30 + ShowOffNVSE 184, 2026-11-24, `GSDocMitchellHouse`
(`0x00103DF9`) and `Goodsprings` (`0x000DAEBB`), 10 official base masters,
`tools/capture/nvse/catalogs/wave4c_extenders.json` (#3679), no DLL or source consulted]:
- `w4c_a01_isjumping_indoor`: `gnd(j=0,doc=0,air=0,z=7360.3)|mid(j=0,air=0,z=7360.3)|post(j=0,air=0,z=7360.3)`
  — returns `0` for grounded `Player` and Doc Mitchell indoors.
- `w4c_a13_isjumping_deep`: `in(gnd=0,fallJ=1,fallA=1,post=0)|out(cell=000DAEBB,gnd=0,gndA=0,jmpFallJ=1,jmpFallA=1,post=0)`
  — returns `0` when grounded indoors and outdoors, flips to `1` while airborne/falling
  (even via `SetPos Z + 120` without pressing Jump, matching `IsInAir`), and returns `0`
  after landing.
- `w4c_a01b_isjumping_bare`: `COMPILE_FAIL` (bare `IsJumping` is unregistered).
NVForever delegates `IsJumping_BROKEN` to `IsInAir`.

## 5. Pure getters (batch 1)

- **StringContainsOnlyASCII** `text`: 1 when every character is below 128
  [observed: rest1.ascii.plain 1, ascii.high 0]. An empty literal does not compile in the
  original.
- **GetFuncCodeByName** `name`: the command's opcode, by name or alias, any case; -1 for
  none [observed: rest1.funccode.*: GetAV and GetActorValue 4110, AttachLight 10361].
- **GetFuncNameByCode** `code`: the command's name; an unknown code fails the call
  [observed: rest1.funcname.*: 4110 GetActorValue, 10361 AttachLight, 1 and 65000 "script
  call failed"]. NVForever gives the catalogue's name, else the longest of the opcode
  table's names for the code (an alias is the shorter) [hypothesis].
- **IsNewGame**: 0 in a loaded save [observed: rest1.isnewgame], 1 in a fresh New Game
  session [observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas
  1.4.0.525 + xNVSE 6.4.9 + SUP NVSE 855 with the 10 official base masters, clean-room
  script probe `sup_12_is_new_game` (`IsNewGame`): `1`; no DLL or source consulted].
  NVForever gives 1 from the New Game menu until a save is loaded.

## 6. SUP's JSON commands (batch 2)

These work on a JSON file under the game folder (a path starting `Data\` reaches Data, including NVForever's layer and VFS, decoded as UTF-8 with any UTF-8 BOM stripped [design]), by top-level key, sharing `Game::other.json.cache` with `JSONGetValue` (`nvse_sup5.rs`) [design]. Probed with a file of our own in the sandbox [observed: sets json1, json2; each command
that crashed ran alone]. In NVForever, files a mod writes go to NVForever's own layer,
never the game folder (`crate::mod_files`). Code: `crates/fnv-game/src/nvse_json.rs`.

- **JSONCacheFile** `path flag`: reads the file into a cache; 0, or -4 for no such file.
- **JSONIsFileCached** `path`: 1 or 0.
- **JSONToString** `path`: the cached copy in one line, keys sorted; "" when not cached.
- **JSONCloseFile** `path flag [n]`: `1` drops the cached copy and gives 0; `0` gives -5
  and keeps it.
- **JSONCacheFileFromString** `text name`: -3 for invalid JSON text [observed: json1/json2;
  observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525
  + xNVSE 6.4.9 + SUP NVSE 855 with the 10 official base masters, clean-room script probe
  `sup_18_json_cache_from_string` (`JSONCacheFileFromString`/`JSONGetValueType`): `-3`;
  no DLL or source consulted]; NVForever caches valid text under the name [hypothesis].
- **JSONGetValueType** `path key flag`: object 1, array 2, string 3, boolean 4, number 5,
  null 0, missing -2 [observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), Fallout:
  New Vegas 1.4.0.525 + xNVSE 6.4.9 + SUP NVSE 855 with the 10 official base masters,
  clean-room script probe `sup_17_json_get_value_types_and_paths`
  (`JSONGetValueType`/`JSONGetValue`/`JSONGetValueArray`): `1,2,3,4,5,0,-2 ; 1`; no DLL
  or source consulted]. Only top-level keys: `obj/inner`, `obj.inner` and `/obj/inner`
  all gave -2.
- **JSONGetValueArray** `path key flag`: an array of 3 for a 3-element array, an object
  gives 1, and a number gives 2 [observed: json1.arr.*, json2.arr.*; observed: 2026-11-23,
  `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + SUP
  NVSE 855 with the 10 official base masters, clean-room script probe
  `sup_31_json_get_value_array_shapes` (`JSONGetValueArray`): `3,1,2`; no DLL or source
  consulted]. (By contrast, `JSONGetValue` in `ext-sup5.md` §2 returns a tagged list
  `[type_code]` / `[type_code, scalar_value]` as observed in `w4d_37_sup_json_and_excel_edges`, #3802 / PR #3803.)
  NVForever gives the array's elements, an object's pairs as a string map,
  and nothing for a scalar [hypothesis; open].
- **The last flag of the readers and writers**: 0 is the file on disk, 1 the cached copy.
  With 0 the file is rewritten at once, indented by four spaces, and the cache keeps its
  old copy. With 1 a file that is not cached gives -4 (`json2.uncached.set.cache`).
- **JSONSetValueFloat / JSONSetValueString** `path key value flag [n]`: set the key; 1.
- **JSONAppendValueFloat / JSONAppendValueString**: add to the key's array; 1.
- **JSONEraseArrayElement** `path key index flag`: take an element out; 1.
- **JSONClearKey** `path key flag`: empty the key's object (`{}`); 1.
- **JSONEraseKey** `path key flag`: crashed the original with either flag
  (`json2r0`, `json2r1`). NVForever takes the key out and gives 1 [decision].
- **Float writes**: some came out as `true` in the original (4 appended, 2 set into the
  cache) and others as numbers (9, 10) [observed]. NVForever writes the number [decision:
  the plugin's fault, nothing a mod can rely on].

## 7. kNVSE's animation commands (batch 3)

Probed on a placed Doc Mitchell and the player [observed: sets knv1-knv3; the rows that
crashed ran alone]. Code: `crates/fnv-game/src/nvse_unlisted2.rs`.

- **GetAnimPathBySequenceType** `type [n]` (alias GetAnimPathByType), on an actor: the
  actor's current file for type 0 (`Characters\_Male\Locomotion\mtidle.kf` for Doc, the
  first-person idle for the player), "" for types 1-3; the call fails for a form given as
  argument (`knv1.pathbytype.form`). NVForever gives the actor's body clip for type 0
  [hypothesis: type 0 is that layer].
- **GetAnimWeight, GetAnimHandType, IsAnimAdditive, IsAnimQueued, GetAnimInterpPriority,
  GetAnimTextKeyTime** `path …`: 0, -1, 0, 0, -1 and -1, for any path, even the exact one
  GetAnimPathBySequenceType gave for the playing idle (`knv3.weight.var` … `textkey`).
  NVForever answers the same [hypothesis: they read kNVSE's own animation overrides, which
  NVForever has none of].
- **SetAnimInterpPriority, CopyAnimPriorities, RemoveAnimTextKey, QueueNextAnim,
  SetAnimAdditiveInterpsWeightMult**: 0, and no getter showed a change (`knv2.setprio` then
  `prio.after` -1; `queue` then `queued.after` 0). NVForever answers 0.
- **SetAnimCycleType, SetAnimDestFrame** `path n [n]`: 1 for an existing `.kf` file of the
  game and 0 for a missing `.kf` file [observed: knv2.cycletype, destframe; Fallout: New
  Vegas 1.4.0.525 + xNVSE 6.4.9 + kNVSE 37, 2026-11-23, `GSDocMitchellHouse`
  (`0x00103DF9`), 10 official base masters, read-only `PrintToConsole` probes
  `b3_knv_02_anim_cycle_dest_frame` (`tools/capture/nvse/catalogs/wave4b_extenders.json`,
  #3679): `PROBE|b3_knv_02_anim_cycle_dest_frame|cyc=1/0,dst=1/0` and
  `knv_02_set_anim_cycle_dest_missing_file`
  (`tools/capture/nvse/catalogs/wave4a_extenders.json`, #3679):
  `PROBE|knv_02_set_anim_cycle_dest_missing_file|0,0`; no DLL or source consulted].
  NVForever gives 1 when the file exists, else 0.
- **EraseCurrentAnimAction**: 1 on the player (`knv2r2`).
- **ThisAnim, ThisAnimDir**: "" outside an animation's own script (`knv1.thisanim.*`).
- **EjectWeapon**: 0 on the player. **RegisterCustomAnimGroup** `group name` compiled in the
  shape run but not as a statement in a probe; NVForever answers 0.
- **CreateIdleAnimForm** `path`: crashed the original, with a real file and with none
  (`knv2r0`, `knv2r1`). NVForever gives no form [decision].
- **PlayGroupAlt** `group flag`: takes an animation group name as PlayGroup does and gives
  1 (`knv3.playgroupalt.*`). NVForever plays it as PlayGroup [hypothesis: what "Alt"
  changes is not seen].
- **GetDirectoryFiles** `pattern [n]`: the paths a wildcard pattern matches, folders and the
  `.` and `..` entries too, each as the pattern's folder plus the name, in reverse order of
  name; the second argument changed nothing (`knv3.dir.*`). A pattern without a wildcard
  gives the folder itself, one ending in `\` nothing, a missing folder an empty array
  (`knv2.dir.*`). NVForever lists Data through its read roots (NVForever's layer, then the
  game's Data) and nothing outside Data [decision].

## 8. ShowOff, SUP, One111, ITR and JohnnyGuitar: the small families (batch 4)

Probed on a placed Doc Mitchell, the player and vanilla forms [observed: set rest4; the
setters that change the world ran one per launch]. Most of these answered a constant
whatever was asked, and what they change was not seen. For those, NVForever gives the
original's answer and changes nothing.

**Answers with a meaning NVForever follows:**
- **GetWorldOffsetPosArray** (ShowOff): `[x, y, z]`, the reference's position
  (37198.9 -117368.7 6056.0 for Doc outside the airport) [hypothesis: the probed worldspace
  has no offset].
- **TopicGetTopicInfoQuest** `info` (SUP): the dialogue line's quest (0008D243); no form for
  a topic.
- **BaseCopyFaceGenFrom** `to from` (ShowOff): answers 1 [observed: the row below]. NPC
  base `to` takes `from`'s look for the session, on every reference of it: race, sex,
  face geometry, skin tone, hair, eyes and head parts [documented: CopyFaceGenFrom, the
  reference form of the same copy; hypothesis: the base form copies the same things, in
  `to from` order, and height and outfit stay the actor's own]. GetIsRace and GetIsSex
  read the copy (`Game::face_from_base`), and the engine draws it (`nvforever`
  actors.rs). Not saved [documented: CopyFaceGenFrom, other NPCs' changes "will not be
  saved"]. ScriptedNVR2's Script Runner file calls it 1,436 times at the game restart.
- **GetCasinoDeckTexture** `casino index` (JohnnyGuitar): the deck's back texture, the
  casino's ICO2 entries in order (`terminals\NV_PlayingCards\DeckB\cardB_Back.dds` for the
  Atomic Wrangler) [observed: rest4.casinodeck.0; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 with the 10 official base masters, clean-room script probe jg_37_casino_deck_and_switch_node (GetCasinoDeckTexture/GetSwitchNodeIndex): terminals\NV_PlayingCards\DeckB\cardB_Back.dds,0; no DLL or source consulted]. **SetCasinoDeckTexture** was not probed:
  NVForever answers 1, as JohnnyGuitar's other setters did, and changes nothing, so
  GetCasinoDeckTexture keeps reading the casino's own deck [hypothesis].
- **GetSwitchNodeIndex** `node [flag]` (JohnnyGuitar): 0 for Doc's `Bip01 Head`, not a
  switch node [observed: rest4.switchnode; observed: jg_37_casino_deck_and_switch_node: 0]. NVForever answers 0 [hypothesis: the index of
  a switch node's shown child, 0 for any other node].

**Constant answers** (each [observed] for the inputs probed; their meaning is open):

| Answer | Commands |
|---|---|
| -1 | GetActorCoverState (ITR, Doc not in cover), GetWeapAdditionalFloat (One111, before and after the setter), HudBarSetForceHide (SUP) |
| 1 | GetExtraAccuracyPenaltyMult (JG, no key); GetCameraShotPath (JG: 1, 3 after SetCameraShotPath); SetCameraShotPath, SetMusicTypePath, SetMediaLocationControllerOverride, ClearMediaLocationControllerOverride (JG); BaseCopyFaceGenFrom (ShowOff) |
| 0 | SetDismemberedLimbVelocity (ITR); SetWeapAdditionalFloat, SetWeapAdditionalFlag, SetOnDamageCalcFlag (One111); IsTemporaryReference (ShowOff: 0 for the player, a placed Doc and a placed record reference alike); Get/SetLevelUpMenu…, Get/SetNoEquipShowOff (the getter read 0 after the setter), GetLiveEffectBaseTrait, SetLiveEffectMagnitudeModifier, MessageExAltShowoff (ShowOff) |
| empty array | GetEligiblePerksEx (ITR), GetPipboyRadioVoiceEntryData (ShowOff), CompareNifBlocks (SUP) |
| no form | SayTo_GetSpeakingActor, SayTo_GetQuest (ShowOff) outside a SayTo |

**Answers NVForever does not match**, with what the original gave:
- **GetActorCoverInfo** (ITR): a string map of five keys the probe did not read. NVForever
  gives an empty one.
- **GetNifChildrenIDsByName** (SUP): an array of 1 for `Bip01` and 2 for `Bip01 Head`;
  NVForever gives an empty array. **GetNifChildrenIDsByID** gave no array, and
  **GetNifTypeByID** and **GetNifStringByID** failed the call; NVForever gives no array, 0
  and "".
- **ShowingOffDisable / Enable** (ShowOff), **SpawnTempParticle** (ITR) and
  **SUPGetConditionVarFloat** (SUP) failed the call. NVForever answers 0.
- **GetCurrentDialogue** (ITR) did not compile as an array, and its return was not seen.
  NVForever answers 0.
- **SetExtraAccuracyPenaltyMult / RemoveExtraAccuracyPenaltyMult** (JG) and
  GetExtraAccuracyPenaltyMult with a key did not compile in the probe. NVForever answers 1.

**No argument pattern compiled**, so the shape is unknown. These get optional arguments
and answer 0: AddPerkEntry (One111); GetPackedPlayerFOV, JGLegacyWorldToScreen,
SetBlockTransform, SetDialogResponseOverrideValues (JG); SetFactionCombatReactionTemp
(ShowOff); HudBarGetPos, GetCalculatedPosFrame, GetNifBlockTranslationToVars,
TopicReplaceTopicInfo (SUP).

## 9. JIP LN (batch 5)

Probed on a placed Doc Mitchell, the player and vanilla forms [observed: set rest5; the
commands that touch the menus, the sky or the minimap ran one per launch].

**Answers with a meaning NVForever follows:**
- **GetRaceAgeRace** `race which`: 0 the older race (ONAM), 1 the younger (YNAM)
  (Caucasian: CaucasianOld, CaucasianChild; CaucasianOld 0: none). **GetRaceVoice** `race
  which`: 0 the male voice, 1 the female (VTCK). **SetRaceAgeRace / SetRaceVoice** answer 0;
  NVForever's getters read the change back for the session [observed: Fallout: New Vegas
  1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 with the 10 official base masters, clean-room
  script probe jip_52_race_age_voice_setters (GetRaceAgeRace/SetRaceAgeRace/GetRaceVoice/SetRaceVoice):
  0,1,000042C2; no DLL or source consulted].
- **GetEquippedArmorRefs**: the actor's equipped armour, 3 for the player and 1 for Doc.
  The original returns inventory references (`FFxxxxxx`) [observed: Fallout: New Vegas
  1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 with the 10 official base masters, clean-room
  script probe jip_53_equipped_armor_refs_kind (GetEquippedArmorRefs): 2,FF001342; no DLL
  or source consulted]; NVForever currently lists the base forms [design: follow-up].
- **GetArrayValue** `array index`: the element (20 for index 1 of [10 20 30]); 0 for a
  reference given instead of an array.
- **SetContainerOpenSound / SetContainerCloseSound** `sound [container]`: the container's
  base takes the sound; 0 (`opensound.set`, `closesound.set`; `cmd2.opensound.set` from a
  reference). NVForever hands them to SetContainerSound.
- **RefMapArraySetValue** `name value key` (alias RefMapSetVal) answers 0, and the map then
  holds one key. **RefMapArrayGetValue** `name key` gave a one-element list. Its element
  read back as a float with a denormal value (2.1e-310) when 5 was set, so how the value is
  stored is open. NVForever keeps the value as given and lists it.
- **HasActionRef**: takes 0 arguments (1-argument calls fail to compile) and returns `1`
  for references with active action/interaction state rather than unconditionally for every
  actor (`rest5.hasactionref.doc` observed `1` on sitting Doc Mitchell;
  `jip_54_hasactionref_non_actor` (`HasActionRef`, Fallout: New Vegas 1.4.0.525 + xNVSE
  6.4.9 + JIP LN 57.30 with the 10 official base masters, no DLL or source consulted)
  observed `0,1,1` on Player, Doc Mitchell, and a door in `GSDocMitchellHouse`) [observed:
  Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24,
  `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, clean-room read-only
  console probes in `tools/capture/nvse/catalogs/wave4c_extenders.json` (#3679), no DLL
  or source consulted:
  `w4c_a03_has_action_ref` (`P=0(00000000)|Doc=0(00000000)|Door=1(00000000)|Cont=0|Acti=0|Furn=0|Stat=0|Misc=0|Dyn0=0(00000000)|Dyn1=0(00000000)` — note that `Doc=0` in `w4c_a03` queried FormID `0x00104C0D` (`VCG01DocMarkerFirstPositionREF`, an `XMarkerHeading`) rather than `DocMitchellREF`),
  `w4c_a03c_has_action_ref_doc_real` (`pl=0|doc(h=1,g=00000000,lnk=00000000,par=00000000)|door(h=1,g=00000000,lnk=00000000,par=00000000)` — placed `DocMitchellREF` `0x00104C0F` (`ACHR`, sitting in furniture `0x001059B0`) and scripted front door `0x00103E61` both return `1` while `Player` returns `0`),
  `w4c_a03d_has_action_ref_doors_actors` (`spawnNpc=0|doors(tot=2,hasAct=1,hasScr=1)` — freshly spawned `PlaceAtMe` NPC returns `0`, and only the 1 scripted door of 2 doors returns `1`),
  and `w4c_a03b_has_action_ref_1arg` (`COMPILE_FAIL`)]. Wave 4D follow-up
  [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24,
  `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated clean-room
  staging copy (read-only `PrintToConsole` probe; zero writes to game install or Saves),
  `w4d_02_has_action_ref_triggers` (`tools/capture/nvse/catalogs/wave4d_extenders.json`,
  #3802 / #3803):
  `PROBE|w4d_02_has_action_ref_triggers|front(1,1)|doc(1,1)|docMk104C0D(0,0)|unscr(001062AC:0,1,1)|spawn(0,1)|cont(001062C6:0,1,0)`;
  zero DLL, PDB, or third-party source consulted] confirmed same-tick and next-tick values:
  front door `0x00103E61` (`1,1`) and `DocMitchellREF` `0x00104C0F` (`1,1`) stay `1`,
  `VCG01DocMarkerFirstPositionREF` `0x00104C0D` (`0,0`) stays `0` (reconciling `w4c_a03`
  `Doc=0` vs `w4c_a03c` `doc=1`), unscripted door `001062AC` transitions `0 -> 1` upon
  `Activate Player 1` and retains `1` on the next tick (`0,1,1`), a freshly spawned NPC
  transitions `0 -> 1` after 1 AI tick (`0,1`), and container `001062C6` transitions
  `0 -> 1` upon `Activate Player 1` and returns to `0` once `ContainerMenu` closes (`0,1,0`).
  NVForever returns `1` when the calling reference has an active `ctx.action_ref` /
  `Game::action_refs` entry (populated on `Activate` and retained across ticks), active
  `Game::furniture_use`, an active non-player AI `current_package`, or a `DOOR` base with
  an attached script, and `0` otherwise [hypothesis: generalizing the observed spawned-NPC
  and front-door results to "active AI package" (`current_package.is_some()`) and "scripted
  door" (`DOOR` with attached script), plus exact clearing rules on `ContainerMenu` close,
  cell unload, or unprobed reference types].

**Constant answers** (each [observed] for the inputs probed; what each changes is not seen):

| Answer | Commands |
|---|---|
| 1 | GetQuestTargetsChanged, ToggleHitEffects |
| 0 | GetScriptBlockDisabled, SetScriptBlockDisabled, IsAttackQueued, GetRecipeMenuNumSubcategories, SetWaterTrait, ToggleDepthClear, ToggleNoMovementCombat, ToggleNoZPosReset, ToggleNodeCollision, SuppressQuestMessages, UnlockRecipeMenuQuantity, SetItemFilter, InitItemFilter, SetTextureTransformKey, ClearModelAltTextures, InitRockItLauncher, EnableImprovedRecipeMenu, SetWindSpeedMult, SwapObjectLOD, ScrollMouseWheel, InitMiniMap, UpdateMiniMap, ReloadCharController, ReloadCloudTextures, ResetClouds, ShowLevelUpMenuEx |
| no array | GetObjectiveTeleportLinks (quest VMS01, objective 10) |

**Answers NVForever does not match**, with what the original gave:
- **GetCollisionNodes**: an array of 18 for Doc; NVForever gives an empty one.
- **CalculateShotsPerSec** (a weapon form or the player) and **ExecuteScript**
  (no arguments) failed the call; NVForever answers 0.
- **ProjectUITile**: the historical six-number probe failed in the original; the
  former generic NVForever stub answered 0. The typed source-command handler now
  also refuses that wrong argument shape with a script error, rather than returning
  0. The old capture probe and its steps remain historical inputs, unchanged; a
  future lab result may differ. This is not renderer or native-call qualification.
- **GetBodyPartVATSTarget** did not compile into a number; NVForever gives "".

**No argument pattern compiled**, so the shape is unknown. These get optional arguments
and answer 0 or "": CCCTaskPackageFlags, GetCameraMovement, GetTranslatedPos,
GetWorldMapPosMults, TransformWorldToLocal.

## 10. Where this leaves the catalogue

Every name the installed plugins register in nvse.log now has a catalogue row (checked
2026-11-13 against the Blue Moon stock game's nvse.log, aliases counted). The extender
census counts every row built; its two unbuilt names (SetStringSetting, EvaluateString)
are from other work. What stays open, by section: the CCC mod's effects
(`ext-unlisted-commands.md` §5.5); the answers NVForever does not match (§8, §9); the
shapes no pattern compiled (§8, §9); and the effects of the many commands whose answers
are constants.
