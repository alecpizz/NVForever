//! HDR output on an HDR screen (docs/design/hdr-output.md): the Display page's HDR rows (switched
//! live, with no restart; the calibration rows too), or `NVF_HDR=1` for a run.
//!
//! - The window's surface is `Rgba16Float`, which wgpu's `SurfaceColorSpace::Auto` makes
//!   scRGB (vendored bevy_render `view/window/mod.rs`).
//! - The main view stays fp16 (graphics.rs) and keeps light above 1: the tonemapping pass
//!   leaves out the SDR clip (lights.rs `GAME_HDR`) and the grade carries that light
//!   (imagespace.wesl).
//! - Before the HUD, [`hdr_expand`] (`hdr_expand.wesl`) spreads the top of the SDR range by
//!   the expansion row and lifts the light past SDR white towards the screen's peak (Auto:
//!   the window's screen as Windows reports it), never past it. Only the window's camera:
//!   the water's reflection camera renders into a texture the frame samples, which must stay
//!   in the picture's range.
//! - The final blit to the window (vendored bevy_core_pipeline `blit`) decodes the SDR range
//!   with gamma 2.2 and scales so the HUD's white shows at the HUD brightness.

use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::prelude::*;
use bevy::render::render_resource::binding_types::{sampler, texture_2d};
use bevy::render::render_resource::{
    AddressMode, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedRenderPipelineId, ColorTargetState, ColorWrites, FilterMode,
    FragmentState, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler, SamplerBindingType,
    SamplerDescriptor, ShaderStages, TextureFormat, TextureSampleType,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::view::ViewTarget;
use bevy::render::{RenderApp, RenderStartup};

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};

/// The row's choice (the INI keeps it; it takes effect at once, and at the start).
static WANTED: AtomicBool = AtomicBool::new(false);
/// The first frame has run: the window's surface is made, so a choice now is a live switch.
static STARTED: AtomicBool = AtomicBool::new(false);
/// Peak brightness in nits (the row's slider); 0 takes the screen's own (`AUTO_PEAK`).
static PEAK_NITS: AtomicU32 = AtomicU32::new(0);
/// The HDR screen's peak as Windows reports it, read at start; 0 when none reported.
static AUTO_PEAK: AtomicU32 = AtomicU32::new(0);
/// The HDR row's choice that opens the calibration.
const CALIBRATE: &str = "Calibrate...";
/// The peak used when neither the calibration nor Windows gives one.
const FALLBACK_PEAK: u32 = 600;
/// How far bright light keeps the SDR picture's hue, x1000 (the row's slider).
static HUE_MILLI: AtomicU32 = AtomicU32::new(1000);
/// How far the top of the SDR range (above the knee) is spread into the HDR range, x1000
/// (the row's slider): at 1000 the SDR picture's white reaches 60 % of the way from paper
/// white to the peak, leaving the rest to light sources.
static EXPAND_MILLI: AtomicU32 = AtomicU32::new(500);
/// The gain on light past the SDR picture's white (sun, fire, neon, lamps), x10.
static SOURCE_GAIN_TENTHS: AtomicU32 = AtomicU32::new(60);
/// Where the spread of the SDR range starts (brightest channel).
const EXPAND_KNEE: f32 = 0.6;
/// The brightness heatmap in place of the picture (the row's switch).
static HEATMAP: AtomicBool = AtomicBool::new(false);

/// Whether this run asked for HDR output (the row at start, or `NVF_HDR=1`).
pub fn on() -> bool {
    bevy::render::view::window::NVF_HDR_SURFACE.load(Relaxed) || std::env::var("NVF_HDR").is_ok_and(|v| v.trim() == "1")
}

/// The tests that set or read `on` (one flag for the process) run one after another.
#[cfg(test)]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn set_wanted(on: bool) {
    WANTED.store(on, Relaxed);
    // Live: the window's surface is reconfigured on the next frame (vendored bevy_render
    // `view/window/mod.rs`), and `announce` says what it became.
    bevy::render::view::window::NVF_HDR_SURFACE.store(on, Relaxed);
}

/// The corner message that says what the screen shows, each time it changes after the start:
/// HDR with its peak and paper white, SDR, or HDR asked for on a screen that offers none.
fn announce(mut last: Local<Option<(bool, bool)>>, messages: Option<ResMut<nvf_play::corner_messages::CornerMessages>>) {
    let now = (on(), bevy::render::view::window::NVF_HDR_SURFACE_ACTIVE.load(Relaxed));
    if !bevy::render::view::window::NVF_HDR_SURFACE_DECIDED.load(Relaxed) || *last == Some(now) {
        return;
    }
    let first = last.is_none();
    *last = Some(now);
    let text = match now {
        (_, true) => format!("HDR on: scRGB, peak {} nits, paper white {} nits", peak_nits(), paper_nits()),
        (true, false) => "HDR asked for, but this screen offers no HDR (Windows HDR off?)".to_string(),
        (false, false) => "HDR off: SDR output".to_string(),
    };
    info!("hdr: {text}");
    if let Some(mut m) = messages {
        // At the start only when HDR was asked for; every change after it.
        if !first || now.0 {
            m.push(nvf_play::script_events::Note { text, icon: None, seconds: Some(4.0) });
        }
    }
}

/// The calibration's ranges, nits: brightness (paper white) and peak [design].
pub(crate) const PAPER_MIN: u32 = 80;
pub(crate) const PAPER_MAX: u32 = 400;
pub(crate) const PEAK_MIN: u32 = 400;
pub(crate) const PEAK_MAX: u32 = 4000;

/// Paper white in nits: how bright the picture's white and, with it, the HUD look.
pub(crate) fn paper_nits() -> u32 {
    bevy::core_pipeline::upscaling::NVF_HDR_PAPER_WHITE_NITS.load(Relaxed)
}
/// Brightness (paper white), the HUD's white with it: one value, as most games' HDR
/// calibration has [design: PR #3348 review, fewer rows].
pub(crate) fn set_paper_nits(nits: u32) {
    let nits = nits.clamp(PAPER_MIN, PAPER_MAX);
    bevy::core_pipeline::upscaling::NVF_HDR_PAPER_WHITE_NITS.store(nits, Relaxed);
    bevy::core_pipeline::upscaling::NVF_HDR_HUD_NITS.store(nits, Relaxed);
}
/// The peak in nits now: the calibration's, else the screen's, else [`FALLBACK_PEAK`].
pub(crate) fn peak_nits() -> u32 {
    match PEAK_NITS.load(Relaxed) {
        0 => match AUTO_PEAK.load(Relaxed) {
            0 => FALLBACK_PEAK,
            n => n,
        },
        n => n,
    }
}
/// The calibration's peak as set: 0 for the screen's own.
pub(crate) fn peak_setting() -> u32 {
    PEAK_NITS.load(Relaxed)
}
/// The calibration's peak in nits; 0 takes the screen's own again.
pub(crate) fn set_peak_nits(nits: u32) {
    PEAK_NITS.store(if nits == 0 { 0 } else { nits.clamp(PEAK_MIN, PEAK_MAX) }, Relaxed);
}

/// The HDR row as the INI keeps it: `On` or `Off`, then the calibration (`peak=` 0 for the
/// screen's own, `paper=`), as `On;peak=1000;paper=200`.
fn hdr_saved() -> String {
    format!("{};peak={};paper={}", if WANTED.load(Relaxed) { "On" } else { "Off" }, PEAK_NITS.load(Relaxed), paper_nits())
}
/// The HDR row from its INI text ([`hdr_saved`]; a bare `1`, `true` or `on` as the old switch
/// kept it); false when the first part is no on or off.
fn hdr_load(text: &str) -> bool {
    let mut parts = text.split(';').map(str::trim);
    let on = match parts.next().map(str::to_ascii_lowercase).as_deref() {
        Some("on" | "1" | "true") => true,
        Some("off" | "0" | "false") => false,
        _ => return false,
    };
    set_wanted(on);
    for part in parts {
        match part.split_once('=').map(|(k, v)| (k.trim(), v.trim().parse::<u32>())) {
            Some(("peak", Ok(n))) => set_peak_nits(n),
            Some(("paper", Ok(n))) => set_paper_nits(n),
            _ => {}
        }
    }
    true
}

/// The brightest HDR screen's peak and black level in nits as Windows reports them (from
/// the screen's EDID, or the Windows HDR Calibration app's profile) [documented:
/// IDXGIOutput6::GetDesc1, DXGI_OUTPUT_DESC1]. Only outputs in HDR mode (PQ, BT.2020)
/// count. `device`: the screen the window is on, by its device name ("\\.\DISPLAY1"); its
/// figures if it is in HDR mode, none if not. Without it, or when no output has that
/// name, the brightest HDR output's.
#[cfg(windows)]
fn display_peak(device: Option<&str>) -> Option<(f32, f32)> {
    use windows::core::Interface;
    use windows::Win32::Graphics::Dxgi::Common::DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, IDXGIOutput6};
    // SAFETY: creates a COM object that the returned wrapper owns and releases.
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ok()?;
    let mut best: Option<(f32, f32)> = None;
    let mut a = 0;
    // SAFETY: `factory` is a live DXGI factory; the call only reads the index.
    while let Ok(adapter) = unsafe { factory.EnumAdapters1(a) } {
        a += 1;
        let mut o = 0;
        // SAFETY: `adapter` is a live DXGI adapter; the call only reads the index.
        while let Ok(output) = unsafe { adapter.EnumOutputs(o) } {
            o += 1;
            let Ok(output6) = output.cast::<IDXGIOutput6>() else { continue };
            // SAFETY: `output6` is a live DXGI output; the description is returned by value.
            let Ok(desc) = (unsafe { output6.GetDesc1() }) else { continue };
            let hdr = desc.ColorSpace == DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020;
            let name = String::from_utf16_lossy(&desc.DeviceName);
            if device.is_some_and(|d| d == name.trim_end_matches('\0')) {
                return hdr.then_some((desc.MaxLuminance, desc.MinLuminance));
            }
            if hdr && desc.MaxLuminance > best.map_or(0.0, |b| b.0) {
                best = Some((desc.MaxLuminance, desc.MinLuminance));
            }
        }
    }
    best
}
#[cfg(not(windows))]
fn display_peak(_device: Option<&str>) -> Option<(f32, f32)> {
    None
}

/// Auto peak follows the screen the window is on: read again from Windows whenever the
/// window's monitor changes. winit names a monitor by the device name DXGI reports
/// ("\\.\DISPLAY1") [hypothesis: both come from GetMonitorInfo's szDevice].
fn follow_monitor(windows: Query<&bevy::window::OnMonitor, With<bevy::window::PrimaryWindow>>, monitors: Query<&bevy::window::Monitor>, mut last: Local<Option<String>>) {
    if !on() {
        return;
    }
    let Some(name) = windows.single().ok().and_then(|m| monitors.get(m.0).ok()).and_then(|m| m.name.clone()) else { return };
    if last.as_deref() == Some(name.as_str()) {
        return;
    }
    match display_peak(Some(&name)) {
        Some((peak, black)) if peak >= 100.0 => {
            AUTO_PEAK.store(peak.round() as u32, Relaxed);
            info!("hdr: the window's screen {name} reports a peak of {peak:.0} nits, black {black:.3} nits");
        }
        _ => {
            AUTO_PEAK.store(0, Relaxed);
            info!("hdr: the window's screen {name} reports no HDR peak; Auto uses {FALLBACK_PEAK} nits");
        }
    }
    *last = Some(name);
}

/// Logs the output once the window's surface is made (or 300 frames on, without one).
fn mark_started(mut frames: Local<u32>) {
    if *frames == 0 {
        STARTED.store(true, Relaxed);
    }
    if *frames == u32::MAX {
        return;
    }
    *frames += 1;
    if bevy::render::view::window::NVF_HDR_SURFACE_ACTIVE.load(Relaxed) || !on() || *frames >= 300 {
        *frames = u32::MAX;
        info!("hdr: output {}", if bevy::render::view::window::NVF_HDR_SURFACE_ACTIVE.load(Relaxed) { "on (scRGB)" } else if on() { "asked for, but the screen's surface has no fp16 scRGB" } else { "off" });
    }
}

pub struct HdrPlugin;

impl Plugin for HdrPlugin {
    fn build(&self, app: &mut App) {
        use nvf_play::settings::{add_choice_list, set_choice_action, ChoiceList, Setting};
        if let Ok(v) = std::env::var("NVF_HDR_PAPER_WHITE") {
            if let Ok(n) = v.trim().parse::<u32>() {
                bevy::core_pipeline::upscaling::NVF_HDR_PAPER_WHITE_NITS.store(n.clamp(80, 1000), Relaxed);
            }
        }
        if let Some(n) = std::env::var("NVF_HDR_PEAK").ok().and_then(|v| v.trim().parse::<u32>().ok()) {
            PEAK_NITS.store(n.clamp(80, 4000), Relaxed);
        }
        if std::env::var("NVF_HDR_HEATMAP").is_ok_and(|v| v.trim() == "1") {
            HEATMAP.store(true, Relaxed);
        }
        if on() {
            match display_peak(None) {
                Some((peak, black)) if peak >= 100.0 => {
                    AUTO_PEAK.store(peak.round() as u32, Relaxed);
                    info!("hdr: the screen reports a peak of {peak:.0} nits, black {black:.3} nits");
                }
                _ => info!("hdr: no HDR screen reports its peak; Auto uses {FALLBACK_PEAK} nits"),
            }
        }
        // One row [design: PR #3348 review, "a lot of settings"]: Off, On, and Calibrate...,
        // which opens the two calibration pictures (hdr_calibrate.rs) and keeps nothing itself.
        // Gamma 2.2, highlight colour, expansion and light source gain keep their defaults;
        // the heatmap is `NVF_HDR_HEATMAP=1`.
        add_choice_list(
            app,
            Setting {
                key: "hdr-output",
                group: "Display",
                label: "HDR output",
                note: "On: the game is drawn for an HDR screen (Windows HDR on). Takes effect at once. Calibrate... sets the screen's peak and the brightness.",
                switches: "",
                get: |_| false,
                set: |_, _| {},
                slider: None,
            },
            ChoiceList {
                names: |_| ["Off", "On", CALIBRATE].map(String::from).to_vec(),
                chosen: |_| if WANTED.load(Relaxed) { "On" } else { "Off" }.to_string(),
                pick: |w, name| match name {
                    "On" | "Off" => {
                        set_wanted(name == "On");
                        true
                    }
                    CALIBRATE => {
                        w.insert_resource(crate::hdr_calibrate::Asked);
                        true
                    }
                    _ => false,
                },
                save: |_| hdr_saved(),
                load: |_, text| hdr_load(text),
                ask_keep: false,
            },
        );
        set_choice_action(app, "hdr-output", |name| name == CALIBRATE);
        if std::env::var("NVF_HDR").is_ok_and(|v| v.trim() == "1") {
            bevy::render::view::window::NVF_HDR_SURFACE.store(true, Relaxed);
        }
        app.add_systems(Update, (mark_started, follow_monitor, announce));
        crate::hdr_calibrate::add(app);
        bevy::asset::embedded_asset!(app, "hdr_expand.wesl");
        let Some(render) = app.get_sub_app_mut(RenderApp) else { return };
        render
            .add_systems(RenderStartup, init_pipeline)
            // After save-thumbnail capture (so SDR thumbnails read the pre-expansion [0, 1]
            // tonemapped frame deterministically, #3808) and before frame generation's
            // HUD-less copy (a copy taken before the expansion differed from the frame
            // everywhere, so DLSS-G's UI alpha came out 1 on every pixel and no frame was
            // generated over the picture, PR #3348 review).
            .add_systems(
                Core3d,
                hdr_expand
                    .after(Core3dSystems::PostProcess)
                    .after(nvf_play::save::SaveShotSystems)
                    .before(crate::frame_gen::capture_hudless)
                    .before(bevy::ui_render::render_pass::ui_pass),
            );
    }
}

#[derive(Resource)]
pub(crate) struct HdrPipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    id: CachedRenderPipelineId,
    params: bevy::render::render_resource::Buffer,
}

fn init_pipeline(mut commands: Commands, device: Res<RenderDevice>, assets: Res<AssetServer>, fullscreen: Res<FullscreenShader>, cache: Res<PipelineCache>) {
    let layout = BindGroupLayoutDescriptor::new(
        "nvf_hdr_layout",
        &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (texture_2d(TextureSampleType::Float { filterable: true }), sampler(SamplerBindingType::Filtering), bevy::render::render_resource::binding_types::uniform_buffer_sized(false, None))),
    );
    let sampler = device.create_sampler(&SamplerDescriptor {
        mag_filter: FilterMode::Nearest,
        min_filter: FilterMode::Nearest,
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        ..default()
    });
    let id = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("nvf_hdr_expand".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: assets.load("embedded://nvforever/hdr_expand.wesl"),
            targets: vec![Some(ColorTargetState { format: TextureFormat::Rgba16Float, blend: None, write_mask: ColorWrites::ALL })],
            ..default()
        }),
        ..default()
    });
    let params = device.create_buffer(&bevy::render::render_resource::BufferDescriptor {
        label: Some("nvf_hdr_params"),
        size: 32,
        usage: bevy::render::render_resource::BufferUsages::UNIFORM | bevy::render::render_resource::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    commands.insert_resource(HdrPipeline { layout, sampler, id, params });
}

/// Stretch the highlights of an fp16 main view before the HUD is drawn.
pub(crate) fn hdr_expand(view: ViewQuery<(&ViewTarget, &bevy::render::camera::ExtractedCamera)>, pipeline: Option<Res<HdrPipeline>>, cache: Res<PipelineCache>, device: Res<RenderDevice>, queue: Res<bevy::render::renderer::RenderQueue>, mut ctx: RenderContext) {
    let (target, camera) = view.into_inner();
    let Some(p) = pipeline else { return };
    if !matches!(camera.target, Some(bevy::camera::NormalizedRenderTarget::Window(_))) {
        return;
    }
    if !bevy::render::view::window::NVF_HDR_SURFACE_ACTIVE.load(Relaxed) || target.main_texture_format() != TextureFormat::Rgba16Float {
        return;
    }
    // Peak and paper white from the rows now (live calibration).
    // The pass works in units of the HUD's white (the blit's scale): the picture's white is
    // `scene` of them and its peak `ratio`.
    let hud = bevy::core_pipeline::upscaling::nvf_hdr_paper_white().max(80) as f32;
    let scene = paper_nits().clamp(80, 1000) as f32 / hud;
    let ratio = (peak_nits() as f32 / hud).max(scene);
    let params = [
        ratio,
        HUE_MILLI.load(Relaxed) as f32 / 1000.0,
        scene,
        if HEATMAP.load(Relaxed) { 1.0 } else { 0.0 },
        SOURCE_GAIN_TENTHS.load(Relaxed) as f32 / 10.0,
        EXPAND_MILLI.load(Relaxed) as f32 / 1000.0,
        EXPAND_KNEE,
        0.0,
    ];
    queue.write_buffer(&p.params, 0, bytemuck_bytes(&params));
    let Some(rp) = cache.get_render_pipeline(p.id) else { return };
    let layout = cache.get_bind_group_layout(&p.layout);
    let post = target.post_process_write();
    let bind_group = device.create_bind_group("nvf_hdr_expand", &layout, &BindGroupEntries::sequential((post.source, &p.sampler, p.params.as_entire_binding())));
    let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some("nvf_hdr_expand"),
        color_attachments: &[Some(RenderPassColorAttachment { view: post.destination, depth_slice: None, resolve_target: None, ops: Operations::default() })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(rp);
    pass.set_bind_group(0, &bind_group, &[]);
    pass.draw(0..3, 0..1);
}

/// The bytes of four floats, little-endian as the GPU reads them.
fn bytemuck_bytes(v: &[f32; 8]) -> &[u8] {
    // SAFETY: f32 has no padding; the slice covers the array exactly.
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), 32) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CPU mirror of hdr_expand.wesl's highlight expansion and light-source shoulder (no hue
    /// restore), in units of the picture's white: `peak` over paper white, `amount` the
    /// expansion (0 to 1), `gain` the source gain.
    fn expand(rgb: [f32; 3], peak: f32, amount: f32, gain: f32) -> [f32; 3] {
        let room = peak - 1.0;
        let sdr = rgb.map(|c| c.clamp(0.0, 1.0));
        let m = sdr[0].max(sdr[1]).max(sdr[2]);
        let white = 1.0 + amount * 0.6 * room.max(0.0);
        let mut out = sdr;
        if m > EXPAND_KNEE && white > 1.0 {
            let t = (m - EXPAND_KNEE) / (1.0 - EXPAND_KNEE);
            let a = (white - EXPAND_KNEE) / (1.0 - EXPAND_KNEE) - 1.0;
            let out_m = EXPAND_KNEE + (m - EXPAND_KNEE) * (1.0 + a * t * t);
            out = sdr.map(|c| c * out_m / m);
        }
        let e = [0, 1, 2].map(|i| (rgb[i].max(0.0) - sdr[i]) * gain);
        let em = e[0].max(e[1]).max(e[2]);
        let room2 = peak - white;
        if room2 > 0.0 && em > 0.0 {
            for i in 0..3 {
                out[i] += e[i] * room2 / (em + room2);
            }
        }
        out
    }

    #[test]
    fn sdr_range_unchanged_without_expansion() {
        for r in 0..=20 {
            for g in 0..=20 {
                let c = [r as f32 / 20.0, g as f32 / 20.0, 0.3];
                assert_eq!(expand(c, 3.0, 0.0, 6.0), c);
            }
        }
    }

    #[test]
    fn below_the_knee_unchanged_with_expansion() {
        let c = [EXPAND_KNEE, 0.4, 0.1];
        assert_eq!(expand(c, 3.0, 1.0, 6.0), c);
    }

    #[test]
    fn never_past_the_peak() {
        for peak in [1.5_f32, 3.0, 10.0] {
            for v in [1.0_f32, 2.0, 18.0, 1.0e6] {
                let out = expand([v, v * 0.5, 0.1], peak, 1.0, 16.0);
                assert!(out.iter().all(|&c| c <= peak + 1e-4), "{out:?} past {peak}");
            }
        }
    }

    #[test]
    fn brighter_light_stays_brighter() {
        let mut last = -1.0;
        for i in 0..=400 {
            let v = i as f32 * 0.125;
            let out = expand([v, v, v], 3.08, 0.5, 6.0)[0];
            assert!(out >= last, "{v}: {out} below {last}");
            last = out;
        }
    }

    #[test]
    fn continuous_at_white() {
        let below = expand([1.0, 1.0, 1.0], 3.08, 0.5, 6.0)[0];
        let above = expand([1.0001, 1.0001, 1.0001], 3.08, 0.5, 6.0)[0];
        assert!((above - below).abs() < 0.01);
    }

    #[test]
    fn light_source_keeps_its_colour() {
        // Red 5, green 2.5: both past white by 4 and 1.5, so their light above SDR white
        // keeps that ratio.
        let out = expand([5.0, 2.5, 0.5], 3.0, 0.0, 6.0);
        let ratio = (out[0] - 1.0) / (out[1] - 1.0);
        assert!((ratio - 4.0 / 1.5).abs() < 1e-3, "{ratio}");
    }

    #[test]
    fn peak_auto_then_the_calibration() {
        set_peak_nits(0);
        AUTO_PEAK.store(0, Relaxed);
        assert_eq!(peak_nits(), FALLBACK_PEAK);
        AUTO_PEAK.store(616, Relaxed);
        assert_eq!(peak_nits(), 616);
        set_peak_nits(100);
        assert_eq!(peak_nits(), PEAK_MIN);
        set_peak_nits(9000);
        assert_eq!(peak_nits(), PEAK_MAX);
        set_peak_nits(0);
    }

    #[test]
    fn brightness_moves_paper_white_and_the_hud_together() {
        set_paper_nits(250);
        assert_eq!((paper_nits(), bevy::core_pipeline::upscaling::NVF_HDR_HUD_NITS.load(Relaxed)), (250, 250));
        set_paper_nits(9000);
        assert_eq!(paper_nits(), PAPER_MAX);
    }

    #[test]
    fn the_row_keeps_on_and_the_calibration() {
        let _lock = test_lock();
        assert!(hdr_load("On;peak=1200;paper=240"));
        assert_eq!((WANTED.load(Relaxed), PEAK_NITS.load(Relaxed), paper_nits()), (true, 1200, 240));
        assert_eq!(hdr_saved(), "On;peak=1200;paper=240");
        assert!(hdr_load("0"), "the old switch's text");
        assert!(!WANTED.load(Relaxed));
        assert!(!hdr_load("bright"));
        set_peak_nits(0);
        set_paper_nits(200);
        bevy::render::view::window::NVF_HDR_SURFACE.store(false, Relaxed);
    }
}
