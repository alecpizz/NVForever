# JohnnyGuitar NVSE, batch 10 (2026-11-09, #1007)

The last 26 JohnnyGuitar functions the census lists once batch 9 (#1930) and the NIF and
camera slice (#1928) are counted, in `crates/fnv-game/src/nvse_jg10.rs`. Sources: the
GECK wiki export [documented: the page named, in our own words], the ESM records, and
NVForever's own systems where a page leaves a value open [hypothesis, with the reason].
Probe set `jg10` (`tools/capture/jip_probes.py`, lab scenario `L5.ext.jg10`) was run in the
original on 2026-11-09; §4 has its answers. Tests: `crates/fnv-game/tests/jg10.rs`.

26 functions. Census: JohnnyGuitar 78 → 52 on main, 0 with #1928 and #1930.

The batch's state is the session's (`OtherState::jg10`) and is not saved:
DisableMuzzleFlashLights "does not persist through saves" and HighlightBodyPartAlt is
"session-persistent" [documented]; the rest is a session's too [hypothesis].

## 1. The functions

| Function | What it does here |
|---|---|
| AddNavmeshObstacle, RemoveNavmeshobstacle | The calling reference joins or leaves the script obstacle list [documented]. Returns 1 both on a reference [observed: §4 returns] and without a calling reference [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `w4c_b3_10_jg10_safe_edges` (`tools/capture/nvse/catalogs/wave4c_extenders.json`, #3679): `obsNoRef=1/1,farTri=4,dm(4.29497e+09,0),fg(badT=0,oob=1),refIdle=0,spl0=1`; no DLL or source consulted]. The engine does not cut the navmesh round it yet (§3). |
| GetNearestNavMeshTriangle `x y z skipDisabled [zLimit]` | [x, y, z, distance] of the nearest loaded triangle's centre [documented]. Asked of the engine's navmesh through `nav_ask` (a new `triangle` question); an empty array when there is no answer [hypothesis]. |
| GetPointInNavMesh `x y z skipDisabled [zLimit]` | The same for the triangle whose area holds the point seen from above [documented]. |
| PathToPoint `x y z [radius] [angle]`, PathToRef `target [radius] [face]` | The actor walks there and keeps its package [documented]. NVForever queues the request for the engine (§3). Returns `1.0` when queued on a valid actor/target [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528, 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, read-only `PrintToConsole` probe `b3_jg10_01_navmesh_and_path_rets` (`tools/capture/nvse/catalogs/wave4b_extenders.json`, #3679, calling `rDoc.PathToPoint` and `rDoc.PathToRef Player`): `PROBE\|b3_jg10_01_navmesh_and_path_rets\|obs=1/1,triSz=4,inSz=4,pp=1.0,pr=1.0`; no DLL or source consulted], and `0` with no actor or target [hypothesis: invalid/missing actor or target unprobed]. A new request replaces the actor's last one. |
| ApplyWeaponPoison `[poison]` | Puts the poison on the actor's equipped weapon or the weapon's inventory reference; none, or a null form, takes it off [documented]. Requires an `ALCH` poison (`Game::is_poison`: Hostile + Detrimental effects): applying a real poison (`BleakVenom` `0x001613D0`, `SilverSting` `0x00167475`) or clearing with an omitted argument returns `1`, while passing non-poison `ALCH` (`Stimpak` `0x0015169`, `GetIsPoison == 0`), passing a non-`ALCH` form (`Weap10mmPistol` `0x434F`, leaving an already-applied poison intact), or calling unarmed without an equipped `WEAP` returns `0` [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 + JIP LN 57.30, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, isolated clean-room staging copy (read-only `PrintToConsole` probes; zero writes to game install or Saves), `w4h_09_apply_weapon_poison_stimpak` (`tools/capture/nvse/catalogs/wave4h_reconcile.json`, PR #3803): `PROBE\|w4h_09_apply_weapon_poison_stimpak\|eq=00004326\|isPois(b=1,s=1,st=0)\|init=00000000\|bleak(1:001613D0)\|clr1(1:00000000)\|stim(0:00000000)\|silver(1:00167475)\|clr2(1:00000000)`, and `w6t2p2_27_jg10_apply_poison_and_muzzle_hex` (`tools/capture/nvse/catalogs/wave6_t2_observed_matches.json`): `PROBE\|w6t2p2_27_jg10_apply_poison_and_muzzle_hex\|unarm=0,ok=1,bad=0,kept=1,clr=1\|muz=4294967295,neg=4.29497e+09`; zero DLL, PDB, or third-party source consulted]. GetEquippedWeaponPoison (JIP batch 11) reads it. The hit does not apply it yet (§3). |
| EjectCasing `[node] [model]` | A casing from the reference's weapon [documented]. On the player, the engine throws the weapon's casing from ShellCasingNode on its next combat frame. |
| DisableMuzzleFlashLights `[mode]` | 0 on, 1 off for all, 2 the player's off, 3 NPCs' off [documented]. nvforever's `shot_fx` leaves the flash's light out. A mode outside 0–3 leaves the setting unchanged and returns exact `u32::MAX` (`4_294_967_295`, `0xFFFFFFFF`, printed as `FFFFFFFF` under `%x` and `4.29497e+09` under `%g`) [observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, read-only `PrintToConsole` probes `w4c_b3_10_jg10_safe_edges` (`tools/capture/nvse/catalogs/wave4c_extenders.json`, #3679): `dm(4.29497e+09,0)`, `w4h_06_disable_muzzle_flash_hex` (`tools/capture/nvse/catalogs/wave4h_reconcile.json`, #3803): `PROBE\|w4h_06_disable_muzzle_flash_hex\|dm99(hex=FFFFFFFF,i=00000000,g=4.29497e+09)\|dm1(1,1)\|dm0(0,0)\|dt(c1=1,ex1=1,c2=-1,ex2=1,ex3=0)\|getOps(0,0,0)`, and `w6t2p2_27_jg10_apply_poison_and_muzzle_hex` (`tools/capture/nvse/catalogs/wave6_t2_observed_matches.json`): `PROBE\|w6t2p2_27_jg10_apply_poison_and_muzzle_hex\|unarm=0,ok=1,bad=0,kept=1,clr=1\|muz=4294967295,neg=4.29497e+09`; zero DLL, PDB, or third-party source consulted]. |
| SetWeaponScopeUIModel `path [weapon]` | The scope picture; a weapon gives its own MOD3 and the path is ignored [documented]. |
| ToggleWeaponScopeUIModel `visible` | Hides (0) or shows (1) the scope [documented]. |
| ClearWeaponScopeUIModel | Hides the scope and takes its picture away [documented]. |
| RemoveScopeModelPath `weapon` | Clears the weapon's MOD3 (as SetScopeModelPath's override) [documented]. Returns 0 when the weapon has no scope [observed: §4 scope.remove; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 with the 10 official base masters, clean-room script probe jg_35_remove_scope_model_path_ret (RemoveScopeModelPath): 0; no DLL or source consulted], 1 when it had one [hypothesis]. |
| FaceGenGetNthProperty `base type n` | The base's nth FaceGen value of the type. Type 0 is FGGS (50 values), 1 FGGA (30), 2 FGTS (50) [hypothesis: the page leaves the types undocumented; this is the records' order, xEdit wbFaceGen; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528 with the 10 official base masters, clean-room script probe jg_36_facegen_property_type_mapping (FaceGenGetNthProperty/FaceGenSetNthProperty): -0.2281,0.0000,-0.2281,1.00,0.2500; no DLL or source consulted — reproduces §4 open finding that types 0 and 2 both read -0.2281 and set/get round-trips 0.2500; observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JohnnyGuitarNVSE 528, 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), 10 official base masters, `w4d_31_jg_facegen_bounds` (`tools/capture/nvse/catalogs/wave4d_extenders.json`): `t0(0=-0.2281,49=0.0000)\|t1(0=0.0000)` on Doc Mitchell (`0x00104C0C`); no DLL or source consulted]. The player's comes from the character creator's look. Returning 0.0 for out-of-range indices (`>= 50` / `>= 30` / `< 0`) or invalid types (`>= 3`) is NVForever's crash-safety guard avoiding out-of-bounds reads, which crash/hang JohnnyGuitarNVSE 528 [design]. |
| FaceGenSetNthProperty `base type n value` | Sets it. On the player, in the look (drawn again); on an NPC, as the base's record value [documented: the arguments]. Invalid type returns 0, while a valid type with an out-of-bounds index leaves data unchanged and returns 1 [observed: `w4c_b3_10_jg10_safe_edges` (`fg(badT=0,oob=1)`)]. |
| FaceGenRefreshAppearance | The reference is drawn again (an appearance change, as SexChange's) [documented]. |
| HighlightBodyPartAlt `[part]` | One actor at a time; no part, or 15, is everything but the weapon [documented]. A part outside 0–15 is not taken [hypothesis]. |
| DeactivateAllHighlightsAlt | Every highlight goes [documented]. |
| RemoveHighlightedRef | The calling reference's highlight goes [hypothesis: the page is empty; read from its name]. |
| GetLandTextureUnderFeet | The land texture (LTEX) under the reference; nothing indoors [documented], though the original gave LRockCanyonRubbleDirt01 (00038A28) inside Doc Mitchell's house [observed: §4 underfeet], taken as the last texture found under the reference outside [hypothesis]. A cast straight down from 64 units above (`ray::CastKind::LandTexture`). The engine answers with the texture showing most where it meets the terrain, from the LAND's BTXT/ATXT layers, as footsteps choose their sound. It gives 0 where the default texture shows more [hypothesis]. |
| RemapLand `land world x y` / `land` | The LAND takes the cell's landscape; the form alone puts it back [documented]. Returns 1 when done. It returns 0 for a source cell that does not exist, or a reset of a LAND never remapped [hypothesis]. The game's data keeps no LAND record types, so any form of no other known type is taken as a LAND [hypothesis]. The terrain is not redrawn yet (§3). |
| RefreshIdle `[stop]` | The actor picks its own idles again after PlayIdle; 1 stops the one playing at once [documented]. NVForever's PlayIdle holds no override past its idle, so the stop (StopIdle's) is all that is left to do [hypothesis]. |
| ShowBarberMenuEx `flag list` | The barber menu (1036). 0 blacklists hairs and beards, 1 blacklists beards and whitelists hairs, 2 the reverse, 3 whitelists both [documented]. `Game::barber_allows` answers for the menu. A flag outside 0–3 or no list opens it unfiltered [hypothesis]. The filter is cleared when menu 1036 closes and by a plain ShowBarberMenu, so a later barber menu is unfiltered (#2018) [design]. |
| TriggerScreenSplatterEx `count duration size opacity alpha colour flare fade` | TriggerScreenSplatter's splats; the textures and the fade are kept for the engine [documented]. Returns 1 both when splats are queued and when `count == 0` (without queuing splats) [observed: `w4c_b3_10_jg10_safe_edges` (`spl0=1`)]. |
| UpdateCrosshairPrompt, UpdateRepairMenu | Counted (`Game::ui_refreshes`) [documented: what they do]. The engine is to work the prompt or the repair list out again on each new count; it does not read the counts yet (§3, settled in #2018: no engine code reads `ui_refreshes`). |

The functions that change something and document no return value return 0.

## 2. Data

The NPC and CREA records now also keep FGGS, FGGA and FGTS among their decoded subrecords
(`ACTOR_SUBS`, data.rs), for FaceGenGetNthProperty.

## 3. Follow-ups (engine)

The game side of each is built; the engine does not read these yet:

- The navmesh answerer: `nav_ask::with_nav` around the script tick (ITR's follow-up,
  ext-itr.md §12), now with `triangle` too. Until it is lent, the triangle functions
  give empty arrays in the game.
- Script obstacles (`Game::script_nav_obstacles`) cut into the nav graph
  (`fnv_nav::obstacles`).
- PathToPoint and PathToRef (`Game::take_path_requests`): the walker goes there
  without a new package.
- A weapon's poison applied by its hits (`Game::weapon_poison`), and how many hits it
  lasts.
- EjectCasing's own node and model, and other actors' casings.
- The scope picture and its hiding (`Game::script_scope`) in `aim_zoom` / `scope_view`.
- An NPC's FaceGen values set by script, drawn from the base's override when the head
  is built.
- VATS body part highlights (`Game::body_part_highlight`).
- RemapLand's swaps (`Game::land_remaps`) in the terrain.
- The barber menu's filter in the creator's hair and beard lists.
- TriggerScreenSplatterEx's textures in `screen_splatter`.
- The prompt and repair menu refreshes.

## 4. Probe answers (set jg10, run 2026-11-09)

On the player at Doc Mitchell's (an interior), in the original game with the extenders
[observed], and what NVForever does with each:

| Probe | Original | NVForever |
|---|---|---|
| `tri`: GetNearestNavMeshTriangle at the player, size and distance under 200 | `4 1` | the engine's navmesh (§3: no answerer lent yet); the records-only lab candidate gives `0` |
| `tri.under`: GetPointInNavMesh at the player and 100,000 units east | `4 4` | as `tri`; far off the navmesh still gives four values in the original |
| `underfeet`: GetLandTextureUnderFeet indoors | `00038A28`, LRockCanyonRubbleDirt01 | the last texture found under the reference outside [hypothesis]; the lab candidate has found none, `00000000` |
| `facegen`: types 0-2 index 0, FGGA index 30, type 3 in one step | crashed the game | split into one read per type, last in the set; the out-of-range reads are not asked again |
| `facegen.fggs`, `.fgga`, `.fgts`: index 0 of types 0, 1, 2 | `-0.228`, `0.000`, `-0.228` | `0.309`, `0.000`, `-1.245`, the record's FGGS, FGGA, FGTS; open (below) |
| `facegen.set`: set FGGS 1 to 0.5, read it, put the first read back, read again | `1 0.500 0.605` | `1 0.500 0.508`, the record's FGGS 1; open (below) |
| `returns`: HighlightBodyPartAlt, RemoveHighlightedRef, DeactivateAllHighlightsAlt, Add/RemoveNavmeshObstacle, UpdateCrosshairPrompt, UpdateRepairMenu, DisableMuzzleFlashLights 0 | `0 0 0 1 1 0 0 0` | the obstacle functions now return 1 |
| `scope.remove`: the 10mm pistol, no scope | `0` | `0` |
| `remapland`: a reset of nothing | `0` | `0` |

FaceGen stays open [observed: the answers above; #2018 rechecked, the golden holds only these rows, so it cannot settle the type mapping]. The original's values are not the
record's (Doc's FGGS starts 0.309, 0.508), not his race's defaults (CaucasianOld's male
FGGS starts -0.204, 0.715), and types 0 and 2 both read -0.228 at index 0. So the types may
not be FGGS/FGGA/FGTS, or the values may be ones the game works out for the loaded face
[hypothesis]. NVForever keeps reading the record until a probe tells more.

Left out, because they move actors, open menus, draw on the screen or need a poison form
not listed here: PathToPoint, PathToRef, ShowBarberMenuEx, TriggerScreenSplatterEx,
ApplyWeaponPoison, EjectCasing and the scope picture functions.
