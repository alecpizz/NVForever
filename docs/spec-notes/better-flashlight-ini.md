# BetterFlashlightNVSE INI spotlight configuration

**Clean-room provenance:** Only the public **Better Flashlight NVSE** mod page
(`https://www.nexusmods.com/newvegas/mods/98905`, v1.1.5) and shipped v1.1.5
`Data\Config\BetterFlashlightNVSE.ini` file (section names, key names, defaults, and
inline comments) were consulted. No DLL (`BetterFlashlightNVSE.dll`), PDB, compiled
shader (`.fxo`), or third-party binary or source code was opened, disassembled, or
consulted. Every NVForever-specific mapping, clamp, fallback, or runtime state machine
below beyond what the INI comments state is tagged `[design]` (NVForever's own behavior,
not the plugin's).

## VFS discovery, vanilla-unchanged gate, and Tweaks precedence

(`engine/nvforever/src/flashlight_ini.rs`, `engine/nvforever/src/flashlight.rs`; extends
the native player spotlight described in `world.md` §8.)

- At `Startup`, NVForever looks up `config\betterflashlightnvse.ini` case-insensitively
  through `nvf_assets::GameData::read` (covering loose files, MO2 layered roots, and
  BSAs) [documented: Better Flashlight NVSE v1.1.5
  (`https://www.nexusmods.com/newvegas/mods/98905`), shipped
  `Data\Config\BetterFlashlightNVSE.ini` install path; design: NVForever's own VFS
  lookup].
- When the INI is absent, or when `[Main] RenderEffects = 0` or `[Flashlight] Enabled = 0`
  [documented: Better Flashlight NVSE v1.1.5 `Data\Config\BetterFlashlightNVSE.ini`
  `[Main] RenderEffects = 1` and `[Flashlight] Enabled = 1`
  (`";disable to turn the mod off entirely"`)], NVForever leaves its vanilla Pip-Boy
  `SpotLight` (`COLOUR`, `BRIGHTNESS`, `RANGE`, `OUTER_DEG`, `INNER_DEG`, head/eye
  `placement`) and toggle behavior completely unchanged [design: NVForever's own gate].
- **Precedence vs. NVForever Tweaks Flashlight settings (`#3280`)** [design: NVForever's
  own precedence policy]: when both an active `Data\Config\BetterFlashlightNVSE.ini`
  (`[Main] RenderEffects = 1` and `[Flashlight] Enabled = 1`) and NVForever's Tweaks >
  Quality of Life > Flashlight rows (`#3280`) are present, the active
  `BetterFlashlightNVSE.ini` takes precedence over the Tweaks Flashlight rows so
  installing the mod works out of the box without editing Tweaks; disabling the INI
  (`RenderEffects = 0` or `Enabled = 0`) or removing it immediately restores the Tweaks
  Flashlight settings. Whichever of `#3280` and `#3727` merges second will merge `main`
  and cover this precedence boundary with a regression test.
- Parsing operates strictly on raw `&[u8]` slices (stripping optional UTF-8 BOM,
  CRLF/LF endings, `;`/`#` comments, ASCII case-insensitive section/key matching, and
  finite `f32` validation) with NVForever's own safety clamps (`Angle * SpotSize` clamped
  to `1°..=85°` strictly below Bevy's `π/2` `SpotLight` limit; `SpotSize` `0.1..=4.0`;
  `Distance` `64..=20000`; `ColorR/G/B` `0..=4`; `Dimmer`/`Intensity` `0..=16`;
  `WarmupTime` `0..=10 s`; `Flicker` `0..=2`; `OffsetX/Y/Z` `-200..=200`; `[Battery]`
  rates/threshold `0..=100`) and omitted-key fallback defaults (`SpotSize = 1.0`,
  `[Battery] Enabled = false`, and the shipped v1.1.5 INI's numeric baseline for omitted
  spotlight/battery fields) [design: NVForever's own parser, clamps, and fallbacks].

## Wired sections and keys

Every stored field has a live consumer in `engine/nvforever/src/flashlight.rs`:

- `[Main] RenderEffects` (shipped `1`) and `[Flashlight] Enabled` (shipped `1`,
  `";disable to turn the mod off entirely"`): master activation gate [documented: Better
  Flashlight NVSE v1.1.5 (`https://www.nexusmods.com/newvegas/mods/98905`),
  `Data\Config\BetterFlashlightNVSE.ini`; hypothesis: whether `[Main] RenderEffects = 0`
  also disables the spotlight or only post-process effects is inferred from the top-level
  INI structure, not probed].
- `[Flashlight] Angle` (shipped `18`), `Distance` (shipped `1800`, `";light cone"`), and
  `SpotSize` (shipped `1.5`,
  `";spot size scale - shrinks the pool (cone + cookie together). 1.0 = full Angle, lower = tighter"`):
  [documented: Better Flashlight NVSE v1.1.5 `Data\Config\BetterFlashlightNVSE.ini`].
  Mapped onto NVForever's native Bevy `SpotLight` `outer_angle` (`Angle * SpotSize`,
  treating `Angle` as base cone half-angle in degrees [hypothesis: whether the plugin
  uses half-angle or full-angle is a visual/shader boundary not stated in the INI
  comment]), `inner_angle` preserving NVForever's vanilla `14 / 30` softness ratio
  [design: NVForever's own inner-cone mapping], and `range` (`Distance`, game units).
  `configured_spotlight` preserves `shadow_maps_enabled = true` and
  `shadow_map_near_z = NEAR` so native wall shadow occlusion and `#3121` first-person
  anti-washout scaling (`lights/display_shader.rs`) continue to apply [design: NVForever's
  own renderer policy].
- `[Flashlight] ColorR` (shipped `2`), `ColorG` (shipped `1.85`), `ColorB` (shipped `1.65`,
  `";color (values above 1.0 are HDR). lower the warm bias (R,G) or raise B for a cooler beam"`):
  [documented: Better Flashlight NVSE v1.1.5 `Data\Config\BetterFlashlightNVSE.ini`].
  Passed as HDR-capable display colour multipliers to `lights::display_color` [design:
  NVForever's own native display-unit mapping].
- `[Interior] Dimmer` (shipped `1`), `Intensity` (shipped `0.35`,
  `";brightness used in interior cells"`), `[Exterior] Dimmer` (shipped `3.20`),
  `Intensity` (shipped `0.35`, `";brightness used in exterior cells"`), and `[Flashlight]
  Dimmer` (shipped `1.0`), `Intensity` (shipped `0.35`,
  `";legacy fallback for configs created before interior/exterior profiles"`):
  [documented: Better Flashlight NVSE v1.1.5 `Data\Config\BetterFlashlightNVSE.ini`].
  Dynamically chosen by current cell interior state (`nvf_play::Location::interior`,
  fallback `SkyState::interior`) and mapped to display brightness
  `BRIGHTNESS * dimmer * intensity * warmup * flicker` via `lights::lumens` [design:
  NVForever's own linear product mapping into display units].
- `[Flashlight] WarmupTime` (shipped `0.4`,
  `";warm-up ramp on toggle (seconds). incandescent feel. 0 = instant (LED)"`) and
  `Flicker` (shipped `0.25`,
  `";sporadic flicker - mostly steady with occasional brief dips. 0 = off, higher = deeper/more often"`):
  [documented: Better Flashlight NVSE v1.1.5 `Data\Config\BetterFlashlightNVSE.ini`].
  Implemented as an incandescent smoothstep ramp over `WarmupTime` seconds (`0` = instant)
  and deterministic SplitMix64 `0.08 s` time-slot sporadic brightness dips bounded in
  `0.15..=1.0` whose frequency and depth scale with `Flicker` (`0` = steady) [design:
  NVForever's own ramp and flicker curves].
- `[Flashlight] AttachToWeapon` (shipped `1`,
  `";light follows weapon when armed, otherwise camera"`) and `OffsetX`, `OffsetY`,
  `OffsetZ` (shipped `0.0`,
  `";position offset relative to attach point (weapon or camera)"`): [documented: Better
  Flashlight NVSE v1.1.5 `Data\Config\BetterFlashlightNVSE.ini`]. When enabled and a
  weapon is drawn in hand, NVForever aligns the spotlight to the weapon's `ProjectileNode`
  muzzle and `+X` barrel axis (`PlayerBody::weapon_aim`, falling back to
  `WeaponAnim::muzzle` with view rotation, or camera/head-eye placement when
  unarmed/holstered or when `AttachToWeapon = 0`) [design: NVForever's own weapon-node
  alignment], then applies `(OffsetX, OffsetY, OffsetZ)` in the attach point's local
  game-space `(right, forward, up)` frame [hypothesis: local axis sign convention for
  `OffsetX/Y/Z`, since the INI comment does not specify individual axis orientations].
- `[Battery] Enabled` (shipped `1`), `DrainRate` (shipped `1`), `RechargeRate` (shipped
  `2.5`, `";drains while the Pip-Boy flashlight is on, recharges while off. Charge resets
  to full on load/new game."`), and `RechargeThreshold` (shipped `10`,
  `";charge needed after full depletion before the flashlight can turn on again"`):
  [documented: Better Flashlight NVSE v1.1.5 `Data\Config\BetterFlashlightNVSE.ini`].
  While actively playing (`PlayState::Playing` and not `Paused`), drains charge
  (`100 -> 0`) at `DrainRate` units/s when on, automatically shuts off at `0` and locks
  out re-toggling until recharged past `RechargeThreshold` (when `> 0`), recharges at
  `RechargeRate` units/s while off, and resets to `100%` (clearing lockout) on save load
  or new game [design: NVForever's own in-memory `FlashlightRuntime` state machine
  without altering the save format].

## Intentionally unbuilt / out-of-scope sections and keys

Skipped during parsing with no dead config fields:

- `[Hotkeys]` (`DimmerIncrease`, `DimmerDecrease`, `IntensityIncrease`,
  `IntensityDecrease`): DirectInput scan-code hotkeys and live INI saving are not built.
- `[EngineLight]` (`Enabled`, `Dimmer`, `Specular`, `Atten0`, `Atten1`, `Atten2`):
  separate Gamebryo `NiPointLight` attenuation pass is not built.
- `[MaterialLight]` (`Enabled`, `Render`, `Intensity`, `Specular`, `SpecularPower`,
  `SpecularBoost`, `NormalStrength`, `MaxGeometry`, `DebugMode`, `LogProjection`):
  D3D9 additive geometry pass is not built; Bevy's native `SpotLight` already lights
  world materials natively.
- `[Volumetric]` (`Enabled`, `FirstPersonEnabled`, `ThirdPersonEnabled`, `Strength`,
  `Anisotropy`, `HeightFalloff`, `MaxDistance`, `FirstPersonStrength`,
  `FirstPersonOffsetRight`, `FirstPersonOffsetDown`): screen-space single-scattering
  volumetric shaft pass is not built.
- `[Flashlight]` D3D9 post-process / interop keys (`SoftEdges`, `DisableNVRFlashlight`,
  `RenderShadows` — marked stubbed in v1 by the INI itself, while NVForever relies on
  native `SpotLight` shadow maps for wall occlusion and `#3121` first-person anti-washout
  scaling — `NearFade`, `HotspotLimit`, `CookieStrength`) are not built.
- `[Battery]` HUD icon placement keys (`IconPosX`, `IconPosY`, `IconScale`,
  `PulseThreshold`, `IconSet`) are not built.
