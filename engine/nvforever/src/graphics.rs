//! Engine-level picture improvements that need no new or changed assets
//! (docs/design/graphics-enhanced.md). `--graphics vanilla` draws the picture the parity
//! checks compare; `--graphics enhanced` adds, each on its own switch of the settings page:
//!
//! - **Ambient occlusion**: a screen-space pass on the depth prepass darkens creases,
//!   corners and the ground under things (`ssao.rs`; works with the multisampled view).
//! - **Longer shadows** (`fine-shadows`): the sun's shadows reach half as far again
//!   (`SkyState::shadow_reach`), the first cascade as near. Their maps are as large as
//!   everyone's: 2048² by default (4096² cost about 2 ms at Novac, #1367; the owner chose
//!   the frame rate), 4096² with the Shadow resolution row ([`SHADOW_RESOLUTIONS`], #2843).
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
//! - **Terrain height blending** (`terrain-height-blend`, off in both presets: whether
//!   `enhanced` turns it on is the owner's call): where landscape texture layers meet, the
//!   incoming texture's brighter texels come through first (terrain.rs "Height blending",
//!   #2550).
//! - **Terrain anti-tiling** (`terrain-anti-tiling`, off in both presets, likewise): the
//!   landscape textures are read at a few offset places hashed from the world and blended,
//!   so their repeat no longer shows as a grid (terrain.rs "Anti-tiling", #2551).
//! - **Terrain parallax** (`terrain-parallax`, off in both presets, likewise until its cost
//!   is measured; it needs Authored materials): the landscape's texture layers whose
//!   texture pack gives a height map (`_p`) show relief near the camera (terrain.rs
//!   "Parallax", #3322).
//! - **Cloud shadows** (`cloud-shadows`, off in both presets: #2670 asked for it "in
//!   `enhanced`", a row the menu overhaul removed, and its strength is not calibrated yet):
//!   the weather's cloud layers darken the sun's direct light where their shadow falls
//!   (cloud_shadows.rs, sun_visibility.rs, docs/design/sun-visibility.md). Switching it
//!   rebuilds the world views' material pipelines.
//! - **Terrain shadows** (`terrain-shadows`, off in both presets: #2671 asked for it "in
//!   `enhanced`", likewise, and its softness and bias are not tuned yet): at a low sun the
//!   landscape shades itself beyond the sun shadows' range (sun_heightfield.rs,
//!   terrain_shadows.rs, sun_visibility.rs, docs/design/terrain-shadows.md). Switching it
//!   rebuilds the world views' material pipelines.
//! - **Heat haze** (`heat-haze`, a prototype, off in both presets): the far ground near the
//!   horizon shimmers slightly on hot clear days, by the sun and the live weather
//!   (`heat_haze.rs`).
//! - **Desert haze** (`desert-haze`, off in every preset): a thin dust haze added to the
//!   weather's own distance fog, thicker with distance and toward the ground, warmer toward
//!   the sun, as far as the weather's fog leaves room for it (haze.rs, #2675).
//! - **Volumetric clouds** (`volumetric-clouds`, a prototype, off in every preset): the
//!   weather's cloud layers drawn as a lit slab of cloud above the world, its cover the one
//!   Cloud shadows composes (volumetric_clouds.rs, #2991).
//! - **Volumetric light** (`volumetric-light`, a prototype, off in every preset): the sun's
//!   light in the air, rays and a glow toward a low sun from a froxel grid
//!   (medium_air*.rs, #3032 stage 3); its strength is a slider of its own
//!   (medium_air_demand.rs `AirStrengthSetting`, not part of this resource). Sun shafts
//!   yield to it while it draws (shafts.rs).
//! - **Aerial perspective** (`aerial-perspective`, off in every preset): far scenery takes on
//!   the weather's sky colours with distance, per colour channel and toward the sun, inside the
//!   weather's own fog (aerial.rs, #2993).
//! - **Night sky detail** (`night-sky-detail`, off in every preset): the stars twinkle, the
//!   moon has a glow and the clouds take its light, from the weather's own colours and fog
//!   (night_sky.rs, #2995).
//! - **Distant lights** (`distant-lights`, off in both presets): at night, placed lights in
//!   the cells beyond the loaded ones show as small glows (distant_lights.rs, #3013).
//!
//! Another switch is off in both looks and not part of `enhanced`: **Blend meshes into
//! ground** (`mesh-terrain-blend`, `mesh_terrain_blend.rs`), outdoor roads, rocks and
//! cliffs taking the landscape's colour and shading where they meet it (#2661).
//!
//! The game's own image space (the grade and the modifiers' blur, imagespace.rs) is left
//! as it is: none of these replaces or repeats it. Every value here is an engineering
//! choice made by eye, not a fact about the original game.

use crate::fly::FlyCam;
use crate::sky::{SkyState, Sun};
use crate::ssao::{AmbientOcclusion, AoLevel};
use bevy::light::{CascadeShadowConfig, DirectionalLightShadowMap, ShadowFilteringMethod};
use crate::post::PostEffects;
use crate::shafts::LightShafts;
use bevy::prelude::*;
use bevy::anti_alias::fxaa::Fxaa;
use bevy::anti_alias::smaa::Smaa;
use bevy::render::camera::{MipBias, TemporalJitter};
use crate::fsr::{FsrQuality, FsrUpscaling};
use crate::upscaler::{DlssModel, DlssUpscaling, UpscaleQuality, UpscalerChoice, UpscalerStatus};

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

/// The older `fsr-upscaling` row's choices by the names its INI lines and the test aids use:
/// [`FSR_CHOICES`]' six as they always were (off and FSR 3.1 at each quality), then DLSS at
/// each quality (#2198). The Upscaler and Upscaling mode rows took over from it and still
/// read these (`settings.rs` `parse_legacy`, the test aid's `fsr-upscaling=<name>`).
pub const FSR_UPSCALING_CHOICES: [(&str, UpscalerChoice); 11] = [
    ("Off", UpscalerChoice::Off),
    ("Native AA", UpscalerChoice::Fsr(UpscaleQuality::NativeAa)),
    ("Quality", UpscalerChoice::Fsr(UpscaleQuality::Quality)),
    ("Balanced", UpscalerChoice::Fsr(UpscaleQuality::Balanced)),
    ("Performance", UpscalerChoice::Fsr(UpscaleQuality::Performance)),
    ("Ultra Performance", UpscalerChoice::Fsr(UpscaleQuality::UltraPerformance)),
    ("DLSS Native AA", UpscalerChoice::Dlss(UpscaleQuality::NativeAa)),
    ("DLSS Quality", UpscalerChoice::Dlss(UpscaleQuality::Quality)),
    ("DLSS Balanced", UpscalerChoice::Dlss(UpscaleQuality::Balanced)),
    ("DLSS Performance", UpscalerChoice::Dlss(UpscaleQuality::Performance)),
    ("DLSS Ultra Performance", UpscalerChoice::Dlss(UpscaleQuality::UltraPerformance)),
];

/// The look as a whole (`--graphics`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Preset {
    /// The picture the parity and picture checks compare: nothing of this module.
    Vanilla,
    /// Every improvement of this module on.
    Enhanced,
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
    /// Its level while it is on (the Ambient occlusion row's Performance or Quality,
    /// `--ambient-occlusion`; ssao.rs `AoLevel`): Quality in every preset.
    pub ambient_occlusion_level: AoLevel,
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
    /// The requested temporal upscaler (`upscaler.rs`), the one setting for FSR and DLSS. What
    /// draws is the status's effective choice (`upscaler::UpscalerStatus`); it replaces the
    /// selected AA while active; UI is drawn at output resolution. FSR's code reads and writes
    /// it through [`Graphics::fsr`] and [`Graphics::set_fsr`].
    pub upscaler: UpscalerChoice,
    /// The DLSS model asked for (the DLSS model row, `--dlss-model`; `upscaler::DlssModel`):
    /// used while DLSS draws, kept meanwhile. Default: NVIDIA's own pick for each quality.
    pub dlss_model: DlssModel,
    pub light_shafts: bool,
    pub water_reflections: bool,
    pub water_effects: bool,
    /// Water mirrors the world through the reflection camera (`reflection.rs`; on in
    /// vanilla, as the original's water reflects the world). Off, water reflects the sky,
    /// and what is on the screen where `water_reflections` is on.
    pub water_mirror: bool,
    /// How the sun's shadow maps are filtered (Bevy's default, Gaussian, in vanilla).
    pub shadow_filter: ShadowFilteringMethod,
    /// Placed meshes blend into the landscape where they meet it (`mesh_terrain_blend.rs`,
    /// #2661): off in both looks and not part of `enhanced` (the maintainer decides that);
    /// the test aid `NVF_PLAY_TEST_MESH_BLEND` overrides it.
    pub mesh_terrain_blend: bool,
    /// Height-based blending of the landscape's folded texture layers (terrain.rs
    /// "Height blending"; off in both presets) [design, #2550].
    pub terrain_height_blend: bool,
    /// Anti-tiling of the landscape textures (terrain.rs "Anti-tiling"; off in both
    /// presets) [design, #2551].
    pub terrain_anti_tiling: bool,
    /// Parallax from authored height maps on the landscape's folded texture layers
    /// (terrain.rs "Parallax"; off in both presets) [design, #3322].
    pub terrain_parallax: bool,
    /// The weather's clouds shade the sun (cloud_shadows.rs; off in both presets)
    /// [design, #2670].
    pub cloud_shadows: bool,
    /// The landscape shades the sun beyond the cascades (terrain_shadows.rs; off in both
    /// presets) [design, #2671].
    pub terrain_shadows: bool,
    /// Heat haze at distance (heat_haze.rs; a prototype, off in both presets) [design].
    pub heat_haze: bool,
    /// A dust haze in the weather's fog (haze.rs; off in every preset) [design, #2675].
    pub desert_haze: bool,
    /// The weather's clouds as a lit volume (volumetric_clouds.rs; a prototype, off in every
    /// preset) [design, #2991].
    pub volumetric_clouds: bool,
    /// Its level while it is on (the Volumetric clouds row's Performance, Normal or High;
    /// `volumetric_clouds.rs` `CloudLevel`): Normal in every preset [design, 2026-11-23].
    pub volumetric_clouds_level: crate::volumetric_clouds::CloudLevel,
    /// Surfaces whose diffuse has an authored PBR set under `textures\pbr\` are shaded from
    /// it (#2981, docs/spec-notes/pbr-materials.md; on in both looks: a texture pack's maps
    /// are assets, as a diffuse replacer is). Read once at the start (nvf_assets
    /// `authored::set_enabled`, main.rs); this field is the settings row's value for the
    /// next start.
    pub authored_materials: bool,
    /// Authored materials reflect the sky or the room around them (#2981 reflections,
    /// reflection_sky.rs; docs/spec-notes/pbr-materials.md "Reflections on authored
    /// materials"; on in both looks: without a pack, or with Authored materials off, nothing
    /// changes). Read once at the start (nvf_assets `reflection_cube::set_enabled`, main.rs);
    /// this field is the settings row's value for the next start.
    pub authored_reflections: bool,
    /// Glows of placed lights beyond the loaded cells at night (distant_lights.rs; off in
    /// both presets) [design, #3013].
    pub distant_lights: bool,
    /// Full models of static scenery in the cells beyond the loaded ones, draw-only
    /// (full_detail.rs; 7 x 7 in `enhanced`, off in vanilla) [design, #2701; maintainer
    /// decision 2026-11-21 on #3048].
    pub full_detail: crate::full_detail::FullDetail,
    /// Static scenery the distant LOD blocks do not hold, from its own LOD models or full
    /// models by size, beyond the loaded cells and the ring, draw-only (distant_objects.rs;
    /// off in both presets) [design, #3014].
    pub distant_objects: crate::distant_objects::DistantObjects,
    /// Far scenery takes on the sky's colours in the weather's fog (aerial.rs; off in every
    /// preset) [design, #2993].
    pub aerial_perspective: bool,
    /// Twinkling stars, a moon glow and moonlit clouds (night_sky.rs; off in every preset)
    /// [design, #2995].
    pub night_sky_detail: bool,
    /// Physical PBR lighting (graphics-enhanced.md §3.25; off in both presets, fixed at the
    /// start: nvf_assets `authored::set_physical_pbr`) [design, #3827].
    pub physical_pbr: bool,
    /// The sun's light in the air (medium_air_demand.rs; a prototype, off in every preset)
    /// [design, #3032 stage 3].
    pub volumetric_light: bool,
}

/// The settings key of the Authored materials row (#2981).
pub const AUTHORED_MATERIALS_KEY: &str = "authored-materials";

/// The settings key of the Reflections on authored materials row (#2981).
pub const AUTHORED_REFLECTIONS_KEY: &str = "authored-reflections";

/// The keys of `--graphics-only`: the settings page's switches and the anti-aliasing,
/// shadow-filter, full-detail, distant-object and cloud-level choices that are not vanilla's, in the
/// order of [`Graphics::describe`].
pub const KEYS: [&str; 33] = [
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
    "water-effects",
    "mesh-terrain-blend",
    "shadow-filter-hardware",
    "shadow-filter-temporal",
    "terrain-height-blend",
    "terrain-anti-tiling",
    "terrain-parallax",
    "cloud-shadows",
    "terrain-shadows",
    "heat-haze",
    "desert-haze",
    "full-detail-7x7",
    "full-detail-9x9",
    "distant-objects-8",
    "distant-objects-12",
    "volumetric-clouds",
    "volumetric-clouds-performance",
    "volumetric-clouds-high",
    "volumetric-light",
    "aerial-perspective",
    "night-sky-detail",
    "physical-pbr",
    "distant-lights",
];

impl Graphics {
    /// FSR's quality when FSR is the requested upscaler; `None` for off and for DLSS (FSR's
    /// code treats a DLSS request as no FSR).
    pub fn fsr(&self) -> Option<FsrQuality> {
        self.upscaler.fsr()
    }

    /// FSR at `quality`, or off: the requested upscaler as the FSR row and `--fsr` set it
    /// before DLSS joined them (both set `upscaler` now; the tests still use it).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_fsr(&mut self, quality: Option<FsrQuality>) {
        self.upscaler = UpscalerChoice::from_fsr(quality);
    }

    pub fn preset(p: Preset) -> Self {
        let on = p == Preset::Enhanced;
        Graphics {
            ambient_occlusion: on,
            ambient_occlusion_level: AoLevel::Quality,
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
            upscaler: UpscalerChoice::Off,
            dlss_model: DlssModel::Default,
            light_shafts: on,
            water_reflections: on,
            water_effects: on,
            water_mirror: true,
            shadow_filter: ShadowFilteringMethod::Gaussian,
            // Off in both looks, so `enhanced` (quality.rs `enhanced_keys`) leaves it out.
            mesh_terrain_blend: false,
            // Off in both looks: an addition of our own, not part of `enhanced` until the
            // owner chooses so [design, #2550].
            terrain_height_blend: false,
            // The same for anti-tiling [design, #2551].
            terrain_anti_tiling: false,
            // The same for terrain parallax, whose cost is still to measure [design, #3322;
            // the maintainer's call].
            terrain_parallax: false,
            // The same for cloud shadows, whose strength is still to calibrate [design,
            // #2670; the issue's "in enhanced" names a row the overhaul removed].
            cloud_shadows: false,
            // The same for terrain shadows, whose softness and bias are still to tune
            // [design, #2671; the issue's "in enhanced" names a row the overhaul removed].
            terrain_shadows: false,
            // Off in both looks: a prototype of our own [design].
            heat_haze: false,
            // Off in every preset: an addition of our own [design, #2675].
            desert_haze: false,
            // Off in every preset: a prototype of our own [design, #2991].
            volumetric_clouds: false,
            volumetric_clouds_level: crate::volumetric_clouds::CloudLevel::Normal,
            // On in both looks [design, #2981]: without a pack nothing changes.
            authored_materials: true,
            // On in both looks [design, #2981]: without a pack nothing reflects.
            authored_reflections: true,
            // Off in both looks: an addition of our own [design, #3013].
            distant_lights: false,
            // An addition of our own for machines with GPU headroom: the 7 x 7 ring in
            // `enhanced`, off in vanilla [design, #2701; maintainer decision 2026-11-21 on
            // #3048].
            full_detail: if on { crate::full_detail::FullDetail::Seven } else { crate::full_detail::FullDetail::Off },
            // Off in both looks: an addition of our own; whether `enhanced` turns it on is
            // the maintainer's call [design, #3014].
            distant_objects: crate::distant_objects::DistantObjects::Off,
            // Off in every preset: an addition of our own [design, #2993].
            aerial_perspective: false,
            // Off in every preset: an addition of our own [design, #2995].
            night_sky_detail: false,
            physical_pbr: false,
            // Off in every preset: a prototype of our own [design, #3032 stage 3].
            volumetric_light: false,
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
            "water-effects" => &mut self.water_effects,
            "mesh-terrain-blend" => &mut self.mesh_terrain_blend,
            "terrain-height-blend" => &mut self.terrain_height_blend,
            "terrain-anti-tiling" => &mut self.terrain_anti_tiling,
            "terrain-parallax" => &mut self.terrain_parallax,
            "cloud-shadows" => &mut self.cloud_shadows,
            "terrain-shadows" => &mut self.terrain_shadows,
            "heat-haze" => &mut self.heat_haze,
            "desert-haze" => &mut self.desert_haze,
            "volumetric-clouds" => &mut self.volumetric_clouds,
            "aerial-perspective" => &mut self.aerial_perspective,
            "night-sky-detail" => &mut self.night_sky_detail,
            "physical-pbr" => &mut self.physical_pbr,
            AUTHORED_MATERIALS_KEY => &mut self.authored_materials,
            AUTHORED_REFLECTIONS_KEY => &mut self.authored_reflections,
            "distant-lights" => &mut self.distant_lights,
            "volumetric-light" => &mut self.volumetric_light,
            _ => return None,
        })
    }

    /// Exactly the improvements named ([`KEYS`]) on (`--graphics-only`); of the
    /// anti-aliasings, of the shadow filters, of the full-detail sizes, of the
    /// distant-object reaches and of the clouds' levels the last named wins.
    pub fn only(keys: &[String]) -> Result<Self, String> {
        let mut g = Graphics::preset(Preset::Vanilla);
        for k in keys.iter().map(|k| k.trim()).filter(|k| !k.is_empty()) {
            match k {
                "fxaa" => g.screen_aa = ScreenAa::Fxaa,
                "smaa" => g.screen_aa = ScreenAa::Smaa,
                "taa" => g.screen_aa = ScreenAa::Taa,
                "shadow-filter-hardware" => g.shadow_filter = ShadowFilteringMethod::Hardware2x2,
                "shadow-filter-temporal" => g.shadow_filter = ShadowFilteringMethod::Temporal,
                "full-detail-7x7" => g.full_detail = crate::full_detail::FullDetail::Seven,
                "full-detail-9x9" => g.full_detail = crate::full_detail::FullDetail::Nine,
                "distant-objects-8" => g.distant_objects = crate::distant_objects::DistantObjects::Eight,
                "distant-objects-12" => g.distant_objects = crate::distant_objects::DistantObjects::Twelve,
                // The switch alone is Normal, as an INI written while the row was a switch.
                "volumetric-clouds" | "volumetric-clouds-performance" | "volumetric-clouds-high" => {
                    use crate::volumetric_clouds::CloudLevel;
                    g.volumetric_clouds = true;
                    g.volumetric_clouds_level = match k {
                        "volumetric-clouds-performance" => CloudLevel::Performance,
                        "volumetric-clouds-high" => CloudLevel::High,
                        _ => CloudLevel::Normal,
                    };
                }
                "no-glow" => g.glow = false,
                "no-eye-adaptation" => g.eye_adaptation = false,
                "no-neon-glow" => g.neon_glow = false,
                "no-water-mirror" => g.water_mirror = false,
                "no-authored-materials" => g.authored_materials = false,
                "no-authored-reflections" => g.authored_reflections = false,
                _ => match g.switch(k) {
                    Some(on) => *on = true,
                    None => return Err(format!("--graphics-only {k}: expected one of {}, no-glow, no-eye-adaptation, no-neon-glow, no-water-mirror, no-authored-materials, no-authored-reflections", KEYS.join(", "))),
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
            (self.water_effects, "water caustics and river flow"),
            (!self.water_mirror, "no mirrored water"),
            (self.mesh_terrain_blend, "meshes blended into the ground"),
            (self.shadow_filter == ShadowFilteringMethod::Hardware2x2, "hardware 2x2 shadow filtering"),
            (self.shadow_filter == ShadowFilteringMethod::Temporal, "temporal shadow filtering"),
            (self.terrain_height_blend, "terrain height blending"),
            (self.terrain_anti_tiling, "terrain anti-tiling"),
            (self.terrain_parallax, "terrain parallax"),
            (self.cloud_shadows, "cloud shadows"),
            (self.terrain_shadows, "terrain shadows"),
            (self.heat_haze, "heat haze"),
            (self.desert_haze, "desert haze"),
            (!self.authored_materials, "no authored materials"),
            (!self.authored_reflections, "no authored reflections"),
            (self.full_detail != crate::full_detail::FullDetail::Off, self.full_detail.describe()),
            (self.distant_objects != crate::distant_objects::DistantObjects::Off, self.distant_objects.describe()),
            (self.volumetric_clouds, "volumetric clouds"),
            (self.volumetric_light, "volumetric light"),
            (self.aerial_perspective, "aerial perspective"),
            (self.night_sky_detail, "night sky detail"),
            (self.physical_pbr, "physical PBR lighting"),
            (self.distant_lights, "distant lights"),
        ];
        let mut on: Vec<&str> = names.iter().filter(|(on, _)| *on).map(|(_, n)| *n).collect();
        if self.fsr().is_some() {
            on.retain(|n| !matches!(*n, "FXAA" | "SMAA" | "temporal anti-aliasing"));
            on.push("FSR 3.1 upscaling");
        } else if self.upscaler.is_dlss() {
            on.retain(|n| !matches!(*n, "FXAA" | "SMAA" | "temporal anti-aliasing"));
            on.push("DLSS upscaling");
        }
        if on.is_empty() {
            "vanilla (no engine-level improvements)".to_string()
        } else {
            on.join(", ")
        }
    }
}

/// Shadow map side of every directional light by default, with longer shadows or without
/// (Bevy's default). 4096² with longer shadows cost about 2.1 ms a frame at Novac and 0.65
/// ms at Goodsprings for edges hardly sharper near the player [observed 2026-11-03, RTX
/// 4070, 1920x1080; owner's choice 2026-11-03, #1367: the smaller maps, for the frame
/// rate]. The Shadow resolution row ([`SHADOW_RESOLUTIONS`]) can choose 4096 in play
/// [engineering choice: the contributor's proposal in #2843, for the maintainer to decide;
/// observed 2026-11-16 with the cascades of that time: 0.2-0.3 ms a frame at Goodsprings,
/// RTX 5070 Ti, 2560 × 1440 (docs/design/frame-rate-audit.md §12)], and the sun's Quality
/// cascades are laid out for the side chosen (sky.rs `shadow_cascade_layout`).
const SHADOW_MAP: usize = 2048;

/// The settings page's Shadow resolution choices, in its order: the side of
/// `DirectionalLightShadowMap`, [`SHADOW_MAP`] first [engineering choice, #2843]. Bevy
/// keeps every directional light's cascades and every shadowed spot light's map as layers
/// of one 32-bit depth texture of that side (the vendored bevy_pbr light.rs
/// `prepare_lights`: as many layers as the most cascades any view's directional lights
/// have, plus one per shadowed spot light), so the row sizes the sun's cascades, the
/// first-person arms' shade light's cascade (player_body.rs) and the flashlight's shadow
/// (flashlight.rs). By arithmetic a layer of 4096² takes 64 MiB against 16 MiB at 2048²:
/// on Quality by day in first person, four sun cascades and the arms' one, about 240 MiB more; on Performance
/// (three, no arms shade light) about 144 MiB more; with the flashlight on 48 MiB more
/// again, and its shadow pass draws 4096² too. Not measured.
pub const SHADOW_RESOLUTIONS: [(&str, usize); 2] = [("2048", 2048), ("4096", 4096)];

/// The Shadow resolution row's choice now (an index into [`SHADOW_RESOLUTIONS`]; a side
/// not among them reads as the first).
pub fn shadow_resolution_choice(world: &World) -> usize {
    let side = crate::sky::map_side(world.get_resource::<DirectionalLightShadowMap>());
    SHADOW_RESOLUTIONS.iter().position(|&(_, s)| s == side).unwrap_or(0)
}

/// Choose Shadow resolution `i` of [`SHADOW_RESOLUTIONS`]: the maps change size now, and
/// the sun gets the cascades laid out for the new side at once (Quality's layout follows
/// the side: sky.rs `shadow_cascade_layout`; the moon's single cascade by night, nothing
/// while the light is faded or the shadows are off), so no frame draws the new maps with
/// the old bounds. Bevy makes new maps, `shadow_rate::hold_cascades` draws every cascade
/// that frame, and the anchors of shadow_anchor.rs start afresh for the new texel size.
/// `sun_shadow_quality` then sees the cascades put there and puts the same ones back. The
/// resource is written only when the side changes.
pub fn set_shadow_resolution_choice(world: &mut World, i: usize) {
    let side = SHADOW_RESOLUTIONS[i.min(SHADOW_RESOLUTIONS.len() - 1)].1;
    let mut map = world.get_resource_or_insert_with(|| DirectionalLightShadowMap { size: SHADOW_MAP });
    if map.size == side {
        return;
    }
    map.size = side;
    let Some(want) = world.get_resource::<SkyState>().and_then(|sky| shadow_key_of(sky, side).wanted()) else { return };
    let mut suns = world.query_filtered::<&mut CascadeShadowConfig, With<Sun>>();
    for mut c in suns.iter_mut(world) {
        *c = want.clone();
    }
}

/// Say the shadow maps' side at the start and whenever it changes (the Shadow resolution
/// row): `sun shadows: maps 2048 x 2048 texels`. PostUpdate, before Bevy builds the
/// cascades and so before `sky::log_sun_cascades` says their bounds: the side is said
/// before the bounds laid out for it.
pub(crate) fn log_shadow_map(map: Option<Res<DirectionalLightShadowMap>>) {
    if let Some(map) = map.filter(|m| m.is_changed()) {
        info!("sun shadows: maps {0} x {0} texels", map.size);
    }
}
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

/// Whether FSR draws now (the effective upscaler, `upscaler::effective`, is FSR): FSR's own
/// question (frame generation). What every temporal upscaler shares asks
/// `upscaler::temporal_active`. With DLSS asked for while FSR keeps drawing until a restart,
/// this stays true.
pub fn fsr_active(world: &World) -> bool {
    crate::upscaler::effective(world).fsr().is_some()
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
/// `shadow-distance=<units>`, `cascades=<n>` (n cascades in the spacing of before #2843:
/// [`apply_shadow_step`]), `shadow-thin=<ratio>` (otherwise as at the start).
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
    /// `cascades=<n>`: n cascades spaced as before #2843 (`sky::CascadeLayout::Spaced`).
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
            .add_plugins((crate::ssao::SsaoPlugin, crate::post::PostPlugin, crate::taa::TaaPlugin, crate::shafts::ShaftsPlugin, crate::heat_haze::HeatHazePlugin, crate::upscaler::prep::PrepPlugin, crate::fsr::FsrPlugin, crate::dlss::DlssPassPlugin))
            .add_systems(Startup, |g: Res<Graphics>| info!("graphics: {}", g.describe()))
            .add_systems(Update, (crate::upscaler::follow_request, apply, sun_shadow_key.pipe(sun_shadow_quality).after(crate::sky::update_sky)).chain())
            .add_systems(PostUpdate, log_shadow_map.before(bevy::light::SimulationLightSystems::UpdateDirectionalLightCascades));
        if let Some(ft) = self.2.clone().filter(|f| f.every > 0.0) {
            // GPU time per render pass (timestamp queries), logged with the frame time.
            ensure_render_diagnostics(app);
            // What the player's fixed steps cost, counted while this resource exists and
            // logged with the frame time (`nvf_play::phys_load`).
            app.init_resource::<nvf_play::phys_load::PhysLoad>();
            // What the GPU API holds, logged with the frame time: every command buffer costs
            // by how many buffers, textures and bind groups exist, so a count that only grows
            // over a session slows every frame (the playtest of 2026-11-24: a session an hour
            // long ran at two thirds of a fresh one's frame rate in the same room).
            let every = ft.every;
            if let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) {
                render_app.add_systems(bevy::render::Render, (move |instance: Res<bevy::render::renderer::RenderInstance>, mut last: Local<Option<std::time::Instant>>| {
                    let now = std::time::Instant::now();
                    if last.is_some_and(|l| now.duration_since(l).as_secs_f32() < every) {
                        return;
                    }
                    *last = Some(now);
                    if let Some(report) = instance.generate_report() {
                        let h = report.hub_report();
                        info!(
                            "gpu resources: {} buffers, {} textures, {} texture views, {} bind groups, {} bind group layouts, {} samplers, {} render pipelines, {} compute pipelines, {} shader modules, {} command buffers",
                            h.buffers.num_allocated, h.textures.num_allocated, h.texture_views.num_allocated, h.bind_groups.num_allocated, h.bind_group_layouts.num_allocated, h.samplers.num_allocated, h.render_pipelines.num_allocated, h.compute_pipelines.num_allocated, h.shader_modules.num_allocated, h.command_buffers.num_allocated
                        );
                    }
                }).in_set(bevy::render::RenderSystems::Cleanup));
            }
            // (time in this period, time counted, slowest, frames counted, cycle index)
            #[allow(clippy::type_complexity)]
            app.add_systems(Update, move |mut commands: Commands, time: Res<Time<Real>>, mut g: ResMut<Graphics>, sky: Res<SkyState>, store: Res<bevy::diagnostic::DiagnosticsStore>, lights: Query<(Option<&Name>, &DirectionalLight, &bevy::camera::visibility::CascadesVisibleEntities)>, views: Query<Option<&Name>, With<Camera>>, mut acc: Local<(f32, f32, f32, u32, usize)>, mut cpu_at: Local<Option<(f64, f64)>>, mut start_shadows: Local<Option<(bool, f32, crate::sky::CascadeLayout, f32)>>, thin: Res<crate::shadow_thin::ShadowThin>, refs: Query<&nvf_play::RefVisual>, mut phys_load: ResMut<nvf_play::phys_load::PhysLoad>| {
                let first = start_shadows.is_none();
                let start = *start_shadows.get_or_insert((sky.shadows, sky.shadow_distance, sky.shadow_layout, thin.ratio));
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
                    info!("frame time: {:.2} ms mean, {:.1} ms slowest over {} frames; graphics: {}; sun shadows {}; sun at heading {:.0}, elevation {:.0}", acc.1 * 1e3 / acc.3.max(1) as f32, acc.2 * 1e3, acc.3, g.describe(), if sky.shadows { format!("to {:.0} in {} cascades", sky.shadow_distance, sky.shadow_layout.count()) } else { "off".into() }, sky.sun_heading(), sky.sun_elevation());
                    // Processor time a frame over the period: this (the main) thread's and the
                    // whole process's, which a display's frame cap does not hide as it does
                    // the frame time.
                    if let (Some(main), Some(all)) = (crate::profile::thread_cpu_secs(), crate::profile::process_cpu_secs()) {
                        if let Some((m0, a0)) = cpu_at.replace((main, all)) {
                            let n = f64::from(acc.3.max(1));
                            info!("cpu time: main thread {:.2} ms a frame, process {:.2} ms a frame", (main - m0) * 1e3 / n, (all - a0) * 1e3 / n);
                        }
                    }
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

/// The sun's shadows of a `--graphics-cycle` set: `start` (on, distance, cascade layout,
/// thinning) as at the start, changed by the set's keys. `cascades=<n>` asks for `n`
/// cascades in the spacing every count had before #2843 (`sky::CascadeLayout::Spaced`),
/// so that `cascades=3` still measures the 3 cascades of before against the modes'
/// layouts; a set without it keeps the start's layout (Quality's, or Performance's with
/// `--sun-shadows=performance`).
fn apply_shadow_step(commands: &mut Commands, sky: &SkyState, start: (bool, f32, crate::sky::CascadeLayout, f32), step: &CycleStep) {
    let on = start.0 && !step.no_sun_shadows;
    let distance = step.shadow_distance.unwrap_or(start.1);
    let cascades = step.cascades.map_or(start.2, crate::sky::CascadeLayout::Spaced);
    let ratio = step.shadow_thin.unwrap_or(start.3);
    commands.queue(move |w: &mut World| {
        let mut thin = w.resource_mut::<crate::shadow_thin::ShadowThin>();
        if thin.ratio != ratio {
            thin.ratio = ratio;
        }
    });
    if on != sky.shadows || distance != sky.shadow_distance || cascades != sky.shadow_layout {
        commands.queue(move |w: &mut World| {
            let mut sky = w.resource_mut::<SkyState>();
            sky.shadow_distance = distance;
            sky.shadow_layout = cascades;
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
    cascades: crate::sky::CascadeLayout,
    /// The shadow maps' side (the Shadow resolution row): Quality's layout follows it.
    map: usize,
}

impl SunShadowKey {
    /// The cascades the sun casts under this key: by day the settings' layout, by night the
    /// moon's single cascade; none with the shadows off, a shadow distance of 0
    /// (`crate::sky::casts_sun_shadows`) or the light faded at the horizon.
    fn wanted(&self) -> Option<CascadeShadowConfig> {
        match self.phase {
            _ if !crate::sky::casts_sun_shadows(self.shadows, self.distance) => None,
            SunShadows::Faded => None,
            SunShadows::Moon => Some(crate::sky::shadow_cascades(MOON_SHADOW_DISTANCE.min(self.distance), 1.0, crate::sky::CascadeLayout::Spaced(1), self.map)),
            SunShadows::Day => Some(crate::sky::shadow_cascades(self.distance, self.reach, self.cascades, self.map)),
        }
    }
}

/// The phase for the sun's height now (the day's indoors) and the shadow settings, with
/// shadow maps `map` texels on a side.
fn shadow_key_of(sky: &SkyState, map: usize) -> SunShadowKey {
    let indoors = sky.interior.is_some();
    let (height, fade) = if indoors { (1.0, 1.0) } else { crate::sky::sun_height_and_fade(sky) };
    SunShadowKey {
        phase: SunShadows::at(indoors, height, fade),
        shadows: sky.shadows,
        distance: sky.shadow_distance,
        reach: sky.shadow_reach,
        cascades: sky.shadow_layout,
        map,
    }
}

/// [`shadow_key_of`] the sky now and the maps' side now.
fn sun_shadow_key(sky: Res<SkyState>, map: Option<Res<DirectionalLightShadowMap>>) -> SunShadowKey {
    shadow_key_of(&sky, crate::sky::map_side(map.as_deref()))
}

/// The sun's shadow quality for the hour: by day the settings' cascade layout, reach and
/// map size (Quality's layout follows the Shadow resolution row's maps);
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
    // None with the shadows off, the light faded or a shadow distance of 0% (the menu
    // overhaul): [`SunShadowKey::wanted`].
    let want = key.wanted();
    let on = want.is_some();
    for (e, mut light, cascades) in &mut suns {
        if light.shadow_maps_enabled != on {
            light.shadow_maps_enabled = on;
        }
        if let Some(want) = want.clone() {
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
///
/// What every temporal upscaler needs follows the effective upscaler (`temporal`: the HDR
/// view, the depth and motion prepasses, the jitter and mip bias, blended cut-outs drawn
/// alpha-tested, multisampling and the other anti-aliasing replaced); FSR's own sharpening and
/// component follow FSR alone, and DLSS's component DLSS alone
/// (`upscaler::effective_temporal`).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn apply(
    mut commands: Commands,
    g: Res<Graphics>,
    msaa: Res<MsaaSetting>,
    fsr_support: Res<crate::fsr::FsrSupport>,
    status: Option<Res<UpscalerStatus>>,
    mut sky: ResMut<SkyState>,
    cams: Query<(Entity, &Msaa, Option<&FsrUpscaling>, Option<&DlssUpscaling>, Has<crate::taa::TemporalAa>, Has<Fxaa>, Has<Smaa>), With<FlyCam>>,
    suns: Query<(Entity, &DirectionalLight), With<Sun>>,
    eye: Res<crate::eye_adapt::EyeCurve>,
    cutouts: Option<ResMut<nvf_assets::nif_material::BlendedCutouts>>,
    map: Option<Res<DirectionalLightShadowMap>>,
    mut done: Local<Option<(Graphics, Msaa, bool, UpscalerChoice, Vec<Entity>, bool)>>,
    work: Option<ResMut<crate::quality_probe::Work>>,
    air: Option<Res<crate::medium_air_demand::AirWanted>>,
) {
    let cam_count = cams.iter().count();
    if cam_count == 0 {
        return;
    }
    let upscaler = crate::upscaler::effective_temporal(g.fsr(), fsr_support.0, status.as_deref());
    let (fsr, dlss) = (upscaler.fsr(), upscaler.dlss());
    let temporal = upscaler != UpscalerChoice::Off;
    // Blended shapes that draw the same as cut-outs (the corn rows) are cut-outs for a
    // temporal upscaler, which needs their depth and motion (nif_material::BlendedCutouts).
    if let Some(mut cutouts) = cutouts {
        cutouts.set_if_neq(nvf_assets::nif_material::BlendedCutouts(temporal));
    }
    let want_msaa = if g.screen_aa == ScreenAa::Off && !temporal { msaa.0 } else { Msaa::Off };
    let aa_matches = cams.iter().all(|(_, actual_msaa, actual_fsr, actual_dlss, taa, fxaa, smaa)| {
        *actual_msaa == want_msaa && actual_fsr.map(|f| f.quality) == fsr && actual_dlss.map(|d| d.quality) == dlss
            && taa == (g.screen_aa == ScreenAa::Taa && !temporal)
            && fxaa == (g.screen_aa == ScreenAa::Fxaa && !temporal)
            && smaa == (g.screen_aa == ScreenAa::Smaa && !temporal)
    });
    let mut entities: Vec<Entity> = cams.iter().map(|c| c.0).chain(suns.iter().map(|s| s.0)).collect();
    entities.sort_unstable();
    // The light in the air (medium_air_demand.rs) is composed in the HDR picture too; absent
    // (tests, or before its plugin) it counts as off.
    let air = air.is_some_and(|a| a.0);
    if aa_matches && done.as_ref().is_some_and(|(d, m, supported, effective, previous, was_air)| *d == *g && *m == msaa.0 && *supported == fsr_support.0 && *effective == upscaler && *previous == entities && *was_air == air) {
        return;
    }
    let first = done.is_none();
    let msaa_changed = done.as_ref().is_some_and(|(previous, saved, _, effective, ..)| {
        let previous_effective = if previous.screen_aa == ScreenAa::Off && *effective == UpscalerChoice::Off { *saved } else { Msaa::Off };
        previous_effective != want_msaa || *saved != msaa.0
    });
    let fsr_changed = done.as_ref().is_none_or(|(_, _, _, effective, ..)| effective.fsr() != fsr);
    let dlss_changed = done.as_ref().is_none_or(|(_, _, _, effective, ..)| effective.dlss() != dlss);
    *done = Some((*g, msaa.0, fsr_support.0, upscaler, entities, air));
    if let Some(mut work) = work {
        work.graphics += 1;
    }
    for (cam, cam_msaa, ..) in &cams {
        let mut e = commands.entity(cam);
        if g.ambient_occlusion {
            e.insert(AmbientOcclusion::at(g.ambient_occlusion_level));
        } else {
            e.remove::<AmbientOcclusion>();
        }
        // FSR already runs its own RCAS sharpening; do not sharpen the picture twice.
        if g.bloom || (g.sharpen && fsr.is_none()) {
            e.insert(PostEffects::new_with_hdr_glow(g.bloom, g.sharpen && fsr.is_none(), nvf_assets::authored::physical_pbr() && g.glow));
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
        // HDR output (hdr.rs) keeps the main view in fp16 so its highlights band no more
        // than the screen's.
        // The volumetric medium's far buffer is composed in the linear HDR picture before
        // tonemapping (medium.rs, volumetric_clouds.rs), so its row keeps the view HDR whatever
        // the glow and the upscaler say [design, #3032 decision 10]; the light in the air of
        // stage 3 likewise.
        // Physical lighting works on linear light up to the tonemapper (§3.25).
        if temporal || g.volumetric_clouds || crate::hdr::on() || air || nvf_assets::authored::physical_pbr() {
            e.insert(bevy::camera::Hdr);
        }
        // Eye adaptation (eye_adapt.rs) and the bright pass need the HDR view; each has
        // its own switch.
        if (g.glow || nvf_assets::authored::physical_pbr()) && g.eye_adaptation {
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
        if g.screen_aa == ScreenAa::Taa && !temporal {
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
            e.remove::<FsrUpscaling>();
        }
        if let Some(quality) = dlss {
            e.insert((
                DlssUpscaling { quality },
                bevy::core_pipeline::prepass::DepthPrepass,
                bevy::core_pipeline::prepass::MotionVectorPrepass,
                TemporalJitter::default(),
                MipBias(-1.0),
            ));
        } else {
            e.remove::<DlssUpscaling>();
        }
        if !temporal {
            e.remove::<bevy::core_pipeline::prepass::MotionVectorPrepass>();
        }
        if g.screen_aa == ScreenAa::Fxaa && !temporal {
            e.insert(Fxaa::default());
        } else {
            e.remove::<Fxaa>();
        }
        if g.screen_aa == ScreenAa::Smaa && !temporal {
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
        if g.heat_haze {
            e.insert(crate::heat_haze::HeatHaze::default());
        } else {
            e.remove::<crate::heat_haze::HeatHaze>();
        }
    }
    let reach = if g.fine_shadows { FINE_SHADOW_REACH } else { 1.0 };
    let reach_changed = sky.shadow_reach != reach;
    if reach_changed {
        sky.shadow_reach = reach;
    }
    for (e, light) in &suns {
        if light.shadow_maps_enabled && (reach_changed || first) {
            commands.entity(e).insert(crate::sky::shadow_cascades(sky.shadow_distance, sky.shadow_reach, sky.shadow_layout, crate::sky::map_side(map.as_deref())));
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
    if let Some(quality) = dlss.filter(|_| dlss_changed) {
        info!("graphics: DLSS {quality:?} active; native anti-aliasing off");
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

                app.world_mut().resource_mut::<Graphics>().set_fsr(Some(quality));
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

                app.world_mut().resource_mut::<Graphics>().set_fsr(None);
                app.update();
                assert_native_aa(&app, camera, selected);
            }
        }
    }

    #[test]
    fn dlss_owns_aa_like_fsr_without_fsr_components_and_off_restores_the_saved_native_choice() {
        use crate::upscaler::{DlssState, Session, UpscaleQuality};
        let ready = Session { streamline_loaded: true, dlss: DlssState::Ready, fsr: None };
        for selected in [ScreenAa::Off, ScreenAa::Taa, ScreenAa::Fxaa, ScreenAa::Smaa] {
            let (mut app, camera) = aa_app(selected, true);
            app.update();
            assert_native_aa(&app, camera, selected);

            let mut status = UpscalerStatus::new(ready);
            status.update(UpscalerChoice::Dlss(UpscaleQuality::Balanced));
            app.insert_resource(status);
            app.world_mut().resource_mut::<Graphics>().upscaler = UpscalerChoice::Dlss(UpscaleQuality::Balanced);
            app.update();
            let dlss_drawn = |app: &App| {
                let entity = app.world().entity(camera);
                assert_eq!(entity.get::<DlssUpscaling>().unwrap().quality, UpscaleQuality::Balanced);
                assert!(!entity.contains::<FsrUpscaling>(), "no FSR component while DLSS draws");
                assert_eq!(*entity.get::<Msaa>().unwrap(), Msaa::Off);
                assert!(!entity.contains::<crate::taa::TemporalAa>() && !entity.contains::<Fxaa>() && !entity.contains::<Smaa>());
                assert!(entity.contains::<TemporalJitter>() && entity.contains::<MipBias>());
                assert!(entity.contains::<bevy::core_pipeline::prepass::MotionVectorPrepass>());
                assert!(entity.contains::<bevy::core_pipeline::prepass::DepthPrepass>());
                assert!(entity.contains::<bevy::camera::Hdr>());
            };
            dlss_drawn(&app);
            assert_eq!(app.world().resource::<Graphics>().screen_aa, selected, "DLSS preserves the saved native AA choice");

            // FSR asked for while DLSS draws (Streamline loaded): FSR at once, never both.
            app.world_mut().resource_mut::<Graphics>().set_fsr(Some(FsrQuality::Quality));
            app.world_mut().resource_mut::<UpscalerStatus>().update(UpscalerChoice::Fsr(FsrQuality::Quality));
            app.update();
            let entity = app.world().entity(camera);
            assert_eq!(entity.get::<FsrUpscaling>().unwrap().quality, FsrQuality::Quality);
            assert!(!entity.contains::<DlssUpscaling>(), "no DLSS component while FSR draws");
            assert_eq!(*entity.get::<Msaa>().unwrap(), Msaa::Off);

            // Back to DLSS, then off: the saved native choice is back at once.
            app.world_mut().resource_mut::<Graphics>().upscaler = UpscalerChoice::Dlss(UpscaleQuality::Balanced);
            app.world_mut().resource_mut::<UpscalerStatus>().update(UpscalerChoice::Dlss(UpscaleQuality::Balanced));
            app.update();
            dlss_drawn(&app);
            app.world_mut().resource_mut::<Graphics>().set_fsr(None);
            app.world_mut().resource_mut::<UpscalerStatus>().update(UpscalerChoice::Off);
            app.update();
            assert_native_aa(&app, camera, selected);
            assert!(!app.world().entity(camera).contains::<DlssUpscaling>() && !app.world().entity(camera).contains::<FsrUpscaling>());
        }
    }

    /// Review P2 (2026-11-12): DLSS asked for while FSR draws in a session that started without
    /// Streamline (the files added since, or `NVF_DLSS_PRELOAD=0`): FSR keeps drawing (and
    /// frame generation's question stays yes) until the restart.
    #[test]
    fn fsr_keeps_drawing_while_dlss_waits_for_a_restart() {
        use crate::upscaler::{DlssState, Session, UpscaleQuality};
        let (mut app, camera) = aa_app(ScreenAa::Smaa, true);
        let stock = Session { streamline_loaded: false, dlss: DlssState::NotLoaded, fsr: None };
        let mut status = UpscalerStatus::new(stock);
        status.update(UpscalerChoice::Fsr(UpscaleQuality::Balanced));
        app.insert_resource(status);
        app.world_mut().resource_mut::<Graphics>().set_fsr(Some(UpscaleQuality::Balanced));
        app.update();
        assert_eq!(app.world().entity(camera).get::<FsrUpscaling>().unwrap().quality, UpscaleQuality::Balanced);

        app.world_mut().resource_mut::<Graphics>().upscaler = UpscalerChoice::Dlss(UpscaleQuality::Quality);
        app.world_mut().resource_mut::<UpscalerStatus>().update(UpscalerChoice::Dlss(UpscaleQuality::Quality));
        assert!(app.world().resource::<UpscalerStatus>().restart_pending());
        app.update();
        let entity = app.world().entity(camera);
        assert_eq!(entity.get::<FsrUpscaling>().unwrap().quality, UpscaleQuality::Balanced, "FSR keeps drawing until the restart");
        assert!(!entity.contains::<DlssUpscaling>() && !entity.contains::<Smaa>());
        assert!(fsr_active(app.world()) && crate::upscaler::temporal_active(app.world()));
        assert_eq!(app.world().resource::<Graphics>().fsr(), None, "the request is DLSS");
    }

    /// The setting's DLSS, effective: the camera gets DLSS's component alone, the common
    /// preparation reads it as DLSS's (its policy from the size DLSS reports, no FSR state), the
    /// separate sharpening runs as it does natively (FSR's own is not there), and once DLSS's
    /// pass gives up (`EvaluateFailed`) the camera draws natively, not with FSR.
    #[test]
    fn a_dlss_camera_from_the_setting_gets_dlsss_policy_and_no_fsr_and_draws_natively_once_dlss_fails() {
        use crate::upscaler::prep::{self, UpscaleSettings};
        use crate::upscaler::{DlssState, Session, Unavailable};
        let (mut app, camera) = aa_app(ScreenAa::Fxaa, true);
        let requested = UpscalerChoice::Dlss(UpscaleQuality::Performance);
        {
            let mut g = app.world_mut().resource_mut::<Graphics>();
            g.sharpen = true;
            g.upscaler = requested;
        }
        let ready = Session { streamline_loaded: true, dlss: DlssState::Ready, fsr: None };
        let mut status = UpscalerStatus::new(ready);
        status.update(requested);
        app.insert_resource(status);
        app.update();
        let entity = app.world().entity(camera);
        assert!(!entity.contains::<FsrUpscaling>(), "no FSR component");
        assert!(!fsr_active(app.world()) && crate::upscaler::temporal_active(app.world()));
        let settings = prep::camera_settings(entity.get::<FsrUpscaling>(), entity.get::<DlssUpscaling>(), true).expect("a temporal camera");
        assert!(matches!(settings, UpscaleSettings::Dlss(UpscaleQuality::Performance)));
        assert!(!prep::allocates_fsr_state(settings.upscaler()), "the common prepared view only");
        let output = UVec2::new(1280, 720);
        let optimal = crate::dlss::sizing::Optimal { render: UVec2::new(640, 360), min: UVec2::new(640, 360), max: UVec2::new(640, 360) };
        let policy = prep::policy(settings, output, &wgpu::Limits::default(), |quality| {
            assert_eq!(quality, UpscaleQuality::Performance);
            Some(optimal)
        })
        .expect("DLSS reported a size");
        assert_eq!(policy.render_size, optimal.render, "the size DLSS reports, not FSR's ratio");
        assert!(entity.get::<PostEffects>().is_some_and(|p| p.params.w > 0.0), "the separate sharpening, as natively");

        // DLSS's pass gives up: what follow_request makes of the session from then on.
        {
            let mut status = app.world_mut().resource_mut::<UpscalerStatus>();
            status.session.dlss = DlssState::Unavailable(Unavailable::EvaluateFailed);
            status.update(requested);
        }
        app.update();
        assert_native_aa(&app, camera, ScreenAa::Fxaa);
        assert!(!app.world().entity(camera).contains::<DlssUpscaling>());
        assert!(!crate::upscaler::temporal_active(app.world()) && !fsr_active(app.world()), "native, not FSR");
        assert_eq!(app.world().resource::<UpscalerStatus>().reason(), Some(Unavailable::EvaluateFailed));
    }

    #[test]
    fn fsr_aa_is_applied_without_a_sun_to_replacement_cameras_and_repairs_conflicting_components() {
        let (mut app, old) = aa_app(ScreenAa::Off, true);
        let suns: Vec<Entity> = app.world_mut().query_filtered::<Entity, With<Sun>>().iter(app.world()).collect();
        for sun in suns { app.world_mut().despawn(sun); }
        app.world_mut().resource_mut::<Graphics>().set_fsr(Some(FsrQuality::NativeAa));
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
            app.world_mut().resource_mut::<Graphics>().set_fsr(Some(FsrQuality::Quality));
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
        app.insert_resource(TestKey(SunShadowKey { phase, shadows: true, distance: 8000.0, reach: 1.0, cascades: crate::sky::CascadeLayout::Quality, map: 2048 }))
            .add_systems(Update, (|k: Res<TestKey>| k.0).pipe(sun_shadow_quality));
        app
    }

    /// A sun as `setup_sky` spawns it: shadows on, the day's cascades.
    fn spawn_day_sun(app: &mut App) -> Entity {
        let day = crate::sky::shadow_cascades(8000.0, 1.0, crate::sky::CascadeLayout::Quality, 2048);
        app.world_mut().spawn((Sun, DirectionalLight { shadow_maps_enabled: true, ..default() }, day)).id()
    }

    fn bounds(app: &App, e: Entity) -> Vec<f32> {
        app.world().get::<CascadeShadowConfig>(e).expect("cascades").bounds.clone()
    }

    #[test]
    fn a_new_sun_or_new_cascades_get_the_phase_again() {
        let mut app = shadow_app(SunShadows::Moon);
        let moon = crate::sky::shadow_cascades(MOON_SHADOW_DISTANCE, 1.0, crate::sky::CascadeLayout::Spaced(1), 2048).bounds;
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
        app.world_mut().entity_mut(second).insert(crate::sky::shadow_cascades(8000.0, 1.0, crate::sky::CascadeLayout::Quality, 2048));
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

    /// The Shadow resolution row's choices set the maps' side, and read back; a world
    /// without the resource reads the first choice and gets one when a choice is made.
    #[test]
    fn shadow_resolution_choices_set_the_map_side() {
        let mut world = World::new();
        assert_eq!(shadow_resolution_choice(&world), 0);
        set_shadow_resolution_choice(&mut world, 1);
        assert_eq!(world.resource::<DirectionalLightShadowMap>().size, 4096);
        assert_eq!(shadow_resolution_choice(&world), 1);
        set_shadow_resolution_choice(&mut world, 0);
        assert_eq!(world.resource::<DirectionalLightShadowMap>().size, SHADOW_MAP);
        assert_eq!(shadow_resolution_choice(&world), 0);
        // Past the end: the last choice.
        set_shadow_resolution_choice(&mut world, 9);
        assert_eq!(world.resource::<DirectionalLightShadowMap>().size, 4096);
        assert_eq!(SHADOW_RESOLUTIONS[0].1, SHADOW_MAP, "the default first");
    }

    /// The Shadow resolution row puts the cascades laid out for the new side on the sun at
    /// once, by day and by night; with the shadows off it leaves them alone.
    #[test]
    fn a_new_resolution_puts_its_cascades_on_the_sun_at_once() {
        use crate::sky::CascadeLayout;
        let mut world = World::new();
        // The test build's derived default leaves the distance and reach at 0 (the game's
        // SkyState starts at DEFAULT_SHADOW_DISTANCE and a reach of 1).
        let mut sky = SkyState::default();
        sky.shadows = true;
        sky.shadow_distance = crate::sky::DEFAULT_SHADOW_DISTANCE;
        sky.shadow_reach = 1.0;
        // Indoors the sun casts the day's cascades whatever the hour.
        sky.interior = Some(Default::default());
        world.insert_resource(sky);
        world.insert_resource(DirectionalLightShadowMap { size: SHADOW_MAP });
        let day = |map| crate::sky::shadow_cascades(crate::sky::DEFAULT_SHADOW_DISTANCE, 1.0, CascadeLayout::Quality, map);
        let sun = world.spawn((Sun, DirectionalLight { shadow_maps_enabled: true, ..default() }, day(2048))).id();
        let config = |w: &World| {
            let c = w.get::<CascadeShadowConfig>(sun).unwrap();
            (c.bounds.clone(), c.overlap_proportion)
        };
        set_shadow_resolution_choice(&mut world, 1);
        assert_eq!(config(&world), (day(4096).bounds, day(4096).overlap_proportion));
        set_shadow_resolution_choice(&mut world, 0);
        assert_eq!(config(&world), (day(2048).bounds, day(2048).overlap_proportion));
        // With the shadows off the map changes and the cascades stay.
        world.resource_mut::<SkyState>().shadows = false;
        set_shadow_resolution_choice(&mut world, 1);
        assert_eq!(world.resource::<DirectionalLightShadowMap>().size, 4096);
        assert_eq!(config(&world).0, day(2048).bounds);
        // The key's cascades: none faded or off, the moon's single one by night.
        let key = SunShadowKey { phase: SunShadows::Moon, shadows: true, distance: 8000.0, reach: 1.5, cascades: CascadeLayout::Quality, map: 4096 };
        assert_eq!(key.wanted().unwrap().bounds, vec![MOON_SHADOW_DISTANCE]);
        assert!(SunShadowKey { phase: SunShadows::Faded, ..key }.wanted().is_none());
        assert!(SunShadowKey { shadows: false, phase: SunShadows::Day, ..key }.wanted().is_none());
        // A shadow distance of 0 (the Shadow distance row's 0%) casts none, by day or night;
        // its least step above 0 (a fifth of 8,000) casts the layout.
        assert!(SunShadowKey { distance: 0.0, phase: SunShadows::Day, ..key }.wanted().is_none());
        assert!(SunShadowKey { distance: 0.0, ..key }.wanted().is_none());
        let short = SunShadowKey { distance: 1600.0, reach: 1.0, phase: SunShadows::Day, ..key };
        assert_eq!(short.wanted().unwrap().bounds, crate::sky::shadow_cascades(1600.0, 1.0, CascadeLayout::Quality, 4096).bounds);
        // Choosing a resolution at a distance of 0 leaves the cascades alone.
        world.resource_mut::<SkyState>().shadows = true;
        world.resource_mut::<SkyState>().shadow_distance = 0.0;
        set_shadow_resolution_choice(&mut world, 0);
        assert_eq!(config(&world).0, day(2048).bounds);
    }

    /// The Shadow resolution row changes the key's map size: Quality's layout follows it
    /// without anything else changing, Performance's stays.
    #[test]
    fn quality_cascades_follow_the_map_size() {
        use crate::sky::CascadeLayout;
        let mut app = shadow_app(SunShadows::Day);
        let sun = spawn_day_sun(&mut app);
        app.update();
        assert_eq!(bounds(&app, sun), crate::sky::shadow_cascades(8000.0, 1.0, CascadeLayout::Quality, 2048).bounds);
        app.world_mut().resource_mut::<TestKey>().0.map = 4096;
        app.update();
        let wide = crate::sky::shadow_cascades(8000.0, 1.0, CascadeLayout::Quality, 4096);
        assert_eq!(bounds(&app, sun), wide.bounds);
        assert_eq!(app.world().get::<CascadeShadowConfig>(sun).unwrap().overlap_proportion, wide.overlap_proportion);
        app.world_mut().resource_mut::<TestKey>().0.cascades = CascadeLayout::Performance;
        app.update();
        assert_eq!(bounds(&app, sun), crate::sky::shadow_cascades(8000.0, 1.0, CascadeLayout::Performance, 2048).bounds);
    }

    /// A shadow distance of 0 (the Shadow distance row's 0%) switches the sun's shadow maps
    /// off and leaves its cascades; the row's least step above 0 brings them back.
    #[test]
    fn a_shadow_distance_of_zero_casts_none() {
        use crate::sky::CascadeLayout;
        let mut app = shadow_app(SunShadows::Day);
        let sun = spawn_day_sun(&mut app);
        app.world_mut().resource_mut::<TestKey>().0.distance = 0.0;
        app.update();
        assert!(!app.world().get::<DirectionalLight>(sun).unwrap().shadow_maps_enabled);
        assert_eq!(bounds(&app, sun), crate::sky::shadow_cascades(8000.0, 1.0, CascadeLayout::Quality, 2048).bounds);
        app.world_mut().resource_mut::<TestKey>().0.distance = 1600.0;
        app.update();
        assert!(app.world().get::<DirectionalLight>(sun).unwrap().shadow_maps_enabled);
        assert_eq!(bounds(&app, sun), crate::sky::shadow_cascades(1600.0, 1.0, CascadeLayout::Quality, 2048).bounds);
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
        assert_eq!(bounds(&app, sun), crate::sky::shadow_cascades(8000.0, 1.0, crate::sky::CascadeLayout::Quality, 2048).bounds);
        // And back out into the night.
        app.world_mut().resource_mut::<TestKey>().0.phase = SunShadows::at(false, -0.5, 1.0);
        app.update();
        assert_eq!(bounds(&app, sun).len(), 1);
    }

    #[test]
    fn every_key_is_a_switch_and_enhanced_leaves_taa_off() {
        let all = Graphics::only(&KEYS.map(String::from)).expect("keys");
        // Of the three anti-aliasings, the two shadow filters, the two full-detail sizes, the
        // two distant-object reaches and the clouds' three levels the last named is on.
        assert_eq!(all.describe().split(", ").count(), KEYS.len() + 1 - 7);
        assert_eq!(all.volumetric_clouds_level, crate::volumetric_clouds::CloudLevel::High);
        assert_eq!(all.full_detail, crate::full_detail::FullDetail::Nine);
        assert_eq!(all.distant_objects, crate::distant_objects::DistantObjects::Twelve);
        assert_eq!((all.screen_aa, all.shadow_filter), (ScreenAa::Taa, ShadowFilteringMethod::Temporal));
        let enhanced = Graphics::preset(Preset::Enhanced);
        assert!(enhanced.screen_aa == ScreenAa::Off && enhanced.light_shafts && enhanced.water_reflections && enhanced.water_effects);
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
    fn authored_materials_are_on_in_both_looks_and_can_be_switched_off() {
        assert!(Graphics::preset(Preset::Vanilla).authored_materials && Graphics::preset(Preset::Enhanced).authored_materials);
        let off = Graphics::only(&["no-authored-materials".into()]).expect("no-authored-materials");
        assert!(!off.authored_materials);
        assert_eq!(off.describe(), "light glow, no authored materials");
        assert!(Graphics::only(&[]).unwrap().authored_materials, "--graphics-only leaves it on unless told");
        let mut g = Graphics::preset(Preset::Vanilla);
        *g.switch(AUTHORED_MATERIALS_KEY).unwrap() = false;
        assert!(!g.authored_materials);
    }

    #[test]
    fn authored_reflections_are_on_in_both_looks_and_can_be_switched_off() {
        assert!(Graphics::preset(Preset::Vanilla).authored_reflections && Graphics::preset(Preset::Enhanced).authored_reflections);
        let off = Graphics::only(&["no-authored-reflections".into()]).expect("no-authored-reflections");
        assert!(!off.authored_reflections && off.authored_materials, "the materials row stays on");
        assert_eq!(off.describe(), "light glow, no authored reflections");
        let only = Graphics::only(&["authored-materials".into()]).unwrap();
        assert!(only.authored_reflections, "--graphics-only leaves it on unless told");
        let mut g = Graphics::preset(Preset::Vanilla);
        *g.switch(AUTHORED_REFLECTIONS_KEY).unwrap() = false;
        assert!(!g.authored_reflections);
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

    /// #2661: the mesh terrain blend is off in both looks, so the enhanced look leaves it
    /// out, and `--graphics-only` and the row's key switch it.
    #[test]
    fn mesh_terrain_blend_is_off_in_both_looks_and_not_enhanced() {
        assert!(!Graphics::preset(Preset::Vanilla).mesh_terrain_blend);
        assert!(!Graphics::preset(Preset::Enhanced).mesh_terrain_blend);
        assert!(!crate::quality::enhanced_keys().contains(&"mesh-terrain-blend"), "not an enhanced effect");
        let on = Graphics::only(&["mesh-terrain-blend".into()]).expect("a --graphics-only key");
        assert!(on.mesh_terrain_blend);
        assert_eq!(on.describe(), "light glow, meshes blended into the ground");
        let mut g = Graphics::preset(Preset::Vanilla);
        *g.switch("mesh-terrain-blend").expect("settings key") = true;
        assert_eq!(g, on);
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

    #[test]
    fn terrain_height_blending_is_off_in_both_looks_and_has_its_own_key() {
        assert!(!Graphics::preset(Preset::Vanilla).terrain_height_blend && !Graphics::preset(Preset::Enhanced).terrain_height_blend);
        let on = Graphics::only(&["terrain-height-blend".into()]).expect("terrain-height-blend");
        assert!(on.terrain_height_blend);
        assert_eq!(on.describe(), "light glow, terrain height blending");
        let mut g = Graphics::preset(Preset::Vanilla);
        *g.switch("terrain-height-blend").expect("settings key") = true;
        assert_eq!(g, on);
        assert!(!Graphics::preset(Preset::Enhanced).describe().contains("terrain"));
    }

    #[test]
    fn cloud_shadows_are_off_in_both_looks_and_have_their_own_key() {
        assert!(!Graphics::preset(Preset::Vanilla).cloud_shadows && !Graphics::preset(Preset::Enhanced).cloud_shadows);
        let on = Graphics::only(&["cloud-shadows".into()]).expect("cloud-shadows");
        assert!(on.cloud_shadows && !on.terrain_anti_tiling);
        assert_eq!(on.describe(), "light glow, cloud shadows");
        let mut g = Graphics::preset(Preset::Vanilla);
        *g.switch("cloud-shadows").expect("settings key") = true;
        assert_eq!(g, on);
        let cycle = FrameTimes::parse_cycle("vanilla;cloud-shadows").expect("cycle");
        assert!(!cycle[0].graphics.cloud_shadows && cycle[1].graphics.cloud_shadows);
    }

    #[test]
    fn terrain_shadows_are_off_in_both_looks_and_have_their_own_key() {
        assert!(!Graphics::preset(Preset::Vanilla).terrain_shadows && !Graphics::preset(Preset::Enhanced).terrain_shadows);
        let on = Graphics::only(&["terrain-shadows".into()]).expect("terrain-shadows");
        assert!(on.terrain_shadows && !on.cloud_shadows);
        assert_eq!(on.describe(), "light glow, terrain shadows");
        let mut g = Graphics::preset(Preset::Vanilla);
        *g.switch("terrain-shadows").expect("settings key") = true;
        assert_eq!(g, on);
        let cycle = FrameTimes::parse_cycle("vanilla;terrain-shadows").expect("cycle");
        assert!(!cycle[0].graphics.terrain_shadows && cycle[1].graphics.terrain_shadows);
    }

    #[test]
    fn terrain_anti_tiling_is_off_in_both_looks_and_has_its_own_key() {
        assert!(!Graphics::preset(Preset::Vanilla).terrain_anti_tiling && !Graphics::preset(Preset::Enhanced).terrain_anti_tiling);
        let on = Graphics::only(&["terrain-anti-tiling".into()]).expect("terrain-anti-tiling");
        assert!(on.terrain_anti_tiling && !on.terrain_height_blend);
        assert_eq!(on.describe(), "light glow, terrain anti-tiling");
        let both = Graphics::only(&["terrain-height-blend".into(), "terrain-anti-tiling".into()]).expect("both");
        assert_eq!(both.describe(), "light glow, terrain height blending, terrain anti-tiling");
        let mut g = Graphics::preset(Preset::Vanilla);
        *g.switch("terrain-anti-tiling").expect("settings key") = true;
        assert_eq!(g, on);
    }

    #[test]
    fn terrain_parallax_is_off_in_both_looks_and_has_its_own_key() {
        assert!(!Graphics::preset(Preset::Vanilla).terrain_parallax && !Graphics::preset(Preset::Enhanced).terrain_parallax);
        let on = Graphics::only(&["terrain-parallax".into()]).expect("terrain-parallax");
        assert!(on.terrain_parallax && !on.terrain_anti_tiling && !on.terrain_height_blend);
        assert_eq!(on.describe(), "light glow, terrain parallax");
        let mut g = Graphics::preset(Preset::Vanilla);
        *g.switch("terrain-parallax").expect("settings key") = true;
        assert_eq!(g, on);
    }

    #[test]
    fn heat_haze_is_off_in_both_looks_and_has_its_own_key() {
        assert!(!Graphics::preset(Preset::Vanilla).heat_haze && !Graphics::preset(Preset::Enhanced).heat_haze);
        let on = Graphics::only(&["heat-haze".into()]).expect("heat-haze");
        assert!(on.heat_haze);
        assert_eq!(on.describe(), "light glow, heat haze");
        let mut g = Graphics::preset(Preset::Vanilla);
        *g.switch("heat-haze").expect("settings key") = true;
        assert_eq!(g, on);
        assert!(!Graphics::preset(Preset::Enhanced).describe().contains("heat haze"));
    }

    #[test]
    fn desert_haze_is_off_in_both_looks_and_has_its_own_key() {
        assert!(!Graphics::preset(Preset::Vanilla).desert_haze && !Graphics::preset(Preset::Enhanced).desert_haze);
        let on = Graphics::only(&["desert-haze".into()]).expect("desert-haze");
        assert!(on.desert_haze);
        assert_eq!(on.describe(), "light glow, desert haze");
        let mut g = Graphics::preset(Preset::Vanilla);
        *g.switch("desert-haze").expect("settings key") = true;
        assert_eq!(g, on);
        assert!(!Graphics::preset(Preset::Enhanced).describe().contains("haze"));
    }

    #[test]
    fn volumetric_clouds_are_off_in_both_looks_and_have_their_own_key() {
        assert!(!Graphics::preset(Preset::Vanilla).volumetric_clouds && !Graphics::preset(Preset::Enhanced).volumetric_clouds);
        let on = Graphics::only(&["volumetric-clouds".into()]).expect("volumetric-clouds");
        assert!(on.volumetric_clouds && !on.cloud_shadows, "the volume does not switch Cloud shadows on");
        assert_eq!(on.describe(), "light glow, volumetric clouds");
        let mut g = Graphics::preset(Preset::Vanilla);
        *g.switch("volumetric-clouds").expect("settings key") = true;
        assert_eq!(g, on);
        // The levels (2026-11-23): the switch alone is Normal; the last named wins.
        use crate::volumetric_clouds::CloudLevel;
        let level = |keys: &[&str]| {
            let g = Graphics::only(&keys.iter().map(|k| (*k).to_string()).collect::<Vec<_>>()).expect("keys");
            (g.volumetric_clouds, g.volumetric_clouds_level, g.describe())
        };
        let words = || "light glow, volumetric clouds".to_string();
        assert_eq!(level(&["volumetric-clouds"]), (true, CloudLevel::Normal, words()));
        assert_eq!(level(&["volumetric-clouds-performance"]), (true, CloudLevel::Performance, words()));
        assert_eq!(level(&["volumetric-clouds-performance", "volumetric-clouds-high"]), (true, CloudLevel::High, words()));
        assert_eq!(level(&["volumetric-clouds-high", "volumetric-clouds"]), (true, CloudLevel::Normal, words()));
        assert!(!Graphics::preset(Preset::Enhanced).describe().contains("volumetric"));
    }

    /// The volumetric medium is composed in the HDR picture: with Light glow off and no
    /// upscaler the row alone makes the main view HDR, and switching it off takes it away
    /// again while the game runs (#3032 decision 10).
    #[test]
    fn volumetric_clouds_keep_the_view_hdr_with_glow_and_upscaler_off() {
        let _hdr = crate::hdr::test_lock();
        for selected in [ScreenAa::Off, ScreenAa::Taa] {
            let (mut app, camera) = aa_app(selected, true);
            app.world_mut().resource_mut::<Graphics>().glow = false;
            app.update();
            assert!(!app.world().entity(camera).contains::<bevy::camera::Hdr>(), "{selected:?}: glow off, no upscaler: not HDR");
            app.world_mut().resource_mut::<Graphics>().volumetric_clouds = true;
            app.update();
            assert!(app.world().entity(camera).contains::<bevy::camera::Hdr>(), "{selected:?}: the row makes the view HDR");
            assert!(!app.world().entity(camera).contains::<bevy::post_process::bloom::Bloom>(), "{selected:?}: and nothing else of the glow");
            app.world_mut().resource_mut::<Graphics>().volumetric_clouds = false;
            app.update();
            assert!(!app.world().entity(camera).contains::<bevy::camera::Hdr>(), "{selected:?}: off again: not HDR");
        }
    }

    #[test]
    fn aerial_perspective_is_off_in_both_looks_and_has_its_own_key() {
        assert!(!Graphics::preset(Preset::Vanilla).aerial_perspective && !Graphics::preset(Preset::Enhanced).aerial_perspective);
        let on = Graphics::only(&["aerial-perspective".into()]).expect("aerial-perspective");
        assert!(on.aerial_perspective);
        assert_eq!(on.describe(), "light glow, aerial perspective");
        let mut g = Graphics::preset(Preset::Vanilla);
        *g.switch("aerial-perspective").expect("settings key") = true;
        assert_eq!(g, on);
        assert!(!Graphics::preset(Preset::Enhanced).describe().contains("aerial"));
    }

    #[test]
    fn night_sky_detail_is_off_in_both_looks_and_has_its_own_key() {
        assert!(!Graphics::preset(Preset::Vanilla).night_sky_detail && !Graphics::preset(Preset::Enhanced).night_sky_detail);
        let on = Graphics::only(&["night-sky-detail".into()]).expect("night-sky-detail");
        assert!(on.night_sky_detail);
        assert_eq!(on.describe(), "light glow, night sky detail");
        let mut g = Graphics::preset(Preset::Vanilla);
        *g.switch("night-sky-detail").expect("settings key") = true;
        assert_eq!(g, on);
        assert!(!Graphics::preset(Preset::Enhanced).describe().contains("night sky"));
    }

    /// The light in the air (#3032 stage 3) is composed in the HDR picture too: its demand
    /// alone keeps the main view HDR, the cached change detection follows the demand both
    /// ways, and an absent or false demand changes nothing.
    #[test]
    fn light_in_the_air_keeps_the_view_hdr_with_glow_and_upscaler_off() {
        let _hdr = crate::hdr::test_lock();
        use crate::medium_air_demand::AirWanted;
        for selected in [ScreenAa::Off, ScreenAa::Taa] {
            let (mut app, camera) = aa_app(selected, true);
            app.world_mut().resource_mut::<Graphics>().glow = false;
            app.update();
            assert!(!app.world().entity(camera).contains::<bevy::camera::Hdr>(), "{selected:?}: no demand resource: not HDR");
            app.insert_resource(AirWanted(false));
            app.update();
            assert!(!app.world().entity(camera).contains::<bevy::camera::Hdr>(), "{selected:?}: demand off: not HDR");
            app.insert_resource(AirWanted(true));
            app.update();
            assert!(app.world().entity(camera).contains::<bevy::camera::Hdr>(), "{selected:?}: the demand makes the view HDR");
            assert!(!app.world().entity(camera).contains::<bevy::post_process::bloom::Bloom>(), "{selected:?}: and nothing else of the glow");
            app.insert_resource(AirWanted(false));
            app.update();
            assert!(!app.world().entity(camera).contains::<bevy::camera::Hdr>(), "{selected:?}: off again: not HDR");
        }
    }

    #[test]
    fn volumetric_light_is_off_in_both_looks_and_has_its_own_key() {
        assert!(!Graphics::preset(Preset::Vanilla).volumetric_light && !Graphics::preset(Preset::Enhanced).volumetric_light);
        let on = Graphics::only(&["volumetric-light".into()]).expect("volumetric-light");
        assert!(on.volumetric_light && !on.volumetric_clouds && !on.cloud_shadows && !on.terrain_shadows, "the light switches no other row on");
        assert_eq!(on.describe(), "light glow, volumetric light");
        let mut g = Graphics::preset(Preset::Vanilla);
        *g.switch("volumetric-light").expect("settings key") = true;
        assert_eq!(g, on);
        assert!(!Graphics::preset(Preset::Enhanced).describe().contains("volumetric"));
        assert!(KEYS.contains(&"volumetric-light"));
    }

    /// The Volumetric light row keeps the view HDR through the demand (medium_air_demand.rs
    /// `follow_demand`, the one writer of `AirWanted`) with Light glow off and no upscaler, and
    /// switching it off takes the HDR view away again.
    #[test]
    fn the_volumetric_light_row_keeps_the_view_hdr() {
        let _hdr = crate::hdr::test_lock();
        use crate::medium_air_demand::{follow_demand, AirAid, AirWanted};
        for selected in [ScreenAa::Off, ScreenAa::Taa] {
            let (mut app, camera) = aa_app(selected, true);
            app.init_resource::<AirAid>().init_resource::<AirWanted>().add_systems(PreUpdate, follow_demand);
            app.world_mut().resource_mut::<Graphics>().glow = false;
            app.update();
            assert!(!app.world().entity(camera).contains::<bevy::camera::Hdr>(), "{selected:?}: row off: not HDR");
            app.world_mut().resource_mut::<Graphics>().volumetric_light = true;
            app.update();
            assert!(app.world().resource::<AirWanted>().0, "{selected:?}: the row asks for the light");
            assert!(app.world().entity(camera).contains::<bevy::camera::Hdr>(), "{selected:?}: the row makes the view HDR");
            app.world_mut().resource_mut::<Graphics>().volumetric_light = false;
            app.update();
            assert!(!app.world().entity(camera).contains::<bevy::camera::Hdr>(), "{selected:?}: off again: not HDR");
        }
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
