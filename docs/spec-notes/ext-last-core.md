# The last core xNVSE functions (2026-10-30)

The xNVSE and FOSE functions left after the 2026-10-26 batches, and two JIP/LU getters the
lab asked for. Built in `crates/fnv-game/src/nvse_rest.rs`, `ui_functions.rs`,
`nvse_misc.rs`, `nvse_gaps.rs` and the script VM (`fnv-script/src/vm.rs`). Sources: the
GECK wiki export [documented: the page named], the lab's probe goldens [observed], and
the HUD captures in hud.md §14 [observed]. Changes last for the session unless an entry states a saved lifetime.

## 1. Setters whose getters were built

| Function | What it changes | Read back by |
|---|---|---|
| SetEyes `eyes` | the NPC base's eyes (ENAM), so every reference of it; 1 when set [documented: "Returns 1 for success", "affects their baseform"] | GetEyes |
| SetHairLength `length [npc]` | the NPC base's LNAM [documented] | GetHairLength |
| SetNPCWeight `weight [npc]` | the NPC base's NAM7 [documented] | GetNPCWeight |
| SetActorBaseFlagsHigh `flags [npc]` | the ACBS flags' high 16 bits, the low ones kept, reaching `fnv-game` actor-flag consumers (§10) [documented; observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 with the 10 official base masters, clean-room script probe `b3_core_01_hair_weight_flags_high` (`tools/capture/nvse/catalogs/wave4b_extenders.json`, #3678 / PR #3679), round-tripping `SetHairLength 0.75`, `SetNPCWeight 0.42`, and `SetActorBaseFlagsHigh (SetBit fh0 0)` on Doc Mitchell base `0x00104C0C`: `hair=0.29->0.75|wt=0.00->0.42|flH=0->1`; no DLL or source consulted] | GetActorBaseFlagsHigh |
| SetOwningFactionRequiredRank `rank` | the rank a reference's owning faction needs (XRNK) [documented; observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 with the 10 official base masters, clean-room script probe `core_26_owning_faction_req_rank` (`tools/capture/nvse/catalogs/wave4a_extenders.json`, #3678 / PR #3679), calling `SetOwningFactionRequiredRank 3` and reading `GetOwningFactionRequiredRank` then restoring on door `0x00103E61`: `0,3`; no DLL or source consulted], carried across same-run loads in `Game::Session` and not saved [hypothesis] | GetOwningFactionRequiredRank, `Game::ownership`, IsOwned, HasOwnership |
| SetPackageTargetDistance `package value` (alias SetPackageTargetCount) | the package target's count or distance, for every reader, the AI too (`Game::package_def`) [documented]; authored package edits (`SetPackageFlag`, `SetPackageLocation*`, `SetPackageTarget*`) are carried across same-run loads in `Game::Session` and not saved [hypothesis] | GetPackageTargetDistance |
| SetRefCount `count` | a placed stack's count [documented]; an inventory reference's stack is left alone [hypothesis] | GetRefCount |
| SetCurrentQuest `quest` | the active quest, "identical to ForceActiveQuest" [documented] | GetCurrentQuest |
| SetStringGameSettingEx `"Name\|Value"` | a string game setting, as SetStringSetting [documented; the page calls it broken and its replacement working] | GetStringGameSetting |
| SetAltControl `control button` | the control's mouse button, as SetControl with the mouse; not written to the INI [documented: "saved to the ini"; decision: NVForever never writes the game's INI files] | GetAltControl |

## 2. Input and menus

- MenuHoldKey / MenuReleaseKey (`mhk`, `mrk`) hold and release a key in menu mode
  [documented]; they share the keys HoldKey holds [hypothesis: the menus read the same
  held keys].
- ShowLevelUpMenu opens the level-up menu for the player's level now, without raising it
  [documented: "does NOT cause an increase in level"].
- GetUIFloatInherited `path [ExcludeMenuRoot]`: `x`, `y` and `depth` with the values of
  every parent tile whose `locus` is on added, up to the menu's root, which
  ExcludeMenuRoot leaves out; a tile without the trait counts 0; any other trait as
  GetUIFloatAlt [documented: GetUIFloatInherited; observed: hud.md §14, the page's example
  gives -50, and 0 with ExcludeMenuRoot, though `radiation_text` has no `depth` in the
  file]. Whether a parent without `locus` stops the sum is open [hypothesis: it does not].
- GetFontFile `n`: fonts 1-8 and 10-89 [documented], the INI's `sFontFile_N` (category
  Fonts), else Fallout_default.ini's names for 1-8 [documented: Fallout_default.ini]; 9
  and fonts nothing names give "" [observed: jip-forms fontfile.2, fontfile.9].
- GetNthPerkEntryString `perk n`: an activate choice's button label (EPF2) [documented:
  "Activate Prompt" for type 4]; "" for any other entry [observed: jip-forms perkentry.0].

## 3. Scripts and debugging

- CompileScript `path [forceRecompile]` (alias GetUDFFromFile): the text file
  `Data\NVSE\user_defined_functions\<path>` compiled into a script that Call runs like a
  UDF, cached by path unless forceRecompile [documented]. Its id is NVForever's own, above
  every plugin's FormIDs [decision, as the lambdas', script-compiler.md §9]. A missing
  file or a compile error returns `00000000` (no form) [observed: Fallout: New Vegas
  1.4.0.525 + xNVSE 6.4.9, 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), 10 official
  base masters, read-only `PrintToConsole` probes
  `core_10_compilescript_missing_and_escape`
  (`tools/capture/nvse/catalogs/wave4a_extenders.json`, #3679):
  `PROBE|core_10_compilescript_missing_and_escape|00000000,00000000` and
  `b3_core_ex_02b_compilescript_real_err`
  (`tools/capture/nvse/catalogs/wave4b_extenders.json`, #3679):
  `PROBE|b3_core_ex_02b_compilescript_real_err|ref=00000000,valid=0`; no DLL or source
  consulted].
- PrintVar / DebugPrintVar (`PrintV`, `DPrintV`) print each variable as `name: value` on a
  line of its own, up to 15; DebugPrintVar only with the calling mod's debug mode on
  [documented: PrintVar, "fMyVariable: 2.5" and "aMyVariable: [2, 4]"; DebugPrintVar]. A
  map shows `key: value` pairs in the brackets [hypothesis].
- ToString `value`: converts a number, string, or form to text [documented: ToString],
  and returns `"NULL"` for list arrays (`ar_List 1 2`) and stringmaps (`ar_Map "k"::5`)
  while keeping `ToString 42` as `"42"` [observed: Fallout: New Vegas 1.4.0.525 +
  xNVSE 6.4.9, 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters,
  read-only `PrintToConsole` probe (zero writes to game install or Saves),
  `b3_core_05_tostring_array_map` (`tools/capture/nvse/catalogs/wave4b_extenders.json`,
  #3679): `PROBE|b3_core_05_tostring_array_map|L=[NULL]|M=[NULL]|N=[42]`; zero DLL, PDB,
  or third-party source consulted]. Returning `"NULL"` for other `XVal::Array(_)` variants
  (such as numeric maps or unset array ID `0`) in `ToString` while leaving unary `$expr`,
  `PrintVar`, `ArrayToString`, `Ar_Dump`, and `Sv_Join` on `Vm::text_of` stays
  [hypothesis].
- PrintF, PrintDebugF, Ar_DumpF, DecompileScript, DumpDocs and DumpCommandWikiDoc write
  files (the Fallout folder, DecompiledScripts, nvse.log) [documented]; NVForever writes
  none of them and they do nothing else [decision: NVForever writes nothing beside the
  game, docs/design/install-layout.md]. DecompileScript gives an empty array.

## 4. Found by probing the original (2026-10-30, #1021, #1025)

Probe sets `rules1` and `rules2` (`tools/capture/jip_probes.py`, run from the owner's save
nvfairport; goldens private). They read to eight or ten decimals, more than the lab's sets.

- **SetNthPackage** `ref.SetNthPackage package index` (the page gives no arguments): the
  package at the index of the actor's base list is replaced, for the AI too, and the result
  is the package it replaced; an index past the end changes nothing [observed: Doc's 9
  packages, index 0 15ED62 became 10B756 and back, the count stayed 9; index 99 left 9;
  the result 15ED62]. Only `let x := ref.SetNthPackage p i` compiled in the game; the
  statement forms did not. The catalogue takes its parameters from the generator's
  observed table (`OBSERVED_PARAMS`, tools/capture/script_functions.py).
- **FATan / FATan2** are the game's own approximation, not the exact angle: an odd
  polynomial of degree 11 in 32-bit floats of min(|y|,|x|)/max(|y|,|x|), then π/2 less it
  when |y| is the larger, π less it when x is negative, negated when y is negative; π and
  π/2 are 32-bit floats (FATan2 1 0 = 90.0000025). The coefficients are a least-squares fit
  of the 21 readings in (0, 1] (residual 4.6e-8 radians, the 32-bit noise there), so
  NVForever matches every reading within 0.000014° and at six significant digits, not to
  the last decimal [observed: 29 readings; `fast_atan2` in fnv-script `vm/jip.rs`].
- **GetRunSpeed** (JohnnyGuitar) reads 338.8 for the player and for Doc Mitchell, and kept
  338.8 while Speed Mult (50), Agility (10), `fMoveRunMult` (5) and `fMoveBaseSpeed` (100)
  were changed one at a time, and while sneaking [observed: rules2]. 338.8 is
  `fMoveBaseSpeed` 77 × 4.4, and no human run clip moves at it (`fnvtool actor
  locomotion`); where the number comes from is open [hypothesis: worked out when the actor
  loads, so live setting changes do not reach it]. Not built.
- **The reference walk in Doc Mitchell's house**: the one reference the game counts only
  with taken references included is `VCG01CasualHardcoreMessageREF` (0011F293), a
  persistent reference the opening quest's state in the owner's save has taken away
  [observed: rules1, both lists]. A new game has it, so the count belongs to the save
  (#1023), not to a counting rule. The game lists the cell's references in its own order,
  not by FormID [observed]; NVForever's order is FormID order.

## 5. PrintActiveTile

The highlighted tile's name, printed on the console [documented: PrintActiveTile, "Prints
name of highlighted UI component"]; nothing with none [hypothesis]. The highlighted tile
is the tile tree's pointer's (`Game::ui_pointer`, #957), as GetActiveUIComponentName reads
it. The console's exact line is not documented: the name alone [hypothesis].

## 6. Not a function

- `eval` is the if-statement form (ext-syntax.md), built in the compiler; the census
  counts it only because it calls every catalogue name as a function.

## 7. Critical weapon damage in hit calculation

[documented: https://fose.silverlock.org/fose_command_doc.html#SetWeaponCritDamage;
`schemas/script/functions.json`, SetWeaponCritDamage] `SetWeaponCritDamage amount
[weapon]` changes the weapon base's critical hit damage. The first argument is an
integer. An omitted weapon selects the calling weapon reference's base form, as
the matching getter does [documented:
https://geckwiki.com/index.php?title=GetWeaponCritDamage].

[design] The effective critical damage must reach the existing melee and projectile
hit calculations, for player and NPC attacks alike. The getter and combat read the
same session override; no second setter store or extra per-frame weapon scan is
needed. The existing setter's receiver and argument admission are retained.
[hypothesis] That the original game's hits use the changed value on every one of
these paths (player and NPC melee, player and NPC projectiles) is inferred from
the documented base-form change, not observed.

[documented: `schemas/fnv/objects.json`, WEAP CRDT] The accepted data model holds
critical damage in an unsigned 16-bit field. Negative integers and values above `65535`
wrap modulo `65536` into that unsigned 16-bit field [observed: 2026-11-23 & 2026-11-24,
`GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 with the
10 official base masters, clean-room script probes `core_16_set_weapon_crit_damage_edge`
(`tools/capture/nvse/catalogs/wave4a_extenders.json`, #3679: `22.0,77.0,65531.0` on
`Weap10mmPistol` — initial `22.0`, after `SetWeaponCritDamage 77` a `TempCloneForm`
clone read `77.0`, and `SetWeaponCritDamage -5` read back `65531.0`) and
`w4c_b2_11_crit_dmg_bounds` (`tools/capture/nvse/catalogs/wave4c_extenders.json`, #3679:
`c0=22,neg=65531,frac=12,over=4464,aliasOp=5259` — `-5 -> 65531` and `70000 -> 4464`); no
DLL or source consulted]. [hypothesis] Fractional truncation (`12.7 -> 12`) remains a
hypothesis because `w4c_b2_11_crit_dmg_bounds` stored the fractional readback in an `int`
variable rather than a `float`. [design] `SetWeaponCritDamage` stores finite numeric
inputs truncated toward zero and wrapped into `u16` (`0..=65535`, non-finite as `0`) so
both script readback and combat consume the same unsigned 16-bit value. [hypothesis]
Non-numeric conversion remains uncaptured. The accepted generic setter's explicit invalid
weapon-argument fallback remains unchanged pending behavioral evidence.

[observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas
1.4.0.525 + xNVSE 6.4.9 with the 10 official base masters, clean-room black-box
script probes `b3_core_02_setcritdam_alias_check` (`GetWeaponCritDamage rW` ->
`SetCritDam 55 rW` -> `22.0 -> 55.0` on `Weap10mmPistol` `0x0000434F`) and
`w1_2704_setcritdam_alias` (`SetCritDam 77 r10mm` -> `22 -> 77`; swapped
`SetCritDam r10mm 77` -> `COMPILE_FAIL`; `Player.SetCritDam 88` -> `22 -> 88`;
and with `Player.SetAV CritChance 100`, live 10mm critical shots at baseline
`critDmg=22` -> `GetHitBaseWeaponDamage = 44.000` vs `SetCritDam 200 r10mm` ->
`222.000`, `+178`; reproducible via `#3678` / PR `#3679`, `tools/capture/nvse/`);
no DLL or source consulted] `SetCritDam` is a live alias for `SetWeaponCritDamage`
with identical argument order (`[ref.]SetCritDam <int> [weapon]`). `SetCritDam` is
registered as an unnumbered alias of `SetWeaponCritDamage` in
`crates/fnv-script/data/encodings.json` (`compiler.aliases`, `#2704`);
wiki-generated `schemas/script/functions.json` is left untouched. [hypothesis]
Native wire/opcode encoding remains uncaptured; this note does not import FOSE
opcode numbers into New Vegas.

[hypothesis] `SetWeaponCritDamage` edits are session-only (not saved in `.fos` or `.nvse`
cosaves), persist across same-process save loads, revert to the ESM value on a fresh
process restart, and stay isolated on a `TempCloneForm` clone after clone creation so
later edits to the source weapon do not leak into the clone (initial inheritance on
`TempCloneForm` is observed in `core_16_set_weapon_crit_damage_edge` above, while
subsequent source-edit isolation and same-run save/reload persistence await a committed
reproducible observation or manual save/load write-up). [design] Loaded base-form
`CRDT/critical_damage` overrides carry across same-run loads via `Game::take_session` /
`Game::put_session` (excluded from `GameSnapshot` and excluding transient `TempCloneForm`
clones), and `TempCloneForm` snapshots the source weapon's effective
`CRDT/critical_damage` onto the clone (`Game::snapshot_weapon_crit_damage`) for
bi-directional isolation. Core source execution, engine hit calculation, native captures
and real installed-mod acceptance are separate validation states.

## 8. Player swimming and Hardcore achievement eligibility

[documented: https://geckwiki.com/index.php?title=IsPlayerSwimming]
`IsPlayerSwimming` takes no arguments and returns whether the player is swimming.
It is global: the calling object's motion does not select its result. The existing
source handler reads the player's `Game::motion` state. [design] The engine keeps
that state supplied from the player's physics character; it must not substitute
water contact, height or NPC motion. Vanilla `IsSwimming` keeps its documented
player-zero behavior.

[documented: `schemas/script/functions.json`, DisableHardcoreTracking]
`DisableHardcoreTracking` (alias `babby`) permanently invalidates the playthrough's
always-Hardcore eligibility. [design] It sets the existing saved
`PlayerState::hardcore_was_off` field, consumed by `IsAlwaysHardcore`; repeated calls
are idempotent. The eligibility state survives a serialized snapshot and cannot be
restored by calling `SetHardcore 1` (the documented "permanently").
`DisableHardcoreTracking` returns 0 and clears `IsAlwaysHardcore` (`1 -> 0`) [observed:
2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE
6.4.9 with the 10 official base masters, clean-room script probe
`core_17_disable_hardcore_tracking_ret` (`tools/capture/nvse/catalogs/wave4a_extenders.json`,
#3678 / PR #3679), reading `IsAlwaysHardcore`, calling `DisableHardcoreTracking`, and
re-reading `IsAlwaysHardcore`: `1,0,0`; no DLL or source consulted]. [hypothesis] It does
not turn off Hardcore mode or toggle needs accumulation; the documentation describes only
the eligibility flag.
This is separate from `ToggleHardcoreTracking` and `GetHardcoreTracking`, whose
existing needs-tracking switch is unchanged.

[hypothesis] Receiver quirks, exact native return values and native command/condition
encoding remain capture work. Source compiler support is exercised independently
of numeric bytecode admission. Native command identity is insufficient evidence for
an argument decoder or CTDA mapping.

## 9. SetCombatDisabled / GetCombatDisabled (`nvse_gaps.rs`)

[documented: https://geckwiki.com/index.php?title=SetCombatDisabled]
`actor.SetCombatDisabled 0/1` disables or re-enables combat for the calling actor:
a combat-disabled actor never enters combat with other actors (though others may
still attack it, and `UseWeapon` / `Use Weapon` packages are unaffected), and the
state resets on every saved-game load (kept out of both `GameSnapshot` and
`Game::Session`). `SetCombatDisabled` returns 0 [observed: gaps2] and
`GetCombatDisabled` reads the flag [observed: gaps3]. Enabling `SetCombatDisabled 1` on an
actor already in combat leaves `IsInCombat == 1` in the same frame (`t0`) and ends active
combat on subsequent AI ticks (`IsInCombat == 0` by `t2` and `t5`) [observed: Fallout: New
Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-23 & 2026-11-24, `GSDocMitchellHouse`
(`0x00103DF9`), 10 official base masters, isolated clean-room staging copy (read-only
`PrintToConsole` probes; zero writes to game install or Saves),
`core_20_set_combat_disabled_stops_combat` (`tools/capture/nvse/catalogs/wave4a_extenders.json`,
#3679: `StartCombat` -> `IsInCombat 1` -> `SetCombatDisabled 1` -> `GetCombatDisabled 1`,
`IsInCombat 1`: `1,1,1`) and `w4h_05_set_combat_disabled_multitick`
(`tools/capture/nvse/catalogs/wave4h_reconcile.json`, PR #3803 / #3802):
`PROBE|w4h_05_set_combat_disabled_multitick|pre=1|t0(cd=1,inCmb=1)|t2(cd=1,inCmb=0)|t5(cd=1,inCmb=0)`;
zero DLL, PDB, or third-party source consulted]. NVForever matches this by setting
`combat_disabled` without calling `Game::stop_combat` immediately (preserving same-frame
`IsInCombat == 1`) and clearing active combat to `State::Normal` on the next AI awareness
tick in `Game::settle_awareness` (`awareness.rs`). Calling `SetCombatDisabled 1`
on a non-combat actor suppresses new combat entry via `StartCombat` (`IsInCombat` remains
`0` same-tick and on subsequent ticks) [observed: Fallout: New Vegas 1.4.0.525 + xNVSE
6.4.9 + JIP LN 57.30, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base
masters, `w4d_07_setcombatdisabled_new_combat`
(`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3679):
`c0=0,cd(0->1->1),sameTick=0,nextTick=0`; no DLL or source consulted].
[hypothesis] Exact tick latency (`t1` vs `t2`) and whether `SetCombatDisabled 1` also
suppresses ally/provocation alarms (`awareness.rs`, `combat_group.rs`, `provocation.rs`)
remain uncaptured.

## 10. Live `SetActorBaseFlagsLow` / `SetActorBaseFlagsHigh` consumers (`actor_values.rs`)

[documented: https://geckwiki.com/index.php?title=SetActorBaseFlagsLow,
https://geckwiki.com/index.php?title=SetActorBaseFlagsHigh,
https://geckwiki.com/index.php?title=GetActorBaseFlagsLow,
https://geckwiki.com/index.php?title=GetActorBaseFlagsHigh,
https://geckwiki.com/index.php?title=ActorHasBaseFlag; `schemas/fnv/gameplay.json`
NPC_ and CREA ACBS]
`SetActorBaseFlagsLow` and `SetActorBaseFlagsHigh` update the low and high 16-bit
halves of an actor base form's 32-bit `ACBS` flags for the current session while
preserving the opposite half. Record-type-specific bits stay guarded by the resolved
actor record signature (`NPC_` vs `CREA`): for example, bit 0 is `Female` on `NPC_`
(`Biped` on `CREA`), bit 4 is `Auto-calc stats` on `NPC_` (`Swims` on `CREA`), bit 23
is `Autocalc Service` on `NPC_` (`Immobile` on `CREA`), and bits 15–18, 21–22, 28–29,
and 31 apply to `CREA` only.

[design] Session overrides are indexed in `OtherState::acbs_flags` (`HashMap<FormId,
u32>`) alongside `field_overrides` (`"ACBS/flags"`) so hot gameplay consumers perform
an O(1) zero-allocation check (and an empty-map fast path when no overrides are set),
while `GetActorBaseFlagsLow`, `GetActorBaseFlagsHigh`, `ActorHasBaseFlag`, and
`GetIsImmobile` read the same override. Every existing `fnv-game` ACBS flag consumer
now checks the resolved template source's live override before falling back to
authored `ActorDef` fields: `can_knock_down` (`No Knockdowns`, `Immobile`),
`no_vats_melee` (`No VATS Melee`), `no_blood` (`No Blood Spray`/`Decal`),
`allows_pc_dialogue` (`Allow PC Dialogue`), `cant_open_doors` (`Can't Open Doors`),
`keeps_out_of_water` (`Walks`/`Flies`/`Swims`), `no_combat_in_water`
(`No Combat in Water`), `base_is_ghost` (`Is Ghost`), `is_fixture` (`Immobile`),
`actor_level` (`PC Level Mult`), `max_health` (`Auto-calc stats`),
`no_low_level_processing` (`No Low Level Processing`), `is_immune` (`Invulnerable`),
`essential_flagged` (`Essential`, below saved `SetEssential` overrides), `respawns`
(`Respawn`), `pickpocket_refusal` (`Allow Pickpocket`), `services`
(`Autocalc Service`), `vats_untargetable` (`Flies`), `actor_female` (`Female`, below
explicit `SetFemale`/character-creator look overrides), and `runtime_minor_flags`
(`Not Pushable`, `No Rotating To Head-track`, `No Head`, `No Right Arm`,
`No Left Arm`).

[design] For placed actor references (including `LVLN`/`LVLC` leveled actors),
`Get/SetActorBaseFlagsLow/High` and `GetIsImmobile` resolve the spawned `NPC_`/`CREA`
base via `template_source(..., |_| false)` without following template links, while
each consumer retains its existing template-flag inheritance policy (`No Knockdowns`
and `No VATS Melee` stay on the actor's own record; Base Data, Model/Animation, Stats,
AI Data, and Traits flags follow their respective template flags). [hypothesis] That
every consumer path reacts immediately in the original game without reloading the
reference or cell, and the exact precedence when `SetEssential` or `SetFemale` has
already run on the same actor, remain uncaptured; overrides retain the module's
session-only lifetime (not saved in `GameSnapshot`).

## 11. Live `ARMO` flag, slot, trait, and repair-list overrides (`armor_overrides.rs`)

[documented: https://geckwiki.com/index.php?title=SetEquipmentSlotsMask,
https://geckwiki.com/index.php?title=SetEquipmentBipedMask,
https://geckwiki.com/index.php?title=SetArmorClass,
https://geckwiki.com/index.php?title=SetIsPowerArmor,
https://geckwiki.com/index.php?title=SetIsPlayable,
https://geckwiki.com/index.php?title=SetArmorDT,
https://geckwiki.com/index.php?title=SetArmorArmorRating,
https://geckwiki.com/index.php?title=SetRepairList,
https://geckwiki.com/index.php?title=TempCloneForm; `schemas/fnv/objects.json` ARMO BMDT/DNAM/DATA]
`ARMO` `BMDT/general_flags` encodes Medium (bit 3), Power Armor (bit 5), Non-Playable
(bit 6), and Heavy (bit 7) in one byte; `BMDT/biped_flags` encodes the occupied biped
slot mask; `DNAM/dt` stores Damage Threshold; `DNAM/dr` stores Damage Resistance × 100
(so `SetArmorArmorRating 2000` sets 20 DR); `DATA/weight` and `DATA/health` store
weight and full condition health; and `REPL` names the repair `FLST`.

[design] Session overrides written by `SetEquipmentSlotsMask`, `SetEquipmentBipedMask`,
`SetArmorClass`, `SetIsPowerArmor`, `SetIsPlayable`, `LNSetIsPlayable`, `SetArmorDT`,
`SetArmorArmorRating`/`SetArmorARAlt`, `SetWeight`, and `SetObjectHealth` stay
synchronized across `field_overrides`, `OtherState::power_armor`,
`Game::playable_overrides`, and `fnv_sim::World`'s session-only `ItemOverride` map
(never serialized in `WorldSnapshot` or `GameSnapshot`), while `SetRepairList` flows
through `Game::effective_repair_list_form` into `functions::repair_list` and
`Game::player_repair_list`. Consumers across `fnv-sim` (`equip_at`, `equip_check`,
`default_choice`, `outfit`, `carried_weight`, `encumbrance`, `stays`), `fnv-game`
(`worn_armor_class`, `armor_speed_mult`, `body_armor`, `wears_power_armor`, `jury_kind`,
`get_armor_rating`, `GetCalculatedItemValue`, `GetBaseEquippedWeight`/`GetCalculatedEquippedWeight`,
`GetEquippedItems`, `GetAllItemRefs`, `RemoveAllItems`, `TestEquippedSlots`,
`GetArmorEffectiveDT`/`DR`, `IsPowerArmor`, `IsPlayable`, `LNIsPlayable`,
`GetEquipmentSlotsMask`/`GetEquipmentBipedMask`), and `fnv-combat`
(`CombatForms::armor_stats_with`, `armor_struck_with`, `armor_values_worn_with_lookup`,
`item_health_with`) read the same effective values.

[design] `TempCloneForm` on an `ARMO` snapshots the source armor's current live
overrides onto the clone (`Game::snapshot_armor_overrides`) and registers the clone in
`World::add_clone_item`, so cloned armor can be equipped, evaluated, and edited
without leaking changes back to the original or inheriting later edits to the original
[observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 +
xNVSE 6.4.9 + JIP LN 57.30 with the 10 official base masters, clean-room script probe
`core_29_armo_tempclone_override_isolation` (`tools/capture/nvse/catalogs/wave4a_extenders.json`,
#3678 / PR #3679), setting `SetArmorDT 25` on `0x00020423`, cloning via `TempCloneForm`,
changing source DT to `5`, and reading clone DT before and after: `6.0,25.0,25.0`; no DLL
or source consulted].
[hypothesis] Whether every original-game consumer (including worn armor speed, Jury
Rigging, and already-equipped Power Armor training checks) re-evaluates immediately
without re-equipping or reloading the cell remains uncaptured; overrides retain the
module's session-only lifetime.
