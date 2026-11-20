//! HDR output (prototype, 2026-11-19; `--hdr`). The window's surface becomes scRGB
//! (`Rgba16Float`, extended sRGB linear: Rec.709 primaries, 1.0 = 80 nits) when the display
//! is in HDR mode (vendored bevy_render, `HDR_OUTPUT_REQUEST`), and the picture is made for
//! it in two places [design]:
//!
//! - The highlights (imagespace.wesl, the grade's last step): on the main view only, the
//!   graded picture's highlights rise above 1 (1 = paper white) towards the display's peak:
//!   above the knee (linear luminance) by the square of the way from the knee to white,
//!   SDR white landing on the peak, one gain for all three channels (the hue kept). Every
//!   other view (the terminals, the minigames), the Pip-Boy's screen and the UI stay SDR.
//!   The main view keeps the HDR (fp16) target so those values last until the encode.
//! - The encode (`hdr_encode.wesl`, [`hdr_encode`]): once everything is drawn, the frame is
//!   copied and written back into the surface with the SDR range read as a gamma 2.2 screen
//!   shows it (OLED blacks stay black instead of the grey a plain sRGB decode lifts them to),
//!   the black floor, paper white and the peak.
//!
//! Why on the graded picture, not in the tonemapper: the scene's own luminance gives next to
//! nothing to lift, and before the grade the picture is dark; Goodsprings at noon peaks near
//! 0.5 (linear) after Tony McMapface and the grade, so a curve there lifted almost nothing
//! [observed 2026-11-19: `.hdr` screenshots of the main view, before the encode].
//!
//! Tuning while the game runs, Ctrl+Alt with: Page Up / Page Down peak ±50 nits, Home / End
//! paper white ±10 nits, Insert / Delete black floor ±0.001, G gamma 2.2 on/off, H highlight
//! strength 1 / 0.5 / 0, K / J the knee ±0.05; each change is logged as "hdr output: …".
//!
//! Not yet: frame generation (its presenter copies into the surface after the encode, so the
//! picture shows unencoded), screenshots (they are taken before the encode), a settings-page
//! row and the INI, a separate UI brightness, wide gamut, HDR10 (PQ) output.

use bevy::core_pipeline::FullscreenShader;
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_resource::binding_types::{texture_2d, uniform_buffer};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedRenderPipelineId, ColorTargetState, ColorWrites, CommandEncoderDescriptor,
    FragmentState, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, ShaderStages, ShaderType, Texture,
    TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView, UniformBuffer,
};
use bevy::render::renderer::{RenderDevice, RenderGraph, RenderGraphSystems, RenderQueue};
use bevy::render::view::{ExtractedWindow, HDR_OUTPUT_ACTIVE, HDR_OUTPUT_REQUEST};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use std::sync::atomic::Ordering;

/// The surface's format with HDR output.
const SURFACE_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
/// scRGB's 1.0, in nits.
const SCRGB_NITS: f32 = 80.0;

/// The HDR output's settings (`--hdr-*`; Ctrl+Alt keys while running, see the module notes).
#[derive(Resource, ExtractResource, Clone, Copy, Debug, PartialEq)]
#[extract_app(bevy::render::RenderApp)]
pub struct HdrOutput {
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
        Self { peak_nits: 1000.0, paper_white_nits: 200.0, black_floor: 0.0, gamma22: true, highlights: 1.0, knee: 0.25 }
    }
}

impl HdrOutput {
    fn clamped(mut self) -> Self {
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

    fn encode(&self) -> EncodeParams {
        EncodeParams {
            paper_scale: self.paper_white_nits / SCRGB_NITS,
            peak_scale: self.peak_nits / SCRGB_NITS,
            black_floor: self.black_floor,
            gamma22: if self.gamma22 { 1.0 } else { 0.0 },
        }
    }

    pub fn describe(&self) -> String {
        format!(
            "peak {:.0} nits, paper white {:.0} nits, black floor {:+.3}, {}, highlights {:.1} from {:.2}",
            self.peak_nits,
            self.paper_white_nits,
            self.black_floor,
            if self.gamma22 { "gamma 2.2 blacks" } else { "sRGB blacks" },
            self.highlights,
            self.knee
        )
    }
}

/// `hdr_encode.wesl`'s uniform.
#[derive(ShaderType, Clone, Copy, Default)]
struct EncodeParams {
    /// Paper white and the peak in scRGB units (80 nits).
    paper_scale: f32,
    peak_scale: f32,
    black_floor: f32,
    gamma22: f32,
}

/// `Some` asks for HDR output.
pub struct HdrPlugin(pub Option<HdrOutput>);

impl Plugin for HdrPlugin {
    fn build(&self, app: &mut App) {
        let Some(settings) = self.0 else { return };
        let settings = settings.clamped();
        // Read when the window's surface is made, after the plugins are built.
        HDR_OUTPUT_REQUEST.store(true, Ordering::Relaxed);
        bevy::asset::embedded_asset!(app, "hdr_encode.wesl");
        app.insert_resource(settings)
            .add_plugins(ExtractResourcePlugin::<HdrOutput>::default())
            .add_systems(Startup, move || info!("hdr output: asked for; {}", settings.describe()))
            .add_systems(Update, tune)
            // After graphics.rs's `apply`, which takes the HDR view off with the light glow,
            // and imagespace.rs's `update_grade`, which writes the grade whole (both Update).
            .add_systems(PostUpdate, main_camera);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        render_app
            .init_resource::<EncodeUniform>()
            .add_systems(RenderStartup, init_pipeline)
            .add_systems(Render, write_uniform.in_set(RenderSystems::PrepareResources))
            // After every view and the UI are drawn; `render_system` presents after.
            .add_systems(RenderGraph, hdr_encode.in_set(RenderGraphSystems::Finish));
    }
}

/// The main camera, once the surface is HDR: the HDR (fp16) view, so the grade's highlights
/// above 1 last until the encode, and the grade's highlight settings.
#[allow(clippy::type_complexity)]
fn main_camera(
    mut commands: Commands,
    settings: Res<HdrOutput>,
    mut cams: Query<(Entity, Has<bevy::camera::Hdr>, Option<&mut crate::imagespace::ImageSpaceGrade>), With<crate::fly::FlyCam>>,
    mut told: Local<bool>,
) {
    if !HDR_OUTPUT_ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    if !*told {
        *told = true;
        info!("hdr output: on");
    }
    let want = settings.grade();
    for (cam, hdr, grade) in &mut cams {
        if !hdr {
            commands.entity(cam).insert(bevy::camera::Hdr);
        }
        if let Some(mut grade) = grade {
            if grade.hdr != want {
                grade.hdr = want;
            }
        }
    }
}

/// The Ctrl+Alt keys of the module notes.
fn tune(keys: Res<ButtonInput<KeyCode>>, mut hdr: ResMut<HdrOutput>) {
    let held = |a: KeyCode, b: KeyCode| keys.pressed(a) || keys.pressed(b);
    if !held(KeyCode::ControlLeft, KeyCode::ControlRight) || !held(KeyCode::AltLeft, KeyCode::AltRight) {
        return;
    }
    let mut next = *hdr;
    for key in keys.get_just_pressed() {
        match key {
            KeyCode::PageUp => next.peak_nits += 50.0,
            KeyCode::PageDown => next.peak_nits -= 50.0,
            KeyCode::Home => next.paper_white_nits += 10.0,
            KeyCode::End => next.paper_white_nits -= 10.0,
            KeyCode::Insert => next.black_floor += 0.001,
            KeyCode::Delete => next.black_floor -= 0.001,
            KeyCode::KeyG => next.gamma22 = !next.gamma22,
            KeyCode::KeyH => next.highlights = if next.highlights > 0.75 { 0.5 } else if next.highlights > 0.25 { 0.0 } else { 1.0 },
            KeyCode::KeyK => next.knee += 0.05,
            KeyCode::KeyJ => next.knee -= 0.05,
            _ => {}
        }
    }
    let next = next.clamped();
    if next != *hdr {
        *hdr = next;
        info!("hdr output: {}", next.describe());
    }
}

#[derive(Resource, Default)]
struct EncodeUniform(UniformBuffer<EncodeParams>);

fn write_uniform(settings: Option<Res<HdrOutput>>, mut uniform: ResMut<EncodeUniform>, device: Res<RenderDevice>, queue: Res<RenderQueue>) {
    let Some(settings) = settings else { return };
    uniform.0.set(settings.encode());
    uniform.0.write_buffer(&device, &queue);
}

#[derive(Resource)]
struct EncodePipeline {
    layout: BindGroupLayoutDescriptor,
    id: CachedRenderPipelineId,
}

fn init_pipeline(mut commands: Commands, assets: Res<AssetServer>, fullscreen: Res<FullscreenShader>, cache: Res<PipelineCache>) {
    let layout = BindGroupLayoutDescriptor::new(
        "hdr_encode_layout",
        &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (texture_2d(TextureSampleType::Float { filterable: false }), uniform_buffer::<EncodeParams>(false))),
    );
    let id = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("hdr_encode_pipeline".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: assets.load("embedded://nvforever/hdr_encode.wesl"),
            targets: vec![Some(ColorTargetState { format: SURFACE_FORMAT, blend: None, write_mask: ColorWrites::ALL })],
            ..default()
        }),
        ..default()
    });
    commands.insert_resource(EncodePipeline { layout, id });
}

/// The frame copied once drawn, for [`hdr_encode`] to read.
struct Scratch {
    texture: Texture,
    view: TextureView,
    size: (u32, u32),
}

/// The finished frame into scRGB (`hdr_encode.wesl`), before it is presented: copied out of
/// the surface's image and drawn back into it.
fn hdr_encode(
    windows: Query<&ExtractedWindow>,
    pipeline: Option<Res<EncodePipeline>>,
    cache: Res<PipelineCache>,
    uniform: Res<EncodeUniform>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut scratch: Local<Option<Scratch>>,
) {
    let Some(pipeline) = pipeline else { return };
    let (Some(render_pipeline), Some(params)) = (cache.get_render_pipeline(pipeline.id), uniform.0.binding()) else { return };
    for window in &windows {
        let (Some(surface), Some(target)) = (window.swap_chain_texture.as_ref(), window.swap_chain_texture_view.as_ref()) else { continue };
        if surface.texture.format() != SURFACE_FORMAT {
            continue;
        }
        let size = surface.texture.size();
        if scratch.as_ref().is_none_or(|s| s.size != (size.width, size.height)) {
            let texture = device.create_texture(&TextureDescriptor {
                label: Some("hdr_encode_scratch"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: SURFACE_FORMAT,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&default());
            *scratch = Some(Scratch { texture, view, size: (size.width, size.height) });
        }
        let Some(s) = scratch.as_ref() else { continue };
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor { label: Some("hdr_encode") });
        encoder.copy_texture_to_texture(surface.texture.as_image_copy(), s.texture.as_image_copy(), size);
        let layout = cache.get_bind_group_layout(&pipeline.layout);
        let bind_group = device.create_bind_group("hdr_encode", &layout, &BindGroupEntries::sequential((&s.view, params.clone())));
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("nvf_hdr_encode"),
                color_attachments: &[Some(RenderPassColorAttachment { view: target, depth_slice: None, resolve_target: None, ops: Operations::default() })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(render_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([encoder.finish()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_stay_in_range() {
        let wild = HdrOutput { peak_nits: 50.0, paper_white_nits: 900.0, black_floor: 1.0, gamma22: true, highlights: 3.0, knee: 2.0 }.clamped();
        assert_eq!(wild.peak_nits, 100.0);
        assert_eq!(wild.paper_white_nits, 100.0, "paper white never above the peak");
        assert_eq!(wild.black_floor, 0.05);
        assert_eq!(wild.highlights, 1.0);
        assert_eq!(wild.knee, 0.9);
    }

    #[test]
    fn units() {
        let hdr = HdrOutput::default();
        let e = hdr.encode();
        assert_eq!((e.paper_scale, e.peak_scale), (2.5, 12.5), "scRGB: 1.0 is 80 nits");
        assert_eq!(hdr.grade(), Vec4::new(1.0, 5.0, 0.25, 1.0), "the peak in paper whites");
    }
}
