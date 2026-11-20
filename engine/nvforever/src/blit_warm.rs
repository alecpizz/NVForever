//! The upscaling pass's pipelines compiled at start-up, so a camera that switches on later
//! does not stall the frame.
//!
//! Bevy's upscaling pass (`bevy_core_pipeline` 0.19.1 `upscaling/mod.rs`,
//! `prepare_view_upscaling_pipelines`) blocks the frame until a view's blit pipeline is
//! compiled, the first time a view with a new key (output format, blend, source space)
//! is drawn ("ensure the pipeline is loaded before continuing the frame") [documented].
//! The compile is a task on the async compute pool, queued behind every other pipeline
//! being compiled, so when the camera switches on as the player comes into a new place,
//! with the place's own shaders compiling, the frame waits for all of them: 1.3 s on the
//! way into Primm, where the water reflection camera (reflection.rs) switches on, a
//! `profile hitch` line with `upscaling::prepare_view_upscaling_pipelines` 1305 ms in it
//! [observed: tools/stress tour Goodsprings–Primm, 2026-10-27; docs/design/
//! streaming-hitches.md §3 had the frame as "not looked into yet"].
//!
//! Every key the game's cameras can ask for is specialized once on the render app's first
//! frames ([`KEYS`]), without waiting: the compiles finish under the loading screen and
//! a later camera finds its pipeline ready. The formats are those of the window (Bevy
//! names a window's Bgra8UnormSrgb Rgba8UnormSrgb) and of the image targets
//! (Bgra8UnormSrgb for the Pip-Boy, terminal, scope and table views, Rgba16Float for the
//! reflection, R32Float for the ripples); a camera after the first on a target blends
//! (alpha blending); no camera sets a compositing space [engineering choice: a dozen small
//! full-screen pipelines].
//!
//! Also the screenshot's copy back into the window (`ScreenshotToScreenPipeline`, keyed by the
//! window's view format: sRGB, or `Rgba16Float` with HDR output, hdr.rs): specialized only
//! when the first screenshot is taken and not waited for, so until it was compiled the
//! screenshot's frame showed an empty (black) window image [observed 2026-11-19: HDR output's
//! `NVF_HDR_PROBE`, every frame of a `--screenshot` run, the screenshot's frame black].

use bevy::core_pipeline::blit::{BlitPipeline, BlitPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{BlendState, PipelineCache, SpecializedRenderPipelines, TextureFormat};
use bevy::render::view::screenshot::ScreenshotToScreenPipeline;
use bevy::render::{Render, RenderApp, RenderSystems};

/// The window's view formats, for the screenshot's copy back into it.
const WINDOW_FORMATS: [TextureFormat; 3] = [TextureFormat::Bgra8UnormSrgb, TextureFormat::Rgba8UnormSrgb, TextureFormat::Rgba16Float];

/// The output formats of the game's views.
const FORMATS: [TextureFormat; 6] = [
    TextureFormat::Rgba8UnormSrgb,
    TextureFormat::Bgra8UnormSrgb,
    TextureFormat::Rgba8Unorm,
    TextureFormat::Bgra8Unorm,
    TextureFormat::Rgba16Float,
    TextureFormat::R32Float,
];

/// Every upscaling key warmed: each format, written over (the first camera on a target)
/// and blended (the ones after it).
pub fn keys() -> Vec<BlitPipelineKey> {
    FORMATS
        .iter()
        .flat_map(|&target_format| {
            [None, Some(BlendState::ALPHA_BLENDING)].map(|blend_state| BlitPipelineKey { target_format, blend_state, samples: 1, source_space: None })
        })
        .collect()
}

pub struct BlitWarmPlugin;

impl Plugin for BlitWarmPlugin {
    fn build(&self, app: &mut App) {
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.add_systems(Render, (warm, warm_screenshot).in_set(RenderSystems::Prepare));
        }
    }
}

/// Specializes every key once, as soon as the blit pipeline exists (it is made in the
/// render app's start-up).
fn warm(
    mut done: Local<bool>,
    cache: Res<PipelineCache>,
    blit: Option<Res<BlitPipeline>>,
    pipelines: Option<ResMut<SpecializedRenderPipelines<BlitPipeline>>>,
) {
    if *done {
        return;
    }
    let (Some(blit), Some(mut pipelines)) = (blit, pipelines) else { return };
    let keys = keys();
    for key in &keys {
        pipelines.specialize(&cache, &blit, *key);
    }
    *done = true;
    info!("blit warm: {} upscaling pipelines queued for compiling at start-up", keys.len());
}

/// The screenshot's copy into the window, for each window format, once.
fn warm_screenshot(
    mut done: Local<bool>,
    cache: Res<PipelineCache>,
    pipeline: Option<Res<ScreenshotToScreenPipeline>>,
    pipelines: Option<ResMut<SpecializedRenderPipelines<ScreenshotToScreenPipeline>>>,
) {
    if *done {
        return;
    }
    let (Some(pipeline), Some(mut pipelines)) = (pipeline, pipelines) else { return };
    for format in WINDOW_FORMATS {
        pipelines.specialize(&cache, &pipeline, format);
    }
    *done = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_format_written_and_blended() {
        let k = keys();
        assert_eq!(k.len(), FORMATS.len() * 2);
        let reflection = BlitPipelineKey { target_format: TextureFormat::Rgba16Float, blend_state: None, samples: 1, source_space: None };
        assert!(k.contains(&reflection), "the water reflection camera's key");
    }
}
