//! Blackjack at the casinos' tables (docs/spec-notes/blackjack.md): a table's script calls
//! ShowBlackJackMenuParams (`fnv_game::blackjack::BlackjackRequest`) and the menu opens
//! when the player carries the casino's chips and is not banned there; else a corner
//! message says why. The rules are `fnv_game::blackjack`; the table, its cards and the bet's
//! chips are the game's models, drawn by the engine (nvforever `blackjack_view.rs`) into
//! [`BlackjackView`], which `screen` shows behind the menu's text.
//!
//! - The bet is taken from the chips as the cards are dealt (and again for a double or a
//!   split); what a hand wins comes back as it is settled. The round's net goes to the
//!   player's earnings at the casino (`fnv_game::blackjack::add_earnings`), which set the
//!   winnings level the casino's scripts read; at the max winnings the player is banned
//!   and the menu closes after the round.
//! - The cards come out one after another as the models' sequences deal them
//!   ([`DEAL_GAP`]); the buttons wait until they are down.
//! - The first time, the help message `HelpBlackjack` comes up over the menu, as
//!   lockpicking's (`crate::lockpick::help`).
//! - It is a `PlayState::Menu`: the world and the game clock stand still.
//!
//! Keys [documented: menus\black_jack_menu.xml `_PCButton_*`]: F Hit; W Double Down (Deal
//! between rounds); E Split (Increase Bet); Q Switch Hands (Decrease Bet); S Surrender
//! (Max Bet); R Stay (Exit Game). Esc leaves between rounds [design]. The buttons can be
//! clicked.
//!
//! Test aid `NVF_PLAY_TEST_BLACKJACK=<seconds>:<step>[,…]`: `seed:<n>` (before `open`),
//! `chips:<casino hex>:<n>` (the player carries that many of the casino's chips),
//! `earnings:<casino hex>:<n>`, `open:<casino hex>[:<min>:<max>]` (as the tables' script
//! does), `stack:<card>-<card>-…` (the next cards out of the shoe, e.g. `Ah-9c-Kd-7s`),
//! `bet:<n>`, `deal`, `hit`, `stay`, `double`, `split`, `switch`, `surrender`, `up`,
//! `down`, `max`, `help` (closes the help box), `auto` (rounds play by themselves: deal,
//! hit under 17, stay), `exit`. At exit, when the menu was asked for: "blackjack
//! summary: …".

pub mod screen;

use crate::lockpick::Help;
use crate::pause::{AidTime, Paused};
use crate::script::{with_world, ScriptState};
use crate::sim::SimState;
use crate::flourish::{Flourish, Highlight};
use crate::ui_sounds::{UiEvent, UiMenu, UiSound};
use crate::PlayState;
use crate::PlaySet;
use bevy::prelude::*;
use crate::ui_pointer::PointerState;
use fnv_game::blackjack::{self as rules, Action, BlackjackRequest, CasinoDef, Card, Closed, Event, Outcome, Phase, Refusal, Rules, Seat, Table};
use fnv_sim::FormId;
use fnv_util::words::{tr, trf};
use rand_chacha::ChaCha8Rng;
use rand_core::SeedableRng;

/// The menu's code to scripts (blackjack.md §1).
pub const MENU_BLACKJACK: u16 = rules::MENU_CODE;
/// Seconds between one card and the next leaving the shoe [design: the models' deal
/// sequences take 0.63 s; the next starts before one lands].
pub const DEAL_GAP: f32 = 0.45;
/// A deal sequence's length, and the discard's [observed: the models' text keys].
pub const CARD_SECONDS: f32 = 0.633;
pub const DISCARD_SECONDS: f32 = 1.3;
/// The dealer's hole card turning over in place (`Dealer_Reveal`, played in this time by
/// the view), then the pause before the dealer draws or the result shows [design: a 0.25 to 0.4 s flip, then 0.5 to 0.8 s].
pub const REVEAL_SECONDS: f32 = 0.35;
pub const REVEAL_PAUSE: f32 = 0.65;
/// Seconds the shuffle takes before the deal [design].
pub const SHUFFLE_SECONDS: f32 = 1.0;
/// Seconds the table stays after the round that banned the player [design].
pub const END_SECONDS: f32 = 3.0;
/// Seconds the menu takes to fade as it closes.
pub const LEAVE_SECONDS: f32 = 0.5;

/// Ask for the menu (the script function, the test aid).
#[derive(Message, Clone, Copy, Debug)]
pub struct OpenBlackjack(pub BlackjackRequest);

/// The table's picture, drawn by the engine while the menu is open.
#[derive(Resource, Clone, Debug)]
pub struct BlackjackView {
    /// The picture of the table drawn alone; `None` when the game's own camera shows the
    /// table where it stands in the world (the room is the backdrop).
    pub image: Option<Handle<Image>>,
}

#[derive(Resource, Default)]
pub struct BlackjackScreen(pub Option<Session>);

/// A card as the table shows it: whose, its place in that hand, and when its deal
/// sequence started (aid seconds). Hand 1 is the split hand (the `Hand2` model).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shown {
    pub seat: Seat,
    pub index: usize,
    pub card: Card,
    pub since: f32,
}

pub struct Session {
    pub casino: FormId,
    pub def: CasinoDef,
    pub chip: FormId,
    /// The chip item's name ("Gomorrah Chip").
    pub chip_name: String,
    pub table: Table,
    pub bet: u32,
    /// The casino's chips the player carries now (the bets on the table taken out).
    pub chips: u32,
    pub earnings: i64,
    pub max_winnings: u32,
    pub status: Option<String>,
    pub help: Option<Help>,
    pub opened: f32,
    /// The cards on the table, in the order they were dealt.
    pub shown: Vec<Shown>,
    /// When the hole card was turned over.
    pub reveal: Option<f32>,
    /// When the last round's cards started to be swept away (they are gone after
    /// [`DISCARD_SECONDS`]).
    pub discard: Option<(f32, Vec<Shown>)>,
    /// When the settled round's cards are swept off (aid seconds).
    pub collect_at: Option<f32>,
    /// The buttons wait until then (aid seconds): the cards are still coming.
    pub busy_until: f32,
    /// The round's result line, shown from then.
    pending: Option<(f32, String)>,
    /// Sounds to play when their time comes.
    sounds: Vec<(f32, UiEvent)>,
    /// Hands won to show as highlights when their time comes: whether it was a blackjack.
    wins: Vec<(f32, bool)>,
    /// The round took the earnings to the max: the menu closes then.
    pub banned_at: Option<f32>,
    pub leaving: Option<(f32, &'static str)>,
    /// The test aid plays by itself.
    auto: bool,
    /// Asked to leave during a round: the menu leaves once it is over.
    leave_after: bool,
    /// What the last round paid back (from when, how many chips): on the table at the
    /// seat until the next bet is set.
    pub paid: Option<(f32, u32)>,
    /// The table in the world the menu is at (`crate::roulette::world_table`): the menu's
    /// table and camera stand where it does, the room around it the backdrop (nvforever
    /// `blackjack_view.rs`).
    pub table_ref: Option<FormId>,
}

/// Seconds the chips a round paid stay before the buttons come back [design].
pub const PAID_SECONDS: f32 = 1.2;
/// Seconds a settled round's cards stay to be read before they are swept off; the chips
/// stay until the next bet [design].
pub const COLLECT_SECONDS: f32 = 1.5;

/// The cards on the table swept off from `now` (none when there are none).
fn sweep(shown: &mut Vec<Shown>, discard: &mut Option<(f32, Vec<Shown>)>, now: f32) -> bool {
    if shown.is_empty() {
        return false;
    }
    *discard = Some((now, std::mem::take(shown)));
    true
}

/// When a new deal's first card may leave the shoe: after the cards being swept off are
/// gone (they are the same cards of the models).
fn after_discard(discard: &Option<(f32, Vec<Shown>)>, t: f32) -> f32 {
    discard.as_ref().map_or(t, |(d, _)| t.max(d + DISCARD_SECONDS))
}

impl Session {
    /// The menu's buttons wait for the cards.
    pub fn busy(&self, now: f32) -> bool {
        now < self.busy_until || self.help.is_some() || self.leaving.is_some() || self.banned_at.is_some()
    }

    pub fn betting(&self) -> bool {
        self.table.phase != Phase::Playing
    }

    /// The chips on the table at this player's seat now: the bet being set, the hands'
    /// stakes, then what the round paid back.
    pub fn chips_shown(&self, now: f32) -> u32 {
        if !self.betting() {
            return self.table.staked();
        }
        match self.paid {
            Some((t, _)) if now < t => self.table.staked(),
            Some((_, n)) => n,
            None => self.bet,
        }
    }

    /// How much of the menu is drawn: 1, down to 0 as it leaves.
    pub fn fade(&self, now: f32) -> f32 {
        self.leaving.map_or(1.0, |(since, _)| (1.0 - (now - since) / LEAVE_SECONDS).clamp(0.0, 1.0))
    }

    fn begin_leaving(&mut self, now: f32, why: &'static str) {
        if self.leaving.is_none() {
            info!("blackjack: leaving ({why})");
            self.leaving = Some((now, why));
            self.help = None;
        }
    }

    /// The choices of the six buttons now, top to bottom (blackjack.md §5): the label,
    /// the key and the command; `None` for a button not drawn.
    pub fn buttons(&self) -> [Option<(&'static str, String, BlackjackInput)>; 6] {
        let h = |k: KeyCode| crate::pad::hint(&[k], crate::pad::at_menu(fnv_game::blackjack::MENU_CODE));
        if self.betting() {
            [
                None,
                Some((tr("Deal"), h(KeyCode::KeyW), BlackjackInput::Deal)),
                Some((tr("Increase Bet"), h(KeyCode::KeyE), BlackjackInput::Bet { up: true })),
                Some((tr("Decrease Bet"), h(KeyCode::KeyQ), BlackjackInput::Bet { up: false })),
                Some((tr("Max Bet"), h(KeyCode::KeyS), BlackjackInput::MaxBet)),
                Some((tr("Exit Game"), h(KeyCode::KeyR), BlackjackInput::Exit)),
            ]
        } else {
            let l = self.table.legal(self.chips);
            let b = |on: bool, label, key, a| on.then_some((label, key, BlackjackInput::Play(a)));
            [
                b(l.hit, tr("Hit"), h(KeyCode::KeyF), Action::Hit),
                b(l.double, tr("Double Down"), h(KeyCode::KeyW), Action::Double),
                b(l.split, tr("Split"), h(KeyCode::KeyE), Action::Split),
                b(l.switch, tr("Switch Hands"), h(KeyCode::KeyQ), Action::Switch),
                b(l.surrender, tr("Surrender"), h(KeyCode::KeyS), Action::Surrender),
                b(l.stay, tr("Stay"), h(KeyCode::KeyR), Action::Stay),
            ]
        }
    }

    /// The third row's title, with the casino's name as its record has it: "Won at
    /// Gommorah:" (NVForever's own words [design, #4495]).
    pub fn earnings_title(&self) -> String {
        trf("Won at {casino}:", &[("casino", &self.def.name)])
    }
}

/// A command for the open menu.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub enum BlackjackInput {
    Deal,
    Bet { up: bool },
    MaxBet,
    SetBet(u32),
    Play(Action),
    CloseHelp,
    Exit,
}

#[derive(Resource, Default, Debug)]
struct BlackjackStats {
    asked: u32,
    opened: u32,
    refused: u32,
    rounds: u32,
    won: u32,
    lost: u32,
    pushed: u32,
    naturals: u32,
    net: i64,
    banned: u32,
}

/// `NVF_PLAY_TEST_BLACKJACK`.
#[derive(Resource, Default)]
struct BlackjackAid {
    steps: Vec<(f32, AidStep)>,
    seed: Option<u64>,
    /// Cards stacked before the menu opened, for its first deal.
    stack: Vec<Card>,
}

#[derive(Debug, Clone, PartialEq)]
enum AidStep {
    Seed(u64),
    Chips(u32, u32),
    Earnings(u32, i64),
    Open(BlackjackRequest),
    Stack(Vec<Card>),
    Auto,
    Input(BlackjackInput),
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
            let bad = || format!("{what:?}: expected seed:<n>, chips:<casino>:<n>, earnings:<casino>:<n>, open:<casino>[:<min>:<max>], stack:<cards>, bet:<n>, deal, hit, stay, double, split, switch, surrender, up, down, max, help, auto or exit");
            let num = |s: Option<&&str>| s.and_then(|s| s.trim().parse::<i64>().ok()).ok_or_else(bad);
            let step = match op {
                "seed" => AidStep::Seed(num(parts.first())? as u64),
                "chips" => AidStep::Chips(parts.first().and_then(|s| hex(s)).ok_or_else(bad)?, num(parts.get(1))?.max(0) as u32),
                "earnings" => AidStep::Earnings(parts.first().and_then(|s| hex(s)).ok_or_else(bad)?, num(parts.get(1))?),
                "open" => {
                    let casino = FormId(parts.first().and_then(|s| hex(s)).ok_or_else(bad)?);
                    let (min_bet, max_bet) = if parts.len() >= 3 { (num(parts.get(1))? as u32, num(parts.get(2))? as u32) } else { (1, 200) };
                    AidStep::Open(BlackjackRequest { casino, min_bet, max_bet, max_winnings: 0, table: None })
                }
                "stack" => AidStep::Stack(arg.split('-').map(|c| Card::parse(c.trim()).ok_or_else(bad)).collect::<Result<_, _>>()?),
                "bet" => AidStep::Input(BlackjackInput::SetBet(num(parts.first())?.max(0) as u32)),
                "deal" => AidStep::Input(BlackjackInput::Deal),
                "hit" => AidStep::Input(BlackjackInput::Play(Action::Hit)),
                "stay" => AidStep::Input(BlackjackInput::Play(Action::Stay)),
                "double" => AidStep::Input(BlackjackInput::Play(Action::Double)),
                "split" => AidStep::Input(BlackjackInput::Play(Action::Split)),
                "switch" => AidStep::Input(BlackjackInput::Play(Action::Switch)),
                "surrender" => AidStep::Input(BlackjackInput::Play(Action::Surrender)),
                "up" => AidStep::Input(BlackjackInput::Bet { up: true }),
                "down" => AidStep::Input(BlackjackInput::Bet { up: false }),
                "max" => AidStep::Input(BlackjackInput::MaxBet),
                "help" => AidStep::Input(BlackjackInput::CloseHelp),
                "auto" => AidStep::Auto,
                "exit" => AidStep::Input(BlackjackInput::Exit),
                _ => return Err(bad()),
            };
            Ok((t, step))
        })
        .collect()
}

pub struct BlackjackPlugin;

impl Plugin for BlackjackPlugin {
    fn build(&self, app: &mut App) {
        let steps = match std::env::var("NVF_PLAY_TEST_BLACKJACK").map(|v| parse_aid(&v)) {
            Ok(Ok(a)) => a,
            Ok(Err(e)) => {
                warn!("NVF_PLAY_TEST_BLACKJACK: {e}");
                Vec::new()
            }
            Err(_) => Vec::new(),
        };
        app.init_resource::<BlackjackScreen>()
            .init_resource::<BlackjackStats>()
            .insert_resource(BlackjackAid { steps, ..default() })
            .add_message::<OpenBlackjack>()
            .add_message::<BlackjackInput>()
            .add_systems(
                Update,
                (run_aid, scripts_open, open, (keys, buttons, auto_play, apply, step).chain().run_if(in_state(PlayState::Menu)), screen::draw, screen::fade)
                    .chain()
                    .after(crate::menu::open_menus)
                    .in_set(PlaySet::Ui),
            )
            .add_systems(OnExit(PlayState::Menu), close)
            .add_systems(Last, report);
    }
}

#[allow(clippy::too_many_arguments)]
fn run_aid(
    time: AidTime,
    mut aid: ResMut<BlackjackAid>,
    mut open: MessageWriter<OpenBlackjack>,
    mut input: MessageWriter<BlackjackInput>,
    mut screen: ResMut<BlackjackScreen>,
    mut sim: ResMut<SimState>,
    mut script: Option<ResMut<ScriptState>>,
) {
    if aid.steps.is_empty() {
        return;
    }
    let Some(now) = time.world_secs() else { return };
    let mut due: Vec<(f32, AidStep)> = aid.steps.iter().filter(|(t, _)| *t <= now).cloned().collect();
    due.sort_by(|a, b| a.0.total_cmp(&b.0));
    aid.steps.retain(|(t, _)| *t > now);
    for (_, step) in due {
        info!("test aid NVF_PLAY_TEST_BLACKJACK: {step:?}");
        match step {
            AidStep::Seed(s) => aid.seed = Some(s),
            AidStep::Chips(casino, n) => {
                let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) else { continue };
                let Some(chip) = fnv_game::casino::chip(&rt.game, FormId(casino)) else {
                    warn!("blackjack: {casino:08X} is not a casino with a chip");
                    continue;
                };
                with_world(rt, &mut sim, |rt| {
                    let have = rt.game.world.item_count(FormId::PLAYER_REF, chip).unwrap_or(0);
                    let n = n as i32;
                    if n > have {
                        let _ = rt.game.world.add_item(FormId::PLAYER_REF, chip, n - have);
                    } else if n < have {
                        let _ = rt.game.world.remove_item(FormId::PLAYER_REF, chip, have - n);
                    }
                });
                info!("blackjack: the player carries {n} of {chip}");
            }
            AidStep::Earnings(casino, n) => {
                let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) else { continue };
                rt.game.player.casino_earnings.insert(FormId(casino), n);
            }
            AidStep::Open(r) => {
                open.write(OpenBlackjack(r));
            }
            AidStep::Stack(cards) => match screen.0.as_mut() {
                Some(s) => s.table.stack(&cards),
                None => aid.stack.extend(cards),
            },
            AidStep::Auto => {
                if let Some(s) = screen.0.as_mut() {
                    s.auto = true;
                }
            }
            AidStep::Input(i) => {
                input.write(i);
            }
        }
    }
}

/// The menus scripts asked for (ShowBlackJackMenuParams), opened while playing.
fn scripts_open(script: Option<ResMut<ScriptState>>, state: Res<State<PlayState>>, mut open: MessageWriter<OpenBlackjack>) {
    let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) else { return };
    if rt.game.blackjack_requests.is_empty() || *state.get() != PlayState::Playing {
        return;
    }
    if let Some(r) = rt.game.blackjack_requests.pop() {
        open.write(OpenBlackjack(r));
    }
    rt.game.blackjack_requests.clear();
}

/// A seed: different every time unless the test aid gives one.
fn seed(casino: FormId, now: f32) -> u64 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    t ^ ((casino.0 as u64) << 24) ^ now.to_bits() as u64
}

/// The line shown when the menu does not open [hypothesis: the words are NVForever's],
/// in the chosen language (`lang/*.toml`, #3146).
pub fn refusal_line(r: Closed, casino: &str, chip: &str) -> String {
    match r {
        Closed::NoCasino => tr("This table is closed.").into(),
        Closed::NoChips => trf("You need {chip}s to play here.", &[("chip", &chip)]),
        Closed::Banned => trf("You have been banned from gambling at {casino}.", &[("casino", &casino)]),
    }
}

#[allow(clippy::too_many_arguments)]
fn open(
    mut requests: MessageReader<OpenBlackjack>,
    state: Res<State<PlayState>>,
    mut next: ResMut<NextState<PlayState>>,
    mut screen: ResMut<BlackjackScreen>,
    paused: Res<Paused>,
    mut sim: ResMut<SimState>,
    mut script: Option<ResMut<ScriptState>>,
    mut aid: ResMut<BlackjackAid>,
    time: AidTime,
    (mut stats, mut corner, mut sounds, bindings): (ResMut<BlackjackStats>, Option<ResMut<crate::corner_messages::CornerMessages>>, MessageWriter<UiSound>, Res<crate::bindings::Bindings>),
    play: Option<Res<crate::PlayWorld>>,
) {
    let Some(OpenBlackjack(r)) = requests.read().last().copied() else { return };
    stats.asked += 1;
    if *state.get() != PlayState::Playing || screen.0.is_some() || paused.0 {
        info!("blackjack: request for {} ignored, the player is not in the world", r.casino);
        return;
    }
    let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) else {
        warn!("blackjack: scripts are not running; the table needs the game's rules");
        return;
    };
    let def = rt.game.data.casinos.get(&r.casino).cloned();
    let chip = fnv_game::casino::chip(&rt.game, r.casino);
    let chip_name = chip.and_then(|c| sim.name(c).map(str::to_string)).unwrap_or_else(|| tr("chip").into());
    let max_winnings = if r.max_winnings > 0 { r.max_winnings } else { def.as_ref().map_or(0, |d| d.max_winnings) };
    let now = time.elapsed_secs();
    let (opening, chips) = with_world(rt, &mut sim, |rt| {
        let chips = chip.map_or(0, |c| rt.game.world.item_count(FormId::PLAYER_REF, c).unwrap_or(0).max(0) as u32);
        (rules::opening(&rt.game, r.casino, chips, max_winnings), chips)
    });
    let (Some(def), Some(chip)) = (def, chip) else {
        warn!("blackjack: {} is not a casino with a chip", r.casino);
        return;
    };
    if let Err(why) = opening {
        stats.refused += 1;
        let line = refusal_line(why, &def.name, &chip_name);
        info!("blackjack: {} refused ({why:?}, {chips} chips): on screen: {line:?}", def.name);
        if let Some(c) = corner.as_deref_mut() {
            c.push(line.into());
        }
        sounds.write(UiSound { menu: UiMenu::Blackjack, event: UiEvent::Refused });
        return;
    }
    stats.opened += 1;
    let rng = ChaCha8Rng::seed_from_u64(aid.seed.unwrap_or_else(|| seed(r.casino, now)));
    let table_rules = Rules { min_bet: r.min_bet, max_bet: r.max_bet, payout: def.payout, stay_soft17: def.stay_soft17 };
    let mut table = Table::new(table_rules, def.decks, def.shuffle_share, rng);
    table.stack(&std::mem::take(&mut aid.stack));
    let earnings = rt.game.player.casino_earnings.get(&r.casino).copied().unwrap_or(0);
    info!(
        "blackjack: open at {} ({}): bets {} to {}, {} decks shuffled at {:.0}%, blackjack pays {}, dealer {} soft 17; {chips} {chip_name}s, earnings {earnings} of {max_winnings}",
        def.name,
        r.casino,
        r.min_bet,
        r.max_bet,
        def.decks,
        def.shuffle_share * 100.0,
        def.payout,
        if def.stay_soft17 { "stays on" } else { "hits" }
    );
    rt.game.open_menu(MENU_BLACKJACK);
    let help = rules::first_help(&mut rt.game).map(|(title, text)| Help::new(&rt.game, title, &text, &bindings));
    if let Some(h) = &help {
        info!("blackjack: help message {} {:?} shown (first use)", rules::HELP_MESSAGE, h.title);
    }
    let bet = table.clamp_bet(r.min_bet, chips);
    let table_ref = crate::roulette::world_table(&sim, play.as_deref(), r.table, def.table_model.as_deref(), "blackjack");
    match table_ref {
        Some(t) => info!("blackjack: at the table {t} in the world (asked by {:?})", r.table),
        None => info!("blackjack: no blackjack table near in the world; the menu draws its table alone"),
    }
    screen.0 = Some(Session {
        casino: r.casino,
        def,
        chip,
        chip_name,
        table,
        bet,
        chips,
        earnings,
        max_winnings,
        status: None,
        help,
        opened: now,
        shown: Vec::new(),
        reveal: None,
        discard: None,
        collect_at: None,
        busy_until: now,
        pending: None,
        sounds: Vec::new(),
        wins: Vec::new(),
        banned_at: None,
        leaving: None,
        auto: false,
        leave_after: false,
        paid: None,
        table_ref,
    });
    sounds.write(UiSound { menu: UiMenu::Blackjack, event: UiEvent::Opened });
    next.set(PlayState::Menu);
}

/// The keys [documented: the menu file's `_PCButton_*`].
fn keys(keys: crate::pad::ActionButtons<KeyCode>, screen: Res<BlackjackScreen>, time: AidTime, mut out: MessageWriter<BlackjackInput>) {
    let keys = keys.snapshot();
    let Some(s) = screen.0.as_ref() else { return };
    if s.leaving.is_some() {
        return;
    }
    if s.help.is_some() {
        if keys.any_just_pressed([KeyCode::KeyE, KeyCode::Escape, KeyCode::Tab, KeyCode::Enter]) {
            out.write(BlackjackInput::CloseHelp);
        }
        return;
    }
    if s.busy(time.elapsed_secs()) {
        return;
    }
    let pressed = [KeyCode::KeyF, KeyCode::KeyW, KeyCode::KeyE, KeyCode::KeyQ, KeyCode::KeyS, KeyCode::KeyR];
    let buttons = s.buttons();
    for (key, b) in pressed.iter().zip(buttons) {
        if let (true, Some((_, _, cmd))) = (keys.just_pressed(*key), b) {
            out.write(cmd);
        }
    }
    if s.betting() && keys.any_just_pressed([KeyCode::Escape, KeyCode::Tab]) {
        out.write(BlackjackInput::Exit);
    }
}

/// A clicked button: the menu's own, or the help box's Close.
type Clicked<'a> = (&'a PointerState, AnyOf<(&'a screen::BlackjackButton, &'a crate::lockpick::screen::LockButton)>);

fn buttons(screen: Res<BlackjackScreen>, pressed: Query<Clicked, Changed<PointerState>>, mut out: MessageWriter<BlackjackInput>) {
    if screen.0.is_none() {
        return;
    }
    for (i, (b, help)) in &pressed {
        if *i != PointerState::Pressed {
            continue;
        }
        if let Some(b) = b {
            out.write(b.0);
        }
        // The help box's Close (drawn by the lockpicking menu's help).
        if help.is_some_and(|h| h.0 == crate::lockpick::LockpickInput::CloseHelp) {
            out.write(BlackjackInput::CloseHelp);
        }
    }
}

/// The test aid's `auto`: rounds play by themselves (deal, hit under 17, stay) [design].
fn auto_play(screen: Res<BlackjackScreen>, time: AidTime, mut out: MessageWriter<BlackjackInput>) {
    let Some(s) = screen.0.as_ref().filter(|s| s.auto) else { return };
    if s.busy(time.elapsed_secs()) {
        return;
    }
    let cmd = if s.betting() {
        if s.leave_after {
            return;
        }
        if s.chips < s.table.rules.min_bet {
            BlackjackInput::Exit
        } else {
            BlackjackInput::Deal
        }
    } else {
        let h = &s.table.hands[s.table.active];
        BlackjackInput::Play(if h.total().0 < 17 && s.table.legal(s.chips).hit { Action::Hit } else { Action::Stay })
    };
    out.write(cmd);
}

/// The result line of a round [hypothesis: the words are NVForever's].
pub fn result_line(outcomes: &[(Outcome, u32)], dealer: u32, net: i64) -> String {
    let words = |o: Outcome| match o {
        Outcome::Blackjack => tr("Blackjack"),
        Outcome::Win => tr("You win"),
        Outcome::Push => tr("Push"),
        Outcome::Lose => tr("Dealer wins"),
        Outcome::Bust => tr("Bust"),
        Outcome::Surrendered => tr("Surrendered"),
    };
    let parts: Vec<&str> = outcomes.iter().map(|(o, _)| words(*o)).collect();
    let net = match net {
        n if n > 0 => format!("+{n}"),
        n => n.to_string(),
    };
    let dealer = if dealer > 21 { tr("Dealer busts").to_string() } else { trf("Dealer has {dealer}", &[("dealer", &dealer)]) };
    format!("{}. {dealer}. {net}", parts.join(" / "))
}

#[allow(clippy::too_many_arguments)]
fn apply(
    mut input: MessageReader<BlackjackInput>,
    mut screen: ResMut<BlackjackScreen>,
    mut sim: ResMut<SimState>,
    mut script: Option<ResMut<ScriptState>>,
    time: AidTime,
    mut stats: ResMut<BlackjackStats>,
    mut sounds: MessageWriter<UiSound>,
) {
    let now = time.elapsed_secs();
    for &cmd in input.read() {
        let Some(s) = screen.0.as_mut() else { return };
        match cmd {
            BlackjackInput::CloseHelp => {
                if s.help.take().is_some() {
                    info!("blackjack: help message closed");
                }
                continue;
            }
            _ if s.leaving.is_some() || s.help.is_some() || s.banned_at.is_some() => continue,
            BlackjackInput::Exit => {
                if s.betting() && now >= s.busy_until {
                    s.begin_leaving(now, "the player left");
                } else {
                    s.leave_after = true;
                }
                continue;
            }
            _ => {}
        }
        if matches!(cmd, BlackjackInput::Deal | BlackjackInput::Bet { .. } | BlackjackInput::MaxBet | BlackjackInput::SetBet(_)) && s.betting() && now >= s.busy_until {
            s.paid = None;
        }
        let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) else { return };
        match cmd {
            BlackjackInput::Bet { up } if s.betting() => {
                let before = s.bet;
                s.bet = s.table.step_bet(s.bet, up, s.chips);
                if s.bet != before {
                    sounds.write(UiSound { menu: UiMenu::Blackjack, event: UiEvent::Value { up } });
                }
            }
            BlackjackInput::MaxBet if s.betting() => {
                let before = s.bet;
                s.bet = s.table.max_bet(s.chips);
                if s.bet != before {
                    sounds.write(UiSound { menu: UiMenu::Blackjack, event: UiEvent::Value { up: s.bet > before } });
                }
            }
            BlackjackInput::SetBet(b) if s.betting() => s.bet = s.table.clamp_bet(b, s.chips),
            BlackjackInput::Deal if s.betting() => {
                s.bet = s.table.clamp_bet(s.bet, s.chips);
                let bet = s.bet;
                let chips = s.chips;
                match s.table.deal(bet, chips) {
                    Ok(events) => {
                        // Last round's cards are swept off first.
                        let mut t = after_discard(&s.discard, now.max(s.busy_until));
                        s.collect_at = None;
                        if sweep(&mut s.shown, &mut s.discard, now) {
                            s.sounds.push((now, UiEvent::Collected { dealer: false }));
                            s.sounds.push((now + 0.3, UiEvent::Collected { dealer: true }));
                            t = after_discard(&s.discard, t);
                        }
                        s.reveal = None;
                        s.status = None;
                        take_chips(rt, &mut sim, s, bet);
                        rt.game.count_stat(fnv_game::misc_stats::BLACKJACK_GAMES_PLAYED, 1.0);
                        stats.rounds += 1;
                        info!("blackjack: round {}: bet {bet} of {chips} chips", s.table.rounds);
                        schedule(s, events, t, &mut stats, rt, &mut sim);
                    }
                    Err(Refusal::Chips) => s.status = Some(trf("You don't have enough {chip}s.", &[("chip", &s.chip_name)])),
                    Err(e) => info!("blackjack: deal refused ({e:?})"),
                }
            }
            BlackjackInput::Play(a) if !s.betting() => {
                let free = s.chips;
                let bet = s.table.hands.get(s.table.active).map_or(0, |h| h.bet);
                match s.table.act(a, free) {
                    Ok(events) => {
                        if matches!(a, Action::Double | Action::Split) {
                            take_chips(rt, &mut sim, s, bet);
                        }
                        let hand = s.table.active;
                        info!("blackjack: {} (hand {})", a.name(), hand + 1);
                        let t = now.max(s.busy_until);
                        schedule(s, events, t, &mut stats, rt, &mut sim);
                    }
                    Err(e) => info!("blackjack: {} refused ({e:?})", a.name()),
                }
            }
            _ => {}
        }
    }
}

/// Chips from the player to the table.
fn take_chips(rt: &mut crate::script::Runtime, sim: &mut SimState, s: &mut Session, n: u32) {
    let chip = s.chip;
    with_world(rt, sim, |rt| {
        let _ = rt.game.world.remove_item(FormId::PLAYER_REF, chip, n as i32);
    });
    s.chips = s.chips.saturating_sub(n);
}

/// A round's events, put on the table one after another from `t`; the chips won paid and
/// the earnings counted at once (the screen shows the result when the cards are down).
fn schedule(s: &mut Session, events: Vec<Event>, mut t: f32, stats: &mut BlackjackStats, rt: &mut crate::script::Runtime, sim: &mut SimState) {
    for e in events {
        match e {
            Event::Shuffled => {
                info!("blackjack: the shoe is shuffled ({} cards)", s.table.shoe.len());
                s.sounds.push((t, UiEvent::Shuffled));
                t += SHUFFLE_SECONDS;
            }
            Event::Card { seat, index, card, up } => {
                info!("blackjack: {} {} card {}: {}", seat_name(seat), ordinal(index), if up { "up" } else { "down" }, card.short());
                s.shown.push(Shown { seat, index, card, since: t });
                s.sounds.push((t, UiEvent::CardDealt));
                t += DEAL_GAP;
            }
            Event::Split => {
                info!("blackjack: split");
                if let Some(c) = s.shown.iter_mut().find(|c| c.seat == Seat::Hand(0) && c.index == 1) {
                    c.seat = Seat::Hand(1);
                    c.index = 0;
                    c.since = t;
                }
                s.sounds.push((t, UiEvent::CardDealt));
                t += DEAL_GAP;
            }
            Event::Reveal(card) => {
                info!("blackjack: the dealer turns over {}", card.short());
                s.reveal = Some(t);
                s.sounds.push((t, UiEvent::CardFlipped));
                t += REVEAL_SECONDS + REVEAL_PAUSE;
            }
            Event::Active(h) => info!("blackjack: playing hand {}", h + 1),
            Event::HandOver { hand, outcome } => info!("blackjack: hand {} {}", hand + 1, outcome.name()),
            Event::Settled { outcomes, dealer, net } => {
                if let Some(natural) = won(&outcomes) {
                    s.wins.push((t, natural));
                }
                let paid: u32 = outcomes.iter().map(|(_, p)| p).sum();
                let chip = s.chip;
                if paid > 0 {
                    with_world(rt, sim, |rt| {
                        let _ = rt.game.world.add_item(FormId::PLAYER_REF, chip, paid as i32);
                    });
                    s.chips += paid;
                }
                s.earnings = rules::add_earnings(&mut rt.game, s.casino, net, s.max_winnings);
                let level = rules::winnings_level(s.earnings, s.max_winnings);
                let line = result_line(&outcomes, dealer, net);
                let names: Vec<&str> = outcomes.iter().map(|(o, _)| o.name()).collect();
                info!("blackjack: round {} over: {} (dealer {dealer}); paid {paid}, net {net}; {} chips, earnings {} (level {level}); on screen: {line:?}", s.table.rounds, names.join(", "), s.chips, s.earnings);
                stats.net += net;
                match net.cmp(&0) {
                    std::cmp::Ordering::Greater => stats.won += 1,
                    std::cmp::Ordering::Less => stats.lost += 1,
                    std::cmp::Ordering::Equal => stats.pushed += 1,
                }
                stats.naturals += outcomes.iter().filter(|(o, _)| *o == Outcome::Blackjack).count() as u32;
                s.pending = Some((t, line));
                s.paid = Some((t, paid));
                s.collect_at = Some(t + COLLECT_SECONDS);
                t += PAID_SECONDS;
                if net != 0 {
                    s.sounds.push((t, UiEvent::RoundOver { won: net > 0 }));
                }
                if level >= 4 {
                    stats.banned += 1;
                    info!("blackjack: the earnings reached {} at {}: banned", s.max_winnings, s.def.name);
                    s.banned_at = Some(t);
                }
            }
        }
    }
    s.busy_until = s.busy_until.max(t + CARD_SECONDS - DEAL_GAP);
}

fn seat_name(seat: Seat) -> &'static str {
    match seat {
        Seat::Hand(0) => "player",
        Seat::Hand(_) => "player's split hand",
        Seat::Dealer => "dealer",
    }
}

/// A round won (any hand a win or a blackjack): whether one was a blackjack.
fn won(outcomes: &[(Outcome, u32)]) -> Option<bool> {
    outcomes.iter().any(|(o, _)| matches!(o, Outcome::Win | Outcome::Blackjack)).then(|| outcomes.iter().any(|(o, _)| *o == Outcome::Blackjack))
}

/// The highlight of a hand won.
fn win_flourish(natural: bool) -> Flourish {
    let text = if natural { tr("BLACKJACK") } else { tr("YOU WIN THE HAND") };
    Flourish { kind: Highlight::HandWon, text: text.into() }
}

fn ordinal(i: usize) -> String {
    format!("#{}", i + 1)
}

/// Timed sounds and lines; the end after a ban; leaving.
fn step(time: AidTime, mut screen: ResMut<BlackjackScreen>, mut next: ResMut<NextState<PlayState>>, mut sounds: MessageWriter<UiSound>, mut flourish: MessageWriter<Flourish>) {
    let Some(s) = screen.0.as_mut() else { return };
    let now = time.elapsed_secs();
    // Several due at once: one banner (they would lie over each other).
    let mut due = None;
    s.wins.retain(|&(t, natural)| {
        if t <= now {
            due = Some(due.unwrap_or(false) || natural);
            false
        } else {
            true
        }
    });
    if let Some(natural) = due {
        flourish.write(win_flourish(natural));
    }
    s.sounds.retain(|(t, e)| {
        if *t <= now {
            sounds.write(UiSound { menu: UiMenu::Blackjack, event: *e });
            false
        } else {
            true
        }
    });
    if s.pending.as_ref().is_some_and(|(t, _)| *t <= now) {
        s.status = s.pending.take().map(|(_, l)| l);
    }
    if s.discard.as_ref().is_some_and(|(t, _)| now - t >= DISCARD_SECONDS) {
        s.discard = None;
    }
    // A settled round's cards are swept off once read (the chips stay until the next bet).
    if s.collect_at.is_some_and(|t| t <= now) {
        s.collect_at = None;
        if sweep(&mut s.shown, &mut s.discard, now) {
            s.reveal = None;
            s.sounds.push((now, UiEvent::Collected { dealer: false }));
            s.sounds.push((now + 0.3, UiEvent::Collected { dealer: true }));
        }
    }
    if s.leave_after && s.betting() && now >= s.busy_until {
        s.begin_leaving(now, "the player left after the round");
    }
    if let Some(at) = s.banned_at {
        if now - at >= END_SECONDS {
            s.begin_leaving(now, "banned from the casino");
        }
    }
    if let Some((since, why)) = s.leaving {
        if now - since >= LEAVE_SECONDS {
            info!("blackjack: closed ({why})");
            next.set(PlayState::Playing);
        }
    }
}

fn close(mut screen: ResMut<BlackjackScreen>, script: Option<ResMut<ScriptState>>, mut sounds: MessageWriter<UiSound>, mut corner: Option<ResMut<crate::corner_messages::CornerMessages>>) {
    let Some(s) = screen.0.take() else { return };
    sounds.write(UiSound { menu: UiMenu::Blackjack, event: UiEvent::Closed });
    if s.banned_at.is_some() {
        if let Some(c) = corner.as_deref_mut() {
            c.push(refusal_line(Closed::Banned, &s.def.name, &s.chip_name).into());
        }
    }
    info!("blackjack: closed at {}: {} rounds, {} chips, earnings {}", s.def.name, s.table.rounds, s.chips, s.earnings);
    if let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) {
        if rt.game.menu == Some(MENU_BLACKJACK) {
            rt.game.close_menu();
        }
    }
}

fn report(mut exit: MessageReader<AppExit>, stats: Res<BlackjackStats>) {
    if exit.read().next().is_none() || stats.asked == 0 {
        return;
    }
    let s = &*stats;
    info!(
        "blackjack summary: {} asked, {} opened ({} refused); {} rounds: {} won, {} lost, {} pushed, {} naturals; net {}; {} bans",
        s.asked, s.opened, s.refused, s.rounds, s.won, s.lost, s.pushed, s.naturals, s.net, s.banned
    );
}

/// [design, #2403] Synthetic logical/ineffective states for detector regressions.
#[cfg(feature = "report-test-support")]
#[doc(hidden)]
pub fn report_progress_fixture(step: usize) -> BlackjackScreen {
    let mut table = Table::new(Rules { min_bet: 1, max_bet: 100, payout: 1.5, stay_soft17: true }, 1, 0.5, ChaCha8Rng::seed_from_u64(42));
    if step >= 2 {
        table.rounds = 1;
    }
    BlackjackScreen(Some(Session {
        casino: FormId(42),
        def: CasinoDef::default(),
        chip: FormId(43),
        chip_name: String::new(),
        table,
        bet: if step >= 1 { 2 } else { 1 },
        chips: 100,
        earnings: 0,
        max_winnings: 1000,
        status: (step >= 3).then(|| "Synthetic result".into()),
        help: None,
        opened: step as f32,
        shown: vec![],
        reveal: None,
        discard: None,
        busy_until: step as f32,
        pending: None,
        sounds: vec![],
        banned_at: None,
        leaving: None,
        auto: false,
        leave_after: false,
        collect_at: None,
        wins: vec![],
        paid: None,
        table_ref: None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_aid_reads_its_steps() {
        let s = parse_aid("1:seed:7,1:chips:140521:500,2:open:140521,2:open:144C3F:5:50,3:stack:Ah-Kd-9c-7s,4:bet:25,5:deal,6:hit,7:stay,8:auto,9:exit,9:earnings:140521:-20").unwrap();
        assert_eq!(s[0], (1.0, AidStep::Seed(7)));
        assert_eq!(s[1], (1.0, AidStep::Chips(0x140521, 500)));
        assert_eq!(s[2], (2.0, AidStep::Open(BlackjackRequest { casino: FormId(0x140521), min_bet: 1, max_bet: 200, max_winnings: 0, table: None })));
        assert_eq!(s[3], (2.0, AidStep::Open(BlackjackRequest { casino: FormId(0x144C3F), min_bet: 5, max_bet: 50, max_winnings: 0, table: None })));
        assert_eq!(s[4].1, AidStep::Stack(["Ah", "Kd", "9c", "7s"].iter().map(|c| Card::parse(c).unwrap()).collect()));
        assert_eq!(s[5].1, AidStep::Input(BlackjackInput::SetBet(25)));
        assert_eq!(s[7].1, AidStep::Input(BlackjackInput::Play(Action::Hit)));
        assert_eq!(s[9].1, AidStep::Auto);
        assert_eq!(s[11].1, AidStep::Earnings(0x140521, -20));
        assert!(parse_aid("1:fold").is_err());
        assert!(parse_aid("1:stack:Zz").is_err());
    }

    #[test]
    fn result_lines_say_what_happened() {
        assert_eq!(result_line(&[(Outcome::Blackjack, 25)], 16, 15), "Blackjack. Dealer has 16. +15");
        assert_eq!(result_line(&[(Outcome::Win, 20), (Outcome::Bust, 0)], 22, 0), "You win / Bust. Dealer busts. 0");
        assert_eq!(result_line(&[(Outcome::Lose, 0)], 20, -10), "Dealer wins. Dealer has 20. -10");
        // A bust (or a surrender) alone: no dealer's total, it drew nothing.
        assert_eq!(result_line(&[(Outcome::Bust, 0)], 13, -20), "Bust. Dealer has 13. -20");
        assert_eq!(result_line(&[(Outcome::Bust, 0), (Outcome::Surrendered, 5)], 13, -15), "Bust / Surrendered. Dealer has 13. -15");
        assert_eq!(win_flourish(true).text, "BLACKJACK");
        assert_eq!(win_flourish(false).text, "YOU WIN THE HAND");
    }

    fn session() -> Session {
        let rules = Rules { min_bet: 5, max_bet: 200, payout: 1.5, stay_soft17: true };
        Session {
            table_ref: None,
            casino: FormId(1),
            def: CasinoDef::default(),
            chip: FormId(2),
            chip_name: "chip".into(),
            table: Table::new(rules, 6, 0.75, ChaCha8Rng::seed_from_u64(1)),
            bet: 25,
            chips: 500,
            earnings: 0,
            max_winnings: 0,
            status: None,
            help: None,
            opened: 0.0,
            shown: Vec::new(),
            reveal: None,
            discard: None,
            collect_at: None,
            busy_until: 0.0,
            pending: None,
            sounds: Vec::new(),
            wins: Vec::new(),
            banned_at: None,
            leaving: None,
            auto: false,
            leave_after: false,
            paid: None,
        }
    }

    #[test]
    fn a_settled_rounds_cards_are_swept_and_the_next_deal_waits_for_them() {
        let card = |i| Shown { seat: Seat::Hand(0), index: i, card: Card::new(10, 0), since: 0.0 };
        let (mut shown, mut discard) = (vec![card(0), card(1)], None);
        assert!(sweep(&mut shown, &mut discard, 5.0));
        assert!(shown.is_empty() && discard.as_ref().is_some_and(|d| d.0 == 5.0 && d.1.len() == 2));
        assert!(!sweep(&mut shown, &mut discard, 6.0), "nothing left to sweep");
        assert_eq!(discard.as_ref().unwrap().0, 5.0, "the sweep under way is kept");
        // A deal at 5.5 waits for the cards to be gone; one later goes at once.
        assert_eq!(after_discard(&discard, 5.5), 5.0 + DISCARD_SECONDS);
        assert_eq!(after_discard(&discard, 9.0), 9.0);
        assert_eq!(after_discard(&None, 5.5), 5.5);
    }

    #[test]
    fn this_seats_chips_are_the_bet_the_stakes_then_the_payout() {
        let mut s = session();
        assert_eq!(s.chips_shown(0.0), 25);
        s.table.deal(25, 500).unwrap();
        if s.table.phase == Phase::Playing {
            assert_eq!(s.chips_shown(0.0), 25);
        }
        s.table.phase = Phase::Over;
        s.paid = Some((5.0, 50));
        assert_eq!((s.chips_shown(4.0), s.chips_shown(5.0)), (25, 50));
    }

    #[test]
    fn refusals_name_the_chip_and_the_casino() {
        assert_eq!(refusal_line(Closed::NoChips, "Gommorah", "Gomorrah Chip"), "You need Gomorrah Chips to play here.");
        assert!(refusal_line(Closed::Banned, "The Tops", "x").contains("The Tops"));
    }
}
