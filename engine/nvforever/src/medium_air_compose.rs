//! The volumetric medium's light in the air (LootJunkieAzza/NVForever#3032, stage 3): the
//! composite. An engineering choice of NVForever's own for the enhanced look, never a fact
//! about the original game and never used to explain a difference from it [design].
//!
//! A fullscreen pass right after the grid pass (medium_air_grid.rs), after
//! `Core3dSystems::MainPass` and before `Core3dSystems::EarlyPostProcess`: it
//! reads the HDR picture (`post_process_write().source`), the depth prepass at its real
//! sample count, the froxel volume (linear, clamped) and the shared uniform, and writes the
//! alternate HDR target, alpha kept. Pixels outside the main pass's viewport
//! (`MainPassResolutionOverride` and the viewport's origin) pass through. Pipelines are keyed
//! by the target's format and the depth's sample type.
//!
//! **What it draws, by view** (`NVF_PLAY_DEBUG_MEDIUM_LIGHT_VIEW` or the tune file's `VIEW`):
//! - `none` (the default): **the light** (the shader's header names the medium_air.rs functions it
//!   mirrors, [`pixel_mirror`] mirrors a pixel whole): one form for every depth sample, in
//!   display values `Y' + corr_t x mix(C', Y', t)` with `corr_t = max(a F_near (Vbar_c x
//!   mix(p_eff, mix(1, p_eff, SKYPHASE), t) - 1) x mix(1, SKY, t), -min(F_near, 1))` and the
//!   blend t (0 near: the additive world rule; 1 for cleared depth: the sky's modulation; a
//!   smoothstep between for far terrain, joined with one over the fog share F_near, so terrain
//!   the fog covers takes the sky's modulation too), a gain held off bright pixels by its share t
//!   (medium_air.rs `hold_bright`), held to 0..=`AIR_HDR_MAX` linear; the mean of the
//!   samples' results under MSAA, one volume lookup shared when they agree, each with its own
//!   F_near and t; a sample with depth holds its change per channel to at least -F_near x C'
//!   (it keeps its surface light at every t);
//! - `delta`: the samples' mean signed scalar corr_t as a picture, mid grey 0.5 plus 4 x the
//!   scalar in display values, opaque (brighter: the light adds; darker: shadowed air takes
//!   away);
//! - `lit`: the lit fraction Vbar = N / M at the pixel as opaque linear grey
//!   0.02 + 0.5 Vbar^2 (a range bloom and tonemapping leave readable), world and sky alike
//!   (sample 0; not gated): the shafts with no look constants in the way;
//! - `rays`: the mean signed scalar corr_t the light actually applies, opaque linear grey
//!   0.2 + 2 x the scalar (gated as the light is);
//! - `visibility` takes the same 0.02 + 0.5 v^2 grey (it was v^2.2, washed out by bloom);
//! - `off`: the grid made and the uniform written every frame, the composite not run and the
//!   target not flipped (the cheapest exact pass-through): with HDR held on by the demand,
//!   the baseline for paired pictures;
//! - the first `visibility` (slice k's own V as grey, 50 % over the picture), `slice`
//!   (the slice index banded, 50 %) and `share` (the fog share at the pixel's distance as
//!   grey, opaque), from depth sample 0.
//!
//! **A known limit: the sky dome counts as a surface** [design]. The dome writes depth (sky.rs: opaque, in the depth prepass), so its samples take the
//! per-channel bound `-F_near x C'` meant for terrain, and shadowed air darkens the dome only
//! as far as the fog colour's budget (at t 1, Y' 0.5, C' 0, F_near 0.5, full shadow: cleared
//! depth reads 0.25, the dome stays 0.5). The dome cannot be told from far terrain by depth:
//! its mesh is the weather's own `atmosphere.nif` (flat, about 500 across and 250 high
//! [hypothesis: read from the model; how was not kept], so its distance depends on the direction and on the data, mods included)
//! scaled to sky.rs `DOME_RADIUS`, and distant LOD terrain reaches beyond sky.rs
//! `SKY_DEPTH_FROM` (volumetric_clouds.rs, which for the same reason draws its clouds with the
//! dome's own mesh under an Equal depth test). Telling them apart needs a coverage mask drawn
//! with the dome's mesh (one bit or an R8 target written where the dome's depth wins, as the
//! cloud proxy's draw finds it), bound to this pass; not built.
//!
//! `none` and `delta` run only while the uniform's `light.w` is 1 (medium_air_light.rs's gate:
//! outdoors, above water, a usable display-value fog, day, a share above 0) and its
//! `fog_colour.w` is 1 (the display-value blend); otherwise nothing runs and the target is
//! not flipped (the reason is logged by medium_air_light.rs). Readiness (both pipelines, the
//! volume written this frame, the depth's sample type and the target's format as the
//! pipeline was made for) is checked before `post_process_write` is called.
//!
//! **Known limits** [design]: a fullscreen pass cannot know which fragments
//! were fogged, so blended surfaces (water, glass, particles), the scope image and emissive
//! overlays get the correction of what the depth shows; under MSAA each sample's result is
//! taken on the resolved colour and the results averaged, an edge approximation under a
//! display-space blend. Every colour the pass computes is held to 0..=`AIR_HDR_MAX` per
//! channel (the debug views too); the pass-through of untouched pixels is not.
//!
//! The volume's lookup (`air_lookup` in the shader, [`lookup_mirror`] here): texel k holds the
//! sums at slice k's far boundary and the camera is a zero boundary, so boundary coordinate
//! c in [0, nz] reads w = (c - 0.5) / nz, scaled by c below 1, the last texel at and beyond
//! the grid's end [design].

use crate::medium_air_grid::{AirState, AirViewGpu};
use bevy::core_pipeline::prepass::ViewPrepassTextures;
use bevy::core_pipeline::FullscreenShader;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::render_resource::binding_types::{sampler, texture_2d, texture_3d, texture_depth_2d, texture_depth_2d_multisampled, uniform_buffer};
use bevy::render::render_resource::{
    AddressMode, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedPipelineState, CachedRenderPipelineId, ColorTargetState, ColorWrites, FilterMode,
    FragmentState, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor,
    ShaderStages, TextureFormat, TextureSampleType,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::view::ViewTarget;
use bevy::shader::ShaderCacheError;

/// The composite's layouts (single-sampled and multisampled depth), its pipelines by target
/// format and depth sample type, and the volume's sampler; all made on first use (nothing
/// while the light is off).
#[derive(Resource, Default)]
pub(crate) struct ComposePipelines {
    layouts: Option<[BindGroupLayoutDescriptor; 2]>,
    pipelines: HashMap<(TextureFormat, bool), CachedRenderPipelineId>,
    sampler: Option<Sampler>,
}

/// medium_air_compose.wesl's bindings 0-4: the picture, the depth (either sample type), the
/// volume, its linear clamping sampler, the uniform.
fn compose_layout(multisampled: bool) -> BindGroupLayoutDescriptor {
    let depth = if multisampled { texture_depth_2d_multisampled() } else { texture_depth_2d() };
    BindGroupLayoutDescriptor::new(
        if multisampled { "medium_air_compose_layout_multisampled" } else { "medium_air_compose_layout" },
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                depth,
                texture_3d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<crate::medium_air_grid::AirGpuUniform>(false),
            ),
        ),
    )
}

impl ComposePipelines {
    /// The pipeline for a target format and depth sample type, asked for on first use.
    pub(crate) fn pipeline(&mut self, cache: &PipelineCache, assets: &AssetServer, fullscreen: &FullscreenShader, format: TextureFormat, multisampled: bool) -> CachedRenderPipelineId {
        let layouts = self.layouts.get_or_insert_with(|| [compose_layout(false), compose_layout(true)]);
        let layout = layouts[multisampled as usize].clone();
        *self.pipelines.entry((format, multisampled)).or_insert_with(|| {
            let shader_defs = if multisampled { vec!["MULTISAMPLED".into()] } else { Vec::new() };
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("medium_air_compose_pipeline".into()),
                layout: vec![layout],
                vertex: fullscreen.to_vertex_state(),
                fragment: Some(FragmentState {
                    shader: assets.load("embedded://nvforever/medium_air_compose.wesl"),
                    shader_defs,
                    entry_point: Some("air_compose".into()),
                    targets: vec![Some(ColorTargetState { format, blend: None, write_mask: ColorWrites::ALL })],
                    ..default()
                }),
                ..default()
            })
        })
    }

    fn sampler(&mut self, device: &RenderDevice) -> Sampler {
        self.sampler
            .get_or_insert_with(|| {
                device.create_sampler(&SamplerDescriptor {
                    label: Some("medium_air_volume_sampler"),
                    mag_filter: FilterMode::Linear,
                    min_filter: FilterMode::Linear,
                    address_mode_u: AddressMode::ClampToEdge,
                    address_mode_v: AddressMode::ClampToEdge,
                    address_mode_w: AddressMode::ClampToEdge,
                    ..default()
                })
            })
            .clone()
    }
}

/// The CPU mirror of the shader's `air_lookup` along one column: `texels[k]` the sums at slice
/// k's far boundary, read at boundary coordinate `c` as a linear clamping sampler reads the
/// depth axis at w = (max(c, 1) - 0.5) / nz, scaled by min(c, 1).
#[cfg(test)]
pub fn lookup_mirror(texels: &[Vec4], c: f32) -> Vec4 {
    let nz = texels.len();
    if nz == 0 {
        return Vec4::ZERO;
    }
    let cc = c.clamp(0.0, nz as f32);
    let w = (cc.max(1.0) - 0.5) / nz as f32;
    // Texel coordinate: the sampler's u x size - 0.5, between neighbouring centres.
    let t = (w * nz as f32 - 0.5).clamp(0.0, (nz - 1) as f32);
    let k = (t.floor() as usize).min(nz - 1);
    let f = t - k as f32;
    let next = texels[(k + 1).min(nz - 1)];
    texels[k].lerp(next, f) * cc.min(1.0)
}

/// What the composite's pixel reads of the uniform and the picture: the inputs of
/// [`pixel_mirror`]. `curve` is the grid's (its `depth` the grid's end, view depth).
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub struct PixelMirror {
    /// The pixel's ray length per unit of view depth.
    pub ray_len: f32,
    /// The floored phase of the pixel's own ray (medium_air.rs `phase_floor`).
    pub p_eff: f32,
    pub v_ref: f32,
    pub rays: f32,
    pub lift: f32,
    pub shade: f32,
    /// The applied scale k x a (medium_air.rs `applied_scale`: the strength times the
    /// effective share; the uniform's phase2.z), before the deck's share.
    pub a: f32,
    /// The clouds' own transmittance (the uniform's cloud_mode.z), for the deck's share (medium_air.rs
    /// `deck_share`); `v_ref` is the reference as already taken.
    pub deck: f32,
    /// The tune's `SHARE` is set (the uniform's rays2.w): the deck's share is 1.
    pub share_set: bool,
    /// SKY and SKYPHASE as the uniform carries them.
    pub sky: f32,
    pub sky_phase: f32,
    pub fog: crate::medium_air::AirFog,
    pub curve: crate::medium_air::AirCurve,
    /// sky.rs `SKY_DEPTH_FROM` (the uniform's proj.w).
    pub sky_from: f32,
    /// The pixel's linear colour and the fog's.
    pub src: Vec3,
    pub fog_colour: Vec3,
}

/// The CPU mirror of the shader's `air_pixel` with `air_result` (and `air_compose`'s scale k a D,
/// once a pixel), over medium_air.rs's functions: `samples` are the depth samples' view depths (`None` for cleared depth: the
/// grid's end as its view depth, the end's radial distance), `lookup` the volume's (N, M) at a
/// view depth already held to the grid's end. One lookup at the **farthest** held depth when the
/// held depths agree ([`crate::medium_air::slices_agree`]; order-free, and the farthest
/// sample's sums hold all the fog in front of the nearer ones),
/// else one per sample; each sample's F_near and its blend t (the far blend joined with the fog
/// blend, [`crate::medium_air::sample_blend`]) from its own distance; a sample
/// with geometry holds its change per channel to `-F_near C'` (`compose_blended`); a gain is held
/// off a bright pixel (`hold_bright`, the brightness shared, t the sample's own); xyz the mean of the samples' linear colours, w the mean of their scalars
/// corr_t. Zero for no samples.
#[cfg(test)]
pub fn pixel_mirror(px: &PixelMirror, samples: &[Option<f32>], lookup: &dyn Fn(f32) -> [f32; 2]) -> Vec4 {
    use crate::medium_air::{blended_correction, compose_blended, deck_share, far_blend, hold_bright, lit_fraction, near_share, relative_lit, sample_blend, slices_agree, AIR_AGREE_SLICES, AIR_HDR_MAX, DISPLAY_GAMMA};
    let depth = px.curve.depth;
    // (cleared, view depth, radial distance), as `air_sample_at`.
    let place = |s: Option<f32>| match s {
        None => (true, depth, depth * px.ray_len),
        Some(z) => (false, z, z * px.ray_len),
    };
    if samples.is_empty() {
        return Vec4::ZERO;
    }
    let held: Vec<f32> = samples.iter().map(|&s| place(s).1.min(depth)).collect();
    let one_tap = slices_agree(&held, &px.curve, AIR_AGREE_SLICES);
    let relative = |z: f32| {
        let [n, m] = lookup(z.min(depth));
        relative_lit(lit_fraction(n, m), px.v_ref, px.rays, px.lift, px.shade)
    };
    // As the shader: one lookup at the farthest held depth when the depths agree, else one per
    // sample.
    let scale = px.a * deck_share(px.deck, px.share_set);
    // The picture as read (held before the transfer; no number as 0, which the shader's clamp gives
    // where max takes the number: WGSL leaves it open), and the largest channels.
    let src = Vec3::from_array(px.src.to_array().map(|v| if v > 0.0 { v.min(AIR_HDR_MAX) } else { 0.0 }));
    let peak = src.max_element().powf(1.0 / DISPLAY_GAMMA);
    let fog_peak = px.fog_colour.max(Vec3::ZERO).max_element().powf(1.0 / DISPLAY_GAMMA);
    let shared = one_tap.then(|| relative(held.iter().copied().fold(f32::NEG_INFINITY, f32::max)));
    let result = |s: Option<f32>| {
        let (cleared, z, d) = place(s);
        let vc = shared.unwrap_or_else(|| relative(z));
        let f_near = near_share(d, depth * px.ray_len, px.fog);
        let t = sample_blend(far_blend(d, cleared, px.sky_from), f_near);
        let c = blended_correction(vc, px.p_eff, px.sky_phase, scale, f_near, px.sky, t);
        let c = hold_bright(c, peak, fog_peak, t);
        compose_blended(src, c, px.fog_colour, t, (!cleared).then_some(f_near)).extend(c)
    };
    samples.iter().map(|&s| result(s)).sum::<Vec4>() / samples.len() as f32
}

/// Whether the composite has a pipeline for a view: the light (`none`), its `delta` and
/// `rays`, `lit` and the debug views; `off` never (the grid alone, the target
/// not flipped).
pub(crate) fn draws(view: crate::medium_air::AirView) -> bool {
    use crate::medium_air::AirView;
    matches!(view, AirView::None | AirView::Delta | AirView::Rays | AirView::Lit | AirView::Visibility | AirView::Slice | AirView::Share)
}

/// Whether the view shows the light (`none`, `delta`, `rays`): drawn only while [`light_on`].
pub(crate) fn shows_light(view: crate::medium_air::AirView) -> bool {
    use crate::medium_air::AirView;
    matches!(view, AirView::None | AirView::Delta | AirView::Rays)
}

/// Whether the uniform asks for the light: `light.w` (medium_air_light.rs's gate, 2 - FLOOR
/// while drawn, medium_air.rs `light_flag`) above 0.5 and `fog_colour.w` (the display-value
/// blend) 1.
pub(crate) fn light_on(uniform: &crate::medium_air_grid::AirGpuUniform) -> bool {
    uniform.light.w > 0.5 && uniform.fog_colour.w > 0.5
}

/// The views whose composite drew the light this render frame (render world): [`air_compose_pass`] adds a view when it issued its draw for the
/// light's views (`none`, `delta`, `rays`), the list is emptied at each frame's
/// `PrepareResources` ([`forget_composed`]), and Sun shafts (shafts.rs `shafts_pass`, after
/// tonemapping, in the same frame) skip their pass on such a view. `yielded` holds the views
/// whose shafts yielded when last asked, for the yield's two log lines. Registered by the
/// shafts' plugin, its one reader [design].
#[derive(Resource, Default, Debug)]
pub(crate) struct AirComposedViews {
    pub(crate) now: Vec<Entity>,
    pub(crate) yielded: Vec<Entity>,
}

/// Each render frame, before the graph: no view has composited the light yet.
pub(crate) fn forget_composed(mut composed: ResMut<AirComposedViews>) {
    if !composed.now.is_empty() {
        composed.now.clear();
    }
}

/// The composite: checked ready first, then the target flipped and every pixel written.
#[allow(clippy::type_complexity)]
pub(crate) fn air_compose_pass(
    view: ViewQuery<(&ViewTarget, &AirViewGpu, Option<&ViewPrepassTextures>)>,
    (mut state, composed): (ResMut<AirState>, Option<ResMut<AirComposedViews>>),
    mut pipelines: ResMut<ComposePipelines>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let entity = view.entity();
    let (target, gpu, prepass) = view.into_inner();
    let Some(compose) = gpu.compose.filter(|_| draws(gpu.view)) else { return };
    // The light's views run only while its gate is open (no flip otherwise; the main world
    // logged why).
    if !state.drawn || (shows_light(gpu.view) && !light_on(gpu.uniform.get())) {
        return;
    }
    let Some(prepass) = prepass else {
        state.tell("the view has no depth prepass");
        return;
    };
    let Some(depth) = prepass.depth_only_view() else {
        state.tell("the view has no depth prepass");
        return;
    };
    // In the frame the multisampling or the HDR switches the view and its pipeline can
    // disagree: skipped quietly for that frame.
    if crate::ssao::depth_multisampled(prepass) != Some(compose.multisampled) || !crate::ssao::format_matches(target, compose.format) {
        return;
    }
    let pipeline = match cache.get_render_pipeline_state(compose.id) {
        CachedPipelineState::Ok(_) => cache.get_render_pipeline(compose.id),
        // The cache's waiting states (the shader or an import not loaded yet), as the grid pass
        // takes them: not a failure.
        CachedPipelineState::Err(ShaderCacheError::ShaderNotLoaded(_) | ShaderCacheError::ShaderImportNotYetAvailable) => return,
        CachedPipelineState::Err(e) => {
            let reason = format!("the composite's pipeline failed: {e}");
            state.tell(&reason);
            return;
        }
        _ => return,
    };
    let (Some(pipeline), Some(uniform), Some(layouts)) = (pipeline, gpu.uniform.binding(), pipelines.layouts.clone()) else { return };
    let volume_sampler = pipelines.sampler(&device);
    let layout = cache.get_bind_group_layout(&layouts[compose.multisampled as usize]);
    let post = target.post_process_write();
    let group = device.create_bind_group("medium_air_compose_bind_group", &layout, &BindGroupEntries::sequential((post.source, depth, &gpu.volume_view, &volume_sampler, uniform)));
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let span = diagnostics.time_span(ctx.command_encoder(), "nvf_medium_air_compose");
    {
        let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
            label: Some("medium_air_compose_pass"),
            color_attachments: &[Some(RenderPassColorAttachment { view: post.destination, depth_slice: None, resolve_target: None, ops: Operations::default() })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
    span.end(ctx.command_encoder());
    // The light composed on this view this frame: its Sun shafts yield (shafts.rs).
    if let Some(mut composed) = composed.filter(|_| shows_light(gpu.view)) {
        if !composed.now.contains(&entity) {
            composed.now.push(entity);
        }
    }
    if !state.told_composed {
        state.told_composed = true;
        info!(
            "volumetric medium: light in the air: composite drawn (view {}, multisampled depth {})",
            crate::medium_air_grid::view_name(gpu.view),
            if compose.multisampled { "yes" } else { "no" }
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::medium_air::{AirFog, AirGrid};
    use crate::medium_air_grid::tests::{block, plain_wgsl, validate};
    use std::cell::Cell;

    const COMPOSE: &str = include_str!("medium_air_compose.wesl");
    /// nvf_assets' first-person depth helpers, which the composite imports (no imports of its own).
    const FIRST_PERSON: &str = include_str!("../../nvf-assets/src/first_person_depth.wesl");

    fn prelude() -> String {
        // As bevy_core_pipeline declares the fullscreen vertex output.
        format!("struct FullscreenVertexOutput {{\n    @builtin(position) position: vec4<f32>,\n    @location(0) uv: vec2<f32>,\n}};\n{}\n", FIRST_PERSON.replace("\r\n", "\n"))
    }

    #[test]
    fn the_composite_is_valid_wgsl_with_either_depth() {
        for multisampled in [false, true] {
            let defs: &[&str] = if multisampled { &["MULTISAMPLED"] } else { &[] };
            validate(&plain_wgsl(COMPOSE, defs, &prelude()), "air_compose", wgpu::naga::ShaderStage::Fragment, &format!("composite, multisampled {multisampled}"));
        }
    }

    #[test]
    fn the_composite_declares_what_the_layout_binds() {
        let source = COMPOSE.replace("\r\n", "\n");
        for binding in [
            "@group(0) @binding(0) var picture: texture_2d<f32>;",
            "@if(MULTISAMPLED) @group(0) @binding(1) var depth: texture_depth_multisampled_2d;",
            "@if(!MULTISAMPLED) @group(0) @binding(1) var depth: texture_depth_2d;",
            "@group(0) @binding(2) var volume: texture_3d<f32>;",
            "@group(0) @binding(3) var volume_sampler: sampler;",
            "@group(0) @binding(4) var<uniform> air: AirUniform;",
        ] {
            assert!(source.contains(binding), "{binding}");
        }
        assert!(source.contains("fn air_compose(in: FullscreenVertexOutput) -> @location(0) vec4<f32>"));
        // The lookup as lookup_mirror has it.
        let lookup = block(&source, "fn air_lookup(");
        assert!(lookup.contains("let w = (max(cc, 1.0) - 0.5) / nz;"));
        assert!(lookup.contains("* min(cc, 1.0);"));
        // The views' codes as medium_air.rs numbers them.
        use crate::medium_air::AirView;
        for (name, view) in [
            ("VIEW_NONE", AirView::None),
            ("VIEW_VISIBILITY", AirView::Visibility),
            ("VIEW_SLICE", AirView::Slice),
            ("VIEW_SHARE", AirView::Share),
            ("VIEW_DELTA", AirView::Delta),
            ("VIEW_LIT", AirView::Lit),
            ("VIEW_RAYS", AirView::Rays),
        ] {
            assert!(source.contains(&format!("const {name}: u32 = {}u;", view.code())), "{name}");
        }
    }

    /// The zero boundary at the camera: boundary k + 1 returns texel k exactly, c = 0 zero,
    /// c = 0.5 half of texel 0, c at or beyond nz the last texel; between boundaries linear.
    #[test]
    fn the_lookup_reads_boundary_k_plus_one_at_texel_k() {
        let texels: Vec<Vec4> = (0..64).map(|k| Vec4::new(k as f32 + 1.0, 2.0 * k as f32, 0.5, 1.0)).collect();
        for k in 0..64 {
            assert_eq!(lookup_mirror(&texels, k as f32 + 1.0), texels[k], "boundary {}", k + 1);
        }
        assert_eq!(lookup_mirror(&texels, 0.0), Vec4::ZERO);
        assert_eq!(lookup_mirror(&texels, -3.0), Vec4::ZERO);
        assert_eq!(lookup_mirror(&texels, 0.5), texels[0] * 0.5);
        assert_eq!(lookup_mirror(&texels, 64.0), texels[63]);
        assert_eq!(lookup_mirror(&texels, 1e9), texels[63]);
        let mid = lookup_mirror(&texels, 10.5);
        assert!((mid - (texels[9] + texels[10]) * 0.5).abs().max_element() < 1e-5, "{mid}");
        assert_eq!(lookup_mirror(&[], 3.0), Vec4::ZERO);
    }

    /// The value of `const NAME: f32 = <value>;` in the shader.
    fn shader_constant(source: &str, name: &str) -> f32 {
        let start = format!("const {name}: f32 = ");
        let line = source.lines().find(|l| l.starts_with(&start)).unwrap_or_else(|| panic!("{name}"));
        line[start.len()..].trim_end_matches(';').trim().parse().unwrap_or_else(|_| panic!("{name}: {line}"))
    }

    /// The shader's constants are medium_air.rs's.
    #[test]
    fn the_shader_constants_are_medium_airs() {
        use crate::medium_air::{AIR_AGREE_SLICES, AIR_BRIGHT_FROM, AIR_BRIGHT_LEAST, AIR_BRIGHT_TO, AIR_DECK_FROM, AIR_DECK_LEAST, AIR_DECK_TO, AIR_FOG_BLEND_FROM, AIR_HDR_MAX, DISPLAY_GAMMA, RATIO_EMPTY, SKY_TAPER_FROM};
        let source = COMPOSE.replace("\r\n", "\n");
        assert_eq!(shader_constant(&source, "RATIO_EMPTY"), RATIO_EMPTY);
        assert_eq!(shader_constant(&source, "DISPLAY_GAMMA"), DISPLAY_GAMMA);
        assert_eq!(shader_constant(&source, "AIR_AGREE_SLICES"), AIR_AGREE_SLICES);
        assert_eq!(shader_constant(&source, "SKY_TAPER_FROM"), SKY_TAPER_FROM);
        assert_eq!(shader_constant(&source, "AIR_FOG_BLEND_FROM"), AIR_FOG_BLEND_FROM);
        assert_eq!(shader_constant(&source, "AIR_DECK_FROM"), AIR_DECK_FROM);
        assert_eq!(shader_constant(&source, "AIR_DECK_TO"), AIR_DECK_TO);
        assert_eq!(shader_constant(&source, "AIR_DECK_LEAST"), AIR_DECK_LEAST);
        assert_eq!(shader_constant(&source, "AIR_BRIGHT_FROM"), AIR_BRIGHT_FROM);
        assert_eq!(shader_constant(&source, "AIR_BRIGHT_TO"), AIR_BRIGHT_TO);
        assert_eq!(shader_constant(&source, "AIR_BRIGHT_LEAST"), AIR_BRIGHT_LEAST);
        assert_eq!(shader_constant(&source, "AIR_HDR_MAX"), AIR_HDR_MAX);
        assert_eq!(shader_constant(&source, "DELTA_GAIN"), 4.0);
    }

    /// The shader's structure where no number can check it (naga validates, it does not run):
    /// every colour the entry computes leaves through `air_held` (the untouched pixel passes as
    /// `src`), the sample's result too; the lit fraction takes no phase (the volume's sums are
    /// phase-free) and no phase floor is left; shadow's
    /// hold is in the sample's scalar.
    #[test]
    fn every_computed_colour_is_held() {
        let source = COMPOSE.replace("\r\n", "\n");
        let entry = block(&source, "fn air_compose(");
        let returns: Vec<&str> = entry.lines().map(str::trim).filter(|l| l.starts_with("return")).collect();
        assert!(returns.len() >= 8, "{returns:?}");
        for line in returns {
            assert!(line == "return src;" || line.contains("air_held("), "{line}");
        }
        assert!(block(&source, "fn air_result(").contains("air_held("));
        assert!(source.contains("fn air_lit_fraction(n: f32, m: f32) -> f32"));
        assert!(!source.contains("PHASE_FLOOR"));
        assert!(block(&source, "fn air_correction(").contains("-min(f_near, 1.0)"));
        // One F_near a sample, and one blend for the correction and the budget.
        let result = block(&source, "fn air_result(");
        assert_eq!(result.matches("air_near_share(").count(), 1);
        assert!(!block(&source, "fn air_correction(").contains("air_near_share("));
        assert!(result.contains("let t = air_sample_blend(air_far_blend(s), f_near);"));
        // The bright hold acts on gains alone and before the colour and the views' scalar.
        assert!(result.contains("if c > 0.0 {\n        c = c * (1.0 - t * air_bright_hold(px.src_peak, c * mix(px.fog_peak, px.src_peak, t)));"));
        assert!(result.find("air_bright_hold(").expect("the hold") < result.find("var delta = c * budget;").expect("the change"));
        assert!(source.contains("px.src_d = pow(air_held(src.rgb), vec3<f32>(1.0 / DISPLAY_GAMMA));"));
        assert!(result.contains("air_correction(px, f_near, vc, t)") && result.contains("mix(px.fog_d, px.src_d, vec3<f32>(t))"));
    }

    /// The composite's texture addressing (`lookup_mirror`) reads what medium_air.rs's 1D
    /// reference `sums_at` gives, for a real column, at boundaries, between them, inside the
    /// first slice and beyond the end (a linear sampler between texel centres k - 0.5 and
    /// k + 0.5 is the linear interpolation between boundaries k and k + 1).
    #[test]
    fn the_lookup_mirror_is_the_reference_lookup() {
        use crate::medium_air::{column_nm, sums_at};
        let g = AirGrid::for_view(16.0 / 9.0, None, None);
        let fog = AirFog { near: 0.0, far: 30_000.0, power: 0.6, strength: 1.0 };
        let nm = column_nm(&g, 1.1, fog, &|k| if k.is_multiple_of(3) { 0.0 } else { 1.0 });
        let texels: Vec<Vec4> = nm.iter().map(|s| Vec4::new(s[0], s[1], 0.0, 1.0)).collect();
        for i in 0..=700 {
            let c = i as f32 * 0.1;
            let a = lookup_mirror(&texels, c);
            let b = sums_at(&nm, c);
            assert!((a.x - b[0]).abs() < 1e-6 && (a.y - b[1]).abs() < 1e-6, "c {c}: {a} vs {b:?}");
        }
    }

    /// No occluder (every V 1), a coarse grid 16 x 8 x 64, g 0.8,
    /// cap 100, a 75 degree vertical field at 16:9 and the sun straight ahead. With the
    /// phase-free sums every column stores N = M exactly (df x 1 is df, the same additions), and
    /// a linear sampler weights N and M alike between columns and slices, so Vbar = N / M is
    /// exactly 1 at every pixel position. The old form (N = sum dF V p per column, divided by
    /// the pixel's own p) gave about 0.655 at the centre: HG(0.8) at the neighbouring columns'
    /// angle (7.3 degrees, cos 0.9919: 0.36 / 0.0530^1.5 = 29.5) over its peak (0.36 / 0.04^1.5
    /// = 45).
    #[test]
    fn an_unobstructed_sun_is_fully_lit_between_columns() {
        use crate::medium_air::{column_ndc, column_nm, lit_fraction, ray_len, AirPhase};
        let g = AirGrid::for_view(16.0 / 9.0, Some([16, 8, 64]), None);
        assert_eq!((g.nx, g.ny, g.nz), (16, 8, 64));
        let proj_y = 1.0 / 37.5f32.to_radians().tan();
        let proj_x = proj_y / (16.0 / 9.0);
        let fog = AirFog { near: 0.0, far: 200_000.0, power: 0.6, strength: 1.0 };
        let phase = AirPhase { g: 0.8, back_g: -0.3, back_weight: 0.0, cap: 100.0 }.normed();
        // Per column: its ray length, the sun's phase on it (the sun ahead: cos = 1 / ray_len),
        // its texels.
        let columns: Vec<(f32, Vec<Vec4>)> = (0..g.ny)
            .flat_map(|j| (0..g.nx).map(move |i| (i, j)))
            .map(|(i, j)| {
                let rl = ray_len(column_ndc(&g, i, j), proj_x, proj_y);
                let texels = column_nm(&g, rl, fog, &|_| 1.0).iter().map(|s| Vec4::new(s[0], s[1], 0.0, 1.0)).collect();
                (phase.phase(1.0 / rl), texels)
            })
            .collect();
        // A linear clamping sampler at continuous column coordinates (x, y): texel coordinate
        // x - 0.5; `weight` scales each column's value (1: the phase-free sums; the column's
        // phase: the old sums).
        let sample = |x: f32, y: f32, c: f32, weighted: bool| -> Vec4 {
            let tx = (x - 0.5).clamp(0.0, (g.nx - 1) as f32);
            let ty = (y - 0.5).clamp(0.0, (g.ny - 1) as f32);
            let (i0, j0) = (tx.floor() as u32, ty.floor() as u32);
            let (i1, j1) = ((i0 + 1).min(g.nx - 1), (j0 + 1).min(g.ny - 1));
            let (fx, fy) = (tx - i0 as f32, ty - j0 as f32);
            let at = |i: u32, j: u32| {
                let (p, texels) = &columns[(j * g.nx + i) as usize];
                let v = lookup_mirror(texels, c);
                if weighted { Vec4::new(v.x * p, v.y, v.z, v.w) } else { v }
            };
            at(i0, j0).lerp(at(i1, j0), fx).lerp(at(i0, j1).lerp(at(i1, j1), fx), fy)
        };
        for xi in 0..=43 {
            for yi in 0..=27 {
                let (x, y) = (xi as f32 * 0.37, yi as f32 * 0.29);
                for c in [0.3f32, 1.0, 7.5, 30.2, 63.9, 64.0] {
                    let v = sample(x, y, c, false);
                    assert_eq!(lit_fraction(v.x, v.y), 1.0, "x {x} y {y} c {c}: {v}");
                }
            }
        }
        // The centre (between the four middle columns): exactly 1 now; the old form's shadow.
        let now = sample(8.0, 4.0, 64.0, false);
        assert_eq!(lit_fraction(now.x, now.y), 1.0);
        let old = sample(8.0, 4.0, 64.0, true);
        let old_vbar = old.x / (phase.phase(1.0) * old.y);
        assert!((0.6..0.7).contains(&old_vbar), "{old_vbar}");
    }

    /// Inputs for the pixel tests: the default grid (48,000), a clear fog, sky.rs's sky distance.
    fn pixel_inputs() -> PixelMirror {
        PixelMirror {
            ray_len: 1.05,
            p_eff: 1.8,
            v_ref: 0.8,
            rays: 4.0,
            lift: 1.75,
            shade: 0.5,
            a: 0.8,
            // No deck: the deck's share is 1.
            deck: 1.0,
            share_set: false,
            sky: 0.4,
            sky_phase: 0.5,
            fog: AirFog { near: 0.0, far: 200_000.0, power: 0.6, strength: 1.0 },
            curve: AirGrid::for_view(16.0 / 9.0, None, None).curve(),
            sky_from: crate::sky::SKY_DEPTH_FROM,
            src: Vec3::new(0.3, 0.45, 0.7),
            fog_colour: Vec3::new(0.8, 0.7, 0.6),
        }
    }

    /// One form, no step at any distance. Near samples the fog does not cover (F_near at most
    /// `AIR_FOG_BLEND_FROM`) are the old world rule exactly, cleared depth the old sky rule (to the f32 round trip of the display transfer,
    /// 1e-6 relative), opaque terrain beyond the sky's distance the same as cleared depth, and
    /// the colour is continuous across 0.6 x sky_from and sky_from (the smoothstep's slope is 0
    /// at both ends; inside it moves t by at most 1.5 / (0.4 x 157,500) per unit).
    #[test]
    fn the_far_blend_is_continuous_and_meets_both_old_rules() {
        use crate::medium_air::{applied_ratio, compose_ratio, compose_sky, correction, far_blend, lit_fraction, near_share, relative_lit, sky_modulation, AIR_FOG_BLEND_FROM};
        let px = pixel_inputs();
        let lookup = |_: f32| [0.3f32, 0.5];
        let sf = px.sky_from;
        // The far blend's own ends.
        assert_eq!(far_blend(0.6 * sf, false, sf), 0.0);
        assert_eq!(far_blend(0.1 * sf, false, sf), 0.0);
        assert_eq!(far_blend(sf, false, sf), 1.0);
        assert_eq!(far_blend(3.0 * sf, false, sf), 1.0);
        assert_eq!(far_blend(10.0, true, sf), 1.0);
        assert!((far_blend(0.8 * sf, false, sf) - 0.5).abs() < 1e-6);
        let at = |d: f32| pixel_mirror(&px, &[Some(d / px.ray_len)], &lookup);
        let cleared = pixel_mirror(&px, &[None], &lookup);
        // Continuity at both ends of the blend (2 units apart) and no step anywhere across it.
        for d0 in [0.6 * sf, sf] {
            let (below, above) = (at(d0 - 1.0), at(d0 + 1.0));
            assert!((below - above).abs().max_element() < 1e-4, "{d0}: {below} vs {above}");
        }
        let mut last = at(0.5 * sf);
        for i in 1..=600 {
            let now = at((0.5 + i as f32 * 0.001) * sf);
            assert!((now - last).abs().max_element() < 1e-2, "step at {}: {last} -> {now}", (0.5 + i as f32 * 0.001) * sf);
            last = now;
        }
        // The limits. vc and F_near as the old rules took them. The world's: in a fog of strength
        // AIR_FOG_BLEND_FROM, whose share never passes the fog blend's start (in the clear fog it
        // is 0.44 at the grid's end).
        let vc = relative_lit(lit_fraction(0.3, 0.5), px.v_ref, px.rays, px.lift, px.shade);
        let end = px.curve.depth * px.ray_len;
        let thin = PixelMirror { fog: AirFog { strength: AIR_FOG_BLEND_FROM, ..px.fog }, ..px };
        for d in [300.0f32, 2_000.0, 30_000.0, 0.59 * sf] {
            let f_near = near_share(d, end, thin.fog);
            assert!(f_near > 0.0 && f_near <= AIR_FOG_BLEND_FROM);
            let old = compose_ratio(px.src, correction(f_near, applied_ratio(vc, px.p_eff, 1.0), px.a), px.fog_colour);
            let new = pixel_mirror(&thin, &[Some(d / px.ray_len)], &lookup).truncate();
            assert!((new - old).abs().max_element() <= 1e-6 * old.max_element().max(1.0), "{d}: {new} vs {old}");
        }
        let f_end = near_share(end, end, px.fog);
        let old_sky = compose_sky(px.src, sky_modulation(vc, px.p_eff, px.sky_phase, px.a, f_end, px.sky));
        assert!((cleared.truncate() - old_sky).abs().max_element() <= 1e-6 * old_sky.max_element().max(1.0), "{cleared} vs {old_sky}");
        // Opaque terrain at and beyond the sky's distance is cleared depth's result exactly
        // (both read the grid's end and its F, t = 1).
        for d in [1.01 * sf, 1.1 * sf, 190_000.0] {
            assert_eq!(at(d), cleared, "{d}");
        }
    }

    /// Through the bright hold a brighter grey pixel never comes out darker, with no step, at the Natural
    /// look (LIFT 1.4, CAP 1.5, SKY 0.25, SKYPHASE 0) and a stronger one as a stress test (LIFT 2,
    /// CAP 2.5, SKY 0.3, SKYPHASE 0.2), strength 200 %, lit air at the phase's peak, shares of 1 and
    /// 0.62: the sky, the dome, terrain through the fog blend and through the far blend, over three
    /// fog colours. (A fade of the gain over an input of 0.70 to 0.95, the first ends, fell by up to
    /// 0.46 for the stronger look's sky.) For the sky: the old rule while the lifted value is at most the
    /// hold's start, no gain from its end on, never past the larger of the input and the end.
    #[test]
    fn a_brighter_pixel_never_comes_out_darker() {
        use crate::medium_air::{applied_scale, blended_correction, compose_sky, AIR_BRIGHT_FROM, AIR_BRIGHT_TO};
        let ramp = AirFog { near: 0.0, far: 30_000.0, power: 1.0, strength: 1.0 };
        let thin = AirFog { strength: 0.3, ..ramp };
        let lit = |_: f32| [0.5f32, 0.5];
        let shown = |v: Vec4| v.x.powf(1.0 / 2.2);
        for (lift, cap, sky, sky_phase, rays, shade) in [(1.4f32, 1.5f32, 0.25f32, 0.0f32, 3.0f32, 0.9f32), (2.0, 2.5, 0.3, 0.2, 7.0, 0.85)] {
            for share in [0.62f32, 1.0] {
                let a = applied_scale(2.0, share);
                let places = [(ramp, None), (ramp, Some(350_000.0f32)), (ramp, Some(60_000.0)), (ramp, Some(12_000.0)), (ramp, Some(18_000.0)), (ramp, Some(24_000.0)), (ramp, Some(28_000.0)), (thin, Some(110_000.0)), (thin, Some(126_000.0)), (thin, Some(145_000.0)), (thin, None), (thin, Some(5_000.0))];
                for (fog, at) in places {
                    for fog_shown in [0.4f32, 0.8, 1.0] {
                        let px = |y: f32| PixelMirror { ray_len: 1.0, p_eff: cap, rays, lift, shade, a, sky, sky_phase, fog, src: Vec3::splat(y.powf(2.2)), fog_colour: Vec3::splat(fog_shown.powf(2.2)), ..pixel_inputs() };
                        let mut last = shown(pixel_mirror(&px(0.0), &[at], &lit));
                        for i in 1..=800 {
                            let y = i as f32 * 0.002;
                            let now = shown(pixel_mirror(&px(y), &[at], &lit));
                            // The two powers there and back: under 1e-6 relative each way.
                            assert!(now >= last - 2e-5, "LIFT {lift} share {share} {at:?} C' {fog_shown}, {y}: {last} -> {now}");
                            // The slope is at most 1 and the scalar's own (at most 8 x t).
                            assert!(now - last <= 0.002 * 9.0 + 2e-5, "a step at {y}: {last} -> {now}");
                            last = now;
                        }
                    }
                }
                // The sky alone (F_near 1, t 1).
                let c = blended_correction(lift, cap, sky_phase, a, 1.0, sky, 1.0);
                assert!(c > 0.0);
                for i in 1..=800 {
                    let y = i as f32 * 0.002;
                    let px = PixelMirror { ray_len: 1.0, p_eff: cap, rays, lift, shade, a, sky, sky_phase, fog: ramp, src: Vec3::splat(y.powf(2.2)), fog_colour: Vec3::ONE, ..pixel_inputs() };
                    let out = pixel_mirror(&px, &[None], &lit);
                    let o = shown(out);
                    assert!(o >= y - 2e-5 && o <= y.max(AIR_BRIGHT_TO) + 2e-5, "LIFT {lift}, {y}: {o}");
                    if y * (1.0 + c) <= AIR_BRIGHT_FROM - 1e-4 {
                        let old = compose_sky(px.src, c);
                        assert!((out.truncate() - old).abs().max_element() <= 2e-6 && out.w == c, "LIFT {lift}, {y}: {out} vs {old}");
                    }
                    if y >= AIR_BRIGHT_TO + 1e-4 {
                        assert!(out.w.abs() < 1e-6 && (o - y).abs() < 2e-5, "LIFT {lift}, {y}: {out}");
                    }
                }
            }
        }
    }

    /// One scalar for the three channels: a colour the hold acts on keeps its hue, a saturated one
    /// (one channel past the hold's end) comes back as it was; the samples of a pixel share its
    /// brightness and take their own t; shadow and the near world's gain are as they were.
    #[test]
    fn the_bright_hold_keeps_the_hue_and_the_near_world() {
        use crate::medium_air::{blended_correction, relative_lit, AIR_BRIGHT_FROM, AIR_BRIGHT_TO};
        let fog = AirFog { near: 0.0, far: 30_000.0, power: 1.0, strength: 1.0 };
        let base = PixelMirror { ray_len: 1.0, fog, ..pixel_inputs() };
        let lit = |_: f32| [0.5f32, 0.5];
        let show = |v: Vec4| v.truncate().powf(1.0 / 2.2);
        // Halfway through the hold: room (TO - FROM) / 2 = 0.13, a gain of 0.464 x 0.63 = 0.292, of
        // which 0.292 x 0.13 / 0.422 = 0.090 is kept: 1.143 x the input.
        let y = Vec3::new(1.0, 0.5, 0.25) * (0.5 * (AIR_BRIGHT_FROM + AIR_BRIGHT_TO));
        let held = show(pixel_mirror(&PixelMirror { src: y.powf(2.2), ..base }, &[None], &lit));
        let ratio = held / y;
        assert!((ratio.x - 1.143).abs() < 2e-3 && (ratio - Vec3::splat(ratio.x)).abs().max_element() < 1e-4, "{held}");
        let y = Vec3::new(1.1, 0.3, 0.1);
        let saturated = pixel_mirror(&PixelMirror { src: y.powf(2.2), ..base }, &[None], &lit);
        assert!(saturated.w == 0.0 && (show(saturated) - y).abs().max_element() < 1e-5, "{saturated}");
        // White under MSAA: the sky sample gains nothing, the near one the world's whole gain.
        let white = PixelMirror { src: Vec3::ONE, ..base };
        let near = pixel_mirror(&white, &[Some(1_500.0)], &lit);
        let world = blended_correction(1.75, base.p_eff, base.sky_phase, base.a, 0.05, base.sky, 0.0);
        assert!((near.w - world).abs() < 1e-5 && world > 0.0, "{near} vs {world}");
        let both = pixel_mirror(&white, &[None, Some(1_500.0)], &lit);
        assert!((both.w - 0.5 * near.w).abs() < 1e-6, "{both} vs {near}");
        // Shadow on a bright sky: the old rule.
        let dark = pixel_mirror(&white, &[None], &|_| [0.0, 0.5]);
        let vc = relative_lit(0.0, base.v_ref, base.rays, base.lift, base.shade);
        assert_eq!(dark.w, blended_correction(vc, base.p_eff, base.sky_phase, base.a, 1.0, base.sky, 1.0));
        assert!(dark.w < 0.0);
    }

    /// Under a deck the whole pixel's correction is the deck's share of it, added or taken alike,
    /// near, far, on the dome and on cleared depth, once a pixel (not a sample); a closed deck keeps
    /// its least, an opaque one (0) too; with `SHARE` set the deck changes nothing. The reference visibility is the mirror's own (`v_ref`), untouched by it.
    #[test]
    fn the_deck_weakens_every_sample_alike() {
        use crate::medium_air::{deck_share, AIR_DECK_FROM, AIR_DECK_LEAST, AIR_DECK_TO};
        // RAYS 1, p_eff 1: lit air is 1.25 x the reference's, shadowed air 0.9 of it.
        let base = PixelMirror { p_eff: 1.0, rays: 1.0, shade: 0.9, src: Vec3::splat(0.05), ..pixel_inputs() };
        let half = 0.5 * (AIR_DECK_FROM + AIR_DECK_TO);
        let d = deck_share(half, false);
        assert!((d - (AIR_DECK_LEAST + (1.0 - AIR_DECK_LEAST) * 0.5)).abs() < 1e-5);
        for n in [0.5f32, 0.0] {
            let lookup = |_: f32| [n, 0.5f32];
            for samples in [vec![Some(2_000.0f32)], vec![Some(60_000.0)], vec![Some(120_000.0)], vec![Some(350_000.0)], vec![None], vec![Some(2_000.0), Some(30_000.0), None, Some(350_000.0)]] {
                let open = pixel_mirror(&base, &samples, &lookup);
                let deck = pixel_mirror(&PixelMirror { deck: half, ..base }, &samples, &lookup);
                assert!((open.w > 1e-4) == (n > 0.0) && open.w.abs() > 1e-4 && (deck.w - d * open.w).abs() < 1e-6, "{n} {samples:?}: {deck} vs {open}");
                for t in [AIR_DECK_FROM, 0.0] {
                    let closed = pixel_mirror(&PixelMirror { deck: t, ..base }, &samples, &lookup);
                    assert!((closed.w - AIR_DECK_LEAST * open.w).abs() < 1e-6, "{n} {samples:?}: {closed} vs {open}");
                }
                let set = pixel_mirror(&PixelMirror { deck: 0.0, share_set: true, ..base }, &samples, &lookup);
                assert_eq!(set, open, "{n} {samples:?}: SHARE set");
            }
        }
        // The shader: once a pixel, in the light's branch, before the samples' loop.
        let source = COMPOSE.replace("\r\n", "\n");
        assert_eq!(source.matches("air_deck_share()").count(), 2, "the function and its one call");
        assert!(block(&source, "fn air_compose(").contains("px.scale = air.phase2.z * air_deck_share();"));
        assert!(block(&source, "fn air_correction(").contains("px.scale * f_near"));
        assert_eq!(source.matches("air.phase2.z").count(), 1);
        assert!(block(&source, "fn air_deck_share(").contains("let x = clamp((air.cloud_mode.z - AIR_DECK_FROM) / (AIR_DECK_TO - AIR_DECK_FROM), 0.0, 1.0);"));
        assert!(block(&source, "fn air_deck_share(").contains("if air.rays2.w > 0.5 {\n        return 1.0;"));
    }

    /// A silhouette the fog covers is not cut out of the sky behind it: at F_near 1 a sample at
    /// 60,000 units and the dome behind it at 350,000, both the fog's colour, take the same result
    /// to the last bit, in lit, average and shadowed air, at either strength; through the fog's
    /// whole ramp the scalar moves with no step.
    #[test]
    fn fogged_terrain_meets_the_sky_behind_it() {
        use crate::medium_air::AIR_FOG_BLEND_FROM;
        // Dark, so no gain here reaches the bright hold (its own test).
        let fog_colour = Vec3::new(0.002, 0.0016, 0.0012);
        let base = PixelMirror {
            ray_len: 1.0,
            fog: AirFog { near: 0.0, far: 30_000.0, power: 1.0, strength: 1.0 },
            src: fog_colour,
            fog_colour,
            ..pixel_inputs()
        };
        for a in [0.62f32, 2.0] {
            let px = PixelMirror { a, ..base };
            for n in [0.5f32, 0.4, 0.0] {
                let lookup = |_: f32| [n, 0.5f32];
                let plain = pixel_mirror(&px, &[Some(60_000.0)], &lookup);
                let dome = pixel_mirror(&px, &[Some(350_000.0)], &lookup);
                assert_eq!(plain, dome, "a {a} n {n}");
                // The ramp: F from 0 to 1 over 30,000 units, in steps of 1/1,200 (the scalar's
                // slope in F is at most a (LIFT 1.75 x p_eff 1.8 - 1) = 2.15 a from F itself, and
                // 2.15 a x 1.5 / (1 - start) from the blend).
                let most = a * (2.15 + 2.15 * 1.5 / (1.0 - AIR_FOG_BLEND_FROM)) / 1_200.0;
                let mut last = pixel_mirror(&px, &[Some(1.0)], &lookup).w;
                for i in 1..=1_200 {
                    let now = pixel_mirror(&px, &[Some(i as f32 * 25.0 + 1.0)], &lookup).w;
                    assert!((now - last).abs() <= 1.01 * most, "a {a} n {n} step {i}: {last} -> {now}");
                    last = now;
                }
            }
        }
        // Lit air at the fog blend's start still takes the world's rule, past it less of it
        // (vc = clamp(1 + 4 x 0.2 / 0.8, 0.5, 1.75) = 1.75).
        let lit = |_: f32| [0.5f32, 0.5];
        let at = |f: f32| pixel_mirror(&base, &[Some(f * 30_000.0)], &lit).w;
        let world = |f: f32| base.a * f * (1.75 * base.p_eff - 1.0);
        let from = AIR_FOG_BLEND_FROM;
        assert!((at(from) - world(from)).abs() < 1e-4, "{} {}", at(from), world(from));
        let half = 0.5 * (from + 1.0);
        assert!(at(half) < world(half), "{} {}", at(half), world(half));
        // F 1: the sky's rule, SKY 0.4 and SKYPHASE 0.5: 0.8 x (1.75 x 1.4 - 1) x 0.4 = 0.464,
        // where the world's rule gave 0.8 x (1.75 x 1.8 - 1) = 1.72.
        assert!((at(1.0) - 0.464).abs() < 1e-5 && (world(1.0) - 1.72).abs() < 1e-5, "{} {}", at(1.0), world(1.0));
    }

    /// Shadow removes at most the budget share F_near of B_t. The case:
    /// a = 1, SHADE 0, RAYS 8, full shadow, f = F_near = 0.5, a white surface in white fog
    /// (Y' = 0.5 x 1 + 0.5 x 1 = 1): the result is (1 - f) S' = 0.5 in display values, and a
    /// stray SKY 2 in the uniform (the parser holds it to 1) cannot take more at any t.
    #[test]
    fn shadow_takes_at_most_the_budget_share() {
        use crate::medium_air::{blended_correction, compose_blended, relative_lit, AirTune};
        let vc = relative_lit(0.0, 1.0, 8.0, 1.75, 0.0);
        assert_eq!(vc, 0.0);
        let white = Vec3::ONE;
        for (sky, t) in [(1.0f32, 0.0f32), (2.0, 0.0), (2.0, 0.5), (2.0, 1.0), (1.0, 1.0)] {
            // a 1, p_eff 1, F_near 0.5.
            let raw = 0.5 * (vc - 1.0) * (1.0 + (sky - 1.0) * t);
            let c = blended_correction(vc, 1.0, 0.0, 1.0, 0.5, sky, t);
            assert_eq!(c, raw.max(-0.5), "SKY {sky} t {t}");
            let shown = compose_blended(white, c, white, t, Some(0.5)).powf(1.0 / 2.2);
            assert!((shown - Vec3::splat(0.5)).abs().max_element() < 1e-6, "SKY {sky} t {t}: {shown}");
        }
        // The whole chain: fog 0..2,000 linear, a sample at 1,000 units (F = 0.5 exactly), no
        // light in the column (N 0).
        let px = PixelMirror {
            ray_len: 1.0,
            p_eff: 1.0,
            v_ref: 1.0,
            rays: 8.0,
            shade: 0.0,
            a: 1.0,
            sky: 1.0,
            sky_phase: 0.0,
            fog: AirFog { near: 0.0, far: 2_000.0, power: 1.0, strength: 1.0 },
            src: white,
            fog_colour: white,
            ..pixel_inputs()
        };
        let out = pixel_mirror(&px, &[Some(1_000.0)], &|_| [0.0, 0.4]);
        assert!((out.truncate().powf(1.0 / 2.2) - Vec3::splat(0.5)).abs().max_element() < 1e-6, "{out}");
        assert!((out.w + 0.5).abs() < 1e-6);
        // The parser holds SKY to a weight.
        assert_eq!(AirTune::parse("SKY=2", &AirTune::default()).sky_weight, 1.0);
    }

    /// The extreme (G 0.9, CAP at its most, LIFT 3, REF 0.25, share 1, lit air on the phase peak,
    /// white): CAP is held to 8, so the form's display value is at most 1 + (3 x 8 - 1) = 24 (24^2.2
    /// = 1,093 linear, finite and in range). A pixel reaches it only with less fog: the additive
    /// rule is whole up to the fog blend's start F0, 1 + F0 x 23; fully fogged white and an HDR sky
    /// (30,000) are bright already and come back as they were (before the bright hold the first gave
    /// 24 and the second was held at 60,000); an absurd fog colour and an infinite pixel are held at
    /// 60,000.
    #[test]
    fn the_extreme_tune_stays_finite_and_in_range() {
        use crate::medium_air::{applied_scale, blended_correction, compose_blended, effective_share, phase_floor, AirTune, AIR_BRIGHT_TO, AIR_FOG_BLEND_FROM, AIR_HDR_MAX};
        let tune = AirTune::parse("G=0.9\nCAP=100\nLIFT=3\nREF=0.25\nSHARE=1\nSTRENGTH=1\nSKY=2\nSKYPHASE=1", &AirTune::default());
        assert_eq!((tune.g, tune.cap, tune.lift, tune.reference, tune.sky_weight), (0.9, 8.0, 3.0, Some(0.25), 1.0));
        let phase = tune.phase().normed();
        assert!((phase.most() - 8.0).abs() < 1e-4, "{}", phase.most());
        let px = PixelMirror {
            ray_len: 1.0,
            p_eff: phase_floor(phase.phase(1.0), tune.floor),
            v_ref: 0.25,
            rays: tune.rays,
            lift: tune.lift,
            shade: tune.shade,
            a: applied_scale(tune.strength, effective_share(0.3, tune.share)),
            sky: tune.sky_weight,
            sky_phase: tune.sky_phase,
            fog: AirFog { near: 0.0, far: 1_000.0, power: 1.0, strength: 1.0 },
            src: Vec3::ONE,
            fog_colour: Vec3::ONE,
            ..pixel_inputs()
        };
        assert_eq!(px.a, 1.0);
        let lit = |_: f32| [0.4f32, 0.4];
        // The form itself at its most (F_near 1, t 0, budget C' 1): 24 at strength 1, 47 at the
        // slider's 200 % (4,786 linear): inside the range.
        for (k, peak) in [(1.0f32, 24.0f32), (2.0, 47.0)] {
            let c = blended_correction(3.0, px.p_eff, px.sky_phase, applied_scale(k, 1.0), 1.0, px.sky, 0.0);
            let most = compose_blended(Vec3::ONE, c, Vec3::ONE, 0.0, Some(1.0));
            let expect = peak.powf(2.2);
            assert!(most.is_finite() && (most.x - expect).abs() <= 2e-4 * expect && expect < AIR_HDR_MAX, "{k}: {most} vs {expect}");
        }
        // The pixel: the additive rule whole (a share at the fog blend's start, t 0).
        let thin = AirFog { strength: AIR_FOG_BLEND_FROM, ..px.fog };
        for k in [1.0f32, 2.0] {
            let out = pixel_mirror(&PixelMirror { a: applied_scale(k, 1.0), fog: thin, ..px }, &[Some(5_000.0)], &lit);
            let expect = (1.0 + k * AIR_FOG_BLEND_FROM * (3.0 * px.p_eff - 1.0)).powf(2.2);
            assert!(out.is_finite() && (out.x - expect).abs() <= 1e-4 * expect, "{k}: {out} vs {expect}");
            // Fully fogged (t 1), white, and the sky on a 30,000 sun disc (SKYPHASE 1: a factor of
            // 24 or 47 without the hold): already bright, so as they were.
            let full = PixelMirror { a: applied_scale(k, 1.0), ..px };
            let white = pixel_mirror(&full, &[Some(5_000.0)], &lit);
            assert!(white.w == 0.0 && (white.truncate() - Vec3::ONE).abs().max_element() < 1e-5, "{k}: {white}");
            let disc = pixel_mirror(&PixelMirror { src: Vec3::splat(30_000.0), ..full }, &[None], &lit);
            assert!(disc.w == 0.0 && (disc.x - 30_000.0).abs() < 3.0, "{k}: {disc}");
            // A black sky stays black, a dim one is lifted, by no more than the bright hold's end.
            assert_eq!(pixel_mirror(&PixelMirror { src: Vec3::ZERO, ..full }, &[None], &lit).truncate(), Vec3::ZERO);
            let dim = pixel_mirror(&PixelMirror { src: Vec3::splat(0.01), ..full }, &[None], &lit).x.powf(1.0 / 2.2);
            assert!(dim > 0.5 && dim <= AIR_BRIGHT_TO + 1e-5, "{k}: {dim}");
            // Above the range, an infinity too: held, no NaN, lit or dark.
            for src in [65_000.0f32, f32::INFINITY] {
                for s in [None, Some(5_000.0), Some(200.0)] {
                    for lookup in [[0.4f32, 0.4], [0.0, 0.4]] {
                        let out = pixel_mirror(&PixelMirror { src: Vec3::splat(src), ..full }, &[s], &|_| lookup);
                        assert!(out.is_finite() && out.max_element() <= AIR_HDR_MAX, "{src} {s:?}: {out}");
                        // No number in a channel, or in all: as that channel black.
                        for (bad, black) in [(Vec3::NAN, Vec3::ZERO), (Vec3::new(0.2, f32::NAN, src), Vec3::new(0.2, 0.0, src))] {
                            let out = pixel_mirror(&PixelMirror { src: bad, ..full }, &[s], &|_| lookup);
                            assert!(out.is_finite() && out == pixel_mirror(&PixelMirror { src: black, ..full }, &[s], &|_| lookup), "{bad} {s:?}: {out}");
                        }
                    }
                }
            }
        }
        // A fog colour no weather has (a mod's): held too, where it is the budget (t 0); fully
        // fogged, the budget is the pixel's own colour.
        let covered = pixel_mirror(&PixelMirror { fog_colour: Vec3::splat(1e6), ..px }, &[Some(5_000.0)], &lit);
        assert!((covered.truncate() - Vec3::ONE).abs().max_element() < 1e-5, "{covered}");
        let wild = pixel_mirror(&PixelMirror { fog_colour: Vec3::splat(1e6), fog: thin, ..px }, &[Some(5_000.0)], &lit);
        assert!(wild.is_finite() && wild.truncate() == Vec3::splat(AIR_HDR_MAX), "{wild}");
    }

    /// Samples share the volume lookup, never their fog. (a) Two world samples one
    /// unit apart across a whole fog ramp (near 15,000, far 15,001: F 0 and 1), one lookup
    /// (well within a quarter slice, about 257 units there) with Vbar exactly 0.5 (N = M / 2:
    /// halving is exact): vc = 1 + (0.5 - 1) = 0.5, the far sample's corr = 1 x 1 x (0.5 - 1) =
    /// -0.5, the near one's 0; the far sample is fully fogged, so its t is 1 and its budget Y' 0.8
    /// (C' 0.6 before the fog blend): 0.8 - 0.5 x 0.8 = 0.4, above its floor 0.8 - 0.6; the pixel
    /// is (0.8^2.2 + 0.4^2.2) / 2 =
    /// (0.612066 + 0.133209) / 2 = 0.372637 whatever the order (sample 0 alone gave 0.612 or
    /// 0.133). (b) Samples at 20,000 and 150,000 beyond a 12,000 grid with SKY 0: one lookup,
    /// one F (the grid's end's), but t 0 and 0.960857 (x = 55,500 / 63,000 = 0.880952,
    /// x^2 (3 - 2x); each joined with the fog blend of F 0.18), so the far sample keeps 1 - t of its correction on a budget mostly its own
    /// colour.
    #[test]
    fn msaa_samples_take_their_own_fog_and_far_blend() {
        let lookups = Cell::new(0u32);
        let half = |_: f32| {
            lookups.set(lookups.get() + 1);
            [0.25f32, 0.5]
        };
        let ramp = PixelMirror {
            ray_len: 1.0,
            p_eff: 1.0,
            v_ref: 1.0,
            rays: 1.0,
            shade: 0.0,
            a: 1.0,
            sky: 1.0,
            sky_phase: 0.0,
            fog: AirFog { near: 15_000.0, far: 15_001.0, power: 1.0, strength: 1.0 },
            src: Vec3::splat(0.8f32.powf(2.2)),
            fog_colour: Vec3::splat(0.6f32.powf(2.2)),
            ..pixel_inputs()
        };
        let forward = pixel_mirror(&ramp, &[Some(15_000.0), Some(15_001.0)], &half);
        assert_eq!(lookups.replace(0), 1, "one shared lookup");
        let backward = pixel_mirror(&ramp, &[Some(15_001.0), Some(15_000.0)], &half);
        assert_eq!(lookups.replace(0), 1);
        let by_hand = (0.8f32.powf(2.2) + 0.4f32.powf(2.2)) / 2.0;
        assert!((by_hand - 0.372637).abs() < 1e-5, "{by_hand}");
        for out in [forward, backward] {
            assert!((out.truncate() - Vec3::splat(by_hand)).abs().max_element() < 1e-6, "{out} vs {by_hand}");
            assert!((out.w + 0.25).abs() < 1e-6, "{out}");
        }
        // (b) Beyond a 12,000 grid, SKY 0.
        let sf = crate::sky::SKY_DEPTH_FROM;
        assert!((sf - 157_500.0).abs() < 1.0, "{sf}");
        let far = PixelMirror {
            ray_len: 1.0,
            p_eff: 1.0,
            v_ref: 1.0,
            rays: 1.0,
            shade: 0.0,
            a: 1.0,
            sky: 0.0,
            sky_phase: 0.0,
            curve: AirGrid::for_view(16.0 / 9.0, None, Some(12_000.0)).curve(),
            ..pixel_inputs()
        };
        let forward = pixel_mirror(&far, &[Some(20_000.0), Some(150_000.0)], &half);
        assert_eq!(lookups.replace(0), 1, "both read the grid's end");
        let backward = pixel_mirror(&far, &[Some(150_000.0), Some(20_000.0)], &half);
        assert_eq!(lookups.replace(0), 1);
        // By hand: F = (12,000 / 200,000)^0.6; corr_i = F (0.5 - 1) (1 - t_i); the display
        // value Y' + corr_i ((1 - t_i) C' + t_i Y').
        let f = (12_000.0f32 / 200_000.0).powf(0.6);
        let x = (150_000.0 - 0.6 * sf) / (0.4 * sf);
        let t2 = x * x * (3.0 - 2.0 * x);
        assert!((t2 - 0.960857).abs() < 1e-4, "{t2}");
        let y = far.src.powf(1.0 / 2.2);
        let c = far.fog_colour.powf(1.0 / 2.2);
        let g = crate::medium_air::fog_blend(f);
        let one = |t_d: f32| {
            let t = t_d + (1.0 - t_d) * g;
            let corr = f * (0.5 - 1.0) * (1.0 - t);
            (y + corr * (c * (1.0 - t) + y * t)).powf(2.2)
        };
        let expect = (one(0.0) + one(t2)) / 2.0;
        for out in [forward, backward] {
            assert!((out.truncate() - expect).abs().max_element() < 1e-6, "{out} vs {expect}");
        }
        // Sample 0 alone (the old sharing) would have given either sample's colour.
        assert!((forward.truncate() - one(0.0)).abs().max_element() > 1e-3 && (forward.truncate() - one(t2)).abs().max_element() > 1e-3);
        // Depths a silhouette apart look up each sample.
        pixel_mirror(&far, &[Some(300.0), Some(900.0), Some(300.0), Some(300.0)], &half);
        assert_eq!(lookups.replace(0), 4);
    }

    /// Sharing a lookup must not depend on the samples' order.
    /// Its case: a centred ray, GRID 17,9,64, DEPTH 1,000, fog from 500 to 501, all air in
    /// shadow, samples at 499.5 and 501 (within a quarter slice: one lookup). The one lookup is at
    /// the farther depth, whose sums hold the fog: the fogged sample (F_near 1) is shadowed,
    /// vc = clamp(1 + (0 - 1), SHADE 0, LIFT) = 0, corr = -1, t 1 (fully fogged), the change
    /// -1 x Y' 0.8 held to -F C' = -0.6: 0.8 - 0.6 = 0.2; the
    /// other (F_near 0) keeps 0.8; the pixel is (0.8^2.2 + 0.2^2.2) / 2 in either order.
    #[test]
    fn msaa_sharing_does_not_depend_on_sample_order() {
        use crate::medium_air::{column_nm, slices_agree, sums_at, AIR_AGREE_SLICES};
        let g = AirGrid::for_view(16.0 / 9.0, Some([17, 9, 64]), Some(1_000.0));
        assert_eq!((g.nx, g.ny, g.nz, g.depth), (17, 9, 64, 1_000.0));
        let fog = AirFog { near: 500.0, far: 501.0, power: 1.0, strength: 1.0 };
        let nm = column_nm(&g, 1.0, fog, &|_| 0.0);
        let curve = g.curve();
        let lookups = Cell::new(0u32);
        let lookup = |z: f32| {
            lookups.set(lookups.get() + 1);
            sums_at(&nm, curve.coord(z))
        };
        assert!(slices_agree(&[499.5, 501.0], &curve, AIR_AGREE_SLICES), "the two depths share one lookup");
        let px = PixelMirror {
            ray_len: 1.0,
            p_eff: 1.0,
            v_ref: 1.0,
            rays: 1.0,
            shade: 0.0,
            a: 1.0,
            sky: 1.0,
            sky_phase: 0.0,
            fog,
            curve,
            src: Vec3::splat(0.8f32.powf(2.2)),
            fog_colour: Vec3::splat(0.6f32.powf(2.2)),
            ..pixel_inputs()
        };
        let forward = pixel_mirror(&px, &[Some(499.5), Some(501.0)], &lookup);
        assert_eq!(lookups.replace(0), 1);
        let backward = pixel_mirror(&px, &[Some(501.0), Some(499.5)], &lookup);
        assert_eq!(lookups.replace(0), 1);
        assert_eq!(forward, backward, "the order of the samples does not matter");
        let by_hand = (0.8f32.powf(2.2) + 0.2f32.powf(2.2)) / 2.0;
        assert!((forward.truncate() - Vec3::splat(by_hand)).abs().max_element() < 1e-5, "{forward} vs {by_hand}");
        assert!((forward.w + 0.5).abs() < 1e-6, "the fogged sample is shadowed (corr -1): {forward}");
    }

    /// Geometry keeps its surface light through the whole pixel: far opaque terrain (t 1, beyond
    /// the sky's distance) in a fog that gives it f = F_near = 0.5 (strength 0.5 reached within
    /// 1,000 units), a white surface in black fog (Y' 0.5), all air in shadow, SKY 1: the pixel
    /// keeps its surface light 0.5 at SHADE 0 and 0.5; cleared depth there still darkens.
    #[test]
    fn far_terrain_keeps_its_surface_light_through_the_pixel() {
        for shade in [0.0f32, 0.5] {
            let px = PixelMirror {
                ray_len: 1.0,
                p_eff: 1.0,
                v_ref: 1.0,
                rays: 8.0,
                shade,
                a: 1.0,
                sky: 1.0,
                sky_phase: 0.0,
                fog: AirFog { near: 0.0, far: 1_000.0, power: 1.0, strength: 0.5 },
                src: Vec3::splat(0.5f32.powf(2.2)),
                fog_colour: Vec3::ZERO,
                ..pixel_inputs()
            };
            let dark = |_: f32| [0.0f32, 0.4];
            let far = pixel_mirror(&px, &[Some(200_000.0)], &dark);
            assert!((far.truncate().powf(1.0 / 2.2) - Vec3::splat(0.5)).abs().max_element() < 1e-6, "SHADE {shade}: {far}");
            let sky = pixel_mirror(&px, &[None], &dark);
            assert!(sky.x < far.x, "SHADE {shade}: the sky still darkens: {sky} vs {far}");
            // The depth-writing dome (the known limit, module doc): at its distance (about
            // 350,000 at the horizon) it is a sample with depth and takes the terrain's bound,
            // the same result as far terrain, not cleared depth's.
            let dome = pixel_mirror(&px, &[Some(350_000.0)], &dark);
            assert_eq!(dome, far, "SHADE {shade}: the dome is held as far terrain is");
            assert!(dome.x > sky.x);
        }
    }

    /// The pixel as a whole: the mean does not depend on the samples' order; near world samples
    /// keep the fogged floor (1 - f) S' (F_near <= f, corr_t >= -F_near on the budget C'); a
    /// cleared sample under all-lit average air at SKYPHASE 0 is the pixel as it was; one sample
    /// is that sample's result.
    #[test]
    fn the_pixel_mirror_keeps_the_floor_and_averages() {
        use crate::medium_air::{column_nm, fog_share, sums_at};
        let px = PixelMirror { sky: 1.0, ..pixel_inputs() };
        let g = AirGrid::for_view(16.0 / 9.0, None, None);
        let nm = column_nm(&g, px.ray_len, px.fog, &|k| if k < 30 { 0.0 } else { 1.0 });
        let curve = px.curve;
        let lookup = |z: f32| sums_at(&nm, curve.coord(z));
        let samples = [Some(300.0f32), Some(9_000.0), None, Some(200_000.0)];
        let mean = pixel_mirror(&px, &samples, &lookup);
        for order in [[3usize, 2, 1, 0], [1, 3, 0, 2], [2, 0, 3, 1]] {
            let shuffled: Vec<Option<f32>> = order.iter().map(|&i| samples[i]).collect();
            let other = pixel_mirror(&px, &shuffled, &lookup);
            assert!((other - mean).abs().max_element() < 1e-6, "{order:?}: {other} vs {mean}");
        }
        // The fogged floor for near world samples, any surface, dark or lit air.
        for s in [0.0f32, 0.5, 4.0] {
            for z in [300.0f32, 2_000.0] {
                let f = fog_share(z * px.ray_len, px.fog);
                let yd = Vec3::splat((1.0 - f) * s.powf(1.0 / 2.2)) + px.fog_colour.powf(1.0 / 2.2) * f;
                for dark in [true, false] {
                    let out = pixel_mirror(&PixelMirror { src: yd.powf(2.2), ..px }, &[Some(z)], &|zz: f32| if dark { [0.0, 0.3] } else { lookup(zz) });
                    let shown = out.truncate().powf(1.0 / 2.2);
                    assert!(shown.min_element() >= (1.0 - f) * s.powf(1.0 / 2.2) - 1e-5 * yd.max_element().max(1.0), "{s} {z} {dark}: {shown}");
                }
            }
        }
        // All lit, V_ref 1, SKYPHASE 0: vc 1, corr 0 on cleared depth, the pixel back after
        // one round trip of the powers.
        let calm = PixelMirror { v_ref: 1.0, sky_phase: 0.0, ..px };
        let same = pixel_mirror(&calm, &[None], &|_| [0.5, 0.5]);
        assert!((same.truncate() - px.src).abs().max_element() < 1e-6 && same.w == 0.0, "{same}");
        assert_eq!(pixel_mirror(&px, &[], &lookup), Vec4::ZERO);
    }

    #[test]
    fn the_light_views_draw_only_through_the_gate() {
        use crate::medium_air::AirView;
        assert!(draws(AirView::None) && draws(AirView::Delta) && draws(AirView::Rays) && draws(AirView::Lit) && draws(AirView::Visibility) && draws(AirView::Slice) && draws(AirView::Share));
        assert!(shows_light(AirView::Rays), "rays: what the light applies, gated");
        assert!(!shows_light(AirView::Lit), "lit: a debug view, not gated");
        assert!(!draws(AirView::Off), "off: the grid alone, the target not flipped");
        assert!(shows_light(AirView::None) && shows_light(AirView::Delta) && !shows_light(AirView::Share));
        let on = crate::medium_air_grid::AirGpuUniform { light: Vec4::new(0.0, 1.0, 0.0, 1.0), fog_colour: Vec4::new(0.5, 0.5, 0.5, 1.0), ..default() };
        assert!(light_on(&on));
        // light.w carries 2 - FLOOR while the light draws: FLOOR 0 reads 2, still on.
        assert!(light_on(&crate::medium_air_grid::AirGpuUniform { light: Vec4::new(0.0, 1.0, 0.0, crate::medium_air::light_flag(true, 0.0)), ..on }));
        assert!(!light_on(&crate::medium_air_grid::AirGpuUniform { light: Vec4::new(0.0, 1.0, 0.0, 0.0), ..on }));
        assert!(!light_on(&crate::medium_air_grid::AirGpuUniform { fog_colour: Vec4::new(0.5, 0.5, 0.5, 0.0), ..on }));
    }
}
