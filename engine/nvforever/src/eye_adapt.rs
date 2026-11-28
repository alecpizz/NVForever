//! Eye adaptation (docs/spec-notes/world.md §6 "Eye adaptation"). The original's HDR
//! pass lets "the adapted scene lum chase the actual lum" toward the image space's Target
//! LUM [documented: GECK wiki ImageSpace, HDR tab]; how it maps one to the other is not
//! documented. Against 31 original frames (the Mojave by day, the lab's interiors, the
//! effects audit's night spots and Dead Money), pictures whose average is above about
//! −7 EV match ours without it, and darker ones are brightened by most of the way back
//! [observed, world.md §6]. The renderer meters the HDR view with Bevy's auto exposure and
//! brightens only: lift = [`LIFT_SLOPE`] × (reference − average log2 luminance), never
//! below 0 or above [`MAX_LIFT`], the reference being [`REFERENCE_EV`] + log2(Target LUM /
//! 1.4) [hypothesis, fitted]. It needs the HDR view, so it goes with the light glow.
//!
//! The sky counts for little in the meter ([`SKY_WEIGHT`], a mask drawn each frame from
//! the depth prepass, `eye_mask.wesl`) [hypothesis, 2026-10-23: the original's clear
//! Mojave night at 23:00, a nearly black sky over most of the picture, is not brightened
//! (the windmill and buildings match ours without adaptation), while metering the sky
//! with the world lifted those views by several EV once our night sky was as dark as the
//! original's; world.md §6 "Eye adaptation"].
//!
//! How fast the view adapts follows the image space's Eye Adapt Speed ([`adapt_speed`]),
//! and an image space modifier that changes Target LUM (the poison stings' doubling, the
//! Big MT's X-8 experiment) shifts the whole picture's exposure by that change
//! ([`modifier_ev`]); both [hypothesis], docs/spec-notes/image-modifiers.md §11.

use bevy::asset::RenderAssetUsages;
use bevy::core_pipeline::prepass::ViewPrepassTextures;
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::curve::cubic_splines::LinearSpline;
use bevy::platform::collections::HashMap;
use bevy::post_process::auto_exposure::{AutoExposure, AutoExposureCompensationCurve, AutoExposurePlugin};
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{texture_depth_2d, texture_depth_2d_multisampled, uniform_buffer_sized};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, Buffer, BufferInitDescriptor, BufferUsages, CachedRenderPipelineId,
    ColorTargetState, ColorWrites, Extent3d, FragmentState, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipelineDescriptor, ShaderStages, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::texture::GpuImage;
use bevy::render::view::ExtractedView;
use bevy::render::{RenderApp, RenderStartup};
use fnv_world::imagespace::{HDR_EYE_ADAPT_SPEED, HDR_TARGET_LUM};

/// The average log2 luminance below which a picture is brightened, at Target LUM 1.4
/// (NVDefaultExterior) [hypothesis, fitted to the frames].
pub const REFERENCE_EV: f32 = -6.8;
/// EV of brightening per EV the average lies under the reference [hypothesis, fitted].
pub const LIFT_SLOPE: f32 = 0.85;
/// The most a picture is brightened, in EV [engineering choice: a black screen stays dark].
pub const MAX_LIFT: f32 = 5.0;
/// Target LUM of NVDefaultExterior, where [`REFERENCE_EV`] was fitted [observed].
const BASE_TARGET_LUM: f32 = 1.4;
/// The metered range of log2 luminance [engineering choice: the darkest frames average
/// about −12 EV, sunlit sand about 0].
const METER_LOW: f32 = -16.0;
const METER_HIGH: f32 = 6.0;
/// The share of the metered histogram (darkest to brightest) the average is taken over:
/// the darkest tenth left out, the brightest kept [hypothesis, 2026-10-25: with Bevy's
/// default 10-90 % the Strip at night, Target LUM 4, was lifted about 0.3 EV over the
/// original's frames (owner's and fx_strip_b) though without adaptation it matched within
/// 5 %; counting the brightest tenth, where its neon and lamps lie, it is not lifted;
/// the original's meter may be a plain average, which bright lights hold up (module note)].
pub const METER_FILTER: std::ops::RangeInclusive<f32> = 0.10..=1.0;
/// Bi-directional camera auto-exposure parameters for Physical lighting.
pub const PHYSICAL_LOW_REF: f32 = -6.0;
pub const PHYSICAL_HIGH_REF: f32 = -1.2;
pub const PHYSICAL_LIFT_SLOPE: f32 = 0.68;
pub const PHYSICAL_MAX_LIFT: f32 = 2.6;
pub const PHYSICAL_DARKEN_SLOPE: f32 = 0.45;
pub const PHYSICAL_MAX_DARKEN: f32 = 1.4;
pub const PHYSICAL_METER_FILTER: std::ops::RangeInclusive<f32> = 0.10..=0.98;
/// The sky's weight in the meter, against 1 for the world [hypothesis, see the module
/// note; not 0, so a view of nothing but sky still has a reading]. The mask keeps 16
/// levels (Bevy's auto exposure), so 1/16 is the smallest weight above none.
pub const SKY_WEIGHT: f32 = 1.0 / 16.0;
/// EV per second the view adapts at under the image spaces' common Eye Adapt Speed 0.9
/// (63 of the masters' 91 IMGS, every Mojave one) [engineering choice: brightening as
/// quickly as darkening, both within a couple of seconds; the speed before Eye Adapt
/// Speed was read].
pub const ADAPT_SPEED: f32 = 3.0;
/// The common Eye Adapt Speed, where [`ADAPT_SPEED`] holds [observed: the masters' IMGS].
pub const BASE_EYE_ADAPT_SPEED: f32 = 0.9;
/// The fastest adaptation, EV per second: Eye Adapt Speed 0 ("the adapted lum always
/// equals the actual lum" [documented: GECK wiki ImageSpace, HDR tab]) [engineering
/// choice: about at once].
pub const INSTANT_ADAPT_SPEED: f32 = 40.0;
/// The slowest adaptation, EV per second [engineering choice: so a view never stays
/// wrongly exposed for good].
pub const SLOWEST_ADAPT_SPEED: f32 = 0.25;
/// The most a modifier's Target LUM change shifts the exposure, either way, in EV
/// [engineering choice].
pub const MAX_MODIFIER_EV: f32 = 4.0;

/// The adaptation speed, EV per second, for an Eye Adapt Speed: in proportion to it,
/// [`ADAPT_SPEED`] at [`BASE_EYE_ADAPT_SPEED`] ("higher numbers give faster chasing"),
/// and at once at 0 or below ("0.0 causes the adapted lum to always equal the actual
/// lum") [documented: GECK wiki ImageSpace, HDR tab; hypothesis: the proportion. The
/// page's third sentence, "1.0 causes the adapted lum to never change", does not fit the
/// first and is not followed: image spaces store up to 8, and NVCaveEnteringISFX halves
/// the speed as the player enters a cave, so the dark lasts].
pub fn adapt_speed(eye_adapt_speed: f32) -> f32 {
    if !eye_adapt_speed.is_finite() {
        return ADAPT_SPEED;
    }
    if eye_adapt_speed <= 0.0 {
        return INSTANT_ADAPT_SPEED;
    }
    // The ratio first, so the base speed gives ADAPT_SPEED exactly (3 × 0.9 / 0.9 rounds to
    // 2.9999998 in f32).
    (ADAPT_SPEED * (eye_adapt_speed / BASE_EYE_ADAPT_SPEED)).clamp(SLOWEST_ADAPT_SPEED, INSTANT_ADAPT_SPEED)
}

/// The exposure change, in EV, of image space modifiers that multiply Target LUM by
/// `ratio`: log2 of it, held to [`MAX_MODIFIER_EV`] either way [hypothesis: Target LUM is
/// the luminance the original's adaptation aims the picture at, so twice the target is
/// twice as bright; the image spaces' own Target LUM keeps its fitted role,
/// [`reference_ev`]].
pub fn modifier_ev(ratio: f32) -> f32 {
    if ratio.is_nan() || ratio <= 0.0 || !ratio.is_finite() {
        return 0.0;
    }
    ratio.log2().clamp(-MAX_MODIFIER_EV, MAX_MODIFIER_EV)
}

/// Size of the metering mask in pixels [engineering choice: coarse is enough for an
/// average].
const MASK_SIZE: UVec2 = UVec2::new(160, 90);

pub struct EyeAdaptPlugin;

impl Plugin for EyeAdaptPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "eye_mask.wesl");
        app.add_plugins(AutoExposurePlugin).init_resource::<EyeCurve>().add_systems(PostUpdate, follow_target_lum);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else { return };
        // Before the post-processing, where Bevy's auto exposure reads the mask.
        render_app.add_systems(RenderStartup, init_mask_pipelines).add_systems(
            Core3d,
            // FSR restores the depth prepass to display resolution before this
            // mask meters the complete screen.
            mask_pass.in_set(Core3dSystems::EarlyPostProcess).after(crate::fsr::fsr_pass),
        );
    }
}

/// The compensation curve every adapting camera shares, the Target LUM it was made for,
/// and the metering mask (drawn over each frame by [`mask_pass`]; full weight until then).
#[derive(Resource)]
pub struct EyeCurve {
    pub handle: Handle<AutoExposureCompensationCurve>,
    pub mask: Handle<Image>,
    target: f32,
    /// The modifiers' exposure change the curve was made with ([`modifier_ev`]).
    extra: f32,
    physical: bool,
}

impl FromWorld for EyeCurve {
    fn from_world(world: &mut World) -> Self {
        let physical = nvf_assets::authored::physical_pbr();
        let handle = world.resource_mut::<Assets<AutoExposureCompensationCurve>>().add(curve_for(BASE_TARGET_LUM, 0.0, physical));
        let size = Extent3d { width: MASK_SIZE.x, height: MASK_SIZE.y, depth_or_array_layers: 1 };
        let mut image = Image::new_fill(size, TextureDimension::D2, &[255], TextureFormat::R8Unorm, RenderAssetUsages::RENDER_WORLD);
        image.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT;
        let mask = world.resource_mut::<Assets<Image>>().add(image);
        EyeCurve { handle, mask, target: BASE_TARGET_LUM, extra: 0.0, physical }
    }
}

/// The reference average (log2 luminance) for an image space's Target LUM; a missing or
/// non-positive value counts as the base one.
pub fn reference_ev(target_lum: f32) -> f32 {
    let t = if target_lum > 0.0 { target_lum } else { BASE_TARGET_LUM };
    REFERENCE_EV + (t / BASE_TARGET_LUM).log2()
}

/// How many EV a picture of average log2 luminance `avg` is brightened.
pub fn lift(avg: f32, target_lum: f32) -> f32 {
    (LIFT_SLOPE * (reference_ev(target_lum) - avg)).clamp(0.0, MAX_LIFT)
}

/// How many EV a picture of average log2 luminance `avg` is exposed under physical PBR:
/// lifts dim scenes below `PHYSICAL_LOW_REF + shift` and stops down bright daylight scenes
/// above `PHYSICAL_HIGH_REF + shift`, with a stable deadband between them.
pub fn physical_exposure(avg: f32, target_lum: f32) -> f32 {
    let t = if target_lum > 0.0 { target_lum } else { BASE_TARGET_LUM };
    let shift = (t / BASE_TARGET_LUM).log2();
    let low = PHYSICAL_LOW_REF + shift;
    let high = PHYSICAL_HIGH_REF + shift;
    if avg < low {
        (PHYSICAL_LIFT_SLOPE * (low - avg)).clamp(0.0, PHYSICAL_MAX_LIFT)
    } else if avg > high {
        (PHYSICAL_DARKEN_SLOPE * (high - avg)).clamp(-PHYSICAL_MAX_DARKEN, 0.0)
    } else {
        0.0
    }
}

/// Bevy's curve: for an average `x` the target exposure is `y − x`, so `y = x + lift(x)`,
/// raised by the modifiers' `extra` EV everywhere. Corners: the meter's ends, the
/// reference(s), and where the lift/darken slopes reach their caps.
fn curve_for(target_lum: f32, extra: f32, physical: bool) -> AutoExposureCompensationCurve {
    let mut xs = if physical {
        let t = if target_lum > 0.0 { target_lum } else { BASE_TARGET_LUM };
        let shift = (t / BASE_TARGET_LUM).log2();
        let low = PHYSICAL_LOW_REF + shift;
        let high = PHYSICAL_HIGH_REF + shift;
        vec![
            METER_LOW,
            low - PHYSICAL_MAX_LIFT / PHYSICAL_LIFT_SLOPE,
            low,
            high,
            high + PHYSICAL_MAX_DARKEN / PHYSICAL_DARKEN_SLOPE,
            METER_HIGH,
        ]
    } else {
        let r = reference_ev(target_lum);
        let cap = r - MAX_LIFT / LIFT_SLOPE;
        vec![METER_LOW, cap, r, METER_HIGH]
    };
    xs.sort_by(f32::total_cmp);
    xs.retain(|x| (METER_LOW..=METER_HIGH).contains(x));
    xs.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
    let points: Vec<Vec2> = xs
        .into_iter()
        .map(|x| {
            let xq = (x * 256.0).round() / 256.0;
            let exp = if physical { physical_exposure(xq, target_lum) } else { lift(xq, target_lum) };
            let yq = ((xq + exp + extra) * 256.0).round() / 256.0;
            Vec2::new(xq, yq)
        })
        .collect();
    AutoExposureCompensationCurve::from_curve(LinearSpline::new(points)).unwrap_or_default()
}

#[cfg(test)]
fn curve(target_lum: f32, extra: f32) -> AutoExposureCompensationCurve {
    curve_for(target_lum, extra, false)
}

/// The camera component for an adapting view (inserted with the light glow, graphics.rs).
pub fn auto_exposure(eye: &EyeCurve) -> AutoExposure {
    let physical = nvf_assets::authored::physical_pbr();
    AutoExposure {
        range: METER_LOW..=METER_HIGH,
        // Brightening as quickly as darkening (`follow_target_lum` sets them from the
        // image space's Eye Adapt Speed).
        speed_brighten: ADAPT_SPEED,
        speed_darken: ADAPT_SPEED,
        compensation_curve: eye.handle.clone(),
        metering_mask: eye.mask.clone(),
        filter: if physical { PHYSICAL_METER_FILTER } else { METER_FILTER },
        ..default()
    }
}

/// Keep the curve on the image space's Target LUM and the modifiers' change to it, and
/// the cameras' speed on its Eye Adapt Speed (modifiers included). The cameras get the
/// adaptation with the HDR view (graphics.rs `apply`), in the same command, so no frame
/// has one without the other.
fn follow_target_lum(
    sky: Option<Res<crate::sky::SkyState>>,
    screen: Option<Res<crate::imagespace::ScreenModifiers>>,
    mut eye: ResMut<EyeCurve>,
    mut curves: ResMut<Assets<AutoExposureCompensationCurve>>,
    mut cameras: Query<&mut AutoExposure>,
    mut told: Local<(bool, f32, f32)>,
) {
    let hdr = screen.as_deref().and_then(|s| s.graded_hdr()).or_else(|| sky.as_deref().map(|s| s.cinematic().hdr));
    let physical = nvf_assets::authored::physical_pbr();
    let mut target = sky.as_deref().map_or(BASE_TARGET_LUM, |s| s.cinematic().hdr.v[HDR_TARGET_LUM]);
    if physical && sky.as_deref().is_some_and(|s| s.interior.is_some()) && target > 0.0 {
        target = target.min(BASE_TARGET_LUM);
    }
    let extra = modifier_ev(screen.as_deref().map_or(1.0, |s| s.hdr_ratio(HDR_TARGET_LUM)));
    if (eye.target - target).abs() > 1e-3 || (eye.extra - extra).abs() > 1e-3 || eye.physical != physical || !told.0 {
        // A modifier's change is logged in quarter-EV steps, not every frame.
        let step = (extra * 4.0).round() / 4.0;
        if (eye.target - target).abs() > 1e-3 || !told.0 || step != told.1 {
            info!("eye adaptation: Target LUM {target:.2}, brightens pictures averaging under {:.1} EV, modifiers {step:+.2} EV", reference_ev(target));
        }
        told.0 = true;
        told.1 = step;
        eye.target = target;
        eye.extra = extra;
        eye.physical = physical;
        let _ = curves.insert(&eye.handle, curve_for(target, extra, physical));
    }
    let eye_speed = match (sky.as_deref().map(|s| s.grade), hdr) {
        // With Colour grade off (`SkyState::grade == false`), `sky.cinematic()` returns
        // `Cinematic::default()` whose `HDR_EYE_ADAPT_SPEED` is 0; keep the base speed
        // rather than treating disabled grading as an IMGS requesting instant adaptation
        // (#3917) [design].
        (Some(false), _) => BASE_EYE_ADAPT_SPEED,
        (_, Some(h)) => h.v[HDR_EYE_ADAPT_SPEED],
        (_, None) => BASE_EYE_ADAPT_SPEED,
    };
    let speed = adapt_speed(eye_speed);
    if (told.2 - speed).abs() > 1e-3 {
        info!("eye adaptation: speed {speed:.2} EV/s (Eye Adapt Speed {eye_speed:.2})");
        told.2 = speed;
    }
    for mut a in &mut cameras {
        if a.speed_brighten != speed || a.speed_darken != speed {
            a.speed_brighten = speed;
            a.speed_darken = speed;
        }
    }
}

/// The mask pass's layouts (single-sampled and multisampled depth) and pipelines.
#[derive(Resource)]
struct MaskPipelines {
    layouts: [BindGroupLayoutDescriptor; 2],
    base: RenderPipelineDescriptor,
    ids: HashMap<bool, CachedRenderPipelineId>,
    /// The parameters' uniform buffer and the values in it: made again only when they
    /// change. A new buffer every frame was GPU memory made and freed every frame, one of
    /// the per-frame allocations behind the night stalls (#897).
    params: Option<([f32; 4], Buffer)>,
}

fn init_mask_pipelines(mut commands: Commands, assets: Res<AssetServer>, fullscreen: Res<FullscreenShader>) {
    let layout = |multi: bool| {
        let depth = if multi { texture_depth_2d_multisampled() } else { texture_depth_2d() };
        BindGroupLayoutDescriptor::new(
            "eye_mask_layout",
            &BindGroupLayoutEntries::sequential(ShaderStages::FRAGMENT, (depth, uniform_buffer_sized(false, None))),
        )
    };
    let base = RenderPipelineDescriptor {
        label: Some("eye_mask_pipeline".into()),
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: assets.load("embedded://nvforever/eye_mask.wesl"),
            targets: vec![Some(ColorTargetState { format: TextureFormat::R8Unorm, blend: None, write_mask: ColorWrites::ALL })],
            ..default()
        }),
        ..default()
    };
    commands.insert_resource(MaskPipelines { layouts: [layout(false), layout(true)], base, ids: HashMap::default(), params: None });
}

/// Draw the metering mask: per mask pixel the share of the world (not the sky dome) in
/// the depth prepass, weighted between [`SKY_WEIGHT`] and 1.
fn mask_pass(
    view: ViewQuery<(&AutoExposure, &ViewPrepassTextures, &ExtractedView)>,
    mut pipelines: ResMut<MaskPipelines>,
    cache: Res<PipelineCache>,
    images: Res<RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (exposure, prepass, extracted) = view.into_inner();
    let Some(multi) = crate::ssao::depth_multisampled(prepass) else { return };
    let Some(depth) = prepass.depth_only_view() else { return };
    let Some(mask) = images.get(&exposure.metering_mask) else { return };
    let id = match pipelines.ids.get(&multi) {
        Some(id) => *id,
        None => {
            let mut d = pipelines.base.clone();
            d.layout = vec![pipelines.layouts[multi as usize].clone()];
            if multi {
                if let Some(f) = d.fragment.as_mut() {
                    f.shader_defs.push("MULTISAMPLED".into());
                }
            }
            let id = cache.queue_render_pipeline(d);
            pipelines.ids.insert(multi, id);
            id
        }
    };
    let Some(pipeline) = cache.get_render_pipeline(id) else { return };
    // Bevy's infinite reverse-depth projection keeps the near plane here.
    let near = extracted.clip_from_view.w_axis.z;
    let params = [crate::sky::SKY_DEPTH_FROM, near, SKY_WEIGHT, 0.0];
    if pipelines.params.as_ref().is_none_or(|(p, _)| *p != params) {
        let bytes: Vec<u8> = params.iter().flat_map(|v| v.to_le_bytes()).collect();
        let buffer = device.create_buffer_with_data(&BufferInitDescriptor { label: Some("eye_mask_params"), contents: &bytes, usage: BufferUsages::UNIFORM });
        pipelines.params = Some((params, buffer));
    }
    let Some((_, buffer)) = pipelines.params.as_ref() else { return };
    let layout = cache.get_bind_group_layout(&pipelines.layouts[multi as usize]);
    let bg = device.create_bind_group("eye_mask", &layout, &BindGroupEntries::sequential((depth, buffer.as_entire_binding())));
    let mut pass = ctx.command_encoder().begin_render_pass(&RenderPassDescriptor {
        label: Some("eye_mask_pass"),
        color_attachments: &[Some(RenderPassColorAttachment { view: &mask.texture_view, depth_slice: None, resolve_target: None, ops: Operations::default() })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &bg, &[]);
    pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifts_only_dark_pictures() {
        // A Mojave day (about −3 EV) is left alone; a Dead Money night (−9 to −12) is lifted.
        assert_eq!(lift(-3.0, 1.4), 0.0);
        assert!((lift(-9.12, 1.4) - 0.85 * 2.32).abs() < 1e-4);
        assert_eq!(lift(-20.0, 1.4), MAX_LIFT);
        // Puesta del Sol's adapter triples Target LUM: the reference rises by log2 3.
        assert!((reference_ev(4.2) - (REFERENCE_EV + 3f32.log2())).abs() < 1e-5);
        // An image space without HDR values counts as the base one.
        assert_eq!(reference_ev(0.0), REFERENCE_EV);
    }

    /// #2892: the exposure the meter's pass works out for an average (curve − average, as
    /// its shader reads the curve) is the lift, and exactly none above the reference. With
    /// Bevy's own byte table read one entry at a time it was a sawtooth between 0 and
    /// about −0.15 EV there, so a noon picture came out darker by an amount that jumped
    /// with the view.
    #[test]
    fn the_metered_exposure_is_the_lift_and_none_above_the_reference() {
        for (target_lum, extra) in [(1.4, 0.0), (4.2, 0.0), (1.4, 1.0)] {
            let c = curve(target_lum, extra);
            let r = reference_ev(target_lum);
            // One table entry is (6 − −16) / 255 EV wide; the corner at the reference is
            // rounded within the entry it falls in.
            let entry = (METER_HIGH - METER_LOW) / 255.0;
            let mut avg = METER_LOW;
            while avg <= METER_HIGH {
                let exposure = c.compensation(avg) - avg;
                let want = lift(avg, target_lum) + extra;
                if avg >= r + entry {
                    assert!((exposure - extra).abs() < 1e-4, "Target LUM {target_lum}: average {avg} above the reference is exposed by {exposure}");
                }
                assert!((exposure - want).abs() < 0.03, "Target LUM {target_lum}: average {avg}: {exposure}, want {want}");
                avg += 0.013;
            }
        }
    }

    #[test]
    fn speed_follows_eye_adapt_speed() {
        // 0.9 is the base image spaces' own value: the speed is ADAPT_SPEED (to f32 rounding).
        assert!((adapt_speed(0.9) - ADAPT_SPEED).abs() < 1e-5);
        // NVCaveEnteringISFX halves it: slower.
        assert!((adapt_speed(0.45) - ADAPT_SPEED / 2.0).abs() < 1e-5);
        assert_eq!(adapt_speed(0.0), INSTANT_ADAPT_SPEED);
        assert_eq!(adapt_speed(0.01), SLOWEST_ADAPT_SPEED);
        assert_eq!(adapt_speed(100.0), INSTANT_ADAPT_SPEED);
        assert!((adapt_speed(f32::NAN) - ADAPT_SPEED).abs() < 1e-5);
    }

    #[test]
    fn target_lum_change_is_exposure() {
        assert_eq!(modifier_ev(1.0), 0.0);
        assert!((modifier_ev(2.0) - 1.0).abs() < 1e-6);
        assert_eq!(modifier_ev(1000.0), MAX_MODIFIER_EV);
        assert_eq!(modifier_ev(0.0), 0.0);
    }

    #[test]
    fn curve_builds() {
        let c = curve(1.4, 0.0);
        let d = format!("{c:?}");
        assert!(d.contains("min_log_lum: -16.0"), "{d}");
    }

    /// CPU mirror of `auto_exposure.wesl`'s exposure update step (#3917): exact exponential
    /// decay clamped to `delta` so no `(speed, dt)` combination can overshoot or oscillate.
    fn exposure_step(exposure: f32, target: f32, speed_up: f32, speed_down: f32, transition: f32, dt: f32) -> f32 {
        let delta = target - exposure;
        let transition = transition.max(1e-6);
        if target > exposure {
            let sd = (speed_down * dt).max(0.0);
            let ed = sd / transition;
            exposure + delta.min(sd.min(delta * (1.0 - (-ed).exp())))
        } else {
            let su = (speed_up * dt).max(0.0);
            let eu = su / transition;
            exposure + delta.max((-su).max(delta * (1.0 - (-eu).exp())))
        }
    }

    #[test]
    fn auto_exposure_step_never_overshoots_or_oscillates_under_high_speed_or_slow_frames() {
        // Reproduces the #3917 playtest conditions: instant adaptation (40 EV/s) or normal
        // adaptation (3 EV/s) under slow frames (0.05..2.0 s) and Bevy's default 1.5 EV
        // transition distance, both brightening and darkening.
        for &(speed, dt, start, target) in &[
            (INSTANT_ADAPT_SPEED, 0.5, 0.0, 2.0),
            (INSTANT_ADAPT_SPEED, 1.0, 0.0, 1.0),
            (INSTANT_ADAPT_SPEED, 0.08, 0.0, 1.0),
            (INSTANT_ADAPT_SPEED, 0.5, 3.0, 0.0),
            (ADAPT_SPEED, 1.0, 0.0, 2.0),
            (ADAPT_SPEED, 2.0, 2.5, 0.0),
            (ADAPT_SPEED, 1.0 / 60.0, 0.0, 2.0),
        ] {
            let mut cur = start;
            let increasing = target > start;
            for frame in 0..32 {
                let next = exposure_step(cur, target, speed, speed, 1.5, dt);
                assert!(next.is_finite(), "speed={speed} dt={dt} frame={frame}: non-finite {next}");
                if increasing {
                    assert!(next >= cur - 1e-6 && next <= target + 1e-6, "overshoot brightening: cur={cur} next={next} target={target}");
                } else {
                    assert!(next <= cur + 1e-6 && next >= target - 1e-6, "overshoot darkening: cur={cur} next={next} target={target}");
                }
                cur = next;
            }
            if speed * dt >= 1.5 {
                assert!((cur - target).abs() < 1e-3, "speed={speed} dt={dt}: did not converge ({cur} vs {target})");
            }
        }
    }

    #[test]
    fn colour_grade_off_keeps_base_eye_adaptation_speed() {
        let mut app = App::new();
        app.init_resource::<Assets<AutoExposureCompensationCurve>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<EyeCurve>()
            .init_resource::<crate::sky::SkyState>()
            .add_systems(Update, follow_target_lum);
        let cam = {
            let eye = app.world().resource::<EyeCurve>();
            let ae = auto_exposure(eye);
            app.world_mut().spawn(ae).id()
        };
        // Colour grade off: SkyState::cinematic() returns Cinematic::default() whose
        // HDR_EYE_ADAPT_SPEED is 0.0, which must NOT trigger INSTANT_ADAPT_SPEED (40 EV/s).
        app.world_mut().resource_mut::<crate::sky::SkyState>().grade = false;
        app.update();
        let ae = app.world().get::<AutoExposure>(cam).unwrap();
        assert!((ae.speed_brighten - ADAPT_SPEED).abs() < 1e-5, "grade off kept base speed: {}", ae.speed_brighten);
        assert!((ae.speed_darken - ADAPT_SPEED).abs() < 1e-5);

        // Colour grade on with an explicit IMGS Eye Adapt Speed of 0.0 still maps to instant.
        app.world_mut().resource_mut::<crate::sky::SkyState>().grade = true;
        let mut zero_hdr = fnv_world::imagespace::Hdr::default();
        zero_hdr.v[HDR_EYE_ADAPT_SPEED] = 0.0;
        let mut mods = crate::imagespace::ScreenModifiers::default();
        mods.set_hdr_for_test(zero_hdr, zero_hdr);
        app.insert_resource(mods);
        app.update();
        let ae = app.world().get::<AutoExposure>(cam).unwrap();
        assert!((ae.speed_brighten - INSTANT_ADAPT_SPEED).abs() < 1e-5, "explicit IMGS 0.0 speed: {}", ae.speed_brighten);
    }

    #[test]
    fn physical_exposure_is_bidirectional_with_deadband_and_smooth_spline() {
        assert_eq!(physical_exposure(PHYSICAL_LOW_REF, 1.4), 0.0);
        assert_eq!(physical_exposure(-3.5, 1.4), 0.0);
        assert_eq!(physical_exposure(PHYSICAL_HIGH_REF, 1.4), 0.0);
        assert!(physical_exposure(0.0, 1.4) < -0.4, "bright daylight stops down");
        assert_eq!(physical_exposure(6.0, 1.4), -PHYSICAL_MAX_DARKEN);
        assert!(physical_exposure(-9.0, 1.4) > 2.0, "dark scenes lift");
        assert_eq!(physical_exposure(-16.0, 1.4), PHYSICAL_MAX_LIFT);

        for (target_lum, extra) in [(1.4, 0.0), (2.8, 0.5), (1.4, -0.25)] {
            let c = curve_for(target_lum, extra, true);
            let mut avg = METER_LOW;
            while avg <= METER_HIGH {
                let exposure = c.compensation(avg) - avg;
                let want = physical_exposure(avg, target_lum) + extra;
                assert!(
                    (exposure - want).abs() < 0.03,
                    "Target LUM {target_lum} avg {avg}: got {exposure}, want {want}"
                );
                avg += 0.05;
            }
        }
    }
}
