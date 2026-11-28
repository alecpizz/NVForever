//! Slot machines (docs/spec-notes/slots.md): a casino's slot machine, activated by the
//! player, runs `ShowSlotMachineMenuParams <casino> 1 <max> 0`; when the player carries the
//! casino's chips and is not banned there (`fnv_game::casino::opening`) the slot machine
//! menu opens, else a line says why. Increase and Decrease Bet change the bet, Bet Max
//! makes it the largest; Spin takes the bet and turns the three reels (laid out from the
//! casino record's reel stops, `fnv_game::slots::reel`); when the last reel stops the
//! payline is paid (`fnv_game::slots::multiple`), the net goes to the player's earnings at
//! the casino and its winnings level (`fnv_game::casino::add_earnings`). At the casino's
//! max winnings the player is banned: the menu closes after the spin. Payout List shows
//! what each line pays.
//!
//! The first time the menu opens the help message `HelpSlots` comes up over it, as
//! lockpicking's (`crate::lockpick::help`).
//!
//! At a machine in the world (`seat`): the player sits on its stool or stands in
//! front of it while the menu is open, and the spins, results and win celebrations go out
//! as [`SlotsSpin`] messages for the body, the machine's model and the camera.
//!
//! It is a `PlayState::Menu`: the world and the game clock stand still. To scripts it is
//! menu [`fnv_game::slots::MENU_CODE`]. The menu's text and buttons follow
//! `menus\slot_machine_menu.xml`; the reels are drawn flat from the record's symbol
//! pictures (`screen`).
//!
//! Keys [documented: the menu file's `_PCButton_W/E/Q/F/S/R`]: W spins, E and Q increase
//! and decrease the bet, F shows the payout list, S bets the most, R leaves (Esc and Tab
//! too, Enter spins [design]). The buttons can be clicked.
//!
//! Test aid `NVF_PLAY_TEST_SLOTS=<seconds>:<step>[,…]`: `chips:<n>[:<casino hex>]` gives the
//! player that many of the casino's chips (The Tops' when left out), `open[:<casino hex>]`
//! as if a machine's script asked (min 1, max 75), `open:<casino hex>:<machine hex>` as if
//! that machine's did (the player sits or stands at it, `seat`), `seed:<n>` (before `open`), `bet:<n>`
//! sets the bet, `up`, `down`, `max`, `reels:<a>-<b>-<c>` makes the next spin stop on those
//! symbols (1 to 7, 7 the cherries W; slots.md §2), `spin`, `payout` shows or hides the
//! payout list, `help` closes the help message, `exit` leaves. At exit, when the menu was
//! asked for: "slots summary: …". `open:<casino hex>:<machine hex>` names the machine
//! (the reference whose script would ask); left out, the slot machine nearest the player.
//!
//! The machine: the session knows the placed slot machine it was opened at
//! ([`Session::machine`]: the reference whose script asked, else the nearest one), and
//! tells the rest of the game what happens on it with [`SlotsSpin`] (opened, spin started,
//! the result and how long its celebration lasts, closed): nvforever `slots_view.rs` turns
//! the reels on that machine in the world and moves the camera; the player's seat and
//! animations listen too. While the reels turn on the machine itself ([`SlotsReels3d`])
//! the flat reels are not drawn (`screen`).

pub mod machines;
pub mod screen;
pub mod seat;

use crate::lockpick::Help;
use crate::pause::{AidTime, Paused};
use crate::script::ScriptState;
use crate::sfx::PlaySound;
use crate::sim::SimState;
use crate::{PlaySet, PlayState};
use bevy::prelude::*;
use crate::ui_pointer::PointerState;
use fnv_game::casino;
use fnv_util::words::{tr, trf};
use fnv_game::slots::{self as rules, Limits, REELS};
use fnv_sim::FormId;
use rand_chacha::ChaCha8Rng;
use rand_core::SeedableRng;
pub use seat::{SlotsSeat, Stance};
use seat::SeatPhase;

/// The menu code scripts see.
pub const MENU_SLOTS: u16 = rules::MENU_CODE;
/// The Tops' casino record, the test aid's default [observed: FalloutNV.esm CSNO
/// `TheTopsCasinoData`].
pub const THE_TOPS: u32 = 0x0014_4C3F;
/// When the reels start to turn after the lever is pulled, seconds: frame 15 of the reel
/// models' 30-frame `Forward` clip, all three alike [observed: fnvtool nif decode of
/// `nv_slotmachine-reel01..03.nif`, the `Cylinder0n` rotation keys].
pub const REEL_START: f32 = 0.5;
/// When each reel stops after the lever is pulled, seconds: frames 71, 99 and 127 of the
/// reel models' `Forward` clip [observed: as [`REEL_START`]]. The pull sound
/// (GAMESlotsPullLever, `sfx_slots_pulllever.wav`, 4.78 s) has its three stop clunks at
/// about 2.37, 3.31 and 4.25 s [observed: its loudness over time], so the reels stop on
/// them when the sound and the spin start together.
pub const REEL_STOPS: [f32; REELS] = [71.0 / 30.0, 99.0 / 30.0, 127.0 / 30.0];
/// Whole turns each reel makes before it stops: 720, 1080 and 1440 degrees in the reel
/// models' clip, one even speed for all three [observed: as [`REEL_START`]].
pub const REEL_TURNS: [usize; REELS] = [2, 3, 4];
/// The bounce at each stop: the reel runs on this far past its stop, degrees, in this long,
/// and comes back in as long again [observed: the keys 5 degrees past each stop, 2 frames
/// after it, and on the stop again 2 frames later].
pub const REEL_BOUNCE_DEGREES: f32 = 5.0;
pub const REEL_BOUNCE_SECONDS: f32 = 2.0 / 30.0;
/// The reel clip's end, frame 131: the right reel is back on its stop and all three rest
/// [observed: the `Forward` sequence's stop time, 4.367 s]. The line is paid and its sound
/// played here, after the right reel's clunk (loudest 4.255 to 4.39 s in the pull sound
/// [observed]), which the result sounds, loud from their first frame [observed], would
/// otherwise cover [design].
pub const REEL_CLIP_END: f32 = 131.0 / 30.0;
/// How long the result stays on the screen before the next spin can start [design].
pub const RESULT_SECONDS: f32 = 2.0;
/// The result stays up this long after a win's celebration ends, seconds [design: room
/// for the camera to come back, the second part of the slots work].
pub const CELEBRATION_MARGIN: f32 = 0.5;
/// A win's camera (nvforever `slots_view.rs`): its eased zoom out to third person of the
/// player and its way back in to the reels after the celebration, seconds [design].
pub const WIN_CAMERA_OUT: f32 = 1.2;
pub const WIN_CAMERA_BACK: f32 = 1.4;
/// After a win no spin starts until its whole sequence is over: the zoom out, the
/// celebration and the way back in [design: blocked from
/// playing while the win animation is playing]. The bet and Exit stay usable.
pub fn win_hold(celebrate: f32) -> f32 {
    celebrate + WIN_CAMERA_OUT + WIN_CAMERA_BACK
}
/// How long a request waits for the player to be back in the world, seconds [design].
pub const REQUEST_WAIT: f32 = 3.0;

/// The slot machine sounds [observed: FalloutNV.esm SOUN records named for them; design:
/// which event each is for, slots.md §5].
pub mod sounds {
    use fnv_sim::FormId;
    pub const ACTIVATE: FormId = FormId(0x0014_D3EB);
    pub const PULL: FormId = FormId(0x0014_89D3);
    pub const INCREASE: [FormId; 3] = [FormId(0x0014_89D4), FormId(0x0014_89D5), FormId(0x0014_89D6)];
    pub const DECREASE: FormId = FormId(0x0014_89D7);
    pub const WIN_SMALL: FormId = FormId(0x0014_89D8);
    pub const WIN_MED: FormId = FormId(0x0014_89D9);
    pub const WIN_JACKPOT: FormId = FormId(0x0014_89DA);
    pub const LOSE: FormId = FormId(0x0014_89FA);
}

/// How near the player a slot machine must stand to be the one played when the request
/// names none (the test aid), game units [design].
pub const MACHINE_REACH: f32 = 250.0;

/// The reels turn on the placed machine in the world (nvforever `slots_view.rs` sets it):
/// the flat reels are not drawn, nor the black ground behind the menu.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotsReels3d(pub bool);

/// What happens on a slot machine, for the machine's drawing, its camera and the player's
/// seat (the hook between the slots work's two parts).
#[derive(Message, Clone, Debug, PartialEq)]
pub struct SlotsSpin {
    /// The placed machine, if known.
    pub machine: Option<FormId>,
    pub phase: SpinPhase,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SpinPhase {
    /// The menu opened at the machine.
    Opened,
    /// The lever is pulled: the reels will stop at `to` (stops), showing `line` (symbols).
    Started { to: [usize; REELS], line: [u8; REELS] },
    /// The last reel stopped: the line paid `multiple` times the bet (`back` chips handed
    /// back, 0 on a loss); a win is celebrated for `celebrate` seconds
    /// (`fnv_game::slots::celebration`).
    Result { multiple: u32, back: u32, celebrate: f32 },
    /// The menu closed.
    Closed,
}

/// Where each reel stands, in stops (a fraction while it turns), `t` seconds after a spin
/// from `from` to `to` started, on a reel of `n` stops, as the reel models' `Forward` clip
/// turns them: still until [`REEL_START`], then an even speed to the stop at its time
/// ([`REEL_STOPS`]), a short bounce past it and back (slots.md §4). The keys' tangents
/// match the turn's own size, so the turn is even [hypothesis: Gamebryo quadratic keys'
/// tangents span the key interval]; the bounce eases out and back [observed: its tangents].
pub fn reel_positions(n: usize, from: [usize; REELS], to: [usize; REELS], t: f32) -> [f32; REELS] {
    let n = n.max(1);
    std::array::from_fn(|i| {
        let k = ((t - REEL_START) / (REEL_STOPS[i] - REEL_START)).clamp(0.0, 1.0);
        let ahead = (to[i] + n - from[i] % n) % n + REEL_TURNS[i] * n;
        let past = REEL_BOUNCE_DEGREES / 360.0 * n as f32;
        let b = (t - REEL_STOPS[i]) / REEL_BOUNCE_SECONDS;
        let bounce = if (0.0..1.0).contains(&b) {
            past * (2.0 * b - b * b)
        } else if (1.0..2.0).contains(&b) {
            let s = b - 1.0;
            past * (1.0 - s * s * (3.0 - 2.0 * s))
        } else {
            0.0
        };
        from[i] as f32 + ahead as f32 * k + bounce
    })
}

/// The open slot machine menu, if any.
#[derive(Resource, Default)]
pub struct SlotsScreen(pub Option<Session>);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    Betting,
    /// The reels turn: since (aid seconds), where each started and the stop it comes to.
    Spinning { since: f32, from: [usize; REELS], to: [usize; REELS] },
    /// The line is paid; the result shows until `until` (aid seconds).
    Paid { until: f32 },
}

pub struct Session {
    pub casino: FormId,
    /// The slot machine played (its reference), when the menu came from one.
    pub machine: Option<FormId>,
    /// Seated on its stool or standing at it.
    pub stance: Option<Stance>,
    /// A win's celebration ends then (aid seconds).
    pub celebrating: Option<f32>,
    /// No spin starts before then (aid seconds): a win's whole sequence ([`win_hold`]).
    pub win_hold: Option<f32>,
    /// Spins made at this machine since the menu opened.
    pub spins: u32,
    pub casino_name: String,
    pub chip: FormId,
    /// The symbols' pictures (CSNO ICON), under `textures\`, symbol 1 to W.
    pub symbols: Vec<String>,
    /// The reel's stops: the symbol (0 to 6) at each.
    pub reel: Vec<u8>,
    pub max_winnings: u32,
    pub limits: Limits,
    pub bet: u32,
    /// The stop each reel shows on the line.
    pub stops: [usize; REELS],
    pub phase: Phase,
    /// The chips the player carries.
    pub chips: u32,
    pub earnings: i64,
    /// The status line (slots.md §5).
    pub status: Option<String>,
    pub help: Option<Help>,
    /// The payout list is shown.
    pub payouts: bool,
    /// Banned by the last spin: the menu closes when its result has shown.
    pub banned: bool,
    /// The test aid's line for the next spin (symbols 0 to 6).
    forced: Option<[u8; REELS]>,
    increases: usize,
    rng: ChaCha8Rng,
}

impl Session {
    /// Where each reel stands, in stops (a fraction while it turns), at `now`.
    pub fn positions(&self, now: f32) -> [f32; REELS] {
        match self.phase {
            Phase::Spinning { since, from, to } => reel_positions(self.reel.len(), from, to, now - since),
            _ => self.stops.map(|s| s as f32),
        }
    }

    /// The symbols on the line.
    pub fn line(&self) -> [u8; REELS] {
        self.stops.map(|s| self.reel.get(s % self.reel.len().max(1)).copied().unwrap_or(0))
    }

    pub fn betting(&self) -> bool {
        self.phase == Phase::Betting && self.help.is_none()
    }

    /// A win's sequence still holds the next spin at `now` ([`win_hold`]).
    pub fn held(&self, now: f32) -> bool {
        self.win_hold.is_some_and(|until| now < until)
    }

    /// The lever pulled to `to` (stops): the reels turn from where they stand; the message
    /// for the machine's drawing and the seat.
    fn begin(&mut self, to: [usize; REELS], now: f32) -> SlotsSpin {
        let from = self.stops;
        self.stops = to;
        self.payouts = false;
        self.status = None;
        self.phase = Phase::Spinning { since: now, from, to };
        self.spins += 1;
        SlotsSpin { machine: self.machine, phase: SpinPhase::Started { to, line: self.line() } }
    }

    /// The stops showing line `l` (symbols): the first stop of each symbol.
    fn stops_of(&self, l: [u8; REELS]) -> [usize; REELS] {
        std::array::from_fn(|i| rules::stop_of(&self.reel, l[i]).unwrap_or(0))
    }
}

/// A command for the open menu.
#[derive(Message, Clone, Debug, PartialEq)]
pub enum SlotsInput {
    Spin,
    Increase,
    Decrease,
    Max,
    Payouts,
    /// Test aid: the bet.
    Bet(u32),
    /// Test aid: the next spin's line (symbols 0 to 6).
    Line([u8; REELS]),
    CloseHelp,
    Exit,
}

#[derive(Resource, Default, Debug)]
struct SlotsStats {
    asked: u32,
    opened: u32,
    refused: u32,
    spins: u32,
    wins: u32,
    won: u64,
    lost: u64,
    bans: u32,
}

/// `NVF_PLAY_TEST_SLOTS`.
#[derive(Resource, Default)]
struct SlotsAid {
    steps: Vec<(f32, AidStep)>,
    seed: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
enum AidStep {
    Open(u32, Option<u32>),
    Chips(u32, u32),
    Seed(u64),
    Input(SlotsInput),
}

fn hex(s: &str) -> Option<u32> {
    u32::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok()
}

/// `a-b-c`, symbols 1 to 7, as 0 to 6.
fn parse_line(s: &str) -> Option<[u8; REELS]> {
    let v: Vec<u8> = s.split('-').map(|x| x.trim().parse::<u8>().ok().filter(|n| (1..=rules::SYMBOLS as u8).contains(n)).map(|n| n - 1)).collect::<Option<_>>()?;
    v.try_into().ok()
}

fn parse_aid(v: &str) -> Result<Vec<(f32, AidStep)>, String> {
    crate::test_aid::timed(v)?
        .into_iter()
        .map(|(t, what)| {
            let (op, arg) = what.split_once(':').unwrap_or((what, ""));
            let num = |name: &str, s: &str| s.trim().parse::<u64>().map_err(|_| format!("{what:?}: expected {op}:<{name}>"));
            let step = match op {
                "open" if arg.is_empty() => AidStep::Open(THE_TOPS, None),
                "open" => {
                    let bad = || format!("{what:?}: expected open:<casino hex>[:<machine hex>]");
                    let (casino, machine) = arg.split_once(':').map_or((arg, None), |(c, m)| (c, Some(m)));
                    AidStep::Open(hex(casino).ok_or_else(bad)?, machine.map(|m| hex(m).ok_or_else(bad)).transpose()?)
                }
                "chips" => {
                    let (n, casino) = arg.split_once(':').unwrap_or((arg, ""));
                    let casino = if casino.is_empty() { THE_TOPS } else { hex(casino).ok_or_else(|| format!("{what:?}: expected chips:<n>:<casino hex>"))? };
                    AidStep::Chips(num("n", n)? as u32, casino)
                }
                "seed" => AidStep::Seed(num("n", arg)?),
                "bet" => AidStep::Input(SlotsInput::Bet(num("n", arg)? as u32)),
                "reels" => AidStep::Input(SlotsInput::Line(parse_line(arg).ok_or_else(|| format!("{what:?}: expected reels:<a>-<b>-<c>, symbols 1 to 7"))?)),
                "up" => AidStep::Input(SlotsInput::Increase),
                "down" => AidStep::Input(SlotsInput::Decrease),
                "max" => AidStep::Input(SlotsInput::Max),
                "payout" => AidStep::Input(SlotsInput::Payouts),
                "spin" => AidStep::Input(SlotsInput::Spin),
                "help" => AidStep::Input(SlotsInput::CloseHelp),
                "exit" => AidStep::Input(SlotsInput::Exit),
                _ => return Err(format!("{what:?}: expected open, chips:, seed:, bet:, up, down, max, reels:, spin, payout, help or exit")),
            };
            Ok((t, step))
        })
        .collect()
}

pub struct SlotsPlugin;

impl Plugin for SlotsPlugin {
    fn build(&self, app: &mut App) {
        let steps = match std::env::var("NVF_PLAY_TEST_SLOTS").map(|v| parse_aid(&v)) {
            Ok(Ok(a)) => a,
            Ok(Err(e)) => {
                warn!("NVF_PLAY_TEST_SLOTS: {e}");
                Vec::new()
            }
            Err(_) => Vec::new(),
        };
        app.init_resource::<SlotsScreen>()
            .init_resource::<SlotsStats>()
            .insert_resource(SlotsAid { steps, seed: None })
            .add_message::<SlotsInput>()
            .add_message::<SlotsSpin>()
            .init_resource::<SlotsSeat>()
            .init_resource::<SlotsReels3d>()
            .init_resource::<machines::SlotMachines>()
            // The input before the frame's state change: Exit closes the menu on the frame
            // it is pressed, so that frame already draws the world, not the menu
            // [observed: test run, play-64s: the Exit frame drew the
            // menu and the win's camera; the view after it came a frame later].
            .add_systems(
                PreUpdate,
                (run_aid, (keys, buttons, apply).chain().run_if(in_state(PlayState::Menu))).chain().after(bevy::ui::UiSystems::Focus),
            )
            .add_systems(
                Update,
                (open, step.run_if(in_state(PlayState::Menu)), machines::track, screen::draw)
                    .chain()
                    .after(crate::menu::open_menus)
                    .in_set(PlaySet::Ui),
            )
            .add_systems(Update, follow_seat.in_set(PlaySet::Ui))
            .add_systems(OnExit(PlayState::Menu), close)
            .add_systems(Last, report);
    }
}

/// How many of `item` the player carries.
fn carried(sim: &SimState, item: FormId) -> u32 {
    sim.world.peek(FormId::PLAYER_REF).and_then(|s| s.inventory.as_ref()).map_or(0, |inv| inv.count(item)).max(0) as u32
}

fn run_aid(time: AidTime, mut aid: ResMut<SlotsAid>, mut sim: ResMut<SimState>, mut script: Option<ResMut<ScriptState>>, mut input: MessageWriter<SlotsInput>) {
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
        info!("test aid NVF_PLAY_TEST_SLOTS: {step:?}");
        match step {
            AidStep::Open(c, machine) => {
                let Some(rt) = script.as_mut().and_then(|s| s.rt.as_mut()) else { continue };
                rt.game.slots_requests.push(rules::SlotsRequest { casino: FormId(c), min_bet: 1, max_bet: 75, max_winnings: 0, machine: machine.map(FormId) });
            }
            AidStep::Chips(n, c) => {
                let chip = script.as_ref().and_then(|s| s.rt.as_ref()).and_then(|rt| fnv_game::casino::chip(&rt.game, FormId(c)));
                match chip {
                    Some(chip) if n > 0 => match sim.world.add_item(FormId::PLAYER_REF, chip, n as i32) {
                        Ok(_) => info!("slots: the player is given {n} chips {chip} ({} carried)", carried(&sim, chip)),
                        Err(e) => warn!("slots: chips not given: {e:?}"),
                    },
                    _ => warn!("slots: no chip for casino {c:08X}"),
                }
            }
            AidStep::Seed(s) => aid.seed = Some(s),
            AidStep::Input(i) => {
                input.write(i);
            }
        }
    }
}

/// Whether a model is a slot machine's: the vanilla `clutter\casino\slot_machine01*.nif`
/// and `…02.nif` [observed: FalloutNV.esm's six slot activators], and the mods' that vary
/// them [hypothesis: they keep "slot" in the name].
pub fn slot_model(path: &str) -> bool {
    let m = path.to_ascii_lowercase();
    m.contains("slot") && !m.contains("static") && !m.contains("broken")
}

/// The machine played: the one asked for when it is in the world, else the slot machine
/// nearest the player within [`MACHINE_REACH`] (an activator whose model is a slot
/// machine's, [`slot_model`]).
pub fn machine_near(sim: &SimState, play: Option<&crate::PlayWorld>, asked: Option<FormId>) -> Option<FormId> {
    let pose = |id: FormId| sim.world.peek(id).map(|r| (r.pos, r.cell)).or_else(|| sim.refs.get(id).map(|r| (r.pos, r.stored_in)));
    if let Some(m) = asked.filter(|m| pose(*m).is_some()) {
        return Some(m);
    }
    let play = play?;
    let (from, cell) = pose(FormId::PLAYER_REF)?;
    let mut best: Option<(f32, FormId)> = None;
    for id in sim.refs.by_cell.get(&cell?).into_iter().flatten() {
        if !sim.present(id.0) {
            continue;
        }
        let Some((p, _)) = pose(*id) else { continue };
        let d = (0..3).map(|i| (p[i] - from[i]).powi(2)).sum::<f32>().sqrt();
        if d > MACHINE_REACH || best.is_some_and(|b| b.0 <= d) {
            continue;
        }
        let Some(base) = sim.world.peek(*id).map(|r| r.base).or_else(|| sim.base_of(id.0)) else { continue };
        if sim.forms.sig(base).is_some_and(|s| s == *b"ACTI") && crate::dropped::model_path(&play.0, base, 0).is_some_and(|m| slot_model(&m)) {
            best = Some((d, *id));
        }
    }
    best.map(|b| b.1)
}

/// A seed: different every time unless the test aid fixed it.
fn seed(aid: &SlotsAid, casino: FormId, now: f32) -> u64 {
    aid.seed.unwrap_or_else(|| {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
        t ^ ((casino.0 as u64) << 20) ^ now.to_bits() as u64
    })
}

/// What seating the player at a machine needs (`seat`).
#[derive(bevy::ecs::system::SystemParam)]
pub struct Place<'w> {
    seat: Res<'w, crate::furniture::PlayerSeat>,
    slots_seat: ResMut<'w, SlotsSeat>,
    sit: MessageWriter<'w, crate::furniture::UseFurniture>,
    phys: ResMut<'w, crate::phys::PhysState>,
    player: ResMut<'w, crate::player::Player>,
    spins: MessageWriter<'w, SlotsSpin>,
    control: Option<ResMut<'w, crate::control::PlayerControl>>,
}

#[allow(clippy::too_many_arguments)]
fn open(
    state: Res<State<PlayState>>,
    mut next: ResMut<NextState<PlayState>>,
    mut screen: ResMut<SlotsScreen>,
    paused: Res<Paused>,
    sim: Res<SimState>,
    script: Option<ResMut<ScriptState>>,
    aid: Res<SlotsAid>,
    time: AidTime,
    (mut stats, mut corner, bindings, mut sounds): (ResMut<SlotsStats>, Option<ResMut<crate::corner_messages::CornerMessages>>, Res<crate::bindings::Bindings>, MessageWriter<PlaySound>),
    play: Option<Res<crate::PlayWorld>>,
    mut waiting: Local<Option<(rules::SlotsRequest, f32)>>,
    mut place: Place,
) {
    let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) else { return };
    let now = time.elapsed_secs();
    if let Some(asked) = rt.game.slots_requests.pop() {
        rt.game.slots_requests.clear();
        stats.asked += 1;
        *waiting = Some((asked, now));
    }
    let Some((asked, since)) = *waiting else { return };
    // A request made while another menu closes waits for the world a moment [design].
    if *state.get() != PlayState::Playing || screen.0.is_some() || paused.0 {
        if now - since > REQUEST_WAIT {
            info!("slots: request for {} dropped, the player is not in the world", asked.casino);
            *waiting = None;
        }
        return;
    }
    *waiting = None;
    let Some(def) = rt.game.data.casinos.get(&asked.casino).cloned() else {
        warn!("slots: {} is not a casino", asked.casino);
        return;
    };
    let max = casino::max_winnings(&rt.game, asked.casino, asked.max_winnings);
    // A script's SetCasinoChip first (`fnv_game::casino::chip`).
    let def_chip = casino::chip(&rt.game, asked.casino);
    let chips = def_chip.map_or(0, |c| carried(&sim, c));
    if let Err(why) = casino::opening(&rt.game, asked.casino, chips, max) {
        stats.refused += 1;
        let line = casino::closed_line(why, &def.name);
        info!("slots: {} {} refused ({why:?}, {chips} chips): on screen: {line:?}", asked.casino, def.name);
        if let Some(c) = corner.as_deref_mut() {
            c.push(line.into());
        }
        return;
    }
    let Some(chip) = def_chip else { return };
    // At the machine first: on its stool, or standing in front of it.
    // The placed machine: the reference whose script asked, else the nearest (test aid).
    let machine = machine_near(&sim, play.as_deref(), asked.machine);
    let stance = machine.and_then(|m| seat::stance_for(&sim, m.0));
    if let (Some(m), Some((frame, st))) = (machine.map(|m| m.0), stance) {
        match st {
            Stance::Seated { stool } => {
                let on = place.seat.on == Some(stool) && place.seat.seated;
                if !on {
                    if place.slots_seat.phase != SeatPhase::Sitting || place.slots_seat.machine != Some(m) {
                        info!("slots seat: the player sits on stool {stool:08X} at machine {m:08X}");
                        place.sit.write(crate::furniture::UseFurniture { reference: stool });
                        *place.slots_seat = SlotsSeat { machine: Some(m), stance: Some(st), phase: SeatPhase::Sitting, since: now };
                    }
                    if now - place.slots_seat.since < seat::SIT_WAIT {
                        *waiting = Some((asked, now));
                        return;
                    }
                    warn!("slots seat: not on stool {stool:08X} after {}s; the menu opens anyway", seat::SIT_WAIT);
                }
                // The view on the reels from the seated eye.
                let (h, p) = seat::look_at_reels(frame, place.player.body_eye.unwrap_or(place.player.eye()));
                info!("slots seat: seated at machine {m:08X} on stool {stool:08X}");
                (place.player.heading, place.player.pitch) = (h, p);
                // Sitting put the weapon away (`furniture::drive_player`); also when the
                // menu opened without the sit having happened (above).
                match place.control.as_deref_mut() {
                    Some(c) if c.weapon_out => {
                        c.weapon_out = false;
                        info!("slots seat: the weapon is put away at the machine");
                    }
                    Some(_) => info!("slots seat: the weapon is away (sitting put it away)"),
                    None => {}
                }
            }
            Stance::Standing { spot, heading } => {
                seat::stand_player(&mut place.phys, &mut place.player, [spot[0], spot[1]], spot[2], heading);
                // The weapon goes away as it does on the stool (`furniture::drive_player`):
                // the win shot and the cheer show the body, both hands up [design].
                if let Some(c) = place.control.as_deref_mut().filter(|c| c.weapon_out) {
                    c.weapon_out = false;
                    info!("slots seat: the weapon is put away at the machine");
                }
                let eye = place.player.eye();
                let (h, p) = seat::look_at_reels(frame, eye);
                (place.player.heading, place.player.pitch) = (h, p);
                info!("slots seat: standing at machine {m:08X} ({:.0}, {:.0}, {:.0}), facing {:.0} deg (no stool in front)", spot[0], spot[1], spot[2], heading.to_degrees());
            }
        }
        *place.slots_seat = SlotsSeat { machine: Some(m), stance: Some(st), phase: SeatPhase::Playing, since: now };
    }
    place.spins.write(SlotsSpin { machine, phase: SpinPhase::Opened });
    stats.opened += 1;
    let limits = Limits::new(asked.min_bet, asked.max_bet);
    let reel = rules::reel(&def.reel_stops);
    let earnings = rt.game.player.casino_earnings.get(&asked.casino).copied().unwrap_or(0);
    rt.game.open_menu(MENU_SLOTS);
    let help = rules::first_help(&mut rt.game).map(|(title, text)| Help::new(&rt.game, title, &text, &bindings));
    info!(
        "slots: open {} ({}): bets {} to {}, {chips} chips carried, earnings {earnings} of {max}, reel stops {:?} ({} a reel), {} symbol pictures, model {}{}",
        def.name,
        asked.casino,
        limits.min_bet,
        limits.max_bet,
        def.reel_stops,
        reel.len(),
        def.symbols.len(),
        def.slot_model.as_deref().unwrap_or("(none)"),
        if help.is_some() { "; help message shown (first use)" } else { "" }
    );
    sounds.write(PlaySound { sound: sounds::ACTIVATE, at: None });
    info!("slots: the machine is {}", machine.map_or_else(|| "none found".to_string(), |m| format!("{:08X}{}", m.0, if asked.machine.is_some() { "" } else { " (the nearest)" })));
    let mut rng = ChaCha8Rng::seed_from_u64(seed(&aid, asked.casino, now));
    let n = reel.len().max(1);
    let stops = std::array::from_fn(|_| (rand_core::RngCore::next_u32(&mut rng) as usize) % n);
    screen.0 = Some(Session {
        casino: asked.casino,
        machine,
        stance: stance.map(|s| s.1),
        celebrating: None,
        win_hold: None,
        spins: 0,
        casino_name: def.name.clone(),
        chip,
        symbols: def.symbols.clone(),
        reel,
        max_winnings: max,
        bet: limits.clamp(limits.min_bet, chips),
        limits,
        stops,
        phase: Phase::Betting,
        chips,
        earnings,
        status: None,
        help,
        payouts: false,
        banned: false,
        forced: None,
        increases: 0,
        rng,
    });
    next.set(PlayState::Menu);
}

fn keys(
    (mut raw, switching): (ResMut<ButtonInput<KeyCode>>, Option<Res<crate::pad::SwitchingInput>>),
    screen: Res<SlotsScreen>,
    mut out: MessageWriter<SlotsInput>,
    tweaks: Option<Res<crate::tweaks::TweakSettings>>,
) {
    let Some(s) = screen.0.as_ref() else { return };
    // The action view (`crate::pad::ActionButtons`), owned: the exit below clears the raw
    // presses it read.
    let keys = switching.as_ref().map_or_else(|| (*raw).clone(), |f| f.filter(&raw).into_owned());
    // The player's tweak: W held spins again each time the reels stop [design: docs/design/tweaks.md].
    if tweaks.is_some_and(|t| t.slots_hold_to_spin) && keys.pressed(KeyCode::KeyW) && !keys.just_pressed(KeyCode::KeyW) && s.betting() {
        out.write(SlotsInput::Spin);
        return;
    }
    if s.help.is_some() {
        if keys.any_just_pressed([KeyCode::KeyE, KeyCode::Escape, KeyCode::Tab, KeyCode::Enter]) {
            out.write(SlotsInput::CloseHelp);
        }
        return;
    }
    if keys.any_just_pressed([KeyCode::KeyR, KeyCode::Escape, KeyCode::Tab]) {
        out.write(SlotsInput::Exit);
        // The game is back this same frame (the input runs before the state change): the
        // press that closed the menu must not also reload, pause or open the Pip-Boy.
        for k in [KeyCode::KeyR, KeyCode::Escape, KeyCode::Tab] {
            raw.clear_just_pressed(k);
        }
        return;
    }
    let pairs = [
        (KeyCode::KeyW, SlotsInput::Spin),
        (KeyCode::Enter, SlotsInput::Spin),
        (KeyCode::KeyE, SlotsInput::Increase),
        (KeyCode::KeyQ, SlotsInput::Decrease),
        (KeyCode::KeyF, SlotsInput::Payouts),
        (KeyCode::KeyS, SlotsInput::Max),
    ];
    for (k, i) in pairs {
        if keys.just_pressed(k) {
            out.write(i);
        }
    }
}

fn buttons(
    screen: Res<SlotsScreen>,
    pressed: Query<(&PointerState, &screen::SlotsButton), Changed<PointerState>>,
    help_close: Query<(&PointerState, &crate::lockpick::screen::LockButton), Changed<PointerState>>,
    mut out: MessageWriter<SlotsInput>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
) {
    if screen.0.is_none() {
        return;
    }
    for (i, b) in &pressed {
        if *i == PointerState::Pressed {
            // As with the keys ([`keys`]): the click on Exit does not also fire.
            if b.0 == SlotsInput::Exit {
                mouse.clear_just_pressed(MouseButton::Left);
            }
            out.write(b.0.clone());
        }
    }
    if help_close.iter().any(|(i, _)| *i == PointerState::Pressed) {
        out.write(SlotsInput::CloseHelp);
    }
}

/// The win sound by what the line paid, times the bet (slots.md §5) [design].
pub fn win_sound(multiple: u32) -> FormId {
    match multiple {
        0 => sounds::LOSE,
        1..=5 => sounds::WIN_SMALL,
        6..=30 => sounds::WIN_MED,
        _ => sounds::WIN_JACKPOT,
    }
}

#[allow(clippy::too_many_arguments)]
fn apply(
    mut input: MessageReader<SlotsInput>,
    mut screen: ResMut<SlotsScreen>,
    mut sim: ResMut<SimState>,
    time: AidTime,
    mut stats: ResMut<SlotsStats>,
    mut next: ResMut<NextState<PlayState>>,
    (mut sounds, mut spin_out): (MessageWriter<PlaySound>, MessageWriter<SlotsSpin>),
) {
    let now = time.elapsed_secs();
    let mut play = |sound| {
        sounds.write(PlaySound { sound, at: None });
    };
    for cmd in input.read() {
        let Some(s) = screen.0.as_mut() else { return };
        match cmd.clone() {
            SlotsInput::CloseHelp => {
                if s.help.take().is_some() {
                    info!("slots: help message closed");
                }
            }
            _ if s.help.is_some() => {}
            SlotsInput::Line(l) => s.forced = Some(l),
            // Exit also while a result shows, a win's sequence too: closing ends it (the
            // camera, the seat and the celebration go with the menu).
            SlotsInput::Exit => {
                if matches!(s.phase, Phase::Betting | Phase::Paid { .. }) {
                    info!("slots: closed (the player left)");
                    next.set(PlayState::Playing);
                }
            }
            _ if s.phase != Phase::Betting => {}
            SlotsInput::Payouts => {
                s.payouts = !s.payouts;
                info!("slots: payout list {}", if s.payouts { "shown" } else { "hidden" });
            }
            SlotsInput::Bet(b) => s.bet = s.limits.clamp(b, s.chips),
            SlotsInput::Increase | SlotsInput::Decrease | SlotsInput::Max => {
                let b = match cmd {
                    SlotsInput::Increase => s.limits.step(s.bet, true, s.chips),
                    SlotsInput::Decrease => s.limits.step(s.bet, false, s.chips),
                    _ => s.limits.top(s.chips),
                };
                if b != s.bet {
                    if b > s.bet {
                        play(sounds::INCREASE[s.increases % 3]);
                        s.increases += 1;
                    } else {
                        play(sounds::DECREASE);
                    }
                    s.bet = b;
                    info!("slots: bet {}", s.bet);
                }
            }
            SlotsInput::Spin if s.held(now) => {
                let left = s.win_hold.map_or(0.0, |t| t - now);
                info!("slots: spin ignored: the win's sequence has {left:.1}s to go");
            }
            SlotsInput::Spin => {
                let bet = s.bet;
                if bet == 0 || bet > s.chips {
                    s.status = Some(tr("You do not have enough chips.").into());
                    continue;
                }
                if let Err(e) = sim.world.remove_item(FormId::PLAYER_REF, s.chip, bet as i32) {
                    warn!("slots: the bet of {bet} chips not taken: {e:?}");
                    continue;
                }
                s.chips = s.chips.saturating_sub(bet);
                let forced = s.forced.take();
                let n = s.reel.len().max(1);
                let to = match forced {
                    // The test aid's line: the first stop showing each symbol.
                    Some(l) => s.stops_of(l),
                    None => rules::spin(&mut s.rng, n),
                };
                spin_out.write(s.begin(to, now));
                stats.spins += 1;
                let names = s.line().map(|x| rules::NAMES[x as usize]);
                info!("slots: spin: bet {bet}, {} chips left; the reels will stop on {} {} {}", s.chips, names[0], names[1], names[2]);
                play(sounds::PULL);
            }
        }
    }
}

/// The spin's end: the line paid, the earnings counted; the result shown, then the next
/// spin (or, banned, the menu closes).
#[allow(clippy::too_many_arguments)]
fn step(
    time: AidTime,
    mut screen: ResMut<SlotsScreen>,
    mut next: ResMut<NextState<PlayState>>,
    mut sim: ResMut<SimState>,
    script: Option<ResMut<ScriptState>>,
    mut stats: ResMut<SlotsStats>,
    (mut sounds, mut spin_out): (MessageWriter<PlaySound>, MessageWriter<SlotsSpin>),
    mut stopped: Local<(f32, usize)>,
) {
    let Some(s) = screen.0.as_mut() else { return };
    let now = time.elapsed_secs();
    // Each reel's stop in the log, against the pull sound's clunk (slots.md §4).
    if let Phase::Spinning { since, .. } = s.phase {
        if stopped.0 != since {
            *stopped = (since, 0);
        }
        while stopped.1 < REELS && now - since >= REEL_STOPS[stopped.1] {
            let i = stopped.1;
            info!("slots: reel {} stops {:.3}s after the pull (its stop {:.3}s, the pull sound's clunk about {:.3}s)", i + 1, now - since, REEL_STOPS[i], [2.375, 3.315, 4.255][i]);
            stopped.1 += 1;
        }
    }
    if s.celebrating.is_some_and(|until| now >= until) {
        s.celebrating = None;
        info!("slots seat: celebration ended");
    }
    if s.win_hold.is_some_and(|until| now >= until) {
        s.win_hold = None;
        info!("slots: spins open again (the win's sequence is over)");
    }
    match s.phase {
        Phase::Spinning { since, .. } if now - since >= REEL_CLIP_END => {
            let line = s.line();
            let multiple = rules::multiple(line);
            let back = s.bet * multiple;
            let net = back as i64 - s.bet as i64;
            if back > 0 {
                if let Err(e) = sim.world.add_item(FormId::PLAYER_REF, s.chip, back as i32) {
                    warn!("slots: winnings of {back} chips not given: {e:?}");
                }
                stats.wins += 1;
            }
            sounds.write(PlaySound { sound: win_sound(multiple), at: None });
            s.chips = carried(&sim, s.chip);
            if net >= 0 {
                stats.won += net as u64;
            } else {
                stats.lost += (-net) as u64;
            }
            if let Some(rt) = script.and_then(|x| x.into_inner().rt.as_mut()) {
                s.earnings = casino::add_earnings(&mut rt.game, s.casino, net, s.max_winnings);
                rt.game.count_stat(fnv_game::misc_stats::SLOTS_GAMES_PLAYED, 1.0);
                if casino::winnings_level(s.earnings, s.max_winnings) >= 4 {
                    s.banned = true;
                    stats.bans += 1;
                }
            }
            info!("slots: the result sound {:08X} plays {:.3}s after the pull", win_sound(multiple).0, now - since);
            let names = line.map(|x| rules::NAMES[x as usize]);
            let won = match rules::win_name(line) {
                Some(w) => format!("{w} pays {multiple} x {} = {back} chips", s.bet),
                None => "nothing".to_string(),
            };
            info!(
                "slots: the reels stop on {} {} {}: {won}, net {net}; earnings {} of {} (level {}){}",
                names[0],
                names[1],
                names[2],
                s.earnings,
                s.max_winnings,
                casino::winnings_level(s.earnings, s.max_winnings),
                if s.banned { "; banned" } else { "" }
            );
            // [hypothesis: the original's words are not in the plugins; these are NVForever's]
            // In the chosen language (`lang/*.toml`, #3146); the log's line above is English.
            let result = match rules::win_words(line) {
                Some(w) => trf("{win}: you win {back} chips.", &[("win", &tr(w)), ("back", &back)]),
                None => trf("You lose {n} chips.", &[("n", &s.bet)]),
            };
            s.status = Some(if s.banned { trf("{result} You have been banned from {casino}.", &[("result", &result), ("casino", &s.casino_name)]) } else { result });
            // A win: its celebration, as long as the prize is big; the result
            // stays up at least as long.
            let party = rules::celebration(multiple);
            if party > 0.0 {
                s.celebrating = Some(now + party);
                info!("slots seat: celebration {party:.2}s ({multiple} x)");
                s.win_hold = Some(now + win_hold(party));
                info!("slots: the next spin waits {:.1}s for the win's sequence", win_hold(party));
            }
            spin_out.write(SlotsSpin { machine: s.machine, phase: SpinPhase::Result { multiple, back, celebrate: party } });
            // The bet and Exit come back after the result's own time; a win holds only the
            // next spin ([`win_hold`]). Banned, the menu closes after the celebration.
            let shown = if s.banned { RESULT_SECONDS.max(party + CELEBRATION_MARGIN) } else { RESULT_SECONDS };
            s.phase = Phase::Paid { until: now + shown };
        }
        Phase::Paid { until } if now >= until => {
            if s.banned {
                info!("slots: closed (banned at {})", s.casino_name);
                next.set(PlayState::Playing);
                return;
            }
            s.phase = Phase::Betting;
            s.bet = s.limits.clamp(s.bet, s.chips);
            if s.chips == 0 {
                info!("slots: closed (no chips left)");
                next.set(PlayState::Playing);
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn close(
    mut screen: ResMut<SlotsScreen>,
    script: Option<ResMut<ScriptState>>,
    mut slots_seat: ResMut<SlotsSeat>,
    seat: Res<crate::furniture::PlayerSeat>,
    mut events: MessageWriter<SlotsSpin>,
    time: AidTime,
) {
    let Some(s) = screen.0.take() else { return };
    events.write(SlotsSpin { machine: s.machine, phase: SpinPhase::Closed });
    // Seated: a moment on the stool, then the getting-off clip (nvforever `slots_seat.rs`
    // gets the player up); standing, free at once.
    match s.stance {
        Some(Stance::Seated { stool }) if seat.on == Some(stool) => {
            *slots_seat = SlotsSeat { phase: SeatPhase::Ending, since: time.elapsed_secs(), ..*slots_seat };
            info!("slots seat: the menu closed; the player stays on stool {stool:08X} a moment");
        }
        _ => {
            if slots_seat.busy() {
                info!("slots seat: the player leaves the machine");
            }
            *slots_seat = SlotsSeat::default();
        }
    }
    if let Some(rt) = script.and_then(|s| s.into_inner().rt.as_mut()) {
        if rt.game.menu == Some(MENU_SLOTS) {
            rt.game.close_menu();
        }
    }
}

/// Getting up from the stool: free once off it (or after a while, whatever happened); a
/// sit that never got the menu open (the player got up, or went elsewhere) lets go too.
fn follow_seat(time: AidTime, mut slots_seat: ResMut<SlotsSeat>, seat: Res<crate::furniture::PlayerSeat>, screen: Res<SlotsScreen>) {
    let now = time.elapsed_secs();
    let stool = match slots_seat.stance {
        Some(Stance::Seated { stool }) => Some(stool),
        _ => None,
    };
    match slots_seat.phase {
        SeatPhase::Leaving if seat.on != stool || now - slots_seat.since > seat::SIT_WAIT => {
            info!("slots seat: the player is up");
            *slots_seat = SlotsSeat::default();
        }
        SeatPhase::Sitting if screen.0.is_none() && now - slots_seat.since > seat::SIT_WAIT + REQUEST_WAIT => {
            *slots_seat = SlotsSeat::default();
        }
        _ => {}
    }
}

fn report(mut exit: MessageReader<AppExit>, stats: Res<SlotsStats>) {
    if exit.read().next().is_none() || stats.asked == 0 {
        return;
    }
    let s = &*stats;
    info!("slots summary: {} asked, {} opened ({} refused); {} spins, {} won, {} chips won, {} lost; {} bans", s.asked, s.opened, s.refused, s.spins, s.wins, s.won, s.lost, s.bans);
}

/// [design, #2403] Synthetic logical/ineffective states for detector regressions.
#[cfg(feature = "report-test-support")]
#[doc(hidden)]
pub fn report_progress_fixture(step: usize) -> SlotsScreen {
    SlotsScreen(Some(Session {
        casino: FormId(42),
        casino_name: String::new(),
        chip: FormId(43),
        symbols: vec![],
        reel: vec![0, 1],
        max_winnings: 1000,
        limits: Limits::new(1, 10),
        bet: if step >= 1 { 2 } else { 1 },
        stops: if step >= 2 { [1; REELS] } else { [0; REELS] },
        phase: if step >= 3 { Phase::Paid { until: step as f32 } } else { Phase::Betting },
        chips: 100,
        earnings: 0,
        status: None,
        help: None,
        payouts: step >= 4,
        banned: false,
        forced: None,
        increases: step,
        rng: ChaCha8Rng::seed_from_u64(step as u64),
        machine: None,
        stance: None,
        celebrating: None,
        win_hold: None,
        spins: 0,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_aid_reads_its_steps() {
        let s = parse_aid("1:chips:200,2:open,3:bet:5,4:reels:6-6-6,5:spin,6:max,7:payout,8:exit,9:open:00140521,10:chips:9:140521").unwrap();
        assert_eq!(s[0], (1.0, AidStep::Chips(200, THE_TOPS)));
        assert_eq!(s[1], (2.0, AidStep::Open(THE_TOPS, None)));
        assert_eq!(s[2], (3.0, AidStep::Input(SlotsInput::Bet(5))));
        assert_eq!(s[3], (4.0, AidStep::Input(SlotsInput::Line([5, 5, 5]))));
        assert_eq!(s[5], (6.0, AidStep::Input(SlotsInput::Max)));
        assert_eq!(s[8], (9.0, AidStep::Open(0x0014_0521, None)));
        assert_eq!(parse_aid("1:open:144C3F:16546A").unwrap()[0], (1.0, AidStep::Open(THE_TOPS, Some(0x0016_546A))));
        assert!(parse_aid("1:open:144C3F:zz").is_err());
        assert_eq!(s[9], (10.0, AidStep::Chips(9, 0x0014_0521)));
        assert!(parse_aid("1:reels:8-1-1").is_err());
        assert!(parse_aid("1:reels:1-1").is_err());
        assert!(parse_aid("1:pull").is_err());
    }

    #[test]
    fn slot_models_by_name() {
        assert!(slot_model("clutter\\casino\\Slot_Machine01TOPS.NIF"));
        assert!(slot_model("Clutter\\Casino\\Slot_Machine02.NIF"));
        assert!(!slot_model("clutter\\casino\\SlotMachine01BrokenA.nif"));
        assert!(!slot_model("clutter\\casino\\slot_machine01static.nif"));
        assert!(!slot_model("clutter\\casino\\roulettetable01.nif"));
    }

    #[test]
    fn reels_turn_to_their_stops() {
        let from = [0, 5, 13];
        let to = [3, 5, 2];
        assert_eq!(reel_positions(14, from, to, 0.0), [0.0, 5.0, 13.0]);
        let end = reel_positions(14, from, to, 10.0);
        for i in 0..REELS {
            assert_eq!((end[i].round() as usize) % 14, to[i]);
        }
        // Each reel stops at its own time.
        let mid = reel_positions(14, from, to, REEL_STOPS[0]);
        assert_eq!(mid[0], end[0]);
        assert!(mid[2] < end[2]);
        // Still until the clip's frame 15, then turning.
        assert_eq!(reel_positions(14, from, to, REEL_START), [0.0, 5.0, 13.0]);
        assert!(reel_positions(14, from, to, REEL_START + 0.1)[0] > 0.0);
    }

    #[test]
    fn reels_stop_on_the_pull_sounds_clunks() {
        // The reel models' frames 71, 99 and 127, and the pull sound's clunks at about
        // 2.37, 3.31 and 4.25 s, within a frame and a half.
        for (stop, clunk) in REEL_STOPS.iter().zip([2.375, 3.315, 4.255]) {
            assert!((stop - clunk).abs() < 0.05, "{stop} vs {clunk}");
        }
    }

    #[test]
    fn reels_turn_evenly_then_bounce_on_the_stop() {
        let (from, to) = ([0, 0, 0], [0, 0, 0]);
        let at = |t: f32| reel_positions(14, from, to, t);
        // Halfway through its turn, the first reel has made half its two turns.
        let half = (REEL_START + REEL_STOPS[0]) / 2.0;
        assert!((at(half)[0] - 14.0).abs() < 1e-3);
        // All three turn at one speed.
        let speed = |i: usize| (REEL_TURNS[i] * 14) as f32 / (REEL_STOPS[i] - REEL_START);
        assert!((speed(0) - speed(2)).abs() < 0.05);
        // 5 degrees past the stop two frames on, back on it two frames later.
        let peak = at(REEL_STOPS[0] + REEL_BOUNCE_SECONDS)[0] - 28.0;
        assert!((peak - 5.0 / 360.0 * 14.0).abs() < 1e-3);
        assert!((at(REEL_STOPS[0] + 2.0 * REEL_BOUNCE_SECONDS + 1e-3)[0] - 28.0).abs() < 1e-4);
        // The clip ends as the right reel is back on its stop.
        assert!((REEL_STOPS[2] + 2.0 * REEL_BOUNCE_SECONDS - REEL_CLIP_END).abs() < 1e-5);
    }

    #[test]
    fn a_win_holds_the_next_spin_through_its_sequence() {
        // The zoom out, the celebration (3 s at 160 x, 0.8 s at 2 x) and the way back in.
        assert!((win_hold(3.0) - 5.6).abs() < 1e-5);
        assert!((win_hold(fnv_game::slots::celebration(2)) - 3.4).abs() < 1e-5);
        assert_eq!(win_hold(0.0), WIN_CAMERA_OUT + WIN_CAMERA_BACK);
    }

    #[test]
    fn win_sounds_by_size() {
        assert_eq!(win_sound(0), sounds::LOSE);
        assert_eq!(win_sound(2), sounds::WIN_SMALL);
        assert_eq!(win_sound(10), sounds::WIN_MED);
        assert_eq!(win_sound(160), sounds::WIN_JACKPOT);
    }
}
