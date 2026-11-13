# Remastered graphics: the `remaster` preset

Status: first set built 2026-11-12 (branch `gfx/remaster`). The owner asked for "a total
remaster": updated materials, a performant global illumination, volumetric fog and more,
built from rendering techniques alone (no external assets).

Everything here is an **engineering choice**, not a fact about Fallout: New Vegas: nothing is
tagged `[documented]`/`[observed]` about the original game, and none of it may be used to
explain a difference against the game's frames. `vanilla` stays the picture the parity lab and
the smoke checks compare; test runs keep defaulting to it (graphics-enhanced.md §5.2).

`--graphics remaster` is `enhanced` (graphics-enhanced.md) plus the switches below; each is a
key of `--graphics-only` and of `--graphics-cycle` (which also takes `remaster` as a set).

| Key | What | Code |
|---|---|---|
| `global-illumination` | Screen-space GI: bounced light and the ambient light's occlusion inside the lighting; hemispherical, normal-mapped ambient; sky reflection; softer sun-shadow edges | `gi.rs`, `gi.wesl`, `lights.rs` (`DISPLAY_EDITS`), vendored `bevy_pbr` |
| `volumetric-fog` | Height fog lit by the sun through its shadow cascades | `atmos.rs`, `atmos.wesl` |
| `colour-grade` | S-curve, split toning and vibrance after tonemapping | `post.wesl` `grade` |

## 1. How the lighting is reached

The game's surfaces are lit by the display-value sum (lights.rs `DISPLAY_HELPERS`,
world.md §8), patched into Bevy's `apply_pbr_lighting`. The remaster adds a **view binding**
to the vendored `bevy_pbr` (`ScreenIndirectLight`, binding 39, shader def `SCREEN_INDIRECT`
on meshes that write depth; vendor/README.md): a full-resolution texture whose rgb is the
light bounced onto the pixel (linear, after exposure) and whose alpha is how much of the
hemisphere is closed. Every material that uses `apply_pbr_lighting` (NIF shapes, terrain,
grass, trees) reads it at its own pixel. Everything the remaster changes in the lighting sits
under `@if(SCREEN_INDIRECT)`, so other views (water reflection, minigames) and the other
presets are unchanged.

## 2. Global illumination (`gi.rs`)

Before the main pass, from the depth prepass alone (the custom prepasses write no normals):

1. **Trace**, half resolution: two slices per pixel turned by interleaved gradient noise each
   frame, 8 steps per side out to 450 game units (squared spacing), each sample a surface of
   40 units' thickness behind the depth ("screen space indirect lighting with visibility
   bitmask"). The 32 sectors are equal in projected solid angle (sin of the angle from the
   normal), so the share of sectors closed is the share of irradiance lost and each newly
   closed sector carries the light of the sample. That light comes from **last frame's HDR
   picture** reprojected with the camera's motion (step 4), accepted only where last frame's
   distance there agrees (what moved in front is not used). Multi-bounce follows: the picture
   already holds last frame's bounce.
2. **Temporal**: 88 % last frame's result where the reprojected distance agrees within 5 %.
3. **Upsample** to full resolution: 3 x 3 half-resolution texels weighed by distance.
4. **Capture**, after the main passes and before tonemapping: the HDR picture averaged over
   2 x 2 blocks with each block's distance, for the next frame.

In the lighting (lights.rs): the ambient term of the display sum is multiplied by the
openness (occlusion capped at 0.45, so dim rooms keep their corners) and by a sky/ground
hemisphere on the **normal-mapped** normal (1.10-1.17 above, 0.68-0.80 below), so shapes read
in the shade, where the game's flat ambient showed no normal map at all. Point and spot lights
take 60 % of the occlusion (they cast no shadows). The openness is raised to 1/2.2 first:
the display sum is raised to 2.2 afterwards, and without it 45 % occlusion took 73 % of the
ambient light and crushed sunlit scenes' shade.

Outdoors (a shadow-casting sun in the light) the remaster leans the light towards the sun,
sun up to x1.15 and the ambient a little cooler and bluer (open shade lit by the sky; x(0.965, 1.0, 1.07)), growing with the sun's height from 12 to 35 degrees (dawn and
dusk keep their soft shade): a harder desert light whose shade the bounce fills. The bounce is added as diffuse colour ×
light. The post-process AO (graphics-enhanced.md §3.1) stays on at 0.35 for contact shadows.

Two longer tiers sit on top of the march, in the manner of radiance cascades (fine and
short near, coarse and long further out):

- **Far cascade**: 4 more steps per side out to 8 times the near reach (at most 0.45 of the
  screen height), with 4 times the thickness, light only: they light sectors nothing nearer
  closed and leave the occlusion to the near steps, so whole valleys are not darkened.
- **Emissive glow field**: last frame's light of texels brighter than 1.5-4 (a soft
  threshold, after exposure: neon, bulbs, fire, lit stair treads; never the sky) at a
  sixteenth of the picture's size, blurred wide both ways and added at strength 7 to the
  bounce of surfaces within 3,000-9,000 units (`NVF_GI`'s seventh value tunes it). The Strip's steps,
  rails and fountain take the signs' warm light, which no march of a few hundred units
  reached.

**Light probe** (probe.rs; world space): six 128 x 128 cameras at the main camera's
position, one per cube face, render the world around it, off the screen too, one face every
4th frame (the whole probe about 5 times a second at 120 frames a second). gi.wesl copies the
faces into a 2D array each frame, reduces each face to its mean light (`probe_reduce`) and
adds them by the surface's world normal (an "ambient cube"; the face looking up is left
out, the game's ambient being the sky's) at 0.6 × openness in the upsample. The array is
also the reflection environment of environment-mapped shapes (§4), at view bindings 40 and
41 of the vendored bevy_pbr. Cost: each probe view is a view's culling, queueing and sun
cascades on the render thread: rendered every frame it added about 2 ms to the frame at
Goodsprings; one face every 4th frame adds about 0.7 ms on average, with the slowest frames
near 14 ms. Limits: one probe at the camera, so no parallax (a window reflects what the
camera's position sees), and it lights by direction only.

**Height field** (probe.rs `HeightField`, gi_height.wesl, gi.wesl `world_rays`; world space,
outdoors): an orthographic camera 20,000 units over the player looks straight down on a
12,000-unit square at 512 x 512 (about 23 units a texel), its centre held to the texel grid,
every 8th frame and never on a probe frame. Its picture is the lit world seen from above; its
depth prepass becomes world heights (R32Float) the frame it is drawn. In the march each
half-resolution pixel sends 3 cosine-distributed rays through the field in 10 growing steps
out to about 3,000 units: a ray that passes under the field has hit something and brings back
the colour seen from above there (0.8), one that gets away is open sky; the share shut in
occludes the ambient light (up to 0.55, the larger of it and the screen march's). So what the
screen does not hold counts: the far side of a canyon, buildings behind the camera, the
cover of eaves and roofs (whatever is under something seen from above). Debug views 6 and 7
(`NVF_GI`'s fifth value) show the field's colour and its height against each pixel.
Limits: one height per texel (no overhang seen from below, walls take the height of the roof
above them), the colour is the top surface's, 23-unit texels. Cost at Goodsprings, 1920 x 1080:
frame 8.37 ms against 8.07 without (the extra view every 8th frame and the rays); slowest
frames near 13.8 ms.

Indoors (the cell has its own lighting) 0.07 of light (linear) is added where the
surroundings are open: FNV's dimmest rooms (the Prospector Saloon) otherwise crush to black
under the occlusion and the grade.

Known limits: nothing off the screen bounces; the glow field ignores occlusion and
direction (a sign's light reaches round a corner within its blur).

## 3. Volumetric atmosphere (`atmos.rs`)

After the main passes, before tonemapping, half resolution: 24 jittered steps from the camera
to the surface (at most 14,000 units), squared spacing. Density 0.00004 per unit at 400 units
below the camera, thinning to 1/e over 1,200 units of height and ramping in between 1,000 and
5,000 units from the camera (what is near stays crisp). Each step takes the sun's light
through its shadow cascades (the pass binds Bevy's mesh view bind group as Bevy's volumetric
fog does; the sun is the directional light whose cascades reach furthest) with a
Henyey-Greenstein phase (g 0.55, strength 3) and the ambient light evenly (0.8), tinted towards the sky's blue (60 % of it the ambient's brightness in (0.62, 0.92, 1.55): the air's own colour at a distance); then a
temporal blend (20 % new, clamped near the new value) and a depth-aware composite into the
HDR picture (picture × transmittance + light). Off indoors (the cell's own fog stands).

It adds to the weather's distance fog, which the surfaces carry already; it is what makes the
low sun's haze and the shafts behind buildings and rocks.

## 4. Materials

Under `SCREEN_INDIRECT` (lights.rs): the lights' highlights × 0.7, and a sky reflection: the
ambient light's brightness (grey) from a bright-above, dark-below hemisphere along the
reflected ray, × Bevy's environment BRDF (F0 from the specular mask, so unmasked surfaces show
none) × openness × specular occlusion, faded out between perceptual roughness 0.3 and 0.6 (rock,
rust and wood stay matte). Sun-shadow edges: the Gaussian filter averages Bevy's 13-tap filter
with its 8-tap spiral at 2.5x, turned per pixel and not per frame (vendored
`shadow_sampling.wesl`), a wider penumbra without temporal noise under MSAA.

**Detail** (terrain.wesl, nif_material.wesl `nvf_detail_*`): near the camera, where the 2010
textures run out of resolution, procedural world-space relief (two octaves of value noise with
an integer hash, cells of 9 and 3.5 units) tilts the normal (terrain 6, shapes 4) and shades
the colour by up to 14 % (terrain) or 12 % (shapes), faded out between 150 and 700 units
(sooner than first built: the reviewer saw busy, likely shimmering gravel further out);
not on FaceGen skin.

**Environment maps** (nif_spawn `Props::env_scale`, `NifSurface::env`/`env_mask`,
nif_material.wesl `env_reflection`): a shape whose shader maps an environment (BSShaderFlags
bit 7 or 21 and a texture-set slot 4) reflects the light probe's faces (§2) along the reflected ray, mixed with a third of an open sky above the horizon (a single probe has no parallax: without it a window seen from above reflected only ground and read as a dusty panel), at its `environment_map_scale` (held to 2) × its mask (slot 5, red) × a Fresnel
rise from 0.5 to 1, hidden where the GI closes the view. The game's own cube maps are not
drawn: they would need cube textures in the bindless material; the light probe's faces
(§2) stand in for them, so glass and metal reflect what is around the camera.

**Cloud shadows** (atmos.wesl `nvf_cloud_shadow`, in the fog only): value noise (4 octaves, each
turned about 37 degrees so the lattice never shows, cells of 11,000 units) at a cloud layer
30,000 units up, read where the ray to the sun crosses it and blended between the ray's two
ends, dims the fog's sun light to 0.4 under a cloud. They were on surfaces too for a while and
were taken off again: shapes drawn without the remaster's shader def (some LOD) stayed sunlit
inside a cloud as straight-edged patches, and from the ground a cloud over the view read as
underexposure rather than a passing cloud (reviewer, 2026-11-13).

## 5. Colour grade (`post.wesl` `grade`)

On display values after tonemapping, in the bloom/sharpen composite: × 1.06, a luminance
S-curve blended in by 0.14 with its toe lifted, shade a touch cooler and light a touch warmer,
vibrance +25 % on greyish colours. Tuned so the mean luminance of the eight test views stays
within -4..+5 of `enhanced`; contrast (standard deviation) rises in seven of them, Doc Mitchell's house is a little flatter (§6).

## 6. Measurements

[observed 2026-11-12, i9-14900K, RTX 4070, 1920 x 1080, `--log-frame-times 5
--graphics-cycle "enhanced;remaster"`, free camera at Goodsprings noon, sun shadows on]:
GPU time per pass: GI 1.10-1.26 ms (1.58 with 12 steps; 8 kept), volumetric fog 0.41-0.46 ms,
main opaque pass +0.27 ms (the extra lighting); about +1.8 ms of GPU in all. Frame time
4.8 ms (enhanced) against 6.4 ms (remaster) in the first cycle. With the far cascade, the glow
field and the cloud shadows (same place and method, later the same day): GI 1.78-1.89 ms,
fog 0.45-0.46 ms, main pass 1.74 ms (enhanced 1.47); frame time 4.86 against 7.17 ms; the
final build 4.91 against 7.34 ms (GI 1.93, fog 0.47, main pass 1.82).

Final build (detail relief, environment-map reflections, sun-only lean, bounce 3) against
`vanilla` (mean / standard deviation / share of pixels under 20): Goodsprings noon
97.8/58.6/8.1 % → 103.3/60.8/7.3 %, dusk → 94.0/54.9/2.8 %, road → 115.0/80.6/3.7 %, Doc
Mitchell's house → 42.6/22.0/10.3 %, saloon → 20.4/27.4/71.6 %, Novac → 75.2/44.7/2.5 %, Strip
→ 61.1/57.0/25.0 %, Primm → 94.2/52.3/3.0 %; frame 4.90 ms (enhanced) against 7.37 ms.

An earlier build against `vanilla`:
Goodsprings noon 97.8/58.6/8.1 % → 97.1/60.5/12.0 %, dusk 78.8/50.7/3.4 % → 92.7/55.1/2.9 %,
road 101.1/77.7/11.3 % → 114.8/81.8/4.4 %, Doc Mitchell's house 37.6/20.1/15.2 % →
42.5/21.8/10.0 %, saloon 17.5/28.7/77.4 % → 19.8/27.2/73.0 %, Novac 71.7/42.6/3.1 % →
75.6/45.2/2.4 %, Strip 60.1/56.0/24.3 % → 60.6/58.0/25.2 %, Primm 88.6/51.2/2.7 % →
90.0/53.1/4.7 %.

Mean / standard deviation of luminance of the eight test views (1600 x 900 screenshots),
`enhanced` against `remaster`: Goodsprings noon 96.9/59.4 → 92.8/60.1, dusk 87.1/53.2 →
88.9/55.9, road 106.0/78.2 → 110.4/81.4, Doc Mitchell's house 37.2/20.5 → 36.4/19.8, saloon
17.4/28.8 → 18.0/29.8, Novac 74.8/45.1 → 75.0/48.1, Strip 58.5/54.5 → 61.3/58.8, Primm
87.8/52.0 → 88.6/55.5.

Judged by eye along the way by a reviewing agent against `enhanced` (three rounds): dusk and
morning views clearly better (sunlit haze, layered mountains), noon and interiors level after
the grade's fix; the open items it ranked next are emissive light reaching the GI from beyond
the march, and true contact-hardening shadows.

## 7. Next candidates

- World-space light (a low-resolution voxel or probe pass) for what is off the screen and
  for occlusion of the glow field.
- Contact-hardening (PCSS) sun shadows: Bevy's needs the `experimental_pbr_pcss` feature and
  is noisy without temporal filtering; a blocker search of our own in the vendored sampler.
- The game's environment maps (shader texture slot 4) drawn on the shapes that name one.
- Volumetric light from point and spot lights indoors.
