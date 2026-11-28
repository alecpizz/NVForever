# World-object model getters

[observed: baseline 0b16bea3 portable source] `GameData::FIELD_SIGS` retains lazy
record-field handles for ACTI but not STAT, MSTT, FURN, DOOR or TACT. The existing
`GetModelPath` handler reads MODL through those fields. A decoded authored MODL can
therefore be present while this public getter receives no field for those five types.
The field definitions are the repository's `schemas/fnv/objects.json` and
`schemas/fnv/placed.json`.

[implementation] Retain the five additional winning-record handles. Keep lazy field
decoding and return the authored string through the existing getter, including its
case, spaces and separators. The owned synthetic fixture covers all six types, a
model-less/null control, a placed reference, clone-local model changes, original
independence and a fresh Game. These fixtures contain no installed plugin or asset.

[observed: earlier owned portable qualification, separate source] The isolated
repair's three tests failed before the admission change and passed afterward; its
owned headless fixture returned all six paths afterward. That earlier aggregate
branch is not this upstream candidate's tested source. Current 0b16 qualification is
pending; an assertion failure in a loop does not prove later loop cases executed.

[observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24,
`GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated staging copy,
read-only, `w4g_02_tempcloneform_world_proj_csty`
(`tools/capture/nvse/catalogs/wave4g_f2_extenders.json`, companion catalog PR #3803):
printed `stat(orig=Architecture\Urban\Rubble\BLD03Chunk01.NIF|cl1=w4g_pre_stat.nif|postSrc=w4g_post_stat.nif|cl2=w4g_pre_stat.nif|srcAfterCl=w4g_post_stat.nif|cl3=w4g_cl_stat.nif)`
and `misc(orig=Clutter\Junk\NukaColaCap.NIF|cl1=w4g_pre_misc.nif|postSrc=w4g_post_misc.nif|cl2=w4g_pre_misc.nif)`;
no DLL or source consulted] `TempCloneForm` performs a copy-at-creation snapshot of
`MODL` paths for `STAT` and `MISC`: clones inherit pre-clone `SetModelPath` edits and
stay isolated from post-clone `SetModelPath` edits to the source form (and edits to a
clone stay isolated from the source). NVForever snapshots `MODL/` overrides onto the
clone at `TempCloneForm` creation (`Game::snapshot_model_path`) and reads only the
form's own `MODL/` override before falling back to the authored record's `MODL` field.
[hypothesis] Applying the same creation-time `MODL/` snapshot isolation across other
record types sharing the `MODL/` override table (`DOOR`, `ACTI`, `FURN`, `MSTT`, `TACT`,
`WEAP`, `ARMO`, `BPTD`) is inferred from `STAT`/`MISC`/`IDLE` (`w4g_02`, `w4g_13`).
This metadata correction does not fix or qualify cloned-form scene placement, rendering,
physics, model reload or snapshot lifetime. Adding record handles has an unmeasured
memory cost; no speedup is claimed.

## Root-relative marker lookup

[observed: PR #2374 core CI on 1b96da7a] Admitting STAT fields made the existing
SetInternalMarker fixture consult its authored `markers\testmarker.nif` path. The
path predicate accepted a nested markers directory but missed the root-relative
markers directory, so the existing `jip12::cells_light_and_settings` assertion
changed from 1 to 0. Previously absent model fields had selected an Editor-ID
fallback. The failed CI log is retained; the three new model getter tests passed.

[observed: candidate correction] Include the root-relative markers directory in the
existing model predicate. Keep the existing meaningful regression and its expected
value, alongside the three authored model getter tests. Current follow-up execution
is pending; this is not original-game or rendered marker parity evidence.
