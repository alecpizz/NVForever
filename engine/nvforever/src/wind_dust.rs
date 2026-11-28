//! Wind-blown dust: the rules (docs/design/volumetric-medium.md, "Wind-blown dust: the
//! rules"; #3032 stage 4). [design] An addition of NVForever's own, not a fact about the
//! original game, never a parity item and never used to explain a difference from it.
//!
//! **Nothing is drawn by this module and nothing runs it yet**: it is the first slice, the
//! rules and their small state, tested here. The next slice puts [`WindDust`] in a resource
//! and gives its [`DustFrame`] to the air's grid. Until then only the tests use it, so the
//! dead code lint is off outside them (the attribute below goes with the first caller).
//!
//! The amount from the weather's wind ([`strength_from_wind`]), none while drops fall
//! ([`asked`], [`Precipitation`]), no jump back ([`Follow`]), the way it goes ([`Drift`]) and
//! its colour ([`dust_colour`]): weather values only, no form IDs, editor IDs or file names. A
//! number that is not finite counts as 0 (none); a wind direction that is not leaves the dust
//! its velocity.
//!
//! **What the next slice must do for [`Precipitation`]** (nothing here knows a particle): tag
//! every birth that comes from a weather's precipitation in particles.rs, the model's own
//! systems' and its add-on nodes', whichever model it was; count the tagged ones alive after
//! the frame's simulation; and say whether weather_fx.rs has a model placed (a holder entity that exists: a model that failed to
//! load leaves `Entity::PLACEHOLDER`, which is not placed). A caller without
//! the count passes 0 and `false`, and gets the weather's own rule only (the dust then begins
//! to come back while the last drops fall).

#![cfg_attr(not(test), allow(dead_code))]

use crate::sky::SkyState;
use crate::weather_fx::{fade_share_on, Role};
use bevy::math::DVec2;
use bevy::prelude::*;
use fnv_world::sky::{ColorType, Weather};

/// The strength from the wind speed byte: straight lines through these knots, the last held
/// above its byte [design, start values: the common 50 is 1 and 127 is 3, the ends of the
/// useful range in the prototype's stills, where 1 was barely seen, 3 was clearly dust and 8
/// heavy [observed: our own frames, 2026-11-25]. Most weathers store 50, HVDustStorm 127 and
/// NVHooverFinalBattle 130 [observed: `fnv_world::sky::Weather::wind_speed`]; the calm end
/// (none to 20) is chosen].
pub const STRENGTH_KNOTS: [(f32, f32); 4] = [(0.0, 0.0), (20.0, 0.0), (50.0, 1.0), (127.0, 3.0)];
/// The most the amount can be: the last knot.
pub const MOST: f32 = STRENGTH_KNOTS[STRENGTH_KNOTS.len() - 1].1;
/// How fast the amount may rise, a second: an ordinary day's dust is back 10 s after the last
/// drop, a strong wind's after 30 [design, start value].
pub const RISE_RATE: f32 = 0.1;
/// How fast the amount may fall, a second, when nothing cuts it to 0 at once: the whole range
/// in a second, quicker than an ordinary fade into rain asks for [design, start value].
pub const FALL_RATE: f32 = 3.0;
/// A longer frame counts as this many seconds for the amount, so a hitch is no jump [design].
/// The drift takes the whole time: the wind blew for all of it.
pub const LONGEST_STEP: f32 = 0.25;
/// How fast the dust goes in a wind of byte 255, units a second: 118 at the common 50
/// [design, start value: about 1.7 m/s at 70 units a metre].
pub const SPEED_AT_FULL: f32 = 600.0;
/// The time constant with which the dust's velocity follows the wind's, seconds [design: a
/// script's SetWindDirection is instant].
pub const TURN_SECONDS: f64 = 2.0;
/// [`Drift`] wraps after this many units: 48 cells of 2,000 and 160 cells of 600, the patches'
/// two octaves [design; the cells are the next slice's start values]. The wrap is seamless
/// only if the next slice's shader hashes both lattices periodically, every 48 and every 160
/// cells on each axis.
pub const DRIFT_PERIOD: f64 = 96_000.0;
/// WTHR DATA classification bits [documented: xEdit; `fnv_world::sky::Weather`].
const RAINY: u8 = 4;
const SNOW: u8 = 8;

/// `x` held to `lo..hi`; `lo` when it is not a finite number.
fn held(x: f32, lo: f32, hi: f32) -> f32 {
    if x.is_finite() { x.clamp(lo, hi) } else { lo }
}

/// A frame's length as the rules take it: 0 (nothing moves) when it is not a finite number.
fn step(dt: f32) -> f32 {
    held(dt, 0.0, LONGEST_STEP)
}

/// How far the weather has faded in, 0..1.
fn progress(sky: &SkyState) -> f32 {
    held(sky.fade, 0.0, 1.0)
}

/// The strength (0..[`MOST`]) a wind speed byte asks for; it need not be whole (two weathers'
/// blended through a fade).
pub fn strength_from_wind(byte: f32) -> f32 {
    let byte = held(byte, 0.0, 255.0);
    for pair in STRENGTH_KNOTS.windows(2) {
        let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
        if byte <= x1 {
            return y0 + (y1 - y0) * (byte - x0) / (x1 - x0);
        }
    }
    MOST
}

/// The wind speed byte now, the two weathers' blended through the fade: as
/// `tree::wind_vector` has it, but not rounded to its thousandths and the same in an
/// interior. The weather outside goes on while the player is inside and the dust with it, so
/// it stands where it should when they come out [design].
pub fn wind_byte(sky: &SkyState) -> f32 {
    let now = f32::from(sky.weather.wind_speed);
    match &sky.prev {
        Some(p) => {
            let old = f32::from(p.wind_speed);
            old + (now - old) * progress(sky)
        }
        None => now,
    }
}

/// A weather's share of the dust (0 or 1): none with the Rainy flag, classified Rainy or Snow,
/// or with a precipitation model. The flag and the classes cut it without a model too: Honest
/// Hearts' rain has none and is drawn by a script [documented: docs/spec-notes/world.md §6].
pub fn dry_share(w: &Weather) -> f32 {
    if w.rainy || w.classification & (RAINY | SNOW) != 0 || w.precipitation_model.is_some() {
        0.0
    } else {
        1.0
    }
}

/// Whether a weather's own precipitation falls now, by weather_fx.rs's rule: the one shown or
/// coming in from its begin-fade-in byte, the one going out until its end-fade-out byte
/// [hypothesis there: from the fields' names]. Drops still in the air are [`Precipitation`]'s.
pub fn precipitation_falls(sky: &SkyState) -> bool {
    let t = progress(sky);
    let falls = |w: &Weather, role| w.precipitation_model.is_some() && fade_share_on(w.precipitation_fade, role);
    match &sky.prev {
        Some(p) => falls(&sky.weather, Role::Incoming(t)) || falls(p, Role::Outgoing(t)),
        None => falls(&sky.weather, Role::Steady),
    }
}

/// The weathers' dry share now (0..1); 0 while either's precipitation falls. A product of
/// linear ramps of the fade's progress: [`dry_share`] blended from one weather to the other;
/// into a precipitation model, 1 at the fade's start down to 0 where it begins to fall; out of
/// one, 0 where it stops up to 1 at the fade's end [design]. So with a model on one side it is
/// a curve, not a straight line. A byte that leaves no room (begin 0, end 255) lets it go or
/// come at once, as the rain does; [`Follow`] then still lets it come back slowly.
pub fn dry_through_fade(sky: &SkyState) -> f32 {
    let now = dry_share(&sky.weather);
    let Some(p) = &sky.prev else { return now };
    if precipitation_falls(sky) {
        return 0.0;
    }
    let t = progress(sky);
    let mut dry = dry_share(p) + (now - dry_share(p)) * t;
    if sky.weather.precipitation_model.is_some() {
        let begin = f32::from(sky.weather.precipitation_fade.0) / 255.0;
        dry *= if begin > 0.0 { (1.0 - t / begin).clamp(0.0, 1.0) } else { 0.0 };
    }
    if p.precipitation_model.is_some() {
        let end = f32::from(p.precipitation_fade.1) / 255.0;
        dry *= if end < 1.0 { ((t - end) / (1.0 - end)).clamp(0.0, 1.0) } else { 0.0 };
    }
    held(dry, 0.0, 1.0)
}

/// What the weather asks for this frame, before anything follows it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Asked {
    /// [`strength_from_wind`] of [`wind_byte`], 0..[`MOST`].
    pub strength: f32,
    /// [`dry_through_fade`], 0..1.
    pub dry: f32,
    /// No dust at all, at once: precipitation falls, or a wet weather is shown with no fade
    /// left (a script's ForceWeather is instant).
    pub cut: bool,
}

/// [`Asked`] by the sky as it is.
pub fn asked(sky: &SkyState) -> Asked {
    let dry = dry_through_fade(sky);
    let cut = precipitation_falls(sky) || (sky.prev.is_none() && dry <= 0.0);
    Asked { strength: strength_from_wind(wind_byte(sky)), dry, cut }
}

/// The weather's precipitation in the world this frame, from whoever draws it (the module
/// notes: what the next slice must do).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Precipitation {
    /// A precipitation model is placed now, also one that has given no drop yet.
    pub placed: bool,
    /// Drops alive after this frame's simulation that any precipitation gave, whichever model
    /// and whether it still stands.
    pub alive: usize,
}

impl Precipitation {
    /// No dust while it lasts.
    pub fn here(self) -> bool {
        self.placed || self.alive > 0
    }
}

/// The amount as it moves from frame to frame. There is no gliding state that a cut could forget
/// to set: every rise, whatever its reason, is held to [`RISE_RATE`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Follow {
    /// The strength and the amount shown last; `None` before the first frame.
    shown: Option<(f32, f32)>,
}

impl Follow {
    /// The amount a frame later (0..[`MOST`]). The strength glides at [`RISE_RATE`] both ways
    /// (a forced weather changes the wind at once). The dry share acts as the fade has it, so
    /// the amount reaches 0 where the rain starts. Their product rises by at most
    /// [`RISE_RATE`] and falls by at most [`FALL_RATE`] a second; 0 at once while cut
    /// (`asked.cut` or `rain`). The first frame shows its target.
    pub fn follow(&mut self, asked: Asked, rain: Precipitation, dt: f32) -> f32 {
        let (wanted, dry) = (held(asked.strength, 0.0, MOST), held(asked.dry, 0.0, 1.0));
        let dt = step(dt);
        let (strength, last) = match self.shown {
            Some((s, a)) => (s + (wanted - s).clamp(-RISE_RATE * dt, RISE_RATE * dt), a),
            None => (wanted, wanted * dry),
        };
        let amount = if asked.cut || rain.here() {
            0.0
        } else {
            held(last + (strength * dry - last).clamp(-FALL_RATE * dt, RISE_RATE * dt), 0.0, MOST)
        };
        self.shown = Some((strength, amount));
        amount
    }
}

/// The wind's velocity now (Bevy world x and z, units a second): the trees' direction
/// (`tree::wind_vector`: fixed, or a script's SetWindDirection, the angle the wind blows to
/// [documented: GECK wiki SetWindDirection]) at the speed of [`wind_byte`]; `None` for a
/// direction that is not a finite number.
pub fn wind_velocity(sky: &SkyState) -> Option<Vec2> {
    let d = crate::tree::wind_vector(sky);
    let d = Vec2::new(d.x, d.z).try_normalize()?;
    let v = d * (wind_byte(sky) / 255.0 * SPEED_AT_FULL);
    v.is_finite().then_some(v)
}

/// How far the wind has carried the dust: its velocity summed exactly over the game's clock for a wind held through each frame,
/// so the frames a held wind is cut into change nothing (a wind that changes within a frame is taken at
/// its end); in f64, kept within one [`DRIFT_PERIOD`] on each
/// axis.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Drift {
    /// Bevy world x and z, 0..[`DRIFT_PERIOD`].
    at: DVec2,
    /// The dust's velocity, which follows the wind's; `None` before the first wind.
    velocity: Option<DVec2>,
}

impl Drift {
    /// `dt` seconds of `wind` (all of them: a hitch does not hold the dust still). The velocity
    /// goes toward the wind with the time constant T = [`TURN_SECONDS`], v(t) = w + (v0 - w)
    /// e^(-t/T), and the place by its integral, w t + (v0 - w) T (1 - e^(-t/T)). The first
    /// wind is taken whole; without one (`None`, or not finite) the velocity stays as it is. A frame of no
    /// time (a pause, a broken clock) changes nothing, a first wind included.
    pub fn advance(&mut self, wind: Option<Vec2>, dt: f32) {
        let t = if dt.is_finite() { f64::from(dt.max(0.0)) } else { 0.0 };
        if t <= 0.0 {
            return;
        }
        let wind = wind.filter(|w| w.is_finite()).map(|w| w.as_dvec2());
        let Some(from) = self.velocity.or(wind) else { return };
        let to = wind.unwrap_or(from);
        let share = -(-t / TURN_SECONDS).exp_m1();
        self.velocity = Some(from + (to - from) * share);
        self.at = wrapped(self.at + to * t + (from - to) * (TURN_SECONDS * share));
    }

    /// The patches' phase at `camera` (Bevy world x and z): (camera - drift) within
    /// 0..[`DRIFT_PERIOD`], to which a shader adds a place relative to the camera. In f64, so
    /// a camera far from the origin keeps its fraction.
    pub fn phase(&self, camera: DVec2) -> Vec2 {
        let p = wrapped(camera - self.at).as_vec2();
        // The f32 nearest to a number just under the period is the period.
        Vec2::from(p.to_array().map(|p| if p < DRIFT_PERIOD as f32 { p } else { 0.0 }))
    }

    /// The dust's velocity now (Bevy world x and z, units a second).
    pub fn velocity(&self) -> Vec2 {
        self.velocity.unwrap_or(DVec2::ZERO).as_vec2()
    }
}

/// Each axis within 0..[`DRIFT_PERIOD`]; 0 where it is not a finite number.
fn wrapped(x: DVec2) -> DVec2 {
    DVec2::from(x.to_array().map(|x| {
        let p = x.rem_euclid(DRIFT_PERIOD);
        // A tiny negative number's remainder rounds up to the period itself.
        if p.is_finite() && p < DRIFT_PERIOD { p } else { 0.0 }
    }))
}

/// The dust's colour (linear): the fog's, tinted by the hue of the light on it, its ambient
/// plus its sunlight (divided by the sum's Rec. 709 luminance, each channel held to 0..2; no
/// tint without light). Never a colour of ours: a black fog's dust is black [design].
pub fn dust_colour(fog: [f32; 3], ambient: [f32; 3], sunlight: [f32; 3]) -> [f32; 3] {
    let light: [f32; 3] = std::array::from_fn(|i| held(ambient[i], 0.0, 1e3) + held(sunlight[i], 0.0, 1e3));
    let luminance = 0.2126 * light[0] + 0.7152 * light[1] + 0.0722 * light[2];
    std::array::from_fn(|i| {
        let hue = if luminance > 1e-4 { held(light[i] / luminance, 0.0, 2.0) } else { 1.0 };
        held(fog[i], 0.0, 1.0) * hue
    })
}

/// [`dust_colour`] of the weather now (its colours at the hour, blended through a fade, with a
/// script's changes).
pub fn sky_dust_colour(sky: &SkyState) -> [f32; 3] {
    dust_colour(sky.color(ColorType::Fog), sky.color(ColorType::Ambient), sky.color(ColorType::Sunlight))
}

/// What the frame's dust is.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DustFrame {
    /// 0..[`MOST`]; 0 in an interior.
    pub amount: f32,
    /// Linear, each channel 0..2.
    pub colour: [f32; 3],
    /// Cut to 0 by rain, snow or drops in the air (a log line, a debug view).
    pub wet: bool,
    /// In an interior.
    pub indoors: bool,
}

/// The rules' state between frames.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WindDust {
    follow: Follow,
    pub drift: Drift,
}

impl WindDust {
    /// A frame, after the sky's and the particles' simulation, by the game's clock (`dt` 0
    /// while it is paused: nothing moves). In an interior (`indoors`: the streamer's cell,
    /// which may have no lighting of its own; or the sky's interior lighting) the amount is
    /// masked to 0 and the rest goes on as the weather outside does: call it there too, so
    /// coming out shows what the glide has reached.
    pub fn advance(&mut self, sky: &SkyState, indoors: bool, rain: Precipitation, dt: f32) -> DustFrame {
        let asked = asked(sky);
        let amount = self.follow.follow(asked, rain, dt);
        self.drift.advance(wind_velocity(sky), dt);
        let indoors = indoors || sky.interior.is_some();
        DustFrame { amount: if indoors { 0.0 } else { amount }, colour: sky_dust_colour(sky), wet: asked.cut || rain.here(), indoors }
    }

    /// A game loaded or a new one, and nothing else: the next frame shows its target (0 if it
    /// is cut). Never for a door, a cell change or a trip within a game: the amount carries on
    /// over those (masked indoors), or the dust would jump back after rain. How far the dust
    /// has drifted stays.
    pub fn arrive(&mut self) {
        self.follow = Follow::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;
    const DRY: Precipitation = Precipitation { placed: false, alive: 0 };
    const PLACED: Precipitation = Precipitation { placed: true, alive: 0 };

    fn falling(alive: usize) -> Precipitation {
        Precipitation { placed: false, alive }
    }

    fn weather(wind: u8) -> Weather {
        Weather { classification: 1, wind_speed: wind, ..default() }
    }

    /// A weather with a precipitation model and its fade bytes.
    fn wet(fades: (u8, u8)) -> Weather {
        Weather { precipitation_model: Some("a\\rain.nif".into()), precipitation_fade: fades, ..weather(127) }
    }

    fn sky(w: Weather) -> SkyState {
        let mut sky = SkyState::default();
        sky.weather = w;
        sky.fade = 1.0;
        sky.hour = 12.0;
        sky
    }

    fn fading(from: Weather, to: Weather, fade: f32) -> SkyState {
        let mut s = sky(to);
        s.prev = Some(from);
        s.fade = fade;
        s
    }

    /// What the weather asks for with nothing to follow: the steady amount.
    fn target(s: &SkyState) -> f32 {
        let a = asked(s);
        if a.cut { 0.0 } else { a.strength * a.dry }
    }

    #[test]
    fn a_calm_day_has_none_an_ordinary_one_and_a_windy_one_three() {
        assert_eq!(strength_from_wind(0.0), 0.0);
        assert_eq!(strength_from_wind(20.0), 0.0);
        assert!((strength_from_wind(50.0) - 1.0).abs() < 1e-6);
        assert!((strength_from_wind(127.0) - 3.0).abs() < 1e-6);
        assert_eq!(strength_from_wind(255.0), MOST);
        assert_eq!(strength_from_wind(1e9), MOST, "held above the byte's range");
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -5.0] {
            assert_eq!(strength_from_wind(bad), 0.0, "{bad}");
        }
        let mut last = 0.0;
        for k in 0..=2550 {
            let a = strength_from_wind(k as f32 / 10.0);
            assert!(a >= last && a - last < 0.004 && a <= MOST, "rising without a step at {}", k as f32 / 10.0);
            last = a;
        }
    }

    #[test]
    fn the_strength_goes_evenly_from_one_weather_to_the_next() {
        assert_eq!(wind_byte(&sky(weather(50))), 50.0);
        assert_eq!(wind_byte(&fading(weather(50), weather(127), 0.0)), 50.0);
        assert_eq!(wind_byte(&fading(weather(50), weather(127), 1.0)), 127.0);
        assert_eq!(wind_byte(&fading(weather(0), weather(100), 0.25)), 25.0);
        let mut last = target(&sky(weather(0)));
        for i in 0..=1000 {
            let a = target(&fading(weather(0), weather(255), i as f32 / 1000.0));
            assert!(a >= last && a - last < 0.01, "{last} -> {a}");
            last = a;
        }
        assert_eq!(last, target(&sky(weather(255))), "the fade's end is the weather after it");
        // A fade that is not a number, or past its ends.
        for (bad, want) in [(f32::NAN, 50.0), (f32::INFINITY, 50.0), (-3.0, 50.0), (7.0, 127.0)] {
            assert_eq!(wind_byte(&fading(weather(50), weather(127), bad)), want, "{bad}");
        }
    }

    #[test]
    fn rain_snow_and_precipitation_models_give_none_by_their_values_alone() {
        let wets = [
            Weather { rainy: true, ..weather(127) },
            Weather { classification: RAINY, ..weather(127) },
            Weather { classification: SNOW | 1, ..weather(127) },
            wet((0, 255)),
        ];
        for w in &wets {
            assert_eq!(dry_share(w), 0.0);
            assert!(asked(&sky(w.clone())).cut, "shown with no fade left: gone at once");
        }
        for class in [0, 1, 2, 3, 16, 128] {
            assert_eq!(dry_share(&Weather { classification: class, ..weather(127) }), 1.0, "class {class}");
        }
        // No name and no ID changes anything.
        let named = Weather { form_id: 0xE3792, edid: Some("HVDustStorm".into()), plugin: Some("Rain.esp".into()), ..weather(50) };
        assert_eq!(asked(&sky(named)), asked(&sky(weather(50))));
        // Into a weather that is wet by its flag alone: evenly to 0, and cut at the end.
        let mut last = f32::INFINITY;
        for i in 0..=100 {
            let s = fading(weather(127), wets[0].clone(), i as f32 / 100.0);
            let a = target(&s);
            assert!(!asked(&s).cut && a <= last && (i == 0 || last - a < 0.04), "{last} -> {a}");
            last = a;
        }
        assert_eq!(last, 0.0);
    }

    #[test]
    fn dust_is_gone_before_rain_falls_and_only_back_after_it_stopped() {
        let steady = target(&sky(weather(127)));
        for byte in [0u8, 1, 52, 255] {
            let mut last = f32::INFINITY;
            for i in 0..=255 {
                let s = fading(weather(127), wet((byte, 128)), i as f32 / 255.0);
                if fade_share_on((byte, 128), Role::Incoming(s.fade)) {
                    assert!(asked(&s).cut && asked(&s).dry == 0.0, "byte {byte}: rain falls at {}", s.fade);
                }
                let a = target(&s);
                assert!(a <= last, "byte {byte}: falls only");
                last = a;
            }
            if byte > 0 {
                assert!((target(&fading(weather(127), wet((byte, 128)), 0.0)) - steady).abs() < 1e-5, "no step at the start");
            }
            let mut last = 0.0;
            for i in 0..=255 {
                let s = fading(wet((128, byte)), weather(127), i as f32 / 255.0);
                if fade_share_on((128, byte), Role::Outgoing(s.fade)) {
                    assert!(asked(&s).cut && asked(&s).dry == 0.0, "byte {byte}: rain still falls at {}", s.fade);
                }
                let a = target(&s);
                assert!(a >= last, "byte {byte}: rises only");
                last = a;
            }
            if byte < 255 {
                assert!((last - steady).abs() < 1e-5, "whole again at the end");
            }
        }
        for i in 0..=20 {
            assert_eq!(target(&fading(wet((52, 52)), wet((200, 52)), i as f32 / 20.0)), 0.0, "rain to rain");
        }
        // A fade that is not a number counts as its start.
        let s = fading(wet((52, 52)), weather(127), f32::NAN);
        assert!(asked(&s).cut && asked(&s).dry == 0.0);
    }

    #[test]
    fn a_fade_into_rain_is_followed_to_nothing_without_a_step() {
        // 30 s to the weather, rain from a fifth of it on: the amount goes down its line.
        let mut f = Follow::default();
        let mut last = f.follow(asked(&sky(weather(127))), DRY, DT);
        assert!((last - 3.0).abs() < 1e-5);
        for frame in 0..=1800 {
            let s = fading(weather(127), wet((51, 128)), frame as f32 / 1800.0);
            let a = f.follow(asked(&s), DRY, DT);
            assert!(a <= last + 1e-6 && last - a < 0.02, "frame {frame}: {last} -> {a}");
            assert!(!precipitation_falls(&s) || a == 0.0);
            last = a;
        }
        assert_eq!(last, 0.0);
    }

    #[test]
    fn after_rain_the_dust_comes_back_at_its_rate_never_at_once() {
        // Every way rain can end: its fade's last frame (end byte 255, no room to fade in), a
        // forced dry weather, the last drop of it.
        let mut f = Follow::default();
        assert_eq!(f.follow(asked(&sky(wet((0, 255)))), DRY, DT), 0.0);
        assert_eq!(f.follow(asked(&fading(wet((0, 255)), weather(127), 0.999)), DRY, DT), 0.0);
        let dry = asked(&sky(weather(127)));
        assert_eq!(f.follow(dry, falling(3), DT), 0.0, "its last drops still fall");
        let mut last = 0.0;
        for frame in 1..=600 {
            let a = f.follow(dry, DRY, DT);
            assert!(a > last && a - last <= RISE_RATE * DT + 1e-5, "frame {frame}: {last} -> {a}");
            last = a;
        }
        assert!((last - 1.0).abs() < 0.01, "ten seconds at a tenth a second: {last}");
        for _ in 0..1300 {
            last = f.follow(dry, DRY, DT);
        }
        assert!((last - 3.0).abs() < 1e-3, "and all of it in thirty: {last}");
    }

    #[test]
    fn a_forced_weather_never_makes_the_amount_jump() {
        let run = |from: Weather, to: Weather| {
            let mut f = Follow::default();
            let mut last = f.follow(asked(&sky(from)), DRY, DT);
            let s = sky(to);
            for _ in 0..(40 * 60) {
                let a = f.follow(asked(&s), DRY, DT);
                assert!((a - last).abs() <= RISE_RATE * DT + 1e-5, "{last} -> {a}");
                last = a;
            }
            assert!((last - target(&s)).abs() < 1e-3, "{last}, it gets there");
        };
        run(weather(0), weather(255));
        run(weather(255), weather(0));
        run(weather(50), weather(127));
    }

    #[test]
    fn a_turned_round_fade_never_lets_the_amount_jump_up() {
        // A tenth into a wet weather, not raining yet; turned back, nine tenths of the way.
        let before = fading(weather(127), wet((52, 52)), 0.1);
        let after = fading(wet((52, 52)), weather(127), 0.9);
        assert!(!precipitation_falls(&before) && !precipitation_falls(&after));
        assert!(target(&after) - target(&before) > 0.5, "the target jumps");
        let mut f = Follow::default();
        let a = f.follow(asked(&before), DRY, DT);
        let b = f.follow(asked(&after), DRY, DT);
        assert!(b > a && b - a <= RISE_RATE * DT + 1e-6, "{a} -> {b}");
        // The other way round it goes down within a second, not in a frame.
        let mut f = Follow::default();
        let a = f.follow(asked(&after), DRY, DT);
        let b = f.follow(asked(&before), DRY, DT);
        assert!(b < a && a - b <= FALL_RATE * DT + 1e-6, "{a} -> {b}");
    }

    #[test]
    fn a_pause_holds_a_hitch_is_one_step_and_wild_numbers_stay_in_range() {
        let dry = Asked { strength: 3.0, dry: 1.0, cut: false };
        let mut f = Follow::default();
        f.follow(Asked { cut: true, ..dry }, DRY, DT);
        assert_eq!(f.follow(dry, DRY, 0.0), 0.0, "paused");
        assert!(f.follow(dry, DRY, 1e6) <= RISE_RATE * LONGEST_STEP + 1e-6, "a hitch");
        let wild = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 0.0, 0.5, 1e30];
        for &strength in &wild {
            for &dry in &wild {
                for &dt in &wild {
                    for cut in [false, true] {
                        let a = f.follow(Asked { strength, dry, cut }, DRY, dt);
                        assert!((0.0..=MOST).contains(&a) && (!cut || a == 0.0), "{a} for {strength} {dry} {dt}");
                        let mut first = Follow::default();
                        let a = first.follow(Asked { strength, dry, cut }, DRY, dt);
                        assert!((0.0..=MOST).contains(&a) && (!cut || a == 0.0));
                    }
                }
            }
        }
    }

    #[test]
    fn drops_in_the_air_keep_the_dust_off_after_the_weather_is_dry() {
        // The sky is dry already; what fell before it is down to its last drops.
        let mut w = WindDust::default();
        let s = sky(weather(127));
        for alive in [40, 40, 12, 1] {
            let f = w.advance(&s, false, falling(alive), DT);
            assert!(f.amount == 0.0 && f.wet, "{alive} in the air: {f:?}");
        }
        // The last one gone: slowly back. A model placed, with no drop yet: none.
        let f = w.advance(&s, false, DRY, DT);
        assert!(f.amount > 0.0 && f.amount <= RISE_RATE * DT + 1e-6 && !f.wet, "{f:?}");
        let f = w.advance(&s, false, PLACED, DT);
        assert!(f.amount == 0.0 && f.wet);
        assert!(w.advance(&s, false, DRY, DT).amount <= RISE_RATE * DT + 1e-6);
        // From full strength, one drop: gone in that frame.
        let mut w = WindDust::default();
        assert!(w.advance(&s, false, DRY, DT).amount > 2.9);
        assert_eq!(w.advance(&s, false, falling(1), DT).amount, 0.0);
    }

    #[test]
    fn dust_first_enabled_in_the_last_drops_of_rain_starts_from_none() {
        let mut w = WindDust::default();
        let s = sky(weather(127));
        assert_eq!(w.advance(&s, false, falling(5), DT).amount, 0.0);
        assert!(w.advance(&s, false, DRY, DT).amount <= RISE_RATE * DT + 1e-6, "no first frame at its target");
    }

    #[test]
    fn a_weather_a_script_forces_wet_cuts_the_dust_and_dry_lets_it_back_slowly() {
        // Through the sky's own forced change, by the weather's rule alone (no count).
        let mut s = sky(weather(127));
        let mut w = WindDust::default();
        assert!(w.advance(&s, false, DRY, DT).amount > 2.9);
        s.script_weather(Weather { rainy: true, form_id: 0x77, ..weather(127) }, [None; 6], true, true);
        assert!(s.prev.is_none() && s.weather.rainy);
        let f = w.advance(&s, false, DRY, DT);
        assert!(f.amount == 0.0 && f.wet, "wet at once, gone at once");
        s.script_weather(Weather { form_id: 0x78, ..weather(127) }, [None; 6], true, true);
        assert!(s.prev.is_none() && !s.weather.rainy);
        let a = w.advance(&s, false, DRY, DT).amount;
        assert!(a > 0.0 && a <= RISE_RATE * DT + 1e-6, "dry at once, back slowly: {a}");
    }

    #[test]
    fn a_turn_changes_the_way_and_never_the_place() {
        let mut d = Drift::default();
        let (east, south) = (Vec2::new(118.0, 0.0), Vec2::new(0.0, 118.0));
        let (mut at, mut v) = (d.phase(DVec2::ZERO), None::<Vec2>);
        for frame in 0..1800 {
            // A right angle at once, then straight back, then a stop (a forced calm weather).
            let wind = match frame {
                0..600 => east,
                600..1000 => south,
                1000..1400 => -south,
                _ => Vec2::ZERO,
            };
            d.advance(Some(wind), DT);
            let now = d.phase(DVec2::ZERO);
            let moved = (now - at).abs();
            let moved = moved.min(Vec2::splat(DRIFT_PERIOD as f32) - moved);
            assert!(moved.length() <= 118.0 * DT + 0.02, "frame {frame}: {moved}");
            if let Some(v) = v {
                assert!((d.velocity() - v).length() <= 2.0 * 118.0 * DT / TURN_SECONDS as f32 + 1e-3, "frame {frame}: no lurch");
            }
            (at, v) = (now, Some(d.velocity()));
        }
        assert!(d.velocity().length() < 6.0, "it comes to rest: {}", d.velocity());
    }

    #[test]
    fn the_frames_a_stretch_of_time_is_cut_into_do_not_move_the_place() {
        // A second of one wind, then four of another, in 1 frame or in 1,000.
        let (first, then) = (Vec2::new(0.0, -118.0), Vec2::new(299.0, 20.0));
        let run = |frames: u32| {
            let mut d = Drift::default();
            d.advance(Some(first), 1.0);
            for _ in 0..frames {
                d.advance(Some(then), 4.0 / frames as f32);
            }
            d
        };
        let one = run(1);
        for frames in [7, 60, 1000] {
            let d = run(frames);
            assert!((d.at - one.at).abs().max_element() < 1e-3, "{frames} frames: {} and {}", d.at, one.at);
            assert!((d.velocity() - one.velocity()).length() < 1e-3);
        }
        // And it is where the velocity summed in small steps of its own ends.
        let (mut at, mut v) = (first.as_dvec2(), first.as_dvec2());
        let steps = 400_000;
        let h = 4.0 / f64::from(steps);
        for _ in 0..steps {
            let mid = v + (then.as_dvec2() - v) * (h * 0.5 / TURN_SECONDS);
            at += mid * h;
            v += (then.as_dvec2() - mid) * (h / TURN_SECONDS);
        }
        assert!((one.at - wrapped(at)).abs().max_element() < 1e-3, "{} and {at}", one.at);
    }

    #[test]
    fn the_first_wind_is_whole_and_what_is_not_one_changes_nothing() {
        let mut d = Drift::default();
        d.advance(None, DT);
        assert_eq!((d.velocity(), d.phase(DVec2::new(5.0, 6.0))), (Vec2::ZERO, Vec2::new(5.0, 6.0)));
        // Ten seconds on, the patch that was at the origin is at (1000, -500).
        d.advance(Some(Vec2::new(100.0, -50.0)), 10.0);
        let p = d.phase(DVec2::new(1000.0, -500.0));
        assert!(p.min(Vec2::splat(DRIFT_PERIOD as f32) - p).max_element() < 0.01, "{p}");
        let before = d;
        d.advance(Some(Vec2::new(0.0, 500.0)), 0.0);
        d.advance(Some(Vec2::new(0.0, 500.0)), f32::NAN);
        d.advance(Some(Vec2::new(0.0, 500.0)), -1.0);
        assert_eq!(d, before, "a paused or broken clock");
        // Nor does a fresh drift take its first wind in a frame of no time.
        let mut fresh = Drift::default();
        for dt in [0.0, -1.0, f32::NAN] {
            fresh.advance(Some(Vec2::new(0.0, 500.0)), dt);
        }
        assert_eq!(fresh, Drift::default());
        // No direction: the velocity it had, and it goes on.
        d.advance(Some(Vec2::new(f32::NAN, 1.0)), 1.0);
        d.advance(None, 1.0);
        assert_eq!(d.velocity(), Vec2::new(100.0, -50.0));
        assert!((d.at.x - 1200.0).abs() < 1e-6, "{}", d.at);
        d.advance(Some(Vec2::new(600.0, 600.0)), f32::MAX);
        assert!(d.at.cmpge(DVec2::ZERO).all() && d.at.cmplt(DVec2::splat(DRIFT_PERIOD)).all() && d.velocity().is_finite());
    }

    #[test]
    fn the_phase_keeps_its_fraction_far_out_and_its_period_fits_both_octaves() {
        for cell in [2_000.0, 600.0] {
            assert_eq!(DRIFT_PERIOD % cell, 0.0, "a whole number of cells of {cell}");
        }
        let mut d = Drift::default();
        for _ in 0..7 {
            d.advance(Some(Vec2::new(-133.3, 77.7)), 0.2);
        }
        for cam in [0.0, 1234.5, -77_777.25, 1.0e6, -1.0e6, 1.0e6 + 0.37, -1e-12, f64::NAN, f64::INFINITY, -1e300] {
            let p = d.phase(DVec2::new(cam, -cam));
            assert!(p.cmpge(Vec2::ZERO).all() && p.cmplt(Vec2::splat(DRIFT_PERIOD as f32)).all(), "{p} at {cam}");
        }
        let moved = d.phase(DVec2::new(1.0e6 + 1.0, 0.0)).x - d.phase(DVec2::new(1.0e6, 0.0)).x;
        assert!((moved - 1.0).abs() < 0.01, "one unit a million units out: {moved}");
        // Where it wraps, nothing moves: a period on is the same place.
        let (a, b) = (d.phase(DVec2::new(1234.5, 9.0)), d.phase(DVec2::new(1234.5 + DRIFT_PERIOD, 9.0 - DRIFT_PERIOD)));
        assert!((a - b).abs().max_element() < 1e-2, "{a} {b}");
    }

    #[test]
    fn the_velocity_is_the_trees_direction_at_the_weathers_speed() {
        let s = sky(weather(50));
        let v = wind_velocity(&s).unwrap();
        let tree = crate::tree::wind_vector(&s);
        assert!((v.length() - 50.0 / 255.0 * SPEED_AT_FULL).abs() < 1e-2, "{v}");
        assert!(v.normalize().dot(Vec2::new(tree.x, tree.z)) > 0.9999);
        assert_eq!(wind_velocity(&sky(weather(0))), Some(Vec2::ZERO), "still air");
        // A script's direction, and one that is no direction at all.
        let mut turned = sky(weather(50));
        turned.script_wind = Some(0.0);
        assert!((wind_velocity(&turned).unwrap().normalize() - Vec2::X).length() < 1e-4);
        for bad in [f32::NAN, f32::INFINITY] {
            turned.script_wind = Some(bad);
            assert_eq!(wind_velocity(&turned), None, "{bad}");
        }
    }

    #[test]
    fn the_colour_is_the_fogs_tinted_by_the_light_and_stays_finite() {
        let lin = |b: [u8; 3]| b.map(fnv_world::sky::srgb_to_linear);
        // A bluish fog under a warm sun and a bluish sky.
        let fog = lin([150, 170, 190]);
        let c = dust_colour(fog, lin([85, 105, 140]), lin([255, 225, 170]));
        assert!(c[0] > fog[0] && c[2] < fog[2], "warmer: {c:?}");
        // White light, or none, is the fog's own colour; its brightness does not matter.
        for c in [dust_colour(fog, [0.0; 3], [0.0; 3]), dust_colour(fog, [0.3; 3], [0.2; 3]), dust_colour(fog, [30.0; 3], [2.0; 3])] {
            assert!((Vec3::from(c) - Vec3::from(fog)).abs().max_element() < 1e-5, "{c:?}");
        }
        assert_eq!(dust_colour([0.0; 3], [0.4, 0.3, 0.2], [1.0; 3]), [0.0; 3], "a black fog's dust is black");
        let wild = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 1e30, 0.0, 1e-9, 0.5];
        for &a in &wild {
            for &b in &wild {
                for &f in &wild {
                    let c = dust_colour([f, 0.3, f], [a, b, 0.1], [b, a, 0.2]);
                    assert!(c.iter().all(|v| (0.0..=2.0).contains(v)), "{c:?} for {a} {b} {f}");
                }
            }
        }
        // A weather with no colours at all, and a fade that is not a number.
        for s in [sky(weather(50)), fading(weather(50), weather(127), f32::NAN)] {
            assert!(sky_dust_colour(&s).iter().all(|v| (0.0..=2.0).contains(v)));
        }
    }

    #[test]
    fn an_indoor_spell_masks_the_amount_and_coming_out_is_no_jump() {
        let s = sky(weather(127));
        let rain = sky(wet((0, 255)));
        // Dry all along: the dust outside is as it was, and the wind went on.
        let mut w = WindDust::default();
        let before = w.advance(&s, false, DRY, DT).amount;
        let drift = w.drift;
        for _ in 0..120 {
            let f = w.advance(&s, true, DRY, DT);
            assert!(f.amount == 0.0 && f.indoors);
        }
        assert!((w.drift.velocity() - drift.velocity()).length() < 1e-3 && w.drift != drift);
        assert!((w.advance(&s, false, DRY, DT).amount - before).abs() < 1e-5);
        // In just after rain: out again five seconds on, five seconds of the way back.
        w.advance(&rain, false, PLACED, DT);
        for _ in 0..300 {
            assert_eq!(w.advance(&s, true, DRY, DT).amount, 0.0);
        }
        let a = w.advance(&s, false, DRY, DT).amount;
        assert!((a - 0.5).abs() < 0.02, "{a}");
        // It rained for a frame while inside: from none again at the door.
        w.advance(&rain, true, DRY, DT);
        w.advance(&s, true, DRY, DT);
        let a = w.advance(&s, false, DRY, DT).amount;
        assert!(a <= 2.0 * RISE_RATE * DT + 1e-6, "no full strength at the door: {a}");
        // The sky's interior lighting masks it too.
        let mut inside = sky(weather(127));
        inside.interior = Some(fnv_world::lighting::CellLighting::default());
        let f = w.advance(&inside, false, DRY, DT);
        assert!(f.amount == 0.0 && f.indoors);
    }

    #[test]
    fn a_load_shows_its_target_and_is_the_only_thing_that_does() {
        let s = sky(weather(50));
        let mut w = WindDust::default();
        w.advance(&s, false, falling(1), DT);
        // Without it, as at a door or on a trip, the amount carries on.
        let mut carried = w;
        assert!(carried.advance(&s, false, DRY, DT).amount <= RISE_RATE * DT + 1e-6);
        let drift = w.drift;
        w.arrive();
        assert_eq!(w.drift, drift);
        assert!((w.advance(&s, false, DRY, DT).amount - 1.0).abs() < 1e-6);
        // Loaded into rain or its last drops: none, and from none.
        for (into, rain) in [(sky(wet((0, 255))), DRY), (sky(weather(50)), falling(2))] {
            w.arrive();
            assert_eq!(w.advance(&into, false, rain, DT).amount, 0.0);
            assert!(w.advance(&s, false, DRY, DT).amount <= RISE_RATE * DT + 1e-6);
        }
    }
}
