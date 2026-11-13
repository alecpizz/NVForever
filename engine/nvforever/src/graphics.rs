//! Engine-level picture improvements that need no new or changed assets
//! (docs/design/graphics-enhanced.md). `--graphics vanilla` draws the picture the parity
//! checks compare; `--graphics enhanced` adds, each on its own switch of the settings page:
//!
//! - **Ambient occlusion**: a screen-space pass on the depth prepass darkens creases,
//!   corners and the ground under things (`ssao.rs`; works with the multisampled view).
//! - **Longer shadows** (`fine-shadows`): the sun's shadows reach half as far again
//!   (`SkyState::shadow_reach`), the first cascade as near. Their maps stay 2048² like
//!   everyone's: 4096² cost about 2 ms at Novac (#1367; the owner chose the frame rate).
//! - **Bloom**: bright light (the sun and its glare, lamps, lit windows) glows into what
//!   lies around it, thresholded on the tonemapped picture (`post.rs`).
//!
//! One switch is on in `vanilla` too: **Light glow** (`glow`), the soft glow around neon,
//! lamps, glowing beams and fire that the original draws with its HDR pass (the owner
//! plays with `bDoHighDynamicRange=1`), driven by the image space's HDR values
//! (`post.rs` `glow_params`; docs/spec-notes/world.md §6 "HDR and bloom"). `--graphics-only
//! no-glow` (or the settings switch) turns it off. Bloom, when on, adds its pass to it
//! (until 2026-10-25 it replaced it, and with it the HDR view, eye adaptation and the
//! Strip's neon glow).
//! - **Sharpening**: contrast-adaptive sharpening of the picture (`post.rs`).
//! - **Anisotropic filtering 16x** (8x in vanilla) for the game's textures loaded from then on.
//! - **Anti-aliasing**, one choice and not part of `enhanced`: multisampling (`--msaa`,
//!   off, 2x, 4x or 8x), or the view drawn without it and smoothed afterwards by Bevy's
//!   FXAA or SMAA or by temporal anti-aliasing over the frames (`taa.rs`) ([`ScreenAa`]).
//! - **Shadow filtering** of the sun's shadow maps, also not part of `enhanced`: Bevy's
//!   default (Gaussian) as in vanilla, its cheaper hardware 2x2 or its temporal filter
//!   (meant for temporal anti-aliasing).
//! - **Sun shafts**: light streaming past what stands in front of the sun, as strong as
//!   the weather's fog makes the air hazy (`shafts.rs`).
//! - **Screen-space water reflections** where the mirrored reflection camera does not
//!   reach: water at another height than the one it shows, and interiors (`water.wesl`).
//!
//! The game's own image space (the grade and the modifiers' blur, imagespace.rs) is left
//! as it is: none of these replaces or repeats it. Every value here is an engineering
//! choice made by eye, not a fact about the original game.

use crate::fly::FlyCam;
use crate::sky::{SkyState, Sun};
use crate::ssao::AmbientOcclusion;
use bevy::light::{CascadeShadowConfig, DirectionalLightShadowMap, ShadowFilteringMethod};
use crate::post::PostEffects;
use crate::shafts::LightShafts;
use bevy::prelude::*;
use bevy::anti_alias::fxaa::Fxaa;
use bevy::anti_alias::smaa::Smaa;
use bevy::render::camera::{MipBias, TemporalJitter};
use crate::fsr::{FsrQuality, FsrUpscaling};

/// The command line and saved settings expose the same upscaling modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum FsrMode {
    #[default]
    Off,
    NativeAa,
    Quality,
    Balanced,
    Performance,
    UltraPerformance,
}

impl FsrMode {
    pub fn quality(self) -> Option<FsrQuality> {
        match self {
            Self::Off => None,
            Self::NativeAa => Some(FsrQuality::NativeAa),
            Self::Quality => Some(FsrQuality::Quality),
            Self::Balanced => Some(FsrQuality::Balanced),
            Self::Performance => Some(FsrQuality::Performance),
            Self::UltraPerformance => Some(FsrQuality::UltraPerformance),
        }
    }
}

pub const FSR_CHOICES: [(&str, FsrMode); 6] = [
    ("Off", FsrMode::Off),
    ("Native AA", FsrMode::NativeAa),
    ("Quality", FsrMode::Quality),
    ("Balanced", FsrMode::Balanced),
    ("Performance", FsrMode::Performance),
    ("Ultra Performance", FsrMode::UltraPerformance),
];

/// The look as a whole (`--graphics`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Preset {
    /// The picture the parity and picture checks compare: nothing of this module.
    Vanilla,
    /// Every improvement of this module on.
    Enhanced,
    /// Enhanced, plus the remaster's lighting (docs/design/graphics-remaster.md): screen-space
    /// global illumination and the rest of its switches.
    Remaster,
}

/// The default of `--graphics` for normal play (test runs default to vanilla; see
/// `main.rs` `graphics_preset`). Vanilla: on the owner's PC (GTX 1050) the enhanced look
/// cost about a third of the frame rate at Goodsprings (docs/design/graphics-enhanced.md
/// §5), so it is asked for with `--graphics enhanced` or switched on in the settings.
pub const PLAY_DEFAULT: Preset = Preset::Vanilla;

/// Anti-aliasing of the finished picture instead of multisampling (the view is then drawn
/// without it). `Off`: the multisampling of [`MsaaSetting`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ScreenAa {
    #[default]
    Off,
    /// Bevy's FXAA (`bevy::anti_alias::fxaa`), at its default thresholds.
    Fxaa,
    /// Bevy's SMAA (`bevy::anti_alias::smaa`), at its default (high) preset.
    Smaa,
    /// Our temporal anti-aliasing (`taa.rs`).
    Taa,
}

/// Which improvements are on (the settings page switches each).
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Graphics {
    pub ambient_occlusion: bool,
    pub fine_shadows: bool,
    pub bloom: bool,
    /// Light glow from the image space's HDR values (on in vanilla; see the module notes).
    pub glow: bool,
    /// Eye adaptation on the light glow's HDR view (`eye_adapt.rs`; on in vanilla).
    pub eye_adaptation: bool,
    /// The light glow's bright pass where the image space's Bright Scale is high (the
    /// Strip's neon at night, `post.rs` `BrightPass`; on in vanilla).
    pub neon_glow: bool,
    pub sharpen: bool,
    pub anisotropic: bool,
    pub screen_aa: ScreenAa,
    /// FSR replaces the selected AA while active; UI is drawn at output resolution.
    pub fsr: Option<FsrQuality>,
    pub light_shafts: bool,
    pub water_reflections: bool,
    /// Water mirrors the world through the reflection camera (`reflection.rs`; on in
    /// vanilla, as the original's water reflects the world). Off, water reflects the sky,
    /// and what is on the screen where `water_reflections` is on.
    pub water_mirror: bool,
    /// How the sun's shadow maps are filtered (Bevy's default, Gaussian, in vanilla).
    pub shadow_filter: ShadowFilteringMethod,
    /// Screen-space global illumination (`gi.rs`, remaster): bounced light and the ambient
    /// light's occlusion inside the lighting. Replaces `ambient_occlusion` while on.
    pub global_illumination: bool,
    /// Volumetric atmosphere (`atmos.rs`, remaster): fog near the ground lit by the sun
    /// through its shadow maps, and by the ambient light.
    pub volumetric_fog: bool,
    /// The remaster's colour grade after tonemapping (`post.wesl` `grade`).
    pub colour_grade: bool,
}

/// The keys of `--graphics-only`: the settings page's switches and the anti-aliasing and
/// shadow-filter choices that are not vanilla's, in the order of [`Graphics::describe`].
pub const KEYS: [&str; 15] = [
    "ambient-occlusion",
    "fine-shadows",
    "bloom",
    "sharpen",
    "anisotropic",
    "fxaa",
    "smaa",
    "taa",
    "light-shafts",
    "water-reflections",
    "shadow-filter-hardware",
    "shadow-filter-temporal",
    "global-illumination",
    "volumetric-fog",
    "colour-grade",
];

impl Graphics {
    pub fn preset(p: Preset) -> Self {
        let on = p != Preset::Vanilla;
        let remaster = p == Preset::Remaster;
        Graphics {
            ambient_occlusion: on,
            fine_shadows: on,
            bloom: on,
            glow: true,
            eye_adaptation: true,
            neon_glow: true,
            sharpen: on,
            anisotropic: on,
            // Anti-aliasing after the picture is the other choice to multisampling, not
            // part of the enhanced look: temporal softens textures while the view moves,
            // and the owner plays with 8x multisampling (docs/design/graphics-enhanced.md
            // §3.6). The shadow filter stays Bevy's default too (§3.9).
            screen_aa: ScreenAa::Off,
            fsr: None,
            light_shafts: on,
            water_reflections: on,
            water_mirror: true,
            shadow_filter: ShadowFilteringMethod::Gaussian,
            global_illumination: remaster,
            volumetric_fog: remaster,
            colour_grade: remaster,
        }
    }

    /// The switch with this settings key.
    pub fn switch(&mut self, key: &str) -> Option<&mut bool> {
        Some(match key {
            "ambient-occlusion" => &mut self.ambient_occlusion,
            "fine-shadows" => &mut self.fine_shadows,
            "bloom" => &mut self.bloom,
            "glow" => &mut self.glow,
            "eye-adaptation" => &mut self.eye_adaptation,
            "neon-glow" => &mut self.neon_glow,
            "water-mirror" => &mut self.water_mirror,
            "sharpen" => &mut self.sharpen,
            "anisotropic" => &mut self.anisotropic,
            "light-shafts" => &mut self.light_shafts,
            "water-reflections" => &mut self.water_reflections,
            "global-illumination" => &mut self.global_illumination,
            "volumetric-fog" => &mut self.volumetric_fog,
            "colour-grade" => &mut self.colour_grade,
            _ => return None,
        })
    }

    /// Exactly the improvements named ([`KEYS`]) on (`--graphics-only`); of the
    /// anti-aliasings and of the shadow filters the last named wins.
    pub fn only(keys: &[String]) -> Result<Self, String> {
        let mut g = Graphics::preset(Preset::Vanilla);
        for k in keys.iter().map(|k| k.trim()).filter(|k| !k.is_empty()) {
            match k {
                "fxaa" => g.screen_aa = ScreenAa::Fxaa,
                "smaa" => g.screen_aa = ScreenAa::Smaa,
                "taa" => g.screen_aa = ScreenAa::Taa,
                "shadow-filter-hardware" => g.shadow_filter = ShadowFilteringMethod::Hardware2x2,
                "shadow-filter-temporal" => g.shadow_filter = ShadowFilteringMethod::Temporal,
                "no-glow" => g.glow = false,
                "no-eye-adaptation" => g.eye_adaptation = false,
                "no-neon-glow" => g.neon_glow = false,
                "no-water-mirror" => g.water_mirror = false,
                _ => match g.switch(k) {
                    Some(on) => *on = true,
                    None => return Err(format!("--graphics-only {k}: expected one of {}, no-glow, no-eye-adaptation, no-neon-glow, no-water-mirror", KEYS.join(", "))),
                },
            }
        }
        Ok(g)
    }

    /// The switches that are on, for the log.
    pub fn describe(&self) -> String {
        let names = [
            (self.ambient_occlusion, "ambient occlusion"),
            (self.fine_shadows, "longer shadows"),
            (self.bloom, "bloom"),
            (self.glow, "light glow"),
            (self.glow && !self.eye_adaptation, "no eye adaptation"),
            (self.glow && !self.neon_glow, "no neon glow"),
            (self.sharpen, "sharpening"),
            (self.anisotropic, "16x anisotropic filtering"),
            (self.screen_aa == ScreenAa::Fxaa, "FXAA"),
            (self.screen_aa == ScreenAa::Smaa, "SMAA"),
            (self.screen_aa == ScreenAa::Taa, "temporal anti-aliasing"),
            (self.light_shafts, "sun shafts"),
            (self.water_reflections, "screen-space water reflections"),
            (!self.water_mirror, "no mirrored water"),
            (self.shadow_filter == ShadowFilteringMethod::Hardware2x2, "hardware 2x2 shadow filtering"),
            (self.shadow_filter == ShadowFilteringMethod::Temporal, "temporal shadow filtering"),
            (self.global_illumination, "global illumination"),
            (self.volumetric_fog, "volumetric fog"),
            (self.colour_grade, "colour grade"),
        ];
        let mut on: Vec<&str> = names.iter().filter(|(on, _)| *on).map(|(_, n)| *n).collect();
        if self.fsr.is_some() {
            on.retain(|n| !matches!(*n, "FXAA" | "SMAA" | "temporal anti-aliasing"));
            on.push("FSR 3.1 upscaling");
        }
        if on.is_empty() {
            "vanilla (no engine-level improvements)".to_string()
        } else {
            on.join(", ")
        }
    }
}

/// Shadow map side of every directional light, with longer shadows or without (Bevy's
/// default). 4096² with longer shadows cost about 2.1 ms a frame at Novac and 0.65 ms at
/// Goodsprings for edges hardly sharper near the player [observed 2026-11-03, RTX 4070,
/// 1920x1080; owner's choice 2026-11-03, #1367: the smaller maps, for the frame rate].
const SHADOW_MAP: usize = 2048;
/// The post-process occlusion's strength beside global illumination (0.55 without).
const GI_CONTACT_AO: f32 = 0.35;
/// With longer shadows they reach this many times `--shadow-distance`.
pub const FINE_SHADOW_REACH: f32 = 1.5;

/// The multisampling of the main view: `--msaa`, then the settings page's anti-aliasing
/// choice; used while [`Graphics::screen_aa`] is off.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MsaaSetting(pub Msaa);

/// The settings page's anti-aliasing choices, in its order: the multisampling levels,
/// then the anti-aliasing after the picture.
pub const AA_CHOICES: [(&str, ScreenAa, Msaa); 7] = [
    ("Off", ScreenAa::Off, Msaa::Off),
    ("2x", ScreenAa::Off, Msaa::Sample2),
    ("4x", ScreenAa::Off, Msaa::Sample4),
    ("8x", ScreenAa::Off, Msaa::Sample8),
    ("FXAA", ScreenAa::Fxaa, Msaa::Off),
    ("SMAA", ScreenAa::Smaa, Msaa::Off),
    ("TAA", ScreenAa::Taa, Msaa::Off),
];

/// Whether FSR replaces native AA on this device.
pub fn fsr_active(world: &World) -> bool {
    world.resource::<Graphics>().fsr.is_some() && world.get_resource::<crate::fsr::FsrSupport>().is_some_and(|s| s.0)
}

/// The saved native anti-aliasing choice, an index of [`AA_CHOICES`]. It is kept
/// while FSR is active, so switching FSR off restores it.
pub fn aa_choice(world: &World) -> usize {
    let (aa, msaa) = (world.resource::<Graphics>().screen_aa, world.resource::<MsaaSetting>().0);
    AA_CHOICES.iter().position(|&(_, a, m)| a == aa && (aa != ScreenAa::Off || m == msaa)).unwrap_or(0)
}

/// Choose anti-aliasing `i` of [`AA_CHOICES`]: a multisampling level is also the one the
/// others give back to when chosen away from; the others keep the level.
pub fn set_aa_choice(world: &mut World, i: usize) {
    let (_, aa, msaa) = AA_CHOICES[i.min(AA_CHOICES.len() - 1)];
    if aa == ScreenAa::Off {
        world.resource_mut::<MsaaSetting>().0 = msaa;
    }
    world.resource_mut::<Graphics>().screen_aa = aa;
}

/// The settings page's shadow-filter choices, in its order.
pub const SHADOW_FILTERS: [(&str, ShadowFilteringMethod); 3] = [
    ("Fast", ShadowFilteringMethod::Hardware2x2),
    ("Smooth", ShadowFilteringMethod::Gaussian),
    ("Temporal", ShadowFilteringMethod::Temporal),
];

/// `--msaa`'s samples as a word for the log.
fn msaa_name(m: Msaa) -> &'static str {
    match m {
        Msaa::Off => "off",
        Msaa::Sample2 => "2x",
        Msaa::Sample4 => "4x",
        Msaa::Sample8 => "8x",
    }
}

/// Test aid for measuring (`--log-frame-times`, `--graphics-cycle`): log the frame time
/// every `every` seconds of real time and then, with a cycle, switch to its next set of
/// improvements (the first half second after a switch, when pipelines compile, is left
/// out of the figures). A set may also change the sun's shadows: `no-sun-shadows`,
/// `shadow-distance=<units>`, `cascades=<n>`, `shadow-thin=<ratio>` (otherwise as at the
/// start).
#[derive(Clone, Default)]
pub struct FrameTimes {
    pub every: f32,
    pub cycle: Vec<CycleStep>,
}

/// One set of `--graphics-cycle`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CycleStep {
    pub graphics: Graphics,
    /// `no-sun-shadows` was among the keys: the sun casts no shadows during this set.
    pub no_sun_shadows: bool,
    /// `shadow-distance=<units>`.
    pub shadow_distance: Option<f32>,
    /// `cascades=<n>`.
    pub cascades: Option<usize>,
    /// `shadow-thin=<ratio>` (shadow_thin.rs).
    pub shadow_thin: Option<f32>,
}

impl FrameTimes {
    /// `--graphics-cycle`: sets separated by `;`, each `vanilla`, `enhanced` or keys as
    /// for `--graphics-only`, any of them with `no-sun-shadows` among its keys.
    pub fn parse_cycle(text: &str) -> Result<Vec<CycleStep>, String> {
        text.split(';').map(str::trim).filter(|t| !t.is_empty()).map(|t| {
            let keys: Vec<String> = t.split(',').map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).collect();
            let no_sun_shadows = keys.iter().any(|k| k == "no-sun-shadows");
            let value = |name: &str| {
                keys.iter()
                    .find_map(|k| k.strip_prefix(name))
                    .map(|v| v.parse::<f32>().map_err(|e| format!("--graphics-cycle {name}{v}: {e}")))
                    .transpose()
            };
            let shadow_distance = value("shadow-distance=")?;
            let cascades = value("cascades=")?.map(|n| n as usize);
            let shadow_thin = value("shadow-thin=")?;
            let keys: Vec<String> = keys.iter().filter(|k| *k != "no-sun-shadows" && !k.contains('=')).cloned().collect();
            let graphics = match keys.as_slice() {
                [] => Graphics::preset(Preset::Vanilla),
                [k] if k == "vanilla" => Graphics::preset(Preset::Vanilla),
                [k] if k == "enhanced" => Graphics::preset(Preset::Enhanced),
                [k] if k == "remaster" => Graphics::preset(Preset::Remaster),
                _ => Graphics::only(&keys)?,
            };
            Ok(CycleStep { graphics, no_sun_shadows, shadow_distance, cascades, shadow_thin })
        }).collect()
    }
}

/// The improvements to start with, `--msaa` and the frame-time test aid.
pub struct GraphicsPlugin(pub Graphics, pub Msaa, pub Option<FrameTimes>);

/// Marks a run whose measuring paths (the frame-time aid, frame telemetry's `gpu`, the span
/// profile) asked for the GPU diagnostics: they then record all the time, and the frame
/// split (`perf_overlay::split`) never takes the recorder out.
#[derive(Resource)]
pub(crate) struct MeasuringDiagnostics;

/// Add the shared GPU diagnostics once when a measuring path needs them, and mark the run
/// as measuring with them ([`MeasuringDiagnostics`]).
pub(crate) fn ensure_render_diagnostics(app: &mut App) {
    app.insert_resource(MeasuringDiagnostics);
    add_render_diagnostics(app);
}

/// Add the GPU diagnostics unless they are there (another path, or Bevy's own with Tracy);
/// true when this call added them.
pub(crate) fn add_render_diagnostics(app: &mut App) -> bool {
    if app.is_plugin_added::<bevy::render::diagnostic::RenderDiagnosticsPlugin>() {
        return false;
    }
    app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin);
    true
}

impl Plugin for GraphicsPlugin {
    fn build(&self, app: &mut App) {
        let g = self.0;
        nvf_assets::texture::set_anisotropy(if g.anisotropic { 16 } else { 8 });
        app.insert_resource(g)
            .insert_resource(MsaaSetting(self.1))
            .insert_resource(DirectionalLightShadowMap { size: SHADOW_MAP })
            .add_plugins((crate::ssao::SsaoPlugin, crate::gi::GiPlugin, crate::probe::ProbePlugin, crate::probe::HeightFieldPlugin, crate::atmos::AtmosPlugin, crate::post::PostPlugin, crate::taa::TaaPlugin, crate::shafts::ShaftsPlugin, crate::fsr::FsrPlugin))
            .add_systems(Startup, |g: Res<Graphics>| info!("graphics: {}", g.describe()))
            .add_systems(Update, (apply, sun_shadow_key.pipe(sun_shadow_quality).after(crate::sky::update_sky)).chain());
        if let Some(ft) = self.2.clone().filter(|f| f.every > 0.0) {
            // GPU time per render pass (timestamp queries), logged with the frame time.
            ensure_render_diagnostics(app);
            // What the player's fixed steps cost, counted while this resource exists and
            // logged with the frame time (`nvf_play::phys_load`).
            app.init_resource::<nvf_play::phys_load::PhysLoad>();
            // (time in this period, time counted, slowest, frames counted, cycle index)
            #[allow(clippy::type_complexity)]
            app.add_systems(Update, move |mut commands: Commands, time: Res<Time<Real>>, mut g: ResMut<Graphics>, sky: Res<SkyState>, store: Res<bevy::diagnostic::DiagnosticsStore>, lights: Query<(Option<&Name>, &DirectionalLight, &bevy::camera::visibility::CascadesVisibleEntities)>, views: Query<Option<&Name>, With<Camera>>, mut acc: Local<(f32, f32, f32, u32, usize)>, mut start_shadows: Local<Option<(bool, f32, usize, f32)>>, thin: Res<crate::shadow_thin::ShadowThin>, refs: Query<&nvf_play::RefVisual>, mut phys_load: ResMut<nvf_play::phys_load::PhysLoad>| {
                let first = start_shadows.is_none();
                let start = *start_shadows.get_or_insert((sky.shadows, sky.shadow_distance, sky.shadow_cascades, thin.ratio));
                if first {
                    if let Some(step) = ft.cycle.first() {
                        apply_shadow_step(&mut commands, &sky, start, step);
                    }
                }
                let dt = time.delta_secs();
                acc.0 += dt;
                if acc.0 > 0.5 || ft.cycle.is_empty() {
                    acc.1 += dt;
                    acc.2 = acc.2.max(dt);
                    acc.3 += 1;
                }
                if acc.0 >= ft.every {
                    info!("frame time: {:.2} ms mean, {:.1} ms slowest over {} frames; graphics: {}; sun shadows {}; sun at heading {:.0}, elevation {:.0}", acc.1 * 1e3 / acc.3.max(1) as f32, acc.2 * 1e3, acc.3, g.describe(), if sky.shadows { format!("to {:.0} in {} cascades", sky.shadow_distance, sky.shadow_cascades) } else { "off".into() }, sky.sun_heading(), sky.sun_elevation());
                    // The GPU's time in the passes that cost the most (ms, about the last
                    // second's average).
                    let mut passes: Vec<(String, f64)> = store
                        .iter()
                        .filter(|d| d.path().as_str().starts_with("render/") && d.path().as_str().ends_with("/elapsed_gpu"))
                        .filter_map(|d| Some((d.path().as_str().trim_start_matches("render/").trim_end_matches("/elapsed_gpu").to_string(), d.average()?)))
                        .collect();
                    passes.sort_by(|a, b| b.1.total_cmp(&a.1));
                    let total: f64 = passes.iter().filter(|(n, _)| !n.contains('/')).map(|(_, t)| t).sum();
                    let list: Vec<String> = passes.iter().filter(|(n, _)| !n.contains('/')).take(12).map(|(n, t)| format!("{n} {t:.2}")).collect();
                    info!("gpu time: {total:.2} ms in all top-level passes; {}", list.join(", "));
                    // FSR's parts (fsr.rs sub-spans): AMD's upscaler and NVForever's passes.
                    let fsr: Vec<String> = passes.iter().filter_map(|(n, t)| n.strip_prefix("nvf_fsr_3_1/").map(|part| format!("{part} {t:.3}"))).collect();
                    if !fsr.is_empty() {
                        info!("gpu time in FSR: {}", fsr.join(", "));
                    }
                    // The player's fixed steps over the whole period (none logged when no
                    // frame was played).
                    if let Some(line) = phys_load.take_line() {
                        info!("{line}");
                    }
                    // What the shadow passes draw (Bevy times no shadow pass): the casters
                    // in each cascade of each light with shadows, per view.
                    let mut casters = Vec::new();
                    for (name, light, cascades) in &lights {
                        if !light.shadow_maps_enabled {
                            continue;
                        }
                        let name = name.map_or("light", |n| n.as_str());
                        for (view, per_cascade) in &cascades.entities {
                            let view = views.get(*view).ok().flatten().map_or("view", |n| n.as_str());
                            let counts: Vec<String> = per_cascade.iter().map(|c| c.entities.len().to_string()).collect();
                            casters.push(format!("{name} for {view}: [{}]", counts.join(", ")));
                        }
                    }
                    info!("shadow casters per cascade: {}", if casters.is_empty() { "none (sun shadows off)".to_string() } else { casters.join("; ") });
                    if !casters.is_empty() {
                        info!("shadow thinning: ratio {}; {} of {} references ({} meshes) far and small cast none", thin.ratio, thin.refs.1, thin.refs.0, thin.thinned);
                        // What the sun's last cascade draws: meshes of placed references by
                        // their angular size (radius / distance), and the other meshes.
                        let buckets = crate::shadow_thin::SIZE_BUCKETS;
                        for (name, light, cascades) in &lights {
                            if !light.shadow_maps_enabled || name.is_none_or(|n| n.as_str() != "Sun") {
                                continue;
                            }
                            for per_cascade in cascades.entities.values() {
                                let Some(last) = per_cascade.last() else { continue };
                                let (mut near, mut other, mut unjudged) = (0, 0, 0);
                                let mut by_size = [0usize; 7];
                                for e in last.entities.iter() {
                                    match refs.get(*e).ok().map(|r| thin.sizes.get(&r.0)) {
                                        None => other += 1,
                                        Some(None) => unjudged += 1,
                                        Some(Some(None)) => near += 1,
                                        Some(Some(Some(v))) => by_size[buckets.iter().position(|b| *v < *b).unwrap_or(buckets.len())] += 1,
                                    }
                                }
                                let sizes: Vec<String> = by_size.iter().enumerate().map(|(i, n)| match i {
                                    0 => format!("<{} {n}", buckets[0]),
                                    i if i == buckets.len() => format!(">={} {n}", buckets[i - 1]),
                                    i => format!("{}-{} {n}", buckets[i - 1], buckets[i]),
                                }).collect();
                                info!("sun's last cascade: {} meshes; placed references within {} units {near}, by angular size {}; not judged {unjudged}; other meshes (terrain, trees, LOD, effects) {other}", last.entities.len(), crate::shadow_thin::NEAR, sizes.join(", "));
                            }
                        }
                    }
                    let next = acc.4 + 1;
                    *acc = (0.0, 0.0, 0.0, 0, next);
                    if !ft.cycle.is_empty() {
                        let step = ft.cycle[next % ft.cycle.len()];
                        *g = step.graphics;
                        apply_shadow_step(&mut commands, &sky, start, &step);
                    }
                }
            });
        }
    }
}

/// The sun's shadows of a `--graphics-cycle` set: `start` (on, distance, cascades) as at
/// the start, changed by the set's keys.
fn apply_shadow_step(commands: &mut Commands, sky: &SkyState, start: (bool, f32, usize, f32), step: &CycleStep) {
    let on = start.0 && !step.no_sun_shadows;
    let distance = step.shadow_distance.unwrap_or(start.1);
    let cascades = step.cascades.unwrap_or(start.2);
    let ratio = step.shadow_thin.unwrap_or(start.3);
    commands.queue(move |w: &mut World| {
        let mut thin = w.resource_mut::<crate::shadow_thin::ShadowThin>();
        if thin.ratio != ratio {
            thin.ratio = ratio;
        }
    });
    if on != sky.shadows || distance != sky.shadow_distance || cascades != sky.shadow_cascades {
        commands.queue(move |w: &mut World| {
            let mut sky = w.resource_mut::<SkyState>();
            sky.shadow_distance = distance;
            sky.shadow_cascades = cascades;
            crate::sky::set_shadows(w, on);
        });
    }
}

/// The moon's shadows (the sun's light below the horizon) reach this far, in one cascade
/// [engineering choice, owner
/// 2026-11-02 (#1369): by night the light is dimmer and softer and the dark hides the far
/// shadows; by day's settings they cost about 2.5 ms on the Strip at 22:00, RTX 4070].
pub const MOON_SHADOW_DISTANCE: f32 = 3_000.0;
/// Below this share of its strength (the fade at the horizon) the sun or moon casts no
/// shadows at all: its maps are not drawn [engineering choice: at a tenth of the light the
/// shadow is too faint to see go].
const SHADOW_FADED: f32 = 0.1;

/// Which shadows the sun's light casts at the sun's current height.
#[derive(Clone, Copy, PartialEq, Debug)]
enum SunShadows {
    Day,
    Moon,
    Faded,
}

impl SunShadows {
    /// For the sun's height above the horizon (below 0: the moon) and the light's fade.
    /// Indoors the sun is the cell's directional light (XCLL), which has nothing to do with
    /// the hour outside: it casts the day's shadows.
    fn at(indoors: bool, height: f32, fade: f32) -> Self {
        if indoors {
            SunShadows::Day
        } else if fade < SHADOW_FADED {
            SunShadows::Faded
        } else if height < 0.0 {
            SunShadows::Moon
        } else {
            SunShadows::Day
        }
    }
}

/// What the sun's shadows depend on: the phase and the shadow settings.
#[derive(Clone, Copy, PartialEq, Debug)]
struct SunShadowKey {
    phase: SunShadows,
    shadows: bool,
    distance: f32,
    reach: f32,
    cascades: usize,
}

/// The phase for the sun's height now (the day's indoors) and the shadow settings.
fn sun_shadow_key(sky: Res<SkyState>) -> SunShadowKey {
    let indoors = sky.interior.is_some();
    let (height, fade) = if indoors { (1.0, 1.0) } else { crate::sky::sun_height_and_fade(&sky) };
    SunShadowKey {
        phase: SunShadows::at(indoors, height, fade),
        shadows: sky.shadows,
        distance: sky.shadow_distance,
        reach: sky.shadow_reach,
        cascades: sky.shadow_cascades,
    }
}

/// The sun's shadow quality for the hour: by day the settings' cascades, reach and map size;
/// by night the moon's cheaper ones ([`MOON_SHADOW_DISTANCE`]); none while the light is
/// faded at the horizon. Applied when the phase or a shadow setting changes, and again when
/// a sun is spawned (a worldspace switch makes a new one with the day's cascades) or someone
/// else puts cascades on it ([`apply`], [`crate::sky::set_shadows`] with an unchanged value).
/// It writes the cascades in place, so its own writes do not count as someone else's.
#[allow(clippy::type_complexity)]
fn sun_shadow_quality(
    In(key): In<SunShadowKey>,
    mut commands: Commands,
    new_suns: Query<(), Added<Sun>>,
    mut suns: Query<(Entity, &mut DirectionalLight, Option<&mut CascadeShadowConfig>), With<Sun>>,
    mut done: Local<Option<SunShadowKey>>,
) {
    if suns.is_empty() {
        return;
    }
    let replaced = !new_suns.is_empty() || suns.iter_mut().any(|(_, _, c)| c.is_some_and(|c| c.is_changed()));
    if !replaced && *done == Some(key) {
        return;
    }
    let phase = key.phase;
    if done.is_some_and(|d| d.phase != phase) {
        info!("sun shadows: {phase:?}");
    }
    *done = Some(key);
    let on = key.shadows && phase != SunShadows::Faded;
    for (e, mut light, cascades) in &mut suns {
        if light.shadow_maps_enabled != on {
            light.shadow_maps_enabled = on;
        }
        if on {
            let want = match phase {
                SunShadows::Moon => crate::sky::shadow_cascades(MOON_SHADOW_DISTANCE.min(key.distance), 1.0, 1),
                _ => crate::sky::shadow_cascades(key.distance, key.reach, key.cascades),
            };
            match cascades {
                Some(mut c) => *c = want,
                None => {
                    commands.entity(e).insert(want);
                }
            }
        }
    }
}

/// Put the improvements that are on onto the main camera and the sun, take those that are
/// off away (when a switch changes, and once at the start).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn apply(
    mut commands: Commands,
    g: Res<Graphics>,
    msaa: Res<MsaaSetting>,
    fsr_support: Res<crate::fsr::FsrSupport>,
    mut sky: ResMut<SkyState>,
    cams: Query<(Entity, &Msaa, Option<&FsrUpscaling>, Has<crate::taa::TemporalAa>, Has<Fxaa>, Has<Smaa>), With<FlyCam>>,
    suns: Query<(Entity, &DirectionalLight), With<Sun>>,
    eye: Res<crate::eye_adapt::EyeCurve>,
    cutouts: Option<ResMut<nvf_assets::nif_material::BlendedCutouts>>,
    mut done: Local<Option<(Graphics, Msaa, bool, Vec<Entity>)>>,
) {
    let cam_count = cams.iter().count();
    if cam_count == 0 {
        return;
    }
    let fsr = g.fsr.filter(|_| fsr_support.0);
    // Blended shapes that draw the same as cut-outs (the corn rows) are cut-outs for FSR,
    // which needs their depth and motion (nif_material::BlendedCutouts).
    if let Some(mut cutouts) = cutouts {
        cutouts.set_if_neq(nvf_assets::nif_material::BlendedCutouts(fsr.is_some()));
    }
    let want_msaa = if g.screen_aa == ScreenAa::Off && fsr.is_none() { msaa.0 } else { Msaa::Off };
    let aa_matches = cams.iter().all(|(_, actual_msaa, actual_fsr, taa, fxaa, smaa)| {
        *actual_msaa == want_msaa && actual_fsr.map(|f| f.quality) == fsr
            && taa == (g.screen_aa == ScreenAa::Taa && fsr.is_none())
            && fxaa == (g.screen_aa == ScreenAa::Fxaa && fsr.is_none())
            && smaa == (g.screen_aa == ScreenAa::Smaa && fsr.is_none())
    });
    let mut entities: Vec<Entity> = cams.iter().map(|c| c.0).chain(suns.iter().map(|s| s.0)).collect();
    entities.sort_unstable();
    if aa_matches && done.as_ref().is_some_and(|(d, m, supported, previous)| *d == *g && *m == msaa.0 && *supported == fsr_support.0 && *previous == entities) {
        return;
    }
    let first = done.is_none();
    let msaa_changed = done.as_ref().is_some_and(|(previous, saved, supported, _)| {
        let previous_effective = if previous.screen_aa == ScreenAa::Off && previous.fsr.filter(|_| *supported).is_none() { *saved } else { Msaa::Off };
        previous_effective != want_msaa || *saved != msaa.0
    });
    let fsr_changed = done.as_ref().is_none_or(|(previous, _, supported, _)| previous.fsr.filter(|_| *supported) != fsr);
    *done = Some((*g, msaa.0, fsr_support.0, entities));
    for (cam, cam_msaa, ..) in &cams {
        let mut e = commands.entity(cam);
        // Beside global illumination (which occludes the ambient light at a larger scale
        // inside the lighting) the post-process occlusion stays for the small contact
        // shadows, weaker.
        if g.ambient_occlusion {
            e.insert(if g.global_illumination { AmbientOcclusion::with_strength(GI_CONTACT_AO) } else { AmbientOcclusion::default() });
        } else {
            e.remove::<AmbientOcclusion>();
        }
        if g.volumetric_fog {
            e.insert(crate::atmos::VolumetricAtmosphere::default());
        } else {
            e.remove::<(crate::atmos::VolumetricAtmosphere, crate::atmos::AtmosUniform)>();
        }
        if g.global_illumination {
            e.insert(crate::gi::GlobalIllumination::default());
        } else {
            e.remove::<(crate::gi::GlobalIllumination, crate::gi::GiUniform, bevy::pbr::ScreenIndirectLight)>();
        }
        // FSR already runs its own RCAS sharpening; do not sharpen the picture twice.
        if g.bloom || (g.sharpen && fsr.is_none()) || g.colour_grade {
            e.insert(PostEffects::new(g.bloom, g.sharpen && fsr.is_none()).graded(g.colour_grade));
        } else {
            e.remove::<PostEffects>();
        }
        // The light glow: an HDR main view (values over 1 kept until the tonemapper) and
        // Bevy's energy-conserving bloom on it, its strength and the bright pass's set
        // every frame from the image space (post.rs `image_space_glow`). The enhanced
        // Bloom adds to it (owner's playtest 2026-10-25: with Bloom on, the Strip's neon
        // had no glow at all).
        if g.glow {
            e.insert((bevy::camera::Hdr, crate::post::glow_bloom(), crate::post::ImageSpaceGlow));
        } else {
            e.remove::<(bevy::post_process::bloom::Bloom, bevy::camera::Hdr, crate::post::ImageSpaceGlow)>();
        }
        if fsr.is_some() {
            e.insert(bevy::camera::Hdr);
        }
        // Eye adaptation (eye_adapt.rs) and the bright pass need the HDR view; each has
        // its own switch.
        if g.glow && g.eye_adaptation {
            e.insert(crate::eye_adapt::auto_exposure(&eye));
        } else {
            e.remove::<bevy::post_process::auto_exposure::AutoExposure>();
        }
        if g.glow && g.neon_glow {
            e.insert(crate::post::BrightPass::default());
        } else {
            e.remove::<crate::post::BrightPass>();
        }
        // Anti-aliasing after the picture replaces the multisampling.
        if *cam_msaa != want_msaa {
            e.insert(want_msaa);
        }
        if g.screen_aa == ScreenAa::Taa && fsr.is_none() {
            e.insert(crate::taa::components());
        } else {
            e.remove::<(crate::taa::TemporalAa, TemporalJitter, MipBias)>();
        }
        if let Some(quality) = fsr {
            e.insert((
                FsrUpscaling { quality, sharpness: if g.sharpen { 0.2 } else { 0.0 } },
                bevy::core_pipeline::prepass::DepthPrepass,
                bevy::core_pipeline::prepass::MotionVectorPrepass,
                TemporalJitter::default(),
                MipBias(-1.0),
            ));
        } else {
            e.remove::<(FsrUpscaling, bevy::core_pipeline::prepass::MotionVectorPrepass)>();
        }
        if g.screen_aa == ScreenAa::Fxaa && fsr.is_none() {
            e.insert(Fxaa::default());
        } else {
            e.remove::<Fxaa>();
        }
        if g.screen_aa == ScreenAa::Smaa && fsr.is_none() {
            e.insert(Smaa::default());
        } else {
            e.remove::<Smaa>();
        }
        // Without the component Bevy filters as Gaussian: vanilla leaves the camera as it was.
        if g.shadow_filter == ShadowFilteringMethod::Gaussian {
            e.remove::<ShadowFilteringMethod>();
        } else {
            e.insert(g.shadow_filter);
        }
        if g.light_shafts {
            e.insert(LightShafts::default());
        } else {
            e.remove::<LightShafts>();
        }
    }
    let reach = if g.fine_shadows { FINE_SHADOW_REACH } else { 1.0 };
    let reach_changed = sky.shadow_reach != reach;
    if reach_changed {
        sky.shadow_reach = reach;
    }
    for (e, light) in &suns {
        if light.shadow_maps_enabled && (reach_changed || first) {
            commands.entity(e).insert(crate::sky::shadow_cascades(sky.shadow_distance, sky.shadow_reach, sky.shadow_cascades));
        }
    }
    if !first {
        info!("graphics: {}", g.describe());
    }
    if msaa_changed {
        info!("graphics: multisampling {}", msaa_name(want_msaa));
    }
    if let Some(quality) = fsr.filter(|_| fsr_changed) {
        info!("graphics: FSR {quality:?} active; native anti-aliasing off");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aa_app(screen_aa: ScreenAa, supported: bool) -> (App, Entity) {
        let mut app = App::new();
        let mut graphics = Graphics::preset(Preset::Vanilla);
        graphics.screen_aa = screen_aa;
        app.insert_resource(graphics)
            .insert_resource(MsaaSetting(Msaa::Sample8))
            .insert_resource(crate::fsr::FsrSupport(supported))
            .init_resource::<SkyState>()
            .init_resource::<Assets<bevy::post_process::auto_exposure::AutoExposureCompensationCurve>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<crate::eye_adapt::EyeCurve>()
            .add_systems(Update, apply);
        let camera = app.world_mut().spawn((FlyCam { yaw: 0.0, pitch: 0.0, speed: 800.0 }, Msaa::Sample8)).id();
        app.world_mut().spawn((Sun, DirectionalLight::default()));
        (app, camera)
    }

    fn assert_native_aa(app: &App, camera: Entity, selected: ScreenAa) {
        let camera = app.world().entity(camera);
        assert!(!camera.contains::<FsrUpscaling>());
        assert_eq!(*camera.get::<Msaa>().unwrap(), if selected == ScreenAa::Off { Msaa::Sample8 } else { Msaa::Off });
        assert_eq!(camera.contains::<crate::taa::TemporalAa>(), selected == ScreenAa::Taa);
        assert_eq!(camera.contains::<TemporalJitter>(), selected == ScreenAa::Taa);
        assert_eq!(camera.contains::<MipBias>(), selected == ScreenAa::Taa);
        assert_eq!(camera.contains::<Fxaa>(), selected == ScreenAa::Fxaa);
        assert_eq!(camera.contains::<Smaa>(), selected == ScreenAa::Smaa);
        assert!(!camera.contains::<bevy::core_pipeline::prepass::MotionVectorPrepass>());
    }

    #[test]
    fn fsr_exclusively_owns_aa_and_off_restores_the_saved_native_choice() {
        for selected in [ScreenAa::Off, ScreenAa::Taa, ScreenAa::Fxaa, ScreenAa::Smaa] {
            for quality in FSR_CHOICES.into_iter().filter_map(|(_, m)| m.quality()) {
                let (mut app, camera) = aa_app(selected, true);
                app.update();
                assert_native_aa(&app, camera, selected);

                app.world_mut().resource_mut::<Graphics>().fsr = Some(quality);
                app.update();
                let entity = app.world().entity(camera);
                assert_eq!(entity.get::<FsrUpscaling>().unwrap().quality, quality);
                assert_eq!(*entity.get::<Msaa>().unwrap(), Msaa::Off);
                assert!(!entity.contains::<crate::taa::TemporalAa>());
                assert!(!entity.contains::<Fxaa>());
                assert!(!entity.contains::<Smaa>());
                assert!(entity.contains::<TemporalJitter>());
                assert!(entity.contains::<bevy::core_pipeline::prepass::MotionVectorPrepass>());
                assert!(entity.contains::<bevy::core_pipeline::prepass::DepthPrepass>());
                assert_eq!(app.world().resource::<Graphics>().screen_aa, selected, "FSR preserves the saved native AA choice");

                app.world_mut().resource_mut::<Graphics>().fsr = None;
                app.update();
                assert_native_aa(&app, camera, selected);
            }
        }
    }

    #[test]
    fn fsr_aa_is_applied_without_a_sun_to_replacement_cameras_and_repairs_conflicting_components() {
        let (mut app, old) = aa_app(ScreenAa::Off, true);
        let suns: Vec<Entity> = app.world_mut().query_filtered::<Entity, With<Sun>>().iter(app.world()).collect();
        for sun in suns { app.world_mut().despawn(sun); }
        app.world_mut().resource_mut::<Graphics>().fsr = Some(FsrQuality::NativeAa);
        app.update();
        assert_eq!(*app.world().get::<Msaa>(old).unwrap(), Msaa::Off);
        assert!(app.world().get::<FsrUpscaling>(old).is_some());

        app.world_mut().despawn(old);
        let camera = app.world_mut().spawn((FlyCam { yaw: 0.0, pitch: 0.0, speed: 800.0 }, Msaa::Sample8)).id();
        app.update();
        assert_eq!(*app.world().get::<Msaa>(camera).unwrap(), Msaa::Off);
        assert!(app.world().get::<FsrUpscaling>(camera).is_some(), "same camera count, different camera");

        app.world_mut().entity_mut(camera).insert((Msaa::Sample8, crate::taa::TemporalAa::default(), Fxaa::default(), Smaa::default()));
        app.update();
        let entity = app.world().entity(camera);
        assert_eq!(*entity.get::<Msaa>().unwrap(), Msaa::Off);
        assert!(!entity.contains::<crate::taa::TemporalAa>() && !entity.contains::<Fxaa>() && !entity.contains::<Smaa>());
        let tick = entity.get_ref::<FsrUpscaling>().unwrap().last_changed();
        app.update();
        assert_eq!(app.world().entity(camera).get_ref::<FsrUpscaling>().unwrap().last_changed(), tick, "stable frames do not reset the camera components");

        app.world_mut().resource_mut::<crate::fsr::FsrSupport>().0 = false;
        app.update();
        assert_native_aa(&app, camera, ScreenAa::Off);
        app.world_mut().resource_mut::<crate::fsr::FsrSupport>().0 = true;
        app.update();
        assert_eq!(*app.world().get::<Msaa>(camera).unwrap(), Msaa::Off);
    }

    #[test]
    fn unsupported_fsr_keeps_native_aa_active() {
        for selected in [ScreenAa::Off, ScreenAa::Taa, ScreenAa::Fxaa, ScreenAa::Smaa] {
            let (mut app, camera) = aa_app(selected, false);
            app.world_mut().resource_mut::<Graphics>().fsr = Some(FsrQuality::Quality);
            app.update();
            assert_native_aa(&app, camera, selected);
        }
    }

    #[test]
    fn the_moon_casts_cheaper_shadows_and_none_at_the_horizon() {
        assert_eq!(SunShadows::at(false, 0.8, 1.0), SunShadows::Day);
        assert_eq!(SunShadows::at(false, -0.5, 1.0), SunShadows::Moon);
        assert_eq!(SunShadows::at(false, 0.005, 0.05), SunShadows::Faded, "sunrise: the light a twentieth of itself");
        assert_eq!(SunShadows::at(false, -0.005, 0.05), SunShadows::Faded, "moonset");
        assert_eq!(SunShadows::at(false, 0.02, SHADOW_FADED), SunShadows::Day, "back on at the threshold");
        // Indoors the sun is the cell's light: the day's shadows whatever the hour outside.
        assert_eq!(SunShadows::at(true, -0.5, 1.0), SunShadows::Day, "an interior at night");
        assert_eq!(SunShadows::at(true, 0.005, 0.05), SunShadows::Day, "an interior at dawn");
    }

    #[derive(Resource)]
    struct TestKey(SunShadowKey);

    fn shadow_app(phase: SunShadows) -> App {
        let mut app = App::new();
        app.insert_resource(TestKey(SunShadowKey { phase, shadows: true, distance: 8000.0, reach: 1.0, cascades: 3 }))
            .add_systems(Update, (|k: Res<TestKey>| k.0).pipe(sun_shadow_quality));
        app
    }

    /// A sun as `setup_sky` spawns it: shadows on, the day's cascades.
    fn spawn_day_sun(app: &mut App) -> Entity {
        let day = crate::sky::shadow_cascades(8000.0, 1.0, 3);
        app.world_mut().spawn((Sun, DirectionalLight { shadow_maps_enabled: true, ..default() }, day)).id()
    }

    fn bounds(app: &App, e: Entity) -> Vec<f32> {
        app.world().get::<CascadeShadowConfig>(e).expect("cascades").bounds.clone()
    }

    #[test]
    fn a_new_sun_or_new_cascades_get_the_phase_again() {
        let mut app = shadow_app(SunShadows::Moon);
        let moon = crate::sky::shadow_cascades(MOON_SHADOW_DISTANCE, 1.0, 1).bounds;
        let first = spawn_day_sun(&mut app);
        app.update();
        assert_eq!(bounds(&app, first), moon);

        // A worldspace switch: the old sun goes, a new one with the day's cascades comes in
        // the same frame, under the same phase.
        app.world_mut().despawn(first);
        let second = spawn_day_sun(&mut app);
        app.update();
        assert_eq!(bounds(&app, second), moon, "the new sun casts the moon's cascade");

        // sky::set_shadows(true) with nothing changed puts the day's cascades back.
        app.world_mut().entity_mut(second).insert(crate::sky::shadow_cascades(8000.0, 1.0, 3));
        app.update();
        assert_eq!(bounds(&app, second), moon, "someone else's cascades are replaced");

        // Its own write does not count as someone else's: no re-apply every frame.
        let tick = app.world().entity(second).get_ref::<CascadeShadowConfig>().unwrap().last_changed();
        app.update();
        app.update();
        assert_eq!(app.world().entity(second).get_ref::<CascadeShadowConfig>().unwrap().last_changed(), tick);

        // While the light fades at the horizon a new sun casts nothing.
        app.world_mut().resource_mut::<TestKey>().0.phase = SunShadows::Faded;
        app.update();
        let third = spawn_day_sun(&mut app);
        app.update();
        assert!(!app.world().get::<DirectionalLight>(third).unwrap().shadow_maps_enabled);
        assert!(!app.world().get::<DirectionalLight>(second).unwrap().shadow_maps_enabled);
    }

    #[test]
    fn going_indoors_at_night_gives_the_day_shadows_back() {
        let mut app = shadow_app(SunShadows::Moon);
        let sun = spawn_day_sun(&mut app);
        app.update();
        assert_eq!(bounds(&app, sun).len(), 1);
        // Indoors the key's phase is Day (SunShadows::at with indoors), whatever the hour.
        app.world_mut().resource_mut::<TestKey>().0.phase = SunShadows::at(true, -0.5, 1.0);
        app.update();
        assert_eq!(bounds(&app, sun), crate::sky::shadow_cascades(8000.0, 1.0, 3).bounds);
        // And back out into the night.
        app.world_mut().resource_mut::<TestKey>().0.phase = SunShadows::at(false, -0.5, 1.0);
        app.update();
        assert_eq!(bounds(&app, sun).len(), 1);
    }

    #[test]
    fn every_key_is_a_switch_and_enhanced_leaves_taa_off() {
        let all = Graphics::only(&KEYS.map(String::from)).expect("keys");
        // Of the three anti-aliasings and the two shadow filters the last named is on.
        assert_eq!(all.describe().split(", ").count(), KEYS.len() + 1 - 3);
        assert_eq!((all.screen_aa, all.shadow_filter), (ScreenAa::Taa, ShadowFilteringMethod::Temporal));
        let enhanced = Graphics::preset(Preset::Enhanced);
        assert!(enhanced.screen_aa == ScreenAa::Off && enhanced.light_shafts && enhanced.water_reflections);
        assert_eq!(enhanced.shadow_filter, ShadowFilteringMethod::Gaussian);
        assert!(Graphics::only(&["nonsense".into()]).is_err());
        let cycle = FrameTimes::parse_cycle("vanilla; taa,light-shafts ;enhanced;vanilla,no-sun-shadows;shadow-distance=8000,cascades=3").expect("cycle");
        let graphics: Vec<Graphics> = cycle.iter().map(|c| c.graphics).collect();
        let vanilla = Graphics::preset(Preset::Vanilla);
        assert_eq!(graphics, vec![vanilla, Graphics::only(&["taa".into(), "light-shafts".into()]).unwrap(), enhanced, vanilla, vanilla]);
        assert_eq!(cycle.iter().map(|c| c.no_sun_shadows).collect::<Vec<_>>(), vec![false, false, false, true, false]);
        assert_eq!((cycle[4].shadow_distance, cycle[4].cascades), (Some(8000.0), Some(3)));
    }

    #[test]
    fn light_glow_is_vanilla_and_can_be_switched_off() {
        let vanilla = Graphics::preset(Preset::Vanilla);
        assert!(vanilla.glow && !vanilla.bloom);
        assert_eq!(vanilla.describe(), "light glow");
        let off = Graphics::only(&["no-glow".into()]).expect("no-glow");
        assert!(!off.glow);
        assert_eq!(off.describe(), "vanilla (no engine-level improvements)");
        // Bloom, when on, adds to the glow.
        assert!(Graphics::only(&["bloom".into()]).unwrap().describe().contains("light glow"));
    }

    #[test]
    fn water_mirror_is_vanilla_and_can_be_switched_off() {
        assert!(Graphics::preset(Preset::Vanilla).water_mirror && Graphics::preset(Preset::Enhanced).water_mirror);
        let off = Graphics::only(&["no-water-mirror".into()]).expect("no-water-mirror");
        assert!(!off.water_mirror);
        assert_eq!(off.describe(), "light glow, no mirrored water");
        let mut g = Graphics::preset(Preset::Vanilla);
        *g.switch("water-mirror").expect("settings key") = false;
        assert_eq!(g, off);
    }

    // The guard alone: repeated requests and an earlier registration. Both callers
    // together are covered by the smoke scenario render-diagnostics-combined.
    #[test]
    fn render_diagnostics_guard_allows_repeated_requests() {
        let mut app = App::new();
        ensure_render_diagnostics(&mut app);
        ensure_render_diagnostics(&mut app);
        assert!(app.is_plugin_added::<bevy::render::diagnostic::RenderDiagnosticsPlugin>());

        let mut already_added = App::new();
        already_added.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin);
        ensure_render_diagnostics(&mut already_added);
        assert!(already_added.is_plugin_added::<bevy::render::diagnostic::RenderDiagnosticsPlugin>());
        assert!(already_added.world().contains_resource::<MeasuringDiagnostics>(), "a measuring path marks the run even when the plugin was there");

        // The overlay's request: true only for the call that added the plugin.
        let mut overlay = App::new();
        assert!(add_render_diagnostics(&mut overlay));
        assert!(!add_render_diagnostics(&mut overlay));
        assert!(!overlay.world().contains_resource::<MeasuringDiagnostics>());
    }
}
