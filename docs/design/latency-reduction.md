# Latency reduction (AMD Anti-Lag, NVIDIA low latency)

An optional switch: the GPU driver keeps the game from preparing frames ahead of the graphics
card, which is meant to bring input to the screen sooner [hypothesis: not yet measured, on
either vendor's GPU]. It is the first half of #1778; FSR 3.1 frame generation, which adds
latency that this offsets, turns it on while it runs ([frame-generation](frame-generation.md)).
One switch, two driver techniques, each through a public Vulkan extension and with no vendor
code in NVForever:

| GPU | Technique | Extension | Code |
|---|---|---|---|
| AMD | Anti-Lag | `VK_AMD_anti_lag` | `engine/nvforever/src/anti_lag.rs` |
| NVIDIA | Low latency mode (what NVIDIA's own SDK calls Reflex) | `VK_NV_low_latency2` | `engine/nvforever/src/nv_low_latency.rs` |

`anti_lag.rs` owns the switch, the frame indices and the points in the frame; it calls into
`nv_low_latency.rs` from the same points.

**Through Streamline's Reflex** (#2317): when NVIDIA's DLSS Frame Generation files are in the
`dlss` folder and frame generation was set up at the start, Streamline's Reflex (`sl.reflex`,
`sl.pcl`) stays loaded for the session, and every NVIDIA call goes through it instead of the
direct extension calls: the same points call `slReflexSleep` and `slPCLSetMarker` every frame,
whatever the switch, and the switch sets Reflex's mode (`eLowLatency` or `eOff`) with
`slReflexSetOptions`, only when it changes (`nv_low_latency.rs` `ViaStreamline`). Reflex's own
backend makes the same driver calls on the swapchain it tracks [documented: Streamline 2.14.16
`vknvll2.cpp`]; DLSS Frame Generation requires it on (`eFailReflexNotDetectedAtRuntime`) and
turns latency reduction on while it runs, as FSR's frame generation does. While frame generation
runs, the frame cap goes whole to Reflex's `frameLimitUs`, which holds the frames shown, generated
ones included, and frame pacing does not wait itself; Reflex's options are set again after
every new swapchain, which does not get them otherwise (#2732). When
frame generation could not start, both are unloaded and the direct calls run as before. The log
names it: `latency reduction: NVIDIA Reflex through Streamline available on …` at the start,
`NVIDIA Reflex (Streamline)` as the technique, and with `NVF_PACE_LOG=1` a line from Reflex's own
latency report every ten seconds. Design: [dlss-frame-generation](dlss-frame-generation.md).

## Using it

**Settings > Graphics > Latency reduction**, or `--latency-reduction`. Off by default, saved like
every switch. It works where the GPU's Vulkan driver has one of the two extensions:

- AMD, Windows: Adrenalin 24.9.1 or later [documented: GPUOpen, Anti-Lag 2 SDK requirements].
- AMD, Linux: Mesa RADV 26.3 or later; Mesa 25.3 to 26.2 had it as an implicit layer
  [documented: Mesa release notes, via Phoronix].
- NVIDIA: a driver whose Vulkan device lists `VK_NV_low_latency2` and `VK_KHR_present_id`, is
  Vulkan 1.2 or later and has timeline semaphores. Which driver version first had them was not
  looked up; the start's log line says what this one has. [observed 2026-11-12, `vulkaninfo`:
  an RTX 5090 on Windows driver 621.14 lists `VK_NV_low_latency2` revision 3,
  `VK_KHR_present_id`, Vulkan 1.4 and `timelineSemaphore`.]

The start says once which technique this GPU uses, or why neither:

- `latency reduction: AMD Anti-Lag (VK_AMD_anti_lag) available on <adapter>`
- `latency reduction: NVIDIA low latency (VK_NV_low_latency2) available on <adapter>`
- `latency reduction: not available on <adapter>: no AMD Anti-Lag (...) and no NVIDIA low
  latency (<what is missing>)`: the row is dimmed ("Not on this GPU").

Turning the switch on or off says `latency reduction: on (NVIDIA low latency)`, with
`, with frame generation` when frame generation turned it on. The settings row itself has one
label and note for both techniques: a row that is not dimmed shows only On or Off, so the log
is where the technique is named.

On NVIDIA three more lines can follow:

- `latency reduction: NVIDIA low latency mode is set on the window's swapchain (boost off); the
  driver now times each frame's start`: the driver accepted the mode (once a run).
- `latency reduction: the driver refused NVIDIA low latency (<call>: <result>)`: it did not
  (once a run); frames go on unpaced.
- `latency reduction: the driver did not end an NVIDIA low latency sleep within 100 ms`: a sleep
  ran to its limit (at most once every ten seconds).

`NVF_PACE_LOG=1` adds, every ten seconds while it is on, `latency reduction: NVIDIA low latency
slept <n> frames in <s> s: <mean> ms a frame, the longest <ms> ms, <n> not ended by the driver`:
how long the driver held each frame's start back. `NVF_NV_LOW_LATENCY=0` never asks the driver
for the extension, so the device and the swapchain are made as on a GPU without it (a way out
if a driver misbehaves with it; the row is then dimmed).

## How it works: both

- **Device creation.** Bevy creates the Vulkan device. Its `raw_vulkan_init` feature (bevy_render
  0.20.1) lets `AntiLagInitPlugin`, added before `DefaultPlugins`, add device extensions through
  callbacks. Each technique's callback adds its extension only when the adapter offers it and
  what it needs. This is the hook Bevy's own DLSS support uses.
- **Where the next frame's input is read.** After the main schedule, each update runs the
  sub-apps' extract. With pipelined rendering, which the game keeps, `RenderExtractApp`'s extract
  waits for the render thread and hands it the frame just simulated; only then does winit
  deliver the next frame's input [documented: bevy_app 0.20.1 `sub_app.rs` `SubApps::update`;
  bevy_render 0.20.1 `pipelined_rendering.rs`, `renderer_extract` and its frame diagram]. So
  `AntiLagPlugin::finish` wraps that extract, and right after the hand-off come the frame cap's
  wait (`frame_pace::after_hand_off`) and then the driver's call for the next frame: a wait in
  it then comes before the next input, not inside the latency of the frame just handed over.
  Without pipelined rendering (no `RenderExtractApp`) both fall back to the end of `Last`.
- **Frame indices.** The main world counts frames; the index is extracted with the frame, so
  the render world's calls carry the index of the frame it draws.
- **Side effect of `raw_vulkan_init`.** When the backends include Vulkan (the default: all of
  them), Bevy creates a Vulkan-only wgpu instance whenever Vulkan starts; DX12 and GL stay only
  for when the Vulkan instance can't be created (bevy_render 0.20.1
  `renderer/raw_vulkan_init.rs`). FSR, the GPU memory budget and the gpu-allocator patch already
  work through Vulkan.
  - What remains: if the Vulkan instance is created but no Vulkan adapter can draw to the
    window, the start stops at Bevy's "Unable to find a GPU" instead of picking DX12
    [documented: bevy_render 0.20.1 `renderer/mod.rs`, `initialize_renderer`; not reproduced].
    `WGPU_BACKEND=dx12` gets round it.
  - Undone in the vendored bevy_render (`engine/vendor/README.md`): upstream, the feature builds
    the Vulkan instance whatever `WGPU_BACKEND` says, and turns wgpu's indirect-call validation
    off in every release build, the DX12 fallback included. Now the Vulkan instance is made only
    when the backends include Vulkan, and `VALIDATION_INDIRECT_CALL` is removed only when they
    don't include DX12, as without the feature.

## How it works: AMD Anti-Lag

- **The extension.** `VK_AMD_anti_lag` (Vulkan registry extension 477) adds one call,
  `vkAntiLagUpdateAMD`, and one feature, `antiLag` [documented: `vk.xml`]. The callback chains
  `VkPhysicalDeviceAntiLagFeaturesAMD` with `antiLag` on, only when the adapter offers both
  (`vkGetPhysicalDeviceFeatures2`). ash 0.38 predates the extension, so its structures are
  written out from the registry.
- **The calls** [documented: Khronos `proposals/VK_AMD_anti_lag.adoc`].
  - *Input*: "immediately before input to the application is processed". The call may block, to
    keep the CPU from running ahead. It goes right after the hand-off (above), with the next
    frame's index.
  - *Present*: "before `QueuePresent`", with the same index. Bevy's renderer draws frame n while
    the main world runs frame n + 1. The render world sends it just before Bevy's
    `render_system`, which submits and presents in one step [engineering choice: the latest
    point before the present that Bevy offers].
- **Off** sends mode OFF every frame. `maxFPS` is 0: the frame cap stays `frame_pace`'s
  [engineering choice; the driver's limiter could take it over once Anti-Lag can be measured].

## How it works: NVIDIA low latency

Everything marked [documented] here is the Vulkan specification's "Latency Reduction" section
(Vulkan-Docs `chapters/VK_KHR_surface/wsi.adoc`, `VK_NV_low_latency2` revision 3 of 2026-04-20)
or `vk.xml`, read on 2026-11-12. The registry is under Apache-2.0 OR MIT (the specification
text CC-BY-4.0). NVIDIA's Streamline, its Reflex SDK and NvAPI are not used, read or shipped:
their licences do not fit GPL-3.0-or-later. ash 0.38 (Vulkan 1.3.281) has the extension's
structures and calls, the same as the registry's now [checked against `vk.xml`: the four
structures used, the marker values and the three commands].

- **The extension.** `VK_NV_low_latency2` (registry extension 506) depends on Vulkan 1.2 or
  `VK_KHR_timeline_semaphore`, and on `VK_KHR_present_id` or `VK_KHR_present_id2`; it has no
  feature structure [documented: `vk.xml`, `depends` and `nofeatures`]. The callback adds it
  and `VK_KHR_present_id` when the driver lists both, is Vulkan 1.2 and has the
  `timelineSemaphore` feature, which wgpu-hal turns on whenever the device has it.
  `VK_KHR_present_id` is only enabled, never used: see the markers.
- **The swapchain.** "To allow low latency mode to be used by a swapchain, add a
  `VkSwapchainLatencyCreateInfoNV` structure to the pNext chain of `VkSwapchainCreateInfoKHR`"
  [documented]. wgpu creates the swapchain, and neither it nor Bevy has a hook there. So
  wgpu-hal 30.0.1 is vendored with one change (`engine/vendor/README.md`): on a device that has
  the extension, a new swapchain is created with that structure, `latencyModeEnable` true.
  The same change hands out the newest such swapchain for the calls below
  (`wgpu::hal::vulkan::with_latency_swapchain`), under a lock that the swapchain's replacement
  and destruction also take: the calls come from three threads (main, render, the
  frame-generation presenter), and the specification lets them run beside other uses of the
  swapchain but not beside `vkDestroySwapchainKHR`. While a swapchain is being replaced (a
  resize) the calls are skipped.
- **Which swapchain.** The window has one swapchain, whoever presents on it, and it is the one
  every call names. Each resize or reconfiguration makes a new one, which gets the mode again
  before the next sleep.
- **The mode.** `vkSetLatencySleepModeNV` with `lowLatencyMode` true, `lowLatencyBoost` false and
  `minimumIntervalUs` 0, once per swapchain. Boost "allows an application to hint to the GPU to
  increase performance ... at a cost of increased power consumption" [documented]; it is left
  off [engineering choice: the switch promises pacing, not a higher power state; a second
  setting could add it once the mode is measured]. The minimum interval is the driver's frame
  limiter; the frame cap stays `frame_pace`'s, as with Anti-Lag [engineering choice].
- **The sleep.** `vkLatencySleepNV` "returns immediately. Applications should use
  `vkWaitSemaphores` with `pSleepInfo->signalSemaphore` to delay host CPU work ... Applications
  should call this function before input sampling, and exactly once between presents"
  [documented]. It goes where Anti-Lag's input call goes: right after the hand-off and the
  frame cap's wait, before the next frame's input. The semaphore is a timeline semaphore of
  NVForever's own (`VUID-VkLatencySleepInfoNV-signalSemaphore-09361`), its value one higher
  each frame. The wait is given 100 ms [engineering choice: a driver that never signals must
  not stop the game; the line above says when that happened].
- **The markers.** `vkSetLatencyMarkerNV` gives the driver the frame's stages, and "correct
  attribution is required for latency reduction" [documented: "Frame Attribution"]. Each
  carries `presentID`, "the application-defined identifier of the application-rendered frame
  that this marker belongs to ... It is not related to presentIds passed via
  `VkPresentIdKHR::pPresentIds`" [documented]. So nothing is chained to the presents, which
  wgpu issues. The identifier is the frame index plus one, so it is never 0.

  | Marker | Thread | Where |
  |---|---|---|
  | `SIMULATION_START`, `INPUT_SAMPLE` | main | right after the sleep, before winit delivers the frame's input |
  | `SIMULATION_END` | main | just before the frame is handed to the render thread (the hand-off first waits for that thread, which is not simulation) |
  | `RENDERSUBMIT_START` | render | the start of the `Render` schedule |
  | `RENDERSUBMIT_END`, `PRESENT_START` | render | after the render graph has submitted the frame (`RenderGraphSystems::Finish`) |
  | `PRESENT_END` | render | after Bevy's `render_system`, which presents |

  [engineering choice: the closest points Bevy's schedules offer without changing its renderer.
  Between `PRESENT_START` and the present `render_system` still submits one small command
  buffer (a screenshot's or a readback's copy, usually empty).] A frame with no present (a
  minimised window) leaves its `PRESENT_START` unused; the next frame's replaces it ("the most
  recent `PRESENT_START` marker's presentID" is the one a present takes [documented]).
- **Frame generation.** With frame generation the render thread does not present: it hands each
  frame to the presenter's thread (`fg_presenter.rs`), which presents the generated frame and
  then the real one on the same swapchain. The specification has a rule for exactly this:
  "Intermediate layers, such as frame-generation implementations, may defer or replace
  `vkQueuePresentKHR` calls. Such layers should leave the original `PRESENT_START` and
  `PRESENT_END` markers in place and emit `OUT_OF_BAND_PRESENT_START` and
  `OUT_OF_BAND_PRESENT_END` markers surrounding each of their `vkQueuePresentKHR` calls", and a
  present made between those two markers belongs to the frame the marker names, which "allows
  frame generation implementations to map multiple `vkQueuePresentKHR` calls to the same
  presentID" [documented]. So:
  - the render thread keeps `PRESENT_START` and `PRESENT_END` where it would present; the
    hand-over to the presenter falls between them;
  - the presenter wraps each of its presents, generated and real, in the out-of-band pair with
    the rendered frame's identifier, which the render thread sends along with the frame;
  - the sleep stays once per rendered frame, not per present: the driver is told the two
    presents are one frame.

  The same holds with `NVF_FG_PRESENTER=0` (both presents from the render thread): the
  generated frame's present is wrapped in the out-of-band pair and the real frame's is the
  frame's own.
- **Off** makes no driver call each frame. When the switch goes off, the mode is turned off on
  the swapchain once. (Anti-Lag sends OFF every frame; here a path nobody has run on hardware
  yet stays out of the way of players who leave the switch off.)

## Checks

- Unit tests (`anti_lag.rs`), on an app with Bevy's own extraction and render app but no GPU:
  - the structures have Vulkan's layout (sizes and offsets on 64-bit);
  - with pipelined rendering (a real render thread), frame n's input call comes after frame
    n - 1 is extracted and handed over, and before frame n's present call, with the same index;
  - without it, the input call comes at the end of `Last`, then the frame's present;
  - Off sends mode OFF at both calls.
- Unit tests (`nv_low_latency.rs`), with a driver that records what it is asked:
  - a frame sets the mode once, asks for the sleep, waits for it, then marks its start; the
    semaphore's value goes up by one a sleep; a new swapchain gets the mode again;
  - Off from the start asks nothing; turning it off turns the mode off once;
  - with no swapchain nothing is asked, and the next frame tries again; a refused mode is not
    slept on and is asked for again;
  - through the same app as Anti-Lag's tests, pipelined and not: every frame's markers come in
    the specification's order with one identifier, and the sleep's wait comes just before the
    frame's first marker;
  - frame generation's presents are each wrapped in the out-of-band pair, with the rendered
    frame's identifier.
- Smoke `latency-reduction`: `--latency-reduction` turns the switch on, the start says which
  technique the driver offers or that it has neither, and the game runs without validation
  errors.

## Limits

- **Neither technique has run on hardware yet.** Anti-Lag: the machines this was built on have
  NVIDIA GPUs. NVIDIA low latency (2026-11-12): built, with its unit tests, but the game was not
  started with it. Not yet seen on a GPU: that the driver takes the swapchain structure and the
  mode, that it signals the sleep's semaphore, how long it sleeps, and that validation stays
  clean. Whether either lowers latency needs a measurement, switch Off and On (PresentMon, or
  the driver's own overlay).
- **The vendored wgpu-hal** is the price of the swapchain structure: about 50,000 lines copied
  for a change of about 40. It goes when wgpu has a hook for swapchain creation.
- **Submissions with frame generation.** The driver also attributes each `vkQueueSubmit` to a
  frame: without out-of-band queues, the submissions since the last present go to the frame of
  the next `vkQueuePresentKHR` [documented: "Frame Attribution"]. With the presenter on its own
  thread, the next frame's first submissions can come before the last frame's presents and are
  then counted for the wrong frame [hypothesis: the driver's view of a frame's GPU work shifts
  by part of a frame; what that does to its pacing is unmeasured]. `VkLatencySubmissionPresentIdNV`
  on each submit would settle it (drivers with the extension's revision 3), but wgpu issues the
  submits; a queue of its own for the presenter, marked out of band, would too, and wgpu has
  one queue.
- **Pacing points.** Anti-Lag's present call comes before Bevy's `render_system` submits, not
  right before `vkQueuePresentKHR`: the closest point Bevy offers [hypothesis: the driver uses
  it to match frames, not as a timestamp; unmeasured]. The driver's call for the next frame
  comes right after the hand-off only with pipelined rendering, the game's setup. Without it
  the call falls back to the end of `Last`, after the frame cap, so before the frame just
  simulated is extracted and drawn. Whether either driver paces as meant at the hand-off is
  unmeasured [hypothesis].
- **One window.** The vendored wgpu-hal hands out the newest swapchain made on the device; a
  second window's swapchain would take the calls from the first. The game has one window.
