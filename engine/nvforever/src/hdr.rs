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
//!   - DLSS Frame Generation is not offered with HDR output, and HDR output is not taken while
//!     DLSS-G is loaded: Streamline's HUD-less and UI buffers would have to be encoded as well
//!     (untested here: the owner's RTX 3070 cannot run DLSS-G).
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

use bevy::prelude::*;
use bevy::render::renderer::{PrePresent, RenderDevice, RenderQueue};
use bevy::render::view::{ExtractedWindow, HDR_OUTPUT_ACTIVE, HDR_OUTPUT_REQUEST};
use bevy::render::RenderApp;
use nvf_play::settings::{add_setting, Setting, Slider};
use std::sync::atomic::Ordering;
use std::sync::Mutex;

/// The surface's format with HDR output.
pub(crate) const SURFACE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// scRGB's 1.0, in nits.
const SCRGB_NITS: f32 = 80.0;

/// The settings page's ranges.
const PEAK: (f32, f32, usize) = (400.0, 2000.0, 33);
const PAPER: (f32, f32, usize) = (80.0, 400.0, 33);
const BLACK: (f32, f32, usize) = (-0.010, 0.010, 21);
const KNEE: (f32, f32, usize) = (0.10, 0.60, 11);

/// The HDR output's settings (the settings page's HDR rows, `--hdr-*`).
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct HdrOutput {
    /// Asked for (the surface is HDR only where the display offers it: [`active`]).
    pub enabled: bool,
    /// The display's brightest, in nits.
    pub peak_nits: f32,
    /// What SDR's white (the UI, a white wall in daylight) shows at, in nits.
    pub paper_white_nits: f32,
    /// Subtracted from the SDR range's linear values and stretched back (< 0 lifts).
    pub black_floor: f32,
    /// Read the SDR range as a gamma 2.2 screen shows it rather than by the sRGB curve.
    pub gamma22: bool,
    /// How far the highlights rise: 0 as SDR, 1 to the peak.
    pub highlights: f32,
    /// The graded picture's luminance (linear) where the highlights start to rise: lower
    /// lifts more of the picture.
    pub knee: f32,
}

impl Default for HdrOutput {
    fn default() -> Self {
        Self { enabled: false, peak_nits: 1000.0, paper_white_nits: 200.0, black_floor: 0.0, gamma22: true, highlights: 1.0, knee: 0.25 }
    }
}

impl HdrOutput {
    pub fn clamped(mut self) -> Self {
        self.peak_nits = self.peak_nits.clamp(100.0, 10_000.0);
        self.paper_white_nits = self.paper_white_nits.clamp(SCRGB_NITS, self.peak_nits);
        self.black_floor = self.black_floor.clamp(-0.05, 0.05);
        self.highlights = self.highlights.clamp(0.0, 1.0);
        self.knee = self.knee.clamp(0.0, 0.9);
        self
    }

    /// The grade's `hdr` (imagespace.rs `ImageSpaceGrade::hdr`): on, the peak in paper
    /// whites, the knee, the strength.
    pub fn grade(&self) -> Vec4 {
        Vec4::new(1.0, self.peak_nits / self.paper_white_nits, self.knee, self.highlights)
    }

    /// `hdr_encode.wgsl`'s uniform: paper white and the peak in scRGB units, the black level,
    /// gamma 2.2 (1) or sRGB (0); the calibration pattern, the peak in paper whites, the knee,
    /// the highlights' strength; the pattern's box.
    fn encode(&self, pattern: Pattern) -> [f32; 12] {
        let [x0, y0, x1, y1] = PATTERN_BOX;
        [
            self.paper_white_nits / SCRGB_NITS,
            self.peak_nits / SCRGB_NITS,
            self.black_floor,
            if self.gamma22 { 1.0 } else { 0.0 },
            pattern as u32 as f32,
            self.peak_nits / self.paper_white_nits,
            self.knee,
            self.highlights,
            x0,
            y0,
            x1,
            y1,
        ]
    }

    pub fn describe(&self) -> String {
        format!(
            "{}; peak {:.0} nits, paper white {:.0} nits, black level {:+.3}, {}, highlights {:.0}% from {:.2}",
            if self.enabled { "on" } else { "off" },
            self.peak_nits,
            self.paper_white_nits,
            self.black_floor,
            if self.gamma22 { "gamma 2.2 blacks" } else { "sRGB blacks" },
            self.highlights * 100.0,
            self.knee
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
    /// Near-black patches on true black: the darkest should just vanish.
    Shadows = 2,
    /// A ramp and paper white.
    PaperWhite = 3,
    /// The SDR ramp over the same ramp with the highlight curve, the knee marked.
    Highlights = 4,
}

impl Pattern {
    /// The pattern for a settings row's key.
    pub(crate) fn for_row(key: &str) -> Pattern {
        match key {
            "hdr-peak" => Pattern::Peak,
            "hdr-black-level" | "hdr-gamma" => Pattern::Shadows,
            "hdr-paper-white" => Pattern::PaperWhite,
            "hdr-highlights" | "hdr-highlight-start" => Pattern::Highlights,
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

/// HDR output is asked for (the frame generation row's DLSS choice is not offered then).
pub(crate) fn asked(world: &World) -> bool {
    world.get_resource::<HdrOutput>().is_some_and(|h| h.enabled)
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
            .add_systems(Update, follow_settings)
            // After graphics.rs's `apply`, which takes the HDR view off with the light glow,
            // and imagespace.rs's `update_grade`, which writes the grade whole (both Update).
            .add_systems(PostUpdate, main_camera);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app.init_resource::<WindowEncode>().add_systems(PrePresent, encode_windows);
    }
}

/// The settings where the renderer and the presenter read them: the surface's request (not
/// while DLSS Frame Generation is loaded, module notes) and the encode's uniform.
fn publish(settings: &HdrOutput, pattern: Pattern) {
    HDR_OUTPUT_REQUEST.store(settings.enabled && !crate::dlss::frame_generation_loaded(), Ordering::Relaxed);
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
    if settings.enabled && crate::dlss::frame_generation_loaded() && !std::mem::replace(&mut *dlssg_told, true) {
        warn!("hdr output: not while DLSS Frame Generation is loaded; SDR");
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
        (Entity, Has<bevy::camera::Hdr>, Has<crate::fsr::FsrUpscaling>, Has<crate::upscaler::DlssUpscaling>, Option<&mut crate::imagespace::ImageSpaceGrade>),
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
    for (cam, hdr, fsr, dlss, grade) in &mut cams {
        if on {
            if !hdr {
                commands.entity(cam).insert(bevy::camera::Hdr);
                *put_on = true;
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
    made: Option<(wgpu::RenderPipeline, wgpu::BindGroupLayout, wgpu::Sampler, wgpu::Buffer)>,
}

impl Encoder {
    /// Draw `source` encoded over the whole of `target` (an image of the HDR surface).
    pub(crate) fn draw(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, source: &wgpu::TextureView, target: &wgpu::TextureView) {
        let (pipeline, layout, sampler, buffer) = self.made.get_or_insert_with(|| {
            let entry = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty, count: None };
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("hdr_encode"),
                entries: &[
                    entry(0, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }),
                    entry(1, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                    entry(2, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
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
            (pipeline, layout, sampler, buffer)
        });
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
        let size = image.texture.size();
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
        let view = image.texture.create_view(&default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("hdr_encode") });
        encoder.copy_texture_to_texture(image.texture.as_image_copy(), scratch.as_image_copy(), size);
        self.encoder.draw(device, queue, &mut encoder, scratch_view, &view);
        queue.submit([encoder.finish()]);
        probe(device, queue, &image.texture, site);
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

/// A slider part for `v` in `(min, max)`, as `Slider::fit` keeps parts.
fn part(v: f32, (min, max, _): (f32, f32, usize)) -> f32 {
    ((v - min) / (max - min) * 1000.0).round().clamp(0.0, 1000.0) / 1000.0
}

/// The value for a slider part, on the range's steps.
fn value(p: f32, (min, max, n): (f32, f32, usize)) -> f32 {
    let i = (p.clamp(0.0, 1.0) * (n - 1) as f32).round();
    min + i * (max - min) / (n - 1) as f32
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
            note: "Brighter highlights and deeper blacks on an HDR display (Windows' HDR on for it). Not with DLSS Frame Generation.",
            switches: "--hdr",
            get: |w| w.resource::<HdrOutput>().enabled,
            set: |w, on| edit(w, |h| h.enabled = on),
            slider: None,
        },
    );
    add_setting(
        app,
        Setting::slider(
            "hdr-peak",
            group,
            "HDR peak brightness",
            "Your display's brightest, in nits. Raise it until the plus in the box at the top right just vanishes into the box (best with the display's HGIG mode).",
            Slider::choice(PEAK.2, |w| part(w.resource::<HdrOutput>().peak_nits, PEAK), |w, v| edit(w, |h| h.peak_nits = value(v, PEAK)), |v| format!("{:.0} nits", value(v, PEAK))),
        )
        .with_switches("--hdr-peak"),
    );
    add_setting(
        app,
        Setting::slider(
            "hdr-paper-white",
            group,
            "HDR paper white",
            "How bright white text and the daylight picture are, in nits: the box at the top right shows it; set it to taste against the room.",
            Slider::choice(PAPER.2, |w| part(w.resource::<HdrOutput>().paper_white_nits, PAPER), |w, v| edit(w, |h| h.paper_white_nits = value(v, PAPER)), |v| format!("{:.0} nits", value(v, PAPER))),
        )
        .with_switches("--hdr-paper-white"),
    );
    add_setting(
        app,
        Setting::slider(
            "hdr-black-level",
            group,
            "HDR black level",
            "Set it so the darkest of the patches at the top right just vanishes into black and the next stays visible.",
            Slider::choice(
                BLACK.2,
                |w| part(w.resource::<HdrOutput>().black_floor, BLACK),
                |w, v| edit(w, |h| h.black_floor = value(v, BLACK)),
                |v| match (value(v, BLACK) * 1000.0).round() as i32 {
                    0 => "0".to_string(),
                    n => format!("{n:+}"),
                },
            ),
        )
        .with_switches("--hdr-black-floor"),
    );
    add_setting(
        app,
        Setting {
            key: "hdr-gamma",
            group,
            label: "HDR gamma 2.2 shadows",
            note: "Shows dark shades as the gamma 2.2 screens the game was made on do: deeper blacks. Off: the sRGB curve, lighter shadows.",
            switches: "--hdr-srgb-blacks",
            get: |w| w.resource::<HdrOutput>().gamma22,
            set: |w, on| edit(w, |h| h.gamma22 = on),
            slider: None,
        },
    );
    add_setting(
        app,
        Setting::slider(
            "hdr-highlights",
            group,
            "HDR highlights",
            "How far highlights (sky, lights, fire) rise above paper white. The box at the top right: SDR above, with highlights below.",
            Slider { get: |w| (w.resource::<HdrOutput>().highlights * 20.0).round() / 20.0, set: |w, v| edit(w, |h| h.highlights = v), min: 0.0, step: 0.05, shown: None },
        ),
    );
    add_setting(
        app,
        Setting::slider(
            "hdr-highlight-start",
            group,
            "HDR highlight start",
            "Where highlights start to rise (the red mark in the box at the top right); lower brightens more of the picture.",
            Slider::choice(KNEE.2, |w| part(w.resource::<HdrOutput>().knee, KNEE), |w, v| edit(w, |h| h.knee = value(v, KNEE)), |v| format!("{:.0}%", value(v, KNEE) * 100.0)),
        )
        .with_switches("--hdr-knee"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_stay_in_range() {
        let wild = HdrOutput { enabled: true, peak_nits: 50.0, paper_white_nits: 900.0, black_floor: 1.0, gamma22: true, highlights: 3.0, knee: 2.0 }.clamped();
        assert_eq!(wild.peak_nits, 100.0);
        assert_eq!(wild.paper_white_nits, 100.0, "paper white never above the peak");
        assert_eq!(wild.black_floor, 0.05);
        assert_eq!(wild.highlights, 1.0);
        assert_eq!(wild.knee, 0.9);
    }

    #[test]
    fn units() {
        let hdr = HdrOutput::default();
        assert_eq!(hdr.encode(Pattern::None)[..4], [2.5, 12.5, 0.0, 1.0], "scRGB: 1.0 is 80 nits");
        assert_eq!(hdr.encode(Pattern::Peak)[4], 1.0);
        assert_eq!(hdr.grade(), Vec4::new(1.0, 5.0, 0.25, 1.0), "the peak in paper whites");
    }

    #[test]
    fn sliders_land_on_their_steps() {
        for range in [PEAK, PAPER, BLACK, KNEE] {
            for i in 0..range.2 {
                let v = range.0 + i as f32 * (range.1 - range.0) / (range.2 - 1) as f32;
                assert!((value(part(v, range), range) - v).abs() < 1e-4, "{v} in {range:?}");
            }
        }
        let d = HdrOutput::default();
        assert_eq!(value(part(d.peak_nits, PEAK), PEAK), 1000.0);
        assert_eq!(value(part(d.paper_white_nits, PAPER), PAPER), 200.0);
        assert!((value(part(d.knee, KNEE), KNEE) - 0.25).abs() < 1e-4, "the default knee is a step");
    }
}
