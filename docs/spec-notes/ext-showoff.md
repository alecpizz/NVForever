# ShowOff events, explosion damage, aux timer stops and the jam animation

Plain-language notes for `crates/fnv-game/src/nvse_showoff.rs`. Sources: the GECK wiki
pages of each function [documented] and the showoff1 probe set run in the original game on
2026-10-27 [observed] (`tools/capture/jip_probes.py`; goldens stay private). The pages of
the ShowOff events themselves are not in the wiki dump, so their arguments come only from
showoff1: each event had one handler taking no arguments and one taking two refs, and both
ran.

## 1. ShowOff:OnAdd and GetAddedItemRefShowOff

- The event fires when an item reaches a container. With a stimpak AddItem'd to the player,
  a two-argument handler got the item's base form (type 47) first and the player second; a
  handler taking no arguments ran as well [observed].
- Inside the handler, the calling reference (`GetSelfAlt` / `ThisAlt`) is the physical
  world item reference when the item was picked up directly from the game world, and `0`
  (null) when added via `AddItem` or an inventory transfer [documented: GECK wiki
  [ShowOff:OnAdd](https://geckwiki.com/index.php?title=ShowOff:OnAdd), "May be null if a
  world item wasn't picked up directly from the environment"; no ShowOff DLL or source
  consulted].
- Inside the handler, `GetAddedItemRefShowOff` gives a reference whose base is the stimpak
  [observed: IsReference 1, base type 47]; the page says a world item reference or an
  inventory reference [documented: ShowOff:OnAdd, "Contains either a world item ref or an
  InventoryRef"]. NVForever gives the picked-up world reference on direct world pickup
  (`Game::run_default_activation` `Outcome::PickedUp`) and an inventory reference to the
  item in the container otherwise. Outside a handler it gives none.
- NVForever fires it wherever its script OnAdd block would run for an item reaching a
  container (AddItem and its variants, RemoveAllItems into a container, picking up).

## 2. ShowOff:OnExplosionHit and the explosion's damage

- The event fires when an explosion's blast reaches an actor, before the damage lands.
  - Earlier capture (`showoff1`): a MineFragExplosion (`FalloutNV.esm` `0x0007078B`)
    placed at the player gave a two-argument handler the player first and none second
    [observed: showoff1].
  - Clean-room probe `[observed 2026-11-24: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 +
    ShowOffNVSE 184 + JIP LN 57.30, 10 official base masters, GSDocMitchellHouse
    (0x00103DF9), isolated staging copy, committed probe w6t2_01_showoff_on_explosion_hit
    (tools/capture/nvse/catalogs/wave6c_showoff_on_explosion_hit.json):
    Player.PlaceAtMe 0x0007078B 1 ->
    PROBE|w6t2_01_showoff_on_explosion_hit|reg=1,n=1,selfIsP=0,tSelf=81,tBase=81,base=0007078B,hitIsP=1,src=00000014,dmg=100.0;
    no DLL or source consulted]`: the calling reference (`GetSelfAlt`) is the live
    explosion reference (type 81 `EXPL`, `GetBaseObject` = `0x0007078B`, not the hit
    actor), argument 1 is the hit actor (`Player`), and argument 2 is the source actor
    (`Player` `0x00000014` when placed on the player; `0` when there is no source actor
    [observed: showoff1]).
- `GetExplosionHitDamage` read 100 inside the handler; after `SetExplosionHitDamage 5` the
  player lost 5 Health [observed]. Both are unusable outside the event [documented].
  Inside a `ShowOff:OnExplosionHit` handler, `GetExplosionHitDamage` reads pre-armor damage
  (`100.0` even with `DT = 50`, where the player loses `75.0` Health, and
  `SetExplosionHitDamage 30` yields `9.0` Health loss after armor DT), with `rad=192.0`
  and `srcP=0`, while outside the handler `GetExplosionHitDamage` returns **`-1.0`**
  [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, ShowOffNVSE 184,
  `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters,
  `tools/capture/nvse/catalogs/wave6d1_get_explosion_hit_damage_outside.json` probe
  `w6d1_18_on_explosion_hit_armor_dt`:
  `PROBE|w6d1_18_on_explosion_hit_armor_dt|dt50(dmg=100.0,loss=75.0)|set30(dmg=100.0,loss=9.0)|rad=192.0,srcP=0,out=-1.0`;
  no binary, DLL, PDB, or third-party source consulted]. Outside the handler, NVForever
  ignores `SetExplosionHitDamage` [documented: unusable outside the event; design: no-op].
- NVForever asks for the damage in the engine's blast loop (`Game::explosion_hit_with_context`,
  passing the live `expl_ref` as calling reference and `cause.map(|c| c.actor).or(source)` as
  the source actor argument, including the caller ref forwarded by `PlaceAtMe` via
  `CombatRequest::Explode { source }` without fabricating a weapon `Cause`), before armour is
  taken off, and deals what the handlers leave, matching the observed pre-armor DT behavior.

## 3. SetOnAuxTimerStopHandler

- `SetOnAuxTimerStopHandler 1/0 handler timerName [owner]` adds or removes a handler for an
  aux timer of an owner, the player when none is given; the handler gets the timer's name
  and its owner [documented].
- It runs when the timer runs out: a 0.1 second timer had run out within 30 frames, its time
  left read -1 (gone) and the handler had run once, with the player as owner [observed]. It
  runs again on `AuxTimerStop name 1` [observed]; AuxTimerStop fires the event only when its
  fireEvent argument is non-0 [documented].
- Registration and removal via `SetOnAuxTimerStopHandler` take effect on the next frame
  (stopping in the same frame as registration does not fire; stopping on the next frame
  fires; stopping in the same frame as removal still fires), and both Start and Stop
  handlers execute with null calling reference (`GetSelfAlt == 0`) [observed: 2026-11-24,
  Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, ShowOffNVSE 184, `GSDocMitchellHouse`
  (`0x00103DF9`), 10 official base masters,
  `tools/capture/nvse/catalogs/wave6d1_aux_timer_stop_deferred_and_self.json` probes
  `w6d1p3_27_aux_timer_stop` (`same=0,noev=0,next=1,remSame=1|nm=[sNm],ownP=1,s0=1,sP=0`)
  and `w6d1p3_28_aux_timer_start_and_clear` (`st(n=1,nm=[sNm],s0=1,sP=0)`); no binary,
  DLL, PDB, or third-party source consulted]. Update handlers likewise pass `None` as the
  calling reference [hypothesis].
- NVForever checks the running timers each frame and erases and reports the ones whose time
  is up. Before this, a timer whose time was up stayed with 0 seconds left.

## 4. ForceWeaponJamAnim

- It returned 0 with the 10mm pistol away and 1 with it drawn [observed: showoff1].
  `IsAnimPlayingEx 4` (reload) never read 1 over the 60 frames after [observed: showoff1],
  so the jam is not a reload as that function counts them.
- A weapon without a jam animation gives 0 [documented: GECK wiki
  [ForceWeaponJamAnim](https://geckwiki.com/index.php?title=ForceWeaponJamAnim), the .357
  Magnum Revolver cannot jam], and calling it interrupts the actor's current hand action
  (such as canceling an active reload and replacing it with the jam animation)
  [documented: ForceWeaponJamAnim].
- Live probes `[observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE Plugin
  184, 2026-11-23/24, GSDocMitchellHouse (0x00103DF9), 10 official base masters, clean-room
  script probes w1_3632_onfireweapon_and_forcejam, so_34_force_weapon_jam_revolver, and
  w4d_08_forcejam_melee_god_nonactor (tools/capture/nvse/catalogs/wave4d_extenders.json,
  #3679); no DLL or source consulted]` confirmed:
  - Drawn 10mm pistol (`Weap10mmPistol` `0x0000434F`, `jam_time = 1.4`) returns **`1`**
    both normally and under `ToggleGodMode` (`w4d_08_forcejam_melee_god_nonactor`:
    `tgm10mm(ret=1,act=9)`); immediately afterward, `GetAnimAction` reads **`9`** (not
    `13`), `IsAnimPlayingEx 4` remains **`0`**, and calling `ForceWeaponJamAnim` again
    while already jamming returns **`1`** (`w1_3632_onfireweapon_and_forcejam`:
    `Player.ForceWeaponJamAnim` with holstered/drawn 10mm, Baseball Bat `0x0000421C`, and
    unarmed fists; `ForceJamAnim` is the wiki-documented alias [documented: GECK wiki
    ForceWeaponJamAnim]).
  - Drawn `.357 Magnum Revolver` (`WeapNV357Revolver` `0x0008F216`, `jam_time = 0.0`,
    `so_34_force_weapon_jam_revolver`: `AddItem` + `EquipItem` + draw `.357` vs `10mm`),
    holstered 10mm pistol, drawn Baseball Bat (melee, `w4d_08_forcejam_melee_god_nonactor`:
    `melee(ret=0,act=0)`), drawn Fists (unarmed), and non-actor static references (door
    `0x103E61`, `w4d_08_forcejam_melee_god_nonactor`: `door=0`) return **`0`**.
- NVForever checks that the actor is alive, has its weapon drawn, and has an equipped gun
  (`DNAM/animation_type` in `3..=9`) with finite `DNAM/jam_time > 0` (`GetWeaponAnimJamTime`;
  honoring session overrides and `TempCloneForm` clones) [observed: FalloutNV.esm WEAP
  DNAM `Weap10mmPistol` `jam_time = 1.4` vs `WeapNV357Revolver` `jam_time = 0.0`;
  hypothesis: using `animation_type` `3..=9` + `DNAM/jam_time > 0` as the rules-side
  availability gate without probing BSA `.kf` presence].
- On success, `ForceWeaponJamAnim` / `ForceJamAnim` queues the actor (deduplicated per
  frame, capped at 64 entries for headless runs [design]) for `nvf_play::combat::fire`
  (`Game::take_force_jam_requests`). For the player, `nvf_play::combat::fire` interrupts
  any active reload or attack phase via `Cycle::jam` and increments `WeaponAnim::jams`
  (which `nvforever::player_body` plays on the reload layer, interrupting any active reload
  clip) while reporting `AnimAction::Jam` (code `9` for `GetAnimAction`, `IsAnimPlayingEx 4`
  remains `0`, `GetSequenceAnimGroup 4` reads `200` `JamA`, and JIP's
  `SetOnReloadWeaponEventHandler` fires on jam entry [observed 2026-11-24: Fallout: New
  Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 + JohnnyGuitarNVSE 528 + ShowOffNVSE 184,
  10 official base masters, `GSDocMitchellHouse` (`0x00103DF9`), isolated clean-room
  staging copy, committed probe `w6t2_14_jip_on_reload_and_seq_anim_group`
  (`tools/capture/nvse/catalogs/wave6c_jip_on_reload_and_seq_group.json`):
  `PROBE|w6t2_14_jip_on_reload_and_seq_anim_group|fj=1,relHit=1,s0=0,s4(pre=24,t0=200,t1=200,cb=200)`;
  no DLL or source consulted]; generalizing `Phase::Jammed -> AnimAction::Jam` (`9` instead
  of `13`) to natural vanilla weapon-condition jams is [hypothesis]).
  `ForceWeaponJamAnim` also enters `AnimAction::Jam` synchronously so immediate script
  queries (`t0=200`) and the `SetOnReloadWeaponEventHandler` callback (`cb=200`) see the
  jam state without double-firing when `nvf-play` syncs `AnimAction::Jam` [design].
  Repeated calls while `Cycle::jammed` is already true return `1` without restarting the
  active jam timer [design]. NPC jam animation playback remains [not built] (`nvf-play`
  models `Cycle` for the player only). No ShowOff DLL or source code was consulted.

## 5. The 2026-10-30 batch (#1010, `nvse_showoff2.rs`)

Checked in the original game with the probe set `showoff2` (`tools/capture/jip_probes.py`,
lab `L5.ext.showoff2`): 49 of 50 rows match.

| Function | NVForever |
|---|---|
| GetEquippedWeapon | the equipped weapon's base, Fists (000001F4) when none [documented] |
| IsEquippedWeaponMelee, IsEquippedWeaponRanged, IsWeaponMelee `[weapon]` | melee is a weapon type (DNAM animation type) of 2 or below [documented: "type is <= 2"]; with no weapon, melee 1 and ranged 0 [documented] |
| GetItemCanHaveHealth `[item]` | a weapon or armor [documented] |
| IsLimbCrippled `[limb] [threshold]`, GetCrippledLimbsAsBitMask `[threshold]`, GetNumCrippledLimbs `[threshold]` | a limb's condition at or below the threshold (0); limbs head 0 … brain 6, bits head 1 … brain 64; -1 or none any limb, while out-of-range limb indices including `7` and `-2` return `0` [documented; observed: DamageAV 200 on the left leg gives 16, 1, 1; 2026-11-25, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE Plugin 184 + JIP LN NVSE 57.30 (5730), 10 official base masters, read-only `PrintToConsole` probe `w6d12_39_showoff_limb_crippled_bitmask_count_and_thresholds` (`tools/capture/nvse/catalogs/wave6d12_magic_effects_explosions_proj.json`): `init(m=0,c=0,any=0)\|bits(1,2,4,8,16,32,64)\|hd(i0=1,any=1,o7=0,m2=0,dmg=100)\|thr(t24=0,t25=1,t26=1,m50=3,c50=2)`; no DLL or source consulted] |
| GetActorValueDamage (GetAVDamage) `av` | the damage, no more than the value had [observed: DamageAV 200 on a limb of 100 reads 100] |
| GetActorValueName (GetAVName) `code` | the script name: 1 Confidence, 25 PerceptionCondition, 76 DamageThreshold, no spaces [observed]. The game compiles the code only in parentheses, as the page's example has it |
| GetAmmoName `[type] [ammo]`, SetAmmoName `name [type] [ammo]` | 0 FULL, 1 ONAM (the short name), 2 QNAM (the abbreviation) [documented: the page; xEdit AMMO]; a name the record lacks reads "<no name>" [observed: 10mm's abbreviation] |
| GetIngestibleConsumeSound `[aid]`, SetIngestibleConsumeSound `sound [aid]` | ENIT's consume sound [observed: the stimpak's 000296B9]; the setter true when set [documented; the game did not compile it in a `let`, so it is not observed]; routed through `Game::ingestible_def` with `TempCloneForm` `ALCH` support [design] |
| GetCreatureFootWeight, GetCreatureTurningSpeed `[creature]` | WNAM, TNAM; -1 for no creature [documented; observed: the dog 1 and 90, Doc -1] |
| SetCreatureFootWeight, SetCreatureTurningSpeed, SetCreatureBaseScale, SetCreatureReach `value [creature]` | WNAM, TNAM, BNAM, RNAM for the session; refused below 0; 1 when set [documented; observed: turning speed 45 read back, -1 refused] |
| GetCellEncounterZone (GetCellZone) `cell` | the cell's zone [documented; observed: Doc's house none] |
| GetZoneRespawns `zone` | not "Never Resets", reading saved `SetZoneRespawns` and session `SetZoneFlag` overrides through `Game::zone` [documented; observed: Vault 11 0, Hoover Dam 1; observed: 2026-11-24, Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, ShowOffNVSE 184, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `tools/capture/nvse/catalogs/wave6d1_showoff_xnvse_events_observed.json` probe `w6d1_39_showoff_levelup_early_exit_and_spell_usage`: `zone(0,1,0)` confirming live `SetZoneFlag`/`SetZoneRespawns` toggle readback; no DLL, PDB, or third-party source consulted] |
| IsBaseForm `form`, IsReferenceCloned, IsActorAlt `[form]` | `IsBaseForm` returns `1` for authored base forms (`WEAP`, `NPC_`, `CELL`, `WRLD`, `QUST`, `FACT`, `FLST`) and `TempCloneForm` `0xFF` base forms, and `0` for `Player`, `DocMitchellRef`, and doors [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, ShowOffNVSE 184, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `tools/capture/nvse/catalogs/wave6d1_is_base_form_tempclone.json` probe `w6d1_35_showoff_is_base_form_and_fade_state`: `base(w=1,cl=1,n=1,ce=1,wr=1,qu=1,fa=1,fl=1,doc=0,pc=0,dr=0)`; no DLL, PDB, or third-party source consulted]; `0xFF` `PlaceAtMe` placed references and temporary inventory references are also rejected [documented: IsBaseForm, a valid base form; design: check `world.placed`, `temp_item`, and `clones`]; `IsReferenceCloned` is a reference made in play (`0xFF`); `IsActorAlt` is an NPC or creature [documented] |
| ModNumericGameSetting `setting by` | the setting changed by the amount; 0 for no such setting [documented; observed: fMoveRunMult 4 + 1 = 5] |
| GetShowOffDebugMode, SetShowOffDebugMode | the switch kept; nothing more is printed |

The one row that differs is not ShowOff's: a `string_var` given to SetAmmoName's name
reached it as the variable's name, "sText", in the game [observed: showoff2
ammoname.restore], as SetUIStringAlt's `%z` did in gaps1. The game's runtime compiler
seems to pass a string variable's name where a parameter takes literal text [hypothesis];
NVForever passes the value. A third case: SetWeaponWorldModelPath given a `string_var` set
the path to the variable's name, "sSave" [observed: jg1, before the step was changed to a
literal].

## 6. The 2026-11-06 batch (#1010, `nvse_showoff3.rs`)

From the pages [documented], then checked in the original on 2026-11-08 with the
`showoff3` probe set (`tools/capture/jip_probes.py`, the airport save; lab scenario
L5.ext.showoff3), settled values tagged [observed: showoff3 <row>]; the census drops from
111 ShowOff functions to 73. Tests: `crates/fnv-game/tests/showoff3.rs` on a synthetic
plugin. None of the remaining ShowOff functions is called by the owner's two mod lists
(`docs/handoff/modlist-function-counts.json`: RemoveAllItemsShowOff once, the rest never),
so the batch was chosen by how well the pages describe a function and whether NVForever
has the system it reads.

Headless on FalloutNV.esm (`fnvtool script run`, new game, 2026-11-06) [observed]: the
player starts with the slots 64 + 8 taken (the Pip-Boy, non-playable) and no playable item
(GetEquippedItems 0 entries, with mask 64 one); with the 10mm pistol equipped the mask is
104 and GetBaseEquippedWeight 3; GetCalculatedMaxCarryWeight 200; TopicInfoGetResponseStrings
on 000E8911 gives the three texts the page's example lists, the first one word for word.

| Function (aliases) | NVForever |
|---|---|
| GetEquippedItems, GetEquippedItemRefs `[mask]` | the equipped items, bases or inventory references, each once. No mask: every playable item in a slot; slot bits 0–19 (GetEquippedObject's slots): the items in those slots, playable or not [documented: the pages' examples]. The mask's further filters are on a template the export lacks; bit 20 alone or with the weapon bit picks nothing [observed: showoff3 so3.items.bit20, .bit20.weapon]; items without a slot (ammo) do not count [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, ShowOffNVSE 184, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `tools/capture/nvse/catalogs/wave6d1_showoff_xnvse_events_observed.json` probe `w6d1p3_30_unequip_items_trigger`: `eq(1,0,m5=1,m20=0,bm=104,wr=1,ir=1)\|un0(0,1)\|un1(0,1)`; no DLL, PDB, or third-party source consulted]. Listed by their lowest slot: the Vault 21 jumpsuit (upper body), the Pip-Boy glove (left hand), the Pip-Boy [observed: showoff3 so3.items.allslots]. The other equipped rows differ from the headless run by what the save wears only (the jumpsuit; no weapon in either) [observed: so3.items.*, bitmask, broken, weight.*] |
| GetEquippedItemsAsBitMask (GetOccupiedEquipSlots) | the slots taken, non-playable items too [documented]: a weapon slot 5, armour its BMDT biped flags |
| GetNumBrokenEquippedItems (GetNumBrokenEq) `[healthPerc] [mask]` | weapons and armour at or below the health percentage, 0 by default [documented] |
| GetBaseEquippedWeight, GetCalculatedEquippedWeight `[minWeight] [mask]` | the base weights; the weights after Modify Light Items, Adjust Heavy Weapon Weight, the weapon's Decrease Weight mods, and the carrier's weightless tweaks (`WeightRules::effective_unit`, the inventory's own rules) [documented; design: docs/design/tweaks.md]. `minWeight` is inclusive (`>=`) and compares against the calculated (Pack Rat-reduced) weight [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, ShowOffNVSE 184, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `tools/capture/nvse/catalogs/wave6d1_showoff_xnvse_events_observed.json` probe `w6d1_31_showoff_equipped_weight_minweight`: `w=1.00\|base(0=1.00,eq=1.00,gt=0.00)\|calc(pre=1.00,0=0.50,half=0.50,mid=0.00,full=0.00)`; no DLL, PDB, or third-party source consulted] |
| GetCalculatedMaxCarryWeight | Carry Weight with the Get Max Carry Weight perks [documented] |
| UnequipItems `[mask] [noEquip] [hideMessage] [triggerOnUnequip]` | UnequipItem on each picked item [documented]; `OnActorUnequip` fires for both `triggerOnUnequip = 0` and `1` [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, ShowOffNVSE 184, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `tools/capture/nvse/catalogs/wave6d1_showoff_xnvse_events_observed.json` probe `w6d1p3_30_unequip_items_trigger`: `un0(0,1)\|un1(0,1)`; no DLL, PDB, or third-party source consulted] |
| GetEquippedWeaponRef, GetEquippedItemRefForItem `itemOrList` | the weapon, or the item or the first worn item of a form list, as an inventory reference; 0 with none [documented] |
| SetRandomizerSeed (SetSeed) `[seed]`, SetSeedUsingForm (SetFormSeed) `[form]`, GetRandomizerSeed (GetSeed), RandSeeded (GenerateSeededRandomNumber) `min max`, GetRandomPercentSeeded, AdvanceSeed `n` | one seed per calling plugin; the default 5489; -1 with no seed; min..=max both included, -1 for min above max; 1–100; AdvanceSeed 0 with no seed [documented]. The draws are MT19937's (5489 is its published default seed; checked against ISO C++ [rand.predef]'s 10000th output), scaled onto the range as min + floor(x × (max - min + 1) / 2^32), a draw whose product's low 32 bits fall under 2^32 mod (max - min + 1) thrown away for the next (0-1000000000 skips the third output for 5489) [observed: so3.seed.range.wide]: seed 5489 draws 82 14 91 84 13 from 1-100, seed 20 59 89 90 96 90 [observed: showoff3 so3.seed.default, .20.twice, .20.nextframe, .advance, .range.wide, .range.bits]. The same seed repeats its sequence, in one frame and the next [observed: so3.seed.20.twice, .20.nextframe]; SetRandomizerSeed 0 or none is 5489, and a default seed made by a first draw counts as set (AdvanceSeed 1) [observed: so3.seed.0, .noarg, .advance.afterdefault]. Seeds and bounds above 2^31 - 1 are held to it: 4294967295 reads 2147483647 and draws as that seed [observed: so3.seed.big, .range.uint]. SetSeedUsingForm takes only its argument: called on a reference with none it sets 5489 [observed: so3.seed.form.ref, .form.caps 15]. A fractional bound does not compile, in NVForever's runtime compile too (ext-syntax.md §11), and min above max is not probed (the page says the game crashes) [observed: so3.seed.range.fraction] |
| ApplyEasingAlt (EaseAlt) `input code mode` | ApplyEasing's curves by code, 0 sine … 9 bounce [documented; observed: showoff3 so3.ease.0 … .9]; -1 for another code [documented] or a mode other than 0-2 [observed: so3.ease.badmode]; an input outside 0-1 is eased as it is, 1.5 by quad-in 2.25 [observed: so3.ease.outside]. A fractional code (`ApplyEasingAlt 0.5 1.5 0`) does not compile, in NVForever's runtime compile too [observed: so3.ease.badcode; ext-syntax.md §11] |
| GetPosArray | X, Y, Z [documented] |
| IsOutsideMainThread | 0: every script runs on the main thread here |
| ConditionPrint | 1, and a console line naming the reference in NVForever's own words [documented] |
| IsAnimPlayingExCond `category [subType]` | IsAnimPlayingEx [documented] |
| IsJumping | IsInAir: the page marks it broken, it "functions equivalently to IsInAir" [documented]. `rP.IsJumping` did not compile in the original's runtime compiler [observed: showoff3 so3.jumping]: not registered under that name or no calling reference, the one row cannot tell. NVForever keeps compiling it [decision: #2037, the owner's 2026-11-10 ruling to refuse what the original refuses needs a rule the data gives; ext-syntax.md §11] |
| GetNumActorsInRangeFromRef `maxDistance [flags]` | the loaded actors within the distance (3-D, same space); 1 no dead, 2 no Invisibility or Chameleon above 0, 4 only actors in combat the caller detects [documented]. The caller does not count itself [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE 184, 2026-11-23/24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated clean-room staging copy (read-only `PrintToConsole` probes; zero writes to game install or Saves), `so_14_actors_in_range_excludes_self` (`tools/capture/nvse/catalogs/wave4a_extenders.json`, #3679, calling `Player.GetNumActorsInRangeFromRef` at radius `1.0`, `5000.0` flag `0`, and `5000.0` flag `1`): `PROBE\|so_14_actors_in_range_excludes_self\|0,1,1`, and `w6t2_08_showoff_equipped_weight_and_ally_temp` (`tools/capture/nvse/catalogs/wave6_t2_observed_matches.json`): `PROBE\|w6t2_08_showoff_equipped_weight_and_ally_temp\|wt(0.0,0.0,0.0,c=0.0)\|rng(10=0,2k=1)\|fac(0,0,0,0)`; zero DLL, PDB, or third-party source consulted] |
| SetIsPCAMurderer `0/1` | the flag IsPCAMurderer reads [documented] |
| GetPCHasSleepWaitOverride, SetPCHasSleepWaitOverride `0/1` | EnableFastTravel's can-wait flag, which the wait menu checks [documented: EnableFastTravel "can modify this canSleepWait override flag"] |
| GetChallengeProgress `challenge` | the progress the challenge system keeps: a non-recurring challenge done stays at its threshold, a recurring one counts again from 0 [documented] |
| GetCalculatedSkillPoints, GetCalculatedPerkPoints `[levelOverride]` (…EarnedPerLevel) | the points of the next level (`level + 1`), or of this one while the level-up menu is open: levelup.md's skill rate with Adjust Gained Skill Points; 1 at a level iLevelsPerPerk divides, else 0 [documented: "can only return 1 or 0"]. A nonzero `levelOverride` likewise evaluates `levelOverride + 1` while the level-up menu is closed: `GetCalculatedPerkPoints 1/2/3/4` returns `1/0/1/0` [observed: Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, ShowOffNVSE 184, 10 official base masters, `b3_so_05_calc_skill_perk_points_and_ini` in Doc Mitchell's house, no DLL or source consulted] |
| SetAllyTemp (SetAllyShowOff), SetEnemyTemp (SetEnemyShowOff) `f1 f2 [flag12 flag21]` | SetAlly's and SetEnemy's reactions, not saved [documented]; they override saved SetAlly / SetEnemy reactions until SetAlly or SetEnemy is called on the pair [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE 184, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, read-only `PrintToConsole` probes `w4c_a05b_faction_combat_reaction` and `w4c_a05c_temp_reaction_flags` (`tools/capture/nvse/catalogs/wave4c_extenders.json`, #3679, toggling `SetEnemyTemp`/`SetAllyTemp` vs `SetEnemy`/`SetAlly` on factions `0x1B2A4` and `0x104C6E` and polling `GetFactionCombatReaction`): `PROBE\|w4c_a05b_faction_combat_reaction\|c0=3,enT=0,alT=3,enP=1,alT_over_enP=3,alP=2,enT_over_alP=0` and `PROBE\|w4c_a05c_temp_reaction_flags\|c0=0,en00=1,al00=2,en11=0,al11=3`, and `w6d1_32_showoff_ally_enemy_temp_precedence` (`tools/capture/nvse/catalogs/wave6d1_showoff_xnvse_events_observed.json`): `init=0,al=2,enT=1,alOver=3,en=1,alT=2,rest=0`; no DLL, PDB, or third-party source consulted] |
| AuxTimerTimeToCountdown `name [owner]` | the time AuxTimerStart gave, -1 when the timer is not running [documented], the name in any case [observed: showoff3 so3.aux.countdown]. The owner argument is a base form: a reference named there stands for its base, so a reference's running timer reads -1 that way and its time when called on the reference [observed: so3.aux.countdown] |
| SetOnAuxTimerStartHandler (SetOnTimerStartHandler), SetOnAuxTimerUpdateHandler (SetOnTimerUpdateHandler) `1/0 handler name [owner]` | the start handlers get the name and the owner on each AuxTimerStart; the update ones, each frame, the name, the owner and the time since the last update (1 for a timer without flag 4), only for timers started with flag 256 [documented; observed: showoff3 so3.aux.update.counts, .frames, a frames timer's time 1]. A handler set or removed takes effect at the next frame: a start in the step that set it runs nothing, a restart a frame later runs it at once, and a restart in the step that removed it still runs it [observed: so3.aux.start.reg, .restart, .remove, .after]. NVForever's aux timers still count seconds whatever the flags say [not built: frames, the menu-mode flags] |
| GetRadiationExtraData, SetRadiationExtraData `rads` | XRAD; round-trips on a placed reference (`0.000 -> 12.500`, setter returning `1`) [observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE 184 + JIP LN 57.30 with the 10 official base masters, clean-room script probe `so_16_radiation_extradata_roundtrip` (`tools/capture/nvse/catalogs/wave4a_extenders.json`, #3678 / PR #3679), calling `GetRadiationExtraData`, `SetRadiationExtraData 12.5`, `GetRadiationExtraData`, and restoring on `0x00104C0F`: `0.000,1,12.500`; no DLL or source consulted]; a set level reaches all radiation consumers (`Game::radiation_markers_in`, using `SetRadius` if set; a reference with no positive XRDS radius gives nothing) [documented]; carried across same-run loads in `Game::Session` and not saved [hypothesis; not built: saving] |
| ModNumericINISetting `setting by` | GetNumericINISetting plus the amount, set back; 0 for no such setting [documented] |
| TopicInfoGetResponseStrings `info` | the response texts in order [documented; observed: showoff3 so3.responses.caravan, three of 144, 120 and 90 characters]; no array (ar_Size -1) for a form that is not a line [observed: so3.responses.stimpak] |

Left unbuilt (73), with the reason:

- No page, or an empty one, in the export: the seven Matrix functions, SayTo_GetTopic,
  SayTo_GetTopicInfo; GetActorFadeState, GetCalculatedActorSpread and IsFormLoading
  (pages marked Incomplete, with no summary).
- They write files or caches (ReadFromJSON, WriteToJSON, CreateFolder, DumpFormList,
  HasINISetting_Cached, ReloadIniCache, SaveCachedIniFile): left for a file batch that
  decides where NVForever may write (never the game folder; install-layout.md).
- A system NVForever lacks or does not expose to scripts: DisableAlt, EnableAlt and
  SetEnableParent (enable parents are fixed from the records), the level-up menu's pages
  and points (Get/SetLevelUpMenuCurrentPage, GetLevelUpMenuPoints,
  SetLevelUpMenuCanExitEarly, ShowPerkMenu, ShowSkillMenu), ShowPauseMenu, live
  explosions and projectiles (Get/SetExplosionRefRadius/Source, GetProjectileRefIsStuck,
  SpawnTracingProjectile, SetForceDrawHitscanProjectiles,
  SetProjectileTracerChanceOverride), the hotkey icons (Get/SetItemHotkeyIconPath), the
  Improved Detection entry point (IsActorInvisibleToPlayer,
  HighlightAdditionalReferenceAlt), combat allies (GetNumCombatActorsFromActor), the
  response-text setters (TopicInfoSetResponseStrings, TopicInfoSetNthResponseString: the
  dialogue reads the records directly), ForceHitStaggerReaction, ForceRecoilAnim,
  FreezeAmmoRegen, the corner-message queue's handler and ClearCinematicTextQueue,
  ToggleQuestMessages, PatchFreezeTime, ResetInteriorAlt, SetOwnershipTemp, the
  pickpocket switch (Get/SetPlayerCanPickpocketEquippedItems), ClearShowoffSavedData
  (the aux timers are not kept per plugin).
- Pages too thin to build from: GetPCHasScriptedFastTravelOverride (which flag is the
  override), GetPCCanFastTravel, KillAllHostiles ("within range" unstated),
  GetCalculatedItemValue, GetCalculatedAPCost, GetVATSMaxEngageDistance, ToANSIChar
  (keyboard layouts), GetHitLocationLingering, PlayerHasNightVisionActive,
  PlayerIsDrinkingPlacedWater, GetIsActivationPromptShown, IsPlayerLookingAround,
  GetSpellUsageNumEx, GetVATSTargetable, PlaceAtReticleAlt, RefillPlayerAmmo (deprecated
  for ITR's RefillAmmo), RemoveAllItemsShowOff (its page says its flags are buggy).

## 8. Batch 5, the last 37 (2026-11-08, #1010, `nvse_showoff5.rs`)

From the pages [documented], then probe set `showoff5` (`tools/capture/jip_probes.py`, lab
`L5.ext.showoff5`) run in the original on 2026-11-08 (save nvfairport, the player at the
airport test ground, Doc Mitchell in his house) [observed]. With it the census has no
ShowOff function left. GetHitLocationLingering on the unhurt player crashed the original
[observed 2026-11-08]; the row is left out of the set. Tests: `crates/fnv-game/tests/showoff5.rs`.

| Function (aliases) | NVForever |
|---|---|
| GetLevelUpMenuCurrentPage (GetLevelUpMenuPage) | -1 closed, 0 skills, 1 perks: the page the engine's menu reports each frame (`nvse_showoff5::LevelUpAsk`, `nvf_play::levelup::script_asks`) [documented] |
| SetLevelUpMenuCurrentPage (SetLevelUpMenuPage) `page` | 1 as Continue (which closes the menu when no perk is due), 0 as Back, 2 closes it, each whatever points are left; 0 when closed or already there [documented]. Closing keeps what was allotted [hypothesis] |
| GetLevelUpMenuPoints `[checkPerks] [checkAssigned]` | the skill points or perks to allot, or allotted; -1 closed [documented]. The page and points read at once after ShowSkillMenu, in the same script: ShowSkillMenu 3 then page 0, points 3, allotted 0, perks 0 [observed 2026-11-08: levelup.skillmenu]. After `ShowPerkMenu 2`, `GetLevelUpMenuPoints 1 0` and `1 1` both read `0` immediately same-frame, then `SetLevelUpMenuCurrentPage 2` returns `1` and `GetLevelUpMenuPoints 1 0` reads `-1` [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE 184 with the 10 official base masters, clean-room script probe `so_26_levelup_perk_menu_points`: `1,0,0,1,-1`; no DLL or source consulted]. SetLevelUpMenuCurrentPage 2 returned 1 and the menu read closed (-1) the next step [observed: levelup.after]; NVForever's engine closes it (`nvf_play::levelup`), the headless query tool has no level-up menu to close, so the lab row stays 0 there |
| SetLevelUpMenuCanExitEarly `1/0` | Continue and Done work with points left; 0 does not undo it [documented] |
| ShowPerkMenu `[numPerks] [title]` | the menu on its perks page alone, Back hidden, no level gained; by default the level's perk (1 at a perk level, else 0, browsing); 0 with no perk to take [documented]. Returns `1` for `ShowPerkMenu 0` and `ShowPerkMenu 2`, and when `SkillMenu` / `LevelUpMenu` (`menus::LEVEL_UP`, 1027) is already open [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE 184, 2026-11-23/24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `so_36_show_perk_menu_while_open` (`tools/capture/nvse/catalogs/wave4a_extenders.json`, #3679: `ShowSkillMenu 3`, `ShowPerkMenu 1`, `SetLevelUpMenuCurrentPage 2` -> `1,1,1`) and `w4d_38_showoff_perk_stagger_fade` (`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3679: `perk(1,1)` for `ShowPerkMenu 0`, `ShowPerkMenu 2`); no DLL or source consulted]; returns `0` without replacing `Game::menu` while any other menu (Container, Pip-Boy, Barter, etc.) is open so the active menu does not lose its `MenuClosed` / JIP `menuclose` event [hypothesis: non-`LevelUpMenu` menus unprobed; design: preserve open-menu close lifecycle] |
| ShowSkillMenu (ShowSkillsMenu) `[numSkills] [showDespiteNoPoints] [title]` | the skills page as the last page, the level's points by default; 0 with no points and showDespiteNoPoints 0 [documented] |
| ShowPauseMenu `[mode]` | the engine opens the pause menu on each call: 1 its load page, 3 its settings; 2 (save) and 4 (help) its main page, NVForever's pause menu having neither [documented: the modes; hypothesis: the fallback] |
| Get/SetExplosionRefRadius, Get/SetExplosionRefSource | the blast's live reference, placed by the engine for its frame (`Game::explosion_placed`) [documented]; GetExplosionRefRadius -1 on a reference that is no explosion [observed 2026-11-08: explosion.none, the player]. The engine works out who the blast reaches in the frame it places it, so a change reaches later reads only [not built: no script runs in between] |
| GetProjectileRefIsStuck | a shot that hit something and stays (`ProjState`) [hypothesis: the isStuck flag] |
| SpawnTracingProjectile | 0 [not built: the engine has no request to fire a bare projectile; follow-up] |
| SetForceDrawHitscanProjectiles `[forceDraw]`, SetProjectileTracerChanceOverride `[chance]` | whether before / the override before; the engine draws hitscan shots, never or always rolls tracers by them (`nvf_play::combat::shot_switches`) [documented] |
| GetPCHasScriptedFastTravelOverride, GetPCCanFastTravel | `GetPCHasScriptedFastTravelOverride` tracks `EnableFastTravel 0/1` (`0` default -> `1` after `EnableFastTravel 0` -> `0` after `EnableFastTravel 1`, `GetPCCanFastTravel == 0` in interior) [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE 184, 2026-11-23/24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, read-only `PrintToConsole` probes `so_17_fast_travel_override_and_can` (`tools/capture/nvse/catalogs/wave4a_extenders.json`, #3679, polling before `EnableFastTravel 0`, after `EnableFastTravel 0`, and after `EnableFastTravel 1`): `PROBE\|so_17_fast_travel_override_and_can\|0,0,1,0,0` and `w6d1_37_showoff_vats_fast_travel_and_switches` (`tools/capture/nvse/catalogs/wave6d1_showoff_xnvse_events_observed.json`): `ft(0,1,0,can=0)\|sw(0,1)\|hs(0,0,1)\|tr(-1,-1,1,0)\|m(1,0)`; no DLL, PDB, or third-party source consulted]; `GetPCCanFastTravel` performs the Pip-Boy's checks but the marker's: the bar, an interior or the world's flag (fast-travel.md §2; lifted by the player's Fast Travel From Anywhere, the map's other travel tweaks not built here), in the air, in combat, hostiles within fHostileActor…Distance, over-encumbered without Long Haul [documented: "the same check"; hypothesis: combat as the player's combat state] |
| IsActorInvisibleToPlayer, HighlightAdditionalReferenceAlt `[check] [flash]` | Invisibility or Chameleon above 0 and no Improved Detection [documented]; the highlight list is kept, the engine's VATS highlight does not draw it yet [not built: follow-up] |
| GetNumCombatActorsFromActor `[maxDistance] [flags]` | the caller's combat targets (2) and allies (1), 0 meaning 3; 4 no invisible ones, 8 only detected ones [documented]; an ally fights one of the caller's targets and would not attack the caller [hypothesis] |
| ForceHitStaggerReaction (ForceHitReaction), ForceRecoilAnim | stagger: 1 when the actor has a hit location (and, when asked, no Ignore Crippled Limbs) [documented; observed: 0 on unhurt Player and Doc Mitchell in `w4d_38_showoff_perk_stagger_fade` (`stag(0,0)`)]; recoil: 1 for a living actor unarmed or with a melee weapon equipped (`Baseball Bat`, `DNAM/animation_type == 2`), and 0 with a ranged gun equipped (`10mm Pistol`, `DNAM/animation_type == 3`) [documented: ForceRecoilAnim, "typically only works for melee/unarmed weapons"; observed 2026-11-08: recoil.player 1; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE 184, 2026-11-23/24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `so_33_force_recoil_unarmed_vs_gun` (unarmed `1`, holstered 10mm Pistol `0`, ForceHitReaction `0` -> `1,0,0`) and `w4d_38_showoff_perk_stagger_fade` (`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3679): `stag(0,0)\|rec(1,1,0)` (stagger on Player/Doc Mitchell; recoil unarmed, Baseball Bat, 10mm Pistol); no DLL or source consulted]; `Combat Knife` (`DNAM/animation_type == 1`) returns `1` while `Frag Grenade` (`DNAM/animation_type == 10`), a door, and a dead actor return `0` [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, ShowOffNVSE 184, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `tools/capture/nvse/catalogs/wave6d1_showoff_xnvse_events_observed.json` probe `w6d1_38_showoff_recoil_stagger_invis_combat_actors`: `rec(kf=1,gr=0,dr=0,dead=0)`; no DLL, PDB, or third-party source consulted]; remaining unprobed weapon animation types (`4..=9`) classified by `DNAM/animation_type <= 2` [hypothesis]; the engine does not play the animations [not built, as ForceWeaponJamAnim §4] |
| KillAllHostiles `[killer]` | KillActor on each loaded actor hostile to the player [documented; hypothesis: "within range" is the loaded area] |
| GetCalculatedItemValue `[barter]` | the value at the item's condition (fItemConditionValue…, as the barter menu) plus attached weapon mods' IMOD values, which condition does not lower (`FormStore::mods_value`) [observed: o26_value, weapon-mods.md §6]; honours `SetItemValue` overrides; with the flag while bartering, multiplies by the active trade's `SetBarterPriceMult` override or live `BarterView` buy/sell multiplier [hypothesis], falling back to the Barter skill GMST formula headless; -1 for no item [documented]; Calculate Buy Price perks outside a live `BarterView` not counted [hypothesis]. An item with a health is at full condition when it has none of its own and the formula holds there too, rounded to the nearest whole number: a fresh 10mm pistol (Value 750) 749 [observed 2026-11-08: itemvalue.pistol "749 749"; 750 · 0.0316 · 10^1.5 = 749.46] and a Varmint Rifle (Value 75) 75 at full condition / 49 at 75 % [observed: o26_value, weapon-mods.md §6; 74.946 → 75, 48.679 → 49] |
| GetCalculatedAPCost (GetCalculatedWeaponAttackAPCost) `attack [weapon]` | 0 the weapon's override or its animation type's fActionPointsAttack… setting [documented]: the 10mm pistol 17, its override [observed 2026-11-08: ap.pistol]; Fists 22, fActionPointsAttackUnarmed, their override AP 18 not used [observed: ap.fists]; other unarmed weapons use their own override AP for attack `0` with `0` for attacks `1`/`2` (`Power Fist` 28/0/0, `Brass Knuckles` `0x4324` 18/0/0), and melee weapons use their own AP for attack `0` with `0` for attack `1` (`Combat Knife` 17/0, `Baseball Bat` 25/0, `Sledgehammer` 38/0) [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE 184, 2026-11-23/24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, read-only `PrintToConsole` probes `so_20_calculated_ap_cost_melee_unarmed` (`tools/capture/nvse/catalogs/wave4a_extenders.json`, #3679): `PROBE\|so_20_calculated_ap_cost_melee_unarmed\|18,0,17,0,38,0,22` and `w6d1_36_showoff_calc_ap_cost_and_item_value` (`tools/capture/nvse/catalogs/wave6d1_showoff_xnvse_events_observed.json`): `pf=28/0/0\|kn=18/0/0\|kf=17/0\|bt=25/0\|val(full=749,dmg50=370,bad=-1)`; no DLL, PDB, or third-party source consulted]. 1 and 2 give 0 for the pistol and for Fists [observed: "17 0 0", "22 0 0"; the page's uppercut and cross settings not read], a melee weapon's 1 its VATS attack's AP [documented, not observed]; perks not counted [hypothesis] |
| GetVATSMaxEngageDistance, GetVATSTargetable | fVATSMaxEngageDistance [documented], 5000 when no plugin sets it [observed 2026-11-08: vats.engage, base game and DLCs]; enabled and present, a living actor or a reference VATS may mark (`Game::vats_targetable`) [documented]. The player is targetable [observed: vats.target.player 1]; Doc Mitchell, in his house while the player stood at the airport, was not [observed: vats.target.doc 0; hypothesis: an actor out of the loaded cells is not] |
| ToANSIChar | "" [decision: the keyboard layout is the user's system's, which no game→engine channel carries; owner's 2026-11-08 rule; follow-up]. On the owner's machine scan code 16 gave "q" and 2 gave "1" [observed 2026-11-08: ansi.16, ansi.2; machine-dependent, the lab rows stay apart] |
| GetHitLocationLingering | GetHitLocation's last hit, which NVForever keeps already [documented] |
| PlayerHasNightVisionActive | through a scope with "Scope has NightVision" at night, 20:00 to 6:00 [documented; hypothesis: the hours] |
| PlayerIsDrinkingPlacedWater | within a second of a drink from placed water [documented; hypothesis: the length] |
| GetIsActivationPromptShown, IsPlayerLookingAround | a reference under the crosshair within reach and no menu; the player's rotation changed since the last frame and looking not disabled [documented; hypothesis: what counts] |
| GetSpellUsageNumEx `ingestible` | the usage monitor's count times its magnitude while it lasts; 0 after, as GetSpellUsageNum [documented; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE 184 with the 10 official base masters, clean-room script probe `so_35_spell_usage_num_ex` (Jet `0x15164`, usage monitor magnitude 30): `0` before use, `30` after 1 use (`0,30`); no DLL or source consulted; hypothesis: NVForever's monitor forgets the count after expiry] |
| PlaceAtReticleAlt `form [count] [min] [max] [health]` | placed where the crosshair points, at least min and at most max along it, at the health given [documented; hypothesis: how the distances bound the point] |
| RefillPlayerAmmo `count`, RemoveAllItemsShowOff `[flags] [type] [target] [exceptions]` | ITR's RefillAmmo on the player [documented: deprecated for it]; RemoveAllTypedItems with flags 1 keep owners, 2 no messages, 4 the player's quest items, 8 non-playable biped items too (`functions::items::remove_all`) [documented] |

Follow-ups for the engine: a bare-projectile request (SpawnTracingProjectile), the VATS
highlight list, the stagger and recoil animations on request, the keyboard layout
(ToANSIChar), and reading back a live explosion's radius and source when a script can run
between placing it and its damage.
## 7. Batch 4 (2026-11-08, #1010, `nvse_showoff4.rs` and `mod_files.rs`)

The owner lifted the 2026-11-06 pause on 2026-11-08 ("finish every unbuilt extender
function"). From the pages alone [documented], not probed yet: probe set `showoff4`
(`tools/capture/jip_probes.py`, lab `L5.ext.showoff4`) is written, not run. The census
drops from 71 ShowOff functions to 37 (602 to 568 in all). Tests:
`crates/fnv-game/tests/showoff4.rs` on a synthetic plugin. The export now has pages for
the seven Matrix functions, SayTo_GetTopic(Info) and HasINISetting_Cached (§6 said none:
their titles have a space where the name has an underscore).

| Function (aliases) | NVForever |
|---|---|
| Matrix_IsMatrix (Mat_IsMat) `array` | a list of numbers (one row) or a list of lists of numbers, all of one length; empty, mixed or ragged arrays are none [documented: the page's seven examples]. Only list arrays count (stringmaps and numeric maps return 0) [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE 184, 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, read-only `PrintToConsole` probe `so_01_mat_is_stringmap` (`tools/capture/nvse/catalogs/wave4a_extenders.json`, #3679, testing `ar_List 1 2 3`, stringmap, and numeric map): `PROBE\|so_01_mat_is_stringmap\|1,0,0`; no DLL or source consulted] |
| Matrix_ApplyOperationWithMatrix (Mat_ApplyOpWithMat) `A op B` | `+ - / %` element by element (`%` the Schur product), `*` the matrix product when A's columns are B's rows; anything else no array [documented: the table and examples, e.g. (3 4; 5 6)·(7 8; 9 10) = (57 64; 89 100), 1×3 · 1×3 undefined] |
| Matrix_ApplyOperationWithScalar (Mat_ApplyOpWithScal) `A op s` | `+ - * /` on each element, `%` no array [documented]; division by 0 returns a same-length array of floating-point infinities (`inf`) rather than `ar_Null` [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE 184, 2026-11-23/24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, read-only `PrintToConsole` probes `so_02_mat_scalar_div0` (`tools/capture/nvse/catalogs/wave4a_extenders.json`, #3679): `PROBE\|so_02_mat_scalar_div0\|2,inf`, `w4c_b6_08_matrix_div0_and_1d_trans` (`tools/capture/nvse/catalogs/wave4c_extenders.json`, #3679): `PROBE\|w4c_b6_08_matrix_div0_and_1d_trans\|transN=3(t0=Array)\|div0N=3\|modN=-1`, and `w6t2_32_showoff_matrix_edges_and_get_weapon_mod_flags` (`tools/capture/nvse/catalogs/wave6_t2_observed_matches.json`): `PROBE\|w6t2_32_showoff_matrix_edges_and_get_weapon_mod_flags\|tr(sz=3,sub=1)\|div0=3\|q(sz=4,w=0.00,z=1.00)\|imod(4.00,0.40)\|pm=100.0`; zero DLL, PDB, or third-party source consulted]. The catalogue row was named "Matrix" (the template's name has a space); fixed in `script_functions.py` NAME_FIXES |
| Matrix_Transpose (Mat_T), Matrix_Dump (Mat_Dump) | the transpose [documented]; the dump prints a heading and each row with four decimals to the console, the heading in NVForever's own words |
| Quaternion_GetMatrix (Quat_GetMat) `[w x y z]`, Matrix3x3_GetQuaternion (Mat_GetQuat) | the layout [w, x, y, z] [documented]. Quat_GetMat does not normalise and gives the transpose of the usual right-handed matrix, in single precision: diagonal 2(w²+a²)−1, (2 0 0 0) gives 7 7 7 [observed 2026-11-08: quat.unnormal]; (0.7071 0 0 0.7071) gives m[0][0] −5.96046e-08, m[0][1] 1, m[1][0] −1, m[2][2] 1 [observed: quat.z90], and the x and y 90° rotations transpose alike (`m_x[1][2]=1, m_x[2][1]=-1, m_y[0][2]=-1, m_y[2][0]=1`) [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE 184, 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, read-only `PrintToConsole` probe `so_03_quat_getmat_xy_transpose` (`tools/capture/nvse/catalogs/wave4a_extenders.json`, #3679): `PROBE\|so_03_quat_getmat_xy_transpose\|1.0000,-1.0000,-1.0000,1.0000`; no DLL or source consulted]. Mat_GetQuat reads the usual right-handed form, by the largest of trace and diagonal with w not negative [observed: quat.back, quat.back.270 equal; observed 2026-11-24: `w6t2_32_showoff_matrix_edges_and_get_weapon_mod_flags` (`wave6_t2_observed_matches.json`): `diag(-1,-1,1)` -> `q(sz=4,w=0.00,z=1.00)`], so the two are not inverses |
| Results' shape | a 1-D row stays 1-D under scalar operations, element-wise matrix operations (`add`, Schur `%`), and `1x3 * 3x1`, and transposes to a column of one-element lists (and empty/ragged arrays give `Matrix_IsMatrix == 0`; `Quaternion_GetMatrix [0,0,0,0]` has `-1.0` diagonal; `Matrix3x3_GetQuaternion` returns `w >= 0` on 90-deg Z and 180-deg X/Y rotations) [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE 184, 2026-11-23/24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, read-only `PrintToConsole` probes `so_04_mat_transpose_1d_shape` (`tools/capture/nvse/catalogs/wave4a_extenders.json`, #3679): `PROBE\|so_04_mat_transpose_1d_shape\|3,1,3,Number`, `w4c_b6_08_matrix_div0_and_1d_trans` (`tools/capture/nvse/catalogs/wave4c_extenders.json`, #3679): `PROBE\|w4c_b6_08_matrix_div0_and_1d_trans\|transN=3(t0=Array)\|div0N=3\|modN=-1`, `w6d1p2_33_matrix_and_quat_edges` (`tools/capture/nvse/catalogs/wave6d1_showoff_xnvse_events_observed.json`: `is=0,0\|1d(add=Number:6,sch=Number:9,mul=1:Number)\|q0=3:-1.0\|qid=4`), and `w6d1p3_34_ext_cell_and_quat_neg` (`rz=(0.707,0.000,0.000,0.707)\|rx=(0.00,1.00,0.00,0.00)\|ry=(0.00,0.00,1.00,0.00)`), `w6t2_32_showoff_matrix_edges_and_get_weapon_mod_flags` (`wave6_t2_observed_matches.json`): `tr(sz=3,sub=1)`; no DLL, PDB, or third-party source consulted] |
| SayTo_GetTopic, SayTo_GetTopicInfo | the topic and response of the actor's running SayTo line (`Game::speech`), none without one [documented] |
| Get/SetItemHotkeyIconPath `item [onlyOverride]`, `item path` | the override, else GetTexturePath's icon; onlyOverride 1 the override or ""; "" resets [documented: GetItemHotkeyIconPath, SetItemHotkeyIconPath]. Wired into the engine's hotkey pictures (`Game::item_hotkey_icon_path`, `nvf_play::hotkeys::icon_override`, `weapon_wheel::draw`, `pipboy::hotkey_panel`) for both assigned slots (excluding Ammo Swap slot 2) and the Pip-Boy hotkey panel's pointed-at centre icon; override paths are normalised to lowercase backslashes relative to `textures\` (stripping any leading separator or `data\textures\` / `textures\` prefix, ignoring whitespace-only values) before `interface::Pictures` lookup [design]. Unmodded vanilla icon resolution is unchanged when no override is set |
| Get/SetPlayerCanPickpocketEquippedItems (Get/SetPCCanStealEqItems) | off by default; on, the pickpocket menu lists the victim's equipped items, never a child's (`Game::pickpocket_worn`, `nvf_play::menu::model::Steal::worn`) [documented] |
| ClearCinematicTextQueue (ClearQuestMessageQueue) | the quest lines waiting are dropped, as KillQuestUpdates [documented: "Clears the quest message queue"; hypothesis: the line on screen finishes] |
| ToggleQuestMessages `[bOn]` | 0 hides every quest line (`UiRequests::quest_messages_off`, read by `nvf_play::script`), 1 shows them; 1 when that changed, 0 when unchanged; -1 or nothing reads 1 while they are on and 0 while off [documented; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE 184, 2026-11-23/24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, read-only `PrintToConsole` probes `so_07_toggle_quest_messages` (`tools/capture/nvse/catalogs/wave4a_extenders.json`, #3679, calling `ToggleQuestMessages`, `-1`, `0`, `0`, no-arg, `1`, no-arg): `PROBE\|so_07_toggle_quest_messages\|1,1,1,0,0,1,1` and `w6d1_34_showoff_toggle_quest_msg_and_reset_int` (`tools/capture/nvse/catalogs/wave6d1_showoff_xnvse_events_observed.json`): `tqm=1,1,1,0,0,1\|ri=1,0\|fa=0,0,1\|ic(defLen=60,o0=[],ov=[interface\icons\w6d1.dds],clr=[])`; no DLL, PDB, or third-party source consulted] |
| SetShowOffOnCornerMessageEventHandler (SetOnCornerMessageEventHandler) `1/0 handler [flag]` | each corner message queued (ShowMessage, MessageEx…) reaches the handlers with its text, icon type 0, icon path, sound path and seconds, an empty string as "<void>" [documented]; run on the next frame, and the sound always "<void>" [hypothesis] |
| FreezeAmmoRegen `[toggle]` | whether it was frozen; while frozen no regenerating weapon gains rounds (`fnv_combat::attack::Cycle::regen_frozen`) [documented] |
| PatchFreezeTime | nothing to do: NVForever never stops loading cells while FreezeTime holds the clock [documented: the patch's only effect] |
| ResetInteriorAlt (IWantThisRoomCleanRightNow) `cell` | ResetInterior that also brings back dead actors flagged to respawn (`FormChanges::expired_cells_full`, `crate::respawn`) [documented]; returns `1` for an interior cell and `0` for a non-cell `WEAP` or null form [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, ShowOffNVSE 184, `GSDocMitchellHouse` (`0x00103DF9`) / `GSMapMarker` (`0x000CDF51`), 10 official base masters, `tools/capture/nvse/catalogs/wave6d1_showoff_xnvse_events_observed.json` probes `w6d1_34_showoff_toggle_quest_msg_and_reset_int` (`ri=1,0`) and `w6d1p3_34_ext_cell_and_quat_neg` (`ext(t=0,isInt=0,ri=0)`); no DLL, PDB, or third-party source consulted]; an exterior cell refused (0) [hypothesis, as ResetInterior] |
| SetOwnershipTemp `[newOwner] [cell]` | SetOwnership or SetCellOwnership for the session: a save writes the owner from before (`Game::snapshot`); SetOwnership / SetCellOwnership afterwards is saved as usual [documented] |
| ClearShowoffSavedData `0 auxTimers` | 1 the calling mod's private permanent aux timers, 2 every public permanent one, gone without their stop event [documented]. "*" starts a temporary timer's name and "_" a public one's [hypothesis: the aux variables' naming; the timers' own page is not in the export] |
| DisableAlt (SnigOff), EnableAlt (SnigOn) `[fade]` | Disable and Enable that also work on a reference with an enable parent: from then on its own flag counts (`World::keep_own_enable`) [documented: "Ignores Disable's limitation"; hypothesis: until SetEnableParent] |
| SetEnableParent (SetParentRef) `[newParent]` | the reference's enable parent for the session, none removing it; GetParentRef and the enable state follow (`World::set_enable_parent`) [documented]; not saved [hypothesis] |
| HasINISetting_Cached (HasINISettingC), ReloadIniCache, SaveCachedIniFile (SaveCachedIni) `[iniPath]` | the cache of `Data\Config` INI files (`mod_files.rs`): whether a key is cached; read again (0 for a missing file); written back into the file in NVForever's layer, keys in place, new ones added to their section in lower case [documented; hypothesis: the format kept] |
| The INI path of every `[iniPath]` function | under `Data\Config`; `..` parts are worked out, so `..\MCM\Translations\X.ini` is `Data\MCM\Translations\X.ini` [observed: MCM Extender's gr_MCMExtender.txt reads its menus' translations so, and its INIs sit there]; a path that would leave Data is not read (charter; the rule in full: local-ini-sibling-paths.md) |
| ReadFromJSON `path [pointer] [mode] [useCache]`, WriteToJSON `value path [pointer] [mode] [useCache] [saveFile] [compact]` | files under the game's folder through NVForever's layer (`GameRoot`, never the game folder), decoded as UTF-8 with any UTF-8 BOM stripped and `Data\...` paths also resolving through NVForever's layer/VFS [design]; objects are string maps, arrays lists, forms "@Plugin.esm:FormID" [documented]; all three parser modes (`0` JSON, `1` JAXN, `2` TaoConfig) read standard JSON, while JAXN comments/trailing commas succeed only in mode `1` and TaoConfig `key = value` succeeds only in mode `2` [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE 184, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `w4d_38_showoff_perk_stagger_fade` (`tools/capture/nvse/catalogs/wave4d_extenders.json`, #3679): `json(42,42,42,0/1,0/1)` (standard JSON across modes 0/1/2, JAXN in modes 0/1, TaoConfig in modes 0/2); no DLL or source consulted] (JAXN and TaoConfig syntax extensions are [not built]). Writing a single-level pointer (e.g., `"/k"`) on a missing file or `null` root creates a root `{}` object (and empty root pointer `""` creates/replaces the file), whereas a pointer with missing intermediate segments (e.g., `"/a/b/c"`) fails (returns `0`) without auto-creating missing intermediate objects [hypothesis]; malformed non-JSON files starting from null [hypothesis]. `useCache` gates reading/writing `ModFiles::showoff_json` (uncached saved writes drop any stale cached entry) and escaping paths are refused even when `saveFile` is 0 [design]. ReadFromJSON on a missing file gives no value, the call fails [observed 2026-11-08: readjson.missing, TestExpr 0]; a file that is not JSON likewise, a pointer to nothing 0 [hypothesis]; WriteToJSON's failures 0 [hypothesis] |
| CreateFolder (CreateDirectory) `path`, DumpFormList (FListDump) `list [file] [append]` | the folder made in NVForever's layer; the list's forms to the console or appended to a file there [documented], the lines in NVForever's own words |
| GetActorFadeState | 0 for a loaded actor not fading [observed: Fallout: New Vegas 1.4.0.525, xNVSE 6.4.9, ShowOffNVSE 184, 10 official base masters, `b3_so_01_lvlup_closed_and_status_flags`: `Player.GetActorFadeState` and `rDoc.GetActorFadeState` both returned `0.0` in Doc Mitchell's house, no DLL or source consulted]; −1 for an unloaded actor [observed 2026-11-08: `showoff4.fadestate.doc`, Doc Mitchell in his unloaded house while the player stood at the airport test ground]; NVForever reports `0` for the player or a present actor in a loaded cell and `−1` otherwise [hypothesis: unloaded/non-actor is the reason for `−1`, and other fade states are unknown because NVForever does not fade actors through this state] |
| GetCalculatedActorSpread | live actor weapon spread from the Gun Spread Formula, stance, crippled arms, and spread perks (`Game::calculated_actor_spread`) — `SetNoGunWobble 1` does **not** zero `GetCalculatedActorSpread`; 1H guns (anim `3`, `4`) add `+0.2` right / `0` left / `+0.4` both; 2H ranged weapons (anim `5`, `6`, `8`) add `+0.4` single arm / `+0.6` both arms; unarmed/melee/placed-mine/thrown (anim `0`, `1`, `10`, `11`) add `0` [documented: Gun Spread Formula; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 + ShowOffNVSE 184, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated clean-room staging copy (read-only `PrintToConsole` probe; zero writes to game install or Saves), `w4c_a06b_spread_settled` (`wave4c_extenders.json`, #3679), settled 6-type `w4d_09_spread_all_anim_types` (`wave4d_extenders.json`, #3802 / PR #3803): `PROBE\|w4d_09_spread_all_anim_types\|t3(0.1434,0.2359,0.1434,0.3284)\|t4(0.1388,0.2313,0.1388,0.3238)\|t5(0.1688,0.3538,0.3538,0.4463)\|t6(0.1434,0.3284,0.3284,0.4209)\|t8(0.1804,0.3654,0.3654,0.4579)\|t10(0.4504,0.4459,0.4504,0.4459)\|t3nw(0.1434,0.1434)`, and `w6d1_40_showoff_spread_anim_1_7_11_and_misc_states` (`tools/capture/nvse/catalogs/wave6d1_showoff_xnvse_events_observed.json`): `t0(1.0000,1.0000)\|t1(1.0000,1.0000)\|t11(1.0000,1.0000)`; zero DLL, PDB, or third-party source consulted; hypothesis: unprobed anim types and remaining scope/GMST details in `docs/spec-notes/nvse-stubs-persistent-fall-spread-unique.md`] |
| IsFormLoading | 0 [decision: the page is marked Incomplete with no summary, arguments or result; nothing to build from] |

Follow-ups: the three Incomplete pages need their text.

Left for batch 5 (37), with what each needs: the level-up menu's pages and points
(Get/SetLevelUpMenuCurrentPage, GetLevelUpMenuPoints, SetLevelUpMenuCanExitEarly,
ShowPerkMenu, ShowSkillMenu: the engine's menu owns its pages; a game↔engine channel is
needed), ShowPauseMenu, live explosions and projectiles (Get/SetExplosionRefRadius/Source,
GetProjectileRefIsStuck, SpawnTracingProjectile, SetForceDrawHitscanProjectiles,
SetProjectileTracerChanceOverride), the fast-travel checks (GetPCCanFastTravel,
GetPCHasScriptedFastTravelOverride), Improved Detection (IsActorInvisibleToPlayer,
HighlightAdditionalReferenceAlt), GetNumCombatActorsFromActor, ForceHitStaggerReaction,
ForceRecoilAnim, KillAllHostiles, GetCalculatedItemValue, GetCalculatedAPCost,
GetVATSMaxEngageDistance, GetVATSTargetable, ToANSIChar, GetHitLocationLingering,
PlayerHasNightVisionActive, PlayerIsDrinkingPlacedWater, GetIsActivationPromptShown,
IsPlayerLookingAround, GetSpellUsageNumEx, PlaceAtReticleAlt, RefillPlayerAmmo,
RemoveAllItemsShowOff.

## 9. `ShowOff:OnPOVChange` and preactivation events

- **`ShowOff:OnPOVChange`** [documented: GECK wiki,
  [ShowOff:OnPOVChange](https://geckwiki.com/index.php?title=ShowOff%3AOnPOVChange);
  observed 2026-11-24: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 +
  ShowOffNVSE Plugin 184, 10 official base masters, `GSDocMitchellHouse` (`0x00103DF9`),
  isolated clean-room staging copy, committed probe
  `w6t2p2_03_showoff_on_pov_change_toggle`
  (`tools/capture/nvse/catalogs/wave6c_showoff_on_pov_change_thisalt.json`):
  `PROBE|w6t2p2_03_showoff_on_pov_change_toggle|init=1|to3p(n=1,arg=0,inCb=1,aft=0,s0=0,sP=1)|to1p(n=2,arg=1,inCb=1,aft=1)`;
  no DLL or source consulted]: fires when the player's camera perspective switches
  between first-person and third-person, with calling reference `Player` (`0x00000014`,
  `GetSelfAlt == Player`, `0::Player` filterable) and one integer argument (`1` =
  first-person, `0` = third-person). NVForever dispatches it from
  `Game::set_first_person` (fed every frame by `nvf_play::control::tell_scripts`) only
  when `senses.first_person` actually changes; on 1P -> 3P (`0`) it fires before
  clearing `senses.first_person` so `IsPC1stPerson` still reads `1` inside the callback
  and `0` afterward, while on 3P -> 1P (`1`) `IsPC1stPerson` reads `1` inside and after.
  Save restore syncs `senses.first_person` directly to the restored camera POV without
  emitting a spurious transition [design]. Free when no handler is registered.
- **`ShowOff:OnPreScriptedActivate`**, **`ShowOff:OnPreActivateInventoryItem`**, and
  **`ShowOff:OnPreActivateInventoryItemAlt`**: see
  [preactivation-events.md](preactivation-events.md). No extender DLL or source code
  was consulted.

## 10. `ShowOff:OnCompleteObjective`, `ShowOff:OnDisplayObjective` and `ShowOff:OnWeaponUnholster`

- **`ShowOff:OnDisplayObjective`** [documented: GECK wiki,
  [Category:Event Handlers (ShowOff NVSE)](https://geckwiki.com/index.php?title=Category:Event_Handlers_%28ShowOff_NVSE%29),
  `ShowOff:OnDisplayObjective`; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 +
  ShowOffNVSE Plugin 184, 2026-11-23, GSDocMitchellHouse (`0x00103DF9`), 10 official base
  masters, clean-room script probes `w1_3656_objectives_unholster_imod` (`StartQuest
  0x000E282D`, `SetObjectiveDisplayed 20 1`/`20 1`/`20 0`/`20 1`/`999 1` + `CompleteQuest`
  + `SetObjectiveDisplayed 60 1`) and `b3_so_ex_06_on_display_complete_objective`
  (`SetObjectiveDisplayed 0x00104C1C 10 1` -> `reg=1,hit=1,dispInside=0`) in #3679; no DLL
  or source consulted]: fires PRE-update when a quest objective transitions from hidden to
  displayed, with calling reference `None` (`00000000`) and two arguments `(quest,
  objective_index)` (`"first"` / `1::` filters the quest form; `"second"` / `2::` filters
  the integer objective index). Inside the callback, `GetObjectiveDisplayed` still reads
  `0` (PRE-update), and reads `1` after the call. Hiding (`0`), repeating display on an
  already-displayed objective (`1 -> 1`), undefined objective index (`999`), and
  completed/failed quests do not fire.
- **`ShowOff:OnCompleteObjective`** [documented: GECK wiki,
  [Category:Event Handlers (ShowOff NVSE)](https://geckwiki.com/index.php?title=Category:Event_Handlers_%28ShowOff_NVSE%29),
  `ShowOff:OnCompleteObjective`; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 +
  ShowOffNVSE Plugin 184, 2026-11-23, GSDocMitchellHouse (`0x00103DF9`), 10 official base
  masters, clean-room script probe `w1_3656_objectives_unholster_imod` in #3679
  (`StartQuest 0x000E282D`, `SetObjectiveCompleted` `20 1`/`20 1`/`20 0`/hidden `30 1`,
  `SetObjectiveDisplayed 50 1` + `5 1` + `CompleteAllObjectives` firing `50` then `5`,
  `CompleteQuest` + `SetObjectiveCompleted 60 1`); no DLL or source consulted]: fires
  PRE-update when a **currently displayed** quest objective transitions from incomplete to
  completed, with calling reference `None` (`00000000`) and two arguments `(quest,
  objective_index)`. Inside the callback, `GetObjectiveCompleted` still reads `0`
  (PRE-update), and reads `1` after the call. Completing a hidden (undisplayed) objective,
  repeating completion (`1 -> 1`), un-completing (`1 -> 0`), undefined objective indices,
  and completed quests do not fire. `CompleteAllObjectives` iterates objectives in
  **`QOBJ` record order** (e.g., `50` then `5`, not ascending index order) and fires
  `ShowOff:OnCompleteObjective` only for currently displayed incomplete objectives.
- **`ShowOff:OnWeaponUnholster`** [documented: GECK wiki,
  [ShowOff:OnWeaponUnholster](https://geckwiki.com/index.php?title=ShowOff%3AOnWeaponUnholster);
  observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE Plugin 184,
  2026-11-23, GSDocMitchellHouse (`0x00103DF9`), 10 official base masters, clean-room
  script probe `w1_3656_objectives_unholster_imod` in #3679 (`Player.SetWeaponOut 0` /
  `Player.SetWeaponOut 1` with `Weap10mmPistol` `0x0000434F` -> `self=0x14, arg1=0x434F,
  IsWeaponOut=0`; `UnequipItem` + unarmed `Player.SetWeaponOut 1` -> `self=0x14, arg1=0,
  IsWeaponOut=0`); no DLL or source consulted]: added in ShowOff NVSE 1.80; fires at
  weapon draw START PRE-update so `IsWeaponOut` still reads `0` inside the callback.
  Calling reference (`GetSelf` / `GetSelfAlt` / `0::`) is the actor and the single
  argument (`"first"` / `1::`) is the actor's equipped or engine-held `WEAP` base form
  (e.g. `0x434F` for 10mm Pistol), or null form `0` when unarmed (NOT `Fists`
  `0x000001F4`). NVForever dispatches it at draw start in `Game::set_anim_state` (on
  `AnimAction::Equip` while holstered) and scripted `SetWeaponOut 1` (`nvse_hud.rs`),
  deduplicated via `ShowOffState::unholster_pending` so interleaved per-frame
  `set_weapon_out(actor, false)` calls during the draw and draw completion
  (`Game::set_weapon_out(actor, true)` / `hud_tick`) do not double-fire, while genuine
  draw cancellations (`AnimAction::Unequip`, `Equip` ending on an earlier frame without
  completing, `HolsterWeapon`, `SetWeaponOut 0`, or `forget_anim_state`) clear the latch;
  when an actor transitions directly to `weapon_out = true` without a prior `Equip`
  action, `set_weapon_out` / `hud_tick` still dispatches PRE-update while `IsWeaponOut`
  reads `0` [design]. Save restore writes `extra` directly without firing a spurious
  unholster event [design].
- [design] All three events short-circuit on `Game::handled(...)` before allocating
  argument vectors or resolving weapon forms, leaving unmodded vanilla gameplay
  unchanged with zero allocations. No ShowOff NVSE DLL, PDB, or C++ source code was
  consulted.

## 11. `ShowOff:OnFireWeapon`

- **`ShowOff:OnFireWeapon`** [documented: GECK wiki Category:Event Handlers (ShowOff NVSE),
  [SetEventHandlerAlt](https://geckwiki.com/index.php?title=SetEventHandlerAlt); observed:
  Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE Plugin 184, 2026-11-23,
  GSDocMitchellHouse (`0x00103DF9`), 10 official base masters, clean-room script probe
  `w1_3632_onfireweapon_and_forcejam` in #3679 (`SetEventHandlerAlt
  "ShowOff:OnFireWeapon"` with `begin Function { ref rWeap }`, `Player.FireWeapon` with
  `Weap10mmPistol` `0x0000434F` and 7-pellet Caravan Shotgun `0x000CD53A`); no DLL or
  source consulted]: dispatched when an actor fires a ranged weapon, with the firing actor
  reference as the calling reference (`0::` filter, `GetSelf`/`GetSelfAlt`, observed for
  `Player 0x14`; NPC-actor dispatch [documented: GECK wiki; hypothesis: unprobed in
  committed #3679 catalog]) and the `WEAP` base form as the single argument (`1::` /
  `"first"::` filter, `Begin Function { ref rWeapon }`, observed `0x434F` 10mm pistol and
  `0xCD53A` Caravan Shotgun).
- Dispatched from `Game::weapon_fired` (player shots) and `Game::projectile_fired`
  (NPC shots and scripted `FireWeapon` shots), deduplicated per `(frame, actor, weapon)`
  so `weapon_fired` followed by `projectile_fired` or a multi-pellet shotgun burst fires
  once per shot [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + ShowOffNVSE Plugin
  184, 2026-11-23, GSDocMitchellHouse (`0x00103DF9`), 10 official base masters, probe
  `w1_3632_onfireweapon_and_forcejam` in #3679: `Player.FireWeapon` on the 7-pellet
  Caravan Shotgun `0x000CD53A` fires `ShowOff:OnFireWeapon` once, not 7 times].
- Is not suppressed by player god mode (`ToggleGodMode`), unlike JIP's
  `SetOnFireWeaponEventHandler` whose god-mode omission suppresses player firing
  [observed: 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas
  1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 + ShowOffNVSE 184 with the 10 official base
  masters, isolated clean-room staging copy (read-only `PrintToConsole` probe; zero writes
  to game install or Saves), committed probe `w6t2p3_04_fire_god_melee`
  (`tools/capture/nvse/catalogs/wave6_t2_observed_matches.json`):
  `PROBE|w6t2p3_04_fire_god_melee|norm(so=1,jp=1)|god(so=1,jp=0)|melee(so=1,jp=1)`; zero
  DLL, PDB, or third-party source consulted]. Excludes non-actors (e.g., trap activators)
  and melee weapons (`DNAM/animation_type` `0..=2`) during normal combat [hypothesis:
  non-actor exclusion unprobed; note scripted `Player.FireWeapon` on a melee weapon fired
  both handlers in `w6t2p3_04_fire_god_melee`].
- Free when no `ShowOff:OnFireWeapon` handler is registered (unmodded vanilla weapon
  firing is unchanged). No ShowOff DLL or source code was consulted.
