# SUP NVSE batch 4: arrays, delayed calls, saves, near cells and the rest (2026-11-09, #1008)

The last 17 SUP functions (the names `handover.md` of 2026-11-08 called SUP batch 4; the
branch `claude/sup-batch4` claimed for them was never pushed, so they are built here on
`claude/sup-batch4b`). Code: `crates/fnv-game/src/nvse_sup4.rs`, the kept calls in
`crates/fnv-game/src/delayed.rs`, the VM's hand-overs in `fnv_script::vm` (Ar_SetFloatsFrom's
output variables, MarkScriptOnLoad's calling script, the plugins' `ar_` functions passed to
the host), and the engine's save and load names in `engine/nvf-play/src/save.rs` and
`script.rs`. Tests: `crates/fnv-game/tests/sup4.rs` and the unit test of `nvse_sup4.rs`.

Source: the GECK wiki export [documented: the page named], in our own words. Not probed
yet: set `sup4` of `tools/capture/jip_probes.py` (lab `L5.ext.sup4`), questions in §3.

## 1. The functions

| Function | NVForever |
|---|---|
| AllocatePlayer | does nothing, 0 [documented: a joke page, "Two. Can play at that game. (no, they can't)"; decision: there is one player] |
| Ar_GetRandomKey `array` | a random key of the array, a number [documented]; 0 for an empty array (not -1) [observed: sup4 randomkey; observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + SUP NVSE 855 with the 10 official base masters, clean-room script probe `sup_08_ar_getrandomkey_empty` (`Ar_GetRandomKey`/`Ar_GetRandomKeyMap`): empty `Ar_GetRandomKey` -> `0`; no DLL or source consulted] |
| Ar_GetRandomKeyMap `array` | a random key as a string, with numeric map keys (positive and negative) and `Ar_List` indices formatted as 6-decimal float strings (`"7.000000"` on `{7: 100}`, `"-3.500000"` on `{-3.5: 99}`, `"2.000000"` on `ar_List 10 20 30`) [documented: the note; observed: 2026-11-23 & 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + SUP NVSE 855 with the 10 official base masters, `sup_26_ar_getrandomkey_num_map` (`7.000000`) and `w4d_36_sup_random_keymap_and_dialog` (`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3679): `rnd(str=[0.000000],neg=[-3.500000],arr=[2.000000],empLen=8)`; no DLL or source consulted]. On stringmaps, `sup_08_ar_getrandomkey_empty` and `w4d_36_sup_random_keymap_and_dialog` observed `"0.000000"` and buffer garbage (`42`/`8` chars) on empty maps (consistent with `sup4 randomkey` in §4); NVForever keeps returning the string key on stringmaps [documented] and `""` on empty arrays [design] |
| Ar_HasInvalidRefs `array [dimension]` | 1 when the array holds an empty reference or one neither the load order nor the world knows [documented: "origin mod is disabled or temporary reference … removed"]; with a dimension the inner array at that key is searched [documented: the second example] |
| Ar_SetFloatsFrom `array f1 … f10` | the array's numbers in key order into the variables, up to 10; the VM writes them (`out_params`) [documented: the example]; non-numeric elements (e.g. strings) and variables beyond the array's length are left unchanged [observed: 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 + JohnnyGuitar 528 + ShowOff 184 + SUP NVSE 855 + Stewie 1005 with the 10 official base masters, `tools/capture/nvse/catalogs/wave6_t1_sup_ar_setfloats.json` (`w6_t1_13_sup_ar_setfloats`): `[3.5, "abc", 8.25]` into `(11, 22, 33, 44)` prints `PROBE\|w6_t1_13_sup_ar_setfloats\|vals=(3.50,22.00,8.25,44.00)`; no DLL or source consulted] |
| CallFunctionNextFrame `udf [frames] [removeOnLoad]` | the UDF runs that many frames later (1 to 1000, 1 by default) on the calling reference (not same frame), returning 0 on schedule [documented: the notes; observed: 2026-11-23 & 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + SUP NVSE 855 with the 10 official base masters, `sup_25_call_function_next_frame_schedule` (`0`) and `w4c_b4_02b_sup_nextframe` (`tools/capture/nvse/catalogs/wave4c_extenders.json`, #3679): `same=0,later=1`; no DLL or source consulted]; frames count in menus too [hypothesis]; with removeOnLoad 0 it outlasts a load (`Game::take_session`), a function script's only [hypothesis: as the extender's other kept callbacks] |
| GetCurrentQuestObjectiveTeleportLinksAlt | the current objective's compass targets, one array: GetCurrentQuestObjectiveTeleportLinks's [documented: "the current compass marker objects"; observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + SUP NVSE 855 with the 10 official base masters, clean-room script probe `sup_22_quest_obj_teleport_links_alt` (`GetCurrentQuestObjectiveTeleportLinksAlt`): `0,0`; no DLL or source consulted] |
| GetLoadedSaveSize, GetSavedSaveSize `type` | type 0 the save file's bytes; -1 before any load or save this session [documented]; type 1, the .nvse co-save, 0 [decision: NVForever writes the extender's state into its one save file] |
| GetSaveName `type` | 0 the last save written, 1 the last loaded, without the extension; "" before any [documented]. The engine tells the game both (the save writer, the load) and they outlast a load |
| GetNearCells `depth` | the loaded exterior cells within `depth` of the player's cell, each axis apart, with a cell south of the origin (y < 0) counted one column further east (x + 1) than it is; -1 every loaded cell; indoors the player's cell alone at every depth [documented: "-1 … uGridsToLoad"; observed 2026-11-11, §5; observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + SUP NVSE 855 with the 10 official base masters, clean-room script probe `sup_21_get_near_cells_indoors` (`GetNearCells`): `1,1,1`; no DLL or source consulted] |
| GetNearMapMarkers `depth` | the map marker references (REFR XMRK) in those cells [documented] |
| MarkScriptOnLoad, IsScriptMarkedOnLoad `script` | the calling script marked until the next load, which starts with none [documented]; MarkScriptOnLoad returns 1 from a UDF/query too [observed: sup4 markonload; observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + SUP NVSE 855 with the 10 official base masters, clean-room script probe `sup_10_mark_script_on_load_udf` (`MarkScriptOnLoad`/`IsScriptMarkedOnLoad`): `1`; no DLL or source consulted] |
| KillAll2 (KillAllAlt) | KillActor on every living actor whose combat target is the player, not the player's teammates [documented: "kills all current player combat enemies leaving companions … intact"; deprecated] |
| SetCaughtPCPickpocketting `[caught]` | the NPC's caught flag that a failed pickpocket sets (`crate::pickpocket`), set when given; the flag as it is now [documented] |
| TranslitText `text` | the text with Cyrillic letters in Latin ones, returned [observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + SUP NVSE 855 with the 10 official base masters, clean-room script probe `sup_11_translit_text_return` (`TranslitText`): `5,Hello`; no DLL or source consulted]; [decision: NVForever's own table, a common Russian romanisation with the Ukrainian and Belarusian letters; an upper-case letter starts its Latin letters in upper case] |

## 2. Left as nothing values

AllocatePlayer (a joke page) and the co-save size (no co-save). No engine follow-ups.

## 3. Probe questions (set sup4, not run)

At Doc Mitchell's house; Doc Mitchell's flag set back; the aux variable erased.

1. `randomkey`: a key in 0-4, a map key of the two, an empty list's (hypothesis `1 1 -1`).
2. `invalidrefs`: valid, an empty reference, an inner array without and with its
   dimension (hypothesis `0 1 0 1`).
3. `setfloats`: the page's example (hypothesis `3 8 5`).
4. `nextframe.set`, `.wait`, `.read`: a UDF two frames later (hypothesis `0` then `1`).
5. `saves`: the loaded save's size, its co-save's, its name, the last save's name before
   any save (hypothesis `1 1 1 0`; NVForever gives 0 for the co-save by decision; the lab's
   candidate loads no save, so it gives -1 sizes there).
6. `near`: cells at depth 0, 2 and -1 indoors, map markers (hypothesis `1 1 1 0`).
7. `markonload`: MarkScriptOnLoad from a query, and Doc Mitchell's script marked
   (hypothesis `0 0`, or the call refused outside a script).
8. `caught`: Doc Mitchell's flag read, set, read (hypothesis `0 1 1`).
9. `translit`: "Привет" (hypothesis `Privet`; the runtime compiler's code page may change
   the letters).

## 4. Probed in the original (2026-11-09, head af8a876c0)

The first run compiled nothing: the runtime compiler refuses the declaration
`string_var sA` [observed: the observer's diagnostic runs, other names compile], so the
probes use `sOut`. The second run answered 10 rows; 3 matched (invalidrefs,
nextframe.set, nextframe.wait). What the others say [observed]:

| row | original | NVForever now |
|---|---|---|
| randomkey | `1 0 0` | an empty array's key is 0 (was -1); the map key the original gave was neither "LALALA" nor "TROLOLO" by `==` [open: what it gives] |
| nextframe.read | `0` | the lambda's aux variable was not set two frames later [open: whether the original runs a lambda, or counts frames from the next one; NVForever runs it at the second frame] |
| saves | `1 1 1 0` | the probe game had loaded a save; the lab's candidate (fnvtool) loads none, so it cannot match; the co-save's size (1 there) stays 0 by decision |
| near | `1 20 25 1` | GetNearCells 2 gave 20 cells where -1 gave 25: settled in §5 |
| markonload | `1 0` | MarkScriptOnLoad gives 1 from a query too (was 0) |
| caught | `0 0 0` | SetCaughtPCPickpocketting 1 on Doc Mitchell did not read back [open: NVForever sets and reads the flag a failed pickpocket sets] |
| translit | not the question | the probe's UTF-8 Cyrillic reached the game as cp1252 bytes; the literal is now "Привет"'s cp1251 bytes written as cp1252 characters. The lab's candidate reads that literal as Latin letters, so it keeps them: the game's own text is decoded by code page (`fnv_esm::text_guess`), Cyrillic in a Windows-1251 plugin is Cyrillic to TranslitText |
| setfloats | did not compile | `Ar_SetFloatsFrom aR fA fB fC` as a statement is refused by the runtime compiler, as SUP's other functions that write variables (ext-sup6.md §1); scripts compiled ahead keep the page's form |
| setfloats (2026-11-24 follow-up) | `(3.50, 22.00, 8.25, 44.00)` | with parenthesised `(f1) (f2) (f3) (f4)` (`wave6_t1_sup_ar_setfloats.json`, `w6_t1_13_sup_ar_setfloats`) it compiles and runs, writing numeric elements and preserving non-numeric and trailing output variables (`PROBE\|w6_t1_13_sup_ar_setfloats\|vals=(3.50,22.00,8.25,44.00)`) |

The open rows wait for another run (the coordinator decides).

## 5. GetNearCells probed (2026-11-11, #2039)

Probe job `rules2039b` [observed; results private] ran from the `nvfairport` save. At four
spots it listed every cell GetNearCells returns at depths 0, 1, 2, 3, 4 and -1, each as
GetCellCoords' x,y:
- the save's spot, cell 9,-29;
- after `cow WastelandNV 0 0`, cell 0,0;
- after `cow WastelandNV -10 5`, which put the player in cell -10,4;
- after `coc GSDocMitchellHouse`.

The rule (§1's table) fits every row:
- **Only loaded cells.** Depth 3, 4 and -1 return the same set: the loaded grid, 5 × 5
  around 9,-29 and 0,0. At -10,4 the grid was 5 × 6 (x -12..-8, y 2..7) just after the
  move, and depths 3, 4 and -1 gave those 30. A depth past the loaded grid adds nothing.
- **A cell with y < 0 is counted at x + 1.** The player's own cell is not shifted. A cell
  is returned when |x' − px| ≤ depth and |y − py| ≤ depth, where x' is x + 1 for y < 0 and
  x otherwise.
  - At 9,-29 every cell has y < 0. Depth 1 gives x 7..9 and depth 2 gives x 7..10 (20
    cells, column 11 left out), where a centred square would give 8..10 and 7..11. That
    is the sup4 row.
  - At 0,0, depth 1 gives row -1 as x -2..0 and rows 0 and 1 as -1..1. Depth 2 gives
    23 cells: rows -2 and -1 lose x 2.
  - At -10,4 no loaded cell has y < 0, and every depth is a centred square cut to the
    loaded grid.
- **Indoors** the player's cell alone, at every depth.
- [hypothesis] The cause is the cell's coordinates packed into one number (x in the high
  half, y in the low half, y's sign borrowing from x) and the x read back from it. Rows
  north of the origin and the player's own cell are not affected.

Built (`nvse_sup4.rs` `near_cells`, #2039): the loaded cells are `Game::loaded_cells`; the
sup4 `near` row is equal since.
