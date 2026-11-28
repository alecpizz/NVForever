# Spec note: JIP math, utility, mod and file functions

JIP LN NVSE functions with no engine system behind them: trigonometry and min/max, actor
value names, mod and plugin queries, version and session getters, and the file and INI
readers and writers. Task J2 of [jip-tasks.md](../handoff/jip-tasks.md); input for J5.

Everything the wiki pages state is tagged `[documented: <page>]` (the private export,
`C:\fnv-private\geckwiki\`, in our own words); results from the original game (JIP 57.30,
xNVSE 6.4.9) are `[observed: jip-getters/<id>]`: runtime-compiled probes
(tools/capture/jip_probes.py, 2026-10-26; golden private). The origin (JIP or LU,
Lutana, now part of JIP) is the catalogue's.

## 1. Math

| Function | Origin | Arguments | Returns | Behaviour | Observed |
|---|---|---|---|---|---|
| fsin, fcos, ftan | LU / LU / JIP | angle (float, degrees) | float | sine, cosine, tangent of an angle **in degrees**, accurate to at least 6 decimals [documented: Fsin; Fcos; Ftan] | fsin 90 = 1, fsin 30 = 0.5, fcos 60 = 0.5, fcos 0 = 1, ftan 45 = 1 [observed: fsin.90, fsin.30, fcos.60, fcos.0, ftan.45] |
| fasin, facos, fatan | JIP | value (float) | float (degrees) | principal arc sine, arc cosine, arc tangent, in degrees [documented: Fasin; Facos; Fatan] | fasin 1 = 90, facos 0 = 90, **fatan 1 = 44.9998**; **fasin 2 = 90** [observed: fasin.1, facos.0, fatan.1, fasin.out]; out-of-range inputs clamped to ±1: fasin(-2) = -90, facos(2) = 0, facos(-2) = 180 [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 with the 10 official base masters, clean-room script probe jip_11_fasin_facos_out_of_range (fasin/facos): -90.0000,0.0000,180.0000; no DLL or source consulted] |
| fatan2 | JIP | y, x (floats) | float (degrees) | arc tangent of y/x using both signs for the quadrant [documented: Fatan2] | **(1,1) = 44.9998**, (1,-1) = 135, (-1,-1) = -135 [observed: fatan2.q1–q3] |
| dATan2 | NVSE | y, x (floats) | float (degrees) | exact arc tangent of y/x in degrees in `[-180, 180]`, with `(0, 0) -> 0` [documented: `https://obse.silverlock.org/obse_command_doc.html` (`(atan2:float) datan2 arg1:float arg2:float`, "ATan2 and DATan2 use degrees"), cross-referenced on `https://geckwiki.com/index.php?title=Fatan2` ("significantly faster than dATan2")] | (0,0) = 0, (1,0) = 90, (0,1) = 0, (-1,0) = -90, (0,-1) = 180, (1,1) = 45, (-1,1) = -45, (-1,-1) = -135, (1,-1) = 135, (1,1.7320508) = 30.000002 [observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9, 10 official base masters, `#3678` / PR `#3679` probe `w1_3495_compound_assign_and_datan2` (`dATan2 y x` across all 10 axis/quadrant points plus `ATan2 1 1 = 0.785398` rad); no DLL or source consulted] |
| fsqrt | LU | value (float) | float | square root [documented: Fsqrt] | fsqrt 16 = 4; **fsqrt -1 = 0** [observed: fsqrt.16, fsqrt.neg] |
| GetMaxOf, GetMinOf | JIP | 2 to 5 floats | float | the largest / smallest of the values [documented: GetMaxOf; GetMinOf] | (1,5,3) → 5 / 1; max (-1,-5) → -1; min (9,8,7,6,5) → 5 [observed: maxof.*, minof.*] |
| GetRandomInRange | JIP | min, max (ints) | int | a random integer with min ≤ x < max [documented: GetRandomInRange] | (5, 6) = 5 [observed: random.single] |

The "f" functions are documented as fast versions of the base game's Sin/Cos/…, accurate
to at least 6 decimals [documented: Fsin and the others]. The arc tangents are not: the
game gives 44.9998 for fatan 1 and fatan2 1 1, where the exact answer is 45 [observed:
fatan.1, fatan2.q1], so JIP uses an approximation there whose formula we do not know.
An exact atan misses those two goldens by 0.0002. Open question [owner review]: accept a
tolerance on the arc tangents, or probe more points (a sweep of fatan over -10…10) and fit
the approximation from the observed values alone.

## 2. Names, mods and plugins

| Function | Origin | Arguments | Returns | Behaviour | Observed |
|---|---|---|---|---|---|
| StringToActorValue | JIP | name (string) | int | the actor value code of a name; -1 when no actor value has that name [documented: StringToActorValue] | "Strength" = 5, "DamageThreshold" = 76, **"strength" = 5 (case ignored)**, unknown = -1 [observed: av.*] |
| GetModName | LU | index (int); keepExt (int, optional) | string | the file name of the mod at that index, without the extension unless keepExt is 1 [documented: GetModName] | 0 → "FalloutNV", (0, 1) → "FalloutNV.esm", **99 → ""** [observed: modname.*] |
| GetSelfModIndex | LU | — | int | the mod index of the calling script [documented: GetSelfModIndex] | — |
| GetPluginHeaderVersion | JIP | plugin file name (string) | float | the plugin's header version [documented: GetPluginHeaderVersion] | FalloutNV.esm = 1.34 [observed: headerversion.fnv] |
| ValidateModIndex | JIP | stored mod index (int) | int | the current index of the mod that had that index when the save was made; -1 if it cannot be resolved [documented: ValidateModIndex] | 0 → 0, 200 → -1 [observed: validatemod.*] |
| GetFormFromMod (GFFM) | LU | plugin name, hex form ID without the mod index (strings) | form | the form; leading zeros may be left out; unlike BuildRef it does not resolve unloaded non-persistent references [documented: GetFormFromMod] | "14" and "000014" in FalloutNV.esm both give the player [observed: formfrommod.*] |
| StringToRef | JIP | text from RefToString or GetFormIDString (string) | ref | rebuilds the reference, resolving the mod index from the text so load-order changes do not break it [documented: StringToRef] | "00000014" → the player [observed: stringtoref.player] |
| GetLoadOrderChanged | JIP | — | 0/1 | 1 if the load order changed since the loaded save was made [documented: GetLoadOrderChanged] | — |

## 3. Versions and session

| Function | Origin | Returns | Behaviour | Observed |
|---|---|---|---|---|
| GetNVSEVersionFull | JIP | float | the extender's full version; registered on an unused base-game opcode, so 0 without the extender [documented: GetNVSEVersionFull] | **6.49** on xNVSE 6.4.9 [observed: nvseversionfull] |
| GetLNVersion (GetJIPLNVersion) | LU | int | documented as always 0, deprecated for GetPluginVersion [documented: GetLNVersion] | **57.3**, not 0 [observed: lnversion]; NVForever follows the game |
| GetGameDifficulty | JIP | int | 0 Very Easy … 4 Very Hard [documented: GetGameDifficulty] | 2 on the owner's save [observed: difficulty] |
| GetDebugModeState | JIP | int | debug mode toggle state: 0 god mode (1 while on), 1 AI (1 while enabled, 0 after `tai`), 2 combat AI (1 while enabled, matching mode 1), 3 collision (0 by default), 4 TB (0 by default), other codes 0 [documented: GetDebugModeState; hypothesis: toggling `tcai`/`tcl`/`tb` is not yet tracked in `Toggles`] | 0 (god mode off) [observed: debugmode.tgm]; modes 0..5 default `(0, 1, 1, 0, 0, 0)` and mode 0 becomes `1` under `ToggleGodMode` [observed: 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 with the 10 official base masters, isolated clean-room staging copy (read-only `PrintToConsole` probe; zero writes to game install or Saves), committed probe `w6t2_24_jip_fall_time_and_is_in_air_edges` (`tools/capture/nvse/catalogs/wave6_t2_debug_mode_state.json`): `PROBE\|w6t2_24_jip_fall_time_and_is_in_air_edges\|dbg(0,1,1,0,0,0,tgm=1)\|fall=0.952,air=0`; zero DLL, PDB, or third-party source consulted] |
| GetLocalGravity | JIP | float | gravity on an axis for the current cell, in game units per second squared; X and Y are 0 unless SetLocalGravityVector was used [documented: GetLocalGravity] | X = 0, **Z = -98.1** [observed: gravity.*] |
| GetGameDaysPassed | JIP | float | precise game days since a date (year, month 1–12, day); with no arguments since 13 October 2281 [documented: GetGameDaysPassed] | no arguments = (2281, 10, 13) [observed: dayspassed.same] |
| GetSessionTime | JIP | float | seconds since the game session started [documented: GetSessionTime] | — |
| GetTimeStamp | LU | string | the user's local time as "HH-mm-SS" [documented: GetTimeStamp] | — |

## 4. Files and INI

| Function | Origin | Arguments | Behaviour |
|---|---|---|---|
| FileExists | JIP | path relative to `Data` (string); isFolder (0/1, optional) | 1 for a loose file, **2 for a file packed in a BSA**, 0 when missing; with isFolder a folder is checked; folders inside BSAs always give 0 [documented: FileExists]. Observed: a BSA file = 2, a loose folder = 1, a missing file = 0 [observed: fileexists.*] |
| GetFilesInFolder (GetFiles), GetFoldersInFolder (GetFolders) | LU | folder relative to `Data`; filter with wildcards (optional) | a list of the names in that folder (files with their extension); `"%e"` lists `Data` itself [documented: both pages] |
| GetINIFloat, GetINIString | LU | "Section:key"; file relative to `Data\Config` (optional; default: the calling mod's name) | the key's value [documented: GetINIFloat; GetINIString] |
| GetINISection, GetINISectionNames | LU | section; file (optional); getNumeric (optional) | a string map of a section's keys (values as floats with getNumeric), or a list of the section names [documented: both pages] |
| ReadStringFromFile | JIP | path **relative to the game folder**; startAtLine, lineCount (optional) | the file's text, at most 16,384 characters [documented: ReadStringFromFile] |
| ReadArrayFromFile | JIP | path; transpose (optional) | tab-separated values into a list, or a matrix (columns first unless transposed); `$` marks strings, `@plugin:formid` marks forms [documented: ReadArrayFromFile] |
| WriteStringToFile, WriteArrayToFile | JIP | path relative to the game folder; append (0/1); a format string with up to 20 values / an array (transpose optional) | create or append to / overwrite a file; returns 1 on success [documented: both pages] |

**Where NVForever reads and writes.** The originals read from and write into the game's
own folders (`Data`, `Data\Config`, the game folder). NVForever never writes into the
game folder (install-layout.md). Proposed for J9 [decision for the owner's review]:
readers look in the game folder (read-only, as the originals) and in NVForever's own
mod-data folder under the engine home; writers write only to that engine-home folder,
mirroring the relative path, and readers prefer the engine-home copy so a mod reads back
what it wrote.

## 5. Not probed (they change state)

ClearJIPSavedData, SetOptionalPatch (GetOptionalPatch reads JIP's optional game patches,
which are also set in `jip_nvse.ini` [documented: SetOptionalPatch; GetOptionalPatch]),
SetGameDifficulty, ToggleImmortalMode, SetLocalGravityVector and the other setters in
these families are documented here only when a later task needs them.
