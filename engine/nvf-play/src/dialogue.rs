//! The dialogue screen (docs/design/dialogue.md step D4; keys, states, the test aid and
//! the checks: docs/design/dialogue-play.md). The conversation itself is the runtime's
//! (`fnv_game::dialogue`, docs/spec-notes/dialogue.md §4–§6): this file shows
//! `Game::dialogue` and turns keys and clicks into `Game::choose`, `Game::skip_response`
//! and `Game::end_dialogue`. The menu is drawn as the original's by
//! `crate::dialogue_menu` (docs/spec-notes/dialogue-menu.md).
//!
//! - **Start.** E on an actor whose default action is to talk starts a conversation with
//!   its greeting (`script::Runtime::talk_after`); so does a script's `Activate player`
//!   on an actor, and scripts start conversations themselves with StartConversation. An
//!   actor with nothing to say starts none (the line "… has nothing to say" and a log
//!   line). Nor does an actor whose greeting has the Goodbye flag when the player talks
//!   to it: it says the line at the player (a subtitle and its voice, like a SayTo line)
//!   and the player is not held [owner statement 2026-10-10; the runtime's rule,
//!   `Game::start_dialogue`]. When the actor force-greets (StartConversation, a Dialogue
//!   package) it is a conversation of that one line, which holds the player until it is
//!   said [owner statement 2026-10-11; `Game::force_greet`].
//! - **State and pausing** (docs/design/dialogue.md, Pausing). While a conversation runs
//!   the game is in [`PlayState::Dialogue`]. Game logic stands still as in the container
//!   and inventory menus and under a script's message box: the player cannot move or
//!   act, and the player's character, the physics world, the game clock, combat and
//!   travelling NPCs ([`WorldHeld`]) hold; scripts are in menu mode by the runtime's own
//!   rule (`Game::in_menu`: MenuMode blocks with code 1009, game time and quest delays
//!   stand still) [owner ruling 2026-10-10: a conversation with the player pauses the
//!   game; lines said outside one, SayTo and Say, do not]. The actor's lines go on in
//!   real time (`Game::tick` advances them in menu mode too).
//!   What the picture does is the switch for every menu, [`MenuPause`] (`--menu-pause`,
//!   `crate::pause`): `living` (the default) keeps everything that is only presentation
//!   running (animations, wind, water, sky, effects). A zoomed conversation locks the view
//!   on the speaker [owner decision 2026-11-13, #2254; `crate::dialogue_look::view_locked`];
//!   without the zoom the player looks around with the mouse while the actor speaks
//!   ([`FreeLook`]; the cursor comes back when choices are offered [owner decision
//!   2026-10-10]); `original` stops the virtual clock, so the picture stands still,
//!   with the cursor free throughout [owner preference 2026-10-10 for `living`: a
//!   deliberate departure from the original's look that keeps what scripts see]. A
//!   message box a result script shows holds the conversation until it is answered (the
//!   runtime's rule); Esc opens the pause menu,
//!   which holds everything, the actor's voice included.
//! - **The actor speaks.** The response being said is shown on a dark band, the actor's
//!   name at the top right (`crate::dialogue_menu`); the log's `subtitle:` line has it as
//!   "Speaker: text" ([`subtitle`]). Space, Enter, Activate (E) or a click skip to the
//!   next response (`Game::skip_response`).
//! - **The player chooses.** The offered choices are listed between two brackets: 1–9
//!   pick one by its place, Up / Down (W / S, the mouse wheel) move the selection, Enter,
//!   Space or Activate pick the selected one, a click picks the one under the cursor. A
//!   choice the actor has answered before is dimmed (`Choice::darkened`); a skill or perk
//!   line carries its tag, and the frame of one the player falls short of is red.
//! - **Leaving.** Tab says goodbye: it picks the offered Goodbye choice. A chosen line
//!   with the Goodbye flag is said on this screen like the lines before it, the player
//!   held, and ends the conversation when it is over [owner statement 2026-11-11, issue
//!   #2157: the last line of a conversation must hold the player like the rest; said in
//!   the world, a quest's line can be interrupted in play]. The runtime keeps #1606's
//!   rule behind a switch nothing here sets (`GameConfig::goodbye_at_once`): with it a
//!   Goodbye choice closes the conversation at once and the actor finishes the line
//!   [observed: owner's playtest 2026-11-05, against the original: choosing a Goodbye
//!   closes the menu at once and the actor finishes the line as the player walks away;
//!   the runtime's rule, `Game::choose`]. Then the screen goes and
//!   the player is free; the line is heard where the actor stands with a subtitle, as a
//!   SayTo line is [design: a line said in the world], and its end script runs when it is
//!   over [hypothesis, docs/spec-notes/dialogue.md §7 item 18]. A log line says so
//!   ([`log_farewell`]). Through a talking activator the line still ends the conversation
//!   when it is over [hypothesis, as before]. At a topic list (the top level)
//!   without a Goodbye choice, Tab leaves it and a log line names the actor (#1595: Julie
//!   Farkas's top level has no Goodbye in the data) [design]; at a line's own choices
//!   without one Tab is refused (a forced conversation; a log line says so).
//! - **The views.** The rules are here; a view only shows them and asks. The hand-built
//!   menu's buttons and the menu drawn from its file (`crate::tiles::dialog_menu`) both come
//!   back through [`ViewInput`]: hover, pick, a step and skip, held to the list they were for
//!   ([`ListId`]), so a click still queued cannot choose from the list that replaced it.
//!
//! Debug switch `NVF_PLAY_DEBUG_TALK_BREAK=1` (off unless set; a development tool, not
//! for a release [owner decision 2026-10-10]): Shift+Tab breaks the conversation off at
//! any point (`Game::end_dialogue`: the line being said is cut, no end script runs, so a
//! quest result can be lost). Without the switch Shift+Tab does nothing.
//!
//! Test aid `NVF_PLAY_TEST_TALK=<when>:<who>[=<step>|<step>…][;…]` ([`parse_talks`]):
//! - `<when>`: seconds on the world aids' clock (`crate::pause::WorldAidClock`), or
//!   `<seconds>@<quest>=<stage>` (after that stage was first seen done), as in
//!   `NVF_PLAY_TEST_SCRIPT`;
//! - `<who>`: a reference (Editor ID, hex FormID, `placed:<base>`) the player activates
//!   then, wherever the crosshair is; or `*`: nothing is started, the steps answer the
//!   next conversation that a script starts;
//! - steps, each once the actor has finished speaking: `<n>` picks the n-th offered
//!   choice (0-based, as `fnvtool dialogue talk`'s `index`), a topic's Editor ID or hex
//!   FormID picks that topic, any other text picks the choice that reads so (ignoring
//!   case; a part of it if only one choice contains it; quotes force text), `leave`
//!   leaves as Tab does, `skip` skips the response being said, and `break` presses
//!   Shift+Tab (which does something only with the debug switch).
//!
//! `NVF_PLAY_TEST_TALK_DELAY=<seconds>` (default [`AID_CHOOSE_SECONDS`]) is how long the
//! choices stay on screen before a step picks one (for screenshots).
//!
//! When the steps run out the aid leaves: by the Goodbye choice if one is offered, else
//! by breaking off. **Unattended runs never wait for input**: a conversation no script
//! answers is left the same way after [`UNATTENDED_CHOOSE_SECONDS`] of choosing, and
//! any conversation after [`UNATTENDED_TALK_LIMIT`].
//!
//! Log, one line each (`dialogue: …`, [`log_event`]): a conversation started, every
//! response said (speaker, topic, line, text), the choices offered, the choice made, a
//! topic added, the conversation ended (reason, lines), and, with the runtime's switch,
//! after a Goodbye choice that the actor finishes the line in the world. The trace log
//! (`NVF_PLAY_SCRIPT_TRACE`) has the same as trace events, and a `talk_input` line for
//! each thing the player or the aid did.

use crate::bindings::{Control, ControlInput};
use crate::dialogue_menu::{ChoiceRow, DialogueMenu, MenuView, TAG_GAP};
use crate::menu::ActiveMenu;
use crate::pause::{MenuPause, Paused};
use crate::phys::PhysState;
use crate::script::{is_script_menu, with_world, ByHand, ScriptState};
use crate::sim::SimState;
use crate::{PlaySet, PlayState, PlayWorld};
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use crate::ui_pointer::PointerState;
use fnv_game::dialogue::{Challenge, Choice};
use fnv_game::trace::{Event, EventKind};
use fnv_game::{Game, GameData};
use fnv_sim::FormId;
use std::collections::{HashMap, VecDeque};

/// Seconds the choices are on screen before the test aid picks one (so the list is
/// drawn and logged first).
pub const AID_CHOOSE_SECONDS: f32 = 0.25;
/// Seconds a response is on screen before a `skip` step skips it.
pub const AID_SKIP_SECONDS: f32 = 0.3;
/// Unattended runs: seconds of choosing after which a conversation that no
/// `NVF_PLAY_TEST_TALK` script answers is left.
pub const UNATTENDED_CHOOSE_SECONDS: f32 = 1.0;
/// Unattended runs: seconds after which any conversation is broken off (a chain of
/// lines that never comes to a choice).
pub const UNATTENDED_TALK_LIMIT: f32 = 600.0;
// ---- the test aid's script (pure) -----------------------------------------------------

/// One step of an `NVF_PLAY_TEST_TALK` script.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// The n-th offered choice, 0-based.
    Index(usize),
    /// A topic (Editor ID or hex FormID), else the choice's text.
    Named(String),
    /// Skip the response being said.
    Skip,
    /// Leave as Tab does (the Goodbye choice), else break off.
    Leave,
    /// Shift+Tab: breaks the conversation off if the debug switch allows it.
    Break,
}

/// One conversation of `NVF_PLAY_TEST_TALK`.
#[derive(Debug, Clone, PartialEq)]
pub struct TalkScript {
    /// Seconds after the start, or after `after`'s stage was first seen done.
    pub at: f32,
    pub after: Option<(String, i32)>,
    /// The reference the player activates; `None` (`*`): the next conversation a script
    /// starts.
    pub who: Option<String>,
    pub steps: Vec<Step>,
}

fn parse_step(s: &str) -> Step {
    let quoted = s.len() >= 2 && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')));
    if quoted {
        return Step::Named(s[1..s.len() - 1].to_string());
    }
    match s.to_ascii_lowercase().as_str() {
        "leave" => Step::Leave,
        "skip" => Step::Skip,
        "break" => Step::Break,
        _ => match s.parse::<usize>() {
            Ok(i) => Step::Index(i),
            Err(_) => Step::Named(s.to_string()),
        },
    }
}

/// Parse `<when>:<who>[=<step>|<step>…]` items separated by `;` (the module notes).
pub fn parse_talks(v: &str) -> Result<Vec<TalkScript>, String> {
    v.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|item| {
            let (when, rest) = item.split_once(':').ok_or_else(|| format!("{item:?}: expected <when>:<who>[=<steps>]"))?;
            let (t, after) = match when.split_once('@') {
                Some((t, gate)) => {
                    let (q, s) = gate.split_once('=').ok_or_else(|| format!("{item:?}: expected <seconds>@<quest>=<stage>"))?;
                    let s = s.trim().parse::<i32>().map_err(|_| format!("{item:?}: bad stage {s:?}"))?;
                    (t, Some((q.trim().to_string(), s)))
                }
                None => (when, None),
            };
            let at: f32 = t.trim().parse().map_err(|_| format!("{item:?}: bad seconds {t:?}"))?;
            let (who, steps) = rest.split_once('=').unwrap_or((rest, ""));
            let who = who.trim();
            if who.is_empty() {
                return Err(format!("{item:?}: expected a reference or * before the steps"));
            }
            let steps = steps.split('|').map(str::trim).filter(|s| !s.is_empty()).map(parse_step).collect();
            Ok(TalkScript { at, after, who: (who != "*").then(|| who.to_string()), steps })
        })
        .collect()
}

/// An offered choice as the picker and the log see it.
#[derive(Debug, Clone, PartialEq)]
pub struct Offer {
    pub topic: FormId,
    /// The topic's Editor ID.
    pub edid: Option<String>,
    pub text: String,
    pub goodbye: bool,
}

fn listed(offers: &[Offer]) -> String {
    offers.iter().enumerate().map(|(i, o)| format!("{i}) {:?} [{}]", o.text, o.edid.as_deref().unwrap_or("?"))).collect::<Vec<_>>().join("; ")
}

/// The offered choice a step names (not for `Skip` and `Leave`).
pub fn pick(step: &Step, offers: &[Offer]) -> Result<usize, String> {
    match step {
        Step::Index(i) if *i < offers.len() => Ok(*i),
        Step::Index(i) => Err(format!("no choice {i} ({} offered: {})", offers.len(), listed(offers))),
        Step::Named(s) => {
            if s.starts_with("0x") || s.starts_with("0X") {
                let t: FormId = s.parse().map_err(|_| format!("bad FormID {s:?}"))?;
                return offers.iter().position(|o| o.topic == t).ok_or_else(|| format!("topic {s} is not offered (offered: {})", listed(offers)));
            }
            if let Some(i) = offers.iter().position(|o| o.edid.as_deref().is_some_and(|e| e.eq_ignore_ascii_case(s))) {
                return Ok(i);
            }
            let want = s.trim().to_lowercase();
            let text = |o: &Offer| o.text.trim().to_lowercase();
            if let Some(i) = offers.iter().position(|o| text(o) == want) {
                return Ok(i);
            }
            let part: Vec<usize> = (0..offers.len()).filter(|&i| text(&offers[i]).contains(&want)).collect();
            match part.as_slice() {
                [i] => Ok(*i),
                [] => Err(format!("no offered topic or choice is {s:?} (offered: {})", listed(offers))),
                _ => Err(format!("{s:?} is part of several choices (offered: {})", listed(offers))),
            }
        }
        Step::Skip | Step::Leave | Step::Break => Err("not a choice".into()),
    }
}

/// The tag of a skill or perk line: "[Speech 40]", "[Speech 37/40]" when the player
/// falls short, "[Speech]" when the line's conditions name no level
/// [documented: GECK wiki Category:Dialogue "AV/Perk", as `fnv_game::dialogue::Challenge`
/// reads it].
pub fn challenge_tag(name: &str, needed: Option<f64>, have: Option<f64>) -> String {
    let n = |x: f64| format!("{}", x.round() as i64);
    match (needed, have) {
        (Some(need), Some(have)) if have < need => format!("[{name} {}/{}]", n(have), n(need)),
        (Some(need), _) => format!("[{name} {}]", n(need)),
        (None, _) => format!("[{name}]"),
    }
}

/// The tag of a Fallout 3 style percentage speech challenge: "[Speech 75%]"
/// [documented: GECK wiki Category:Dialogue "Speech Challenge" (percentage chance of
/// success) and Fallout Wiki "Speech (Fallout 3)" (`[Speech 25%]`); hypothesis: rounding
/// to the nearest whole percent and exact FNV/TTW label format not yet captured in-game].
pub fn speech_challenge_tag(name: &str, chance: f64) -> String {
    let pct = (chance.clamp(0.0, 1.0) * 100.0).round() as i64;
    format!("[{name} {pct}%]")
}

// ---- reading the conversation -----------------------------------------------------------

/// A dialogue text as it is shown (the dialogue menu's line and choices, subtitles):
/// without the acting notes in braces, "{Sadly - her dog died}Okay." reading "Okay."
/// [owner statement 2026-10-11: the original shows no `{…}` content, Ringo's caravan
/// question shows no "{questioning}"; observed: 5,634 of FalloutNV.esm's response
/// texts carry such a note, 4,617 of them at the start]. A note the data closes with
/// ")" instead of "}" ("{KAI-zar)Caesar awaits!", 4 texts) ends there, one never closed
/// runs to the end, and a stray "}" goes [design]. Square brackets stay: "[SUCCEEDED]",
/// "[FAILED]" and "[You hear static.]" are shown by the original [owner statement
/// 2026-10-11 names only braces]. The spaces left around a note are closed up.
pub fn shown_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        if rest[i..].starts_with('}') {
            rest = &rest[i + 1..];
            continue;
        }
        let note = &rest[i + 1..];
        rest = match note.find('}').or_else(|| note.find(')')) {
            Some(j) => &note[j + 1..],
            None => "",
        };
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The name an actor is shown by: its template's or base form's display name, else its
/// Editor ID (`SimState::ref_name`).
fn speaker_name(g: &Game, sim: &SimState, actor: FormId) -> String {
    sim.ref_name(Some(g), actor.0).map(str::to_string).unwrap_or_else(|| g.data.name(actor))
}

/// The text of the response being said in the conversation, if it has any.
fn response_text(g: &Game) -> Option<String> {
    let c = g.dialogue.as_ref()?;
    let l = c.line.as_ref()?;
    let def = g.data.infos.get(&l.info)?;
    let r = *def.response_order().get(l.response)?;
    // The player's tweak keeps the acting notes [design: docs/design/tweaks.md].
    let text = if g.tweaks.keep_acting_notes { g.expand_codes(&def.responses[r].text).trim().to_string() } else { shown_text(&g.expand_codes(&def.responses[r].text)) };
    (!text.is_empty()).then_some(text)
}

pub(crate) fn observe_classic(state: &mut crate::dialogue_touch::classic_state::State, game: &Game) {
    state.observe(game, response_text);
}

/// The subtitle of the response being said in the conversation: "Speaker: text".
pub fn subtitle(g: &Game, sim: &SimState) -> Option<String> {
    let c = g.dialogue.as_ref()?;
    response_text(g).map(|text| format!("{}: {text}", speaker_name(g, sim, c.actor)))
}

fn offers(g: &Game, choices: &[Choice]) -> Vec<Offer> {
    choices
        .iter()
        .map(|c| Offer { topic: c.topic, edid: g.data.edid_of.get(&c.topic).cloned(), text: shown_text(&g.shown_choice_text(c)), goodbye: c.goodbye })
        .collect()
}

/// What a choice reads: a check's tag and the line [observed: two spaces between them on
/// the owner's screenshot]; a topic without a line shows its name.
fn row_label(text: &str, fallback: &str, tag: Option<String>) -> String {
    let text = shown_text(text);
    let text = if text.is_empty() { format!("({fallback})") } else { text };
    match tag {
        Some(t) => format!("{t}{TAG_GAP}{text}"),
        None => text,
    }
}

fn tag_of(g: &Game, c: &Challenge) -> String {
    tag_of_data(g.data, c)
}

fn tag_of_data(data: &GameData, c: &Challenge) -> String {
    if let Some(chance) = c.chance {
        let name = if data.actor_value_infos.contains_key(&c.form) || data.perks.contains_key(&c.form) {
            challenge_name(data, c.form)
        } else {
            // Synthetic load orders without an AVSpeech AVIF record fall back to "Speech"
            // instead of displaying a raw FormID [design].
            "Speech".to_string()
        };
        return speech_challenge_tag(&name, chance);
    }
    challenge_tag(&challenge_name(data, c.form), c.needed, c.have)
}

/// The name a check's tag shows: an actor value's AVIF FULL ("Speech"), a perk's FULL;
/// the Editor ID only when the record has no name. AVIF records are not base forms of
/// the simulation, so their names come from the game data's actor value table
/// [observed: the tag read "[AVSpeech]", the AVIF's Editor ID, on Trudy's Powder Ganger
/// check; owner's playtest list 7, item 10].
pub(crate) fn challenge_name(data: &GameData, form: FormId) -> String {
    let full = data.actor_value_infos.get(&form).map(|a| a.name.as_str()).or_else(|| data.perks.get(&form).map(|p| p.name.as_str()));
    match full {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => data.name(form),
    }
}

/// What the menu shows, in whichever view draws it.
pub(crate) fn view(g: &Game, sim: &SimState) -> Option<MenuView> {
    let c = g.dialogue.as_ref()?;
    view_named(g, speaker_name(g, sim, speaker_form(c)))
}

/// Who the menu names: through a talking activator the activator ("Intercom", not Chris
/// Haversam: owner screenshot 2026-10-22 of the REPCONN intercom).
pub(crate) fn speaker_form(c: &fnv_game::dialogue::Conversation) -> FormId {
    c.through.unwrap_or(c.actor)
}

/// [`view`] with the speaker's name given.
pub(crate) fn view_named(g: &Game, speaker: String) -> Option<MenuView> {
    let c = g.dialogue.as_ref()?;
    // A message box or menu over the conversation takes the input: the menu goes.
    if held(g) {
        return None;
    }
    if c.line.is_some() || c.choices.is_empty() {
        return Some(MenuView { speaker, line: response_text(g), choices: Vec::new(), goodbye: false });
    }
    let choices = c
        .choices
        .iter()
        .map(|ch| {
            let fallback = g.data.name(ch.topic);
            // The player's tweak leaves the skill tags off [design: docs/design/tweaks.md].
            let tag = ch.challenge.as_ref().filter(|_| !g.tweaks.no_skill_tags).map(|t| tag_of(g, t));
            let short = ch.challenge.as_ref().is_some_and(|t| matches!((t.needed, t.have), (Some(n), Some(h)) if h < n));
            ChoiceRow { label: row_label(&g.shown_choice_text(ch), &fallback, tag), darkened: ch.darkened, short }
        })
        .collect();
    // The hint stands where Tab leaves: a Goodbye choice, or a topic list (`resolve`).
    let goodbye = c.topic_list || c.choices.iter().any(|ch| ch.goodbye);
    Some(MenuView { speaker, line: None, choices, goodbye })
}

// ---- the log ----------------------------------------------------------------------------

/// The log lines of the conversation's trace events (called for every event
/// `script::log_event` has no line of its own for).
pub(crate) fn log_event(data: &GameData, e: &Event) {
    let n = |f: FormId| data.name(f);
    match &e.kind {
        EventKind::DialogueStarted { actor, topic } => info!("dialogue: conversation with {} started (topic {})", n(*actor), n(*topic)),
        EventKind::ResponseSaid { speaker, info, number, seconds, .. } => {
            let def = data.infos.get(info);
            let topic = def.and_then(|d| d.topic).map_or_else(|| "?".to_string(), n);
            let (k, of) = def.map_or((0, 0), |d| (d.response_order().iter().position(|&r| d.responses[r].number == *number).map_or(0, |p| p + 1), d.responses.len()));
            let text = def.and_then(|d| d.responses.iter().find(|r| r.number == *number)).map_or("", |r| r.text.trim());
            info!("dialogue: {} says [{topic}] {} response {k}/{of} ({seconds:.2} s): {text:?}", n(*speaker), n(*info));
        }
        EventKind::ChoicesOffered { actor, choices } => {
            let list: Vec<String> = choices
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let check = c.challenge.as_ref().map_or_else(String::new, |t| format!(" (check {})", tag_of_data(data, t)));
                    let marks = format!("{}{}{check}", if c.goodbye { " (goodbye)" } else { "" }, if c.darkened { " (dimmed)" } else { "" });
                    format!("{i}) {:?} [{}]{marks}", data.choice_text(c.topic, c.info), n(c.topic))
                })
                .collect();
            info!("dialogue: {} offers {} choices: {}", n(*actor), choices.len(), list.join("; "));
        }
        EventKind::ChoiceMade { actor, index, topic, info } => {
            info!("dialogue: choice {index} made: {:?} [{}], {} answers with {}", data.choice_text(*topic, *info), n(*topic), n(*actor), n(*info))
        }
        EventKind::TopicAdded { topic, by } => info!("dialogue: topic {} added{}", n(*topic), by.map_or_else(String::new, |b| format!(" by line {}", n(b)))),
        EventKind::DialogueEnded { actor, reason, lines } => info!("dialogue: conversation with {} ended ({reason}, {lines} lines)", n(*actor)),
        EventKind::ActorConversationStarted { speaker, target, topic } => {
            info!("dialogue: conversation between {} and {} started (topic {})", n(*speaker), n(*target), n(*topic))
        }
        EventKind::ActorConversationEnded { speaker, target, reason, lines } => {
            info!("dialogue: conversation between {} and {} ended ({reason}, {lines} lines)", n(*speaker), n(*target))
        }
        EventKind::NoLine { speaker, topic } => debug!("dialogue: {} has no line of {} to say", n(*speaker), n(*topic)),
        _ => {}
    }
}

/// The log line of a conversation that ended at once on the player's Goodbye choice
/// (`Game::choose` with `GameConfig::goodbye_at_once`, off by default since #2157): its
/// actor finishes the line in the world (`Speech::farewell`).
/// Called for every trace event after its own log line. The runtime notes the line as
/// the conversation closes (`Game::farewell_said`), which this takes on that
/// conversation's goodbye `dialogue_ended`, so the line is logged even when something
/// ended it later in the same frame.
pub(crate) fn log_farewell(g: &mut Game, e: &Event) {
    let EventKind::DialogueEnded { actor, reason, .. } = &e.kind else { return };
    if *reason != fnv_game::dialogue::ended::GOODBYE || g.farewell_said.is_none_or(|(speaker, _)| speaker != *actor) {
        return;
    }
    if let Some((speaker, info)) = g.farewell_said.take() {
        info!("dialogue: the conversation ends on the Goodbye choice; {} finishes the line {} in the world", g.data.name(speaker), g.data.name(info));
    }
}

// ---- the screen's state -----------------------------------------------------------------

/// The script that answers the running conversation.
struct Driver {
    actor: FormId,
    steps: VecDeque<Step>,
}

#[derive(Default)]
struct TalkAid {
    /// Scripts whose time has not come.
    waiting: Vec<TalkScript>,
    /// When each gate's stage was first seen done (seconds since the start).
    gates: HashMap<(String, i32), f32>,
    /// `*` scripts whose time has come, waiting for a conversation.
    armed: VecDeque<TalkScript>,
    driver: Option<Driver>,
}

impl TalkAid {
    /// Move the scripts that are due out of `waiting`: `*` scripts are armed; the first
    /// due script with a reference is returned when `start` allows one (no conversation
    /// is running).
    fn due(&mut self, g: &Game, now: f32, start: bool) -> Option<TalkScript> {
        for s in &self.waiting {
            if let Some(gate) = &s.after {
                if !self.gates.contains_key(gate) {
                    let done = fnv_game::runner::resolve(g, &gate.0).ok().is_some_and(|q| g.quests.get(&q).is_some_and(|s| s.stages_done.contains(&gate.1)));
                    if done {
                        self.gates.insert(gate.clone(), now);
                    }
                }
            }
        }
        let gates = &self.gates;
        let is_due = |s: &TalkScript| match &s.after {
            None => now >= s.at,
            Some(gate) => gates.get(gate).is_some_and(|&t| now >= t + s.at),
        };
        let mut named = None;
        let mut i = 0;
        while i < self.waiting.len() {
            let s = &self.waiting[i];
            if !is_due(s) || (s.who.is_some() && (!start || named.is_some())) {
                i += 1;
                continue;
            }
            let s = self.waiting.remove(i);
            if s.who.is_some() {
                named = Some(s);
            } else {
                info!("test aid NVF_PLAY_TEST_TALK: {} steps wait for the next conversation a script starts", s.steps.len());
                self.armed.push_back(s);
            }
        }
        named
    }
}

/// Something else chooses for the player in this run (nvforever's quest pilot): an
/// unattended run then neither leaves a conversation nobody scripted nor breaks a long
/// one off; whoever chooses does both.
static PILOTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// See [`PILOTED`].
pub fn set_piloted(on: bool) {
    PILOTED.store(on, std::sync::atomic::Ordering::Relaxed);
}

fn piloted() -> bool {
    PILOTED.load(std::sync::atomic::Ordering::Relaxed)
}

/// The dialogue screen: the selection, the test aid and what is drawn.
#[derive(Resource, Default)]
pub struct DialogueScreen {
    /// The selected choice.
    selected: usize,
    /// See [`DialogueScreen::framed`].
    framed: bool,
    /// See [`DialogueScreen::keyed`].
    keyed: u64,
    /// The offered list the selection belongs to: the actor and the lines said so far.
    list: Option<(FormId, u32)>,
    aid: TalkAid,
    /// An unattended run: nobody is there to choose.
    unattended: bool,
    /// Seconds the choices are on screen before the aid picks (`NVF_PLAY_TEST_TALK_DELAY`).
    aid_delay: f32,
    /// `NVF_PLAY_DEBUG_TALK_BREAK`: Shift+Tab breaks a conversation off.
    debug_break: bool,
    /// Since when (seconds) the conversation runs, the choices are offered, and the
    /// response (line, position) is being said.
    talk_since: Option<f32>,
    choosing_since: Option<f32>,
    speaking: Option<((FormId, usize), f32)>,
}

impl DialogueScreen {
    /// [design, #2403 / file refs] Logical choice progress from this screen's state;
    /// excludes aid timers and the key counter, which also advances at list boundaries.
    pub fn progress(&self) -> impl std::fmt::Debug + '_ {
        (self.list, self.selected, self.framed)
    }

    fn new(talks: Vec<TalkScript>, unattended: bool) -> DialogueScreen {
        let aid_delay = std::env::var("NVF_PLAY_TEST_TALK_DELAY").ok().and_then(|d| d.trim().parse().ok()).unwrap_or(AID_CHOOSE_SECONDS);
        let debug_break = debug_switch(std::env::var("NVF_PLAY_DEBUG_TALK_BREAK").ok().as_deref());
        if debug_break {
            info!("debug switch NVF_PLAY_DEBUG_TALK_BREAK: Shift+Tab breaks a conversation off (no end script runs)");
        }
        DialogueScreen { aid: TalkAid { waiting: talks, ..default() }, unattended, aid_delay, debug_break, ..default() }
    }

    /// The selected choice, for a view that draws it.
    pub(crate) fn selected(&self) -> usize {
        self.selected
    }

    /// Something chose on this list: the tile view frames nothing until then.
    pub(crate) fn framed(&self) -> bool {
        self.framed
    }

    /// How many times the keys have moved the selection: the tile view scrolls to the choice
    /// they moved it to, and not to a choice the pointer came onto.
    pub(crate) fn keyed(&self) -> u64 {
        self.keyed
    }

    fn conversation_over(&mut self) {
        if let Some(d) = self.aid.driver.take() {
            if !d.steps.is_empty() {
                warn!("test aid NVF_PLAY_TEST_TALK: the conversation ended with {} steps unused: {:?}", d.steps.len(), d.steps);
            }
        }
        (self.selected, self.list, self.framed) = (0, None, false);
        (self.talk_since, self.choosing_since, self.speaking) = (None, None, None);
    }
}

/// [design, #2403] Synthetic navigation states for reports integration tests only.
#[cfg(feature = "report-test-support")]
#[doc(hidden)]
pub fn report_progress_fixture(step: usize) -> DialogueScreen {
    DialogueScreen {
        list: Some((FormId(42), 1)),
        selected: step.min(1),
        framed: true,
        keyed: step as u64,
        choosing_since: Some(step as f32),
        ..Default::default()
    }
}

/// Whether a debug switch's value turns it on: anything but unset, empty, `0`, `off`,
/// `false` and `no`.
pub fn debug_switch(value: Option<&str>) -> bool {
    value.map(|v| v.trim().to_ascii_lowercase()).is_some_and(|v| !matches!(v.as_str(), "" | "0" | "off" | "false" | "no"))
}

/// What the player (or the aid) does this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Act {
    Choose(usize),
    Skip,
    /// Tab: the Goodbye choice; `force`: break off when none is offered.
    Leave { force: bool },
    BreakOff,
    /// Tab at a topic list that offers no Goodbye: the conversation ends there (no line is
    /// being said, so nothing is cut).
    LeaveTopicList,
}

/// What Tab (`Act::Leave`) does: the offered Goodbye choice (`goodbye`, its index); at a
/// topic list without one (`topic_list`), leave it, so no top level traps the player
/// [design, #1595: Julie Farkas's top level has no Goodbye in the data; observer check 12
/// asks whether the original's Tab leaves any topic list]; else, forced, break off, or be
/// refused (a line's own choices without a Goodbye: a forced conversation).
fn resolve(act: Act, goodbye: Option<usize>, topic_list: bool) -> Act {
    match (act, goodbye) {
        (Act::Leave { .. }, Some(i)) => Act::Choose(i),
        (Act::Leave { .. }, None) if topic_list => Act::LeaveTopicList,
        (Act::Leave { force: true }, None) => Act::BreakOff,
        (a, _) => a,
    }
}

/// Which screen of the conversation a view's input was for: its actor, its generation
/// (`fnv_game::dialogue::conversation_stamp`: the conversation, the lines said, the response)
/// and the topics offered (none while a line is being said). A conversation that goes on, a
/// next response, or the same actor's next conversation has another [`ListId`] [design].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListId {
    actor: FormId,
    stamp: u64,
    topics: Vec<FormId>,
}

impl ListId {
    /// The generation, as input queued on this screen is stamped with.
    pub fn stamp(&self) -> u64 {
        self.stamp
    }
}

#[cfg(test)]
impl ListId {
    /// A list for a test.
    pub(crate) fn for_test(stamp: u32, topics: &[u32]) -> ListId {
        ListId { actor: FormId(0x10), stamp: u64::from(stamp), topics: topics.iter().map(|&t| FormId(t)).collect() }
    }
}

/// The running conversation's generation, 0 without one.
pub fn stamp_now(g: &Game) -> u64 {
    fnv_game::dialogue::conversation_stamp(g.dialogue_serial, g.dialogue.as_ref())
}

/// The running conversation's [`ListId`].
pub fn list_id(g: &Game) -> Option<ListId> {
    let c = g.dialogue.as_ref()?;
    let topics = if c.line.is_some() { Vec::new() } else { c.choices.iter().map(|ch| ch.topic).collect() };
    Some(ListId { actor: c.actor, stamp: stamp_now(g), topics })
}

/// What a view of the menu asks of the conversation this frame, for [`input`] to turn into
/// the same commands the keys give. Empty unless a view filled it.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct ViewInput {
    /// The list on screen when this was read: anything else finds this input void.
    pub list: Option<ListId>,
    /// The pointer came onto this choice.
    pub hover: Option<usize>,
    /// This choice was clicked.
    pub pick: Option<usize>,
    /// A click meant to skip the line being said.
    pub skip: bool,
    /// A quick-exit action (such as tapping the Pip-Boy power button) that leaves
    /// dialogue only when a normal exit/goodbye option is available.
    pub leave: bool,
}

impl ViewInput {
    /// This input if it was for the list `now`, else nothing: the list on screen was replaced
    /// after the view read it.
    pub(crate) fn for_list(self, now: Option<&ListId>) -> ViewInput {
        if self.list.as_ref() == now {
            self
        } else {
            ViewInput::default()
        }
    }

    /// The selection after a frame's Up (`up`) and Down (`down`) and this input, over `len`
    /// choices: the keys, the step, then the pointer's choice, each kept in the list.
    fn selection(&self, selected: usize, len: usize, up: bool, down: bool) -> usize {
        let last = len.saturating_sub(1);
        let mut s = selected.min(last);
        if up {
            s = s.saturating_sub(1);
        }
        if down {
            s = (s + 1).min(last);
        }
        match self.hover {
            Some(h) => h.min(last),
            None => s,
        }
    }
}

// ---- plugin and systems -----------------------------------------------------------------

/// A conversation with the player is open: game logic stands still. What runs only
/// while `Playing` holds by itself; this is for what does not know the play state
/// (nvforever's travelling NPCs).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorldHeld(pub bool);

/// The player may look around with the mouse although not `Playing`: while an actor
/// speaks in a conversation that is not zoomed, in [`MenuPause::Living`]
/// (`player::mouse_look`, `player::grab_cursor`; a zoomed one locks the view [owner
/// decision 2026-11-13, #2254]).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FreeLook(pub bool);

pub struct DialoguePlugin;

impl Plugin for DialoguePlugin {
    fn build(&self, app: &mut App) {
        let talks = match std::env::var("NVF_PLAY_TEST_TALK") {
            Ok(v) => match parse_talks(&v) {
                Ok(t) => {
                    info!("test aid NVF_PLAY_TEST_TALK: {} conversation(s)", t.len());
                    t
                }
                Err(e) => {
                    warn!("NVF_PLAY_TEST_TALK: {e}");
                    Vec::new()
                }
            },
            Err(_) => Vec::new(),
        };
        let unattended = crate::test_aid::Unattended::from_env().0 || app.world().resource::<PlayWorld>().auto_walk > 0.0;
        app.insert_resource(DialogueScreen::new(talks, unattended))
            .init_resource::<WorldHeld>()
            .init_resource::<FreeLook>()
            .init_resource::<ViewInput>()
            // After the keys of a script's menu (a key that answers a message box is not
            // also a choice), before the frame's scripts (a choice's line starts in the
            // frame it was made).
            .add_systems(Update, input.in_set(PlaySet::Scripts).after(crate::script_hud::menu_keys).before(crate::script::run_scripts))
            .add_plugins(crate::dialogue_menu::DialogueMenuPlugin)
            .add_systems(Update, (sync_state, draw).chain().in_set(PlaySet::Ui).before(crate::dialogue_menu::DrawMenu));
    }
}

/// Whether a menu is open on top of the conversation (a message box a result script
/// showed, a character-creation menu; `Game::menu`, which a conversation itself never
/// sets): it holds the conversation's lines and takes the keys first.
pub fn held(g: &Game) -> bool {
    g.menu.is_some()
}

/// The test aid's due steps and the player's keys and clicks, as actions on the runtime.
#[allow(clippy::too_many_arguments)]
pub(crate) fn input(
    keys: crate::pad::ActionButtons<KeyCode>,
    controls: ControlInput,
    scroll: Res<AccumulatedMouseScroll>,
    time: (crate::pause::AidTime, Res<Time<Real>>),
    state: Res<State<PlayState>>,
    paused: Res<Paused>,
    phys: Res<PhysState>,
    clicks: Query<(&PointerState, &ChoiceButton), Changed<PointerState>>,
    mut screen: ResMut<DialogueScreen>,
    mut script: ResMut<ScriptState>,
    mut sim: ResMut<SimState>,
    mut view_input: ResMut<ViewInput>,
    touch: Option<ResMut<crate::dialogue_touch::TouchFrame>>,
    classic: Option<ResMut<crate::dialogue_touch::classic_state::State>>,
    classic_menus: crate::dialogue_touch::classic_state::Menus,
) {
    let keys = keys.snapshot();
    use KeyCode::*;
    // Taken whatever happens below: what a view asked is for this frame only.
    let mut asked = std::mem::take(&mut *view_input);
    let touch_active = touch.as_ref().is_some_and(|t| t.active);
    let mut barter = false;
    if let Some(mut touch) = touch.filter(|t| t.active) {
        barter = std::mem::take(&mut touch.barter);
        asked = std::mem::take(&mut touch.asked);
    }
    // Like the scripts: not under the pause menu, not before the player stands in the
    // world.
    if paused.0 || phys.player.is_none() {
        return;
    }
    let sc = &mut *screen;
    let st = &mut *script;
    // The aids' clock: it goes on while the picture stands still (`crate::pause`).
    let now = time.0.elapsed_secs();
    if barter && st.held {
        return;
    }
    let Some(rt) = st.rt.as_mut() else { return };
    if let Some(mut classic) = classic {
        observe_classic(&mut classic, &rt.game);
    }
    if barter {
        if *state.get() == PlayState::Dialogue && !classic_menus.blocked() {
            with_world(rt, &mut sim, |rt| crate::dialogue_touch::classic_state::request(&mut rt.game, asked.list.as_ref()));
        }
        return;
    }
    let mut notes: Vec<serde_json::Value> = Vec::new();

    // The aid's conversations whose time has come, on the world aids' clock
    // (`crate::pause::WorldAidClock`).
    if let Some(s) = time.0.world_secs().and_then(|w| sc.aid.due(&rt.game, w, rt.game.dialogue.is_none())) {
        let who = s.who.clone().unwrap_or_default();
        let r = with_world(rt, &mut sim, |rt| {
            let r = fnv_game::runner::resolve(&rt.game, &who)?;
            rt.activate_as_player(r).map(|done| (r, done))
        });
        let error = match r {
            Ok((r, ByHand::Talk)) => {
                info!("test aid NVF_PLAY_TEST_TALK: the player activated {who} ({r}); {} steps answer the conversation", s.steps.len());
                sc.aid.driver = rt.game.dialogue_actor().map(|actor| Driver { actor, steps: s.steps.into() });
                None
            }
            Ok((r, ByHand::Greeted)) => {
                info!("test aid NVF_PLAY_TEST_TALK: the player activated {who} ({r}); it said its greeting at the player (a Goodbye line), no conversation to answer");
                None
            }
            Ok(_) => Some(format!("activating {who} started no conversation")),
            Err(e) => Some(e),
        };
        if let Some(e) = &error {
            warn!("test aid NVF_PLAY_TEST_TALK: {who}: {e}");
        }
        notes.push(serde_json::json!({"kind": "talk_input", "op": "activate", "who": who, "by": "test aid", "error": error}));
    }

    let Some(conv) = rt.game.dialogue.clone() else {
        sc.conversation_over();
        for n in notes {
            st.trace_note(n);
        }
        return;
    };
    // A driver left over from another conversation is not this one's.
    if sc.aid.driver.as_ref().is_some_and(|d| d.actor != conv.actor) {
        sc.conversation_over();
    }
    if sc.aid.driver.is_none() {
        if let Some(s) = sc.aid.armed.pop_front() {
            info!("test aid NVF_PLAY_TEST_TALK: {} steps answer the conversation with {}", s.steps.len(), rt.game.data.name(conv.actor));
            sc.aid.driver = Some(Driver { actor: conv.actor, steps: s.steps.into() });
        }
    }
    let speaking = conv.line.as_ref().map(|l| (l.info, l.response));
    let choosing = speaking.is_none() && !conv.choices.is_empty();
    let key = (conv.actor, conv.lines);
    if sc.list != Some(key) {
        (sc.list, sc.selected, sc.framed) = (Some(key), 0, false);
    }
    sc.talk_since.get_or_insert(now);
    match speaking {
        Some(k) if sc.speaking.map(|s| s.0) != Some(k) => sc.speaking = Some((k, now)),
        Some(_) => {}
        None => sc.speaking = None,
    }
    if choosing {
        sc.choosing_since.get_or_insert(now);
    } else {
        sc.choosing_since = None;
    }
    // A message box or menu holds the conversation and takes the keys.
    if held(&rt.game) {
        for n in notes {
            st.trace_note(n);
        }
        return;
    }
    let list_now = list_id(&rt.game);
    let mut asked = asked.for_list(list_now.as_ref());
    // What the view asked is held to its list; the hand-built buttons carry theirs below.
    let by_tiles = asked.list.is_some() && !touch_active;
    let offered = offers(&rt.game, &conv.choices);
    let chosen_for = |since: Option<f32>, limit: f32| since.is_some_and(|t| now - t >= limit);

    // The aid.
    let mut act: Option<(Act, &'static str)> = None;
    if let Some(d) = sc.aid.driver.as_mut() {
        match d.steps.front().cloned() {
            Some(Step::Skip) if speaking.is_some() => {
                if sc.speaking.is_some_and(|(_, t)| now - t >= AID_SKIP_SECONDS) {
                    d.steps.pop_front();
                    act = Some((Act::Skip, "test aid"));
                }
            }
            // Nothing is being said: the step is void.
            Some(Step::Skip) => {
                d.steps.pop_front();
            }
            // Shift+Tab, once the response or the choices have been on screen a moment.
            Some(Step::Break) => {
                let shown = sc.speaking.is_some_and(|(_, t)| now - t >= AID_SKIP_SECONDS) || (choosing && chosen_for(sc.choosing_since, sc.aid_delay));
                if shown {
                    d.steps.pop_front();
                    if sc.debug_break {
                        act = Some((Act::BreakOff, "test aid (Shift+Tab)"));
                    } else {
                        info!("test aid NVF_PLAY_TEST_TALK: step Break: Shift+Tab does nothing (NVF_PLAY_DEBUG_TALK_BREAK is not set)");
                    }
                }
            }
            step if choosing && chosen_for(sc.choosing_since, sc.aid_delay) => {
                d.steps.pop_front();
                act = Some(match step {
                    None => {
                        info!("test aid NVF_PLAY_TEST_TALK: no steps left: leaving the conversation");
                        (Act::Leave { force: true }, "test aid")
                    }
                    Some(Step::Leave) => (Act::Leave { force: true }, "test aid"),
                    Some(step) => match pick(&step, &offered) {
                        Ok(i) => {
                            info!("test aid NVF_PLAY_TEST_TALK: step {step:?} picks choice {i}");
                            (Act::Choose(i), "test aid")
                        }
                        Err(e) => {
                            warn!("test aid NVF_PLAY_TEST_TALK: step {step:?}: {e}; leaving the conversation");
                            d.steps.clear();
                            (Act::Leave { force: true }, "test aid")
                        }
                    },
                });
            }
            _ => {}
        }
    } else if sc.unattended && !piloted() && choosing && chosen_for(sc.choosing_since, UNATTENDED_CHOOSE_SECONDS) {
        info!("dialogue: unattended run and no NVF_PLAY_TEST_TALK script for this conversation: leaving it");
        act = Some((Act::Leave { force: true }, "unattended run"));
    }
    if sc.unattended && !piloted() && chosen_for(sc.talk_since, UNATTENDED_TALK_LIMIT) {
        warn!("dialogue: unattended run: the conversation has run for {UNATTENDED_TALK_LIMIT} s: breaking it off");
        act = Some((Act::BreakOff, "unattended run"));
    }

    // The player.
    if act.is_none() && *state.get() == PlayState::Dialogue {
        let shift = keys.any_pressed([ShiftLeft, ShiftRight]);
        let confirm = keys.any_just_pressed([Enter, NumpadEnter, Space]) || controls.just_pressed(Control::Activate);
        if keys.just_pressed(Tab) && shift {
            // Only with the debug switch; otherwise Shift+Tab does nothing (it is not Tab).
            if sc.debug_break {
                act = Some((Act::BreakOff, "Shift+Tab"));
            }
        } else if speaking.is_some() {
            // The player's tweak: spoken lines are not cut short by the player [design: docs/design/tweaks.md].
            let no_skip = rt.game.tweaks.no_skip_dialogue;
            if !no_skip && (confirm || controls.mouse.just_pressed(MouseButton::Left) || asked.skip) {
                act = Some((Act::Skip, if asked.skip { "tile click" } else { "key or click" }));
            }
        } else if choosing {
            let len = conv.choices.len();
            let digits = [Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9];
            let pads = [Numpad1, Numpad2, Numpad3, Numpad4, Numpad5, Numpad6, Numpad7, Numpad8, Numpad9];
            let wheel = scroll.delta.y;
            // The hand-built buttons ask as the tiles do, for the list they were spawned or last
            // brought up to date for.
            for (i, b) in clicks.iter().filter(|(_, b)| !touch_active && b.1 == list_now) {
                match i {
                    PointerState::Pressed => asked.pick = Some(b.0),
                    PointerState::Hovered => asked.hover = Some(b.0),
                    PointerState::None => {}
                }
            }
            let (up, down) = (keys.any_just_pressed([ArrowUp, KeyW]) || wheel > 0.0, keys.any_just_pressed([ArrowDown, KeyS]) || wheel < 0.0);
            // The list drawn from its file opens with nothing framed and the first press frames
            // the first choice [observed: capture of the original, private job dialogue, 1280 x 720: `_highlight_y` -1,
            // then 0, 58, 116 on Down]. The hand-built menu's first choice stands framed at open.
            let first_press = by_tiles && !sc.framed;
            let moved_by_key = up || down;
            let selected = asked.selection(sc.selected, len, up && !first_press, down && !first_press);
            sc.framed |= moved_by_key || asked.hover.is_some();
            sc.keyed += u64::from(moved_by_key);
            let clicked = asked.pick;
            sc.selected = selected;
            if let Some(i) = (0..9).find(|&i| keys.just_pressed(digits[i]) || keys.just_pressed(pads[i])).filter(|&i| i < len) {
                act = Some((Act::Choose(i), "number key"));
            } else if let Some(i) = clicked.filter(|&i| i < len) {
                act = Some((Act::Choose(i), if by_tiles { "tile click" } else { "click" }));
            } else if confirm {
                act = Some((Act::Choose(selected), "Enter"));
            } else if asked.leave || keys.just_pressed(Tab) {
                act = Some((Act::Leave { force: false }, if asked.leave { "power button" } else { "Tab" }));
            }
        }
    }

    if let Some((act, by)) = act {
        let goodbye = offered.iter().position(|o| o.goodbye);
        let act = resolve(act, goodbye, conv.topic_list);
        if act == Act::LeaveTopicList {
            info!("dialogue: {by} leaves {}'s topic list, which offers no Goodbye (#1595: the data's or a condition's gap)", conv.actor);
        }
        let (op, index, error) = with_world(rt, &mut sim, |rt| {
            rt.player_action(|g| match act {
                Act::Choose(i) => ("choose", Some(i), g.choose(i).err()),
                Act::Skip => ("skip", None, (!g.skip_response()).then(|| "nothing to skip".to_string())),
                Act::BreakOff => ("break_off", None, (!g.end_dialogue()).then(|| "no conversation".to_string())),
                Act::LeaveTopicList => ("leave", None, (!g.end_dialogue()).then(|| "no conversation".to_string())),
                Act::Leave { .. } => ("leave", None, Some("this conversation offers no goodbye".to_string())),
            })
        });
        match (&error, act) {
            (Some(e), Act::Leave { .. }) => info!("dialogue: Tab refused: {e}"),
            (Some(e), _) => warn!("dialogue: {op} by {by}: {e}"),
            (None, Act::BreakOff) => info!("dialogue: the conversation is broken off ({by})"),
            (None, _) => info!("dialogue: {op} {index:?} by {by}"),
        }
        notes.push(serde_json::json!({"kind": "talk_input", "op": op, "index": index, "by": by, "error": error}));
    }
    for n in notes {
        st.trace_note(n);
    }
}

/// `PlayState::Dialogue` exactly while a conversation runs, unless an engine menu
/// (container, inventory) or the pause menu is open. Everything that stands still in
/// `Menu` stands still here too (it runs only while `Playing`): the player, physics, the
/// game clock, activation, combat. Runs after `script::hold_player`
/// (which would put the player in `Menu` for a message box): in a conversation the box
/// is answered from the `Dialogue` state, and when the conversation ends under an open
/// box the player goes to `Menu`. The repair services and the recipe menu a line's result
/// script opens (#2308) are `Menu` over the conversation, which waits under them: their
/// own close goes back to `Dialogue` (`crate::repair::after_close`), and a conversation
/// that ends as one opens (a Goodbye line's end script) leaves the player in `Menu`.
#[allow(clippy::too_many_arguments)]
fn sync_state(
    script: Option<Res<ScriptState>>,
    active: Res<ActiveMenu>,
    services: (Option<Res<crate::repair::RepairServices>>, Option<Res<crate::crafting::RecipeMenu>>),
    paused: Res<Paused>,
    mode: Res<MenuPause>,
    state: Res<State<PlayState>>,
    mut next: ResMut<NextState<PlayState>>,
    mut world_held: ResMut<WorldHeld>,
    mut look: ResMut<FreeLook>,
    zoom: Res<crate::dialogue_zoom::DialogueZoom>,
) {
    let game = script.as_ref().and_then(|s| s.rt.as_ref()).map(|rt| &rt.game);
    let talking = game.is_some_and(|g| g.dialogue.is_some());
    let boxed = game.and_then(|g| g.menu).is_some_and(is_script_menu);
    let service = services.0.is_some_and(|r| r.is_open()) || services.1.is_some_and(|r| r.is_open());
    let own_menu = active.0.is_some() || paused.0 || service;
    match (*state.get(), talking) {
        (PlayState::Playing, true) => next.set(PlayState::Dialogue),
        (PlayState::Menu, true) if !own_menu => next.set(PlayState::Dialogue),
        (PlayState::Dialogue, false) => next.set(if boxed || service { PlayState::Menu } else { PlayState::Playing }),
        _ => {}
    }
    world_held.set_if_neq(WorldHeld(talking));
    // Free look while the actor speaks; the cursor while the player chooses or answers a
    // message box.
    let speaking = game.is_some_and(|g| !held(g) && g.dialogue.as_ref().is_some_and(|c| c.line.is_some()));
    // Whether the picture stands still meanwhile is `crate::pause::freeze_picture`'s.
    // The pointer stays free throughout a zoomed conversation: the zoom locks the view
    // [owner decision 2026-11-13, #2254].
    look.set_if_neq(FreeLook(*mode == MenuPause::Living && speaking && *state.get() == PlayState::Dialogue && !crate::dialogue_look::view_locked(&zoom)));
}

// ---- drawing ----------------------------------------------------------------------------

/// A clickable choice, drawn by `crate::dialogue_menu`: its index in the offered list and the
/// list it stands for on screen.
#[derive(Component, Clone, Debug)]
pub struct ChoiceButton(pub usize, pub Option<ListId>);

/// Hand the menu what it shows and the selection.
fn draw(script: Option<Res<ScriptState>>, sim: Res<SimState>, screen: Res<DialogueScreen>, mut menu: ResMut<DialogueMenu>) {
    let v = script.as_ref().and_then(|s| s.rt.as_ref()).and_then(|rt| view(&rt.game, &sim));
    if menu.view != v {
        menu.view = v;
    }
    if menu.selected != screen.selected {
        menu.selected = screen.selected;
    }
    let list = script.as_ref().and_then(|s| s.rt.as_ref()).and_then(|rt| list_id(&rt.game));
    if menu.list != list {
        menu.list = list;
    }
}

#[cfg(test)]
pub(crate) fn install_classic_input_fixture(app: &mut App) {
    app.init_resource::<DialogueScreen>()
        .init_resource::<ViewInput>()
        .init_resource::<Paused>()
        .init_resource::<crate::pause::PlayClock>()
        .add_systems(Update, input.in_set(PlaySet::Scripts))
        .add_systems(Update, draw.in_set(PlaySet::Ui).before(crate::dialogue_menu::DrawMenu));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_leaves_a_topic_list_without_goodbye() {
        let tab = Act::Leave { force: false };
        // A Goodbye choice offered: Tab picks it, at the top level or not.
        assert_eq!(resolve(tab, Some(2), true), Act::Choose(2));
        assert_eq!(resolve(tab, Some(0), false), Act::Choose(0));
        // #1595: Julie Farkas's top level offers none; Tab still leaves it.
        assert_eq!(resolve(tab, None, true), Act::LeaveTopicList);
        // A line's own choices without one: a forced conversation, Tab is refused...
        assert_eq!(resolve(tab, None, false), tab);
        // ...unless forced (the aids), which break off.
        assert_eq!(resolve(Act::Leave { force: true }, None, false), Act::BreakOff);
        assert_eq!(resolve(Act::Choose(1), None, true), Act::Choose(1), "other acts pass through");
    }

    #[test]
    fn talk_scripts_parse() {
        let t = parse_talks("8:EasyPeteRef=0|GREETING| \"what do you do\" |skip|leave; 0@VCG01=80:*=VCG01DocMitchellTopic000|0x00104C5B ;2@VCG01=115:placed:GSTrudy;3:SunnyREF=").unwrap();
        assert_eq!(
            t,
            vec![
                TalkScript {
                    at: 8.0,
                    after: None,
                    who: Some("EasyPeteRef".into()),
                    steps: vec![Step::Index(0), Step::Named("GREETING".into()), Step::Named("what do you do".into()), Step::Skip, Step::Leave],
                },
                TalkScript {
                    at: 0.0,
                    after: Some(("VCG01".into(), 80)),
                    who: None,
                    steps: vec![Step::Named("VCG01DocMitchellTopic000".into()), Step::Named("0x00104C5B".into())],
                },
                TalkScript { at: 2.0, after: Some(("VCG01".into(), 115)), who: Some("placed:GSTrudy".into()), steps: vec![] },
                TalkScript { at: 3.0, after: None, who: Some("SunnyREF".into()), steps: vec![] },
            ]
        );
        assert_eq!(parse_talks("1:*=Break|skip").unwrap()[0].steps, vec![Step::Break, Step::Skip]);
        // The debug switch is off unless it is given a value that is not a "no".
        assert!(!debug_switch(None));
        for off in ["", "0", "off", "FALSE", " no "] {
            assert!(!debug_switch(Some(off)), "{off:?}");
        }
        assert!(debug_switch(Some("1")) && debug_switch(Some("on")));
        // Quotes force text: a number, a keyword.
        assert_eq!(parse_talks("1:*='3'|\"leave\"").unwrap()[0].steps, vec![Step::Named("3".into()), Step::Named("leave".into())]);
        assert!(parse_talks("EasyPeteRef").is_err());
        assert!(parse_talks("x:EasyPeteRef").is_err());
        assert!(parse_talks("3:=0").is_err());
        assert!(parse_talks("3@VCG01:EasyPeteRef").is_err());
        assert!(parse_talks("").unwrap().is_empty());
    }

    fn offer(topic: u32, edid: &str, text: &str, goodbye: bool) -> Offer {
        Offer { topic: FormId(topic), edid: Some(edid.into()), text: text.into(), goodbye }
    }

    #[test]
    fn a_step_picks_by_index_topic_or_text() {
        let o = [
            offer(0x10, "TopicWork", "What do you do here?", false),
            offer(0x11, "TopicTown", "Tell me about the town.", false),
            offer(0x12, "TopicTownHistory", "Tell me about the town's past.", false),
            offer(0x13, "GOODBYE", "Goodbye.", true),
        ];
        assert_eq!(pick(&Step::Index(2), &o), Ok(2));
        assert!(pick(&Step::Index(4), &o).is_err());
        // A topic by Editor ID (any case) or FormID.
        assert_eq!(pick(&Step::Named("topictown".into()), &o), Ok(1));
        assert_eq!(pick(&Step::Named("0x00000012".into()), &o), Ok(2));
        assert!(pick(&Step::Named("0x00000099".into()), &o).is_err());
        // Text: the whole of it, else a part only one choice has.
        assert_eq!(pick(&Step::Named("goodbye.".into()), &o), Ok(3));
        assert_eq!(pick(&Step::Named("what do you do".into()), &o), Ok(0));
        assert_eq!(pick(&Step::Named("town's past".into()), &o), Ok(2));
        let several = pick(&Step::Named("tell me about".into()), &o).unwrap_err();
        assert!(several.contains("several") && several.contains("TopicTownHistory"), "{several}");
        assert!(pick(&Step::Named("caps".into()), &o).unwrap_err().contains("offered"));
        assert!(pick(&Step::Leave, &o).is_err());
        assert!(pick(&Step::Break, &o).is_err());
        assert!(pick(&Step::Index(0), &[]).is_err());
    }

    #[test]
    fn acting_notes_in_braces_are_not_shown() {
        assert_eq!(shown_text("{questioning}You know how to play Caravan?"), "You know how to play Caravan?");
        assert_eq!(shown_text("{Sadly - her dog died}Okay. Well... good luck to you."), "Okay. Well... good luck to you.");
        // In the middle and at the end; the spaces around a note close up.
        assert_eq!(shown_text("Look alive, {beat} guys. {Stern}"), "Look alive, guys.");
        assert_eq!(shown_text("[FAILED] {Angered, cutting the player off}Enough."), "[FAILED] Enough.");
        // A note closed by a parenthesis, a stray brace, a note never closed.
        assert_eq!(shown_text("{Angered}What are you doing standing around, whelp? {KAI-zar)Caesar awaits!"), "What are you doing standing around, whelp? Caesar awaits!");
        assert_eq!(shown_text("{thrilled}}It is? It will?"), "It is? It will?");
        assert_eq!(shown_text("Wait {unfinished"), "Wait");
        // Square brackets stay; text without notes is unchanged but for its ends.
        assert_eq!(shown_text("[You hear static.]"), "[You hear static.]");
        assert_eq!(shown_text("  Howdy.  "), "Howdy.");
        assert_eq!(shown_text("{Laughs}"), "");
        // A choice's row reads without its note too.
        assert_eq!(row_label("{lie} I'm with the NCR.", "TopicLie", None), "I'm with the NCR.");
    }

    fn list(lines: u32, topics: &[u32]) -> ListId {
        ListId::for_test(lines, topics)
    }

    #[test]
    fn a_views_input_holds_for_the_list_it_was_read_on() {
        let asked = ViewInput { list: Some(list(3, &[1, 2])), pick: Some(1), ..default() };
        // The same list: taken as it is.
        assert_eq!(asked.clone().for_list(Some(&list(3, &[1, 2]))).pick, Some(1));
        // A click still queued finds another list void: a line said since, other topics, no
        // conversation.
        for now in [list(4, &[1, 2]), list(3, &[1, 3]), list(3, &[])] {
            assert_eq!(asked.clone().for_list(Some(&now)), ViewInput::default(), "{now:?}");
        }
        assert_eq!(asked.for_list(None), ViewInput::default());
        // Nothing asked stays nothing.
        assert_eq!(ViewInput::default().for_list(Some(&list(3, &[1]))), ViewInput::default());
    }

    #[test]
    fn the_selection_takes_the_keys_the_step_and_the_pointer_in_the_list() {
        let none = ViewInput::default();
        assert_eq!(none.selection(1, 4, false, false), 1);
        assert_eq!(none.selection(1, 4, true, false), 0);
        assert_eq!(none.selection(3, 4, false, true), 3, "the last stays the last");
        assert_eq!(none.selection(0, 4, true, false), 0, "the first stays the first");
        // A list that got shorter since.
        assert_eq!(none.selection(7, 4, false, false), 3);
        // The pointer has the last word, inside the list.
        let hover = |h| ViewInput { hover: Some(h), ..default() };
        assert_eq!(hover(2).selection(0, 4, false, true), 2);
        assert_eq!(hover(9).selection(0, 4, false, false), 3);
        // No choices: nothing to overrun.
        assert_eq!(none.selection(0, 0, true, true), 0);
    }

    #[test]
    fn rows_and_tags_read_as_documented() {
        assert_eq!(challenge_tag("Speech", Some(40.0), Some(52.0)), "[Speech 40]");
        assert_eq!(challenge_tag("Speech", Some(40.0), Some(37.0)), "[Speech 37/40]");
        assert_eq!(challenge_tag("Speech", Some(40.0), None), "[Speech 40]");
        assert_eq!(challenge_tag("Terrifying Presence", None, None), "[Terrifying Presence]");
        assert_eq!(speech_challenge_tag("Speech", 0.0), "[Speech 0%]");
        assert_eq!(speech_challenge_tag("Speech", 0.424), "[Speech 42%]");
        assert_eq!(speech_challenge_tag("Speech", 0.426), "[Speech 43%]");
        assert_eq!(speech_challenge_tag("Speech", 1.0), "[Speech 100%]");
        assert_eq!(speech_challenge_tag("Speech", 1.5), "[Speech 100%]");
        assert_eq!(speech_challenge_tag("Speech", -0.2), "[Speech 0%]");
        assert_eq!(row_label(" Goodbye. ", "GOODBYE", None), "Goodbye.");
        assert_eq!(row_label("Trust me", "TopicSkill", Some("[Speech 40]".into())), "[Speech 40]  Trust me");
        assert_eq!(row_label("Tell me where he is.", "MegMoriarty", Some("[Speech 35%]".into())), "[Speech 35%]  Tell me where he is.");
        // A topic without a name still shows something to pick.
        assert_eq!(row_label("", "TopicNameless", None), "(TopicNameless)");
    }

    #[test]
    fn speech_challenge_choice_tag_uses_percentage_and_respects_no_skill_tags() {
        let lo = fnv_esm::LoadOrder::new(&[]).unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
        let schema = fnv_esm::Schema::load_dir(root.join("fnv")).unwrap();
        let functions = root.join("script/functions.json");
        let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&functions).unwrap()).unwrap();
        let forms = fnv_sim::FormStore::build(&lo, &schema);
        let refs = fnv_sim::RefTable::build(&lo, &schema);
        let mut data = GameData::build(&lo, &schema, fnv_script::FunctionTable::load(&functions).unwrap(), &json, &refs);
        data.actor_value_infos.insert(
            FormId(0x0000_04BB),
            fnv_game::data::ActorValueInfo { name: "Speech".into(), short_name: "Speech".into(), ..Default::default() },
        );
        let fo3_with_avif = Challenge { form: FormId(0x0000_04BB), difficulty: Some(3), chance: Some(0.45), needed: None, have: None, skill: Some(43) };
        let fo3_missing_avif = Challenge { form: FormId(0), difficulty: Some(4), chance: Some(0.14175), needed: None, have: None, skill: Some(43) };
        let nv_check = Challenge { form: FormId(0x0000_04BB), difficulty: None, chance: None, needed: Some(50.0), have: Some(35.0), skill: Some(43) };
        assert_eq!(tag_of_data(&data, &fo3_with_avif), "[Speech 45%]");
        assert_eq!(tag_of_data(&data, &fo3_missing_avif), "[Speech 14%]");
        assert_eq!(tag_of_data(&data, &nv_check), "[Speech 35/50]");

        // Full view_named path: percentage tag shown with short = false, hidden under no_skill_tags,
        // while a New Vegas check under threshold stays short = true.
        let world = fnv_sim::World::new(&forms, &refs, fnv_sim::SimConfig::default());
        let mut g = Game::new(&data, world, fnv_game::game::GameConfig::default());
        g.dialogue = Some(fnv_game::dialogue::Conversation {
            actor: FormId(0x10),
            through: None,
            line: None,
            choices: vec![
                Choice { topic: FormId(0x20), info: FormId(0x21), chars: 7, prompt: false, goodbye: false, darkened: false, challenge: Some(fo3_with_avif), dumb: false },
                Choice { topic: FormId(0x22), info: FormId(0x23), chars: 7, prompt: false, goodbye: false, darkened: false, challenge: Some(nv_check), dumb: false },
            ],
            topic_list: true,
            lines: 1,
            then: None,
            ending: false,
        });
        let shown = view_named(&g, "Moriarty".into()).unwrap();
        assert_eq!(shown.choices.len(), 2);
        assert!(shown.choices[0].label.starts_with("[Speech 45%]  "), "{}", shown.choices[0].label);
        assert!(!shown.choices[0].short);
        assert!(shown.choices[1].label.starts_with("[Speech 35/50]  "), "{}", shown.choices[1].label);
        assert!(shown.choices[1].short);

        g.tweaks.no_skill_tags = true;
        let hidden = view_named(&g, "Moriarty".into()).unwrap();
        assert!(!hidden.choices[0].label.contains("[Speech"));
        assert!(!hidden.choices[1].label.contains("[Speech"));
    }
}
