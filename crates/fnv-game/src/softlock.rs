//! The soft-lock watchdog (issue #2365): the player is held and nothing in the game is
//! moving on to let them go.
//!
//! All of it is [design]: the original has no such check. A soft lock is a hold that no
//! key ends, so it is found by what the game's own state says, not by what is drawn:
//!
//! | kind | the hold | limit |
//! |---|---|---|
//! | `controls_held` | scripts have the movement controls off (`DisablePlayerControls`), or the player is restrained, outside any conversation or menu, and no story event has happened | [`CONTROLS_SECS`] |
//! | `dialogue_dead_end` | a conversation is open with no line being said and no choice offered | [`DEAD_END_SECS`] |
//! | `line_never_ends` | one response of a conversation has been the line being said | [`LINE_SECS`] |
//! | `menu_never_opens` | a menu the scripts opened holds a conversation and the caller says no screen answers for it (#2308: ShowRepairMenu from a line's result script) | [`MENU_SECS`] |
//!
//! **Story events** ([`story`]) are the game's own events that say a scene goes on: quest
//! stages and objectives, references enabled, disabled, moved or placed, activations,
//! messages, menus, conversations and their lines, image modifiers, knock-outs. Not among
//! them: variables and globals (a timer that counts forever would hide the hold), package
//! choices (the residents of a town make them all day), chatter ([`Game::chatter_topic`]: hellos and the lines of Combat and Detection topics,
//! which say nothing about the scene; a fight beside a held player went on for a minute
//! with no finding before they were left out), and items changing hands between others
//! (a fighter's spent rounds). The lines of a conversation between two actors do count,
//! like any other line that is not chatter: a scene among actors is a scene going on, so
//! a town's talk beside a held player starts the wait again [design]. `Game::emit` counts
//! them in [`Seen`].
//!
//! The clock is the caller's: seconds in which the game ran (not the pause menu, not a
//! loading screen). One finding per hold; it is armed again when anything of the hold
//! changes. A `controls_held` finding is made once until the player is free again (the
//! movement controls back and not restrained): a story event or a menu part way through
//! one hold starts the wait again but makes no second finding for it, so a DLC's load
//! message box answered while the controls stay off is not a second soft lock [design,
//! #3676].

use crate::game::Game;
use crate::trace::EventKind;
use fnv_sim::FormId;

/// Seconds the movement controls may be held with no story event. The longest waits
/// scripts are known to make with the controls off are a few seconds (fades, the 4 s
/// before Hardin's training package, #2309); an actor walking up to the player takes
/// longer, hence the margin.
pub const CONTROLS_SECS: f64 = 30.0;
pub const DEAD_END_SECS: f64 = 5.0;
pub const LINE_SECS: f64 = 120.0;
pub const MENU_SECS: f64 = 5.0;

/// How many waiting actors a finding names.
const NAMED: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    ControlsHeld,
    DialogueDeadEnd,
    LineNeverEnds,
    MenuNeverOpens,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::ControlsHeld => "controls_held",
            Kind::DialogueDeadEnd => "dialogue_dead_end",
            Kind::LineNeverEnds => "line_never_ends",
            Kind::MenuNeverOpens => "menu_never_opens",
        }
    }
}

/// The limits, seconds (the test aids shorten them).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    pub controls: f64,
    pub dead_end: f64,
    pub line: f64,
    pub menu: f64,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { controls: CONTROLS_SECS, dead_end: DEAD_END_SECS, line: LINE_SECS, menu: MENU_SECS }
    }
}

impl Limits {
    fn of(&self, kind: Kind) -> f64 {
        match kind {
            Kind::ControlsHeld => self.controls,
            Kind::DialogueDeadEnd => self.dead_end,
            Kind::LineNeverEnds => self.line,
            Kind::MenuNeverOpens => self.menu,
        }
    }
}

/// What `Game::emit` keeps for the watchdog. Not saved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seen {
    /// Story events so far.
    pub story: u64,
    /// The last quest stage set.
    pub stage: Option<(FormId, i32)>,
    /// The last [`LINES_KEPT`] lines (INFO) anyone began to say, oldest first: for a
    /// caller that waits for an actor to say one (the quest pilot).
    pub lines: std::collections::VecDeque<FormId>,
    /// How many lines were begun in all: a caller keeps the count to know which of
    /// [`Seen::lines`] are new to it ([`Seen::lines_since`]).
    pub lines_begun: u64,
}

impl Seen {
    /// The lines begun since the count was `mark` (the kept ones of them).
    pub fn lines_since(&self, mark: u64) -> impl Iterator<Item = FormId> + '_ {
        let new = self.lines_begun.saturating_sub(mark).min(self.lines.len() as u64) as usize;
        self.lines.iter().skip(self.lines.len() - new).copied()
    }
}

pub const LINES_KEPT: usize = 4096;

/// Whether an event says the scene goes on (module notes). `chatter`: the event is a
/// line of a chatter topic ([`Game::chatter_topic`]).
pub fn story(kind: &EventKind, chatter: bool) -> bool {
    use EventKind::*;
    match kind {
        LineStarted { .. } => !chatter,
        ItemAdded { to: who, .. } | ItemRemoved { from: who, .. } => *who == FormId::PLAYER_REF,
        QuestStarted { .. }
        | QuestStopped { .. }
        | QuestCompleted { .. }
        | QuestFailed { .. }
        | StageSet { .. }
        | ObjectiveDisplayed { .. }
        | ObjectiveCompleted { .. }
        | RefEnabled { .. }
        | RefDisabled { .. }
        | Moved { .. }
        | Placed { .. }
        | Activated { .. }
        | MessageShown { .. }
        | NoteShown { .. }
        | MenuOpened { .. }
        | MenuClosed { .. }
        | DialogueStarted { .. }
        | ResponseSaid { .. }
        | ChoicesOffered { .. }
        | ChoiceMade { .. }
        | DialogueEnded { .. }
        | ImageModifierApplied { .. }
        | ImageModifierRemoved { .. }
        | KnockedOut { .. }
        | GotUp { .. } => true,
        _ => false,
    }
}

/// What only the caller knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outside {
    /// The menu in `Game::menu` has a screen the player can answer. The headless runner,
    /// which answers menus itself, says true.
    pub menu_shown: bool,
}

impl Default for Outside {
    fn default() -> Outside {
        Outside { menu_shown: true }
    }
}

/// A hold and everything that, changing, means it is not the same hold any more.
#[derive(Debug, Clone, PartialEq)]
struct Hold {
    kind: Kind,
    story: u64,
    conversation: u64,
    menu: Option<u16>,
    boxes: usize,
    locks: [bool; 7],
    restrained: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub kind: Kind,
    /// Seconds the hold has lasted.
    pub seconds: f64,
    /// What holds the player and what the game seems to wait for.
    pub text: String,
}

#[derive(Debug, Default)]
pub struct Watchdog {
    pub limits: Limits,
    hold: Option<Hold>,
    held: f64,
    fired: bool,
    /// A `controls_held` finding was made and the player has not been free since.
    controls_reported: bool,
}

impl Watchdog {
    pub fn new(limits: Limits) -> Watchdog {
        Watchdog { limits, ..Default::default() }
    }

    /// One frame of `dt` seconds in which the game ran. A finding once per hold, when it
    /// has lasted its limit.
    pub fn step(&mut self, g: &Game, dt: f64, outside: Outside) -> Option<Finding> {
        if g.is_dead(FormId::PLAYER_REF) || !(g.controls_locked()[0] || player_restrained(g)) {
            self.controls_reported = false;
        }
        let now = hold(g, outside);
        if now != self.hold {
            (self.hold, self.held, self.fired) = (now, 0.0, false);
            return None;
        }
        let kind = self.hold.as_ref()?.kind;
        self.held += dt.max(0.0);
        if self.fired || self.held < self.limits.of(kind) {
            return None;
        }
        self.fired = true;
        if kind == Kind::ControlsHeld {
            if self.controls_reported {
                return None;
            }
            self.controls_reported = true;
        }
        Some(Finding { kind, seconds: self.held, text: describe(g, kind) })
    }
}

impl Game<'_> {
    /// A topic whose lines say nothing about a scene: HELLO, and the Combat and Detection
    /// topics (attack shouts, alarms, "who's there").
    pub fn chatter_topic(&self, topic: FormId) -> bool {
        use crate::data::topic_type::{COMBAT, DETECTION};
        self.hello_topic() == Some(topic) || self.data.topic_defs.get(&topic).is_some_and(|t| matches!(t.topic_type, COMBAT | DETECTION))
    }
}

fn player_restrained(g: &Game) -> bool {
    g.extra.get(&FormId::PLAYER_REF).is_some_and(|e| e.restrained)
}

fn hold(g: &Game, outside: Outside) -> Option<Hold> {
    // A dead player is held for good by design.
    if g.is_dead(FormId::PLAYER_REF) {
        return None;
    }
    let locks = g.controls_locked();
    let restrained = player_restrained(g);
    let kind = match (&g.dialogue, g.menu) {
        (Some(_), Some(_)) if !outside.menu_shown => Kind::MenuNeverOpens,
        // A menu with a screen: the player's turn.
        (_, Some(_)) => return None,
        (Some(c), None) if c.line.is_some() => Kind::LineNeverEnds,
        // Choosing: the player's turn. Inside a line's result script neither is set, and
        // the script ends within the frame.
        (Some(c), None) if !c.choices.is_empty() => return None,
        (Some(_), None) => Kind::DialogueDeadEnd,
        // A line that is not chatter still being said is the scene going on: a SayTo line
        // counts once when it begins, and Honest Hearts' end slides narrate one of five
        // responses, about 43 s, while the controls are held [observed: pilot run
        // 2026-11-17, INFO 0x02010117]; the hold was found at 30 s, mid-line.
        (None, None) if (locks[0] || restrained) && g.speech.iter().any(|s| s.remaining > 0.0 && !g.chatter_topic(s.topic)) => return None,
        (None, None) if locks[0] || restrained => Kind::ControlsHeld,
        (None, None) => return None,
    };
    let conversation = crate::dialogue::conversation_stamp(g.dialogue_serial, g.dialogue.as_ref());
    Some(Hold { kind, story: g.softlock_seen.story, conversation, menu: g.menu, boxes: g.boxes.len(), locks, restrained })
}

/// The groups of `Game::controls_locked`, in its order.
const GROUPS: [&str; 7] = ["movement", "Pip-Boy", "fighting", "POV", "looking", "rollover", "sneaking"];

fn describe(g: &Game, kind: Kind) -> String {
    let mut out = match kind {
        Kind::ControlsHeld => {
            let off: Vec<&str> = GROUPS.iter().zip(g.controls_locked()).filter(|(_, l)| *l).map(|(n, _)| *n).collect();
            let mut s = String::new();
            if !off.is_empty() {
                s += &format!("scripts hold the controls ({})", off.join(", "));
            }
            if player_restrained(g) {
                s += if s.is_empty() { "the player is restrained" } else { " and the player is restrained" };
            }
            s + ", outside any conversation or menu, and no story event has happened"
        }
        Kind::DialogueDeadEnd => match &g.dialogue {
            Some(c) => format!("the conversation with {} has no line being said and no choice offered after {} lines", g.data.name(c.actor), c.lines),
            None => String::new(),
        },
        Kind::LineNeverEnds => match g.dialogue.as_ref().and_then(|c| c.line.as_ref().map(|l| (c.actor, l.response))) {
            Some((actor, response)) => format!("{} has been saying response {} of one line", g.data.name(actor), response),
            None => String::new(),
        },
        Kind::MenuNeverOpens => {
            let who = g.dialogue.as_ref().map(|c| g.data.name(c.actor)).unwrap_or_default();
            format!("menu {} holds the conversation with {who} and no screen answers for it", g.menu.unwrap_or(0))
        }
    };
    if let Some((quest, stage)) = g.softlock_seen.stage {
        out += &format!("; last stage set: {} {stage}", g.data.name(quest));
    }
    if kind == Kind::ControlsHeld {
        let waiting = waiting_actors(g);
        if !waiting.is_empty() {
            out += &format!("; script packages: {}", waiting.join(", "));
        }
    }
    out
}

/// The actors a script gave a package (the ones a scene most often waits for), as
/// `actor (package[, restrained])`, at most [`NAMED`] and how many more.
fn waiting_actors(g: &Game) -> Vec<String> {
    let all: Vec<(FormId, FormId, bool)> = g.extra.iter().filter(|(r, e)| **r != FormId::PLAYER_REF && !e.dead).filter_map(|(r, e)| Some((*r, e.script_package?, e.restrained))).collect();
    let mut out: Vec<String> = all.iter().take(NAMED).map(|(r, p, restrained)| format!("{} ({}{})", g.data.name(*r), g.data.name(*p), if *restrained { ", restrained" } else { "" })).collect();
    if all.len() > NAMED {
        out.push(format!("{} more", all.len() - NAMED));
    }
    out
}
