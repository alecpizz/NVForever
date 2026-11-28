//! DLSS through NVIDIA Streamline (#2198): Streamline's interposer loaded and initialised, the
//! Vulkan instance made through it, DLSS's support checked, DLSS Super Resolution in every mode
//! drawing a camera in its own pass (`evaluate.rs`), and Streamline shut down in order at exit.
//! The loader runs when the start-up choice is DLSS (the settings' Upscaler row or the command
//! line) or the four files are in the folder ([`bundle_present`], a cheap existence check), on
//! Windows with Vulkan among the renderer's backends, so with the files there every switch of
//! the upscaler is live (`upscaler::UpscalerStartup`); the test aid `NVF_DLSS_PRELOAD=1` runs
//! it always, `=0` only for a DLSS choice. `main` adds [`DlssInitPlugin`] only then (otherwise
//! nothing is loaded or logged here).
//!
//! - **Files** [design]: `<program folder>\dlss\` (or `NVF_DLSS_DIR=<absolute path>`) holding
//!   `sl.interposer.dll`, `sl.common.dll`, `sl.dlss.dll` and `nvngx_dlss.dll`; with any missing,
//!   the stock path and one line saying which. NVForever ships none of them.
//! - **Start** (before `DefaultPlugins`, Windows only, and only when the renderer's backends
//!   include Vulkan, as Bevy picks them from `WGPU_BACKEND`): the interposer's signature checked
//!   (`security.rs`), the library loaded by full path and kept for the process, `slInit` with
//!   Vulkan, DLSS, the folder as the plugin path, the over-the-air flags off and an engine
//!   version and project id (without both Streamline keeps NGX off when no NVIDIA application
//!   id is given [documented: `src/commonEntry.cpp:1494-1535`]). Then Bevy's raw Vulkan init is
//!   told to make the instance from the interposer (vendored bevy_render,
//!   `RawVulkanInitSettings::set_vulkan_library`). Any failure: today's path.
//! - **Preflight** (`preflight.rs`): the folder is listed before the interposer is loaded and
//!   again just before `slInit`; an `nvngx_update.exe` in it, in any case, or a folder that
//!   cannot be listed keeps Streamline out (Streamline would start that file unchecked), and so
//!   do a folder or DLL that is a link, a release DLL with another hash, an `nvngx_*.dll` not
//!   signed by NVIDIA, or any other DLL. The checked files stay open until the shutdown. Every
//!   GPU Vulkan exposes, of any kind, is read through the system's Vulkan loader before the
//!   interposer is loaded, and after `slInit` checked against `slGetFeatureRequirements(DLSS)`
//!   plus what Streamline's `vkCreateDevice` always adds: Bevy can fall back from any adapter
//!   it is asked for to wgpu's own pick [documented: vendor/bevy_render/src/renderer/mod.rs:
//!   166-194, 304], so no GPU is left out. Any GPU short of it, or none at all, and `slShutdown`
//!   runs and the stock path follows, before `set_vulkan_library`. With an NVIDIA GPU beside
//!   another maker's (a CPU's integrated graphics, #2921), a second snapshot is taken with the
//!   loader limited to NVIDIA's driver (`VK_LOADER_DRIVERS_SELECT`, `win::narrowed_probe`);
//!   when every GPU fails and that one passes, showing the same NVIDIA GPUs and nothing else,
//!   the filter is kept for the run so the renderer sees only what was checked. A filter or
//!   `WGPU_ADAPTER_NAME` the player set stands.
//!   [`loader_state`] says what the loader ended in.
//!   Clearing the two update flags does not stop Streamline starting NVIDIA's updater, and a
//!   driver override can still pick downloaded plugins [documented: `src/pluginManager.cpp:
//!   966-968, 994-1005`]; watch for that in runs.
//! - **Rollback**: if the instance cannot be made from the interposer after `slInit` worked,
//!   `slShutdown` runs before the renderer falls back to the system's loader, and DLSS
//!   stays off. There is no fallback once the device is being made: wgpu-hal panics when
//!   Streamline's device proxy refuses for missing extensions [documented: wgpu-hal 30.0.1
//!   `vulkan/adapter.rs:3044-3050`].
//! - **Adapter pin** (after the device check, `preflight::pin`): when `WGPU_ADAPTER_NAME` is
//!   unset and exactly one checked GPU is NVIDIA's and its name picks it alone, the renderer is
//!   asked for it by name ([`render_plugin`], Bevy's `WgpuSettings::adapter_name`). That only
//!   prefers the NVIDIA GPU among the checked ones (Bevy takes its own pick when no adapter of
//!   that name supports the window's surface); what keeps the renderer off an unchecked GPU is that the device
//!   check covers every GPU Vulkan exposes. Once the device exists the GPU Bevy chose is
//!   compared with the checked one (name and LUID); a mismatch keeps DLSS off.
//! - **Support** (once the device exists, `Plugin::cleanup`): `slIsFeatureSupported` with the
//!   physical device and its LUID (Streamline skips the adapter check without a LUID
//!   [documented: `src/sl.cpp:855-860`]), the versions Streamline reports, and the evidence that
//!   the device came through its proxies (the instance marker; Streamline's own log lines).
//!   With DLSS supported, its two functions fetched and the adapter as checked, DLSS is
//!   **active** ([`loader_state`] `Ready`).
//! - **Which camera** [design]: the one the graphics settings give `upscaler::DlssUpscaling`,
//!   which they do while DLSS is the effective choice (`upscaler::resolve`, from the requested
//!   choice and this session's facts: [`session_state`], [`streamline_loaded`]). Cameras with
//!   `fsr::FsrUpscaling` are FSR's; nothing here touches them.
//! - **Sizes** (`sizing.rs`): the view preparation (`upscaler::prep::prepare`) asks
//!   `slDLSSGetOptimalSettings` for the mode, output size and HDR-ness, once per change, and
//!   draws at the size DLSS reports, with DLSS's jitter phases and mip bias.
//! - **Options** ([`configure`], a render system after the view preparation, before anything of
//!   the frame is recorded): `slDLSSSetOptions` when the mode, output size, HDR-ness or the DLSS
//!   model (the settings' DLSS model row, `Graphics::dlss_model`, [`DlssMainState`]) changes;
//!   when DLSS may already hold resources for the viewport, the GPU is drained (`Device::poll`
//!   wait) and `slFreeResources` runs first, so no feature is released while the GPU may still
//!   use it (Streamline itself releases a replaced feature three frames later [documented:
//!   `src/dlssEntry.cpp:357-369`]). When DLSS stops drawing (the effective upscaler is FSR or
//!   off) with options set, the same drain and `slFreeResources` free its resources at once
//!   rather than at the shutdown, and the next DLSS frame sets the options again.
//! - **Evaluate** ([`dlss_pass`], DLSS's own pass, where `fsr::fsr_pass` sits for FSR: after the
//!   ambient occlusion, before bloom, the bright pass and the other full-resolution effects): the
//!   common corrected depth, one DLSS evaluate (`evaluate.rs` says the order), the scene's alpha
//!   restored where a shape marks it, the depth expanded. No FSR state is made for it. Any
//!   failure: the common spatial fallback draws that frame and the history is invalidated;
//!   [`FAILED_FRAMES`] frames running without DLSS's output and DLSS is unavailable for the
//!   session ([`session_state`]): the status then draws the camera natively. FSR's frame
//!   generation never runs on a DLSS camera (`frame_gen::usable` asks for FSR drawing).
//! - **Output proof** (`evaluate.rs` `OutputProof`): `slEvaluateFeature` answers `eOk` even
//!   when NGX's evaluate failed (Streamline's DLSS plugin ignores NGX's result [documented:
//!   `src/dlssEntry.cpp:738`]), so a call that worked is not output. Once per generation of
//!   DLSS's options (each `slDLSSSetOptions`: the start, a mode change, a resize) a grid of the
//!   output's texels is read back asynchronously right after the evaluate, before anything
//!   else writes the output, which is cleared to black just before the evaluate: finite RGB
//!   that is not all zero can only be DLSS's (`output observed`); all black is inconclusive (a
//!   black scene looks the same) and is tried again later, a bounded number of times. Beside
//!   it, the lines `sl.common` logs when NGX's evaluate fails (`NGX evaluate feature ...
//!   failed` [documented: `src/commonEntry.cpp:792, 1047`]) are latched from Streamline's log
//!   callback (`note_ngx_line`) and each counts as a frame without DLSS's output toward
//!   [`FAILED_FRAMES`]. Neither proves image quality (motion, stability): that still takes
//!   pictures and movement [design: the plan's risk P2].
//! - **Shutdown** ([`shutdown`], from `main` after `App::run` returns): by then the app,
//!   its render world and its render thread are gone (Bevy waits for the render app to come back
//!   before dropping it [design: vendor/bevy_render/src/pipelined_rendering.rs:67-80]), but the
//!   loader still holds its own clones of the device, queue, adapter and instance, so the Vulkan
//!   device and instance are alive. The GPU is drained (`Device::poll` wait, then
//!   `vkDeviceWaitIdle`), DLSS's resources for the viewport are freed (`slFreeResources`, which
//!   needs pending evaluates flushed [documented: `sl_core_api.h:233-237`]), `slShutdown` runs, and only then are the clones dropped, the last
//!   handles, so the device and instance are destroyed after it. Streamline's cleanup uses the
//!   device it stored [documented: `src/vulkan.cpp:1349-1407`]. Then the DLSS folder's checked
//!   files, held open since the folder pass (`win.rs` `Pins`), are released. The interposer stays
//!   loaded.
//! - **Ending at once** ([`end_now`], #2733): a loaded save's handoff ends the old run with
//!   `std::process::exit` from the main schedule (`handoff.rs` `end_now`), so `App::run` never
//!   returns and [`shutdown`] above never ran: Streamline and DLSS-G were left to the DLLs'
//!   detach at process exit. In a player's runs every such exit with DLSS-G set up ended in a
//!   crash in `nvngx_dlssg.dll` [observed: the player's logs and dumps, 2026-11-15]; that the
//!   detach without `slShutdown` is what crashes is a [hypothesis] this change tests. Before
//!   that exit, with the device made through Streamline: the render app is taken back from the
//!   render thread (Bevy renders nothing after), FSR frame generation's presenter threads are
//!   stopped and joined (`crate::frame_gen::stop_presenters`: they present on threads of their
//!   own and hold the window's surface), device polling and pipeline creation are held off
//!   (`gpu_poll::quiet`), DLSS-G's mode goes off and the window's swapchain is released
//!   (`frame_gen.rs`, "Exit"), and [`shutdown`] runs as above. The render world is not dropped
//!   (the point of ending at once); so the swapchain is released by hand and the device
//!   outlives `slShutdown`, as it does above.
//!
//!   The old run now waits, without a time limit, for the render thread's frame, the
//!   presenters' last presents, the pipeline gate (a pipeline being compiled) and the GPU drain
//!   (`Device::poll` wait, `vkDeviceWaitIdle`). Not bounded [design]: every other exit waits
//!   for the same things without a limit (Bevy's `RenderAppChannels::drop` for the render
//!   app, `Presenter::drop`, `win.rs` `shutdown`'s drain), and a limit that gave up would
//!   leave exactly the exit that crashed. The new run is already shown by then (its loading
//!   screen may still be up), so a short wait is not seen; the cost is the old process's RAM
//!   and video memory held for as long as it waits, which a long wait would take from the new
//!   run's loading or play. A renderer or driver that never returns would leave the old
//!   process behind the new window holding that memory, as it would hang any exit. The
//!   handoff's own limits (`handoff.rs` `ACK_WAIT_SECS`, `GIVE_UP_MARGIN_SECS`) time the new
//!   run's start, not this.
//!
//! - **Frame generation** (#2317; docs/design/dlss-frame-generation.md): with the four files of
//!   [`FG_FILES`] in the folder beside the four above, `slInit` asks for DLSS, DLSS Frame
//!   Generation and Reflex with frame-based tagging; DLSS-G's and Reflex's requirements are
//!   logged and merged into the device check, and when that or `slInit` fails, Streamline is shut
//!   down and initialised again with DLSS alone. Without the four files DLSS runs as before (one
//!   line says which are missing). Once the device exists, DLSS-G stays loaded when the start's
//!   Frame generation choice is DLSS and is unloaded before the window's swapchain otherwise;
//!   Reflex stays loaded, and NVIDIA low latency goes through it ([`frame_generation_on`],
//!   `nv_low_latency.rs`). `frame_gen.rs` follows the Frame generation row (on, off, the frames
//!   to generate), sets the constants once a frame for FSR or DLSS, tags the depth, motion, a
//!   copy of the frame without the HUD and a UI alpha on frames that may be generated, sets
//!   DLSS-G's options on changes, reads its state after every present, holds a window change
//!   until a frame was presented with DLSS-G off, and loads or unloads DLSS-G live with the
//!   swapchain made again; `frame_token.rs` gives every Streamline call our frame number.
//!   FSR's frame generation never runs while DLSS-G is loaded (`crate::frame_gen::usable`).
//!
//! Log lines start `DLSS:` (`DLSS-G:` for frame generation's own); lines before the log is up
//! wait in a buffer, written each frame (`First`) and at shutdown.

#[cfg(any(windows, test))]
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Mutex, PoisonError};

use bevy::core_pipeline::prepass::{PreviousViewData, ViewPrepassTextures};
use bevy::log::Level;
use bevy::prelude::*;
use bevy::render::camera::TemporalJitter;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};

use crate::upscaler::prep::{self, PreparedUpscale, UpscaleRenderer, UpscaleSettings};
use crate::upscaler::{DlssModel, DlssState, UpscaleQuality, Unavailable, Upscaler};

#[cfg(windows)]
mod evaluate;
#[cfg(windows)]
mod ffi;
// The download, the signature checks and the folder rule live in `nvf-dlss-files`, shared with
// the launcher (#2765); they keep their paths here.
pub(crate) use nvf_dlss_files::download;
#[cfg(windows)]
mod frame_gen;
#[cfg(windows)]
mod frame_token;
#[cfg(any(windows, test))]
mod preflight;
#[cfg(windows)]
use nvf_dlss_files::security;
pub(crate) mod sizing;
#[cfg(windows)]
mod win;

pub(crate) use sizing::OptimalSizes;

/// The folder rule, the files it must hold (the interposer first) and the frame
/// generation files (`nvf-dlss-files`).
#[cfg(any(windows, test))]
use nvf_dlss_files::{resolve_folder, FolderSource, FG_FILES, FILES};
/// The folder's variable, read where the loader runs (Windows).
#[cfg(windows)]
use nvf_dlss_files::ENV_DIR;

/// Whether DLSS Frame Generation can run this session, as far as the loader knows: the files
/// (none of the four frame generation files, or not the loader at all), the GPU's support (from
/// `slIsFeatureSupported(kFeatureDLSS_G)`, `sl_result.h:32-75`), or ready. For the settings
/// page's reasons ([`frame_generation_unavailable`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
#[cfg_attr(not(windows), allow(dead_code))]
enum FgAvailability {
    /// The four frame generation files are not all in the folder.
    FilesMissing = 1,
    /// `eErrorAdapterNotSupported` or `eErrorNoSupportedAdapterFound`.
    GpuNotSupported = 2,
    /// `eErrorOSDisabledHWS`: Windows' hardware-accelerated GPU scheduling is off.
    SchedulingOff = 3,
    /// `eErrorDriverOutOfDate`.
    DriverOld = 4,
    /// Any other refusal (slInit, the device check, a function not handed out).
    Failed = 5,
    Ready = 6,
}

/// 0 until the loader gets as far as the files (no DLSS files, not Vulkan: as without them).
static FG_AVAILABILITY: AtomicU8 = AtomicU8::new(0);

#[cfg_attr(not(windows), allow(dead_code))]
fn set_fg_availability(availability: FgAvailability) {
    FG_AVAILABILITY.store(availability as u8, Ordering::Relaxed);
}

/// Why DLSS Frame Generation cannot be chosen this session, in the settings page's words;
/// `None` when it can (the Frame generation row's DLSS choice, `crate::frame_gen`).
pub(crate) fn frame_generation_unavailable(download_offered: impl FnOnce() -> bool) -> Option<&'static str> {
    frame_generation_reason(cfg!(windows), FG_AVAILABILITY.load(Ordering::Relaxed), download::current().map(|h| h.progress().phase).as_ref(), download_offered)
}

/// [design] Only missing-file refusals suggest a download or restart; the caller supplies
/// whether its Upscaler row actually offers the download.
fn frame_generation_reason(windows: bool, availability: u8, phase: Option<&download::Phase>, download_offered: impl FnOnce() -> bool) -> Option<&'static str> {
    if !windows {
        return Some("Not on this GPU");
    }
    match availability {
        6 => None,
        2 => Some("Needs an NVIDIA RTX 40 series or newer"),
        3 => Some("Needs hardware-accelerated GPU scheduling"),
        4 => Some("Needs a newer NVIDIA driver"),
        5 => Some("Not on this GPU"),
        _ if matches!(phase, Some(download::Phase::Done(_))) => Some("Restart to use DLSS Frame Generation"),
        _ if download_offered() => Some("Files missing: Download DLSS… on the Upscaler row"),
        _ => Some("The DLSS Frame Generation files are missing"),
    }
}

/// Whether the start's Frame generation choice is DLSS (`main`, from the command line and the
/// INI as the row's own rule reads them): DLSS-G stays loaded from the start; otherwise it is
/// unloaded before the window's swapchain is made and loaded when chosen (`frame_gen.rs`).
static FG_AT_START: AtomicBool = AtomicBool::new(false);

/// `main` tells the loader the start's choice ([`FG_AT_START`]).
pub(crate) fn set_frame_generation_at_start(dlss: bool) {
    FG_AT_START.store(dlss, Ordering::Relaxed);
}

/// The most frames DLSS-G can generate per real frame on this GPU (`DLSSGState::
/// numFramesToGenerateMax`, read after every present); 0 before it is known.
static FG_MAX_FRAMES: AtomicU32 = AtomicU32::new(0);

/// [`FG_MAX_FRAMES`], for the Generated frames row.
pub(crate) fn frame_generation_max() -> u32 {
    FG_MAX_FRAMES.load(Ordering::Relaxed)
}

/// DLSS Frame Generation is loaded now (#2317): set at the start once DLSS-G was requested in
/// `slInit` and the device check passed with it, before any plugin's `finish`; cleared when it is
/// unloaded (not chosen at the start, refused, switched off live) and set again when it is
/// loaded (`win.rs`, `frame_gen.rs`), cleared at shutdown.
static FG_KEPT_LOADED: AtomicBool = AtomicBool::new(false);

/// Whether Streamline's DLSS Frame Generation is loaded now (before `Plugin::cleanup`: was asked
/// for), so the window's swapchain is Streamline's proxy: FSR's frame generation never runs then
/// (`crate::frame_gen::usable`). Always false off Windows and without the frame generation files.
pub(crate) fn frame_generation_loaded() -> bool {
    FG_KEPT_LOADED.load(Ordering::Relaxed)
}

/// DLSS Frame Generation and Reflex were set up this session (#2317): DLSS-G and Reflex asked
/// for in `slInit` with the files there and the device check passed; Reflex stays loaded for
/// the session. Cleared when frame generation does not start after all.
static FG_ON: AtomicBool = AtomicBool::new(false);

/// Whether Streamline's DLSS Frame Generation and Reflex were set up this session: NVIDIA low
/// latency then goes through Streamline's Reflex while it runs (`nv_low_latency::load`). Always
/// false off Windows and without the frame generation files.
pub(crate) fn frame_generation_on() -> bool {
    FG_ON.load(Ordering::Relaxed)
}

/// DLSS-G ended frame generation for the session (a status other than `eOk` while on,
/// `frame_gen.rs` `state`). Reflex stays loaded; vsync and the frame cap go back to frame
/// pacing's own ([`frame_generation_running`]).
static FG_ENDED: AtomicBool = AtomicBool::new(false);

/// The frame limit Reflex is to keep, in microseconds per frame shown (0: none), from frame
/// pacing's cap while frame generation runs (`frame_pace.rs` `wait_for_cap`).
static REFLEX_LIMIT_US: AtomicU32 = AtomicU32::new(0);

/// Whether DLSS frame generation runs: [`frame_generation_on`], DLSS-G loaded (chosen) and not
/// ended by a failed status. While it does, latency reduction is on with it (`crate::frame_gen`
/// `follow_switch`, `LatencyReduction::by_frame_generation`) and the frame cap is Reflex's
/// (`frame_pace.rs`), also in menus, where DLSS-G's mode is off for a while but frame generation
/// is not over.
pub(crate) fn frame_generation_running() -> bool {
    frame_generation_on() && !FG_ENDED.load(Ordering::Relaxed) && frame_generation_loaded()
}

/// The frame limit Reflex keeps (microseconds per frame shown, 0: none), set by frame pacing
/// while frame generation runs; applied with Reflex's options at the next frame's input
/// ([`reflex_options`]).
pub(crate) fn set_reflex_frame_limit(frame_limit_us: u32) {
    REFLEX_LIMIT_US.store(frame_limit_us, Ordering::Relaxed);
}

/// Streamline's Reflex runs this session: kept loaded and its calls installed
/// (`frame_token.rs`). Every latency call then goes through it (`nv_low_latency.rs`).
pub(crate) fn reflex_running() -> bool {
    #[cfg(windows)]
    {
        frame_token::active()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Reflex's options for the latency reduction switch (`eLowLatency` on, `eOff` off) and the
/// frame limit frame pacing set ([`set_reflex_frame_limit`]), set only when either changes or a
/// swapchain was made since they were set, which does not have them (`frame_token.rs`, "Reflex
/// and the swapchain"; #2732) (`slReflexSetOptions` is not thread safe: the main thread,
/// `nv_low_latency.rs` at each frame's input). Each is said, with the swapchains made so far:
/// `Reflex options: … (swapchains made: N)` for a change, `Reflex options set again for a new
/// swapchain: … (swapchains made: N)` for the other. A refused call is made again each frame; a
/// refusal repeated frame after frame is said once, and the call that works after it says how
/// many tries failed (`frame_token::Retries`). `Err`: Streamline refused, or Reflex does not run.
pub(crate) fn reflex_options(on: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        use ffi::{REFLEX_MODE_LOW_LATENCY, REFLEX_MODE_OFF};
        static RETRIES: Mutex<frame_token::Retries> = Mutex::new(frame_token::Retries::new());
        let mode = if on { REFLEX_MODE_LOW_LATENCY } else { REFLEX_MODE_OFF };
        let limit = REFLEX_LIMIT_US.load(Ordering::Relaxed);
        match frame_token::set_reflex_options(mode, limit) {
            None => Ok(()),
            Some((why, swapchains, result)) => {
                let options = format!("mode {}, frame limit {}: {}", frame_token::reflex_mode_name(mode), limit_text(limit), result.name());
                let head = match why {
                    frame_token::SetWhy::Changed => "Reflex options",
                    frame_token::SetWhy::NewSwapchain => "Reflex options set again for a new swapchain",
                };
                let mut retries = RETRIES.lock().unwrap_or_else(PoisonError::into_inner);
                if result.is_ok() {
                    let failed = retries.worked();
                    let after = if failed > 0 { format!(", after {failed} refused tries") } else { String::new() };
                    note_fg(Level::INFO, format!("{head}: {options} (swapchains made: {swapchains}{after})"));
                    Ok(())
                } else {
                    if retries.failed((mode, limit, result.0)) {
                        note_fg(Level::WARN, format!("{head}: {options} (swapchains made: {swapchains}); tried again each frame, the same refusal said once"));
                    }
                    Err(format!("slReflexSetOptions {}", result.name()))
                }
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = on;
        Err("Streamline is Windows only".to_string())
    }
}

/// A Reflex frame limit for the log.
#[cfg_attr(not(windows), allow(dead_code))]
fn limit_text(frame_limit_us: u32) -> String {
    if frame_limit_us == 0 { "none".to_string() } else { format!("{frame_limit_us} us a frame shown") }
}

/// The `NVF_PACE_LOG` line from `slReflexGetState` while Streamline's Reflex runs (the main
/// thread; not thread safe); `None` otherwise.
pub(crate) fn reflex_report_line() -> Option<String> {
    #[cfg(windows)]
    {
        frame_token::reflex_report()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Reflex's sleep through Streamline for the frame whose marker number is `present_id`
/// (`nv_low_latency.rs` `present_id`, the engine frame plus one: the number `frame_token.rs`
/// `index_of` gives the frame's constants and tags), while [`frame_generation_on`]; `false` when
/// it was not made (not installed, or refused).
pub(crate) fn reflex_sleep(present_id: u64) -> bool {
    #[cfg(windows)]
    {
        frame_token::sleep(frame_token::number(present_id)).is_ok()
    }
    #[cfg(not(windows))]
    {
        let _ = present_id;
        false
    }
}

/// A latency marker through Streamline's PCL for the frame whose marker number is `present_id`
/// (as [`reflex_sleep`]), while [`frame_generation_on`]: the driver's marker mapped to PCL's
/// (none for `INPUT_SAMPLE`, which PCL no longer has).
pub(crate) fn reflex_mark(marker: ash::vk::LatencyMarkerNV, present_id: u64) {
    #[cfg(windows)]
    if let Some(pcl) = frame_token::pcl_marker(marker) {
        frame_token::mark(pcl, frame_token::number(present_id));
    }
    #[cfg(not(windows))]
    let _ = (marker, present_id);
}

/// When the DLSS passes were added (app construction): the clock of the Frame Generation
/// test aid `NVF_DLSSG_RESIZE`.
static TEST_START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// Seconds since [`TEST_START`].
fn test_seconds() -> f64 {
    TEST_START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
}

/// `NVF_DLSSG_RESIZE=<seconds>:<w>x<h>,...`: the window resized during play (a test aid for the
/// Frame Generation's window changes, `frame_gen.rs` `window_changes`; seconds since
/// [`TEST_START`]). No other aid resizes outside the pause menu.
const RESIZE_ENV: &str = "NVF_DLSSG_RESIZE";

/// [`RESIZE_ENV`]'s steps.
#[derive(Resource, Default)]
struct ResizePlan {
    steps: Vec<(f64, UVec2)>,
    next: usize,
}

/// `NVF_DLSSG_RESIZE` read: `<seconds>:<w>x<h>` steps, in time order.
fn resizes_from(value: &str) -> Result<Vec<(f64, UVec2)>, String> {
    let mut steps = Vec::new();
    for item in value.split(',').map(str::trim).filter(|i| !i.is_empty()) {
        let parsed = item.split_once(':').and_then(|(at, size)| {
            let at = at.trim().parse::<f64>().ok().filter(|t| t.is_finite() && *t >= 0.0)?;
            let (w, h) = size.trim().split_once('x')?;
            let size = UVec2::new(w.trim().parse().ok()?, h.trim().parse().ok()?);
            (size.min_element() >= 64).then_some((at, size))
        });
        match parsed {
            Some(step) => steps.push(step),
            None => return Err(format!("{RESIZE_ENV}: '{item}' is not <seconds>:<width>x<height> (at least 64x64); no resize")),
        }
    }
    steps.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(steps)
}

/// The resize test aid: the primary window's physical size set at each step's time.
fn resize_aid(mut plan: ResMut<ResizePlan>, mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>) {
    let Some(&(at, size)) = plan.steps.get(plan.next) else { return };
    if test_seconds() < at {
        return;
    }
    plan.next += 1;
    for mut window in &mut windows {
        window.resolution.set_physical_resolution(size.x, size.y);
        info!("test aid: window resized to {}x{} ({RESIZE_ENV}, {at} s)", size.x, size.y);
    }
}

/// The files of [`FG_FILES`] that `folder` lacks, in that order.
#[cfg(any(windows, test))]
fn missing_fg_files(folder: &Path) -> Vec<&'static str> {
    FG_FILES.iter().copied().filter(|name| !folder.join(name).is_file()).collect()
}

/// Lines waiting for the log: written before the log is up, or by Streamline's own log thread.
static PENDING: Mutex<Vec<(Level, String)>> = Mutex::new(Vec::new());
/// Lines kept at most between two drains; past that they are counted, not kept.
const PENDING_MAX: usize = 4096;
static DROPPED: Mutex<usize> = Mutex::new(0);

fn push(level: Level, text: String) {
    let mut pending = PENDING.lock().unwrap_or_else(PoisonError::into_inner);
    if pending.len() < PENDING_MAX {
        pending.push((level, text));
    } else {
        *DROPPED.lock().unwrap_or_else(PoisonError::into_inner) += 1;
    }
}

/// A line of DLSS's for the log.
fn note(level: Level, text: impl AsRef<str>) {
    push(level, format!("DLSS: {}", text.as_ref()));
}

/// A line of DLSS Frame Generation's (#2317) for the log.
#[cfg(windows)]
fn note_fg(level: Level, text: impl AsRef<str>) {
    push(level, format!("DLSS-G: {}", text.as_ref()));
}

/// A line of Streamline's own log (its callback) for the log.
#[cfg(windows)]
fn note_sl(level: Level, text: &str) {
    push(level, format!("DLSS: sl: {text}"));
}

/// Whether a line of Streamline's log is `sl.common` reporting that NGX's evaluate failed: its
/// format strings are `"[%s] NGX evaluate feature failed 0x%x"` and, for D3D12, `"[%s] NGX
/// evaluate feature (D3D12) failed 0x%x"` [documented: `src/commonEntry.cpp:792, 1047`]; the
/// callback gets them after Streamline's own header (`src/log.cpp:352-371`). The create
/// failure (`"NGX create feature failed"`, `:768`) is not one: DLSS's evaluate then returns
/// `eErrorNGXFailed` itself [documented: `src/dlssEntry.cpp:472-483`], counted as a failed call.
#[cfg_attr(not(windows), allow(dead_code))]
fn ngx_evaluate_error(line: &str) -> bool {
    line.find("NGX evaluate feature").is_some_and(|at| line[at..].contains(" failed 0x"))
}

/// The NGX evaluate errors Streamline's log reported: atomics only, because the log callback
/// runs on Streamline's log thread, possibly while a Streamline call made with the loader's
/// lock held is under way (so it never takes that lock).
struct NgxLatch {
    /// Reported and not yet taken by the failure count ([`record`]).
    pending: AtomicU32,
    /// Reported in all, for the shutdown.
    total: AtomicU64,
    /// The options' generation whose first error was said (`u64::MAX`: none yet).
    said: AtomicU64,
}

impl NgxLatch {
    const fn new() -> Self {
        Self { pending: AtomicU32::new(0), total: AtomicU64::new(0), said: AtomicU64::new(u64::MAX) }
    }

    /// One line of Streamline's error log during `generation`: latched when it is an NGX
    /// evaluate error, and the WARN to say returned the first time in the generation.
    #[cfg_attr(not(windows), allow(dead_code))]
    fn latch(&self, line: &str, generation: u64) -> Option<String> {
        if !ngx_evaluate_error(line) {
            return None;
        }
        self.pending.fetch_add(1, Ordering::Relaxed);
        self.total.fetch_add(1, Ordering::Relaxed);
        (self.said.swap(generation, Ordering::Relaxed) != generation).then(|| format!("NGX reported an evaluate error: {line}"))
    }

    /// The errors reported since the last take.
    fn take(&self) -> u32 {
        self.pending.swap(0, Ordering::Relaxed)
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }
}

static NGX: NgxLatch = NgxLatch::new();

/// The generation of DLSS's options now (`evaluate.rs` `OutputProof`; 0 before the first),
/// for the log callback, which cannot take the loader's lock.
#[cfg_attr(not(windows), allow(dead_code))]
static GENERATION: AtomicU64 = AtomicU64::new(0);

#[cfg(windows)]
fn set_generation(generation: u64) {
    GENERATION.store(generation, Ordering::Relaxed);
}

/// An error line of Streamline's log (`win.rs` `sl_log`): an NGX evaluate error is latched
/// ([`NgxLatch`]) and said once per generation.
#[cfg(windows)]
fn note_ngx_line(line: &str) {
    if let Some(say) = NGX.latch(line, GENERATION.load(Ordering::Relaxed)) {
        note(Level::WARN, say);
    }
}

/// NGX evaluate errors reported this session, for the shutdown's counts.
#[cfg(windows)]
fn ngx_errors_total() -> u64 {
    NGX.total()
}

/// Writes the waiting lines to the log.
fn drain_log() {
    let lines = std::mem::take(&mut *PENDING.lock().unwrap_or_else(PoisonError::into_inner));
    for (level, text) in lines {
        if level == Level::ERROR {
            error!("{text}");
        } else if level == Level::WARN {
            warn!("{text}");
        } else {
            info!("{text}");
        }
    }
    let dropped = std::mem::take(&mut *DROPPED.lock().unwrap_or_else(PoisonError::into_inner));
    if dropped > 0 {
        warn!("DLSS: {dropped} log lines dropped (more than {PENDING_MAX} between two frames)");
    }
}

/// Loads and initialises Streamline before the renderer starts. Added before `DefaultPlugins`:
/// `slInit` must come before any Vulkan call through the interposer [documented:
/// `ProgrammingGuide.md:115-119`], and Bevy reads `RawVulkanInitSettings` when it creates the
/// renderer (vendor/bevy_render/src/lib.rs `insert_future_resources`).
pub struct DlssInitPlugin;

impl Plugin for DlssInitPlugin {
    /// `main` adds the plugin when the start-up choice is DLSS, when the four files are in the
    /// DLSS folder ([`bundle_present`]) or with `NVF_DLSS_PRELOAD=1`, not with
    /// `NVF_DLSS_PRELOAD=0` unless DLSS is chosen (`upscaler::UpscalerStartup::loads_streamline`).
    fn build(&self, app: &mut App) {
        LOADER_RAN.store(true, Ordering::Relaxed);
        app.add_systems(First, drain_log);
        start(app);
    }

    /// After every plugin's `finish`, so after the renderer's resources are in the render world:
    /// the support check and the loader's own handles on the device.
    fn cleanup(&self, app: &mut App) {
        #[cfg(windows)]
        win::after_device(app);
        #[cfg(not(windows))]
        let _ = app;
    }
}

#[cfg(windows)]
fn start(app: &mut App) {
    win::start(app);
}

#[cfg(not(windows))]
fn start(_app: &mut App) {
    set_loader_state(DlssState::Unavailable(Unavailable::NotWindows));
    note(Level::INFO, "inactive: Windows only (Streamline's libraries are Windows DLLs)");
}

/// What the loader ended in (set at start and once the device exists).
static LOADER_STATE: Mutex<DlssState> = Mutex::new(DlssState::NotLoaded);

fn set_loader_state(state: DlssState) {
    *LOADER_STATE.lock().unwrap_or_else(PoisonError::into_inner) = state;
}

/// What the DLSS loader ended in this session: `NotLoaded` when it never ran (DLSS not asked
/// for), `Ready` once DLSS is supported and its functions fetched, otherwise the first reason it
/// stopped at (`Unavailable`). Final after `Plugin::cleanup`; for the settings (the plan's
/// step 8).
pub fn loader_state() -> DlssState {
    *LOADER_STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What DLSS's own failure left of this session: `None`, or `EvaluateFailed` once
/// [`FAILED_FRAMES`] frames ran without DLSS's output.
static EVALUATE_FAILED: AtomicBool = AtomicBool::new(false);

/// Whether DLSS can run in this session as far as is known now: [`loader_state`], unless DLSS
/// has since stopped producing output (`Unavailable::EvaluateFailed`). Read by the upscaler
/// status (`upscaler::dlss_state_now`), so the camera then draws natively and the settings say
/// why.
pub fn session_state() -> DlssState {
    if EVALUATE_FAILED.load(Ordering::Relaxed) {
        DlssState::Unavailable(Unavailable::EvaluateFailed)
    } else {
        loader_state()
    }
}

/// DLSS is active: the device came through Streamline on the GPU the check saw, DLSS is
/// supported and its functions were fetched (set in `win::after_device`, cleared at shutdown).
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// The renderer's device was made through Streamline's interposer and the loader holds it (set
/// in `win::after_device` whatever the support check then says, cleared at shutdown). An atomic,
/// not the loader's lock, because the main world reads it every frame while the render thread
/// may hold that lock for an evaluate.
static STREAMLINE_DEVICE: AtomicBool = AtomicBool::new(false);

/// Streamline was loaded and initialised at start and the renderer's device came through its
/// proxies, whether or not DLSS then turned out supported or kept working
/// (`upscaler::Session::streamline_loaded`). Not `loader_state() == Ready` [design]: a GPU
/// without DLSS, an adapter that did not match the check, or DLSS given up on later all leave
/// Streamline under the device (FSR then draws through the interposer).
pub fn streamline_loaded() -> bool {
    STREAMLINE_DEVICE.load(Ordering::Relaxed)
}

/// The loader ran at start ([`DlssInitPlugin`] was added), whatever it ended in.
static LOADER_RAN: AtomicBool = AtomicBool::new(false);

/// Whether the loader ran at start (`upscaler::dlss_state_now`: a loader that ran and left
/// [`loader_state`] `NotLoaded` was refused before it said why).
pub fn loader_ran() -> bool {
    LOADER_RAN.load(Ordering::Relaxed)
}

/// Whether the four files are in the DLSS folder (`NVF_DLSS_DIR`, else `dlss` beside the
/// program): the start's cheap check whether to run the loader at all
/// (`upscaler::UpscalerStartup`), and the status's whether a restart would bring DLSS. Only
/// that they exist as files; the loader's own checks (the updater, the signature, the device
/// check) still decide. Always false off Windows.
pub fn bundle_present() -> bool {
    #[cfg(windows)]
    {
        let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
        resolve_folder(std::env::var_os(ENV_DIR).as_deref(), exe_dir.as_deref()).is_ok_and(|(folder, _)| missing_files(&folder).is_empty())
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// DLSS may draw a camera now: active, and not given up on ([`session_state`]).
fn ready() -> bool {
    ACTIVE.load(Ordering::Relaxed) && !EVALUATE_FAILED.load(Ordering::Relaxed)
}

/// The main textures' usages DLSS needs on a camera it draws, its own and not FSR's
/// (`upscaler::prep::prepare` adds them for DLSS cameras only): the output is a storage image
/// (`GENERAL`, `STORAGE_READ_WRITE` in `evaluate.rs`), and NGX clears it with
/// `vkCmdClearColorImage`, which needs `TRANSFER_DST` [observed: Khronos validation layer,
/// `VUID-vkCmdClearColorImage-image-00002` inside `nv.ngx.dlss.Evaluate`, 2026-11-12]. The
/// HDR format (`Rgba16Float`) every prepared view has already (`prep::prepare` renders others
/// natively); nothing DLSS does copies from the main textures, so FSR's `COPY_SRC` (its opaque
/// copy) is not needed.
pub(crate) fn main_texture_usages() -> wgpu::TextureUsages {
    wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_DST
}

/// Frames running without DLSS's output (fallback frames) after which DLSS is given up on for
/// the session [design: about a second at 60 frames a second; a first frame without a previous
/// view or a camera cut is one frame, not sixty].
pub(crate) const FAILED_FRAMES: u32 = 60;

/// The run of fallback frames on DLSS cameras.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FailureRun {
    run: u32,
    failed: bool,
    /// NGX evaluate errors reported and not yet charged to an evaluated frame (at most
    /// [`FAILED_FRAMES`]).
    ngx_debt: u32,
}

impl FailureRun {
    /// One DLSS camera's frame: `evaluated` when DLSS wrote it. Returns `true` the one time the
    /// run reaches [`FAILED_FRAMES`].
    pub(crate) fn push(&mut self, evaluated: bool) -> bool {
        if evaluated {
            self.run = 0;
            return false;
        }
        self.run = self.run.saturating_add(1);
        if self.run >= FAILED_FRAMES && !self.failed {
            self.failed = true;
            return true;
        }
        false
    }

    /// One DLSS camera's frame with the NGX evaluate errors reported since the last frame
    /// (`NgxLatch::take`). Streamline's log thread delivers them later than the evaluate they
    /// belong to and sometimes several at once, so each is charged to one evaluated frame, this
    /// one or a later one: an evaluated frame with an error owing counts as a frame without
    /// DLSS's output [design]. Returns what [`Self::push`] returns.
    pub(crate) fn frame(&mut self, evaluated: bool, ngx_errors: u32) -> bool {
        self.ngx_debt = self.ngx_debt.saturating_add(ngx_errors).min(FAILED_FRAMES);
        let wrote = evaluated && self.ngx_debt == 0;
        if evaluated && !wrote {
            self.ngx_debt -= 1;
        }
        self.push(wrote)
    }
}

static FAILURE_RUN: Mutex<FailureRun> = Mutex::new(FailureRun { run: 0, failed: false, ngx_debt: 0 });

/// Counts a DLSS camera's frame toward [`FAILED_FRAMES`], with the NGX evaluate errors latched
/// since the last ([`FailureRun::frame`]); at the limit DLSS is marked unavailable for the
/// session, said once.
fn record(evaluated: bool) {
    let ngx_errors = NGX.take();
    if FAILURE_RUN.lock().unwrap_or_else(PoisonError::into_inner).frame(evaluated, ngx_errors) {
        EVALUATE_FAILED.store(true, Ordering::Relaxed);
        note(
            Level::WARN,
            format!("{FAILED_FRAMES} frames running without DLSS's output: DLSS is unavailable for the rest of the session ({}); the camera draws natively from the next frame until another upscaler is chosen (FSR 3.1 or Off take effect at once)", Unavailable::EvaluateFailed.key()),
        );
    }
}

/// What [`dlss_pass`] hands the evaluate for one frame: the common prepared inputs and the
/// camera.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct Frame<'a> {
    pub target: &'a ViewTarget,
    /// The common corrected depth: render size, Depth32Float, first-person depth unsqueezed
    /// (`upscaler::prep::Pipelines::corrected_depth`).
    pub depth: &'a wgpu::Texture,
    /// The prepass motion vectors (display size, the render rectangle valid) and their view.
    pub motion: &'a wgpu::Texture,
    pub motion_view: &'a wgpu::TextureView,
    pub quality: UpscaleQuality,
    pub render_size: UVec2,
    pub output_size: UVec2,
    pub view: &'a ExtractedView,
    pub previous: Option<&'a PreviousViewData>,
    /// `TemporalJitter::offset` (render pixels).
    pub jitter: Vec2,
    pub near: f32,
    /// Vertical field of view, radians.
    pub fov: f32,
    /// A camera cut or an invalid history this frame.
    pub reset: bool,
    /// The frame's Streamline number (`Presenting::frame` plus one), for the Frame Generation
    /// frame tokens of DLSS Frame Generation (`frame_token.rs`); `None` without latency's frame counter.
    pub frame_index: Option<u32>,
}

/// What [`dlss_pass`] does after [`evaluate`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) enum Outcome {
    /// DLSS wrote the other main texture (alpha 1).
    Evaluated,
    /// DLSS did not draw this frame: the common spatial fallback draws it, and the history is
    /// invalidated (no FSR state is made).
    Fallback,
}

/// One DLSS evaluate, or [`Outcome::Fallback`] (at once, touching nothing, when DLSS is not
/// ready).
#[cfg(windows)]
fn evaluate(ctx: &mut RenderContext, frame: &Frame) -> Outcome {
    if !ready() {
        return Outcome::Fallback;
    }
    win::evaluate(ctx, frame)
}

#[cfg(not(windows))]
fn evaluate(_ctx: &mut RenderContext, _frame: &Frame) -> Outcome {
    Outcome::Fallback
}

/// `slDLSSGetOptimalSettings` for a key (`sizing::OptimalSizes` asks, once per change).
#[cfg(windows)]
fn query_optimal(key: sizing::Key) -> Result<sizing::Optimal, String> {
    if !ready() {
        return Err("DLSS is not active".to_string());
    }
    win::optimal_settings(key)
}

#[cfg(not(windows))]
fn query_optimal(_key: sizing::Key) -> Result<sizing::Optimal, String> {
    Err("DLSS is Windows only".to_string())
}

/// DLSS's own render systems: the options before the frame is recorded ([`configure`]) and the
/// pass ([`dlss_pass`]). Added by the graphics plugin beside FSR's (graphics.rs).
pub struct DlssPassPlugin;

impl Plugin for DlssPassPlugin {
    fn build(&self, app: &mut App) {
        TEST_START.get_or_init(std::time::Instant::now);
        if let Ok(value) = std::env::var(RESIZE_ENV) {
            match resizes_from(&value) {
                Ok(steps) => {
                    app.insert_resource(ResizePlan { steps, next: 0 }).add_systems(Update, resize_aid);
                }
                Err(line) => warn!("{line}"),
            }
        }
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .init_resource::<DlssMainState>()
            .add_systems(ExtractSchedule, extract_main_state)
            .add_systems(Render, configure.in_set(RenderSystems::PrepareResources).after(prep::prepare))
            .add_systems(
                bevy::core_pipeline::Core3d,
                // Where `fsr_pass` is for FSR: after the occlusion before the upscaler (ssao.rs
                // orders `ao_before_fsr` before this), before bloom and the bright pass, and
                // before `fsr_pass`, which every other full-resolution effect (the eye's
                // metering mask, frame generation's passes) follows.
                dlss_pass
                    .in_set(bevy::core_pipeline::Core3dSystems::EarlyPostProcess)
                    .before(crate::fsr::fsr_pass)
                    .before(bevy::post_process::bloom::bloom)
                    .before(crate::post::bright_pass_hdr),
            );
        // DLSS Frame Generation's render systems (#2317, `frame_gen.rs`); each does nothing
        // unless DLSS-G was kept loaded.
        #[cfg(windows)]
        frame_gen::add_systems(render_app);
    }
}

/// What the render world needs of the main world's DLSS settings each frame (render world,
/// [`extract_main_state`]): the DLSS model asked for, and whether DLSS is the effective upscaler
/// (`None` before the upscaler status exists: nothing is released then).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DlssMainState {
    pub model: DlssModel,
    pub drawing: Option<bool>,
}

/// Copies [`DlssMainState`] from the main world: `Graphics::dlss_model` and
/// `upscaler::UpscalerStatus::effective`.
fn extract_main_state(mut state: ResMut<DlssMainState>, graphics: Extract<Option<Res<crate::graphics::Graphics>>>, status: Extract<Option<Res<crate::upscaler::UpscalerStatus>>>) {
    let next = DlssMainState { model: graphics.as_ref().map_or_else(DlssModel::default, |g| g.dlss_model), drawing: status.as_ref().map(|s| s.effective().is_dlss()) };
    state.set_if_neq(next);
}

/// Whether DLSS's resources are to be freed this frame ([`configure`]): DLSS is not the
/// effective upscaler and no view is DLSS's. A frame without a DLSS view while DLSS is still
/// the effective choice (a loading screen, a camera made again) keeps them.
fn release_now(main: DlssMainState, dlss_view: bool) -> bool {
    main.drawing == Some(false) && !dlss_view
}

/// The options for the DLSS camera's key (mode, output size, HDR-ness) and the DLSS model, set
/// before anything of the frame is recorded: on a change the GPU is drained and the viewport's
/// resources freed first (`win::configure`). One viewport, so the first DLSS camera only. Once
/// DLSS stops drawing ([`release_now`]), its resources are freed the same way (`win::release`),
/// also when DLSS was given up on.
pub(crate) fn configure(views: Query<(&ExtractedView, &PreparedUpscale)>, sizes: Res<OptimalSizes>, main: Res<DlssMainState>) {
    let dlss_view = views.iter().find_map(|(view, prepared)| match prepared.camera.settings {
        UpscaleSettings::Dlss(quality) => Some((view, quality)),
        UpscaleSettings::Fsr(_) => None,
    });
    if release_now(*main, dlss_view.is_some()) {
        release();
    }
    let Some((view, quality)) = dlss_view else { return };
    if !ready() {
        return;
    }
    let key = sizing::Key { quality, output: view.viewport.zw(), hdr: sizing::hdr_format(view.target_format) };
    let Some(optimal) = sizes.cached(key) else { return };
    #[cfg(windows)]
    win::configure(key, main.model, optimal);
    #[cfg(not(windows))]
    let _ = (optimal, main.model);
}

/// The Streamline frame number of the frame the render world draws (`frame_token::index_of`);
/// `None` off Windows or without latency reduction's frame counter.
fn frame_index(presenting: Option<&crate::anti_lag::Presenting>) -> Option<u32> {
    #[cfg(windows)]
    {
        presenting.map(|p| frame_token::index_of(p.frame))
    }
    #[cfg(not(windows))]
    {
        let _ = presenting;
        None
    }
}

/// DLSS's resources freed when options are set (`win::release`); off Windows nothing is held.
#[cfg(windows)]
fn release() {
    win::release();
}

#[cfg(not(windows))]
fn release() {}

/// DLSS's own pass for a view DLSS draws: the common corrected depth, the evaluate, the
/// scene's alpha restored where a shape marks it, the depth expanded for the screen-space
/// effects; on any failure the common spatial fallback and an invalidated history. Nothing of
/// FSR's is read or made.
#[allow(clippy::type_complexity)]
pub(crate) fn dlss_pass(
    view: ViewQuery<(&ViewTarget, &PreparedUpscale, Option<&ViewPrepassTextures>, &TemporalJitter, &ExtractedView, Option<&PreviousViewData>)>,
    upscale: Res<UpscaleRenderer>,
    device: Res<RenderDevice>,
    presenting: Option<Res<crate::anti_lag::Presenting>>,
    mut ctx: RenderContext,
) {
    let (target, prepared, prepass, jitter, extracted, previous) = view.into_inner();
    let UpscaleSettings::Dlss(quality) = prepared.camera.settings else { return };
    let Some(common) = &upscale.0 else { return };
    debug_assert!(!prep::allocates_fsr_state(Upscaler::Dlss));
    let device = device.wgpu_device();
    let inputs = prepass.and_then(|prepass| prepass.depth.as_ref().zip(prepass.motion_vectors.as_ref()));
    let Some((depth, motion)) = inputs else {
        prepared.history_valid.store(false, Ordering::Relaxed);
        record(false);
        common.fallback(device, ctx.command_encoder(), prepared, target);
        if let Some(depth) = prepass.and_then(|prepass| prepass.depth.as_ref()) {
            common.expand_depth(device, ctx.command_encoder(), prepared, &depth.texture.texture);
        }
        return;
    };
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let span = diagnostics.time_span(ctx.command_encoder(), "nvf_dlss");
    let reset = prepared.reset || !prepared.history_valid.load(Ordering::Relaxed);
    common.corrected_depth(device, ctx.command_encoder(), prepared, &depth.texture.texture);
    let outcome = evaluate(
        &mut ctx,
        &Frame {
            target,
            depth: &prepared.depth,
            motion: &motion.texture.texture,
            motion_view: &motion.texture.default_view,
            quality,
            render_size: prepared.render_size,
            output_size: prepared.output_size,
            view: extracted,
            previous,
            jitter: jitter.offset,
            near: prepared.camera.near,
            fov: prepared.camera.fov,
            reset,
            frame_index: frame_index(presenting.as_deref()),
        },
    );
    let encoder = ctx.command_encoder();
    match outcome {
        Outcome::Evaluated => {
            // DLSS's output has alpha 1 (`evaluate.rs` step 6); the scene's marks come back
            // only while a shape marks its pixels (the Pip-Boy's screen on its model), as for FSR.
            if prepared.camera.screen_mark {
                common.restore_alpha(device, encoder, prepared, target);
            }
            target.post_process_write();
            prepared.history_valid.store(true, Ordering::Relaxed);
        }
        Outcome::Fallback => {
            prepared.history_valid.store(false, Ordering::Relaxed);
            common.fallback(device, encoder, prepared, target);
        }
    }
    record(outcome == Outcome::Evaluated);
    common.expand_depth(device, encoder, prepared, &depth.texture.texture);
    span.end(ctx.command_encoder());
}

/// The renderer's plugin with the adapter pin (the module comment's "Adapter pin"): Bevy's
/// defaults, with `WgpuSettings::adapter_name` set to the checked NVIDIA GPU when the loader
/// pinned one. Without a pin it is `RenderPlugin::default()`. `main` sets it on
/// `DefaultPlugins`, after [`DlssInitPlugin`] has run.
///
/// Both carry the GPU allocator's block sizes for this machine's GPU budget
/// (memory_budget.rs, #2754).
pub fn render_plugin() -> bevy::render::RenderPlugin {
    let adapter_name = adapter_pin();
    let memory_hints = crate::memory_budget::hints_at_start();
    bevy::render::RenderPlugin {
        render_creation: bevy::render::settings::WgpuSettings { adapter_name, memory_hints, ..default() }.into(),
        ..default()
    }
}

/// The adapter name the loader pinned, if any.
#[cfg(windows)]
fn adapter_pin() -> Option<String> {
    win::pinned_name()
}

#[cfg(not(windows))]
fn adapter_pin() -> Option<String> {
    None
}

/// Streamline shut down in order, from `main` once `App::run` has returned (the module's
/// "Shutdown"). Does nothing when Streamline was never initialised.
pub fn shutdown() {
    ACTIVE.store(false, Ordering::Relaxed);
    STREAMLINE_DEVICE.store(false, Ordering::Relaxed);
    #[cfg(windows)]
    win::shutdown();
    drain_log();
}

/// [`end_now`] has run (once a process).
static ENDING: AtomicBool = AtomicBool::new(false);

/// Whether this is the first time `flag` is taken (the run-once guard of [`end_now`]).
fn first_time(flag: &AtomicBool) -> bool {
    !flag.swap(true, Ordering::SeqCst)
}

/// The process is about to end without `App::run` returning (`handoff.rs` `end_now`, which
/// calls `std::process::exit` from the main schedule, so `main` never reaches [`shutdown`]):
/// the module's "Ending at once". With the renderer's device made through Streamline, the
/// render app is taken back from the render thread (so Bevy renders nothing from here on),
/// FSR frame generation's presenter threads are stopped and joined
/// (`crate::frame_gen::stop_presenters`), device polling and pipeline creation are held off
/// (`gpu_poll::quiet`), DLSS-G's mode is set off and the window's swapchain released
/// (`frame_gen.rs` `end_now`), and then the shutdown runs as at any other exit ([`shutdown`]).
/// `why` names the reason in the log lines (`exit: <why>: …`). Runs once; nothing to do
/// without Streamline. Waits without a time limit (the module's "Ending at once" says why).
/// The caller ends the process right after: the render app and the poll gate are kept (never
/// given back) until then.
pub fn end_now(main_world: &mut World, why: &str) {
    if !first_time(&ENDING) {
        return;
    }
    #[cfg(windows)]
    if streamline_loaded() {
        let mut render_app = render_app_back(main_world);
        match render_app.as_mut() {
            // With the render thread idle no frame is handed to a presenter any more; each ends
            // after presenting what it holds, before anything below drains or releases.
            Some(app) => {
                let stopped = crate::frame_gen::stop_presenters(app.world_mut());
                if stopped > 0 {
                    note(Level::INFO, format!("exit: {why}: {stopped} FSR frame generation presenter(s) stopped and joined"));
                }
            }
            None => note(
                Level::WARN,
                format!("exit: {why}: the render app could not be taken back from the render thread (no pipelined rendering, or the thread stopped); presenters and the window's swapchain left in place"),
            ),
        }
        // Taken only once the render thread has handed its app back and the presenters have
        // ended (both may poll the device while finishing); held until the process ends.
        let quiet = crate::gpu_poll::quiet();
        frame_gen::end_now(render_app.as_mut().map(|a| a.world_mut()), why);
        shutdown();
        note(Level::INFO, format!("exit: {why}: Streamline shut down before the process ends"));
        drain_log();
        // Neither is dropped: the render world's non-send data belongs to this thread's
        // teardown, which the process end skips; the poll gate stays shut until then.
        std::mem::forget(render_app);
        std::mem::forget(quiet);
        return;
    }
    let _ = (main_world, why);
    shutdown();
}

/// The render app back from the render thread (pipelined rendering), waiting for the frame it
/// is rendering and running the main-thread tasks it may queue meanwhile, as Bevy's own
/// extraction does [design: vendor/bevy_render/src/pipelined_rendering.rs `renderer_extract`];
/// the render thread then waits for an app that never comes. `None` without pipelined
/// rendering (no `RenderAppChannels`: the render app is the main app's and ran to the end of
/// its frame before the main schedule) or when the render thread has stopped.
#[cfg(windows)]
fn render_app_back(main_world: &mut World) -> Option<bevy::app::SubApp> {
    use bevy::ecs::schedule::MainThreadExecutor;
    use bevy::render::pipelined_rendering::RenderAppChannels;
    use bevy::tasks::ComputeTaskPool;

    let executor = main_world.get_resource::<MainThreadExecutor>()?.clone();
    let mut channels = main_world.get_resource_mut::<RenderAppChannels>()?;
    ComputeTaskPool::get()
        .scope_with_executor(true, Some(&*executor.0), |s| {
            s.spawn(async { channels.recv().await });
        })
        .pop()
        .flatten()
}



/// The files of [`FILES`] that `folder` lacks, in that order.
#[cfg(any(windows, test))]
fn missing_files(folder: &Path) -> Vec<&'static str> {
    FILES.iter().copied().filter(|name| !folder.join(name).is_file()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    /// The end without Bevy's teardown runs once (#2733): the guard is taken the first time
    /// only.
    #[test]
    fn the_end_at_once_runs_once() {
        let flag = AtomicBool::new(false);
        assert!(first_time(&flag), "the first end runs");
        assert!(!first_time(&flag), "a second call does nothing");
        assert!(!first_time(&flag));
    }

    /// DLSS's resources are freed once DLSS is not the effective upscaler and no view is DLSS's;
    /// a frame without a DLSS view while DLSS still draws, or before the status exists, keeps
    /// them.
    #[test]
    fn resources_are_freed_only_when_dlss_stopped_drawing() {
        let state = |drawing| DlssMainState { model: DlssModel::L, drawing };
        assert!(release_now(state(Some(false)), false), "FSR or off is the effective choice");
        assert!(!release_now(state(Some(false)), true), "a DLSS view this frame: not yet");
        assert!(!release_now(state(Some(true)), false), "DLSS still the choice (a loading screen, a camera made again)");
        assert!(!release_now(state(Some(true)), true));
        assert!(!release_now(state(None), false), "no status yet");
        assert_eq!(DlssMainState::default(), DlssMainState { model: DlssModel::Default, drawing: None });
    }

    #[test]
    fn loader_state_reports_what_the_loader_set() {
        assert_eq!(DlssState::default(), DlssState::NotLoaded);
        set_loader_state(DlssState::Unavailable(crate::upscaler::Unavailable::UpdaterInFolder));
        assert_eq!(loader_state(), DlssState::Unavailable(crate::upscaler::Unavailable::UpdaterInFolder));
        assert_eq!(session_state(), loader_state(), "the loader's state while DLSS has not been given up on");
        set_loader_state(DlssState::NotLoaded);
        assert_eq!(loader_state(), DlssState::NotLoaded);
    }

    #[test]
    fn without_the_loader_streamline_is_not_under_the_device_and_dlss_never_draws() {
        // The tests never add DlssInitPlugin: no device came through the interposer.
        assert!(!streamline_loaded());
        assert!(!ready());
        assert!(!frame_generation_loaded(), "no DLSS-G kept loaded: NVIDIA low latency may load");
    }

    #[test]
    fn sixty_fallback_frames_running_give_dlss_up_once() {
        let mut run = FailureRun::default();
        // A first frame without inputs, then DLSS draws: the run starts again.
        assert!(!run.push(false));
        assert!(!run.push(true));
        for _ in 0..FAILED_FRAMES - 1 {
            assert!(!run.push(false));
        }
        assert!(!run.push(true), "59 running and then output: not given up");
        for frame in 1..=FAILED_FRAMES {
            assert_eq!(run.push(false), frame == FAILED_FRAMES, "frame {frame}");
        }
        assert!(!run.push(false), "said once");
        assert!(!run.push(true) && !run.push(false));
    }

    #[test]
    fn ngx_evaluate_errors_are_told_from_other_lines() {
        // Sample lines written here in the shape of `sl.common`'s format strings after
        // Streamline's header (`src/commonEntry.cpp:768, 792, 1047`; `src/log.cpp:352-371`).
        let header = "[10-20-30][streamline][error][tid:4242][1.234s]commonEntry.cpp:792[evaluateNGXFeature]";
        assert!(ngx_evaluate_error(&format!("{header} [sl.dlss] NGX evaluate feature failed 0xbad00005")));
        assert!(ngx_evaluate_error(&format!("{header} [sl.dlss] NGX evaluate feature (D3D12) failed 0xbad00002")));
        assert!(!ngx_evaluate_error(&format!("{header} [sl.dlss] NGX create feature failed 0xbad00001")), "a create failure is a failed call");
        assert!(!ngx_evaluate_error("[sl.dlss] NGX evaluate feature"), "no failure in it");
        assert!(!ngx_evaluate_error("failed 0x1 before NGX evaluate feature"), "the failure must follow");
        assert!(!ngx_evaluate_error(""));
    }

    #[test]
    fn ngx_errors_are_latched_and_said_once_per_generation() {
        let latch = NgxLatch::new();
        let line = "[sl.dlss] NGX evaluate feature failed 0xbad00005";
        assert_eq!(latch.latch("[sl.dlss] something else failed 0x1", 1), None);
        assert_eq!(latch.take(), 0, "only NGX evaluate errors are latched");
        assert_eq!(latch.latch(line, 1).as_deref(), Some("NGX reported an evaluate error: [sl.dlss] NGX evaluate feature failed 0xbad00005"));
        assert_eq!(latch.latch(line, 1), None, "said once in the generation");
        assert_eq!(latch.take(), 2, "both counted");
        assert_eq!(latch.take(), 0, "taken");
        assert!(latch.latch(line, 2).is_some(), "said again in a new generation (a resize, a mode change)");
        assert_eq!((latch.take(), latch.total()), (1, 3));
    }

    #[test]
    fn a_latched_ngx_error_counts_as_a_frame_without_output() {
        let mut run = FailureRun::default();
        // An evaluated frame with an error owing is a failed frame; the next clean one resets.
        assert!(!run.frame(true, 1));
        assert_eq!(run.run, 1);
        assert!(!run.frame(true, 0));
        assert_eq!(run.run, 0);
        // Errors arriving several at once are charged to the evaluated frames that follow; a
        // fallback frame fails anyway and leaves them owing.
        assert!(!run.frame(true, 3));
        assert!(!run.frame(false, 0));
        assert!(!run.frame(true, 0));
        assert!(!run.frame(true, 0));
        assert_eq!((run.run, run.ngx_debt), (4, 0));
        assert!(!run.frame(true, 0));
        assert_eq!(run.run, 0, "debt paid: DLSS's output again");
        // NGX failing every evaluate gives DLSS up at the sixtieth frame, as fallbacks do.
        for frame in 1..=FAILED_FRAMES {
            assert_eq!(run.frame(true, 1), frame == FAILED_FRAMES, "frame {frame}");
        }
        // The debt is bounded.
        let mut run = FailureRun::default();
        run.frame(false, u32::MAX);
        assert_eq!(run.ngx_debt, FAILED_FRAMES);
    }

    #[test]
    fn dlss_main_textures_get_storage_and_copy_destination() {
        let usages = main_texture_usages();
        assert!(usages.contains(wgpu::TextureUsages::STORAGE_BINDING), "the output is a storage image");
        assert!(usages.contains(wgpu::TextureUsages::COPY_DST), "NGX clears it");
        assert!(!usages.contains(wgpu::TextureUsages::COPY_SRC), "FSR's opaque copy is FSR's own");
    }

    #[test]
    fn without_a_pin_the_renderer_plugin_is_bevys_default() {
        // No loader ran in the tests: no pin.
        assert_eq!(adapter_pin(), None);
        let plugin = render_plugin();
        let bevy::render::settings::RenderCreation::Automatic(settings) = plugin.render_creation else { panic!("automatic creation") };
        assert_eq!(settings.adapter_name, None);
    }

    #[test]
    fn folder_from_the_variable_or_beside_the_program() {
        let exe = std::env::temp_dir();
        assert_eq!(resolve_folder(None, Some(&exe)), Ok((exe.join("dlss"), FolderSource::BesideProgram)));
        assert_eq!(resolve_folder(Some(OsStr::new("")), Some(&exe)), Ok((exe.join("dlss"), FolderSource::BesideProgram)), "empty counts as unset");
        let elsewhere = std::env::temp_dir().join("nvf-dlss-elsewhere");
        assert_eq!(resolve_folder(Some(elsewhere.as_os_str()), Some(&exe)), Ok((elsewhere.clone(), FolderSource::EnvDir)));
        assert_eq!(resolve_folder(Some(elsewhere.as_os_str()), None), Ok((elsewhere, FolderSource::EnvDir)));
        assert!(resolve_folder(Some(OsStr::new("relative\\dlss")), Some(&exe)).unwrap_err().contains("not an absolute path"));
        assert!(resolve_folder(None, None).is_err());
    }

    #[test]
    fn frame_generation_reasons_follow_the_loader() {
        // The tests never run the loader: as without the files.
        assert_eq!(frame_generation_unavailable(|| true), Some(if cfg!(windows) { "Files missing: Download DLSS… on the Upscaler row" } else { "Not on this GPU" }));
        for (availability, expected) in [(2, Some("Needs an NVIDIA RTX 40 series or newer")), (3, Some("Needs hardware-accelerated GPU scheduling")), (4, Some("Needs a newer NVIDIA driver")), (5, Some("Not on this GPU")), (6, None)] {
            assert_eq!(frame_generation_reason(true, availability, None, || panic!("only missing files need the offer")), expected);
        }
        assert_eq!(frame_generation_max(), 0);
    }

    #[test]
    fn frame_generation_missing_files_hint_follows_the_download() {
        for availability in [0, FgAvailability::FilesMissing as u8] {
            assert_eq!(frame_generation_reason(true, availability, None, || true), Some("Files missing: Download DLSS… on the Upscaler row"));
            assert_eq!(frame_generation_reason(true, availability, None, || false), Some("The DLSS Frame Generation files are missing"));
            let done = download::Phase::Done(std::path::PathBuf::from("dlss"));
            assert_eq!(frame_generation_reason(true, availability, Some(&done), || panic!("installed: no offer needed")), Some("Restart to use DLSS Frame Generation"));
            assert_eq!(frame_generation_reason(false, availability, None, || panic!("Windows only")), Some("Not on this GPU"));
        }
    }

    #[test]
    fn the_resize_aid_reads_seconds_and_sizes() {
        assert_eq!(resizes_from("24:1280x720, 16:1600x900"), Ok(vec![(16.0, UVec2::new(1600, 900)), (24.0, UVec2::new(1280, 720))]));
        assert!(resizes_from("16:10x10").unwrap_err().contains("at least 64x64"));
        assert!(resizes_from("16:1600").is_err());
        assert_eq!(resizes_from(""), Ok(vec![]));
    }

    #[test]
    fn frame_generation_files_checked_by_name() {
        let dir = std::env::temp_dir().join(format!("nvf-dlssg-files-test-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())));
        assert_eq!(missing_fg_files(&dir), FG_FILES.to_vec(), "no folder: all missing");
        std::fs::create_dir_all(&dir).unwrap();
        for name in FILES {
            std::fs::write(dir.join(name), b"").unwrap();
        }
        assert_eq!(missing_fg_files(&dir), FG_FILES.to_vec(), "the four SR files are not the frame generation ones");
        std::fs::write(dir.join("sl.dlss_g.dll"), b"").unwrap();
        std::fs::write(dir.join("sl.reflex.dll"), b"").unwrap();
        assert_eq!(missing_fg_files(&dir), vec!["nvngx_dlssg.dll", "sl.pcl.dll"]);
        std::fs::write(dir.join("nvngx_dlssg.dll"), b"").unwrap();
        std::fs::write(dir.join("sl.pcl.dll"), b"").unwrap();
        assert!(missing_fg_files(&dir).is_empty());
        assert!(missing_files(&dir).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn four_files_checked_by_name() {
        let dir = std::env::temp_dir().join(format!("nvf-dlss-files-test-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())));
        assert_eq!(missing_files(&dir), FILES.to_vec(), "no folder: all missing");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(missing_files(&dir), FILES.to_vec());
        std::fs::write(dir.join("sl.interposer.dll"), b"").unwrap();
        std::fs::write(dir.join("sl.dlss.dll"), b"").unwrap();
        // A folder with a file's name is not the file.
        std::fs::create_dir_all(dir.join("nvngx_dlss.dll")).unwrap();
        assert_eq!(missing_files(&dir), vec!["sl.common.dll", "nvngx_dlss.dll"]);
        std::fs::remove_dir(dir.join("nvngx_dlss.dll")).unwrap();
        std::fs::write(dir.join("sl.common.dll"), b"").unwrap();
        std::fs::write(dir.join("nvngx_dlss.dll"), b"").unwrap();
        assert!(missing_files(&dir).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
