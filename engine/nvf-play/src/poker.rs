//! Poker (docs/design/poker.md): Texas Hold'em at a casino table against four opponents,
//! NVForever's own minigame (the original has none). The rules and the opponents are
//! `fnv_game::poker`; the table is drawn by the engine (nvforever `poker_view.rs`) into
//! [`PokerView`], which `screen` shows under the text.
//!
//! - Opened by the console's `poker [casino]` or the test aid ([`OpenPoker`]); refused with
//!   fewer than [`MIN_CAPS`] caps (a corner message).
//! - The player's stack is the buy-in ([`BUY_IN`] or the caps carried, the smaller); no caps
//!   change hands until the player leaves, when the difference is added or taken
//!   ([`settle`]). Leaving in the middle of a hand forfeits what is in the pot.
//! - It is a `PlayState::Menu`: the world and the game clock stand still.
//! - The casino's floor is heard and its radio station plays (`sound`); the world's music,
//!   loops and radios are quiet meanwhile.
//!
//! Keys: E checks or calls, R raises to the amount shown, the mouse wheel or Up/Down change
//! it by the big blind (with Shift by five), A all in, F folds, Space deals the next hand
//! at once, Tab or Esc leaves. The choices can be clicked.
//!
//! Test aid `NVF_PLAY_TEST_POKER=<seconds>:<step>[,…]`: `seed:<n>` and `caps:<n>` (before
//! `open`), `open[:<casino letter>]`, `check`, `call`, `raise:<to>`, `allin`, `fold`, `auto`
//! (the player's seat plays by the opponents' rules), `next`, `exit`. At exit, when the
//! table was asked for: "poker summary: …".

pub mod screen;
pub mod sound;

use crate::pause::{AidTime, Paused};
use crate::sim::SimState;
use crate::{PlaySet, PlayState};
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use crate::ui_pointer::PointerState;
use fnv_game::poker::{self as rules, Action, Event, Seat, Street, Style, Table};
use fnv_sim::FormId;
use rand_chacha::ChaCha8Rng;
use rand_core::SeedableRng;

/// The player's seat: nearest the camera.
pub const PLAYER: usize = 0;
pub const OPPONENTS: usize = 4;
/// The most a buy-in takes, and the least caps to sit down with [design].
pub const BUY_IN: u32 = 200;
pub const MIN_CAPS: u32 = 100;
pub const SMALL_BLIND: u32 = 5;
pub const BIG_BLIND: u32 = 10;
/// An opponent's stack: from and to [design].
pub const OPPONENT_STACK: (u32, u32) = (150, 300);
/// Seconds an opponent takes over a choice, from and to; the player's seat when it plays
/// by itself (`auto`) the same [design].
pub const THINK_SECONDS: (f32, f32) = (0.6, 1.2);
/// Seconds from a hand's end to the next deal [design].
pub const NEXT_HAND_SECONDS: f32 = 3.0;
/// Seconds the table stays after the game ends (the player out, or every opponent).
pub const END_SECONDS: f32 = 3.0;
/// Seconds the menu takes to fade as it closes.
pub const LEAVE_SECONDS: f32 = 0.5;
/// Seconds between two cards dealt, and from a hand's start to its first card (the
/// shuffle is heard first) [design].
pub const DEAL_STEP: f32 = 0.12;
pub const SHUFFLE_SECONDS: f32 = 0.9;

/// The casino whose cards, felt and name are used (docs/design/poker.md §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Casino {
    Tops,
    Gomorrah,
    UltraLuxe,
    Lucky38,
    Plain,
}

impl Casino {
    pub fn from_letter(s: &str) -> Option<Casino> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "" | "t" | "tops" => Casino::Tops,
            "g" | "gomorrah" => Casino::Gomorrah,
            "u" | "ultraluxe" | "ultra-luxe" => Casino::UltraLuxe,
            "l" | "lucky38" => Casino::Lucky38,
            "b" | "plain" => Casino::Plain,
            _ => return None,
        })
    }

    /// The deck's letter in the card textures' names.
    pub fn deck(self) -> char {
        match self {
            Casino::Tops => 't',
            Casino::Gomorrah => 'g',
            Casino::UltraLuxe => 'u',
            Casino::Lucky38 => 'l',
            Casino::Plain => 'b',
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Casino::Tops => "The Tops",
            Casino::Gomorrah => "Gomorrah",
            Casino::UltraLuxe => "The Ultra-Luxe",
            Casino::Lucky38 => "Lucky 38",
            Casino::Plain => "Card Table",
        }
    }

    /// The casino's name printed on the felt, if the game has one for it.
    pub fn logo(self) -> Option<&'static str> {
        match self {
            Casino::Tops => Some("textures\\terminals\\nv_blackjack\\nv_blackjack-casino-logo01.dds"),
            Casino::Gomorrah => Some("textures\\terminals\\nv_blackjack\\nv_blackjack-casino-logo02.dds"),
            Casino::UltraLuxe => Some("textures\\terminals\\nv_blackjack\\nv_blackjack-casino-logo03.dds"),
            _ => None,
        }
    }

    pub fn felt(self) -> &'static str {
        match self {
            Casino::UltraLuxe => "textures\\terminals\\nv_blackjack\\nv_blackjack-table02u.dds",
            _ => "textures\\terminals\\nv_blackjack\\nv_blackjack-table02.dds",
        }
    }

    pub fn card_texture(self, card: rules::Card) -> String {
        let d = self.deck();
        format!("textures\\terminals\\nv_playingcards\\deck{d}\\card{d}_{}.dds", card.texture_code())
    }

    pub fn back_texture(self) -> String {
        let d = self.deck();
        format!("textures\\terminals\\nv_playingcards\\deck{d}\\card{d}_back.dds")
    }
}

/// Ask for the table (the console, the test aid).
#[derive(Message, Clone, Copy, Debug)]
pub struct OpenPoker {
    pub casino: Casino,
    /// The poker table in the world it was opened at, if any.
    pub table: Option<FormId>,
}

/// The table's picture, drawn by the engine while it is open; `anchors`: where each seat's
/// label goes, as fractions of the view from its top left (the engine projects the seats).
/// No picture: the table stands at a table in the world, seen by the game's own camera.
#[derive(Resource, Clone, Debug)]
pub struct PokerView {
    pub image: Option<Handle<Image>>,
    pub anchors: Vec<Vec2>,
}

/// The open table, if any.
#[derive(Resource, Default)]
pub struct PokerScreen(pub Option<Session>);

pub struct Session {
    pub table: Table,
    pub casino: Casino,
    pub buy_in: u32,
    /// The line in the middle: a hand's result, the game's end.
    pub status: Option<String>,
    /// What the player will raise to with R.
    pub raise_to: u32,
    /// Aid seconds when the table opened, when the hand's first card was dealt and when
    /// each street's cards came (the view slides the cards in from then: [`Session::card_time`]).
    pub opened: f32,
    pub dealt_at: f32,
    pub street_at: [f32; 3],
    /// The player's seat plays by itself (test aid `auto`).
    pub auto: bool,
    /// The game is over (why), since when.
    pub ended: Option<(f32, String)>,
    pub leaving: Option<f32>,
    next_choice: Option<f32>,
    next_hand: Option<f32>,
    /// Sounds to play (`sound`): when (aid seconds) and which record.
    pub cues: Vec<(f32, &'static str)>,
    /// Hands won by a person at the table, to show as highlights then.
    pub wins: Vec<(f32, String)>,
    /// Hands played and won by the player, for the summary.
    hands: u32,
    won: u32,
    /// The poker table in the world this was opened at: the engine
    /// draws the table there, the room around it.
    pub table_ref: Option<FormId>,
}

impl Session {
    /// When a seat's `k`th hole card is dealt: one card each from the small blind round,
    /// then the second (aid seconds).
    pub fn card_time(&self, seat: usize, k: usize) -> f32 {
        let n = self.table.seats.len();
        let dealt: Vec<usize> = (1..=n).map(|d| (self.table.button + d) % n).filter(|&i| !self.table.seats[i].hole.is_empty()).collect();
        let pos = dealt.iter().position(|&i| i == seat).unwrap_or(0);
        self.dealt_at + (pos + k * dealt.len()) as f32 * DEAL_STEP
    }

    /// When the board's `j`th card is dealt.
    pub fn board_time(&self, j: usize) -> f32 {
        let (street, k) = if j < 3 { (0, j) } else { (j - 2, 0) };
        self.street_at[street] + k as f32 * DEAL_STEP * 1.5
    }

    pub fn player(&self) -> &Seat {
        &self.table.seats[PLAYER]
    }

    /// The player's turn, with nothing in the way.
    /// Where the table is for its controller buttons (`crate::pad::menu_buttons`): between
    /// hands A deals the next.
    pub fn hint_at(&self) -> crate::pad::MenuAt {
        let tab = if self.table.street == Street::Over { crate::pad::POKER_BETWEEN_HANDS } else { 0 };
        crate::pad::MenuAt { code: Some(crate::pad::POKER_MENU), tab }
    }

    pub fn players_turn(&self) -> bool {
        self.table.to_act == Some(PLAYER) && !self.auto && self.leaving.is_none() && self.ended.is_none()
    }

    /// What the player keeps on leaving now: the stack, less what is in the pot.
    pub fn stack_on_leaving(&self) -> u32 {
        self.player().stack
    }

    pub fn fade(&self, now: f32) -> f32 {
        self.leaving.map_or(1.0, |since| (1.0 - (now - since) / LEAVE_SECONDS).clamp(0.0, 1.0))
    }

    /// The least and most the player's raise may be, while it is the player's turn.
    pub fn raise_range(&self) -> Option<(u32, u32)> {
        self.table.legal(PLAYER).raise
    }

    fn clamp_raise(&mut self) {
        if let Some((lo, hi)) = self.raise_range() {
            self.raise_to = self.raise_to.clamp(lo, hi);
        }
    }

    fn begin_leaving(&mut self, now: f32, why: &str) {
        if self.leaving.is_none() {
            info!("poker: leaving ({why})");
            self.leaving = Some(now);
        }
    }
}

/// A command for the open table.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub enum PokerInput {
    /// Check or call, whichever the bet asks.
    CheckCall,
    /// Raise to the amount chosen, or to this.
    Raise(Option<u32>),
    AllIn,
    Fold,
    /// Change the raise by this many big blinds.
    Adjust(i32),
    Next,
    Auto,
    Exit,
}

#[derive(Resource, Default, Debug)]
struct PokerStats {
    asked: u32,
    opened: u32,
    refused: u32,
    hands: u32,
    won: u32,
    settled: i64,
}

/// `NVF_PLAY_TEST_POKER`, and a request waiting for the player to be in the world (the
/// console asks while its own menu is still open).
#[derive(Resource, Default)]
struct PokerAid {
    steps: Vec<(f32, AidStep)>,
    seed: Option<u64>,
    waiting: Option<(OpenPoker, f32)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum AidStep {
    Open(Casino),
    Seed(u64),
    Caps(u32),
    Input(PokerInput),
}

fn parse_aid(v: &str) -> Result<Vec<(f32, AidStep)>, String> {
    crate::test_aid::timed(v)?
        .into_iter()
        .map(|(t, what)| {
            let (op, arg) = what.split_once(':').unwrap_or((what, ""));
            let num = |name: &str| arg.trim().parse::<u64>().map_err(|_| format!("{what:?}: expected {op}:<{name}>"));
            let step = match op {
                "open" => AidStep::Open(Casino::from_letter(arg).ok_or_else(|| format!("{what:?}: expected open:<t, g, u, l or b>"))?),
                "seed" => AidStep::Seed(num("n")?),
                "caps" => AidStep::Caps(num("n")? as u32),
                "check" | "call" => AidStep::Input(PokerInput::CheckCall),
                "raise" if arg.is_empty() => AidStep::Input(PokerInput::Raise(None)),
                "raise" => AidStep::Input(PokerInput::Raise(Some(num("to")? as u32))),
                "allin" => AidStep::Input(PokerInput::AllIn),
                "fold" => AidStep::Input(PokerInput::Fold),
                "auto" => AidStep::Input(PokerInput::Auto),
                "next" => AidStep::Input(PokerInput::Next),
                "exit" => AidStep::Input(PokerInput::Exit),
                _ => return Err(format!("{what:?}: expected seed:, caps:, open, check, call, raise, allin, fold, auto, next or exit")),
            };
            Ok((t, step))
        })
        .collect()
}

pub struct PokerPlugin;

impl Plugin for PokerPlugin {
    fn build(&self, app: &mut App) {
        let steps = match std::env::var("NVF_PLAY_TEST_POKER").map(|v| parse_aid(&v)) {
            Ok(Ok(a)) => a,
            Ok(Err(e)) => {
                warn!("NVF_PLAY_TEST_POKER: {e}");
                Vec::new()
            }
            Err(_) => Vec::new(),
        };
        app.init_resource::<PokerScreen>()
            .init_resource::<PokerStats>()
            .insert_resource(PokerAid { steps, ..default() })
            .add_message::<OpenPoker>()
            .add_message::<PokerInput>()
            .add_systems(
                Update,
                (run_aid, open, (keys, buttons, apply, step).chain().run_if(in_state(PlayState::Menu)), screen::draw, screen::fade, sound::sound)
                    .chain()
                    .after(crate::menu::open_menus)
                    .in_set(PlaySet::Ui),
            )
            .add_systems(OnExit(PlayState::Menu), close)
            .add_systems(Last, report);
    }
}

fn run_aid(time: AidTime, mut aid: ResMut<PokerAid>, mut sim: ResMut<SimState>, mut open: MessageWriter<OpenPoker>, mut input: MessageWriter<PokerInput>) {
    if aid.steps.is_empty() {
        return;
    }
    let Some(now) = time.world_secs() else { return };
    let mut due: Vec<(f32, AidStep)> = aid.steps.iter().filter(|(t, _)| *t <= now).copied().collect();
    due.sort_by(|a, b| a.0.total_cmp(&b.0));
    aid.steps.retain(|(t, _)| *t > now);
    for (_, step) in due {
        info!("test aid NVF_PLAY_TEST_POKER: {step:?}");
        match step {
            AidStep::Open(casino) => {
                open.write(OpenPoker { casino, table: None });
            }
            AidStep::Seed(s) => aid.seed = Some(s),
            AidStep::Caps(n) => {
                let have = caps(&mut sim);
                let r = if n > have { sim.world.add_item(FormId::PLAYER_REF, CAPS, (n - have) as i32).map(|_| ()) } else { sim.world.remove_item(FormId::PLAYER_REF, CAPS, (have - n) as i32).map(|_| ()) };
                if let Err(e) = r {
                    warn!("test aid NVF_PLAY_TEST_POKER: caps: {e}");
                }
                info!("test aid NVF_PLAY_TEST_POKER: the player's caps {have} -> {}", caps(&mut sim));
            }
            AidStep::Input(i) => {
                input.write(i);
            }
        }
    }
}

/// The caps item [observed: MISC 0x0000000F `Caps001`].
pub const CAPS: FormId = crate::menu::model::CAPS;

fn caps(sim: &mut SimState) -> u32 {
    sim.world.item_count(FormId::PLAYER_REF, CAPS).unwrap_or(0).max(0) as u32
}

fn seed_now(now: f32) -> u64 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    t ^ now.to_bits() as u64
}

/// The line when the table refuses [design: NVForever's words].
pub fn refusal_line() -> String {
    format!("You need at least {MIN_CAPS} caps to play.")
}

#[allow(clippy::too_many_arguments)]
fn open(
    mut requests: MessageReader<OpenPoker>,
    state: Res<State<PlayState>>,
    mut next: ResMut<NextState<PlayState>>,
    mut screen: ResMut<PokerScreen>,
    paused: Res<Paused>,
    mut sim: ResMut<SimState>,
    mut aid: ResMut<PokerAid>,
    time: AidTime,
    (mut stats, mut corner): (ResMut<PokerStats>, Option<ResMut<crate::corner_messages::CornerMessages>>),
) {
    let now = time.elapsed_secs();
    if let Some(r) = requests.read().last().copied() {
        stats.asked += 1;
        aid.waiting = Some((r, now));
    }
    let Some((asked, at)) = aid.waiting else { return };
    if *state.get() != PlayState::Playing || screen.0.is_some() || paused.0 {
        // The console asks with its own menu open: wait for it to close.
        if now - at > 2.0 {
            info!("poker: request ignored, the player is not in the world");
            aid.waiting = None;
        }
        return;
    }
    aid.waiting = None;
    let have = caps(&mut sim);
    if have < MIN_CAPS {
        stats.refused += 1;
        let line = refusal_line();
        info!("poker: refused ({have} caps): on screen: {line:?}");
        if let Some(c) = corner.as_deref_mut() {
            c.push(line.into());
        }
        return;
    }
    let buy_in = have.min(BUY_IN);
    let seed = aid.seed.unwrap_or_else(|| seed_now(now));
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0x5EA7);
    let mut seats = vec![Seat::new("You", buy_in, Style { tightness: 0.5, aggression: 0.5, bluff: 0.05 })];
    for i in 1..=OPPONENTS {
        let (lo, hi) = OPPONENT_STACK;
        let stack = (lo + ((hi - lo) as f32 * rules::unit(&mut rng)) as u32) / SMALL_BLIND * SMALL_BLIND;
        seats.push(Seat::new(&format!("Seat {i}"), stack, Style::random(&mut rng)));
    }
    let table = Table::new(seats, SMALL_BLIND, BIG_BLIND, seed);
    info!(
        "poker: open at {} (seed {seed}): buy-in {buy_in} of {have} caps, blinds {SMALL_BLIND}/{BIG_BLIND}; opponents {}",
        asked.casino.title(),
        table.seats[1..].iter().map(|s| format!("{} {} (tight {:.2}, aggressive {:.2}, bluff {:.2})", s.name, s.stack, s.style.tightness, s.style.aggression, s.style.bluff)).collect::<Vec<_>>().join(", ")
    );
    stats.opened += 1;
    let mut s = new_session(table, asked.casino, buy_in, now);
    s.table_ref = asked.table;
    screen.0 = Some(s);
    next.set(PlayState::Menu);
}

fn new_session(table: Table, casino: Casino, buy_in: u32, now: f32) -> Session {
    Session {
        table,
        casino,
        buy_in,
        status: None,
        raise_to: 2 * BIG_BLIND,
        opened: now,
        dealt_at: now,
        street_at: [now; 3],
        auto: false,
        ended: None,
        leaving: None,
        next_choice: None,
        next_hand: Some(now + 1.0),
        cues: Vec::new(),
        wins: Vec::new(),
        hands: 0,
        won: 0,
        table_ref: None,
    }
}

fn keys(keys: crate::pad::ActionButtons<KeyCode>, scroll: Option<Res<AccumulatedMouseScroll>>, screen: Res<PokerScreen>, mut out: MessageWriter<PokerInput>) {
    let keys = keys.snapshot();
    let Some(s) = screen.0.as_ref() else { return };
    if s.leaving.is_some() {
        return;
    }
    if keys.any_just_pressed([KeyCode::Escape, KeyCode::Tab]) {
        out.write(PokerInput::Exit);
        return;
    }
    if keys.just_pressed(KeyCode::Space) {
        out.write(PokerInput::Next);
    }
    if keys.just_pressed(KeyCode::KeyE) {
        out.write(PokerInput::CheckCall);
    }
    if keys.just_pressed(KeyCode::KeyR) {
        out.write(PokerInput::Raise(None));
    }
    if keys.just_pressed(KeyCode::KeyA) {
        out.write(PokerInput::AllIn);
    }
    if keys.just_pressed(KeyCode::KeyF) {
        out.write(PokerInput::Fold);
    }
    let big = if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) { 5 } else { 1 };
    let mut steps = 0;
    if keys.just_pressed(KeyCode::ArrowUp) {
        steps += big;
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        steps -= big;
    }
    if let Some(y) = scroll.map(|s| s.delta.y).filter(|y| *y != 0.0) {
        steps += y.signum() as i32 * big;
    }
    if steps != 0 {
        out.write(PokerInput::Adjust(steps));
    }
}

fn buttons(screen: Res<PokerScreen>, pressed: Query<(&PointerState, &screen::PokerButton), Changed<PointerState>>, mut out: MessageWriter<PokerInput>) {
    if screen.0.is_none() {
        return;
    }
    for (i, b) in &pressed {
        if *i == PointerState::Pressed {
            out.write(b.0);
        }
    }
}

/// The table's events into the log, and the screen's lines.
fn log_events(s: &mut Session, events: &[Event], now: f32) {
    for e in events {
        let name = |i: usize| s.table.seats[i].name.clone();
        match e {
            Event::HandStarted { number, button } => {
                // The last hand's cards gathered, a shuffle, then a card at a time.
                if *number > 1 {
                    s.cues.push((now, sound::COLLECT_DEALER));
                }
                s.cues.push((now + 0.2, sound::SHUFFLE));
                s.dealt_at = now + SHUFFLE_SECONDS;
                for i in 0..s.table.seats.len() {
                    for k in 0..s.table.seats[i].hole.len() {
                        let t = s.card_time(i, k);
                        s.cues.push((t, sound::DEAL));
                    }
                }
                s.status = None;
                info!("poker: hand {number}: {} has the button; the player holds {}", name(*button), s.player().hole.iter().map(|c| c.short()).collect::<Vec<_>>().join(" "));
            }
            Event::Blind { seat, amount, big } => {
                s.cues.push((now, sound::CHIPS));
                info!("poker: {} posts the {} blind {amount}", name(*seat), if *big { "big" } else { "small" });
            }
            Event::Acted { seat, action, put } => {
                s.cues.push((now, match action {
                    Action::Fold => sound::FOLD,
                    Action::Check => sound::CHECK,
                    _ => sound::CHIPS,
                }));
                let what = match action {
                    Action::Fold => "folds".to_string(),
                    Action::Check => "checks".to_string(),
                    Action::Call => format!("calls {put}"),
                    Action::Raise(to) => format!("raises to {to}"),
                    Action::AllIn => format!("goes all in ({put})"),
                };
                info!("poker: {} {what}", name(*seat));
            }
            Event::Street { street, cards } => {
                let i = match street {
                    Street::Flop => 0,
                    Street::Turn => 1,
                    _ => 2,
                };
                s.street_at[i] = now;
                let first = s.table.community.len() - cards.len();
                for j in first..s.table.community.len() {
                    let t = s.board_time(j);
                    s.cues.push((t, sound::DEAL));
                    s.cues.push((t + DEAL_STEP, sound::FLIP));
                }
                info!("poker: {}: {}", street.name(), cards.iter().map(|c| c.short()).collect::<Vec<_>>().join(" "));
            }
            Event::Shown { seat, value } => {
                if !s.cues.iter().any(|c| c.1 == sound::FLIP && c.0 >= now) {
                    s.cues.push((now, sound::FLIP));
                }
                info!("poker: {} shows {} ({})", name(*seat), s.table.seats[*seat].hole.iter().map(|c| c.short()).collect::<Vec<_>>().join(" "), value.describe());
            }
            Event::Won { seat, amount, value } => {
                let with = value.map(|v| format!(" with {}", v.describe())).unwrap_or_default();
                info!("poker: {} wins {amount}{with}", name(*seat));
                s.cues.push((now + 0.3, sound::COLLECT_CHIPS));
                if *seat == PLAYER {
                    s.cues.push((now + 0.1, sound::WIN));
                }
                // The player's win is a highlight.
                if *seat == PLAYER {
                    s.wins.push((now + 0.3, fnv_util::words::tr("YOU WIN THE HAND").to_string()));
                }
                let who = if *seat == PLAYER { "You win".to_string() } else { format!("{} wins", name(*seat)) };
                let line = format!("{who} {amount}{with}");
                s.status = Some(match &s.status {
                    Some(prev) => format!("{prev}   {line}"),
                    None => line,
                });
                if *seat == PLAYER {
                    s.won += 1;
                }
            }
            Event::Out { seat } => info!("poker: {} is out of chips", name(*seat)),
            Event::HandOver => {
                // Beaten at the showdown.
                if s.table.showdown && s.player().in_hand() && !s.table.winners.iter().any(|w| w.0 == PLAYER) {
                    s.cues.push((now + 0.1, sound::LOSE));
                }
                s.hands += 1;
                info!("poker: hand {} over: stacks {}", s.table.hand, s.table.seats.iter().map(|x| format!("{} {}", x.name, x.stack)).collect::<Vec<_>>().join(", "));
                s.next_hand = Some(now + NEXT_HAND_SECONDS);
            }
        }
    }
}

fn apply(mut input: MessageReader<PokerInput>, mut screen: ResMut<PokerScreen>, time: AidTime) {
    let now = time.elapsed_secs();
    for &cmd in input.read() {
        let Some(s) = screen.0.as_mut() else { return };
        if s.leaving.is_some() {
            continue;
        }
        match cmd {
            PokerInput::Exit => {
                let why = if s.table.street != Street::Over && s.player().in_hand() { "the player left in the middle of a hand" } else { "the player left" };
                s.begin_leaving(now, why);
            }
            PokerInput::Next => {
                if s.next_hand.is_some() {
                    s.next_hand = Some(now);
                }
            }
            PokerInput::Auto => {
                s.auto = true;
                info!("poker: the player's seat plays by itself");
            }
            PokerInput::Adjust(steps) => {
                let before = s.raise_to;
                s.raise_to = (s.raise_to as i64 + steps as i64 * BIG_BLIND as i64).max(0) as u32;
                s.clamp_raise();
                match s.raise_to.cmp(&before) {
                    std::cmp::Ordering::Greater => s.cues.push((now, sound::BET_UP)),
                    std::cmp::Ordering::Less => s.cues.push((now, sound::BET_DOWN)),
                    std::cmp::Ordering::Equal => s.cues.push((now, sound::INVALID)),
                }
            }
            _ if !s.players_turn() => {}
            PokerInput::CheckCall | PokerInput::Raise(_) | PokerInput::AllIn | PokerInput::Fold => {
                let action = match cmd {
                    PokerInput::CheckCall => Action::Call,
                    PokerInput::Raise(to) => Action::Raise(to.unwrap_or(s.raise_to)),
                    PokerInput::AllIn => Action::AllIn,
                    _ => Action::Fold,
                };
                match s.table.act(PLAYER, action) {
                    Ok(ev) => log_events(s, &ev, now),
                    Err(e) => {
                        info!("poker: the player's {action:?} refused: {e}");
                        s.cues.push((now, sound::INVALID));
                    }
                }
            }
        }
    }
}

/// The table's frame: opponents' choices, the next deal, the game's end, leaving.
fn step(time: AidTime, mut screen: ResMut<PokerScreen>, mut next: ResMut<NextState<PlayState>>, mut stats: ResMut<PokerStats>) {
    let Some(s) = screen.0.as_mut() else { return };
    let now = time.elapsed_secs();
    if let Some(since) = s.leaving {
        if now - since >= LEAVE_SECONDS {
            info!("poker: closed");
            next.set(PlayState::Playing);
        }
        return;
    }
    if let Some((since, _)) = &s.ended {
        if now - since >= END_SECONDS {
            s.begin_leaving(now, "the game is over");
        }
        return;
    }
    if s.table.street == Street::Over {
        // The game is over when the player is out or every opponent is.
        let over = if s.player().out {
            Some("You are out of chips".to_string())
        } else if s.table.playing() < 2 {
            Some("You cleaned out the table".to_string())
        } else {
            None
        };
        if let Some(why) = over {
            info!("poker: the game is over: {why}");
            s.status = Some(why.clone());
            s.ended = Some((now, why));
            return;
        }
        if s.next_hand.is_some_and(|t| now >= t) {
            s.next_hand = None;
            if let Some(ev) = s.table.start_hand() {
                stats.hands += 1;
                s.raise_to = 2 * BIG_BLIND;
                log_events(s, &ev, now);
            }
        }
        return;
    }
    // A seat that is not the player (or the player playing by itself) chooses after a
    // moment.
    let Some(seat) = s.table.to_act else { return };
    if seat == PLAYER && !s.auto {
        s.clamp_raise();
        return;
    }
    let Some(at) = s.next_choice else {
        let (lo, hi) = THINK_SECONDS;
        s.next_choice = Some(now + lo + (hi - lo) * rules::unit(&mut s.table.rng));
        return;
    };
    if now < at {
        return;
    }
    s.next_choice = None;
    let action = s.table.decide(seat);
    match s.table.act(seat, action) {
        Ok(ev) => log_events(s, &ev, now),
        Err(e) => warn!("poker: {}'s {action:?} refused: {e}", s.table.seats[seat].name),
    }
}

/// The caps the player gains (or, below zero, loses) leaving with `stack` after buying in
/// with `buy_in`.
pub fn settlement(stack: u32, buy_in: u32) -> i64 {
    stack as i64 - buy_in as i64
}

/// Add or take the difference.
fn settle(sim: &mut SimState, delta: i64) -> Result<(), String> {
    match delta {
        0 => Ok(()),
        d if d > 0 => sim.world.add_item(FormId::PLAYER_REF, CAPS, d as i32).map(|_| ()).map_err(|e| e.to_string()),
        d => sim.world.remove_item(FormId::PLAYER_REF, CAPS, (-d) as i32).map(|_| ()).map_err(|e| e.to_string()),
    }
}

fn close(mut screen: ResMut<PokerScreen>, mut sim: ResMut<SimState>, mut stats: ResMut<PokerStats>) {
    let Some(s) = screen.0.take() else { return };
    let stack = s.stack_on_leaving();
    let delta = settlement(stack, s.buy_in);
    let before = caps(&mut sim);
    match settle(&mut sim, delta) {
        Ok(()) => info!("poker: settled: left with {stack} of a {} buy-in, {delta:+} caps ({before} -> {}); {} hands, {} won", s.buy_in, caps(&mut sim), s.hands, s.won),
        Err(e) => warn!("poker: could not settle {delta:+} caps: {e}"),
    }
    stats.won += s.won;
    stats.settled += delta;
}

fn report(mut exit: MessageReader<AppExit>, stats: Res<PokerStats>) {
    if exit.read().next().is_none() || stats.asked == 0 {
        return;
    }
    let s = &*stats;
    info!("poker summary: {} asked, {} opened ({} refused); {} hands dealt, {} won by the player; caps {:+}", s.asked, s.opened, s.refused, s.hands, s.won, s.settled);
}

/// [design, #2403] Synthetic logical/ineffective states for detector regressions.
#[cfg(feature = "report-test-support")]
#[doc(hidden)]
pub fn report_progress_fixture(step: usize) -> PokerScreen {
    let mut table = Table::new(vec![Seat::new("Synthetic seat", 100, Style { tightness: 0.5, aggression: 0.5, bluff: 0.0 })], 1, 2, step as u64);
    if step >= 2 {
        table.seats[0].stack = 90;
    }
    PokerScreen(Some(Session {
        table,
        casino: Casino::Plain,
        buy_in: 100,
        status: (step >= 3).then(|| "Synthetic result".into()),
        raise_to: if step >= 1 { 4 } else { 2 },
        opened: step as f32,
        dealt_at: step as f32,
        street_at: [step as f32; 3],
        auto: false,
        ended: None,
        leaving: None,
        next_choice: Some(step as f32),
        next_hand: None,
        cues: vec![],
        hands: 0,
        won: 0,
        wins: vec![],
        table_ref: None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_aid_reads_its_steps() {
        let s = parse_aid("1:seed:42,1:caps:500,2:open:g,3:call,4:raise:60,5:raise,6:fold,7:allin,8:auto,9:next,10:exit,11:open").unwrap();
        assert_eq!(s[0], (1.0, AidStep::Seed(42)));
        assert_eq!(s[1], (1.0, AidStep::Caps(500)));
        assert_eq!(s[2], (2.0, AidStep::Open(Casino::Gomorrah)));
        assert_eq!(s[3], (3.0, AidStep::Input(PokerInput::CheckCall)));
        assert_eq!(s[4], (4.0, AidStep::Input(PokerInput::Raise(Some(60)))));
        assert_eq!(s[5], (5.0, AidStep::Input(PokerInput::Raise(None))));
        assert_eq!(s[10], (10.0, AidStep::Input(PokerInput::Exit)));
        assert_eq!(s[11], (11.0, AidStep::Open(Casino::Tops)));
        assert!(parse_aid("1:open:x").is_err());
        assert!(parse_aid("1:bluff").is_err());
    }

    #[test]
    fn casinos_name_the_games_textures() {
        let c = rules::Card::parse("Ah").unwrap();
        assert_eq!(Casino::Tops.card_texture(c), "textures\\terminals\\nv_playingcards\\deckt\\cardt_h01.dds");
        assert_eq!(Casino::UltraLuxe.back_texture(), "textures\\terminals\\nv_playingcards\\decku\\cardu_back.dds");
        assert!(Casino::UltraLuxe.felt().ends_with("table02u.dds"));
        assert!(Casino::Lucky38.logo().is_none());
    }

    #[test]
    fn leaving_settles_the_difference() {
        assert_eq!(settlement(260, 200), 60);
        assert_eq!(settlement(0, 200), -200);
        assert_eq!(settlement(200, 200), 0);
    }
}
