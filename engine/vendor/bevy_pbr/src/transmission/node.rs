use crate::{ScreenSpaceTransmission, Transmissive3d, ViewTransmissionTexture};

use bevy_camera::{MainPassResolutionOverride, Viewport};
use bevy_core_pipeline::{core_3d::Transparent3d, oit::OrderIndependentTransparencySettings};
use bevy_ecs::prelude::*;
use bevy_image::ToExtents;
use bevy_render::{
    camera::ExtractedCamera,
    diagnostic::RecordDiagnostics,
    render_phase::ViewSortedRenderPhases,
    render_resource::{RenderPassDescriptor, StoreOp},
    renderer::{RenderContext, ViewQuery},
    view::{ExtractedView, ViewDepthStencilTexture, ViewTarget},
};
use core::ops::Range;
use tracing::error;
#[cfg(feature = "trace")]
use tracing::info_span;

/// NVForever one resolve (2026-11-10, engine/vendor/README.md). With multisampling the
/// opaque, transmissive and transparent passes each resolve the whole main target when
/// they end, and the transmissive pass copies the resolved picture besides. Only two of
/// those are read: the picture behind the transmissive shapes, and the last one of the
/// frame. So when the view's transparent pass is going to run (it has items), the passes
/// before it keep their samples unresolved ([`ViewTarget::set_resolve_deferred`]), the
/// transmissive pass resolves straight into the transmission texture instead of copying
/// ([`main_transmissive_pass_3d`]), and the transparent pass resolves once, as upstream.
/// A view with a main-pass resolution override (the game's FSR passes read the resolved
/// picture between these passes), a view with order-independent transparency (upstream's
/// transparent pass can return early, without resolving, while its resolve pipeline is not
/// ready) and a view without multisampling keep upstream's path.
pub fn begin_deferred_resolve(
    view: ViewQuery<(
        &ExtractedView,
        &ViewTarget,
        Has<MainPassResolutionOverride>,
        Has<OrderIndependentTransparencySettings>,
    )>,
    transparent_phases: Res<ViewSortedRenderPhases<Transparent3d>>,
) {
    let (extracted_view, target, resolution_override, has_oit) = view.into_inner();
    let transparent_follows = transparent_phases
        .get(&extracted_view.retained_view_entity)
        .is_some_and(|phase| !phase.items.is_empty());
    target.set_resolve_deferred(transparent_follows && !resolution_override && !has_oit);
}

/// NVForever one resolve: the passes from here on resolve again ([`begin_deferred_resolve`]).
pub fn end_deferred_resolve(view: ViewQuery<&ViewTarget>) {
    view.into_inner().set_resolve_deferred(false);
}

pub fn main_transmissive_pass_3d(
    world: &World,
    view: ViewQuery<(
        &ExtractedCamera,
        &ExtractedView,
        &ScreenSpaceTransmission,
        &ViewTarget,
        Option<&ViewTransmissionTexture>,
        &ViewDepthStencilTexture,
        Option<&MainPassResolutionOverride>,
    )>,
    transmissive_phases: Res<ViewSortedRenderPhases<Transmissive3d>>,
    mut ctx: RenderContext,
) {
    let view_entity = view.entity();

    let (
        camera,
        extracted_view,
        transmission_settings,
        target,
        transmission,
        depth,
        resolution_override,
    ) = view.into_inner();

    let Some(transmissive_phase) = transmissive_phases.get(&extracted_view.retained_view_entity)
    else {
        return;
    };

    let Some(physical_target_size) = camera.physical_target_size else {
        return;
    };

    #[cfg(feature = "trace")]
    let _main_transmissive_pass_3d_span = info_span!("main_transmissive_pass_3d").entered();

    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();

    let render_pass_descriptor = RenderPassDescriptor {
        label: Some("main_transmissive_pass_3d"),
        color_attachments: &[Some(target.get_color_attachment())],
        depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    };

    if !transmissive_phase.items.is_empty() {
        let steps = transmission_settings.steps;
        if steps > 0 {
            let transmission =
                transmission.expect("`ViewTransmissionTexture` should exist at this point");

            // `transmissive_phase.items` are depth sorted, so we split them into N = `steps`
            // ranges, rendering them back-to-front in multiple steps, allowing multiple levels of transparency.
            for range in split_range(0..transmissive_phase.items.len(), steps) {
                if target.resolve_deferred() {
                    // NVForever one resolve: the main texture's samples have not been
                    // resolved (`begin_deferred_resolve`); a pass that draws nothing
                    // resolves them into the transmission texture, which takes the
                    // place of upstream's resolve and copy.
                    let mut attachment = target.get_color_attachment();
                    attachment.resolve_target = Some(&transmission.view);
                    drop(ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
                        label: Some("resolve_to_transmission_texture"),
                        color_attachments: &[Some(attachment)],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    }));
                } else {
                    // Copy the main texture to the transmission texture
                    ctx.command_encoder().copy_texture_to_texture(
                        target.main_texture().as_image_copy(),
                        transmission.texture.as_image_copy(),
                        physical_target_size.to_extents(),
                    );
                }

                let mut render_pass = ctx.begin_tracked_render_pass(render_pass_descriptor.clone());
                let pass_span =
                    diagnostics.pass_span(&mut render_pass, "main_transmissive_pass_3d");

                // NVForever FSR integration (2026-11-05): the stepped transmission
                // path must use the same render viewport as the opaque pass.
                if let Some(viewport) =
                    Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution_override)
                {
                    render_pass.set_camera_viewport(&viewport);
                }

                if let Err(err) =
                    transmissive_phase.render_range(&mut render_pass, world, view_entity, range)
                {
                    error!("Error encountered while rendering the transmissive phase {err:?}");
                }

                pass_span.end(&mut render_pass);
            }
        } else {
            let mut render_pass = ctx.begin_tracked_render_pass(render_pass_descriptor);
            let pass_span = diagnostics.pass_span(&mut render_pass, "main_transmissive_pass_3d");

            if let Some(viewport) =
                Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution_override)
            {
                render_pass.set_camera_viewport(&viewport);
            }

            if let Err(err) = transmissive_phase.render(&mut render_pass, world, view_entity) {
                error!("Error encountered while rendering the transmissive phase {err:?}");
            }

            pass_span.end(&mut render_pass);
        }
    }
}

/// Splits a [`Range`] into at most `max_num_splits` sub-ranges without overlaps
///
/// Properly takes into account remainders of inexact divisions (by adding extra
/// elements to the initial sub-ranges as needed)
fn split_range(range: Range<usize>, max_num_splits: usize) -> impl Iterator<Item = Range<usize>> {
    let len = range.end - range.start;
    assert!(len > 0, "to be split, a range must not be empty");
    assert!(max_num_splits > 0, "max_num_splits must be at least 1");
    let num_splits = max_num_splits.min(len);
    let step = len / num_splits;
    let mut rem = len % num_splits;
    let mut start = range.start;

    (0..num_splits).map(move |_| {
        let extra = if rem > 0 {
            rem -= 1;
            1
        } else {
            0
        };
        let end = (start + step + extra).min(range.end);
        let result = start..end;
        start = end;
        result
    })
}
