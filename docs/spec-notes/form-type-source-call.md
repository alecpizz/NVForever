# GetIsFormType source-call boundary

[documented] The existing `schemas/script/functions.json` catalogue entry for
[GetIsFormType](https://geckwiki.com/index.php?title=GetIsFormType) describes a
reference predicate whose argument is a four-character record signature, such as
ACTI, CONT or CREA. This is separate from GetObjectType's numeric result.

[observed: baseline 19c0f08a portable source] The catalogue declares the function,
but the named-call dispatcher has no handler. `functions::base_of` already resolves
placed references and temporary inventory references to their base forms. The
existing `nvse_other` base helper also leaves a direct base-form argument available
for signature lookup when it is not a placed reference.

[observed: candidate design; current execution pending] Resolve the calling subject
through that helper and compare its base signature with a four-byte argument.
Return false for missing subjects, wrong-length arguments or different signatures.
The isolated owned fixture checks an ACTI placed reference, a WEAP base and an actual
script-created inventory reference, plus mismatched/invalid argument controls. It
contains no installed mod text or game data. Its current-base execution is pending.

[observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24,
GSDocMitchellHouse (`0x00103DF9`), 10 official base masters, isolated staging copy
(read-only), committed probe `w4g_01_getisformtype_case` in
`tools/capture/nvse/catalogs/wave4g_f2_extenders.json` (companion catalog PR #3803);
no DLL or source consulted] `GetIsFormType` is strictly ASCII case-sensitive on the
4-character base-form signature and requires a reference receiver (placed/spawned
`REFR`), rejecting direct base forms with `0` (logging a base-object error while
continuing the script). Verbatim probe output:
- `PROBE|w4g_01_getisformtype_case|door(DOOR=1,door=0,Door=0,dOoR=0,ACTI=0,DOO=0,DOORS=0,emp=0)`
- `Attempting to call a function on a base object (this must be a reference)`
- `PROBE|w4g_01_getisformtype_case|weapBase(WEAP=0,weap=0,Weap=0,DOOR=0)`
- `PROBE|w4g_01_getisformtype_case|weapRef(WEAP=1,weap=0,Weap=0,DOOR=0)`

[design] NVForever resolves the calling subject through `functions::base_of` requiring
`base != subject` so placed references, spawned references (`PlaceAtMe`), `Player`
(`0x14`), and temporary inventory references resolve to their base form while direct
base forms (including `WEAP`, whose `base_of` returns itself for perk conditions) return
`0` and continue without printing xNVSE's console diagnostic text. This named-call patch
adds no native opcode mapping and does not establish event-handler delivery, clone
signatures or complete Quick Drop behavior. Earlier aggregate-primary small-library
results remain historical only.
