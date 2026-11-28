# JIP LN `GetAnglesBetweenPoints` and `GetAngleEx`

Plain-language notes for `GetAnglesBetweenPoints` and `GetAngleEx` in
`crates/fnv-game/src/nvse_jip3.rs`. No JIP LN DLL or implementation source was
consulted.

## 1. `GetAnglesBetweenPoints`

- `GetAnglesBetweenPoints x0 y0 z0 x1 y1 z1 resAngX resAngZ` writes the pitch (`resAngX`)
  and heading (`resAngZ`), in degrees, of the vector from `(x0, y0, z0)` to `(x1, y1, z1)`
  and returns `0` [documented: GECK wiki `GetAnglesBetweenPoints`; `schemas/script/functions.json`].
- **Angle signs and range `[observed]`:**
  - `resAngX` (pitch) is **up-positive** (`+Z` upward -> `+90`, `-Z` downward -> `-90`):
    `atan2(dz, hypot(dx, dy))` in degrees.
  - `resAngZ` (heading) is **counter-clockwise from north (`+Y`) in `(-180, 180]`**
    (`N = 0`, `E = -90`, `W = +90`, `S = +180`, `NE = -45`, `NW = +45`, `SE = -135`,
    `SW = +135`): `atan2(-dx, dy)` in degrees.
  - This decomposes the unit direction `Rz(resAngZ) * Rx(resAngX) * +Y` (pitch positive
    up, heading counter-clockwise from `+Y`).
  - Coincident points `(50, 50, 50) -> (50, 50, 50)` overwrite both output variables with
    `(0.0, 0.0)` and return `0`.
  - Provenance: [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525 (`040020D0`) +
    xNVSE 6.4.9 + JIP LN NVSE 57.30 (`5730`) + lStewieAl's Tweaks 1005
    (`bUseConsoleOutputFile=1`), 10 official base masters, isolated clean-room staging
    copy (read-only `PrintToConsole` probe; zero writes to game install or Saves), start
    `GSDocMitchellHouse` (`0x00103DF9`), committed probe `w6_t1_01_angles_between_points`
    (`tools/capture/nvse/catalogs/wave6_t1_jip_angles.json`):
    `PROBE|w6_t1_01_angles_between_points|N=0(-0.00,0.00)|E=0(-0.00,-90.00)|W=0(-0.00,90.00)|S=0(-0.00,180.00)|U=0(90.00,0.00)|D=0(-90.00,0.00)|NE=(-0.00,-45.00)|NW=(-0.00,45.00)|SE=(-0.00,-135.00)|SW=(-0.00,135.00)|P45=(45.00,0.00)|M45=(-45.00,0.00)|DG=(45.00,-90.00)|ZERO=0(-0.00,0.00)`;
    no DLL, PDB, or third-party source consulted].

## 2. `GetAngleEx`

- `ref.GetAngleEx resAngleX resAngleY resAngleZ [getLocal]` writes the reference's Euler
  angles in degrees into the three script variables and returns `0` [documented: GECK wiki
  `GetAngleEx`; `schemas/script/functions.json`].
- **Global (`getLocal = 0` or omitted) vs local (`getLocal = 1`) `[observed]`:**
  - `getLocal = 0` (or omitted) writes the reference's world Euler angles in degrees,
    matching `GetAngle X`, `GetAngle Y`, `GetAngle Z`.
  - `getLocal = 1` decomposes the inverse/local rotation matrix `Rx(-x) * Ry(-y) * Rz(-z)`
    into ZYX Euler angles in degrees (matching all five tested rotations to `0.01°`:
    `(0, 0, 5.70) -> (0, 0, -5.70)`, `(25, 0, 0) -> (-25, 0, 0)`,
    `(0, 0, 90) -> (0, 0, -90)`, `(30, 0, 90) -> (0, -30, -90)`, and
    `(25, 0, 315) -> (-18.25, 17.39, 42.19)`).
  - When called on an invalid/missing reference, output variables are preserved unchanged
    and `0` is returned [hypothesis: not probed in `w6_t1_02_angle_ex_and_heading`].
  - Provenance: [observed: 2026-11-24, same FNV 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30
    isolated clean-room staging run, `GSDocMitchellHouse` (`0x00103DF9`), `DocMitchellRef`
    (`0x00104C0F`), `GSDocMitchellHouseIntDoorREF` (`0x00103E61`), and `Player` rotated
    across `(25, 0)`, `(0, 90)`, `(30, 90)`, `(25, 315)`, committed probe
    `w6_t1_02_angle_ex_and_heading`
    (`tools/capture/nvse/catalogs/wave6_t1_jip_angles.json`):
    `PROBE|w6_t1_02_angle_ex_and_heading|docGA=(0.00,-0.00,5.70)|docEx0=0(0.00,-0.00,5.70)|docEx1=0(0.00,-0.00,-5.70)|loc25_0=(-25.00,-0.00,-0.00)|loc0_90=(0.00,-0.00,-90.00)|loc30_90=(0.00,-30.00,-90.00)|pcGA=(25.00,-0.00,315.00)|pcEx0=0(25.00,-0.00,315.00)|pcEx1=0(-18.25,17.39,42.19)|ha=(84.22,0.00,-0.00)`;
    no DLL, PDB, or third-party source consulted].
