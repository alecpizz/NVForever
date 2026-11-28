//! Dialogue voice in the game (docs/design/dialogue.md step D3, docs/spec-notes/voice.md):
//! the lines scripts make actors say (`Game::speech`, today SayTo and Say) are heard, and
//! last as long as their sound.
//!
//! - **Index.** `fnv_voice::VoiceBank` (the lines of the load order over the archives) is
//!   built on its own thread at the start, like the script data. Once both are ready the
//!   bank becomes the runtime's line-length hook (`Game::voice`): a response lasts as
//!   long as its voice file, and keeps the length fitted to its text when it has none.
//!   A line started before that keeps the length it started with.
//! - **Playback.** Each frame the lines being said (`Game::speech`, and the line of a
//!   conversation with the player, `Game::dialogue`) are compared with what is playing:
//!   a response that starts gets its file read from the archive and played through
//!   Bevy's audio (`AudioPlayer`, Ogg Vorbis by the `vorbis` feature; the 40 lines that
//!   also exist as WAV play their Ogg file); a line that stops before its sound is over
//!   (cut by another line, or its speaker gone) has the sound stopped. One log line per
//!   sound started: `voice: playing <path> (<seconds> s) ...`; a response without a file
//!   logs `voice: no file ...`.
//! - **A line's sound stopped** (cut by another line, its speaker gone, a response
//!   replaced) fades out over `audio_out::DECLICK_FADE` (#2217) [design] instead of being
//!   cut mid-waveform (`audio_out::stop_sound`, counted as voice stops in `sfx declicks:
//!   …`), the voice files, the panned and the cut-to-one-cry lines alike; a held line is
//!   resumed for its fade. A line nearly over is still left to finish by itself.
//! - **Held with its line.** Whatever holds a line's timer holds its sound
//!   ([`line_held`]): a SayTo or Say line stands still in menu mode (the inventory, a
//!   container, a message box, a conversation with the player), so its sound is paused
//!   there and resumed after; a line of the conversation itself goes on in real time and
//!   is held only by a message box on top of it; the pause menu holds every line. One log
//!   line each way (`voice: … held` / `goes on`).
//! - **Heard where it is said** (since 2026-10-11, owner request: "audio volume based on
//!   the distance … if the NPC is out of hearing range we shouldn't show the
//!   subtitle"). A line said outside the conversation with the player (SayTo, Say, a
//!   hello, a Goodbye greeting or the rest of a Goodbye line the player chose, said on
//!   after the conversation closed (`fnv_game::game::Speech::farewell`), a line between
//!   two actors, a detection bark) is heard from where its
//!   speaker stands ([`hearing`]), a line said through a talking activator from where the
//!   activator is (`Speech::place`; the actor need not be there, issues #2451, #2768),
//!   panned to its side but at full volume whatever the distance ([`line_gain`],
//!   [`in_reach`]) [documented: GECK wiki SetTalkingActivatorActor, Notes: "Talking
//!   Activators set via script will have no distance attenuation in the New Vegas
//!   engine"; hypothesis: the same for the actor its scripts name before one is set, and
//!   that one in another cell is still not heard]. Other lines: at full volume within [`DIALOG_MIN_DISTANCE`] of the
//!   player's eye, falling as minimum ÷ distance beyond it and fading to nothing at
//!   [`DIALOG_MAX_DISTANCE`] ([`voice_gain`]),
//!   and panned to the side it is on (`crate::sfx::pan_gains`), following the speaker and
//!   the player while it plays (its gains eased, `crate::sfx::PAN_EASE`). A speaker in another cell (an interior other than the
//!   player's, or inside while the player is outside) is not heard. While a line is not
//!   heard it is silent and shows no subtitle (`crate::script_hud`; [`audible`]): both
//!   follow the distance while the line lasts; the log says `voice: … out of hearing`
//!   when a response starts out of hearing. The
//!   conversation with the player stays at full volume and unpanned. Lip sync follows
//!   [`VoiceState::spoken`] from `nvforever`'s `lips.rs` (docs/spec-notes/lipsync.md).
//!
//! - **Through a helmet or a speaker** ([`crate::voice_filter`]): a speaker wearing an
//!   armour that "modulates voice" (power armour helmets) is heard through the voice
//!   filter (`voice: playing … through the voice filter`). A talking activator's lines play
//!   as their files: the intercoms' files are already processed (issue #992).
//!
//! - **In the room** ([`crate::reverb::for_voice`]): a line heard where it is said goes,
//!   after the voice filter, into the reverb of the place the listener is in, and its
//!   reverb stops when the line does (`voice: … in the room's reverb: <environment>`); the
//!   conversation with the player stays dry. [`VoiceSettings::reverb`] (the Sound tab's
//!   Voice reverb) keeps every line dry.
//!
//! - **A response's own sound.** A response with a sound (TRDT; creatures' and robots'
//!   combat lines, `fnv_game::combat_lines`) plays that SOUN where its speaker stands
//!   instead of a voice file, through `crate::sfx` (`voice: … plays its sound <edid>`); the
//!   line keeps its fitted length.
//!
//! - **One cry of a long generic death file** ([`one_cry`], issue #1539, voice.md §9.1):
//!   a Generic Death file over 5 s (MaleAdult07's hold several cries) is decoded, split
//!   at its silences, and one cry picked at random plays; the game's line ends with it
//!   (`Game::cut_line_after`) and lip sync follows the cry's place in the file
//!   (`voice: one cry of <path>: cry <k> of <n>, …`).
//!
//! Switches: [`VoiceSettings`] (`nvforever --no-voice`, `--voice-volume`);
//! `NVF_PLAY_VOICE_LENGTHS=0` keeps the fitted line lengths (sounds still play).

use crate::player::Player;
use crate::script::ScriptState;
use crate::sfx::{pan_gains, ActorSound, Pan, Panned, PcmSound, PlaySound};
use crate::sim::SimState;
use crate::{PlaySet, PlayWorld};
use bevy::audio::Volume;
use crate::audio_out::{stop_sound, AddAudioSource, Retiring, SoundCategory, StopReason};
use bevy::prelude::*;
use fnv_game::game::Speech;
use fnv_game::Game;
use fnv_combat::rng::SplitMix;
use fnv_sim::FormId;
use fnv_voice::cry::{self, Cut};
use fnv_voice::VoiceBank;
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// How dialogue voice behaves; insert before the play plugins to change the defaults.
#[derive(Resource, Clone, Copy, Debug)]
pub struct VoiceSettings {
    /// Play the sounds (`--no-voice` turns it off; line lengths stay).
    pub play: bool,
    /// Linear volume, 1 = as recorded.
    pub volume: f32,
    /// Lines last as long as their sound (off: the fitted length).
    pub lengths: bool,
    /// Lines said at a place go into the room's reverb ([`crate::reverb::for_voice`]).
    pub reverb: bool,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        VoiceSettings { play: true, volume: 1.0, lengths: std::env::var("NVF_PLAY_VOICE_LENGTHS").map_or(true, |v| v != "0"), reverb: true }
    }
}

/// Dialogue said by an actor is heard at full volume within this many units of the
/// listener [observed: `fDialogMinDistance=600.00` in the [Audio] section of the
/// install's `Fallout_default.ini`; hypothesis: that it is the distance up to which
/// dialogue is not attenuated, as its name says].
pub const DIALOG_MIN_DISTANCE: f32 = 600.0;
/// … and not at all beyond this many [observed: `fDialogMaxDistance=2000.00`, the same
/// section; hypothesis: that it is the distance at which dialogue falls silent]. A line
/// said farther away shows no subtitle either [owner request 2026-10-11].
pub const DIALOG_MAX_DISTANCE: f32 = 2000.0;
/// The speaker's mouth above its feet, game units [design: about a standing head].
pub const MOUTH_HEIGHT: f32 = 110.0;

/// The gain of dialogue said `distance` units from the listener: 1 within
/// [`DIALOG_MIN_DISTANCE`], minimum ÷ distance beyond it, faded linearly to 0 at
/// [`DIALOG_MAX_DISTANCE`] [design: dialogue has no SOUN curve; this is the shape sound
/// effects had before they followed theirs].
pub fn voice_gain(distance: f32) -> f32 {
    let (min, max) = (DIALOG_MIN_DISTANCE, DIALOG_MAX_DISTANCE);
    if distance >= max {
        return 0.0;
    }
    if distance <= min {
        return 1.0;
    }
    (min / distance) * (1.0 - (distance - min) / (max - min))
}

/// The gain of a line heard `distance` units away: [`voice_gain`], or 1 for a line said
/// through a talking activator (`through`), which has no distance attenuation
/// [documented: GECK wiki SetTalkingActivatorActor, Notes].
pub fn line_gain(distance: f32, through: bool) -> f32 {
    if through {
        1.0
    } else {
        voice_gain(distance)
    }
}

/// Whether a line heard `distance` units away is within hearing (and shows its
/// subtitle): nearer than [`DIALOG_MAX_DISTANCE`] (or `reach`, the player's subtitle
/// distance, when given), or at any distance through a talking activator ([`line_gain`]).
pub fn in_reach(distance: f32, through: bool, reach: Option<f32>) -> bool {
    through || distance < reach.unwrap_or(DIALOG_MAX_DISTANCE)
}

/// Where the listener hears a speaker: the speaker's mouth (game space) and its distance
/// from the player's eye; `None` when the speaker is not in the player's cell space (an
/// interior other than the player's, or inside while the player is outside) or nowhere.
pub fn hearing(sim: &SimState, player: &Player, speaker: FormId) -> Option<([f32; 3], f32)> {
    let me = sim.world.peek(FormId::PLAYER_REF)?;
    let (cell, pos) = speaker_place(sim, speaker)?;
    let space = |cell: Option<FormId>| cell.map(|c| sim.refs.world_of(c).map_or((false, c.0), |w| (true, w.0)));
    if space(me.cell)? != space(cell)? {
        return None;
    }
    let at = [pos[0], pos[1], pos[2] + MOUTH_HEIGHT];
    let eye = player.eye();
    let d = ((at[0] - eye[0]).powi(2) + (at[1] - eye[1]).powi(2) + (at[2] - eye[2]).powi(2)).sqrt();
    Some((at, d))
}

/// A speaker's cell and position: its simulation state, or where it is placed while the
/// simulation has not touched it yet (references are materialised on first use, and
/// Doc Mitchell says the opening's lines before anything moved him: they were taken as
/// said in another cell, silent; owner playtest 2026-10-12, "Doc Mitchell's opening
/// lines are not voiced").
pub fn speaker_place(sim: &SimState, speaker: FormId) -> Option<(Option<FormId>, [f32; 3])> {
    match sim.world.peek(speaker) {
        Some(s) => Some((s.cell, s.pos)),
        None => sim.refs.get(speaker).map(|p| (p.cell, p.pos)),
    }
}

/// Whether the player hears a line `speaker` says outside the conversation with the
/// player (its subtitle is shown): in the player's cell space and nearer than
/// [`DIALOG_MAX_DISTANCE`]. Without a player (no world yet) everything is heard.
pub fn audible(sim: &SimState, player: Option<&Player>, speaker: FormId) -> bool {
    audible_line(sim, player, speaker, false, None)
}

/// [`audible`] for a line heard from `place`, said through a talking activator or not
/// (`through`, [`in_reach`]), with the player's subtitle distance `reach` if set.
pub fn audible_line(sim: &SimState, player: Option<&Player>, place: FormId, through: bool, reach: Option<f32>) -> bool {
    match player {
        None => true,
        Some(p) => hearing(sim, p, place).is_some_and(|(_, d)| in_reach(d, through, reach)),
    }
}

/// A sound being played for a line.
struct Playing {
    info: FormId,
    /// Where the line is heard from ([`Said::place`]), and whether through a talking
    /// activator ([`Said::through`]).
    place: FormId,
    through: bool,
    response: usize,
    entity: Option<Entity>,
    /// Real time (seconds since start) at which the sound is over (moved on by the time
    /// the sound is held).
    ends_at: f64,
    /// A line of the conversation with the player (else a SayTo or Say line).
    conversation: bool,
    /// The line's timer stands still, and so does the sound ([`line_held`]).
    held: bool,
    /// The line's time left when last seen: a rise means the line was said again.
    remaining: f64,
    /// The sound's file (`None`: the response has none), and the real time it started.
    path: Option<String>,
    started_at: f64,
    /// Real seconds the sound has stood still so far (`held`).
    held_secs: f64,
    /// Seconds into the file at which the sound began: one cry of a long generic death
    /// file starts inside it ([`one_cry`]); 0 otherwise.
    offset: f64,
    /// A line heard where it is said: its left and right gains, distance included,
    /// followed while it plays (`None`: the conversation's, flat and whole).
    pan: Option<Pan>,
}

/// A response being said, for what follows the voice (lip sync, `nvforever`'s
/// `lips.rs`): who, the sound's file, and when it started and ends on the real-time
/// clock (`Time<Real>`, seconds since start) — also with `--no-voice`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spoken<'a> {
    pub speaker: FormId,
    /// The line (INFO) and the response's index in its record order (`InfoDef::responses`).
    pub info: FormId,
    pub response: usize,
    pub path: Option<&'a str>,
    pub started_at: f64,
    /// Moves on while the sound is held.
    pub ends_at: f64,
    /// Real seconds the sound has stood still so far, held with its line (a menu, the
    /// pause menu): the sound's own time is `now - started_at - held_secs`
    /// ([`Spoken::position`]), which is what a mouth follows.
    pub held_secs: f64,
}

impl Spoken<'_> {
    /// Seconds into the sound at real time `now`: it stands still while the sound is held.
    pub fn position(&self, now: f64) -> f64 {
        now - self.started_at - self.held_secs
    }
}

impl VoiceState {
    /// The responses being said this frame, one per speaker.
    pub fn spoken(&self) -> impl Iterator<Item = Spoken<'_>> {
        // A sound that began inside its file (one cry, [`one_cry`]) reports when the file's
        // start would have been heard, so a mouth following the file's `.lip` is at the
        // cry; it stops at the cut (`ends_at`).
        self.playing.iter().map(|(&speaker, p)| Spoken {
            speaker,
            info: p.info,
            response: p.response,
            path: p.path.as_deref(),
            started_at: p.started_at - p.offset,
            ends_at: p.ends_at,
            held_secs: p.held_secs,
        })
    }
}

#[derive(Resource, Default)]
pub struct VoiceState {
    loader: Option<Mutex<Receiver<(VoiceBank, f64)>>>,
    asked: bool,
    pub bank: Option<Arc<VoiceBank>>,
    /// By speaker.
    playing: HashMap<FormId, Playing>,
    /// Sounds started so far.
    pub started: u64,
    /// Responses that started without a file, leaving out the silent ones ([`silent_text`]:
    /// no words to say, so no file is expected; the reports' `audio` anomaly reads this).
    pub missing: u64,
    /// Responses that started out of the player's hearing (no sound, no subtitle).
    pub unheard: u64,
    /// Responses played through the voice filter ([`crate::voice_filter`]).
    pub filtered: u64,
    /// Responses played through the room's reverb ([`crate::reverb::for_voice`]).
    pub reverbed: u64,
    /// Long generic death files of which one cry was played ([`one_cry`]).
    pub one_cry: u64,
    /// Picks the cry (seeded from the clock on first use).
    rng: Option<SplitMix>,
}

/// Marks the audio entity of a line.
#[derive(Component)]
pub struct VoiceSound;

/// Which response of a line is being said `elapsed` seconds into it, the responses
/// lasting `lengths` one after another.
pub fn response_at(lengths: &[f64], elapsed: f64) -> usize {
    let mut end = 0.0;
    for (i, &l) in lengths.iter().enumerate() {
        end += l;
        if elapsed < end {
            return i;
        }
    }
    lengths.len().saturating_sub(1)
}

/// The response lengths of a line being said, in record order, with the speaker's base
/// taken from the simulation (between frames the runtime's own world is swapped out,
/// `script::with_world`).
pub fn line_lengths(g: &Game, sim: &SimState, s: &Speech) -> Vec<f64> {
    g.response_lengths_voiced(sim.base_of(s.speaker.0), voice_base(g, sim, s.speaker), s.info)
}

/// The base form whose voice type a speaker speaks with: its template chain's "Traits"
/// source, a leveled list standing for the entry picked for it, so an actor speaks with
/// the voice of the sex and race it shows (`Game::voice_base`; issue #1051).
fn voice_base(g: &Game, sim: &SimState, speaker: FormId) -> Option<FormId> {
    sim.base_of(speaker.0).map(|b| g.voice_base_in(&sim.world, b, speaker))
}

/// Which response (index in record order) of a SayTo or Say line is being said: the
/// responses follow one another in response-number order (`InfoDef::response_order`),
/// each for its length. A line of a conversation between actors is its first response
/// only (`Speech::conversation`).
pub fn speech_response(g: &Game, sim: &SimState, s: &Speech) -> usize {
    if s.conversation.is_some() {
        return g.data.infos.get(&s.info).and_then(|d| d.response_order().first().copied()).unwrap_or(0);
    }
    let lengths = line_lengths(g, sim, s);
    let order = g.data.infos.get(&s.info).map(|d| d.response_order()).filter(|o| o.len() == lengths.len());
    let order = order.unwrap_or_else(|| (0..lengths.len()).collect());
    let in_order: Vec<f64> = order.iter().map(|&r| lengths[r]).collect();
    let total: f64 = in_order.iter().sum();
    order.get(response_at(&in_order, total - s.remaining)).copied().unwrap_or(0)
}

/// A response being said this frame.
struct Said {
    speaker: FormId,
    /// Where it is heard from: the talking activator it is said through, else the speaker
    /// (`fnv_game::game::Speech::place`, issues #2451 and #2768).
    place: FormId,
    /// Said through a talking activator: no distance attenuation ([`line_gain`]).
    through: bool,
    info: FormId,
    /// Index in record order.
    response: usize,
    /// Seconds left (of the line for SayTo and Say, of the response in a conversation):
    /// only its direction is used.
    remaining: f64,
    /// A line of the conversation with the player.
    conversation: bool,
    /// Said as a 2D sound (Say's is2D, `fnv_game::game::Speech::two_d`): heard flat and
    /// whole wherever the player is, as the conversation's line is.
    two_d: bool,
}

/// Every response being said: the SayTo and Say lines, and the conversation's line.
///
/// A co-op guest's speaker saying a line of the host's (`Speech::remote`) is heard saying
/// that one only: a line of the guest's own by the same speaker (a hello at this player)
/// would take turns with it, each starting the sound over. No line is remote outside
/// co-op.
fn said_now(g: &Game, sim: &SimState) -> Vec<Said> {
    let hosts: Vec<FormId> = g.speech.iter().filter(|s| s.remote).map(|s| s.speaker).collect();
    let mut out: Vec<Said> =
        g.speech.iter().filter(|s| s.remote || !hosts.contains(&s.speaker)).map(|s| Said { speaker: s.speaker, place: s.place(), through: s.through.is_some(), info: s.info, response: speech_response(g, sim, s), remaining: s.remaining, conversation: false, two_d: s.two_d }).collect();
    if let Some((c, line)) = g.dialogue.as_ref().and_then(|c| Some((c, c.line.as_ref()?))) {
        let order = g.data.infos.get(&line.info).map(|d| d.response_order()).unwrap_or_default();
        if let Some(&r) = order.get(line.response) {
            out.push(Said { speaker: c.actor, place: c.actor, through: false, info: line.info, response: r, remaining: line.remaining, conversation: true, two_d: false });
        }
    }
    out
}

/// What to do about one speaker's sound this frame.
#[derive(Debug, PartialEq)]
pub enum Step {
    /// The response already playing goes on.
    Keep,
    /// Start this response (stopping what the speaker was playing, if it is not over).
    Start(usize),
}

/// Where a line is heard from, and whether it is said through a talking activator
/// ([`Said::place`], [`Said::through`]).
pub type Source = (FormId, bool);

/// Compare a line being said with what its speaker is playing (line, response, time
/// left, [`Source`]). A line heard from elsewhere starts again from there: the same line
/// of a talking actor said through another activator replaces the first in the game
/// (`Game::say_line_through`) and must not stay at the first one's place, even while a
/// menu holds its timer (#2768).
pub fn step(playing: Option<(FormId, usize, f64, Source)>, info: FormId, response: usize, remaining: f64, source: Source) -> Step {
    match playing {
        Some((i, r, before, from)) if i == info && r == response && remaining <= before + 1e-6 && from == source => Step::Keep,
        _ => Step::Start(response),
    }
}

/// Whether a line's timer stands still, in which case its sound is paused and resumed
/// with it. A SayTo or Say line is held by menu mode (`Game::tick` advances it only
/// outside menus: the inventory, a container, a message box, a conversation with the
/// player). A line of the conversation itself goes on in real time by design and is held
/// only by a menu on top of the conversation (a message box). The pause menu holds every
/// line (`script.rs` does not tick under it).
pub fn line_held(conversation: bool, pause_menu: bool, in_menu: bool, menu_on_top: bool) -> bool {
    pause_menu || if conversation { menu_on_top } else { in_menu }
}

/// One cry of a long generic death file (issue #1539, `fnv_voice::cry`, voice.md §9.1):
/// the file decoded, split at its silences and one cry picked by `roll` (in [0, 1)), cut
/// out with its fades; with the cut and the file's length. `None`: the file plays whole
/// (short, silent, or not decodable).
pub fn one_cry(bytes: Vec<u8>, roll: f64) -> Option<(PcmSound, Cut, f64)> {
    let pcm = PcmSound::from_encoded(bytes).ok()?;
    let found = cry::cries(pcm.samples(), pcm.channels(), pcm.rate());
    let cut = cry::choose(&found, pcm.seconds(), roll)?;
    let samples = cry::cut_samples(pcm.samples(), pcm.channels(), pcm.rate(), &cut);
    Some((PcmSound::from_samples(samples, pcm.channels(), pcm.rate()), cut, pcm.seconds()))
}

/// Whether a response's file is cut to one cry ([`one_cry`]): a generic death file
/// (`fnv_voice::cry::is_generic_death`) longer than `fnv_voice::cry::LONG_SECONDS`, of a
/// line of one response said outside the conversation with the player.
pub fn cut_to_one_cry(path: &str, seconds: f64, responses: usize, conversation: bool) -> bool {
    !conversation && responses <= 1 && seconds > cry::LONG_SECONDS && cry::is_generic_death(path)
}

/// A response with no words in it: empty, or only dots and spaces (Generic's HELLO line
/// 0x0017AF3D is `...` [observed: FalloutNV.esm]). Such a line has no voice file by design,
/// so its having none is not counted as missing (#2758).
pub fn silent_text(text: &str) -> bool {
    text.chars().all(|c| c == '.' || c == '\u{2026}' || c.is_whitespace())
}

/// A sound whose line stopped this long before the sound's end is left to finish (the
/// line's clock and the sound's are not the same clock); earlier, it is stopped.
pub const TAIL_SECONDS: f64 = 0.25;

fn load(mut state: ResMut<VoiceState>, play: Option<Res<PlayWorld>>, assets: Option<Res<nvf_assets::GameData>>) {
    if state.bank.is_some() {
        return;
    }
    if !state.asked {
        let (Some(play), Some(assets)) = (play, assets) else { return };
        state.asked = true;
        let (lo, vfs) = (play.lo, assets.vfs.clone());
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new().name("nvf-voice".into()).spawn(move || {
            let t = Instant::now();
            let bank = VoiceBank::build(lo, vfs);
            let _ = tx.send((bank, t.elapsed().as_secs_f64()));
        });
        match spawned {
            Ok(_) => state.loader = Some(Mutex::new(rx)),
            Err(e) => warn!("voice is off: cannot start the index thread: {e}"),
        }
        return;
    }
    let got = state.loader.as_ref().and_then(|rx| rx.lock().ok().and_then(|r| r.try_recv().ok()));
    if let Some((bank, secs)) = got {
        info!(
            "voice ready in {secs:.2} s: {} lines, {} voice types, {} speakers with a voice type",
            bank.index.lines.len(),
            bank.index.voice_types.len(),
            bank.index.speakers.len()
        );
        state.bank = Some(Arc::new(bank));
        state.loader = None;
    }
}

/// Hand the bank to the runtime as its line-length hook, once both exist.
fn hook(state: Res<VoiceState>, settings: Res<VoiceSettings>, mut script: ResMut<ScriptState>) {
    if !settings.lengths {
        return;
    }
    let Some(bank) = state.bank.as_ref() else { return };
    if !script.rt.as_ref().is_some_and(|rt| rt.game.voice.is_none()) {
        return;
    }
    if let Some(rt) = script.rt.as_mut() {
        rt.game.voice = Some(bank.clone());
        info!("voice: lines last as long as their sound (fitted length without one)");
    }
}

/// The left and right gains of a line heard from where its speaker is now (0 out of
/// hearing).
fn placed_gains(sim: &SimState, player: Option<&Player>, speaker: FormId, through: bool) -> [f32; 2] {
    let Some(p) = player else { return [1.0, 1.0] };
    match hearing(sim, p, speaker) {
        Some((at, d)) => {
            let g = line_gain(d, through);
            let [l, r] = pan_gains(p.eye(), p.heading, at);
            [l * g, r * g]
        }
        None => [0.0, 0.0],
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn play(
    mut commands: Commands,
    time: Res<Time<Real>>,
    settings: Res<VoiceSettings>,
    mut state: ResMut<VoiceState>,
    mut script: ResMut<ScriptState>,
    sim: Res<SimState>,
    (sounds, mut panned, player): (Option<ResMut<Assets<AudioSource>>>, Option<ResMut<Assets<Panned<AudioSource>>>>, Option<Res<Player>>),
    paused: Res<crate::pause::Paused>,
    // Not a line fading out after it was stopped (audio_out).
    sinks: Query<&AudioSink, (With<VoiceSound>, Without<Retiring>)>,
    (mut effects, mut by_actors): (MessageWriter<PlaySound>, MessageWriter<ActorSound>),
    (sfx, sfx_settings): (Option<Res<crate::sfx::Sfx>>, Option<Res<crate::sfx::SfxSettings>>),
    mut cut_sounds: Option<ResMut<Assets<Panned<PcmSound>>>>,
    mut cover: Option<ResMut<crate::sound_cover::SoundCover>>,
) {
    let player = player.as_deref();
    let st = &mut *state;
    let (Some(bank), Some(rt)) = (st.bank.clone(), script.rt.as_ref()) else { return };
    let g = &rt.game;
    let now = time.elapsed_secs_f64();
    let mut sounds = sounds;
    let said_now = said_now(g, &sim);
    // Lines cut to one cry this frame: (speaker, line, seconds), told to the game after.
    let mut cuts: Vec<(FormId, FormId, f64)> = Vec::new();
    // Lines no longer said: stop a sound that is not nearly over (the line was cut).
    // A held sound cannot finish by itself: it is stopped as well.
    st.playing.retain(|speaker, p| {
        let said = said_now.iter().any(|s| s.speaker == *speaker && s.info == p.info);
        if !said && (p.held || now < p.ends_at - TAIL_SECONDS) {
            if let Some(e) = p.entity.take() {
                stop_sound(&mut commands, e, StopReason::Voice);
            }
            info!("voice: stopped {} line {} ({:.2} s before its end)", g.data.name(*speaker), g.data.name(p.info), p.ends_at - now);
        }
        said
    });
    // A line heard where it is said follows its speaker and the listener.
    let cover_now = std::time::Instant::now();
    for (speaker, p) in st.playing.iter() {
        if let Some(pan) = &p.pan {
            pan.set(placed_gains(&sim, player, p.place, p.through));
            // Heard at the listener while it plays, by the voice's fall-off
            // (`crate::sound_cover`); a held line makes no sound.
            let heard = player.filter(|_| p.entity.is_some() && !p.held && now < p.ends_at && *speaker != FormId::PLAYER_REF).and_then(|pl| hearing(&sim, pl, p.place));
            if let (Some((_, d)), Some(cover)) = (heard, cover.as_deref_mut()) {
                cover.note(Some(crate::sound_cover::SoundCover::key(crate::sound_cover::VOICE_KIND, u64::from(speaker.0))), line_gain(d, p.through), d, crate::sound_cover::LOOP_HOLD, cover_now);
            }
        }
    }
    // Whatever holds a line's timer holds its sound: the sink is paused, the sound's end
    // moves on by the time held, and both go on together.
    for (speaker, p) in st.playing.iter_mut() {
        // The companion wheel's reply goes on behind it (`fnv_game::companion_wheel`).
        let hold = if g.line_goes_on_in_menu(*speaker) { paused.0 } else { line_held(p.conversation, paused.0, g.in_menu(), g.menu.is_some()) };
        if hold {
            p.ends_at += time.delta_secs_f64();
            p.held_secs += time.delta_secs_f64();
        }
        if hold != p.held {
            p.held = hold;
            info!("voice: {} line {} {}", g.data.name(*speaker), g.data.name(p.info), if hold { "held (its timer stands still)" } else { "goes on" });
        }
        // The sink appears a frame or two after the sound was spawned.
        if let Some(sink) = p.entity.and_then(|e| sinks.get(e).ok()) {
            if hold && !sink.is_paused() {
                sink.pause();
            } else if !hold && sink.is_paused() {
                sink.play();
            }
        }
    }
    for s in &said_now {
        let k = s.response;
        let was = st.playing.get(&s.speaker).map(|p| (p.info, p.response, p.remaining, (p.place, p.through)));
        if step(was, s.info, k, s.remaining, (s.place, s.through)) == Step::Keep {
            if let Some(p) = st.playing.get_mut(&s.speaker) {
                p.remaining = s.remaining;
            }
            continue;
        }
        if let Some(old) = st.playing.remove(&s.speaker) {
            if old.held || now < old.ends_at - TAIL_SECONDS {
                if let Some(e) = old.entity {
                    stop_sound(&mut commands, e, StopReason::Voice);
                }
            }
        }
        let placed = sim.base_of(s.speaker.0);
        let base = voice_base(g, &sim, s.speaker);
        let fitted = g.response_seconds_voiced(placed, base, s.info, k);
        let who = g.data.name(s.speaker);
        let number = bank.index.lines.get(&s.info.0).and_then(|l| l.responses.get(k)).map_or(0, |r| r.number);
        // A line said for its fixed speaker (INFO ANAM) by another actor (its own GetIsID
        // lets it: the Think Tank through Dr. Klein, the end slides' narrator) has its file
        // in the fixed speaker's folder only: tried after the speaker's own, as the line's
        // length is (`Game::response_seconds_as`) [observed: fnvtool voice line, INFO
        // 010134BA and 001614B2; hypothesis: the game plays that file].
        // A fixed speaker that is the placed base is no other actor: its file is in the
        // record's own voice, not the one the actor speaks with (issue #1051).
        let fixed = g.data.infos.get(&s.info).and_then(|d| d.speaker).filter(|f| Some(*f) != base && Some(*f) != placed);
        // The player speaks in the voice of the sex she or he is (`Game::voice_override`,
        // issue #2088), not the player record's.
        let own_voice = g.voice_override(base);
        let own = match own_voice {
            Some(vt) => bank.file_for_voice(s.info.0, k, vt.0),
            None => base.and_then(|b| bank.file(s.info.0, k, b.0)),
        };
        let file = own.or_else(|| fixed.and_then(|f| bank.file(s.info.0, k, f.0)));
        // A line outside the conversation is heard from where it is said, or not at all;
        // a 2D one (Say's is2D) as the conversation's.
        let heard = if s.conversation || s.two_d { None } else { Some(player.and_then(|p| hearing(&sim, p, s.place))) };
        let out_of_hearing = match heard {
            Some(Some((_, d))) if !in_reach(d, s.through, None) => Some(format!("{d:.0} units away")),
            Some(None) if player.is_some() => {
                let cell = |r: FormId| speaker_place(&sim, r).and_then(|(c, _)| c).map_or("none".to_string(), |c| g.data.name(c));
                Some(format!("in another cell: {} vs the player's {}", cell(s.place), cell(FormId::PLAYER_REF)))
            }
            _ => None,
        };
        let pan = heard.map(|_| Pan::new(placed_gains(&sim, player, s.place, s.through)));
        // A response with a sound of its own plays it instead of the voice (creatures'
        // and robots' combat lines; docs/spec-notes/combat-dialogue.md §3).
        let own_sound = g.data.infos.get(&s.info).and_then(|d| d.responses.get(k)).and_then(|r| r.sound);
        if let Some(sound) = own_sound {
            let at = match heard {
                Some(Some((at, _))) => Some(at),
                _ => None,
            };
            if settings.play && out_of_hearing.is_none() {
                // Where it is said it goes by its distances, as another actor's sounds do
                // (audio.md §15.6): 105 lines name a 2D song.
                match at {
                    Some(at) => {
                        by_actors.write(ActorSound { sound, at });
                    }
                    None => {
                        effects.write(PlaySound { sound, at: None });
                    }
                }
            }
            st.started += 1;
            st.unheard += u64::from(out_of_hearing.is_some());
            info!("voice: {who} line {} response {number} plays its sound {}{}", g.data.name(s.info), g.data.name(sound), if out_of_hearing.is_some() { " [out of hearing]" } else { "" });
            st.playing.insert(
                s.speaker,
                Playing { info: s.info, place: s.place, through: s.through, response: k, entity: None, ends_at: now + fitted, remaining: s.remaining, path: None, started_at: now, held_secs: 0.0, conversation: s.conversation, held: false, pan, offset: 0.0 },
            );
            continue;
        }
        let mut offset = 0.0;
        let (entity, seconds) = match &file {
            Some(path) => {
                let mut seconds = bank.seconds(path).unwrap_or(fitted);
                // A long generic death file: one cry of it (issue #1539).
                let responses = g.data.infos.get(&s.info).map_or(0, |d| d.responses.len());
                let cry = if cut_to_one_cry(path, seconds, responses, s.conversation) {
                    let rng = st.rng.get_or_insert_with(|| SplitMix(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(7, |d| d.as_nanos() as u64)));
                    let roll = rng.next_f32() as f64;
                    bank.read(path).and_then(|b| crate::sfx::checked(path, b)).and_then(|b| one_cry(b, roll))
                } else {
                    None
                };
                if let Some((_, cut, total)) = &cry {
                    seconds = cut.span.seconds();
                    offset = cut.span.start;
                    st.one_cry += 1;
                    cuts.push((s.speaker, s.info, seconds));
                    let which = match cut.cry {
                        Some((k, n)) => format!("cry {} of {n}", k + 1),
                        None => "a stretch (no silence found)".to_string(),
                    };
                    info!("voice: one cry of {path}: {which}, {:.2}-{:.2} s of its {total:.2} s; {who} line {} ends with it", cut.span.start, cut.span.end, g.data.name(s.info));
                }
                // Played also out of hearing, silent: it is heard if the speaker comes
                // near (or the player does) while it lasts.
                // Said through a helmet or a speaker: the voice filter (crate::voice_filter).
                // The player's tweak leaves voices unfiltered [design: docs/design/tweaks.md].
                let filtered = !g.tweaks.no_voice_filter && crate::voice_filter::speaker_filtered(&sim, s.speaker);
                // Said at a place: into the listener's room, cut at the line's end.
                let room = sfx.as_ref().and_then(|x| x.room_reverb);
                let reverb_on = sfx_settings.as_ref().is_some_and(|x| x.reverb);
                let reverb = crate::reverb::for_voice(room, reverb_on, settings.reverb, pan.is_some() && out_of_hearing.is_none());
                let entity = match (settings.play, cry.as_ref(), cut_sounds.as_mut()) {
                    // One cry: its samples, at the speaker's place (or whole at both ears).
                    (true, Some((pcm, ..)), Some(cut_sounds)) => {
                        let playback = PlaybackSettings::DESPAWN.with_volume(Volume::Linear(settings.volume.max(0.0)));
                        let pan = pan.clone().unwrap_or_else(|| Pan::new([1.0, 1.0]));
                        let handle = cut_sounds.add(Panned { voice_filter: filtered, reverb, ..Panned::new(pcm.clone(), pan) });
                        Some(commands.spawn((AudioPlayer::<Panned<PcmSound>>(handle), playback, VoiceSound, SoundCategory::Voice)).id())
                    }
                    _ => match (settings.play, sounds.as_mut(), bank.read(path).and_then(|b| crate::sfx::checked(path, b))) {
                        (true, Some(assets), Some(bytes)) => {
                            let source = AudioSource { bytes: bytes.into() };
                            let volume = Volume::Linear(settings.volume.max(0.0));
                            let playback = PlaybackSettings::DESPAWN.with_volume(volume);
                            // The conversation's line, filtered, at both ears whole.
                            let pan = pan.clone().or_else(|| filtered.then(|| Pan::new([1.0, 1.0])));
                            match (&pan, panned.as_mut()) {
                                (Some(pan), Some(panned)) => {
                                    let handle = panned.add(Panned { voice_filter: filtered, reverb, ..Panned::new(source, pan.clone()) });
                                    Some(commands.spawn((AudioPlayer::<Panned<AudioSource>>(handle), playback, VoiceSound, SoundCategory::Voice)).id())
                                }
                                _ => Some(commands.spawn((AudioPlayer::new(assets.add(source)), playback, VoiceSound, SoundCategory::Voice)).id()),
                            }
                        }
                        _ => None,
                    },
                };
                st.started += 1;
                st.filtered += u64::from(filtered && entity.is_some());
                let reverb = reverb.filter(|_| entity.is_some());
                st.reverbed += u64::from(reverb.is_some());
                let place = match heard {
                    Some(Some((_, d))) => format!(" at {d:.0} units, gain {:.2}", voice_gain(d)),
                    _ => String::new(),
                };
                let silent = match (&out_of_hearing, entity) {
                    (Some(why), _) => format!(" [out of hearing, {why}: silent and no subtitle while it is]"),
                    (None, None) => " [silent: voice playback is off]".to_string(),
                    (None, Some(_)) => String::new(),
                };
                let filter = if filtered { " through the voice filter" } else { "" };
                info!("voice: playing {path} ({seconds:.2} s) {who} line {} response {number}{place}{filter}{silent}", g.data.name(s.info));
                if let Some(r) = reverb {
                    info!("voice: {who} line {} response {number} in the room's reverb: {} (send {:.2}, cut at its end)", g.data.name(s.info), r.preset.name, r.send);
                }
                (entity, seconds)
            }
            None => {
                let text = g.data.infos.get(&s.info).and_then(|d| d.responses.get(k)).map_or("", |r| r.text.as_str());
                st.missing += u64::from(!silent_text(text));
                if let Some(why) = &out_of_hearing {
                    info!("voice: {who} line {} response {number} is out of hearing ({why}): no subtitle", g.data.name(s.info));
                }
                let voice = own_voice.map(|v| v.0).or_else(|| base.and_then(|b| bank.index.voice_of(b.0))).and_then(|v| bank.index.voice_types.get(&v));
                info!("voice: no file for {who} line {} response {number} (voice type {})", g.data.name(s.info), voice.map_or("none", |v| v.as_str()));
                (None, fitted)
            }
        };
        st.unheard += u64::from(out_of_hearing.is_some());
        st.playing.insert(
            s.speaker,
            Playing { info: s.info, place: s.place, through: s.through, response: k, entity, ends_at: now + seconds, remaining: s.remaining, path: file, started_at: now, held_secs: 0.0, conversation: s.conversation, held: false, pan, offset },
        );
    }
    // The lines cut to one cry end with it (and DeathResponse comes then).
    if let Some(rt) = script.rt.as_mut() {
        for (speaker, info, seconds) in cuts {
            rt.game.cut_line_after(speaker, info, seconds);
        }
    }
}

pub struct VoicePlugin;

impl Plugin for VoicePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VoiceSettings>()
            .init_resource::<VoiceState>()
            .add_audio_source::<Panned<AudioSource>>()
            .add_audio_source::<Panned<PcmSound>>()
            .add_systems(Update, (load, hook, play).chain().in_set(PlaySet::Ui))
            .add_systems(Last, report);
        crate::settings::add_setting(
            app,
            crate::settings::Setting {
                key: "voice",
                group: crate::pause::GROUP_MENUS,
                label: "Dialogue voice",
                note: "On: lines are spoken. Off: they are silent from the next line on, and still last as long as their sound.",
                switches: "--no-voice",
                get: |w| w.resource::<VoiceSettings>().play,
                set: |w, on| w.resource_mut::<VoiceSettings>().play = on,
                slider: None,
            },
        );
    }
}

/// At exit: how many responses were played and how many were out of hearing.
fn report(mut exit: MessageReader<AppExit>, state: Res<VoiceState>) {
    if exit.read().next().is_some() {
        info!(
            "voice summary: {} responses started with a file, {} without one, {} started out of the player's hearing (silent, no subtitle while out of it), {} through the voice filter, {} through the room's reverb, {} long generic death files cut to one cry",
            state.started, state.missing, state.unheard, state.filtered, state.reverbed, state.one_cry
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_with_no_words_are_silent() {
        assert!(silent_text("..."));
        assert!(silent_text(""));
        assert!(silent_text(" \u{2026} "));
        assert!(!silent_text("Hello."));
        assert!(!silent_text("...Hello?"), "words after the dots are said");
    }

    #[test]
    fn only_long_generic_death_lines_are_cut_to_one_cry() {
        let long = r"sound\voice\falloutnv.esm\maleadult07\generic_death_00175182_1.ogg";
        assert!(cut_to_one_cry(long, 15.23, 1, false));
        assert!(!cut_to_one_cry(long, 1.09, 1, false), "a short file plays whole");
        assert!(!cut_to_one_cry(long, 15.23, 2, false), "a line of several responses is left whole");
        assert!(!cut_to_one_cry(long, 15.23, 1, true), "the conversation's line is left whole");
        let named = r"sound\voice\falloutnv.esm\bennyvoice\vmsbennydialogue_death_00112233_1.ogg";
        assert!(!cut_to_one_cry(named, 15.23, 1, false), "a named NPC's own Death line is not cut");
    }

    #[test]
    fn dialogue_is_heard_by_distance() {
        assert_eq!(voice_gain(0.0), 1.0);
        assert_eq!(voice_gain(DIALOG_MIN_DISTANCE), 1.0);
        // Halfway out: 600/1300 of it, faded by half.
        let g = voice_gain(1300.0);
        assert!((g - 600.0 / 1300.0 * 0.5).abs() < 1e-5, "{g}");
        // Falling all the way, and nothing from the maximum on.
        assert!(voice_gain(1000.0) > voice_gain(1500.0) && voice_gain(1500.0) > voice_gain(1999.0));
        assert_eq!(voice_gain(DIALOG_MAX_DISTANCE), 0.0);
        assert_eq!(voice_gain(5000.0), 0.0);
    }

    /// A line said through a talking activator has no distance attenuation (GECK wiki
    /// SetTalkingActivatorActor, #2768): full volume and within hearing at any distance;
    /// other lines as before, the subtitle distance included.
    #[test]
    fn a_line_through_a_talking_activator_is_not_attenuated() {
        for d in [0.0, 1300.0, DIALOG_MAX_DISTANCE, 5000.0] {
            assert_eq!(line_gain(d, true), 1.0, "{d}");
            assert_eq!(line_gain(d, false), voice_gain(d), "{d}");
            assert!(in_reach(d, true, None) && in_reach(d, true, Some(500.0)), "{d}");
        }
        assert!(in_reach(1999.0, false, None) && !in_reach(DIALOG_MAX_DISTANCE, false, None));
        assert!(!in_reach(600.0, false, Some(500.0)) && in_reach(400.0, false, Some(500.0)));
    }

    #[test]
    fn responses_follow_one_another() {
        let l = [2.0, 3.0, 1.5];
        assert_eq!(response_at(&l, 0.0), 0);
        assert_eq!(response_at(&l, 1.99), 0);
        assert_eq!(response_at(&l, 2.0), 1);
        assert_eq!(response_at(&l, 4.99), 1);
        assert_eq!(response_at(&l, 5.0), 2);
        // Past the end (the last frame of a line): the last response.
        assert_eq!(response_at(&l, 99.0), 2);
        assert_eq!(response_at(&[], 1.0), 0);
    }

    #[test]
    fn a_sound_is_held_with_its_lines_timer() {
        // (conversation, pause menu, menu mode, a menu on top of the conversation)
        // A SayTo line: free in the world, held by any menu and by a conversation.
        assert!(!line_held(false, false, false, false));
        assert!(line_held(false, false, true, false));
        assert!(line_held(false, false, true, true));
        // The conversation's own line: menu mode is the conversation itself, so it plays;
        // a message box on top holds it.
        assert!(!line_held(true, false, true, false));
        assert!(line_held(true, false, true, true));
        // The pause menu holds everything.
        assert!(line_held(true, true, true, false));
        assert!(line_held(false, true, false, false));
    }

    #[test]
    fn a_sound_starts_once_per_response() {
        let (a, b) = (FormId(0x10), FormId(0x11));
        let here = (FormId(0x20), false);
        // Nothing playing: start.
        assert_eq!(step(None, a, 0, 4.0, here), Step::Start(0));
        // The same response, time running down: keep.
        assert_eq!(step(Some((a, 0, 4.0, here)), a, 0, 3.9, here), Step::Keep);
        // A menu holds script time: still the same sound.
        assert_eq!(step(Some((a, 0, 3.9, here)), a, 0, 3.9, here), Step::Keep);
        // The line's next response.
        assert_eq!(step(Some((a, 0, 2.1, here)), a, 1, 1.9, here), Step::Start(1));
        // Another line by the same speaker.
        assert_eq!(step(Some((a, 1, 1.9, here)), b, 0, 5.0, here), Step::Start(0));
        // The same line said again: its time left went up.
        assert_eq!(step(Some((a, 0, 0.4, here)), a, 0, 4.0, here), Step::Start(0));
    }

    /// One talking actor's line said through a second activator while a menu holds its
    /// timer (the same line, response and time left): the sound starts again from the
    /// second activator instead of staying at the first (#2768); so does a line that
    /// starts or stops going through an activator at the same place.
    #[test]
    fn a_line_moved_to_another_activator_starts_again_there() {
        let line = FormId(0x10);
        let (first, second, actor) = (FormId(0x601), FormId(0x607), FormId(0x502));
        assert_eq!(step(Some((line, 0, 3.0, (first, true))), line, 0, 3.0, (first, true)), Step::Keep);
        assert_eq!(step(Some((line, 0, 3.0, (first, true))), line, 0, 3.0, (second, true)), Step::Start(0));
        assert_eq!(step(Some((line, 1, 1.0, (first, true))), line, 1, 1.0, (second, true)), Step::Start(1));
        assert_eq!(step(Some((line, 0, 3.0, (actor, false))), line, 0, 3.0, (actor, true)), Step::Start(0));
    }
}
