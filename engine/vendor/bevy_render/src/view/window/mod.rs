use crate::renderer::wgpu_wrapper;
use crate::sync_world::{MainEntity, RenderEntity, SyncToRenderWorld};
use crate::{camera::extract_cameras, renderer::RenderQueue};
use crate::{
    render_resource::{SurfaceTexture, TextureView},
    renderer::{RenderAdapter, RenderDevice, RenderInstance},
    Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
};
use bevy_app::{App, Plugin};
use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;
use bevy_log::{debug, info, warn};
use bevy_utils::default;
use bevy_window::{
    CompositeAlphaMode, PresentMode, PrimaryWindow, RawHandleWrapper, Window, WindowClosing,
};
use core::num::NonZero;
use wgpu::{
    SurfaceConfiguration, SurfaceTargetUnsafe, TextureFormat, TextureUsages, TextureViewDescriptor,
};

pub mod screenshot;

use screenshot::ScreenshotPlugin;

pub struct WindowRenderPlugin;

impl Plugin for WindowRenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ScreenshotPlugin);

        // We need to sync the window entity in the render world
        // We can't use [`SyncComponentPlugin`] because it would introduce `bevy_render` as
        // a dependency to `bevy_window`
        {
            app.add_observer(|trigger: On<Add<Window>>, mut commands: Commands| {
                commands
                    .entity(trigger.entity)
                    .insert(SyncToRenderWorld::default());
            });

            // The primary window gets added before this plugin so we can't rely on the observer
            let _ = app.world_mut().run_system_once(
                |mut commands: Commands, windows: Query<Entity, With<Window>>| {
                    for entity in &windows {
                        commands.entity(entity).insert(SyncToRenderWorld::default());
                    }
                },
            );
        }

        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                // NVForever (2026-11-13): see `HoldSurfaceReconfigure`.
                .init_resource::<HoldSurfaceReconfigure>()
                .add_systems(ExtractSchedule, extract_windows.before(extract_cameras))
                .add_systems(
                    Render,
                    create_surfaces
                        .run_if(need_surface_configuration)
                        .before(prepare_windows),
                )
                .add_systems(Render, prepare_windows.in_set(RenderSystems::PrepareViews));
        }
    }
}

#[derive(Component)]
pub struct ExtractedWindow {
    pub physical_width: u32,
    pub physical_height: u32,
    pub present_mode: PresentMode,
    pub desired_maximum_frame_latency: Option<NonZero<u32>>,
    /// Note: this will not always be the swap chain texture view. When taking a screenshot,
    /// this will point to an alternative texture instead to allow for copying the render result
    /// to CPU memory.
    pub swap_chain_texture_view: Option<TextureView>,
    pub swap_chain_texture: Option<SurfaceTexture>,
    pub swap_chain_texture_format: Option<TextureFormat>,
    /// This is an srgb view of [`ExtractedWindow::swap_chain_texture_format`]
    /// so that in shaders we are always in linear space.
    pub swap_chain_texture_view_format: Option<TextureFormat>,
    pub size_changed: bool,
    pub present_mode_changed: bool,
    pub alpha_mode: CompositeAlphaMode,
    /// Whether this window needs an initial buffer commit.
    ///
    /// On Wayland, windows must present at least once before they are shown.
    /// See <https://wayland.app/protocols/xdg-shell#xdg_surface>
    pub needs_initial_present: bool,
}

/// NVForever (2026-11-13): while `true`, no window's surface is reconfigured: a size or
/// present-mode change, or an outdated surface, is marked instead ([`SurfaceData`]'s outdated
/// mark) and [`create_surfaces`] reconfigures it on the first frame after the hold is released;
/// [`prepare_windows`] gives an outdated window no image that frame. NVForever's DLSS Frame
/// Generation holds it for the frame it presents with DLSS-G off before a window change (DLSS-G
/// must be off before the swapchain changes). Upstream behaviour while `false`, the default.
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HoldSurfaceReconfigure(pub bool);

/// NVForever (2026-11-08): time the render thread spent in the swapchain's calls, presents and
/// takes (`get_current_texture`), since the start: a measuring aid (frame generation's log).
pub struct SwapchainWaits {
    present_nanos: core::sync::atomic::AtomicU64,
    presents: core::sync::atomic::AtomicU64,
    acquire_nanos: core::sync::atomic::AtomicU64,
    acquires: core::sync::atomic::AtomicU64,
}

impl SwapchainWaits {
    fn add_present(&self, d: core::time::Duration) {
        use core::sync::atomic::Ordering::Relaxed;
        self.present_nanos.fetch_add(d.as_nanos() as u64, Relaxed);
        self.presents.fetch_add(1, Relaxed);
    }

    fn add_acquire(&self, d: core::time::Duration) {
        use core::sync::atomic::Ordering::Relaxed;
        self.acquire_nanos.fetch_add(d.as_nanos() as u64, Relaxed);
        self.acquires.fetch_add(1, Relaxed);
    }

    /// (nanoseconds in presents, presents, nanoseconds in takes, takes) so far.
    pub fn totals(&self) -> (u64, u64, u64, u64) {
        use core::sync::atomic::Ordering::Relaxed;
        (self.present_nanos.load(Relaxed), self.presents.load(Relaxed), self.acquire_nanos.load(Relaxed), self.acquires.load(Relaxed))
    }
}

/// NVForever (2026-11-19, HDR output prototype): set before the first window's surface is
/// made to ask for an HDR surface: `Rgba16Float` in the extended sRGB linear colour space
/// (scRGB: Rec.709 primaries, 1.0 = 80 nits, values above 1 brighter). Taken only when the
/// surface lists that pair and can be a copy source (the game's HDR encode pass copies the
/// finished frame before writing it back encoded).
pub static HDR_OUTPUT_REQUEST: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// NVForever (2026-11-19): a window's surface was configured for HDR ([`HDR_OUTPUT_REQUEST`]).
pub static HDR_OUTPUT_ACTIVE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

pub static SWAPCHAIN_WAITS: SwapchainWaits = SwapchainWaits {
    present_nanos: core::sync::atomic::AtomicU64::new(0),
    presents: core::sync::atomic::AtomicU64::new(0),
    acquire_nanos: core::sync::atomic::AtomicU64::new(0),
    acquires: core::sync::atomic::AtomicU64::new(0),
};

/// `get_current_texture`, its time added to [`SWAPCHAIN_WAITS`].
fn timed_get_current_texture(surface: &wgpu::Surface<'static>) -> wgpu::CurrentSurfaceTexture {
    let started = std::time::Instant::now();
    let texture = surface.get_current_texture();
    SWAPCHAIN_WAITS.add_acquire(started.elapsed());
    texture
}

/// NVForever (2026-11-08): a window whose swapchain images Bevy does not take. [`prepare_windows`]
/// gives such a window a stand-in texture of its own (the surface's size and format, one of
/// [`STAND_INS`] used in turn) as its view, so the frame is recorded and submitted without
/// touching the surface; whoever set the component takes the images and presents them, through
/// [`SurfaceData::shared`] (NVForever's frame generation does it on a thread of its own).
#[derive(Component, Default)]
pub struct DeferredSwapchain {
    stand_ins: [Option<StandIn>; STAND_INS],
    /// The stand-in this frame draws into.
    current: usize,
    /// NVForever (2026-11-09): [`prepare_windows`] gave the window a stand-in since
    /// [`DeferredSwapchain::take_stand_in`] last took one.
    fresh: core::sync::atomic::AtomicBool,
}

/// How many stand-ins a [`DeferredSwapchain`] uses in turn: the frame being drawn and two whose
/// copy into the window may still be waiting.
pub const STAND_INS: usize = 3;

struct StandIn {
    texture: crate::render_resource::Texture,
    view: TextureView,
    width: u32,
    height: u32,
    format: TextureFormat,
}

impl DeferredSwapchain {
    /// This frame's stand-in (its view in the window's view format), when there is one.
    pub fn stand_in(&self) -> Option<(&crate::render_resource::Texture, &TextureView)> {
        self.stand_ins[self.current].as_ref().map(|s| (&s.texture, &s.view))
    }

    /// NVForever (2026-11-09): [`DeferredSwapchain::stand_in`], once per frame drawn: `None`
    /// until [`prepare_windows`] gives the window its next stand-in (a frame it passed the
    /// window over has last frame's, which whoever presents has taken already).
    pub fn take_stand_in(&self) -> Option<(&crate::render_resource::Texture, &TextureView)> {
        self.fresh.swap(false, core::sync::atomic::Ordering::Relaxed).then(|| self.stand_in()).flatten()
    }

    /// The next stand-in for this frame, made or remade for the surface's size and format.
    fn prepare(&mut self, render_device: &RenderDevice, width: u32, height: u32, format: TextureFormat) -> TextureView {
        self.current = (self.current + 1) % STAND_INS;
        *self.fresh.get_mut() = true;
        let slot = &mut self.stand_ins[self.current];
        if slot.as_ref().is_none_or(|s| s.width != width || s.height != height || s.format != format) {
            let view_format = format.add_srgb_suffix();
            let texture = render_device.create_texture(&wgpu::TextureDescriptor {
                label: Some("deferred_swapchain_stand_in"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_SRC,
                view_formats: if view_format == format { &[] } else { core::slice::from_ref(&view_format) },
            });
            let view = texture.create_view(&TextureViewDescriptor { format: Some(view_format), ..default() });
            *slot = Some(StandIn { texture, view, width, height, format });
        }
        slot.as_ref().unwrap().view.clone()
    }
}

/// NVForever (2026-11-08): a window's surface for a thread of its own to take images from and
/// present ([`DeferredSwapchain`]). Taking and presenting happen under [`SharedSurface::lock`],
/// which [`create_surfaces`] also takes to reconfigure the surface.
///
/// NVForever (2026-11-09): the holder never configures the surface. wgpu-core 30's
/// `configure_surface` waits for the GPU and fails (`GpuWaitTimeout`, a validation error) when
/// another thread submits meanwhile, and the render thread keeps submitting; an outdated
/// surface is marked instead ([`SurfaceGuard::acquire`]) and [`create_surfaces`] reconfigures it
/// on the render thread, under the lock. It also keeps the window, as Bevy's [`RawHandleWrapper`]
/// beside [`SurfaceData`] does, so the window outlives the surface (`create_surface_unsafe`'s
/// contract) however long the holder keeps it.
#[derive(Clone)]
pub struct SharedSurface {
    surface: std::sync::Arc<WgpuSurface>,
    state: std::sync::Arc<SurfaceShare>,
    /// NVForever (2026-11-09): after `surface`, so it is dropped after it.
    _window: RawHandleWrapper,
}

/// NVForever (2026-11-09): what a window's [`SurfaceData`] shares with [`SharedSurface`]
/// holders: the configuration as they see it, the lock they take the surface's images under,
/// and whether one of them found the surface outdated.
struct SurfaceShare {
    configuration: std::sync::Mutex<SurfaceConfiguration>,
    outdated: core::sync::atomic::AtomicBool,
}

/// The surface while [`SharedSurface::lock`] is held.
pub struct SurfaceGuard<'a> {
    surface: &'a wgpu::Surface<'static>,
    configuration: std::sync::MutexGuard<'a, SurfaceConfiguration>,
    outdated: &'a core::sync::atomic::AtomicBool,
}

impl SharedSurface {
    pub fn lock(&self) -> SurfaceGuard<'_> {
        SurfaceGuard {
            surface: &self.surface,
            configuration: self.state.configuration.lock().unwrap_or_else(std::sync::PoisonError::into_inner),
            outdated: &self.state.outdated,
        }
    }
}

impl SurfaceGuard<'_> {
    /// The surface's format and the sRGB view of it that windows draw through.
    pub fn formats(&self) -> (TextureFormat, TextureFormat) {
        (self.configuration.format, self.configuration.format.add_srgb_suffix())
    }

    /// The next image; `None` when the window has none to give (occluded, a timeout, lost, or
    /// outdated). NVForever (2026-11-09): an outdated or lost surface is not configured here but
    /// marked for [`create_surfaces`], which reconfigures it on the render thread before the next
    /// frame is prepared; the images until then are skipped.
    pub fn acquire(&self) -> Option<wgpu::SurfaceTexture> {
        match timed_get_current_texture(self.surface) {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => Some(t),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.outdated.store(true, core::sync::atomic::Ordering::Relaxed);
                None
            }
            _ => None,
        }
    }

    /// Present `texture`, its time added to [`SWAPCHAIN_WAITS`].
    pub fn present(&self, queue: &wgpu::Queue, texture: wgpu::SurfaceTexture) {
        let started = std::time::Instant::now();
        queue.present(texture);
        SWAPCHAIN_WAITS.add_present(started.elapsed());
    }
}

impl ExtractedWindow {
    fn set_swapchain_texture(&mut self, frame: wgpu::SurfaceTexture) {
        self.swap_chain_texture_view_format = Some(frame.texture.format().add_srgb_suffix());
        let texture_view_descriptor = TextureViewDescriptor {
            format: self.swap_chain_texture_view_format,
            ..default()
        };
        self.swap_chain_texture_view = Some(TextureView::from(
            frame.texture.create_view(&texture_view_descriptor),
        ));
        self.swap_chain_texture = Some(SurfaceTexture::from(frame));
    }

    fn has_swapchain_texture(&self) -> bool {
        self.swap_chain_texture_view.is_some() && self.swap_chain_texture.is_some()
    }

    pub fn present(&mut self, queue: &RenderQueue) {
        if let Some(surface_texture) = self.swap_chain_texture.take() {
            // TODO(clean): winit docs recommends calling pre_present_notify before this.
            // though `present()` doesn't present the frame, it schedules it to be presented
            // by wgpu.
            // https://docs.rs/winit/0.29.9/wasm32-unknown-unknown/winit/window/struct.Window.html#method.pre_present_notify
            let started = std::time::Instant::now();
            surface_texture.present(queue);
            SWAPCHAIN_WAITS.add_present(started.elapsed());
        }
    }
}

/// NVForever (2026-11-13): public, so a system can be ordered before or after it (DLSS Frame
/// Generation sets its mode off before a closing window's surface is removed here).
pub fn extract_windows(
    mut commands: Commands,
    mut extracted_windows: Query<&mut ExtractedWindow>,
    mut closing: Extract<MessageReader<WindowClosing>>,
    windows: Extract<Query<(RenderEntity, &Window, &RawHandleWrapper, Has<PrimaryWindow>)>>,
    mut removed: Extract<RemovedComponents<RawHandleWrapper>>,
    mut removed_primary: Extract<RemovedComponents<PrimaryWindow>>,
    mapper: Extract<Query<&RenderEntity>>,
) {
    for (render_entity, window, handle, is_primary) in windows.iter() {
        if is_primary {
            commands.entity(render_entity).insert(PrimaryWindow);
        }

        let (new_width, new_height) = (
            window.resolution.physical_width().max(1),
            window.resolution.physical_height().max(1),
        );

        let Ok(mut extracted_window) = extracted_windows.get_mut(render_entity) else {
            commands.entity(render_entity).insert((
                ExtractedWindow {
                    physical_width: new_width,
                    physical_height: new_height,
                    present_mode: window.present_mode,
                    desired_maximum_frame_latency: window.desired_maximum_frame_latency,
                    swap_chain_texture: None,
                    swap_chain_texture_view: None,
                    size_changed: false,
                    swap_chain_texture_format: None,
                    swap_chain_texture_view_format: None,
                    present_mode_changed: false,
                    alpha_mode: window.composite_alpha_mode,
                    needs_initial_present: true,
                },
                handle.clone(),
            ));
            continue;
        };

        if extracted_window.swap_chain_texture.is_none() {
            // If we called present on the previous swap-chain texture last update,
            // then drop the swap chain frame here, otherwise we can keep it for the
            // next update as an optimization. `prepare_windows` will only acquire a new
            // swap chain texture if needed.
            extracted_window.swap_chain_texture_view = None;
        }

        extracted_window.size_changed = new_width != extracted_window.physical_width
            || new_height != extracted_window.physical_height;
        extracted_window.present_mode_changed =
            window.present_mode != extracted_window.present_mode;

        if extracted_window.size_changed {
            debug!(
                "Window size changed from {}x{} to {}x{}",
                extracted_window.physical_width,
                extracted_window.physical_height,
                new_width,
                new_height
            );
            extracted_window.physical_width = new_width;
            extracted_window.physical_height = new_height;
        }

        if extracted_window.present_mode_changed {
            debug!(
                "Window Present Mode changed from {:?} to {:?}",
                extracted_window.present_mode, window.present_mode
            );
            extracted_window.present_mode = window.present_mode;
        }
    }

    // Remove the components instead of despawn the synced render entity here:
    // `RawHandleWrapper` removal is not necessarily
    // terminal. On Android the native window is destroyed (and its handle removed) when the app
    // suspends, but the window entity survives and is given a new handle on resume. Despawning
    // would leave the `SubEntity` on the main world entity pointing at a dead entity, and the
    // next round of extraction would panic when reusing it.
    //
    // The render entity is only ever despawned by the entity sync system once the main world
    // window entity is actually destroyed. Here we only tear down the surface and drop the
    // extracted window data; extraction and `create_surfaces` will recreate them whenever the
    // window has a (new) `RawHandleWrapper` again.
    for closing_window in closing.read() {
        if let Ok(render_entity) = mapper.get(closing_window.window) {
            commands
                .entity(render_entity.entity())
                .remove::<(ExtractedWindow, RawHandleWrapper, SurfaceData)>();
        }
    }
    for removed_window in removed.read() {
        if let Ok(render_entity) = mapper.get(removed_window) {
            commands
                .entity(render_entity.entity())
                .remove::<(ExtractedWindow, RawHandleWrapper, SurfaceData)>();
        }
    }
    for removed_window in removed_primary.read() {
        if let Ok(render_entity) = mapper.get(removed_window) {
            commands
                .entity(render_entity.entity())
                .remove::<PrimaryWindow>();
        }
    }
}

// TODO: what lifetime should this be?
wgpu_wrapper!(struct WgpuSurface(wgpu::Surface<'static>));

#[derive(Component)]
pub struct SurfaceData {
    surface: std::sync::Arc<WgpuSurface>,
    configuration: SurfaceConfiguration,
    texture_view_format: Option<TextureFormat>,
    /// NVForever (2026-11-08): the configuration as [`SharedSurface`] holders see it, and the
    /// lock they take the surface's images under. NVForever (2026-11-09): with whether a holder
    /// found the surface outdated.
    shared: std::sync::Arc<SurfaceShare>,
    /// NVForever (2026-11-09): the window the surface was made on, for [`SharedSurface`] holders
    /// (after `surface`, so it is dropped after it).
    window: RawHandleWrapper,
}

impl SurfaceData {
    /// NVForever (2026-11-08): the surface for a thread of its own ([`DeferredSwapchain`]).
    pub fn shared(&self) -> SharedSurface {
        SharedSurface { surface: self.surface.clone(), state: self.shared.clone(), _window: self.window.clone() }
    }

    /// NVForever (2026-11-09): a [`SharedSurface`] holder found the surface outdated.
    fn outdated(&self) -> bool {
        self.shared.outdated.load(core::sync::atomic::Ordering::Relaxed)
    }

    /// NVForever (2026-11-13): whether [`create_surfaces`] would reconfigure the surface this
    /// frame: `window`'s size or present mode changed, or the surface is marked outdated (also
    /// by a [`HoldSurfaceReconfigure`] that held a change back).
    pub fn reconfigure_pending(&self, window: &ExtractedWindow) -> bool {
        window.size_changed || window.present_mode_changed || self.outdated()
    }

    /// NVForever (2026-11-13): marks the surface outdated, so [`create_surfaces`] reconfigures it
    /// when no [`HoldSurfaceReconfigure`] holds.
    fn hold_back(&self) {
        self.shared.outdated.store(true, core::sync::atomic::Ordering::Relaxed);
    }
}

/// (re)configures window surfaces, and obtains a swapchain texture for rendering.
///
/// NOTE: `get_current_texture` in `prepare_windows` can take a long time if the GPU workload is
/// the performance bottleneck. This can be seen in profiles as multiple prepare-set systems all
/// taking an unusually long time to complete, and all finishing at about the same time as the
/// `prepare_windows` system. Improvements in bevy are planned to avoid this happening when it
/// should not but it will still happen as it is easy for a user to create a large GPU workload
/// relative to the GPU performance and/or CPU workload.
/// This can be caused by many reasons, but several of them are:
/// - GPU workload is more than your current GPU can manage
/// - Error / performance bug in your custom shaders
/// - wgpu was unable to detect a proper GPU hardware-accelerated device given the chosen
///   [`Backends`](crate::settings::Backends), [`WgpuLimits`](crate::settings::WgpuLimits),
///   and/or [`WgpuFeatures`](crate::settings::WgpuFeatures). For example, on Windows currently
///   `DirectX 11` is not supported by wgpu 0.12 and so if your GPU/drivers do not support Vulkan,
///   it may be that a software renderer called "Microsoft Basic Render Driver" using `DirectX 12`
///   will be chosen and performance will be very poor. This is visible in a log message that is
///   output during renderer initialization.
///   Another alternative is to try to use [`ANGLE`](https://github.com/gfx-rs/wgpu#angle) and
///   [`Backends::GL`](crate::settings::Backends::GL) with the `gles` feature enabled if your
///   GPU/drivers support `OpenGL 4.3` / `OpenGL ES 3.0` or later.
pub fn prepare_windows(
    mut windows: Query<(MainEntity, &mut ExtractedWindow, Option<&SurfaceData>, Option<&mut DeferredSwapchain>)>,
    render_device: Res<RenderDevice>,
    sorted_cameras: Res<crate::camera::SortedCameras>,
    // NVForever (2026-11-13): an outdated surface is not configured here while this holds.
    hold: Option<Res<HoldSurfaceReconfigure>>,
    #[cfg(target_os = "linux")] render_instance: Res<RenderInstance>,
) {
    for (main_entity, mut window, maybe_surface_data, deferred) in &mut windows {
        // Skip acquiring a swap-chain texture for windows that no camera
        // targets. This avoids a wasted clear pass in
        // `handle_uncovered_swap_chains` that triggers a DMA-fence fd leak on
        // Adreno 740 (Quest 3). The exception is windows that still need their
        // initial present (required on Wayland).
        let is_camera_target = sorted_cameras.0.iter().any(|c| {
            matches!(
                &c.target,
                Some(bevy_camera::NormalizedRenderTarget::Window(w)) if w.entity() == main_entity
            ) && matches!(c.output_mode, bevy_camera::CameraOutputMode::Write { .. })
        });
        if !is_camera_target && !window.needs_initial_present {
            continue;
        }

        let Some(surface_data) = maybe_surface_data else {
            continue;
        };

        // NVForever (2026-11-08): the frame goes to a stand-in; another thread takes the image.
        // An image Bevy still holds (taken before the window was deferred) is used and
        // presented as usual for this one frame.
        if let Some(mut deferred) = deferred {
            if window.swap_chain_texture.is_some() {
                continue;
            }
            let format = surface_data.configuration.format;
            let view = deferred.prepare(&render_device, surface_data.configuration.width, surface_data.configuration.height, format);
            window.swap_chain_texture_view = Some(view);
            window.swap_chain_texture_view_format = Some(format.add_srgb_suffix());
            window.swap_chain_texture_format = Some(format);
            continue;
        }

        // We didn't present the previous frame, so we can keep using our existing swapchain texture.
        if window.has_swapchain_texture() && !window.size_changed && !window.present_mode_changed {
            continue;
        }

        // A recurring issue is hitting `wgpu::SurfaceError::Timeout` on certain Linux
        // mesa driver implementations. This seems to be a quirk of some drivers.
        // We'd rather keep panicking when not on Linux mesa, because in those case,
        // the `Timeout` is still probably the symptom of a degraded unrecoverable
        // application state.
        // see https://github.com/bevyengine/bevy/pull/5957
        // and https://github.com/gfx-rs/wgpu/issues/1218
        #[cfg(target_os = "linux")]
        let may_erroneously_timeout = || {
            bevy_tasks::IoTaskPool::get().scope(|scope| {
                scope.spawn(async {
                    render_instance
                        .enumerate_adapters(wgpu::Backends::VULKAN)
                        .await
                        .iter()
                        .any(|adapter| {
                            let name = adapter.get_info().name;
                            name.starts_with("Radeon")
                                || name.starts_with("AMD")
                                || name.starts_with("Intel")
                        })
                });
            })[0]
        };

        let surface = &surface_data.surface;
        match timed_get_current_texture(surface) {
            wgpu::CurrentSurfaceTexture::Success(surface_texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(surface_texture) => {
                window.set_swapchain_texture(surface_texture);
            }
            #[cfg(target_os = "linux")]
            wgpu::CurrentSurfaceTexture::Timeout if may_erroneously_timeout() => {
                bevy_log::trace!(
                    "Couldn't get swap chain texture. This is probably a quirk \
                        of your Linux GPU driver, so it can be safely ignored."
                );
            }
            // NVForever (2026-11-13): while a hold is on, marked for `create_surfaces` instead,
            // and the window gets no image this frame.
            wgpu::CurrentSurfaceTexture::Outdated if hold.as_ref().is_some_and(|h| h.0) => {
                surface_data.hold_back();
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                render_device.configure_surface(surface, &surface_data.configuration);
                let frame = match surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(surface_texture)
                    | wgpu::CurrentSurfaceTexture::Suboptimal(surface_texture) => surface_texture,
                    variant => {
                        // This is a common occurrence on X11 and Xwayland with NVIDIA drivers
                        // when opening and resizing the window.
                        warn!(
                            "Couldn't get swap chain texture after configuring. Cause: '{variant:?}'"
                        );
                        continue;
                    }
                };
                window.set_swapchain_texture(frame);
            }
            wgpu::CurrentSurfaceTexture::Occluded => {}
            other => {
                bevy_log::error!("Couldn't get swap chain texture: {other:?}");
            }
        }
        window.swap_chain_texture_format = Some(surface_data.configuration.format);
    }
}

pub fn need_surface_configuration(windows: Query<(&ExtractedWindow, Option<&SurfaceData>)>) -> bool {
    for (window, surface_data) in &windows {
        // NVForever (2026-11-09): or a surface a `SharedSurface` holder found outdated.
        if surface_data.is_none_or(SurfaceData::outdated) || window.size_changed || window.present_mode_changed {
            return true;
        }
    }
    false
}

// 2 is wgpu's default/what we've been using so far.
// 1 is the minimum, but may cause lower framerates due to the cpu waiting for the gpu to finish
// all work for the previous frame before starting work on the next frame, which then means the gpu
// has to wait for the cpu to finish to start on the next frame.
const DEFAULT_DESIRED_MAXIMUM_FRAME_LATENCY: u32 = 2;

/// Creates window surfaces.
pub fn create_surfaces(
    mut commands: Commands,
    // By accessing a NonSend resource, we tell the scheduler to put this system on the main thread,
    // which is necessary for some OS's
    #[cfg(any(target_os = "macos", target_os = "ios"))] _marker: bevy_ecs::system::NonSendMarker,
    mut windows: Query<(
        Entity,
        &mut ExtractedWindow,
        &RawHandleWrapper,
        Option<&mut SurfaceData>,
    )>,
    render_instance: Res<RenderInstance>,
    render_adapter: Res<RenderAdapter>,
    render_device: Res<RenderDevice>,
    // NVForever (2026-11-13): a change is marked, not applied, while this holds.
    hold: Option<Res<HoldSurfaceReconfigure>>,
) {
    for (entity, mut window, handle, mut maybe_surface_data) in &mut windows {
        let Some(data) = maybe_surface_data.as_mut() else {
            let surface_target = SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(handle.get_display_handle()),
                raw_window_handle: handle.get_window_handle(),
            };
            // SAFETY: The window handles in ExtractedWindows will always be valid objects to create surfaces on
            let surface = unsafe {
                // NOTE: On some OSes this MUST be called from the main thread.
                // As of wgpu 0.15, only fallible if the given window is a HTML canvas and obtaining a WebGPU or WebGL2 context fails.
                render_instance
                    .create_surface_unsafe(surface_target)
                    .expect("Failed to create wgpu surface")
            };
            let caps = surface.get_capabilities(&render_adapter);
            let present_mode = present_mode(&window, &caps);
            let formats = caps.formats.clone();
            // For future HDR output support, we'll need to request a format that supports HDR,
            // but as of wgpu 0.15 that is not yet supported.
            // Prefer sRGB formats for surfaces, but fall back to first available format if no sRGB formats are available.
            let mut format = *formats.first().expect("No supported formats for surface");
            for available_format in formats {
                // Rgba8UnormSrgb and Bgra8UnormSrgb and the only sRGB formats wgpu exposes that we can use for surfaces.
                if available_format == TextureFormat::Rgba8UnormSrgb
                    || available_format == TextureFormat::Bgra8UnormSrgb
                {
                    format = available_format;
                    break;
                }
            }
            // NVForever (2026-11-19): HDR output when asked for and offered.
            let mut color_space = wgpu::SurfaceColorSpace::Auto;
            let mut usage = TextureUsages::RENDER_ATTACHMENT;
            if HDR_OUTPUT_REQUEST.load(core::sync::atomic::Ordering::Relaxed) {
                let spaces = caps.color_spaces(TextureFormat::Rgba16Float);
                let offered: Vec<_> = caps.format_capabilities.iter().map(|fc| alloc::format!("{:?} {:?}", fc.format, fc.color_spaces)).collect();
                info!("hdr output: the surface offers {}", offered.join(", "));
                if spaces.contains(wgpu::SurfaceColorSpaces::EXTENDED_SRGB_LINEAR) && caps.usages.contains(TextureUsages::COPY_SRC) {
                    format = TextureFormat::Rgba16Float;
                    color_space = wgpu::SurfaceColorSpace::ExtendedSrgbLinear;
                    usage |= TextureUsages::COPY_SRC;
                    HDR_OUTPUT_ACTIVE.store(true, core::sync::atomic::Ordering::Relaxed);
                    info!("hdr output: Rgba16Float, extended sRGB linear (scRGB)");
                } else {
                    warn!("hdr output: asked for, but the surface has no Rgba16Float in extended sRGB linear that can be copied (is Windows HDR on for this display?); SDR");
                }
            }

            let texture_view_format = if !format.is_srgb() {
                Some(format.add_srgb_suffix())
            } else {
                None
            };
            let configuration = SurfaceConfiguration {
                format,
                width: window.physical_width,
                height: window.physical_height,
                usage,
                present_mode,
                desired_maximum_frame_latency: window
                    .desired_maximum_frame_latency
                    .map(NonZero::<u32>::get)
                    .unwrap_or(DEFAULT_DESIRED_MAXIMUM_FRAME_LATENCY),
                alpha_mode: match window.alpha_mode {
                    CompositeAlphaMode::Auto => wgpu::CompositeAlphaMode::Auto,
                    CompositeAlphaMode::Opaque => wgpu::CompositeAlphaMode::Opaque,
                    CompositeAlphaMode::PreMultiplied => wgpu::CompositeAlphaMode::PreMultiplied,
                    CompositeAlphaMode::PostMultiplied => wgpu::CompositeAlphaMode::PostMultiplied,
                    CompositeAlphaMode::Inherit => wgpu::CompositeAlphaMode::Inherit,
                },
                view_formats: match texture_view_format {
                    Some(format) => vec![format],
                    None => vec![],
                },
                color_space,
            };

            render_device.configure_surface(&surface, &configuration);

            // NVForever (2026-11-08): the surface in an `Arc` and the share, for `SharedSurface`;
            // NVForever (2026-11-09): with the window the surface was made on.
            commands.entity(entity).insert(SurfaceData {
                surface: std::sync::Arc::new(WgpuSurface::new(surface)),
                shared: std::sync::Arc::new(SurfaceShare {
                    configuration: std::sync::Mutex::new(configuration.clone()),
                    outdated: core::sync::atomic::AtomicBool::new(false),
                }),
                window: handle.clone(),
                configuration,
                texture_view_format,
            });
            continue;
        };

        // NVForever (2026-11-09): also when a `SharedSurface` holder found the surface outdated,
        // which can happen with the window's size unchanged (alt-tab, a display mode or HDR
        // change); the holder does not configure it itself.
        if window.size_changed || window.present_mode_changed || data.outdated() {
            // NVForever (2026-11-13): held back for a later frame (`HoldSurfaceReconfigure`).
            if hold.as_ref().is_some_and(|h| h.0) {
                data.hold_back();
                continue;
            }
            // normally this is dropped on present but we double check here to be safe as failure to
            // drop it will cause validation errors in wgpu
            drop(window.swap_chain_texture.take());
            #[cfg_attr(
                target_arch = "wasm32",
                expect(clippy::drop_non_drop, reason = "texture views are not drop on wasm")
            )]
            drop(window.swap_chain_texture_view.take());

            data.configuration.width = window.physical_width;
            data.configuration.height = window.physical_height;
            let caps = data.surface.get_capabilities(&render_adapter);
            data.configuration.present_mode = present_mode(&window, &caps);
            // NVForever (2026-11-08): not while a thread holding the surface takes or presents.
            let mut shared = data.shared.configuration.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            // NVForever (2026-11-09): cleared under the lock, which holders set it under.
            data.shared.outdated.store(false, core::sync::atomic::Ordering::Relaxed);
            render_device.configure_surface(&data.surface, &data.configuration);
            *shared = data.configuration.clone();
        }
    }
}

fn present_mode(window: &ExtractedWindow, caps: &wgpu::SurfaceCapabilities) -> wgpu::PresentMode {
    let present_mode = match window.present_mode {
        PresentMode::Fifo => wgpu::PresentMode::Fifo,
        PresentMode::FifoRelaxed => wgpu::PresentMode::FifoRelaxed,
        PresentMode::Mailbox => wgpu::PresentMode::Mailbox,
        PresentMode::Immediate => wgpu::PresentMode::Immediate,
        PresentMode::AutoVsync => wgpu::PresentMode::AutoVsync,
        PresentMode::AutoNoVsync => wgpu::PresentMode::AutoNoVsync,
    };
    let fallbacks = match present_mode {
        wgpu::PresentMode::AutoVsync => {
            &[wgpu::PresentMode::FifoRelaxed, wgpu::PresentMode::Fifo][..]
        }
        wgpu::PresentMode::AutoNoVsync => &[
            wgpu::PresentMode::Immediate,
            wgpu::PresentMode::Mailbox,
            wgpu::PresentMode::Fifo,
        ][..],
        wgpu::PresentMode::Mailbox => &[
            wgpu::PresentMode::Mailbox,
            wgpu::PresentMode::Immediate,
            wgpu::PresentMode::Fifo,
        ][..],
        // Always end in FIFO to make sure it's always supported
        x => &[x, wgpu::PresentMode::Fifo][..],
    };
    let new_present_mode = fallbacks
        .iter()
        .copied()
        .find(|fallback| caps.present_modes.contains(fallback))
        .unwrap_or_else(|| {
            unreachable!(
                "Fallback system failed to choose present mode. \
                            This is a bug. Mode: {:?}, Options: {:?}",
                window.present_mode, &caps.present_modes
            );
        });
    if new_present_mode != present_mode && fallbacks.contains(&present_mode) {
        info!("PresentMode {present_mode:?} requested but not available. Falling back to {new_present_mode:?}");
    }
    new_present_mode
}
