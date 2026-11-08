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

Third change (#1803), in `src/material.rs`: `MaterialQueueProbe`, an optional resource
that records watched main-pass entries and their pipeline IDs as `queue_material_meshes`
queues them, for `nvforever/src/pipboy_ready.rs`; Bevy installs none, so without it the
pass does what upstream does. Drop it when an upstream observation API replaces its
caller or that readiness check is retired. Retries for meshes not yet uploaded and a
per-view visibility filter of pending entries were tried with it and dropped: the
`pipboy-first-open` scenarios passed with a clean first opening without them
(2026-11-07).

## bevy_render 0.20.1 (MIT OR Apache-2.0, Bevy contributors)

Early finishing of command encoders (#1427, docs/design/frame-rate-audit.md §5). One
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

Second change (docs/design/frame-rate-audit.md §7), in `src/batching/gpu_preprocessing.rs`,
`coalesce_batch_ranges` (comment starting `NVForever coalesced build jobs`):
`prepare_indirect_parameters_build_jobs` merges each view's batch ranges that follow one
another into one build job. Upstream makes one job, and so one compute dispatch, per
range; wgpu puts a barrier between dispatches that write the same storage buffers, and on
the GPU each one waits for the last to drain. The shader handles each batch index on its
own, so the merged dispatch does the same work. Drop it if upstream merges the jobs.

[observed 2026-11-08, Nsight Graphics GPU Trace, Goodsprings 12:00, 2560x1440, 4x MSAA,
no actors] Compute dispatches a frame 2,518 → 904, compute-active time 16 → 9 % of the
frame; interleaved runs of one build with and without it: 9.0 → 8.5 ms a frame.

## wgpu-ffx 0.1.0 (MIT, the wgpu-ffx developers and Advanced Micro Devices)

The FSR 3.1.4 upscaler (docs/design/fsr-upscaling.md): a Rust/wgpu port of AMD FidelityFX
SDK 1.1.4's upscaler, [cwfitzgerald/wgpu-ffx](https://github.com/cwfitzgerald/wgpu-ffx) at
commit `6fe7064064b396b30df97341c2ca957ca1b00884`. It is not on crates.io, so it is not a
`[patch]`: the engine uses its `wgpu-ffx` and `wgpu-ffx-shaders-spv` crates as path
dependencies, and `engine/Cargo.toml` excludes the workspace. Its shaders are the SDK's
GLSL (FidelityFX-SDK commit `c6efa6bf7f2027b3ec94f28578bb5965eabb9e55`, with AMD's MIT
notices), compiled to the SPIR-V that `wgpu-ffx-shaders-spv` embeds. `xtask/` and the
shaders it compiles are that SPIR-V's source and stay with it: `shaders/src/fsr3upscaler`
and what it includes (`shaders/include` with `fsr3upscaler`, `spd` for the luma pyramid
and `fsr1` for RCAS). `LICENSE.MIT` holds both copyright
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
- Left out: the upstream repository's Git and CI files, and the shader sources of the SDK's
  other effects (blur, Brixelizer, CACAO, CAS, the denoiser, depth of field, FSR 2, frame
  interpolation, optical flow, SSSR and the rest). Nothing here compiles them:
  `cargo xtask compile-shaders` builds the effects with a `perm.toml` in
  `wgpu-ffx-shaders-spv/src`, only FSR 3.1. Every `#include` of its shaders resolves among
  the kept files.

The kept shader files are unchanged. To rebuild the SPIR-V, install `glslc` (Vulkan SDK or
Shaderc) and run `cargo xtask compile-shaders` in `engine/vendor/wgpu-ffx`; `cargo xtask
vendor` downloads the whole AMD SDK and is not needed. Not used: AMD's signed DLLs, frame
generation, the SDK's samples and prebuilt binaries.

## Dropped

- bevy_pbr 0.19.1 (#903, PR #905): the bindless index-table comparator in
  `MaterialBindlessSlab::get_bindless_index_table`, reversed in 0.19.1, which kept freed
  `ExtendedMaterial`s' textures in the slabs. Fixed upstream (bevyengine/bevy#25848);
  Bevy 0.20 has the fix, in `bevy_render::material_bind_groups` (#934).
