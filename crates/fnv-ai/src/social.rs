//! Actors who talk on their own (docs/spec-notes/dialogue.md §5.2): an actor says hello
//! to the player as the player passes, and an actor goes up to another near it and starts
//! a random conversation. What is said is the game's (`Game::say_hello`,
//! `Game::start_actor_conversation`); when, and who, is decided here.
//!
//! Who may: the actor's current package allows it. A package with the general flag
//! "Skip Fallout Behavior" (bit 12) allows only the Fallout behaviours it sets ("Hellos to
//! player", "Random conversations"); a package without it leaves the actor the default,
//! where both are allowed [documented: Category:Packages, "Enable Fallout Behavior": the
//! two behaviours; observed: in FalloutNV.esm the behaviour flags of the 2,478 packages
//! without bit 12 are 0 or always 0x1CD, those of the 1,685 with it vary; owner statement
//! 2026-10-11: Vault 3's Fiends talk a lot, and their packages
//! (DefaultSandboxEditorLocation512, DefaultPatrolCasual) have bit 12 clear and no
//! behaviour flag set; hypothesis: what bit 12 means]. Only Sandbox, Guard, Eat and
//! Patrol packages start random conversations [documented: Category:Packages]; a
//! Sandbox or Wander package with "No Conversation" takes part in none [documented:
//! xEdit, the flag's name]. Nobody talks while asleep (a Sleep package), on its way,
//! talking already, or carrying out a Dialogue package [hypothesis].
//!
//! When:
//! - **Hello**: when the player comes within [`HELLO_DISTANCE`] of the actor, with
//!   nothing solid between the two (`Game::greet_sight`, the line a force greet needs:
//!   an actor behind a wall waits until the player is in the open); again only
//!   after the player has gone beyond [`HELLO_REARM`] times that distance [stand-ins:
//!   the game's distance and repeat rules are not in the documentation or the masters].
//!   Idle lines (hellos and idle chatter) keep apart: after one, nobody in the area says
//!   another for [`IDLE_LINE_GAP`] seconds, and the actor that said it none for
//!   [`IDLE_LINE_AGAIN`]. An actor the player comes up to while the area waits has
//!   missed its hello, as one that has said it (so a crowd does not greet one after
//!   another once the wait is over). No line said of late is said again, by anyone
//!   (`Game::say_hello_fresh`, `fnv_game::dialogue::IDLE_LINE_MEMORY`) [design: owner's
//!   playtest 2026-11-11, #2156: NPCs "way too talkative", the same line from everyone
//!   in the Legion's Fort, idle lines through walls, "the proximity cut down 33%"].
//! - **Random conversation**: the loaded area tries one after a time drawn between
//!   `fAISocialTimerForConversationsMin` and `…Max` seconds of play (30 and 60 in
//!   FalloutNV.esm), with a chance of `fAISocialchanceForConversation` per cent
//!   (`…Interior` indoors; 25 both); the starter is drawn among the actors that may
//!   start one, and one who has rested since its last conversation for a time drawn the
//!   same way [observed: the settings' values in FalloutNV.esm; the original's rate,
//!   docs/spec-notes/npc-behaviour-compare.md §5: 4 conversations in 6 game hours of
//!   Westside, one per 180 real seconds as one timer of 30–60 s at 25 % gives, where a
//!   timer per actor gives about ten times more, and a timer in game seconds a hundred;
//!   hypothesis: one timer for the area, and real seconds rather than game minutes,
//!   which the four do not tell apart]. The partner is the
//!   nearest actor that may take part, for whom the starter has a HELLO line (Link From
//!   blank) and to whom it has a way, within `fAISocialRadiusToTriggerConversationInterior`
//!   (1000) indoors and `fAISocialRadiusToTriggerConversation` outdoors (not in the
//!   masters: the indoor value stands in). Only an actor the player stands next to
//!   starts one: the player within [`CHAT_NEAR_PLAYER`] of the starter or its partner, in
//!   the same space [owner's playtest list 5 item 28: "NPC idle conversations trigger too
//!   far away: the player must stand next to them"; the distance is a stand-in].
//!
//! **The walk** ([`Chat`]): the starter goes up to the partner, who waits where it stands
//! and turns to it, and they talk once the starter is within [`TALK_DISTANCE`]; a way
//! that ends farther off (the navmesh goes no nearer) gives the conversation up [hypothesis:
//! the original's start at a median of 120 units, npc-behaviour-compare.md §8] [documented: Sandbox Package, "Conversation -- no
//! duration, the behavior is done when the conversation is finished"; StartConversation:
//! "the calling actor will seek out the target actor"; the distance: the standard
//! Activate Distance of a Dialogue package, iActivatePickLength 150 (Dialogue Package);
//! hypothesis: the same for random conversations, and that the partner waits]. A walk
//! that takes longer than [`CHAT_WALK_SECONDS`], a partner that goes, dies or starts
//! talking to someone else, or a package that changes gives the conversation up
//! [stand-in]. Meanwhile neither does what its package has it do; afterwards each goes
//! back to its package's place, a sandboxing actor going on from where it is
//! (`Ai::back_to_package`, as after a push) [hypothesis].
//!
//! **Lines of the Fallout behaviours** (packages.md §7.7), each allowed by its package
//! as hellos are ([`allows`]), said in the world (`Game::say`, the player as the
//! conditions' target), never by an actor in a combat state:
//! - **Idle chatter** ("Allow idle chatter using the IdleChatter topic" [documented:
//!   Category:Packages]; IdleChatter is "said by actors on a random basis who aren't in
//!   combat" [documented: Conversation Tab]): the loaded area tries one after a time
//!   drawn between `fIdleChatterCommentTimer` and `fIdleChatterCommentTimerMax` (15 and
//!   60 in FalloutNV.esm) [hypothesis: the settings' meaning; one timer for the area as
//!   for random conversations]; an actor within [`HELLO_DISTANCE`] of the player, with a
//!   clear line to it, is drawn and says its line, under the idle lines' waits above.
//! - **Reaction to player actions** (the PLAYER… topics [documented: Topics Tab]: "the
//!   player fires their weapon near an actor", "throws a grenade near an actor who isn't
//!   in a combat state", "places a mine near an actor"): when the player's body starts a
//!   gun's shot, a throw or a mine (`Game::anim_state`), the nearest actor within
//!   [`REACT_DISTANCE`] with a line says it, at most once per `fCommentOnPlayerActionsTimer`
//!   (60) for the area [hypothesis: the setting's meaning]. The masters have lines only
//!   for PLAYERFIREWEAPON, PLAYERTHROWGRENADE and PLAYERLAYMINE [observed].
//! - **Observe combat** ("ObserveCombat: actors in "spectator" mode -- watching a combat
//!   but not joining in" [documented: Misc Tab]): an actor within [`OBSERVE_DISTANCE`]
//!   of an actor in Combat, and within [`REACT_DISTANCE`] of the player, says its line,
//!   each actor at most once per `fCommentOnPlayerActionsTimer` [stand-in]. Spectators
//!   "run away if they get too close" [documented: GECK wiki Packages; no distance is
//!   documented]: an actor whose package allows the behaviour, not in a combat state,
//!   within [`SPECTATOR_FLEE_DISTANCE`] of a fighter [design: stand-in] runs from it,
//!   wherever the player is, by the combat flee's search for a spot (a door, cover, or
//!   straight away: `crate::alert`) without entering a combat state; at its spot it
//!   stands facing the fight, runs again (at most once per
//!   [`SPECTATOR_REFLEE_SECONDS`]) if the fight comes as close again, and goes back to
//!   its package once no fighter is within [`OBSERVE_DISTANCE`] [design].
//!
//! While two actors talk, and while an actor says hello or a SayTo line, they turn to
//! whom they talk to (`Game::facings`, which says who turns; [hypothesis] for hellos and
//! conversations); the engine keeps whoever waits or talks where it stands
//! ([`Ai::holds_for_chat`]).

use crate::alert::{Search, FLEE_WALK_SECONDS};
use crate::carry::{Activity, Ai, AiEvent, Talking};
use crate::goal::{self, dist};
use crate::sandbox::Rng;
use fnv_game::game::Game;
use fnv_game::anim_state::AnimAction;
use fnv_game::package::{fallout, flags, type_flags, PackageDef, PackageType};
use fnv_nav::follow::{heading_delta, heading_of, turn_towards};
use fnv_nav::{NavGraph, Place, Space};
use fnv_sim::FormId;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// The player is greeted, and chattered at, from this close, game units [design: owner
/// 2026-11-11, #2156: a third closer than the 300 it was].
pub const HELLO_DISTANCE: f32 = 200.0;
/// An actor that said hello greets again once the player has gone this many times
/// [`HELLO_DISTANCE`] away [stand-in: 600 units, as before #2156].
pub const HELLO_REARM: f32 = 3.0;
/// Seconds of play between two idle lines (hellos, idle chatter) in the loaded area
/// [design: owner 2026-11-11, #2156].
pub const IDLE_LINE_GAP: f64 = 20.0;
/// Seconds of play before an actor that said an idle line says another [design: same].
pub const IDLE_LINE_AGAIN: f64 = 60.0;
/// Two actors start talking this close, game units [the standard Activate Distance of a
/// Dialogue package, iActivatePickLength (Dialogue Package); hypothesis for random
/// conversations].
pub const TALK_DISTANCE: f32 = 150.0;
/// A random conversation starts only with the player this close to the starter or its
/// partner, game units [stand-in: the owner's "right next to them", playtest list 5 item
/// 28; 300 units, the hello distance until #2156 brought hellos closer].
pub const CHAT_NEAR_PLAYER: f32 = 300.0;
/// A walk up to a partner that takes longer than this, seconds, is given up [stand-in].
pub const CHAT_WALK_SECONDS: f64 = 30.0;
/// The settings' values when the load order does not set them [the indoor values of
/// FalloutNV.esm].
const TIMER_MIN: f32 = 30.0;
const TIMER_MAX: f32 = 60.0;
const CHANCE: f32 = 25.0;
const RADIUS: f32 = 1000.0;
/// `fIdleChatterCommentTimer` and `…Max` when the load order does not set them [their
/// values in FalloutNV.esm].
const CHATTER_MIN: f32 = 15.0;
const CHATTER_MAX: f32 = 60.0;
/// `fCommentOnPlayerActionsTimer` when the load order does not set it [its value in
/// FalloutNV.esm].
const COMMENT_SECONDS: f32 = 60.0;
/// An actor reacts to what the player does from this close, game units [stand-in: "near
/// an actor"; the indoor random-conversation radius].
pub const REACT_DISTANCE: f32 = 1000.0;
/// An actor watches a fight this close to one of its fighters, game units [stand-in].
pub const OBSERVE_DISTANCE: f32 = 1500.0;
/// A spectator runs from a fighter this close, game units [design: stand-in; no source
/// documents the original's distance (docs/handoff/observer-checks.md); well inside
/// [`OBSERVE_DISTANCE`], so it watches before it runs].
pub const SPECTATOR_FLEE_DISTANCE: f32 = 600.0;
/// A spectator at its flee spot runs again, the fight come as close, at most this often,
/// seconds [design: so a cornered one does not search every frame].
pub const SPECTATOR_REFLEE_SECONDS: f64 = 5.0;
/// WEAP DNAM animation types that shoot (pistols, rifles, the launcher) [documented: xEdit
/// wbWeaponAnimTypeEnum: 3 Pistol Ballistic … 9 Launcher].
const GUN_ANIM_TYPES: std::ops::RangeInclusive<i64> = 3..=9;

/// The packages that start random conversations [documented: Category:Packages].
const STARTERS: [PackageType; 4] = [PackageType::Sandbox, PackageType::Guard, PackageType::Eat, PackageType::Patrol];

/// Where an actor is with a random conversation ([`crate::Agent::chat`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "chat", rename_all = "snake_case")]
pub enum Chat {
    /// Walking up to `partner`, since `since` (`Game::time`).
    Going { partner: FormId, since: f64 },
    /// Waiting where it stands for `starter` to come up.
    Waiting { starter: FormId },
    /// In the conversation with `with` (said by the game, `Game::speech`).
    Talking { with: FormId },
}

pub(crate) struct Social {
    /// When (`Game::time`) the loaded area next tries a random conversation.
    next_try: Option<f64>,
    /// Until when (`Game::time`) an actor rests after a conversation before it starts
    /// another.
    next_chat: BTreeMap<FormId, f64>,
    /// Actors that said hello, or missed it, and wait for the player to go away.
    greeted: BTreeSet<FormId>,
    /// When (`Game::time`) the loaded area may next hear an idle line ([`IDLE_LINE_GAP`]).
    next_idle_line: f64,
    /// Until when each actor that said an idle line says no other ([`IDLE_LINE_AGAIN`]).
    quiet: BTreeMap<FormId, f64>,
    pub(crate) rng: Rng,
    pub(crate) stats: SocialStats,
    /// When the loaded area next tries idle chatter.
    next_chatter: Option<f64>,
    /// When an actor may next comment on what the player does.
    next_comment: f64,
    /// Until when each actor that commented on a fight watches quietly.
    observed: BTreeMap<FormId, f64>,
    /// Spectators running from a fight: when they last set off.
    fleeing: BTreeMap<FormId, f64>,
    /// The player's body last frame: a gun's shot going, a throw going.
    player_was: (bool, bool),
    /// The Looks turning bodies, by actor: the target, how far the turn is, and the body's
    /// heading (radians; a walker's own heading stays its route's).
    looks: BTreeMap<FormId, (FormId, LookTurn, f32)>,
}

/// Where the body of an actor held by a Look with `rotateBody` is.
#[derive(Debug, Clone, Copy, PartialEq)]
enum LookTurn {
    Turning,
    Facing,
    Held(&'static str),
}

/// A body held by a Look that is further than this off its target is logged as turning
/// again, radians [design: a few degrees, so a target moving about is followed without a
/// log line every frame; the body itself follows at any angle].
pub const LOOK_TURN_FROM: f32 = 0.1;
/// How fast a Look with `rotateBody` turns the body, radians a second [observed: RingoRef
/// turned 0 to 89 degrees at 140-145 degrees a second, evenly from the first frame, AI
/// observer campaign `knockdown_look` 2026-10-19, docs/design/script-stubs.md section 3].
pub const LOOK_TURN_RATE: f32 = 145.0 * std::f32::consts::PI / 180.0;

/// Counts for the log's summary, to set the conversation rate against the original's
/// (docs/spec-notes/npc-behaviour-compare.md §5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct SocialStats {
    /// Seconds summed over the actors in the loaded area within [`CHAT_NEAR_PLAYER`] of
    /// the player (actor-seconds of play).
    pub near_player_seconds: f64,
    /// Times a starter was due with someone to talk to next to the player.
    pub tries: u64,
    /// Of these, the times the chance was won.
    pub chances_won: u64,
}

impl Default for Social {
    fn default() -> Self {
        Social {
            next_try: None,
            next_chat: BTreeMap::new(),
            greeted: BTreeSet::new(),
            next_idle_line: 0.0,
            quiet: BTreeMap::new(),
            rng: Rng(0x5EED_C0DE),
            stats: SocialStats::default(),
            next_chatter: None,
            next_comment: 0.0,
            observed: BTreeMap::new(),
            fleeing: BTreeMap::new(),
            player_was: (false, false),
            looks: BTreeMap::new(),
        }
    }
}

/// Whether a package allows a Fallout behaviour (`fallout::HELLOS_TO_PLAYER`,
/// `fallout::RANDOM_CONVERSATIONS`): see the module notes.
pub fn allows(def: &PackageDef, behaviour: u16) -> bool {
    !def.has(flags::SKIP_FALLOUT_BEHAVIOR) || def.fallout_flags & behaviour != 0
}

/// Whether a package lets its actor take part in random conversations.
pub fn chats(def: &PackageDef) -> bool {
    allows(def, fallout::RANDOM_CONVERSATIONS) && !def.sandbox_flag(type_flags::NO_CONVERSATION)
}

/// An actor that may talk now, with where it is and its package.
struct Free<'a> {
    actor: FormId,
    at: Place,
    def: &'a PackageDef,
    /// On furniture: it may be gone up to, but does not walk.
    seated: bool,
}

fn setting(game: &Game, name: &str, default: f32) -> f32 {
    game.world.forms.setting_f32(name).unwrap_or(default)
}

/// Whether an actor is in a combat state (Alert, Combat, Lost).
fn in_combat(game: &Game, actor: FormId) -> bool {
    game.awareness.state(actor).in_combat()
}

/// The fighter nearest `at` other than `actor`, in its space: who, where, how far.
fn nearest_fighter(fighters: &[(FormId, Place)], actor: FormId, at: Place) -> Option<(FormId, [f32; 3], f32)> {
    fighters.iter().filter(|(id, p)| *id != actor && p.space == at.space).map(|(id, p)| (*id, p.pos, dist(p.pos, at.pos))).min_by(|a, b| a.2.total_cmp(&b.2))
}

/// Where a spectator at `at` runs from: the middle of the fighters within
/// [`OBSERVE_DISTANCE`] of it, so that it runs from the whole fight and not into its far
/// side (the nearest fighter's place when it is alone) [design: review of #1216].
fn fight_centre(fighters: &[(FormId, Place)], actor: FormId, at: Place) -> [f32; 3] {
    let near: Vec<[f32; 3]> = fighters.iter().filter(|(id, p)| *id != actor && p.space == at.space && dist(p.pos, at.pos) <= OBSERVE_DISTANCE).map(|(_, p)| p.pos).collect();
    let n = near.len().max(1) as f32;
    let sum = near.iter().fold([0.0f32; 3], |s, p| [s[0] + p[0], s[1] + p[1], s[2] + p[2]]);
    if near.is_empty() {
        at.pos
    } else {
        [sum[0] / n, sum[1] / n, sum[2] / n]
    }
}

/// Whether the player's foe is within [`OBSERVE_DISTANCE`] of the player at `p`, in the
/// same space: a resident by where it walks; another by `seen`, its place (cell and
/// position) when it is present and alive. A foe with no place is near no one (#3223: a
/// hunter left in a cell no longer loaded, its Combat against the player kept, had
/// townsfolk comment on a fight from anywhere), and one in another space is not near
/// however close its coordinates (#4891: two Powder Gangers left in Combat in Cell Block
/// A of the NCR Correctional Facility, whose interiors overlap in x and y, kept the
/// player "in a fight" in the visitor center, and Meyers ran from it and would not talk).
fn foe_near(resident: Option<Place>, seen: Option<Place>, p: Place) -> bool {
    resident.or(seen).is_some_and(|f| f.space == p.space && dist(f.pos, p.pos) <= OBSERVE_DISTANCE)
}

/// Whether an actor is in the world and alive.
fn here(game: &Game, actor: FormId) -> bool {
    game.present(actor) && !game.extra.get(&actor).is_some_and(|e| e.dead)
}

impl Ai {
    /// The counts of random conversations for the log's summary.
    pub fn social_stats(&self) -> SocialStats {
        self.social.stats
    }

    /// Whether the engine should keep `actor` where it stands for a random conversation:
    /// it waits for its partner, or talks.
    pub fn holds_for_chat(&self, actor: FormId) -> bool {
        self.agents.get(&actor).is_some_and(|a| matches!(a.chat, Some(Chat::Waiting { .. } | Chat::Talking { .. })))
    }

    /// One frame of hellos and random conversations (module notes). Call after the
    /// update that moved the actors; nothing happens while a menu, a loading screen or a
    /// conversation with the player holds the game.
    pub fn social(&mut self, game: &mut Game, nav: &NavGraph, dt: f32) {
        if game.menu.is_some() || game.loading || game.dialogue.is_some() {
            return;
        }
        self.chats_on(game, nav);
        self.face_listeners(game, dt);
        // A script's Look turning the body wins over facing a listener: last.
        self.face_looks(game, dt);
        let now = game.time;
        let mut free: Vec<Free> = Vec::new();
        for (&actor, a) in &self.agents {
            let Some(def) = a.package.and_then(|p| game.package_def(&p)) else { continue };
            let busy = a.activity == Activity::Travelling || a.talk.is_some() || a.chat.is_some() || a.search.is_some() || def.ty == PackageType::Sleep;
            // Only actors in the loaded area talk (`crate::process`).
            if busy || !self.is_high(actor) || !here(game, actor) || game.is_talking(actor) || game.in_actor_conversation(actor) {
                continue;
            }
            free.push(Free { actor, at: Place { space: a.walker.space, pos: a.walker.feet }, def, seated: a.using.is_some() });
        }
        let player = goal::place_of_ref(game, FormId::PLAYER_REF);
        let anywhere = self.cfg.chats_anywhere;
        let near_player = |at: &Place| player.is_some_and(|p| p.space == at.space && dist(p.pos, at.pos) <= CHAT_NEAR_PLAYER);
        let near = self.agents.iter().filter(|(id, a)| self.is_high(**id) && here(game, **id) && near_player(&Place { space: a.walker.space, pos: a.walker.feet })).count();
        self.social.stats.near_player_seconds += near as f64 * f64::from(dt);
        let mut events: Vec<(FormId, AiEvent)> = Vec::new();
        // Hellos.
        if let Some(p) = player {
            for f in &free {
                let away = if f.at.space == p.space { dist(f.at.pos, p.pos) } else { f32::INFINITY };
                if away > HELLO_DISTANCE * HELLO_REARM {
                    self.social.greeted.remove(&f.actor);
                }
                if away > HELLO_DISTANCE || self.social.greeted.contains(&f.actor) || !allows(f.def, fallout::HELLOS_TO_PLAYER) || game.is_talking(f.actor) {
                    continue;
                }
                // Behind a wall or a closed door: it greets once the player is in the open.
                if !game.greet_sight(f.actor, FormId::PLAYER_REF) {
                    continue;
                }
                // Greeted, or its hello missed: another idle line is being said or was
                // said just now, or it has said one itself of late.
                self.social.greeted.insert(f.actor);
                if game.speech.iter().any(|s| s.hello) || !self.may_say_idle_line(f.actor, now) {
                    continue;
                }
                if let Some(line) = game.say_hello_fresh(f.actor) {
                    self.said_idle_line(f.actor, now);
                    events.push((f.actor, AiEvent::Hello { line, away }));
                }
            }
        }
        if let Some(p) = player {
            self.idle_chatter(game, p, &mut events);
            self.player_actions(game, p, &mut events);
            self.observe_combat(game, nav, p, &mut events);
        }
        for (actor, e) in events {
            self.log(game, actor, e);
        }
        // Random conversations, only next to the player: one try for the whole loaded
        // area when its timer is due.
        let (lo, hi) = (setting(game, "fAISocialTimerForConversationsMin", TIMER_MIN), setting(game, "fAISocialTimerForConversationsMax", TIMER_MAX));
        let due = *self.social.next_try.get_or_insert_with(|| now + self.social.rng.range(lo, hi) as f64);
        if now < due {
            return;
        }
        // Who may start one: its package lets it, it is not resting after a conversation,
        // and it has someone within the radius, the one or the other next to the player.
        let mut starters: Vec<(&Free, Vec<(f32, &Free)>)> = Vec::new();
        for f in &free {
            // Not one that has just said hello either.
            if !STARTERS.contains(&f.def.ty) || !chats(f.def) || self.social.next_chat.get(&f.actor).is_some_and(|&t| now < t) || game.is_talking(f.actor) {
                continue;
            }
            let indoors = matches!(f.at.space, Space::Interior(_));
            let interior = setting(game, "fAISocialRadiusToTriggerConversationInterior", RADIUS);
            let radius = if indoors { interior } else { setting(game, "fAISocialRadiusToTriggerConversation", interior) };
            let mut near: Vec<(f32, &Free)> = free
                .iter()
                .filter(|o| o.actor != f.actor && o.at.space == f.at.space && chats(o.def) && (anywhere || near_player(&f.at) || near_player(&o.at)))
                .map(|o| (dist(o.at.pos, f.at.pos), o))
                .filter(|(d, _)| *d <= radius)
                .collect();
            near.sort_by(|a, b| a.0.total_cmp(&b.0));
            if !near.is_empty() {
                starters.push((f, near));
            }
        }
        // Nobody to talk to next to the player: the try is due again as soon as there is
        // (the timer is not drawn again, so a player who comes up hears one soon).
        if starters.is_empty() {
            return;
        }
        self.social.next_try = Some(now + self.social.rng.range(lo, hi) as f64);
        self.social.stats.tries += 1;
        let indoors = matches!(starters[0].0.at.space, Space::Interior(_));
        let chance = setting(game, if indoors { "fAISocialchanceForConversationInterior" } else { "fAISocialchanceForConversation" }, CHANCE);
        if self.social.rng.next() * 100.0 >= chance {
            return;
        }
        self.social.stats.chances_won += 1;
        // The starter is drawn; one without a line for anyone near passes the try on to
        // the next drawn.
        while !starters.is_empty() {
            let i = ((self.social.rng.next() * starters.len() as f32) as usize).min(starters.len() - 1);
            let (f, near) = starters.swap_remove(i);
            for (away, o) in near {
                // A seated starter does not get up to go to someone.
                if away > TALK_DISTANCE && f.seated {
                    continue;
                }
                if game.conversation_opener(f.actor, o.actor).is_none() {
                    continue;
                }
                let started = if away <= TALK_DISTANCE { self.chat_start(game, f.actor, o.actor, away) } else { self.chat_set_off(game, nav, f.actor, o.actor, o.at, away) };
                if started {
                    return;
                }
            }
        }
    }

    /// Whether `actor` may say an idle line now: the area's wait since the last one is
    /// over, and its own (module notes).
    fn may_say_idle_line(&self, actor: FormId, now: f64) -> bool {
        now >= self.social.next_idle_line && self.social.quiet.get(&actor).is_none_or(|&t| now >= t)
    }

    /// `actor` said an idle line: the waits start.
    fn said_idle_line(&mut self, actor: FormId, now: f64) {
        self.social.quiet.retain(|_, until| now < *until);
        self.social.next_idle_line = now + IDLE_LINE_GAP;
        self.social.quiet.insert(actor, now + IDLE_LINE_AGAIN);
    }

    /// Idle chatter (module notes): when the area's timer is due, an actor next to the
    /// player that may chatter says its IdleChatter line.
    fn idle_chatter(&mut self, game: &mut Game, player: Place, events: &mut Vec<(FormId, AiEvent)>) {
        let now = game.time;
        let (lo, hi) = (setting(game, "fIdleChatterCommentTimer", CHATTER_MIN), setting(game, "fIdleChatterCommentTimerMax", CHATTER_MAX));
        let due = *self.social.next_chatter.get_or_insert_with(|| now + self.social.rng.range(lo, hi) as f64);
        if now < due || now < self.social.next_idle_line || game.speech.iter().any(|s| s.hello) {
            return;
        }
        let Some(topic) = game.data.by_edid("IdleChatter").filter(|t| game.data.topic_defs.contains_key(t)) else { return };
        let mut can: Vec<(FormId, f32)> = self
            .onlookers(game, player, fallout::ALLOW_IDLE_CHATTER)
            .into_iter()
            .filter(|(a, _, d)| *d <= HELLO_DISTANCE && self.may_say_idle_line(*a, now) && game.greet_sight(*a, FormId::PLAYER_REF))
            .map(|(a, _, d)| (a, d))
            .collect();
        // Nobody next to the player: due again as soon as someone is.
        if can.is_empty() {
            return;
        }
        self.social.next_chatter = Some(now + self.social.rng.range(lo, hi) as f64);
        while !can.is_empty() {
            let i = ((self.social.rng.next() * can.len() as f32) as usize).min(can.len() - 1);
            let (actor, away) = can.swap_remove(i);
            if let Some(line) = game.say_chatter(actor, topic) {
                self.said_idle_line(actor, now);
                events.push((actor, AiEvent::IdleChatter { line, away }));
                return;
            }
        }
    }

    /// Reactions to what the player does (module notes): a gun's shot, a grenade's throw
    /// or a mine laid, seen as it starts on the player's body.
    fn player_actions(&mut self, game: &mut Game, player: Place, events: &mut Vec<(FormId, AiEvent)>) {
        let body = game.anim_state(FormId::PLAYER_REF).cloned().unwrap_or_default();
        let gun = game.weapon_equipped(FormId::PLAYER_REF).and_then(|w| game.data.weapon_anim_types.get(&w)).is_some_and(|t| GUN_ANIM_TYPES.contains(t));
        // The shot, its follow-through and return (GetAnimAction 2 to 4): one frame of 2
        // alone could fall between two looks.
        let firing = gun && matches!(body.action, Some(AnimAction::Attack | AnimAction::FollowThrough | AnimAction::AttackReturn));
        let (was_firing, was_throwing) = std::mem::replace(&mut self.social.player_was, (firing, body.throwing));
        let topic = if body.throwing && !was_throwing {
            if body.mine {
                "PLAYERLAYMINE"
            } else {
                "PLAYERTHROWGRENADE"
            }
        } else if firing && !was_firing {
            "PLAYERFIREWEAPON"
        } else {
            return;
        };
        let now = game.time;
        if now < self.social.next_comment {
            return;
        }
        let Some(t) = game.data.by_edid(topic).filter(|t| game.data.topic_defs.contains_key(t)) else { return };
        let mut near: Vec<(FormId, Place, f32)> = self.onlookers(game, player, fallout::REACTION_TO_PLAYER_ACTIONS);
        near.sort_by(|a, b| a.2.total_cmp(&b.2));
        for (actor, _, away) in near {
            if let Some(line) = game.say(actor, None, t, false) {
                self.social.next_comment = now + setting(game, "fCommentOnPlayerActionsTimer", COMMENT_SECONDS) as f64;
                events.push((actor, AiEvent::PlayerAction { topic, line, away }));
                return;
            }
        }
    }

    /// Who in the loaded area may comment on what happens near the player now, with where
    /// each is and how far from the player: within [`REACT_DISTANCE`], its package
    /// allowing `behaviour`, not asleep, talking or in a combat state. Walking ones too: a
    /// shot or a fight is reacted to on the way.
    fn onlookers(&self, game: &Game, player: Place, behaviour: u16) -> Vec<(FormId, Place, f32)> {
        self.agents
            .iter()
            .filter(|(id, a)| a.walker.space == player.space && self.is_high(**id) && here(game, **id) && a.chat.is_none() && a.talk.is_none())
            .filter(|(_, a)| a.package.and_then(|p| game.package_def(&p)).is_some_and(|d| allows(d, behaviour) && d.ty != PackageType::Sleep))
            .filter(|(id, _)| !in_combat(game, **id) && !game.is_talking(**id))
            .map(|(id, a)| (*id, Place { space: a.walker.space, pos: a.walker.feet }, dist(a.walker.feet, player.pos)))
            .filter(|(_, _, d)| *d <= REACT_DISTANCE)
            .collect()
    }

    /// The fighters now: residents in Combat, and the player while someone in Combat (not
    /// Alert or Lost: a search is no fight) fights it within [`OBSERVE_DISTANCE`] of it
    /// (its foe may be no resident: then where the world has it while present and alive;
    /// a foe with no place, or in another space, is near no one: [`foe_near`]).
    fn fighters(&self, game: &Game, player: Option<Place>) -> Vec<(FormId, Place)> {
        use fnv_game::awareness::State;
        let mut fighters: Vec<(FormId, Place)> = self
            .agents
            .iter()
            .filter(|(id, _)| game.awareness.state(**id) == State::Combat && here(game, **id))
            .map(|(id, a)| (*id, Place { space: a.walker.space, pos: a.walker.feet }))
            .collect();
        let fought = |p: Place| {
            game.awareness.actors.iter().any(|(id, w)| {
                let resident = self.agents.get(id).map(|a| Place { space: a.walker.space, pos: a.walker.feet });
                // Where the world has it (its cell and position), while present and alive.
                let seen = goal::place_of_ref(game, *id).filter(|_| here(game, *id));
                let near = foe_near(resident, seen, p);
                w.state == State::Combat && w.targets.contains_key(&FormId::PLAYER_REF) && near
            })
        };
        if let Some(p) = player.filter(|p| fought(*p)) {
            fighters.push((FormId::PLAYER_REF, p));
        }
        fighters
    }

    /// Whether `actor` takes a side in a fight of `fighter` rather than watching it: the
    /// player's teammates and those following someone (a Follow or Accompany package) for
    /// the player, and an ally or friend of the fighter by their factions (who will join
    /// it) [design: review of #1216].
    fn takes_sides(&self, game: &Game, actor: FormId, fighter: FormId) -> bool {
        let follows = self.agents.get(&actor).and_then(|a| a.package).and_then(|p| game.package_def(&p)).is_some_and(|d| matches!(d.ty, PackageType::Follow | PackageType::Accompany));
        game.is_teammate(actor) || follows || fnv_game::functions::faction_relation(game, actor, fighter) >= 2
    }

    /// Comments on a fight nearby, and spectators running from one too close (module
    /// notes).
    fn observe_combat(&mut self, game: &mut Game, nav: &NavGraph, player: Place, events: &mut Vec<(FormId, AiEvent)>) {
        let now = game.time;
        self.social.observed.retain(|_, until| now < *until);
        let fighters = self.fighters(game, Some(player));
        if fighters.is_empty() {
            return;
        }
        if let Some(topic) = game.data.by_edid("ObserveCombat").filter(|t| game.data.topic_defs.contains_key(t)) {
            for (actor, at, _) in self.onlookers(game, player, fallout::OBSERVE_COMBAT) {
                if self.social.observed.contains_key(&actor) {
                    continue;
                }
                let Some((fighter, _, away)) = nearest_fighter(&fighters, actor, at).filter(|f| f.2 <= OBSERVE_DISTANCE) else { continue };
                // Tried once per wait, said or not, so an actor without a line does not
                // look for one every frame.
                self.social.observed.insert(actor, now + setting(game, "fCommentOnPlayerActionsTimer", COMMENT_SECONDS) as f64);
                if let Some(line) = game.say(actor, None, topic, false) {
                    events.push((actor, AiEvent::ObserveCombat { fighter, line, away }));
                    break;
                }
            }
        }
        // Spectators too close run, wherever the player is.
        let close: Vec<(FormId, FormId, [f32; 3], f32)> = self
            .agents
            .iter()
            .filter(|(id, a)| self.is_high(**id) && here(game, **id) && a.chat.is_none() && a.talk.is_none() && a.search.is_none() && !self.social.fleeing.contains_key(id))
            .filter(|(_, a)| a.package.and_then(|p| game.package_def(&p)).is_some_and(|d| allows(d, fallout::OBSERVE_COMBAT) && d.ty != PackageType::Sleep))
            .filter(|(id, _)| !in_combat(game, **id) && !game.is_talking(**id))
            .filter_map(|(id, a)| {
                let place = Place { space: a.walker.space, pos: a.walker.feet };
                let (fighter, _, away) = nearest_fighter(&fighters, *id, place)?;
                (away <= SPECTATOR_FLEE_DISTANCE && !self.takes_sides(game, *id, fighter)).then(|| (*id, fighter, fight_centre(&fighters, *id, place), away))
            })
            .collect();
        for (actor, fighter, at, away) in close {
            self.spectator_flee(game, nav, actor, fighter, at, away);
        }
    }

    /// A spectator sets off from the fight at `at` ([`fight_centre`]; `fighter` the
    /// nearest of it), `away` units off.
    fn spectator_flee(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, fighter: FormId, at: [f32; 3], away: f32) {
        if self.agents[&actor].using.is_some() {
            self.set_using(game, actor, None);
            self.log(game, actor, AiEvent::Furniture { furniture: None });
        }
        self.social.fleeing.insert(actor, game.time);
        self.log(game, actor, AiEvent::SpectatorFlee { fighter, away });
        self.flee_from(game, nav, actor, fighter, at);
    }

    /// One update of a spectator running from a fight (module notes), out of a combat
    /// state; whether it holds the actor (its package waits meanwhile).
    pub(crate) fn spectator_step(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId, dt: f32) -> bool {
        let Some(&set_off) = self.social.fleeing.get(&actor) else { return false };
        let now = game.time;
        let a = &self.agents[&actor];
        let place = Place { space: a.walker.space, pos: a.walker.feet };
        match a.search {
            Some(Search::Fleeing { from, to, since }) => {
                if a.activity != Activity::Travelling || now - since > FLEE_WALK_SECONDS {
                    let a = self.agents.get_mut(&actor).expect("listed");
                    a.walker.follower = None;
                    a.activity = Activity::There;
                    let feet = a.walker.feet;
                    a.search = Some(Search::Fled { from, at: feet });
                    let hides = game.sneaks(actor);
                    self.log(game, actor, AiEvent::FleeArrived { from, seconds: (now - since) as f32, off: dist(feet, to), hides });
                }
                true
            }
            Some(Search::Fled { .. }) => {
                let player = goal::place_of_ref(game, FormId::PLAYER_REF);
                let fighters = self.fighters(game, player);
                match nearest_fighter(&fighters, actor, place) {
                    Some((fighter, _, away)) if away <= SPECTATOR_FLEE_DISTANCE && now - set_off >= SPECTATOR_REFLEE_SECONDS => {
                        let at = fight_centre(&fighters, actor, place);
                        self.spectator_flee(game, nav, actor, fighter, at, away);
                    }
                    Some((_, at, away)) if away <= OBSERVE_DISTANCE => {
                        self.face(game, actor, Place { space: place.space, pos: at }, dt);
                    }
                    _ => {
                        self.social.fleeing.remove(&actor);
                        self.search_over(game, nav, actor, "the fight it ran from is over");
                        return false;
                    }
                }
                true
            }
            // Something else took it over (a fight of its own, and back out of it): done.
            _ => {
                self.social.fleeing.remove(&actor);
                false
            }
        }
    }

    /// The random conversations under way: walks that arrive or are given up, and
    /// conversations that are over.
    fn chats_on(&mut self, game: &mut Game, nav: &NavGraph) {
        let now = game.time;
        let going: Vec<(FormId, Option<Chat>)> =self.agents.iter().filter(|(_, a)| a.chat.is_some()).map(|(id, a)| (*id, a.chat)).collect();
        for (actor, chat) in going {
            // Already settled from the other side in this pass.
            if self.agents[&actor].chat != chat {
                continue;
            }
            match chat {
                Some(Chat::Going { partner, since }) => {
                    let waits = self.agents.get(&partner).is_some_and(|p| p.chat == Some(Chat::Waiting { starter: actor }));
                    let why = if !here(game, actor) || !here(game, partner) {
                        Some("one of them is gone")
                    } else if !waits || game.is_talking(partner) || game.in_actor_conversation(partner) {
                        Some("the partner is busy")
                    } else if now - since > CHAT_WALK_SECONDS {
                        Some("the way took too long")
                    } else {
                        None
                    };
                    if let Some(why) = why {
                        self.chat_give_up(game, nav, actor, partner, why);
                        continue;
                    }
                    let a = &self.agents[&actor];
                    let at = goal::place_of_ref(game, partner);
                    let away = at.filter(|p| p.space == a.walker.space).map_or(f32::INFINITY, |p| dist(p.pos, a.walker.feet));
                    // Near enough; or the way is at its end (the navmesh lets it get no
                    // nearer, or it was stopped) still out of reach: it gives up rather
                    // than talk across the distance.
                    if away <= TALK_DISTANCE || a.activity != Activity::Travelling {
                        let a = self.agents.get_mut(&actor).expect("listed");
                        a.walker.follower = None;
                        a.activity = Activity::There;
                        if away > TALK_DISTANCE {
                            self.chat_give_up(game, nav, actor, partner, "the way ends out of reach");
                        } else if !self.chat_start(game, actor, partner, away) {
                            self.chat_give_up(game, nav, actor, partner, "it has no line for the partner");
                        }
                    }
                }
                Some(Chat::Waiting { starter }) => {
                    // The starter gave up, or its package changed.
                    let goes = self.agents.get(&starter).is_some_and(|s| matches!(s.chat, Some(Chat::Going { partner: p, .. }) if p == actor));
                    if !goes {
                        self.agents.get_mut(&actor).expect("listed").chat = None;
                    }
                }
                Some(Chat::Talking { with }) if !game.in_actor_conversation(actor) && !game.in_actor_conversation(with) => {
                    self.chat_over(game, nav, actor);
                }
                Some(Chat::Talking { .. }) | None => {}
            }
        }
    }

    /// `starter` and `partner` are close: the conversation starts.
    fn chat_start(&mut self, game: &mut Game, starter: FormId, partner: FormId, away: f32) -> bool {
        let Ok(line) = game.start_actor_conversation(starter, partner, None) else { return false };
        for (who, with) in [(starter, partner), (partner, starter)] {
            if let Some(a) = self.agents.get_mut(&who) {
                a.chat = Some(Chat::Talking { with });
            }
        }
        self.log(game, starter, AiEvent::Chat { partner, line, away });
        true
    }

    /// `starter` sets off to `partner`, who waits. False when there is no way there.
    fn chat_set_off(&mut self, game: &mut Game, nav: &NavGraph, starter: FormId, partner: FormId, to: Place, away: f32) -> bool {
        let params = self.path_params(game, starter);
        let speed = self.calm_speeds(starter).walk;
        let a = self.agents.get_mut(&starter).expect("listed");
        (a.walker.speed, a.walk_pace) = (speed, None);
        let Ok(length) = a.walker.go_to(nav, to, &params).map(|p| p.length) else { return false };
        a.trip_started = game.time;
        a.trip_from = a.walker.travelled;
        a.activity = Activity::Travelling;
        a.chat = Some(Chat::Going { partner, since: game.time });
        if let Some(p) = self.agents.get_mut(&partner) {
            p.chat = Some(Chat::Waiting { starter });
        }
        self.log(game, starter, AiEvent::ChatApproach { partner, away, length });
        true
    }

    /// The walk to `partner` ends without a conversation: both go back to their packages.
    fn chat_give_up(&mut self, game: &mut Game, nav: &NavGraph, starter: FormId, partner: FormId, why: &'static str) {
        self.log(game, starter, AiEvent::ChatGaveUp { partner, why });
        if let Some(p) = self.agents.get_mut(&partner) {
            if p.chat == Some(Chat::Waiting { starter }) {
                p.chat = None;
            }
        }
        let a = self.agents.get_mut(&starter).expect("listed");
        a.walker.follower = None;
        a.activity = Activity::There;
        self.chat_over(game, nav, starter);
    }

    /// The conversation is over (or never began): the actor goes back to its package, and
    /// its time to the next one starts over [hypothesis: the timer is the time between an
    /// actor's conversations; without it the same two started again at once].
    fn chat_over(&mut self, game: &mut Game, nav: &NavGraph, actor: FormId) {
        self.agents.get_mut(&actor).expect("listed").chat = None;
        let (lo, hi) = (setting(game, "fAISocialTimerForConversationsMin", TIMER_MIN), setting(game, "fAISocialTimerForConversationsMax", TIMER_MAX));
        let next = game.time + self.social.rng.range(lo, hi) as f64;
        self.social.next_chat.insert(actor, next);
        self.back_to_package(game, nav, actor, |package, away| AiEvent::ChatBack { package, away });
    }

    /// Who says a line to someone turns its body to the listener, and the listener of a
    /// conversation between actors to the speaker (`Game::facings`: SayTo lines, hellos,
    /// conversations; not a line whose script asked for no turning); who waits for a
    /// partner turns to it as it comes. Not on furniture, on its way, or turning with its
    /// Dialogue package already.
    fn face_listeners(&mut self, game: &mut Game, dt: f32) {
        let mut pairs: Vec<(FormId, FormId)> = game.facings().into_iter().filter(|f| f.body).map(|f| (f.who, f.to)).collect();
        pairs.extend(self.agents.iter().filter_map(|(id, a)| match a.chat {
            Some(Chat::Waiting { starter }) => Some((*id, starter)),
            _ => None,
        }));
        for (who, to) in pairs {
            let Some(a) = self.agents.get(&who) else { continue };
            // One on its way turns only for its Goodbye greeting at the player, which the
            // engine stops it for (`Game::greets_player`).
            let walking = a.activity == Activity::Travelling && !game.greets_player(who);
            if !self.is_high(who) || a.using.is_some() || walking || a.talk == Some(Talking::Speaking) {
                continue;
            }
            if let Some(at) = goal::place_of_ref(game, to) {
                self.face(game, who, at, dt);
            }
        }
    }
}

impl Ai {
    /// An actor a script's Look turns whole (`rotateBody` 1, [`Game::body_looks`])
    /// turns its body to the target at [`LOOK_TURN_RATE`] and keeps it on the target to
    /// within a degree, whatever the target does, until StopLook [observed: RingoRef,
    /// campaign `knockdown_look` 2026-10-19; documented: GECK wiki Look, "rotate his
    /// entire body"]. A walker walks on along its package's route with its body turned to
    /// the target (Ringo walked 120 units to a chair facing the player); it does not sit
    /// down while the Look lasts, and does once it ends (`Ai::set_using`). After StopLook
    /// nothing turns the body back: a standing actor keeps its heading, a walker takes its
    /// route's again [observed: same]. Not turned (its head still is, the engine's): an
    /// actor out of the loaded area, dead, in furniture, or turning to the player for its
    /// Dialogue package.
    fn face_looks(&mut self, game: &mut Game, dt: f32) {
        let wanted = game.body_looks();
        let over: Vec<FormId> = self.social.looks.keys().copied().filter(|a| !wanted.iter().any(|(w, t)| w == a && self.social.looks.get(a).is_some_and(|l| l.0 == *t))).collect();
        for a in over {
            self.social.looks.remove(&a);
            if !wanted.iter().any(|(w, _)| *w == a) {
                if let Some(heading) = self.agents.get(&a).map(|g| g.walker.heading) {
                    self.log(game, a, AiEvent::LookOver { heading });
                }
            }
        }
        // A seat a Look kept the actor from is taken once the Look is over.
        let free: Vec<(FormId, FormId)> = self.agents.iter().filter(|(r, _)| !wanted.iter().any(|(w, _)| w == *r)).filter_map(|(r, a)| a.held_seat.map(|f| (*r, f))).collect();
        for (who, f) in free {
            self.set_using(game, who, Some(f));
        }
        for (who, to) in wanted {
            let Some(a) = self.agents.get(&who) else { continue };
            let why = if game.extra.get(&who).is_some_and(|e| e.dead) {
                Some("it is dead")
            } else if !self.is_high(who) {
                Some("it is out of the loaded area")
            } else if a.using.is_some() {
                Some("it is in furniture")
            } else if a.talk == Some(Talking::Speaking) {
                Some("it faces whom it talks to (its Dialogue package)")
            } else if a.fixed {
                Some("it never turns (Immobile, Turning Speed 0)")
            } else {
                None
            };
            let prev = self.social.looks.get(&who).copied();
            let state = prev.map(|l| l.1);
            if let Some(why) = why {
                if state != Some(LookTurn::Held(why)) {
                    self.social.looks.insert(who, (to, LookTurn::Held(why), a.walker.heading));
                    self.log(game, who, AiEvent::LookHeld { target: to, why });
                }
                continue;
            }
            let Some(at) = goal::place_of_ref(game, to) else { continue };
            let (feet, space, walking) = (a.walker.feet, a.walker.space, a.activity == Activity::Travelling);
            let d = [at.pos[0] - feet[0], at.pos[1] - feet[1]];
            if d[0].hypot(d[1]) < 1.0 {
                continue;
            }
            let want = heading_of(d);
            // A walker's own heading is its route's: the body's is kept here.
            let from = match prev {
                Some((_, LookTurn::Turning | LookTurn::Facing, h)) if walking => h,
                _ => a.walker.heading,
            };
            let mut turn = match state {
                Some(LookTurn::Turning) => LookTurn::Turning,
                _ if heading_delta(from, want).abs() > LOOK_TURN_FROM => {
                    self.log(game, who, AiEvent::LookTurn { target: to, from, to: want });
                    LookTurn::Turning
                }
                _ => LookTurn::Facing,
            };
            let body = turn_towards(from, want, LOOK_TURN_RATE * dt.max(0.0));
            if turn == LookTurn::Turning && heading_delta(body, want).abs() < 0.02 {
                turn = LookTurn::Facing;
                self.log(game, who, AiEvent::LookFaced { target: to, heading: body });
            }
            if !walking {
                self.agents.get_mut(&who).expect("listed").walker.heading = body;
            }
            let cell = goal::cell_of_place(game.world.placed, &Place { space, pos: feet });
            let _ = game.set_actor_place(who, cell, feet, body);
            self.social.looks.insert(who, (to, turn, body));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(ty: PackageType, general: u32, behaviour: u16, specific: u16) -> PackageDef {
        PackageDef { ty, flags: general, fallout_flags: behaviour, type_flags: specific, ..PackageDef::default() }
    }

    #[test]
    fn a_foe_with_no_place_is_near_no_one() {
        let space = Space::World(0xDA726);
        let p = Place { space, pos: [0.0, 0.0, 0.0] };
        // #3223: a hunter in a cell no longer loaded: no resident, no place.
        assert!(!foe_near(None, None, p));
        // Where the world has it: near or far.
        assert!(foe_near(None, Some(Place { space, pos: [1000.0, 0.0, 0.0] }), p));
        assert!(!foe_near(None, Some(Place { space, pos: [9200.0, 0.0, 0.0] }), p));
        // #4891: no resident, in another interior at overlapping coordinates (the report's
        // Cell Block A ganger and visitor-center player): not near.
        let block_a = Place { space: Space::Interior(1), pos: [3902.0, -100.0, 14464.0] };
        let visitors = Place { space: Space::Interior(2), pos: [4136.0, -326.0, 14465.0] };
        assert!(!foe_near(None, Some(block_a), visitors));
        assert!(foe_near(None, Some(Place { space: visitors.space, ..block_a }), visitors));
        // A resident: by where it walks, in the same space only.
        assert!(foe_near(Some(Place { space, pos: [0.0, 1400.0, 0.0] }), None, p));
        assert!(!foe_near(Some(Place { space: Space::Interior(0x103DF9), pos: [0.0, 10.0, 0.0] }), None, p));
    }

    #[test]
    fn the_default_allows_and_a_skip_package_allows_what_it_sets() {
        // Bit 12 clear: the default, whatever the stored behaviour flags (Vault 3's
        // DefaultSandboxEditorLocation512 stores none).
        let plain = def(PackageType::Sandbox, 0, 0, 0);
        assert!(allows(&plain, fallout::HELLOS_TO_PLAYER) && chats(&plain));
        // Bit 12 set: only what it sets (Easy Pete's chair package, 0x1F6).
        let chair = def(PackageType::Travel, flags::SKIP_FALLOUT_BEHAVIOR, 0x1F6, 0);
        assert!(!allows(&chair, fallout::HELLOS_TO_PLAYER) && chats(&chair));
        let none = def(PackageType::Patrol, flags::SKIP_FALLOUT_BEHAVIOR, 0, 0);
        assert!(!allows(&none, fallout::HELLOS_TO_PLAYER) && !chats(&none));
        // No Conversation counts for Sandbox and Wander only.
        assert!(!chats(&def(PackageType::Sandbox, 0, 0, type_flags::NO_CONVERSATION)));
        assert!(chats(&def(PackageType::Guard, 0, 0, type_flags::NO_CONVERSATION)));
    }
}
