//! DLSS Frame Generation (DLSS-G) through Streamline (#2317; docs/design/dlss-frame-generation.md).
//! Not FSR's frame generation (`crate::frame_gen`), which never starts while DLSS-G is loaded.
//!
//! - **What runs** while DLSS-G and Reflex were set up (`win.rs`): the Frame generation row
//!   asks for DLSS ([`follow_request`], from `crate::frame_gen::DlssgRequest`) and DLSS-G is
//!   loaded; frames are generated in play, `requested_frames` per real frame (the Generated
//!   frames row, at most what the state allows):
//!   - [`set_constants`] (before the frame is recorded): the common constants once a frame for
//!     whichever upscaler draws (FSR or DLSS; `evaluate.rs` `constants_for`), with our frame
//!     number's token (`frame_token.rs`); DLSS's evaluate then sets none of its own.
//!   - [`tag`] (after the post passes, before the UI, where FSR's frame generation copies its
//!     frame without the HUD, `crate::frame_gen` `capture_hudless`): on a frame that may be
//!     generated ([`may_generate`]) the frame without the HUD is written, sRGB-encoded, into a
//!     display-sized `Rgba8Unorm` texture of its own (two used in turn; [`ENCODED_FORMAT`]: the
//!     back buffer's colour space, as the guide asks), the upscaler's depth (`PreparedUpscale::depth`,
//!     render size), the prepass motion (its render rectangle) and that copy are moved to
//!     `RESOURCE` on Bevy's encoder and tagged `eValidUntilPresent` with `slSetTagForFrame`,
//!     each `Resource` reporting the layout wgpu-hal puts `RESOURCE` in, as the SR evaluate does
//!     (`DEPTH_STENCIL_READ_ONLY_OPTIMAL` for the depth, `SHADER_READ_ONLY_OPTIMAL` for the
//!     others [design: wgpu-hal 30.0.1 `vulkan/conv.rs:240-260`, cited in `evaluate.rs`]).
//!     Nothing after the UI and Bevy's upscaling blit touches them before the present. Other
//!     frames tag all three as null [documented: `ProgrammingGuideDLSS_G.md` §5.2, null tags in
//!     menus and loading].
//!   - [`options`] (an exclusive system at the end of the render graph, so on the render thread
//!     just before Bevy's `render_system` presents): `slDLSSGSetOptions` only when what it
//!     should be changes: `eOn` from the first tagged play frame, `eOff` (resources kept) on a
//!     frame that is not, the sizes (depth and motion at the render size, colour at the
//!     output size), the back buffers left to DLSS-G (0). A call that fails is
//!     made again next frame. The options take effect at the next present, deterministically
//!     only when set on the presenting thread [documented: `ProgrammingGuideDLSS_G.md` §6.0].
//!   - [`state`] (an exclusive system right after `render_system`, the thread that presented):
//!     `slDLSSGGetState` after every present (null options: no VRAM estimate, which is
//!     expensive [documented: §13, §15]); `numFramesActuallyPresented` is the frames presented
//!     for that real frame [documented: §13], the generated ones added to
//!     `crate::frame_gen::GeneratedFrames` (the performance overlay's count); a line every ten
//!     seconds with real and presented frames; a status other than `eOk` while on turns frame
//!     generation off for the session, said once with the flags' names [documented: §14.3].
//! - **Queue parallelism** (2026-11-23): the options ask for `eBlockNoClientQueues`, so DLSS-G no
//!   longer holds the game's queue (wgpu's one queue, which presents) until its whole workload
//!   is done; the next frame's GPU work can run while DLSS-G still generates on its own queues
//!   [documented: Streamline v2.14.16 `sl_dlss_g.h:55-66` `DLSSGQueueParallelismMode`;
//!   `ProgrammingGuideDLSS_G.md` (Version 2.14.16) §17.0, `:963-967`: "may offer extra
//!   performance gains depending on the workload ... most apparent in GPU-limited
//!   applications"; `docs/spec-notes/streamline.md`]. In
//!   exchange the game must not touch DLSS-G's inputs (the depth, motion, HUD-less and UI buffer
//!   it tagged) until DLSS-G has read them: [`state`] keeps `inputsProcessingCompletionFence` and
//!   `lastPresentInputsProcessingCompletionFenceValue` after a present with the mode on, and
//!   [`wait_for_inputs`] (exclusive, first in the next frame's `Render`, on the render thread,
//!   before any of the frame's submits) stages a wait on it for the queue's next submit
//!   (wgpu-hal's `Queue::add_wait_semaphore`, all commands), which every later submit follows
//!   (wgpu-hal orders its submits with relay semaphores). The object is a timeline `VkSemaphore`
//!   waited on the GPU, as NVIDIA's sample does it: `QueueGPUWaitOnSyncObjectSet` queues
//!   `queueWaitForSemaphore(Graphics, reinterpret_cast<VkSemaphore>(syncObj), syncObjVal)` on
//!   Vulkan, called at the start of `RenderScene` "before any of the inputs are modified in the
//!   subsequent command list submission", when DLSS-G was on last frame [documented:
//!   NVIDIA-RTX/Streamline_Sample tag v2.14.16 (94da85b65) `src/SLWrapper.cpp:250-279`,
//!   `src/StreamlineSample.cpp:828-836`; the Streamline headers and guide do not name the
//!   Vulkan type]. A wait no submit took (a frame
//!   with nothing submitted) is taken back before the next one is staged, and when DLSS-G is
//!   loaded or unloaded. `NVF_DLSSG_QUEUE=block` keeps the default mode, for comparisons.
//!   GPU-bound at 4x it measured +6% real frames, latency the same [observed: RTX 5090, driver
//!   621.24, design doc "Queue parallelism"].
//! - **UI buffer** ([`ui_buffer`], stage 4 item 2; after the UI, before Bevy's upscaling blit,
//!   where FSR's frame generation's `generate` sits): on a tagged frame the UI is drawn a second
//!   time, alone, over transparent black (`fg_ui_layer.rs`, the main texture's format), which
//!   gives the UI's coverage (its alpha); a full-screen pass ([`UI_COLOUR_WGSL`]) turns it, the
//!   frame and the HUD-less copy into the UI colour and alpha in the back buffer's encoded values
//!   (`Rgba8Unorm`, two used in turn), its colour premultiplied and chosen so DLSS-G's blend gives
//!   back the real frame, and tags it `kBufferTypeUIColorAndAlpha` `eValidUntilPresent`
//!   [documented: `ProgrammingGuideDLSS_G.md` §5.1: "the RGB channels must be pre-multiplied by
//!   alpha", `Final_Color.RGB = UI.RGB + (1 - UI.Alpha) x Hudless.RGB`]: see-through HUD pixels
//!   keep their own alpha. With
//!   `NVF_FG_UI_LAYER=0`, as before, a full-screen pass writes
//!   a display-sized `R16Float` UI alpha from the frame (with the UI) and its HUD-less copy: 1
//!   where they differ, 0 elsewhere. With alpha 1 the UI's colour is the frame's, so the guide's
//!   rules hold: 0 where there is no UI, above 0 where there is, and `Final = UI.RGB + (1 -
//!   UI.A) * Hudless` [documented: `ProgrammingGuideDLSS_G.md` §5.1, "UI Alpha OR UI Color and
//!   Alpha"; UI alpha is the preferred, faster one], at the cost of taking a see-through HUD
//!   pixel as opaque UI (AMD's HUD-less mode in FSR's frame generation makes the same choice,
//!   `crate::frame_gen`). It is tagged `kBufferTypeUIAlpha` `eValidUntilPresent`, and the
//!   options ask for user interface recomposition, which needs the HUD-less and the UI alpha
//!   ("If both Hudless and UI alpha are tagged, you can also enable user interface
//!   recomposition", §5.1; §6.6). Not asked for: `eEnableFullscreenMenuDetection` (§6.5, which
//!   needs `kBufferTypeUIColorAndAlpha`): the mode goes off in every menu, loading and pause
//!   already (`may_generate`). While DLSS-G is unloaded (the live toggle) the HUD-less and UI
//!   textures are freed.
//! - **Window changes** ([`window_changes`], stage 4, an exclusive system before Bevy's
//!   `create_surfaces`): DLSS-G must be off before the swapchain is resized or its present mode
//!   changes [documented: `ProgrammingGuideDLSS_G.md` §12.0, §17]. When a window change is
//!   pending (`SurfaceData::reconfigure_pending`) while DLSS-G was on at the last present, the
//!   surface's reconfigure is held (the vendored `HoldSurfaceReconfigure`), the mode set off
//!   for this frame and presented once; on the next frame, with the last present off, the hold
//!   is released and Bevy reconfigures with the mode still off; frame generation comes back on
//!   the frame after. At most [`MAX_HOLD`] frames are held (a window that is never presented,
//!   minimised, would hold it for ever).
//!   What this meets of "off before any window manipulation" (§17): the OS window has already
//!   changed when the change reaches the render world (winit resized it, Bevy extracted the new
//!   size), so DLSS-G is still on while the OS resizes, maximises or changes the display mode;
//!   what the hold covers is the swapchain's reconfigure, which happens only after a frame was
//!   presented with the mode off. A hold whose image comes back `Outdated` presents nothing;
//!   no present since the mode went off counts as off ([`state`]).
//! - **On and off live** (the Frame generation row, [`follow_request`]): "the swap chain must be torn down and recreated every time DLSS-G is switched on
//!   or off" [documented: §18.0]. Off: the mode set off and presented once; then, in the next
//!   frame's extraction ([`toggle_switch`]: the main thread is extracting and the render thread
//!   waits, so neither calls Vulkan meanwhile, and the main thread's Reflex calls come after the
//!   extraction, `anti_lag.rs`), the GPU drained, the window's image dropped and its surface
//!   released (Bevy's `SurfaceData` taken away; wgpu waits for the device and destroys the
//!   swapchain and the surface when the last reference goes, wgpu-hal
//!   `Swapchain::release_resources`), DLSS-G's resources freed (`slFreeResources`, as for a
//!   long-term off, §6.4), `slSetFeatureLoaded(DLSS-G, false)`, and Bevy's `create_surfaces`
//!   makes the surface and swapchain again that frame, now native. On: the same with `true` and
//!   nothing to free, so the new swapchain is Streamline's proxy. A reconfigure alone would not
//!   do: wgpu makes the new swapchain before it destroys the old one. Device polling and Bevy's
//!   pipeline creation on task threads take the same gate's read side; the switch holds its
//!   write side (`gpu_poll::quiet`) through the drain, surface release and feature load call
//!   [documented: vendored `pipeline_cache.rs::create_pipeline_task`; `sl_core_api.h:130-131`
//!   forbids parallel Vulkan calls during `slSetFeatureLoaded`].
//!   While DLSS-G is unloaded frame generation does not run (`super::frame_generation_running`):
//!   vsync and the frame cap are frame pacing's again.
//! - **Exit** ([`exit_watch`], in the extraction before Bevy's `extract_windows`): on `AppExit`
//!   the mode is set off for the frame being extracted, which is still presented; on a window
//!   closing the mode is set off at once (`extract_windows` queues the surface's removal, which
//!   Bevy applies at the render world's `ExtractCommands`, before that frame renders). Both
//!   then reach `win.rs` `shutdown` from `main` once `App::run` has returned.
//!   The order of the teardown, whatever ends the run:
//!   1. the mode off ([`exit_mode_off`], once a run; the guide's way of disabling frame
//!      generation, `mode` `eOff` [documented: `ProgrammingGuideDLSS_G.md` §6.4]) and the
//!      window's swapchain released while it is off ("Turn DLSS-G off ... before any window
//!      manipulation", §17; the swapchain released after the GPU is idle, §18.0's pseudocode);
//!   2. the tags set null, the GPU drained and DLSS-G's resources freed with `slFreeResources`
//!      ([documented: `ProgrammingGuide.md` §2.8 and the null tag before a resource goes,
//!      `ProgrammingGuide.md:1017-1025`; `ProgrammingGuideDLSS_G.md` §6.4 step 2]);
//!   3. `slShutdown`, "before destroying ... Vulkan instances, devices" [documented:
//!      `ProgrammingGuideDLSS_G.md` §2.0; `ProgrammingGuide.md` §2.15], which unloads every
//!      plugin (each one's `onShutdown`, then `FreeLibrary`) [documented: Streamline's source,
//!      `src/sl.cpp:289-327` `slShutdown`, `src/pluginManager.cpp:1200-1229` `unloadPlugins`].
//!
//!   A loaded save's handoff ends the old run without `App::run` returning: `handoff.rs`
//!   `end_now` calls `std::process::exit` from the `EndNow` schedule after `Last`, so neither
//!   this frame's extraction (where [`exit_watch`] would see its `AppExit`) nor `main`'s
//!   shutdown ran, and Streamline and DLSS-G were left to the DLLs' detach at process exit
//!   [design]. In a player's runs every such exit with DLSS-G set up crashed in
//!   `nvngx_dlssg.dll` on the main thread (#2733) [observed: the player's logs and dumps,
//!   2026-11-15]; that the detach without `slShutdown` is the cause is a [hypothesis] the fix
//!   tests. It now runs the same steps first (`super::end_now`): the render app taken back
//!   from the render thread, FSR frame generation's presenters stopped and joined (they
//!   present on threads of their own and hold the surface), then [`end_now`] here (step 1, the
//!   mode off at once as for a closing window; the handoff's loading screen keeps the options
//!   off before, `may_generate` needs play [design]), then `win.rs` `shutdown` (steps 2 and
//!   3); the process ends after.
//! - **Errors**: with DLSS-G loaded, `vkQueuePresentKHR` and `vkAcquireNextImageKHR` run
//!   asynchronously inside Streamline and the host sees only "the last known error"; the
//!   callback reports each call whose `VkResult` is not `VK_SUCCESS` and must return at once
//!   [documented: §6.7; `sl_core_types.h:516-517`]. It stores the value in atomics: errors
//!   (negative), the swapchain out of date or suboptimal (`VK_ERROR_OUT_OF_DATE_KHR`,
//!   `VK_SUBOPTIMAL_KHR`: a resize, answered by a reconfigure) and other positive results apart.
//!
//! Every system here runs only while DLSS-G is installed ([`active`], a static flag, so without
//! DLSS-G and Reflex set up nothing is locked or called). The DLSS-G functions are "NOT thread safe"
//! (`sl_dlss_g.h:187-208`) and are called only from the exclusive systems, which run on the
//! render thread (exclusive systems run on the thread that runs the schedule, and Bevy's
//! `render_system`, itself exclusive, runs the render graph's schedule there).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use ash::vk;
use ash::vk::Handle;
use bevy::camera::{CameraMainTextureUsages, NormalizedRenderTarget};
use bevy::core_pipeline::prepass::{PreviousViewData, ViewPrepassTextures};
use bevy::core_pipeline::{upscaling::upscaling, Core3d, Core3dSystems};
use bevy::log::Level;
use bevy::prelude::*;
use bevy::render::camera::{ExtractedCamera, TemporalJitter};
use bevy::render::renderer::{render_system, RenderAdapter, RenderContext, RenderGraph, RenderGraphSystems, RenderQueue, ViewQuery};
use bevy::render::sync_world::MainEntity;
use bevy::render::view::{create_surfaces, extract_windows, prepare_view_targets, ExtractedView, ExtractedWindow, HoldSurfaceReconfigure, SurfaceData, ViewTarget};
use bevy::render::{Extract, ExtractSchedule, Render, RenderSystems};
use bevy::window::{PrimaryWindow, WindowClosing};
use bevy::ui_render::{ui_pass, UiCameraView};

use super::evaluate::{constants_for, describe, ConstantsInput, VIEWPORT};
use super::ffi::{
    boolean_name, dlssg_status_name, ApiError, BaseStructure, DlssgOptions, DlssgState, Extent, ResourceTag, SlDlssgGetState, SlDlssgSetOptions, SubresourceRange, ViewportHandle,
    BOOL_TRUE, BUFFER_DEPTH, BUFFER_HUDLESS_COLOR, DLSSG_QUEUE_BLOCK_NO_CLIENT_QUEUES, DLSSG_QUEUE_BLOCK_PRESENTING_CLIENT_QUEUE, BUFFER_MOTION_VECTORS, BUFFER_UI_ALPHA, BUFFER_UI_COLOR_AND_ALPHA, DLSSG_FLAG_RETAIN_RESOURCES_WHEN_OFF, DLSSG_MODE_OFF, DLSSG_MODE_ON, LIFECYCLE_VALID_UNTIL_PRESENT,
};
use super::{frame_token, note_fg};
use crate::upscaler::prep::{PreparedUpscale, UpscaleCamera};

/// How often the state line is said.
const STATE_EVERY: Duration = Duration::from_secs(1);
/// How often the real-versus-presented line is said (stage 2).
const RATIO_EVERY: Duration = Duration::from_secs(10);
/// Frames whose tagged textures and views are kept alive: wgpu frees a texture once its last
/// submission is done, but DLSS-G reads the tags at the present, on its own queue, possibly
/// later [design: a margin over Bevy's frames in flight].
const KEEP_FRAMES: usize = 4;
/// The most frames a window change is held for DLSS-G to be presented off first.
const MAX_HOLD: u32 = 3;

/// The queue parallelism the options ask for (the module comment's "Queue parallelism"):
/// `eBlockNoClientQueues`, or the default with `NVF_DLSSG_QUEUE=block` (for comparisons).
static QUEUE_MODE: LazyLock<u32> = LazyLock::new(|| {
    if std::env::var("NVF_DLSSG_QUEUE").is_ok_and(|v| v.trim().eq_ignore_ascii_case("block")) { DLSSG_QUEUE_BLOCK_PRESENTING_CLIENT_QUEUE } else { DLSSG_QUEUE_BLOCK_NO_CLIENT_QUEUES }
});

fn queue_mode_name(mode: u32) -> &'static str {
    if mode == DLSSG_QUEUE_BLOCK_NO_CLIENT_QUEUES { "eBlockNoClientQueues" } else { "eBlockPresentingClientQueue" }
}

/// The options as last set, to set them again only on a change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Applied {
    on: bool,
    frames: u32,
    render: UVec2,
    output: UVec2,
    back_buffers: u32,
}

/// What [`tag`] found for this frame, for [`options`].
#[derive(Clone, Copy, Debug)]
struct Decision {
    generate: bool,
    render: UVec2,
    output: UVec2,
}

/// The frame without the HUD, or the UI colour and alpha: two textures used in turn, at the
/// output size, in [`ENCODED_FORMAT`].
struct Hudless {
    textures: [wgpu::Texture; 2],
    views: [wgpu::TextureView; 2],
    next: usize,
}

impl Hudless {
    fn new(device: &wgpu::Device, size: wgpu::Extent3d, label: &'static str) -> Self {
        let textures: [wgpu::Texture; 2] = std::array::from_fn(|_| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: ENCODED_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        });
        let views = std::array::from_fn(|i| textures[i].create_view(&wgpu::TextureViewDescriptor::default()));
        Self { textures, views, next: 0 }
    }
}

/// Real and presented frames since the period began.
struct Period {
    since: Instant,
    real: u64,
    presented: u64,
}

impl Period {
    fn new(now: Instant) -> Self {
        Self { since: now, real: 0, presented: 0 }
    }
}

/// DLSS-G's functions and its state.
struct FrameGen {
    get_state: SlDlssgGetState,
    set_options: SlDlssgSetOptions,
    applied: Option<Applied>,
    /// Frames to generate per real frame, as the Generated frames row asks (before the state's
    /// limit).
    requested_frames: u32,
    /// The options and result of the last failed call, said once.
    failed_said: Option<(Applied, i32)>,
    /// A status other than `eOk` while on: frame generation off for the session.
    status_failed: Option<String>,
    /// From the last state: the most frames generatable, the least output dimension.
    max_frames: u32,
    min_size: u32,
    /// This frame's constants were set for this frame number ([`set_constants`]).
    constants: Option<u32>,
    decision: Option<Decision>,
    hudless: Option<Hudless>,
    kept: VecDeque<(Vec<wgpu::Texture>, Vec<wgpu::TextureView>)>,
    last_state_line: Option<Instant>,
    period: Option<Period>,
    /// Calls already said failing (once each).
    said: Vec<&'static str>,
    /// DLSS-G is loaded (`slSetFeatureLoaded`; switched by the live toggle).
    loaded: bool,
    /// The mode is held off this frame (a window change or a toggle under way).
    window_off: bool,
    /// The last present ran with the mode off (or DLSS-G unloaded) ([`state`]).
    presented_off: bool,
    /// Frames the pending window change has been held so far.
    hold_frames: u32,
    /// The switch asked for (the Frame generation row) and not done yet.
    toggle: Option<bool>,
    /// The last frame number tags were set for (null tags at shutdown).
    last_index: Option<u32>,
    /// The live toggle's switch, done in the next extraction ([`toggle_switch`]).
    pending_load: Option<bool>,
    /// The app is exiting: the mode stays off ([`exit_watch`]).
    exiting: bool,
    /// The mode was set off at once for an exit ([`exit_mode_off`]): once a run.
    exit_off: bool,
    /// This frame's HUD-less copy, for the UI buffer ([`ui_buffer`]); set by a tagged frame.
    ui_source: Option<wgpu::TextureView>,
    /// The UI buffer's pass and its two textures (`NVF_FG_UI_LAYER=0`: the UI alpha).
    ui: Option<UiPass>,
    /// The UI drawn alone (`fg_ui_layer.rs`, the main texture's format), read in the same frame.
    ui_layer: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// The UI colour and alpha tagged ([`ENCODED_FORMAT`], two used in turn) and its pass
    /// (the module comment's "UI buffer").
    ui_colour: Option<Hudless>,
    ui_colour_pass: Option<Fullscreen>,
    /// The HUD-less copy's encoding pass.
    hudless_pass: Option<Fullscreen>,
    /// The UI buffer was tagged once (said).
    ui_said: bool,
    /// The swapchain was just made again by the live toggle: the mode stays off one frame more
    /// ([`window_changes`]).
    just_switched: bool,
    /// `eBlockNoClientQueues`: DLSS-G's inputs fence (a timeline `VkSemaphore`) and the value it
    /// reaches once the last present's inputs are read, from [`state`] after a present with the
    /// mode on; for [`wait_for_inputs`].
    inputs_fence: Option<(u64, u64)>,
    /// The wait [`wait_for_inputs`] staged on the queue (the queue, the semaphore), taken back if
    /// no submit took it before the next is staged or DLSS-G is loaded or unloaded.
    staged_wait: Option<(wgpu::Queue, u64)>,
    /// The first staged wait was said.
    wait_said: bool,
}

/// Takes a staged wait back from the queue's next submit, if no submit took it yet.
fn unstage(queue: &wgpu::Queue, semaphore: u64) {
    // SAFETY: only reads wgpu-hal's queue to drop an entry from its pending wait list; no
    // Vulkan call is made.
    if let Some(hal) = unsafe { queue.as_hal::<wgpu::hal::api::Vulkan>() } {
        hal.remove_wait_semaphore(vk::Semaphore::from_raw(semaphore));
    }
}

/// The UI alpha's full-screen pass (`NVF_FG_UI_LAYER=0`) and its textures (two used in turn,
/// as the HUD-less).
struct UiPass {
    pass: Fullscreen,
    textures: Option<([wgpu::Texture; 2], [wgpu::TextureView; 2])>,
    next: usize,
}

/// A full-screen pass of DLSS-G's own: its inputs loaded at the pixel (bindings 0, 1, ...), one
/// target, no blending; the bind groups made once for each set of input views (the main
/// texture's two views, the copies used in turn), the last eight kept.
struct Fullscreen {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    groups: Vec<(Vec<wgpu::TextureView>, wgpu::BindGroup)>,
}

impl Fullscreen {
    /// `body` after [`COMMON_WGSL`] (the vertex stage and `encode`); `inputs` textures.
    fn new(device: &wgpu::Device, label: &'static str, body: &str, inputs: u32, format: wgpu::TextureFormat) -> Self {
        let source = format!("{COMMON_WGSL}{body}");
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(source.into()) });
        let entries: Vec<wgpu::BindGroupLayoutEntry> = (0..inputs)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                count: None,
            })
            .collect();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries: &entries });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(label), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vertex"), compilation_options: default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: default(),
            depth_stencil: None,
            multisample: default(),
            multiview_mask: None,
            cache: None,
        });
        Self { pipeline, layout, groups: Vec::new() }
    }

    /// Draws every pixel of `target` (load, not clear) from `inputs`.
    fn draw(&mut self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, inputs: &[&wgpu::TextureView], target: &wgpu::TextureView) {
        let group = match self.groups.iter().find(|(k, _)| k.iter().eq(inputs.iter().copied())) {
            Some((_, group)) => group.clone(),
            None => {
                let entries: Vec<wgpu::BindGroupEntry> =
                    inputs.iter().zip(0..).map(|(view, binding)| wgpu::BindGroupEntry { binding, resource: wgpu::BindingResource::TextureView(view) }).collect();
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("dlssg_fullscreen"), layout: &self.layout, entries: &entries });
                if self.groups.len() >= 8 {
                    self.groups.remove(0);
                }
                self.groups.push((inputs.iter().map(|v| (*v).clone()).collect(), group.clone()));
                group
            }
        };
        let mut render = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("dlssg_fullscreen"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        render.set_pipeline(&self.pipeline);
        render.set_bind_group(0, &group, &[]);
        render.draw(0..3, 0..1);
    }
}

/// The HUD-less copy's and the UI colour and alpha's format: the window's 8-bit kind holding
/// sRGB-encoded values, as the back buffer DLSS-G is given does. DLSS-G wants the HUD-less "same
/// color space ... as the color back buffer" and blends `UI.RGB + (1 - UI.Alpha) x Hudless.RGB`
/// in it, and does not take FP16 [documented: `ProgrammingGuideDLSS_G.md` §5.1 table, §11.0].
/// The window's image is a UNORM image drawn through an sRGB view (Streamline refuses sRGB
/// back buffers; vendored `bevy_render` `view/window/mod.rs` `formats`), so it holds encoded
/// values; the main texture is linear `Rgba16Float`, which a plain copy left linear: the
/// generated frames' HUD then sat on a background unlike the real frames' (the crosshair's
/// faint glow flickered [observed: #3595]). `VK_FORMAT_R8G8B8A8_UNORM` is in Streamline's table
/// [documented: `source/platforms/sl.chi/vulkan.cpp` `getFormat`].
const ENCODED_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The full-screen vertex stage and the sRGB encoding (IEC 61966-2-1), in every pass here.
const COMMON_WGSL: &str = "
@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn encode(c: vec3<f32>) -> vec3<f32> {
    let x = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3<f32>(0.0031308));
}
";

/// The HUD-less copy: the frame before the UI, sRGB-encoded ([`ENCODED_FORMAT`]).
const HUDLESS_WGSL: &str = "
@group(0) @binding(0) var frame: texture_2d<f32>;

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(encode(textureLoad(frame, vec2<i32>(position.xy), 0).rgb), 1.0);
}
";

/// The UI colour and alpha from the frame with the UI (linear), its HUD-less copy (encoded) and
/// the UI drawn alone (`fg_ui_layer.rs`, linear, premultiplied): the alpha is the layer's, as the
/// 8-bit target will hold it; the colour is what makes DLSS-G's blend in the encoded values give
/// back the real frame exactly, `UI.RGB = encode(Final) - (1 - UI.A) x Hudless`. A blend in
/// encoded values cannot equal the frame's linear blend over every background; this one is
/// exact over the real frame's own and close over the nearby ones a generated frame shows.
/// Where the alpha is 0, nothing (the guide's "0.0 for pixels with no UI elements").
const UI_COLOUR_WGSL: &str = "
@group(0) @binding(0) var final_color: texture_2d<f32>;
@group(0) @binding(1) var hudless: texture_2d<f32>;
@group(0) @binding(2) var layer: texture_2d<f32>;

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(position.xy);
    let a = round(clamp(textureLoad(layer, p, 0).a, 0.0, 1.0) * 255.0) / 255.0;
    if a <= 0.0 {
        return vec4<f32>(0.0);
    }
    let f = encode(textureLoad(final_color, p, 0).rgb);
    let h = textureLoad(hudless, p, 0).rgb;
    return vec4<f32>(clamp(f - (1.0 - a) * h, vec3<f32>(0.0), vec3<f32>(1.0)), a);
}
";

/// The format of the UI alpha (one channel, 0 or 1 is all it holds). `R16Float`, not `R8Unorm`:
/// Streamline 2.14.16 reads a tagged Vulkan image's format through its own table, which has
/// `VK_FORMAT_R16_SFLOAT` but no `VK_FORMAT_R8_UNORM`, and an R8 image logged four "Cannot have
/// undefined format" errors a frame [documented: `source/platforms/sl.chi/vulkan.cpp` `getFormat`,
/// `isFormatSupported` 3070-3081; observed in the 2026-11-13 runs].
const UI_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;

/// The UI alpha (`NVF_FG_UI_LAYER=0`) from the frame (linear) and its HUD-less copy (encoded,
/// 8 bits): 1 where they differ by more than the copy's own rounding (the same `encode` of the
/// same value, stored to 8 bits: at most half a step off, 0.6 with a GPU's looser rounding).
const UI_BUFFER_WGSL: &str = "
@group(0) @binding(0) var final_color: texture_2d<f32>;
@group(0) @binding(1) var hudless: texture_2d<f32>;

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(position.xy);
    let f = encode(textureLoad(final_color, p, 0).rgb) * 255.0;
    let h = round(textureLoad(hudless, p, 0).rgb * 255.0);
    return vec4<f32>(select(0.0, 1.0, any(abs(f - h) > vec3<f32>(0.75))), 0.0, 0.0, 0.0);
}
";

impl FrameGen {
    /// Publish only a completed transition; FSR's next extraction reads the shared flag.
    fn complete_load(&mut self, load: bool, result: super::ffi::SlResult, shared: &AtomicBool) {
        if !result.is_ok() {
            return;
        }
        // DLSS-G's inputs fence belongs to the DLSS-G that was loaded: no wait on it any more.
        self.inputs_fence = None;
        if let Some((queue, semaphore)) = self.staged_wait.take() {
            unstage(&queue, semaphore);
        }
        self.loaded = load;
        shared.store(load, Ordering::Relaxed);
        self.applied = None;
        self.presented_off = true;
    }

    fn new(get_state: SlDlssgGetState, set_options: SlDlssgSetOptions, loaded: bool) -> Self {
        Self {
            get_state,
            set_options,
            applied: None,
            requested_frames: 1,
            failed_said: None,
            status_failed: None,
            max_frames: 0,
            min_size: 0,
            constants: None,
            decision: None,
            hudless: None,
            kept: VecDeque::new(),
            last_state_line: None,
            period: None,
            said: Vec::new(),
            loaded,
            window_off: false,
            presented_off: true,
            hold_frames: 0,
            toggle: None,
            last_index: None,
            pending_load: None,
            exiting: false,
            exit_off: false,
            ui_source: None,
            ui: None,
            ui_layer: None,
            ui_colour: None,
            ui_colour_pass: None,
            hudless_pass: None,
            ui_said: false,
            just_switched: false,
            inputs_fence: None,
            staged_wait: None,
            wait_said: false,
        }
    }
}

static FRAME_GEN: Mutex<Option<FrameGen>> = Mutex::new(None);
/// [`FRAME_GEN`] holds something: the run condition of every system here.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// The mode is [`Mode::On`]: the run condition of the stage 2 systems.
static ON: AtomicBool = AtomicBool::new(false);

/// Present and acquire results the callback reported since the last state line: errors
/// (negative `VkResult`s) and other non-success results (positive), each with the last value.
static API_ERRORS: AtomicU32 = AtomicU32::new(0);
static LAST_API_ERROR: AtomicI32 = AtomicI32::new(0);
static API_OTHER: AtomicU32 = AtomicU32::new(0);
static LAST_API_OTHER: AtomicI32 = AtomicI32::new(0);
/// `VK_ERROR_OUT_OF_DATE_KHR` and `VK_SUBOPTIMAL_KHR` apart: the swapchain no longer matching
/// the window (a resize), which Bevy answers with a reconfigure; not an error.
static API_OUT_OF_DATE: AtomicU32 = AtomicU32::new(0);

/// Whether a present or acquire result only says the swapchain no longer matches the surface:
/// `VK_ERROR_OUT_OF_DATE_KHR` or `VK_SUBOPTIMAL_KHR` [documented: the Vulkan specification's
/// `vkQueuePresentKHR` and `vkAcquireNextImageKHR` return codes].
fn out_of_date(value: i32) -> bool {
    value == vk::Result::ERROR_OUT_OF_DATE_KHR.as_raw() || value == vk::Result::SUBOPTIMAL_KHR.as_raw()
}

fn frame_gen() -> MutexGuard<'static, Option<FrameGen>> {
    FRAME_GEN.lock().unwrap_or_else(PoisonError::into_inner)
}

fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// DLSS-G kept loaded (`win.rs` `frame_generation_after_device`): its two functions, fetched with
/// `slGetFeatureFunction` under `kFeatureDLSS_G`.
pub(super) fn install(get_state: SlDlssgGetState, set_options: SlDlssgSetOptions, loaded: bool) {
    *frame_gen() = Some(FrameGen::new(get_state, set_options, loaded));
    ON.store(true, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
}

/// At shutdown (stage 4 item 6; `win.rs` `shutdown`, the render thread gone, or idle with the
/// render app taken back from it at a handoff's end, `super::end_now`), the first steps of
/// the order eOff, null tags, drain, `slFreeResources`, `slShutdown`: the mode set off and the frame's
/// tags set null for the next frame number [documented:
/// `ProgrammingGuide.md:1017-1025`, a tag set to null before the resource goes; fork-notes
/// `fg-facts.md` §4], then the functions dropped. The render world, and with it the tagged
/// textures, is gone already (Bevy drops it before `App::run` returns); no present has followed
/// and Streamline keeps no reference to a Vulkan resource [documented:
/// `ProgrammingGuideDLSS_G.md` §5.2, "This does not apply to Vulkan"]. Returns `None` when
/// DLSS-G was not installed, else whether it is loaded (its resources are then freed by the
/// caller after the drain).
pub(super) fn uninstall() -> Option<bool> {
    ACTIVE.store(false, Ordering::Relaxed);
    ON.store(false, Ordering::Relaxed);
    let fg = frame_gen().take()?;
    if !fg.loaded {
        note_fg(Level::INFO, "shutdown: DLSS-G unloaded (the live toggle); no options, tags or resources to release");
        return Some(false);
    }
    let mut options = off_options();
    if let Some(a) = fg.applied {
        fill_sizes(&mut options, a);
    }
    let viewport = ViewportHandle::new(VIEWPORT);
    // SAFETY: DLSS-G's function with its header signature, options and viewport alive for the
    // call; nothing else calls it now (the render thread is gone).
    let result = unsafe { (fg.set_options)(&viewport, &options) };
    note_fg(if result.is_ok() { Level::INFO } else { Level::WARN }, format!("shutdown: options mode eOff: {}", result.name()));
    {
        let index = fg.last_index.map_or(0, |i| i.wrapping_add(1));
        let tagged = frame_token::set_tags(index, &null_tags());
        note_fg(if tagged.is_ok() { Level::INFO } else { Level::WARN }, format!("shutdown: null tags (frame {index}): {}", tagged.name()));
    }
    Some(true)
}

/// The render systems (`DlssPassPlugin`); each runs only while DLSS-G is installed.
pub(super) fn add_systems(render_app: &mut SubApp) {
    render_app
        .add_systems(Render, main_texture_copies.in_set(RenderSystems::PrepareViews).after(crate::upscaler::prep::prepare).before(prepare_view_targets).run_if(on))
        .add_systems(Render, set_constants.in_set(RenderSystems::PrepareResources).after(crate::upscaler::prep::prepare).run_if(on))
        .add_systems(Render, window_changes.before(create_surfaces).run_if(on))
        .add_systems(ExtractSchedule, toggle_switch.after(extract_windows).after(crate::frame_gen::ReleasePresenter).run_if(on))
        .add_systems(ExtractSchedule, follow_request.before(toggle_switch).run_if(on))
        .add_systems(ExtractSchedule, exit_watch.before(extract_windows).run_if(active))
        .add_systems(Core3d, tag.after(Core3dSystems::PostProcess).before(ui_pass).run_if(on))
        .add_systems(Core3d, ui_buffer.after(ui_pass).before(upscaling).run_if(on))
        .add_systems(RenderGraph, options.after(RenderGraphSystems::Finish).run_if(active))
        .add_systems(Render, state.in_set(RenderSystems::Render).after(render_system).run_if(active))
        .add_systems(Render, wait_for_inputs.in_set(RenderSystems::ExtractCommands).run_if(active));
}

/// `DLSSGOptions::onErrorCallback`: stores the call's `VkResult` and returns at once (the module
/// comment's "Errors"). Never takes a lock, never logs, never unwinds.
unsafe extern "C" fn on_api_error(error: *const ApiError) {
    if error.is_null() {
        return;
    }
    // SAFETY: Streamline passes a reference to its `APIError`; on Vulkan its `vkRes` member is the
    // one written, and every bit pattern of the 4-byte union is a valid `i32`.
    let value = unsafe { (*error).vk_res };
    if out_of_date(value) {
        API_OUT_OF_DATE.fetch_add(1, Ordering::Relaxed);
    } else if value < 0 {
        LAST_API_ERROR.store(value, Ordering::Relaxed);
        API_ERRORS.fetch_add(1, Ordering::Relaxed);
    } else {
        LAST_API_OTHER.store(value, Ordering::Relaxed);
        API_OTHER.fetch_add(1, Ordering::Relaxed);
    }
}

/// Mode off, resources kept while off, the error callback.
fn off_options() -> DlssgOptions {
    let mut options = DlssgOptions::new();
    options.mode = DLSSG_MODE_OFF;
    options.flags = DLSSG_FLAG_RETAIN_RESOURCES_WHEN_OFF;
    options.on_error_callback = Some(on_api_error);
    // "must be same for all viewports" (one here), and the same in every call.
    options.queue_parallelism_mode = *QUEUE_MODE;
    options
}

/// The sizes and back buffers of `applied` in `options` (`sl_dlss_g.h:85-95`).
fn fill_sizes(options: &mut DlssgOptions, applied: Applied) {
    options.num_back_buffers = applied.back_buffers;
    options.mvec_depth_width = applied.render.x;
    options.mvec_depth_height = applied.render.y;
    options.color_width = applied.output.x;
    options.color_height = applied.output.y;
}

/// The options for `applied` (stage 2): on or off with resources kept, the frames, the sizes;
/// no dynamic resolution, the default queue parallelism.
fn options_for(applied: Applied) -> DlssgOptions {
    let mut options = off_options();
    if applied.on {
        options.mode = DLSSG_MODE_ON;
        // The HUD-less and the UI buffer are tagged on every frame that is on (§6.6).
        options.enable_user_interface_recomposition = BOOL_TRUE;
    }
    options.num_frames_to_generate = applied.frames;
    fill_sizes(&mut options, applied);
    options
}

/// Facts about one frame for [`may_generate`].
#[derive(Clone, Copy, Debug, Default)]
struct Facts {
    /// The play state is Playing (not a menu, dialogue, loading or paused).
    playing: bool,
    /// The upscaler reconstructed this frame (not its spatial fallback).
    reconstructed: bool,
    /// This frame's constants were set ([`set_constants`]).
    constants: bool,
    /// The camera draws into the window's image (not a screenshot's texture).
    to_window: bool,
    /// The prepass motion exists and the main texture can be copied from.
    inputs: bool,
    /// The output is at least DLSS-G's minimum size (`DLSSGState::minWidthOrHeight`).
    size_ok: bool,
    /// A status other than `eOk` turned frame generation off for the session.
    failed: bool,
}

/// Whether this frame may be generated: in play, the upscaler's output real, the constants and
/// inputs there, the window shown, large enough, and no failed status. A camera cut is not a
/// reason: the constants' `reset` tells DLSS-G [documented: `ProgrammingGuideDLSS_G.md` §5.2,
/// mode off and null tags only where the inputs are not valid (menus, loading, pause)].
fn may_generate(f: Facts) -> bool {
    f.playing && f.reconstructed && f.constants && f.to_window && f.inputs && f.size_ok && !f.failed
}

/// The main texture can be copied from on the cameras an upscaler draws, for the frame without
/// the HUD (stage 2). Every frame: Bevy extracts the usages again.
fn main_texture_copies(mut views: Query<&mut CameraMainTextureUsages, With<UpscaleCamera>>) {
    for mut usages in &mut views {
        usages.0 |= wgpu::TextureUsages::COPY_SRC;
    }
}

/// The common constants for the first camera an upscaler draws, with this frame's number
/// (stage 2; the module comment). Without a previous view nothing is set and the frame is not
/// generated.
fn set_constants(views: Query<(&ExtractedView, &PreparedUpscale, &TemporalJitter, Option<&PreviousViewData>)>, presenting: Option<Res<crate::anti_lag::Presenting>>) {
    // DLSS's evaluator's own reset (its last frame was not its evaluate: a fallback, a new set of
    // options), as the SR evaluate ORs it into its constants (`evaluate.rs` step 2); asked before
    // this module's lock is taken (`win.rs`'s lock is never taken inside it).
    let dlss_stale = super::win::dlss_history_stale();
    let mut guard = frame_gen();
    let Some(fg) = guard.as_mut() else { return };
    fg.constants = None;
    let (Some(presenting), Some((view, prepared, jitter, Some(previous)))) = (presenting, views.iter().next()) else { return };
    let index = frame_token::index_of(presenting.frame);
    let constants = constants_for(&ConstantsInput {
        view,
        previous,
        jitter: jitter.offset,
        near: prepared.camera.near,
        fov: prepared.camera.fov,
        render_size: prepared.render_size,
        reset: prepared.reset || !prepared.history_valid.load(Ordering::Relaxed) || (matches!(prepared.camera.settings, crate::upscaler::prep::UpscaleSettings::Dlss(_)) && dlss_stale),
    });
    let result = frame_token::set_constants(index, &constants);
    if result.is_ok() {
        fg.constants = Some(index);
    } else if !fg.said.contains(&"slSetConstants") {
        fg.said.push("slSetConstants");
        note_fg(Level::WARN, format!("slSetConstants (frame {index}): {}; those frames are not generated (said once)", result.name()));
    }
}

/// The frame's tags (stage 2; the module comment): the three inputs on a frame that may be
/// generated, null tags otherwise, and the decision for [`options`].
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn tag(
    view: ViewQuery<(&ViewTarget, &PreparedUpscale, Option<&ViewPrepassTextures>, &ExtractedCamera)>,
    presenting: Option<Res<crate::anti_lag::Presenting>>,
    adapter: Res<RenderAdapter>,
    windows: Query<(MainEntity, &ExtractedWindow)>,
    mut ctx: RenderContext,
) {
    let (target, prepared, prepass, camera) = view.into_inner();
    let mut guard = frame_gen();
    let Some(fg) = guard.as_mut() else { return };
    let Some(presenting) = presenting else { return };
    let index = frame_token::index_of(presenting.frame);
    fg.ui_source = None;
    let motion = prepass.and_then(|p| p.motion_vectors.as_ref());
    let main: &wgpu::Texture = target.main_texture();
    // Where the camera draws: the window's image, or a screenshot's texture (as
    // `crate::frame_gen::generate` tells them apart).
    let to_window = match camera.target.as_ref() {
        Some(NormalizedRenderTarget::Window(window)) => windows
            .iter()
            .find(|(w, _)| *w == window.entity())
            .and_then(|(_, w)| w.swap_chain_texture_view.as_ref())
            .zip(target.out_texture())
            .is_some_and(|(image, out)| image.id() == out.id()),
        _ => false,
    };
    let facts = Facts {
        playing: prepared.camera.play_state.is_none_or(|s| s == nvf_play::PlayState::Playing),
        reconstructed: prepared.history_valid.load(Ordering::Relaxed),
        constants: fg.constants == Some(index),
        to_window,
        inputs: motion.is_some() && main.usage().contains(wgpu::TextureUsages::COPY_SRC),
        size_ok: prepared.output_size.min_element() >= fg.min_size,
        failed: fg.status_failed.is_some() || fg.window_off || !fg.loaded,
    };
    let tagged = match motion.filter(|_| may_generate(facts)) {
        Some(motion) => tag_inputs(fg, &mut ctx, &adapter, index, prepared, (main, target.main_texture_view()), &motion.texture.texture, &motion.texture.default_view),
        None => {
            let tags = null_tags();
            let result = frame_token::set_tags(index, &tags);
            if !result.is_ok() && !fg.said.contains(&"null tags") {
                fg.said.push("null tags");
                note_fg(Level::WARN, format!("slSetTagForFrame (null tags, frame {index}): {} (said once)", result.name()));
            }
            false
        }
    };
    fg.last_index = Some(index);
    fg.decision = Some(Decision { generate: tagged, render: prepared.render_size, output: prepared.output_size });
}

/// The copy without the HUD, the transitions and the three tags (`eValidUntilPresent`); false
/// when anything was missing or Streamline refused the tags.
#[allow(clippy::too_many_arguments)]
fn tag_inputs(
    fg: &mut FrameGen,
    ctx: &mut RenderContext,
    adapter: &RenderAdapter,
    index: u32,
    prepared: &PreparedUpscale,
    (main, main_view): (&wgpu::Texture, &wgpu::TextureView),
    motion: &wgpu::Texture,
    motion_view: &wgpu::TextureView,
) -> bool {
    let device = ctx.render_device().wgpu_device().clone();
    if fg.hudless.as_ref().is_none_or(|h| h.textures[0].size() != main.size()) {
        fg.hudless = Some(Hudless::new(&device, main.size(), "dlssg_hudless"));
    }
    let Some(hudless) = fg.hudless.as_mut() else { return false };
    let at = hudless.next;
    hudless.next = 1 - at;
    let (hudless_texture, hudless_view) = (hudless.textures[at].clone(), hudless.views[at].clone());
    let depth = &prepared.depth;
    let depth_view = depth.create_view(&wgpu::TextureViewDescriptor { label: Some("dlssg_depth"), aspect: wgpu::TextureAspect::DepthOnly, ..default() });
    let described = {
        // SAFETY: only read.
        let Some(hal) = (unsafe { adapter.as_hal::<wgpu::hal::api::Vulkan>() }) else { return false };
        [
            describe(&hal, depth, &depth_view, vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL),
            describe(&hal, motion, motion_view, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL),
            describe(&hal, &hudless_texture, &hudless_view, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL),
        ]
    };
    let [Some(mut depth_res), Some(mut motion_res), Some(mut hudless_res)] = described else {
        if !fg.said.contains(&"handles") {
            fg.said.push("handles");
            note_fg(Level::WARN, "a tagged texture or view has no Vulkan handle; frames are not generated (said once)");
        }
        return false;
    };
    {
        let pass = fg.hudless_pass.get_or_insert_with(|| Fullscreen::new(&device, "dlssg_hudless", HUDLESS_WGSL, 1, ENCODED_FORMAT));
        let encoder = ctx.command_encoder();
        pass.draw(&device, encoder, &[main_view], &hudless_view);
        let transition = |texture, state| wgpu::TextureTransition { texture, selector: None, state };
        encoder.transition_resources(
            std::iter::empty(),
            [transition(depth, wgpu::TextureUses::RESOURCE), transition(motion, wgpu::TextureUses::RESOURCE), transition(&hudless_texture, wgpu::TextureUses::RESOURCE)].into_iter(),
        );
    }
    // The depth aspect, else the colour aspect is assumed (as for the SR evaluate,
    // `evaluate.rs` step 4).
    let mut depth_range = SubresourceRange::new();
    depth_range.aspect_mask = vk::ImageAspectFlags::DEPTH.as_raw();
    depth_range.level_count = 1;
    depth_range.layer_count = 1;
    depth_res.base.next = (&raw mut depth_range).cast::<BaseStructure>();
    let render = Extent { top: 0, left: 0, width: prepared.render_size.x, height: prepared.render_size.y };
    let output = Extent { top: 0, left: 0, width: prepared.output_size.x, height: prepared.output_size.y };
    let tags = [
        ResourceTag::new(&raw mut depth_res, BUFFER_DEPTH, LIFECYCLE_VALID_UNTIL_PRESENT, render),
        ResourceTag::new(&raw mut motion_res, BUFFER_MOTION_VECTORS, LIFECYCLE_VALID_UNTIL_PRESENT, render),
        ResourceTag::new(&raw mut hudless_res, BUFFER_HUDLESS_COLOR, LIFECYCLE_VALID_UNTIL_PRESENT, output),
    ];
    let result = frame_token::set_tags(index, &tags);
    if result.is_ok() {
        fg.ui_source = Some(hudless_view.clone());
    }
    fg.kept.push_back((vec![depth.clone(), motion.clone(), hudless_texture], vec![depth_view, motion_view.clone(), hudless_view]));
    while fg.kept.len() > KEEP_FRAMES {
        fg.kept.pop_front();
    }
    if !result.is_ok() && !fg.said.contains(&"tags") {
        fg.said.push("tags");
        note_fg(Level::WARN, format!("slSetTagForFrame (frame {index}): {}; those frames are not generated (said once)", result.name()));
    }
    result.is_ok()
}

/// What the options should be now: stage 1b off once; stage 2 from this frame's decision.
/// `numBackBuffers` is left 0 (optional, `sl_dlss_g.h:85`): wgpu makes `max(latency + 1, the
/// surface's minimum image count)` images and DLSS-G's proxy may make others, neither known
/// here [design: the review's P3e].
fn wanted(fg: &FrameGen, decision: Option<Decision>) -> Applied {
    let back_buffers = 0;
    let last = fg.applied;
    let (render, output) = decision.map(|d| (d.render, d.output)).or(last.map(|a| (a.render, a.output))).unwrap_or_default();
    let on = decision.is_some_and(|d| d.generate) && fg.status_failed.is_none() && !fg.window_off && fg.loaded && !fg.exiting;
    Applied { on, frames: fg.requested_frames.clamp(1, fg.max_frames.max(1)), render, output, back_buffers }
}

/// Whether the options must be set: they differ, except that off stays off whatever the sizes.
fn must_set(applied: Option<Applied>, want: Applied) -> bool {
    match applied {
        None => true,
        Some(a) if !a.on && !want.on => false,
        Some(a) => a != want,
    }
}

/// The options when they must change, on the render thread just before the present (the module
/// comment); a failed call is made again next frame.
fn options(_world: &mut World) {
    let mut guard = frame_gen();
    let Some(fg) = guard.as_mut() else { return };
    let decision = fg.decision.take();
    if !fg.loaded {
        // Unloaded (the live toggle): no DLSS-G call; set again once loaded.
        fg.applied = None;
        return;
    }
    let want = wanted(fg, decision);
    if !must_set(fg.applied, want) {
        return;
    }
    let options = options_for(want);
    let viewport = ViewportHandle::new(VIEWPORT);
    // SAFETY: DLSS-G's function with its header signature; a viewport handle and options from
    // their constructors, alive for the call; the callback is a static function.
    let result = unsafe { (fg.set_options)(&viewport, &options) };
    let text = if want.on {
        format!(
            "options: mode On, {} generated frame(s) per real frame (at most {}), {}x{}, depth and motion {}x{}, back buffers left to DLSS-G, user interface recomposition on: {}",
            want.frames,
            fg.max_frames,
            want.output.x,
            want.output.y,
            want.render.x,
            want.render.y,
            result.name()
        )
    } else {
        format!("options: mode Off (resources kept), {}x{}: {}", want.output.x, want.output.y, result.name())
    };
    if result.is_ok() {
        fg.applied = Some(want);
        fg.failed_said = None;
        note_fg(Level::INFO, text);
    } else if fg.failed_said != Some((want, result.0)) {
        fg.failed_said = Some((want, result.0));
        note_fg(Level::WARN, format!("{text}; tried again each frame (said once)"));
    }
}

/// The state after every present (the module comment).
fn state(world: &mut World) {
    let generated = world.get_resource::<crate::frame_gen::GeneratedFrames>().cloned();
    let mut guard = frame_gen();
    let Some(fg) = guard.as_mut() else { return };
    if !fg.loaded {
        fg.presented_off = true;
        return;
    }
    let viewport = ViewportHandle::new(VIEWPORT);
    let mut state = DlssgState::new();
    // SAFETY: DLSS-G's function with its header signature; null options ask for no VRAM estimate.
    let result = unsafe { (fg.get_state)(&viewport, &mut state, std::ptr::null()) };
    if !result.is_ok() {
        if !fg.said.contains(&"slDLSSGGetState") {
            fg.said.push("slDLSSGGetState");
            note_fg(Level::WARN, format!("state: slDLSSGGetState {} (said once)", result.name()));
        }
        return;
    }
    fg.max_frames = state.num_frames_to_generate_max;
    super::FG_MAX_FRAMES.store(fg.max_frames, Ordering::Relaxed);
    fg.min_size = state.min_width_or_height;
    // The mode is off now: a window change may go ahead, whether or not a frame was presented
    // (an image that came back `Outdated` presents nothing, and DLSS-G has had no present to
    // work on). So a hold lasts the one frame that sets the mode off; the "going ahead" warning
    // in `window_changes` is a safeguard that does not come up.
    let off_now = !fg.applied.is_some_and(|a| a.on);
    if state.num_frames_actually_presented > 0 || off_now {
        fg.presented_off = off_now;
    }
    // The fence for the next frame's first submit: after a present with the mode on, as NVIDIA's
    // sample waits only when DLSS-G was on last frame (a value of 0 waits on nothing).
    let fence = state.inputs_processing_completion_fence;
    let value = state.last_present_inputs_processing_completion_fence_value;
    fg.inputs_fence = (*QUEUE_MODE == DLSSG_QUEUE_BLOCK_NO_CLIENT_QUEUES && !off_now && state.num_frames_actually_presented > 0 && !fence.is_null() && value > 0).then_some((fence as u64, value));
    let now = Instant::now();
    // A frame nothing was presented for (the window minimised) is not counted as a real frame.
    if state.num_frames_actually_presented > 0 {
        let period = fg.period.get_or_insert_with(|| Period::new(now));
        period.real += 1;
        period.presented += u64::from(state.num_frames_actually_presented);
    }
    let is_on = fg.applied.is_some_and(|a| a.on);
    {
        if let Some(generated) = &generated {
            generated.0.fetch_add(u64::from(state.num_frames_actually_presented.saturating_sub(1)), Ordering::Relaxed);
        }
        if state.status != 0 && is_on && fg.status_failed.is_none() {
            let names = dlssg_status_name(state.status);
            note_fg(Level::WARN, format!("status {names}: frame generation off for the rest of the session (mode eOff from the next frame)"));
            fg.status_failed = Some(names);
            super::FG_ENDED.store(true, Ordering::Relaxed);
        }
    }
    if fg.last_state_line.is_none_or(|last| now.duration_since(last) >= STATE_EVERY) {
        fg.last_state_line = Some(now);
        let errors = (API_ERRORS.swap(0, Ordering::Relaxed), LAST_API_ERROR.load(Ordering::Relaxed));
        let other = (API_OTHER.swap(0, Ordering::Relaxed), LAST_API_OTHER.load(Ordering::Relaxed));
        note_fg(Level::INFO, state_line(&state, errors, other, API_OUT_OF_DATE.swap(0, Ordering::Relaxed)));
    }
    {
        if let Some(period) = fg.period.as_mut().filter(|p| now.duration_since(p.since) >= RATIO_EVERY) {
            let line = ratio_line(now.duration_since(period.since), period.real, period.presented, fg.status_failed.as_deref());
            *period = Period::new(now);
            note_fg(Level::INFO, line);
        }
    }
}

/// `eBlockNoClientQueues`: the GPU wait on DLSS-G's inputs fence for this frame's submits (the
/// module comment's "Queue parallelism"). Exclusive and first in `Render`, so on the render
/// thread (which presents) and before any of the frame's submits; after the extraction, so a
/// live toggle has already dropped the fence ([`FrameGen::complete_load`]).
fn wait_for_inputs(world: &mut World) {
    let mut guard = frame_gen();
    let Some(fg) = guard.as_mut() else { return };
    // A wait no submit took (nothing submitted last frame): the newer value covers it.
    if let Some((queue, semaphore)) = fg.staged_wait.take() {
        unstage(&queue, semaphore);
    }
    let Some((semaphore, value)) = fg.inputs_fence.take() else { return };
    if !fg.loaded {
        return;
    }
    let queue: wgpu::Queue = (**world.resource::<RenderQueue>()).clone();
    // SAFETY: only adds an entry to wgpu-hal's pending wait list for the queue's next submit; the
    // semaphore is DLSS-G's, alive while DLSS-G is loaded (a load or unload takes the entry back).
    let Some(hal) = (unsafe { queue.as_hal::<wgpu::hal::api::Vulkan>() }) else { return };
    hal.add_wait_semaphore(vk::Semaphore::from_raw(semaphore), Some(value), vk::PipelineStageFlags::ALL_COMMANDS);
    drop(hal);
    fg.staged_wait = Some((queue, semaphore));
    if !std::mem::replace(&mut fg.wait_said, true) {
        note_fg(
            Level::INFO,
            format!("queue parallelism {}: each frame's first submit waits on DLSS-G's inputs fence (first value {value})", queue_mode_name(*QUEUE_MODE)),
        );
    }
}

/// The UI alpha's pass.
fn ui_pass_for(device: &wgpu::Device, format: wgpu::TextureFormat) -> UiPass {
    UiPass { pass: Fullscreen::new(device, "dlssg_ui_buffer", UI_BUFFER_WGSL, 2, format), textures: None, next: 0 }
}

/// The tags a frame that is not generated sets null (DLSS-G's inputs, the HUD-less and
/// both UI buffer kinds).
fn null_tags() -> [ResourceTag; 5] {
    let null = |buffer| ResourceTag::new(std::ptr::null_mut(), buffer, LIFECYCLE_VALID_UNTIL_PRESENT, Extent::default());
    [null(BUFFER_DEPTH), null(BUFFER_MOTION_VECTORS), null(BUFFER_HUDLESS_COLOR), null(BUFFER_UI_ALPHA), null(BUFFER_UI_COLOR_AND_ALPHA)]
}

/// The UI buffer on a tagged frame (the module comment's "UI buffer"): the pass, the move to
/// `RESOURCE`, the tag. When it cannot be made, whatever the reason, the frame is not
/// generated: its tags go null.
fn ui_buffer(
    world: &World,
    view: ViewQuery<(&ViewTarget, &PreparedUpscale, &ExtractedCamera, Option<&UiCameraView>)>,
    adapter: Res<RenderAdapter>,
    mut ctx: RenderContext,
) {
    let (target, prepared, camera, ui) = view.into_inner();
    let mut guard = frame_gen();
    let Some(fg) = guard.as_mut() else { return };
    let hudless_view = fg.ui_source.take();
    let (Some(index), Some(decision)) = (fg.last_index, fg.decision.filter(|d| d.generate)) else { return };
    let layer = *crate::fg_ui_layer::ENABLED;
    let result = match hudless_view {
        Some(hudless_view) if layer => draw_ui_layer(fg, &mut ctx, &adapter, world, ui, camera, target, prepared, index, &hudless_view),
        Some(hudless_view) => draw_ui_alpha(fg, &mut ctx, &adapter, target, prepared, index, &hudless_view),
        None => Err("no HUD-less copy this frame".to_string()),
    };
    match result {
        Ok(()) => {
            if !fg.ui_said {
                fg.ui_said = true;
                let what = if layer {
                    format!("{ENCODED_FORMAT:?} UI colour and alpha (the UI drawn alone over transparent black, premultiplied, sRGB-encoded as the back buffer with the HUD-less copy)")
                } else {
                    "R16Float UI alpha (1 where the frame differs from its HUD-less copy, 0 elsewhere)".to_string()
                };
                note_fg(
                    Level::INFO,
                    format!(
                        "UI buffer: {}x{} {what}, tagged eValidUntilPresent with the HUD-less copy; user interface recomposition on",
                        prepared.output_size.x, prepared.output_size.y
                    ),
                );
            }
        }
        Err(why) => {
            if !fg.said.contains(&"ui tag") {
                fg.said.push("ui tag");
                note_fg(Level::WARN, format!("UI buffer (frame {index}): {why}; those frames are not generated, their tags null (said once)"));
            }
            frame_token::set_tags(index, &null_tags());
            fg.decision = Some(Decision { generate: false, ..decision });
        }
    }
}

/// The UI drawn alone (`fg_ui_layer.rs`), the UI colour and alpha made from it with the frame
/// and its HUD-less copy ([`UI_COLOUR_WGSL`], [`ENCODED_FORMAT`]), its transition and its tag
/// for frame number `index`: `Final = UI.RGB + (1 - UI.A) * Hudless` with the colour
/// premultiplied, as DLSS-G asks [documented: `ProgrammingGuideDLSS_G.md` §5.1].
#[allow(clippy::too_many_arguments)]
fn draw_ui_layer(
    fg: &mut FrameGen,
    ctx: &mut RenderContext,
    adapter: &RenderAdapter,
    world: &World,
    ui: Option<&UiCameraView>,
    camera: &ExtractedCamera,
    target: &ViewTarget,
    prepared: &PreparedUpscale,
    index: u32,
    hudless_view: &wgpu::TextureView,
) -> Result<(), String> {
    let main: &wgpu::Texture = target.main_texture();
    let main_view: &wgpu::TextureView = target.main_texture_view();
    let device = ctx.render_device().wgpu_device().clone();
    if fg.ui_layer.as_ref().is_none_or(|(t, _)| t.size() != main.size() || t.format() != main.format()) {
        let texture = crate::fg_ui_layer::layer_texture(&device, main.size(), main.format(), "dlssg_ui_layer");
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        fg.ui_layer = Some((texture, view));
        if let Some(pass) = fg.ui_colour_pass.as_mut() {
            pass.groups.clear();
        }
    }
    if fg.ui_colour.as_ref().is_none_or(|c| c.textures[0].size() != main.size()) {
        fg.ui_colour = Some(Hudless::new(&device, main.size(), "dlssg_ui_color_alpha"));
    }
    let (Some((_, layer_view)), Some(colour)) = (fg.ui_layer.as_ref(), fg.ui_colour.as_mut()) else { return Err("no UI layer texture".to_string()) };
    let layer_view = layer_view.clone();
    let at = colour.next;
    colour.next = 1 - at;
    let (ui_texture, ui_view) = (colour.textures[at].clone(), colour.views[at].clone());
    let described = {
        // SAFETY: only read.
        let Some(hal) = (unsafe { adapter.as_hal::<wgpu::hal::api::Vulkan>() }) else { return Err("not a Vulkan adapter".to_string()) };
        describe(&hal, &ui_texture, &ui_view, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
    };
    let Some(mut ui_res) = described else { return Err("the UI colour and alpha has no Vulkan handle".to_string()) };
    crate::fg_ui_layer::draw(world, ui, camera, ctx, &layer_view)?;
    let pass = fg.ui_colour_pass.get_or_insert_with(|| Fullscreen::new(&device, "dlssg_ui_color_alpha", UI_COLOUR_WGSL, 3, ENCODED_FORMAT));
    let encoder = ctx.command_encoder();
    pass.draw(&device, encoder, &[main_view, hudless_view, &layer_view], &ui_view);
    encoder.transition_resources(std::iter::empty(), std::iter::once(wgpu::TextureTransition { texture: &ui_texture, selector: None, state: wgpu::TextureUses::RESOURCE }));
    let output = Extent { top: 0, left: 0, width: prepared.output_size.x, height: prepared.output_size.y };
    let tag = [ResourceTag::new(&raw mut ui_res, BUFFER_UI_COLOR_AND_ALPHA, LIFECYCLE_VALID_UNTIL_PRESENT, output)];
    let result = frame_token::set_tags(index, &tag);
    if let Some(kept) = fg.kept.back_mut() {
        kept.0.push(ui_texture);
        kept.1.push(ui_view);
    }
    if result.is_ok() { Ok(()) } else { Err(format!("slSetTagForFrame {}", result.name())) }
}

/// The UI alpha's pass, transition and tag for frame number `index` (`NVF_FG_UI_LAYER=0`).
#[allow(clippy::too_many_arguments)]
fn draw_ui_alpha(
    fg: &mut FrameGen,
    ctx: &mut RenderContext,
    adapter: &RenderAdapter,
    target: &ViewTarget,
    prepared: &PreparedUpscale,
    index: u32,
    hudless_view: &wgpu::TextureView,
) -> Result<(), String> {
    let main: &wgpu::Texture = target.main_texture();
    let main_view: &wgpu::TextureView = target.main_texture_view();
    let device = ctx.render_device().wgpu_device().clone();
    let pass = fg.ui.get_or_insert_with(|| ui_pass_for(&device, UI_FORMAT));
    if pass.textures.as_ref().is_none_or(|(t, _)| t[0].width() != main.width() || t[0].height() != main.height()) {
        let textures: [wgpu::Texture; 2] = std::array::from_fn(|_| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("dlssg_ui_alpha"),
                size: main.size(),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: UI_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        });
        let views = std::array::from_fn(|i| textures[i].create_view(&wgpu::TextureViewDescriptor::default()));
        pass.textures = Some((textures, views));
        pass.pass.groups.clear();
    }
    let Some((textures, views)) = pass.textures.as_ref() else { return Err("no UI texture".to_string()) };
    let at = pass.next;
    pass.next = 1 - at;
    let (ui_texture, ui_view) = (textures[at].clone(), views[at].clone());
    let described = {
        // SAFETY: only read.
        let Some(hal) = (unsafe { adapter.as_hal::<wgpu::hal::api::Vulkan>() }) else { return Err("not a Vulkan adapter".to_string()) };
        describe(&hal, &ui_texture, &ui_view, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
    };
    let Some(mut ui_res) = described else { return Err("the UI texture has no Vulkan handle".to_string()) };
    let encoder = ctx.command_encoder();
    pass.pass.draw(&device, encoder, &[main_view, hudless_view], &ui_view);
    encoder.transition_resources(std::iter::empty(), std::iter::once(wgpu::TextureTransition { texture: &ui_texture, selector: None, state: wgpu::TextureUses::RESOURCE }));
    let output = Extent { top: 0, left: 0, width: prepared.output_size.x, height: prepared.output_size.y };
    let tag = [ResourceTag::new(&raw mut ui_res, BUFFER_UI_ALPHA, LIFECYCLE_VALID_UNTIL_PRESENT, output)];
    let result = frame_token::set_tags(index, &tag);
    if let Some(kept) = fg.kept.back_mut() {
        kept.0.push(ui_texture);
        kept.1.push(ui_view);
    }
    if result.is_ok() { Ok(()) } else { Err(format!("slSetTagForFrame {}", result.name())) }
}

/// The Frame generation row's request, read in the extraction (`crate::frame_gen::DlssgRequest`):
/// the frames to generate, and the live switch: off at once when DLSS is no longer what the row
/// asks for (or can be had). Loading waits for FSR's presenter to release the surface in
/// [`toggle_switch`], after `crate::frame_gen::ReleasePresenter` joins its threads.
fn follow_request(request: Extract<Option<Res<crate::frame_gen::DlssgRequest>>>) {
    let Some(request) = request.as_deref().copied() else { return };
    let mut guard = frame_gen();
    let Some(fg) = guard.as_mut() else { return };
    fg.requested_frames = request.frames.clamp(1, 3);
    if request.on == fg.loaded {
        // A newer choice cancels a switch that has not reached Streamline yet.
        fg.toggle = None;
        fg.pending_load = None;
        return;
    }
    if fg.toggle.is_none() && fg.pending_load.is_none() && (!request.on || fg.status_failed.is_none()) {
        fg.toggle = Some(request.on);
        note_fg(Level::INFO, format!("switch: frame generation {} asked (the Frame generation row)", if request.on { "on" } else { "off" }));
    }
}

/// What [`window_changes`] does to the surface this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowStep {
    /// Nothing pending, or Bevy may reconfigure (the mode held off this frame too).
    Go { off: bool },
    /// Hold the reconfigure; the mode off and presented first.
    Hold,
    /// Release the swapchain and load (`true`) or unload DLSS-G (the live toggle).
    Load(bool),
}

/// Loading DLSS-G requires the presenter's last surface reference to be gone.
fn switch_ready(load: bool, presenters_released: bool) -> bool {
    !load || presenters_released
}

/// The decision for one frame: `pending` window change, the toggle asked for, whether DLSS-G
/// is loaded and whether the last present ran with the mode off, the frames held so far.
fn window_step(pending: bool, toggle: Option<bool>, loaded: bool, presented_off: bool, held: u32) -> WindowStep {
    match toggle {
        Some(on) if on != loaded => {
            // Loading: DLSS-G is unloaded, so the presents are native; unloading waits for an
            // off present.
            if on || presented_off || held >= MAX_HOLD {
                return WindowStep::Load(on);
            }
            return WindowStep::Hold;
        }
        _ => {}
    }
    if !pending {
        return WindowStep::Go { off: false };
    }
    if loaded && !presented_off && held < MAX_HOLD { WindowStep::Hold } else { WindowStep::Go { off: true } }
}

/// Window changes and the live toggle (the module comment), before Bevy's `create_surfaces`.
fn window_changes(world: &mut World) {
    let found = {
        let mut windows = world.query_filtered::<(Entity, &ExtractedWindow, &SurfaceData), With<PrimaryWindow>>();
        windows.iter(world).next().map(|(entity, window, data)| {
            let what = if window.size_changed {
                format!("size {}x{}", window.physical_width, window.physical_height)
            } else if window.present_mode_changed {
                format!("present mode {:?}", window.present_mode)
            } else {
                "the surface outdated".to_string()
            };
            (entity, data.reconfigure_pending(window), what)
        })
    };
    let step = {
        let mut guard = frame_gen();
        let Some(fg) = guard.as_mut() else { return };
        // A switch to what DLSS-G is already: nothing to do (the review's P3b).
        if fg.toggle == Some(fg.loaded) {
            fg.toggle = None;
        }
        let pending = found.as_ref().is_some_and(|(_, pending, _)| *pending);
        let step = window_step(pending, fg.toggle, fg.loaded, fg.presented_off, fg.hold_frames);
        let just_switched = std::mem::take(&mut fg.just_switched);
        match step {
            WindowStep::Hold => {
                fg.hold_frames += 1;
                if fg.hold_frames == 1 {
                    let what = match (fg.toggle, &found) {
                        (Some(_), _) => "the live switch".to_string(),
                        (None, Some((_, _, what))) => what.clone(),
                        (None, None) => "a window change".to_string(),
                    };
                    note_fg(Level::INFO, format!("window change ({what}): DLSS-G off first; the surface's reconfigure held until a frame is presented with it off"));
                }
                fg.window_off = true;
            }
            WindowStep::Go { off } => {
                if off && fg.hold_frames > 0 {
                    let level = if fg.presented_off { Level::INFO } else { Level::WARN };
                    let how = if fg.presented_off { "DLSS-G off at the last present" } else { "no frame was presented with DLSS-G off; going ahead" };
                    note_fg(level, format!("window change: the surface reconfigured after {} held frame(s) ({how}), DLSS-G still off this frame", fg.hold_frames));
                }
                if !off {
                    fg.hold_frames = 0;
                }
                fg.window_off = off || just_switched;
            }
            WindowStep::Load(load) => {
                fg.window_off = true;
                fg.toggle = None;
                fg.pending_load = Some(load);
            }
        }
        if let WindowStep::Go { off: true } = step {
            fg.hold_frames = 0;
        }
        step
    };
    world.resource_mut::<HoldSurfaceReconfigure>().0 = step == WindowStep::Hold;
}

/// The live toggle's switch, in the extraction after the frame that decided it (the module
/// comment): the swapchain released, DLSS-G's resources freed when it goes off,
/// `slSetFeatureLoaded`; Bevy's `create_surfaces` makes the swapchain again this frame.
fn toggle_switch(world: &mut World) {
    let Some(load) = frame_gen().as_mut().and_then(|fg| fg.pending_load.take()) else { return };
    if !switch_ready(load, crate::frame_gen::presenters_released(world)) {
        if let Some(fg) = frame_gen().as_mut() {
            fg.pending_load = Some(load);
        }
        return;
    }
    let entity = {
        let mut windows = world.query_filtered::<Entity, (With<ExtractedWindow>, With<SurfaceData>, With<PrimaryWindow>)>();
        windows.iter(world).next()
    };
    let Some(entity) = entity else {
        note_fg(Level::WARN, "switch: no window surface to make again; nothing switched");
        return;
    };
    // The main thread's Reflex calls come after the extraction (`anti_lag.rs`), the render thread
    // waits during it; device polling and pipeline creation are held off (`gpu_poll::quiet`)
    // from before the drain until after `slSetFeatureLoaded`. Streamline's own present queue
    // is drained with the device.
    let quiet = crate::gpu_poll::quiet();
    let polled = world.resource::<bevy::render::renderer::RenderDevice>().wgpu_device().poll(wgpu::PollType::wait_indefinitely());
    release_surface(world, entity);
    let freed = (!load).then(super::win::free_frame_generation);
    let result = super::win::set_frame_generation_loaded(load);
    drop(quiet);
    let mut guard = frame_gen();
    let Some(fg) = guard.as_mut() else { return };
    fg.complete_load(load, result, &super::FG_KEPT_LOADED);
    if result.is_ok() && !load {
        // Nothing reads them while DLSS-G is unloaded (the GPU was drained above).
        fg.hudless = None;
        fg.ui_layer = None;
        fg.ui_colour = None;
        fg.kept.clear();
        for pass in [fg.hudless_pass.as_mut(), fg.ui_colour_pass.as_mut(), fg.ui.as_mut().map(|u| &mut u.pass)].into_iter().flatten() {
            pass.groups.clear();
        }
        if let Some(pass) = fg.ui.as_mut() {
            pass.textures = None;
        }
    }
    fg.hold_frames = 0;
    fg.just_switched = true;
    let freed = freed.map_or_else(String::new, |r| format!("slFreeResources(DLSS-G, viewport {VIEWPORT}) {}, ", r.name()));
    note_fg(
        if result.is_ok() { Level::INFO } else { Level::WARN },
        format!(
            "switch: frame generation {}: GPU drained ({}), the swapchain released, {freed}slSetFeatureLoaded(DLSS-G, {load}) {}, the swapchain made again this frame ({})",
            if load { "on" } else { "off" },
            if matches!(&polled, Ok(s) if s.wait_finished()) { "ok" } else { "incomplete" },
            result.name(),
            if load { "Streamline's proxy" } else { "native" }
        ),
    );
}

/// The app exiting (the module comment's "Exit"): on `AppExit` the mode goes off for the frame
/// being extracted (still presented); on a window closing, whose surface `extract_windows`
/// removes in this extraction, off at once (the render thread waits during the extraction, so
/// nothing else calls DLSS-G).
fn exit_watch(mut exits: Extract<MessageReader<AppExit>>, mut closing: Extract<MessageReader<WindowClosing>>) {
    let exit = exits.read().count() > 0;
    let closes = closing.read().count() > 0;
    if !exit && !closes {
        return;
    }
    let mut guard = frame_gen();
    let Some(fg) = guard.as_mut() else { return };
    if fg.exiting {
        return;
    }
    if !closes {
        fg.exiting = true;
        note_fg(Level::INFO, "exit: DLSS-G's mode off for the last frame (AppExit)");
        return;
    }
    // The render thread waits during the extraction: nothing else calls DLSS-G.
    exit_mode_off(fg, "the window closes");
}

/// The mode set off at once for an exit (the module comment's "Exit", step 1), whatever ends
/// the run: the window closing ([`exit_watch`]) or the run ending without Bevy's teardown
/// ([`end_now`]). Once a run: a second call does nothing. The caller makes sure no other
/// thread calls DLSS-G meanwhile (its functions are not thread safe, `sl_dlss_g.h:187-208`).
/// Logs `exit: <why>: DLSS-G's mode off now: <result>`. Returns whether `slDLSSGSetOptions`
/// was called.
fn exit_mode_off(fg: &mut FrameGen, why: &str) -> bool {
    fg.exiting = true;
    if fg.exit_off {
        return false;
    }
    fg.exit_off = true;
    if !fg.loaded {
        note_fg(Level::INFO, format!("exit: {why}: DLSS-G unloaded (the live toggle); no mode to set off"));
        return false;
    }
    let mut options = off_options();
    if let Some(a) = fg.applied {
        fill_sizes(&mut options, a);
    }
    let viewport = ViewportHandle::new(VIEWPORT);
    // SAFETY: DLSS-G's function with its header signature, options and viewport alive for the
    // call; no other thread calls DLSS-G (the caller's condition).
    let result = unsafe { (fg.set_options)(&viewport, &options) };
    if result.is_ok() {
        fg.applied = fg.applied.map(|a| Applied { on: false, ..a });
    }
    note_fg(if result.is_ok() { Level::INFO } else { Level::WARN }, format!("exit: {why}: DLSS-G's mode off now: {}", result.name()));
    true
}

/// A window's image dropped and its surface released: wgpu waits for the device and destroys
/// the swapchain (Streamline's proxy while DLSS-G is loaded), then the surface, when the last
/// reference goes (wgpu-hal `Swapchain::release_resources`). For the live toggle's switch
/// ([`toggle_switch`]) and a run's end ([`end_now`]); the caller drains the GPU first and holds
/// `gpu_poll::quiet`.
fn release_surface(world: &mut World, entity: Entity) {
    if let Some(mut window) = world.get_mut::<ExtractedWindow>(entity) {
        drop(window.swap_chain_texture.take());
        window.swap_chain_texture_view = None;
    }
    world.entity_mut(entity).remove::<SurfaceData>();
}

/// The run ends without Bevy's teardown (`super::end_now`, the module comment's "Exit"): with
/// the render thread idle (its render app taken back, `render_world`) and `gpu_poll::quiet`
/// held by the caller, the mode off at once ([`exit_mode_off`]), the GPU drained and every
/// window's swapchain released, before `win.rs` `shutdown` frees DLSS-G's resources and shuts
/// Streamline down. Nothing while DLSS-G is not installed. Without the render world (no
/// pipelined rendering, or the render thread stopped; `super::end_now` says so) only the mode
/// goes off. The line says whether the options had the mode on until now: what is known of the
/// last presents, not that one was presented off ([`state`] counts no present as off).
pub(super) fn end_now(render_world: Option<&mut World>, why: &str) {
    if !active() {
        return;
    }
    let was_on = {
        let mut guard = frame_gen();
        let Some(fg) = guard.as_mut() else { return };
        let was_on = fg.applied.is_some_and(|a| a.on);
        exit_mode_off(fg, why);
        was_on
    };
    let options = if was_on { "the mode was on in the options until now" } else { "the mode was off in the options already" };
    let Some(world) = render_world else { return };
    let polled = world.get_resource::<bevy::render::renderer::RenderDevice>().map(|d| d.wgpu_device().poll(wgpu::PollType::wait_indefinitely()));
    let entities: Vec<Entity> = world.query_filtered::<Entity, (With<ExtractedWindow>, With<SurfaceData>)>().iter(world).collect();
    for &entity in &entities {
        release_surface(world, entity);
    }
    note_fg(
        Level::INFO,
        format!(
            "exit: {why}: GPU drained ({}), {} window swapchain(s) released ({options})",
            if matches!(&polled, Some(Ok(s)) if s.wait_finished()) { "ok" } else { "incomplete" },
            entities.len()
        ),
    );
}

/// The state line, with the present and acquire results since the last one: errors (negative),
/// other non-success results (positive), each `(count, last value)`, and the swapchain out of
/// date or suboptimal apart ([`out_of_date`]).
fn state_line(state: &DlssgState, errors: (u32, i32), other: (u32, i32), out_of_date: u32) -> String {
    let errors = if errors.0 == 0 { "none".to_string() } else { format!("{} (the last VkResult {})", errors.0, errors.1) };
    let mut line = format!(
        "state: status {}, frames to generate at most {}, min size {}, vsync supported {}, dynamic MFG {}, presented for the last frame {}; present and acquire errors since last: {errors}",
        dlssg_status_name(state.status),
        state.num_frames_to_generate_max,
        state.min_width_or_height,
        boolean_name(state.b_is_vsync_support_available),
        boolean_name(state.b_is_dynamic_mfg_supported),
        state.num_frames_actually_presented,
    );
    if out_of_date > 0 {
        line.push_str(&format!(", swapchain out of date {out_of_date}"));
    }
    if other.0 > 0 {
        line.push_str(&format!(", other non-success results {} (the last VkResult {})", other.0, other.1));
    }
    line
}

/// The real-versus-presented line (stage 2).
fn ratio_line(over: Duration, real: u64, presented: u64, failed: Option<&str>) -> String {
    let ratio = presented as f64 / real.max(1) as f64;
    let status = failed.map_or_else(|| "status eOk".to_string(), |names| format!("off since status {names}"));
    format!("{:.1} s: {real} real frames, {presented} presented ({ratio:.2} per real frame), {status}", over.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_passes_shaders_are_valid_wgsl() {
        use wgpu::naga;
        for body in [HUDLESS_WGSL, UI_COLOUR_WGSL, UI_BUFFER_WGSL] {
            let source = format!("{COMMON_WGSL}{body}");
            let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).unwrap_or_else(|e| panic!("{e:?}"));
        }
    }

    /// `encode` of `COMMON_WGSL`, one channel.
    fn encode(x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        if x <= 0.003_130_8 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }
    }

    /// Stored to 8 bits and read back.
    fn unorm8(x: f32) -> f32 {
        (x.clamp(0.0, 1.0) * 255.0).round() / 255.0
    }

    /// The UI colour and alpha (`UI_COLOUR_WGSL`, stored to 8 bits) blended by DLSS-G's rule over
    /// the HUD-less copy (encoded, 8 bits) gives back the real frame as the window holds it
    /// (encoded), for faint glows and opaque HUD over dark and bright scenes; no UI, nothing.
    #[test]
    fn dlss_gs_blend_of_the_ui_colour_gives_back_the_frame() {
        let mut worst: f32 = 0.0;
        for alpha in [0.0, 0.004, 0.02, 0.1, 0.35, 0.7, 1.0] {
            for ui in [0.0, 0.3, 0.9, 1.0] {
                for scene in [0.0, 0.002, 0.05, 0.2, 0.6, 1.0] {
                    let layer_a = alpha;
                    let layer_rgb = ui * alpha; // premultiplied, linear (`fg_ui_layer.rs`)
                    let frame = layer_rgb + (1.0 - layer_a) * scene; // Bevy's blend, linear
                    let hudless = unorm8(encode(scene));
                    let a = unorm8(layer_a);
                    let rgb = if a <= 0.0 { 0.0 } else { unorm8((encode(frame) - (1.0 - a) * hudless).clamp(0.0, 1.0)) };
                    if a <= 0.0 {
                        assert_eq!(rgb, 0.0);
                    }
                    let shown = rgb + (1.0 - a) * hudless;
                    let real = unorm8(encode(frame));
                    worst = worst.max((shown - real).abs());
                }
            }
        }
        assert!(worst <= 1.5 / 255.0, "worst {worst}");
    }

    #[test]
    fn dlss_to_fsr_stays_blocked_until_unload_succeeds() {
        use super::super::ffi::SlResult;
        let shared = AtomicBool::new(true);
        let mut fg = FrameGen::new(dummy_get_state, dummy_set_options, true);
        assert_eq!(window_step(false, Some(false), fg.loaded, false, 0), WindowStep::Hold);
        assert!(shared.load(Ordering::Relaxed), "FSR blocked while the off frame is presented");
        assert_eq!(window_step(false, Some(false), fg.loaded, true, 1), WindowStep::Load(false));
        fg.complete_load(false, SlResult(-1), &shared);
        assert!(fg.loaded && shared.load(Ordering::Relaxed), "a failed unload keeps FSR blocked");
        fg.complete_load(false, SlResult::OK, &shared);
        assert!(!fg.loaded && !shared.load(Ordering::Relaxed), "FSR may take the native surface now");
    }

    #[test]
    fn fsr_to_dlss_waits_for_presenter_release_and_blocks_fsr_after_loading() {
        use super::super::ffi::SlResult;
        let shared = AtomicBool::new(false);
        let mut fg = FrameGen::new(dummy_get_state, dummy_set_options, false);
        assert_eq!(window_step(false, Some(true), fg.loaded, true, 0), WindowStep::Load(true));
        assert!(!switch_ready(true, false), "a presenter still holds the surface, regardless of elapsed frames");
        assert!(!fg.loaded && !shared.load(Ordering::Relaxed));
        assert!(switch_ready(true, true), "Presenter::drop joined its thread and released the surface");
        fg.complete_load(true, SlResult(-1), &shared);
        assert!(!fg.loaded && !shared.load(Ordering::Relaxed), "a failed load stays native");
        fg.complete_load(true, SlResult::OK, &shared);
        assert!(fg.loaded && shared.load(Ordering::Relaxed), "FSR stays blocked until a successful unload");
        assert!(switch_ready(false, true));
    }

    #[test]
    fn the_off_options_keep_resources_and_set_the_callback() {
        let options = off_options();
        assert_eq!((options.mode, options.flags), (0, 8), "eOff, eRetainResourcesWhenOff (sl_dlss_g.h:36, 48)");
        assert!(options.on_error_callback.is_some());
        assert_eq!((options.num_frames_to_generate, options.base.struct_version), (1, 5));
        assert_eq!(options.queue_parallelism_mode, *QUEUE_MODE);
        if std::env::var_os("NVF_DLSSG_QUEUE").is_none() {
            assert_eq!(*QUEUE_MODE, 1, "eBlockNoClientQueues by default (sl_dlss_g.h, second enumerator)");
        }
        assert_eq!(queue_mode_name(1), "eBlockNoClientQueues");
    }

    #[test]
    fn the_on_options_carry_frames_sizes_and_back_buffers() {
        let applied = Applied { on: true, frames: 3, render: UVec2::new(853, 480), output: UVec2::new(1280, 720), back_buffers: 3 };
        let o = options_for(applied);
        assert_eq!((o.mode, o.flags, o.num_frames_to_generate, o.num_back_buffers), (1, 8, 3, 3));
        assert_eq!((o.mvec_depth_width, o.mvec_depth_height, o.color_width, o.color_height), (853, 480, 1280, 720));
        assert_eq!(o.dynamic_res_width, 0, "no dynamic resolution");
        assert_eq!(o.queue_parallelism_mode, *QUEUE_MODE, "the same queue mode in every call");
        assert_eq!(o.enable_user_interface_recomposition, BOOL_TRUE, "the HUD-less and the UI buffer tagged (§6.6)");
        let off = options_for(Applied { on: false, ..applied });
        assert_eq!((off.mode, off.flags), (0, 8));
    }

    #[test]
    fn a_frame_is_generated_only_when_everything_holds() {
        let all = Facts { playing: true, reconstructed: true, constants: true, to_window: true, inputs: true, size_ok: true, failed: false };
        assert!(may_generate(all));
        for (what, f) in [
            ("a menu", Facts { playing: false, ..all }),
            ("the spatial fallback", Facts { reconstructed: false, ..all }),
            ("no constants", Facts { constants: false, ..all }),
            ("a screenshot", Facts { to_window: false, ..all }),
            ("no motion", Facts { inputs: false, ..all }),
            ("too small", Facts { size_ok: false, ..all }),
            ("a failed status", Facts { failed: true, ..all }),
        ] {
            assert!(!may_generate(f), "{what}");
        }
    }

    #[test]
    fn options_change_only_when_needed_and_frames_are_clamped() {
        let on = Applied { on: true, frames: 1, render: UVec2::new(853, 480), output: UVec2::new(1280, 720), back_buffers: 3 };
        assert!(must_set(None, Applied { on: false, ..on }), "the first frame sets them");
        assert!(!must_set(Some(on), on));
        assert!(must_set(Some(on), Applied { on: false, ..on }));
        assert!(must_set(Some(on), Applied { output: UVec2::new(1600, 900), ..on }), "a resize while on");
        let off = Applied { on: false, ..on };
        assert!(!must_set(Some(off), Applied { output: UVec2::new(1600, 900), ..off }), "off stays off whatever the sizes");
        let mut fg = FrameGen::new(dummy_get_state, dummy_set_options, true);
        fg.requested_frames = 3;
        let decision = Decision { generate: true, render: on.render, output: on.output };
        assert_eq!(wanted(&fg, Some(decision)).frames, 1, "the limit not known yet: one");
        fg.max_frames = 5;
        assert_eq!(wanted(&fg, Some(decision)), Applied { frames: 3, back_buffers: 0, ..on });
        fg.max_frames = 1;
        assert_eq!(wanted(&fg, Some(decision)).frames, 1, "clamped to the state's limit");
        assert!(!wanted(&fg, None).on, "no camera tagged: off");
        fg.max_frames = 5;
        fg.window_off = true;
        assert!(!wanted(&fg, Some(decision)).on, "held off for a window change");
        fg.window_off = false;
        fg.loaded = false;
        assert!(!wanted(&fg, Some(decision)).on, "unloaded");
        fg.loaded = true;
        fg.status_failed = Some("eFailReflexNotDetectedAtRuntime".into());
        assert!(!wanted(&fg, Some(decision)).on, "a failed status keeps it off");
    }

    unsafe extern "C" fn dummy_get_state(_: *const ViewportHandle, _: *mut DlssgState, _: *const DlssgOptions) -> super::super::ffi::SlResult {
        super::super::ffi::SlResult::OK
    }

    unsafe extern "C" fn dummy_set_options(_: *const ViewportHandle, _: *const DlssgOptions) -> super::super::ffi::SlResult {
        super::super::ffi::SlResult::OK
    }

    /// Calls of [`counting_set_options`] and the mode of the last one.
    static SET_CALLS: AtomicU32 = AtomicU32::new(0);
    static SET_MODE: AtomicI32 = AtomicI32::new(-1);

    unsafe extern "C" fn counting_set_options(_: *const ViewportHandle, options: *const DlssgOptions) -> super::super::ffi::SlResult {
        SET_CALLS.fetch_add(1, Ordering::SeqCst);
        // SAFETY: the options `exit_mode_off` passes, alive for the call.
        SET_MODE.store(unsafe { (*options).mode } as i32, Ordering::SeqCst);
        super::super::ffi::SlResult::OK
    }

    #[test]
    fn an_exit_sets_the_mode_off_once_whatever_ends_the_run() {
        let mut fg = FrameGen::new(dummy_get_state, counting_set_options, true);
        fg.applied = Some(Applied { on: true, frames: 1, render: UVec2::new(853, 480), output: UVec2::new(1280, 720), back_buffers: 0 });
        assert!(exit_mode_off(&mut fg, "the run hands over to a new one"), "the first exit sets the mode off");
        assert_eq!(SET_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(SET_MODE.load(Ordering::SeqCst), DLSSG_MODE_OFF as i32);
        assert!(fg.exiting && fg.exit_off && !fg.applied.unwrap().on, "the mode stays off for the frames left");
        assert!(!exit_mode_off(&mut fg, "the window closes"), "a second exit path does nothing");
        assert_eq!(SET_CALLS.load(Ordering::SeqCst), 1, "slDLSSGSetOptions called once");
        assert!(!wanted(&fg, Some(Decision { generate: true, render: UVec2::new(853, 480), output: UVec2::new(1280, 720) })).on, "no frame turns it on again");

        let mut unloaded = FrameGen::new(dummy_get_state, counting_set_options, false);
        assert!(!exit_mode_off(&mut unloaded, "the run hands over to a new one"), "unloaded: no DLSS-G call");
        assert!(unloaded.exiting && unloaded.exit_off);
        assert_eq!(SET_CALLS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_window_change_waits_for_an_off_present_once() {
        use WindowStep::*;
        assert_eq!(window_step(false, None, true, false, 0), Go { off: false }, "nothing pending");
        assert_eq!(window_step(true, None, true, false, 0), Hold, "on at the last present: off first");
        assert_eq!(window_step(true, None, true, true, 1), Go { off: true }, "off was presented: reconfigure, still off");
        assert_eq!(window_step(true, None, true, false, MAX_HOLD), Go { off: true }, "never presented: no longer held");
        assert_eq!(window_step(true, None, false, false, 0), Go { off: true }, "unloaded: nothing to hold for");
        assert_eq!(window_step(false, Some(false), true, false, 0), Hold, "switching off: an off present first");
        assert_eq!(window_step(false, Some(false), true, true, 1), Load(false));
        assert_eq!(window_step(false, Some(true), false, true, 0), Load(true), "switching on: at once (native presents)");
        assert_eq!(window_step(false, Some(true), true, false, 0), Go { off: false }, "already on: nothing to switch");
    }

    #[test]
    fn the_state_and_ratio_lines() {
        let mut state = DlssgState::new();
        state.num_frames_to_generate_max = 5;
        state.min_width_or_height = 100;
        state.num_frames_actually_presented = 2;
        assert_eq!(
            state_line(&state, (0, 0), (0, 0), 0),
            "state: status eOk, frames to generate at most 5, min size 100, vsync supported eFalse, dynamic MFG eFalse, presented for the last frame 2; present and acquire errors since last: none"
        );
        assert!(state_line(&state, (2, -4), (3, 5), 0).ends_with("errors since last: 2 (the last VkResult -4), other non-success results 3 (the last VkResult 5)"));
        assert!(state_line(&state, (0, 0), (0, 0), 2).ends_with("errors since last: none, swapchain out of date 2"), "a resize is not an error");
        state.status = 0b10;
        assert!(state_line(&state, (0, 0), (0, 0), 0).starts_with("state: status eFailReflexNotDetectedAtRuntime,"));
        assert_eq!(ratio_line(Duration::from_secs(10), 600, 1200, None), "10.0 s: 600 real frames, 1200 presented (2.00 per real frame), status eOk");
        assert!(ratio_line(Duration::from_secs(10), 0, 0, Some("eFailCommonConstantsInvalid")).ends_with("(0.00 per real frame), off since status eFailCommonConstantsInvalid"));
    }

    #[test]
    fn the_error_callback_counts_errors_and_other_results_apart() {
        let (errors, other, stale) = (API_ERRORS.load(Ordering::Relaxed), API_OTHER.load(Ordering::Relaxed), API_OUT_OF_DATE.load(Ordering::Relaxed));
        // SAFETY: valid APIErrors, as Streamline passes; a null one is ignored.
        unsafe {
            on_api_error(&ApiError { vk_res: -4 });
            on_api_error(&ApiError { vk_res: 1_000_018_000 });
            on_api_error(&ApiError { vk_res: -1_000_001_004 });
            on_api_error(&ApiError { vk_res: 1_000_001_003 });
            on_api_error(std::ptr::null());
        }
        assert!(API_ERRORS.load(Ordering::Relaxed) > errors);
        assert!(API_OTHER.load(Ordering::Relaxed) > other);
        assert!(API_OUT_OF_DATE.load(Ordering::Relaxed) >= stale + 2, "out of date and suboptimal");
        assert_eq!((LAST_API_ERROR.load(Ordering::Relaxed), LAST_API_OTHER.load(Ordering::Relaxed)), (-4, 1_000_018_000));
        assert!(out_of_date(-1_000_001_004) && out_of_date(1_000_001_003) && !out_of_date(-4));
    }
}
