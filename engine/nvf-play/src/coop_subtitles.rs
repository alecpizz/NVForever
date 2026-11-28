//! Subtitles of the host's NPC lines on a co-op guest [observed: buckallobill, in game:
//! the guest hears the NPC's lines but sees no subtitles].
//!
//! The lines the host's NPCs say reach a guest as speech of its own
//! (`Game::set_remote_speech`, `Speech::remote`); their text is learned
//! here when they first come, as this game learns the text of a line it starts itself
//! (`crate::script`, on `LineStarted`), so the HUD subtitles them like any line said
//! nearby: only within hearing (`crate::voice::audible`), in time with the voice.
//!
//! Outside co-op no line is remote, so this does nothing.

use crate::script::ScriptState;
use crate::script_hud::ScriptHud;
use crate::PlaySet;
use bevy::prelude::*;

pub struct CoopSubtitlesPlugin;

impl Plugin for CoopSubtitlesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, learn_remote_lines.in_set(PlaySet::Ui).run_if(resource_exists::<ScriptState>).run_if(resource_exists::<ScriptHud>));
    }
}

/// The text of each remote line this game has not started itself.
fn learn_remote_lines(script: Res<ScriptState>, mut hud: ResMut<ScriptHud>, play: Option<Res<crate::PlayWorld>>) {
    let Some(rt) = script.rt.as_ref() else { return };
    for s in rt.game.speech.iter().filter(|s| s.remote) {
        if hud.knows_line(s.info) {
            continue;
        }
        let mut texts = play.as_deref().map(|p| crate::script::responses(p, s.info)).unwrap_or_default();
        if texts.is_empty() {
            texts = rt.game.data.infos.get(&s.info).map(|d| d.responses.iter().map(|r| r.text.clone()).collect()).unwrap_or_default();
        }
        debug!("co-op subtitle: learned the text of {}", rt.game.data.name(s.info));
        hud.remember_line(s.info, texts);
    }
}
