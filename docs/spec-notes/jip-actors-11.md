# JIP LN NVSE, batch 11: actors (2026-11-09, #1006)

Combat allies and targets, detection, packages, the actor's traits (alpha, gravity, light,
breath, movement, velocity, leveled list, unarmed effect), base actor values of a base
form, kill XP, radiation at an NPC, falls, active and temporary effects and effect lists,
the equipped items, armour DR and DT by condition and whether a hit got through, base
faction ranks, ClearDeadActors, the hardcore stages' effects and the weapon condition
penalty, in `crates/fnv-game/src/nvse_jip11.rs`. Sources: the GECK wiki export, each
function's page [documented: the page named]. The probe set `jip11`
(`tools/capture/jip_probes.py`, lab `L5.ext.jip11`) asked the original what the
[hypothesis] values below leave open; its answers are in §6.

40 functions; JIP 142 → 102 [observed: the extender_census test, 2026-11-09].

List getters with nothing to give return no array, not an empty one [observed: the jip10
probes, 2026-11-08, for JIP's list getters; hypothesis: the same here].

## 1. Combat, detection and packages

| Function | NVForever |
|---|---|
| GetCombatAllies | for an actor in combat, the others of its combat group (`Game::combat_group`: those fighting a foe it fights) [documented; hypothesis: the group is the game's allies] |
| GetCombatTargets | for an actor in combat, its targets now (`awareness` targets) [documented] |
| GetDetectedActors | the actors whose latest detection by it is in the Detected band, as GetDetected [documented; hypothesis: the same test] |
| GetDetectionValue `target` | the latest detection value of the pair, rounded [documented: int]; -100 for a pair not measured [documented: the value a left cell sets; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 with the 10 official base masters, clean-room script probe jip_33_detection_value_unmeasured (GetDetectionValue/GetDetectedActors/GetCombatAllies): -100,-1,-1; no DLL or source consulted] |
| ForceActorDetectionValue `[value] [asModifier]` | the calling actor's detection value by anyone is the value, or the value plus it as a modifier; none or 0 gives it back to the rules [documented]. Applied in `Game::detect`; the band follows the value |
| GetTargetUnreachable | 1 while a package the actor could not reach is set aside (`Game::block_package`) [hypothesis: NVForever's form of "unable to find a path"] |
| GetCurrentStablePackage | the package its list chose (`Game::current_package`) [documented] |
| GetInterruptPackage | 0 [decision: NVForever's interrupts (combat, dialogue, flight) are states, not package forms] |

## 2. The actor's traits

| Function | NVForever |
|---|---|
| GetActorAlpha | SetActorAlpha's alpha, 1 opaque [documented] |
| GetActorDiveBreath, SetActorDiveBreath `seconds` | the player's breath left, a full breath above water [observed: 10 on land]; the engine passes it each frame (`Game::sync_player_breath`) and takes a script's seconds back while under water, capped at the full breath [documented]; 0 for NPCs [decision: NVForever's NPCs do not drown] |
| GetActorGravityMult | SetActorGravityMult's multiplier; 0 for an actor no script set [observed: Doc Mitchell 0, against the page's "1.0 by default"] |
| GetActorLeveledList | the LVLN or LVLC the reference was placed from (NVForever keeps it as the base), else 0 [documented] |
| GetActorLightAmount | the light level at the actor, 0-100, as the last detection round measured it (`Situation::light`) [documented: the range]; 0 for an actor not measured [hypothesis] |
| GetActorMovementFlags | forward/backward/left/right 1/2/4/8, turning 16/32, walking 256, running 512, sneaking 1024, swimming 2048 [documented: the table]; jumping, flying, falling, sliding 0 [decision: not known game-side] |
| GetActorUnarmedEffect, SetActorUnarmedEffect `[effect] [baseForm]` | the EITM of the base supplying the actor's effects; the setter takes an object or actor effect [hypothesis: anything else refused], for the session; the engine's unarmed attacks read it (`Game::unarmed_effect`) |
| GetActorVelocityAlt `x y z [local]` | returns `1` and writes the velocity on world axes (`local=0` or omitted) or the actor's own axes (`local=1`, x right, y forward) into the three output script variables [documented; observed: 2026-11-24, Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 (`5730`), 10 official base masters, isolated clean-room staging copy (read-only `PrintToConsole` probe; zero writes to game install or Saves), `GSDocMitchellHouse` (`0x00103DF9`), committed probe `w6_t1_05_velocities_jip` (`tools/capture/nvse/catalogs/wave6_t1_jip_velocities.json`): `PROBE\|w6_t1_05_velocities_jip\|act0=1(0.0,0.0,0.0)\|act1=1(0.0,0.0,0.0)\|lin=0(111.0,222.0,333.0)\|linM=0(111,222,333)\|ang=0(111.0,222.0,333.0)\|angM=0(111,222,333)`; no DLL, PDB, or third-party source consulted]; the actor is followed from the first call, which reads 0 [hypothesis], as GetActorVelocity |
| GetBaseActorValueAlt `av [baseForm]` | the base value of the reference, or of an NPC base form without a calling ref, and `0.0` for a non-actor base form [observed: `jip_34_base_av_alt_unplaced_base` (`4.00,7.00`); Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `w4d_28_base_av_alt_and_faction_rank` (`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3679): `av(pl=5.0,docBase=4.0,nonAct=0.0)|fac(b0=-1->3,exist=-1->3,spawn=3)`; no DLL or source consulted]; NVForever resolves through its first placed reference, 0 for a base placed nowhere [design: NVForever computes base values on references] |
| GetKillXP | the XP a kill gives (`Game::kill_xp`), before the perks' adjustment [documented] |
| GetRadiationLevelAlt | rads a second after Rad Resistance: the player's as GetRadiationLevel, an NPC's from the radiation markers and zones at its place [documented: any NPC; hypothesis: no water for NPCs] |
| GetFallTimeRemaining | direct engine value (`Game::set_fall_time_remaining`), else kinematic estimate while airborne (`gaps.air_times > 0`) from `ray::cast`, velocity and gravity, 0 grounded [documented; hypothesis: `docs/spec-notes/nvse-stubs-persistent-fall-spread-unique.md`] |
| ResetFallTime | the engine measures the fall again from where the actor is (`nvf_play::falls`) [documented] |

## 3. Effects

| Function | NVForever |
|---|---|
| GetActiveEffects `[filter]` | the magic items with effects on the actor, once each, in the order they took; 1 object effects (ENCH), 2 actor effects (SPEL), 4 ingestibles (ALCH, INGR), summed [documented: the filter; observed: jip11; Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 with the 10 official base masters, clean-room script probe jip_35_active_effects_filter_forms (GetActiveEffects): -1,1,00015164; no DLL or source consulted] |
| RemoveNthEffect `item index` | the entry leaves the item's list (applying it and GetNumEffects skip it), for the session; the new count, -1 for no such entry or the last one [documented]. The index is the scripts' list's, AddNewEffect's entries included, as every Nth-effect function's (jip-last-14.md §1) [decision: #2016] |
| RemoveNthTempEffect `index` | the index-th of GetTempEffects' list is dispelled [documented] |

## 4. Equipped items and armour

| Function | NVForever |
|---|---|
| GetEquippedEx `list` | the first of the list's items equipped [documented] |
| GetEquippedWeaponPoison | 0 [decision: NVForever puts no poison on weapons] |
| IsEquippedWeaponScoped | 1 DNAM "Has Scope" without "Scope from Mod", 2 an Increase Zoom mod attached, 0 [documented] |
| IsEquippedWeaponSilenced | 1 VNAM Sound Level "Silent", 2 a Silence or Suppressor mod attached, 0 [documented; hypothesis: "silenced by default" is the sound level] |
| TestEquippedSlots `armor` | whether its BMDT biped slots overlap those of an armour the actor wears, including when testing the currently worn armour itself [documented; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-25, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated clean-room staging copy (read-only probe; zero writes to game install or Saves), `w6d2p3_52_empty_recipes_and_eff_dt_curve` (`tools/capture/nvse/catalogs/wave6d2_test_equipped_slots_same_armor.json`): `PROBE\|w6d2p3_52_empty_recipes_and_eff_dt_curve\|recE=0,-1\|dt10=9.00\|ov=1,1\|sil=0->1`; zero DLL, PDB, or third-party source consulted] |
| SetNoUnequip `flag` | on the inventory reference of an equipped item: the world's NoUnequip lock (saved with the world), or off [documented] |
| GetArmorEffectiveDR, GetArmorEffectiveDT | DNAM DR or DT times `1 − (0.5 − condition) × penalty` clamped at 0 below half condition (`Game::armor_condition_mult`: SetArmorConditionPenalty's penalty, else Stewie's `Tweaks::armor_condition_penalty`, default 1), of an inventory reference or a placed armour [documented: SetArmorConditionPenalty's formula; design: docs/design/tweaks.md] |
| GetArmourPenetrated | whether the actor's last hit's damage before armour was above its DT; the engine tells it before the hit is dealt (`Game::note_armour_hit`), so OnHit reads this hit's [documented] |
| SetConditionDamagePenalty `mult` | the weapon damage penalty below 75 % condition, `1 − (0.75 − condition) × mult` [documented]; the engine's damage rules (`Rules::condition_damage_penalty`) and `Game::weapon_condition_mult` (also honoring Stewie's `no_condition_damage_penalty` and `no_condition_damage_buffer` tweaks [design: docs/design/tweaks.md]) take it |

## 5. Bases, the dead and hardcore

| Function | NVForever |
|---|---|
| SetBaseFactionRank `faction rank [baseActor]` | the rank of the base supplying the factions, propagating immediately to existing placed references and newly spawned references (`PlaceAtMe`) as well as `GetBaseFactionRank`, and below 0 removes the faction (`GetBaseFactionRank` reads `-1`) [observed: `jip_36_base_faction_rank_neg_remove` (`-1,2,-1`); Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `w4d_28_base_av_alt_and_faction_rank` (`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3679): `av(pl=5.0,docBase=4.0,nonAct=0.0)|fac(b0=-1->3,exist=-1->3,spawn=3)`; no DLL or source consulted]; for the session [hypothesis: not saved] |
| ClearDeadActors `[keepRespawning]` | every dead actor in the loaded cells is disabled, except a quest-item base, one carrying a quest item, and (1) one that respawns [documented] |
| GetHardcoreStageEffect, SetHardcoreStageEffect `stage [effect]` | the stage record's actor effect; omitted `effect` clears to `0`, while non-`SPEL` forms and explicit null (`0`) are no-ops [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-25, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated clean-room staging copy (read-only probe; zero writes to game install or Saves), `w6d2_30_actor_unarmed_and_hardcore_stage_effect` (`tools/capture/nvse/catalogs/wave6d2_set_hardcore_stage_effect_validate.json`): `PROBE\|w6d2_30_actor_unarmed_and_hardcore_stage_effect\|u(0z=1,r=1/0/0,set=1,badEq=1,clrZ=0)\|hc(0z=0,set=1,badEq=1,badZ=0,nullZ=0,omitZ=1)`; zero DLL, PDB, or third-party source consulted]; the player gets the new one from the next change of stage [hypothesis] |

## 6. The original's answers (probe set jip11, 2026-11-09)

The observer session ran set `jip11` on the probe save (nvfairport): 28 rows, all compiled,
no crash; lab `L5.ext.jip11` 17 of 28 at 588937cc4 [observed]. Followed since:
GetActorGravityMult's 0 and GetActorDiveBreath's full breath on land. The probes of the
form getters now go through GetFormIDString (the collector and fnvtool wrote a form
differently). Confirmed: GetActiveEffects' elements are the magic items (an armour's
object effect first); out of combat the list getters give no array; GetInterruptPackage,
GetActorLeveledList and GetActorUnarmedEffect on Doc gave none, as ours. Not matched, and
why: the player's light (100 outdoors in the save; the candidate runs no detection
rounds), player→Doc detection -28 (likewise), the player's active effects (the save's
worn jumpsuit and Concussion), Doc's stable package (the save's schedule), the fall time
on the ground (-0.004, the original's own residue).

## 7. Follow-ups

- GetInterruptPackage: package forms for NVForever's combat, dialogue and flight states.
- GetFallTimeRemaining: wired core-side (`Game::fall_time_remaining`, `Game::set_fall_time_remaining`); engine physics can optionally push direct per-actor values when desired.
- GetEquippedWeaponPoison: poisons on weapons.
- GetActorMovementFlags: jumping, falling and sliding from the engine's character state.
- RemoveNthEffect: the HUD still sees the record's list (the Nth-effect functions share
  one list since #2016).
