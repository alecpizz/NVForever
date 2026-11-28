# Patched dependencies

Crates copied from crates.io with a small change, used through `[patch.crates-io]` in
`engine/Cargo.toml`. Each keeps its own licence files. When the crate is upgraded, copy
the new version here and apply the change again (or drop the patch if upstream fixed it).

## bevy_winit 0.20.1 (MIT OR Apache-2.0, Bevy)

Adds an explicit cursor request/receipt consumer before ordinary Window
synchronization. Only acknowledged requests update both Window and its private
cache; ordinary setters retain their existing path. The cache remains private.
The `cursor-test-support` feature exposes an offscreen fixture for that same
consumer with an injected native result, without creating an OS window.
See `docs/spec-notes/windows-cursor-ack.md` for ownership, bounds and validation
limits. Both upstream license files are retained.

In `bevy_winit/src/lib.rs`, the crate-level `unsafe_code` lint changes from
`forbid` to `deny` so Windows-only `native_cursor_owner` in
`bevy_winit/src/cursor_position.rs` can opt in locally. It uses the live winit-owned
window handle and a stack `POINT` to check foreground ownership and whether the
cursor is inside the client area before a warp, even without a motion event. The
query itself changes neither focus nor cursor position. Every pre-existing
module retains `forbid`; the rest of the new cursor consumer inherits `deny`.

In `bevy_winit/src/winit_windows.rs`, `attempt_grab` is a no-op returning
`Ok(())` on `target_os = "android"`, because Android `NativeActivity` has no
desktop cursor-grab API and winit otherwise logs `Unable to ungrab cursor` and
`Could not set cursor grab mode` during initial `CursorOptions` sync.

Source: crates.io `bevy_winit` 0.20.1, archive SHA-256
`397c685e6ab6d4d87b89fb4ce11993c223d0e3c7b34c415cb4b9f69f213d85b6`.

## gilrs 0.11.2 (Apache-2.0 OR MIT, gilrs authors)

Gamepad input under bevy_gilrs. One change (#3097), marked `NVForever patch` in
`src/gamepad.rs` `Gilrs::next_event_priv`: a raw event for a pad id gilrs holds no data
for (anything but the `Connected` that adds the next pad) is dropped, with one warning
per pad id, and the next event is read instead.

Why: gilrs looks every button and axis event's pad up with `Gilrs::gamepad`, which
indexes `gamepads_data[id]`, and bevy_gilrs runs the `axis_dpad_to_button` filter on every
event, which does the same. A second controller's input arriving before its `Connected`
(or a `Connected` with an id past the next one, which upstream only logs as a platform
error) panicked the game: crash signature `panic-f2bce0a1986452c8`, "index out of bounds:
the len is 1 but the index is 1" at `gamepad.rs:474`. 0.11.2 is still the latest release.

Second change, in `SDL_GameControllerDB/gamecontrollerdb.txt` (`# Windows`) and
`src/mapping/mod.rs` (unit test `gamesir_hid_3537_100e_windows_mapping`): adds Windows
USB (`03000000373500000e10000000000000`) and Bluetooth (`05000000373500000e10000000000000`)
mappings for the GameSir HID controller (`VID 0x3537`, `PID 0x100E`), matching the
sibling `VID 0x3537` Windows HID entries already bundled in `gamecontrollerdb.txt`
(`GameSir G7 Pro` `2210`, `GameSir Tegenaria Lite` `9410`, `Anbernic RG P01` `0710`/`4610`).
Without an entry, Windows Gaming Input's `RawGameController` fallback uses `Mapping::default`,
where `b4` (`Y`) maps to `Button::LeftTrigger` (`LB`), `b0`/`b3`/`b8` shift (`A -> X`,
`X -> Y`, `LT -> Back`), and idle trigger axis `a5` at `-1.0` maps to `Axis::RightStickY`,
aiming the camera continuously upward.

The crate ships no licence files of its own; its `Cargo.toml` names Apache-2.0/MIT. The
bundled `SDL_GameControllerDB` keeps its LICENSE. `Cargo.lock` and `Cargo.toml.orig` from
the archive are left out.

Source: crates.io `gilrs` 0.11.2, archive SHA-256
`902fb00d3f6398e635be22e5c837b303c501835cca7ac11a47bba138f7aafdd8`.

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

## bevy_core_pipeline 0.20.1 (MIT OR Apache-2.0, Bevy contributors)

HDR output (docs/design/hdr-output.md). Changes, marked `NVForever HDR output`:

- `src/blit/mod.rs`: `BlitPipelineKey` gains `hdr` (a blit to an HDR window) and
  `hdr_gamma22`, adding the shader defs `NVF_HDR_PAPER_WHITE` and `NVF_HDR_GAMMA22`. The
  layout gains a third binding, the uniform `BlitPipeline::nvf_hdr_scale` (x: the HUD's
  white in nits / 80), written by `nvf_write_hdr_scale` when it changes, so moving the
  brightness rebuilds no pipeline (it was a shader constant, one pipeline per step; PR
  #3348 review).
- `src/blit/blit.wesl`: under those defs the 0..1 range is decoded with gamma 2.2 and the
  colour is scaled by `nvf_hdr_scale.x` (scRGB's 1.0 is 80 nits).
- `src/upscaling/mod.rs`: the statics `NVF_HDR_PAPER_WHITE_NITS`, `NVF_HDR_HUD_NITS` and
  `NVF_HDR_GAMMA22` (set by nvforever's settings), and the upscaling pipeline's key takes
  them for a window camera whose target is `Rgba16Float` while the window's surface is
  HDR (`bevy_render`'s `NVF_HDR_SURFACE_ACTIVE`).

Why: the window blit is the last pass that writes the swapchain, and the HUD is drawn
into the picture before it, so the HUD's brightness in nits can only be set there.
Upstream's blit copies the colour unchanged.

Graphics pass (2026-11-23), for bevy#26239 (open upstream): GPU occlusion culling's depth
pyramid is built from the main pass's region of the depth buffer only, and the downsampling
shader reads that region (`src/mip_generation/experimental/depth.rs`,
`downsample_depth.wesl`, a `bytemuck` dependency as upstream's, in `Cargo.toml`). Before it,
with an upscaler's `MainPassResolutionOverride` the pyramid was made from the whole
display-size texture while meshes were tested in render-rectangle coordinates. Upstream saw
only hidden meshes drawn; in NVForever, whose upscaler glue copies the display-size depth
back over the prepass depth (upscaler/prep.rs `expand_depth`), visible rocks, posts and
plants were culled. The culling stays an experiment behind `NVF_OCCLUSION_CULLING=1`
(nvforever gpu_occlusion.rs).

Source: crates.io `bevy_core_pipeline` 0.20.1, archive SHA-256
`2463cf477b683bfe8567c43142295d00ed980f8e2a293c0a69e2f9e10e9c0aaf`.

## bevy_post_process 0.20.1 (MIT OR Apache-2.0, Bevy contributors)

Bevy's post-processing, for its auto exposure, which meters the view for the game's eye
adaptation (`engine/nvforever/src/eye_adapt.rs`). One change (#2892), marked `NVForever
exact curve`, in `src/auto_exposure/`:

- `compensation_curve.rs`: the compensation curve's table holds the compensation values
  themselves as `f32` (texture `R32Float`); upstream keeps a `u8` between the curve's
  lowest and highest value, rounded down (`R8Unorm`). `AutoExposureCompensationCurve::
  compensation` gives the value for an average as the shader reads it, for tests.
- `auto_exposure.wesl`: the curve is read between two table entries in proportion;
  upstream reads the one entry at or below the average.

Why: the exposure is `curve(average) − average`. With one byte entry per 0.086 EV of
average and bytes 0.067 EV apart, a curve `y = x` (no change) came out as a sawtooth
between 0 and about −0.15 EV, so a daylight picture was darkened by up to 10 %, by an
amount that jumped as the view's average crossed an entry.

A second change (graphics pass 2026-11-23), marked `NVForever (upstream Bevy's fix,
backported)`, in `src/auto_exposure/auto_exposure.wesl`: `compute_histogram` adds its
workgroup's bins to the global histogram from the first 64 invocations only, and skips
empty bins. Upstream 0.20.1 let all 256 invocations of the 16 × 16 workgroup do it; the
indices past 63 were clamped onto bin 63, so the brightest bin was counted 194 times over
(the metered average and exposure moved when bright pixels were in view) and every
workgroup made ~200 contended atomics on that one address (the pass went from ~0.14 ms to
below the GPU log's top entries at 4K with DLSS Quality on an RTX 5090). Upstream Bevy's
main has the same guard.

HDR output: `src/msaa_writeback.rs` fills `BlitPipelineKey`'s two new fields (`false`,
`false`), as the vendored bevy_core_pipeline's key needs them.

A third auto-exposure change (#3917), marked `NVForever (#3917)`, in `src/auto_exposure/`:

- `auto_exposure.wesl` (`compute_average`): replaces the unclamped forward-Euler
  exponential step `delta * exp_down` / `delta * exp_up` with exact exponential decay
  `delta * (1.0 - exp(-exp_down))` clamped to `delta`. Upstream overshot `target_exposure`
  whenever `speed * delta_time / exponential_transition_distance > 1.0`, and diverged into
  a sustained `±(speed * delta_time)` EV period-2 black/bright strobe whenever that ratio
  reached `2.0` (e.g. under slow frames or fast adaptation speeds after switching Quality
  presets back up with Light glow re-enabled).
- `buffers.rs` and `mod.rs`: maps `RemovedComponents<AutoExposure>` main-world entities to
  `RenderEntity` (and prunes inactive views), removes `ViewAutoExposurePipeline` when
  `AutoExposure` is removed, and guards `auto_exposure` with `With<AutoExposure>` and
  `camera.hdr`. Upstream passed main-world entities into an `EntityHashMap` keyed by
  `RenderEntity` and left `ViewAutoExposurePipeline` attached, so removing `AutoExposure`
  (switching Light glow off via Quality Low) left the compute pass running on SDR views
  and carried stale GPU exposure state into a later switch back up.

## bevy_pbr 0.20.1 (MIT OR Apache-2.0, Bevy contributors)

Skinning storage-buffer wrapping and normal hardening in `src/render/skinning.wesl`:
storage `joint_matrices` and `prev_joint_matrices` are wrapped in
`struct StorageJointMatrices`, an exactly singular skin matrix (`det == 0.0` in
`inverse_transpose_3x3m`) uses an identity normal transform, and an exactly zero
transformed normal in `skin_normals` uses `+Z`.
Finite zero-scale animation and unweighted vertices can reach these cases; WGSL leaves
runtime divide-by-zero and zero normalization indeterminate
([WGSL floating point evaluation](https://www.w3.org/TR/WGSL/#floating-point-evaluation)).
Nondegenerate inverse-transpose/normalization arithmetic, positions, skin weights,
CURRENT/PREVIOUS storage layouts, buffer selection and morph gates are unchanged.
This defines previously undefined normal directions; it does not establish a device-loss
or Android visual-corruption cause. CPU production-WESL controls and GPU qualification
are separate; remove/reconcile this patch when upstream provides equivalent guards.


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

The same for the contact shadows' depth march (ALC shadows, once Lamp shadows, on Quality, light_shadow_detail.rs;
docs/design/graphics-enhanced.md §3.17), each part marked `NVForever (main pass viewport)`:

- `src/ssr/raymarch.wesl`: `DepthRayMarch` and its distance function carry
  `depth_uv_scale` and `depth_uv_offset` (default (1, 1) and (0, 0)): the depth texture is
  sampled at `view_uv * scale + offset`, and a ray's length in pixels is taken over the
  part of the texture the view was drawn in.
- `src/render/pbr_functions.wesl` `calculate_contact_shadow`: sets them from
  `view.main_pass_viewport` and the depth texture's size. With FSR or DLSS the main passes
  fill only the top-left render-size part of full-size textures; upstream's march read the
  whole texture as the view and threw hard-edged false shadows that changed as the camera
  moved [observed 2026-11-18, playtest videos; a walk along the Prospector Saloon's
  porch with DLSS Quality, DLSS Performance and FSR was clean after the change].

Without the override the scale and offset are (1, 1) and (0, 0), upstream's march. Bevy's
screen-space reflections (`ssr.wesl`) keep the defaults.

No directional shadows for one lighting call (the NIF shader's shade light,
nvf_assets `nif_material.wesl` `with_shade_light`), each part marked `NVForever (no
directional shadows)`:

- `src/render/mesh_types.wesl`: `NVF_MESH_FLAGS_NO_DIRECTIONAL_SHADOWS_BIT` (bit 26 of a
  `PbrInput`'s flags, which bevy_pbr leaves unused; set by shaders only, never by the CPU).
- `src/render/pbr_functions.wesl` `apply_pbr_lighting`: the directional loop's shadow-map,
  transmitted-lobe and contact-shadow conditions skip the light's shadow while the bit is
  set; point and spot lights keep theirs. `with_shade_light` relit with the receiver bit
  cleared before, which also dropped the lamps' shadows and changed the ambient occlusion's
  factor on lamps; its extrapolation (by up to 1/share) turned that difference into a glow
  on the first-person arms, weapon and Pip-Boy [observed 2026-11-19, Prospector Saloon at
  22:00, ALC shadows Quality with ambient occlusion; gone after the change].

Without the bit set the lighting is upstream's.

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

[documented: `bevy_pbr/src/material.rs`, `decal/forward.rs`, `transmission/`]
A forward decal can opt into the scene color used for refraction. The transmission
phase draws these decals over the opaque receivers before its first texture copy.
The game selects this for submerged world marks so the water shades their color
with the lakebed. Cameras without screen-space transmission use the decal's usual
alpha blending.

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

Another change (equal-depth position invariance,
docs/spec-notes/depth-equality-invariance.md), marked `NVForever equal-depth passes`:
`src/render/forward_io.wesl` and `src/prepass/io.wesl` mark both vertex position
producers invariant for the engine's Equal-depth passes. Position calculations and
depth policy are unchanged. `nvf-shader-tests/tests/depth_invariance.rs` composes the actual
source through Bevy WESL and Naga and checks the SPIR-V decoration, including
unannotated controls. An earlier integrated run passed three controls and exposed
two compiler-setup failures; the corrected pass-specific feature matrix and shared
production lighting preparation are unrun. Reapply both annotations
after upgrading Bevy until upstream provides the same guarantee, or retire them
with the Equal-depth policy.

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
  out is not the one of the frame before (the layer count grew, the map size changed, or the
  old one was dropped), the render world's hold is cleared for that frame, so every cascade is
  drawn into the new texture instead of a held one sampling an empty or stale layer (#2182
  review). It logs `held shadow maps: a new directional shadow texture`.
- `src/render/light.rs` `NvfDirectionalShadowLayerHighWater` (`#3779`, `[design]`): while
  `HoldDirectionalShadowMaps` or `HoldLightShadowMaps` is active and the directional shadow
  map's side stays unchanged, `prepare_lights` keeps a high-water layer count instead of
  shrinking the descriptor back down when first-person `ArmsShade` or the flashlight turns
  off, avoiding repeated 256–384 MB (at 4096) texture reallocations on view/flashlight
  transitions. Changing the shadow map resolution resets the high-water count to the needed
  layers; when the cascade count changes on a reused texture, the frame clears the directional
  hold once so no held cascade samples a shifted layer.

Bevy inserts no such resource, and without it (or with no bits set) nothing changes.
Whoever sets it must hand the lights the held cascades as the frame that drew them had
them, as the game does.
Upstream has no way to draw a light's shadow map less often than every frame.

The same for point and spot lights (held light shadow maps, 2026-11-18; the game's Lamp
shadows, nvforever light_shadows.rs, docs/design/graphics-enhanced.md §3.17), marked
`NVForever held light shadow maps`:

- `src/render/light.rs`: the render-world resource `HoldLightShadowMaps` (extracted from
  the main world's) with `slots` (a stable slot per shadowed point light), `point_capacity`,
  `spot_slots` and `spot_capacity` (the same for spot lights), `held` (a bit per cube face,
  bit 0 for a spot light) and `fade` (a light's shadow strength, 0..1). In
  `prepare_lights`:
  - A shadowed point light with a slot draws its cube into layers slot × 6 + face, not its
    index among the shadowed lights. Without that a light starting or stopping to cast moved
    every later light's layers. Other shadowed point lights (the poker lamp) take the lowest
    free slot each frame. The cube texture has room for the highest slot and
    `point_capacity`.
  - The light's GPU flags carry slot + 1 in bits 16..23 (bits 0-15 are Bevy's flags and
    its `UNINITIALIZED` mark; Bevy's point light flags use bits 0-6 only). A light whose
    slot + 1 would not fit in 8 bits gets no shadow flag.
  - A point light with a slot whose `shadow_maps_enabled` is false (no views: no culling,
    extraction of casters or queueing) still gets the shadow flag and its slot. It then
    casts from its cube once all six faces hold what it drew into this texture. The game
    switches a light's maps on only on the frames it draws them.
  - Spot lights the same way, with `spot_slots` and `spot_capacity`: a shadowed spot light
    with a slot draws its map into layer `cascades + slot` of the directional shadow
    texture, not `cascades + its index among the spot lights` (that index follows the
    shadow-enabled order, so it moved whenever another spot light started or stopped
    casting); any other shadowed spot light (the flashlight) takes the lowest slot left that
    frame. The texture has room for the cascades plus the highest slot + 1, `spot_capacity`
    and Bevy's shadowed spot count (without the resource: Bevy's count, as upstream). The
    flags carry the absolute layer + 1 in bits 16..23; a spot light with a slot and no views
    gets the shadow flag and its layer once that layer holds what it drew into this texture.
    The layers below the cascade count are dropped from the record every frame (the sun's
    cascades draw there; the count changes when sun shadows or the cascade count change), so
    a layer the cascades took over is never taken for a spot light's map. A slotted spot
    light whose layer has no room in the texture gets no views and no shadow flag.
  - The cascade count and the directional texture are taken before the lights' GPU data
    (moved up from below), since a spot light's layer and the texture's id decide its flag.
  - `fade` (per light, absent: 1), the weight of the light's shadow (the game's fade, 0..1,
    times its strength, up to `NVF_SHADOW_WEIGHT_MAX`, 1.27), goes into bits 24..31 of a
    slotted light's flags as `255 - round(weight × 200)` (`nvf_fade_bits`), so 0 (every
    other light) is a weight of 1. Until 2026-11-25 it was a fade alone, `255 - fade × 255`.
  - A shadowed light's views are made again when its slot (or spot layer) or the texture
    changed, not only when the light changed. Upstream left a light's views pointing at its
    old layer and texture when another light changed the shadowed set.
  - The point texture is taken before the lights' GPU data, since its id decides the flag.
  - A held face, or spot map, is left out of the frame's root views: its
    `RootNonCameraView` is removed, so the camera driver runs no Core3d schedule (culling,
    preprocessing, pass) for it. It is held only if its layer was last drawn by the same
    light into the same texture (`NvfLightShadowRecord`); otherwise it is drawn, whatever
    the hold says.
- `src/render/shadow_sampling.wesl`: `nvf_point_shadow_cube` turns a light's id into its
  cube (its slot from the flags' bits 16..23, else its id, as before), in the two point
  shadow lookups; `nvf_spot_shadow_layer` turns a spot light's id into its layer (from the
  same bits, else Bevy's `light_id + spot_light_shadowmap_offset`).
- `src/render/shadows.wesl`: Bevy's `fetch_point_shadow` and `fetch_spot_shadow` are renamed
  `nvf_fetch_point_shadow_unfaded` and `nvf_fetch_spot_shadow_unfaded` (the spot one takes
  its layer from `nvf_spot_shadow_layer`); new `fetch_point_shadow` and `fetch_spot_shadow`
  of the same signatures return `max(1 - weight × (1 - shadow), 0)` with the weight from
  bits 24..31 (`nvf_shadow_weight`, `nvf_weigh_shadow`; `nvf_shadow_fade` is the weight held
  to 1). `nvf_fetch_point_shadow_weighed` and `nvf_fetch_spot_shadow_weighed` give the same
  not held to 0, and `apply_pbr_lighting`'s point and spot loops call those: with a weight
  past 1 a place the map shades in full gets less than none of the light, which nvforever's
  display sum takes off the other light there (its edits make Bevy's own sum take
  `max(shadow, 0)`). The weight is applied here, not in `pbr_functions.wesl`, whose
  `apply_pbr_lighting` nvforever's lights/display_shader.rs patches by text anchors.
  `pbr_functions.wesl` does fade the contact shadows of point and spot lights
  (2026-11-24), since 2026-11-25 with the map's shadow in one:
  `mix(1 - fade, shadow, contact)`, and only where the map leaves light
  (`shadow > 1 - fade`). Faded apart, the two multiplied below a fade of 1.
- `src/volumetric_fog/volumetric_fog.wesl`: `fetch_spot_shadow_without_normal` takes its
  layer from `nvf_spot_shadow_layer` too (no fade: fog is not faded).

Without the resource, or with it inactive, nothing changes. `HoldLightShadowMaps::active`
is the gate: the game sets it only while its ALC shadows row is on (nvforever
light_shadows.rs, [design, #3077 review]); while it is false, `prepare_lights` takes the hold
as absent, so no light gets a slot, a held face, the static cache or a fade, and these paths
all run as upstream's: the hidden light views below, the cached shadow views, the kept shadow
phases (`NvfParkedShadowViews::active` false: `specialize_shadows` and `queue_shadows`
specialize and bin as upstream, `nvf_add_once` is upstream's `add`) and the light view
counts (no atomics touched, no clock read). When it switches either way, every light's
shadow views are made again and the record of what each layer held is dropped, so no view
keeps a slot's layer or a static-cache attachment it no longer has. What stays compiled in
either way: the shaders' lookups of bits 16..31 (all 0 without the hold: upstream's index,
full strength) and the shadow textures taken earlier in `prepare_lights` (the same
descriptors as upstream's while nothing is held). Measured before it (2026-11-18, RTX
5090, 1280 × 720, the Prospector Saloon): every shadowed point light cost about 1 ms of
frame time, six Core3d schedule runs a frame.

Two more for the ALC shadows' (once Lamp shadows) performance pass (2026-11-18, docs/design/graphics-enhanced.md
§3.17 "Performance pass"):

- **Hidden light views** (`NVForever hidden light views`, `src/render/light.rs`
  `extract_lights`): a point or spot light that is hidden (its `ViewVisibility` false)
  loses its `PointAndSpotLightViewEntities` together with its `ExtractedPointLight`, so the
  observer `remove_point_and_spot_light_view_entities` despawns its shadow views. Upstream
  removed only `ExtractedPointLight`; `prepare_lights`, the only place that despawns the
  views of a light no longer shadowed, never saw that light again, and its six (or one)
  views stayed alive and ran the whole Core3d schedule every frame while it was hidden
  [documented: bevyengine/bevy#26059; the fix is bevyengine/bevy#26480, the same two lines,
  still open (not merged) on 2026-11-18, which measured 16 point and 16 spot lights hidden
  at 7.6 ms before and 1.2 ms after]. The game's lamps switch their maps on for a frame and
  off again, so a lamp hidden by a turn of the camera in the frame after its maps were drawn
  left its views running until it was seen again [hypothesis, from the code; the summary's
  `despawned with their light hidden` count shows how often it happens]. Its regression
  tests are not ported (the engine workspace does not run the vendored crates' tests). Only
  while the hold is active (as last frame's `prepare_lights` saw it): with the ALC shadows
  row Off it is upstream's two lines. It stays behind the row until upstream merges
  #26480 [design, #3077: the maintainer's decision of 2026-11-21]; drop it with the first
  Bevy release that has #26480.
- **Light view counts** (`NVForever light view counts`, `src/render/light.rs` and
  `src/lib.rs`): `nvf_count_light_views` (render world, `RenderSystems::Prepare`, after
  `prepare_lights`) counts the point and spot light shadow views alive and those with a
  `RootNonCameraView` (those that run the Core3d schedule); `extract_lights` counts the
  views a hidden light takes with it. `extract_lights` and `prepare_lights` are timed
  (`std::time::Instant`, summed and longest a frame): `prepare_lights` whole and in its
  consecutive stretches (`NVF_SECTIONS`: the light uniforms' writer, counts and slots, the
  shadow textures and their views, the lights' GPU data, the point lights' views, the spot
  lights' views, the cameras' cascades, the closing sweep of phases and caches), and its
  events counted (`NVF_EVENTS`: views spawned, despawned and made again, shadow phases made
  and dropped, point and directional shadow textures that were not the last frame's). The
  counts are summed in atomics and read and reset by `nvf_take_light_view_counts()` (the
  game's 10-s ALC shadow summary, `light shadows: renderer over …`). Bevy reads none of it;
  the cost is a few clock reads and relaxed atomic adds a frame. Drop it once the counts
  are not wanted.
- **Cached shadow views** (`NVForever cached shadow views`, `src/render/light.rs`
  `prepare_lights`, `NvfShadowViewCache`): the point and directional shadow textures' array
  views, the point and spot lights' per-layer attachments and the cascades' per-layer views
  are kept across frames while the texture is the same (by texture id; a new texture
  starts them anew), instead of made with `create_view` every frame. A kept attachment is
  set to clear again (`prepare_for_new_frame`) at the start of each frame; the cascades
  still get a new attachment each view, over the kept view. Upstream makes the two array
  views and every cascade's layer view every frame, and a point light's six layer views
  every frame its views are made again (every frame it draws, since its extraction changes).
  The count `texture views made` shows what is still made. `NVF_SHADOW_VIEW_CACHE=0`
  (read once) makes every view anew each frame, as upstream, for comparing. [hypothesis:
  `prepare_lights` took up to 16 ms in the main session's profile while hundreds of
  references were toggled and faded in Primm (thousands of materials made and dropped
  on other render threads); fewer device calls there give it less to wait on. Not measured
  yet.]

One more for the ALC shadows (static shadow cache, 2026-11-18; docs/design/graphics-enhanced.md
§3.17 "Static shadow cache"), marked `NVForever static shadow cache`, all in
`src/render/light.rs`:

- `HoldLightShadowMaps` gains `static_cache` (keep the cache), `to_static` and `on_static`
  (bits per cube face of a point light, this frame). With `static_cache`, `prepare_lights`
  takes a second cube texture from the texture cache, `nvf_point_light_static_shadow_cache`,
  of the same size and layers as the sampled point texture (`RENDER_ATTACHMENT | COPY_SRC`;
  the sampled texture gets `COPY_DST` too, so it is another texture while the cache is on),
  every frame so the texture cache keeps it. Of a point light's faces drawn this frame (not
  held):
  - a face in `to_static` gets an `NvfStaticShadowFace` on its view: the pass draws into the
    cache's layer (an attachment that clears) in place of the `ShadowView`'s, and after the
    pass the layer is copied into the same layer of the sampled texture (whole face, one
    array layer, `copy_texture_to_texture`; depth copies take whole subresources of one
    format [documented: WebGPU `copyTextureToTexture` validation for depth-or-stencil
    formats]). What the layer was drawn with (light, cache texture) is recorded.
  - a face in `on_static` whose cache layer holds what this light drew into this cache
    texture gets the cache's layer copied into the sampled layer before the pass, and the
    pass draws over it with an attachment that loads (no clear value). Otherwise (no such
    record) it is drawn as any face, and counted as a miss.
  - any other face loses the component and is drawn as before.
  The copies are recorded in `view_shadow_pass` (the shared shadow pass of the point light
  views), on the view's own encoder: the copy in before the early pass, the copy out after
  the last pass (the late one only with occlusion culling, which light views do not have).
  Which casters each face's list holds is the game's to choose (nvforever light_shadows.rs
  `skip_fixtures`: the static ones into the cache, the moving ones over it). The cache's
  layer attachments and the sampled texture's loading attachments are kept in
  `NvfShadowViewCache` like the others. Three more events in the light view counts: `static
  cache draws`, `static cache composites`, `static cache misses`.
  Without `static_cache` (Bevy's default, and the game's `NVF_LIGHT_SHADOW_STATIC_CACHE=0`)
  nothing changes. The cache costs as much memory as the sampled cubes (at 512 × 512, 6 MB a
  lamp). Upstream has no way to draw a shadow map over an earlier picture: its shadow
  attachments always clear. Not measured in game yet.

One more for the ALC shadows (kept shadow phases, 2026-11-19; docs/design/graphics-enhanced.md
§3.17 "Kept shadow phases"), marked `NVForever kept shadow phases` and `NVForever kept shadow
views`, in `src/render/light.rs` and `src/material.rs`:

- **Kept shadow phases.** The render-world resource `NvfParkedShadowViews` (registered in
  `MaterialsPlugin`): `prepare_lights` fills it each frame with the retained view entities of
  every light `HoldLightShadowMaps` gives a slot (six faces of a point light, one view of a
  spot light), drawing or held, seen or not. Its closing sweep keeps their shadow phases,
  `LightKeyCache` entries and specialized pipelines though the light has no views
  (upstream drops them with the views, so a light whose maps are switched off and on again
  had every caster specialized and queued anew into new phases).
  `specialize_shadows` keeps their pending queues and pipelines the same way, and checks
  each such view that has no view entity this frame against the frame's
  `DirtySpecializations`: an entity in its pipelines that changed (`changed_renderables`:
  its mesh, material or flags) makes the kept view stale, and its phase, key, pipelines and
  pending queue are dropped (counted as `kept phases wiped`), so the light is drawn as a new
  one; an entity removed is dropped from its pipelines only. A kept view back after a frame
  without a view is `resumed` (`specialize_shadows` fills `NvfParkedShadowViews::resumed`):
  its light's visible list went on changing (or was made anew, a hidden light's, every
  visible entity "added") while no `queue_shadows` saw the differences, so instead of them
  every visible entity is specialized if it is not, and `queue_shadows` resyncs the phase
  (`nvf_resync_kept_phase`): binned entities no longer visible leave, visible ones not
  binned are queued. Every add goes through `nvf_add_once`, which takes an entity already
  binned out first: queueing from the differences alone binned entities twice, bevy_render's
  panic "Binning main entity … when it was already binned" (Novac at 22:00, ALC Quality,
  2026-11-19; unit test `a_kept_shadow_phase_back_in_view_is_resynced_and_never_bins_twice`).
  The resync also takes out the stale entries (2026-11-25): a mesh no view saw while the
  light's views were away is dropped from `RenderMeshInstances` (upstream extracts only
  visible meshes), and seen again it gets another input uniform index than the one its bin
  was filled with. The kept bin went on drawing it with the old index, by then another
  mesh's transform: slabs and shards of shadow around the player in the Primm casino, where
  about 100 casters only the lamp's own views see come and go with each of its draws. A
  binned entity whose mesh instance's `current_uniform_index` is not the one it was binned
  with (`CachedBinnedEntity::nvf_input_uniform_index`, bevy_render below) leaves its bin
  and is queued again (`NVF_LIGHT_SHADOW_STALE_CHECK=0` leaves the check out, to see the
  fault again).
  The same day, from a reading of the code: an entity removed while the view is away
  leaves the kept phase's bin as well as its pipelines (`nvf_binned_len`; one removed and
  then given another mesh or material in a later frame was no longer among the pipelines,
  so the view was not wiped and drew it from the old bin); the far texture's lights
  (`far_slots`) keep their phases too (they were left out); and a resumed view queues only
  the pending pairs still visible.
  New events in the light view counts: `phases reused` (phases a light's new views found
  kept), `kept phases wiped`, `kept phases resynced` (resumed views), `entities
  rebinned` (the guard's catches; none expected) and `stale entries requeued`. The game's
  `NVF_LIGHT_SHADOW_KEEP_PHASES=0` (read once, `nvf_keep_shadow_phases_on`) leaves the set
  empty: upstream's behaviour.
- **Kept shadow views.** A point or spot light whose `ExtractedPointLight` changed (every
  frame it draws: its casters' lists change) has its views made again only when what they
  are made from changed (`NvfViewSource`: a point light's place, a spot light's transform
  and outer angle, the near plane, the range, the render layers; or its slot or texture, as
  before). A face drawn on views not made again gets its `RootNonCameraView` back (a held
  face's was removed). Event `views kept`. With `NVF_SHADOW_VIEW_CACHE=0` they are made again
  as before.

Without `HoldLightShadowMaps` the set stays empty and the sweep is upstream's. [hypothesis:
part of the measured 1.4 ms a held light's switch-on cost goes; the view spawn and the drawn
faces' Core3d runs stay. Not measured in game yet.] Upstream has no way to keep a shadow
view's phase without the view.

Gated materials (#2661, docs/design/graphics-enhanced.md §3.14 "Off"), in `src/material.rs`,
each part marked `NVForever gated materials`: `MaterialPlugin::gated(MaterialGate)` (a new
`gate` field, `None` by default). With a gate, the material type's own systems run only
while it is open (main world: `mark_meshes_as_changed_if_their_materials_changed`,
`check_entities_needing_specialization`; render world: `extract_mesh_materials`,
`early_sweep_material_instances`, the two specialization extractions, and the asset
extraction and preparation through `ErasedRenderAssetPlugin::gated` below); its shaders are
loaded and its bind group allocator (with the bindless fallback buffers) made at the first
extraction after the gate opens, and both dropped at the first extraction after it closes
(a device recovery while open makes the allocator again, as upstream's startup system
does). The owner opens the gate before the type's first asset or component and closes it
only once none has been left for a few frames. Without a gate (every Bevy and every other
NVForever material) the plugin does what upstream does. Upstream registers a material's
systems, loads its shaders and makes its allocator at startup, and a plugin cannot be added
after startup; nvforever's mesh terrain blend, off in most runs, gates its two material
types with it. Drop it if upstream gains a way to register a material type lazily.

Another change (sun visibility, #2670 and #2671, docs/design/sun-visibility.md), each part
marked `NVForever sun visibility`, for nvforever's `sun_visibility.rs`:

- `src/render/light.rs`: the render-world component `NvfOutdoorSun`. `prepare_lights` sets
  the new `DirectionalLightFlags::NVF_OUTDOOR_SUN` (bit 4; `mesh_view_types.wesl`
  `DIRECTIONAL_LIGHT_FLAGS_NVF_OUTDOOR_SUN_BIT`) in the GPU flags of the directional light
  whose render entity has it, so a shader can tell the outdoor sun from other directional
  lights without assuming an index.
- `src/render/mesh_view_bindings.rs`: the render-world view component
  `NvfSunVisibilityBindings` (a uniform buffer, a filterable float 2D texture for the
  clouds, an unfilterable float 2D texture for the terrain shadow-height map and a
  filtering sampler), the layout key bit `MeshPipelineViewLayoutKey::NVF_SUN_VISIBILITY`
  and bindings 39-42 of the mesh view bind group, present only for a view with it.
  Binding 41 is declared `Float { filterable: false }` (2026-11-17, #2671 stage T2): the
  terrain map is `Rgba32Float`, which is filterable only with the optional
  `FLOAT32_FILTERABLE` feature, so nvforever reads it with `textureLoad` and never with
  the sampler; its stand-in while no map is uploaded is nvforever's own one-texel
  `Rgba32Float` texture, not Bevy's fallback image. Bindings 45 and 46 (2026-11-20,
  #3032 stage 2i M2), under the key bit `MeshPipelineViewLayoutKey::NVF_CLOUD_VOLUME`
  (with `NVF_SUN_VISIBILITY` only; the component's `cloud_volume`): the volumetric
  clouds' light volume, its near and far completed front sets (`light_near`,
  `light_far`: filterable float 2D arrays, `R16Float`, sampled with 42), which the cloud
  shadows read from the volume (nvforever `light_volume_reader.wesl`); nvforever's own
  one-texel, two-layer stand-in when there is none. nvforever leaves `cloud_volume` off
  while its Volumetric clouds row is off and on an adapter whose fragment stage has too few sampled textures for two more (the
  weather-only layout, `sun_visibility.rs` `CloudVolumeRoom`). The entries 39-46 are
  made by the public `nvf_sun_visibility_layout_entries`, which the tests read.
- `src/render/mesh.rs`: `MeshPipelineKey::NVF_SUN_VISIBILITY`, `NVF_CLOUD_VISIBILITY` and
  `NVF_TERRAIN_VISIBILITY` (bits 25-27; the key's bit fields above them move up by three,
  well below the base mesh key's bits from 58), and `NVF_CLOUD_VOLUME_VISIBILITY` (bit 29,
  after the ambient occlusion's 28; shader def `NVF_CLOUD_VOLUME`), set by
  `check_views_need_specialization` from the same component, and the shader defs of the
  same names. The pipelines' layout
  key and the view's bind group are both read from that one component in the same frame,
  so they cannot disagree; the component is inserted, changed and removed only during
  extraction.

The shaders that use the bindings are nvforever's (`sun_visibility.wesl`, patched into
`apply_pbr_lighting` at run time like lights.rs's edits). Bevy inserts neither component;
without them the flags, layouts and shader defs are upstream's, and so is every view while
the game's switches are off; the pipeline keys are the same keys numbered differently (the
packed fields above the new bits moved up by three). The meshlet path (feature off here) builds its
view key without the bits, so a meshlet view must not get the component. Upstream has no
way for an application to add bindings to the mesh view bind group.

Another change (ambient occlusion on the ambient light, #2897 stage two,
docs/design/graphics-enhanced.md §3.1), each part marked `NVForever ambient occlusion`, for
nvforever's `ssao_lighting.rs`, built the same way as the sun visibility's:

- `src/render/mesh_view_bindings.rs`: the render-world view component
  `NvfAmbientOcclusionBindings` (a uniform buffer and an unfilterable float 2D texture, the
  half-resolution occlusion read with `textureLoad`), the layout key bit
  `MeshPipelineViewLayoutKey::NVF_AMBIENT_OCCLUSION` and bindings 43-44 of the mesh view
  bind group, present only for a view with it.
- `src/render/mesh.rs`: `MeshPipelineKey::NVF_AMBIENT_OCCLUSION` (bit 28; the key's bit
  fields above it move up by one more, their top, the colour target format, ending at bit
  49, still below the base mesh key's bits from 58, which the file's `const_assert_eq!`
  checks), set by `check_views_need_specialization` from the same component, and the shader
  def of the same name, pushed only for a pipeline that writes depth (opaque, alpha mask,
  alpha to coverage), the rule Bevy's own `SCREEN_SPACE_AMBIENT_OCCLUSION` def follows: a
  blended draw is not in the depth prepass the occlusion is worked out from. A blended
  pipeline of such a view keeps the bindings in its layout and leaves them unread.

The shader that reads the bindings is nvforever's (`ssao_lighting.wesl`, patched into
`apply_pbr_lighting` at run time after the sun visibility's hook). Bevy inserts no such
component; without it the layouts and shader defs are upstream's, and the pipeline keys are
the same keys numbered differently (the packed fields above the new bit moved up by one).
As for the sun visibility, a meshlet view must not get the component.

Another change (the bounce light's volume, #3009, docs/design/rtgi-volume.md), each part marked
`NVForever RTGI volume`, for nvforever's `rtgi_volume.rs`, built the same way. nvforever inserts
the component only where something asks for a volume, which a developer aid alone does:

- `src/render/mesh_view_bindings.rs`: the render-world view component `NvfRtgiVolumeBindings`
  (a uniform buffer, a filterable float 3D texture and a filtering sampler), the layout key bit
  `MeshPipelineViewLayoutKey::NVF_RTGI_VOLUME` (20) and bindings 47-49 of the mesh view bind
  group, present only for a view with it. The entries are made by the public
  `nvf_rtgi_layout_entries`, which the tests read. Two counters, `NVF_VIEW_BIND_GROUPS` and
  `NVF_RTGI_VIEW_BIND_GROUPS`, say how many mesh view bind groups `prepare_mesh_view_bind_groups`
  made and how many of them with those bindings.
- `src/render/mesh.rs`: `MeshPipelineKey::NVF_RTGI_VOLUME` (bit 30; the packed fields above it
  move up by one more and end at bit 51, still below the base mesh key's bits from 58), set by
  `check_views_need_specialization` from the same component. No shader def follows from it:
  nvf-assets' NIF material pushes `NVF_RTGI_VOLUME` for the pipelines that read the bindings.

Bevy inserts no such component; without it the layouts and shader defs are upstream's, and the
pipeline keys are the same keys numbered differently. A meshlet view must not get it.

Two more for the ALCS row (the ALC shadows' rewrite, 2026-11-24;
docs/design/graphics-enhanced.md §3.17 "Rewrite of 2026-11-24"):

- **ALCS soft shadows**, marked `NVForever ALCS soft shadows`, in
  `src/render/shadow_sampling.wesl` (`nvf_sample_shadow_cubemap_soft`,
  `nvf_point_shadow_major`, `nvf_point_shadow_depth`, `nvf_own_surface`) and
  `src/render/shadows.wesl` (the point light's soft branch calls it in place of
  `sample_shadow_cubemap_pcss`). Percentage-closer soft shadows of a point light in world
  units: the blocker search covers the light as the fragment sees it (`light_size /
  distance`, a tap at the fragment itself and 8 round it), the filter (12 taps) is as wide as
  the penumbra of the blockers found, never under a texel, and each tap is compared against
  where the fragment's own surface (the plane through it, from the surface normal) lies
  along that tap, less a texel-sized margin. The taps turn by a per-pixel angle, a new one
  each frame under `TEMPORAL_JITTER`. Since 2026-11-25 the search covers twice the light's
  width (`NVF_SOFT_SEARCH_SCALE`) and the filter is never wider than the search nor than
  `NVF_SOFT_FILTER_MAX` (0.015, was 0.07 with a search of at most 0.1): a filter wider than
  the search blurred a shadow whose outer part the search had not found, a faint blob with
  a hard outline where a lamp hangs close over what it lights. Upstream's version scales the search and the filter
  by the light's size as a plain number and compares every tap against the fragment's one
  clip-space depth: with a light size in the game's units (24) every lit surface shadowed
  itself in stripes. `sample_shadow_cubemap_pcss` is left as it is, unused by the game.
  Since the second round the same day a lamp the game gave a slot takes it with or
  without a soft size: with size 0 there is no search, a fixed filter of
  `NVF_SOFT_FILTER_MIN` texels and `NVF_PLAIN_FILTER_TAPS` taps (the camera's filter spans
  a fraction of a texel of the lamps' maps, whose texels then showed as blocks).
- **ALCS soft sizes**, marked `NVForever ALCS soft sizes`, in `src/render/light.rs`:
  `HoldLightShadowMaps::soft_size` gives a light's soft-shadow size while the hold is
  active, in place of `soft_shadows_enabled` and `radius` (upstream takes the size from the
  radius, which its lighting also reads). A light not in the map is upstream's.
- **ALCS unready casters**, marked `NVForever ALCS unready casters`, in
  `src/render/light.rs`: `queue_shadows` counts, for each point or spot light view drawn
  this frame while the hold is active, the casters it could not queue or that will be
  skipped when drawn (not specialized though a caster to be, `nvf_caster_to_be`; no mesh
  instance, prepared material or mesh slabs; a pipeline not built yet), and notes whether
  any pipeline is being built (`PipelineCache::waiting_pipelines`). Lights with either are
  kept for the game in `NVF_UNREADY` (`nvf_take_unready_shadow_lights`: light, casters,
  building). Upstream draws such a view again next frame and the caster appears; a light
  the game holds is not drawn again unless the game marks it.
- **ALCS far maps**, marked `NVForever ALCS far maps`, in `src/render/light.rs`,
  `mesh_view_bindings.rs`, `mesh_view_bindings.wesl`, `shadow_sampling.wesl` and
  `shadows.wesl`: `HoldLightShadowMaps::far_slots`, `far_capacity` and `far_size` put point
  lights' cubes in a second cube texture of another side (and a static cache of its own),
  bound to the mesh views at 50 (45 until the light volume of #3032 took 45 and 46; 47 to 49 are for the bounce light volume of #4442; the first texture's view while there is none). A far
  light's cube number is `NVF_FAR_CUBE` (128) + its slot: the number keys the per-layer
  records and attachment caches, its low seven bits are the slot in its texture, and bits
  16..23 of the light's flags carry it + 1 (`nvf_point_far`). The first texture's cubes are
  counted from the shadowed lights that draw into it alone (upstream's count of all of them
  made the texture anew whenever far lights drew). The game's own sampler reads the light's
  texture (`nvf_point_compare`, `nvf_point_depth`, `nvf_point_side`), and the normal bias,
  scaled by the first texture's texel, is scaled again for a far light
  (`nvf_point_texel_scale`). Upstream has one size for every point light's cube.
- **ALCS quiet static draws**, marked `NVForever ALCS quiet static draws`, in
  `src/render/light.rs`: `HoldLightShadowMaps::static_quiet` (bits per cube face, within
  `to_static`) and `NvfStaticShadowFace::quiet`. A quiet face is drawn into the static
  cache's layer and not copied into the sampled layer after, which keeps its picture; only
  a layer that already holds this light's map may be left so (`nvf_had_map`). The game asks
  for it where moving casters stand in the face and composites the face in the next frame,
  so their shadows never leave the sampled map. Without `static_quiet` nothing changes.
- **Hosted shadow views** (2026-11-25), marked `NVForever hosted shadow views`, in
  `src/render/light.rs` (`NvfSharedShadowViews`, `nvf_host_shadow_views`,
  `per_view_shadow_pass`), `src/render/gpu_preprocess.rs`
  (`gather_shadow_cascades_for_view`), `src/lib.rs`, with `bevy_core_pipeline`'s
  `NvfHostedView` and `bevy_render`'s `RenderContext::nvf_split_encoder`. Upstream makes
  every point and spot light shadow view a root view: the camera driver runs the whole
  `Core3d` schedule for it, which records four compute passes and the shadow pass in five
  command buffers, for each of a point light's six faces. While the hold is active the
  first camera that runs `Core3d` with light views hosts them instead, as it does its
  cascades: they join its uniform allocation, bin unpacking and mesh preprocessing passes,
  and `per_view_shadow_pass` builds each one's indirect parameters and draws its map after
  the cascades', two views to a command encoder so the encoders are finished side by
  side. The views carry `NvfHostedView` and the driver runs nothing for them. Measured in
  a casino hall of 25 lamps drawing 5 to 6 faces a frame (a build with spans): the render
  thread 8.6 ms a frame before, 6.0 ms after, 4.6 ms without the lamps' shadows; the
  camera driver 2.75 ms, then 1.07 ms. The GPU's time in the shadow passes is under
  `per_view_shadow_pass` now (the span `hosted_shadow_views` inside it), no longer under
  `auxiliary_views`. `NVF_LIGHT_SHADOW_HOSTED=0` gives upstream's root views back (a
  measuring aid). Tried first and dropped: one command encoder shared by the root views'
  systems; finishing an encoder costs by the passes in it, so the one long encoder took as
  long as the many and was finished on the submitting thread.
- **Stale unpacking bind groups** (2026-11-25), marked `NVForever stale unpacking bind
  groups`, in `src/render/gpu_preprocess.rs` `prepare_preprocess_bind_groups`: upstream
  only ever adds to `BinUnpackingBindGroups` and `UniformAllocationBindGroups`, so the
  bind groups of every view there ever was stay alive. A light's shadow views are new
  views each time its cell is loaded: with the game's lamps casting, every cell entered
  left about 1,800 bind groups for good, and each command buffer costs more to finish and
  submit the more there are (33,600 after 24 cells against 5,200; the submission 0.58 ms a
  frame, then 1.27 ms). The entries of views that are gone are dropped each frame, and no
  bind groups are made for a light view the game holds this frame (a point or spot light
  view without `RootNonCameraView`: nothing is dispatched or drawn for it). After: about
  250 bind groups in the same room however many cells were entered. Found with the game's
  `gpu resources:` line (`--log-frame-times`). The two resources are one system parameter
  now (the system had sixteen). Since bevy#26418 came in (below) the tables' entries no
  bind group was made for in a frame are dropped by its own sweep, so the drop of gone
  views' entries here was taken out again; what is left of this change is that no bind
  groups are made for a held light view.

Graphics pass 2026-11-23, four more changes:

- `src/cluster/gpu.rs`, `src/cluster/mod.rs`: bevy#25869 (GPU clustering keeps each view's
  clustering buffers from frame to frame instead of making new ones; merged upstream after
  0.20.1) and bevy#26588 (the clustering metadata staging buffers no longer pile up in a
  list that is never emptied). nvforever still clusters on the CPU unless
  `NVF_GPU_CLUSTERING=1` (main.rs): the buffer churn behind #897 is gone, but the GPU
  path's cost has to be measured first.
- `src/render/pbr_functions.wesl`, comments `NVForever shadow early-out`: a point or spot
  light whose range does not reach the fragment, or that lies behind the shading normal,
  gives shadow 0 without reading its shadow map or marching contact shadows, and a shadow
  receiver facing away from a directional light likewise. The light's contribution there is
  0 either way (Bevy's range falloff and N·L, and nvforever's display sum, lights.rs), so
  the picture is unchanged. Not under `STANDARD_MATERIAL_CLEARCOAT`, whose second lobe has
  a normal of its own.
- `src/render/mesh_preprocess.wesl`, comment `NVForever: meshes that opt out of frustum
  culling`: a mesh with `NoFrustumCulling` is never occlusion culled either (first-person
  shapes write a squeezed depth their boxes do not describe; nvforever gpu_occlusion.rs).
- bevy#26239 (open upstream; the `src/render/light.rs` and `src/meshlet/` parts): the
  depth pyramid covers the main pass's region of the depth buffer (with bevy_core_pipeline
  and bevy_render below).
- bevy#26418 (open upstream, fixes bevy#26257; `src/render/gpu_preprocess.rs`, with its
  stale-job clearing part in bevy_render's `batching/gpu_preprocessing.rs`): the GPU
  preprocessing bind groups (bin unpacking, uniform allocation, preprocess, indirect
  parameters) are kept from frame to frame, keyed by their resources, offsets and sizes, and
  dropped after two unused frames, instead of being made again for every view, phase and
  batch set each frame on the render thread. Its tests need upstream's `wgpu` `noop`
  dev-dependency and are not built here.
- Merged scene unpacking (`src/render/unpack_bins.wesl`, `src/render/allocate_uniforms.wesl`
  and the bind groups and dispatches in `src/render/gpu_preprocess.rs`, with bevy_render's
  `SceneUnpackingClassBuffers` below; comments start `NVForever merged scene unpacking`):
  `unpack_bins` and each step of `allocate_uniforms` run as one dispatch for all the batch
  sets of a view, phase and mesh indexed-ness instead of one dispatch per batch set, each
  dispatch in a compute pass with a barrier after it. Every batch set is a job in a storage
  array; each workgroup looks its job up in a workgroup-to-job table and adds the job's
  offsets to the batch-local indices the shaders used before. The fan buffer has one entry
  per workgroup; steps 2 and 3 run only when some batch set has more than 256 bins, and
  then only each batch set's own workgroups do work. [observed 2026-11-24, Novac 12:00,
  4K DLSS Quality, maxed settings] `uniform_allocation` 0.21 and `bin_unpacking` 0.20 ms
  a frame → both under 0.05; process CPU time 27.3 → 23.6 ms a frame.

## bevy_light 0.20.1 (MIT OR Apache-2.0, Bevy contributors)

Vendored 2026-11-18, unchanged but for three patches. The first (shadow caster candidates, for
the ALC shadows' (once Lamp shadows) performance pass: nvforever light_shadow_casters.rs, docs/design/graphics-enhanced.md
§3.17 "Performance pass"), marked `NVForever shadow caster candidates`, in `src/lib.rs`:

- The resource `NvfShadowCasterCandidates` (`lights`: a list of entities per light).
  `check_point_light_mesh_visibility` tests, for a shadowed point or spot light listed there,
  only the entities listed for it (one thread, `iter_many_mut` over the same query) instead
  of every shadow caster in the world in parallel; `nvf_caster_faces` applies the parallel
  pass's tests unchanged (inherited visibility, render layers, visibility ranges against
  the shadow LOD origin, the light's sphere against the caster's box, each face's frustum;
  no bounds or `NoFrustumCulling`: every face). The lists are sorted and deduplicated as
  upstream sorts them.
- The caller must list every caster whose bounds may reach the light's sphere (a superset
  is fine: what the query does not match is skipped). The game files every mesh in a grid
  by its bounds' centre (cells of 1,024 units; a sphere over 512 units, no bounds or no
  culling: listed for every light) and lists the cells a drawing lamp's sphere, grown by
  512, overlaps.

Bevy inserts no such resource; without it, or for a light not listed (the flashlight, the
menu scenes' lights), the search is upstream's. [observed 2026-11-18, the main session's
profile build in Primm at night: `check_point_light_mesh_visibility` 0.4 ms at the median and
7.6 ms in the worst frames, for at most a few lamps drawing a frame; upstream tests every
caster for each.] Upstream has no way to hand a light its candidates; drop this if it gains a
spatial index for light culling.

The second (shown shadow lights, 2026-11-19, the ALC shadows' pre-draw under a loading
screen: nvforever light_shadows.rs `shown_unseen`, docs/design/graphics-enhanced.md §3.17
"Out of view too"), marked `NVForever shown shadow lights`, in `src/lib.rs`:

- The resource `NvfShadowLightsShown` (`lights`: a set of point and spot lights).
  `check_point_light_mesh_visibility` goes over those lights after the ones the cameras see
  (each once) and finds their casters the same way, and marks each such light visible
  (its `ViewVisibility`, through a query of the point and spot lights that are not meshes,
  disjoint from the casters' query). bevy_pbr's `extract_lights` then extracts the light and
  draws its shadow maps, though no camera sees it; it lies in no cluster of any view, so it
  lights nothing on screen.
- Upstream finds casters only for lights a camera sees (its loop goes over the cameras'
  visible lights), and drops a light no camera sees from the render world, so a light's
  shadow maps cannot be drawn before it comes into view.
- `update_point_light_frusta` and `update_spot_light_frusta` (`src/point_light.rs`,
  `src/spot_light.rs`) also take a light listed there (2026-11-25): they skip a light whose
  `ViewVisibility` is off, and a shown light is marked visible only afterwards, so one no
  camera ever saw kept the default frusta, which cull every caster [from the code].

Bevy inserts no such resource; without it, or empty, nothing changes. With the ALC shadows
row Off both resources stay empty (the game's systems that fill them do not run, after
emptying them once when the row is switched off), so the culling is upstream's. Upstream has
no way to draw an unseen light's shadow maps ahead of time; drop this if it gains one.

Another change (two-channel normal maps, #2981, docs/spec-notes/pbr-materials.md), in
`src/render/pbr_functions.wesl` `apply_normal_mapping`, marked `NVForever two-channel normal
clamp`: Z is rebuilt as `sqrt(max(1 - x² - y², 0))` instead of `sqrt(1 - x² - y²)`, so X and Y
outside the unit disk (BC5 endpoints, filtering between texels) give Z 0 instead of a NaN.
Only materials whose normal map is a two-channel format (`Bc5RgUnorm`, `Rg8Unorm`: Bevy's
`TWO_COMPONENT_NORMAL_MAP` flag) reach the line; in NVForever those are the authored `_n`
maps (nvf-assets `TextureUsage::NormalTwoChannel`), while the game's own normal maps are
expanded to three channels on the CPU (docs/spec-notes/dds-bc5.md). Inside the disk the
result is upstream's. Drop it when upstream clamps there.

Another change (height maps, #2981 stage 4, docs/spec-notes/pbr-materials.md), in
`src/render/parallax_mapping.wesl` `sample_depth_map`, marked `NVForever height maps`: the
sample (bindless and ordinary binding alike) is returned as `1.0 - r` instead of `r`. The
authored `_p` stores height (white = high); Bevy's march reads depth (white = deepest). The
inversion sits after the two bindings' samples, so they cannot diverge; the march and both
refinements read only through this function. It changes the convention for every
`StandardMaterial::depth_map` in the engine: none was set in production before (terrain
asserts landscape materials have none; the one positive assignment is a Pip-Boy test
fixture), and the only maps set now are the authored `_p` and the game's own slot-3 height
maps (#2979; nvf-assets `authored.rs`, `nif_spawn.rs`). Keep it while NVForever's height maps
are heights; drop it if they are ever converted to depth on the CPU instead.

A third change in the same file (#2979), in `parallaxed_uv`, marked `NVForever grazing
clamp`: the step's divisor is `max(view_steepness, 0.15)` instead of `view_steepness`, so a
shallow view no longer throws the samples several texture repeats away and a view along the
surface no longer gives non-finite coordinates (the full-depth shift stays under about 6.6 x
the depth scale). The layer count still mixes by the unclamped steepness. nvf-assets
`authored::Parallax::full_depth_shift` mirrors the step on the CPU and its test checks this line. Drop
it when upstream bounds the division. It applies to every height map, the authored `_p` too: an
intentional change to authored materials (at |Vt.z| = 0.05 a floor's step is a third of
upstream's), kept for both so no view gives runaway or non-finite coordinates.

Two guards in the same file (#2979): `sample_depth_map` holds the sampled height to 0..1
before the inversion (`1.0 - clamp(r, 0.0, 1.0)`, comment `NVForever height range`), so a
signed texture read through the linear loader cannot leave the march's layers; and the
occlusion refinement (comment `NVForever occlusion guard`) interpolates only when the march
brackets a hit (last layer at or below the surface, the one before above it, a non-zero
denominator), keeping the last layer's coordinates otherwise instead of extrapolating or
dividing by zero. The relief refinement is unchanged.

Another change (fog parameters, #2993, docs/design/graphics-enhanced.md §3.18), each part
marked `NVForever fog parameters`, for nvforever's `aerial.rs`:

- `src/fog.rs`: `DistanceFog::nvf_params`, three `Vec4`s, zero by default.
- `src/render/fog.rs`: `GpuFog::nvf_params`, copied from the component in `prepare_fog` for
  every fog mode (the Atmospheric arm now ends in `..Default::default()` like the others).
- `src/render/mesh_view_types.wesl`: `Fog::nvf_params: array<vec4<f32>, 3>` after `mode`
  (offset 64, the struct 112 bytes; nvforever's `aerial.rs` tests the Rust and the WGSL
  layout).

Bevy's fog functions never read the new field, so with it zero (Bevy's default, and every
NVForever view while the Aerial perspective row is off) the fog is upstream's. The fog
uniform grows by 48 bytes; the mesh view bind group keeps its entries. nvforever's own edits
of `fog.wesl` and `pbr_functions.wesl` (fog.rs, haze.rs, aerial.rs, at run time) read it.
Upstream has no way for an application to hand its own values to the fog shader (the
uniform's free fields were used up by the desert haze, §3.16).

The third (graphics pass 2026-11-23), marked `NVForever small casters`, in
`src/lib.rs` `check_dir_light_mesh_visibility`: a caster whose bounding radius (box half
diagonal times the largest scale) is below `nvf_small_caster_texels()` texels of a
cascade's map (`Cascade::texel_size`) is left out of that cascade; `NVF_SMALL_CASTER_TEXELS`
(default 1, 0 off). Meshes with `NoFrustumCulling` are never left out.

## bevy_render 0.20.1 (MIT OR Apache-2.0, Bevy contributors)

[documented: `src/render_resource/pipeline_cache.rs::create_pipeline_task`, #2317]
`RENDER_DEVICE_API_GATE` gates pipeline creation on task threads (and synchronous creation).
Each compilation holds its read side. NVForever's device poll thread shares it;
`gpu_poll::quiet` takes its write side during DLSS-G's live load/unload, so no pipeline
compile overlaps `slSetFeatureLoaded` (`sl_core_api.h:130-131` forbids parallel Vulkan calls).
The guard is taken inside the task, covering even a compile whose cache entry was replaced
by shader reload. `PipelineTaskOutput` also takes the read side when destroying an unclaimed
output: async-task can discard a cancelled task's pipeline after its compilation guard was
released. Outputs taken by the render thread are stored as upstream does. A switch waits
without a time limit for running driver compiles: they cannot safely be cancelled, so a
user-requested switch must wait to preserve device safety. A GPU-free unit test checks that
discarded outputs are destroyed while the gate excludes writers. Drop this patch if Bevy
supplies an equivalent device-operation gate.

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

Merged scene unpacking (with bevy_pbr's of that name above), in
`batching/gpu_preprocessing.rs`: each view and phase keeps one `SceneUnpackingClassBuffers`
for indexed and one for non-indexed meshes. `write_binned_instance_buffers` packs every
multi-drawn batch set's three CPU-written buffers into them each frame (the idea of
bevy#26580, open upstream, which packs them into shared buffers bound per batch set; that
PR does not apply over #2064 and binds ranges, where the merged dispatches need one
binding), with a `GpuSceneUnpackingJob` per batch set and the workgroup-to-job tables, and
writes each packed buffer with `write_buffer_if_changed`. The batch sets' own buffers are
CPU staging only and no longer uploaded (no GPU buffer, no write per batch set), and their
fan buffers are not allocated. The uniform metadata buffers with a dynamic offset per batch
set stay empty.

Gated render assets (#2661), in `src/erased_render_asset.rs`, marked `NVForever gated
materials`: `ErasedRenderAssetPlugin::gated(Arc<AtomicBool>)` runs the asset type's
extraction and preparation systems only while the flag is set, for bevy_pbr's gated
materials above. `Default` has no flag and does what upstream does.

Slab accounting (docs/design/streaming-hitches.md §3.2), in `src/material_bind_groups.rs`,
each part marked by a comment starting `NVForever slab accounting`. A bindless slab's
binding arrays hold each resource once, with a reference count, and `free` gives a slot
back only when the last reference goes. Upstream's `check_allocation` and
`insert_resources` count a resource the slab lacks once for every bindless index of the
material that binds it, so a material that binds one resource at several indices takes
more slots than it gives back and a slab's count only rises as such materials come and
go: the slab refuses materials while nearly empty, and the slabs multiply. NVForever's
mesh blend material binds the fallback image at each unused palette index (up to 400,
nvf-assets `nif_surface_blend.rs`) and remakes its clones at every cell crossing. Now:

- a missing resource is counted once per material (`check_allocation`) and a slot is taken
  only when the resource was not in the array (`insert_resources`), so the count is the
  number of resources the arrays hold;
- `check_allocation` returns as soon as the missing resources exceed the slab's free
  slots, the count `try_allocate` refuses the slab for, instead of looking up the rest of
  the material's resources in a slab that cannot take it (`allocate_unprepared` tries
  every slab in turn).

Which slab a material goes to can differ from upstream's choice (slabs fill properly);
what a slab binds for it does not. Two GPU-free unit tests (`tests::slab_accounting`, the
noop backend); the vendored crate is not a workspace member, so they run from a copy of it
outside the workspace (`cargo test --release --lib material_bind_groups`). Drop the patch
when Bevy counts distinct resources.

[observed 2026-11-20, `tools/stress run tour --towns Goodsprings Primm --step 1024`, a
player's NVForever.ini (Very High, mesh blend on, DLSS Quality at 3840x2160), the `profile`
build with the two changes behind a switch, one run each, Ryzen 9 9950X3D and RTX 5090]
`NifSurfaceBlend` slabs during the 150 s tour: 6 rising to 82 before, 1 throughout after.
Frames of 20 ms or more: 176 of 16,433 before, 79 of 16,961 after;
`prepare_erased_assets` was the largest span of 98 of them before (longest 33.0 ms, the
allocation 1,365 ms of their 1,730 ms) and of 8 after (longest 4.4 ms). Mean frame 8.62 →
8.36 ms, 1 % low 25.5 → 22.5 ms.

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

Window changes held for one frame (2026-11-13, the DLSS Frame Generation spike, #2317), in
`src/view/window/mod.rs`, each part marked `NVForever (2026-11-13)`:

- `HoldSurfaceReconfigure`, a render-world resource (default `false`). While `true`,
  `create_surfaces` does not reconfigure a window whose size or present mode changed or whose
  surface is outdated: it marks the surface outdated (the 2026-11-09 mark) and reconfigures it
  on the first frame after the hold is released; `prepare_windows` gives a window whose image
  comes back `Outdated` no image that frame (marked the same way) instead of configuring the
  surface there.
- `SurfaceData::reconfigure_pending`: whether `create_surfaces` would reconfigure the surface
  this frame, for the system that decides the hold.
- `extract_windows` is public, so a system can be ordered around it (DLSS Frame Generation sets
  its mode off before a closing window's surface is removed there, and switches DLSS-G on or
  off after it).

NVIDIA's DLSS Frame Generation must be off (its mode set off and presented once) before the
swapchain changes [documented: Streamline 2.14.16 `ProgrammingGuideDLSS_G.md` §12.0, §17],
and upstream reconfigures in the frame the change is extracted. NVForever's
`engine/nvforever/src/dlss/frame_gen.rs` holds the reconfigure for that one frame. Nothing
else sets it, so every other run takes upstream's path.

Vulkan library for the instance (2026-11-12, DLSS spike, #2198), in
`src/renderer/raw_vulkan_init.rs`, each part marked by a comment starting `NVForever
(2026-11-12, DLSS spike #2198)`, and an optional `ash` 0.38 dependency (the version wgpu-hal
30 uses) under the `raw_vulkan_init` feature in `Cargo.toml`:

- `RawVulkanInitSettings::set_vulkan_library` (unsafe): the raw Vulkan instance is made from
  that library (`ash::Entry::load_from`) instead of the system's loader, for NVIDIA
  Streamline's interposer. wgpu-hal 30.0.1's `Instance::init_with_callback` loads the loader
  itself, so its steps are done here with wgpu-hal's public `desired_extensions` and
  `from_raw`: the same application info, extensions, validation layer and features, and the
  loader's API version. Instance callbacks cannot run on that path (their argument type has a
  private field), so with any registered it is not taken. wgpu-hal's debug messenger cannot be
  handed over (private fields), so when the instance flags ask for `VK_EXT_debug_utils` the
  file's own messenger writes the layers' messages as wgpu-hal does (same log target, levels,
  wording and suppressed ids) and is destroyed before the instance. On success
  `VulkanLibraryInstance` is set in `AdditionalVulkanFeatures`; on any failure the caller's
  report hears why and upstream's path runs.

Without `set_vulkan_library`, upstream's path runs unchanged. NVForever calls it only once
its DLSS loader has passed every check (on Windows, with Vulkan among the renderer's
backends). The loader runs when DLSS is chosen, when the four DLSS files are in the DLSS
folder, or with `NVF_DLSS_PRELOAD=1`; `NVF_DLSS_PRELOAD=0` leaves out the files case
(`engine/nvforever/src/upscaler.rs` `UpscalerStartup`, docs/design/dlss-upscaling.md, "Loaded
whenever the files are there"). Every other run takes upstream's path. Drop it if wgpu-hal gains a way to pass the entry in.

Kept shadow phases (2026-11-19, the bevy_pbr "kept shadow phases" above), in
`src/render_phase/mod.rs`, marked `NVForever kept shadow phases`: `BinnedRenderPhase` gains
`nvf_is_binned` (whether an entity is binned) and `nvf_retain_binned` (removes the binned
entities a test refuses, as `remove` does), so a kept phase can be resynced with its light's
visible list and an entity never binned twice. `CachedBinnedEntity` gains
`nvf_input_uniform_index`, the index `add` binned the entity with, which the test is given
(2026-11-25: the stale entries of a kept phase, bevy_pbr above).

Pipeline compiling (perf/pipeline-cache, 2026-11-19; nvforever
`src/pipeline_disk_cache.rs` has the measurements), in `src/render_resource/pipeline_cache.rs`,
each part marked by a comment starting `NVForever`:

- `NVForever pipeline timing`: with `NVF_PLAY_TEST_PIPELINE_LOG` set, each pipeline logs
  `pipeline timing: queued .. ms, lock .. ms, compose .. ms, naga .. ms (<n> modules), create
  .. ms: <label>`: the wait for a thread, for the shader cache's lock, the Wesl composition,
  the part of it in `create_shader_module`, and the pipeline's creation (the driver's compile).
- `NVForever persistent pipeline cache`: `PipelineCache::set_wgpu_pipeline_cache` gives every
  pipeline created from then on a `wgpu::PipelineCache` (`cache:` in the descriptors, `None`
  upstream); nvforever loads it from the home and saves it.
- `NVForever composed shaders`: `PipelineCache::set_composed_store` hands bevy_shader's store
  (below) to the shader cache.
- `NVForever compile pool`: pipelines compile on a pool of their own (a quarter of the logical
  cores, 2 to 8 threads; `NVF_PIPELINE_THREADS=<n>`, 0 for upstream's async compute pool
  and no list pool),
  not on the async compute pool (at most 4 threads, shared with the game's loading). On a
  driver cache that has none of the pipelines (each 0.6-0.8 s), a Novac start's screen waited
  6.1 s for them on the async compute pool and 5.3 s on this pool.
- `NVForever modules off the lock`: the pipeline tasks take their shader modules from
  bevy_shader's `ShaderCache::prepare` (below) under the lock, then make the ones left to them
  without it (`nvf_modules`: naga's parse, validation and SPIR-V, and `vkCreateShaderModule`)
  and hand them back with `finish`; a module another thread is making is waited for, after
  this thread's own are made. Upstream made every module under the one lock: a Novac start's
  245 modules took 751 ms one at a time while the compile threads waited 3.9 s in all for
  the lock [observed 2026-11-19]. This needs wgpu's error scopes per thread, so the
  dependency on `wgpu` adds its `std` feature (without it the scopes are one stack for all
  threads, and two threads making modules at once panic with a mismatched
  `pop_error_scope`).
- `NVForever pipeline list`: a new module `src/render_resource/nvf_manifest.rs`
  (`PipelineManifest`, its module notes say how it works) and `PipelineCache::
  set_pipeline_manifest`. The render and compute tasks turn their descriptor into a recipe
  (shaders by `Shader::path`) and ask the list: a pipeline the list built from the same
  modules is handed over, else the task builds it and the list keeps the recipe.
  `process_queue` feeds the list's recipes whose shaders have loaded (only those asked in
  the last `REBUILD_RUNS` runs, at most `MAX_REBUILT`) to a pool of its own
  (`nvf_list_pool`, half the compile pool's default threads), which builds each inside a
  validation error scope. For the JSON of the recipes the crate depends on `serde` and
  `serde_json` and on `wgpu-types` with `serde` (all already in the build).

Without the setters and with `NVF_PIPELINE_THREADS=0`, upstream's path runs (no list pool
either), except that the modules are made outside the lock.

Graphics pass 2026-11-23: bevy#26239 (open upstream), `src/view/mod.rs`
(`ExtractedView::main_pass_viewport`) and `src/occlusion_culling/mod.rs`, with
bevy_core_pipeline below.

HDR output (docs/design/hdr-output.md), marked `NVForever HDR output` in
`src/view/window/mod.rs`: the statics `NVF_HDR_SURFACE` (asked for), `NVF_HDR_SURFACE_ACTIVE`
(the window's surface is fp16 scRGB) and `NVF_HDR_SURFACE_DECIDED` (a surface has been
made). When the surface is made with HDR asked for (`NVF_HDR_SURFACE` or `NVF_HDR=1`) and
it offers `Rgba16Float`, that format is chosen; `SurfaceColorSpace::Auto` makes it scRGB.
Upstream prefers an sRGB format. `SurfaceData` keeps the SDR format and whether fp16 is
offered; `nvf_hdr_changed` compares the asked-for output with the configured format each
frame, and a change reconfigures the surface at once (no restart). Test aid
`NVF_PLAY_TEST_SDR_SURFACE=1`: the surface counts as offering no `Rgba16Float`.

## bevy_shader 0.20.1 (MIT OR Apache-2.0, Bevy contributors)

The shader cache, for composed shaders kept between runs (perf/pipeline-cache, 2026-11-19).
A new module `src/nvf_composed.rs` (`ComposedStore`: keyed compositions with the source
hash of each module the composer read, a file format with a producer line, pruning of
entries unused for 30 runs, at most 96 MB, up to 4 variants a key) and, in
`src/shader_cache.rs`, each part marked by a comment starting `NVForever composed shaders`:
`ShaderCache::set_composed_store`, each shader's source hash and each module path's shader
(kept up to date in `set_shader` and `remove`), and in `get`'s Wesl branch a lookup before
`wesl::compile` and a `put` after it. The key is the module path, the composer's options
with its sorted definitions, and the `constants` module's source; an entry is used only
while every module it read still has the same source. Without a store `get` composes as
upstream does.

Why: once the driver has a pipeline in its own cache, composing its shaders is most of what
it costs (15-90 ms a module, one at a time under the cache's lock, against about 2 ms to
make the module), and the composer orders a module's declarations differently from run to
run, which hands the driver new SPIR-V. At a Novac start, with NVIDIA's cache warm, the
screen's wait for pipelines went from 3.8 to 2.3 s and the 45 pipelines asked in the first
seconds after it from 6.3 s to 1.2 s (asked to ready, all together) [observed, nvforever
`pipeline_disk_cache.rs`]. Composing on several threads instead was tried and is slower
(wesl 0.6's identifiers share reference counts). The test `composed_store_skips_compositions_of_unchanged_sources`
checks a second cache composes nothing the first kept and composes again after a change.
Drop it when Bevy caches compositions itself.

`NVForever modules off the lock` (`src/shader_cache.rs`): `get` is split into `prepare`
(compose under the lock; returns `ModuleStep::Ready`, `Build` with the composed source, or
`Wait` on the `ModuleSlot` of the thread making the same module) and `finish` (keep the
module made, unless the shader was replaced meanwhile: each shader's data has a
`generation` that `clear` bumps, and `clear` drops the slots being made). A `ModuleBuild`
dropped unfinished (its maker panicked or stopped early) fills its slot with
`ShaderImportNotYetAvailable`, an error the pipeline cache retries, so a waiter's pipeline
is queued again rather than failed for the run. `get` is
`prepare`, the module made holding the lock, and `finish`, as before. `ShaderCacheError` is
`Clone` for the waiters. For the pipeline list (bevy_render above): `shader_path`,
`shader_by_path` (a path two shaders share is neither) and `shaders_set`, and `prepare`
takes the pipeline as an `Option` (`None` for the list's builds, which are not compiled
again when a shader changes). Tests: `a_module_asked_twice_at_once_is_made_once_and_a_stale_one_is_not_kept`,
`a_build_dropped_unfinished_wakes_its_waiters`, `shaders_are_found_by_a_path_only_they_have`.

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
- `vulkan::swapchains_made` (new, public; marked `NVForever (2026-11-16)`): a count of the
  swapchains `create_swapchain` has made in the process, on any device, moved on after
  `vkCreateSwapchainKHR` returned. Streamline's Reflex sets its sleep mode and frame limit on
  the swapchain it tracks at the call, and a new swapchain starts without them, so the
  engine sets Reflex's options again when the count moves (`nvforever/src/dlss/frame_token.rs`,
  LootJunkieAzza/NVForever#2732).

Not run on a GPU yet (2026-11-12): built, and the engine's tests and clippy pass. The copy
is the crates.io package as published (its `examples` are kept: the manifest names them).
Drop it when wgpu lets a caller extend swapchain creation, or creates the swapchain with
the structure itself.

Test aid (marked `NVForever (2026-11-19)`, `src/vulkan/mod.rs` `spirv_salt` and
`src/vulkan/device.rs` `create_shader_module_impl`): `NVF_PLAY_TEST_SPIRV_SALT=<n>` XORs `n`
into each shader module's SPIR-V generator word, which names only the tool that wrote it, so
a driver's own shader cache sees new code: a first launch after a driver update, on demand,
for measuring (perf/pipeline-cache). Without it nothing changes.

## naga 30.0.1 (MIT OR Apache-2.0, the gfx-rs developers)

wgpu's shader translator, for the Vulkan SPIR-V of `@builtin(sample_mask)` (#2413). Vulkan
requires the `SampleMask` built-in to be an array of 32-bit integers
[documented: Vulkan spec, "Built-In Variables", VUID-SampleMask-SampleMask-04359]; naga's
SPIR-V back end declares it as a scalar `u32`, as WGSL writes it (open upstream as
gfx-rs/wgpu#7311). The engine's prepass coverage (`nvf-assets/src/nif_material.rs`
`prepass_coverage`, PR #2181) returns that built-in from the depth prepass. One change,
each part marked by a comment starting `NVForever`:

- `src/back/spv/mod.rs`: `Writer::sample_mask_vars`, the ids of the `SampleMask` varyings.
- `src/back/spv/writer.rs`: `write_varying` declares a `SampleMask` input or output as
  `OpTypeArray` of `u32` with length 1; `load_io_with_f16_polyfill` and
  `store_io_with_f16_polyfill` load and store it through `sample_mask_element`, an
  `OpAccessChain` to element 0. The WGSL side (`u32`) and every other varying are as
  upstream.

Why: on an Intel N150 (driver 32.0.101.7088, Vulkan) the game crashed in the driver's
shader compiler (`igc64.dll`) while building `pbr_prepass_pipeline` with MSAA on (the
default `--msaa 4`), 5 runs of 5 [observed]. Upstream naga's output fails `spirv-val` with
VUID 04359 and the patched output passes [observed]; on the N150 the game then ran 60 s at
`--msaa 4` with prepass coverage on, all 25 `pbr_prepass_pipeline` builds done
[observed, 2026-11-13]. The coverage is worth keeping there: the opaque pass took 40.3 ms
with it and 48.3 ms without, 1080p 4x MSAA [observed]. `nvf-assets/tests/sample_mask_spirv.rs`
checks the declaration. The copy is the crates.io package as published; its own
`.gitignore` lists `Cargo.lock`, which is kept here all the same, as for the other copies.
Drop it when a naga release declares `SampleMask` as an array (gfx-rs/wgpu#7311 fixed).

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

## winit 0.30.13 (Apache-2.0; rust-windowing contributors)

Android controller admission only, marked `NVForever controller admission` in
`src/platform/android.rs` and `src/platform_impl/android/mod.rs`: an optional
application handler runs before the single native queue consumer translates input into
window events. Unhandled inputs use the upstream path. NVForever copies controller
samples into its own bounded queue, then uses Bevy's ordinary RawGamepadEvent route.
No other platform backend changes. The upstream crate archive checksum was matched to
Cargo.lock and its source files matched the installed registry before copying; upstream
licences remain beside the source. Drop this patch when winit exposes equivalent native
controller admission or provides a complete controller-event backend.

## rapier3d 0.36.1 (Apache-2.0, Dimforge)

Vendored at the repository root (`vendor/rapier3d/`) and wired via `[patch.crates-io]`
in both `Cargo.toml` and `engine/Cargo.toml` so both workspaces resolve the exact same
physics crate. Carries two bounded zero-dt collision-query refresh fixes in
`src/pipeline/physics_pipeline/substep.rs` (deferred BVH optimization join before
`dt <= 0.0` return) and `src/dynamics/island_manager/manager.rs`
(`commit_sleeping_chunks` after persistent sleep bootstrap). Full baseline SHA-256,
rationale, and test notes are in `vendor/rapier3d/README.nvforever.md`.
