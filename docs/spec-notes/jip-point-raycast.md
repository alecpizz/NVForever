# JIP LN `GetPointRayCastPos` direction angles, miss output, range, and collision flags

Plain-language notes for `GetPointRayCastPos` in `crates/fnv-game/src/nvse_jip3.rs`.
No JIP LN DLL or implementation source was consulted.

## 1. Signature and documented behavior

- `(success:bool) GetPointRayCastPos originX originY originZ rotationX rotationY xOut yOut zOut [collisionFlag] [maxRange]`
  casts a ray from `(originX, originY, originZ)` in the direction defined by `(rotationX, rotationY)`
  (degrees) up to `maxRange` and writes the resulting world-space coordinates into the three
  output script variables `(xOut, yOut, zOut)`
  [documented: GECK wiki
  [GetPointRayCastPos](https://geckwiki.com/index.php?title=GetPointRayCastPos);
  `schemas/script/functions.json`].
- Earlier runtime probes (`ray1` / `ray2`, `tools/capture/jip-probes/ray1.steps.json`) passed
  bare declared variable names (`fX fY fZ`), which xNVSE's runtime compiler refuses for
  `ScriptVar` output parameters. Wrapping the declared output variables in parentheses
  (`(ox) (oy) (oz)`) compiles and executes cleanly in runtime-compiled UDFs via
  `tools/capture/nvse/catalogs/wave6_point_raycast.json`.

## 2. Direction angle convention

- **Direction angles `[observed]`:** `GetPointRayCastPos` casts along right-handed Euler
  `Rz(rotZ) · Rx(rotX) · +Y = (-sin(rotZ)·cos(rotX), cos(rotZ)·cos(rotX), sin(rotX))`
  (equivalent to `facing(-rotX, -rotZ)` — pitch positive up, yaw counter-clockwise from `+Y`),
  rather than `GetAngle`'s down-positive pitch and clockwise heading
  [observed: 2026-11-24, Fallout: New Vegas 1.4.0.525 (`040020D0`) + xNVSE 6.4.9 + JIP LN NVSE
  57.30 (`5730`) + lStewieAl's Tweaks 1005 (`bUseConsoleOutputFile=1`), 10 official base masters,
  isolated clean-room staging copy (read-only `PrintToConsole` probe; zero writes to game install
  or Saves), start `GSDocMitchellHouse` (`0x00103DF9`) → `Player.MoveTo` Goodsprings map marker
  (`0x000CDF51`, exterior `IsInInterior == 0`), cast at `zSky = pz + 500` with `collisionFlag = 19`,
  `maxRange = 300`, and downward/upward from `zLow = pz + 100`, committed probe
  `w6_01_point_raycast_angles` (`tools/capture/nvse/catalogs/wave6_point_raycast.json`):
  `PROBE|w6_01_point_raycast_angles|int=0|a0_0=1(0.0,300.0,0.0)|a0_90=1(-300.0,-0.0,0.0)|a0_n90=1(300.0,-0.0,0.0)|a0_180=1(0.0,-300.0,0.0)|a90_0=1(0.0,-0.0,300.0)|an90_0=1(0.0,-0.0,-300.0)|a45_0=1(0.0,212.1,212.1)|an45_0=1(0.0,212.1,-212.1)|a30_n90=1(259.8,-0.0,150.0)|dnHit=1(-101.1)|upHit=1(300.0)`;
  no DLL, PDB, or third-party source consulted].
  - `(0, 0)` → `(0.0, +300.0, 0.0)` (`+Y` North)
  - `(0, 90)` → `(-300.0, 0.0, 0.0)` (`-X` West)
  - `(0, -90)` → `(+300.0, 0.0, 0.0)` (`+X` East)
  - `(0, 180)` → `(0.0, -300.0, 0.0)` (`-Y` South)
  - `(90, 0)` → `(0.0, 0.0, +300.0)` (`+Z` Up)
  - `(-90, 0)` → `(0.0, 0.0, -300.0)` (`-Z` Down; hits Goodsprings terrain at `dz = -101.1` from `pz + 100`)
  - `(45, 0)` / `(-45, 0)` → `(0.0, +212.1, ±212.1)` (up-positive pitch)
  - `(30, -90)` → `(+259.8, 0.0, +150.0)` (confirms `Rz(rotZ) · Rx(rotX) · +Y` composition order)

## 3. Miss output, return value, and `maxRange`

- **Miss output coordinates and return value `[observed]`:** Pre-seeding `(ox, oy, oz) = (111, 222, 333)`
  before an open-sky miss at `maxRange = 300` overwrites all three output variables with the ray
  endpoint `origin + dir * maxRange` (`d = 300.0`, `dxyz = (0.0, -0.0, 300.0)`) and returns **`1`**
  (not `0` — every open-sky miss in `w6_01`, `w6_02`, and `w6_03` returned `1`)
  [observed: 2026-11-24, same FNV 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 staging run, Goodsprings
  exterior (`0x000CDF51`, `IsInInterior == 0`), `zSky = pz + 500`, pitch `90`, yaw `0`, committed
  probe `w6_02_point_raycast_miss_and_range` (`tools/capture/nvse/catalogs/wave6_point_raycast.json`):
  `PROBE|w6_02_point_raycast_miss_and_range|int=0|m300=1(d=300.0,dxyz=0.0,-0.0,300.0,raw=-70992.0,2560.0,9167.5)|rng0=1(d=0.0,dz=0.0,oz=8867.5)|rngN1=1(d=1.0,dz=-1.0,oz=8866.5)|om9=1(d=50000.0,dz=50000.0)|om8=1(d=50000.0,dz=50000.0)`;
  no DLL, PDB, or third-party source consulted].
- **`maxRange` default and non-positive values `[observed]`:**
  - When `maxRange` is **omitted** (both 9-argument `om9` with `collisionFlag = 19` and 8-argument
    `om8` with both optional arguments omitted), default reach is **`50_000.0`** game units
    (`d = 50000.0, dz = 50000.0`, return `1`).
  - When `maxRange` is explicitly **`0`** (`rng0`), JIP LN does **not** fall back to `50_000.0`:
    it uses `reach = 0.0`, writing `origin` (`d = 0.0, dz = 0.0, oz = 8867.5 == zSky`) and
    returning `1`.
  - When `maxRange` is explicitly **`-1`** (`rngN1`), JIP LN does **not** fall back to `50_000.0`:
    it uses `reach = -1.0`, casting backward to `origin + dir * (-1)` (`d = 1.0, dz = -1.0,
    oz = 8866.5 == zSky - 1.0`) and returning `1`.
- **Headless / unprobed failure return `[hypothesis]`:** Conditions under which JIP LN returns `0`
  (e.g., invalid cell / no loaded physics world) were not triggered in loaded exterior/interior
  cells; NVForever returns `1` on headless calls without a lent physics caster (`[hypothesis]`).

## 4. `collisionFlag` filtering

- **Observed flag behavior `[observed]`:** Across Goodsprings exterior terrain (`gnd`, `-90, 0`
  from `pz + 100`, range `300`), a spawned NPC actor (`npc`, `0x00104C0C` placed 120 units East,
  horizontal `(0, -90)` from `nx - 80, ny, nz + 60`, range `200`), and `GSDocMitchellHouse`
  interior (`int1`, `Player.MoveTo 0x00104C0F`, `-90, 0` from `pz + 100`, range `300`):
  - `collisionFlag` **`0`, `1`, `6`, `-1`**: hit exterior terrain (`dz = -101`), hit the spawned
    NPC (`dx = 67`), and hit the interior actor (`dz = -12`).
  - `collisionFlag` **`8`, `19`**: hit exterior terrain (`dz = -101`), **skip the spawned NPC**
    (reaching the `200`-unit endpoint, `dx = 200`), and **skip the interior actor** to hit the
    interior static geometry below (`dz = -77`).
  [observed: 2026-11-24, same FNV 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30 staging run, committed
  probe `w6_03_point_raycast_collision_flags` (`tools/capture/nvse/catalogs/wave6_point_raycast.json`):
  `PROBE|w6_03_point_raycast_collision_flags|gnd(0=1:-101,1=1:-101,6=1:-101,8=1:-101,19=1:-101,-1=1:-101)|npc(0=1:67,1=1:67,6=1:67,8=1:200,19=1:200,-1=1:67,aw=1)|int1(0=1:-12,1=1:-12,6=1:-12,8=1:-77,19=1:-77,-1=1:-12)`;
  no DLL, PDB, or third-party source consulted].
- **Engine filter mapping `[design]` / `[hypothesis]`:** NVForever maps `collisionFlag` `8` and
  `19` to existing `CastKind::CameraClearance` (terrain + static collision, excluding actors,
  `no_actors: true`), and other values (including `0`, `1`, `6`, `-1`, and omitted) to
  `CastKind::World` (terrain + static + dynamic + non-player actors). Because
  `CastKind::CameraClearance` also excludes dynamic bodies (`no_dynamic: true`) and other
  Havok layer numbers (`2..=5`, `7`, `9..=18`, `20+`) were not probed, dynamic-clutter filtering
  for `8`/`19` and all unprobed flag numbers remain `[design]` / `[hypothesis]`.
