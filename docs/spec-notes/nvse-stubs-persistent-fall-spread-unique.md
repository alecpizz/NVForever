# NVSE zero-stub batch: `SetPersistent`, `GetFallTimeRemaining`, `GetCalculatedActorSpread`, `IsItemUnique` / `ToggleItemUnique`

Part of #2272. Core-only implementations in `crates/fnv-game` replacing four
`=> zero` stubs without altering `Game::Session` or `engine/`.

## 1. `SetPersistent` (`crates/fnv-game/src/nvse_gaps.rs`, `nvse_refs.rs`)

- [documented: https://geckwiki.com/index.php?title=SetPersistent]
  `reference.SetPersistent persistent:0/1` sets or clears the persistent record
  flag (`0x400`, `fnv_sim::refs::FLAG_PERSISTENT`) on the calling reference for
  the session.
- [observed: `gaps6` probe `persistent.doc`] `SetPersistent` returns `0`.
- [design] `SetPersistent` records an explicit per-reference boolean in
  `OtherState::persistent_overrides` and keeps bit `0x400` in
  `OtherState::flags` synchronized (starting from the reference's current
  record flags and persistent-group status, including `FormId::PLAYER_REF`).
  `Game::persistent` checks `OtherState::persistent_overrides` first and
  otherwise falls back to `FormId::PLAYER_REF` (persistent by default) or static
  placed flags (`0x400`) or `in_persistent_group`, so unrelated writes to
  `OtherState::flags` via `SetReferenceFlag` or `SetFlagsLow` on a
  persistent-group reference whose static record lacks bit `0x400` cannot
  accidentally clear its persistence. `IsPersistent`, `GetPermanent`,
  `GetReferenceFlag 1024`, `GetFlagsLow`, unloaded-cell walks
  (`GetFirstRefInCell`/`GetNumRefsInCell`/`GetRefsInCell`), `GetFormRefs`, and
  `PersistentRefCount` all stay synchronized with explicit `SetPersistent`
  calls.
- [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24,
  `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters,
  `w4d_06_player_setpersistent_0` (`tools/capture/nvse/catalogs/wave4d_extenders.json`,
  #3679): `PROBE|w4d_06_player_setpersistent_0|p0=1,after0=0,after1=1`; no DLL or source
  consulted] `Player.IsPersistent` starts at `1`, `Player.SetPersistent 0` clears it to
  `0`, and `Player.SetPersistent 1` restores `1`.
- [hypothesis] Persistence changes made by `SetPersistent` are session-only and
  do not persist across save/load.

## 2. `GetFallTimeRemaining` (`crates/fnv-game/src/nvse_jip11.rs`)

- [documented: https://geckwiki.com/index.php?title=GetFallTimeRemaining]
  `actor.GetFallTimeRemaining` returns the estimated remaining fall time in
  seconds before the calling actor reaches the ground, and `0` when grounded.
- [design] `Jip11State::fall_time_remaining` and `Game::set_fall_time_remaining`
  allow an engine caller to supply a direct per-actor remaining fall time. When
  no direct value is present and the actor is grounded (`air_times <= 0`),
  `Game::fall_time_remaining` returns `0.0`.
- [hypothesis] When an actor is airborne (`gaps.air_times > 0`) and no direct
  engine value is supplied, `Game::fall_time_remaining` estimates remaining time
  by solving `0.5 * g_eff * t^2 - vz * t - height = 0` from a downward
  `CastKind::World` ray cast (`crate::ray::cast`), tracked vertical velocity
  (`jip2.velocity`, falling back to `-g_eff * air_time` when untracked), world
  gravity (`686.7` units/s² = 9.81 m/s² × 70 units/m, or `SetLocalGravityVector`
  when active), and `SetActorGravityMult`. `IsInAir` is untouched.

## 3. `GetCalculatedActorSpread` (`crates/fnv-game/src/nvse_showoff4.rs`)

- [documented: https://geckwiki.com/index.php?title=GetCalculatedActorSpread,
  https://geckwiki.com/index.php?title=Gun_Spread_Formula_(New_Vegas),
  https://geckwiki.com/index.php?title=Perk_Entry_Points,
  `crates/fnv-combat/src/tuning.rs`, `crates/fnv-combat/src/spread.rs`,
  `schemas/fnv/objects.json` `WEAP.DNAM.animation_type`]
  `actor.GetCalculatedActorSpread` computes the calling actor's live weapon
  spread from the equipped weapon's skill, Strength/skill requirements, and
  animation type (`DNAM` with live `field_overrides`), actor skill/Strength,
  stance (iron sights/aiming, walking/running, sneaking), crippled arms (1H vs
  2H), and perk entry points (`MODIFY_WEAPON_STRENGTH_REQ`,
  `MODIFY_AIMING_MOVE_SPEED`, `CALCULATE_GUN_SPREAD`), reading the 12 Gun Spread
  Formula GMSTs from the load order with fallback defaults matching
  `crates/fnv-combat/src/tuning.rs` (`SETTINGS`, sourced from GECK wiki
  `Gun Spread Formula (New Vegas)`):
  - `fUnaimedSpreadPenalty = 0.2` `[documented: Gun Spread Formula (New Vegas); crates/fnv-combat/src/tuning.rs]`
  - `fWalkingSpreadPenalty = 0.1` `[documented: Gun Spread Formula (New Vegas); crates/fnv-combat/src/tuning.rs]`
  - `fRunningSpreadPenalty = 0.2` `[documented: Gun Spread Formula (New Vegas); crates/fnv-combat/src/tuning.rs]`
  - `fStandingSpreadPenalty = 0.1` `[documented: Gun Spread Formula (New Vegas); crates/fnv-combat/src/tuning.rs]`
  - `fCrippledArm1HSpreadPenalty = 0.2` `[documented: Gun Spread Formula (New Vegas); crates/fnv-combat/src/tuning.rs]`
  - `fCrippledArm2HSpreadPenalty = 0.4` `[documented: Gun Spread Formula (New Vegas); crates/fnv-combat/src/tuning.rs]`
  - `fCrippledArms1HSpreadPenalty = 0.4` `[documented: Gun Spread Formula (New Vegas); crates/fnv-combat/src/tuning.rs]`
  - `fCrippledArms2HSpreadPenalty = 0.6` `[documented: Gun Spread Formula (New Vegas); crates/fnv-combat/src/tuning.rs; observed: w4d_09_spread_all_anim_types below]`
  - `fWeapStrengthReqPenalty = 0.025` `[documented: Gun Spread Formula (New Vegas); crates/fnv-combat/src/tuning.rs]`
  - `fWeapSkillReqPenalty = 0.01` `[documented: Gun Spread Formula (New Vegas); crates/fnv-combat/src/tuning.rs]` (note: the wiki page reports the original game applies `fWeapStrengthReqPenalty` to the skill shortfall as a bug; matching `fnv_combat::spread::actor_spread`, NVForever uses `fWeapSkillReqPenalty` `[design]`)
  - `fWobbleToSkillConversion = 0.5` `[documented: Gun Spread Formula (New Vegas); crates/fnv-combat/src/tuning.rs]`
  - `fMinGunSpreadValue = 0.01` `[documented: Gun Spread Formula (New Vegas); crates/fnv-combat/src/tuning.rs]`
- [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 +
  ShowOffNVSE 184, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official
  base masters, isolated clean-room staging copy (read-only `PrintToConsole`
  probes; zero writes to game install or Saves), `#3679` and companion PR
  `#3803` (`#3802`); zero DLL, PDB, or third-party source consulted]:
  - `w4c_a06b_spread_settled` (`tools/capture/nvse/catalogs/wave4c_extenders.json`,
    `#3679`):
    `1h(norm=0.60925,nw=0.36175,back=0.14338,rc=0.23587,both=0.32838,cripNW=0.32838)|2h(crip=0.32838,cripNW=0.32838,norm=0.14338)`
    — `SetNoGunWobble 1` does **not** zero `GetCalculatedActorSpread` (remains
    `0.32838` on both 1H and 2H). In `w4c_a06b`, the 1H `norm=0.60925` vs
    `nw=0.36175` vs `back=0.14338` readings were taken while the weapon-draw
    spread transient was still decaying (`imm=0.81475 -> settled 0.14338`), not
    an effect of `SetNoGunWobble`; settled `t3nw(0.1434,0.1434)` below confirms
    `n == nw == 0.14338`.
  - `w4d_09_spread_all_anim_types` (`tools/capture/nvse/catalogs/wave4d_extenders.json`,
    `#3802` / PR `#3803`, waited 12 ticks after `SetWeaponOut 1` so every weapon
    settled before sampling):
    `PROBE|w4d_09_spread_all_anim_types|t3(0.1434,0.2359,0.1434,0.3284)|t4(0.1388,0.2313,0.1388,0.3238)|t5(0.1688,0.3538,0.3538,0.4463)|t6(0.1434,0.3284,0.3284,0.4209)|t8(0.1804,0.3654,0.3654,0.4579)|t10(0.4504,0.4459,0.4504,0.4459)|t3nw(0.1434,0.1434)`
    — across settled spread readings (explaining the earlier unsettled run where
    `t3` uncrippled was still decaying from `0.81475` down to `0.14338`):
    1. **1H guns (`anim 3` 10mm Pistol, `anim 4` Laser Pistol):** right arm
       adds `+0.0925` (`0.2 * skill_mult`), left arm alone adds `+0.0000`, both
       arms add `+0.1850` (`0.4 * skill_mult`).
    2. **2H weapons (`anim 5` Hunting Rifle, `anim 6` Plasma Rifle, `anim 8`
       Flamer):** either single arm (`r` or `l`) adds `+0.1850`
       (`0.4 * skill_mult`), and both arms add `+0.2775`
       (`0.6 * skill_mult`, matching `fCrippledArms2HSpreadPenalty = 0.6`).
    3. **Thrown (`anim 10` Frag Grenade `0x4330`):** zero crippled-arm spread
       penalty (`0.4504` / `0.4459` / `0.4504` / `0.4459`, `±0.0045` idle sway).
    4. **Settled `SetNoGunWobble` check (`t3nw(0.1434,0.1434)`):** `n == nw ==
       0.1434`, confirming `SetNoGunWobble 1` leaves `GetCalculatedActorSpread`
       unchanged once weapon-draw bloom has settled.
  - `w6t2p2_07_spread_anim_7_9_2_1_0`
    (`tools/capture/nvse/catalogs/wave6_t2_observed_matches.json`):
    `PROBE|w6t2p2_07_spread_anim_7_9_2_1_0|t8(0.6212,0.4232,0.4232,0.5157)|t9(0.1781,0.3631,0.3631,0.4556)|t2(1.0000,1.0000)`
    — directly confirms:
    1. **`anim 9` (Missile Launcher `0x4340`):** either single arm adds
       `+0.1850` (`0.4 * skill_mult`, `0.1781 -> 0.3631`) and both arms add
       `+0.2775` (`0.6 * skill_mult`, `0.1781 -> 0.4556`), matching the 2H
       ranged group `5..=9`.
    2. **`anim 8` (Minigun `0x433F`):** symmetric single-arm readings
       (`0.4232` R and L) and both-arms increment `+0.0925` (`0.4232 -> 0.5157`,
       with `n=0.6212` still decaying from draw bloom).
    3. **`anim 2` (Baseball Bat `0x421C`):** zero crippled-arm penalty
       (`1.0000 -> 1.0000` with both arms crippled).
- [hypothesis] Items without a direct external source citation or original-game
  observation:
  1. **Unprobed weapon animation types (`0..=1`, `7`, `11..=13`)**:
     `w4d_09_spread_all_anim_types` and `w6t2p2_07_spread_anim_7_9_2_1_0`
     directly observed types `3, 4` (1H guns), `5, 6, 8, 9` (2H ranged), and
     `2, 10` (melee/thrown). Grouping `7` (Rifle Energy) with `5, 6, 8, 9` (2H
     ranged), and `0..=1` (unarmed/1H melee) plus `11..=13` (mines/thrown) with
     `2, 10` (zero crippled-arm penalty) is `[hypothesis]`.
  2. **GMST fallback defaults when absent from masters**: per
     `crates/fnv-combat/src/tuning.rs` and `docs/spec-notes/combat-data.md` §3,
     falling back to the wiki GMST values when a load order omits those GMST
     records is `[hypothesis]`.
  3. **ShowOff `GetCalculatedActorSpread` remaining scope/units**: returning the
     actor-spread component in spread units after `CALCULATE_GUN_SPREAD` perks
     (excluding weapon `min_spread`, draw recoil bloom, and ammo spread
     modifiers, and returning `0.0` when unarmed or called without a subject) is
     `[hypothesis]` pending further original-game captures.

## 4. `IsItemUnique` / `ToggleItemUnique` (`crates/fnv-game/src/nvse_plugins.rs`, `nvse_gaps.rs`, `repair.rs`)

- [documented: https://geckwiki.com/index.php?title=ToggleItemUnique,
  https://geckwiki.com/index.php?title=IsItemUnique]
  `ToggleItemUnique item/list:form doToggle:1/0` marks or un-marks a single item
  form or every member of a `FLST` form list as unique for the session so it
  does not appear as an optional repair-part source in the repair menu;
  `IsItemUnique item:form` returns `1` if marked unique and `0` otherwise.
- [observed: `data3` probes `unique.stimpak`, `unique.pistol`, `unique.toggle`,
  `unique.restore`; and Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30,
  2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, read-only
  `PrintToConsole` probe `core_19_toggle_item_unique_non_weap_armo`
  (`tools/capture/nvse/catalogs/wave4a_extenders.json`, #3679, calling `ToggleItemUnique`
  then `IsItemUnique` on `WEAP` `0x434F`, `MISC` Caps `0xF`, and `ALCH` Stimpak `0x15169`):
  `PROBE|core_19_toggle_item_unique_non_weap_armo|1,0,0`; no DLL or source consulted]
  Non-repairable forms such as `ALCH` ingestibles (stimpak) and `MISC` items (caps)
  ignore `ToggleItemUnique` and continue reading `0` from `IsItemUnique`.
- [design] Session state is stored in `GapsState::unique_items` (keeping
  `Game::Session` untouched), restricted to `WEAP` and `ARMO` base forms (and
  `FLST` members resolving to `WEAP`/`ARMO`). `Game::is_item_unique` exposes the
  flag, and `Game::player_repair_list` (`crates/fnv-game/src/repair.rs`) filters
  out unique items as repair-part sources while still allowing a unique item to
  be repaired as the target using non-unique donors.
- [hypothesis] Whether other non-`WEAP`/non-`ARMO` forms beyond `ALCH` and `MISC`
  are also ignored in the original game, and whether unique status persists across
  save/load, remain hypothesis pending capture.
