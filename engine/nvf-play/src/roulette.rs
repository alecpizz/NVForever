//! Roulette (docs/spec-notes/roulette.md): a casino's roulette table, activated by the
//! player, runs `ShowRouletteMenuParams <casino> 1 200 0`; when the player carries the
//! casino's chips and is not banned there (`fnv_game::casino::opening`) the roulette menu
//! opens, else a line says why. In the menu a marker moves over the bet points of the felt
//! (the mouse, or the arrow keys); Place Bet puts the current bet there, Remove Bet takes
//! the bet there back, Increase and Decrease Bet change the current bet; Finish Bet spins
//! the wheel: the chips staked are taken, the ball comes to rest in a pocket, winning bets
//! are paid (`fnv_game::roulette::Table`), the net goes to the player's earnings at the
//! casino and its winnings level (`fnv_game::casino::add_earnings`). At the casino's max
//! winnings the player is banned: the menu closes after the spin.
//!
//! The first time the menu opens the help message `HelpRoulette` comes up over it, as
//! lockpicking's (`crate::lockpick::help`).
//!
//! It is a `PlayState::Menu`: the world and the game clock stand still. To scripts it is
//! menu [`fnv_game::roulette::MENU_CODE`]. What is on the screen besides the table
//! (`screen`) follows `menus\roulette_menu.xml`; the table, the wheel, the chips and the
//! marker are the game's models drawn by the engine (nvforever `roulette_view.rs`) into
//! [`RouletteView`].
//!
//! Keys [documented: the menu file's `_PCButton_W/F/S/Q/E/R`]: W places a bet, F removes
//! one, S finishes betting (spins), Q and E decrease and increase the bet, R leaves (Esc
//! and Tab too [design]); the arrow keys move the marker [design]. The buttons can be
//! clicked; a click on the felt places a bet where the pointer is [design].
//!
//! Test aid `NVF_PLAY_TEST_ROULETTE=<seconds>:<step>[,…]`: `chips:<n>[:<casino hex>]` gives
//! the player that many of the casino's chips (The Tops' when left out), `open[:<casino
//! hex>]` as if the table's script asked (min 1, max 200), `seed:<n>` (before `open`),
//! `bet:<n>` sets the current bet, `at:<point>` moves the marker to a bet point (`D12`,
//! `S08` …, roulette.md §3), `place`, `remove`, `pocket:<n>` makes the next spin's pocket
//! (37 for 00), `spin`, `help` closes the help message, `exit` leaves. At exit, when the
//! menu was asked for: "roulette summary: …".

pub mod screen;

use crate::lockpick::Help;
use crate::pause::{AidTime, Paused};
use crate::script::ScriptState;
use crate::sfx::PlaySound;
use crate::sim::SimState;
use crate::{PlaySet, PlayState};
use bevy::prelude::*;
use crate::ui_pointer::{Button, PointerState};
use fnv_game::casino;
use fnv_util::words::{tr, trf};
use fnv_game::roulette::{self as rules, Refusal, Table};
use fnv_sim::FormId;
use rand_chacha::ChaCha8Rng;
use rand_core::SeedableRng;

/// The menu code scripts see.
pub const MENU_ROULETTE: u16 = rules::MENU_CODE;
/// The Tops' casino record, the test aid's default [observed: FalloutNV.esm CSNO
/// `TheTopsCasinoData`].
pub const THE_TOPS: u32 = 0x0014_4C3F;
/// How long a spin lasts: the table model's `Spin_1`–`Spin_3` sequences (9.83 s each)
/// [observed: the file's sequences]; the engine plays one of them.
pub const SPIN_SECONDS: f32 = 9.833_334;
/// How long the result stays on the screen before the next bets [design].
pub const RESULT_SECONDS: f32 = 3.0;
/// How long a request waits for the player to be back in the world, seconds [design].
pub const REQUEST_WAIT: f32 = 3.0;
/// The first bet point the marker stands on: 17 [design].
pub const FIRST_POINT: &str = "D12";

/// The roulette sounds [observed: FalloutNV.esm SOUN `GAMERouletteSpin` 0x001519E2,
/// `…InvalidBet` 0x001519E3, `…ValidBet` 0x001519E4, `…Win` 0x001519E5, `…DecreaseBet`
/// 0x001519E6, `…IncreaseBet` 0x001519E7; design: which event each is for].
pub mod sounds {
    use fnv_sim::FormId;
    pub const SPIN: FormId = FormId(0x0015_19E2);
    pub const INVALID: FormId = FormId(0x0015_19E3);
    pub const VALID: FormId = FormId(0x0015_19E4);
    pub const WIN: FormId = FormId(0x0015_19E5);
    pub const DECREASE: FormId = FormId(0x0015_19E6);
    pub const INCREASE: FormId = FormId(0x0015_19E7);
}

/// The table's picture, drawn by the engine (nvforever `roulette_view.rs`) while the menu
/// is open; `screen` shows it behind the menu's text.
#[derive(Resource, Clone, Debug)]
pub struct RouletteView {
    /// The picture of the table drawn alone; `None` when the game's own camera shows the
    /// table where it stands in the world (the room is the backdrop).
    pub image: Option<Handle<Image>>,
}

/// The bet point under the mouse pointer, as the engine finds it on the felt (nvforever
/// `roulette_view.rs`), when the pointer last moved over the table; `None` off the felt.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub struct RoulettePointer {
    pub point: Option<String>,
    /// Bumped each time the pointer moves onto another point.
    pub moves: u32,
}

/// The open roulette menu, if any.
#[derive(Resource, Default)]
pub struct RouletteScreen(pub Option<Session>);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    Betting,
    /// The wheel turns: since (aid seconds), the spin sequence (1 to 3), the pocket the
    /// ball comes to rest in.
    Spinning { since: f32, clip: u8, pocket: u8 },
    /// The bets are paid; the result shows until `until` (aid seconds); the spin's start,
    /// sequence and pocket kept, its end held on the screen.
    Paid { until: f32, since: f32, clip: u8, pocket: u8 },
}

pub struct Session {
    pub casino: FormId,
    pub casino_name: String,
    pub chip: FormId,
    /// The roulette table's model (CSNO MOD4), under `meshes\`.
    pub model: Option<String>,
    pub max_winnings: u32,
    pub table: Table,
    /// The current bet (what Place Bet puts down).
    pub bet: u32,
    /// The bet point the marker stands on.
    pub cursor: String,
    pub phase: Phase,
    /// The chips the player carries (less nothing: bets are taken as the wheel spins).
    pub chips: u32,
    pub earnings: i64,
    /// The status lines at the top left (roulette.md §5).
    pub status: Option<[String; 2]>,
    pub help: Option<Help>,
    /// The last spin's pocket, for the wheel's resting place.
    pub last: Option<u8>,
    /// Banned by the last spin: the menu closes when its result has shown.
    pub banned: bool,
    /// The test aid's pocket for the next spin.
    forced: Option<u8>,
    rng: ChaCha8Rng,
    /// The table in the world the menu is at ([`world_table`]): the menu's table and
    /// camera stand where it does, the room around it the backdrop (nvforever
    /// `roulette_view.rs`).
    pub table_ref: Option<FormId>,
}

impl Session {
    /// Whether the current bet can go where the marker is.
    pub fn can_place(&self) -> bool {
        self.phase == Phase::Betting && self.table.can_place(&self.cursor, self.bet, self.chips).is_ok()
    }

    /// The chips not on the table: while betting the bets are not taken yet; once the
    /// wheel turns they have left the inventory.
    pub fn free_chips(&self) -> u32 {
        if self.phase == Phase::Betting {
            self.chips.saturating_sub(self.table.total())
        } else {
            self.chips
        }
    }

    /// The "Bet:" and "Payout:" lines for the bet point under the marker.
    pub fn bet_lines(&self) -> [String; 2] {
        match rules::spot(&self.cursor) {
            Some(s) => [trf("Bet: {bet}", &[("bet", &s.label_with(&|w| tr(w)))]), trf("Payout: {n} to 1", &[("n", &s.payout())])],
            None => [tr("Bet:").into(), tr("Payout:").into()],
        }
    }
}

/// A command for the open menu.
#[derive(Message, Clone, Debug, PartialEq)]
pub enum RouletteInput {
    /// Move the marker to a bet point (the pointer, the test aid).
    At(String),
    /// Move the marker to the next point that way (`dx`, `dy` along the felt).
    Step(f32, f32),
    Place,
    Remove,
    Spin,
    Increase,
    Decrease,
    /// Test aid: the current bet.
    Bet(u32),
    /// Test aid: the next spin's pocket.
    Pocket(u8),
    CloseHelp,
    Exit,
}

#[derive(Resource, Default, Debug)]
struct RouletteStats {
    asked: u32,
    opened: u32,
    refused: u32,
    placed: u32,
    refused_bets: u32,
    spins: u32,
    won: u64,
    lost: u64,
    bans: u32,
}

/// `NVF_PLAY_TEST_ROULETTE`.
#[derive(Resource, Default)]
struct RouletteAid {
    steps: Vec<(f32, AidStep)>,
    seed: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
enum AidStep {
    Open(u32),
    Chips(u32, u32),
    Seed(u64),
    Input(RouletteInput),
}

fn hex(s: &str) -> Option<u32> {
    u32::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok()
}

fn parse_aid(v: &str) -> Result<Vec<(f32, AidStep)>, String> {
    crate::test_aid::timed(v)?
        .into_iter()
        .map(|(t, what)| {
            let (op, arg) = what.split_once(':').unwrap_or((what, ""));
            let num = |name: &str, s: &str| s.trim().parse::<u64>().map_err(|_| format!("{what:?}: expected {op}:<{name}>"));
            let step = match op {
                "open" if arg.is_empty() => AidStep::Open(THE_TOPS),
                "open" => AidStep::Open(hex(arg).ok_or_else(|| format!("{what:?}: expected open:<casino hex>"))?),
                "chips" => {
                    let (n, casino) = arg.split_once(':').unwrap_or((arg, ""));
                    let casino = if casino.is_empty() { THE_TOPS } else { hex(casino).ok_or_else(|| format!("{what:?}: expected chips:<n>:<casino hex>"))? };
                    AidStep::Chips(num("n", n)? as u32, casino)
                }
                "seed" => AidStep::Seed(num("n", arg)?),
                "bet" => AidStep::Input(RouletteInput::Bet(num("n", arg)? as u32)),
                "at" if rules::spot(arg).is_some() => AidStep::Input(RouletteInput::At(arg.trim().to_ascii_uppercase())),
                "at" => return Err(format!("{what:?}: no bet point {arg:?}")),
                "pocket" if num("n", arg)? < rules::POCKETS as u64 => AidStep::Input(RouletteInput::Pocket(num("n", arg)? as u8)),
                "pocket" => return Err(format!("{what:?}: pockets are 0 to 37 (37: 00)")),
                "place" => AidStep::Input(RouletteInput::Place),
                "remove" => AidStep::Input(RouletteInput::Remove),
                "spin" => AidStep::Input(RouletteInput::Spin),
                "help" => AidStep::Input(RouletteInput::CloseHelp),
                "exit" => AidStep::Input(RouletteInput::Exit),
                _ => return Err(format!("{what:?}: expected open, chips:, seed:, bet:, at:, place, remove, pocket:, spin, help or exit")),
            };
            Ok((t, step))
        })
        .collect()
}

pub struct RoulettePlugin;

impl Plugin for RoulettePlugin {
    fn build(&self, app: &mut App) {
        let steps = match std::env::var("NVF_PLAY_TEST_ROULETTE").map(|v| parse_aid(&v)) {
            Ok(Ok(a)) => a,
            Ok(Err(e)) => {
                warn!("NVF_PLAY_TEST_ROULETTE: {e}");
                Vec::new()
            }
            Err(_) => Vec::new(),
        };
        app.init_resource::<RouletteScreen>()
            .init_resource::<RouletteStats>()
            .init_resource::<RoulettePointer>()
            .insert_resource(RouletteAid { steps, seed: None })
            .add_message::<RouletteInput>()
            .add_systems(
                Update,
                (run_aid, open, (keys, buttons, pointer, apply, step).chain().run_if(in_state(PlayState::Menu)), screen::draw)
                    .chain()
                    .after(crate::menu::open_menus)
                    .in_set(PlaySet::Ui),
            )
            .add_systems(OnExit(PlayState::Menu), close)
            .add_systems(Last, report);
    }
}

/// How many of `item` the player carries.
fn carried(sim: &SimState, item: FormId) -> u32 {
    sim.world.peek(FormId::PLAYER_REF).and_then(|s| s.inventory.as_ref()).map_or(0, |inv| inv.count(item)).max(0) as u32
}

fn run_aid(
    time: AidTime,
    mut aid: ResMut<RouletteAid>,
    mut sim: ResMut<SimState>,
    mut script: Option<ResMut<ScriptState>>,
    mut input: MessageWriter<RouletteInput>,
) {
    if aid.steps.is_empty() {
        return;
    }
    let Some(now) = time.world_secs() else { return };
    // Steps wait for the scripts (the casino records are theirs).
    if script.as_ref().and_then(|s| s.rt.as_ref()).is_none() {
        return;
    }
    let mut due: Vec<(f32, AidStep)> = aid.steps.iter().filter(|(t, _)| *t <= now).cloned().collect();
    due.sort_by(|a, b| a.0.total_cmp(&b.0));
    aid.steps.retain(|(t, _)| *t > now);
    for (_, step) in due {
        info!("test aid NVF_PLAY_TEST_ROULETTE: {step:?}");
        match step {
            AidStep::Open(c) => {
                let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) else {
                    warn!("roulette: scripts are not running; the test aid cannot open the table");
                    continue;
                };
                rt.game.roulette_requests.push(rules::RouletteRequest { casino: FormId(c), min_bet: 1, max_bet: 200, max_winnings: 0, table: None });
            }
            AidStep::Chips(n, c) => {
                let chip = script.as_ref().and_then(|s| s.rt.as_ref()).and_then(|rt| fnv_game::casino::chip(&rt.game, FormId(c)));
                match chip {
                    Some(chip) if n > 0 => match sim.world.add_item(FormId::PLAYER_REF, chip, n as i32) {
                        Ok(_) => info!("roulette: the player is given {n} chips {chip} ({} carried)", carried(&sim, chip)),
                        Err(e) => warn!("roulette: chips not given: {e:?}"),
                    },
                    _ => warn!("roulette: no chip for casino {c:08X} ({} casinos known)", script.as_ref().and_then(|s| s.rt.as_ref()).map_or(0, |rt| rt.game.data.casinos.len())),
                }
            }
            AidStep::Seed(s) => aid.seed = Some(s),
            AidStep::Input(i) => {
                input.write(i);
            }
        }
    }
}

/// A seed: different every time unless the test aid fixed it.
fn seed(aid: &RouletteAid, casino: FormId, now: f32) -> u64 {
    aid.seed.unwrap_or_else(|| {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
        t ^ ((casino.0 as u64) << 20) ^ now.to_bits() as u64
    })
}

/// Roulette's stats, corner messages, key bindings and the play world.
type RouletteExtras<'w> = (ResMut<'w, RouletteStats>, Option<ResMut<'w, crate::corner_messages::CornerMessages>>, Res<'w, crate::bindings::Bindings>, Option<Res<'w, crate::PlayWorld>>);

#[allow(clippy::too_many_arguments)]
fn open(
    state: Res<State<PlayState>>,
    mut next: ResMut<NextState<PlayState>>,
    mut screen: ResMut<RouletteScreen>,
    paused: Res<Paused>,
    sim: Res<SimState>,
    script: Option<ResMut<ScriptState>>,
    aid: Res<RouletteAid>,
    time: AidTime,
    (mut stats, mut corner, bindings, play): RouletteExtras,
    mut waiting: Local<Option<(rules::RouletteRequest, f32)>>,
) {
    let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) else { return };
    let now = time.elapsed_secs();
    if let Some(asked) = rt.game.roulette_requests.pop() {
        rt.game.roulette_requests.clear();
        stats.asked += 1;
        *waiting = Some((asked, now));
    }
    let Some((asked, since)) = *waiting else { return };
    // A request made while another menu closes waits for the world a moment [design].
    if *state.get() != PlayState::Playing || screen.0.is_some() || paused.0 {
        if now - since > REQUEST_WAIT {
            info!("roulette: request for {} dropped, the player is not in the world", asked.casino);
            *waiting = None;
        }
        return;
    }
    *waiting = None;
    let Some(def) = rt.game.data.casinos.get(&asked.casino).cloned() else {
        warn!("roulette: {} is not a casino", asked.casino);
        return;
    };
    let max = casino::max_winnings(&rt.game, asked.casino, asked.max_winnings);
    // A script's SetCasinoChip first (`fnv_game::casino::chip`).
    let def_chip = casino::chip(&rt.game, asked.casino);
    let chips = def_chip.map_or(0, |c| carried(&sim, c));
    if let Err(why) = casino::opening(&rt.game, asked.casino, chips, max) {
        stats.refused += 1;
        let line = casino::closed_line(why, &def.name);
        info!("roulette: {} {} refused ({why:?}, {chips} chips): on screen: {line:?}", asked.casino, def.name);
        if let Some(c) = corner.as_deref_mut() {
            c.push(line.into());
        }
        return;
    }
    let Some(chip) = def_chip else { return };
    stats.opened += 1;
    let table = Table::new(asked.min_bet, asked.max_bet);
    let earnings = rt.game.player.casino_earnings.get(&asked.casino).copied().unwrap_or(0);
    rt.game.open_menu(MENU_ROULETTE);
    let help = rules::first_help(&mut rt.game).map(|(title, text)| Help::new(&rt.game, title, &text, &bindings));
    info!(
        "roulette: open {} ({}): bets {} to {}, {chips} chips carried, earnings {earnings} of {max}, table {}{}",
        def.name,
        asked.casino,
        table.min_bet,
        table.max_bet,
        def.roulette_model.as_deref().unwrap_or("(none)"),
        if help.is_some() { "; help message shown (first use)" } else { "" }
    );
    let bet = table.clamp_bet(5);
    let table_ref = world_table(&sim, play.as_deref(), asked.table, def.roulette_model.as_deref(), "roulette");
    match table_ref {
        Some(t) => info!("roulette: at the table {t} in the world (asked by {:?})", asked.table),
        None => info!("roulette: no table of {} near in the world; the menu draws its table alone", def.roulette_model.as_deref().unwrap_or("(no model)")),
    }
    screen.0 = Some(Session {
        casino: asked.casino,
        casino_name: def.name.clone(),
        chip,
        model: def.roulette_model.clone(),
        max_winnings: max,
        table,
        bet,
        cursor: FIRST_POINT.into(),
        phase: Phase::Betting,
        chips,
        earnings,
        status: None,
        help,
        last: None,
        banned: false,
        forced: None,
        rng: ChaCha8Rng::seed_from_u64(seed(&aid, asked.casino, now)),
        table_ref,
    });
    next.set(PlayState::Menu);
}

/// How far from the asking reference (or the player) the table in the world may be, game
/// units [design].
pub const TABLE_REACH: f32 = 400.0;

/// A model path as the plugins and the casino records give it, for comparing.
pub fn same_model(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        let s = s.replace('/', "\\").to_ascii_lowercase();
        s.strip_prefix("meshes\\").map(str::to_string).unwrap_or(s)
    };
    norm(a) == norm(b)
}

/// The table in the world a casino menu is at: the nearest reference of a table model in
/// the player's cell (the casino's menu `model`, or one whose path names the `game`: the
/// world's tables are the clutter models, `clutter\casino\roulettetable01.nif`, not the
/// menu's [observed: FalloutNV.esm ACTI `RouletteTableUltraLuxe`]), within [`TABLE_REACH`]
/// of the reference whose script asked (the activated table), else of the player. None
/// when there is none (the menu then draws its table alone).
pub fn world_table(sim: &SimState, play: Option<&crate::PlayWorld>, asked: Option<FormId>, model: Option<&str>, game: &str) -> Option<FormId> {
    let play = play?;
    let model = model.unwrap_or("");
    let pose = |id: FormId| sim.world.peek(id).map(|r| (r.pos, r.cell)).or_else(|| sim.refs.get(id).map(|r| (r.pos, r.stored_in)));
    let (from, cell) = asked.and_then(pose).filter(|p| p.1.is_some()).or_else(|| pose(FormId::PLAYER_REF))?;
    let near = |p: [f32; 3]| (0..3).map(|i| (p[i] - from[i]).powi(2)).sum::<f32>().sqrt();
    let mut best: Option<(f32, FormId)> = None;
    for id in sim.refs.by_cell.get(&cell?).into_iter().flatten() {
        if !sim.present(id.0) {
            continue;
        }
        let Some((p, _)) = pose(*id) else { continue };
        let d = near(p);
        if d > TABLE_REACH || best.is_some_and(|b| b.0 <= d) {
            continue;
        }
        let Some(base) = sim.world.peek(*id).map(|r| r.base).or_else(|| sim.base_of(id.0)) else { continue };
        if crate::dropped::model_path(&play.0, base, 0).is_some_and(|m| same_model(&m, model) || table_model(&m, game)) {
            best = Some((d, *id));
        }
    }
    best.map(|b| b.1)
}

/// The table references standing where `table` does (within [`TWIN_REACH`], itself first):
/// a casino can place a static table under the activator [observed: FalloutNV.esm
/// `VVBlackjackTableSTATIC` 0x00162079 at Vikki and Vance's 0x000A58D6]; all are hidden
/// while the menu's table stands in for them.
pub fn table_twins(sim: &SimState, play: Option<&crate::PlayWorld>, table: FormId, game: &str) -> Vec<FormId> {
    let mut out = vec![table];
    let Some(play) = play else { return out };
    let pose = |id: FormId| sim.world.peek(id).map(|r| (r.pos, r.cell)).or_else(|| sim.refs.get(id).map(|r| (r.pos, r.stored_in)));
    let Some((at, Some(cell))) = pose(table) else { return out };
    for id in sim.refs.by_cell.get(&cell).into_iter().flatten() {
        let Some((p, _)) = pose(*id).filter(|_| *id != table) else { continue };
        if (0..3).map(|i| (p[i] - at[i]).powi(2)).sum::<f32>().sqrt() > TWIN_REACH {
            continue;
        }
        let Some(base) = sim.world.peek(*id).map(|r| r.base).or_else(|| sim.base_of(id.0)) else { continue };
        if crate::dropped::model_path(&play.0, base, 0).is_some_and(|m| table_model(&m, game)) {
            out.push(*id);
        }
    }
    out
}

/// Whether a world model is a table of `game`: its path names the game and a table; a
/// blackjack table may be a card table [observed: FalloutNV.esm ACTI
/// `BlackjackTableUltraLuxe`: `clutter\casino\CardTable01.NIF`].
pub fn table_model(path: &str, game: &str) -> bool {
    let m = path.to_ascii_lowercase();
    m.contains("table") && (m.contains(game) || (game == "blackjack" && m.contains("cardtable")))
}

/// How near another table reference must stand to be the same table, game units [design].
pub const TWIN_REACH: f32 = 30.0;
/// How near the table an actor stands to be in the menu camera's way, game units: the
/// croupier behind the wheel, players leaning on the rail [design: the table's camera
/// flies low over the felt and the wheel; an Ultra-Luxe croupier's arm filled the left of
/// the view, 2026-11-04].
pub const STAFF_REACH: f32 = 160.0;

/// What stands at a table (not the table, not the player): the actors and the furniture
/// (stools, chairs) the menu's camera, low over the felt, would look through while the
/// table is drawn in the world; the engine keeps the small ones and the actors [design].
pub fn table_staff(sim: &SimState, table: FormId) -> Vec<FormId> {
    let pose = |id: FormId| sim.world.peek(id).map(|r| (r.pos, r.cell)).or_else(|| sim.refs.get(id).map(|r| (r.pos, r.stored_in)));
    let Some((at, Some(cell))) = pose(table) else { return Vec::new() };
    sim.refs
        .by_cell
        .get(&cell)
        .into_iter()
        .flatten()
        .copied()
        .filter(|id| *id != FormId::PLAYER_REF && *id != table)
        .filter(|id| pose(*id).is_some_and(|(p, _)| (0..3).map(|i| (p[i] - at[i]).powi(2)).sum::<f32>().sqrt() <= STAFF_REACH))
        .collect()
}

/// The keys.
fn keys(keys: crate::pad::ActionButtons<KeyCode>, mouse: crate::pad::ActionButtons<MouseButton>, screen: Res<RouletteScreen>, pointer: Res<RoulettePointer>, over_button: Query<&PointerState, With<Button>>, mut out: MessageWriter<RouletteInput>) {
    let keys = keys.snapshot();
    let mouse = mouse.snapshot();
    let Some(s) = screen.0.as_ref() else { return };
    if s.help.is_some() {
        if keys.any_just_pressed([KeyCode::KeyE, KeyCode::Escape, KeyCode::Tab, KeyCode::Enter]) {
            out.write(RouletteInput::CloseHelp);
        }
        return;
    }
    if keys.any_just_pressed([KeyCode::KeyR, KeyCode::Escape, KeyCode::Tab]) {
        out.write(RouletteInput::Exit);
        return;
    }
    let pairs = [
        (KeyCode::KeyW, RouletteInput::Place),
        (KeyCode::KeyF, RouletteInput::Remove),
        (KeyCode::KeyS, RouletteInput::Spin),
        (KeyCode::KeyQ, RouletteInput::Decrease),
        (KeyCode::KeyE, RouletteInput::Increase),
        (KeyCode::ArrowLeft, RouletteInput::Step(-1.0, 0.0)),
        (KeyCode::ArrowRight, RouletteInput::Step(1.0, 0.0)),
        (KeyCode::ArrowUp, RouletteInput::Step(0.0, 1.0)),
        (KeyCode::ArrowDown, RouletteInput::Step(0.0, -1.0)),
    ];
    for (k, i) in pairs {
        if keys.just_pressed(k) {
            out.write(i);
        }
    }
    // A click on the felt places the bet there; the mouse wheel changes the bet [design].
    let on_button = over_button.iter().any(|i| *i != PointerState::None);
    if mouse.just_pressed(MouseButton::Left) && !on_button && pointer.point.is_some() {
        out.write(RouletteInput::Place);
    }
    if mouse.just_pressed(MouseButton::Right) && !on_button && pointer.point.is_some() {
        out.write(RouletteInput::Remove);
    }
}

fn buttons(
    screen: Res<RouletteScreen>,
    pressed: Query<(&PointerState, &screen::RouletteButton), Changed<PointerState>>,
    help_close: Query<(&PointerState, &crate::lockpick::screen::LockButton), Changed<PointerState>>,
    mut out: MessageWriter<RouletteInput>,
) {
    if screen.0.is_none() {
        return;
    }
    for (i, b) in &pressed {
        if *i == PointerState::Pressed {
            out.write(b.0.clone());
        }
    }
    // The help box's Close (lockpicking's box, `crate::lockpick::help`).
    if help_close.iter().any(|(i, _)| *i == PointerState::Pressed) {
        out.write(RouletteInput::CloseHelp);
    }
}

/// The marker follows the pointer over the felt.
fn pointer(pointer: Res<RoulettePointer>, mut seen: Local<u32>, mut out: MessageWriter<RouletteInput>) {
    if pointer.moves == *seen {
        return;
    }
    *seen = pointer.moves;
    if let Some(p) = &pointer.point {
        out.write(RouletteInput::At(p.clone()));
    }
}

fn refusal_line(r: Refusal) -> &'static str {
    // [hypothesis: the original's words are not in the plugins; these are NVForever's],
    // in the chosen language (`lang/*.toml`, #3146)
    match r {
        Refusal::NoSpot => tr("No bet can go there."),
        Refusal::Chips => tr("You do not have enough chips."),
        Refusal::Limit => tr("That is over the table's limit."),
        Refusal::NoBet => tr("There is no bet there."),
        Refusal::NoBets => tr("Place a bet first."),
    }
}

#[allow(clippy::too_many_arguments)]
fn apply(mut input: MessageReader<RouletteInput>, mut screen: ResMut<RouletteScreen>, mut sim: ResMut<SimState>, time: AidTime, mut stats: ResMut<RouletteStats>, mut next: ResMut<NextState<PlayState>>, mut sounds: MessageWriter<PlaySound>) {
    let now = time.elapsed_secs();
    let mut play = |sound| {
        sounds.write(PlaySound { sound, at: None });
    };
    for cmd in input.read() {
        let Some(s) = screen.0.as_mut() else { return };
        match cmd.clone() {
            RouletteInput::CloseHelp => {
                if s.help.take().is_some() {
                    info!("roulette: help message closed");
                }
            }
            _ if s.help.is_some() => {}
            RouletteInput::Exit => {
                if s.phase == Phase::Betting {
                    info!("roulette: closed (the player left; {} chips on the table taken back)", s.table.total());
                    next.set(PlayState::Playing);
                }
            }
            RouletteInput::Pocket(p) => s.forced = Some(p),
            _ if s.phase != Phase::Betting => {}
            RouletteInput::At(p) => {
                if rules::spot(&p).is_some() && s.cursor != p {
                    s.cursor = p;
                }
            }
            RouletteInput::Step(dx, dy) => {
                if let Some(p) = rules::neighbour(&s.cursor, dx, dy) {
                    s.cursor = p;
                }
            }
            RouletteInput::Bet(b) => s.bet = s.table.clamp_bet(b),
            RouletteInput::Increase | RouletteInput::Decrease => {
                let up = *cmd == RouletteInput::Increase;
                let b = s.table.step_bet(s.bet, up);
                if b != s.bet {
                    s.bet = b;
                    play(if up { sounds::INCREASE } else { sounds::DECREASE });
                }
            }
            RouletteInput::Place => match s.table.place(&s.cursor.clone(), s.bet, s.chips) {
                Ok(on) => {
                    stats.placed += 1;
                    info!("roulette: {} on {} ({}): {on} there, {} on the table", s.bet, s.cursor, rules::spot(&s.cursor).map(|x| x.label()).unwrap_or_default(), s.table.total());
                    play(sounds::VALID);
                }
                Err(r) => {
                    stats.refused_bets += 1;
                    info!("roulette: {} on {} refused: {r:?}", s.bet, s.cursor);
                    s.status = Some([refusal_line(r).into(), String::new()]);
                    play(sounds::INVALID);
                }
            },
            RouletteInput::Remove => match s.table.remove(&s.cursor.clone()) {
                Ok(back) => {
                    info!("roulette: the bet on {} taken back ({back}), {} on the table", s.cursor, s.table.total());
                    play(sounds::DECREASE);
                }
                Err(_) => play(sounds::INVALID),
            },
            RouletteInput::Spin => {
                if s.table.bets.is_empty() {
                    s.status = Some([refusal_line(Refusal::NoBets).into(), String::new()]);
                    play(sounds::INVALID);
                    continue;
                }
                // The chips staked leave the inventory as the wheel turns.
                let staked = s.table.total();
                if let Err(e) = sim.world.remove_item(FormId::PLAYER_REF, s.chip, staked as i32) {
                    warn!("roulette: the stake of {staked} chips not taken: {e:?}");
                    continue;
                }
                let pocket = s.forced.take().unwrap_or_else(|| rules::spin(&mut s.rng));
                let clip = (rand_core::RngCore::next_u32(&mut s.rng) % 3 + 1) as u8;
                s.chips = s.chips.saturating_sub(staked);
                s.status = None;
                s.phase = Phase::Spinning { since: now, clip, pocket };
                stats.spins += 1;
                info!("roulette: spin (Spin_{clip}): {staked} chips on {} bets; the ball will stop in {}", s.table.bets.len(), rules::pocket_name(pocket));
                play(sounds::SPIN);
            }
        }
    }
}

/// The spin's end: the bets paid, the earnings counted; the result shown, then the next
/// bets (or, banned, the menu closes).
#[allow(clippy::too_many_arguments)]
fn step(time: AidTime, mut screen: ResMut<RouletteScreen>, mut next: ResMut<NextState<PlayState>>, mut sim: ResMut<SimState>, script: Option<ResMut<ScriptState>>, mut stats: ResMut<RouletteStats>, mut sounds: MessageWriter<PlaySound>) {
    let Some(s) = screen.0.as_mut() else { return };
    let now = time.elapsed_secs();
    match s.phase {
        Phase::Spinning { since, clip, pocket } if now - since >= SPIN_SECONDS => {
            let staked = s.table.total();
            let (paid, net) = s.table.settle(pocket);
            let back: u32 = paid.iter().map(|p| p.back).sum();
            if back > 0 {
                if let Err(e) = sim.world.add_item(FormId::PLAYER_REF, s.chip, back as i32) {
                    warn!("roulette: winnings of {back} chips not given: {e:?}");
                }
                sounds.write(PlaySound { sound: sounds::WIN, at: None });
            }
            s.chips = carried(&sim, s.chip);
            if net >= 0 {
                stats.won += net as u64;
            } else {
                stats.lost += (-net) as u64;
            }
            if let Some(rt) = script.and_then(|x| x.into_inner().rt.as_mut()) {
                s.earnings = casino::add_earnings(&mut rt.game, s.casino, net, s.max_winnings);
                rt.game.count_stat(fnv_game::misc_stats::ROULETTE_GAMES_PLAYED, 1.0);
                if casino::winnings_level(s.earnings, s.max_winnings) >= 4 {
                    s.banned = true;
                    stats.bans += 1;
                }
            }
            let name = format!("{} {}", rules::pocket_name(pocket), rules::colour(pocket).name());
            let result = match net {
                n if n > 0 => trf("You win {n} chips.", &[("n", &n)]),
                0 if back > 0 => tr("You break even.").to_string(),
                n => trf("You lose {n} chips.", &[("n", &-n)]),
            };
            info!(
                "roulette: the ball stops in {name}: {} of {} bets win, {back} chips back of {staked}, net {net}; earnings {} of {} (level {}){}",
                paid.iter().filter(|p| p.back > 0).count(),
                paid.len(),
                s.earnings,
                s.max_winnings,
                casino::winnings_level(s.earnings, s.max_winnings),
                if s.banned { "; banned" } else { "" }
            );
            // The log's line names the pocket in English; the screen's in the chosen language.
            let shown = format!("{} {}", rules::pocket_name(pocket), tr(rules::colour(pocket).name()));
            s.status = Some([shown, if s.banned { trf("{result} You have been banned from {casino}.", &[("result", &result), ("casino", &s.casino_name)]) } else { result }]);
            s.last = Some(pocket);
            s.phase = Phase::Paid { until: now + RESULT_SECONDS, since, clip, pocket };
        }
        Phase::Paid { until, .. } if now >= until => {
            if s.banned {
                info!("roulette: closed (banned at {})", s.casino_name);
                next.set(PlayState::Playing);
                return;
            }
            s.phase = Phase::Betting;
            s.bet = s.table.clamp_bet(s.bet.min(s.chips.max(1)));
            if s.chips == 0 {
                info!("roulette: closed (no chips left)");
                next.set(PlayState::Playing);
            }
        }
        _ => {}
    }
}

fn close(mut screen: ResMut<RouletteScreen>, script: Option<ResMut<ScriptState>>) {
    if screen.0.take().is_none() {
        return;
    }
    if let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) {
        if rt.game.menu == Some(MENU_ROULETTE) {
            rt.game.close_menu();
        }
    }
}

fn report(mut exit: MessageReader<AppExit>, stats: Res<RouletteStats>) {
    if exit.read().next().is_none() || stats.asked == 0 {
        return;
    }
    let s = &*stats;
    info!(
        "roulette summary: {} asked, {} opened ({} refused); {} bets placed, {} refused; {} spins, {} chips won, {} lost; {} bans",
        s.asked, s.opened, s.refused, s.placed, s.refused_bets, s.spins, s.won, s.lost, s.bans
    );
}

/// [design, #2403] Synthetic logical/ineffective states for detector regressions.
#[cfg(feature = "report-test-support")]
#[doc(hidden)]
pub fn report_progress_fixture(step: usize) -> RouletteScreen {
    let mut table = Table::default();
    if step >= 3 {
        table.bets.push((rules::spot("S01").unwrap(), 1));
    }
    RouletteScreen(Some(Session {
        casino: FormId(42),
        casino_name: String::new(),
        chip: FormId(43),
        model: None,
        max_winnings: 1000,
        table,
        bet: if step >= 1 { 2 } else { 1 },
        cursor: if step >= 2 { "S01".into() } else { "S02".into() },
        phase: if step >= 4 { Phase::Spinning { since: step as f32, clip: 1, pocket: 1 } } else { Phase::Betting },
        chips: 100,
        earnings: 0,
        status: None,
        help: None,
        last: None,
        banned: false,
        forced: None,
        rng: ChaCha8Rng::seed_from_u64(step as u64),
        table_ref: None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fnv_game::casino::Closed;

    #[test]
    fn the_aid_reads_its_steps() {
        let s = parse_aid("1:chips:500,2:open,3:bet:25,4:at:d12,5:place,6:pocket:37,7:spin,8:exit,9:open:00140521,10:chips:9:140521").unwrap();
        assert_eq!(s[0], (1.0, AidStep::Chips(500, THE_TOPS)));
        assert_eq!(s[1], (2.0, AidStep::Open(THE_TOPS)));
        assert_eq!(s[2], (3.0, AidStep::Input(RouletteInput::Bet(25))));
        assert_eq!(s[3], (4.0, AidStep::Input(RouletteInput::At("D12".into()))));
        assert_eq!(s[5], (6.0, AidStep::Input(RouletteInput::Pocket(37))));
        assert_eq!(s[8], (9.0, AidStep::Open(0x0014_0521)));
        assert_eq!(s[9], (10.0, AidStep::Chips(9, 0x0014_0521)));
        assert!(parse_aid("1:at:Z9").is_err());
        assert!(parse_aid("1:pocket:38").is_err());
        assert!(parse_aid("1:deal").is_err());
    }

    #[test]
    fn closed_lines_say_why() {
        assert!(casino::closed_line(Closed::NoChips, "The Tops").contains("The Tops chips"));
        assert!(casino::closed_line(Closed::Banned, "The Tops").contains("banned"));
    }
}
