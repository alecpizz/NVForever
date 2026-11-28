# FSR 3.1 upscaling

An optional temporal upscaler. The 3D view is drawn at a lower resolution with a sub-pixel
jitter, and AMD FidelityFX Super Resolution 3.1.4 (FSR 3.1) rebuilds it at the window's
resolution from this frame and its history. It is the temporal follow-up that #1639
(FSR 1, spatial) leaves out of scope: it brings the per-object motion vectors that issue
names as missing. The upscaler is the MIT-licensed wgpu-ffx port of the FidelityFX SDK 1.1.4
shaders, as SPIR-V, vendored with its shader sources and compile tool
(engine/vendor/README.md). No AMD DLL is used or shipped; a player's own FidelityFX API library
is called when one is in the game's folder ("The FidelityFX API path"). Frame generation,
built on top of it, is
[frame-generation](frame-generation.md). What FSR
needs and what the engine gives it: [spec notes](../spec-notes/fsr-upscaling.md). NVIDIA DLSS
is the other upscaler of the same settings row, on the same common preparation:
[dlss-upscaling](dlss-upscaling.md).

## Using it

**Settings > Graphics > Picture > Upscaler** offers Off (the default), FSR 3.1 and DLSS
([dlss-upscaling](dlss-upscaling.md)); **Upscaling mode** below it, Native AA, Quality,
Balanced, Performance and Ultra Performance, for either (dimmed while Upscaler is Off). They
are saved as `upscaler` and `upscaler-quality`; an older INI's single `fsr-upscaling` line,
with the names FSR's choices were always saved by (`Off`, `Native AA`, `Quality`, …), still
loads into both, and the test aids still take `fsr-upscaling=<choice>`. `--fsr <mode>`
(`off`, `native-aa`, `quality`, `balanced`, `performance`, `ultra-performance`), or
`--upscaler fsr --upscaler-quality <mode>`, sets the starting choice; a saved setting wins in
play, as for every setting. Every change is live; only DLSS asked for in a session that
started without NVIDIA's files takes a restart. The render size per axis is 1, 1/1.5, 1/1.7, 1/2 or 1/3 of the window's
[documented: AMD's FSR 3.1 guide, see the spec notes].

FSR does its own anti-aliasing, so while it is on, the anti-aliasing row is dimmed, reads
`Off (upscaling active)` (the same for DLSS) and takes no edits. The saved choice (MSAA,
FXAA, SMAA or TAA) comes back when Upscaler is Off. The Sharpening switch drives FSR's own RCAS sharpening in place of the
separate pass. The HUD and menus are drawn after FSR, at the window's resolution.

FSR runs on wgpu's Vulkan backend only (SPIR-V passthrough). On another backend, or a GPU
without the formats and limits it needs, the log says `FSR 3.1 unavailable: …` once, the
frame is drawn natively, and the anti-aliasing row stays editable.

The performance overlay's Upscaling row (`fps-counter-upscaling`, off by default; #2242) shows
the effective upscaler with the render and output size the render world drew the main camera
at (`upscaler::UpscaleDrawn`, written by `upscaler::prep::prepare`), for example
`FSR 3.1 Balanced  1129x635 -> 1920x1080`, or `Off` and the window's size while it draws
natively [design: `perf_overlay/readings.rs` `upscale_text`].

## Render contract

`fsr.rs` (`FsrPlugin`), on the view preparation every temporal upscaler shares,
`upscaler/prep.rs` (`PrepPlugin`, split out of `fsr.rs` unchanged for DLSS, #2198): the
history cuts, the render-size override, the jitter and mip bias on the camera, the corrected
and expanded depth, the resolution uniform, the spatial fallback and the alpha copy are
there, with FSR's own numbers (`fsr::policy`); FSR's context, masks and history textures
(`PreparedFsr`) stay in `fsr.rs` and are made for FSR's cameras only. The FSR context is made
the first time a view uses it; while a FidelityFX API library reconstructs the view (below),
neither it nor the port's dilation buffers are made. Each view with `FsrUpscaling` (the player's camera) has its
own history and textures.

- **Size.** Bevy's `MainPassResolutionOverride` draws the main passes into the top-left,
  render-size part of the display-size HDR target, and FSR fills the whole target. Native
  AA draws at 1:1.
- **Jitter and mip bias.** FSR's Halton jitter for the render/output ratio goes through
  Bevy's `TemporalJitter`, negated (Bevy displaces the projection the other way), with a
  texture mip bias of log2(render/output) − 1 [documented: AMD's FSR 3.1 guide].
- **Inputs**, in `Core3dSystems::EarlyPostProcess` before bloom and tonemapping:
  - the HDR colour;
  - the depth prepass, reverse infinite Z, with the first-person squeeze undone
    (`fsr_depth.wgsl`, `FIRST_PERSON_DEPTH_SHARE`);
  - Bevy's motion-vector prepass, scaled by minus the render size. `nif_prepass.wesl` keeps
    the previous transforms and skinning of NIF meshes, and grass and trees also move their
    previous vertex by the previous frame's wind and billboard turn (`grass.wesl`,
    `tree.wesl`);
  - a reactive mask and a transparency/composition mask (below);
  - the frame time.
- **History resets** (`UpscaleCamera::cut_from`, `upscaler/prep.rs`): a change of upscaler,
  a quality or size change, a skipped frame, a
  frame over 250 ms, a move of 400 units or more or a turn over 45° in one frame, a
  near-plane change, a magnification (tan(fov/2)) that changes by more than twice either
  way (`LENS_CUT_RATIO`), a new loading screen, and going into or out of dialogue or a
  loading screen (`play_state_cut`). A smooth zoom, such as aiming down the sights, keeps the
  history ("Aim zoom"), and so do the Pip-Boy and the other menus, which do not move the
  camera ("Mip bias, the Pip-Boy and the Strip").
- **After FSR**, for the passes that still read the scene on the display-size target:
  - The depth prepass is expanded to display size by nearest texels, with the first-person
    squeeze put back, for SSAO, the light shafts and the image-space blur.
  - FSR writes alpha 1, so each pixel's alpha is copied back from its nearest render texel
    (`fsr_fallback.wgsl` `alpha`), while the Pip-Boy's screen is on its model
    (`PipBoyScreen::on_model`): it is the one shape that marks its pixels in alpha, and the
    display-sized pass is saved otherwise (2026-11-10: FSR's GPU time 1.08 → 1.03 ms at 4K
    Native AA). That way the Pip-Boy screen's mark still keeps it out of
    the image-space grade (`imagespace.wesl` `SCREEN_MARK`). [observed 2026-11-06, the
    Pip-Boy open at Goodsprings at noon, clear weather, Native AA, 1280x720] Without the
    copy, the screen was graded with the scene: its mean luma was 0.208, against 0.158
    with FSR off. With the copy it was 0.151.
- **Screen-space lookups in the main pass.**
  - Refraction, the additive shapes' backdrop and water map the render viewport into the
    display-size transmission texture (`transmission_uv`).
  - Water looks its mirror reflection up with the unjittered projection, as the reflection
    camera draws without jitter.
  - Two `bevy_pbr` patches (engine/vendor/README.md) make the transmission pass and the
    clustered point and spot lights follow the override.
- **Fallbacks.** A view FSR can't use (multisampled, not HDR, or offset) logs a warning once
  and is drawn natively. A failed dispatch logs `FSR dispatch failed` and shows a bilinear
  upscale (`fsr_fallback.wgsl`).

## The reactive mask

FSR 3.1.4 reads two masks [observed, in its shaders: `ffx_fsr3upscaler_prepare_reactivity.h`,
`ffx_fsr3upscaler_accumulate.h`]:

- the **reactive** mask, dilated over 3x3, counts as a shading change: the pixel's
  accumulated history is cut back by it, and its colour clamp tightens;
- the **transparency and composition** mask tightens the colour clamp (from three times the
  neighbourhood's spread to once) and takes away the history's protection (FSR's locks on
  thin features and its luma-instability allowance), without cutting the accumulation.

The port has no `GenerateReactive` step, so `fsr.rs` makes both masks itself
(`fsr_reactive.wgsl`, two passes: `presence_pass` writes each pixel's presence (R8) and its
light change (R16F: this frame's luminance against FSR's last output at the same surface
point), `fragment` the two masks (R8 each) from those):

- The main texture is copied after the opaque pass (`capture_opaque`).
- **Presence**: that copy against the finished frame, relative to the brighter of the two,
  at most 0.9: whatever the transparent phase draws over the copy (particles, glass, water,
  neon tubes, blended meshes, hair).
- **The transparency/composition mask** is the presence here and where each pixel's history
  comes from last frame. FSR takes a pixel's history from where its motion vector points,
  and a blended shape is in no prepass, so the vector is that of the surface behind it: the
  history of the pixels a moving blended shape covered last frame still holds it, and smoke,
  a projectile's glow, a muzzle flash, a lamp's halo or hair left a trail. Last frame's
  presence (kept in a second texture) is read at the texels with a real share of that
  bilinear read; at a texel's centre that is the texel itself.
- **The reactive mask** is what changed: presence that appeared (above all of last frame's
  within a texel of the history's source) or went (above all of this frame's within a
  texel), and light that moved (below). A blended shape that stays on the surface behind it
  (a neon tube, a sign, a window, a decal) keeps its history: it is clamped, not redrawn from
  one frame. The one-texel margin keeps the jitter, which makes a thin tube's presence come
  and go between samples, from counting as a change.
- **Light that moved**: a moving light (a muzzle flash, a projectile's light, the flashlight,
  a swaying tree's shadow) changes the shading of surfaces that do not move, their motion
  vectors carry the old shading along, and FSR's history, an average over many frames, lags
  it even when each frame changes little. Each pixel's 3x3 texels are compared with FSR's
  last output at the same surface points (by each texel's own motion, this frame's jitter
  undone; `fsr_fallback.wgsl` `luma` keeps that output's luminance, render size). Where at
  least 8 of the 9 are brighter, or darker, by more than |ln| 0.12 (full at 0.5), the history
  is stale. With 7 of 9, thin aliased detail (slats, a ridge against a hill) set it off now
  and then and flickered 12% more.

Until 2026-11-08 one mask served as both: this frame's presence. That kept a still neon tube
reactive, so FSR drew it from one jittered frame at a time: a thin, very bright line whose
jagged edges moved every frame, and the bloom after FSR spread them into a wobbling halo
[observed: a player's video of the Strip at night, a still camera; the harness's neon case
below].

[observed 2026-11-08, `tools/fsr-validation` `ghosting.rs`: the engine's mask shader and FSR
3.1.4 on a synthetic scene, Quality, a textured background still or panned, the error of the
output against the scene drawn at the display pixels, tonemapped luminance] Before is this
frame's presence as both masks, after the masks above. The trails follow a disc moving 2.5
render pixels a frame:

| Case | Before | After |
|---|---|---|
| Alpha-blended smoke, still camera: trail | 0.0431 | 0.0336 |
| Alpha-blended smoke, panning: trail | 0.0166 | 0.0155 |
| Additive glow, still camera: trail | 0.0247 | 0.0107 |
| Additive glow, panning: trail | 0.0105 | 0.0074 |
| Moving light on opaque ground, still: trail (lit spot) | 0.0532 (0.0939) | 0.0276 (0.0547) |
| Moving light, panning: trail (lit spot) | 0.0198 (0.0323) | 0.0157 (0.0314) |
| Still neon tube: frame-to-frame change on it (error) | 0.0507 (0.0664) | 0.0029 (0.0554) |

Without any mask the tube changed 0.0016 a frame. On a moving disc itself the error rose a
little (smoke 0.0055 → 0.0073, glow 0.0011 → 0.0023): it keeps its clamped history where its
presence did not change. Clear of the shapes nothing changed, a fire-like flicker of the
whole ground (8%, slowly) is left to FSR (both 0.0104), and the lookup reversed (a control)
leaves larger trails. FSR's own shading-change detection does little for a moving light: in
its debug view the light gave at most 0.043, the flicker 0.023; it compares sorted sets of
luminance, which a textured surface's detail keeps overlapping. Raising its scale (AMD's
`fShadingChangeScale`, `FsrView::configure`) to 4 left the trail as it was.

FSR also cuts its history to 0.15 wherever anything moves faster than half a pixel a frame
at 4K (`ComputeBaseAccumulationWeight`, AMD's `fVelocityFactor`), so in motion fine detail
(roof edges, fence slats) is anti-aliased from about the last two frames and shimmers more
than when still. [observed, the harness's fine-detail case] Every velocity factor from 1 to
0.1 gave the same result (any real motion saturates it); 0 kept more history and raised the
error of panned detail from 0.034 to 0.043. AMD's default (1) stays.

What the masks cost: "Performance" below.

See also "Mip bias, the Pip-Boy and the Strip" below, measured frame by frame in game.

`NVF_FSR_DEBUG_VIEW=1` has FSR draw AMD's debug view over its output: a 3x3 grid, the
bottom row the disocclusions, the shading change with the reactive mask, and the
transparency/composition mask with FSR's motion divergence (its protection takedown: the
`REACTIVE` channel, `max(motion divergence, dilated T&C)` [documented: the vendored FSR
3.1.4 `ffx_fsr3upscaler_prepare_reactivity.h`]), in green. With the debug view the light term
is off: the output holds the grid, not the picture, so the history luminance is cleared each
frame instead of read from it, as on a reset, and the masks shown are those of play without
the light term. A diagnostic, not a setting.

Two bands of the transparent phase draw surfaces that are solid in effect.
`reference_sky_and_terrain_layers` draws them again over the copy before the mask is made,
with the same pipelines and depth, so they count as opaque:

- **The terrain's texture layers.** The terrain draws each extra layer as a blended copy of
  the quadrant (`terrain.rs`), sorted as one band at the start of the transparent phase
  (`nif_material::sort_first`). [observed 2026-11-05, owner report and FSR input captures
  at Goodsprings, heading 0, noon, Native AA, 1280x720] Left reactive, the mask was:
  - above 0.05 on 53% of the image and above 0.3 on 25%, nearly all of it textured ground;
  - so FSR kept little history there, and its dilation of the mask spread that to the
    twigs, blades and leaf edges standing on the ground, which shimmered with the jitter.

  With the band in the reference:
  - those shares fell to 11% and 3.3%;
  - with a still camera, the mean frame-to-frame change of tonemapped luma fell from
    0.0024 to 0.0007 over the image, and from 0.0099 to 0.0013 over a bush patch;
  - the bush patch now changes about the same at every quality. Before, its change grew
    from 0.0099 at Native AA to 0.0139 at Ultra Performance.
- **The sky's blended parts**: the cloud layers, the night's starfield, the stars and the
  moon (`sky::sorts_with_sky`, the band before the terrain layers). The sky dome itself is
  opaque and in the prepass (depth about 1.8e-5, 350,000 units away), so the sky has motion
  vectors. The parts drawn over it lie far beyond the world, so a turn of the camera moves
  them with the dome. [observed 2026-11-06, a still view at Goodsprings, FSR Quality,
  1280x720]
  - By day, the mask over the sky fell from a mean of 0.214 to 0.
  - The frame-to-frame change along the skyline fell from 0.00052 to 0.00044.
  - Turning at 6° a second, the motion-compensated change along the skyline fell from
    0.00441 to 0.00369, with no smeared clouds or silhouettes in the frames.

  The clouds' own drift is not in the motion vectors [hypothesis: slow enough for FSR's
  history clamping].

Water, refraction and every other blended mesh stay reactive. The copy is display-size to
share the depth attachment; only the render rectangle is read. The log gives each band's
draw count (`FSR reactive reference: …`), which smoke `fsr-foliage` checks.

## Blended cut-outs

A blended mesh is in neither the depth prepass nor the motion prepass, and it draws after the
opaque copy. So FSR sees what is behind it, under a reactive mask. That is right for glass,
but not for shapes whose alpha is a cut-out mask.

[observed 2026-11-05] Goodsprings' corn rows (`scol\scolmaizerow1b.nif`, `2b`,
`maizewitheredgroup2b.nif`) flickered under FSR. Their NiAlphaProperty is 0x12ED with
threshold 0 (blend SRC_ALPHA / INV_SRC_ALPHA, test GREATER 0), so they are drawn blended
(`nif_spawn::alpha_mode`), and their reactive value reached 0.9. The single corn plants cut
the same `maize_d.dds` at 124 and were fine.

Where alpha is 0 or 1, blending and alpha testing give the same pixels.
`nif_spawn::blended_cutout` marks a shape (`NifSurface::blended_cutout`) when all of these
hold:

- it blends SRC_ALPHA / INV_SRC_ALPHA and tests alpha below `BLEND_TEST_CUT` (64);
- nothing else sets its alpha: no vertex-alpha cut, material alpha 1, no alpha animation,
  falloff, light, decal, refraction or hair shading;
- its base texture's alpha is a cut-out mask (`texture::AlphaShares::is_mask`: at least 2%
  clear and at most 13% in between, in the top mip).

While FSR is on, `nif_material::BlendedCutouts` draws those materials alpha-tested at 0.5,
with depth and motion. With FSR off, they are blended as before. Copies that a fade or an
effect drives are not switched, and the shapes cast no shadow either way.

[observed, a one-off survey of the base game's and DLC's meshes] There are 144 such shapes,
in 115 files: the corn, 10 plant models, the tumbleweed, grave flowers, the quarry's
lattices, broken-wall stones, signs and glass shards. Dirty or clear glass stays blended:
`nvposter01.nif` is 40% in between, the glasses 26% and the Robobrain dome 68%. At the corn,
the reactive mean fell from 0.082 to 0.005, and the mean frame-to-frame change from 0.0040
to 0.0008. Smoke `fsr-corn-cutouts` turns FSR on and off there and checks both switches.

## Blended shapes drawn opaque (#3230)

`nif_spawn::blend_is_opaque` draws a shape opaque when it asks to be blended (SRC_ALPHA /
INV_SRC_ALPHA) but nothing can thin it: an opaque texture in every texel, material alpha 1,
no vertex alpha, alpha animation, falloff, light, decal, refraction, hair or window
environment map. With alpha 1 everywhere the blend gives the same pixels; the shape then
also goes into the depth and motion prepasses. The placed T-45d Power Armor
(`armor\powerarmor\powerarmorgo.nif`) was the reported case (#3347).

[observed 2026-11-27, `cargo run --release -p nvf-assets --example blend_opaque_survey --
<Data> schemas/nif <archives>`, which calls the rule's own code] Every model in the named
archives:

| Archives | Models | Shapes drawn blended | Drawn opaque by the rule | Alpha test off / on | Kept blended only by a window map |
|---|---|---|---|---|---|
| New Vegas base (`Fallout - Meshes`, `- Textures`, `- Textures2`, `- Misc`, `Update`) | 14,911 | 4,125 | 89 shapes in 82 models | 68 / 21 | 0 |
| Tale of Two Wastelands (`Fallout3 - Meshes`, `- Textures`, TTW's `Main` and `Textures`, with New Vegas' textures) | 11,284 | 1,615 | 43 shapes in 38 models | 30 / 13 | 0 |

In the base game, 45 of the 82 models are Anchorage-style metal corridors and pipes left
in `dlcanch\` (`Fallout - Meshes.bsa`); the rest are the displacer and zap gloves, the
throwing spear, the train station, the Sunsets Sarsaparilla glass and sign, McCarran's
terminal, The Tops' neon wall, the drinking glass and pitcher, the NCR glasses, two thrones,
the ootheca, three `go.nif` outfits and the power armour. In TTW, most are the `temp\` building blocks, a few weapons of the add-ons'
folders and TTW's food tins. No shape anywhere was kept blended by the window exclusion
alone.

[observed 2026-11-27, TTW, interior 0x0A015157 at (624, 9900, −511), the placed armour
0x0A01EF22 faced by `NVF_PLAY_TEST_FACE`, Enhanced look, 1920×1080] The armour on the table
is solid with DLSS Performance (frame generation did not start: DLSS's needs
hardware-accelerated GPU scheduling, off on this machine) and with FSR 3.1 Performance and
FSR frame generation running. The room's haze is drawn over it as over the table. The
report's faint outline was not seen.

## Mip bias, the Pip-Boy and the Strip

Players reported shimmering roofs and fences, on the Strip and in the Pip-Boy (2026-11-08).
To compare settings in game frame by frame, `NVF_FIXED_FRAME_TIME_MS` moves the clocks on a
fixed step every frame (Bevy's `TimeUpdateStrategy::ManualDuration`), so with
`NVF_PLAY_AIDS_FROM_SCRIPTS=1` the test aids reach the same view on the same frame in every
run, and `NVF_FSR_MIP_BIAS_OFFSET` replaces the −1 of AMD's mip bias. Both are diagnostics
(`dev\tools\mip-bias-capture.ps1`, `mip_bias_metrics.py`, outside the repository). The
reference is the same frames with FSR off at twice the size and 8x MSAA, scaled down by two.
Two runs of the same setting differed by 0.4 of 255 on still frames and 1.7 in motion (the
jitter's phase follows the frames since the start, which loading varies), and their scores
by under 0.02.

[observed 2026-11-08, RTX 5090, 1280x720, FSR Quality; mean |frame − reference| and
flicker, the frame-to-frame change the reference does not have, of 255] The mip bias:

| View | log2(r/d) − 1 (AMD) | − 0.5 | + 0 |
|---|---|---|---|
| Goodsprings roofs and fences, strafing: flicker (error) | 6.32 (6.00) | 6.32 (6.16) | 6.50 (6.46) |
| Goodsprings roofs, still: flicker (error) | 0.41 (5.41) | 0.34 (5.79) | 0.29 (6.41) |
| The Strip's signs, still: flicker (error) | 0.56 (3.90) | 0.53 (3.98) | 0.54 (4.19) |

A softer bias only takes a little flicker off still views; in motion, where the roofs and
fences shimmer, it does not help, and it loses texture detail everywhere. AMD's −1 stays.

**The Pip-Boy.** Opening it switches the play state to `Menu`, and every change of play state
used to count as a camera cut: each time the Pip-Boy opened or closed, FSR threw its history
away and the whole view was rebuilt from single jittered frames for about a second. The
Pip-Boy and the menus move no camera, so they keep the history now. On the background around
the raised Pip-Boy (the strips it does not cover), during its raise: error 3.83 → 3.38 and
3.10 → 2.51, flicker 0.89 → 0.79 and 0.77 → 0.69; its close the same; once it is up,
flicker the same (0.17), the brightness 1 of 255 apart (the eye adaptation followed a
different raise). With FSR off the raise scored 1.45 and 1.18: what is left is FSR's own
handling of the arm's fast motion and the blur behind the Pip-Boy, which reads the jittered
depth after FSR ("Limits"). On the screen itself, open and still, flicker was 0.08 at Quality
and 0.13 at Performance (0.00 with FSR off): its picture is a 1280x960 texture without mip
levels, about three times minified at Quality's render size. The screen's beam sweeps on the
real clock (`device_screen::sweep`), so it is in different frames from run to run: compare
the screen by eye.

## Aim zoom

[observed 2026-11-06, FSR input captures, a 9mm pistol aimed in and out at Goodsprings]
When every change of the field of view counted as a cut, all 18 frames of the aim zoom
(`nvf_play::aim_zoom`, 0.25 s each way) reset the history. Each aim showed the raw jittered
frame for a quarter of a second.

Bevy's motion vectors use the previous frame's unjittered projection, so a zoom reprojects
like any other camera move. Only a jump past twice the magnification (the ratio of
tan(fov/2), `LENS_CUT_RATIO`), such as the console's `fov`, is now a cut. A 20° sight's zoom
keeps the history at 15 frames a second and up (unit test
`an_aim_zoom_keeps_the_history_and_a_lens_jump_resets_it`).

## Ambient occlusion

**Since #2897 stage b it goes into the lighting by default** (graphics-enhanced.md §3.1.2):
worked out at FSR's render size between the depth prepass and the main pass, which draws at
that size and reads it, so FSR accumulates it with the lit scene. What follows is the picture
placement, now the fallback (`NVF_PLAY_DEBUG_AO=post`); `NVF_AO_BEFORE_FSR` applies to it only.

**Since 2026-11-08 it runs before FSR** (`ssao.rs` `ao_before_fsr`): at FSR's render size,
from the depth prepass as drawn (before `expand_depth`), at half that size, and multiplied
into the render rectangle of the HDR picture FSR reads, through a blend (`ssao.wesl`
`multiply`). The same factor goes into FSR's opaque copy, so the masks compare the two as
before. FSR then accumulates the occlusion with everything else: it is jittered with the
scene and smoothed by FSR instead of being worked out again, unsmoothed, on every frame of
the upscaled picture. Its largest radius on the screen is scaled to the render size, so it
covers what it did at the display size. It darkens the HDR light before tonemapping, so
bright surfaces lose less of their brightness than when it is multiplied into the
tonemapped picture. `NVF_AO_BEFORE_FSR=0` brings back the way below, for comparisons;
without FSR the pass is unchanged.

Before: `ssao.wesl` ran after FSR on the expanded depth, where one render texel covers 1.5
to 3 pixels. It took each normal from the neighbouring pixels, and there it found the same
texel again, with no depth step. So the normal faced the camera, and every surface seen at
an angle occluded itself.

[observed 2026-11-06, AO-on and AO-off screenshots of one still view at Goodsprings, FSR
Performance, 1280x720] This laid a grey veil over the scene: a mean AO factor of 0.958,
against 0.993 without FSR.

The normals now come from the neighbouring texels as drawn, at their centres (`normal_at`).
The common preparation (`upscaler/prep.rs`) sets `AmbientOcclusion::render_scale` while an
upscaler draws. At 1:1 the pass is
unchanged.

## Performance

What FSR saves is GPU time, and only in the passes drawn at the render size: the depth
prepass and the opaque and transparent passes. Bloom, the exposure, tonemapping, the grade
and the UI run at the window's size after it; the ambient occlusion ran there too until
2026-11-08 and now runs before FSR at the render size (below). FSR itself costs about the
same in every scene. NVForever's scenes are cheap
to draw for a GPU of today, so that fixed cost takes much of the saving, and in a light scene
all of it.

[observed 2026-11-08, RTX 5090, 3840x2160 borderless, `--graphics enhanced`, clear weather;
GPU time a frame over all top-level passes, `dev\tools\fsr-perf.ps1`]

| | Off | Native AA | Quality | Balanced | Performance | Ultra Performance |
|---|---|---|---|---|---|---|
| Goodsprings, noon | 3.55 ms | 4.29 | 2.78 (−22%) | 2.58 (−27%) | 2.41 (−32%) | 1.94 (−45%) |
| The Strip, 21:00 | 1.93 ms | 2.80 | 2.13 (+10%) | 1.99 (+3%) | 1.86 (−4%) | 1.66 (−14%) |
| FSR's own span (`nvf_fsr_3_1`) | | 1.13 | 0.77 | 0.71 | 0.64 | 0.56 |

Native AA is anti-aliasing, not a speed-up: it always costs more than Off. Of FSR's 0.77 ms at
Quality, AMD's upscaler is 0.52 (`nvf_fsr_3_1/upscale`, logged with `--log-frame-times`), the
masks and FSR's depth 0.12 (`inputs`), the alpha copy and the output's luminance 0.06
(`alpha_and_history`), and the expanded depth for the effects after FSR 0.06
(`depth_expand`). The masks first took twice as long: their light test sorted nine values in
an array indexed by a variable, which went to the GPU's local memory; it keeps the two
smallest and two largest in registers now (`inputs` 0.25 → 0.12 ms at Quality, 0.53 → 0.30 at
Native AA, the same masks). Before the masks of 2026-11-08 the whole span was 0.67 ms at
Quality and 0.94 at Native AA.

[observed 2026-11-10, the same PC, the owner's settings: 3840x2160 borderless, Native AA,
standing at the Goodsprings skyline, vsync off; two builds run in turn twice, each reading
the same to 0.001 ms] `inputs` 0.266 → 0.195 ms and the whole span 1.00 → 0.93 ms (1.08 before
the alpha pass was left out, "After FSR" above): the light change of a texel is worked out
once, in the first pass, where the second pass read motion, history and luminance again for
each of a pixel's nine neighbours, and the second pass takes its 3x3 neighbourhoods four
texels at a read (`textureGather`): about 15 reads a pixel where there were 51. The masks are
the same: all of `tools/fsr-validation`'s results, the ghosting cases among them, are
unchanged to the digits it prints.

So the frame rate rises with FSR only where the GPU sets the frame, and then by about the
share above. Where something else sets it, it does not move:

- **Vsync** (Display sync's default) holds frames to the refresh rate.
- **Frame generation** holds the game's own frames to just under half the refresh rate (29 on
  a 60 Hz screen, 78 at 160 Hz; [frame-generation](frame-generation.md)). The frame-rate
  counter counted only those, so turning frame generation on read as the frame rate halving;
  the performance overlay's FPS row shows the frames shown and the rendered ones apart now
  (`156  (78 rendered)`, `perf_overlay::readings::fps_text`).
- **The CPU**: towns, crowds and battles take 6-46 ms a frame of CPU on the owner's i9-14900K
  (docs/design/frame-rate-audit.md §5), which FSR does not touch.
- On the machine measured here, presents were held to the 160 Hz refresh even with vsync
  off (`--log-frame-times`): a small interior whose GPU time was 0.29 ms ran at exactly 6.25
  ms a frame. Measure FSR by GPU time.

On a GPU several times slower than this one the same shares apply where it sets the frame
[hypothesis: the passes' costs were not measured on other GPUs].

### Half precision and the ambient occlusion before FSR (2026-11-08)

Two changes cut the GPU time under FSR: AMD's half-precision (FP16) variant of the
accumulate pass, where the GPU has 16-bit shader floats and integers (wgpu `SHADER_F16` and
`SHADER_I16`; `NVF_FSR_FP16=0` keeps full precision), and the ambient occlusion before FSR
("Ambient occlusion" above). Only the accumulate pass has a working FP16 variant: the luma
and shading-change pyramids' FFX_HALF code is empty in SDK 1.1.4, and the other passes do
not read FFX_HALF (engine/vendor/README.md).

[observed 2026-11-08, RTX 5090, 3840x2160 borderless, `--graphics enhanced`, clear weather,
frame generation off; GPU time a frame over all top-level passes, medians of 9 periods of
4 s, the builds interleaved and the first run again at the end of each block
(`dev\tools\fsr-compare-perf.ps1`); runs whose window lost focus (held to 60 frames a
second by the driver, which also lowered the GPU's clocks: 2.99 ms against 2.80-2.83) were
run again]

| GPU ms a frame | Off | Quality before | Quality now | Performance before | Performance now |
|---|---|---|---|---|---|
| Goodsprings, noon | 3.67 | 2.81 | 2.68 (−5%) | 2.40 | 2.18 (−9%) |
| Novac, noon | 3.10 | 2.73 | 2.61 (−4%) | 2.38 | 2.24 (−6%) |
| Primm, noon | 3.52 | 2.72 | 2.52 (−7%) | 2.18 | 2.08 (−5%) |
| The Strip, 21:00 | 1.93 | 2.13 | 2.01 (−5%) | 1.84 | 1.72 (−6%) |

With the sun's shadows on, as in play (`--sun-shadows`; the views above have none), the same
0.1-0.15 ms is a smaller share: Quality 3.16 → 3.12 ms at Goodsprings, 3.15 → 3.06 at Novac,
2.96 → 2.86 at Primm, 2.25 → 2.12 on the Strip; Performance 2.65 → 2.51, 2.77 → 2.63, 2.56
→ 2.44, 1.96 → 1.83 (FSR off: 4.27, 3.56, 3.77, 2.09). On this machine none of it reaches
the frame rate: frames stay at the 160 Hz the presents are held to, and at Goodsprings with
FSR off something other than the GPU sets the frame (9.5 ms against 4.27 of GPU time).

Most of it is the ambient occlusion: 0.14-0.18 ms after FSR at the display size, 0.08-0.09
at Quality's render size and 0.05 at Performance's. FP16 makes AMD's upscale 0.02-0.03 ms
cheaper (0.51-0.52 → 0.49-0.50 ms at Quality, 0.44-0.45 → 0.42-0.43 at Performance); on this
GPU 16-bit arithmetic runs at the 32-bit rate, so the gain is registers and bandwidth only
[hypothesis: GPUs with double-rate packed FP16, AMD's RDNA among them, may gain more; not
measured]. The whole frame of identical runs differs by up to 0.26 ms, so the totals rest on
the passes' own spans.

On the picture [observed, frame-exact captures in the four towns at 1280x720, Quality,
against FSR off at 2560x1440 with 8x MSAA scaled down, `dev\tools\town-capture.ps1`,
`fp16_ao_metrics.py`]:

- **FP16 changes nothing measurable.** Error, flicker and detail against the reference are
  the same as full precision's in every town, still and moving; the frames differ per pixel
  as much as two runs of one build do (Goodsprings 2.5 and 2.3/255). In the GPU harness the
  panning cases' background error rises from 0.0054 to 0.0059 with FP16 and the trails
  shrink a little (`tools/fsr-validation`, `NVF_FSR_FP16=1`).
- **The occlusion no longer shimmers.** After FSR it was worked out again on every frame
  from the jittered depth, unsmoothed; before FSR, FSR accumulates it. The flicker of still
  frames against the reference: Goodsprings 0.33 → 0.22, Novac 0.25 → 0.17, Primm 0.33 →
  0.24, the Strip 0.54 → 0.47, the same as with no ambient occlusion at all (0.21, 0.17,
  0.24, 0.47); at Performance 0.33 → 0.19 at Goodsprings. Moving frames are unchanged.
- **The exposure moves a little.** With the occlusion in the HDR picture, eye adaptation
  meters a slightly different average, and Bevy's 8-bit, point-sampled compensation curve
  turns that into a step: Primm 3 % darker, Goodsprings 1 % brighter than without it (with
  the light glow and eye adaptation off, the occlusion darkens exactly as after FSR). The
  steps come from the curve, not the occlusion ("Limits").

## The FidelityFX API path

Tools that replace a game's upscaler (OptiScaler) take over the calls a game makes into an
upscaler's library. The wgpu-ffx port makes no such call, so such a tool finds nothing to
take over [observed 2026-11-11, a player's report: OptiScaler's overlay read "FSR Hooks: Don't
Exist"] (#2169). When a FidelityFX API library is in the game's folder, the upscale goes
through it instead, the way a game built on AMD's SDK calls it (`ffx_api.rs`).

**State 2026-11-12.** One run by the owner with OptiScaler 0.9.4 (as `dxgi.dll`) and its
`amd_fidelityfx_vk.dll` in the folder [observed: RTX 5090, 4K, Native AA, frame generation
on]: OptiScaler hooked all five entry points; 17,847 dispatches, none failed; the output was
written under OptiScaler's FSR 2.2.1 (its default on Vulkan), FSR 3.1 and FSR 3.1 through
its DX12 bridge; frame generation ran on top (about 75 real, 151 presented frames a second);
the context was destroyed on exit. While OptiScaler changes its upscaler (about a second) it
returns OK without writing the output. AMD's own `amd_fidelityfx_vk.dll` (SDK 1.x, without
OptiScaler) [observed 2026-11-13: RTX 5090, 1280x720 and a resize to 4K, Quality]: a context
at each size, every dispatch ok, alone and with Streamline loaded (DLSS's files in the folder,
the device made through its interposer); see "Vulkan functions the library asks for" below
for what it took. Not run: FSR SDK 2.x's library, a change of size or mode in play, the
Vulkan validation layer, DLSS and XeSS behind OptiScaler, an AMD or Intel GPU. What is said below about those is from the
libraries' sources and headers.

- **What is loaded.** The first of these that loads, by its bare name, the first time a view
  uses FSR: `amd_fidelityfx_loader_vk.dll` (FSR SDK 2.x's loader, which finds its upscaler
  library `amd_fidelityfx_upscaler_vk.dll` itself: FSR 4 where the GPU runs it), then
  `amd_fidelityfx_vk.dll` (SDK 1.x: FSR 3.1). Only a file that is in the executable's
  folder is asked for, so a file of that name in System32 or on `PATH` is never loaded. The
  load itself searches the executable's folder and System32, never the working directory or
  `PATH`; System32 stays in because the library's own imports (the Vulkan loader, the C
  runtime) are found with the same flags [documented: `LoadLibraryExW`,
  `LOAD_LIBRARY_SEARCH_*`]. With the file present, a failed load is reported as one (Windows
  error 126 then means one of the library's own imports is missing, as with SDK 2.x's loader
  but no `amd_fidelityfx_upscaler_vk.dll`), and a library without all five entry points is
  unloaded again. No signature, hash or version is checked: OptiScaler's files are unsigned,
  and the trust is that of any DLL beside an executable. The bare name is what a tool that
  hooks the load compares [observed 2026-11-12: OptiScaler's source,
  `LibraryLoad_Hooks.cpp`, `DllNames.h`], and such a tool may answer the load with a library
  of its own. NVForever ships no such file; without one the port runs as before.
- **The setting.** Settings > Graphics > Picture > FSR library (`fsr-library`, under DLSS
  model; dimmed, reading `FSR only`, while FSR is not the upscaler that draws, as DLSS model is
  for DLSS): Built-in, or Game folder, shown with the library found (`Game folder (amd_fidelityfx_vk.dll)`, `(none
  found)`, or the library's name and `failed`). Game folder is the default; with no library
  there the port runs. Switching makes the view again with the other upscaler, and
  switching back to Game folder tries a library that failed once more. `NVF_FFX_API=0`
  keeps the port whatever the folder and the setting say, and the row then reads
  `Built-in (NVF_FFX_API=0)`.
- **The calls.** The five entry points of FidelityFX SDK 1.1.4's `ffx_api.h`. One upscale
  context for each view and pair of sizes (`ffxCreateContext`: the upscale descriptor with
  the port's flags, and the Vulkan backend descriptor with the device's handles from wgpu's
  `as_hal`), `ffxDispatch` once a frame, `ffxDestroyContext` when the sizes change or the
  view goes. The declarations follow the SDK's MIT headers; a test holds their layout.
- **FSR SDK 2.x's library** takes the same upscale and dispatch structures [documented: SDK
  2.4.0 `ffx_upscale.h`, `ffx_api_types.h` against 1.1.4's: fields and values unchanged, new
  ones appended]. Two things differ: its Vulkan backend descriptor has a type of its own
  (`0x01000003`; the same three handles), and it is told the upscaler API version the game
  was built against (`ffxCreateContextDescUpscaleVersion`, here 4.1.2, the 2.4.0 header's).
  Those four headers are MIT (each carries the notice and is on the list of the SDK's
  `docs/license.md`, whose default for everything unlisted is binary-only); nothing else of
  SDK 2.x was read or used.
- **The inputs** are the port's: the colour (the display-sized main texture, the render
  rectangle top-left), Bevy's motion vectors, both masks, the jitter, the motion scale, the
  frame time, the reset, reversed infinite depth. The depth goes as an `R32Float` colour
  copy of the common corrected depth (`upscaler/prep.rs` `depth_as_color`, `fsr_depth.wgsl`): the API has no depth formats, and OptiScaler
  makes its view under the format the API names, which a depth image does not allow
  [observed: OptiScaler's source, `FfxApi_Vk.cpp` `CreateIVandNVRes`].
- **Vulkan functions the library asks for.** It is given `vkGetDeviceProcAddr` and looks
  up what it calls. AMD's library asked for functions this device was not made with
  (`vkGetBufferMemoryRequirements2KHR`, `vkSetDebugUtilsObjectNameEXT`,
  `vkCmdBegin/EndDebugUtilsLabelEXT`, `vkCmdWriteBufferMarkerAMD`,
  `vkCmdWriteBufferMarker2AMD`), got NULL, called one while it made its context, and the
  game stopped with an access violation at address 0 [observed 2026-11-13, crash dumps].
  So the library gets NVForever's own lookup in front of the device's (`ffx_api.rs`
  `device_proc_addr`): a `KHR`/`EXT` name the device lacks is looked up again under its core
  name (an extension promoted to core keeps its functions there, identical [documented: the
  Vulkan specification, "Extension Promotion"]); the debug names and labels, which change
  nothing the GPU does, are answered by a no-op; anything else by a stub that notes the call.
  A library that calls such a stub is given up (at its context: not used; in a frame: a
  failed dispatch), and the log names what it asked for. AMD's library then ran, asking for
  the two AMD buffer markers without calling them on this NVIDIA GPU.
- **Three command buffers a frame.** wgpu 30 allows no raw commands in an encoder that has
  commands of its own [documented: wgpu-core 30.0.1 `EncodingApi::set` panics]. So the
  library records into an encoder used only through `as_hal_mut`, and Bevy gets ours before,
  the library's, and ours after, in that order (`RenderContext::add_command_buffer`).
- **Image states.** wgpu does not track the library's commands. The last thing in ours
  before them is `transition_resources`: the inputs to `RESOURCE`
  (`SHADER_READ_ONLY_OPTIMAL`), the output to `STORAGE_READ_WRITE` (`GENERAL`), and the API
  is told `COMPUTE_READ` and `UNORDERED_ACCESS`. AMD's library returns every image to the
  state it was given [documented: SDK 1.1.4 `ffx_vk.cpp` `UnregisterResourcesVK`], and
  OptiScaler assumes exactly these layouts whatever a game names [observed: its source,
  `IFeature_VkwDx12.cpp`, `FFXFeature_Vk.cpp`], so wgpu's tracking holds afterwards
  [hypothesis until a run under the validation layer]. wgpu clears a texture it has seen no
  write to before its first read, and it does not see the library's write: a main texture
  given to the library for the first time is cleared by us first.
- **Alpha.** The port's output has alpha 1; what another upscaler behind the API leaves there
  is not known, so on this path the scene's own alpha always comes back (`restore_alpha`,
  one display-sized pass the port only needs with the Pip-Boy's screen).
- **When it fails.** A context that cannot be created, or a dispatch that fails three frames
  in a row, ends the library's use for the session: the log says why, and the port takes the
  view over. A frame whose dispatch failed shows the spatial fallback.
- **Destroying a context** waits for the GPU first (at most two seconds): the library frees
  its Vulkan objects at once, and wgpu cannot hold them for commands it does not know. If the
  wait runs out the context is left undestroyed (its memory stays used), rather than freed
  under commands the GPU may still run.
- **Frame generation** is unchanged: it works from the engine's depth and motion vectors and
  the upscaled frame, not from the upscaler's own textures
  ([frame-generation](frame-generation.md)).
- **Licence.** No AMD file is in the build or the package; what a player puts in the folder
  is theirs. LEGAL.md and [dependencies](dependencies.md) stand as they are.
- **Not covered.** The library's GPU memory in the memory budget; translations of the
  settings row; a smoke scenario (it needs a library to load). AMD's backend
  picks its shader variants from what the GPU supports, not from what the device enabled
  [documented: SDK 1.1.4 `ffx_vk.cpp` `GetDeviceCapabilitiesVK`]: untested on an AMD GPU.
- **What the log says.** Which library loaded and the file it came from (a tool that
  answers the load shows there), a library that failed to load and why, a context that could
  not be made, and a dispatch failure. The observations above came from a development trace
  (entry-point owners, every call, a read-back of the output) used while the path was built
  and removed before review; it is in this branch's history (`ffx_api_trace.rs`).

## Limits

- **Backend and speed.** Vulkan only. AMD's FP16 variants only for the accumulate pass, and
  only on GPUs with 16-bit shader floats and integers; frame generation's passes stay full
  precision. The gain depends on how busy the GPU is; a CPU-bound scene gains nothing, and a
  light one can cost more than it saves ("Performance").
- **Eye adaptation's exposure steps.** Bevy reads its exposure compensation curve from an
  8-bit table of 256 entries without interpolation, so NVForever's curve (eye_adapt.rs, which
  should leave daylight unchanged) leaves it 0 to about 0.15 EV too dark, by where the
  metered average falls, and steps when that crosses an entry. Any change to what is metered
  moves the brightness by a few per cent; the ambient occlusion before FSR is one [observed
  2026-11-08; read in Bevy 0.20.1's `auto_exposure.wesl` and `compensation_curve.rs`]. Not
  fixed here.
- **Surfaces whose picture changes by itself** have no mask of their own: animated textures
  and render-to-texture screens (the Pip-Boy's, terminals, a scope's lens). The masks' light
  term catches a change that brightens or darkens most of a 3x3 area, not detail that moves
  across a surface (text scrolling on a screen), which may trail [hypothesis: not measured].
- **Blended shapes** have no depth or motion of their own: the masks clamp the history where
  they are and were last frame and cut it where they came or went, so a fast one is drawn with
  less accumulation (more aliasing) rather than with a trail. A moving shape with detail of its
  own (a smoke puff's texture) may still smear a little inside, where its presence stays.
- **Fine detail in motion** (roof edges against a hill, fence slats, power lines) shimmers more
  than when still: FSR 3.1 keeps little history on anything that moves (above); neither its
  velocity factor nor a softer mip bias helps ("Mip bias, the Pip-Boy and the Strip").
- **The Pip-Boy's screen** is a texture without mip levels, minified about three times at
  Quality (more at lower modes): its text and scanlines alias, and FSR's jitter moves that
  aliasing a little every frame.
- **Jittered depth after FSR.** SSAO, the light shafts and the image-space blur read a depth
  drawn with the frame's jitter, so their edges move by up to half a render texel from frame
  to frame. Computing the occlusion before FSR would fix that, but would change the look:
  it is multiplied in after tonemapping now.
- **Thin and fine detail.** FSR replaces MSAA, so thin alpha-tested strands lose
  multisampled coverage, and FSR resolves their edges over time instead. Fine textures at
  grazing angles shimmer at Performance (minification under the mip bias).
