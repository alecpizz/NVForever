# JIP LN NVSE, batch 8 (2026-11-08, #1006)

Actor base data, actor effects, ingestibles and base effects, weapon fields and mods,
idles, body-part names and limb models, cells and water, in
`crates/fnv-game/src/nvse_jip8.rs`. Sources: the GECK wiki export, each function's page
[documented: the page named]; the fields: xEdit's record definitions as the schema keeps
them (`schemas/fnv`); the probe set `jip8` (`tools/capture/jip_probes.py`, run in the
original from the nvfairport save with JIP LN 57.30; lab `L5.ext.jip8`) [observed]: 263
of 282 rows matched on the first run, and the 18 that differed for a rule are fixed to
follow the game (the 19th, water trait 15, in §5). Choices still open are tagged
[hypothesis]. Setters change the form for the session and are read by this batch's
getters and the earlier ones (GetBodyPartName, GetBodyPartData, GetSkeletonModel,
GetBaseEffectFlag, GetEffectFlag, GetCellFlag, GetCellWaterForm, GetWaterFormEffect,
GetWeaponItemMod, GetWeaponItemModEffect, GetWeaponItemModValue1, GetWeaponOnHitSetting,
GetWeaponSemiAutoFireDelay, GetWeaponModel). Every setter returns 0 [observed: jip8, as
batch 5's did, though the catalogue names a result for some].

47 functions; JIP 248 → 201.

## 1. Actor base data

A `[baseForm]` argument, else the calling reference's base; not an NPC or creature, the
page's nothing (0, none) [documented: each page's `ref.F` and `F … baseForm` forms].

| Function | NVForever |
|---|---|
| Get/SetDeathItem `[item] [base]` | INAM |
| GetActorTemplate `[base]` | TPLT |
| Get/SetActorLevelingData `type [value] [base]` | ACBS: 0 the fixed level, or the level mult when the actor has PC Level Mult (ACBS flag bit 7; the record keeps it × 1000), 1 calc min, 2 calc max, which is not set below calc min [documented: the table and the example's "level multiplier to 0.75"; observed: Boone's raw 750 reads 0.75, Angela's calc max set to 0 reads her calc min 1]; past 2, 0 |
| GetBaseFactionRank `faction [base]` | the base's SNAM rank, or its template's when it takes its factions from one; -1 when not a member [documented; observed: a mercenary with a Use Factions template reads the template's faction] |
| GetInFactionList `list` | the calling actor in one of the list's factions now (AddToFaction and the rest counted) [documented] |
| GetNumActorEffects, GetNthActorEffect `[index] [base]` | SPLO, the record's last first; past the end none [observed: the Super Mutant's two in reverse record order]. The original compiles GetNthActorEffect only straight into a variable (`let r := …`) |
| Add/RemoveBaseEffectListEffect `effect [base]` | the list, an actor effect (SPEL) only; added at the front, once [documented: "the specified Actor Effect"; observed: after two adds the list is one longer and the old first is second] |
| SetBodyPartData `creature bpd` | CREA PNAM [documented]; GetBodyPartData reads it |
| SetCRESkeletonModel `creature path`, SetNPCSkeletonModel `npc [path]` | MODL; an NPC's path left out, sNPCModelMale or sNPCModelFemale by its ACBS Female bit [documented]; GetSkeletonModel reads it |
| Get/SetExcludedCombatActions `[mask]` | the calling actor's mask of the 27 combat actions, bits past 26 dropped; 0 until set; not saved [documented: "reset upon loading a game"] |

The actor records keep INAM, TPLT and SPLO for these (`ACTOR_SUBS`, data.rs).

## 2. Ingestibles and base effects

| Function | NVForever |
|---|---|
| Get/SetAddictionChance `alch [chance]` | ENIT addiction chance, kept as given (50, 1.5 and -0.5 too) [observed]; accepts `TempCloneForm` `ALCH` clones and feeds `Game::ingest_on` through `Game::ingestible_def` [design] |
| Get/SetAddictionEffect `alch [effect]` | ENIT withdrawal effect, an actor effect; none is ignored [observed: Cateye's stays]; accepts `TempCloneForm` `ALCH` and `SPEL` clones and feeds `Game::ingest_on` and `Game::sated` through `Game::ingestible_def` [design] |
| Get/SetBaseEffectScript `mgef [script]` | the associated script of a script-type base effect (archetype 1); any other, none and unchanged [documented: "script-type Base Effect"]; accepts `TempCloneForm` `MGEF` clones and feeds `Game::apply_magic_item_with` through `Game::magic_effect_def` [design] |
| SetBaseEffectFlag `mgef flag set` | any of MGEF DATA's 32 bits, not only the page's table's [documented: the table; observed: flag 11 is set]; accepts `TempCloneForm` `MGEF` clones and feeds `Game::magic_effect_def` across magic application, aid preview/names, poison detection (`Game::is_poison`, `GetIsPoison`, `GetIsPoisoned`), hit resistance, death dispel, pickpocket explosive harm, and item effect text [design] |
| SetEffectFlag `form flag set` | any bit 0-7 of an actor effect's SPIT or an object effect's or ingestible's ENIT flags, not only the tables' [documented: the three tables; observed: every bit tried sticks]. GetEffectFlag reads SPIT for SPEL and ENIT for ENCH/ALCH through `Game::effect_flags` (including `TempCloneForm` clones), `Game::area_targets` reads SPEL SPIT bit 4 ("Area Effect Ignores LOS") via `Game::spell_ignores_los`, `Dispel` uses `Game::is_ability_effect` for cloned Ability spells, and `Game::aid_preview` / `Game::ingest_on` / `Game::item_service` / `GetIsUsedItemEquipType` read ALCH ENIT flags and `ETYP` via `Game::ingestible_def` [design] |

`TempCloneForm` snapshots effective `SPEL`, `ALCH`, `MGEF`, and `ENCH` trait/flag overrides (`Game::snapshot_magic_traits`) and effect-list edits (`Game::snapshot_magic_effects`) so pre-clone edits carry over while post-clone edits on the source remain isolated [design]. Cloned magic items (`TempCloneForm` `ALCH` / `MGEF`) inherit record effects, support `AddNewEffect` isolated to the clone, and preserve pre-clone `SetEffectFlag` / `SetBaseEffectFlag` edits with post-clone isolation [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-25, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated clean-room staging copy (read-only probe; zero writes to game install or Saves), `w6d2_06_magic_effects_tempclone_isolation` (`tools/capture/nvse/catalogs/wave6d2_magic_effects_tempclone.json`): `PROBE|w6d2_06_magic_effects_tempclone_isolation|n=4/4,e0Ok=1,m0=5.0|add=4,nAft=5/4|ef(pre=1,post=1)|bf(pre=1,post=1)`; zero DLL, PDB, or third-party source consulted]. Original-game capture parity for `TempCloneForm` clone-of-clone isolation and live mid-session consumer re-evaluation of `SetBaseEffectFlag`/`SetBaseEffectScript`/`SetEffectFlag` remains unqualified.

## 3. Weapons

| Function | NVForever |
|---|---|
| Get/SetWeaponDetectionSoundLevel `weapon [level]` | VNAM, 0 loud, 1 normal, 2 silent [documented] |
| SetWeaponItemMod `weapon slot [mod]` | WMI1-3; left out, null, or a non-IMOD form clears the slot to 0 [documented: the examples; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated clean-room staging copy (read-only probe; zero writes to game install or Saves), `jip_20_weapon_item_mod_non_imod` (`tools/capture/nvse/catalogs/wave4a_extenders.json`): `PROBE\|jip_20_weapon_item_mod_non_imod\|m0=0010C858,m1=00000000,eq0=1,eqSt=0,eqM0=0`; zero DLL, PDB, or third-party source consulted] |
| SetWeaponItemModEffect, SetWeaponItemModValue `weapon slot value` | DNAM effect_mod1-3 and value_a_mod1-3; an effect 1-16, 0 (none) ignored [documented: the table, 0-14, and the JohnnyGuitar note; observed: 0 leaves the effect, 15 is stored; Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 with the 10 official base masters, clean-room script probe jip_21_weapon_mod_effect_16 (GetWeaponItemModEffect/SetWeaponItemModEffect): 2,2,2; no DLL or source consulted] |
| SetWeaponOnHitSetting `weapon 0-3` | DNAM on_hit [documented] |
| SetWeaponSemiAutoFireDelay `weapon setMax delay` | DNAM's min or max delay, raw finite floats including negative values stored unclamped [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 with the 10 official base masters, clean-room script probe jip_22_weapon_semiauto_delay_neg (GetWeaponSemiAutoFireDelay/SetWeaponSemiAutoFireDelay): 0.000,-0.500; no DLL or source consulted] |
| SetWeaponModel `weapon mask path` | 0 MODL, 1-7 MWD1-7, as GetWeaponModel reads them [documented: the table] |
| Get/SetWeaponModReloadAnim `weapon [anim]` | DNAM reload_animation_mod, 0-22 [the enum: xEdit WEAP] |
| WeaponHasModType `weapon type` | the slot 1-3 whose effect is the type, else 0; a placed weapon by its base [documented]; the type IDs are DNAM's effect values [the enum: xEdit WEAP; observed: the 10mm pistol's 2, 3, 11 in slots 1-3] |
| EquippedWeaponHasModType `type` | an attached mod of the calling actor's equipped weapon (its mod bits 1, 2, 4) has the type [documented] |
| SetEmbeddedWeaponAV `weapon av` | DNAM's embedded weapon actor value: PerceptionCondition … BrainCondition as 0-6 [the enum: xEdit WEAP; documented: the example]; any other actor value changes nothing |
| SetEmbeddedWeaponNode `weapon node` | NNAM [documented] |

## 4. Idles and body parts

| Function | NVForever |
|---|---|
| Get/SetIdleLoopTimes `idle getMax` / `idle min max` | IDLE DATA loop_min, loop_max, each clamped to a u8, the max raised to the min [documented; observed: RepairWall02 2/5, and 300 2 reads 255 255] |
| Get/SetBodyPartReplacementModel `bpd part [path]` | the part's NAM1; "" without one or without the part; a part the record lacks is not added [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 with the 10 official base masters, clean-room script probe jip_23_bodypart_repl_missing_part (GetBodyPartReplacementModel/SetBodyPartReplacementModel): 0,0; no DLL or source consulted] |
| SetBodyPartName `bpd part name` | the part's BPTN, read by GetBodyPartName |

A part is the group of subrecords from its BPTN (or its BPNN when it has no name) to the
next part, holding the BPND of that part type [documented: xEdit BPTD].

## 5. Cells and water

| Function | NVForever |
|---|---|
| SetCellFlag `cell flag set` | 0-7 DATA's bits (4 too, which the table leaves out), 8-16 the lighting template inherit flags, LNAM bits 0-8 [documented: the table; observed: 4 is set; GetCellFlag 8-16 follow LNAM in this order on Doc Mitchell's house and the saloon]. GetCellFlag reads 8-16 the same way now (it read DATA, so 0) and requires a `CELL` form (and integer `flagID` in `0..=16`), returning `0` for non-`CELL` forms such as `WRLD` so worldspace `DATA` flags are not read as cell flags [documented: GECK wiki GetCellFlag (`cell:ref`, `flagID:int{0-16}`); hypothesis: exact original JIP LN return on a `WRLD` with bit 0 set is pending an original-game runtime probe, #3757] |
| SetCellWaterForm `cell water` | XCWT, a water (WATR) [documented]; GetCellWaterForm reads it |
| SetWaterFormEffect `water [effect]` | the actor effect, or none; GetWaterFormEffect and drinking (`Game::drink_from`) read it. In the original it has no result (`let x := SetWaterFormEffect …` does not compile) |
| GetWaterTrait `water trait` | 0 opacity (ANAM), 1 the flags (FNAM), 2-50 DNAM in its order; colours 12-14 as R × 1000000 + G × 1000 + B; the table's "Unknown" 9 (unused bytes), and past 50, 0 [documented: the table; observed: NVCleanWater's 0-14 and 16-50] |

Water trait 15 is the one row left apart: the original reads 6.53e-39 (the bits 0x00471CFE
as a float) where the record's bytes are 0, a value of its own at run time [observed:
jip8]. NVForever gives the record's 0 [decision: no record value to give].

## 6. Not built here, and follow-ups

- SetBaseFactionRank: the base factions feed detection, combat and templates straight
  from the loaded records (`GameData::actors`, `base_factions_at`); a session change
  needs those readers moved onto one lookup first.
- Get/SetActorUnarmedEffect: the NPC and creature records' unarmed attack effect is not
  in the schema yet (`schemas/fnv` NPC_ and CREA list no EITM).
- The engine and the game rules still read some of these records as loaded: the combat AI does
  not skip excluded actions; spawned actors take their abilities from the record's SPLO;
  engine-side UI/audio readers (`engine/nvf-play`) still read loaded MGEF/ALCH maps;
  combat reads the weapon's mods,
  on-hit setting, delays and sound level as loaded; the idle player its loop counts;
  dismemberment its limb models; the level calculation ACBS as loaded; drinking-water
  radiation (`radiation::water_rads`) the record's effect.
