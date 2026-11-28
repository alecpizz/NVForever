//! Sky, sun, ambient light and fog from the worldspace's climate and weather at a time of
//! day (fnv_world::sky; hypotheses listed there, in docs/spec-notes/world.md §6 and in
//! docs/design/layer3.md).
//!
//! The sky is the game's own sky meshes around the camera [observed: `meshes\sky\`]:
//! - `atmosphere.nif`, a dome coloured by the weather's horizon, sky-lower and sky-upper
//!   colours, weighted by its vertex colours' red, green and blue, mixed as display (sRGB)
//!   values (`dome_color`) [observed 2026-11-16 against the original's dome at five
//!   elevations, docs/spec-notes/world.md §6 "Dome weights"].
//! - `clouds.nif`, four cloud shapes whose vertex alpha fades them towards the horizon.
//!   The weather's cloud layers 0-3 (WTHR DNAM, CNAM, ANAM, BNAM textures and PNAM
//!   colours) go on the four shapes in file order [hypothesis: layer 0 is the top dome,
//!   which the night weather fills with a starfield; layer 3 the low dome, which the
//!   clear day weather fills with `NVCloudlight.dds`].
//! - The sun: the climate's sun texture and glare texture spread over angular squares
//!   around the sun, coloured by the weather's "sun" colour and drawn into the dome (the
//!   disc, behind the clouds) and the clouds (the glare) by sky.wesl [hypothesis, fitted].
//! - The climate's night-sky model (CLMT MODL, `Sky\Stars.nif`, additive at
//!   `STAR_STRENGTH`, weighted by the star texture's alpha where its shader flags set
//!   Alpha_Texture),
//!   coloured by the weather's "stars" colour and faded in as the sun sinks from 0° to
//!   −12° (sunset to full dark) [hypothesis: the stars colour is white by day, so the
//!   weather alone does not hide them].
//! - A moon disc opposite the sun (where the night light comes from), textured
//!   `sky\masser_<phase>.dds` by the climate's phase length and the days passed, faded in
//!   as the sun goes from +5° to −6° [hypothesis].
//!
//! Every sky part uses `SkyMaterial` (sky.wesl): its colour is a display value, mapped
//! through the inverse of the camera's tonemapper so the image-space grade sees the
//! weather's colours as the game's grade does, scaled by the image space's "LUM Ramp No
//! Tex" (`SkyLight`, `SkyState::sky_scale`) [hypothesis, fitted to two worldspaces].
//!
//! Without those meshes the sky falls back to a sphere. The sun lights the world as a
//! directional light (colour "sunlight"), optionally with cascaded shadow maps. Ambient
//! light uses "ambient", fog "fog" with the weather's day/night distances; an unset range
//! (far not beyond near) takes the other one, and a weather with neither has no fog
//! [hypothesis, world.md §6].

use crate::fly::FlyCam;
use crate::world::{game_to_bevy, WorldData};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::light::{CascadeShadowConfig, CascadeShadowConfigBuilder, NotShadowCaster, NotShadowReceiver};
use bevy::math::Affine2;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::pbr::{DistanceFog, ExtendedMaterial, FogFalloff, MaterialExtension};
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::prelude::*;
use fnv_nif::scene::{mesh_data, scene_objects, Decoded};
use fnv_nif::NifFile;
use fnv_world::imagespace::{adapter, cinematic_at, image_space, weather_adapters, Adapter, Cinematic};
use std::collections::HashMap;
use fnv_world::sky::{
    climate, default_weather, eligible, global_value, moon_phase, roll, roll_due, smoothstep, sun_direction, weather, Climate, ColorType,
    RegionWeather, Weather, WeatherChoice, MOON_PHASES,
};
use nvf_assets::{GameData, TextureCache};
use std::f32::consts::PI;

#[path = "sky_ini.rs"]
pub mod ini;
pub use ini::{CloudUpgradeConfig, MoonlightConfig, CLOUD_UPGRADE_INI_PATH, MOONLIGHT_INI_PATH};

/// Dome radius in Bevy units (inside the camera's far plane).
const DOME_RADIUS: f32 = 350_000.0;
/// Radius of the game's sky meshes [observed: atmosphere.nif and clouds.nif span about
/// ±500 units horizontally].
const NIF_SKY_RADIUS: f32 = 500.0;
/// The atmosphere dome is flat: about 500 units across but only 250 high [observed], so
/// sky objects stay within half the dome radius to remain in front of it overhead.
const SKY_INNER: f32 = 0.5 * DOME_RADIUS;
/// Depth beyond this distance from the camera (game units) is the sky dome's (it is opaque
/// and in the depth prepass); nothing of the world is drawn this far (sun shafts,
/// `shafts.rs`).
pub const SKY_DEPTH_FROM: f32 = 0.9 * SKY_INNER;
/// Distance of the sun and moon discs and the sun's size [hypothesis: about 4 degrees
/// across].
pub(crate) const SUN_DISTANCE: f32 = 0.8 * SKY_INNER;
const SUN_SIZE: f32 = 0.07 * SUN_DISTANCE;
/// Angular size (full width, degrees) over which the sun texture is spread around the
/// sun and the factor on its colour, and the same for the glare texture [hypothesis,
/// fitted to the sun and its glow in screenshots of the original game,
/// docs/spec-notes/world.md §6 "Sun and glare"].
const SUN_SIZE_DEG: f32 = 20.0;
const SUN_STRENGTH: f32 = 1.8;
const GLARE_SIZE_DEG: f32 = 130.0;
const GLARE_STRENGTH: f32 = 0.12;
// Angular footprints of the same climate textures in clear daytime volume skies [design].
const FAIR_SUN_SIZE_DEG: f32 = 11.0;
const FAIR_GLARE_SIZE_DEG: f32 = 100.0;
const FAIR_GLARE_GAIN: f32 = 0.75;
// The sun there [design; judged by eye from stills and a play test, 2026-11-24]: the sun texture's
// halo widens to the plain sky's size as the sun sinks between these elevations (degrees), its
// bright middle, 16 of 512 texels across, is drawn as a disc this wide (degrees; the texture's
// own is 0.34 at 11), and the rest of the texture is dimmed by this factor so the disc shows
// against it.
const FAIR_SUN_LOW_FROM: f32 = 32.0;
const FAIR_SUN_LOW_TO: f32 = 14.0;
const FAIR_DISC_DEG: f32 = 1.5;
const FAIR_HALO: f32 = 0.6;

/// The sun over the fair volume sky [design]: the constants above, or what the debug aid
/// `NVF_PLAY_DEBUG_FAIR_SUN` says instead (pictures only): `size=<deg>,glare=<deg>,gain=<x>` for
/// the high sun and, where `from` is above `to`, `lowsize=`, `lowglare=`, `lowgain=` for a sun at
/// or below `to` degrees of elevation, eased in from `from` degrees; `disc=<deg>` (0: the
/// texture's own) and `halo=<x>`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FairSun {
    high: [f32; 3],
    low: [f32; 3],
    from: f32,
    to: f32,
    /// The bright middle's width in degrees, 0 for the texture's own; and the factor on the
    /// sun texture outside it.
    disc: f32,
    halo: f32,
}

/// Radius of the bright middle of the game's sun texture, in uv: it ends 8 texels from the
/// centre of 512 [observed, 2026-11-24: `textures\sky\sun.dds` of the Steam game's `Fallout -
/// Textures2.bsa`, extracted with `fnvtool bsa extract` and decoded, the mean of alpha over rings of one
/// texel around the centre: 255 to 4 texels, 135 at 8, 109 at 12, falling to 0 at 252].
const SUN_MIDDLE_UV: f32 = 8.0 / 512.0;

impl FairSun {
    const DEFAULT: FairSun = {
        let v = [FAIR_SUN_SIZE_DEG, FAIR_GLARE_SIZE_DEG, FAIR_GLARE_GAIN];
        FairSun { high: v, low: [SUN_SIZE_DEG, v[1], v[2]], from: FAIR_SUN_LOW_FROM, to: FAIR_SUN_LOW_TO, disc: FAIR_DISC_DEG, halo: FAIR_HALO }
    };

    /// The aid's value; a key it does not name, or one that is no finite number, keeps its
    /// default (the low glare the high one's value).
    #[cfg(any(test, debug_assertions, feature = "dev-aids"))]
    fn parse(text: &str) -> FairSun {
        let mut f = Self::DEFAULT;
        let mut low = [None; 3];
        for part in text.split(',') {
            let Some((k, v)) = part.split_once('=') else { continue };
            let Some(v) = v.trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v >= 0.0) else { continue };
            match k.trim() {
                "size" => f.high[0] = v.clamp(1.0, 90.0),
                "glare" => f.high[1] = v.clamp(1.0, 160.0),
                "gain" => f.high[2] = v.min(8.0),
                "lowsize" => low[0] = Some(v.clamp(1.0, 90.0)),
                "lowglare" => low[1] = Some(v.clamp(1.0, 160.0)),
                "lowgain" => low[2] = Some(v.min(8.0)),
                "from" => f.from = v,
                "to" => f.to = v,
                "disc" => f.disc = v.min(20.0),
                "halo" => f.halo = v.min(8.0),
                _ => {}
            }
        }
        let default = [f.low[0], f.high[1], f.high[2]];
        f.low = std::array::from_fn(|i| low[i].unwrap_or(default[i]));
        f
    }

    /// [`FairSun::DEFAULT`], which a build without the developer aids (dev_aids.rs) always has.
    #[cfg(not(any(test, debug_assertions, feature = "dev-aids")))]
    #[inline]
    const fn get() -> FairSun {
        Self::DEFAULT
    }

    /// The aid, read once; its absence is [`FairSun::DEFAULT`].
    #[cfg(any(test, debug_assertions, feature = "dev-aids"))]
    fn get() -> FairSun {
        static AID: std::sync::OnceLock<FairSun> = std::sync::OnceLock::new();
        *AID.get_or_init(|| {
            let Some(text) = crate::dev_aids::var("NVF_PLAY_DEBUG_FAIR_SUN") else { return Self::DEFAULT };
            let f = Self::parse(&text);
            info!("sky: fair sun aid: {f:?}");
            f
        })
    }

    /// [`SkyLight::disc`] for a sun texture spread over `size` degrees, the disc drawn at
    /// `weight` (0..=1).
    fn disc_light(&self, size: f32, weight: f32) -> Vec4 {
        if self.disc <= 0.0 || weight <= 0.0 {
            return Vec4::ZERO;
        }
        // The texture lies on a plane facing the sun (sky.wesl `sun_texel`), hence the tangents.
        let half_tan = |deg: f32| (deg / 2.0).to_radians().tan();
        let k = 1.0 + (half_tan(self.disc) / (2.0 * SUN_MIDDLE_UV * half_tan(size)) - 1.0) * weight;
        Vec4::new(k.clamp(1.001, 0.2 / SUN_MIDDLE_UV), SUN_MIDDLE_UV, 1.0 + (self.halo - 1.0) * weight, 0.0)
    }

    /// Size and glare size (degrees) and glare gain at a sun elevation (degrees).
    fn at(&self, elevation: f32) -> [f32; 3] {
        let low = if self.from > self.to { 1.0 - smoothstep(self.to, self.from, elevation) } else { 0.0 };
        std::array::from_fn(|i| self.high[i] + (self.low[i] - self.high[i]) * low)
    }
}
/// Moon quad size at SUN_DISTANCE; the disc fills about 30% of the masser textures
/// [observed], so this draws it about 3.6 degrees across [tuned by eye].
const MOON_SIZE: f32 = 2.4 * SUN_SIZE;
/// Sun elevations (degrees) over which the stars and the moon fade in [hypothesis].
const STARS_FADE: (f32, f32) = (0.0, -12.0);
const MOON_FADE: (f32, f32) = (5.0, -6.0);
/// The moon disc is hidden while its direction's height (Bevy y) is below this
/// (`update_night_sky`; night_sky.rs follows the same rule).
pub(crate) const MOON_HIDDEN_Y: f32 = -0.1;
/// The volumetric clouds' daylight under Physical lighting ([`SkyState::physical_cloud_light`]):
/// linear RGB, in the physical dome's units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CloudDaylight {
    pub sun: [f32; 3],
    pub upper: [f32; 3],
    pub horizon: [f32; 3],
    /// The sky light the ground under the clouds takes (their light from below): the dome's
    /// mean of overhead and horizon, in place of the weather's Ambient.
    pub ground: [f32; 3],
}

/// How far south of the east-west line the sun's path runs, in radians (9°) [hypothesis,
/// fitted to the original game's sun positions; fnv_world::sky::sun_direction].
const SUN_SOUTH: f32 = 9.0 * PI / 180.0;
/// Elevation (radians) below which the directional light fades out towards the horizon.
const HORIZON_FADE: f32 = 0.1;
/// Game hours per real second at the default TimeScale of 30 [documented: GECK wiki
/// TimeScale]: a weather's transition, given in real seconds
/// (`Weather::transition_seconds`), fades over that many game hours.
const GAME_HOURS_PER_SECOND: f32 = 30.0 / 3600.0;
/// Transparent sky parts are drawn before everything else. Bevy sorts transparent meshes
/// by view-space z plus depth bias, ascending, and that value grows towards the camera
/// [documented: bevy_core_pipeline 0.19 Transparent3d], so a large negative bias draws the
/// sky (centred on the camera) first; blended world meshes write no depth and would
/// otherwise be painted over. Within the sky, lower biases are drawn first.
pub(crate) const SKY_SORT_BIAS: f32 = -1.0e6;
/// The cloud shapes' size against the atmosphere dome's. The dome is opaque and writes
/// depth, and the blended sky parts carry [`SKY_SORT_BIAS`], which Bevy also applies as the
/// pipeline's depth bias: with reversed depth that moves a part about 14 % farther off
/// (10^6 steps of a 2^-23 mantissa, a factor 0.88 on its depth) [hypothesis from the
/// figures below]. `clouds.nif`'s low shape (100 to 320 units from the centre) and its
/// horizon shapes (411 to 445) are well inside the dome (250 to 490) and passed; its first
/// shape, `CloudDome`, is 0 to 6 % *outside* the dome (461 to 446 units at 10 degrees up,
/// 300 to 284 at 50, 250 to 250 at the top) [observed: `fnvtool nif decode`] and was never
/// drawn, at 0.97 and 0.9 of its size either. So the urban weathers' clouds (NVUrbanClear,
/// NVTheStripWeather: cloud layer 0, `NV_WastelandUpperSky2.dds`) were missing, while the
/// same texture on layer 3 (NVWastelandHazy) showed [observed 2026-11-16/17 against the
/// original in Freeside]. At 0.75 every cloud shape is under 0.8 of the dome's distance.
/// The sky is centred on the camera, so the size changes nothing else.
const CLOUD_INSET: f32 = 0.75;

/// Whether a transparent item's sort key (view distance plus depth bias, as Bevy sorts
/// them) is one of the sky's blended or additive parts ([`SKY_SORT_BIAS`]: the cloud
/// layers, the night's starfield layer, the stars, the moon): above half again the bias and
/// up to the terrain layers' band (`nif_material::sorts_first`), which leaves room for
/// every view distance in the world (the stars sit at the world's origin, the moon
/// [`SUN_DISTANCE`] out). FSR draws these into its reactive reference (fsr.rs).
pub(crate) fn sorts_with_sky(sort_key: f32) -> bool {
    sort_key > SKY_SORT_BIAS * 1.5 && sort_key <= nvf_assets::nif_material::SORT_FIRST_BIAS * 1.5
}
/// How far the sun's shadows reach when they are switched on in a run started without
/// them (game units): the default of `--shadow-distance`. With the default cascades
/// [engineering choice, 2026-10-12, owner's playtest "shadow rendering seriously affects
/// performance"]: every cascade is a pass over its casters, and at Goodsprings by day the
/// earlier 16,000 in 4 cascades took the frame from about 7.6 ms without sun shadows to
/// 19.7 ms; 8,000 in 3 takes it to about 14.4 ms (docs/design/graphics-enhanced.md §2).
/// The original casts no sun shadows from the world, so shadows ending nearer are no
/// further from its look.
pub const DEFAULT_SHADOW_DISTANCE: f32 = 8_000.0;
/// How the sun's shadow cascades are laid out ([`shadow_cascade_layout`]): the Sun shadows
/// mode picks Quality or Performance (shadow_rate.rs `set_performance`); the test aid
/// `--graphics-cycle cascades=<n>` asks for a count spaced as before #2843.
///
/// The default, Quality's (see [`DEFAULT_SHADOW_DISTANCE`]): 4 cascades, laid out by [`shadow_cascade_layout`] [engineering choice: the contributor's proposal in
/// #2843, for the maintainer to decide; 3 from 2026-10-12 until then]. The 4 cascades of
/// before 2026-10-12 reached 16,000 units, and every cascade was drawn every frame (8,000 in
/// 4 then took 15.0-16.8 ms against 13.7-15.2 for 3: graphics-enhanced.md §5.3); now the
/// reach stays 8,000, the casters are culled to each cascade's slice
/// ([`sun_casters_over_slice`]) and the cascades are staggered (shadow_rate.rs
/// `STAGGERED_SCHEDULE`: two drawn in a frame while they are staggered, all four in the
/// frame after a change of bounds, map size or texture; 2 of 4 on average against
/// 1.75 of 3). [observed 2026-11-16: Goodsprings' start view at 09:00 standing still,
/// 2560 × 1440, RTX 5070 Ti, ambient occlusion, longer shadows (reach 12,000), light glow,
/// 16x anisotropic filtering, sun shafts, `--no-actors`; sun shadows off 2.81-2.85 ms a
/// frame] with 2048² maps, 3 cascades with far bounds 667, 2,828, 12,000 (texels 0.77,
/// 3.2, 13.8 units) 3.15-3.16 ms; 4 with 700, 2,000, 5,000, 12,000 (texels 0.80, 2.3, 5.7,
/// 13.8) 3.22 ms. With 4096² maps 4 with 1,500, 3,500, 7,000, 12,000 and a blend band of
/// 0.4 (texels 0.86, 2.0, 4.0, 6.9) 3.43-3.48 ms (docs/design/frame-rate-audit.md §12).
/// Without Longer shadows the 4096² layout is 1,500, 2,400, 4,800, 8,000, derived by
/// [`SHADOW_CAPS_QUALITY_WIDE`]'s rule; its cost was not measured.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CascadeLayout {
    /// Quality's 4 cascades (the default), their bounds following the shadow maps' size.
    #[default]
    Quality,
    /// Performance's 3 cascades (shadow_rate.rs, the Sun shadows row's middle choice).
    Performance,
    /// This many cascades in Bevy's geometric spacing from a first bound of a twelfth of
    /// the distance, as every count had it before #2843 (`--graphics-cycle cascades=<n>`,
    /// so that measurements of then still compare; 1 is the moon's single cascade).
    Spaced(usize),
}

impl CascadeLayout {
    /// The number of cascades.
    pub const fn count(self) -> usize {
        match self {
            CascadeLayout::Quality => SHADOW_SHARES_QUALITY.len() + 1,
            CascadeLayout::Performance => SHADOW_SHARES_PERFORMANCE.len() + 1,
            CascadeLayout::Spaced(n) => if n == 0 { 1 } else { n },
        }
    }
}

/// Shares of the weather's and the interior's light colours (display values) in the
/// display-value lighting sum (lights.rs `DISPLAY_HELPERS`; world.md §8 "Light on
/// surfaces in display values") [fitted to the original's frames: the lab's 15 L3 frames,
/// the effects audit's night frames and Dead Money's lantern]. Outdoors: the weather's
/// ambient colour, its sunlight by day (before the Sunlight Dimmer's scale) and its
/// night "sunlight" as moonlight. Interiors: XCLL's ambient and directional colours as
/// stored.
pub const AMBIENT_SHARE: f32 = 0.9;
pub const SUN_SHARE: f32 = 0.75;
pub const MOON_SHARE: f32 = 0.5;
pub const INTERIOR_AMBIENT_SHARE: f32 = 1.0;
pub const INTERIOR_DIRECTIONAL_SHARE: f32 = 1.0;
const PHYSICAL_SUN_GAIN: f32 = 6.8;
const PHYSICAL_MOON_GAIN: f32 = 1.2;
const PHYSICAL_INTERIOR_DIR_GAIN: f32 = 0.85;

/// Earth atmosphere constants (Hillaire EGSR 2020 / Nishita, km) tuned for Mojave elevation (~1.1 km).
const ATMO_RE: f32 = 6360.0;
const ATMO_RA: f32 = 6460.0;
const ATMO_H0: f32 = 1.1;
const ATMO_HR: f32 = 8.0;
const ATMO_HM: f32 = 1.0;
const BETA_R: Vec3 = Vec3::new(4.60e-3, 13.00e-3, 35.50e-3);
const BETA_O: Vec3 = Vec3::new(1.10e-3, 2.35e-3, 0.06e-3);
const ATMO_SUN_IRRADIANCE: f32 = 8.2;

fn ray_sphere_exit(r: f32, mu: f32, rad: f32) -> f32 {
    let disc = (r * r * (mu * mu - 1.0) + rad * rad).max(0.0);
    (-r * mu + disc.sqrt()).max(0.0)
}

/// Mojave aerosol Mie scattering coefficient (`/km`) modulated by weather clearness `[0, 1]`.
pub(crate) fn physical_mie_coeff(clearness: f32) -> f32 {
    let c = clearness.clamp(0.0, 1.0);
    14.0e-3 + (1.0e-3 - 14.0e-3) * c
}

/// Spherical Rayleigh + Mie + Chappuis ozone transmittance from radial altitude `r` (km) toward sun cosine `mu_s`.
pub(crate) fn physical_sun_transmittance(r: f32, mu_s: f32, beta_m_ext: f32) -> Vec3 {
    let mut earth_vis = 1.0f32;
    if mu_s < 0.0 {
        let min_r = r * (1.0 - mu_s * mu_s).max(0.0).sqrt();
        earth_vis = smoothstep(ATMO_RE - 6.0, ATMO_RE + 1.0, min_r);
        if earth_vis <= 1e-4 {
            return Vec3::ZERO;
        }
    }
    let t_max = ray_sphere_exit(r, mu_s, ATMO_RA);
    let (mut od_r, mut od_m, mut od_o, mut prev_t) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for j in 0..4 {
        let u = (j + 1) as f32 / 4.0;
        let cur_t = t_max * u * u;
        let dt = cur_t - prev_t;
        let mid_t = 0.5 * (prev_t + cur_t);
        prev_t = cur_t;
        let h = ((r * r + mid_t * mid_t + 2.0 * r * mid_t * mu_s).max(0.0).sqrt() - ATMO_RE).max(0.0);
        od_r += (-h / ATMO_HR).exp() * dt;
        od_m += (-h / ATMO_HM).exp() * dt;
        od_o += (1.0 - (h - 25.0).abs() / 15.0).max(0.0) * dt;
    }
    let tau = BETA_R * od_r + Vec3::splat(beta_m_ext * od_m) + BETA_O * od_o;
    Vec3::new((-tau.x).exp(), (-tau.y).exp(), (-tau.z).exp()) * earth_vis
}

/// 8-step quadratic spherical Rayleigh/Mie/Ozone sky radiance matching `sky.wesl` `eval_physical_sky`.
pub(crate) fn eval_physical_sky(dir: Vec3, sun_dir: Vec3, clearness: f32) -> Vec3 {
    let d = dir.normalize_or(Vec3::Y);
    let s = sun_dir.normalize_or(Vec3::Y);
    let c_clamped = clearness.clamp(0.0, 1.0);
    let d_sky = if d.y < 0.004 {
        let h2 = d.x * d.x + d.z * d.z;
        let h_dir = if h2 > 1e-8 { Vec3::new(d.x, 0.0, d.z) / h2.sqrt() } else { Vec3::X };
        (h_dir + Vec3::new(0.0, 0.004, 0.0)).normalize()
    } else {
        d
    };
    let mu_v = d_sky.y.max(0.004);
    let cos_t = d_sky.dot(s).clamp(-1.0, 1.0);

    let beta_m_s = physical_mie_coeff(c_clamped);
    let beta_m_e = beta_m_s * 1.11;
    let phase_r = (3.0 / (16.0 * PI)) * (1.0 + cos_t * cos_t);
    let g = 0.68 + 0.14 * c_clamped;
    let hg_d = (1.0 + g * g - 2.0 * g * cos_t).max(0.025);
    let phase_m = (3.0 / (8.0 * PI)) * ((1.0 - g * g) * (1.0 + cos_t * cos_t)) / ((2.0 + g * g) * hg_d * hg_d.sqrt());

    let r0 = ATMO_RE + ATMO_H0;
    let t_max = ray_sphere_exit(r0, mu_v, ATMO_RA);
    let mut accum = Vec3::ZERO;
    let mut trans_v = Vec3::ONE;
    let mut prev_t = 0.0f32;
    for i in 0..8 {
        let u = (i + 1) as f32 / 8.0;
        let cur_t = t_max * u * u;
        let dt = cur_t - prev_t;
        let mid_t = 0.5 * (prev_t + cur_t);
        prev_t = cur_t;
        let p = Vec3::new(d_sky.x * mid_t, r0 + d_sky.y * mid_t, d_sky.z * mid_t);
        let r = p.length();
        let h = (r - ATMO_RE).max(0.0);
        let rho_r = (-h / ATMO_HR).exp();
        let rho_m = (-h / ATMO_HM).exp();
        let rho_o = (1.0 - (h - 25.0).abs() / 15.0).max(0.0);
        let mu_s = (p.dot(s) / r.max(1.0)).clamp(-1.0, 1.0);
        let t_sun = physical_sun_transmittance(r, mu_s, beta_m_e);
        let ext = BETA_R * rho_r + Vec3::splat(beta_m_e * rho_m) + BETA_O * rho_o;
        let step_t = Vec3::new((-ext.x * dt).exp(), (-ext.y * dt).exp(), (-ext.z * dt).exp());
        let ms_sun = smoothstep(-0.10, 0.20, mu_s);
        let scat = BETA_R * (rho_r * phase_r) + Vec3::splat(beta_m_s * rho_m * phase_m);
        let ms = (BETA_R * rho_r + Vec3::splat(beta_m_s * rho_m)) * (0.022 / (4.0 * PI) * ms_sun);
        let s_tot = t_sun * scat + ms;
        let integ = (s_tot - s_tot * step_t) / ext.max(Vec3::splat(1e-6));
        accum += trans_v * integ;
        trans_v *= step_t;
    }

    let night = Vec3::new(0.0007, 0.0016, 0.0040) * (0.6 + 0.5 * (1.0 - mu_v).powi(2));
    let mut sky = accum * ATMO_SUN_IRRADIANCE + night;
    if d.y < 0.0 {
        let ground_tint = smoothstep(0.0, 0.15, -d.y);
        sky = sky.lerp(sky * Vec3::new(0.30, 0.24, 0.17), ground_tint);
    }
    sky
}

/// Light level that makes a colour of 1.0 light a white surface to 1.0 on screen: the
/// inverse of the camera's exposure (Bevy's default, EV100 9.7). The game's lighting is
/// treated as texture × (ambient + sunlight · N·L) in display units [hypothesis], so the
/// sun gets π times this (Lambert's 1/π) and ambient light this.
pub(crate) fn light_unit() -> f32 {
    1.0 / bevy::camera::Exposure::default().exposure()
}

/// Material of every sky part: unlit, with the colour mapped through the inverse of the
/// camera's tonemapper so the display value reaches the image-space grade (sky.wesl).
pub type SkyMaterial = ExtendedMaterial<StandardMaterial, SkyDisplay>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct SkyDisplay {
    #[uniform(100)]
    pub light: SkyLight,
    /// The climate's sun texture (drawn on the dome only, so clouds pass in front).
    #[texture(101)]
    #[sampler(102)]
    pub sun_texture: Option<Handle<Image>>,
    /// The climate's sun glare texture (dome and clouds).
    #[texture(103)]
    #[sampler(104)]
    pub glare_texture: Option<Handle<Image>>,
}

/// What the sky's parts take besides their own colour (sky.wesl): the sky level, linear
/// colour × `scale` ([`SkyState::sky_scale`]: the image space's "LUM Ramp No Tex")
/// [hypothesis, fitted to two worldspaces, docs/spec-notes/world.md §6 "Sky level"], and
/// the sun. `glow` adds nothing to the colour now (its fourth figure is still read).
///
/// The sun is drawn into the same colours, before the tonemapper inverse: the sun texture
/// and the glare texture spread over angular squares centred on the sun (SUN_SIZE_DEG,
/// GLARE_SIZE_DEG), added as colour × alpha (the textures' blend in the sky meshes) times
/// the weather's sun colour and a strength [hypothesis, fitted]. Adding them as separate
/// additive quads after the inverse could not reproduce the game's glow over a bright sky.
#[derive(Clone, Copy, Debug, Default, ShaderType, Reflect)]
pub struct SkyLight {
    /// x: scale (`sky_scale`); y: `1` for additive parts weighted by texture alpha
    /// (`Alpha_Texture`) and `1` for the moon disc (`AlphaMode::Blend`, linearizing its
    /// sRGB-authored alpha halo and emitting direct HDR radiance under Tony McMapface),
    /// `0` otherwise; z: `1` for parts sky daylight applies to (the dome and clouds),
    /// `0` for stars and moon; w: `2` to draw the sun disc and glare (the dome), `1` the
    /// glare only (clouds), `0` neither.
    pub params: Vec4,
    /// Linear RGB added; w 1 when the view's tonemapper is the clip (lights.rs
    /// `display_clip`), 0 for Tony McMapface.
    pub glow: Vec4,
    /// xyz: direction to the sun (Bevy world space); w: physical sky clearness (`> 0` under
    /// physical PBR without `clip` tonemap override, `0` otherwise).
    pub sun_dir: Vec4,
    /// rgb: sun colour × strength (linear); w: tan of half the disc's angular size.
    pub sun: Vec4,
    /// rgb: sun colour × glare strength (linear); w: tan of half the glare's size.
    pub glare: Vec4,
    /// Night sky detail (night_sky.rs; zero with its switch off, and on every part it does not
    /// touch): xyz where the moon disc is drawn from the sky's centre (the main camera, which
    /// every sky part follows: the direction × [`SUN_DISTANCE`]), w the Henyey-Greenstein g of
    /// the moon's lobe (the dome's glow, the clouds' moonlight).
    pub moon: Vec4,
    /// Night sky detail, by part: on the stars x the twinkle's amplitude and y its lattice
    /// cells per unit of direction; on the dome and clouds rgb the moon term's gain and w its
    /// cap (0: none).
    pub night: Vec4,
    /// The sun's disc enlarged (the fair sun aid): x the magnification of the sun texture's
    /// middle (0: none), y that middle's radius in the texture (uv), z the factor on the rest.
    pub disc: Vec4,
}

impl MaterialExtension for SkyDisplay {
    fn fragment_shader() -> ShaderRef {
        "embedded://nvforever/sky.wesl".into()
    }
}

/// Daylight (`SkyLight`) applies to every part but the additive ones; `sun` is the
/// `SkyLight::params.w` mode.
fn sky_material(base: StandardMaterial, sun: f32) -> SkyMaterial {
    let applies = if matches!(base.alpha_mode, AlphaMode::Add) { 0.0 } else { 1.0 };
    let light = SkyLight { params: Vec4::new(1.0, 0.0, applies, sun), ..default() };
    ExtendedMaterial {
        base: StandardMaterial { unlit: true, fog_enabled: false, cull_mode: None, ..base },
        extension: SkyDisplay { light, ..default() },
    }
}

/// Material for the moon disc: alpha-blended (`params.y = 1`) and unscaled by daytime
/// sky brightness (`params.z = 0`), so `sky.wesl` linearizes its sRGB-authored alpha halo
/// without amplifying DXT5 noise or shifting low-alpha colour through `tony_inverse`.
fn moon_material() -> SkyMaterial {
    let mut m = sky_material(
        StandardMaterial {
            alpha_mode: AlphaMode::Blend,
            depth_bias: SKY_SORT_BIAS - 20.0,
            ..default()
        },
        0.0,
    );
    m.extension.light.params.y = 1.0;
    m.extension.light.params.z = 0.0;
    m
}

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "sky.wesl");
        app.add_plugins(MaterialPlugin::<SkyMaterial>::default());
    }
}

/// Live session weather visual overrides synced from `fnv_game::Game` by
/// [`sync_weather_visuals`]: `SetWeatherRGBColor` (`NAM0` and `PNAM`),
/// `SetWeatherImageSpaceMod` (`0IAD..5IAD`), and `SetWeatherTransitionTimeOverride`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WeatherVisualOverrides {
    /// `SetWeatherRGBColor` (`NAM0` colour types `0..=9`, excluding type 2) by weather FormID.
    pub colors: HashMap<u32, [[Option<[u8; 3]>; 6]; fnv_world::sky::COLOR_TYPES]>,
    /// `SetWeatherRGBColor` type-2 (`PNAM` lower cloud layers `0..=3`) by weather FormID.
    pub cloud_colors: HashMap<u32, [[Option<[u8; 3]>; 6]; 4]>,
    /// `SetWeatherImageSpaceMod` (`0IAD..5IAD`) resolved adapters by weather FormID.
    /// Outer `Some` overrides the authored slot; inner `None` explicitly clears it.
    pub adapters: HashMap<u32, [Option<Option<Adapter>>; 6]>,
    /// `SetWeatherTransitionTimeOverride` transition length in real seconds (`None`: weather's own).
    pub transition_seconds: Option<f32>,
}

#[derive(Resource)]
#[cfg_attr(test, derive(Default))]
pub struct SkyState {
    pub climate: Climate,
    pub weather: Weather,
    pub hour: f32,
    /// Fog distances from the weather are used as stored unless this is set.
    pub fog_override: Option<(f32, f32)>,
    /// Interior mode: the cell's own lighting replaces sky, sun and weather.
    pub interior: Option<fnv_world::lighting::CellLighting>,
    /// Interior mode: the cell's own image space (CELL XCIM) grades the picture.
    pub interior_grade: Option<Cinematic>,
    /// The sun casts shadows (`--sun-shadows` with `--shadow-distance` above 0; the settings page:
    /// [`set_shadows`]).
    pub shadows: bool,
    /// Sun shadows reach this far from the camera (game units).
    pub shadow_distance: f32,
    /// The cascades reach this many times `shadow_distance` (1; more with the finer
    /// shadows of `graphics.rs`), the first cascade as near as without.
    pub shadow_reach: f32,
    /// How the sun's shadow cascades are laid out (their number and far bounds).
    pub shadow_layout: CascadeLayout,
    /// The picture is graded by the image space (`--no-grade` and the settings page turn
    /// it off).
    pub grade: bool,
    /// Test aid: game hours per real second (0: time stands still).
    pub time_scale: f32,
    /// The time-lapse clock runs (set once the cells around the start are loaded).
    pub clock_running: bool,
    /// Game days passed at midnight of the current day (moon phase).
    pub days_passed: f32,
    /// The next change the sky would make is skipped (a short fast travel under the
    /// player's Improved Weather tweak, [`SkyState::keep_weather_once`]).
    keep_once: bool,
    /// The weather being faded out, while a change is in progress.
    pub prev: Option<Weather>,
    /// Progress of the fade from `prev` to `weather` (1 = done).
    pub fade: f32,
    /// The climate's weathers with their gates, when the weather follows the climate
    /// (no --weather).
    choices: Vec<Choice>,
    /// The worldspace's base image space (WRLD INAM); without one there is no grade.
    pub image_space: Option<Cinematic>,
    /// The FormID of the worldspace's base image space (WRLD INAM or parent's), so live
    /// `SetImageSpaceTrait` edits can re-evaluate [`Self::image_space`].
    pub world_image_space_id: Option<u32>,
    /// Session `SetWeatherTraitNumeric` fog overrides (`15..=20`: day near/far, night
    /// near/far, day/night power) keyed by weather FormID, synced by [`sync_weather_fog`].
    pub weather_fog_overrides: HashMap<u32, [Option<f32>; 6]>,
    /// Session `SetWeatherRGBColor`, `SetWeatherImageSpaceMod`, and
    /// `SetWeatherTransitionTimeOverride` overrides synced by [`sync_weather_visuals`].
    pub weather_visual_overrides: WeatherVisualOverrides,
    /// Each weather's image space adapters per time of day, by weather form ID.
    adapters: HashMap<u32, [Option<Adapter>; 6]>,
    changed: bool,
    /// A script's weather (SetWeather, ForceWeather), while it lasts.
    script: Option<ScriptWeather>,
    /// The weather was given (`--weather`): it never changes by itself (the parity lab's
    /// frames and the reference views pass it).
    pinned: bool,
    /// The worldspace's overriding weather regions with their weathers.
    regions: Vec<RegionList>,
    /// The region whose list the weather follows (index into `regions`); `None`: the
    /// climate's.
    source: Option<usize>,
    /// The camera's place has been looked at once ([`weather_region`]); until then the
    /// weather is the one the start chose and no roll is made.
    placed: bool,
    /// Last sampled worldspace and exterior cell [design, #2531; `Self::place_outdoors`].
    region_cell: Option<(u32, (i32, i32))>,
    /// The game clock has moved the sky (fades take game time; without a clock a change
    /// is at once).
    pub(crate) clock_seen: bool,
    /// The game clock's hour as it is this frame (`--play`), which the sun light's
    /// direction follows; `hour` follows it in half-game-minute steps
    /// ([`Self::follow_game_clock`]), and turning the light by each step made every sun
    /// shadow jump about once a real second. `None` until the clock is seen, and again
    /// after the hour is set another way. [engineering choice]
    sun_hour: Option<f32>,
    /// A weather fade moves with every clock reading, not with the half-minute steps: the
    /// volumetric clouds are drawn ([`fade_cadence`]). Off, the steps' cadence.
    pub(crate) smooth_fade: bool,
    /// The hours since the last half-minute step that smooth readings have given the fade
    /// already; the next step gives it the rest, so switching [`Self::smooth_fade`] between
    /// two steps moves nothing by itself and counts no time twice or not at all.
    fade_given: f32,
    /// The hours since the last half-minute step of the last clock reading, smooth or not.
    clock_ahead: f32,
    /// The list (region FormID, `None` for the climate) and its eligible weathers the
    /// last roll was made for.
    rolled_for: Option<(Option<u32>, Vec<u32>)>,
    /// The weather the last roll chose: shown unless a script's weather is.
    pick: u32,
    /// Rolls made so far.
    rolls: u64,
    /// The weather rolls' random state (splitmix64; `--weather-seed`, kept in saves).
    pub rng: u64,
    /// The whole game hour of the last volatility check.
    hour_index: Option<i64>,
    /// Gating globals' values as the simulation has them (`--play`).
    live_globals: HashMap<u32, f32>,
    /// A loaded game's weather: kept, without a roll, by the first list it is eligible in.
    resume: Option<u32>,
    /// A lightning flash's light now (linear RGB, added to the ambient light and the
    /// sky's glow; `crate::weather_fx`).
    pub(crate) flash: [f32; 3],
    /// The scripts' wind (SetWindDirection, `fnv_game::nvse_jip12`): degrees from the
    /// game's +x toward +y; `None` leaves `crate::tree`'s fixed direction.
    pub script_wind: Option<f32>,
    /// The scripts' moon textures by phase (SetMoonTexture), under `textures\`; `None`
    /// leaves the phase's own.
    pub moon_textures: [Option<String>; 8],
    /// A script asked for a lightning flash (TriggerLightningFX); `crate::weather_fx`
    /// draws it.
    pub lightning_request: bool,
    /// Optional `Data\NVSE\Plugins\MoonlightNVSE.ini` configuration (`None` when absent,
    /// leaving nighttime directional light bit-for-bit unchanged).
    pub moonlight: Option<MoonlightConfig>,
    /// Optional `Data\NVSE\Plugins\CloudUpgrade.ini` configuration (`None` when absent,
    /// leaving cloud layer scrolling bit-for-bit unchanged).
    pub cloud_upgrade: Option<CloudUpgradeConfig>,
}

/// An overriding weather region of the worldspace and its weathers.
struct RegionList {
    region: RegionWeather,
    choices: Vec<Choice>,
}

/// The weather rolls' seed when `--weather-seed` is not given [engineering choice: a
/// fixed seed keeps every run the same, the smoke tests' included; a save keeps the state].
pub const DEFAULT_WEATHER_SEED: u64 = 0x4E56_5745_4154_4852;

/// The sky's part of a save (nvf_play::save): the weather shown (the start takes it from
/// the save's header), whether a script holds it against the climate and the rolls' random
/// state. All of it is taken as the sky is made ([`SkyState::restore_saved`]), so loading
/// the part later does nothing: by then the sky has rolled from that state and a load
/// script may have released or changed the weather.
pub const SAVE_HOOK: nvf_play::save::SaveHook = nvf_play::save::SaveHook {
    key: "sky",
    save: |w| {
        let s = w.get_resource::<SkyState>()?;
        Some(serde_json::json!({"weather": s.weather.edid, "held": s.script.is_some_and(|sw| sw.hold), "rng": s.rng.to_string()}))
    },
    load: |_, _| {},
    remap: None,
};

/// A weather a script set: held until ReleaseWeatherOverride, or kept until the
/// weather system's next roll after the one it was set under [documented: GECK wiki
/// SetWeather, "the weather will change back in the normal fashion according to the
/// weather system"; hypothesis: "in the normal fashion" is the next roll].
#[derive(Debug, Clone, Copy)]
struct ScriptWeather {
    hold: bool,
    /// [`SkyState::rolls`] when the script set it.
    roll: u64,
}

/// A weather list entry (climate or region) with its weather and gating global.
struct Choice {
    weather: Weather,
    chance: i32,
    global: Option<Gate>,
}

/// A gating global: FormID, Editor ID and stored value.
struct Gate {
    id: u32,
    edid: Option<String>,
    stored: Option<f32>,
}

impl Choice {
    fn entry(&self) -> WeatherChoice {
        WeatherChoice { weather: self.weather.form_id, chance: self.chance, global: self.global.as_ref().map(|g| g.id) }
    }
}

/// Value of a gating global at `hour`: the simulation's value when it runs, else the
/// stored one; VNight always by the hour, standing in for the one script that drives the
/// Mojave's weather gate: NightQuestScript (FalloutNV.esm) sets VNight to 1 when
/// `GetCurrentTime >= 21 || GetCurrentTime < 4.30`, else to 0 [observed: its source text,
/// SCTX] (the script runs every few seconds, the stand-in keeps runs without `--play` and
/// the hour's edge exact).
fn gate_value(edid: Option<&str>, value: Option<f32>, hour: f32) -> Option<f32> {
    match edid {
        Some(e) if e.eq_ignore_ascii_case("VNight") => Some(if !(4.3..21.0).contains(&hour) { 1.0 } else { 0.0 }),
        _ => value,
    }
}

/// The next number in 0..1 of a splitmix64 stream.
fn next_unit(state: &mut u64) -> f32 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

impl SkyState {
    /// The worldspace's climate (or its parent's) and the weather: `pinned` (`--weather`)
    /// for good, else `start` (a loaded game's) or the climate's most likely, followed by
    /// the rolls from the first frame on ([`weather_region`]).
    pub fn new(world: &WorldData, pinned: Option<&str>, start: Option<&str>, hour: f32, seed: u64) -> Result<SkyState, String> {
        let ws = world.index.worldspaces.get(&world.world).ok_or("worldspace not in the world index")?;
        let clim_id = ws
            .climate
            .or_else(|| ws.parent.and_then(|p| world.index.worldspaces.get(&p)).and_then(|p| p.climate))
            .ok_or("worldspace has no climate")?;
        SkyState::with_climate(world, clim_id, pinned, start, hour, seed)
    }

    /// The sky of the start, when the start worldspace's own cannot be made (no climate,
    /// or one that is unreadable or lists no weather: a broken mod). Travel keeps the
    /// previous worldspace's sky then (worldspace.rs); at the start there is none, so the
    /// climate of another worldspace stands in: WastelandNV's, else the first (by FormID)
    /// that works [hypothesis: our fallback, not the original's behaviour].
    pub fn fallback(world: &WorldData, pinned: Option<&str>, start: Option<&str>, hour: f32, seed: u64) -> Result<SkyState, String> {
        let mut spaces: Vec<_> = world.index.worldspaces.values().filter_map(|w| Some((w.form_id, w.climate?, w.edid.as_deref()))).collect();
        spaces.sort_by_key(|&(id, _, edid)| (!edid.is_some_and(|e| e.eq_ignore_ascii_case("WastelandNV")), id));
        for (id, clim, _) in spaces {
            if let Ok(s) = SkyState::with_climate(world, clim, pinned, start, hour, seed) {
                info!("sky: the start uses worldspace {id:08X}'s climate");
                return Ok(s);
            }
        }
        Err("no worldspace in the load order has a usable climate".into())
    }

    /// [`SkyState::new`] with climate `clim_id`.
    pub fn with_climate(world: &WorldData, clim_id: u32, pinned: Option<&str>, start: Option<&str>, hour: f32, seed: u64) -> Result<SkyState, String> {
        let clim = climate(world.lo, world.schema, clim_id).ok_or("climate unreadable")?;
        let most_likely = || default_weather(&clim, |g| global_value(world.lo, world.schema, g)).ok_or("climate lists no weather");
        let wid = match (pinned, start) {
            (Some(n), _) => find_weather(world, n).ok_or_else(|| format!("no weather {n}"))?,
            (None, Some(n)) => match find_weather(world, n) {
                Some(id) => id,
                None => {
                    warn!("weather: the saved weather {n} is not in the load order; the climate's instead");
                    most_likely()?
                }
            },
            (None, None) => most_likely()?,
        };
        let load = |list: &[WeatherChoice]| -> Vec<Choice> {
            list.iter()
                .filter_map(|c| {
                    let global = c.global.map(|g| Gate { id: g, edid: global_edid(world, g), stored: global_value(world.lo, world.schema, g) });
                    Some(Choice { weather: weather(world.lo, world.schema, c.weather)?, chance: c.chance, global })
                })
                .collect()
        };
        let (choices, regions) = if pinned.is_some() {
            (Vec::new(), Vec::new())
        } else {
            let regions: Vec<RegionList> = world
                .weather_regions
                .iter()
                .filter(|r| r.overrides() && r.world == Some(world.world))
                .map(|r| RegionList { choices: load(&r.weathers), region: r.clone() })
                .filter(|r| !r.choices.is_empty())
                .collect();
            (load(&clim.weathers), regions)
        };
        let w = weather(world.lo, world.schema, wid).ok_or("weather unreadable")?;
        let world_image_space_id = worldspace_image_space_id(world);
        let image_space = world_image_space_id.and_then(|id| {
            let c = image_space(world.lo, world.schema, id);
            info!("image space {id:#010x}: {c:?}");
            c
        });
        let adapters = choices
            .iter()
            .chain(regions.iter().flat_map(|r| &r.choices))
            .map(|c| &c.weather)
            .chain(std::iter::once(&w))
            .map(|w| {
                let ids = weather_adapters(world.lo, world.schema, w);
                (w.form_id, ids.map(|id| id.and_then(|id| adapter(world.lo, world.schema, id))))
            })
            .collect();
        Ok(SkyState {
            climate: clim,
            weather: w,
            hour,
            fog_override: None,
            interior: None,
            interior_grade: None,
            shadows: false,
            shadow_distance: DEFAULT_SHADOW_DISTANCE,
            shadow_reach: 1.0,
            shadow_layout: CascadeLayout::Quality,
            grade: true,
            time_scale: 0.0,
            clock_running: false,
            days_passed: 0.0,
            keep_once: false,
            prev: None,
            fade: 1.0,
            choices,
            image_space,
            world_image_space_id,
            weather_fog_overrides: HashMap::new(),
            weather_visual_overrides: WeatherVisualOverrides::default(),
            adapters,
            changed: true,
            script: None,
            pinned: pinned.is_some(),
            regions,
            source: None,
            placed: false,
            region_cell: None,
            clock_seen: false,
            sun_hour: None,
            smooth_fade: false,
            fade_given: 0.0,
            clock_ahead: 0.0,
            rolled_for: None,
            pick: wid,
            rolls: 0,
            rng: seed,
            hour_index: None,
            live_globals: HashMap::new(),
            resume: start.map(|_| wid),
            flash: [0.0; 3],
            script_wind: None,
            moon_textures: Default::default(),
            lightning_request: false,
            moonlight: None,
            cloud_upgrade: None,
        })
    }

    /// [documented: GECK wiki ForceWeather / SetWeather / ReleaseWeatherOverride]
    /// An override lasts until released. [hypothesis, #1997; observer check 209 in
    /// docs/handoff/observer-checks.md] This also holds across worldspace doors.
    /// [design: worldspace.rs::carry_sky] Keep override visuals and their adapters,
    /// including an unfinished fade, without importing the old climate's roll lists.
    pub(crate) fn carry_weather_from(&mut self, from: &SkyState) {
        // The clock goes on in the same step: what it has read of it, and given the fade.
        if self.hour == from.hour {
            (self.clock_ahead, self.fade_given) = (from.clock_ahead, from.fade_given);
        }
        if from.script.is_none() && !from.pinned {
            return;
        }
        self.pinned = from.pinned;
        self.resume = from.resume;
        self.script = from.script.map(|sw| ScriptWeather { roll: self.rolls, ..sw });
        self.weather = from.weather.clone();
        self.prev = from.prev.clone();
        self.fade = from.fade;
        for w in std::iter::once(&self.weather).chain(self.prev.iter()) {
            if let Some(a) = from.adapters.get(&w.form_id) {
                self.adapters.insert(w.form_id, *a);
            }
        }
        self.changed = true;
        if self.script.is_some_and(|sw| sw.hold) {
            info!("weather: {} ({:08X}) held after sky replacement", self.weather.edid.as_deref().unwrap_or("?"), self.weather.form_id);
        }
    }

    /// Preserve the sampled cell and region across a climate-only rebuild [design,
    /// #2531; `worldspace::script_sky`]. Rebind the source by FormID into the new list.
    /// An invalidated travel placement stays invalidated (`Self::set_interior`).
    /// The caller seeds current live gates before this selects weather [design, #2531;
    /// `worldspace::carry_climate_sky`].
    pub(crate) fn carry_region_placement(&mut self, from: &Self) {
        self.region_cell = from.region_cell;
        self.source = from.source.and_then(|i| self.regions.iter().position(|r| r.region.form_id == from.regions[i].region.form_id));
        self.placed = from.placed;
        if self.region_cell.is_some() {
            self.follow_climate(true);
        }
    }

    /// A loaded game's part of the sky ([`SAVE_HOOK`]), before the first look at the weather
    /// lists ([`SkyState::place`]): a hold set after that look would hold what its roll
    /// chose. Only the weather the save holds is held: the sky starts on another when that
    /// one is gone from the load order or `--weather` names one, and nothing asked to hold
    /// those.
    pub fn restore_saved(&mut self, saved: &serde_json::Value) {
        if let Some(r) = saved.get("rng").and_then(serde_json::Value::as_str).and_then(|r| r.parse().ok()) {
            self.rng = r;
        }
        let held = saved.get("held").and_then(serde_json::Value::as_bool) == Some(true);
        let showing = saved.get("weather").and_then(serde_json::Value::as_str).is_some_and(|w| self.weather.edid.as_deref().is_some_and(|e| e.eq_ignore_ascii_case(w)));
        if held && showing {
            self.script = Some(ScriptWeather { hold: true, roll: self.rolls });
        }
    }

    /// Follow the sampled region (FormID; `Some(None)`: climate), or keep the list
    /// indoors (`None`). Placement can snap without repeating startup [design, #2531].
    pub fn place(&mut self, at: Option<Option<u32>>, instant: bool) {
        let first = !self.placed;
        self.placed = true;
        // Said here, not in `new`, which runs before the log is set up.
        if first {
            info!("weather: the sky starts on day {:.0} at {:02}:{:02}", self.days_passed, self.hour as u32 % 24, (self.hour.fract() * 60.0) as u32);
        }
        if first && !self.regions.is_empty() {
            let names: Vec<String> = self
                .regions
                .iter()
                .map(|r| format!("{} (priority {}, {} weathers)", r.region.edid.as_deref().unwrap_or("?"), r.region.priority, r.choices.len()))
                .collect();
            info!("weather: {} region(s) override the climate here: {}", self.regions.len(), names.join(", "));
        }
        if first && self.script.is_some_and(|sw| sw.hold) {
            info!("load: the weather stays held as a script left it");
        }
        if first {
            if let Some(ml) = self.moonlight {
                info!(
                    "sky: loaded {MOONLIGHT_INI_PATH} (bMoonPhaseBrightness={}, mults={:?})",
                    ml.phase_brightness, ml.phase_mults
                );
            }
            if let Some(cu) = self.cloud_upgrade {
                info!(
                    "sky: loaded {CLOUD_UPGRADE_INI_PATH} (Speed={}, MaxSpeed={}, Volatility={})",
                    cu.speed, cu.max_speed, cu.volatility
                );
            }
        }
        if let Some(region) = at {
            self.source = region.and_then(|f| self.regions.iter().position(|r| r.region.form_id == f));
        }
        self.follow_climate(instant || first || !self.clock_seen);
        // A release waits for the roll after this one, not for the start's own.
        if let Some(sw) = self.script.as_mut().filter(|_| first) {
            sw.roll = self.rolls;
        }
    }

    /// Sample the outline on exterior cell entry, holding the result within that cell
    /// [observed: captures run15b_w and run15b_w2, 2026-11-15; world.md §6, #2531].
    /// A fresh or invalidated placement snaps; walking across a cell border fades
    /// [observed: same captures, script MoveTo versus walking; hypothesis: other Travel].
    fn place_outdoors(&mut self, regions: &[RegionWeather], world: u32, x: f32, y: f32) {
        let cell = (world, fnv_world::cell_of(x, y));
        if self.region_cell == Some(cell) {
            return;
        }
        let instant = self.region_cell.is_none_or(|(previous_world, _)| previous_world != world);
        self.region_cell = Some(cell);
        let inside = fnv_world::sky::region_at(regions, world, x, y)
            .map(|r| r.form_id)
            .filter(|f| self.regions.iter().any(|r| r.region.form_id == *f));
        let now = self.source.map(|i| self.regions[i].region.form_id);
        if instant || !self.placed || inside != now {
            self.place(Some(inside), instant);
        }
    }

    /// The camera's region, if the place has been looked at.
    pub fn region(&self) -> Option<&str> {
        self.source.map(|i| self.regions[i].region.edid.as_deref().unwrap_or("?"))
    }

    /// The gating globals of the lists, to read from the simulation.
    pub fn gate_globals(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.choices.iter().chain(self.regions.iter().flat_map(|r| &r.choices)).filter_map(|c| c.global.as_ref().map(|g| g.id)).collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// A gating global's value as the simulation has it; a change looks at the lists again.
    pub fn set_live_global(&mut self, id: u32, v: f32) {
        if self.live_globals.insert(id, v) != Some(v) {
            self.follow_climate(!self.clock_seen);
        }
    }

    /// The list followed now: the region's or the climate's, and its name.
    fn list(&self) -> (&[Choice], String) {
        match self.source {
            Some(i) => {
                let r = &self.regions[i].region;
                (&self.regions[i].choices, format!("region {} (priority {})", r.edid.as_deref().unwrap_or("?"), r.priority))
            }
            None => (&self.choices, format!("climate {}", self.climate.edid.as_deref().unwrap_or("?"))),
        }
    }

    /// The entries of the list followed that a roll draws from now.
    fn eligible_now(&self) -> Vec<WeatherChoice> {
        let (list, _) = self.list();
        let h = self.hour;
        let entries: Vec<WeatherChoice> = list.iter().map(Choice::entry).collect();
        eligible(&entries, |g| {
            let gate = list.iter().find_map(|c| c.global.as_ref().filter(|x| x.id == g))?;
            gate_value(gate.edid.as_deref(), self.live_globals.get(&g).copied().or(gate.stored), h)
        })
    }

    /// Follow the weather system (docs/spec-notes/world.md §6 "Choosing a weather"): roll
    /// again when the list followed or its eligible weathers change, or when the
    /// climate's volatility calls for it at a whole game hour; show the pick unless a
    /// script's weather holds. A change fades, or is at once with `instant`.
    fn follow_climate(&mut self, instant: bool) {
        if self.pinned || !self.placed {
            return;
        }
        let src = self.source.map(|i| self.regions[i].region.form_id);
        let el = self.eligible_now();
        let key = (src, el.iter().map(|w| w.weather).collect::<Vec<u32>>());
        // The player's Improved Weather tweak: after a short fast travel nothing rolls,
        // the hours it took and a new region alike, and the weather shown stays
        // [design: docs/design/tweaks.md].
        if self.keep_once && self.clock_seen {
            self.keep_once = false;
            self.rolled_for = Some(key);
            self.hour_index = Some((self.days_passed as f64 * 24.0 + self.hour as f64).floor() as i64);
            self.pick = self.weather.form_id;
            info!("weather: kept through a short fast travel (tweak)");
            return;
        }
        let mut why = None;
        if self.rolled_for.as_ref() != Some(&key) {
            why = Some(match &self.rolled_for {
                None => "at the start",
                Some((s, _)) if *s != src && src.is_some() => "on entering the region",
                Some((s, _)) if *s != src => "on leaving a weather region",
                _ => "as the weathers open to it changed",
            });
            if let Some(p) = self.resume.take().filter(|p| key.1.contains(p)) {
                self.pick = p;
                self.rolled_for = Some(key.clone());
                why = None;
                info!("weather: {} kept from the saved game ({})", self.name_of(p), self.list().1);
            }
        }
        let idx = (self.days_passed as f64 * 24.0 + self.hour as f64).floor() as i64;
        if why.is_none() && self.climate.volatility > 0 {
            if let Some(prev) = self.hour_index.filter(|&p| idx > p) {
                for _ in prev..idx.min(prev + 24) {
                    if roll_due(self.climate.volatility, next_unit(&mut self.rng)) {
                        why = Some("by the climate's volatility");
                        break;
                    }
                }
            }
        }
        self.hour_index = Some(idx);
        if let Some(why) = why {
            let u = next_unit(&mut self.rng);
            if let Some(p) = roll(&el, u) {
                self.pick = p;
                self.rolls += 1;
                let total: i32 = el.iter().map(|w| w.chance.max(0)).sum();
                let chance = el.iter().find(|w| w.weather == p).map_or(0, |w| w.chance);
                info!(
                    "weather roll: {} ({chance} of {total} among {} eligible) from {} {why}, at {:02}:{:02}",
                    self.name_of(p),
                    el.len(),
                    self.list().1,
                    self.hour as u32 % 24,
                    (self.hour.fract() * 60.0) as u32
                );
            }
            self.rolled_for = Some(key);
        }
        if let Some(sw) = self.script {
            if sw.hold || sw.roll == self.rolls {
                return;
            }
            self.script = None;
            info!("weather: the climate takes over from the script's weather");
        }
        let pick = self.pick;
        if pick == self.weather.form_id {
            if instant {
                self.prev = None;
                self.fade = 1.0;
            }
            return;
        }
        if let Some(next) = self.weather_by_id(pick) {
            self.switch_to(next, instant);
        }
    }

    /// A weather of the lists by FormID.
    fn weather_by_id(&self, id: u32) -> Option<Weather> {
        self.choices.iter().chain(self.regions.iter().flat_map(|r| &r.choices)).find(|c| c.weather.form_id == id).map(|c| c.weather.clone())
    }

    fn name_of(&self, id: u32) -> String {
        match self.choices.iter().chain(self.regions.iter().flat_map(|r| &r.choices)).find(|c| c.weather.form_id == id) {
            Some(c) => c.weather.edid.clone().unwrap_or_else(|| format!("{id:08X}")),
            None => format!("{id:08X}"),
        }
    }

    /// Show `w`: a fade from the weather shown, or at once. A change while a fade is under
    /// way starts from what the screen shows: back to the weather fading out, the fade
    /// turns round where it stands; to a third one, it fades from whichever of the two
    /// shows more (crossing a region's edge and back mid-fade jumped the whole way).
    fn switch_to(&mut self, w: Weather, instant: bool) {
        let h = self.hour;
        self.fade_to(w, instant);
        info!("weather: {} at {:02}:{:02}", self.weather.edid.as_deref().unwrap_or("?"), h as u32, (h.fract() * 60.0) as u32);
        self.changed = true;
    }

    /// Make `w` the weather and set the fade to it ([`SkyState::switch_to`]).
    fn fade_to(&mut self, w: Weather, instant: bool) {
        let old = std::mem::replace(&mut self.weather, w);
        if instant {
            self.prev = None;
            self.fade = 1.0;
        } else {
            match self.prev.take() {
                Some(p) if p.form_id == self.weather.form_id => {
                    self.fade = 1.0 - self.fade.clamp(0.0, 1.0);
                    self.prev = Some(old);
                }
                Some(p) if self.fade < 0.5 => {
                    self.prev = Some(p);
                    self.fade = 0.0;
                }
                _ => {
                    self.prev = Some(old);
                    self.fade = 0.0;
                }
            }
        }
    }

    /// A script's weather (SetWeather fades to it, ForceWeather is `instant`), held
    /// against the weather system with `hold` until [`SkyState::release_script_weather`].
    pub fn script_weather(&mut self, w: Weather, adapters: [Option<Adapter>; 6], instant: bool, hold: bool) {
        self.adapters.entry(w.form_id).or_insert(adapters);
        self.script = Some(ScriptWeather { hold, roll: self.rolls });
        info!(
            "weather: {} by a script ({}{})",
            w.edid.as_deref().unwrap_or("?"),
            if instant { "at once" } else { "fading in" },
            if hold { ", held until released" } else { "" }
        );
        if w.form_id == self.weather.form_id {
            return;
        }
        self.fade_to(w, instant);
        self.changed = true;
    }

    /// ReleaseWeatherOverride: the weather system chooses again (at its next roll).
    pub fn release_script_weather(&mut self) {
        if let Some(sw) = self.script.as_mut() {
            sw.hold = false;
            info!("weather: a script releases its weather");
        }
        self.follow_climate(false);
    }

    /// Light a lightning flash adds now (linear RGB; zero when none).
    pub fn set_flash(&mut self, flash: [f32; 3]) {
        if flash != self.flash {
            self.flash = flash;
            self.changed = true;
        }
    }

    /// The weather shown is rainy (WTHR DATA) and its change is over (IsRaining).
    pub fn raining(&self) -> bool {
        self.weather.rainy && self.prev.is_none()
    }

    /// Set the hour; `instant` skips weather fades (a jump rather than time passing).
    pub fn set_hour(&mut self, hour: f32, instant: bool) {
        self.hour = hour.rem_euclid(24.0);
        self.sun_hour = None;
        (self.clock_ahead, self.fade_given) = (0.0, 0.0);
        self.follow_climate(instant);
        self.changed = true;
    }

    /// Keep the weather through the next change the sky would make (the Improved Weather
    /// tweak's short trip).
    pub fn keep_weather_once(&mut self) {
        self.keep_once = true;
    }

    /// `--play`: the sky shows the game's time of day (GameHour, which the simulation
    /// advances at TimeScale). Steps of half a game minute, so the weather is not
    /// re-evaluated every frame.
    pub fn follow_game_clock(&mut self, hour: f32, days_passed: f32) {
        self.clock_seen = true;
        let ahead = (hour - self.hour).rem_euclid(24.0);
        // While the volume is drawn an ongoing fade also moves between the steps, by the
        // interval since the last clock reading, so the cloud field and its cached sunlight
        // move together.
        if ahead < 1.0 / 120.0 {
            let give = (ahead - self.clock_ahead).min(ahead - self.fade_given);
            if self.smooth_fade && give > 0.0 {
                self.fade_given += give;
                self.advance_fade(give);
            }
            self.clock_ahead = ahead;
        }
        if (1.0 / 120.0..12.0).contains(&ahead) {
            self.days_passed = days_passed.floor();
            self.advance_fade((ahead - self.fade_given).max(0.0));
            self.set_hour(hour, false);
        } else if ahead >= 12.0 {
            // The game's clock was set back (or far on): a jump, not time passing.
            self.days_passed = days_passed.floor();
            self.set_hour(hour, true);
        }
        // Between the steps the sun light still moves with the clock (`sun_hour`).
        let ahead = (hour - self.hour).rem_euclid(24.0);
        self.sun_hour = (ahead < 1.0 / 120.0).then_some(hour.rem_euclid(24.0));
    }

    /// The hour the sun light's direction is taken from: the game clock's own between the
    /// sky's half-minute steps, otherwise the sky's.
    pub fn sun_light_hour(&self) -> f32 {
        self.sun_hour.unwrap_or(self.hour)
    }

    /// Advance a weather fade by `hours` of game time.
    fn advance_fade(&mut self, hours: f32) {
        if self.prev.is_some() {
            let secs = self
                .weather_visual_overrides
                .transition_seconds
                .unwrap_or_else(|| self.weather.transition_seconds());
            let length = secs * GAME_HOURS_PER_SECOND;
            self.fade = if length > 0.0 { (self.fade + hours / length).min(1.0) } else { 1.0 };
            if self.fade >= 1.0 {
                self.prev = None;
            }
            self.changed = true;
        }
    }

    /// Every colour type now as stored (0..255), for GetCurrentSkyColor
    /// (`fnv_game::nvse_jg9`).
    pub fn all_color_bytes(&self) -> [[f32; 3]; 10] {
        use fnv_world::sky::ColorType::*;
        [SkyUpper, Fog, Unused, Ambient, Sunlight, Sun, Stars, SkyLower, Horizon, EffectLighting].map(|t| self.color_bytes(t))
    }

    /// A weather colour at the current hour, blended across a weather fade, with any
    /// live session `SetWeatherRGBColor` overrides applied.
    pub fn color(&self, ty: ColorType) -> [f32; 3] {
        let of = |w: &Weather| w.color_with(&self.climate, ty, self.hour, self.weather_visual_overrides.colors.get(&w.form_id));
        let cur = of(&self.weather);
        match &self.prev {
            Some(p) => {
                let old = of(p);
                std::array::from_fn(|i| old[i] + (cur[i] - old[i]) * self.fade)
            }
            None => cur,
        }
    }

    /// A colour type now as stored (0..255 per channel, blended on the bytes;
    /// `fnv_world::sky::Weather::color_bytes`), blended across a weather fade, with any
    /// live session `SetWeatherRGBColor` overrides applied.
    pub fn color_bytes(&self, ty: ColorType) -> [f32; 3] {
        let of = |w: &Weather| w.color_bytes_with(&self.climate, ty, self.hour, self.weather_visual_overrides.colors.get(&w.form_id));
        let cur = of(&self.weather);
        match &self.prev {
            Some(p) => {
                let old = of(p);
                std::array::from_fn(|i| old[i] + (cur[i] - old[i]) * self.fade)
            }
            None => cur,
        }
    }

    /// Cloud layer `layer`'s colour for `w` at the current hour, with any live session
    /// `SetWeatherRGBColor` type-2 (`PNAM`) overrides applied.
    pub fn cloud_color_for(&self, w: &Weather, layer: usize) -> [f32; 3] {
        w.cloud_color_with(&self.climate, layer, self.hour, self.weather_visual_overrides.cloud_colors.get(&w.form_id))
    }

    /// Effective per-time-of-day image-space adapters for weather `weather_id`: authored
    /// `0IAD..5IAD` overlaid with any live session `SetWeatherImageSpaceMod` overrides.
    fn adapters_for(&self, weather_id: u32) -> [Option<Adapter>; 6] {
        let mut out = self.adapters.get(&weather_id).copied().unwrap_or([None; 6]);
        if let Some(ov) = self.weather_visual_overrides.adapters.get(&weather_id) {
            for (dst, val) in out.iter_mut().zip(ov) {
                if let Some(adapter) = val {
                    *dst = *adapter;
                }
            }
        }
        out
    }

    /// The image-space grade now: the base image space with the weather's adapters for
    /// the time of day, blended across a weather fade; none indoors yet [interim].
    pub fn cinematic(&self) -> Cinematic {
        if !self.grade {
            return Cinematic::default();
        }
        if self.interior.is_some() {
            return self.interior_grade.unwrap_or_default();
        }
        let Some(base) = self.image_space else { return Cinematic::default() };
        let at = |w: &Weather| {
            let adapters = self.adapters_for(w.form_id);
            cinematic_at(&base, &adapters, &self.climate, self.hour)
        };
        let cur = at(&self.weather);
        match &self.prev {
            Some(p) => at(p).lerp(&cur, self.fade),
            None => cur,
        }
    }

    /// How strong the outdoor sun and moon are against the Mojave's at this hour: the
    /// image space's Sunlight Dimmer with the weather's adapters, blended like the grade
    /// and across a weather fade, over the dimmer the Mojave's lighting was fitted under
    /// (`fnv_world::imagespace::sun_scale`; world.md §6). 1 without an image space. Kept
    /// when the grade is switched off: it is light, not a colour grade.
    pub fn sun_scale(&self) -> f32 {
        let Some(base) = self.image_space else { return 1.0 };
        let at = |w: &Weather| {
            let adapters = self.adapters_for(w.form_id);
            cinematic_at(&base, &adapters, &self.climate, self.hour).hdr.v[fnv_world::imagespace::HDR_SUNLIGHT_DIMMER]
        };
        let cur = at(&self.weather);
        let dimmer = match &self.prev {
            Some(p) => at(p) + (cur - at(p)) * self.fade,
            None => cur,
        };
        fnv_world::imagespace::sun_scale(dimmer, &self.climate, self.hour)
    }

    /// Fog near, far and strength (0..1), blended across a weather fade; `None` when
    /// neither weather has fog. Fading to or from a weather without fog fades the
    /// strength and keeps the other weather's distances. Applies any live session
    /// `SetWeatherTraitNumeric` (`15..=20`) overrides stored in [`Self::weather_fog_overrides`].
    pub fn fog(&self) -> Option<(f32, f32, f32)> {
        let fog_of = |w: &Weather| w.fog_at_with(&self.climate, self.hour, self.weather_fog_overrides.get(&w.form_id).copied());
        let cur = fog_of(&self.weather);
        let prev = match &self.prev {
            Some(p) => fog_of(p),
            None => return cur.map(|(n, f)| (n, f, 1.0)),
        };
        let t = self.fade;
        match (prev, cur) {
            (Some((pn, pf)), Some((n, f))) => Some((pn + (n - pn) * t, pf + (f - pf) * t, 1.0)),
            (Some((n, f)), None) => Some((n, f, 1.0 - t)),
            (None, Some((n, f))) => Some((n, f, t)),
            (None, None) => None,
        }
    }

    /// Fog power now (the weather's FNAM day/night power, blended across a weather fade;
    /// see `fnv_world::sky::Weather::fog_power_at`). An overridden fog (`--fog`) is linear.
    pub fn fog_power(&self) -> f32 {
        if self.fog_override.is_some() {
            return 1.0;
        }
        let fog_of = |w: &Weather| w.fog_at_with(&self.climate, self.hour, self.weather_fog_overrides.get(&w.form_id).copied());
        let power_of = |w: &Weather| w.fog_power_at_with(&self.climate, self.hour, self.weather_fog_overrides.get(&w.form_id).copied());
        let cur = power_of(&self.weather);
        match &self.prev {
            Some(p) if fog_of(p).is_some() && fog_of(&self.weather).is_some() => {
                let prev = power_of(p);
                prev + (cur - prev) * self.fade
            }
            Some(p) if fog_of(&self.weather).is_none() => power_of(p),
            _ => cur,
        }
    }

    /// How much brighter emitted light is drawn under the image space now: its adapted
    /// Emissive Mult ("multiplier applied to the emittance of an object" [documented:
    /// GECK wiki ImageSpace, HDR tab]) over the worldspace base image space's own
    /// [hypothesis: the renderer's emissive light was judged under the Mojave's base
    /// value, 1.5, which its adapters leave alone; Dead Money's Villa adapter multiplies
    /// it by 3.5 and Puesta del Sol's by 2, and there the lanterns' glass burns near
    /// white in the original. world.md §6 "Emissive mult"]. 1 indoors and without a base.
    pub fn emissive_scale(&self) -> f32 {
        use fnv_world::imagespace::HDR_EMISSIVE_MULT;
        if self.interior.is_some() {
            return 1.0;
        }
        let Some(base) = self.image_space.as_ref().map(|c| c.hdr.v[HDR_EMISSIVE_MULT]).filter(|&v| v > 0.0) else { return 1.0 };
        let now = self.cinematic().hdr.v[HDR_EMISSIVE_MULT];
        if now > 0.0 { now / base } else { 1.0 }
    }

    /// Recompute sky, light and fog next frame (e.g. after something else changed the fog).
    pub fn refresh(&mut self) {
        self.changed = true;
    }

    /// The image-space grade of an interior cell (CELL XCIM), if it names one.
    pub fn cell_grade(lo: &fnv_esm::LoadOrder, schema: &fnv_esm::Schema, cell: u32) -> Option<Cinematic> {
        fnv_world::lighting::cell_image_space(lo, schema, cell).and_then(|f| fnv_world::imagespace::image_space(lo, schema, f))
    }

    /// Switch between interior lighting (`Some`) and the worldspace's sky (`None`).
    pub fn set_interior(&mut self, lighting: Option<fnv_world::lighting::CellLighting>) {
        self.interior = lighting;
        // Every Travel calls this, including same-cell exterior moves [design, #2531;
        // world.rs::travel]. Recheck the destination without repeating startup logs.
        self.region_cell = None;
        self.changed = true;
    }

    /// Clearness factor (`0.05..=1.0`) driving the physical Rayleigh/Mie desert sky model
    /// across weather classes, fog range, and non-blue stylized skies (e.g. Dead Money).
    pub(crate) fn physical_sky_clearness(&self) -> f32 {
        let sun_y = self.sun_dir().y;
        let weather_clearness = |w: &Weather| -> f32 {
            let class_factor = if w.rainy || (w.classification & 4) != 0 {
                0.25
            } else if (w.classification & 2) != 0 {
                0.55
            } else {
                1.0
            };
            let fog_factor = w
                .fog_at(&self.climate, self.hour)
                .map_or(1.0, |(_, far)| smoothstep(4_000.0, 28_000.0, far).clamp(0.25, 1.0));
            let blueness = if sun_y > 0.08 {
                let u = w.color(&self.climate, ColorType::SkyUpper, self.hour);
                let sum = u[0] + u[1] + u[2];
                if sum > 1e-4 {
                    (((u[2] - u[0] * 0.8) / sum) * 3.0 + 0.5).clamp(0.15, 1.0)
                } else {
                    1.0
                }
            } else {
                1.0
            };
            class_factor * fog_factor * blueness
        };
        let cur = weather_clearness(&self.weather);
        let blended = match &self.prev {
            Some(p) => {
                let old = weather_clearness(p);
                old + (cur - old) * self.fade
            }
            None => cur,
        };
        blended.clamp(0.05, 1.0)
    }

    /// Physical solar spectral transmittance blended with weather sunlight attenuation.
    pub(crate) fn physical_sun_color(&self) -> [f32; 3] {
        let w_sun = Vec3::from_array(self.color(ColorType::Sunlight));
        let sun_dir = self.sun_dir().normalize_or(Vec3::Y);
        if sun_dir.y < 0.0 {
            return w_sun.to_array();
        }
        let clearness = self.physical_sky_clearness();
        let phys_weight = smoothstep(0.10, 0.85, clearness);
        let t_sun = physical_sun_transmittance(ATMO_RE + ATMO_H0, sun_dir.y, physical_mie_coeff(clearness) * 1.11);
        let w_luma = w_sun.dot(Vec3::new(0.2126, 0.7152, 0.0722));
        let weather_atten = (w_luma / 0.75).clamp(0.20, 1.15);
        w_sun.lerp(t_sun * weather_atten, phys_weight).to_array()
    }

    /// The daylight the volumetric clouds take under Physical lighting (volumetric_clouds.rs
    /// `inputs`), in the physical dome's own units (linear light, no tonemapper inverse): the
    /// sun colour the surfaces' sun light has ([`Self::physical_sun_color`]), and the sky's
    /// colour overhead and at the horizon as sky.wesl draws the dome (the weather's, times its
    /// day level, blended toward `eval_physical_sky` by the clearness, as physical_ibl.rs does
    /// too). The sky colours are divided by the sky's display scale, which the clouds apply
    /// themselves, so the product is the dome's [design].
    ///
    /// The sun carries the surfaces' physical gain (`PHYSICAL_SUN_GAIN`, by night
    /// `PHYSICAL_MOON_GAIN`, as [`aim_sun`] gives the ground): without it the clouds were lit by
    /// about a seventh of the ground's sun and came out grey against the sky behind them, once
    /// the exposure followed the brighter ground [observed: Goodsprings at 17:30, 2026-11-26].
    pub(crate) fn physical_cloud_light(&self) -> CloudDaylight {
        let sun_dir = self.sun_dir().normalize_or(Vec3::Y);
        let clearness = self.physical_sky_clearness();
        let phys_weight = smoothstep(0.10, 0.85, clearness);
        let day_vis = smoothstep(-0.04, 0.25, sun_dir.y);
        let scale = self.sky_scale().max(1e-6);
        let perp = Vec3::new(-sun_dir.z, 0.0, sun_dir.x).normalize_or(Vec3::X);
        let horizon_ray = (perp * 0.9975 + Vec3::Y * 0.07).normalize();
        let level = 0.45 + (1.35 - 0.45) * day_vis;
        let dome = |weather: [f32; 3], dir: Vec3| -> [f32; 3] {
            let weather = Vec3::from_array(weather) * level;
            weather.lerp(eval_physical_sky(dir, sun_dir, clearness) / scale, phys_weight).to_array()
        };
        let gain = if sun_dir.y >= 0.0 { PHYSICAL_SUN_GAIN } else { PHYSICAL_MOON_GAIN };
        let upper = dome(self.color(ColorType::SkyUpper), Vec3::Y);
        let horizon = dome(self.color(ColorType::Horizon), horizon_ray);
        CloudDaylight {
            sun: (Vec3::from_array(self.physical_sun_color()) * gain).to_array(),
            upper,
            horizon,
            ground: ((Vec3::from_array(upper) + Vec3::from_array(horizon)) * 0.5).to_array(),
        }
    }

    /// Horizon scattering colour matched to `eval_physical_sky` at 4° elevation so aerial
    /// perspective fog and the physical sky dome meet seamlessly at distant ridges.
    pub(crate) fn physical_horizon_color(&self) -> [f32; 3] {
        let sun_dir = self.sun_dir().normalize_or(Vec3::Y);
        let clearness = self.physical_sky_clearness();
        let phys_weight = smoothstep(0.10, 0.85, clearness);
        let perp = Vec3::new(-sun_dir.z, 0.0, sun_dir.x).normalize_or(Vec3::X);
        let horizon_ray = (perp * 0.9975 + Vec3::Y * 0.07).normalize();
        let phys_horizon = eval_physical_sky(horizon_ray, sun_dir, clearness);
        let weather_fog = Vec3::from_array(self.color(ColorType::Fog));
        let mut col = weather_fog.lerp(phys_horizon, phys_weight);
        if sun_dir.y < -0.05 {
            col = col.max(weather_fog);
        }
        col.to_array()
    }

    /// How daylight changes the sky's display colours now (see [`SkyLight`]).
    fn light(&self, fair_sun: f32, fair_disc: f32) -> SkyLight {
        // The disc and glare fade out as the sun sets [hypothesis: the game shows no glare
        // once the sun is down].
        let fade = if self.interior.is_some() { 0.0 } else { smoothstep(-2.0, 3.0, self.sun_elevation()) };
        let sun_col = Vec3::from_array(self.color(ColorType::Sun)) * fade;
        let south = SUN_SOUTH;
        let dir = game_to_bevy().rotation * Vec3::from_array(sun_direction(&self.climate, self.hour, south));
        let half_tan = |deg: f32| (deg / 2.0).to_radians().tan();
        let fair_value = |base: f32, fair: f32| base + (fair - base) * fair_sun;
        let [fair_size, fair_glare, fair_gain] = FairSun::get().at(self.sun_elevation());
        // The weather's Sun Glare, blended across a weather fade.
        let glare = self.weather.glare_scale();
        let glare = self.prev.as_ref().map_or(glare, |p| p.glare_scale() + (glare - p.glare_scale()) * self.fade);
        let clearness = if nvf_assets::authored::physical_pbr() && !crate::lights::display_clip() {
            self.physical_sky_clearness()
        } else {
            0.0
        };
        SkyLight {
            params: Vec4::new(self.sky_scale(), 0.0, 1.0, 0.0),
            // w: the main view's tonemapper curve (lights.rs `ToneCurve::code`, 1 display
            // values clipped), which the sky maps its display values for (tone_input.wesl).
            glow: Vec3::ZERO.extend(crate::lights::tone_curve().code()),
            // w: the physical sky's clearness (sky.wesl, physical lighting; 0 otherwise).
            sun_dir: dir.extend(clearness),
            sun: (sun_col * SUN_STRENGTH).extend(half_tan(fair_value(SUN_SIZE_DEG, fair_size))),
            glare: (sun_col * GLARE_STRENGTH * glare * fair_value(1.0, fair_gain)).extend(half_tan(fair_value(GLARE_SIZE_DEG, fair_glare))),
            disc: FairSun::get().disc_light(fair_value(SUN_SIZE_DEG, fair_size), fair_disc),
            // Written by night_sky.rs alone (update_sky keeps them).
            moon: Vec4::ZERO,
            night: Vec4::ZERO,
        }
    }

    /// What the sky's display colours are scaled by: the image space's "LUM Ramp No Tex"
    /// with the weather's adapter on it (1 indoors and where it is not over 0) [hypothesis,
    /// fitted 2026-11-16 to the original's sky in two worldspaces, world.md §6 "Sky
    /// level": the sky is 0.88 of the weather's colours by day in the Mojave
    /// (NVDefaultExterior 1.1 × NVWastelandIS 0.8) and 1.55 in the Capital Wasteland
    /// (WastelandDayISFXDC sets it); the emissive mult, 1.5 and 1.4 there, does not fit].
    pub fn sky_scale(&self) -> f32 {
        if self.interior.is_some() {
            return 1.0;
        }
        let v = self.cinematic().hdr.v[fnv_world::imagespace::HDR_LUM_RAMP_NO_TEX];
        if v > 0.0 && v.is_finite() { v } else { 1.0 }
    }

    /// Direction towards the sun, in Bevy's axes (also below the horizon).
    pub fn sun_dir(&self) -> Vec3 {
        game_to_bevy().rotation * Vec3::from_array(sun_direction(&self.climate, self.hour, SUN_SOUTH))
    }

    /// Direction towards the sun, in game axes (also below the horizon).
    pub fn sun_dir_game(&self) -> [f32; 3] {
        sun_direction(&self.climate, self.hour, SUN_SOUTH)
    }

    /// Compass heading of the sun in degrees (0 north, 90 east).
    pub fn sun_heading(&self) -> f32 {
        let d = sun_direction(&self.climate, self.hour, SUN_SOUTH);
        d[0].atan2(d[1]).to_degrees().rem_euclid(360.0)
    }

    /// Sun elevation above the horizon in degrees.
    pub fn sun_elevation(&self) -> f32 {
        sun_direction(&self.climate, self.hour, SUN_SOUTH)[2].clamp(-1.0, 1.0).asin().to_degrees()
    }

    /// Sun elevation in degrees at another `hour` of the same climate (any value; the
    /// heat haze's afternoon lag, `heat_haze.rs`).
    pub fn sun_elevation_at(&self, hour: f32) -> f32 {
        sun_direction(&self.climate, hour, SUN_SOUTH)[2].clamp(-1.0, 1.0).asin().to_degrees()
    }

    /// Star and moon visibility (0..1) at this hour.
    pub fn night_visibility(&self) -> (f32, f32) {
        let e = self.sun_elevation();
        (smoothstep(STARS_FADE.0, STARS_FADE.1, e), smoothstep(MOON_FADE.0, MOON_FADE.1, e))
    }

    /// Index into MOON_PHASES for the current day.
    pub fn moon_phase(&self) -> usize {
        moon_phase(self.days_passed + self.hour / 24.0, self.climate.moon_phase_length())
    }

    /// Load optional `Data\NVSE\Plugins\MoonlightNVSE.ini` and
    /// `Data\NVSE\Plugins\CloudUpgrade.ini` configurations from `vfs` (case-insensitive).
    /// Absent files leave both fields `None` and vanilla sky behaviour unchanged.
    pub fn load_plugin_inis(&mut self, vfs: &fnv_vfs::Vfs) {
        self.moonlight = MoonlightConfig::load_from_vfs(vfs);
        self.cloud_upgrade = CloudUpgradeConfig::load_from_vfs(vfs);
    }

    /// Nighttime directional moonlight intensity multiplier from `MoonlightNVSE.ini`:
    /// returns `1.0` when the INI is absent, disabled (`bMoonPhaseBrightness=0`), indoors,
    /// or when the active climate has no moon [design].
    pub fn moon_brightness_multiplier(&self) -> f32 {
        if self.interior.is_some() || !self.climate.has_moon() {
            return 1.0;
        }
        match self.moonlight {
            Some(cfg) => cfg.multiplier_for_phase(self.moon_phase()),
            None => 1.0,
        }
    }

    /// Cloud layer scroll rate (texture repeats per real second): exact vanilla
    /// `Weather::cloud_scroll` when `CloudUpgrade.ini` is absent, otherwise modulated
    /// by `CloudUpgradeConfig`.
    pub fn cloud_scroll(&self, weather: &Weather, layer: usize) -> f32 {
        match self.cloud_upgrade {
            Some(cfg) => cfg.cloud_scroll(weather, layer),
            None => weather.cloud_scroll(layer),
        }
    }

    /// Cloud layer UV scroll offset (`0..1` repeats) at `secs` real seconds: exact vanilla
    /// `Weather::cloud_offset` when `CloudUpgrade.ini` is absent, otherwise modulated
    /// by `CloudUpgradeConfig`.
    pub fn cloud_offset(&self, weather: &Weather, layer: usize, secs: f64) -> f32 {
        match self.cloud_upgrade {
            Some(cfg) => cfg.cloud_offset(weather, layer, secs),
            None => weather.cloud_offset(layer, secs),
        }
    }

    /// The time, sun elevation and the luminance of the light and sky colours at this
    /// hour (for the time-lapse log).
    pub fn sample(&self) -> String {
        let h = self.hour;
        let lum = |ty| {
            let v = self.color(ty);
            0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2]
        };
        let elevation = self.sun_elevation();
        let (stars, moon) = self.night_visibility();
        format!(
            "day {:.0} {:02}:{:02}  {}  sun elevation {elevation:6.1}°  stars {stars:.2} moon {moon:.2} ({})  luminance: sunlight {:.3} ambient {:.3} sky-upper {:.3} horizon {:.3} fog {:.3}",
            self.days_passed,
            h as u32 % 24,
            (h.fract() * 60.0) as u32,
            self.weather.edid.as_deref().unwrap_or("?"),
            MOON_PHASES[self.moon_phase()],
            lum(ColorType::Sunlight),
            lum(ColorType::Ambient),
            lum(ColorType::SkyUpper),
            lum(ColorType::Horizon),
            lum(ColorType::Fog)
        )
    }

    pub fn describe(&self) -> String {
        if self.interior.is_some() {
            return "interior lighting (XCLL / template)".into();
        }
        format!(
            "{:02}:{:02}  climate {}  weather {}{}",
            self.hour as u32 % 24,
            ((self.hour.fract()) * 60.0) as u32,
            self.climate.edid.as_deref().unwrap_or("?"),
            self.weather.edid.as_deref().unwrap_or("?"),
            self.region().map_or(String::new(), |r| format!("  region {r}"))
        )
    }
}

/// Sample weather regions at cell entry or placement, using the camera point
/// [design: `SkyState::place_outdoors`, #2531]. Indoors keep the list [hypothesis].
pub fn weather_region(world: Res<WorldData>, cam: Query<&Transform, With<FlyCam>>, mut sky: ResMut<SkyState>) {
    let Ok(t) = cam.single() else { return };
    if sky.interior.is_some() {
        if !sky.placed {
            sky.place(None, true);
        }
        return;
    }
    let g = crate::world::bevy_to_game(t.translation);
    sky.place_outdoors(world.weather_regions, world.world, g.x, g.y);
}

/// The FormID of the worldspace's image space (WRLD INAM), or its parent's.
pub(crate) fn worldspace_image_space_id(world: &WorldData) -> Option<u32> {
    let inam = |id: u32| {
        let (pi, r) = fnv_world::fields::winner(world.lo, id)?;
        let f = fnv_world::fields::Fields::new(r, pi, world.schema).ok()?;
        fnv_world::fields::global(world.lo, pi, fnv_world::fields::formid_of(&f.get(b"INAM")?)?)
    };
    let ws = world.index.worldspaces.get(&world.world)?;
    inam(world.world).or_else(|| ws.parent.and_then(inam))
}

/// Sync live session `SetWeatherTraitNumeric` fog overrides (`15..=20`) from `Game::traits`
/// into [`SkyState::weather_fog_overrides`]. Keyed on `(game_instance_id, weather_fog_revision)`
/// so unchanged frames do zero work and game-instance switches or runtime drops clear stale
/// session overrides cleanly. Explicit console/CLI `sky.fog_override` precedence stays
/// intact in [`apply_cell_fog`] and [`SkyState::fog_power`].
pub fn sync_weather_fog(
    script: Option<Res<nvf_play::script::ScriptState>>,
    mut sky: ResMut<SkyState>,
    mut seen: Local<Option<Option<(u64, u64)>>>,
) {
    let game = script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game);
    let token = game.map(|g| g.weather_fog_sync_token());
    if *seen == Some(token) {
        return;
    }
    *seen = Some(token);
    let next = game.map(|g| g.weather_fog_overrides()).unwrap_or_default();
    if sky.weather_fog_overrides != next {
        sky.weather_fog_overrides = next;
        sky.refresh();
    }
}

/// Sync live session `SetWeatherRGBColor`, `SetWeatherImageSpaceMod`, and
/// `SetWeatherTransitionTimeOverride` overrides from `Game` into
/// [`SkyState::weather_visual_overrides`]. Keyed on `(game_instance_id,
/// weather_visual_revision)` so unchanged frames do zero work and game-instance switches
/// or runtime drops clear stale session overrides cleanly.
pub fn sync_weather_visuals(
    world: Res<WorldData>,
    script: Option<Res<nvf_play::script::ScriptState>>,
    mut sky: ResMut<SkyState>,
    mut seen: Local<Option<Option<(u64, u64)>>>,
) {
    let game = script.as_deref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game);
    let token = game.map(|g| g.weather_visual_sync_token());
    if *seen == Some(token) {
        return;
    }
    *seen = Some(token);
    let next = match game {
        Some(g) => {
            let adapters = g
                .weather_adapter_overrides()
                .into_iter()
                .map(|(wid, slots)| {
                    let resolved = slots.map(|slot| {
                        slot.map(|id| (id != 0).then(|| adapter(world.lo, world.schema, id)).flatten())
                    });
                    (wid, resolved)
                })
                .collect();
            WeatherVisualOverrides {
                colors: g.weather_color_overrides(),
                cloud_colors: g.weather_cloud_color_overrides(),
                adapters,
                transition_seconds: g.weather_transition_override().map(|s| s as f32).filter(|s| s.is_finite()),
            }
        }
        None => WeatherVisualOverrides::default(),
    };
    if sky.weather_visual_overrides != next {
        sky.weather_visual_overrides = next;
        sky.refresh();
    }
}

/// A global variable's Editor ID.
fn global_edid(world: &WorldData, fid: u32) -> Option<String> {
    let (pi, r) = fnv_world::fields::winner(world.lo, fid)?;
    fnv_world::fields::Fields::new(r, pi, world.schema).ok()?.string(b"EDID")
}

/// A global variable's value by Editor ID (e.g. `GameDaysPassed`).
pub fn find_global(world: &WorldData, edid: &str) -> Option<f32> {
    world.lo.records.iter().enumerate().find_map(|(pi, recs)| {
        recs.iter().filter(|r| &r.header.sig == b"GLOB").find_map(|r| {
            let f = fnv_world::fields::Fields::new(r, pi, world.schema).ok()?;
            f.string(b"EDID")?.eq_ignore_ascii_case(edid).then(|| f.get(b"FLTV")?.as_f64().map(|v| v as f32)).flatten()
        })
    })
}

fn find_weather(world: &WorldData, name: &str) -> Option<u32> {
    if let Some(hex) = name.strip_prefix("0x") {
        return u32::from_str_radix(hex, 16).ok();
    }
    world.lo.records.iter().enumerate().find_map(|(pi, recs)| {
        recs.iter().filter(|r| &r.header.sig == b"WTHR").find_map(|r| {
            let f = fnv_world::fields::Fields::new(r, pi, world.schema).ok()?;
            let e = f.string(b"EDID")?;
            e.eq_ignore_ascii_case(name).then(|| world.lo.to_global(pi, r.header.form_id)).flatten()
        })
    })
}

/// The atmosphere dome, with each vertex's weights of the horizon, sky-lower and sky-upper
/// colours (the mesh's vertex colour: red, green, blue) kept from creation: the colours
/// are recomputed from these ([`dome_color`]).
#[derive(Component)]
pub struct SkyDome(Vec<[f32; 3]>);

impl SkyDome {
    /// The weights per vertex, in the mesh's vertex order (aerial.rs reads the dome's own
    /// weights by elevation).
    pub fn weights(&self) -> &[[f32; 3]] {
        &self.0
    }
}

#[cfg(test)]
impl SkyDome {
    /// A dome without vertex weights (other modules' tests: night_sky.rs, the volumetric clouds).
    pub(crate) fn for_test() -> Self {
        SkyDome(Vec::new())
    }
}

/// One of the four cloud layers (WTHR layer index) in one of two sets: the set showing
/// the current weather and the one fading out the previous weather.
#[derive(Component)]
pub struct CloudLayer {
    layer: usize,
    set: usize,
    /// Form ID of the weather whose texture this layer shows.
    shows: Option<u32>,
}

impl CloudLayer {
    /// The WTHR cloud layer (0-3) this dome shape draws (cloud_shadows.rs).
    pub(crate) fn layer(&self) -> usize {
        self.layer
    }

    /// A layer of set 0 showing nothing yet, for other modules' tests.
    #[cfg(test)]
    pub(crate) fn for_test(layer: usize) -> Self {
        CloudLayer { layer, set: 0, shows: None }
    }
}

#[derive(Component)]
pub struct Sun;

/// Render layer of what casts a sun shadow without being seen: the sun casts from it and
/// from the world's layer 0, and no camera draws it (the player's body in first person,
/// `player_body.rs`). Layer 1 is the water's (`reflection::WATER_LAYER`).
pub const SHADOW_LAYER: usize = 2;

/// Render layer of the player's first-person arms and their weapon: seen by the cameras,
/// and the sun does not cast from it (`player_body.rs`).
pub const ARMS_LAYER: usize = 3;

/// Render layer of what the cameras see of the player's third-person body and its weapon:
/// the sun casts from it, the flashlight does not (`flashlight.rs`; owner 2026-10-22: the
/// player casts no flashlight shadow, as the arms did aiming in third person).
pub const BODY_LAYER: usize = 4;

/// Alpha_Texture in a BSShaderProperty's shader flags (bit 8) [documented: nif.xml BSShaderFlags].
const ALPHA_TEXTURE: i64 = 1 << 8;

/// What a star dome adds, as a share of its texture's display value × the weather's
/// stars colour [hypothesis, fitted 2026-10-23 to the original's clear Mojave night at
/// 23:00 (star peaks p50 39, p90 69, max 143 grey in a 1280 × 720 frame at FOV 75; this
/// value gives 40, 76, 130); the same value leaves the Big MT's stars about 1.7 × the
/// original's, as much as our grade lifts that sky's background (world.md §6 "Star
/// domes")]. The material's emissive multiplier (× 1 Mojave, × 10 Big MT) is not used.
const STAR_STRENGTH: f32 = 0.25;

/// The climate's star model.
#[derive(Component)]
pub struct Stars;

/// The moon disc with one texture per phase (MOON_PHASES order).
#[derive(Component)]
pub struct MoonDisc {
    phases: Vec<Option<Handle<Image>>>,
    shown: Option<usize>,
}

/// Geometry of one shape of a sky mesh, in game space (z up).
struct SkyShape {
    /// The SkyShaderProperty's texture (file_name), if any.
    texture: Option<String>,
    /// The SkyShaderProperty's shader flags set Alpha_Texture (bit 8).
    alpha_texture: bool,
    positions: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    uv: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

/// The triangle shapes of a sky NIF in file order (their transforms are the identity in
/// the game's sky meshes [observed]).
fn sky_shapes(data: &GameData, path: &str) -> Vec<SkyShape> {
    let Some(bytes) = data.read(path) else {
        warn!("{path}: not found; using the fallback sky");
        return Vec::new();
    };
    // A panic reading a broken file counts as a file with no shapes (the fallback sky).
    let read = nvf_assets::guard::guarded(|| Ok(read_sky_shapes(data, &bytes)));
    read.unwrap_or_else(|e| {
        warn!("{path}: not read ({e}); using the fallback sky");
        Vec::new()
    })
}

fn read_sky_shapes(data: &GameData, bytes: &[u8]) -> Vec<SkyShape> {
    let Ok(file) = NifFile::parse(bytes, Some(&data.schema)) else { return Vec::new() };
    let dec = Decoded::new(&file, bytes, &data.schema);
    scene_objects(&file, &dec)
        .iter()
        .filter(|o| !o.is_node)
        .filter_map(|o| {
            let shape = dec.blocks[o.block].as_ref().ok()?;
            let md = mesh_data(dec.get(shape.block_ref("data")?)?);
            let n = md.positions.len();
            if n == 0 || md.triangles.is_empty() {
                return None;
            }
            let colors = if md.colors.len() == n { md.colors } else { vec![[1.0; 4]; n] };
            let uv = md.uv_sets.into_iter().next().filter(|u| u.len() == n).unwrap_or_else(|| vec![[0.0; 2]; n]);
            let indices = md.triangles.iter().filter(|t| t.iter().all(|&i| (i as usize) < n)).flatten().copied().collect();
            let props: Vec<_> = shape.refs("properties").unwrap_or_default().iter().filter_map(|&r| dec.get(r)).collect();
            let texture = props.iter().find_map(|p| p.text("file_name").filter(|t| !t.is_empty()).map(str::to_string));
            let alpha_texture = props.iter().filter_map(|p| p.int("shader_flags")).any(|f| f & ALPHA_TEXTURE != 0);
            Some(SkyShape { texture, alpha_texture, positions: md.positions, colors, uv, indices })
        })
        .collect()
}

fn sky_mesh(s: &SkyShape, colors: Vec<[f32; 4]>) -> Mesh {
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, s.positions.clone());
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 0.0, 1.0]; s.positions.len()]);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, s.uv.clone());
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    m.insert_indices(Indices::U32(s.indices.clone()));
    m
}

/// Fallback dome: a sphere (game space, z up) with weights from height, in the sky
/// mesh's channels (red horizon, green sky-lower, blue sky-upper): sky-lower to sky-upper
/// over the lower half of the upper hemisphere, sky-lower to the horizon colour just below
/// the horizon.
fn fallback_dome() -> SkyShape {
    let mut sphere = Sphere::new(NIF_SKY_RADIUS).mesh().uv(48, 24);
    let pos: Vec<[f32; 3]> = match sphere.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(p)) => p.iter().map(|v| [v[0], -v[2], v[1]]).collect(),
        _ => Vec::new(),
    };
    let colors = pos
        .iter()
        .map(|p| {
            let y = p[2] / NIF_SKY_RADIUS;
            if y >= 0.0 {
                let t = (y / 0.5).clamp(0.0, 1.0).sqrt();
                [0.0, 1.0 - t, t, 1.0]
            } else {
                // Sky-lower turns to the horizon colour over the first fifth below the horizon.
                let t = (-y / 0.2).clamp(0.0, 1.0);
                [t, 1.0 - t, 0.0, 1.0]
            }
        })
        .collect();
    let n = pos.len();
    let indices = match sphere.indices_mut() {
        Some(Indices::U32(i)) => i.clone(),
        Some(Indices::U16(i)) => i.iter().map(|&x| x as u32).collect(),
        None => Vec::new(),
    };
    SkyShape { texture: None, alpha_texture: false, positions: pos, colors, uv: vec![[0.0; 2]; n], indices }
}

/// Transform of a sky mesh around the camera: game space to Bevy, scaled to the dome.
fn sky_transform(at: Vec3) -> Transform {
    Transform { translation: at, rotation: game_to_bevy().rotation, scale: Vec3::splat(DOME_RADIUS / NIF_SKY_RADIUS) }
}

#[allow(clippy::too_many_arguments)]
pub fn setup_sky(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SkyMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut textures: ResMut<TextureCache>,
    data: Res<GameData>,
    sky: Res<SkyState>,
    map: Option<Res<bevy::light::DirectionalLightShadowMap>>,
) {
    let sun_tex = sky.climate.sun_texture.as_deref().and_then(|t| textures.get(t, true, &data, &mut images, None));
    let glare_tex = sky.climate.sun_glare_texture.as_deref().and_then(|t| textures.get(t, true, &data, &mut images, None));
    let atmosphere = sky_shapes(&data, "meshes\\sky\\atmosphere.nif").into_iter().next().unwrap_or_else(fallback_dome);
    let weights = atmosphere.colors.clone();
    // Per vertex: the weights of the horizon, sky-lower and sky-upper colours (red,
    // green, blue).
    let dome: Vec<[f32; 3]> = weights.iter().map(|w| [w[0], w[1], w[2]]).collect();
    // The same weights by height, for the reflection cube's sky (reflection_sky.rs).
    commands.insert_resource(crate::reflection_sky::DomeProfile::from_vertices(&atmosphere.positions, &atmosphere.colors));
    commands.spawn((
        Name::new("Sky"),
        SkyDome(dome),
        Mesh3d(meshes.add(sky_mesh(&atmosphere, weights))),
        // Opaque, so it is drawn before every transparent part of the world.
        MeshMaterial3d(materials.add({
            let mut m = sky_material(StandardMaterial::default(), 2.0);
            m.extension.sun_texture = sun_tex;
            m.extension.glare_texture = glare_tex.clone();
            m
        })),
        sky_transform(Vec3::ZERO),
        NotShadowCaster,
        NotShadowReceiver,
    ));
    for (layer, shape) in sky_shapes(&data, "meshes\\sky\\clouds.nif").iter().take(4).enumerate() {
        // Vertex alpha fades the layer out; the colour comes from the weather.
        let colors: Vec<[f32; 4]> = shape.colors.iter().map(|c| [1.0, 1.0, 1.0, c[3]]).collect();
        let mesh = meshes.add(sky_mesh(shape, colors));
        for set in 0..2 {
            commands.spawn((
                Name::new(format!("Cloud layer {layer}.{set}")),
                CloudLayer { layer, set, shows: None },
                Mesh3d(mesh.clone()),
                MeshMaterial3d(materials.add({
                    let mut m = sky_material(
                        StandardMaterial { alpha_mode: AlphaMode::Blend, depth_bias: SKY_SORT_BIAS + (2 * layer + set) as f32, ..default() },
                        1.0,
                    );
                    m.extension.glare_texture = glare_tex.clone();
                    m
                })),
                // Inside the atmosphere dome, which writes depth ([`CLOUD_INSET`]).
                sky_transform(Vec3::ZERO).with_scale(Vec3::splat(DOME_RADIUS / NIF_SKY_RADIUS * CLOUD_INSET)),
                Visibility::Hidden,
                NotShadowCaster,
                NotShadowReceiver,
            ));
        }
    }
    // Stars: inside the atmosphere dome, drawn before the moon, sun and clouds.
    if let Some(model) = sky.climate.model.as_deref() {
        let shapes = sky_shapes(&data, &format!("meshes\\{model}"));
        // The star sphere is about 16 units in radius [observed: Sky\Stars.nif].
        let radius = shapes
            .iter()
            .flat_map(|s| s.positions.iter())
            .map(|p| Vec3::from_array(*p).length())
            .fold(0.0f32, f32::max)
            .max(1.0);
        for shape in &shapes {
            let tex = shape.texture.as_deref().and_then(|t| textures.get(t, true, &data, &mut images, None));
            commands.spawn((
                Name::new("Stars"),
                Stars,
                Mesh3d(meshes.add(sky_mesh(shape, vec![[1.0; 4]; shape.positions.len()]))),
                MeshMaterial3d(materials.add({
                    let mut m = sky_material(StandardMaterial {
                        base_color: Color::LinearRgba(LinearRgba::new(0.0, 0.0, 0.0, 1.0)),
                        base_color_texture: tex,
                        alpha_mode: AlphaMode::Add,
                        depth_bias: SKY_SORT_BIAS - 30.0,
                        ..default()
                    }, 0.0);
                    // Additive (NiAlphaProperty ONE/ONE): the texture's alpha weighs the
                    // colour only where the shader flags set Alpha_Texture (sky.wesl).
                    m.extension.light.params.y = if shape.alpha_texture { 1.0 } else { 0.0 };
                    m
                })),
                Transform { rotation: game_to_bevy().rotation, scale: Vec3::splat(0.9 * SKY_INNER / radius), ..default() },
                NotShadowCaster,
                NotShadowReceiver,
            ));
        }
    }
    if sky.climate.has_moon() {
        // A script's texture for a phase in place of the phase's own (SetMoonTexture).
        let phases = MOON_PHASES
            .iter()
            .zip(&sky.moon_textures)
            .map(|(p, set)| match set {
                Some(t) => textures.get(t, true, &data, &mut images, None),
                None => textures.get(&format!("sky\\masser_{p}.dds"), true, &data, &mut images, None),
            })
            .collect();
        commands.spawn((
            Name::new("Moon disc"),
            MoonDisc { phases, shown: None },
            Mesh3d(meshes.add(Mesh::from(Rectangle::new(MOON_SIZE, MOON_SIZE)))),
            MeshMaterial3d(materials.add(moon_material())),
            Transform::from_scale(Vec3::ZERO),
            NotShadowCaster,
            NotShadowReceiver,
        ));
    }
    let mut sun = commands.spawn((
        Name::new("Sun"),
        Sun,
        DirectionalLight { illuminance: PI * light_unit(), shadow_maps_enabled: sky.shadows, ..default() },
        Transform::IDENTITY,
        // A light's shadow casters are the entities on its layers; it lights the views
        // whose layers meet its own, so the main and the reflection camera as before.
        RenderLayers::from_layers(&[0, SHADOW_LAYER, BODY_LAYER]),
    ));
    if sky.shadows {
        sun.insert(shadow_cascades(sky.shadow_distance, sky.shadow_reach, sky.shadow_layout, map_side(map.as_deref())));
    }
}

/// How far toward the sun each of the sun's shadow cascades reaches before its casters
/// are squashed onto its near plane, past the nearest of the view's slice (game units).
/// Bevy puts a cascade's near plane at the slice's corner nearest the sun, which with the
/// sun behind the camera lies just in front of the eye: the player's unseen body and a
/// wall behind it would both land on that plane, and the arms' shadows from beyond the
/// body (nif_material.wesl `shadow_past_body`) could not tell them apart. Pushed back by
/// this much the body keeps its true depth [engineering choice: well past the body's
/// reach behind the eye, about 100 units].
pub const SUN_REACH_BACK: f32 = 256.0;

/// Move the near plane of a directional light's cascade `extra` units further toward the
/// light, keeping its far plane: the depth range (reverse Z, near 1, far 0) is stretched
/// over the longer box. Its projection and view-projection change alike.
pub fn reach_toward_light(cascade: &mut bevy::light::cascade::Cascade, extra: f32) {
    // Bevy's projection maps the light-space depth z (0 at the near plane, -range at the
    // far) to r z + 1 with r = 1 / range; wanted is r' (z - extra) + 1 with
    // r' = 1 / (range + extra), so NDC depth d becomes s d + 1 - s - r' extra, s = r' / r.
    let r = cascade.clip_from_cascade.z_axis.z;
    if r <= 0.0 || extra <= 0.0 {
        return;
    }
    let r_new = 1.0 / (1.0 / r + extra);
    let s = r_new / r;
    let remap = Mat4::from_cols(Vec4::X, Vec4::Y, Vec4::new(0.0, 0.0, s, 0.0), Vec4::new(0.0, 0.0, 1.0 - s - r_new * extra, 1.0));
    cascade.clip_from_cascade = remap * cascade.clip_from_cascade;
    cascade.clip_from_world = remap * cascade.clip_from_world;
}

/// Stretch the sun's shadow cascades [`SUN_REACH_BACK`] toward the sun (PostUpdate, after
/// Bevy builds them each frame and before their frusta and shadow views are made; after
/// they are anchored and before the held ones are given back: shadow_anchor.rs
/// `cascade_edits` says why).
pub fn reach_toward_sun(mut suns: Query<&mut bevy::light::Cascades, With<Sun>>) {
    for mut cascades in &mut suns {
        for view in cascades.cascades.values_mut() {
            for cascade in view.iter_mut() {
                reach_toward_light(cascade, SUN_REACH_BACK);
            }
        }
    }
}

/// Drop the sun's casters that cannot shade anything a cascade covers. Seen from the sun,
/// a caster shades a receiver only if it covers it, and a cascade's receivers all lie in
/// the slice of the camera's view it was fitted to; Bevy draws everything in the square
/// around that slice's outline (`bevy_light` `calculate_cascade`), so the square's corners
/// hold casters for nothing. Kept: whatever comes within [`SLICE_MARGIN_TEXELS`] of the
/// outline, for the shadow filter's reach.
#[allow(clippy::type_complexity)]
pub fn sun_casters_over_slice(
    mut suns: Query<(&mut bevy::camera::visibility::CascadesVisibleEntities, &bevy::light::Cascades, &CascadeShadowConfig, &GlobalTransform), With<Sun>>,
    views: Query<(&GlobalTransform, &Projection)>,
    casters: Query<(&bevy::camera::primitives::Aabb, &GlobalTransform)>,
    share: Option<Res<crate::shadow_rate::MirrorShadowShare>>,
    hold: Option<Res<bevy::pbr::HoldDirectionalShadowMaps>>,
) {
    let shared_mirror = share.as_ref().and_then(|s| s.0).map(|(_, mirror)| mirror);
    let held_mask = hold.as_ref().map_or(0, |h| h.0);
    for (mut visible, cascades, config, light) in &mut suns {
        let light_from_world = Mat3::from_quat(light.rotation()).transpose();
        let lx = light_from_world.row(0);
        let ly = light_from_world.row(1);
        let near_bounds = std::iter::once(config.minimum_distance).chain(config.bounds.iter().map(|b| b * (1.0 - config.overlap_proportion)));
        let bounds: Vec<(f32, f32)> = near_bounds.zip(config.bounds.iter().copied()).collect();
        for (view, lists) in visible.entities.iter_mut() {
            // `shadow_rate::share_mirror_casters` runs right after this system and replaces
            // the shared mirror's caster lists with the main camera's lists [design].
            if shared_mirror == Some(*view) {
                continue;
            }
            let (Ok((at, projection)), Some(cs)) = (views.get(*view), cascades.cascades.get(view)) else { continue };
            let world_from_view = at.affine();
            for (cascade_idx, ((list, c), (near, far))) in lists.iter_mut().zip(cs).zip(&bounds).enumerate() {
                // Held cascades are not redrawn this frame (`HoldDirectionalShadowMaps`) [design].
                if cascade_idx < 32 && (held_mask & (1u32 << cascade_idx)) != 0 {
                    continue;
                }
                let outline = hull(projection.get_frustum_corners(-near, -far).map(|p| {
                    let wp = Vec3::from(world_from_view.transform_point3a(p));
                    Vec2::new(lx.dot(wp), ly.dot(wp))
                }));
                let edges = outline_edges(&outline);
                let margin = SLICE_MARGIN_TEXELS * c.texel_size;
                list.entities.retain(|e| {
                    let Ok((aabb, at)) = casters.get(*e) else { return true };
                    let place = at.affine();
                    let world_c = Vec3::from(place.transform_point3a(aabb.center));
                    let centre = Vec2::new(lx.dot(world_c), ly.dot(world_c));
                    let c0 = Vec3::from(place.matrix3.x_axis);
                    let c1 = Vec3::from(place.matrix3.y_axis);
                    let c2 = Vec3::from(place.matrix3.z_axis);
                    let half = Vec3::from(aabb.half_extents);
                    let reach = Vec2::new(
                        Vec3::new(lx.dot(c0), lx.dot(c1), lx.dot(c2)).abs().dot(half) + margin,
                        Vec3::new(ly.dot(c0), ly.dot(c1), ly.dot(c2)).abs().dot(half) + margin,
                    );
                    overlaps_edges(&edges, centre, reach)
                });
            }
        }
    }
}

/// How far past a slice's outline a caster still counts, in the cascade's shadow-map
/// texels [engineering choice: wider than the shadow filter's reach, a few texels].
const SLICE_MARGIN_TEXELS: f32 = 8.0;

/// Precomputed outward unit normal, its absolute value, and support offset `n · a` for
/// each edge of a convex hull so per-caster overlap tests avoid `sqrt` and division [design].
fn outline_edges(outline: &[Vec2]) -> Vec<(Vec2, Vec2, f32)> {
    (0..outline.len())
        .map(|i| {
            let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
            let n = (-(b - a).perp()).normalize_or_zero();
            (n, n.abs(), n.dot(a))
        })
        .collect()
}

fn overlaps_edges(edges: &[(Vec2, Vec2, f32)], centre: Vec2, reach: Vec2) -> bool {
    edges.iter().all(|&(n, n_abs, limit)| n.dot(centre) - n_abs.dot(reach) <= limit)
}

#[cfg(test)]
#[path = "sky_slice_culling_tests.rs"]
mod sky_slice_culling_tests;

/// The convex hull of `points`, counter-clockwise (monotone chain).
fn hull<const N: usize>(mut points: [Vec2; N]) -> Vec<Vec2> {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    let mut out: Vec<Vec2> = Vec::with_capacity(2 * N);
    for pass in 0..2 {
        let start = out.len();
        for i in 0..N {
            let p = if pass == 0 { points[i] } else { points[N - 1 - i] };
            while out.len() >= start + 2 && (out[out.len() - 1] - out[out.len() - 2]).perp_dot(p - out[out.len() - 2]) <= 0.0 {
                out.pop();
            }
            out.push(p);
        }
        out.pop();
    }
    out
}

/// Whether the rectangle `centre` ± `reach` overlaps the convex polygon `outline`
/// (counter-clockwise): separated only along one of the outline's edges (the rectangle's
/// own axes are the cascade's square, which Bevy has tested already).
#[cfg_attr(not(test), allow(dead_code))]
fn overlaps(outline: &[Vec2], centre: Vec2, reach: Vec2) -> bool {
    overlaps_edges(&outline_edges(outline), centre, reach)
}

/// Whether the sun casts shadows with its shadows switched `on` and a shadow distance of
/// `distance`: a distance of 0 (the Shadow distance row's 0%, the menu overhaul) casts none.
/// The one test for both [`set_shadows`] and graphics.rs `SunShadowKey::wanted`.
pub(crate) fn casts_sun_shadows(on: bool, distance: f32) -> bool {
    on && distance > 0.0
}

/// The sun's shadow cascades, in game units ([`shadow_cascade_layout`]), for shadow maps
/// `map` texels on a side.
pub(crate) fn shadow_cascades(distance: f32, reach: f32, layout: CascadeLayout, map: usize) -> CascadeShadowConfig {
    let (bounds, overlap_proportion) = shadow_cascade_layout(distance, reach, layout, map);
    CascadeShadowConfig { bounds, overlap_proportion, minimum_distance: SHADOW_MINIMUM_DISTANCE }
}

/// The side of the sun's shadow maps (texels): the resource's, or Bevy's default without
/// it (2048, graphics.rs `SHADOW_MAP` too).
pub(crate) fn map_side(map: Option<&bevy::light::DirectionalLightShadowMap>) -> usize {
    map.map_or_else(|| bevy::light::DirectionalLightShadowMap::default().size, |m| m.size)
}

/// Where the sun's cascades start (game units from the camera) and how far each overlaps
/// the one before it (a share of its far bound), as Bevy's builder was given them here
/// before; Quality with the larger maps blends over [`WIDE_SHADOW_OVERLAP`].
const SHADOW_MINIMUM_DISTANCE: f32 = 5.0;
const SHADOW_OVERLAP: f32 = 0.2;
/// The first cascade's far bound is no nearer than this (game units) at short shadow
/// distances, unless half the last bound is nearer still ([`shadow_cascade_layout`]).
const FIRST_CASCADE_FLOOR: f32 = 200.0;
/// The least step between two neighbouring far bounds, as a share of the room between the
/// first bound and the last ([`shadow_cascade_layout`]) [engineering choice: small enough
/// to leave every layout's shares alone at the usual distances; with at most 4
/// cascades the steps take at most 15 % of the room].
const MIN_CASCADE_STEP: f32 = 0.05;
/// The far bounds of all cascades but the last, as shares of the shadow distance
/// [engineering choice: the contributor's proposal in #2843, for the maintainer to decide.
/// The shadows were sharp only in a small radius around the player and coarse beyond,
/// worst on Performance, where everything past the first bound's 667 units lay on the last
/// cascade]. [observed 2026-11-16, the view and settings of [`CascadeLayout`]]
///
/// Performance's 3 cascades: at 8,000 units 1,500 and 4,500. With 2048² maps 3.09-3.11 ms
/// a frame against 3.05-3.10 for the 2 cascades of before (667, 12,000), and a telegraph
/// pole's crossarm past 667 units cast a clean shadow instead of a shapeless blob.
const SHADOW_SHARES_PERFORMANCE: [f32; 2] = [0.1875, 0.5625];
/// Quality's 4 cascades with maps smaller than [`WIDE_SHADOW_MAP`] (2048²): at 8,000 units
/// 700, 2,000, 5,000, 3.22 ms against 3.15-3.16 for the 3 of before. A farther first bound
/// costs sharpness at the player's feet with these maps (a wire's shadow crisp at 0.77
/// units a texel, soft at 1.15, blurred at 1.72), so their first bound stays near.
const SHADOW_SHARES_QUALITY: [f32; 3] = [0.0875, 0.25, 0.625];
/// Quality's 4 cascades with maps of [`WIDE_SHADOW_MAP`] or more (4096²): 0.1875, 0.4375
/// and 0.875 of the distance, the second and third capped at [`SHADOW_CAPS_QUALITY_WIDE`]
/// of the last bound, blended over [`WIDE_SHADOW_OVERLAP`]. With Longer shadows (reach
/// 1.5) at 8,000 units: 1,500, 3,500, 7,000, 12,000, the layout measured and pictured on
/// 2026-11-16: 3.43-3.48 ms against 3.38 for the 3 of before with these maps; the near
/// shadow stays crisp (0.86 units a texel) with the seams farther out.
const SHADOW_SHARES_QUALITY_WIDE: [f32; 3] = [0.1875, 0.4375, 0.875];
/// The caps of Quality's in-between bounds with the larger maps, as shares of the last
/// bound (none on the first) [engineering choice, #2843 review, for the maintainer to
/// decide: without Longer shadows (reach 1) the uncapped bounds end at 1,500, 3,500, 7,000
/// of 8,000, and with the 0.4 blend band the fourth cascade (4,200-8,000) would add almost
/// nothing beside the third (2,100-7,000) for a fourth draw]. At reach 1.5 they leave the
/// measured layout as it is (3,500 under 3,600, 7,000 under 7,200); at reach 1 they give
/// 1,500, 2,400, 4,800, 8,000, derived by this rule; its cost was not measured.
const SHADOW_CAPS_QUALITY_WIDE: [f32; 3] = [f32::INFINITY, 0.3, 0.6];
/// The shadow map side from which Quality takes [`SHADOW_SHARES_QUALITY_WIDE`].
const WIDE_SHADOW_MAP: usize = 4096;
/// The overlap (blend band) of Quality's cascades with the larger maps: seams farther out
/// are wider apart in texel size, and a wider band hides them [engineering choice, #2843].
const WIDE_SHADOW_OVERLAP: f32 = 0.4;

/// The far bounds of the sun's shadow cascades (game units from the camera, nearest
/// first) and their overlap, for `layout` and shadow maps `map` texels on a side. The last
/// bound is `distance` × `reach` (`reach` 1, or more with the longer shadows of
/// `graphics.rs`), so the Shadow distance slider and `--shadow-distance` move them all; the
/// others are not stretched by `reach`.
///
/// - Performance: the shares of `distance` in [`SHADOW_SHARES_PERFORMANCE`], whatever the
///   map size; overlap [`SHADOW_OVERLAP`].
/// - Quality: [`SHADOW_SHARES_QUALITY`] and [`SHADOW_OVERLAP`] with maps under
///   [`WIDE_SHADOW_MAP`]; from it [`SHADOW_SHARES_QUALITY_WIDE`], each in-between bound
///   capped at its [`SHADOW_CAPS_QUALITY_WIDE`] share of the last, and
///   [`WIDE_SHADOW_OVERLAP`].
/// - Spaced(n) (`--graphics-cycle cascades=<n>`): Bevy's geometric spacing from a first
///   bound of a twelfth of `distance`, as every count had it before #2843, so that
///   measurements of then still compare; overlap [`SHADOW_OVERLAP`].
/// - One cascade (the moon's, Spaced(1)): `distance` × `reach`.
///
/// At short distances (the Shadow distance row's least step above 0 % is a fifth of
/// `--shadow-distance`, and `--shadow-distance` itself may be short; at 0 the sun casts
/// none, [`casts_sun_shadows`], though the layout is still well formed there) the first bound stays at [`FIRST_CASCADE_FLOOR`] but under half the
/// last, and the last at four times the minimum distance at least. The floor on the first
/// bound (or a cap) can then push a share to or past its neighbour, so each in-between
/// bound `i` of `n` is its (capped) share held between `first + i × step` and
/// `last − (n − 1 − i) × step`, a step being [`MIN_CASCADE_STEP`] of the room between the
/// first bound and the last. Both limits rise by a step from one bound to the next and the
/// capped shares rise strictly (with a distance above 0; at 0 they are all 0 and the lower
/// limits decide), so the bounds always rise strictly, the first and the last included.
/// The limits only act when a share comes within a step of them: at 8,000 units, with or
/// without Longer shadows, every layout keeps its shares exactly (Quality with 4096² maps
/// at reach 1, for one, has its second bound 2,400 against a lower limit of 1,825 and its
/// third 4,800 against 2,150) [engineering choice].
pub(crate) fn shadow_cascade_layout(distance: f32, reach: f32, layout: CascadeLayout, map: usize) -> (Vec<f32>, f32) {
    let count = layout.count();
    let last = (distance * reach.max(1.0)).max(4.0 * SHADOW_MINIMUM_DISTANCE);
    let (shares, caps, overlap): (&[f32], &[f32], f32) = match layout {
        CascadeLayout::Performance => (&SHADOW_SHARES_PERFORMANCE[..], &[][..], SHADOW_OVERLAP),
        CascadeLayout::Quality if map >= WIDE_SHADOW_MAP => (&SHADOW_SHARES_QUALITY_WIDE[..], &SHADOW_CAPS_QUALITY_WIDE[..], WIDE_SHADOW_OVERLAP),
        CascadeLayout::Quality => (&SHADOW_SHARES_QUALITY[..], &[][..], SHADOW_OVERLAP),
        CascadeLayout::Spaced(_) => (&[][..], &[][..], SHADOW_OVERLAP),
    };
    if count == 1 {
        return (vec![last], overlap);
    }
    let first_share = shares.first().copied().unwrap_or(1.0 / 12.0);
    let first = (distance * first_share).max(FIRST_CASCADE_FLOOR).min(last / 2.0);
    if shares.is_empty() {
        let bounds = CascadeShadowConfigBuilder {
            num_cascades: count,
            minimum_distance: SHADOW_MINIMUM_DISTANCE,
            first_cascade_far_bound: first,
            maximum_distance: last,
            overlap_proportion: overlap,
        }
        .build()
        .bounds;
        return (bounds, overlap);
    }
    let step = MIN_CASCADE_STEP * (last - first);
    let bounds = std::iter::once(first)
        .chain(shares.iter().enumerate().skip(1).map(|(i, share)| {
            let cap = caps.get(i).map_or(f32::INFINITY, |c| c * last);
            let (low, high) = (first + i as f32 * step, last - (count - 1 - i) as f32 * step);
            (distance * share).min(cap).min(high).max(low)
        }))
        .chain(std::iter::once(last))
        .collect();
    (bounds, overlap)
}

/// Log the sun's cascades whenever their far bounds or the shadow maps' size change (the
/// first frame with cascades, the day's and the moon's, a change of mode, distance or
/// Shadow resolution): `sun shadows: cascades far bounds [..] texels [..] units`, with each
/// cascade's texel size as Bevy built it for the main camera. Runs after Bevy builds the
/// cascades and before the hold can give back a cascade of an earlier frame
/// (shadow_anchor.rs `cascade_edits`); a few floats compared a frame.
pub fn log_sun_cascades(
    suns: Query<(&DirectionalLight, &CascadeShadowConfig, &bevy::light::Cascades), With<Sun>>,
    cams: Query<Entity, With<FlyCam>>,
    map: Option<Res<bevy::light::DirectionalLightShadowMap>>,
    mut said: Local<(Vec<f32>, usize)>,
) {
    let Some(cam) = cams.iter().next() else { return };
    let side = map_side(map.as_deref());
    for (light, config, cascades) in &suns {
        if !light.shadow_maps_enabled || (config.bounds == said.0 && side == said.1) {
            continue;
        }
        // Wait for the cascades Bevy builds from this config.
        let Some(list) = cascades.cascades.get(&cam).filter(|l| l.len() == config.bounds.len()) else { continue };
        let bounds: Vec<String> = config.bounds.iter().map(|b| format!("{b:.0}")).collect();
        let texels: Vec<String> = list.iter().map(|c| format!("{:.3}", c.texel_size)).collect();
        info!("sun shadows: cascades far bounds [{}] texels [{}] units", bounds.join(", "), texels.join(", "));
        said.0.clone_from(&config.bounds);
        said.1 = side;
    }
}

/// Height of the sun's direction above the horizon (the sine of its elevation; below 0
/// the directional light is the moon, from the opposite side) and the share of its
/// strength the light has at that height (it fades out within [`HORIZON_FADE`]; [`update_sky`]).
pub fn sun_height_and_fade(sky: &SkyState) -> (f32, f32) {
    let y = (game_to_bevy().rotation * Vec3::from_array(sun_direction(&sky.climate, sky.hour, SUN_SOUTH))).y;
    (y, (y.abs() / HORIZON_FADE.sin()).min(1.0))
}

/// Switch the sun's shadows on or off now (the settings page); they reach
/// [`SkyState::shadow_distance`].
pub fn set_shadows(world: &mut World, on: bool) {
    if let Some(mut work) = world.get_resource_mut::<crate::quality_probe::Work>() {
        work.shadow_updates += 1;
    }
    let distance = {
        let mut sky = world.resource_mut::<SkyState>();
        sky.shadows = on;
        (sky.shadow_distance, sky.shadow_reach, sky.shadow_layout)
    };
    let map = map_side(world.get_resource::<bevy::light::DirectionalLightShadowMap>());
    // A shadow distance of 0% (the menu overhaul) casts none.
    let on = casts_sun_shadows(on, distance.0);
    let mut suns = world.query_filtered::<(Entity, &mut DirectionalLight), With<Sun>>();
    let lit: Vec<Entity> = suns
        .iter_mut(world)
        .map(|(e, mut light)| {
            light.shadow_maps_enabled = on;
            e
        })
        .collect();
    if on {
        for e in lit {
            world.entity_mut(e).insert(shadow_cascades(distance.0, distance.1, distance.2, map));
        }
    }
}

/// Cloud layers follow the camera and drift with their ONAM speeds; the current set shows
/// the current weather's textures and colours at the fade's alpha, the other set the
/// previous weather's at the remaining alpha.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_clouds(
    sky: Res<SkyState>,
    time: Res<Time>,
    data: Res<GameData>,
    mut textures: ResMut<TextureCache>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<SkyMaterial>>,
    cam: Query<&Transform, (With<FlyCam>, Without<CloudLayer>)>,
    mut clouds: Query<(&mut CloudLayer, &MeshMaterial3d<SkyMaterial>, &mut Transform, &mut Visibility), Without<FlyCam>>,
    mut logged: Local<Option<u32>>,
) {
    let Ok(cam_t) = cam.single() else { return };
    // Elapsed seconds, or the frozen phase of NVF_PLAY_DEBUG_CLOUD_PHASE (cloud_shadows.rs).
    let secs = crate::cloud_shadows::cloud_seconds(&time);
    // Set 0 shows the current weather while no fade runs; during a fade the set whose
    // texture already shows the new weather keeps it.
    for (mut cl, m, mut t, mut vis) in &mut clouds {
        t.translation = cam_t.translation;
        let (w, alpha) = match (&sky.prev, cl.set) {
            _ if sky.interior.is_some() => (None, 0.0),
            (None, 0) => (Some(&sky.weather), 1.0),
            (None, _) => (None, 0.0),
            (Some(_), 0) => (Some(&sky.weather), sky.fade),
            (Some(p), _) => (Some(p), 1.0 - sky.fade),
        };
        let Some(w) = w else {
            *vis = Visibility::Hidden;
            continue;
        };
        let new_texture = cl.shows != Some(w.form_id);
        let c = sky.cloud_color_for(w, cl.layer);
        let color = LinearRgba::new(c[0], c[1], c[2], alpha);
        // Each set scrolls at its own weather's ONAM speeds (modulated when
        // `CloudUpgrade.ini` is present), so a fade blends the two weathers' layers each
        // moving at its own rate (fnv_world::sky::Weather::cloud_scroll).
        let offset = sky.cloud_offset(w, cl.layer, secs);
        // The material is written only when it shows differently: a new texture, a colour
        // moved by more than a thousandth, or a scroll of more than 1/2048 of the texture
        // (under a texel; the scroll is a few thousandths of a repeat a second). Written
        // every frame, each cloud layer's material was made again on the GPU every frame,
        // GPU memory made and freed behind the night stalls (#897).
        let Some(was) = materials.get(&m.0) else { continue };
        let old = was.base.base_color.to_linear();
        let moved = (was.base.uv_transform.translation.x - offset).rem_euclid(1.0);
        let unchanged = !new_texture
            && [old.red - color.red, old.green - color.green, old.blue - color.blue, old.alpha - color.alpha].iter().all(|d| d.abs() <= 1e-3)
            && moved.min(1.0 - moved) <= 1.0 / 2048.0;
        let mut textured = was.base.base_color_texture.is_some();
        if !unchanged {
            let Some(mut mat) = materials.get_mut(&m.0) else { continue };
            if new_texture {
                mat.base.base_color_texture = w.cloud_textures[cl.layer].as_deref().and_then(|p| {
                    // A texture the base game's own weather names and no archive has is
                    // no fault to report (#2408): the layer stays without a texture.
                    let absent = official_plugin(w.plugin.as_deref()) && !data.vfs.exists(&nvf_assets::texture::resolved_path(p, |q| data.vfs.exists(q)));
                    if absent {
                        debug!("clouds: layer {} of {}: {p} is in no archive (named by {}): no texture", cl.layer, w.edid.as_deref().unwrap_or("?"), w.plugin.as_deref().unwrap_or("?"));
                        return None;
                    }
                    textures.get(p, true, &data, &mut images, None)
                });
                cl.shows = Some(w.form_id);
            }
            mat.base.base_color = Color::LinearRgba(color);
            mat.base.uv_transform = Affine2::from_translation(Vec2::new(offset, 0.0));
            textured = mat.base.base_color_texture.is_some();
        }
        *vis = if textured && alpha > 0.0 { Visibility::Inherited } else { Visibility::Hidden };
        if cl.set == 0 && cl.layer == 0 && *logged != Some(w.form_id) {
            *logged = Some(w.form_id);
            let rates = [0, 1, 2, 3].map(|i| format!("{:.5}", sky.cloud_scroll(w, i)));
            info!(
                "clouds: {} layer speeds {:?} (ONAM), scrolling {} texture repeats/s",
                w.edid.as_deref().unwrap_or("?"),
                w.cloud_speeds,
                rates.join(", ")
            );
        }
    }
}

/// Stars and moon follow the camera; their visibility, colour and the moon's phase and
/// position follow the hour.
#[allow(clippy::type_complexity)]
pub fn update_night_sky(
    sky: Res<SkyState>,
    cam: Query<&Transform, (With<FlyCam>, Without<Stars>, Without<MoonDisc>)>,
    mut stars: Query<(&mut Transform, &MeshMaterial3d<SkyMaterial>, &mut Visibility), (With<Stars>, Without<FlyCam>, Without<MoonDisc>)>,
    mut moon: Query<(&mut Transform, &MeshMaterial3d<SkyMaterial>, &mut MoonDisc), (Without<FlyCam>, Without<Stars>)>,
    mut materials: ResMut<Assets<SkyMaterial>>,
    mut last: Local<Option<(f32, f32)>>,
) {
    let Ok(cam_t) = cam.single() else { return };
    let cam_pos = cam_t.translation;
    let indoors = sky.interior.is_some();
    let sun_dir = game_to_bevy().rotation * Vec3::from_array(sun_direction(&sky.climate, sky.hour, SUN_SOUTH));
    let (star_vis, moon_vis) = if indoors { (0.0, 0.0) } else { sky.night_visibility() };
    let refresh = *last != Some((sky.hour, sky.days_passed));
    *last = Some((sky.hour, sky.days_passed));
    for (mut t, m, mut vis) in &mut stars {
        t.translation = cam_pos;
        *vis = if star_vis > 0.0 { Visibility::Inherited } else { Visibility::Hidden };
        if refresh {
            let c = sky.color(ColorType::Stars);
            let v = star_vis * STAR_STRENGTH;
            let color = Color::LinearRgba(LinearRgba::new(c[0] * v, c[1] * v, c[2] * v, 1.0));
            // Read first: taking the material mutably marks it changed, and by day (the
            // stars at 0) every half game minute wrote the same black again.
            if materials.get(&m.0).is_some_and(|mat| mat.base.base_color != color) {
                if let Some(mut mat) = materials.get_mut(&m.0) {
                    mat.base.base_color = color;
                }
            }
        }
    }
    for (mut t, m, mut disc) in &mut moon {
        let moon_dir = -sun_dir;
        if indoors || moon_vis <= 0.0 || moon_dir.y < MOON_HIDDEN_Y {
            t.scale = Vec3::ZERO;
            continue;
        }
        t.scale = Vec3::ONE;
        t.translation = cam_pos + moon_dir * SUN_DISTANCE;
        t.look_at(cam_pos, Vec3::Y);
        let phase = sky.moon_phase();
        if refresh || disc.shown != Some(phase) {
            let color = Color::LinearRgba(LinearRgba::new(1.0, 1.0, 1.0, moon_vis));
            // Read first, as the stars.
            if materials.get(&m.0).is_some_and(|mat| mat.base.base_color_texture != disc.phases[phase] || mat.base.base_color != color) {
                if let Some(mut mat) = materials.get_mut(&m.0) {
                    mat.base.base_color_texture = disc.phases[phase].clone();
                    mat.base.base_color = color;
                }
            }
            disc.shown = Some(phase);
        }
    }
}

/// The dome mesh last coloured and the sky colours it was coloured with.
type DomeShown = (AssetId<Mesh>, [f32; 9]);

/// Whether the dome mesh `mesh` needs colouring for the sky colours `now`: a new dome (a
/// worldspace change spawns one whose vertex colours are still the atmosphere's weights,
/// drawn as a green-to-red sky, #1395) or colours that moved noticeably since `shown`.
fn dome_needs_colour(shown: Option<DomeShown>, mesh: AssetId<Mesh>, now: [f32; 9]) -> bool {
    shown.is_none_or(|(was_mesh, was)| was_mesh != mesh || was.iter().zip(now).any(|(a, b)| (a - b).abs() > 1e-3))
}

/// Colour of an atmosphere vertex whose colour is `w`, linear RGB: horizon × red plus
/// sky-lower × green plus sky-upper × blue, mixed as display (sRGB) values [observed
/// 2026-11-16, base game, FalloutNV.esm's WastelandClear at 09:09 (upper (40, 46, 255),
/// lower (123, 156, 242), horizon (162, 194, 204)), looking away from the sun: with the
/// grade taken out the original's red and green at 46°, 31° and 25° up are (52, 59),
/// (74, 93) and (84, 105); green as sky-lower gives (53, 65), (76, 93) and (81, 101),
/// green as the horizon colour (91, 108) at 31°. world.md §6 "Dome weights"].
pub(crate) fn dome_color(w: [f32; 3], lower: [f32; 3], horizon: [f32; 3], upper: [f32; 3]) -> [f32; 3] {
    use crate::lodgen::{linear_to_srgb as d, srgb_to_linear as l};
    std::array::from_fn(|i| l(w[0] * d(horizon[i]) + w[1] * d(lower[i]) + w[2] * d(upper[i])))
}

fn to_color(c: [f32; 3]) -> Color {
    Color::LinearRgba(LinearRgba::rgb(c[0], c[1], c[2]))
}

/// The explorer's `[` and `]`: the hour back or on (a quarter with left Shift). Not run
/// with `--play`: there they are the radio's stations, and the hour moved here became the
/// game's hour, schedules and timers shifted with it [#1988].
pub fn sky_keys(input: nvf_play::pad::ActionButtons<KeyCode>, mut sky: ResMut<SkyState>) {
    let input = input.snapshot();
    let step = if input.pressed(KeyCode::ShiftLeft) { 0.25 } else { 1.0 };
    if input.just_pressed(KeyCode::BracketRight) {
        let h = sky.hour + step;
        sky.set_hour(h, true);
    }
    if input.just_pressed(KeyCode::BracketLeft) {
        let h = sky.hour - step;
        sky.set_hour(h, true);
    }
}

/// The camera's far plane outdoors (and indoors without a clip distance), game units.
pub const OUTDOOR_FAR: f32 = 400_000.0;

/// Indoors the camera sees no farther than the cell's Fog Clip Distance (CELL XCLL, or
/// its lighting template's) [documented: GECK wiki SetClipDist, "overriding far clip
/// distance in interior cells"; GetInteriorLightingTraitNumeric "Fog clip distance";
/// docs/spec-notes/world.md §6 "Interior clip distance"]; 0 is no clip [hypothesis: 43
/// interiors store 0]. Outdoors, [`OUTDOOR_FAR`].
pub fn interior_clip(sky: Res<SkyState>, mut cam: Query<&mut Projection, With<FlyCam>>, mut last: Local<Option<f32>>) {
    let far = sky.interior.as_ref().map(|l| l.fog_clip_dist).filter(|&c| c > 0.0).unwrap_or(OUTDOOR_FAR);
    if *last == Some(far) {
        return;
    }
    for mut p in &mut cam {
        if let Projection::Perspective(pp) = &mut *p {
            pp.far = far;
            *last = Some(far);
            info!("sky: the camera's far plane at {far:.0} units{}", if sky.interior.is_some() && far < OUTDOOR_FAR { " (the cell's fog clip distance)" } else { "" });
        }
    }
}

/// Interior fog's far distance with the No Interior Fog tweak: farther than any room
/// [design: docs/design/tweaks.md].
const NO_FOG_FAR: f32 = 1_000_000.0;

/// The sky's colours now, for GetCurrentSkyColor (`fnv_game::nvse_jg9`); populated in
/// interiors as well as exteriors [observed: `w6_t1_09_jg_sky_color_interior`,
/// `docs/spec-notes/ext-jg9.md` §6].
pub fn tell_sky_colors(sky: Res<SkyState>, mut script: Option<ResMut<nvf_play::script::ScriptState>>) {
    let Some(rt) = script.as_deref_mut().and_then(|s| s.rt.as_mut()) else { return };
    rt.game.other.jg9.sky_colors = Some(sky.all_color_bytes());
}

/// Whether an interior cell's fog range is active.
///
/// Cells whose effective fog range has `fog_far <= fog_near.max(0.0)` (most commonly
/// `0..0` with black fog colour in interiors without a lighting template) have no active
/// distance fog, matching how exterior weathers treat unset fog ranges
/// (`fnv_world::sky::Weather::fog_at`).
pub(crate) fn interior_fog_active(l: fnv_world::lighting::CellLighting) -> bool {
    l.fog_near.is_finite() && l.fog_far.is_finite() && l.fog_far > l.fog_near.max(0.0)
}

/// Compose the current cell/weather fog for the main camera. Underwater fog uses
/// the same composition on exit, without waiting for the next weather refresh.
pub(crate) fn apply_cell_fog(sky: &SkyState, fog: &mut DistanceFog, emissive_mod: f32, no_interior_fog: bool) {
    if let Some(l) = sky.interior {
        let color = to_color(l.fog_color.map(fnv_world::sky::srgb_to_linear));
        if interior_fog_active(l) {
            fog.color = color;
            let far = if no_interior_fog { NO_FOG_FAR } else { l.fog_far };
            fog.falloff = crate::fog::falloff(l.fog_near.max(0.0), far, l.fog_power, false, 1.0);
        } else {
            // Unset interior fog range (e.g. 0..0): disable fog via alpha 0, matching
            // exterior weathers (`Weather::fog_at`), instead of clamping far to near + 1.
            fog.color = color.with_alpha(0.0);
            fog.falloff = crate::fog::falloff(0.0, NO_FOG_FAR, l.fog_power, false, 1.0);
        }
    } else {
        let range = sky.fog_override.map(|(n, f)| (n, f, 1.0)).or_else(|| sky.fog());
        let (near, far, strength) = range.unwrap_or((0.0, 0.0, 0.0));
        // Under Physical lighting the game's fog fades far land into the physical dome's
        // horizon colour, the sky behind it (graphics-enhanced.md §3.25).
        let fog_rgb = if nvf_assets::authored::physical_pbr() {
            sky.physical_horizon_color()
        } else {
            sky.color(ColorType::Fog)
        };
        fog.color = to_color(fog_rgb).with_alpha(strength);
        fog.falloff = crate::fog::falloff(near.max(0.0), far, sky.fog_power(), true, sky.emissive_scale() * emissive_mod);
    }
}

/// Before the sky follows the game's clock (main.rs): a weather fade moves between the sky's
/// half-minute steps only while the volumetric clouds are on and drawn. `CloudVolumeShown` is last
/// frame's, so the row itself is asked too: switched off, not one more reading moves the fade.
pub fn fade_cadence(mut sky: ResMut<SkyState>, graphics: Option<Res<crate::graphics::Graphics>>, shown: Option<Res<crate::volumetric_clouds::CloudVolumeShown>>) {
    let smooth = graphics.is_some_and(|g| g.volumetric_clouds) && shown.is_some_and(|v| v.0);
    if sky.smooth_fade != smooth {
        sky.smooth_fade = smooth;
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_sky(
    mut sky: ResMut<SkyState>,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut clear: ResMut<ClearColor>,
    mut ambient: ResMut<GlobalAmbientLight>,
    cam: Query<(&Transform, &mut DistanceFog), (With<FlyCam>, Without<SkyDome>, Without<Sun>, Without<CloudLayer>)>,
    mut dome: Query<(&Mesh3d, &SkyDome, &mut Transform), (Without<FlyCam>, Without<Sun>, Without<CloudLayer>)>,
    mut sun: Query<(&mut DirectionalLight, &mut Transform), (With<Sun>, Without<FlyCam>, Without<SkyDome>, Without<CloudLayer>)>,
    mut materials: ResMut<Assets<SkyMaterial>>,
    mut flashed: Local<bool>,
    mut dome_shown: Local<Option<DomeShown>>,
    mut last_interior_fog: Local<Option<(f32, f32, f32)>>,
    screen: Option<Res<crate::imagespace::ScreenModifiers>>,
    tweaks: Option<Res<nvf_play::tweaks::TweakSettings>>,
    volume: (Option<Res<crate::volumetric_clouds::CloudVolumeShown>>, Option<Res<crate::cloud_shadows::CloudCoverPublished>>),
    mut last_fair_sun: Local<[f32; 2]>,
) {
    let mut cam = cam;
    // What the image space modifiers on the screen do to the Sunlight Dimmer and the
    // Emissive Mult (image-modifiers.md §11), on top of the image space's.
    let ratio = |i: usize| screen.as_deref().map_or(1.0, |s| s.hdr_ratio(i));
    let (sun_mod, emissive_mod) = (ratio(fnv_world::imagespace::HDR_SUNLIGHT_DIMMER), ratio(fnv_world::imagespace::HDR_EMISSIVE_MULT));
    let Ok((cam_t, mut fog)) = cam.single_mut() else { return };
    let cam_pos = cam_t.translation;
    let to_bevy = game_to_bevy();
    let unit = light_unit();
    if sky.time_scale > 0.0 && sky.clock_running && sky.interior.is_none() {
        sky.clock_seen = true;
        let dt = time.delta_secs() * sky.time_scale;
        let h = sky.hour + dt;
        if h >= 24.0 {
            sky.days_passed += (h / 24.0).floor();
        }
        sky.advance_fade(dt);
        sky.set_hour(h, false);
    }
    let changed = std::mem::take(&mut sky.changed);
    if let Some(l) = sky.interior {
        if !changed {
            return;
        }
        let c = |v: [u8; 3]| to_color(v.map(fnv_world::sky::srgb_to_linear));
        if let Ok((_, _, mut t)) = dome.single_mut() {
            t.scale = Vec3::ZERO;
        }
        clear.0 = c(l.fog_color);
        let shown = |v: [u8; 3]| v.map(|b| b as f32 / 255.0);
        ambient.color = crate::lights::display_ambient(shown(l.ambient));
        ambient.brightness = unit * INTERIOR_AMBIENT_SHARE;
        if let Ok((mut light, mut t)) = sun.single_mut() {
            let from = game_to_bevy().rotation * interior_light_from(l.directional_rotation_xy, l.directional_rotation_z);
            *t = Transform::IDENTITY.looking_to(-from, Vec3::Y);
            let physical = nvf_assets::authored::physical_pbr();
            light.color = if physical {
                to_color(l.directional.map(fnv_world::sky::srgb_to_linear))
            } else {
                crate::lights::display_color(shown(l.directional))
            };
            let gain = if physical { PHYSICAL_INTERIOR_DIR_GAIN } else { 1.0 };
            light.illuminance = PI * unit * l.directional_fade.clamp(0.0, 1.0) * INTERIOR_DIRECTIONAL_SHARE * gain;
        }
        apply_cell_fog(&sky, &mut fog, emissive_mod, tweaks.as_ref().is_some_and(|t| t.no_interior_fog));
        let fog_key = (l.fog_near, l.fog_far, l.fog_power);
        if *last_interior_fog != Some(fog_key) {
            *last_interior_fog = Some(fog_key);
            if interior_fog_active(l) {
                info!("interior lighting: fog {:.0}..{:.0} power {:.2}", l.fog_near, l.fog_far, l.fog_power);
            } else {
                info!("interior lighting: fog none (stored {:.0}..{:.0} power {:.2})", l.fog_near, l.fog_far, l.fog_power);
            }
        }
        return;
    }
    *last_interior_fog = None;
    let c = &sky.climate;

    // Follow the camera every frame; clouds drift with their ONAM speeds. The dome is
    // shrunk to nothing in interiors: give it its size back (the sun disc and glare are
    // drawn on it, so after the first interior, e.g. a new game's Doc Mitchell's house,
    // neither showed again).
    if let Ok((_, _, mut t)) = dome.single_mut() {
        t.translation = cam_pos;
        if t.scale == Vec3::ZERO {
            info!("sky: outdoors again, the sky dome (with the sun disc and glare) shown");
        }
        t.scale = sky_transform(cam_pos).scale;
    }
    // The sun light turns with the game clock every frame, not with the sky's half-minute
    // steps (`SkyState::sun_light_hour`); its colour still changes with the sky below.
    let light_dir = to_bevy.rotation * Vec3::from_array(sun_direction(c, sky.sun_light_hour(), SUN_SOUTH));
    let moon_mult = sky.moon_brightness_multiplier();
    if let Ok((mut light, mut t)) = sun.single_mut() {
        aim_sun(&mut light, &mut t, light_dir, to_bevy.rotation * Vec3::Y, PI * unit * sky.sun_scale() * sun_mod, moon_mult);
    }
    // Use the same authored cover and weather bias as the fair cumulus, before the
    // renderer splits low and high layers. A volume handover can occur at a frozen hour.
    // The disc's weight is the same without the sun's elevation: it stays to the horizon.
    let [fair_sun, fair_disc] = if volume.0.as_ref().is_some_and(|v| v.0) {
        volume.1.as_ref().and_then(|p| p.0.as_ref())
            .and_then(|c| crate::volumetric_clouds::blend_covers(c.contributors(&sky), &c.weathers))
            .map_or([0.0; 2], |c| {
                let bias = crate::volumetric_clouds::sky_bias(sky.weather.classification, sky.prev.as_ref().map(|w| w.classification), sky.fade);
                [sky.sun_elevation(), 90.0].map(|e| crate::volumetric_clouds::fair_daylight_weight(c.amount, bias, e))
            })
    } else {
        [0.0; 2]
    };
    // A weight that reached 0 is written whatever it was: without the fair volume sky the sun is
    // exactly the plain sky's.
    let ended = [(fair_sun, last_fair_sun[0]), (fair_disc, last_fair_sun[1])].iter().any(|&(now, was)| now == 0.0 && was != 0.0);
    if !changed && !ended && (fair_sun - last_fair_sun[0]).abs() < 1e-4 && (fair_disc - last_fair_sun[1]).abs() < 1e-4 {
        return;
    }
    *last_fair_sun = [fair_sun, fair_disc];

    // Daylight on every sky part that takes it (see SkyLight).
    let mut light = sky.light(fair_sun, fair_disc);
    let flash = Vec3::from_array(sky.flash);
    light.glow += flash.extend(0.0);
    // Written only when the light moved noticeably: each write remakes the material's
    // uniform buffer on the GPU, and writing every frame (the sun creeps a little each
    // frame) made and freed GPU memory every frame, part of the night stalls (#897). A
    // thousandth is far below what shows; a lightning flash passes it at once.
    let ids: Vec<_> = materials.ids().collect();
    for id in ids {
        let Some(m) = materials.get(id) else { continue };
        let old = m.extension.light;
        // The night sky detail's fields are night_sky.rs's own: kept as they are.
        let new = SkyLight { params: light.params.with_y(old.params.y).with_z(old.params.z).with_w(old.params.w), moon: old.moon, night: old.night, ..light };
        let same = |a: Vec4, b: Vec4| if ended { a == b } else { a.abs_diff_eq(b, 1e-3) };
        if same(old.params, new.params) && same(old.glow, new.glow) && same(old.sun_dir, new.sun_dir) && same(old.sun, new.sun) && same(old.glare, new.glare) && same(old.disc, new.disc) {
            continue;
        }
        if let Some(mut m) = materials.get_mut(id) {
            m.extension.light = new;
        }
    }
    let upper = sky.color(ColorType::SkyUpper);
    let lower = sky.color(ColorType::SkyLower);
    let horizon = sky.color(ColorType::Horizon);
    // The dome is coloured again only when its colours moved noticeably (as the sky
    // materials above): a changed mesh is sent to the GPU again, every frame before (#897).
    let shown = [upper[0], upper[1], upper[2], lower[0], lower[1], lower[2], horizon[0], horizon[1], horizon[2]];
    if let Some((m, weights, _)) = dome.single().ok().filter(|(m, _, _)| dome_needs_colour(*dome_shown, m.0.id(), shown)) {
        if dome_shown.is_none_or(|(was, _)| was != m.0.id()) {
            info!("sky: a new dome coloured from {}'s sky colours", sky.weather.edid.as_deref().unwrap_or("the weather"));
        }
        *dome_shown = Some((m.0.id(), shown));
        if let Some(mut mesh) = meshes.get_mut(&m.0) {
            let cols: Vec<[f32; 4]> = weights
                .0
                .iter()
                .map(|wt| {
                    let c = dome_color(*wt, lower, horizon, upper);
                    [c[0], c[1], c[2], 1.0]
                })
                .collect();
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, cols);
        }
    }
    let physical = nvf_assets::authored::physical_pbr();
    clear.0 = to_color(if physical { sky.physical_horizon_color() } else { lower });

    let sunlight = sky.color(ColorType::Sunlight);
    if let Ok((mut light, _)) = sun.single_mut() {
        light.color = if physical {
            to_color(sky.physical_sun_color())
        } else {
            crate::lights::display_color(sunlight.map(crate::lights::linear_to_display))
        };
    }
    let amb = sky.color(ColorType::Ambient);
    ambient.color = crate::lights::display_ambient([amb[0] + flash.x, amb[1] + flash.y, amb[2] + flash.z].map(crate::lights::linear_to_display));
    ambient.brightness = unit * AMBIENT_SHARE;

    // No fog: the fog colour's alpha scales the whole effect, so zero turns it off.
    let range = sky.fog_override.map(|(n, f)| (n, f, 1.0)).or_else(|| sky.fog());
    let (near, far, strength) = range.unwrap_or((0.0, 0.0, 0.0));
    let power = sky.fog_power();
    apply_cell_fog(&sky, &mut fog, emissive_mod, false);
    // A lightning flash changes the light each frame it lasts: not worth a line each.
    let flashing = std::mem::replace(&mut *flashed, flash != Vec3::ZERO);
    if sky.time_scale <= 0.0 && flash == Vec3::ZERO && !flashing {
        let desc = match range {
            Some(_) if strength < 1.0 => format!("{near:.0}..{far:.0} power {power:.2} x{strength:.2}"),
            Some(_) => format!("{near:.0}..{far:.0} power {power:.2} ({:.0} % at 1,000 units)", 100.0 * crate::fog::share(near.max(0.0), far, power, 1_000.0)),
            None => "none".into(),
        };
        let g = sky.cinematic();
        info!(
            "sky: {}  fog {desc}  sun x{:.2}  grade: saturation {:.2} contrast {:.2} around {:.2} brightness {:.2} tint ({:.2}, {:.2}, {:.2}) x{:.2}",
            sky.describe(),
            sky.sun_scale(),
            g.saturation,
            g.contrast,
            g.contrast_avg_lum,
            g.brightness,
            g.tint[0],
            g.tint[1],
            g.tint[2],
            g.tint_amount
        );
    }
}

pub fn initial_fog() -> DistanceFog {
    DistanceFog { color: Color::WHITE, falloff: FogFalloff::Linear { start: 8_000.0, end: 20_000.0 }, ..default() }
}

/// Aim the sun light along `sun_dir` (towards the sun, Bevy axes) and set its strength
/// from `full` (the light at full strength before the sun/moon share).
///
/// `axis` is the line the sun's path turns about (`fnv_world::sky::sun_direction` keeps
/// the game's north axis fixed). Taken as the light's up, the shadow maps turn rigidly
/// about it as the sun moves, instead of also rolling as they did with the world's up:
/// less crawl along the shadows' edges while the sun light follows the clock every frame
/// (`SkyState::sun_light_hour`). [engineering choice]
pub(crate) fn aim_sun(light: &mut Mut<DirectionalLight>, t: &mut Mut<Transform>, sun_dir: Vec3, axis: Vec3, full: f32, moon_mult: f32) {
    // Below the horizon the light comes from the opposite side (moonlight colour is the
    // weather's night "sunlight") [hypothesis].
    let from = if sun_dir.y >= 0.0 { sun_dir } else { -sun_dir };
    t.set_if_neq(Transform::IDENTITY.looking_to(-from, axis));
    // Fade out while the sun (or moon) is within HORIZON_FADE of the horizon, so the
    // switch between the two does not flip shadows at full strength [interim]. `full` is
    // scaled by the image space's Sunlight Dimmer against the Mojave's (the Big MT's sun
    // is about twice it, Dead Money's West Town's under half) [hypothesis].
    // When `MoonlightNVSE.ini` is absent or disabled `moon_mult` is 1.0 and nighttime
    // illuminance stays bit-for-bit identical to vanilla.
    let share = if sun_dir.y >= 0.0 {
        SUN_SHARE * if nvf_assets::authored::physical_pbr() { PHYSICAL_SUN_GAIN } else { 1.0 }
    } else {
        let moon = MOON_SHARE * if nvf_assets::authored::physical_pbr() { PHYSICAL_MOON_GAIN } else { 1.0 };
        if moon_mult == 1.0 { moon } else { moon * moon_mult }
    };
    let illuminance = full * (sun_dir.y.abs() / HORIZON_FADE.sin()).min(1.0) * share;
    if light.illuminance != illuminance {
        light.illuminance = illuminance;
    }
}

/// The direction towards an interior's directional light, in game axes, from its XCLL
/// (or lighting template) rotation in degrees: `xy` an azimuth about z, `z` a pitch,
/// together the direction the light shines in (270 straight down) [hypothesis, world.md
/// §8 "Interior cells"; observer-checks 101].
pub fn interior_light_from(xy: i32, z: i32) -> Vec3 {
    let (az, el) = ((xy as f32).to_radians(), (z as f32).to_radians());
    -Vec3::new(az.sin() * el.cos(), az.cos() * el.cos(), el.sin())
}

/// Whether a record's plugin is the base game or one of its add-ons
/// (`crate::reports::OFFICIAL_PLUGINS`, by file name whatever its case). Their data is
/// the game's as shipped: FalloutNV.esm's NVWastelandClearNight names
/// `sky\NVWastelandStarfieldSky.dds` for its first cloud layer and no base archive has
/// it [observed: world.md, "Stars and moon"; `missing_asset` from 25 of 43 installs,
/// issue #2408], so a file such a record names and nobody has is not reported as missing.
fn official_plugin(name: Option<&str>) -> bool {
    name.is_some_and(|n| crate::reports::OFFICIAL_PLUGINS.iter().any(|o| o.eq_ignore_ascii_case(n)))
}

#[cfg(test)]
#[path = "sky_weather_percent_tests.rs"]
mod weather_percent_tests;

#[cfg(test)]
#[path = "sky_lighting_session_tests.rs"]
mod session_lighting_tests;

#[cfg(test)]
#[path = "cartographer_live_tests.rs"]
pub(crate) mod cartographer_live_tests;

#[cfg(test)]
pub(crate) fn authored_session_sky() -> SkyState {
    session_lighting_tests::sky()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #2408: the base game's and its add-ons' weathers may name a cloud texture
    /// no archive has without a warning; a mod's weather, or one of no known file, may not.
    #[test]
    fn the_fair_sun_grows_as_it_sinks_and_has_a_disc() {
        let d = FairSun::parse("");
        assert_eq!(d, FairSun::DEFAULT);
        assert_eq!(d.at(40.0), [FAIR_SUN_SIZE_DEG, FAIR_GLARE_SIZE_DEG, FAIR_GLARE_GAIN]);
        assert_eq!(d.at(FAIR_SUN_LOW_FROM), d.at(90.0));
        for e in [FAIR_SUN_LOW_TO, 3.0, -5.0] {
            assert_eq!(d.at(e), [SUN_SIZE_DEG, FAIR_GLARE_SIZE_DEG, FAIR_GLARE_GAIN]);
        }
        let mut last = d.at(-5.0)[0];
        for e in 0..60 {
            let size = d.at(e as f32)[0];
            assert!(size <= last && (FAIR_SUN_SIZE_DEG..=SUN_SIZE_DEG).contains(&size), "{e}");
            last = size;
        }
        // The disc is as wide as asked whatever the halo's size; none at no weight, so a sky
        // that is not the fair volume sky keeps the texture's own sun.
        for size in [FAIR_SUN_SIZE_DEG, 15.0, SUN_SIZE_DEG] {
            assert_eq!(d.disc_light(size, 0.0), Vec4::ZERO);
            let l = d.disc_light(size, 1.0);
            let drawn = 2.0 * (l.x * 2.0 * SUN_MIDDLE_UV * (size / 2.0).to_radians().tan()).atan().to_degrees();
            assert!((drawn - FAIR_DISC_DEG).abs() < 1e-4 && (l.z - FAIR_HALO).abs() < 1e-6, "{size} {l}");
            // What the shader draws is inside the texture, before its warp ends (sky.wesl).
            assert!(l.x > 1.0 && l.x * l.y * 1.2 < (l.x * l.y * 4.0).min(0.48), "{size} {l}");
        }
        let f = FairSun::parse("size=11,gain=1,lowsize=20,from=20,to=8,glare=NaN,junk,disc=0");
        assert_eq!(f.at(30.0), [11.0, FAIR_GLARE_SIZE_DEG, 1.0]);
        assert_eq!(f.at(8.0), [20.0, FAIR_GLARE_SIZE_DEG, 1.0]);
        let mid = f.at(14.0)[0];
        assert!((mid - 15.5).abs() < 1e-3, "{mid}");
        assert_eq!(f.disc_light(11.0, 1.0), Vec4::ZERO);
    }

    /// The disc's warp (sky.wesl `sun_texel`, the same numbers): the radius read rises with the
    /// radius drawn, so no part of the texture is drawn twice, and is the texture's own outside.
    #[test]
    fn the_sun_discs_warp_never_folds() {
        for size in [FAIR_SUN_SIZE_DEG, 15.0, SUN_SIZE_DEG] {
            let l = FairSun::DEFAULT.disc_light(size, 1.0);
            let edge = l.x * l.y;
            let read = |r: f32| r * (1.0 / l.x + (1.0 - 1.0 / l.x) * smoothstep(edge * 1.2, (edge * 4.0).min(0.48), r));
            let mut last = 0.0;
            for i in 1..=500 {
                let r = i as f32 * 0.001;
                assert!(read(r) > last, "{size} {r}");
                last = read(r);
            }
            assert!((read(edge) - l.y).abs() < 1e-5 && (read(0.5) - 0.5).abs() < 1e-5);
        }
        let wesl = include_str!("sky.wesl");
        assert!(wesl.contains("smoothstep(edge * 1.2, min(edge * 4.0, 0.48), r)"));
    }

    #[test]
    fn only_the_games_own_plugins_are_excused_a_missing_cloud_texture() {
        for name in ["FalloutNV.esm", "falloutnv.esm", "DeadMoney.esm", "GunRunnersArsenal.esm", "CaravanPack.esm"] {
            assert!(official_plugin(Some(name)), "{name}");
        }
        for name in [Some("MyWeatherMod.esp"), Some("FalloutNV.esm.bak"), Some("TaleOfTwoWastelands.esm"), None] {
            assert!(!official_plugin(name), "{name:?}");
        }
    }

    /// The common template pitch (z = 270) puts the light straight overhead, and the Gas
    /// Station's (xy 180, z 250, #1645) puts it high above, not below the floor.
    #[test]
    fn interior_light_comes_from_above() {
        let overhead = interior_light_from(0, 270);
        assert!((overhead - Vec3::Z).length() < 1e-5, "{overhead:?}");
        let gas_station = interior_light_from(180, 250);
        assert!(gas_station.z > 0.9, "{gas_station:?}");
        assert!((gas_station.length() - 1.0).abs() < 1e-5);
        // In Bevy's axes (y up) the light shines downwards.
        let from = game_to_bevy().rotation * gas_station;
        let shines = Transform::IDENTITY.looking_to(-from, Vec3::Y).forward();
        assert!(shines.y < -0.9, "{shines:?}");
    }

    /// The rolls made once a sky that started on `sky_day` follows a restored game on day
    /// 25, half a game minute on from the save's noon, with a saved weather its list holds
    /// and a volatility that rolls at every whole hour passed.
    fn rolls_following_the_restored_game(sky_day: f32) -> u64 {
        const SAVED: u32 = 1;
        let weather = |id| Weather { form_id: id, ..Default::default() };
        let choice = |id| Choice { weather: weather(id), chance: 100, global: None };
        let mut sky = SkyState {
            climate: Climate { volatility: 255, ..Default::default() },
            weather: weather(SAVED),
            hour: 12.0,
            fog_override: None,
            interior: None,
            interior_grade: None,
            shadows: false,
            shadow_distance: DEFAULT_SHADOW_DISTANCE,
            shadow_reach: 1.0,
            shadow_layout: CascadeLayout::Quality,
            grade: true,
            time_scale: 0.0,
            clock_running: false,
            days_passed: sky_day,
            keep_once: false,
            prev: None,
            fade: 1.0,
            choices: vec![choice(SAVED), choice(2)],
            image_space: None,
            world_image_space_id: None,
            weather_fog_overrides: HashMap::new(),
            weather_visual_overrides: WeatherVisualOverrides::default(),
            adapters: HashMap::new(),
            changed: true,
            script: None,
            pinned: false,
            regions: Vec::new(),
            source: None,
            placed: false,
            region_cell: None,
            clock_seen: false,
            sun_hour: None,
            smooth_fade: false,
            fade_given: 0.0,
            clock_ahead: 0.0,
            rolled_for: None,
            pick: SAVED,
            rolls: 0,
            rng: DEFAULT_WEATHER_SEED,
            hour_index: None,
            live_globals: HashMap::new(),
            resume: Some(SAVED),
            flash: [0.0; 3],
            script_wind: None,
            moon_textures: Default::default(),
            lightning_request: false,
            moonlight: None,
            cloud_upgrade: None,
        };
        sky.place(Some(None), false);
        assert_eq!((sky.rolls, sky.pick), (0, SAVED), "the first look keeps the saved weather");
        sky.follow_game_clock(12.0 + 1.0 / 60.0, 25.375);
        assert_eq!(sky.days_passed, 25.0);
        sky.rolls
    }

    #[test]
    fn a_sky_on_the_games_day_rolls_nothing_following_it() {
        assert_eq!(rolls_following_the_restored_game(25.0), 0);
        // On the plugins' day it counts the days between as hours passed (#1607).
        assert_eq!(rolls_following_the_restored_game(5.0), 1);
        // Ahead of the game: a step back, no roll.
        assert_eq!(rolls_following_the_restored_game(30.0), 0);
    }

    const HELD: u32 = 9;
    const LISTED: u32 = 1;
    const GATED: u32 = 2;
    const GATE: u32 = 77;

    /// A sky as the start makes it for a save showing `HELD`, which no list holds; the
    /// list's other entry waits for global `GATE`.
    fn sky_started_on_held() -> SkyState {
        let weather = |id| Weather { form_id: id, edid: Some(format!("W{id}")), ..Default::default() };
        let gate = Gate { id: GATE, edid: None, stored: Some(0.0) };
        SkyState {
            climate: Climate::default(),
            weather: weather(HELD),
            hour: 12.0,
            fog_override: None,
            interior: None,
            interior_grade: None,
            shadows: false,
            shadow_distance: DEFAULT_SHADOW_DISTANCE,
            shadow_reach: 1.0,
            shadow_layout: CascadeLayout::Quality,
            grade: true,
            time_scale: 0.0,
            clock_running: false,
            days_passed: 25.0,
            keep_once: false,
            prev: None,
            fade: 1.0,
            choices: vec![Choice { weather: weather(LISTED), chance: 100, global: None }, Choice { weather: weather(GATED), chance: 100, global: Some(gate) }],
            image_space: None,
            world_image_space_id: None,
            weather_fog_overrides: HashMap::new(),
            weather_visual_overrides: WeatherVisualOverrides::default(),
            adapters: HashMap::new(),
            changed: true,
            script: None,
            pinned: false,
            regions: Vec::new(),
            source: None,
            placed: false,
            region_cell: None,
            clock_seen: false,
            sun_hour: None,
            smooth_fade: false,
            fade_given: 0.0,
            clock_ahead: 0.0,
            rolled_for: None,
            pick: HELD,
            rolls: 0,
            rng: DEFAULT_WEATHER_SEED,
            hour_index: None,
            live_globals: HashMap::new(),
            resume: Some(HELD),
            flash: [0.0; 3],
            script_wind: None,
            moon_textures: Default::default(),
            lightning_request: false,
            moonlight: None,
            cloud_upgrade: None,
        }
    }

    // Synthetic region and weather fixtures [design, #2531; no game data].
    fn region_test_sky() -> (SkyState, Vec<RegionWeather>) {
        let mut sky = sky_started_on_held();
        sky.clock_seen = true;
        let region = RegionWeather {
            form_id: 3,
            edid: Some("TestRegion".into()),
            world: Some(11),
            areas: vec![vec![[100.0, 100.0], [200.0, 100.0], [200.0, 200.0], [100.0, 200.0]]],
            flags: fnv_world::sky::REGION_WEATHER_OVERRIDE,
            ..Default::default()
        };
        sky.regions = vec![RegionList {
            region: region.clone(),
            choices: vec![Choice { weather: Weather { form_id: 4, ..Default::default() }, chance: 100, global: None }],
        }];
        (sky, vec![region])
    }

    #[test]
    fn weather_region_holds_across_outline_until_cell_entry() {
        let (mut sky, regions) = region_test_sky();
        sky.place_outdoors(&regions, 11, 150.0, 150.0);
        assert_eq!((sky.region(), sky.weather.form_id, sky.fade), (Some("TestRegion"), 4, 1.0));
        assert!(sky.prev.is_none());
        let rolls = sky.rolls;
        let rng = sky.rng;
        sky.place_outdoors(&regions, 11, 250.0, 150.0);
        assert_eq!((sky.region(), sky.weather.form_id, sky.rolls, sky.rng), (Some("TestRegion"), 4, rolls, rng));
        sky.place_outdoors(&regions, 11, 4096.0, 150.0);
        assert_eq!((sky.region(), sky.weather.form_id, sky.rolls), (None, LISTED, rolls + 1));
        assert_eq!(sky.prev.as_ref().map(|w| w.form_id), Some(4));
        assert_eq!(sky.fade, 0.0);

        sky.place_outdoors(&regions, 11, 300.0, 150.0);
        sky.place_outdoors(&regions, 11, 150.0, 150.0);
        assert_eq!((sky.region(), sky.weather.form_id, sky.rolls), (None, LISTED, rolls + 1));
    }

    #[test]
    fn weather_region_cell_entry_inside_outline_fades_on_either_axis() {
        for axis in 0..2 {
            let (mut sky, mut regions) = region_test_sky();
            for point in &mut regions[0].areas[0] {
                point[axis] -= 150.0;
            }
            let mut at = [150.0, 150.0];
            at[axis] = -100.0;
            sky.place_outdoors(&regions, 11, at[0], at[1]);
            assert_eq!(sky.weather.form_id, LISTED);
            let rolls = sky.rolls;
            at[axis] = -25.0;
            sky.place_outdoors(&regions, 11, at[0], at[1]);
            assert_eq!((sky.region(), sky.rolls), (None, rolls));
            at[axis] = 0.0;
            sky.place_outdoors(&regions, 11, at[0], at[1]);
            assert_eq!((sky.region(), sky.weather.form_id, sky.rolls), (Some("TestRegion"), 4, rolls + 1));
            assert_eq!(sky.prev.as_ref().map(|w| w.form_id), Some(LISTED));
            assert_eq!(sky.fade, 0.0);
        }
    }

    // Synthetic equivalent of with_climate's fresh state and retained start weather
    // [design, #2531; SkyState::with_climate, worldspace::script_sky].
    fn climate_rebuild_test_sky(from: &SkyState) -> SkyState {
        let (mut sky, _) = region_test_sky();
        sky.climate.form_id = 12;
        sky.choices = vec![Choice { weather: Weather { form_id: 5, ..Default::default() }, chance: 100, global: None }];
        sky.weather = from.weather.clone();
        sky.pick = from.weather.form_id;
        sky.resume = Some(from.weather.form_id);
        sky.rng = from.rng;
        sky
    }

    #[test]
    fn weather_region_climate_rebuild_keeps_sample_outside_outline() {
        let (mut sky, regions) = region_test_sky();
        sky.place_outdoors(&regions, 11, 150.0, 150.0);
        sky.place_outdoors(&regions, 11, 250.0, 150.0);
        let mut rebuilt = climate_rebuild_test_sky(&sky);
        // A rebuilt list may have a different order [design: synthetic fixture].
        rebuilt.regions.insert(0, RegionList { region: RegionWeather { form_id: 13, ..Default::default() }, choices: Vec::new() });
        crate::worldspace::carry_climate_sky(&sky, &mut rebuilt, |_| None);
        assert_eq!(rebuilt.source, Some(1));
        assert!(rebuilt.placed);
        assert_eq!(rebuilt.region_cell, sky.region_cell);
        rebuilt.place_outdoors(&regions, 11, 250.0, 150.0);
        rebuilt.set_hour(12.1, false);
        assert_eq!((rebuilt.region(), rebuilt.weather.form_id, rebuilt.rolls, rebuilt.rng), (Some("TestRegion"), 4, 0, sky.rng));
        assert!(rebuilt.prev.is_none());
        assert_eq!(rebuilt.fade, 1.0);

        rebuilt.place_outdoors(&regions, 11, 4096.0, 150.0);
        assert_eq!((rebuilt.region(), rebuilt.weather.form_id, rebuilt.rolls), (None, 5, 1));
        assert_eq!(rebuilt.prev.as_ref().map(|w| w.form_id), Some(4));
        assert_eq!(rebuilt.fade, 0.0);
    }

    #[test]
    fn weather_region_climate_rebuild_seeds_current_gates_before_selecting() {
        for previous_value in [None, Some(0.0), Some(1.0)] {
            let gated_region = |sky: &mut SkyState| {
                sky.regions[0].choices.push(Choice {
                    weather: Weather { form_id: GATED, ..Default::default() },
                    chance: 100,
                    global: Some(Gate { id: GATE, edid: None, stored: Some(0.0) }),
                });
            };
            let (mut sky, regions) = region_test_sky();
            gated_region(&mut sky);
            sky.set_live_global(GATE, 1.0);
            sky.place_outdoors(&regions, 11, 150.0, 150.0);
            assert_eq!(sky.weather.form_id, GATED);
            // The old cache may lag a script's current gate [design: synthetic fixture].
            sky.live_globals = previous_value.map(|v| (GATE, v)).into_iter().collect();
            let mut rebuilt = climate_rebuild_test_sky(&sky);
            gated_region(&mut rebuilt);
            crate::worldspace::carry_climate_sky(&sky, &mut rebuilt, |id| {
                assert_eq!(id, GATE);
                Some(1.0)
            });
            let assert_retained = |s: &SkyState| {
                assert_eq!((s.region(), s.weather.form_id, s.pick), (Some("TestRegion"), GATED, GATED));
                assert_eq!((s.rolls, s.rng), (0, sky.rng), "previous gate: {previous_value:?}");
                assert!(s.prev.is_none());
                assert_eq!(s.fade, 1.0);
            };
            assert_retained(&rebuilt);
            rebuilt.set_live_global(GATE, 1.0);
            rebuilt.place_outdoors(&regions, 11, 250.0, 150.0);
            rebuilt.set_hour(12.1, false);
            assert_retained(&rebuilt);
        }
    }

    #[test]
    fn weather_region_climate_rebuild_keeps_sample_inside_outline() {
        let (mut sky, regions) = region_test_sky();
        sky.place_outdoors(&regions, 11, 250.0, 150.0);
        sky.place_outdoors(&regions, 11, 150.0, 150.0);
        let mut rebuilt = climate_rebuild_test_sky(&sky);
        crate::worldspace::carry_climate_sky(&sky, &mut rebuilt, |_| None);
        rebuilt.place_outdoors(&regions, 11, 150.0, 150.0);
        assert_eq!((rebuilt.region(), rebuilt.weather.form_id, rebuilt.rolls), (None, 5, 1));
        assert!(rebuilt.placed);
        assert_eq!(rebuilt.region_cell, sky.region_cell);
        assert!(rebuilt.prev.is_none());
        assert_eq!(rebuilt.fade, 1.0);
    }

    #[test]
    fn weather_region_climate_rebuild_preserves_travel_invalidation() {
        for travel_before_rebuild in [true, false] {
            let (mut sky, regions) = region_test_sky();
            sky.place_outdoors(&regions, 11, 150.0, 150.0);
            if travel_before_rebuild {
                sky.set_interior(None);
            }
            let mut rebuilt = climate_rebuild_test_sky(&sky);
            crate::worldspace::carry_climate_sky(&sky, &mut rebuilt, |_| None);
            if !travel_before_rebuild {
                rebuilt.set_interior(None);
            }
            assert!(rebuilt.region_cell.is_none());
            rebuilt.place_outdoors(&regions, 11, 250.0, 150.0);
            assert_eq!((rebuilt.region(), rebuilt.weather.form_id), (None, 5));
            assert!(rebuilt.prev.is_none());
            assert_eq!(rebuilt.fade, 1.0);
        }

        let (mut sky, regions) = region_test_sky();
        sky.place_outdoors(&regions, 11, 150.0, 150.0);
        let mut rebuilt = climate_rebuild_test_sky(&sky);
        crate::worldspace::carry_climate_sky(&sky, &mut rebuilt, |_| None);
        rebuilt.place_outdoors(&regions, 22, 150.0, 150.0);
        assert_eq!((rebuilt.region(), rebuilt.weather.form_id), (None, 5));
        assert!(rebuilt.prev.is_none());
        assert_eq!(rebuilt.fade, 1.0);
    }

    #[test]
    fn weather_region_same_cell_travel_rechecks_and_snaps() {
        let (mut sky, regions) = region_test_sky();
        sky.place_outdoors(&regions, 11, 150.0, 150.0);
        let rolls = sky.rolls;
        sky.set_interior(None);
        sky.place_outdoors(&regions, 11, 250.0, 150.0);
        assert_eq!((sky.region(), sky.weather.form_id, sky.rolls), (None, LISTED, rolls + 1));
        assert!(sky.prev.is_none());
        assert_eq!(sky.fade, 1.0);
        sky.set_interior(Some(Default::default()));
        sky.set_interior(None);
        sky.place_outdoors(&regions, 11, 150.0, 150.0);
        assert_eq!((sky.region(), sky.weather.form_id), (Some("TestRegion"), 4));
        assert!(sky.prev.is_none());
        assert_eq!(sky.fade, 1.0);
    }

    #[test]
    fn weather_region_world_change_rechecks_same_grid_and_snaps() {
        let (mut sky, regions) = region_test_sky();
        sky.place_outdoors(&regions, 11, 150.0, 150.0);
        sky.place_outdoors(&regions, 22, 150.0, 150.0);
        assert_eq!((sky.region(), sky.weather.form_id), (None, LISTED));
        assert!(sky.prev.is_none());
        assert_eq!(sky.fade, 1.0);
    }

    #[test]
    fn weather_region_negative_cell_entry_uses_floor() {
        let (mut sky, mut regions) = region_test_sky();
        for point in &mut regions[0].areas[0] {
            point[0] -= 300.0;
        }
        sky.place_outdoors(&regions, 11, -150.0, 150.0);
        sky.place_outdoors(&regions, 11, -0.5, 150.0);
        assert_eq!(sky.region(), Some("TestRegion"));
        sky.place_outdoors(&regions, 11, 0.0, 150.0);
        assert_eq!(sky.region(), None);
        assert_eq!(sky.fade, 0.0);
    }

    #[test]
    fn weather_region_travel_preserves_script_hold() {
        let (mut sky, regions) = region_test_sky();
        sky.restore_saved(&saved_held());
        sky.place_outdoors(&regions, 11, 150.0, 150.0);
        sky.set_interior(None);
        sky.place_outdoors(&regions, 11, 250.0, 150.0);
        assert_eq!((sky.weather.form_id, sky.pick), (HELD, LISTED));
        assert!(sky.script.is_some_and(|sw| sw.hold));
    }

    #[test]
    fn stepping_back_out_of_a_region_mid_fade_turns_the_fade_round() {
        const REGION: u32 = 500;
        const CAMP: u32 = 3;
        let mut sky = sky_started_on_held();
        sky.resume = None;
        sky.clock_seen = true;
        let camp = Weather { form_id: CAMP, edid: Some("Camp".into()), ..Default::default() };
        sky.regions = vec![RegionList {
            region: RegionWeather { form_id: REGION, ..Default::default() },
            choices: vec![Choice { weather: camp, chance: 100, global: None }],
        }];
        sky.place(Some(None), false);
        assert_eq!((sky.weather.form_id, sky.prev.as_ref().map(|p| p.form_id)), (LISTED, None));
        // Into the region: a fade from the climate's weather (30 s, a quarter game hour).
        sky.place(Some(Some(REGION)), false);
        assert_eq!((sky.weather.form_id, sky.prev.as_ref().map(|p| p.form_id), sky.fade), (CAMP, Some(LISTED), 0.0));
        sky.follow_game_clock(12.1, 25.0);
        assert!((sky.fade - 0.4).abs() < 1e-3, "{}", sky.fade);
        // Back out before it ends: the screen shows 40 % camp, and stays there to fade back
        // (it went to the whole camp look and faded from there).
        sky.place(Some(None), false);
        assert_eq!((sky.weather.form_id, sky.prev.as_ref().map(|p| p.form_id)), (LISTED, Some(CAMP)));
        assert!((sky.fade - 0.6).abs() < 1e-3, "{}", sky.fade);
        // A third weather mid-fade fades from the one showing more.
        sky.fade_to(Weather { form_id: 4, ..Default::default() }, false);
        assert_eq!((sky.weather.form_id, sky.prev.as_ref().map(|p| p.form_id), sky.fade), (4, Some(LISTED), 0.0));
        sky.fade = 0.3;
        sky.fade_to(Weather { form_id: 5, ..Default::default() }, false);
        assert_eq!((sky.weather.form_id, sky.prev.as_ref().map(|p| p.form_id), sky.fade), (5, Some(LISTED), 0.0));
    }

    /// A fade of a quarter game hour from noon, the sky just made at 12.0.
    fn fading_sky() -> SkyState {
        let mut sky = sky_started_on_held();
        sky.set_hour(12.0, true);
        let weather = Weather { form_id: 0x1234, trans_delta: Some(255), ..Default::default() };
        sky.script_weather(weather, [None; 6], false, true);
        sky
    }

    #[test]
    fn a_smooth_weather_fade_follows_each_clock_interval_once() {
        let mut sky = fading_sky();
        sky.smooth_fade = true;
        for offset in [0.002, 0.004, 0.004, 0.009, 0.011] {
            sky.follow_game_clock(12.0 + offset, 25.0);
            assert!((sky.fade - offset / 0.25).abs() < 1e-5);
        }
    }

    /// Without the volume a fade moves with the half-minute steps alone, by the whole step, and
    /// the readings between them change nothing the sky draws.
    #[test]
    fn a_weather_fade_moves_with_the_half_minute_steps() {
        let mut sky = fading_sky();
        sky.changed = false;
        // What the fade is after each reading: only a step of a half game minute moves it.
        for (offset, faded) in [(0.002, 0.0), (0.004, 0.0), (0.009, 0.009), (0.011, 0.009), (0.0175, 0.0175)] {
            let before = sky.fade;
            sky.follow_game_clock(12.0 + offset, 25.0);
            assert!((sky.fade - faded / 0.25).abs() < 5e-5, "{offset}: {}", sky.fade);
            assert_eq!(std::mem::take(&mut sky.changed), sky.fade != before, "{offset}");
        }
    }

    /// The volume switched on or off between two steps: the switch moves nothing, and the next
    /// step gives the fade exactly the time it has not had.
    #[test]
    fn switching_the_volume_in_a_fade_neither_jumps_nor_loses_time() {
        // On in the middle of a step: smooth from here, the time before it paid at the step.
        let mut sky = fading_sky();
        sky.follow_game_clock(12.004, 25.0);
        assert_eq!(sky.fade, 0.0);
        sky.smooth_fade = true;
        sky.follow_game_clock(12.004, 25.0);
        assert_eq!(sky.fade, 0.0);
        sky.follow_game_clock(12.006, 25.0);
        assert!((sky.fade - 0.002 / 0.25).abs() < 5e-5, "{}", sky.fade);
        sky.follow_game_clock(12.009, 25.0);
        assert!((sky.fade - 0.009 / 0.25).abs() < 5e-5, "{}", sky.fade);
        // Off in the middle of a step: it holds, and the step does not count the smooth part twice.
        sky.follow_game_clock(12.012, 25.0);
        let held = sky.fade;
        assert!((held - 0.012 / 0.25).abs() < 5e-5, "{held}");
        sky.smooth_fade = false;
        sky.follow_game_clock(12.015, 25.0);
        assert_eq!(sky.fade, held);
        sky.follow_game_clock(12.018, 25.0);
        assert!((sky.fade - 0.018 / 0.25).abs() < 5e-5, "{}", sky.fade);
    }

    /// A sky made by a worldspace door in the middle of a step takes over the last clock reading
    /// and what the fade was given of the step: read again at the same hour the fade stays, and the
    /// step gives it the rest once, whether all or only a part of the step was given before.
    #[test]
    fn a_door_in_a_step_counts_no_time_of_a_fade_twice() {
        for smooth_from_the_start in [true, false] {
            let mut sky = fading_sky();
            sky.smooth_fade = smooth_from_the_start;
            sky.follow_game_clock(12.004, 25.0);
            sky.smooth_fade = true;
            sky.follow_game_clock(12.006, 25.0);
            let given = if smooth_from_the_start { 0.006 } else { 0.002 };
            assert!((sky.fade - given / 0.25).abs() < 5e-5, "{}", sky.fade);
            let mut next = sky_started_on_held();
            next.set_hour(sky.hour, true);
            next.carry_weather_from(&sky);
            next.smooth_fade = true;
            next.follow_game_clock(12.006, 25.0);
            assert!((next.fade - given / 0.25).abs() < 5e-5, "the same reading again: {}", next.fade);
            next.follow_game_clock(12.007, 25.0);
            assert!((next.fade - (given + 0.001) / 0.25).abs() < 5e-5, "{}", next.fade);
            next.follow_game_clock(12.009, 25.0);
            assert!((next.fade - 0.009 / 0.25).abs() < 5e-5, "the step pays the rest: {}", next.fade);
        }
    }

    /// The row switched off: `CloudVolumeShown` still says drawn when the clock is read in that
    /// frame (it is written in `PostUpdate`), and the reading moves no fade all the same.
    #[test]
    fn the_fade_is_smooth_no_more_in_the_frame_the_row_goes_off() {
        use crate::graphics::{Graphics, Preset};
        use crate::volumetric_clouds::CloudVolumeShown;
        #[derive(Resource)]
        struct Clock(f32);
        let mut app = App::new();
        app.insert_resource(Graphics { volumetric_clouds: true, ..Graphics::preset(Preset::Vanilla) })
            .insert_resource(CloudVolumeShown(true))
            .insert_resource(fading_sky())
            .insert_resource(Clock(12.002))
            .add_systems(Update, (fade_cadence, |mut sky: ResMut<SkyState>, clock: Res<Clock>| sky.follow_game_clock(clock.0, 25.0)).chain())
            // As `show_volume` and the rest of the volume leave it at the frame's end.
            .add_systems(PostUpdate, |g: Res<Graphics>, mut shown: ResMut<CloudVolumeShown>| shown.0 = g.volumetric_clouds);
        let fade = |app: &App| app.world().resource::<SkyState>().fade;
        app.update();
        assert!((fade(&app) - 0.002 / 0.25).abs() < 5e-5, "on and drawn: every reading");
        app.world_mut().resource_mut::<Graphics>().volumetric_clouds = false;
        app.world_mut().resource_mut::<Clock>().0 = 12.004;
        assert!(app.world().resource::<CloudVolumeShown>().0, "last frame's");
        app.update();
        assert!((fade(&app) - 0.002 / 0.25).abs() < 5e-5, "off: {}", fade(&app));
        app.world_mut().resource_mut::<Clock>().0 = 12.009;
        app.update();
        assert!((fade(&app) - 0.009 / 0.25).abs() < 5e-5, "the step gives the rest");
        // On again and not drawn yet: the steps alone.
        app.world_mut().resource_mut::<Graphics>().volumetric_clouds = true;
        app.world_mut().resource_mut::<Clock>().0 = 12.011;
        app.update();
        assert!((fade(&app) - 0.009 / 0.25).abs() < 5e-5);
        // The game's chain asks before it reads the clock.
        assert!(include_str!("main.rs").contains("worldspace::script_sky, sky::fade_cadence, sky_follows_game, "));
    }

    #[test]
    fn the_sun_light_follows_the_clock_between_the_skys_steps() {
        let mut sky = sky_started_on_held();
        sky.set_hour(12.0, true);
        assert_eq!(sky.sun_light_hour(), 12.0);
        // Less than the sky's half-minute step: the sky holds, the sun light moves.
        let a_little = 12.0 + 1.0 / 600.0;
        sky.follow_game_clock(a_little, 25.0);
        assert_eq!((sky.hour, sky.sun_light_hour()), (12.0, a_little));
        // A whole step: both at the clock.
        let a_step = 12.0 + 1.0 / 100.0;
        sky.follow_game_clock(a_step, 25.0);
        assert_eq!((sky.hour, sky.sun_light_hour()), (a_step, a_step));
        // An hour set another way wins over the last clock reading.
        sky.follow_game_clock(a_step + 1.0 / 600.0, 25.0);
        sky.set_hour(18.0, true);
        assert_eq!(sky.sun_light_hour(), 18.0);
    }

    fn saved_held() -> serde_json::Value {
        serde_json::json!({"weather": format!("W{HELD}"), "held": true, "rng": "123"})
    }

    #[test]
    fn held_weather_survives_sky_replacement_and_releases_to_the_new_climate() {
        let mut from = sky_started_on_held();
        from.place(Some(None), false);
        from.rolls = 41;
        from.script_weather(Weather { form_id: HELD, ..Default::default() }, [None; 6], true, true);
        let mut to = sky_started_on_held();
        to.climate.form_id = 100;
        to.resume = None;
        to.weather = to.choices[0].weather.clone();

        to.carry_weather_from(&from);
        assert_eq!((to.weather.form_id, to.climate.form_id), (HELD, 100));
        assert_eq!(to.script.unwrap().roll, to.rolls);
        assert!(to.script.unwrap().hold);
        assert!(!to.pinned);
        to.place(Some(None), false);
        assert_eq!((to.weather.form_id, to.pick), (HELD, LISTED));
        to.release_script_weather();
        assert_eq!(to.weather.form_id, HELD);
        to.set_live_global(GATE, 1.0);
        assert_eq!(to.weather.form_id, GATED);
        assert!(to.script.is_none());
    }

    #[test]
    fn held_weather_carries_its_fade_and_both_weathers_adapters() {
        let mut from = sky_started_on_held();
        from.place(Some(None), false);
        let old_adapters = [Some(Adapter { tint: Some(([0.1, 0.2, 0.3], 0.4)), ..Default::default() }); 6];
        let held_adapters = [Some(Adapter { tint: Some(([0.5, 0.6, 0.7], 0.8)), ..Default::default() }); 6];
        from.adapters.insert(LISTED, old_adapters);
        from.script_weather(Weather { form_id: HELD, ..Default::default() }, held_adapters, false, true);
        from.fade = 0.375;
        let mut to = sky_started_on_held();
        to.adapters.clear();

        to.carry_weather_from(&from);
        to.place(Some(None), false);
        assert_eq!((to.weather.form_id, to.prev.as_ref().unwrap().form_id, to.fade), (HELD, LISTED, 0.375));
        assert_eq!(to.adapters.get(&HELD), Some(&held_adapters));
        assert_eq!(to.adapters.get(&LISTED), Some(&old_adapters));
        to.script_weather(Weather { form_id: GATED, ..Default::default() }, [None; 6], true, false);
        assert_eq!(to.weather.form_id, GATED);
        assert!(!to.script.unwrap().hold);
        assert!(to.prev.is_none());
    }

    #[test]
    fn natural_weather_does_not_gain_an_override_on_sky_replacement() {
        let mut from = sky_started_on_held();
        from.place(Some(None), false);
        let mut to = sky_started_on_held();
        to.weather = to.choices[1].weather.clone();
        to.resume = None;
        to.carry_weather_from(&from);
        assert_eq!(to.weather.form_id, GATED);
        assert!(to.script.is_none());
        assert!(!to.pinned);
        assert!(to.resume.is_none());
        to.place(Some(None), false);
        assert_eq!(to.weather.form_id, LISTED);

        // [design: worldspace.rs::script_sky] A climate rebuild's own start weather
        // remains resumable when there is no override to carry.
        let mut climate = sky_started_on_held();
        climate.weather = climate.choices[0].weather.clone();
        climate.resume = Some(LISTED);
        climate.carry_weather_from(&from);
        assert_eq!(climate.resume, Some(LISTED));
        climate.place(Some(None), false);
        assert_eq!((climate.weather.form_id, climate.rolls), (LISTED, 0));
    }

    #[test]
    fn a_command_line_pin_and_resume_survive_sky_replacement() {
        let mut from = sky_started_on_held();
        from.pinned = true;
        let mut to = sky_started_on_held();
        to.weather = to.choices[0].weather.clone();
        to.resume = None;
        to.carry_weather_from(&from);
        assert!(to.pinned);
        assert_eq!(to.resume, Some(HELD));
        assert!(to.script.is_none());
        to.place(Some(None), false);
        assert_eq!(to.weather.form_id, HELD);
    }

    #[test]
    fn a_held_weather_saved_after_sky_replacement_loads_held() {
        let mut from = sky_started_on_held();
        from.script_weather(from.weather.clone(), [None; 6], true, true);
        let mut to = sky_started_on_held();
        to.resume = None;
        to.weather = to.choices[0].weather.clone();
        to.carry_weather_from(&from);
        to.place(Some(None), false);

        let mut world = World::new();
        world.insert_resource(to);
        let saved = (SAVE_HOOK.save)(&world).unwrap();
        assert_eq!(saved["held"], true);
        assert_eq!(saved["weather"], format!("W{HELD}"));
        let saved: serde_json::Value = serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
        let mut loaded = sky_started_on_held();
        loaded.restore_saved(&saved);
        loaded.place(Some(None), false);
        loaded.set_live_global(GATE, 1.0);
        assert_eq!(loaded.weather.form_id, HELD);
        assert!(loaded.script.unwrap().hold);
        loaded.release_script_weather();
        loaded.set_live_global(GATE, 0.0);
        assert_eq!(loaded.weather.form_id, LISTED);
    }

    #[test]
    fn a_saved_hold_outlasts_the_first_look_and_what_follows() {
        let mut sky = sky_started_on_held();
        sky.restore_saved(&saved_held());
        sky.place(Some(None), false);
        // The climate still rolls under the hold; what it picks waits for a release.
        assert_eq!((sky.weather.form_id, sky.rolls, sky.pick), (HELD, 1, LISTED));
        let mut rng = 123;
        next_unit(&mut rng);
        assert_eq!(sky.rng, rng, "the roll comes from the saved random state");
        sky.follow_game_clock(12.0 + 1.0 / 60.0, 25.0);
        assert_eq!(sky.weather.form_id, HELD);
        sky.set_live_global(GATE, 1.0);
        assert_eq!((sky.weather.form_id, sky.rolls, sky.pick), (HELD, 2, GATED));
        assert!(sky.prev.is_none());
    }

    #[test]
    fn a_release_after_the_load_stands_and_the_next_roll_takes_over() {
        let mut sky = sky_started_on_held();
        sky.restore_saved(&saved_held());
        sky.place(Some(None), false);
        // A load script releases it before the engine's part of the save is put back.
        sky.release_script_weather();
        assert_eq!(sky.weather.form_id, HELD, "until the next roll, as any release");
        let rng = sky.rng;
        let mut world = World::new();
        world.insert_resource(sky);
        (SAVE_HOOK.load)(&mut world, &saved_held());
        let mut sky = world.remove_resource::<SkyState>().unwrap();
        assert_eq!(sky.rng, rng, "the hook does not put the random state back");
        sky.set_live_global(GATE, 1.0);
        assert_eq!(sky.weather.form_id, GATED, "the hook did not hold it again");
        assert!(sky.script.is_none());
    }

    #[test]
    fn only_the_saved_held_weather_is_held() {
        // Not held in the save, and held in a save whose weather the sky could not start
        // on: the first look's roll shows.
        for saved in [serde_json::json!({"weather": format!("W{HELD}"), "held": false}), serde_json::json!({"weather": "Gone", "held": true})] {
            let mut sky = sky_started_on_held();
            sky.restore_saved(&saved);
            sky.place(Some(None), false);
            assert_eq!(sky.weather.form_id, LISTED, "{saved}");
            assert!(sky.script.is_none());
        }
    }

    #[test]
    fn a_new_dome_is_coloured_even_when_the_sky_colours_stay() {
        // #1395: on a worldspace change the colours were recorded against the old dome in
        // the frame before the new one spawned; the new one kept its raw weights.
        let colours = [0.2, 0.4, 0.5, 0.5, 0.6, 0.6, 0.6, 0.7, 0.7];
        let (old, new) = (AssetId::<Mesh>::from(bevy::asset::uuid::Uuid::from_u128(1)), AssetId::<Mesh>::from(bevy::asset::uuid::Uuid::from_u128(2)));
        assert!(dome_needs_colour(None, old, colours));
        assert!(!dome_needs_colour(Some((old, colours)), old, colours));
        assert!(dome_needs_colour(Some((old, colours)), new, colours));
        let mut moved = colours;
        moved[0] += 0.01;
        assert!(dome_needs_colour(Some((old, colours)), old, moved));
    }

    #[test]
    fn a_caster_counts_only_near_the_slice_outline() {
        // A diamond with a point inside it; the hull keeps the four corners.
        let outline = hull([Vec2::new(0.0, -10.0), Vec2::new(10.0, 0.0), Vec2::new(0.0, 10.0), Vec2::new(-10.0, 0.0), Vec2::new(1.0, 1.0)]);
        assert_eq!(outline.len(), 4);
        // Inside, and touching an edge.
        assert!(overlaps(&outline, Vec2::ZERO, Vec2::splat(1.0)));
        assert!(overlaps(&outline, Vec2::new(6.0, 6.0), Vec2::splat(1.0)));
        // In the diamond's bounding square but past its edge: Bevy would draw it, nothing
        // in the slice is under it.
        assert!(!overlaps(&outline, Vec2::new(8.0, 8.0), Vec2::splat(1.0)));
        // The same with the margin added to its reach.
        assert!(overlaps(&outline, Vec2::new(8.0, 8.0), Vec2::splat(1.0 + 5.0)));
    }

    #[test]
    fn cascade_reaches_further_toward_the_light() {
        // A light shining straight down; a box 100 units deep whose near plane is at y = 50.
        let range = 100.0;
        let near_y = 50.0;
        let clip_from_cascade = Mat4::from_cols(Vec4::new(0.01, 0.0, 0.0, 0.0), Vec4::new(0.0, 0.01, 0.0, 0.0), Vec4::new(0.0, 0.0, 1.0 / range, 0.0), Vec4::new(0.0, 0.0, 1.0, 1.0));
        // Light space: z points up toward the light, origin on the near plane.
        let cascade_from_world = Mat4::from_cols(Vec4::X, Vec4::new(0.0, 0.0, 1.0, 0.0), Vec4::new(0.0, -1.0, 0.0, 0.0), Vec4::new(0.0, 0.0, -near_y, 1.0));
        let mut cascade = bevy::light::cascade::Cascade {
            world_from_cascade: cascade_from_world.inverse(),
            clip_from_cascade,
            clip_from_world: clip_from_cascade * cascade_from_world,
            texel_size: 1.0,
        };
        let depth = |c: &bevy::light::cascade::Cascade, y: f32| c.clip_from_world.project_point3(Vec3::new(3.0, y, 4.0)).z;
        assert!((depth(&cascade, near_y) - 1.0).abs() < 1e-6);
        assert!(depth(&cascade, near_y - range).abs() < 1e-6);
        reach_toward_light(&mut cascade, 200.0);
        // The far plane stays, the near plane moves 200 units toward the light.
        assert!(depth(&cascade, near_y - range).abs() < 1e-6);
        assert!((depth(&cascade, near_y + 200.0) - 1.0).abs() < 1e-6);
        assert!((depth(&cascade, near_y) - 1.0 / 3.0).abs() < 1e-6);
        // Projection and view-projection agree.
        let both = cascade.clip_from_cascade * cascade_from_world;
        assert!(both.abs_diff_eq(cascade.clip_from_world, 1e-6));
        // Nothing to stretch.
        let before = cascade.clip_from_world;
        reach_toward_light(&mut cascade, 0.0);
        assert_eq!(before, cascade.clip_from_world);
    }

    /// The far bounds and overlap of each layout at the default 8,000 units, without and
    /// with the longer shadows' reach: only the last bound takes the reach.
    #[test]
    fn cascade_layouts_at_the_default_distance() {
        use CascadeLayout::{Performance, Quality, Spaced};
        let near = |(got, overlap): (Vec<f32>, f32), want: &[f32], want_overlap: f32| {
            assert_eq!(got.len(), want.len(), "{got:?} against {want:?}");
            assert!(got.iter().zip(want).all(|(g, w)| (g - w).abs() < 0.5), "{got:?} against {want:?}");
            assert_eq!(overlap, want_overlap, "{got:?}");
        };
        for map in [2048, 4096] {
            // Performance, whatever the maps: 1,500, 4,500, then the distance times the reach.
            near(shadow_cascade_layout(8000.0, 1.0, Performance, map), &[1500.0, 4500.0, 8000.0], 0.2);
            near(shadow_cascade_layout(8000.0, 1.5, Performance, map), &[1500.0, 4500.0, 12000.0], 0.2);
            // 3 spaced as before (`--graphics-cycle cascades=3`): geometric from a twelfth
            // of the distance (667, 2,828, 12,000 with the reach, as measured on 2026-11-16).
            near(shadow_cascade_layout(8000.0, 1.0, Spaced(3), map), &[666.7, 2309.4, 8000.0], 0.2);
            near(shadow_cascade_layout(8000.0, 1.5, Spaced(3), map), &[666.7, 2828.4, 12000.0], 0.2);
            // The moon's single cascade.
            near(shadow_cascade_layout(3000.0, 1.0, Spaced(1), map), &[3000.0], 0.2);
            near(shadow_cascade_layout(8000.0, 1.5, Spaced(1), map), &[12000.0], 0.2);
        }
        // Quality with 2048² maps: 700, 2,000, 5,000, overlap 0.2.
        near(shadow_cascade_layout(8000.0, 1.0, Quality, 2048), &[700.0, 2000.0, 5000.0, 8000.0], 0.2);
        near(shadow_cascade_layout(8000.0, 1.5, Quality, 2048), &[700.0, 2000.0, 5000.0, 12000.0], 0.2);
        // Quality with 4096² maps, overlap 0.4: with Longer shadows 1,500, 3,500, 7,000 (the
        // layout measured); without, the middle two capped at 0.3 and 0.6 of the last.
        near(shadow_cascade_layout(8000.0, 1.5, Quality, 4096), &[1500.0, 3500.0, 7000.0, 12000.0], 0.4);
        near(shadow_cascade_layout(8000.0, 1.0, Quality, 4096), &[1500.0, 2400.0, 4800.0, 8000.0], 0.4);
        near(shadow_cascade_layout(16000.0, 1.0, Quality, 4096), &[3000.0, 4800.0, 9600.0, 16000.0], 0.4);
        // The shares follow the distance (the Shadow distance slider).
        near(shadow_cascade_layout(16000.0, 1.0, Quality, 2048), &[1400.0, 4000.0, 10000.0, 16000.0], 0.2);
        near(shadow_cascade_layout(16000.0, 1.0, Performance, 2048), &[3000.0, 9000.0, 16000.0], 0.2);
        // The counts.
        assert_eq!((Quality.count(), Performance.count(), Spaced(3).count(), Spaced(0).count()), (4, 3, 3, 1));
        // The config carries them with the layout's overlap and the minimum distance of before.
        let config = shadow_cascades(8000.0, 1.0, Quality, 4096);
        assert_eq!((config.bounds.clone(), config.overlap_proportion), shadow_cascade_layout(8000.0, 1.0, Quality, 4096));
        assert_eq!(config.minimum_distance, 5.0);
        // Without the map resource: Bevy's default side, the smaller maps' layout.
        assert_eq!(map_side(None), 2048);
    }

    /// Over the Shadow distance row's whole range and below (from 0 to twice the default
    /// distance, every 25 units, and the row's steps), the bounds rise strictly and the
    /// first stays past the minimum distance, for every layout, reach and map size. At 0 the
    /// sun casts none (`casts_sun_shadows`), but the layout stays well formed.
    #[test]
    fn cascade_bounds_rise_at_short_distances() {
        use CascadeLayout::{Performance, Quality, Spaced};
        let layouts = [Quality, Performance, Spaced(1), Spaced(2), Spaced(3), Spaced(4), Spaced(5)];
        let sweep = (0..=640).map(|k| k as f32 * 25.0);
        let steps = crate::settings::SHADOW_DISTANCES.map(|v| DEFAULT_SHADOW_DISTANCE * v);
        for distance in sweep.chain(steps).chain([DEFAULT_SHADOW_DISTANCE * crate::draw_distance::MIN, 10.0, 3.0, 0.5]) {
            for reach in [1.0, 1.5] {
                for layout in layouts {
                    for map in [2048, 4096] {
                        let (b, _) = shadow_cascade_layout(distance, reach, layout, map);
                        assert_eq!(b.len(), layout.count());
                        assert!(b[0] > SHADOW_MINIMUM_DISTANCE, "{distance} x {reach}, {layout:?} {map}: {b:?}");
                        assert!(b.windows(2).all(|w| w[1] > w[0]), "{distance} x {reach}, {layout:?} {map}: {b:?}");
                    }
                }
            }
        }
        // The Shadow distance row's least step above 0 % (a fifth of 8,000): 1,600, a step
        // of the room 70 (65 from a first bound of 300); every share and cap stays as it is.
        assert_eq!(crate::settings::SHADOW_DISTANCES[1], 0.2);
        near_eq(&shadow_cascade_layout(1600.0, 1.0, Quality, 2048).0, &[200.0, 400.0, 1000.0, 1600.0]);
        near_eq(&shadow_cascade_layout(1600.0, 1.0, Quality, 4096).0, &[300.0, 480.0, 960.0, 1600.0]);
        near_eq(&shadow_cascade_layout(1600.0, 1.0, Performance, 2048).0, &[300.0, 900.0, 1600.0]);
        // At a tenth of the default distance (the slider's low end before the overhaul; a
        // short `--shadow-distance` at 20 %) the first bound is held at 200 and the room to
        // the last is 600 (a step 30): Quality's second share (200) is lifted one step above
        // the first; the 4096 caps (240, 480) and the other shares stay as they are.
        near_eq(&shadow_cascade_layout(800.0, 1.0, Quality, 2048).0, &[200.0, 230.0, 500.0, 800.0]);
        near_eq(&shadow_cascade_layout(800.0, 1.0, Quality, 4096).0, &[200.0, 240.0, 480.0, 800.0]);
        near_eq(&shadow_cascade_layout(800.0, 1.0, Performance, 2048).0, &[200.0, 450.0, 800.0]);
        // With no distance at all (the row's 0 %, which casts none: `casts_sun_shadows`):
        // the last at 20, the first at half of it, the rest a step apart above it.
        near_eq(&shadow_cascade_layout(0.0, 1.0, Quality, 2048).0, &[10.0, 10.5, 11.0, 20.0]);
        fn near_eq(got: &[f32], want: &[f32]) {
            assert!(got.len() == want.len() && got.iter().zip(want).all(|(g, w)| (g - w).abs() < 0.5), "{got:?} against {want:?}");
        }
        assert!(!casts_sun_shadows(true, 0.0) && !casts_sun_shadows(false, 1600.0) && casts_sun_shadows(true, 1600.0));
    }

    #[test]
    fn dome_mixes_display_values() {
        let (lower, horizon, upper) = ([0.1, 0.2, 0.3], [0.7, 0.8, 0.9], [0.03, 0.06, 0.25]);
        // Each channel of the vertex colour gives one colour: red the horizon's, green
        // sky-lower, blue sky-upper.
        for (w, want) in [([1.0, 0.0, 0.0], horizon), ([0.0, 1.0, 0.0], lower), ([0.0, 0.0, 1.0], upper)] {
            let c = dome_color(w, lower, horizon, upper);
            assert!(c.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-4), "{w:?}");
        }
        // Half of a colour as display values is darker than half in linear light.
        assert!(dome_color([0.0, 0.0, 0.5], lower, horizon, upper)[2] < 0.5 * upper[2]);
    }

    #[test]
    fn moon_material_preserves_alpha_and_sky_scale_flags() {
        let mat = moon_material();
        assert!(matches!(mat.base.alpha_mode, AlphaMode::Blend));
        let old = mat.extension.light;
        assert_eq!(old.params.y, 1.0);
        assert_eq!(old.params.z, 0.0);
        assert_eq!(old.params.w, 0.0);

        let updated = Vec4::new(1.87, 0.0, 1.0, 0.0)
            .with_y(old.params.y)
            .with_z(old.params.z)
            .with_w(old.params.w);
        assert_eq!(updated, Vec4::new(1.87, 1.0, 0.0, 0.0));
    }

    #[test]
    fn physical_sky_and_sun_transmittance_match_desert_optics() {
        let afternoon_sun = Vec3::new(0.0, 0.50, -0.866_025_4).normalize();
        let zenith = eval_physical_sky(Vec3::Y, afternoon_sun, 1.0);
        let horizon = eval_physical_sky(Vec3::new(1.0, 0.08, 0.0).normalize(), afternoon_sun, 1.0);
        assert!(zenith.z > zenith.y * 2.0 && zenith.y > zenith.x * 2.0 && zenith.z > zenith.x * 4.0, "clear desert zenith is deep cobalt: {zenith}");
        let luma = |v: Vec3| v.dot(Vec3::new(0.2126, 0.7152, 0.0722));
        assert!(luma(zenith) < luma(horizon), "zenith darker than horizon: zenith={zenith} horizon={horizon}");

        let sunset_sun = Vec3::new(0.0, 0.0, -1.0);
        let sunset_horizon = eval_physical_sky(Vec3::new(0.0, 0.05, -1.0).normalize(), sunset_sun, 1.0);
        let sunset_zenith = eval_physical_sky(Vec3::Y, sunset_sun, 1.0);
        assert!(sunset_horizon.x > sunset_horizon.z * 2.0, "sunset horizon scatters warm orange/red: {sunset_horizon}");
        assert!(luma(sunset_zenith) < luma(sunset_horizon));

        let t_high = physical_sun_transmittance(ATMO_RE + ATMO_H0, 0.75, physical_mie_coeff(1.0) * 1.11);
        let t_low = physical_sun_transmittance(ATMO_RE + ATMO_H0, 0.04, physical_mie_coeff(1.0) * 1.11);
        assert!(luma(t_high) > luma(t_low));
        assert!(t_low.x > t_low.y && t_low.y > t_low.z, "low-sun transmittance reddens: {t_low}");

        let night = eval_physical_sky(Vec3::Y, Vec3::new(0.0, -0.5, 0.866).normalize(), 1.0);
        assert!(night.x > 0.0 && night.z > night.x && night.x < 0.01, "night sky is dim cool blue: {night}");
    }

    /// The clouds' physical daylight is the dome's: on a clear day the physical sky (over the
    /// sky's display scale, which the clouds apply), under rain the weather's at its day level,
    /// and the sun the surfaces' sun light has.
    #[test]
    fn physical_cloud_light_is_the_domes() {
        let mut s = authored_session_sky();
        s.climate.sunrise = (5.0, 7.0);
        s.climate.sunset = (18.0, 20.0);
        s.hour = 13.0;
        let mut cols = vec![vec![[0, 0, 0]; 4]; 10];
        cols[ColorType::SkyUpper as usize] = vec![[40, 46, 255]; 4];
        cols[ColorType::Horizon as usize] = vec![[200, 180, 160]; 4];
        cols[ColorType::Sunlight as usize] = vec![[255, 240, 220]; 4];
        s.weather = Weather { classification: 1, fog: [5_000.0, 120_000.0, 0.0, 0.0, 1.0, 1.0], colors: cols, ..default() };
        let near = |a: [f32; 3], b: Vec3| (Vec3::from_array(a) - b).abs().max_element() < 1e-4 * (1.0 + b.max_element());
        let clear = s.physical_cloud_light();
        let sun = s.sun_dir().normalize();
        let w = smoothstep(0.10, 0.85, s.physical_sky_clearness());
        let level = 0.45 + 0.9 * smoothstep(-0.04, 0.25, sun.y);
        let want = (Vec3::from_array(s.color(ColorType::SkyUpper)) * level).lerp(eval_physical_sky(Vec3::Y, sun, s.physical_sky_clearness()) / s.sky_scale(), w);
        assert!(w > 0.9 && near(clear.upper, want), "{:?} {want}", clear.upper);
        let gained = (Vec3::from_array(s.physical_sun_color()) * PHYSICAL_SUN_GAIN).to_array();
        assert_eq!(clear.sun, gained, "the ground's sun gain, so clouds and land are lit alike");
        assert!(near(clear.ground, (Vec3::from_array(clear.upper) + Vec3::from_array(clear.horizon)) * 0.5));

        s.weather.classification = 4;
        s.weather.rainy = true;
        let rain = s.physical_cloud_light();
        let w = smoothstep(0.10, 0.85, s.physical_sky_clearness());
        let weather = Vec3::from_array(s.color(ColorType::SkyUpper)) * level;
        let want = weather.lerp(eval_physical_sky(Vec3::Y, sun, s.physical_sky_clearness()) / s.sky_scale(), w);
        assert!(w < 0.5 && near(rain.upper, want), "rain keeps most of the weather's own sky: {w} {:?} {want}", rain.upper);
    }

    #[test]
    fn physical_sky_clearness_separates_clear_cloudy_and_stylized_skies() {
        let mut s = authored_session_sky();
        s.climate.sunrise = (5.0, 7.0);
        s.climate.sunset = (18.0, 20.0);
        s.hour = 13.0;
        let sky_colors = |rgb: [u8; 3]| {
            let mut cols = vec![vec![[0, 0, 0]; 4]; 10];
            cols[ColorType::SkyUpper as usize] = vec![rgb; 4];
            cols
        };
        s.weather = Weather {
            classification: 1,
            fog: [5_000.0, 120_000.0, 0.0, 0.0, 1.0, 1.0],
            colors: sky_colors([40, 46, 255]),
            ..default()
        };
        assert!(s.physical_sky_clearness() > 0.9, "clear desert weather");

        s.weather.classification = 4;
        s.weather.rainy = true;
        assert!(s.physical_sky_clearness() <= 0.3, "rainy weather lowers clearness");

        s.weather.classification = 2;
        s.weather.rainy = false;
        s.weather.fog[1] = 10_000.0;
        assert!(s.physical_sky_clearness() < 0.5, "foggy overcast weather lowers clearness");

        s.weather = Weather {
            classification: 1,
            fog: [5_000.0, 120_000.0, 0.0, 0.0, 1.0, 1.0],
            colors: sky_colors([180, 35, 25]),
            ..default()
        };
        assert!(s.physical_sky_clearness() <= 0.3, "stylized crimson sky stays faithful: {}", s.physical_sky_clearness());
    }

    fn assert_fog_falloff(actual: &FogFalloff, expected: &FogFalloff) {
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
    }

    #[test]
    fn interior_zero_fog_range_disables_fog_instead_of_clamping_to_one_unit() {
        // Synthetic reproduction of #3038: an interior cell with no lighting template and
        // stored fog range 0..0 with black fog colour must not saturate the room at >=1 unit.
        let cell = fnv_world::lighting::CellLighting {
            ambient: [55, 50, 45],
            fog_color: [0, 0, 0],
            fog_near: 0.0,
            fog_far: 0.0,
            fog_power: 1.0,
            ..Default::default()
        };
        assert!(!interior_fog_active(cell));
        let mut sky = SkyState::default();
        sky.set_interior(Some(cell));
        for no_interior_fog in [false, true] {
            // Start from an opaque underwater-like fog to also cover exiting water into a 0..0 cell.
            let mut fog = DistanceFog {
                color: Color::srgba(0.2, 0.4, 0.3, 1.0),
                falloff: crate::fog::falloff(0.0, 700.0, 1.0, false, 1.0),
                ..default()
            };
            apply_cell_fog(&sky, &mut fog, 1.0, no_interior_fog);
            assert_eq!(fog.color.alpha(), 0.0, "unset 0..0 interior fog must disable fog via alpha 0");
            assert_fog_falloff(&fog.falloff, &crate::fog::falloff(0.0, NO_FOG_FAR, 1.0, false, 1.0));
            for dist in [1.0_f32, 10.0, 500.0] {
                let effective_share = crate::fog::share(0.0, NO_FOG_FAR, 1.0, dist) * fog.color.alpha();
                assert_eq!(effective_share, 0.0, "effective fog share at {dist} units");
            }
        }
    }

    #[test]
    fn interior_degenerate_and_inverted_fog_ranges_disable_fog() {
        for (near, far) in [(500.0, 500.0), (500.0, 100.0), (-50.0, 0.0), (0.0, -10.0), (f32::NAN, 100.0), (0.0, f32::NAN)] {
            let cell = fnv_world::lighting::CellLighting {
                fog_color: [10, 20, 30],
                fog_near: near,
                fog_far: far,
                fog_power: 1.0,
                ..Default::default()
            };
            assert!(!interior_fog_active(cell), "range {near}..{far} should be inactive");
            let mut sky = SkyState::default();
            sky.set_interior(Some(cell));
            let mut fog = DistanceFog::default();
            apply_cell_fog(&sky, &mut fog, 1.0, false);
            assert_eq!(fog.color.alpha(), 0.0, "range {near}..{far}");
            assert_fog_falloff(&fog.falloff, &crate::fog::falloff(0.0, NO_FOG_FAR, 1.0, false, 1.0));
        }
    }

    #[test]
    fn interior_active_fog_range_preserves_opaque_fog_and_no_interior_fog_tweak() {
        // Template-inherited interior range (e.g. 64..6500): remains opaque and active.
        let active = fnv_world::lighting::CellLighting {
            fog_color: [134, 134, 128],
            fog_near: 64.0,
            fog_far: 6500.0,
            fog_power: 1.0,
            ..Default::default()
        };
        assert!(interior_fog_active(active));
        let mut sky = SkyState::default();
        sky.set_interior(Some(active));
        let mut fog = DistanceFog::default();
        apply_cell_fog(&sky, &mut fog, 1.0, false);
        assert_eq!(fog.color.alpha(), 1.0);
        assert_fog_falloff(&fog.falloff, &crate::fog::falloff(64.0, 6500.0, 1.0, false, 1.0));

        // Negative near with positive far stays active with near clamped to 0.
        let neg_near = fnv_world::lighting::CellLighting {
            fog_color: [80, 90, 100],
            fog_near: -100.0,
            fog_far: 2000.0,
            fog_power: 1.2,
            ..Default::default()
        };
        assert!(interior_fog_active(neg_near));
        sky.set_interior(Some(neg_near));
        apply_cell_fog(&sky, &mut fog, 1.0, false);
        assert_eq!(fog.color.alpha(), 1.0);
        assert_fog_falloff(&fog.falloff, &crate::fog::falloff(0.0, 2000.0, 1.2, false, 1.0));

        // No Interior Fog tweak on an active interior range keeps opaque colour with NO_FOG_FAR.
        let tweaked = fnv_world::lighting::CellLighting {
            fog_color: [120, 40, 20],
            fog_near: 27.0,
            fog_far: 1900.0,
            fog_power: 1.7,
            ..Default::default()
        };
        sky.set_interior(Some(tweaked));
        apply_cell_fog(&sky, &mut fog, 1.0, true);
        assert_eq!(fog.color.alpha(), 1.0);
        assert_fog_falloff(&fog.falloff, &crate::fog::falloff(27.0, NO_FOG_FAR, 1.7, false, 1.0));
    }
}
