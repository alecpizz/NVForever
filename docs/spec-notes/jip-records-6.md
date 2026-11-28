# JIP LN NVSE, batch 6 (2026-11-01, #1006)

Destruction data, creature data and messages, in `crates/fnv-game/src/nvse_jip6.rs`.
Sources: the GECK wiki export, each function's page [documented: the page named]; the
fields: xEdit's record definitions as the schema keeps them (`schemas/fnv/common.json`
DEST/DSTD/DMDL, `gameplay.json` CREA and MESG). Not probed in the original: the extender
goldens of this machine's probe game are kept on the other machine, so every value here
comes from the pages and the records; choices the pages leave open are tagged
[hypothesis]. Setters change the form for the session (nothing is saved: "the changes will
not be saved with the game", AddDestructionStage [documented]) and are read by the getters
and by the game.

## 1. Destruction data

`object` is a base form; a reference reads its base's [documented: the pages' `object:ref`,
"only usable with object types that have Destruction Data"]. A form without destruction
data reads 0, none or "".

| Function | NVForever |
|---|---|
| Get/SetDestructionDataHealth `object [health]` | DEST's `health` (`i32`); `SetDestructionDataHealth` returns 0 [observed: gaps8, 75 reads back], ignores forms without destruction data [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 with the 10 official base masters, clean-room script probe jip_13_dest_health_no_dest_form (GetDestructionDataHealth/SetDestructionDataHealth): 0,0,0; no DLL or source consulted], ignores NaN/infinity [hypothesis], and updates `Game::destruction` (`Jip6State::destruction`) |
| GetDestructionDataNumStages `object` | count of DSTD stages on `Game::destruction` (reference resolves to its base) [documented: GetDestructionDataNumStages; observed: gaps3] |
| Get/SetDestructionDataTargetable `object [0/1]` | DEST's flag VATS Targetable |
| GetNthDestructionStageDebris / Explosion `object index` | DSTD's debris and explosion |
| GetNthDestructionStageReplacement `object index` | the stage's DMDL path, "" without one |
| Get/SetNthDestructionStageTrait `object index trait [value]` | 0 health percentage, 1 self damage per second, 2 model damage stage, 3 flags (1 cap damage, 2 disable, 4 destroy), 4 debris count [documented: the page's tables]; a trait past 4 reads 0 |
| SetNthDestructionStageDebris / Explosion `object index [form]` | left out: none [documented: the page's second example] |
| SetNthDestructionStageReplacement `object index path` | |
| AddDestructionStage `object index healthPercent damageStage` | a new stage at the index: the stage's index; an index past the end adds nothing and gives -1; a form without destruction data gets it (not VATS targetable) with the one stage [observed: jip6 dest.add.*, jip6b dest.add.*; the page's "new stage count" is not what the game returns] |
| RemoveDestructionStage `object index` | the removed stage's index, -1 when nothing was removed [observed: jip6 dest.remove.*] |

After an add or a remove, each stage's DSTD index is its place in the list [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 with the 10 official base masters, clean-room script probe jip_14_dest_stage_add_reindex (AddDestructionStage/RemoveDestructionStage/GetDestructionDataNumStages/GetNthDestructionStageTrait): 4,0,77,0; no DLL or source consulted].
NaN and infinity are not set, and AddDestructionStage with a NaN or infinite index adds
nothing and gives -1 [hypothesis]. A health percentage takes any whole number: it wraps as an
unsigned 32-bit value and is kept modulo 101 (150 reads 49, 1000 reads 91, -1 reads 67)
[observed: jip6 dest.settrait.1.0.*, jip6b dest.hp.*]. The self damage and the debris count
take what they are given [observed: jip6b dest.selfdmg.*, dest.debris.*]. The model damage
stage works the same way, modulo 9: the value wraps as an unsigned 32-bit number and is kept
modulo 9, so 9 reads 0, 10 reads 1, 255 reads 3, 256 reads 4, 300 reads 3, 1000 reads 1,
-1 (4294967295) reads 3 and -5 reads 8. No value is refused: the old "10 is refused" read
was 10 modulo 9, the stage's own 1. Records with 1, 2, 3, 4 and 6 stages, first and last
stage alike, give the same results [observed 2026-11-11: VHDExplosiveGeneratorActivator,
TrapBabyCarriage, V22MushroomDoorBusterTrap01, FusionGenerator01 and Truck08, 40 values from
-5 to 1000, each set over 2 and again over 3; probe job `rules2039`, results private].
Read as the field's range 0-8, as the health percentage's modulo 101 is 0-100 [hypothesis]. The int arguments reach the
functions already made whole (the VM's int conversion), so a number past the i32 field
arrives as its nearest end.
The setters return 0, except SetNthDestructionStageReplacement, which returns 1 [observed:
jip6 dest.setrepl, jip6b dest.setexpl.result, dest.settarget.result].

The game reads the changed data too (`Game::destruction`: object damage, the stages'
models and explosions, VATS targeting, GetDestructionStage). Session destruction edits
(`Jip6State::destruction`) are carried across same-run loads via `Game::take_session` /
`put_session` and are not saved in `GameSnapshot` [documented: AddDestructionStage, "the
changes will not be saved with the game"; hypothesis: carried across same-run loads]. The
page says the changes reach only references made after the call [documented: AddDestructionStage, "no effect on
existing references"]; NVForever reads a base's data each time a reference is damaged, so
existing references take them too [decision: one store per base, nothing kept per
reference].

## 2. Creatures

`[baseForm]`, else the calling reference's base [documented: the pages' examples,
`RexRef.GetCreatureDamage` and `GetCreatureDamage Rex`]. Not a creature: 0 / none.

| Function | NVForever |
|---|---|
| Get/SetCreatureDamage `[damage] [baseForm]` | DATA's damage (the unarmed attack's), which combat's creature attack reads too (`Game::creature_attack`); held to 0-65535 (70000 reads 65535, -1 reads 0) [observed: jip6 cr.setdamage.*, jip6b cr.dmg.*]; NaN is not set |
| GetCreatureReach `[baseForm]` | RNAM |
| GetCreatureWeaponList `[baseForm]` | LNAM, the melee weapon list |
| Get/SetCreatureSoundsTemplate `creature [template]` | CSCR ("Inherits Sounds from"); left out clears it [documented] |
| CreatureHasModel `path [baseForm]` | the model list (NIFZ) has the path turned on, any case [documented]; a model turned off is not had [observed: jip6 cr.toggle.off] |
| GetCreatureModels `[baseForm]` | an array of the paths toggled on [documented] |
| ToggleCreatureModel `path on [baseForm]` | turns one on or off: 1 when its state changed, 0 when it already was so; turning on a path not in the list adds it and gives 1 [observed: jip6 cr.toggle.*, jip6b cr.toggle.add, cr.toggle.offagain] |

Toggling a model changes the list the functions read; the engine still draws a creature
from its whole list (follow-up: drawing only the models toggled on).

## 3. Messages

| Function | NVForever |
|---|---|
| Get/SetMessageFlags `message [flags]` | DNAM: 1 Is Message Box, 2 Auto-display [documented]; `7` (`> 3`) is rejected without mutating flags (`mf=1->1`) [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-25, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated clean-room staging copy (read-only probe; zero writes to game install or Saves), `w6d2p3_47_jip6_messages_creatures_and_bptd` (`tools/capture/nvse/catalogs/wave6d2_set_message_flags_bounds.json`): `PROBE\|w6d2p3_47_jip6_messages_creatures_and_bptd\|mf=1->1\|md=0->1\|cr(h=1->0,t=1/1,n=5)\|bp=[W6D2Head]`; zero DLL, PDB, or third-party source consulted]; rejecting `< 0` (e.g., `-1`) and truncating fractional values (`.trunc()`) are [hypothesis], and ignoring omitted/non-finite arguments is [design]; ShowMessage shows a message as a box by the changed flag |
| SetMessageDisplayTime `message seconds` | TNAM, the seconds a corner message stays; a negative value, one past the u32 field, NaN or infinity is not set [hypothesis] |
| Get/SetMessageDisabled `messageOrList [0/1]` | a disabled message is never shown [documented]: by ShowMessage, nor as the box the engine shows on refusing power armour without the training; a form list disables or enables each message in it [documented] |
| ToggleCraftingMessages `0/1` | the recipe menu's "added" corner lines off or on [documented]; returns the state before the call, 1 shown [observed: jip6 msg.crafting, jip6b msg.crafting.twice] |

ToggleCraftingMessages' page names its argument's type but not the argument, and the
compiler counts only named ones, so the catalogue row got a name
(`tools/capture/script_functions.py` PARAM_FIXES).

## Probed in the original (2026-11-08, #1006)

Sets jip6 (92 rows) and jip6b (37 rows) of `tools/capture/jip_probes.py`, run from the
`nvfairport` save on FusionGenerator01, the 10mm round, VStreetCrBrahmin2,
CrSuperMutantDeadMinigunRubyHill, Craig Boone and VCGTutorialJump; goldens private (lab
L5.ext.jip6, L5.ext.jip6b). The values the tables above now tag [observed] come from them.
Equal after the fixes: jip6 90 of 92, jip6b 32 of 37. Left:
- the model damage stage's out-of-range values: settled on 2026-11-11 (§1: modulo 9; #2039)
  and built (`nvse_jip6.rs`): jip6 91 of 91, jip6b 37 of 37 since;
- `SetNthDestructionStageTrait rObj 1 1 3.5` did not compile in the game ("script did not
  compile"): a fractional literal for this argument is refused by the game's compiler
  [observed], and by NVForever's runtime compile since #2037 (ext-syntax.md §11).
