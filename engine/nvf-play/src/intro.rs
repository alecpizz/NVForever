//! Films: the intro film before a new game, and the films scripts play (PlayBink).
//!
//! **The intro** (`Video\FNVIntro.bik`, the owner's choice of film, 2026-10-15, or the
//! INI's `sIntroMovie`, none when that is empty: `crate::game_start`): chosen New Game on
//! the main menu (`crate::main_menu`) plays it, then the opening begins. Any key or a
//! click skips it. The game stays held under it as under the main menu
//! (`crate::pause::Paused`).
//!
//! **PlayBink** (`fnv_game::functions::misc::play_bink`, docs/spec-notes/bink-films.md):
//! a film a script asks for plays once the player is in the world and no menu, loading
//! screen or other film is up, full screen over everything, with its sound. The game is
//! held as under the pause menu while it plays (`Paused`, `PlayState::Menu`): scripts,
//! the clock and the world's sounds stop [documented: GECK wiki PlayBink, playing a film
//! "appears to stop game time"]; afterwards the game goes on where it was. Esc ends it
//! when the call made it interruptible [documented: the page, "the user can interrupt
//! the Bink by hitting the escape key"]; the music pauses under it when asked
//! (`pauseMusic`, [`Intro::silences_music`]); a letterboxed film is fitted inside the
//! window with black around it, else stretched over the whole window [hypothesis: the
//! page only says letterboxing is skipped]. `muteAudio 0` keeps the world's sounds going
//! under the film while the game stays held [documented: the page, "the game's world
//! sounds will continue to play"]: the placed and background loops (`crate::ambient`)
//! and the world's radios (`crate::radio`) are not paused
//! ([`Intro::film_keeps_world_sound`]); lines being said stay held with the scripts
//! that time them [hypothesis]. The file must be loose under
//! `Data` [documented: the page, "can't be packaged into a BSA File"]: one that is not
//! there is skipped and the game goes on at once (the opening's `fnvlogo1.bik`,
//! `fnvlogo2.bik` and `vb01.bik` are not in the owner's `Data\Video`, which holds only
//! `FNVIntro.bik` [observed 2026-10-18]). The opening's own call for the intro film
//! (`QUST 0x00102037` stage 0, [observed: decoded scripts]) is not played again once
//! the main menu has shown it in this run. Unattended runs (a test aid set) do not play
//! scripted films unless `NVF_PLAY_TEST_FILMS=1`, so the opening's call does not hold
//! every smoke run for the intro's three minutes.
//!
//! The file is Bink video 1 (`BIKi`), 1280×720 at 30 frames a second with 48 kHz stereo
//! sound [observed: its header and a decode of its first 300 frames, 2026-10-15]; the
//! original's INI names another film for its start (`sIntroMovie=Fallout INTRO Vsk.bik`,
//! not in the owner's `Data\Video`) [documented: Fallout_default.ini]. It is decoded by the
//! `infinitier_bik_decoder` crate (docs/design/dependencies.md) on a thread of its own,
//! read straight from the loose file (it is 353 MB), a few frames ahead; each frame is
//! shown when its time has come by the wall clock since the film started (late frames are
//! dropped), the sound plays from a queue the decoder fills. The picture is fitted inside
//! the window, black around it. A film skipped stops its decoder and fades its sound
//! out over the sound it has queued (`crate::audio_out::stop_sound`, #2217: not cut
//! mid-waveform; counted as a film stop in `sfx declicks: …`), silence once that runs
//! out, then the sound ends; a film played to its end lets its sound play out what it
//! has queued and end on its own.
//!
//! Built with the cargo feature `intro-video`, on by default (its decoder is
//! GPL-3.0-or-later, as NVForever is: owner decision 2026-10-15). A build without it
//! (`--no-default-features`) goes straight to the opening (`intro: not built in`) and
//! skips scripted films (`film: … not built in`).
//!
//! Log: `intro: playing <file> (<w>x<h>, <n> frames)`, `intro: skipped at <s> s` or
//! `intro: ended after <s> s`, `intro: <file>: <why>` when it cannot play (the game then
//! starts at once). Scripted films: `film: playing <path> (<w>x<h>, <n> frames, PlayBink
//! …)`, `film: skipped at <s> s` / `film: ended after <s> s`, `film: the game goes on`,
//! `film: <path>: <why>; the game goes on` when it cannot play, `film: <path>: …not
//! played…` when it is passed over. Test aids: `NVF_PLAY_TEST_INTRO_SKIP=<seconds>` skips
//! every film after that long; `NVF_PLAY_TEST_INTRO_SHOT=<seconds>` saves a screenshot
//! of the first film at that time, `intro.png` (the intro) or `film.png` (a scripted
//! one), in the working folder; `NVF_PLAY_TEST_FILMS=1` plays scripted films in an
//! unattended run.

use crate::pause::Paused;
use crate::{PlaySet, PlayState};
#[cfg(feature = "intro-video")]
use bevy::asset::RenderAssetUsages;
use bevy::audio::{ChannelCount, Decodable, Sample, SampleRate, Source};
use crate::audio_out::{stop_sound, AddAudioSource, StopReason};
use bevy::prelude::*;
#[cfg(feature = "intro-video")]
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::PrimaryWindow;
use fnv_game::functions::ui::FilmRequest;
#[cfg(feature = "intro-video")]
use infinitier_bik_decoder::{BikOutputFormat, BikPixels, BikStreamingDecoder};
#[cfg(feature = "intro-video")]
use nvf_assets::GameData;
use std::collections::VecDeque;
use std::num::NonZero;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(feature = "intro-video")]
use std::sync::mpsc::sync_channel;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The film unless the INI names another [owner, 2026-10-15] (`crate::game_start`).
pub const FILM: &str = crate::game_start::DEFAULT_INTRO;
/// Frames decoded ahead of the one shown.
#[cfg(feature = "intro-video")]
const AHEAD: usize = 8;

/// Play the intro, then start the game (sent by the main menu's New Game).
#[derive(Message, Debug, Clone, Copy)]
pub struct PlayIntro;

/// What the decoder thread shares: the sound queue, its format, and stop / done flags.
struct Shared {
    sound: Mutex<VecDeque<i16>>,
    format: Mutex<Option<(u8, u32)>>,
    stop: AtomicBool,
    done: AtomicBool,
}

/// A frame: its number and its pixels (RGBA).
type Frame = (u32, Vec<u8>);

struct Playing {
    frames: Mutex<Receiver<Frame>>,
    shared: Arc<Shared>,
    image: Handle<Image>,
    size: Vec2,
    frame_secs: f64,
    /// A valid decoded frame has reached the live image; Android cover admission.
    frame_uploaded: bool,
    /// Real time the first frame was shown.
    started: Option<f64>,
    /// The next frame waiting for its time.
    waiting: Option<Frame>,
    sound: Option<Entity>,
    root: Entity,
    /// The intro, or the script's call.
    film: Film,
}

/// Which film plays.
#[derive(Debug, Clone, PartialEq)]
enum Film {
    Intro,
    Script(FilmRequest),
}

impl Film {
    /// Esc ends a scripted film only when the call made it interruptible; any key or
    /// click skips the intro.
    fn skipped_by(&self, keys: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>) -> bool {
        match self {
            Film::Intro => keys.get_just_pressed().next().is_some() || mouse.get_just_pressed().next().is_some(),
            Film::Script(f) => f.interruptible && keys.just_pressed(KeyCode::Escape),
        }
    }

    fn letterboxed(&self) -> bool {
        match self {
            Film::Intro => true,
            Film::Script(f) => f.letterboxed,
        }
    }
}

#[derive(Resource, Default)]
pub struct Intro {
    playing: Option<Playing>,
    skip_after: Option<f32>,
    shot_at: Option<f32>,
    /// Films scripts asked for, waiting for the world.
    queue: VecDeque<FilmRequest>,
    /// The intro film New Game plays (`crate::game_start`, the INI's `sIntroMovie`);
    /// `None`: none.
    film: Option<String>,
    /// The intro film has been shown (played or skipped) in this run.
    intro_shown: bool,
    /// Scripted films are played (not in an unattended run without `NVF_PLAY_TEST_FILMS`).
    script_films: bool,
}

impl Intro {
    pub fn frame_ready(&self) -> bool {
        self.playing.as_ref().is_some_and(|p| p.frame_uploaded)
    }

    pub fn is_playing(&self) -> bool {
        self.playing.is_some()
    }

    /// A script's film (PlayBink) is playing, not the intro before a new game.
    pub fn playing_script_film(&self) -> bool {
        self.playing.as_ref().is_some_and(|p| matches!(p.film, Film::Script(_)))
    }

    /// The music is silent: under the intro, and under a scripted film that pauses it.
    pub fn silences_music(&self) -> bool {
        self.playing.as_ref().is_some_and(|p| match &p.film {
            Film::Intro => true,
            Film::Script(f) => f.pause_music,
        })
    }

    /// While a script's film plays: whether the world's sounds go on under it (`muteAudio
    /// 0`, `Some(true)`) or are held (`muteAudio 1`, the default, `Some(false)`); `None`
    /// when no scripted film plays (the intro before a new game holds them as the pause
    /// menu does). The game itself is held either way [documented: GECK wiki PlayBink,
    /// "appears to stop game time, even if the flags are set to keep ingame noises
    /// going"].
    pub fn film_keeps_world_sound(&self) -> Option<bool> {
        self.playing.as_ref().and_then(|p| match &p.film {
            Film::Intro => None,
            Film::Script(f) => Some(!f.mute_audio),
        })
    }

    /// `path` is the intro film's.
    fn is_intro(&self, path: &str) -> bool {
        self.film.as_deref().is_some_and(|f| f.eq_ignore_ascii_case(path))
    }

    /// A script's film: queued to play, or passed over (the intro film shown already,
    /// or an unattended run). Returns whether it was queued.
    fn ask(&mut self, film: FilmRequest) -> bool {
        if self.is_intro(&film.path) && self.intro_shown {
            info!("film: {}: the intro film, shown before the opening already; not played again", film.path);
            return false;
        }
        if !self.script_films {
            info!("film: {}: not played in an unattended run (test aid NVF_PLAY_TEST_FILMS=1 plays it)", film.path);
            return false;
        }
        self.queue.push_back(film);
        true
    }
}

/// The film's sound, played from the queue the decoder fills.
#[derive(Asset, TypePath, Clone)]
pub struct IntroSound {
    shared: Arc<Shared>,
    channels: ChannelCount,
    rate: SampleRate,
}

pub struct IntroDecoder(IntroSound);

impl Iterator for IntroDecoder {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        let s = self.0.shared.sound.lock().ok()?.pop_front();
        match s {
            Some(v) => Some(v as f32 / 32768.0),
            // Skipped: silence once the queue is played, until the sound's fade out ends it
            // (`audio_out::stop_sound`; the decoder thread sets `done` as it stops, so the
            // stop is read first). Played to its end: the end once the queue is empty.
            // Waiting for the decoder: silence.
            None if self.0.shared.stop.load(Ordering::Relaxed) => Some(0.0),
            None if self.0.shared.done.load(Ordering::Relaxed) => None,
            None => Some(0.0),
        }
    }
}

impl Source for IntroDecoder {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> ChannelCount {
        self.0.channels
    }
    fn sample_rate(&self) -> SampleRate {
        self.0.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

impl Decodable for IntroSound {
    type Decoder = IntroDecoder;
    fn decoder(&self) -> IntroDecoder {
        IntroDecoder(self.clone())
    }
}

#[cfg(feature = "intro-video")]
#[derive(Component)]
struct IntroRoot;
#[derive(Component)]
struct IntroPicture;

pub struct IntroPlugin;

impl Plugin for IntroPlugin {
    fn build(&self, app: &mut App) {
        let aid = |name: &str| std::env::var(name).ok().and_then(|v| v.trim().parse().ok());
        let script_films = !crate::test_aid::Unattended::from_env().0 || std::env::var("NVF_PLAY_TEST_FILMS").is_ok_and(|v| v.trim() == "1");
        let film = crate::game_start::GameStart::of_app(app).intro;
        app.insert_resource(Intro { skip_after: aid("NVF_PLAY_TEST_INTRO_SKIP"), shot_at: aid("NVF_PLAY_TEST_INTRO_SHOT"), script_films, film, ..default() })
            .add_message::<PlayIntro>()
            .add_audio_source::<IntroSound>()
            .add_systems(Update, (take_films, start, play).chain().in_set(PlaySet::Ui));
    }
}

/// Whether the held game holds the world's sounds (`crate::ambient`, `crate::radio`):
/// whatever holds the game (`paused`, the pause menu or a film) holds them, except a
/// scripted film with `muteAudio 0` (`film_keeps`, [`Intro::film_keeps_world_sound`]).
pub fn world_sound_held(paused: bool, film_keeps: Option<bool>) -> bool {
    paused && film_keeps != Some(true)
}

/// Let the held new game run (what New Game does once the film is over).
fn begin_game(paused: &mut Paused, next: &mut NextState<PlayState>) {
    paused.0 = false;
    next.set(PlayState::Playing);
    info!("intro: the opening begins");
}

/// The game goes on after a scripted film.
fn resume_game(paused: &mut Paused, next: &mut NextState<PlayState>) {
    paused.0 = false;
    next.set(PlayState::Playing);
    info!("film: the game goes on");
}

/// The films scripts asked for (PlayBink, `UiRequests::films`), into the queue.
fn take_films(mut intro: ResMut<Intro>, script: Option<ResMut<crate::script::ScriptState>>) {
    let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) else { return };
    for f in std::mem::take(&mut rt.game.ui.films) {
        intro.ask(f);
    }
}

/// Whether a queued scripted film may start now: the player in the world, nothing held.
fn world_free(state: &State<PlayState>, paused: &Paused, loading: &crate::loading::Loading) -> bool {
    *state.get() == PlayState::Playing && !paused.0 && !loading.active()
}

/// Without the decoder: straight to the opening; scripted films are passed over.
#[cfg(not(feature = "intro-video"))]
fn start(mut asked: MessageReader<PlayIntro>, mut intro: ResMut<Intro>, mut paused: ResMut<Paused>, mut next: ResMut<NextState<PlayState>>) {
    if asked.read().last().is_some() {
        match &intro.film {
            Some(f) => info!("intro: not built in (cargo feature intro-video); {f} is not played"),
            None => info!("intro: none ({} is empty)", crate::game_start::INTRO_KEY),
        }
        intro.intro_shown = true;
        begin_game(&mut paused, &mut next);
    }
    for f in std::mem::take(&mut intro.queue) {
        info!("film: {}: not built in (cargo feature intro-video); the game goes on", f.path);
    }
}

/// Open a loose film file for decoding.
#[cfg(feature = "intro-video")]
fn open_film(data: Option<&GameData>, file: &str) -> Result<(std::path::PathBuf, BikStreamingDecoder<std::io::BufReader<std::fs::File>>), String> {
    let path = data.ok_or_else(|| "no game data".to_string())?.vfs.loose_path(file).ok_or_else(|| "not a loose file in the game's Data folder".to_string())?;
    let f = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    let name = path.file_name().map_or_else(|| file.to_string(), |n| n.to_string_lossy().into_owned());
    let d = BikStreamingDecoder::new(std::io::BufReader::with_capacity(1 << 20, f), &name).map_err(|e| e.to_string())?.with_output_format(BikOutputFormat::Rgba);
    // A broken header's size would ask the GPU for a texture it refuses (Bevy then closes
    // the game) or for gigabytes of frame: such a film is not played.
    let (w, h) = (d.width(), d.height());
    if !(1..=8192).contains(&w) || !(1..=8192).contains(&h) {
        return Err(format!("film size {w}x{h} out of range"));
    }
    Ok((path, d))
}

#[cfg(feature = "intro-video")]
#[allow(clippy::too_many_arguments)]
fn start(
    mut asked: MessageReader<PlayIntro>,
    mut intro: ResMut<Intro>,
    data: Option<Res<GameData>>,
    mut images: ResMut<Assets<Image>>,
    mut commands: Commands,
    mut paused: ResMut<Paused>,
    mut next: ResMut<NextState<PlayState>>,
    (state, loading): (Res<State<PlayState>>, Res<crate::loading::Loading>),
) {
    if intro.playing.is_some() {
        return;
    }
    let (film, opened) = if asked.read().last().is_some() {
        intro.intro_shown = true;
        let Some(file) = intro.film.clone() else {
            info!("intro: none ({} is empty)", crate::game_start::INTRO_KEY);
            begin_game(&mut paused, &mut next);
            return;
        };
        match open_film(data.as_deref(), &file) {
            Ok(x) => (Film::Intro, x),
            Err(e) => {
                warn!("intro: {file}: {e}");
                begin_game(&mut paused, &mut next);
                return;
            }
        }
    } else {
        // A scripted film once the world is free; one that cannot play is passed over.
        loop {
            if intro.queue.is_empty() || !world_free(&state, &paused, &loading) {
                return;
            }
            let Some(f) = intro.queue.pop_front() else { return };
            match open_film(data.as_deref(), &f.path) {
                Ok(x) => {
                    if intro.is_intro(&f.path) {
                        intro.intro_shown = true;
                    }
                    paused.0 = true;
                    next.set(PlayState::Menu);
                    break (Film::Script(f), x);
                }
                Err(e) => info!("film: {}: {e}; the game goes on", f.path),
            }
        }
    };
    let (path, mut dec) = opened;
    let (w, h, frames, frame_us) = (dec.width(), dec.height(), dec.frame_count(), dec.frame_duration_us());
    match &film {
        Film::Intro => info!("intro: playing {} ({w}x{h}, {frames} frames)", path.display()),
        Film::Script(f) => info!(
            "film: playing {} ({w}x{h}, {frames} frames, PlayBink{}{}{}{})",
            f.path,
            if f.interruptible { ", Esc ends it" } else { "" },
            if f.mute_audio { "" } else { ", world sounds go on" },
            if f.pause_music { ", music paused" } else { "" },
            if f.letterboxed { ", letterboxed" } else { ", stretched" }
        ),
    }
    let shared = Arc::new(Shared { sound: Mutex::new(VecDeque::new()), format: Mutex::new(None), stop: AtomicBool::new(false), done: AtomicBool::new(false) });
    let (tx, rx) = sync_channel::<Frame>(AHEAD);
    let s = shared.clone();
    std::thread::Builder::new()
        .name("nvf-film".into())
        .spawn(move || {
            let mut i = 0u32;
            while !s.stop.load(Ordering::Relaxed) {
                // A panic in the decoder (a broken film) ends the film, logged, rather
                // than the game (guard.rs).
                match nvf_assets::guard::guarded(|| dec.next_frame().map_err(|e| e.to_string())) {
                    Ok(Some(fr)) => {
                        for a in fr.audio {
                            if let Ok(mut f) = s.format.lock() {
                                f.get_or_insert((a.channels, a.sample_rate));
                            }
                            if let Ok(mut q) = s.sound.lock() {
                                q.extend(a.samples);
                            }
                        }
                        if let BikPixels::Rgba(px) = fr.video.pixels {
                            if tx.send((i, px)).is_err() {
                                break;
                            }
                        }
                        i += 1;
                    }
                    Ok(None) => break,
                    Err(e) => {
                        warn!("film: frame {i}: {e}");
                        break;
                    }
                }
            }
            s.done.store(true, Ordering::Relaxed);
        })
        .expect("start the film's decoder");
    let image = images.add(Image::new_fill(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, &[0, 0, 0, 255], TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD));
    let root = commands
        .spawn((IntroRoot, Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(Color::BLACK), GlobalZIndex(250)))
        .with_children(|r| {
            r.spawn((IntroPicture, ImageNode { image_mode: NodeImageMode::Stretch, ..ImageNode::new(image.clone()) }, Node { position_type: PositionType::Absolute, ..default() }));
        })
        .id();
    intro.playing = Some(Playing { frames: Mutex::new(rx), shared, image, size: Vec2::new(w as f32, h as f32), frame_secs: frame_us.max(1) as f64 / 1e6, frame_uploaded: false, started: None, waiting: None, sound: None, root, film });
}

/// The part of the window a picture of `size` takes when fitted inside it.
pub fn contain(size: Vec2, window: Vec2) -> (Vec2, Vec2) {
    if size.x <= 0.0 || size.y <= 0.0 {
        return (window, Vec2::ZERO);
    }
    let fitted = size * (window.x / size.x).min(window.y / size.y);
    (fitted, (window - fitted) / 2.0)
}

#[allow(clippy::too_many_arguments)]
fn play(
    real: Res<Time<Real>>,
    keys: crate::pad::ActionButtons<KeyCode>,
    mouse: crate::pad::ActionButtons<MouseButton>,
    mut intro: ResMut<Intro>,
    mut images: ResMut<Assets<Image>>,
    mut sounds: ResMut<Assets<IntroSound>>,
    mut pictures: Query<&mut Node, With<IntroPicture>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut commands: Commands,
    mut paused: ResMut<Paused>,
    mut next: ResMut<NextState<PlayState>>,
) {
    let keys = keys.snapshot();
    let mouse = mouse.snapshot();
    let skip_after = intro.skip_after;
    let shot_at = intro.shot_at;
    let Some(p) = intro.playing.as_mut() else { return };
    let now = real.elapsed_secs_f64();
    // The sound starts with the first frame shown, once its format is known.
    if p.sound.is_none() {
        let format = p.shared.format.lock().ok().and_then(|f| *f);
        if let Some((ch, rate)) = format {
            let (Some(channels), Some(rate)) = (NonZero::new(u16::from(ch.max(1))), NonZero::new(rate.max(1))) else { return };
            let h = sounds.add(IntroSound { shared: p.shared.clone(), channels, rate });
            p.sound = Some(commands.spawn((AudioPlayer::<IntroSound>(h), PlaybackSettings::DESPAWN)).id());
        }
    }
    let started = *p.started.get_or_insert(now);
    let elapsed = now - started;
    let due = (elapsed / p.frame_secs) as u32;
    // The newest frame whose time has come; older ones are dropped.
    let mut show: Option<Frame> = None;
    let mut ended = false;
    loop {
        let f = match p.waiting.take() {
            Some(f) => f,
            None => match p.frames.lock().map_err(|_| TryRecvError::Disconnected).and_then(|r| r.try_recv()) {
                Ok(f) => f,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    ended = true;
                    break;
                }
            },
        };
        if f.0 <= due {
            show = Some(f);
        } else {
            p.waiting = Some(f);
            break;
        }
    }
    if let Some((_, px)) = show {
        if let Some(mut img) = images.get_mut(&p.image) {
            // A frame of the wrong length would panic the render thread: dropped.
            let size = img.texture_descriptor.size;
            if px.len() == size.width as usize * size.height as usize * 4 {
                img.data = Some(px);
                p.frame_uploaded = true;
            }
        }
    }
    if let (Ok(w), Ok(mut node)) = (windows.single(), pictures.single_mut()) {
        let window = Vec2::new(w.width(), w.height());
        let (size, offset) = if p.film.letterboxed() { contain(p.size, window) } else { (window, Vec2::ZERO) };
        node.width = Val::Px(size.x);
        node.height = Val::Px(size.y);
        node.left = Val::Px(offset.x);
        node.top = Val::Px(offset.y);
    }
    let (tag, shot) = match p.film {
        Film::Intro => ("intro", "intro.png"),
        Film::Script(_) => ("film", "film.png"),
    };
    if let Some(t) = shot_at.filter(|t| elapsed >= f64::from(*t)) {
        info!("{tag}: screenshot {shot} at {t} s");
        commands.spawn(bevy::render::view::screenshot::Screenshot::primary_window()).observe(bevy::render::view::screenshot::save_to_disk(shot));
        intro.shot_at = None;
        return;
    }
    let by_aid = skip_after.is_some_and(|s| elapsed >= f64::from(s));
    // Not the key or click that chose New Game, a moment before.
    let pressed = elapsed > 0.3 && p.film.skipped_by(&keys, &mouse);
    let skipped = pressed || by_aid;
    if !(skipped || (ended && p.waiting.is_none())) {
        return;
    }
    if skipped {
        info!("{tag}: skipped at {elapsed:.1} s{}", if by_aid { " (test aid NVF_PLAY_TEST_INTRO_SKIP)" } else { "" });
    } else {
        info!("{tag}: ended after {elapsed:.1} s");
    }
    if skipped {
        // The decoder stops; the sound fades out (#2217), or goes at once if it has made
        // none yet. Played to its end, its decoder is done already and the sound plays out
        // what it has queued, then ends (`PlaybackSettings::DESPAWN`).
        p.shared.stop.store(true, Ordering::Relaxed);
        if let Some(e) = p.sound.take() {
            stop_sound(&mut commands, e, StopReason::Film);
        }
    }
    commands.entity(p.root).despawn();
    let film = intro.playing.take().map(|p| p.film);
    match film {
        Some(Film::Script(_)) => resume_game(&mut paused, &mut next),
        _ => begin_game(&mut paused, &mut next),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_film_with_mute_audio_0_keeps_the_world_sounding() {
        assert!(!world_sound_held(false, None));
        assert!(world_sound_held(true, None), "the pause menu or the intro holds them");
        assert!(world_sound_held(true, Some(false)), "muteAudio 1 holds them");
        assert!(!world_sound_held(true, Some(true)), "muteAudio 0 leaves them playing");
    }

    #[test]
    fn the_film_fits_inside_the_window_with_black_around_it() {
        let (s, o) = contain(Vec2::new(1280.0, 720.0), Vec2::new(1280.0, 800.0));
        assert_eq!(s, Vec2::new(1280.0, 720.0));
        assert_eq!(o, Vec2::new(0.0, 40.0));
        let (s, o) = contain(Vec2::new(1280.0, 720.0), Vec2::new(2560.0, 1080.0));
        assert_eq!(s, Vec2::new(1920.0, 1080.0));
        assert_eq!(o, Vec2::new(320.0, 0.0));
    }

    fn film(path: &str) -> FilmRequest {
        FilmRequest { path: path.into(), interruptible: false, mute_audio: true, pause_music: true, letterboxed: true }
    }

    #[test]
    fn scripted_films_queue_unless_the_intro_was_shown_or_the_run_is_unattended() {
        let mut intro = Intro { script_films: true, film: Some(FILM.into()), ..default() };
        assert!(intro.ask(film(FILM)), "the opening's call plays the intro film when the main menu did not");
        assert!(intro.ask(film("video\\fnvlogo1.bik")));
        intro.intro_shown = true;
        assert!(!intro.ask(film(FILM)), "not twice");
        assert!(intro.ask(film("video\\other.bik")));
        assert_eq!(intro.queue.len(), 3);
        // With no intro film (`sIntroMovie=`) the opening's call plays it.
        let mut none = Intro { script_films: true, intro_shown: true, ..default() };
        assert!(none.ask(film(FILM)));
        let mut unattended = Intro::default();
        assert!(!unattended.ask(film("video\\other.bik")));
        assert!(unattended.queue.is_empty());
    }
    #[test]
    fn android_intro_readiness_requires_a_valid_frame_applied_to_the_live_image() {
        use bevy::{asset::RenderAssetUsages, render::render_resource::{Extent3d, TextureDimension, TextureFormat}};
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::input::InputPlugin, bevy::state::app::StatesPlugin))
            .init_state::<PlayState>().init_resource::<Paused>()
            .init_resource::<Assets<Image>>().init_resource::<Assets<IntroSound>>()
            .add_systems(Update, play);
        let image = app.world_mut().resource_mut::<Assets<Image>>().add(Image::new_fill(
            Extent3d { width: 1, height: 1, depth_or_array_layers: 1 }, TextureDimension::D2,
            &[0, 0, 0, 255], TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD));
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let root = app.world_mut().spawn_empty().id();
        app.insert_resource(Intro { playing: Some(Playing {
            frames: Mutex::new(rx), shared: Arc::new(Shared { sound: Mutex::new(VecDeque::new()),
                format: Mutex::new(None), stop: AtomicBool::new(false), done: AtomicBool::new(false) }),
            image: image.clone(), size: Vec2::ONE, frame_secs: 1.0, frame_uploaded: false,
            started: None, waiting: None, sound: None, root, film: Film::Intro,
        }), ..default() });
        app.update();
        assert!(app.world().resource::<Intro>().is_playing());
        assert!(!app.world().resource::<Intro>().frame_ready(), "starting decoder/black placeholder never admits reveal");
        tx.send((0, vec![1, 2, 3])).unwrap(); app.update();
        assert!(!app.world().resource::<Intro>().frame_ready(), "malformed frame is not content evidence");
        tx.send((0, vec![11, 22, 33, 255])).unwrap(); app.update();
        assert!(app.world().resource::<Intro>().frame_ready());
        assert_eq!(app.world().resource::<Assets<Image>>().get(&image).unwrap().data.as_deref(), Some([11, 22, 33, 255].as_slice()));
    }

}
