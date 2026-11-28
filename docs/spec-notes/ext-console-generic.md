# xNVSE console functions, base animations and generic form lists (2026-10-30)

Three small xNVSE families: the `Con_…` functions (a console command run from a script),
the base actor's animation list (AddAnimation …) and the generic embedded-list functions
(Generic…Form). Built in `crates/fnv-game/src/nvse_console.rs` and `nvse_lists.rs`
(commit dc19c50e); this note is the plain-language account they follow (issues #913,
#914). Sources: the GECK wiki export, `functions.xml` [documented: the page named], and
the in-game probe set nvse-eighth [observed] (`docs/handoff/probe-sets.md`).

## 1. Console functions (`Con_…`, 21)

Each runs the console command of the same name, with the script's arguments as the
command's words [documented: the "Con …" pages]. A command's printed lines reach the
console only while the console echo is on (SetConsoleEcho) [documented: Con Inv, Con
ShowVars, Con SQV, "requires console echo"]. A command that refuses is reported as a
script error in the console and the script goes on [hypothesis].

| Function | Console command | NVForever |
|---|---|---|
| Con_Inv | ShowInventory | the calling reference's items |
| Con_ShowVars | ShowVars | the reference's script variables |
| Con_SQV | ShowQuestVars | a quest's variables |
| Con_SetGameSetting | SetGameSetting | changes the setting for the session |
| con_CloseAllMenus, con_QuitGame | CloseAllMenus, QuitGame | as the console |
| con_Save, con_LoadGame | SaveGame, LoadGame | NVForever's own saves; Save's optional dump file is not written [decision: NVForever writes nothing beside the game] |
| con_SetCameraFOV, Con_SetUFOCamSpeedMult, Con_TCL, Con_TFC, Con_ToggleMenus | SetCameraFOV, SetUFOCamSpeedMult, ToggleCollision, ToggleFlyCam, ToggleMenus | the engine's console commands |
| con_GetINISetting, con_SetINISetting | GetINISetting, SetINISetting | the extender's INI store (`nvse_settings`); Get prints `name = value`, or "No INI setting name" [design: the console's line is not documented]; Set takes a number as a number, anything else as text |
| con_RefreshINI | RefreshINI | the store reads the files again, so the session's changes go [documented: Con RefreshINI, "Refresh INI settings from files"] |
| con_SaveINI | SaveINI | does nothing [decision: NVForever never writes the game's INI files] |
| Con_SCOF | SCOF (console output to a file) | keeps the file name only, for GetConsoleOutputFilename; nothing is written [decision, as above] |
| Con_TFIK | ToggleFootIK | switches the feet's placing on the ground, the settings page's switch (`--no-foot-ik`) |
| Con_TDT, con_SetVel | ToggleDebugText, SetVel | do nothing [hypothesis: NVForever has no debug text; SetVel's physics hook is not built] |

Every one returns 0. What a script's command asks of the engine (quit, save, load, close
the menus) is taken the same frame, as a typed console line's is.

## 2. The base actor's animation list (4)

`AddAnimation name baseActor`, `DelAnimation name baseActor`, `DelAnimations baseActor`,
`GetNthAnimation index baseActor` work on the list of animation (KF) file names of an
NPC or creature base [documented: their pages]. GetNthAnimation gives the name at the
index, an empty string past the end. AddAnimation does not add a name already there and
DelAnimation removes by name, both ignoring case [hypothesis]. The list starts as the
record's (KFFZ) and the changes last for the session [hypothesis]. A reference passed
for the base stands for its base form.

## 3. Generic embedded lists (4)

`GenericGetForm code target index`, `GenericAddForm code target form index`,
`GenericDeleteForm code target index`, `GenericReplaceForm code target new index`
read and change a list of forms held inside another form, chosen by a list code
[documented: GenericAddForm]:

| Code | List |
|---|---|
| 0 | a race's hairs |
| 1 | a race's eyes |
| 5 | an actor base's AI packages |
| 6 | an actor base's spells |
| 7 | an actor base's leveled spells |
| 10 | an NPC's head parts |
| 13 | a form list's forms |

For codes other than 0, 1 and 13 a reference stands for its base [observed: nvse-eighth,
generic.5.ref.0 and generic.5.base.0 both give Doc Mitchell's first package].

- Get gives the form at the index, 0 past the end [documented].
- Add puts the form in at the index; -2 adds at the end [documented]. The page gives no
  result, but the game gives the index it went in at: for `-2` on an empty list (`n0 == 0`)
  it appends (`n: 0 -> 1`) and returns `0`, for `-2` on a non-empty list it appends and
  returns the index of the last entry before it (`n0 - 1`), and on negative indices other
  than `-2` (`-1`, `-3`, `-10`) or past-end indices (`999`, `n + 5`) it returns `-1`
  without modifying the list [observed: nvse-eighth; 2026-11-23 & 2026-11-24,
  `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 with
  the 10 official base masters, isolated clean-room staging copy (read-only `PrintToConsole`
  probe; zero writes to game install or Saves), `core_13_generic_add_form_split` (`0 -> 0`,
  `-2 -> n0 - 1`, `999 -> -1`) and `w4d_04_generic_form_neg_idx`
  (`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3802 / PR #3803):
  `PROBE|w4d_04_generic_form_neg_idx|emp(n0=0,d=0/0/0/0:0,r=0:0/0:1/0/0:1,a=-1/0:1/-1/-1/-1:1)|ne(n0=38,a=-1/37:39/-1/-1/-1:39,r=0/1/0/0,d=1:38/1:37/1:36/0:36)|flstC1(g=0,a=-1,r=0,d=0,n=36)`;
  zero DLL, PDB, or third-party source consulted]. At the exact boundary `index == len`:
  on an empty list (`len == 0`) index `0` inserts and returns `0` (`len: 0 -> 1`), whereas
  on a non-empty list (`len > 0`) `index == len` returns `-1` without appending (valid
  non-`-2` indices are `0..len` when `len > 0`, and `0` when `len == 0`) [observed:
  2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE
  6.4.9 + JIP LN 57.30 with the 10 official base masters, isolated clean-room staging copy
  (read-only `PrintToConsole` probe; zero writes to game install or Saves), committed
  probe `w6t2p3_25_generic_form_real_flst`
  (`tools/capture/nvse/catalogs/wave6_t2_generic_form.json`):
  `PROBE|w6t2p3_25_generic_form_real_flst|add(0,-1,-1,n=1,past=-1)|delM1(h=1,t=0)|delM2(b=0,c=0)|nEnd=0`;
  zero DLL, PDB, or third-party source consulted].
- Target record types are validated against the list code (`0`/`1` `RACE`, `5`–`7`
  `NPC_`/`CREA`, `10` `NPC_`, `13` `FLST`): on a mismatched target record type or unknown
  list code, `GenericAddForm` returns `-1` even at valid index `0`/`-2` without mutating,
  and `GenericGetForm`/`GenericDeleteForm`/`GenericReplaceForm` all return `00000000`
  [observed: 2026-11-23 & 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`),
  `core_13_generic_add_form_split` (`NPC_` + code 1, `RACE` + code 5, `FLST` + code 1)
  and `w4d_04_generic_form_neg_idx` (#3802 / PR #3803: `flstC1(g=0,a=-1,r=0,d=0,n=36)`,
  confirming FLST + code 1 rejection across Get, Add, Replace, and Delete); `CREA` and
  codes 6/7/10 follow [documented: GenericGetForm's list codes] / [hypothesis]].
- Delete takes out the form at the index and gives it [documented]; past the end (`>= len`)
  or on an empty list returns `0`, while negative indices (`-1`, `-2`, `-3`) on a
  non-empty list each remove an entry (returning a non-zero form and decrementing the
  count `39 -> 38 -> 37 -> 36`) [observed: `w4d_04_generic_form_neg_idx` (#3802 / PR
  #3803): `emp(...,d=0/0/0/0:0,...)` and `ne(...,d=1:38/1:37/1:36/0:36)`]. Removing
  index `0` (head of the list) for negative indices is [hypothesis] (`w4d_04` checks
  non-zero return and decremented counts, not which position was removed).
- Replace puts the new form at the index and gives the old one [documented]
  [observed: generic.10.replace and its restore]; `-2` replaces the last entry on a
  non-empty list (returning the previous last form) and on an empty list returns `0` while
  appending the replacement form (`n: 0 -> 1`), whereas `-1`, `-3`, and past-end indices
  return `0` without mutating [observed: `w4d_04_generic_form_neg_idx` (#3802 / PR #3803):
  `emp(...,r=0:0/0:1/0/0:1,...)` and `ne(...,r=0/1/0/0,...)`].
- The lists' own getters read the changed list: GetRaceHairs, GetRaceEyes (0, 1),
  GetBaseSpellListSpells (6), GetBaseSpellListLevSpells (7), GetHeadParts (10), and the
  AI's package list and ListGetNthForm for 5 and 13 [design].

Changes last for the session and are not saved [hypothesis, as ListAddForm's are
documented to be]. A change to an actor's 3D (head parts) shows once its 3D reloads
[documented: GenericAddForm, "until it reloads"].
