//! Placed sounds and the cells' background loops (docs/spec-notes/audio.md §5, §6).
//!
//! - **Sound markers** are references whose base is a SOUN. One whose sound has the
//!   Loop flag plays round and round while the player is within the sound's maximum
//!   distance of it, at the loudness [`crate::sfx`] gives a sound at that distance,
//!   followed as the player moves. One whose sound has Play At Random plays a file of
//!   its folder now and then: once a second, when it is not already sounding, it is
//!   rolled against the record's Random Chance [hypothesis: the chance is per second];
//!   a roll won while the file is still being read ([`crate::sfx`]) is skipped.
//!   Either only between the sound's start and stop hours, and only while the reference
//!   is enabled.
//! - **The background loop** of the cell the player is in: its acoustic space (ASPC)
//!   names a looping sound for dawn, afternoon, dusk and night
//!   (`AcousticSpace::loop_at`); it plays at the listener and gives way to the next
//!   over [`BED_FADE`] seconds [design].
//! - **Things that sound while they are there** ([`SoundSource`], sent every frame by
//!   `crate::world_sounds` for activators', containers' and doors' looping sounds and by
//!   nvforever for creatures' conscious and movement loops): played as a sound marker of
//!   that sound at that place [design], following it as it moves.
//! - Loops are decoded as they play (WAV or Ogg Vorbis, [`LayeredMusic`] with one file),
//!   so a long one costs no more memory than its file. Placed loops are panned
//!   ([`crate::sfx::Panned`], as the sound effects are, their gains eased over
//!   [`crate::sfx::PAN_EASE`]), the background loop is not.
//! - The Esc pause menu, a loading screen and the poker table (`crate::poker`) hold the
//!   loops; other menus do not [design].
//! - **SOUN flags** (audio.md §15.2): a 2D sound is not panned; a loop with Start at
//!   Random Position starts somewhere in its file; while the listener is under water a
//!   Mute When Submerged sound is held silent. A [`SoundSource`] may ask for a loop
//!   whatever the flags (a weapon's idle sound).
//! - **Region sounds** (REGN `RDSD`, [`region_sounds`]; docs/spec-notes/default-objects.md
//!   §2): every [`REGION_ROLL`] seconds the sounds of the region the cell's acoustic space
//!   names (and of the cell's own regions) that play in the weather shown are tried in a random order, each
//!   at its chance, and the first that passes plays: at the listener when it is 2D, else
//!   at a point about the listener [design: the interval, one at a time, the place;
//!   hypothesis: the chance is per try].
//! - Not built: sound markers that neither loop nor play at random (played by scripts
//!   or once), Random Location, walla
//!   and region sounds (REGN `RDSD`). Placed loops and random markers play through the
//!   acoustic space's reverb ([`crate::reverb`]); the background loop does not.
//!
//! - **Stopped loops fade out** (#2217) [design]: a placed loop or a source no longer
//!   heard, and every loop of a place left (another worldspace or interior), fade out over
//!   `audio_out::DECLICK_FADE` instead of being cut (`audio_out::stop_sound`, counted as
//!   ambient stops in `sfx declicks: …`); they leave [`Ambient`]'s map at once and are
//!   neither held nor resumed with the loops while they fade. A background loop giving
//!   way keeps its own [`BED_FADE`] and is removed once silent.
//!
//! Switched off with the sound effects (`nvforever --no-sound`). An `ambient summary`
//! line at the end.

use crate::music::{cell_at, game_hour, AudioPlaces, LayeredMusic, Space};
use crate::pause::Paused;
use crate::player::Player;
use crate::sfx::{distance_gain, folder_unlisted, one_shot_heard, pan_gains, resolve, static_gain, OneShotNow, Out, Pan, Panned, PcmSound, Resolved, Sfx, SfxSettings, SoundSource};
use crate::sim::SimState;
use crate::sound_cover::SoundCover;
use crate::sound_grid::FixedGrid;
use crate::{Location, PlayState, PlayWorld};
use bevy::audio::AudioSinkPlayback;
use crate::audio_out::{stop_sound, AddAudioSource, Retiring, StopReason};
use bevy::prelude::*;
use fnv_sim::FormId;
use fnv_world::audio::{self, SoundMarker};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// Seconds one background loop takes to give way to the next.
pub const BED_FADE: f32 = 2.0;

/// A loop stops once the player is this many times its sound's maximum distance away
/// (a little more than where it starts, so standing on the edge does not restart it).
const LEAVE: f32 = 1.05;

/// Whether a sound limited to hours sounds at `hour`: from its start up to its stop,
/// over midnight when the stop is the earlier; equal hours mean all day [observed:
/// 1,150 of the 1,178 placed sounds store 0 and 0].
pub fn in_hours(start: f32, stop: f32, hour: f32) -> bool {
    if (start - stop).abs() < 1e-3 {
        true
    } else if start < stop {
        hour >= start && hour < stop
    } else {
        hour >= start || hour < stop
    }
}

/// A loop being played: its entity, the level it is mixed at and, at a place, its pan.
struct Playing {
    entity: Entity,
    level: Arc<[AtomicU32; 3]>,
    pan: Option<Pan>,
    /// Where a placed loop sounds (`crate::occlusion` reads it).
    at: [f32; 3],
}

impl Playing {
    fn set(&self, gain: f32) {
        self.level[0].store(gain.to_bits(), Ordering::Relaxed);
    }
}

/// A background loop.
struct Bed {
    sound: u32,
    /// `None`: its file could not be played.
    playing: Option<Playing>,
    /// How far it is faded in, 0–1, and its gain when fully in.
    level: f32,
    gain: f32,
}

#[derive(Component)]
struct AmbientSound;

impl Ambient {
    /// The placed loops playing: entity, pan and place (`crate::occlusion`).
    pub fn placed(&self) -> impl Iterator<Item = (Entity, &Pan, [f32; 3])> {
        self.loops.values().filter_map(|p| p.pan.as_ref().map(|pan| (p.entity, pan, p.at)))
    }
}

#[derive(Resource, Default)]
pub struct Ambient {
    space: Option<Space>,
    /// Looping markers that sound, by reference, and sources, by key.
    loops: HashMap<u64, Playing>,
    /// Random markers and sources: the real time until which each still sounds, and the
    /// file a skipped roll picked (still being read), which its next roll plays.
    busy_until: HashMap<u64, f64>,
    picks: HashMap<u64, String>,
    next_roll: f64,
    /// CELL `XCAS` by cell.
    spaces: HashMap<u32, Option<u32>>,
    /// Loop files by archive path; `None`: not there.
    pub files: crate::file_bytes::FileBytes,
    bed: Option<Bed>,
    beds_leaving: Vec<Bed>,
    pub loops_started: u32,
    pub random_played: u32,
    pub beds_started: u32,
    /// Of [`SoundSource`]s: loops started, random sounds played, sources heard.
    pub source_loops_started: u32,
    pub source_random_played: u32,
    /// Loops started at a random place in their file (Start at Random Position).
    pub random_starts: u32,
    /// Region sounds played (REGN `RDSD`).
    pub region_played: u32,
    sources_heard: std::collections::HashSet<u64>,
    /// Why the loops are held now (`None`: they play), for the log.
    held_why: Option<&'static str>,
    /// A scripted film with `muteAudio 0` plays and the loops go on under it.
    under_film: bool,
    /// The grid over each space's sound markers ([`marker_grid`]), the number of sound
    /// forms it was made with, and this frame's candidates (reused).
    marker_grids: HashMap<Space, FixedGrid>,
    grid_sounds: usize,
    near: Vec<u32>,
}

/// A marker or a source in earshot, this frame.
struct Heard {
    key: u64,
    sound: u32,
    at: [f32; 3],
    source: bool,
    /// Goes round whatever its flags say ([`SoundSource::looping`]).
    looping: bool,
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Whether a placed sound whose sound's maximum distance is `max_distance` is out of
/// earshot `dist` away: never heard with no distance, and a loop already `playing` keeps
/// going a little farther ([`LEAVE`]).
fn beyond(max_distance: f32, dist: f32, playing: bool) -> bool {
    max_distance <= 0.0 || dist >= max_distance * if playing { LEAVE } else { 1.0 }
}

/// The grid over a space's sound markers (`crate::sound_grid`) [design]: each at its
/// place, reaching as far as [`beyond`] lets a playing loop be heard (its sound's maximum
/// distance times [`LEAVE`], at least the distance a marker not playing is heard within).
/// A marker whose sound is not in the load order (`max_distance` answers `None`), or
/// whose maximum distance is not above 0, is never heard and left out. The markers stay
/// where the plugins put them (`AudioPlaces` is read once), so the grid is made once per
/// space; a frame asks [`beyond`] of the grid's candidates only, in list order, so the
/// Play At Random rolls are drawn in the same order as over the whole list.
fn marker_grid(markers: &[SoundMarker], max_distance: impl Fn(u32) -> Option<f32>) -> FixedGrid {
    FixedGrid::build(markers.iter().enumerate().filter_map(|(i, m)| {
        let d = max_distance(m.sound)?;
        if d <= 0.0 {
            return None;
        }
        Some((i as u32, m.position, d * LEAVE))
    }))
}

/// Whether `sound`'s file is still coming off the IO pool (asking starts the read): its
/// loop starts on a later frame, and a missing file is not taken for one (#1993).
fn file_pending(amb: &mut Ambient, sfx: &mut Sfx, vfs: &Arc<fnv_vfs::Vfs>, sound: u32) -> bool {
    if folder_unlisted(sfx, FormId(sound)) {
        return false;
    }
    let Some(Resolved { path, .. }) = resolve(sfx, vfs, FormId(sound)) else { return false };
    matches!(amb.files.fetch(&path, vfs, "ambient"), crate::file_bytes::Got::Pending)
}

/// Start a SOUN going round, silent; the record's static gain with it. With `pan` it
/// plays at a place.
#[allow(clippy::too_many_arguments)]
fn start_loop(commands: &mut Commands, amb: &mut Ambient, sfx: &mut Sfx, settings: &SfxSettings, vfs: &Arc<fnv_vfs::Vfs>, assets: &mut Streams, sound: u32, pan: Option<Pan>) -> Option<(Playing, f32)> {
    // A folder of loops not listed yet (sfx's folder index): tried again next frame.
    if folder_unlisted(sfx, FormId(sound)) {
        return None;
    }
    let Resolved { s, name, path } = resolve(sfx, vfs, FormId(sound))?;
    // Read off the main thread (#1993): `file_pending` holds the start back until it is in.
    let crate::file_bytes::Got::Ready(bytes) = amb.files.fetch(&path, vfs, "ambient") else { return None };
    let bytes = bytes?;
    let Some((music, level)) = LayeredMusic::single(bytes, true) else {
        warn!("ambient: {name}: {path}: not decoded");
        amb.files.mark_broken(path);
        return None;
    };
    // Start at Random Position: somewhere in the file (audio.md §15.2).
    let music = if s.has("Start at Random Position") {
        amb.random_starts += 1;
        music.starting_at(sfx.rng.next_f32())
    } else {
        music
    };
    debug!("ambient: {name} {path} loops");
    let speed = (1.0 + s.frequency_adjust_percent / 100.0).clamp(0.25, 4.0);
    let playback = PlaybackSettings::DESPAWN.with_speed(speed);
    let entity = match &pan {
        Some(pan) => {
            // A placed loop sounds in the room it is heard in (crate::reverb).
            let reverb = crate::reverb::for_sound(sfx.room_reverb, settings, &s);
            sfx.reverbed += reverb.is_some() as u32;
            sfx.reverb_send_sum += reverb.map_or(0.0, |r| r.send);
            commands.spawn((AudioPlayer::<Panned<LayeredMusic>>(assets.1.add(Panned { inner: music, pan: pan.clone(), reverb, voice_filter: false })), playback, AmbientSound)).id()
        }
        None => commands.spawn((AudioPlayer::<LayeredMusic>(assets.0.add(music)), playback, AmbientSound)).id(),
    };
    Some((Playing { entity, level, pan, at: [0.0; 3] }, static_gain(&s)))
}

/// The loops' stream assets: as they are (background loops) and panned (placed ones).
pub(crate) type Streams<'a> = (ResMut<'a, Assets<LayeredMusic>>, ResMut<'a, Assets<Panned<LayeredMusic>>>);

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn play(
    mut commands: Commands,
    time: Res<Time<Real>>,
    settings: Res<SfxSettings>,
    mut amb: ResMut<Ambient>,
    places: Option<Res<AudioPlaces>>,
    sfx: Option<ResMut<Sfx>>,
    (d, loc, sim): (Res<PlayWorld>, Res<Location>, Res<SimState>),
    (state, paused, poker): (Res<State<PlayState>>, Res<Paused>, Option<Res<crate::poker::PokerScreen>>),
    (player, intro): (Option<Res<Player>>, Option<Res<crate::intro::Intro>>),
    data: Option<Res<nvf_assets::GameData>>,
    mut pcm: ResMut<Assets<PcmSound>>,
    mut streams: Streams,
    mut sources: MessageReader<SoundSource>,
    // Not the loops fading out after a stop: they play their fade out (audio_out).
    sinks: Query<&AudioSink, (With<AmbientSound>, Without<Retiring>)>,
    mut cover: Option<ResMut<SoundCover>>,
) {
    let cover_now = std::time::Instant::now();
    let (Some(places), Some(mut sfx), Some(player), Some(data)) = (places, sfx, player, data) else {
        sources.clear();
        return;
    };
    let (amb, sfx) = (&mut *amb, &mut *sfx);
    // The poker table holds them too: it plays its casino's sounds (`crate::poker`). A
    // film holds them unless its PlayBink said `muteAudio 0` (`crate::intro`).
    let film_keeps = intro.as_deref().and_then(|i| i.film_keeps_world_sound());
    let held_why = if *state.get() == PlayState::Loading {
        Some("loading")
    } else if poker.is_some_and(|p| p.0.is_some()) {
        Some("the poker table")
    } else if crate::intro::world_sound_held(paused.0, film_keeps) {
        Some(if film_keeps.is_some() { "a film, PlayBink muteAudio 1" } else { "the game is held" })
    } else {
        None
    };
    if amb.held_why != held_why {
        match held_why {
            Some(why) => info!("ambient: world sounds held ({why}, {} loops)", amb.loops.len()),
            None => info!("ambient: world sounds go on ({} loops)", amb.loops.len()),
        }
        amb.held_why = held_why;
    }
    let under_film = film_keeps == Some(true) && held_why.is_none();
    if under_film != amb.under_film {
        if under_film {
            info!("ambient: world sounds go on under the film (PlayBink muteAudio 0, {} loops)", amb.loops.len());
        }
        amb.under_film = under_film;
    }
    let held = held_why.is_some();
    let now = time.elapsed_secs_f64();
    let eye = player.eye();
    let hour = game_hour(&sim);
    let volume = settings.volume.max(0.0);
    let space = Space::of(&loc);
    if amb.space != Some(space) {
        // Another worldspace or interior: nothing of the last one is heard here.
        for (_, p) in amb.loops.drain() {
            stop_sound(&mut commands, p.entity, StopReason::Ambient);
        }
        amb.busy_until.clear();
        amb.picks.clear();
        amb.space = Some(space);
    }
    let roll = !held && now >= amb.next_roll;
    if roll {
        amb.next_roll = now + 1.0;
    }
    // The markers of this place that can be in earshot (`marker_grid`) and are enabled,
    // then the sources sent this frame (the last word for each key).
    let markers = if settings.play { places.sounds.get(&space).map_or(&[][..], Vec::as_slice) } else { &[] };
    if places.is_changed() || amb.grid_sounds != sfx.forms.sounds.len() {
        amb.marker_grids.clear();
        amb.grid_sounds = sfx.forms.sounds.len();
    }
    if markers.is_empty() {
        amb.near.clear();
    } else {
        let grid = amb.marker_grids.entry(space).or_insert_with(|| marker_grid(markers, |s| sfx.forms.sounds.get(&FormId(s)).map(|r| r.max_distance)));
        grid.near(eye, |c, r| distance(c, eye) >= r, &mut amb.near);
    }
    let mut heard: Vec<Heard> = amb
        .near
        .iter()
        .map(|&i| &markers[i as usize])
        .filter(|m| sim.world.is_enabled(FormId(m.ref_id)).unwrap_or(!m.initially_disabled))
        .map(|m| Heard { key: u64::from(m.ref_id), sound: m.sound, at: m.position, source: false, looping: false })
        .collect();
    let mut sent: HashMap<u64, Heard> = HashMap::new();
    for s in sources.read() {
        sent.insert(s.key, Heard { key: s.key, sound: s.sound.0, at: s.at, source: true, looping: s.looping });
    }
    if settings.play {
        let mut sent: Vec<Heard> = sent.into_values().collect();
        sent.sort_by_key(|h| h.key);
        heard.extend(sent);
    }
    let mut sounding: Vec<u64> = Vec::new();
    for m in heard {
        let Some(s) = sfx.forms.sounds.get(&FormId(m.sound)) else { continue };
        let dist = distance(m.at, eye);
        let playing = amb.loops.contains_key(&m.key);
        if beyond(s.max_distance, dist, playing) {
            continue;
        }
        if !in_hours(s.start_hour, s.stop_hour, hour) {
            continue;
        }
        if m.source && amb.sources_heard.insert(m.key) {
            debug!("ambient: source {:X} sounds {} at {:?}", m.key, s.edid.as_deref().unwrap_or("?"), m.at);
        }
        if s.has("Loop") || m.looping {
            // Under water a sound so flagged is held silent (audio.md §15.2).
            let muted = sfx.submerged && s.has("Mute When Submerged");
            let gain = if muted { 0.0 } else { static_gain(s) * distance_gain(s, dist) * volume };
            // Heard at the listener while it sounds (`crate::sound_cover`), held loops not.
            if let (false, false, Some(cover)) = (muted, held, cover.as_deref_mut()) {
                cover.note(Some(SoundCover::key(crate::sound_cover::AMBIENT_KIND, m.key)), static_gain(s) * distance_gain(s, dist), dist, crate::sound_cover::LOOP_HOLD, cover_now);
            }
            sounding.push(m.key);
            if let Some(p) = amb.loops.get_mut(&m.key) {
                p.at = m.at;
                p.set(gain);
                if let Some(pan) = &p.pan {
                    pan.set(pan_gains(eye, player.heading, m.at));
                }
            } else if !held {
                // A 2D sound is not panned (audio.md §15.2).
                let pan = (!s.has("2D")).then(|| Pan::new(pan_gains(eye, player.heading, m.at)));
                if file_pending(amb, sfx, &data.vfs, m.sound) {
                    continue;
                }
                let Some((mut p, _)) = start_loop(&mut commands, amb, sfx, &settings, &data.vfs, &mut streams, m.sound, pan) else { continue };
                p.at = m.at;
                p.set(gain);
                amb.loops.insert(m.key, p);
                if m.source {
                    amb.source_loops_started += 1;
                } else {
                    amb.loops_started += 1;
                }
            }
        } else if s.has("Play At Random") && roll && amb.busy_until.get(&m.key).is_none_or(|&t| now >= t) {
            let chance = s.random_chance_percent / 100.0;
            if sfx.rng.next_f32() < chance {
                let mut out = Out { commands: &mut commands, assets: &mut pcm, settings: &settings, player: Some(&*player) };
                let mut pick = amb.picks.remove(&m.key);
                let now_heard = one_shot_heard(&mut out, sfx, &data.vfs, FormId(m.sound), Some(m.at), &mut pick);
                if let Some(p) = pick {
                    amb.picks.insert(m.key, p);
                }
                match now_heard {
                    OneShotNow::Played(seconds) => {
                        amb.busy_until.insert(m.key, now + seconds as f64);
                        if m.source {
                            amb.source_random_played += 1;
                        } else {
                            amb.random_played += 1;
                        }
                    }
                    // Its file is being read (sfx, #1428): this roll is skipped and the
                    // marker stays quiet until the file is ready [engineering choice:
                    // the plays are at random anyway, so a skipped roll is not heard as
                    // a delay, where a late start could be]. The file picked is kept
                    // (`picks`) for the marker's next roll.
                    OneShotNow::Waiting => sfx.waits.rolls_skipped += 1,
                    OneShotNow::NoFile => {}
                }
            }
        }
    }
    amb.loops.retain(|r, p| {
        let keep = sounding.contains(r);
        if !keep {
            stop_sound(&mut commands, p.entity, StopReason::Ambient);
        }
        keep
    });

    // The background loop of the cell.
    let wanted = settings.play.then(|| cell_at(&d, &loc, eye)).flatten().and_then(|c| {
        let space = (*amb.spaces.entry(c).or_insert_with(|| audio::cell_audio(d.lo, d.schema, c).acoustic_space))?;
        places.forms.spaces.get(&space)?.loop_at(hour)
    });
    switch_bed(&mut commands, amb, sfx, &settings, &data.vfs, &mut streams, wanted, held);
    let step = time.delta_secs() / BED_FADE;
    if let Some(b) = amb.bed.as_mut() {
        b.level = (b.level + step).min(1.0);
    }
    for b in amb.beds_leaving.iter_mut() {
        b.level -= step;
    }
    for b in amb.bed.iter().chain(amb.beds_leaving.iter()) {
        if let Some(p) = &b.playing {
            p.set(b.level.max(0.0) * b.gain * volume);
        }
    }
    amb.beds_leaving.retain(|b| {
        if let (true, Some(p)) = (b.level <= 0.0, &b.playing) {
            commands.entity(p.entity).try_despawn();
        }
        b.level > 0.0
    });

    for sink in &sinks {
        match (held, sink.is_paused()) {
            (true, false) => sink.pause(),
            (false, true) => sink.play(),
            _ => {}
        }
    }
}

/// The background loop of the cell becomes `wanted` (`None`: none), the last one fading
/// out, unless the loops are `held`. A bed naming a folder waits for sfx's folder index,
/// tried again each frame (it would be remembered as having no file otherwise).
#[allow(clippy::too_many_arguments)]
pub(crate) fn switch_bed(commands: &mut Commands, amb: &mut Ambient, sfx: &mut Sfx, settings: &SfxSettings, vfs: &Arc<fnv_vfs::Vfs>, streams: &mut Streams, wanted: Option<u32>, held: bool) {
    // A bed whose file is still being read waits the same way (#1993).
    let listing = wanted.is_some_and(|s| folder_unlisted(sfx, FormId(s)) || file_pending(amb, sfx, vfs, s));
    if amb.bed.as_ref().map(|b| b.sound) != wanted && !held && !listing {
        if let Some(old) = amb.bed.take() {
            amb.beds_leaving.push(old);
        }
        if let Some(sound) = wanted {
            let started = start_loop(commands, amb, sfx, settings, vfs, streams, sound, None);
            amb.beds_started += started.is_some() as u32;
            // A sound without a file is remembered too, so it is not asked for again.
            let gain = started.as_ref().map_or(0.0, |s| s.1);
            amb.bed = Some(Bed { sound, playing: started.map(|s| s.0), level: 0.0, gain });
        }
    }
}

/// Seconds between two tries of the regions' sounds [design].
pub const REGION_ROLL: f64 = 10.0;

/// What [`region_sounds`] keeps: when it last tried, the regions of each cell, the
/// classification of each weather, its own random numbers.
#[derive(Default)]
struct RegionRolls {
    next: f64,
    cells: HashMap<u32, Vec<u32>>,
    weathers: HashMap<u32, u8>,
    rng: Option<fnv_combat::rng::SplitMix>,
}

/// The regions' sounds (see the module notes).
#[allow(clippy::too_many_arguments)]
fn region_sounds(
    time: Res<Time<Real>>,
    mut amb: ResMut<Ambient>,
    places: Option<Res<AudioPlaces>>,
    (d, loc): (Res<PlayWorld>, Res<Location>),
    player: Option<Res<Player>>,
    script: Option<Res<crate::script::ScriptState>>,
    (sfx, settings): (Option<Res<Sfx>>, Res<SfxSettings>),
    mut st: Local<RegionRolls>,
    mut out: MessageWriter<crate::sfx::PlaySound>,
) {
    let (Some(places), Some(player), Some(sfx)) = (places, player, sfx) else { return };
    let now = time.elapsed_secs_f64();
    if now < st.next || amb.held_why.is_some() || !settings.play || places.forms.region_sounds.is_empty() {
        return;
    }
    st.next = now + REGION_ROLL;
    let eye = player.eye();
    let Some(cell) = cell_at(&d, &loc, eye) else { return };
    let st = &mut *st;
    let regions = st
        .cells
        .entry(cell)
        .or_insert_with(|| {
            // The region the cell's acoustic space names (the `Audio…` regions hold the
            // sounds), then the cell's own regions (audio.md §6).
            let space = audio::cell_audio(d.lo, d.schema, cell).acoustic_space.and_then(|s| places.forms.spaces.get(&s)?.region);
            let all: Vec<u32> = space.into_iter().chain(audio::cell_regions(d.lo, d.schema, cell)).collect();
            let mut with: Vec<u32> = all.iter().copied().filter(|r| places.forms.region_sounds.contains_key(r)).collect();
            with.dedup();
            debug!("ambient: cell {cell:08X}: regions {:08X?}, with sounds {:08X?}", all, with);
            with
        })
        .clone();
    if regions.is_empty() {
        return;
    }
    // The weather shown, as the game was told (`fnv_game::weather`); none known: any.
    let shown = script.as_deref().and_then(|s| s.rt.as_ref()).and_then(|rt| rt.game.weather.shown);
    let class = shown.map_or(0xF, |w| *st.weathers.entry(w.0).or_insert_with(|| fnv_world::sky::weather(d.lo, d.schema, w.0).map_or(0xF, |x| x.classification)));
    let rng = st.rng.get_or_insert(fnv_combat::rng::SplitMix(0x5EED_4E61));
    let mut tries: Vec<audio::RegionSound> = regions.iter().flat_map(|r| places.forms.region_sounds[r].iter().copied()).filter(|s| s.plays_in(class)).collect();
    // A random order (Fisher-Yates).
    for i in (1..tries.len()).rev() {
        let j = ((rng.next_f32() * (i + 1) as f32) as usize).min(i);
        tries.swap(i, j);
    }
    for s in tries {
        if rng.next_f32() * 100.0 >= s.chance {
            continue;
        }
        let rec = sfx.forms.sounds.get(&FormId(s.sound));
        let at = rec.filter(|r| !r.has("2D")).map(|r| {
            let a = rng.next_f32() * std::f32::consts::TAU;
            let d = r.min_distance.max(100.0) + rng.next_f32() * (r.max_distance * 0.5 - r.min_distance).max(0.0);
            [eye[0] + a.cos() * d, eye[1] + a.sin() * d, eye[2]]
        });
        debug!("ambient: region sound {} ({:.0}%){}", rec.and_then(|r| r.edid.as_deref()).unwrap_or("?"), s.chance, if at.is_some() { " about the listener" } else { " at the listener" });
        out.write(crate::sfx::PlaySound { sound: FormId(s.sound), at });
        amb.region_played += 1;
        break;
    }
}

fn report(mut exit: MessageReader<AppExit>, amb: Res<Ambient>) {
    if exit.read().last().is_some() {
        info!(
            "ambient summary: {} placed loops started, {} random sounds played, {} background loops started; things that sound (objects, creatures, water, weapons, effects): {} heard, {} loops started, {} random sounds played; {} loops started at a random position; {} region sounds played",
            amb.loops_started,
            amb.random_played,
            amb.beds_started,
            amb.sources_heard.len(),
            amb.source_loops_started,
            amb.source_random_played,
            amb.random_starts,
            amb.region_played
        );
    }
}

pub struct AmbientPlugin;

/// Let go of the ambient's files nothing plays ([`crate::file_bytes`]).
fn release_files(real: Res<Time<Real>>, limits: Option<Res<nvf_assets::KeepLimits>>, x: Option<ResMut<Ambient>>) {
    if let Some(mut x) = x {
        x.files.sweep(real.elapsed_secs_f64(), crate::file_bytes::limits(limits));
    }
}

impl Plugin for AmbientPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Ambient>()
            .add_audio_source::<Panned<LayeredMusic>>()
            // After sfx's prepare (the decodes finished, the voices playing, the
            // listener under water) and before its play, which plays the region sounds
            // asked for here in the same frame.
            .add_systems(Update, (play, region_sounds).chain().in_set(crate::sfx::SfxStep::Ambient))
            .add_systems(Last, (release_files, report));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The markers the grid hands out, each asked as the loop asks it, are the ones the
    /// whole list gives, in the same order (the Play At Random rolls are drawn in it):
    /// markers across cell borders, on the edge of their reach, with sounds of no distance,
    /// a negative or NaN one, one not in the load order, and disabled ones; loops playing
    /// and not.
    #[test]
    fn marker_grid_keeps_every_marker_in_earshot_in_list_order() {
        use crate::sound_grid::tests::Lcg;
        fn max_distance(sound: u32) -> Option<f32> {
            match sound {
                0 => Some(500.0),
                1 => Some(4096.0 / LEAVE),
                2 => Some(3000.0),
                3 => Some(0.0),
                4 => Some(-200.0),
                5 => Some(f32::NAN),
                6 => Some(9000.0),
                _ => None,
            }
        }
        let mut rng = Lcg(5);
        let markers: Vec<SoundMarker> = (0..1200u32)
            .map(|i| {
                let mut position = [rng.range(-30000.0, 30000.0), rng.range(-30000.0, 30000.0), rng.range(-1000.0, 3000.0)];
                if i % 6 == 0 {
                    position[0] = 4096.0 * (rng.next_u32() % 11) as f32 - 20480.0;
                }
                SoundMarker { ref_id: 0x2000 + i, cell: 1, position, sound: i % 8, initially_disabled: i % 9 == 0, enable_parent: None }
            })
            .collect();
        let grid = marker_grid(&markers, max_distance);
        let mut eyes: Vec<[f32; 3]> = (0..150).map(|_| [rng.range(-32000.0, 32000.0), rng.range(-32000.0, 32000.0), rng.range(0.0, 2000.0)]).collect();
        eyes.extend([[0.0; 3], [4096.0, 0.0, 0.0], [f32::NAN, 0.0, 0.0]]);
        for m in markers.iter().filter(|m| m.sound < 3).take(40) {
            let d = max_distance(m.sound).unwrap();
            eyes.push([m.position[0] + d, m.position[1], m.position[2]]);
            eyes.push([m.position[0] - d * LEAVE, m.position[1], m.position[2]]);
        }
        let mut near = Vec::new();
        let mut heard_any = 0;
        for eye in eyes {
            for playing in [false, true] {
                let heard = |m: &&SoundMarker| !m.initially_disabled && max_distance(m.sound).is_some_and(|d| !beyond(d, distance(m.position, eye), playing));
                let whole: Vec<u32> = markers.iter().filter(heard).map(|m| m.ref_id).collect();
                grid.near(eye, |c, r| distance(c, eye) >= r, &mut near);
                let indexed: Vec<u32> = near.iter().map(|&i| &markers[i as usize]).filter(heard).map(|m| m.ref_id).collect();
                assert_eq!(indexed, whole, "listener at {eye:?}, playing {playing}");
                if eye[0].is_finite() {
                    assert!(near.len() < markers.len() / 4, "the grid narrows the candidates at {eye:?}");
                }
                heard_any += whole.len();
            }
        }
        assert!(heard_any > 0);
    }

    #[test]
    fn hours_of_a_sound() {
        // No hours: all day.
        assert!(in_hours(0.0, 0.0, 3.0) && in_hours(20.06, 20.06, 12.0));
        // Night sounds, over midnight.
        assert!(in_hours(21.0, 6.0, 23.0) && in_hours(21.0, 6.0, 2.0));
        assert!(!in_hours(21.0, 6.0, 6.0) && !in_hours(21.0, 6.0, 12.0));
        // Day sounds.
        assert!(in_hours(8.25, 17.34, 12.0) && !in_hours(8.25, 17.34, 18.0) && !in_hours(8.25, 17.34, 8.0));
    }
}
