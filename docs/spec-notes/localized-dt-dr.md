# Spec note: optional localized head and body armor DT/DR (`localized-dt-dr`, #3843)

Covers the optional, off-by-default NVForever setting `localized-dt-dr`
(`Gameplay › Combat` — **Localized armor DT/DR**), applying worn head-slot armor DT/DR
only to head hits and worn body-slot armor DT/DR only to non-head hits, per the
2026-11-24 owner ruling on `#3607` (`docs/design/extender-compat.md` §2 and §8: JIP LN
`[GamePatches]` are not read from `jip_nvse.ini`; wanted patches become individual named
NVForever options, off by default).

## 1. Documented plugin behaviour

[documented: **JIP LN NVSE Plugin v57.30** (`https://www.nexusmods.com/newvegas/mods/58277`),
**JIP LN Settings INI v56.24** public `jip_nvse.ini` `[GamePatches]` section comment; no
plugin DLL, PDB, or third-party source code was consulted]

> `bLocalizedDTDR=0` (comment text; optional INI ships `0` or `1` depending on user config)
> `; When enabled, a distinction will be made between head-armor and body-armor when applying damage reduction from DR/DT.`
> `; Head hits will benefit only from DT/DR gained from worn head-armor (if any), whereas body hits, in similar fashion,`
> `; only from DT/DR gained from worn body-armor. (Note: Requires bIgnoreDTDRFix to be enabled).`

NVForever does **not** read `jip_nvse.ini` for this option; it exposes a named NVForever
setting that defaults to **Off** (`false`) so unmodded vanilla gameplay (full worn armor
DT/DR summed across head and body slots on every hit) is unchanged unless the player opts
in.

## 2. Observed explosion and innate actor DT/DR behaviour

[observed: Fallout: New Vegas `1.4.0.525` (`040020D0`) + xNVSE `6.4.9` + JIP LN NVSE
`57.30`, 2026-11-24, 10 official base masters (`FalloutNV.esm`, `DeadMoney.esm`,
`HonestHearts.esm`, `OldWorldBlues.esm`, `LonesomeRoad.esm`, `GunRunnersArsenal.esm`,
`CaravanPack.esm`, `ClassicPack.esm`, `MercenaryPack.esm`, `TribalPack.esm`), Goodsprings
outdoor marker (`FalloutNV.esm` `0x000CDF51`), isolated clean-room staging copy separate
from the real install comparing `jip_nvse.ini` `bIgnoreDTDRFix=1` with `bLocalizedDTDR=1`
vs `bLocalizedDTDR=0` (zero files written by probe scripts; only console `PrintToConsole`
output collected); committed probe `w4e_c02_jip_localized_dtdr_explosions_innate` in
`tools/capture/nvse/probes/localized_dt_dr_probe.json`; no DLL, PDB, or third-party source
code was consulted]

Steps in the committed probe (`w4e_c02_jip_localized_dtdr_explosions_innate`):
move player to `0x000CDF51`, set Health to `1000`, and detonate `MineFragExplosion`
(`FalloutNV.esm` `0x0007078B`, base damage `100`) at distance `0` across five equipment
states:
1. Naked (no armor, `0` innate DT).
2. Body armor only (`ArmorCombat` `0x00020420`, DT `15`).
3. Helmet only (`ArmorCombatHelmet` `0x00020426`, DT `3`).
4. Both body armor and helmet (`15 + 3 = 18` DT).
5. Naked with innate actor DT (`ModAV DamageThreshold 15`).

Recorded console output (identical under `bLocalizedDTDR=1` and `bLocalizedDTDR=0`):

- `c1_naked:dt=0|dmg=150.00|hhd=150.00|hbd=100.00`
- `c2_body:dt=15|dmg=127.50|hhd=127.50`
- `c3_helm:dt=3|dmg=145.50|hhd=145.50`
- `c4_both:dt=18|dmg=123.00|hhd=123.00`
- `c5_innate:dt=15|dmg=127.50|hhd=127.50`

This confirms two boundary rules under `bLocalizedDTDR`:
- **Explosions** continue to apply full worn armor DT/DR across both body and head slots
  (`Zone::All` / unlocalized `target_armor`).
- **Innate actor DT/DR** (Actor Values `DamageThreshold` 76 and `DamageResistance` 18,
  e.g. from perks, traits, or creature base stats) continues to apply everywhere in
  addition to the zone's worn armor.

## 3. NVForever design choices

All items below are NVForever's own engineering choices (`[design]`, not behaviour claimed
from JIP LN's plugin):

- **[design] Setting identity, placement, and default:** named NVForever switch
  `localized-dt-dr` (**Localized armor DT/DR**), registered in
  `engine/nvf-play/src/combat.rs` (`LocalizedDtDr` resource, synced live into
  `Combat::localized_dt_dr` by `sync_localized_dt_dr`), placed under `Gameplay › Combat` in
  `crates/nvf-settings-layout/src/lib.rs`, persisted in `NVForever.ini` `[Settings]`, and
  **off by default** (`false`).
- **[design] No separate `bIgnoreDTDRFix` gate:** JIP LN's INI comment notes
  `(Note: Requires bIgnoreDTDRFix to be enabled)` because unpatched Fallout: New Vegas
  applied DR before DT and broke the Ignore DT/DR weapon flag. NVForever's
  `fnv_combat::damage::hit` already subtracts DT before DR and honors `WEAP`
  `Ignores Normal Weapon Resistance` (`docs/spec-notes/weapon-data-fields.md` §6), so
  `localized-dt-dr` is a single standalone switch.
- **[design] Head vs. body slot classification and hit-zone routing:**
  `CombatForms::armor_values_worn_in_zone_with` filters worn `ARMO` pieces by
  `ArmorStats::covers_head()` for `Zone::Head`, `ArmorStats::covers_body()` for
  `Zone::Body`, and all worn armor for `Zone::All`. `target_armor_for_zone` is used on
  projectile hits (`fly`), player melee (`melee`), NPC/creature attacks (`actor_attacks`),
  the `armor_blocks_blood` blood-suppression check (`fly`), and direct-hit VATS damage
  estimates (`vats_hit_estimate`, while its explosion estimate uses unlocalized
  `target_armor`). Hits without body-part data fall back to `Zone::Body` via `Zone::of(None)`.
  Explosions (`blast`), the target HUD (`hud_target.rs`), and the Pip-Boy total DT readout
  (`pipboy/items.rs`) continue to use unlocalized `target_armor`.
