//! Sounds scripts ask for: `PlaySound` and `PlaySound3D` (docs/spec-notes/audio.md §11),
//! and the music `PlayMusic` asks for (§13).
//!
//! The rules only record what is to be heard ([`SoundRequest`], taken from
//! [`Game::sound_requests`] by whoever plays sounds) and trace it as `sound_asked`.
//! Playing it (the SOUN's file, its loudness) is the engine's work. In a headless run
//! nothing takes them: at most [`MAX_PENDING`] are kept. They are not part of the saved
//! state.

use crate::functions::{none, Args, Ctx};
use crate::game::Game;
use crate::trace::EventKind;
use fnv_script::vm::Value;
use fnv_sim::FormId;
use serde::Serialize;

/// Requests kept at most; older ones are dropped when nothing takes them (a headless
/// run) [design].
pub const MAX_PENDING: usize = 256;

/// A sound a script asked for.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SoundRequest {
    /// The SOUN.
    pub sound: FormId,
    /// `PlaySound3D`: the calling reference, where the sound is [documented: GECK wiki
    /// PlaySound3D]. `None`: `PlaySound`, not placed ("non-locational") [documented:
    /// GECK wiki PlaySound].
    pub at: Option<FormId>,
    /// `PlaySound`'s SystemSound flag: it plays in menu mode too; without it the sound
    /// may wait for game mode [documented: GECK wiki PlaySound].
    pub system: bool,
    /// Its id in the sound functions' list (`crate::nvse_sound`), for the engine's answer.
    pub id: u32,
    /// Where the sound is, in place of the reference's place (PlaySound3DNode's node,
    /// `crate::nvse_jip14`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub point: Option<[f32; 3]>,
}

/// A script effect a co-op host's guests play too ([`Game::coop_effects`]) [design].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoopEffect {
    /// A sound at a reference other than the host's player (PlaySound3D).
    Sound { sound: FormId, at: FormId },
    /// A music type (PlayMusic).
    Music(FormId),
    /// A script moved the player (MoveTo, PositionCell...): the guests go with it.
    PlayerMoved,
    /// A script played an animation group on a reference (`PlayGroup`, flag 0-2): the
    /// guests' copies play it too (a rocket bay opening, a gate rising).
    Anim { on: FormId, group: u16, flag: i32 },
    /// A script reset a reference's animation state (`Reset3DState`).
    AnimReset(FormId),
}

impl Game<'_> {
    /// Queue a sound for the engine and trace it.
    pub fn ask_sound(&mut self, mut r: SoundRequest) {
        self.emit(EventKind::SoundAsked { sound: r.sound, at: r.at, system: r.system });
        // A world sound goes to a host's guests too; the host's own 2D and player
        // sounds do not (they are its player's) [design].
        if let (Some(out), Some(at)) = (self.coop_effects.as_mut(), r.at.filter(|&a| a != FormId::PLAYER_REF)) {
            if out.len() < MAX_PENDING {
                out.push(CoopEffect::Sound { sound: r.sound, at });
            }
        }
        r.id = self.gaps.sounds.ask(r.sound, r.at, self.frame);
        if self.sound_requests.len() >= MAX_PENDING {
            self.sound_requests.remove(0);
        }
        self.sound_requests.push(r);
    }
}

/// `PlaySound sound [SystemSound]`: play a SOUN, not placed [documented: GECK wiki
/// PlaySound, owner export].
pub fn play_sound(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let sound = a.form(0)?;
    let system = a.num_or(1, 0.0) != 0.0;
    g.ask_sound(SoundRequest { sound, at: None, system, id: 0, point: None });
    none()
}

impl Game<'_> {
    /// Queue a music type for the engine and trace it.
    pub fn ask_music(&mut self, music: FormId) {
        self.emit(EventKind::MusicAsked { music });
        if let Some(out) = self.coop_effects.as_mut().filter(|o| o.len() < MAX_PENDING) {
            out.push(CoopEffect::Music(music));
        }
        self.gaps.music_type = Some(music);
        if self.music_requests.len() >= MAX_PENDING {
            self.music_requests.remove(0);
        }
        self.music_requests.push(music);
    }
}

/// `PlayMusic musicType`: play a music type (MUSC) [documented: GECK wiki PlayMusic, owner
/// export; in New Vegas it "will not stop the currently playing music of a music
/// marker"]. The engine plays it (docs/spec-notes/audio.md §13).
pub fn play_music(g: &mut Game, _ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let music = a.form(0)?;
    g.ask_music(music);
    none()
}

/// `ref.PlaySound3D sound`: play a SOUN at the calling reference [documented: GECK wiki
/// PlaySound3D, owner export].
pub fn play_sound_3d(g: &mut Game, ctx: &Ctx, a: &Args) -> Result<Value, String> {
    let sound = a.form(0)?;
    let at = ctx.subject()?;
    g.ask_sound(SoundRequest { sound, at: Some(at), system: false, id: 0, point: None });
    none()
}
