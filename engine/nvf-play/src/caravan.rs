//! Caravan, the card game (docs/spec-notes/caravan.md): a dialogue line's ShowCaravanMenu
//! (`fnv_game::caravan::CaravanRequest`) opens the menu against the speaker. Four screens,
//! as the original's: the bet, the deck builder, the game, the results. The rules and the
//! opponent are `fnv_game::caravan`; `screen` draws it flat from the game's textures.
//!
//! - Refused, with a corner message, while the player owns fewer than 30 cards.
//! - The ante is taken from the player's caps when the game starts (Play Caravan); the
//!   winner gets the pot (caravan.md §3). Leaving before then costs nothing; Cancel during
//!   the game forfeits (pressed twice).
//! - Bets in any currency (caravan.md §3.1, 2026-10-19) [documented: `HelpCaravanBetting`,
//!   "players may also offer NCR dollars or Legion coins"]: each side's funds are its caps
//!   and its NCR dollars and Legion coins at their records' value ([`CURRENCY`]); an ante
//!   is paid in caps first, then in notes and coins from the least valued up, the change
//!   of the last one given back in caps ([`pay_plan`]) [design]. The pot is paid in caps.
//! - The help messages come up once each, over the screen they explain.
//! - It is a `PlayState::Menu`: the world and the game clock stand still.
//!
//! Keys [observed: the owner's screenshots, the menu files' `_PCButtonText`]: bet W Auto
//! Match, A Raise, F Accept, R Exit; deck W Add, A Remove, S Randomize, F Play Caravan, Left
//! and Right (or the wheel) move along the cards; game W Select Card (and place it), Q
//! Discard Card, E Discard Track, R Cancel, Left and Right choose the hand's card or the
//! place for the chosen one. Cards, places and buttons can be clicked. Esc leaves before
//! the game; any key goes on from the results.
//!
//! Test aid `NVF_PLAY_TEST_CARAVAN=<seconds>:<step>[,…]`: `seed:<n>` (before `open`),
//! `caps:<n>` (the player carries that many caps), `cards:<deck hex>` (every card of a
//! CDCK deck given to the player, as AddCardToPlayer), `open:<actor hex>:<deck hex>[:<share>]`
//! (as ShowCaravanMenu on that actor), `match`, `raise`, `accept`, `add`, `remove`,
//! `randomize`, `play`, `auto` (the player's side plays by the opponent's rules from then
//! on, help boxes close by themselves and the results go on after [`AUTO_RESULTS`]),
//! `forfeit`, `help` (closes the help box), `continue`, `exit`, `tie:<value>` (the second
//! track's caravans, NEW RENO and REDDING, rebuilt from each side's own deck to that value,
//! a tie seen in game on 2026-11-15; test only). At exit, when the menu was asked for: "caravan
//! summary: …".

pub mod guide;
pub mod screen;

use fnv_util::words::{tr, trf};
use crate::lockpick::Help;
use crate::pause::{AidTime, Paused};
use crate::script::{with_world, ScriptState};
use crate::sim::SimState;
use crate::ui_sounds::{UiEvent, UiMenu, UiSound};
use crate::{PlaySet, PlayState};
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use crate::ui_pointer::PointerState;
use fnv_game::caravan::{self as rules, Bet, BetterCaravanConfig, Card, CaravanRequest, End, Event, Game, Move, Phase, Rank, Record, CAPS, MIN_DECK, OPPONENT, PLAYER};
use fnv_sim::FormId;
use rand_chacha::ChaCha8Rng;
use rand_core::{RngCore, SeedableRng};
use std::collections::HashMap;

/// The menu's code to scripts (caravan.md §1).
pub const MENU_CARAVAN: u16 = rules::MENU_CODE;
/// Seconds the opponent takes over a move, from and to [design].
pub const THINK_SECONDS: (f32, f32) = (0.8, 1.4);
/// Seconds the player's side takes when it plays by itself (the test aid's `auto`).
pub const AUTO_SECONDS: f32 = 0.3;
/// Seconds the table stays after the game before the results [design].
pub const END_SECONDS: f32 = 2.5;
/// Seconds the menu takes to fade as it closes.
pub const LEAVE_SECONDS: f32 = 0.5;
/// Seconds the results stay under the test aid's `auto`.
pub const AUTO_RESULTS: f32 = 3.0;
/// The skill the test aid's `auto` plays the player's side with.
const AUTO_SKILL: f32 = 0.5;

/// Ask for the menu (the script function, the test aid).
#[derive(Message, Clone, Copy, Debug)]
pub struct OpenCaravan(pub CaravanRequest);

#[derive(Resource, Default)]
pub struct CaravanScreen(pub Option<Session>);

/// The game's table drawn in 3D (nvforever `caravan_view.rs`): the picture the game screen
/// shows in place of its flat planks and cards, laid out as [`screen::table_cards`] says
/// so the names, values and outlines drawn over it fall on its cards. Present only while
/// the table is built.
#[derive(Resource)]
pub struct CaravanView {
    pub image: Handle<Image>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Bet,
    Deck,
    Play,
    Results,
}

/// What the player is choosing in the game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// A card of the hand (`Session::hand`).
    Hand,
    /// A place for the chosen card: the legal moves and the one shown.
    Place { moves: Vec<Move>, at: usize },
    /// One of the player's caravans to throw away.
    Track { tracks: Vec<usize>, at: usize },
}

/// Additional keyboard keys parsed from `BetterCaravan.ini` (`sKeyUp`, `sKeyDown`,
/// `sKeyLeft`, `sKeyRight`). All `None` when the INI is absent [design].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BetterCaravanKeys {
    pub up: Option<KeyCode>,
    pub down: Option<KeyCode>,
    pub left: Option<KeyCode>,
    pub right: Option<KeyCode>,
}

impl BetterCaravanKeys {
    pub fn from_config(cfg: &BetterCaravanConfig) -> Self {
        Self {
            up: cfg.key_up.as_deref().and_then(parse_better_caravan_key),
            down: cfg.key_down.as_deref().and_then(parse_better_caravan_key),
            left: cfg.key_left.as_deref().and_then(parse_better_caravan_key),
            right: cfg.key_right.as_deref().and_then(parse_better_caravan_key),
        }
    }
}

/// Parse a BetterCaravan INI key name (single ASCII letter/digit, `KEY_NAMES` entry, or
/// common arrow/numpad alias) into a Bevy [`KeyCode`] [design].
pub(crate) fn parse_better_caravan_key(name: &str) -> Option<KeyCode> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
        return None;
    }
    if let Some(crate::bindings::Input::Key(k)) = crate::bindings::parse_input(trimmed) {
        return Some(k);
    }
    match trimmed.to_ascii_lowercase().as_str() {
        "arrowup" => Some(KeyCode::ArrowUp),
        "arrowdown" => Some(KeyCode::ArrowDown),
        "arrowleft" => Some(KeyCode::ArrowLeft),
        "arrowright" => Some(KeyCode::ArrowRight),
        "esc" | "escape" => Some(KeyCode::Escape),
        "numpad0" => Some(KeyCode::Numpad0),
        "numpad1" => Some(KeyCode::Numpad1),
        "numpad2" => Some(KeyCode::Numpad2),
        "numpad3" => Some(KeyCode::Numpad3),
        "numpad4" => Some(KeyCode::Numpad4),
        "numpad5" => Some(KeyCode::Numpad5),
        "numpad6" => Some(KeyCode::Numpad6),
        "numpad7" => Some(KeyCode::Numpad7),
        "numpad8" => Some(KeyCode::Numpad8),
        "numpad9" => Some(KeyCode::Numpad9),
        _ => None,
    }
}

pub struct Session {
    pub opponent: FormId,
    pub opponent_name: String,
    pub deck_form: FormId,
    pub opponent_deck: Vec<Card>,
    pub skill: f32,
    pub stage: Stage,
    pub bet: Bet,
    /// Every card the player owns, each form once, in the builder's order.
    pub owned: Vec<FormId>,
    /// The deck being built.
    pub deck: Vec<FormId>,
    /// The builder's middle card (in `owned`).
    pub cursor: usize,
    pub game: Option<Game>,
    pub hand: usize,
    pub pick: Pick,
    /// Cancel was pressed once with nothing chosen: again forfeits.
    pub forfeit_armed: bool,
    /// The player's games against this opponent.
    pub record: Record,
    pub status: Option<String>,
    pub help: Option<Help>,
    /// Help messages still to show, in order, once the one up is closed.
    queued_help: Vec<rules::Help>,
    /// The side to move waits until then (aid seconds).
    pub think_until: f32,
    /// The game ended then.
    pub over_at: Option<f32>,
    pub leaving: Option<(f32, &'static str)>,
    /// The ante was taken (the game started).
    pub staked: bool,
    settled: bool,
    pub auto: bool,
    /// A card form's face and back textures (under `textures\`).
    pub textures: HashMap<FormId, (Option<String>, Option<String>)>,
    /// A card form's rank (for BetterCaravan face-card descriptions in Deck stage).
    pub card_ranks: HashMap<FormId, Rank>,
    /// Clean-room `BetterCaravan.ini` settings (all off when INI is absent).
    pub better: BetterCaravanConfig,
    /// Parsed additional directional keys from `BetterCaravan.ini`.
    pub better_keys: BetterCaravanKeys,
    rng: ChaCha8Rng,
}

/// The Caravan guide on, kept with the settings as `caravan-tips`.
static TIPS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn tips() -> bool {
    TIPS.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_tips(on: bool) {
    TIPS.store(on, std::sync::atomic::Ordering::Relaxed);
}

impl Session {
    /// The guide's text for the table as it stands ([`guide::text`]), empty when the
    /// guide is off or it is not the player's turn.
    pub fn guide(&self) -> String {
        // A tie in the sold range is said whatever the tips and whoever's turn it is.
        let tie = self.game.as_ref().and_then(guide::tie_note);
        let tip = self.tip();
        match (tie, tip.is_empty()) {
            (Some(t), true) => t,
            (Some(t), false) => format!("{t} {tip}"),
            (None, _) => tip,
        }
    }

    /// The tips' text ([`guide::text`]), empty when they are off or it is not the
    /// player's turn.
    fn tip(&self) -> String {
        let Some(g) = self.game.as_ref().filter(|_| tips() && self.players_turn()) else { return String::new() };
        let shown = match &self.pick {
            Pick::Place { moves, at } => moves.get(*at).copied(),
            Pick::Track { tracks, at } => tracks.get(*at).map(|&track| Move::DiscardTrack { track }),
            Pick::Hand => None,
        };
        let refused = self.hand_moves(self.hand).next().is_none();
        guide::text(g, self.hand, shown.as_ref(), refused)
    }

    pub fn fade(&self, now: f32) -> f32 {
        self.leaving.map_or(1.0, |(since, _)| (1.0 - (now - since) / LEAVE_SECONDS).clamp(0.0, 1.0))
    }

    fn begin_leaving(&mut self, now: f32, why: &'static str) {
        if self.leaving.is_none() {
            info!("caravan: leaving ({why})");
            self.leaving = Some((now, why));
            self.help = None;
        }
    }

    pub fn players_turn(&self) -> bool {
        self.game.as_ref().is_some_and(|g| g.phase != Phase::Over && g.turn == PLAYER) && self.help.is_none()
    }

    /// Where the screen is for its controller buttons (`crate::pad::menu_buttons`): its
    /// stage, or the help box while one is up (whose A and B close it).
    pub fn hint_at(&self) -> crate::pad::MenuAt {
        if self.help.is_some() {
            return crate::pad::MenuAt { code: Some(MENU_CARAVAN), tab: crate::pad::HELP_PAGE };
        }
        self.stage_at()
    }

    /// The stage's own buttons, help box or not (the buttons under a help box keep their
    /// hints).
    fn stage_at(&self) -> crate::pad::MenuAt {
        let tab = match self.stage {
            Stage::Bet => crate::pad::CARAVAN_BET,
            Stage::Deck => crate::pad::CARAVAN_DECK,
            Stage::Play => crate::pad::CARAVAN_PLAY,
            Stage::Results => crate::pad::HELP_PAGE,
        };
        crate::pad::MenuAt { code: Some(MENU_CARAVAN), tab }
    }

    /// The hint for the keys `keys` on this screen ("[W]", with a controller "[A]").
    pub fn hint(&self, keys: &[KeyCode]) -> String {
        crate::pad::hint(keys, self.stage_at())
    }

    /// The screen's hints as one log line ("Auto Match W), Raise A), …", with a help box
    /// "…; help: Close [E]").
    pub fn hint_line(&self) -> String {
        use KeyCode::*;
        let parts: Vec<String> = match self.stage {
            Stage::Deck => [(tr("Randomize Caravan Deck"), KeyS), (tr("Add"), KeyW), (tr("Remove"), KeyA), (tr("Play Caravan"), KeyF)].iter().map(|(l, k)| format!("{l} {}", self.hint(&[*k]))).collect(),
            _ => self.buttons().into_iter().map(|(l, h, _, _)| format!("{l} {h}")).collect(),
        };
        let mut line = parts.join(", ");
        if let Some(h) = &self.help {
            line.push_str(&format!("; help: {} {}", h.words.close, crate::pad::game_hint(&h.words.close_hint, &[KeyE, Escape], crate::pad::HELP_AT)));
        }
        line
    }

    /// The buttons of the bet and game screens, top to bottom: label, key hint, command,
    /// lit.
    pub fn buttons(&self) -> Vec<(&'static str, String, CaravanInput, bool)> {
        use KeyCode::*;
        match self.stage {
            Stage::Bet => vec![
                (tr("Auto Match"), self.hint(&[KeyW]), CaravanInput::AutoMatch, self.bet.can_match()),
                (tr("Raise"), self.hint(&[KeyA]), CaravanInput::Raise, self.bet.can_raise()),
                (tr("Accept"), self.hint(&[KeyF]), CaravanInput::Accept, self.bet.can_accept()),
                (tr("Exit"), self.hint(&[KeyR, Escape]), CaravanInput::Exit, true),
            ],
            Stage::Play => {
                let mine = self.players_turn();
                let g = self.game.as_ref();
                let opening = g.is_some_and(|g| g.phase == Phase::Opening);
                let select = mine && (matches!(self.pick, Pick::Place { .. } | Pick::Track { .. }) || self.hand_moves(self.hand).next().is_some());
                let discard = mine && !matches!(self.pick, Pick::Track { .. });
                let track = mine && !opening && g.is_some_and(|g| g.sides[PLAYER].caravans.iter().any(|c| !c.is_empty()));
                vec![
                    (tr("Select Card"), self.hint(&[KeyW]), CaravanInput::Select, select),
                    (tr("Discard Card"), self.hint(&[KeyQ]), CaravanInput::Discard, discard),
                    (tr("Discard Track"), self.hint(&[KeyE]), CaravanInput::DiscardTrack, track),
                    (tr("Cancel"), self.hint(&[KeyR]), CaravanInput::Cancel, mine || self.game.as_ref().is_some_and(|g| g.phase != Phase::Over)),
                    (if tips() { tr("Tips On") } else { tr("Tips Off") }, String::new(), CaravanInput::Tips, true),
                ]
            }
            _ => Vec::new(),
        }
    }

    /// The legal places for the hand's card `i`.
    pub fn hand_moves(&self, i: usize) -> impl Iterator<Item = Move> + '_ {
        let g = self.game.as_ref();
        g.into_iter().flat_map(move |g| g.moves_for(PLAYER).into_iter().filter(move |m| matches!(m, Move::Play { card, .. } if *card == i)))
    }

    pub fn in_deck(&self, f: FormId) -> bool {
        self.deck.contains(&f)
    }

    fn show_help(&mut self, rt: &mut crate::script::Runtime, which: rules::Help, bindings: &crate::bindings::Bindings) {
        if self.help.is_some() {
            self.queued_help.push(which);
            return;
        }
        if let Some((title, text)) = rules::first_help(&mut rt.game, which) {
            info!("caravan: help message {} {title:?} shown (first time)", which.message());
            self.help = Some(Help::new(&rt.game, title, &text, bindings));
        }
    }
}

/// A command for the open menu.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub enum CaravanInput {
    AutoMatch,
    Raise,
    Accept,
    Exit,
    Add,
    Remove,
    Randomize,
    PlayDeck,
    /// Along the builder's cards, or the hand, or the places (by `n`).
    Step(i32),
    /// Vertical move along a track (`-1` up, `+1` down; BetterCaravan `sKeyUp`/`sKeyDown`).
    StepVertical(i32),
    /// The builder's card `n` to the middle.
    CursorTo(usize),
    Select,
    /// The hand's card `n` chosen (a click).
    PickCard(usize),
    /// The place `n` of the chosen card taken (a click).
    Place(usize),
    Discard,
    DiscardTrack,
    Cancel,
    Forfeit,
    CloseHelp,
    Continue,
    /// The guide on or off.
    Tips,
}

#[derive(Resource, Default, Debug)]
struct CaravanStats {
    asked: u32,
    opened: u32,
    refused: u32,
    games: u32,
    won: u32,
    lost: u32,
    net: i64,
    moves: u32,
}

/// `NVF_PLAY_TEST_CARAVAN`.
#[derive(Resource, Default)]
struct CaravanAid {
    steps: Vec<(f32, AidStep)>,
    seed: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
enum AidStep {
    Seed(u64),
    Caps(u32),
    Cards(u32),
    Open(CaravanRequest),
    Auto,
    Input(CaravanInput),
    Tie(u32),
}

fn hex(s: &str) -> Option<u32> {
    u32::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok()
}

fn parse_aid(v: &str) -> Result<Vec<(f32, AidStep)>, String> {
    crate::test_aid::timed(v)?
        .into_iter()
        .map(|(t, what)| {
            let (op, arg) = what.split_once(':').unwrap_or((what, ""));
            let parts: Vec<&str> = arg.split(':').collect();
            let bad = || format!("{what:?}: expected seed:<n>, caps:<n>, cards:<deck>, open:<actor>:<deck>[:<share>], match, raise, accept, add, remove, randomize, play, auto, forfeit, help, continue, exit or tie:<value>");
            let num = |s: Option<&&str>| s.and_then(|s| s.trim().parse::<i64>().ok()).ok_or_else(bad);
            let step = match op {
                "seed" => AidStep::Seed(num(parts.first())? as u64),
                "caps" => AidStep::Caps(num(parts.first())?.max(0) as u32),
                "cards" => AidStep::Cards(parts.first().and_then(|s| hex(s)).ok_or_else(bad)?),
                "open" => {
                    let opponent = FormId(parts.first().and_then(|s| hex(s)).ok_or_else(bad)?);
                    let deck = FormId(parts.get(1).and_then(|s| hex(s)).ok_or_else(bad)?);
                    let max_bet_share = parts.get(2).and_then(|s| s.trim().parse::<f32>().ok()).unwrap_or(0.25);
                    AidStep::Open(CaravanRequest { opponent, deck, difficulty: 0, max_bet_share })
                }
                "match" => AidStep::Input(CaravanInput::AutoMatch),
                "raise" => AidStep::Input(CaravanInput::Raise),
                "accept" => AidStep::Input(CaravanInput::Accept),
                "add" => AidStep::Input(CaravanInput::Add),
                "remove" => AidStep::Input(CaravanInput::Remove),
                "randomize" => AidStep::Input(CaravanInput::Randomize),
                "play" => AidStep::Input(CaravanInput::PlayDeck),
                "auto" => AidStep::Auto,
                "forfeit" => AidStep::Input(CaravanInput::Forfeit),
                "help" => AidStep::Input(CaravanInput::CloseHelp),
                "continue" => AidStep::Input(CaravanInput::Continue),
                "exit" => AidStep::Input(CaravanInput::Exit),
                "tie" => AidStep::Tie(num(parts.first())?.clamp(1, 40) as u32),
                _ => return Err(bad()),
            };
            Ok((t, step))
        })
        .collect()
}

pub struct CaravanPlugin;

impl Plugin for CaravanPlugin {
    fn build(&self, app: &mut App) {
        let steps = match std::env::var("NVF_PLAY_TEST_CARAVAN").map(|v| parse_aid(&v)) {
            Ok(Ok(a)) => a,
            Ok(Err(e)) => {
                warn!("NVF_PLAY_TEST_CARAVAN: {e}");
                Vec::new()
            }
            Err(_) => Vec::new(),
        };
        crate::settings::add_setting(
            app,
            crate::settings::Setting {
                key: "caravan-tips",
                group: crate::pause::GROUP_MENUS,
                label: "Caravan tips",
                note: "On: the Caravan table says what the chosen card does and why a card can't be played, and the menu has a Tips button. Off: only the game's short messages. Not in the original.",
                switches: "",
                get: |_| tips(),
                set: |_, on| set_tips(on),
                slider: None,
            },
        );
        app.init_resource::<CaravanScreen>()
            .init_resource::<CaravanStats>()
            .insert_resource(CaravanAid { steps, ..default() })
            .add_message::<OpenCaravan>()
            .add_message::<CaravanInput>()
            .add_systems(
                Update,
                (run_aid, scripts_open, open, (keys, buttons, apply, step).chain().run_if(in_state(PlayState::Menu)), screen::draw, screen::fade)
                    .chain()
                    .after(crate::menu::open_menus)
                    .in_set(PlaySet::Ui),
            )
            .add_systems(OnExit(PlayState::Menu), close)
            .add_systems(Last, report);
    }
}

fn run_aid(time: AidTime, mut aid: ResMut<CaravanAid>, mut open: MessageWriter<OpenCaravan>, mut input: MessageWriter<CaravanInput>, mut screen: ResMut<CaravanScreen>, mut sim: ResMut<SimState>, mut script: Option<ResMut<ScriptState>>) {
    if aid.steps.is_empty() {
        return;
    }
    let Some(now) = time.world_secs() else { return };
    let mut due: Vec<(f32, AidStep)> = aid.steps.iter().filter(|(t, _)| *t <= now).cloned().collect();
    due.sort_by(|a, b| a.0.total_cmp(&b.0));
    aid.steps.retain(|(t, _)| *t > now);
    for (_, step) in due {
        info!("test aid NVF_PLAY_TEST_CARAVAN: {step:?}");
        match step {
            AidStep::Seed(s) => aid.seed = Some(s),
            AidStep::Caps(n) => {
                let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) else { continue };
                with_world(rt, &mut sim, |rt| {
                    let have = rt.game.world.item_count(FormId::PLAYER_REF, CAPS).unwrap_or(0);
                    let n = n as i32;
                    if n > have {
                        let _ = rt.game.world.add_item(FormId::PLAYER_REF, CAPS, n - have);
                    } else if n < have {
                        let _ = rt.game.world.remove_item(FormId::PLAYER_REF, CAPS, have - n);
                    }
                });
                info!("caravan: the player carries {n} caps");
            }
            AidStep::Cards(deck) => {
                let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) else { continue };
                let Some(cards) = rt.game.data.caravan_decks.get(&FormId(deck)).map(|d| d.cards.clone()) else {
                    warn!("caravan: {deck:08X} is not a Caravan deck");
                    continue;
                };
                let given = cards.iter().filter(|&&c| rules::add_card(&mut rt.game, c)).count();
                info!("caravan: {given} cards of deck {deck:08X} given to the player ({} owned)", rules::owned(&rt.game).len());
            }
            AidStep::Open(r) => {
                open.write(OpenCaravan(r));
            }
            AidStep::Auto => {
                if let Some(s) = screen.0.as_mut() {
                    s.auto = true;
                }
            }
            AidStep::Input(i) => {
                input.write(i);
            }
            AidStep::Tie(value) => {
                let Some(g) = screen.0.as_mut().and_then(|s| s.game.as_mut()) else {
                    warn!("caravan: test aid tie: no game on the table");
                    continue;
                };
                let done = [PLAYER, OPPONENT].map(|side| tie_track(g, side, TIE_TRACK, value));
                if done.iter().all(|&d| d) {
                    info!("caravan: test aid: {} and {} set to {value}", guide::track_name(PLAYER, TIE_TRACK), guide::track_name(OPPONENT, TIE_TRACK));
                } else {
                    warn!("caravan: test aid tie: no cards in the decks make {value} (player {}, opponent {})", done[0], done[1]);
                }
            }
        }
    }
}

/// The track the `tie` aid step sets: NEW RENO against REDDING, as in an in-game frame [observed: buckallobill, in game, FNV 1.4.0.525].
const TIE_TRACK: usize = 1;

/// Test aid: `side`'s caravan on `track` rebuilt from two to four number cards of its own
/// deck adding up to `value`; the caravan's old cards go to the bottom of the deck. False
/// (nothing changed) when no such cards are left.
fn tie_track(g: &mut Game, side: usize, track: usize, value: u32) -> bool {
    let s = &mut g.sides[side];
    let numbers: Vec<usize> = (0..s.deck.len()).filter(|&i| s.deck[i].rank.number().is_some()).collect();
    let worth = |i: usize| s.deck[i].rank.number().unwrap_or(0);
    let mut pick: Option<Vec<usize>> = None;
    'search: for n in 2..=4usize {
        let mut at: Vec<usize> = (0..n).collect();
        while numbers.len() >= n {
            if at.iter().map(|&k| worth(numbers[k])).sum::<u32>() == value {
                pick = Some(at.iter().map(|&k| numbers[k]).collect());
                break 'search;
            }
            // The next combination of `n` of the number cards, in order.
            let Some(j) = (0..n).rev().find(|&j| at[j] < numbers.len() - n + j) else { break };
            at[j] += 1;
            let base = at[j];
            for (o, a) in at[j + 1..].iter_mut().enumerate() {
                *a = base + 1 + o;
            }
        }
    }
    let Some(mut pick) = pick else { return false };
    pick.sort_unstable_by(|a, b| b.cmp(a));
    let mut cards: Vec<Card> = pick.iter().map(|&i| s.deck.remove(i)).collect();
    cards.reverse();
    let old = std::mem::take(&mut s.caravans[track].cards);
    for p in old {
        s.deck.insert(0, p.card);
        for f in p.faces {
            s.deck.insert(0, f);
        }
    }
    s.caravans[track].cards = cards.into_iter().map(|card| rules::Placed { card, faces: Vec::new() }).collect();
    true
}

/// The games scripts asked for (ShowCaravanMenu), opened while playing (after the
/// conversation that asked closed).
fn scripts_open(script: Option<ResMut<ScriptState>>, state: Res<State<PlayState>>, mut open: MessageWriter<OpenCaravan>) {
    let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) else { return };
    if rt.game.caravan_requests.is_empty() || *state.get() != PlayState::Playing {
        return;
    }
    if let Some(r) = rt.game.caravan_requests.pop() {
        open.write(OpenCaravan(r));
    }
    rt.game.caravan_requests.clear();
}

fn seed(opponent: FormId, now: f32) -> u64 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    t ^ ((opponent.0 as u64) << 20) ^ now.to_bits() as u64
}

/// The currencies other than caps a bet may use [documented: `HelpCaravanBetting`; the
/// FalloutNV.esm MISC records NCRMoney5, NCRMoney20, NCRMoney100, LegionMoney1 (Legion
/// Denarius) and LegionMoney25 (Legion Aureus); the mangled denarius is no currency], and
/// the caravan money (CMNY) records of the same notes and coins, MoneyNCR5, MoneyNCR20,
/// MoneyNCR100, MoneyLegion1 and MoneyLegion25, which are the ones the world's
/// inventories hold [observed: FalloutNV.esm; `fnv_sim::forms::Item::money`].
pub const CURRENCY: [FormId; 10] = [
    FormId(0x0013_A4D6),
    FormId(0x0013_A4D7),
    FormId(0x0013_A4D8),
    FormId(0x0013_7B9D),
    FormId(0x0013_A4D9),
    FormId(0x0017_6AC6),
    FormId(0x0017_6AC8),
    FormId(0x0017_6AC9),
    FormId(0x0017_6AC4),
    FormId(0x0017_6AC7),
];

/// What one side holds to bet: its caps, and each currency it carries as (item, count,
/// value in caps).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Purse {
    pub caps: u32,
    pub notes: Vec<(FormId, u32, u32)>,
}

impl Purse {
    /// Caps and the notes' and coins' worth.
    pub fn worth(&self) -> u32 {
        self.caps.saturating_add(self.notes.iter().map(|&(_, n, v)| n.saturating_mul(v)).sum::<u32>())
    }
}

/// How an amount is paid from a purse (module notes).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Payment {
    pub caps: u32,
    pub notes: Vec<(FormId, u32)>,
    /// Caps given back for the last note's or coin's overpay.
    pub change: u32,
    /// What could not be paid (the purse was short).
    pub short: u32,
}

/// Pay `amount` from `purse`: caps first, then notes and coins from the least valued up,
/// as many of each as the rest needs; the overpay of the last comes back as change.
pub fn pay_plan(purse: &Purse, amount: u32) -> Payment {
    let caps = purse.caps.min(amount);
    let mut rest = amount - caps;
    let mut notes: Vec<(FormId, u32, u32)> = purse.notes.iter().filter(|n| n.1 > 0 && n.2 > 0).copied().collect();
    notes.sort_by_key(|&(id, _, v)| (v, id.0));
    let mut out = Payment { caps, ..Default::default() };
    for (id, count, value) in notes {
        if rest == 0 {
            break;
        }
        let n = count.min(rest.div_ceil(value));
        out.notes.push((id, n));
        let paid = n * value;
        if paid >= rest {
            out.change = paid - rest;
            rest = 0;
        } else {
            rest -= paid;
        }
    }
    out.short = rest;
    out
}

/// The caps and currency of `holders` (an actor and its merchant container) [hypothesis:
/// the container's as barter counts it, menu/model.rs `Trade::stock`].
fn purse_of(world: &mut fnv_sim::World<'_>, forms: &fnv_sim::forms::FormStore, holders: &[FormId]) -> Purse {
    let mut count = |h: FormId, i: FormId| world.item_count(h, i).unwrap_or(0).max(0) as u32;
    let caps = holders.iter().map(|&h| count(h, CAPS)).sum();
    let mut notes = Vec::new();
    for c in CURRENCY {
        let n: u32 = holders.iter().map(|&h| count(h, c)).sum();
        let v = forms.item(c).and_then(|it| it.worth()).unwrap_or(0).max(0) as u32;
        if n > 0 && v > 0 {
            notes.push((c, n, v));
        }
    }
    Purse { caps, notes }
}

/// Take `amount` from `holders` (in that order) by [`pay_plan`]; the change goes to the
/// first. Returns the payment for the log.
fn pay(world: &mut fnv_sim::World<'_>, forms: &fnv_sim::forms::FormStore, holders: &[FormId], amount: u32) -> Payment {
    let plan = pay_plan(&purse_of(world, forms, holders), amount);
    let mut take = |item: FormId, mut n: u32| {
        for &h in holders {
            let have = world.item_count(h, item).unwrap_or(0).max(0) as u32;
            let k = have.min(n);
            if k > 0 {
                let _ = world.remove_item(h, item, k as i32);
                n -= k;
            }
        }
    };
    take(CAPS, plan.caps);
    for &(id, n) in &plan.notes {
        take(id, n);
    }
    if plan.change > 0 {
        if let Some(&first) = holders.first() {
            let _ = world.add_item(first, CAPS, plan.change as i32);
        }
    }
    plan
}

/// A payment for the log: "54 caps", "18 caps, 3 x 0x0013A4D7 (change 2 caps)".
fn payment_text(p: &Payment, forms: &fnv_sim::forms::FormStore) -> String {
    let mut parts = vec![format!("{} caps", p.caps)];
    parts.extend(p.notes.iter().map(|&(id, n)| format!("{n} x {}", crate::menu::model::display_name(forms, id))));
    let mut t = parts.join(", ");
    if p.change > 0 {
        t.push_str(&format!(" (change {} caps)", p.change));
    }
    if p.short > 0 {
        t.push_str(&format!(" ({} short)", p.short));
    }
    t
}

/// The caps and currency an actor can bet: its own and its merchant container's
/// (caravan.md §3).
fn caps_of(sim: &mut SimState, actor: FormId) -> (u32, Option<FormId>) {
    let stock = sim.refs.get(actor).and_then(|p| p.merchant_container);
    let holders: Vec<FormId> = std::iter::once(actor).chain(stock).collect();
    (purse_of(&mut sim.world, sim.forms, &holders).worth(), stock)
}

/// The line shown when the menu does not open [hypothesis: the words are NVForever's].
pub fn refusal_line(owned: usize) -> String {
    trf("You need at least {n} Caravan cards to play (you have {owned}).", &[("n", &MIN_DECK), ("owned", &owned)])
}

#[allow(clippy::too_many_arguments)]
fn open(
    mut requests: MessageReader<OpenCaravan>,
    state: Res<State<PlayState>>,
    mut next: ResMut<NextState<PlayState>>,
    mut screen: ResMut<CaravanScreen>,
    paused: Res<Paused>,
    mut sim: ResMut<SimState>,
    mut script: Option<ResMut<ScriptState>>,
    aid: Res<CaravanAid>,
    time: AidTime,
    (mut stats, mut corner, mut sounds, bindings): (ResMut<CaravanStats>, Option<ResMut<crate::corner_messages::CornerMessages>>, MessageWriter<UiSound>, Res<crate::bindings::Bindings>),
) {
    let Some(OpenCaravan(r)) = requests.read().last().copied() else { return };
    stats.asked += 1;
    if *state.get() != PlayState::Playing || screen.0.is_some() || paused.0 {
        info!("caravan: request against {} ignored, the player is not in the world", r.opponent);
        return;
    }
    let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) else {
        warn!("caravan: scripts are not running; the game needs the game's rules");
        return;
    };
    let Some(deck_def) = rt.game.data.caravan_decks.get(&r.deck).cloned() else {
        warn!("caravan: {} is not a Caravan deck", r.deck);
        return;
    };
    let owned = rules::owned(&rt.game);
    if owned.len() < MIN_DECK {
        stats.refused += 1;
        let line = refusal_line(owned.len());
        info!("caravan: refused against {}: {} cards owned: on screen: {line:?}", r.opponent, owned.len());
        if let Some(c) = corner.as_deref_mut() {
            c.push(line.into());
        }
        sounds.write(UiSound { menu: UiMenu::Caravan, event: UiEvent::Refused });
        return;
    }
    stats.opened += 1;
    let now = time.elapsed_secs();
    let mut rng = ChaCha8Rng::seed_from_u64(aid.seed.unwrap_or_else(|| seed(r.opponent, now)));
    let opponent_deck = rules::cards_of(&rt.game, &deck_def.cards);
    let skill = rules::skill(&opponent_deck);
    // The opponent's name as shown everywhere (`SimState::ref_name`: SetActorFullName,
    // the template's name).
    let base = sim.world.get(r.opponent).map(|s| s.base).unwrap_or(r.opponent);
    let opponent_name = sim.ref_name(Some(&rt.game), r.opponent.0).or_else(|| sim.name(base)).or_else(|| sim.name(r.opponent)).unwrap_or(tr("Opponent")).to_string();
    let forms = sim.forms;
    let purse = purse_of(&mut sim.world, forms, &[FormId::PLAYER_REF]);
    let funds = purse.worth();
    let (opponent_funds, _) = caps_of(&mut sim, r.opponent);
    let bet = Bet::new(funds, opponent_funds, r.max_bet_share);
    let mut textures = HashMap::new();
    let mut card_ranks = HashMap::new();
    for f in owned.iter().chain(deck_def.cards.iter()) {
        if let Some(d) = rt.game.data.caravan_cards.get(f) {
            textures.insert(*f, (d.face.clone(), d.back.clone()));
            card_ranks.insert(*f, d.rank);
        }
    }
    let (better, better_source) = rules::BetterCaravanConfig::load_from_game(&rt.game);
    let better_keys = BetterCaravanKeys::from_config(&better);
    if better.debug {
        info!(
            "caravan [BetterCaravan debug]: loaded from {:?}: ai_fixes={}, improved_ai={}, preview={}, highlight_winning={}, mod_cards={:?}x{}, view_npc_cards={}, keys={:?}",
            better_source,
            better.ai_fixes,
            better.improved_ai,
            better.preview_move_result,
            better.highlight_winning_caravans,
            better.mod_card_mode,
            better.mod_card_count,
            better.view_npc_cards,
            better_keys
        );
    }
    let deck = rules::saved_deck(&rt.game);
    let record = rt.game.player.caravan.records.get(&r.opponent).copied().unwrap_or_default();
    info!(
        "caravan: open against {opponent_name} ({}): deck {:?} ({}, {} cards, skill {skill:.2}, difficulty {} not used), bid {} of {opponent_funds} caps at {:.2}; the player has {funds} caps ({} in caps, {} in NCR dollars and Legion coins), {} cards owned, a saved deck of {}",
        r.opponent,
        deck_def.name,
        r.deck,
        opponent_deck.len(),
        r.difficulty,
        bet.bid,
        r.max_bet_share,
        purse.caps,
        funds - purse.caps,
        owned.len(),
        deck.len()
    );
    let _ = rng.next_u32();
    rt.game.open_menu(MENU_CARAVAN);
    let mut s = Session {
        opponent: r.opponent,
        opponent_name,
        deck_form: r.deck,
        opponent_deck,
        skill,
        stage: Stage::Bet,
        bet,
        owned,
        deck,
        cursor: 0,
        game: None,
        hand: 0,
        pick: Pick::Hand,
        forfeit_armed: false,
        record,
        status: None,
        help: None,
        queued_help: Vec::new(),
        think_until: now,
        over_at: None,
        leaving: None,
        staked: false,
        settled: false,
        auto: false,
        textures,
        card_ranks,
        better,
        better_keys,
        rng,
    };
    s.show_help(rt, rules::Help::Betting, &bindings);
    screen.0 = Some(s);
    sounds.write(UiSound { menu: UiMenu::Caravan, event: UiEvent::Opened });
    next.set(PlayState::Menu);
}

/// Resolve keyboard/wheel inputs for `stage`, including optional BetterCaravan
/// directional keys (`sKeyUp`, `sKeyDown`, `sKeyLeft`, `sKeyRight`). When `better_keys`
/// has all fields `None` (INI absent), produces the exact vanilla inputs [design].
pub(crate) fn stage_inputs(stage: Stage, better_keys: &BetterCaravanKeys, just_pressed: impl Fn(KeyCode) -> bool, any_pressed: bool, wheel_y: f32) -> Vec<CaravanInput> {
    let mut out = Vec::new();
    let send = |out: &mut Vec<CaravanInput>, k: KeyCode, i: CaravanInput| {
        if just_pressed(k) {
            out.push(i);
        }
    };
    let send_extra = |out: &mut Vec<CaravanInput>, extra: Option<KeyCode>, existing: &[KeyCode], i: CaravanInput| {
        if extra.is_some_and(|k| !existing.contains(&k) && just_pressed(k)) {
            out.push(i);
        }
    };
    const DECK_VANILLA_KEYS: &[KeyCode] = &[KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyF, KeyCode::Escape, KeyCode::ArrowLeft, KeyCode::ArrowRight];
    const PLAY_VANILLA_KEYS: &[KeyCode] = &[KeyCode::KeyW, KeyCode::KeyQ, KeyCode::KeyE, KeyCode::KeyR, KeyCode::ArrowLeft, KeyCode::ArrowRight, KeyCode::KeyA, KeyCode::KeyD];
    match stage {
        Stage::Bet => {
            send(&mut out, KeyCode::KeyW, CaravanInput::AutoMatch);
            send(&mut out, KeyCode::KeyA, CaravanInput::Raise);
            send(&mut out, KeyCode::KeyF, CaravanInput::Accept);
            send(&mut out, KeyCode::KeyR, CaravanInput::Exit);
            send(&mut out, KeyCode::Escape, CaravanInput::Exit);
        }
        Stage::Deck => {
            send(&mut out, KeyCode::KeyW, CaravanInput::Add);
            send_extra(&mut out, better_keys.up, DECK_VANILLA_KEYS, CaravanInput::Add);
            send(&mut out, KeyCode::KeyA, CaravanInput::Remove);
            send_extra(&mut out, better_keys.down, DECK_VANILLA_KEYS, CaravanInput::Remove);
            send(&mut out, KeyCode::KeyS, CaravanInput::Randomize);
            send(&mut out, KeyCode::KeyF, CaravanInput::PlayDeck);
            send(&mut out, KeyCode::Escape, CaravanInput::Exit);
            send(&mut out, KeyCode::ArrowLeft, CaravanInput::Step(-1));
            send_extra(&mut out, better_keys.left, DECK_VANILLA_KEYS, CaravanInput::Step(-1));
            send(&mut out, KeyCode::ArrowRight, CaravanInput::Step(1));
            send_extra(&mut out, better_keys.right, DECK_VANILLA_KEYS, CaravanInput::Step(1));
            if wheel_y != 0.0 {
                out.push(CaravanInput::Step(if wheel_y > 0.0 { -1 } else { 1 }));
            }
        }
        Stage::Play => {
            send(&mut out, KeyCode::KeyW, CaravanInput::Select);
            send(&mut out, KeyCode::KeyQ, CaravanInput::Discard);
            send(&mut out, KeyCode::KeyE, CaravanInput::DiscardTrack);
            send(&mut out, KeyCode::KeyR, CaravanInput::Cancel);
            send(&mut out, KeyCode::ArrowLeft, CaravanInput::Step(-1));
            send(&mut out, KeyCode::ArrowRight, CaravanInput::Step(1));
            send(&mut out, KeyCode::KeyA, CaravanInput::Step(-1));
            send(&mut out, KeyCode::KeyD, CaravanInput::Step(1));
            send_extra(&mut out, better_keys.left, PLAY_VANILLA_KEYS, CaravanInput::Step(-1));
            send_extra(&mut out, better_keys.right, PLAY_VANILLA_KEYS, CaravanInput::Step(1));
            send_extra(&mut out, better_keys.up, PLAY_VANILLA_KEYS, CaravanInput::StepVertical(-1));
            send_extra(&mut out, better_keys.down, PLAY_VANILLA_KEYS, CaravanInput::StepVertical(1));
            if wheel_y != 0.0 {
                out.push(CaravanInput::Step(if wheel_y > 0.0 { -1 } else { 1 }));
            }
        }
        Stage::Results => {
            if any_pressed {
                out.push(CaravanInput::Continue);
            }
        }
    }
    out
}

fn keys(keys: crate::pad::ActionButtons<KeyCode>, wheel: Res<AccumulatedMouseScroll>, screen: Res<CaravanScreen>, mut out: MessageWriter<CaravanInput>) {
    let keys = keys.snapshot();
    let Some(s) = screen.0.as_ref() else { return };
    if s.leaving.is_some() {
        return;
    }
    // A help message closes on its own keys; the stage's other keys still act ([design]: a forfeit pressed under the opening's help was lost), and `apply` closes
    // the message before acting.
    let help = s.help.is_some();
    const CLOSE: [KeyCode; 4] = [KeyCode::KeyE, KeyCode::Escape, KeyCode::Tab, KeyCode::Enter];
    if help && keys.any_just_pressed(CLOSE) {
        out.write(CaravanInput::CloseHelp);
        return;
    }
    // Under a help message its closing keys only close it (above), and no key counts as
    // the results' "any key".
    let any_pressed = !help && keys.get_just_pressed().next().is_some();
    for cmd in stage_inputs(s.stage, &s.better_keys, |k| keys.just_pressed(k) && !(help && CLOSE.contains(&k)), any_pressed, wheel.delta.y) {
        out.write(cmd);
    }
}

type Clicked<'a> = (&'a PointerState, AnyOf<(&'a screen::CaravanButton, &'a crate::lockpick::screen::LockButton)>);

fn buttons(mut screen: ResMut<CaravanScreen>, pressed: Query<Clicked, Changed<PointerState>>, mouse: crate::pad::ActionButtons<MouseButton>, mut out: MessageWriter<CaravanInput>) {
    let mouse = mouse.snapshot();
    let Some(s) = screen.0.as_mut() else { return };
    if s.stage == Stage::Results && s.help.is_none() && mouse.just_pressed(MouseButton::Left) {
        out.write(CaravanInput::Continue);
        return;
    }
    for (i, (b, help)) in &pressed {
        if let (PointerState::Hovered, Some(screen::CaravanButton(CaravanInput::Place(n)))) = (i, b) {
            if s.better.preview_move_result && s.stage == Stage::Play {
                match &mut s.pick {
                    Pick::Place { moves, at } if *n < moves.len() && *at != *n => *at = *n,
                    Pick::Track { tracks, at } if *n < tracks.len() && *at != *n => *at = *n,
                    _ => {}
                }
            }
        }
        if *i != PointerState::Pressed {
            continue;
        }
        if let Some(b) = b {
            out.write(b.0);
        }
        if help.is_some_and(|h| h.0 == crate::lockpick::LockpickInput::CloseHelp) {
            out.write(CaravanInput::CloseHelp);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply(
    mut input: MessageReader<CaravanInput>,
    mut screen: ResMut<CaravanScreen>,
    mut sim: ResMut<SimState>,
    mut script: Option<ResMut<ScriptState>>,
    time: AidTime,
    mut stats: ResMut<CaravanStats>,
    mut sounds: MessageWriter<UiSound>,
    (bindings, mut commands): (Res<crate::bindings::Bindings>, Commands),
) {
    let now = time.elapsed_secs();
    let sound = |sounds: &mut MessageWriter<UiSound>, event| {
        sounds.write(UiSound { menu: UiMenu::Caravan, event });
    };
    for &cmd in input.read() {
        let Some(s) = screen.0.as_mut() else { return };
        let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) else { return };
        if cmd == CaravanInput::CloseHelp {
            if s.help.take().is_some() {
                info!("caravan: help message closed");
                if !s.queued_help.is_empty() {
                    let next = s.queued_help.remove(0);
                    s.show_help(rt, next, &bindings);
                }
            }
            continue;
        }
        if s.leaving.is_some() {
            continue;
        }
        if s.help.is_some() {
            // An action under a help message closes it and still acts; a message queued
            // after it comes up in its place.
            s.help = None;
            info!("caravan: help message closed by {cmd:?}");
            if !s.queued_help.is_empty() {
                let next = s.queued_help.remove(0);
                s.show_help(rt, next, &bindings);
            }
        }
        match (s.stage, cmd) {
            (Stage::Bet, CaravanInput::AutoMatch) => {
                if s.bet.auto_match() {
                    sound(&mut sounds, UiEvent::Value { up: true });
                }
            }
            (Stage::Bet, CaravanInput::Raise) => {
                if s.bet.raise() {
                    sound(&mut sounds, UiEvent::Value { up: true });
                }
            }
            (Stage::Bet, CaravanInput::Accept) => {
                if !s.bet.can_accept() {
                    sound(&mut sounds, UiEvent::Refused);
                    continue;
                }
                info!("caravan: bet accepted: ante {} (pot {})", s.bet.ante, s.bet.pot());
                s.stage = Stage::Deck;
                s.status = None;
                sound(&mut sounds, UiEvent::Picked);
                s.show_help(rt, rules::Help::Deck, &bindings);
            }
            (Stage::Bet | Stage::Deck, CaravanInput::Exit) => s.begin_leaving(now, "the player left before the game"),
            (Stage::Deck, CaravanInput::Step(n)) => {
                let len = s.owned.len() as i32;
                let c = (s.cursor as i32 + n).clamp(0, (len - 1).max(0)) as usize;
                if c != s.cursor {
                    s.cursor = c;
                    sound(&mut sounds, UiEvent::Focus);
                }
            }
            (Stage::Deck, CaravanInput::CursorTo(i)) if i < s.owned.len() => {
                if i != s.cursor {
                    s.cursor = i;
                    sound(&mut sounds, UiEvent::Focus);
                }
            }
            (Stage::Deck, CaravanInput::Add) => match s.owned.get(s.cursor).copied() {
                Some(f) if !s.in_deck(f) => {
                    s.deck.push(f);
                    sound(&mut sounds, UiEvent::Picked);
                    s.cursor = (s.cursor + 1).min(s.owned.len().saturating_sub(1));
                }
                _ => sound(&mut sounds, UiEvent::Refused),
            },
            (Stage::Deck, CaravanInput::Remove) => match s.owned.get(s.cursor).copied() {
                Some(f) if s.in_deck(f) => {
                    s.deck.retain(|&d| d != f);
                    sound(&mut sounds, UiEvent::Picked);
                }
                _ => sound(&mut sounds, UiEvent::Refused),
            },
            (Stage::Deck, CaravanInput::Randomize) => {
                s.deck = rules::randomize(&s.owned, &mut s.rng);
                info!("caravan: random deck of {} cards", s.deck.len());
                sound(&mut sounds, UiEvent::Shuffled);
            }
            (Stage::Deck, CaravanInput::PlayDeck) => {
                if s.deck.len() < MIN_DECK {
                    s.status = Some(trf("Your Caravan deck needs at least {n} cards.", &[("n", &MIN_DECK)]));
                    sound(&mut sounds, UiEvent::Refused);
                    continue;
                }
                start_game(s, rt, &mut sim, now, &bindings);
                stats.games += 1;
                sound(&mut sounds, UiEvent::Accepted);
            }
            (Stage::Play, CaravanInput::Tips) => {
                set_tips(!tips());
                info!("caravan: tips {}", if tips() { "on" } else { "off" });
                commands.queue(crate::settings::keep);
                sound(&mut sounds, UiEvent::Value { up: tips() });
            }
            (Stage::Play, CaravanInput::Forfeit) => forfeit(s, rt, &mut sim, now, &mut stats, &mut sounds),
            (Stage::Play, CaravanInput::Cancel) => match s.pick {
                Pick::Place { .. } | Pick::Track { .. } => {
                    s.pick = Pick::Hand;
                    sound(&mut sounds, UiEvent::Back);
                }
                Pick::Hand if s.game.as_ref().is_some_and(|g| g.phase != Phase::Over) => {
                    if s.forfeit_armed {
                        forfeit(s, rt, &mut sim, now, &mut stats, &mut sounds);
                    } else {
                        s.forfeit_armed = true;
                        s.status = Some(trf("Press {key} again to forfeit the game.", &[("key", &s.hint(&[KeyCode::KeyR]))]));
                    }
                }
                _ => {}
            },
            (Stage::Play, cmd) if s.players_turn() => {
                if cmd != CaravanInput::Cancel && s.forfeit_armed {
                    s.forfeit_armed = false;
                    s.status = None;
                }
                player_input(s, rt, &mut sim, cmd, now, &mut stats, &mut sounds, &bindings);
            }
            (Stage::Results, CaravanInput::Continue) => s.begin_leaving(now, "the results were seen"),
            _ => {}
        }
    }
}

/// The ante taken, the deck saved and both decks dealt.
fn start_game(s: &mut Session, rt: &mut crate::script::Runtime, sim: &mut SimState, now: f32, bindings: &crate::bindings::Bindings) {
    rt.game.player.caravan.deck = s.deck.clone();
    let ante = s.bet.ante;
    let forms = sim.forms;
    let paid = with_world(rt, sim, |rt| pay(&mut rt.game.world, forms, &[FormId::PLAYER_REF], ante));
    info!("caravan: the player's ante {ante} paid with {}", payment_text(&paid, forms));
    s.staked = true;
    let player_forms = if s.better.mod_card_count > 0 {
        let modified = rules::apply_deck_mod(&s.deck, &s.owned, s.better.mod_card_mode, s.better.mod_card_count, &mut s.rng);
        if s.better.debug {
            info!(
                "caravan [BetterCaravan debug]: deck mod {:?} (count {}): {} -> {} cards",
                s.better.mod_card_mode,
                s.better.mod_card_count,
                s.deck.len(),
                modified.len()
            );
        }
        modified
    } else {
        s.deck.clone()
    };
    let player = rules::cards_of(&rt.game, &player_forms);
    let game = Game::new(player, s.opponent_deck.clone(), &mut s.rng);
    info!("caravan: game {} against {}: the player's deck {} cards, the opponent's {} (skill {:.2}); ante {ante} taken", s.record.won + s.record.lost + 1, s.opponent_name, player_forms.len(), s.opponent_deck.len(), s.skill);
    s.game = Some(game);
    s.stage = Stage::Play;
    s.status = None;
    s.hand = 0;
    s.pick = Pick::Hand;
    s.think_until = now + think(&mut s.rng);
    s.show_help(rt, rules::Help::Starting, bindings);
}

fn think(rng: &mut ChaCha8Rng) -> f32 {
    let (lo, hi) = THINK_SECONDS;
    lo + (rng.next_u32() as f32 / u32::MAX as f32) * (hi - lo)
}

/// Vertical screen order within a track: opponent cards top-to-bottom (`-len..-1`),
/// player cards top-to-bottom (`0..len`). Returns `None` when stepping down past the
/// bottommost move on the track (caller returns to hand) [design].
pub(crate) fn step_vertical(moves: &[Move], at: usize, dir: i32) -> Option<usize> {
    let Move::Play { track, .. } = *moves.get(at)? else {
        return Some(at);
    };
    let mut col: Vec<(i32, usize)> = moves
        .iter()
        .enumerate()
        .filter_map(|(idx, m)| match *m {
            Move::Play { side, track: t, on, .. } if t == track => {
                let y_rank = if side == OPPONENT { -(on.unwrap_or(0) as i32) - 1 } else { on.unwrap_or(0) as i32 };
                Some((y_rank, idx))
            }
            _ => None,
        })
        .collect();
    col.sort_by_key(|&(rank, idx)| (rank, idx));
    let pos = col.iter().position(|&(_, idx)| idx == at)?;
    if dir < 0 {
        Some(col[pos.saturating_sub(1)].1)
    } else if pos + 1 < col.len() {
        Some(col[pos + 1].1)
    } else {
        None
    }
}

#[allow(clippy::too_many_arguments)]
fn player_input(s: &mut Session, rt: &mut crate::script::Runtime, sim: &mut SimState, cmd: CaravanInput, now: f32, stats: &mut CaravanStats, sounds: &mut MessageWriter<UiSound>, bindings: &crate::bindings::Bindings) {
    let sound = |sounds: &mut MessageWriter<UiSound>, event| {
        sounds.write(UiSound { menu: UiMenu::Caravan, event });
    };
    let hand_len = s.game.as_ref().map_or(0, |g| g.sides[PLAYER].hand.len());
    let choose_card = |s: &mut Session, i: usize, sounds: &mut MessageWriter<UiSound>| {
        let moves: Vec<Move> = s.hand_moves(i).collect();
        if moves.is_empty() {
            // The card picked before goes back into the hand: only the refused card, the
            // one in focus, stays raised [design].
            s.pick = Pick::Hand;
            s.status = Some(tr("That card cannot be played now.").into());
            sound(sounds, UiEvent::Refused);
        } else {
            s.status = None;
            s.pick = Pick::Place { moves, at: 0 };
            sound(sounds, UiEvent::CardFlipped);
        }
    };
    let mut chosen: Option<Move> = None;
    match (cmd, s.pick.clone()) {
        (CaravanInput::Step(n), Pick::Hand) if hand_len > 0 => {
            let h = (s.hand as i32 + n).rem_euclid(hand_len as i32) as usize;
            s.hand = h;
            sound(sounds, UiEvent::Focus);
        }
        (CaravanInput::Step(n), Pick::Place { moves, at }) => {
            s.pick = Pick::Place { at: (at as i32 + n).rem_euclid(moves.len() as i32) as usize, moves };
            sound(sounds, UiEvent::Switch);
        }
        (CaravanInput::Step(n), Pick::Track { tracks, at }) => {
            s.pick = Pick::Track { at: (at as i32 + n).rem_euclid(tracks.len() as i32) as usize, tracks };
            sound(sounds, UiEvent::Switch);
        }
        (CaravanInput::StepVertical(dir), Pick::Hand) if dir < 0 => choose_card(s, s.hand, sounds),
        (CaravanInput::StepVertical(dir), Pick::Place { moves, at }) => match step_vertical(&moves, at, dir) {
            Some(next) if next != at => {
                s.pick = Pick::Place { moves, at: next };
                sound(sounds, UiEvent::Switch);
            }
            Some(_) => {}
            None => {
                s.pick = Pick::Hand;
                sound(sounds, UiEvent::Back);
            }
        },
        (CaravanInput::StepVertical(dir), Pick::Track { .. }) if dir > 0 => {
            s.pick = Pick::Hand;
            sound(sounds, UiEvent::Back);
        }
        (CaravanInput::Select, Pick::Hand) => choose_card(s, s.hand, sounds),
        (CaravanInput::PickCard(i), _) if i < hand_len => {
            s.hand = i;
            choose_card(s, i, sounds);
        }
        (CaravanInput::Select, Pick::Place { moves, at }) => chosen = moves.get(at).copied(),
        (CaravanInput::Place(i), Pick::Place { moves, .. }) => chosen = moves.get(i).copied(),
        (CaravanInput::Place(i), Pick::Track { tracks, .. }) => chosen = tracks.get(i).map(|&track| Move::DiscardTrack { track }),
        (CaravanInput::Select | CaravanInput::DiscardTrack, Pick::Track { tracks, at }) => chosen = tracks.get(at).map(|&track| Move::DiscardTrack { track }),
        (CaravanInput::Discard, Pick::Hand | Pick::Place { .. }) if hand_len > 0 => {
            let card = match s.pick {
                Pick::Place { moves: ref m, .. } => match m.first() {
                    Some(Move::Play { card, .. }) => *card,
                    _ => s.hand,
                },
                _ => s.hand,
            };
            chosen = Some(Move::Discard { card });
        }
        (CaravanInput::DiscardTrack, Pick::Hand | Pick::Place { .. }) => {
            let tracks: Vec<usize> = s.game.as_ref().map(|g| g.moves_for(PLAYER).into_iter().filter_map(|m| if let Move::DiscardTrack { track } = m { Some(track) } else { None }).collect()).unwrap_or_default();
            if tracks.is_empty() {
                sound(sounds, UiEvent::Refused);
            } else {
                s.pick = Pick::Track { tracks, at: 0 };
                sound(sounds, UiEvent::Switch);
            }
        }
        _ => {}
    }
    if let Some(m) = chosen {
        make_move(s, rt, sim, PLAYER, m, now, stats, sounds, bindings);
    }
}

/// A move by either side: carried out, heard, logged; the next side waits its turn.
#[allow(clippy::too_many_arguments)]
fn make_move(s: &mut Session, rt: &mut crate::script::Runtime, sim: &mut SimState, by: usize, m: Move, now: f32, stats: &mut CaravanStats, sounds: &mut MessageWriter<UiSound>, bindings: &crate::bindings::Bindings) {
    let Some(g) = s.game.as_mut() else { return };
    let what = describe(g, by, m);
    let events = match g.apply(by, m) {
        Ok(e) => e,
        Err(e) => {
            info!("caravan: {} move refused ({e:?}): {what}", side_name(by));
            sounds.write(UiSound { menu: UiMenu::Caravan, event: UiEvent::Refused });
            return;
        }
    };
    stats.moves += 1;
    let values: Vec<String> = (0..2).map(|side| g.sides[side].caravans.iter().map(|c| c.value().to_string()).collect::<Vec<_>>().join("/")).collect();
    info!("caravan: {} {what}; caravans player {} opponent {}; cards left {} / {}", side_name(by), values[PLAYER], values[OPPONENT], g.sides[PLAYER].cards_left(), g.sides[OPPONENT].cards_left());
    let mut over = None;
    for e in &events {
        let event = match e {
            Event::Played { .. } => Some(UiEvent::CardDealt),
            Event::Removed { cards, side, track } => {
                info!("caravan: {} cards leave the {} caravan on track {}", cards.len(), side_name(*side), track + 1);
                Some(UiEvent::Collected { dealer: *side == OPPONENT })
            }
            Event::TrackDiscarded { .. } | Event::Discarded { .. } => Some(UiEvent::Collected { dealer: by == OPPONENT }),
            Event::OpeningOver => {
                info!("caravan: the opening is over");
                None
            }
            Event::Over { winner, end } => {
                over = Some((*winner, *end));
                None
            }
            Event::Drew { .. } => None,
        };
        if let Some(event) = event {
            sounds.write(UiSound { menu: UiMenu::Caravan, event });
        }
    }
    if events.contains(&Event::OpeningOver) {
        s.show_help(rt, rules::Help::War, bindings);
    }
    s.pick = Pick::Hand;
    let hand_len = s.game.as_ref().map_or(0, |g| g.sides[PLAYER].hand.len());
    s.hand = s.hand.min(hand_len.saturating_sub(1));
    let next_player = s.game.as_ref().is_some_and(|g| g.turn == PLAYER);
    s.think_until = now + if s.auto && next_player { AUTO_SECONDS } else { think(&mut s.rng) };
    if let Some((winner, end)) = over {
        settle(s, rt, sim, winner, end, now, stats, sounds);
    }
}

fn describe(g: &Game, by: usize, m: Move) -> String {
    match m {
        Move::Play { card, side, track, on } => {
            let c = g.sides[by].hand.get(card).map_or("?".into(), Card::short);
            match on {
                None => format!("plays {c} on its caravan {}", track + 1),
                Some(i) => {
                    let t = g.sides[side].caravans[track].cards.get(i).map_or("?".into(), |p| p.card.short());
                    format!("plays {c} on {t} of the {} caravan {}", side_name(side), track + 1)
                }
            }
        }
        Move::Discard { card } => format!("discards {}", g.sides[by].hand.get(card).map_or("?".into(), Card::short)),
        Move::DiscardTrack { track } => format!("discards its caravan {}", track + 1),
    }
}

fn side_name(side: usize) -> &'static str {
    if side == PLAYER {
        "player"
    } else {
        "opponent"
    }
}

#[allow(clippy::too_many_arguments)]
fn forfeit(s: &mut Session, rt: &mut crate::script::Runtime, sim: &mut SimState, now: f32, stats: &mut CaravanStats, sounds: &mut MessageWriter<UiSound>) {
    let Some(g) = s.game.as_mut() else { return };
    if g.phase == Phase::Over {
        return;
    }
    g.forfeit(PLAYER);
    info!("caravan: the player forfeits");
    settle(s, rt, sim, OPPONENT, End::Forfeit, now, stats, sounds);
}

/// The pot to the winner, the record and the statistics (caravan.md §3, §7).
#[allow(clippy::too_many_arguments)]
fn settle(s: &mut Session, rt: &mut crate::script::Runtime, sim: &mut SimState, winner: usize, end: End, now: f32, stats: &mut CaravanStats, sounds: &mut MessageWriter<UiSound>) {
    if s.settled {
        return;
    }
    s.settled = true;
    s.over_at = Some(now);
    let ante = s.bet.ante;
    let (_, stock) = caps_of(sim, s.opponent);
    let opponent = s.opponent;
    let forms = sim.forms;
    let paid = with_world(rt, sim, |rt| {
        let w = &mut rt.game.world;
        if winner == PLAYER {
            let _ = w.add_item(FormId::PLAYER_REF, CAPS, (ante * 2) as i32);
            // The opponent's ante: its merchant container's first, then its own.
            let holders: Vec<FormId> = stock.into_iter().chain(std::iter::once(opponent)).collect();
            Some(pay(w, forms, &holders, ante))
        } else {
            let _ = w.add_item(stock.unwrap_or(opponent), CAPS, ante as i32);
            None
        }
    });
    if let Some(p) = paid {
        info!("caravan: the opponent's ante {ante} paid with {}", payment_text(&p, forms));
    }
    let mut record = rt.game.player.caravan.records.get(&opponent).copied().unwrap_or_default();
    if winner == PLAYER {
        record.won += 1;
        record.winnings += ante as u64;
        record.biggest_pot = record.biggest_pot.max(ante);
        rt.game.count_stat(fnv_game::misc_stats::CARAVAN_GAMES_WON, 1.0);
        stats.won += 1;
        stats.net += ante as i64;
    } else {
        record.lost += 1;
        record.losses += ante as u64;
        rt.game.count_stat(fnv_game::misc_stats::CARAVAN_GAMES_LOST, 1.0);
        stats.lost += 1;
        stats.net -= ante as i64;
    }
    rt.game.player.caravan.records.insert(opponent, record);
    s.record = record;
    // Each result one whole text, so a translation need not join pieces (#3146).
    let with: &[(&str, &dyn std::fmt::Display)] = &[("ante", &ante)];
    let line = match (winner, end) {
        (PLAYER, End::Sold) => trf("You sold two of three caravans and win {ante} caps.", with),
        (PLAYER, End::OutOfCards) => trf("Your opponent is out of cards. You win {ante} caps.", with),
        (PLAYER, End::Forfeit) => trf("Your opponent gives up. You win {ante} caps.", with),
        (_, End::Sold) => trf("Your opponent sold two of three caravans. You lose {ante} caps.", with),
        (_, End::OutOfCards) => trf("You are out of cards. You lose {ante} caps.", with),
        (_, End::Forfeit) => trf("You forfeit. You lose {ante} caps.", with),
    };
    info!("caravan: game over: {} wins ({end:?}); ante {ante}; record {}/{} against {}, net {}; on screen: {line:?}", side_name(winner), s.record.won, s.record.lost, s.opponent_name, s.record.net());
    s.status = Some(line);
    s.pick = Pick::Hand;
    sounds.write(UiSound { menu: UiMenu::Caravan, event: UiEvent::RoundOver { won: winner == PLAYER } });
}

/// The opponent's moves (and the player's under `auto`), the results after the game,
/// leaving.
#[allow(clippy::too_many_arguments)]
fn step(time: AidTime, mut screen: ResMut<CaravanScreen>, mut sim: ResMut<SimState>, mut script: Option<ResMut<ScriptState>>, mut stats: ResMut<CaravanStats>, mut sounds: MessageWriter<UiSound>, mut next: ResMut<NextState<PlayState>>, bindings: Res<crate::bindings::Bindings>) {
    let Some(s) = screen.0.as_mut() else { return };
    let now = time.elapsed_secs();
    if let Some((since, why)) = s.leaving {
        if now - since >= LEAVE_SECONDS {
            info!("caravan: closed ({why})");
            next.set(PlayState::Playing);
        }
        return;
    }
    if s.help.is_some() {
        if s.auto {
            info!("caravan: help message closed (auto)");
            s.help = None;
            if !s.queued_help.is_empty() {
                let next = s.queued_help.remove(0);
                if let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) {
                    s.show_help(rt, next, &bindings);
                }
            }
        }
        return;
    }
    if let Some(at) = s.over_at {
        if s.stage == Stage::Play && now - at >= END_SECONDS {
            s.stage = Stage::Results;
            info!("caravan: results: winnings {}, losses {}, {}/{}, biggest {}", s.record.winnings, s.record.losses, s.record.won, s.record.lost, s.record.biggest_pot);
        }
        if s.stage == Stage::Results && s.auto && now - at >= END_SECONDS + AUTO_RESULTS {
            s.begin_leaving(now, "the results were seen");
        }
        return;
    }
    if s.stage != Stage::Play || now < s.think_until {
        return;
    }
    let Some(g) = s.game.as_ref() else { return };
    let by = g.turn;
    if by == PLAYER && !s.auto {
        return;
    }
    let skill = if by == PLAYER { AUTO_SKILL } else { s.skill };
    let Some(m) = rules::choose_with_config(g, by, skill, &s.better, &mut s.rng) else { return };
    if s.better.debug {
        info!(
            "caravan [BetterCaravan debug]: AI ({}, skill {:.2}, ai_fixes={}, improved_ai={}) chose {}",
            side_name(by),
            skill,
            s.better.ai_fixes,
            s.better.improved_ai,
            describe(g, by, m)
        );
    }
    let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) else { return };
    make_move(s, rt, &mut sim, by, m, now, &mut stats, &mut sounds, &bindings);
}

fn close(mut screen: ResMut<CaravanScreen>, mut sim: ResMut<SimState>, script: Option<ResMut<ScriptState>>, mut stats: ResMut<CaravanStats>, mut sounds: MessageWriter<UiSound>) {
    let Some(mut s) = screen.0.take() else { return };
    sounds.write(UiSound { menu: UiMenu::Caravan, event: UiEvent::Closed });
    let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) else { return };
    // Closed in the middle of a game (not by the menu): a forfeit.
    if s.staked && !s.settled {
        if let Some(g) = s.game.as_mut() {
            g.forfeit(PLAYER);
        }
        settle(&mut s, rt, &mut sim, OPPONENT, End::Forfeit, 0.0, &mut stats, &mut sounds);
    }
    info!("caravan: closed against {}: record {}/{}, net {}", s.opponent_name, s.record.won, s.record.lost, s.record.net());
    if rt.game.menu == Some(MENU_CARAVAN) {
        rt.game.close_menu();
    }
}

fn report(mut exit: MessageReader<AppExit>, stats: Res<CaravanStats>) {
    if exit.read().next().is_none() || stats.asked == 0 {
        return;
    }
    let s = &*stats;
    info!("caravan summary: {} asked, {} opened ({} refused); {} games: {} won, {} lost; net {}; {} moves", s.asked, s.opened, s.refused, s.games, s.won, s.lost, s.net, s.moves);
}

/// [design, #2403] Synthetic logical/ineffective states for detector regressions.
#[cfg(feature = "report-test-support")]
#[doc(hidden)]
pub fn report_progress_fixture(step: usize) -> CaravanScreen {
    CaravanScreen(Some(Session {
        opponent: FormId(42),
        opponent_name: String::new(),
        deck_form: FormId(43),
        opponent_deck: vec![],
        skill: 50.0,
        stage: if step >= 1 { Stage::Deck } else { Stage::Bet },
        bet: Bet::new(100, 100, 0.5),
        owned: vec![],
        deck: if step >= 2 { vec![FormId(44)] } else { vec![] },
        cursor: usize::from(step >= 3),
        game: None,
        hand: 0,
        pick: Pick::Hand,
        forfeit_armed: step >= 4,
        record: Default::default(),
        status: None,
        help: None,
        queued_help: vec![],
        think_until: step as f32,
        over_at: None,
        leaving: None,
        staked: false,
        settled: false,
        auto: false,
        textures: Default::default(),
        card_ranks: Default::default(),
        better: Default::default(),
        better_keys: Default::default(),
        rng: ChaCha8Rng::seed_from_u64(step as u64),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ante_is_paid_in_caps_then_the_least_valued_notes() {
        let (ncr5, ncr20, aureus) = (FormId(1), FormId(2), FormId(3));
        let purse = Purse { caps: 10, notes: vec![(ncr20, 3, 8), (ncr5, 2, 2), (aureus, 1, 100)] };
        assert_eq!(purse.worth(), 10 + 24 + 4 + 100);
        // Caps cover it.
        assert_eq!(pay_plan(&purse, 7), Payment { caps: 7, ..Default::default() });
        // 10 caps, both $5 notes (4), then one $20 note (8) for the last 1: change 7.
        assert_eq!(pay_plan(&purse, 15), Payment { caps: 10, notes: vec![(ncr5, 2), (ncr20, 1)], change: 7, short: 0 });
        // Everything but the aureus, then the aureus.
        let p = pay_plan(&purse, 50);
        assert_eq!(p.notes, vec![(ncr5, 2), (ncr20, 3), (aureus, 1)]);
        assert_eq!((p.change, p.short), (100 - 12, 0));
        // More than the purse holds.
        assert_eq!(pay_plan(&Purse { caps: 3, notes: vec![] }, 5).short, 2);
    }

    #[test]
    fn the_tie_aid_builds_both_caravans_to_the_value_from_their_own_decks() {
        use fnv_game::caravan::{Rank, Suit};
        let deck = |base: u32| (0..30u32).map(|i| Card::new(FormId(base + i), Suit::Hearts, Rank::Num((i % 10 + 1) as u8))).collect::<Vec<_>>();
        let mut g = Game::new(deck(0x100), deck(0x200), &mut ChaCha8Rng::seed_from_u64(7));
        g.phase = Phase::War;
        let before: usize = g.sides.iter().map(|s| s.deck.len()).sum();
        assert!(tie_track(&mut g, PLAYER, TIE_TRACK, 24) && tie_track(&mut g, OPPONENT, TIE_TRACK, 24));
        assert_eq!((g.sides[PLAYER].caravans[TIE_TRACK].value(), g.sides[OPPONENT].caravans[TIE_TRACK].value()), (24, 24));
        assert!(g.sides[PLAYER].caravans[TIE_TRACK].cards.iter().all(|p| p.card.form.0 / 0x100 == 1), "the player's own cards");
        assert!(guide::tie_note(&g).is_some_and(|n| n.starts_with("NEW RENO and REDDING are tied at 24")));
        let after: usize = g.sides.iter().map(|s| s.deck.len() + s.caravans[TIE_TRACK].cards.len()).sum();
        assert_eq!(after, before, "no card made or lost");
        // Nothing adds up to it: nothing changes.
        assert!(!tie_track(&mut g, PLAYER, TIE_TRACK, 41));
        assert_eq!(parse_aid("30:tie:24").unwrap()[0].1, AidStep::Tie(24));
    }

    #[test]
    fn the_aid_reads_its_steps() {
        let s = parse_aid("1:seed:7,1:caps:500,2:cards:15D7A2,3:open:104C5A:15D7A2:0.4,4:match,5:accept,6:randomize,7:play,8:auto,9:forfeit,10:continue,11:exit").unwrap();
        assert_eq!(s[0], (1.0, AidStep::Seed(7)));
        assert_eq!(s[1].1, AidStep::Caps(500));
        assert_eq!(s[2].1, AidStep::Cards(0x15D7A2));
        assert_eq!(s[3].1, AidStep::Open(CaravanRequest { opponent: FormId(0x104C5A), deck: FormId(0x15D7A2), difficulty: 0, max_bet_share: 0.4 }));
        assert_eq!(s[4].1, AidStep::Input(CaravanInput::AutoMatch));
        assert_eq!(s[8].1, AidStep::Auto);
        assert_eq!(s[11].1, AidStep::Input(CaravanInput::Exit));
        assert!(parse_aid("1:fold").is_err());
        assert!(parse_aid("1:open:zz").is_err());
    }

    #[test]
    fn the_refusal_says_how_many_cards_are_needed() {
        assert!(refusal_line(12).contains("30"));
        assert!(refusal_line(12).contains("12"));
    }

    #[test]
    fn better_caravan_keys_parse_bind_and_deduplicate() {
        let absent = BetterCaravanKeys::default();
        // Absent INI: I/J/K/L do nothing in Deck or Play.
        for k in [KeyCode::KeyI, KeyCode::KeyJ, KeyCode::KeyK, KeyCode::KeyL] {
            assert!(stage_inputs(Stage::Deck, &absent, |x| x == k, true, 0.0).is_empty());
            assert!(stage_inputs(Stage::Play, &absent, |x| x == k, true, 0.0).is_empty());
        }

        let cfg = BetterCaravanConfig::parse("[Better Caravan]\nsKeyUp = i\nsKeyDown = k\nsKeyLeft = j\nsKeyRight = l\n");
        let keys = BetterCaravanKeys::from_config(&cfg);
        assert_eq!(keys.up, Some(KeyCode::KeyI));
        assert_eq!(keys.down, Some(KeyCode::KeyK));
        assert_eq!(keys.left, Some(KeyCode::KeyJ));
        assert_eq!(keys.right, Some(KeyCode::KeyL));

        assert_eq!(stage_inputs(Stage::Deck, &keys, |x| x == KeyCode::KeyI, true, 0.0), vec![CaravanInput::Add]);
        assert_eq!(stage_inputs(Stage::Deck, &keys, |x| x == KeyCode::KeyK, true, 0.0), vec![CaravanInput::Remove]);
        assert_eq!(stage_inputs(Stage::Deck, &keys, |x| x == KeyCode::KeyJ, true, 0.0), vec![CaravanInput::Step(-1)]);
        assert_eq!(stage_inputs(Stage::Deck, &keys, |x| x == KeyCode::KeyL, true, 0.0), vec![CaravanInput::Step(1)]);

        assert_eq!(stage_inputs(Stage::Play, &keys, |x| x == KeyCode::KeyI, true, 0.0), vec![CaravanInput::StepVertical(-1)]);
        assert_eq!(stage_inputs(Stage::Play, &keys, |x| x == KeyCode::KeyK, true, 0.0), vec![CaravanInput::StepVertical(1)]);
        assert_eq!(stage_inputs(Stage::Play, &keys, |x| x == KeyCode::KeyJ, true, 0.0), vec![CaravanInput::Step(-1)]);
        assert_eq!(stage_inputs(Stage::Play, &keys, |x| x == KeyCode::KeyL, true, 0.0), vec![CaravanInput::Step(1)]);

        // Overlapping bindings with any vanilla key bound in that stage (e.g. WASD)
        // never emit duplicate or conflicting extra actions.
        let wasd_cfg = BetterCaravanConfig::parse("[Better Caravan]\nsKeyUp = w\nsKeyDown = s\nsKeyLeft = a\nsKeyRight = d\n");
        let wasd_keys = BetterCaravanKeys::from_config(&wasd_cfg);
        assert_eq!(stage_inputs(Stage::Deck, &wasd_keys, |x| x == KeyCode::KeyW, true, 0.0), vec![CaravanInput::Add]);
        assert_eq!(stage_inputs(Stage::Deck, &wasd_keys, |x| x == KeyCode::KeyS, true, 0.0), vec![CaravanInput::Randomize]);
        assert_eq!(stage_inputs(Stage::Deck, &wasd_keys, |x| x == KeyCode::KeyA, true, 0.0), vec![CaravanInput::Remove]);
        assert_eq!(stage_inputs(Stage::Deck, &wasd_keys, |x| x == KeyCode::KeyD, true, 0.0), vec![CaravanInput::Step(1)]);
        assert_eq!(stage_inputs(Stage::Play, &wasd_keys, |x| x == KeyCode::KeyW, true, 0.0), vec![CaravanInput::Select]);
        assert_eq!(stage_inputs(Stage::Play, &wasd_keys, |x| x == KeyCode::KeyS, true, 0.0), vec![CaravanInput::StepVertical(1)]);
        assert_eq!(stage_inputs(Stage::Play, &wasd_keys, |x| x == KeyCode::KeyA, true, 0.0), vec![CaravanInput::Step(-1)]);
        assert_eq!(stage_inputs(Stage::Play, &wasd_keys, |x| x == KeyCode::KeyD, true, 0.0), vec![CaravanInput::Step(1)]);

        let dup_cfg = BetterCaravanConfig::parse("[Better Caravan]\nsKeyLeft = a\nsKeyRight = ArrowRight\n");
        let dup_keys = BetterCaravanKeys::from_config(&dup_cfg);
        assert_eq!(stage_inputs(Stage::Play, &dup_keys, |x| x == KeyCode::KeyA, true, 0.0), vec![CaravanInput::Step(-1)]);
        assert_eq!(stage_inputs(Stage::Play, &dup_keys, |x| x == KeyCode::ArrowRight, true, 0.0), vec![CaravanInput::Step(1)]);

        // Special key names & None.
        assert_eq!(parse_better_caravan_key("ArrowUp"), Some(KeyCode::ArrowUp));
        assert_eq!(parse_better_caravan_key("Numpad8"), Some(KeyCode::Numpad8));
        assert_eq!(parse_better_caravan_key("None"), None);
        assert_eq!(parse_better_caravan_key(""), None);
    }

    #[test]
    fn better_caravan_vertical_stepping_walks_track_and_returns_to_hand() {
        let moves = vec![
            Move::Play { card: 0, side: PLAYER, track: 0, on: Some(0) },
            Move::Play { card: 0, side: PLAYER, track: 0, on: Some(1) },
            Move::Play { card: 0, side: OPPONENT, track: 0, on: Some(0) },
            Move::Play { card: 0, side: OPPONENT, track: 0, on: Some(1) },
            Move::Play { card: 0, side: PLAYER, track: 1, on: Some(0) },
        ];
        // From PLAYER[1] (idx 1) stepping up: PLAYER[0] (0) -> OPPONENT[0] (2) -> OPPONENT[1] (3) -> stays 3.
        assert_eq!(step_vertical(&moves, 1, -1), Some(0));
        assert_eq!(step_vertical(&moves, 0, -1), Some(2));
        assert_eq!(step_vertical(&moves, 2, -1), Some(3));
        assert_eq!(step_vertical(&moves, 3, -1), Some(3));

        // Stepping down from OPPONENT[1] (3): OPPONENT[0] (2) -> PLAYER[0] (0) -> PLAYER[1] (1) -> None (hand).
        assert_eq!(step_vertical(&moves, 3, 1), Some(2));
        assert_eq!(step_vertical(&moves, 2, 1), Some(0));
        assert_eq!(step_vertical(&moves, 0, 1), Some(1));
        assert_eq!(step_vertical(&moves, 1, 1), None);

        // Numbered card row: down returns None (back to hand), up stays put.
        let num_moves = vec![
            Move::Play { card: 0, side: PLAYER, track: 0, on: None },
            Move::Play { card: 0, side: PLAYER, track: 1, on: None },
        ];
        assert_eq!(step_vertical(&num_moves, 0, -1), Some(0));
        assert_eq!(step_vertical(&num_moves, 0, 1), None);
    }

    #[test]
    fn better_caravan_screen_highlight_preview_and_face_descriptions() {
        use fnv_game::caravan::{Caravan, Placed, Side};

        let c = |s: &str| Card::parse(s).unwrap();
        let pile = |cards: &[&str]| Caravan { cards: cards.iter().map(|s| Placed { card: c(s), faces: Vec::new() }).collect() };
        let game = Game {
            sides: [
                Side {
                    deck: vec![c("2c"); 10],
                    hand: vec![c("6h"), c("Ks"), c("Jd")],
                    caravans: [pile(&["10h", "8h"]), pile(&["10s", "9s", "5s"]), pile(&["10c", "9c", "8c"])],
                },
                Side {
                    deck: vec![c("2d"); 10],
                    hand: vec![c("7h")],
                    caravans: [pile(&["10d", "6d", "5d"]), pile(&["10d", "9d", "5d"]), pile(&["5d"])],
                },
            ],
            turn: PLAYER,
            phase: Phase::War,
            winner: None,
            moves: 0,
        };
        // Track 0: player 18 vs opponent 21 (opponent winning).
        // Track 1: player 24 vs opponent 24 (tied, neither winning).
        // Track 2: player 27 (bust > 26) vs opponent 5.

        // The sold-range white (sRGB 1.0) and the winning white (`Color::WHITE`, linear) are the
        // same 8-bit colour; compare them as shown.
        let px = |c: Color| { let s = c.to_srgba(); [s.red, s.green, s.blue].map(|v| (v * 255.0).round() as u8) };
        // Vanilla (highlight off, no preview): a value in the sold range (21) is white, as in
        // the original (the ship's `value_colour`), bust 27 is red.
        let (t0, c0) = screen::value_display(&game, OPPONENT, 0, None, false);
        assert_eq!((t0.as_str(), px(c0)), ("21", px(Color::WHITE)));
        let (t2, c2) = screen::value_display(&game, PLAYER, 2, None, false);
        assert_eq!(t2, "27");
        assert_ne!(px(c2), px(Color::WHITE));
        assert_ne!(c2, crate::interface::amber(255.0));

        // Highlight winning caravans on: opponent track 0 (21) is white; tied track 1 (24) is
        // white too, being in the sold range; an unsold value stays amber.
        let (t0w, c0w) = screen::value_display(&game, OPPONENT, 0, None, true);
        assert_eq!((t0w.as_str(), px(c0w)), ("21", px(Color::WHITE)));
        let (t1w, c1w) = screen::value_display(&game, PLAYER, 1, None, true);
        assert_eq!((t1w.as_str(), px(c1w)), ("24", px(Color::WHITE)));
        let (t2o, c2o) = screen::value_display(&game, OPPONENT, 2, None, true);
        assert_eq!((t2o.as_str(), c2o), ("5", crate::interface::amber(255.0)));

        // Preview move result: playing 6h on player track 0 turns 18 -> 24 (winning white!).
        let mut better = BetterCaravanConfig::parse(
            "[Better Caravan]\nbPreviewMoveResult = 1\nbHighlightWinningCaravans = 1\nsDescriptionKing = King doubles.\nsDescriptionJack = Jack removes.\n",
        );
        let king_form = FormId(0x100);
        let num_form = FormId(0x101);
        let mut card_ranks = HashMap::new();
        card_ranks.insert(king_form, Rank::King);
        card_ranks.insert(num_form, Rank::Num(6));

        let mut session = Session {
            opponent: FormId(1),
            opponent_name: "Ringo".into(),
            deck_form: FormId(2),
            opponent_deck: vec![],
            skill: 0.3,
            stage: Stage::Play,
            bet: Bet::new(100, 100, 0.25),
            owned: vec![king_form, num_form],
            deck: vec![],
            cursor: 0,
            game: Some(game.clone()),
            hand: 1, // Ks
            pick: Pick::Place { moves: vec![Move::Play { card: 0, side: PLAYER, track: 0, on: None }], at: 0 },
            forfeit_armed: false,
            record: Default::default(),
            status: None,
            help: None,
            queued_help: vec![],
            think_until: 0.0,
            over_at: None,
            leaving: None,
            staked: true,
            settled: false,
            auto: false,
            textures: Default::default(),
            card_ranks,
            better: better.clone(),
            better_keys: BetterCaravanKeys::from_config(&better),
            rng: ChaCha8Rng::seed_from_u64(1),
        };

        let prev = screen::active_preview(&session).expect("preview active");
        let (pt, pc) = screen::value_display(&game, PLAYER, 0, Some(&prev), true);
        assert_eq!((pt.as_str(), px(pc)), ("18->24", px(Color::WHITE)));

        // Preview disabled -> None.
        better.preview_move_result = false;
        session.better = better.clone();
        assert!(screen::active_preview(&session).is_none());

        // Face-card description in Play (Pick::Hand on Ks -> King; on 6h -> None) and Deck.
        session.pick = Pick::Hand;
        session.hand = 1; // Ks
        assert_eq!(screen::active_face_description(&session), Some("King doubles."));
        session.hand = 2; // Jd
        assert_eq!(screen::active_face_description(&session), Some("Jack removes."));
        session.hand = 0; // 6h
        assert_eq!(screen::active_face_description(&session), None);

        session.stage = Stage::Deck;
        session.cursor = 0; // king_form
        assert_eq!(screen::active_face_description(&session), Some("King doubles."));
        session.cursor = 1; // num_form
        assert_eq!(screen::active_face_description(&session), None);
    }
}
