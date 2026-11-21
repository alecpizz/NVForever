//! Sound effects in the game (docs/spec-notes/combat-rules.md §16): SOUN records played
//! on request ([`PlaySound`]), today the sounds of the player's shots: the weapon firing,
//! clicking empty and being drawn or put away, impacts by material, explosions.
//!
//! - **The record** [documented: xEdit SOUN]: a file or a folder under `sound\`, flags,
//!   a minimum and a maximum attenuation distance, a static attenuation in hundredths of
//!   a decibel and a frequency adjustment in percent.
//! - **The file.** A folder stands for the `.wav` and `.ogg` files directly in it; one is
//!   picked at random each time [hypothesis: from the folders holding a few takes of one
//!   sound]. A record names a `.wav`; where the archives hold an `.ogg` of that name
//!   instead, that is the file [observed: 2,184 of the base sound archive's effects are
//!   Ogg Vorbis, the ambient ones above all, docs/spec-notes/audio.md §5]. WAV files of
//!   PCM (8, 16 or 24 bits) and 32-bit float are decoded here, Ogg Vorbis by rodio, both
//!   to samples played through Bevy's audio as a source of our own ([`PcmSound`]).
//!   Other encodings are reported once and not played.
//! - **Off the main thread** (#1428) [engineering choice]: a file is read and decoded on a
//!   worker thread the first time it is asked for ([`FileState`]); the request waits for
//!   it ([`Waiting`]) and starts once it is ready, with the listener as it is then; a
//!   game event's one-shot whose file comes in more than [`MAX_ONE_SHOT_WAIT`] after it
//!   was asked for is dropped instead. A
//!   folder's files come from an index of the folders the SOUN records name, listed on
//!   a worker at the start; a request for a folder before it is in waits for it. Before, the first playing of a file read and decoded it in
//!   the frame (9-15 ms in the slow frames of the profiling campaign's exterior runs).
//!   What the original game does with a sound whose file is not in memory yet is not
//!   known. The exit report counts the requests that began at once and those that waited.
//! - **A second chance in the frame** (#1428) [engineering choice]: late in the frame,
//!   before the audio output takes up new players ([`late_start`], `PostUpdate`), the
//!   decodes finished since [`play`] are taken in once more and the requests whose file
//!   is now ready start as [`play`] starts them, so a file decoded a few milliseconds
//!   after it was asked for can start in that frame. The exit report counts them apart.
//! - **Loudness**: the static attenuation is taken off (gain 10^(−dB ÷ 20)). A sound with
//!   a place follows its record's attenuation curve [documented: GECK wiki Sound,
//!   Attenuation tab: five sliders, "the sound's volume percentage at its inner radius"
//!   down to "at its outer radius", the curve between, "logarithmic" by default]: the
//!   first point within its minimum distance of the listener (the player's eye), the last
//!   at its maximum distance, silent beyond ([`distance_gain`]). The three middle points
//!   stand evenly between the two distances and the volume goes straight from point to
//!   point ([`curve_gain`]) [hypothesis]; a percentage is a share of the amplitude
//!   [hypothesis]. A record without a curve plays the default 100, 50, 20, 5, 0.
//! - **Panning** [design; how the game places sounds is not known]: a sound with a place
//!   is heard more on the side of the listener it is on ([`pan_gains`]): the ear facing
//!   it hears it whole, the other ear loses up to [`PAN_DEPTH`] of it, by how far to the
//!   side it is (straight ahead, behind, above or below: both ears alike). A panned sound
//!   is played in stereo, its channels mixed to one first. The listener is the player's
//!   eye, facing the player's heading; [`pan_sounds`] follows the listener while a sound
//!   plays ([`SoundAt`]), and the sound's gains ease to the new ones, a whole unit over
//!   [`PAN_EASE`] (20 ms, #2217), its first frame at them as they are, one pair of gains
//!   for both ears of a frame ([`PanEase`], in both decoders: so also for the other
//!   modules' panned streams). Sounds at the listener (the player's own, interface
//!   sounds) and the background loops are not panned. Other modules' positioned streams
//!   use [`Panned`] (placed loops in [`crate::ambient`], radios in [`crate::radio`],
//!   voices said at a place in `crate::voice`).
//! - **Pitch**: the frequency adjustment, and ±5% at random with the Random Frequency
//!   Shift flag [design: the size of the shift].
//! - **Loops.** A [`LoopSound`] starts a SOUN going round and stops it again (a
//!   weapon's Shoot 3D Looping sound while it fires).
//! - **Stops and voice slots** (#2217) [design; how the original stops a sound and what
//!   its voice limits are is not known]. A playing sound stopped before its end (a
//!   LoopSound off or on again, StopSound without a fade, a one-shot taken over at
//!   [`MAX_SAME`]) fades out over `audio_out::DECLICK_FADE` (`audio_out::stop_sound`),
//!   instead of being cut mid-waveform; the stop drops its marker ([`Looping`],
//!   [`ScriptPlayed`]) at once, so it is no longer the loop or the script's sound. One
//!   started earlier in the same frame has made no sound yet and goes at once. Every
//!   one-shot takes a voice slot ([`Slot`]) until its sink is empty, also while it fades
//!   out, so [`MAX_SAME`] and [`MAX_VOICES`] hold for what is heard: a one-shot over its
//!   SOUN's limit has a slot's sound fade out (one already fading, else the oldest) and
//!   waits for it ([`Replacement`], at most one a slot), starting once that sound has
//!   ended (5 ms and a frame or so later), with the file and the pitch chosen when it was
//!   asked for and the listener as it is then (gain, pan and place worked out again); a
//!   game event's one-shot waiting more than [`MAX_ONE_SHOT_WAIT`] in all is dropped, a
//!   script's is answered when it starts. With every slot of the SOUN already promised,
//!   one more is refused. A StopSound drops the script's replacements asked for before
//!   it, as it drops the requests waiting for their file. Exit lines: `sfx hard stops:
//!   …` (sounds still cut: none should be), `sfx declicks: … ; C completed, W pending;
//!   …`, `sfx voices: at most V total, S of one SOUN; Q replacements waiting, R
//!   replacements started, C replacements cancelled, E replacements expired, M
//!   replacements silenced at start, P replacements still waiting; I one-shots started
//!   at once over one not heard yet` ([`Sfx::voices_line`]; Q: requests that waited for
//!   a slot, Q = R + C + E + M + P; R: those started in their slot; I: one-shots that
//!   took over one of their SOUN started in the same frame, no wait). `sfx files: …`
//!   counts a replacement when it starts or is silenced, not when it takes its slot, by
//!   the frames from its request to its start (its slot's wait in them); at its end, how
//!   soon the requests' files were ready, apart.
//! - **Things that sound while they are there** ([`SoundSource`]): a creature's conscious
//!   loop, an activator's or a container's looping sound; sent every frame they should
//!   sound and played by [`crate::ambient`] as it plays sound markers.
//! - **Reverb** ([`crate::reverb`]): a world sound plays through the reverb environment
//!   of the listener's acoustic space as it starts, in stereo, with a tail after its file.
//! - The sounds animations and doors name in their text keys
//!   (reloads, footsteps, armour rustle, doors) are sent here by nvforever's
//!   `anim_sounds.rs` (docs/spec-notes/audio.md §10). Placed sounds and the cells'
//!   background loops are [`crate::ambient`]; music is [`crate::music`].
//!
//! [`SfxSettings`] turns them off (`nvforever --no-sound`) and sets the volume.

use crate::player::Player;
use crate::reverb::{Preset, Reverb, RoomSend};
use crate::voice_filter::VoiceFilter;
use crate::PlayWorld;
use bevy::audio::{ChannelCount, Decodable, Sample, SampleRate, Source, Volume};
use crate::audio_out::{stop_sound, AddAudioSource, KeepsSpeed, Retiring, SoundCategory, SoundStops, StopReason};
use bevy::prelude::*;
use bevy::tasks::{futures::check_ready, IoTaskPool, Task, TaskPool};
use fnv_combat::fx::{FxForms, Sound, DEFAULT_ATTENUATION_CURVE};
use fnv_combat::rng::SplitMix;
use fnv_sim::FormId;
use std::collections::{HashMap, HashSet};
use std::num::NonZero;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How sound effects behave; insert before the play plugins to change the defaults.
#[derive(Resource, Clone, Copy, Debug)]
pub struct SfxSettings {
    pub play: bool,
    /// Linear volume, 1 = as recorded less the record's attenuation.
    pub volume: f32,
    /// World sounds through the acoustic space's reverb ([`crate::reverb`]).
    pub reverb: bool,
}

impl Default for SfxSettings {
    fn default() -> Self {
        SfxSettings { play: true, volume: 1.0, reverb: true }
    }
}

/// Play a SOUN: at a place (game space), or with `None` at the listener.
#[derive(Message, Clone, Copy, Debug)]
pub struct PlaySound {
    pub sound: FormId,
    pub at: Option<[f32; 3]>,
}

/// Play a SOUN once as [`PlaySound`] does, its loudness times `gain` (1 = as the record
/// has it): the shot sounds under the player's Weapon Volume Scaling tweak
/// [design: docs/design/tweaks.md].
#[derive(Message, Clone, Copy, Debug)]
pub struct ScaledSound {
    pub sound: FormId,
    pub at: Option<[f32; 3]>,
    pub gain: f32,
}

/// Start (`on`) or stop a SOUN that goes round until it is stopped, at the listener.
#[derive(Message, Clone, Copy, Debug)]
pub struct LoopSound {
    pub sound: FormId,
    pub on: bool,
}

/// A looping sound being played.
#[derive(Component)]
struct Looping(FormId);

/// A sound a script played (`fnv_game::nvse_sound`), which StopSound stops.
#[derive(Component)]
struct ScriptPlayed(FormId);

/// Sounds scripts play, between the rules and this module: what to play (from
/// `crate::world_sounds`; its id, the SOUN, its place), what StopSound stopped (the SOUN
/// and the id of the last sound asked for before the stop, `fnv_game::nvse_sound`), and
/// the answer for each played sound, its seconds or `None` for one that loops until
/// stopped.
#[derive(Resource, Default)]
pub struct ScriptSounds {
    pub plays: Vec<(u32, FormId, Option<[f32; 3]>)>,
    pub stops: Vec<(FormId, u32)>,
    pub heard: Vec<(u32, Option<f32>)>,
    /// Sounds made for files (`fnv_game::nvse_jg9`, PlaySoundFromPath and the like): the
    /// id, the Data path and whether it goes round; played as SOUNs from then on.
    pub paths: Vec<(FormId, String, bool)>,
    /// Script sounds (request ids) that fade in, over seconds (PlaySoundFade, a fade time).
    pub fades_in: Vec<(u32, f32)>,
    /// Sounds whose stop fades them out over seconds instead of at once.
    pub fades_out: Vec<(FormId, f32)>,
    /// Items a script gave the player whose pick-up sound plays at the listener
    /// (`script_events::ScriptEffect::ItemAddedSound`, #3095).
    pub items_added: Vec<FormId>,
}

/// A script's sound fading in or out (`fnv_game::nvse_jg9`): seconds in all and so far,
/// and its full volume (read from its player the first frame it has one). One fading out
/// is gone at its end.
#[derive(Component, Debug)]
pub struct ScriptFade {
    pub seconds: f32,
    pub elapsed: f32,
    pub out: bool,
    full: Option<f32>,
}

impl ScriptFade {
    fn new(seconds: f32, out: bool) -> ScriptFade {
        ScriptFade { seconds: seconds.max(0.001), elapsed: 0.0, out, full: None }
    }

    /// The share of the full volume now: rising from 0 or falling to 0 in a straight line
    /// [hypothesis: the pages give no curve].
    pub fn share(&self) -> f32 {
        let t = (self.elapsed / self.seconds).clamp(0.0, 1.0);
        if self.out { 1.0 - t } else { t }
    }
}

/// Fades on: each fading sound's volume set from its share, those faded in left at full
/// volume, those faded out gone.
fn script_fades(mut commands: Commands, time: Res<Time>, mut fading: Query<(Entity, &mut ScriptFade, Option<&mut AudioSink>), Without<Retiring>>) {
    let dt = time.delta_secs();
    for (e, mut f, sink) in &mut fading {
        let Some(mut sink) = sink else { continue };
        let full = *f.full.get_or_insert_with(|| sink.volume().to_linear());
        f.elapsed += dt;
        sink.set_volume(Volume::Linear(full * f.share()));
        if f.elapsed >= f.seconds {
            if f.out {
                // At zero volume already: removed, no fade of its own needed.
                commands.entity(e).try_despawn();
            } else {
                commands.entity(e).remove::<ScriptFade>();
            }
        }
    }
}

/// Shares of their full volume for [`LoopSound`] loops, by SOUN (1 when not named): a
/// module fading a loop it started sets its share here (`crate::heartbeat`).
#[derive(Resource, Default, Debug)]
pub struct LoopScale(pub HashMap<FormId, f32>);

/// A loop's full volume, read from its sink the first time [`loop_scales`] sees it.
#[derive(Component)]
struct LoopFull(f32);

/// Each [`LoopSound`] loop at its [`LoopScale`] share of its full volume.
fn loop_scales(mut commands: Commands, scale: Res<LoopScale>, mut q: Query<(Entity, &Looping, Option<&mut AudioSink>, Option<&LoopFull>)>) {
    if scale.0.is_empty() && !scale.is_changed() {
        return;
    }
    for (e, l, sink, full) in &mut q {
        let Some(mut sink) = sink else { continue };
        let full = match full {
            Some(f) => f.0,
            None => {
                let v = sink.volume().to_linear();
                commands.entity(e).insert(LoopFull(v));
                v
            }
        };
        let want = full * scale.0.get(&l.0).copied().unwrap_or(1.0).clamp(0.0, 1.0);
        if (sink.volume().to_linear() - want).abs() > 1e-4 {
            sink.set_volume(Volume::Linear(want));
        }
    }
}

/// Something that sounds as long as it is there, at a place (game space): sent every
/// frame it should sound; [`crate::ambient`] plays its SOUN as it plays a sound marker
/// (looping, or now and then for Play At Random) while the listener is within reach.
/// `key` tells one source from another ([`SoundSource::key`]).
#[derive(Message, Clone, Copy, Debug)]
pub struct SoundSource {
    pub key: u64,
    pub sound: FormId,
    pub at: [f32; 3],
    /// Go round whatever the sound's flags say (a weapon's idle sound, audio.md §15.2).
    pub looping: bool,
}

impl SoundSource {
    /// A key from a kind of source (one per sender and use) and a reference.
    pub fn key(kind: u8, reference: u32) -> u64 {
        (u64::from(kind) << 32) | u64::from(reference)
    }
}

/// How much of a sound straight to one side the far ear loses [design].
pub const PAN_DEPTH: f32 = 0.7;

/// The left and right gains of a sound at `at` heard from `eye` facing the compass
/// `heading` (radians, 0 north, π/2 east; game space, z up): see the module notes.
pub fn pan_gains(eye: [f32; 3], heading: f32, at: [f32; 3]) -> [f32; 2] {
    let d = [at[0] - eye[0], at[1] - eye[1], at[2] - eye[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    // A broken place (NaN, infinite) is heard in the middle, not as NaN gains.
    if len < 1.0 || !len.is_finite() || !heading.is_finite() {
        return [1.0, 1.0];
    }
    // The listener's right: the heading turned a quarter clockwise.
    let side = ((d[0] * heading.cos() - d[1] * heading.sin()) / len).clamp(-1.0, 1.0);
    if side >= 0.0 {
        [1.0 - PAN_DEPTH * side, 1.0]
    } else {
        [1.0, 1.0 + PAN_DEPTH * side]
    }
}

/// Left and right gains shared with the audio thread while a sound plays: the targets
/// the main thread sets; the sound eases to them ([`PanEase`]). The third is how much the
/// sound is occluded (Modern Feel, [`crate::occlusion`]; 0 clear).
#[derive(Clone, Debug)]
pub struct Pan(Arc<[AtomicU32; 3]>);

impl Pan {
    pub fn new(gains: [f32; 2]) -> Pan {
        Pan(Arc::new([AtomicU32::new(gains[0].to_bits()), AtomicU32::new(gains[1].to_bits()), AtomicU32::new(0)]))
    }

    /// Set how occluded the sound is ([`crate::occlusion::muffle`]; 0 clear).
    pub fn set_occlusion(&self, o: f32) {
        self.0[2].store(o.max(0.0).to_bits(), Ordering::Relaxed);
    }

    pub fn occlusion(&self) -> f32 {
        f32::from_bits(self.0[2].load(Ordering::Relaxed))
    }

    pub fn set(&self, gains: [f32; 2]) {
        self.0[0].store(gains[0].to_bits(), Ordering::Relaxed);
        self.0[1].store(gains[1].to_bits(), Ordering::Relaxed);
    }

    pub fn get(&self) -> [f32; 2] {
        [f32::from_bits(self.0[0].load(Ordering::Relaxed)), f32::from_bits(self.0[1].load(Ordering::Relaxed))]
    }
}

/// A sound playing at a place: [`pan_sounds`] turns its [`Pan`] with the listener.
#[derive(Component, Clone, Debug)]
pub struct SoundAt {
    pub at: [f32; 3],
    pub pan: Pan,
}

impl SoundAt {
    /// A sound at `at`, panned for the listener as it is now (centred without one).
    pub fn new(at: [f32; 3], player: Option<&Player>) -> SoundAt {
        let gains = player.map_or([1.0, 1.0], |p| pan_gains(p.eye(), p.heading, at));
        SoundAt { at, pan: Pan::new(gains) }
    }
}

/// How long a panned sound's gain takes to move a whole unit to a new target (#2217)
/// [design: the same scale as `audio_out::VOLUME_EASE`, so a turn of the listener or a
/// sound passing does not step the gains between two blocks of samples; how the original
/// moves its pan is not known]. Timed in the sound's own frames, so slowed playing
/// stretches it as it stretches the sound.
pub const PAN_EASE: Duration = Duration::from_millis(20);

/// The gains a panned sound plays at, its decoder's own (never the shared [`Pan`]'s):
/// the first frame plays at the targets as they are, each one after moves each gain
/// toward its target by at most `step` (a whole unit over [`PAN_EASE`] at the source's
/// rate); one pair for both channels of a frame. A target that is not a number leaves
/// its gain where it is (centred, 1, before the first frame). Nothing allocated, no lock.
#[derive(Clone, Debug)]
struct PanEase {
    now: Option<[f32; 2]>,
    step: f32,
    /// The occlusion's filter ([`crate::occlusion::Muffle`]).
    muffle: crate::occlusion::Muffle,
}

impl PanEase {
    fn new(rate: u32) -> PanEase {
        PanEase { now: None, step: 1.0 / (rate.max(1) as f32 * PAN_EASE.as_secs_f32()), muffle: crate::occlusion::Muffle::new(rate) }
    }

    /// The gains of the next frame, for `target`.
    fn next(&mut self, target: [f32; 2]) -> [f32; 2] {
        let now = match self.now {
            None => target.map(|t| if t.is_finite() { t } else { 1.0 }),
            Some(now) => [0, 1].map(|i| if target[i].is_finite() { now[i] + (target[i] - now[i]).clamp(-self.step, self.step) } else { now[i] }),
        };
        self.now = Some(now);
        now
    }
}

/// One frame of `channels` samples mixed to one and placed by `pan`, at the gains `ease`
/// has reached: the left sample and the right one; `None` when the input has ended (the
/// gains not moved for it).
fn pan_frame(frame: impl Iterator<Item = Sample>, channels: usize, pan: &Pan, ease: &mut PanEase, filter: Option<&mut VoiceFilter>) -> Option<(Sample, Sample)> {
    let mut sum = 0.0;
    let mut n = 0;
    for s in frame.take(channels.max(1)) {
        sum += s;
        n += 1;
    }
    if n == 0 {
        return None;
    }
    let mut m = sum / channels.max(1) as f32;
    if let Some(f) = filter {
        m = f.run(m);
    }
    m = ease.muffle.run(m, pan.occlusion());
    let [l, r] = ease.next(pan.get());
    Some((m * l, m * r))
}

/// Another module's sound played at a place: its source, mixed to one channel and
/// panned, in stereo.
#[derive(Asset, TypePath, Clone)]
pub struct Panned<T: Decodable + TypePath + Clone> {
    pub inner: T,
    pub pan: Pan,
    /// Played through this environment's reverb ([`crate::reverb`]).
    pub reverb: Option<RoomSend>,
    /// Played through the voice filter ([`crate::voice_filter`]).
    pub voice_filter: bool,
}

impl<T: Decodable + TypePath + Clone> Panned<T> {
    /// Panned, without reverb.
    pub fn new(inner: T, pan: Pan) -> Panned<T> {
        Panned { inner, pan, reverb: None, voice_filter: false }
    }
}

pub struct PanDecoder<S> {
    input: S,
    pan: Pan,
    ease: PanEase,
    right: Option<Sample>,
    reverb: Option<Reverb>,
    filter: Option<VoiceFilter>,
}

impl<S: Source> Iterator for PanDecoder<S> {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        if let Some(r) = self.right.take() {
            return Some(r);
        }
        let c = self.input.channels().get() as usize;
        let (l, r) = with_reverb(pan_frame(&mut self.input, c, &self.pan, &mut self.ease, self.filter.as_mut()), &mut self.reverb)?;
        self.right = Some(r);
        Some(l)
    }
}

/// A frame through a sound's reverb, if it has one; once the input has ended
/// (`None`), the reverb's tail.
fn with_reverb(frame: Option<(Sample, Sample)>, reverb: &mut Option<Reverb>) -> Option<(Sample, Sample)> {
    match (frame, reverb) {
        (Some((l, r)), Some(rv)) => Some(rv.process(l, r)),
        (None, Some(rv)) => rv.tail(),
        (frame, None) => frame,
    }
}

impl<S: Source> Source for PanDecoder<S> {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        NonZero::new(2).unwrap_or(NonZero::<u16>::MIN)
    }

    fn sample_rate(&self) -> SampleRate {
        self.input.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        let tail = self.reverb.as_ref().map_or(0.0, Reverb::seconds_left);
        self.input.total_duration().map(|d| d + Duration::from_secs_f32(tail))
    }
}

impl<T: Decodable + TypePath + Clone> Decodable for Panned<T> {
    type Decoder = PanDecoder<T::Decoder>;

    fn decoder(&self) -> Self::Decoder {
        let input = self.inner.decoder();
        let reverb = self.reverb.map(|p| p.reverb(input.sample_rate().get()));
        let filter = self.voice_filter.then(|| VoiceFilter::new(input.sample_rate().get()));
        let ease = PanEase::new(input.sample_rate().get());
        PanDecoder { input, pan: self.pan.clone(), ease, right: None, reverb, filter }
    }
}

/// Decoded samples of a sound file.
#[derive(Asset, TypePath, Clone, Debug)]
pub struct PcmSound {
    samples: Arc<[f32]>,
    channels: u16,
    rate: u32,
    /// Played at a place: mixed to one channel and panned, in stereo.
    pan: Option<Pan>,
    /// Played through this environment's reverb, in stereo ([`crate::reverb`]).
    reverb: Option<RoomSend>,
    /// A shot's tail and distance shaping, in stereo ([`crate::gun_audio`]).
    shot: Option<crate::gun_audio::ShotShape>,
}

pub struct PcmDecoder {
    sound: PcmSound,
    at: usize,
    /// The gains a placed sound has reached ([`PanEase`]).
    ease: PanEase,
    right: Option<Sample>,
    reverb: Option<Reverb>,
    shot: Option<crate::gun_audio::ShotDsp>,
}

impl PcmDecoder {
    /// The next frame as a stereo pair (panned, or a mono sound on both ears, or the
    /// first two channels); `None` at the end.
    fn stereo_frame(&mut self) -> Option<(Sample, Sample)> {
        let c = self.sound.channels.max(1) as usize;
        let frame = self.sound.samples.get(self.at..self.at + c)?;
        self.at += c;
        match &self.sound.pan {
            Some(pan) => pan_frame(frame.iter().copied(), c, pan, &mut self.ease, None),
            None if c == 1 => Some((frame[0], frame[0])),
            None => Some((frame[0], frame[1])),
        }
    }
}

impl Iterator for PcmDecoder {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        if self.sound.pan.is_none() && self.reverb.is_none() && self.shot.is_none() {
            let s = self.sound.samples.get(self.at).copied();
            self.at += 1;
            return s;
        }
        if let Some(r) = self.right.take() {
            return Some(r);
        }
        let frame = match (self.stereo_frame(), self.shot.as_mut()) {
            (Some((l, r)), Some(shot)) => Some(shot.process(l, r)),
            (None, Some(shot)) => shot.tail(),
            (frame, None) => frame,
        };
        let (l, r) = with_reverb(frame, &mut self.reverb)?;
        self.right = Some(r);
        Some(l)
    }
}

impl Source for PcmDecoder {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        let c = if self.sound.pan.is_some() || self.reverb.is_some() || self.shot.is_some() { 2 } else { self.sound.channels };
        NonZero::new(c).unwrap_or(NonZero::<u16>::MIN)
    }

    fn sample_rate(&self) -> SampleRate {
        NonZero::new(self.sound.rate).unwrap_or(NonZero::<u32>::MIN)
    }

    fn total_duration(&self) -> Option<Duration> {
        let tail = self.reverb.as_ref().map_or(0.0, Reverb::seconds_left) + self.shot.as_ref().map_or(0.0, crate::gun_audio::ShotDsp::seconds_left);
        Some(Duration::from_secs_f64(self.sound.seconds() + tail as f64))
    }
}

impl Decodable for PcmSound {
    type Decoder = PcmDecoder;

    fn decoder(&self) -> PcmDecoder {
        let reverb = self.reverb.map(|p| p.reverb(self.rate));
        let shot = self.shot.map(|s| crate::gun_audio::ShotDsp::new(&s, self.rate));
        PcmDecoder { sound: self.clone(), at: 0, ease: PanEase::new(self.rate), right: None, reverb, shot }
    }
}

/// Whether Bevy's own player can open `bytes` (an `AudioSource`): it stops the game on a
/// file it cannot open [observed: bevy_audio 0.19.1 `AudioSource::decoder` unwraps the
/// decoder]. So a broken voice or holotape file is checked here first, the same way that
/// player opens it, and is skipped when it fails.
pub fn playable(bytes: &[u8]) -> Result<(), String> {
    let data = std::io::Cursor::new(bytes.to_vec());
    nvf_assets::guard::guarded(|| {
        let d = rodio::Decoder::builder().with_byte_len(bytes.len() as u64).with_data(data).build().map_err(|e| e.to_string())?;
        format_ok(d.channels().get(), d.sample_rate().get())
    })
}

/// Sample rates a sound file may have; one outside is a broken header. rodio's rate
/// converter steps through the file rate's ratio to the output's for every frame, so a
/// rate of billions stalls the audio thread [observed: rodio 0.22.2
/// `conversions/sample_rate.rs`].
pub const RATES: std::ops::RangeInclusive<u32> = 1_000..=384_000;

/// The most channels a sound file may have [engineering choice: 7.1; the game's files
/// are mono or stereo].
pub const MAX_CHANNELS: u16 = 8;

/// The most samples a sound effect decoded whole keeps: 128 MB, over six minutes of
/// 44.1 kHz stereo [engineering choice: a broken Ogg file's packets can each unpack to a
/// whole block, so its decoded size is not bounded by its length].
pub const MAX_DECODED_SAMPLES: usize = 32 << 20;

/// `Err` naming what is wrong when a file's channels or sample rate cannot be played
/// ([`RATES`], [`MAX_CHANNELS`]).
pub fn format_ok(channels: u16, rate: u32) -> Result<(), String> {
    if !RATES.contains(&rate) {
        return Err(format!("sample rate {rate} out of range"));
    }
    if !(1..=MAX_CHANNELS).contains(&channels) {
        return Err(format!("{channels} channels"));
    }
    Ok(())
}

/// `bytes` of the file at `path` when [`playable`]; otherwise a warning, and `None` (the
/// line goes on silent for its usual length).
pub fn checked(path: &str, bytes: Vec<u8>) -> Option<Vec<u8>> {
    let bytes = nvf_assets::guard::guarded(|| Ok(nvf_assets::data::test_broken(path, bytes))).unwrap_or_else(|e| {
        warn!("sound not loaded: {path}: {e}; it is skipped");
        None
    })?;
    match playable(&bytes) {
        Ok(()) => Some(bytes),
        Err(e) => {
            warn!("sound not loaded: {path}: {e}; it is skipped");
            None
        }
    }
}

impl PcmSound {
    pub fn seconds(&self) -> f64 {
        self.samples.len() as f64 / (self.channels.max(1) as f64 * self.rate.max(1) as f64)
    }

    /// Interleaved samples (`channels` per frame, `rate` frames per second), unplaced.
    pub fn from_samples(samples: Vec<f32>, channels: u16, rate: u32) -> PcmSound {
        PcmSound { samples: samples.into(), channels, rate, pan: None, reverb: None, shot: None }
    }

    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Decode a whole file of another encoding (Ogg Vorbis) with rodio.
    pub fn from_encoded(bytes: Vec<u8>) -> Result<PcmSound, String> {
        let d = rodio::Decoder::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
        let (channels, rate) = (d.channels().get(), d.sample_rate().get());
        format_ok(channels, rate)?;
        let samples: Vec<f32> = d.take(MAX_DECODED_SAMPLES).map(|x| if x.is_finite() { x } else { 0.0 }).collect();
        Ok(PcmSound { samples: samples.into(), channels, rate, pan: None, reverb: None, shot: None })
    }

    /// Decode a WAV file: the `fmt ` and `data` chunks of a RIFF/WAVE file [documented:
    /// the WAVE format]. `Err` names what is not supported.
    pub fn from_wav(bytes: &[u8]) -> Result<PcmSound, String> {
        if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return Err("not a RIFF/WAVE file".into());
        }
        let u16_at = |b: &[u8], i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        let u32_at = |b: &[u8], i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        let mut fmt: Option<(u16, u16, u32, u16)> = None;
        let mut at = 12;
        while at + 8 <= bytes.len() {
            let id = &bytes[at..at + 4];
            let len = u32_at(bytes, at + 4) as usize;
            let body = &bytes[at + 8..(at + 8 + len).min(bytes.len())];
            match id {
                b"fmt " if body.len() >= 16 => {
                    let mut tag = u16_at(body, 0);
                    // WAVE_FORMAT_EXTENSIBLE: the real format is the first two bytes of
                    // the sub-format.
                    if tag == 0xFFFE && body.len() >= 26 {
                        tag = u16_at(body, 24);
                    }
                    fmt = Some((tag, u16_at(body, 2), u32_at(body, 4), u16_at(body, 14)));
                }
                b"data" => {
                    let (tag, channels, rate, bits) = fmt.ok_or("data before fmt")?;
                    let samples: Vec<f32> = match (tag, bits) {
                        (1, 8) => body.iter().map(|&b| (b as f32 - 128.0) / 128.0).collect(),
                        (1, 16) => body.as_chunks::<2>().0.iter().map(|c| i16::from_le_bytes(*c) as f32 / 32768.0).collect(),
                        (1, 24) => body.as_chunks::<3>().0.iter().map(|c| (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8_388_608.0).collect(),
                        // A garbled float (NaN, infinite) is silence, not a NaN through the mix.
                        (3, 32) => body.as_chunks::<4>().0.iter().map(|c| Some(f32::from_le_bytes(*c)).filter(|x| x.is_finite()).unwrap_or(0.0)).collect(),
                        _ => return Err(format!("format tag {tag} with {bits} bits is not decoded")),
                    };
                    format_ok(channels, rate)?;
                    return Ok(PcmSound { samples: samples.into(), channels, rate, pan: None, reverb: None, shot: None });
                }
                _ => {}
            }
            at += 8 + len + (len & 1);
        }
        Err("no data chunk".into())
    }
}

/// The gain of a sound `distance` units from the listener: its attenuation curve between
/// the minimum and the maximum distance (see the module notes).
pub fn distance_gain(s: &Sound, distance: f32) -> f32 {
    // A sound at a broken place (NaN, infinite) is not heard.
    if !distance.is_finite() {
        return 0.0;
    }
    let curve = s.attenuation_curve.unwrap_or(DEFAULT_ATTENUATION_CURVE).map(|p| (p / 100.0).clamp(0.0, 1.0));
    let (min, max) = (s.min_distance.max(1.0), s.max_distance);
    if distance <= min {
        return curve[0];
    }
    if max <= min {
        // No span to lay the curve on: the inner level, falling as min ÷ distance [design].
        return curve[0] * min / distance;
    }
    if distance >= max {
        return 0.0;
    }
    curve_gain(&curve, (distance - min) / (max - min))
}

/// A five-point curve at `t` (0 at the inner radius, 1 at the outer): the points stand
/// evenly over the span, straight lines between them [hypothesis: the GECK's five
/// sliders say nothing of where the middle three stand].
pub fn curve_gain(curve: &[f32; 5], t: f32) -> f32 {
    let x = t.clamp(0.0, 1.0) * 4.0;
    let i = (x.floor() as usize).min(3);
    let f = x - i as f32;
    curve[i] + (curve[i + 1] - curve[i]) * f
}

/// The gain the record's static attenuation leaves.
pub fn static_gain(s: &Sound) -> f32 {
    10f32.powf(-s.static_attenuation_db / 20.0)
}

#[derive(Resource)]
pub struct Sfx {
    pub forms: FxForms,
    /// Sound files by archive path ([`FileState`]), and the paths still being decoded.
    files: HashMap<String, FileState>,
    decoding: Vec<String>,
    /// The `.wav` and `.ogg` files of a folder. Once the folder index is in (`indexed`;
    /// `index` while a worker lists it), a folder not in it has none; before, a folder is
    /// listed on its first use.
    folders: HashMap<String, Vec<String>>,
    index: Option<Task<HashMap<String, Vec<String>>>>,
    indexed: bool,
    /// Requests waiting for their file, oldest first.
    waiting: Vec<Waiting>,
    /// [`play`]'s runs: the frame a request is asked in.
    frame: u64,
    /// How long requests waited for their files (the exit report).
    pub waits: Waits,
    /// One-shots played at a place since [`crate::sound_cover`] last took them: their gain
    /// at the listener (no effects volume), distance and seconds.
    pub cover_notes: Vec<(f32, f32, f32)>,
    /// The age limit in force ([`MAX_ONE_SHOT_WAIT`]; the tests' real decodes on a busy
    /// machine would otherwise run into it).
    one_shot_wait: Duration,
    reported: HashSet<String>,
    pub(crate) rng: SplitMix,
    pub played: u32,
    pub silent: u32,
    pub failed: u32,
    /// [`LoopScale`] as [`play`] last read it: a [`LoopSound`] loop starts at its share, so
    /// one faded in from silence (`crate::heartbeat`) does not sound whole for its first frame.
    loop_start: HashMap<FormId, f32>,
    /// The listener's reverb environment now ([`crate::reverb::RoomReverb`] keeps it),
    /// and how many sounds were played through one.
    pub room_reverb: Option<&'static Preset>,
    pub reverbed: u32,
    /// Sounds kept out of the room's reverb by their Reverb Attenuation Control (100),
    /// and the sum of the sends of those played through one (`crate::reverb::send_of`).
    pub reverb_kept_out: u32,
    pub reverb_send_sum: f32,
    /// Placed sounds played by a curve of their own (not the default 100, 50, 20, 5, 0).
    pub own_curve: u32,
    /// The listener is under water ([`listener_submerged`]), how many sounds that
    /// silenced (Mute When Submerged, audio.md §15.2) and the deepest the listener went
    /// under a surface, units.
    pub submerged: bool,
    pub muted_submerged: u32,
    pub deepest: f32,
    /// Sounds with the 2D flag played at a place, not panned.
    pub flat_2d: u32,
    /// The voice slots ([`Slot`]): how many are taken now (counted each frame by
    /// [`prepare`] and again by [`late_start`], and as one-shots start), those of each
    /// SOUN, the replacements waiting for a slot of theirs to empty ([`Replacement`]),
    /// the most slots taken at once, and how many one-shots were not started because
    /// [`MAX_VOICES`] or [`MAX_SAME`] were playing.
    voices: usize,
    same: HashMap<FormId, Vec<Slot>>,
    pending: Vec<Replacement>,
    /// The start number of the next one-shot ([`OneShot`]), so the oldest is known.
    next_voice: u64,
    pub most_voices: usize,
    /// The most one-shots of one SOUN at once.
    pub most_same: usize,
    pub voice_limited: u32,
    /// One-shots that took over the oldest voice of their SOUN at [`MAX_SAME`] (started
    /// in its slot): after a wait (a [`Replacement`]) or at once (`taken_at_once`).
    pub voice_stolen: u32,
    /// The replacements (#2217): the requests that waited for a slot's sound to fade out
    /// (`waited`), and what became of them: started in the slot, dropped by a StopSound
    /// (`cancelled`), dropped past [`MAX_ONE_SHOT_WAIT`] (`expired`), or decided silent
    /// when their turn came (too quiet, under water, sounds off: `silenced`); the rest
    /// still wait ([`Sfx::pending`]). `waited` is the sum of the others and those waiting.
    /// `taken_at_once`: one-shots started at once over one of their SOUN started in the
    /// same frame, not heard yet (no wait, no replacement).
    pub replacements_waited: u32,
    pub replacements_started: u32,
    pub replacements_cancelled: u32,
    pub replacements_expired: u32,
    pub replacements_silenced: u32,
    pub taken_at_once: u32,
    /// Per SOUN: one-shots started, those started over its oldest voice, refused.
    per_sound: HashMap<FormId, [u32; 3]>,
    /// Decoded files nothing plays, and when each was first seen so ([`release_files`]).
    unused: nvf_assets::release::Unused<String>,
    /// Real seconds of the last [`release_files`] sweep, and the files it has let go of.
    swept_at: f64,
    pub files_released: u64,
    /// The files of the sounds made for files ([`ScriptSounds::paths`]), by their id.
    path_files: HashMap<FormId, String>,
    /// Script sounds (request ids) to fade in once they start ([`ScriptSounds::fades_in`]).
    fades_in: HashMap<u32, f32>,
}

impl Sfx {
    /// A SOUN record by Editor ID (any case). The form store (`fnv_sim::forms`) does not
    /// index SOUN records, so its `by_edid` finds no sound (#3095).
    pub fn sound_by_edid(&self, edid: &str) -> Option<FormId> {
        self.forms.sounds.iter().find(|(_, s)| s.edid.as_deref().is_some_and(|e| e.eq_ignore_ascii_case(edid))).map(|(f, _)| *f)
    }

    fn new(forms: FxForms) -> Sfx {
        Sfx {
            forms,
            loop_start: HashMap::new(),
            files: HashMap::new(),
            decoding: Vec::new(),
            folders: HashMap::new(),
            index: None,
            indexed: false,
            waiting: Vec::new(),
            frame: 0,
            waits: Waits::default(),
            cover_notes: Vec::new(),
            one_shot_wait: MAX_ONE_SHOT_WAIT,
            reported: HashSet::new(),
            rng: SplitMix(0x5f37_59df),
            played: 0,
            silent: 0,
            failed: 0,
            room_reverb: None,
            reverbed: 0,
            reverb_kept_out: 0,
            reverb_send_sum: 0.0,
            own_curve: 0,
            submerged: false,
            muted_submerged: 0,
            deepest: 0.0,
            flat_2d: 0,
            voices: 0,
            same: HashMap::new(),
            pending: Vec::new(),
            next_voice: 0,
            most_voices: 0,
            most_same: 0,
            voice_limited: 0,
            voice_stolen: 0,
            replacements_waited: 0,
            replacements_started: 0,
            replacements_cancelled: 0,
            replacements_expired: 0,
            replacements_silenced: 0,
            taken_at_once: 0,
            per_sound: HashMap::new(),
            unused: nvf_assets::release::Unused::default(),
            swept_at: 0.0,
            files_released: 0,
            path_files: HashMap::new(),
            fades_in: HashMap::new(),
        }
    }

    /// A sound made for a file: a SOUN of its own from now on, its file that path, no
    /// flags but Loop for one that goes round, the GECK's default curve and a reach of
    /// 100 to 2,000 units [engineering choice: the pages name no reach].
    fn add_path_sound(&mut self, id: FormId, path: &str, looping: bool) {
        let name = format!("file {path}");
        let flags = if looping { vec!["Loop".to_string()] } else { Vec::new() };
        let s = Sound { edid: Some(name), flags, min_distance: 100.0, max_distance: 2000.0, ..Default::default() };
        self.forms.sounds.insert(id, s);
        self.path_files.insert(id, path.to_string());
    }
}

/// A sound file in [`Sfx`]'s cache.
enum FileState {
    /// Being read and decoded on a worker thread ([`decode_file`]); the SOUN it was first
    /// asked for, named in the warning if it fails.
    Decoding(Task<Result<PcmSound, String>>, String),
    Ready(Handle<PcmSound>),
    /// Not there or not decoded: warned of once, never played.
    Broken,
}

/// A file as a request finds it.
enum FileNow {
    Ready(Handle<PcmSound>),
    Pending,
    Broken,
}

/// What a request waiting for its file does once the file is ready.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Wanted {
    /// [`LoopSound`] on: goes round at the listener.
    Loop,
    /// [`PlaySound`]: once, at a place or at the listener.
    Once(Option<[f32; 3]>),
    /// [`ScaledSound`]: once, its loudness times the gain.
    Scaled(Option<[f32; 3]>, f32),
    /// A sound a script played: its id (for [`ScriptSounds::heard`]), its place, and
    /// whether it goes round until StopSound (the Loop flag).
    Script { id: u32, at: Option<[f32; 3]>, looping: bool },
}

/// A request waiting for its file: the SOUN, the file picked for it (a folder's random
/// pick is made once; `None` while the folder index is still being listed, the pick is
/// made when it is in), the frame it was asked in and when it joined the list (the exit
/// report's milliseconds of the late starts), what it plays.
#[derive(Clone, Debug, PartialEq)]
struct Waiting {
    sound: FormId,
    path: Option<String>,
    frame: u64,
    asked: Instant,
    wanted: Wanted,
}

/// The longest a game event's one-shot ([`Wanted::Once`]: a footstep, a shot, an impact)
/// waits for its file: one decided later than this after it joined the waiting list is
/// dropped, not started, so it is not heard long after its moment. Loops and scripts'
/// sounds are started however late, so what a script hears of its sound does not change
/// [engineering choice: about nine frames at 60 Hz; the original's behaviour with a sound
/// not yet in memory is not known, docs/spec-notes/audio.md §7].
pub const MAX_ONE_SHOT_WAIT: Duration = Duration::from_millis(150);

/// Whether a request is a one-shot that [`MAX_ONE_SHOT_WAIT`] holds to.
fn wait_limited(wanted: Wanted) -> bool {
    matches!(wanted, Wanted::Once(_) | Wanted::Scaled(..))
}

/// Drop the waiting requests `drop` picks; how many were dropped.
fn cancel(waiting: &mut Vec<Waiting>, drop: impl Fn(&Waiting) -> bool) -> u32 {
    let before = waiting.len();
    waiting.retain(|w| !drop(w));
    (before - waiting.len()) as u32
}

/// The waiting requests no longer `pending` (their file is ready or cannot be played,
/// or the folder index they wait for is in), oldest first; the others go on waiting.
fn take_decided(waiting: &mut Vec<Waiting>, pending: impl Fn(&Waiting) -> bool) -> Vec<Waiting> {
    let (wait, go): (Vec<Waiting>, Vec<Waiting>) = std::mem::take(waiting).into_iter().partition(|w| pending(w));
    *waiting = wait;
    go
}

/// How long requests waited for their file, for the exit report.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Waits {
    /// Sounds that began playing in the frame they were asked in, those that began
    /// later, those more than one frame later, and the most frames one waited: from the
    /// frame it was asked in to the frame it began, whatever it waited for (its file, a
    /// voice slot's sound fading out, or both). Counted when a sound begins, or is decided
    /// silent: a one-shot that waited for a voice slot ([`Replacement`]) is counted when it
    /// starts (or is silenced) in the slot; one dropped while it waited for the slot is not
    /// counted here (that is `sfx voices: …`'s).
    pub at_once: u32,
    pub late: u32,
    pub over_one: u32,
    pub most_frames: u64,
    /// How soon the file was ready, apart from when the sound began: the requests that
    /// found their file ready in the frame they were asked in, those that found it later,
    /// and the most frames one waited for it; counted as the file is ready, whatever then
    /// became of the request (begun, waiting for a slot, silent).
    pub files_at_once: u32,
    pub files_late: u32,
    pub files_most_frames: u64,
    /// Requests decided silent (too quiet, under water, sounds off, the voice limit),
    /// at once or after waiting.
    pub silent: u32,
    /// Requests dropped while they waited (LoopSound off or on again, StopSound), and
    /// those whose file turned out not to play.
    pub cancelled: u32,
    pub broken: u32,
    /// One-shots dropped when their file came in after more than [`MAX_ONE_SHOT_WAIT`].
    pub too_late: u32,
    /// Play At Random rolls skipped because the file was still being read.
    pub rolls_skipped: u32,
    /// Requests started by the second chance late in the frame ([`late_start`]), those
    /// of them that began in the frame they were asked for, and, of those it started
    /// only, the longest time from joining the waiting list to the start being decided,
    /// milliseconds (a frame is no fixed time). Not the latency from the sender's
    /// request to the sound being heard: the time before [`play`] took the request, the
    /// output's attachment and the mixer's buffers are not in it, nor the requests
    /// started by a later frame's [`play`].
    pub late_started: u32,
    pub late_same_frame: u32,
    pub late_most_ms: f32,
}

impl Waits {
    /// A request decided `waited` frames after it was asked for: it began playing
    /// (`heard`), or it is silent.
    fn decided(&mut self, waited: u64, heard: bool) {
        if !heard {
            self.silent += 1;
            return;
        }
        if waited == 0 {
            self.at_once += 1;
        } else {
            self.late += 1;
            self.over_one += (waited > 1) as u32;
        }
        self.most_frames = self.most_frames.max(waited);
    }

    /// A request whose file is ready, `waited` frames after it was asked for.
    fn file_ready(&mut self, waited: u64) {
        if waited == 0 {
            self.files_at_once += 1;
        } else {
            self.files_late += 1;
        }
        self.files_most_frames = self.files_most_frames.max(waited);
    }

    /// A request decided as [`Begun`] says: begun or silent; one waiting for a voice slot
    /// is not decided yet ([`start_replacements`] counts it).
    fn decided_unless_waiting(&mut self, waited: u64, begun: Begun) {
        match begun {
            Begun::Started(_) => self.decided(waited, true),
            Begun::Refused => self.decided(waited, false),
            Begun::Waiting => {}
        }
    }

    /// A request started by [`late_start`], `waited` frames after it was asked for and
    /// `ms` milliseconds after it joined the waiting list.
    fn started_late(&mut self, waited: u64, ms: f32) {
        self.late_started += 1;
        self.late_same_frame += (waited == 0) as u32;
        self.late_most_ms = self.late_most_ms.max(ms);
    }
}

#[derive(Component)]
struct SfxSound;

/// A one-shot sound effect playing, of this SOUN, started as the n-th ([`MAX_VOICES`]),
/// in this frame ([`Sfx::frame`]).
#[derive(Component)]
struct OneShot(FormId, u64, u64);

/// One-shot sound effects playing at once, at most, those fading out after a stop counted
/// (module notes); one asked for beyond it is not played (unless its own SOUN is at
/// [`MAX_SAME`], whose slot it then takes over)
/// [engineering choice: the game's own limit is not known. Two dozen mantis nymphs walking
/// started about 130 footsteps a second; each mixed through its own reverb, the mixer fell
/// behind real time, sounds stopped ending and 1,800 piled up (docs/design/
/// frame-rate-audit.md §2)]. Interface sounds are not limited. Requests waiting for their
/// file are not counted until they start.
pub const MAX_VOICES: usize = 64;
/// The same SOUN playing at once, at most (a crowd's footsteps, a burst of shots)
/// [engineering choice]. One more of it fades the oldest playing voice of that SOUN out
/// and starts in its slot once that has ended (#2217: before, it was cut and the new one
/// started at once, a click in every burst; module notes), instead of not being played:
/// an automatic weapon fires 9 to 20
/// shots a second, each shot's sound with its tail longer than 8 shots apart, so
/// refusing the newest left gaps in a burst (#1098, owner 2026-10-30) [engineering
/// choice: the usual rule of a voice limit; whether the game cuts the previous shot's
/// sound is not known]. The voices stay within [`MAX_VOICES`] and this either way, which
/// is what kept the mantis crowd's mixer real-time (frame-rate-audit.md §2): with the
/// stealing, mantis-egg-spawn mixed 66.1 s in 66.1 s with 64 one-shots at most.
pub const MAX_SAME: usize = 8;

/// Whether one more one-shot of `sound` may start with `voices` playing, `same` of them
/// of that sound ([`MAX_VOICES`], [`MAX_SAME`]).
pub fn voice_free(voices: usize, same: usize) -> bool {
    voices < MAX_VOICES && same < MAX_SAME
}

fn init(mut commands: Commands, d: Res<PlayWorld>, data: Option<Res<nvf_assets::GameData>>) {
    let t = std::time::Instant::now();
    let forms = FxForms::build(d.lo, d.schema);
    info!(
        "sfx: {} sounds, {} impacts in {} sets, {} add-on nodes read in {:.0} ms",
        forms.sounds.len(),
        forms.impacts.len(),
        forms.impact_sets.len(),
        forms.addons.len(),
        t.elapsed().as_secs_f64() * 1000.0
    );
    let mut sfx = Sfx::new(forms);
    if let Some(data) = data {
        // The folders the records name, listed on a worker (a folder's first use would
        // otherwise go through every path of the archives in the frame).
        let wanted: HashSet<String> = sfx.forms.sounds.values().filter_map(|s| s.file.as_deref()).filter_map(|f| folder_of(&sound_path(f))).collect();
        let vfs = data.vfs.clone();
        sfx.index = Some(IoTaskPool::get_or_init(TaskPool::default).spawn(async move {
            let _span = info_span!("sfx::folders").entered();
            folder_index(vfs.paths(|p| is_sound_file(p) && p.rfind('\\').is_some_and(|i| wanted.contains(&p[..=i]))))
        }));
    }
    commands.insert_resource(sfx);
}

/// The archive path a SOUN's `FNAM` names, under `sound\`.
fn sound_path(file: &str) -> String {
    fnv_vfs::normalize(&format!("sound\\{}", file.trim_start_matches(['\\', '/'])))
}

/// A `.wav` or an `.ogg` file.
fn is_sound_file(path: &str) -> bool {
    path.ends_with(".wav") || path.ends_with(".ogg")
}

/// The folder (ending in `\`) a SOUN's path stands for; `None` for a file.
fn folder_of(path: &str) -> Option<String> {
    if is_sound_file(path) {
        return None;
    }
    Some(if path.ends_with('\\') { path.to_string() } else { format!("{path}\\") })
}

/// The `.wav` and `.ogg` files directly in a folder, sorted.
fn list_folder(vfs: &fnv_vfs::Vfs, folder: &str) -> Vec<String> {
    vfs.paths(|p| p.starts_with(folder) && is_sound_file(p) && !p[folder.len()..].contains('\\'))
}

/// `paths` (sound files) by the folder each is directly in, each folder's sorted: what
/// [`list_folder`] gives for that folder.
fn folder_index(paths: Vec<String>) -> HashMap<String, Vec<String>> {
    let mut by: HashMap<String, Vec<String>> = HashMap::new();
    for p in paths {
        if let Some(i) = p.rfind('\\') {
            by.entry(p[..=i].to_string()).or_default().push(p);
        }
    }
    for v in by.values_mut() {
        v.sort();
    }
    by
}

/// Whether `sound` names a folder whose files are not known yet: the folder index is
/// still being listed on a worker. A request for it waits for the index (the main
/// thread never scans the archives' paths for it); the ambient skips it this frame.
pub(crate) fn folder_unlisted(sfx: &Sfx, sound: FormId) -> bool {
    sfx.index.is_some()
        && sfx.forms.sounds.get(&sound).and_then(|s| s.file.as_deref()).and_then(|f| folder_of(&sound_path(f))).is_some_and(|d| !sfx.folders.contains_key(&d))
}

/// The archive path of the file a SOUN plays this time: the file it names, or the Ogg
/// Vorbis file of the same name when the archives hold that instead; of a folder, one
/// of its `.wav` and `.ogg` files.
fn pick_file(sfx: &mut Sfx, vfs: &fnv_vfs::Vfs, file: &str) -> Option<String> {
    let path = sound_path(file);
    if let Some(stem) = path.strip_suffix(".wav") {
        let ogg = format!("{stem}.ogg");
        return Some(if !vfs.exists(&path) && vfs.exists(&ogg) { ogg } else { path });
    }
    if path.ends_with(".ogg") {
        return Some(path);
    }
    let folder = folder_of(&path)?;
    if !sfx.folders.contains_key(&folder) {
        // Without a folder index (no game data at the start) the folder is listed here;
        // while one is being listed, requests wait for it ([`folder_unlisted`]).
        let files = if sfx.indexed { Vec::new() } else { list_folder(vfs, &folder) };
        sfx.folders.insert(folder.clone(), files);
    }
    let files = &sfx.folders[&folder];
    if files.is_empty() {
        return None;
    }
    let k = (sfx.rng.next_f32() * files.len() as f32) as usize;
    Some(files[k.min(files.len() - 1)].clone())
}

/// A SOUN for one playing of it: its record, name and file.
#[derive(Clone)]
pub(crate) struct Resolved {
    pub s: Sound,
    pub name: String,
    pub path: String,
}

/// The record, name and file of a SOUN for this playing of it; a sound without a file
/// is counted and reported once.
pub(crate) fn resolve(sfx: &mut Sfx, vfs: &fnv_vfs::Vfs, sound: FormId) -> Option<Resolved> {
    let s = sfx.forms.sounds.get(&sound).cloned()?;
    let name = s.edid.clone().unwrap_or_else(|| sound.to_string());
    if let Some(path) = sfx.path_files.get(&sound) {
        return Some(Resolved { s, name, path: path.clone() });
    }
    let Some(path) = s.file.as_deref().and_then(|f| pick_file(sfx, vfs, f)) else {
        sfx.failed += 1;
        if sfx.reported.insert(name.clone()) {
            warn!("sfx: {name}: no file for {:?}", s.file);
        }
        return None;
    };
    Some(Resolved { s, name, path })
}

/// Read and decode the sound file at `path` whole (on a worker thread). A panic decoding
/// a broken file is that file's failure, not the game's (nvf_assets::guard).
fn decode_file(vfs: &fnv_vfs::Vfs, path: &str) -> Result<PcmSound, String> {
    nvf_assets::guard::guarded(|| {
        let read = {
            let _span = info_span!("sfx::read").entered();
            vfs.read(path)
        };
        let bytes = match read {
            Some(Ok(bytes)) => nvf_assets::data::test_broken(path, bytes).ok_or_else(|| format!("{path}: not in the archives"))?,
            Some(Err(e)) => return Err(format!("{path}: {e}")),
            None => return Err(format!("{path}: not in the archives")),
        };
        let _span = info_span!("sfx::decode").entered();
        if path.ends_with(".wav") {
            PcmSound::from_wav(&bytes).map_err(|e| format!("{path}: {e}"))
        } else {
            PcmSound::from_encoded(bytes).map_err(|e| format!("{path}: {e}"))
        }
    })
}

/// Start reading and decoding the file at `path` on a worker thread, unless it is known
/// already (being decoded, ready, or not playable). `name`: the SOUN asking for it, named
/// if it fails. A read-ahead can call this for files about to be asked for.
pub(crate) fn prefetch(sfx: &mut Sfx, vfs: &Arc<fnv_vfs::Vfs>, path: &str, name: &str) {
    if sfx.files.contains_key(path) {
        return;
    }
    let (vfs, owned) = (vfs.clone(), path.to_string());
    // On the I/O pool, away from the reads of cells, models and actors, which were on the
    // async compute pool then and have their own pool now (`nvf_assets::read_pool`)
    // [engineering choice; hypothesis: a sound asked for while a cell streams in waits
    // behind those reads, the smoke runs of 2026-11-03 had starts up to about 100 frames
    // late on the compute pool]. Bevy's `AssetServer` loads on the I/O pool
    // too, but this engine loads only its embedded shaders through it, at start-up (the
    // `RenderStartup` systems, e.g. engine/nvforever/src/post.rs `init_pipelines`, and
    // the materials' shaders when their plugins are built, grass.rs, tree.rs, sky.rs);
    // the game's files are read by nvf-assets on its read pool and added to
    // `Assets` directly (nvf-assets/src/model.rs, actor.rs) [design]. During play the
    // I/O pool holds this module's decodes and its folder index only.
    let task = IoTaskPool::get_or_init(TaskPool::default).spawn(async move { decode_file(&vfs, &owned) });
    sfx.files.insert(path.to_string(), FileState::Decoding(task, name.to_string()));
    sfx.decoding.push(path.to_string());
}

/// The file at `path` as it is now; its decode is started if nobody asked for it before.
fn file_now(sfx: &mut Sfx, vfs: &Arc<fnv_vfs::Vfs>, path: &str, name: &str) -> FileNow {
    prefetch(sfx, vfs, path, name);
    match sfx.files.get(path) {
        Some(FileState::Ready(h)) => FileNow::Ready(h.clone()),
        Some(FileState::Decoding(..)) => FileNow::Pending,
        Some(FileState::Broken) | None => FileNow::Broken,
    }
}

/// Whether a decoded file is in use: a playing holds its handle or shares its samples
/// (a placed or reverbed playing is an asset of its own over the same samples).
fn file_in_use(h: &Handle<PcmSound>, assets: &Assets<PcmSound>) -> bool {
    nvf_assets::release::held_beyond(h, 1) || assets.get(h).is_some_and(|p| Arc::strong_count(&p.samples) > 1)
}

impl Sfx {
    /// The decoded files kept: (files, those nothing plays, their samples' bytes).
    pub fn file_share(&self, assets: &Assets<PcmSound>) -> (usize, usize, u64) {
        let (mut n, mut idle, mut bytes) = (0, 0, 0u64);
        for state in self.files.values() {
            if let FileState::Ready(h) = state {
                n += 1;
                idle += usize::from(!file_in_use(h, assets));
                bytes += assets.get(h).map_or(0, |p| (p.samples.len() * size_of::<f32>()) as u64);
            }
        }
        (n, idle, bytes)
    }
}

/// Let go of decoded sound files nothing has played for a while, as the asset caches do
/// with models and textures (nvf_assets `release.rs`, the same [`KeepLimits`]). Without
/// it every file heard stayed decoded for the whole session: decoded samples are 32-bit,
/// about 170 KB a second of mono 44.1 kHz sound, and the RAM a long session used only
/// grew [observed 2026-11-06, the stress tour; design]. A file asked for again is decoded
/// again; a playing keeps its own samples, so dropping the cache's entry never cuts one.
///
/// [`KeepLimits`]: nvf_assets::KeepLimits
fn release_files(real: Res<Time<Real>>, limits: Option<Res<nvf_assets::KeepLimits>>, sfx: Option<ResMut<Sfx>>, assets: Res<Assets<PcmSound>>) {
    let Some(mut sfx) = sfx else { return };
    let now = real.elapsed_secs_f64();
    if now - sfx.swept_at < nvf_assets::release::SWEEP_SECS as f64 {
        return;
    }
    sfx.swept_at = now;
    let limits = limits.map_or_else(nvf_assets::KeepLimits::default, |l| *l);
    let Sfx { files, unused, .. } = &mut *sfx;
    let entries: Vec<(String, bool)> = files
        .iter()
        .filter_map(|(path, state)| match state {
            FileState::Ready(h) => Some((path.clone(), file_in_use(h, &assets))),
            _ => None,
        })
        .collect();
    let drop = unused.sweep(now, entries, limits.secs, limits.max);
    for path in &drop {
        files.remove(path);
    }
    sfx.files_released += drop.len() as u64;
}

/// Take in the decodes the worker threads have finished: a sound ready to play, or a
/// file that cannot be played (warned of once); and the folder index once it is listed.
fn collect_files(sfx: &mut Sfx, assets: &mut Assets<PcmSound>) {
    if let Some(index) = sfx.index.as_mut().and_then(check_ready) {
        sfx.index = None;
        sfx.indexed = true;
        for (folder, files) in index {
            sfx.folders.entry(folder).or_insert(files);
        }
    }
    if sfx.decoding.is_empty() {
        return;
    }
    let Sfx { files, decoding, .. } = sfx;
    decoding.retain(|path| {
        let Some(state) = files.get_mut(path) else { return false };
        let FileState::Decoding(task, name) = state else { return false };
        let Some(result) = check_ready(task) else { return true };
        *state = match result {
            Ok(pcm) => FileState::Ready(assets.add(pcm)),
            Err(e) => {
                warn!("sfx: {name}: {e}");
                FileState::Broken
            }
        };
        false
    });
}

/// What starting a sound needs of the frame: the commands, the decoded sounds, the
/// settings, the listener.
pub(crate) struct Out<'a, 'w, 's> {
    pub commands: &'a mut Commands<'w, 's>,
    pub assets: &'a mut Assets<PcmSound>,
    pub settings: &'a SfxSettings,
    pub player: Option<&'a Player>,
}

/// A request: started or played silent at once when its file is ready, a script
/// answered at once when there is no file to play, otherwise left waiting for its file
/// (or, its file ready, for a voice slot: [`Replacement`]). The entity playing it, if it
/// started.
fn request(out: &mut Out, sfx: &mut Sfx, vfs: &Arc<fnv_vfs::Vfs>, heard: &mut Vec<(u32, Option<f32>)>, sound: FormId, wanted: Wanted) -> Option<Entity> {
    let asked = Instant::now();
    if folder_unlisted(sfx, sound) {
        sfx.waiting.push(Waiting { sound, path: None, frame: sfx.frame, asked, wanted });
        return None;
    }
    let Some(r) = resolve(sfx, vfs, sound) else {
        // No file to play: over at once.
        if let Wanted::Script { id, .. } = wanted {
            heard.push((id, Some(0.0)));
        }
        return None;
    };
    match file_now(sfx, vfs, &r.path, &r.name) {
        FileNow::Pending => {
            sfx.waiting.push(Waiting { sound, path: Some(r.path), frame: sfx.frame, asked, wanted });
            None
        }
        FileNow::Ready(h) => begin(out, sfx, heard, &r, Some(h), sound, wanted, None, asked).entity(),
        FileNow::Broken => begin(out, sfx, heard, &r, None, sound, wanted, None, asked).entity(),
    }
}

/// What became of a request decided ([`begin`], [`one_shot_ready`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Begun {
    /// It plays, this entity.
    Started(Entity),
    /// It waits for a voice slot whose sound is fading out ([`Replacement`]); a script
    /// is answered when it starts.
    Waiting,
    /// Nothing plays: no file, too quiet, under water, sounds off or the voice limit.
    Refused,
}

impl Begun {
    fn entity(self) -> Option<Entity> {
        match self {
            Begun::Started(e) => Some(e),
            _ => None,
        }
    }
}

/// A request decided, its file known: `handle`, or `None` for a file that cannot be
/// played. It starts, waits for a voice slot, or plays silent (muted under water, too
/// far, sounds off, the voice limit); a script is told its seconds once that is decided
/// (`None` going round; 0 without a file). `waited`: for a request that waited for its
/// file, the frames since it was asked for (0 when [`late_start`] decides it in its own
/// frame); `None` for one decided when asked. `asked`: when it joined the waiting list
/// (or was asked for), for the age a replacement may reach ([`MAX_ONE_SHOT_WAIT`]).
#[allow(clippy::too_many_arguments)]
fn begin(out: &mut Out, sfx: &mut Sfx, heard: &mut Vec<(u32, Option<f32>)>, r: &Resolved, handle: Option<Handle<PcmSound>>, sound: FormId, wanted: Wanted, waited: Option<u64>, asked: Instant) -> Begun {
    let Some(handle) = handle else {
        sfx.failed += 1;
        sfx.waits.broken += waited.is_some() as u32;
        // A script's sound whose file cannot be played is over at once (0 seconds), also
        // one with the Loop flag, so IsSoundPlaying reads 0 for it [engineering choice;
        // hypothesis: what the original answers for a broken looping sound is not known,
        // audio.md §11.5].
        if let Wanted::Script { id, .. } = wanted {
            heard.push((id, Some(0.0)));
        }
        return Begun::Refused;
    };
    if let Some(waited) = waited {
        debug!("sfx: {} {} decided {waited} frames after it was asked for (its file was being read)", r.name, r.path);
    }
    sfx.waits.file_ready(waited.unwrap_or(0));
    // The frame it was asked in, for a replacement's count of frames to its start.
    let asked_frame = sfx.frame.saturating_sub(waited.unwrap_or(0));
    let begun = match wanted {
        Wanted::Loop => start_looping(out, sfx, r, handle, sound, false).map_or(Begun::Refused, Begun::Started),
        // A SOUN with the Loop flag goes round until StopSound stops it [observed:
        // sound1, EMTMedicalBeep01 (Loop) still playing 120 frames on; UIVATSLoop (no
        // Loop flag) over after one frame]; a placed one loops at the listener [design].
        Wanted::Script { id, looping: true, .. } => {
            heard.push((id, None));
            let begun = start_looping(out, sfx, r, handle, sound, true).map_or(Begun::Refused, Begun::Started);
            fade_in(out, sfx, wanted, begun);
            begun
        }
        Wanted::Script { looping: false, .. } | Wanted::Once(_) | Wanted::Scaled(..) => {
            let (seconds, begun) = one_shot_ready(out, sfx, r, handle, sound, wanted, asked, asked_frame, None);
            one_shot_decided(out, sfx, heard, sound, wanted, seconds, begun);
            begun
        }
    };
    // One waiting for a voice slot is counted when it starts or is silenced there.
    sfx.waits.decided_unless_waiting(waited.unwrap_or(0), begun);
    begun
}

/// A script's one-shot decided at last ([`begin`], or its [`Replacement`] started): the
/// script told its seconds and, playing, the sound marked as the script's (StopSound
/// stops it) and faded in if it asked; once, and not while it waits for a slot.
fn one_shot_decided(out: &mut Out, sfx: &mut Sfx, heard: &mut Vec<(u32, Option<f32>)>, sound: FormId, wanted: Wanted, seconds: f32, begun: Begun) {
    let Wanted::Script { id, .. } = wanted else { return };
    if begun == Begun::Waiting {
        return;
    }
    heard.push((id, Some(seconds)));
    if let Begun::Started(e) = begun {
        out.commands.entity(e).insert(ScriptPlayed(sound));
    }
    fade_in(out, sfx, wanted, begun);
}

/// A script's sound that asked to fade in ([`ScriptSounds::fades_in`]) does once it
/// plays.
fn fade_in(out: &mut Out, sfx: &mut Sfx, wanted: Wanted, begun: Begun) {
    if let (Wanted::Script { id, .. }, Begun::Started(e)) = (wanted, begun) {
        if let Some(seconds) = sfx.fades_in.remove(&id) {
            out.commands.entity(e).insert(ScriptFade::new(seconds, false));
        }
    }
}

/// Start a SOUN going round at the listener: for [`LoopSound`], or for a script
/// (StopSound stops it).
fn start_looping(out: &mut Out, sfx: &mut Sfx, r: &Resolved, handle: Handle<PcmSound>, sound: FormId, script: bool) -> Option<Entity> {
    let gain = static_gain(&r.s) * out.settings.volume.max(0.0);
    debug!("sfx: {} {} looping{}, gain {gain:.3}", r.name, r.path, if script { " for a script" } else { "" });
    if !out.settings.play {
        sfx.silent += 1;
        return None;
    }
    sfx.played += 1;
    // A loop of a module's starts at its share ([`LoopScale`]), its whole gain kept for
    // [`loop_scales`].
    let share = if script { 1.0 } else { sfx.loop_start.get(&sound).copied().unwrap_or(1.0).clamp(0.0, 1.0) };
    let mut e = out.commands.spawn((AudioPlayer::<PcmSound>(handle), PlaybackSettings::LOOP.with_volume(Volume::Linear(gain * share)), SfxSound, SoundCategory::of_effect(r.s.edid.as_deref())));
    if script {
        e.insert(ScriptPlayed(sound));
    } else {
        e.insert((Looping(sound), LoopFull(gain)));
        if is_interface(r.s.edid.as_deref()) {
            e.insert(KeepsSpeed);
        }
    }
    Some(e.id())
}

/// The one-shots playing, for the voice slots: each with whether it is fading out after
/// a stop and its sink (none until the output takes it up).
type Voices<'w, 's> = Query<'w, 's, (Entity, &'static OneShot, Has<Retiring>, Option<&'static AudioSink>)>;

/// Before anything plays this frame ([`SfxStep::Prepare`]): the decodes finished on the
/// workers taken in, the frame counted, and the voice slots counted from the one-shots
/// playing now ([`reconcile`]) for the voice limits of the ambient's random sounds and of
/// [`play`].
fn prepare(playing: Voices, sfx: Option<ResMut<Sfx>>, mut assets: ResMut<Assets<PcmSound>>) {
    let Some(mut sfx) = sfx else { return };
    let sfx = &mut *sfx;
    collect_files(sfx, &mut assets);
    sfx.frame += 1;
    reconcile(sfx, &playing);
}

/// The voice slots from the one-shots playing (see [`Slot`]): every one-shot takes one,
/// also one fading out after a stop, until its sink is empty; a slot whose sound has
/// ended stays taken while a [`Replacement`] waits for it. What this frame decided
/// ([`Sfx::same`] is kept up to date as one-shots start, steal and stop) is not lost: the
/// one-shots started in it are entities by now, stops are [`Retiring`] (or gone, never
/// attached), the reservations are in [`Sfx::pending`].
fn reconcile(sfx: &mut Sfx, playing: &Voices) {
    let Sfx { same, pending, voices, .. } = sfx;
    same.clear();
    *voices = 0;
    for (e, o, retiring, sink) in playing {
        let reserved = pending.iter().any(|p| p.slot == e);
        // Over: faded out, or come to its end (the output despawns it after this).
        let ended = sink.is_some_and(|s| s.empty());
        if ended && !reserved {
            continue;
        }
        same.entry(o.0).or_default().push(Slot { start: o.1, entity: e, frame: o.2, retiring, reserved, ended });
        *voices += 1;
    }
    for p in pending.iter() {
        let list = same.entry(p.sound).or_default();
        if !list.iter().any(|s| s.entity == p.slot) {
            // Its sound gone already: the slot is the replacement's still.
            list.push(Slot { start: p.start, entity: p.slot, frame: 0, retiring: true, reserved: true, ended: true });
            *voices += 1;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn play(
    mut commands: Commands,
    (mut requests, mut scaled): (MessageReader<PlaySound>, MessageReader<ScaledSound>),
    mut loops: MessageReader<LoopSound>,
    looping: Query<(Entity, &Looping)>,
    scripted: Query<(Entity, &ScriptPlayed)>,
    mut script_sounds: ResMut<ScriptSounds>,
    sfx: Option<ResMut<Sfx>>,
    settings: Res<SfxSettings>,
    player: Option<Res<Player>>,
    data: Option<Res<nvf_assets::GameData>>,
    mut assets: ResMut<Assets<PcmSound>>,
    loop_scale: Option<Res<LoopScale>>,
) {
    let (Some(mut sfx), Some(data)) = (sfx, data) else {
        requests.clear();
        scaled.clear();
        loops.clear();
        return;
    };
    let sfx = &mut *sfx;
    if let Some(l) = loop_scale.filter(|l| l.is_changed()) {
        sfx.loop_start.clone_from(&l.0);
    }
    let vfs = &data.vfs;
    let ss = &mut *script_sounds;
    let mut out = Out { commands: &mut commands, assets: &mut assets, settings: &settings, player: player.as_deref() };
    // LoopSound: any word on a SOUN stops it going round, also one started earlier in
    // this frame, and drops a start of it still waiting for its file; on starts it anew.
    // So on then off in one frame leaves nothing playing [engineering choice; hypothesis:
    // what the original does with both in one frame is not known, audio.md §11.5].
    let mut new_loops: Vec<(FormId, Entity)> = Vec::new();
    for l in loops.read() {
        for (e, p) in &looping {
            if p.0 == l.sound {
                // No longer the loop of that SOUN at once: it fades out and goes.
                out.commands.entity(e).try_remove::<Looping>();
                stop_sound(out.commands, e, StopReason::Loop);
            }
        }
        // One started earlier in this frame is not attached to the output yet: removed
        // with no sound made (`stop_sound` counts no stop for it).
        new_loops.retain(|&(s, e)| {
            if s == l.sound {
                stop_sound(out.commands, e, StopReason::Loop);
            }
            s != l.sound
        });
        sfx.waits.cancelled += cancel(&mut sfx.waiting, |w| w.sound == l.sound && w.wanted == Wanted::Loop);
        if l.on {
            if let Some(e) = request(&mut out, sfx, vfs, &mut ss.heard, l.sound, Wanted::Loop) {
                new_loops.push((l.sound, e));
            }
        }
    }
    // StopSound: what a script played of that SOUN stops, its requests still waiting
    // for their file are dropped (all asked for in earlier frames), and so are this
    // frame's asked for before the stop (`PlaySound X; StopSound X`); one asked for after
    // it (`StopSound X; PlaySound X`, a higher id) plays [engineering choice; hypothesis:
    // the original's order in one frame is not known, audio.md §11.5, observer check 82].
    for (id, path, looping) in std::mem::take(&mut ss.paths) {
        sfx.add_path_sound(id, &path, looping);
    }
    sfx.fades_in.extend(std::mem::take(&mut ss.fades_in));
    let stops = std::mem::take(&mut ss.stops);
    let fades_out = std::mem::take(&mut ss.fades_out);
    for &(s, _) in &stops {
        let fade = fades_out.iter().find(|f| f.0 == s).map(|f| f.1);
        for (e, p) in &scripted {
            if p.0 == s {
                match fade {
                    Some(seconds) => {
                        out.commands.entity(e).remove::<ScriptPlayed>().insert(ScriptFade::new(seconds, true));
                    }
                    None => {
                        // The script's no longer (StopSound again finds nothing); it fades
                        // out over the declick and its slot is counted until it has.
                        out.commands.entity(e).try_remove::<(ScriptPlayed, ScriptFade)>();
                        sfx.retiring(s, e);
                        stop_sound(out.commands, e, StopReason::Script);
                    }
                }
            }
        }
        sfx.waits.cancelled += cancel(&mut sfx.waiting, |w| w.sound == s && matches!(w.wanted, Wanted::Script { .. }));
    }
    // A script's one-shot waiting for a voice slot is stopped as one waiting for its file.
    sfx.cancel_replacements(|p| matches!(p.wanted, Wanted::Script { id, .. } if stopped_before(&stops, p.sound, id)));
    // The replacements whose slot has emptied, then the requests whose file is now ready
    // (or will not play), oldest first, then this frame's.
    start_replacements(&mut out, sfx, &mut ss.heard);
    start_decided(&mut out, sfx, vfs, &mut ss.heard, false);
    for r in requests.read() {
        request(&mut out, sfx, vfs, &mut ss.heard, r.sound, Wanted::Once(r.at));
    }
    for r in scaled.read() {
        request(&mut out, sfx, vfs, &mut ss.heard, r.sound, Wanted::Scaled(r.at, r.gain.max(0.0)));
    }
    for (id, sound, at) in std::mem::take(&mut ss.plays) {
        if stopped_before(&stops, sound, id) {
            sfx.waits.cancelled += 1;
            continue;
        }
        let looping = sfx.forms.sounds.get(&sound).is_some_and(|s| s.has("Loop"));
        request(&mut out, sfx, vfs, &mut ss.heard, sound, Wanted::Script { id, at, looping });
    }
}

/// The decodes finished on the workers taken in, then the waiting requests whose file is
/// now ready (or will not play), or whose folder index is in, decided, oldest first:
/// started as a request is (the voice limits as counted this frame, the listener as it
/// is now), played silent or answered; a folder's file is picked once its index is in
/// (and may wait for its file then). Each is taken off the waiting list before it
/// starts, so it is decided once. The frames waited count from each request's frame.
/// For [`play`], and for [`late_start`] (`late`: its starts counted apart).
fn start_decided(out: &mut Out, sfx: &mut Sfx, vfs: &Arc<fnv_vfs::Vfs>, heard: &mut Vec<(u32, Option<f32>)>, late: bool) {
    collect_files(sfx, out.assets);
    if sfx.waiting.is_empty() {
        return;
    }
    let frame = sfx.frame;
    let decided = {
        let Sfx { waiting, files, index, .. } = &mut *sfx;
        let listing = index.is_some();
        take_decided(waiting, |w| match &w.path {
            None => listing,
            Some(p) => matches!(files.get(p), Some(FileState::Decoding(..))),
        })
    };
    for mut w in decided {
        let waited = frame - w.frame;
        if wait_limited(w.wanted) && w.asked.elapsed() > sfx.one_shot_wait {
            sfx.waits.too_late += 1;
            if let Wanted::Script { id, .. } = w.wanted {
                heard.push((id, Some(0.0)));
            }
            continue;
        }
        let started = match w.path.take() {
            None => {
                // The folder index is in: the folder's file is picked now.
                let Some(r) = resolve(sfx, vfs, w.sound) else {
                    sfx.waits.broken += 1;
                    if let Wanted::Script { id, .. } = w.wanted {
                        heard.push((id, Some(0.0)));
                    }
                    continue;
                };
                match file_now(sfx, vfs, &r.path, &r.name) {
                    FileNow::Pending => {
                        sfx.waiting.push(Waiting { path: Some(r.path), ..w });
                        continue;
                    }
                    FileNow::Ready(h) => begin(out, sfx, heard, &r, Some(h), w.sound, w.wanted, Some(waited), w.asked),
                    FileNow::Broken => begin(out, sfx, heard, &r, None, w.sound, w.wanted, Some(waited), w.asked),
                }
            }
            Some(path) => {
                let Some(s) = sfx.forms.sounds.get(&w.sound).cloned() else { continue };
                let handle = match sfx.files.get(&path) {
                    Some(FileState::Ready(h)) => Some(h.clone()),
                    _ => None,
                };
                let r = Resolved { name: s.edid.clone().unwrap_or_else(|| w.sound.to_string()), s, path };
                begin(out, sfx, heard, &r, handle, w.sound, w.wanted, Some(waited), w.asked)
            }
        };
        if late && matches!(started, Begun::Started(_)) {
            sfx.waits.started_late(waited, w.asked.elapsed().as_secs_f32() * 1000.0);
        }
    }
}

/// The second chance in the frame (see the module notes): in `PostUpdate`, before the
/// audio output takes up new players (`crate::audio_out::AudioOutSystems`, which then
/// finds these players, the commands applied before it), the requests whose file was
/// decoded since [`play`] start as [`play`] starts them ([`start_decided`]). It reads no
/// requests of its own: those are [`play`]'s. [`prepare`]'s frame number and voice counts
/// hold, so frames waited count from the request's frame.
fn add_late_start(app: &mut App) {
    // Ordinary `before`, not `before_ignore_deferred`: its players are spawned (the
    // commands applied) before the output attaches new players.
    app.add_systems(PostUpdate, late_start.before(crate::audio_out::AudioOutSystems));
}

/// See [`add_late_start`]. Before its starts the voice slots are counted again
/// ([`reconcile`]): a fade that has ended since [`prepare`] hands its slot to the
/// replacement waiting for it now, not a frame later.
#[allow(clippy::too_many_arguments)]
fn late_start(
    mut commands: Commands,
    mut script_sounds: ResMut<ScriptSounds>,
    sfx: Option<ResMut<Sfx>>,
    settings: Res<SfxSettings>,
    player: Option<Res<Player>>,
    data: Option<Res<nvf_assets::GameData>>,
    mut assets: ResMut<Assets<PcmSound>>,
    playing: Voices,
) {
    let (Some(mut sfx), Some(data)) = (sfx, data) else { return };
    if sfx.waiting.is_empty() && sfx.pending.is_empty() {
        return;
    }
    let sfx = &mut *sfx;
    let ss = &mut *script_sounds;
    // A StopSound asked for since [`play`] drops the script requests it stops before they
    // start; the stops stay for the next frame's [`play`], which stops what is playing.
    // A LoopSound since then waits for that `play`: telling its words apart from those
    // `play` has read would need a cursor shared with it [engineering choice].
    if !ss.stops.is_empty() {
        let stops = &ss.stops;
        sfx.waits.cancelled += cancel(&mut sfx.waiting, |w| matches!(w.wanted, Wanted::Script { id, .. } if stopped_before(stops, w.sound, id)));
        sfx.cancel_replacements(|p| matches!(p.wanted, Wanted::Script { id, .. } if stopped_before(stops, p.sound, id)));
    }
    reconcile(sfx, &playing);
    let mut out = Out { commands: &mut commands, assets: &mut assets, settings: &settings, player: player.as_deref() };
    start_replacements(&mut out, sfx, &mut ss.heard);
    start_decided(&mut out, sfx, &data.vfs, &mut ss.heard, true);
}

/// Whether a script's sound `id` of `sound` was asked for before one of `stops` of that
/// sound ([`ScriptSounds::stops`]), which then stops it before it starts.
fn stopped_before(stops: &[(FormId, u32)], sound: FormId, id: u32) -> bool {
    // Ids are handed out with `wrapping_add`: `id` is at or before `last` when it is less
    // than half the id space behind it.
    stops.iter().any(|&(s, last)| s == sound && last.wrapping_sub(id) < 1 << 31)
}

/// Whether the listener (the player's eye) is under water: the surface of the water it
/// is in or over (physics' water: cells' and placed volumes', none of an interior's
/// own, physics.md §6.6) lies above it [hypothesis: the game's listener is the camera].
/// A swimmer floating at the surface has its eye above it (feet `float_depth` 102.5
/// under); one below the floating height stays there, under water (physics.md §6.6).
pub fn listener_submerged(phys: Option<Res<crate::phys::PhysState>>, player: Option<Res<Player>>, sfx: Option<ResMut<Sfx>>) {
    let (Some(phys), Some(player), Some(mut sfx)) = (phys, player, sfx) else { return };
    let eye = player.eye();
    let depth = phys.world.water_surface_for(eye).map_or(0.0, |s| s - eye[2]);
    sfx.deepest = sfx.deepest.max(depth);
    let under = depth > 0.0;
    if sfx.submerged != under {
        sfx.submerged = under;
        debug!("sfx: the listener is {} water ({depth:.1} under the surface)", if under { "under" } else { "out of the" });
    }
}

/// Turn the sounds playing at a place with the listener.
pub fn pan_sounds(player: Option<Res<Player>>, sounds: Query<&SoundAt>) {
    let Some(p) = player else { return };
    let (eye, heading) = (p.eye(), p.heading);
    for s in &sounds {
        s.pan.set(pan_gains(eye, heading, s.at));
    }
}

/// A one-shot played now, as [`one_shot_heard`] found it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum OneShotNow {
    /// Its seconds as played, also when it is too quiet to be heard or sounds are off.
    Played(f32),
    /// Its file is still being read: nothing played (the file's decode is under way).
    Waiting,
    /// No file we can play.
    NoFile,
}

/// Play a SOUN once now for the player, at a place (panned for them) or at the listener,
/// if its file is ready; nothing is queued (see [`OneShotNow`]). `pick`: the file an
/// earlier try picked and found still being read, played this time instead of a new
/// random pick of a folder (so that decode is not wasted); set when this try finds its
/// file being read, emptied otherwise.
pub(crate) fn one_shot_heard(out: &mut Out, sfx: &mut Sfx, vfs: &Arc<fnv_vfs::Vfs>, sound: FormId, at: Option<[f32; 3]>, pick: &mut Option<String>) -> OneShotNow {
    if folder_unlisted(sfx, sound) {
        return OneShotNow::Waiting;
    }
    let r = match (pick.take(), sfx.forms.sounds.get(&sound)) {
        (Some(path), Some(s)) => Resolved { name: s.edid.clone().unwrap_or_else(|| sound.to_string()), s: s.clone(), path },
        _ => {
            let Some(r) = resolve(sfx, vfs, sound) else { return OneShotNow::NoFile };
            r
        }
    };
    match file_now(sfx, vfs, &r.path, &r.name) {
        FileNow::Pending => {
            *pick = Some(r.path);
            OneShotNow::Waiting
        }
        FileNow::Broken => {
            sfx.failed += 1;
            OneShotNow::NoFile
        }
        FileNow::Ready(h) => {
            sfx.waits.file_ready(0);
            let frame = sfx.frame;
            let (seconds, begun) = one_shot_ready(out, sfx, &r, h, sound, Wanted::Once(at), Instant::now(), frame, None);
            sfx.waits.decided_unless_waiting(0, begun);
            OneShotNow::Played(seconds)
        }
    }
}

/// A voice slot ([`MAX_VOICES`], [`MAX_SAME`]) [design, #2217]: the one-shot in it, its
/// start number (the oldest is taken over first) and the frame it started in (one
/// started in this frame is not attached to the output yet: it has made no sound). A
/// one-shot fading out after a stop (`retiring`) keeps its slot until its sink is empty
/// (`ended`), and so does a slot a [`Replacement`] waits for (`reserved`) until that
/// starts or is cancelled: a fading sound counts against both limits, so the limits are
/// never passed, not even for the fade.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Slot {
    start: u64,
    entity: Entity,
    frame: u64,
    retiring: bool,
    reserved: bool,
    ended: bool,
}

/// A one-shot asked for at its SOUN's [`MAX_SAME`] waiting for a slot whose sound fades
/// out after the steal (#2217): the slot's entity and start number, what it plays (the
/// file picked and the pitch chosen when it was asked for, a Random Frequency Shift
/// rolled then, once), when it was asked for and the frame it was asked in (`frame`,
/// [`Sfx::frame`]: [`Waits`] counts its frames from there to its start). It starts once
/// that sound has ended, with the listener as it is then
/// (its gain, pan and place worked out again; nothing rolled again); a game event's one
/// waiting longer than [`MAX_ONE_SHOT_WAIT`] in all is dropped. One per slot at most, so
/// at most [`MAX_VOICES`].
struct Replacement {
    slot: Entity,
    start: u64,
    sound: FormId,
    r: Resolved,
    handle: Handle<PcmSound>,
    speed: f32,
    wanted: Wanted,
    asked: Instant,
    frame: u64,
}

impl Sfx {
    /// The one-shot `e` of `sound` is fading out after a stop.
    fn retiring(&mut self, sound: FormId, e: Entity) {
        for s in self.same.get_mut(&sound).into_iter().flatten().filter(|s| s.entity == e) {
            s.retiring = true;
        }
    }

    /// The slot of `sound` held by `slot` given up: gone if its sound has ended (or
    /// always, `whole`), else no longer reserved.
    fn release(&mut self, sound: FormId, slot: Entity, whole: bool) {
        let Some(list) = self.same.get_mut(&sound) else { return };
        let Some(i) = list.iter().position(|s| s.entity == slot) else { return };
        if whole || list[i].ended {
            list.swap_remove(i);
            self.voices = self.voices.saturating_sub(1);
        } else {
            list[i].reserved = false;
        }
    }

    /// Drop the replacements `drop` picks (a StopSound), giving their slots back.
    fn cancel_replacements(&mut self, drop: impl Fn(&Replacement) -> bool) {
        let mut i = 0;
        while i < self.pending.len() {
            if drop(&self.pending[i]) {
                let p = self.pending.remove(i);
                self.release(p.sound, p.slot, false);
                self.replacements_cancelled += 1;
            } else {
                i += 1;
            }
        }
    }
}

/// The replacements whose slot's sound has ended start in it, oldest first: as they
/// would have when asked for, the listener and the gains as they are now; a game
/// event's one-shot asked for more than [`MAX_ONE_SHOT_WAIT`] ago is dropped instead.
fn start_replacements(out: &mut Out, sfx: &mut Sfx, heard: &mut Vec<(u32, Option<f32>)>) {
    if sfx.pending.is_empty() {
        return;
    }
    let ended = |sfx: &Sfx, p: &Replacement| sfx.same.get(&p.sound).is_some_and(|l| l.iter().any(|s| s.entity == p.slot && s.ended));
    let mut i = 0;
    while i < sfx.pending.len() {
        if !ended(sfx, &sfx.pending[i]) {
            i += 1;
            continue;
        }
        let p = sfx.pending.remove(i);
        if wait_limited(p.wanted) && p.asked.elapsed() > sfx.one_shot_wait {
            sfx.release(p.sound, p.slot, true);
            sfx.replacements_expired += 1;
            continue;
        }
        let (seconds, begun) = one_shot_ready(out, sfx, &p.r, p.handle, p.sound, p.wanted, p.asked, p.frame, Some((p.slot, p.speed)));
        match begun {
            Begun::Started(_) => sfx.replacements_started += 1,
            _ => sfx.replacements_silenced += 1,
        }
        // Begun (or silent) now: counted by the frames from its request to this one, the
        // wait for its file and for its slot both.
        sfx.waits.decided(sfx.frame.saturating_sub(p.frame), matches!(begun, Begun::Started(_)));
        one_shot_decided(out, sfx, heard, p.sound, p.wanted, seconds, begun);
    }
}

/// The slot a one-shot of a SOUN at [`MAX_SAME`] takes over: one already fading with no
/// replacement yet (no voice is cut for it), else the oldest playing one; `None` when
/// every slot has its replacement already.
fn slot_to_take(list: &[Slot]) -> Option<usize> {
    list.iter().position(|s| s.retiring && !s.reserved).or_else(|| (0..list.len()).filter(|&i| !list[i].retiring && !list[i].reserved).min_by_key(|&i| list[i].start))
}

/// Play a SOUN once whose file is ready, at a place (heard from the player's eye and
/// panned for them) or at the listener: the seconds it lasts as played, also when it is
/// too quiet to be heard or sounds are switched off, and what became of it ([`Begun`]:
/// the entity playing it, waiting for a voice slot, or nothing heard). `wanted`, `asked`
/// and `asked_frame` (the frame it was asked in) are kept for a [`Replacement`]; `kept`: a
/// replacement starting, the slot reserved for it (its sound ended, not counted against
/// the limits again) and the pitch chosen when it was asked for (not rolled again).
#[allow(clippy::too_many_arguments)]
fn one_shot_ready(out: &mut Out, sfx: &mut Sfx, r: &Resolved, handle: Handle<PcmSound>, sound: FormId, wanted: Wanted, asked: Instant, asked_frame: u64, kept: Option<(Entity, f32)>) -> (f32, Begun) {
    let slot = kept.map(|k| k.0);
    let (at, scale) = match wanted {
        Wanted::Once(at) | Wanted::Script { at, .. } => (at, 1.0),
        // The player's Weapon Volume tweak's shots (`ScaledSound`).
        Wanted::Scaled(at, gain) => (at, gain),
        Wanted::Loop => (None, 1.0),
    };
    let (s, name, path) = (&r.s, &r.name, &r.path);
    let (settings, eye) = (out.settings, out.player.map(Player::eye));
    let placed = at.zip(out.player).map(|(at, p)| SoundAt::new(at, Some(p)));
    let distance = at.zip(eye).map(|(at, e)| ((at[0] - e[0]).powi(2) + (at[1] - e[1]).powi(2) + (at[2] - e[2]).powi(2)).sqrt());
    let gain = static_gain(s) * distance.map_or(1.0, |d| distance_gain(s, d)) * settings.volume.max(0.0) * scale;
    // The pitch: a replacement's was chosen when it was asked for; only what hangs on the
    // listener (gain, pan, place) is worked out again.
    let speed = match kept {
        Some((_, speed)) => speed,
        None => {
            let mut speed = 1.0 + s.frequency_adjust_percent / 100.0;
            if s.has("Random Frequency Shift") {
                speed *= 0.95 + 0.1 * sfx.rng.next_f32();
            }
            speed.clamp(0.25, 4.0)
        }
    };
    let seconds = out.assets.get(&handle).map_or(0.0, |p| p.seconds() as f32) / speed;
    debug!("sfx: {name} {path} gain {gain:.3}{} speed {speed:.2}", distance.map_or(String::new(), |d| format!(" at {d:.0} units")));
    // Under water a sound so flagged is not heard; a 2D one is not panned (audio.md §15.2).
    if sfx.submerged && s.has("Mute When Submerged") {
        sfx.muted_submerged += 1;
        sfx.silent += 1;
        if let Some(slot) = slot {
            sfx.release(sound, slot, true);
        }
        return (seconds, Begun::Refused);
    }
    // Heard at the listener, for the sound cover (`crate::sound_cover`): what the record
    // and the distance leave, the effects volume not in it.
    // At most a battle's worth waits, should nothing take them.
    if let (true, Some(d), true) = (settings.play, distance, sfx.cover_notes.len() < 512) {
        sfx.cover_notes.push((static_gain(s) * distance_gain(s, d), d, seconds));
    }
    let placed = match placed {
        Some(_) if s.has("2D") => {
            sfx.flat_2d += 1;
            None
        }
        p => p,
    };
    if !settings.play || gain < 0.001 {
        sfx.silent += 1;
        if let Some(slot) = slot {
            sfx.release(sound, slot, true);
        }
        return (seconds, Begun::Refused);
    }
    let interface = is_interface(s.edid.as_deref());
    // Started in the slot of another one-shot of its SOUN (the voice limit's steal).
    let mut over_oldest = slot.is_some();
    if let Some(slot) = slot {
        // The reserved slot, its sound ended, is this one's now.
        sfx.release(sound, slot, true);
    } else if !interface {
        let same = sfx.same.get(&sound).map_or(0, Vec::len);
        if same >= MAX_SAME {
            // A slot of this SOUN is taken over: one fading already, else its oldest
            // voice fades out (#2217: not cut). The new one starts once that sound has
            // ended, unless the slot's one-shot started in this frame and has made no
            // sound yet: then that one is dropped and this one starts in its place now.
            let frame = sfx.frame;
            let list = sfx.same.get_mut(&sound).expect("counted above");
            let Some(i) = slot_to_take(list) else {
                // Every slot of the SOUN has a replacement waiting already.
                sfx.voice_limited += 1;
                sfx.per_sound.entry(sound).or_default()[2] += 1;
                return (seconds, Begun::Refused);
            };
            let taken = list[i];
            if !taken.retiring && taken.frame == frame {
                list.swap_remove(i);
                stop_sound(out.commands, taken.entity, StopReason::Steal);
                sfx.voices = sfx.voices.saturating_sub(1);
                sfx.taken_at_once += 1;
                over_oldest = true;
            } else {
                list[i].retiring = true;
                list[i].reserved = true;
                if !taken.retiring {
                    // No longer a script's sound (StopSound finds nothing) nor fading in.
                    out.commands.entity(taken.entity).try_remove::<(ScriptPlayed, ScriptFade)>();
                    stop_sound(out.commands, taken.entity, StopReason::Steal);
                }
                sfx.pending.push(Replacement { slot: taken.entity, start: taken.start, sound, r: r.clone(), handle, speed, wanted, asked, frame: asked_frame });
                sfx.replacements_waited += 1;
                return (seconds, Begun::Waiting);
            }
        } else if !voice_free(sfx.voices, same) {
            sfx.voice_limited += 1;
            sfx.per_sound.entry(sound).or_default()[2] += 1;
            return (seconds, Begun::Refused);
        }
    }
    if !interface {
        sfx.voices += 1;
        sfx.most_voices = sfx.most_voices.max(sfx.voices);
    }
    if over_oldest {
        sfx.voice_stolen += 1;
        sfx.per_sound.entry(sound).or_default()[1] += 1;
    }
    sfx.played += 1;
    let playback = PlaybackSettings::DESPAWN.with_volume(Volume::Linear(gain)).with_speed(speed);
    let category = SoundCategory::of_effect(s.edid.as_deref());
    let reverb = crate::reverb::for_sound(sfx.room_reverb, settings, s);
    if reverb.is_none() && sfx.room_reverb.is_some() && settings.reverb && crate::reverb::send_of(s.reverb_attenuation_percent) <= 0.0 {
        sfx.reverb_kept_out += 1;
    }
    if distance.is_some() && s.attenuation_curve.is_some_and(|c| c != DEFAULT_ATTENUATION_CURVE) {
        sfx.own_curve += 1;
    }
    if let Some(p) = reverb {
        sfx.reverbed += 1;
        sfx.reverb_send_sum += p.send;
        debug!("sfx: {name} through reverb {} (send {:.2})", p.preset.name, p.send);
    }
    // A shot's tail and distance shaping (Modern Feel, `crate::gun_audio`), if combat
    // marked this sound as a shot's.
    let shot = crate::gun_audio::take(sound, at, distance, sfx.room_reverb);
    let own = |pcm: PcmSound, pan: Option<Pan>| PcmSound { pan, reverb, shot, ..pcm };
    let mut e = match (placed, out.assets.get(&handle).cloned()) {
        // This playing's own asset: the samples are shared, the pan and reverb are its own.
        (Some(placed), Some(pcm)) => out.commands.spawn((AudioPlayer::<PcmSound>(out.assets.add(own(pcm, Some(placed.pan.clone())))), playback, placed, SfxSound, category)),
        (None, Some(pcm)) if reverb.is_some() || shot.is_some() => out.commands.spawn((AudioPlayer::<PcmSound>(out.assets.add(own(pcm, None))), playback, SfxSound, category)),
        _ => out.commands.spawn((AudioPlayer::<PcmSound>(handle), playback, SfxSound, category)),
    };
    if interface {
        e.insert(KeepsSpeed);
    } else {
        let n = sfx.next_voice;
        sfx.next_voice += 1;
        e.insert(OneShot(sound, n, sfx.frame));
        let id = e.id();
        let list = sfx.same.entry(sound).or_default();
        list.push(Slot { start: n, entity: id, frame: sfx.frame, retiring: false, reserved: false, ended: false });
        sfx.most_same = sfx.most_same.max(list.len());
        sfx.per_sound.entry(sound).or_default()[0] += 1;
    }
    (seconds, Begun::Started(e.id()))
}

/// An interface sound by its SOUN editor ID (`UI…`: menus, VATS, the Pip-Boy), which keeps
/// its speed in slow motion (`audio_out::SoundSpeed`) [hypothesis: the base game's
/// interface records are named so, docs/spec-notes/audio.md].
pub(crate) fn is_interface(edid: Option<&str>) -> bool {
    edid.and_then(|e| e.get(..2)).is_some_and(|p| p.eq_ignore_ascii_case("UI"))
}

impl Sfx {
    /// The `sfx voices: …` exit line: the most slots taken (fading voices and reserved ones
    /// counted), the requests that waited for a slot to empty (Q) and what became of them
    /// (Q = started + cancelled + expired + silenced at start + still waiting), and the
    /// one-shots started at once over one of their SOUN not heard yet (no replacement).
    pub fn voices_line(&self) -> String {
        format!(
            "sfx voices: at most {} total, {} of one SOUN; {} replacements waiting, {} replacements started, {} replacements cancelled, {} replacements expired, {} replacements silenced at start, {} replacements still waiting; {} one-shots started at once over one not heard yet",
            self.most_voices,
            self.most_same,
            self.replacements_waited,
            self.replacements_started,
            self.replacements_cancelled,
            self.replacements_expired,
            self.replacements_silenced,
            self.pending.len(),
            self.taken_at_once
        )
    }
}

fn report(mut exit: MessageReader<AppExit>, sfx: Option<Res<Sfx>>, stops: Option<Res<SoundStops>>) {
    if let (Some(_), Some(s)) = (exit.read().last(), sfx) {
        // The sounds stopped before their end and the voice slots (#2217), told whether
        // or not anything played.
        if let Some(stops) = stops {
            info!("{}", stops.hard_line());
            info!("{}", stops.declick_line());
        }
        info!("{}", s.voices_line());
        if s.played + s.silent + s.failed + s.waits.cancelled + s.waits.rolls_skipped + s.waits.too_late > 0 || !s.waiting.is_empty() {
            info!(
                "sfx: {} sounds played, {} too quiet or switched off, {} without a file we can play; {} silenced under water (at most {:.0} under a surface), {} 2D sounds not panned; {} not started at the voice limit, at most {} one-shots at once; {} placed by a curve of their own",
                s.played, s.silent, s.failed, s.muted_submerged, s.deepest, s.flat_2d, s.voice_limited, s.most_voices, s.own_curve
            );
            // Sounds whose file was still being read when they were asked for (#1428).
            let w = &s.waits;
            info!(
                "sfx files: {} sounds began in the frame they were asked for, {} later while their file was read ({} more than one frame later, at most {} frames); {} decided silent (too quiet, under water, sounds off or the voice limit); {} dropped while they waited, {} waited for a file that could not be played, {} still waiting at the end; {} random sound rolls skipped while their file was read; {} started late in a frame once their file came in, {} of them in the frame they were asked for, those at most {:.1} ms from joining the waiting list to their start (not the delay until heard); {} one-shots dropped, their file in after more than {} ms; {} requests found their file ready in the frame they were asked for, {} later (at most {} frames)",
                w.at_once, w.late, w.over_one, w.most_frames, w.silent, w.cancelled, w.broken, s.waiting.len(), w.rolls_skipped, w.late_started, w.late_same_frame, w.late_most_ms, w.too_late, MAX_ONE_SHOT_WAIT.as_millis(), w.files_at_once, w.files_late, w.files_most_frames
            );
            if s.voice_stolen > 0 {
                info!("sfx: {} one-shots took over the oldest voice of their sound at {MAX_SAME} playing", s.voice_stolen);
            }
            // The sounds that met a limit (#1098: which shots of a burst went unheard).
            let mut limited: Vec<_> = s.per_sound.iter().filter(|(_, c)| c[1] + c[2] > 0).collect();
            limited.sort_by_key(|(sound, _)| **sound);
            for (sound, [started, stolen, refused]) in limited {
                let name = s.forms.sounds.get(sound).and_then(|x| x.edid.clone()).unwrap_or_else(|| sound.to_string());
                info!("sfx: {name}: {started} started, {stolen} of them over its oldest voice, {refused} refused at the voice limit");
            }
        }
    }
}

/// The systems that play sounds asked for, after [`crate::PlaySet::Sync`]: every Update
/// system that sends a [`PlaySound`], a [`LoopSound`], a script's sound or a
/// [`SoundSource`] orders itself before this set, so its request is seen in the frame it
/// is asked in (the exit report counts waits from that frame). Inside, [`SfxStep`].
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SfxSet;

/// The steps of [`SfxSet`], in order: the listener under water and the frame's
/// accounting ([`prepare`]), the ambient's placed sounds and region sounds
/// ([`crate::ambient`]), then the requests ([`play`]) and the panning.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SfxStep {
    Prepare,
    Ambient,
    Play,
}

pub struct SfxPlugin;

impl Plugin for SfxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SfxSettings>()
            .add_audio_source::<PcmSound>()
            .add_message::<PlaySound>()
            .add_message::<ScaledSound>()
            .add_message::<LoopSound>()
            .init_resource::<ScriptSounds>()
            .add_message::<SoundSource>()
            .add_systems(Startup, init)
            .configure_sets(Update, SfxSet.after(crate::PlaySet::Sync))
            .configure_sets(Update, (SfxStep::Prepare, SfxStep::Ambient, SfxStep::Play).chain().in_set(SfxSet))
            .add_systems(Update, (listener_submerged, prepare).chain().in_set(SfxStep::Prepare))
            .init_resource::<LoopScale>()
            .add_systems(Update, (play, pan_sounds, script_fades, loop_scales).chain().in_set(SfxStep::Play))
            .add_systems(Last, (release_files, report));
        add_late_start(app);
        crate::reverb::build(app);
        crate::gun_audio::build(app);
        crate::occlusion::build(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_voice_limit_holds_back_a_crowd() {
        assert!(voice_free(0, 0));
        assert!(!voice_free(MAX_VOICES, 0), "all voices playing");
        assert!(!voice_free(1, MAX_SAME), "that sound playing its most");
        assert!(voice_free(MAX_VOICES - 1, MAX_SAME - 1));
    }

    fn wav(tag: u16, channels: u16, rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
        let mut v = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        v.extend(16u32.to_le_bytes());
        v.extend(tag.to_le_bytes());
        v.extend(channels.to_le_bytes());
        v.extend(rate.to_le_bytes());
        v.extend((rate * channels as u32 * bits as u32 / 8).to_le_bytes());
        v.extend((channels * bits / 8).to_le_bytes());
        v.extend(bits.to_le_bytes());
        // A chunk of odd length before the data, padded to an even one.
        v.extend(b"LIST");
        v.extend(3u32.to_le_bytes());
        v.extend([1, 2, 3, 0]);
        v.extend(b"data");
        v.extend((data.len() as u32).to_le_bytes());
        v.extend(data);
        v
    }

    #[test]
    fn wav_pcm_is_decoded() {
        let s = PcmSound::from_wav(&wav(1, 2, 44100, 16, &[0, 0x40, 0, 0xC0, 0xFF, 0x7F, 0, 0x80])).unwrap();
        assert_eq!((s.channels, s.rate), (2, 44100));
        assert_eq!(&s.samples[..], &[0.5, -0.5, 32767.0 / 32768.0, -1.0]);
        assert!((s.seconds() - 2.0 / 44100.0).abs() < 1e-9);
        let s = PcmSound::from_wav(&wav(1, 1, 22050, 8, &[128, 255, 0])).unwrap();
        assert_eq!(&s.samples[..], &[0.0, 127.0 / 128.0, -1.0]);
        // ADPCM (tag 2) is named, not played as noise.
        assert!(PcmSound::from_wav(&wav(2, 1, 22050, 4, &[1, 2])).unwrap_err().contains("format tag 2"));
        assert!(PcmSound::from_wav(b"OggS").is_err());
    }

    #[test]
    fn interface_sounds_are_the_ui_records() {
        assert!(is_interface(Some("UIVATSEnter")) && is_interface(Some("uiMenuOK")));
        assert!(!is_interface(Some("WPNPistol10mmFire3D")) && !is_interface(Some("U")) && !is_interface(None));
    }

    #[test]
    fn panned_by_side() {
        // Facing north (heading 0): east is to the right, west to the left.
        let eye = [0.0, 0.0, 0.0];
        assert_eq!(pan_gains(eye, 0.0, [0.0, 500.0, 0.0]), [1.0, 1.0]);
        assert_eq!(pan_gains(eye, 0.0, [0.0, -500.0, 0.0]), [1.0, 1.0]);
        assert_eq!(pan_gains(eye, 0.0, [0.0, 0.0, 500.0]), [1.0, 1.0]);
        let right = pan_gains(eye, 0.0, [500.0, 0.0, 0.0]);
        assert!((right[0] - (1.0 - PAN_DEPTH)).abs() < 1e-5 && right[1] == 1.0);
        let left = pan_gains(eye, 0.0, [-500.0, 0.0, 0.0]);
        assert!(left[0] == 1.0 && (left[1] - (1.0 - PAN_DEPTH)).abs() < 1e-5);
        // Facing east (π/2): a sound to the north is on the left.
        let g = pan_gains(eye, std::f32::consts::FRAC_PI_2, [0.0, 500.0, 0.0]);
        assert!(g[0] == 1.0 && g[1] < 0.5);
        // At the listener: centred.
        assert_eq!(pan_gains(eye, 1.0, [0.2, 0.0, 0.0]), [1.0, 1.0]);
    }

    #[test]
    fn a_panned_sound_is_stereo() {
        let pan = Pan::new([1.0, 0.5]);
        let mono = PcmSound { samples: vec![0.8, -0.4].into(), channels: 1, rate: 22050, pan: Some(pan.clone()), reverb: None, shot: None };
        let d = mono.decoder();
        assert_eq!(d.channels().get(), 2);
        assert_eq!(d.collect::<Vec<_>>(), vec![0.8, 0.4, -0.4, -0.2]);
        // Stereo in: the two channels are mixed to one, then placed; a frame cut short
        // at the end is dropped.
        let stereo = PcmSound { samples: vec![0.2, 0.6, 1.0, 1.0, 0.5].into(), channels: 2, rate: 22050, pan: Some(pan.clone()), reverb: None, shot: None };
        assert_eq!(stereo.decoder().collect::<Vec<_>>(), vec![0.4, 0.2, 1.0, 0.5]);
        // Not placed: as it is.
        let plain = PcmSound { pan: None, ..mono.clone() };
        assert_eq!(plain.decoder().channels().get(), 1);
        assert_eq!(plain.decoder().collect::<Vec<_>>(), vec![0.8, -0.4]);
        // Another module's source, panned.
        let other = Panned::new(plain, pan);
        assert_eq!(other.decoder().channels().get(), 2);
        assert_eq!(other.decoder().collect::<Vec<_>>(), vec![0.8, 0.4, -0.4, -0.2]);
    }

    #[test]
    fn pan_gains_ease_in_both_decoders() {
        const RATE: u32 = 48_000;
        let step = 1.0 / (RATE as f32 * PAN_EASE.as_secs_f32());
        let ones = |pan: Option<Pan>| PcmSound { samples: vec![1.0; RATE as usize].into(), channels: 1, rate: RATE, pan, reverb: None, shot: None };
        // A placed PCM sound, and another module's source panned (the same file unplaced
        // under `Panned`): a constant 1, so each frame plays its gains as they are.
        let pcm_pan = Pan::new([1.0, 0.3]);
        let other_pan = Pan::new([1.0, 0.3]);
        let decoders: Vec<(&str, Pan, Box<dyn Iterator<Item = Sample>>)> = vec![
            ("pcm", pcm_pan.clone(), Box::new(ones(Some(pcm_pan)).decoder())),
            ("panned", other_pan.clone(), Box::new(Panned::new(ones(None), other_pan).decoder())),
        ];
        for (name, pan, mut d) in decoders {
            let mut frame = || (d.next().unwrap(), d.next().unwrap());
            // The first frames at the targets as they are, not eased in from anywhere.
            for _ in 0..3 {
                assert_eq!(frame(), (1.0, 0.3), "{name}");
            }
            // The listener turns: the sound swaps sides. Each gain moves by a step at most
            // a frame, steadily, both of a frame from one pair (the two moves mirror each
            // other, so a right sample of another frame than its left would show).
            pan.set([0.3, 1.0]);
            let mut last = (1.0f32, 0.3f32);
            let mut frames = 0;
            while last != (0.3, 1.0) {
                let (l, r) = frame();
                frames += 1;
                assert!(l <= last.0 && last.0 - l <= step * 1.001, "{name} frame {frames}: left {} -> {l}", last.0);
                assert!(r >= last.1 && r - last.1 <= step * 1.001, "{name} frame {frames}: right {} -> {r}", last.1);
                assert!((l + r - 1.3).abs() < 1e-4, "{name} frame {frames}: {l} {r}, not one pair");
                last = (l, r);
                assert!(frames < RATE as usize / 10, "{name}: never settles");
            }
            // 0.7 of a unit takes 0.7 of PAN_EASE, give or take a frame; then it stays.
            let expected = 0.7 / step;
            assert!((frames as f32 - expected).abs() <= 2.0, "{name}: {frames} frames, about {expected} expected");
            for _ in 0..100 {
                assert_eq!(frame(), (0.3, 1.0), "{name}: settled at the targets");
            }
            // A target that is not a number holds the gain where it is.
            pan.set([f32::NAN, 1.0]);
            assert_eq!(frame(), (0.3, 1.0), "{name}");
        }
    }

    #[test]
    fn a_sound_through_reverb_is_stereo_with_a_tail() {
        let room = crate::reverb::preset(Some("Small Room"));
        let mono = PcmSound { samples: vec![0.5; 100].into(), channels: 1, rate: 22050, pan: None, reverb: room.map(RoomSend::full), shot: None };
        let d = mono.decoder();
        assert_eq!(d.channels().get(), 2);
        let tail = crate::reverb::Reverb::tail_seconds(room.unwrap());
        assert!((d.total_duration().unwrap().as_secs_f64() - (100.0 / 22050.0 + tail as f64)).abs() < 1e-3);
        let out: Vec<f32> = d.collect();
        assert_eq!(out.len(), 2 * (100 + (tail * 22050.0) as usize));
        // The dry sound first, on both ears, before any reflection arrives.
        assert_eq!(&out[..4], &[0.5, 0.5, 0.5, 0.5]);
        // Something rings on after the file's end.
        assert!(out[200..].iter().any(|s| s.abs() > 1e-4));
        // A panned sound keeps its pan in the dry part.
        let panned = PcmSound { pan: Some(Pan::new([1.0, 0.5])), ..mono };
        assert_eq!(&panned.decoder().collect::<Vec<_>>()[..2], &[0.5, 0.25]);
    }

    #[test]
    fn loudness_by_distance() {
        let s = Sound { min_distance: 200.0, max_distance: 1000.0, static_attenuation_db: 20.0, ..Sound::default() };
        assert!((static_gain(&s) - 0.1).abs() < 1e-6);
        assert_eq!(distance_gain(&s, 0.0), 1.0);
        assert_eq!(distance_gain(&s, 200.0), 1.0);
        // The default curve (100, 50, 20, 5, 0) over the 800 units from 200 to 1000.
        assert!((distance_gain(&s, 400.0) - 0.5).abs() < 1e-6);
        assert!((distance_gain(&s, 600.0) - 0.2).abs() < 1e-6);
        assert!((distance_gain(&s, 700.0) - 0.125).abs() < 1e-6);
        assert!((distance_gain(&s, 800.0) - 0.05).abs() < 1e-6);
        assert_eq!(distance_gain(&s, 1000.0), 0.0);
        assert_eq!(distance_gain(&s, 5000.0), 0.0);
        // The record's own curve: FXBulletWhiz's (100, 30, 20, 5, 0) falls faster at first.
        let whiz = Sound { attenuation_curve: Some([100.0, 30.0, 20.0, 5.0, 0.0]), ..s.clone() };
        assert!((distance_gain(&whiz, 400.0) - 0.3).abs() < 1e-6);
        // A flat one (100 at every point) is whole up to the outer radius.
        let flat = Sound { attenuation_curve: Some([100.0; 5]), ..s.clone() };
        assert_eq!(distance_gain(&flat, 999.0), 1.0);
        // No span: the inner level, then min ÷ distance.
        let point = Sound { min_distance: 200.0, max_distance: 0.0, ..Sound::default() };
        assert!((distance_gain(&point, 400.0) - 0.5).abs() < 1e-6);
    }

    /// A RIFF/WAVE file of `fmt ` fields as given (no arithmetic on them, so any value
    /// goes in) and `data` declaring `data_len` bytes with `data` after it.
    fn raw_wav(tag: u16, channels: u16, rate: u32, bits: u16, data_len: u32, data: &[u8]) -> Vec<u8> {
        let mut v = b"RIFF\xFF\xFF\xFF\xFFWAVEfmt ".to_vec();
        v.extend(16u32.to_le_bytes());
        v.extend(tag.to_le_bytes());
        v.extend(channels.to_le_bytes());
        v.extend(rate.to_le_bytes());
        v.extend(0u32.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(bits.to_le_bytes());
        v.extend(b"data");
        v.extend(data_len.to_le_bytes());
        v.extend(data);
        v
    }

    #[test]
    fn broken_wav_headers_are_refused_not_played() {
        // Zero, absurd and too many channels or rates: refused, never a panic or a
        // rate the mixer cannot convert.
        for (channels, rate) in [(1, 0), (0, 44100), (1, 1), (1, u32::MAX), (1, 999), (1, 384_001), (9, 44100), (u16::MAX, 44100)] {
            let e = PcmSound::from_wav(&raw_wav(1, channels, rate, 16, 4, &[0; 4])).unwrap_err();
            assert!(e.contains("rate") || e.contains("channels"), "{channels} ch {rate} Hz: {e}");
            assert!(format_ok(channels, rate).is_err());
        }
        assert!(format_ok(2, 44100).is_ok() && format_ok(1, 1_000).is_ok() && format_ok(8, 384_000).is_ok());
        // A data chunk declaring 4 GB holds what the file has, no more.
        let s = PcmSound::from_wav(&raw_wav(1, 1, 22050, 16, u32::MAX, &[0, 0x40, 0, 0xC0, 7])).unwrap();
        assert_eq!(&s.samples[..], &[0.5, -0.5]);
    }

    #[test]
    fn broken_wav_bodies_do_not_panic() {
        // Cut anywhere, a WAV is refused or holds less; no index runs past the end.
        let whole = raw_wav(1, 2, 44100, 16, 8, &[1, 2, 3, 4, 5, 6, 7, 8]);
        for n in 0..whole.len() {
            let _ = PcmSound::from_wav(&whole[..n]);
        }
        assert!(PcmSound::from_wav(b"").is_err());
        assert!(PcmSound::from_wav(b"RIFF\0\0\0\0WAVE").unwrap_err().contains("no data"));
        // A chunk claiming 4 GB before the data ends the walk; a zero-sized one moves on.
        let mut big = b"RIFF\0\0\0\0WAVEjunk".to_vec();
        big.extend(u32::MAX.to_le_bytes());
        big.extend([0; 64]);
        assert!(PcmSound::from_wav(&big).is_err());
        let mut zero = b"RIFF\0\0\0\0WAVE".to_vec();
        for _ in 0..1000 {
            zero.extend(b"junk\0\0\0\0");
        }
        assert!(PcmSound::from_wav(&zero).unwrap_err().contains("no data"));
        // A fmt chunk too short to read, then data: "data before fmt".
        let mut short = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        short.extend(4u32.to_le_bytes());
        short.extend([1, 0, 1, 0, b'd', b'a', b't', b'a', 2, 0, 0, 0, 0, 0]);
        assert!(PcmSound::from_wav(&short).is_err());
        // Garbled floats are silence, not NaN through the mix.
        let mut floats = Vec::new();
        for x in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.25] {
            floats.extend(x.to_le_bytes());
        }
        let s = PcmSound::from_wav(&raw_wav(3, 1, 44100, 32, 16, &floats)).unwrap();
        assert_eq!(&s.samples[..], &[0.0, 0.0, 0.0, 0.25]);
    }

    #[test]
    fn broken_encoded_files_are_refused() {
        // Empty, garbage, a cut Ogg and a WAV of rate 0 through rodio: an error, never a
        // panic out of the guard the play path runs it in ([`decode_file`]; symphonia-core
        // 0.5.5 itself panics opening a WAV of rate 0 [observed: units.rs:150]).
        let ogg = fnv_voice::length::synth::ogg_vorbis(2, 44100, 44100, 4);
        let mut garbage = Vec::new();
        let mut x = 0x1234_5678u32;
        for _ in 0..4096 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            garbage.push(x as u8);
        }
        let mut ogg_garbage = b"OggS".to_vec();
        ogg_garbage.extend(&garbage);
        for bytes in [Vec::new(), garbage.clone(), ogg_garbage, ogg[..ogg.len() / 2].to_vec(), ogg.clone(), raw_wav(1, 1, 0, 16, 4, &[0; 4]), raw_wav(1, 1, u32::MAX, 16, 4, &[0; 4])] {
            if let Ok(s) = nvf_assets::guard::guarded(|| PcmSound::from_encoded(bytes.clone())) {
                assert!(format_ok(s.channels, s.rate).is_ok() && s.samples.len() <= MAX_DECODED_SAMPLES);
                assert!(s.samples.iter().all(|x| x.is_finite()));
            }
            if playable(&bytes).is_ok() {
                assert!(!bytes.is_empty());
            }
        }
        assert!(playable(&raw_wav(1, 1, 0, 16, 4, &[0; 4])).is_err());
        assert!(playable(&raw_wav(1, 1, 5_000_000, 16, 4, &[0; 4])).is_err());
    }

    #[test]
    fn broken_places_are_not_nan() {
        let eye = [0.0, 0.0, 0.0];
        for at in [[f32::NAN, 0.0, 0.0], [f32::INFINITY, 0.0, 0.0], [f32::MAX, f32::MAX, 0.0]] {
            assert_eq!(pan_gains(eye, 0.0, at), [1.0, 1.0], "{at:?}");
        }
        assert_eq!(pan_gains(eye, f32::NAN, [500.0, 0.0, 0.0]), [1.0, 1.0]);
        let s = Sound { min_distance: 100.0, max_distance: 1000.0, ..Sound::default() };
        assert_eq!(distance_gain(&s, f32::NAN), 0.0);
        assert_eq!(distance_gain(&s, f32::INFINITY), 0.0);
        let point = Sound { min_distance: 100.0, max_distance: 0.0, ..Sound::default() };
        assert_eq!(distance_gain(&point, f32::NAN), 0.0);
    }

    fn waiting(sound: u32, path: &str, frame: u64, wanted: Wanted) -> Waiting {
        // One request time for all, so requests compare by what they ask.
        static ASKED: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        Waiting { sound: FormId(sound), path: Some(path.into()), frame, asked: *ASKED.get_or_init(Instant::now), wanted }
    }

    #[test]
    fn waiting_requests_are_dropped_and_decided_in_order() {
        let script = |id| Wanted::Script { id, at: None, looping: false };
        let mut q = vec![waiting(1, "a", 1, Wanted::Loop), waiting(1, "a", 1, script(7)), waiting(2, "b", 2, Wanted::Loop), waiting(1, "c", 3, Wanted::Once(None))];
        // LoopSound of SOUN 1: only its own loop start goes.
        assert_eq!(cancel(&mut q, |w| w.sound == FormId(1) && w.wanted == Wanted::Loop), 1);
        // StopSound of SOUN 1: only the scripts' requests of it.
        assert_eq!(cancel(&mut q, |w| w.sound == FormId(1) && matches!(w.wanted, Wanted::Script { .. })), 1);
        assert_eq!(q, vec![waiting(2, "b", 2, Wanted::Loop), waiting(1, "c", 3, Wanted::Once(None))]);
        // Nothing decided while every file is pending; then the ready ones, oldest first.
        assert!(take_decided(&mut q, |_| true).is_empty());
        let pending = |p: &'static str| move |w: &Waiting| w.path.as_deref() == Some(p);
        assert_eq!(q.len(), 2);
        q.push(waiting(3, "b", 4, Wanted::Once(Some([1.0, 2.0, 3.0]))));
        let go = take_decided(&mut q, pending("c"));
        assert_eq!(go, vec![waiting(2, "b", 2, Wanted::Loop), waiting(3, "b", 4, Wanted::Once(Some([1.0, 2.0, 3.0])))]);
        assert_eq!(q, vec![waiting(1, "c", 3, Wanted::Once(None))]);
    }

    #[test]
    fn waits_count_late_starts() {
        let mut w = Waits::default();
        for waited in [0, 0, 1, 3, 2] {
            w.decided(waited, true);
        }
        // Silent ones are counted apart, whatever they waited.
        w.decided(9, false);
        assert_eq!((w.at_once, w.late, w.over_one, w.most_frames, w.silent), (2, 3, 2, 3, 1));
    }

    /// A data folder of loose files, removed when dropped.
    struct TestFiles(std::path::PathBuf);

    impl Drop for TestFiles {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const BEEP: FormId = FormId(0x10);
    const BROKEN: FormId = FormId(0x11);
    const BEEP_LOOP: FormId = FormId(0x12);
    const BROKEN_LOOP: FormId = FormId(0x13);
    const NO_FILE: FormId = FormId(0x14);

    fn record(edid: &str, file: Option<&str>, flags: &[&str]) -> Sound {
        Sound { edid: Some(edid.into()), file: file.map(Into::into), flags: flags.iter().map(|f| f.to_string()).collect(), ..Sound::default() }
    }

    /// Half a second of silence, 22,050 Hz mono.
    fn half_second() -> Vec<u8> {
        wav(1, 1, 22050, 16, &[0; 22050])
    }

    /// An app running [`play`] over a data folder holding a sound, a broken file and a
    /// folder of two takes, without game data otherwise.
    fn sound_app() -> (App, TestFiles) {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = TestFiles(std::env::temp_dir().join(format!("nvf-sfx-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))));
        let fx = dir.0.join("sound").join("fx");
        std::fs::create_dir_all(fx.join("takes")).unwrap();
        std::fs::write(fx.join("beep.wav"), half_second()).unwrap();
        std::fs::write(fx.join("broken.wav"), b"not a sound file").unwrap();
        std::fs::write(fx.join("takes").join("a.wav"), half_second()).unwrap();
        std::fs::write(fx.join("takes").join("b.wav"), half_second()).unwrap();
        let data = nvf_assets::GameData {
            vfs: Arc::new(fnv_vfs::Vfs::open(&dir.0, &[]).unwrap()),
            schema: Arc::new(fnv_nif::Schema::load_dir(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/nif")).unwrap()),
        };
        let mut forms = FxForms::default();
        forms.sounds.insert(BEEP, record("TestBeep", Some("fx\\beep.wav"), &[]));
        forms.sounds.insert(BROKEN, record("TestBroken", Some("fx\\broken.wav"), &[]));
        forms.sounds.insert(BEEP_LOOP, record("TestBeepLoop", Some("fx\\beep.wav"), &["Loop"]));
        forms.sounds.insert(BROKEN_LOOP, record("TestBrokenLoop", Some("fx\\broken.wav"), &["Loop"]));
        forms.sounds.insert(NO_FILE, record("TestNoFile", None, &[]));
        let mut app = App::new();
        app.insert_resource(data)
            .insert_resource(Sfx { one_shot_wait: Duration::from_secs(3600), ..Sfx::new(forms) })
            .init_resource::<SfxSettings>()
            .init_resource::<ScriptSounds>()
            .init_resource::<Assets<PcmSound>>()
            .add_message::<PlaySound>()
            .add_message::<ScaledSound>()
            .add_message::<LoopSound>()
            .add_systems(Update, (prepare, play).chain());
        (app, dir)
    }

    /// Frames run until `done` holds (the workers decode meanwhile); how many.
    fn run_until(app: &mut App, done: impl Fn(&mut World) -> bool) -> u32 {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut n = 0;
        loop {
            app.update();
            n += 1;
            if done(app.world_mut()) {
                return n;
            }
            assert!(std::time::Instant::now() < deadline, "the sound files were not decoded in time");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn script_plays(app: &mut App, plays: &[(u32, FormId)]) {
        app.world_mut().resource_mut::<ScriptSounds>().plays.extend(plays.iter().map(|&(id, s)| (id, s, None)));
    }

    fn heard(app: &mut App) -> Vec<(u32, Option<f32>)> {
        std::mem::take(&mut app.world_mut().resource_mut::<ScriptSounds>().heard)
    }

    fn count<C: Component>(app: &mut App) -> usize {
        let world = app.world_mut();
        world.query::<&C>().iter(world).count()
    }

    #[test]
    fn a_cold_file_is_decoded_on_a_worker_and_the_request_starts_when_it_is_ready() {
        let (mut app, _files) = sound_app();
        script_plays(&mut app, &[(1, BEEP)]);
        app.update();
        // Asked for, not yet decided: the script hears nothing and nothing plays.
        assert!(heard(&mut app).is_empty());
        assert_eq!(app.world().resource::<Sfx>().waiting.len(), 1);
        assert_eq!(count::<OneShot>(&mut app), 0);
        run_until(&mut app, |w| !w.resource::<ScriptSounds>().heard.is_empty());
        let h = heard(&mut app);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].0, 1);
        assert!((h[0].1.unwrap() - 0.5).abs() < 1e-3, "its seconds: {h:?}");
        assert_eq!(count::<ScriptPlayed>(&mut app), 1);
        let w = app.world().resource::<Sfx>().waits.clone();
        assert_eq!((w.at_once, w.late), (0, 1));
        assert!(w.most_frames >= 1);
        // The file is ready now: the next request is decided in its own frame.
        script_plays(&mut app, &[(2, BEEP)]);
        app.update();
        assert_eq!(heard(&mut app), vec![(2, Some(h[0].1.unwrap()))]);
        assert_eq!(app.world().resource::<Sfx>().waits.at_once, 1);
        assert!(app.world().resource::<Sfx>().waiting.is_empty());
    }

    #[test]
    fn a_one_shot_whose_file_comes_in_too_late_is_dropped_and_loops_are_not() {
        let (mut app, _files) = sound_app();
        app.world_mut().resource_mut::<Sfx>().one_shot_wait = MAX_ONE_SHOT_WAIT;
        app.world_mut().write_message(PlaySound { sound: BEEP, at: None });
        script_plays(&mut app, &[(1, BEEP), (2, BEEP_LOOP)]);
        app.update();
        // All three wait for the cold file; make them older than the limit.
        let old = Instant::now().checked_sub(MAX_ONE_SHOT_WAIT + Duration::from_millis(50)).unwrap();
        {
            let mut sfx = app.world_mut().resource_mut::<Sfx>();
            assert_eq!(sfx.waiting.len(), 3);
            for w in &mut sfx.waiting {
                w.asked = old;
            }
        }
        run_until(&mut app, |w| w.resource::<Sfx>().waiting.is_empty());
        // The game event's one-shot is dropped; the script's one-shot and loop start late.
        let mut h = heard(&mut app);
        h.sort_by_key(|&(id, _)| id);
        assert_eq!(h.len(), 2);
        assert!(matches!(h[0], (1, Some(s)) if s > 0.0), "{h:?}");
        assert_eq!(h[1], (2, None));
        assert_eq!(count::<ScriptPlayed>(&mut app), 2);
        let w = app.world().resource::<Sfx>().waits.clone();
        assert_eq!((w.too_late, w.late), (1, 2));
        // A fresh one-shot of the now-ready file plays as usual.
        app.world_mut().write_message(PlaySound { sound: BEEP, at: None });
        app.update();
        assert_eq!(count::<OneShot>(&mut app), 2);
        assert_eq!(app.world().resource::<Sfx>().waits.too_late, 1);
    }

    /// What a probe in the output's set saw: the sound effects' players there.
    #[derive(Resource, Default)]
    struct OutputSaw(Vec<usize>);

    fn output_probe(players: Query<(), (With<AudioPlayer<PcmSound>>, With<SfxSound>)>, mut saw: ResMut<OutputSaw>) {
        saw.0.push(players.iter().count());
    }

    /// The decode of `path` (in the data folder) finished on its worker.
    fn wait_decoded(app: &App, path: &str) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !matches!(app.world().resource::<Sfx>().files.get(path), Some(FileState::Decoding(t, _)) if t.is_finished()) {
            assert!(std::time::Instant::now() < deadline, "the decode did not finish in time");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// [`sound_app`] with the second chance registered as the plugin does, and a probe in
    /// the output's set ([`OutputSaw`], registered first so that without the ordering it
    /// would not see the late players); `ask` is asked for in this frame's [`play`] (its
    /// file `path` not decoded yet), and the worker has finished that decode before
    /// `PostUpdate` runs.
    fn asked_then_decoded_at(path: &str, ask: impl FnOnce(&mut App)) -> (App, TestFiles) {
        let (mut app, files) = sound_app();
        app.init_resource::<OutputSaw>().add_systems(PostUpdate, output_probe.in_set(crate::audio_out::AudioOutSystems));
        add_late_start(&mut app);
        ask(&mut app);
        app.world_mut().run_schedule(Update);
        assert!(!app.world().resource::<Sfx>().waiting.is_empty());
        wait_decoded(&app, path);
        app.world_mut().run_schedule(PostUpdate);
        (app, files)
    }

    fn asked_then_decoded_in_one_frame(ask: impl FnOnce(&mut App)) -> (App, TestFiles) {
        asked_then_decoded_at("sound\\fx\\beep.wav", ask)
    }

    #[test]
    fn a_file_decoded_after_play_starts_in_that_frames_post_update() {
        let (mut app, _files) = asked_then_decoded_in_one_frame(|app| {
            app.world_mut().write_message(PlaySound { sound: BEEP, at: None });
        });
        assert_eq!(count::<OneShot>(&mut app), 1);
        // The output's set saw the player in that frame.
        assert_eq!(app.world().resource::<OutputSaw>().0, vec![1]);
        let sfx = app.world().resource::<Sfx>();
        assert!(sfx.waiting.is_empty());
        // In the frame it was asked for: on time, counted as a late start too.
        assert_eq!((sfx.waits.at_once, sfx.waits.late, sfx.waits.most_frames), (1, 0, 0));
        assert_eq!((sfx.waits.late_started, sfx.waits.late_same_frame), (1, 1));
        assert!(sfx.waits.late_most_ms >= 0.0);
    }

    #[test]
    fn a_request_started_late_is_not_started_again() {
        let (mut app, _files) = asked_then_decoded_in_one_frame(|app| {
            app.world_mut().write_message(PlaySound { sound: BEEP, at: None });
        });
        // The next frames' play and second chance find nothing waiting.
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(count::<OneShot>(&mut app), 1);
        let sfx = app.world().resource::<Sfx>();
        assert_eq!(sfx.played, 1);
        assert_eq!((sfx.waits.at_once, sfx.waits.late, sfx.waits.late_started), (1, 0, 1));
    }

    #[test]
    fn a_script_sound_started_late_answers_heard_once() {
        let (mut app, _files) = asked_then_decoded_in_one_frame(|app| script_plays(app, &[(1, BEEP)]));
        let h = heard(&mut app);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].0, 1);
        assert!((h[0].1.unwrap() - 0.5).abs() < 1e-3, "its seconds: {h:?}");
        assert_eq!(count::<ScriptPlayed>(&mut app), 1);
        for _ in 0..3 {
            app.update();
        }
        assert!(heard(&mut app).is_empty());
        assert_eq!(count::<ScriptPlayed>(&mut app), 1);
    }

    #[test]
    fn the_voice_limit_counts_across_play_and_the_late_start() {
        let (mut app, _files) = sound_app();
        add_late_start(&mut app);
        // BEEP's file decoded beforehand; the one-shot it played gone again.
        script_plays(&mut app, &[(1, BEEP)]);
        run_until(&mut app, |w| !w.resource::<ScriptSounds>().heard.is_empty());
        heard(&mut app);
        let ones: Vec<Entity> = {
            let world = app.world_mut();
            world.query_filtered::<Entity, With<OneShot>>().iter(world).collect()
        };
        for e in ones {
            app.world_mut().despawn(e);
        }
        // Playing two short of the limit (Prepare counts them; two of them fading out
        // after a stop, which count until their fade is over), one more started in play
        // (BEEP, ready), two of a cold file waiting: the late start starts one, the
        // limit holds back the other.
        for n in 0..(MAX_VOICES - 2) as u64 {
            let mut e = app.world_mut().spawn(OneShot(FormId(0x99), 1000 + n, 0));
            if n < 2 {
                e.insert(Retiring);
            }
        }
        let takes = FormId(0x21);
        app.world_mut().resource_mut::<Sfx>().forms.sounds.insert(takes, record("TestTakeA", Some("fx\\takes\\a.wav"), &[]));
        app.world_mut().write_message_batch([PlaySound { sound: BEEP, at: None }, PlaySound { sound: takes, at: None }, PlaySound { sound: takes, at: None }]);
        app.world_mut().run_schedule(Update);
        assert_eq!(app.world().resource::<Sfx>().waiting.len(), 2);
        wait_decoded(&app, "sound\\fx\\takes\\a.wav");
        let before = app.world().resource::<Sfx>().waits.clone();
        let limited = app.world().resource::<Sfx>().voice_limited;
        app.world_mut().run_schedule(PostUpdate);
        let sfx = app.world().resource::<Sfx>();
        assert!(sfx.waiting.is_empty());
        assert_eq!(sfx.voice_limited - limited, 1);
        assert_eq!(sfx.most_voices, MAX_VOICES);
        assert_eq!((sfx.waits.late_started - before.late_started, sfx.waits.late_same_frame - before.late_same_frame), (1, 1));
        assert_eq!(count::<OneShot>(&mut app), MAX_VOICES);
    }

    #[test]
    fn a_waiting_file_that_fails_in_its_own_frame_counts_as_broken() {
        let (mut app, _files) = asked_then_decoded_at("sound\\fx\\broken.wav", |app| script_plays(app, &[(1, BROKEN)]));
        assert_eq!(heard(&mut app), vec![(1, Some(0.0))]);
        let sfx = app.world().resource::<Sfx>();
        assert!(sfx.waiting.is_empty());
        assert_eq!((sfx.waits.broken, sfx.failed, sfx.waits.late_started), (1, 1, 0));
    }

    #[test]
    fn a_stop_after_play_drops_the_waiting_script_sound_before_the_late_start() {
        let (mut app, _files) = asked_then_decoded_in_one_frame(|app| {
            script_plays(app, &[(1, BEEP)]);
            // Taken in by play only next frame: as a StopSound asked for after play.
            app.add_systems(Update, (|mut ss: ResMut<ScriptSounds>| ss.stops.push((BEEP, 1))).after(play));
        });
        assert!(heard(&mut app).is_empty());
        assert_eq!(count::<SfxSound>(&mut app), 0);
        let sfx = app.world().resource::<Sfx>();
        assert!(sfx.waiting.is_empty());
        assert_eq!((sfx.waits.cancelled, sfx.waits.late_started), (1, 0));
        // The stop is left for the next frame's play.
        assert_eq!(app.world().resource::<ScriptSounds>().stops, vec![(BEEP, 1)]);
    }

    #[test]
    fn stop_sound_drops_the_waiting_requests_asked_before_it() {
        let (mut app, _files) = sound_app();
        script_plays(&mut app, &[(1, BEEP)]);
        app.update();
        // StopSound, then PlaySound again, in one frame: the first is dropped, the new one
        // (asked for after the stop: a higher id than the stop's last) plays.
        app.world_mut().resource_mut::<ScriptSounds>().stops.push((BEEP, 1));
        script_plays(&mut app, &[(2, BEEP)]);
        run_until(&mut app, |w| !w.resource::<ScriptSounds>().heard.is_empty());
        let h = heard(&mut app);
        assert_eq!(h.iter().map(|x| x.0).collect::<Vec<_>>(), vec![2]);
        let sfx = app.world().resource::<Sfx>();
        assert_eq!(sfx.waits.cancelled, 1);
        assert!(sfx.waiting.is_empty());
    }

    #[test]
    fn play_then_stop_in_one_frame_plays_nothing() {
        let (mut app, _files) = sound_app();
        // PlaySound X; StopSound X (the stop's last id is the play's): dropped before it
        // starts, also when its file is ready; the loop of the same frame too.
        script_plays(&mut app, &[(1, BEEP), (2, BEEP_LOOP)]);
        app.world_mut().resource_mut::<ScriptSounds>().stops.extend([(BEEP, 2), (BEEP_LOOP, 2)]);
        app.update();
        assert!(heard(&mut app).is_empty());
        assert!(app.world().resource::<Sfx>().waiting.is_empty());
        assert_eq!(app.world().resource::<Sfx>().waits.cancelled, 2);
        // The file decoded meanwhile by another request; the same again.
        script_plays(&mut app, &[(3, BEEP)]);
        run_until(&mut app, |w| !w.resource::<ScriptSounds>().heard.is_empty());
        heard(&mut app);
        script_plays(&mut app, &[(4, BEEP)]);
        app.world_mut().resource_mut::<ScriptSounds>().stops.push((BEEP, 4));
        app.update();
        app.update();
        assert!(heard(&mut app).is_empty());
        assert_eq!(app.world().resource::<Sfx>().waits.cancelled, 3);
        assert!(stopped_before(&[(BEEP, 4)], BEEP, 4) && !stopped_before(&[(BEEP, 4)], BEEP, 5) && !stopped_before(&[(BEEP, 4)], BEEP_LOOP, 1));
        // Across the wrap of the ids: MAX was asked before a stop at 0, 0 after a stop at MAX.
        assert!(stopped_before(&[(BEEP, 0)], BEEP, u32::MAX) && !stopped_before(&[(BEEP, u32::MAX)], BEEP, 0));
    }

    /// A folder index being listed on a worker, held until [`open_gate`].
    type Gate = Arc<std::sync::Mutex<(bool, Option<std::task::Waker>)>>;

    /// [`sound_app`]'s folder index listed on a worker held at the gate returned.
    fn gated_index(app: &mut App) -> Gate {
        use std::task::Poll;
        let gate: Gate = Arc::default();
        let worker_gate = gate.clone();
        let vfs = app.world().resource::<nvf_assets::GameData>().vfs.clone();
        let task = IoTaskPool::get_or_init(TaskPool::default).spawn(async move {
            std::future::poll_fn(move |cx| {
                let mut g = worker_gate.lock().unwrap();
                if g.0 {
                    Poll::Ready(())
                } else {
                    g.1 = Some(cx.waker().clone());
                    Poll::Pending
                }
            })
            .await;
            folder_index(vfs.paths(is_sound_file))
        });
        app.world_mut().resource_mut::<Sfx>().index = Some(task);
        gate
    }

    fn open_gate(gate: &Gate) {
        let mut g = gate.lock().unwrap();
        g.0 = true;
        if let Some(w) = g.1.take() {
            w.wake();
        }
    }

    #[test]
    fn a_folder_pick_waits_for_the_folder_index() {
        let (mut app, _files) = sound_app();
        let gate = gated_index(&mut app);
        let takes = FormId(0x20);
        app.world_mut().resource_mut::<Sfx>().forms.sounds.insert(takes, record("TestTakes", Some("fx\\takes"), &[]));
        script_plays(&mut app, &[(1, takes)]);
        for _ in 0..3 {
            app.update();
        }
        // Waiting, no file picked, nothing scanned, nothing heard.
        {
            let sfx = app.world().resource::<Sfx>();
            assert_eq!(sfx.waiting.len(), 1);
            assert_eq!(sfx.waiting[0].path, None);
            assert!(sfx.folders.is_empty() && sfx.files.is_empty());
        }
        assert!(heard(&mut app).is_empty());
        open_gate(&gate);
        run_until(&mut app, |w| !w.resource::<ScriptSounds>().heard.is_empty());
        let h = heard(&mut app);
        assert!((h[0].1.unwrap() - 0.5).abs() < 1e-3, "{h:?}");
        let sfx = app.world().resource::<Sfx>();
        assert!(sfx.indexed && sfx.index.is_none());
        assert_eq!(sfx.waits.late, 1);
    }

    /// A background bed of a folder of loops, for the ambient's bed test.
    const FOLDER_BED: FormId = FormId(0x21);

    /// The ambient's bed step, wanting [`FOLDER_BED`] every frame.
    fn bed_wanted(mut commands: Commands, mut amb: ResMut<crate::ambient::Ambient>, mut sfx: ResMut<Sfx>, settings: Res<SfxSettings>, data: Res<nvf_assets::GameData>, mut streams: crate::ambient::Streams) {
        crate::ambient::switch_bed(&mut commands, &mut amb, &mut sfx, &settings, &data.vfs, &mut streams, Some(FOLDER_BED.0), false);
    }

    #[test]
    fn an_ambient_bed_on_a_folder_waits_for_the_folder_index_then_starts() {
        use crate::music::LayeredMusic;
        let (mut app, files) = sound_app();
        // A folder of one loop, a plain 44-byte-header WAV (the streamed loops' decoder
        // wants the RIFF size [`wav`] leaves 0).
        let data = [0u8; 4410];
        let mut bed = b"RIFF".to_vec();
        bed.extend((36 + data.len() as u32).to_le_bytes());
        bed.extend(b"WAVEfmt ");
        bed.extend(16u32.to_le_bytes());
        bed.extend(1u16.to_le_bytes());
        bed.extend(1u16.to_le_bytes());
        bed.extend(22050u32.to_le_bytes());
        bed.extend(44100u32.to_le_bytes());
        bed.extend(2u16.to_le_bytes());
        bed.extend(16u16.to_le_bytes());
        bed.extend(b"data");
        bed.extend((data.len() as u32).to_le_bytes());
        bed.extend(data);
        let beds = files.0.join("sound").join("fx").join("beds");
        std::fs::create_dir_all(&beds).unwrap();
        std::fs::write(beds.join("a.wav"), bed).unwrap();
        let vfs = Arc::new(fnv_vfs::Vfs::open(&files.0, &[]).unwrap());
        app.world_mut().resource_mut::<nvf_assets::GameData>().vfs = vfs;
        let gate = gated_index(&mut app);
        app.world_mut().resource_mut::<Sfx>().forms.sounds.insert(FOLDER_BED, record("TestBed", Some("fx\\beds"), &["Loop"]));
        app.init_resource::<crate::ambient::Ambient>()
            .init_resource::<Assets<LayeredMusic>>()
            .init_resource::<Assets<Panned<LayeredMusic>>>()
            .add_systems(Update, bed_wanted.after(prepare).before(play));
        // While the index is listed the bed is tried each frame, not remembered as
        // having no file.
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(app.world().resource::<crate::ambient::Ambient>().beds_started, 0);
        // Once it is in, the bed starts, once.
        open_gate(&gate);
        run_until(&mut app, |w| w.resource::<crate::ambient::Ambient>().beds_started > 0);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(app.world().resource::<crate::ambient::Ambient>().beds_started, 1);
        assert!(app.world().resource::<Sfx>().indexed);
    }

    #[test]
    fn loop_sound_on_and_off_while_the_file_is_read() {
        let (mut app, _files) = sound_app_with_output();
        let on = |on| LoopSound { sound: BEEP, on };
        // On, off, on, off, then on twice, in one frame: each word drops the start
        // before it; the last one is left. It waits for its file, or has already started:
        // `play` reads the loop words before `start_decided`, and the worker may finish
        // the decode in between (a race, CI on main 2026-11-04).
        app.world_mut().write_message_batch([on(true), on(false), on(true), on(false), on(true), on(true)]);
        app.update();
        assert_eq!(app.world().resource::<Sfx>().waits.cancelled, 3);
        let waiting = app.world().resource::<Sfx>().waiting.len();
        assert_eq!(waiting + count::<Looping>(&mut app), 1, "one start left, waiting or going round");
        // One loop once the file is ready.
        run_until(&mut app, |w| w.resource::<Sfx>().waiting.is_empty());
        assert_eq!(count::<Looping>(&mut app), 1);
        // Attached to the output and heard for a moment.
        play_for(&app, 10);
        // The file ready: on and off in one frame leave nothing going round. The loop
        // that was playing is no longer the loop at once and fades out (#2217); the one
        // the "on" started is not attached yet and goes without a sound.
        app.world_mut().write_message_batch([on(true), on(false)]);
        app.update();
        app.update();
        assert_eq!(count::<Looping>(&mut app), 0);
        assert_eq!((count::<Retiring>(&mut app), count::<SfxSound>(&mut app)), (1, 1), "the old loop fading out, the new one gone");
        let stops = app.world().resource::<SoundStops>().clone();
        assert_eq!((stops.declicked[StopReason::Loop as usize], stops.hard), (1, [0; StopReason::COUNT]));
        play_for(&app, 30);
        app.update();
        assert_eq!(count::<SfxSound>(&mut app), 0, "gone once its fade has played");
        assert_eq!(app.world().resource::<SoundStops>().completed, 1);
    }

    /// [`sound_app`] with the audio output (no device: [`play_for`] plays the sounds on,
    /// as the audio thread would) and the late start.
    fn sound_app_with_output() -> (App, TestFiles) {
        let (mut app, files) = sound_app();
        crate::audio_out::test_output::<PcmSound>(&mut app);
        add_late_start(&mut app);
        (app, files)
    }

    /// The output plays `ms` milliseconds of sound on (48 kHz stereo).
    fn play_for(app: &App, ms: usize) {
        app.world().resource::<crate::audio_out::AudioOut>().drain(96 * ms);
    }

    /// [`BEEP`]'s file decoded, the one-shot that read it played out and gone.
    fn beep_ready(app: &mut App) {
        script_plays(app, &[(1000, BEEP)]);
        run_until(app, |w| !w.resource::<ScriptSounds>().heard.is_empty());
        heard(app);
        play_for(app, 600);
        app.update();
        assert_eq!(count::<OneShot>(app), 0);
    }

    fn stops(app: &App) -> SoundStops {
        app.world().resource::<SoundStops>().clone()
    }

    #[test]
    fn a_steal_waits_for_its_fading_slot() {
        let (mut app, _files) = sound_app_with_output();
        beep_ready(&mut app);
        let beep = PlaySound { sound: BEEP, at: None };
        // Eight of BEEP playing, heard for a moment.
        app.world_mut().write_message_batch(std::iter::repeat_n(beep, MAX_SAME));
        app.update();
        play_for(&app, 10);
        assert_eq!(count::<OneShot>(&mut app), MAX_SAME);
        // A ninth: the oldest fades out and the ninth waits for its slot, no ninth voice.
        app.world_mut().write_message(beep);
        app.update();
        let check_waiting = |app: &mut App| {
            assert_eq!(count::<OneShot>(app), MAX_SAME, "never more than eight of the SOUN");
            assert_eq!(count::<Retiring>(app), 1);
            let sfx = app.world().resource::<Sfx>();
            assert_eq!((sfx.pending.len(), sfx.replacements_waited, sfx.voice_stolen, sfx.replacements_started), (1, 1, 0, 0));
        };
        check_waiting(&mut app);
        // Not played on, or not for long enough: the fade is not over, nothing starts.
        app.update();
        check_waiting(&mut app);
        play_for(&app, 2);
        app.update();
        check_waiting(&mut app);
        // Its fade over, the replacement starts in its slot and the old one goes.
        play_for(&app, 10);
        app.update();
        assert_eq!((count::<OneShot>(&mut app), count::<Retiring>(&mut app)), (MAX_SAME, 0));
        let sfx = app.world().resource::<Sfx>();
        assert!(sfx.pending.is_empty());
        assert_eq!((sfx.voice_stolen, sfx.per_sound[&BEEP][1], sfx.per_sound[&BEEP][2]), (1, 1, 0));
        assert_eq!((sfx.replacements_started, sfx.taken_at_once), (1, 0));
        assert_eq!((sfx.most_same, sfx.most_voices), (MAX_SAME, MAX_SAME));
        let first = stops(&app);
        assert_eq!((first.declicked[StopReason::Steal as usize], first.completed, first.hard), (1, 1, [0; StopReason::COUNT]));
        // All played out; then a burst of sixteen in one frame: the second eight take over
        // the first, which are not attached to the output yet (no sound made), so those
        // go at once, uncounted, and the new ones start in their place without waiting.
        play_for(&app, 600);
        app.update();
        assert_eq!(count::<OneShot>(&mut app), 0);
        app.world_mut().write_message_batch(std::iter::repeat_n(beep, 2 * MAX_SAME));
        app.update();
        assert_eq!(count::<OneShot>(&mut app), MAX_SAME);
        let sfx = app.world().resource::<Sfx>();
        assert!(sfx.pending.is_empty());
        assert_eq!((sfx.voice_stolen, sfx.most_same), (1 + MAX_SAME as u32, MAX_SAME));
        // Those are no replacements: R counts only the ones that waited in a slot.
        assert_eq!((sfx.replacements_started, sfx.taken_at_once, sfx.replacements_waited), (1, MAX_SAME as u32, 1));
        let after = stops(&app);
        assert_eq!((after.declicked, after.hard), (first.declicked, [0; StopReason::COUNT]), "nothing heard was stopped");
    }

    #[test]
    fn fading_voices_count_across_play_and_late_start() {
        let (mut app, _files) = sound_app_with_output();
        beep_ready(&mut app);
        // Others playing to the total limit with BEEP's eight: another SOUN's, two of them
        // fading out (they never end here: not attached).
        for n in 0..(MAX_VOICES - MAX_SAME) as u64 {
            let mut e = app.world_mut().spawn(OneShot(FormId(0x99), 1000 + n, 0));
            if n < 2 {
                e.insert(Retiring);
            }
        }
        let beep = PlaySound { sound: BEEP, at: None };
        app.world_mut().write_message_batch(std::iter::repeat_n(beep, MAX_SAME));
        app.update();
        play_for(&app, 10);
        assert_eq!(count::<OneShot>(&mut app), MAX_VOICES);
        // A burst of BEEP in one frame, and two of a cold file decided late in it: each
        // of BEEP's slots gets one replacement, the rest and the late ones are refused.
        let takes = FormId(0x21);
        app.world_mut().resource_mut::<Sfx>().forms.sounds.insert(takes, record("TestTakeA", Some("fx\\takes\\a.wav"), &[]));
        let limited = app.world().resource::<Sfx>().voice_limited;
        app.world_mut().write_message_batch(std::iter::repeat_n(beep, 20).chain([PlaySound { sound: takes, at: None }; 2]));
        app.world_mut().run_schedule(Update);
        assert_eq!(app.world().resource::<Sfx>().waiting.len(), 2);
        wait_decoded(&app, "sound\\fx\\takes\\a.wav");
        app.world_mut().run_schedule(PostUpdate);
        let sfx = app.world().resource::<Sfx>();
        assert!(sfx.waiting.is_empty());
        assert_eq!((sfx.pending.len(), sfx.replacements_waited), (MAX_SAME, MAX_SAME as u32), "one replacement a slot");
        assert_eq!(sfx.voice_limited - limited, (20 - MAX_SAME as u32) + 2);
        assert_eq!(sfx.voices, MAX_VOICES);
        assert_eq!(count::<OneShot>(&mut app), MAX_VOICES, "the fading voices still count");
        // Their fades over: the replacements start in their slots, within both limits.
        play_for(&app, 10);
        app.update();
        assert_eq!(count::<OneShot>(&mut app), MAX_VOICES);
        let sfx = app.world().resource::<Sfx>();
        assert!(sfx.pending.is_empty());
        assert_eq!((sfx.voice_stolen, sfx.most_voices, sfx.most_same), (MAX_SAME as u32, MAX_VOICES, MAX_SAME));
        assert_eq!(stops(&app).hard, [0; StopReason::COUNT]);
    }

    #[test]
    fn stops_cancel_pending_replacements() {
        let (mut app, _files) = sound_app_with_output();
        beep_ready(&mut app);
        // Eight script sounds of BEEP playing, each answered once.
        script_plays(&mut app, &(1..=MAX_SAME as u32).map(|id| (id, BEEP)).collect::<Vec<_>>());
        app.update();
        play_for(&app, 10);
        let mut h: Vec<u32> = heard(&mut app).iter().map(|x| x.0).collect();
        h.sort();
        assert_eq!(h, (1..=MAX_SAME as u32).collect::<Vec<_>>());
        // A ninth waits for the oldest's slot: no answer yet; the oldest no longer the
        // script's.
        script_plays(&mut app, &[(9, BEEP)]);
        app.update();
        assert!(heard(&mut app).is_empty());
        assert_eq!((app.world().resource::<Sfx>().pending.len(), count::<ScriptPlayed>(&mut app)), (1, MAX_SAME - 1));
        // StopSound BEEP (after the ninth was asked for) and a tenth asked for after the
        // stop, in one frame: every voice fades out, the ninth is dropped, the tenth waits
        // for a fading slot.
        app.world_mut().resource_mut::<ScriptSounds>().stops.push((BEEP, 9));
        script_plays(&mut app, &[(10, BEEP)]);
        app.update();
        assert!(heard(&mut app).is_empty());
        assert_eq!(count::<ScriptPlayed>(&mut app), 0);
        assert_eq!(count::<Retiring>(&mut app), MAX_SAME);
        assert_eq!(app.world().resource::<Sfx>().pending.len(), 1);
        assert_eq!(app.world().resource::<Sfx>().replacements_cancelled, 1, "the ninth, counted as dropped");
        // The fades over: the tenth starts, answered once; the ninth never comes back.
        play_for(&app, 10);
        app.update();
        let h = heard(&mut app);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].0, 10);
        assert!((h[0].1.unwrap() - 0.5).abs() < 1e-3, "{h:?}");
        for _ in 0..3 {
            play_for(&app, 10);
            app.update();
        }
        assert!(heard(&mut app).is_empty());
        assert_eq!((count::<ScriptPlayed>(&mut app), count::<OneShot>(&mut app)), (1, 1));
        let stops = stops(&app);
        assert_eq!((stops.declicked[StopReason::Steal as usize], stops.declicked[StopReason::Script as usize]), (1, MAX_SAME as u32 - 1));
        assert_eq!((stops.completed, stops.hard), (MAX_SAME as u32, [0; StopReason::COUNT]));
        // StopSound in the late start's frame drops a replacement as it drops a request
        // waiting for its file: eight playing again, an eleventh waiting, then a stop
        // asked for after play.
        script_plays(&mut app, &(11..=17).map(|id| (id, BEEP)).collect::<Vec<_>>());
        app.update();
        play_for(&app, 10);
        heard(&mut app);
        script_plays(&mut app, &[(18, BEEP)]);
        app.update();
        assert_eq!(app.world().resource::<Sfx>().pending.len(), 1);
        app.add_systems(Update, (|mut ss: ResMut<ScriptSounds>| ss.stops.push((BEEP, 18))).after(play));
        app.update();
        assert!(app.world().resource::<Sfx>().pending.is_empty(), "dropped by the late start");
        play_for(&app, 10);
        app.update();
        assert!(heard(&mut app).iter().all(|x| x.0 != 18), "the eighteenth is never answered");
        let sfx = app.world().resource::<Sfx>();
        assert_eq!((sfx.replacements_waited, sfx.replacements_started, sfx.replacements_cancelled), (3, 1, 2));
    }

    /// Every one-shot played out and gone, then eight of [`BEEP`] heard for a moment and a
    /// ninth asked for, which waits for the oldest's slot.
    fn a_ninth_waits(app: &mut App) {
        play_for(app, 600);
        app.update();
        assert_eq!(count::<OneShot>(app), 0);
        let beep = PlaySound { sound: BEEP, at: None };
        app.world_mut().write_message_batch(std::iter::repeat_n(beep, MAX_SAME));
        app.update();
        play_for(app, 10);
        app.world_mut().write_message(beep);
        app.update();
        assert_eq!(app.world().resource::<Sfx>().pending.len(), 1);
    }

    #[test]
    fn a_waiting_replacement_keeps_its_pitch() {
        let (mut app, _files) = sound_app_with_output();
        beep_ready(&mut app);
        let shift = FormId(0x30);
        app.world_mut().resource_mut::<Sfx>().forms.sounds.insert(shift, record("TestShift", Some("fx\\beep.wav"), &["Random Frequency Shift"]));
        // Eight of a SOUN with Random Frequency Shift playing; a ninth's pitch is rolled
        // once, as it is asked for, and kept while it waits for its slot.
        let one = PlaySound { sound: shift, at: None };
        app.world_mut().write_message_batch(std::iter::repeat_n(one, MAX_SAME));
        app.update();
        play_for(&app, 10);
        let mut roll = SplitMix(app.world().resource::<Sfx>().rng.0);
        app.world_mut().write_message(one);
        app.update();
        let pitch = (0.95 + 0.1 * roll.next_f32()).clamp(0.25, 4.0);
        let sfx = app.world().resource::<Sfx>();
        assert_eq!(sfx.pending.len(), 1);
        assert_eq!(sfx.pending[0].speed, pitch);
        assert_eq!(sfx.rng.0, roll.0, "one roll, for the ninth");
        // Turned down while it waits: what hangs on the listener (the gain) is worked out
        // again as it starts, the pitch is the one chosen and nothing is rolled again.
        app.world_mut().resource_mut::<SfxSettings>().volume = 0.5;
        play_for(&app, 10);
        app.update();
        let sfx = app.world().resource::<Sfx>();
        assert_eq!((sfx.pending.len(), sfx.replacements_started), (0, 1));
        assert_eq!(sfx.rng.0, roll.0, "no roll as it starts");
        let world = app.world_mut();
        let newest = world.query::<(&OneShot, &PlaybackSettings)>().iter(world).max_by_key(|(o, _)| o.1).map(|(_, p)| *p).unwrap();
        assert_eq!(newest.speed, pitch);
        assert!((newest.volume.to_linear() - 0.5).abs() < 1e-6, "{:?}", newest.volume);
    }

    #[test]
    fn replacement_outcomes_are_counted() {
        let (mut app, _files) = sound_app_with_output();
        beep_ready(&mut app);
        // Counted from here (the cold file's wait before is not this test's).
        app.world_mut().resource_mut::<Sfx>().waits = Waits::default();
        let before = Waits::default();
        // A ninth waiting is not yet a sound begun (counted when it starts or is
        // silenced, not when it takes a slot); its file was ready at once all the same.
        a_ninth_waits(&mut app);
        let waits = |app: &App| app.world().resource::<Sfx>().waits.clone();
        let w = waits(&app);
        assert_eq!((w.at_once - before.at_once, w.late - before.late, w.silent - before.silent), (MAX_SAME as u32, 0, 0));
        assert_eq!((w.files_at_once - before.files_at_once, w.files_late - before.files_late), (MAX_SAME as u32 + 1, 0));
        // Started once its slot has emptied, in the next frame: begun a frame after it was
        // asked for (its slot's wait counted), though its file was ready at once.
        play_for(&app, 10);
        app.update();
        assert_eq!(app.world().resource::<Sfx>().replacements_started, 1);
        let w = waits(&app);
        assert_eq!((w.at_once - before.at_once, w.late - before.late, w.over_one - before.over_one), (MAX_SAME as u32, 1, 0));
        assert_eq!((w.most_frames, w.files_most_frames), (1, 0), "a frame to its start, none for its file");
        // Past the age bound when its slot empties: dropped, counted as expired only.
        a_ninth_waits(&mut app);
        app.world_mut().resource_mut::<Sfx>().one_shot_wait = Duration::ZERO;
        let at = waits(&app);
        play_for(&app, 10);
        app.update();
        app.world_mut().resource_mut::<Sfx>().one_shot_wait = Duration::from_secs(3600);
        assert_eq!(app.world().resource::<Sfx>().replacements_expired, 1);
        assert_eq!(waits(&app), at, "an expired replacement is no file wait");
        // Too quiet when its turn comes: silenced at start, decided silent.
        a_ninth_waits(&mut app);
        app.world_mut().resource_mut::<SfxSettings>().volume = 0.0;
        play_for(&app, 10);
        app.update();
        app.world_mut().resource_mut::<SfxSettings>().volume = 1.0;
        let sfx = app.world().resource::<Sfx>();
        assert_eq!(sfx.replacements_silenced, 1);
        assert_eq!(waits(&app).silent - before.silent, 1);
        // One left waiting at the end; every request that waited is accounted for.
        a_ninth_waits(&mut app);
        let s = app.world().resource::<Sfx>();
        assert_eq!((s.replacements_waited, s.replacements_started, s.replacements_cancelled, s.replacements_expired, s.replacements_silenced, s.pending.len()), (4, 1, 0, 1, 1, 1));
        assert_eq!(s.replacements_waited, s.replacements_started + s.replacements_cancelled + s.replacements_expired + s.replacements_silenced + s.pending.len() as u32);
        assert_eq!(
            s.voices_line(),
            "sfx voices: at most 8 total, 8 of one SOUN; 4 replacements waiting, 1 replacements started, 0 replacements cancelled, 1 replacements expired, 1 replacements silenced at start, 1 replacements still waiting; 0 one-shots started at once over one not heard yet"
        );
    }

    #[test]
    fn a_paused_stopped_loop_can_finish() {
        let (mut app, _files) = sound_app_with_output();
        app.world_mut().write_message(LoopSound { sound: BEEP, on: true });
        run_until(&mut app, |w| w.query_filtered::<(), (With<Looping>, With<AudioSink>)>().iter(w).count() == 1);
        let e = {
            let world = app.world_mut();
            world.query_filtered::<Entity, With<Looping>>().single(world).unwrap()
        };
        // Held (as the ambient's loops are under the pause menu): nothing plays on.
        app.world().get::<AudioSink>(e).unwrap().pause();
        play_for(&app, 20);
        // Stopped while held: it is resumed so its fade can play out, then it goes.
        app.world_mut().write_message(LoopSound { sound: BEEP, on: false });
        app.update();
        assert_eq!(count::<Looping>(&mut app), 0);
        assert!(app.world().get::<Retiring>(e).is_some());
        assert!(!app.world().get::<AudioSink>(e).unwrap().is_paused());
        play_for(&app, 30);
        app.update();
        assert!(app.world().get_entity(e).is_err(), "removed once its fade has played");
        let stops = stops(&app);
        assert_eq!((stops.declicked[StopReason::Loop as usize], stops.completed, stops.hard), (1, 1, [0; StopReason::COUNT]));
    }

    #[test]
    fn broken_missing_and_muted_requests_answer_the_script() {
        let (mut app, _files) = sound_app();
        // No file at all: over at once.
        script_plays(&mut app, &[(1, NO_FILE)]);
        app.update();
        assert_eq!(heard(&mut app), vec![(1, Some(0.0))]);
        // A file that does not decode: over (0 s) once that is known, a loop too (not
        // "going round" before it was tried).
        script_plays(&mut app, &[(2, BROKEN), (3, BROKEN_LOOP)]);
        app.update();
        assert!(heard(&mut app).is_empty());
        run_until(&mut app, |w| w.resource::<ScriptSounds>().heard.len() == 2);
        assert_eq!(heard(&mut app), vec![(2, Some(0.0)), (3, Some(0.0))]);
        assert_eq!(app.world().resource::<Sfx>().waits.broken, 2);
        assert_eq!(count::<SfxSound>(&mut app), 0);
        // Sounds switched off: a one-shot still says its seconds and a loop that it goes
        // round, with nothing playing.
        app.world_mut().resource_mut::<SfxSettings>().play = false;
        script_plays(&mut app, &[(4, BEEP), (5, BEEP_LOOP)]);
        run_until(&mut app, |w| w.resource::<ScriptSounds>().heard.len() == 2);
        let h = heard(&mut app);
        assert_eq!(h[0].0, 4);
        assert!((h[0].1.unwrap() - 0.5).abs() < 1e-3);
        assert_eq!(h[1], (5, None));
        assert_eq!(count::<SfxSound>(&mut app), 0);
        assert_eq!(app.world().resource::<Sfx>().silent, 2);
    }

    #[test]
    fn a_random_marker_skips_its_roll_while_the_file_is_read() {
        use bevy::ecs::system::RunSystemOnce;
        let (mut app, _files) = sound_app();
        let once = |app: &mut App| {
            app.world_mut()
                .run_system_once(|mut commands: Commands, mut sfx: ResMut<Sfx>, mut assets: ResMut<Assets<PcmSound>>, data: Res<nvf_assets::GameData>, settings: Res<SfxSettings>| {
                    collect_files(&mut sfx, &mut assets);
                    let mut out = Out { commands: &mut commands, assets: &mut assets, settings: &settings, player: None };
                    one_shot_heard(&mut out, &mut sfx, &data.vfs, BEEP, None, &mut None)
                })
                .unwrap()
        };
        assert_eq!(once(&mut app), OneShotNow::Waiting);
        // Nothing is queued for it: the marker rolls again later.
        assert!(app.world().resource::<Sfx>().waiting.is_empty());
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let seconds = loop {
            match once(&mut app) {
                OneShotNow::Played(s) => break s,
                r => assert_eq!(r, OneShotNow::Waiting),
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        };
        assert!((seconds - 0.5).abs() < 1e-3);
        assert_eq!(once(&mut app), OneShotNow::Played(seconds));
        // A folder's pick skipped while its file is read is kept for the next roll: one
        // file of the two decoded, the same one played.
        let takes = FormId(0x22);
        app.world_mut().resource_mut::<Sfx>().forms.sounds.insert(takes, record("TestRandomTakes", Some("fx\\takes"), &[]));
        let mut pick = None;
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let before: Option<String> = pick.clone();
            let given = before.clone();
            let now = app.world_mut().run_system_once(move |mut commands: Commands, mut sfx: ResMut<Sfx>, mut assets: ResMut<Assets<PcmSound>>, data: Res<nvf_assets::GameData>, settings: Res<SfxSettings>| {
                collect_files(&mut sfx, &mut assets);
                let mut out = Out { commands: &mut commands, assets: &mut assets, settings: &settings, player: None };
                let mut pick = given.clone();
                (one_shot_heard(&mut out, &mut sfx, &data.vfs, takes, None, &mut pick), pick)
            });
            let (now, after) = now.unwrap();
            if now != OneShotNow::Waiting {
                assert!(after.is_none());
                break;
            }
            assert!(before.is_none() || before == after, "the pick changed: {before:?} {after:?}");
            pick = after;
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(app.world().resource::<Sfx>().files.keys().filter(|p| p.contains("takes")).count(), 1);
    }

    #[test]
    fn a_record_names_a_wav_an_ogg_or_a_folder() {
        let (mut app, files) = sound_app();
        std::fs::write(files.0.join("sound").join("fx").join("only.ogg"), b"OggS").unwrap();
        let vfs = Arc::new(fnv_vfs::Vfs::open(&files.0, &[]).unwrap());
        let mut sfx = app.world_mut().resource_mut::<Sfx>();
        // A .wav that is there; a .wav the archives hold as .ogg; an .ogg named as such
        // (#1428 review: this one had stopped resolving); a folder's take; nothing.
        assert_eq!(pick_file(&mut sfx, &vfs, "fx\\beep.wav").as_deref(), Some("sound\\fx\\beep.wav"));
        assert_eq!(pick_file(&mut sfx, &vfs, "FX/Only.wav").as_deref(), Some("sound\\fx\\only.ogg"));
        assert_eq!(pick_file(&mut sfx, &vfs, "\\fx\\only.ogg").as_deref(), Some("sound\\fx\\only.ogg"));
        let take = pick_file(&mut sfx, &vfs, "fx\\takes").unwrap();
        assert!(["sound\\fx\\takes\\a.wav", "sound\\fx\\takes\\b.wav"].contains(&take.as_str()), "{take}");
        assert_eq!(pick_file(&mut sfx, &vfs, "fx\\empty"), None);
        // A named file that is not there keeps its name (and fails when it is read).
        assert_eq!(pick_file(&mut sfx, &vfs, "fx\\gone.wav").as_deref(), Some("sound\\fx\\gone.wav"));
    }

    #[test]
    fn the_folder_index_lists_what_a_folder_scan_lists() {
        let (mut app, _files) = sound_app();
        let vfs = app.world().resource::<nvf_assets::GameData>().vfs.clone();
        let index = folder_index(vfs.paths(is_sound_file));
        assert_eq!(index["sound\\fx\\takes\\"], list_folder(&vfs, "sound\\fx\\takes\\"));
        assert_eq!(index["sound\\fx\\"], list_folder(&vfs, "sound\\fx\\"));
        assert_eq!(index["sound\\fx\\"], vec!["sound\\fx\\beep.wav".to_string(), "sound\\fx\\broken.wav".into()]);
        assert_eq!(folder_of(&sound_path("fx\\takes")).as_deref(), Some("sound\\fx\\takes\\"));
        assert_eq!(folder_of(&sound_path("fx\\beep.wav")), None);
        // A folder's take is picked from the index once it is in, the same files.
        let mut sfx = app.world_mut().resource_mut::<Sfx>();
        sfx.indexed = true;
        sfx.folders = index;
        let picked = pick_file(&mut sfx, &vfs, "fx\\takes").unwrap();
        assert!(picked.starts_with("sound\\fx\\takes\\"), "{picked}");
        assert_eq!(pick_file(&mut sfx, &vfs, "fx\\nothing"), None);
    }
}
