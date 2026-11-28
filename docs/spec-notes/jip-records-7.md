# JIP LN NVSE, batch 7 (2026-11-01, #1006)

Ammunition, ammo effects, projectiles, explosions, impact data, weathers and climates, and
armour, in `crates/fnv-game/src/nvse_jip7.rs`. Sources: the GECK wiki export, each
function's page [documented: the page named]; the fields: xEdit's record definitions as
the schema keeps them (`schemas/fnv`). Not probed in the original (the extender goldens
are kept on the other machine); choices the pages leave open are tagged [hypothesis].
Setters change the form for the session and are read by this batch's getters and the
earlier ones (GetAmmoSpeed, GetAmmoConsumedPercent, GetAmmoCasing, GetNumAmmoEffects,
GetNthAmmoEffect, GetAmmoEffectTraitNumeric, GetClimateNumWeatherTypes,
GetClimateNthWeather…). The setters of a trait return 0 [observed: jip7 below; Fallout:
New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-23, `GSDocMitchellHouse`
(`0x00103DF9`), 10 official base masters, read-only `PrintToConsole` probe
`b3_jip_ex_04_set_ammo_trait_ret` (`tools/capture/nvse/catalogs/wave4b_extenders.json`,
#3679, calling `SetAmmoTraitNumeric rAm 0 1.5` on 10mm Ammo `0x4241`):
`PROBE|b3_jip_ex_04_set_ammo_trait_ret|s0=1.00,ret=0,s1=1.50`; no DLL or source
consulted].

## 1. Ammunition and ammo effects

| Function | NVForever |
|---|---|
| SetAmmoTraitNumeric `ammo trait value` | 0 speed (DATA), 1 projectiles per shot (DAT2), 2 ammo consumed percent (DAT2), 3 clip rounds (DATA) [documented: the page's table, which lists 0-3 though its argument says 0-2]; accepts `TempCloneForm` `AMMO` clones [design] |
| SetAmmoProjectile, SetAmmoCasing `ammo [form]` | DAT2's projectile and consumed ammo; left out, none [documented]; accepts `TempCloneForm` `AMMO` clones, and `SetAmmoProjectile` validates `PROJ` (following clones) when a non-null form is supplied [hypothesis] |
| AddAmmoEffect, RemoveAmmoEffect `ammo effect` | the RCIL list: 1 when it changed, 0 for an effect already there or not there [documented: `successful`; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 with the 10 official base masters, clean-room script probe jip_16_ammo_effect_add_remove_ret (AddAmmoEffect/RemoveAmmoEffect): 1,0,1,0; no DLL or source consulted]; accepts `TempCloneForm` `AMMO` and `AMEF` clones [design]; `GetNumAmmoEffects`, `GetNthAmmoEffect`, `ApplyAmmoEffects`, `GetCalculatedWeaponDegradation`, and `Game::ammo_traits` / `Game::ammo_effect_defs` read the changed list |
| SetAmmoEffectTraitNumeric `effect trait value` | AMEF DATA: 0 type, 1 operation, 2 value [documented]; accepts `TempCloneForm` `AMEF` clones [design] |

Session `AMMO` and `AMEF` overrides are exposed canonically through `Game::ammo_traits`,
`Game::ammo_flag`, `Game::ammo_effect_traits`, `Game::ammo_effect_defs`, and
`Game::apply_ammo_effects_value` (`crates/fnv-game/src/nvse_jip7.rs`), and `TempCloneForm`
snapshots all effective `AMMO` / `AMEF` fields (`Game::snapshot_ammo_traits`) so pre-clone
edits carry over while post-clone edits on the original or parent clone remain isolated
[design]. `fnv-combat` provides `AmmoEffect::from_numeric` and `CombatForms::chain_with_ammo`
for live overridden `AmmoStats` / `AmmoEffect` evaluation in shot chains, damage, spread,
and weapon wear [design]. Wiring engine combat (`engine/nvf-play`) to read
`Game::ammo_traits` at shot creation remains a follow-up, and original-game capture parity
for wrong-type `SetAmmoProjectile` rejection and clone-of-clone isolation remain unqualified.

## 2. Projectiles and explosions

| Function | NVForever |
|---|---|
| SetProjectileTraitNumeric `projectile trait value` | DATA: 0 type, 1 gravity, 2 speed, 3 range, 4 tracer chance, 5 explosion proximity, 6 explosion timer, 7 muzzle flash duration, 8 fade duration, 9 impact force, 10-12 rotation x/y/z, 13 bouncy mult; 14 the detection sound level (VNAM) [documented: the page's table] |
| SetProjectileLight `projectile [light]` | DATA's light; left out, none [documented] |
| SetProjectileMuzzleFlash `projectile path` | NAM1, the muzzle flash model |
| SetExplosionTraitNumeric `explosion trait value` | DATA: 0 force, 1 damage, 2 radius, 3 IS radius, 4 radiation level, 5 dissipation time, 6 radiation radius, 7 sound level [documented]; accepts `TempCloneForm` `EXPL` clones and feeds `Game::explosion_traits`, `Game::add_explosion_radiation_zone`, `Game::explosive_from_explosion`, `Game::explosion_hit_damage`, and `Game::explosion_placed` radius overrides [design] |

The engine's projectiles and explosions still read most visual/audio fields as loaded;
session `DATA/speed`, `DATA/impact_force`, and `DATA/flags` overrides
(`SetProjectileTraitNumeric`, `SetProjectileFlag`) are exposed clone-first through
`Game::projectile_launch_traits`, `Game::projectile_flags_override`,
`Game::projectile_flags`, and `Game::projectile_can_be_disabled`, `EXPL` session trait
and flag overrides are exposed through `Game::explosion_traits`, and
`fnv_combat::data::ProjectileStats::apply_flags` (alongside `WeaponStats::apply_flags1`
for `WEAP` `DNAM/flags1`) projects bitmasks onto typed combat records [design]
(follow-up: wire the remaining engine projectile, weapon, and explosion visual/audio/blast
readers through these views).

## 3. Impact data and materials

| Function | NVForever |
|---|---|
| GetImpactDataTrait `ipct trait` | 0-4 the impact data (DATA: duration, orientation, angle threshold, placement radius, sound level), 5-7 the texture set (DNAM) and sounds 1 and 2 (SNAM, NAM1), 8-15 the decal data (DODT: widths, heights, depth, shininess, scale, passes), 16 the flags, 17 the colour as R × 1000000 + G × 1000 + B [documented: the page's table]; past 17, 0 |
| SetImpactDataTraitNumeric `ipct trait value` | the same without the forms: 0-4, 5-12 the decal data, 13 the flags, 14 the colour [documented] |
| SetImpactDataTraitForm `ipct trait form` | 0 texture set, 1 sound 1, 2 sound 2 [documented]; a form of the wrong type (a creature for a sound) sets none [observed: jip7 ipct.setform.sound1] |
| SetImpactDataSet `weapon dataset` | WEAP INAM |
| Get/SetImpactMaterialType `[material] [baseActor]` | NAM4 of the NPC or creature, 0-11 [documented: the table]; a number past 11 is not set |

The flags combine two records' bits: 1 No Decal Data is DATA's flag bit 0, 2 Parallax, 4
Alpha Blending and 8 Alpha Testing are the decal data's flag bits 0-2 [documented: the
table; the bits: xEdit IPCT and the decal data].

## 4. Weathers and climates

| Function | NVForever |
|---|---|
| Get/SetWeatherPrecipitationModel `weather [path]` | MODL |
| Get/SetWeatherTexture `weather layer [path]` | the cloud layers 0-3: DNAM, CNAM, ANAM, BNAM [documented: layers 0-3; the fields: xEdit WTHR] |
| SetWeatherTransitionTimeOverride `[seconds]` | kept (`Game::weather_transition_override`); none or 0 removes it [documented] |
| ClimateAddWeatherType `climate weather chance [global]` | adds to WLST, or sets the chance and global of a weather already there [documented]; 1 [documented: `successful`] |
| ClimateRemoveWeatherType `climate weather` | 1 when it was there, 0 otherwise [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 with the 10 official base masters, clean-room script probe jip_18_climate_remove_missing_weather (ClimateRemoveWeatherType): 0; no DLL or source consulted] |

The sky still reads the climates' weathers and the weathers' textures as loaded, and its
transitions take their own time (follow-up: the sky through these changes).

## 5. Armour

| Function | NVForever |
|---|---|
| SetArmorClass `armor class` | 1 light, 2 medium, 3 heavy [documented]: BMDT's general flags Medium (bit 3) and Heavy (bit 7), read by `Game::armor_class` |
| SetArmorRCT `armor path` | BMCT, the ragdoll constraint template |
| Get/SetBipedModelList `armor [list]` | ARMO BIPL (`nvse_plugins.rs`); validates `ARMO` target and `FLST` list, treats non-`FLST` and explicit null (`0`) as no-ops, and clears only when `list` is omitted [documented: GECK wiki GetBipedModelList, SetBipedModelList; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-25, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated clean-room staging copy (read-only probe; zero writes to game install or Saves), `w6d2p5_04_biped_model_list_omit` (`tools/capture/nvse/catalogs/wave6d2_set_biped_model_list_validate.json`): `PROBE\|w6d2p5_04_biped_model_list_omit\|set=1,badEqFl=1,badZ=0,nullZ=0,omitZ=1`; zero DLL, PDB, or third-party source consulted] |
| SetArmorConditionPenalty `[multiplier]` | kept; returns the value before the call; left out, unchanged; defaults to Stewie's `Tweaks::armor_condition_penalty` (1.0 by default) until a script sets it [documented; observed: jip7 armor.condpenalty; design: docs/design/tweaks.md]; any finite float is stored raw without clamping (5.0, 3.0, -1.0) [observed: jip7b armor.condpenalty.neg; Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 with the 10 official base masters, clean-room script probe jip_19_armor_cond_penalty_clamp (SetArmorConditionPenalty): 1.000,1.000,5.000,5.000,-1.000; no DLL or source consulted] |
| SetDamageToArmorMaxPercent `[share]` | kept, 0-1 only (2 and -1 set nothing); returns the value before the call; left out, unchanged; 1 until set [observed: jip7 armor.maxpercent, jip7b armor.maxpercent.range] |

Combat still uses the fixed multiplier 1.0 and no cap (`fnv_combat::condition`), and the
ragdolls and movement read the records as loaded (follow-up: these through the changes).
SetDamageToArmorMaxPercent "is saved in the game" [documented]; NVForever keeps it for the
session only for now.

## 6. Values a field cannot hold

Every setter of this batch passes its number through one check
(`crate::nvse::field_number`): NaN and infinity set nothing; any other number is clamped
to the field's range: an f32 field's, a u8's (clip rounds, parallax passes), a u32's
(projectiles per shot), the documented ranges of an enumeration (effect type 0-5,
operation 0-2, projectile type 1-16, sound level 0-2, orientation 0-2, a climate chance
0-100, impact flags 0-15, the colour up to 255255255); and a whole number for every field
that is not an f32 [hypothesis: the game keeps its record fields in range]. Also:
SetWeatherTransitionTimeOverride's seconds (0 and up), SetArmorConditionPenalty's
multiplier (any finite float stored unclamped, observed above) and SetDamageToArmorMaxPercent's share (0-1
[documented]). The later batches use the same check.

## Probed in the original (2026-11-08, #1006)

Sets jip7 (108 rows) and jip7b (46 rows) of `tools/capture/jip_probes.py`, run from the
`nvfairport` save on the 10mm round, AmmoEffectMagnumDTReduction, 10mmBulletProjectile,
GrenadeFragExplosion, BaseBallBatMetalImpact (no decal data) and RipperImpactOrganic,
NVWastelandClear, NVDefaultClimate and ArmorLeather; goldens private (lab L5.ext.jip7,
L5.ext.jip7b). What §6 left open is settled field by field [observed]:
- the setters of a trait return 0, as supposed;
- ammunition: projectiles per shot keep their sign, cut to whole (-5, -2.5 reads -2); clip
  rounds wrap as a byte (300 reads 44, 256 reads 0, -1 reads 255);
- ammo effects: a type past 0-5 and a negative value set nothing;
- projectiles: the type takes 0 or a single type bit (1, 2, 4, 8, 16; 3 and 17 set
  nothing), the sound level only 1 or 2 (0, 3, 5 and -1 set nothing);
- explosions: the sound level wraps as a byte with no range (9 reads 9, 300 reads 44);
- impact data: the flags are kept as set, to nine bits (99, 256; -1 reads 511); a colour
  past 0-255255255 sets nothing, a channel past 255 below it is kept (300 reads 300); an
  impact without decal data reads widths and heights 8 to 32, depth 32, shininess 4,
  parallax scale 1 and 4 passes.
Equal after the fixes: jip7 105 of 108, jip7b 44 of 46. Left then: the colour of an impact
without decal data, and 16777215 reading back as 16009218. Both were settled on 2026-11-11 (below)
and built (`nvse_jip7.rs`, #2039): jip7 108 of 108, jip7b 46 of 46 since.

### The colour's packing (2026-11-11, #2039)

Probe job `rules2039`: 23 colours set on RipperImpactOrganic and BaseBallBatMetalImpact,
each over 63063063, read back; GetImpactDataTrait 17 on every impact without DODT
[observed; results private]. One rule fits every row:
- **Set:** a number past 255255255 or below 0 sets nothing. Otherwise it is split as
  RRRGGGBBB (R = n / 1000000, G = n / 1000 mod 1000, B = n mod 1000), and the colour's four
  bytes become R + G × 256 + B × 65536 as one 32-bit sum. A channel past 255 carries into
  the next byte: G 999 is 3 × 256 + 231, so the next byte gets 3 more.
- **Get:** R is the first byte and G the second. B is not cut to a byte: it is the third
  and fourth bytes together, as a 16-bit number.
- **Checked by:** 256000 reads 1, 999000 reads 231003, 254999999 reads 254232002,
  16777215 reads 16009218, and 300, 1256, 65535 and 65536 read back as set. 255255255 is
  kept and 255255256 sets nothing. The two impacts gave the same rows.
- **An impact without DODT** reads the decal data of its texture set (DNAM):
  BallisticOrganicBBImpact reads 100000052, which is DecalOrganicBallisticBB's DODT colour
  bytes 64 00 34 00. An impact with no texture set reads all four bytes as FF:
  R 255, G 255, B 65535, which gives 255320535. This held for BaseBallBatMetalImpact,
  BaseBallBatConcImpact, NoImpact, SwordOtherImpact and ImpactWaterSplashSm.
- **Not probed:** that the other decal fields of such an impact also come from its texture
  set [hypothesis]. Without one they read the defaults above.
- **Probe side effect:** setting the colour back to 255320535 (past 255255255) set nothing,
  so the bat impact kept 1256 for the rest of that game session. Nothing was saved.
