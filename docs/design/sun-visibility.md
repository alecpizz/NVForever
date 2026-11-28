# Sun visibility: one factor on the sun's direct light

Status 2026-11-17: cloud shadows (#2670) built as a player's switch: the plumbing (stage
C0, the base for terrain shadows #2671) with a debug view and a test producer, the cloud
producer (C1, `engine/nvforever/src/cloud_shadows.rs`) and the settings row with the
water highlight and the first-person arms (C2, §7); the relative transmittance (§7a) after the
contributor's play-test of 2026-11-17. Terrain shadows (#2671, T0-T2) fill the terrain
slot: the receivers and the row Terrain shadows (experimental) are built (§5a,
[terrain-shadows.md](terrain-shadows.md); what has been run with them and what not: its
"What has been run"). Runs so far: main session run 2026-11-17, fork-notes (the checkerboard
reached terrain, objects, trees and distant hills at Goodsprings 12:00; no shader or layout
errors; an interior run clean; with C2 and the relative rule, Goodsprings 12:00 under a clear
and an overcast weather, the logs free of rendering errors). What is not yet shown is in §6,
what is left to calibrate in §8.

Like everything in [graphics-enhanced.md](graphics-enhanced.md), this is an engineering
choice, not a fact about the original game.

## 1. The interface

Producers write the main-world resource `SunVisibility` (`engine/nvforever/src/sun_visibility.rs`):

| Field | Type | Meaning |
|---|---|---|
| `cloud_enabled` | `bool` | Cloud shadows is switched on: the views take the cloud variant whether or not `cloud` is set; without `cloud` the cloud is neutral (K = 1) |
| `cloud` | `Option<CloudVisibility { image, uv_scale, uv_offset, plane_height, strength }>` | K: a transmittance texture (red, 1 clear, 0 opaque) on a horizontal plane `plane_height` up, looked up where the ray from the receiver towards the light meets it, at uv = hit.xz × `uv_scale` + `uv_offset` (repeating); K = 1 − `strength` × (1 − texel) |
| `terrain_enabled` | `bool` | Terrain shadows is switched on: the views take the terrain variant whether or not `terrain` is set; without `terrain` it is neutral (T = 1, Vsolid = C) |
| `terrain` | `Option<TerrainVisibility { image, uv_scale, uv_offset, texel, side }>` | T: the shadow-height map over world xz (#2671, terrain_shadows.rs; `Rgba32Float`, R S, G caster distance, B relief), read at uv = xz × `uv_scale` + `uv_offset` |
| `debug` | `DebugMode` (`Off`, `Cloud`, `Terrain`, `Combined`) | the debug view |

Axes are Bevy's world axes (y up), units game units. In the shader
(`engine/nvforever/src/sun_visibility.wesl`):

- `nvf_sun_visibility(world_position, direction_to_light, cascade_shadow, view_z) -> f32`
  gives V; `nvf_sun_visibility_parts` gives K, T, Vsolid and V together.
- The resources sit in the mesh view bind group at bindings 39 (uniform
  `NvfSunVisibility`: cloud uv transform, plane height, strength, horizon fade; terrain uv
  transform, two unused words (0), texel size, side; feature mask, debug mode), 40 (cloud texture,
  filterable), 41 (terrain map, declared unfilterable and read with `textureLoad` only; a
  one-texel stand-in that casts nothing until a map is uploaded, and while it stands in the
  written uniform's terrain bit is cleared, `usable_uniform`, so T = 1 and Vsolid = C
  before any load) and 42 (a linear, repeating
  sampler, for 40) (`engine/vendor/bevy_pbr/src/render/mesh_view_bindings.rs`,
  `NvfSunVisibilityBindings`; engine/vendor/README.md). Since #3032 stage 2i M2 also 45 and 46,
  the volumetric clouds' light volume's near and far front sets, and the uniform's `volume` and
  `light_volume` blocks (both in the shader's struct with bindings 45-46 alone): with the volumetric clouds on, K comes from the light volume where it has
  data (graphics-enhanced.md §3.23, "Ground shadows from the light volume"; mask bit 4, bit 8 for
  the developer aid `NVF_PLAY_DEBUG_CLOUD_SHADOWS_SOURCE=volume`, not in release builds); with bit 4 clear K is the layers' as
  before, word for word. Bindings 45 and 46 are in the layout only with the cloud variant, while the
  Volumetric clouds row is on (`volume_layout`), on an
  adapter with room for them (`CloudVolumeRoom`, measured once, when they are first wanted, and logged in one
  line: the fullest pipeline's fragment textures with them, the view layout's at its fullest
  and the world material with the most outside binding arrays, plus 4 kept free, against
  `max_sampled_textures_per_shader_stage`); otherwise the views have the weather-only layout
  and K is the layers' whatever the aid asks.
- Shader defs: `NVF_SUN_VISIBILITY` while the plumbing is on for the view,
  `NVF_CLOUD_VISIBILITY` while Cloud shadows is switched on (or the test producer sets a
  cloud), `NVF_TERRAIN_VISIBILITY` while Terrain shadows is switched on, `NVF_CLOUD_VOLUME`
  with bindings 45-46 (the light volume's reader compiled in).
- Switched on and published are kept apart: the switch alone decides the markers, the
  layout key bit, the pipeline key bits and the shader defs; the publication only fills
  the uniform and the texture. Switched on with nothing published, the uniform carries a
  neutral cloud (strength 0, the plane below every receiver) and the texture binding is the
  white fallback, so K = 1; for the terrain the mask bit is clear, so T = 1 and Vsolid = C.

## 2. The composition rule

V = K × Vsolid, with Vsolid = min(C, T) at every distance; C is the cascade shadow as Bevy
computes it (contact shadows and the mesh's shadow factor included), T the terrain's
visibility, K the clouds'. Without terrain, Vsolid = C exactly.
[engineering choice, from the plan for #2670/#2671; no blend by distance: the maintainer's
decision on #2920, 2026-11-18]

Why `min` and not C × T [engineering choice]: near the camera the cascades already contain
the terrain as a caster, so a ridge's shadow is in both C and T. Multiplied, the same
filtered edge would be squared (darker and harder than either). The minimum counts the
darker of the two once. It is an approximation: two scalar masks cannot rebuild the exact
union of two different occluders' soft edges; that would need shared samples or separate
terrain and object shadow maps. Beyond the last cascade C is 1 (Bevy's
`fetch_directional_shadow` returns lit there), so T alone shades there, and object shadows
end at the cascades' cut-off line as with the row off. K multiplies, as clouds are a different medium from the
solid occluders. With sun shadows off C is 1 and Vsolid = T.

Where it acts (`HOOK_EDITS` in sun_visibility.rs, patched into bevy_pbr's
`apply_pbr_lighting` after lights.rs's display-value edits): in the directional light loop,
the outdoor sun's `shadow` becomes V before the light is added, so Bevy's `direct_light`,
the display-value sum `nvf_direct` and the transmitted lobe (`transmitted_shadow`) all take
it. It is outside the `MESH_FLAGS_SHADOW_RECEIVER_BIT` condition, so tree leaves, distant
tree cards and water (not cascade receivers) take it too. Ambient, emissive, point and spot
lights, fog and reflections are left alone.

Only the sun: the `Sun` light (sky.rs) carries the marker `OutdoorSunLight` while outdoors,
extracted as bevy_pbr's `NvfOutdoorSun`, which sets `DIRECTIONAL_LIGHT_FLAGS_NVF_OUTDOOR_SUN_BIT`
in its GPU flags; the hook tests that bit. At night the same light is the moon's, so V
applies to the moonlight too. Indoors the same entity is the cell's directional light and
is unmarked. The first-person arms' shade light and menu views' lights never carry it.

Which views: the world camera (`FlyCam`), the water reflection camera and a scope's lens
camera. While Cloud shadows is switched on they keep their bindings, layout key bit and
shader defs through doors, missing or still-decoding textures and weathers without cover:
the producer clearing its output (it does indoors, cloud_shadows.rs) only makes the cloud
neutral, and indoors the light is unmarked, so nothing darkens. Only switching the feature
on or off changes the views' pipelines. A unit test runs the producer's own door path
(cloud_shadows.rs `cloud_bindings_survive_doors`); that no pipeline is built at a door is
for a run to show (§6).

## 3. Off by default

`SunVisibility` starts empty. While nothing is switched on (no producer, no debug view) no
camera is marked, no view has the bindings, the mesh view layout key bit and the pipeline
key bits are clear, and every statement the hook adds is under `@if(NVF_SUN_VISIBILITY)`,
so the shader a pipeline compiles has the display-value patch's statements (the raw text
differs; compiled-byte identity is not established). No GPU buffer, sampler or image is
made until it is first on (the uniform buffer and sampler are then kept after switching
off).

What the off path still costs [documented: the code; hypothesis: negligible, unmeasured]:

- At start: the shader `sun_visibility.wesl` is loaded, the debug aids are read, and the
  hooked copy of `apply_pbr_lighting` is made as a string (lights.rs, `hook_patched`).
- Every frame: run-condition checks (`resource_changed`, `marking_needed`, the cloud
  producer's switch and change checks), extraction's early return and preparation's empty
  query; in the vendored bevy_pbr, the views' query reads an optional component
  (`mesh.rs`, `mesh_view_bindings.rs`) and `prepare_lights` tests the sun marker per
  directional light (`light.rs`); sky.rs calls the cached cloud-phase helper.
- The pipeline key's packed fields moved up by three bits (`MeshPipelineKey::LAST_FLAG`),
  a different numbering of the same keys, not more of them.

Turning it on or off changes the views' pipeline keys, so every material pipeline of those
views is built again, as any view key change does; it is the only thing that does
(§2, "Which views").

## 4. Test aids

- `NVF_PLAY_DEBUG_SUN_VISIBILITY=cloud|terrain|combined`: draws K, T or V (grey, 1 white)
  in place of the lit colour, before ambient and fog (the fog block is left out under the
  debug view). T is 1 unless Terrain shadows is on (the row or its override). The image space grade and the eye adaptation still act afterwards
  (`--no-grade` leaves the grade out). Alone it turns the plumbing on, and `combined` then
  shows the cascade shadow C.
- `NVF_PLAY_DEBUG_SUN_VISIBILITY_TEST=checker`: a built-in producer filling `cloud` with a
  64×64 checkerboard of clear and opaque cells of 2,000 units, strength 1, on a plane
  20,000 units up.
- Log: `sun visibility: cloud <on|off> terrain <on|off> debug <mode>` whenever the switches
  change, and `sun visibility: test producer: ...` for the test producer.
- Cloud shadows' own (cloud_shadows.rs): `NVF_PLAY_DEBUG_CLOUD_SHADOWS=1` (on whatever the
  row says), `NVF_PLAY_DEBUG_CLOUD_SHADOW=<plane height>,<repeat>,<strength>[,<direction>]`
  (the calibration), `NVF_PLAY_DEBUG_CLOUD_PHASE=<seconds>` (the clouds' scroll clock frozen,
  dome and shadows alike), `NVF_PLAY_DEBUG_CLOUD_REFERENCE=<percentile 0..100 | off>` (the
  clear reference's percentile, §7a; `off` the absolute product); log `cloud shadows: layers
  ... reference <K_ref | off> ...` when the composed layers, their weights or the projection
  change, `cloud shadows: none (<reason>)` when the output goes.

## 5. Outside apply_pbr_lighting

- Water's own sun highlight (`engine/nvforever/src/water.wesl`, in the
  `lights.n_directional_lights > 0u` block) is drawn outside `apply_pbr_lighting`. Under
  `NVF_SUN_VISIBILITY` it takes the light flagged as the outdoor sun (none flagged indoors:
  light 0, as without the def) and, when flagged, multiplies the highlight once by
  `nvf_sun_visibility(pos, L, 1.0, view_z)` (water casts and receives no cascade shadow).
  The reflection image is not multiplied. The water body is lit by `apply_pbr_lighting`
  and takes V through the hook.
- The first-person arms' `with_shade_light` (`engine/nvf-assets/src/nif_material.wesl`)
  rebuilds the sun's part of their light from two `apply_pbr_lighting` results (module
  notes of nif_material.rs, "Shade light"). With the sun's value V(C) in both (C = 1 for
  the unshadowed one) and the shade light's unchanged:
  `missing = (V(1) − V(own)) + share × (1 − shade)` and
  `k = (V(1) + share − V(wanted)) / missing`. With V(C) = C (no def, or indoors) these are
  the old `(1 − own) + share × (1 − shade)` and `(1 + share − wanted) / missing` term for
  term; the derivation is in the shader. The finished colour is not multiplied.
- Terrain: see §5a.

## 5a. Terrain shadows (#2671, stage T2)

T (sun_visibility.wesl `nvf_terrain_visibility`) reads the shadow-height map at the four
texels around the receiver with `textureLoad` and filters the four comparisons bilinearly,
never S; each comparison is a smoothstep over a band under S that grows with the caster's
distance (the sun's size) and with a texel's fall of S; the receiver is lifted by twice the
ground's relief there plus 16 units against speckle on lit slopes; T fades to 1 over the
map's last 4 texels and is 1 outside it and over texels with no terrain. With a map
published Vsolid = min(C, T) (§2): object shadows keep the cascades' cut-off line, past the
cascades T alone shades (terrain-shadows.md, "Only terrain-scale shadows; the darker of the
two"), and with Sun shadows off T acts alone. The switch is the
row Terrain shadows (experimental) (`terrain-shadows`, Graphics › Shadows, after Cloud
shadows; off by default and in every preset; Continue waits for its pipelines), or
`NVF_PLAY_DEBUG_TERRAIN_SHADOWS=1`, which wins; `graphics.rs` key `terrain-shadows`. The
formulas, their reasons, what is unmeasured and what is open for tuning:
[terrain-shadows.md, "T2: the receivers"](terrain-shadows.md#t2-the-receivers). Smoke:
`terrain-shadows`.

## 6. Not yet proved

Unit tests resolve the shader defs this feature writes in the complete patched lighting
function, water.wesl and nif_material.wesl and check their structure
(`patched_lighting_resolves_in_every_variant`, `water_and_arms_edits_resolve`), and naga
validates `sun_visibility.wesl` in its four variants. Only a run can show: that naga
accepts the patched lighting, water and arms shaders with bevy_pbr's imports resolved
(no unit test resolves them) in every pipeline of a marked view; that the layouts match on
the reflection and scope views; that the clouds land on actors, grass, leaves, tree cards
and water and on the arms; that a door, a weather change or a texture read builds no
pipeline; the cost, on and off.

## 7. The switch (stage C2)

- Settings row `cloud-shadows`, "Cloud shadows (experimental)", under Graphics › Shadows
  after Longer sun shadows (`crates/nvf-settings-layout/src/lib.rs`; registered in
  `engine/nvforever/src/settings.rs`, docs/settings-reference.md). Off by default. It is
  among the rows Continue waits for (`register_pipeline_waits`): switching it rebuilds the
  world views' material pipelines, nothing else does.
- `graphics.rs`: `Graphics::cloud_shadows`, the key `cloud-shadows` for `--graphics-only`
  and `--graphics-cycle`, "cloud shadows" in the log's `graphics:` line. Off in both
  presets: #2670 asks for it "in `enhanced`", a row the menu overhaul removed, and its
  strength is not calibrated (§8); whether a preset turns it on is the maintainer's call
  (for the PR text).
- `NVF_PLAY_DEBUG_CLOUD_SHADOWS=1` turns it on whatever the row says
  (`CloudShadows::forced`).
- The producer drops compositions from before a worldspace or weather change and clears
  its output on world travel; the wind's displacement is accumulated, so a direction
  change turns the drift and never moves the pattern (cloud_shadows.rs module notes).
- Smoke: `cloud-shadows` (tools/smoke/scenarios.toml).
- The label says "(experimental)", after the existing parenthesised suffix of "3D scopes
  (test)" (settings.rs; translated in each lang file), and the note says the look is still
  being tuned: the relative rule (§7a) and the calibration (§8) are first values.

## 7a. Relative transmittance: patches, not an overall dim

Why: the contributor's play-test of 2026-11-17 found that under an overcast weather the
composed transmittance is nearly uniform, so switching Cloud shadows on only dimmed the
whole scene evenly ("too monotone"), over a sun colour the overcast weather already makes
darker. Wanted: the feature darkens where there is more cloud than elsewhere and never dims
overall.

The rule [engineering choice; cloud_shadows.rs `compose_relative`]: after the product over
the layers, the map is divided by a clear reference taken from the map itself,
`K_rel = min(K / K_ref, 1)`, `K_ref` the 90th percentile of the 512 × 512 values
(`REFERENCE_PERCENTILE`, from a 256-bin histogram; to be calibrated from pictures). Then:

- uniform cover of any density: every texel is the reference, all 1, no dimming at all
  (`cloud_uniform_cover_casts_nothing`);
- scattered cloud on clear sky (most texels near 1): the reference is 1, the map unchanged
  within rounding (`cloud_scattered_cover_is_kept`);
- heavy cloud with gaps: the gaps are the reference and read fully lit, the cloud darker by
  its ratio to them (`cloud_gaps_become_the_reference`);
- cover with no clear gaps: under `REFERENCE_FLOOR` (0.1) the darkening is scaled down in
  proportion to the reference, to none at 0, so an opaque map casts nothing and a tiny
  reference does not blow the alpha's 8-bit steps up into speckle
  (`cloud_reference_floor_fades_out`).

The reference moves with the map. With one layer, or all at one speed, the image is never
recomposed for the scroll (it moves by `uv_offset`), so `K_ref` stays put. Layers at other
speeds shift against the scrolling's reference layer, which changes the product's histogram
along a period of `1 / |speed difference|` seconds (NVWastelandClear's 52 against 65: 1,969 s),
a new composition being due after a quarter texel; a percentile of such a map moves little
per step [hypothesis]. During a weather fade it moves with the weights; a 64th of the fade
makes a new composition due, and the rate cap, decoding and the task's own time can make the
step between two compositions larger. A composition whose own reference is lower than the
previous one takes its own at once (never a reference above the map's own: uniform cover
that follows a clearer map would otherwise be dimmed evenly until the smoothing caught up);
a rising one is moved `REFERENCE_SMOOTHING` (a quarter) of the way from the previous one
towards the map's own, and the map is composed again at the rate cap until the two are
within half an 8-bit step; a discontinuity (a worldspace or a
weather set replaced) starts it from the next map's own (`cloud_reference_smooths_and_resets`).
That no "breathing" of the whole pattern is seen is a hypothesis: the rule has been run
standing still under two pinned weathers, not through a weather change.

## 8. Open for calibration

The values are engineering choices, to be set from pictures (cloud_shadows.rs, the test
aid `NVF_PLAY_DEBUG_CLOUD_SHADOW=<plane height>,<repeat>,<strength>[,<direction>]`):

- Strength (`STRENGTH`, 0.6): how much of the sun's light an opaque cloud takes away, and
  repeat (`REPEAT_UNITS`, 60,000 units per texture repeat): the size of the pattern on the
  ground. Both are the contributor's pick in play on 2026-11-17, with the relative rule
  (§7a), at Goodsprings at noon under a clear and an overcast weather, among five pairs:
  60,000 / 0.6, 40,000 / 0.7, 30,000 / 0.8, 20,000 / 0.8 and 30,000 / 1.0 [engineering
  choice]. Under clear weather the pick is subtle; at strength 1.0 the ground under a dense
  cloud went nearly black. To be refined.
- Plane height (`PLANE_ABOVE_GROUND`, 20,000 units above the ground reference): how far the
  pattern shifts with the sun's angle.
- Reference percentile (`REFERENCE_PERCENTILE`, 90) and floor (`REFERENCE_FLOOR`, 0.1) of
  the relative rule (§7a), and its smoothing (`REFERENCE_SMOOTHING`, a quarter per
  composition).
- Overcast double dimming (an overcast weather's darker sun colour, then an even cloud
  shadow on top: NVBlackMountainWeather dimmed the ground almost evenly, main session run
  2026-11-17, fork-notes): addressed by the relative rule (§7a), which casts nothing for
  even cover; how strong the patches it leaves should be (strength) and how large (repeat)
  are still open.
- Night: the moon's light takes the same factor at the same strength [engineering choice,
  not looked at].
