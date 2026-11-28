//! Night sky detail (`night-sky-detail`, #2995; off in every preset): twinkling stars, a glow
//! round the moon and moonlit clouds, from the climate's own stars and moon and the shown
//! weather's own colours and fog. An engineering choice for the enhanced look [design], not a
//! fact about the original game and never used to explain a difference from it
//! (docs/design/graphics-enhanced.md §3.19).
//!
//! No pass, texture or mesh of its own: every part is drawn by the sky's own shader
//! (`sky.wesl`) from two fields of the sky parts' uniform (`sky::SkyLight::moon` and
//! `::night`), which this module writes per part (the star dome, the atmosphere dome, the
//! cloud layers) from the live [`SkyState`] every frame, only when a value moved. With the
//! switch off they stay zero and the shader skips every branch: the picture and the log are
//! as without the row.
//!
//! - **Twinkle** (stars): each star's added light × 1 + A · air(e) · s(dir, t), where s is
//!   lattice value noise in time (each corner of a 3D lattice over the dome direction a mean
//!   of two sines, 1 to 4 Hz, phases and rates from an integer hash; smoothstep-interpolated
//!   between corners, Ebert et al., "Texturing and Modeling: A Procedural Approach", lattice
//!   noises [documented: cited publication]) and air(e) = 1 - 1/X with X the air mass of
//!   Rozenberg's formula 1 / (sin e + 0.025 e^(-11 sin e)) (G. V. Rozenberg, "Twilight",
//!   1966 [documented: cited publication]): none at the zenith, most at the horizon. Time is
//!   Bevy's `globals.time`, the game's virtual clock wrapped at an hour (a paused game holds
//!   still), and every rate is a whole number of cycles an hour, so the wrap joins.
//! - **Moon glow** (dome, behind the clouds): a Henyey-Greenstein lobe (L. G. Henyey and
//!   J. L. Greenstein, "Diffuse radiation in the Galaxy", Astrophysical Journal 93, 1941
//!   [documented: cited publication]) about the moon disc's direction, divided by its peak,
//!   coloured by the weather's night sunlight colour (its moonlight), wider and stronger the
//!   hazier the weather's fog.
//! - **Moonlit clouds** (cloud layers): the cloud's own colour × a broader forward lobe
//!   toward the moon, bounded.
//!
//! Debug aid `NVF_PLAY_DEBUG_NIGHT_SKY=<multiplier>` (0 to 4) scales all three; it does not
//! turn the switch on.

use crate::sky::{CloudLayer, SkyDome, SkyMaterial, SkyState, Stars, MOON_HIDDEN_Y, SUN_DISTANCE};
use bevy::prelude::*;
use fnv_world::sky::{ColorType, MOON_PHASES};

// ---- Constants [engineering choice unless said otherwise] ----

/// The distance at which the weather's fog share is read as its haze: the desert haze's
/// (haze.rs `WEIGHT_REF_DISTANCE`), where the clear Mojave weather's night fog has 0.5.
pub const HAZE_REF_DISTANCE: f32 = 50_000.0;
/// The twinkle's amplitude in clear air and what a full fog share adds to it: 0.15 + 0.25 q,
/// so the clear Mojave night (q 0.5) twinkles ±0.27 at the horizon, ±0.14 at 30° up.
pub const TWINKLE_CLEAR: f32 = 0.15;
pub const TWINKLE_HAZE: f32 = 0.25;
/// The amplitude's bound (with the debug multiplier): a star never dims below a tenth.
pub const TWINKLE_MOST: f32 = 0.9;
/// The lattice cell: about the size of a star on screen, in pixels at 1080 lines with the
/// game's default field of view (75, fDefaultFOV; vertical 59.8°, `free_cam::vertical_fov`):
/// a star of the climate's star dome covers 2 to 4 pixels there [hypothesis], so 4 pixels
/// (3.9e-3 rad, 0.22°) gives neighbouring stars their own phases while a star's pixels share
/// nearly one (the interpolation leaves no edge between cells).
pub const TWINKLE_CELL_PIXELS: f32 = 4.0;
pub const REFERENCE_LINES: f32 = 1080.0;
pub const REFERENCE_FOV: f32 = 75.0;
/// Rozenberg's air mass constants (`air_weight`; the shader writes them in).
#[cfg(test)]
const AIR_A: f32 = 0.025;
#[cfg(test)]
const AIR_B: f32 = 11.0;
/// The moon glow at its centre (added to the dome's linear colour, as the sun's glare is) at
/// a full moon in clear air, and what a full fog share adds (× 1 + 0.5 q): at most 0.09.
pub const GLOW_PEAK: f32 = 0.06;
pub const GLOW_HAZE_GAIN: f32 = 0.5;
/// The glow's Henyey-Greenstein g in clear air and under a full fog share: half its peak at
/// about 4.6° from the moon's centre in clear air, about 12.7° under full haze (the disc is
/// 3.6° across, sky.rs `MOON_SIZE`).
pub const GLOW_G_CLEAR: f32 = 0.9;
pub const GLOW_G_HAZY: f32 = 0.75;
/// A moonlight colour whose largest channel is below this keeps its hue with its brightness
/// scaled down (so a black night sunlight gives no glow); at or above it, the hue alone.
pub const MOONLIGHT_FLOOR: f32 = 0.05;
/// The clouds' lobe (broad: the side of the cover facing the moon), how much of the cloud's
/// own colour it adds at its centre, and the most it adds (linear, like the glow).
pub const CLOUD_G: f32 = 0.6;
pub const CLOUD_GAIN: f32 = 0.5;
pub const CLOUD_PEAK: f32 = 0.06;
/// The debug aid and its largest multiplier.
pub const DEBUG_VAR: &str = "NVF_PLAY_DEBUG_NIGHT_SKY";
pub const DEBUG_MOST: f32 = 4.0;
/// A stored field is written again when it moved by more than this (as sky.rs's materials,
/// #897: each write remakes the uniform on the GPU).
const WRITE_TOLERANCE: f32 = 1e-3;

// ---- The pure parts ----

/// `x` held to 0..1; 0 for a value that is not a number.
fn unit(x: f32) -> f32 {
    if x.is_finite() { x.clamp(0.0, 1.0) } else { 0.0 }
}

/// The debug multiplier held to 0..[`DEBUG_MOST`]; 1 for a value that is not a number.
fn multiplier(m: f32) -> f32 {
    if m.is_finite() { m.clamp(0.0, DEBUG_MOST) } else { 1.0 }
}

/// The weather's haze (0..1): its fog share at [`HAZE_REF_DISTANCE`] by the curve the fog
/// shader draws (`fog::share`, with the near held at 0 or above and the far at least 1 beyond
/// it, as the fog uniform has them) × the fog's strength; 0 without fog.
pub fn haze_share(range: Option<(f32, f32, f32)>, power: f32) -> f32 {
    let Some((near, far, strength)) = range else { return 0.0 };
    if !(near.is_finite() && far.is_finite() && strength.is_finite()) {
        return 0.0;
    }
    let near = near.max(0.0);
    let far = far.max(near + 1.0);
    let power = if power.is_finite() { power } else { 1.0 };
    unit(crate::fog::share(near, far, power, HAZE_REF_DISTANCE) * strength.clamp(0.0, 1.0))
}

/// The lit share of the moon in phase `phase` (MOON_PHASES order: full, three quarters,
/// half and a quarter waning, new, a quarter, half and three quarters waxing): 1 at the full
/// moon, 0 at the new moon.
pub fn lit_share(phase: usize) -> f32 {
    let n = MOON_PHASES.len();
    let i = phase % n;
    1.0 - i.min(n - i) as f32 / (n / 2) as f32
}

/// The twinkle's amplitude for haze `haze` and the debug multiplier `m`.
pub fn twinkle_amplitude(haze: f32, m: f32) -> f32 {
    ((TWINKLE_CLEAR + TWINKLE_HAZE * unit(haze)) * multiplier(m)).clamp(0.0, TWINKLE_MOST)
}

/// The share of the amplitude at a direction whose height (sine of its elevation) is `up`:
/// 1 - 1/X, X Rozenberg's air mass; 0 at the zenith, 0.975 at and below the horizon; what
/// sky.wesl `night_twinkle` computes (the tests' mirror).
#[cfg(test)]
pub fn air_weight(up: f32) -> f32 {
    let u = unit(up);
    (1.0 - (u + AIR_A * (-AIR_B * u).exp())).clamp(0.0, 1.0)
}

/// Lattice cells per unit of direction (sky.wesl `night.y`): one over the cell's angle.
pub fn twinkle_cells() -> f32 {
    let pixel = nvf_play::free_cam::vertical_fov(REFERENCE_FOV) / REFERENCE_LINES;
    1.0 / (TWINKLE_CELL_PIXELS * pixel)
}

/// The glow's g for haze `haze`.
pub fn glow_g(haze: f32) -> f32 {
    GLOW_G_CLEAR + (GLOW_G_HAZY - GLOW_G_CLEAR) * unit(haze)
}

/// The glow at its centre for haze `haze` (before the moon's strength).
pub fn glow_peak(haze: f32) -> f32 {
    GLOW_PEAK * (1.0 + GLOW_HAZE_GAIN * unit(haze))
}

/// The Henyey-Greenstein lobe of `g` at the cosine `cos` from its axis over its value on the
/// axis: (1 - g)³ / (1 + g² - 2 g cos)^1.5, 1 on the axis; what sky.wesl `night_moon` takes
/// (the tests' mirror). The denominator is written (1 - g)² + 2 g (1 - cos), which keeps it
/// exact on the axis (1 + g² - 2 g cos cancels in f32 there).
#[cfg(test)]
pub fn lobe(cos: f32, g: f32) -> f32 {
    let g = if g.is_finite() { g.clamp(0.0, 0.99) } else { 0.0 };
    let cos = if cos.is_finite() { cos.clamp(-1.0, 1.0) } else { 0.0 };
    let d = ((1.0 - g) * (1.0 - g) + 2.0 * g * (1.0 - cos)).max(1e-6);
    ((1.0 - g) * (1.0 - g) / d).min(1.0).powf(1.5)
}

/// The moon lobe's axis as sky.wesl `night_moon` takes it (the tests' mirror): from the
/// rendering `eye` to the disc, drawn at the sky's `centre` (the main camera, which every sky
/// part follows) + `moon` (the uniform's `moon.xyz`). For the main view the eye is the centre
/// and this is the moon's direction; for the water reflection's mirrored eye it is still the
/// way to the disc that eye sees.
#[cfg(test)]
pub fn glow_axis(centre: Vec3, moon: Vec3, eye: Vec3) -> Vec3 {
    (centre + moon - eye).normalize_or_zero()
}

/// The direction the star lattice is read along (sky.wesl `night_twinkle`'s argument, the
/// tests' mirror): from the star dome's centre, not the eye, so an eye anywhere (the water
/// reflection's mirrored one at any height) gives a star the same cell and phase.
#[cfg(test)]
pub fn star_direction(fragment: Vec3, centre: Vec3) -> Vec3 {
    (fragment - centre).normalize_or_zero()
}

/// The angle (degrees) from the axis at which the lobe of `g` falls to half (for the log).
pub fn half_width_deg(g: f32) -> f32 {
    let g = g.clamp(0.01, 0.99);
    // (1 - g)² / d = 2^(-2/3) at half: d = (1 - g)² 2^(2/3).
    let d = (1.0 - g) * (1.0 - g) * 2f32.powf(2.0 / 3.0);
    ((1.0 + g * g - d) / (2.0 * g)).clamp(-1.0, 1.0).acos().to_degrees()
}

/// The moon's strength (0..1): its visibility × the phase's lit share, none while the disc is
/// hidden below the horizon rule (`sky::update_night_sky`).
pub fn moon_strength(moon_vis: f32, lit: f32, moon_y: f32) -> f32 {
    if !moon_y.is_finite() || moon_y < MOON_HIDDEN_Y {
        return 0.0;
    }
    unit(moon_vis) * unit(lit)
}

/// The glow's colour from the weather's night sunlight colour (linear): its hue at full
/// brightness (largest channel 1), dimmer when the colour itself is darker than
/// [`MOONLIGHT_FLOOR`]; never negative.
pub fn moonlight_hue(c: [f32; 3]) -> Vec3 {
    let c = Vec3::from_array(c.map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 }));
    c / c.max_element().max(MOONLIGHT_FLOOR)
}

/// What the detail reads from the sky.
#[derive(Clone, Copy, Debug)]
pub struct Inputs {
    pub has_moon: bool,
    /// `SkyState::night_visibility().1`.
    pub moon_vis: f32,
    /// The direction to the moon disc (Bevy axes): `-SkyState::sun_dir()`.
    pub moon_dir: Vec3,
    /// `SkyState::moon_phase()`.
    pub phase: usize,
    /// Fog near, far and strength as the fog uniform takes them (`--fog` or the weather's).
    pub fog: Option<(f32, f32, f32)>,
    pub fog_power: f32,
    /// The weather's sunlight colour now (linear; at night its moonlight).
    pub moonlight: [f32; 3],
    /// The debug multiplier.
    pub multiplier: f32,
}

/// The fields the sky parts get (sky.rs `SkyLight::moon` and `::night`), and the figures for
/// the log.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Detail {
    /// `night` of the star parts (x amplitude, y cells); their `moon` stays 0.
    pub stars: Vec4,
    pub dome_moon: Vec4,
    pub dome_night: Vec4,
    pub cloud_moon: Vec4,
    pub cloud_night: Vec4,
    pub haze: f32,
    pub lit: f32,
    /// The moon's strength (with the multiplier).
    pub strength: f32,
}

/// The detail from the sky now (outdoors).
pub fn detail(i: &Inputs) -> Detail {
    let haze = haze_share(i.fog, i.fog_power);
    let m = multiplier(i.multiplier);
    let amplitude = twinkle_amplitude(haze, m);
    let stars = if amplitude > 0.0 { Vec4::new(amplitude, twinkle_cells(), 0.0, 0.0) } else { Vec4::ZERO };
    let lit = lit_share(i.phase);
    let dir = i.moon_dir.normalize_or_zero();
    let strength = if i.has_moon && dir != Vec3::ZERO { moon_strength(i.moon_vis, lit, dir.y) * m } else { 0.0 };
    let mut d = Detail { stars, haze, lit, strength, ..default() };
    if strength > 0.0 {
        let glow = moonlight_hue(i.moonlight) * glow_peak(haze) * strength;
        let cap = glow.max_element();
        if cap > 0.0 {
            d.dome_moon = (dir * SUN_DISTANCE).extend(glow_g(haze));
            d.dome_night = glow.extend(cap);
        }
        d.cloud_moon = (dir * SUN_DISTANCE).extend(CLOUD_G);
        d.cloud_night = Vec3::splat(CLOUD_GAIN * strength).extend(CLOUD_PEAK * strength);
    }
    d
}

/// The twinkle factor along `dir` at `hour` (the clock's fraction of its hour, 0..1) for the
/// `night` field of a star part: sky.wesl `night_twinkle` in Rust (the tests' mirror).
#[cfg(test)]
pub fn twinkle_factor(dir: Vec3, hour: f32, night: Vec4) -> f32 {
    fn mix(x: u32) -> u32 {
        let mut h = x;
        h ^= h >> 16;
        h = h.wrapping_mul(0x85eb_ca6b);
        h ^= h >> 13;
        h = h.wrapping_mul(0xc2b2_ae35);
        h ^ (h >> 16)
    }
    fn wave(c: IVec3, hour: f32) -> f32 {
        let u = c.as_uvec3();
        let h = mix(u.x.wrapping_mul(73_856_093) ^ u.y.wrapping_mul(19_349_663) ^ u.z.wrapping_mul(83_492_791));
        let r = mix(h ^ 0x9e37_79b9);
        let k1 = (3600 + r % 10801) as f32;
        let k2 = (3600 + (r >> 16) % 10801) as f32;
        let p1 = (h & 0xffff) as f32 / 65536.0;
        let p2 = (h >> 16) as f32 / 65536.0;
        let tau = std::f32::consts::TAU;
        0.5 * ((tau * (k1 * hour + p1).fract()).sin() + (tau * (k2 * hour + p2).fract()).sin())
    }
    let p = dir * night.y;
    let cell = p.floor();
    let f = p - cell;
    let w = f * f * (Vec3::splat(3.0) - 2.0 * f);
    let c0 = cell.as_ivec3();
    let mut s = 0.0;
    for i in 0..8 {
        let o = IVec3::new(i & 1, (i >> 1) & 1, (i >> 2) & 1);
        let pick = |k: i32, wk: f32| if k == 1 { wk } else { 1.0 - wk };
        s += pick(o.x, w.x) * pick(o.y, w.y) * pick(o.z, w.z) * wave(c0 + o, hour);
    }
    1.0 + night.x.clamp(0.0, 1.0) * air_weight(dir.y) * s
}

/// The debug multiplier from the variable's text: `None` for unset or empty (as designed),
/// else a finite number held to 0..[`DEBUG_MOST`]; `Err` for anything else.
pub fn debug_multiplier(text: Option<&str>) -> Result<Option<f32>, String> {
    let Some(t) = text.map(str::trim).filter(|t| !t.is_empty()) else { return Ok(None) };
    match t.parse::<f32>() {
        Ok(v) if v.is_finite() => Ok(Some(v.clamp(0.0, DEBUG_MOST))),
        _ => Err(format!("{DEBUG_VAR}={t}: expected a number (the night sky detail's multiplier)")),
    }
}

/// The detail's figures for the log.
fn describe(weather: Option<&str>, has_moon: bool, phase: usize, d: &Detail, m: f32) -> String {
    let debug = if m != 1.0 { format!(", debug x{m}") } else { String::new() };
    let moon = if has_moon {
        format!(
            "moon {} lit {:.2}, strength {:.2}, glow {:.3} half width {:.1}°",
            MOON_PHASES[phase % MOON_PHASES.len()],
            d.lit,
            d.strength,
            d.dome_night.w,
            half_width_deg(glow_g(d.haze))
        )
    } else {
        "no moon".into()
    };
    format!("{}: haze {:.2}, twinkle {:.2}, {moon}{debug}", weather.unwrap_or("the weather"), d.haze, d.stars.x)
}

// ---- Following the sky ----

pub struct NightSkyPlugin;

impl Plugin for NightSkyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, update_detail.after(crate::sky::update_night_sky));
    }
}

/// What the log last said.
struct Logged {
    weather: Option<String>,
    haze: f32,
    lit: f32,
}

#[derive(Default)]
struct DetailState {
    /// Fields other than zero may be in the materials (to be cleared when it goes off).
    shown: bool,
    /// On outdoors (for the on and off lines).
    active: bool,
    /// The debug multiplier, read once.
    debug: Option<f32>,
    logged: Option<Logged>,
    /// What the last detail was worked from and the detail: worked again only when one of
    /// them changed.
    derived: Option<(Key, Detail)>,
    /// The line saying the parts took the detail has been written since it turned on.
    applied: bool,
}

/// What the detail depends on in the sky: climate, weather, the one fading out, the fade,
/// the hour, the days passed (the phase), `--fog`.
type Key = (u32, u32, Option<u32>, f32, f32, f32, Option<(f32, f32)>);

/// Every frame after the sky's parts: with the switch on, outdoors, the detail from the sky
/// as it is now into each part's uniform where it moved; otherwise zero (once).
#[allow(clippy::type_complexity)]
fn update_detail(
    graphics: Option<Res<crate::graphics::Graphics>>,
    sky: Option<Res<SkyState>>,
    parts: Query<(&MeshMaterial3d<SkyMaterial>, Has<Stars>, Has<SkyDome>), Or<(With<Stars>, With<SkyDome>, With<CloudLayer>)>>,
    mut materials: ResMut<Assets<SkyMaterial>>,
    mut state: Local<DetailState>,
) {
    let wanted = graphics.as_deref().is_some_and(|g| g.night_sky_detail);
    let indoors = sky.as_deref().is_none_or(|s| s.interior.is_some());
    let d = match sky.as_deref().filter(|_| wanted && !indoors) {
        None => {
            if state.active {
                state.active = false;
                state.logged = None;
                state.derived = None;
                state.applied = false;
                info!("night sky: detail off{}", if wanted && indoors { " indoors" } else { "" });
            }
            if !state.shown {
                return;
            }
            Detail::default()
        }
        Some(sky) => {
            // Read once; the tests' runs keep the multiplier at 1 whatever the environment holds.
            let m = *state.debug.get_or_insert_with(|| {
                let text = if cfg!(test) { None } else { std::env::var(DEBUG_VAR).ok() };
                match debug_multiplier(text.as_deref()) {
                    Ok(Some(m)) => {
                        info!("debug switch {DEBUG_VAR}: the night sky detail x{m}");
                        m
                    }
                    Ok(None) => 1.0,
                    Err(e) => {
                        warn!("{e}; the night sky detail as designed");
                        1.0
                    }
                }
            });
            let key: Key = (sky.climate.form_id, sky.weather.form_id, sky.prev.as_ref().map(|p| p.form_id), sky.fade, sky.hour, sky.days_passed, sky.fog_override);
            if let Some((_, d)) = state.derived.filter(|(k, _)| *k == key) {
                d
            } else {
                let has_moon = sky.climate.has_moon();
                let phase = sky.moon_phase();
                let inputs = Inputs {
                    has_moon,
                    moon_vis: sky.night_visibility().1,
                    moon_dir: -sky.sun_dir(),
                    phase,
                    fog: sky.fog_override.map(|(n, f)| (n, f, 1.0)).or_else(|| sky.fog()),
                    fog_power: sky.fog_power(),
                    moonlight: sky.color(ColorType::Sunlight),
                    multiplier: m,
                };
                let d = detail(&inputs);
                // The log: on, when the lit share changes, and when the weather or its haze moves
                // notably (not during a time-lapse).
                let name = sky.weather.edid.as_deref();
                let line = if !state.active {
                    state.active = true;
                    Some(format!("night sky: detail on: {}", describe(name, has_moon, phase, &d, m)))
                } else {
                    let notable = state.logged.as_ref().is_none_or(|l| {
                        (has_moon && l.lit != d.lit) || (sky.time_scale <= 0.0 && (l.weather.as_deref() != name || (l.haze - d.haze).abs() >= 0.1))
                    });
                    notable.then(|| format!("night sky: {}", describe(name, has_moon, phase, &d, m)))
                };
                if let Some(line) = line {
                    info!("{line}");
                    state.logged = Some(Logged { weather: name.map(str::to_string), haze: d.haze, lit: d.lit });
                }
                state.derived = Some((key, d));
                d
            }
        }
    };
    for (m, star, dome) in &parts {
        let (moon, night) = match (star, dome) {
            (true, _) => (Vec4::ZERO, d.stars),
            (_, true) => (d.dome_moon, d.dome_night),
            _ => (d.cloud_moon, d.cloud_night),
        };
        let Some(mat) = materials.get(&m.0) else { continue };
        let l = mat.extension.light;
        if same_night(l.night, night) && same_moon(l.moon, moon) {
            continue;
        }
        if let Some(mut mat) = materials.get_mut(&m.0) {
            mat.extension.light.moon = moon;
            mat.extension.light.night = night;
        }
    }
    state.shown = d != Detail::default();
    // Once each time it turns on: the stars' materials hold an amplitude and the dome's a glow,
    // read back from the materials after the writes (a smoke test's proof that the values
    // reached the parts, not only the log). The cloud count is of materials, hidden layers
    // included.
    // Only while it is still owed and this frame's values can give it (not by day, at a new
    // moon, without a moon or at the multiplier 0).
    if state.active && !state.applied && d.stars.x > 0.0 && d.dome_night.w > 0.0 {
        let (mut twinkling, mut moonlit, mut amplitude, mut glow) = (0, 0, 0.0f32, 0.0f32);
        for (m, star, dome) in &parts {
            let Some(night) = materials.get(&m.0).map(|mat| mat.extension.light.night) else { continue };
            match (star, dome) {
                (true, _) if night.x > 0.0 => {
                    twinkling += 1;
                    amplitude = night.x;
                }
                (false, true) => glow = glow.max(night.w),
                (false, false) if night.w > 0.0 => moonlit += 1,
                _ => {}
            }
        }
        if twinkling > 0 && glow > 0.0 {
            state.applied = true;
            info!("night sky: applied: {twinkling} star material(s) twinkling (amplitude {amplitude:.2}), the dome's glow {glow:.3}, {moonlit} cloud material(s) moonlit");
        }
    }
}

/// Whether a stored `night` field may stay for `want`: exactly zero when zero is wanted (the
/// shader's branches test for above 0), else within [`WRITE_TOLERANCE`].
fn same_night(stored: Vec4, want: Vec4) -> bool {
    if want == Vec4::ZERO { stored == Vec4::ZERO } else { stored.abs_diff_eq(want, WRITE_TOLERANCE) }
}

/// The same for a `moon` field, whose xyz is the disc's place [`SUN_DISTANCE`] out: within
/// that many times the tolerance there (the same angle as 1e-3 of a unit direction), within
/// the tolerance in w (the lobe's g).
fn same_moon(stored: Vec4, want: Vec4) -> bool {
    if want == Vec4::ZERO {
        return stored == Vec4::ZERO;
    }
    stored.truncate().abs_diff_eq(want.truncate(), WRITE_TOLERANCE * SUN_DISTANCE) && (stored.w - want.w).abs() <= WRITE_TOLERANCE
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clear night: the clear Mojave weather's night fog (0..200,000, power 0.5) and a
    /// bluish moonlight, the moon high, full.
    fn night() -> Inputs {
        Inputs {
            has_moon: true,
            moon_vis: 1.0,
            moon_dir: Vec3::new(0.3, 0.8, -0.2),
            phase: 0,
            fog: Some((0.0, 200_000.0, 1.0)),
            fog_power: 0.5,
            moonlight: [0.12, 0.14, 0.2],
            multiplier: 1.0,
        }
    }

    fn finite(v: Vec4) -> bool {
        v.is_finite()
    }

    fn all_finite(d: &Detail) -> bool {
        [d.stars, d.dome_moon, d.dome_night, d.cloud_moon, d.cloud_night].iter().all(|v| finite(*v)) && d.haze.is_finite() && d.lit.is_finite() && d.strength.is_finite()
    }

    #[test]
    fn the_lit_share_follows_the_phase_order() {
        let want = [1.0, 0.75, 0.5, 0.25, 0.0, 0.25, 0.5, 0.75];
        for (i, w) in want.iter().enumerate() {
            assert_eq!(lit_share(i), *w, "{}", MOON_PHASES[i]);
            assert_eq!(lit_share(i + 8), *w, "wraps");
        }
        assert_eq!(MOON_PHASES[0], "full");
        assert_eq!(MOON_PHASES[4], "new");
        // A climate with a phase length of 0 shows the full moon (fnv_world::sky::moon_phase).
        assert_eq!(lit_share(fnv_world::sky::moon_phase(17.0, 0)), 1.0);
    }

    #[test]
    fn the_haze_is_the_fog_share_at_the_reference_and_stays_in_bounds() {
        // The clear Mojave night: 0.5 (haze.rs's figure for the same weather).
        assert!((haze_share(Some((0.0, 200_000.0, 1.0)), 0.5) - 0.5).abs() < 1e-3);
        // No fog, or fading out.
        assert_eq!(haze_share(None, 1.0), 0.0);
        assert!((haze_share(Some((0.0, 200_000.0, 0.5)), 0.5) - 0.25).abs() < 1e-3);
        // Extremes: far 0 (held to near + 1: all fog), far a million, near above far, power 0,
        // values that are not numbers.
        for (range, power) in [
            (Some((0.0, 0.0, 1.0)), 1.0),
            (Some((0.0, 1e6, 1.0)), 1.0),
            (Some((0.0, 1e6, 1.0)), 0.0),
            (Some((5e5, 100.0, 1.0)), 0.3),
            (Some((-1e6, 1e6, 1.0)), -2.0),
            (Some((0.0, 1e6, 1.0)), 1e6),
            (Some((f32::NAN, 1e6, 1.0)), 1.0),
            (Some((0.0, f32::INFINITY, 1.0)), 1.0),
            (Some((0.0, 1e6, f32::NAN)), 1.0),
            (Some((0.0, 1e6, 1.0)), f32::NAN),
        ] {
            let h = haze_share(range, power);
            assert!(h.is_finite() && (0.0..=1.0).contains(&h), "{range:?} {power}: {h}");
        }
        assert_eq!(haze_share(Some((0.0, 0.0, 1.0)), 1.0), 1.0);
        assert!(haze_share(Some((0.0, 1e6, 1.0)), 1.0) < 0.06);
        // Denser fog, more haze.
        assert!(haze_share(Some((0.0, 100_000.0, 1.0)), 1.0) > haze_share(Some((0.0, 200_000.0, 1.0)), 1.0));
    }

    #[test]
    fn the_air_weight_is_none_at_the_zenith_and_grows_toward_the_horizon() {
        assert_eq!(air_weight(1.0), 0.0);
        assert!((air_weight(0.0) - 0.975).abs() < 1e-6);
        assert!((air_weight(-0.5) - 0.975).abs() < 1e-6, "below the horizon as at it");
        assert_eq!(air_weight(f32::NAN), air_weight(0.0));
        let mut last = 0.0;
        for i in (0..=100).rev() {
            let w = air_weight(i as f32 / 100.0);
            assert!(w >= last - 1e-6 && (0.0..1.0).contains(&w), "{i}: {w}");
            last = w;
        }
    }

    #[test]
    fn the_twinkle_has_mean_one_and_stays_within_its_amplitude() {
        let a = twinkle_amplitude(1.0, DEBUG_MOST);
        assert_eq!(a, TWINKLE_MOST);
        let night = Vec4::new(a, twinkle_cells(), 0.0, 0.0);
        // Whole cycles an hour: over the wrap sampled evenly (more samples than the fastest
        // rate's cycles) each sine sums to 0, so the mean is 1.
        let n = 32_768;
        for dir in [Vec3::X, Vec3::new(0.3, 0.2, -0.93).normalize(), Vec3::new(-0.7, 0.05, 0.7).normalize(), Vec3::NEG_Z] {
            let mut sum = 0.0f64;
            for j in 0..n {
                let f = twinkle_factor(dir, j as f32 / n as f32, night);
                assert!(f.is_finite() && f >= 1.0 - a - 1e-5 && f <= 1.0 + a + 1e-5, "{dir:?} {j}: {f}");
                sum += f as f64;
            }
            let mean = sum / n as f64;
            assert!((mean - 1.0).abs() < 2e-3, "{dir:?}: mean {mean}");
        }
        // The zenith does not twinkle; the horizon (elevation 0) does, within 0.975 × a.
        for j in 0..100 {
            assert_eq!(twinkle_factor(Vec3::Y, j as f32 / 100.0, night), 1.0);
        }
        let swing = (0..400).map(|j| twinkle_factor(Vec3::X, j as f32 / 400.0, night)).fold((f32::MAX, f32::MIN), |(lo, hi), f| (lo.min(f), hi.max(f)));
        assert!(swing.1 - swing.0 > 0.2, "the horizon twinkles: {swing:?}");
        assert!(swing.0 >= 1.0 - 0.975 * a - 1e-5 && swing.1 <= 1.0 + 0.975 * a + 1e-5);
        // Off: 1 everywhere.
        assert_eq!(twinkle_factor(Vec3::X, 0.3, Vec4::ZERO.with_y(twinkle_cells())), 1.0);
    }

    #[test]
    fn the_twinkle_is_smooth_in_time_and_across_cells() {
        let night = Vec4::new(0.5, twinkle_cells(), 0.0, 0.0);
        let dir = Vec3::new(0.9, 0.1, 0.42).normalize();
        // One frame at 60 Hz is 1/216,000 of the hour: the factor moves little (no per-frame
        // noise); the fastest rate (4 Hz) moves at most 2π·4/60 of a sine of amplitude 0.5.
        let step = 1.0 / (60.0 * 3600.0);
        for j in 0..200 {
            let t = 0.25 + j as f32 * step;
            let d = (twinkle_factor(dir, t + step, night) - twinkle_factor(dir, t, night)).abs();
            assert!(d < 0.5 * 0.42 + 0.02, "{j}: {d}");
        }
        // Across a cell border along the sky the factor has no jump.
        let cells = night.y;
        let border = (dir.x * cells).floor() + 1.0;
        let before = Vec3::new((border - 1e-3) / cells, dir.y, dir.z);
        let after = Vec3::new((border + 1e-3) / cells, dir.y, dir.z);
        assert!((twinkle_factor(before, 0.4, night) - twinkle_factor(after, 0.4, night)).abs() < 0.01);
    }

    #[test]
    fn the_cell_is_a_few_pixels_at_1080_lines() {
        let angle = 1.0 / twinkle_cells();
        assert!((3.5e-3..4.3e-3).contains(&angle), "{angle}");
    }

    #[test]
    fn the_glow_widens_and_strengthens_with_haze_within_its_bounds() {
        assert_eq!(glow_g(0.0), GLOW_G_CLEAR);
        assert_eq!(glow_g(1.0), GLOW_G_HAZY);
        assert_eq!(glow_peak(0.0), GLOW_PEAK);
        assert!(glow_peak(f32::NAN) == GLOW_PEAK && glow_peak(10.0) <= GLOW_PEAK * (1.0 + GLOW_HAZE_GAIN));
        let (clear, hazy) = (half_width_deg(glow_g(0.0)), half_width_deg(glow_g(1.0)));
        assert!((4.0..5.5).contains(&clear) && (11.0..14.0).contains(&hazy), "{clear} {hazy}");
        for g in [0.0, 0.3, 0.6, 0.9, 0.99, 1.5, -1.0, f32::NAN] {
            assert!((lobe(1.0, g) - 1.0).abs() < 1e-5, "{g}: 1 on the axis");
            let mut last = 1.0;
            for i in 0..=200 {
                let c = 1.0 - i as f32 / 100.0;
                let v = lobe(c, g);
                assert!(v.is_finite() && (0.0..=1.0).contains(&v) && v <= last + 1e-6, "{g} {c}: {v}");
                last = v;
            }
        }
    }

    #[test]
    fn a_clear_full_moon_night_gives_every_part_its_bounded_values() {
        let d = detail(&night());
        assert!(all_finite(&d));
        assert!((d.haze - 0.5).abs() < 1e-3);
        assert_eq!(d.lit, 1.0);
        assert_eq!(d.strength, 1.0);
        // Twinkle on the stars only.
        assert!(d.stars.x > 0.2 && d.stars.x < 0.3 && d.stars.y > 200.0);
        // The glow: the moonlight's hue at the peak, toward the disc.
        let peak = glow_peak(d.haze);
        assert!((d.dome_night.w - peak).abs() < 1e-6 && d.dome_night.w <= GLOW_PEAK * (1.0 + GLOW_HAZE_GAIN));
        assert!((d.dome_night.z - peak).abs() < 1e-6, "blue is the moonlight's largest channel");
        assert!(d.dome_night.x < d.dome_night.z);
        // The disc's place from the sky's centre (sky.rs places it SUN_DISTANCE out).
        assert!((d.dome_moon.truncate() / SUN_DISTANCE - night().moon_dir.normalize()).length() < 1e-6);
        assert_eq!(d.cloud_moon.truncate(), d.dome_moon.truncate());
        assert!(d.dome_moon.w > GLOW_G_HAZY && d.dome_moon.w < GLOW_G_CLEAR);
        // The clouds: their colour × the gain, capped.
        assert_eq!(d.cloud_night, Vec4::new(CLOUD_GAIN, CLOUD_GAIN, CLOUD_GAIN, CLOUD_PEAK));
        assert_eq!(d.cloud_moon.w, CLOUD_G);
    }

    #[test]
    fn nothing_of_the_moon_by_day_while_hidden_or_at_the_new_moon() {
        let mut i = night();
        i.moon_vis = 0.0;
        let d = detail(&i);
        assert_eq!((d.dome_moon, d.dome_night, d.cloud_moon, d.cloud_night), (Vec4::ZERO, Vec4::ZERO, Vec4::ZERO, Vec4::ZERO));
        assert!(d.stars.x > 0.0, "the twinkle does not depend on the moon");
        let mut i = night();
        i.moon_dir = Vec3::new(0.0, -0.2, 1.0);
        assert_eq!(detail(&i).strength, 0.0, "the disc below the horizon rule");
        let mut i = night();
        i.phase = 4;
        assert_eq!(detail(&i).dome_night, Vec4::ZERO, "new moon");
        let mut i = night();
        i.phase = 2;
        assert_eq!(detail(&i).strength, 0.5, "half");
        // A climate without a moon: twinkle only.
        let mut i = night();
        i.has_moon = false;
        let d = detail(&i);
        assert!(d.stars.x > 0.0 && d.dome_night == Vec4::ZERO && d.cloud_night == Vec4::ZERO);
        // A black night sunlight: no glow, the clouds still take theirs.
        let mut i = night();
        i.moonlight = [0.0; 3];
        let d = detail(&i);
        assert_eq!(d.dome_night, Vec4::ZERO);
        assert!(d.cloud_night.w > 0.0);
        // No direction: nothing of the moon.
        let mut i = night();
        i.moon_dir = Vec3::ZERO;
        assert_eq!(detail(&i).strength, 0.0);
    }

    #[test]
    fn the_sun_at_the_horizon_and_a_synthetic_extreme_weather_stay_finite() {
        // The sun exactly at the horizon: the moon's visibility as SkyState gives it there and
        // a direction exactly on the horizon (elevation 0).
        let moon_vis = fnv_world::sky::smoothstep(5.0, -6.0, 0.0);
        let mut i = night();
        i.moon_vis = moon_vis;
        i.moon_dir = Vec3::new(1.0, 0.0, 0.0);
        let d = detail(&i);
        assert!(all_finite(&d) && d.strength > 0.0 && d.strength < 1.0);
        // The moon straight overhead (elevation 90).
        i.moon_dir = Vec3::Y;
        assert!(all_finite(&detail(&i)));
        // Extremes: fog far 0 and a million, power 0, a huge moonlight, values that are not
        // numbers, the largest multiplier and above.
        for (fog, power, light, m) in [
            (Some((0.0, 0.0, 1.0)), 0.0, [1e6, 0.0, 3.0], 4.0),
            (Some((0.0, 1e6, 1.0)), 1.0, [f32::NAN, -1.0, 0.5], 100.0),
            (None, f32::NAN, [0.0; 3], f32::NAN),
            (Some((-5.0, 1e6, 0.3)), 0.0, [255.0; 3], 0.0),
        ] {
            let mut i = night();
            i.fog = fog;
            i.fog_power = power;
            i.moonlight = light;
            i.multiplier = m;
            let d = detail(&i);
            assert!(all_finite(&d), "{fog:?} {power} {light:?} {m}: {d:?}");
            for v in [d.stars, d.dome_night, d.cloud_night] {
                assert!(v.min_element() >= 0.0, "{v:?}");
            }
            assert!(d.stars.x <= TWINKLE_MOST);
            assert!(d.dome_night.w <= GLOW_PEAK * (1.0 + GLOW_HAZE_GAIN) * DEBUG_MOST + 1e-6);
            assert!(d.cloud_night.w <= CLOUD_PEAK * DEBUG_MOST + 1e-6);
            assert!(d.dome_moon.w < 1.0);
        }
        // The multiplier 0 turns all three off.
        let mut i = night();
        i.multiplier = 0.0;
        let d = detail(&i);
        assert_eq!((d.stars, d.dome_night, d.cloud_night), (Vec4::ZERO, Vec4::ZERO, Vec4::ZERO));
    }

    /// The water reflection's camera is the main camera mirrored in the water plane
    /// (reflection.rs), while the sky parts and the moon disc stay centred on the main camera.
    #[test]
    fn a_mirrored_eye_sees_the_glow_round_the_disc_and_the_stars_unchanged() {
        let water = 120.0;
        let eye = Vec3::new(3_000.0, water + 5_000.0, -8_000.0);
        let mirrored = Vec3::new(eye.x, 2.0 * water - eye.y, eye.z);
        let e = 10f32.to_radians();
        let m = Vec3::new(e.cos() * 0.6, e.sin(), e.cos() * 0.8);
        let disc = eye + m * SUN_DISTANCE;
        // The main view: the axis is the moon's direction.
        assert!((glow_axis(eye, m * SUN_DISTANCE, eye) - m).length() < 1e-5);
        // The mirrored eye: the axis points at the disc it sees; the moon's own direction
        // would miss it by about the clear-air glow's half width.
        let seen = (disc - mirrored).normalize();
        assert!((glow_axis(eye, m * SUN_DISTANCE, mirrored) - seen).length() < 1e-5);
        let miss = m.dot(seen).clamp(-1.0, 1.0).acos().to_degrees();
        assert!(miss > 3.5 && miss < 4.5, "{miss}");
        // A star of the dome reads the same lattice cell from either eye.
        let s = Vec3::new(-0.2, 0.3, 0.93).normalize();
        let star = eye + s * 0.9 * 175_000.0;
        assert!((star_direction(star, eye) - s).length() < 1e-5);
        let night = Vec4::new(0.5, twinkle_cells(), 0.0, 0.0);
        for t in [0.1, 0.37, 0.8] {
            assert!((twinkle_factor(star_direction(star, eye), t, night) - twinkle_factor(s, t, night)).abs() < 1e-3);
        }
        assert!((star - mirrored).normalize().dot(s) < 0.9999, "measured from the eye it would move");
    }

    /// A weather record whose fog ranges are both unset (0..0) has no fog (`Weather::fog_at`),
    /// so the live sky gives no haze; an explicit `--fog 0,0` (the case above) is all fog.
    #[test]
    fn a_weather_with_both_fog_ranges_unset_has_no_haze() {
        let mut sky = crate::sky::authored_session_sky();
        sky.weather.fog = [0.0; 6];
        sky.hour = 23.0;
        assert_eq!(sky.fog(), None);
        assert_eq!(haze_share(sky.fog_override.map(|(n, f)| (n, f, 1.0)).or_else(|| sky.fog()), sky.fog_power()), 0.0);
        let mut i = night();
        i.fog = sky.fog();
        i.fog_power = sky.fog_power();
        let d = detail(&i);
        assert_eq!(d.haze, 0.0);
        assert!((d.stars.x - TWINKLE_CLEAR).abs() < 1e-6);
        assert_eq!(d.dome_moon.w, GLOW_G_CLEAR);
    }

    /// A night sky with a moon: sunrise 6-7, sunset 18-19, 23:00, full moon, fog 0..200,000.
    fn moonlit_sky() -> SkyState {
        let mut sky = crate::sky::authored_session_sky();
        sky.climate.form_id = 7;
        sky.climate.moons = 0x83;
        sky.climate.sunrise = (6.0, 7.0);
        sky.climate.sunset = (18.0, 19.0);
        sky.hour = 23.0;
        sky.weather.form_id = 1;
        sky.weather.fog = [0.0, 200_000.0, 0.0, 200_000.0, 1.0, 1.0];
        sky.weather.colors = vec![vec![[60, 70, 100]; 6]; 10];
        sky
    }

    fn part_material(app: &mut App) -> Handle<SkyMaterial> {
        let m = SkyMaterial { base: StandardMaterial::default(), extension: crate::sky::SkyDisplay::default() };
        app.world_mut().resource_mut::<Assets<SkyMaterial>>().add(m)
    }

    fn light(app: &App, h: &Handle<SkyMaterial>) -> crate::sky::SkyLight {
        app.world().resource::<Assets<SkyMaterial>>().get(h).expect("material").extension.light
    }

    fn set_on(app: &mut App, on: bool) {
        app.world_mut().resource_mut::<crate::graphics::Graphics>().night_sky_detail = on;
    }

    /// The derivation is kept between frames: a part made later takes the kept values, a
    /// weather fade works them out again, and off clears every part (on fills them again).
    #[test]
    fn the_kept_detail_fills_new_parts_follows_a_fade_and_clears_when_off() {
        let mut app = App::new();
        let mut g = crate::graphics::Graphics::preset(crate::graphics::Preset::Vanilla);
        g.night_sky_detail = true;
        app.insert_resource(g).insert_resource(moonlit_sky()).init_resource::<Assets<SkyMaterial>>().add_systems(Update, update_detail);
        let (star, dome, cloud) = (part_material(&mut app), part_material(&mut app), part_material(&mut app));
        app.world_mut().spawn((Stars, MeshMaterial3d(star.clone())));
        app.world_mut().spawn((SkyDome::for_test(), MeshMaterial3d(dome.clone())));
        app.world_mut().spawn((CloudLayer::for_test(0), MeshMaterial3d(cloud.clone())));
        app.update();
        let first = light(&app, &star);
        assert!(first.night.x > 0.0 && first.moon == Vec4::ZERO, "{first:?}");
        let dome_first = light(&app, &dome);
        assert!(dome_first.night.w > 0.0 && dome_first.moon.y > 0.0, "{dome_first:?}");
        assert!(light(&app, &cloud).night.w > 0.0);

        // A star part made later (a new worldspace's sky), the sky unchanged: the kept values.
        let late = part_material(&mut app);
        app.world_mut().spawn((Stars, MeshMaterial3d(late.clone())));
        app.update();
        assert_eq!(light(&app, &late).night, first.night);

        // A fade toward a hazier weather: worked out again, every part follows.
        {
            let mut sky = app.world_mut().resource_mut::<SkyState>();
            let mut hazier = sky.weather.clone();
            hazier.form_id = 2;
            hazier.fog = [0.0, 20_000.0, 0.0, 20_000.0, 1.0, 1.0];
            sky.prev = Some(hazier);
            sky.fade = 0.5;
        }
        app.update();
        let faded = light(&app, &star).night;
        assert!(faded.x > first.night.x + 0.01, "{faded:?} {:?}", first.night);
        assert_eq!(light(&app, &late).night, faded);
        assert!(light(&app, &dome).moon.w < dome_first.moon.w, "wider under the haze");

        // Off: every part back to zero; on again: filled again.
        set_on(&mut app, false);
        app.update();
        for h in [&star, &dome, &cloud, &late] {
            let l = light(&app, h);
            assert_eq!((l.moon, l.night), (Vec4::ZERO, Vec4::ZERO));
        }
        set_on(&mut app, true);
        app.update();
        assert_eq!(light(&app, &star).night, faded);
        assert!(light(&app, &dome).night.w > 0.0);
    }

    #[test]
    fn a_moon_place_is_written_again_only_when_it_moved_by_the_tolerances_angle() {
        let m = Vec3::new(0.3, 0.8, -0.2).normalize();
        let want = (m * SUN_DISTANCE).extend(0.9);
        // A step far under a thousandth of a radian stays; a larger one or a new g is written.
        let near = ((m + Vec3::new(2e-4, 0.0, 0.0)).normalize() * SUN_DISTANCE).extend(0.9);
        assert!(same_moon(near, want));
        let far = ((m + Vec3::new(5e-3, 0.0, 0.0)).normalize() * SUN_DISTANCE).extend(0.9);
        assert!(!same_moon(far, want));
        assert!(!same_moon(want.with_w(0.85), want));
        // Zero is written exactly.
        assert!(same_moon(Vec4::ZERO, Vec4::ZERO));
        assert!(!same_moon(Vec4::new(1e-3, 0.0, 0.0, 0.0), Vec4::ZERO));
        assert!(!same_night(Vec4::new(0.0, 0.0, 0.0, 1e-4), Vec4::ZERO));
        assert!(same_night(Vec4::new(0.3, 258.0, 0.0, 0.0), Vec4::new(0.3005, 258.0, 0.0, 0.0)));
    }

    #[test]
    fn the_debug_multiplier_reads_a_number() {
        assert_eq!(debug_multiplier(None), Ok(None));
        assert_eq!(debug_multiplier(Some(" ")), Ok(None));
        assert_eq!(debug_multiplier(Some("2")), Ok(Some(2.0)));
        assert_eq!(debug_multiplier(Some("9")), Ok(Some(DEBUG_MOST)));
        assert_eq!(debug_multiplier(Some("-1")), Ok(Some(0.0)));
        assert!(debug_multiplier(Some("lots")).is_err());
        assert!(debug_multiplier(Some("inf")).is_err());
    }

    #[test]
    fn the_shader_takes_the_mirrors_constants() {
        let s = include_str!("sky.wesl");
        for line in [
            "const NIGHT_TAU: f32 = 6.2831855;",
            "h *= 0x85ebca6bu;",
            "h *= 0xc2b2ae35u;",
            "return night_mix((u.x * 73856093u) ^ (u.y * 19349663u) ^ (u.z * 83492791u));",
            "let r = night_mix(h ^ 0x9e3779b9u);",
            "let k1 = f32(3600u + r % 10801u);",
            "let k2 = f32(3600u + (r >> 16u) % 10801u);",
            "let hour = globals.time / 3600.0;",
            "let air = clamp(1.0 - (up + 0.025 * exp(-11.0 * up)), 0.0, 1.0);",
            "let d = max((1.0 - g) * (1.0 - g) + 2.0 * g * (1.0 - cos_t), 1e-6);",
            "let lobe = pow(min((1.0 - g) * (1.0 - g) / d, 1.0), 1.5);",
            "moon: vec4<f32>,",
            "night: vec4<f32>,",
            "return get_world_from_local(instance_index)[3].xyz;",
            "let to_moon = night_centre(in.instance_index) + sky.moon.xyz - view.world_position.xyz;",
            "let own = pbr_input.material.base_color.rgb * sky.params.x;",
            "add += night_moon(select(own, vec3(1.0), sky.params.w > 1.5), dir, to_moon);",
            "let dir = normalize(in.world_position.xyz - night_centre(in.instance_index));",
        ] {
            assert!(s.contains(line), "sky.wesl must hold `{line}` (night_sky.rs mirrors it)");
        }
        assert_eq!((AIR_A, AIR_B), (0.025, 11.0));
    }
}
