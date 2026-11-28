# Spec note: JIP LN batch 4 (record traits and game-state getters)

The fourth JIP batch (issue #955, `crates/fnv-game/src/nvse_jip4.rs`, 2026-10-30). The
census (`extender_census`) counted 390 of JIP LN's 768 functions unbuilt before it, and 344
after. The owner's mod lists (docs/handoff/modlist-function-counts.json) call only 40 of the
390 names, each a few times, and most of those need the engine: the cursor, NIF material
properties, the menus. So this batch takes the families that already have built siblings
(the projectile, explosion, ammo, weapon, sound and weather traits; jip-records.md,
`nvse_traits.rs`) and can be read from the records headless [decision].

Wiki statements are `[documented: <page>]` (the private export
`C:\fnv-private\geckwiki\functions.xml`). Field names are the Layer 1 schema's
(`schemas/fnv`). The probe set `jip4` (tools/capture/jip_probes.py, 281 rows, golden
`jip/jip4.json`, lab scenario `L5.ext.jip4`) ran these functions in the original game
with JIP 57.30 from the `nvftest` save on 2026-10-30, three times; the rows that two runs
share came back the same but for one GetDelayElapsed row (§10). Values marked [observed]
are from it. The example values in §2–§9 are the game's and NVForever's alike.

## 1. Rules shared by every setter

- A setter changes the record for the session only. It is kept in `Game::field_overrides`,
  so every reader of the field through `nvse::record_value` sees it. The engine's
  drawing, sky and combat go on reading the records [hypothesis: as with the other
  record setters, nothing is saved].
- An ID outside a page's table reads 0 or "" and a setter with one does nothing
  [hypothesis].
- A flag ID is a bit number of the record's flags field, as with the projectile and
  explosion flags [observed: jip-records, projflag/explflag]. Here it is
  [documented: each page's flag table, which matches the schema's bit names].

## 2. Classes (CLAS)

`GetClassTraitNumeric class n`, `SetClassTraitNumeric class n value` [documented: their
pages, IDs 0–10]:

| n | Trait (wiki) | Field |
|---|---|---|
| 0–3 | tag skill #1–#4 (actor value codes) | DATA tag_skills[n] |
| 4–10 | base Strength … Luck | ATTR strength … luck |

SetClassTraitNumeric returns the value it set [documented: returnType traitValue].

`GetClassFlag class id` / `SetClassFlag class id value` [documented: IDs 0–19]:
- 0 Playable and 1 Guard are DATA flags bits 0 and 1.
- 2–19 are the service flags from bit 0. The page names 2 weapons … 8 stimpaks,
  12 miscellaneous, 18 recharge and 19 repair; the schema's ServiceFlags has them at
  bits 0–6, 10, 16 and 17, so ID = bit + 2 [documented; the offset is read from the two
  tables].

FalloutNV.esm, the page's example class NCRRanger: tag skill #3 42, Strength 5, Luck 5.

## 3. Combat styles (CSTY)

`Get/SetCombatStyleFlag style id` [documented: IDs 0–8]: CSTD flags, whose bits the
schema names the same way (bit 3 has no name on either side).

`Get/SetCombatStyleTraitNumeric style n` [documented: IDs 0–63, their names]:
- 0–14 are CSSD in order: cover search radius … semi-auto firing delay mult max, with
  9 the weapon restrictions.
- 15–42 are CSTD in order: dodge chance … rushing attack distance mult.
- 43–63 are CSAD in the record's field order.

The page lists the dodge fatigue, block skill and attack skill pairs base first, but CSAD
keeps them mult first, and the IDs follow CSAD, not the page's names [observed: jip4,
FollowersCombatStyleRanged 43 reads -20, CSAD's first field; 44 reads 0; the same for
53/54 and 57/58]. FollowersCombatStyleRanged: cover search radius 2048, combat radius
10000, dodge chance 50.

## 4. Lights (LIGH)

`Get/SetLightTraitNumeric light n` [documented: IDs 0–6]: 0 radius, 1–3 red, green and
blue (DATA radius, color.red/green/blue), 4 falloff exponent, 5 FOV (DATA), 6 fade
(FNAM). `Get/SetLightFlag light id` [documented: IDs 0–31]: DATA flags bits. The page's
11 and 12 (colour shift) have no schema name and read the bit all the same.
BasementLightFillWarm: radius 200, red 177, fade 1.25.

## 5. Grass (GRAS)

`Get/SetGrassTraitNumeric grass n` [documented: IDs 0–9] are the DATA fields in order:
density, min slope, max slope, units from water amount, units from water type, position
range, height range, colour range, wave period, flags (1 vertex lighting, 2 uniform
scaling, 4 fit to slope, the schema's bits 0–2). `Get/SetGrassModel` is MODL
[documented]. GrassWasteland01: density 30, position range 22, model
`Landscape\Grass\GrassWasteland01.NIF`.

## 6. Texture sets (TXST)

`Get/SetTextureSetTexture set n` [documented: map IDs 0–5, diffuse … environment] read
TX00–TX05, "" when the subrecord is absent. 1stPersonSniperRifleTexture 2 (the page's
example) reads `Weapons\2HandRifle\SniperRifle_m.dds`. Not built: the texture set's
flags 1–3 and numeric traits, which belong to the decal data (DODT) that the schema does
not decode yet.

## 7. Climates (CLMT)

`Get/SetClimateTraitNumeric climate n` [documented: IDs 0–6]:
- 0–3 are the sunrise and sunset begin and end in minutes. TNAM keeps them in 10-minute
  units, so the trait is the byte × 10 [documented: the page says minutes; the schema's
  wbClmtTime note says 10-minute units]. WastelandClimate (the page's example) has
  sunset begin 960, sunrise begin 300.
- 4 is volatility (TNAM).
- 5 (moons) and 6 (phase length) share TNAM's last byte: the moons are its top two
  bits, the phase length its low six [observed: jip4, WastelandClimate reads moons 2
  and phase length 3; SetClimateTraitNumeric 6 7 reads back 7 and leaves the moons 2].
- Bounds and validation: sun times (`0..=3`) convert via unsigned 10-minute slots capped
  at `143` (`25 -> 20`, `1500 -> 1430`, `-10 -> 1430`), moons (`5`) clamp to `0..=3`
  (`4 -> 3`), phase length (`6`) clamps to `0..=63` (`64 -> 63`), and non-`CLMT` forms
  are ignored (`bad=0`) [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN
  57.30, 2026-11-25, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters,
  isolated clean-room staging copy (read-only probe; zero writes to game install or
  Saves), `w6d2_16_climate_traits_and_weather_add_remove`
  (`tools/capture/nvse/catalogs/wave6d2_set_climate_trait_bounds.json`):
  `PROBE|w6d2_16_climate_traits_and_weather_add_remove|sun(300->25:20,1500:1430,neg:1430)|m5(2->4:3)|p6(3->64:63)|bad=0|wlst(n=1->1,r=1,1,1,1)`;
  zero DLL, PDB, or third-party source consulted].

## 8. Quests

`GetQuestFlag quest id` / `SetQuestFlag quest id value` [documented: IDs 0–7]:
- 0 running, 1 completed and 6 failed are the quest's state, the same one GetQuestRunning,
  GetQuestCompleted and FailQuest use.
- 2 and 3 are the record's DATA flag bits 2 and 3 (repeated topics, repeated stages).
- 4 (set running on reset) is DATA bit 0, Start Game Enabled [documented: the page's note,
  Start Game Enabled "equals flags 0 and 4"].
- 5 (shown in the Pip-Boy) reads an explicit `SetQuestFlag q 5 0/1` override first,
  otherwise record bit `0x20` or 1 for a named quest that has completed or has a
  displayed/completed objective [observed: jip4, 1 on VCG01 and VCG02, which the save has
  run, 0 on VMS01 and VMS20, which it has not, and 0 on VDeadSea, which runs from the
  start with no name; SetQuestFlag 5 1 reads back 1; Fallout: New Vegas 1.4.0.525 + xNVSE
  6.4.9 + JIP LN 57.30 with the 10 official base masters, clean-room script probe
  jip_27_quest_flag_5_shown (GetQuestFlag): 0,0; Fallout: New Vegas 1.4.0.525 + xNVSE
  6.4.9 + JIP LN 57.30, 2026-11-25, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base
  masters, isolated clean-room staging copy (read-only probe; zero writes to game install
  or Saves), `w6d2_26_quest_flag_5_clear_and_all_bits`
  (`tools/capture/nvse/catalogs/wave6d2_set_quest_flag_5_clear.json`):
  `PROBE|w6d2_26_quest_flag_5_clear_and_all_bits|vcg(0=1,4=1,5=0->on:1->off:0)|vms5=0->1|ds=1,0|f7=0->1,f8=0`
  (running named `VCG01` starts with flag 5 = `0`, `SetQuestFlag 5 1` sets `1`,
  `SetQuestFlag 5 0` clears back to `0`, `VMS01` `0->1`, unnamed running `VDeadSea` `1,0`,
  flag 7 `0->1`, OOB flag 8 `0`); no DLL or source consulted; hypothesis: the
  completed/displayed-objective fallback when flag 5 was not explicitly overridden].
- 7 is unknown and reads as a script set it.

## 9. Factions, cells, waters, sounds

- `Get/SetFactionReputationType faction`: FACT WMI1 [documented: its page; the field: the
  schema's Reputation]. NCRFactionNV reads 0F43DE.
- `Get/SetCellClimate`, `Get/SetCellLightingTemplate`, `Get/SetCellNoiseTexture`: CELL
  XCCM, LTMP and XNAM [documented: their pages]. Gomorrah00, the pages' example, has none
  of the first two (0).
- `Get/SetWaterSound`, `Get/SetWaterNoiseTexture`: WATR SNAM and NNAM [documented]. NVCleanWater:
  sound 167340, noise `Data\Textures\Water\WastelandWaterPotomac.dds`. Not built:
  GetWaterTrait's 51 IDs, many of which its page leaves "Unknown".
- `GetSoundFlag sound id` / `SetSoundFlag sound id newValue` [documented: GECK wiki
  GetSoundFlag, SetSoundFlag; `schemas/fnv/objects.json` `SOUN.SNDD.flags`/`SNDX.flags`
  bits `0..=13` (`0` Random Frequency Shift, `1` Play At Random, `2` Environment Ignored,
  `3` Random Location, `4` Loop, `5` Menu Sound, `6` 2D, `7` 360 LFE, `8` Dialogue Sound,
  `9` Envelope Fast, `10` Envelope Slow, `11` 2D Radius, `12` Mute When Submerged, `13`
  Start at Random Position); observed: gaps4, SetSoundFlag returns 0]: SNDD flags (SNDX
  in older records), bits the schema names the same way. EMTMedicalBeep01: loop (4) 1,
  2D (6) 0. `SetSoundFlag` stores per-`SOUN` session bitmask overrides
  (`Game::sound_flags`, `Game::sound_flag`, `Game::set_sound_flag`,
  `Game::sound_flags_override`, `Game::snapshot_sound_flags`) [design] with
  `TempCloneForm` snapshotting and post-clone isolation [hypothesis: also session-only
  lifetime and rejection of fractional/out-of-range flag IDs and non-`SOUN` forms].

### 9.1 `TempCloneForm` SOUN aliases and engine sound overrides

- [documented: GECK wiki `TempCloneForm`, `SetSoundSourceFile`, `SetSoundTraitNumeric`]
  A temporary clone of a `SOUN` record keeps its own FormID while inheriting the
  original sound's `FNAM` path and `SNDD` traits. Session overrides via
  `SetSoundSourceFile` and `SetSoundTraitNumeric` apply to the target FormID without
  mutating the original.
- [hypothesis] Pre-clone `SetSoundSourceFile` and `SetSoundTraitNumeric` (`0..=11`) edits
  on an original or parent clone carry onto newly created `TempCloneForm` `SOUN` clones,
  while post-clone edits on the original or parent clone stay isolated from
  already-created clones (e.g., silencing an original `SOUN` with trait `3 = -99` after
  cloning a playable copy).
- [design] `TempCloneForm` snapshots the source form's active `SetSoundSourceFile` and
  `SetSoundTraitNumeric` (`0..=11`) overrides onto the new clone at creation time
  (`Game::snapshot_sound_traits`), and `Game::sound_source_override` /
  `Game::sound_trait_override` read only the queried FormID's own entry before falling
  back to the authored ESM `FNAM` / `SNDD` record.
  `engine/nvf-play/src/world_sounds.rs` (`sync_script_sounds`) runs in `PlaySet::Sync`
  (before `SfxSet` resolves `PlaySound` requests): it materializes cloned `SOUN` entries
  in `sfx.forms.sounds` from their original's authored `Sound` with `edid = None`
  (preventing accidental EDID-lookup collisions), applies `SetSoundSourceFile` (mapping
  empty/whitespace paths to `sound.file = None` so empty-path shadowing silences
  playback cleanly) and `SetSoundTraitNumeric` traits `0..=11` (`min_distance`,
  `max_distance`, `frequency_adjust_percent`, `static_attenuation_db`, `stop_hour`,
  `start_hour`, `attenuation_curve[0..5]`, `reverb_attenuation_percent`) to both
  authored and cloned `SOUN` entries, and restores mutated authored `SOUN`s while
  pruning all stale clones across sessions (including when a new session reuses a
  deterministic clone FormID such as `0x00FE_0000` for a non-`SOUN` form).
- [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24,
  `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters,
  `w4c_b1_21_sound_and_diff_bounds` (`tools/capture/nvse/catalogs/wave4c_extenders.json`,
  #3679): `SetSoundTraitNumeric 0x403C2 12 77` left `GetSoundTraitNumeric 0x403C2 12` at
  `0` (`prio(0->0)|badSnd(0,0)|diff(3,9=3,-1=3)`); no DLL or source consulted] Trait `12`
  is ignored/unwritable by `SetSoundTraitNumeric` in JIP LN 57.30, so NVForever only
  stores `SetSoundTraitNumeric` overrides for traits `0..=11` (matching the traits carried
  on `fnv_combat::fx::Sound`). Clone-of-clone override snapshotting and original-game audio
  parity remain unqualified against original-game captures [hypothesis].

## 10. Game-state getters

- `GetGameDifficulty`, `SetGameDifficulty n` [documented: 0 very easy … 4 very hard]:
  NVForever's difficulty. A number outside 0–4 changes nothing [observed: Fallout: New
  Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 with the 10 official base masters,
  clean-room script probe jip_26_game_difficulty_out_of_range
  (GetGameDifficulty/SetGameDifficulty): 3,3,3; no DLL or source consulted].
- `GetSessionTime` [documented: "the game's current session duration in seconds"]: the
  seconds run since the game started, menus included, as each frame's own length added
  up (the engine's frame time changes every frame, so the frame count times the current
  one is no clock, #1297) [hypothesis: a loaded save does not start a new session].
- `GetPCDetectionState` [documented: -1 n/a, 0 hidden, 1 caution, 2 detected, 3 danger]:
  the highest band any actor has the player in (`detection::Detections::highest`): hidden 0,
  noticed 1, detected 2, and 3 when that actor's combat target is the player
  [observed: jip4, 2 in the `nvftest` save, where Ringo, not hostile, stands near the
  player; hypothesis: 3 in combat, not probed]. With no measure it reads 0. NVForever never reads
  -1, because the function runs only in a game.
- `GetRefType` [documented: the type code of the reference, not its base]: the
  reference's own record type. The player is an ACHR; an inventory reference or one made
  at run time is a REFR, an ACHR or an ACRE by its base, as the script host names it.
- `GetTeleportDoor` [documented]: the door at the other end, SetDoorTeleport's change
  first (the same reading as GetLinkedDoor).
- `GetDelayElapsed quest` [documented: "the time remaining until the next iteration of
  the quest's script"]: the script delay minus the seconds since it last ran. It goes
  below 0 for a moment before the script runs, so there is no floor [observed: a scratch
  capture read VDeadSea 1.397, 1.381, 1.365 … s, 15.6 ms less each frame; one jip4 run
  read VCG01 below 0 and the next above]. It stays within the delay (5 s here).
- `SetDelayElapsed quest seconds` [documented: sets the remaining time until the quest's
  script next runs]: sets `since_run = (delay - seconds.max(0)).max(0)` and returns `0.0`
  (not `seconds`) [observed: 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), Fallout:
  New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 with the 10 official base masters,
  isolated clean-room staging copy (read-only `PrintToConsole` probe; zero writes to game
  install or Saves), committed probe `w6t2_31_xnvse_items_ammo_rounds_and_ingrid`
  (`tools/capture/nvse/catalogs/wave6_t2_set_delay_elapsed.json`):
  `PROBE|w6t2_31_xnvse_items_ammo_rounds_and_ingrid|ammo(u=0,0=0,5=0)|grid(def=1,0=1)|del=0.00|tf(ref=0,arg=0)`;
  zero DLL, PDB, or third-party source consulted].
- `GetGlobalRef global`, `SetGlobalRef global [form]` [documented: their pages; on a
  reference with no form, the reference]. The form ID is written into the global's own
  value as the bits of its float, and GetGlobalRef reads the bits back, 0 when they name
  no form [observed: jip4, GameYear 2281 reads GetGlobalRef 0; after SetGlobalRef of
  00104C0F it reads GetGlobalValue 1.49665e-39, which is those bits].
- `GetEditorPosition` [documented: returns the cell and writes the six output variables;
  0 for a reference not placed in the editor]: angles are written in degrees matching
  `GetStartingAngle`, and when called on a reference with no editor placement (such as
  `Player`), it returns `00000000` and preserves all six output variables unchanged
  [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525 (`040020D0`) + xNVSE 6.4.9 + JIP LN
  NVSE 57.30 (`5730`) + lStewieAl's Tweaks 1005 (`bUseConsoleOutputFile=1`), 10 official
  base masters, isolated clean-room staging copy (read-only `PrintToConsole` probe; zero
  writes to game install or Saves), `GSDocMitchellHouse` (`0x00103DF9`), `DocMitchellRef`
  (`0x00104C0F`) and `Player`, committed probe `w6_t1_03_pos_ex_and_editor_pos`
  (`tools/capture/nvse/catalogs/wave6_t1_editor_pos.json`):
  `PROBE|w6_t1_03_pos_ex_and_editor_pos|posExPc=0(0.00,0.00,0.00)|posExDoc=0(0.00,0.00,0.00)|edDoc=00103DF9(dpos=0.00,0.00,0.00|rot=0.000,0.000,22.918|start=0.000,0.000,22.918)|edPc=00000000(111.0,222.0,333.0,444.0,555.0,666.0)`;
  no DLL, PDB, or third-party source consulted]. (Earlier 2026-10-30 probes passed bare
  variable names, which the runtime compiler refuses; parenthesized `(ox) ... (rz)`
  compiles cleanly.)
- `GetStartingPos axis`, `GetStartingAngle axis` (vanilla, built with this batch in
  `functions/refs.rs`): the reference's placement in the plugins, the angle in degrees;
  the player reads 0 on every axis [observed: jip4, DocMitchellRef 2288, 2244, 7360 and
  angle Z 22.918; his door 2430, 1536, 7359]. For dynamic references created via
  `PlaceAtMe`, `GetStartingAngle` returns the reference's creation heading (`RefState::made_rot`,
  optional backwards-compatible save field) in degrees rather than `0` [observed: Fallout:
  New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-23, `GSDocMitchellHouse`
  (`0x00103DF9`), 10 official base masters, isolated clean-room staging copy (read-only
  probe; zero writes to game install or Saves), `b3_jip4_02_session_time_and_start_angle`
  (`tools/capture/nvse/catalogs/wave4b_extenders.json`, #3679):
  `PROBE|b3_jip4_02_session_time_and_start_angle|sess=5.31,docZ=22.92,dynZ=270.00`; zero
  DLL, PDB, or third-party source consulted; hypothesis: X/Y axes and persistence of
  `made_rot` across saves].
- `GetIsPoisoned` [documented: "under the influence of a poison-type effect"]: returns `1`
  for active `SPEL` type-5 poisons (`RadScorpion1Poison`, `BarkScorpionPoison`,
  `CazadorPoison`; cleared to `0` by Antivenom dispel), `ALCH` poisons (`BleakVenom`),
  and hostile+detrimental `SPEL` type-0 effects (`SpellFlamerEffect`), and `0` for clean
  actors or beneficial ingestibles (`RadX`) [observed: 2026-11-25, `GSDocMitchellHouse`
  (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN NVSE 57.30 (5730),
  10 official base masters, read-only `PrintToConsole` probe
  `w6d12_38_getispoisoned_all_sources_and_antivenom_clear`
  (`tools/capture/nvse/catalogs/wave6d12_magic_effects_explosions_proj.json`):
  `clean=0|sp5(rad=1,bark=1,afterAnti=0,caz=1)|alchPois=1|sp0Flm=1|alchBen=0`; no DLL or
  source consulted]. Ignoring inactive (`!e.inactive`) and permanent (`!e.permanent`, e.g.,
  addictions) effects and scoping the non-`is_poison` Hostile+Detrimental check to `SPEL`
  sources (leaving mixed `ALCH` items governed by `Game::is_poison`) are NVForever choices
  [design].

## 11. Still open

The detection state 3 (needs a fight), GetEditorPosition (does not compile at run time,
§10), and GetIsPoisoned on a poisoned actor. The quest flag 5 rule and flag 7 are read
from five quests only.
