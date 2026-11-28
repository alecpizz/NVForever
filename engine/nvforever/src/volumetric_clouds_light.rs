//! Volumetric clouds' light (#3032 stage 2e; docs/design/graphics-enhanced.md §3.23): the CPU
//! mirror of volumetric_clouds_light.wesl, the leaf module the cloud march
//! (volumetric_clouds_march.wesl) imports for the shading of one view sample. An engineering
//! choice for the enhanced look, not a fact about the original game.
//!
//! The split [design]: the march keeps each step's transmittance, the
//! colour's weighting, the distance, the early exit and the counters; the leaf takes the ray's
//! angular part once per ray ([`ray_light`]) and lights each sample from its density result and
//! the optical depth toward the light ([`lean_shade`]). The leaf never sees the march's uniform
//! (`CloudVolume`): the march imports the leaf, so the leaf importing the march's struct back
//! would be a circular module dependency; it takes its own small structs ([`Shading`],
//! [`RayLight`]) the march fills ([`shading`]).
//!
//! The light ([`lighting`], [`lean_shade`]): Beer-Lambert extinction toward the light, two octaves of a
//! two-lobe Henyey-Greenstein phase capped at [`crate::volumetric_clouds::PHASE_CAP`] (Hillaire, "Physically
//! Based Sky, Atmosphere and Cloud Rendering in Frostbite", SIGGRAPH 2016 [documented: cited publication]),
//! the back-scatter floor and the light from above and below, every colour bounded together; the
//! direct light's colour a mix of the weather's sunlight and its cloud colour ([`direct_colour`],
//! the `SUNCOLOUR` aid).

use crate::volumetric_clouds::{hg, table_depth_of, target_opacity, CloudView, CloudVolumeUniform, EXTINCTION_TABLE, VolumeInputs, AMBIENT_GAIN, BACK_WEIGHT, G_BACK, G_FORWARD, MAX_LIGHT, PHASE_CAP, SUN_GAIN};
use bevy::math::DVec3;
use bevy::prelude::*;
use bevy::shader::Shader;

/// volumetric_clouds_light.wesl, imported by the march as `nvforever::volumetric_clouds_light`
/// (registered beside the march's own module, volumetric_clouds.rs `MARCH_SHADER`).
pub const LIGHT_SHADER: Handle<Shader> = bevy::asset::uuid_handle!("c4e1a7d2-6b3f-4f8e-9a52-7d0b3e9c1f64");

// ---------------------------------------------------------------------------------------
// The light model (#3032 stage 2e).
// Engineering choices for the enhanced look.
// ---------------------------------------------------------------------------------------

/// Each further octave of the direct light: its optical depth, its contribution and
/// its phase's eccentricity times these factors, one half each, the starting factors of
/// Wrenninge, Kulla and Lundqvist ("Oz: The Great and Volumetric", SIGGRAPH 2013 talk, its
/// equations 1-2 and defaults) [documented: cited publication, read at second
/// hand]; two octaves in all, Bauer 2019 slides 28-29 ("two direct light octaves to
/// approximate multi scattering [Wrenninge13]", "settled for 2 iterations")
/// [documented: Bauer 2019, slides 28-29].
pub const OCTAVE_DEPTH: f32 = 0.5;
pub const OCTAVE_WEIGHT: f32 = 0.5;
pub const OCTAVE_PHASE: f32 = 0.5;
/// The back-scatter floor (Bauer 2019 slide 30: the phase clamped from below by a
/// value that grows with the material's extinction [documented: Bauer 2019, slide 30]): the
/// floor is the local extinction per unit times [`FLOOR_LENGTH`], at most [`FLOOR_MAX`] (in the
/// phase's units, isotropic 1), each times its aid. [engineering choice: 2,000 units bring a
/// dense cumulus (shape 0.8, about 3 optical depths over 5,000 units) near the most, a sheet
/// (1,200 units tall) past it, a tall tower (15,000 units) to about a third; the most 1 lifts the
/// mixed phase's least, 0.52 at about 120 degrees and 0.78 straight back, measurably on a dense lit
/// side seen away from the light, never to the Lambertian 4 that would flatten the whole lobe.]
pub const FLOOR_LENGTH: f32 = 2000.0;
pub const FLOOR_MAX: f32 = 1.0;
/// A column's mean shape for the vertical estimates of the cloud above and below a sample
/// (the unit-shape columns are `K (1 - h)` and `K h`) [engineering choice: the
/// extinction table holds the column's depth per unit of its mean shape; one half is a rounded
/// profile's mean, between a thin edge's tenths and a dense core's 0.8].
pub const MEAN_SHAPE: f32 = 0.5;
/// The default phase's mean cosine (its asymmetry g, without the cap).
pub const G_MEAN: f32 = (1.0 - BACK_WEIGHT) * G_FORWARD + BACK_WEIGHT * G_BACK;
/// The light from above and below falls through a column of optical depth t as
/// `1 / (1 + AMBIENT_FALL t)`: the two-stream diffuse transmission's form, `2 / (2 + (1 - g) t)`
/// for a beam (Bohren 1987, its equations 15 and 21 [documented: cited publication, read at
/// second hand]), with the path doubled for light arriving from a whole hemisphere
/// [hypothesis: the two-stream diffusivity factor of about 2, from memory]: bounded by 1, no
/// exponential collapse, the slow tail of multiply scattered light.
pub const AMBIENT_FALL: f32 = 1.0 - G_MEAN;
/// The light's elevation sines (30 and 60 degrees) over which a measured depth toward the light
/// takes over the vertical estimate of the cloud above (`depth x sin(elevation)` is the
/// vertical column of a plane-parallel slab, good for a high light only) [engineering choice].
pub const MEASURED_FROM: f32 = 0.5;
pub const MEASURED_TO: f32 = 0.866;
/// Each hemisphere's weight in what a sample scatters toward the camera [design: an isotropic
/// scatterer takes half its light from each hemisphere; the energy check needs the two to sum to
/// one].
pub const HEMISPHERE: f32 = 0.5;
/// The light from above: the sky's colour times this gain [engineering choice: twice the first
/// ambient's gain, so that with the half hemisphere weight a cloud's top under open sky keeps the
/// light's top ambient level], the colour the sky's own (`SKY_UPPER_SHARE` of SkyUpper, the rest Horizon: a
/// hemisphere seen from a cloud is mostly the upper sky).
pub const UPPER_GAIN: f32 = 2.0 * AMBIENT_GAIN;
pub const SKY_UPPER_SHARE: f32 = 0.7;
/// The model's direct light colour (#3032 stage 2e, after L2): a mix of (0) the weather's cloud
/// colour (the visible layers' weighted PNAM colour, the one the legacy ambient takes) scaled to the
/// sunlight's Rec. 709 luminance and (1) the weather's sunlight colour, as the light had alone
/// before; the sunlight's share is [`SUN_COLOUR_SHARE`] by default, the `SUNCOLOUR` aid (0..1)
/// otherwise ([`direct_colour`]). [engineering choice: the flat layers the volume replaces are
/// drawn in the weather's cloud colours, which its author chose as the look of lit cloud at that
/// hour; the sunlight colour alone made the sunlit parts a more saturated yellow than those layers
/// (blue over red near 0.60 to 0.66 against their 0.71 at clear noon) [hypothesis: stills of
/// 2026-11-20, graphics-enhanced.md §3.23]; one half the starting value, to be set with pictures.]
pub const SUN_COLOUR_SHARE: f32 = 0.5;
/// The variable of the sunlight's share in the direct light's colour (`SUNCOLOUR`, 0..1), and
/// its alias spelt `SUNCOLOR` (read when the first is not set).
pub const SUN_COLOUR_VAR: &str = "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_SUNCOLOUR";
pub const SUN_COLOR_VAR: &str = "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_SUNCOLOR";
/// Rec. 709's luminance weights for linear RGB (ITU-R BT.709, the Y row) [documented: the
/// standard's coefficients].
pub const LUMA: [f64; 3] = [0.2126, 0.7152, 0.0722];
/// Under this luminance (linear) the cloud colour has no hue to give (no visible layer: the
/// layers' mean is black) and the direct light keeps the sunlight colour [design].
pub const MIN_CLOUD_LUMA: f64 = 1e-6;
/// Clouds by moonlight (#3032): below the horizon the direct light and the light from below are each dimmed
/// toward the luminance of the weather's visible cloud layers, by `(L(cloud) / L(source))^q` where the source
/// is the brighter, at most 1 ([`night_gain`]), q this pull times the night's share ([`night_share`]). An
/// attenuation only: the hues, and the weathers whose night sunlight is no brighter than their clouds, stay.
/// Why: in many weathers the night sunlight is far brighter than the cloud layers, which the flat sky draws nearly
/// black, so clouds lit at the sunlight's luminance glowed [hypothesis: stills of 2026-11-25 at Goodsprings
/// with a development build, the views not kept]. [engineering choice: the contributor's pick from stills, 2026-11-25, at Goodsprings and in Zion's weather.]
pub const NIGHT_PULL: f32 = 0.45;
/// The most the `NIGHTPULL` aid asks (1: the cloud layers' luminance itself).
pub const MAX_NIGHT_PULL: f32 = 2.0;
/// The variable of the night pull (`NIGHTPULL` in a tune file).
pub const NIGHT_PULL_VAR: &str = "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_NIGHTPULL";
/// From [`MIN_CLOUD_LUMA`] to this many times it the night's dimming grows in, so a weather whose clouds fade
/// in from black makes no jump [design].
pub const NIGHT_LUMA_RAMP: f64 = 8.0;
/// The forward lobe's weight by moonlight (the `MOONFORWARD` aid) and the back-scatter floor's (`MOONBACK`), in
/// place of FORWARD's and BACKSCATTER's by the night's share ([`LightAids::phase_weight`]), so moonlit clouds can
/// have a brighter edge near the moon and darker bodies away from it; `None`: the day's weights [engineering
/// choice: the starting values, to be set with pictures].
pub const MOON_FORWARD: Option<f32> = Some(1.6);
pub const MOON_BACK: Option<f32> = Some(0.1);
/// The phase's cap by moonlight (the `MOONCAP` aid), in place of [`PHASE_CAP`](crate::volumetric_clouds::PHASE_CAP)
/// by the night's share ([`LightAids::phase_cap`]): above it the cloud right at the moon outshines the rest;
/// `None`: the day's cap [engineering choice: the starting value, to be set with pictures].
pub const MOON_CAP: Option<f32> = Some(9.0);
/// The most the `MOONCAP` aid asks.
pub const MAX_MOON_CAP: f32 = 16.0;
pub const MOON_CAP_VAR: &str = "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_MOONCAP";
/// The variables of the two moonlight weights (`MOONFORWARD`, `MOONBACK` in a tune file).
pub const MOON_FORWARD_VAR: &str = "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_MOONFORWARD";
pub const MOON_BACK_VAR: &str = "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_MOONBACK";
/// The ground's reflectance for the light from below, neutral [hypothesis: a dry
/// desert's albedo is about 0.3, from memory].
pub const GROUND_REFLECTANCE: f32 = 0.3;
/// The direct light's daylight over the sun's elevation (degrees), as `direct_light` fades it.
const DAYLIGHT_FADE_DEG: f32 = crate::volumetric_clouds::LIGHT_FADE_DEG;
/// The most an aid scales its weight by.
pub const MAX_WEIGHT: f32 = 4.0;

// ---------------------------------------------------------------------------------------
// Form and contrast (#3032, after stage 2i M3): the look of that day was one even cream with
// no shading inside, against references where a cloud runs from a dark blue-grey base to a white
// sunlit rim [design: judged by eye, 2026-11-20]. The start values had bought brightness with an even
// ambient fill (SUN 0.3 against UPPER 1.5, LOWER 1.25). These terms let the cloud be carried on its
// direct light again with its overall level held by one key. Engineering choices for the enhanced
// look, written from the cited descriptions; no code from any project.
// ---------------------------------------------------------------------------------------

/// The powder term (Schneider and Vos 2015: "Beer's-Powder", the light darkened as `1 - e^(-2d)`
/// where little cloud has yet scattered it in [documented: cited publication, from its
/// description]): the share of it applied looking straight away from the light (none toward it,
/// linearly in `(1 - cos) / 2` between), times the `POWDER` aid, at most 1; the local optical depth
/// `d` is the sample's extinction per unit times [`POWDER_LENGTH`] units. [engineering choice: one
/// half; 2,000 units, the floor's length, so a dense cumulus side (6e-4 per unit) keeps 93 % or more
/// and a thin wisp (1e-4) loses up to a third: the creases between lobes read darker.]
pub const POWDER_SHARE: f32 = 0.5;
pub const POWDER_LENGTH: f32 = 2000.0;
/// The start levels, before the keys' weights: the light from above and
/// below at this share of the designed hemisphere's gain, and the back-scatter floor's most at this share
/// of [`FLOOR_MAX`] [hypothesis: judged by eye in an A/B of 2026-11-21, the development build 734673c09 (its
/// view and steps were not kept), where the designed ambient, nearly uniform, flattened the cloud and UPPER
/// and LOWER 0.5 gave the references' contrast, and BACKSCATTER 4 clipped every dense lit sample; the values
/// are our choice].
pub const LEAN_AMBIENT: f32 = 0.5;
pub const LEAN_FLOOR: f32 = 0.5;
/// The number of light weights ([`LightWeight::ALL`]).
pub const LIGHT_WEIGHTS: usize = 10;

/// The light's weights an aid scales (each 0 to [`MAX_WEIGHT`], 1 as designed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightWeight {
    /// The second octave's relative weight before the weights are normalised (0: the first alone).
    Octave,
    /// The back-scatter floor.
    Backscatter,
    /// The light from above.
    Upper,
    /// The light from below.
    Lower,
    /// The ground's reflectance.
    Ground,
    /// The phase's forward lobe.
    Forward,
    /// The direct light (both octaves).
    Sun,
    /// The whole radiance (direct, from above, from below): one exposure-like level, inside the bound.
    Level,
    /// How steeply the cloud above (and below) occludes the light from above (and below): the
    /// ambient's fall per optical depth times this.
    Occlude,
    /// The powder darkening's share ([`POWDER_SHARE`]).
    Powder,
}

impl LightWeight {
    pub const ALL: [LightWeight; LIGHT_WEIGHTS] = [
        LightWeight::Octave,
        LightWeight::Backscatter,
        LightWeight::Upper,
        LightWeight::Lower,
        LightWeight::Ground,
        LightWeight::Forward,
        LightWeight::Sun,
        LightWeight::Level,
        LightWeight::Occlude,
        LightWeight::Powder,
    ];

    /// Its short name in a tune file.
    pub fn short(self) -> &'static str {
        match self {
            LightWeight::Octave => "OCTAVE",
            LightWeight::Backscatter => "BACKSCATTER",
            LightWeight::Upper => "UPPER",
            LightWeight::Lower => "LOWER",
            LightWeight::Ground => "GROUND",
            LightWeight::Forward => "FORWARD",
            LightWeight::Sun => "SUN",
            LightWeight::Level => "LEVEL",
            LightWeight::Occlude => "OCCLUDE",
            LightWeight::Powder => "POWDER",
        }
    }

    /// Its start variable.
    pub fn var(self) -> &'static str {
        match self {
            LightWeight::Octave => "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_OCTAVE",
            LightWeight::Backscatter => "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_BACKSCATTER",
            LightWeight::Upper => "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_UPPER",
            LightWeight::Lower => "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_LOWER",
            LightWeight::Ground => "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_GROUND",
            LightWeight::Forward => "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_FORWARD",
            LightWeight::Sun => "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_SUN",
            LightWeight::Level => "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_LEVEL",
            LightWeight::Occlude => "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_OCCLUDE",
            LightWeight::Powder => "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_POWDER",
        }
    }

    /// Its place in [`LightWeight::ALL`] and [`LightAids::weights`].
    pub fn index(self) -> usize {
        self as usize
    }
}

/// The light's aids: the ten weights, the sunlight's share and the night's values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightAids {
    pub weights: [f32; LIGHT_WEIGHTS],
    /// The sunlight's share in the direct light's colour (0..1, [`SUN_COLOUR_SHARE`] as designed).
    pub sun_colour: f32,
    /// How far the clouds' light is drawn toward the cloud layers' luminance by night (0..[`MAX_NIGHT_PULL`],
    /// [`NIGHT_PULL`] as designed).
    pub night_pull: f32,
    /// The forward lobe's and the back-scatter floor's weights by moonlight, 0..[`MAX_WEIGHT`]; `None`: the day's.
    pub moon_forward: Option<f32>,
    pub moon_back: Option<f32>,
    /// The phase's cap by moonlight, 0..[`MAX_MOON_CAP`]; `None`: the day's.
    pub moon_cap: Option<f32>,
}

impl Default for LightAids {
    fn default() -> Self {
        LightAids { weights: [1.0; LIGHT_WEIGHTS], sun_colour: SUN_COLOUR_SHARE, night_pull: NIGHT_PULL, moon_forward: MOON_FORWARD, moon_back: MOON_BACK, moon_cap: MOON_CAP }
    }
}

impl LightAids {
    /// A weight's aid, held to 0..[`MAX_WEIGHT`] (1 for a value that is not a number).
    pub fn weight(&self, w: LightWeight) -> f32 {
        let v = self.weights[w.index()];
        if v.is_finite() { v.clamp(0.0, MAX_WEIGHT) } else { 1.0 }
    }

    /// These aids with `w` set to `v`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn with_weight(mut self, w: LightWeight, v: f32) -> Self {
        self.weights[w.index()] = v;
        self
    }

    /// The sunlight's share in the direct light's colour, held to 0..1 ([`SUN_COLOUR_SHARE`] for a
    /// value that is not a number).
    pub fn sun_colour_share(&self) -> f32 {
        if self.sun_colour.is_finite() { self.sun_colour.clamp(0.0, 1.0) } else { SUN_COLOUR_SHARE }
    }

    /// The night pull, held to 0..[`MAX_NIGHT_PULL`] ([`NIGHT_PULL`] for a value that is not a number).
    pub fn night_pull(&self) -> f32 {
        if self.night_pull.is_finite() { self.night_pull.clamp(0.0, MAX_NIGHT_PULL) } else { NIGHT_PULL }
    }

    /// A phase weight (FORWARD, BACKSCATTER) at the night's share `night` ([`night_share`]): the day's by day, at
    /// the horizon and where the moon has none of its own; else from the day's to the moon's.
    pub fn phase_weight(&self, w: LightWeight, night: Option<f32>) -> f32 {
        let day = self.weight(w);
        let moon = match w {
            LightWeight::Forward => self.moon_forward,
            LightWeight::Backscatter => self.moon_back,
            _ => None,
        };
        match (night, moon) {
            (Some(n), Some(m)) if m.is_finite() => day + (m.clamp(0.0, MAX_WEIGHT) - day) * n,
            _ => day,
        }
    }

    /// The phase's cap at the night's share `night`: the day's by day, at the horizon and where the moon has none of
    /// its own; else from the day's to the moon's.
    pub fn phase_cap(&self, night: Option<f32>) -> f32 {
        match (night, self.moon_cap) {
            (Some(n), Some(m)) if m.is_finite() => PHASE_CAP + (m.clamp(0.0, MAX_MOON_CAP) - PHASE_CAP) * n,
            _ => PHASE_CAP,
        }
    }

    /// The weights an aid changed and the share and the night's values if changed, in words for the log (empty when
    /// all are as designed).
    pub fn text(&self) -> String {
        let mut parts: Vec<String> = LightWeight::ALL.into_iter().filter(|w| self.weight(*w) != 1.0).map(|w| format!("{} x{:.2}", w.short(), self.weight(w))).collect();
        if self.sun_colour_share() != SUN_COLOUR_SHARE {
            parts.push(format!("SUNCOLOUR {:.2}", self.sun_colour_share()));
        }
        if self.night_pull() != NIGHT_PULL {
            parts.push(format!("NIGHTPULL {:.2}", self.night_pull()));
        }
        for (name, v, designed) in [("MOONFORWARD", self.moon_forward, MOON_FORWARD), ("MOONBACK", self.moon_back, MOON_BACK), ("MOONCAP", self.moon_cap, MOON_CAP)] {
            if v != designed {
                parts.push(v.map_or_else(|| format!("{name} the day's"), |v| format!("{name} x{v:.2}")));
            }
        }
        parts.join(", ")
    }

    /// The light model in force, one log line's text.
    pub fn model_text(&self) -> String {
        let model = format!(
            "two octaves (depth, weight and eccentricity x{OCTAVE_DEPTH} each, weights normalised) lit in the weather's visible cloud layers' PNAM colour at its sunlight's luminance mixed with its sunlight colour (the sunlight's share SUNCOLOUR {:.2}), so the lit clouds follow the weather's own cloud colours, the back-scatter floor (local extinction x {FLOOR_LENGTH:.0} units, at most {FLOOR_MAX}), the sky from above and the ground from below through the column (fall 1 / (1 + {AMBIENT_FALL:.3} depth x OCCLUDE)), the powder darkening of thin cloud seen away from the light (POWDER), the whole times one level (LEVEL), by night the direct light and the light from below dimmed toward the cloud layers' luminance where theirs is higher (NIGHTPULL {:.2}), every colour bounded to {MAX_LIGHT:.0}",
            self.sun_colour_share(),
            self.night_pull()
        );
        let aids = self.text();
        if aids.is_empty() { model } else { format!("{model}; weights: {aids}") }
    }
}

/// `SUNCOLOUR` in a tune file or its start variable: a finite number held to 0..1; `None` otherwise.
pub fn try_share(v: &str) -> Option<f32> {
    v.trim().parse::<f32>().ok().filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 1.0))
}

/// `MOONCAP` in a tune file or its start variable: a finite number held to 0..[`MAX_MOON_CAP`]; `None` otherwise.
#[cfg_attr(not(any(test, debug_assertions, feature = "dev-aids")), allow(dead_code))]
pub fn try_moon_cap(v: &str) -> Option<f32> {
    v.trim().parse::<f32>().ok().filter(|v| v.is_finite()).map(|v| v.clamp(0.0, MAX_MOON_CAP))
}

/// `NIGHTPULL` in a tune file or its start variable: a finite number held to 0..[`MAX_NIGHT_PULL`]; `None` otherwise.
pub fn try_night_pull(v: &str) -> Option<f32> {
    v.trim().parse::<f32>().ok().filter(|v| v.is_finite()).map(|v| v.clamp(0.0, MAX_NIGHT_PULL))
}

/// A weight's start variable: a finite number held to 0..[`MAX_WEIGHT`]; anything else 1.
#[cfg(test)] // the tests' reader; a run starts from the START values (stage 2i M3)
pub fn debug_weight(value: Option<&str>) -> f32 {
    value.and_then(try_weight).unwrap_or(1.0)
}

/// A weight in a tune file: a finite number held to 0..[`MAX_WEIGHT`]; `None` otherwise.
pub fn try_weight(v: &str) -> Option<f32> {
    v.trim().parse::<f32>().ok().filter(|v| v.is_finite()).map(|v| v.clamp(0.0, MAX_WEIGHT))
}

/// The light's aids from the start variables (`get` reads one by name; `SUNCOLOUR`'s alias
/// `SUNCOLOR` when the first is not set), each [`START_LIGHT`]'s when unset or unreadable.
pub fn start_light_aids(get: impl Fn(&str) -> Option<String>) -> LightAids {
    let sun_colour = get(SUN_COLOUR_VAR).or_else(|| get(SUN_COLOR_VAR));
    let weight = |var: &str| get(var).as_deref().and_then(try_weight);
    let mut aids = LightAids {
        sun_colour: sun_colour.as_deref().and_then(try_share).unwrap_or(START_LIGHT.sun_colour),
        night_pull: get(NIGHT_PULL_VAR).as_deref().and_then(try_night_pull).unwrap_or(START_LIGHT.night_pull),
        moon_forward: weight(MOON_FORWARD_VAR).or(START_LIGHT.moon_forward),
        moon_back: weight(MOON_BACK_VAR).or(START_LIGHT.moon_back),
        moon_cap: get(MOON_CAP_VAR).as_deref().and_then(try_moon_cap).or(START_LIGHT.moon_cap),
        ..START_LIGHT
    };
    for w in LightWeight::ALL {
        aids.weights[w.index()] = get(w.var()).as_deref().and_then(try_weight).unwrap_or(START_LIGHT.weights[w.index()]);
    }
    aids
}

/// The start light (#3032, the look judged by eye on 2026-11-23): the weights at their designed 1 but for
/// BACKSCATTER, SUN and LEVEL; SUNCOLOUR 0 (the weather's cloud hue at its sunlight luminance); the night's values as designed. A start
/// variable or a live tune line overrides each value; removing a tune line restores its start value.
/// [`LightAids::default`] carries the designed weights for tests.
pub const START_LIGHT: LightAids = LightAids { weights: [1.0, 1.65, 1.0, 1.0, 1.0, 1.0, 1.30, 1.12, 1.0, 1.0], sun_colour: 0.0, night_pull: NIGHT_PULL, moon_forward: MOON_FORWARD, moon_back: MOON_BACK, moon_cap: MOON_CAP };

/// The sky's colours and the sun's elevation the two ambients take (live sky state, linear RGB;
/// sky.rs `SkyState::color`, `sun_elevation`), blended across a weather fade as the sky blends them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SkyLight {
    /// SkyUpper and Horizon (the light from above).
    pub upper: [f32; 3],
    pub horizon: [f32; 3],
    /// The weather's Ambient (the ground's sky light, the light from below).
    pub ambient: [f32; 3],
    /// The sun's signed elevation (degrees; negative at night).
    pub elevation: f32,
}

/// The light's colours and block of the uniform (volumetric_clouds.rs `uniform`): every incident
/// colour bounded together by one factor ([`light_bound`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lighting {
    pub light: Vec3,
    /// The cloud colour's ambient ([`AMBIENT_GAIN`]) under the bound: a term of the bound only.
    #[cfg_attr(not(test), allow(dead_code))]
    pub ambient: Vec3,
    pub lmode: Vec4,
    pub lupper: Vec4,
    pub llower: Vec4,
    pub loct: Vec4,
    pub lfloor: Vec4,
    pub lpowder: Vec4,
    /// The shade's colour ([`shade_colour`]) under the bound: a term of the bound only.
    #[cfg_attr(not(test), allow(dead_code))]
    pub shade: Vec3,
    /// The common factor the bound applied (1 where it did not engage), in f64 as it was computed:
    /// under a mod's huge sky level it can be far under f32's least (8e-73 for a sky level of 3e38
    /// with every aid at its most), which an f32 would flush to 0.
    #[cfg_attr(not(test), allow(dead_code))]
    pub bound: f64,
}

fn nn(x: f32) -> f32 {
    if x.is_finite() { x.max(0.0) } else { 0.0 }
}

/// A colour times a gain, exact in f64 (two f32 factors' product is exact there; rounded once to
/// f32 it is the f32 product, so the legacy colours stay the bits they were).
fn gained(c: [f32; 3], k: f64) -> DVec3 {
    DVec3::new(f64::from(nn(c[0])), f64::from(nn(c[1])), f64::from(nn(c[2]))) * k
}

/// The second octave's weight for its aid (the weights `b^i` normalised by their sum,
/// the aid scaling the second's relative weight first, so 0 is the first octave alone).
pub fn octave_weight(aid: f32) -> f32 {
    let r = OCTAVE_WEIGHT * nn(aid);
    r / (1.0 + r)
}

/// A linear colour's Rec. 709 luminance ([`LUMA`]).
pub fn luminance(c: DVec3) -> f64 {
    c.dot(DVec3::from_array(LUMA))
}

/// The model's direct light colour before its gains (the `SUNCOLOUR` aid, [`SUN_COLOUR_SHARE`]):
/// `cloud x (L(sun) / L(cloud)) x (1 - share) + sun x share`, L the Rec. 709 luminance, so the
/// colour keeps the sunlight's luminance at every share (both ends have it) and moves only its hue,
/// from the cloud colour's (0) to the sunlight's (1); share 1 is the sunlight colour to the bit. A
/// cloud colour under [`MIN_CLOUD_LUMA`] (no visible layer) leaves the sunlight colour. Inputs as
/// [`gained`] takes them (not-a-number and negative channels 0); in f64, so nothing overflows.
pub fn direct_colour(sunlight: [f32; 3], cloud: [f32; 3], share: f32) -> DVec3 {
    let sun = gained(sunlight, 1.0);
    let c = gained(cloud, 1.0);
    let lc = luminance(c);
    if lc.is_nan() || lc <= MIN_CLOUD_LUMA {
        return sun;
    }
    let s = if share.is_finite() { f64::from(share.clamp(0.0, 1.0)) } else { f64::from(SUN_COLOUR_SHARE) };
    c * (luminance(sun) / lc * (1.0 - s)) + sun * s
}

/// The sun's daylight for the ground: its elevation's sine times the direct light's
/// fade over the first degrees, 0 with the sun under the horizon (never the moon).
pub fn daylight(elevation_deg: f32) -> f32 {
    if !elevation_deg.is_finite() || elevation_deg <= 0.0 {
        return 0.0;
    }
    let t = (elevation_deg / DAYLIGHT_FADE_DEG).clamp(0.0, 1.0);
    elevation_deg.to_radians().sin().clamp(0.0, 1.0) * t * t * (3.0 - 2.0 * t)
}

/// The night's share for the clouds by moonlight: `None` by day, at the horizon and for an elevation that is
/// not a number (nothing of the night is then computed at all); below it from 0 at the horizon to 1 from
/// [`crate::volumetric_clouds::LIGHT_FADE_DEG`] on, smooth: the direct light is 0 at the horizon and the light
/// from below is undimmed there, so nothing steps. The elevation is the stepped sky hour's, as `direct_light`'s.
pub fn night_share(elevation_deg: f32) -> Option<f32> {
    (elevation_deg.is_finite() && elevation_deg < 0.0).then(|| smooth(0.0, crate::volumetric_clouds::LIGHT_FADE_DEG, -elevation_deg))
}

/// What the night keeps of a light of luminance `source` with the cloud layers' at `cloud`:
/// `min(1, cloud / source)^q` ([`NIGHT_PULL`]). 1 with `cloud` at or under [`MIN_CLOUD_LUMA`] (no visible cloud
/// colour to follow), growing in up to [`NIGHT_LUMA_RAMP`] times it; 1 for anything that is not a number and
/// for a black source. In 0..=1, so it only ever dims.
pub fn night_gain(source: f64, cloud: f64, q: f64) -> f64 {
    if source.is_nan() || cloud.is_nan() || q.is_nan() || q <= 0.0 || cloud <= MIN_CLOUD_LUMA || source <= cloud {
        return 1.0;
    }
    let t = ((cloud - MIN_CLOUD_LUMA) / (MIN_CLOUD_LUMA * (NIGHT_LUMA_RAMP - 1.0))).clamp(0.0, 1.0);
    1.0 - (1.0 - (cloud / source).powf(q)) * (t * t * (3.0 - 2.0 * t))
}

/// The incident light from above, unbounded (before its hemisphere weight): [`UPPER_GAIN`] times the sky
/// level times the sky's colours (SkyUpper and Horizon mixed by [`SKY_UPPER_SHARE`]).
pub fn upper_light(i: &VolumeInputs) -> DVec3 {
    let s = f64::from(SKY_UPPER_SHARE);
    (gained(i.sky.upper, s) + gained(i.sky.horizon, 1.0 - s)) * (f64::from(nn(i.sky_scale)) * f64::from(UPPER_GAIN))
}

/// The incident light from below, unbounded (before its hemisphere weight): the ground's
/// reflectance (times its aid) times the sky level times the weather's Ambient plus its sunlight
/// by the sun's daylight and what the cloud lets through (`1 - target_opacity(amount)`).
pub fn lower_light(i: &VolumeInputs) -> DVec3 {
    let k = f64::from(nn(i.sky_scale)) * f64::from(GROUND_REFLECTANCE) * f64::from(i.light_aids.weight(LightWeight::Ground));
    let through = f64::from(1.0 - target_opacity(i.amount)).clamp(0.0, 1.0);
    (gained(i.sky.ambient, 1.0) + gained(i.sunlight, f64::from(daylight(i.sky.elevation)) * through)) * k
}

/// The sky's own colour (as [`upper_light`] mixes it) scaled to the Rec. 709 luminance of `upper` (the light
/// from above after its aids): a term of the bound ([`light_bound`]). A sky too dark to have a hue
/// ([`MIN_CLOUD_LUMA`]) leaves `upper`. In f64.
pub fn shade_colour(i: &VolumeInputs, upper: DVec3) -> DVec3 {
    let s = f64::from(SKY_UPPER_SHARE);
    let sky = gained(i.sky.upper, s) + gained(i.sky.horizon, 1.0 - s);
    let ls = luminance(sky);
    if ls.is_nan() || ls <= MIN_CLOUD_LUMA {
        return upper;
    }
    sky * (luminance(upper) / ls)
}

/// The bound: the most a step can emit in a channel is the two weighted ambients (`upper` the
/// larger, channel by channel, of the light from above and the shade's colour) plus the direct light
/// (`direct`, the colour [`direct_colour`] mixed, whose worst channel can pass the sunlight's) times
/// [`PHASE_CAP`] plus `silver`, times the `SUN` aid (`sun`): the octaves' weights sum to 1, the floor and the
/// forward aid stay under the cap, the depth responses under 1. The light's strength counts as 1 (its most),
/// so the factor does not follow the hour. The sunlight's own light and the cloud colour's ambient (`light`,
/// `ambient`) bound it as well. The larger sets one factor `min(1, MAX_LIGHT / B)` for every colour (computed
/// in f64: nothing overflows).
#[allow(clippy::too_many_arguments)]
pub fn light_bound(light: DVec3, direct: DVec3, ambient: DVec3, upper: DVec3, lower: DVec3, sun: f64, silver: f64, cap: f32) -> f64 {
    let cap = f64::from(cap);
    let legacy = (ambient + light * cap).max_element();
    let model = (upper + lower + direct * ((cap + silver) * sun)).max_element();
    let most = legacy.max(model);
    if most.is_finite() && most > f64::from(MAX_LIGHT) { f64::from(MAX_LIGHT) / most } else { 1.0 }
}

/// The light's colours and block for the uniform's inputs: the weather's alone ([design]: the
/// shading follows the weather and the game data). The direct light's hue is the weather's authored cloud
/// colour (the visible layers' PNAM mean) at the sunlight's luminance, moved toward the WTHR sunlight colour
/// by SUNCOLOUR ([`direct_colour`]); the light from above is the weather's sky colours, from below the ground
/// bounce mixed with skylight at the same luminance, both at [`LEAN_AMBIENT`] of the hemisphere's gain; the
/// back-scatter floor's most is [`LEAN_FLOOR`] of the designed one. Never a colour constant.
///
/// The `LEVEL` aid multiplies the direct colour and the light from above and below before the bound, so the
/// bound holds whatever the level; by night the direct colour and the light from below are then dimmed, each by
/// its own gain ([`night_gain`]; not computed by day), after the bound was taken of the undimmed ones, so dimming
/// one never lifts another; `OCCLUDE` multiplies the ambient's fall (`lmode.z`); `POWDER` fills
/// `lpowder.x`.
pub fn lighting(i: &VolumeInputs) -> Lighting {
    let a = i.light_aids;
    let scale = nn(i.sky_scale);
    let level = f64::from(a.weight(LightWeight::Level));
    // The legacy light's and ambient's terms of the bound.
    let light = gained(i.sunlight, f64::from(scale * SUN_GAIN));
    let ambient = gained(i.ambient, f64::from(scale * AMBIENT_GAIN));
    let elevation = i.sky.elevation;
    let low_sun = if elevation.is_finite() { smooth(0.0, 5.0, elevation) * (1.0 - smooth(5.0, 25.0, elevation)) } else { 0.0 };
    let share = a.sun_colour_share();
    // Keep the endpoints of SUNCOLOUR and the daytime setting. Only the direct term warms and
    // brightens slightly as the sun approaches the horizon. Once the moon takes over, its
    // direct light has the same strength relative to sunlight as on the surrounding landscape.
    let sun_share = share / (1.0 - 0.67 * low_sun * (1.0 - share));
    let moon_ratio = crate::sky::MOON_SHARE / crate::sky::SUN_SHARE;
    // MoonlightNVSE.ini's factor on all of the light from below the horizon on, as the landscape's
    // (sky.rs `aim_sun`; the light's strength is 0 at the horizon): 0 extra without the file.
    let moon_ini = if elevation.is_finite() && elevation < 0.0 { 1.0 + i.moon_extra } else { 1.0 };
    let moon_gain = if elevation.is_finite() { 1.0 - (1.0 - moon_ratio) * smooth(0.0, crate::volumetric_clouds::LIGHT_FADE_DEG, -elevation) } else { 1.0 };
    let mixed = direct_colour(i.sunlight, i.ambient, sun_share);
    let mut direct = mixed * f64::from(scale * SUN_GAIN) * level * f64::from((1.0 + 0.15 * low_sun) * moon_gain * moon_ini);
    let lean_gain = f64::from(LEAN_AMBIENT);
    let sky = upper_light(i);
    let upper = sky * f64::from(HEMISPHERE * a.weight(LightWeight::Upper)) * level * lean_gain;
    // [hypothesis] The lower face receives diffuse skylight from the open sides of
    // the bank as well as reflected light from below. Preserve the weather's bounce luminance.
    let mut bounce = lower_light(i);
    let y = luminance(sky);
    if y > MIN_CLOUD_LUMA {
        bounce = bounce.lerp(sky * (luminance(bounce) / y), 0.35);
    }
    let mut lower = bounce * f64::from(HEMISPHERE * a.weight(LightWeight::Lower)) * level * lean_gain;
    let shade = shade_colour(i, upper);
    let night = night_share(elevation);
    // The bound takes the larger of the day's cap and the night's: a deck's scattered light is added past the
    // cap, so a small night cap must not loosen it.
    let cap = a.phase_cap(night).max(PHASE_CAP);
    let k = light_bound(light, direct, ambient, upper.max(shade), lower, f64::from(a.weight(LightWeight::Sun)), 0.0, cap);
    // By night: each light toward the cloud layers' luminance, in the weather's own units (before the sky
    // level and the gains; MOON_SHARE and MoonlightNVSE.ini's factor stay whole, on top).
    if let Some(n) = night {
        let q = f64::from(n) * f64::from(a.night_pull());
        let cloud = luminance(gained(i.ambient, 1.0));
        direct *= night_gain(luminance(mixed), cloud, q);
        if scale > 0.0 {
            lower *= night_gain(luminance(bounce) / f64::from(scale), cloud, q);
        }
    }
    let f = |v: DVec3| (v * k).as_vec3();
    let amount = if i.amount.is_finite() { i.amount.clamp(0.0, 1.0) } else { 0.0 };
    let column = table_depth_of(&EXTINCTION_TABLE, amount) * nn(i.density) * MEAN_SHAPE;
    let back = a.phase_weight(LightWeight::Backscatter, night);
    Lighting {
        light: f(direct),
        ambient: f(ambient),
        lmode: Vec4::new(0.0, column, AMBIENT_FALL * a.weight(LightWeight::Occlude), a.weight(LightWeight::Sun)),
        lupper: f(upper).extend(0.0),
        llower: f(lower).extend(0.0),
        loct: Vec4::new(octave_weight(a.weight(LightWeight::Octave)), OCTAVE_DEPTH, OCTAVE_PHASE, a.phase_weight(LightWeight::Forward, night)),
        lfloor: Vec4::new(FLOOR_LENGTH * back, FLOOR_MAX * back * LEAN_FLOOR, MEASURED_FROM, MEASURED_TO),
        lpowder: Vec4::new((POWDER_SHARE * a.weight(LightWeight::Powder)).min(1.0), 0.0, 0.0, 0.0),
        shade: f(shade),
        bound: k,
    }
}

/// What the shading takes of the march's uniform, the same for every sample of a ray (march
/// `VcShading`): the debug view in force, the uniform's `lmode`, the two weighted ambients, `loct`,
/// `lfloor` and the light's elevation sine.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shading {
    pub mode: u32,
    pub column: Vec4,
    pub upper: Vec3,
    pub lower: Vec3,
    pub octave: Vec4,
    pub back: Vec4,
    pub rise: f32,
}

/// A ray's angular part (march `VcRayLight`): the direct light's colour times its strength, the two
/// octaves' mixed phases before the floor and the cap, the cap, and the powder's share along the ray.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayLight {
    pub light: Vec3,
    pub p0: f32,
    pub p1: f32,
    pub cap: f32,
    pub powder: f32,
}

/// The debug view's code as the march reads it (march `vc_view_code`): 0 to 13 (5 reserved for
/// the cloud map's view, stage 2d; 6, 7, 8 the light's, [`CloudView::Sun`], [`CloudView::Ambient`],
/// [`CloudView::Depth`]).
pub fn view_code(u: &CloudVolumeUniform) -> u32 {
    (u.flags.z.clamp(0.0, 13.0) + 0.5) as u32
}

/// The depth view's scale (march `VC_DEPTH_WHITE_LOG2`, `VC_DEPTH_GREY`) [design]: a measured
/// depth's grey is `log2(1 + depth) / 6 x 0.9`, black at 0, each sixth a doubling of `1 + depth`
/// (depths 1, 3, 7, 15, 31), white (0.9, under the tonemapper inverse's saturation at 0.9875) from
/// 63 on: the typical columns (a few optical depths) fill the lower
/// half, the extremes (16 to 64) the upper.
pub const DEPTH_WHITE_LOG2: f32 = 6.0;
pub const DEPTH_GREY: f32 = 0.9;
/// The opacity over which the depth view shows a ray's mean opaque (march `VC_DEPTH_SHOWN`).
#[cfg_attr(not(test), allow(dead_code))]
pub const DEPTH_SHOWN: f32 = 0.01;

/// A sample's code in the depth view (march `vc_depth_code`): the grey of its measured depth,
/// magenta `(0.9, 0, 0.9)` where the march skipped the light (the gate).
#[cfg_attr(not(test), allow(dead_code))]
pub fn depth_code(depth: f32, sampled: bool) -> Vec3 {
    if !sampled {
        return Vec3::new(DEPTH_GREY, 0.0, DEPTH_GREY);
    }
    Vec3::splat(DEPTH_GREY * ((1.0 + depth.max(0.0)).log2() / DEPTH_WHITE_LOG2).clamp(0.0, 1.0))
}

/// The log text of the light's debug views (`NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_VIEW=sun|ambient|depth`);
/// empty for the others.
pub fn view_text(view: CloudView) -> &'static str {
    match view {
        CloudView::Sun => "the clouds lit by the direct light alone (the ambient zeroed): its two octaves with the back-scatter floor and the powder",
        CloudView::Ambient => "the clouds lit by the ambient alone (the direct light zeroed): the sky from above and the ground from below through the column",
        CloudView::Opacity => "camera-ray opacity 1 - transmittance: black empty sky through white opaque cloud",
        CloudView::BodyHeight => "opacity-weighted visible body height from the local base (black) to its crown (white)",
        CloudView::Depth => "the optical depth toward the light, the opacity-weighted mean per ray, opaque: grey log2(1 + depth) / 6 (black 0; 1, 3, 7, 15, 31 each a sixth more; white from 63), magenta where the light was not marched (the gate)",
        _ => "",
    }
}

/// The march's `vc_shading`: what the shading takes of the uniform.
#[cfg_attr(not(test), allow(dead_code))]
pub fn shading(u: &CloudVolumeUniform) -> Shading {
    Shading {
        mode: view_code(u),
        column: u.lmode,
        upper: u.lupper.truncate(),
        lower: u.llower.truncate(),
        octave: u.loct,
        back: u.lfloor,
        rise: u.light_dir.y,
    }
}

/// The powder's share along a ray at `cos` (march `vc_powder_share`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn powder_share(cos: f32, share: f32) -> f32 {
    share.clamp(0.0, 1.0) * (0.5 - 0.5 * cos).clamp(0.0, 1.0)
}

/// What the powder term keeps of the direct light at a local extinction `ext` per unit (march
/// `vc_powder`): `1 - share e^(-2 ext POWDER_LENGTH)`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn powder(ext: f32, share: f32) -> f32 {
    1.0 - share * (-2.0 * ext.max(0.0) * POWDER_LENGTH).exp()
}

/// The ray's angular part for a ray along the unit `dir` (march `vc_ray_light_of`,
/// `vc_ray_light`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn ray_light(dir: Vec3, u: &CloudVolumeUniform) -> RayLight {
    let cos = dir.dot(u.light_dir.truncate());
    let fw = u.loct.w * (1.0 - u.phase.z);
    RayLight {
        light: u.light.truncate() * u.light_dir.w,
        p0: octave_phase(cos, u.phase, fw, 1.0),
        p1: octave_phase(cos, u.phase, fw, u.loct.z),
        cap: u.phase.w,
        powder: powder_share(cos, u.lpowder.x),
    }
}

/// An octave's two lobes mixed, not capped (march `vc_octave_phase`): the forward lobe's weight
/// `fw`, the back lobe's `ph.z`, both g times `c`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn octave_phase(cos: f32, ph: Vec4, fw: f32, c: f32) -> f32 {
    fw * hg(cos, ph.x * c) + ph.z * hg(cos, ph.y * c)
}

/// The back-scatter floor at a local extinction `ext` per unit (march `vc_floor`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn floor_at(ext: f32, s: &Shading) -> f32 {
    (ext.max(0.0) * s.back.x).min(s.back.y)
}

/// The share of hemispheric light crossing a column of optical depth `t` (march `vc_through`).
#[cfg_attr(not(test), allow(dead_code))]
pub fn through(t: f32, fall: f32) -> f32 {
    1.0 / (1.0 + fall * t.max(0.0))
}

/// WGSL's `smoothstep` (the march's).
#[cfg_attr(not(test), allow(dead_code))]
fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The column of cloud above a sample the light from above crosses (leaf `vc_lean_shade`'s `above`): the
/// vertical estimate `K m (1 - h)`, toward the measured depth times the light's elevation sine for
/// a measured sample under a high light.
#[cfg_attr(not(test), allow(dead_code))]
pub fn column_above(d: Vec4, depth: f32, sampled: bool, s: &Shading) -> f32 {
    let h = d.w.clamp(0.0, 1.0);
    let vertical = s.column.y * (1.0 - h);
    if !sampled {
        return vertical;
    }
    let m = smooth(s.back.z, s.back.w, s.rise);
    vertical + (depth.max(0.0) * s.rise.max(0.0) - vertical) * m
}

/// The share of the lean light's scattered octave that follows the smooth path estimate in fair weather
/// (leaf `VC_LEAN_BROAD`) [design].
#[cfg_attr(not(test), allow(dead_code))]
pub const LEAN_BROAD: f32 = 0.8;
/// The light from below a fair cloud's face receives, against the deck's (leaf `VC_LEAN_FAIR_LOWER`) [design].
#[cfg_attr(not(test), allow(dead_code))]
pub const LEAN_FAIR_LOWER: f32 = 0.7;

/// The lean model's light (march `vc_lean_shade`): the normalised two octaves with the
/// extinction's floor and the powder in the direct light, the light from above and below through the
/// columns, and sunlight scattered across a connected storm layer of optical depth `sheet_depth`.
/// `open_flank` is the exposed fraction of a resolved fair-weather bank.
/// The debug views by `s.mode`: 6 the direct term alone, 7 the ambient alone, 8 the depth's code.
// The CPU mirror keeps the shader function's arguments one for one (`vc_lean_shade`).
#[allow(clippy::too_many_arguments)]
#[cfg_attr(not(test), allow(dead_code))]
pub fn lean_shade(d: Vec4, altitude: f32, depth: f32, sampled: bool, ray: &RayLight, s: &Shading, crown_depth: f32, foot: f32, sheet_depth: f32, open_flank: f32) -> Vec3 {
    if s.mode == 8 {
        return depth_code(depth, sampled);
    }
    let ext = d.x * d.y;
    let h = d.w.clamp(0.0, 1.0);
    let measured = if sampled { depth.max(0.0) } else { 0.0 };
    let high_sun = smooth(0.18, 0.60, s.rise);
    let deck = smooth(0.55, 0.85, d.z);
    let fair = 1.0 - smooth(0.55, 0.73, d.z);
    let fair_face = smooth(450.0, 1300.0, altitude);
    let face = fair_face + (smooth(0.22, 0.52, h) - fair_face) * deck;
    let base = (1.0 - face) * d.x.clamp(0.0, 1.0);
    let lid = s.column.y * base * high_sun;
    let bulk = s.column.y * (1.0 - h);
    let cache = measured.max(lid).max(crown_depth);
    let t = (1.0 - foot) * cache + foot * bulk / s.rise.max(0.25);
    let shoulder = high_sun * face;
    let cap_depth = 0.15 + 0.35 * s.column.y * (1.0 - h);
    let fill_depth = (t + (t.min(cap_depth) - t) * shoulder).max((1.0 - foot) * crown_depth);
    let broad = lid.max((1.0 - foot) * crown_depth);
    let scattered_path = fill_depth + (broad - fill_depth) * (LEAN_BROAD * fair);
    let (v0, v1) = ((-t).exp(), (-s.octave.y * scattered_path).exp());
    let sky = column_above(d, depth, sampled, s).max(lid).max(crown_depth * s.rise.max(0.0));
    let above = (1.0 - foot) * sky + foot * bulk;
    let f = floor_at(ext, s);
    let retained = 1.0 - open_flank * high_sun * (1.0 - smooth(0.25, 0.65, h));
    let q0 = ray.p0.max(f * retained).min(ray.cap);
    let q1 = ray.p1.max(f).max(shoulder).min(ray.cap);
    let phase = (1.0 - s.octave.x) * q0 * v0 + retained * retained * s.octave.x * q1 * v1;
    let mut source = powder(ext, ray.powder) * phase;
    if sheet_depth > 0.0 {
        let covered = smooth(0.55, 0.73, d.z) * (1.0 - (-sheet_depth / s.rise.max(0.25)).exp());
        let path = sheet_depth + bulk + 0.6 * (sheet_depth * (1.0 - h) - bulk);
        let scattered = s.octave.x * s.rise.max(0.0) * through(s.octave.y * path, 1.0);
        source += covered * (scattered - source);
    }
    let direct = ray.light * (s.column.w * source);
    let lower = s.lower * (1.0 + (LEAN_FAIR_LOWER - 1.0) * fair);
    let ambient = s.upper * through(above, s.column.z) + lower * through(s.column.y * h, s.column.z);
    let ambient = if s.mode == 6 { Vec3::ZERO } else { ambient };
    let direct = if s.mode == 7 { Vec3::ZERO } else { direct };
    ambient + direct
}

/// The march module with its one import line (the light's leaf) replaced by the leaf's text, as
/// plain WGSL for naga: the two test assemblers (volumetric_clouds.rs `the_march_module_is_valid_wgsl`,
/// medium.rs `plain_trace_wgsl`) use it. The import must name the leaf by its path and only items
/// the leaf declares. Only a run of the game proves the WESL import's resolution itself.
#[cfg(test)]
pub(crate) fn plain_march_wgsl() -> String {
    let march = include_str!("volumetric_clouds_march.wesl").replace("\r\n", "\n");
    let leaf = include_str!("volumetric_clouds_light.wesl").replace("\r\n", "\n");
    let mut out = String::new();
    let mut imports = 0;
    for line in march.lines() {
        if let Some(rest) = line.strip_prefix("import ") {
            imports += 1;
            let items = rest
                .strip_prefix("nvforever::volumetric_clouds_light::{")
                .and_then(|r| r.strip_suffix("};"))
                .unwrap_or_else(|| panic!("an import the assembler does not resolve: {line}"));
            for item in items.split(',').map(str::trim) {
                assert!(
                    leaf.contains(&format!("\nfn {item}(")) || leaf.contains(&format!("\nstruct {item} {{")) || leaf.contains(&format!("\nconst {item}:")),
                    "the march imports `{item}`, which the leaf does not declare"
                );
            }
            out.push_str(&leaf);
            out.push('\n');
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    assert_eq!(imports, 1, "the march imports the light's leaf and nothing else");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volumetric_clouds::{uniform, VolumeInputs};

    fn leaf() -> String {
        include_str!("volumetric_clouds_light.wesl").replace("\r\n", "\n")
    }

    /// Independent PNAM, sun, zenith, horizon and ground colours exercise both endpoints
    /// of the direct mix and the indirect hue through the actual lean leaf.
    #[test]
    fn the_lean_light_takes_the_weathers_colours() {
        let sun = [1.0f32, 0.45, 0.2];
        let sky = SkyLight { upper: [0.15, 0.35, 1.0], horizon: [0.3, 0.6, 0.9], ambient: [0.2, 0.9, 0.3], elevation: 40.0 };
        let pnam = [0.9f32, 0.9, 0.1];
        let hue = |c: Vec3| c / c.x.max(c.y).max(c.z).max(1e-12);
        let same_hue = |a: Vec3, b: Vec3, what: &str| assert!((hue(a) - hue(b)).abs().max_element() < 1e-4, "{what}: {a} against {b}");
        let inputs = |aids: LightAids| VolumeInputs {
            sunlight: sun,
            ambient: pnam,
            sky,
            sky_scale: 1.0,
            amount: 0.3,
            density: 1.0,
            light_strength: 1.0,
            light_dir: Vec3::new(0.3, 0.64, 0.2),
            light_aids: aids,
            ..Default::default()
        };
        let sky_mix = Vec3::from_array(sky.upper) * SKY_UPPER_SHARE + Vec3::from_array(sky.horizon) * (1.0 - SKY_UPPER_SHARE);
        for aids in [LightAids::default(), START_LIGHT, LightAids { sun_colour: 0.25, ..START_LIGHT }, LightAids { sun_colour: 1.0, ..START_LIGHT }] {
            let i = inputs(aids);
            let l = lighting(&i);
            // The direct light: the cloud colour's hue at SUNCOLOUR 0, the sunlight's at 1, the mix
            // between (the low sun's lift aside: 40 degrees here).
            same_hue(l.light, direct_colour(sun, pnam, aids.sun_colour_share()).as_vec3(), "direct: the weather's cloud and sunlight colours by SUNCOLOUR");
            if aids.sun_colour_share() == 0.0 {
                same_hue(l.light, Vec3::from_array(pnam), "SUNCOLOUR 0: the cloud colour's hue");
            }
            same_hue(l.lupper.truncate(), sky_mix, "from above: the sky's colours");
            let bounce = lower_light(&i);
            let skylight = sky_mix.as_dvec3() * (luminance(bounce) / luminance(sky_mix.as_dvec3()));
            same_hue(l.llower.truncate(), (0.65 * bounce + 0.35 * skylight).as_vec3(), "lower faces receive weather bounce and skylight");
            // The start levels: the ambient at LEAN_AMBIENT of the hemisphere's gain and the floor at LEAN_FLOOR of
            // the designed one (before the bound, which these inputs do not reach).
            assert_eq!(l.bound, 1.0);
            let designed = f64::from(HEMISPHERE * aids.weight(LightWeight::Lower) * aids.weight(LightWeight::Level));
            assert!((luminance(l.llower.truncate().as_dvec3()) - luminance(bounce) * designed * f64::from(LEAN_AMBIENT)).abs() < 1e-5);
            assert!((l.lfloor.y - FLOOR_MAX * aids.weight(LightWeight::Backscatter) * LEAN_FLOOR).abs() < 1e-6);
            // The leaf's mirror on one lit, partly shadowed sample.
            let mut u = uniform(&i);
            u.light_dir.w = 1.0;
            let ray = ray_light(Vec3::new(0.0, 1.0, 0.0), &u);
            let d = Vec4::new(0.6, 3.0e-4, 0.3, 0.4);
            let mut s = shading(&u);
            s.mode = 6;
            let direct = lean_shade(d, 900.0, 1.5, true, &ray, &s, 0.0, 0.0, 0.0, 0.0);
            assert!(direct.max_element() > 0.0);
            same_hue(direct, direct_colour(sun, pnam, aids.sun_colour_share()).as_vec3(), "the direct view follows SUNCOLOUR");
            s.mode = 7;
            let ambient = lean_shade(d, 900.0, 1.5, true, &ray, &s, 0.0, 0.0, 0.0, 0.0);
            let lid = s.column.y * d.x * (1.0 - smooth(450.0, 1300.0, 900.0)) * smooth(0.18, 0.60, s.rise);
            let lower = s.lower * LEAN_FAIR_LOWER;
            let want = s.upper * through(column_above(d, 1.5, true, &s).max(lid), s.column.z) + lower * through(s.column.y * d.w, s.column.z);
            assert!((ambient - want).abs().max_element() < 1e-6, "{ambient} against {want}");
            assert!((hue(ambient) - hue(direct)).abs().max_element() > 0.05, "distinct sun and sky hues at the same opacity: {ambient} {direct}");
            s.mode = 0;
            assert!((lean_shade(d, 900.0, 1.5, true, &ray, &s, 0.0, 0.0, 0.0, 0.0) - (direct + ambient)).abs().max_element() < 1e-5);
        }
        // Isolate height shading with identical measured light depth, opacity and hue.
        // At noon a thick base under a full column is dark. At low sun the measured
        // horizontal path controls the direct light, including thin backlit edges.
        let u = uniform(&inputs(LightAids::default()));
        let ray = ray_light(Vec3::Y, &u);
        let mut s = shading(&u);
        s.mode = 6;
        s.column.y = 6.0;
        s.rise = 0.9;
        let foot = Vec4::new(1.0, 0.0025, 0.3, 0.16);
        let crown = Vec4::new(foot.x, foot.y, foot.z, 0.70);
        let high = lean_shade(crown, 1800.0, 0.15, true, &ray, &s, 0.0, 0.0, 0.0, 0.0);
        let sheltered = lean_shade(crown, 1800.0, 0.15, true, &ray, &s, 2.0, 0.0, 0.0, 0.0);
        assert!(sheltered.max_element() < high.max_element(), "a neighbouring head must shadow the crown fill");
        assert!(high.max_element() > 0.0);
        assert!(lean_shade(foot, 350.0, 0.15, true, &ray, &s, 0.0, 0.0, 0.0, 0.0).max_element() < 0.5 * high.max_element());
        // Cache variations must not emboss either light component on an opaque foot.
        for mode in [6, 7] {
            s.mode = mode;
            let base = lean_shade(foot, 350.0, 0.0, false, &ray, &s, 0.0, 1.0, 0.0, 0.0);
            assert_eq!(base, lean_shade(foot, 350.0, 12.0, true, &ray, &s, 3.0, 1.0, 0.0, 0.0));
        }
        s.mode = 6;
        s.rise = 0.10;
        assert_eq!(lean_shade(foot, 350.0, 0.15, true, &ray, &s, 0.0, 0.0, 0.0, 0.0), lean_shade(crown, 1800.0, 0.15, true, &ray, &s, 0.0, 0.0, 0.0, 0.0));
        // A sky bright enough engages the bound.
        assert!(lighting(&VolumeInputs { sky_scale: 4.0e4, ..inputs(LightAids::default()) }).bound < 1.0);
        // The leaf's text is this mirror's.
        let leaf = leaf();
        for line in [
            "fn vc_lean_shade(d: vec4<f32>, altitude: f32, depth: f32, sampled: bool, ray: VcRayLight, s: VcShading, crown_depth: f32, foot: f32, sheet_depth: f32, open_flank: f32) -> vec3<f32> {",
            "let t = mix(max(max(measured, lid), crown_depth), bulk / max(s.rise, 0.25), foot);",
            "let face = mix(smoothstep(450.0, 1300.0, altitude), smoothstep(0.22, 0.52, h), deck);",
            "let fill_depth = max((1.0 - foot) * crown_depth, mix(t, min(t, cap_depth), shoulder));",
            "const VC_LEAN_BROAD: f32 = 0.8;",
            "let fair = 1.0 - smoothstep(0.55, 0.73, d.z);",
            "let broad = max((1.0 - foot) * crown_depth, lid);",
            "let v1 = exp(-s.octave.y * mix(fill_depth, broad, VC_LEAN_BROAD * fair));",
            "let q1 = min(max(max(ray.p1, f), shoulder), ray.cap);",
            "above = mix(max(above, max(lid, crown_depth * max(s.rise, 0.0))), bulk, foot);",
            "let phase = (1.0 - s.octave.x) * q0 * v0 + retained * retained * s.octave.x * q1 * v1;",
            "var source = vc_powder(ext, ray.powder) * phase;",
            "source = mix(source, scattered, covered);",
            "var direct = ray.light * (s.column.w * source);",
            "const VC_LEAN_FAIR_LOWER: f32 = 0.7;",
            "let lower = s.lower * mix(1.0, VC_LEAN_FAIR_LOWER, fair);",
            "var ambient = s.upper * vc_through(above, s.column.z) + lower * vc_through(s.column.y * h, s.column.z);",
        ] {
            assert!(leaf.contains(line), "the leaf no longer has `{line}`");
        }
        let lean = &leaf[leaf.find("fn vc_lean_shade(").unwrap()..];
        let lean = &lean[..lean.find("\n}\n").unwrap()];
        for term in ["silver", "backlit", "scatter_loss", "s.shade", "sky_open", "diffuse_sky"] {
            assert!(!lean.contains(term), "the lean light takes no `{term}`");
        }
        // The other terms of the leaf it takes.
        for line in [
            "    return clamp(share, 0.0, 1.0) * clamp(0.5 - 0.5 * cos_theta, 0.0, 1.0);",
            "const VC_POWDER_LENGTH: f32 = 2000.0;",
            "    return 1.0 - share * exp(-2.0 * max(ext, 0.0) * VC_POWDER_LENGTH);",
            "    return fw * vc_hg(cos_theta, ph.x * c) + ph.z * vc_hg(cos_theta, ph.y * c);",
            "    let fw = oct.w * (1.0 - ph.z);",
            "    return VcRayLight(light * strength, vc_octave_phase(cos_theta, ph, fw, 1.0), vc_octave_phase(cos_theta, ph, fw, oct.z), ph.w, vc_powder_share(cos_theta, powder));",
            "    return min(max(ext, 0.0) * s.back.x, s.back.y);",
            "    return 1.0 / (1.0 + fall * max(t, 0.0));",
        ] {
            assert!(leaf.contains(line), "the leaf no longer has `{line}`");
        }
        assert_eq!(POWDER_LENGTH, 2000.0, "the constant the leaf writes");
        let march = include_str!("volumetric_clouds_march.wesl").replace("\r\n", "\n");
        assert!(march.contains("return vc_lean_shade(d, above, depth, sampled, ray, ls, crown_depth, foot * (1.0 - 0.70 * lobes), sheet_depth, open_flank);"));
        let incident = &march[march.find("fn vc_incident(").unwrap()..];
        let incident = &incident[..incident.find("\n}\n").unwrap()];
        let (before, after) = incident.split_once("return vc_lean_shade(").unwrap();
        assert!(before.contains("if s.mode == 13u {") && before.contains("if s.mode == 10u {") && !after.contains("return"), "after the diagnostics, the only light");
    }

    /// Every aid at its most.
    fn most_aids() -> LightAids {
        LightAids { weights: [MAX_WEIGHT; LIGHT_WEIGHTS], sun_colour: 0.0, ..Default::default() }
    }

    /// The bound: below it the direct light is the mixed colour and the ambient the
    /// cloud colour's to the bit, nothing scaled; a mod's huge sky level with every aid at its most keeps what
    /// a step can emit under MAX_LIGHT, finite and non-negative; nothing at all is no division by zero.
    #[test]
    fn the_light_is_bounded_and_exact_below_the_bound() {
        let old = |c: [f32; 3], k: f32, most: f32| (Vec3::new(nn(c[0]), nn(c[1]), nn(c[2])) * k).min(Vec3::splat(most));
        let sky = SkyLight { upper: [0.2, 0.35, 0.8], horizon: [0.6, 0.6, 0.65], ambient: [0.3, 0.3, 0.35], elevation: 60.0 };
        for (sun, amb, scale) in [([1.0f32, 0.8, 0.6], [0.4f32, 0.5, 0.7], 1.3f32), ([3.0, 2.9, 2.5], [0.9, 0.9, 1.0], 7.0), ([0.0; 3], [0.0; 3], 0.0), ([0.123, 4.56, 0.0789], [1.0 / 3.0, 0.1, 2.0], 1.0)] {
            // With the sunlight's whole share in its colour (SUNCOLOUR 1) the sunlight's own bits; the default
            // mixes in the cloud colour.
            for sun_colour in [1.0, SUN_COLOUR_SHARE] {
                let i = VolumeInputs { sunlight: sun, ambient: amb, sky_scale: scale, sky, amount: 0.4, density: 1.0, light_aids: LightAids { sun_colour, ..Default::default() }, ..Default::default() };
                let l = lighting(&i);
                assert_eq!(l.bound, 1.0);
                if sun_colour == 1.0 {
                    assert_eq!(l.light, old(sun, scale * SUN_GAIN, MAX_LIGHT / PHASE_CAP), "the sunlight's bits");
                } else {
                    assert_eq!(l.light, (direct_colour(sun, amb, sun_colour) * f64::from(scale * SUN_GAIN)).as_vec3(), "the mixed colour");
                }
                assert_eq!(l.ambient, old(amb, scale * AMBIENT_GAIN, MAX_LIGHT), "the cloud colour's ambient's bits");
            }
        }
        for scale in [3e38f32, 1e6, 3_000.0] {
            let i = VolumeInputs { sunlight: [1.0, 1e30, 0.5], ambient: [2.0, 1.0, 1e29], sky_scale: scale, sky: SkyLight { upper: [1e30, 1.0, 1.0], horizon: [3e38; 3], ambient: [1e20; 3], elevation: 90.0 }, light_aids: most_aids(), amount: 0.0, density: 4.0, ..Default::default() };
            let l = lighting(&i);
            for v in [l.light, l.ambient, l.lupper.truncate(), l.llower.truncate(), l.shade] {
                assert!(v.is_finite() && v.min_element() >= 0.0, "{v}");
            }
            // The bound's own terms: the larger of the light from above and the shade's colour, the direct
            // light's cap times SUN, and the sunlight's and the cloud colour's.
            let legacy = (l.ambient + l.light * PHASE_CAP).max_element();
            let model = (l.lupper.truncate().max(l.shade) + l.llower.truncate() + l.light * (PHASE_CAP * l.lmode.w)).max_element();
            assert!(legacy <= MAX_LIGHT * 1.0001 && model <= MAX_LIGHT * 1.0001, "scale {scale}: legacy {legacy}, model {model}");
            assert!(l.bound > 0.0 && l.bound < 1.0, "scale {scale}: engaged, {}", l.bound);
            assert!(legacy.max(model) >= MAX_LIGHT * 0.999, "scale {scale}: the worst channel scaled to the bound, not under it: {legacy}, {model}");
            for v in [l.lmode, l.loct, l.lfloor, l.lpowder] {
                assert!(v.is_finite());
            }
        }
        let zero = lighting(&VolumeInputs::default());
        assert!(zero.bound == 1.0 && zero.light == Vec3::ZERO && zero.lupper == Vec4::ZERO && zero.llower == Vec4::ZERO);
        // The block's constants and aids.
        let l = lighting(&VolumeInputs { amount: 0.1, density: 1.0, ..Default::default() });
        assert_eq!(l.lmode, Vec4::new(0.0, crate::volumetric_clouds::table_depth(0.1) * MEAN_SHAPE, AMBIENT_FALL, 1.0));
        assert!((l.loct.x - 1.0 / 3.0).abs() < 1e-6 && l.loct.y == OCTAVE_DEPTH && l.loct.z == OCTAVE_PHASE && l.loct.w == 1.0);
        assert_eq!(l.lfloor, Vec4::new(FLOOR_LENGTH, FLOOR_MAX * LEAN_FLOOR, MEASURED_FROM, MEASURED_TO));
    }

    /// A model shading with the ambients and the floor off and the given second octave's weight.
    fn bare(w1: f32) -> Shading {
        Shading { mode: 0, column: Vec4::new(0.0, 0.0, 0.0, 1.0), upper: Vec3::ZERO, lower: Vec3::ZERO, octave: Vec4::new(w1, OCTAVE_DEPTH, OCTAVE_PHASE, 1.0), back: Vec4::new(0.0, 0.0, MEASURED_FROM, MEASURED_TO), rise: 1.0 }
    }

    /// A unit white light with both octaves' phases `p0`, `p1`.
    fn lit(p0: f32, p1: f32) -> RayLight {
        RayLight { light: Vec3::ONE, p0, p1, cap: PHASE_CAP, powder: 0.0 }
    }

    #[test]
    fn scattered_sunlight_escapes_from_exposed_lower_cumulus() {
        let mut s = bare(0.5);
        s.upper = Vec3::splat(0.4);
        let mut ray = lit(0.3, 1.0);
        ray.light = Vec3::new(1.0, 0.65, 0.30);
        let sample = |height, rise, flank, mode| {
            let d = Vec4::new(0.6, 0.001, 0.30, height);
            lean_shade(d, 350.0, 0.8, true, &ray, &Shading { rise, mode, ..s }, 0.0, 0.0, 0.0, flank)
        };
        let closed = sample(0.15, 0.8, 0.0, 6);
        let middle = sample(0.15, 0.8, 0.5, 6);
        let open = sample(0.15, 0.8, 1.0, 6);
        // The open flank retains the beam. Half the retention keeps a quarter of the diffuse term.
        assert!(open.x > 0.0 && closed.x > open.x);
        assert!((middle - (open + 0.25 * (closed - open))).abs().max_element() < 1e-6);
        assert!((middle / middle.x - ray.light).abs().max_element() < 1e-6);
        assert_eq!(sample(0.85, 0.8, 0.0, 6), sample(0.85, 0.8, 1.0, 6));
        assert_eq!(sample(0.15, 0.10, 0.0, 6), sample(0.15, 0.10, 1.0, 6));
        assert_eq!(sample(0.15, 0.8, 0.0, 7), sample(0.15, 0.8, 1.0, 7));
    }

    #[test]
    fn the_storm_layer_scatters_weather_sunlight_across_local_shadows() {
        let mut s = bare(1.0 / 3.0);
        s.column.y = 4.0;
        s.rise = 0.7;
        s.upper = Vec3::splat(0.5);
        let mut ray = lit(1.0, 1.0);
        ray.light = Vec3::new(1.0, 0.75, 0.4);
        let sample = |cover, depth, layer, mode| {
            let d = Vec4::new(0.6, 0.001, cover, 0.65);
            lean_shade(d, 2400.0, depth, true, &ray, &Shading { mode, ..s }, 0.0, 0.0, layer, 0.0)
        };
        let local_range = sample(0.73, 0.25, 0.0, 6) - sample(0.73, 5.0, 0.0, 6);
        // The fair faces' share of the light from below is gone where the sheet is whole.
        let below = |cover| {
            let lit = Shading { mode: 7, upper: Vec3::ZERO, lower: Vec3::ONE, ..s };
            lean_shade(Vec4::new(0.6, 0.001, cover, 0.65), 2400.0, 1.0, true, &ray, &lit, 0.0, 0.0, 0.0, 0.0).x
        };
        let whole = through(s.column.y * 0.65, s.column.z);
        assert!((below(0.73) - whole).abs() < 1e-6 && (below(0.3) - LEAN_FAIR_LOWER * whole).abs() < 1e-6);
        assert!(local_range.x > 0.4);
        for layer in [0.05_f32, 1.0, 4.0] {
            let high = sample(0.73, 0.25, layer, 6);
            let low = sample(0.73, 5.0, layer, 6);
            assert!(((high - low) - local_range * (-layer / s.rise).exp()).abs().max_element() < 1e-5);
            assert!((low / low.x - ray.light).abs().max_element() < 1e-5);
            assert_eq!(sample(0.55, 0.25, layer, 6), sample(0.55, 0.25, 0.0, 6));
            assert_eq!(sample(0.73, 5.0, layer, 7), sample(0.73, 5.0, 0.0, 7));
        }
        let range = |cover| sample(cover, 0.25, 1.0, 6).x - sample(cover, 5.0, 1.0, 6).x;
        assert!(range(0.55) > range(0.64) && range(0.64) > range(0.73));
    }

    /// The column above a sample: the vertical estimate `K m (1 - h)` while the light is unmeasured or low, the
    /// measured depth times the light's elevation sine under a high one, between them between; none at the top;
    /// and what crosses a column falls with it and stays in 0..1.
    #[test]
    fn the_column_above_follows_a_high_lights_measured_depth() {
        for k in [2.6f32, 5.7] {
            let s = Shading { column: Vec4::new(0.0, k * MEAN_SHAPE, AMBIENT_FALL, 1.0), ..bare(0.0) };
            let d = Vec4::new(0.5, 0.001, 0.3, 0.0);
            assert!((column_above(d, 0.8 * k, true, &s) - 0.8 * k).abs() < 1e-6);
            assert!((column_above(d, 0.8 * k, false, &s) - k * MEAN_SHAPE).abs() < 1e-6);
            let low_sun = Shading { rise: 0.3, ..s };
            assert!((column_above(d, 0.8 * k, true, &low_sun) - k * MEAN_SHAPE).abs() < 1e-6);
            let between = Shading { rise: 0.7, ..s };
            let c = column_above(d, 0.8 * k, true, &between);
            let (lo, hi) = ((0.8 * k * 0.7).min(k * MEAN_SHAPE), (0.8 * k * 0.7).max(k * MEAN_SHAPE));
            assert!(c > lo && c < hi, "between the two estimates: {c}");
            assert!((column_above(Vec4::new(0.5, 0.001, 0.3, 1.0), 0.0, false, &s)).abs() < 1e-6);
        }
        let mut last = 2.0;
        for k in 0..=100 {
            let r = through(k as f32, AMBIENT_FALL);
            assert!(r <= 1.0 && r > 0.0 && r < last);
            last = r;
        }
        assert_eq!((through(-3.0, AMBIENT_FALL), through(0.0, AMBIENT_FALL)), (1.0, 1.0));
    }

    /// The sample shapes the lean light's tests run over: the density, the altitude, the depth toward the light
    /// and whether it was measured, the crown's depth, the foot, the sheet's depth and the open flank.
    type LeanSample = (Vec4, f32, f32, bool, [f32; 4]);

    fn lean_samples() -> Vec<LeanSample> {
        let mut out = Vec::new();
        for (x, ext) in [(1.0f32, 1.0f32), (0.6, 1e-3), (0.05, 1e-4), (0.0, 0.0)] {
            for cover in [0.0f32, 0.3, 0.6, 0.73, 1.0] {
                for h in [0.0f32, 0.15, 0.5, 1.0, 1.4] {
                    for altitude in [0.0f32, 900.0, 5_000.0] {
                        for (depth, sampled) in [(0.0f32, true), (1.5, true), (64.0, true), (0.0, false)] {
                            for extra in [[0.0f32; 4], [2.0, 0.0, 0.0, 1.0], [0.5, 0.6, 3.0, 0.0], [0.0, 1.0, 0.05, 0.5], [30.0, 0.3, 80.0, 1.0]] {
                                out.push((Vec4::new(x, ext, cover, h), altitude, depth, sampled, extra));
                            }
                        }
                    }
                }
            }
        }
        out
    }

    fn lean_at((d, altitude, depth, sampled, [crown, foot, sheet, flank]): LeanSample, ray: &RayLight, s: &Shading) -> Vec3 {
        lean_shade(d, altitude, depth, sampled, ray, s, crown, foot, sheet, flank)
    }

    /// What the lean light emits is bounded by what the bound counts ([`light_bound`]'s model term), sample by
    /// sample: the direct term by the direct light times the phase's cap times SUN, the ambient by the light
    /// from above plus the light from below, so a step emits at most MAX_LIGHT: a mod's huge sky level, every
    /// aid at its most, every share of the sun's colour, all view angles, sun heights, depths and heights.
    /// Finite, non-negative, and the two views' sum.
    #[test]
    fn what_the_lean_light_emits_is_bounded() {
        for (scale, sun_colour) in [(3e38f32, 0.0f32), (1e3, 0.5), (1.0, 1.0)] {
            for rise in [-0.5f32, 0.05, 0.4, 1.0] {
                let i = VolumeInputs { sunlight: [5.0, 1e30, 1.0], ambient: [1e30, 0.0, 30.0], sky_scale: scale, light_dir: Vec3::new((1.0 - rise * rise).sqrt(), rise, 0.0), light_strength: 1.0, density: 4.0, amount: 0.0, sky: SkyLight { upper: [1.0, 1e30, 1.0], horizon: [3e38; 3], ambient: [1e30; 3], elevation: 90.0 }, light_aids: LightAids { sun_colour, ..most_aids() }, ..Default::default() };
                let u = uniform(&i);
                let s = shading(&u);
                for cos in [-1.0f32, -0.4, 0.0, 0.7, 1.0] {
                    let ray = RayLight { p0: octave_phase(cos, u.phase, u.loct.w * (1.0 - u.phase.z), 1.0), p1: octave_phase(cos, u.phase, u.loct.w * (1.0 - u.phase.z), u.loct.z), powder: powder_share(cos, u.lpowder.x), ..ray_light(Vec3::Y, &u) };
                    let direct_most = ray.light * (PHASE_CAP * s.column.w) * 1.0001;
                    let ambient_most = (s.upper + s.lower) * 1.0001;
                    for at in lean_samples() {
                        let direct = lean_at(at, &ray, &Shading { mode: 6, ..s });
                        let ambient = lean_at(at, &ray, &Shading { mode: 7, ..s });
                        let e = lean_at(at, &ray, &s);
                        assert!(direct.is_finite() && direct.cmpge(Vec3::ZERO).all() && direct.cmple(direct_most).all(), "{at:?}, cos {cos}, rise {rise}: direct {direct} over {direct_most}");
                        assert!(ambient.is_finite() && ambient.cmpge(Vec3::ZERO).all() && ambient.cmple(ambient_most).all(), "{at:?}, rise {rise}: ambient {ambient} over {ambient_most}");
                        assert_eq!(e, ambient + direct, "{at:?}");
                        assert!(e.max_element() <= MAX_LIGHT * 1.0002, "{at:?}, cos {cos}, rise {rise}, scale {scale}: {e}");
                    }
                }
            }
        }
    }

    /// The same bound by moonlight, at every night cap (0 and tiny ones included), with a storm deck's scattered
    /// light past the cap: the emission is finite and at most MAX_LIGHT for the extreme inputs.
    #[test]
    fn the_night_cap_keeps_the_emission_bounded() {
        for cap in [0.0f32, 0.01, 1.0, PHASE_CAP, 9.0, MAX_MOON_CAP] {
            for scale in [1.0f32, 1e3, 3e38] {
                let i = VolumeInputs {
                    sunlight: [1e30, 5.0, 1e30],
                    ambient: [0.0; 3],
                    sky_scale: scale,
                    light_dir: Vec3::new(0.0, 1.0, 0.0),
                    light_strength: 1.0,
                    density: 4.0,
                    amount: 1.0,
                    sky: SkyLight { upper: [0.0; 3], horizon: [0.0; 3], ambient: [0.0; 3], elevation: -40.0 },
                    light_aids: LightAids { moon_cap: Some(cap), night_pull: 0.0, ..most_aids() },
                    ..Default::default()
                };
                let u = uniform(&i);
                assert!(u.light.is_finite(), "cap {cap}, scale {scale}: {}", u.light);
                let s = shading(&u);
                for cos in [-1.0f32, 0.0, 1.0] {
                    let ray = RayLight { p0: octave_phase(cos, u.phase, u.loct.w * (1.0 - u.phase.z), 1.0), p1: octave_phase(cos, u.phase, u.loct.w * (1.0 - u.phase.z), u.loct.z), powder: powder_share(cos, u.lpowder.x), ..ray_light(Vec3::Y, &u) };
                    for sheet in [0.0f32, 0.5, 3.0] {
                        let e = lean_shade(Vec4::new(0.8, 3.0 / 5_000.0, 0.95, 0.9), 900.0, 1.5, true, &ray, &s, 0.0, 0.0, sheet, 0.0);
                        assert!(e.is_finite() && e.cmpge(Vec3::ZERO).all() && e.max_element() <= MAX_LIGHT * 1.0002, "cap {cap}, scale {scale}, cos {cos}, sheet {sheet}: {e}");
                    }
                }
            }
        }
    }

    /// Energy: unit radiance from both hemispheres, no direct beam, the
    /// default hemisphere weights (summing to one) and a unit background behind: whatever the slab's
    /// depth, its amount and however its steps are cut, what leaves it toward the camera is at most 1.
    #[test]
    fn a_slab_under_a_uniform_sky_emits_no_more_than_arrives() {
        assert_eq!(HEMISPHERE + HEMISPHERE, 1.0);
        const { assert!(LEAN_FAIR_LOWER <= 1.0 && LEAN_AMBIENT <= 1.0) };
        let s = Shading { upper: Vec3::splat(HEMISPHERE), lower: Vec3::splat(HEMISPHERE), column: Vec4::new(0.0, 1.78, AMBIENT_FALL, 1.0), ..bare(1.0 / 3.0) };
        let dark = RayLight { light: Vec3::ZERO, ..lit(1.0, 1.0) };
        for cover in [0.0f32, 0.3, 0.65, 1.0] {
            for thick in [0.01f32, 0.5, 1.0, 4.0, 16.0, 100.0] {
                for n in [1u32, 3, 7, 64, 1000] {
                    let dt = thick / n as f32;
                    let (mut trans, mut out) = (1.0f32, 0.0f32);
                    for k in 0..n {
                        let h = (k as f32 + 0.5) / n as f32;
                        let e = lean_shade(Vec4::new(1.0, 1.0, cover, h), 3_000.0 * h, thick * (1.0 - h), true, &dark, &s, 0.0, 0.0, 0.0, 0.0);
                        let kept = (-dt).exp();
                        out += trans * (1.0 - kept) * e.x;
                        trans *= kept;
                    }
                    out += trans;
                    assert!(out <= 1.0 + 1e-5 && out > 0.0, "amount {cover}, depth {thick}, {n} steps: {out}");
                }
            }
        }
    }

    /// Grey weather inputs give grey light, warm ones keep their hue.
    #[test]
    fn the_ambients_keep_the_weathers_hue() {
        let grey = lighting(&VolumeInputs { sunlight: [0.6; 3], ambient: [0.5; 3], sky_scale: 1.0, amount: 0.5, sky: SkyLight { upper: [0.4; 3], horizon: [0.45; 3], ambient: [0.3; 3], elevation: 40.0 }, ..Default::default() });
        for v in [grey.lupper, grey.llower] {
            assert!((v.x - v.y).abs() <= 1e-6 * v.x && (v.y - v.z).abs() <= 1e-6 * v.x && v.x > 0.0, "grey stays grey: {v}");
        }
        let dusk = lighting(&VolumeInputs { sunlight: [1.0, 0.5, 0.2], ambient: [0.9, 0.6, 0.4], sky_scale: 1.0, amount: 0.2, sky: SkyLight { upper: [0.5, 0.3, 0.3], horizon: [0.9, 0.5, 0.3], ambient: [0.4, 0.3, 0.25], elevation: 5.0 }, ..Default::default() });
        assert!(dusk.lupper.x > dusk.lupper.z && dusk.llower.x > dusk.llower.z, "dusk stays warm");
    }

    /// The directions: more depth toward the light never brightens a sample, at any view angle and height; nor
    /// does the light from above brighten with the column over it. The crown of a lit column is brighter than
    /// its base under the same sun.
    #[test]
    fn the_lean_light_darkens_with_depth() {
        let u = uniform(&noon(START_LIGHT));
        let s = shading(&u);
        for cos in [-1.0f32, -0.3, 0.4, 1.0] {
            let ray = ray_light(Vec3::new((1.0 - cos * cos).max(0.0).sqrt(), cos, 0.0), &u);
            for cover in [0.1f32, 0.6, 0.8] {
                for h in [0.1f32, 0.5, 0.9] {
                    for mode in [6u32, 7] {
                        let (mut first, mut last) = (0.0f32, f32::MAX);
                        for j in 0..=64 {
                            let e = lum(lean_shade(Vec4::new(0.6, 1e-3, cover, h), 4_000.0 * h, j as f32 * 0.25, true, &ray, &Shading { mode, ..s }, 0.0, 0.0, 0.0, 0.0));
                            assert!(e.is_finite() && e >= 0.0 && e <= last * 1.000_01, "mode {mode}, cos {cos}, amount {cover}, height {h}, depth {}: {e} over {last}", j as f32 * 0.25);
                            if j == 0 {
                                first = e;
                            }
                            last = e;
                        }
                        // Nothing but the measured depth changes along the loop: it is what darkens.
                        let most = if mode == 6 { 0.6 } else { 0.9 };
                        assert!(last < most * first, "mode {mode}, cos {cos}, amount {cover}, height {h}: {last} at depth 16 against {first} at 0");
                    }
                }
            }
            let crown = lean_shade(Vec4::new(1.0, 1e-3, 0.1, 1.0), 6_000.0, 0.0, true, &ray, &s, 0.0, 0.0, 0.0, 0.0);
            let base = lean_shade(Vec4::new(1.0, 1e-3, 0.1, 0.0), 0.0, 4.0, true, &ray, &s, 0.0, 0.0, 0.0, 0.0);
            assert!(lum(base) < lum(crown), "cos {cos}: {base} against {crown}");
        }
    }

    /// The `SUN` aid scales the direct light (both octaves) and nothing else: the ambients stay; its most is
    /// inside the bound.
    #[test]
    fn the_sun_aid_scales_the_direct_light_only() {
        let at = |sun: f32| {
            let i = VolumeInputs { sunlight: [1.0, 0.9, 0.8], ambient: [0.5; 3], sky_scale: 1.0, light_dir: Vec3::Y, light_strength: 1.0, density: 1.0, amount: 0.3, sky: SkyLight { upper: [0.3, 0.4, 0.7], horizon: [0.6; 3], ambient: [0.3; 3], elevation: 80.0 }, light_aids: LightAids::default().with_weight(LightWeight::Sun, sun), ..Default::default() };
            let u = uniform(&i);
            let (s, ray) = (shading(&u), ray_light(Vec3::new(0.3, 0.9, 0.2).normalize(), &u));
            let of = |mode: u32| lean_shade(Vec4::new(0.6, 0.001, 0.3, 0.4), 1_500.0, 1.5, true, &ray, &Shading { mode, ..s }, 0.0, 0.0, 0.0, 0.0);
            (of(6), of(7), u.lmode.w)
        };
        let (sun1, amb1, w1) = at(1.0);
        let (sun0, amb0, _) = at(0.0);
        let (sun2, amb2, w2) = at(2.0);
        assert_eq!((w1, w2), (1.0, 2.0));
        assert!(sun1.x > 0.0 && sun0 == Vec3::ZERO && (sun2 - 2.0 * sun1).abs().max_element() < 1e-5, "{sun0} {sun1} {sun2}");
        assert!(amb0 == amb1 && amb2 == amb1, "the ambients untouched");
        assert_eq!((try_weight("4"), LightWeight::Sun.var()), (Some(4.0), "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_SUN"));
    }

    /// The `SUNCOLOUR` aid: the model's direct light colour moves from the weather's cloud colour at
    /// the sunlight's luminance (0) to the sunlight colour to the bit (1), one half by default; the
    /// luminance is the sunlight's at every share; no visible cloud layer (a black cloud colour)
    /// leaves the sunlight colour; the bound is computed from the mixed colour (a hue far from the
    /// sunlight's engages it where the sunlight alone would not); the start variable and its alias read 0..1.
    #[test]
    fn the_sun_colour_follows_the_cloud_layers() {
        let lum = |c: [f32; 3]| luminance(DVec3::new(c[0].into(), c[1].into(), c[2].into()));
        // A warm noon sunlight and a cream cloud colour, an overcast pair, a dusk pair.
        let pairs = [([1.0f32, 0.86, 0.45], [0.72f32, 0.6, 0.36]), ([0.8, 0.78, 0.6], [0.75, 0.62, 0.36]), ([1.0, 0.45, 0.15], [0.55, 0.36, 0.22])];
        for (sun, cloud) in pairs {
            let ls = lum(sun);
            let sun_d = DVec3::new(sun[0].into(), sun[1].into(), sun[2].into());
            let cloud_d = DVec3::new(cloud[0].into(), cloud[1].into(), cloud[2].into());
            assert_eq!(direct_colour(sun, cloud, 1.0), sun_d, "share 1: the sunlight colour to the bit");
            let at0 = direct_colour(sun, cloud, 0.0);
            let hue = at0 / cloud_d;
            assert!((hue.x - hue.y).abs() < 1e-9 * hue.x && (hue.y - hue.z).abs() < 1e-9 * hue.x, "share 0: the cloud colour's hue, {hue}");
            for k in 0..=20 {
                let share = k as f32 / 20.0;
                let c = direct_colour(sun, cloud, share);
                assert!(c.is_finite() && c.min_element() >= 0.0, "{c}");
                assert!((luminance(c) - ls).abs() <= 1e-6 * ls, "share {share}: luminance {} against the sunlight's {ls}", luminance(c));
                // Between the two ends, channel by channel (a straight mix).
                let (lo, hi) = (at0.min(sun_d), at0.max(sun_d));
                assert!(c.cmpge(lo - 1e-9).all() && c.cmple(hi + 1e-9).all(), "share {share}: {c} outside {lo}..{hi}");
            }
            assert!((direct_colour(sun, cloud, SUN_COLOUR_SHARE) - (at0 + sun_d) * 0.5).abs().max_element() < 1e-9, "the default: half and half");
            assert_eq!(direct_colour(sun, cloud, f32::NAN), direct_colour(sun, cloud, SUN_COLOUR_SHARE));
            assert_eq!((direct_colour(sun, cloud, -2.0), direct_colour(sun, cloud, 7.0)), (at0, sun_d), "held to 0..1");
        }
        // No visible layer (the layers' mean is black), or a cloud colour too dark to have a hue:
        // the sunlight colour at any share; a black sunlight stays black.
        for cloud in [[0.0f32; 3], [1e-7, 1e-7, 1e-7], [f32::NAN, -1.0, 0.0]] {
            for share in [0.0f32, 0.5, 1.0] {
                assert_eq!(direct_colour([1.0, 0.9, 0.7], cloud, share), DVec3::new(1.0, 0.9f32.into(), 0.7f32.into()), "{cloud:?} {share}");
            }
        }
        assert_eq!(direct_colour([0.0; 3], [0.8, 0.7, 0.5], 0.0), DVec3::ZERO);
        // Through the uniform: the default is the mix at one half, the colour the uniform's light.
        let inputs = |sun_colour: f32| VolumeInputs {
            sunlight: [1.0, 0.86, 0.45],
            ambient: [0.72, 0.6, 0.36],
            sky_scale: 1.0,
            light_dir: Vec3::Y,
            light_strength: 1.0,
            density: 1.0,
            amount: 0.3,
            sky: SkyLight { upper: [0.3, 0.4, 0.7], horizon: [0.6; 3], ambient: [0.3; 3], elevation: 70.0 },
            light_aids: LightAids { sun_colour, ..Default::default() },
            ..Default::default()
        };
        assert_eq!((LightAids::default().sun_colour_share(), SUN_COLOUR_SHARE), (0.5, 0.5));
        let def = lighting(&VolumeInputs { light_aids: LightAids::default(), ..inputs(0.0) });
        assert_eq!(def.light, (direct_colour([1.0, 0.86, 0.45], [0.72, 0.6, 0.36], 0.5) * f64::from(SUN_GAIN)).as_vec3());
        let (l0, l1) = (lighting(&inputs(0.0)), lighting(&inputs(1.0)));
        assert!(l0.light.z / l0.light.x > def.light.z / def.light.x && def.light.z / def.light.x > l1.light.z / l1.light.x, "less yellow toward 0");
        assert!(l0.lupper == l1.lupper && l0.llower == l1.llower && l0.ambient == l1.ambient && l0.lmode == l1.lmode, "only the direct light's colour moves");
        // The bound from the mixed colour: a white sunlight and a pure blue cloud colour at share 0
        // put the blue channel at 1 / 0.0722 of the luminance; at a sky level of 1,000 the sunlight
        // alone would not engage the bound, the mix does (SUN 4).
        let blue = VolumeInputs {
            sunlight: [1.0; 3],
            ambient: [0.0, 0.0, 1.0],
            sky_scale: 1_000.0,
            light_dir: Vec3::Y,
            light_strength: 1.0,
            density: 1.0,
            amount: 0.3,
            light_aids: LightAids { sun_colour: 0.0, ..LightAids::default().with_weight(LightWeight::Sun, MAX_WEIGHT) },
            ..Default::default()
        };
        let l = lighting(&blue);
        assert!(l.bound < 1.0, "engaged by the mixed colour: {}", l.bound);
        let sunlit = gained([1.0; 3], 1_000.0 * f64::from(SUN_GAIN));
        let gain = f64::from(HEMISPHERE) * f64::from(LEAN_AMBIENT);
        let (up, low) = (upper_light(&blue) * gain, lower_light(&blue) * gain);
        assert_eq!(light_bound(sunlit, sunlit, gained(blue.ambient, 1_000.0 * f64::from(AMBIENT_GAIN)), up.max(shade_colour(&blue, up)), low, f64::from(MAX_WEIGHT), 0.0, PHASE_CAP), 1.0, "the sunlight's own worst channel would not have");
        let most = (l.light * (PHASE_CAP * MAX_WEIGHT) + l.lupper.truncate().max(l.shade) + l.llower.truncate()).max_element();
        assert!((MAX_LIGHT * 0.999..=MAX_LIGHT * 1.0001).contains(&most), "the worst channel scaled to the bound: {most}");
        // The start variable, its alias (the first wins when both are set), the log's words.
        let one = |name: &'static str, v: &'static str| move |n: &str| (n == name).then(|| v.to_string());
        assert_eq!(start_light_aids(one(SUN_COLOUR_VAR, "0.8")).sun_colour, 0.8);
        assert_eq!(start_light_aids(one(SUN_COLOR_VAR, " 0.25 ")).sun_colour, 0.25);
        assert_eq!(start_light_aids(one(SUN_COLOUR_VAR, "4")).sun_colour, 1.0, "held to 0..1");
        assert_eq!(start_light_aids(one(SUN_COLOUR_VAR, "warm")).sun_colour, START_LIGHT.sun_colour);
        assert_eq!(start_light_aids(|_| None).sun_colour, START_LIGHT.sun_colour);
        let both = |n: &str| match n {
            SUN_COLOUR_VAR => Some("0.1".to_string()),
            SUN_COLOR_VAR => Some("0.9".to_string()),
            _ => None,
        };
        assert_eq!(start_light_aids(both).sun_colour, 0.1);
        assert_eq!((SUN_COLOUR_VAR, SUN_COLOR_VAR), ("NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_SUNCOLOUR", "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_SUNCOLOR"));
        assert!(LightAids { sun_colour: 0.8, ..Default::default() }.text().contains("SUNCOLOUR 0.80") && !LightAids::default().text().contains("SUNCOLOUR"));
        assert!(LightAids { sun_colour: 0.0, ..Default::default() }.model_text().ends_with("; weights: SUNCOLOUR 0.00"));
        // The log line says where the lit colour comes from (the share in force).
        assert!(LightAids::default().model_text().contains("cloud layers' PNAM colour at its sunlight's luminance mixed with its sunlight colour (the sunlight's share SUNCOLOUR 0.50)"));
        assert!(LightAids { sun_colour: 0.25, ..Default::default() }.model_text().contains("(the sunlight's share SUNCOLOUR 0.25)"));
        assert_eq!((LightAids { sun_colour: f32::NAN, ..Default::default() }.sun_colour_share(), LightAids { sun_colour: -1.0, ..Default::default() }.sun_colour_share()), (SUN_COLOUR_SHARE, 0.0));
    }

    /// The back-scatter floor: it lifts a dense lit side seen away from the
    /// light (the mixed phase 0.776 straight back), does nothing at zero extinction or with its aid
    /// at 0, and is held to its most.
    #[test]
    fn the_floor_lifts_a_dense_side() {
        let u = uniform(&VolumeInputs { sunlight: [1.0; 3], sky_scale: 1.0, light_dir: Vec3::Y, light_strength: 1.0, density: 1.0, amount: 0.3, light_aids: LightAids::default().with_weight(LightWeight::Backscatter, 2.0), ..Default::default() });
        let s = Shading { mode: 6, ..shading(&u) };
        // The floor alone: the powder off.
        let away = RayLight { powder: 0.0, ..ray_light(-Vec3::Y, &u) };
        let toward = RayLight { powder: 0.0, ..ray_light(Vec3::Y, &u) };
        assert!((away.p0 - 0.776).abs() < 2e-3, "straight back {}", away.p0);
        assert!(toward.p0 > PHASE_CAP && toward.cap == PHASE_CAP, "the forward lobe before the cap: {}", toward.p0);
        // A dense cumulus side: shape 0.8, 3 optical depths over 5,000 units.
        let dense = Vec4::new(0.8, 3.0 / 5_000.0, 0.3, 0.9);
        assert!((floor_at(dense.x * dense.y, &s) - (2.0 * FLOOR_LENGTH * 4.8e-4).min(2.0 * FLOOR_MAX * LEAN_FLOOR)).abs() < 1e-4);
        let at = |d: Vec4, s: &Shading| lean_shade(d, 100.0, 0.0, true, &away, s, 0.0, 0.0, 0.0, 0.0).x;
        let off = Shading { back: Vec4::new(0.0, 0.0, MEASURED_FROM, MEASURED_TO), ..s };
        assert!(at(dense, &s) > at(dense, &off) * 1.02, "a lift: {} over {}", at(dense, &s), at(dense, &off));
        let clear = Vec4::new(0.8, 0.0, 0.3, 0.9);
        assert_eq!(at(clear, &s), at(clear, &off), "nothing at zero extinction");
        assert_eq!((floor_at(0.0, &s), floor_at(-1.0, &s)), (0.0, 0.0));
        assert_eq!(floor_at(1.0, &s), 2.0 * FLOOR_MAX * LEAN_FLOOR, "held to its most");
    }

    /// The light's aids: the start variables and their start defaults ([`START_LIGHT`]), held to 0..4.
    #[test]
    fn the_light_aids_parse() {
        assert_eq!(start_light_aids(|_| None), START_LIGHT);
        assert_eq!(START_LIGHT.text(), "BACKSCATTER x1.65, SUN x1.30, LEVEL x1.12, SUNCOLOUR 0.00", "the start look named against the designed scale");
        assert!(LightAids::default().weights == [1.0; LIGHT_WEIGHTS] && LightAids::default().text().is_empty(), "the designed values, the tests' base");
        let vars = |name: &str| match name {
            "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_OCTAVE" => Some("0".to_string()),
            "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_UPPER" => Some("9".to_string()),
            "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_GROUND" => Some("NaN".to_string()),
            "NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_FORWARD" => Some("-2".to_string()),
            _ => None,
        };
        let a = start_light_aids(vars);
        assert_eq!((a.weight(LightWeight::Octave), a.weight(LightWeight::Upper), a.weight(LightWeight::Ground), a.weight(LightWeight::Forward), a.weight(LightWeight::Backscatter)), (0.0, 4.0, 1.0, 0.0, START_LIGHT.weight(LightWeight::Backscatter)));
        assert!(a.text().contains("OCTAVE x0.00") && a.text().contains("UPPER x4.00") && a.text().contains("BACKSCATTER x1.65"), "an unset weight its start default");
        assert!(!LightAids::default().model_text().contains("weights:"));
        assert!(LightAids::default().with_weight(LightWeight::Backscatter, 2.0).model_text().contains("BACKSCATTER x2.00"));
        assert_eq!((try_weight(" 2.5 "), try_weight("7"), try_weight("inf"), try_weight("x")), (Some(2.5), Some(4.0), None, None));
        assert_eq!((debug_weight(None), debug_weight(Some("x"))), (1.0, 1.0));
        let names: Vec<&str> = LightWeight::ALL.into_iter().map(LightWeight::short).collect();
        assert_eq!(names, ["OCTAVE", "BACKSCATTER", "UPPER", "LOWER", "GROUND", "FORWARD", "SUN", "LEVEL", "OCCLUDE", "POWDER"]);
        assert!(LightWeight::ALL.into_iter().enumerate().all(|(k, w)| w.index() == k));
        assert!(LightWeight::ALL.into_iter().all(|w| w.var() == format!("NVF_PLAY_DEBUG_VOLUMETRIC_CLOUDS_{}", w.short())));
        // A weight that is not a number reads as 1 wherever it came from.
        assert_eq!(LightAids { weights: [f32::NAN; LIGHT_WEIGHTS], sun_colour: f32::NAN, ..Default::default() }.weight(LightWeight::Upper), 1.0);
        assert_eq!((octave_weight(0.0), octave_weight(4.0)), (0.0, 2.0 / 3.0));
    }

    /// The sun's daylight for the ground: 0 under the horizon (the moon lights no ground), rising
    /// through the first degrees, the elevation's sine above.
    #[test]
    fn the_ground_sees_the_sun_only_by_day() {
        assert_eq!((daylight(-30.0), daylight(0.0), daylight(f32::NAN), daylight(f32::INFINITY)), (0.0, 0.0, 0.0, 0.0));
        assert!(daylight(1.0) > 0.0 && daylight(1.0) < daylight(4.0));
        assert!((daylight(30.0) - 0.5).abs() < 1e-6 && (daylight(90.0) - 1.0).abs() < 1e-6);
        let mut last = 0.0;
        for k in 0..=90 {
            let d = daylight(k as f32);
            assert!(d >= last && d <= 1.0);
            last = d;
        }
        // At night the light from below is the weather's Ambient alone, never the moon's light.
        let night = VolumeInputs { sunlight: [1.0; 3], sky_scale: 1.0, sky: SkyLight { ambient: [0.1; 3], elevation: -20.0, ..Default::default() }, ..Default::default() };
        let g = lower_light(&night);
        assert!((g - DVec3::splat(0.1 * f64::from(GROUND_REFLECTANCE))).abs().max_element() < 1e-9, "{g}");
        // Overcast lets less sun onto the ground than clear.
        let day = |amount: f32| lower_light(&VolumeInputs { amount, sky: SkyLight { elevation: 60.0, ..night.sky }, ..night }).x;
        assert!(day(0.9) < day(0.1) && day(0.1) > g.x);
    }

    fn march() -> String {
        include_str!("volumetric_clouds_march.wesl").replace("\r\n", "\n")
    }

    /// The leaf alone is valid WGSL and a leaf: no import, no binding, no texture, no `CloudVolume`.
    #[test]
    fn the_light_module_is_a_valid_leaf() {
        use wgpu::naga;
        let src = leaf();
        for word in ["import ", "@group", "@binding", "texture", "CloudVolume", "sampler"] {
            assert!(!src.lines().filter(|l| !l.trim_start().starts_with("//")).any(|l| l.contains(word)), "the leaf has `{word}`");
        }
        let module = naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&src)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).unwrap_or_else(|e| panic!("{e:?}"));
    }

    /// The march calls the leaf at its one emission site and prepares the ray's angular part once per
    /// stretch; the shading lines left the march.
    #[test]
    fn the_march_shades_through_the_leaf() {
        let src = march();
        let plain = plain_march_wgsl();
        assert!(!plain.lines().any(|l| l.trim_start().starts_with("import ")), "the assembler resolved the import");
        for gone in ["fn vc_hg(", "let direct = "] {
            assert!(!src.contains(gone), "the march still has `{gone}`");
        }
        for views in [false, true] {
            let src = crate::volumetric_clouds_lightvol::lv_resolve_variant(&src, false, views);
            assert_eq!(src.matches("let ray_light = vc_ray_light_of(ray.dir, v);").count(), 1, "once per stretch");
            assert_eq!(src.matches("let shading = vc_shading(v);").count(), 1);
            assert_eq!(src.matches("emitted = vc_incident(d, depth, sampled, ray_light, shading, rel, ray.origin, width, column, v);").count(), 1, "the emission site");
            // What stays in the march: the step's transmittance, the weighting, the distance.
            assert_eq!(src.matches("m.colour += taken * emitted;").count(), 1);
            assert_eq!(src.matches("let kept = exp(-d.x * d.y * dt);").count(), 1);
        }
    }

    /// The march toward the light gives the optical depth, not its transmittance; a sample under the gate
    /// defaults to depth 0 not measured.
    #[test]
    fn the_light_march_returns_the_depth() {
        let src = march();
        let flat = &src[src.find("fn vc_march_refined(").expect("the march")..];
        let flat = &flat[..flat.find("\n}\n").expect("its end")];
        assert!(flat.contains("optical += found.x * found.y * ds;") && flat.contains("let taps = clamp(u32(v.march.y), 1u, 16u);"), "the taps");
        assert_eq!(flat.matches("exp(").count(), 1, "the step's transmittance alone: the light's depth is never turned into one here");
        assert_eq!(flat.matches("var depth = 0.0;\n").count(), 1);
        assert_eq!(flat.matches("var sampled = false;\n").count(), 1);
        assert_eq!(flat.matches("if !reuse && d.x > v.march.w && v.light_dir.w > 0.0 {").count(), 1);
        assert_eq!(flat.matches("depth = handed + keep * optical;\n                sampled = true;").count(), 1);
        assert!(!src.contains("var lit"), "no transmittance default of 1 left");
    }

    /// The light's views: codes 6, 7, 8 (5 left to the cloud map), parsed
    /// and decoded; sun and ambient split the clouds' light in two; the depth's code is bounded,
    /// monotone and marks a skipped sample apart; the trace averages it by opacity; the base-plane
    /// views stay 1, 2 and 4 in the entries.
    #[test]
    fn the_light_views() {
        use crate::volumetric_clouds::debug_view;
        let views = [(CloudView::Sun, "sun", 6), (CloudView::Ambient, " Ambient ", 7), (CloudView::Depth, "depth", 8), (CloudView::Opacity, "opacity", 10), (CloudView::BodyHeight, "body-height", 13)];
        for (view, name, code) in views {
            assert_eq!(debug_view(Some(name)), view);
            assert_eq!(view.code(), code as f32);
            let mut u = uniform(&VolumeInputs { view, ..Default::default() });
            assert_eq!(view_code(&u), code);
            assert!(!view_text(view).is_empty());
            u.flags.z = 99.0;
            assert_eq!(view_code(&u), 13, "held to the highest view code");
        }
        assert!(view_text(CloudView::Cover).is_empty());
        let sky = SkyLight { upper: [0.2, 0.3, 0.6], horizon: [0.5, 0.5, 0.55], ambient: [0.3, 0.3, 0.3], elevation: 70.0 };
        let u = uniform(&VolumeInputs { sunlight: [1.0, 0.9, 0.7], ambient: [0.3, 0.4, 0.6], sky_scale: 1.0, light_dir: Vec3::Y, light_strength: 1.0, steps: 28, density: 1.0, amount: 0.3, sky, ..Default::default() });
        let ray = ray_light(Vec3::new(0.2, 0.9, 0.1).normalize(), &u);
        let base = shading(&u);
        for (depth, sampled) in [(0.0f32, true), (2.5, true), (0.0, false)] {
            let d = Vec4::new(0.4, 0.001, 0.5, 0.6);
            let at = |mode: u32| lean_shade(d, 1_500.0, depth, sampled, &ray, &Shading { mode, ..base }, 0.0, 0.0, 0.0, 0.0);
            assert_eq!(at(6) + at(7), at(0), "sun and ambient sum to the clouds' light");
            assert!(at(6).min_element() > 0.0 && at(7).min_element() > 0.0);
            assert_eq!(at(8), depth_code(depth, sampled));
        }
        let mut last = -1.0;
        for k in 0..=200 {
            let g = depth_code(k as f32 * 0.5, true);
            assert!(g.x == g.y && g.y == g.z && g.x >= last && g.x <= DEPTH_GREY && g.is_finite());
            last = g.x;
        }
        assert_eq!(depth_code(0.0, true), Vec3::ZERO);
        assert!((depth_code(1.0, true).x - DEPTH_GREY / 6.0).abs() < 1e-6 && (depth_code(63.0, true).x - DEPTH_GREY).abs() < 1e-6);
        assert_eq!(depth_code(f32::INFINITY, true).x, DEPTH_GREY);
        assert!(depth_code(-1.0, true) == Vec3::ZERO && depth_code(f32::NAN, true).is_finite());
        let skipped = depth_code(0.0, false);
        assert!(skipped.y == 0.0 && skipped.x > 0.5 && skipped.z > 0.5, "magenta, never a grey");
        let src = march();
        assert!(src.contains("return u32(clamp(v.flags.z, 0.0, 13.0) + 0.5);"), "the code range includes the optical diagnostics");
        assert_eq!(src.matches("if (vc_view_code(v) == 8u || vc_view_code(v) == 13u) && vc_depth_shown(hidden) {\n        return MediumSample(colour / hidden, 0.0, dist);").count(), 1, "the trace");
        let leaf = leaf();
        assert!(leaf.contains("const VC_DEPTH_WHITE_LOG2: f32 = 6.0;") && leaf.contains("const VC_DEPTH_GREY: f32 = 0.9;") && leaf.contains("const VC_DEPTH_SHOWN: f32 = 0.01;"));
        assert_eq!((DEPTH_WHITE_LOG2, DEPTH_GREY, DEPTH_SHOWN), (6.0, 0.9, 0.01), "the constants the leaf writes");
        let trace = include_str!("medium_trace.wesl").replace("\r\n", "\n");
        assert_eq!(trace.matches("if view == 1u || view == 2u || view == 4u {").count(), 2, "the light's views take the march path in both entries");
    }

    // -----------------------------------------------------------------------------------
    // Clouds by moonlight (#3032).
    // -----------------------------------------------------------------------------------

    /// [`lighting`]'s colours as they were before the night's dimming, operation by operation: the direct light,
    /// the light from above and the one from below.
    fn undimmed(i: &VolumeInputs) -> [Vec3; 3] {
        let a = i.light_aids;
        let scale = nn(i.sky_scale);
        let level = f64::from(a.weight(LightWeight::Level));
        let light = gained(i.sunlight, f64::from(scale * SUN_GAIN));
        let ambient = gained(i.ambient, f64::from(scale * AMBIENT_GAIN));
        let elevation = i.sky.elevation;
        let low_sun = if elevation.is_finite() { smooth(0.0, 5.0, elevation) * (1.0 - smooth(5.0, 25.0, elevation)) } else { 0.0 };
        let share = a.sun_colour_share();
        let sun_share = share / (1.0 - 0.67 * low_sun * (1.0 - share));
        let moon_ratio = crate::sky::MOON_SHARE / crate::sky::SUN_SHARE;
        let moon_ini = if elevation.is_finite() && elevation < 0.0 { 1.0 + i.moon_extra } else { 1.0 };
        let moon_gain = if elevation.is_finite() { 1.0 - (1.0 - moon_ratio) * smooth(0.0, crate::volumetric_clouds::LIGHT_FADE_DEG, -elevation) } else { 1.0 };
        let direct = direct_colour(i.sunlight, i.ambient, sun_share) * f64::from(scale * SUN_GAIN) * level * f64::from((1.0 + 0.15 * low_sun) * moon_gain * moon_ini);
        let lean_gain = f64::from(LEAN_AMBIENT);
        let sky = upper_light(i);
        let upper = sky * f64::from(HEMISPHERE * a.weight(LightWeight::Upper)) * level * lean_gain;
        let mut bounce = lower_light(i);
        let y = luminance(sky);
        if y > MIN_CLOUD_LUMA {
            bounce = bounce.lerp(sky * (luminance(bounce) / y), 0.35);
        }
        let lower = bounce * f64::from(HEMISPHERE * a.weight(LightWeight::Lower)) * level * lean_gain;
        let shade = shade_colour(i, upper);
        let k = light_bound(light, direct, ambient, upper.max(shade), lower, f64::from(a.weight(LightWeight::Sun)), 0.0, a.phase_cap(night_share(i.sky.elevation)).max(PHASE_CAP));
        [direct, upper, lower].map(|v| (v * k).as_vec3())
    }

    /// A clear night whose sunlight is some fifty times the cloud layers' luminance.
    fn clear_night(light_aids: LightAids) -> VolumeInputs {
        VolumeInputs {
            sunlight: [0.46, 0.50, 0.70],
            ambient: [0.009, 0.009, 0.009],
            sky_scale: 1.0,
            light_dir: Vec3::new(0.3, 0.5, 0.2),
            light_strength: 1.0,
            density: 1.0,
            amount: 0.3,
            sky: SkyLight { upper: [0.0; 3], horizon: [0.05, 0.06, 0.08], ambient: [0.41, 0.40, 0.68], elevation: -40.0 },
            light_aids,
            ..Default::default()
        }
    }

    /// Weathers of every kind: dark and bright clouds, black and huge colours, channels that are not numbers.
    fn skies() -> Vec<VolumeInputs> {
        let base = clear_night(START_LIGHT);
        let with = |sunlight: [f32; 3], ambient: [f32; 3], sky_scale: f32| VolumeInputs { sunlight, ambient, sky_scale, ..base };
        vec![
            base,
            noon(START_LIGHT),
            with([0.0; 3], [0.0; 3], 1.0),
            with([0.0; 3], [0.5; 3], 1.0),
            with([1.0; 3], [0.0; 3], 1.0),
            with([1.0; 3], [2e-6; 3], 1.0),
            with([0.05; 3], [0.9, 0.8, 0.7], 0.0),
            with([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1_000.0),
            with([f32::NAN, 1.0, -1.0], [f32::INFINITY, 0.2, f32::NAN], f32::NAN),
            with([1e30; 3], [1e-30; 3], 1e6),
            VolumeInputs { sky: SkyLight { upper: [f32::NAN; 3], horizon: [f32::INFINITY; 3], ambient: [1e30; 3], elevation: 0.0 }, ..base },
            VolumeInputs { moon_extra: 0.7, sky: SkyLight { ambient: [0.0; 3], ..base.sky }, ..base },
        ]
    }

    /// Aids far from their start, the night's among them.
    fn tunes() -> Vec<LightAids> {
        let odd = LightAids { sun_colour: 0.3, ..most_aids() };
        vec![
            START_LIGHT,
            LightAids::default(),
            LightAids { night_pull: 0.0, ..START_LIGHT },
            LightAids { night_pull: MAX_NIGHT_PULL, moon_forward: Some(4.0), moon_back: Some(0.0), ..START_LIGHT },
            odd,
            LightAids { night_pull: f32::NAN, moon_forward: Some(f32::NAN), ..odd },
        ]
    }

    fn bits(v: Vec3) -> [u32; 3] {
        v.to_array().map(f32::to_bits)
    }

    /// By day and at the horizon the packed uniform is the one before the moonlight, bit for bit, for every
    /// weather, tune and value of the night's keys.
    #[test]
    fn the_day_is_untouched_by_the_night_pull() {
        let mut elevations: Vec<f32> = (1..=400).map(|k| k as f32 * 0.01).collect();
        elevations.extend((4..=90).map(|k| k as f32));
        elevations.extend([0.0, -0.0, f32::MIN_POSITIVE, f32::NAN, f32::INFINITY, f32::NEG_INFINITY]);
        for sky in skies() {
            for aids in tunes() {
                for &elevation in &elevations {
                    assert_eq!(night_share(elevation), None, "{elevation}");
                    let i = VolumeInputs { light_aids: aids, sky: SkyLight { elevation, ..sky.sky }, ..sky };
                    let u = uniform(&i);
                    assert_eq!([bits(u.light.truncate()), bits(u.lupper.truncate()), bits(u.llower.truncate())], undimmed(&i).map(bits), "{elevation}: {i:?}");
                    let back = aids.weight(LightWeight::Backscatter);
                    assert_eq!((u.loct.w.to_bits(), u.lfloor.x.to_bits(), u.lfloor.y.to_bits()), (aids.weight(LightWeight::Forward).to_bits(), (FLOOR_LENGTH * back).to_bits(), (FLOOR_MAX * back * LEAN_FLOOR).to_bits()));
                    // The night's keys reach nothing else of the uniform.
                    let other = uniform(&VolumeInputs { light_aids: LightAids { night_pull: 1.3, moon_forward: Some(2.5), moon_back: Some(0.25), moon_cap: Some(14.0), ..aids }, ..i });
                    assert!(u == other || format!("{u:?}") == format!("{other:?}"), "{elevation}: {i:?}");
                }
            }
        }
    }

    /// By night the dimming only lowers the direct light and the light from below, never the light from above;
    /// a pull of 0 is the light before it bit for bit; every value stays finite.
    #[test]
    fn the_night_only_dims() {
        for sky in skies() {
            for aids in tunes() {
                for elevation in [-1e-6f32, -0.01, -0.5, -2.0, -4.0, -30.0, -90.0] {
                    let i = VolumeInputs { light_aids: aids, sky: SkyLight { elevation, ..sky.sky }, ..sky };
                    let l = lighting(&i);
                    let [light, upper, lower] = undimmed(&i);
                    assert!(l.light.is_finite() && l.llower.is_finite() && l.lupper.is_finite() && l.loct.is_finite() && l.lfloor.is_finite(), "{i:?}");
                    assert!(l.light.cmple(light).all() && l.llower.truncate().cmple(lower).all(), "{elevation}: {i:?}");
                    assert_eq!(bits(l.lupper.truncate()), bits(upper));
                    let none = lighting(&VolumeInputs { light_aids: LightAids { night_pull: 0.0, ..aids }, ..i });
                    assert_eq!([bits(none.light), bits(none.llower.truncate())], [bits(light), bits(lower)]);
                }
            }
        }
    }

    /// The gain: at most 1, falling with the pull and the source, rising with the cloud colour, 1 for a source no
    /// brighter than the clouds, for no cloud colour and for what is not a number; no jump where it begins.
    #[test]
    fn the_night_gain_is_a_monotone_attenuation() {
        assert!((night_gain(0.508, 0.009, 0.5) - (0.009f64 / 0.508).sqrt()).abs() < 1e-12);
        assert!((night_gain(0.508, 0.009, 1.0) * 0.508 - 0.009).abs() < 1e-12, "a pull of 1: the clouds' own luminance");
        for (s, c, q) in [(0.2, 0.2, 1.0), (0.1, 0.2, 1.0), (0.0, 0.0, 1.0), (0.0, 0.5, 1.0), (1.0, 0.0, 1.0), (1.0, MIN_CLOUD_LUMA, 1.0), (1.0, 0.1, 0.0), (f64::NAN, 0.1, 1.0), (1.0, f64::NAN, 1.0), (1.0, 0.1, f64::NAN)] {
            assert_eq!(night_gain(s, c, q), 1.0, "{s} {c} {q}");
        }
        let mut last = 1.0;
        for k in 0..=200 {
            let g = night_gain(0.5, 0.01, f64::from(k) * 0.01);
            assert!(g <= last && g > 0.0);
            last = g;
        }
        let (mut last_cloud, mut last_source) = (0.0, 1.0);
        for k in 1..=1_000 {
            let x = f64::from(k) * 1e-3;
            let (c, s) = (night_gain(1.0, x, 0.5), night_gain(x, 0.01, 0.5));
            assert!(c >= last_cloud && s <= last_source, "{x}");
            (last_cloud, last_source) = (c, s);
        }
        // Where the clouds fade in from black: from 1, without a jump, to the whole gain.
        let mut last = 1.0;
        for k in 0..=7_000 {
            let g = night_gain(0.5, MIN_CLOUD_LUMA * (1.0 + f64::from(k) * 1e-3), 0.5);
            assert!((g - last).abs() < 1e-3, "{k}: {last} to {g}");
            last = g;
        }
        assert!((last - (8.0 * MIN_CLOUD_LUMA / 0.5).sqrt()).abs() < 1e-9);
    }

    /// The horizon: the night's share grows from 0 to 1 over the fade, and what the sky shows of the two lights
    /// (the direct one with its strength) has no step in a sweep of a hundredth of a degree, through sunset and
    /// through the end of the fade.
    #[test]
    fn the_horizon_shows_no_step_of_the_night() {
        assert!(night_share(-1e-3).is_some_and(|n| n < 1e-6));
        assert_eq!((night_share(-crate::volumetric_clouds::LIGHT_FADE_DEG), night_share(-80.0)), (Some(1.0), Some(1.0)));
        let mut last_n = 0.0;
        for k in 1..=500 {
            let n = night_share(k as f32 * -0.01).expect("below the horizon");
            assert!(n >= last_n && n <= 1.0);
            last_n = n;
        }
        let shown = |elevation: f32| {
            let base = clear_night(LightAids { moon_forward: Some(3.0), moon_back: Some(0.2), ..START_LIGHT });
            let (_, strength) = crate::volumetric_clouds::direct_light(Vec3::Y, elevation, true);
            let l = lighting(&VolumeInputs { light_strength: strength, sky: SkyLight { elevation, ..base.sky }, ..base });
            (l.light * strength, l.llower.truncate(), Vec2::new(l.loct.w, l.lfloor.y))
        };
        let whole = undimmed(&clear_night(START_LIGHT));
        let mut last = shown(8.0);
        for k in (-800..800).rev() {
            let now = shown(k as f32 * 0.01);
            assert!((now.0 - last.0).abs().max_element() < 0.01 * whole[0].max_element(), "direct at {k}: {} to {}", last.0, now.0);
            assert!((now.1 - last.1).abs().max_element() < 0.01 * whole[2].max_element(), "below at {k}: {} to {}", last.1, now.1);
            assert!((now.2 - last.2).abs().max_element() < 0.02, "the phase's weights at {k}");
            last = now;
        }
    }

    /// The deep night's levels: the direct light at the two luminances' mix by [`NIGHT_PULL`], its hue kept, the
    /// light from below pulled by its own gain, the sky level and MoonlightNVSE.ini's factor outside of it, each
    /// once; a weather whose night sunlight is no brighter than its clouds untouched.
    #[test]
    fn the_moonlit_clouds_follow_the_weathers_cloud_luminance() {
        let i = clear_night(START_LIGHT);
        let (l, [light, _, lower]) = (lighting(&i), undimmed(&i));
        let (ls, lc) = (luminance(gained(i.sunlight, 1.0)), luminance(gained(i.ambient, 1.0)));
        let g = (lc / ls).powf(f64::from(NIGHT_PULL)) as f32;
        assert!((l.light - light * g).abs().max_element() < 1e-6 * light.max_element(), "{} against {}", l.light, light * g);
        let kept = lower * ((lc / luminance(lower_light(&i))).powf(f64::from(NIGHT_PULL)) as f32);
        assert!((l.llower.truncate() - kept).abs().max_element() < 1e-5 * lower.max_element());
        // Twice the sky level: twice the light, the gains the same.
        let two = lighting(&VolumeInputs { sky_scale: 2.0, ..i });
        assert!((two.light - 2.0 * l.light).abs().max_element() < 1e-6 && (two.llower - 2.0 * l.llower).abs().max_element() < 1e-6);
        // MoonlightNVSE.ini's factor, once, on the direct light alone.
        let ini = lighting(&VolumeInputs { moon_extra: 0.5, ..i });
        assert!((ini.light - 1.5 * l.light).abs().max_element() < 1e-6 && ini.llower == l.llower);
        // Clouds as bright as the moonlight (a lit overcast): both lights as they were.
        let overcast = VolumeInputs { ambient: [0.75; 3], ..i };
        assert_eq!((lighting(&overcast).light, lighting(&overcast).llower.truncate()), (undimmed(&overcast)[0], undimmed(&overcast)[2]));
        // Without a moon the direct light's strength is 0; the light from below dims all the same.
        assert_eq!(crate::volumetric_clouds::direct_light(Vec3::Y, -40.0, false).1, 0.0);
    }

    /// The night's aids: their start variables, their ranges and their words; the two phase weights in place of
    /// the day's by the night's share, the day's where the moon has none.
    #[test]
    fn the_night_aids_parse_and_weigh() {
        let one = |v: &'static str| move |n: &str| (n == NIGHT_PULL_VAR).then(|| v.to_string());
        assert_eq!((start_light_aids(one("0.8")).night_pull, start_light_aids(one("9")).night_pull, start_light_aids(one("-1")).night_pull), (0.8, MAX_NIGHT_PULL, 0.0));
        assert_eq!((start_light_aids(one("dark")).night_pull, start_light_aids(one("NaN")).night_pull, start_light_aids(|_| None).night_pull), (NIGHT_PULL, NIGHT_PULL, NIGHT_PULL));
        assert_eq!((LightAids { night_pull: f32::NAN, ..START_LIGHT }.night_pull(), LightAids { night_pull: 5.0, ..START_LIGHT }.night_pull()), (NIGHT_PULL, MAX_NIGHT_PULL));
        assert!(LightAids { night_pull: 0.75, ..START_LIGHT }.text().ends_with("NIGHTPULL 0.75") && !START_LIGHT.text().contains("NIGHTPULL"));
        assert!(START_LIGHT.model_text().contains("(NIGHTPULL 0.45), every colour bounded to"));
        let vars = |n: &str| match n {
            MOON_FORWARD_VAR => Some("2.5".to_string()),
            MOON_BACK_VAR => Some("9".to_string()),
            _ => None,
        };
        let a = start_light_aids(vars);
        assert_eq!((a.moon_forward, a.moon_back), (Some(2.5), Some(MAX_WEIGHT)));
        assert!(a.text().ends_with("MOONFORWARD x2.50, MOONBACK x4.00"), "{}", a.text());
        assert_eq!((START_LIGHT.moon_forward, START_LIGHT.moon_back), (MOON_FORWARD, MOON_BACK));
        // The cap by moonlight: parsed and held, from the day's to the moon's by the night's share, the day's by day
        // and where it is unset or not a number; in the uniform and nowhere else of it.
        let cap = |v: &'static str| move |n: &str| (n == MOON_CAP_VAR).then(|| v.to_string());
        assert_eq!((start_light_aids(cap("12")).moon_cap, start_light_aids(cap("99")).moon_cap, start_light_aids(cap("-1")).moon_cap), (Some(12.0), Some(MAX_MOON_CAP), Some(0.0)));
        assert_eq!((start_light_aids(cap("high")).moon_cap, START_LIGHT.moon_cap), (MOON_CAP, MOON_CAP));
        let capped = |m: Option<f32>| LightAids { moon_cap: m, ..START_LIGHT };
        assert_eq!((capped(Some(12.0)).phase_cap(None), capped(Some(12.0)).phase_cap(Some(0.0)), capped(Some(12.0)).phase_cap(Some(0.5)), capped(Some(12.0)).phase_cap(Some(1.0))), (PHASE_CAP, PHASE_CAP, (PHASE_CAP + 12.0) * 0.5, 12.0));
        assert_eq!((capped(None).phase_cap(Some(1.0)), capped(Some(f32::NAN)).phase_cap(Some(1.0)), capped(Some(50.0)).phase_cap(Some(1.0))), (PHASE_CAP, PHASE_CAP, MAX_MOON_CAP));
        let night = uniform(&clear_night(capped(Some(12.0))));
        assert_eq!(night.phase.w, 12.0, "the deep night");
        assert_eq!(uniform(&clear_night(capped(None))).phase.w, PHASE_CAP);
        let plain = uniform(&clear_night(capped(None)));
        assert_eq!((night.phase.truncate(), night.light_dir, night.lmode, night.loct, night.lfloor), (plain.phase.truncate(), plain.light_dir, plain.lmode, plain.loct, plain.lfloor));
        // Between the day's and the moon's, the other weights and the day untouched.
        assert_eq!((a.phase_weight(LightWeight::Forward, None), a.phase_weight(LightWeight::Forward, Some(0.5)), a.phase_weight(LightWeight::Forward, Some(1.0))), (1.0, 1.75, 2.5));
        assert_eq!(a.phase_weight(LightWeight::Sun, Some(1.0)), a.weight(LightWeight::Sun));
        let own = LightAids { moon_forward: None, moon_back: None, ..a };
        assert_eq!((own.phase_weight(LightWeight::Forward, Some(1.0)), own.phase_weight(LightWeight::Backscatter, Some(1.0))), (1.0, START_LIGHT.weight(LightWeight::Backscatter)));
        assert_eq!(LightAids { moon_forward: Some(f32::NAN), ..own }.phase_weight(LightWeight::Forward, Some(1.0)), 1.0);
        // Through the uniform: a stronger forward lobe brightens the ray toward the moon, a weaker floor darkens
        // the side seen away from it.
        let at = |aids: LightAids, toward: f32| {
            let i = clear_night(aids);
            let u = uniform(&i);
            let s = Shading { mode: 6, ..shading(&u) };
            lean_shade(Vec4::new(0.8, 3.0 / 5_000.0, 0.3, 0.9), 100.0, 0.0, true, &ray_light(u.light_dir.truncate() * toward, &u), &s, 0.0, 0.0, 0.0, 0.0).max_element()
        };
        assert!(at(LightAids { moon_back: Some(0.0), ..own }, -1.0) < at(own, -1.0));
        let dim = LightAids { moon_forward: Some(0.2), ..own };
        assert!(at(dim, 1.0) < at(own, 1.0));
    }

    // -----------------------------------------------------------------------------------
    // Form and contrast (#3032, after stage 2i M3).
    // -----------------------------------------------------------------------------------

    /// Clear noon: a warm noon sunlight, a cream cloud colour, a blue sky, the sun straight overhead.
    fn noon(light_aids: LightAids) -> VolumeInputs {
        VolumeInputs {
            sunlight: [1.0, 0.86, 0.45],
            ambient: [0.72, 0.6, 0.36],
            sky_scale: 1.0,
            light_dir: Vec3::Y,
            light_strength: 1.0,
            density: 1.0,
            amount: 0.1,
            sky: SkyLight { upper: [0.3, 0.4, 0.7], horizon: [0.6, 0.6, 0.6], ambient: [0.3; 3], elevation: 89.0 },
            light_aids,
            ..Default::default()
        }
    }

    fn lum(c: Vec3) -> f32 {
        c.dot(Vec3::new(LUMA[0] as f32, LUMA[1] as f32, LUMA[2] as f32))
    }

    /// A low sun shifts the direct term toward the weather sunlight. The moon's direct
    /// contribution follows the world's moon-to-sun intensity ratio through twilight.
    #[test]
    fn low_sun_rims_follow_the_weather_sunlight() {
        let at = |angle: f32| {
            let mut i = noon(LightAids { sun_colour: 0.25, ..START_LIGHT });
            i.sky.elevation = angle;
            i.sunlight = [1.0, 0.4, 0.1];
            lighting(&i).light
        };
        let high = at(60.0);
        let low = at(5.0);
        let moon = at(-30.0);
        assert!((moon - high * (crate::sky::MOON_SHARE / crate::sky::SUN_SHARE)).abs().max_element() < 1e-5);
        // MoonlightNVSE.ini's factor, by night alone.
        let with = |angle: f32, extra: f32| {
            let mut i = noon(LightAids { sun_colour: 0.25, ..START_LIGHT });
            i.sky.elevation = angle;
            i.sunlight = [1.0, 0.4, 0.1];
            i.moon_extra = extra;
            lighting(&i).light
        };
        assert!((with(-30.0, 0.5) - moon * 1.5).abs().max_element() < 1e-5);
        assert_eq!(with(60.0, 0.5), high);
        assert_eq!(with(-2.0, -1.0), Vec3::ZERO, "no moonlight at a factor of 0, at dusk either");
        assert!(lum(moon) < lum(at(-1.0)) && lum(at(-1.0)) < lum(high));
        assert!(low.x / low.z > high.x / high.z);
        assert!((lum(low) / lum(high) - 1.15).abs() < 1e-5);
    }

    /// The powder: none toward the light, whole away from it; it keeps between 1 - share and 1, more of a
    /// denser sample.
    #[test]
    fn the_powder_darkens_thin_cloud_away_from_the_light() {
        let u = uniform(&noon(START_LIGHT));
        assert_eq!(u.lpowder, Vec4::new(POWDER_SHARE, 0.0, 0.0, 0.0));
        assert_eq!((powder_share(1.0, u.lpowder.x), powder_share(-1.0, u.lpowder.x)), (0.0, POWDER_SHARE));
        for k in 0..=200 {
            assert!((0.0..=POWDER_SHARE).contains(&powder_share(-1.0 + k as f32 / 100.0, u.lpowder.x)));
        }
        // The powder's term itself.
        for share in [0.0f32, 0.25, 0.5, 1.0] {
            let mut last = 0.0f32;
            for k in 0..=40 {
                let ext = k as f32 * 2.5e-5;
                let p = powder(ext, share);
                assert!(p >= 1.0 - share - 1e-6 && p <= 1.0 && p >= last, "share {share}, ext {ext}: {p}");
                last = p;
            }
            assert!((powder(0.0, share) - (1.0 - share)).abs() < 1e-6 && powder(1.0, share) == 1.0 && powder(-1.0, share) == powder(0.0, share));
        }
        // The aid: the share held to 1 at the most.
        assert_eq!(lighting(&noon(LightAids::default().with_weight(LightWeight::Powder, MAX_WEIGHT))).lpowder.x, 1.0);
    }

    /// LEVEL scales the direct light and the light from above and below; OCCLUDE scales the ambient's fall.
    #[test]
    fn the_level_and_occlusion_keys() {
        let at = |level: f32| lighting(&noon(START_LIGHT.with_weight(LightWeight::Level, level)));
        let (one, two) = (at(1.0), at(2.0));
        assert_eq!((one.bound, two.bound), (1.0, 1.0));
        for (a, b) in [(one.light, two.light), (one.lupper.truncate(), two.lupper.truncate()), (one.llower.truncate(), two.llower.truncate())] {
            assert!((b - 2.0 * a).abs().max_element() <= 1e-6 * b.max_element(), "{a} {b}");
        }
        for occ in [0.0f32, 1.0, 1.5, 4.0] {
            assert_eq!(lighting(&noon(START_LIGHT.with_weight(LightWeight::Occlude, occ))).lmode.z, AMBIENT_FALL * occ);
        }
    }
}
