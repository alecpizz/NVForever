# NVIDIA Streamline (DLSS Super Resolution through Vulkan)

An optional renderer feature of NVForever's own, not a claim about the original game. What
NVIDIA Streamline 2.14.16 expects of a host and does on its side, as far as
`engine/nvforever/src/dlss/` relies on it. Design, decisions and the spike's results:
[dlss-upscaling](../design/dlss-upscaling.md).

Sources: the public repository NVIDIA-RTX/Streamline at tag v2.14.16 (MIT). `include/…` and the
guides (`docs/ProgrammingGuide.md`, `docs/ProgrammingGuideDLSS.md`) are cited by their path;
`src/<file>` means the source file of that name in the repository's `source/` tree (the copy read
kept file names only; each name cited is unique there): `src/sl.cpp` and `src/internal.h` are in
`source/core/sl.api/`, `src/wrapper.cpp` in `source/core/sl.interposer/vulkan/`, `src/hook.cpp`
and `src/exports.def` in `source/core/sl.interposer/`, `src/log.cpp` in `source/core/sl.log/`,
`src/ota.cpp` in `source/core/sl.ota/`, `src/pluginManager.cpp` in
`source/core/sl.plugin-manager/`, `src/vulkan.cpp` in `source/platforms/sl.chi/`,
`src/commonEntry.cpp` and `src/commonInterface.cpp` in `source/plugins/sl.common/`, and
`src/dlssEntry.cpp` in `source/plugins/sl.dlss/`, `src/secureLoadLibrary.cpp` in
`source/core/sl.security/`; `premake.lua` is at the repository's root (the tag's file list, read
2026-11-12). Line numbers are those files' at the tag. Everything is described in our
words; no NVIDIA text or code is reproduced here beyond names.

## Files

- [documented] A Vulkan title with DLSS Super Resolution needs `sl.interposer.dll`,
  `sl.common.dll`, `sl.dlss.dll` and NVIDIA's `nvngx_dlss.dll` (`ProgrammingGuide.md:1440-1445`).
- [documented] The Streamline DLLs are published only inside the v2.14.16 release archive
  (`streamline-sdk-v2.14.16.zip`), whose `bin/x64` also holds `nvngx_dlss.dll` 310.9.16; the
  same DLL is a plain file in NVIDIA/DLSS `lib/Windows_x86_64/rel/` (both checked 2026-11-12).
- [documented] Streamline looks for plugins in the absolute folders the host lists, the first
  with the highest priority (`include/sl_core_types.h:565-568`).
- [documented] JSON configuration files are read only by non-production builds
  (`src/hook.cpp:43`; `Debugging-JSON-Configs.md`); NVIDIA's signed release DLLs are production
  builds.

## Signature check

- [documented] Every module of the SDK's `bin/x64` carries two signatures: a standard Windows
  one, checked with `WinVerifyTrust`, and NVIDIA's own. The host should check
  `sl.interposer.dll` both ways, with the full path, before loading it; the interposer itself
  checks the plugins it loads (`ProgrammingGuide.md:96-104`).
- [documented] Streamline's header helper does this: it verifies the primary signature with
  `WinVerifyTrust`, then compares the public key of the nested (secondary) signature's signer
  with a key it carries (`include/sl_security.h:134-459`, the key at `:274-301`). It does not
  show that the nested signature signs the file. NVForever's translation:
  `crates/nvf-dlss-files/src/security.rs`, whose comment holds the open point.

## Loading and initialisation

- [documented] On Vulkan, the host loads `sl.interposer.dll` in place of `vulkan-1.dll` and takes
  `vkGetInstanceProcAddr` from it (`ProgrammingGuide.md:32`). The interposer forwards what it
  does not intercept to the system's Vulkan loader (`src/wrapper.cpp:2365-2403`).
- [documented] `slInit` loads the requested plugins and must come before any Vulkan call through
  the interposer; features are loaded but not initialised until the device exists, so set, get
  and evaluate calls cannot be used right after it (`ProgrammingGuide.md:115-119`).
- [documented] The preferences are a versioned structure whose C++ constructor sets defaults; the
  default flags are "disable command-list state tracking", "allow over-the-air updates" and
  "load downloaded plugins" (`include/sl_core_types.h:580`). Streamline copies what the
  preference pointers point at during `slInit` (`src/pluginManager.cpp:92-164`).
- [documented] The log callback receives a type and a NUL-terminated message
  (`include/sl_core_types.h:506`), called on Streamline's own log thread with the message valid
  only during the call (`src/log.cpp:107-112, 386-391`).
- [documented] With no application id, Streamline uses a temporary one of its own
  (`src/pluginManager.cpp:160`, `src/internal.h:68`). With an engine version and a project id
  both given, it starts NGX with the project id; without them and without an application id,
  NGX stays off (`src/commonEntry.cpp:1494-1535`).
- [documented] The interposer raises the instance's requested API version to at least Vulkan
  1.3 (`src/wrapper.cpp:129-132`) and, at device creation, adds features (timeline semaphores,
  descriptor indexing, buffer device address) and device extensions (timeline semaphore, push
  descriptor) of its own besides those its plugins ask for (`src/wrapper.cpp:281-294`). A device
  lacking an extension it asks for is refused (`src/wrapper.cpp:816-829`), which wgpu-hal
  30.0.1 turns into a panic (wgpu-hal `src/vulkan/adapter.rs:3044-3050`). The 1.2 and 1.3
  features it asks for are not refused but merged with the host's and masked to what the
  device supports (`src/wrapper.cpp:767-789`; `include/sl_helpers_vk.h:124-216`), so a missing
  feature reaches the device unasked rather than failing its creation.
- [documented] After `slInit`, `slGetFeatureRequirements` tells a feature's requirements without
  a device; before `slInit` it always fails (`include/sl_core_api.h:187-197`). Its
  `FeatureRequirements` (version 2, `include/sl_core_types.h:642-692`) holds, in order: flags
  (among them "Vulkan supported", bit 2, `:625-630`), the CPU threads the feature makes, the
  viewports it supports, the buffer tags it requires (a count and a pointer), the OS and driver
  versions detected and required, then for Vulkan the compute and graphics queues required,
  the device and instance extensions (each a count and an array of names), the 1.2 and 1.3
  feature names (likewise), and the optical-flow queues required. Streamline asserts that last
  member's offset, 176 (`src/sl.cpp:924`). The pointers point into Streamline's own memory
  (`src/sl.cpp:916-1096`).

## Support and functions

- [documented] `slIsFeatureSupported` checks the adapter only when the adapter information
  carries a LUID; without one it answers yes for any adapter (`src/sl.cpp:855-860`).
- [documented] DLSS's own functions (`slDLSSGetOptimalSettings`, `slDLSSSetOptions` and others)
  are fetched by name with `slGetFeatureFunction` (`include/sl_dlss.h:151, 173, 177-192`),
  which answers "not initialised" until the device exists (`src/sl.cpp:1099-1118`).
- [documented] The structures share a header of a 16-byte type GUID and a version, chained by a
  `next` pointer, and Streamline finds a structure by all of the GUID
  (`include/sl_struct.h:32-41, 108-115`). Zeroed memory does not reproduce the constructors'
  defaults, so NVForever's `ffi.rs` writes each default out and tests the layouts.

## Modes and sizes

- [documented] DLSS's modes are a 32-bit enumeration, in order from 0: off, max performance,
  balanced, max quality, ultra performance, ultra quality, DLAA (`include/sl_dlss.h:34-44`), so
  max quality is 3 and DLAA 6. NVForever's mapping from its qualities: the design note, "The
  quality mapping".
- [documented] `slDLSSGetOptimalSettings` takes DLSS's options (the mode and output size among
  them) and fills a `DLSSOptimalSettings` (`include/sl_dlss.h:151, 177-180`): the render size to
  draw at, a suggested sharpness, and minimum and maximum render sizes, all zero by default
  (version 1, `include/sl_dlss.h:108-128`). The range is given when dynamic resolution is
  supported (`ProgrammingGuideDLSS.md:103-126`). It is fetched by name like DLSS's other
  functions (below, "Support and functions").
- [documented] The jitter's phase count: NVIDIA's DLSS Programming Guide (31 March 2026, §3.7.1.1
  "Pattern Phases", p. 22,
  https://raw.githubusercontent.com/NVIDIA/DLSS/main/doc/DLSS_Programming_Guide_Release.pdf)
  gives a base of 8 phases times the square of the display-to-render ratio, with a minimum per
  mode in its table: 8 for DLAA, 18 for Quality, 24 for Balanced, 32 for Performance and 72
  for Ultra Performance. NVForever rounds the product up and takes at least the minimum
  (`engine/nvforever/src/dlss/sizing.rs`).
- [documented] The texture mip bias: the same guide (§3.5 "Mip-Map Bias", p. 14) gives the
  native bias plus log2 of the render width over the display width, minus one, plus a small
  tuning term. NVForever uses a native bias and tuning term of 0 (`dlss/sizing.rs`).
- [documented] Jitter offsets are given in pixels (`ProgrammingGuideDLSS.md:340`).

## DLSS presets

- [documented] `DLSSPreset` is a 32-bit enumeration (`include/sl_dlss.h:46-67`): `eDefault` 0
  ("may or may not change after an OTA"); presets A to D removed (the header says to use J or
  K); E 5 and F 6 deprecated; G 7, H 8, I 9, N 14 and O 15 "revert to default, not recommended";
  J 10, K 11, L 12, M 13. The header's notes on those four: J is similar to K, with slightly less
  ghosting at the cost of extra flickering, and K is generally recommended over it; K is the
  default for DLAA, Balanced and Quality, transformer based, the best image quality at a higher
  performance cost; L is the default for Ultra Performance, a sharper, more stable image with
  less ghosting than J and K, more expensive; M is the default for Performance, similar to L in
  quality, closer to J and K in speed (`:60-63`).
- [documented] Streamline 2.10.0 added L and M as "two additional DLSS Super Resolution
  Transformer Models" (`changelog.txt:243-248`).
- [documented] `DLSSOptions` holds one preset per mode: DLAA, quality, balanced, performance,
  ultra performance and ultra quality, each `eDefault` unless set (`include/sl_dlss.h:90-96`).
  Setting the options hands the first five to NGX as render-preset hints
  (`src/dlssEntry.cpp:288-292`); ultra quality's is not handed on. A change of any of those five
  recreates the context like a mode change (below, "Viewports and resize").
- [documented] For the optimal settings Streamline sets only the output size, the mode and the
  RTX flag before asking NGX (`src/dlssEntry.cpp:823-827`). [hypothesis] The optimal sizes do
  not depend on the preset.
- [observed: the main session's runs at 1280x720 with DLSS 310.9.16, 2026-11-12, every preset
  left `eDefault`] NGX's log said it used the title default preset K for DLAA, Quality and
  Balanced, M for Performance and L for Ultra Performance, as the header's notes say.
- NVForever offers Default, K, L, M and J (design note, "The DLSS model").

## Evaluate

- [documented] DLSS needs depth, motion vectors, the render-size colour and the output colour
  tagged (`ProgrammingGuideDLSS.md:130`), all valid until the evaluate
  (`src/dlssEntry.cpp:200-201`). Tags passed with the evaluate itself are neither copied nor kept
  (`src/commonEntry.cpp:248, 319`).
- [documented] The render size DLSS uses is the colour input's extent
  (`src/dlssEntry.cpp:567-576`). A depth image needs its depth aspect given, else NGX gets the
  colour aspect (`src/dlssEntry.cpp:151-158`). On Vulkan a resource also carries its size,
  format, mips and layers, as Streamline cannot query an image (`src/vulkan.cpp:2779-2844`).
- [documented] The motion-vector scale is multiplied by the render size before it reaches NGX
  (`src/dlssEntry.cpp:684-685`), so vectors in UV units take a scale of about one, not the render
  size. DLSS's evaluate does not read the near and far planes (`src/dlssEntry.cpp:683-736`).
- [documented] Each frame takes a token from `slGetNewFrameToken`; Streamline keeps a small ring
  of frames in flight (`src/sl.cpp:1120-1146`). The viewport id is any value the host picks
  (`ProgrammingGuide.md:646-656`).
- [documented] Streamline moves the tagged images to the layouts NGX needs and back to the state
  each resource reported, inside a scope around the NGX call (`src/dlssEntry.cpp:666-681`,
  `:132-139`; `ProgrammingGuide.md:1026-1027`). It restores no pipeline or descriptor state on
  Vulkan (`src/commonInterface.cpp:442-453, 542-547`).
- [documented] DLSS's evaluate drops NGX's result: the call returns success to the host even
  when NGX's evaluate failed (`src/dlssEntry.cpp:738`), while the common plugin logs an NGX
  evaluate failure as an error (`src/commonEntry.cpp:772-794`). A success code alone does not
  show that DLSS wrote the output.

## Viewports and resize

- [documented] A change of mode, output size or preset recreates the DLSS context and resets the
  history; the old context is released three frames later, not when the GPU is done with it
  (`src/dlssEntry.cpp:346-376`).
- [documented] `slFreeResources` frees a viewport's resources; a command list with a pending
  evaluate must be submitted first (`include/sl_core_api.h:228-239`). DLSS's version releases
  the viewport's context at once and forgets the viewport (`src/dlssEntry.cpp:884-902`), so the
  next options make a fresh context and the three-frame delayed release above, which runs only
  for a viewport that still holds a context (`:363-373`), has nothing to release.
- [documented] The tagging API assumes one swap chain (`ProgrammingGuide.md:1416-1418`).

## Shutdown

- [documented] `slShutdown`'s cleanup uses the Vulkan device Streamline stored, so it must run
  while that device is alive (`src/vulkan.cpp:1349-1407`).

## DLSS Frame Generation

What Streamline 2.14.16 documents of DLSS Frame Generation (DLSS-G), Reflex and PCL as
`engine/nvforever/src/dlss/` relies on it (#2317; design:
[dlss-frame-generation](../design/dlss-frame-generation.md)). Sources beside the above:
`include/sl_dlss_g.h`, `include/sl_reflex.h`, `include/sl_pcl.h`, `docs/ProgrammingGuideDLSS_G.md`
(DG), `docs/ProgrammingGuideReflex.md` (RG), `docs/ProgrammingGuidePCL.md`, the Reflex plugin's
`reflexEntry.cpp` and the Vulkan low latency backend `vknvll2.cpp`. The DLSS-G plugin itself is
shipped as a binary only (README), so its inner workings are known only from the guide.

- [documented] Files: `sl.dlss_g.dll` (prebuilt only), NVIDIA's model `nvngx_dlssg.dll`,
  `sl.reflex.dll` (DLSS-G requires Streamline's Reflex: DG §8.0) and `sl.pcl.dll` (PCL is loaded
  with any feature, `src/pluginManager.cpp:151-154`).
- [documented] Features asked for in `slInit`: `kFeatureDLSS_G` (1000) and `kFeatureReflex` (3)
  (`sl_core_types.h:233-244`); a feature not asked for there cannot be loaded later, one asked for
  may be unloaded and loaded again with `slSetFeatureLoaded` (ProgrammingGuide §2.14), which
  wants no Vulkan call in parallel (`sl_core_api.h:120-134`).
- [documented] Requirements (`slGetFeatureRequirements`): DLSS-G's say vsync off required and GPU
  scheduling required when the plugin's configuration does (`src/sl.cpp:948-955`); a device that
  lacks a loaded plugin's required extension is refused at `vkCreateDevice`
  (`src/wrapper.cpp:816-829`); extra queues are added only where the family has room (`:906-959`).
- [observed, RTX 5070 Ti, driver 610.88, fork-notes `fg-spike-results.md`] DLSS-G asked for 1
  graphics, 2 compute and 1 optical flow queue, vsync off required yes, GPU scheduling required
  yes, OS 10.0.19041 and driver 527.64 at least; `numFramesToGenerateMax` 5, `minWidthOrHeight`
  100.
- [documented] While DLSS-G is loaded the host renders off-screen and presents through
  Streamline's proxy swapchain, even with its mode off (DG §18.0); the swapchain must be torn down
  and made again when DLSS-G is switched on or off (DG §18.0), and the mode must be off before a
  resize, full-screen change or other window change (DG §12.0, §17).
- [documented] `slDLSSGSetOptions` takes effect at the next present and is deterministic only on
  the presenting thread (DG §6.0); `slDLSSGGetState`'s `numFramesActuallyPresented` is the
  frames presented for the last real frame when called after each present (DG §13); a status
  other than `eOk` should turn it off (DG §14.3). Both functions are not thread safe and are
  fetched with `slGetFeatureFunction(kFeatureDLSS_G, …)` (`sl_dlss_g.h:187-222`).
- [documented] Inputs (DG §5.1): depth and motion as for DLSS SR, a HUD-less colour the size of
  the back buffer, and a UI buffer, preferably UI alpha (`kBufferTypeUIAlpha` 69), with
  `Final = UI.RGB + (1 - UI.A) * Hudless`; tags `eValidUntilPresent` with `slSetTagForFrame`,
  which needs `eUseFrameBasedResourceTagging` (`src/sl.cpp` `slSetTagForFrame`); a tag set null
  before its resource goes (ProgrammingGuide:1017-1025). User interface recomposition with the
  HUD-less and a UI buffer (DG §6.6).
- [documented] Frame identity: the present markers' frame number must match the constants' (DG
  §8.0; "common constants cannot be found for frame N" otherwise). `slGetNewFrameToken` with an
  index reuses the newest slot only for the same index, else moves on in a ring of six
  (`src/sl.cpp:1120-1146`, `sl_consts.h:95`).
- [documented] Reflex: `slReflexSleep` and `slPCLSetMarker` every frame even with Reflex off;
  `slReflexSetOptions` at least once (RG §4.0); the Vulkan backend makes the
  `VK_NV_low_latency2` calls on the swapchain it tracks and waits inside the sleep
  (`vknvll2.cpp`), and reports its timings in microseconds (`vknvll2.cpp:274-288`).
- [documented] API errors of the asynchronous present and acquire come through
  `DLSSGOptions::onErrorCallback`, which must not stall (DG §6.7).
- [observed] Frames are generated only while the game's window is in front: with other windows
  in front the ratio fell to 1.00 with the mode on and status `eOk` (fork-notes
  `fg-spike-results.md`).

### Queue parallelism

Sources: Streamline at tag v2.14.16 (`include/sl_dlss_g.h`, and `docs/ProgrammingGuideDLSS_G.md`,
whose header says "Version 2.14.16"), and NVIDIA's sample NVIDIA-RTX/Streamline_Sample at tag
v2.14.16, commit 94da85b6594e6c5109e84f995f5969811014c71f (MIT), read 2026-11-24.

- [documented] `DLSSGOptions::queueParallelismMode` (added with `kStructVersion3`) sets "the level
  of client and DLSSG queue parallelism to use for performance gain - must be same for all
  viewports" (`sl_dlss_g.h:110-112`). Its type `DLSSGQueueParallelismMode` is an
  `enum class : uint32_t` with no explicit values (`:55-66`): `eBlockPresentingClientQueue` (0,
  the default), in which the "client's presenting queue is blocked until DLSSG workload execution
  completes" (`:57`), and `eBlockNoClientQueues` (1), "only supported on Vulkan presently" (a D3D
  client gets the default), which "helps achieve maximum performance benefit from queue-level
  paralleism in Vulkan during DLSS-G processing" (`:59-64`).
- [documented] In `eBlockNoClientQueues` the client must wait on
  `DLSSGState::inputsProcessingCompletionFence` and its value "before it can modify or destroy the
  tagged resources input to DLSS-G enabled for the corresponding previously presented frame on any
  client queue" (`:61-63`). `DLSSGState` holds the fence (`void* inputsProcessingCompletionFence`)
  and `uint64_t lastPresentInputsProcessingCompletionFenceValue`; with this mode the wait is
  "always required for VK" (DG §15.1, `ProgrammingGuideDLSS_G.md:894-900`), and the client calls `slDLSSGGetState` "on
  the present thread to retrieve the fence value for the inputs consumed by FG, on which client
  would wait in the frame it would modify those inputs" (`sl_dlss_g.h:170-177`). Neither the
  header nor the guide names the Vulkan object behind the `void*`.
- [documented] DG §17.0 (`ProgrammingGuideDLSS_G.md:963-967`): the mode "may offer extra performance gains depending on the
  workload"; "Gains are most apparent in GPU-limited applications that use multiple queues for
  submissions", and highly CPU-limited applications may see smaller gains.
- [documented] NVIDIA's sample waits on the fence as a timeline `VkSemaphore` with that value on
  its graphics queue: `SLWrapper::QueueGPUWaitOnSyncObjectSet` calls
  `queueWaitForSemaphore(nvrhi::CommandQueue::Graphics, reinterpret_cast<VkSemaphore>(syncObj),
  syncObjVal)` on Vulkan (`src/SLWrapper.cpp:250-279`, the call at `:276`). `RenderScene` queues it
  when DLSS-G was on the last frame, "before any of the inputs are modified in the subsequent
  command list submission", if the value is 0 or above the last one waited on
  (`src/StreamlineSample.cpp:828-836`). Its comment calls the wait "redundant until SL DLSS FG
  allows SMSCG but done for now for demonstration purposes" (`:834`).
- [observed, RTX 5090, driver 621.24, 2026-11-23] At 3840x2160, DLSS Quality, 4x, no frame limit,
  Display sync Off: 96.5 real / 385 presented frames a second with `eBlockNoClientQueues` against
  91.0 / 364 with the default (+6%), latency within noise, no present errors (design:
  [dlss-frame-generation](../design/dlss-frame-generation.md) "Queue parallelism"). [hypothesis]
  NVForever submits on one queue, so the gain comes from the next frame's work running beside
  DLSS-G's own queues rather than from other queues of the game's.

## NVIDIA's updater

- [documented] In production builds, `slInit` reads the cached server manifest and asks for an
  update check on every call; the "allow over-the-air updates" flag only decides whether optional
  updates are requested, and no flag skips the check (`src/pluginManager.cpp:961-969`).
- [documented] The check starts the driver's `nvngx_update.exe` on a detached thread and does not
  wait for it (`src/ota.cpp:240-246`), only on a machine with an NVIDIA GPU
  (`src/ota.cpp:849-857`).
- [documented] When a file named `nvngx_update.exe` sits beside `sl.interposer.dll`, that file is
  started in place of the driver's, with no signature check (`src/ota.cpp:194-247`, the folder
  test at `:214-221`).
- [documented] With "load downloaded plugins" cleared, no downloaded plugin is loaded, except
  when the driver's DLSS override setting is on for the program or globally, which loads a
  complete downloaded override set in place of the folder's plugins
  (`src/pluginManager.cpp:994-1049`).
- [observed] In the spike's runs (2026-11-12), NGX's own log lines show it looking for DLSS in
  NVIDIA's model cache (`C:\ProgramData\NVIDIA\NGX\models`) before loading the player's
  `nvngx_dlss.dll`, which it then used. [hypothesis] A DLSS in that cache for NVForever's ids
  would be used instead of the player's file; NVIDIA does not document the choice.

Sources, licences and NVForever's translation of the headers: `engine/nvforever/src/dlss/`
(ffi.rs, security.rs); the vendored change for the Vulkan library: `engine/vendor/README.md`.
