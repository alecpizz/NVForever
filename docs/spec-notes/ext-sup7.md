# SUP NVSE batch 7: NIF block interpolations, vertices, HUD bar sources (2026-11-08, #1008)

12 SUP functions: the NIF block interpolations (8), the vertices of a NIF block (2) and
the HUD bars' script-variable and UDF values (2). Code: `crates/fnv-game/src/nvse_sup7.rs`
(the functions and `Game::sup7`), the scene mirror's helpers in
`crates/fnv-game/src/nif_blocks.rs` (`push_edit`, `node_now`, `vertices`), the bars'
`bar_id` and `set_value` in `crates/fnv-game/src/ui_hudbar.rs`, the VM's hand-over of a
variable by place (`fnv_script::vm::Host::var_place`) and the engine's side of the
vertices in `engine/nvforever/src/nif_mirror.rs`. Tests: `crates/fnv-game/tests/sup7.rs`
(the engine's part played by the test) and the unit test of `nvse_sup7.rs`.

Source: the GECK wiki export [documented: the page named], in our own words. The modes'
category page ("NifBlock Interpolation Functions (SUP)") is not in the export; the modes
are read from the functions' examples. Probed once in the original: set `sup7` of
`tools/capture/jip_probes.py` (lab `L5.ext.sup7`), the answers in §5 and as [observed]
below.

## 1. The interpolations

| Function | NVForever |
|---|---|
| NifBlockInterpolateTranslation `block x0 y0 z0 x1 y1 z1 timer mode world pc calcStart` | the block's translation from start to end in `timer` seconds by mode [documented]; returns `1` on a valid node (`"Bip01 Head"`) and `0` on a missing node (`"NoSuchNodeXYZ"`) [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + SUP NVSE 855, 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, read-only `PrintToConsole` probe `b3_sup7_02_nif_interp_ret` (`tools/capture/nvse/catalogs/wave4b_extenders.json`, #3679, calling `rDoc.NifBlockInterpolateTranslation` on missing and valid nodes): `PROBE\|b3_sup7_02_nif_interp_ret\|miss=0,ok=1`; no DLL or source consulted]; `0` without a calling reference remains [hypothesis] |
| NifBlockInterpolateRotation `block x0 y0 z0 x1 y1 z1 timer mode pc calcStart` | the rotation (degrees, as GetNifBlockRotation) the same way [documented] |
| NifBlockInterpolateScale `block s0 s1 timer mode pc calcStart` | the scale the same way [documented] |
| NifBlockInterpolateStop `[block] [type] [pc]` | stops the matching ones; 0 without arguments (nothing stopped) OR when the reference has zero active interpolations, 1 when one matched, and -1 when the reference has active interpolations but none match [observed: sup7; `sup_23_nif_interp_stop_and_type` (`0,1,1,-1`); Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + SUP NVSE 855, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `w4d_36_sup_random_keymap_and_dialog` (`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3679): `nifStop(0arg=0,miss=0)`; no DLL or source consulted]; the block keeps the value it had [hypothesis] |
| IsNifBlockInterpolated `[block] [type] [pc]` | 1 when one matches [documented]; without a block 0 [observed: with a scale running, against the page's example] |
| NifBlockInterpolateSetTrait `trait value [block] [type] [pc]` | trait 0: the start value set again when it ends [documented]; 0 [observed: on a running one and for trait 5] |
| SetSUPOnInterpolateFinishEvent `udf [loops]` | the handler, called `{ref, block, type, fromLoop, pcNode}` when one ends (pcNode 1 on the player's third-person model [observed]; read as "the reference is the player" [hypothesis: the name]), and with `loops` at each loop's end too [documented: the handler skeleton]; the same handler again changes its loops flag [hypothesis] |
| RemoveSUPOnInterpolateFinishEvent `udf` | takes the handler away [documented] |

- **Modes** [hypothesis: the category page is missing]: 0 start to end once (the first
  scale example, "from start to end in 2 seconds"); 1 start to end and back to start,
  once ("from start to end and to start again"); 2 there and back again and again ("Infinite
  amount of times … and then back"); 3 start to end again and again, the remaining case.
  Another number is clamped to 0-3.
- **Types**: 0 the scale [observed: sup7 scale.start, event.read], 1 the rotation
  [documented: NifBlockInterpolateStop's example; observed], 2 the translation
  [hypothesis: the one left; a translation is not type 0, observed]; -1 any [documented: "if type -1 specified - it
  will be ignored"].
- **Searches**: no block finds none: Stop gives 0 and stops nothing, IsNifBlockInterpolated
  gives 0 [observed: sup7 second run, the block still interpolated after a bare Stop;
  against the pages' "Will stop all interpolations on Player"]; a block without pc is the third-person model's [documented:
  "third person by default"].
- **One of each type a block** [hypothesis: the pages allow "many types of interpolations at
  the same time"]: a new one of the same type on the same block and model replaces the old.
- **calcStart** takes the block's value from the scene mirror as the start [documented:
  "Scale N will be calculated automatically"]; without the mirror's tree yet (the first
  frame a reference is asked about) the given start is used [hypothesis].
- **The world flag** of the translation is not used [decision: the mirror holds and sets
  values in the parent's space, as SetNifBlockTranslation; engine follow-up: a world
  translation turned into the parent's space].
- **The first-person model** (pc, player only): NVForever's mirror has one tree a
  reference, the drawn one, so a first-person interpolation changes that tree [decision:
  kept apart for the searches and the events; engine follow-up: the first-person
  viewmodel's nodes in the mirror].
- **Running**: each frame (`Game::tick`, before the frame's time step) the value at the
  game time since the start goes to the engine as the NIF block setters' `SceneEdit`, and
  shows in the mirror at once; the engine holds it over the animation as it holds
  SetNifBlockScale's (`nif_mirror.rs`). Menu mode and FreezeTime hold the game time, so
  they hold the interpolation [hypothesis]. The finish event runs after the frame's values
  are sent, so a handler may start a new one on the block [documented: "It is possible to
  set a new interpolation for this block in the event iself"].
- **Not saved** [hypothesis: SUP's pages say a hot reload loses the bars' variables; the
  interpolations likewise run in the session only].

## 2. The vertices

| Function | NVForever |
|---|---|
| GetNifBlockNumVerticles `block [pc]` | the number of the shape's vertices [documented] |
| NifBlockGetVerticles `block [pc]` | three arrays, X, Y and Z of each vertex [documented: `myarray[1][0]` the first vertex's Y] |

The engine tells the vertices of the shapes a script asked for, each frame from then on,
in the shape's own space as its mesh holds them (`SceneMirror::set_vertices`). Asked the
first time they come with the next frame, as the JIP block functions' trees do; until then,
GetNifBlockNumVerticles gives -2 for a block of the tree with none told and -1 for a
missing block, NifBlockGetVerticles an empty array [observed: "Bip01 Head" -2, a made-up
name -1, both empty; observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), Fallout:
New Vegas 1.4.0.525 + xNVSE 6.4.9 + SUP NVSE 855 with the 10 official base masters,
clean-room script probe `sup_35_nif_block_num_verticles` (`GetNifBlockNumVerticles`):
`-2,-1`; no DLL or source consulted]. A mesh kept only on
the GPU tells none, and a skinned shape's are its bind pose [hypothesis; engine follow-up
if a mod needs a posed one]. pc is not read (the drawn model only).

## 3. The HUD bars' sources

| Function | NVForever |
|---|---|
| HudBarSetValueScriptVar `bar variable calcMax` | the bar reads the variable [documented] |
| HudBarSetValueUDF `bar udf [calcMax] [caller]` | the bar reads the UDF's result, called on the caller when given [documented] |

- The variable goes to the game by its place, not its value: the VM hands a global,
  another script's variable or a local of a script instance over as a number
  (`Host::var_place`, `-1 - n`), which the game reads again later. A value that is no
  such variable (a number, a result script's local) is refused [hypothesis].
- Read each fUpdateTimer seconds (bar trait 2, 0.1 s by default) [documented: "Value will be
  updated each instance of Bar update"], the first time in the frame the source was set
  [hypothesis]. A value that is no number is not written [design]. A bar hidden by
  HudBarShowBar is not read [documented: HudBarShowBar, "Bar value calculations will stop
  processing"]; the rest (catch-up, failing sources, conditions): hud-bars.md.
- calcMax 1: the value against the bar's max as HudBarSetValueFloat [documented]; 0: "raw
  value will be passed", read as the meter's share 0.00-1.00 as HudBarSetValuePercentage,
  the value text showing the raw value [hypothesis].
- 1 when set [observed: the UDF source on a made bar]; -1 for a missing bar or a refused source, as the other value setters
  [observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + SUP NVSE 855 with the 10 official base masters, clean-room script probe `sup_24_hud_bar_missing_source_ret` (`HudBarSetValueUDF`): `-1`; no DLL or source consulted]. A new source replaces the bar's old one; a destroyed bar
  takes its source with it [hypothesis].

## 4. Left as nothing values

None. The engine follow-ups: the translation's world flag and the first-person model (§1),
posed vertices of skinned shapes (§2).

## 5. Probed in the original (2026-11-08, first run)

The observer ran set `sup7` once (xNVSE 6.4.9, supNVSE, branch head 845bff7cd; golden kept
private; write-up on PR #1911). 2 of 8 rows matched; the code above follows the answers:

| row | original | what it says |
|---|---|---|
| scale.start | `1 1 0 0 0 0` | a scale is not type 2; IsNifBlockInterpolated without arguments 0 |
| scale.next | `0 -1 -1 1 0` | Stop that finds none gives -1; the scale did not read above 1 a frame on [open] |
| trait | `1 0 0 0` | SetTrait gives 0 on a running one; the Stop found the scale of scale.start, still running |
| event.wait, event.read | `0`, `1100 1 0` | the scale is type 0; iPCNode 1 on the player's third-person model |
| verts | `-2 -1 0 0` | a node -2, a missing block -1, empty arrays |

Diagnostic forms: after a translation, type 0 does not find it; after a rotation, type 1
does; Stop without arguments 0. Two probes did not compile there: array elements as a
command's arguments (rewritten as `types.translation` and `types.rotation`, the elements
taken into floats first), and HudBarSetValueScriptVar in every form tried at run time
(`bars` now probes the UDF source only; the variable source needs a script compiled into
a plugin, an open question for the owner). Second run: questions in §6.

## 6. Second run (2026-11-08, head ba45d6dc0)

All 11 rows compiled; lab 7 of 11 before the last fixes. What it added [observed]:
`types.translation` `101 0 1` and `types.rotation` `111 0 1`: the scale of scale.start
was still running (scale.next's Stops named other types), the translation is type 2, and
a Stop without arguments gives 0 and stops nothing (the block still interpolated after
it), so the earlier interpolations kept running; `trait` `1 0 0 0` follows from that; the
`bars` UDF source gives 1 on a made bar and -1 on a missing one. `verts` gave -2 for
"Bip01 Head" where a run without a drawn tree (fnvtool) gave -1: a reference with no tree
yet now reads its model file for the block's name (an actor's skeleton is its base's
model), -2 when it names it.

Still open: HudBarSetValueScriptVar did not compile at run time in any form; it needs a
script compiled into a plugin to probe.
