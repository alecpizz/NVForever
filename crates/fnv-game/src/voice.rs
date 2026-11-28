//! How long a response of a dialogue line lasts (docs/design/dialogue.md step D3,
//! docs/spec-notes/voice.md §8): as long as its voice file when the game was given a
//! way to ask ([`Game::voice`], crate `fnv-voice`), else the length fitted to its text
//! ([`crate::game::fitted_seconds`], docs/spec-notes/scripts.md §7.7). Every response is
//! timed through `Game::response_seconds` (`crate::dialogue`), which comes here.

use crate::game::{fitted_seconds, Game};
use fnv_sim::FormId;

/// A voice file shorter than [`PLACEHOLDER_SECONDS`] and than this share of the length
/// fitted to its text is a placeholder, not a reading of the text: the line keeps its
/// fitted length [design, #4825: the "{no VO}" description lines (Rex's examine branch,
/// INFO 0015BA61) ship with a 0.12 s file, gone before the text could be read; a real
/// reading of even one short word is longer than a quarter second; `fnvtool voice
/// coverage --lengths` counts the placeholders as `placeholder_sounds`].
pub const PLACEHOLDER_SHARE: f64 = 0.25;
/// See [`PLACEHOLDER_SHARE`].
pub const PLACEHOLDER_SECONDS: f64 = 0.25;

/// Whether a voice file of `sound` seconds is a placeholder for a response of `chars`
/// characters ([`PLACEHOLDER_SHARE`]).
pub fn is_placeholder(sound: f64, chars: usize) -> bool {
    sound < PLACEHOLDER_SECONDS && sound < fitted_seconds(chars) * PLACEHOLDER_SHARE
}

/// How long a response with a voice file of `sound` seconds lasts: the file's length,
/// unless it is a placeholder ([`is_placeholder`]), then the length fitted to its text.
pub fn seconds_with_sound(sound: f64, chars: usize) -> f64 {
    if is_placeholder(sound, chars) {
        fitted_seconds(chars)
    } else {
        sound
    }
}

/// The lengths of voice files. Implemented outside this crate (`fnv_voice::VoiceBank`),
/// so the rules stay free of archives and sound formats.
pub trait VoiceLengths: Send + Sync {
    /// Seconds of the sound of response `response` (index in record order, as in
    /// `InfoDef::response_chars`) of line `info`, said by an actor or talking activator
    /// whose base form is `speaker_base`. `None`: no sound is known for it.
    fn response_seconds(&self, info: FormId, response: usize, speaker_base: Option<FormId>) -> Option<f64>;

    /// The Data path of that response's voice file, when known (the voice path of
    /// SetOnNPCResponseEventHandler, `nvse_jg8`).
    fn response_path(&self, _info: FormId, _response: usize, _speaker_base: Option<FormId>) -> Option<String> {
        None
    }

    /// [`VoiceLengths::response_seconds`] in voice type `voice_type` (a VTYP), for a
    /// speaker whose voice is not its record's (the player, [`Game::voice_override`]).
    fn response_seconds_in_voice(&self, _info: FormId, _response: usize, _voice_type: FormId) -> Option<f64> {
        None
    }

    /// [`VoiceLengths::response_path`] in voice type `voice_type`.
    fn response_path_in_voice(&self, _info: FormId, _response: usize, _voice_type: FormId) -> Option<String> {
        None
    }
}

impl Game<'_> {
    /// [`Game::response_seconds`] for a speaker given by its base form (for callers that
    /// hold the world themselves, as the engine does between frames): the length of the
    /// response's voice file [hypothesis: the game ends a line when its sound ends; the
    /// intro's stage timings compare both lengths with the capture, voice.md §8], else
    /// the length fitted to its text.
    ///
    /// A line with a fixed speaker (INFO `ANAM`) said by another actor (its own GetIsID
    /// condition lets that actor say it, `Game::can_say_as`) has its file in the fixed
    /// speaker's voice folder, not in the actor's: the speaker's own voice is tried
    /// first, then the fixed speaker's [observed: Old World Blues' Think Tank lines
    /// spoken through Dr. Klein with ANAM Dr. 8 or Dr. O (INFO 010134BA) and the end
    /// slides with ANAM a Boomer said by the narrator (INFO 001614B2) exist only in the
    /// fixed speaker's folder (`fnvtool voice line`); voice.md §3.1: 22,252 of 22,589
    /// fixed-speaker responses are in that speaker's folder; hypothesis: the game plays
    /// that file; found by walkthrough/old-world-blues.actions.json].
    pub fn response_seconds_as(&self, speaker_base: Option<FormId>, info: FormId, response: usize) -> f64 {
        self.response_seconds_voiced(speaker_base, speaker_base, info, response)
    }

    /// [`Game::response_seconds_as`] for a speaker placed with `placed_base` that speaks
    /// with `voice_base`'s voice ([`Game::voice_base`]). The fixed speaker's file stands in
    /// only for a line said by another actor: a fixed speaker that is the placed base
    /// itself is no other actor, and its file (in the record's own voice type) would bring
    /// back the voice the actor does not have (issue #1051).
    pub fn response_seconds_voiced(&self, placed_base: Option<FormId>, voice_base: Option<FormId>, info: FormId, response: usize) -> f64 {
        let voice = self.voice.as_ref();
        let own = match self.voice_override(voice_base) {
            Some(vt) => voice.and_then(|v| v.response_seconds_in_voice(info, response, vt)),
            None => voice.and_then(|v| v.response_seconds(info, response, voice_base)),
        };
        let sound = own.or_else(|| {
            let fixed = self.dialogue_view().info(&info).and_then(|d| d.speaker).filter(|s| Some(*s) != voice_base && Some(*s) != placed_base)?;
            voice?.response_seconds(info, response, Some(fixed))
        });
        let chars = || self.dialogue_view().info(&info).and_then(|d| d.response_chars.get(response)).copied().unwrap_or(0);
        match sound {
            Some(s) => seconds_with_sound(s, chars()),
            None => fitted_seconds(chars()),
        }
    }

    /// The length of each response of a line, in record order. A line without responses
    /// lasts as one empty response does ([`Game::line_seconds`] is their sum).
    pub fn response_lengths(&self, speaker: FormId, info: FormId) -> Vec<f64> {
        self.response_lengths_voiced(crate::functions::base_of(self, speaker), self.speaker_voice_base(speaker), info)
    }

    /// [`Game::response_lengths`] for a speaker placed with `placed_base` speaking with
    /// `voice_base`'s voice ([`Game::response_seconds_voiced`]).
    pub fn response_lengths_voiced(&self, placed_base: Option<FormId>, voice_base: Option<FormId>, info: FormId) -> Vec<f64> {
        let n = self.dialogue_view().info(&info).map_or(0, |d| d.response_chars.len());
        (0..n.max(1)).map(|k| self.response_seconds_voiced(placed_base, voice_base, info, k)).collect()
    }

    /// The base form whose voice `speaker` (a reference) speaks with
    /// ([`Game::voice_base`] of its base).
    pub fn speaker_voice_base(&self, speaker: FormId) -> Option<FormId> {
        crate::functions::base_of(self, speaker).map(|b| self.voice_base(b, speaker))
    }

    /// The voice type (VTYP) actor `r` speaks with: the player's by sex
    /// ([`Game::voice_override`]), else that of the record supplying its traits (VTCK);
    /// a talking activator's is its base's VNAM (#3060) [hypothesis: as its voice folder
    /// is]. `GetIsVoiceType` and "Allow Default Dialog" read it.
    pub fn voice_type_of(&self, r: FormId) -> Option<FormId> {
        let player = (r == FormId::PLAYER_REF).then(|| self.player_voice_type()).flatten();
        player
            .or_else(|| self.traits_def(r).and_then(|d| d.voice))
            .or_else(|| crate::functions::base_of(self, r).and_then(|b| self.data.talking_voices.get(&b).copied()))
    }

    /// The player's voice type: the DefaultObjectManager's Player Voice (Female) while the
    /// player is female, its Player Voice (Male) while male [hypothesis: the game picks
    /// the place by the player's sex; FalloutNV.esm's player record names
    /// PlayerVoiceMale, and `sound\voice\falloutnv.esm\playervoicefemale` holds the same
    /// GenericPlayer Hit, PowerAttack and Death lines as `playervoicemale`, observed;
    /// issue #2088]. A player whose race is a child race (RACE DATA flag Child, the one
    /// `IsChild` reads; SetRace's race included) speaks with Player Voice (Male Child) or
    /// (Female Child) [hypothesis: the same rule for the two child places; Tale of Two
    /// Wastelands plays Fallout 3's childhood in this engine]; when the list names no
    /// child voice, with the adult one of the same sex [design]. `None` when the list
    /// names none (the record's VTCK then).
    pub fn player_voice_type(&self) -> Option<FormId> {
        use crate::data::DefaultObject::{PlayerVoiceFemale, PlayerVoiceFemaleChild, PlayerVoiceMale, PlayerVoiceMaleChild};
        let female = self.actor_female(FormId::PLAYER_REF);
        let child = self.actor_race(FormId::PLAYER_REF).is_some_and(|race| self.data.child_races.contains(&race));
        let (adult, young) = if female { (PlayerVoiceFemale, PlayerVoiceFemaleChild) } else { (PlayerVoiceMale, PlayerVoiceMaleChild) };
        child.then(|| self.data.default_object(young)).flatten().or_else(|| self.data.default_object(adult))
    }

    /// The voice type a speaker whose voice comes from base `voice_base`
    /// ([`Game::speaker_voice_base`]) speaks with instead of its record's: the player's
    /// ([`Game::player_voice_type`]); `None` for every other speaker.
    pub fn voice_override(&self, voice_base: Option<FormId>) -> Option<FormId> {
        (voice_base == Some(FormId::PLAYER_BASE)).then(|| self.player_voice_type()).flatten()
    }

    /// [`Game::response_lengths`] for a speaker given by its base form.
    pub fn response_lengths_as(&self, speaker_base: Option<FormId>, info: FormId) -> Vec<f64> {
        let n = self.dialogue_view().info(&info).map_or(0, |d| d.response_chars.len());
        (0..n.max(1)).map(|k| self.response_seconds_as(speaker_base, info, k)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_placeholder_file_gives_way_to_the_text() {
        // Rex's bull painting line (#4825): 89 characters, a 0.12 s file.
        let chars = 89;
        assert!(is_placeholder(0.12, chars));
        assert_eq!(seconds_with_sound(0.12, chars), fitted_seconds(chars));
        // A real reading keeps its own length, longer or shorter than the fit.
        assert_eq!(seconds_with_sound(4.2, chars), 4.2);
        assert_eq!(seconds_with_sound(9.0, chars), 9.0);
        // One short word said: "Yes." in 0.3 s stays 0.3 s.
        assert!(!is_placeholder(0.3, 4));
        assert_eq!(seconds_with_sound(0.3, 4), 0.3);
        // A near-empty file for an empty text is no placeholder (the fit is 1.1 s).
        assert!(!is_placeholder(0.26, 0));
    }
}
