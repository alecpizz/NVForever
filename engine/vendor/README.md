# Patched dependencies

Crates copied from crates.io with a small change, used through `[patch.crates-io]` in
`engine/Cargo.toml`. Each keeps its own licence files. When the crate is upgraded, copy
the new version here and apply the change again (or drop the patch if upstream fixed it).

## gpu-allocator 0.28.0 (MIT OR Apache-2.0, Traverse-Research/gpu-allocator)

Used by wgpu for Vulkan memory. Change, in `src/vulkan/mod.rs` `MemoryType::free`: an
emptied general memory block is kept for reuse while fewer than
`KEEP_EMPTY_GENERAL_BLOCKS` (2) other empty general blocks of that memory type exist;
upstream destroys every emptied block except the last.

Still needed with Bevy 0.20.1 (#934): its wgpu-hal 30 uses gpu-allocator 0.28, and 0.28.0
is still the latest release, with the same free policy.

Why (#897): late in long sessions the block wgpu keeps fills with long-lived data, so each
frame's short-lived staging memory spills into a new block, which empties when the frame
is done and was destroyed at once. An Nsight Systems capture of the owner's game showed
about 285 blocks of 128 MiB made and freed a second, each zeroed and paged in by Windows,
and the render thread stalling 100-800 ms in the driver meanwhile.

Second change (#1429), in `src/vulkan/mod.rs` `MemoryBlock::new` and `src/lib.rs`: a
`vkAllocateMemory` that takes 1 ms or more logs `gpu memory: vkAllocateMemory of <n> MiB
(memory type <t>, host-visible|device-local, a general block|dedicated) took <ms> ms`, so a
single slow frame charged to a render system that only writes a buffer
(`upload_light_probes`, `write_binned_instance_buffers`) can be matched to a new memory
block. `std` is linked for the clock (wgpu-hal turns the crate's `std` feature off).

## bevy_pbr 0.20.1 (MIT OR Apache-2.0, Bevy contributors)

Bevy's 3D passes, for FSR upscaling (docs/design/fsr-upscaling.md), which draws the main
passes at a lower resolution through `MainPassResolutionOverride`. Two changes, each marked
by a comment starting `NVForever FSR integration`:

- `src/transmission/node.rs`: the stepped transmission pass (refraction, water) takes the
  override's size, as the opaque and transparent passes do; upstream it takes the whole
  target's.
- `src/render/clustered_forward.wesl`: a fragment's position is mapped through the
  main-pass viewport before it picks a light cluster, as the clusters are laid out for the
  display size, so point and spot lights stay where they belong below 1:1.

Without the override both do what upstream does. This is a new patch for Bevy 0.20.1, not
the dropped 0.19.1 one below.

Another change (one resolve, docs/design/frame-rate-audit.md §9), in `src/transmission/`,
each part marked `NVForever one resolve`, with `ViewTarget::set_resolve_deferred` in
bevy_render below:

- `node.rs`: `begin_deferred_resolve` (before the opaque pass) and `end_deferred_resolve`
  (after the transmissive pass, before the transparent pass) hold back the multisample
  resolve of a view whose transparent pass is going to run and which has no main-pass
  resolution override (the game's FSR passes read the resolved picture in between).
  `main_transmissive_pass_3d` then resolves the samples straight into the transmission
  texture with a pass that draws nothing, instead of copying the resolved picture.
- `texture.rs`: the transmission texture is also a render attachment, to be resolved into.
- `mod.rs`: the two systems, ordered around the opaque and transmissive passes.

Upstream resolves the whole target at the end of the opaque, transmissive and transparent
passes and copies it once: three resolves and a copy where one resolve to the transmission
texture and one to the main texture are read. Without multisampling, with a resolution
override, or when the transparent phase is empty, upstream's path runs unchanged.

Both FSR changes are already in Bevy's `main`, written almost the same way: bevyengine/bevy#26235
(transmission, merged 2026-10-26) and bevyengine/bevy#26236 (cluster lookup, merged
2026-10-20). Neither is in a release yet; 0.20.1 is the latest. Drop those two with the first
Bevy release that has both.

Another change (prepass coverage, docs/design/frame-rate-audit.md §9), in
`src/render/pbr_prepass_functions.wesl`, marked `NVForever prepass coverage`:
`nvf_prepass_sample_color` and its result struct, which hand out the base colour, flags and
alpha cutoff that `prepass_sample_color_and_alpha_discard` samples, without testing them.
nvf-assets' `fade_prepass.wesl` turns that alpha into a sample mask (the soft edge of grass,
leaf cards and tree billboards, written by the depth prepass). Upstream's function is
unchanged, so nothing else behaves differently. Upstream has no way to get the sampled alpha
out of the prepass's own code; drop this when it has, or when the sampling is written in
nvf-assets.

Third change (#1803), in `src/material.rs`, each part marked by a comment starting
`NVForever material queue probe`: `MaterialQueueProbe`, an optional resource
that records watched main-pass entries and their pipeline IDs as `queue_material_meshes`
queues them, for `nvforever/src/pipboy_ready.rs`; Bevy installs none, so without it the
pass does what upstream does. Drop it when an upstream observation API replaces its
caller or that readiness check is retired. Retries for meshes not yet uploaded and a
per-view visibility filter of pending entries were tried with it and dropped: the
`pipboy-first-open` scenarios passed with a clean first opening without them
(2026-11-07).

Another change (held shadow maps, docs/design/frame-rate-audit.md §9 and §10; the game's
Sun shadows' Performance mode and staggered cascades, nvforever shadow_rate.rs), marked
`NVForever held shadow maps`:

- `src/render/light.rs`: the render-world resource `HoldDirectionalShadowMaps`, a bit per
  cascade index (#2063: it was one switch for all of them). `per_view_shadow_pass` leaves
  out the directional light views whose cascade's bit is set, so their shadow maps keep the
  picture of the last frame that drew them. Point and spot lights' maps are drawn as ever.
- `src/render/gpu_preprocess.rs`: `gather_shadow_cascades_for_view` leaves out the same
  views, so `early_gpu_preprocess`, `allocate_uniforms` and `unpack_bins` prepare nothing
  for those cascades that frame.
- `src/render/light.rs` `prepare_lights`: when the directional shadow texture the cache hands
  out is not the one of the frame before (a shadowed spot light such as the flashlight
  switched on or off changes its layer count, or the old one was dropped), the render
  world's hold is cleared for that frame, so every cascade is drawn into the new texture
  instead of a held one sampling an empty or stale layer (#2182 review). It logs
  `held shadow maps: a new directional shadow texture`.

Bevy inserts no such resource, and without it (or with no bits set) nothing changes.
Whoever sets it must hand the lights the held cascades as the frame that drew them had
them, as the game does.
Upstream has no way to draw a light's shadow map less often than every frame.

## bevy_render 0.20.1 (MIT OR Apache-2.0, Bevy contributors)

Early finishing of command encoders (#1427, docs/design/frame-rate-audit.md §5). First
change, in `src/renderer/render_context.rs`, each part marked by a comment starting
`NVForever early finish`: when a render system is done (`RenderContextState`'s
`SystemBuffer::queue`), each encoder it recorded is handed to the compute threads to be
finished (`CommandEncoder::finish`) while the render thread records the systems after it.
Upstream finishes them all at once inside `submit_pending_command_buffers`, with the render
thread waiting. At submission an encoder no thread has started is finished there, and one
being finished is waited for; the command buffers are submitted in the same order as
upstream, so what the GPU does is unchanged. wasm keeps upstream's path (its encoders are
not `Send`).

[observed 2026-11-07, `campaign.py --only goodsprings-noon novac-noon battle-50`, the
`profile` build] Alone it took 0.2-0.5 ms a frame off finishing the encoders' wall time
(Goodsprings 0.88 → 0.58 ms, battle-50 2.3 → 1.8 ms). The blended pass is recorded last
among the big ones and finishes in 0.8-1.8 ms, so it is what is still waited for.

Another change (one resolve, docs/design/frame-rate-audit.md §9), in `src/view/mod.rs`,
marked `NVForever one resolve`: `ViewTarget::set_resolve_deferred` and
`ViewTarget::resolve_deferred`. While set, `get_color_attachment` hands out the multisampled
texture without its resolve target, so a pass keeps its samples and resolves nothing. A new
`ViewTarget` (each frame) starts with it off, and nothing in Bevy sets it: bevy_pbr's
transmission systems above do.

Second change (#1887, docs/design/frame-rate-audit.md, the indirect-parameter build jobs),
in `src/batching/gpu_preprocessing.rs`, `coalesce_batch_ranges` (comment starting
`NVForever coalesced build jobs`): `prepare_indirect_parameters_build_jobs` merges each
view's batch ranges that follow one another into one build job. Upstream makes one job,
and so one compute dispatch, per range; wgpu puts a barrier between dispatches that write
the same storage buffers, and on the GPU each one waits for the last to drain. The shader
handles each batch index on its own, so the merged dispatch does the same work. Drop it if
upstream merges the jobs.

[observed 2026-11-08, Nsight Graphics GPU Trace, Goodsprings 12:00, 2560x1440, 4x MSAA,
no actors] Compute dispatches a frame 2,518 → 904, compute-active time 16 → 9 % of the
frame; interleaved runs of one build with and without it: 9.0 → 8.5 ms a frame.

Another change (#2064, docs/design/frame-rate-audit.md §10), marked `NVForever unchanged
uploads`: `RawBufferVec::write_buffer_if_changed` (`src/render_resource/buffer_vec.rs`)
queues nothing when the same bytes were last written into the same GPU buffer, and
`batching/gpu_preprocessing.rs` uses it for each multi-drawn batch set's three buffers that
only the CPU writes (`render_binned_mesh_instance_buffer`, `bin_metadata_buffer`,
`bin_index_to_bin_metadata_index_buffer`, read-only in `allocate_uniforms.wesl` and
`unpack_bins.wesl`). Upstream writes all three every frame: about 300 of a Goodsprings
frame's 400 `queue.write_buffer` calls, each a copy with its own barrier before the frame's
first pass. Test aid `NVF_PLAY_TEST_SAME_WRITES=1` writes every frame again.

A measuring aid for the encoder spans (`NVF_PLAY_TEST_PROFILE_ENCODERS=1`, `enc:<system>`, #2060),
in `src/diagnostic/internal.rs` (marked `NVForever encoder gaps`), adds
`zz:wall` (the frame's first encoder to its last), `zz:gaps` (time inside that no encoder
covers, with the four largest gaps by name) and `zz:between_frames` (the previous frame's
last encoder to this frame's first) to the `profile gpu:` line.

Third change (#1868), in `src/diagnostic/mod.rs`, each part marked by a comment starting
`NVForever recorder gate`: `begin_diagnostics_frame`, `resolve_encoder` and
`finish_diagnostics_frame` take the `DiagnosticsRecorder` as optional and do nothing
without it. Upstream they require it, so taking it out of the render world stops the app
(a missing resource fails the systems' parameter check). Every pass already records only
while the recorder is there (`RenderContext::diagnostic_recorder` is optional; the camera
driver checks `contains_resource`), so without it no timestamp is written, resolved or read
back. `nvforever/src/perf_overlay/split.rs` takes it out while the performance overlay's
frame split, the only reader that added the render diagnostics, is not shown. With the
recorder there (every run that does not take it out) the systems do what upstream does.

`NVForever recorded instant` (`diagnostic/internal.rs`, `diagnostic/mod.rs`): each frame's
`FrameData` notes when it began recording, `RenderDiagnostics` carries that instant, and
`sync_diagnostics` keeps the last pair (the import time its measurements carry in the store,
the frame's recording time) in a new main-world resource, `RenderDiagnosticsRecorded`. The
store's measurements keep upstream's import time (frame telemetry exports it as such,
docs/design/frame-telemetry.md). `nvforever/src/perf_overlay/split.rs` uses the pairs to
read only the measurements of frames recorded in its window and since its row began to show.

Another change (a measuring aid, docs/design/frame-rate-audit.md §9), in
`src/renderer/render_context.rs`, marked `NVForever encoder spans`: with
`NVF_PLAY_TEST_PROFILE_ENCODERS=1` and a diagnostics recorder (the `profile` build's
`NVF_PLAY_TEST_PROFILE`), every render system's command encoder is wrapped in a GPU time
span `enc:<system>`, opened when the encoder is made and closed when the system's
`RenderContext` is dropped. The passes' own spans nest inside it, so the difference is the
GPU time of what a system records outside its passes: image copies, multisample resolves,
passes without a span. Without the variable nothing is recorded.

Fourth change (#1778, docs/design/latency-reduction.md): Bevy's `raw_vulkan_init` feature,
which the engine turns on so `anti_lag.rs` can add a device extension, no longer changes which
backend starts or which validation wgpu runs. Each part is marked by a comment starting
`NVForever (2026-11-09)`:

- `src/renderer/mod.rs` `initialize_renderer`: the raw Vulkan instance is created only when
  the backends include Vulkan, otherwise `Instance::new`, as without the feature. Upstream
  creates it whatever the backends say, so `WGPU_BACKEND=dx12` or `gl` was ignored.
- `src/settings.rs` `WgpuSettings::default`: upstream, the feature also removes
  `InstanceFlags::VALIDATION_INDIRECT_CALL` in every release build, on every backend. Left
  out: the flag goes only when the backends don't include DX12, as without the feature. On
  DX12 the built-ins `num_workgroups`, `vertex_index` and `instance_index` read wrong in
  indirect calls without it [documented: wgpu-types 30.0.1 `instance.rs`], and release builds
  on Vulkan with the default backends keep the validation (and its GPU time) they had before.

Still as upstream: when the backends include Vulkan, the instance is Vulkan-only whenever
Vulkan starts, so a Vulkan instance with no adapter that can draw to the window stops the start
instead of going on to DX12. Left so: the game does not run on DX12 as it is (main's own
`WGPU_BACKEND=dx12` start stops on bind group validation errors [observed 2026-11-09, RTX
4070]), so there is nothing to fall back to. Where the Vulkan instance cannot be made at all
(no driver), upstream's own fallback to `Instance::new` is kept.

Still as upstream, and different from a build without the feature: the instance made from
the raw Vulkan one takes wgpu-core's default instance flags (`Instance::from_hal`), so
`WgpuSettings::instance_flags` and the `WGPU_VALIDATION*`/`WGPU_DEBUG` environment switches
reach the Vulkan layer (wgpu-hal) but not wgpu-core [documented: wgpu-core 30.0.1
`instance.rs` `from_hal_instance`]. The defaults are the same either way. Drop this change
when a Bevy release makes the feature follow the backends.

Window images for another thread (2026-11-08, frame generation's presenter,
docs/design/frame-generation.md "Presentation and pacing", h9x000/NVForever#1). In
`src/view/window/mod.rs`, each part marked by a comment starting `NVForever (2026-11-08)`:

- `DeferredSwapchain`, a component on a window: `prepare_windows` takes no swapchain image
  for it and points the window's view at a stand-in texture of the window's size instead
  (`STAND_INS` = 3, used in turn, so a thread may still read the last two).
- `SharedSurface` (`SurfaceData::shared`): the window's surface and its configuration behind
  a lock, for one other thread to take images from and present (`SurfaceGuard::acquire`,
  `present`). `create_surfaces` reconfigures under the same lock, so never while that thread
  holds an image (wgpu allows one per surface).
- `SWAPCHAIN_WAITS`: the time spent in the swapchain's acquires and presents, for frame
  generation's log line.

Review fixes (2026-11-09, h9x000/NVForever#6), marked `NVForever (2026-11-09)`:

- The thread never configures the surface. `SurfaceGuard::acquire` returns no image on
  `Outdated` and marks the surface (`SurfaceShare::outdated`); `need_surface_configuration`
  and `create_surfaces` then reconfigure it on the render thread, under the lock, whether or
  not the window's size changed (alt-tab, a display mode or HDR change), and clear the mark.
  Before, `acquire` configured the surface itself, and wgpu-core 30's `configure_surface` waits
  for the GPU and fails with `GpuWaitTimeout`, a validation error, when another thread submits
  meanwhile (`wgpu-core-30.0.1/src/device/resource.rs` 5338-5351); the render thread keeps
  submitting.
- `SurfaceData` keeps a clone of the window's `RawHandleWrapper` and `SharedSurface` holds
  one, dropped after its surface, so the window outlives every holder of the surface
  (`create_surface_unsafe`'s contract, which upstream keeps by removing `RawHandleWrapper` and
  `SurfaceData` together).
- Review of LootJunkieAzza/NVForever#1964: `SurfaceGuard::acquire` marks the surface on `Lost`
  as on `Outdated` (a lost surface froze the window with nothing asking for a reconfigure),
  and `DeferredSwapchain::take_stand_in` gives each frame's stand-in once, so a window
  `prepare_windows` passed over (no camera draws to it) is not handed over a second time.

Upstream Bevy has no way for a thread other than the render thread to present a window.

## wgpu-hal 30.0.1 (MIT OR Apache-2.0, the gfx-rs developers)

wgpu's Vulkan backend, for NVIDIA low latency (`VK_NV_low_latency2`,
docs/design/latency-reduction.md, `nvforever/src/nv_low_latency.rs`). One change, in
`src/vulkan/swapchain/native.rs` and `src/vulkan/mod.rs`, each part marked by a comment
starting `NVForever (2026-11-12)`:

- `NativeSurface::create_swapchain`: on a device whose enabled extensions include
  `VK_NV_low_latency2`, the swapchain is created with `VkSwapchainLatencyCreateInfoNV`,
  `latencyModeEnable` true. Low latency mode may be used only on a swapchain created so
  [documented: Vulkan spec, "Latency Reduction", `VkSwapchainLatencyCreateInfoNV`], wgpu
  creates the swapchain, and it has no hook there. wgpu-hal itself never enables the
  extension; only a device-creation callback does (`nv_low_latency.rs`), so without one the
  crate does what upstream does.
- `vulkan::with_latency_swapchain` (new, public): calls a closure with the newest such
  swapchain, under a lock that `create_swapchain` takes to clear the handle before the old
  swapchain is handed over as `oldSwapchain`, and `NativeSwapchain`'s `Drop` before
  `vkDestroySwapchainKHR`. The extension's calls (`vkSetLatencySleepModeNV`,
  `vkLatencySleepNV`, `vkSetLatencyMarkerNV`) take the swapchain and come from threads that
  do not present; upstream's `Surface::raw_native_swapchain` gives the handle but nothing
  keeps it alive afterwards, and reading it waits behind a blocked acquire or present. One
  handle for the process: with two windows the newer swapchain takes the calls.

Not run on a GPU yet (2026-11-12): built, and the engine's tests and clippy pass. The copy
is the crates.io package as published (its `examples` are kept: the manifest names them).
Drop it when wgpu lets a caller extend swapchain creation, or creates the swapchain with
the structure itself.
## wgpu-ffx 0.1.0 (MIT, the wgpu-ffx developers and Advanced Micro Devices)

The FSR 3.1.4 upscaler (docs/design/fsr-upscaling.md) and, added here, FSR 3.1 frame
generation (docs/design/frame-generation.md): a Rust/wgpu port of AMD FidelityFX
SDK 1.1.4's upscaler, [cwfitzgerald/wgpu-ffx](https://github.com/cwfitzgerald/wgpu-ffx) at
commit `6fe7064064b396b30df97341c2ca957ca1b00884`. It is not on crates.io, so it is not a
`[patch]`: the engine uses its `wgpu-ffx` and `wgpu-ffx-shaders-spv` crates as path
dependencies, and `engine/Cargo.toml` excludes the workspace. Its shaders are the SDK's
GLSL (FidelityFX-SDK commit `c6efa6bf7f2027b3ec94f28578bb5965eabb9e55`, with AMD's MIT
notices), compiled to the SPIR-V that `wgpu-ffx-shaders-spv` embeds. `xtask/` and the
shaders it compiles are that SPIR-V's source and stay with it: `shaders/src/fsr3upscaler`,
`shaders/src/frameinterpolation`, `shaders/src/opticalflow` and what they include
(`shaders/include` with those three, `spd` for the pyramids and `fsr1` for RCAS). `LICENSE.MIT` holds both copyright
notices; `tools/release/third_party_licenses.py` takes it from the workspace root for the
release's `THIRD-PARTY-LICENSES.txt`.

Changes:

- wgpu 29 → 30 (the version Bevy 0.20.1 uses); the runtime crate's manifest names this
  workspace (`..`).
- `wgpu-ffx/src/pass.rs`: wgpu 30's entry-point metadata for SPIR-V passthrough modules.
  The accumulation pass's motion inputs were reversed upstream: the low-resolution variants
  now read the converted, dilated motion and the display-resolution variants the input
  motion, as AMD's shader bindings declare. With them reversed, moving things left trails
  at every quality, Native AA included [observed]. A binding test covers both sharpening
  variants.
- `wgpu-ffx/src/lib.rs`: the dispatch no longer pushes and pops a device-wide error scope,
  which raced Bevy's pipeline compilation on other threads.
- `wgpu-ffx/src/lib.rs` (2026-11-08): `FsrView::configure` and `FsrConfigureKey`, the
  upscaler's tuning constants as FidelityFX SDK 1.1.4's `FfxFsr3UpscalerConfigureKey` sets
  them (`ffx_fsr3upscaler.h`: names, descriptions, defaults and ranges). The engine keeps
  AMD's defaults; `tools/fsr-validation` (`ghosting.rs`) uses it to measure them.
- **Bind groups kept** (2026-11-10): `pass.rs` (`BindKey`, in the view's `FsrResources`) and
  `fg_pass.rs` (`BindGroups`, in the optical-flow and frame-interpolation views) keep each
  pass's bind group by what it binds, where upstream and the first port made the bind group
  and its texture views again for every pass of every frame (0.3 and 0.5 ms a frame of the
  render thread at 4K, 0.09 and 0.07 after [observed, docs/design/frame-generation.md]).
  `frameinterpolation.rs`: `FrameInterpolationDispatchInfo::previous_source`, the previous
  interpolation source kept by the caller, which saves AMD's display-sized copy of it.
  **Unused ones dropped** (2026-11-11, #2085): `kept.rs` (new) is the list both use. A kept
  bind group holds clones of the caller's textures, and the lists only started again at
  their caps (128 and 256), which a steady state never reaches: when the caller stopped
  handing a texture in while the view lived on, it stayed alive with the view. In the
  engine, switching frame generation changes the main textures' usage flags, so Bevy makes
  two new display-sized ones while the FSR view stays [observed in the code:
  `prepare_view_targets`, `fsr.rs` and `frame_gen.rs` `prepare`; the memory was not
  measured]. Now each entry remembers the frame it was last used in, and one not used for
  16 frames (calls of the view's dispatch or prepare) is dropped; the caps stay.
- **Half precision** (2026-11-08): the upscaler's `perm.toml` compiles `FFX_HALF = [0, 1]`
  (shaderc 2026.4, Vulkan SDK 1.4.363.0, `--only fsr3upscaler`). The committed FP32 SPIR-V
  is kept byte for byte (the Half::Off arms of `fsr3upscaler/mod.rs` name the same files);
  26 FP16 files are added, for the accumulate pass and the two pyramids. `lib.rs`:
  `FsrContextFlags::ALLOW_FP16` (not one of AMD's flags) and `FP16_FEATURES` (`SHADER_F16`
  and `SHADER_I16`: the FP16 SPIR-V declares `Float16` and, in 8 accumulate variants,
  `Int16`). The pyramids stay full precision: SDK 1.1.4's FFX_HALF SPD callbacks in
  `ffx_fsr3upscaler_luma_pyramid.h` and `ffx_fsr3upscaler_shading_change_pyramid.h` return
  zero and store nothing, and those variants compile to passes that bind 2 resources where
  the FP32 ones bind 7 and 12 [observed 2026-11-08, spirv-dis]. The FP16 accumulate variants
  bind the same sets, bindings and image formats as their FP32 slots [observed 2026-11-08,
  from the SPIR-V's decorations].
- **Frame generation** (2026-11-07, #1778), new code ported from AMD's MIT host sources of
  FidelityFX SDK 1.1.4 (`sdk/src/components/opticalflow/ffx_opticalflow.cpp`,
  `sdk/src/components/frameinterpolation/ffx_frameinterpolation.cpp`, and the parameters
  `sdk/src/components/fsr3/ffx_fsr3.cpp` passes them; tag `v1.1.4` of
  GPUOpen-LibrariesAndSDKs/FidelityFX-SDK, MIT throughout, read as reference and not
  vendored):
  - `wgpu-ffx/src/opticalflow.rs` and `wgpu-ffx/src/frameinterpolation.rs`: the passes,
    their resources, constants, order and dispatch sizes. Left out: the debug view, the async
    (second queue) mode and the distortion-field input (a 1x1 zero field is bound, as AMD's
    default).
  - `wgpu-ffx/src/fg_pass.rs`: the passes bind by GLSL name. AMD's frame-interpolation
    shaders put their sampler at binding 1000 (over wgpu's limit) and number bindings with
    gaps; wgpu-hal's Vulkan backend numbers a layout's descriptors 0, 1, 2... and rewrites
    only shaders it translates, so the SPIR-V's binding decorations are rewritten to that
    dense order when each module is created. The shader sources stay unchanged.
  - Tests compare every pass's binding table with the `#define`s of its `.glsl` file and the
    declarations of the callbacks header, and with the bindings the compiled SPIR-V
    declares; GPU checks against true midpoints are in `tools/fsr-validation` (`fg.rs`).
  - Shaders: `shaders/src/{frameinterpolation,opticalflow}` and
    `shaders/include/{frameinterpolation,opticalflow}` (46 files), each identical to the
    v1.1.4 tag's file but for line endings (compared by git blob hash, 2026-11-07), with
    AMD's copyright and MIT notice. Compiled with `perm.toml` configs in
    `wgpu-ffx-shaders-spv/src/{frameinterpolation,opticalflow}` (base defines from AMD's
    `CMakeCompile*Shaders.txt`). Not permuted: jittered motion vectors, whose define AMD's
    list misnames so its own build never compiles that branch (it does not compile).
- `xtask/src/compile_shaders.rs`: `--only <effect>` compiles one effect's `perm.toml`, so
  the upscaler's committed SPIR-V is left alone (a newer `glslc` gives different bytes for
  unchanged sources). The frame-generation SPIR-V was built with shaderc 2026.4 (Vulkan SDK
  1.4.363.0): `cargo xtask compile-shaders --only frameinterpolation --only opticalflow`.
- Left out: the upstream repository's Git and CI files, and the shader sources of the SDK's
  other effects (blur, Brixelizer, CACAO, CAS, the denoiser, depth of field, FSR 2, SSSR and
  the rest). Nothing here compiles them: `cargo xtask compile-shaders` builds the effects
  with a `perm.toml` in `wgpu-ffx-shaders-spv/src`: the FSR 3.1 upscaler, frame
  interpolation and optical flow. Every `#include` of their shaders resolves among the kept
  files.

The kept shader files are unchanged. To rebuild the SPIR-V, install `glslc` (Vulkan SDK or
Shaderc) and run `cargo xtask compile-shaders` in `engine/vendor/wgpu-ffx` (`--only <effect>`
for one); `cargo xtask vendor` downloads the whole AMD SDK and is not needed. Not used:
AMD's signed DLLs (FSR SDK 2.x), its frame-generation swapchain and DLLs, the SDK's samples
and prebuilt binaries.

## Dropped

- bevy_pbr 0.19.1 (#903, PR #905): the bindless index-table comparator in
  `MaterialBindlessSlab::get_bindless_index_table`, reversed in 0.19.1, which kept freed
  `ExtendedMaterial`s' textures in the slabs. Fixed upstream (bevyengine/bevy#25848);
  Bevy 0.20 has the fix, in `bevy_render::material_bind_groups` (#934).

### bevy_pbr: NVForever screen indirect light (remaster, docs/design/graphics-remaster.md)

Each part marked `NVForever screen indirect light` (or `NVForever remaster`):

- `src/render/mesh_view_bindings.rs`: `ScreenIndirectLight` (an extracted camera component),
  `ScreenIndirectTexture` (the render world's texture) and view binding 39
  (`SCREEN_INDIRECT_BINDING`); a new layout key bit binds the texture, or the zero fallback
  until there is one.
- `src/render/mesh_view_bindings.wesl`: `screen_indirect_texture` under `SCREEN_INDIRECT`.
- `src/render/mesh.rs`: `MeshPipelineKey::SCREEN_INDIRECT` (now `LAST_FLAG`) from the view's
  component, and the `SCREEN_INDIRECT` shader def on meshes that write depth.
- `src/render/shadow_sampling.wesl`: under `SCREEN_INDIRECT` the Gaussian filter is
  softened (its 13 taps averaged with an 8-tap spiral turned per pixel).

Without the component every view draws as upstream.

## bevy_solari 0.20.1 (MIT OR Apache-2.0, Bevy contributors)

For the experimental ray-traced lighting (`--solari`, nvforever solari.rs). One change, in
`src/scene/bindings.wesl`: `RAY_T_MIN` 0.001 → 1.5 and `RAY_T_MAX` 100,000 → 2,000,000.
The game's unit is about 1.43 cm and its cells lie tens of thousands of units from the
origin, where a position reconstructed from depth is off by far more than 0.001: shadow
rays hit their own surface (black blotches on the terrain).
