# Modern Feel: volumetric fog and object motion blur

Part of the fork owner's Modern Feel experiment (`nvf_play::modern_feel::ModernFeel`, rows
under NVF Features › Modern Feel). Both parts are **NVForever's own design** [design]: none
of it is the original game's look, never a parity item, and every constant below is an
engineering choice tuned by eye. Both are on by default and act only while their switch
(`volumetric_fog()`, `object_motion_blur()`) is on; turning one off takes its components off
the camera and the picture is what it was.

## 1. Volumetric fog (`engine/nvforever/src/vol_fog.rs`, `vol_fog.wesl`)

**What it does.** The air holds a little dust. Sunlight lights it where the sun's shadow
maps say the sun reaches, so shafts stand where light falls past roofs, poles, rocks and
the gaps in walls, and the haze glows towards a low sun. Lamps and spotlights (the
flashlight among them) light the dust around them, indoors and out (the Strip's lamps at
night). It only adds light: no extinction (the weather's distance fog keeps that job), and
the dust thins above the camera, so the sky and far ridges are never greyed.

**Density** follows the place:

| Where | Density (per game unit) | Notes |
|---|---|---|
| Outdoors | `2.5e-6 × (1 + 2.5 × low sun) × (1 + 2 × haze)` | low sun: 1 below 8° elevation, 0 above 35°; haze: shafts.rs `haze(fog far, strength)`, the weather's own fog (NVWastelandClear 0.1) |
| Indoors | `4e-5 × clamp(8000 / cell fog far, 0.5, 1.75)` | a near interior fog (vaults, factories) is a dustier room; lamps only (the interior's directional light is a fill light, not a sun) |

Outdoors the dust thins above the camera with an e-folding height of 2,500 units; the
march reaches 6,000 units outdoors (inside the sun's 8,000-unit shadow maps) and 3,000
indoors. Scattering: Henyey-Greenstein g 0.55 outdoors, 0.3 indoors, with 20 % of it
the same every way. Lamps use the game's falloff (lights.rs, `1 − d²/r²`), squared, so
their haze is a soft halo inside their radius; spotlights keep their cone. Outdoors the
lamps light the dust only as the sun goes down (by day their halo is lost in the light, and
the clustered lights' loop is the march's dearest part).

**How.** One pass at half the picture's size marches each pixel's ray from the camera to
the depth prepass's surface in 16 steps with a per-pixel offset (interleaved gradient
noise; varied per frame only when a temporal anti-aliasing or upscaler smooths it). It
binds Bevy's own mesh view bindings (as Bevy's volumetric fog does), so it samples the
sun's real shadow cascades (`get_cascade_index`, `sample_shadow_map_hardware`) and the
clustered point and spot lights. The sun is the directional light pointing at the sky's
sun (the brightest if several: the first-person arms' shade light shares its direction at
1/1024 of its strength). A second pass upsamples it by depth (3 × 3 half-size texels,
weighted by nearness and by how close their surfaces' distances are; in a multisampled
view the fog of the nearest and the farthest sample of the pixel, mixed by their share, so
the grass's dithered distance fade and plants' soft edges do not leave a dotted pattern)
and adds it to the
HDR picture **before bloom and tonemapping**, so the eye adaptation, the tonemapper and the
grade treat it as part of the scene. Bevy's `VolumetricFog` was not used: it marches at
full size inside a box (the box edge showed in 2026-10, graphics-enhanced.md §4) and
cannot follow the weather's fog.

**Agreement with the other haze.** The weather's distance fog is untouched. The
screen-space sun shafts (`light-shafts`, shafts.rs) are kept for the sky beyond the shadow
maps (light streaming past far mountains), at 60 % of their strength while the fog is on
and the sun is up, so the two do not double.

**Log.** `volumetric fog: on, outdoors, sun, density …/unit, reach 6000` when the place or
the density changes by half the base; `volumetric fog: off`. GPU time: the
`nvf_volumetric_fog` span of `--log-frame-times`.

## 2. Object motion blur (`engine/nvforever/src/object_blur.rs`, `object_blur.wesl`)

**What it does.** Moving things blur by their own motion: an actor running across the
view, a swinging door, a body thrown by a hit, a vehicle. The still world stays sharp when
the view turns (the camera's own blur stays the game's: the image space modifiers' motion
blur, imagespace.rs, VATS and the sights), and an actor the camera tracks stays sharp
too, because it does not move on the screen.

**How.**
- The main camera gets Bevy's motion-vector prepass while the switch is on (graphics.rs
  takes it off whenever no temporal upscaler needs it; this module puts it back each frame).
  With multisampling on (the default), the alpha-tested shapes' soft edges stay in the
  prepass (nif_material.rs "Prepass coverage"): `nif_prepass.wesl` and `fade_prepass.wesl`
  got motion variants that also return the sample mask, and `prepass_coverage_applies` now
  allows a motion-vector prepass. Without that the main pass fell back to alpha to coverage
  and cost 1.8 ms more at Goodsprings (below).
  The NIF, grass and tree prepasses already write motion vectors for the upscalers
  (`nif_prepass.wesl`: Bevy's previous model, skin and morph matrices; `grass.wesl`,
  `tree.wesl`: last frame's wind), so **rigid and skinned meshes both have their own
  motion**: actors' animation, ragdolls and physics bodies included.
- Velocity pass (full size): each pixel's motion on the screen is compared with the motion
  the camera alone gives a still point at its depth (the depth reprojected with last
  frame's camera, the matrix worked out in double precision on the CPU). Where they differ
  by 0.6 pixels or more over the exposure (fully from 1.8), the pixel belongs to something
  that moves in the world, and its **whole** motion on the screen is its blur. A world move
  of 8-15 % of the picture's height in one frame is a jump (a script's MoveTo): no blur.
- Exposure 1/120 s (a 180° shutter at 60 fps), fixed in time so the blur is the same at any
  frame rate; radius held to 2.1 % of the picture's height (30 pixels at 1440p). Pixels
  nearer than 40 units (the first-person arms and weapon) neither blur nor get smeared
  over. A camera cut (300 units or 45° in a frame) blurs nothing that frame.
- Reconstruction: the largest blur of each 16 × 16 tile and of its neighbours sets the line
  the 12 taps lie on; each tap is weighted by whether it lies in front of or behind the
  pixel and whether its own blur reaches it (McGuire, Hennessy, Bukowski, Osman, "A
  Reconstruction Filter for Plausible Motion Blur", I3D 2012 [documented: the published
  method; our own code from it]). So a moving actor smears over the still background, not
  the other way round. Pixels in tiles with nothing moving are copied.
- Placement: on the tonemapped picture after the anti-aliasing (so TAA's history holds
  no blur) and the ambient occlusion, before the sun shafts and the image space.
- **TAA follows the vectors too** (taa.rs, taa.wesl): when the camera has the
  motion-vector prepass (this switch, or never otherwise: TAA and the upscalers exclude each
  other), the history of a pixel is fetched where its own motion vector (at the nearest
  depth of its 3 × 3) says it was, instead of where the camera alone moved it; the
  first-person arms keep their camera-locked rule and the variance clipping stays. What
  moves by itself (actors, plants in the wind) keeps its smoothing and trails less.

**Log.** `object motion blur: on (motion vectors from the prepass, own motion only)` /
`off`. GPU time: the `nvf_object_blur` span.

## 3. Cost

[observed 2026-11-21: release-quick build, the owner's machine (RTX 4070, other sessions'
builds running), 2560 × 1440, MSAA 4x, light glow, sun shadows, `--log-frame-times 5`; GPU
time of the passes, steady 5-second periods]

| View | Switches off | On | Volumetric fog | Object blur | Prepass (motion vectors) |
|---|---|---|---|---|---|
| Goodsprings 18:36, facing the sun, much grass | 4.18 ms | 5.37 ms | 0.51 ms | 0.39 ms | 1.10 against 0.86 ms |
| Prospector Saloon, 21:00 | 2.27 ms | ~2.9 ms | ~0.5 ms | 0.31 ms | 0.27 against 0.15 ms |

So both parts together cost about 1.2 ms at 1440p (about 0.5 for the fog, 0.4 for the
blur, 0.25 for the motion vectors). Before the prepass coverage fix the main pass alone
rose from 1.57 to 3.41 ms with the motion-vector prepass on. Both scale with the pixel
count.

## 4. What is rough or left out

- The fog has no extinction: it brightens only. In a dark interior with a near fog it is
  the lamps' halos that show; a thick smoke room is not modelled.
- Lamps do not cast shadows into the fog (no LIGH lamp has a shadow map); a lamp just
  behind a thin wall lights the air in front of it within its radius. The squared
  falloff keeps that small.
- The fog's march stops at the first surface; blended surfaces (glass, water) are not in
  the depth prepass, so fog behind a window is drawn as if the window were not there.
- The cloud shadows and the moon do not light the fog.
- Shapes that are not in the depth prepass (blended: glass, particles, water's surface)
  take the motion of what lies behind them, so a muzzle flash or smoke puff is not blurred
  by its own motion.
- Object blur is 4 full-size or tile passes in fragment shaders; a compute version could
  halve its 0.4 ms.
- Verified by eye on stills only: a gecko charging at Goodsprings noon (its body and tail
  smeared along its run, grass and rocks around it sharp), the dusk view, the saloon's
  hallway lamp. Fast turns with tracked actors were not filmed.
