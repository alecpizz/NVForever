# SUP NVSE batch 3: the fly camera, calculated positions, INI readers (2026-11-07, #1008)

21 SUP functions: the fly (TFC) camera's position, angles and raw rotation (12), the
angles from the camera to a point (2), positions at an angle and distance (2) and the
INI readers (5). Code: `crates/fnv-game/src/nvse_sup3.rs` (camera and positions),
`crates/fnv-game/src/mod_files.rs` (INI), the camera's place in
`crates/fnv-game/src/functions/camera.rs` (`CameraRequests::tfc`) and the engine's side
in `engine/nvf-play/src/free_cam.rs`. Tests: `crates/fnv-game/tests/sup3.rs`.

Probed first, then built: set `sup3` (46 rows) in the original game, SUP installed,
from the save nvftest [observed: golden `jip/sup3.json`, private]. Lab `L5.ext.sup3`
(§5).

## 1. The camera's place

| Function | NVForever |
|---|---|
| GetTFCPos `axis`, SetTFCPos `axis value`, SetTFCPosEx `x y z`, GetTFCPosEx `x y z` | the camera's position in game units [documented]. Never used it reads 0 0 0; turned on it starts at the player's eye, 118 above the feet in first person; it keeps its last place when turned off [observed: sup3 cam.off.pos, cam.on.pos, cam.off2.pos]. Setters return 0 and the next frame reads the same [observed: cam.setpos, cam.setpos.next]. `GetTFCPosEx` returns 0 and writes `(0, 0, 0)` initially and `(100, 200, 300)` after `SetTFCPosEx 100 200 300` [observed: 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), FNV 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 + JohnnyGuitar 528 + ShowOff 184 + SUP NVSE 855 + Stewie 1005, 10 official base masters, `tools/capture/nvse/catalogs/wave6_t1_sup_tfc_calc.json` (`w6_t1_12_sup_tfc_and_mouse`): `PROBE\|w6_t1_12_sup_tfc_and_mouse\|tfcP0=0(0.0,0.0,0.0)\|tfcP2=(100.0,200.0,300.0)\|tfcA=0(15.00,90.00)\|tfcR=0(0.2618,1.5708)\|mPos=0(111,222)\|mMov=0(0.0,0.0)`] |
| GetTFCAngle `axis`, SetTFCAngle `axis value`, …AngleEx | X (down positive) and Z (the heading, 0 north, clockwise) in degrees, as GetAngle gives them: turned on, the player's angles [observed: cam.on.angles]; values kept as given, 400 and -100 not wrapped [observed: cam.setangle.wide]. Y: 0, and setting it does nothing [observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + SUP NVSE 855 with the 10 official base masters, clean-room script probe `sup_05_tfc_angle_rot_y_z` (`GetTFCAngle`/`GetTFCRot`): `0.000,0.000,0.000`; no DLL or source consulted]. `SetTFCAngleEx 15 90` followed by `GetTFCAngleEx` returns 0 and writes `(15.00, 90.00)` [observed: 2026-11-24, `w6_t1_12_sup_tfc_and_mouse`] |
| GetTFCRot `axis`, SetTFCRot `axis value`, …RotEx | "not the actual angle value" [documented]: X is angle X in radians and Z is angle Z in radians; Y reads 0 and setting it changes nothing [observed: cam.on.angles, cam.setangle.*, cam.setrot.y (9.1804° read 0.1602, 30° 0.5236, 0.5 set reads 28.6479°); earlier `sup_05_tfc_angle_rot_y_z` read `0.000` on Z before any Z rotation was set; observed: 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), FNV 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 + JohnnyGuitar 528 + ShowOff 184 + SUP NVSE 855 + Stewie 1005, 10 official base masters, `tools/capture/nvse/catalogs/wave6_t1_sup_tfc_calc.json` (`w6_t1_12_sup_tfc_and_mouse` & `w6_t1_12b_sup_tfc_rot_set`): after `SetTFCAngleEx 15 90`, `GetTFCRotEx` returns 0 and writes `(0.2618, 1.5708)`; after `SetTFCRotEx 0.5 1.0`, `PROBE\|w6_t1_12b_sup_tfc_rot_set\|rotEx=(0.5000,1.0000)\|angEx=(28.65,57.30)\|rotXYZ=(0.5000,0.0000,1.0000)`; no DLL or source consulted; hypothesis: `SetTFCRot Z` sets angle Z in radians symmetrically with `GetTFCRot Z` and `SetTFCRotEx`] |
| GetCalculatedAngleXForTFCCamera `x y z` | the angle X to look at the point from the camera: −atan2(rise, distance across), up negative [observed: cam.calc.x, -45, 45, 0, -53.1301] |
| GetCalculatedAngleZForTFCCamera `x y` | the heading of the point from the camera: atan2(east, north), straight south -180 [observed: cam.calc.z, 90 0 -90 -180 45] |

The `…Ex` getters write their results into the script's variables (the VM's
`out_params`, as GetAngleEx). The game's runtime compiler refused bare-variable calls of
them [observed: sup3 cam.setposex, cam.angleex, cam.rotex, "script did not compile"; it
compiled SetTFCPosEx], while parenthesised `(var)` out-var calls compile and execute in
the original game [observed: 2026-11-24, `wave6_t1_sup_tfc_calc.json`].

**The engine** (`nvf_play::free_cam`): turning the camera on writes its own place into
`Game::ui.camera.tfc`; a script's change (`tfc_set`) moves the camera on the next update;
the viewer's moves are written back. A place a script set is kept exactly (the pitch
limit of ±1.55 rad applies only while the mouse moves it), so an angle of 400 reads back
400 as in the original [design]. Game space to the camera: `space::to_bevy`, yaw −heading,
pitch −angle X [documented: `space::look_rotation`]. Not checked in the running engine by
a smoke scenario yet (follow-up: a `tfc` scenario reading GetTFCPos after a script sets
it).

## 2. Positions at an angle and distance

| Function | NVForever |
|---|---|
| `ref.GetCalculatedPos angle dist xOut yOut` | the point `dist` along heading `angle` from the reference: x + dist·sin, y + dist·cos, returning 0 [documented: the page's "in front of the player" with GetAngle Z; observed: 2026-11-24, `GSDocMitchellHouse` (`0x00103DF9`), FNV 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 + JohnnyGuitar 528 + ShowOff 184 + SUP NVSE 855 + Stewie 1005, 10 official base masters, `tools/capture/nvse/catalogs/wave6_t1_sup_tfc_calc.json` (`w6_t1_11_sup_calc_pos`): `PROBE\|w6_t1_11_sup_calc_pos\|p0=0(0.0,100.0)\|p90=0(100.0,0.0)\|a0=0(1000.0,2100.0)\|a45=0(1070.7,2070.7)\|a90=0(1100.0,2000.0)\|a180=0(1000.0,1900.0)\|am90=0(900.0,2000.0)`; no DLL or source consulted] |
| GetCalculatedPosAlt `angle dist xOut yOut xIn yIn` | the same from the point, returning 0 [documented; observed: 2026-11-24, `w6_t1_11_sup_calc_pos`] |

Both write their outputs into the script's variables; bare variable names did not compile
in `sup3` [observed: calc.pos.*, calc.alt.*], while parenthesised `(ox) (oy)` calls
compile and confirm the exact heading convention [observed: 2026-11-24,
`w6_t1_11_sup_calc_pos`].

## 3. The INI readers

| Function | NVForever |
|---|---|
| ReadINIFloatFromFile `path section key` | the value as a number, the path from the game folder [documented]; 0 for a missing key or file [documented; observed: ini.float.missing] |
| ReadINIFloatFromFileAlt | the same, NaN when missing [documented; observed: ini.alt.missing, ini.alt.nofile] |
| ReadINIStringFromFile | the value's text, "" when missing [documented; observed: ini.string, ini.string.missing] |
| ReadINISectionKeysFromFile `path section` | the section's keys, sorted [observed: ini.keys: JohnnyGuitar.ini's nine [MAIN] keys come first bFixDeathVoicelines, last bReset3rdPersonCamera, which is sorted order, not the file's; observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + SUP NVSE 855 with the 10 official base masters, clean-room script probe `sup_07_read_ini_sections_sorted` (`ReadINISectionsFromFile`/`ReadINISectionKeysFromFile`): `3,Alpha,2,ka`; no DLL or source consulted] |
| ReadINISectionsFromFile `path` | the file's sections, sorted as the keys [observed: 2026-11-23, `GSDocMitchellHouse` (`0x00103DF9`), `sup_07_read_ini_sections_sorted`: `3,Alpha`]; no file gives no array [observed: ini.sections.nofile, ar_Size -1] |

All of them: the section and key in any case [observed: ini.float.case, "Main" for
[MAIN]], a UTF-8 byte order mark before the first section [observed: ini.float.bom,
supNVSE.ini], a space before a section's bracket [observed: ini.float.space,
jip_nvse.ini], `\` or `/` in the path [observed: ini.float.backslash], and a `;` comment
after a value. The file is read through `ModFiles::read_root`: NVForever's own copy
first, then the game folder's.

**Decision**: a path that leaves the game folder (`..`, a drive, a root) is not read and
gives the missing value, by `mod_files::on_disk` (the charter; PR #1291's review). The
original read `../Fallout New Vegas/Data/NVSE/Plugins/supNVSE.ini` [observed:
ini.float.outside, 1]. For review.

## 4. A compiler fix found on the way

With OldWorldBlues.esm and LonesomeRoad.esm loaded, a bare `Z` in an extender function's
axis argument compiled as the placed reference whose editor ID is `Z` (each DLC has one),
not as the axis, so `SetTFCAngle Z 45` set nothing in NVForever while the original read
back 45 [observed: sup3 cam.setangle.z]. An extender function without an opcode now takes
`X`, `Y` or `Z` as the axis where the catalogue gives the parameter as an axis, or its
wiki type is axis letters (`X/Z`) (`compile/gen/x.rs` `axis_param`). The 19 catalogue
functions with axis parameters gain it, JIP's GetLocalGravity, Get/SetActorVelocity and
the NiNode functions among them.

## 5. Lab (2026-11-07)

`L5.ext.sup3`: 29 of 35 equal; 6 changed and 11 extra, each explained:
- `cam.off.pos`, `cam.off.player`, `cam.on.angles`, `cam.setangle.x`: the save's player
  stands and faces elsewhere than the headless start (the rows read the player's place
  and angles, which the camera takes when it turns on).
- `cam.off2.angles`: it reads what the earlier rows left; `cam.angleex` and `cam.rotex`
  did not compile in the game, so their SetTFCAngleEx and SetTFCRotEx never ran there.
- `ini.float.outside`: the decision of §3.
- 11 extra rows: lines the game's compiler refused (the `…Ex` getters, GetCalculatedPos
  and …Alt); NVForever runs them.

## 6. Not in this batch

SUP's other 101 names: Excel, JSON, file times and sizes, the clipboard, screenshots,
dialog boxes, the web and Nexus functions, the cursor, mouse and HUD flags, debug text,
NIF block interpolation, the topic setters of batch 2 (#1817) and the rest. The file
writers and the machine's own state (clipboard, windows, the web) need a policy first
(ext-sup1.md).
