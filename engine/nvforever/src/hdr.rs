//! HDR output (prototype, 2026-11-19; Settings > Display > HDR, or `--hdr`). The window's
//! surface becomes scRGB (`Rgba16Float`, extended sRGB linear: Rec.709 primaries, 1.0 = 80
//! nits) when the display is in HDR mode (vendored bevy_render: `HDR_OUTPUT_REQUEST`, switched
//! live by a reconfigure of the surface), and the picture is made for it in two places [design]:
//!
//! - The highlights (imagespace.wesl, the grade's last step): on the main view only, the
//!   graded picture's highlights rise above 1 (1 = paper white) towards the display's peak:
//!   above the knee (linear luminance) by the square of the way from the knee to white,
//!   SDR white landing on the peak, one gain for all three channels (the hue kept). Every
//!   other view (the terminals, the minigames), the Pip-Boy's screen and the UI stay SDR.
//!   The main view keeps the HDR (fp16) target so those values last until the encode.
//! - The encode (`hdr_encode.wgsl`, [`Encoder`]): every image of the window, once drawn, is
//!   written into scRGB with the SDR range read as a gamma 2.2 screen shows it (OLED blacks
//!   stay black instead of the grey a plain sRGB decode lifts them to), the black level, paper
//!   white and the peak. Where the image comes from decides where it is encoded:
//!   - Bevy's own present: [`encode_windows`], in the vendored `PrePresent` schedule, after the
//!     screenshots' copies into the window's image (so a screenshot frame is encoded too) and
//!     before the present.
//!   - FSR 3.1 frame generation's presenter thread (`fg_presenter.rs`): it draws the real and
//!     generated frames into the window's images through an [`Encoder`] instead of its copy.
//!     With `NVF_FG_PRESENTER=0` the generated frame is encoded before the render thread
//!     presents it (`frame_gen.rs` `present_generated`), the real one as Bevy's present.
//!   - DLSS Frame Generation (experimental with HDR, 2026-11-20): Streamline makes the
//!     generated frames from the tagged HUD-less copy and takes the UI from the presented
//!     frame, so the HUD-less copy is encoded as well, right after the UI alpha pass reads it
//!     ([`WindowEncode::encode_in_place`] from `dlss/frame_gen.rs`), and moved back to the
//!     state its tag declares. Switching HDR changes the swapchain, which counts as a pending
//!     window change, so DLSS-G's mode goes off first as for a resize. Untested here: the
//!     owner's RTX 3070 cannot run DLSS-G.
//!
//! Screenshots are of the picture before the encode (1.0 = paper white): a PNG shows the SDR
//! picture with the highlights clipped, an `.hdr` or `.exr` keeps them.
//!
//! Why on the graded picture, not in the tonemapper: the scene's own luminance gives next to
//! nothing to lift, and before the grade the picture is dark; Goodsprings at noon peaks near
//! 0.5 (linear) after Tony McMapface and the grade, so a curve there lifted almost nothing
//! [observed 2026-11-19: `.hdr` screenshots of the main view, before the encode].
//!
//! Not yet: a separate UI brightness, wide gamut, HDR10 (PQ) output.

use bevy::core_pipeline::fullscreen_material::fullscreen_material_system;
use bevy::core_pipeline::{Core3d, Core3dSystems};
use bevy::prelude::*;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::renderer::{PrePresent, RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::view::{ExtractedWindow, ViewTarget, HDR_OUTPUT_ACTIVE, HDR_OUTPUT_REQUEST};
use bevy::render::RenderApp;
use bevy::ui_render::ui_pass;
use nvf_play::settings::{add_setting, Setting, Slider};
use std::sync::atomic::Ordering;
use std::sync::Mutex;

/// The surface's format with HDR output.
pub(crate) const SURFACE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// scRGB's 1.0, in nits.
const SCRGB_NITS: f32 = 80.0;

/// The settings page's ranges.
/// The peak row: Auto, then 400 to 2000 nits in 50s.
const PEAK: (f32, f32, usize) = (400.0, 2000.0, 33);
/// The peak when it is Auto and the display reports none to go by.
const FALLBACK_PEAK: f32 = 1000.0;
/// The brightness row's choices of paper white, in nits, the reference white among them.
const PAPER_STEPS: [f32; 15] = [80.0, 100.0, 120.0, 140.0, 160.0, 180.0, 203.0, 220.0, 240.0, 260.0, 280.0, 300.0, 320.0, 350.0, 400.0];
/// Paper white by default: the reference white of HDR video and games, 203 nits
/// [documented: ITU-R BT.2408, "HDR Reference White" 203 cd/m2; the owner's pointer, 2026-11-20:
/// gammastudios.tech's The Sinking City 2 HDR write-up uses it as its default].
const PAPER_DEFAULT: f32 = 203.0;

/// The knee (the brightest channel, linear, where the highlights start to rise) as a part of
/// the picture's average luminance ([`SceneKey`]), and its least and most: at night the
/// average is low and a fire or a lamp is a highlight; at noon the knee is high and walls and
/// sand stay as in SDR [design, 2026-11-20: the owner found Nipton's fires at night not
/// punchy; measured, the fire was 0.4-0.55 in the graded SDR picture, under the fixed knee's
/// reach].
const KNEE_PER_AVERAGE: f32 = 2.5;
const KNEE_RANGE: (f32, f32) = (0.10, 0.55);
/// The white point (where a highlight gets the peak's whole gain) the same way: at night the
/// brightest things are about 0.5 in the graded picture, so with white at 1 they rose to
/// about paper white and no further [observed 2026-11-20, Nipton's fire at 23:00]; on a
/// bright day it stays 1.
const WHITE_PER_AVERAGE: f32 = 12.0;
const WHITE_RANGE: (f32, f32) = (0.40, 1.0);

/// The HDR output's settings (the settings page's HDR rows, `--hdr-*`).
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct HdrOutput {
    /// Asked for (the surface is HDR only where the display offers it: [`active`]).
    pub enabled: bool,
    /// The display's brightest, in nits; `None`: Auto, what the display reports
    /// ([`display_peak`]), else [`FALLBACK_PEAK`].
    pub peak_nits: Option<f32>,
    /// What SDR's white (the UI, a white wall in daylight) shows at, in nits.
    pub paper_white_nits: f32,
    /// Subtracted from the SDR range's linear values and stretched back (< 0 lifts).
    pub black_floor: f32,
    /// Read the SDR range as a gamma 2.2 screen shows it rather than by the sRGB curve.
    pub gamma22: bool,
    /// How far the highlights rise: 0 as SDR, 1 to the peak.
    pub highlights: f32,
    /// A fixed knee (the brightest channel, linear, where the highlights start to rise);
    /// `None`: it follows the picture's average ([`SceneKey`]).
    pub knee: Option<f32>,
}

impl Default for HdrOutput {
    fn default() -> Self {
        Self { enabled: false, peak_nits: None, paper_white_nits: PAPER_DEFAULT, black_floor: 0.0, gamma22: true, highlights: 1.0, knee: None }
    }
}

impl HdrOutput {
    pub fn clamped(mut self) -> Self {
        self.peak_nits = self.peak_nits.map(|p| p.clamp(100.0, 10_000.0));
        self.paper_white_nits = self.paper_white_nits.clamp(SCRGB_NITS, 10_000.0);
        self.black_floor = self.black_floor.clamp(-0.05, 0.05);
        self.highlights = self.highlights.clamp(0.0, 1.0);
        self.knee = self.knee.map(|k| k.clamp(0.0, 0.9));
        self
    }

    /// The peak now: the chosen one, or Auto's.
    pub fn peak(&self) -> f32 {
        self.peak_nits.unwrap_or_else(|| detected_peak().unwrap_or(FALLBACK_PEAK))
    }

    /// Paper white now, never above the peak.
    pub fn paper(&self) -> f32 {
        self.paper_white_nits.min(self.peak())
    }

    /// The knee now: the fixed one, or from the picture's average.
    pub fn knee_now(&self) -> f32 {
        self.knee.unwrap_or_else(|| knee_for(scene_average()))
    }

    /// The white point now: 1 with a fixed knee, or from the picture's average.
    pub fn white_now(&self) -> f32 {
        if self.knee.is_some() { 1.0 } else { white_for(scene_average()) }
    }

    /// The grade's `hdr` (imagespace.rs `ImageSpaceGrade::hdr`): the white point (above 0:
    /// on), the peak in paper whites, the knee, the strength.
    pub fn grade(&self) -> Vec4 {
        Vec4::new(self.white_now(), self.peak() / self.paper(), self.knee_now(), self.highlights)
    }

    /// `hdr_encode.wgsl`'s uniform: paper white and the peak in scRGB units, the black level,
    /// gamma 2.2 (1) or sRGB (0); the calibration pattern, the peak in paper whites, the knee,
    /// the highlights' strength; the pattern's box.
    fn encode(&self, pattern: Pattern) -> [f32; 12] {
        let [x0, y0, x1, y1] = PATTERN_BOX;
        [
            self.paper() / SCRGB_NITS,
            self.peak() / SCRGB_NITS,
            self.black_floor,
            if self.gamma22 { 1.0 } else { 0.0 },
            pattern as u32 as f32,
            self.peak() / self.paper(),
            self.knee_now(),
            self.highlights,
            x0,
            y0,
            x1,
            y1,
        ]
    }

    pub fn describe(&self) -> String {
        format!(
            "{}; peak {}, paper white {:.0} nits, black level {:+.3}, {}, highlights {:.0}% from {}",
            if self.enabled { "on" } else { "off" },
            match self.peak_nits {
                Some(p) => format!("{p:.0} nits"),
                None => format!("Auto ({:.0} nits{})", self.peak(), if detected_peak().is_some() { ", the display's" } else { "" }),
            },
            self.paper_white_nits,
            self.black_floor,
            if self.gamma22 { "gamma 2.2 blacks" } else { "sRGB blacks" },
            self.highlights * 100.0,
            self.knee.map_or("the picture's average".to_string(), |k| format!("{k:.2}"))
        )
    }
}

/// The encode's settings for every thread that encodes ([`Encoder::draw`]).
static ENCODE: Mutex<[f32; 12]> = Mutex::new([2.5, 12.5, 0.0, 1.0, 0.0, 5.0, 0.25, 1.0, 0.0, 0.0, 0.0, 0.0]);

/// The calibration pattern the encode draws while an HDR row of the settings page is chosen
/// (`hdr_encode.wgsl` `pattern`), in absolute nits over the frame [design, after the
/// calibration screens HDR games and Windows' HDR Calibration show].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u32)]
pub(crate) enum Pattern {
    #[default]
    None = 0,
    /// A box at the peak setting with a plus at 10,000 nits: raised until the plus vanishes.
    Peak = 1,
    /// The player's scene over a ramp to paper white.
    PaperWhite = 3,
}

impl Pattern {
    /// Shows the player's own scene ([`ScenePreview`]) with a strip of the pattern under it.
    pub(crate) fn shows_scene(self) -> bool {
        matches!(self, Pattern::PaperWhite)
    }

    /// The pattern for a settings row's key.
    pub(crate) fn for_row(key: &str) -> Pattern {
        match key {
            "hdr-peak-brightness" => Pattern::Peak,
            "hdr-brightness" => Pattern::PaperWhite,
            _ => Pattern::None,
        }
    }
}

/// Where the pattern goes, in the window's parts (left, top, right, bottom): the settings
/// page's top right, over the tab rail, which the keys do not need while on a row
/// [observed 2026-11-20: the settings page at 1920x1080, rows end near x 0.68, the rail's
/// tabs from y 0.12].
const PATTERN_BOX: [f32; 4] = [0.725, 0.09, 0.975, 0.40];

/// The window's surface is HDR now.
pub(crate) fn active() -> bool {
    HDR_OUTPUT_ACTIVE.load(Ordering::Relaxed)
}

/// The picture's average luminance (SDR linear, each channel clipped to 1), smoothed, as f32
/// bits ([`SceneKey`]); 0.1 until measured.
static SCENE_AVERAGE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3DCC_CCCD);

fn scene_average() -> f32 {
    f32::from_bits(SCENE_AVERAGE.load(Ordering::Relaxed))
}

/// The knee for a picture's average luminance.
fn knee_for(average: f32) -> f32 {
    (KNEE_PER_AVERAGE * average).clamp(KNEE_RANGE.0, KNEE_RANGE.1)
}

/// The white point for a picture's average luminance.
fn white_for(average: f32) -> f32 {
    (WHITE_PER_AVERAGE * average).clamp(WHITE_RANGE.0, WHITE_RANGE.1)
}

/// The peak the window's display reports (f32 bits, 0: none yet or none usable).
static DISPLAY_PEAK: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn detected_peak() -> Option<f32> {
    Some(f32::from_bits(DISPLAY_PEAK.load(Ordering::Relaxed))).filter(|p| *p > 0.0)
}

/// What a display reports of its luminance (Windows: DXGI's output description for the
/// monitor the window is on): its peak, its peak over the whole screen, its least, and whether
/// the output is in HDR (BT.2100 PQ) now.
#[derive(Clone, Debug, PartialEq)]
struct DisplayLuminance {
    /// The output's name (`\\.\DISPLAY1`), for the log.
    name: String,
    peak: f32,
    full_frame: f32,
    min: f32,
    hdr: bool,
}

/// The peak worth taking from a report: a display in HDR mode reporting between 300 and
/// 10,000 nits, and not a stand-in report: the same peak over the whole screen as for a
/// highlight above 1000 nits is what Windows gives for a display whose own data says nothing
/// usable [observed 2026-11-20: the owner's OLED reported 4000 nits, 4000 over the whole
/// screen and 0.12 at least, where an OLED shows a few hundred over the whole screen and
/// near 0 black; design: displays reporting nothing give 0, and some SDR panels in HDR mode
/// 270-ish]. Those keep the fallback; Windows' HDR Calibration app replaces the report with
/// measured values.
fn usable_peak(l: &DisplayLuminance) -> Option<f32> {
    let stand_in = l.peak > 1000.0 && l.full_frame >= l.peak;
    (l.hdr && (300.0..=10_000.0).contains(&l.peak) && !stand_in).then_some(l.peak)
}

/// The window's display's luminance, as Windows reports it (`IDXGIOutput6::GetDesc1`, which
/// Windows' own HDR Calibration app's profile changes) [documented: Microsoft Learn,
/// DXGI_OUTPUT_DESC1: MaxLuminance, MaxFullFrameLuminance, MinLuminance in nits, ColorSpace].
#[cfg(windows)]
fn display_luminance(hwnd: *mut core::ffi::c_void) -> Option<DisplayLuminance> {
    use windows::core::Interface;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dxgi::Common::DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, IDXGIOutput6};
    use windows::Win32::Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONEAREST};
    // SAFETY: the window handle is the game's own live window; the DXGI calls take and
    // return COM interfaces the `windows` crate counts references of.
    unsafe {
        let monitor = MonitorFromWindow(HWND(hwnd), MONITOR_DEFAULTTONEAREST);
        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        for a in 0u32.. {
            let Ok(adapter) = factory.EnumAdapters1(a) else { break };
            for o in 0u32.. {
                let Ok(output) = adapter.EnumOutputs(o) else { break };
                let Ok(output) = output.cast::<IDXGIOutput6>() else { continue };
                let Ok(desc) = output.GetDesc1() else { continue };
                if desc.Monitor == monitor {
                    let len = desc.DeviceName.iter().position(|c| *c == 0).unwrap_or(desc.DeviceName.len());
                    return Some(DisplayLuminance {
                        name: String::from_utf16_lossy(&desc.DeviceName[..len]),
                        peak: desc.MaxLuminance,
                        full_frame: desc.MaxFullFrameLuminance,
                        min: desc.MinLuminance,
                        hdr: desc.ColorSpace == DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020,
                    });
                }
            }
        }
        None
    }
}

#[cfg(not(windows))]
fn display_luminance(_hwnd: *mut core::ffi::c_void) -> Option<DisplayLuminance> {
    None
}

/// While the surface is HDR, the window's display's report read now and then (every few
/// seconds: the window may move to another display) into Auto's peak; logged when it changes.
fn display_peak(windows: Query<&bevy::window::RawHandleWrapper, With<bevy::window::PrimaryWindow>>, mut last: Local<Option<(std::time::Instant, Option<DisplayLuminance>)>>) {
    if !active() || last.as_ref().is_some_and(|(t, _)| t.elapsed().as_secs() < 5) {
        return;
    }
    let Ok(handle) = windows.single() else { return };
    let hwnd = match handle.get_window_handle() {
        wgpu::rwh::RawWindowHandle::Win32(h) => h.hwnd.get() as *mut core::ffi::c_void,
        _ => return,
    };
    let report = display_luminance(hwnd);
    if last.as_ref().is_none_or(|(_, r)| *r != report) {
        match &report {
            Some(l) => info!(
                "hdr output: {} reports peak {:.0} nits (whole screen {:.0}, least {:.4}), {}; Auto's peak {}",
                l.name,
                l.peak,
                l.full_frame,
                l.min,
                if l.hdr { "in HDR" } else { "not in HDR" },
                usable_peak(l).map_or(format!("{FALLBACK_PEAK:.0} nits (not usable)"), |p| format!("{p:.0} nits"))
            ),
            None => info!("hdr output: the display reports nothing; Auto's peak {FALLBACK_PEAK:.0} nits"),
        }
    }
    DISPLAY_PEAK.store(report.as_ref().and_then(usable_peak).unwrap_or(0.0).to_bits(), Ordering::Relaxed);
    *last = Some((std::time::Instant::now(), report));
}

pub struct HdrPlugin(pub HdrOutput);

impl Plugin for HdrPlugin {
    fn build(&self, app: &mut App) {
        let settings = self.0.clamped();
        publish(&settings, Pattern::None);
        app.insert_resource(settings)
            .add_systems(Startup, move || {
                if settings.enabled {
                    info!("hdr output: {}", settings.describe());
                }
            })
            .add_systems(Update, (display_peak, follow_settings).chain())
            // After graphics.rs's `apply`, which takes the HDR view off with the light glow,
            // and imagespace.rs's `update_grade`, which writes the grade whole (both Update).
            .add_systems(PostUpdate, main_camera);
        app.add_plugins(ExtractComponentPlugin::<PreviewSource>::default());
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .init_resource::<WindowEncode>()
            .init_resource::<ScenePreview>()
            .init_resource::<SceneKey>()
            .add_systems(PrePresent, (encode_windows, read_scene_key))
            // The graded picture (its highlights in), before the UI is drawn over it.
            .add_systems(
                Core3d,
                capture_views.after(Core3dSystems::PostProcess).after(fullscreen_material_system::<crate::imagespace::ImageSpaceGrade>).before(ui_pass),
            );
    }
}

/// The settings where the renderer and the presenter read them: the surface's request (not
/// while DLSS Frame Generation is loaded, module notes) and the encode's uniform.
fn publish(settings: &HdrOutput, pattern: Pattern) {
    HDR_OUTPUT_REQUEST.store(settings.enabled, Ordering::Relaxed);
    *ENCODE.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = settings.encode(pattern);
}

/// The pattern for this frame: the pause menu's settings page with an HDR row chosen.
fn chosen_pattern(world: &World) -> Pattern {
    use nvf_play::pause::{Page, Paused};
    use nvf_play::settings_view::RowId;
    if !world.get_resource::<Paused>().is_some_and(|p| p.0) {
        return Pattern::None;
    }
    let Some(Page::Settings { selected: Some(RowId::Setting(i)) }) = world.get_resource::<Page>().copied() else { return Pattern::None };
    world.get_resource::<nvf_play::settings::Settings>().and_then(|s| s.list().get(i)).map_or(Pattern::None, |row| Pattern::for_row(row.key))
}

fn follow_settings(world: &World, mut told: Local<Option<HdrOutput>>, mut dlssg_told: Local<bool>, mut shown: Local<Pattern>) {
    let settings = *world.resource::<HdrOutput>();
    let pattern = chosen_pattern(world);
    if pattern != *shown {
        *shown = pattern;
        info!("hdr output: calibration pattern {pattern:?}");
    }
    publish(&settings, pattern);
    if settings.enabled && active() && crate::dlss::frame_generation_loaded() && !std::mem::replace(&mut *dlssg_told, true) {
        warn!("hdr output: with DLSS Frame Generation loaded: experimental (its HUD-less copy is encoded as the frame is; untested on the owner's RTX 3070)");
    }
    if told.is_some_and(|t| t != settings) {
        info!("hdr output: {}", settings.describe());
    }
    *told = Some(settings);
}

/// The main camera while the surface is HDR: the HDR (fp16) view, so the grade's highlights
/// above 1 last until the encode, and the grade's highlight settings. When HDR output goes
/// off, the HDR view it put on comes off again unless the light glow or a temporal upscaler
/// wants it (graphics.rs `apply`).
#[allow(clippy::type_complexity)]
fn main_camera(
    mut commands: Commands,
    settings: Res<HdrOutput>,
    graphics: Res<crate::graphics::Graphics>,
    mut cams: Query<
        (Entity, Has<bevy::camera::Hdr>, Has<crate::fsr::FsrUpscaling>, Has<crate::upscaler::DlssUpscaling>, Option<&mut crate::imagespace::ImageSpaceGrade>, Has<PreviewSource>),
        With<crate::fly::FlyCam>,
    >,
    mut put_on: Local<bool>,
    mut was: Local<bool>,
) {
    let on = active();
    if on != *was {
        *was = on;
        info!("hdr output: the surface is {}", if on { "HDR (scRGB)" } else { "SDR" });
    }
    for (cam, hdr, fsr, dlss, grade, source) in &mut cams {
        if on {
            if !hdr {
                commands.entity(cam).insert(bevy::camera::Hdr);
                *put_on = true;
            }
            if !source {
                commands.entity(cam).insert(PreviewSource);
            }
            if let Some(mut grade) = grade {
                let want = settings.grade();
                if grade.hdr != want {
                    grade.hdr = want;
                }
            }
        } else if *put_on {
            *put_on = false;
            if hdr && !graphics.glow && !fsr && !dlss {
                commands.entity(cam).remove::<bevy::camera::Hdr>();
            }
        }
    }
}

/// The encode's pipeline, for any thread with the device (the render thread, frame
/// generation's presenter).
#[derive(Default)]
pub(crate) struct Encoder {
    made: Option<(wgpu::RenderPipeline, wgpu::BindGroupLayout, wgpu::Sampler, wgpu::Buffer, wgpu::TextureView)>,
}

impl Encoder {
    /// Draw `source` encoded over the whole of `target` (an image of the HDR surface).
    pub(crate) fn draw(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, source: &wgpu::TextureView, target: &wgpu::TextureView) {
        let (pipeline, layout, sampler, buffer, no_preview) = self.made.get_or_insert_with(|| {
            let entry = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty, count: None };
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("hdr_encode"),
                entries: &[
                    entry(0, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }),
                    entry(1, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                    entry(2, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
                    entry(3, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }),
                ],
            });
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("hdr_encode"), source: wgpu::ShaderSource::Wgsl(include_str!("hdr_encode.wgsl").into()) });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("hdr_encode"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("hdr_encode"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some("vertex"), compilation_options: default(), buffers: &[] },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fragment"),
                    compilation_options: default(),
                    targets: &[Some(wgpu::ColorTargetState { format: SURFACE_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                }),
                primitive: default(),
                depth_stencil: None,
                multisample: default(),
                multiview_mask: None,
                cache: None,
            });
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("hdr_encode"), ..default() });
            let buffer = device.create_buffer(&wgpu::BufferDescriptor { label: Some("hdr_encode"), size: 48, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            // Bound while there is no scene preview ([`ScenePreview`]).
            let no_preview = device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("hdr_encode_no_preview"),
                    size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: SURFACE_FORMAT,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&default());
            (pipeline, layout, sampler, buffer, no_preview)
        });
        let preview = PREVIEW.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        let params = *ENCODE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let bytes: Vec<u8> = params.iter().flat_map(|v| v.to_le_bytes()).collect();
        queue.write_buffer(buffer, 0, &bytes);
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("hdr_encode"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(source) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(preview.as_ref().unwrap_or(no_preview)) },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("nvf_hdr_encode"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// On the main camera while the surface is HDR: [`capture_preview`] takes its picture.
#[derive(Component, ExtractComponent, Clone, Copy)]
#[extract_app(bevy::render::RenderApp)]
pub struct PreviewSource;

/// The scene preview's size: small, as the pattern's box is about a quarter of the window.
const PREVIEW_SIZE: (u32, u32) = (640, 360);

/// The scene preview the encode draws in the pattern's box for the paper white and highlight
/// rows: the main view's graded picture (its highlights in, 1.0 = paper white) before the
/// UI, scaled down each frame while such a row is chosen, so a slider's change shows on the
/// player's own scene at once [design: the owner, 2026-11-20, asked whether an in-game picture
/// would be better than patterns: the patterns stay for the display's limits, the scene is
/// for what is taste].
static PREVIEW: Mutex<Option<wgpu::TextureView>> = Mutex::new(None);

#[derive(Resource, Default)]
struct ScenePreview {
    target: Option<(wgpu::Texture, wgpu::TextureView)>,
    blit: crate::frame_gen::Blit,
}

/// The scene key's sample: the size it is drawn down to, and every how many frames.
const KEY_SIZE: (u32, u32) = (64, 36);
const KEY_EVERY: u32 = 4;

/// The picture's average luminance for the knee ([`knee_for`]): the main view's graded
/// picture drawn down to [`KEY_SIZE`] every [`KEY_EVERY`] frames, copied to a buffer and read
/// back once the GPU is done with it (never waited for), smoothed over about half a second.
#[derive(Resource, Default)]
struct SceneKey {
    small: Option<(wgpu::Texture, wgpu::TextureView, wgpu::Buffer)>,
    blit: crate::frame_gen::Blit,
    state: KeyState,
    mapped: std::sync::Arc<std::sync::atomic::AtomicBool>,
    frame: u32,
    logged: Option<std::time::Instant>,
}

#[derive(Default, PartialEq, Eq, Clone, Copy)]
enum KeyState {
    #[default]
    Idle,
    /// The copy is recorded; it is mapped once submitted.
    Copied,
    /// Waiting for the map.
    Mapping,
}

/// The main view's picture into the scene key's sample and, while the paper white pattern
/// is up, into the scene preview.
fn capture_views(
    view: ViewQuery<&ViewTarget, With<PreviewSource>>,
    device: Res<RenderDevice>,
    mut preview: ResMut<ScenePreview>,
    mut key: ResMut<SceneKey>,
    mut ctx: RenderContext,
) {
    let target = view.into_inner();
    if !active() || target.main_texture_format() != SURFACE_FORMAT {
        return;
    }
    let device = device.wgpu_device();
    let key = &mut *key;
    key.frame = key.frame.wrapping_add(1);
    if key.state == KeyState::Idle && key.frame.is_multiple_of(KEY_EVERY) {
        let (texture, view, buffer) = key.small.get_or_insert_with(|| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("hdr_scene_key"),
                size: wgpu::Extent3d { width: KEY_SIZE.0, height: KEY_SIZE.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SURFACE_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&default());
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("hdr_scene_key"),
                size: (KEY_SIZE.0 * 8 * KEY_SIZE.1) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            (texture, view, buffer)
        });
        key.blit.draw(device, ctx.command_encoder(), target.main_texture_view(), view, SURFACE_FORMAT, "hdr_scene_key");
        ctx.command_encoder().copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo { buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(KEY_SIZE.0 * 8), rows_per_image: None } },
            wgpu::Extent3d { width: KEY_SIZE.0, height: KEY_SIZE.1, depth_or_array_layers: 1 },
        );
        key.state = KeyState::Copied;
    }
    let pattern = ENCODE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)[4];
    if !(Pattern::PaperWhite.shows_scene() && pattern == Pattern::PaperWhite as u32 as f32) {
        return;
    }
    let ScenePreview { target: out, blit } = &mut *preview;
    let (_, out_view) = out.get_or_insert_with(|| {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hdr_scene_preview"),
            size: wgpu::Extent3d { width: PREVIEW_SIZE.0, height: PREVIEW_SIZE.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SURFACE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&default());
        *PREVIEW.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(view.clone());
        (texture, view)
    });
    blit.draw(device, ctx.command_encoder(), target.main_texture_view(), out_view, SURFACE_FORMAT, "hdr_scene_preview");
}

/// After the frame is submitted (`PrePresent`): the scene key's copy mapped, and a mapped one
/// read into [`SCENE_AVERAGE`].
fn read_scene_key(mut key: ResMut<SceneKey>) {
    let key = &mut *key;
    let Some((_, _, buffer)) = key.small.as_ref() else { return };
    match key.state {
        KeyState::Idle => {}
        KeyState::Copied => {
            let mapped = key.mapped.clone();
            mapped.store(false, Ordering::Relaxed);
            buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| mapped.store(r.is_ok(), Ordering::Release));
            key.state = KeyState::Mapping;
        }
        KeyState::Mapping => {
            if !key.mapped.load(Ordering::Acquire) {
                return;
            }
            let average = {
                let Ok(data) = buffer.slice(..).get_mapped_range() else { return };
                let f = |at: usize| half::f16::from_bits(u16::from_le_bytes([data[at], data[at + 1]])).to_f32().clamp(0.0, 1.0);
                let n = (KEY_SIZE.0 * KEY_SIZE.1) as usize;
                (0..n).map(|i| 0.2126 * f(i * 8) + 0.7152 * f(i * 8 + 2) + 0.0722 * f(i * 8 + 4)).sum::<f32>() / n as f32
            };
            buffer.unmap();
            key.state = KeyState::Idle;
            // About half a second at 60 frames a second, a sample every fourth.
            let smoothed = scene_average() + (average - scene_average()) * 0.12;
            SCENE_AVERAGE.store(smoothed.to_bits(), Ordering::Relaxed);
            if key.logged.is_none_or(|t| t.elapsed().as_secs() >= 10) {
                key.logged = Some(std::time::Instant::now());
                info!("hdr output: the picture's average {smoothed:.3}, highlights from {:.2} to {:.2} (brightest channel)", knee_for(smoothed), white_for(smoothed));
            }
        }
    }
}

/// The encode of a window's image in place, on the render thread: the image copied out, then
/// drawn back encoded.
#[derive(Resource, Default)]
pub(crate) struct WindowEncode {
    encoder: Encoder,
    scratch: Option<(wgpu::Texture, wgpu::TextureView)>,
}

impl WindowEncode {
    /// Encode the window's current image, when it has one and the surface is HDR.
    pub(crate) fn encode_window(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, window: &ExtractedWindow, site: &'static str) {
        // The image itself, not the window's view: on a screenshot's frame that view is the
        // screenshot's texture, which Bevy copies into the image afterwards.
        let Some(image) = window.swap_chain_texture.as_ref() else { return };
        if !active() || image.texture.format() != SURFACE_FORMAT {
            return;
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("hdr_encode") });
        self.encode_in_place(device, queue, &mut encoder, &image.texture);
        queue.submit([encoder.finish()]);
        probe(device, queue, &image.texture, site);
    }

    /// Record the encode of `texture` (an `Rgba16Float` render attachment that can be copied
    /// from) in place into `encoder`: copied out, drawn back encoded. DLSS Frame Generation's
    /// HUD-less copy goes through it too (`dlss/frame_gen.rs`), so the frames DLSS-G makes from
    /// it are encoded as the real ones are.
    pub(crate) fn encode_in_place(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, texture: &wgpu::Texture) {
        let size = texture.size();
        if self.scratch.as_ref().is_none_or(|(t, _)| t.size() != size) {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("hdr_encode_scratch"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SURFACE_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&default());
            self.scratch = Some((texture, view));
        }
        let Some((scratch, scratch_view)) = self.scratch.as_ref() else { return };
        let view = texture.create_view(&default());
        encoder.copy_texture_to_texture(texture.as_image_copy(), scratch.as_image_copy(), size);
        self.encoder.draw(device, queue, encoder, scratch_view, &view);
    }
}

/// Every window image Bevy presents, encoded (`PrePresent`: after the screenshots' copies).
/// Frame generation's presenter thread takes its own images and encodes them itself.
fn encode_windows(windows: Query<&ExtractedWindow>, device: Res<RenderDevice>, queue: Res<RenderQueue>, mut state: ResMut<WindowEncode>) {
    if !active() {
        return;
    }
    for window in &windows {
        state.encode_window(device.wgpu_device(), &queue, window, "present");
    }
}

/// Test aid `NVF_HDR_PROBE=<n>[:<every>]`: `n` window images (every `every`th, 1 if left out) each encode site (`present`:
/// Bevy's present; `fg`: frame generation's presenter thread; `fg-generated`: its render-thread
/// fallback) wrote are read back after the encode and logged as "hdr probe: <site> <i>: …" in
/// nits (scRGB 1.0 = 80): the median, the 99.9th and 1st percentiles of the luminance over the
/// image (`NVF_HDR_PROBE_DIR` keeps the images). Each read waits for the GPU: a test aid,
/// never in play.
pub(crate) fn probe(device: &wgpu::Device, queue: &wgpu::Queue, image: &wgpu::Texture, site: &'static str) {
    static LIMIT: std::sync::OnceLock<(usize, usize)> = std::sync::OnceLock::new();
    static COUNTS: Mutex<Vec<(&'static str, usize)>> = Mutex::new(Vec::new());
    let (limit, every) = *LIMIT.get_or_init(|| {
        let v = std::env::var("NVF_HDR_PROBE").unwrap_or_default();
        let mut parts = v.trim().split(':').map(|p| p.parse::<usize>().ok());
        (parts.next().flatten().unwrap_or(0), parts.next().flatten().unwrap_or(1).max(1))
    });
    if limit == 0 || image.format() != SURFACE_FORMAT || !image.usage().contains(wgpu::TextureUsages::COPY_SRC) {
        return;
    }
    let index = {
        let mut counts = COUNTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let entry = match counts.iter_mut().find(|(s, _)| *s == site) {
            Some(e) => e,
            None => {
                counts.push((site, 0));
                counts.last_mut().unwrap()
            }
        };
        entry.1 += 1;
        if entry.1 % every != 0 || entry.1 / every > limit {
            return;
        }
        entry.1
    };
    let size = image.size();
    let row = (size.width * 8).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor { label: Some("hdr_probe"), size: (row * size.height) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("hdr_probe") });
    encoder.copy_texture_to_buffer(
        image.as_image_copy(),
        wgpu::TexelCopyBufferInfo { buffer: &buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None } },
        size,
    );
    queue.submit([encoder.finish()]);
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    if device.poll(wgpu::PollType::wait_indefinitely()).is_err() {
        return;
    }
    let Ok(data) = buffer.slice(..).get_mapped_range() else { return };
    // `NVF_HDR_PROBE_DIR=<folder>`: each read image saved as `<site>-<i>.f16` (width and
    // height as two little-endian u32, then the rows of RGBA f16, padding dropped), for
    // looking at the encoded picture off the display. Never into the game's folders.
    if let Some(dir) = std::env::var_os("NVF_HDR_PROBE_DIR").map(std::path::PathBuf::from) {
        if fnv_install::guard_write(&dir, None).is_ok() && std::fs::create_dir_all(&dir).is_ok() {
            let mut out = Vec::with_capacity(8 + (size.width * size.height * 8) as usize);
            out.extend_from_slice(&size.width.to_le_bytes());
            out.extend_from_slice(&size.height.to_le_bytes());
            for y in 0..size.height {
                let at = (y * row) as usize;
                out.extend_from_slice(&data[at..at + (size.width * 8) as usize]);
            }
            let _ = std::fs::write(dir.join(format!("{site}-{index}.f16")), out);
        }
    }
    let f = |at: usize| half::f16::from_bits(u16::from_le_bytes([data[at], data[at + 1]])).to_f32();
    let mut nits: Vec<f32> = Vec::with_capacity((size.width * size.height / 4) as usize);
    for y in (0..size.height).step_by(2) {
        for x in (0..size.width).step_by(2) {
            let at = (y * row + x * 8) as usize;
            nits.push((0.2126 * f(at) + 0.7152 * f(at + 2) + 0.0722 * f(at + 4)) * SCRGB_NITS);
        }
    }
    nits.sort_by(f32::total_cmp);
    let q = |p: f64| nits[((nits.len() - 1) as f64 * p) as usize];
    info!("hdr probe: {site} {index}: median {:.1} nits, p99.9 {:.0}, max {:.0}, p1 {:.3}, black {:.3}", q(0.5), q(0.999), q(1.0), q(0.01), nits.iter().filter(|n| **n < 0.01).count() as f32 / nits.len() as f32);
}

/// The peak row's part: Auto first (0), then [`PEAK`]'s steps.
fn peak_part(peak: Option<f32>) -> f32 {
    let n = PEAK.2;
    match peak {
        None => 0.0,
        Some(p) => {
            let i = ((p - PEAK.0) / (PEAK.1 - PEAK.0) * (n - 1) as f32).round().clamp(0.0, (n - 1) as f32);
            ((i + 1.0) / n as f32 * 1000.0).round() / 1000.0
        }
    }
}

/// The peak for the peak row's part: `None` for Auto.
fn peak_value(v: f32) -> Option<f32> {
    let n = PEAK.2;
    let i = (v.clamp(0.0, 1.0) * n as f32).round() as usize;
    (i > 0).then(|| PEAK.0 + (i - 1) as f32 * (PEAK.1 - PEAK.0) / (n - 1) as f32)
}

fn peak_text(v: f32) -> String {
    match peak_value(v) {
        Some(p) => format!("{p:.0} nits"),
        None => match detected_peak() {
            Some(p) => format!("Auto ({p:.0} nits)"),
            None => format!("Auto ({FALLBACK_PEAK:.0} nits)"),
        },
    }
}

/// The brightness row's part for a paper white: its nearest choice.
fn paper_part(paper: f32) -> f32 {
    let i = PAPER_STEPS.iter().enumerate().min_by(|a, b| (a.1 - paper).abs().total_cmp(&(b.1 - paper).abs())).map_or(0, |(i, _)| i);
    (i as f32 / (PAPER_STEPS.len() - 1) as f32 * 1000.0).round() / 1000.0
}

fn paper_value(v: f32) -> f32 {
    PAPER_STEPS[(v.clamp(0.0, 1.0) * (PAPER_STEPS.len() - 1) as f32).round() as usize]
}

fn edit(world: &mut World, change: impl FnOnce(&mut HdrOutput)) {
    let mut hdr = world.resource_mut::<HdrOutput>();
    let mut next = *hdr;
    change(&mut next);
    let next = next.clamped();
    if next != *hdr {
        *hdr = next;
    }
}

/// The settings page's rows (Display > HDR, nvf-settings-layout), each kept in the INI.
pub(crate) fn register_settings(app: &mut App, group: &'static str) {
    add_setting(
        app,
        Setting {
            key: "hdr",
            group,
            label: "HDR output",
            note: "Brighter highlights and deeper blacks on an HDR display (Windows' HDR on for it). With DLSS Frame Generation: experimental.",
            switches: "--hdr",
            get: |w| w.resource::<HdrOutput>().enabled,
            set: |w, on| edit(w, |h| h.enabled = on),
            slider: None,
        },
    );
    add_setting(
        app,
        Setting::slider(
            "hdr-peak-brightness",
            group,
            "HDR peak brightness",
            "Your display's brightest. Auto: what Windows reports for it. Or raise it until the plus at the top right just vanishes (best with HGIG on).",
            Slider::choice(PEAK.2 + 1, |w| peak_part(w.resource::<HdrOutput>().peak_nits), |w, v| edit(w, |h| h.peak_nits = peak_value(v)), peak_text),
        )
        .with_switches("--hdr-peak"),
    );
    add_setting(
        app,
        Setting::slider(
            "hdr-brightness",
            group,
            "HDR brightness",
            "The game's brightness, in nits (white text, daylight): set it to taste for your room on your scene in the box at the top right.",
            Slider::choice(PAPER_STEPS.len(), |w| paper_part(w.resource::<HdrOutput>().paper_white_nits), |w, v| edit(w, |h| h.paper_white_nits = paper_value(v)), |v| format!("{:.0} nits", paper_value(v))),
        )
        .with_switches("--hdr-paper-white"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_stay_in_range() {
        let wild = HdrOutput { enabled: true, peak_nits: Some(50.0), paper_white_nits: 900.0, black_floor: 1.0, gamma22: true, highlights: 3.0, knee: Some(2.0) }.clamped();
        assert_eq!(wild.peak_nits, Some(100.0));
        assert_eq!(wild.paper(), 100.0, "paper white never above the peak");
        assert_eq!(wild.black_floor, 0.05);
        assert_eq!(wild.highlights, 1.0);
        assert_eq!(wild.knee, Some(0.9));
    }

    #[test]
    fn units() {
        let hdr = HdrOutput { peak_nits: Some(1000.0), paper_white_nits: 200.0, ..HdrOutput::default() };
        assert_eq!(hdr.encode(Pattern::None)[..4], [2.5, 12.5, 0.0, 1.0], "scRGB: 1.0 is 80 nits");
        assert_eq!(hdr.encode(Pattern::Peak)[4], 1.0);
        let fixed = HdrOutput { knee: Some(0.3), ..hdr };
        assert_eq!(fixed.grade(), Vec4::new(1.0, 5.0, 0.3, 1.0), "the peak in paper whites, a fixed knee");
    }

    #[test]
    fn sliders_land_on_their_steps() {
        assert_eq!(peak_value(peak_part(None)), None, "Auto");
        for i in 0..PEAK.2 {
            let p = PEAK.0 + i as f32 * 50.0;
            assert_eq!(peak_value(peak_part(Some(p))), Some(p));
        }
        for p in PAPER_STEPS {
            assert_eq!(paper_value(paper_part(p)), p);
        }
        let d = HdrOutput::default();
        assert_eq!(d.peak_nits, None, "Auto by default");
        assert_eq!(d.paper_white_nits, 203.0, "BT.2408's reference white");
        assert_eq!(paper_value(paper_part(d.paper_white_nits)), 203.0);
        assert_eq!(paper_value(paper_part(1000.0)), 400.0, "the nearest choice");
    }

    #[test]
    fn display_reports() {
        let oled = DisplayLuminance { name: String::new(), peak: 800.0, full_frame: 200.0, min: 0.0005, hdr: true };
        assert_eq!(usable_peak(&oled), Some(800.0));
        assert_eq!(usable_peak(&DisplayLuminance { hdr: false, ..oled.clone() }), None, "not in HDR now");
        assert_eq!(usable_peak(&DisplayLuminance { peak: 0.0, ..oled.clone() }), None, "nothing reported");
        assert_eq!(usable_peak(&DisplayLuminance { peak: 270.0, ..oled.clone() }), None, "an SDR panel's report");
        let stand_in = DisplayLuminance { peak: 4000.0, full_frame: 4000.0, min: 0.1199, ..oled.clone() };
        assert_eq!(usable_peak(&stand_in), None, "the owner's OLED before calibration (2026-11-20)");
        assert_eq!(usable_peak(&DisplayLuminance { peak: 1000.0, full_frame: 1000.0, ..oled }), Some(1000.0), "an LCD holding 1000 nits over the screen");
    }

    #[test]
    fn the_knee_follows_the_picture() {
        // Nipton at night (2026-11-20): an average near 0.02, the fire's brightest channel
        // 0.4-0.55: above the knee now.
        assert_eq!(knee_for(0.02), KNEE_RANGE.0);
        assert!(knee_for(0.15) > 0.3 && knee_for(0.15) < KNEE_RANGE.1, "a bright day keeps walls and sand as in SDR");
        assert_eq!(knee_for(1.0), KNEE_RANGE.1);
        assert_eq!(white_for(0.03), WHITE_RANGE.0, "at night a fire at 0.5 gets the peak's gain");
        assert_eq!(white_for(0.15), 1.0, "a bright day: SDR white to the peak");
        assert!(knee_for(0.03) < white_for(0.03));
    }
}
