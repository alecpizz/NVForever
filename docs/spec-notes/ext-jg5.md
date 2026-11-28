# JohnnyGuitar NVSE, batch 5 (2026-11-08, #1007)

Alternate textures, armour FaceGen model flags and icons, worldspace map textures and
encounter zones, region map names and the location name, camera shots, body part strings,
explosion and projectile sounds, equip types, recipe and weapon mod lookups, actor effect
types, the time played and GetRGBColor, in `crates/fnv-game/src/nvse_jg5.rs`. Sources: the
GECK wiki export [documented: the page named]; the record layouts from `schemas/fnv`
(xEdit wbDefinitionsFNV.pas); the base game's records read with `fnvtool esm decode`
[observed]; probe set `jg5` (`tools/capture/jip_probes.py`, run by the observer session on
nvfairport) [observed: jg5], lab `L5.ext.jg5` 88 of 88 rows equal. Setters change the
record for the session, read by the getters. Tests: `crates/fnv-game/tests/jg5.rs`.

The schema gains REGN `RDMP`, the map name: a zero-terminated string after an RDAT of
type 4 [observed: VMapGoodspringsRegion holds "Goodsprings", priority 50]. `GameData`
keeps it with the header's priority (`RegionDef::map`).

"Fails" below is the game's "script call failed": the script stops. NVForever words the
error as the VM's NoReference, which stops it too (as ClampToGround, `nvse_jippp`).

| Function | NVForever |
|---|---|
| GetWeaponAltTextures `weapon` | the texture sets of the world model's alternate textures (MODS), sorted by 3D index [documented; observed: jg5, dynamite's record lists indices 2, 0, 1 and the game 0, 1, 2] |
| GetArmorAltTextures `armor model` | 1 male biped (MODS), 2 female biped (MO3S), 3 male world (MO2S), 4 female world (MO4S), sorted by 3D index [documented: the model IDs; the subrecords: ARMO's model order, MODL MOD3 MOD2 MOD4, as SetBipedModelPath reads it; observed: jg5, the radiation suit]; another model gives no array (ar_Size -1) [observed: jg5, models 0 and 5] |
| GetAltTexturesEx `form [model]` | a map from each 3D index to its texture set [documented; observed: jg5]; an armour without a model reads model 3, the male world model [observed: jg5, MS18RangerBattlearmor and the radiation suit]; models outside 1-4 give no array [observed: jg5] |
| SetWeaponAltTexture `weapon index set`, SetArmorAltTexture `armor model index set` | the entry of that 3D index takes the texture set, or a new entry is added [documented: the notes on 5.07]; 1 when done, 0 and nothing added for a form that is not a TXST [observed: jg5, a Stimpak]. A new entry has no shape name [hypothesis] |
| ClearWeaponAltTexture `weapon index` | that index's entry removed, every one for -1 [documented]; 1 when one was removed and 0 for a specific index with none [observed: jg5, index 5; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 with the 10 official base masters, clean-room script probe `jg_12_clear_weapon_alt_tex_ret` (SetWeaponAltTexture/ClearWeaponAltTexture): 1,1,0; no DLL or source consulted], and 1 for `-1` (clear all) even when no alternate textures were set [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `w4c_b6_06_clear_weap_alt_tex` (`tools/capture/nvse/catalogs/wave4c_extenders.json`, #3679): `emp5=0,all=1`; no DLL or source consulted] |
| GetFacegenModelFlag, SetFacegenModelFlag `armor flag female [on]` | 0 head, 1 torso, 2 right hand, 3 left hand of MODD (male) or MOSD (female) [documented: the flag IDs; the subrecords: schema common.json; observed: leather armour's MODD and MOSD are both Torso] |
| SetBipedIconPathAlt `path female form` | ICON (male) or ICO2 (female), read by GetBipedIconPath [documented] |
| GetWorldSpaceMapTexture, SetWorldSpaceMapTexture `world [path]` | WRLD ICON [documented; observed: WastelandNV's `interface\worldmap\wasteland_nv_1024_no_map.dds`, the page's example path] |
| GetWorldspaceEncounterZone (GetWorldZone) `world` | SetWorldspaceEncounterZone's zone, else WRLD XEZN [documented] |
| SetWorldspaceEncounterZone (SetWorldZone) `world [zone]` | none removes it; 1 when done, 0 for a form that is not a zone [documented]. A cell that names no zone takes its worldspace's (`nvse_jg5::cell_zone`, read by GetCellEncounterZone, GetZone and the cell-load level fixing, `zones.rs`) [hypothesis, as encounter-zones.md §1]. SetCellEncounterZone's change now reaches the level fixing too |
| GetRegionMapName `region` | the map name (RDMP) [documented; observed: jg5, VMapGoodspringsRegion "Goodsprings"]; a region without map data fails [observed: jg5, 0x123CE3]; a form that is no region also fails / returns no name [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 with the 10 official base masters, clean-room script probe jg_13_region_map_name_non_region (GetRegionMapName): Goodsprings,-1; no DLL or source consulted] |
| SetRegionMapName `region name` | 1; a region without map data gains it [documented: "it will be created"; observed: jg5, 0x123CE3 reads "NvfProbe" back], at priority 0 [hypothesis] |
| GetLocationName `ref.` | indoors the cell's name (FULL); outdoors the first map name among the regions the cell is in, in CELL XCLR order, an empty one passed over; else the worldspace's name [documented: "interior cell name, worldspace name or a Region Map Data name", and the 5.18 note; observed: the observer's travel job: Doc Mitchell's House, Goodsprings outdoors, "Mojave Wasteland" at McCarran and in cell (18,-5), whose regions have no map data]. Order: a name SetRegionMapName gave 0x123CE3, Goodsprings' first region in XCLR, won over VMapGoodspringsRegion's at priority 50 [observed]; that XCLR order decides rather than the priority is [hypothesis] (no base-game cell in a 400-cell sample is in two map regions). Without a calling reference the call fails [observed: jg5] |
| SetExplosionSound `explosion which sound` | 1 sound 1, 2 sound 2 (EXPL DATA), read by GetExplosionTraitForm and `Game::explosion_traits` [documented]; accepts `TempCloneForm` `EXPL` and `SOUN` forms [design] |
| SetProjectileSound `projectile which sound` | 1 the sound, 2 countdown, 3 disable (PROJ DATA sound, sound_countdown, sound_disable) [documented] |
| (both sound setters) | a form that is not a SOUN changes nothing [hypothesis] |
| SetEquipType `item type` | ETYP of the item (a reference's base), read by GetEquipType [documented]; types 0-13 are set and give 1; -1 and anything past 13 change nothing and give 0 [observed: jg5, the Stimpak (11) set to 5, 40 and -1] |
| GetRecipeCategoryFlags `category` | 1 when the category is a subcategory (RCCT DATA bit 0, `RecipeCategoryDef`) [documented] |
| GetFormRecipesAlt `item` | the recipes whose outputs hold the item, in FormID order [documented; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 with the 10 official base masters, clean-room script probe `jg_14_form_recipes_alt_and_weapons_for_mod_order` (GetFormRecipesAlt/GetWeaponsForMod): 3,00176E5B,-1; no DLL or source consulted] |
| GetWeaponsForMod `mod` | the weapons whose mod slots (WMI1-3) hold the mod, in FormID order [documented; observed: `jg_14_form_recipes_alt_and_weapons_for_mod_order`]; returns `ar_Null` (`ar_Size == -1`) for a form that is not an IMOD [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `jg_14_form_recipes_alt_and_weapons_for_mod_order` (`3,00176E5B,-1`) and `w4c_b3_02_region_loc_weapmod` (`tools/capture/nvse/catalogs/wave4c_extenders.json`, #3679): `loc=[Doc Mitchell's House]\|modN=3(w0=00176E5B)\|badModN=-1`, and `w6t2_30_jg5_get_weapons_for_mod_and_armor_alt_tex` (`tools/capture/nvse/catalogs/wave6_t2_observed_matches.json`): `PROBE\|w6t2_30_jg5_get_weapons_for_mod_and_armor_alt_tex\|wMod(n=3,w0=00176E5B,bad=-1)\|clr(1m1=1,10=0,4m1=1,0m1=0,5m1=0,199=0)`; no DLL or source consulted] |
| GetActorEffectType `effect` | SPEL SPIT type: 0 actor effect, 1 disease, 2 power, 3 lesser power, 4 ability, 5 poison, 10 addiction; -1 for anything else [documented; observed: jg5, WithdrawalJet 10, Jet and the 10mm pistol -1] |
| GetCameraShotFlags, SetCameraShotFlags `shot [flags]` | CAMS DATA flags [documented; observed: jg5, CamDefaultShoot 7] |
| GetCameraShotImageSpaceModifier, SetCameraShotImageSpaceModifier `shot [imod]` | MNAM, none clearing it [documented; observed: jg5]; a form that is not an IMAD changes nothing [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 with the 10 official base masters, clean-room script probe jg_15_camerashot_imod_non_imad (GetCameraShotImageSpaceModifier/SetCameraShotImageSpaceModifier): 1,00028FA1; no DLL or source consulted] |
| GetCameraShotTraitNumeric, SetCameraShotTraitNumeric `shot trait [value]` | 0 action, 1 location, 2 target (their DATA codes), 3-5 the player's, target's and global time multipliers, 6 max time, 7 min time, 8 target % between actors [documented: the notes; observed: jg5, CamDefaultShoot]; 0 for another trait. The setter gives 1 when it set the trait, and keeps a code as given even outside the field's table [observed: jg5, location 7 reads 7 and gives 1; max time 2.5 gives 1]; the kill camera keeps the shot's last valid code. A number that is not finite changes nothing [hypothesis] |
| GetBodyPartTraitString `bpd part trait` | 1 part node (BPNN), 2 VATS target (BPNT), 3 IK start node (BPNI), 4 part name (BPTN), 5 gore target bone (NAM4) of the part of that type (BPND part type) [documented: the tables; the subrecords: schema combat.json; observed: jg5, DefaultBodyPartData and NVGecko, trait 3 of the torso reading the skeleton path its BPNI holds]; "" for a part the data lacks or another trait [observed: jg5, traits 0 and 6]. A part starts at its BPTN, or at a BPNN when the part before has its BPND [documented: xEdit BPTD order, as `GameData::body_part_name`] |
| GetTimePlayed `[unit]` | 0 milliseconds, 1 seconds, 2 minutes since the executable started [documented; observed: jg5, the ratios 1000 and 60]; no unit reads milliseconds, another unit 0 [observed: jg5, unit 3]. NVForever counts from the first game the process makes (`Game::new`) |
| GetRGBColor `r g b` | R × 65536 + G × 256 + B [observed: jg5, 255 128 1 reads 16744449, 1 0 0 65536, 0 1 0 256, 0 0 1 1, 255 255 255 16777215]; returns 0 when any channel is outside 0..=255 [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 with the 10 official base masters, clean-room script probe jg_16_rgb_color_out_of_range (GetRGBColor): (256,0,0), (0,256,0), (-1,0,0) -> 0,0,0; no DLL or source consulted] |

The kill camera (`engine/nvf-play/src/kill_cam.rs`) plays a camera shot as a script
changed it (`nvse_jg5::camera_shot`).

Follow-ups: the drawn models read their alternate textures from the plugins
(texture-swaps.md), so the alt-texture setters reach the getters only; the world map reads
WRLD ICON from the plugins; the Pip-Boy's place line does not read region map names yet;
the kill camera picks which shots of a path are hit shots from the record's action, not a
changed one; nothing reads a projectile's sounds yet.

Not in this batch, still on the census: HSVtoRGB and RGBtoHSV (outputs into script
variables), GetCustomMapMarkerIcon and SetCustomMapMarkerIcon (the default icon path is not
documented), and the functions that need engine systems NVForever lacks (NiNode and
texture queries, navmesh obstacles, HUD and menu hooks, the event handlers).
