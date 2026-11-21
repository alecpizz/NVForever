//! Layered gunshots (Modern Feel, `ModernFeel::gun_audio`, docs/design/modern-feel-audio.md)
//! [design: NVForever's own, none of it in the original; every constant an engineering
//! choice tuned by ear]. Every weapon shot, the player's and other actors', is played with
//! a tail made from its own sound by the place it is heard in, and is shaped by distance:
//!
//! - **The tail** ([`ShotDsp`]): outside, a rolling echo, the slap off distant terrain:
//!   a few delayed copies spread over [`ECHO_FIRST`]..[`ECHO_LAST`], each quieter and duller
//!   than the one before, alternating sides, smeared by two short all-passes per ear so
//!   they roll rather than click. Inside (an interior cell) a short dense slap: early
//!   reflections 18–120 ms, scaled by the room's reverb decay; outside among buildings
//!   (the City, Alley and Parkinglot environments) both, the echo lower. The I3DL2 reverb
//!   of the acoustic space (`crate::reverb`) still follows it.
//! - **Distance** (another actor's shot): air absorption, a low-pass falling from
//!   [`AIR_FULL_HZ`] with distance ([`air_cutoff`]); the tail is louder against the direct
//!   sound the farther the shot ([`tail_gain`]); between [`LAYER_FROM`] and the whole of
//!   the 3D sound's reach the weapon's own distant shot (WEAP Shoot Dist) is layered in
//!   under it, rising as the 3D one fades ([`distant_layer`]), so a mid-range shot sounds
//!   layered and not only quieter (beyond the reach the distant one alone, as before).
//! - **The player's own shot**: a low shelf adds weight under [`PUNCH_HZ`] and the whole
//!   is trimmed so it is not louder overall ([`PUNCH_LOW`], [`PUNCH_TRIM`]).
//! - **Silenced** shots (a Silence or Suppressor mod's sound set) keep [`SILENCED_TAIL`]
//!   of the tail.
//!
//! How it reaches the mixer: combat marks a shot's sound as it asks for it
//! ([`mark_shot`]), and `crate::sfx` takes the mark when that sound starts ([`take`]),
//! giving its samples a [`ShotShape`] that the decoder runs ([`ShotDsp`]). Marks not taken
//! within [`MARK_LIFE`] are dropped. With the switch off no mark is made and every shot
//! plays as recorded; tails already ringing finish.
//!
//! Cost: per shot voice a mono delay line of up to 1.6 s (≈300 KB at 48 kHz, made when it
//! starts) and per frame about ten taps, four all-passes and three one-pole filters.

use crate::modern_feel::ModernFeel;
use crate::reverb::Preset;
use bevy::prelude::*;
use fnv_combat::fx::ShootSounds;
use fnv_sim::FormId;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The first and the last echo outside, seconds.
pub const ECHO_FIRST: f32 = 0.18;
pub const ECHO_LAST: f32 = 1.45;
/// The air's low-pass at the shooter, and the dullest it gets, Hz.
pub const AIR_FULL_HZ: f32 = 20_000.0;
pub const AIR_MIN_HZ: f32 = 1_600.0;
/// Units over which the air's low-pass halves (about 17 m) .
pub const AIR_HALF: f32 = 1_200.0;
/// The share of the 3D sound's reach where the distant shot starts to come in.
pub const LAYER_FROM: f32 = 0.4;
/// The distant shot's level at the end of the 3D sound's reach (its record's own curve
/// and attenuation on top).
pub const LAYER_TOP: f32 = 0.85;
/// The player's punch: the shelf's corner, the low band's added share, the overall trim.
pub const PUNCH_HZ: f32 = 140.0;
pub const PUNCH_LOW: f32 = 0.45;
pub const PUNCH_TRIM: f32 = 0.8;
/// The share of the tail a silenced shot keeps.
pub const SILENCED_TAIL: f32 = 0.12;
/// A shot of the same sound within this of the last is automatic fire, and keeps this
/// share of its tail.
pub const RAPID: Duration = Duration::from_millis(250);
pub const RAPID_TAIL: f32 = 0.25;
/// How long a mark waits for its sound to start (its file may be read first).
pub const MARK_LIFE: Duration = Duration::from_millis(600);

/// The switch as the last frame read it (set by [`follow`]); off without a [`ModernFeel`].
static ON: AtomicBool = AtomicBool::new(false);
/// The listener is in an interior cell (set by [`follow`]).
static INSIDE: AtomicBool = AtomicBool::new(false);
/// A seed per shot, so no two tails are alike.
static SEED: AtomicU32 = AtomicU32::new(0x9e37_79b9);
/// The last shot of each sound heard lately (automatic fire, [`RAPID`]).
static LAST: Mutex<Vec<(FormId, Instant)>> = Mutex::new(Vec::new());
/// Shots marked and not yet started.
static MARKS: Mutex<Vec<Mark>> = Mutex::new(Vec::new());
/// Shots shaped: echo, slap, both, layered distant sounds; for the exit line.
static COUNTS: [AtomicU32; 4] = [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];
/// When the last `gun audio:` line was logged (bits of seconds since start), so automatic
/// fire does not flood the log.
static LOGGED: Mutex<Option<Instant>> = Mutex::new(None);

/// What kind of shot a marked sound is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShotKind {
    pub player: bool,
    pub silenced: bool,
    /// The 3D sound's reach (its record's maximum distance), 0 when not known.
    pub near_max: f32,
    /// The distant layer laid under a 3D shot: dry (no tail of its own).
    pub layer: bool,
}

#[derive(Clone, Copy, Debug)]
struct Mark {
    sound: FormId,
    at: Option<[u32; 3]>,
    kind: ShotKind,
    when: Instant,
}

fn bits(at: Option<[f32; 3]>) -> Option<[u32; 3]> {
    at.map(|a| a.map(f32::to_bits))
}

/// Whether layered gunshots are on now.
pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Mark the sound a shot is about to ask for (`crate::combat`): the sound, its place
/// (`None` at the listener) and what shot it is. Nothing when the switch is off.
pub fn mark_shot(sound: FormId, at: Option<[f32; 3]>, kind: ShotKind) {
    if !on() {
        return;
    }
    let Ok(mut marks) = MARKS.lock() else { return };
    let now = Instant::now();
    marks.retain(|m| now.duration_since(m.when) < MARK_LIFE);
    if marks.len() < 128 {
        marks.push(Mark { sound, at: bits(at), kind, when: now });
    }
}

/// The distant shot laid under another actor's 3D shot `distance` units off, and its
/// gain (module notes): from [`LAYER_FROM`] of the 3D sound's reach up to the reach,
/// rising smoothly to [`LAYER_TOP`]. `None` when off, nearer, beyond the reach (the
/// distant sound then plays alone, as before) or with no distant sound.
pub fn distant_layer(shoot: &ShootSounds, distance: f32, near_max: f32) -> Option<(FormId, f32)> {
    if !on() || near_max <= 0.0 || !distance.is_finite() {
        return None;
    }
    let dist = shoot.dist?;
    let t = distance / near_max;
    if !(LAYER_FROM..1.0).contains(&t) || Some(dist) == shoot.near {
        return None;
    }
    let x = (t - LAYER_FROM) / (1.0 - LAYER_FROM);
    let w = x * x * (3.0 - 2.0 * x) * LAYER_TOP;
    (w > 0.02).then_some((dist, w))
}

/// The air's low-pass at `distance` units, Hz ([`AIR_FULL_HZ`] for none).
pub fn air_cutoff(distance: f32) -> f32 {
    if distance < 300.0 {
        return AIR_FULL_HZ;
    }
    (AIR_FULL_HZ / (1.0 + (distance - 300.0) / AIR_HALF)).max(AIR_MIN_HZ)
}

/// The tail's level against the direct sound for a shot `distance` units off in a 3D
/// sound's reach `near_max`: whole near, up to twice at the reach's end and beyond.
pub fn tail_gain(distance: f32, near_max: f32) -> f32 {
    let reach = if near_max > 0.0 { near_max } else { 4000.0 };
    1.0 + (distance / reach).clamp(0.0, 1.0)
}

/// What a shot's tail is made of, by the place.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Space {
    /// Open air: the rolling echo.
    Open,
    /// Outside among buildings: a light slap and a lower echo.
    Street,
    /// Inside, the room's decay (seconds) sizing the slap.
    Room(f32),
}

impl Space {
    /// The space of the listener now: an interior is a room (its reverb's decay, else a
    /// medium room's), outside the built-up environments are streets, the rest open.
    pub fn now(room: Option<&Preset>) -> Space {
        if INSIDE.load(Ordering::Relaxed) {
            return Space::Room(room.map_or(1.3, |p| p.decay_time));
        }
        match room.map(|p| p.name) {
            Some("City" | "Alley" | "Parkinglot") => Space::Street,
            _ => Space::Open,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Space::Open => "open (rolling echo)",
            Space::Street => "street (slap + echo)",
            Space::Room(_) => "room (slap)",
        }
    }
}

/// The shaping a shot sound is played with ([`ShotDsp`]): its parameters, made on the
/// main thread when it starts; the decoder builds its state from them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShotShape {
    pub space: Space,
    /// The tail's level (0 for none: a distant layer).
    pub tail: f32,
    /// The air's low-pass on the direct sound, Hz ([`AIR_FULL_HZ`]: none).
    pub air_hz: f32,
    /// The player's punch.
    pub punch: bool,
    pub seed: u32,
}

/// The shaping of the sound `sound` starting now at `at` (`distance` from the listener),
/// if a shot marked it ([`mark_shot`]); the mark is used up. `room`: the listener's reverb
/// environment (`crate::sfx::Sfx::room_reverb`).
pub fn take(sound: FormId, at: Option<[f32; 3]>, distance: Option<f32>, room: Option<&Preset>) -> Option<ShotShape> {
    if !on() {
        return None;
    }
    let kind = {
        let mut marks = MARKS.lock().ok()?;
        let key = bits(at);
        let i = marks.iter().position(|m| m.sound == sound && m.at == key)?;
        marks.swap_remove(i).kind
    };
    let space = Space::now(room);
    let d = distance.unwrap_or(0.0);
    let mut tail = if kind.layer { 0.0 } else { tail_gain(d, kind.near_max) };
    if kind.silenced {
        tail *= SILENCED_TAIL;
    }
    // Automatic fire: a shot of the same sound soon after the last keeps less of its
    // tail; the bursts' tails overlapping whole drove a room's mix into the limiter.
    let now = Instant::now();
    if let Ok(mut last) = LAST.lock() {
        if last.iter().any(|&(s, t)| s == sound && now.duration_since(t) < RAPID) {
            tail *= RAPID_TAIL;
        }
        last.retain(|&(s, t)| s != sound && now.duration_since(t) < RAPID);
        last.push((sound, now));
    }
    let air_hz = air_cutoff(d);
    let seed = SEED.fetch_add(0x6d2b_79f5, Ordering::Relaxed);
    let shape = ShotShape { space, tail, air_hz, punch: kind.player && !kind.silenced, seed };
    let c = if kind.layer {
        3
    } else {
        match space {
            Space::Open => 0,
            Space::Room(_) => 1,
            Space::Street => 2,
        }
    };
    COUNTS[c].fetch_add(1, Ordering::Relaxed);
    if let Ok(mut last) = LOGGED.lock() {
        if last.is_none_or(|t| now.duration_since(t) > Duration::from_millis(400)) {
            *last = Some(now);
            info!(
                "gun audio: {} shot {sound} {}: {}, tail {:.2}, air {:.0} Hz{}{}",
                if kind.player { "player" } else { "npc" },
                distance.map_or("at the listener".to_string(), |d| format!("{d:.0} units off")),
                if kind.layer { "distant layer (dry)" } else { space.name() },
                tail,
                air_hz,
                if shape.punch { ", punch" } else { "" },
                if kind.silenced { ", silenced" } else { "" },
            );
        }
    }
    Some(shape)
}

/// The first value of the next random number (xorshift), 0..1.
fn rand(s: &mut u32) -> f32 {
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    (*s >> 8) as f32 / (1u32 << 24) as f32
}

/// The one-pole low-pass coefficient for `hz` at `rate` (`y += (1 − a)(x − y)`).
fn pole(hz: f32, rate: f32) -> f32 {
    (-std::f32::consts::TAU * hz.min(rate * 0.45) / rate).exp()
}

/// One echo or reflection.
struct Tap {
    delay: usize,
    gain: [f32; 2],
    a: f32,
    y: f32,
}

struct Allpass {
    buf: Vec<f32>,
    at: usize,
    g: f32,
}

impl Allpass {
    fn new(seconds: f32, rate: f32, g: f32) -> Allpass {
        Allpass { buf: vec![0.0; ((seconds * rate) as usize).max(1)], at: 0, g }
    }

    fn run(&mut self, x: f32) -> f32 {
        let d = self.buf[self.at];
        let v = x + self.g * d;
        self.buf[self.at] = v;
        self.at = (self.at + 1) % self.buf.len();
        d - self.g * v
    }
}

/// A shot's shaping in the decoder (module notes): stereo frames in, the direct sound
/// (air low-pass, punch) with its tail out; once the input ends, the tail rings out.
pub struct ShotDsp {
    air_a: f32,
    air: [f32; 2],
    punch: Option<(f32, [f32; 2])>,
    line: Vec<f32>,
    at: usize,
    taps: Vec<Tap>,
    diffuse: [[Allpass; 2]; 2],
    /// Frames of tail still to play once the input has ended.
    left: usize,
    rate: f32,
}

impl ShotDsp {
    pub fn new(shape: &ShotShape, rate: u32) -> ShotDsp {
        let r = rate.clamp(8_000, 192_000) as f32;
        let mut seed = shape.seed | 1;
        let mut taps = Vec::new();
        if shape.tail > 0.0 {
            // Outside: the rolling echo; among buildings lower, under a slap.
            let echo_level = match shape.space {
                Space::Open => 0.36,
                Space::Street => 0.22,
                Space::Room(_) => 0.0,
            };
            if echo_level > 0.0 {
                let n = 9;
                for i in 0..n {
                    let f = i as f32 / (n - 1) as f32;
                    let t = (ECHO_FIRST + (ECHO_LAST - ECHO_FIRST) * f.powf(1.35) + (rand(&mut seed) - 0.5) * 0.06).max(0.1);
                    let g = echo_level * (-(t - ECHO_FIRST) / 0.55).exp() * (0.65 + 0.35 * rand(&mut seed));
                    let hz = 3000.0 * (700.0f32 / 3000.0).powf(f);
                    let side = if i % 2 == 0 { 1.0 } else { -1.0 } * (0.3 + 0.5 * rand(&mut seed));
                    taps.push(Tap { delay: (t * r) as usize, gain: spread(side, g * shape.tail), a: pole(hz, r), y: 0.0 });
                }
            }
            // Inside and among buildings: the slap, early reflections sized by the room.
            // The reflections overlap the shot and one another, so their gains are scaled
            // so that their power sums to the level's (not their amplitudes: eight taps at
            // 0.5 each tripled the mix's peak in a room).
            let (slap_level, size) = match shape.space {
                Space::Room(decay) => (0.2, (decay / 1.5).clamp(0.6, 1.6)),
                Space::Street => (0.18, 0.8),
                Space::Open => (0.0, 1.0),
            };
            if slap_level > 0.0 {
                let first = taps.len();
                let n = 8;
                for i in 0..n {
                    let f = i as f32 / (n - 1) as f32;
                    let t = (0.018 + 0.102 * f.powf(1.2) + (rand(&mut seed) - 0.5) * 0.008) * size;
                    let g = slap_level * (1.0 - 0.7 * f) * (0.7 + 0.3 * rand(&mut seed));
                    let hz = 6500.0 * (2800.0f32 / 6500.0).powf(f);
                    let side = (rand(&mut seed) - 0.5) * 1.6;
                    taps.push(Tap { delay: (t * r).max(1.0) as usize, gain: spread(side, g * shape.tail), a: pole(hz, r), y: 0.0 });
                }
                let power: f32 = taps[first..].iter().map(|t| 0.5 * (t.gain[0].powi(2) + t.gain[1].powi(2))).sum();
                let k = slap_level * shape.tail / power.sqrt().max(1e-6);
                for t in &mut taps[first..] {
                    t.gain = t.gain.map(|g| g * k);
                }
            }
        }
        let longest = taps.iter().map(|t| t.delay).max().unwrap_or(0);
        let diffuse = [[Allpass::new(0.0113, r, 0.55), Allpass::new(0.0271, r, 0.5)], [Allpass::new(0.0137, r, 0.55), Allpass::new(0.0313, r, 0.5)]];
        let air_a = if shape.air_hz >= AIR_FULL_HZ { 0.0 } else { pole(shape.air_hz, r) };
        ShotDsp {
            air_a,
            air: [0.0; 2],
            punch: shape.punch.then(|| (pole(PUNCH_HZ, r), [0.0; 2])),
            line: if taps.is_empty() { Vec::new() } else { vec![0.0; longest + 1] },
            at: 0,
            // The all-passes' ring after the last tap: an echo's longer, a slap's short (its
            // voice holds a slot of the voice limit no longer than it must).
            left: if taps.is_empty() { (0.02 * r) as usize } else { longest + (if matches!(shape.space, Space::Room(_)) { 0.15 } else { 0.4 } * r) as usize },
            taps,
            diffuse,
            rate: r,
        }
    }

    /// Seconds of tail still to play.
    pub fn seconds_left(&self) -> f32 {
        self.left as f32 / self.rate
    }

    /// One frame.
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let mono = 0.5 * (l + r);
        let mut out = [l, r];
        if self.air_a > 0.0 {
            for (o, y) in out.iter_mut().zip(self.air.iter_mut()) {
                *y += (1.0 - self.air_a) * (*o - *y);
                *o = *y;
            }
        }
        if let Some((a, low)) = self.punch.as_mut() {
            for (o, y) in out.iter_mut().zip(low.iter_mut()) {
                *y += (1.0 - *a) * (*o - *y);
                *o = (*o + PUNCH_LOW * *y) * PUNCH_TRIM;
            }
        }
        let (wl, wr) = self.wet(mono);
        (out[0] + wl, out[1] + wr)
    }

    /// The next frame of the tail after the input has ended; `None` once it has rung out.
    pub fn tail(&mut self) -> Option<(f32, f32)> {
        if self.left == 0 {
            return None;
        }
        self.left -= 1;
        // The direct filters ring down too (a few samples).
        Some(self.process(0.0, 0.0))
    }

    fn wet(&mut self, x: f32) -> (f32, f32) {
        let n = self.line.len();
        if n == 0 {
            return (0.0, 0.0);
        }
        self.line[self.at] = x;
        let mut s = [0.0f32; 2];
        for t in self.taps.iter_mut() {
            let v = self.line[(self.at + n - t.delay.min(n - 1)) % n];
            t.y += (1.0 - t.a) * (v - t.y);
            s[0] += t.y * t.gain[0];
            s[1] += t.y * t.gain[1];
        }
        self.at = (self.at + 1) % n;
        for (ear, v) in s.iter_mut().enumerate() {
            for a in self.diffuse[ear].iter_mut() {
                *v = a.run(*v);
            }
        }
        (s[0], s[1])
    }
}

/// Left and right gains for a side −1 (left) … 1 (right), equal power, 1 each centred.
fn spread(side: f32, g: f32) -> [f32; 2] {
    let p = side.clamp(-1.0, 1.0);
    [g * (1.0 - p).sqrt(), g * (1.0 + p).sqrt()]
}

/// Follow the switch and whether the listener is inside.
fn follow(feel: Option<Res<ModernFeel>>, loc: Option<Res<crate::Location>>, mut was: Local<Option<bool>>) {
    let on = feel.is_some_and(|f| f.gun_audio());
    ON.store(on, Ordering::Relaxed);
    INSIDE.store(loc.is_some_and(|l| l.interior.is_some()), Ordering::Relaxed);
    if *was != Some(on) {
        *was = Some(on);
        info!("gun audio: layered gunshots {}", if on { "on" } else { "off" });
        if !on {
            if let Ok(mut m) = MARKS.lock() {
                m.clear();
            }
        }
    }
}

fn report(mut exit: MessageReader<AppExit>) {
    if exit.read().last().is_some() {
        let c = COUNTS.each_ref().map(|c| c.load(Ordering::Relaxed));
        if c.iter().any(|&n| n > 0) {
            info!("gun audio summary: {} shots with a rolling echo, {} with a room's slap, {} with a street's slap and echo, {} distant layers", c[0], c[1], c[2], c[3]);
        }
    }
}

/// Added by [`crate::sfx::SfxPlugin`].
pub(crate) fn build(app: &mut App) {
    app.add_systems(Update, follow.before(crate::sfx::SfxSet)).add_systems(Last, report);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shot_rings_on_after_its_sound() {
        for space in [Space::Open, Space::Street, Space::Room(1.3)] {
            let shape = ShotShape { space, tail: 1.0, air_hz: AIR_FULL_HZ, punch: false, seed: 7 };
            let mut d = ShotDsp::new(&shape, 22050);
            let (l, r) = d.process(1.0, 1.0);
            assert!((l - 1.0).abs() < 1e-6 && (r - 1.0).abs() < 1e-6, "dry passes at once");
            let mut energy = 0.0;
            while let Some((l, r)) = d.tail() {
                energy += l * l + r * r;
            }
            assert!(energy > 0.0, "{space:?} has a tail");
        }
    }

    /// Render a shot file (`NVF_SHOT_IN`, a WAV) through each space's shaping into
    /// `NVF_SHOT_OUT_<space>.wav` (16-bit stereo, full scale 4), to look at the tails' envelopes.
    #[test]
    #[ignore = "renders files named by the environment"]
    fn render_shot() {
        let (Ok(input), Ok(out)) = (std::env::var("NVF_SHOT_IN"), std::env::var("NVF_SHOT_OUT")) else { return };
        let pcm = crate::sfx::PcmSound::from_wav(&std::fs::read(&input).unwrap()).unwrap();
        let c = pcm.channels() as usize;
        let cases = [
            ("dry", None),
            ("open", Some(ShotShape { space: Space::Open, tail: 1.0, air_hz: AIR_FULL_HZ, punch: false, seed: 11 })),
            ("open-player", Some(ShotShape { space: Space::Open, tail: 1.0, air_hz: AIR_FULL_HZ, punch: true, seed: 11 })),
            ("open-far", Some(ShotShape { space: Space::Open, tail: tail_gain(3000.0, 4000.0), air_hz: air_cutoff(3000.0), punch: false, seed: 11 })),
            ("street", Some(ShotShape { space: Space::Street, tail: 1.0, air_hz: AIR_FULL_HZ, punch: false, seed: 11 })),
            ("room", Some(ShotShape { space: Space::Room(1.3), tail: 1.0, air_hz: AIR_FULL_HZ, punch: false, seed: 11 })),
            ("silenced", Some(ShotShape { space: Space::Open, tail: SILENCED_TAIL, air_hz: AIR_FULL_HZ, punch: false, seed: 11 })),
        ];
        for (name, shape) in cases {
            let mut dsp = shape.map(|s| ShotDsp::new(&s, pcm.rate()));
            let mut frames: Vec<(f32, f32)> = Vec::new();
            for f in pcm.samples().chunks(c) {
                let (l, r) = if c == 1 { (f[0], f[0]) } else { (f[0], f[1]) };
                frames.push(dsp.as_mut().map_or((l, r), |d| d.process(l, r)));
            }
            while let Some(f) = dsp.as_mut().and_then(ShotDsp::tail) {
                frames.push(f);
            }
            // `NVF_SHOT_REVERB=<environment>`: then through that room's reverb, as in play.
            if let Some(p) = std::env::var("NVF_SHOT_REVERB").ok().and_then(|n| crate::reverb::preset(Some(&n))) {
                let mut rv = crate::reverb::Reverb::new(p, pcm.rate());
                frames = frames.into_iter().map(|(l, r)| rv.process(l, r)).collect();
                while let Some(f) = rv.tail() {
                    frames.push(f);
                }
            }
            // `NVF_SHOT_BURST=<shots>:<seconds apart>`: that many overlapping, as a burst.
            if let Some((n, gap)) = std::env::var("NVF_SHOT_BURST").ok().and_then(|b| b.split_once(':').map(|(n, g)| (n.parse::<usize>().unwrap_or(1), g.parse::<f32>().unwrap_or(0.1)))) {
                let step = (gap * pcm.rate() as f32) as usize;
                let mut sum = vec![(0.0f32, 0.0f32); frames.len() + step * n];
                for k in 0..n {
                    for (i, f) in frames.iter().enumerate() {
                        sum[i + k * step].0 += f.0;
                        sum[i + k * step].1 += f.1;
                    }
                }
                frames = sum;
            }
            let mut v = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
            v.extend(16u32.to_le_bytes());
            v.extend(1u16.to_le_bytes());
            v.extend(2u16.to_le_bytes());
            v.extend(pcm.rate().to_le_bytes());
            v.extend((pcm.rate() * 4).to_le_bytes());
            v.extend(4u16.to_le_bytes());
            v.extend(16u16.to_le_bytes());
            v.extend(b"data");
            v.extend(((frames.len() * 4) as u32).to_le_bytes());
            for (l, r) in frames {
                v.extend(((l.clamp(-4.0, 4.0) * 8191.0) as i16).to_le_bytes());
                v.extend(((r.clamp(-4.0, 4.0) * 8191.0) as i16).to_le_bytes());
            }
            std::fs::write(format!("{out}_{name}.wav"), v).unwrap();
        }
    }

    #[test]
    fn the_layer_comes_in_toward_the_reach() {
        ON.store(true, Ordering::Relaxed);
        let s = ShootSounds { own: Some(FormId(1)), near: Some(FormId(1)), dist: Some(FormId(2)), silenced: false };
        assert_eq!(distant_layer(&s, 100.0, 1000.0), None);
        let mid = distant_layer(&s, 700.0, 1000.0).unwrap();
        let far = distant_layer(&s, 950.0, 1000.0).unwrap();
        assert!(mid.1 < far.1);
        assert_eq!(distant_layer(&s, 1000.0, 1000.0), None);
        assert!(air_cutoff(5000.0) < air_cutoff(1000.0));
    }
}
