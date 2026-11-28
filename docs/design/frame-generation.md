# FSR 3.1 frame generation

NVForever has two kinds of frame generation on one row, **Settings > Graphics > Frame
generation**: Off, **FSR 3.1** (this page) and **DLSS**, NVIDIA's DLSS Frame Generation through
Streamline ([dlss-frame-generation](dlss-frame-generation.md)). They never run together: FSR's
waits while DLSS-G is loaded. `--frame-generation` with no value (or `fsr`) chooses FSR 3.1's,
`--frame-generation dlss` DLSS's; an INI saved when the row was a switch reads its on as FSR 3.1.

FSR 3.1's: a frame is generated between each two real frames, so motion looks
smoother. It is the second half of #1778, after [latency reduction](latency-reduction.md),
and builds on [FSR 3.1 upscaling](fsr-upscaling.md). Code: `engine/nvforever/src/frame_gen.rs`;
the passes are in the vendored `wgpu-ffx` (`opticalflow.rs`, `frameinterpolation.rs`).

## Using it

**Settings > Graphics > Frame generation: FSR 3.1**, or `--frame-generation`. Off by default,
saved like every row. It runs when:

- FSR 3.1 is the upscaler that draws, in any mode (Native AA too): it reads the depth and
  motion FSR gets. Not with DLSS ([dlss-upscaling](dlss-upscaling.md)): frame generation
  assumes FSR drew the frame, and DLSS's pass makes none of FSR's state; with FSR asked for
  in a session that loaded Streamline (`NVF_DLSS_PRELOAD=1`), it runs as usual
  (`frame_gen::usable`, `graphics::fsr_active`, by the effective upscaler);
- vsync is on (Vertical sync or Variable refresh): the two frames are paced by the swapchain;
- the GPU runs the passes: Vulkan with SPIR-V passthrough and read-write storage textures in
  the formats they use (`frame_gen::FrameGenPlugin::finish`);
- the GPU reads and writes the output image without a format in the shader: Vulkan 1.3, and
  `R16G16B16A16_SFLOAT` with `STORAGE_READ_WITHOUT_FORMAT` and `STORAGE_WRITE_WITHOUT_FORMAT`
  in `VkFormatProperties3` (`frame_gen::storage_without_format`). AMD declares the output
  with no format, so two passes' SPIR-V needs the `StorageImage{Read,Write}WithoutFormat`
  capabilities [observed: their `OpCapability`s], and wgpu-hal 30 does not enable the Vulkan
  1.0 features that would allow them otherwise.

Otherwise the FSR 3.1 choice is dimmed with the reason ("Not on this GPU", "Not with DLSS", "Needs FSR 3.1",
"Needs vsync", checked in that order)
and the log says `frame generation: on but not running (...)`. While it runs, real frames are
capped slightly below half the refresh rate (78 at 160 Hz) and latency reduction is on
([latency-reduction](latency-reduction.md), AMD GPUs only).

## How it works

AMD's FSR 3.1 frame generation is two effects of FidelityFX SDK 1.1.4: **optical flow** (block
motion between the previous and the current colour, coarse to fine over seven levels, and
scene-change detection from luminance histograms) and **frame interpolation** (dilates the
game's depth and motion, builds motion fields from both sources, finds disocclusions, warps
both real frames to the midpoint, inpaints holes from a pyramid). Both are MIT. NVForever
runs AMD's GLSL compiled to SPIR-V and a Rust port of AMD's host code
([engine/vendor/README.md](../../engine/vendor/README.md), "wgpu-ffx").

Per real frame, in the camera's render schedule (`Core3d`):

1. After FSR (`fsr::fsr_pass`): frame interpolation's prepare pass on FSR's depth
   (the common preparation's `PreparedUpscale::depth`, `upscaler/prep.rs`, render size,
   first-person depth unsqueezed; only on a camera FSR draws) and Bevy's motion
   vectors, with FSR's scale (minus the render size), reversed infinite depth and FSR's
   jitter. AMD's host code passes the upscaler's jitter on
   (`ffxFsr3ContextDispatchFrameGenerationPrepare`); its pass reads it only to find
   display-resolution motion vectors (`ComputeHrPosFromLrPos`), so with Bevy's
   render-resolution ones it changes nothing [observed 2026-11-08, GPU harness: a square
   over a still background with jittered depth and motion, the same midpoint error 0.00064
   with and without it]. The frames it interpolates are FSR's output, so FSR's masks
   ([fsr-upscaling](fsr-upscaling.md), "The reactive mask") keep trails out of them too.
2. After the post passes, before the UI: a copy of the frame without the HUD.
3. After the UI, before Bevy's upscaling blit: optical flow on the HUD-less frame, then
   interpolation between the previous HUD-less frame and this one, into a display-sized
   texture (one of three used in turn). The main texture keeps the real frame, and Bevy's
   blit puts it in the window's stand-in (below).
4. After Bevy's `render_system` has submitted the frame (and a screenshot's copy into the
   stand-in, which it records after the render graph): the generated frame and the stand-in
   (the real frame) are handed to the window's presenter thread (`fg_presenter.rs`), which
   takes the window's images, copies them in and presents them, paced ("Presentation and
   pacing").

One display-sized copy a real frame is left, the frame without the HUD (two textures used in
turn, so the interpolation reads last frame's where it is). Until 2026-11-10 there were four:
the real frame was copied aside, the generated one into the main texture, and AMD's host code
copies the interpolation source for the next frame (`FrameInterpolationDispatchInfo::
previous_source` now takes the caller's). The window presented from the render thread
(`NVF_FG_PRESENTER=0`) still copies the real frame aside and the generated one into the main
texture, as Bevy takes its image from there.

Colours are the tonemapped frame, linear in [0, 1] (the window's sRGB view encodes them).
AMD's passes are told it is scRGB with 0 and 80 nits as black and white, which they read as
linear unchanged (`RawRGBToLinear`, transfer function 2).

### The HUD and menus

**The UI as a layer of its own** (2026-11-23, `fg_ui_layer.rs`): after Bevy's `ui_pass` the
camera's UI phase is drawn a second time, alone, into a display-sized texture cleared to
transparent black. Every Bevy UI pipeline blends with `ALPHA_BLENDING`, so that texture holds
the UI premultiplied by its coverage, and `frame = layer.rgb + (1 - layer.a) * frame without
UI`. The passes then interpolate the frames without the HUD only (no HUD-less mode), and the
layer is drawn over the generated frame (premultiplied "over"). The generated frame carries
this real frame's HUD exactly, its see-through and faint parts included. Cost: the UI drawn
twice and one full-screen blend a real frame. `NVF_FG_UI_LAYER=0` keeps the mode below, for
comparisons.

*Why it was needed* [observed: #3595, the HP bar's ticks doubled or
smeared in motion]: the HUD-less mode below takes the HUD's pixel in proportion to how much the
two frames differ, saturating at about 10% (`CalculateStaticContentFactor`), so the HUD's
faint parts (glow, soft edges, see-through fills) were mixed with the interpolated scene.

Before the layer (and with `NVF_FG_UI_LAYER=0`), AMD's "HUD-less" mode does it: the passes get the frame without the HUD (to interpolate) and
the frame with it. Where the two differ, the generated frame takes this frame's HUD pixels
(`computeInpainting`, weighted by how static they are). So the HUD is never interpolated: the
generated frame shows the newer real frame's HUD. *Observed* (2026-11-07, Goodsprings, FSR
Quality, strafing; a capture of generated and real frames): on the HUD's pixels (6% of the frame) the generated
frame differed from the real one by 0.2-0.4 of 255.

### When no frame is generated

Menus, dialogue, loading screens and pauses (every `PlayState` but Playing), a camera cut
(`UpscaleCamera::cut_from`, as for FSR's history), a frame FSR did not reconstruct (its spatial
fallback), and the first frame after any of them: those frames go through the passes with a
reset (to start the history again) and are presented once. AMD's scene-change detection also
stops interpolation for four frames after a detected cut; such frames show the real frame.

### Presentation and pacing

**A presenter thread** (`engine/nvforever/src/fg_presenter.rs`, 2026-11-08, h9x000/NVForever#1).
While frame generation runs, the window's images are not Bevy's: the vendored bevy_render's
`DeferredSwapchain` gives the frame a stand-in texture instead of a swapchain image
(three, used in turn), and `SharedSurface` lets one other thread take and present the
window's images (engine/vendor/README.md, "bevy_render"). After each frame is submitted the
render thread hands the stand-in and the real frame to the presenter and goes on with the next
frame; it waits only if the presenter is two frames behind, so nothing still to be copied is
drawn over. The presenter waits for the frame's GPU work, copies the generated frame into a
window image and presents it, then copies the real frame into the next one and presents it
half a real frame later.

Why: presented one after the other from the render thread (the path before), the real frame's
present waited in the driver until the generated frame was shown, about one refresh (4.4-6.2 ms
at 160 Hz), and the render thread recorded nothing meanwhile. Once a frame's own work passed
about 7 ms, work plus wait came to more than two refreshes, and the real rate fell from the cap
to 50-58 at 160 Hz, below what the game reached without frame generation [observed 2026-11-08,
the owner: 4K, maxed settings, 109 fps without frame generation, 58 real with it]. Taking the
window's images later or a deeper swapchain did not help (the wait is in the present, not the
acquire).

**Pacing** follows the design of AMD's `FrameInterpolationSwapchainVK` (FidelityFX SDK 1.1.4,
MIT; read as reference, no code taken): the two presents are spaced by half the mean real frame
time of the last 10 frames, less 0.1 x its standard deviation and 0.1 ms, never closer than one
refresh; the average starts again after a gap of more than 100 ms [documented:
`FrameInterpolationSwapchainVK.cpp` 1251-1268, with `varianceFactor` 0.1 and
`safetyMarginInSec` 0.0001 from `FrameInterpolationSwapchainVK.h` 216-217; AMD's `getVariance`
returns the square root of the variance, `FrameInterpolationSwapchainVK_Helpers.h` 373-391].
The spacing counts from when the last present was issued, not from when it returned. Two
choices are NVForever's own, not AMD's: until 10 frames are in, AMD's average is 0 and it
presents back to back, where NVForever averages the frames it has; and AMD waits by spinning
on the performance counter (`FrameInterpolationSwapchainVK_Helpers.cpp` 29-36), where
NVForever sleeps and spins only its last millisecond. The real frame rate is capped slightly
below half the refresh rate (`frame_gen::real_frame_cap`: 1/40 under it, at least 1 fps; 78 at
160 Hz, 71 at 144 Hz, 59 at 120 Hz), as AMD advises, so the two presents never fall within one
refresh.

Limits and notes:

- **Below the cap** the generated frame is still shown halfway between the real ones: the
  spacing follows the real frame time.
- **Latency**: the real frame is shown half a real frame after the generated one (6.3 ms at
  78 fps), as before within 0.1 ms. The generated frame is presented when the presenter sees the
  GPU finish the frame (polled about every 0.5 ms), where the driver showed it before: about
  0.3-0.5 ms later at the same frame rate, and much earlier where the old path fell below the
  cap. Latency reduction is turned on to offset it where the driver has it: AMD Anti-Lag, or
  NVIDIA low latency, whose markers map both presents to the rendered frame
  ([latency-reduction](latency-reduction.md), "Frame generation").
- **Catching up**: at the cap two paced presents take nearly a whole real frame, so a backlog
  (a late frame followed by an early one) drained by only about 0.2 ms a frame, and frames
  waited in the presenter for up to another frame [observed 2026-11-09, 4K, 78 real fps: the
  first present 13-21 ms after the frame was handed over in some 10 s periods, 7.4 in others].
  When the next frame is already waiting, the presenter now shows the late frame's real image
  only, a refresh after the last present, and leaves its generated frame out (the log line
  counts them): 7.3-7.75 ms in every period after, 8.5-9.0 with 4 ms more work, one generated
  frame left out in 14 periods of 10 s.
- **wgpu holds a device-wide lock across each present**, which stops submits from every
  thread while a present waits in the driver (1-1.6 ms each at 160 Hz with the presents 6.3 ms
  apart). The real frame's present falls about 1 ms after the next frame's submit, so the
  render thread is not held [observed 2026-11-08: it kept the cap].
- **Variable refresh** keeps vsync on (FIFO relaxed); the cap is the lower of the frame
  generation cap and half the page's or variable refresh's cap: those count frames shown, two
  for each real frame, as they do for DLSS-G (`frame_pace::own_cap`; until 2026-11-23 the
  page's cap held the real frames here, so a limit of 60 showed 120).
- **Vsync off** is not supported: the second present would replace the first at once.
- **Screenshots** (`--screenshot`, F12): Bevy points the camera's output at a screenshot
  texture of its own, and only after the render graph copies it to the screenshot's buffer and
  into the window's image (the stand-in, with the presenter) [documented: bevy_render 0.20.1,
  `prepare_screenshots` in `view/window/screenshot.rs`, `render_system` in `renderer/mod.rs`].
  On such a frame no generated frame is presented (`frame_gen::generate`, the camera's output
  against the window's stand-in): the passes still run, so the next frame interpolates from
  this one, and the real frame is presented once, so the screenshot and the screen both show
  it. The presenter is handed the frame after `render_system`, once that copy is in the
  stand-in.
- **An outdated or lost window surface** (a resize, alt-tab, a display mode or HDR change): the
  presenter skips its images and marks the surface; Bevy's `create_surfaces` reconfigures it on
  the render thread before the next frame, size changed or not. The presenter never configures
  the surface itself: wgpu-core 30's `configure_surface` waits for the GPU and reports a
  validation error when another thread submits meanwhile
  (`wgpu-core-30.0.1/src/device/resource.rs` 5338-5351), and the render thread keeps
  submitting.
- **A presenter thread that cannot start or stops** (a panic) is noticed when the next frame is
  handed over: an error is logged once, the presenter is dropped and the window is presented
  from the render thread, as with `NVF_FG_PRESENTER=0`, until the game restarts.
- **A presenter thread that is alive but held** (in the driver: a present or an image take
  that does not return) is waited for 250 ms a frame; a frame it has not caught up for by then
  is left out, not queued behind the ones it holds, and a warning is logged once. The frames
  go over again when it catches up.
- **While frame generation cannot run** (FSR off, vsync off: `frame_gen::usable`) the window
  is Bevy's: no stand-in, no presenter. Menus and pauses keep the presenter, which shows
  their frames alone.
- **Closing the window** ends its presenter at the next frame. The presenter keeps the window
  as long as it holds the window's surface (Bevy's `RawHandleWrapper`, as the render world does),
  so the surface is never left on a destroyed window.
- **Switching it off** ends the presenter (it presents what it was handed first) before Bevy
  takes the window's images again; switching it on starts a new one.
- **`NVF_FG_PRESENTER=0`** presents from the render thread as before, for comparisons.
  `NVF_TEST_RENDER_STALL_MS=<ms>` holds the render thread that long each frame (a heavy frame
  on a fast PC), `NVF_SWAPCHAIN_LATENCY=<n>` sets the frames the swapchain may queue, and
  `NVF_FPS_LOG=<s>` logs the performance overlay's readings as shown (with the overlay on,
  `--fps`: the first reading of each showing, then at most every `<s>` seconds): measuring
  aids.
- **The cap on slower screens**: half the refresh rate is 30 real frames a second on a 60 Hz
  screen, which is little to interpolate from and to aim with; frame generation suits screens
  of 120 Hz and more.
- **The performance overlay's FPS row** (`--fps`, the settings page's Frame rate counter)
  shows the frames shown and the game's own apart while frame generation shows generated
  frames (`156  (78 rendered)  12.8 ms  1% low …` at 160 Hz; the frame times beside them are
  the rendered frames'); the counter showed only the game's own, which read as frame
  generation halving the frame rate [observed 2026-11-08, 1280x720 at 160 Hz, FSR Quality,
  Goodsprings].
- **Swapchain depth**: Bevy's default (two frames queued). With the presenter, three gave the
  same as two (2026-11-08, 4K: 77.7 real fps, the first present 7.2-8.0 ms after handing over
  against 7.4-7.9); before it, three lowered the real rate (56 fps, 2026-11-07) and four gave the
  same as two.

## Checks

- **Unit tests** (`cargo test`): every optical-flow and frame-interpolation pass's binding table
  against its `.glsl` file's `#define`s and the callbacks header's declarations, and against the
  bindings the compiled SPIR-V declares; the `cbFI` block's std140 layout; the pyramid sizes; the
  real-frame cap, the present-interval statistics, the switch's effect on latency reduction;
  the presents' spacing (AMD's rule, with the standard deviation), and a presenter whose thread
  ended (it takes no frames and does not hold the render thread).
- **GPU harness** (`tools/fsr-validation`, `fg.rs`; numbers from 2026-11-07, RTX 5090, Vulkan
  validation layer on, no validation messages): a textured plane sliding at a known speed, the
  generated frame against the scene drawn at the true midpoint, mean absolute error per channel:

  | Case | Generated | Previous frame | Current frame | 50/50 blend |
  |---|---|---|---|---|
  | Native AA, 6,2 px/frame | 0.0001 | 0.0439 | 0.0439 | 0.0199 |
  | Native AA, 14,-5 px/frame | 0.0006 | 0.0886 | 0.0885 | 0.0609 |
  | Quality (render-resolution motion), 6,2 | 0.0001 | 0.0439 | 0.0439 | 0.0199 |
  | Quality, 14,-5 | 0.0006 | 0.0886 | 0.0885 | 0.0609 |

  Controls: a still scene comes back unchanged (0.00011); reversed game motion gives 0.049 and a
  reset on every frame 0.044 (against 0.0001); at a scene cut the generated frame is the new
  frame (difference 0.0000).
- **In game** (`NVF_FRAME_GEN_CAPTURE_DIR`, compared by script; Goodsprings,
  1280x720, FSR Quality, strafing): each generated frame differed from the real frames before
  and after it by less than they differ from each other, and about equally (7.7 and 7.7 against
  10.0; 14.3 and 13.9 against 16.6).
- **Smoke**: `frame-generation` (generated frames presented from the presenter thread, latency
  reduction on with it, no validation errors), `frame-generation-switch` (switched off and on
  again on the settings page: the presenter ends and a new one presents) and
  `frame-generation-needs-fsr` (held off without FSR, starts when FSR is chosen).

## Measurements

RTX 5090 (NVIDIA 621.14), Ryzen 9 9950X3D, Vulkan, 1920x1080 window on a 160 Hz monitor (vsync
presents as FIFO relaxed), FSR Quality, `--graphics enhanced`, a 30 s walk in Goodsprings,
2026-11-07:

| | Real fps | Presented fps | GPU time (mean per real frame) |
|---|---|---|---|
| Frame generation off | 132 | 132 | FSR 0.24 ms |
| Frame generation on (last 10 s) | 74 | 147 | FSR 0.24 ms, frame generation 0.41-0.46 ms |

Here the game already runs above half the refresh rate, so frame generation trades real frames
for presented ones: real frames are held near the cap of 80 (74 reached, 92%), and presented
frames rise from 132 to 147. It pays where the real rate is below half the refresh rate (a slower
GPU, a higher resolution), which is also where this path paces worst (above). The intervals
between the two presents, taken on the CPU, are short (median 0.4 ms) with a real frame's time
between pairs: the swapchain's queue spaces them. When each is shown was not measured (that
needs PresentMon or present timing from the driver).

**The presenter** (2026-11-08, h9x000/NVForever#1): the same PC at the owner's settings (a copy
of their `NVForever.ini`: 3840x2160 borderless, variable refresh, FSR Quality, every slider and
effect at the top), playing at the Goodsprings skyline at noon, 45 s runs, the last 30 s.
The heavy case adds 4 ms of render-thread work a frame
(`NVF_TEST_RENDER_STALL_MS=4`), standing in for the owner's heavier view:

| | Real fps | Shown fps | Between presents, 95th percentile | Presents' time on the render thread |
|---|---|---|---|---|
| Off | 128-131 | 128-131 | | |
| Off, 4 ms more | 90-91 | 90-91 | | |
| On, from the render thread (r7's path, cap 80) | 79.3-79.6 | 159 | | 4.2-4.4 ms a frame |
| On, from the render thread, cap 78 (`NVF_FG_PRESENTER=0`) | 77.7 | 155 | 13.5 ms | 4.4 ms a frame |
| On, from the render thread, 4 ms more | 55.6-55.7 | 111 | | 6.3 ms a frame |
| On, presenter (cap 78) | 77.8-78.0 | 155-156 | 6.9-7.0 ms | none |
| On, presenter, 4 ms more | 77.6-77.9 | 155-156 | 6.9 ms | none |
| On, presenter, walking and turning | 77.8 | 155 | 7.0 ms | none |

Below the cap (12 ms more work a frame, 51.5-52.2 real fps) the presents were 9.6 ms apart
(median; 95th percentile 10.1), half a real frame: the generated frame halfway [observed
2026-11-09, r8's build].

With frame generation off the presenter path is not used: 7.47-7.49 ms a frame against r7's
7.59-7.83 (`--log-frame-times`, GPU time 2.74 ms in both). The first version spaced the
presents from when each returned and fell short of the cap (74-77 real fps): each present's
0.26-0.47 ms in the driver was added to every spacing.

**The limiter, the copies and the bind groups** (2026-11-10). Three things changed:

- **The frame limiter waits before the next frame's input**, not after the frame is simulated
  (`frame_pace.rs`): right after the frame is handed to the render thread, where `anti_lag.rs`
  already announces the next frame's input. In `Last`, where it waited before, the wait lay
  between the player's input and the picture. A late frame is made up for a hundredth of a
  frame's time at a time, so the mean rate is the cap's.
- **The presenter reports how long frames wait in it** and the limiter holds the next frame
  back by half of what is over a millisecond, at most 1.5 ms (`frame_pace::catch_up`;
  `NVF_FG_LAG_FEEDBACK=0` turns it off). Two paced presents take nearly a whole real frame, so
  a presenter that falls behind stays behind until it leaves a generated frame out; held
  back, the wait is before the input instead, and few generated frames are left out. It
  costs real frames while it catches up (below): a choice between the two columns.
- **Less work a frame**: three of four display-sized copies gone ("How it works"), FSR's and
  frame generation's bind groups kept from frame to frame instead of made again for every
  pass (wgpu-ffx `pass.rs`, `fg_pass.rs`: about 40 bind groups and 170 texture views a
  frame), FSR's alpha pass only while the Pip-Boy's screen is on its model
  ([fsr-upscaling](fsr-upscaling.md)), and the presenter polls every 50 us once the GPU is
  nearly done instead of every 0.5 ms.

[observed 2026-11-10, RTX 5090 (NVIDIA), Ryzen 9 9950X3D, the owner's settings: 3840x2160
borderless, variable refresh (cap 152), FSR Native AA, every slider and effect at the top;
Goodsprings at noon, walking on and turning every two seconds, one game at a time, 100 s
runs, the 10 s periods after the first 20 s (`NVF_PACE_LOG=1`);
the view is held by the main thread, about 76 real fps before]

| Frame generation on | Before | After, catch-up off | After |
|---|---|---|---|
| Real fps | 71.4-77.4 | 77.4-78.1 | 71.7-76.3 |
| Shown fps | 140-154 | 153-155 | 142-153 |
| From a frame's start to its hand-off to the render thread | 12.8-13.2 ms | 7.7-8.8 ms | 6.7-8.1 ms |
| The limiter's wait in that | 2.2-3.3 ms | none (4.3-5.0 ms after it) | none (5.3-6.3 ms after it) |
| First present after the hand-over (the GPU's work in it) | 9.0-12.1 ms | 10.9-12.7 ms | 6.3-9.4 ms |
| Generated frames left out to catch up, in 10 s | 4-24 | 9-25 | 0-3 (11 in the first period) |
| Between presents, 95th percentile | 7.3-11.7 ms | not read | 7.9-9.8 ms |
| Render thread, recording FSR's passes | 282-303 us | | 78-94 us |
| Render thread, recording frame generation's passes | 508-542 us | | 62-87 us |
| GPU time, frame generation | 1.61 ms | | 1.47 ms |

With frame generation off and standing still (vsync off, `--log-frame-times`): FSR's GPU time
1.08 → 1.03 ms of 3.69 → 3.62 ms in all passes (the alpha pass); recording FSR 271-277 → 92-98
us; the frame rate is the main thread's there (88-103 fps in both, the same within the runs'
spread). From a frame's start to its first present, the two rows added: about 24 ms before,
20 with the catch-up off, 16 with it. The picture: a capture of generated and real frames
(1280x720, FSR Quality, strafing; `NVF_FRAME_GEN_CAPTURE_DIR`, compared as under "Checks") gives the generated frame 0.75-0.84 of
the real frames' difference from each neighbour and 0.20-0.33 of 255 on the HUD's pixels, as
before the change ("Checks"); a screenshot with frame generation on is of the real frame. Not measured: what reaches the screen and when
(PresentMon), and views the GPU or the cap holds, where the limiter's wait, and so what moving
it saves, is longer.

Not tested: AMD and Intel GPUs, Linux (the code is the same Vulkan path), exclusive full screen,
multiple windows, and how it looks to players over long sessions.
